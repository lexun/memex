//! Core types for Cortex

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a worker
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub String);

impl WorkerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string()[..8].to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Current state of a worker
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    /// Worker is starting up
    Starting,
    /// Worker is ready to receive messages
    Ready,
    /// Worker is processing a message
    Working,
    /// Worker is idle, waiting for work
    Idle,
    /// Worker has terminated
    Stopped,
    /// Worker encountered an error
    Error(String),
}

/// Status information about a worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub id: WorkerId,
    pub state: WorkerState,
    pub worktree: Option<String>,
    pub current_task: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub messages_sent: u64,
    pub messages_received: u64,
}

impl WorkerStatus {
    pub fn new(id: WorkerId) -> Self {
        let now = Utc::now();
        Self {
            id,
            state: WorkerState::Starting,
            worktree: None,
            current_task: None,
            started_at: now,
            last_activity: now,
            messages_sent: 0,
            messages_received: 0,
        }
    }
}

/// Configuration for spawning a worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    /// Working directory for the worker (worktree path)
    pub cwd: String,

    /// Initial system prompt additions
    pub system_prompt: Option<String>,

    /// Initial message to send after startup
    pub initial_message: Option<String>,

    /// Model to use (defaults to sonnet)
    pub model: Option<String>,

    /// Maximum context tokens before summarization
    pub max_context: Option<u32>,
}

impl WorkerConfig {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            system_prompt: None,
            initial_message: None,
            model: None,
            max_context: None,
        }
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_initial_message(mut self, message: impl Into<String>) -> Self {
        self.initial_message = Some(message.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

