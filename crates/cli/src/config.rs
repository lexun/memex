use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_NAME: &str = "memex";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub web: WebConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub socket_path: Option<PathBuf>,
    pub pid_file: Option<PathBuf>,
    /// Worker health monitor configuration
    #[serde(default)]
    pub health_monitor: HealthMonitorConfig,
    /// Background curation monitor configuration
    #[serde(default)]
    pub curation_monitor: CurationMonitorConfig,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: None,
            pid_file: None,
            health_monitor: HealthMonitorConfig::default(),
            curation_monitor: CurationMonitorConfig::default(),
        }
    }
}

/// Configuration for the worker health monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMonitorConfig {
    /// Enable the health monitor background task (default: true)
    #[serde(default = "default_health_enabled")]
    pub enabled: bool,
    /// How often to check worker health, in seconds (default: 30)
    #[serde(default = "default_health_check_interval")]
    pub check_interval_secs: u64,
    /// Worker inactivity threshold before alerting coordinator, in seconds (default: 300 = 5 minutes)
    #[serde(default = "default_health_inactivity_threshold")]
    pub inactivity_threshold_secs: u64,
}

fn default_health_enabled() -> bool {
    true
}

fn default_health_check_interval() -> u64 {
    30 // 30 seconds
}

fn default_health_inactivity_threshold() -> u64 {
    300 // 5 minutes
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: default_health_enabled(),
            check_interval_secs: default_health_check_interval(),
            inactivity_threshold_secs: default_health_inactivity_threshold(),
        }
    }
}

/// Configuration for the background curation monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationMonitorConfig {
    /// Enable the curation monitor background task (default: true)
    #[serde(default = "default_curation_enabled")]
    pub enabled: bool,
    /// How often to check for unprocessed memos, in seconds (default: 60)
    #[serde(default = "default_curation_check_interval")]
    pub check_interval_secs: u64,
    /// Maximum number of concurrent curation workers (default: 2)
    #[serde(default = "default_curation_max_workers")]
    pub max_concurrent_workers: usize,
    /// Batch size for fetching unprocessed memos (default: 10)
    #[serde(default = "default_curation_batch_size")]
    pub batch_size: usize,
}

fn default_curation_enabled() -> bool {
    true
}

fn default_curation_check_interval() -> u64 {
    60 // 1 minute
}

fn default_curation_max_workers() -> usize {
    2
}

fn default_curation_batch_size() -> usize {
    10
}

impl Default for CurationMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: default_curation_enabled(),
            check_interval_secs: default_curation_check_interval(),
            max_concurrent_workers: default_curation_max_workers(),
            batch_size: default_curation_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// Enable the web UI server (default: true)
    #[serde(default = "default_web_enabled")]
    pub enabled: bool,
    /// Port for the web UI server (default: 3030)
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// Host to bind to (default: 127.0.0.1)
    #[serde(default = "default_web_host")]
    pub host: String,
    /// Path to static files for the Leptos web UI (default: target/site)
    #[serde(default)]
    pub static_path: Option<String>,
}

fn default_web_enabled() -> bool {
    true
}

fn default_web_port() -> u16 {
    3030
}

fn default_web_host() -> String {
    "127.0.0.1".to_string()
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: default_web_enabled(),
            port: default_web_port(),
            host: default_web_host(),
            static_path: None,
        }
    }
}

/// Database configuration - re-exported from db crate
pub use db::DatabaseConfig;

/// LLM provider configuration - re-exported from llm crate
pub use llm::LlmConfig;

pub fn get_config_dir() -> Result<PathBuf> {
    // MEMEX_CONFIG_PATH overrides the default config directory
    if let Ok(path) = std::env::var("MEMEX_CONFIG_PATH") {
        return Ok(PathBuf::from(path));
    }

    // Use XDG-style config directory (~/.config/memex)
    let home = std::env::var("HOME")
        .context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config").join(APP_NAME))
}

pub fn get_config_file() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("config.toml"))
}

pub fn get_socket_path(config: &Config) -> Result<PathBuf> {
    if let Some(path) = &config.daemon.socket_path {
        return Ok(path.clone());
    }
    Ok(get_config_dir()?.join("memex.sock"))
}

pub fn get_pid_file(config: &Config) -> Result<PathBuf> {
    if let Some(path) = &config.daemon.pid_file {
        return Ok(path.clone());
    }
    Ok(get_config_dir()?.join("memex.pid"))
}

pub fn get_db_path(config: &Config) -> Result<PathBuf> {
    if let Some(path) = &config.database.path {
        return Ok(path.clone());
    }
    Ok(get_config_dir()?.join("db"))
}

pub fn load_config() -> Result<Config> {
    let config_file = get_config_file()?;

    if !config_file.exists() {
        return Ok(Config::default());
    }

    let contents = fs::read_to_string(&config_file)
        .with_context(|| format!("Failed to read config file: {}", config_file.display()))?;

    toml::from_str(&contents)
        .with_context(|| format!("Failed to parse config file: {}", config_file.display()))
}

pub fn save_config(config: &Config) -> Result<()> {
    let config_file = get_config_file()?;
    let config_dir = get_config_dir()?;

    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)
            .with_context(|| format!("Failed to create config directory: {}", config_dir.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o700);
            fs::set_permissions(&config_dir, perms)?;
        }
    }

    let contents = toml::to_string_pretty(config)?;
    fs::write(&config_file, contents)
        .with_context(|| format!("Failed to write config file: {}", config_file.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&config_file, perms)?;
    }

    Ok(())
}

pub fn get_config_value(config: &Config, key: &str) -> Option<String> {
    match key {
        "daemon.socket_path" => config.daemon.socket_path.as_ref().map(|p| p.display().to_string()),
        "daemon.pid_file" => config.daemon.pid_file.as_ref().map(|p| p.display().to_string()),
        "daemon.health_monitor.enabled" => Some(config.daemon.health_monitor.enabled.to_string()),
        "daemon.health_monitor.check_interval_secs" => Some(config.daemon.health_monitor.check_interval_secs.to_string()),
        "daemon.health_monitor.inactivity_threshold_secs" => Some(config.daemon.health_monitor.inactivity_threshold_secs.to_string()),
        "daemon.curation_monitor.enabled" => Some(config.daemon.curation_monitor.enabled.to_string()),
        "daemon.curation_monitor.check_interval_secs" => Some(config.daemon.curation_monitor.check_interval_secs.to_string()),
        "daemon.curation_monitor.max_concurrent_workers" => Some(config.daemon.curation_monitor.max_concurrent_workers.to_string()),
        "daemon.curation_monitor.batch_size" => Some(config.daemon.curation_monitor.batch_size.to_string()),
        "database.path" => config.database.path.as_ref().map(|p| p.display().to_string()),
        "database.url" => config.database.url.clone(),
        "database.namespace" => config.database.namespace.clone(),
        "database.username" => config.database.username.clone(),
        "database.password" => Some("********".to_string()), // Don't expose password
        "llm.provider" => Some(config.llm.provider.clone()),
        "llm.model" => Some(config.llm.model.clone()),
        "llm.embedding_model" => Some(config.llm.embedding_model.clone()),
        "llm.api_key" => Some("********".to_string()), // Don't expose API key
        "llm.base_url" => config.llm.base_url.clone(),
        "web.enabled" => Some(config.web.enabled.to_string()),
        "web.port" => Some(config.web.port.to_string()),
        "web.host" => Some(config.web.host.clone()),
        _ => None,
    }
}

pub fn set_config_value(config: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "daemon.socket_path" => config.daemon.socket_path = Some(PathBuf::from(value)),
        "daemon.pid_file" => config.daemon.pid_file = Some(PathBuf::from(value)),
        "daemon.health_monitor.enabled" => config.daemon.health_monitor.enabled = value.parse()
            .with_context(|| format!("Invalid boolean value: {}", value))?,
        "daemon.health_monitor.check_interval_secs" => config.daemon.health_monitor.check_interval_secs = value.parse()
            .with_context(|| format!("Invalid integer value: {}", value))?,
        "daemon.health_monitor.inactivity_threshold_secs" => config.daemon.health_monitor.inactivity_threshold_secs = value.parse()
            .with_context(|| format!("Invalid integer value: {}", value))?,
        "daemon.curation_monitor.enabled" => config.daemon.curation_monitor.enabled = value.parse()
            .with_context(|| format!("Invalid boolean value: {}", value))?,
        "daemon.curation_monitor.check_interval_secs" => config.daemon.curation_monitor.check_interval_secs = value.parse()
            .with_context(|| format!("Invalid integer value: {}", value))?,
        "daemon.curation_monitor.max_concurrent_workers" => config.daemon.curation_monitor.max_concurrent_workers = value.parse()
            .with_context(|| format!("Invalid integer value: {}", value))?,
        "daemon.curation_monitor.batch_size" => config.daemon.curation_monitor.batch_size = value.parse()
            .with_context(|| format!("Invalid integer value: {}", value))?,
        "database.path" => config.database.path = Some(PathBuf::from(value)),
        "database.url" => config.database.url = Some(value.to_string()),
        "database.namespace" => config.database.namespace = Some(value.to_string()),
        "database.username" => config.database.username = Some(value.to_string()),
        "database.password" => config.database.password = Some(value.to_string()),
        "llm.provider" => config.llm.provider = value.to_string(),
        "llm.model" => config.llm.model = value.to_string(),
        "llm.embedding_model" => config.llm.embedding_model = value.to_string(),
        "llm.api_key" => config.llm.api_key = Some(value.to_string()),
        "llm.base_url" => config.llm.base_url = Some(value.to_string()),
        _ => anyhow::bail!("Unknown config key: {}", key),
    }
    Ok(())
}
