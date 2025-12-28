//! Memo types for Atlas
//!
//! Memos are standalone pieces of information recorded on request.

use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

/// A memo - a standalone piece of recorded information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memo {
    /// Unique identifier (set by database)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,

    /// When the memo was created
    pub created_at: Datetime,

    /// The memo content
    pub content: String,

    /// Source attribution
    pub source: MemoSource,
}

/// Source attribution for a memo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoSource {
    /// Who created this memo (e.g., "user:luke", "agent:claude")
    pub actor: String,

    /// On whose authority (user or agent)
    pub authority: MemoAuthority,

    /// Additional context about how it was recorded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<MemoContext>,
}

/// Authority level for a memo
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoAuthority {
    /// Recorded at user's explicit direction
    User,
    /// Recorded by agent on its own initiative
    Agent,
}

/// Context about how a memo was recorded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoContext {
    /// How it was recorded (cli, mcp, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

impl Memo {
    /// Create a new memo with the given content and source
    pub fn new(content: impl Into<String>, source: MemoSource) -> Self {
        Self {
            id: None,
            created_at: Datetime::default(),
            content: content.into(),
            source,
        }
    }

    /// Get the memo ID as a string, if set
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}

impl MemoSource {
    /// Create a source for user-directed recording
    pub fn user(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            authority: MemoAuthority::User,
            context: None,
        }
    }

    /// Create a source for agent-initiated recording
    pub fn agent(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            authority: MemoAuthority::Agent,
            context: None,
        }
    }

    /// Add context about how the memo was recorded
    pub fn with_context(mut self, via: impl Into<String>) -> Self {
        self.context = Some(MemoContext {
            via: Some(via.into()),
        });
        self
    }
}
