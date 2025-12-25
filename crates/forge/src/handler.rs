//! Command handlers for Forge CLI
//!
//! These handlers execute the task management commands.
//! Currently prints placeholders - storage will be added later.

use anyhow::Result;

use crate::cli::{DepCommand, NoteCommand, TaskCommand};
use crate::task::Task;

/// Handle a task command
pub fn handle_task_command(cmd: TaskCommand) -> Result<()> {
    match cmd {
        TaskCommand::Create {
            title,
            description,
            project,
            priority,
        } => {
            let mut task = Task::new(&title).with_priority(priority);
            if let Some(desc) = description {
                task = task.with_description(desc);
            }
            if let Some(proj) = project {
                task = task.with_project(proj);
            }

            // TODO: Store task
            println!("Created task: {}", task.id);
            println!("  Title: {}", task.title);
            if let Some(ref desc) = task.description {
                println!("  Description: {}", desc);
            }
            if let Some(ref proj) = task.project {
                println!("  Project: {}", proj);
            }
            println!("  Priority: {}", task.priority);
            Ok(())
        }

        TaskCommand::List { project, status } => {
            // TODO: Query storage
            println!("Listing tasks...");
            if let Some(proj) = project {
                println!("  Project filter: {}", proj);
            }
            if let Some(stat) = status {
                println!("  Status filter: {}", stat);
            }
            println!("  (no tasks yet - storage not implemented)");
            Ok(())
        }

        TaskCommand::Ready { project } => {
            // TODO: Query storage for pending tasks with no blockers
            println!("Ready tasks...");
            if let Some(proj) = project {
                println!("  Project filter: {}", proj);
            }
            println!("  (no tasks yet - storage not implemented)");
            Ok(())
        }

        TaskCommand::Get { id } => {
            // TODO: Fetch from storage
            println!("Task: {}", id);
            println!("  (storage not implemented)");
            Ok(())
        }

        TaskCommand::Update {
            id,
            status,
            priority,
        } => {
            // TODO: Update in storage
            println!("Updating task: {}", id);
            if let Some(stat) = status {
                println!("  Status -> {}", stat);
            }
            if let Some(prio) = priority {
                println!("  Priority -> {}", prio);
            }
            Ok(())
        }

        TaskCommand::Close { id, reason } => {
            // TODO: Update status to completed in storage
            println!("Closing task: {}", id);
            if let Some(r) = reason {
                println!("  Reason: {}", r);
            }
            Ok(())
        }

        TaskCommand::Delete { id } => {
            // TODO: Delete from storage
            println!("Deleting task: {}", id);
            Ok(())
        }

        TaskCommand::Note(note_cmd) => handle_note_command(note_cmd),
        TaskCommand::Dep(dep_cmd) => handle_dep_command(dep_cmd),
    }
}

fn handle_note_command(cmd: NoteCommand) -> Result<()> {
    match cmd {
        NoteCommand::Add { task_id, content } => {
            println!("Adding note to task: {}", task_id);
            println!("  Content: {}", content);
            Ok(())
        }
        NoteCommand::Edit { note_id, content } => {
            println!("Editing note: {}", note_id);
            println!("  New content: {}", content);
            Ok(())
        }
        NoteCommand::Delete { note_id } => {
            println!("Deleting note: {}", note_id);
            Ok(())
        }
    }
}

fn handle_dep_command(cmd: DepCommand) -> Result<()> {
    match cmd {
        DepCommand::Add { from, to, relation } => {
            println!("Adding dependency: {} {} {}", from, relation, to);
            Ok(())
        }
        DepCommand::Remove { from, to, relation } => {
            println!("Removing dependency: {} {} {}", from, relation, to);
            Ok(())
        }
        DepCommand::Show { task_id } => {
            println!("Dependencies for task: {}", task_id);
            println!("  (storage not implemented)");
            Ok(())
        }
    }
}
