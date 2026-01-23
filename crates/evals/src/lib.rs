//! Evaluation framework for Memex
//!
//! Tests memex end-to-end with realistic scenarios and uses LLM
//! evaluation to score answer quality.
//!
//! ## Test Categories
//!
//! - **Strong cases**: Explicit mentions, clear relationships. Expected to pass.
//! - **Weak cases**: Implicit context, inference required. Measures improvement.
//! - **Regression cases**: Previously working. Must not break.
//!
//! ## Metrics
//!
//! - **Precision**: How many extractions were correct
//! - **Recall**: How many expected items were found
//! - **F1 Score**: Harmonic mean of precision and recall
//!
//! ## Snapshots
//!
//! The snapshot module enables loading pre-built knowledge bases for testing:
//! - Avoid reconstruction overhead from building test databases each run
//! - Test against realistic data volumes
//! - Create reproducible test scenarios

pub mod comparative;
pub mod extraction;
pub mod harness;
pub mod judge;
pub mod loader;
pub mod scenario;
pub mod snapshot;

pub use extraction::{
    ExtractionTest, ExtractionTestResult, ExtractionSuiteResult,
    ExtractedRecord, ExtractedEdge, TestCategory, TestDifficulty,
};
pub use harness::TestHarness;
pub use judge::{EvalResult, EvalScore, Judge};
pub use loader::{load_scenario, load_scenarios_from_dir};
pub use scenario::{Action, ExpectedOutcome, Query, Scenario, SnapshotScenario, builtin_snapshot_scenarios};
pub use comparative::{ComparativeResult, BranchComparison};
pub use snapshot::{Snapshot, SnapshotLoader, SnapshotMemo, SnapshotRecord, SnapshotEdge, LoadResult};
