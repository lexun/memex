//! Agent command for launching Claude in a zellij session with self-restart capability
//!
//! This module enables running Claude within a zellij terminal multiplexer,
//! which allows the agent to restart itself (e.g., to pick up new MCP tools).

use anyhow::{Context, Result};
use std::env;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Default session name for memex agent
const DEFAULT_SESSION_NAME: &str = "memex-agent";

/// Check if we're already running inside a zellij session
pub fn in_zellij() -> bool {
    env::var("ZELLIJ_SESSION_NAME").is_ok()
}

/// Get the current zellij session name if we're in one
#[allow(dead_code)]
pub fn current_session() -> Option<String> {
    env::var("ZELLIJ_SESSION_NAME").ok()
}

/// Launch the agent in a zellij session
///
/// If already in zellij, launches claude directly.
/// Otherwise, creates/attaches to a zellij session first.
pub fn start(session: Option<String>, resume: bool) -> Result<()> {
    if in_zellij() {
        // Already in zellij, just launch claude
        launch_claude(resume)
    } else {
        // Need to enter zellij first
        let session_name = session.unwrap_or_else(|| DEFAULT_SESSION_NAME.to_string());
        enter_zellij_and_launch(&session_name, resume)
    }
}

/// Launch claude with optional resume flag
fn launch_claude(resume: bool) -> Result<()> {
    let claude_binary = env::var("CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string());

    let mut cmd = Command::new(&claude_binary);
    if resume {
        cmd.arg("--resume");
    }

    // Use exec to replace the current process
    let err = cmd.exec();
    Err(anyhow::anyhow!("Failed to exec claude: {}", err))
}

/// Enter zellij session and run memex agent inside it
fn enter_zellij_and_launch(session_name: &str, resume: bool) -> Result<()> {
    // Build the command to run inside zellij
    let memex_binary = env::current_exe()
        .context("Failed to get current executable path")?;

    let mut inner_args = vec!["agent", "run"];
    if resume {
        inner_args.push("--resume");
    }

    // Use zellij attach --create to either attach to existing or create new session
    // The -c flag runs a command instead of the default shell
    let err = Command::new("zellij")
        .arg("attach")
        .arg(session_name)
        .arg("--create")
        .arg("-c")
        .arg(&memex_binary)
        .args(&inner_args)
        .exec();

    Err(anyhow::anyhow!("Failed to exec zellij: {}", err))
}

/// Run agent directly (called from within zellij)
///
/// This is the entry point when already inside a zellij session.
pub fn run(resume: bool) -> Result<()> {
    launch_claude(resume)
}

/// Restart the agent session
///
/// This uses zellij's action system to close and reopen the pane,
/// which effectively restarts the claude process.
pub fn restart() -> Result<()> {
    if !in_zellij() {
        anyhow::bail!("Not running inside a zellij session. Use 'memex agent' to start.");
    }

    // Use zellij action to close and relaunch
    // The action "close-pane" closes current pane, but we need to restart
    // Instead, we'll use write-chars to send a command that restarts

    // Get the memex binary path (kept for potential future use)
    let _memex_binary = env::current_exe()
        .context("Failed to get current executable path")?;

    // Write exit command followed by restart command
    // This works because zellij will restart the pane with the original command
    println!("Restarting agent session...");

    // Use zellij action to run a command in the current pane
    // close-pane then the session restarts with original command
    let status = Command::new("zellij")
        .args(["action", "close-pane"])
        .status()
        .context("Failed to execute zellij action")?;

    if !status.success() {
        // Alternative approach: just exit and let zellij config handle restart
        // Or use a more direct approach with exec
        println!("Attempting alternative restart method...");

        // Simply exec into a new claude process
        let claude_binary = env::var("CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string());
        let err = Command::new(&claude_binary).arg("--resume").exec();
        anyhow::bail!("Failed to restart: {}", err);
    }

    Ok(())
}

/// List active zellij sessions
pub fn list_sessions() -> Result<Vec<String>> {
    let output = Command::new("zellij")
        .args(["list-sessions"])
        .output()
        .context("Failed to run zellij list-sessions")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let sessions = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect();

    Ok(sessions)
}

/// Kill a zellij session
pub fn kill_session(session_name: &str) -> Result<()> {
    let status = Command::new("zellij")
        .args(["kill-session", session_name])
        .status()
        .context("Failed to run zellij kill-session")?;

    if !status.success() {
        anyhow::bail!("Failed to kill session: {}", session_name);
    }

    Ok(())
}
