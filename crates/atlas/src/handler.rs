//! Command handlers for Atlas CLI
//!
//! These handlers execute the memo management commands via the daemon.

use std::path::Path;

use anyhow::Result;

use crate::cli::MemoCommand;
use crate::client::MemoClient;
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
