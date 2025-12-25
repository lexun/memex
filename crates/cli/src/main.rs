//! Memex CLI
//!
//! Command-line interface for Memex.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "memex")]
#[command(about = "AI-powered knowledge management and agent orchestration")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Task management
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    /// Daemon management
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
}

#[derive(Subcommand)]
enum TaskCommands {
    /// Create a new task
    Create {
        /// Task title
        title: String,
    },
    /// List tasks
    List,
}

#[derive(Subcommand)]
enum DaemonCommands {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Check daemon status
    Status,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Task { command }) => match command {
            TaskCommands::Create { title } => {
                println!("Creating task: {}", title);
            }
            TaskCommands::List => {
                println!("Listing tasks...");
            }
        },
        Some(Commands::Daemon { command }) => match command {
            DaemonCommands::Start => {
                println!("Starting daemon...");
            }
            DaemonCommands::Stop => {
                println!("Stopping daemon...");
            }
            DaemonCommands::Status => {
                println!("Checking daemon status...");
            }
        },
        None => {
            println!("Memex - AI-powered knowledge management");
            println!("Run 'memex --help' for usage");
        }
    }
}
