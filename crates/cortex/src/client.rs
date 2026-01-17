//! IPC client for Cortex operations
//!
//! Communicates with the memex daemon to manage workers.

use std::path::Path;

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use ipc::Client;

use crate::types::{TranscriptEntry, WorkerId, WorkerStatus};
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

    /// Send a message to a worker and get the response (synchronous)
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

    /// Send a message to a worker asynchronously, returning a message ID
    pub async fn send_message_async(&self, worker_id: &WorkerId, message: &str) -> Result<String> {
        let params = json!({
            "worker_id": worker_id.0,
            "message": message,
        });

        let result = self.client.request("cortex_send_message_async", params).await?;
        let message_id: String = serde_json::from_value(result)?;
        Ok(message_id)
    }

    /// Get the response for an async message (None if still processing)
    pub async fn get_response(&self, message_id: &str) -> Result<Option<WorkerResponse>> {
        let params = json!({
            "message_id": message_id,
        });

        let result = self.client.request("cortex_get_response", params).await?;

        // Check if still pending (null means not ready)
        if result.is_null() {
            return Ok(None);
        }

        let response: WorkerResponseDto = serde_json::from_value(result)?;
        Ok(Some(WorkerResponse {
            result: response.result,
            is_error: response.is_error,
            session_id: response.session_id,
            duration_ms: response.duration_ms,
        }))
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

    /// Get the conversation transcript for a worker
    pub async fn get_transcript(
        &self,
        worker_id: &WorkerId,
        limit: Option<usize>,
    ) -> Result<Vec<TranscriptEntry>> {
        let params = json!({
            "worker_id": worker_id.0,
            "limit": limit,
        });

        let result = self
            .client
            .request("cortex_worker_transcript", params)
            .await?;
        let transcript: Vec<TranscriptEntry> = serde_json::from_value(result)?;
        Ok(transcript)
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
