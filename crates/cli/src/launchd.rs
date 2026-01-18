//! macOS launchd integration for automatic daemon startup
//!
//! This module provides enable/disable functionality for the memex daemon
//! using macOS launchd. When enabled, the daemon will automatically start
//! on login and restart if it crashes.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use crate::config::get_config_dir;

const PLIST_LABEL: &str = "com.lexun.memex.daemon";

/// Get the path to the LaunchAgents directory
fn launch_agents_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

/// Get the path to the plist file
pub fn plist_path() -> Result<PathBuf> {
    Ok(launch_agents_dir()?.join(format!("{}.plist", PLIST_LABEL)))
}

/// Get the path to the daemon log file
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

/// Generate the launchd plist content
fn generate_plist() -> Result<String> {
    let binary_path = resolve_memex_binary()?;
    let config_dir = get_config_dir()?;
    let stdout_log = log_path()?;
    let stderr_log = error_log_path()?;

    // Ensure config directory exists
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)
            .with_context(|| format!("Failed to create config directory: {}", config_dir.display()))?;
    }

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>

    <key>Program</key>
    <string>{binary}</string>

    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>daemon</string>
        <string>run</string>
    </array>

    <key>EnvironmentVariables</key>
    <dict>
        <key>MEMEX_CONFIG_PATH</key>
        <string>{config_dir}</string>
    </dict>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <dict>
        <key>Crashed</key>
        <true/>
        <key>SuccessfulExit</key>
        <false/>
    </dict>

    <key>StandardOutPath</key>
    <string>{stdout}</string>

    <key>StandardErrorPath</key>
    <string>{stderr}</string>

    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
"#,
        label = PLIST_LABEL,
        binary = binary_path.display(),
        config_dir = config_dir.display(),
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
    );

    Ok(plist)
}

/// Check if the launchd service is currently enabled
pub fn is_enabled() -> Result<bool> {
    let path = plist_path()?;
    Ok(path.exists())
}

/// Check if the launchd service is currently loaded
pub fn is_loaded() -> Result<bool> {
    let output = Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output()
        .context("Failed to run launchctl list")?;

    Ok(output.status.success())
}

/// Enable the launchd service
pub fn enable() -> Result<()> {
    let plist_file = plist_path()?;
    let launch_dir = launch_agents_dir()?;

    // Ensure LaunchAgents directory exists
    if !launch_dir.exists() {
        fs::create_dir_all(&launch_dir)
            .with_context(|| format!("Failed to create LaunchAgents directory: {}", launch_dir.display()))?;
    }

    // Generate and write the plist
    let plist_content = generate_plist()?;
    let mut file = fs::File::create(&plist_file)
        .with_context(|| format!("Failed to create plist file: {}", plist_file.display()))?;
    file.write_all(plist_content.as_bytes())?;

    println!("Created plist: {}", plist_file.display());

    // Load the service
    let status = Command::new("launchctl")
        .args(["load", "-w", plist_file.to_str().unwrap()])
        .status()
        .context("Failed to run launchctl load")?;

    if !status.success() {
        anyhow::bail!("launchctl load failed");
    }

    println!("Loaded service: {}", PLIST_LABEL);
    println!();
    println!("The daemon will now start automatically on login and restart if it crashes.");

    Ok(())
}

/// Disable the launchd service
pub fn disable() -> Result<()> {
    let plist_file = plist_path()?;

    // Check if enabled
    if !plist_file.exists() {
        println!("Launchd service is not enabled");
        return Ok(());
    }

    // Unload the service (stop it first if running)
    let status = Command::new("launchctl")
        .args(["unload", "-w", plist_file.to_str().unwrap()])
        .status()
        .context("Failed to run launchctl unload")?;

    if !status.success() {
        // Service might not be loaded, but continue with removal
        eprintln!("Warning: launchctl unload returned non-zero status");
    }

    // Remove the plist file
    fs::remove_file(&plist_file)
        .with_context(|| format!("Failed to remove plist file: {}", plist_file.display()))?;

    println!("Removed plist: {}", plist_file.display());
    println!("Unloaded service: {}", PLIST_LABEL);
    println!();
    println!("The daemon will no longer start automatically.");

    Ok(())
}

/// Show launchd service status
pub fn status() -> Result<()> {
    let enabled = is_enabled()?;
    let loaded = is_loaded()?;

    println!("Launchd autostart:");
    println!("  Enabled: {}", if enabled { "yes" } else { "no" });
    println!("  Loaded: {}", if loaded { "yes" } else { "no" });

    if enabled {
        let plist_file = plist_path()?;
        println!("  Plist: {}", plist_file.display());
    }

    if loaded {
        // Get more details from launchctl
        let output = Command::new("launchctl")
            .args(["list", PLIST_LABEL])
            .output()
            .context("Failed to run launchctl list")?;

        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            // Parse the output (format: PID Status Label)
            let parts: Vec<&str> = info.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                let pid = parts[0];
                let last_exit = parts[1];
                if pid != "-" {
                    println!("  PID: {}", pid);
                }
                if last_exit != "0" && last_exit != "-" {
                    println!("  Last exit status: {}", last_exit);
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

    // Check if log files exist
    if !log_file.exists() && !error_log_file.exists() {
        println!("No log files found yet.");
        println!("  Expected stdout log: {}", log_file.display());
        println!("  Expected stderr log: {}", error_log_file.display());
        println!();
        println!("The daemon may not have been started via launchd yet.");
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
