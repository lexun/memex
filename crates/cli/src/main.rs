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
    /// Daemon management
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Task management
    Task {
        #[command(subcommand)]
        action: forge::TaskCommand,
    },
    /// MCP server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Initialize memex configuration
    Init,
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
        Commands::Daemon { action } => handle_daemon(action).await,
        Commands::Config { action } => handle_config(action),
        Commands::Task { action } => {
            let cfg = config::load_config()?;
            let socket_path = config::get_socket_path(&cfg)?;
            forge::handle_task_command(action, &socket_path).await
        }
        Commands::Mcp { action } => handle_mcp(action).await,
        Commands::Init => handle_init(),
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
