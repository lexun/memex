//! Fact and Entity types for Atlas knowledge graph
//!
//! Facts are medium-granularity assertions extracted from episodes.
//! Entities are named things referenced by facts.

use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

fn default_confidence() -> f32 {
    1.0
}

/// A fact - a medium-granularity assertion extracted from episodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Unique identifier (set by database)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,

    /// The fact content - a self-contained assertion
    pub content: String,

    /// Fact type classification (stored as string for DB compatibility)
    #[serde(default)]
    pub fact_type: String,

    /// Confidence score (0.0 - 1.0)
    #[serde(default = "default_confidence")]
    pub confidence: f32,

    /// Project/context boundary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// Source episodes (memo IDs, event IDs)
    pub source_episodes: Vec<EpisodeRef>,

    /// When this fact was extracted
    pub created_at: Datetime,

    /// When this fact was last updated
    pub updated_at: Datetime,

    /// How many times this fact has been retrieved
    #[serde(default)]
    pub access_count: u32,

    /// When this fact was last accessed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<Datetime>,
}

/// Reference to a source episode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeRef {
    /// Episode type: "memo" or "event"
    pub episode_type: String,
    /// Episode ID
    pub episode_id: String,
}

/// Classification of fact types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactType {
    /// Objective statement of fact
    Statement,
    /// Decision that was made
    Decision,
    /// Preference or opinion
    Preference,
    /// Question or open issue
    Question,
    /// Technical specification or requirement
    Specification,
}

impl Default for FactType {
    fn default() -> Self {
        FactType::Statement
    }
}

impl std::fmt::Display for FactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FactType::Statement => write!(f, "statement"),
            FactType::Decision => write!(f, "decision"),
            FactType::Preference => write!(f, "preference"),
            FactType::Question => write!(f, "question"),
            FactType::Specification => write!(f, "specification"),
        }
    }
}

/// An entity - a named thing referenced in facts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier (set by database)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,

    /// Canonical name of the entity
    pub name: String,

    /// Entity type classification (stored as string for DB compatibility)
    #[serde(default)]
    pub entity_type: String,

    /// Project/context boundary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// Confidence in this entity's existence/relevance
    #[serde(default = "default_confidence")]
    pub confidence: f32,

    /// Alternative names/aliases
    #[serde(default)]
    pub aliases: Vec<String>,

    /// Source episodes that mention this entity
    pub source_episodes: Vec<EpisodeRef>,

    /// When this entity was first extracted
    pub created_at: Datetime,

    /// When this entity was last updated
    pub updated_at: Datetime,
}

/// Classification of entity types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    /// A project or codebase
    Project,
    /// A person
    Person,
    /// A technology, library, or tool
    Technology,
    /// An abstract concept or pattern
    Concept,
    /// A task or work item
    Task,
    /// A file or document
    Document,
}

impl Default for EntityType {
    fn default() -> Self {
        EntityType::Concept
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityType::Project => write!(f, "project"),
            EntityType::Person => write!(f, "person"),
            EntityType::Technology => write!(f, "technology"),
            EntityType::Concept => write!(f, "concept"),
            EntityType::Task => write!(f, "task"),
            EntityType::Document => write!(f, "document"),
        }
    }
}

impl Fact {
    /// Create a new fact
    pub fn new(content: impl Into<String>, fact_type: FactType, confidence: f32) -> Self {
        let now = Datetime::default();
        Self {
            id: None,
            content: content.into(),
            fact_type: fact_type.to_string(),
            confidence,
            project: None,
            source_episodes: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            access_count: 0,
            last_accessed: None,
        }
    }

    /// Set the project context
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Add a source episode
    pub fn with_source(mut self, episode_type: &str, episode_id: &str) -> Self {
        self.source_episodes.push(EpisodeRef {
            episode_type: episode_type.to_string(),
            episode_id: episode_id.to_string(),
        });
        self
    }

    /// Get the fact ID as a string
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}

impl Entity {
    /// Create a new entity
    pub fn new(name: impl Into<String>, entity_type: EntityType) -> Self {
        let now = Datetime::default();
        Self {
            id: None,
            name: name.into(),
            entity_type: entity_type.to_string(),
            project: None,
            confidence: 1.0,
            aliases: Vec::new(),
            source_episodes: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Set the project context
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Add an alias
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Get the entity ID as a string
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}
