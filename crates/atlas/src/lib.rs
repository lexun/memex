//! Atlas - Memory and knowledge graph for Memex
//!
//! Atlas maps and navigates information terrain. It ingests episodes,
//! builds graph representations, and generates context on demand.

pub mod cli;
pub mod client;
pub mod handler;

pub use cli::MemoCommand;
pub use client::MemoClient;
pub use handler::handle_memo_command;

// Re-export memo types from forge for convenience
pub use forge::{Memo, MemoAuthority, MemoSource};
