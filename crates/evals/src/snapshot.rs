//! Snapshot-based test data management
//!
//! Enables loading pre-built knowledge bases for testing without reconstruction
//! overhead. Snapshots capture:
//! - Memos with content and source attribution
//! - Records with types and content
//! - Edges between records
//! - Extracted facts and entities
//!
//! # Example
//!
//! ```ignore
//! use std::path::Path;
//! use evals::snapshot::{Snapshot, SnapshotLoader};
//!
//! // Load a pre-built snapshot
//! let snapshot = Snapshot::load(Path::new("fixtures/snapshots/enterprise_kb.json"))?;
//!
//! // Apply to test database
//! let socket_path = Path::new("/tmp/memex.sock");
//! let loader = SnapshotLoader::new(socket_path);
//! loader.load(&snapshot).await?;
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A complete snapshot of a knowledge base state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Snapshot metadata
    pub meta: SnapshotMeta,
    /// Memos in the knowledge base
    pub memos: Vec<SnapshotMemo>,
    /// Records (entities with types)
    pub records: Vec<SnapshotRecord>,
    /// Edges between records
    pub edges: Vec<SnapshotEdge>,
    /// Extracted facts (optional, for advanced scenarios)
    #[serde(default)]
    pub facts: Vec<SnapshotFact>,
    /// Extracted entities (optional)
    #[serde(default)]
    pub entities: Vec<SnapshotEntity>,
}

/// Snapshot metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    /// Unique name for this snapshot
    pub name: String,
    /// Description of what this snapshot represents
    pub description: String,
    /// Version for compatibility checking
    #[serde(default = "default_version")]
    pub version: String,
    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,
    /// When this snapshot was created
    #[serde(default)]
    pub created_at: Option<String>,
}

fn default_version() -> String {
    "1.0".to_string()
}

/// A memo in the snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMemo {
    /// Memo content
    pub content: String,
    /// Source actor (e.g., "user", "agent:claude")
    #[serde(default = "default_actor")]
    pub source_actor: String,
    /// Source authority (e.g., "direct", "inferred")
    #[serde(default = "default_authority")]
    pub source_authority: String,
}

fn default_actor() -> String {
    "snapshot:loader".to_string()
}

fn default_authority() -> String {
    "test".to_string()
}

/// A record in the snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    /// Record type (person, team, repo, company, rule, skill, etc.)
    pub record_type: String,
    /// Record name
    pub name: String,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
    /// Optional additional content (type-specific fields)
    #[serde(default)]
    pub content: Option<serde_json::Value>,
}

/// An edge between records in the snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEdge {
    /// Source record name
    pub from: String,
    /// Relationship type
    pub relation: String,
    /// Target record name
    pub to: String,
    /// Optional metadata
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// A fact in the snapshot (for pre-extracted knowledge)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFact {
    /// Fact content
    pub content: String,
    /// Fact type
    #[serde(default = "default_fact_type")]
    pub fact_type: String,
    /// Confidence score
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    /// Project scope
    #[serde(default)]
    pub project: Option<String>,
    /// Entity names this fact mentions
    #[serde(default)]
    pub entities: Vec<String>,
}

fn default_fact_type() -> String {
    "statement".to_string()
}

fn default_confidence() -> f64 {
    1.0
}

/// An entity in the snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntity {
    /// Entity name
    pub name: String,
    /// Entity type
    #[serde(default = "default_entity_type")]
    pub entity_type: String,
    /// Description
    #[serde(default)]
    pub description: Option<String>,
    /// Project scope
    #[serde(default)]
    pub project: Option<String>,
    /// Aliases
    #[serde(default)]
    pub aliases: Vec<String>,
}

fn default_entity_type() -> String {
    "concept".to_string()
}

impl Snapshot {
    /// Create a new empty snapshot
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            meta: SnapshotMeta {
                name: name.into(),
                description: description.into(),
                version: default_version(),
                tags: Vec::new(),
                created_at: Some(chrono::Utc::now().to_rfc3339()),
            },
            memos: Vec::new(),
            records: Vec::new(),
            edges: Vec::new(),
            facts: Vec::new(),
            entities: Vec::new(),
        }
    }

    /// Load a snapshot from a JSON file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read snapshot file: {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse snapshot: {}", path.display()))
    }

    /// Load a snapshot from a TOML file
    pub fn load_toml(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read snapshot file: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse snapshot: {}", path.display()))
    }

    /// Save snapshot to a JSON file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize snapshot")?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write snapshot: {}", path.display()))
    }

    /// Add a memo to the snapshot
    pub fn add_memo(&mut self, content: impl Into<String>) -> &mut Self {
        self.memos.push(SnapshotMemo {
            content: content.into(),
            source_actor: default_actor(),
            source_authority: default_authority(),
        });
        self
    }

    /// Add a record to the snapshot
    pub fn add_record(
        &mut self,
        record_type: impl Into<String>,
        name: impl Into<String>,
        description: Option<String>,
    ) -> &mut Self {
        self.records.push(SnapshotRecord {
            record_type: record_type.into(),
            name: name.into(),
            description,
            content: None,
        });
        self
    }

    /// Add an edge to the snapshot
    pub fn add_edge(
        &mut self,
        from: impl Into<String>,
        relation: impl Into<String>,
        to: impl Into<String>,
    ) -> &mut Self {
        self.edges.push(SnapshotEdge {
            from: from.into(),
            relation: relation.into(),
            to: to.into(),
            metadata: None,
        });
        self
    }

    /// Get statistics about this snapshot
    pub fn stats(&self) -> SnapshotStats {
        SnapshotStats {
            memo_count: self.memos.len(),
            record_count: self.records.len(),
            edge_count: self.edges.len(),
            fact_count: self.facts.len(),
            entity_count: self.entities.len(),
        }
    }
}

/// Statistics about a snapshot
#[derive(Debug, Clone)]
pub struct SnapshotStats {
    pub memo_count: usize,
    pub record_count: usize,
    pub edge_count: usize,
    pub fact_count: usize,
    pub entity_count: usize,
}

impl std::fmt::Display for SnapshotStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} memos, {} records, {} edges, {} facts, {} entities",
            self.memo_count,
            self.record_count,
            self.edge_count,
            self.fact_count,
            self.entity_count
        )
    }
}

/// Loader for applying snapshots to a running memex instance
pub struct SnapshotLoader {
    socket_path: std::path::PathBuf,
}

impl SnapshotLoader {
    /// Create a new snapshot loader
    pub fn new(socket_path: &Path) -> Self {
        Self {
            socket_path: socket_path.to_path_buf(),
        }
    }

    /// Load a snapshot into the database
    ///
    /// This creates memos, records, and edges via the daemon API.
    /// Facts and entities are handled through the normal extraction pipeline
    /// when memos are recorded.
    pub async fn load(&self, snapshot: &Snapshot) -> Result<LoadResult> {
        let mut result = LoadResult::default();

        // Track record name -> ID mapping for edge creation
        let mut record_ids: HashMap<String, String> = HashMap::new();

        // Load memos first (they trigger extraction)
        for memo in &snapshot.memos {
            match self.record_memo(&memo.content, &memo.source_actor).await {
                Ok(_) => result.memos_loaded += 1,
                Err(e) => {
                    tracing::warn!("Failed to load memo: {}", e);
                    result.errors.push(format!("Memo: {}", e));
                }
            }
        }

        // Small delay to let extraction complete
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Load records and track their IDs
        for record in &snapshot.records {
            match self.create_record(record).await {
                Ok(id) => {
                    record_ids.insert(record.name.clone(), id);
                    result.records_loaded += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to load record {}: {}", record.name, e);
                    result.errors.push(format!("Record {}: {}", record.name, e));
                }
            }
        }

        // Load edges using tracked IDs
        for edge in &snapshot.edges {
            match self.create_edge_with_ids(edge, &record_ids).await {
                Ok(_) => result.edges_loaded += 1,
                Err(e) => {
                    tracing::warn!("Failed to load edge {} -> {}: {}", edge.from, edge.to, e);
                    result.errors.push(format!("Edge {} -> {}: {}", edge.from, edge.to, e));
                }
            }
        }

        Ok(result)
    }

    /// Record a memo via the daemon
    async fn record_memo(&self, content: &str, source_actor: &str) -> Result<String> {
        let client = atlas::MemoClient::new(&self.socket_path);
        let memo = client
            .record_memo(content, true, Some(source_actor))
            .await
            .context("Failed to record memo")?;
        Ok(memo.id_str().unwrap_or_default())
    }

    /// Create a record via the daemon
    async fn create_record(&self, record: &SnapshotRecord) -> Result<String> {
        let client = atlas::RecordClient::new(&self.socket_path);
        let created = client
            .create_record(
                &record.record_type,
                &record.name,
                record.description.as_deref(),
                record.content.clone(),
            )
            .await
            .context("Failed to create record")?;
        Ok(created.id_str().unwrap_or_default())
    }

    /// Create an edge using pre-tracked record IDs
    async fn create_edge_with_ids(
        &self,
        edge: &SnapshotEdge,
        record_ids: &HashMap<String, String>,
    ) -> Result<()> {
        let client = atlas::RecordClient::new(&self.socket_path);

        let from_id = record_ids
            .get(&edge.from)
            .with_context(|| format!("Source record not found: {}", edge.from))?;
        let to_id = record_ids
            .get(&edge.to)
            .with_context(|| format!("Target record not found: {}", edge.to))?;

        client
            .create_edge(from_id, to_id, &edge.relation, edge.metadata.clone())
            .await
            .context("Failed to create edge")?;

        Ok(())
    }
}

/// Result of loading a snapshot
#[derive(Debug, Clone, Default)]
pub struct LoadResult {
    pub memos_loaded: usize,
    pub records_loaded: usize,
    pub edges_loaded: usize,
    pub errors: Vec<String>,
}

impl LoadResult {
    pub fn success(&self) -> bool {
        self.errors.is_empty()
    }
}

impl std::fmt::Display for LoadResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Loaded: {} memos, {} records, {} edges",
            self.memos_loaded, self.records_loaded, self.edges_loaded
        )?;
        if !self.errors.is_empty() {
            write!(f, " ({} errors)", self.errors.len())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_creation() {
        let mut snapshot = Snapshot::new("test", "Test snapshot");
        snapshot
            .add_memo("TechCorp is a software company")
            .add_record("company", "TechCorp", Some("A software company".to_string()))
            .add_record("person", "Alice Smith", Some("CEO".to_string()))
            .add_edge("Alice Smith", "member_of", "TechCorp");

        assert_eq!(snapshot.memos.len(), 1);
        assert_eq!(snapshot.records.len(), 2);
        assert_eq!(snapshot.edges.len(), 1);
    }

    #[test]
    fn test_snapshot_serialization() {
        let mut snapshot = Snapshot::new("test", "Test snapshot");
        snapshot.add_memo("Test memo");
        snapshot.add_record("person", "Test Person", None);

        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: Snapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.meta.name, "test");
        assert_eq!(parsed.memos.len(), 1);
        assert_eq!(parsed.records.len(), 1);
    }

    #[test]
    fn test_snapshot_stats() {
        let mut snapshot = Snapshot::new("test", "Test");
        snapshot.add_memo("Memo 1");
        snapshot.add_memo("Memo 2");
        snapshot.add_record("person", "Alice", None);

        let stats = snapshot.stats();
        assert_eq!(stats.memo_count, 2);
        assert_eq!(stats.record_count, 1);
        assert_eq!(stats.edge_count, 0);
    }
}
