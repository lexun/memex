//! Core task types and operations

use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

/// Task ID type (just the key portion)
pub type TaskId = String;

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task is waiting to be started
    Pending,
    /// Task is actively being worked on
    InProgress,
    /// Task is blocked by something
    Blocked,
    /// Task has been completed successfully
    Completed,
    /// Task was cancelled or abandoned
    Cancelled,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Completed => "completed",
            TaskStatus::Cancelled => "cancelled",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(TaskStatus::Pending),
            "in_progress" => Ok(TaskStatus::InProgress),
            "blocked" => Ok(TaskStatus::Blocked),
            "completed" => Ok(TaskStatus::Completed),
            "cancelled" => Ok(TaskStatus::Cancelled),
            _ => anyhow::bail!("Unknown status: {}", s),
        }
    }
}

/// A task representing a unit of work
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// SurrealDB record ID
    pub id: Option<Thing>,
    /// Short title
    pub title: String,
    /// Optional longer description
    pub description: Option<String>,
    /// Current status
    pub status: TaskStatus,
    /// Optional project/category
    pub project: Option<String>,
    /// Priority (higher = more important)
    pub priority: i32,
    /// When the task was created
    pub created_at: Datetime,
    /// When the task was last updated
    pub updated_at: Datetime,
    /// When the task was completed (if applicable)
    pub completed_at: Option<Datetime>,
}

impl Task {
    /// Create a new task with the given title
    pub fn new(title: impl Into<String>) -> Self {
        let now = Datetime::default();
        Self {
            id: None,
            title: title.into(),
            description: None,
            status: TaskStatus::default(),
            project: None,
            priority: 0,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        }
    }

    /// Get the task ID as a string (just the key part)
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the project
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Set the priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

/// A note/update on a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNote {
    /// SurrealDB record ID
    pub id: Option<Thing>,
    /// The task this note belongs to
    pub task_id: Thing,
    /// Note content
    pub content: String,
    /// When the note was created
    pub created_at: Datetime,
    /// When the note was last updated
    pub updated_at: Datetime,
}

/// A dependency between tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDependency {
    /// SurrealDB record ID
    pub id: Option<Thing>,
    /// The task that depends on another
    pub from_task: Thing,
    /// The task being depended on
    pub to_task: Thing,
    /// Type of relationship (blocks, blocked_by, relates_to)
    pub relation: String,
    /// When the dependency was created
    pub created_at: Datetime,
}
