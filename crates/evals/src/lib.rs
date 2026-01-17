//! Evaluation framework for Memex
//!
//! Tests memex end-to-end with realistic scenarios and uses LLM
//! evaluation to score answer quality.

pub mod harness;
pub mod judge;
pub mod loader;
pub mod scenario;

pub use harness::TestHarness;
pub use judge::{EvalResult, EvalScore, Judge};
pub use loader::{load_scenario, load_scenarios_from_dir};
pub use scenario::{Action, ExpectedOutcome, Query, Scenario};
