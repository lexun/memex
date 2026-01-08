//! Error types for Cortex

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CortexError {
    #[error("Worker not found: {0}")]
    WorkerNotFound(String),

    #[error("Worker failed to start: {0}")]
    WorkerStartFailed(String),

    #[error("Worker communication failed: {0}")]
    WorkerCommunicationFailed(String),

    #[error("Worker timed out")]
    WorkerTimeout,

    #[error("Worktree error: {0}")]
    WorktreeError(String),

    #[error("Vibetree command failed: {0}")]
    VibetreeFailed(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CortexError>;
