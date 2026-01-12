# Watcher

Claude Code transcript capture for memex.

## Overview

This crate watches Claude Code session files in `~/.claude/projects/` and captures conversations into the memex knowledge base.

## Features

- **JSONL Parser**: Parse Claude Code session files with support for all entry types:
  - `user` - User messages
  - `assistant` - Assistant responses (with tool use support)
  - `summary` - Session summaries
  - `file-history-snapshot` - File state snapshots
  - `system` - System messages
  - `queue-operation` - Queue operations

- **Incremental Reading**: Tail-like functionality to only read new entries from files

- **File Watching**: Monitors the Claude projects directory for changes using the `notify` crate

- **Project Filtering**: Configurable include/exclude patterns for selective watching

## Usage

```rust
use watcher::{TranscriptWatcher, WatchConfig};

// Create watcher with default config (~/.claude/projects)
let config = WatchConfig::default();
let watcher = TranscriptWatcher::new(config);

// Start watching (long-running async task)
watcher.start_watching().await?;
```

### Parsing Session Files

```rust
use watcher::parse_jsonl;

let entries = parse_jsonl(Path::new("session.jsonl"))?;
for entry in entries {
    match entry.entry_type {
        EntryType::User | EntryType::Assistant => {
            if let Some(msg) = entry.message {
                if let Some(content) = msg.content_as_string() {
                    println!("Message: {}", content);
                }
            }
        }
        _ => {}
    }
}
```

## Testing

Run tests with:
```bash
cargo test -p watcher
```

Test with real session files:
```bash
cargo run --example test_parse --manifest-path crates/watcher/Cargo.toml
```

## Architecture

- **parser.rs**: JSONL parsing logic with incremental reading support
- **watcher.rs**: File watching infrastructure with project filtering
- **lib.rs**: Public API and error types

## TODO

- [ ] Integration with Atlas (store threads/entries)
- [ ] Retroactive import command
- [ ] Configuration for project filtering
- [ ] CLI commands (status, import, toggle)
