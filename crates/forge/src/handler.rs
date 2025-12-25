//! Command handlers for Forge CLI
//!
//! These handlers execute the task management commands.

use std::path::Path;

use anyhow::Result;

use crate::cli::{DepCommand, NoteCommand, TaskCommand};
use crate::store::Store;
use crate::task::Task;

/// Handle a task command
pub async fn handle_task_command(cmd: TaskCommand, db_path: &Path) -> Result<()> {
    let store = Store::new(db_path).await?;

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

            let created = store.create_task(task).await?;
            println!("Created task: {}", created.id_str().unwrap_or_default());
            Ok(())
        }

        TaskCommand::List { project, status } => {
            let status_filter = status
                .as_ref()
                .map(|s| s.parse())
                .transpose()?;

            let tasks = store
                .list_tasks(project.as_deref(), status_filter)
                .await?;

            if tasks.is_empty() {
                println!("No tasks found");
            } else {
                for task in tasks {
                    print_task_summary(&task);
                }
            }
            Ok(())
        }

        TaskCommand::Ready { project } => {
            let tasks = store.ready_tasks(project.as_deref()).await?;

            if tasks.is_empty() {
                println!("No ready tasks");
            } else {
                println!("Ready tasks:");
                for task in tasks {
                    print_task_summary(&task);
                }
            }
            Ok(())
        }

        TaskCommand::Get { id } => {
            match store.get_task(&id).await? {
                Some(task) => {
                    print_task_detail(&task);

                    // Show notes
                    let notes = store.get_notes(&id).await?;
                    if !notes.is_empty() {
                        println!("\nNotes:");
                        for note in notes {
                            let note_id = note.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();
                            println!("  [{}] {} - {}",
                                note_id,
                                note.created_at.format("%Y-%m-%d %H:%M"),
                                note.content
                            );
                        }
                    }

                    // Show dependencies
                    let deps = store.get_dependencies(&id).await?;
                    if !deps.is_empty() {
                        println!("\nDependencies:");
                        for dep in deps {
                            let from_id = dep.from_task.id.to_raw();
                            let to_id = dep.to_task.id.to_raw();
                            println!("  {} {} {}", from_id, dep.relation, to_id);
                        }
                    }
                }
                None => {
                    println!("Task not found: {}", id);
                }
            }
            Ok(())
        }

        TaskCommand::Update {
            id,
            status,
            priority,
        } => {
            let status_update = status
                .as_ref()
                .map(|s| s.parse())
                .transpose()?;

            match store.update_task(&id, status_update, priority).await? {
                Some(task) => {
                    println!("Updated task: {}", task.id_str().unwrap_or_default());
                    print_task_summary(&task);
                }
                None => {
                    println!("Task not found: {}", id);
                }
            }
            Ok(())
        }

        TaskCommand::Close { id, reason } => {
            match store.close_task(&id, reason.as_deref()).await? {
                Some(task) => {
                    let action = if reason.is_some() { "Cancelled" } else { "Completed" };
                    println!("{} task: {}", action, task.id_str().unwrap_or_default());
                }
                None => {
                    println!("Task not found: {}", id);
                }
            }
            Ok(())
        }

        TaskCommand::Delete { id } => {
            match store.delete_task(&id).await? {
                Some(task) => {
                    println!("Deleted task: {}", task.id_str().unwrap_or_default());
                }
                None => {
                    println!("Task not found: {}", id);
                }
            }
            Ok(())
        }

        TaskCommand::Note(note_cmd) => handle_note_command(&store, note_cmd).await,
        TaskCommand::Dep(dep_cmd) => handle_dep_command(&store, dep_cmd).await,
    }
}

async fn handle_note_command(store: &Store, cmd: NoteCommand) -> Result<()> {
    match cmd {
        NoteCommand::Add { task_id, content } => {
            let note = store.add_note(&task_id, &content).await?;
            let note_id = note.id.as_ref().map(|t| t.id.to_raw()).unwrap_or_default();
            println!("Added note: {}", note_id);
            Ok(())
        }
        NoteCommand::Edit { note_id, content } => {
            match store.edit_note(&note_id, &content).await? {
                Some(_) => println!("Updated note: {}", note_id),
                None => println!("Note not found: {}", note_id),
            }
            Ok(())
        }
        NoteCommand::Delete { note_id } => {
            match store.delete_note(&note_id).await? {
                Some(_) => println!("Deleted note: {}", note_id),
                None => println!("Note not found: {}", note_id),
            }
            Ok(())
        }
    }
}

async fn handle_dep_command(store: &Store, cmd: DepCommand) -> Result<()> {
    match cmd {
        DepCommand::Add { from, to, relation } => {
            store.add_dependency(&from, &to, &relation).await?;
            println!("Added dependency: {} {} {}", from, relation, to);
            Ok(())
        }
        DepCommand::Remove { from, to, relation } => {
            store.remove_dependency(&from, &to, &relation).await?;
            println!("Removed dependency: {} {} {}", from, relation, to);
            Ok(())
        }
        DepCommand::Show { task_id } => {
            let deps = store.get_dependencies(&task_id).await?;

            if deps.is_empty() {
                println!("No dependencies for task: {}", task_id);
            } else {
                println!("Dependencies for task {}:", task_id);
                for dep in deps {
                    let from_id = dep.from_task.id.to_raw();
                    let to_id = dep.to_task.id.to_raw();
                    println!("  {} {} {}", from_id, dep.relation, to_id);
                }
            }
            Ok(())
        }
    }
}

fn print_task_summary(task: &Task) {
    let id = task.id_str().unwrap_or_default();
    let project = task.project.as_deref().unwrap_or("-");
    println!(
        "[{}] {} ({}) [{}] p:{}",
        id, task.title, task.status, project, task.priority
    );
}

fn print_task_detail(task: &Task) {
    println!("Task: {}", task.id_str().unwrap_or_default());
    println!("  Title: {}", task.title);
    if let Some(ref desc) = task.description {
        println!("  Description: {}", desc);
    }
    println!("  Status: {}", task.status);
    if let Some(ref proj) = task.project {
        println!("  Project: {}", proj);
    }
    println!("  Priority: {}", task.priority);
    println!("  Created: {}", task.created_at.format("%Y-%m-%d %H:%M:%S"));
    println!("  Updated: {}", task.updated_at.format("%Y-%m-%d %H:%M:%S"));
    if let Some(ref completed) = task.completed_at {
        println!("  Completed: {}", completed.format("%Y-%m-%d %H:%M:%S"));
    }
}
