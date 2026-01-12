//! File watcher for Claude Code session files

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::{parser, Result};

/// Configuration for transcript watching
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Directory containing Claude Code projects (~/.claude/projects)
    pub claude_projects_dir: PathBuf,

    /// Optional: Only watch projects matching these patterns
    pub include_projects: Vec<String>,

    /// Optional: Exclude projects matching these patterns
    pub exclude_projects: Vec<String>,
}

impl Default for WatchConfig {
    fn default() -> Self {
        let home = dirs::home_dir().expect("Could not determine home directory");
        Self {
            claude_projects_dir: home.join(".claude/projects"),
            include_projects: Vec::new(),
            exclude_projects: Vec::new(),
        }
    }
}

/// State for a watched session file
#[derive(Debug, Clone)]
pub struct WatchState {
    /// Last byte offset we read from this file
    pub last_position: u64,

    /// Optional: Atlas thread ID this session maps to
    pub thread_id: Option<String>,
}

impl Default for WatchState {
    fn default() -> Self {
        Self {
            last_position: 0,
            thread_id: None,
        }
    }
}

/// Transcript watcher that monitors Claude Code session files
pub struct TranscriptWatcher {
    config: WatchConfig,
    known_sessions: Arc<RwLock<HashMap<PathBuf, WatchState>>>,
}

impl TranscriptWatcher {
    /// Create a new transcript watcher
    pub fn new(config: WatchConfig) -> Self {
        Self {
            config,
            known_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start watching for transcript changes
    ///
    /// This is a long-running async function that will watch for file changes
    /// and process new session entries as they appear.
    pub async fn start_watching(self) -> Result<()> {
        info!(
            "Starting transcript watcher on {}",
            self.config.claude_projects_dir.display()
        );

        // Scan for existing sessions on startup
        self.scan_existing_sessions().await?;

        // Set up file watcher
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    if tx.blocking_send(event).is_err() {
                        // Channel closed, watcher should stop
                    }
                }
            },
            Config::default(),
        )?;

        // Watch the projects directory recursively
        watcher.watch(&self.config.claude_projects_dir, RecursiveMode::Recursive)?;

        info!("File watcher started, monitoring for changes...");

        // Process file events
        while let Some(event) = rx.recv().await {
            if let Err(e) = self.handle_file_event(event).await {
                error!("Error handling file event: {}", e);
            }
        }

        Ok(())
    }

    /// Scan for existing session files on startup
    async fn scan_existing_sessions(&self) -> Result<()> {
        info!("Scanning for existing session files...");

        let projects_dir = &self.config.claude_projects_dir;
        if !projects_dir.exists() {
            warn!(
                "Claude projects directory does not exist: {}",
                projects_dir.display()
            );
            return Ok(());
        }

        let mut session_count = 0;

        // Walk through project directories
        for entry in std::fs::read_dir(projects_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Check if this project matches filters
            if !self.should_watch_project(&path) {
                debug!("Skipping filtered project: {}", path.display());
                continue;
            }

            // Find all .jsonl files in this project directory
            for file_entry in std::fs::read_dir(&path)? {
                let file_entry = file_entry?;
                let file_path = file_entry.path();

                if file_path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                    debug!("Found existing session: {}", file_path.display());
                    session_count += 1;

                    // Initialize watch state at end of file (don't import retroactively)
                    let file_size = std::fs::metadata(&file_path)?.len();
                    let mut sessions = self.known_sessions.write().await;
                    sessions.insert(
                        file_path,
                        WatchState {
                            last_position: file_size,
                            thread_id: None,
                        },
                    );
                }
            }
        }

        info!(
            "Found {} existing session files (will watch for new content)",
            session_count
        );
        Ok(())
    }

    /// Handle a file system event
    async fn handle_file_event(&self, event: Event) -> Result<()> {
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                        if let Some(project_dir) = path.parent() {
                            if self.should_watch_project(project_dir) {
                                self.process_session_file(&path).await?;
                            }
                        }
                    }
                }
            }
            _ => {
                // Ignore other event types (delete, rename, etc.)
            }
        }

        Ok(())
    }

    /// Process new entries from a session file
    async fn process_session_file(&self, file_path: &Path) -> Result<()> {
        let mut sessions = self.known_sessions.write().await;

        // Get or create watch state
        let state = sessions
            .entry(file_path.to_path_buf())
            .or_insert_with(Default::default);

        let last_pos = state.last_position;

        // Parse new entries from last position
        let (entries, new_pos) = parser::parse_jsonl_incremental(file_path, last_pos)?;

        if entries.is_empty() {
            return Ok(());
        }

        info!(
            "Processing {} new entries from {}",
            entries.len(),
            file_path.display()
        );

        // TODO: Send entries to Atlas for storage
        // For now, just log them
        for entry in entries {
            debug!("  Entry type: {:?}", entry.entry_type);
            if let Some(msg) = &entry.message {
                if let Some(content) = msg.content_as_string() {
                    let preview = content.chars().take(60).collect::<String>();
                    debug!("  Content: {}...", preview);
                }
            }
        }

        // Update last position
        state.last_position = new_pos;

        Ok(())
    }

    /// Check if we should watch a project directory based on filters
    fn should_watch_project(&self, project_path: &Path) -> bool {
        let path_str = project_path.to_string_lossy();

        // Check exclude patterns first
        for pattern in &self.config.exclude_projects {
            if path_str.contains(pattern) {
                return false;
            }
        }

        // If include patterns specified, path must match at least one
        if !self.config.include_projects.is_empty() {
            return self
                .config
                .include_projects
                .iter()
                .any(|pattern| path_str.contains(pattern));
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_watch_project_no_filters() {
        let config = WatchConfig::default();
        let watcher = TranscriptWatcher::new(config);

        // No filters = watch everything
        assert!(watcher.should_watch_project(Path::new("/some/project")));
    }

    #[test]
    fn test_should_watch_project_exclude() {
        let config = WatchConfig {
            exclude_projects: vec!["scratch".to_string(), "temp".to_string()],
            ..Default::default()
        };
        let watcher = TranscriptWatcher::new(config);

        assert!(!watcher.should_watch_project(Path::new("/projects/scratch")));
        assert!(!watcher.should_watch_project(Path::new("/projects/temp-work")));
        assert!(watcher.should_watch_project(Path::new("/projects/memex")));
    }

    #[test]
    fn test_should_watch_project_include() {
        let config = WatchConfig {
            include_projects: vec!["memex".to_string(), "work".to_string()],
            ..Default::default()
        };
        let watcher = TranscriptWatcher::new(config);

        assert!(watcher.should_watch_project(Path::new("/projects/memex")));
        assert!(watcher.should_watch_project(Path::new("/home/work/project")));
        assert!(!watcher.should_watch_project(Path::new("/projects/random")));
    }

    #[test]
    fn test_should_watch_project_both_filters() {
        let config = WatchConfig {
            include_projects: vec!["memex".to_string()],
            exclude_projects: vec!["archive".to_string()],
            ..Default::default()
        };
        let watcher = TranscriptWatcher::new(config);

        assert!(watcher.should_watch_project(Path::new("/projects/memex")));
        assert!(!watcher.should_watch_project(Path::new("/projects/memex-archive")));
        assert!(!watcher.should_watch_project(Path::new("/projects/other")));
    }
}
