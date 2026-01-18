//! Database store for Forge task management
//!
//! Handles task, note, and dependency operations.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use db::Database;
use surrealdb::sql::{Datetime, Thing};
use tracing::debug;

use crate::task::{Task, TaskDependency, TaskId, TaskNote, TaskStatus};
use crate::worker::DbWorker;

/// Database store for task management
#[derive(Clone)]
pub struct Store {
    db: Database,
}

impl Store {
    /// Create a new store with the given database connection
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get the database connection
    pub fn db(&self) -> &Database {
        &self.db
    }

    // ========== Task Operations ==========

    /// Create a new task
    pub async fn create_task(&self, task: Task) -> Result<Task> {
        let created: Option<Task> = self
            .db
            .client()
            .create("task")
            .content(task)
            .await
            .context("Failed to create task")?;

        created.context("Task creation returned no result")
    }

    /// List tasks with optional filters
    pub async fn list_tasks(
        &self,
        project: Option<&str>,
        status: Option<TaskStatus>,
    ) -> Result<Vec<Task>> {
        let start = Instant::now();

        let mut query = String::from("SELECT * FROM task");
        let mut conditions = Vec::new();

        if project.is_some() {
            conditions.push("project = $project");
        }
        if status.is_some() {
            conditions.push("status = $status");
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        query.push_str(" ORDER BY priority ASC, created_at DESC");

        let query_start = Instant::now();
        let mut stmt = self.db.client().query(&query);

        if let Some(p) = project {
            stmt = stmt.bind(("project", p.to_string()));
        }
        if let Some(s) = status {
            stmt = stmt.bind(("status", s.to_string()));
        }

        let mut response = stmt.await.context("Failed to query tasks")?;
        let query_elapsed = query_start.elapsed();

        let parse_start = Instant::now();
        let tasks: Vec<Task> = response.take(0).context("Failed to parse tasks")?;
        let parse_elapsed = parse_start.elapsed();

        let total_elapsed = start.elapsed();
        debug!(
            operation = "list_tasks",
            total_ms = total_elapsed.as_micros() as f64 / 1000.0,
            query_ms = query_elapsed.as_micros() as f64 / 1000.0,
            parse_ms = parse_elapsed.as_micros() as f64 / 1000.0,
            task_count = tasks.len(),
            "DB timing breakdown"
        );

        Ok(tasks)
    }

    /// Get tasks that are ready to work on (pending with no blocking dependencies)
    pub async fn ready_tasks(&self, project: Option<&str>) -> Result<Vec<Task>> {
        let query = if project.is_some() {
            r#"
            SELECT * FROM task
            WHERE status = 'pending'
            AND project = $project
            AND id NOT IN (
                SELECT from_task FROM task_dependency
                WHERE relation = 'blocked_by'
                AND (SELECT status FROM task WHERE id = to_task)[0] != 'completed'
            )
            ORDER BY priority ASC, created_at ASC
            "#
        } else {
            r#"
            SELECT * FROM task
            WHERE status = 'pending'
            AND id NOT IN (
                SELECT from_task FROM task_dependency
                WHERE relation = 'blocked_by'
                AND (SELECT status FROM task WHERE id = to_task)[0] != 'completed'
            )
            ORDER BY priority ASC, created_at ASC
            "#
        };

        let mut stmt = self.db.client().query(query);
        if let Some(p) = project {
            stmt = stmt.bind(("project", p.to_string()));
        }

        let mut response = stmt.await.context("Failed to query ready tasks")?;
        let tasks: Vec<Task> = response.take(0).context("Failed to parse ready tasks")?;

        Ok(tasks)
    }

    /// Get a task by ID
    pub async fn get_task(&self, id: &TaskId) -> Result<Option<Task>> {
        let task: Option<Task> = self
            .db
            .client()
            .select(("task", id.as_str()))
            .await
            .context("Failed to get task")?;

        Ok(task)
    }

    /// Update a task
    ///
    /// For description and project, use `Some(Some("value"))` to set,
    /// `Some(None)` to clear, and `None` to leave unchanged.
    pub async fn update_task(
        &self,
        id: &TaskId,
        status: Option<TaskStatus>,
        priority: Option<i32>,
        title: Option<&str>,
        description: Option<Option<&str>>,
        project: Option<Option<&str>>,
    ) -> Result<Option<Task>> {
        let existing = self.get_task(id).await?;
        let Some(mut task) = existing else {
            return Ok(None);
        };

        if let Some(s) = status {
            task.status = s;
            if s == TaskStatus::Completed {
                task.completed_at = Some(Datetime::default());
            }
        }
        if let Some(p) = priority {
            task.priority = p;
        }
        if let Some(t) = title {
            task.title = t.to_string();
        }
        // Option<Option<&str>>: Some(Some("value")) sets, Some(None) clears, None leaves unchanged
        if let Some(d) = description {
            task.description = d.map(|s| s.to_string());
        }
        if let Some(p) = project {
            task.project = p.map(|s| s.to_string());
        }
        task.updated_at = Datetime::default();

        let updated: Option<Task> = self
            .db
            .client()
            .update(("task", id.as_str()))
            .content(task)
            .await
            .context("Failed to update task")?;

        Ok(updated)
    }

    /// Close a task (mark as completed or cancelled)
    ///
    /// If `status` is provided, it will be used directly. Otherwise defaults to `Completed`.
    /// The `reason` is stored independently of the status.
    pub async fn close_task(
        &self,
        id: &TaskId,
        status: Option<TaskStatus>,
        _reason: Option<&str>,
    ) -> Result<Option<Task>> {
        let final_status = status.unwrap_or(TaskStatus::Completed);

        // Only allow completed or cancelled as valid close statuses
        let final_status = match final_status {
            TaskStatus::Completed | TaskStatus::Cancelled => final_status,
            _ => TaskStatus::Completed,
        };

        self.update_task(id, Some(final_status), None, None, None, None)
            .await
    }

    /// Delete a task
    pub async fn delete_task(&self, id: &TaskId) -> Result<Option<Task>> {
        // First delete related notes and dependencies
        self.db
            .client()
            .query("DELETE FROM task_note WHERE task_id = $task_id")
            .bind(("task_id", Thing::from(("task", id.as_str()))))
            .await
            .context("Failed to delete task notes")?;

        self.db
            .client()
            .query("DELETE FROM task_dependency WHERE from_task = $task_id OR to_task = $task_id")
            .bind(("task_id", Thing::from(("task", id.as_str()))))
            .await
            .context("Failed to delete task dependencies")?;

        let deleted: Option<Task> = self
            .db
            .client()
            .delete(("task", id.as_str()))
            .await
            .context("Failed to delete task")?;

        Ok(deleted)
    }

    // ========== Note Operations ==========

    /// Add a note to a task
    pub async fn add_note(&self, task_id: &TaskId, content: &str) -> Result<TaskNote> {
        let now = Datetime::default();
        let note = TaskNote {
            id: None,
            task_id: Thing::from(("task", task_id.as_str())),
            content: content.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        let created: Option<TaskNote> = self
            .db
            .client()
            .create("task_note")
            .content(note)
            .await
            .context("Failed to create note")?;

        created.context("Note creation returned no result")
    }

    /// Get notes for a task
    pub async fn get_notes(&self, task_id: &TaskId) -> Result<Vec<TaskNote>> {
        let mut response = self
            .db
            .client()
            .query("SELECT * FROM task_note WHERE task_id = $task_id ORDER BY created_at ASC")
            .bind(("task_id", Thing::from(("task", task_id.as_str()))))
            .await
            .context("Failed to query notes")?;

        let notes: Vec<TaskNote> = response.take(0).context("Failed to parse notes")?;
        Ok(notes)
    }

    /// List all notes across all tasks (for knowledge rebuild)
    pub async fn list_all_notes(&self) -> Result<Vec<TaskNote>> {
        let mut response = self
            .db
            .client()
            .query("SELECT * FROM task_note ORDER BY created_at ASC")
            .await
            .context("Failed to query all notes")?;

        let notes: Vec<TaskNote> = response.take(0).context("Failed to parse notes")?;
        Ok(notes)
    }

    /// Edit a note
    pub async fn edit_note(&self, note_id: &str, content: &str) -> Result<Option<TaskNote>> {
        let mut response = self
            .db
            .client()
            .query("UPDATE task_note SET content = $content, updated_at = $now WHERE id = $id")
            .bind(("id", Thing::from(("task_note", note_id))))
            .bind(("content", content.to_string()))
            .bind(("now", Datetime::default()))
            .await
            .context("Failed to update note")?;

        let updated: Option<TaskNote> = response.take(0).context("Failed to parse updated note")?;
        Ok(updated)
    }

    /// Delete a note
    pub async fn delete_note(&self, note_id: &str) -> Result<Option<TaskNote>> {
        let deleted: Option<TaskNote> = self
            .db
            .client()
            .delete(("task_note", note_id))
            .await
            .context("Failed to delete note")?;

        Ok(deleted)
    }

    // ========== Dependency Operations ==========

    /// Add a dependency between tasks
    pub async fn add_dependency(
        &self,
        from_id: &TaskId,
        to_id: &TaskId,
        relation: &str,
    ) -> Result<TaskDependency> {
        let dep = TaskDependency {
            id: None,
            from_task: Thing::from(("task", from_id.as_str())),
            to_task: Thing::from(("task", to_id.as_str())),
            relation: relation.to_string(),
            created_at: Datetime::default(),
        };

        let created: Option<TaskDependency> = self
            .db
            .client()
            .create("task_dependency")
            .content(dep)
            .await
            .context("Failed to create dependency")?;

        created.context("Dependency creation returned no result")
    }

    /// Remove a dependency between tasks
    pub async fn remove_dependency(
        &self,
        from_id: &TaskId,
        to_id: &TaskId,
        relation: &str,
    ) -> Result<bool> {
        let mut response = self
            .db
            .client()
            .query(
                "DELETE FROM task_dependency WHERE from_task = $from AND to_task = $to AND relation = $rel",
            )
            .bind(("from", Thing::from(("task", from_id.as_str()))))
            .bind(("to", Thing::from(("task", to_id.as_str()))))
            .bind(("rel", relation.to_string()))
            .await
            .context("Failed to delete dependency")?;

        // Check if anything was deleted
        let _: Vec<TaskDependency> = response.take(0).unwrap_or_default();
        Ok(true)
    }

    /// Get dependencies for a task
    pub async fn get_dependencies(&self, task_id: &TaskId) -> Result<Vec<TaskDependency>> {
        let mut response = self
            .db
            .client()
            .query("SELECT * FROM task_dependency WHERE from_task = $task_id OR to_task = $task_id")
            .bind(("task_id", Thing::from(("task", task_id.as_str()))))
            .await
            .context("Failed to query dependencies")?;

        let deps: Vec<TaskDependency> = response.take(0).context("Failed to parse dependencies")?;
        Ok(deps)
    }

    // ========== Import/Export Operations ==========

    /// Import data from a .surql file
    pub async fn import_from_file(&self, path: &Path) -> Result<usize> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read import file: {}", path.display()))?;

        // Count statements for reporting
        let statement_count = content
            .lines()
            .filter(|line| line.trim().starts_with("INSERT"))
            .count();

        // Execute the SQL
        self.db
            .client()
            .query(&content)
            .await
            .context("Failed to execute import SQL")?;

        Ok(statement_count)
    }

    // ========== Worker Operations ==========

    /// Create a new worker record
    pub async fn create_worker(&self, worker: DbWorker) -> Result<DbWorker> {
        let created: Option<DbWorker> = self
            .db
            .client()
            .create("worker")
            .content(worker)
            .await
            .context("Failed to create worker")?;

        created.context("Worker creation returned no result")
    }

    /// Get a worker by worker_id
    pub async fn get_worker(&self, worker_id: &str) -> Result<Option<DbWorker>> {
        let mut response = self
            .db
            .client()
            .query("SELECT * FROM worker WHERE worker_id = $worker_id LIMIT 1")
            .bind(("worker_id", worker_id.to_string()))
            .await
            .context("Failed to query worker")?;

        let workers: Vec<DbWorker> = response.take(0).context("Failed to parse worker")?;
        Ok(workers.into_iter().next())
    }

    /// List all workers, optionally filtered by state
    pub async fn list_workers(&self, state: Option<&str>) -> Result<Vec<DbWorker>> {
        let query = if state.is_some() {
            "SELECT * FROM worker WHERE state = $state ORDER BY started_at DESC"
        } else {
            "SELECT * FROM worker ORDER BY started_at DESC"
        };

        let mut stmt = self.db.client().query(query);
        if let Some(s) = state {
            stmt = stmt.bind(("state", s.to_string()));
        }

        let mut response = stmt.await.context("Failed to query workers")?;
        let workers: Vec<DbWorker> = response.take(0).context("Failed to parse workers")?;

        Ok(workers)
    }

    /// Update worker status fields
    pub async fn update_worker_status(
        &self,
        worker_id: &str,
        state: &str,
        error_message: Option<&str>,
        current_task: Option<&str>,
        messages_sent: i64,
        messages_received: i64,
    ) -> Result<Option<DbWorker>> {
        let mut response = self
            .db
            .client()
            .query(
                r#"UPDATE worker SET
                    state = $state,
                    error_message = $error_message,
                    current_task = $current_task,
                    messages_sent = $messages_sent,
                    messages_received = $messages_received,
                    last_activity = time::now(),
                    updated_at = time::now()
                WHERE worker_id = $worker_id"#,
            )
            .bind(("worker_id", worker_id.to_string()))
            .bind(("state", state.to_string()))
            .bind(("error_message", error_message.map(|s| s.to_string())))
            .bind(("current_task", current_task.map(|s| s.to_string())))
            .bind(("messages_sent", messages_sent))
            .bind(("messages_received", messages_received))
            .await
            .context("Failed to update worker status")?;

        let updated: Option<DbWorker> = response.take(0).context("Failed to parse updated worker")?;
        Ok(updated)
    }

    /// Update worker session ID (called after each message)
    pub async fn update_worker_session(
        &self,
        worker_id: &str,
        session_id: Option<&str>,
    ) -> Result<Option<DbWorker>> {
        let mut response = self
            .db
            .client()
            .query(
                r#"UPDATE worker SET
                    last_session_id = $session_id,
                    last_activity = time::now(),
                    updated_at = time::now()
                WHERE worker_id = $worker_id"#,
            )
            .bind(("worker_id", worker_id.to_string()))
            .bind(("session_id", session_id.map(|s| s.to_string())))
            .await
            .context("Failed to update worker session")?;

        let updated: Option<DbWorker> = response.take(0).context("Failed to parse updated worker")?;
        Ok(updated)
    }

    /// Delete a worker record
    pub async fn delete_worker(&self, worker_id: &str) -> Result<bool> {
        self.db
            .client()
            .query("DELETE FROM worker WHERE worker_id = $worker_id")
            .bind(("worker_id", worker_id.to_string()))
            .await
            .context("Failed to delete worker")?;

        Ok(true)
    }
}
