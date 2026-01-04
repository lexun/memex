//! Command handlers for Atlas CLI
//!
//! These handlers execute the memo and event management commands via the daemon.

use std::path::Path;

use anyhow::Result;

use crate::cli::{ContextCommand, EventCommand, MemoCommand};
use crate::client::{ContextClient, EventClient, MemoClient};
use crate::event::Event;
use crate::Memo;

/// Handle a memo command via the daemon
pub async fn handle_memo_command(cmd: MemoCommand, socket_path: &Path) -> Result<()> {
    let client = MemoClient::new(socket_path);

    match cmd {
        MemoCommand::List { limit } => {
            let memos = client.list_memos(limit).await?;

            if memos.is_empty() {
                println!("No memos found");
            } else {
                for memo in memos {
                    print_memo_summary(&memo);
                }
            }
            Ok(())
        }

        MemoCommand::Get { id } => {
            match client.get_memo(&id).await? {
                Some(memo) => print_memo_detail(&memo),
                None => println!("Memo not found: {}", id),
            }
            Ok(())
        }

        MemoCommand::Delete { id } => {
            match client.delete_memo(&id).await? {
                Some(memo) => {
                    println!("Deleted memo: {}", memo.id_str().unwrap_or_default());
                }
                None => {
                    println!("Memo not found: {}", id);
                }
            }
            Ok(())
        }
    }
}

fn print_memo_summary(memo: &Memo) {
    let id = memo.id_str().unwrap_or_default();
    let timestamp = memo.created_at.format("%Y-%m-%d %H:%M");
    // Truncate content for summary
    let content = if memo.content.len() > 60 {
        format!("{}...", &memo.content[..60])
    } else {
        memo.content.clone()
    };
    println!("[{}] ({}) {}", id, timestamp, content);
}

fn print_memo_detail(memo: &Memo) {
    println!("Memo: {}", memo.id_str().unwrap_or_default());
    println!("  Created: {}", memo.created_at.format("%Y-%m-%d %H:%M:%S"));
    println!("  Actor: {}", memo.source.actor);
    println!("  Authority: {:?}", memo.source.authority);
    println!("  Content:");
    println!("    {}", memo.content);
}

/// Handle an event command via the daemon
pub async fn handle_event_command(cmd: EventCommand, socket_path: &Path) -> Result<()> {
    let client = EventClient::new(socket_path);

    match cmd {
        EventCommand::List { event_type, limit } => {
            let events = client.list_events(event_type.as_deref(), limit).await?;

            if events.is_empty() {
                println!("No events found");
            } else {
                for event in events {
                    print_event_summary(&event);
                }
            }
            Ok(())
        }

        EventCommand::Get { id } => {
            match client.get_event(&id).await? {
                Some(event) => print_event_detail(&event),
                None => println!("Event not found: {}", id),
            }
            Ok(())
        }
    }
}

fn print_event_summary(event: &Event) {
    let id = event.id_str().unwrap_or_default();
    let timestamp = event.timestamp.format("%Y-%m-%d %H:%M");
    println!("[{}] ({}) {}", id, timestamp, event.event_type);
}

fn print_event_detail(event: &Event) {
    println!("Event: {}", event.id_str().unwrap_or_default());
    println!("  Timestamp: {}", event.timestamp.format("%Y-%m-%d %H:%M:%S"));
    println!("  Type: {}", event.event_type);
    println!("  Actor: {}", event.source.actor);
    if let Some(ref authority) = event.source.authority {
        println!("  Authority: {:?}", authority);
    }
    if let Some(ref via) = event.source.via {
        println!("  Via: {}", via);
    }
    println!("  Payload:");
    if let Ok(pretty) = serde_json::to_string_pretty(&event.payload) {
        for line in pretty.lines() {
            println!("    {}", line);
        }
    }
}

/// Handle a context command via the daemon
pub async fn handle_context_command(cmd: ContextCommand, socket_path: &Path) -> Result<()> {
    let client = ContextClient::new(socket_path);

    match cmd {
        ContextCommand::Search {
            query,
            entity_type,
            project,
            limit,
        } => {
            let result = client
                .discover(&query, entity_type.as_deref(), project.as_deref(), Some(limit))
                .await?;

            if result.results.is_empty() {
                println!("No results found for: {}", query);
            } else {
                // Show summary first if available
                if let Some(ref summary) = result.summary {
                    println!("Summary: {}", summary);
                    println!();
                }

                println!("Found {} result(s) for: {}", result.count, query);
                println!();
                for item in result.results {
                    print_context_result(&item);
                }
            }
            Ok(())
        }
    }
}

fn print_context_result(item: &serde_json::Value) {
    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");

    match item_type {
        "memo" => {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let truncated = if content.len() > 80 {
                format!("{}...", &content[..80])
            } else {
                content.to_string()
            };
            println!("[memo:{}] {}", id, truncated);
        }
        "task" => {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let priority = item.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
            println!("[task:{}] [P{}] [{}] {}", id, priority, status, title);
        }
        "task_note" => {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let truncated = if content.len() > 80 {
                format!("{}...", &content[..80])
            } else {
                content.to_string()
            };
            println!("[note:{}] {}", id, truncated);
        }
        _ => {
            if let Ok(json) = serde_json::to_string(item) {
                println!("[{}] {}", item_type, json);
            }
        }
    }
}
