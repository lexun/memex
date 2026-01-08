//! Worker process management
//!
//! Handles spawning, communicating with, and monitoring Claude worker processes.
//!
//! ## Approach
//!
//! For MVP, we use Claude's `-p` (print) mode with JSON output. Each message
//! spawns a new Claude process. This is simpler than stream-json mode and
//! works reliably.
//!
//! Future improvements could use:
//! - `--resume` flag for session continuity
//! - `--input-format stream-json` for true bidirectional communication

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use chrono::Utc;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::error::{CortexError, Result};
use crate::types::{WorkerConfig, WorkerId, WorkerState, WorkerStatus};

/// Result of sending a message to a worker
#[derive(Debug, Clone)]
pub struct WorkerResponse {
    /// The text response from the worker
    pub result: String,
    /// Whether the request succeeded
    pub is_error: bool,
    /// Session ID (for potential resume)
    pub session_id: Option<String>,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Internal state for a single worker
struct Worker {
    #[allow(dead_code)] // Used for debugging/logging
    id: WorkerId,
    config: WorkerConfig,
    status: WorkerStatus,
    /// Session ID from last interaction (for --resume)
    last_session_id: Option<String>,
}

/// Manages multiple Claude worker processes
pub struct WorkerManager {
    workers: Arc<RwLock<HashMap<WorkerId, Worker>>>,
}

impl WorkerManager {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new worker (doesn't spawn a process yet)
    pub async fn create(&self, config: WorkerConfig) -> Result<WorkerId> {
        let id = WorkerId::new();
        info!("Creating worker {} for directory {}", id, config.cwd);

        let mut status = WorkerStatus::new(id.clone());
        status.worktree = Some(config.cwd.clone());
        status.state = WorkerState::Ready;

        let worker = Worker {
            id: id.clone(),
            config,
            status,
            last_session_id: None,
        };

        let mut workers = self.workers.write().await;
        workers.insert(id.clone(), worker);

        info!("Worker {} created successfully", id);
        Ok(id)
    }

    /// Send a message to a worker and get the response
    ///
    /// This spawns a Claude process with `-p` mode, sends the message,
    /// and returns the response.
    pub async fn send_message(&self, id: &WorkerId, message: &str) -> Result<WorkerResponse> {
        let mut workers = self.workers.write().await;
        let worker = workers.get_mut(id).ok_or_else(|| {
            CortexError::WorkerNotFound(id.to_string())
        })?;

        worker.status.state = WorkerState::Working;
        worker.status.last_activity = Utc::now();

        // Build command
        let mut cmd = Command::new("claude");
        cmd.arg("-p").arg(message);
        cmd.arg("--output-format").arg("json");
        cmd.current_dir(&worker.config.cwd);

        // Add model if specified
        if let Some(ref model) = worker.config.model {
            cmd.arg("--model").arg(model);
        }

        // Skip permission prompts for automated use
        cmd.arg("--dangerously-skip-permissions");

        // Add resume if we have a session
        if let Some(ref session_id) = worker.last_session_id {
            cmd.arg("--resume").arg(session_id);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!("Running claude for worker {}", id);

        let output = cmd.output().await.map_err(|e| {
            CortexError::WorkerStartFailed(format!("Failed to spawn claude: {}", e))
        })?;

        worker.status.messages_sent += 1;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            worker.status.state = WorkerState::Error(stderr.to_string());
            return Err(CortexError::WorkerCommunicationFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse JSON response
        let json: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| {
            CortexError::WorkerCommunicationFailed(format!("Invalid JSON response: {}", e))
        })?;

        let result = json.get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let is_error = json.get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let session_id = json.get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let duration_ms = json.get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // Store session ID for potential resume
        if let Some(ref sid) = session_id {
            worker.last_session_id = Some(sid.clone());
        }

        worker.status.messages_received += 1;
        worker.status.last_activity = Utc::now();
        worker.status.state = WorkerState::Idle;

        Ok(WorkerResponse {
            result,
            is_error,
            session_id,
            duration_ms,
        })
    }

    /// Get status of a worker
    pub async fn status(&self, id: &WorkerId) -> Result<WorkerStatus> {
        let workers = self.workers.read().await;
        let worker = workers.get(id).ok_or_else(|| {
            CortexError::WorkerNotFound(id.to_string())
        })?;
        Ok(worker.status.clone())
    }

    /// List all workers and their statuses
    pub async fn list(&self) -> Vec<WorkerStatus> {
        let workers = self.workers.read().await;
        workers.values().map(|w| w.status.clone()).collect()
    }

    /// Remove a worker
    pub async fn remove(&self, id: &WorkerId) -> Result<()> {
        let mut workers = self.workers.write().await;
        if workers.remove(id).is_none() {
            return Err(CortexError::WorkerNotFound(id.to_string()));
        }
        info!("Worker {} removed", id);
        Ok(())
    }

    /// Remove all workers
    pub async fn remove_all(&self) {
        let mut workers = self.workers.write().await;
        let count = workers.len();
        workers.clear();
        info!("Removed {} workers", count);
    }
}

impl Default for WorkerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_creation() {
        let manager = WorkerManager::new();
        let config = WorkerConfig::new("/tmp");
        let id = manager.create(config).await.unwrap();

        let status = manager.status(&id).await.unwrap();
        assert_eq!(status.state, WorkerState::Ready);
    }

    #[tokio::test]
    async fn test_worker_list() {
        let manager = WorkerManager::new();

        let config1 = WorkerConfig::new("/tmp/a");
        let config2 = WorkerConfig::new("/tmp/b");

        manager.create(config1).await.unwrap();
        manager.create(config2).await.unwrap();

        let list = manager.list().await;
        assert_eq!(list.len(), 2);
    }
}
