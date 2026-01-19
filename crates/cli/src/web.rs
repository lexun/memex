//! Web server integration for the daemon
//!
//! Provides a local web UI for browsing records, tasks, and memos.

use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse},
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
        // Pages
        .route("/", get(index_page))
        .route("/records", get(records_page))
        .route("/records/:id", get(record_detail_page))
        .route("/tasks", get(tasks_page))
        .route("/memos", get(memos_page))
        // API endpoints
        .route("/api/records", get(api_list_records))
        .route("/api/records/:id", get(api_get_record))
        .route("/api/tasks", get(api_list_tasks))
        .route("/api/memos", get(api_list_memos))
        // Static assets (CSS)
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}

// -----------------------------------------------------------------------------
// Templates using Askama
// -----------------------------------------------------------------------------

#[derive(askama::Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    title: &'static str,
    record_count: usize,
    task_count: usize,
    memo_count: usize,
}

#[derive(askama::Template)]
#[template(path = "records.html")]
struct RecordsTemplate {
    title: &'static str,
    records: Vec<RecordView>,
}

#[derive(askama::Template)]
#[template(path = "record_detail.html")]
struct RecordDetailTemplate {
    title: String,
    record: RecordView,
}

#[derive(askama::Template)]
#[template(path = "tasks.html")]
struct TasksTemplate {
    title: &'static str,
    tasks: Vec<TaskView>,
}

#[derive(askama::Template)]
#[template(path = "memos.html")]
struct MemosTemplate {
    title: &'static str,
    memos: Vec<MemoView>,
}

// -----------------------------------------------------------------------------
// View models (simplified for templates)
// -----------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
struct RecordView {
    id: String,
    record_type: String,
    title: String,
    description: Option<String>,
    created_at: String,
}

#[derive(Clone, serde::Serialize)]
struct TaskView {
    id: String,
    title: String,
    status: String,
    priority: i32,
    project: Option<String>,
    created_at: String,
}

#[derive(Clone, serde::Serialize)]
struct MemoView {
    id: String,
    content: String,
    source: String,
    created_at: String,
}

// -----------------------------------------------------------------------------
// Page handlers
// -----------------------------------------------------------------------------

async fn index_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let records = state.atlas.list_records(None, false, None).await.unwrap_or_default();
    let tasks = state.forge.list_tasks(None, None).await.unwrap_or_default();
    let memos = state.atlas.list_memos(None).await.unwrap_or_default();

    let template = IndexTemplate {
        title: "Dashboard",
        record_count: records.len(),
        task_count: tasks.len(),
        memo_count: memos.len(),
    };

    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn records_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let records = state.atlas.list_records(None, false, None).await.unwrap_or_default();

    let records: Vec<RecordView> = records
        .into_iter()
        .map(|r| RecordView {
            id: r.id.map(|t| t.id.to_string()).unwrap_or_default(),
            record_type: format!("{:?}", r.record_type),
            title: r.name,
            description: r.description,
            created_at: r.created_at.to_string(),
        })
        .collect();

    let template = RecordsTemplate { title: "Records", records };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn record_detail_page(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.atlas.get_record(&id).await {
        Ok(Some(r)) => {
            let title = r.name.clone();
            let record = RecordView {
                id: r.id.map(|t| t.id.to_string()).unwrap_or_default(),
                record_type: format!("{:?}", r.record_type),
                title: r.name,
                description: r.description,
                created_at: r.created_at.to_string(),
            };
            let template = RecordDetailTemplate { title, record };
            Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
        }
        _ => Html("Record not found".to_string()),
    }
}

async fn tasks_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let tasks = state.forge.list_tasks(None, None).await.unwrap_or_default();

    let tasks: Vec<TaskView> = tasks
        .into_iter()
        .map(|t| TaskView {
            id: t.id.map(|id| id.id.to_string()).unwrap_or_default(),
            title: t.title,
            status: t.status.to_string(),
            priority: t.priority,
            project: t.project,
            created_at: t.created_at.to_string(),
        })
        .collect();

    let template = TasksTemplate { title: "Tasks", tasks };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

async fn memos_page(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let memos = state.atlas.list_memos(None).await.unwrap_or_default();

    let memos: Vec<MemoView> = memos
        .into_iter()
        .map(|m| MemoView {
            id: m.id.map(|t| t.id.to_string()).unwrap_or_default(),
            content: if m.content.len() > 200 {
                format!("{}...", &m.content[..200])
            } else {
                m.content
            },
            source: m.source.actor.clone(),
            created_at: m.created_at.to_string(),
        })
        .collect();

    let template = MemosTemplate { title: "Memos", memos };
    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}

// -----------------------------------------------------------------------------
// API handlers (JSON)
// -----------------------------------------------------------------------------

async fn api_list_records(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.atlas.list_records(None, false, None).await {
        Ok(records) => {
            let records: Vec<RecordView> = records
                .into_iter()
                .map(|r| RecordView {
                    id: r.id.map(|t| t.id.to_string()).unwrap_or_default(),
                    record_type: format!("{:?}", r.record_type),
                    title: r.name,
                    description: r.description,
                    created_at: r.created_at.to_string(),
                })
                .collect();
            Json(records).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list records: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to list records").into_response()
        }
    }
}

async fn api_get_record(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.atlas.get_record(&id).await {
        Ok(Some(r)) => {
            let record = RecordView {
                id: r.id.map(|t| t.id.to_string()).unwrap_or_default(),
                record_type: format!("{:?}", r.record_type),
                title: r.name,
                description: r.description,
                created_at: r.created_at.to_string(),
            };
            Json(record).into_response()
        }
        Ok(None) => {
            (axum::http::StatusCode::NOT_FOUND, "Record not found").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get record: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to get record").into_response()
        }
    }
}

async fn api_list_tasks(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    match state.forge.list_tasks(None, None).await {
        Ok(tasks) => {
            let tasks: Vec<TaskView> = tasks
                .into_iter()
                .map(|t| TaskView {
                    id: t.id.map(|id| id.id.to_string()).unwrap_or_default(),
                    title: t.title,
                    status: t.status.to_string(),
                    priority: t.priority,
                    project: t.project,
                    created_at: t.created_at.to_string(),
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
