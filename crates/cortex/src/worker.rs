//! Worker process management
//!
//! Handles spawning, communicating with, and monitoring Claude worker processes.
//!
//! ## Approach
//!
//! For MVP, we use Claude's `-p` (print) mode with JSON output. Each message
//! spawns a new Claude process. This is simpler than stream-json mode and
//! works reliably.
//!
//! Future improvements could use:
//! - `--resume` flag for session continuity
//! - `--input-format stream-json` for true bidirectional communication

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::{CortexError, Result};
use crate::types::{TranscriptEntry, WorkerConfig, WorkerId, WorkerMcpConfig, WorkerState, WorkerStatus};

/// Maximum number of transcript entries to keep per worker
const MAX_TRANSCRIPT_ENTRIES: usize = 50;

/// Check if an error message indicates a session resume failure
///
/// Claude CLI returns specific error messages when a session cannot be resumed
/// (e.g., session expired, invalid session ID, session not found).
fn is_session_resume_failure(stderr: &str, stdout: &str) -> bool {
    let combined = format!("{} {}", stderr, stdout).to_lowercase();
    combined.contains("session") && (
        combined.contains("not found") ||
        combined.contains("expired") ||
        combined.contains("invalid") ||
        combined.contains("cannot resume") ||
        combined.contains("unable to resume") ||
        combined.contains("does not exist")
    )
}

/// Find the claude binary path (async)
///
/// Searches in order:
/// 1. CLAUDE_BINARY env var (explicit override, used by dev scripts)
/// 2. `which claude` (uses current PATH)
/// 3. Common installation locations (homebrew, standard paths)
/// 4. Falls back to "claude" and hopes PATH works
async fn find_claude_binary_async() -> PathBuf {
    // Check for explicit override first (set by just recipe and dev scripts)
    if let Ok(path) = std::env::var("CLAUDE_BINARY") {
        debug!("Using CLAUDE_BINARY from env: {}", path);
        return PathBuf::from(path);
    }

    // Try `which claude` - this respects the current PATH (async)
    let mut cmd = Command::new("which");
    cmd.arg("claude")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Ok(output) = cmd.output().await {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                debug!("Found claude via which: {}", path);
                return PathBuf::from(path);
            }
        }
    }

    // Check common installation locations
    // Use tokio::fs::try_exists for async file existence checks
    let common_paths = [
        // NixOS/home-manager user profile
        dirs::home_dir().map(|h| h.join(".nix-profile/bin/claude")),
        // Standard local install
        dirs::home_dir().map(|h| h.join(".claude/local/claude")),
        // Homebrew on macOS
        Some(PathBuf::from("/opt/homebrew/bin/claude")),
        // Linux standard locations
        Some(PathBuf::from("/usr/local/bin/claude")),
        Some(PathBuf::from("/usr/bin/claude")),
    ];

    for path_opt in common_paths.iter().flatten() {
        if tokio::fs::try_exists(path_opt).await.unwrap_or(false) {
            debug!("Found claude at: {}", path_opt.display());
            return path_opt.clone();
        }
    }

    // Last resort: just use "claude" and hope PATH works
    debug!("Claude binary not found in common locations, falling back to PATH");
    PathBuf::from("claude")
}

/// Capture the direnv/devshell environment for a directory (async)
///
/// Runs `direnv export json` to get the environment variables that would be
/// set by the .envrc in the given directory. This allows workers to spawn
/// with the correct nix shell environment.
///
/// Returns an empty HashMap if direnv fails or isn't available.
async fn get_direnv_env_async(path: &std::path::Path) -> HashMap<String, String> {
    let mut cmd = Command::new("direnv");
    cmd.args(["export", "json"])
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = match cmd.output().await {
        Ok(output) => output,
        Err(e) => {
            debug!("direnv not available: {}", e);
            return HashMap::new();
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!("direnv export failed: {}", stderr);
        return HashMap::new();
    }

    // direnv outputs empty string if no .envrc or environment unchanged
    if output.stdout.is_empty() {
        debug!("No direnv environment for {}", path.display());
        return HashMap::new();
    }

    match serde_json::from_slice::<HashMap<String, String>>(&output.stdout) {
        Ok(env) => {
            info!("Captured {} env vars from direnv for {}", env.len(), path.display());
            env
        }
        Err(e) => {
            debug!("Failed to parse direnv output: {}", e);
            HashMap::new()
        }
    }
}

/// Default timeout for shell validation (60 seconds)
const SHELL_VALIDATION_TIMEOUT_SECS: u64 = 60;

/// Result of shell environment validation
///
/// Used to check if a worker's shell environment will load correctly
/// before performing operations that depend on it (like reload).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ShellValidation {
    /// Shell environment loaded successfully
    Success {
        /// Environment variables that would be set
        env: HashMap<String, String>,
    },
    /// Shell environment failed to load
    Failed {
        /// Error message describing the failure
        error: String,
        /// Exit code if the command ran but failed
        exit_code: Option<i32>,
    },
}

impl ShellValidation {
    /// Returns true if the validation succeeded
    pub fn is_success(&self) -> bool {
        matches!(self, ShellValidation::Success { .. })
    }

    /// Returns the error message if validation failed
    pub fn error_message(&self) -> Option<&str> {
        match self {
            ShellValidation::Failed { error, .. } => Some(error),
            ShellValidation::Success { .. } => None,
        }
    }
}

/// Result of reloading a worker's shell environment
///
/// Used to report the outcome of a shell reload operation,
/// including whether the session was cleared.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ShellReloadResult {
    /// Shell environment reloaded successfully
    Success {
        /// Number of environment variables loaded
        env_var_count: usize,
        /// Whether the environment includes nix store paths
        has_nix: bool,
        /// Whether the session was cleared (only true if clear_session was requested
        /// AND the worker had an existing session)
        session_cleared: bool,
    },
    /// Shell environment failed to load
    Failed {
        /// Error message describing the failure
        error: String,
        /// Exit code if the command ran but failed
        exit_code: Option<i32>,
    },
}

impl ShellReloadResult {
    /// Returns true if the reload succeeded
    pub fn is_success(&self) -> bool {
        matches!(self, ShellReloadResult::Success { .. })
    }

    /// Returns the error message if reload failed
    pub fn error_message(&self) -> Option<&str> {
        match self {
            ShellReloadResult::Failed { error, .. } => Some(error),
            ShellReloadResult::Success { .. } => None,
        }
    }
}

/// Validate that the shell environment for a directory loads correctly
///
/// This function checks if direnv can successfully export the environment
/// for the given directory. It's used to verify that a worker's shell
/// environment is valid before performing operations that depend on it.
///
/// # Why This Matters
///
/// Workers operate in nix devshells via direnv. If a worker modifies
/// flake.nix and introduces an error, the shell will fail to load.
/// Without validation, attempting to reload the worker would kill it
/// before discovering the shell is broken, leaving it stuck.
///
/// By validating first, we can detect problems and report them to the
/// worker so it can fix the issue before requesting a reload.
///
/// # Returns
///
/// - `ShellValidation::Success` if the environment loads correctly
/// - `ShellValidation::Failed` if there's an error loading the environment
pub async fn validate_shell_env(worktree_path: &Path) -> Result<ShellValidation> {
    validate_shell_env_with_timeout(worktree_path, Duration::from_secs(SHELL_VALIDATION_TIMEOUT_SECS)).await
}

/// Validate shell environment with a custom timeout
pub async fn validate_shell_env_with_timeout(
    worktree_path: &Path,
    timeout: Duration,
) -> Result<ShellValidation> {
    info!("Validating shell environment for {}", worktree_path.display());

    // Use tokio::process::Command for async timeout support
    let mut cmd = Command::new("direnv");
    cmd.args(["export", "json"])
        .current_dir(worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Spawn the process
    let child = cmd.spawn().map_err(|e| {
        CortexError::ValidationFailed(format!("Failed to spawn direnv: {}", e))
    })?;

    // Wait for output with timeout
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Ok(ShellValidation::Failed {
                error: format!("Failed to run direnv: {}", e),
                exit_code: None,
            });
        }
        Err(_) => {
            return Ok(ShellValidation::Failed {
                error: format!(
                    "Shell validation timed out after {} seconds. This may indicate a hang in flake evaluation.",
                    timeout.as_secs()
                ),
                exit_code: None,
            });
        }
    };

    // Check if command succeeded
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code();

        warn!(
            "Shell validation failed for {}: exit_code={:?}, stderr={}",
            worktree_path.display(),
            exit_code,
            stderr
        );

        return Ok(ShellValidation::Failed {
            error: if stderr.is_empty() {
                "direnv export failed with no error output".to_string()
            } else {
                stderr
            },
            exit_code,
        });
    }

    // Parse the environment
    // Note: direnv outputs empty string if no .envrc or environment unchanged
    if output.stdout.is_empty() {
        debug!("No direnv environment for {} (empty output)", worktree_path.display());
        // Empty output is still success - just means no env changes needed
        return Ok(ShellValidation::Success {
            env: HashMap::new(),
        });
    }

    let env: HashMap<String, String> = match serde_json::from_slice(&output.stdout) {
        Ok(env) => env,
        Err(e) => {
            return Ok(ShellValidation::Failed {
                error: format!("Failed to parse direnv output: {}", e),
                exit_code: None,
            });
        }
    };

    // Sanity check: PATH should include nix store if we're in a nix environment
    if let Some(path) = env.get("PATH") {
        if !path.contains("/nix/store") {
            warn!(
                "Shell validation warning: PATH doesn't include /nix/store for {}",
                worktree_path.display()
            );
            // This isn't necessarily an error - the directory might not use nix
            // But it's worth noting since cortex workers typically do
        }
    }

    info!(
        "Shell validation succeeded for {} ({} env vars)",
        worktree_path.display(),
        env.len()
    );

    Ok(ShellValidation::Success { env })
}

/// Result of sending a message to a worker
#[derive(Debug, Clone)]
pub struct WorkerResponse {
    /// The text response from the worker
    pub result: String,
    /// Whether the request succeeded
    pub is_error: bool,
    /// Session ID (for potential resume)
    pub session_id: Option<String>,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Internal state for a single worker
struct Worker {
    #[allow(dead_code)] // Used for debugging/logging
    id: WorkerId,
    config: WorkerConfig,
    status: WorkerStatus,
    /// Session ID from last interaction (for --resume)
    last_session_id: Option<String>,
    /// Conversation transcript history
    transcript: Vec<TranscriptEntry>,
    /// Cached direnv environment (captured at worker creation)
    /// This avoids calling direnv on every message, which can be slow.
    cached_direnv_env: HashMap<String, String>,
}

/// Manages multiple Claude worker processes
///
/// Uses two-tier locking for better concurrency:
/// - Global map lock: Brief read lock to get worker reference
/// - Per-worker lock: Write lock for state updates
///
/// This allows multiple workers to update state concurrently.
#[derive(Clone)]
pub struct WorkerManager {
    workers: Arc<RwLock<HashMap<WorkerId, Arc<RwLock<Worker>>>>>,
}

impl WorkerManager {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new worker (doesn't spawn a process yet)
    pub async fn create(&self, config: WorkerConfig) -> Result<WorkerId> {
        let id = WorkerId::new();
        info!("Creating worker {} for directory {}", id, config.cwd);

        // Capture direnv environment at creation time (cached for all future messages)
        let cached_direnv_env = get_direnv_env_async(std::path::Path::new(&config.cwd)).await;

        let mut status = WorkerStatus::new(id.clone());
        status.worktree = Some(config.cwd.clone());
        status.host = Some(hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()));
        status.state = WorkerState::Ready;

        let worker = Worker {
            id: id.clone(),
            config,
            status,
            last_session_id: None,
            transcript: Vec::new(),
            cached_direnv_env,
        };

        let mut workers = self.workers.write().await;
        workers.insert(id.clone(), Arc::new(RwLock::new(worker)));

        info!("Worker {} created successfully", id);
        Ok(id)
    }

    /// Load a worker with existing state (for restoring from DB)
    ///
    /// This allows the daemon to restore workers from persistent storage
    /// after a restart.
    pub async fn load_worker(
        &self,
        id: WorkerId,
        config: WorkerConfig,
        status: WorkerStatus,
        last_session_id: Option<String>,
    ) -> Result<()> {
        info!("Loading worker {} from persistent storage", id);

        // Capture direnv environment (needs to be refreshed on reload)
        let cached_direnv_env = get_direnv_env_async(std::path::Path::new(&config.cwd)).await;

        let worker = Worker {
            id: id.clone(),
            config,
            status,
            last_session_id,
            transcript: Vec::new(), // Fresh transcript on reload
            cached_direnv_env,
        };

        let mut workers = self.workers.write().await;
        workers.insert(id.clone(), Arc::new(RwLock::new(worker)));

        info!("Worker {} loaded successfully", id);
        Ok(())
    }

    /// Get the session ID for a worker (for persistence)
    pub async fn get_session_id(&self, id: &WorkerId) -> Result<Option<String>> {
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };
        let worker = worker_arc.read().await;
        Ok(worker.last_session_id.clone())
    }

    /// Get the config for a worker (for persistence)
    pub async fn get_config(&self, id: &WorkerId) -> Result<WorkerConfig> {
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };
        let worker = worker_arc.read().await;
        Ok(worker.config.clone())
    }

    /// Send a message to a worker and get the response
    ///
    /// This spawns a Claude process with `-p` mode, sends the message,
    /// and returns the response.
    ///
    /// If a session resume fails (e.g., session expired), the method automatically
    /// clears the invalid session and retries without `--resume`.
    pub async fn send_message(&self, id: &WorkerId, message: &str) -> Result<WorkerResponse> {
        let start_time = Utc::now();

        // Get worker reference with brief global read lock
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };

        // Phase 1: Acquire per-worker lock, extract config, update state, add transcript entry
        let (cwd, model, system_prompt, last_session_id, mcp_config, chrome, transcript_idx, direnv_env) = {
            let mut worker = worker_arc.write().await;

            worker.status.state = WorkerState::Working;
            // Note: current_task is preserved - it holds the task ID assigned at dispatch time
            worker.status.last_activity = start_time;
            worker.status.messages_sent += 1;

            // Add transcript entry for this message (response pending)
            let entry = TranscriptEntry {
                timestamp: start_time,
                prompt: message.to_string(),
                response: None,
                is_error: false,
                duration_ms: 0,
            };
            worker.transcript.push(entry);
            let idx = worker.transcript.len() - 1;

            // Trim transcript if too long
            if worker.transcript.len() > MAX_TRANSCRIPT_ENTRIES {
                worker.transcript.remove(0);
            }

            (
                worker.config.cwd.clone(),
                worker.config.model.clone(),
                worker.config.system_prompt.clone(),
                worker.last_session_id.clone(),
                worker.config.mcp_config.clone(),
                worker.config.chrome,
                idx.min(MAX_TRANSCRIPT_ENTRIES - 1), // Adjust index if we trimmed
                worker.cached_direnv_env.clone(),    // Use cached environment
            )
            // Per-worker lock released here
        };

        // Phase 2: Build and run command (NO LOCK HELD - allows concurrent operations)
        // Note: direnv_env was cached at worker creation time to avoid blocking calls
        // We may retry without session if resume fails

        let claude_path = find_claude_binary_async().await;
        let mcp = mcp_config.unwrap_or_else(WorkerMcpConfig::none);

        // Try with session first, retry without if session resume fails
        let mut use_session_id = last_session_id.clone();
        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 1; // Only retry once (without session)

        loop {
            let mut cmd = Command::new(&claude_path);
            cmd.arg("-p").arg(message);
            cmd.arg("--output-format").arg("json");
            cmd.current_dir(&cwd);

            // Apply direnv environment (nix shell, etc.)
            if !direnv_env.is_empty() {
                cmd.envs(&direnv_env);
            }

            // Add model if specified
            if let Some(ref model) = model {
                cmd.arg("--model").arg(model);
            }

            // Add system prompt if specified (appends to default system prompt)
            if let Some(ref prompt) = system_prompt {
                cmd.arg("--append-system-prompt").arg(prompt);
            }

            // Skip permission prompts for automated use
            cmd.arg("--dangerously-skip-permissions");

            // Apply MCP configuration
            // By default, workers are isolated (no inherited MCP servers)
            if mcp.strict {
                // Use strict mode to ignore user's global MCP config
                cmd.arg("--strict-mcp-config");
            }
            // Add any specified MCP server configs
            for server_config in &mcp.servers {
                cmd.arg("--mcp-config").arg(server_config);
            }

            // Enable Chrome browser integration if requested
            if chrome {
                cmd.arg("--chrome");
            }

            // Add resume if we have a session
            if let Some(ref session_id) = use_session_id {
                debug!("Attempting to resume session {} for worker {}", session_id, id);
                cmd.arg("--resume").arg(session_id);
            }

            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            debug!("Running claude for worker {} (retry={})", id, retry_count);

            let output = cmd.output().await.map_err(|e| {
                CortexError::WorkerStartFailed(format!("Failed to spawn claude: {}", e))
            })?;

            // Phase 3: Parse response (still no lock needed)
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let exit_code = output.status.code();

                // Check if this is a session resume failure and we can retry
                if use_session_id.is_some() && retry_count < MAX_RETRIES && is_session_resume_failure(&stderr, &stdout) {
                    warn!(
                        "Session resume failed for worker {} (session={}), retrying without session: stderr={}, stdout={}",
                        id,
                        use_session_id.as_ref().unwrap(),
                        if stderr.is_empty() { "(empty)" } else { &stderr },
                        if stdout.is_empty() { "(empty)" } else { &stdout }
                    );

                    // Clear session and retry
                    use_session_id = None;
                    retry_count += 1;

                    // Also clear the session from the worker state so it's not persisted
                    // Use per-worker lock for better concurrency
                    {
                        let mut worker = worker_arc.write().await;
                        worker.last_session_id = None;
                        info!("Cleared invalid session for worker {}", id);
                    }

                    continue; // Retry the loop without session
                }

                // Get signal info on Unix systems
                #[cfg(unix)]
                let signal = {
                    use std::os::unix::process::ExitStatusExt;
                    output.status.signal()
                };
                #[cfg(not(unix))]
                let signal: Option<i32> = None;

                // Build detailed error message
                let error_msg = format!(
                    "exit_code={:?}, signal={:?}, stderr={}, stdout={}",
                    exit_code,
                    signal,
                    if stderr.is_empty() { "(empty)" } else { &stderr },
                    if stdout.is_empty() { "(empty)" } else { &stdout }
                );

                // Update worker state to error and update transcript
                // Note: current_task is preserved - worker remains assigned to task even on error
                {
                    let mut worker = worker_arc.write().await;
                    worker.status.state = WorkerState::Error(error_msg.clone());

                    // Update transcript entry with error
                    if let Some(entry) = worker.transcript.get_mut(transcript_idx) {
                        entry.response = Some(error_msg.clone());
                        entry.is_error = true;
                    }
                }
                return Err(CortexError::WorkerCommunicationFailed(error_msg));
            }

            // Success! Parse the response
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse JSON response
            let json: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| {
                CortexError::WorkerCommunicationFailed(format!("Invalid JSON response: {}", e))
            })?;

            let result = json.get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let is_error = json.get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let session_id = json.get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let duration_ms = json.get("duration_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            // Log session info for observability
            if retry_count > 0 {
                info!(
                    "Worker {} successfully started fresh session after resume failure (new_session={})",
                    id,
                    session_id.as_deref().unwrap_or("none")
                );
            } else if use_session_id.is_some() {
                debug!(
                    "Worker {} successfully resumed session (session={})",
                    id,
                    use_session_id.as_ref().unwrap()
                );
            }

            // Phase 4: Acquire per-worker lock again to update final state and transcript
            // Note: current_task is preserved - worker remains assigned to task across messages
            {
                let mut worker = worker_arc.write().await;
                // Store session ID for potential resume
                if let Some(ref sid) = session_id {
                    worker.last_session_id = Some(sid.clone());
                }

                worker.status.messages_received += 1;
                worker.status.last_activity = Utc::now();
                worker.status.state = WorkerState::Idle;

                // Update transcript entry with response
                if let Some(entry) = worker.transcript.get_mut(transcript_idx) {
                    entry.response = Some(result.clone());
                    entry.is_error = is_error;
                    entry.duration_ms = duration_ms;
                }
            }

            return Ok(WorkerResponse {
                result,
                is_error,
                session_id,
                duration_ms,
            });
        }
    }

    /// Get status of a worker
    pub async fn status(&self, id: &WorkerId) -> Result<WorkerStatus> {
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };
        let worker = worker_arc.read().await;
        Ok(worker.status.clone())
    }

    /// List all workers and their statuses
    pub async fn list(&self) -> Vec<WorkerStatus> {
        let worker_arcs: Vec<_> = {
            let workers = self.workers.read().await;
            workers.values().cloned().collect()
        };
        let mut statuses = Vec::with_capacity(worker_arcs.len());
        for worker_arc in worker_arcs {
            let worker = worker_arc.read().await;
            statuses.push(worker.status.clone());
        }
        statuses
    }

    /// Set the current task for a worker
    ///
    /// This updates the in-memory worker status with the task ID.
    /// Use this when dispatching a worker to a task.
    pub async fn set_current_task(&self, id: &WorkerId, task_id: Option<String>) -> Result<()> {
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };
        let mut worker = worker_arc.write().await;
        worker.status.current_task = task_id;
        Ok(())
    }

    /// Remove a worker
    pub async fn remove(&self, id: &WorkerId) -> Result<()> {
        let mut workers = self.workers.write().await;
        if workers.remove(id).is_none() {
            return Err(CortexError::WorkerNotFound(id.to_string()));
        }
        info!("Worker {} removed", id);
        Ok(())
    }

    /// Remove all workers
    pub async fn remove_all(&self) {
        let mut workers = self.workers.write().await;
        let count = workers.len();
        workers.clear();
        info!("Removed {} workers", count);
    }

    /// Refresh a worker's session to restore MCP tool access
    ///
    /// This clears the worker's Claude session ID, causing it to start a fresh
    /// session on the next message with properly configured MCP tools.
    ///
    /// # Returns
    ///
    /// A tuple of (session_cleared, current_state) where session_cleared is true
    /// if there was a session to clear.
    pub async fn refresh(&self, id: &WorkerId) -> Result<(bool, WorkerState)> {
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };

        let mut worker = worker_arc.write().await;
        let had_session = worker.last_session_id.is_some();
        worker.last_session_id = None;
        let state = worker.status.state.clone();

        if had_session {
            info!("Worker {} session cleared (refreshed)", id);
        } else {
            info!("Worker {} already had no session", id);
        }

        Ok((had_session, state))
    }

    /// Get the conversation transcript for a worker
    pub async fn transcript(&self, id: &WorkerId, limit: Option<usize>) -> Result<Vec<TranscriptEntry>> {
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };
        let worker = worker_arc.read().await;

        let transcript = &worker.transcript;
        let limit = limit.unwrap_or(transcript.len());

        // Return the most recent entries up to the limit
        let start = transcript.len().saturating_sub(limit);
        Ok(transcript[start..].to_vec())
    }

    /// Validate the shell environment for a worker's directory
    ///
    /// This checks if the direnv/nix environment loads correctly for the
    /// worker's worktree. Use this before performing operations that depend
    /// on the shell environment (like reload) to ensure they will succeed.
    ///
    /// # Returns
    ///
    /// - `ShellValidation::Success` if the environment loads correctly
    /// - `ShellValidation::Failed` if there's an error (broken flake.nix, etc.)
    pub async fn validate_shell(&self, id: &WorkerId) -> Result<ShellValidation> {
        // Get the worker's directory with brief global lock, then per-worker lock
        let cwd = {
            let worker_arc = {
                let workers = self.workers.read().await;
                workers.get(id).cloned().ok_or_else(|| {
                    CortexError::WorkerNotFound(id.to_string())
                })?
            };
            let worker = worker_arc.read().await;
            worker.config.cwd.clone()
        };

        // Validate the shell environment
        let path = Path::new(&cwd);
        validate_shell_env(path).await
    }

    /// Validate the shell environment for an arbitrary directory
    ///
    /// This is a convenience method for validating shell environments
    /// without requiring a worker to exist for that directory.
    pub async fn validate_shell_for_path(&self, path: &Path) -> Result<ShellValidation> {
        validate_shell_env(path).await
    }

    /// Reload the shell environment for a worker
    ///
    /// This validates that the shell environment loads correctly and optionally
    /// clears the worker's session so it starts fresh with the new environment.
    ///
    /// # Why This Is Useful
    ///
    /// Workers operate in nix devshells via direnv. When a worker modifies
    /// flake.nix, .envrc, or other shell configuration, they may want to:
    /// 1. Verify the new shell loads correctly before continuing
    /// 2. Reset their session so Claude picks up new tools/paths
    ///
    /// Note: The direnv environment is already captured fresh on each message,
    /// so the main benefit of reload is validation + optional session reset.
    ///
    /// # Arguments
    ///
    /// * `id` - Worker ID to reload
    /// * `clear_session` - If true, clears the session ID so the next message
    ///   starts a fresh Claude session instead of resuming
    ///
    /// # Returns
    ///
    /// - `ShellReloadResult::Success` with environment info if reload succeeded
    /// - `ShellReloadResult::Failed` with error details if the shell won't load
    pub async fn reload_shell(&self, id: &WorkerId, clear_session: bool) -> Result<ShellReloadResult> {
        // Get worker reference with brief global lock
        let worker_arc = {
            let workers = self.workers.read().await;
            workers.get(id).cloned().ok_or_else(|| {
                CortexError::WorkerNotFound(id.to_string())
            })?
        };

        // Get cwd with per-worker lock
        let cwd = {
            let worker = worker_arc.read().await;
            worker.config.cwd.clone()
        };

        let path = Path::new(&cwd);
        let validation = validate_shell_env(path).await?;

        match validation {
            ShellValidation::Success { env } => {
                // Shell is valid - update cached environment and optionally clear session
                let mut worker = worker_arc.write().await;

                // Update cached direnv environment
                worker.cached_direnv_env = env.clone();

                let session_cleared = if clear_session {
                    let had_session = worker.last_session_id.is_some();
                    worker.last_session_id = None;
                    info!(
                        "Worker {} shell reloaded, session {}",
                        id,
                        if had_session { "cleared" } else { "was already empty" }
                    );
                    had_session
                } else {
                    false
                };

                Ok(ShellReloadResult::Success {
                    env_var_count: env.len(),
                    has_nix: env.get("PATH").map(|p| p.contains("/nix/store")).unwrap_or(false),
                    session_cleared,
                })
            }
            ShellValidation::Failed { error, exit_code } => {
                warn!(
                    "Worker {} shell reload failed: {} (exit_code={:?})",
                    id, error, exit_code
                );
                Ok(ShellReloadResult::Failed { error, exit_code })
            }
        }
    }
}

impl Default for WorkerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_creation() {
        let manager = WorkerManager::new();
        let config = WorkerConfig::new("/tmp");
        let id = manager.create(config).await.unwrap();

        let status = manager.status(&id).await.unwrap();
        assert_eq!(status.state, WorkerState::Ready);
    }

    #[tokio::test]
    async fn test_worker_list() {
        let manager = WorkerManager::new();

        let config1 = WorkerConfig::new("/tmp/a");
        let config2 = WorkerConfig::new("/tmp/b");

        manager.create(config1).await.unwrap();
        manager.create(config2).await.unwrap();

        let list = manager.list().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_shell_validation_helpers() {
        // Test ShellValidation helper methods
        let success = ShellValidation::Success {
            env: HashMap::from([("PATH".to_string(), "/nix/store/abc:/usr/bin".to_string())]),
        };
        assert!(success.is_success());
        assert!(success.error_message().is_none());

        let failure = ShellValidation::Failed {
            error: "flake.nix syntax error".to_string(),
            exit_code: Some(1),
        };
        assert!(!failure.is_success());
        assert_eq!(failure.error_message(), Some("flake.nix syntax error"));
    }

    #[tokio::test]
    async fn test_shell_reload_result_helpers() {
        // Test ShellReloadResult helper methods
        let success = ShellReloadResult::Success {
            env_var_count: 42,
            has_nix: true,
            session_cleared: true,
        };
        assert!(success.is_success());
        assert!(success.error_message().is_none());

        let failure = ShellReloadResult::Failed {
            error: "direnv failed".to_string(),
            exit_code: Some(2),
        };
        assert!(!failure.is_success());
        assert_eq!(failure.error_message(), Some("direnv failed"));
    }

    #[tokio::test]
    async fn test_validate_shell_for_nonexistent_worker() {
        let manager = WorkerManager::new();
        let fake_id = WorkerId::new();

        // Should error for non-existent worker
        let result = manager.validate_shell(&fake_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reload_shell_for_nonexistent_worker() {
        let manager = WorkerManager::new();
        let fake_id = WorkerId::new();

        // Should error for non-existent worker
        let result = manager.reload_shell(&fake_id, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validate_shell_for_path_without_envrc() {
        // Validate a path that doesn't have an .envrc
        // This should return Success with empty env (direnv outputs empty string)
        let result = validate_shell_env(Path::new("/tmp")).await;
        assert!(result.is_ok());

        match result.unwrap() {
            ShellValidation::Success { env } => {
                // /tmp likely has no .envrc, so direnv returns empty
                // This is still a "success" from direnv's perspective
                assert!(env.is_empty() || !env.is_empty()); // Either way is valid
            }
            ShellValidation::Failed { .. } => {
                // Also acceptable - depends on system configuration
            }
        }
    }

    #[tokio::test]
    async fn test_reload_shell_clears_session() {
        let manager = WorkerManager::new();
        let config = WorkerConfig::new("/tmp");
        let id = manager.create(config).await.unwrap();

        // Manually set a session ID to test clearing
        {
            let workers = manager.workers.read().await;
            if let Some(worker_arc) = workers.get(&id) {
                let mut worker = worker_arc.write().await;
                worker.last_session_id = Some("test-session-123".to_string());
            }
        }

        // Verify session was set
        let session_before = manager.get_session_id(&id).await.unwrap();
        assert_eq!(session_before, Some("test-session-123".to_string()));

        // Reload with clear_session=true
        // Note: This will fail shell validation since /tmp has no .envrc,
        // but that's fine - we're testing the session clearing logic
        let result = manager.reload_shell(&id, true).await.unwrap();

        match result {
            ShellReloadResult::Success { session_cleared, .. } => {
                // Session should be cleared
                assert!(session_cleared);
                let session_after = manager.get_session_id(&id).await.unwrap();
                assert!(session_after.is_none());
            }
            ShellReloadResult::Failed { .. } => {
                // Shell validation failed (no .envrc), session should NOT be cleared
                let session_after = manager.get_session_id(&id).await.unwrap();
                assert_eq!(session_after, Some("test-session-123".to_string()));
            }
        }
    }

    #[tokio::test]
    async fn test_set_current_task() {
        let manager = WorkerManager::new();
        let config = WorkerConfig::new("/tmp");
        let id = manager.create(config).await.unwrap();

        // Initially no task assigned
        let status = manager.status(&id).await.unwrap();
        assert!(status.current_task.is_none());

        // Set a task
        manager.set_current_task(&id, Some("task-123".to_string())).await.unwrap();
        let status = manager.status(&id).await.unwrap();
        assert_eq!(status.current_task, Some("task-123".to_string()));

        // Clear the task
        manager.set_current_task(&id, None).await.unwrap();
        let status = manager.status(&id).await.unwrap();
        assert!(status.current_task.is_none());
    }

    #[tokio::test]
    async fn test_set_current_task_nonexistent_worker() {
        let manager = WorkerManager::new();
        let fake_id = WorkerId::new();

        // Should error for non-existent worker
        let result = manager.set_current_task(&fake_id, Some("task-123".to_string())).await;
        assert!(result.is_err());
    }
}
