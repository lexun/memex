//! Cortex: Multi-agent orchestration for Claude processes
//!
//! Cortex enables a coordinator Claude instance to spawn and manage multiple
//! worker Claude processes, each operating in isolated worktrees.
//!
//! ## Architecture
//!
//! ```text
//! Coordinator Claude <-> Cortex Daemon <-> [Worker 1, Worker 2, ...]
//!                        (MCP Server)      (Claude processes)
//! ```
//!
//! ## Usage
//!
//! The coordinator interacts with Cortex via MCP tools:
//! - `cortex_spawn_worker` - Start a new Claude worker
//! - `cortex_send_message` - Send a message to a worker
//! - `cortex_get_response` - Get response from a worker
//! - `cortex_list_workers` - List active workers
//! - `cortex_kill_worker` - Terminate a worker

pub mod error;
pub mod types;
pub mod worker;
pub mod worktree;

pub use error::{CortexError, Result};
pub use types::{WorkerId, WorkerStatus, WorkerState, WorkerConfig};
pub use worker::{WorkerManager, WorkerResponse};
pub use worktree::WorktreeManager;
