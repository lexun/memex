//! Forge - Task and work management for Memex
//!
//! Forge tracks tasks, dependencies, and progress. Task events are sent to Atlas
//! as episodes, building institutional memory from work activity.

pub mod task;

pub use task::{Task, TaskId, TaskStatus};
