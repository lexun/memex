//! JSONL parser for Claude Code session files

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use tracing::{debug, warn};

use crate::Result;

/// A single entry from a Claude Code session JSONL file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeEntry {
    #[serde(rename = "type")]
    pub entry_type: EntryType,

    /// User or assistant message (for user/assistant types)
    #[serde(default)]
    pub message: Option<Message>,

    /// Session summary (for summary type)
    #[serde(default)]
    pub summary: Option<String>,

    /// Leaf UUID for tracking conversation structure
    #[serde(rename = "leafUuid", default)]
    pub leaf_uuid: Option<String>,

    /// Timestamp (may not be present in all entry types)
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,

    /// Additional fields we don't parse (for forward compatibility)
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// Entry type in Claude Code session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryType {
    /// User message
    User,
    /// Assistant response
    Assistant,
    /// Session summary
    Summary,
    /// File history snapshot
    FileHistorySnapshot,
    /// System message
    System,
    /// Queue operation
    QueueOperation,
}

/// Message content from user or assistant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message content (string for user, may be complex for assistant with tool use)
    #[serde(default)]
    pub content: serde_json::Value,

    /// Additional message fields
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl Message {
    /// Get message content as string, if possible
    pub fn content_as_string(&self) -> Option<String> {
        match &self.content {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(arr) => {
                // Handle content blocks (may have text blocks, tool use, etc.)
                let mut parts = Vec::new();
                for item in arr {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        parts.push(text.to_string());
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n\n"))
                }
            }
            _ => None,
        }
    }
}

/// Parse JSONL entries incrementally from a given file offset
///
/// Returns parsed entries and the new file offset to resume from.
/// Skips malformed lines with a warning.
pub fn parse_jsonl_incremental(
    file_path: &Path,
    from_offset: u64,
) -> Result<(Vec<ClaudeCodeEntry>, u64)> {
    let mut file = File::open(file_path)?;
    file.seek(SeekFrom::Start(from_offset))?;

    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut current_offset = from_offset;

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                warn!(
                    "Failed to read line {} in {}: {}",
                    line_num + 1,
                    file_path.display(),
                    e
                );
                continue;
            }
        };

        // Update offset (line length + newline)
        current_offset += line.len() as u64 + 1;

        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }

        // Parse JSON line
        match serde_json::from_str::<ClaudeCodeEntry>(&line) {
            Ok(entry) => {
                debug!(
                    "Parsed entry type {:?} from {}:{}",
                    entry.entry_type,
                    file_path.display(),
                    line_num + 1
                );
                entries.push(entry);
            }
            Err(e) => {
                warn!(
                    "Failed to parse JSON at {}:{}: {} (line: {}...)",
                    file_path.display(),
                    line_num + 1,
                    e,
                    &line.chars().take(80).collect::<String>()
                );
                continue;
            }
        }
    }

    Ok((entries, current_offset))
}

/// Parse all entries from a session file (from beginning)
pub fn parse_jsonl(file_path: &Path) -> Result<Vec<ClaudeCodeEntry>> {
    let (entries, _) = parse_jsonl_incremental(file_path, 0)?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_summary_entry() {
        let json = r#"{"type":"summary","summary":"Test Session","leafUuid":"123"}"#;
        let entry: ClaudeCodeEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.entry_type, EntryType::Summary);
        assert_eq!(entry.summary, Some("Test Session".to_string()));
        assert_eq!(entry.leaf_uuid, Some("123".to_string()));
    }

    #[test]
    fn test_parse_user_message() {
        let json = r#"{"type":"user","message":{"content":"Hello, Claude!"}}"#;
        let entry: ClaudeCodeEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.entry_type, EntryType::User);
        assert!(entry.message.is_some());
        let msg = entry.message.unwrap();
        assert_eq!(msg.content_as_string(), Some("Hello, Claude!".to_string()));
    }

    #[test]
    fn test_parse_assistant_message() {
        let json = r#"{"type":"assistant","message":{"content":"Hi there!"}}"#;
        let entry: ClaudeCodeEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.entry_type, EntryType::Assistant);
        assert!(entry.message.is_some());
    }

    #[test]
    fn test_parse_jsonl_incremental() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"summary","summary":"Test"}}"#).unwrap();
        writeln!(file, r#"{{"type":"user","message":{{"content":"Hi"}}}}"#).unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"content":"Hello"}}}}"#
        )
        .unwrap();

        let path = file.path().to_path_buf();
        let (entries, offset) = parse_jsonl_incremental(&path, 0).unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].entry_type, EntryType::Summary);
        assert_eq!(entries[1].entry_type, EntryType::User);
        assert_eq!(entries[2].entry_type, EntryType::Assistant);

        // Read from offset (should return no new entries)
        let (new_entries, _) = parse_jsonl_incremental(&path, offset).unwrap();
        assert_eq!(new_entries.len(), 0);

        // Append a new line
        writeln!(
            file,
            r#"{{"type":"user","message":{{"content":"Bye"}}}}"#
        )
        .unwrap();

        // Read from previous offset (should return 1 new entry)
        let (new_entries, _) = parse_jsonl_incremental(&path, offset).unwrap();
        assert_eq!(new_entries.len(), 1);
        assert_eq!(new_entries[0].entry_type, EntryType::User);
    }

    #[test]
    fn test_message_content_as_string_with_blocks() {
        let json = r#"{"content":[{"type":"text","text":"First block"},{"type":"text","text":"Second block"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();

        assert_eq!(
            msg.content_as_string(),
            Some("First block\n\nSecond block".to_string())
        );
    }

    #[test]
    fn test_skip_malformed_lines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"summary","summary":"Test"}}"#).unwrap();
        writeln!(file, "invalid json").unwrap();
        writeln!(file, r#"{{"type":"user","message":{{"content":"Hi"}}}}"#).unwrap();

        let path = file.path().to_path_buf();
        let (entries, _) = parse_jsonl_incremental(&path, 0).unwrap();

        // Should parse 2 valid entries, skipping the malformed one
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entry_type, EntryType::Summary);
        assert_eq!(entries[1].entry_type, EntryType::User);
    }
}
