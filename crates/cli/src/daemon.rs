//! Daemon process for Memex
//!
//! The daemon holds the database connection and handles requests from clients
//! via a Unix socket using JSON-RPC style messages.

use std::fs::{self, File};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use nix::unistd::{fork, setsid, ForkResult};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use forge::{Store, Task, TaskStatus};
use ipc::{Error as IpcError, ErrorCode, Request, Response};

use crate::config::{get_config_dir, get_db_path, get_pid_file, get_socket_path, load_config, Config};
use crate::pid::{check_daemon, remove_pid_file, write_pid_file, PidInfo};

/// The daemon process
pub struct Daemon {
    #[allow(dead_code)]
    config: Config,
    socket_path: PathBuf,
    pid_path: PathBuf,
    db_path: PathBuf,
}

impl Daemon {
    /// Create a new daemon instance
    pub fn new() -> Result<Self> {
        let config = load_config()?;
        let socket_path = get_socket_path(&config)?;
        let pid_path = get_pid_file(&config)?;
        let db_path = get_db_path(&config)?;

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
            db_path,
        })
    }

    /// Start the daemon (forks to background)
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

        // Build and run tokio runtime in daemon process
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.run_daemon())
    }

    /// Main daemon loop
    async fn run_daemon(self) -> Result<()> {
        // Remove stale socket
        if self.socket_path.exists() {
            fs::remove_file(&self.socket_path)
                .with_context(|| format!("Failed to remove stale socket: {}", self.socket_path.display()))?;
        }

        // Initialize the database store
        tracing::info!("Opening database at: {}", self.db_path.display());
        let store = Store::new(&self.db_path).await?;
        let store = Arc::new(store);

        // Bind the socket
        let listener = UnixListener::bind(&self.socket_path)
            .with_context(|| format!("Failed to bind socket: {}", self.socket_path.display()))?;

        tracing::info!("Daemon listening on {}", self.socket_path.display());

        // Write PID file
        let pid_info = PidInfo::new(process::id(), &self.socket_path);
        write_pid_file(&self.pid_path, &pid_info)?;

        // Run the server
        let result = self.run_server(listener, store).await;
        self.cleanup();
        result
    }

    /// Accept and handle connections
    async fn run_server(&self, listener: UnixListener, store: Arc<Store>) -> Result<()> {
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _addr)) => {
                            let store = Arc::clone(&store);
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, store).await {
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

/// Handle a single client connection
async fn handle_connection(stream: UnixStream, store: Arc<Store>) -> Result<()> {
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
        let response = handle_request(&request, &store).await;

        // Send response
        let mut json = serde_json::to_string(&response)?;
        json.push('\n');
        writer.write_all(json.as_bytes()).await?;
    }

    Ok(())
}

/// Handle a single request
async fn handle_request(request: &Request, store: &Store) -> Response {
    let result = dispatch_request(request, store).await;

    match result {
        Ok(value) => Response::success(&request.id, value).unwrap_or_else(|e| {
            Response::error(&request.id, IpcError::internal(format!("Serialization error: {}", e)))
        }),
        Err(err) => Response::error(&request.id, err),
    }
}

/// Dispatch request to the appropriate handler
async fn dispatch_request(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    match request.method.as_str() {
        // Health check
        "health_check" => Ok(serde_json::json!(true)),

        // Status
        "status" => Ok(serde_json::json!({
            "status": "running",
            "version": env!("CARGO_PKG_VERSION"),
            "pid": process::id()
        })),

        // Task operations
        "create_task" => handle_create_task(request, store).await,
        "list_tasks" => handle_list_tasks(request, store).await,
        "ready_tasks" => handle_ready_tasks(request, store).await,
        "get_task" => handle_get_task(request, store).await,
        "update_task" => handle_update_task(request, store).await,
        "close_task" => handle_close_task(request, store).await,
        "delete_task" => handle_delete_task(request, store).await,

        // Note operations
        "add_note" => handle_add_note(request, store).await,
        "get_notes" => handle_get_notes(request, store).await,
        "edit_note" => handle_edit_note(request, store).await,
        "delete_note" => handle_delete_note(request, store).await,

        // Dependency operations
        "add_dependency" => handle_add_dependency(request, store).await,
        "remove_dependency" => handle_remove_dependency(request, store).await,
        "get_dependencies" => handle_get_dependencies(request, store).await,

        // Unknown method
        _ => Err(IpcError::method_not_found(&request.method)),
    }
}

// ========== Task Handlers ==========

async fn handle_create_task(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let task: Task = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid task: {}", e)))?;

    store
        .create_task(task)
        .await
        .map(|t| serde_json::to_value(t).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct ListTasksParams {
    project: Option<String>,
    status: Option<String>,
}

async fn handle_list_tasks(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: ListTasksParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ListTasksParams { project: None, status: None });

    let status = params.status.and_then(|s| s.parse::<TaskStatus>().ok());

    store
        .list_tasks(params.project.as_deref(), status)
        .await
        .map(|tasks| serde_json::to_value(tasks).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct ReadyTasksParams {
    project: Option<String>,
}

async fn handle_ready_tasks(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: ReadyTasksParams = serde_json::from_value(request.params.clone())
        .unwrap_or(ReadyTasksParams { project: None });

    store
        .ready_tasks(params.project.as_deref())
        .await
        .map(|tasks| serde_json::to_value(tasks).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct GetTaskParams {
    id: String,
}

async fn handle_get_task(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: GetTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Missing id: {}", e)))?;

    store
        .get_task(&params.id)
        .await
        .map(|task| serde_json::to_value(task).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct UpdateTaskParams {
    id: String,
    status: Option<String>,
    priority: Option<i32>,
}

async fn handle_update_task(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: UpdateTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let status = match params.status {
        Some(s) => Some(s.parse::<TaskStatus>().map_err(|e| IpcError::invalid_params(e.to_string()))?),
        None => None,
    };

    store
        .update_task(&params.id, status, params.priority)
        .await
        .map(|task| serde_json::to_value(task).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct CloseTaskParams {
    id: String,
    reason: Option<String>,
}

async fn handle_close_task(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: CloseTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .close_task(&params.id, params.reason.as_deref())
        .await
        .map(|task| serde_json::to_value(task).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct DeleteTaskParams {
    id: String,
}

async fn handle_delete_task(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: DeleteTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .delete_task(&params.id)
        .await
        .map(|task| serde_json::to_value(task).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

// ========== Note Handlers ==========

#[derive(Deserialize)]
struct AddNoteParams {
    task_id: String,
    content: String,
}

async fn handle_add_note(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: AddNoteParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .add_note(&params.task_id, &params.content)
        .await
        .map(|note| serde_json::to_value(note).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct GetNotesParams {
    task_id: String,
}

async fn handle_get_notes(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: GetNotesParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .get_notes(&params.task_id)
        .await
        .map(|notes| serde_json::to_value(notes).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct EditNoteParams {
    note_id: String,
    content: String,
}

async fn handle_edit_note(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: EditNoteParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .edit_note(&params.note_id, &params.content)
        .await
        .map(|note| serde_json::to_value(note).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct DeleteNoteParams {
    note_id: String,
}

async fn handle_delete_note(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: DeleteNoteParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .delete_note(&params.note_id)
        .await
        .map(|note| serde_json::to_value(note).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

// ========== Dependency Handlers ==========

#[derive(Deserialize)]
struct AddDependencyParams {
    from_id: String,
    to_id: String,
    relation: String,
}

async fn handle_add_dependency(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: AddDependencyParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .add_dependency(&params.from_id, &params.to_id, &params.relation)
        .await
        .map(|dep| serde_json::to_value(dep).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct RemoveDependencyParams {
    from_id: String,
    to_id: String,
    relation: String,
}

async fn handle_remove_dependency(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: RemoveDependencyParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .remove_dependency(&params.from_id, &params.to_id, &params.relation)
        .await
        .map(|removed| serde_json::to_value(removed).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct GetDependenciesParams {
    task_id: String,
}

async fn handle_get_dependencies(request: &Request, store: &Store) -> Result<serde_json::Value, IpcError> {
    let params: GetDependenciesParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .get_dependencies(&params.task_id)
        .await
        .map(|deps| serde_json::to_value(deps).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
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
