//! IPC client for Cortex operations
//!
//! Communicates with the memex daemon to manage workers.

use std::path::Path;

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use ipc::Client;

use crate::types::{WorkerId, WorkerStatus};
use crate::worker::WorkerResponse;

/// Client for Cortex worker operations via daemon IPC
pub struct CortexClient {
    client: Client,
}

impl CortexClient {
    /// Create a new client connected to the daemon socket
    pub fn new(socket_path: &Path) -> Self {
        Self {
            client: Client::new(socket_path),
        }
    }

    /// Check if daemon is reachable
    pub async fn health_check(&self) -> Result<bool> {
        self.client.health_check().await
    }

    /// Create a new worker
    pub async fn create_worker(
        &self,
        cwd: &str,
        model: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Result<WorkerId> {
        let params = json!({
            "cwd": cwd,
            "model": model,
            "system_prompt": system_prompt,
        });

        let result = self.client.request("cortex_create_worker", params).await?;
        let id: String = serde_json::from_value(result)?;
        Ok(WorkerId::from_string(id))
    }

    /// Send a message to a worker and get the response
    pub async fn send_message(&self, worker_id: &WorkerId, message: &str) -> Result<WorkerResponse> {
        let params = json!({
            "worker_id": worker_id.0,
            "message": message,
        });

        let result = self.client.request("cortex_send_message", params).await?;
        let response: WorkerResponseDto = serde_json::from_value(result)?;
        Ok(WorkerResponse {
            result: response.result,
            is_error: response.is_error,
            session_id: response.session_id,
            duration_ms: response.duration_ms,
        })
    }

    /// Get status of a worker
    pub async fn get_worker_status(&self, worker_id: &WorkerId) -> Result<WorkerStatus> {
        let params = json!({
            "worker_id": worker_id.0,
        });

        let result = self.client.request("cortex_worker_status", params).await?;
        let status: WorkerStatus = serde_json::from_value(result)?;
        Ok(status)
    }

    /// List all workers
    pub async fn list_workers(&self) -> Result<Vec<WorkerStatus>> {
        let result = self.client.request("cortex_list_workers", json!({})).await?;
        let workers: Vec<WorkerStatus> = serde_json::from_value(result)?;
        Ok(workers)
    }

    /// Remove a worker
    pub async fn remove_worker(&self, worker_id: &WorkerId) -> Result<()> {
        let params = json!({
            "worker_id": worker_id.0,
        });

        self.client.request("cortex_remove_worker", params).await?;
        Ok(())
    }
}

/// DTO for worker response (matches what daemon returns)
#[derive(Debug, Deserialize)]
struct WorkerResponseDto {
    result: String,
    is_error: bool,
    session_id: Option<String>,
    duration_ms: u64,
}
