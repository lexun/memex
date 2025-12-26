//! IPC client for task operations
//!
//! Provides the same API as Store but communicates with the daemon over IPC.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use ipc::Client as IpcClient;

use crate::task::{Task, TaskDependency, TaskNote, TaskStatus};

/// Client for task operations via the daemon
///
/// This client has the same API as Store but communicates with the daemon
/// over a Unix socket instead of accessing the database directly.
#[derive(Debug, Clone)]
pub struct TaskClient {
    client: IpcClient,
}

impl TaskClient {
    /// Create a new client for the given socket path
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            client: IpcClient::new(socket_path),
        }
    }

    /// Check if the daemon is running and responsive
    pub async fn health_check(&self) -> Result<bool> {
        self.client.health_check().await
    }

    // ========== Task Operations ==========

    /// Create a new task
    pub async fn create_task(&self, task: Task) -> Result<Task> {
        let result = self
            .client
            .request("create_task", &task)
            .await
            .context("Failed to create task")?;

        serde_json::from_value(result).context("Failed to parse task response")
    }

    /// List tasks with optional filters
    pub async fn list_tasks(
        &self,
        project: Option<&str>,
        status: Option<TaskStatus>,
    ) -> Result<Vec<Task>> {
        #[derive(Serialize)]
        struct Params<'a> {
            project: Option<&'a str>,
            status: Option<String>,
        }

        let params = Params {
            project,
            status: status.map(|s| s.to_string()),
        };

        let result = self
            .client
            .request("list_tasks", &params)
            .await
            .context("Failed to list tasks")?;

        serde_json::from_value(result).context("Failed to parse tasks response")
    }

    /// Get tasks that are ready to work on
    pub async fn ready_tasks(&self, project: Option<&str>) -> Result<Vec<Task>> {
        #[derive(Serialize)]
        struct Params<'a> {
            project: Option<&'a str>,
        }

        let result = self
            .client
            .request("ready_tasks", Params { project })
            .await
            .context("Failed to get ready tasks")?;

        serde_json::from_value(result).context("Failed to parse tasks response")
    }

    /// Get a task by ID
    pub async fn get_task(&self, id: &str) -> Result<Option<Task>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("get_task", Params { id })
            .await
            .context("Failed to get task")?;

        // Handle null response as None
        if result.is_null() {
            return Ok(None);
        }

        let task: Task = serde_json::from_value(result).context("Failed to parse task response")?;
        Ok(Some(task))
    }

    /// Update a task
    pub async fn update_task(
        &self,
        id: &str,
        status: Option<TaskStatus>,
        priority: Option<i32>,
    ) -> Result<Option<Task>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
            status: Option<String>,
            priority: Option<i32>,
        }

        let params = Params {
            id,
            status: status.map(|s| s.to_string()),
            priority,
        };

        let result = self
            .client
            .request("update_task", &params)
            .await
            .context("Failed to update task")?;

        if result.is_null() {
            return Ok(None);
        }

        let task: Task = serde_json::from_value(result).context("Failed to parse task response")?;
        Ok(Some(task))
    }

    /// Close a task (mark as completed or cancelled)
    pub async fn close_task(&self, id: &str, reason: Option<&str>) -> Result<Option<Task>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
            reason: Option<&'a str>,
        }

        let result = self
            .client
            .request("close_task", Params { id, reason })
            .await
            .context("Failed to close task")?;

        if result.is_null() {
            return Ok(None);
        }

        let task: Task = serde_json::from_value(result).context("Failed to parse task response")?;
        Ok(Some(task))
    }

    /// Delete a task
    pub async fn delete_task(&self, id: &str) -> Result<Option<Task>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("delete_task", Params { id })
            .await
            .context("Failed to delete task")?;

        if result.is_null() {
            return Ok(None);
        }

        let task: Task = serde_json::from_value(result).context("Failed to parse task response")?;
        Ok(Some(task))
    }

    // ========== Note Operations ==========

    /// Add a note to a task
    pub async fn add_note(&self, task_id: &str, content: &str) -> Result<TaskNote> {
        #[derive(Serialize)]
        struct Params<'a> {
            task_id: &'a str,
            content: &'a str,
        }

        let result = self
            .client
            .request("add_note", Params { task_id, content })
            .await
            .context("Failed to add note")?;

        serde_json::from_value(result).context("Failed to parse note response")
    }

    /// Get notes for a task
    pub async fn get_notes(&self, task_id: &str) -> Result<Vec<TaskNote>> {
        #[derive(Serialize)]
        struct Params<'a> {
            task_id: &'a str,
        }

        let result = self
            .client
            .request("get_notes", Params { task_id })
            .await
            .context("Failed to get notes")?;

        serde_json::from_value(result).context("Failed to parse notes response")
    }

    /// Edit a note
    pub async fn edit_note(&self, note_id: &str, content: &str) -> Result<Option<TaskNote>> {
        #[derive(Serialize)]
        struct Params<'a> {
            note_id: &'a str,
            content: &'a str,
        }

        let result = self
            .client
            .request("edit_note", Params { note_id, content })
            .await
            .context("Failed to edit note")?;

        if result.is_null() {
            return Ok(None);
        }

        let note: TaskNote =
            serde_json::from_value(result).context("Failed to parse note response")?;
        Ok(Some(note))
    }

    /// Delete a note
    pub async fn delete_note(&self, note_id: &str) -> Result<Option<TaskNote>> {
        #[derive(Serialize)]
        struct Params<'a> {
            note_id: &'a str,
        }

        let result = self
            .client
            .request("delete_note", Params { note_id })
            .await
            .context("Failed to delete note")?;

        if result.is_null() {
            return Ok(None);
        }

        let note: TaskNote =
            serde_json::from_value(result).context("Failed to parse note response")?;
        Ok(Some(note))
    }

    // ========== Dependency Operations ==========

    /// Add a dependency between tasks
    pub async fn add_dependency(
        &self,
        from_id: &str,
        to_id: &str,
        relation: &str,
    ) -> Result<TaskDependency> {
        #[derive(Serialize)]
        struct Params<'a> {
            from_id: &'a str,
            to_id: &'a str,
            relation: &'a str,
        }

        let result = self
            .client
            .request("add_dependency", Params { from_id, to_id, relation })
            .await
            .context("Failed to add dependency")?;

        serde_json::from_value(result).context("Failed to parse dependency response")
    }

    /// Remove a dependency between tasks
    pub async fn remove_dependency(
        &self,
        from_id: &str,
        to_id: &str,
        relation: &str,
    ) -> Result<bool> {
        #[derive(Serialize)]
        struct Params<'a> {
            from_id: &'a str,
            to_id: &'a str,
            relation: &'a str,
        }

        let result = self
            .client
            .request("remove_dependency", Params { from_id, to_id, relation })
            .await
            .context("Failed to remove dependency")?;

        serde_json::from_value(result).context("Failed to parse response")
    }

    /// Get dependencies for a task
    pub async fn get_dependencies(&self, task_id: &str) -> Result<Vec<TaskDependency>> {
        #[derive(Serialize)]
        struct Params<'a> {
            task_id: &'a str,
        }

        let result = self
            .client
            .request("get_dependencies", Params { task_id })
            .await
            .context("Failed to get dependencies")?;

        serde_json::from_value(result).context("Failed to parse dependencies response")
    }
}

/// Parameters for creating a task via IPC
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskParams {
    pub title: String,
    pub description: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i32>,
    pub task_type: Option<String>,
}

impl CreateTaskParams {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            project: None,
            priority: None,
            task_type: None,
        }
    }
}
