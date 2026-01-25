//! Core types for Cortex

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a worker
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub String);

impl WorkerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string()[..8].to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Current state of a worker
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    /// Worker is starting up
    Starting,
    /// Worker is ready to receive messages
    Ready,
    /// Worker is processing a message
    Working,
    /// Worker is idle, waiting for work
    Idle,
    /// Worker has terminated
    Stopped,
    /// Worker encountered an error
    Error(String),
}

/// A single entry in the worker's conversation transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    /// When this exchange occurred
    pub timestamp: DateTime<Utc>,
    /// The message sent to the worker
    pub prompt: String,
    /// The response from the worker (None if still processing)
    pub response: Option<String>,
    /// Whether the response was an error
    pub is_error: bool,
    /// Duration in milliseconds (0 if still processing)
    pub duration_ms: u64,
}

/// Status information about a worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub id: WorkerId,
    pub state: WorkerState,
    /// Working directory / worktree path
    pub worktree: Option<String>,
    /// Task ID this worker is assigned to (links to task record)
    pub current_task: Option<String>,
    /// Hostname where this worker is running (for multi-host orchestration)
    pub host: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub messages_sent: u64,
    pub messages_received: u64,
}

impl WorkerStatus {
    pub fn new(id: WorkerId) -> Self {
        let now = Utc::now();
        Self {
            id,
            state: WorkerState::Starting,
            worktree: None,
            current_task: None,
            host: None,
            started_at: now,
            last_activity: now,
            messages_sent: 0,
            messages_received: 0,
        }
    }

    /// Set the hostname for this worker
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Set the current task for this worker
    pub fn with_task(mut self, task_id: impl Into<String>) -> Self {
        self.current_task = Some(task_id.into());
        self
    }

    /// Set the worktree path for this worker
    pub fn with_worktree(mut self, path: impl Into<String>) -> Self {
        self.worktree = Some(path.into());
        self
    }
}

/// MCP configuration for workers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkerMcpConfig {
    /// If true, only use MCP servers specified in `servers`, ignoring user's global MCP config.
    /// This prevents workers from inheriting user-scoped servers like Notion that may pop up auth dialogs.
    #[serde(default = "default_strict")]
    pub strict: bool,

    /// MCP server configurations to include. Each entry is a JSON string containing MCP config.
    /// If empty and strict is true, worker will have no MCP servers.
    #[serde(default)]
    pub servers: Vec<String>,
}

fn default_strict() -> bool {
    true // Workers should be isolated by default
}

impl WorkerMcpConfig {
    /// Create a new config with no MCP servers (fully isolated)
    pub fn none() -> Self {
        Self {
            strict: true,
            servers: vec![],
        }
    }

    /// Create a config that inherits user's MCP configuration
    pub fn inherit() -> Self {
        Self {
            strict: false,
            servers: vec![],
        }
    }

    /// Create a config with only specific MCP servers
    pub fn only(servers: Vec<String>) -> Self {
        Self {
            strict: true,
            servers,
        }
    }
}

/// Configuration for spawning a worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    /// Working directory for the worker (worktree path)
    pub cwd: String,

    /// Initial system prompt additions
    pub system_prompt: Option<String>,

    /// Initial message to send after startup
    pub initial_message: Option<String>,

    /// Model to use (defaults to sonnet)
    pub model: Option<String>,

    /// Maximum context tokens before summarization
    pub max_context: Option<u32>,

    /// MCP configuration for the worker.
    /// By default, workers are isolated and don't inherit user's MCP servers.
    #[serde(default)]
    pub mcp_config: Option<WorkerMcpConfig>,

    /// Enable Chrome browser integration for web UI testing.
    /// When true, passes --chrome flag to Claude CLI.
    #[serde(default)]
    pub chrome: bool,
}

impl WorkerConfig {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            system_prompt: None,
            initial_message: None,
            model: None,
            max_context: None,
            mcp_config: Some(WorkerMcpConfig::none()), // Isolated by default
            chrome: false,
        }
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_initial_message(mut self, message: impl Into<String>) -> Self {
        self.initial_message = Some(message.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Configure MCP servers for this worker
    pub fn with_mcp_config(mut self, config: WorkerMcpConfig) -> Self {
        self.mcp_config = Some(config);
        self
    }

    /// Make worker inherit user's MCP configuration
    pub fn with_inherited_mcp(mut self) -> Self {
        self.mcp_config = Some(WorkerMcpConfig::inherit());
        self
    }

    /// Give worker access to only specific MCP servers
    pub fn with_mcp_servers(mut self, servers: Vec<String>) -> Self {
        self.mcp_config = Some(WorkerMcpConfig::only(servers));
        self
    }

    /// Enable Chrome browser integration
    pub fn with_chrome(mut self, enabled: bool) -> Self {
        self.chrome = enabled;
        self
    }
}

