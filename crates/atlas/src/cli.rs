//! CLI command definitions for Atlas memo, event, and record management
//!
//! This module defines the clap subcommands that can be imported
//! by the main CLI crate.

use clap::{Subcommand, ValueEnum};

/// Memo management commands
#[derive(Debug, Subcommand)]
pub enum MemoCommand {
    /// List memos
    List {
        /// Maximum number of memos to show
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Show memo details
    Get {
        /// Memo ID
        id: String,
    },

    /// Delete a memo
    Delete {
        /// Memo ID
        id: String,
    },
}

/// Event management commands
#[derive(Debug, Subcommand)]
pub enum EventCommand {
    /// List events
    List {
        /// Filter by event type prefix (e.g., "task" or "task.created")
        #[arg(short = 't', long)]
        event_type: Option<String>,

        /// Maximum number of events to show
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Show event details
    Get {
        /// Event ID
        id: String,
    },
}

/// Knowledge discovery commands
#[derive(Debug, Subcommand)]
pub enum KnowledgeCommand {
    /// Query knowledge and get an LLM-summarized response
    ///
    /// Searches extracted facts and uses an LLM to summarize
    /// the results into a natural language response.
    Query {
        /// The query to answer
        query: String,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,

        /// Maximum number of facts to consider
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Search for raw facts matching a query
    ///
    /// Returns the raw extracted facts without LLM processing.
    /// Use this when you want to see the underlying data.
    Search {
        /// The search query
        query: String,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,

        /// Maximum number of results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Extract facts from existing memos
    ///
    /// This is useful for backfilling facts from memos recorded
    /// before extraction was enabled.
    Extract {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,

        /// Maximum number of memos to process
        #[arg(short, long, default_value = "20")]
        batch_size: usize,
    },

    /// Rebuild the knowledge graph from scratch
    ///
    /// Deletes all derived data (facts, entities) and re-extracts
    /// from all memos. Use this after schema changes or to regenerate
    /// with improved extraction logic.
    Rebuild {
        /// Filter by project (only rebuild facts for this project)
        #[arg(short, long)]
        project: Option<String>,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show knowledge system status
    ///
    /// Displays statistics about facts and embeddings, useful for
    /// diagnosing search issues.
    Status,

    /// List known entities
    ///
    /// Shows entities (projects, people, technologies, concepts) that have
    /// been extracted from memos.
    Entities {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,

        /// Filter by entity type (project, person, technology, concept, task, document)
        #[arg(short = 't', long)]
        entity_type: Option<String>,

        /// Maximum number of entities to show
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Get facts about a specific entity
    ///
    /// Shows all facts that reference the named entity.
    Entity {
        /// Entity name to look up
        name: String,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Extract records from a specific memo (test command)
    ///
    /// Uses the new Records + Links extraction pipeline. This is useful
    /// for testing extraction on individual memos before running backfill.
    ExtractRecords {
        /// Memo ID to extract from
        memo_id: String,

        /// Confidence threshold (0.0-1.0, default 0.5)
        #[arg(short, long, default_value = "0.5")]
        threshold: f32,

        /// Dry run - show what would be extracted without creating records
        #[arg(short, long)]
        dry_run: bool,

        /// Use multi-step extraction (entity → match → decide) for better updates
        #[arg(short, long)]
        multi_step: bool,
    },

    /// Backfill records from all memos
    ///
    /// Processes all memos through the Records + Links extraction pipeline.
    /// Creates records, links, and clarification tasks for questions.
    BackfillRecords {
        /// Maximum number of memos to process
        #[arg(short, long, default_value = "50")]
        batch_size: usize,

        /// Confidence threshold (0.0-1.0, default 0.5)
        #[arg(short = 't', long, default_value = "0.5")]
        threshold: f32,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

/// Record type for CLI
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RecordTypeCli {
    Repo,
    Team,
    Person,
    Company,
    Initiative,
    Rule,
    Skill,
    Document,
    Task,
    Technology,
}

impl std::fmt::Display for RecordTypeCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordTypeCli::Repo => write!(f, "repo"),
            RecordTypeCli::Team => write!(f, "team"),
            RecordTypeCli::Person => write!(f, "person"),
            RecordTypeCli::Company => write!(f, "company"),
            RecordTypeCli::Initiative => write!(f, "initiative"),
            RecordTypeCli::Rule => write!(f, "rule"),
            RecordTypeCli::Skill => write!(f, "skill"),
            RecordTypeCli::Document => write!(f, "document"),
            RecordTypeCli::Task => write!(f, "task"),
            RecordTypeCli::Technology => write!(f, "technology"),
        }
    }
}

/// Edge relation for CLI
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EdgeRelationCli {
    AppliesTo,
    BelongsTo,
    MemberOf,
    Owns,
    AvailableTo,
    DependsOn,
    RelatedTo,
}

impl std::fmt::Display for EdgeRelationCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeRelationCli::AppliesTo => write!(f, "applies_to"),
            EdgeRelationCli::BelongsTo => write!(f, "belongs_to"),
            EdgeRelationCli::MemberOf => write!(f, "member_of"),
            EdgeRelationCli::Owns => write!(f, "owns"),
            EdgeRelationCli::AvailableTo => write!(f, "available_to"),
            EdgeRelationCli::DependsOn => write!(f, "depends_on"),
            EdgeRelationCli::RelatedTo => write!(f, "related_to"),
        }
    }
}

/// Record management commands
#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    /// List records
    List {
        /// Filter by record type
        #[arg(short = 't', long, value_enum)]
        record_type: Option<RecordTypeCli>,

        /// Include soft-deleted records
        #[arg(short = 'd', long)]
        include_deleted: bool,

        /// Maximum number of records to show
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Get a record by ID
    Get {
        /// Record ID
        id: String,
    },

    /// Create a new record
    Create {
        /// Record type
        #[arg(value_enum)]
        record_type: RecordTypeCli,

        /// Record name
        name: String,

        /// Description
        #[arg(short, long)]
        description: Option<String>,

        /// JSON content (type-specific fields)
        #[arg(short, long)]
        content: Option<String>,
    },

    /// Update a record
    Update {
        /// Record ID
        id: String,

        /// New name
        #[arg(short, long)]
        name: Option<String>,

        /// New description
        #[arg(short, long)]
        description: Option<String>,

        /// JSON content to merge
        #[arg(short, long)]
        content: Option<String>,
    },

    /// Delete a record (soft-delete)
    Delete {
        /// Record ID
        id: String,
    },

    /// Edge management
    Edge {
        #[command(subcommand)]
        action: EdgeCommand,
    },

    /// Assemble context from a record
    ///
    /// Starting from a record (e.g., a repo), traverses edges to collect
    /// related records like rules, skills, team members, etc.
    Context {
        /// Record ID to start from
        id: String,

        /// Maximum traversal depth
        #[arg(short, long, default_value = "3")]
        depth: usize,
    },
}

/// Edge management commands
#[derive(Debug, Subcommand)]
pub enum EdgeCommand {
    /// Create an edge between records
    Add {
        /// Source record ID
        source: String,

        /// Target record ID
        target: String,

        /// Relationship type
        #[arg(value_enum)]
        relation: EdgeRelationCli,

        /// Optional JSON metadata
        #[arg(short, long)]
        metadata: Option<String>,
    },

    /// List edges from/to a record
    List {
        /// Record ID
        id: String,

        /// Direction: "from" (outgoing), "to" (incoming), or "both"
        #[arg(short, long, default_value = "both")]
        direction: String,
    },

    /// Delete an edge
    Delete {
        /// Edge ID
        id: String,
    },
}
