//! Linux systemd integration for automatic daemon startup
//!
//! This module provides enable/disable functionality for the memex daemon
//! using systemd user services. When enabled, the daemon will automatically
//! start on login and restart if it crashes.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use crate::config::get_config_dir;

const SERVICE_NAME: &str = "memex-daemon";

/// Get the path to the systemd user directory
fn systemd_user_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("systemd")
        .join("user"))
}

/// Get the path to the service unit file
pub fn service_path() -> Result<PathBuf> {
    Ok(systemd_user_dir()?.join(format!("{}.service", SERVICE_NAME)))
}

/// Get the path to the daemon log file (via journalctl)
pub fn log_path() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("daemon.log"))
}

/// Get the path to the daemon error log file
pub fn error_log_path() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("daemon.error.log"))
}

/// Resolve the memex binary path using `which`
fn resolve_memex_binary() -> Result<PathBuf> {
    let output = Command::new("which")
        .arg("memex")
        .output()
        .context("Failed to run 'which memex'")?;

    if !output.status.success() {
        anyhow::bail!(
            "Could not find memex binary in PATH. Ensure memex is installed and in your PATH."
        );
    }

    let path_str = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in which output")?
        .trim()
        .to_string();

    Ok(PathBuf::from(path_str))
}

/// Generate the systemd service unit content
fn generate_service_unit() -> Result<String> {
    let binary_path = resolve_memex_binary()?;
    let config_dir = get_config_dir()?;
    let stdout_log = log_path()?;
    let stderr_log = error_log_path()?;

    // Ensure config directory exists
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).with_context(|| {
            format!(
                "Failed to create config directory: {}",
                config_dir.display()
            )
        })?;
    }

    let unit = format!(
        r#"[Unit]
Description=Memex Daemon
Documentation=https://github.com/lexun/memex
After=network.target

[Service]
Type=simple
ExecStart={binary} daemon run
Environment=MEMEX_CONFIG_PATH={config_dir}
Restart=on-failure
RestartSec=5
StandardOutput=append:{stdout}
StandardError=append:{stderr}

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths={config_dir}
PrivateTmp=yes

[Install]
WantedBy=default.target
"#,
        binary = binary_path.display(),
        config_dir = config_dir.display(),
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
    );

    Ok(unit)
}

/// Check if the systemd service is currently enabled
pub fn is_enabled() -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["--user", "is-enabled", SERVICE_NAME])
        .output()
        .context("Failed to run systemctl is-enabled")?;

    Ok(output.status.success())
}

/// Check if the systemd service is currently active (running)
pub fn is_active() -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["--user", "is-active", SERVICE_NAME])
        .output()
        .context("Failed to run systemctl is-active")?;

    Ok(output.status.success())
}

/// Enable the systemd service
pub fn enable() -> Result<()> {
    let service_file = service_path()?;
    let systemd_dir = systemd_user_dir()?;

    // Ensure systemd user directory exists
    if !systemd_dir.exists() {
        fs::create_dir_all(&systemd_dir).with_context(|| {
            format!(
                "Failed to create systemd user directory: {}",
                systemd_dir.display()
            )
        })?;
    }

    // Generate and write the service unit
    let service_content = generate_service_unit()?;
    let mut file = fs::File::create(&service_file)
        .with_context(|| format!("Failed to create service file: {}", service_file.display()))?;
    file.write_all(service_content.as_bytes())?;

    println!("Created service unit: {}", service_file.display());

    // Reload systemd to pick up the new unit
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("Failed to run systemctl daemon-reload")?;

    if !status.success() {
        anyhow::bail!("systemctl daemon-reload failed");
    }

    // Enable the service
    let status = Command::new("systemctl")
        .args(["--user", "enable", SERVICE_NAME])
        .status()
        .context("Failed to run systemctl enable")?;

    if !status.success() {
        anyhow::bail!("systemctl enable failed");
    }

    println!("Enabled service: {}", SERVICE_NAME);

    // Start the service
    let status = Command::new("systemctl")
        .args(["--user", "start", SERVICE_NAME])
        .status()
        .context("Failed to run systemctl start")?;

    if !status.success() {
        eprintln!("Warning: systemctl start returned non-zero status");
        eprintln!("The service may already be running or there may be an issue.");
        eprintln!("Check status with: memex daemon status");
    } else {
        println!("Started service: {}", SERVICE_NAME);
    }

    println!();
    println!("The daemon will now start automatically on login and restart if it crashes.");

    Ok(())
}

/// Disable the systemd service
pub fn disable() -> Result<()> {
    let service_file = service_path()?;

    // Check if service file exists
    if !service_file.exists() {
        println!("Systemd service is not enabled (service file not found)");
        return Ok(());
    }

    // Stop the service first
    let _ = Command::new("systemctl")
        .args(["--user", "stop", SERVICE_NAME])
        .status();

    // Disable the service
    let status = Command::new("systemctl")
        .args(["--user", "disable", SERVICE_NAME])
        .status()
        .context("Failed to run systemctl disable")?;

    if !status.success() {
        eprintln!("Warning: systemctl disable returned non-zero status");
    }

    // Remove the service file
    fs::remove_file(&service_file)
        .with_context(|| format!("Failed to remove service file: {}", service_file.display()))?;

    println!("Removed service unit: {}", service_file.display());

    // Reload systemd
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    println!("Disabled service: {}", SERVICE_NAME);
    println!();
    println!("The daemon will no longer start automatically.");

    Ok(())
}

/// Show systemd service status
pub fn status() -> Result<()> {
    let service_file = service_path()?;
    let file_exists = service_file.exists();
    let enabled = if file_exists { is_enabled()? } else { false };
    let active = if file_exists { is_active()? } else { false };

    println!("Systemd autostart:");
    println!("  Enabled: {}", if enabled { "yes" } else { "no" });
    println!("  Active: {}", if active { "yes" } else { "no" });

    if file_exists {
        println!("  Unit file: {}", service_file.display());
    }

    if active {
        // Get more details from systemctl
        let output = Command::new("systemctl")
            .args(["--user", "show", SERVICE_NAME, "--property=MainPID,ActiveState,SubState"])
            .output()
            .context("Failed to run systemctl show")?;

        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            for line in info.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    match key {
                        "MainPID" if value != "0" => println!("  PID: {}", value),
                        "SubState" => println!("  State: {}", value),
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

/// Tail the daemon logs
pub fn tail_logs(follow: bool) -> Result<()> {
    let log_file = log_path()?;
    let error_log_file = error_log_path()?;

    // First try journalctl for systemd-managed logs
    if is_active().unwrap_or(false) {
        println!("Showing logs from journalctl (systemd):");
        println!();

        let mut args = vec!["--user", "-u", SERVICE_NAME, "-n", "50", "--no-pager"];
        if follow {
            args.push("-f");
        }

        let status = Command::new("journalctl")
            .args(&args)
            .status()
            .context("Failed to run journalctl")?;

        if status.success() {
            return Ok(());
        }
        // Fall through to file-based logs if journalctl fails
        println!();
        println!("Journalctl unavailable, falling back to log files:");
        println!();
    }

    // Check if log files exist
    if !log_file.exists() && !error_log_file.exists() {
        println!("No log files found yet.");
        println!("  Expected stdout log: {}", log_file.display());
        println!("  Expected stderr log: {}", error_log_file.display());
        println!();
        println!("The daemon may not have been started via systemd yet.");
        return Ok(());
    }

    // Use tail to show logs
    let mut args = vec!["-n", "50"];
    if follow {
        args.push("-f");
    }

    // Show both stdout and stderr logs
    let mut log_files = Vec::new();
    if log_file.exists() {
        log_files.push(log_file.to_str().unwrap());
    }
    if error_log_file.exists() {
        log_files.push(error_log_file.to_str().unwrap());
    }

    args.extend(log_files);

    let status = Command::new("tail")
        .args(&args)
        .status()
        .context("Failed to run tail")?;

    if !status.success() && !follow {
        // tail -f exits with ctrl-c, which is normal
        anyhow::bail!("tail failed");
    }

    Ok(())
}
