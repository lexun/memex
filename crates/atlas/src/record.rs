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
    /// Content fields: { status, priority, project, completed_at }
    Task,
    /// An agent message - communication between agents (Coordinator, workers, Primary Claude)
    /// Content fields: { message_type, read, read_at }
    Message,
    /// An MCP (Model Context Protocol) server configuration
    /// Content fields: { command, args, env }
    McpServer,
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
            RecordType::Message => write!(f, "message"),
            RecordType::McpServer => write!(f, "mcp_server"),
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
            "message" => Ok(RecordType::Message),
            "mcp_server" => Ok(RecordType::McpServer),
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContent {
    /// Current workflow status
    pub status: TaskStatus,
    /// Priority (lower = higher priority: 0=critical, 1=high, 2=medium, 3=low)
    pub priority: i32,
    /// Optional project grouping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// When the task was completed (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<Datetime>,
}

impl Default for TaskContent {
    fn default() -> Self {
        Self {
            status: TaskStatus::default(),
            priority: 2, // medium
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

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
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
}
