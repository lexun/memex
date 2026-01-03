//! Database configuration
//!
//! Supports both embedded RocksDB and remote SurrealDB connections.
//! Connection type is inferred from which fields are set.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Database configuration
///
/// Connection type is inferred:
/// - If `url` is set → remote connection
/// - If `path` is set (no `url`) → embedded RocksDB
/// - If neither → use default embedded path
/// - If both → error (ambiguous)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path for embedded RocksDB database
    pub path: Option<PathBuf>,

    /// URL for remote SurrealDB connection (e.g., "wss://cloud.surrealdb.com")
    pub url: Option<String>,

    /// Namespace (defaults to "memex")
    pub namespace: Option<String>,

    /// Username for remote connection
    pub username: Option<String>,

    /// Password for remote connection
    pub password: Option<String>,
}

impl DatabaseConfig {
    /// Create config for embedded database at the given path
    pub fn embedded(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Default::default()
        }
    }

    /// Create config for remote database
    pub fn remote(url: String, username: String, password: String) -> Self {
        Self {
            url: Some(url),
            username: Some(username),
            password: Some(password),
            ..Default::default()
        }
    }

    /// Check if this config is for a remote connection
    pub fn is_remote(&self) -> bool {
        self.url.is_some()
    }

    /// Get the namespace (defaults to "memex")
    pub fn namespace(&self) -> &str {
        self.namespace.as_deref().unwrap_or("memex")
    }
}
