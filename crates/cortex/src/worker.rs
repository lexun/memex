//! Worker process management
//!
//! Handles spawning, communicating with, and monitoring Claude worker processes.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::error::{CortexError, Result};
use crate::types::{WorkerConfig, WorkerId, WorkerMessage, WorkerResponse, WorkerState, WorkerStatus};

/// Internal state for a single worker
struct Worker {
    id: WorkerId,
    config: WorkerConfig,
    process: Child,
    status: WorkerStatus,
    /// Channel to send messages to the worker's stdin handler
    tx: mpsc::Sender<String>,
    /// Channel to receive responses from the worker
    rx: mpsc::Receiver<WorkerResponse>,
}

/// Manages multiple Claude worker processes
pub struct WorkerManager {
    workers: Arc<RwLock<HashMap<WorkerId, Arc<Mutex<Worker>>>>>,
}

impl WorkerManager {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawn a new Claude worker process
    pub async fn spawn(&self, config: WorkerConfig) -> Result<WorkerId> {
        let id = WorkerId::new();
        info!("Spawning worker {} in {}", id, config.cwd);

        // Build the claude command
        let mut cmd = Command::new("claude");
        cmd.arg("--input-format").arg("stream-json");
        cmd.arg("--output-format").arg("stream-json");
        cmd.current_dir(&config.cwd);

        if let Some(ref model) = config.model {
            cmd.arg("--model").arg(model);
        }

        // Set up stdio for communication
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut process = cmd.spawn().map_err(|e| {
            CortexError::WorkerStartFailed(format!("Failed to spawn claude: {}", e))
        })?;

        // Take ownership of stdio
        let stdin = process.stdin.take().ok_or_else(|| {
            CortexError::WorkerStartFailed("Failed to get stdin".to_string())
        })?;
        let stdout = process.stdout.take().ok_or_else(|| {
            CortexError::WorkerStartFailed("Failed to get stdout".to_string())
        })?;

        // Create channels for communication
        let (tx, mut rx_internal) = mpsc::channel::<String>(32);
        let (tx_response, rx_response) = mpsc::channel::<WorkerResponse>(32);

        // Spawn stdin writer task
        let worker_id_clone = id.clone();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(msg) = rx_internal.recv().await {
                debug!("Worker {} <- {}", worker_id_clone, msg);
                if let Err(e) = stdin.write_all(msg.as_bytes()).await {
                    error!("Failed to write to worker {}: {}", worker_id_clone, e);
                    break;
                }
                if let Err(e) = stdin.write_all(b"\n").await {
                    error!("Failed to write newline to worker {}: {}", worker_id_clone, e);
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    error!("Failed to flush worker {}: {}", worker_id_clone, e);
                    break;
                }
            }
        });

        // Spawn stdout reader task
        let worker_id_clone = id.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                debug!("Worker {} -> {}", worker_id_clone, line);
                if let Ok(response) = serde_json::from_str::<WorkerResponse>(&line) {
                    if tx_response.send(response).await.is_err() {
                        warn!("Response channel closed for worker {}", worker_id_clone);
                        break;
                    }
                } else {
                    debug!("Non-JSON output from worker {}: {}", worker_id_clone, line);
                }
            }
        });

        let mut status = WorkerStatus::new(id.clone());
        status.worktree = Some(config.cwd.clone());
        status.state = WorkerState::Ready;

        let worker = Worker {
            id: id.clone(),
            config,
            process,
            status,
            tx,
            rx: rx_response,
        };

        // Store the worker
        {
            let mut workers = self.workers.write().await;
            workers.insert(id.clone(), Arc::new(Mutex::new(worker)));
        }

        info!("Worker {} spawned successfully", id);
        Ok(id)
    }

    /// Send a message to a worker
    pub async fn send_message(&self, id: &WorkerId, message: WorkerMessage) -> Result<()> {
        let workers = self.workers.read().await;
        let worker = workers.get(id).ok_or_else(|| {
            CortexError::WorkerNotFound(id.to_string())
        })?;

        let mut worker = worker.lock().await;
        let json = serde_json::to_string(&message)?;

        worker.tx.send(json).await.map_err(|e| {
            CortexError::WorkerCommunicationFailed(format!("Send failed: {}", e))
        })?;

        worker.status.messages_sent += 1;
        worker.status.last_activity = chrono::Utc::now();
        worker.status.state = WorkerState::Working;

        Ok(())
    }

    /// Get the next response from a worker (with timeout)
    pub async fn get_response(
        &self,
        id: &WorkerId,
        timeout_ms: Option<u64>,
    ) -> Result<Option<WorkerResponse>> {
        let workers = self.workers.read().await;
        let worker = workers.get(id).ok_or_else(|| {
            CortexError::WorkerNotFound(id.to_string())
        })?;

        let mut worker = worker.lock().await;
        let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(30000));

        match tokio::time::timeout(timeout, worker.rx.recv()).await {
            Ok(Some(response)) => {
                worker.status.messages_received += 1;
                worker.status.last_activity = chrono::Utc::now();

                // Update state based on response type
                if response.msg_type == "assistant" && response.content.is_none() {
                    // End of response
                    worker.status.state = WorkerState::Idle;
                }

                Ok(Some(response))
            }
            Ok(None) => {
                // Channel closed
                worker.status.state = WorkerState::Stopped;
                Ok(None)
            }
            Err(_) => Err(CortexError::WorkerTimeout),
        }
    }

    /// Get status of a worker
    pub async fn status(&self, id: &WorkerId) -> Result<WorkerStatus> {
        let workers = self.workers.read().await;
        let worker = workers.get(id).ok_or_else(|| {
            CortexError::WorkerNotFound(id.to_string())
        })?;

        let worker = worker.lock().await;
        Ok(worker.status.clone())
    }

    /// List all workers and their statuses
    pub async fn list(&self) -> Vec<WorkerStatus> {
        let workers = self.workers.read().await;
        let mut statuses = Vec::new();

        for worker in workers.values() {
            let worker = worker.lock().await;
            statuses.push(worker.status.clone());
        }

        statuses
    }

    /// Kill a worker process
    pub async fn kill(&self, id: &WorkerId) -> Result<()> {
        let mut workers = self.workers.write().await;
        let worker = workers.remove(id).ok_or_else(|| {
            CortexError::WorkerNotFound(id.to_string())
        })?;

        let mut worker = worker.lock().await;
        info!("Killing worker {}", id);

        if let Err(e) = worker.process.kill().await {
            warn!("Failed to kill worker {}: {}", id, e);
        }

        Ok(())
    }

    /// Kill all workers
    pub async fn kill_all(&self) {
        let mut workers = self.workers.write().await;
        for (id, worker) in workers.drain() {
            let mut worker = worker.lock().await;
            info!("Killing worker {}", id);
            let _ = worker.process.kill().await;
        }
    }
}

impl Default for WorkerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WorkerManager {
    fn drop(&mut self) {
        // Note: async cleanup not possible in Drop
        // Workers will be orphaned - caller should use kill_all() before dropping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_id_generation() {
        let id1 = WorkerId::new();
        let id2 = WorkerId::new();
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_worker_status_new() {
        let id = WorkerId::new();
        let status = WorkerStatus::new(id.clone());
        assert_eq!(status.id, id);
        assert_eq!(status.state, WorkerState::Starting);
        assert_eq!(status.messages_sent, 0);
    }
}
