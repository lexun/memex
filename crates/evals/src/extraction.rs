//! Extraction evaluation framework
//!
//! Tests the record extraction pipeline by:
//! 1. Loading test fixtures with memos and expected records
//! 2. Recording memos to a clean database
//! 3. Running extraction on each memo
//! 4. Comparing extracted records to expected records

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

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
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionTestResult {
    pub test_name: String,
    pub memos_recorded: usize,
    pub records_expected: usize,
    pub records_extracted: usize,
    pub records_matched: usize,
    pub records_missing: Vec<String>,
    pub records_extra: Vec<String>,
    pub edges_expected: usize,
    pub edges_matched: usize,
    pub edges_missing: Vec<String>,
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

        let edges_matched = expected_edges.intersection(&extracted_edges_set).count();
        let edges_missing: Vec<_> = expected_edges
            .difference(&extracted_edges_set)
            .map(|(f, r, t)| format!("{} --{}-> {}", f, r, t))
            .collect();

        let record_accuracy = if self.expected.records.is_empty() {
            1.0
        } else {
            matched.len() as f64 / self.expected.records.len() as f64
        };

        let edge_accuracy = if self.expected.edges.is_empty() {
            1.0
        } else {
            edges_matched as f64 / self.expected.edges.len() as f64
        };

        ExtractionTestResult {
            test_name: self.meta.name.clone(),
            memos_recorded: self.memos.len(),
            records_expected: self.expected.records.len(),
            records_extracted: extracted_records.len(),
            records_matched: matched.len(),
            records_missing: missing,
            records_extra: extra,
            edges_expected: self.expected.edges.len(),
            edges_matched,
            edges_missing,
            record_accuracy,
            edge_accuracy,
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
    /// Print a summary of the test results
    pub fn print_summary(&self) {
        println!("=== {} ===", self.test_name);
        println!();
        println!("Memos recorded: {}", self.memos_recorded);
        println!();
        println!("Records:");
        println!("  Expected:  {}", self.records_expected);
        println!("  Extracted: {}", self.records_extracted);
        println!("  Matched:   {}", self.records_matched);
        println!("  Accuracy:  {:.1}%", self.record_accuracy * 100.0);

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
        println!("  Expected: {}", self.edges_expected);
        println!("  Matched:  {}", self.edges_matched);
        println!("  Accuracy: {:.1}%", self.edge_accuracy * 100.0);

        if !self.edges_missing.is_empty() {
            println!();
            println!("  Missing edges:");
            for e in &self.edges_missing {
                println!("    - {}", e);
            }
        }

        println!();
        println!("Overall: {:.1}% records, {:.1}% edges",
            self.record_accuracy * 100.0,
            self.edge_accuracy * 100.0
        );
    }
}
