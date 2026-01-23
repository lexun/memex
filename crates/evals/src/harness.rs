//! Test harness for running evaluation scenarios
//!
//! Sets up the environment, executes scenarios, and collects results.
//! Supports both traditional scenarios and snapshot-based scenarios
//! that load pre-built knowledge bases.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use crate::judge::{EvalResult, Judge};
use crate::scenario::{Action, Scenario, SnapshotScenario};
use crate::snapshot::{Snapshot, SnapshotLoader};

/// Results from running a scenario
#[derive(Debug)]
pub struct ScenarioResult {
    /// Name of the scenario
    pub name: String,
    /// Results for each query
    pub query_results: Vec<EvalResult>,
    /// Whether the scenario passed overall
    pub passed: bool,
    /// Number of queries that passed
    pub passed_count: usize,
    /// Total number of queries
    pub total_count: usize,
}

impl ScenarioResult {
    /// Calculate pass rate as percentage
    pub fn pass_rate(&self) -> f32 {
        if self.total_count == 0 {
            0.0
        } else {
            (self.passed_count as f32 / self.total_count as f32) * 100.0
        }
    }
}

/// Complete evaluation report
#[derive(Debug)]
pub struct EvalReport {
    /// Results for each scenario
    pub scenarios: Vec<ScenarioResult>,
    /// Overall statistics
    pub total_scenarios: usize,
    pub passed_scenarios: usize,
    pub total_queries: usize,
    pub passed_queries: usize,
}

impl EvalReport {
    /// Calculate overall pass rate
    pub fn overall_pass_rate(&self) -> f32 {
        if self.total_queries == 0 {
            0.0
        } else {
            (self.passed_queries as f32 / self.total_queries as f32) * 100.0
        }
    }

    /// Print a summary of the report
    pub fn print_summary(&self) {
        println!("\n========== EVALUATION REPORT ==========\n");
        println!(
            "Scenarios: {}/{} passed ({:.1}%)",
            self.passed_scenarios,
            self.total_scenarios,
            if self.total_scenarios == 0 {
                0.0
            } else {
                (self.passed_scenarios as f32 / self.total_scenarios as f32) * 100.0
            }
        );
        println!(
            "Queries: {}/{} passed ({:.1}%)",
            self.passed_queries,
            self.total_queries,
            self.overall_pass_rate()
        );

        println!("\n---------- Scenario Details ----------\n");
        for result in &self.scenarios {
            let status = if result.passed { "PASS" } else { "FAIL" };
            println!(
                "[{}] {} - {}/{} queries passed ({:.1}%)",
                status,
                result.name,
                result.passed_count,
                result.total_count,
                result.pass_rate()
            );

            // Show failed queries
            for qr in &result.query_results {
                if !qr.passed() {
                    println!("  FAILED: \"{}\"", qr.query);
                    println!("    Scores: relevance={}, completeness={}, accuracy={}, coherence={}, overall={}",
                        qr.scores.relevance, qr.scores.completeness, qr.scores.accuracy,
                        qr.scores.coherence, qr.scores.overall);
                    if !qr.missing_mentions.is_empty() {
                        println!("    Missing: {:?}", qr.missing_mentions);
                    }
                    if !qr.feedback.is_empty() {
                        println!("    Feedback: {}", qr.feedback);
                    }
                }
            }
        }
        println!("\n========================================\n");
    }
}

/// Test harness for running evaluation scenarios
pub struct TestHarness {
    socket_path: std::path::PathBuf,
    judge: Judge,
    /// Map of task titles to IDs (for referencing tasks in scenarios)
    task_ids: HashMap<String, String>,
}

impl TestHarness {
    /// Create a new test harness
    pub fn new(socket_path: &Path, judge: Judge) -> Self {
        Self {
            socket_path: socket_path.to_path_buf(),
            judge,
            task_ids: HashMap::new(),
        }
    }

    /// Run a single scenario
    pub async fn run_scenario(&mut self, scenario: &Scenario) -> Result<ScenarioResult> {
        info!("Running scenario: {}", scenario.name);

        // Clear task ID map for this scenario
        self.task_ids.clear();

        // Execute setup actions
        for action in &scenario.setup {
            self.execute_action(action).await?;
        }

        // Small delay to ensure extraction completes
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Run queries and evaluate
        let mut query_results = Vec::new();
        for query in &scenario.queries {
            let answer = self.query_knowledge(&query.query).await?;
            let result = self
                .judge
                .evaluate(&query.query, &answer, &query.expected)
                .await?;
            query_results.push(result);
        }

        // Calculate pass/fail
        let passed_count = query_results.iter().filter(|r| r.passed()).count();
        let total_count = query_results.len();
        let passed = passed_count == total_count;

        Ok(ScenarioResult {
            name: scenario.name.clone(),
            query_results,
            passed,
            passed_count,
            total_count,
        })
    }

    /// Run multiple scenarios and generate a report
    pub async fn run_scenarios(&mut self, scenarios: &[Scenario]) -> Result<EvalReport> {
        let mut results = Vec::new();

        for scenario in scenarios {
            let result = self.run_scenario(scenario).await?;
            results.push(result);
        }

        let total_scenarios = results.len();
        let passed_scenarios = results.iter().filter(|r| r.passed).count();
        let total_queries: usize = results.iter().map(|r| r.total_count).sum();
        let passed_queries: usize = results.iter().map(|r| r.passed_count).sum();

        Ok(EvalReport {
            scenarios: results,
            total_scenarios,
            passed_scenarios,
            total_queries,
            passed_queries,
        })
    }

    /// Execute a setup action
    async fn execute_action(&mut self, action: &Action) -> Result<()> {
        match action {
            Action::RecordMemo { content, delay_secs } => {
                if *delay_secs > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(*delay_secs)).await;
                }
                self.record_memo(content).await?;
            }
            Action::CreateTask {
                title,
                description,
                project,
            } => {
                let task_id = self
                    .create_task(title, description.as_deref(), project.as_deref())
                    .await?;
                self.task_ids.insert(title.clone(), task_id);
            }
            Action::AddTaskNote { task_title, content } => {
                if let Some(task_id) = self.task_ids.get(task_title) {
                    self.add_task_note(task_id, content).await?;
                } else {
                    anyhow::bail!("Task not found: {}", task_title);
                }
            }
            Action::CloseTask { task_title, reason } => {
                if let Some(task_id) = self.task_ids.get(task_title) {
                    self.close_task(task_id, reason.as_deref()).await?;
                } else {
                    anyhow::bail!("Task not found: {}", task_title);
                }
            }
        }
        Ok(())
    }

    /// Record a memo via the daemon
    async fn record_memo(&self, content: &str) -> Result<String> {
        let client = atlas::MemoClient::new(&self.socket_path);
        let memo = client
            .record_memo(content, true, Some("eval:harness"))
            .await
            .context("Failed to record memo")?;
        Ok(memo.id_str().unwrap_or_default())
    }

    /// Query knowledge via the daemon
    async fn query_knowledge(&self, query: &str) -> Result<String> {
        let client = atlas::KnowledgeClient::new(&self.socket_path);
        let result = client
            .query(query, None, Some(10))
            .await
            .context("Failed to query knowledge")?;
        Ok(result.answer)
    }

    /// Create a task via the daemon
    async fn create_task(
        &self,
        title: &str,
        description: Option<&str>,
        project: Option<&str>,
    ) -> Result<String> {
        let client = forge::TaskClient::new(&self.socket_path);
        let mut task = forge::Task::new(title);
        if let Some(desc) = description {
            task.description = Some(desc.to_string());
        }
        if let Some(proj) = project {
            task.project = Some(proj.to_string());
        }
        let created = client
            .create_task(task)
            .await
            .context("Failed to create task")?;
        Ok(created.id_str().unwrap_or_default())
    }

    /// Add a note to a task via the daemon
    async fn add_task_note(&self, task_id: &str, content: &str) -> Result<()> {
        let client = forge::TaskClient::new(&self.socket_path);
        client
            .add_note(task_id, content)
            .await
            .context("Failed to add task note")?;
        Ok(())
    }

    /// Close a task via the daemon
    async fn close_task(&self, task_id: &str, reason: Option<&str>) -> Result<()> {
        let client = forge::TaskClient::new(&self.socket_path);
        client
            .close_task(task_id, None, reason)
            .await
            .context("Failed to close task")?;
        Ok(())
    }

    // =========================================================================
    // Snapshot-based scenario support
    // =========================================================================

    /// Run a snapshot-based scenario
    ///
    /// This loads a pre-built knowledge base from a snapshot file, applies
    /// any update actions, and then runs queries to evaluate the results.
    pub async fn run_snapshot_scenario(
        &mut self,
        scenario: &SnapshotScenario,
        fixtures_dir: &Path,
    ) -> Result<ScenarioResult> {
        info!("Running snapshot scenario: {}", scenario.name);

        // Clear task ID map for this scenario
        self.task_ids.clear();

        // Resolve snapshot path relative to fixtures directory
        let snapshot_path = fixtures_dir.join(&scenario.snapshot_path);
        info!("Loading snapshot: {}", snapshot_path.display());

        // Load the snapshot
        let snapshot = Snapshot::load(&snapshot_path)
            .with_context(|| format!("Failed to load snapshot: {}", snapshot_path.display()))?;

        info!(
            "Snapshot loaded: {} ({} memos, {} records, {} edges)",
            snapshot.meta.name,
            snapshot.memos.len(),
            snapshot.records.len(),
            snapshot.edges.len()
        );

        // Apply snapshot to database
        let loader = SnapshotLoader::new(&self.socket_path);
        let load_result = loader.load(&snapshot).await?;
        info!("Snapshot applied: {}", load_result);

        if !load_result.success() {
            tracing::warn!("Snapshot had {} errors during load", load_result.errors.len());
            for err in &load_result.errors {
                tracing::warn!("  - {}", err);
            }
        }

        // Apply update actions
        for action in &scenario.updates {
            self.execute_action(action).await?;
        }

        // Small delay to ensure extraction completes
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Run queries and evaluate
        let mut query_results = Vec::new();
        for query in &scenario.queries {
            let answer = self.query_knowledge(&query.query).await?;
            let result = self
                .judge
                .evaluate(&query.query, &answer, &query.expected)
                .await?;
            query_results.push(result);
        }

        // Calculate pass/fail
        let passed_count = query_results.iter().filter(|r| r.passed()).count();
        let total_count = query_results.len();
        let passed = passed_count == total_count;

        Ok(ScenarioResult {
            name: scenario.name.clone(),
            query_results,
            passed,
            passed_count,
            total_count,
        })
    }

    /// Run multiple snapshot scenarios and generate a report
    pub async fn run_snapshot_scenarios(
        &mut self,
        scenarios: &[SnapshotScenario],
        fixtures_dir: &Path,
    ) -> Result<EvalReport> {
        let mut results = Vec::new();

        for scenario in scenarios {
            let result = self.run_snapshot_scenario(scenario, fixtures_dir).await?;
            results.push(result);
        }

        let total_scenarios = results.len();
        let passed_scenarios = results.iter().filter(|r| r.passed).count();
        let total_queries: usize = results.iter().map(|r| r.total_count).sum();
        let passed_queries: usize = results.iter().map(|r| r.passed_count).sum();

        Ok(EvalReport {
            scenarios: results,
            total_scenarios,
            passed_scenarios,
            total_queries,
            passed_queries,
        })
    }
}
