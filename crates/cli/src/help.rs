//! Custom help system with categorized command sections
//!
//! This module provides nix-style help output with commands grouped into
//! logical sections. The command registry must be kept in sync with the
//! actual subcommand definitions.

use std::collections::BTreeMap;

/// Command categories for help grouping
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// Primary everyday commands
    Main,
    /// Data exploration and power-user tools
    Explore,
    /// Background services
    Services,
    /// Setup and configuration
    Configure,
}

impl Category {
    pub fn heading(&self) -> &'static str {
        match self {
            Category::Main => "Main",
            Category::Explore => "Explore",
            Category::Services => "Services",
            Category::Configure => "Configure",
        }
    }

    pub fn order(&self) -> u8 {
        match self {
            Category::Main => 0,
            Category::Explore => 1,
            Category::Services => 2,
            Category::Configure => 3,
        }
    }
}

/// Metadata for a single command
#[derive(Debug, Clone)]
pub struct CommandInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub category: Category,
}

/// Registry of all commands with their metadata
///
/// IMPORTANT: This must be kept in sync with the Commands enum.
/// The `test_command_registry_matches_enum` test verifies this.
pub fn command_registry() -> Vec<CommandInfo> {
    vec![
        // Main commands - everyday use
        CommandInfo {
            name: "record",
            description: "Record a memo",
            category: Category::Main,
        },
        CommandInfo {
            name: "query",
            description: "Query knowledge",
            category: Category::Main,
        },
        CommandInfo {
            name: "task",
            description: "Manage tasks",
            category: Category::Main,
        },

        // Explore - data exploration and power-user tools
        CommandInfo {
            name: "search",
            description: "Search facts",
            category: Category::Explore,
        },
        CommandInfo {
            name: "memo",
            description: "Browse memos",
            category: Category::Explore,
        },
        CommandInfo {
            name: "event",
            description: "View system events",
            category: Category::Explore,
        },
        CommandInfo {
            name: "rebuild",
            description: "Rebuild knowledge index",
            category: Category::Explore,
        },
        CommandInfo {
            name: "backfill",
            description: "Backfill missing knowledge",
            category: Category::Explore,
        },
        CommandInfo {
            name: "status",
            description: "Knowledge system status",
            category: Category::Explore,
        },
        CommandInfo {
            name: "entities",
            description: "List known entities",
            category: Category::Explore,
        },
        CommandInfo {
            name: "entity",
            description: "Get facts about entity",
            category: Category::Explore,
        },

        // Services - background processes
        CommandInfo {
            name: "daemon",
            description: "Background daemon",
            category: Category::Services,
        },
        CommandInfo {
            name: "mcp",
            description: "MCP server",
            category: Category::Services,
        },

        // Configure - setup and settings
        CommandInfo {
            name: "init",
            description: "Initialize",
            category: Category::Configure,
        },
        CommandInfo {
            name: "config",
            description: "Settings",
            category: Category::Configure,
        },
        CommandInfo {
            name: "completions",
            description: "Shell completions",
            category: Category::Configure,
        },
    ]
}

/// Generate the complete help text with categorized sections
pub fn generate_help() -> String {
    let mut output = String::new();

    // Header
    output.push_str("memex - A knowledge management system for AI\n");
    output.push_str("\n");
    output.push_str("Usage: memex <command> [options]\n");
    output.push_str("\n");

    // Group commands by category
    let commands = command_registry();
    let mut by_category: BTreeMap<Category, Vec<&CommandInfo>> = BTreeMap::new();

    for cmd in &commands {
        by_category.entry(cmd.category).or_default().push(cmd);
    }

    // Sort categories by their defined order
    let mut categories: Vec<_> = by_category.keys().collect();
    categories.sort_by_key(|c| c.order());

    // Render each category
    for category in categories {
        let cmds = &by_category[category];

        output.push_str(category.heading());
        output.push_str(":\n");

        // Find max command name length for alignment
        let max_name_len = cmds.iter().map(|c| c.name.len()).max().unwrap_or(0);

        for cmd in cmds {
            output.push_str("  ");
            output.push_str(cmd.name);
            // Pad to align descriptions
            for _ in 0..(max_name_len - cmd.name.len() + 2) {
                output.push(' ');
            }
            output.push_str(cmd.description);
            output.push('\n');
        }

        output.push('\n');
    }

    // Footer
    output.push_str("Run 'memex <command> --help' for more information on a command.\n");

    output
}

/// Get all registered command names (for sync testing)
#[cfg(test)]
fn registered_command_names() -> Vec<&'static str> {
    command_registry().iter().map(|c| c.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use crate::Cli;

    #[test]
    fn test_command_registry_matches_enum() {
        // Get command names from clap
        let cli = Cli::command();
        let mut clap_commands: Vec<String> = cli
            .get_subcommands()
            .filter(|c| !c.is_hide_set()) // Skip hidden commands
            .map(|c| c.get_name().to_string())
            .collect();
        clap_commands.sort();

        // Get command names from our registry
        let mut registry_commands: Vec<String> = registered_command_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        registry_commands.sort();

        assert_eq!(
            clap_commands, registry_commands,
            "Command registry is out of sync with Commands enum!\n\
             Clap commands: {:?}\n\
             Registry commands: {:?}",
            clap_commands, registry_commands
        );
    }

    #[test]
    fn test_help_generation() {
        let help = generate_help();

        // Check structure
        assert!(help.contains("Main:"));
        assert!(help.contains("Explore:"));
        assert!(help.contains("Services:"));
        assert!(help.contains("Configure:"));

        // Check some commands are present
        assert!(help.contains("record"));
        assert!(help.contains("query"));
        assert!(help.contains("task"));
        assert!(help.contains("daemon"));
    }
}
