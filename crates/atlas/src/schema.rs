//! Flexible record schema prototype for Atlas
//!
//! This module provides a flexible type system for records that separates
//! two orthogonal dimensions:
//!
//! 1. **Kind** (functional/workflow): How the record behaves
//!    - Document: Passive descriptive content (wiki-like)
//!    - Task: Active workflow with lifecycle states
//!    - Rule: Prescriptive guidance applied during context assembly
//!    - Skill: Executable agent capability
//!
//! 2. **Type** (semantic/domain): What the record represents
//!    - Built-in types: person, company, team, repo, technology, project
//!    - User-defined types: any string (flexible extension)
//!
//! This design allows:
//! - A Person record (kind=document, type=person)
//! - A Person task (kind=task, type=person) - e.g., "Onboard new team member"
//! - User-defined types like "design_system" or "api_endpoint"
//!
//! ## Schema Definitions
//!
//! Types can optionally have schema definitions stored as records themselves,
//! enabling validation and structured content without hardcoding types in Rust.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use surrealdb::sql::{Datetime, Thing};

// =============================================================================
// Record Kind: The functional dimension (how it behaves)
// =============================================================================

/// Record kind - the functional/workflow dimension
///
/// Kind determines how the record behaves in the system:
/// - What lifecycle states it has (if any)
/// - How it appears in context assembly
/// - What operations are available
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    /// Passive descriptive content (wiki-like)
    /// Examples: Person profiles, Company info, Project documentation
    /// Behavior: No lifecycle, appears in context as reference material
    #[default]
    Document,

    /// Active workflow with lifecycle states
    /// Examples: Tasks, Issues, Action items
    /// Behavior: Has status (pending/in_progress/completed), priority, dependencies
    Task,

    /// Prescriptive guidance applied during context assembly
    /// Examples: Coding standards, Git workflow rules, Project guidelines
    /// Behavior: Injected into agent system prompts when context is assembled
    Rule,

    /// Executable agent capability
    /// Examples: Git operations, Code review, Testing
    /// Behavior: Listed as available actions for agents
    Skill,

    /// Goal/objective that contains sub-goals and tasks
    /// Examples: Quarterly OKRs, Feature epics, Project milestones
    /// Behavior: Forms hierarchy with part_of edges, tracks completion
    Goal,

    /// Communication between agents or users
    /// Examples: Status updates, Requests, Notifications
    /// Behavior: Has read/unread state, sender/recipient
    Message,

    /// Conversation transcript
    /// Examples: Claude Code sessions, API conversations
    /// Behavior: Contains entries, has session metadata
    Thread,

    /// Single entry within a thread
    /// Examples: User message, Assistant response
    /// Behavior: Has role, turn number, linked to thread
    Entry,

    /// Configuration record
    /// Examples: MCP server configs, API keys, Settings
    /// Behavior: Used for system configuration, not displayed to agents
    Config,
}

impl std::fmt::Display for RecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordKind::Document => write!(f, "document"),
            RecordKind::Task => write!(f, "task"),
            RecordKind::Rule => write!(f, "rule"),
            RecordKind::Skill => write!(f, "skill"),
            RecordKind::Goal => write!(f, "goal"),
            RecordKind::Message => write!(f, "message"),
            RecordKind::Thread => write!(f, "thread"),
            RecordKind::Entry => write!(f, "entry"),
            RecordKind::Config => write!(f, "config"),
        }
    }
}

impl std::str::FromStr for RecordKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "document" => Ok(RecordKind::Document),
            "task" => Ok(RecordKind::Task),
            "rule" => Ok(RecordKind::Rule),
            "skill" => Ok(RecordKind::Skill),
            "goal" => Ok(RecordKind::Goal),
            "message" => Ok(RecordKind::Message),
            "thread" => Ok(RecordKind::Thread),
            "entry" => Ok(RecordKind::Entry),
            "config" => Ok(RecordKind::Config),
            _ => Err(format!("Unknown record kind: {}", s)),
        }
    }
}

// =============================================================================
// Record Type: The semantic dimension (what it represents)
// =============================================================================

/// Built-in semantic types (optional - any string is valid)
///
/// These are common types with well-known semantics. Users can create
/// records with any type string, but these have special handling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinType {
    /// A person (developer, user, stakeholder)
    Person,
    /// A company or organization
    Company,
    /// A team of people
    Team,
    /// A code repository
    Repo,
    /// A technology, tool, framework, or service
    Technology,
    /// A project or initiative
    Project,
    /// Generic/untyped
    Generic,
}

impl BuiltinType {
    /// Check if a type string matches a built-in type
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "person" => Some(BuiltinType::Person),
            "company" => Some(BuiltinType::Company),
            "team" => Some(BuiltinType::Team),
            "repo" | "repository" => Some(BuiltinType::Repo),
            "technology" | "tech" => Some(BuiltinType::Technology),
            "project" => Some(BuiltinType::Project),
            "generic" | "" => Some(BuiltinType::Generic),
            _ => None,
        }
    }

    /// Get the canonical string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            BuiltinType::Person => "person",
            BuiltinType::Company => "company",
            BuiltinType::Team => "team",
            BuiltinType::Repo => "repo",
            BuiltinType::Technology => "technology",
            BuiltinType::Project => "project",
            BuiltinType::Generic => "generic",
        }
    }
}

impl Default for BuiltinType {
    fn default() -> Self {
        BuiltinType::Generic
    }
}

impl std::fmt::Display for BuiltinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// Semantic Type wrapper (built-in or custom)
// =============================================================================

/// A semantic type - either built-in or user-defined
///
/// This allows the flexibility of custom types while providing
/// special handling for well-known types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SemanticType {
    /// A built-in type with special handling
    Builtin(BuiltinType),
    /// A user-defined type (any string)
    Custom(String),
}

impl SemanticType {
    /// Create a semantic type from a string
    pub fn from_string(s: impl Into<String>) -> Self {
        let s = s.into();
        match BuiltinType::from_str_opt(&s) {
            Some(builtin) => SemanticType::Builtin(builtin),
            None => SemanticType::Custom(s),
        }
    }

    /// Get the string representation
    pub fn as_str(&self) -> &str {
        match self {
            SemanticType::Builtin(b) => b.as_str(),
            SemanticType::Custom(s) => s.as_str(),
        }
    }

    /// Check if this is a built-in type
    pub fn is_builtin(&self) -> bool {
        matches!(self, SemanticType::Builtin(_))
    }
}

impl Default for SemanticType {
    fn default() -> Self {
        SemanticType::Builtin(BuiltinType::default())
    }
}

impl std::fmt::Display for SemanticType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<&str> for SemanticType {
    fn from(s: &str) -> Self {
        SemanticType::from_string(s)
    }
}

impl From<String> for SemanticType {
    fn from(s: String) -> Self {
        SemanticType::from_string(s)
    }
}

// =============================================================================
// Flexible Record: The new record structure
// =============================================================================

/// A flexible record with separated kind and type dimensions
///
/// This is the prototype for evolving the current Record struct.
/// It separates functional behavior (kind) from semantic category (type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexRecord {
    /// Unique identifier (set by database)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Thing>,

    /// Functional kind - how the record behaves
    pub kind: RecordKind,

    /// Semantic type - what the record represents
    /// Can be a built-in type or any custom string
    #[serde(rename = "type")]
    pub semantic_type: String,

    /// Human-readable name
    pub name: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Flexible content for type-specific fields
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

impl FlexRecord {
    /// Create a new flexible record
    pub fn new(kind: RecordKind, semantic_type: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Datetime::default();
        Self {
            id: None,
            kind,
            semantic_type: semantic_type.into(),
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

    /// Create a document record (most common case)
    pub fn document(semantic_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self::new(RecordKind::Document, semantic_type, name)
    }

    /// Create a task record
    pub fn task(name: impl Into<String>) -> Self {
        Self::new(RecordKind::Task, "generic", name)
    }

    /// Create a rule record
    pub fn rule(name: impl Into<String>) -> Self {
        Self::new(RecordKind::Rule, "generic", name)
    }

    /// Create a skill record
    pub fn skill(name: impl Into<String>) -> Self {
        Self::new(RecordKind::Skill, "generic", name)
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

    /// Get the semantic type as a SemanticType enum
    pub fn get_semantic_type(&self) -> SemanticType {
        SemanticType::from_string(&self.semantic_type)
    }

    /// Convert to the legacy record_type string format
    ///
    /// For backward compatibility with existing code that uses
    /// the combined record_type field.
    pub fn to_legacy_record_type(&self) -> String {
        // For tasks/rules/skills, use the kind as the type
        // For documents, use the semantic type
        match self.kind {
            RecordKind::Task => "task".to_string(),
            RecordKind::Rule => "rule".to_string(),
            RecordKind::Skill => "skill".to_string(),
            RecordKind::Goal => "goal".to_string(),
            RecordKind::Message => "message".to_string(),
            RecordKind::Thread => "thread".to_string(),
            RecordKind::Entry => "entry".to_string(),
            RecordKind::Config => self.semantic_type.clone(),
            RecordKind::Document => self.semantic_type.clone(),
        }
    }
}

// =============================================================================
// Type Schema: Optional schema definitions for types
// =============================================================================

/// Field type for schema validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Number,
    Boolean,
    Date,
    DateTime,
    Url,
    Email,
    RecordRef,  // Reference to another record
    Array(Box<FieldType>),
    Object,
    Any,
}

impl Default for FieldType {
    fn default() -> Self {
        FieldType::Any
    }
}

/// A field definition in a type schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    /// Field name (key in content object)
    pub name: String,

    /// Display label for UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Field type
    #[serde(default)]
    pub field_type: FieldType,

    /// Whether this field is required
    #[serde(default)]
    pub required: bool,

    /// Default value (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<JsonValue>,

    /// Description for documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// For RecordRef: which record types are allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_types: Option<Vec<String>>,
}

impl FieldDef {
    /// Create a new field definition
    pub fn new(name: impl Into<String>, field_type: FieldType) -> Self {
        Self {
            name: name.into(),
            label: None,
            field_type,
            required: false,
            default: None,
            description: None,
            ref_types: None,
        }
    }

    /// Mark as required
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Set a default value
    pub fn with_default(mut self, default: JsonValue) -> Self {
        self.default = Some(default);
        self
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// A type schema definition
///
/// Type schemas are stored as records (kind=config, type=type_schema)
/// and define the expected structure of content for a given type.
///
/// Schemas are optional - records can have any content structure.
/// When a schema exists, it enables:
/// - UI form generation
/// - Validation
/// - Documentation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeSchema {
    /// The type this schema defines (e.g., "person", "api_endpoint")
    pub for_type: String,

    /// Which kinds this schema applies to (empty = all)
    #[serde(default)]
    pub for_kinds: Vec<RecordKind>,

    /// Display name for the type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Description of this type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Field definitions
    #[serde(default)]
    pub fields: Vec<FieldDef>,

    /// Icon name (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Color (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl TypeSchema {
    /// Create a new type schema
    pub fn new(for_type: impl Into<String>) -> Self {
        Self {
            for_type: for_type.into(),
            for_kinds: Vec::new(),
            display_name: None,
            description: None,
            fields: Vec::new(),
            icon: None,
            color: None,
        }
    }

    /// Add a field definition
    pub fn with_field(mut self, field: FieldDef) -> Self {
        self.fields.push(field);
        self
    }

    /// Set display name
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Restrict to specific kinds
    pub fn for_kinds(mut self, kinds: Vec<RecordKind>) -> Self {
        self.for_kinds = kinds;
        self
    }

    /// Validate content against this schema
    pub fn validate(&self, content: &JsonValue) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        let obj = match content.as_object() {
            Some(o) => o,
            None => {
                errors.push(ValidationError {
                    field: None,
                    message: "Content must be an object".to_string(),
                });
                return errors;
            }
        };

        for field in &self.fields {
            let value = obj.get(&field.name);

            // Check required
            if field.required && value.is_none() {
                errors.push(ValidationError {
                    field: Some(field.name.clone()),
                    message: format!("Required field '{}' is missing", field.name),
                });
                continue;
            }

            // Type checking (basic)
            if let Some(v) = value {
                if !self.check_type(v, &field.field_type) {
                    errors.push(ValidationError {
                        field: Some(field.name.clone()),
                        message: format!(
                            "Field '{}' has wrong type, expected {:?}",
                            field.name, field.field_type
                        ),
                    });
                }
            }
        }

        errors
    }

    fn check_type(&self, value: &JsonValue, expected: &FieldType) -> bool {
        match expected {
            FieldType::String | FieldType::Url | FieldType::Email | FieldType::Date | FieldType::DateTime => {
                value.is_string()
            }
            FieldType::Number => value.is_number(),
            FieldType::Boolean => value.is_boolean(),
            FieldType::Object | FieldType::RecordRef => value.is_object() || value.is_string(),
            FieldType::Array(inner) => {
                if let Some(arr) = value.as_array() {
                    arr.iter().all(|v| self.check_type(v, inner))
                } else {
                    false
                }
            }
            FieldType::Any => true,
        }
    }

    /// Convert to JSON for storage in Record.content
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Parse from JSON
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// A validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: Option<String>,
    pub message: String,
}

// =============================================================================
// Migration helpers: Converting between old and new formats
// =============================================================================

/// Convert an old-style record_type to (kind, type) pair
///
/// This handles the existing RecordType enum values by mapping them
/// to the new two-dimensional system.
pub fn parse_legacy_record_type(record_type: &str) -> (RecordKind, String) {
    match record_type.to_lowercase().as_str() {
        // Workflow types -> specific kind, generic semantic type
        "task" => (RecordKind::Task, "generic".to_string()),
        "goal" => (RecordKind::Goal, "generic".to_string()),
        "rule" => (RecordKind::Rule, "generic".to_string()),
        "skill" => (RecordKind::Skill, "generic".to_string()),
        "message" => (RecordKind::Message, "generic".to_string()),
        "thread" => (RecordKind::Thread, "generic".to_string()),
        "entry" => (RecordKind::Entry, "generic".to_string()),

        // Config types
        "mcp_server" => (RecordKind::Config, "mcp_server".to_string()),

        // Entity types -> document kind, specific semantic type
        "repo" => (RecordKind::Document, "repo".to_string()),
        "team" => (RecordKind::Document, "team".to_string()),
        "person" => (RecordKind::Document, "person".to_string()),
        "company" => (RecordKind::Document, "company".to_string()),
        "initiative" => (RecordKind::Document, "project".to_string()),
        "technology" => (RecordKind::Document, "technology".to_string()),
        "document" => (RecordKind::Document, "generic".to_string()),
        "project" => (RecordKind::Document, "project".to_string()),

        // Unknown -> document with the type as-is
        other => (RecordKind::Document, other.to_string()),
    }
}

/// Convert a (kind, type) pair back to legacy record_type
///
/// For backward compatibility with existing queries and APIs.
pub fn to_legacy_record_type(kind: RecordKind, semantic_type: &str) -> String {
    match kind {
        RecordKind::Task => "task".to_string(),
        RecordKind::Goal => "goal".to_string(),
        RecordKind::Rule => "rule".to_string(),
        RecordKind::Skill => "skill".to_string(),
        RecordKind::Message => "message".to_string(),
        RecordKind::Thread => "thread".to_string(),
        RecordKind::Entry => "entry".to_string(),
        RecordKind::Config | RecordKind::Document => semantic_type.to_string(),
    }
}

// =============================================================================
// Example type schemas
// =============================================================================

/// Create the built-in type schemas
pub fn builtin_schemas() -> HashMap<String, TypeSchema> {
    let mut schemas = HashMap::new();

    // Person schema
    schemas.insert(
        "person".to_string(),
        TypeSchema::new("person")
            .with_display_name("Person")
            .for_kinds(vec![RecordKind::Document])
            .with_field(FieldDef::new("email", FieldType::Email))
            .with_field(FieldDef::new("role", FieldType::String))
            .with_field(FieldDef::new("github", FieldType::String))
            .with_field(FieldDef::new("team", FieldType::RecordRef)),
    );

    // Repo schema
    schemas.insert(
        "repo".to_string(),
        TypeSchema::new("repo")
            .with_display_name("Repository")
            .for_kinds(vec![RecordKind::Document])
            .with_field(FieldDef::new("path", FieldType::String).required())
            .with_field(FieldDef::new("default_branch", FieldType::String).with_default(serde_json::json!("main")))
            .with_field(FieldDef::new("languages", FieldType::Array(Box::new(FieldType::String))))
            .with_field(FieldDef::new("remote_url", FieldType::Url)),
    );

    // Technology schema
    schemas.insert(
        "technology".to_string(),
        TypeSchema::new("technology")
            .with_display_name("Technology")
            .for_kinds(vec![RecordKind::Document])
            .with_field(FieldDef::new("category", FieldType::String))
            .with_field(FieldDef::new("version", FieldType::String))
            .with_field(FieldDef::new("website", FieldType::Url))
            .with_field(FieldDef::new("docs_url", FieldType::Url)),
    );

    // MCP Server schema
    schemas.insert(
        "mcp_server".to_string(),
        TypeSchema::new("mcp_server")
            .with_display_name("MCP Server")
            .for_kinds(vec![RecordKind::Config])
            .with_field(FieldDef::new("command", FieldType::String).required())
            .with_field(FieldDef::new("args", FieldType::Array(Box::new(FieldType::String))))
            .with_field(FieldDef::new("env", FieldType::Object)),
    );

    schemas
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_kind_from_str() {
        assert_eq!("task".parse::<RecordKind>().unwrap(), RecordKind::Task);
        assert_eq!("document".parse::<RecordKind>().unwrap(), RecordKind::Document);
        assert_eq!("RULE".parse::<RecordKind>().unwrap(), RecordKind::Rule);
        assert!("invalid".parse::<RecordKind>().is_err());
    }

    #[test]
    fn test_semantic_type_from_string() {
        let person = SemanticType::from_string("person");
        assert!(person.is_builtin());
        assert_eq!(person.as_str(), "person");

        let custom = SemanticType::from_string("my_custom_type");
        assert!(!custom.is_builtin());
        assert_eq!(custom.as_str(), "my_custom_type");
    }

    #[test]
    fn test_parse_legacy_record_type() {
        let (kind, stype) = parse_legacy_record_type("task");
        assert_eq!(kind, RecordKind::Task);
        assert_eq!(stype, "generic");

        let (kind, stype) = parse_legacy_record_type("person");
        assert_eq!(kind, RecordKind::Document);
        assert_eq!(stype, "person");

        let (kind, stype) = parse_legacy_record_type("mcp_server");
        assert_eq!(kind, RecordKind::Config);
        assert_eq!(stype, "mcp_server");
    }

    #[test]
    fn test_to_legacy_record_type() {
        assert_eq!(to_legacy_record_type(RecordKind::Task, "generic"), "task");
        assert_eq!(to_legacy_record_type(RecordKind::Document, "person"), "person");
        assert_eq!(to_legacy_record_type(RecordKind::Config, "mcp_server"), "mcp_server");
    }

    #[test]
    fn test_flex_record_creation() {
        let record = FlexRecord::document("person", "Alice")
            .with_description("A developer")
            .with_content(serde_json::json!({"email": "alice@example.com"}));

        assert_eq!(record.kind, RecordKind::Document);
        assert_eq!(record.semantic_type, "person");
        assert_eq!(record.name, "Alice");
        assert_eq!(record.to_legacy_record_type(), "person");
    }

    #[test]
    fn test_flex_record_task() {
        let record = FlexRecord::task("Implement feature X");

        assert_eq!(record.kind, RecordKind::Task);
        assert_eq!(record.semantic_type, "generic");
        assert_eq!(record.to_legacy_record_type(), "task");
    }

    #[test]
    fn test_type_schema_validation() {
        let schema = TypeSchema::new("test")
            .with_field(FieldDef::new("required_field", FieldType::String).required())
            .with_field(FieldDef::new("optional_field", FieldType::Number));

        // Valid content
        let content = serde_json::json!({
            "required_field": "hello",
            "optional_field": 42
        });
        let errors = schema.validate(&content);
        assert!(errors.is_empty());

        // Missing required field
        let content = serde_json::json!({
            "optional_field": 42
        });
        let errors = schema.validate(&content);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("required_field"));

        // Wrong type
        let content = serde_json::json!({
            "required_field": "hello",
            "optional_field": "not a number"
        });
        let errors = schema.validate(&content);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("optional_field"));
    }

    #[test]
    fn test_builtin_schemas() {
        let schemas = builtin_schemas();

        assert!(schemas.contains_key("person"));
        assert!(schemas.contains_key("repo"));
        assert!(schemas.contains_key("technology"));
        assert!(schemas.contains_key("mcp_server"));

        // Test person schema
        let person_schema = &schemas["person"];
        assert_eq!(person_schema.for_type, "person");
        assert!(person_schema.fields.iter().any(|f| f.name == "email"));
    }
}
