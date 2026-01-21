//! Web server integration for the daemon
//!
//! Provides a local web UI organized into three sections:
//! - Work: Tasks, Workers (active operations)
//! - Directory: People, Teams, Projects, Repos, Rules (entities)
//! - Activity: Memos, Threads, Events (inputs/event sourcing)

use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Json, Router,
};
use tower_http::services::ServeDir;

use crate::config::WebConfig;

/// Shared state for web handlers
pub struct WebState {
    pub forge: forge::Store,
    pub atlas: atlas::Store,
}

/// Build the web router
pub fn build_router(state: Arc<WebState>, _config: &WebConfig) -> Router {
    Router::new()
        // Home redirects to tasks (primary workspace)
        .route("/", get(|| async { Redirect::to("/tasks") }))

        // Work section
        .route("/tasks", get(tasks_page))
        .route("/workers", get(workers_page))

        // Directory section
        .route("/people", get(people_page))
        .route("/teams", get(teams_page))
        .route("/projects", get(projects_page))
        .route("/repos", get(repos_page))
        .route("/rules", get(rules_page))

        // Activity section
        .route("/memos", get(memos_page))
        .route("/threads", get(threads_page))
        .route("/events", get(events_page))

        // API endpoints
        .route("/api/tasks", get(api_list_tasks))
        .route("/api/memos", get(api_list_memos))
        .route("/api/records", get(api_list_records))
        .route("/api/records/:record_type", get(api_list_records_by_type))

        // Static assets
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}

// -----------------------------------------------------------------------------
// Templates
// -----------------------------------------------------------------------------

#[derive(askama::Template)]
#[template(path = "tasks.html")]
struct TasksTemplate {
    title: &'static str,
    active_section: &'static str,
    tasks: Vec<TaskView>,
    pending_count: usize,
    in_progress_count: usize,
    done_count: usize,
}

#[derive(askama::Template)]
#[template(path = "workers.html")]
struct WorkersTemplate {
    title: &'static str,
    active_section: &'static str,
    workers: Vec<WorkerView>,
}

#[derive(askama::Template)]
#[template(path = "people.html")]
struct PeopleTemplate {
    title: &'static str,
    active_section: &'static str,
    people: Vec<RecordView>,
}

#[derive(askama::Template)]
#[template(path = "teams.html")]
struct TeamsTemplate {
    title: &'static str,
    active_section: &'static str,
    teams: Vec<RecordView>,
}

#[derive(askama::Template)]
#[template(path = "projects.html")]
struct ProjectsTemplate {
    title: &'static str,
    active_section: &'static str,
    projects: Vec<RecordView>,
}

#[derive(askama::Template)]
#[template(path = "repos.html")]
struct ReposTemplate {
    title: &'static str,
    active_section: &'static str,
    repos: Vec<RecordView>,
}

#[derive(askama::Template)]
#[template(path = "rules.html")]
struct RulesTemplate {
    title: &'static str,
    active_section: &'static str,
    rules: Vec<RecordView>,
}

#[derive(askama::Template)]
#[template(path = "memos.html")]
struct MemosTemplate {
    title: &'static str,
    active_section: &'static str,
    memos: Vec<MemoView>,
}

#[derive(askama::Template)]
#[template(path = "threads.html")]
struct ThreadsTemplate {
    title: &'static str,
    active_section: &'static str,
}

#[derive(askama::Template)]
#[template(path = "events.html")]
struct EventsTemplate {
    title: &'static str,
    active_section: &'static str,
    events: Vec<EventView>,
}

// -----------------------------------------------------------------------------
// View models
// -----------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
struct TaskView {
    id: String,
    title: String,
    status: String,
    status_class: String,
    priority: i32,
    priority_class: String,
    project: Option<String>,
    created_at: String,
}

#[derive(Clone, serde::Serialize)]
struct WorkerView {
    id: String,
    state: String,
    current_task: Option<String>,
    worktree: Option<String>,
    messages_sent: u64,
    messages_received: u64,
    last_activity: String,
    error_message: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct RecordView {
    id: String,
    name: String,
    description: Option<String>,
    created_at: String,
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

// -----------------------------------------------------------------------------
// Work section handlers
// -----------------------------------------------------------------------------

async fn tasks_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let all_tasks = state.forge.list_tasks(None, None).await.unwrap_or_default();

    let pending_count = all_tasks.iter().filter(|t| t.status.to_string() == "pending").count();
    let in_progress_count = all_tasks.iter().filter(|t| t.status.to_string() == "in_progress").count();
    let done_count = all_tasks.iter().filter(|t| t.status.to_string() == "done").count();

    let tasks: Vec<TaskView> = all_tasks
        .into_iter()
        .map(|t| {
            let status = t.status.to_string();
            let status_class = status.replace("_", "-");
            let priority_class = if t.priority == 1 { "p1" } else if t.priority == 2 { "p2" } else { "" };
            TaskView {
                id: t.id.map(|id| id.id.to_string()).unwrap_or_default(),
                title: t.title,
                status,
                status_class,
                priority: t.priority,
                priority_class: priority_class.to_string(),
                project: t.project,
                created_at: t.created_at.to_string(),
            }
        })
        .collect();

    let template = TasksTemplate {
        title: "Tasks",
        active_section: "tasks",
        tasks,
        pending_count,
        in_progress_count,
        done_count,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn workers_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    // Get workers from forge store
    let db_workers = state.forge.list_workers(None).await.unwrap_or_default();

    let workers: Vec<WorkerView> = db_workers
        .into_iter()
        .map(|w| WorkerView {
            id: w.worker_id,
            state: w.state,
            current_task: w.current_task,
            worktree: w.worktree,
            messages_sent: w.messages_sent as u64,
            messages_received: w.messages_received as u64,
            last_activity: w.last_activity.to_string(),
            error_message: w.error_message,
        })
        .collect();

    let template = WorkersTemplate {
        title: "Workers",
        active_section: "workers",
        workers,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

// -----------------------------------------------------------------------------
// Directory section handlers
// -----------------------------------------------------------------------------

async fn people_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let records = state.atlas.list_records(Some("person"), false, None).await.unwrap_or_default();
    let people: Vec<RecordView> = records.into_iter().map(record_to_view).collect();

    let template = PeopleTemplate {
        title: "People",
        active_section: "people",
        people,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn teams_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let records = state.atlas.list_records(Some("team"), false, None).await.unwrap_or_default();
    let teams: Vec<RecordView> = records.into_iter().map(record_to_view).collect();

    let template = TeamsTemplate {
        title: "Teams",
        active_section: "teams",
        teams,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn projects_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let records = state.atlas.list_records(Some("initiative"), false, None).await.unwrap_or_default();
    let projects: Vec<RecordView> = records.into_iter().map(record_to_view).collect();

    let template = ProjectsTemplate {
        title: "Projects",
        active_section: "projects",
        projects,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn repos_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let records = state.atlas.list_records(Some("repo"), false, None).await.unwrap_or_default();
    let repos: Vec<RecordView> = records.into_iter().map(record_to_view).collect();

    let template = ReposTemplate {
        title: "Repositories",
        active_section: "repos",
        repos,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn rules_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let records = state.atlas.list_records(Some("rule"), false, None).await.unwrap_or_default();
    let rules: Vec<RecordView> = records.into_iter().map(record_to_view).collect();

    let template = RulesTemplate {
        title: "Rules",
        active_section: "rules",
        rules,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

fn record_to_view(r: atlas::Record) -> RecordView {
    RecordView {
        id: r.id.map(|t| t.id.to_string()).unwrap_or_default(),
        name: r.name,
        description: r.description,
        created_at: r.created_at.to_string(),
    }
}

// -----------------------------------------------------------------------------
// Events section handlers
// -----------------------------------------------------------------------------

async fn memos_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let memos_data = state.atlas.list_memos(Some(50)).await.unwrap_or_default();

    let memos: Vec<MemoView> = memos_data
        .into_iter()
        .map(|m| MemoView {
            id: m.id.map(|t| t.id.to_string()).unwrap_or_default(),
            content: m.content,
            source: m.source.actor.clone(),
            created_at: m.created_at.to_string(),
        })
        .collect();

    let template = MemosTemplate {
        title: "Memos",
        active_section: "memos",
        memos,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn threads_page() -> impl IntoResponse {
    let template = ThreadsTemplate {
        title: "Threads",
        active_section: "threads",
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn events_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    // Get task events (event_type starts with "task.")
    let events_data = state.atlas.list_events(Some("task."), Some(50)).await.unwrap_or_default();

    let events: Vec<EventView> = events_data
        .into_iter()
        .map(|e| {
            // Extract a summary from the payload
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

    let template = EventsTemplate {
        title: "Events",
        active_section: "events",
        events,
    };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

/// Extract a human-readable summary from an event payload
fn extract_event_summary(event_type: &str, payload: &serde_json::Value) -> Option<String> {
    match event_type {
        "task.created" => {
            payload.get("task")
                .and_then(|t| t.get("title"))
                .and_then(|t| t.as_str())
                .map(|title| format!("Created task: {}", title))
        }
        "task.updated" => {
            let task_id = payload.get("task_id").and_then(|t| t.as_str()).unwrap_or("unknown");
            let changes = payload.get("changes")
                .map(|c| {
                    let keys: Vec<&str> = c.as_object()
                        .map(|o| o.keys().map(|s| s.as_str()).collect())
                        .unwrap_or_default();
                    keys.join(", ")
                })
                .unwrap_or_default();
            Some(format!("Updated task {}: {}", task_id, changes))
        }
        "task.closed" => {
            let task_id = payload.get("task_id").and_then(|t| t.as_str()).unwrap_or("unknown");
            let reason = payload.get("reason").and_then(|r| r.as_str());
            match reason {
                Some(r) => Some(format!("Closed task {}: {}", task_id, r)),
                None => Some(format!("Closed task {}", task_id)),
            }
        }
        "task.deleted" => {
            let task_id = payload.get("task_id").and_then(|t| t.as_str()).unwrap_or("unknown");
            Some(format!("Deleted task {}", task_id))
        }
        "task.note_added" => {
            let task_id = payload.get("task_id").and_then(|t| t.as_str()).unwrap_or("unknown");
            Some(format!("Note added to task {}", task_id))
        }
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// API handlers
// -----------------------------------------------------------------------------

async fn api_list_tasks(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.forge.list_tasks(None, None).await {
        Ok(tasks) => {
            let tasks: Vec<TaskView> = tasks
                .into_iter()
                .map(|t| {
                    let status = t.status.to_string();
                    TaskView {
                        id: t.id.map(|id| id.id.to_string()).unwrap_or_default(),
                        title: t.title,
                        status: status.clone(),
                        status_class: status.replace("_", "-"),
                        priority: t.priority,
                        priority_class: String::new(),
                        project: t.project,
                        created_at: t.created_at.to_string(),
                    }
                })
                .collect();
            Json(tasks).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list tasks: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to list tasks").into_response()
        }
    }
}

async fn api_list_memos(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.atlas.list_memos(None).await {
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
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to list memos").into_response()
        }
    }
}

async fn api_list_records(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.atlas.list_records(None, false, None).await {
        Ok(records) => {
            let records: Vec<RecordView> = records.into_iter().map(record_to_view).collect();
            Json(records).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list records: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to list records").into_response()
        }
    }
}

async fn api_list_records_by_type(
    State(state): State<Arc<WebState>>,
    Path(record_type): Path<String>,
) -> impl IntoResponse {
    match state.atlas.list_records(Some(&record_type), false, None).await {
        Ok(records) => {
            let records: Vec<RecordView> = records.into_iter().map(record_to_view).collect();
            Json(records).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list records: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to list records").into_response()
        }
    }
}
