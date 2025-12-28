//! Forge - Task and work management for Memex
//!
//! Forge tracks tasks, dependencies, and progress. Task events are sent to Atlas
//! as episodes, building institutional memory from work activity.

pub mod cli;
pub mod client;
pub mod handler;
pub mod memo;
pub mod migrations;
pub mod store;
pub mod task;

pub use cli::TaskCommand;
pub use client::TaskClient;
pub use handler::handle_task_command;
pub use memo::{Memo, MemoAuthority, MemoSource};
pub use store::Store;
pub use task::{Task, TaskDependency, TaskId, TaskNote, TaskStatus};
