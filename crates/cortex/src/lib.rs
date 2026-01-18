//! Cortex: Multi-agent orchestration for Claude processes
//!
//! Cortex enables a coordinator Claude instance to spawn and manage multiple
//! worker Claude processes, each operating in isolated worktrees.
//!
//! ## Architecture
//!
//! ```text
//! Coordinator Claude <-> Memex Daemon <-> [Worker 1, Worker 2, ...]
//!                        (with Cortex)    (Claude processes)
//! ```
//!
//! Cortex is integrated into the memex daemon, sharing the same IPC socket
//! and MCP server as Atlas (knowledge) and Forge (tasks).
//!
//! ## Usage
//!
//! The coordinator interacts with Cortex via MCP tools:
//! - `cortex_create_worker` - Create a new worker for a directory
//! - `cortex_send_message` - Send a message to a worker
//! - `cortex_list_workers` - List active workers
//! - `cortex_worker_status` - Get detailed worker status
//! - `cortex_remove_worker` - Remove a worker

pub mod client;
pub mod error;
pub mod types;
pub mod worker;

pub use client::CortexClient;
pub use error::{CortexError, Result};
pub use types::{TranscriptEntry, WorkerConfig, WorkerId, WorkerMcpConfig, WorkerState, WorkerStatus};
pub use worker::{ShellValidation, WorkerManager, WorkerResponse, validate_shell_env};
