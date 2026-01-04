//! Command handlers for Atlas CLI
//!
//! These handlers execute the memo and event management commands via the daemon.

use std::path::Path;

use anyhow::Result;

use crate::cli::{EventCommand, MemoCommand};
use crate::client::{EventClient, MemoClient};
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
