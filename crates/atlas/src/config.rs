use anyhow::{Context, Result};
use directories::ProjectDirs;
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
pub struct DatabaseConfig {
    pub path: Option<PathBuf>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self { path: None }
    }
}

pub fn get_config_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("MEMEX_CONFIG_PATH") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return Ok(path);
        } else if let Some(parent) = path.parent() {
            return Ok(parent.to_path_buf());
        }
    }

    ProjectDirs::from("", "", APP_NAME)
        .map(|dirs| dirs.config_dir().to_path_buf())
        .context("Could not determine config directory")
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
        _ => None,
    }
}

pub fn set_config_value(config: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "daemon.socket_path" => config.daemon.socket_path = Some(PathBuf::from(value)),
        "daemon.pid_file" => config.daemon.pid_file = Some(PathBuf::from(value)),
        "database.path" => config.database.path = Some(PathBuf::from(value)),
        _ => anyhow::bail!("Unknown config key: {}", key),
    }
    Ok(())
}
