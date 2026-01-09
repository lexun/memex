//! Daemon process for Memex
//!
//! The daemon holds database connections for forge and atlas, handling requests
//! from clients via a Unix socket using JSON-RPC style messages.

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

use atlas::{Event, EventSource, Extractor, MemoSource, QueryDecomposer, Store as AtlasStore};
use cortex::{WorkerConfig, WorkerId, WorkerManager};
use db::Database;
use forge::{Store as ForgeStore, Task, TaskStatus};
use ipc::{Error as IpcError, ErrorCode, Request, Response};
use llm::LlmClient;
use serde_json::json;

use crate::config::{get_config_dir, get_db_path, get_pid_file, get_socket_path, load_config, Config};
use crate::pid::{check_daemon, remove_pid_file, write_pid_file, PidInfo};

/// Container for stores and services
struct Stores {
    forge: ForgeStore,
    atlas: AtlasStore,
    extractor: Option<Extractor>,
    workers: WorkerManager,
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
        let stores = Arc::new(Stores {
            forge: ForgeStore::new(forge_db),
            atlas: AtlasStore::new(atlas_db),
            extractor,
            workers: WorkerManager::new(),
        });

        // Bind the socket
        let listener = UnixListener::bind(&self.socket_path)
            .with_context(|| format!("Failed to bind socket: {}", self.socket_path.display()))?;

        tracing::info!("Daemon listening on {}", self.socket_path.display());

        // Write PID file
        let pid_info = PidInfo::new(process::id(), &self.socket_path);
        write_pid_file(&self.pid_path, &pid_info)?;

        // Run the server
        let result = self.run_server(listener, stores).await;
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
async fn handle_request(request: &Request, stores: &Stores) -> Response {
    let result = dispatch_request(request, stores).await;

    match result {
        Ok(value) => Response::success(&request.id, value).unwrap_or_else(|e| {
            Response::error(&request.id, IpcError::internal(format!("Serialization error: {}", e)))
        }),
        Err(err) => Response::error(&request.id, err),
    }
}

/// Dispatch request to the appropriate handler
async fn dispatch_request(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    match request.method.as_str() {
        // Health check
        "health_check" => Ok(serde_json::json!(true)),

        // Status
        "status" => Ok(serde_json::json!({
            "status": "running",
            "version": env!("CARGO_PKG_VERSION"),
            "pid": process::id()
        })),

        // Task operations (forge) - these also emit events to atlas
        "create_task" => handle_create_task(request, stores).await,
        "list_tasks" => handle_list_tasks(request, &stores.forge).await,
        "ready_tasks" => handle_ready_tasks(request, &stores.forge).await,
        "get_task" => handle_get_task(request, &stores.forge).await,
        "update_task" => handle_update_task(request, stores).await,
        "close_task" => handle_close_task(request, stores).await,
        "delete_task" => handle_delete_task(request, stores).await,

        // Note operations (forge) - these also emit events to atlas
        "add_note" => handle_add_note(request, stores).await,
        "get_notes" => handle_get_notes(request, &stores.forge).await,
        "edit_note" => handle_edit_note(request, stores).await,
        "delete_note" => handle_delete_note(request, stores).await,

        // Dependency operations (forge) - these also emit events to atlas
        "add_dependency" => handle_add_dependency(request, stores).await,
        "remove_dependency" => handle_remove_dependency(request, stores).await,
        "get_dependencies" => handle_get_dependencies(request, &stores.forge).await,

        // Memo operations (atlas) - record_memo also triggers extraction
        "record_memo" => handle_record_memo(request, stores).await,
        "list_memos" => handle_list_memos(request, &stores.atlas).await,
        "get_memo" => handle_get_memo(request, &stores.atlas).await,
        "delete_memo" => handle_delete_memo(request, &stores.atlas).await,

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

        // Entity operations
        "list_entities" => handle_list_entities(request, &stores.atlas).await,
        "get_entity_facts" => handle_get_entity_facts(request, &stores.atlas).await,
        "get_related_facts" => handle_get_related_facts(request, &stores.atlas).await,

        // Cortex operations (worker management)
        "cortex_create_worker" => handle_cortex_create_worker(request, &stores.workers).await,
        "cortex_send_message" => handle_cortex_send_message(request, &stores.workers).await,
        "cortex_worker_status" => handle_cortex_worker_status(request, &stores.workers).await,
        "cortex_list_workers" => handle_cortex_list_workers(&stores.workers).await,
        "cortex_remove_worker" => handle_cortex_remove_worker(request, &stores.workers).await,

        // Unknown method
        _ => Err(IpcError::method_not_found(&request.method)),
    }
}

// ========== Task Handlers ==========

async fn handle_create_task(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let task: Task = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid task: {}", e)))?;

    let created = stores
        .forge
        .create_task(task)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.created event to Atlas
    let task_json = serde_json::to_value(&created).unwrap();
    let event = Event::new(
        "task.created",
        EventSource::system("forge").with_via("daemon"),
        json!({ "task": task_json }),
    );
    if let Err(e) = stores.atlas.record_event(event).await {
        tracing::warn!("Failed to record task.created event: {}", e);
    }

    // Extract knowledge from task content if extractor is available
    if let Some(ref extractor) = stores.extractor {
        // Build content from title and description
        let content = if let Some(ref desc) = created.description {
            format!("{}\n\n{}", created.title, desc)
        } else {
            created.title.clone()
        };

        // Only extract if there's meaningful content (not just a short title)
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

async fn handle_list_tasks(request: &Request, store: &ForgeStore) -> Result<serde_json::Value, IpcError> {
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

async fn handle_ready_tasks(request: &Request, store: &ForgeStore) -> Result<serde_json::Value, IpcError> {
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

async fn handle_get_task(request: &Request, store: &ForgeStore) -> Result<serde_json::Value, IpcError> {
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
    title: Option<String>,
    /// Use Some("value") to set, or explicitly null to clear
    description: Option<Option<String>>,
    /// Use Some("value") to set, or explicitly null to clear
    project: Option<Option<String>>,
}

async fn handle_update_task(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: UpdateTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let status = match &params.status {
        Some(s) => Some(s.parse::<TaskStatus>().map_err(|e| IpcError::invalid_params(e.to_string()))?),
        None => None,
    };

    let updated = stores
        .forge
        .update_task(
            &params.id,
            status,
            params.priority,
            params.title.as_deref(),
            params.description.as_ref().map(|d| d.as_deref()),
            params.project.as_ref().map(|p| p.as_deref()),
        )
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.updated event to Atlas
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
            EventSource::system("forge").with_via("daemon"),
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
    reason: Option<String>,
}

async fn handle_close_task(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: CloseTaskParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let closed = stores
        .forge
        .close_task(&params.id, params.reason.as_deref())
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.closed event to Atlas
    if let Some(ref task) = closed {
        let task_json = serde_json::to_value(task).unwrap();
        let event = Event::new(
            "task.closed",
            EventSource::system("forge").with_via("daemon"),
            json!({
                "task_id": params.id,
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

    let deleted = stores
        .forge
        .delete_task(&params.id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.deleted event to Atlas (includes reason for context)
    if let Some(ref task) = deleted {
        let task_json = serde_json::to_value(task).unwrap();
        let event = Event::new(
            "task.deleted",
            EventSource::system("forge").with_via("daemon"),
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

    Ok(serde_json::to_value(deleted).unwrap())
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

    let note = stores
        .forge
        .add_note(&params.task_id, &params.content)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.note_added event to Atlas
    let note_json = serde_json::to_value(&note).unwrap();
    let event = Event::new(
        "task.note_added",
        EventSource::system("forge").with_via("daemon"),
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
        // Only extract if there's meaningful content
        if params.content.len() > 20 {
            // Get task to determine project context
            let project = stores.forge.get_task(&params.task_id).await
                .ok()
                .flatten()
                .and_then(|t| t.project);

            let note_id = note.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();

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

async fn handle_get_notes(request: &Request, store: &ForgeStore) -> Result<serde_json::Value, IpcError> {
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

async fn handle_edit_note(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: EditNoteParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let updated = stores
        .forge
        .edit_note(&params.note_id, &params.content)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.note_updated event to Atlas
    if let Some(ref note) = updated {
        let note_json = serde_json::to_value(note).unwrap();
        let event = Event::new(
            "task.note_updated",
            EventSource::system("forge").with_via("daemon"),
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

    let deleted = stores
        .forge
        .delete_note(&params.note_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.note_deleted event to Atlas
    if let Some(ref note) = deleted {
        let note_json = serde_json::to_value(note).unwrap();
        let event = Event::new(
            "task.note_deleted",
            EventSource::system("forge").with_via("daemon"),
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

    let dep = stores
        .forge
        .add_dependency(&params.from_id, &params.to_id, &params.relation)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.dependency_added event to Atlas
    let event = Event::new(
        "task.dependency_added",
        EventSource::system("forge").with_via("daemon"),
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
        .forge
        .remove_dependency(&params.from_id, &params.to_id, &params.relation)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    // Emit task.dependency_removed event to Atlas
    if removed {
        let event = Event::new(
            "task.dependency_removed",
            EventSource::system("forge").with_via("daemon"),
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

async fn handle_get_dependencies(request: &Request, store: &ForgeStore) -> Result<serde_json::Value, IpcError> {
    let params: GetDependenciesParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    store
        .get_dependencies(&params.task_id)
        .await
        .map(|deps| serde_json::to_value(deps).unwrap())
        .map_err(|e| IpcError::internal(e.to_string()))
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

    // Store facts and create entity links
    for extracted_fact in result.facts {
        match stores.atlas.create_fact(extracted_fact.fact).await {
            Ok(created_fact) => {
                // Link fact to its referenced entities
                if let (Some(ref fact_id), entity_refs) = (&created_fact.id, &extracted_fact.entity_refs) {
                    for entity_name in entity_refs {
                        if let Some(entity) = entity_map.get(entity_name) {
                            if let Some(ref entity_id) = entity.id {
                                if let Err(e) = stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await {
                                    tracing::warn!("Failed to link fact to entity '{}': {}", entity_name, e);
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

    // Store facts and create entity links
    for extracted_fact in result.facts {
        match stores.atlas.create_fact(extracted_fact.fact).await {
            Ok(created_fact) => {
                facts_created += 1;
                // Link fact to its referenced entities
                if let (Some(ref fact_id), entity_refs) = (&created_fact.id, &extracted_fact.entity_refs) {
                    for entity_name in entity_refs {
                        if let Some(entity) = entity_map.get(entity_name) {
                            if let Some(ref entity_id) = entity.id {
                                if stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await.is_ok() {
                                    links_created += 1;
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
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

/// Query knowledge and return an LLM-summarized answer
async fn handle_query_knowledge(request: &Request, stores: &Stores) -> Result<serde_json::Value, IpcError> {
    let params: KnowledgeParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

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
                Some(params.limit),
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

    // Entity-focused expansion
    // For each keyword, find matching entities and include their linked facts
    const ENTITY_SCORE: f64 = 0.4; // Lower than direct matches to rank after them
    for keyword in &keywords {
        let entity_results = stores
            .atlas
            .expand_via_entities(
                keyword,
                params.project.as_deref(),
                ENTITY_SCORE,
                Some(5), // Limit entity-linked facts per keyword
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

    // HyDE search - Search using hypothetical answer embedding
    // This helps find facts semantically similar to what the answer might look like
    if let Some(ref hypo_emb) = hypothetical_embedding {
        const HYDE_SCORE: f64 = 0.35; // Lower than direct matches

        let hyde_results = stores
            .atlas
            .vector_search_facts_temporal(
                hypo_emb,
                params.project.as_deref(),
                Some(5), // Limit HyDE results
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
    all_results.truncate(params.limit);

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
    let context = results
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{}. {} (confidence: {:.0}%)", i + 1, r.content, r.confidence * 100.0))
        .collect::<Vec<_>>()
        .join("\n");

    // Use LLM to summarize
    let system = "You are a helpful assistant that answers questions based on facts from a knowledge base. \
        Provide direct, concise answers. If the facts don't fully answer the question, say what you know and note what's missing.";

    let user = format!(
        "Question: {}\n\nKnown facts:\n{}\n\nAnswer the question based on these facts.",
        params.query,
        context
    );

    let answer = extractor
        .client()
        .complete(system, &user)
        .await
        .map_err(|e| IpcError::internal(format!("LLM completion failed: {}", e)))?;

    Ok(json!({
        "query": params.query,
        "answer": answer,
        "facts_used": facts_used,
    }))
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
            Some(params.limit),
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

    Ok(json!({
        "facts": {
            "total": total_facts,
            "with_embeddings": with_embeddings,
            "without_embeddings": without_embeddings,
        },
        "llm_configured": llm_configured,
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
                        let fact_id = id.id.to_raw();
                        if stores.atlas.update_fact_embedding(&fact_id, embedding).await.is_ok() {
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
struct GetRelatedFactsParams {
    fact_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

async fn handle_get_related_facts(request: &Request, store: &AtlasStore) -> Result<serde_json::Value, IpcError> {
    let params: GetRelatedFactsParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let facts = store
        .get_related_facts(&params.fact_id, params.limit)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(json!({
        "fact_id": params.fact_id,
        "related_facts": facts,
        "count": facts.len(),
    }))
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

                // Store facts and create entity links
                for extracted_fact in result.facts {
                    if let Ok(created_fact) = stores.atlas.create_fact(extracted_fact.fact).await {
                        facts_created += 1;

                        if let Some(ref fact_id) = created_fact.id {
                            for entity_name in &extracted_fact.entity_refs {
                                if let Some(entity) = entity_map.get(entity_name) {
                                    if let Some(ref entity_id) = entity.id {
                                        if stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await.is_ok() {
                                            links_created += 1;
                                        }
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

                // Store facts and create entity links
                for extracted_fact in result.facts {
                    if let Ok(created_fact) = stores.atlas.create_fact(extracted_fact.fact).await {
                        facts_created += 1;

                        // Link fact to its referenced entities
                        if let Some(ref fact_id) = created_fact.id {
                            for entity_name in &extracted_fact.entity_refs {
                                if let Some(entity) = entity_map.get(entity_name) {
                                    if let Some(ref entity_id) = entity.id {
                                        if stores.atlas.link_fact_entity(fact_id, entity_id, "mentions").await.is_ok() {
                                            links_created += 1;
                                        }
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

// ========== Cortex Handlers ==========

#[derive(Deserialize)]
struct CreateWorkerParams {
    cwd: String,
    model: Option<String>,
    system_prompt: Option<String>,
}

async fn handle_cortex_create_worker(
    request: &Request,
    workers: &WorkerManager,
) -> Result<serde_json::Value, IpcError> {
    let params: CreateWorkerParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let mut config = WorkerConfig::new(&params.cwd);
    if let Some(model) = params.model {
        config = config.with_model(model);
    }
    if let Some(prompt) = params.system_prompt {
        config = config.with_system_prompt(prompt);
    }

    let worker_id = workers
        .create(config)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(json!(worker_id.0))
}

#[derive(Deserialize)]
struct SendMessageParams {
    worker_id: String,
    message: String,
}

async fn handle_cortex_send_message(
    request: &Request,
    workers: &WorkerManager,
) -> Result<serde_json::Value, IpcError> {
    let params: SendMessageParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);

    let response = workers
        .send_message(&worker_id, &params.message)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

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

async fn handle_cortex_remove_worker(
    request: &Request,
    workers: &WorkerManager,
) -> Result<serde_json::Value, IpcError> {
    let params: WorkerIdParams = serde_json::from_value(request.params.clone())
        .map_err(|e| IpcError::invalid_params(format!("Invalid params: {}", e)))?;

    let worker_id = WorkerId::from_string(&params.worker_id);

    workers
        .remove(&worker_id)
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?;

    Ok(json!(true))
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
