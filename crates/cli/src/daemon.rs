//! Daemon process for Memex
//!
//! The daemon holds database connections for forge and atlas, handling requests
//! from clients via a Unix socket using JSON-RPC style messages.

use std::collections::HashMap;
use std::fs::{self, File};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use anyhow::{Context, Result};
use nix::unistd::{fork, setsid, ForkResult};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use atlas::{
    Event, EventSource, Extractor, MemoSource, MultiStepExtractor, QueryDecomposer,
    Record, RecordType, Store as AtlasStore,
    TaskView, TaskNoteView, TaskDependencyView,
    ThreadSource, EntryRole,
};
use cortex::{ShellReloadResult, WorkerConfig, WorkerId, WorkerManager, WorkerState, WorkerStatus};
use db::Database;
use forge::{from_surreal_datetime, DbWorker, Store as ForgeStore, TaskStatus};
use ipc::{Error as IpcError, ErrorCode, Request, Response};
use llm::LlmClient;
use serde_json::json;

use crate::config::{get_config_dir, get_db_path, get_pid_file, get_socket_path, load_config, Config};
use crate::pid::{check_daemon, remove_pid_file, write_pid_file, PidInfo};
use crate::web;

/// State for a pending async response
#[allow(dead_code)]
struct AsyncResponseState {
    /// The response value (None = still processing, Some = complete)
    response: Option<serde_json::Value>,
    /// When this entry was created (for cleanup of stale entries)
    created_at: chrono::DateTime<chrono::Utc>,
    /// When the response completed (for cleanup timing)
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// An inter-agent message for async communication
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentMessage {
    /// Unique message ID
    id: String,
    /// Sender identifier (e.g., worker ID or "primary" for main session)
    sender: String,
    /// Optional recipient identifier (None = broadcast/coordinator)
    recipient: Option<String>,
    /// Message content
    content: String,
    /// Message type for categorization (e.g., "status", "request", "response")
    message_type: String,
    /// When the message was sent
    timestamp: chrono::DateTime<chrono::Utc>,
    /// Whether this message has been read
    read: bool,
}

/// System prompt for the Coordinator worker
const COORDINATOR_SYSTEM_PROMPT: &str = r#"# Coordinator Role

You are the Coordinator - a dedicated agent responsible for managing worker orchestration. Your job is to keep workers productive on the right tasks while freeing the Primary Claude (who works directly with the user) from management overhead.

## Your Responsibilities

1. **Receive Guidance**: Primary Claude sends you high-level direction via messages (e.g., "keep 3 workers busy on memex tasks", "focus on the orchestration feature")

2. **Dispatch Workers**: Use `cortex_dispatch_task` to assign tasks to workers. Pick tasks from the ready queue that align with current guidance.

3. **Monitor Progress**: Periodically check worker status with `cortex_list_workers` and `cortex_worker_status`. Look for:
   - Workers that have finished (messages_received > messages_sent)
   - Workers that seem stuck (long time since last activity)
   - Workers in error state

4. **Review Completed Work**: When a worker finishes:
   - Check their worktree for commits using git commands
   - Review the changes are reasonable
   - Either merge to dev branch or flag for human review

5. **Surface Issues**: Create tasks or send messages when you encounter:
   - Architectural questions that need human decision
   - Workers that are stuck on something unclear
   - Merge conflicts or test failures

6. **Handle Health Alerts**: The daemon sends health alerts when workers appear stuck or unhealthy. See the "Health Alert Handling" section below for details.

## What You Do NOT Do

- Talk directly to the user (only through Primary Claude or by creating tasks)
- Make big architectural decisions without surfacing for review
- Push to remote repositories
- Replace Primary Claude for design discussions

## Communication Protocol

- Check for messages at the start of each work cycle: `check_messages(recipient="coordinator")`
- Send status updates to Primary Claude: `send_agent_message(sender="coordinator", recipient="primary", message_type="status", ...)`
- When you need clarification: `send_agent_message(sender="coordinator", recipient="primary", message_type="request", ...)`

## Health Alert Handling

The daemon monitors worker health and sends alerts via the message queue. Health alerts have `message_type: "health_alert"` and contain JSON with:
- `worker_id`: Which worker triggered the alert
- `alert_type`: One of `inactive_working`, `inactive_idle`, or `error_state`
- `description`: Human-readable explanation
- `current_task`: Task ID the worker is assigned to (if any)
- `last_activity`: When the worker was last active
- `inactive_secs`: How long the worker has been inactive

**Responding to Health Alerts:**

1. **inactive_working**: Worker appears stuck while processing a message
   - First, check `cortex_worker_transcript` to see what they were doing
   - If they seem genuinely stuck, try sending a follow-up message to prompt them
   - If no response after another check cycle, consider removing and re-dispatching the task
   - Escalate to Primary if the pattern repeats

2. **inactive_idle**: Worker has been idle for a long time
   - This may be intentional if no guidance to keep workers busy
   - If you have tasks to dispatch, assign one to this worker
   - If worker was supposed to be working, check their transcript for issues

3. **error_state**: Worker encountered an error
   - Check `cortex_worker_status` for the error details
   - Determine if it's recoverable (retry) or needs escalation
   - Consider removing the worker and re-dispatching its task
   - Record findings as a memo for future reference

**Don't kill workers arbitrarily** - health alerts are informational. Use your judgment:
- Long-running legitimate work (big refactors, complex tasks) may exceed thresholds
- Check transcript to understand what the worker is doing before taking action
- When in doubt, ping the worker or escalate to Primary rather than killing

## Async Messaging Pattern

**IMPORTANT**: For long-running operations like sending messages to workers, use async messaging to avoid blocking:

1. **Send async**: Use `cortex_send_message` with `run_in_background=true` - returns a message_id immediately
2. **Poll for results**: Use `cortex_get_response(message_id)` to check if the response is ready
   - Returns `null` if still processing
   - Returns the response when complete

Example workflow for communicating with workers:
```
# Dispatch message asynchronously
message_id = cortex_send_message(worker_id, message, run_in_background=true)

# Continue with other work or poll periodically
response = cortex_get_response(message_id)
if response is null:
    # Still processing - check back later or do other work
else:
    # Response ready - process it
```

This pattern is essential for efficient orchestration - you can dispatch multiple workers and poll for their results without blocking on each one.

## Work Loop

Each cycle:
1. Check for new messages/guidance (including health alerts)
2. List current workers and their status
3. Handle any health alerts appropriately
4. If workers finished, review their work
5. If guidance says to keep N workers busy, dispatch tasks until you have N active workers
6. Record any significant decisions or findings as memos
7. Go idle if no guidance or no ready tasks

## Git Workflow

When reviewing worker output:
- Workers work in isolated worktrees off the dev branch
- Use `vibetree_merge` with `squash=true` to merge completed work
- Only merge to dev, never to main
- Do not push to remote

## Available Tools

You have access to all Memex MCP tools including:
- `cortex_dispatch_task` - spawn a worker for a task
- `cortex_dispatch_tasks` - spawn multiple workers in parallel
- `cortex_list_workers` - see all workers
- `cortex_worker_status` - detailed worker info
- `cortex_worker_transcript` - see worker conversation
- `cortex_send_message` - send message to a worker (use run_in_background=true for async)
- `cortex_get_response` - get response from async message
- `cortex_remove_worker` - remove a worker (use after reviewing their work or if stuck)
- `vibetree_merge` - merge worktree branches
- `ready_tasks` - get tasks ready to work on
- `send_agent_message` / `check_messages` - inter-agent communication
- `record_memo` - capture decisions and findings
"#;

/// Container for stores and services
struct Stores {
    forge: ForgeStore,
    atlas: AtlasStore,
    extractor: Option<Extractor>,
    workers: WorkerManager,
    /// Storage for async message responses
    async_responses: RwLock<HashMap<String, AsyncResponseState>>,
    /// Message queue for inter-agent communication
    agent_messages: RwLock<Vec<AgentMessage>>,
}

/// Load persisted workers from database on startup
async fn load_workers_from_db(stores: &Arc<Stores>) -> Result<()> {
    let db_workers = stores.forge.list_workers(None).await?;

    if db_workers.is_empty() {
        tracing::info!("No persisted workers to load");
        return Ok(());
    }

    tracing::info!("Loading {} persisted workers from database", db_workers.len());

    for db_worker in db_workers {
        // Skip workers that were stopped or errored
        if db_worker.state == "stopped" || db_worker.state == "error" {
            tracing::debug!("Skipping {} worker {}", db_worker.state, db_worker.worker_id);
            continue;
        }

        // Build WorkerConfig
        let mut config = WorkerConfig::new(&db_worker.cwd);
        if let Some(ref model) = db_worker.model {
            if !model.is_empty() {
                config = config.with_model(model);
            }
        }
        if let Some(ref prompt) = db_worker.system_prompt {
            if !prompt.is_empty() {
                config = config.with_system_prompt(prompt);
            }
        }
        // Restore MCP config if persisted
        if let Some(ref mcp_json) = db_worker.mcp_config {
            match serde_json::from_str::<cortex::WorkerMcpConfig>(mcp_json) {
                Ok(mcp_config) => {
                    config = config.with_mcp_config(mcp_config);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse MCP config for worker {}: {}",
                        db_worker.worker_id,
                        e
                    );
                }
            }
        }

        // Build WorkerStatus
        let worker_id = WorkerId::from_string(&db_worker.worker_id);
        let mut status = WorkerStatus::new(worker_id.clone());
        status.worktree = db_worker.worktree.clone();
        status.current_task = db_worker.current_task.clone();
        status.host = Some(hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()));
        status.messages_sent = db_worker.messages_sent as u64;
        status.messages_received = db_worker.messages_received as u64;
        // Set state to Idle - orchestrator will decide what to do
        status.state = WorkerState::Idle;

        // IMPORTANT: Clear stale session IDs on daemon restart.
        //
        // Claude sessions reference MCP servers that were running in the previous
        // daemon process. When we resume a stale session, those MCP connections
        // are broken and Claude reports "No such tool available" errors.
        //
        // By clearing the session ID, workers start fresh with properly configured
        // MCP tools when they receive their next message. The worker's configuration
        // (cwd, model, system_prompt, mcp_config) is preserved, only the session
        // context is reset.
        let stale_session = db_worker.last_session_id.is_some();

        // Load into WorkerManager without session ID to force fresh start
        if let Err(e) = stores.workers.load_worker(
            worker_id.clone(),
            config,
            status,
            None, // Clear session - will start fresh on next message
        ).await {
            tracing::warn!("Failed to load worker {}: {}", db_worker.worker_id, e);
            continue;
        }

        // Also clear the session in database to keep it consistent
        if stale_session {
            if let Err(e) = stores.forge.update_worker_session(&db_worker.worker_id, None).await {
                tracing::warn!(
                    "Failed to clear stale session for worker {}: {}",
                    db_worker.worker_id,
                    e
                );
            }
        }

        // Log with clear indication of session status
        let session_status = if stale_session { "cleared (stale)" } else { "fresh start" };
        tracing::info!(
            "Loaded worker {} - {} (task: {}, worktree: {}, msgs: {}/{})",
            db_worker.worker_id,
            session_status,
            db_worker.current_task.as_deref().unwrap_or("none"),
            db_worker.worktree.as_deref().unwrap_or("none"),
            db_worker.messages_sent,
            db_worker.messages_received,
        );
    }

    Ok(())
}

/// The daemon process
pub struct Daemon {
    config: Config,
    socket_path: PathBuf,
    pid_path: PathBuf,
}

impl Daemon {
    /// Create a new daemon instance
    pub fn new() -> Result<Self> {
        let config = load_config()?;
        let socket_path = get_socket_path(&config)?;
        let pid_path = get_pid_file(&config)?;

        if let Some(info) = check_daemon(&pid_path)? {
            anyhow::bail!(
                "Daemon already running (PID: {}, started: {})",
                info.pid,
                info.started_at
            );
        }

        Ok(Self {
            config,
            socket_path,
            pid_path,
        })
    }

    /// Start the daemon (forks to background and re-execs to avoid macOS fork issues)
    pub fn start(self) -> Result<()> {
        // Ensure config directory exists before forking
        if let Some(parent) = self.socket_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Fork to background
        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                println!("Daemon started (PID: {})", child);
                return Ok(());
            }
            Ok(ForkResult::Child) => {
                // Continue in child process
            }
            Err(e) => {
                anyhow::bail!("Fork failed: {}", e);
            }
        }

        // Child process: become session leader
        setsid().context("Failed to create new session")?;

        // Re-exec ourselves with daemon run to get a clean process
        // This avoids macOS fork() + Objective-C issues
        let exe = std::env::current_exe()?;
        let exe_str = exe.to_string_lossy().to_string();
        let err = exec::execvp(&exe_str, &[&exe_str, "daemon", "run"]);
        anyhow::bail!("Failed to exec daemon: {}", err);
    }

    /// Run the daemon directly (called after re-exec)
    pub fn run_foreground() -> Result<()> {
        // Redirect stdin/stdout/stderr to /dev/null
        let dev_null = File::open("/dev/null")?;
        let null_fd = dev_null.as_raw_fd();
        unsafe {
            libc::dup2(null_fd, 0); // stdin
            libc::dup2(null_fd, 1); // stdout
            libc::dup2(null_fd, 2); // stderr
        }

        // Set up logging to file
        let log_path = get_config_dir().ok().map(|d| d.join("daemon.log"));
        if let Some(ref path) = log_path {
            if let Ok(file) = File::create(path) {
                let file_fd = file.as_raw_fd();
                unsafe {
                    libc::dup2(file_fd, 1); // stdout to log
                    libc::dup2(file_fd, 2); // stderr to log
                }
            }
        }

        let config = load_config()?;
        let socket_path = get_socket_path(&config)?;
        let pid_path = get_pid_file(&config)?;

        let daemon = Self {
            config,
            socket_path,
            pid_path,
        };

        // Build and run tokio runtime in daemon process
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(daemon.run_daemon())
    }

    /// Main daemon loop
    async fn run_daemon(self) -> Result<()> {
        // Remove stale socket
        if self.socket_path.exists() {
            fs::remove_file(&self.socket_path)
                .with_context(|| format!("Failed to remove stale socket: {}", self.socket_path.display()))?;
        }

        // Get default database path for embedded mode
        let default_db_path = get_db_path(&self.config)?;
        let is_embedded = self.config.database.url.is_none();

        // Connect to forge database
        tracing::info!("Connecting to forge database...");
        let forge_db = Database::connect(
            &self.config.database,
            "forge",
            Some(default_db_path.clone()),
        )
        .await
        .context("Failed to connect to forge database")?;

        // Connect to atlas database
        // For embedded mode, share the connection to avoid RocksDB lock conflicts
        tracing::info!("Connecting to atlas database...");
        let atlas_db = if is_embedded {
            forge_db
                .with_database("atlas")
                .await
                .context("Failed to connect to atlas database")?
        } else {
            Database::connect(&self.config.database, "atlas", Some(default_db_path))
                .await
                .context("Failed to connect to atlas database")?
        };

        // Create extractor if LLM configured
        let extractor = if self.config.llm.api_key.is_some() {
            tracing::info!("LLM configured: {} / {}", self.config.llm.provider, self.config.llm.model);
            let llm_client = LlmClient::new(llm::LlmConfig {
                provider: self.config.llm.provider.clone(),
                model: self.config.llm.model.clone(),
                embedding_model: self.config.llm.embedding_model.clone(),
                api_key: self.config.llm.api_key.clone(),
                base_url: self.config.llm.base_url.clone(),
            });
            Some(Extractor::new(llm_client))
        } else {
            tracing::info!("LLM not configured (no API key) - fact extraction disabled");
            None
        };

        // Create stores
        let forge_store = ForgeStore::new(forge_db);
        let atlas_store = AtlasStore::new(atlas_db);

        let stores = Arc::new(Stores {
            forge: forge_store.clone(),
            atlas: atlas_store.clone(),
            extractor,
            workers: WorkerManager::new(),
            async_responses: RwLock::new(HashMap::new()),
            agent_messages: RwLock::new(Vec::new()),
        });

        // Load persisted workers from database
        if let Err(e) = load_workers_from_db(&stores).await {
            tracing::warn!("Failed to load workers from DB: {}", e);
        }

        // Bind the Unix socket for IPC
        let listener = UnixListener::bind(&self.socket_path)
            .with_context(|| format!("Failed to bind socket: {}", self.socket_path.display()))?;

        tracing::info!("Daemon listening on {}", self.socket_path.display());

        // Start web server if enabled
        let web_handle = if self.config.web.enabled {
            let web_state = Arc::new(web::WebState {
                forge: forge_store,
                atlas: atlas_store,
                workers: stores.workers.clone(),
            });
            let web_router = web::build_router(web_state, &self.config.web);
            let web_addr = format!("{}:{}", self.config.web.host, self.config.web.port);

            tracing::info!("Web UI available at http://{}", web_addr);

            let web_listener = tokio::net::TcpListener::bind(&web_addr)
                .await
                .with_context(|| format!("Failed to bind web server to {}", web_addr))?;

            Some(tokio::spawn(async move {
                if let Err(e) = axum::serve(web_listener, web_router).await {
                    tracing::error!("Web server error: {}", e);
                }
            }))
        } else {
            tracing::info!("Web UI disabled");
            None
        };

        // Start health monitor if enabled
        let health_handle = if self.config.daemon.health_monitor.enabled {
            let stores_for_health = Arc::clone(&stores);
            let check_interval = self.config.daemon.health_monitor.check_interval_secs;
            let inactivity_threshold = self.config.daemon.health_monitor.inactivity_threshold_secs;

            Some(tokio::spawn(async move {
                run_health_monitor(stores_for_health, check_interval, inactivity_threshold).await;
            }))
        } else {
            tracing::info!("Worker health monitor disabled");
            None
        };

        // Write PID file
        let pid_info = PidInfo::new(process::id(), &self.socket_path);
        write_pid_file(&self.pid_path, &pid_info)?;

        // Run the IPC server
        let result = self.run_server(listener, stores).await;

        // Abort health monitor if running
        if let Some(handle) = health_handle {
            handle.abort();
        }

        // Abort web server if running
        if let Some(handle) = web_handle {
            handle.abort();
        }

        self.cleanup();
        result
    }

    /// Accept and handle connections
    async fn run_server(&self, listener: UnixListener, stores: Arc<Stores>) -> Result<()> {
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _addr)) => {
                            let stores = Arc::clone(&stores);
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, stores).await {
                                    tracing::error!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {}", e);
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Received shutdown signal");
                    break;
                }
            }
        }
        Ok(())
    }

    /// Clean up resources on shutdown
    fn cleanup(&self) {
        if let Err(e) = remove_pid_file(&self.pid_path) {
            tracing::error!("Failed to remove PID file: {}", e);
        }
        if self.socket_path.exists() {
            if let Err(e) = fs::remove_file(&self.socket_path) {
                tracing::error!("Failed to remove socket file: {}", e);
            }
        }
    }
}

/// Worker health alert sent to coordinator via agent message queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHealthAlert {
    /// Worker ID that triggered the alert
    pub worker_id: String,
    /// Type of health issue detected
    pub alert_type: HealthAlertType,
    /// Human-readable description
    pub description: String,
    /// Worker's current state
    pub state: String,
    /// Task the worker is assigned to (if any)
    pub current_task: Option<String>,
    /// When the worker was last active
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// How long the worker has been inactive (in seconds)
    pub inactive_secs: u64,
}

/// Types of health alerts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthAlertType {
    /// Worker has been inactive beyond the threshold while in Working state
    InactiveWorking,
    /// Worker has been idle beyond the threshold
    InactiveIdle,
    /// Worker is in an error state
    ErrorState,
}

/// Background task that monitors worker health and sends alerts to coordinator
async fn run_health_monitor(
    stores: Arc<Stores>,
    check_interval_secs: u64,
    inactivity_threshold_secs: u64,
) {
    let check_interval = Duration::from_secs(check_interval_secs);
    let inactivity_threshold = chrono::Duration::seconds(inactivity_threshold_secs as i64);

    tracing::info!(
        "Health monitor started: checking every {}s, inactivity threshold {}s",
        check_interval_secs,
        inactivity_threshold_secs
    );

    // Track which workers we've already alerted about to avoid spamming
    let mut alerted_workers: HashMap<String, chrono::DateTime<chrono::Utc>> = HashMap::new();

    loop {
        tokio::time::sleep(check_interval).await;

        let now = chrono::Utc::now();

        // Get all workers
        let workers = stores.workers.list().await;

        if workers.is_empty() {
            continue;
        }

        tracing::debug!("Health monitor checking {} workers", workers.len());

        for worker in workers {
            let worker_id = worker.id.to_string();
            let time_since_activity = now.signed_duration_since(worker.last_activity);

            // Skip workers that haven't exceeded the threshold
            if time_since_activity < inactivity_threshold {
                // Remove from alerted set if they've become active again
                alerted_workers.remove(&worker_id);
                continue;
            }

            // Determine alert type based on worker state
            let alert_type = match &worker.state {
                WorkerState::Working => HealthAlertType::InactiveWorking,
                WorkerState::Idle => HealthAlertType::InactiveIdle,
                WorkerState::Error(_) => HealthAlertType::ErrorState,
                // Don't alert for Starting, Ready, or Stopped states
                _ => continue,
            };

            // Check if we've already alerted about this worker recently
            // Re-alert every 5 minutes (10x the check interval) if still inactive
            let realert_threshold = chrono::Duration::seconds((check_interval_secs * 10) as i64);
            if let Some(last_alert_time) = alerted_workers.get(&worker_id) {
                if now.signed_duration_since(*last_alert_time) < realert_threshold {
                    continue;
                }
            }

            let inactive_secs = time_since_activity.num_seconds() as u64;

            let description = match &alert_type {
                HealthAlertType::InactiveWorking => format!(
                    "Worker {} has been in 'working' state for {} seconds without activity",
                    worker_id, inactive_secs
                ),
                HealthAlertType::InactiveIdle => format!(
                    "Worker {} has been idle for {} seconds",
                    worker_id, inactive_secs
                ),
                HealthAlertType::ErrorState => format!(
                    "Worker {} is in error state: {:?}",
                    worker_id, worker.state
                ),
            };

            let alert = WorkerHealthAlert {
                worker_id: worker_id.clone(),
                alert_type,
                description: description.clone(),
                state: format!("{:?}", worker.state),
                current_task: worker.current_task.clone(),
                last_activity: worker.last_activity,
                inactive_secs,
            };

            // Send alert to coordinator via agent message queue
            let alert_json = serde_json::to_string(&alert).unwrap_or_default();
            let message = AgentMessage {
                id: format!("health-alert-{}-{}", worker_id, now.timestamp_millis()),
                sender: "health_monitor".to_string(),
                recipient: Some("coordinator".to_string()),
                content: alert_json,
                message_type: "health_alert".to_string(),
                timestamp: now,
                read: false,
            };

            // Add to message queue
            {
                let mut messages = stores.agent_messages.write().await;
                messages.push(message);
            }

            tracing::warn!("{}", description);
            alerted_workers.insert(worker_id, now);
        }

        // Clean up old entries from alerted_workers map
        let cleanup_threshold = chrono::Duration::hours(1);
        alerted_workers.retain(|_, last_alert| {
            now.signed_duration_since(*last_alert) < cleanup_threshold
        });
    }
}

/// Handle a single client connection
async fn handle_connection(stream: UnixStream, stores: Arc<Stores>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }

        // Parse request
        let request: Request = match serde_json::from_str(line.trim()) {
            Ok(req) => req,
            Err(e) => {
                let response = Response::error(
                    "",
                    IpcError::new(ErrorCode::ParseError, format!("Invalid JSON: {}", e)),
                );
                let mut json = serde_json::to_string(&response)?;
                json.push('\n');
                writer.write_all(json.as_bytes()).await?;
                continue;
            }
        };

        // Handle request
        let response = handle_request(&request, &stores).await;

        // Send response
        let mut json = serde_json::to_string(&response)?;
        json.push('\n');
        writer.write_all(json.as_bytes()).await?;
    }

    Ok(())
}

/// Handle a single request
async fn handle_request(request: &Request, stores: &Arc<Stores>) -> Response {
    let start = Instant::now();
    let result = dispatch_request(request, stores).await;
    let elapsed = start.elapsed();

    // Log timing for performance monitoring
    let elapsed_ms = elapsed.as_millis();
    if elapsed_ms > 100 {
        tracing::warn!(
            method = %request.method,
            elapsed_ms = %elapsed_ms,
            "Slow request"
        );
    } else {
        tracing::debug!(
            method = %request.method,
            elapsed_ms = %elapsed_ms,
            "Request completed"
        );
    }

    match result {
        Ok(value) => Response::success(&request.id, value).unwrap_or_else(|e| {
            Response::error(&request.id, IpcError::internal(format!("Serialization error: {}", e)))
        }),
        Err(err) => Response::error(&request.id, err),
    }
}

/// Dispatch request to the appropriate handler
async fn dispatch_request(request: &Request, stores: &Arc<Stores>) -> Result<serde_json::Value, IpcError> {
    match request.method.as_str() {
        // Health check
        "health_check" => Ok(serde_json::json!(true)),

        // Status
        "status" => Ok(serde_json::json!({
            "status": "running",
            "version": env!("CARGO_PKG_VERSION"),
            "pid": process::id()
        })),

        // Task operations (atlas) - Tasks are Records with workflow semantics
        "create_task" => handle_create_task(request, stores).await,
        "list_tasks" => handle_list_tasks(request, &stores.atlas).await,
        "ready_tasks" => handle_ready_tasks(request, &stores.atlas).await,
        "get_task" => handle_get_task(request, &stores.atlas).await,
        "update_task" => handle_update_task(request, stores).await,
        "close_task" => handle_close_task(request, stores).await,
        "delete_task" => handle_delete_task(request, stores).await,

        // Note operations (atlas)
        "add_note" => handle_add_note(request, stores).await,
        "get_notes" => handle_get_notes(request, &stores.atlas).await,
        "edit_note" => handle_edit_note(request, stores).await,
        "delete_note" => handle_delete_note(request, stores).await,

        // Dependency operations (atlas)
        "add_dependency" => handle_add_dependency(request, stores).await,
        "remove_dependency" => handle_remove_dependency(request, stores).await,
        "get_dependencies" => handle_get_dependencies(request, &stores.atlas).await,

        // Memo operations (atlas) - record_memo also triggers extraction
        "record_memo" => handle_record_memo(request, stores).await,
        "list_memos" => handle_list_memos(request, &stores.atlas).await,
        "get_memo" => handle_get_memo(request, &stores.atlas).await,
        "delete_memo" => handle_delete_memo(request, &stores.atlas).await,
        "purge_memo" => handle_purge_memo(request, &stores.atlas).await,

        // Event operations (atlas)
        "list_events" => handle_list_events(request, &stores.atlas).await,
        "get_event" => handle_get_event(request, &stores.atlas).await,

        // Knowledge operations (query, search, extract, rebuild, backfill)
        "query_knowledge" => handle_query_knowledge(request, stores).await,
        "search_knowledge" => handle_search_knowledge(request, stores).await,
        "extract_facts" => handle_extract_facts(request, stores).await,
        "rebuild_knowledge" => handle_rebuild_knowledge(request, stores).await,
        "backfill_embeddings" => handle_backfill_embeddings(request, stores).await,
        "knowledge_status" => handle_knowledge_status(stores).await,

        // Migration operations (Forge to Atlas)
        "migrate_tasks_to_records" => handle_migrate_tasks_to_records(stores).await,
        // Test-only: Import into Forge (for migration testing)
        "test_import_forge_task" => handle_test_import_forge_task(request, stores).await,
        "test_import_forge_note" => handle_test_import_forge_note(request, stores).await,
        "test_import_forge_dependency" => handle_test_import_forge_dependency(request, stores).await,

        // Entity operations (legacy - being replaced by Record operations)
        "list_entities" => handle_list_entities(request, &stores.atlas).await,
        "get_entity_facts" => handle_get_entity_facts(request, &stores.atlas).await,
        "get_related_facts" => handle_get_related_facts(request, &stores.atlas).await,

        // Record-based knowledge operations (new - part of Entity→Record migration)
        "get_record_facts" => handle_get_record_facts(request, &stores.atlas).await,

        // Record extraction operations (new Records + Links pipeline)
        "extract_records_from_memo" => handle_extract_records_from_memo(request, stores).await,
        "backfill_records" => handle_backfill_records(request, stores).await,

        // Record/Graph operations (atlas)
        "list_records" => handle_list_records(request, &stores.atlas).await,
        "get_record" => handle_get_record(request, &stores.atlas).await,
        "create_record" => handle_create_record(request, stores).await,
        "update_record" => handle_update_record(request, stores).await,
        "delete_record" => handle_delete_record(request, stores).await,
        "purge_record" => handle_purge_record(request, &stores.atlas).await,
        "merge_records" => handle_merge_records(request, stores).await,
        "preview_merge_records" => handle_preview_merge_records(request, &stores.atlas).await,
        "create_edge" => handle_create_edge(request, stores).await,
        "list_edges" => handle_list_edges(request, &stores.atlas).await,
        "delete_edge" => handle_delete_edge(request, stores).await,
        "assemble_context" => handle_assemble_context(request, &stores.atlas).await,

        // Cortex operations (worker management)
        "cortex_create_worker" => handle_cortex_create_worker(request, stores).await,
        "cortex_dispatch_task" => handle_cortex_dispatch_task(request, stores).await,
        "cortex_dispatch_tasks" => handle_cortex_dispatch_tasks(request, stores).await,
        "cortex_send_message" => handle_cortex_send_message(request, stores).await,
        "cortex_send_message_async" => handle_cortex_send_message_async(request, stores).await,
        "cortex_get_response" => handle_cortex_get_response(request, stores).await,
        "cortex_worker_status" => handle_cortex_worker_status(request, &stores.workers).await,
        "cortex_list_workers" => handle_cortex_list_workers(&stores.workers).await,
        "get_workers_by_task" => handle_get_workers_by_task(request, &stores.forge).await,
        "cortex_remove_worker" => handle_cortex_remove_worker(request, stores).await,
        "cortex_worker_transcript" => handle_cortex_worker_transcript(request, stores).await,
        "cortex_validate_shell" => handle_cortex_validate_shell(request, &stores.workers).await,
        "cortex_reload_shell" => handle_cortex_reload_shell(request, stores).await,
        "cortex_refresh_worker" => handle_cortex_refresh_worker(request, stores).await,
        "cortex_get_coordinator" => handle_cortex_get_coordinator(stores).await,

        // Vibetree operations (worktree management)
        "vibetree_list" => handle_vibetree_list(request).await,
        "vibetree_create" => handle_vibetree_create(request).await,
        "vibetree_remove" => handle_vibetree_remove(request).await,
        "vibetree_merge" => handle_vibetree_merge(request).await,
        "vibetree_env" => handle_vibetree_env(request).await,

        // Agent messaging operations (inter-agent communication)
        "send_agent_message" => handle_send_agent_message(request, stores).await,
        "check_messages" => handle_check_messages(request, stores).await,
        "clear_agent_messages" => handle_clear_agent_messages(request, stores).await,

        // Unknown method
        _ => Err(IpcError::method_not_found(&request.method)),
    }
}

// ========== Task Handlers ==========
// Note: Tasks are Records in Atlas with workflow semantics stored in content.
// The TaskView adapter provides API compatibility with the old Forge format.

async fn handle_create_task(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    // Accept TaskView format (backwards compatible with old Task format)
    let task_view: TaskView = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid task: {}", e)))?;

    // Create task in Atlas
    let record = stores
        .atlas
        .create_task(
            &task_view.title,
            task_view.description.as_deref(),
            task_view.project.as_deref(),
            task_view.priority,
        )
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskView for API response
    let created = TaskView::from_record(&record)
        .ok_or_else(|| IpcError::internal("Failed to convert record to task view".to_string()))?;

    // Emit task.created event
    let task_json = serde_json::to_value(&created).unwrap();
    let event = Event::new(
        "task.created",
        EventSource::system("atlas").with_via("daemon"),
        json!({ "task": task_json }),
    );
    if let Err(e) = stores.atlas.record_event(event).await {
        tracing::warn!("Failed to record task.created event: {}", e);
    }

    // Extract knowledge from task content if extractor is available
    if let Some(ref extractor) = stores.extractor {
        let content = if let Some(ref desc) = created.description {
            format!("{}\n\n{}", created.title, desc)
        } else {
            created.title.clone()
        };

        if content.len() > 20 {
            let task_id = created.id_str().unwrap_or_default();
            let project = created.project.as_deref();

            match extractor.extract_from_task_content(&content, &task_id, "task", project).await {
                Ok(result) => {
                    store_extraction_results(stores, result).await;
                }
                Err(e) => {
                    tracing::warn!("Task content extraction failed: {}", e);
                }
            }
        }
    }

    Ok(serde_json::to_value(created).unwrap())
}

#[derive(Deserialize)]
struct ListTasksParams {
    project: Option<String>,
    status: Option<String>,
}

async fn handle_list_tasks(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: ListTasksParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ListTasksParams { project: None, status: None });

    let records = store
        .list_tasks(params.project.as_deref(), params.status.as_deref())
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskView for API compatibility
    let tasks: Vec<TaskView> = records
        .iter()
        .filter_map(TaskView::from_record)
        .collect();

    Ok(serde_json::to_value(tasks).unwrap())
}

#[derive(Deserialize)]
struct ReadyTasksParams {
    project: Option<String>,
}

async fn handle_ready_tasks(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: ReadyTasksParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ReadyTasksParams { project: None });

    let records = store
        .ready_tasks(params.project.as_deref())
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskView for API compatibility
    let tasks: Vec<TaskView> = records
        .iter()
        .filter_map(TaskView::from_record)
        .collect();

    Ok(serde_json::to_value(tasks).unwrap())
}

#[derive(Deserialize)]
struct GetTaskParams {
    id: String,
}

async fn handle_get_task(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Missing id: {}", e)))?;

    let record = store
        .get_record(&params.id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskView for API compatibility
    let task = record.and_then(|r| TaskView::from_record(&r));

    Ok(serde_json::to_value(task).unwrap())
}

#[derive(Deserialize)]
struct UpdateTaskParams {
    id: String,
    status: Option<String>,
    priority: Option<i32>,
    title: Option<String>,
    /// Use Some("value") to set, or explicitly null to clear
    description: Option<Option<String>>,
    /// Use Some("value") to set, or explicitly null to clear
    project: Option<Option<String>>,
}

async fn handle_update_task(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: UpdateTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let record = stores
        .atlas
        .update_task(
            &params.id,
            params.status.as_deref(),
            params.priority,
            params.title.as_deref(),
            params.description.as_ref().map(|d| d.as_deref()),
            params.project.as_ref().map(|p| p.as_deref()),
        )
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskView for API compatibility
    let updated = record.as_ref().and_then(|r| TaskView::from_record(r));

    // Emit task.updated event
    if let Some(ref task) = updated {
        let task_json = serde_json::to_value(task).unwrap();
        let mut changes = serde_json::Map::new();
        if let Some(s) = &params.status {
            changes.insert("status".to_string(), json!(s));
        }
        if let Some(p) = params.priority {
            changes.insert("priority".to_string(), json!(p));
        }
        if let Some(t) = &params.title {
            changes.insert("title".to_string(), json!(t));
        }
        if params.description.is_some() {
            changes.insert("description".to_string(), json!(task.description));
        }
        if params.project.is_some() {
            changes.insert("project".to_string(), json!(task.project));
        }
        let event = Event::new(
            "task.updated",
            EventSource::system("atlas").with_via("daemon"),
            json!({
                "task_id": params.id,
                "changes": changes,
                "snapshot": task_json
            }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record task.updated event: {}", e);
        }
    }

    Ok(serde_json::to_value(updated).unwrap())
}

#[derive(Deserialize)]
struct CloseTaskParams {
    id: String,
    /// Optional explicit status: "completed" or "cancelled". Defaults to "completed".
    status: Option<String>,
    reason: Option<String>,
}

async fn handle_close_task(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: CloseTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let record = stores
        .atlas
        .close_task(&params.id, params.status.as_deref(), params.reason.as_deref())
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskView for API compatibility
    let closed = record.as_ref().and_then(|r| TaskView::from_record(r));

    // Emit task.closed event
    if let Some(ref task) = closed {
        let task_json = serde_json::to_value(task).unwrap();
        let event = Event::new(
            "task.closed",
            EventSource::system("atlas").with_via("daemon"),
            json!({
                "task_id": params.id,
                "status": task.status.to_string(),
                "reason": params.reason,
                "snapshot": task_json
            }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record task.closed event: {}", e);
        }
    }

    Ok(serde_json::to_value(closed).unwrap())
}

#[derive(Deserialize)]
struct DeleteTaskParams {
    id: String,
    /// Reason for deletion (e.g., "duplicate", "test data") - preserved in event log
    reason: Option<String>,
}

async fn handle_delete_task(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: DeleteTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Get the task before deletion for the event
    let task_before = stores.atlas.get_record(&params.id).await
        .ok()
        .flatten()
        .and_then(|r| TaskView::from_record(&r));

    let deleted = stores
        .atlas
        .delete_task(&params.id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskView for API compatibility
    let deleted_view = deleted.as_ref().and_then(|r| TaskView::from_record(r));

    // Emit task.deleted event (using task_before since delete returns the deleted record)
    if let Some(ref task) = task_before.or(deleted_view.clone()) {
        let task_json = serde_json::to_value(task).unwrap();
        let event = Event::new(
            "task.deleted",
            EventSource::system("atlas").with_via("daemon"),
            json!({
                "task_id": params.id,
                "reason": params.reason,
                "snapshot": task_json
            }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record task.deleted event: {}", e);
        }
    }

    Ok(serde_json::to_value(deleted_view).unwrap())
}

// ========== Note Handlers ==========

#[derive(Deserialize)]
struct AddNoteParams {
    task_id: String,
    content: String,
}

async fn handle_add_note(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: AddNoteParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let note_record = stores
        .atlas
        .add_task_note(&params.task_id, &params.content)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskNoteView for API compatibility
    let note = TaskNoteView::from_record(&note_record);

    // Emit task.note_added event
    let note_json = serde_json::to_value(&note).unwrap();
    let event = Event::new(
        "task.note_added",
        EventSource::system("atlas").with_via("daemon"),
        json!({
            "task_id": params.task_id,
            "note": note_json
        }),
    );
    if let Err(e) = stores.atlas.record_event(event).await {
        tracing::warn!("Failed to record task.note_added event: {}", e);
    }

    // Extract knowledge from note content if extractor is available
    if let Some(ref extractor) = stores.extractor {
        if params.content.len() > 20 {
            // Get task to determine project context
            let project = stores.atlas.get_record(&params.task_id).await
                .ok()
                .flatten()
                .and_then(|r| r.content.get("project").and_then(|v| v.as_str()).map(|s| s.to_string()));

            let note_id = note.as_ref().and_then(|n| n.id_str()).unwrap_or_default();

            match extractor.extract_from_task_content(
                &params.content,
                &note_id,
                "task_note",
                project.as_deref(),
            ).await {
                Ok(result) => {
                    store_extraction_results(stores, result).await;
                }
                Err(e) => {
                    tracing::warn!("Task note extraction failed: {}", e);
                }
            }
        }
    }

    Ok(serde_json::to_value(note).unwrap())
}

#[derive(Deserialize)]
struct GetNotesParams {
    task_id: String,
}

async fn handle_get_notes(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetNotesParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let note_records = store
        .get_task_notes(&params.task_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskNoteView for API compatibility
    let notes: Vec<TaskNoteView> = note_records
        .iter()
        .filter_map(TaskNoteView::from_record)
        .collect();

    Ok(serde_json::to_value(notes).unwrap())
}

#[derive(Deserialize)]
struct EditNoteParams {
    note_id: String,
    content: String,
}

async fn handle_edit_note(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: EditNoteParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let record = stores
        .atlas
        .edit_task_note(&params.note_id, &params.content)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskNoteView for API compatibility
    let updated = record.as_ref().and_then(TaskNoteView::from_record);

    // Emit task.note_updated event
    if let Some(ref note) = updated {
        let note_json = serde_json::to_value(note).unwrap();
        let event = Event::new(
            "task.note_updated",
            EventSource::system("atlas").with_via("daemon"),
            json!({
                "note_id": params.note_id,
                "new_content": params.content,
                "note": note_json
            }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record task.note_updated event: {}", e);
        }
    }

    Ok(serde_json::to_value(updated).unwrap())
}

#[derive(Deserialize)]
struct DeleteNoteParams {
    note_id: String,
}

async fn handle_delete_note(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: DeleteNoteParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Get note before deletion for the event
    let note_before = stores.atlas.get_record(&params.note_id).await
        .ok()
        .flatten()
        .and_then(|r| TaskNoteView::from_record(&r));

    let record = stores
        .atlas
        .delete_task_note(&params.note_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskNoteView for API compatibility
    let deleted = record.as_ref().and_then(TaskNoteView::from_record);

    // Emit task.note_deleted event
    if let Some(ref note) = note_before.or(deleted.clone()) {
        let note_json = serde_json::to_value(note).unwrap();
        let event = Event::new(
            "task.note_deleted",
            EventSource::system("atlas").with_via("daemon"),
            json!({
                "note_id": params.note_id,
                "note": note_json
            }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record task.note_deleted event: {}", e);
        }
    }

    Ok(serde_json::to_value(deleted).unwrap())
}

// ========== Dependency Handlers ==========

#[derive(Deserialize)]
struct AddDependencyParams {
    from_id: String,
    to_id: String,
    relation: String,
}

async fn handle_add_dependency(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: AddDependencyParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let edge = stores
        .atlas
        .add_task_dependency(&params.from_id, &params.to_id, &params.relation)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskDependencyView for API compatibility
    let dep = TaskDependencyView::from_edge(&edge);

    // Emit task.dependency_added event
    let event = Event::new(
        "task.dependency_added",
        EventSource::system("atlas").with_via("daemon"),
        json!({
            "from_task_id": params.from_id,
            "to_task_id": params.to_id,
            "relation": params.relation
        }),
    );
    if let Err(e) = stores.atlas.record_event(event).await {
        tracing::warn!("Failed to record task.dependency_added event: {}", e);
    }

    Ok(serde_json::to_value(dep).unwrap())
}

#[derive(Deserialize)]
struct RemoveDependencyParams {
    from_id: String,
    to_id: String,
    relation: String,
}

async fn handle_remove_dependency(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: RemoveDependencyParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let removed = stores
        .atlas
        .remove_task_dependency(&params.from_id, &params.to_id, &params.relation)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.dependency_removed event
    if removed {
        let event = Event::new(
            "task.dependency_removed",
            EventSource::system("atlas").with_via("daemon"),
            json!({
                "from_task_id": params.from_id,
                "to_task_id": params.to_id,
                "relation": params.relation
            }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record task.dependency_removed event: {}", e);
        }
    }

    Ok(serde_json::to_value(removed).unwrap())
}

#[derive(Deserialize)]
struct GetDependenciesParams {
    task_id: String,
}

async fn handle_get_dependencies(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetDependenciesParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let edges = store
        .get_task_dependencies(&params.task_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Convert to TaskDependencyView for API compatibility
    let deps: Vec<TaskDependencyView> = edges
        .iter()
        .map(TaskDependencyView::from_edge)
        .collect();

    Ok(serde_json::to_value(deps).unwrap())
}

// ========== Memo Handlers ==========

#[derive(Deserialize)]
struct RecordMemoParams {
    content: String,
    #[serde(default)]
    user_directed: bool,
    actor: Option<String>,
}

/// Store extraction results (facts and entities) in Atlas
///
/// This is a helper function used by memo recording and task operations.
///
/// As part of the Entity→Record consolidation, this function now also links
/// facts to matching Records (in addition to legacy Entity links).
async fn store_extraction_results(
    stores: &Stores,
    result: atlas::ExtractionResult,
) {
    // First, store all entities and build a name -> entity map
    let mut entity_map: std::collections::HashMap<String, atlas::Entity> = std::collections::HashMap::new();

    for entity in result.entities {
        let entity_name = entity.name.clone();
        let project = entity.project.clone();

        // Try to find existing entity or create new
        match stores.atlas.find_entity_by_name(&entity_name, project.as_deref()).await {
            Ok(Some(existing)) => {
                // Entity already exists - merge source episodes from new entity
                if let Some(ref entity_id) = existing.id {
                    if let Err(e) = stores.atlas.add_entity_source_episodes(
                        entity_id,
                        &entity.source_episodes,
                    ).await {
                        tracing::warn!("Failed to merge entity source episodes: {}", e);
                    }
                }
                entity_map.insert(entity_name, existing);
            }
            Ok(None) => {
                match stores.atlas.create_entity(entity).await {
                    Ok(created) => {
                        entity_map.insert(entity_name, created);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to store extracted entity: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check for existing entity: {}", e);
            }
        }
    }

    // Store facts and create entity + record links
    for extracted_fact in result.facts {
        match stores.atlas.create_fact(extracted_fact.fact).await {
            Ok(created_fact) => {
                // Link fact to its referenced entities (legacy) AND matching records (new)
                if let (Some(ref fact_id), entity_refs) = (&created_fact.id, &extracted_fact.entity_refs) {
                    for entity_name in entity_refs {
                        // Legacy: Link to Entity
                        if let Some(entity) = entity_map.get(entity_name) {
                            if let Some(ref entity_id) = entity.id {
                                if let Err(e) = stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await {
                                    tracing::warn!("Failed to link fact to entity '{}': {}", entity_name, e);
                                }
                            }
                        }

                        // New: Also link to matching Record (for Entity→Record migration)
                        // Try to find a Record that matches this entity name
                        if let Ok(Some(record)) = stores.atlas.find_record_by_name_or_alias(entity_name, None).await {
                            if let Some(record_id) = record.id_str() {
                                if let Err(e) = stores.atlas.link_fact_record(fact_id, &record_id, "mentions", 1.0).await {
                                    tracing::debug!("Failed to link fact to record '{}': {}", entity_name, e);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to store extracted fact: {}", e);
            }
        }
    }
}

/// Store extraction results and return counts (facts_created, entities_created, links_created)
///
/// As part of the Entity→Record consolidation, this function now also links
/// facts to matching Records (in addition to legacy Entity links).
async fn store_extraction_results_counted(
    stores: &Stores,
    result: atlas::ExtractionResult,
) -> (usize, usize, usize) {
    let mut facts_created = 0;
    let mut entities_created = 0;
    let mut links_created = 0;

    // First, store all entities and build a name -> entity map
    let mut entity_map: std::collections::HashMap<String, atlas::Entity> = std::collections::HashMap::new();

    for entity in result.entities {
        let entity_name = entity.name.clone();
        let project = entity.project.clone();

        // Try to find existing entity or create new
        match stores.atlas.find_entity_by_name(&entity_name, project.as_deref()).await {
            Ok(Some(existing)) => {
                // Entity already exists - merge source episodes from new entity
                if let Some(ref entity_id) = existing.id {
                    if let Err(e) = stores.atlas.add_entity_source_episodes(
                        entity_id,
                        &entity.source_episodes,
                    ).await {
                        tracing::warn!("Failed to merge entity source episodes: {}", e);
                    }
                }
                entity_map.insert(entity_name, existing);
            }
            Ok(None) => {
                match stores.atlas.create_entity(entity).await {
                    Ok(created) => {
                        entities_created += 1;
                        entity_map.insert(entity_name, created);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to store extracted entity: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check for existing entity: {}", e);
            }
        }
    }

    // Store facts and create entity + record links
    for extracted_fact in result.facts {
        match stores.atlas.create_fact(extracted_fact.fact).await {
            Ok(created_fact) => {
                facts_created += 1;
                // Link fact to its referenced entities (legacy) AND matching records (new)
                if let (Some(ref fact_id), entity_refs) = (&created_fact.id, &extracted_fact.entity_refs) {
                    for entity_name in entity_refs {
                        // Legacy: Link to Entity
                        if let Some(entity) = entity_map.get(entity_name) {
                            if let Some(ref entity_id) = entity.id {
                                if stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await.is_ok() {
                                    links_created += 1;
                                }
                            }
                        }

                        // New: Also link to matching Record (for Entity→Record migration)
                        if let Ok(Some(record)) = stores.atlas.find_record_by_name_or_alias(entity_name, None).await {
                            if let Some(record_id) = record.id_str() {
                                // Record links don't count toward legacy links_created
                                let _ = stores.atlas.link_fact_record(fact_id, &record_id, "mentions", 1.0).await;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to store extracted fact: {}", e);
            }
        }
    }

    (facts_created, entities_created, links_created)
}

async fn handle_record_memo(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: RecordMemoParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let actor = params.actor.unwrap_or_else(|| "user:default".to_string());
    let source = if params.user_directed {
        MemoSource::user(actor)
    } else {
        MemoSource::agent(actor)
    };

    let memo = stores
        .atlas
        .record_memo(&params.content, source)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Extract facts from the memo if extractor is available
    if let Some(ref extractor) = stores.extractor {
        match extractor.extract_from_memo(&memo, None).await {
            Ok(result) => {
                store_extraction_results(stores, result).await;
            }
            Err(e) => {
                tracing::warn!("Fact extraction failed: {}", e);
            }
        }
    }

    Ok(serde_json::to_value(memo).unwrap())
}

#[derive(Deserialize)]
struct ListMemosParams {
    limit: Option<usize>,
}

async fn handle_list_memos(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: ListMemosParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ListMemosParams { limit: None });

    store
        .list_memos(params.limit)
        .await
        .map(|memos| serde_json::to_value(memos).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct GetMemoParams {
    id: String,
}

async fn handle_get_memo(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetMemoParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .get_memo(&params.id)
        .await
        .map(|memo| serde_json::to_value(memo).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct DeleteMemoParams {
    id: String,
}

async fn handle_delete_memo(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: DeleteMemoParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .delete_memo(&params.id)
        .await
        .map(|memo| serde_json::to_value(memo).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct PurgeMemoParams {
    id: String,
    dry_run: Option<bool>,
}

/// Purge a memo and all derived data (events, facts, entities)
async fn handle_purge_memo(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: PurgeMemoParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let dry_run = params.dry_run.unwrap_or(false);

    if dry_run {
        store
            .preview_purge_memo(&params.id)
            .await
            .map(|result| serde_json::to_value(result).unwrap())
            .map_err(|e| IpcError::internal(e.to_string()))
    } else {
        store
            .purge_memo(&params.id)
            .await
            .map(|result| serde_json::to_value(result).unwrap())
            .map_err(|e| IpcError::internal(e.to_string()))
    }
}

// ========== Event Handlers ==========

#[derive(Deserialize)]
struct ListEventsParams {
    event_type_prefix: Option<String>,
    limit: Option<usize>,
}

async fn handle_list_events(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: ListEventsParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ListEventsParams { event_type_prefix: None, limit: None });

    store
        .list_events(params.event_type_prefix.as_deref(), params.limit)
        .await
        .map(|events| serde_json::to_value(events).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct GetEventParams {
    id: String,
}

async fn handle_get_event(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetEventParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .get_event(&params.id)
        .await
        .map(|event| serde_json::to_value(event).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

// ========== Knowledge Handlers ==========

#[derive(Deserialize)]
struct KnowledgeParams {
    query: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// Optional record ID to assemble context from (e.g., a repo record)
    /// If provided, rules and skills that apply_to this record will be
    /// included in the query context.
    #[serde(default)]
    record_id: Option<String>,
}

/// Query knowledge and return an LLM-summarized answer
async fn handle_query_knowledge(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: KnowledgeParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let limit = params.limit.unwrap_or(20);

    tracing::info!("Query knowledge: query='{}', project={:?}", params.query, params.project);

    // Decompose query, generate embeddings, and hypothetical answer if LLM is available
    let (keywords, query_embedding, hypothetical_embedding, temporal_filter) = if let Some(ref extractor) = stores.extractor {
        // Decompose natural language query into keywords for BM25
        let decomposer = QueryDecomposer::new(extractor.client());
        let decomposed = match decomposer.decompose(&params.query).await {
            Ok(d) => {
                tracing::info!(
                    "Decomposed query '{}' into keywords: {:?}, temporal: {:?}",
                    params.query,
                    d.keywords,
                    d.temporal_filter
                );
                d
            }
            Err(e) => {
                tracing::warn!("Query decomposition failed, using original: {}", e);
                atlas::DecomposedQuery {
                    original: params.query.clone(),
                    keywords: vec![params.query.clone()],
                    search_text: params.query.clone(),
                    intent: atlas::QueryIntent::Factual,
                    temporal_filter: atlas::TemporalParser::parse(&params.query),
                }
            }
        };

        // Generate embedding for semantic search (use original query for semantic intent)
        let embedding = match extractor.client().embed_one(&params.query).await {
            Ok(emb) => {
                tracing::info!("Generated query embedding: {} dimensions", emb.len());
                Some(emb)
            }
            Err(e) => {
                tracing::warn!("Failed to generate query embedding: {}", e);
                None
            }
        };

        // HyDE (Hypothetical Document Embeddings) - Generate hypothetical answer and embed it
        // This bridges semantic gap between short queries and longer facts
        let hypo_embedding = {
            let generator = atlas::HypotheticalGenerator::new(extractor.client());
            match generator.generate(&params.query).await {
                Ok(hypothetical) => {
                    tracing::info!("Generated hypothetical: {}", hypothetical);
                    match extractor.client().embed_one(&hypothetical).await {
                        Ok(emb) => {
                            tracing::info!("Generated hypothetical embedding: {} dimensions", emb.len());
                            Some(emb)
                        }
                        Err(e) => {
                            tracing::warn!("Failed to embed hypothetical: {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Hypothetical generation failed: {}", e);
                    None
                }
            }
        };

        (decomposed.keywords, embedding, hypo_embedding, decomposed.temporal_filter)
    } else {
        tracing::info!("No LLM configured, using raw query");
        // Still try to parse temporal expressions even without LLM
        let temporal = atlas::TemporalParser::parse(&params.query);
        (vec![params.query.clone()], None, None, temporal)
    };

    // Extract date range from temporal filter
    let (date_start, date_end) = temporal_filter
        .map(|tf| (tf.start, tf.end))
        .unwrap_or((None, None));

    // Search each keyword separately and merge results (OR semantics for synonyms)
    let mut all_results = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for keyword in &keywords {
        let keyword_results = stores
            .atlas
            .hybrid_search_facts_temporal(
                keyword,
                query_embedding.as_deref(),
                params.project.as_deref(),
                Some(limit),
                date_start,
                date_end,
            )
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?;

        // Deduplicate by fact ID
        for result in keyword_results {
            let id = result.id.clone();
            if seen_ids.insert(id) {
                all_results.push(result);
            }
        }
    }

    // Entity-focused expansion (legacy)
    // For each keyword, find matching entities and include their linked facts
    const ENTITY_SCORE: f64 = 0.4; // Lower than direct matches to rank after them
    for keyword in &keywords {
        let entity_results = stores
            .atlas
            .expand_via_entities(
                keyword,
                params.project.as_deref(),
                ENTITY_SCORE,
                Some(10), // Limit entity-linked facts per keyword
            )
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?;

        // Add entity-linked facts (deduplicated)
        for result in entity_results {
            let id = result.id.clone();
            if seen_ids.insert(id) {
                all_results.push(result);
            }
        }
    }

    // Record-focused expansion (new - part of Entity→Record migration)
    // For each keyword, find matching Records and include their linked facts
    const RECORD_SCORE: f64 = 0.45; // Slightly higher than entity expansion
    for keyword in &keywords {
        let record_results = stores
            .atlas
            .expand_via_records(
                keyword,
                None, // Search all record types
                RECORD_SCORE,
                Some(10), // Limit record-linked facts per keyword
            )
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?;

        // Add record-linked facts (deduplicated)
        for result in record_results {
            let id = result.id.clone();
            if seen_ids.insert(id) {
                all_results.push(result);
            }
        }
    }

    // HyDE search - Search using hypothetical answer embedding
    // This helps find facts semantically similar to what the answer might look like
    if let Some(ref hypo_emb) = hypothetical_embedding {
        const HYDE_SCORE: f64 = 0.35; // Lower than direct matches

        let hyde_results = stores
            .atlas
            .vector_search_facts_temporal(
                hypo_emb,
                params.project.as_deref(),
                Some(10), // Limit HyDE results
                date_start,
                date_end,
            )
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?;

        tracing::info!("HyDE search found {} results", hyde_results.len());

        // Add HyDE results with discounted score
        for mut result in hyde_results {
            let id = result.id.clone();
            if seen_ids.insert(id) {
                // Discount the score for HyDE results
                result.score *= HYDE_SCORE;
                all_results.push(result);
            }
        }
    }

    // Sort by score descending and limit
    all_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    all_results.truncate(limit);

    let results = all_results;

    let facts_used = results.len();

    // If no facts found, return empty answer
    if results.is_empty() {
        return Ok(json!({
            "query": params.query,
            "answer": "",
            "facts_used": 0,
        }));
    }

    // Check if LLM is configured
    let Some(ref extractor) = stores.extractor else {
        // No LLM configured, return a simple concatenation of facts
        let answer = results
            .iter()
            .map(|r| format!("- {}", r.content))
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(json!({
            "query": params.query,
            "answer": answer,
            "facts_used": facts_used,
        }));
    };

    // Build context from facts
    let facts_context = results
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{}. {} (confidence: {:.0}%)", i + 1, r.content, r.confidence * 100.0))
        .collect::<Vec<_>>()
        .join("\n");

    // Assemble graph context if record_id is provided
    let graph_context = if let Some(ref record_id) = params.record_id {
        match stores.atlas.assemble_context(record_id, 3).await {
            Ok(assembly) => {
                let rules = assembly.rules();
                let skills = assembly.skills();

                let mut sections = Vec::new();

                if !rules.is_empty() {
                    let rules_text = rules
                        .iter()
                        .map(|r| {
                            let content = r.content.get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&r.name);
                            format!("- {}: {}", r.name, content)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    sections.push(format!("Rules:\n{}", rules_text));
                }

                if !skills.is_empty() {
                    let skills_text = skills
                        .iter()
                        .map(|s| {
                            let desc = s.description.as_deref().unwrap_or("");
                            format!("- {}: {}", s.name, desc)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    sections.push(format!("Available Skills:\n{}", skills_text));
                }

                if sections.is_empty() {
                    None
                } else {
                    Some(sections.join("\n\n"))
                }
            }
            Err(e) => {
                tracing::warn!("Failed to assemble context from record {}: {}", record_id, e);
                None
            }
        }
    } else {
        None
    };

    // Use LLM to summarize
    let system = if graph_context.is_some() {
        "You are a helpful assistant that answers questions based on facts from a knowledge base. \
        You also have access to rules and guidelines that apply to the current context - follow them when relevant. \
        Provide comprehensive answers that include all relevant details from the facts. \
        Cover all aspects of the question using the available information. \
        If the facts don't fully answer the question, say what you know and note what's missing."
    } else {
        "You are a helpful assistant that answers questions based on facts from a knowledge base. \
        Provide comprehensive answers that include all relevant details from the facts. \
        Cover all aspects of the question using the available information. \
        If the facts don't fully answer the question, say what you know and note what's missing."
    };

    let user = if let Some(ref graph_ctx) = graph_context {
        format!(
            "Context (rules and skills that apply):\n{}\n\nQuestion: {}\n\nKnown facts:\n{}\n\nAnswer the question based on these facts, following any relevant rules.",
            graph_ctx,
            params.query,
            facts_context
        )
    } else {
        format!(
            "Question: {}\n\nKnown facts:\n{}\n\nAnswer the question based on these facts.",
            params.query,
            facts_context
        )
    };

    let answer = extractor
        .client()
        .complete(system, &user)
        .await
        .map_err(|e| IpcError::internal(format!("LLM completion failed: {}", e)))?;

    let mut response = json!({
        "query": params.query,
        "answer": answer,
        "facts_used": facts_used,
    });

    // Include graph context info if it was used
    if graph_context.is_some() {
        response["graph_context_used"] = json!(true);
        response["record_id"] = json!(params.record_id);
    }

    Ok(response)
}

/// Search for raw facts matching a query
async fn handle_search_knowledge(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: KnowledgeParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Parse temporal expressions from query
    let temporal_filter = atlas::TemporalParser::parse(&params.query);
    let (date_start, date_end) = temporal_filter
        .map(|tf| (tf.start, tf.end))
        .unwrap_or((None, None));

    // Generate query embedding if LLM is available
    let query_embedding = if let Some(ref extractor) = stores.extractor {
        extractor
            .client()
            .embed_one(&params.query)
            .await
            .ok()
    } else {
        None
    };

    // Hybrid search (BM25 + vector if embedding available) with temporal filtering
    let results = stores
        .atlas
        .hybrid_search_facts_temporal(
            &params.query,
            query_embedding.as_deref(),
            params.project.as_deref(),
            params.limit,
            date_start,
            date_end,
        )
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    let count = results.len();
    let facts: Vec<_> = results
        .into_iter()
        .map(|r| {
            json!({
                "content": r.content,
                "fact_type": r.fact_type,
                "confidence": r.confidence,
                "score": r.score,
                "project": r.project,
                "source_episodes": r.source_episodes,
            })
        })
        .collect();

    Ok(json!({
        "query": params.query,
        "results": facts,
        "count": count,
    }))
}

/// Get knowledge system status (diagnostic)
///
/// Returns comprehensive health information including:
/// - Schema status and validation
/// - Fact statistics and embedding coverage
/// - LLM configuration status
/// - Overall health assessment
async fn handle_knowledge_status(stores: &Stores) -> Result<serde_json::Value, IpcError> {
    // Count facts with/without embeddings
    let (with_embeddings, without_embeddings) = stores
        .atlas
        .count_fact_embeddings()
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Count total facts
    let total_facts = with_embeddings + without_embeddings;

    // Check if LLM is configured
    let llm_configured = stores.extractor.is_some();

    // Get schema status for Atlas database
    let schema_status = db::get_schema_status(stores.atlas.db().client(), "atlas")
        .await
        .map_err(|e| IpcError::internal(format!("Schema status check failed: {}", e)))?;

    // Count records (memos, tasks, etc.)
    let memo_count = stores
        .atlas
        .list_memos(Some(1))
        .await
        .map(|m| if m.is_empty() { 0 } else { 1 }) // Just check if any exist
        .unwrap_or(0);

    // Determine overall health
    let schema_healthy = schema_status.valid;
    let embeddings_healthy = total_facts == 0 || (with_embeddings as f64 / total_facts as f64) >= 0.5;
    let overall_healthy = schema_healthy && llm_configured && embeddings_healthy;

    let health_issues: Vec<&str> = {
        let mut issues = Vec::new();
        if !schema_healthy {
            issues.push("Schema validation failed");
        }
        if !llm_configured {
            issues.push("LLM not configured - extraction disabled");
        }
        if !embeddings_healthy && total_facts > 0 {
            issues.push("Many facts missing embeddings - semantic search degraded");
        }
        issues
    };

    Ok(json!({
        "healthy": overall_healthy,
        "issues": health_issues,
        "schema": {
            "version": schema_status.version,
            "latest_version": schema_status.latest_version,
            "up_to_date": schema_status.up_to_date,
            "pending_migrations": schema_status.pending_migrations,
            "valid": schema_status.valid,
            "valid_tables": schema_status.valid_tables,
            "missing_tables": schema_status.missing_tables,
        },
        "facts": {
            "total": total_facts,
            "with_embeddings": with_embeddings,
            "without_embeddings": without_embeddings,
            "embedding_coverage": if total_facts > 0 {
                format!("{:.1}%", (with_embeddings as f64 / total_facts as f64) * 100.0)
            } else {
                "N/A".to_string()
            },
        },
        "llm_configured": llm_configured,
        "has_data": memo_count > 0 || total_facts > 0,
    }))
}

// ========== Backfill Handlers ==========

#[derive(Deserialize)]
struct BackfillEmbeddingsParams {
    #[serde(default = "default_backfill_batch_size")]
    batch_size: usize,
}

fn default_backfill_batch_size() -> usize {
    50
}

async fn handle_backfill_embeddings(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: BackfillEmbeddingsParams = serde_json::from_value(request.params.clone())
        .unwrap_or(BackfillEmbeddingsParams { batch_size: 50 });

    let extractor = match &stores.extractor {
        Some(e) => e,
        None => {
            return Err(IpcError::internal("LLM not configured - cannot generate embeddings".to_string()));
        }
    };

    // Get facts without embeddings
    let facts = stores
        .atlas
        .get_facts_without_embeddings(Some(params.batch_size))
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    let facts_processed = facts.len();
    let mut facts_updated = 0;

    // Generate embeddings in batches
    if !facts.is_empty() {
        let texts: Vec<String> = facts.iter().map(|f| f.content.clone()).collect();

        match extractor.client().embed(texts).await {
            Ok(embeddings) => {
                for (fact, embedding) in facts.iter().zip(embeddings.into_iter()) {
                    if let Some(ref id) = fact.id {
                        if stores.atlas.update_fact_embedding(id, embedding).await.is_ok() {
                            facts_updated += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to generate embeddings: {}", e);
            }
        }
    }

    // Count remaining facts without embeddings
    let (_, facts_remaining) = stores
        .atlas
        .count_fact_embeddings()
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(json!({
        "facts_processed": facts_processed,
        "facts_updated": facts_updated,
        "facts_remaining": facts_remaining,
    }))
}

// ========== Entity Handlers ==========

#[derive(Deserialize)]
struct ListEntitiesParams {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    entity_type: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn handle_list_entities(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: ListEntitiesParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ListEntitiesParams {
            project: None,
            entity_type: None,
            limit: None,
        });

    let entities = store
        .list_entities(
            params.project.as_deref(),
            params.entity_type.as_deref(),
            params.limit,
        )
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(serde_json::to_value(entities).unwrap())
}

#[derive(Deserialize)]
struct GetEntityFactsParams {
    name: String,
    #[serde(default)]
    project: Option<String>,
}

async fn handle_get_entity_facts(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetEntityFactsParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let facts = store
        .get_facts_for_entity_name(&params.name, params.project.as_deref())
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(json!({
        "entity": params.name,
        "facts": facts,
        "count": facts.len(),
    }))
}

#[derive(Deserialize)]
struct GetRecordFactsParams {
    /// Record name or alias to look up
    name: String,
    /// Optional record type filter for more precise matching
    #[serde(default)]
    record_type: Option<String>,
}

/// Get facts linked to a Record (part of Entity→Record migration)
///
/// This is the Record-based equivalent of get_entity_facts.
/// Uses find_record_by_name_or_alias for flexible identity resolution.
async fn handle_get_record_facts(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetRecordFactsParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let facts = store
        .get_facts_for_record_name(&params.name, params.record_type.as_deref())
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(json!({
        "record": params.name,
        "facts": facts,
        "count": facts.len(),
    }))
}

#[derive(Deserialize)]
struct GetRelatedFactsParams {
    fact_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

async fn handle_get_related_facts(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetRelatedFactsParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Try both Entity-based and Record-based related facts
    let entity_facts = store
        .get_related_facts(&params.fact_id, params.limit)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    let record_facts = store
        .get_related_facts_via_records(&params.fact_id, params.limit)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Merge and deduplicate results
    let mut all_facts = entity_facts;
    let seen_ids: std::collections::HashSet<_> = all_facts.iter()
        .filter_map(|f| f.id.as_ref().map(|t| t.to_string()))
        .collect();

    for fact in record_facts {
        if let Some(ref id) = fact.id {
            if !seen_ids.contains(&id.to_string()) {
                all_facts.push(fact);
            }
        }
    }

    // Apply limit if specified
    if let Some(limit) = params.limit {
        all_facts.truncate(limit);
    }

    Ok(json!({
        "fact_id": params.fact_id,
        "related_facts": all_facts,
        "count": all_facts.len(),
    }))
}

// ========== Record/Graph Handlers ==========

use atlas::EdgeRelation;

#[derive(Deserialize)]
struct ListRecordsParams {
    #[serde(default)]
    record_type: Option<String>,
    #[serde(default)]
    include_deleted: bool,
    #[serde(default)]
    limit: Option<usize>,
}

async fn handle_list_records(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: ListRecordsParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ListRecordsParams {
            record_type: None,
            include_deleted: false,
            limit: None,
        });

    let records = store
        .list_records(params.record_type.as_deref(), params.include_deleted, params.limit)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(serde_json::to_value(records).unwrap())
}

#[derive(Deserialize)]
struct GetRecordParams {
    id: String,
}

async fn handle_get_record(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetRecordParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let record = store
        .get_record(&params.id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(serde_json::to_value(record).unwrap())
}

#[derive(Deserialize)]
struct CreateRecordParams {
    record_type: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content: Option<serde_json::Value>,
}

async fn handle_create_record(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: CreateRecordParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Parse record type
    let record_type: RecordType = params.record_type.parse()
        .map_err(|e: String| IpcError::invalid_params(e))?;

    let mut record = Record::new(record_type, &params.name);
    if let Some(desc) = params.description {
        record = record.with_description(desc);
    }
    if let Some(content) = params.content {
        record = record.with_content(content);
    }

    let created = stores.atlas
        .create_record(record)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit record.created event
    let record_json = serde_json::to_value(&created).unwrap();
    let event = Event::new(
        "record.created",
        EventSource::system("atlas").with_via("daemon"),
        json!({ "record": record_json }),
    );
    if let Err(e) = stores.atlas.record_event(event).await {
        tracing::warn!("Failed to record record.created event: {}", e);
    }

    Ok(serde_json::to_value(created).unwrap())
}

#[derive(Deserialize)]
struct UpdateRecordParams {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    record_type: Option<String>,
}

async fn handle_update_record(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: UpdateRecordParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Get the old record for diffing
    let old_record = stores.atlas
        .get_record(&params.id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    let record = stores.atlas
        .update_record(
            &params.id,
            params.name.as_deref(),
            params.description.as_deref(),
            params.content.clone(),
            params.record_type.as_deref(),
        )
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit record.updated event with diff
    if let (Some(ref old), Some(ref new)) = (old_record, &record) {
        let mut changes = serde_json::Map::new();
        if params.name.is_some() && params.name.as_deref() != Some(&old.name) {
            changes.insert("name".to_string(), json!({
                "old": old.name,
                "new": new.name
            }));
        }
        if params.description.is_some() && params.description != old.description {
            changes.insert("description".to_string(), json!({
                "old": old.description,
                "new": new.description
            }));
        }
        if params.content.is_some() {
            changes.insert("content".to_string(), json!({
                "old": old.content,
                "new": new.content
            }));
        }

        if !changes.is_empty() {
            let record_json = serde_json::to_value(new).unwrap();
            let event = Event::new(
                "record.updated",
                EventSource::system("atlas").with_via("daemon"),
                json!({
                    "record_id": params.id,
                    "record_type": new.record_type,
                    "changes": changes,
                    "snapshot": record_json
                }),
            );
            if let Err(e) = stores.atlas.record_event(event).await {
                tracing::warn!("Failed to record record.updated event: {}", e);
            }
        }
    }

    Ok(serde_json::to_value(record).unwrap())
}

#[derive(Deserialize)]
struct DeleteRecordParams {
    id: String,
}

async fn handle_delete_record(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: DeleteRecordParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let record = stores.atlas
        .delete_record(&params.id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit record.deleted event
    if let Some(ref deleted) = record {
        let record_json = serde_json::to_value(deleted).unwrap();
        let event = Event::new(
            "record.deleted",
            EventSource::system("atlas").with_via("daemon"),
            json!({
                "record_id": params.id,
                "record_type": deleted.record_type,
                "snapshot": record_json
            }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record record.deleted event: {}", e);
        }
    }

    Ok(serde_json::to_value(record).unwrap())
}

#[derive(Deserialize)]
struct PurgeRecordParams {
    id: String,
    dry_run: Option<bool>,
}

/// Purge a record and all derived data (events, facts, entities, edges)
async fn handle_purge_record(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: PurgeRecordParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let dry_run = params.dry_run.unwrap_or(false);

    if dry_run {
        store
            .preview_purge_record(&params.id)
            .await
            .map(|result| serde_json::to_value(result).unwrap())
            .map_err(|e| IpcError::internal(e.to_string()))
    } else {
        store
            .purge_record(&params.id)
            .await
            .map(|result| serde_json::to_value(result).unwrap())
            .map_err(|e| IpcError::internal(e.to_string()))
    }
}

#[derive(Deserialize)]
struct MergeRecordsParams {
    source_id: String,
    target_id: String,
    #[serde(default)]
    merge_content: bool,
    #[serde(default)]
    merge_description: bool,
}

/// Merge two records, redirecting edges and marking source as superseded
async fn handle_merge_records(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: MergeRecordsParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let result = stores.atlas
        .merge_records(
            &params.source_id,
            &params.target_id,
            params.merge_content,
            params.merge_description,
        )
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit record.merged event for provenance
    let event = Event::new(
        "record.merged",
        EventSource::system("atlas").with_via("daemon"),
        json!({
            "source_id": params.source_id,
            "target_id": params.target_id,
            "source_type": result.source_type,
            "edges_redirected": result.edges_redirected,
            "edges_skipped": result.edges_skipped,
            "content_merged": result.content_merged,
            "description_merged": result.description_merged
        }),
    );
    if let Err(e) = stores.atlas.record_event(event).await {
        tracing::warn!("Failed to record record.merged event: {}", e);
    }

    Ok(serde_json::to_value(result).unwrap())
}

#[derive(Deserialize)]
struct PreviewMergeParams {
    source_id: String,
    target_id: String,
}

/// Preview what a merge would do without executing it
async fn handle_preview_merge_records(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: PreviewMergeParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .preview_merge_records(&params.source_id, &params.target_id)
        .await
        .map(|result| serde_json::to_value(result).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct CreateEdgeParams {
    source: String,
    target: String,
    relation: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

async fn handle_create_edge(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: CreateEdgeParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Parse relation type
    let relation: EdgeRelation = params.relation.parse()
        .map_err(|e: String| IpcError::invalid_params(e))?;

    let edge = stores.atlas
        .create_edge(&params.source, &params.target, relation, params.metadata)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit record.edge_created event
    let edge_id = edge.id_str().unwrap_or_default();
    let event = Event::new(
        "record.edge_created",
        EventSource::system("atlas").with_via("daemon"),
        json!({
            "source_record_id": params.source,
            "target_record_id": params.target,
            "relation": params.relation,
            "edge_id": edge_id
        }),
    );
    if let Err(e) = stores.atlas.record_event(event).await {
        tracing::warn!("Failed to record record.edge_created event: {}", e);
    }

    Ok(serde_json::to_value(edge).unwrap())
}

#[derive(Deserialize)]
struct ListEdgesParams {
    id: String,
    #[serde(default = "default_edge_direction")]
    direction: String,
}

fn default_edge_direction() -> String {
    "both".to_string()
}

async fn handle_list_edges(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: ListEdgesParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Default to showing only current edges (not superseded)
    let edges = match params.direction.as_str() {
        "from" => store.get_edges_from(&params.id, None, true).await
            .map_err(|e| IpcError::internal(e.to_string()))?,
        "to" => store.get_edges_to(&params.id, None, true).await
            .map_err(|e| IpcError::internal(e.to_string()))?,
        "both" | _ => {
            let from = store.get_edges_from(&params.id, None, true).await
                .map_err(|e| IpcError::internal(e.to_string()))?;
            let to = store.get_edges_to(&params.id, None, true).await
                .map_err(|e| IpcError::internal(e.to_string()))?;
            let mut all = from;
            all.extend(to);
            all
        }
    };

    Ok(serde_json::to_value(edges).unwrap())
}

#[derive(Deserialize)]
struct DeleteEdgeParams {
    id: String,
}

async fn handle_delete_edge(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: DeleteEdgeParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let edge = stores.atlas
        .delete_edge(&params.id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit record.edge_deleted event
    if edge.is_some() {
        let event = Event::new(
            "record.edge_deleted",
            EventSource::system("atlas").with_via("daemon"),
            json!({ "edge_id": params.id }),
        );
        if let Err(e) = stores.atlas.record_event(event).await {
            tracing::warn!("Failed to record record.edge_deleted event: {}", e);
        }
    }

    Ok(serde_json::to_value(edge).unwrap())
}

#[derive(Deserialize)]
struct AssembleContextParams {
    id: String,
    #[serde(default = "default_context_depth")]
    depth: usize,
}

fn default_context_depth() -> usize {
    3
}

async fn handle_assemble_context(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: AssembleContextParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let context = store
        .assemble_context(&params.id, params.depth)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(serde_json::to_value(context).unwrap())
}

// ========== Extraction Handlers ==========

#[derive(Deserialize)]
struct ExtractFactsParams {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_batch_size")]
    batch_size: usize,
}

fn default_batch_size() -> usize {
    20
}

async fn handle_extract_facts(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: ExtractFactsParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ExtractFactsParams { project: None, batch_size: 20 });

    let extractor = match &stores.extractor {
        Some(e) => e,
        None => {
            return Err(IpcError::internal("LLM not configured - cannot extract facts".to_string()));
        }
    };

    let mut facts_created = 0;
    let mut entities_created = 0;
    let mut memos_processed = 0;
    let mut links_created = 0;

    // Get all memos
    let memos = stores
        .atlas
        .list_memos(Some(params.batch_size))
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    for memo in memos {
        memos_processed += 1;
        match extractor.extract_from_memo(&memo, params.project.as_deref()).await {
            Ok(result) => {
                // First, store all entities and build a name -> entity map
                let mut entity_map: std::collections::HashMap<String, atlas::Entity> = std::collections::HashMap::new();

                for entity in result.entities {
                    let entity_name = entity.name.clone();
                    let project = entity.project.clone();

                    match stores.atlas.find_entity_by_name(&entity_name, project.as_deref()).await {
                        Ok(Some(existing)) => {
                            // Merge source episodes from new entity
                            if let Some(ref entity_id) = existing.id {
                                let _ = stores.atlas.add_entity_source_episodes(
                                    entity_id,
                                    &entity.source_episodes,
                                ).await;
                            }
                            entity_map.insert(entity_name, existing);
                        }
                        Ok(None) => {
                            if let Ok(created) = stores.atlas.create_entity(entity).await {
                                entities_created += 1;
                                entity_map.insert(entity_name, created);
                            }
                        }
                        Err(_) => {}
                    }
                }

                // Store facts and create entity + record links
                for extracted_fact in result.facts {
                    if let Ok(created_fact) = stores.atlas.create_fact(extracted_fact.fact).await {
                        facts_created += 1;

                        if let Some(ref fact_id) = created_fact.id {
                            for entity_name in &extracted_fact.entity_refs {
                                // Legacy: Link to Entity
                                if let Some(entity) = entity_map.get(entity_name) {
                                    if let Some(ref entity_id) = entity.id {
                                        if stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await.is_ok() {
                                            links_created += 1;
                                        }
                                    }
                                }

                                // New: Also link to matching Record (for Entity→Record migration)
                                if let Ok(Some(record)) = stores.atlas.find_record_by_name_or_alias(entity_name, None).await {
                                    if let Some(record_id) = record.id_str() {
                                        let _ = stores.atlas.link_fact_record(fact_id, &record_id, "mentions", 1.0).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Extraction failed for memo {}: {}", memo.id_str().unwrap_or_default(), e);
            }
        }
    }

    Ok(json!({
        "memos_processed": memos_processed,
        "facts_created": facts_created,
        "entities_created": entities_created,
        "links_created": links_created,
    }))
}

// ============================================================================
// Record Extraction Handlers (new Records + Links pipeline)
// ============================================================================

#[derive(Deserialize)]
struct ExtractRecordsFromMemoParams {
    memo_id: String,
    #[serde(default = "default_threshold")]
    threshold: f32,
    #[serde(default)]
    dry_run: bool,
    #[serde(default = "default_true")]
    multi_step: bool,
}

fn default_true() -> bool {
    true
}

fn default_threshold() -> f32 {
    0.5
}

#[derive(Serialize)]
struct ExtractRecordsResponse {
    extraction: atlas::RecordExtractionResult,
    processing: Option<atlas::ExtractionProcessingResult>,
}

async fn handle_extract_records_from_memo(
    request: &Request,
    stores: &Stores,
) -> Result<serde_json::Value, IpcError> {
    let params: ExtractRecordsFromMemoParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(e.to_string()))?;

    // Get the LLM client from extractor
    let llm = match &stores.extractor {
        Some(e) => e.client(),
        None => {
            return Err(IpcError::internal(
                "LLM not configured - cannot extract records".to_string(),
            ));
        }
    };

    // Get the memo
    let memo = stores
        .atlas
        .get_memo(&params.memo_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?
        .ok_or_else(|| IpcError::internal(format!("Memo not found: {}", params.memo_id)))?;

    // Extract using single-shot or multi-step extractor
    let result = if params.multi_step {
        // Multi-step: entity extraction → record matching → action decision
        let extractor = MultiStepExtractor::new(llm, &stores.atlas);
        extractor
            .extract(&memo.content, &params.memo_id)
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?
    } else {
        // Single-shot: one LLM call with context
        let context = stores
            .atlas
            .get_extraction_context()
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?;

        let extractor = atlas::RecordExtractor::new(llm);
        extractor
            .extract_from_memo(&memo.content, &params.memo_id, &context)
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?
    };

    // If not dry run, process the results (create records, edges, etc.)
    let processing = if !params.dry_run {
        Some(stores
            .atlas
            .process_extraction_results(&result, params.threshold)
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?)
    } else {
        None
    };

    // Return combined response
    let response = ExtractRecordsResponse {
        extraction: result,
        processing,
    };
    serde_json::to_value(&response).map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct BackfillRecordsParams {
    #[serde(default = "default_records_batch_size")]
    batch_size: usize,
    #[serde(default = "default_threshold")]
    threshold: f32,
}

fn default_records_batch_size() -> usize {
    50
}

async fn handle_backfill_records(
    request: &Request,
    stores: &Stores,
) -> Result<serde_json::Value, IpcError> {
    let params: BackfillRecordsParams = serde_json::from_value(request.params.clone())
        .unwrap_or(BackfillRecordsParams {
            batch_size: 50,
            threshold: 0.5,
        });

    // Get the LLM client from extractor
    let llm = match &stores.extractor {
        Some(e) => e.client(),
        None => {
            return Err(IpcError::internal(
                "LLM not configured - cannot backfill records".to_string(),
            ));
        }
    };

    let mut memos_processed = 0;
    let mut records_created = 0;
    let mut records_updated = 0;
    let mut edges_created = 0;
    let mut skipped_count = 0;
    let mut all_questions: Vec<atlas::ExtractionQuestion> = Vec::new();

    // Get memos to process
    let memos = stores
        .atlas
        .list_memos(Some(params.batch_size))
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Use multi-step extractor (better accuracy via entity matching)
    let extractor = MultiStepExtractor::new(llm, &stores.atlas);

    for memo in memos {
        memos_processed += 1;
        let memo_id = memo.id_str().unwrap_or_default();

        // Extract records from this memo using multi-step pipeline
        match extractor.extract(&memo.content, &memo_id).await {
            Ok(result) => {
                // Process the results
                match stores
                    .atlas
                    .process_extraction_results(&result, params.threshold)
                    .await
                {
                    Ok(processing_result) => {
                        records_created += processing_result.created_records.len();
                        records_updated += processing_result.updated_records.len();
                        edges_created += processing_result.created_edges.len();
                        skipped_count += processing_result.skipped_low_confidence.len();
                        all_questions.extend(processing_result.questions);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to process extraction results for memo {}: {}", memo_id, e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Extraction failed for memo {}: {}", memo_id, e);
            }
        }
    }

    Ok(json!({
        "memos_processed": memos_processed,
        "records_created": records_created,
        "records_updated": records_updated,
        "edges_created": edges_created,
        "skipped_count": skipped_count,
        "questions": all_questions,
    }))
}

#[derive(Deserialize)]
struct RebuildKnowledgeParams {
    #[serde(default)]
    project: Option<String>,
}

async fn handle_rebuild_knowledge(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: RebuildKnowledgeParams = serde_json::from_value(request.params.clone())
        .unwrap_or(RebuildKnowledgeParams { project: None });

    let extractor = match &stores.extractor {
        Some(e) => e,
        None => {
            return Err(IpcError::internal("LLM not configured - cannot rebuild knowledge".to_string()));
        }
    };

    // Step 1: Delete all derived data
    let (facts_deleted, entities_deleted) = stores
        .atlas
        .delete_derived_data(params.project.as_deref())
        .await
        .map_err(|e| IpcError::internal(format!("Failed to delete derived data: {}", e)))?;

    // Step 2: Re-extract from all memos
    let memos = stores
        .atlas
        .list_memos(None) // Get all memos
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    let mut facts_created = 0;
    let mut entities_created = 0;
    let mut memos_processed = 0;

    let mut links_created = 0;

    for memo in memos {
        memos_processed += 1;
        match extractor.extract_from_memo(&memo, params.project.as_deref()).await {
            Ok(result) => {
                // First, store all entities and build a name -> entity map
                let mut entity_map: std::collections::HashMap<String, atlas::Entity> = std::collections::HashMap::new();

                for entity in result.entities {
                    let entity_name = entity.name.clone();
                    let project = entity.project.clone();

                    match stores.atlas.find_entity_by_name(&entity_name, project.as_deref()).await {
                        Ok(Some(existing)) => {
                            // Merge source episodes from new entity
                            if let Some(ref entity_id) = existing.id {
                                let _ = stores.atlas.add_entity_source_episodes(
                                    entity_id,
                                    &entity.source_episodes,
                                ).await;
                            }
                            entity_map.insert(entity_name, existing);
                        }
                        Ok(None) => {
                            if let Ok(created) = stores.atlas.create_entity(entity).await {
                                entities_created += 1;
                                entity_map.insert(entity_name, created);
                            }
                        }
                        Err(_) => {}
                    }
                }

                // Store facts and create entity + record links
                for extracted_fact in result.facts {
                    if let Ok(created_fact) = stores.atlas.create_fact(extracted_fact.fact).await {
                        facts_created += 1;

                        // Link fact to its referenced entities (legacy) AND matching records (new)
                        if let Some(ref fact_id) = created_fact.id {
                            for entity_name in &extracted_fact.entity_refs {
                                // Legacy: Link to Entity
                                if let Some(entity) = entity_map.get(entity_name) {
                                    if let Some(ref entity_id) = entity.id {
                                        if stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await.is_ok() {
                                            links_created += 1;
                                        }
                                    }
                                }

                                // New: Also link to matching Record (for Entity→Record migration)
                                if let Ok(Some(record)) = stores.atlas.find_record_by_name_or_alias(entity_name, None).await {
                                    if let Some(record_id) = record.id_str() {
                                        let _ = stores.atlas.link_fact_record(fact_id, &record_id, "mentions", 1.0).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Extraction failed for memo {}: {}", memo.id_str().unwrap_or_default(), e);
            }
        }
    }

    // Step 3: Re-extract from all tasks (title + description)
    let tasks = stores
        .forge
        .list_tasks(None, None) // Get all tasks
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    let mut tasks_processed = 0;

    for task in tasks {
        // Build content from title and description
        let content = if let Some(ref desc) = task.description {
            format!("{}\n\n{}", task.title, desc)
        } else {
            task.title.clone()
        };

        // Only extract if there's meaningful content
        if content.len() > 20 {
            tasks_processed += 1;
            let task_id = task.id_str().unwrap_or_default();
            let project = task.project.as_deref();

            match extractor.extract_from_task_content(&content, &task_id, "task", project).await {
                Ok(result) => {
                    let (f, e, l) = store_extraction_results_counted(stores, result).await;
                    facts_created += f;
                    entities_created += e;
                    links_created += l;
                }
                Err(e) => {
                    tracing::warn!("Extraction failed for task {}: {}", task_id, e);
                }
            }
        }
    }

    // Step 4: Re-extract from all task notes
    let notes = stores
        .forge
        .list_all_notes()
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    let mut notes_processed = 0;

    for note in notes {
        // Only extract if there's meaningful content
        if note.content.len() > 20 {
            notes_processed += 1;
            let note_id = note.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();

            // Get the task to determine project context
            let task_id_str = note.task_id.id.to_raw();
            let project = stores.forge.get_task(&task_id_str).await
                .ok()
                .flatten()
                .and_then(|t| t.project);

            match extractor.extract_from_task_content(
                &note.content,
                &note_id,
                "task_note",
                project.as_deref(),
            ).await {
                Ok(result) => {
                    let (f, e, l) = store_extraction_results_counted(stores, result).await;
                    facts_created += f;
                    entities_created += e;
                    links_created += l;
                }
                Err(e) => {
                    tracing::warn!("Extraction failed for note {}: {}", note_id, e);
                }
            }
        }
    }

    Ok(json!({
        "facts_deleted": facts_deleted,
        "entities_deleted": entities_deleted,
        "memos_processed": memos_processed,
        "tasks_processed": tasks_processed,
        "notes_processed": notes_processed,
        "facts_created": facts_created,
        "entities_created": entities_created,
        "links_created": links_created,
    }))
}

// ========== Migration Handlers (Forge to Atlas) ==========

/// Migrate tasks from Forge to Atlas records
///
/// This is a one-time migration that:
/// 1. Reads all tasks from Forge
/// 2. Creates corresponding records in Atlas with record_type = "task"
/// 3. Migrates task notes as linked document records
/// 4. Migrates task dependencies as edges
async fn handle_migrate_tasks_to_records(stores: &Stores) -> Result<serde_json::Value, IpcError> {
    use atlas::{TaskContent, RecordType, Record, task_relations};
    use std::collections::HashMap;

    let mut tasks_migrated = 0;
    let mut notes_migrated = 0;
    let mut deps_migrated = 0;
    let mut skipped = 0;
    let mut errors: Vec<String> = Vec::new();

    // Build ID mapping: forge_id -> atlas_record_id
    let mut id_map: HashMap<String, String> = HashMap::new();

    // Get all tasks from Forge
    let forge_tasks = stores
        .forge
        .list_tasks(None, None)
        .await
        .map_err(|e| IpcError::internal(format!("Failed to list Forge tasks: {}", e)))?;

    // === PASS 1: Migrate tasks and notes, build ID mapping ===
    for forge_task in &forge_tasks {
        let forge_id = forge_task.id_str().unwrap_or_default();

        // Check if already migrated (record with same name exists)
        if let Ok(Some(existing)) = stores.atlas.get_record_by_type_name("task", &forge_task.title).await {
            tracing::debug!("Task already migrated: {} ({})", forge_task.title, forge_id);
            // Still record the mapping for dependency resolution
            if let Some(record_id) = existing.id_str() {
                id_map.insert(forge_id.clone(), record_id);
            }
            skipped += 1;
            continue;
        }

        // Create task content
        let status = match forge_task.status {
            TaskStatus::Pending => atlas::TaskStatus::Pending,
            TaskStatus::InProgress => atlas::TaskStatus::InProgress,
            TaskStatus::Blocked => atlas::TaskStatus::Blocked,
            TaskStatus::NeedsDiscussion => atlas::TaskStatus::NeedsDiscussion,
            TaskStatus::Completed => atlas::TaskStatus::Completed,
            TaskStatus::Cancelled => atlas::TaskStatus::Cancelled,
        };

        let content = TaskContent {
            status,
            priority: forge_task.priority,
            impact: atlas::Impact::from_priority(forge_task.priority),
            urgency: atlas::Urgency::default(),
            project: forge_task.project.clone(),
            completed_at: forge_task.completed_at.clone(),
        };

        // Create the record with preserved timestamps
        let mut record = Record::new(RecordType::Task, &forge_task.title)
            .with_content(content.to_json())
            .with_timestamps(forge_task.created_at.clone(), forge_task.updated_at.clone());

        if let Some(ref desc) = forge_task.description {
            record = record.with_description(desc);
        }

        let created = match stores.atlas.create_record(record).await {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("Failed to create task {}: {}", forge_id, e));
                continue;
            }
        };

        let record_id = created.id_str().unwrap_or_default();
        id_map.insert(forge_id.clone(), record_id.clone());
        tasks_migrated += 1;

        // Migrate notes for this task with preserved timestamps
        if let Ok(notes) = stores.forge.get_notes(&forge_id).await {
            for note in notes {
                match stores.atlas.add_task_note_with_timestamps(
                    &record_id,
                    &note.content,
                    note.created_at.clone(),
                    note.updated_at.clone(),
                ).await {
                    Ok(_) => notes_migrated += 1,
                    Err(e) => {
                        errors.push(format!("Failed to migrate note for task {}: {}", forge_id, e));
                    }
                }
            }
        }
    }

    // === PASS 2: Migrate dependencies using ID mapping ===
    for forge_task in &forge_tasks {
        let forge_id = forge_task.id_str().unwrap_or_default();

        if let Ok(deps) = stores.forge.get_dependencies(&forge_id).await {
            for dep in deps {
                let from_forge_id = dep.from_task.id.to_raw();
                let to_forge_id = dep.to_task.id.to_raw();

                // Only migrate if this is the "from" task to avoid duplicates
                if from_forge_id != forge_id {
                    continue;
                }

                // Look up both IDs in our mapping
                let from_record_id = match id_map.get(&from_forge_id) {
                    Some(id) => id.clone(),
                    None => {
                        errors.push(format!("Dependency source not found: {}", from_forge_id));
                        continue;
                    }
                };

                let to_record_id = match id_map.get(&to_forge_id) {
                    Some(id) => id.clone(),
                    None => {
                        errors.push(format!("Dependency target not found: {}", to_forge_id));
                        continue;
                    }
                };

                let relation = match dep.relation.as_str() {
                    "blocks" => task_relations::BLOCKS,
                    "blocked_by" => task_relations::BLOCKED_BY,
                    _ => task_relations::RELATES_TO,
                };

                // Create the dependency edge with preserved timestamp
                match stores.atlas.create_edge_with_details(
                    &from_record_id,
                    &to_record_id,
                    relation,
                    dep.created_at.clone(),
                ).await {
                    Ok(_) => deps_migrated += 1,
                    Err(e) => {
                        errors.push(format!("Failed to create dependency edge {} -> {}: {}",
                            from_forge_id, to_forge_id, e));
                    }
                }
            }
        }
    }

    tracing::info!(
        "Migration complete: {} tasks, {} notes, {} dependencies (skipped {} already migrated, {} errors)",
        tasks_migrated, notes_migrated, deps_migrated, skipped, errors.len()
    );

    if !errors.is_empty() {
        tracing::warn!("Migration errors: {:?}", errors);
    }

    Ok(json!({
        "tasks_migrated": tasks_migrated,
        "notes_migrated": notes_migrated,
        "dependencies_migrated": deps_migrated,
        "skipped_already_migrated": skipped,
        "errors": errors
    }))
}

// ========== Test Import Handler (for migration testing) ==========

/// Import a task directly into Forge with preserved timestamps.
/// This is for testing the migration - NOT for production use.
async fn handle_test_import_forge_task(
    request: &Request,
    stores: &Stores,
) -> Result<serde_json::Value, IpcError> {
    use forge::Task;

    // Accept raw Forge Task format with timestamps
    let task: Task = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid task: {}", e)))?;

    let created = stores
        .forge
        .create_task(task)
        .await
        .map_err(|e| IpcError::internal(format!("Failed to create Forge task: {}", e)))?;

    Ok(serde_json::to_value(&created).unwrap())
}

/// Import a task note directly into Forge with preserved timestamps.
async fn handle_test_import_forge_note(
    request: &Request,
    stores: &Stores,
) -> Result<serde_json::Value, IpcError> {
    use forge::TaskNote;

    let note: TaskNote = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid note: {}", e)))?;

    let created = stores
        .forge
        .create_note(note)
        .await
        .map_err(|e| IpcError::internal(format!("Failed to create Forge note: {}", e)))?;

    Ok(serde_json::to_value(&created).unwrap())
}

/// Import a task dependency directly into Forge with preserved timestamps.
async fn handle_test_import_forge_dependency(
    request: &Request,
    stores: &Stores,
) -> Result<serde_json::Value, IpcError> {
    use forge::TaskDependency;

    let dep: TaskDependency = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid dependency: {}", e)))?;

    let created = stores
        .forge
        .create_dependency(dep)
        .await
        .map_err(|e| IpcError::internal(format!("Failed to create Forge dependency: {}", e)))?;

    Ok(serde_json::to_value(&created).unwrap())
}

// ========== Cortex Handlers ==========

/// Helper to persist full worker status to the database
///
/// This should be called after any operation that changes worker state
/// to ensure the database reflects the current in-memory state.
async fn persist_worker_status(stores: &Arc<Stores>, worker_id: &WorkerId) {
    if let Ok(status) = stores.workers.status(worker_id).await {
        let state_str = match &status.state {
            WorkerState::Starting => "starting",
            WorkerState::Ready => "ready",
            WorkerState::Working => "working",
            WorkerState::Idle => "idle",
            WorkerState::Stopped => "stopped",
            WorkerState::Error(_) => "error",
        };
        let error_msg = match &status.state {
            WorkerState::Error(msg) => Some(msg.as_str()),
            _ => None,
        };
        if let Err(e) = stores.forge.update_worker_status(
            &worker_id.0,
            state_str,
            error_msg,
            status.current_task.as_deref(),
            status.messages_sent as i64,
            status.messages_received as i64,
        ).await {
            tracing::warn!("Failed to persist worker status to DB: {}", e);
        }
    }
}

#[derive(Deserialize)]
struct CreateWorkerParams {
    cwd: String,
    model: Option<String>,
    system_prompt: Option<String>,
    /// If true (default), worker won't inherit user's MCP servers
    mcp_strict: Option<bool>,
    /// List of MCP server JSON configs to include
    mcp_servers: Option<Vec<String>>,
    /// Enable Chrome browser integration for web UI testing
    chrome: Option<bool>,
}

async fn handle_cortex_create_worker(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: CreateWorkerParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let mut config = WorkerConfig::new(&params.cwd);
    if let Some(ref model) = params.model {
        config = config.with_model(model);
    }
    if let Some(ref prompt) = params.system_prompt {
        config = config.with_system_prompt(prompt);
    }

    // Configure MCP access
    // Default: strict mode (no inherited MCP servers)
    let mcp_strict = params.mcp_strict.unwrap_or(true);
    let mcp_servers = params.mcp_servers.unwrap_or_default();

    let mcp_config = cortex::WorkerMcpConfig {
        strict: mcp_strict,
        servers: mcp_servers,
    };
    // Serialize before moving into config
    let mcp_config_json = serde_json::to_string(&mcp_config).unwrap_or_default();
    config = config.with_mcp_config(mcp_config);

    // Enable Chrome browser integration if requested
    if params.chrome.unwrap_or(false) {
        config = config.with_chrome(true);
    }

    // Create in-memory worker
    let worker_id = stores.workers
        .create(config)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Persist to database
    let db_worker = DbWorker::new(&worker_id.0, &params.cwd)
        .with_model(params.model.clone().unwrap_or_default())
        .with_system_prompt(params.system_prompt.clone().unwrap_or_default())
        .with_mcp_config(mcp_config_json)
        .with_chrome(params.chrome.unwrap_or(false));

    if let Err(e) = stores.forge.create_worker(db_worker).await {
        tracing::warn!("Failed to persist worker to DB: {}", e);
        // Continue anyway - worker is created in memory
    }

    // Create a Thread record for transcript capture
    let thread_name = format!("Worker {} session", &worker_id.0);
    if let Err(e) = stores.atlas.create_thread(
        &thread_name,
        ThreadSource::CortexWorker,
        None,  // session_id will be set after first message
        Some(&params.cwd),
        None,  // task_id - will be linked via dispatch_task if applicable
        Some(&worker_id.0),
    ).await {
        tracing::warn!("Failed to create thread for worker {}: {}", worker_id.0, e);
        // Continue anyway - transcript capture is non-critical
    }

    Ok(json!(worker_id.0))
}

#[derive(Deserialize)]
struct SendMessageParams {
    worker_id: String,
    message: String,
}

async fn handle_cortex_send_message(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: SendMessageParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);

    // Get the thread for this worker before sending message
    let thread = stores.atlas.get_thread_by_worker(&params.worker_id).await.ok().flatten();
    let thread_id = thread.as_ref().and_then(|t| t.id_str());

    // Get current turn number from thread
    let turn_number = thread.as_ref()
        .and_then(|t| t.content.get("entry_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;

    let response = stores.workers
        .send_message(&worker_id, &params.message)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Update session_id in database for resume support
    if let Some(ref session_id) = response.session_id {
        if let Err(e) = stores.forge.update_worker_session(&params.worker_id, Some(session_id)).await {
            tracing::warn!("Failed to persist worker session to DB: {}", e);
        }
    }

    // Create Entry records for transcript capture
    if let Some(ref tid) = thread_id {
        // Create user entry (the prompt)
        if let Err(e) = stores.atlas.create_entry(
            tid,
            EntryRole::User,
            turn_number,
            &params.message,
            None,  // tokens not available for user input
            None,  // no duration for user input
            None,  // no model for user input
            vec![],
        ).await {
            tracing::warn!("Failed to create user entry for thread {}: {}", tid, e);
        }

        // Create assistant entry (the response)
        // Extract tools used from response if possible (we can enhance this later)
        let tools_used: Vec<String> = vec![];  // TODO: parse from response.result if needed
        if let Err(e) = stores.atlas.create_entry(
            tid,
            EntryRole::Assistant,
            turn_number + 1,
            &response.result,
            None,  // tokens - could be parsed from response metadata
            Some(response.duration_ms as i64),
            None,  // model - could come from worker config
            tools_used,
        ).await {
            tracing::warn!("Failed to create assistant entry for thread {}: {}", tid, e);
        }
    }

    // Update status in database
    persist_worker_status(stores, &worker_id).await;

    Ok(json!({
        "result": response.result,
        "is_error": response.is_error,
        "session_id": response.session_id,
        "duration_ms": response.duration_ms,
    }))
}

#[derive(Deserialize)]
struct WorkerIdParams {
    worker_id: String,
}

async fn handle_cortex_worker_status(
    request: &Request,
    workers: &WorkerManager,
) -> Result<serde_json::Value, IpcError> {
    let params: WorkerIdParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);

    let status = workers
        .status(&worker_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(serde_json::to_value(status).unwrap())
}

async fn handle_cortex_list_workers(
    workers: &WorkerManager,
) -> Result<serde_json::Value, IpcError> {
    let list = workers.list().await;
    Ok(serde_json::to_value(list).unwrap())
}

#[derive(Deserialize)]
struct GetWorkersByTaskParams {
    task_id: String,
}

async fn handle_get_workers_by_task(
    request: &Request,
    forge: &ForgeStore,
) -> Result<serde_json::Value, IpcError> {
    let params: GetWorkersByTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let workers = forge
        .get_workers_by_task(&params.task_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(serde_json::to_value(workers).unwrap())
}

async fn handle_cortex_remove_worker(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: WorkerIdParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);

    // End the thread for this worker (mark as ended)
    if let Ok(Some(thread)) = stores.atlas.get_thread_by_worker(&params.worker_id).await {
        if let Some(thread_id) = thread.id_str() {
            if let Err(e) = stores.atlas.end_thread(&thread_id).await {
                tracing::warn!("Failed to end thread for worker {}: {}", params.worker_id, e);
            }
        }
    }

    // Remove from in-memory manager
    stores.workers
        .remove(&worker_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Delete from database
    if let Err(e) = stores.forge.delete_worker(&params.worker_id).await {
        tracing::warn!("Failed to delete worker from DB: {}", e);
    }

    Ok(json!(true))
}

#[derive(Deserialize)]
struct TranscriptParams {
    worker_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

/// Get the conversation transcript for a worker
async fn handle_cortex_worker_transcript(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: TranscriptParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);

    // First, try to get persistent transcript from database
    if let Ok(Some(thread)) = stores.atlas.get_thread_by_worker(&params.worker_id).await {
        if let Some(thread_id) = thread.id_str() {
            if let Ok((_, entries)) = stores.atlas.get_thread_with_entries(&thread_id, params.limit).await {
                // Convert Entry records to TranscriptEntry format for API compatibility
                let transcript: Vec<serde_json::Value> = entries.iter().map(|entry| {
                    let content = &entry.content;
                    let role = content.get("role").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let is_user = role == "user";
                    // Convert SurrealDB Datetime to chrono DateTime for proper RFC 3339 serialization
                    let timestamp = from_surreal_datetime(&entry.created_at);

                    json!({
                        "timestamp": timestamp,
                        "prompt": if is_user { entry.description.clone().unwrap_or_default() } else { String::new() },
                        "response": if !is_user { entry.description.clone() } else { None::<String> },
                        "is_error": false,
                        "duration_ms": content.get("duration_ms").and_then(|v| v.as_i64()).unwrap_or(0),
                        "role": role,
                        "turn_number": content.get("turn_number").and_then(|v| v.as_i64()).unwrap_or(0),
                    })
                }).collect();

                if !transcript.is_empty() {
                    return Ok(json!({
                        "source": "database",
                        "thread_id": thread_id,
                        "entries": transcript,
                    }));
                }
            }
        }
    }

    // Fall back to in-memory transcript
    let transcript = stores.workers
        .transcript(&worker_id, params.limit)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(json!({
        "source": "memory",
        "entries": transcript,
    }))
}

#[derive(Deserialize)]
struct ValidateShellParams {
    /// Worker ID to validate shell for (if provided)
    worker_id: Option<String>,
    /// Directory path to validate shell for (alternative to worker_id)
    path: Option<String>,
}

/// Validate the shell environment for a worker or directory
///
/// This checks if direnv can successfully load the environment.
/// Use this before operations that depend on the shell (like reload)
/// to verify the environment is valid.
async fn handle_cortex_validate_shell(
    request: &Request,
    workers: &WorkerManager,
) -> Result<serde_json::Value, IpcError> {
    let params: ValidateShellParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let validation = if let Some(ref worker_id) = params.worker_id {
        // Validate for a specific worker
        let wid = WorkerId::from_string(worker_id);
        workers
            .validate_shell(&wid)
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?
    } else if let Some(ref path) = params.path {
        // Validate for a specific path
        let path = std::path::Path::new(path);
        workers
            .validate_shell_for_path(path)
            .await
            .map_err(|e| IpcError::internal(e.to_string()))?
    } else {
        return Err(IpcError::invalid_params(
            "Must provide either worker_id or path".to_string(),
        ));
    };

    Ok(serde_json::to_value(validation).unwrap())
}

#[derive(Deserialize)]
struct ReloadShellParams {
    /// Worker ID to reload shell for
    worker_id: String,
    /// If true, clears the session so the next message starts fresh
    #[serde(default)]
    clear_session: bool,
}

/// Reload the shell environment for a worker
///
/// Validates that the shell environment loads correctly and optionally
/// clears the worker's session so it starts fresh with the new environment.
async fn handle_cortex_reload_shell(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: ReloadShellParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);
    let result = stores.workers
        .reload_shell(&worker_id, params.clear_session)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // If session was cleared, persist to database
    if let ShellReloadResult::Success { session_cleared: true, .. } = &result {
        if let Err(e) = stores.forge.update_worker_session(&params.worker_id, None).await {
            tracing::warn!("Failed to persist session clear to DB: {}", e);
        }
    }

    Ok(serde_json::to_value(result).unwrap())
}

/// Parameters for refreshing a worker
#[derive(Debug, Deserialize)]
struct RefreshWorkerParams {
    worker_id: String,
}

/// Refresh a worker's session to restore MCP tool access
async fn handle_cortex_refresh_worker(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: RefreshWorkerParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);
    let (session_cleared, state) = stores.workers
        .refresh(&worker_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Persist session clear to database
    if session_cleared {
        if let Err(e) = stores.forge.update_worker_session(&params.worker_id, None).await {
            tracing::warn!("Failed to persist session clear to DB: {}", e);
        }
    }

    Ok(json!({
        "worker_id": params.worker_id,
        "session_cleared": session_cleared,
        "state": format!("{:?}", state),
    }))
}

/// Well-known Coordinator worker ID
const COORDINATOR_WORKER_ID: &str = "coordinator";

/// Get or create the Coordinator worker
///
/// The Coordinator is a long-running headless worker with a special system prompt
/// for managing worker orchestration. There is only one Coordinator.
async fn handle_cortex_get_coordinator(
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let coordinator_id = WorkerId::from_string(COORDINATOR_WORKER_ID);

    // Check if coordinator already exists and is running
    if let Ok(status) = stores.workers.status(&coordinator_id).await {
        let is_healthy = match &status.state {
            WorkerState::Error(_) | WorkerState::Idle | WorkerState::Stopped => false,
            _ => true,
        };

        if is_healthy {
            // Coordinator exists and is healthy
            return Ok(json!({
                "worker_id": COORDINATOR_WORKER_ID,
                "state": format!("{:?}", status.state),
                "created": false,
            }));
        }
        // Coordinator exists but in bad state - remove and recreate
        tracing::info!("Removing stale coordinator in state {:?}", status.state);
        let _ = stores.workers.remove(&coordinator_id).await;
    }

    // Create new Coordinator worker
    // Use a temporary directory since Coordinator doesn't need a worktree
    let home_dir = dirs::home_dir()
        .ok_or_else(|| IpcError::internal("Could not determine home directory".to_string()))?;
    let cwd = home_dir.to_string_lossy().to_string();

    let mut config = WorkerConfig::new(&cwd);
    config = config.with_system_prompt(COORDINATOR_SYSTEM_PROMPT);
    config = config.with_model("sonnet"); // Use a capable model for coordination

    // Only Memex MCP, not all user servers (avoids auth popups from Notion etc)
    let memex_mcp_config = r#"{"mcpServers":{"memex":{"command":"memex","args":["mcp","serve"]}}}"#;
    let mcp_config = cortex::WorkerMcpConfig {
        strict: true,
        servers: vec![memex_mcp_config.to_string()],
    };
    // Serialize before moving into config
    let mcp_config_json = serde_json::to_string(&mcp_config).unwrap_or_default();
    config = config.with_mcp_config(mcp_config);

    // Use load_worker with our known ID instead of create (which generates IDs)
    let worker_id = WorkerId::from_string(COORDINATOR_WORKER_ID);
    let mut status = WorkerStatus::new(worker_id.clone());
    status.worktree = Some(cwd.clone());
    status.host = Some(hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".to_string()));
    status.state = WorkerState::Ready;

    stores.workers
        .load_worker(worker_id.clone(), config, status, None)
        .await
        .map_err(|e| IpcError::internal(format!("Failed to create coordinator: {}", e)))?;

    // Persist to database (including MCP config and worktree so it survives daemon restart)
    let db_worker = DbWorker::new(&worker_id.0, &cwd)
        .with_model("sonnet".to_string())
        .with_system_prompt(COORDINATOR_SYSTEM_PROMPT.to_string())
        .with_worktree(cwd.clone())
        .with_mcp_config(mcp_config_json);

    if let Err(e) = stores.forge.create_worker(db_worker).await {
        tracing::warn!("Failed to persist coordinator to DB: {}", e);
    }

    tracing::info!("Created new Coordinator worker: {}", worker_id.0);

    Ok(json!({
        "worker_id": worker_id.0,
        "state": "created",
        "created": true,
    }))
}

/// Send a message asynchronously - returns immediately with a message ID
async fn handle_cortex_send_message_async(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: SendMessageParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);

    // Generate a unique message ID
    let message_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string());
    let now = chrono::Utc::now();

    // Store None to indicate "processing"
    {
        let mut responses = stores.async_responses.write().await;

        // Cleanup: remove entries older than 5 minutes that have completed responses
        let cutoff = now - chrono::Duration::minutes(5);
        responses.retain(|_, state| {
            match state.completed_at {
                Some(completed) => completed > cutoff,
                None => true, // Keep pending responses
            }
        });

        responses.insert(message_id.clone(), AsyncResponseState {
            response: None,
            created_at: now,
            completed_at: None,
        });
    }

    // Clone what we need for the spawned task
    let stores_clone = Arc::clone(stores);
    let message_id_clone = message_id.clone();
    let message = params.message.clone();

    // Spawn the actual work in a background task
    tokio::spawn(async move {
        let result = stores_clone.workers.send_message(&worker_id, &message).await;

        let response_value = match &result {
            Ok(response) => json!({
                "result": response.result,
                "is_error": response.is_error,
                "session_id": response.session_id,
                "duration_ms": response.duration_ms,
            }),
            Err(e) => json!({
                "result": e.to_string(),
                "is_error": true,
                "session_id": null,
                "duration_ms": 0,
            }),
        };

        // Persist session_id and status to database (same as sync handler)
        if let Ok(response) = &result {
            if let Some(ref session_id) = response.session_id {
                if let Err(e) = stores_clone.forge.update_worker_session(&worker_id.0, Some(session_id)).await {
                    tracing::warn!("Failed to persist worker session to DB: {}", e);
                }
            }

            // Update status in database
            let status = stores_clone.workers.status(&worker_id).await.ok();
            if let Some(status) = status {
                let state_str = match &status.state {
                    WorkerState::Starting => "starting",
                    WorkerState::Ready => "ready",
                    WorkerState::Working => "working",
                    WorkerState::Idle => "idle",
                    WorkerState::Stopped => "stopped",
                    WorkerState::Error(_) => "error",
                };
                let error_msg = match &status.state {
                    WorkerState::Error(msg) => Some(msg.as_str()),
                    _ => None,
                };
                if let Err(e) = stores_clone.forge.update_worker_status(
                    &worker_id.0,
                    state_str,
                    error_msg,
                    status.current_task.as_deref(),
                    status.messages_sent as i64,
                    status.messages_received as i64,
                ).await {
                    tracing::warn!("Failed to persist worker status to DB: {}", e);
                }
            }
        }

        // Store the result
        let mut responses = stores_clone.async_responses.write().await;
        if let Some(state) = responses.get_mut(&message_id_clone) {
            state.response = Some(response_value);
            state.completed_at = Some(chrono::Utc::now());
        }
    });

    // Return immediately with the message ID
    Ok(json!(message_id))
}

#[derive(Deserialize)]
struct GetResponseParams {
    message_id: String,
}

/// Get the response for an async message (returns null if still processing)
async fn handle_cortex_get_response(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: GetResponseParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let responses = stores.async_responses.read().await;

    match responses.get(&params.message_id) {
        Some(state) => {
            match &state.response {
                Some(response) => {
                    // Response is ready - return it
                    // Note: we don't remove it here so it can be retrieved multiple times
                    // Cleanup happens during new message insertion
                    Ok(response.clone())
                }
                None => {
                    // Still processing
                    Ok(serde_json::Value::Null)
                }
            }
        }
        None => {
            // Unknown message ID (may have been cleaned up)
            Err(IpcError::invalid_params(format!(
                "Unknown message ID: {} (may have expired)",
                params.message_id
            )))
        }
    }
}

// ========== Dispatch Task Handler ==========

#[derive(Deserialize)]
struct DispatchTaskParams {
    /// Task ID to dispatch
    task_id: String,
    /// Optional: specific worktree path. Auto-creates if not provided.
    worktree: Option<String>,
    /// Optional: model override (defaults to sonnet)
    model: Option<String>,
    /// Optional: repo path for worktree creation (defaults to cwd)
    repo_path: Option<String>,
    /// Enable Chrome browser integration for web UI testing
    chrome: Option<bool>,
}

/// Dispatch a task to a worker with automatic context assembly
///
/// This is the key orchestration abstraction:
/// 1. Get task record
/// 2. Find project from task (if any)
/// 3. Find repo record for the project
/// 4. Assemble context from repo (rules, skills, etc.)
/// 5. Create/find worktree for the work
/// 6. Create worker with context + task in prompt
/// 7. Create assigned_to edge
/// 8. Send initial message to start work
async fn handle_cortex_dispatch_task(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: DispatchTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // 1. Get task record
    let task_record = stores.atlas
        .get_record(&params.task_id)
        .await
        .map_err(|e| IpcError::internal(format!("Failed to get task: {}", e)))?
        .ok_or_else(|| IpcError::invalid_params(format!("Task not found: {}", params.task_id)))?;

    // Ensure it's a task
    if task_record.record_type != "task" {
        return Err(IpcError::invalid_params(format!(
            "Record {} is not a task (type: {})",
            params.task_id, task_record.record_type
        )));
    }

    // Extract task info
    let task_title = &task_record.name;
    let task_description = task_record.description.as_deref().unwrap_or("");
    let task_project = task_record.content.get("project")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 2. Find context source (repo or project record)
    // Try to find a record that matches the project name, checking multiple types
    let context_record_id = if let Some(ref project) = task_project {
        // Try repo first, then project, then initiative
        let record_types = ["repo", "project", "initiative"];
        let mut found_id = None;
        for record_type in record_types {
            if let Ok(Some(record)) = stores.atlas.get_record_by_type_name(record_type, project).await {
                found_id = record.id.map(|t| t.id.to_raw());
                break;
            }
        }
        found_id
    } else {
        None
    };

    // 3. Assemble context if we have a record ID
    let (context_prompt, mcp_server_configs) = if let Some(ref record_id) = context_record_id {
        match stores.atlas.assemble_context(record_id, 3).await {
            Ok(assembly) => {
                let prompt = assembly.to_system_prompt();
                let mcp_configs = assembly.mcp_server_configs();
                (
                    if prompt.is_empty() { None } else { Some(prompt) },
                    mcp_configs
                )
            }
            Err(e) => {
                tracing::warn!("Failed to assemble context from {}: {}", record_id, e);
                (None, vec![])
            }
        }
    } else {
        (None, vec![])
    };

    // 4. Determine worktree path
    let worktree_path = if let Some(path) = params.worktree {
        // Use specified path
        path
    } else {
        // Auto-create worktree using vibetree
        let repo_path = std::path::PathBuf::from(params.repo_path.as_deref().unwrap_or("."));

        // Generate a branch name from task ID
        let branch_name = format!("task-{}", &params.task_id[..8.min(params.task_id.len())]);

        // Create vibetree app and worktree
        let mut app = vibetree::VibeTreeApp::with_parent(repo_path.clone())
            .map_err(|e| IpcError::internal(format!("Failed to load vibetree config: {}", e)))?;

        app.add_worktree(
            branch_name.clone(),
            None, // from_branch - use current HEAD
            None, // custom_values
            false, // dry_run
            false, // switch (don't spawn shell)
            true, // reuse existing worktree if present
        )
        .map_err(|e| IpcError::internal(format!(
            "Failed to create worktree: {}. Specify worktree path manually.",
            e
        )))?;

        // Compute the worktree path (branches_dir/branch_name)
        let branches_dir = app.get_config_mut().project_config.branches_dir.clone();
        let computed_path = repo_path.join(&branches_dir).join(&branch_name);

        // Verify the worktree directory actually exists
        if !computed_path.exists() {
            return Err(IpcError::internal(format!(
                "Worktree creation reported success but directory does not exist: {}",
                computed_path.display()
            )));
        }

        computed_path.to_string_lossy().to_string()
    };

    // 5. Build system prompt with task and context
    let mut system_parts = Vec::new();

    // Add task context
    system_parts.push(format!(
        "# Your Task\n\n\
         **Title:** {}\n\n\
         **Description:**\n{}\n\n\
         Work on this task. When you complete it, summarize what you accomplished.",
        task_title, task_description
    ));

    // Add worker guidance
    system_parts.push(
        "# Worker Guidelines\n\n\
         ## Git Workflow\n\
         - You are working in an isolated git worktree\n\
         - Commit your changes with single-line commit messages (under 50 chars)\n\
         - Commit frequently as you make progress\n\
         - Do not push - your changes will be reviewed and merged by the coordinator\n\n\
         ## Available Tools\n\
         - You have access to Memex MCP tools for querying project knowledge\n\
         - Use `query_knowledge` if you need more context about the project\n\
         - Use `search_knowledge` for specific fact lookups\n\
         - Use `record_memo` to capture important findings or decisions\n\n\
         ## Communication\n\
         - If you get stuck or need clarification, explain what's blocking you\n\
         - When done, provide a clear summary of changes made and any issues found\n\
         - If the task is already done or not needed, explain why"
            .to_string(),
    );

    // Add assembled context (rules, skills, etc.)
    if let Some(context) = context_prompt {
        system_parts.push(context);
    }

    let system_prompt = system_parts.join("\n\n---\n\n");

    // 6. Create worker
    let mut config = WorkerConfig::new(&worktree_path);
    config = config.with_system_prompt(&system_prompt);
    if let Some(ref model) = params.model {
        config = config.with_model(model);
    }

    // Configure MCP servers from context assembly
    // If no MCP servers found in context, fall back to memex-only
    let mcp_server_list = if mcp_server_configs.is_empty() {
        // Fallback: only Memex, not all user servers (avoids auth popups from Notion etc)
        let memex_mcp_config = r#"{"mcpServers":{"memex":{"command":"memex","args":["mcp","serve"]}}}"#;
        vec![memex_mcp_config.to_string()]
    } else {
        mcp_server_configs
    };

    let mcp_config = cortex::WorkerMcpConfig {
        strict: true,
        servers: mcp_server_list,
    };
    // Serialize before moving into config
    let mcp_config_json = serde_json::to_string(&mcp_config).unwrap_or_default();
    config = config.with_mcp_config(mcp_config);

    // Enable Chrome browser integration if requested
    let chrome_enabled = params.chrome.unwrap_or(false);
    if chrome_enabled {
        config = config.with_chrome(true);
    }

    let worker_id = stores.workers
        .create(config)
        .await
        .map_err(|e| IpcError::internal(format!("Failed to create worker: {}", e)))?;

    // 7. Set task assignment in worker's in-memory status
    stores.workers.set_current_task(&worker_id, Some(params.task_id.clone()))
        .await
        .map_err(|e| IpcError::internal(format!("Failed to set worker task: {}", e)))?;

    // Persist worker to database with task reference and worktree
    let db_worker = DbWorker::new(&worker_id.0, &worktree_path)
        .with_model(params.model.unwrap_or_default())
        .with_system_prompt(system_prompt)
        .with_current_task(Some(params.task_id.clone()))
        .with_worktree(worktree_path.clone())
        .with_mcp_config(mcp_config_json)
        .with_chrome(chrome_enabled);

    if let Err(e) = stores.forge.create_worker(db_worker).await {
        tracing::warn!("Failed to persist worker to DB: {}", e);
    }

    // 8. Update task status to in_progress
    if let Err(e) = stores.atlas.update_task_status(&params.task_id, "in_progress").await {
        tracing::warn!("Failed to update task status: {}", e);
    }

    // 9. Send initial message to worker asynchronously to start work
    let worker_id_for_msg = worker_id.clone();
    let stores_for_msg = Arc::clone(stores);
    tokio::spawn(async move {
        let message = "Begin work on your assigned task.";
        match stores_for_msg.workers.send_message(&worker_id_for_msg, message).await {
            Ok(response) => {
                // Persist session_id after initial message
                if let Some(ref session_id) = response.session_id {
                    if let Err(e) = stores_for_msg.forge.update_worker_session(&worker_id_for_msg.0, Some(session_id)).await {
                        tracing::warn!("Failed to update worker session in DB: {}", e);
                    }
                }
                // Persist full worker status (state, current_task, message counts)
                persist_worker_status(&stores_for_msg, &worker_id_for_msg).await;
            }
            Err(e) => {
                tracing::warn!("Failed to send initial message to worker {}: {}", worker_id_for_msg.0, e);
                // Still persist status on error so we capture the error state
                persist_worker_status(&stores_for_msg, &worker_id_for_msg).await;
            }
        }
    });

    Ok(json!({
        "worker_id": worker_id.0,
        "worktree": worktree_path,
        "task_id": params.task_id,
        "context_from": context_record_id,
        "ready": true,
    }))
}

// ========== Batch Dispatch Tasks Handler ==========

#[derive(Deserialize)]
struct DispatchTasksParams {
    /// List of task IDs to dispatch
    task_ids: Vec<String>,
    /// Optional: model override for all workers
    model: Option<String>,
    /// Optional: repo path for worktree creation
    repo_path: Option<String>,
    /// Enable Chrome browser integration for all workers
    chrome: Option<bool>,
}

/// Result for a single task dispatch in a batch
#[derive(Serialize)]
struct DispatchedWorkerResult {
    task_id: String,
    worker_id: Option<String>,
    worktree: Option<String>,
    context_from: Option<String>,
    success: bool,
    error: Option<String>,
}

/// Dispatch multiple tasks to workers in parallel
///
/// This is a streamlined command that spawns multiple workers concurrently.
/// Each task gets its own isolated worktree and worker.
async fn handle_cortex_dispatch_tasks(
    request: &Request,
    stores: &Arc<Stores>,
) -> Result<serde_json::Value, IpcError> {
    let params: DispatchTasksParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    if params.task_ids.is_empty() {
        return Err(IpcError::invalid_params("task_ids cannot be empty".to_string()));
    }

    // Dispatch all tasks concurrently using join_all
    let chrome_enabled = params.chrome.unwrap_or(false);
    let dispatch_futures: Vec<_> = params.task_ids.iter().map(|task_id| {
        let stores = Arc::clone(stores);
        let task_id = task_id.clone();
        let model = params.model.clone();
        let repo_path = params.repo_path.clone();

        async move {
            dispatch_single_task(&stores, &task_id, model.as_deref(), repo_path.as_deref(), chrome_enabled).await
        }
    }).collect();

    let results = futures::future::join_all(dispatch_futures).await;

    // Collect results
    let mut workers = Vec::new();
    let mut dispatched = 0;
    let mut failed = 0;

    for result in results {
        match result {
            Ok(worker_result) => {
                if worker_result.success {
                    dispatched += 1;
                } else {
                    failed += 1;
                }
                workers.push(worker_result);
            }
            Err(e) => {
                // This shouldn't happen since dispatch_single_task handles errors internally
                failed += 1;
                workers.push(DispatchedWorkerResult {
                    task_id: "unknown".to_string(),
                    worker_id: None,
                    worktree: None,
                    context_from: None,
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(json!({
        "workers": workers,
        "dispatched": dispatched,
        "failed": failed,
    }))
}

/// Dispatch a single task - extracted to allow concurrent dispatch
async fn dispatch_single_task(
    stores: &Arc<Stores>,
    task_id: &str,
    model: Option<&str>,
    repo_path: Option<&str>,
    chrome: bool,
) -> Result<DispatchedWorkerResult, IpcError> {
    // 1. Get task record
    let task_record = match stores.atlas.get_record(task_id).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return Ok(DispatchedWorkerResult {
                task_id: task_id.to_string(),
                worker_id: None,
                worktree: None,
                context_from: None,
                success: false,
                error: Some(format!("Task not found: {}", task_id)),
            });
        }
        Err(e) => {
            return Ok(DispatchedWorkerResult {
                task_id: task_id.to_string(),
                worker_id: None,
                worktree: None,
                context_from: None,
                success: false,
                error: Some(format!("Failed to get task: {}", e)),
            });
        }
    };

    // Ensure it's a task
    if task_record.record_type != "task" {
        return Ok(DispatchedWorkerResult {
            task_id: task_id.to_string(),
            worker_id: None,
            worktree: None,
            context_from: None,
            success: false,
            error: Some(format!("Record {} is not a task (type: {})", task_id, task_record.record_type)),
        });
    }

    // Extract task info
    let task_title = &task_record.name;
    let task_description = task_record.description.as_deref().unwrap_or("");
    let task_project = task_record.content.get("project")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 2. Find context source (repo or project record)
    // Try to find a record that matches the project name, checking multiple types
    let context_record_id = if let Some(ref project) = task_project {
        let record_types = ["repo", "project", "initiative"];
        let mut found_id = None;
        for record_type in record_types {
            if let Ok(Some(record)) = stores.atlas.get_record_by_type_name(record_type, project).await {
                found_id = record.id.map(|t| t.id.to_raw());
                break;
            }
        }
        found_id
    } else {
        None
    };

    // 3. Assemble context if we have a record ID
    let (context_prompt, mcp_server_configs) = if let Some(ref record_id) = context_record_id {
        match stores.atlas.assemble_context(record_id, 3).await {
            Ok(assembly) => {
                let prompt = assembly.to_system_prompt();
                let mcp_configs = assembly.mcp_server_configs();
                (
                    if prompt.is_empty() { None } else { Some(prompt) },
                    mcp_configs
                )
            }
            Err(e) => {
                tracing::warn!("Failed to assemble context from {}: {}", record_id, e);
                (None, vec![])
            }
        }
    } else {
        (None, vec![])
    };

    // 4. Create worktree using vibetree
    let repo_path = std::path::PathBuf::from(repo_path.unwrap_or("."));
    let branch_name = format!("task-{}", &task_id[..8.min(task_id.len())]);

    let worktree_path = match vibetree::VibeTreeApp::with_parent(repo_path.clone()) {
        Ok(mut app) => {
            match app.add_worktree(
                branch_name.clone(),
                None,
                None,
                false,
                false,
                true, // reuse existing worktree if present
            ) {
                Ok(_) => {
                    let branches_dir = app.get_config_mut().project_config.branches_dir.clone();
                    let computed_path = repo_path.join(&branches_dir).join(&branch_name);

                    // Verify the worktree directory actually exists
                    if !computed_path.exists() {
                        return Ok(DispatchedWorkerResult {
                            task_id: task_id.to_string(),
                            worker_id: None,
                            worktree: None,
                            context_from: context_record_id,
                            success: false,
                            error: Some(format!(
                                "Worktree creation reported success but directory does not exist: {}",
                                computed_path.display()
                            )),
                        });
                    }

                    computed_path.to_string_lossy().to_string()
                }
                Err(e) => {
                    return Ok(DispatchedWorkerResult {
                        task_id: task_id.to_string(),
                        worker_id: None,
                        worktree: None,
                        context_from: context_record_id,
                        success: false,
                        error: Some(format!("Failed to create worktree: {}", e)),
                    });
                }
            }
        }
        Err(e) => {
            return Ok(DispatchedWorkerResult {
                task_id: task_id.to_string(),
                worker_id: None,
                worktree: None,
                context_from: context_record_id,
                success: false,
                error: Some(format!("Failed to load vibetree config: {}", e)),
            });
        }
    };

    // 5. Build system prompt
    let mut system_parts = Vec::new();

    system_parts.push(format!(
        "# Your Task\n\n\
         **Title:** {}\n\n\
         **Description:**\n{}\n\n\
         Work on this task. When you complete it, summarize what you accomplished.",
        task_title, task_description
    ));

    system_parts.push(
        "# Worker Guidelines\n\n\
         ## Git Workflow\n\
         - You are working in an isolated git worktree\n\
         - Commit your changes with single-line commit messages (under 50 chars)\n\
         - Commit frequently as you make progress\n\
         - Do not push - your changes will be reviewed and merged by the coordinator\n\n\
         ## Available Tools\n\
         - You have access to Memex MCP tools for querying project knowledge\n\
         - Use `query_knowledge` if you need more context about the project\n\
         - Use `search_knowledge` for specific fact lookups\n\
         - Use `record_memo` to capture important findings or decisions\n\n\
         ## Communication\n\
         - If you get stuck or need clarification, explain what's blocking you\n\
         - When done, provide a clear summary of changes made and any issues found\n\
         - If the task is already done or not needed, explain why"
            .to_string(),
    );

    if let Some(context) = context_prompt {
        system_parts.push(context);
    }

    let system_prompt = system_parts.join("\n\n---\n\n");

    // 6. Create worker
    let mut config = WorkerConfig::new(&worktree_path);
    config = config.with_system_prompt(&system_prompt);
    if let Some(model) = model {
        config = config.with_model(model);
    }

    // Configure MCP servers from context assembly
    // If no MCP servers found in context, fall back to memex-only
    let mcp_server_list = if mcp_server_configs.is_empty() {
        // Fallback: only Memex, not all user servers (avoids auth popups from Notion etc)
        let memex_mcp_config = r#"{"mcpServers":{"memex":{"command":"memex","args":["mcp","serve"]}}}"#;
        vec![memex_mcp_config.to_string()]
    } else {
        mcp_server_configs
    };

    let mcp_config = cortex::WorkerMcpConfig {
        strict: true,
        servers: mcp_server_list,
    };
    // Serialize before moving into config
    let mcp_config_json = serde_json::to_string(&mcp_config).unwrap_or_default();
    config = config.with_mcp_config(mcp_config);

    // Enable Chrome browser integration if requested
    if chrome {
        config = config.with_chrome(true);
    }

    let worker_id = match stores.workers.create(config).await {
        Ok(id) => id,
        Err(e) => {
            return Ok(DispatchedWorkerResult {
                task_id: task_id.to_string(),
                worker_id: None,
                worktree: Some(worktree_path),
                context_from: context_record_id,
                success: false,
                error: Some(format!("Failed to create worker: {}", e)),
            });
        }
    };

    // Set task assignment in worker's in-memory status
    if let Err(e) = stores.workers.set_current_task(&worker_id, Some(task_id.to_string())).await {
        tracing::warn!("Failed to set worker task: {}", e);
    }

    // 7. Persist worker to database with task reference and worktree
    let db_worker = DbWorker::new(&worker_id.0, &worktree_path)
        .with_model(model.unwrap_or_default())
        .with_system_prompt(system_prompt)
        .with_current_task(Some(task_id.to_string()))
        .with_worktree(worktree_path.clone())
        .with_mcp_config(mcp_config_json)
        .with_chrome(chrome);

    if let Err(e) = stores.forge.create_worker(db_worker).await {
        tracing::warn!("Failed to persist worker to DB: {}", e);
    }

    // 8. Update task status to in_progress
    if let Err(e) = stores.atlas.update_task_status(task_id, "in_progress").await {
        tracing::warn!("Failed to update task status: {}", e);
    }

    // 9. Send initial message to worker asynchronously to start work
    let worker_id_for_msg = worker_id.clone();
    let stores_for_msg = Arc::clone(stores);
    tokio::spawn(async move {
        let message = "Begin work on your assigned task.";
        match stores_for_msg.workers.send_message(&worker_id_for_msg, message).await {
            Ok(response) => {
                // Persist session_id after initial message
                if let Some(ref session_id) = response.session_id {
                    if let Err(e) = stores_for_msg.forge.update_worker_session(&worker_id_for_msg.0, Some(session_id)).await {
                        tracing::warn!("Failed to update worker session in DB: {}", e);
                    }
                }
                // Persist full worker status (state, current_task, message counts)
                persist_worker_status(&stores_for_msg, &worker_id_for_msg).await;
            }
            Err(e) => {
                tracing::warn!("Failed to send initial message to worker {}: {}", worker_id_for_msg.0, e);
                // Still persist status on error so we capture the error state
                persist_worker_status(&stores_for_msg, &worker_id_for_msg).await;
            }
        }
    });

    Ok(DispatchedWorkerResult {
        task_id: task_id.to_string(),
        worker_id: Some(worker_id.0.clone()),
        worktree: Some(worktree_path),
        context_from: context_record_id,
        success: true,
        error: None,
    })
}

// ========== Vibetree Handlers ==========

#[derive(Deserialize)]
struct VibetreeListParams {
    cwd: String,
}

async fn handle_vibetree_list(request: &Request) -> Result<serde_json::Value, IpcError> {
    let params: VibetreeListParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let path = std::path::PathBuf::from(&params.cwd);

    // Use load_existing to avoid creating config files
    let app = vibetree::VibeTreeApp::load_existing_with_parent(path)
        .map_err(|e| IpcError::internal(format!("Failed to load vibetree config: {}", e)))?;

    let worktrees = app
        .collect_worktree_data()
        .map_err(|e| IpcError::internal(format!("Failed to list worktrees: {}", e)))?;

    // Convert to serializable format
    let result: Vec<serde_json::Value> = worktrees
        .into_iter()
        .map(|wt| {
            json!({
                "name": wt.name,
                "status": wt.status,
                "values": wt.values,
            })
        })
        .collect();

    Ok(json!(result))
}

#[derive(Deserialize)]
struct VibetreeCreateParams {
    cwd: String,
    branch_name: String,
    from_branch: Option<String>,
    reuse: Option<bool>,
}

async fn handle_vibetree_create(request: &Request) -> Result<serde_json::Value, IpcError> {
    let params: VibetreeCreateParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let path = std::path::PathBuf::from(&params.cwd);

    let mut app = vibetree::VibeTreeApp::with_parent(path)
        .map_err(|e| IpcError::internal(format!("Failed to load vibetree config: {}", e)))?;

    // Create the worktree (dry_run=false, switch=false)
    app.add_worktree(
        params.branch_name.clone(),
        params.from_branch,
        None, // custom_values
        false, // dry_run
        false, // switch (don't spawn shell)
        params.reuse.unwrap_or(false), // reuse existing worktree
    )
    .map_err(|e| IpcError::internal(format!("Failed to create worktree: {}", e)))?;

    // Get the created worktree info
    let worktrees = app
        .collect_worktree_data()
        .map_err(|e| IpcError::internal(format!("Failed to get worktree data: {}", e)))?;

    let created = worktrees
        .into_iter()
        .find(|wt| wt.name == params.branch_name);

    match created {
        Some(wt) => Ok(json!({
            "name": wt.name,
            "status": wt.status,
            "values": wt.values,
        })),
        None => Ok(json!({
            "name": params.branch_name,
            "status": "created",
        })),
    }
}

#[derive(Deserialize)]
struct VibetreeRemoveParams {
    cwd: String,
    branch_name: String,
    force: Option<bool>,
    keep_branch: Option<bool>,
}

async fn handle_vibetree_remove(request: &Request) -> Result<serde_json::Value, IpcError> {
    let params: VibetreeRemoveParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let path = std::path::PathBuf::from(&params.cwd);

    let mut app = vibetree::VibeTreeApp::with_parent(path)
        .map_err(|e| IpcError::internal(format!("Failed to load vibetree config: {}", e)))?;

    // Use the test method to bypass confirmation prompts
    app.remove_worktree_for_test(
        params.branch_name.clone(),
        params.force.unwrap_or(true), // Default to force for programmatic use
        params.keep_branch.unwrap_or(false),
    )
    .map_err(|e| IpcError::internal(format!("Failed to remove worktree: {}", e)))?;

    Ok(json!({
        "removed": params.branch_name,
    }))
}

#[derive(Deserialize)]
struct VibetreeMergeParams {
    cwd: String,
    branch_name: String,
    into: Option<String>,
    squash: Option<bool>,
    remove: Option<bool>,
    message: Option<String>,
}

async fn handle_vibetree_merge(request: &Request) -> Result<serde_json::Value, IpcError> {
    let params: VibetreeMergeParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let path = std::path::PathBuf::from(&params.cwd);
    let into_branch = params.into.as_deref().unwrap_or("dev");
    let squash = params.squash.unwrap_or(false);
    let remove_after = params.remove.unwrap_or(false);

    // Find the git repository root
    let repo_root = vibetree::GitManager::find_repo_root(&path)
        .map_err(|e| IpcError::internal(format!("Failed to find git repository: {}", e)))?;

    // Check that the branch to merge exists
    let branch_exists = std::process::Command::new("git")
        .args(["rev-parse", "--verify", &params.branch_name])
        .current_dir(&repo_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !branch_exists {
        return Err(IpcError::invalid_params(format!(
            "Branch '{}' does not exist",
            params.branch_name
        )));
    }

    // Check that the target branch exists
    let target_exists = std::process::Command::new("git")
        .args(["rev-parse", "--verify", into_branch])
        .current_dir(&repo_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !target_exists {
        return Err(IpcError::invalid_params(format!(
            "Target branch '{}' does not exist",
            into_branch
        )));
    }

    // Get current branch to restore later if needed
    let current_branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| IpcError::internal(format!("Failed to get current branch: {}", e)))?;

    let original_branch = String::from_utf8_lossy(&current_branch.stdout).trim().to_string();

    // Checkout the target branch
    let checkout_output = std::process::Command::new("git")
        .args(["checkout", into_branch])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| IpcError::internal(format!("Failed to checkout target branch: {}", e)))?;

    if !checkout_output.status.success() {
        let stderr = String::from_utf8_lossy(&checkout_output.stderr);
        return Err(IpcError::internal(format!(
            "Failed to checkout '{}': {}",
            into_branch, stderr
        )));
    }

    // Perform the merge
    let merge_result = if squash {
        // Squash merge
        let squash_output = std::process::Command::new("git")
            .args(["merge", "--squash", &params.branch_name])
            .current_dir(&repo_root)
            .output()
            .map_err(|e| IpcError::internal(format!("Failed to squash merge: {}", e)))?;

        if !squash_output.status.success() {
            // Restore original branch on failure
            let _ = std::process::Command::new("git")
                .args(["checkout", &original_branch])
                .current_dir(&repo_root)
                .output();
            let stderr = String::from_utf8_lossy(&squash_output.stderr);
            return Err(IpcError::internal(format!(
                "Squash merge failed: {}",
                stderr
            )));
        }

        // Commit the squashed changes
        let commit_msg = params.message.clone().unwrap_or_else(|| {
            format!("Merge branch '{}' (squashed)", params.branch_name)
        });
        let commit_output = std::process::Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(&repo_root)
            .output()
            .map_err(|e| IpcError::internal(format!("Failed to commit squash merge: {}", e)))?;

        if !commit_output.status.success() {
            // Check if there's nothing to commit (branches already identical)
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            if !stderr.contains("nothing to commit") {
                // Restore original branch on failure
                let _ = std::process::Command::new("git")
                    .args(["checkout", &original_branch])
                    .current_dir(&repo_root)
                    .output();
                return Err(IpcError::internal(format!(
                    "Failed to commit squash merge: {}",
                    stderr
                )));
            }
        }

        Ok(())
    } else {
        // Regular merge
        let merge_msg = params.message.clone().unwrap_or_else(|| {
            format!("Merge branch '{}'", params.branch_name)
        });
        let merge_output = std::process::Command::new("git")
            .args(["merge", &params.branch_name, "-m", &merge_msg])
            .current_dir(&repo_root)
            .output()
            .map_err(|e| IpcError::internal(format!("Failed to merge: {}", e)))?;

        if !merge_output.status.success() {
            // Restore original branch on failure
            let _ = std::process::Command::new("git")
                .args(["merge", "--abort"])
                .current_dir(&repo_root)
                .output();
            let _ = std::process::Command::new("git")
                .args(["checkout", &original_branch])
                .current_dir(&repo_root)
                .output();
            let stderr = String::from_utf8_lossy(&merge_output.stderr);
            return Err(IpcError::internal(format!("Merge failed: {}", stderr)));
        }

        Ok(())
    };

    merge_result?;

    // Restore original branch if it wasn't the target
    if original_branch != into_branch {
        let _ = std::process::Command::new("git")
            .args(["checkout", &original_branch])
            .current_dir(&repo_root)
            .output();
    }

    // Remove worktree if requested
    let mut removed = false;
    if remove_after {
        // Try to load vibetree config and remove the worktree
        if let Ok(mut app) = vibetree::VibeTreeApp::with_parent(path.clone()) {
            if app.remove_worktree_for_test(
                params.branch_name.clone(),
                true, // force
                false, // don't keep branch since we merged it
            ).is_ok() {
                removed = true;
            }
        }
    }

    Ok(json!({
        "merged": params.branch_name,
        "into": into_branch,
        "squashed": squash,
        "removed": removed,
    }))
}

#[derive(Deserialize)]
struct VibetreeEnvParams {
    cwd: String,
    branch: Option<String>,
    format: Option<String>,
}

async fn handle_vibetree_env(request: &Request) -> Result<serde_json::Value, IpcError> {
    let params: VibetreeEnvParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let path = std::path::PathBuf::from(&params.cwd);
    let format = params.format.as_deref().unwrap_or("export");

    // Load vibetree config
    let app = vibetree::VibeTreeApp::load_existing_with_parent(path.clone())
        .map_err(|e| IpcError::internal(format!("Failed to load vibetree config: {}", e)))?;

    let worktrees = app
        .collect_worktree_data()
        .map_err(|e| IpcError::internal(format!("Failed to list worktrees: {}", e)))?;

    // Determine which worktree to get env for
    let branch_name = if let Some(ref branch) = params.branch {
        branch.clone()
    } else {
        // Try to detect branch from cwd path
        // The cwd might be inside a worktree like .vibetree/branches/<branch-name>/...
        let cwd_str = path.to_string_lossy();
        let mut detected_branch: Option<String> = None;

        // Check if we're in a worktree directory
        for wt in &worktrees {
            // Worktree paths are typically .vibetree/branches/<name>
            if cwd_str.contains(&format!("/{}/", wt.name)) || cwd_str.ends_with(&format!("/{}", wt.name)) {
                detected_branch = Some(wt.name.clone());
                break;
            }
        }

        detected_branch.ok_or_else(|| {
            IpcError::invalid_params(
                "Could not detect worktree from cwd. Please specify a branch name.".to_string()
            )
        })?
    };

    // Find the worktree
    let worktree = worktrees
        .into_iter()
        .find(|wt| wt.name == branch_name)
        .ok_or_else(|| {
            IpcError::invalid_params(format!("Worktree '{}' not found", branch_name))
        })?;

    // Format the output based on requested format
    let output = match format {
        "json" => {
            serde_json::to_string_pretty(&worktree.values)
                .unwrap_or_else(|_| "{}".to_string())
        }
        "dotenv" => {
            worktree.values
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => {
            // Default "export" format for bash/zsh
            worktree.values
                .iter()
                .map(|(k, v)| format!("export {}={}", k, v))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    Ok(json!(output))
}

// ========== Agent Messaging Handlers ==========

/// Send a message to the inter-agent message queue
async fn handle_send_agent_message(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    #[derive(Deserialize)]
    struct SendMessageParams {
        /// Sender identifier (e.g., worker ID or "primary")
        sender: String,
        /// Optional recipient (None = coordinator/broadcast)
        recipient: Option<String>,
        /// Message content
        content: String,
        /// Message type (e.g., "status", "request", "response")
        #[serde(default = "default_message_type")]
        message_type: String,
    }

    fn default_message_type() -> String {
        "message".to_string()
    }

    let params: SendMessageParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    // Generate unique message ID
    let message_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string());

    let message = AgentMessage {
        id: message_id.clone(),
        sender: params.sender.clone(),
        recipient: params.recipient.clone(),
        content: params.content,
        message_type: params.message_type,
        timestamp: chrono::Utc::now(),
        read: false,
    };

    // Add to message queue
    {
        let mut messages = stores.agent_messages.write().await;
        messages.push(message);
    }

    tracing::debug!(
        "Agent message queued: {} from {} to {:?}",
        message_id,
        params.sender,
        params.recipient
    );

    Ok(json!({
        "message_id": message_id,
        "status": "queued"
    }))
}

/// Check for messages in the queue, optionally filtering by recipient
async fn handle_check_messages(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    #[derive(Deserialize, Default)]
    struct CheckMessagesParams {
        /// Filter messages for this recipient (None = all unread)
        recipient: Option<String>,
        /// Include messages with no specific recipient (broadcast/coordinator messages)
        #[serde(default = "default_include_broadcast")]
        include_broadcast: bool,
        /// Mark retrieved messages as read
        #[serde(default = "default_mark_read")]
        mark_read: bool,
        /// Maximum number of messages to return
        limit: Option<usize>,
    }

    fn default_include_broadcast() -> bool {
        true
    }

    fn default_mark_read() -> bool {
        true
    }

    let params: CheckMessagesParams = if request.params.is_null() {
        CheckMessagesParams::default()
    } else {
        serde_json::from_value(request.params.clone())
            .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?
    };

    let mut messages_to_return = Vec::new();
    let mut message_ids_to_mark = Vec::new();

    {
        let messages = stores.agent_messages.read().await;

        for msg in messages.iter() {
            // Skip already-read messages
            if msg.read {
                continue;
            }

            // Filter by recipient
            let matches = match (&params.recipient, &msg.recipient) {
                // If no recipient filter, include all unread messages
                (None, _) => true,
                // If filtering for a specific recipient
                (Some(filter), Some(msg_recipient)) => filter == msg_recipient,
                // Include broadcast messages if requested
                (Some(_), None) => params.include_broadcast,
            };

            if matches {
                messages_to_return.push(json!({
                    "id": msg.id,
                    "sender": msg.sender,
                    "recipient": msg.recipient,
                    "content": msg.content,
                    "message_type": msg.message_type,
                    "timestamp": msg.timestamp.to_rfc3339(),
                }));
                message_ids_to_mark.push(msg.id.clone());

                // Check limit
                if let Some(limit) = params.limit {
                    if messages_to_return.len() >= limit {
                        break;
                    }
                }
            }
        }
    }

    // Mark messages as read if requested
    if params.mark_read && !message_ids_to_mark.is_empty() {
        let mut messages = stores.agent_messages.write().await;
        for msg in messages.iter_mut() {
            if message_ids_to_mark.contains(&msg.id) {
                msg.read = true;
            }
        }
    }

    Ok(json!({
        "messages": messages_to_return,
        "count": messages_to_return.len(),
    }))
}

/// Clear old/read messages from the queue (housekeeping)
async fn handle_clear_agent_messages(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    #[derive(Deserialize, Default)]
    struct ClearParams {
        /// Clear only read messages (default: true)
        #[serde(default = "default_only_read")]
        only_read: bool,
        /// Clear messages older than N seconds
        older_than_secs: Option<i64>,
    }

    fn default_only_read() -> bool {
        true
    }

    let params: ClearParams = if request.params.is_null() {
        ClearParams::default()
    } else {
        serde_json::from_value(request.params.clone())
            .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?
    };

    let now = chrono::Utc::now();

    let cleared_count = {
        let mut messages = stores.agent_messages.write().await;
        let original_len = messages.len();

        messages.retain(|msg| {
            let should_keep = if params.only_read && !msg.read {
                // Keep unread messages when only_read is true
                true
            } else if let Some(secs) = params.older_than_secs {
                // Keep messages newer than the threshold
                let age = (now - msg.timestamp).num_seconds();
                age < secs
            } else if params.only_read {
                // Remove read messages
                !msg.read
            } else {
                // Clear all
                false
            };
            should_keep
        });

        original_len - messages.len()
    };

    tracing::debug!("Cleared {} agent messages from queue", cleared_count);

    Ok(json!({
        "cleared": cleared_count
    }))
}

// ========== Public Functions ==========

/// Stop the running daemon
pub async fn stop_daemon() -> Result<()> {
    let config = load_config()?;
    let pid_path = get_pid_file(&config)?;

    let info = match check_daemon(&pid_path)? {
        Some(info) => info,
        None => {
            println!("Daemon is not running");
            return Ok(());
        }
    };

    println!("Stopping daemon (PID: {})...", info.pid);
    crate::pid::send_sigterm(info.pid)?;

    for _ in 0..50 {
        if !crate::pid::is_process_running(info.pid) {
            println!("Daemon stopped");
            let _ = remove_pid_file(&pid_path);
            let socket_path = get_socket_path(&config)?;
            if socket_path.exists() {
                let _ = fs::remove_file(&socket_path);
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    anyhow::bail!("Daemon did not stop within 5 seconds")
}

/// Check and display daemon status
pub fn daemon_status() -> Result<()> {
    let config = load_config()?;
    let pid_path = get_pid_file(&config)?;

    match check_daemon(&pid_path)? {
        Some(info) => {
            let running_version = &info.version;
            let cli_version = env!("CARGO_PKG_VERSION");

            println!("Daemon status: running");
            println!("  PID: {}", info.pid);
            println!("  Version: {}", running_version);
            println!("  Socket: {}", info.socket);
            println!("  Started: {}", info.started_at);

            if running_version != cli_version {
                println!();
                println!("Warning: Daemon version ({}) differs from CLI version ({})",
                         running_version, cli_version);
                println!("Consider running: memex daemon restart");
            }
        }
        None => {
            println!("Daemon status: not running");
        }
    }

    Ok(())
}
