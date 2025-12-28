//! CLI command definitions for Atlas memo management
//!
//! This module defines the clap subcommands that can be imported
//! by the main CLI crate.

use clap::Subcommand;

/// Memo management commands
#[derive(Debug, Subcommand)]
pub enum MemoCommand {
    /// Record a new memo
    Record {
        /// Memo content
        content: String,
    },

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
