mod completions;
mod config;
mod daemon;
mod help;
mod pid;

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
    /// Rebuild knowledge from memos
    #[command(display_order = 6)]
    Rebuild {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Show knowledge system status
    #[command(display_order = 7)]
    Status,
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
    /// Generate shell completions
    #[command(display_order = 24)]
    Completions {
        /// Shell to generate completions for
        shell: CompletionShell,
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
            KnowledgeCommands::Rebuild { project } => {
                let client = atlas::KnowledgeClient::new(&socket_path);
                let result = client.rebuild(project.as_deref()).await?;
                println!("Rebuilt knowledge:");
                println!("  Deleted: {} facts, {} entities", result.facts_deleted, result.entities_deleted);
                println!("  Created: {} facts, {} entities from {} memos",
                    result.facts_created, result.entities_created, result.memos_processed);
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
                    println!("      Run 'memex rebuild' to regenerate facts with embeddings.");
                }

                if !status.llm_configured {
                    println!();
                    println!("Warning: LLM not configured. Semantic search disabled.");
                    println!("         Set OPENAI_API_KEY or configure llm.api_key.");
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
            SystemCommands::Completions { .. } => unreachable!("Handled in main()"),
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
        DaemonAction::Status => daemon::daemon_status(),
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
