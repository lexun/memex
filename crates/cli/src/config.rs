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
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: None,
            pid_file: None,
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
