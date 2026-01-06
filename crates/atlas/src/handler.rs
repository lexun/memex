//! Command handlers for Atlas CLI
//!
//! These handlers execute the memo and event management commands via the daemon.

use std::path::Path;

use anyhow::Result;

use crate::cli::{EventCommand, KnowledgeCommand, MemoCommand};
use crate::client::{EventClient, KnowledgeClient, MemoClient};
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

/// Handle a knowledge command via the daemon
pub async fn handle_knowledge_command(cmd: KnowledgeCommand, socket_path: &Path) -> Result<()> {
    let client = KnowledgeClient::new(socket_path);

    match cmd {
        KnowledgeCommand::Query {
            query,
            project,
            limit,
        } => {
            let result = client
                .query(&query, project.as_deref(), Some(limit))
                .await?;

            if result.answer.is_empty() {
                println!("No relevant knowledge found for: {}", query);
                println!();
                println!("Note: Facts are extracted from memos. Try recording some memos first.");
            } else {
                println!("{}", result.answer);
            }
            Ok(())
        }

        KnowledgeCommand::Search {
            query,
            project,
            limit,
        } => {
            let result = client
                .search(&query, project.as_deref(), Some(limit))
                .await?;

            if result.results.is_empty() {
                println!("No facts found for: {}", query);
                println!();
                println!("Note: Facts are extracted from memos. Try recording some memos first.");
            } else {
                println!("Found {} fact(s) for: {}", result.count, query);
                println!();
                for fact in result.results {
                    print_fact(&fact);
                }
            }
            Ok(())
        }

        KnowledgeCommand::Extract {
            project,
            batch_size,
        } => {
            println!("Extracting facts from memos...");
            let result = client
                .extract_facts(project.as_deref(), Some(batch_size))
                .await?;

            println!("Extraction complete:");
            println!("  Memos processed: {}", result.memos_processed);
            println!("  Facts created: {}", result.facts_created);
            println!("  Entities created: {}", result.entities_created);
            Ok(())
        }

        KnowledgeCommand::Rebuild { project, yes } => {
            // Confirmation prompt unless --yes flag is set
            if !yes {
                println!("This will delete all facts and entities and re-extract from memos.");
                if project.is_some() {
                    println!("Scope: project '{}'", project.as_ref().unwrap());
                } else {
                    println!("Scope: ALL projects");
                }
                println!();
                print!("Continue? [y/N] ");
                use std::io::{self, Write};
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            println!("Rebuilding knowledge graph...");
            let result = client.rebuild(project.as_deref()).await?;

            println!("Rebuild complete:");
            println!("  Facts deleted: {}", result.facts_deleted);
            println!("  Entities deleted: {}", result.entities_deleted);
            println!("  Memos processed: {}", result.memos_processed);
            println!("  Facts created: {}", result.facts_created);
            println!("  Entities created: {}", result.entities_created);
            Ok(())
        }

        KnowledgeCommand::Status => {
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
                println!("Warning: LLM not configured. Fact extraction and semantic search disabled.");
                println!("         Set OPENAI_API_KEY or configure llm.api_key in config.");
            }

            Ok(())
        }

        KnowledgeCommand::Entities {
            project,
            entity_type,
            limit,
        } => {
            let entities = client
                .list_entities(project.as_deref(), entity_type.as_deref(), limit)
                .await?;

            if entities.is_empty() {
                println!("No entities found.");
                println!();
                println!("Note: Entities are extracted from memos. Try recording some memos first.");
            } else {
                println!("Found {} entities:", entities.len());
                println!();
                for entity in entities {
                    print_entity_summary(&entity);
                }
            }
            Ok(())
        }

        KnowledgeCommand::Entity { name, project } => {
            let result = client
                .get_entity_facts(&name, project.as_deref())
                .await?;

            if result.facts.is_empty() {
                println!("No facts found for entity: {}", name);
                println!();
                println!("The entity may not exist or have no linked facts.");
            } else {
                println!("Found {} fact(s) about \"{}\":", result.count, result.entity);
                println!();
                for fact in result.facts {
                    print_entity_fact(&fact);
                }
            }
            Ok(())
        }
    }
}

fn print_entity_summary(entity: &crate::Entity) {
    let id = entity.id_str().unwrap_or_default();
    println!("[{}] {} ({})", entity.entity_type, entity.name, id);
    if !entity.description.is_empty() {
        println!("      {}", entity.description);
    }
}

fn print_entity_fact(fact: &crate::Fact) {
    println!("[{}] {}", fact.fact_type, fact.content);
}

fn print_fact(fact: &serde_json::Value) {
    let content = fact.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let fact_type = fact.get("fact_type").and_then(|v| v.as_str()).unwrap_or("statement");
    let confidence = fact.get("confidence").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let score = fact.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);

    println!(
        "[{}] (conf: {:.0}%, score: {:.2}) {}",
        fact_type,
        confidence * 100.0,
        score,
        content
    );
}
