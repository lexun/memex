//! CLI command definitions for Atlas memo and event management
//!
//! This module defines the clap subcommands that can be imported
//! by the main CLI crate.

use clap::Subcommand;

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
}
