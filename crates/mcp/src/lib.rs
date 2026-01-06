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

use atlas::{EventClient, KnowledgeClient, MemoClient};
use forge::task::{Task, TaskStatus};
use forge::TaskClient;

/// MCP server for Memex task management
pub struct MemexMcpServer {
    task_client: TaskClient,
    memo_client: MemoClient,
    event_client: EventClient,
    knowledge_client: KnowledgeClient,
}

impl MemexMcpServer {
    /// Create a new MCP server connected to the daemon
    pub fn new(socket_path: &Path) -> Self {
        let task_client = TaskClient::new(socket_path);
        let memo_client = MemoClient::new(socket_path);
        let event_client = EventClient::new(socket_path);
        let knowledge_client = KnowledgeClient::new(socket_path);
        Self {
            task_client,
            memo_client,
            event_client,
            knowledge_client,
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
    /// Priority is a simple integer where higher numbers mean more important.
    /// Suggested scale: 0=normal, 1=elevated, 2=high, 3=urgent.
    /// Most tasks should be priority 0. Reserve higher priorities for truly
    /// time-sensitive or blocking work.
    #[tool]
    async fn create_task(
        &self,
        /// Short descriptive title for the task
        title: String,
        /// Detailed description of what needs to be done
        description: Option<String>,
        /// Project name for grouping related tasks
        project: Option<String>,
        /// Priority level (0=normal, higher=more urgent). Default: 0
        priority: Option<i32>,
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
    /// Priority: higher numbers = more urgent (0=normal, 1=elevated, 2=high, 3=urgent)
    #[tool]
    async fn update_task(
        &self,
        /// Task ID to update
        id: String,
        /// New status (pending, in_progress, blocked, completed, cancelled)
        status: Option<String>,
        /// New priority level (0=normal, higher=more urgent)
        priority: Option<i32>,
    ) -> mcp_attr::Result<String> {
        if status.is_none() && priority.is_none() {
            return Err(mcp_attr::Error::new(ErrorCode::INVALID_PARAMS)
                .with_message("Must specify status or priority".to_string(), true));
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

        match self
            .task_client
            .update_task(&id, status_update, priority)
            .await
        {
            Ok(Some(task)) => {
                let id_str = task.id_str().unwrap_or_default();
                Ok(format!(
                    "Updated task: {}\n  Status: {}\n  Priority: {}",
                    id_str, task.status, task.priority
                ))
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

    /// Close a task, marking it as completed
    ///
    /// Use this when work on a task is finished, whether successfully completed
    /// or no longer needed. Optionally provide a reason to document why the task
    /// was closed.
    ///
    /// Prefer close_task over delete_task - closing preserves the task history
    /// and any notes/updates, while deletion permanently removes the task.
    #[tool]
    async fn close_task(
        &self,
        /// Task ID to close
        id: String,
        /// Optional explanation of how/why the task was closed
        reason: Option<String>,
    ) -> mcp_attr::Result<String> {
        match self.task_client.close_task(&id, reason.as_deref()).await {
            Ok(Some(task)) => {
                let id_str = task.id_str().unwrap_or_default();
                Ok(format!("Closed task: {}\n  Status: {}", id_str, task.status))
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
    ) -> mcp_attr::Result<String> {
        match self.task_client.delete_task(&id).await {
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
