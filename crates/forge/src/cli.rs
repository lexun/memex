//! CLI command definitions for Forge task management
//!
//! This module defines the clap subcommands that can be imported
//! by the main CLI crate.

use clap::Subcommand;

/// Task management commands
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Create a new task
    Create {
        /// Task title
        title: String,
        /// Task description
        #[arg(short, long)]
        description: Option<String>,
        /// Project name
        #[arg(short, long)]
        project: Option<String>,
        /// Priority (higher = more important)
        #[arg(long, default_value = "0")]
        priority: i32,
    },

    /// List tasks
    List {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
        /// Filter by status (pending, in_progress, blocked, completed, cancelled)
        #[arg(short, long)]
        status: Option<String>,
    },

    /// Show tasks ready to work on (pending with no blockers)
    Ready {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Show task details
    Get {
        /// Task ID
        id: String,
    },

    /// Update a task
    Update {
        /// Task ID
        id: String,
        /// New status
        #[arg(short, long)]
        status: Option<String>,
        /// New priority
        #[arg(short, long)]
        priority: Option<i32>,
    },

    /// Close/complete a task
    Close {
        /// Task ID
        id: String,
        /// Reason for closing
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// Delete a task
    Delete {
        /// Task ID
        id: String,
    },

    /// Manage task updates/notes
    #[command(subcommand)]
    Note(NoteCommand),

    /// Manage task dependencies
    #[command(subcommand)]
    Dep(DepCommand),
}

/// Commands for task notes/updates
#[derive(Debug, Subcommand)]
pub enum NoteCommand {
    /// Add a note to a task
    Add {
        /// Task ID
        task_id: String,
        /// Note content
        content: String,
    },

    /// Edit an existing note
    Edit {
        /// Note ID
        note_id: String,
        /// New content
        content: String,
    },

    /// Delete a note
    Delete {
        /// Note ID
        note_id: String,
    },
}

/// Commands for task dependencies
#[derive(Debug, Subcommand)]
pub enum DepCommand {
    /// Add a dependency between tasks
    Add {
        /// The task that depends on another
        from: String,
        /// The task being depended on
        to: String,
        /// Relationship type (blocks, blocked_by, relates_to)
        #[arg(short, long, default_value = "blocks")]
        relation: String,
    },

    /// Remove a dependency
    Remove {
        /// The task that depends on another
        from: String,
        /// The task being depended on
        to: String,
        /// Relationship type
        #[arg(short, long, default_value = "blocks")]
        relation: String,
    },

    /// Show dependencies for a task
    Show {
        /// Task ID
        task_id: String,
    },
}
