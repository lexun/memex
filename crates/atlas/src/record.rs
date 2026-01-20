//! Record types for Atlas knowledge graph
//!
//! Records are typed, stateful objects that form the middle tier of the
//! three-tier model: Events (immutable) → Records (stateful) → Graph (index).
//!
//! Records have flexible edges allowing graph relationships rather than
//! rigid hierarchies. A Rule can apply_to both a Repo AND a Team.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use surrealdb::sql::{Datetime, Thing};

/// A record - a typed, stateful object in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    /// Unique identifier (set by database)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,

    /// Record type classification
    pub record_type: String,

    /// Human-readable name
    pub name: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Flexible content for type-specific fields
    /// Examples:
    ///   repo: { path, default_branch, languages }
    ///   person: { email, role, preferences }
    ///   rule: { content, severity, auto_apply }
    ///   task: { status, priority, project }
    #[serde(default)]
    pub content: JsonValue,

    /// Soft delete timestamp (None = active)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<Datetime>,

    /// If deleted/merged, points to successor record
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<Thing>,

    /// Event ID that caused supersession
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_via: Option<String>,

    /// When this record was created
    pub created_at: Datetime,

    /// When this record was last updated
    pub updated_at: Datetime,
}

/// Classification of record types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordType {
    /// A code repository
    Repo,
    /// A team of people
    Team,
    /// A person (developer, user, stakeholder)
    Person,
    /// A company or organization
    Company,
    /// A project initiative or epic
    Initiative,
    /// A rule or guideline (coding standards, preferences)
    Rule,
    /// A skill or capability (like Claude skills)
    Skill,
    /// A wiki-like document
    Document,
    /// A task or work item
    Task,
    /// A technology, tool, framework, or service (databases, cloud services, etc.)
    Technology,
}

impl Default for RecordType {
    fn default() -> Self {
        RecordType::Document
    }
}

impl std::fmt::Display for RecordType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordType::Repo => write!(f, "repo"),
            RecordType::Team => write!(f, "team"),
            RecordType::Person => write!(f, "person"),
            RecordType::Company => write!(f, "company"),
            RecordType::Initiative => write!(f, "initiative"),
            RecordType::Rule => write!(f, "rule"),
            RecordType::Skill => write!(f, "skill"),
            RecordType::Document => write!(f, "document"),
            RecordType::Task => write!(f, "task"),
            RecordType::Technology => write!(f, "technology"),
        }
    }
}

impl std::str::FromStr for RecordType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "repo" => Ok(RecordType::Repo),
            "team" => Ok(RecordType::Team),
            "person" => Ok(RecordType::Person),
            "company" => Ok(RecordType::Company),
            "initiative" => Ok(RecordType::Initiative),
            "rule" => Ok(RecordType::Rule),
            "skill" => Ok(RecordType::Skill),
            "document" => Ok(RecordType::Document),
            "task" => Ok(RecordType::Task),
            "technology" => Ok(RecordType::Technology),
            _ => Err(format!("Unknown record type: {}", s)),
        }
    }
}

/// An edge connecting two records
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordEdge {
    /// Unique identifier (set by database)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,

    /// Source record
    pub source: Thing,

    /// Target record
    pub target: Thing,

    /// Relationship type
    pub relation: String,

    /// Optional metadata for the edge
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,

    /// When this edge was created
    pub created_at: Datetime,
}

/// Types of relationships between records
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRelation {
    /// Rule/skill applies to repo/team/person
    AppliesTo,
    /// Repo belongs to team, team belongs to company
    BelongsTo,
    /// Person is member of team
    MemberOf,
    /// Person/team owns repo/initiative
    Owns,
    /// Skill available to person/team/repo
    AvailableTo,
    /// Task depends on task
    DependsOn,
    /// Generic association
    RelatedTo,
}

impl Default for EdgeRelation {
    fn default() -> Self {
        EdgeRelation::RelatedTo
    }
}

impl std::fmt::Display for EdgeRelation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeRelation::AppliesTo => write!(f, "applies_to"),
            EdgeRelation::BelongsTo => write!(f, "belongs_to"),
            EdgeRelation::MemberOf => write!(f, "member_of"),
            EdgeRelation::Owns => write!(f, "owns"),
            EdgeRelation::AvailableTo => write!(f, "available_to"),
            EdgeRelation::DependsOn => write!(f, "depends_on"),
            EdgeRelation::RelatedTo => write!(f, "related_to"),
        }
    }
}

impl std::str::FromStr for EdgeRelation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "applies_to" => Ok(EdgeRelation::AppliesTo),
            "belongs_to" => Ok(EdgeRelation::BelongsTo),
            "member_of" => Ok(EdgeRelation::MemberOf),
            "owns" => Ok(EdgeRelation::Owns),
            "available_to" => Ok(EdgeRelation::AvailableTo),
            "depends_on" => Ok(EdgeRelation::DependsOn),
            "related_to" => Ok(EdgeRelation::RelatedTo),
            _ => Err(format!("Unknown edge relation: {}", s)),
        }
    }
}

impl Record {
    /// Create a new record
    pub fn new(record_type: RecordType, name: impl Into<String>) -> Self {
        let now = Datetime::default();
        Self {
            id: None,
            record_type: record_type.to_string(),
            name: name.into(),
            description: None,
            content: JsonValue::Object(serde_json::Map::new()),
            deleted_at: None,
            superseded_by: None,
            superseded_via: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the content
    pub fn with_content(mut self, content: JsonValue) -> Self {
        self.content = content;
        self
    }

    /// Get the record ID as a string
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }

    /// Check if this record is deleted (soft-deleted)
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

impl RecordEdge {
    /// Create a new edge between records
    pub fn new(source: Thing, target: Thing, relation: EdgeRelation) -> Self {
        Self {
            id: None,
            source,
            target,
            relation: relation.to_string(),
            metadata: None,
            created_at: Datetime::default(),
        }
    }

    /// Add metadata to the edge
    pub fn with_metadata(mut self, metadata: JsonValue) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Get the edge ID as a string
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}

/// Result of a graph traversal for context assembly
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextAssembly {
    /// Records collected during traversal, grouped by type
    pub records: Vec<Record>,
    /// The traversal path (for debugging/explanation)
    pub traversal_path: Vec<String>,
}

impl ContextAssembly {
    /// Get records of a specific type
    pub fn records_of_type(&self, record_type: RecordType) -> Vec<&Record> {
        let type_str = record_type.to_string();
        self.records
            .iter()
            .filter(|r| r.record_type == type_str)
            .collect()
    }

    /// Get all rules
    pub fn rules(&self) -> Vec<&Record> {
        self.records_of_type(RecordType::Rule)
    }

    /// Get all skills
    pub fn skills(&self) -> Vec<&Record> {
        self.records_of_type(RecordType::Skill)
    }

    /// Get all people
    pub fn people(&self) -> Vec<&Record> {
        self.records_of_type(RecordType::Person)
    }
}
