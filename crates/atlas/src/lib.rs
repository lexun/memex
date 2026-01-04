//! Atlas - Memory and knowledge graph for Memex
//!
//! Atlas maps and navigates information terrain. It ingests episodes,
//! builds graph representations, and generates context on demand.

pub mod cli;
pub mod client;
pub mod event;
pub mod handler;
pub mod memo;
pub mod store;

pub use cli::{ContextCommand, EventCommand, MemoCommand};
pub use client::{ContextClient, ContextResult, EventClient, MemoClient};
pub use event::{Event, EventAuthority, EventSource};
pub use handler::{handle_context_command, handle_event_command, handle_memo_command};
pub use memo::{Memo, MemoAuthority, MemoSource};
pub use store::Store;
