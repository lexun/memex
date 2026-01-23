//! Comparative evaluation framework
//!
//! Enables running the same test suite against two different branches
//! and comparing the results to measure improvement or detect regression.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::extraction::ExtractionSuiteResult;
use crate::harness::EvalReport;

/// Result from a single branch run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchResult {
    /// Branch name or identifier
    pub branch: String,
    /// Git commit hash (if available)
    pub commit: Option<String>,
    /// Timestamp of the run
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Extraction test results
    pub extraction: Option<ExtractionSuiteResult>,
    /// Retrieval/scenario test results
    pub retrieval: Option<RetrievalSummary>,
}

/// Summary of retrieval test results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalSummary {
    pub total_scenarios: usize,
    pub passed_scenarios: usize,
    pub total_queries: usize,
    pub passed_queries: usize,
    pub avg_relevance: f64,
    pub avg_completeness: f64,
    pub avg_accuracy: f64,
    pub avg_coherence: f64,
    pub avg_overall: f64,
}

impl From<&EvalReport> for RetrievalSummary {
    fn from(report: &EvalReport) -> Self {
        let all_results: Vec<_> = report.scenarios
            .iter()
            .flat_map(|s| &s.query_results)
            .collect();

        let count = all_results.len();
        if count == 0 {
            return Self {
                total_scenarios: report.total_scenarios,
                passed_scenarios: report.passed_scenarios,
                total_queries: 0,
                passed_queries: 0,
                avg_relevance: 0.0,
                avg_completeness: 0.0,
                avg_accuracy: 0.0,
                avg_coherence: 0.0,
                avg_overall: 0.0,
            };
        }

        let sum_relevance: u32 = all_results.iter().map(|r| r.scores.relevance as u32).sum();
        let sum_completeness: u32 = all_results.iter().map(|r| r.scores.completeness as u32).sum();
        let sum_accuracy: u32 = all_results.iter().map(|r| r.scores.accuracy as u32).sum();
        let sum_coherence: u32 = all_results.iter().map(|r| r.scores.coherence as u32).sum();
        let sum_overall: u32 = all_results.iter().map(|r| r.scores.overall as u32).sum();

        Self {
            total_scenarios: report.total_scenarios,
            passed_scenarios: report.passed_scenarios,
            total_queries: report.total_queries,
            passed_queries: report.passed_queries,
            avg_relevance: sum_relevance as f64 / count as f64,
            avg_completeness: sum_completeness as f64 / count as f64,
            avg_accuracy: sum_accuracy as f64 / count as f64,
            avg_coherence: sum_coherence as f64 / count as f64,
            avg_overall: sum_overall as f64 / count as f64,
        }
    }
}

/// Comparison between two branch results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchComparison {
    /// Baseline branch (typically main or current)
    pub baseline: BranchResult,
    /// Candidate branch (the one being evaluated)
    pub candidate: BranchResult,
    /// Per-test comparison for extraction
    pub extraction_diffs: Vec<TestDiff>,
    /// Per-query comparison for retrieval
    pub retrieval_diffs: Vec<QueryDiff>,
    /// Overall verdict
    pub verdict: ComparisonVerdict,
}

/// Difference in a single test between branches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDiff {
    pub test_name: String,
    pub baseline_f1: f64,
    pub candidate_f1: f64,
    pub delta: f64,
    pub status: DiffStatus,
}

/// Difference in a single query between branches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryDiff {
    pub scenario: String,
    pub query: String,
    pub baseline_overall: u8,
    pub candidate_overall: u8,
    pub delta: i16,
    pub status: DiffStatus,
}

/// Status of a diff
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffStatus {
    /// Significantly improved (>5% or >5 points)
    Improved,
    /// No significant change
    Unchanged,
    /// Significantly regressed (<-5% or <-5 points)
    Regressed,
}

/// Overall comparison verdict
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonVerdict {
    /// Candidate is better overall
    Better,
    /// No significant difference
    Equivalent,
    /// Candidate is worse overall
    Worse,
    /// Mixed results - some better, some worse
    Mixed,
}

/// Comparative result combining both extraction and retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparativeResult {
    /// Results per branch
    pub branches: HashMap<String, BranchResult>,
    /// Pairwise comparisons
    pub comparisons: Vec<BranchComparison>,
}

impl ComparativeResult {
    /// Create a new comparative result
    pub fn new() -> Self {
        Self {
            branches: HashMap::new(),
            comparisons: Vec::new(),
        }
    }

    /// Add a branch result
    pub fn add_branch(&mut self, result: BranchResult) {
        self.branches.insert(result.branch.clone(), result);
    }

    /// Compare two branches
    pub fn compare(&mut self, baseline_name: &str, candidate_name: &str) -> Result<&BranchComparison> {
        let baseline = self.branches.get(baseline_name)
            .context(format!("Baseline branch '{}' not found", baseline_name))?
            .clone();
        let candidate = self.branches.get(candidate_name)
            .context(format!("Candidate branch '{}' not found", candidate_name))?
            .clone();

        let comparison = Self::build_comparison(baseline, candidate);
        self.comparisons.push(comparison);
        Ok(self.comparisons.last().unwrap())
    }

    /// Build a comparison between two branch results
    fn build_comparison(baseline: BranchResult, candidate: BranchResult) -> BranchComparison {
        let mut extraction_diffs = Vec::new();
        let retrieval_diffs = Vec::new();

        // Compare extraction results
        if let (Some(base_ext), Some(cand_ext)) = (&baseline.extraction, &candidate.extraction) {
            for base_result in &base_ext.results {
                if let Some(cand_result) = cand_ext.results.iter()
                    .find(|r| r.test_name == base_result.test_name)
                {
                    let base_f1 = (base_result.record_f1 + base_result.edge_f1) / 2.0;
                    let cand_f1 = (cand_result.record_f1 + cand_result.edge_f1) / 2.0;
                    let delta = cand_f1 - base_f1;

                    let status = if delta > 0.05 {
                        DiffStatus::Improved
                    } else if delta < -0.05 {
                        DiffStatus::Regressed
                    } else {
                        DiffStatus::Unchanged
                    };

                    extraction_diffs.push(TestDiff {
                        test_name: base_result.test_name.clone(),
                        baseline_f1: base_f1,
                        candidate_f1: cand_f1,
                        delta,
                        status,
                    });
                }
            }
        }

        // Determine verdict
        let improved_count = extraction_diffs.iter()
            .filter(|d| d.status == DiffStatus::Improved)
            .count();
        let regressed_count = extraction_diffs.iter()
            .filter(|d| d.status == DiffStatus::Regressed)
            .count();

        let verdict = if regressed_count == 0 && improved_count > 0 {
            ComparisonVerdict::Better
        } else if improved_count == 0 && regressed_count > 0 {
            ComparisonVerdict::Worse
        } else if improved_count > 0 && regressed_count > 0 {
            ComparisonVerdict::Mixed
        } else {
            ComparisonVerdict::Equivalent
        };

        BranchComparison {
            baseline,
            candidate,
            extraction_diffs,
            retrieval_diffs,
            verdict,
        }
    }

    /// Save results to a JSON file
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load results from a JSON file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let result = serde_json::from_str(&content)?;
        Ok(result)
    }

    /// Print comparison summary
    pub fn print_comparison(&self, baseline_name: &str, candidate_name: &str) {
        if let Some(comparison) = self.comparisons.iter()
            .find(|c| c.baseline.branch == baseline_name && c.candidate.branch == candidate_name)
        {
            comparison.print_summary();
        } else {
            println!("No comparison found between {} and {}", baseline_name, candidate_name);
        }
    }
}

impl Default for ComparativeResult {
    fn default() -> Self {
        Self::new()
    }
}

impl BranchComparison {
    /// Print a summary of the comparison
    pub fn print_summary(&self) {
        println!("\n========== BRANCH COMPARISON ==========");
        println!();
        println!("Baseline:  {} ({})",
            self.baseline.branch,
            self.baseline.commit.as_deref().unwrap_or("unknown")
        );
        println!("Candidate: {} ({})",
            self.candidate.branch,
            self.candidate.commit.as_deref().unwrap_or("unknown")
        );
        println!();

        // Extraction comparison
        if !self.extraction_diffs.is_empty() {
            println!("---------- Extraction Tests ----------");
            println!();
            println!("{:<30} {:>10} {:>10} {:>10} {:>10}",
                "Test", "Baseline", "Candidate", "Delta", "Status"
            );
            println!("{}", "-".repeat(72));

            for diff in &self.extraction_diffs {
                let status_str = match diff.status {
                    DiffStatus::Improved => "+",
                    DiffStatus::Unchanged => "=",
                    DiffStatus::Regressed => "-",
                };
                println!("{:<30} {:>9.1}% {:>9.1}% {:>+9.1}% {:>10}",
                    diff.test_name,
                    diff.baseline_f1 * 100.0,
                    diff.candidate_f1 * 100.0,
                    diff.delta * 100.0,
                    status_str
                );
            }
            println!();
        }

        // Retrieval comparison
        if !self.retrieval_diffs.is_empty() {
            println!("---------- Retrieval Queries ----------");
            println!();
            for diff in &self.retrieval_diffs {
                let status_str = match diff.status {
                    DiffStatus::Improved => "+",
                    DiffStatus::Unchanged => "=",
                    DiffStatus::Regressed => "-",
                };
                println!("[{}] {} / {} - base: {}, cand: {}, delta: {:+}",
                    status_str,
                    diff.scenario,
                    diff.query,
                    diff.baseline_overall,
                    diff.candidate_overall,
                    diff.delta
                );
            }
            println!();
        }

        // Summary stats
        if let (Some(base_ext), Some(cand_ext)) = (&self.baseline.extraction, &self.candidate.extraction) {
            println!("---------- Summary ----------");
            println!();
            println!("Extraction:");
            println!("  Baseline avg F1:  {:.1}% records, {:.1}% edges",
                base_ext.avg_record_f1 * 100.0,
                base_ext.avg_edge_f1 * 100.0
            );
            println!("  Candidate avg F1: {:.1}% records, {:.1}% edges",
                cand_ext.avg_record_f1 * 100.0,
                cand_ext.avg_edge_f1 * 100.0
            );
            println!("  Delta:            {:+.1}% records, {:+.1}% edges",
                (cand_ext.avg_record_f1 - base_ext.avg_record_f1) * 100.0,
                (cand_ext.avg_edge_f1 - base_ext.avg_edge_f1) * 100.0
            );
        }

        // Verdict
        println!();
        let verdict_str = match self.verdict {
            ComparisonVerdict::Better => "BETTER - Candidate improves on baseline",
            ComparisonVerdict::Equivalent => "EQUIVALENT - No significant difference",
            ComparisonVerdict::Worse => "WORSE - Candidate regresses from baseline",
            ComparisonVerdict::Mixed => "MIXED - Some improvements, some regressions",
        };
        println!("Verdict: {}", verdict_str);
        println!();
        println!("==========================================\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_status() {
        // Improved case
        let diff = TestDiff {
            test_name: "test".to_string(),
            baseline_f1: 0.7,
            candidate_f1: 0.85,
            delta: 0.15,
            status: DiffStatus::Improved,
        };
        assert_eq!(diff.status, DiffStatus::Improved);

        // Regressed case
        let diff = TestDiff {
            test_name: "test".to_string(),
            baseline_f1: 0.85,
            candidate_f1: 0.7,
            delta: -0.15,
            status: DiffStatus::Regressed,
        };
        assert_eq!(diff.status, DiffStatus::Regressed);
    }

    #[test]
    fn test_comparative_result() {
        let result = ComparativeResult::new();
        assert!(result.branches.is_empty());
        assert!(result.comparisons.is_empty());
    }
}
