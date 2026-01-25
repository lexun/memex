//! Memex web server with Leptos SSR + hydration
//!
//! This server provides:
//! - SSR for all pages (tasks, workers, directory, activity)
//! - API endpoints that proxy to the daemon via IPC
//! - Static file serving for the WASM bundle and assets

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::{routing::get, Router};
    use leptos::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use memex_web::app::*;
    use std::sync::Arc;
    use tower_http::services::ServeDir;

    tracing_subscriber::fmt::init();

    // Get socket path from config directory
    let config_dir = dirs::config_dir()
        .expect("No config directory")
        .join("memex");
    let socket_path = config_dir.join("memex.sock");

    // Create IPC client for daemon communication
    let ipc_client = ipc::Client::new(&socket_path);
    let ipc_client = Arc::new(ipc_client);

    // Generate routes from the Leptos app
    let conf = get_configuration(None).await.unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    // API router with IPC client state
    let api_router = Router::new()
        // Dashboard stats
        .route("/api/stats", get(api_stats))
        // Tasks
        .route("/api/tasks", get(api_list_tasks))
        .route("/api/tasks/:id", get(api_get_task))
        // Workers
        .route("/api/workers", get(api_list_workers))
        .route("/api/workers/:id", get(api_get_worker))
        // Records by type and single record
        .route("/api/records/:record_type", get(api_list_records_by_type))
        .route("/api/record/:id", get(api_get_record).put(api_update_record))
        .route("/api/record/:id/content", axum::routing::put(api_update_record_content))
        // Memos
        .route("/api/memos", get(api_list_memos))
        // Events
        .route("/api/events", get(api_list_events))
        .with_state(ipc_client);

    // Build the main router
    let app = Router::new()
        // Merge API routes
        .merge(api_router)
        // Serve static files
        .nest_service("/pkg", ServeDir::new("target/site/pkg"))
        .nest_service("/style", ServeDir::new("style"))
        .nest_service("/assets", ServeDir::new("assets"))
        // Leptos routes
        .leptos_routes(&leptos_options, routes, App)
        .with_state(leptos_options);

    tracing::info!("listening on http://{}", &addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service()).await.unwrap();
}

// =============================================================================
// Types
// =============================================================================

#[cfg(feature = "ssr")]
type IpcClient = std::sync::Arc<ipc::Client>;

/// Extract ID from SurrealDB Thing format
#[cfg(feature = "ssr")]
fn extract_id(id: Option<&serde_json::Value>) -> String {
    match id {
        Some(serde_json::Value::Object(obj)) => obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        Some(serde_json::Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

// =============================================================================
// API Handlers
// =============================================================================

#[cfg(feature = "ssr")]
async fn api_stats(
    axum::extract::State(client): axum::extract::State<IpcClient>,
) -> Result<axum::Json<memex_web::types::DashboardStats>, axum::http::StatusCode> {
    use serde_json::json;

    // Get counts from daemon
    let tasks_result = client.request("list_tasks", json!({})).await;
    let records_result = client
        .request("list_records", json!({ "include_deleted": false }))
        .await;
    let memos_result = client.request("list_memos", json!({})).await;

    let tasks = tasks_result
        .ok()
        .and_then(|v| v.as_array().map(|a| a.len()))
        .unwrap_or(0);
    let records = records_result
        .ok()
        .and_then(|v| v.as_array().map(|a| a.len()))
        .unwrap_or(0);
    let memos = memos_result
        .ok()
        .and_then(|v| v.as_array().map(|a| a.len()))
        .unwrap_or(0);

    Ok(axum::Json(memex_web::types::DashboardStats {
        tasks,
        records,
        memos,
    }))
}

#[cfg(feature = "ssr")]
async fn api_list_tasks(
    axum::extract::State(client): axum::extract::State<IpcClient>,
) -> Result<axum::Json<Vec<memex_web::types::Task>>, axum::http::StatusCode> {
    use serde_json::json;

    let result = client.request("list_tasks", json!({})).await;

    match result {
        Ok(value) => {
            let tasks: Vec<memex_web::types::Task> = value
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| {
                    Some(memex_web::types::Task {
                        id: extract_id(v.get("id")),
                        title: v.get("title")?.as_str()?.to_string(),
                        description: v
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(|s| s.to_string()),
                        status: v.get("status")?.as_str()?.to_string(),
                        priority: v.get("priority")?.as_i64()? as i32,
                        project: v
                            .get("project")
                            .and_then(|p| p.as_str())
                            .map(|s| s.to_string()),
                        created_at: v.get("created_at")?.as_str()?.to_string(),
                        updated_at: v.get("updated_at")?.as_str()?.to_string(),
                    })
                })
                .collect();

            Ok(axum::Json(tasks))
        }
        Err(e) => {
            tracing::error!("Failed to list tasks: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(feature = "ssr")]
async fn api_get_task(
    axum::extract::State(client): axum::extract::State<IpcClient>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::Json<memex_web::types::TaskDetail>, axum::http::StatusCode> {
    use serde_json::json;

    // Get task
    let task_result = client.request("get_task", json!({ "id": id })).await;

    let task_value = match task_result {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to get task {}: {}", id, e);
            return Err(axum::http::StatusCode::NOT_FOUND);
        }
    };

    let task = memex_web::types::Task {
        id: extract_id(task_value.get("id")),
        title: task_value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        description: task_value
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string()),
        status: task_value
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending")
            .to_string(),
        priority: task_value
            .get("priority")
            .and_then(|v| v.as_i64())
            .unwrap_or(2) as i32,
        project: task_value
            .get("project")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string()),
        created_at: task_value
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        updated_at: task_value
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    };

    // Get notes for this task
    let notes_result = client.request("get_notes", json!({ "task_id": id })).await;
    let notes: Vec<memex_web::types::Note> = notes_result
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            Some(memex_web::types::Note {
                id: extract_id(v.get("id")),
                content: v.get("content")?.as_str()?.to_string(),
                created_at: v.get("created_at")?.as_str()?.to_string(),
            })
        })
        .collect();

    // Get workers assigned to this task
    let workers_result = client
        .request("get_workers_by_task", json!({ "task_id": id }))
        .await;
    let assigned_workers: Vec<memex_web::types::Worker> = workers_result
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| parse_worker(v))
        .collect();

    Ok(axum::Json(memex_web::types::TaskDetail {
        task,
        notes,
        assigned_workers,
    }))
}

#[cfg(feature = "ssr")]
fn parse_worker(v: &serde_json::Value) -> Option<memex_web::types::Worker> {
    Some(memex_web::types::Worker {
        id: v.get("worker_id")?.as_str()?.to_string(),
        state: v.get("state")?.as_str()?.to_string(),
        current_task: v
            .get("current_task")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string()),
        worktree: v
            .get("worktree")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string()),
        cwd: v.get("cwd")?.as_str()?.to_string(),
        model: v
            .get("model")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string()),
        messages_sent: v.get("messages_sent")?.as_u64()?,
        messages_received: v.get("messages_received")?.as_u64()?,
        started_at: v.get("started_at")?.as_str()?.to_string(),
        last_activity: v.get("last_activity")?.as_str()?.to_string(),
        error_message: v
            .get("error_message")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string()),
    })
}

#[cfg(feature = "ssr")]
async fn api_list_workers(
    axum::extract::State(client): axum::extract::State<IpcClient>,
) -> Result<axum::Json<Vec<memex_web::types::Worker>>, axum::http::StatusCode> {
    use serde_json::json;

    let result = client.request("cortex_list_workers", json!({})).await;

    match result {
        Ok(value) => {
            let workers: Vec<memex_web::types::Worker> = value
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| parse_worker(v))
                .collect();

            Ok(axum::Json(workers))
        }
        Err(e) => {
            tracing::error!("Failed to list workers: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(feature = "ssr")]
async fn api_get_worker(
    axum::extract::State(client): axum::extract::State<IpcClient>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::Json<memex_web::types::Worker>, axum::http::StatusCode> {
    use serde_json::json;

    let result = client.request("get_worker", json!({ "worker_id": id })).await;

    match result {
        Ok(value) => match parse_worker(&value) {
            Some(worker) => Ok(axum::Json(worker)),
            None => {
                tracing::error!("Failed to parse worker {}", id);
                Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        Err(e) => {
            tracing::error!("Failed to get worker {}: {}", id, e);
            Err(axum::http::StatusCode::NOT_FOUND)
        }
    }
}

#[cfg(feature = "ssr")]
async fn api_list_records_by_type(
    axum::extract::State(client): axum::extract::State<IpcClient>,
    axum::extract::Path(record_type): axum::extract::Path<String>,
) -> Result<axum::Json<Vec<memex_web::types::Record>>, axum::http::StatusCode> {
    use serde_json::json;

    let result = client
        .request(
            "list_records",
            json!({
                "record_type": record_type,
                "include_deleted": false
            }),
        )
        .await;

    match result {
        Ok(value) => {
            let records: Vec<memex_web::types::Record> = value
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| parse_record(v))
                .collect();

            Ok(axum::Json(records))
        }
        Err(e) => {
            tracing::error!("Failed to list records: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(feature = "ssr")]
fn parse_record(v: &serde_json::Value) -> Option<memex_web::types::Record> {
    Some(memex_web::types::Record {
        id: extract_id(v.get("id")),
        record_type: v.get("record_type")?.as_str()?.to_string(),
        name: v.get("name")?.as_str()?.to_string(),
        description: v
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string()),
        content: v.get("content").cloned().unwrap_or(serde_json::Value::Null),
        created_at: v.get("created_at")?.as_str()?.to_string(),
        updated_at: v.get("updated_at")?.as_str()?.to_string(),
    })
}

#[cfg(feature = "ssr")]
async fn api_get_record(
    axum::extract::State(client): axum::extract::State<IpcClient>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::Json<memex_web::types::RecordDetail>, axum::http::StatusCode> {
    use serde_json::json;

    // Get the record
    let record_result = client.request("get_record", json!({ "id": id })).await;

    let record_value = match record_result {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to get record {}: {}", id, e);
            return Err(axum::http::StatusCode::NOT_FOUND);
        }
    };

    let record = match parse_record(&record_value) {
        Some(r) => r,
        None => {
            tracing::error!("Failed to parse record {}", id);
            return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get related records via edges
    let edges_result = client
        .request("list_edges", json!({ "id": id, "direction": "both" }))
        .await;

    let mut related: Vec<memex_web::types::Record> = Vec::new();

    if let Ok(edges_value) = edges_result {
        if let Some(edges) = edges_value.as_array() {
            for edge in edges {
                // Get the other record ID (either source or target depending on direction)
                let other_id = if edge.get("source").and_then(|s| s.as_str()) == Some(&id) {
                    edge.get("target").and_then(|t| t.as_str())
                } else {
                    edge.get("source").and_then(|s| s.as_str())
                };

                if let Some(other_id) = other_id {
                    if let Ok(other_value) = client
                        .request("get_record", json!({ "id": other_id }))
                        .await
                    {
                        if let Some(other_record) = parse_record(&other_value) {
                            related.push(other_record);
                        }
                    }
                }
            }
        }
    }

    Ok(axum::Json(memex_web::types::RecordDetail { record, related }))
}

/// Request body for updating a record
#[cfg(feature = "ssr")]
#[derive(serde::Deserialize)]
struct UpdateRecordRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content: Option<serde_json::Value>,
}

/// Update a record's name, description, or content
#[cfg(feature = "ssr")]
async fn api_update_record(
    axum::extract::State(client): axum::extract::State<IpcClient>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<UpdateRecordRequest>,
) -> Result<axum::Json<memex_web::types::Record>, axum::http::StatusCode> {
    use serde_json::json;

    let mut params = json!({ "id": id });
    if let Some(name) = body.name {
        params["name"] = json!(name);
    }
    if let Some(description) = body.description {
        params["description"] = json!(description);
    }
    if let Some(content) = body.content {
        params["content"] = content;
    }

    let result = client.request("update_record", params).await;

    match result {
        Ok(value) => match parse_record(&value) {
            Some(record) => Ok(axum::Json(record)),
            None => {
                tracing::error!("Failed to parse updated record {}", id);
                Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        Err(e) => {
            tracing::error!("Failed to update record {}: {}", id, e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Request body for updating record content (markdown)
#[cfg(feature = "ssr")]
#[derive(serde::Deserialize)]
struct UpdateContentRequest {
    content: String,
}

/// Update only the content field of a record (for markdown editor)
#[cfg(feature = "ssr")]
async fn api_update_record_content(
    axum::extract::State(client): axum::extract::State<IpcClient>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<UpdateContentRequest>,
) -> Result<axum::Json<memex_web::types::Record>, axum::http::StatusCode> {
    use serde_json::json;

    // Store the markdown as a JSON object with a "text" field for compatibility
    let content_value = json!({ "text": body.content });

    let result = client
        .request(
            "update_record",
            json!({
                "id": id,
                "content": content_value
            }),
        )
        .await;

    match result {
        Ok(value) => match parse_record(&value) {
            Some(record) => Ok(axum::Json(record)),
            None => {
                tracing::error!("Failed to parse updated record {}", id);
                Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        Err(e) => {
            tracing::error!("Failed to update record content {}: {}", id, e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(feature = "ssr")]
async fn api_list_memos(
    axum::extract::State(client): axum::extract::State<IpcClient>,
) -> Result<axum::Json<Vec<memex_web::types::MemoView>>, axum::http::StatusCode> {
    use serde_json::json;

    let result = client.request("list_memos", json!({ "limit": 50 })).await;

    match result {
        Ok(value) => {
            let memos: Vec<memex_web::types::MemoView> = value
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| {
                    Some(memex_web::types::MemoView {
                        id: extract_id(v.get("id")),
                        content: v.get("content")?.as_str()?.to_string(),
                        source: v
                            .get("source")
                            .and_then(|s| s.get("actor"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        created_at: v.get("created_at")?.as_str()?.to_string(),
                    })
                })
                .collect();

            Ok(axum::Json(memos))
        }
        Err(e) => {
            tracing::error!("Failed to list memos: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(feature = "ssr")]
async fn api_list_events(
    axum::extract::State(client): axum::extract::State<IpcClient>,
) -> Result<axum::Json<Vec<memex_web::types::Event>>, axum::http::StatusCode> {
    use serde_json::json;

    let result = client
        .request("list_events", json!({ "event_type": "task.", "limit": 50 }))
        .await;

    match result {
        Ok(value) => {
            let events: Vec<memex_web::types::Event> = value
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| {
                    let event_type = v.get("event_type")?.as_str()?.to_string();
                    let payload = v.get("payload");

                    // Extract summary from payload
                    let summary = extract_event_summary(&event_type, payload);

                    Some(memex_web::types::Event {
                        id: extract_id(v.get("id")),
                        event_type,
                        source: v
                            .get("source")
                            .and_then(|s| s.get("actor"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        timestamp: v.get("timestamp")?.as_str()?.to_string(),
                        summary,
                    })
                })
                .collect();

            Ok(axum::Json(events))
        }
        Err(e) => {
            tracing::error!("Failed to list events: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(feature = "ssr")]
fn extract_event_summary(
    event_type: &str,
    payload: Option<&serde_json::Value>,
) -> Option<String> {
    let payload = payload?;
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
                .and_then(|c| c.as_object())
                .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
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

#[cfg(not(feature = "ssr"))]
fn main() {
    // This is required for wasm-pack
}
