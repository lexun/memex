//! Memex evaluation runner
//!
//! Runs evaluation scenarios against a memex instance and reports results.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use evals::{load_scenarios_from_dir, scenario::builtin_scenarios, Judge, Scenario, TestHarness, ExtractionTest, ExtractedRecord, ExtractedEdge};

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
    /// Run evaluation scenarios
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

    /// Run extraction tests
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
        Commands::Extraction { fixture, clean, verbose, multi_step } => {
            run_extraction_test(&socket_path, &fixture, clean, verbose, multi_step).await
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

    // Evaluate
    let result = test.evaluate(&extracted_records, &extracted_edges);
    result.print_summary();

    // Return error if not perfect
    if result.record_accuracy < 1.0 || result.edge_accuracy < 1.0 {
        std::process::exit(1);
    }

    Ok(())
}
