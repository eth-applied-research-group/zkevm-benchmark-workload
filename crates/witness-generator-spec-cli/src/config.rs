use std::{env, fs, path::PathBuf, time::Duration};

use anyhow::{Context, ensure};
use serde::Deserialize;

const DEFAULT_OUT_ROOT: &str = "/var/lib/stateless-inputs";
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(4);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_BATCH_SIZE: u64 = 500;

#[derive(Debug, Clone)]
pub(crate) struct CollectorConfig {
    pub(crate) network: String,
    pub(crate) cl_url: String,
    pub(crate) el_url: String,
    pub(crate) out_root: PathBuf,
    pub(crate) poll_interval: Duration,
    pub(crate) request_timeout: Duration,
    pub(crate) batch_size: u64,
    pub(crate) r2: Option<R2PublishConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct R2PublishConfig {
    pub(crate) bucket: String,
    #[serde(default)]
    pub(crate) prefix: String,
    pub(crate) account_id: String,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    network: String,
    cl_url: Option<String>,
    el_url: Option<String>,
    out_root: Option<PathBuf>,
    poll_interval: Option<String>,
    request_timeout: Option<String>,
    batch_size: Option<u64>,
    r2: Option<R2PublishConfig>,
}

impl CollectorConfig {
    pub(crate) fn from_path(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        Self::from_toml_str(&contents)
            .with_context(|| format!("failed to parse config {}", path.display()))
    }

    pub(crate) fn from_toml_str(contents: &str) -> anyhow::Result<Self> {
        let file: ConfigFile = toml::from_str(contents)?;
        ensure!(!file.network.trim().is_empty(), "network must not be empty");

        let cl_url = endpoint_from_config_or_env(file.cl_url, "CL_RPC_URL")
            .context("cl_url is required in config or CL_RPC_URL")?;
        let el_url = endpoint_from_config_or_env(file.el_url, "EL_RPC_URL")
            .context("el_url is required in config or EL_RPC_URL")?;

        let poll_interval = parse_duration_or_default(
            file.poll_interval.as_deref(),
            DEFAULT_POLL_INTERVAL,
            "poll_interval",
        )?;
        let request_timeout = parse_duration_or_default(
            file.request_timeout.as_deref(),
            DEFAULT_REQUEST_TIMEOUT,
            "request_timeout",
        )?;
        let batch_size = file.batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
        ensure!(batch_size > 0, "batch_size must be greater than zero");

        Ok(Self {
            network: file.network,
            cl_url,
            el_url,
            out_root: file
                .out_root
                .unwrap_or_else(|| PathBuf::from(DEFAULT_OUT_ROOT)),
            poll_interval,
            request_timeout,
            batch_size,
            r2: file.r2.map(R2PublishConfig::normalize).transpose()?,
        })
    }

    pub(crate) fn network_root(&self) -> PathBuf {
        self.out_root.join(&self.network)
    }

    pub(crate) fn blocks_root(&self) -> PathBuf {
        self.network_root().join("blocks")
    }

    pub(crate) fn exports_root(&self) -> PathBuf {
        self.network_root().join("exports")
    }

    pub(crate) fn batches_root(&self) -> PathBuf {
        self.exports_root().join("batches")
    }

    pub(crate) fn catalog_cache_path(&self) -> PathBuf {
        self.exports_root().join("catalog-cache.json")
    }

    pub(crate) fn index_path(&self) -> PathBuf {
        self.network_root().join("index.jsonl")
    }

    pub(crate) fn state_path(&self) -> PathBuf {
        self.network_root().join("state.json")
    }
}

impl R2PublishConfig {
    fn normalize(mut self) -> anyhow::Result<Self> {
        ensure!(
            !self.bucket.trim().is_empty(),
            "r2.bucket must not be empty"
        );
        ensure!(
            !self.account_id.trim().is_empty(),
            "r2.account_id must not be empty"
        );

        self.bucket = self.bucket.trim().to_owned();
        self.prefix = self.prefix.trim_matches('/').to_owned();
        self.account_id = self.account_id.trim().to_owned();
        Ok(self)
    }

    pub(crate) fn endpoint_url(&self) -> String {
        format!("https://{}.r2.cloudflarestorage.com", self.account_id)
    }
}

fn endpoint_from_config_or_env(file_value: Option<String>, env_name: &str) -> Option<String> {
    file_value
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var(env_name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn parse_duration_or_default(
    value: Option<&str>,
    default: Duration,
    label: &str,
) -> anyhow::Result<Duration> {
    let Some(value) = value else {
        return Ok(default);
    };
    let duration = humantime::parse_duration(value)
        .with_context(|| format!("failed to parse {label} duration `{value}`"))?;
    ensure!(!duration.is_zero(), "{label} must be greater than zero",);
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_and_r2_config() {
        let config = CollectorConfig::from_toml_str(
            r#"
network = "glamsterdam-devnet-5"
cl_url = "http://cl"
el_url = "http://el"

[r2]
bucket = "stateless-inputs"
prefix = "/devnets/"
account_id = "abc123"
"#,
        )
        .unwrap();

        assert_eq!(config.network, "glamsterdam-devnet-5");
        assert_eq!(config.out_root, PathBuf::from(DEFAULT_OUT_ROOT));
        assert_eq!(config.poll_interval, DEFAULT_POLL_INTERVAL);
        assert_eq!(config.batch_size, DEFAULT_BATCH_SIZE);
        let r2 = config.r2.unwrap();
        assert_eq!(r2.bucket, "stateless-inputs");
        assert_eq!(r2.prefix, "devnets");
        assert_eq!(r2.account_id, "abc123");
        assert_eq!(r2.endpoint_url(), "https://abc123.r2.cloudflarestorage.com");
    }

    #[test]
    fn parses_custom_duration_values() {
        let config = CollectorConfig::from_toml_str(
            r#"
network = "glamsterdam-devnet-5"
cl_url = "http://cl"
el_url = "http://el"
out_root = "/tmp/stateless"
poll_interval = "10s"
request_timeout = "45s"
batch_size = 100
"#,
        )
        .unwrap();

        assert_eq!(config.out_root, PathBuf::from("/tmp/stateless"));
        assert_eq!(config.poll_interval, Duration::from_secs(10));
        assert_eq!(config.request_timeout, Duration::from_secs(45));
        assert_eq!(config.batch_size, 100);
    }

    #[test]
    fn endpoint_resolution_prefers_config_over_env() {
        let from_config =
            endpoint_from_config_or_env(Some("http://from-config".to_owned()), "CL_RPC_URL");

        assert_eq!(from_config.as_deref(), Some("http://from-config"));
    }
}
