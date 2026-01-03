//! Atlas - Memory and knowledge graph for Memex
//!
//! Atlas maps and navigates information terrain. It ingests episodes,
//! builds graph representations, and generates context on demand.

pub mod cli;
pub mod client;
pub mod handler;
pub mod memo;
pub mod store;

pub use cli::MemoCommand;
pub use client::MemoClient;
pub use handler::handle_memo_command;
pub use memo::{Memo, MemoAuthority, MemoSource};
pub use store::Store;
