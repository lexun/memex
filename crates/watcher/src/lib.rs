//! Claude Code transcript capture
//!
//! This crate watches Claude Code session files in `~/.claude/projects/` and
//! captures conversations into the memex knowledge base.

mod parser;
mod watcher;

pub use parser::{ClaudeCodeEntry, EntryType, Message, parse_jsonl, parse_jsonl_incremental};
pub use watcher::{TranscriptWatcher, WatchConfig, WatchState};

/// Result type for watcher operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for transcript watching
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("File watching error: {0}")]
    Notify(#[from] notify::Error),

    #[error("Invalid session file: {0}")]
    InvalidSession(String),
}
