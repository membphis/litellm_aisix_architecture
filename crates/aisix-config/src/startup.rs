use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StartupConfig {
    pub server: ServerConfig,
    pub etcd: EtcdConfig,
    pub redis: RedisConfig,
    pub log: LogConfig,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    pub deployment: DeploymentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub default: CacheDefaultMode,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            default: CacheDefaultMode::Disabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum CacheDefaultMode {
    #[serde(rename = "enabled")]
    Enabled,
    #[default]
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerConfig {
    pub listen: String,
    pub admin_listen: String,
    pub metrics_listen: String,
    pub request_body_limit_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EtcdConfig {
    pub endpoints: Vec<String>,
    pub prefix: String,
    pub dial_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LogConfig {
    pub level: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RuntimeConfig {
    pub worker_threads: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeploymentConfig {
    pub admin: AdminConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminConfig {
    pub enabled: bool,
    #[serde(default)]
    pub admin_keys: Vec<AdminKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminKey {
    pub key: String,
}

pub fn load_from_path(path: &str) -> Result<StartupConfig> {
    let contents = std::fs::read_to_string(path)?;
    let config = serde_yaml::from_str(&contents)?;
    Ok(config)
}
