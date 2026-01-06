//! Atlas - Memory and knowledge graph for Memex
//!
//! Atlas maps and navigates information terrain. It ingests episodes,
//! builds graph representations, and generates context on demand.

pub mod cli;
pub mod client;
pub mod event;
pub mod extraction;
pub mod fact;
pub mod handler;
pub mod memo;
pub mod store;

pub use cli::{EventCommand, KnowledgeCommand, MemoCommand};
pub use client::{
    EventClient, FactStats, KnowledgeClient, KnowledgeStatus, MemoClient, QueryResult,
    RebuildResult, SearchResult,
};
pub use event::{Event, EventAuthority, EventSource};
pub use extraction::{ExtractionResult, Extractor};
pub use fact::{Entity, EntityType, EpisodeRef, Fact, FactType};
pub use handler::{handle_event_command, handle_knowledge_command, handle_memo_command};
pub use memo::{Memo, MemoAuthority, MemoSource};
pub use store::Store;
