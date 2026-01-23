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
///
/// Records are typed objects in the knowledge graph. They include both
/// descriptive content (what do we know about X?) and workflow objects
/// (what needs to be done?).
///
/// Tasks are now Records with specialized workflow behavior stored in
/// the `content` field. Tasks use content for workflow state (status, priority, etc).
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
    /// An agent-executable skill (like Claude skills from github.com/anthropics/skills)
    /// NOT personal expertise - that's an attribute of Person records
    Skill,
    /// A wiki-like document (generic narrative content)
    Document,
    /// A technology, tool, framework, or service (databases, cloud services, etc.)
    Technology,
    /// A task - a unit of work with lifecycle and completion semantics
    /// Content fields: { status, impact, urgency, project, completed_at }
    Task,
    /// A goal - a high-level objective that contains sub-goals and tasks
    /// Forms a tree structure: goals -> sub-goals -> tasks
    /// Content fields: { impact, urgency, status }
    Goal,
    /// An agent message - communication between agents (Coordinator, workers, Primary Claude)
    /// Content fields: { message_type, read, read_at }
    Message,
    /// An MCP (Model Context Protocol) server configuration
    /// Content fields: { command, args, env }
    McpServer,
    /// A conversation thread - groups related entries (e.g., Claude session transcripts)
    /// Content fields: { source, session_id, started_at, ended_at, metadata }
    Thread,
    /// A single entry within a thread (e.g., a message turn in a conversation)
    /// Content fields: { role, turn_number, tool_calls, tokens, timestamp }
    Entry,
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
            RecordType::Technology => write!(f, "technology"),
            RecordType::Task => write!(f, "task"),
            RecordType::Goal => write!(f, "goal"),
            RecordType::Message => write!(f, "message"),
            RecordType::McpServer => write!(f, "mcp_server"),
            RecordType::Thread => write!(f, "thread"),
            RecordType::Entry => write!(f, "entry"),
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
            "technology" => Ok(RecordType::Technology),
            "task" => Ok(RecordType::Task),
            "goal" => Ok(RecordType::Goal),
            "message" => Ok(RecordType::Message),
            "mcp_server" => Ok(RecordType::McpServer),
            "thread" => Ok(RecordType::Thread),
            "entry" => Ok(RecordType::Entry),
            _ => Err(format!("Unknown record type: {}", s)),
        }
    }
}

/// An edge connecting two records
///
/// Edges support temporal validity - they can represent relationships that
/// change over time. For example, "TechFlow uses PostgreSQL" might be valid
/// from 2024-01 to 2024-06, then superseded by "TechFlow uses SurrealDB".
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

    /// When this edge was created (ingestion time)
    pub created_at: Datetime,

    // === Temporal Validity ===

    /// When this relationship became valid (event time)
    /// None means "from the beginning" or "unknown start"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<Datetime>,

    /// When this relationship ceased to be valid (event time)
    /// None means "still valid" or "ongoing"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<Datetime>,

    /// If this edge was superseded, points to the replacing edge
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<Thing>,

    /// Event that caused this edge to be superseded (for provenance)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_via: Option<String>,
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
    /// Task depends on task (blocking relationship)
    DependsOn,
    /// Task is part of larger task (decomposition without blocking)
    /// Used for yak-map style task breakdown where subtasks contribute
    /// to parent completion but don't strictly block it
    PartOf,
    /// Task/work assigned to worker or person
    /// Links agents or people to tasks they're actively working on
    AssignedTo,
    /// Message addressed to an agent/worker
    /// Links messages to their recipients for inter-agent communication
    AddressedTo,
    /// Message sent from an agent/worker
    /// Links messages to their sender for provenance
    SentFrom,
    /// Repo/project uses MCP server
    /// Links repos/projects to their configured MCP servers
    UsesServer,
    /// Thread contains entry, project contains document
    /// Links container records to their contents
    Contains,
    /// Thread/entry captures context from session
    /// Links transcript records to their source (repo, task, worker)
    CapturesFrom,
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
            EdgeRelation::PartOf => write!(f, "part_of"),
            EdgeRelation::AssignedTo => write!(f, "assigned_to"),
            EdgeRelation::AddressedTo => write!(f, "addressed_to"),
            EdgeRelation::SentFrom => write!(f, "sent_from"),
            EdgeRelation::UsesServer => write!(f, "uses_server"),
            EdgeRelation::Contains => write!(f, "contains"),
            EdgeRelation::CapturesFrom => write!(f, "captures_from"),
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
            "part_of" => Ok(EdgeRelation::PartOf),
            "assigned_to" => Ok(EdgeRelation::AssignedTo),
            "addressed_to" => Ok(EdgeRelation::AddressedTo),
            "sent_from" => Ok(EdgeRelation::SentFrom),
            "uses_server" => Ok(EdgeRelation::UsesServer),
            "contains" => Ok(EdgeRelation::Contains),
            "captures_from" => Ok(EdgeRelation::CapturesFrom),
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

    /// Set the created_at timestamp (for migrations)
    pub fn with_created_at(mut self, created_at: Datetime) -> Self {
        self.created_at = created_at;
        self
    }

    /// Set the updated_at timestamp (for migrations)
    pub fn with_updated_at(mut self, updated_at: Datetime) -> Self {
        self.updated_at = updated_at;
        self
    }

    /// Set both timestamps at once (for migrations)
    pub fn with_timestamps(mut self, created_at: Datetime, updated_at: Datetime) -> Self {
        self.created_at = created_at;
        self.updated_at = updated_at;
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
            valid_from: None,
            valid_until: None,
            superseded_by: None,
            superseded_via: None,
        }
    }

    /// Set when this relationship became valid
    pub fn with_valid_from(mut self, valid_from: Datetime) -> Self {
        self.valid_from = Some(valid_from);
        self
    }

    /// Set when this relationship ceased to be valid
    pub fn with_valid_until(mut self, valid_until: Datetime) -> Self {
        self.valid_until = Some(valid_until);
        self
    }

    /// Check if this edge is currently valid (not superseded, no valid_until in past)
    pub fn is_current(&self) -> bool {
        self.superseded_by.is_none() && self.valid_until.is_none()
    }

    /// Set the created_at timestamp (for migrations)
    pub fn with_created_at(mut self, created_at: Datetime) -> Self {
        self.created_at = created_at;
        self
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

    /// Get all tasks
    pub fn tasks(&self) -> Vec<&Record> {
        self.records_of_type(RecordType::Task)
    }

    /// Get all MCP servers
    pub fn mcp_servers(&self) -> Vec<&Record> {
        self.records_of_type(RecordType::McpServer)
    }

    /// Convert MCP server records to MCP config JSON strings
    ///
    /// Returns a vector of JSON config strings, one per MCP server.
    /// These can be passed to worker MCP configuration.
    pub fn mcp_server_configs(&self) -> Vec<String> {
        self.mcp_servers()
            .iter()
            .filter_map(|record| {
                let content = McpServerContent::from_json(&record.content)?;
                Some(content.to_mcp_config_json(&record.name))
            })
            .collect()
    }

    /// Format the context assembly as a system prompt for agents
    ///
    /// This creates a structured text representation suitable for injection
    /// into an agent's system prompt.
    pub fn to_system_prompt(&self) -> String {
        let mut sections = Vec::new();

        // Rules section
        let rules = self.rules();
        if !rules.is_empty() {
            let mut rule_lines = vec!["## Rules".to_string(), String::new()];
            for rule in rules {
                rule_lines.push(format!("### {}", rule.name));
                if let Some(ref desc) = rule.description {
                    rule_lines.push(desc.clone());
                }
                rule_lines.push(String::new());
            }
            sections.push(rule_lines.join("\n"));
        }

        // Skills section
        let skills = self.skills();
        if !skills.is_empty() {
            let mut skill_lines = vec!["## Available Skills".to_string(), String::new()];
            for skill in skills {
                skill_lines.push(format!("- **{}**", skill.name));
                if let Some(ref desc) = skill.description {
                    skill_lines.push(format!("  {}", desc));
                }
            }
            sections.push(skill_lines.join("\n"));
        }

        // People/Team section
        let people = self.people();
        if !people.is_empty() {
            let mut people_lines = vec!["## Team".to_string(), String::new()];
            for person in people {
                people_lines.push(format!("- **{}**", person.name));
                if let Some(ref desc) = person.description {
                    people_lines.push(format!("  {}", desc));
                }
            }
            sections.push(people_lines.join("\n"));
        }

        if sections.is_empty() {
            return String::new();
        }

        format!(
            "# Context from Knowledge Base\n\n\
             The following context was automatically assembled from the knowledge graph.\n\n\
             {}",
            sections.join("\n\n")
        )
    }
}

// =============================================================================
// Task-specific types: Tasks are records with workflow semantics
// =============================================================================

/// Impact rating for task/goal importance
///
/// Measures how much completing this task/goal matters to the overall objective.
/// Higher impact items contribute more significantly to goals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    /// Critical impact - essential for success
    Critical,
    /// High impact - significantly contributes to goals
    High,
    /// Medium impact - moderate contribution (default)
    Medium,
    /// Low impact - nice to have but not essential
    Low,
    /// Minimal impact - very minor contribution
    Minimal,
}

impl Default for Impact {
    fn default() -> Self {
        Self::Medium
    }
}

impl std::fmt::Display for Impact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Impact::Critical => write!(f, "critical"),
            Impact::High => write!(f, "high"),
            Impact::Medium => write!(f, "medium"),
            Impact::Low => write!(f, "low"),
            Impact::Minimal => write!(f, "minimal"),
        }
    }
}

impl std::str::FromStr for Impact {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "critical" => Ok(Impact::Critical),
            "high" => Ok(Impact::High),
            "medium" => Ok(Impact::Medium),
            "low" => Ok(Impact::Low),
            "minimal" => Ok(Impact::Minimal),
            _ => Err(format!("Unknown impact level: {}", s)),
        }
    }
}

impl Impact {
    /// Convert to numeric score for sorting (higher = more important)
    pub fn score(&self) -> i32 {
        match self {
            Impact::Critical => 5,
            Impact::High => 4,
            Impact::Medium => 3,
            Impact::Low => 2,
            Impact::Minimal => 1,
        }
    }

    /// Convert from legacy priority (0=critical, 3=low)
    pub fn from_priority(priority: i32) -> Self {
        match priority {
            0 => Impact::Critical,
            1 => Impact::High,
            2 => Impact::Medium,
            3 => Impact::Low,
            _ => Impact::Minimal,
        }
    }
}

/// Urgency rating for time-sensitivity
///
/// Measures how time-sensitive a task/goal is.
/// Urgency can change over time as deadlines approach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    /// Immediate - needs attention right now
    Immediate,
    /// Soon - should be addressed within days
    Soon,
    /// Normal - standard timeline (default)
    Normal,
    /// Later - can be deferred without issue
    Later,
    /// Someday - no time pressure
    Someday,
}

impl Default for Urgency {
    fn default() -> Self {
        Self::Normal
    }
}

impl std::fmt::Display for Urgency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Urgency::Immediate => write!(f, "immediate"),
            Urgency::Soon => write!(f, "soon"),
            Urgency::Normal => write!(f, "normal"),
            Urgency::Later => write!(f, "later"),
            Urgency::Someday => write!(f, "someday"),
        }
    }
}

impl std::str::FromStr for Urgency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "immediate" => Ok(Urgency::Immediate),
            "soon" => Ok(Urgency::Soon),
            "normal" => Ok(Urgency::Normal),
            "later" => Ok(Urgency::Later),
            "someday" => Ok(Urgency::Someday),
            _ => Err(format!("Unknown urgency level: {}", s)),
        }
    }
}

impl Urgency {
    /// Convert to numeric score for sorting (higher = more urgent)
    pub fn score(&self) -> i32 {
        match self {
            Urgency::Immediate => 5,
            Urgency::Soon => 4,
            Urgency::Normal => 3,
            Urgency::Later => 2,
            Urgency::Someday => 1,
        }
    }
}

/// Calculate effective priority from impact and urgency
///
/// Returns a score where higher = should be done first.
/// Uses Eisenhower matrix style weighting: impact matters most, urgency breaks ties.
pub fn calculate_priority(impact: Impact, urgency: Urgency) -> i32 {
    // Impact weighted 2x relative to urgency
    impact.score() * 2 + urgency.score()
}

/// Task status for workflow lifecycle
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
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Blocked => write!(f, "blocked"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(TaskStatus::Pending),
            "in_progress" => Ok(TaskStatus::InProgress),
            "blocked" => Ok(TaskStatus::Blocked),
            "completed" => Ok(TaskStatus::Completed),
            "cancelled" => Ok(TaskStatus::Cancelled),
            _ => Err(format!("Unknown task status: {}", s)),
        }
    }
}

/// Helper struct for task content fields stored in Record.content
///
/// Task records use Record with:
/// - record_type: "task"
/// - name: task title
/// - description: optional detailed description
/// - content: TaskContent (serialized as JSON)
///
/// Tasks can be part of a goal hierarchy using the `part_of` edge relation.
/// Priority is derived from impact + urgency + position in goal tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContent {
    /// Current workflow status
    pub status: TaskStatus,

    /// Legacy priority (lower = higher priority: 0=critical, 1=high, 2=medium, 3=low)
    /// Deprecated: use impact/urgency instead. Kept for backward compatibility.
    #[serde(default = "default_priority")]
    pub priority: i32,

    /// Impact rating - how much this task matters
    #[serde(default)]
    pub impact: Impact,

    /// Urgency rating - how time-sensitive this task is
    #[serde(default)]
    pub urgency: Urgency,

    /// Optional project grouping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// When the task was completed (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<Datetime>,
}

fn default_priority() -> i32 {
    2 // medium
}

impl Default for TaskContent {
    fn default() -> Self {
        Self {
            status: TaskStatus::default(),
            priority: 2, // medium
            impact: Impact::default(),
            urgency: Urgency::default(),
            project: None,
            completed_at: None,
        }
    }
}

impl TaskContent {
    /// Create new task content with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set priority (legacy - prefer with_impact and with_urgency)
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        // Also set impact from priority for consistency
        self.impact = Impact::from_priority(priority);
        self
    }

    /// Set impact rating
    pub fn with_impact(mut self, impact: Impact) -> Self {
        self.impact = impact;
        self
    }

    /// Set urgency rating
    pub fn with_urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// Get effective priority score (higher = should be done first)
    pub fn effective_priority(&self) -> i32 {
        calculate_priority(self.impact, self.urgency)
    }

    /// Set project
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Set status
    pub fn with_status(mut self, status: TaskStatus) -> Self {
        self.status = status;
        self
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// Task note edge relation constant
pub const TASK_NOTE_RELATION: &str = "has_note";

/// Task dependency edge relations
pub mod task_relations {
    /// Task A blocks Task B (B cannot proceed until A is done)
    pub const BLOCKS: &str = "blocks";
    /// Task A is blocked by Task B (A cannot proceed until B is done)
    pub const BLOCKED_BY: &str = "blocked_by";
    /// Tasks are related (informational link)
    pub const RELATES_TO: &str = "relates_to";
}

// =============================================================================
// Goal-specific types: Goals form hierarchical task maps
// =============================================================================

/// Goal status for tracking progress toward objectives
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Goal is defined but not yet being worked on
    NotStarted,
    /// Goal has active sub-goals or tasks
    InProgress,
    /// Goal is achieved
    Achieved,
    /// Goal was abandoned or deprioritized
    Abandoned,
}

impl Default for GoalStatus {
    fn default() -> Self {
        Self::NotStarted
    }
}

impl std::fmt::Display for GoalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoalStatus::NotStarted => write!(f, "not_started"),
            GoalStatus::InProgress => write!(f, "in_progress"),
            GoalStatus::Achieved => write!(f, "achieved"),
            GoalStatus::Abandoned => write!(f, "abandoned"),
        }
    }
}

impl std::str::FromStr for GoalStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "not_started" => Ok(GoalStatus::NotStarted),
            "in_progress" => Ok(GoalStatus::InProgress),
            "achieved" => Ok(GoalStatus::Achieved),
            "abandoned" => Ok(GoalStatus::Abandoned),
            _ => Err(format!("Unknown goal status: {}", s)),
        }
    }
}

/// Helper struct for goal content fields stored in Record.content
///
/// Goal records use Record with:
/// - record_type: "goal"
/// - name: goal title/objective
/// - description: optional detailed description
/// - content: GoalContent (serialized as JSON)
///
/// Goals form a tree structure using `part_of` edges:
/// - Root goals have no parent (trunk of the tree)
/// - Sub-goals link to parent goals via `part_of`
/// - Tasks link to goals via `part_of`
///
/// Work flows from leaves (tasks) toward trunk (root goals).
/// Completing all children advances the parent goal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalContent {
    /// Current status of the goal
    #[serde(default)]
    pub status: GoalStatus,

    /// Impact rating - how important is achieving this goal
    #[serde(default)]
    pub impact: Impact,

    /// Urgency rating - how time-sensitive is this goal
    #[serde(default)]
    pub urgency: Urgency,

    /// Optional target/deadline for the goal
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date: Option<Datetime>,

    /// When the goal was achieved (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub achieved_at: Option<Datetime>,
}

impl Default for GoalContent {
    fn default() -> Self {
        Self {
            status: GoalStatus::default(),
            impact: Impact::default(),
            urgency: Urgency::default(),
            target_date: None,
            achieved_at: None,
        }
    }
}

impl GoalContent {
    /// Create new goal content with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set impact rating
    pub fn with_impact(mut self, impact: Impact) -> Self {
        self.impact = impact;
        self
    }

    /// Set urgency rating
    pub fn with_urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// Set status
    pub fn with_status(mut self, status: GoalStatus) -> Self {
        self.status = status;
        self
    }

    /// Set target date
    pub fn with_target_date(mut self, target: Datetime) -> Self {
        self.target_date = Some(target);
        self
    }

    /// Get effective priority score (higher = should be done first)
    pub fn effective_priority(&self) -> i32 {
        calculate_priority(self.impact, self.urgency)
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// Goal relation constants
pub mod goal_relations {
    /// Sub-goal/task is part of this goal (child -> parent)
    pub const PART_OF: &str = "part_of";
    /// Goal depends on another goal being achieved first
    pub const DEPENDS_ON: &str = "depends_on";
}

// =============================================================================
// Personal ordering: Per-person priority views over goals/tasks
// =============================================================================

/// Personal ordering content for per-person task/goal prioritization
///
/// Different people may have different priority orderings over the same
/// goal tree. A PersonalOrdering record stores one person's view of what
/// matters most to them.
///
/// PersonalOrdering uses Record with:
/// - record_type: "document" (or could add a new type)
/// - name: "{person_name}'s priorities" or similar
/// - content: PersonalOrderingContent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalOrderingContent {
    /// The person this ordering belongs to (record ID)
    pub person_id: String,

    /// Ordered list of goal/task IDs from highest to lowest priority
    /// Items earlier in the list take precedence
    #[serde(default)]
    pub priority_order: Vec<String>,

    /// Override impact ratings for specific items
    /// Key: goal/task ID, Value: personal impact rating
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub impact_overrides: std::collections::HashMap<String, Impact>,

    /// Override urgency ratings for specific items
    /// Key: goal/task ID, Value: personal urgency rating
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub urgency_overrides: std::collections::HashMap<String, Urgency>,
}

impl PersonalOrderingContent {
    /// Create a new personal ordering for a person
    pub fn new(person_id: impl Into<String>) -> Self {
        Self {
            person_id: person_id.into(),
            priority_order: Vec::new(),
            impact_overrides: std::collections::HashMap::new(),
            urgency_overrides: std::collections::HashMap::new(),
        }
    }

    /// Add an item to the priority order
    pub fn with_priority(mut self, item_id: impl Into<String>) -> Self {
        self.priority_order.push(item_id.into());
        self
    }

    /// Override impact for an item
    pub fn with_impact_override(mut self, item_id: impl Into<String>, impact: Impact) -> Self {
        self.impact_overrides.insert(item_id.into(), impact);
        self
    }

    /// Override urgency for an item
    pub fn with_urgency_override(mut self, item_id: impl Into<String>, urgency: Urgency) -> Self {
        self.urgency_overrides.insert(item_id.into(), urgency);
        self
    }

    /// Get effective priority for an item, considering overrides
    pub fn effective_priority_for(&self, item_id: &str, base_impact: Impact, base_urgency: Urgency) -> i32 {
        let impact = self.impact_overrides.get(item_id).copied().unwrap_or(base_impact);
        let urgency = self.urgency_overrides.get(item_id).copied().unwrap_or(base_urgency);

        // Position in priority_order adds bonus (earlier = higher)
        let position_bonus = self.priority_order
            .iter()
            .position(|id| id == item_id)
            .map(|pos| (self.priority_order.len() - pos) as i32)
            .unwrap_or(0);

        calculate_priority(impact, urgency) + position_bonus
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

// =============================================================================
// Message-specific types: Messages are records for inter-agent communication
// =============================================================================

/// Message type for categorizing communications
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// High-level guidance from Primary Claude
    Guidance,
    /// Status update from a worker
    Status,
    /// Request for clarification or help
    Request,
    /// Response to a request
    Response,
    /// Task completion report
    Completion,
    /// Error or issue report
    Error,
    /// General communication
    General,
}

impl Default for MessageType {
    fn default() -> Self {
        Self::General
    }
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Guidance => write!(f, "guidance"),
            MessageType::Status => write!(f, "status"),
            MessageType::Request => write!(f, "request"),
            MessageType::Response => write!(f, "response"),
            MessageType::Completion => write!(f, "completion"),
            MessageType::Error => write!(f, "error"),
            MessageType::General => write!(f, "general"),
        }
    }
}

impl std::str::FromStr for MessageType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "guidance" => Ok(MessageType::Guidance),
            "status" => Ok(MessageType::Status),
            "request" => Ok(MessageType::Request),
            "response" => Ok(MessageType::Response),
            "completion" => Ok(MessageType::Completion),
            "error" => Ok(MessageType::Error),
            "general" => Ok(MessageType::General),
            _ => Err(format!("Unknown message type: {}", s)),
        }
    }
}

/// Helper struct for message content fields stored in Record.content
///
/// Message records use Record with:
/// - record_type: "message"
/// - name: message subject/summary (for display)
/// - description: the actual message content
/// - content: MessageContent (serialized as JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    /// Type of message (guidance, status, request, etc.)
    pub message_type: MessageType,
    /// Sender identifier (worker ID, "coordinator", or "primary")
    pub from: String,
    /// Recipient identifier (worker ID, "coordinator", or "primary")
    pub to: String,
    /// Whether the message has been read
    pub read: bool,
    /// When the message was read (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<Datetime>,
    /// Optional thread/conversation ID for grouping related messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Optional reference to a task ID this message relates to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl Default for MessageContent {
    fn default() -> Self {
        Self {
            message_type: MessageType::default(),
            from: String::new(),
            to: String::new(),
            read: false,
            read_at: None,
            thread_id: None,
            task_id: None,
        }
    }
}

impl MessageContent {
    /// Create new message content
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            ..Default::default()
        }
    }

    /// Set message type
    pub fn with_type(mut self, message_type: MessageType) -> Self {
        self.message_type = message_type;
        self
    }

    /// Set thread ID
    pub fn with_thread(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }

    /// Set task ID
    pub fn with_task(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

// =============================================================================
// McpServer-specific types: MCP servers are records with config in content
// =============================================================================

/// Helper struct for MCP server content fields stored in Record.content
///
/// MCP server records use Record with:
/// - record_type: "mcp_server"
/// - name: human-readable server name (e.g., "Memex", "Filesystem")
/// - description: optional description of what this server provides
/// - content: McpServerContent (serialized as JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerContent {
    /// Command to run (e.g., "memex", "npx", "/usr/local/bin/mcp-server")
    pub command: String,
    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional environment variables to set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<serde_json::Map<String, JsonValue>>,
}

impl McpServerContent {
    /// Create new MCP server content
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: vec![],
            env: None,
        }
    }

    /// Set args
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Set environment variables
    pub fn with_env(mut self, env: serde_json::Map<String, JsonValue>) -> Self {
        self.env = Some(env);
        self
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }

    /// Convert to MCP server JSON config string
    /// Returns the JSON string in the format expected by Claude Code's MCP config
    pub fn to_mcp_config_json(&self, server_name: &str) -> String {
        let mut server_config = serde_json::json!({
            "command": self.command,
        });

        if !self.args.is_empty() {
            server_config["args"] = serde_json::json!(self.args);
        }

        if let Some(ref env) = self.env {
            if !env.is_empty() {
                server_config["env"] = serde_json::json!(env);
            }
        }

        serde_json::json!({
            "mcpServers": {
                server_name: server_config
            }
        })
        .to_string()
    }
}

// =============================================================================
// Project-specific types: Projects are hub records linking all project info
// =============================================================================

/// Standard document types for projects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectDocumentType {
    /// Why this project exists, what problem it solves
    Vision,
    /// How it's structured, component architecture
    Architecture,
    /// What it does, user-facing capabilities
    Features,
    /// What's planned, future features in priority order
    Roadmap,
    /// History of releases and changes
    Changelog,
    /// How people contribute
    Contributing,
    /// Custom/other document type
    Other,
}

impl std::fmt::Display for ProjectDocumentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectDocumentType::Vision => write!(f, "vision"),
            ProjectDocumentType::Architecture => write!(f, "architecture"),
            ProjectDocumentType::Features => write!(f, "features"),
            ProjectDocumentType::Roadmap => write!(f, "roadmap"),
            ProjectDocumentType::Changelog => write!(f, "changelog"),
            ProjectDocumentType::Contributing => write!(f, "contributing"),
            ProjectDocumentType::Other => write!(f, "other"),
        }
    }
}

impl std::str::FromStr for ProjectDocumentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "vision" => Ok(ProjectDocumentType::Vision),
            "architecture" => Ok(ProjectDocumentType::Architecture),
            "features" => Ok(ProjectDocumentType::Features),
            "roadmap" => Ok(ProjectDocumentType::Roadmap),
            "changelog" => Ok(ProjectDocumentType::Changelog),
            "contributing" => Ok(ProjectDocumentType::Contributing),
            "other" => Ok(ProjectDocumentType::Other),
            _ => Err(format!("Unknown project document type: {}", s)),
        }
    }
}

/// A document associated with a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDocument {
    /// Type of document
    pub doc_type: ProjectDocumentType,
    /// Record ID of the document (points to a Document record)
    pub record_id: String,
    /// Display order for UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
}

/// Helper struct for project content fields stored in Record.content
///
/// Project records use Record with:
/// - record_type: "project"
/// - name: project name
/// - description: project overview/summary
/// - content: ProjectContent (serialized as JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContent {
    /// Git repository URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    /// Tools/technologies used (stored as strings, linked via edges to Technology records)
    #[serde(default)]
    pub tools: Vec<String>,
    /// Standard project documents (vision, architecture, etc.)
    #[serde(default)]
    pub documents: Vec<ProjectDocument>,
}

impl Default for ProjectContent {
    fn default() -> Self {
        Self {
            repo_url: None,
            tools: Vec::new(),
            documents: Vec::new(),
        }
    }
}

impl ProjectContent {
    /// Create new project content with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set repository URL
    pub fn with_repo_url(mut self, repo_url: impl Into<String>) -> Self {
        self.repo_url = Some(repo_url.into());
        self
    }

    /// Add a tool/technology
    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tools.push(tool.into());
        self
    }

    /// Add a document
    pub fn with_document(mut self, doc_type: ProjectDocumentType, record_id: impl Into<String>) -> Self {
        self.documents.push(ProjectDocument {
            doc_type,
            record_id: record_id.into(),
            order: None,
        });
        self
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

// =============================================================================
// Thread-specific types: Threads group conversation entries for transcript capture
// =============================================================================

/// Source of a conversation thread
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadSource {
    /// Claude Code CLI session
    ClaudeCode,
    /// Cortex worker session
    CortexWorker,
    /// API conversation
    Api,
    /// Imported from external source
    Import,
    /// Unknown or unspecified source
    Unknown,
}

impl Default for ThreadSource {
    fn default() -> Self {
        Self::Unknown
    }
}

impl std::fmt::Display for ThreadSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThreadSource::ClaudeCode => write!(f, "claude_code"),
            ThreadSource::CortexWorker => write!(f, "cortex_worker"),
            ThreadSource::Api => write!(f, "api"),
            ThreadSource::Import => write!(f, "import"),
            ThreadSource::Unknown => write!(f, "unknown"),
        }
    }
}

impl std::str::FromStr for ThreadSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude_code" => Ok(ThreadSource::ClaudeCode),
            "cortex_worker" => Ok(ThreadSource::CortexWorker),
            "api" => Ok(ThreadSource::Api),
            "import" => Ok(ThreadSource::Import),
            "unknown" => Ok(ThreadSource::Unknown),
            _ => Err(format!("Unknown thread source: {}", s)),
        }
    }
}

/// Helper struct for thread content fields stored in Record.content
///
/// Thread records use Record with:
/// - record_type: "thread"
/// - name: thread title/summary (e.g., "Implement feature X")
/// - description: optional detailed description of the conversation
/// - content: ThreadContent (serialized as JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadContent {
    /// Source of this thread (claude_code, cortex_worker, api, etc.)
    pub source: ThreadSource,
    /// Session ID from the source (e.g., Claude Code session ID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// When the conversation started
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<Datetime>,
    /// When the conversation ended (None if still active)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Datetime>,
    /// Total number of entries in this thread
    #[serde(default)]
    pub entry_count: i32,
    /// Total tokens used in this thread (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i64>,
    /// Working directory or context path (for Claude Code sessions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Associated task ID (if thread is related to a task)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Associated worker ID (for Cortex worker threads)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    /// Custom metadata (source-specific data)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

impl Default for ThreadContent {
    fn default() -> Self {
        Self {
            source: ThreadSource::default(),
            session_id: None,
            started_at: None,
            ended_at: None,
            entry_count: 0,
            total_tokens: None,
            cwd: None,
            task_id: None,
            worker_id: None,
            metadata: None,
        }
    }
}

impl ThreadContent {
    /// Create new thread content with default values
    pub fn new(source: ThreadSource) -> Self {
        Self {
            source,
            ..Default::default()
        }
    }

    /// Set session ID
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set start time
    pub fn with_started_at(mut self, started_at: Datetime) -> Self {
        self.started_at = Some(started_at);
        self
    }

    /// Set end time
    pub fn with_ended_at(mut self, ended_at: Datetime) -> Self {
        self.ended_at = Some(ended_at);
        self
    }

    /// Set working directory
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set task ID
    pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Set worker ID
    pub fn with_worker_id(mut self, worker_id: impl Into<String>) -> Self {
        self.worker_id = Some(worker_id.into());
        self
    }

    /// Set custom metadata
    pub fn with_metadata(mut self, metadata: JsonValue) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

// =============================================================================
// Entry-specific types: Entries are individual turns within a thread
// =============================================================================

/// Role of the entity in a conversation entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryRole {
    /// User/human input
    User,
    /// Assistant/Claude response
    Assistant,
    /// System message or prompt
    System,
    /// Tool call or result
    Tool,
}

impl Default for EntryRole {
    fn default() -> Self {
        Self::User
    }
}

impl std::fmt::Display for EntryRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryRole::User => write!(f, "user"),
            EntryRole::Assistant => write!(f, "assistant"),
            EntryRole::System => write!(f, "system"),
            EntryRole::Tool => write!(f, "tool"),
        }
    }
}

impl std::str::FromStr for EntryRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "user" => Ok(EntryRole::User),
            "assistant" => Ok(EntryRole::Assistant),
            "system" => Ok(EntryRole::System),
            "tool" => Ok(EntryRole::Tool),
            _ => Err(format!("Unknown entry role: {}", s)),
        }
    }
}

/// Helper struct for entry content fields stored in Record.content
///
/// Entry records use Record with:
/// - record_type: "entry"
/// - name: brief summary of the entry (auto-generated or first line)
/// - description: the actual content/text of the entry
/// - content: EntryContent (serialized as JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryContent {
    /// Role of this entry (user, assistant, system, tool)
    pub role: EntryRole,
    /// Turn number within the thread (0-indexed)
    pub turn_number: i32,
    /// Timestamp of this entry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<Datetime>,
    /// Number of tool calls in this entry (for assistant entries)
    #[serde(default)]
    pub tool_call_count: i32,
    /// Names of tools called (for quick reference without parsing content)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_used: Vec<String>,
    /// Token count for this entry (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<i32>,
    /// Cache status (e.g., "hit", "miss", "write")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_status: Option<String>,
    /// Model used for this entry (for assistant entries)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Duration in milliseconds (for assistant entries with timing)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Reference to parent thread ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

impl Default for EntryContent {
    fn default() -> Self {
        Self {
            role: EntryRole::default(),
            turn_number: 0,
            timestamp: None,
            tool_call_count: 0,
            tools_used: Vec::new(),
            tokens: None,
            cache_status: None,
            model: None,
            duration_ms: None,
            thread_id: None,
        }
    }
}

impl EntryContent {
    /// Create new entry content
    pub fn new(role: EntryRole, turn_number: i32) -> Self {
        Self {
            role,
            turn_number,
            ..Default::default()
        }
    }

    /// Set timestamp
    pub fn with_timestamp(mut self, timestamp: Datetime) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Set tool call count
    pub fn with_tool_calls(mut self, count: i32) -> Self {
        self.tool_call_count = count;
        self
    }

    /// Set tools used
    pub fn with_tools_used(mut self, tools: Vec<String>) -> Self {
        self.tools_used = tools;
        self
    }

    /// Set token count
    pub fn with_tokens(mut self, tokens: i32) -> Self {
        self.tokens = Some(tokens);
        self
    }

    /// Set model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set duration
    pub fn with_duration_ms(mut self, duration_ms: i64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Set thread ID reference
    pub fn with_thread_id(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }

    /// Convert to JSON Value for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from Record.content JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

// =============================================================================
// Message view types
// =============================================================================

/// A view of a Message record for API compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageView {
    /// Record ID
    pub id: Option<Thing>,
    /// Subject/summary (from Record.name)
    pub subject: String,
    /// Message body (from Record.description)
    pub body: Option<String>,
    /// Sender identifier
    pub from: String,
    /// Recipient identifier
    pub to: String,
    /// Message type
    pub message_type: MessageType,
    /// Whether the message has been read
    pub read: bool,
    /// When the message was read
    pub read_at: Option<Datetime>,
    /// Thread/conversation ID
    pub thread_id: Option<String>,
    /// Related task ID
    pub task_id: Option<String>,
    /// When the message was created
    pub created_at: Datetime,
}

impl MessageView {
    /// Convert a Record to a MessageView
    pub fn from_record(record: &Record) -> Option<Self> {
        if record.record_type != "message" {
            return None;
        }

        let content = MessageContent::from_json(&record.content)?;

        Some(Self {
            id: record.id.clone(),
            subject: record.name.clone(),
            body: record.description.clone(),
            from: content.from,
            to: content.to,
            message_type: content.message_type,
            read: content.read,
            read_at: content.read_at,
            thread_id: content.thread_id,
            task_id: content.task_id,
            created_at: record.created_at.clone(),
        })
    }

    /// Get the message ID as a string
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}

// =============================================================================
// TaskView - API compatibility layer (matches old Forge Task struct)
// =============================================================================

/// A view of a Task record that matches the old Forge API format
///
/// This provides UX parity with the old Forge task system by presenting
/// Record-based tasks in the same format as the old Forge::Task struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskView {
    /// SurrealDB record ID (same format as old Forge: task:xxxx)
    pub id: Option<Thing>,
    /// Short title (from Record.name)
    pub title: String,
    /// Optional longer description (from Record.description)
    pub description: Option<String>,
    /// Current status (from content.status)
    pub status: TaskStatus,
    /// Optional project/category (from content.project)
    pub project: Option<String>,
    /// Priority (from content.priority)
    pub priority: i32,
    /// When the task was created (from Record.created_at)
    pub created_at: Datetime,
    /// When the task was last updated (from Record.updated_at)
    pub updated_at: Datetime,
    /// When the task was completed (from content.completed_at)
    pub completed_at: Option<Datetime>,
}

impl TaskView {
    /// Convert a Record to a TaskView
    ///
    /// Returns None if the record is not a task or has invalid content.
    pub fn from_record(record: &Record) -> Option<Self> {
        if record.record_type != "task" {
            return None;
        }

        let content = TaskContent::from_json(&record.content).unwrap_or_default();

        // Convert id from record:xxx to task:xxx for backwards compatibility
        let task_id = record.id.as_ref().map(|r| {
            Thing::from(("task", r.id.to_raw().as_str()))
        });

        Some(Self {
            id: task_id,
            title: record.name.clone(),
            description: record.description.clone(),
            status: content.status,
            project: content.project,
            priority: content.priority,
            completed_at: content.completed_at,
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
        })
    }

    /// Get the task ID as a string (just the key part)
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }

    /// Create a new TaskView (for creating new tasks via API)
    pub fn new(title: impl Into<String>) -> Self {
        let now = Datetime::default();
        Self {
            id: None,
            title: title.into(),
            description: None,
            status: TaskStatus::default(),
            project: None,
            priority: 2, // medium
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        }
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

    /// Convert to a Record for storage
    pub fn to_record(&self) -> Record {
        let content = TaskContent {
            status: self.status,
            priority: self.priority,
            impact: Impact::from_priority(self.priority),
            urgency: Urgency::default(),
            project: self.project.clone(),
            completed_at: self.completed_at.clone(),
        };

        let mut record = Record::new(RecordType::Task, &self.title)
            .with_content(content.to_json());

        if let Some(ref desc) = self.description {
            record = record.with_description(desc);
        }

        record
    }

    /// Convert the underlying record ID back to the Atlas record ID format
    ///
    /// The TaskView stores IDs as task:xxx for backwards compatibility,
    /// but Atlas needs record:xxx for database operations.
    pub fn record_id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}

/// A view of a TaskNote that matches the old Forge API format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNoteView {
    /// SurrealDB record ID (format: task_note:xxxx)
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

impl TaskNoteView {
    /// Convert a Record (note) to a TaskNoteView
    pub fn from_record(record: &Record) -> Option<Self> {
        if record.record_type != "document" {
            return None;
        }

        let content = record.content.get("content")?.as_str()?.to_string();
        let task_id_str = record.content.get("task_id")?.as_str()?;

        Some(Self {
            id: record.id.as_ref().map(|r| {
                Thing::from(("task_note", r.id.to_raw().as_str()))
            }),
            task_id: Thing::from(("task", task_id_str)),
            content,
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
        })
    }

    /// Get the note ID as a string
    pub fn id_str(&self) -> Option<String> {
        self.id.as_ref().map(|t| t.id.to_raw())
    }
}

/// A view of a TaskDependency that matches the old Forge API format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDependencyView {
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

impl TaskDependencyView {
    /// Convert a RecordEdge to a TaskDependencyView
    pub fn from_edge(edge: &RecordEdge) -> Self {
        // Convert record:xxx to task:xxx format
        let from_task = Thing::from(("task", edge.source.id.to_raw().as_str()));
        let to_task = Thing::from(("task", edge.target.id.to_raw().as_str()));

        Self {
            id: edge.id.clone(),
            from_task,
            to_task,
            relation: edge.relation.clone(),
            created_at: edge.created_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use surrealdb::sql::Datetime;

    fn make_record(record_type: &str, name: &str, description: Option<&str>) -> Record {
        Record {
            id: None,
            record_type: record_type.to_string(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            content: serde_json::json!({}),
            deleted_at: None,
            superseded_by: None,
            superseded_via: None,
            created_at: Datetime::default(),
            updated_at: Datetime::default(),
        }
    }

    #[test]
    fn test_to_system_prompt_empty() {
        let assembly = ContextAssembly {
            records: vec![],
            traversal_path: vec![],
        };
        assert_eq!(assembly.to_system_prompt(), "");
    }

    #[test]
    fn test_to_system_prompt_rules_only() {
        let assembly = ContextAssembly {
            records: vec![
                make_record("rule", "No merge commits", Some("Always use squash or rebase")),
                make_record("rule", "Single-line commits", None),
            ],
            traversal_path: vec![],
        };

        let prompt = assembly.to_system_prompt();
        assert!(prompt.contains("# Context from Knowledge Base"));
        assert!(prompt.contains("## Rules"));
        assert!(prompt.contains("### No merge commits"));
        assert!(prompt.contains("Always use squash or rebase"));
        assert!(prompt.contains("### Single-line commits"));
        // Should not have other sections
        assert!(!prompt.contains("## Available Skills"));
        assert!(!prompt.contains("## Team"));
    }

    #[test]
    fn test_to_system_prompt_skills_only() {
        let assembly = ContextAssembly {
            records: vec![
                make_record("skill", "Git Operations", Some("Can perform git operations")),
            ],
            traversal_path: vec![],
        };

        let prompt = assembly.to_system_prompt();
        assert!(prompt.contains("## Available Skills"));
        assert!(prompt.contains("- **Git Operations**"));
        assert!(prompt.contains("Can perform git operations"));
        assert!(!prompt.contains("## Rules"));
    }

    #[test]
    fn test_to_system_prompt_people_only() {
        let assembly = ContextAssembly {
            records: vec![
                make_record("person", "Alice", Some("Lead developer")),
                make_record("person", "Bob", None),
            ],
            traversal_path: vec![],
        };

        let prompt = assembly.to_system_prompt();
        assert!(prompt.contains("## Team"));
        assert!(prompt.contains("- **Alice**"));
        assert!(prompt.contains("Lead developer"));
        assert!(prompt.contains("- **Bob**"));
    }

    #[test]
    fn test_to_system_prompt_mixed_types() {
        let assembly = ContextAssembly {
            records: vec![
                make_record("rule", "TDD Required", Some("Write tests first")),
                make_record("skill", "Rust Development", Some("Can write Rust code")),
                make_record("person", "Luke", None),
                make_record("repo", "memex", Some("Knowledge management system")), // Should be ignored
            ],
            traversal_path: vec![],
        };

        let prompt = assembly.to_system_prompt();
        assert!(prompt.contains("## Rules"));
        assert!(prompt.contains("### TDD Required"));
        assert!(prompt.contains("## Available Skills"));
        assert!(prompt.contains("- **Rust Development**"));
        assert!(prompt.contains("## Team"));
        assert!(prompt.contains("- **Luke**"));
        // Repo should not appear (not a recognized section)
        assert!(!prompt.contains("memex"));
    }

    #[test]
    fn test_records_of_type() {
        let assembly = ContextAssembly {
            records: vec![
                make_record("rule", "Rule 1", None),
                make_record("skill", "Skill 1", None),
                make_record("rule", "Rule 2", None),
            ],
            traversal_path: vec![],
        };

        let rules = assembly.rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "Rule 1");
        assert_eq!(rules[1].name, "Rule 2");

        let skills = assembly.skills();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "Skill 1");

        let people = assembly.people();
        assert_eq!(people.len(), 0);
    }

    #[test]
    fn test_impact_from_str() {
        use super::Impact;
        assert_eq!("critical".parse::<Impact>().unwrap(), Impact::Critical);
        assert_eq!("high".parse::<Impact>().unwrap(), Impact::High);
        assert_eq!("medium".parse::<Impact>().unwrap(), Impact::Medium);
        assert_eq!("low".parse::<Impact>().unwrap(), Impact::Low);
        assert_eq!("minimal".parse::<Impact>().unwrap(), Impact::Minimal);
        assert!("invalid".parse::<Impact>().is_err());
    }

    #[test]
    fn test_impact_from_priority() {
        use super::Impact;
        assert_eq!(Impact::from_priority(0), Impact::Critical);
        assert_eq!(Impact::from_priority(1), Impact::High);
        assert_eq!(Impact::from_priority(2), Impact::Medium);
        assert_eq!(Impact::from_priority(3), Impact::Low);
        assert_eq!(Impact::from_priority(4), Impact::Minimal);
        assert_eq!(Impact::from_priority(99), Impact::Minimal);
    }

    #[test]
    fn test_urgency_from_str() {
        use super::Urgency;
        assert_eq!("immediate".parse::<Urgency>().unwrap(), Urgency::Immediate);
        assert_eq!("soon".parse::<Urgency>().unwrap(), Urgency::Soon);
        assert_eq!("normal".parse::<Urgency>().unwrap(), Urgency::Normal);
        assert_eq!("later".parse::<Urgency>().unwrap(), Urgency::Later);
        assert_eq!("someday".parse::<Urgency>().unwrap(), Urgency::Someday);
        assert!("invalid".parse::<Urgency>().is_err());
    }

    #[test]
    fn test_calculate_priority() {
        use super::{calculate_priority, Impact, Urgency};

        // Critical + Immediate should be highest priority
        let high = calculate_priority(Impact::Critical, Urgency::Immediate);
        // Minimal + Someday should be lowest
        let low = calculate_priority(Impact::Minimal, Urgency::Someday);
        assert!(high > low);

        // Impact should matter more than urgency
        let high_impact_low_urgency = calculate_priority(Impact::Critical, Urgency::Someday);
        let low_impact_high_urgency = calculate_priority(Impact::Minimal, Urgency::Immediate);
        assert!(high_impact_low_urgency > low_impact_high_urgency);
    }

    #[test]
    fn test_goal_status_from_str() {
        use super::GoalStatus;
        assert_eq!("not_started".parse::<GoalStatus>().unwrap(), GoalStatus::NotStarted);
        assert_eq!("in_progress".parse::<GoalStatus>().unwrap(), GoalStatus::InProgress);
        assert_eq!("achieved".parse::<GoalStatus>().unwrap(), GoalStatus::Achieved);
        assert_eq!("abandoned".parse::<GoalStatus>().unwrap(), GoalStatus::Abandoned);
        assert!("invalid".parse::<GoalStatus>().is_err());
    }

    #[test]
    fn test_goal_content_builder() {
        use super::{GoalContent, GoalStatus, Impact, Urgency};

        let content = GoalContent::new()
            .with_impact(Impact::High)
            .with_urgency(Urgency::Soon)
            .with_status(GoalStatus::InProgress);

        assert_eq!(content.impact, Impact::High);
        assert_eq!(content.urgency, Urgency::Soon);
        assert_eq!(content.status, GoalStatus::InProgress);
    }

    #[test]
    fn test_goal_content_json_roundtrip() {
        use super::{GoalContent, GoalStatus, Impact, Urgency};

        let content = GoalContent::new()
            .with_impact(Impact::Critical)
            .with_urgency(Urgency::Immediate);

        let json = content.to_json();
        let parsed = GoalContent::from_json(&json).unwrap();

        assert_eq!(parsed.impact, content.impact);
        assert_eq!(parsed.urgency, content.urgency);
        assert_eq!(parsed.status, content.status);
    }

    #[test]
    fn test_task_content_with_impact_urgency() {
        use super::{Impact, TaskContent, Urgency};

        let content = TaskContent::new()
            .with_impact(Impact::High)
            .with_urgency(Urgency::Soon);

        assert_eq!(content.impact, Impact::High);
        assert_eq!(content.urgency, Urgency::Soon);
        assert!(content.effective_priority() > 0);
    }

    #[test]
    fn test_personal_ordering() {
        use super::{Impact, PersonalOrderingContent, Urgency};

        let ordering = PersonalOrderingContent::new("user123")
            .with_priority("task1")
            .with_priority("task2")
            .with_impact_override("task3", Impact::Critical);

        assert_eq!(ordering.person_id, "user123");
        assert_eq!(ordering.priority_order.len(), 2);

        // Task1 should have higher effective priority than task2 due to position
        let prio1 = ordering.effective_priority_for("task1", Impact::Medium, Urgency::Normal);
        let prio2 = ordering.effective_priority_for("task2", Impact::Medium, Urgency::Normal);
        assert!(prio1 > prio2);

        // Task3 should have high priority due to impact override
        let prio3 = ordering.effective_priority_for("task3", Impact::Low, Urgency::Normal);
        let prio_no_override = ordering.effective_priority_for("task4", Impact::Low, Urgency::Normal);
        assert!(prio3 > prio_no_override);
    }
}
