//! Event types for Atlas
//!
//! Events are immutable records of things that happened - task changes,
//! file modifications, git commits, etc. Each event is self-contained
//! with all relevant context in its payload.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use surrealdb::sql::{Datetime, Thing};

/// An event - an immutable record of something that happened
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique identifier (set by database)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,

    /// When the event occurred
    pub timestamp: Datetime,

    /// Event type (e.g., "task.created", "task.updated", "git.commit")
    pub event_type: String,

    /// Source attribution - who/what triggered this event
    pub source: EventSource,

    /// Event payload - flexible JSON containing event-specific data
    pub payload: Value,
}

/// Source attribution for an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSource {
    /// Who triggered this event (e.g., "user:alice", "agent:claude", "system:forge")
    pub actor: String,

    /// On whose authority (user, agent, or system)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<EventAuthority>,

    /// How the event was triggered (cli, mcp, hook, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

/// Authority level for an event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventAuthority {
    /// Action taken at user's explicit direction
    User,
    /// Action taken by agent on its own initiative
    Agent,
    /// System-generated event (automated, no human/agent decision)
    System,
}

impl Event {
    /// Create a new event
    pub fn new(event_type: impl Into<String>, source: EventSource, payload: Value) -> Self {
        Self {
            id: None,
            timestamp: Datetime::default(),
            event_type: event_type.into(),
            source,
            payload,
        }
    }

    /// Get the event ID as a string, if set
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}

impl EventSource {
    /// Create a source for user-triggered events
    pub fn user(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            authority: Some(EventAuthority::User),
            via: None,
        }
    }

    /// Create a source for agent-triggered events
    pub fn agent(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            authority: Some(EventAuthority::Agent),
            via: None,
        }
    }

    /// Create a source for system-generated events
    pub fn system(component: impl Into<String>) -> Self {
        Self {
            actor: component.into(),
            authority: Some(EventAuthority::System),
            via: None,
        }
    }

    /// Add context about how the event was triggered
    pub fn with_via(mut self, via: impl Into<String>) -> Self {
        self.via = Some(via.into());
        self
    }
}

// ============ Task Event Helpers ============

/// Helper for creating task events with proper structure
pub mod task {
    use super::*;
    use serde_json::json;

    /// Create a task.created event
    pub fn created(task: &Value, source: EventSource) -> Event {
        Event::new(
            "task.created",
            source,
            json!({ "task": task }),
        )
    }

    /// Create a task.updated event
    pub fn updated(
        task_id: &str,
        changes: Value,
        snapshot: &Value,
        source: EventSource,
    ) -> Event {
        Event::new(
            "task.updated",
            source,
            json!({
                "task_id": task_id,
                "changes": changes,
                "snapshot": snapshot
            }),
        )
    }

    /// Create a task.closed event
    pub fn closed(
        task_id: &str,
        reason: Option<&str>,
        snapshot: &Value,
        source: EventSource,
    ) -> Event {
        Event::new(
            "task.closed",
            source,
            json!({
                "task_id": task_id,
                "reason": reason,
                "snapshot": snapshot
            }),
        )
    }

    /// Create a task.deleted event
    pub fn deleted(task_id: &str, snapshot: &Value, source: EventSource) -> Event {
        Event::new(
            "task.deleted",
            source,
            json!({
                "task_id": task_id,
                "snapshot": snapshot
            }),
        )
    }

    /// Create a task.note_added event
    pub fn note_added(task_id: &str, note: &Value, source: EventSource) -> Event {
        Event::new(
            "task.note_added",
            source,
            json!({
                "task_id": task_id,
                "note": note
            }),
        )
    }

    /// Create a task.note_updated event
    pub fn note_updated(
        task_id: &str,
        note_id: &str,
        old_content: &str,
        new_content: &str,
        source: EventSource,
    ) -> Event {
        Event::new(
            "task.note_updated",
            source,
            json!({
                "task_id": task_id,
                "note_id": note_id,
                "old_content": old_content,
                "new_content": new_content
            }),
        )
    }

    /// Create a task.note_deleted event
    pub fn note_deleted(task_id: &str, note_id: &str, source: EventSource) -> Event {
        Event::new(
            "task.note_deleted",
            source,
            json!({
                "task_id": task_id,
                "note_id": note_id
            }),
        )
    }

    /// Create a task.dependency_added event
    pub fn dependency_added(
        from_id: &str,
        to_id: &str,
        relation: &str,
        source: EventSource,
    ) -> Event {
        Event::new(
            "task.dependency_added",
            source,
            json!({
                "from_task_id": from_id,
                "to_task_id": to_id,
                "relation": relation
            }),
        )
    }

    /// Create a task.dependency_removed event
    pub fn dependency_removed(
        from_id: &str,
        to_id: &str,
        relation: &str,
        source: EventSource,
    ) -> Event {
        Event::new(
            "task.dependency_removed",
            source,
            json!({
                "from_task_id": from_id,
                "to_task_id": to_id,
                "relation": relation
            }),
        )
    }
}

// ============ Record Event Helpers ============

/// Helper for creating record change events with proper structure
///
/// These events capture changes to Atlas records (repos, teams, persons,
/// rules, skills, etc.) for provenance tracking.
pub mod record {
    use super::*;
    use serde_json::json;

    /// Create a record.created event
    pub fn created(record: &Value, source: EventSource) -> Event {
        Event::new(
            "record.created",
            source,
            json!({ "record": record }),
        )
    }

    /// Create a record.updated event with diff
    ///
    /// The changes object captures what was modified:
    /// - For simple fields: { "field": { "old": ..., "new": ... } }
    /// - For content updates: { "content": { "old": {...}, "new": {...} } }
    pub fn updated(
        record_id: &str,
        record_type: &str,
        changes: Value,
        snapshot: &Value,
        source: EventSource,
    ) -> Event {
        Event::new(
            "record.updated",
            source,
            json!({
                "record_id": record_id,
                "record_type": record_type,
                "changes": changes,
                "snapshot": snapshot
            }),
        )
    }

    /// Create a record.deleted event (soft delete)
    pub fn deleted(record_id: &str, record_type: &str, snapshot: &Value, source: EventSource) -> Event {
        Event::new(
            "record.deleted",
            source,
            json!({
                "record_id": record_id,
                "record_type": record_type,
                "snapshot": snapshot
            }),
        )
    }

    /// Create a record.edge_created event
    pub fn edge_created(
        source_id: &str,
        target_id: &str,
        relation: &str,
        edge_id: &str,
        source: EventSource,
    ) -> Event {
        Event::new(
            "record.edge_created",
            source,
            json!({
                "source_record_id": source_id,
                "target_record_id": target_id,
                "relation": relation,
                "edge_id": edge_id
            }),
        )
    }

    /// Create a record.edge_superseded event
    ///
    /// Emitted when an edge is marked as superseded (relationship changed)
    pub fn edge_superseded(
        old_edge_id: &str,
        new_edge_id: &str,
        old_target_id: &str,
        new_target_id: &str,
        relation: &str,
        source: EventSource,
    ) -> Event {
        Event::new(
            "record.edge_superseded",
            source,
            json!({
                "old_edge_id": old_edge_id,
                "new_edge_id": new_edge_id,
                "old_target_id": old_target_id,
                "new_target_id": new_target_id,
                "relation": relation
            }),
        )
    }

    /// Create a record.edge_deleted event
    pub fn edge_deleted(edge_id: &str, source: EventSource) -> Event {
        Event::new(
            "record.edge_deleted",
            source,
            json!({ "edge_id": edge_id }),
        )
    }
}
