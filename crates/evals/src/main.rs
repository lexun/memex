//! Memex evaluation runner
//!
//! Runs evaluation scenarios against a memex instance and reports results.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use evals::{load_scenarios_from_dir, scenario::builtin_scenarios, Judge, Scenario, TestHarness};

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
