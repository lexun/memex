//! Web server integration for the daemon
//!
//! Provides a local web UI organized into three sections:
//! - Work: Tasks, Workers (active operations)
//! - Directory: People, Teams, Companies, Projects, Repos, Rules, Skills, Documents, Technologies
//! - Activity: Memos, Threads, Events (inputs/event sourcing)
//!
//! The UI is built with Leptos and the WASM/JS bundle is embedded in the binary.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};

use crate::config::WebConfig;

// Embedded WASM/JS assets - built by wasm-pack before cargo build
const WASM_JS: &str = include_str!("../../web/pkg/memex_web.js");
const WASM_BG: &[u8] = include_bytes!("../../web/pkg/memex_web_bg.wasm");
const MAIN_CSS: &str = include_str!("../../web/style/main.css");

/// Shared state for web handlers
pub struct WebState {
    pub forge: forge::Store,
    pub atlas: atlas::Store,
    pub workers: cortex::WorkerManager,
}

/// Build the web router
pub fn build_router(state: Arc<WebState>, _config: &WebConfig) -> Router {
    Router::new()
        // API endpoints
        .route("/api/stats", get(api_stats))
        .route("/api/tasks", get(api_list_tasks))
        .route("/api/tasks/:id", get(api_get_task))
        .route("/api/workers", get(api_list_workers))
        .route("/api/workers/:id", get(api_get_worker))
        .route("/api/workers/:id/transcript", get(api_get_worker_transcript))
        .route("/api/activity", get(api_get_activity_feed))
        .route("/api/records/:record_type", get(api_list_records_by_type))
        .route("/api/record/:id", get(api_get_record))
        .route("/api/memos", get(api_list_memos))
        .route("/api/events", get(api_list_events))
        // Serve embedded WASM/JS/CSS assets
        .route("/pkg/memex_web.js", get(serve_wasm_js))
        .route("/pkg/memex_web_bg.wasm", get(serve_wasm_bg))
        .route("/style/main.css", get(serve_main_css))
        // Fallback to index.html for SPA routing
        .fallback(get(spa_fallback))
        .with_state(state)
}

// -----------------------------------------------------------------------------
// Embedded asset handlers
// -----------------------------------------------------------------------------

async fn serve_wasm_js() -> Response {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        WASM_JS,
    )
        .into_response()
}

async fn serve_wasm_bg() -> Response {
    (
        [(header::CONTENT_TYPE, "application/wasm")],
        WASM_BG,
    )
        .into_response()
}

async fn serve_main_css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css")],
        MAIN_CSS,
    )
        .into_response()
}

// -----------------------------------------------------------------------------
// View models for API responses
// -----------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
struct DashboardStats {
    records: usize,
    tasks: usize,
    memos: usize,
}

#[derive(Clone, serde::Serialize)]
struct TaskView {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: i32,
    project: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, serde::Serialize)]
struct TaskDetailResponse {
    task: TaskView,
    notes: Vec<NoteView>,
    assigned_workers: Vec<WorkerView>,
}

#[derive(Clone, serde::Serialize)]
struct WorkerView {
    id: String,
    state: String,
    current_task: Option<String>,
    worktree: Option<String>,
    cwd: String,
    model: Option<String>,
    messages_sent: u64,
    messages_received: u64,
    started_at: String,
    last_activity: String,
    error_message: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct NoteView {
    id: String,
    content: String,
    created_at: String,
}

#[derive(Clone, serde::Serialize)]
struct RecordView {
    id: String,
    record_type: String,
    name: String,
    description: Option<String>,
    content: serde_json::Value,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, serde::Serialize)]
struct RecordDetailView {
    record: RecordView,
    related: Vec<RecordView>,
}

#[derive(Clone, serde::Serialize)]
struct MemoView {
    id: String,
    content: String,
    source: String,
    created_at: String,
}

#[derive(Clone, serde::Serialize)]
struct EventView {
    id: String,
    event_type: String,
    source: String,
    timestamp: String,
    summary: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct TranscriptEntryView {
    timestamp: String,
    prompt: String,
    response: Option<String>,
    is_error: bool,
    duration_ms: u64,
}

#[derive(Clone, serde::Serialize)]
struct WorkerTranscriptView {
    source: String,
    thread_id: Option<String>,
    entries: Vec<TranscriptEntryView>,
}

#[derive(Clone, serde::Serialize)]
struct ActivityEntry {
    worker_id: String,
    worker_state: String,
    current_task: Option<String>,
    timestamp: String,
    prompt: String,
    response: Option<String>,
    is_error: bool,
    duration_ms: u64,
}

#[derive(Clone, serde::Serialize)]
struct ActivityFeed {
    entries: Vec<ActivityEntry>,
}

// -----------------------------------------------------------------------------
// SPA fallback - serve the Leptos app shell
// -----------------------------------------------------------------------------

async fn spa_fallback() -> impl IntoResponse {
    // Return a minimal HTML that loads the Leptos app
    Html(include_str!("../templates/shell.html"))
}

// -----------------------------------------------------------------------------
// API handlers
// -----------------------------------------------------------------------------

async fn api_stats(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let tasks = state.forge.list_tasks(None, None).await.unwrap_or_default().len();
    let records = state.atlas.list_records(None, false, None).await.unwrap_or_default().len();
    let memos = state.atlas.list_memos(None).await.unwrap_or_default().len();

    Json(DashboardStats {
        records,
        tasks,
        memos,
    })
}

async fn api_list_tasks(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.forge.list_tasks(None, None).await {
        Ok(tasks) => {
            let tasks: Vec<TaskView> = tasks
                .into_iter()
                .map(|t| TaskView {
                    id: t.id.map(|id| id.id.to_string()).unwrap_or_default(),
                    title: t.title,
                    description: t.description,
                    status: t.status.to_string(),
                    priority: t.priority,
                    project: t.project,
                    created_at: t.created_at.to_string(),
                    updated_at: t.updated_at.to_string(),
                })
                .collect();
            Json(tasks).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list tasks: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list tasks").into_response()
        }
    }
}

async fn api_get_task(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Get the task
    let task = match state.forge.get_task(&id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "Task not found").into_response();
        }
        Err(e) => {
            tracing::error!("Failed to get task {}: {}", id, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get task").into_response();
        }
    };

    // Get notes for this task
    let notes: Vec<NoteView> = state
        .forge
        .get_notes(&id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|n| NoteView {
            id: n.id.map(|t| t.id.to_string()).unwrap_or_default(),
            content: n.content,
            created_at: n.created_at.to_string(),
        })
        .collect();

    // Get workers assigned to this task
    let assigned_workers: Vec<WorkerView> = state
        .forge
        .get_workers_by_task(&id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(worker_to_view)
        .collect();

    let task_view = TaskView {
        id: task.id.map(|id| id.id.to_string()).unwrap_or_default(),
        title: task.title,
        description: task.description,
        status: task.status.to_string(),
        priority: task.priority,
        project: task.project,
        created_at: task.created_at.to_string(),
        updated_at: task.updated_at.to_string(),
    };

    Json(TaskDetailResponse {
        task: task_view,
        notes,
        assigned_workers,
    })
    .into_response()
}

fn worker_to_view(w: forge::DbWorker) -> WorkerView {
    WorkerView {
        id: w.worker_id,
        state: w.state,
        current_task: w.current_task,
        worktree: w.worktree,
        cwd: w.cwd,
        model: w.model,
        messages_sent: w.messages_sent as u64,
        messages_received: w.messages_received as u64,
        started_at: w.started_at.to_string(),
        last_activity: w.last_activity.to_string(),
        error_message: w.error_message,
    }
}

async fn api_list_workers(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.forge.list_workers(None).await {
        Ok(workers) => {
            let workers: Vec<WorkerView> = workers.into_iter().map(worker_to_view).collect();
            Json(workers).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list workers: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list workers").into_response()
        }
    }
}

async fn api_get_worker(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.forge.get_worker(&id).await {
        Ok(Some(w)) => Json(worker_to_view(w)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Worker not found").into_response(),
        Err(e) => {
            tracing::error!("Failed to get worker {}: {}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get worker").into_response()
        }
    }
}

async fn api_get_worker_transcript(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // First try to get persistent transcript from database
    if let Ok(Some(thread)) = state.atlas.get_thread_by_worker(&id).await {
        if let Some(thread_id) = thread.id_str() {
            if let Ok((_, entries)) = state.atlas.get_thread_with_entries(&thread_id, None).await {
                let transcript: Vec<TranscriptEntryView> = entries.iter().filter_map(|entry| {
                    let content = &entry.content;
                    let role = content.get("role").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let is_user = role == "user";

                    Some(TranscriptEntryView {
                        timestamp: entry.created_at.to_string(),
                        prompt: if is_user { entry.description.clone().unwrap_or_default() } else { String::new() },
                        response: if !is_user { entry.description.clone() } else { None },
                        is_error: false,
                        duration_ms: content.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                    })
                }).collect();

                if !transcript.is_empty() {
                    return Json(WorkerTranscriptView {
                        source: "database".to_string(),
                        thread_id: Some(thread_id.to_string()),
                        entries: transcript,
                    }).into_response();
                }
            }
        }
    }

    // Fall back to in-memory transcript
    let worker_id = cortex::WorkerId::from_string(&id);
    match state.workers.transcript(&worker_id, None).await {
        Ok(entries) => {
            let transcript: Vec<TranscriptEntryView> = entries.iter().map(|e| {
                TranscriptEntryView {
                    timestamp: e.timestamp.to_rfc3339(),
                    prompt: e.prompt.clone(),
                    response: e.response.clone(),
                    is_error: e.is_error,
                    duration_ms: e.duration_ms,
                }
            }).collect();

            Json(WorkerTranscriptView {
                source: "memory".to_string(),
                thread_id: None,
                entries: transcript,
            }).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get transcript for worker {}: {}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get transcript").into_response()
        }
    }
}

async fn api_list_records_by_type(
    State(state): State<Arc<WebState>>,
    Path(record_type): Path<String>,
) -> impl IntoResponse {
    match state.atlas.list_records(Some(&record_type), false, None).await {
        Ok(records) => {
            let records: Vec<RecordView> = records
                .into_iter()
                .map(|r| RecordView {
                    id: r.id.map(|t| t.id.to_string()).unwrap_or_default(),
                    record_type: r.record_type,
                    name: r.name,
                    description: r.description,
                    content: r.content,
                    created_at: r.created_at.to_string(),
                    updated_at: r.updated_at.to_string(),
                })
                .collect();
            Json(records).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list records: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list records").into_response()
        }
    }
}

async fn api_get_record(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Get the record
    let record = match state.atlas.get_record(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "Record not found").into_response();
        }
        Err(e) => {
            tracing::error!("Failed to get record {}: {}", id, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get record").into_response();
        }
    };

    // Get related records via edges (both directions)
    let mut related: Vec<RecordView> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    seen_ids.insert(id.clone());

    // Get outgoing edges
    if let Ok(edges_out) = state.atlas.get_edges_from(&id, None, true).await {
        for edge in edges_out {
            let target_id = edge.target.id.to_string();
            if !seen_ids.contains(&target_id) {
                seen_ids.insert(target_id.clone());
                if let Ok(Some(r)) = state.atlas.get_record(&target_id).await {
                    related.push(record_to_view(r));
                }
            }
        }
    }

    // Get incoming edges
    if let Ok(edges_in) = state.atlas.get_edges_to(&id, None, true).await {
        for edge in edges_in {
            let source_id = edge.source.id.to_string();
            if !seen_ids.contains(&source_id) {
                seen_ids.insert(source_id.clone());
                if let Ok(Some(r)) = state.atlas.get_record(&source_id).await {
                    related.push(record_to_view(r));
                }
            }
        }
    }

    let record_view = record_to_view(record);

    Json(RecordDetailView {
        record: record_view,
        related,
    })
    .into_response()
}

fn record_to_view(r: atlas::Record) -> RecordView {
    RecordView {
        id: r.id.map(|t| t.id.to_string()).unwrap_or_default(),
        record_type: r.record_type,
        name: r.name,
        description: r.description,
        content: r.content,
        created_at: r.created_at.to_string(),
        updated_at: r.updated_at.to_string(),
    }
}

async fn api_list_memos(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.atlas.list_memos(Some(50)).await {
        Ok(memos) => {
            let memos: Vec<MemoView> = memos
                .into_iter()
                .map(|m| MemoView {
                    id: m.id.map(|t| t.id.to_string()).unwrap_or_default(),
                    content: m.content,
                    source: m.source.actor.clone(),
                    created_at: m.created_at.to_string(),
                })
                .collect();
            Json(memos).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list memos: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list memos").into_response()
        }
    }
}

async fn api_list_events(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.atlas.list_events(Some("task."), Some(50)).await {
        Ok(events) => {
            let events: Vec<EventView> = events
                .into_iter()
                .map(|e| {
                    let summary = extract_event_summary(&e.event_type, &e.payload);
                    EventView {
                        id: e.id.map(|t| t.id.to_string()).unwrap_or_default(),
                        event_type: e.event_type,
                        source: e.source.actor,
                        timestamp: e.timestamp.to_string(),
                        summary,
                    }
                })
                .collect();
            Json(events).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list events: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list events").into_response()
        }
    }
}

/// Extract a human-readable summary from an event payload
fn extract_event_summary(event_type: &str, payload: &serde_json::Value) -> Option<String> {
    match event_type {
        "task.created" => payload
            .get("task")
            .and_then(|t| t.get("title"))
            .and_then(|t| t.as_str())
            .map(|title| format!("Created task: {}", title)),
        "task.updated" => {
            let task_id = payload
                .get("task_id")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            let changes = payload
                .get("changes")
                .map(|c| {
                    let keys: Vec<&str> = c
                        .as_object()
                        .map(|o| o.keys().map(|s| s.as_str()).collect())
                        .unwrap_or_default();
                    keys.join(", ")
                })
                .unwrap_or_default();
            Some(format!("Updated task {}: {}", task_id, changes))
        }
        "task.closed" => {
            let task_id = payload
                .get("task_id")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            let reason = payload.get("reason").and_then(|r| r.as_str());
            match reason {
                Some(r) => Some(format!("Closed task {}: {}", task_id, r)),
                None => Some(format!("Closed task {}", task_id)),
            }
        }
        "task.deleted" => {
            let task_id = payload
                .get("task_id")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            Some(format!("Deleted task {}", task_id))
        }
        "task.note_added" => {
            let task_id = payload
                .get("task_id")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            Some(format!("Note added to task {}", task_id))
        }
        _ => None,
    }
}

async fn api_get_activity_feed(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    // Get all workers
    let workers = match state.forge.list_workers(None).await {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("Failed to list workers for activity feed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get workers").into_response();
        }
    };

    let mut all_entries: Vec<ActivityEntry> = Vec::new();

    // Get transcript for each worker and combine entries
    for worker in &workers {
        let worker_id = cortex::WorkerId::from_string(&worker.worker_id);

        // Get transcript for this worker (limit to last 10 entries per worker)
        if let Ok(entries) = state.workers.transcript(&worker_id, Some(10)).await {
            for e in entries {
                all_entries.push(ActivityEntry {
                    worker_id: worker.worker_id.clone(),
                    worker_state: worker.state.clone(),
                    current_task: worker.current_task.clone(),
                    timestamp: e.timestamp.to_rfc3339(),
                    prompt: e.prompt,
                    response: e.response,
                    is_error: e.is_error,
                    duration_ms: e.duration_ms,
                });
            }
        }
    }

    // Sort by timestamp (newest first)
    all_entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // Limit total entries
    all_entries.truncate(50);

    Json(ActivityFeed { entries: all_entries }).into_response()
}
