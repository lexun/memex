//! IPC client for Cortex operations
//!
//! Communicates with the memex daemon to manage workers.

use std::path::Path;

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use ipc::Client;

use crate::types::{TranscriptEntry, WorkerId, WorkerStatus};
use crate::worker::{ShellValidation, WorkerResponse};

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
    ///
    /// # Arguments
    /// * `cwd` - Working directory for the worker
    /// * `model` - Model to use (e.g., "haiku", "sonnet", "opus")
    /// * `system_prompt` - Additional system prompt context
    /// * `mcp_strict` - If true (default), worker won't inherit user's MCP servers
    /// * `mcp_servers` - List of MCP server JSON configs to include
    pub async fn create_worker(
        &self,
        cwd: &str,
        model: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Result<WorkerId> {
        self.create_worker_with_mcp(cwd, model, system_prompt, None, None).await
    }

    /// Create a new worker with MCP configuration
    pub async fn create_worker_with_mcp(
        &self,
        cwd: &str,
        model: Option<&str>,
        system_prompt: Option<&str>,
        mcp_strict: Option<bool>,
        mcp_servers: Option<Vec<String>>,
    ) -> Result<WorkerId> {
        let params = json!({
            "cwd": cwd,
            "model": model,
            "system_prompt": system_prompt,
            "mcp_strict": mcp_strict,
            "mcp_servers": mcp_servers,
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

    /// Validate the shell environment for a worker
    ///
    /// Checks if the direnv/nix environment loads correctly for the worker's
    /// directory. Use this before operations that depend on the shell environment
    /// (like reload) to ensure they will succeed.
    ///
    /// # Returns
    ///
    /// - `ShellValidation::Success` if the environment loads correctly
    /// - `ShellValidation::Failed` if there's an error (broken flake.nix, etc.)
    pub async fn validate_shell(&self, worker_id: &WorkerId) -> Result<ShellValidation> {
        let params = json!({
            "worker_id": worker_id.0,
        });

        let result = self.client.request("cortex_validate_shell", params).await?;
        let validation: ShellValidation = serde_json::from_value(result)?;
        Ok(validation)
    }

    /// Validate the shell environment for a directory
    ///
    /// Checks if the direnv/nix environment loads correctly for the specified
    /// directory. This can be used to validate environments before creating
    /// workers or for arbitrary paths.
    ///
    /// # Returns
    ///
    /// - `ShellValidation::Success` if the environment loads correctly
    /// - `ShellValidation::Failed` if there's an error (broken flake.nix, etc.)
    pub async fn validate_shell_for_path(&self, path: &str) -> Result<ShellValidation> {
        let params = json!({
            "path": path,
        });

        let result = self.client.request("cortex_validate_shell", params).await?;
        let validation: ShellValidation = serde_json::from_value(result)?;
        Ok(validation)
    }

    /// Get or create the Coordinator worker
    ///
    /// The Coordinator is a long-running headless worker with a special system prompt
    /// for managing worker orchestration. There is only one Coordinator.
    pub async fn get_coordinator(&self) -> Result<CoordinatorResult> {
        let result = self.client.request("cortex_get_coordinator", json!({})).await?;
        let coordinator: CoordinatorResult = serde_json::from_value(result)?;
        Ok(coordinator)
    }

    /// Dispatch a task to a worker with automatic context assembly
    ///
    /// This is the key orchestration abstraction - one command to go from
    /// task ID to a worker executing with full context.
    pub async fn dispatch_task(
        &self,
        task_id: &str,
        worktree: Option<&str>,
        model: Option<&str>,
        repo_path: Option<&str>,
    ) -> Result<DispatchTaskResult> {
        let params = json!({
            "task_id": task_id,
            "worktree": worktree,
            "model": model,
            "repo_path": repo_path,
        });

        let result = self.client.request("cortex_dispatch_task", params).await?;
        let dispatch_result: DispatchTaskResult = serde_json::from_value(result)?;
        Ok(dispatch_result)
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

/// Result of getting or creating the Coordinator
#[derive(Debug, Deserialize)]
pub struct CoordinatorResult {
    /// Worker ID (always "coordinator")
    pub worker_id: String,
    /// State of the coordinator
    pub state: String,
    /// Whether a new coordinator was created
    pub created: bool,
}

/// Result of dispatching a task to a worker
#[derive(Debug, Deserialize)]
pub struct DispatchTaskResult {
    /// Worker ID that was created
    pub worker_id: String,
    /// Worktree path where the worker is operating
    pub worktree: String,
    /// Task ID that was dispatched
    pub task_id: String,
    /// Record ID that context was assembled from (if any)
    pub context_from: Option<String>,
    /// Whether the worker is ready to receive messages
    pub ready: bool,
}
