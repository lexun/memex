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

    /// Curation processing state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curation: Option<CurationState>,
}

/// Source attribution for a memo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoSource {
    /// Who created this memo (e.g., "user:alice", "agent:claude")
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

/// Curation processing state for a memo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationState {
    /// Whether this memo has been processed by the curation agent
    pub processed: bool,

    /// When the memo was last processed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<Datetime>,

    /// Content hash when last processed (to detect edits requiring re-processing)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,

    /// IDs of records created from this memo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_records: Option<Vec<String>>,

    /// ID of clarification task if one was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clarification_task: Option<String>,
}

impl Memo {
    /// Create a new memo with the given content and source
    pub fn new(content: impl Into<String>, source: MemoSource) -> Self {
        Self {
            id: None,
            created_at: Datetime::default(),
            content: content.into(),
            source,
            curation: None,
        }
    }

    /// Get the memo ID as a string, if set
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }

    /// Compute content hash for change detection
    pub fn content_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.content.as_bytes());
        format!("{:x}", hasher.finalize())
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
