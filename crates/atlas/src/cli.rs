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

/// Context discovery commands
#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Search for facts matching a query
    ///
    /// Facts are automatically extracted from memos and events.
    /// This searches the extracted knowledge, not raw episodes.
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
}
