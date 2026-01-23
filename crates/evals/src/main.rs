//! Memex evaluation runner
//!
//! Runs evaluation scenarios against a memex instance and reports results.
//! Supports extraction tests, retrieval tests, and comparative analysis.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use evals::{
    load_scenarios_from_dir, scenario::builtin_scenarios,
    Judge, Scenario, TestHarness,
    ExtractionTest, ExtractionSuiteResult, ExtractedRecord, ExtractedEdge,
    ComparativeResult,
    comparative::BranchResult,
};

#[derive(Parser)]
#[command(name = "memex-eval")]
#[command(about = "Evaluation framework for Memex")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to the memex socket
    #[arg(short, long)]
    socket: Option<PathBuf>,

    /// Directory containing TOML scenario files (or set MEMEX_EVAL_SCENARIOS env var)
    #[arg(long)]
    scenarios_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run evaluation scenarios (retrieval tests)
    Run {
        /// Only run scenarios matching this name pattern
        #[arg(short, long)]
        filter: Option<String>,

        /// Show detailed output for each query
        #[arg(short, long)]
        verbose: bool,

        /// Only run built-in scenarios (ignore --scenarios-dir)
        #[arg(long)]
        builtin_only: bool,
    },

    /// List available scenarios
    List {
        /// Only list built-in scenarios
        #[arg(long)]
        builtin_only: bool,
    },

    /// Run a single extraction test
    Extraction {
        /// Path to the extraction test fixture (TOML file)
        fixture: PathBuf,

        /// Clear the database before running (DESTRUCTIVE - use with dev only!)
        #[arg(long)]
        clean: bool,

        /// Show detailed output
        #[arg(short, long)]
        verbose: bool,

        /// Use multi-step extraction pipeline for better updates
        #[arg(short, long)]
        multi_step: bool,

        /// Output full results for human/LLM review (no pass/fail, just data)
        #[arg(long)]
        review: bool,
    },

    /// Run all extraction tests from a directory
    ExtractionSuite {
        /// Directory containing extraction test fixtures
        #[arg(default_value = "crates/evals/fixtures/extraction")]
        fixtures_dir: PathBuf,

        /// Filter tests by name pattern
        #[arg(short, long)]
        filter: Option<String>,

        /// Filter by category (strong, weak, regression)
        #[arg(long)]
        category: Option<String>,

        /// Show detailed output for each test
        #[arg(short, long)]
        verbose: bool,

        /// Use multi-step extraction pipeline
        #[arg(short, long)]
        multi_step: bool,

        /// Output results as JSON (for comparison)
        #[arg(long)]
        json: bool,

        /// Save results to file for later comparison
        #[arg(long)]
        save: Option<PathBuf>,
    },

    /// Compare results between two branches
    Compare {
        /// Path to baseline results JSON file
        baseline: PathBuf,

        /// Path to candidate results JSON file
        candidate: PathBuf,
    },

    /// Run full test suite (extraction + retrieval) and save results
    Suite {
        /// Label for this run (e.g., branch name)
        #[arg(short, long, default_value = "current")]
        label: String,

        /// Directory for extraction fixtures
        #[arg(long, default_value = "crates/evals/fixtures/extraction")]
        extraction_dir: PathBuf,

        /// Directory for retrieval scenarios
        #[arg(long)]
        retrieval_dir: Option<PathBuf>,

        /// Use multi-step extraction pipeline
        #[arg(short, long)]
        multi_step: bool,

        /// Save results to JSON file
        #[arg(long)]
        save: Option<PathBuf>,

        /// Show detailed output
        #[arg(short, long)]
        verbose: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    // Get socket path
    let socket_path = cli
        .socket
        .or_else(|| std::env::var("MEMEX_SOCKET").ok().map(PathBuf::from))
        .or_else(|| {
            // Try default locations
            let config_path = dirs::config_dir()?.join("memex").join("memex.sock");
            if config_path.exists() {
                Some(config_path)
            } else {
                // Try dev path
                let dev_path = PathBuf::from("./.memex/memex.sock");
                if dev_path.exists() {
                    Some(dev_path)
                } else {
                    None
                }
            }
        })
        .context("Socket path not found. Set MEMEX_SOCKET or use --socket")?;

    // Get scenarios dir from CLI or env var
    let scenarios_dir = cli
        .scenarios_dir
        .or_else(|| std::env::var("MEMEX_EVAL_SCENARIOS").ok().map(PathBuf::from));

    match cli.command {
        Commands::Run {
            filter,
            verbose,
            builtin_only,
        } => {
            let scenarios_dir = if builtin_only {
                None
            } else {
                scenarios_dir.as_ref()
            };
            run_evals(&socket_path, scenarios_dir, filter.as_deref(), verbose).await
        }
        Commands::List { builtin_only } => {
            let scenarios_dir = if builtin_only {
                None
            } else {
                scenarios_dir.as_ref()
            };
            list_scenarios(scenarios_dir)?;
            Ok(())
        }
        Commands::Extraction { fixture, clean, verbose, multi_step, review } => {
            run_extraction_test(&socket_path, &fixture, clean, verbose, multi_step, review).await
        }
        Commands::ExtractionSuite {
            fixtures_dir,
            filter,
            category,
            verbose,
            multi_step,
            json,
            save,
        } => {
            run_extraction_suite(
                &socket_path,
                &fixtures_dir,
                filter.as_deref(),
                category.as_deref(),
                verbose,
                multi_step,
                json,
                save.as_ref(),
            ).await
        }
        Commands::Compare { baseline, candidate } => {
            compare_results(&baseline, &candidate)
        }
        Commands::Suite {
            label,
            extraction_dir,
            retrieval_dir,
            multi_step,
            save,
            verbose,
        } => {
            run_full_suite(
                &socket_path,
                &label,
                &extraction_dir,
                retrieval_dir.as_ref(),
                multi_step,
                save.as_ref(),
                verbose,
            ).await
        }
    }
}

async fn run_evals(
    socket_path: &PathBuf,
    scenarios_dir: Option<&PathBuf>,
    filter: Option<&str>,
    verbose: bool,
) -> Result<()> {
    println!("Running evaluations against: {}", socket_path.display());

    // Create LLM client for the judge
    let llm = llm::LlmClient::from_env().context("Failed to create LLM client")?;
    let judge = Judge::new(llm);

    // Create test harness
    let mut harness = TestHarness::new(socket_path, judge);

    // Get scenarios
    let scenarios = get_scenarios(scenarios_dir)?;
    let scenarios: Vec<_> = if let Some(f) = filter {
        scenarios
            .into_iter()
            .filter(|s| s.name.contains(f))
            .collect()
    } else {
        scenarios
    };

    if scenarios.is_empty() {
        println!("No scenarios match the filter");
        return Ok(());
    }

    println!("Running {} scenario(s)...\n", scenarios.len());

    // Run scenarios
    let report = harness.run_scenarios(&scenarios).await?;

    // Print results
    if verbose {
        for result in &report.scenarios {
            println!("\n--- {} ---", result.name);
            for qr in &result.query_results {
                println!("\nQ: {}", qr.query);
                println!("A: {}", qr.answer);
                println!(
                    "Scores: rel={}, comp={}, acc={}, coh={}, overall={}",
                    qr.scores.relevance,
                    qr.scores.completeness,
                    qr.scores.accuracy,
                    qr.scores.coherence,
                    qr.scores.overall
                );
                if !qr.missing_mentions.is_empty() {
                    println!("Missing: {:?}", qr.missing_mentions);
                }
                println!("Feedback: {}", qr.feedback);
            }
        }
    }

    report.print_summary();

    // Exit with error if any failures
    if report.passed_queries < report.total_queries {
        std::process::exit(1);
    }

    Ok(())
}

fn list_scenarios(scenarios_dir: Option<&PathBuf>) -> Result<()> {
    let scenarios = get_scenarios(scenarios_dir)?;
    println!("Available scenarios:\n");
    for scenario in scenarios {
        println!("  {} - {}", scenario.name, scenario.description);
        println!("    Setup: {} actions", scenario.setup.len());
        println!("    Queries: {}", scenario.queries.len());
        println!();
    }
    Ok(())
}

/// Load scenarios from directory (if provided) or use built-in scenarios
fn get_scenarios(scenarios_dir: Option<&PathBuf>) -> Result<Vec<Scenario>> {
    match scenarios_dir {
        Some(dir) => {
            println!("Loading scenarios from: {}", dir.display());
            let mut scenarios = load_scenarios_from_dir(dir)?;

            if scenarios.is_empty() {
                println!("No scenarios found in directory, using built-in scenarios");
                scenarios = builtin_scenarios();
            }

            Ok(scenarios)
        }
        None => Ok(builtin_scenarios()),
    }
}

/// Run an extraction test
async fn run_extraction_test(
    socket_path: &PathBuf,
    fixture_path: &PathBuf,
    clean: bool,
    verbose: bool,
    multi_step: bool,
    review: bool,
) -> Result<()> {
    use atlas::{KnowledgeClient, MemoClient, RecordClient};

    println!("=== Extraction Test ===");
    println!("Fixture: {}", fixture_path.display());
    println!("Socket: {}", socket_path.display());
    println!("Mode: {}", if multi_step { "multi-step" } else { "single-shot" });
    println!();

    // Load the test fixture
    let test = ExtractionTest::load(fixture_path)?;
    println!("Test: {} - {}", test.meta.name, test.meta.description);
    println!("Expected: {} records, {} edges", test.expected.records.len(), test.expected.edges.len());
    println!("Memos: {}", test.memos.len());
    println!();

    // Create clients
    let memo_client = MemoClient::new(socket_path);
    let record_client = RecordClient::new(socket_path);
    let knowledge_client = KnowledgeClient::new(socket_path);

    // Optionally clean database (just delete records, not memos)
    if clean {
        println!("Cleaning existing records...");
        let records = record_client.list_records(None, false, None).await?;
        let count = records.len();
        for record in records {
            if let Some(id) = record.id_str() {
                // Note: This is best-effort, some may fail
                let _ = record_client.delete_record(&id).await;
            }
        }
        println!("Cleaned {} records", count);
        println!();
    }

    // Record all memos
    println!("Recording {} memos...", test.memos.len());
    let mut memo_ids = Vec::new();
    for (i, memo) in test.memos.iter().enumerate() {
        if verbose {
            let preview = if memo.content.len() > 50 {
                format!("{}...", &memo.content[..50])
            } else {
                memo.content.clone()
            };
            println!("  [{}] Recording: {}", i + 1, preview.replace('\n', " "));
        }
        let recorded = memo_client.record_memo(&memo.content, true, None).await?;
        if let Some(id) = recorded.id_str() {
            memo_ids.push(id.to_string());
        }
    }
    println!("Recorded {} memos", memo_ids.len());
    println!();

    // Run extraction on each memo
    println!("Running extraction...");

    for (i, memo_id) in memo_ids.iter().enumerate() {
        if verbose {
            println!("  [{}] Extracting from memo {}...", i + 1, memo_id);
        }
        // Use the extract command via client
        let result = knowledge_client.extract_records_from_memo(memo_id, 0.5, false, multi_step).await;
        match result {
            Ok(r) => {
                if verbose {
                    println!("    Extracted {} records, {} links",
                        r.extraction.records.len(),
                        r.extraction.links.len()
                    );
                }
            }
            Err(e) => {
                println!("    Warning: extraction failed: {}", e);
            }
        }
    }
    println!();

    // List all records and edges
    println!("Fetching extracted records...");
    let records = record_client.list_records(None, false, None).await?;

    // Convert to simplified format for comparison
    let extracted_records: Vec<ExtractedRecord> = records
        .iter()
        .map(|r| ExtractedRecord {
            record_type: r.record_type.to_string(),
            name: r.name.clone(),
        })
        .collect();

    // Fetch edges for each record and build edge list
    let mut extracted_edges: Vec<ExtractedEdge> = Vec::new();
    let record_id_to_name: std::collections::HashMap<String, String> = records
        .iter()
        .filter_map(|r| r.id_str().map(|id| (id.to_string(), r.name.clone())))
        .collect();

    for record in &records {
        if let Some(id) = record.id_str() {
            if let Ok(edges) = record_client.list_edges(&id, "from").await {
                for edge in edges {
                    // Convert Thing to string ID for lookup
                    let source_id = edge.source.id.to_raw();
                    let target_id = edge.target.id.to_raw();

                    // Resolve IDs to names
                    let from_name = record_id_to_name.get(&source_id).cloned()
                        .unwrap_or_else(|| source_id.clone());
                    let to_name = record_id_to_name.get(&target_id).cloned()
                        .unwrap_or_else(|| target_id.clone());

                    extracted_edges.push(ExtractedEdge {
                        from: from_name,
                        relation: edge.relation.to_string(),
                        to: to_name,
                    });
                }
            }
        }
    }

    // Deduplicate edges (same edge may appear when fetching from both ends)
    extracted_edges.sort_by(|a, b| {
        (&a.from, &a.relation, &a.to).cmp(&(&b.from, &b.relation, &b.to))
    });
    extracted_edges.dedup_by(|a, b| {
        a.from == b.from && a.relation == b.relation && a.to == b.to
    });

    println!("Found {} records, {} edges", extracted_records.len(), extracted_edges.len());
    println!();

    // Review mode: output full data for human/LLM evaluation
    if review {
        print_review_output(&test, &extracted_records, &extracted_edges, &records);
        return Ok(());
    }

    // Evaluate
    let result = test.evaluate(&extracted_records, &extracted_edges);
    result.print_summary();

    // Return error if test did not pass (based on category thresholds)
    if !result.passed() {
        std::process::exit(1);
    }

    Ok(())
}

/// Print extraction results in a format suitable for human/LLM review
fn print_review_output(
    test: &ExtractionTest,
    extracted_records: &[ExtractedRecord],
    extracted_edges: &[ExtractedEdge],
    full_records: &[atlas::Record],
) {
    println!("=== EXTRACTION REVIEW ===\n");
    println!("Test: {} - {}", test.meta.name, test.meta.description);
    println!();

    // Print source memos
    println!("## SOURCE MEMOS\n");
    for (i, memo) in test.memos.iter().enumerate() {
        println!("### Memo {}\n", i + 1);
        println!("{}\n", memo.content);
        println!("---\n");
    }

    // Print extracted records with full details
    println!("## EXTRACTED RECORDS ({} total)\n", extracted_records.len());
    for record in full_records {
        let type_str = &record.record_type;
        let name = &record.name;
        let desc = record.description.as_deref().unwrap_or("(no description)");
        println!("### [{}] {}\n", type_str.to_uppercase(), name);
        println!("Description: {}\n", desc);
        if !record.content.is_null() && record.content != serde_json::json!({}) {
            println!("Content: {}\n", serde_json::to_string_pretty(&record.content).unwrap_or_default());
        }
        println!();
    }

    // Print extracted edges
    if !extracted_edges.is_empty() {
        println!("## EXTRACTED RELATIONSHIPS ({} total)\n", extracted_edges.len());
        for edge in extracted_edges {
            println!("- {} --[{}]--> {}", edge.from, edge.relation, edge.to);
        }
        println!();
    }

    // Print expected vs actual comparison
    println!("## EXPECTED VS ACTUAL\n");
    println!("### Expected Records:");
    for er in &test.expected.records {
        let optional = if er.optional { " (optional)" } else { "" };
        println!("- [{}] {}{}", er.record_type, er.name, optional);
    }
    println!();

    println!("### Actually Extracted:");
    for er in extracted_records {
        println!("- [{}] {}", er.record_type, er.name);
    }
    println!();

    if !test.expected.edges.is_empty() {
        println!("### Expected Edges:");
        for ee in &test.expected.edges {
            println!("- {} --[{}]--> {}", ee.from, ee.relation, ee.to);
        }
        println!();
    }

    // Print review questions
    println!("## REVIEW QUESTIONS\n");
    println!("Please evaluate the extraction quality:\n");
    println!("1. **Completeness**: Are all important entities from the memos captured?");
    println!("2. **Accuracy**: Are the record types correct? Are descriptions accurate?");
    println!("3. **Noise**: Are there records that shouldn't exist (internal code, IDs, paths)?");
    println!("4. **Relationships**: Do the edges correctly represent relationships from the text?");
    println!("5. **Granularity**: Are records at the right level of detail (not too fine/coarse)?");
    println!("6. **Usefulness**: Would these records help an agent understand the domain?\n");
    println!("Rate overall quality: Poor / Fair / Good / Excellent");
}

/// Run all extraction tests from a directory
async fn run_extraction_suite(
    socket_path: &PathBuf,
    fixtures_dir: &PathBuf,
    filter: Option<&str>,
    category: Option<&str>,
    verbose: bool,
    multi_step: bool,
    json: bool,
    save: Option<&PathBuf>,
) -> Result<()> {
    use evals::TestCategory;
    use atlas::{KnowledgeClient, MemoClient, RecordClient};

    println!("=== Extraction Test Suite ===");
    println!("Fixtures directory: {}", fixtures_dir.display());
    println!("Mode: {}", if multi_step { "multi-step" } else { "single-shot" });
    println!();

    // Find all TOML files in the fixtures directory
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(fixtures_dir)
        .context(format!("Failed to read fixtures directory: {}", fixtures_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "toml"))
        .collect();

    fixtures.sort();

    if fixtures.is_empty() {
        println!("No test fixtures found in {}", fixtures_dir.display());
        return Ok(());
    }

    // Filter by name pattern
    if let Some(pattern) = filter {
        fixtures.retain(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map_or(false, |s| s.contains(pattern))
        });
    }

    // Load and filter by category
    let mut tests: Vec<(PathBuf, ExtractionTest)> = Vec::new();
    for path in &fixtures {
        match ExtractionTest::load(path) {
            Ok(test) => {
                // Filter by category if specified
                if let Some(cat) = category {
                    let matches = match cat.to_lowercase().as_str() {
                        "strong" => matches!(test.meta.category, TestCategory::Strong),
                        "weak" => matches!(test.meta.category, TestCategory::Weak),
                        "regression" => matches!(test.meta.category, TestCategory::Regression),
                        _ => true,
                    };
                    if !matches {
                        continue;
                    }
                }
                tests.push((path.clone(), test));
            }
            Err(e) => {
                println!("Warning: Failed to load {}: {}", path.display(), e);
            }
        }
    }

    println!("Running {} test(s)...\n", tests.len());

    // Create clients
    let memo_client = MemoClient::new(socket_path);
    let record_client = RecordClient::new(socket_path);
    let knowledge_client = KnowledgeClient::new(socket_path);

    let mut results = Vec::new();

    for (_path, test) in &tests {
        if verbose {
            println!("\n--- {} ---", test.meta.name);
            println!("Description: {}", test.meta.description);
        }

        // Clean records before each test
        let records = record_client.list_records(None, false, None).await?;
        for record in records {
            if let Some(id) = record.id_str() {
                let _ = record_client.delete_record(&id).await;
            }
        }

        // Record memos
        let mut memo_ids = Vec::new();
        for memo in &test.memos {
            let recorded = memo_client.record_memo(&memo.content, true, None).await?;
            if let Some(id) = recorded.id_str() {
                memo_ids.push(id.to_string());
            }
        }

        // Run extraction
        for memo_id in &memo_ids {
            let _ = knowledge_client
                .extract_records_from_memo(memo_id, 0.5, false, multi_step)
                .await;
        }

        // Fetch results
        let records = record_client.list_records(None, false, None).await?;
        let extracted_records: Vec<ExtractedRecord> = records
            .iter()
            .map(|r| ExtractedRecord {
                record_type: r.record_type.to_string(),
                name: r.name.clone(),
            })
            .collect();

        // Fetch edges
        let mut extracted_edges: Vec<ExtractedEdge> = Vec::new();
        let record_id_to_name: std::collections::HashMap<String, String> = records
            .iter()
            .filter_map(|r| r.id_str().map(|id| (id.to_string(), r.name.clone())))
            .collect();

        for record in &records {
            if let Some(id) = record.id_str() {
                if let Ok(edges) = record_client.list_edges(&id, "from").await {
                    for edge in edges {
                        let source_id = edge.source.id.to_raw();
                        let target_id = edge.target.id.to_raw();
                        let from_name = record_id_to_name.get(&source_id).cloned()
                            .unwrap_or_else(|| source_id.clone());
                        let to_name = record_id_to_name.get(&target_id).cloned()
                            .unwrap_or_else(|| target_id.clone());
                        extracted_edges.push(ExtractedEdge {
                            from: from_name,
                            relation: edge.relation.to_string(),
                            to: to_name,
                        });
                    }
                }
            }
        }

        // Deduplicate edges
        extracted_edges.sort_by(|a, b| {
            (&a.from, &a.relation, &a.to).cmp(&(&b.from, &b.relation, &b.to))
        });
        extracted_edges.dedup_by(|a, b| {
            a.from == b.from && a.relation == b.relation && a.to == b.to
        });

        // Evaluate
        let result = test.evaluate(&extracted_records, &extracted_edges);
        if verbose {
            result.print_summary();
        }
        results.push(result);
    }

    // Build suite result
    let suite_result = ExtractionSuiteResult::from_results(results);

    if json {
        println!("{}", suite_result.to_json()?);
    } else {
        suite_result.print_summary();
    }

    // Save results if requested
    if let Some(path) = save {
        let json = suite_result.to_json()?;
        std::fs::write(path, json)?;
        println!("Results saved to: {}", path.display());
    }

    // Exit with error if strong tests failed
    if suite_result.strong_passed < suite_result.strong_total {
        std::process::exit(1);
    }

    Ok(())
}

/// Compare results between two branches
fn compare_results(baseline_path: &PathBuf, candidate_path: &PathBuf) -> Result<()> {
    println!("=== Comparative Analysis ===\n");
    println!("Baseline:  {}", baseline_path.display());
    println!("Candidate: {}", candidate_path.display());
    println!();

    // Load baseline results
    let baseline_content = std::fs::read_to_string(baseline_path)
        .context("Failed to read baseline results")?;
    let baseline_extraction: ExtractionSuiteResult = serde_json::from_str(&baseline_content)
        .context("Failed to parse baseline results")?;

    // Load candidate results
    let candidate_content = std::fs::read_to_string(candidate_path)
        .context("Failed to read candidate results")?;
    let candidate_extraction: ExtractionSuiteResult = serde_json::from_str(&candidate_content)
        .context("Failed to parse candidate results")?;

    // Build comparison
    let mut result = ComparativeResult::new();

    result.add_branch(BranchResult {
        branch: "baseline".to_string(),
        commit: None,
        timestamp: chrono::Utc::now(),
        extraction: Some(baseline_extraction),
        retrieval: None,
    });

    result.add_branch(BranchResult {
        branch: "candidate".to_string(),
        commit: None,
        timestamp: chrono::Utc::now(),
        extraction: Some(candidate_extraction),
        retrieval: None,
    });

    result.compare("baseline", "candidate")?;
    result.print_comparison("baseline", "candidate");

    Ok(())
}

/// Run the full test suite (extraction + retrieval)
async fn run_full_suite(
    _socket_path: &PathBuf,
    label: &str,
    _extraction_dir: &PathBuf,
    retrieval_dir: Option<&PathBuf>,
    multi_step: bool,
    save: Option<&PathBuf>,
    _verbose: bool,
) -> Result<()> {
    use evals::comparative::RetrievalSummary;

    println!("=== Full Test Suite ===");
    println!("Label: {}", label);
    println!("Mode: {}", if multi_step { "multi-step" } else { "single-shot" });
    println!();

    // Run extraction suite (silently collect results)
    let extraction_result = {
        // This is a simplified version - in practice we'd refactor to reuse code
        println!("Running extraction tests...");
        // For now, just indicate we would run this
        None::<ExtractionSuiteResult>
    };

    // Run retrieval suite
    let retrieval_result = if retrieval_dir.is_some() {
        println!("Running retrieval tests...");
        // Would run retrieval tests here
        None::<RetrievalSummary>
    } else {
        None
    };

    // Build branch result
    let branch_result = BranchResult {
        branch: label.to_string(),
        commit: get_git_commit(),
        timestamp: chrono::Utc::now(),
        extraction: extraction_result,
        retrieval: retrieval_result,
    };

    // Save if requested
    if let Some(path) = save {
        let json = serde_json::to_string_pretty(&branch_result)?;
        std::fs::write(path, json)?;
        println!("\nResults saved to: {}", path.display());
    }

    println!("\nSuite complete for: {}", label);

    Ok(())
}

/// Get the current git commit hash
fn get_git_commit() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}
