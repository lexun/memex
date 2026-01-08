//! Memex evaluation runner
//!
//! Runs evaluation scenarios against a memex instance and reports results.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use evals::{scenario::builtin_scenarios, Judge, TestHarness};

#[derive(Parser)]
#[command(name = "memex-eval")]
#[command(about = "Evaluation framework for Memex")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to the memex socket
    #[arg(short, long)]
    socket: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run all built-in evaluation scenarios
    Run {
        /// Only run scenarios matching this name pattern
        #[arg(short, long)]
        filter: Option<String>,

        /// Show detailed output for each query
        #[arg(short, long)]
        verbose: bool,
    },

    /// List available scenarios
    List,
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

    match cli.command {
        Commands::Run { filter, verbose } => {
            run_evals(&socket_path, filter.as_deref(), verbose).await
        }
        Commands::List => {
            list_scenarios();
            Ok(())
        }
    }
}

async fn run_evals(socket_path: &PathBuf, filter: Option<&str>, verbose: bool) -> Result<()> {
    println!("Running evaluations against: {}", socket_path.display());

    // Create LLM client for the judge
    let llm = llm::LlmClient::from_env().context("Failed to create LLM client")?;
    let judge = Judge::new(llm);

    // Create test harness
    let mut harness = TestHarness::new(socket_path, judge);

    // Get scenarios
    let scenarios = builtin_scenarios();
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

fn list_scenarios() {
    let scenarios = builtin_scenarios();
    println!("Available scenarios:\n");
    for scenario in scenarios {
        println!("  {} - {}", scenario.name, scenario.description);
        println!("    Setup: {} actions", scenario.setup.len());
        println!("    Queries: {}", scenario.queries.len());
        println!();
    }
}
