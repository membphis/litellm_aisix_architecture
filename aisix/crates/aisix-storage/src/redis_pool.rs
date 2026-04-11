use std::time::Duration;

use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

#[derive(Debug, Clone)]
pub struct RedisPool {
    endpoint: RedisEndpoint,
    connect_timeout: Duration,
}

#[derive(Debug, Clone)]
struct RedisEndpoint {
    host: String,
    port: u16,
}

#[derive(Debug, Error)]
pub enum RedisError {
    #[error("invalid redis url: {0}")]
    InvalidUrl(String),
    #[error("redis unavailable: {0}")]
    Unavailable(String),
    #[error("redis timeout")]
    Timeout,
    #[error("redis io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("redis protocol error: {0}")]
    Protocol(String),
}

impl RedisPool {
    pub fn from_url(url: &str) -> Result<Self, RedisError> {
        let endpoint = RedisEndpoint::parse(url)?;
        Ok(Self {
            endpoint,
            connect_timeout: Duration::from_millis(250),
        })
    }

    pub async fn incr(&self, key: &str, ttl_seconds: u64) -> Result<u64, RedisError> {
        let mut connection = self.connect().await?;
        let value = connection.integer_command(&["INCR", key]).await?;
        if value == 1 {
            let ttl = ttl_seconds.to_string();
            let _ = connection
                .integer_command(&["EXPIRE", key, ttl.as_str()])
                .await?;
        }

        Ok(value as u64)
    }

    pub async fn incr_by(&self, key: &str, amount: u64) -> Result<u64, RedisError> {
        let mut connection = self.connect().await?;
        let amount = amount.to_string();
        let value = connection
            .integer_command(&["INCRBY", key, amount.as_str()])
            .await?;
        Ok(value as u64)
    }

    async fn connect(&self) -> Result<RedisConnection, RedisError> {
        let address = format!("{}:{}", self.endpoint.host, self.endpoint.port);
        let stream = timeout(self.connect_timeout, TcpStream::connect(address))
            .await
            .map_err(|_| RedisError::Timeout)??;

        Ok(RedisConnection {
            stream: BufReader::new(stream),
        })
    }
}

impl RedisEndpoint {
    fn parse(url: &str) -> Result<Self, RedisError> {
        let remainder = url
            .strip_prefix("redis://")
            .ok_or_else(|| RedisError::InvalidUrl(url.to_string()))?;
        let authority = remainder.split('/').next().unwrap_or(remainder);
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let (host, port) = authority
            .rsplit_once(':')
            .ok_or_else(|| RedisError::InvalidUrl(url.to_string()))?;
        let port = port
            .parse::<u16>()
            .map_err(|_| RedisError::InvalidUrl(url.to_string()))?;
        if host.is_empty() {
            return Err(RedisError::InvalidUrl(url.to_string()));
        }

        Ok(Self {
            host: host.to_string(),
            port,
        })
    }
}

struct RedisConnection {
    stream: BufReader<TcpStream>,
}

impl RedisConnection {
    async fn integer_command(&mut self, parts: &[&str]) -> Result<i64, RedisError> {
        let command = encode_command(parts);
        self.stream.get_mut().write_all(&command).await?;
        self.stream.get_mut().flush().await?;

        let mut line = Vec::new();
        self.stream.read_until(b'\n', &mut line).await?;
        if line.len() < 3 || !line.ends_with(b"\r\n") {
            return Err(RedisError::Protocol("truncated response".to_string()));
        }

        match line[0] {
            b':' => parse_integer(&line[1..line.len() - 2]),
            b'-' => Err(RedisError::Unavailable(
                String::from_utf8_lossy(&line[1..line.len() - 2]).into_owned(),
            )),
            other => Err(RedisError::Protocol(format!(
                "unexpected response prefix: {}",
                other as char
            ))),
        }
    }
}

fn encode_command(parts: &[&str]) -> Vec<u8> {
    let mut buffer = Vec::new();
    buffer.extend_from_slice(format!("*{}\r\n", parts.len()).as_bytes());
    for part in parts {
        buffer.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
        buffer.extend_from_slice(part.as_bytes());
        buffer.extend_from_slice(b"\r\n");
    }
    buffer
}

fn parse_integer(raw: &[u8]) -> Result<i64, RedisError> {
    let value = std::str::from_utf8(raw)
        .map_err(|error| RedisError::Protocol(format!("invalid integer utf8: {error}")))?;
    value
        .parse::<i64>()
        .map_err(|error| RedisError::Protocol(format!("invalid integer response: {error}")))
}
