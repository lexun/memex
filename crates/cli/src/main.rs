mod agent;
mod completions;
mod config;
mod daemon;
mod help;
mod launchd;
mod pid;
mod web;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::env::CompleteEnv;

/// Supported shells for completion generation
#[derive(Clone, Debug, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
    /// Generate carapace spec (YAML)
    Carapace,
    /// Auto-detect and install completions
    Install,
}

#[derive(Parser)]
#[command(name = "memex")]
#[command(about = "A knowledge management system for AI")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(flatten)]
    Knowledge(KnowledgeCommands),

    #[command(flatten)]
    Tasks(TaskCommands),

    #[command(flatten)]
    System(SystemCommands),
}

#[derive(Subcommand)]
enum KnowledgeCommands {
    /// Record a memo to the knowledge base
    #[command(display_order = 1)]
    Record {
        /// The content to record
        content: String,
    },
    /// Query the knowledge base (LLM-summarized answer)
    #[command(display_order = 2)]
    Query {
        /// The query to answer
        query: String,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Search for facts in the knowledge base
    #[command(display_order = 3)]
    Search {
        /// The search query
        query: String,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,

        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// Manage memos
    #[command(display_order = 4)]
    Memo {
        #[command(subcommand)]
        action: atlas::MemoCommand,
    },
    /// View events
    #[command(display_order = 5)]
    Event {
        #[command(subcommand)]
        action: atlas::EventCommand,
    },
    /// Manage graph records and edges
    #[command(display_order = 6)]
    Graph {
        #[command(subcommand)]
        action: atlas::RecordCommand,
    },
    /// Rebuild knowledge from memos
    #[command(display_order = 7)]
    Rebuild {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Backfill missing embeddings without rebuilding
    #[command(display_order = 8)]
    Backfill {
        /// Number of facts to process per batch
        #[arg(short, long, default_value = "50")]
        batch_size: usize,

        /// Process all facts (run until none remain)
        #[arg(short, long)]
        all: bool,
    },
    /// Show knowledge system status
    #[command(display_order = 9)]
    Status,
    /// List known entities
    #[command(display_order = 10)]
    Entities {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,

        /// Filter by entity type
        #[arg(short = 't', long)]
        entity_type: Option<String>,

        /// Maximum results
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Get facts about a specific entity
    #[command(display_order = 11)]
    Entity {
        /// Entity name to look up
        name: String,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Extract records from a memo
    #[command(display_order = 12)]
    Extract {
        /// Memo ID to extract from
        memo_id: String,

        /// Confidence threshold (0.0-1.0)
        #[arg(short, long, default_value = "0.5")]
        threshold: f32,

        /// Show what would be extracted without creating records
        #[arg(short, long)]
        dry_run: bool,

        /// Use multi-step extraction for better updates
        #[arg(short, long)]
        multi_step: bool,
    },
    /// Backfill records from all memos
    #[command(name = "backfill-records", display_order = 13)]
    BackfillRecords {
        /// Number of memos to process per batch
        #[arg(short, long, default_value = "50")]
        batch_size: usize,

        /// Confidence threshold for auto-creation (0.0-1.0)
        #[arg(short = 't', long, default_value = "0.5")]
        threshold: f32,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum TaskCommands {
    /// Task management
    #[command(display_order = 10)]
    Task {
        #[command(subcommand)]
        action: forge::TaskCommand,
    },
}

#[derive(Subcommand)]
enum SystemCommands {
    /// Daemon management
    #[command(display_order = 20)]
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Configuration management
    #[command(display_order = 21)]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Initialize memex configuration
    #[command(display_order = 22)]
    Init,
    /// MCP server
    #[command(display_order = 23)]
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Cortex worker management
    #[command(display_order = 24)]
    Cortex {
        #[command(subcommand)]
        action: CortexAction,
    },
    /// Launch Claude agent in zellij session with self-restart capability
    #[command(display_order = 25)]
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Generate shell completions
    #[command(display_order = 26)]
    Completions {
        /// Shell to generate completions for
        shell: CompletionShell,
    },
    /// Upgrade memex via nix and restart daemon if needed
    #[command(display_order = 27)]
    Upgrade,
    /// Data migration commands
    #[command(display_order = 28)]
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Completely purge sensitive data from the system
    #[command(display_order = 29)]
    Purge {
        #[command(subcommand)]
        action: PurgeAction,
    },
}

#[derive(Subcommand)]
enum MigrateAction {
    /// Migrate tasks from Forge to Atlas records
    TasksToRecords,
}

#[derive(Subcommand)]
enum PurgeAction {
    /// Purge a memo and all derived data (events, facts, entities)
    Memo {
        /// Memo ID to purge
        id: String,

        /// Preview what would be deleted without actually deleting
        #[arg(short, long)]
        dry_run: bool,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Purge a record and all derived data (events, facts, entities, edges)
    Record {
        /// Record ID to purge
        id: String,

        /// Preview what would be deleted without actually deleting
        #[arg(short, long)]
        dry_run: bool,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Check daemon status
    Status,
    /// Restart the daemon
    Restart,
    /// Run daemon in foreground (internal use after fork+exec)
    #[command(hide = true)]
    Run,
    /// Enable automatic startup via launchd (macOS)
    Enable,
    /// Disable automatic startup via launchd (macOS)
    Disable,
    /// View daemon logs
    Logs {
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,
    /// Get a configuration value
    Get { key: String },
    /// Set a configuration value
    Set { key: String, value: String },
    /// Show configuration file path
    Path,
}

#[derive(Subcommand)]
enum McpAction {
    /// Start MCP server on stdio
    Serve,
}

#[derive(Subcommand)]
enum CortexAction {
    /// Show status of all workers (dashboard view)
    Status,
    /// List all workers
    List,
    /// Show transcript for a worker
    Transcript {
        /// Worker ID
        worker_id: String,
        /// Maximum number of entries
        #[arg(short, long)]
        limit: Option<usize>,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// Start agent in zellij session (default if no subcommand)
    #[command(display_order = 1)]
    Start {
        /// Zellij session name (default: memex-agent)
        #[arg(short, long)]
        session: Option<String>,

        /// Resume previous Claude conversation
        #[arg(short, long)]
        resume: bool,
    },
    /// Run agent directly (used internally when already in zellij)
    #[command(hide = true)]
    Run {
        /// Resume previous Claude conversation
        #[arg(short, long)]
        resume: bool,
    },
    /// Restart the agent to pick up new MCP tools
    #[command(display_order = 2)]
    Restart,
    /// List active zellij sessions
    #[command(display_order = 3)]
    Sessions,
    /// Kill a zellij session
    #[command(display_order = 4)]
    Kill {
        /// Session name to kill (default: memex-agent)
        session: Option<String>,
    },
}

fn main() -> Result<()> {
    // Handle dynamic shell completions (if triggered by shell completion request)
    CompleteEnv::with_factory(Cli::command).complete();

    // Check for top-level help before parsing
    // This lets us show our custom help while letting subcommands use clap's help
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 || (args.len() == 2 && (args[1] == "-h" || args[1] == "--help")) {
        print!("{}", help::generate_help());
        return Ok(());
    }

    let cli = Cli::parse();

    // If somehow we got here with no command, show help
    let Some(command) = cli.command else {
        print!("{}", help::generate_help());
        return Ok(());
    };

    // Handle completions command synchronously
    if let Commands::System(SystemCommands::Completions { shell }) = &command {
        completions::generate_completions(shell.clone());
        return Ok(());
    }

    // Handle daemon start/restart/run synchronously (before any tokio runtime)
    // This allows proper fork() without runtime conflicts
    match &command {
        Commands::System(SystemCommands::Daemon { action: DaemonAction::Start }) => {
            let daemon = daemon::Daemon::new()?;
            return daemon.start();
        }
        Commands::System(SystemCommands::Daemon { action: DaemonAction::Restart }) => {
            // Need async for stop, so create a temporary runtime
            let cfg = config::load_config()?;
            let pid_path = config::get_pid_file(&cfg)?;
            if pid::check_daemon(&pid_path)?.is_some() {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(async {
                    daemon::stop_daemon().await?;
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    Ok::<_, anyhow::Error>(())
                })?;
                // Runtime is dropped here before fork
            }
            let daemon = daemon::Daemon::new()?;
            return daemon.start();
        }
        Commands::System(SystemCommands::Daemon { action: DaemonAction::Run }) => {
            // Called after fork+exec, run daemon directly
            return daemon::Daemon::run_foreground();
        }
        _ => {}
    }

    // For all other commands, use tokio runtime
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(command))
}

async fn async_main(command: Commands) -> Result<()> {
    // Default to WARN level for quiet CLI output
    // Use RUST_LOG=info or RUST_LOG=debug for verbose output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let cfg = config::load_config()?;
    let socket_path = config::get_socket_path(&cfg)?;

    match command {
        // Knowledge commands
        Commands::Knowledge(cmd) => match cmd {
            KnowledgeCommands::Record { content } => {
                let client = atlas::MemoClient::new(&socket_path);
                let memo = client.record_memo(&content, true, Some("user:default")).await?;
                println!("Recorded: {}", memo.id_str().unwrap_or_default());
                Ok(())
            }
            KnowledgeCommands::Query { query, project } => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let result = client.query(&query, project.as_deref(), Some(10)).await?;
                if result.answer.is_empty() {
                    println!("No relevant knowledge found for: {}", query);
                    println!();
                    println!("Note: Facts are extracted from memos. Try recording some memos first.");
                } else {
                    println!("{}", result.answer);
                }
                Ok(())
            }
            KnowledgeCommands::Search { query, project, limit } => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let result = client.search(&query, project.as_deref(), Some(limit)).await?;
                if result.results.is_empty() {
                    println!("No facts found for: {}", query);
                } else {
                    for fact in &result.results {
                        let score = fact["score"].as_f64().unwrap_or(0.0);
                        let content = fact["content"].as_str().unwrap_or("");
                        println!("[{:.2}] {}", score, content);
                    }
                }
                Ok(())
            }
            KnowledgeCommands::Memo { action } => {
                atlas::handle_memo_command(action, &socket_path).await
            }
            KnowledgeCommands::Event { action } => {
                atlas::handle_event_command(action, &socket_path).await
            }
            KnowledgeCommands::Graph { action } => {
                atlas::handle_record_command(action, &socket_path).await
            }
            KnowledgeCommands::Rebuild { project } => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let result = client.rebuild(project.as_deref()).await?;
                println!("Rebuilt knowledge:");
                println!("  Deleted: {} facts, {} entities", result.facts_deleted, result.entities_deleted);
                println!("  Created: {} facts, {} entities, {} links from {} memos",
                    result.facts_created, result.entities_created, result.links_created, result.memos_processed);
                Ok(())
            }
            KnowledgeCommands::Backfill { batch_size, all } => {
                let client = atlas::KnowledgeClient::new(&socket_path);

                if all {
                    // Process all facts until none remain
                    let mut total_updated = 0;
                    loop {
                        let result = client.backfill_embeddings(Some(batch_size)).await?;
                        total_updated += result.facts_updated;

                        if result.facts_remaining == 0 {
                            println!("Backfill complete: {} facts updated", total_updated);
                            break;
                        }

                        println!("Progress: {} updated, {} remaining...", total_updated, result.facts_remaining);
                    }
                } else {
                    // Process single batch
                    let result = client.backfill_embeddings(Some(batch_size)).await?;
                    println!("Backfill:");
                    println!("  Processed: {}", result.facts_processed);
                    println!("  Updated: {}", result.facts_updated);
                    println!("  Remaining: {}", result.facts_remaining);

                    if result.facts_remaining > 0 {
                        println!();
                        println!("Run 'memex backfill --all' to process all remaining facts.");
                    }
                }
                Ok(())
            }
            KnowledgeCommands::Status => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let status = client.status().await?;

                println!("Knowledge Status:");
                println!("  LLM configured: {}", status.llm_configured);
                println!();
                println!("Facts:");
                println!("  Total: {}", status.facts.total);
                println!("  With embeddings: {}", status.facts.with_embeddings);
                println!("  Without embeddings: {}", status.facts.without_embeddings);

                if status.facts.without_embeddings > 0 && status.llm_configured {
                    println!();
                    println!("Note: {} facts are missing embeddings.", status.facts.without_embeddings);
                    println!("      Run 'memex backfill --all' to add embeddings without rebuilding.");
                }

                if !status.llm_configured {
                    println!();
                    println!("Warning: LLM not configured. Semantic search disabled.");
                    println!("         Set OPENAI_API_KEY or configure llm.api_key.");
                }

                Ok(())
            }
            KnowledgeCommands::Entities { project, entity_type, limit } => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let entities = client.list_entities(project.as_deref(), entity_type.as_deref(), limit).await?;

                if entities.is_empty() {
                    println!("No entities found.");
                    println!();
                    println!("Note: Entities are extracted from memos.");
                } else {
                    println!("Found {} entities:", entities.len());
                    println!();
                    for entity in entities {
                        let id = entity.id_str().unwrap_or_default();
                        println!("[{}] {} ({})", entity.entity_type, entity.name, id);
                        if !entity.description.is_empty() {
                            println!("      {}", entity.description);
                        }
                    }
                }
                Ok(())
            }
            KnowledgeCommands::Entity { name, project } => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let result = client.get_entity_facts(&name, project.as_deref()).await?;

                if result.facts.is_empty() {
                    println!("No facts found for entity: {}", name);
                    println!();
                    println!("The entity may not exist or have no linked facts.");
                } else {
                    println!("Found {} fact(s) about \"{}\":", result.count, result.entity);
                    println!();
                    for fact in result.facts {
                        println!("[{}] {}", fact.fact_type, fact.content);
                    }
                }
                Ok(())
            }
            KnowledgeCommands::Extract { memo_id, threshold, dry_run, multi_step } => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let result = client.extract_records_from_memo(&memo_id, threshold, dry_run, multi_step).await?;

                if dry_run {
                    println!("DRY RUN - Would extract from memo {}:", memo_id);
                } else {
                    println!("Extraction results for memo {}:", memo_id);
                }
                println!();

                // Show extracted records
                if result.extraction.records.is_empty() {
                    println!("No records extracted.");
                } else {
                    println!("Records ({}):", result.extraction.records.len());
                    for record in &result.extraction.records {
                        let action = match record.action {
                            atlas::RecordAction::Create => "CREATE",
                            atlas::RecordAction::Update => "UPDATE",
                            atlas::RecordAction::Reference => "REF",
                        };
                        println!("  [{:.0}%] {} {} \"{}\"",
                            record.confidence * 100.0,
                            action,
                            record.record_type,
                            record.name
                        );
                        if let Some(desc) = &record.description {
                            println!("         {}", desc);
                        }
                    }
                }
                println!();

                // Show extracted links
                if result.extraction.links.is_empty() {
                    println!("No links extracted.");
                } else {
                    println!("Links ({}):", result.extraction.links.len());
                    for link in &result.extraction.links {
                        println!("  [{:.0}%] {} --{}-> {}",
                            link.confidence * 100.0,
                            link.source,
                            link.relation,
                            link.target
                        );
                    }
                }
                println!();

                // Show questions
                if !result.extraction.questions.is_empty() {
                    println!("Questions ({}):", result.extraction.questions.len());
                    for q in &result.extraction.questions {
                        println!("  ? {}", q.text);
                        if let Some(ctx) = &q.context {
                            println!("    Context: {}", ctx);
                        }
                    }
                }

                // Show processing results if not dry run
                if !dry_run {
                    if let Some(processing) = &result.processing {
                        println!();
                        println!("Processing results:");
                        println!("  Created: {} records, {} edges",
                            processing.created_records.len(),
                            processing.created_edges.len()
                        );
                        if !processing.updated_records.is_empty() {
                            println!("  Updated: {} records", processing.updated_records.len());
                        }
                        if !processing.skipped_low_confidence.is_empty() {
                            println!("  Skipped (low confidence): {}", processing.skipped_low_confidence.len());
                        }
                    }
                }

                Ok(())
            }
            KnowledgeCommands::BackfillRecords { batch_size, threshold, yes } => {
                if !yes {
                    println!("This will extract records from all memos.");
                    println!("Use -y/--yes to confirm, or run with --dry-run first.");
                    return Ok(());
                }

                let client = atlas::KnowledgeClient::new(&socket_path);
                println!("Starting record backfill (batch size: {}, threshold: {})...", batch_size, threshold);
                println!();

                let result = client.backfill_records(batch_size, threshold).await?;

                println!("Backfill complete:");
                println!("  Memos processed: {}", result.memos_processed);
                println!("  Records created: {}", result.records_created);
                println!("  Records updated: {}", result.records_updated);
                println!("  Edges created: {}", result.edges_created);
                println!("  Skipped (low confidence): {}", result.skipped_count);

                if !result.questions.is_empty() {
                    println!();
                    println!("Questions for clarification ({}):", result.questions.len());
                    for q in &result.questions {
                        println!("  ? {}", q.text);
                    }
                }

                Ok(())
            }
        },

        // Task commands
        Commands::Tasks(cmd) => match cmd {
            TaskCommands::Task { action } => {
                forge::handle_task_command(action, &socket_path).await
            }
        },

        // System commands
        Commands::System(cmd) => match cmd {
            SystemCommands::Daemon { action } => handle_daemon(action).await,
            SystemCommands::Config { action } => handle_config(action),
            SystemCommands::Init => handle_init(),
            SystemCommands::Mcp { action } => handle_mcp(action).await,
            SystemCommands::Cortex { action } => handle_cortex(action, &socket_path).await,
            SystemCommands::Agent { action } => handle_agent(action),
            SystemCommands::Completions { .. } => unreachable!("Handled in main()"),
            SystemCommands::Upgrade => handle_upgrade().await,
            SystemCommands::Migrate { action } => handle_migrate(action, &socket_path).await,
            SystemCommands::Purge { action } => handle_purge(action, &socket_path).await,
        },
    }
}

async fn handle_daemon(action: DaemonAction) -> Result<()> {
    match action {
        DaemonAction::Start | DaemonAction::Restart | DaemonAction::Run => {
            // Handled in main() before runtime starts
            unreachable!()
        }
        DaemonAction::Stop => daemon::stop_daemon().await,
        DaemonAction::Status => {
            daemon::daemon_status()?;
            println!();
            launchd::status()
        }
        DaemonAction::Enable => launchd::enable(),
        DaemonAction::Disable => launchd::disable(),
        DaemonAction::Logs { follow } => launchd::tail_logs(follow),
    }
}

fn handle_config(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let cfg = config::load_config()?;
            let toml_str = toml::to_string_pretty(&cfg)?;
            println!("{}", toml_str);
            Ok(())
        }
        ConfigAction::Get { key } => {
            let cfg = config::load_config()?;
            match config::get_config_value(&cfg, &key) {
                Some(value) => println!("{}", value),
                None => {
                    let valid_keys = ["daemon.socket_path", "daemon.pid_file", "database.path"];
                    if valid_keys.contains(&key.as_str()) {
                        println!("(not set)");
                    } else {
                        anyhow::bail!("Unknown config key: {}", key);
                    }
                }
            }
            Ok(())
        }
        ConfigAction::Set { key, value } => {
            let mut cfg = config::load_config()?;
            config::set_config_value(&mut cfg, &key, &value)?;
            config::save_config(&cfg)?;
            println!("Set {} = {}", key, value);
            Ok(())
        }
        ConfigAction::Path => {
            let path = config::get_config_file()?;
            println!("{}", path.display());
            Ok(())
        }
    }
}

async fn handle_mcp(action: McpAction) -> Result<()> {
    match action {
        McpAction::Serve => {
            let cfg = config::load_config()?;
            let socket_path = config::get_socket_path(&cfg)?;
            mcp::start_server(&socket_path).await
        }
    }
}

fn handle_agent(action: AgentAction) -> Result<()> {
    match action {
        AgentAction::Start { session, resume } => agent::start(session, resume),
        AgentAction::Run { resume } => agent::run(resume),
        AgentAction::Restart => agent::restart(),
        AgentAction::Sessions => {
            let sessions = agent::list_sessions()?;
            if sessions.is_empty() {
                println!("No active zellij sessions.");
            } else {
                println!("Active zellij sessions:");
                for session in sessions {
                    println!("  {}", session);
                }
            }
            Ok(())
        }
        AgentAction::Kill { session } => {
            let session_name = session.unwrap_or_else(|| "memex-agent".to_string());
            agent::kill_session(&session_name)?;
            println!("Killed session: {}", session_name);
            Ok(())
        }
    }
}

fn handle_init() -> Result<()> {
    let config_file = config::get_config_file()?;

    if config_file.exists() {
        println!("Config file already exists: {}", config_file.display());
        return Ok(());
    }

    let cfg = config::Config::default();
    config::save_config(&cfg)?;
    println!("Created config file: {}", config_file.display());

    println!();
    println!("Default paths:");
    println!("  Socket: {}", config::get_socket_path(&cfg)?.display());
    println!("  PID file: {}", config::get_pid_file(&cfg)?.display());
    println!("  Database: {}", config::get_db_path(&cfg)?.display());

    Ok(())
}

async fn handle_upgrade() -> Result<()> {
    use std::process::Command;

    // Check daemon status before upgrade
    let cfg = config::load_config()?;
    let pid_path = config::get_pid_file(&cfg)?;
    let daemon_running_before = pid::check_daemon(&pid_path)?.is_some();

    println!("Running nix profile upgrade memex...");

    // Shell out to nix profile upgrade with inherited stdout/stderr
    let status = Command::new("nix")
        .args(["profile", "upgrade", "memex"])
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run nix profile upgrade: {}", e))?;

    if !status.success() {
        anyhow::bail!("nix profile upgrade failed with exit code: {:?}", status.code());
    }

    println!("Upgrade complete");

    // Always restart daemon if it was running (version comparison unreliable during rapid dev)
    if daemon_running_before {
        println!("Restarting daemon...");

        // Stop the daemon
        daemon::stop_daemon().await?;

        // Brief pause to ensure clean shutdown
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Start the daemon - need to spawn new process since Daemon::start() forks
        let status = Command::new("memex")
            .args(["daemon", "start"])
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to start daemon: {}", e))?;

        if !status.success() {
            anyhow::bail!("Failed to restart daemon");
        }

        println!("Daemon restarted");
    }

    Ok(())
}

async fn handle_migrate(action: MigrateAction, socket_path: &std::path::Path) -> Result<()> {
    let client = ipc::Client::new(socket_path);

    match action {
        MigrateAction::TasksToRecords => {
            println!("Migrating tasks from Forge to Atlas records...");
            let result: serde_json::Value = client.request("migrate_tasks_to_records", &()).await?;

            let tasks = result["tasks_migrated"].as_u64().unwrap_or(0);
            let notes = result["notes_migrated"].as_u64().unwrap_or(0);
            let deps = result["dependencies_migrated"].as_u64().unwrap_or(0);
            let skipped = result["skipped_already_migrated"].as_u64().unwrap_or(0);

            println!("Migration complete:");
            println!("  Tasks migrated: {}", tasks);
            println!("  Notes migrated: {}", notes);
            println!("  Dependencies migrated: {}", deps);
            if skipped > 0 {
                println!("  Skipped (already migrated): {}", skipped);
            }
            Ok(())
        }
    }
}

async fn handle_purge(action: PurgeAction, socket_path: &std::path::Path) -> Result<()> {
    use atlas::{MemoClient, RecordClient};
    use std::io::{self, Write};

    match action {
        PurgeAction::Memo { id, dry_run, yes } => {
            let client = MemoClient::new(socket_path);

            // First, preview what will be deleted
            let preview = client.purge_memo(&id, true).await?;

            if !preview.any_deleted() {
                println!("Nothing to purge. Memo not found: {}", id);
                return Ok(());
            }

            println!("Purge preview for memo: {}", id);
            println!("  Memo: will be deleted");
            println!("  Events: {} to delete", preview.events_deleted);
            println!("  Facts: {} to delete", preview.facts_deleted);
            println!("  Entities (orphaned): {} to delete", preview.entities_deleted);
            println!("  Total: {} items", preview.total_deleted());
            println!();

            if dry_run {
                println!("[DRY RUN] No changes made.");
                return Ok(());
            }

            // Confirm unless --yes flag
            if !yes {
                print!("This will PERMANENTLY delete all listed items. Continue? [y/N] ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            // Execute the purge
            let result = client.purge_memo(&id, false).await?;
            println!("Purge complete:");
            println!("  Memo: deleted");
            println!("  Events deleted: {}", result.events_deleted);
            println!("  Facts deleted: {}", result.facts_deleted);
            println!("  Entities deleted: {}", result.entities_deleted);
            println!("  Total items removed: {}", result.total_deleted());

            Ok(())
        }

        PurgeAction::Record { id, dry_run, yes } => {
            let client = RecordClient::new(socket_path);

            // First, preview what will be deleted
            let preview = client.purge_record(&id, true).await?;

            if !preview.any_deleted() {
                println!("Nothing to purge. Record not found: {}", id);
                return Ok(());
            }

            println!("Purge preview for record: {}", id);
            println!("  Record: will be deleted");
            println!("  Events: {} to delete", preview.events_deleted);
            println!("  Facts: {} to delete", preview.facts_deleted);
            println!("  Entities (orphaned): {} to delete", preview.entities_deleted);
            println!("  Edges: {} to delete", preview.edges_deleted);
            println!("  Total: {} items", preview.total_deleted());
            println!();

            if dry_run {
                println!("[DRY RUN] No changes made.");
                return Ok(());
            }

            // Confirm unless --yes flag
            if !yes {
                print!("This will PERMANENTLY delete all listed items. Continue? [y/N] ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            // Execute the purge
            let result = client.purge_record(&id, false).await?;
            println!("Purge complete:");
            println!("  Record: deleted");
            println!("  Events deleted: {}", result.events_deleted);
            println!("  Facts deleted: {}", result.facts_deleted);
            println!("  Entities deleted: {}", result.entities_deleted);
            println!("  Edges deleted: {}", result.edges_deleted);
            println!("  Total items removed: {}", result.total_deleted());

            Ok(())
        }
    }
}

async fn handle_cortex(action: CortexAction, socket_path: &std::path::Path) -> Result<()> {
    use cortex::{CortexClient, WorkerState};

    let client = CortexClient::new(socket_path);

    match action {
        CortexAction::Status | CortexAction::List => {
            let workers = client.list_workers().await?;

            if workers.is_empty() {
                println!("No cortex workers running.");
                return Ok(());
            }

            println!("Cortex Workers ({}):", workers.len());
            println!();

            for status in &workers {
                // State indicator
                let state_icon = match &status.state {
                    WorkerState::Working => "●",  // Working
                    WorkerState::Ready => "○",    // Ready
                    WorkerState::Idle => "○",     // Idle
                    WorkerState::Error(_) => "✗", // Error
                    _ => "?",
                };

                // Format state with activity
                let state_str = match &status.state {
                    WorkerState::Working => {
                        if let Some(ref task) = status.current_task {
                            format!("working: {}", task)
                        } else {
                            "working".to_string()
                        }
                    }
                    WorkerState::Error(msg) => format!("error: {}", msg),
                    other => format!("{:?}", other).to_lowercase(),
                };

                // Calculate idle time
                let idle_info = if !matches!(status.state, WorkerState::Working) {
                    let idle_secs = (chrono::Utc::now() - status.last_activity).num_seconds();
                    if idle_secs > 3600 {
                        format!(" ({}h idle)", idle_secs / 3600)
                    } else if idle_secs > 60 {
                        format!(" ({}m idle)", idle_secs / 60)
                    } else if idle_secs > 0 {
                        format!(" ({}s idle)", idle_secs)
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                // Get directory basename
                let dir = status.worktree.as_deref().map(|p| {
                    std::path::Path::new(p)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(p)
                }).unwrap_or("-");

                println!("{} {} [{}]{}", state_icon, status.id, state_str, idle_info);
                println!("    dir: {}", dir);
                println!("    messages: {} sent, {} received", status.messages_sent, status.messages_received);
                println!("    started: {}", status.started_at.format("%Y-%m-%d %H:%M:%S UTC"));
                println!();
            }

            Ok(())
        }

        CortexAction::Transcript { worker_id, limit } => {
            let wid = cortex::WorkerId::from_string(&worker_id);
            let entries = client.get_transcript(&wid, limit).await?;

            if entries.is_empty() {
                println!("No transcript entries for worker {}", worker_id);
                return Ok(());
            }

            println!("Transcript for worker {} ({} entries):", worker_id, entries.len());
            println!();

            for (i, entry) in entries.iter().enumerate() {
                println!("─── Entry {} ({}) ───", i + 1, entry.timestamp.format("%H:%M:%S"));

                // Show prompt (truncated)
                let prompt_preview = if entry.prompt.len() > 200 {
                    format!("{}...", &entry.prompt[..200])
                } else {
                    entry.prompt.clone()
                };
                println!("Prompt: {}", prompt_preview);

                // Show response or status
                match &entry.response {
                    Some(response) => {
                        let response_preview = if response.len() > 500 {
                            format!("{}...", &response[..500])
                        } else {
                            response.clone()
                        };
                        if entry.is_error {
                            println!("Error: {}", response_preview);
                        } else {
                            println!("Response: {}", response_preview);
                        }
                        println!("Duration: {}ms", entry.duration_ms);
                    }
                    None => {
                        println!("Status: Processing...");
                    }
                }
                println!();
            }

            Ok(())
        }
    }
}
