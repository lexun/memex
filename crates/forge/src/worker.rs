//! Worker persistence types for cortex workers
//!
//! This module provides database types for persisting cortex worker state,
//! allowing workers to survive daemon restarts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

/// A persisted worker record in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbWorker {
    /// SurrealDB record ID
    pub id: Option<Thing>,
    /// Worker identifier (8-char UUID prefix)
    pub worker_id: String,
    /// Working directory
    pub cwd: String,
    /// System prompt additions
    pub system_prompt: Option<String>,
    /// Model to use
    pub model: Option<String>,
    /// Current state (starting, ready, working, idle, stopped, error)
    pub state: String,
    /// Error message if state is "error"
    pub error_message: Option<String>,
    /// Associated worktree path
    pub worktree: Option<String>,
    /// Current task being worked on
    pub current_task: Option<String>,
    /// When the worker was started
    pub started_at: Datetime,
    /// Last activity timestamp
    pub last_activity: Datetime,
    /// Number of messages sent
    pub messages_sent: i64,
    /// Number of messages received
    pub messages_received: i64,
    /// Claude session ID for resuming
    pub last_session_id: Option<String>,
    /// When the record was created
    pub created_at: Datetime,
    /// When the record was last updated
    pub updated_at: Datetime,
}

impl DbWorker {
    /// Create a new DbWorker with the given worker_id and cwd
    pub fn new(worker_id: impl Into<String>, cwd: impl Into<String>) -> Self {
        let now = Datetime::default();
        Self {
            id: None,
            worker_id: worker_id.into(),
            cwd: cwd.into(),
            system_prompt: None,
            model: None,
            state: "ready".to_string(),
            error_message: None,
            worktree: None,
            current_task: None,
            started_at: now.clone(),
            last_activity: now.clone(),
            messages_sent: 0,
            messages_received: 0,
            last_session_id: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Get the database record ID as a string
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_worktree(mut self, worktree: impl Into<String>) -> Self {
        self.worktree = Some(worktree.into());
        self
    }

    pub fn with_current_task(mut self, task_id: Option<String>) -> Self {
        self.current_task = task_id;
        self
    }
}

/// Convert chrono DateTime to SurrealDB Datetime
pub fn to_surreal_datetime(dt: DateTime<Utc>) -> Datetime {
    Datetime::from(dt)
}

/// Convert SurrealDB Datetime to chrono DateTime
pub fn from_surreal_datetime(dt: &Datetime) -> DateTime<Utc> {
    DateTime::from(dt.0)
}
