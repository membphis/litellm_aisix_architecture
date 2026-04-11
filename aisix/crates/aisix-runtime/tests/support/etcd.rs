use std::{
    net::TcpListener,
    process::Command,
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aisix_config::startup::EtcdConfig;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;

pub struct EtcdHarness {
    _guard: MutexGuard<'static, ()>,
    container_name: String,
    endpoint: String,
}

impl EtcdHarness {
    pub async fn start() -> Result<Self> {
        let guard = harness_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let container_name = format!(
            "aisix-runtime-etcd-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        );
        let host_port = reserve_host_port()?;
        let endpoint = format!("127.0.0.1:{host_port}");
        let advertised = format!("http://{endpoint}");

        run_docker(&[
            "run",
            "--detach",
            "--rm",
            "--name",
            &container_name,
            "-p",
            &format!("127.0.0.1:{host_port}:2379"),
            "quay.io/coreos/etcd:v3.5.17",
            "etcd",
            "--name=test-etcd",
            "--listen-client-urls=http://0.0.0.0:2379",
            &format!("--advertise-client-urls={advertised}"),
            "--listen-peer-urls=http://0.0.0.0:2380",
        ])
        .with_context(|| format!("failed to start etcd container {container_name}"))?;

        let harness = Self {
            _guard: guard,
            container_name,
            endpoint,
        };
        harness.wait_until_ready().await?;
        Ok(harness)
    }

    pub fn config(&self) -> EtcdConfig {
        EtcdConfig {
            endpoints: vec![self.endpoint.clone()],
            prefix: "/aisix".to_string(),
            dial_timeout_ms: 1_000,
        }
    }

    pub async fn put_json<T: Serialize>(&self, key: &str, value: &T) -> Result<i64> {
        let body = serde_json::to_vec(value).context("failed to serialize etcd fixture")?;
        let mut client = self.connect_client().await?;
        let response = client
            .put(key, body, None)
            .await
            .context("failed to put etcd fixture")?;

        Ok(response
            .header()
            .map(|header| header.revision())
            .unwrap_or(0))
    }

    pub fn pause(&self) -> Result<()> {
        run_docker(&["pause", &self.container_name])
            .with_context(|| format!("failed to pause etcd container {}", self.container_name))?;
        Ok(())
    }

    pub fn unpause(&self) -> Result<()> {
        run_docker(&["unpause", &self.container_name])
            .with_context(|| format!("failed to unpause etcd container {}", self.container_name))?;
        Ok(())
    }

    async fn wait_until_ready(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut last_error = None;
        while tokio::time::Instant::now() < deadline {
            match self.connect_client().await {
                Ok(mut client) => match client.get("/__ready__", None).await {
                    Ok(_) => return Ok(()),
                    Err(error) => last_error = Some(anyhow!(error)),
                },
                Err(error) => last_error = Some(error),
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(last_error.unwrap_or_else(|| anyhow!("timed out waiting for etcd")))
    }

    async fn connect_client(&self) -> Result<etcd_client::Client> {
        let options = etcd_client::ConnectOptions::default()
            .with_connect_timeout(Duration::from_secs(1))
            .with_timeout(Duration::from_secs(1));

        etcd_client::Client::connect([self.endpoint.as_str()], Some(options))
            .await
            .with_context(|| format!("failed to connect to test etcd at {}", self.endpoint))
    }
}

impl Drop for EtcdHarness {
    fn drop(&mut self) {
        let _ = run_docker(&["rm", "--force", &self.container_name]);
    }
}

fn harness_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn reserve_host_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to reserve etcd test port")?;
    let port = listener
        .local_addr()
        .context("failed to inspect reserved etcd test port")?
        .port();
    drop(listener);
    Ok(port)
}

fn run_docker(args: &[&str]) -> Result<String> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute docker {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "docker {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
