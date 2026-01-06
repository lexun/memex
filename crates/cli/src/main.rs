mod config;
mod daemon;
mod pid;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "memex")]
#[command(about = "A knowledge management system for AI")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Task management
    Task {
        #[command(subcommand)]
        action: forge::TaskCommand,
    },
    /// Record a memo to the knowledge base
    Record {
        /// The content to record
        content: String,
    },
    /// Query the knowledge base (LLM-summarized answer)
    Query {
        /// The query to answer
        query: String,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Atlas knowledge base management
    Atlas {
        #[command(subcommand)]
        action: AtlasAction,
    },
    /// Daemon management
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// MCP server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Initialize memex configuration
    Init,
    /// Launch the graphical user interface
    Gui,
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
enum AtlasAction {
    /// Memo management
    Memo {
        #[command(subcommand)]
        action: atlas::MemoCommand,
    },
    /// Event management
    Event {
        #[command(subcommand)]
        action: atlas::EventCommand,
    },
    /// Knowledge discovery (query, search, extract)
    Knowledge {
        #[command(subcommand)]
        action: atlas::KnowledgeCommand,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// Start MCP server on stdio
    Serve,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle daemon start/restart synchronously (before any tokio runtime)
    // This allows proper fork() without runtime conflicts
    match &cli.command {
        Commands::Daemon { action: DaemonAction::Start } => {
            let daemon = daemon::Daemon::new()?;
            return daemon.start();
        }
        Commands::Daemon { action: DaemonAction::Restart } => {
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
        _ => {}
    }

    // For all other commands, use tokio runtime
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> Result<()> {
    // Default to WARN level for quiet CLI output
    // Use RUST_LOG=info or RUST_LOG=debug for verbose output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    match cli.command {
        Commands::Task { action } => {
            let cfg = config::load_config()?;
            let socket_path = config::get_socket_path(&cfg)?;
            forge::handle_task_command(action, &socket_path).await
        }
        Commands::Record { content } => {
            let cfg = config::load_config()?;
            let socket_path = config::get_socket_path(&cfg)?;
            let client = atlas::MemoClient::new(&socket_path);
            let memo = client.record_memo(&content, true, Some("user:default")).await?;
            println!("Recorded: {}", memo.id_str().unwrap_or_default());
            Ok(())
        }
        Commands::Query { query, project } => {
            let cfg = config::load_config()?;
            let socket_path = config::get_socket_path(&cfg)?;
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
        Commands::Atlas { action } => {
            let cfg = config::load_config()?;
            let socket_path = config::get_socket_path(&cfg)?;
            match action {
                AtlasAction::Memo { action } => {
                    atlas::handle_memo_command(action, &socket_path).await
                }
                AtlasAction::Event { action } => {
                    atlas::handle_event_command(action, &socket_path).await
                }
                AtlasAction::Knowledge { action } => {
                    atlas::handle_knowledge_command(action, &socket_path).await
                }
            }
        }
        Commands::Daemon { action } => handle_daemon(action).await,
        Commands::Mcp { action } => handle_mcp(action).await,
        Commands::Config { action } => handle_config(action),
        Commands::Init => handle_init(),
        Commands::Gui => handle_gui(),
    }
}

fn handle_gui() -> Result<()> {
    use std::process::Command;

    // Try to find memex-gui in PATH or next to current executable
    let gui_name = if cfg!(windows) { "memex-gui.exe" } else { "memex-gui" };

    // First try PATH
    match Command::new(gui_name).spawn() {
        Ok(_) => {
            println!("Launched memex-gui");
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Try next to current executable
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(dir) = exe_path.parent() {
                    let gui_path = dir.join(gui_name);
                    if gui_path.exists() {
                        Command::new(&gui_path).spawn()?;
                        println!("Launched {}", gui_path.display());
                        return Ok(());
                    }
                }
            }
            anyhow::bail!(
                "memex-gui not found. Build it with: cargo build -p memex-gui"
            )
        }
        Err(e) => Err(e.into()),
    }
}

async fn handle_daemon(action: DaemonAction) -> Result<()> {
    match action {
        DaemonAction::Start | DaemonAction::Restart => {
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
