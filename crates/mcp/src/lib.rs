//! MCP server for Memex
//!
//! Exposes Memex functionality via the Model Context Protocol (MCP),
//! allowing AI assistants to manage tasks, record memos, and query context.
//!
//! # Tools Provided
//!
//! - **Task management**: create, list, update, close, delete tasks
//! - **Notes**: add, edit, delete task notes
//! - **Dependencies**: manage task relationships
//! - **Memos**: record and query knowledge base memos
//! - **Events**: view the event log
//! - **Context discovery**: search across all stored knowledge
//!
//! # Usage
//!
//! The server runs on stdio and connects to the Memex daemon via Unix socket:
//!
//! ```bash
//! memex mcp serve
//! ```

use std::path::Path;

use anyhow::Result;
use mcp_attr::server::{mcp_server, serve_stdio, McpServer};
use mcp_attr::ErrorCode;

use atlas::{EventClient, KnowledgeClient, MemoClient, RecordClient};
use cortex::CortexClient;
use forge::task::{Task, TaskStatus};
use forge::TaskClient;
use ipc::Client as IpcClient;

/// MCP server for Memex task management
pub struct MemexMcpServer {
    task_client: TaskClient,
    memo_client: MemoClient,
    event_client: EventClient,
    knowledge_client: KnowledgeClient,
    record_client: RecordClient,
    cortex_client: CortexClient,
    ipc_client: IpcClient,
}

impl MemexMcpServer {
    /// Create a new MCP server connected to the daemon
    pub fn new(socket_path: &Path) -> Self {
        let task_client = TaskClient::new(socket_path);
        let memo_client = MemoClient::new(socket_path);
        let event_client = EventClient::new(socket_path);
        let knowledge_client = KnowledgeClient::new(socket_path);
        let record_client = RecordClient::new(socket_path);
        let cortex_client = CortexClient::new(socket_path);
        let ipc_client = IpcClient::new(socket_path);
        Self {
            task_client,
            memo_client,
            event_client,
            knowledge_client,
            record_client,
            cortex_client,
            ipc_client,
        }
    }

    fn format_task_summary(task: &Task) -> String {
        let id = task.id_str().unwrap_or_default();
        let proj = task.project.as_deref().unwrap_or("-");
        format!(
            "{:<20} [{:>1}] {:>11} {} ({})",
            id, task.priority, task.status, task.title, proj
        )
    }
}

#[mcp_server]
impl McpServer for MemexMcpServer {
    /// Create a new task
    ///
    /// Priority is a simple integer where lower numbers mean higher priority.
    /// Scale: 0=critical, 1=high, 2=medium (default), 3=low.
    /// Most tasks should be priority 2. Reserve 0 and 1 for truly urgent or
    /// blocking work.
    #[tool]
    async fn create_task(
        &self,
        /// Short descriptive title for the task
        title: String,
        /// Detailed description of what needs to be done
        description: Option<String>,
        /// Priority level (0=critical, 1=high, 2=medium, 3=low). Default: 0
        priority: Option<i32>,
        /// Project name for grouping related tasks
        project: Option<String>,
        _task_type: Option<String>,
    ) -> mcp_attr::Result<String> {
        let mut task = Task::new(&title).with_priority(priority.unwrap_or(0));

        if let Some(desc) = description {
            task = task.with_description(desc);
        }
        if let Some(proj) = project {
            task = task.with_project(proj);
        }

        match self.task_client.create_task(task).await {
            Ok(created) => {
                let id = created.id_str().unwrap_or_default();
                Ok(format!(
                    "Created task: {}\n  Title: {}\n  Status: {}\n  Priority: {}",
                    id, created.title, created.status, created.priority
                ))
            }
            Err(e) => {
                let msg = format!("Failed to create task: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// List tasks with optional filtering by status and project
    #[tool]
    async fn list_tasks(
        &self,
        status: Option<String>,
        project: Option<String>,
    ) -> mcp_attr::Result<String> {
        let status_filter = status.and_then(|s| s.parse::<TaskStatus>().ok());

        match self.task_client.list_tasks(project.as_deref(), status_filter).await {
            Ok(tasks) => {
                if tasks.is_empty() {
                    Ok("No tasks found".to_string())
                } else {
                    let output = tasks
                        .iter()
                        .map(Self::format_task_summary)
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list tasks: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get tasks that are ready to work on (pending with no blocking dependencies)
    #[tool]
    async fn ready_tasks(&self, project: Option<String>) -> mcp_attr::Result<String> {
        match self.task_client.ready_tasks(project.as_deref()).await {
            Ok(tasks) => {
                if tasks.is_empty() {
                    Ok("No ready tasks".to_string())
                } else {
                    let output = tasks
                        .iter()
                        .map(|t| format!("  {}", Self::format_task_summary(t)))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(format!("Ready tasks:\n{}", output))
                }
            }
            Err(e) => {
                let msg = format!("Failed to get ready tasks: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get detailed information about a task including notes and dependencies
    #[tool]
    async fn get_task(&self, id: String) -> mcp_attr::Result<String> {
        match self.task_client.get_task(&id).await {
            Ok(Some(task)) => {
                let id_str = task.id_str().unwrap_or_default();
                let mut output = format!(
                    "Task: {}\n  Title: {}\n  Status: {}\n  Priority: {}",
                    id_str, task.title, task.status, task.priority
                );

                if let Some(desc) = &task.description {
                    output.push_str(&format!("\n  Description: {}", desc));
                }
                if let Some(proj) = &task.project {
                    output.push_str(&format!("\n  Project: {}", proj));
                }

                // Fetch and display notes
                if let Ok(notes) = self.task_client.get_notes(&id).await {
                    if !notes.is_empty() {
                        output.push_str(&format!("\n\n  Notes ({}):", notes.len()));
                        for note in notes {
                            let note_id = note
                                .id
                                .as_ref()
                                .map(|t| t.id.to_raw())
                                .unwrap_or_default();
                            let timestamp = note.created_at.to_string();
                            output.push_str(&format!(
                                "\n    [{}] ({}) {}",
                                note_id, timestamp, note.content
                            ));
                        }
                    }
                }

                // Fetch and display dependencies
                if let Ok(deps) = self.task_client.get_dependencies(&id).await {
                    if !deps.is_empty() {
                        output.push_str("\n\n  Dependencies:");
                        for dep in deps {
                            let from_id = dep.from_task.id.to_raw();
                            let to_id = dep.to_task.id.to_raw();
                            output.push_str(&format!(
                                "\n    {} {} {}",
                                from_id, dep.relation, to_id
                            ));
                        }
                    }
                }

                Ok(output)
            }
            Ok(None) => {
                let msg = format!("Task not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to get task: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Update a task's status or priority
    ///
    /// Valid status values: pending, in_progress, blocked, completed, cancelled
    /// Priority: lower numbers = higher priority (0=critical, 1=high, 2=medium, 3=low)
    #[tool]
    async fn update_task(
        &self,
        /// Task ID to update
        id: String,
        /// New status (pending, in_progress, blocked, completed, cancelled)
        status: Option<String>,
        /// New priority level (0=critical, 1=high, 2=medium, 3=low)
        priority: Option<i32>,
        /// New title for the task
        title: Option<String>,
        /// New description (use empty string to clear)
        description: Option<String>,
        /// New project (use empty string to clear)
        project: Option<String>,
    ) -> mcp_attr::Result<String> {
        if status.is_none() && priority.is_none() && title.is_none() && description.is_none() && project.is_none() {
            return Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS)
                .with_message("Must specify at least one field to update".to_string(), true));
        }

        let status_update = match &status {
            Some(s) => match s.parse::<TaskStatus>() {
                Ok(st) => Some(st),
                Err(_) => {
                    let msg = format!("Invalid status: {}", s);
                    return Err(
                        mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true)
                    );
                }
            },
            None => None,
        };

        // Convert empty strings to None for clearing fields
        let desc_update = description.as_ref().map(|d| {
            if d.is_empty() { None } else { Some(d.as_str()) }
        });
        let proj_update = project.as_ref().map(|p| {
            if p.is_empty() { None } else { Some(p.as_str()) }
        });

        match self
            .task_client
            .update_task(&id, status_update, priority, title.as_deref(), desc_update, proj_update)
            .await
        {
            Ok(Some(task)) => {
                let id_str = task.id_str().unwrap_or_default();
                let mut result = format!("Updated task: {}", id_str);
                result.push_str(&format!("\n  Title: {}", task.title));
                result.push_str(&format!("\n  Status: {}", task.status));
                result.push_str(&format!("\n  Priority: {}", task.priority));
                if let Some(ref p) = task.project {
                    result.push_str(&format!("\n  Project: {}", p));
                }
                Ok(result)
            }
            Ok(None) => {
                let msg = format!("Task not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to update task: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Close a task, marking it as completed or cancelled
    ///
    /// Use this when work on a task is finished. The task is preserved in history.
    ///
    /// When to use close_task:
    /// - Task was completed successfully
    /// - Task was cancelled/abandoned
    /// - Task is no longer relevant
    ///
    /// Prefer close_task over delete_task - closing preserves history and knowledge.
    #[tool]
    async fn close_task(
        &self,
        /// Task ID to close
        id: String,
        /// Status to set: "completed" (default) or "cancelled"
        status: Option<String>,
        /// Optional explanation of how/why the task was closed
        reason: Option<String>,
    ) -> mcp_attr::Result<String> {
        match self
            .task_client
            .close_task(&id, status.as_deref(), reason.as_deref())
            .await
        {
            Ok(Some(task)) => {
                let id_str = task.id_str().unwrap_or_default();
                let mut result = format!("Closed task: {}\n  Status: {}", id_str, task.status);
                if let Some(ref r) = reason {
                    result.push_str(&format!("\n  Reason: {}", r));
                }
                Ok(result)
            }
            Ok(None) => {
                let msg = format!("Task not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to close task: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Permanently delete a task
    ///
    /// This removes the task and all its notes/updates from the database.
    /// Use this only when you want to completely erase a task, such as
    /// removing duplicates or test data.
    ///
    /// For normal task completion, use close_task instead - it marks the task
    /// as done while preserving the history for future reference.
    #[tool]
    async fn delete_task(
        &self,
        /// Task ID to permanently delete
        id: String,
        /// Reason for deletion (e.g., "duplicate of task X", "test data cleanup")
        /// This helps preserve context for knowledge extraction.
        reason: Option<String>,
    ) -> mcp_attr::Result<String> {
        match self.task_client.delete_task(&id, reason.as_deref()).await {
            Ok(Some(_)) => Ok(format!("Deleted task: {}", id)),
            Ok(None) => {
                let msg = format!("Task not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to delete task: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Add a note/update to a task
    #[tool]
    async fn add_task_update(
        &self,
        task_id: String,
        content: String,
    ) -> mcp_attr::Result<String> {
        match self.task_client.add_note(&task_id, &content).await {
            Ok(note) => {
                let note_id = note.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();
                // Get count of notes
                match self.task_client.get_notes(&task_id).await {
                    Ok(notes) => Ok(format!(
                        "Added note to task: {}\n  Note ID: {}\n  Total notes: {}",
                        task_id,
                        note_id,
                        notes.len()
                    )),
                    Err(_) => Ok(format!("Added note to task: {}\n  Note ID: {}", task_id, note_id)),
                }
            }
            Err(e) => {
                let msg = format!("Failed to add note: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Edit an existing task note
    #[tool]
    async fn edit_task_update(
        &self,
        update_id: String,
        content: String,
    ) -> mcp_attr::Result<String> {
        match self.task_client.edit_note(&update_id, &content).await {
            Ok(Some(_)) => Ok(format!("Updated note: {}", update_id)),
            Ok(None) => {
                let msg = format!("Note not found: {}", update_id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to edit note: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Delete a task note
    #[tool]
    async fn delete_task_update(&self, update_id: String) -> mcp_attr::Result<String> {
        match self.task_client.delete_note(&update_id).await {
            Ok(Some(_)) => Ok(format!("Deleted note: {}", update_id)),
            Ok(None) => {
                let msg = format!("Note not found: {}", update_id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to delete note: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Add a dependency between tasks
    #[tool]
    async fn add_dependency(
        &self,
        from_task_id: String,
        to_task_id: String,
        relation_type: String,
    ) -> mcp_attr::Result<String> {
        // Validate relation type
        let valid_relations = ["blocks", "blocked_by", "relates_to"];
        if !valid_relations.contains(&relation_type.as_str()) {
            let msg = format!(
                "Invalid relation type: {}. Valid types: {:?}",
                relation_type, valid_relations
            );
            return Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true));
        }

        match self
            .task_client
            .add_dependency(&from_task_id, &to_task_id, &relation_type)
            .await
        {
            Ok(_) => Ok(format!(
                "Added dependency: {} {} {}",
                from_task_id, relation_type, to_task_id
            )),
            Err(e) => {
                let msg = format!("Failed to add dependency: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Remove a dependency between tasks
    #[tool]
    async fn remove_dependency(
        &self,
        from_task_id: String,
        to_task_id: String,
        relation_type: String,
    ) -> mcp_attr::Result<String> {
        match self
            .task_client
            .remove_dependency(&from_task_id, &to_task_id, &relation_type)
            .await
        {
            Ok(_) => Ok(format!(
                "Removed dependency: {} {} {}",
                from_task_id, relation_type, to_task_id
            )),
            Err(e) => {
                let msg = format!("Failed to remove dependency: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get dependencies for a task
    #[tool]
    async fn get_dependencies(&self, task_id: String) -> mcp_attr::Result<String> {
        match self.task_client.get_dependencies(&task_id).await {
            Ok(deps) => {
                if deps.is_empty() {
                    Ok(format!("No dependencies for task: {}", task_id))
                } else {
                    let output = deps
                        .iter()
                        .map(|d| {
                            let from_id = d.from_task.id.to_raw();
                            let to_id = d.to_task.id.to_raw();
                            format!("  {} {} {}", from_id, d.relation, to_id)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(format!("Dependencies for task {}:\n{}", task_id, output))
                }
            }
            Err(e) => {
                let msg = format!("Failed to get dependencies: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Memo tools (Atlas knowledge base)
    // -------------------------------------------------------------------------

    /// Record a memo to the knowledge base
    ///
    /// Memos are free-form observations, notes, or facts that you want to remember.
    /// They are recorded with attribution (who recorded them and under what authority).
    #[tool]
    async fn record_memo(&self, content: String) -> mcp_attr::Result<String> {
        // When recording via MCP, we consider this user-directed
        // (the agent is acting as a messenger for the user's intent)
        match self
            .memo_client
            .record_memo(&content, true, Some("user:default"))
            .await
        {
            Ok(memo) => {
                let id = memo
                    .id
                    .as_ref()
                    .map(|t| t.id.to_raw())
                    .unwrap_or_default();
                Ok(format!(
                    "Recorded memo: {}\n  Content: {}\n  Created: {}",
                    id,
                    memo.content,
                    memo.created_at
                ))
            }
            Err(e) => {
                let msg = format!("Failed to record memo: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// List memos from the knowledge base
    #[tool]
    async fn list_memos(&self, limit: Option<i32>) -> mcp_attr::Result<String> {
        let limit = limit.map(|l| l as usize);
        match self.memo_client.list_memos(limit).await {
            Ok(memos) => {
                if memos.is_empty() {
                    Ok("No memos found".to_string())
                } else {
                    let output = memos
                        .iter()
                        .map(|m| {
                            let id = m.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();
                            let content = if m.content.len() > 60 {
                                format!("{}...", &m.content[..60])
                            } else {
                                m.content.clone()
                            };
                            format!("[{}] {}", id, content)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list memos: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get a specific memo by ID
    #[tool]
    async fn get_memo(&self, id: String) -> mcp_attr::Result<String> {
        match self.memo_client.get_memo(&id).await {
            Ok(Some(memo)) => {
                let id_str = memo.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();
                Ok(format!(
                    "Memo: {}\n  Created: {}\n  Actor: {}\n  Authority: {:?}\n  Content: {}",
                    id_str, memo.created_at, memo.source.actor, memo.source.authority, memo.content
                ))
            }
            Ok(None) => {
                let msg = format!("Memo not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to get memo: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Delete a memo by ID
    #[tool]
    async fn delete_memo(&self, id: String) -> mcp_attr::Result<String> {
        match self.memo_client.delete_memo(&id).await {
            Ok(Some(_)) => Ok(format!("Deleted memo: {}", id)),
            Ok(None) => {
                let msg = format!("Memo not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to delete memo: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Event tools (Atlas event history)
    // -------------------------------------------------------------------------

    /// List events from the event log
    ///
    /// Events are immutable records of things that happened - task changes,
    /// notes added, dependencies modified, etc. Use event_type to filter
    /// by prefix (e.g., "task" for all task events, "task.created" for just creations).
    #[tool]
    async fn list_events(
        &self,
        event_type: Option<String>,
        limit: Option<i32>,
    ) -> mcp_attr::Result<String> {
        let limit = limit.map(|l| l as usize);
        match self
            .event_client
            .list_events(event_type.as_deref(), limit)
            .await
        {
            Ok(events) => {
                if events.is_empty() {
                    Ok("No events found".to_string())
                } else {
                    let output = events
                        .iter()
                        .map(|e| {
                            let id = e.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();
                            format!("[{}] {} {}", id, e.timestamp.format("%Y-%m-%d %H:%M"), e.event_type)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list events: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get a specific event by ID
    #[tool]
    async fn get_event(&self, id: String) -> mcp_attr::Result<String> {
        match self.event_client.get_event(&id).await {
            Ok(Some(event)) => {
                let id_str = event.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();
                let payload_str = serde_json::to_string_pretty(&event.payload)
                    .unwrap_or_else(|_| "{}".to_string());
                Ok(format!(
                    "Event: {}\n  Timestamp: {}\n  Type: {}\n  Actor: {}\n  Payload:\n{}",
                    id_str,
                    event.timestamp,
                    event.event_type,
                    event.source.actor,
                    payload_str
                        .lines()
                        .map(|l| format!("    {}", l))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            }
            Ok(None) => {
                let msg = format!("Event not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to get event: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Context tools
    // -------------------------------------------------------------------------

    /// Get project context (tasks + knowledge)
    #[tool]
    async fn get_project_context(&self, project: String) -> mcp_attr::Result<String> {
        // Get tasks for the project
        match self.task_client.list_tasks(Some(&project), None).await {
            Ok(tasks) => {
                let mut output = format!("Project: {}\n\n", project);

                if tasks.is_empty() {
                    output.push_str("No tasks found for this project.");
                } else {
                    output.push_str(&format!("Tasks ({}):\n", tasks.len()));
                    for task in &tasks {
                        output.push_str(&format!("  {}\n", Self::format_task_summary(task)));
                    }
                }

                Ok(output)
            }
            Err(e) => {
                let msg = format!("Failed to get project context: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Query knowledge and get an LLM-summarized answer
    ///
    /// Searches extracted facts and uses an LLM to synthesize a natural language
    /// answer. Use this when you want a direct answer to a question.
    #[tool]
    async fn query_knowledge(
        &self,
        query: String,
        project: Option<String>,
        limit: Option<i32>,
    ) -> mcp_attr::Result<String> {
        let limit = limit.map(|l| l as usize);
        match self
            .knowledge_client
            .query(&query, project.as_deref(), limit)
            .await
        {
            Ok(result) => {
                if result.answer.is_empty() {
                    Ok(format!("No relevant knowledge found for: \"{}\"\n\nNote: Facts are extracted from memos. Try recording some memos first.", query))
                } else {
                    Ok(result.answer)
                }
            }
            Err(e) => {
                let msg = format!("Failed to query knowledge: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Search for raw facts matching a query
    ///
    /// Returns the raw extracted facts without LLM processing.
    /// Use this when you want to see the underlying data.
    #[tool]
    async fn search_knowledge(
        &self,
        query: String,
        project: Option<String>,
        limit: Option<i32>,
    ) -> mcp_attr::Result<String> {
        let limit = limit.map(|l| l as usize);
        match self
            .knowledge_client
            .search(&query, project.as_deref(), limit)
            .await
        {
            Ok(result) => {
                if result.results.is_empty() {
                    Ok(format!("No facts found for: \"{}\"\n\nNote: Facts are extracted from memos. Try recording some memos first.", query))
                } else {
                    let mut output = String::new();
                    output.push_str(&format!("Found {} fact(s) for: \"{}\"\n\n", result.count, query));

                    for fact in result.results {
                        let content = fact.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        let fact_type = fact.get("fact_type").and_then(|v| v.as_str()).unwrap_or("statement");
                        let confidence = fact.get("confidence").and_then(|v| v.as_f64()).unwrap_or(1.0);
                        let score = fact.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);

                        output.push_str(&format!(
                            "[{}] (conf: {:.0}%, score: {:.2}) {}\n",
                            fact_type,
                            confidence * 100.0,
                            score,
                            content
                        ));
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to search knowledge: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// List all known entities
    ///
    /// Shows entities (projects, people, technologies, concepts) that have been
    /// extracted from memos. Use this to discover what the system knows about.
    #[tool]
    async fn list_entities(
        &self,
        /// Filter by project
        project: Option<String>,
        /// Filter by entity type (project, person, technology, concept, task, document)
        entity_type: Option<String>,
        /// Maximum number of entities to return
        limit: Option<i32>,
    ) -> mcp_attr::Result<String> {
        let limit = limit.map(|l| l as usize);
        match self
            .knowledge_client
            .list_entities(project.as_deref(), entity_type.as_deref(), limit)
            .await
        {
            Ok(entities) => {
                if entities.is_empty() {
                    Ok("No entities found.\n\nNote: Entities are extracted from memos. Try recording some memos first.".to_string())
                } else {
                    let mut output = format!("Found {} entities:\n\n", entities.len());
                    for entity in entities {
                        let id = entity.id_str().unwrap_or_default();
                        output.push_str(&format!(
                            "[{}] {} ({})\n",
                            entity.entity_type, entity.name, id
                        ));
                        if !entity.description.is_empty() {
                            output.push_str(&format!("  {}\n", entity.description));
                        }
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list entities: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get facts about a specific entity
    ///
    /// Returns all known facts that mention or relate to the named entity.
    /// This enables entity-centric queries like "what do we know about X?"
    #[tool]
    async fn get_entity_facts(
        &self,
        /// Entity name to look up
        name: String,
        /// Filter by project
        project: Option<String>,
    ) -> mcp_attr::Result<String> {
        match self
            .knowledge_client
            .get_entity_facts(&name, project.as_deref())
            .await
        {
            Ok(result) => {
                if result.facts.is_empty() {
                    Ok(format!(
                        "No facts found for entity: \"{}\"\n\nThe entity may not exist or have no linked facts.",
                        name
                    ))
                } else {
                    let mut output = format!(
                        "Found {} fact(s) about \"{}\":\n\n",
                        result.count, result.entity
                    );
                    for fact in result.facts {
                        let content = &fact.content;
                        let fact_type = &fact.fact_type;
                        output.push_str(&format!("[{}] {}\n", fact_type, content));
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to get entity facts: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get facts related to a given fact via shared entities
    ///
    /// Traverses the knowledge graph to find facts that share entities with
    /// the specified fact. This enables discovery of related knowledge.
    #[tool]
    async fn get_related_facts(
        &self,
        /// The fact ID to find related facts for
        fact_id: String,
        /// Maximum number of related facts to return
        limit: Option<i32>,
    ) -> mcp_attr::Result<String> {
        let limit = limit.map(|l| l as usize);
        match self
            .knowledge_client
            .get_related_facts(&fact_id, limit)
            .await
        {
            Ok(result) => {
                if result.related_facts.is_empty() {
                    Ok(format!(
                        "No related facts found for fact: {}\n\nThe fact may not exist or have no entity links.",
                        fact_id
                    ))
                } else {
                    let mut output = format!(
                        "Found {} related fact(s) for {}:\n\n",
                        result.count, result.fact_id
                    );
                    for fact in result.related_facts {
                        let id = fact.id_str().unwrap_or_default();
                        let content = &fact.content;
                        let fact_type = &fact.fact_type;
                        output.push_str(&format!("[{}] {} ({})\n", fact_type, content, id));
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to get related facts: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Cortex tools (worker management)
    // -------------------------------------------------------------------------

    /// Create a new worker for multi-agent orchestration
    ///
    /// Workers are Claude processes that can operate in isolated directories.
    /// Use this to spawn parallel workers for different tasks.
    #[tool]
    async fn cortex_create_worker(
        &self,
        /// Working directory for the worker
        cwd: String,
        /// Model to use (e.g., "haiku", "sonnet", "opus")
        model: Option<String>,
        /// Additional system prompt context for the worker
        system_prompt: Option<String>,
        /// If true (default), worker won't inherit user's MCP servers.
        /// Set to false to let worker use the user's configured MCP servers.
        mcp_strict: Option<bool>,
        /// List of MCP server JSON configs to include.
        /// Each entry should be a JSON string containing MCP server configuration.
        /// Example: ["{\"mcpServers\":{\"memex\":{\"command\":\"memex\",\"args\":[\"mcp\",\"serve\"]}}}"]
        mcp_servers: Option<Vec<String>>,
        /// Record ID to assemble context from. If provided, rules, skills, and team info
        /// related to this record will be automatically added to the worker's system prompt.
        /// This is typically a repo or project record ID.
        context_from: Option<String>,
    ) -> mcp_attr::Result<String> {
        // Build the system prompt, optionally including assembled context
        let final_system_prompt = if let Some(ref record_id) = context_from {
            // Assemble context from the specified record
            let context = self
                .record_client
                .assemble_context(record_id, 3)
                .await
                .map_err(|e| {
                    let msg = format!("Failed to assemble context from {}: {}", record_id, e);
                    mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true)
                })?;

            let context_prompt = context.to_system_prompt();

            // Combine with any user-provided system prompt
            match (system_prompt.clone(), context_prompt.is_empty()) {
                (Some(user_prompt), false) => Some(format!("{}\n\n{}", context_prompt, user_prompt)),
                (Some(user_prompt), true) => Some(user_prompt),
                (None, false) => Some(context_prompt),
                (None, true) => None,
            }
        } else {
            system_prompt.clone()
        };

        match self
            .cortex_client
            .create_worker_with_mcp(
                &cwd,
                model.as_deref(),
                final_system_prompt.as_deref(),
                mcp_strict,
                mcp_servers.clone(),
            )
            .await
        {
            Ok(worker_id) => {
                let mcp_mode = if mcp_strict.unwrap_or(true) {
                    "isolated (no inherited MCP servers)"
                } else {
                    "inheriting user's MCP servers"
                };
                let servers_info = mcp_servers
                    .map(|s| format!("\n  MCP servers: {} configured", s.len()))
                    .unwrap_or_default();
                let context_info = context_from
                    .map(|id| format!("\n  Context from: {}", id))
                    .unwrap_or_default();
                Ok(format!(
                    "Created worker: {}\n  Directory: {}\n  MCP mode: {}{}{}",
                    worker_id, cwd, mcp_mode, servers_info, context_info
                ))
            }
            Err(e) => {
                let msg = format!("Failed to create worker: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Send a message to a worker and get the response
    ///
    /// The message is sent to the Claude worker, which processes it
    /// and returns a response. Workers maintain session context between calls.
    ///
    /// If run_in_background is true, returns immediately with a message_id.
    /// Use cortex_get_response to retrieve the result later.
    #[tool]
    async fn cortex_send_message(
        &self,
        worker_id: String,
        message: String,
        run_in_background: Option<bool>,
    ) -> mcp_attr::Result<String> {
        let wid = cortex::WorkerId::from_string(&worker_id);

        if run_in_background.unwrap_or(false) {
            // Async mode: dispatch and return immediately
            match self.cortex_client.send_message_async(&wid, &message).await {
                Ok(message_id) => {
                    Ok(format!("Message dispatched to worker {}.\nMessage ID: {}\nUse cortex_get_response to retrieve the result.", worker_id, message_id))
                }
                Err(e) => {
                    let msg = format!("Failed to dispatch message: {}", e);
                    Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
                }
            }
        } else {
            // Sync mode: wait for response
            match self.cortex_client.send_message(&wid, &message).await {
                Ok(response) => {
                    let mut output = String::new();
                    if response.is_error {
                        output.push_str(&format!("Worker {} returned an error:\n", worker_id));
                    }
                    output.push_str(&response.result);
                    output.push_str(&format!("\n\n[Duration: {}ms]", response.duration_ms));
                    Ok(output)
                }
                Err(e) => {
                    let msg = format!("Failed to send message: {}", e);
                    Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
                }
            }
        }
    }

    /// Get the response from an async message
    ///
    /// Returns the response if ready, or status if still processing.
    #[tool]
    async fn cortex_get_response(
        &self,
        /// Message ID from cortex_send_message with run_in_background=true
        message_id: String,
    ) -> mcp_attr::Result<String> {
        match self.cortex_client.get_response(&message_id).await {
            Ok(Some(response)) => {
                let mut output = String::new();
                if response.is_error {
                    output.push_str("Worker returned an error:\n");
                }
                output.push_str(&response.result);
                output.push_str(&format!("\n\n[Duration: {}ms]", response.duration_ms));
                Ok(output)
            }
            Ok(None) => {
                Ok(format!("Message {} is still processing. Check back later.", message_id))
            }
            Err(e) => {
                let msg = format!("Failed to get response: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get detailed status of a worker
    #[tool]
    async fn cortex_worker_status(&self, worker_id: String) -> mcp_attr::Result<String> {
        let wid = cortex::WorkerId::from_string(&worker_id);
        match self.cortex_client.get_worker_status(&wid).await {
            Ok(status) => {
                let mut output = format!("Worker: {}\n", worker_id);
                output.push_str(&format!("  State: {:?}\n", status.state));
                if let Some(ref wt) = status.worktree {
                    output.push_str(&format!("  Directory: {}\n", wt));
                }
                if let Some(ref task) = status.current_task {
                    output.push_str(&format!("  Current Task: {}\n", task));
                }
                output.push_str(&format!("  Started: {}\n", status.started_at.format("%Y-%m-%d %H:%M:%S UTC")));
                output.push_str(&format!("  Last Activity: {}\n", status.last_activity.format("%Y-%m-%d %H:%M:%S UTC")));
                output.push_str(&format!(
                    "  Messages: {} sent, {} received",
                    status.messages_sent, status.messages_received
                ));
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Failed to get worker status: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// List all workers
    #[tool]
    async fn cortex_list_workers(&self) -> mcp_attr::Result<String> {
        match self.cortex_client.list_workers().await {
            Ok(workers) => {
                if workers.is_empty() {
                    Ok("No workers running".to_string())
                } else {
                    let mut output = format!("Workers ({}):\n", workers.len());
                    for status in workers {
                        // Format state with activity info
                        let state_str = match &status.state {
                            cortex::WorkerState::Working => {
                                if let Some(ref task) = status.current_task {
                                    format!("Working: {}", task)
                                } else {
                                    "Working".to_string()
                                }
                            }
                            other => format!("{:?}", other),
                        };

                        // Calculate idle time for non-working workers
                        let activity_info = if !matches!(status.state, cortex::WorkerState::Working) {
                            let idle_secs = (chrono::Utc::now() - status.last_activity).num_seconds();
                            if idle_secs > 60 {
                                format!(" (idle {}m)", idle_secs / 60)
                            } else if idle_secs > 0 {
                                format!(" (idle {}s)", idle_secs)
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };

                        // Get directory basename for cleaner output
                        let dir = status.worktree.as_deref().map(|p| {
                            std::path::Path::new(p)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(p)
                        }).unwrap_or("-");

                        output.push_str(&format!(
                            "  {} [{}]{} | {} | msgs: {}/{}\n",
                            status.id,
                            state_str,
                            activity_info,
                            dir,
                            status.messages_sent,
                            status.messages_received,
                        ));
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list workers: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Remove a worker
    ///
    /// Stops and removes the worker. Any in-progress work will be lost.
    #[tool]
    async fn cortex_remove_worker(&self, worker_id: String) -> mcp_attr::Result<String> {
        let wid = cortex::WorkerId::from_string(&worker_id);
        match self.cortex_client.remove_worker(&wid).await {
            Ok(()) => Ok(format!("Removed worker: {}", worker_id)),
            Err(e) => {
                let msg = format!("Failed to remove worker: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get a worker's conversation transcript
    ///
    /// Returns the history of messages sent to and responses received from the worker.
    /// Use this to inspect what a worker has been doing or to debug issues.
    #[tool]
    async fn cortex_worker_transcript(
        &self,
        worker_id: String,
        /// Maximum number of entries to return (defaults to all)
        limit: Option<usize>,
    ) -> mcp_attr::Result<String> {
        let wid = cortex::WorkerId::from_string(&worker_id);
        match self.cortex_client.get_transcript(&wid, limit).await {
            Ok(entries) => {
                if entries.is_empty() {
                    Ok(format!("No transcript entries for worker {}", worker_id))
                } else {
                    let mut output = format!("Transcript for worker {} ({} entries):\n\n", worker_id, entries.len());
                    for (i, entry) in entries.iter().enumerate() {
                        output.push_str(&format!("--- Entry {} ({}) ---\n", i + 1, entry.timestamp.format("%H:%M:%S")));

                        // Show truncated prompt
                        let prompt_preview = if entry.prompt.len() > 200 {
                            format!("{}...", &entry.prompt[..200])
                        } else {
                            entry.prompt.clone()
                        };
                        output.push_str(&format!("Prompt: {}\n", prompt_preview));

                        match &entry.response {
                            Some(response) => {
                                // Show truncated response
                                let response_preview = if response.len() > 500 {
                                    format!("{}...", &response[..500])
                                } else {
                                    response.clone()
                                };
                                if entry.is_error {
                                    output.push_str(&format!("Error: {}\n", response_preview));
                                } else {
                                    output.push_str(&format!("Response: {}\n", response_preview));
                                }
                                output.push_str(&format!("Duration: {}ms\n", entry.duration_ms));
                            }
                            None => {
                                output.push_str("Status: Processing...\n");
                            }
                        }
                        output.push('\n');
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to get transcript: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Validate the shell environment for a worker or directory
    ///
    /// Checks if the direnv/nix shell environment loads correctly. This is useful
    /// before performing operations that depend on the shell environment (like
    /// reloading a worker) to ensure they will succeed.
    ///
    /// **Why this matters:** Workers operate in nix devshells via direnv. If a
    /// worker modifies flake.nix and introduces an error, the shell will fail
    /// to load. Validating first allows detecting and fixing issues before
    /// attempting operations that would fail.
    ///
    /// Provide either worker_id (to validate a worker's directory) or path
    /// (to validate an arbitrary directory), but not both.
    #[tool]
    async fn cortex_validate_shell(
        &self,
        /// Worker ID to validate shell for (mutually exclusive with path)
        worker_id: Option<String>,
        /// Directory path to validate shell for (mutually exclusive with worker_id)
        path: Option<String>,
    ) -> mcp_attr::Result<String> {
        // Must provide exactly one of worker_id or path
        let (has_worker, has_path) = (worker_id.is_some(), path.is_some());
        if has_worker == has_path {
            return Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS)
                .with_message("Must provide exactly one of worker_id or path".to_string(), true));
        }

        let validation = if let Some(ref wid) = worker_id {
            let worker_id = cortex::WorkerId::from_string(wid);
            self.cortex_client.validate_shell(&worker_id).await
        } else {
            self.cortex_client.validate_shell_for_path(path.as_ref().unwrap()).await
        };

        match validation {
            Ok(cortex::ShellValidation::Success { env }) => {
                let mut output = String::from("Shell validation: SUCCESS\n");
                output.push_str(&format!("Environment variables: {} loaded\n", env.len()));

                // Show if nix environment is active
                if let Some(path) = env.get("PATH") {
                    if path.contains("/nix/store") {
                        output.push_str("Nix environment: Active (PATH includes /nix/store)\n");
                    } else {
                        output.push_str("Nix environment: Not detected (PATH doesn't include /nix/store)\n");
                    }
                }

                // Show key environment variables if present
                let key_vars = ["NIX_BUILD_TOP", "IN_NIX_SHELL", "DIRENV_DIR"];
                for var in key_vars {
                    if let Some(val) = env.get(var) {
                        let display_val = if val.len() > 50 {
                            format!("{}...", &val[..50])
                        } else {
                            val.clone()
                        };
                        output.push_str(&format!("  {}: {}\n", var, display_val));
                    }
                }

                Ok(output)
            }
            Ok(cortex::ShellValidation::Failed { error, exit_code }) => {
                let mut output = String::from("Shell validation: FAILED\n\n");
                output.push_str(&format!("Error: {}\n", error));
                if let Some(code) = exit_code {
                    output.push_str(&format!("Exit code: {}\n", code));
                }
                output.push_str("\nThe shell environment failed to load. This typically means:\n");
                output.push_str("- There's a syntax error in flake.nix\n");
                output.push_str("- A dependency failed to build\n");
                output.push_str("- The .envrc is missing or invalid\n");
                output.push_str("\nFix the issue before attempting operations that require the shell.");
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Failed to validate shell: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Vibetree tools (git worktree management)
    // -------------------------------------------------------------------------

    /// List all git worktrees in a repository
    ///
    /// Returns worktrees managed by vibetree, including their branch names,
    /// paths, and allocated port/environment values.
    #[tool]
    async fn vibetree_list(
        &self,
        /// Path to the git repository root
        cwd: String,
    ) -> mcp_attr::Result<String> {
        let params = serde_json::json!({ "cwd": cwd });

        match self.ipc_client.request("vibetree_list", params).await {
            Ok(result) => {
                let worktrees: Vec<serde_json::Value> = serde_json::from_value(result)
                    .map_err(|e| mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR)
                        .with_message(format!("Failed to parse response: {}", e), true))?;

                if worktrees.is_empty() {
                    Ok("No worktrees found".to_string())
                } else {
                    let mut output = format!("Worktrees ({}):\n", worktrees.len());
                    for wt in worktrees {
                        let name = wt.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let status = wt.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                        output.push_str(&format!("  {} [{}]\n", name, status));

                        // Show port/env values if present
                        if let Some(values) = wt.get("values").and_then(|v| v.as_object()) {
                            for (key, val) in values {
                                output.push_str(&format!("    {}={}\n", key, val));
                            }
                        }
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list worktrees: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Create a new git worktree with isolated environment
    ///
    /// Creates a new git worktree and automatically allocates ports/environment
    /// variables for isolated development. The worktree will be created in the
    /// configured branches directory (default: .worktrees/).
    #[tool]
    async fn vibetree_create(
        &self,
        /// Path to the git repository root
        cwd: String,
        /// Name for the new branch/worktree
        branch_name: String,
        /// Optional branch to create from (defaults to current HEAD)
        from_branch: Option<String>,
    ) -> mcp_attr::Result<String> {
        let params = serde_json::json!({
            "cwd": cwd,
            "branch_name": branch_name,
            "from_branch": from_branch,
        });

        match self.ipc_client.request("vibetree_create", params).await {
            Ok(result) => {
                let name = result.get("name").and_then(|v| v.as_str()).unwrap_or(&branch_name);
                let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("created");

                let mut output = format!("Created worktree: {} [{}]\n", name, status);

                // Show allocated values
                if let Some(values) = result.get("values").and_then(|v| v.as_object()) {
                    output.push_str("  Allocated values:\n");
                    for (key, val) in values {
                        output.push_str(&format!("    {}={}\n", key, val));
                    }
                }

                Ok(output)
            }
            Err(e) => {
                let msg = format!("Failed to create worktree: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Remove a git worktree
    ///
    /// Removes a worktree and its associated branch. The allocated ports/values
    /// will be released for reuse.
    #[tool]
    async fn vibetree_remove(
        &self,
        /// Path to the git repository root
        cwd: String,
        /// Name of the branch/worktree to remove
        branch_name: String,
        /// Skip confirmation and force removal (default: true for programmatic use)
        force: Option<bool>,
        /// Keep the git branch after removing the worktree directory
        keep_branch: Option<bool>,
    ) -> mcp_attr::Result<String> {
        let params = serde_json::json!({
            "cwd": cwd,
            "branch_name": branch_name,
            "force": force.unwrap_or(true),
            "keep_branch": keep_branch.unwrap_or(false),
        });

        match self.ipc_client.request("vibetree_remove", params).await {
            Ok(result) => {
                let removed = result.get("removed").and_then(|v| v.as_str()).unwrap_or(&branch_name);
                Ok(format!("Removed worktree: {}", removed))
            }
            Err(e) => {
                let msg = format!("Failed to remove worktree: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Merge a worktree branch into another branch
    ///
    /// Merges the specified branch into the target branch (default: main).
    /// Optionally squashes commits and removes the worktree after merge.
    ///
    /// IMPORTANT: Always provide a clear, descriptive commit message.
    /// Use single-line messages under 50 characters (e.g., "Add user authentication").
    #[tool]
    async fn vibetree_merge(
        &self,
        /// Path to the git repository root
        cwd: String,
        /// Name of the branch to merge
        branch_name: String,
        /// Commit message for the merge. Required. Use a single line under 50 chars
        /// describing what the branch adds (e.g., "Add macOS launchd daemon autostart").
        message: String,
        /// Target branch to merge into (default: "main")
        into: Option<String>,
        /// Squash commits into a single commit
        squash: Option<bool>,
        /// Remove the worktree after successful merge
        remove: Option<bool>,
    ) -> mcp_attr::Result<String> {
        let params = serde_json::json!({
            "cwd": cwd,
            "branch_name": branch_name,
            "into": into,
            "squash": squash.unwrap_or(false),
            "remove": remove.unwrap_or(false),
            "message": message,
        });

        match self.ipc_client.request("vibetree_merge", params).await {
            Ok(result) => {
                let merged = result.get("merged").and_then(|v| v.as_str()).unwrap_or(&branch_name);
                let into_branch = result.get("into").and_then(|v| v.as_str()).unwrap_or("main");
                let squashed = result.get("squashed").and_then(|v| v.as_bool()).unwrap_or(false);
                let removed = result.get("removed").and_then(|v| v.as_bool()).unwrap_or(false);

                let mut output = format!("Merged '{}' into '{}'", merged, into_branch);
                if squashed {
                    output.push_str(" (squashed)");
                }
                if removed {
                    output.push_str("\nWorktree removed after merge");
                }

                Ok(output)
            }
            Err(e) => {
                let msg = format!("Failed to merge worktree: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Graph tools (records and edges)
    // -------------------------------------------------------------------------

    /// List records in the knowledge graph
    ///
    /// Records are typed, stateful objects like repos, teams, rules, skills.
    /// Use filters to find specific record types.
    #[tool]
    async fn list_records(
        &self,
        /// Filter by record type (repo, team, person, company, initiative, rule, skill, document, task)
        record_type: Option<String>,
        /// Include soft-deleted records
        include_deleted: Option<bool>,
        /// Maximum number of records to return
        limit: Option<i32>,
    ) -> mcp_attr::Result<String> {
        let limit = limit.map(|l| l as usize);
        match self
            .record_client
            .list_records(record_type.as_deref(), include_deleted.unwrap_or(false), limit)
            .await
        {
            Ok(records) => {
                if records.is_empty() {
                    Ok("No records found".to_string())
                } else {
                    let mut output = format!("Records ({}):\n", records.len());
                    for record in records {
                        let id = record.id_str().unwrap_or_default();
                        let deleted = if record.is_deleted() { " [DELETED]" } else { "" };
                        output.push_str(&format!(
                            "  [{}] {} ({}){}",
                            record.record_type, record.name, id, deleted
                        ));
                        if let Some(ref desc) = record.description {
                            if desc.len() > 40 {
                                output.push_str(&format!("\n      {}...", &desc[..40]));
                            } else {
                                output.push_str(&format!("\n      {}", desc));
                            }
                        }
                        output.push('\n');
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list records: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Get a specific record by ID
    #[tool]
    async fn get_record(&self, id: String) -> mcp_attr::Result<String> {
        match self.record_client.get_record(&id).await {
            Ok(Some(record)) => {
                let id_str = record.id_str().unwrap_or_default();
                let mut output = format!("Record: {}\n", id_str);
                output.push_str(&format!("  Type: {}\n", record.record_type));
                output.push_str(&format!("  Name: {}\n", record.name));
                if let Some(ref desc) = record.description {
                    output.push_str(&format!("  Description: {}\n", desc));
                }
                if !record.content.is_null() && record.content != serde_json::json!({}) {
                    output.push_str(&format!("  Content: {}\n", record.content));
                }
                output.push_str(&format!("  Created: {}\n", record.created_at));
                if record.is_deleted() {
                    output.push_str(&format!("  Deleted: {:?}\n", record.deleted_at));
                }
                Ok(output)
            }
            Ok(None) => {
                let msg = format!("Record not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to get record: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Create a new record in the knowledge graph
    ///
    /// Records are typed objects that can be connected via edges.
    /// Common types: repo, team, person, rule, skill
    #[tool]
    async fn create_record(
        &self,
        /// Record type (repo, team, person, company, initiative, rule, skill, document, task)
        record_type: String,
        /// Human-readable name for the record
        name: String,
        /// Optional description
        description: Option<String>,
        /// Optional JSON content with type-specific fields
        content: Option<String>,
    ) -> mcp_attr::Result<String> {
        let content_json = if let Some(ref c) = content {
            Some(serde_json::from_str(c).map_err(|e| {
                mcp_attr::Error::new(ErrorCode::INVALID_PARAMS)
                    .with_message(format!("Invalid JSON content: {}", e), true)
            })?)
        } else {
            None
        };

        match self
            .record_client
            .create_record(&record_type, &name, description.as_deref(), content_json)
            .await
        {
            Ok(record) => {
                let id = record.id_str().unwrap_or_default();
                Ok(format!(
                    "Created record: {}\n  Type: {}\n  Name: {}",
                    id, record.record_type, record.name
                ))
            }
            Err(e) => {
                let msg = format!("Failed to create record: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Update an existing record
    #[tool]
    async fn update_record(
        &self,
        /// Record ID to update
        id: String,
        /// New name (optional)
        name: Option<String>,
        /// New description (optional)
        description: Option<String>,
        /// New JSON content (optional, merges with existing)
        content: Option<String>,
    ) -> mcp_attr::Result<String> {
        let content_json = if let Some(ref c) = content {
            Some(serde_json::from_str(c).map_err(|e| {
                mcp_attr::Error::new(ErrorCode::INVALID_PARAMS)
                    .with_message(format!("Invalid JSON content: {}", e), true)
            })?)
        } else {
            None
        };

        match self
            .record_client
            .update_record(&id, name.as_deref(), description.as_deref(), content_json)
            .await
        {
            Ok(Some(record)) => {
                let id_str = record.id_str().unwrap_or_default();
                Ok(format!("Updated record: {}\n  Name: {}", id_str, record.name))
            }
            Ok(None) => {
                let msg = format!("Record not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to update record: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Delete a record (soft-delete)
    ///
    /// Marks the record as deleted but preserves it in the database.
    #[tool]
    async fn delete_record(&self, id: String) -> mcp_attr::Result<String> {
        match self.record_client.delete_record(&id).await {
            Ok(Some(_)) => Ok(format!("Deleted record: {}", id)),
            Ok(None) => {
                let msg = format!("Record not found: {}", id);
                Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS).with_message(msg, true))
            }
            Err(e) => {
                let msg = format!("Failed to delete record: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Create an edge between two records
    ///
    /// Edges define relationships like "rule applies_to repo" or "person member_of team".
    /// Valid relations: applies_to, belongs_to, member_of, owns, available_to, depends_on, part_of, assigned_to, related_to
    #[tool]
    async fn create_edge(
        &self,
        /// Source record ID
        source: String,
        /// Target record ID
        target: String,
        /// Relationship type (applies_to, belongs_to, member_of, owns, available_to, depends_on, part_of, assigned_to, related_to)
        relation: String,
        /// Optional JSON metadata for the edge
        metadata: Option<String>,
    ) -> mcp_attr::Result<String> {
        let metadata_json = if let Some(ref m) = metadata {
            Some(serde_json::from_str(m).map_err(|e| {
                mcp_attr::Error::new(ErrorCode::INVALID_PARAMS)
                    .with_message(format!("Invalid JSON metadata: {}", e), true)
            })?)
        } else {
            None
        };

        match self
            .record_client
            .create_edge(&source, &target, &relation, metadata_json)
            .await
        {
            Ok(edge) => {
                let id = edge.id_str().unwrap_or_default();
                Ok(format!(
                    "Created edge: {}\n  {} --{}-> {}",
                    id, source, relation, target
                ))
            }
            Err(e) => {
                let msg = format!("Failed to create edge: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// List edges for a record
    ///
    /// Shows relationships connected to a record.
    #[tool]
    async fn list_edges(
        &self,
        /// Record ID to list edges for
        id: String,
        /// Direction: "from" (outgoing), "to" (incoming), or "both" (default)
        direction: Option<String>,
    ) -> mcp_attr::Result<String> {
        let dir = direction.as_deref().unwrap_or("both");
        match self.record_client.list_edges(&id, dir).await {
            Ok(edges) => {
                if edges.is_empty() {
                    Ok(format!("No edges found for record: {}", id))
                } else {
                    let mut output = format!("Edges for {} ({}):\n", id, edges.len());
                    for edge in edges {
                        let edge_id = edge.id_str().unwrap_or_default();
                        let source = edge.source.id.to_raw();
                        let target = edge.target.id.to_raw();
                        output.push_str(&format!(
                            "  [{}] {} --{}-> {}\n",
                            edge_id, source, edge.relation, target
                        ));
                    }
                    Ok(output)
                }
            }
            Err(e) => {
                let msg = format!("Failed to list edges: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }

    /// Assemble context from a record
    ///
    /// Starting from a record (e.g., a repo), traverses edges to collect
    /// related records like rules, skills, and team members that apply.
    /// This is used to build context for LLM queries.
    #[tool]
    async fn assemble_context(
        &self,
        /// Record ID to start traversal from
        id: String,
        /// Maximum traversal depth (default: 3)
        depth: Option<i32>,
    ) -> mcp_attr::Result<String> {
        let depth = depth.unwrap_or(3) as usize;
        match self.record_client.assemble_context(&id, depth).await {
            Ok(context) => {
                let mut output = format!("Context from record: {}\n", id);
                output.push_str(&format!("  Depth: {}\n", depth));
                output.push_str(&format!("  Records found: {}\n\n", context.records.len()));

                if context.records.is_empty() {
                    output.push_str("No related records found.");
                } else {
                    // Group by type
                    let rules = context.rules();
                    let skills = context.skills();
                    let people = context.people();

                    if !rules.is_empty() {
                        output.push_str(&format!("Rules ({}):\n", rules.len()));
                        for r in rules {
                            let content = r.content.get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            output.push_str(&format!("  - {}: {}\n", r.name, content));
                        }
                        output.push('\n');
                    }

                    if !skills.is_empty() {
                        output.push_str(&format!("Skills ({}):\n", skills.len()));
                        for s in skills {
                            let desc = s.description.as_deref().unwrap_or("");
                            output.push_str(&format!("  - {}: {}\n", s.name, desc));
                        }
                        output.push('\n');
                    }

                    if !people.is_empty() {
                        output.push_str(&format!("People ({}):\n", people.len()));
                        for p in people {
                            output.push_str(&format!("  - {}\n", p.name));
                        }
                        output.push('\n');
                    }

                    output.push_str("All records:\n");
                    for record in &context.records {
                        output.push_str(&format!(
                            "  [{}] {} ({})\n",
                            record.record_type,
                            record.name,
                            record.id_str().unwrap_or_default()
                        ));
                    }
                }
                Ok(output)
            }
            Err(e) => {
                let msg = format!("Failed to assemble context: {}", e);
                Err(mcp_attr::Error::new(ErrorCode::INTERNAL_ERROR).with_message(msg, true))
            }
        }
    }
}

/// Start the MCP server on stdio
pub async fn start_server(socket_path: &Path) -> Result<()> {
    tracing::info!("Starting Memex MCP server");

    let server = MemexMcpServer::new(socket_path);

    // Check if daemon is reachable
    if !server.task_client.health_check().await? {
        anyhow::bail!(
            "Cannot connect to daemon at {}. Is the daemon running? Try: memex daemon start",
            socket_path.display()
        );
    }

    tracing::info!("Connected to daemon, MCP server ready on stdio");
    serve_stdio(server).await?;
    Ok(())
}
