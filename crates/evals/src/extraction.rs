//! Extraction evaluation framework
//!
//! Tests the record extraction pipeline by:
//! 1. Loading test fixtures with memos and expected records
//! 2. Recording memos to a clean database
//! 3. Running extraction on each memo
//! 4. Comparing extracted records to expected records
//!
//! Supports test categorization (strong/weak cases) and detailed metrics
//! for tracking improvement over time.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// Test category indicating expected performance level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TestCategory {
    /// Strong case - explicit mentions, clear relationships
    /// Expected to pass with high accuracy
    #[default]
    Strong,
    /// Weak case - implicit context, requires inference
    /// Used to measure improvement over time
    Weak,
    /// Regression case - previously working, must not break
    Regression,
}

/// Difficulty level for categorizing test complexity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TestDifficulty {
    /// Easy - single entity, explicit mentions
    Easy,
    /// Medium - multiple entities, clear structure
    #[default]
    Medium,
    /// Hard - complex relationships, inference required
    Hard,
}

/// An extraction test fixture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionTest {
    pub meta: TestMeta,
    pub expected: ExpectedOutput,
    pub memos: Vec<TestMemo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMeta {
    pub name: String,
    pub description: String,
    /// Test category for classification
    #[serde(default)]
    pub category: TestCategory,
    /// Difficulty level
    #[serde(default)]
    pub difficulty: TestDifficulty,
    /// Tags for filtering tests
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutput {
    pub records: Vec<ExpectedRecord>,
    #[serde(default)]
    pub edges: Vec<ExpectedEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedRecord {
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedEdge {
    pub from: String,
    pub relation: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMemo {
    pub content: String,
}

/// Result of running an extraction test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionTestResult {
    pub test_name: String,
    pub category: TestCategory,
    pub difficulty: TestDifficulty,
    pub memos_recorded: usize,
    pub records_expected: usize,
    pub records_extracted: usize,
    pub records_matched: usize,
    pub records_missing: Vec<String>,
    pub records_extra: Vec<String>,
    pub edges_expected: usize,
    pub edges_matched: usize,
    pub edges_missing: Vec<String>,
    pub edges_extra: Vec<String>,
    /// Precision: matched / extracted (how many extractions were correct)
    pub record_precision: f64,
    /// Recall: matched / expected (how many expected were found)
    pub record_recall: f64,
    /// F1 score: harmonic mean of precision and recall
    pub record_f1: f64,
    pub edge_precision: f64,
    pub edge_recall: f64,
    pub edge_f1: f64,
    /// Legacy accuracy metrics (recall)
    pub record_accuracy: f64,
    pub edge_accuracy: f64,
}

impl ExtractionTest {
    /// Load a test fixture from a TOML file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Compare extracted records against expected records
    pub fn evaluate(
        &self,
        extracted_records: &[ExtractedRecord],
        extracted_edges: &[ExtractedEdge],
    ) -> ExtractionTestResult {
        // Build sets for comparison (normalized to lowercase for fuzzy matching)
        let expected_set: HashSet<(String, String)> = self
            .expected
            .records
            .iter()
            .map(|r| (r.record_type.to_lowercase(), r.name.to_lowercase()))
            .collect();

        let extracted_set: HashSet<(String, String)> = extracted_records
            .iter()
            .map(|r| (r.record_type.to_lowercase(), r.name.to_lowercase()))
            .collect();

        // Find matches, missing, and extra
        let matched: HashSet<_> = expected_set.intersection(&extracted_set).collect();
        let missing: Vec<_> = expected_set
            .difference(&extracted_set)
            .map(|(t, n)| format!("{}:{}", t, n))
            .collect();
        let extra: Vec<_> = extracted_set
            .difference(&expected_set)
            .map(|(t, n)| format!("{}:{}", t, n))
            .collect();

        // Build edge comparison (by name references)
        let expected_edges: HashSet<(String, String, String)> = self
            .expected
            .edges
            .iter()
            .map(|e| (e.from.to_lowercase(), e.relation.to_lowercase(), e.to.to_lowercase()))
            .collect();

        let extracted_edges_set: HashSet<(String, String, String)> = extracted_edges
            .iter()
            .map(|e| (e.from.to_lowercase(), e.relation.to_lowercase(), e.to.to_lowercase()))
            .collect();

        let edges_matched_count = expected_edges.intersection(&extracted_edges_set).count();
        let edges_missing: Vec<_> = expected_edges
            .difference(&extracted_edges_set)
            .map(|(f, r, t)| format!("{} --{}-> {}", f, r, t))
            .collect();
        let edges_extra: Vec<_> = extracted_edges_set
            .difference(&expected_edges)
            .map(|(f, r, t)| format!("{} --{}-> {}", f, r, t))
            .collect();

        // Calculate precision, recall, F1 for records
        let record_precision = if extracted_records.is_empty() {
            1.0 // No false positives if nothing extracted
        } else {
            matched.len() as f64 / extracted_records.len() as f64
        };

        let record_recall = if self.expected.records.is_empty() {
            1.0
        } else {
            matched.len() as f64 / self.expected.records.len() as f64
        };

        let record_f1 = if record_precision + record_recall == 0.0 {
            0.0
        } else {
            2.0 * (record_precision * record_recall) / (record_precision + record_recall)
        };

        // Calculate precision, recall, F1 for edges
        let edge_precision = if extracted_edges.is_empty() {
            1.0
        } else {
            edges_matched_count as f64 / extracted_edges.len() as f64
        };

        let edge_recall = if self.expected.edges.is_empty() {
            1.0
        } else {
            edges_matched_count as f64 / self.expected.edges.len() as f64
        };

        let edge_f1 = if edge_precision + edge_recall == 0.0 {
            0.0
        } else {
            2.0 * (edge_precision * edge_recall) / (edge_precision + edge_recall)
        };

        ExtractionTestResult {
            test_name: self.meta.name.clone(),
            category: self.meta.category,
            difficulty: self.meta.difficulty,
            memos_recorded: self.memos.len(),
            records_expected: self.expected.records.len(),
            records_extracted: extracted_records.len(),
            records_matched: matched.len(),
            records_missing: missing,
            records_extra: extra,
            edges_expected: self.expected.edges.len(),
            edges_matched: edges_matched_count,
            edges_missing,
            edges_extra,
            record_precision,
            record_recall,
            record_f1,
            edge_precision,
            edge_recall,
            edge_f1,
            // Legacy: accuracy = recall
            record_accuracy: record_recall,
            edge_accuracy: edge_recall,
        }
    }
}

/// A record as extracted (simplified for comparison)
#[derive(Debug, Clone)]
pub struct ExtractedRecord {
    pub record_type: String,
    pub name: String,
}

/// An edge as extracted (simplified for comparison)
#[derive(Debug, Clone)]
pub struct ExtractedEdge {
    pub from: String,
    pub relation: String,
    pub to: String,
}

impl ExtractionTestResult {
    /// Check if this test passed based on category-specific thresholds
    pub fn passed(&self) -> bool {
        match self.category {
            TestCategory::Strong | TestCategory::Regression => {
                // Strong/regression tests should achieve high accuracy
                self.record_f1 >= 0.8 && self.edge_f1 >= 0.7
            }
            TestCategory::Weak => {
                // Weak tests have lower thresholds - we're measuring improvement
                self.record_f1 >= 0.5 && self.edge_f1 >= 0.4
            }
        }
    }

    /// Print a summary of the test results
    pub fn print_summary(&self) {
        let category_str = match self.category {
            TestCategory::Strong => "STRONG",
            TestCategory::Weak => "WEAK",
            TestCategory::Regression => "REGRESSION",
        };
        let difficulty_str = match self.difficulty {
            TestDifficulty::Easy => "Easy",
            TestDifficulty::Medium => "Medium",
            TestDifficulty::Hard => "Hard",
        };

        println!("=== {} ({} / {}) ===", self.test_name, category_str, difficulty_str);
        println!();
        println!("Memos recorded: {}", self.memos_recorded);
        println!();
        println!("Records:");
        println!("  Expected:  {}", self.records_expected);
        println!("  Extracted: {}", self.records_extracted);
        println!("  Matched:   {}", self.records_matched);
        println!("  Precision: {:.1}%", self.record_precision * 100.0);
        println!("  Recall:    {:.1}%", self.record_recall * 100.0);
        println!("  F1 Score:  {:.1}%", self.record_f1 * 100.0);

        if !self.records_missing.is_empty() {
            println!();
            println!("  Missing records:");
            for r in &self.records_missing {
                println!("    - {}", r);
            }
        }

        if !self.records_extra.is_empty() {
            println!();
            println!("  Extra records (unexpected):");
            for r in &self.records_extra {
                println!("    - {}", r);
            }
        }

        println!();
        println!("Edges:");
        println!("  Expected:  {}", self.edges_expected);
        println!("  Extracted: {}", self.edges_matched + self.edges_extra.len());
        println!("  Matched:   {}", self.edges_matched);
        println!("  Precision: {:.1}%", self.edge_precision * 100.0);
        println!("  Recall:    {:.1}%", self.edge_recall * 100.0);
        println!("  F1 Score:  {:.1}%", self.edge_f1 * 100.0);

        if !self.edges_missing.is_empty() {
            println!();
            println!("  Missing edges:");
            for e in &self.edges_missing {
                println!("    - {}", e);
            }
        }

        if !self.edges_extra.is_empty() {
            println!();
            println!("  Extra edges (unexpected):");
            for e in &self.edges_extra {
                println!("    - {}", e);
            }
        }

        println!();
        let status = if self.passed() { "PASS" } else { "FAIL" };
        println!("[{}] Record F1: {:.1}%, Edge F1: {:.1}%",
            status,
            self.record_f1 * 100.0,
            self.edge_f1 * 100.0
        );
    }

    /// Generate a compact one-line summary
    pub fn one_line_summary(&self) -> String {
        let status = if self.passed() { "PASS" } else { "FAIL" };
        format!(
            "[{}] {} - Records: {:.0}% F1, Edges: {:.0}% F1",
            status,
            self.test_name,
            self.record_f1 * 100.0,
            self.edge_f1 * 100.0
        )
    }
}

/// Aggregate results across multiple extraction tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionSuiteResult {
    pub results: Vec<ExtractionTestResult>,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub strong_passed: usize,
    pub strong_total: usize,
    pub weak_passed: usize,
    pub weak_total: usize,
    pub avg_record_f1: f64,
    pub avg_edge_f1: f64,
}

impl ExtractionSuiteResult {
    /// Create a suite result from individual test results
    pub fn from_results(results: Vec<ExtractionTestResult>) -> Self {
        let total_tests = results.len();
        let passed_tests = results.iter().filter(|r| r.passed()).count();

        let strong_tests: Vec<_> = results.iter()
            .filter(|r| matches!(r.category, TestCategory::Strong | TestCategory::Regression))
            .collect();
        let weak_tests: Vec<_> = results.iter()
            .filter(|r| matches!(r.category, TestCategory::Weak))
            .collect();

        let strong_passed = strong_tests.iter().filter(|r| r.passed()).count();
        let strong_total = strong_tests.len();
        let weak_passed = weak_tests.iter().filter(|r| r.passed()).count();
        let weak_total = weak_tests.len();

        let avg_record_f1 = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.record_f1).sum::<f64>() / results.len() as f64
        };

        let avg_edge_f1 = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.edge_f1).sum::<f64>() / results.len() as f64
        };

        Self {
            results,
            total_tests,
            passed_tests,
            strong_passed,
            strong_total,
            weak_passed,
            weak_total,
            avg_record_f1,
            avg_edge_f1,
        }
    }

    /// Print a summary of the suite results
    pub fn print_summary(&self) {
        println!("\n========== EXTRACTION TEST SUITE ==========\n");

        for result in &self.results {
            println!("{}", result.one_line_summary());
        }

        println!("\n---------- Summary ----------\n");
        println!("Total:  {}/{} passed ({:.1}%)",
            self.passed_tests, self.total_tests,
            if self.total_tests == 0 { 0.0 } else {
                self.passed_tests as f64 / self.total_tests as f64 * 100.0
            }
        );

        if self.strong_total > 0 {
            println!("Strong: {}/{} passed ({:.1}%)",
                self.strong_passed, self.strong_total,
                self.strong_passed as f64 / self.strong_total as f64 * 100.0
            );
        }

        if self.weak_total > 0 {
            println!("Weak:   {}/{} passed ({:.1}%)",
                self.weak_passed, self.weak_total,
                self.weak_passed as f64 / self.weak_total as f64 * 100.0
            );
        }

        println!();
        println!("Average Record F1: {:.1}%", self.avg_record_f1 * 100.0);
        println!("Average Edge F1:   {:.1}%", self.avg_edge_f1 * 100.0);
        println!("\n=============================================\n");
    }

    /// Export results to JSON for comparison
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("Failed to serialize suite results")
    }
}
