//! Shared types for the Memex web UI
//!
//! These types are used for API responses and component props.

use serde::{Deserialize, Serialize};

/// Task view model for list and detail pages
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: i32,
    pub project: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Task {
    /// Get CSS class for status badge
    pub fn status_class(&self) -> String {
        self.status.replace('_', "-")
    }

    /// Get CSS class for priority badge
    pub fn priority_class(&self) -> &'static str {
        match self.priority {
            0 => "p0",
            1 => "p1",
            2 => "p2",
            _ => "",
        }
    }
}

/// Worker view model
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Worker {
    pub id: String,
    pub state: String,
    pub current_task: Option<String>,
    pub worktree: Option<String>,
    pub cwd: String,
    pub model: Option<String>,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub started_at: String,
    pub last_activity: String,
    pub error_message: Option<String>,
}

impl Worker {
    /// Get CSS class for state badge
    pub fn state_class(&self) -> &'static str {
        match self.state.as_str() {
            "error" => "error",
            "working" => "working",
            "idle" | "ready" => "idle",
            _ => "",
        }
    }
}

/// Task note view model
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: String,
    pub content: String,
    pub created_at: String,
}

/// Record view model for directory pages
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub id: String,
    pub record_type: String,
    pub name: String,
    pub description: Option<String>,
    pub content: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

/// Memo view model (named MemoView to avoid conflict with leptos::Memo)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MemoView {
    pub id: String,
    pub content: String,
    pub source: String,
    pub created_at: String,
}

/// Event view model
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    pub event_type: String,
    pub source: String,
    pub timestamp: String,
    pub summary: Option<String>,
}

/// Task detail response including notes and assigned workers
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskDetail {
    pub task: Task,
    pub notes: Vec<Note>,
    pub assigned_workers: Vec<Worker>,
}

/// Stats for dashboard and list pages
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct TaskStats {
    pub pending: usize,
    pub in_progress: usize,
    pub done: usize,
    pub total: usize,
}

/// Dashboard stats
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct DashboardStats {
    pub records: usize,
    pub tasks: usize,
    pub memos: usize,
}

/// Record detail response including related records
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RecordDetail {
    pub record: Record,
    pub related: Vec<Record>,
}

/// Transcript entry for worker conversation history
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TranscriptEntry {
    pub timestamp: String,
    pub prompt: String,
    pub response: Option<String>,
    pub is_error: bool,
    pub duration_ms: u64,
}

/// Worker transcript response
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkerTranscript {
    pub source: String,
    pub thread_id: Option<String>,
    pub entries: Vec<TranscriptEntry>,
}

/// Activity feed entry combining worker info with transcript entry
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActivityEntry {
    pub worker_id: String,
    pub worker_state: String,
    pub current_task: Option<String>,
    pub timestamp: String,
    pub prompt: String,
    pub response: Option<String>,
    pub is_error: bool,
    pub duration_ms: u64,
}

/// Activity feed response
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActivityFeed {
    pub entries: Vec<ActivityEntry>,
}
