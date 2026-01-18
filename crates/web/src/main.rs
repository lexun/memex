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
        .route("/api/records", get(api_list_records))
        .route("/api/records/:id", get(api_get_record))
        .with_state(ipc_client);

    // Build the main router
    let app = Router::new()
        // Merge API routes
        .merge(api_router)
        // Serve static files
        .nest_service("/pkg", ServeDir::new("target/site/pkg"))
        .nest_service("/assets", ServeDir::new("assets"))
        // Leptos routes
        .leptos_routes(&leptos_options, routes, App)
        .with_state(leptos_options);

    tracing::info!("listening on http://{}", &addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service()).await.unwrap();
}

/// Record as returned by the daemon (matches atlas::Record serialization)
#[cfg(feature = "ssr")]
#[derive(serde::Deserialize)]
struct DaemonRecord {
    id: Option<serde_json::Value>,
    record_type: String,
    title: String,
    description: Option<String>,
    content: serde_json::Value,
    created_at: String,
    updated_at: String,
}

#[cfg(feature = "ssr")]
impl From<DaemonRecord> for memex_web::app::Record {
    fn from(r: DaemonRecord) -> Self {
        // Extract ID from SurrealDB Thing format
        let id = match r.id {
            Some(serde_json::Value::Object(obj)) => {
                obj.get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            }
            Some(serde_json::Value::String(s)) => s,
            _ => String::new(),
        };

        memex_web::app::Record {
            id,
            record_type: r.record_type,
            title: r.title,
            description: r.description,
            content: r.content,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[cfg(feature = "ssr")]
type IpcClient = std::sync::Arc<ipc::Client>;

#[cfg(feature = "ssr")]
async fn api_list_records(
    axum::extract::State(client): axum::extract::State<IpcClient>,
) -> Result<axum::Json<Vec<memex_web::app::Record>>, axum::http::StatusCode> {
    use serde_json::json;

    // Request records from daemon
    let result = client
        .request("list_records", json!({ "include_deleted": false }))
        .await;

    match result {
        Ok(value) => {
            // Parse the response into daemon Record type
            let daemon_records: Vec<DaemonRecord> = serde_json::from_value(value)
                .map_err(|e| {
                    tracing::error!("Failed to parse records: {}", e);
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR
                })?;

            // Convert to web Record type
            let records: Vec<memex_web::app::Record> = daemon_records
                .into_iter()
                .map(Into::into)
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
async fn api_get_record(
    axum::extract::State(client): axum::extract::State<IpcClient>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::Json<memex_web::app::Record>, axum::http::StatusCode> {
    use serde_json::json;

    // Request record from daemon
    let result = client
        .request("get_record", json!({ "id": id }))
        .await;

    match result {
        Ok(value) => {
            // Parse the response
            let daemon_record: DaemonRecord = serde_json::from_value(value)
                .map_err(|e| {
                    tracing::error!("Failed to parse record: {}", e);
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR
                })?;

            Ok(axum::Json(daemon_record.into()))
        }
        Err(e) => {
            tracing::error!("Failed to get record {}: {}", id, e);
            Err(axum::http::StatusCode::NOT_FOUND)
        }
    }
}

#[cfg(not(feature = "ssr"))]
fn main() {
    // This is required for wasm-pack
}
