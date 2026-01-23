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
pub mod multi_step_extraction;
pub mod query;
pub mod record;
pub mod record_extraction;
pub mod store;

pub use cli::{EdgeCommand, EdgeRelationCli, EventCommand, KnowledgeCommand, MemoCommand, RecordCommand, RecordTypeCli};
pub use client::{
    BackfillResult, EntityFactsResult, EventClient, ExtractRecordsResponse, FactStats,
    KnowledgeClient, KnowledgeStatus, MemoClient, QueryResult, RebuildResult, RecordBackfillResult,
    RecordClient, RelatedFactsResult, SearchResult,
};
pub use event::{Event, EventAuthority, EventSource};
pub use extraction::{ExtractedFact, ExtractionResult, Extractor};
pub use fact::{Entity, EntityType, EpisodeRef, Fact, FactType};
pub use query::{DecomposedQuery, HypotheticalGenerator, QueryDecomposer, QueryIntent, TemporalFilter, TemporalParser};
pub use handler::{handle_event_command, handle_knowledge_command, handle_memo_command, handle_record_command};
pub use memo::{Memo, MemoAuthority, MemoSource};
pub use record::{
    ContextAssembly, EdgeRelation, Record, RecordEdge, RecordType,
    TaskContent, TaskStatus, TaskView, TaskNoteView, TaskDependencyView,
    MessageContent, MessageType, MessageView,
    ThreadContent, ThreadSource, EntryContent, EntryRole,
    TASK_NOTE_RELATION, task_relations,
};
pub use record_extraction::{
    ExtractionContext, ExtractionProcessingResult, ExtractionQuestion,
    ExtractedLink, ExtractedRecord, RecordAction, RecordExtractionResult,
    RecordExtractor, RecordSummary,
};
pub use multi_step_extraction::MultiStepExtractor;
pub use store::Store;
