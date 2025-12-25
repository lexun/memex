//! Forge - Task and work management for Memex
//!
//! Forge tracks tasks, dependencies, and progress. Task events are sent to Atlas
//! as episodes, building institutional memory from work activity.

pub mod cli;
pub mod handler;
pub mod migrations;
pub mod store;
pub mod task;

pub use cli::TaskCommand;
pub use handler::handle_task_command;
pub use store::Store;
pub use task::{Task, TaskId, TaskStatus};
