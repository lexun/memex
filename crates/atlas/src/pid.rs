use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidInfo {
    pub pid: u32,
    pub version: String,
    pub socket: String,
    pub started_at: DateTime<Utc>,
}

impl PidInfo {
    pub fn new(pid: u32, socket: &Path) -> Self {
        Self {
            pid,
            version: env!("CARGO_PKG_VERSION").to_string(),
            socket: socket.display().to_string(),
            started_at: Utc::now(),
        }
    }
}

pub fn write_pid_file(path: &Path, info: &PidInfo) -> Result<()> {
    let contents = serde_json::to_string_pretty(info)?;

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(path, contents)
        .with_context(|| format!("Failed to write PID file: {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}

pub fn read_pid_file(path: &Path) -> Result<Option<PidInfo>> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read PID file: {}", path.display()))?;

    let info: PidInfo = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse PID file: {}", path.display()))?;

    Ok(Some(info))
}

pub fn remove_pid_file(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("Failed to remove PID file: {}", path.display()))?;
    }
    Ok(())
}

pub fn is_process_running(pid: u32) -> bool {
    let pid = Pid::from_raw(pid as i32);
    kill(pid, None).is_ok()
}

pub fn check_daemon(pid_path: &Path) -> Result<Option<PidInfo>> {
    let info = match read_pid_file(pid_path)? {
        Some(info) => info,
        None => return Ok(None),
    };

    if is_process_running(info.pid) {
        Ok(Some(info))
    } else {
        tracing::debug!("Removing stale PID file for non-existent process {}", info.pid);
        remove_pid_file(pid_path)?;
        Ok(None)
    }
}

pub fn send_sigterm(pid: u32) -> Result<()> {
    let pid = Pid::from_raw(pid as i32);
    kill(pid, Signal::SIGTERM)
        .with_context(|| format!("Failed to send SIGTERM to process {}", pid))
}
