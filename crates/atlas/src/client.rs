//! IPC client for memo and event operations
//!
//! Provides access to memo and event storage via the daemon.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::fact::{Entity, Fact};
use crate::Memo;
use ipc::Client as IpcClient;

/// Client for memo operations via the daemon
#[derive(Debug, Clone)]
pub struct MemoClient {
    client: IpcClient,
}

impl MemoClient {
    /// Create a new client for the given socket path
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            client: IpcClient::new(socket_path),
        }
    }

    /// Check if the daemon is running and responsive
    pub async fn health_check(&self) -> Result<bool> {
        self.client.health_check().await
    }

    /// Record a new memo
    ///
    /// If `user_directed` is true, the memo is recorded with user authority
    /// (the agent is acting as a messenger for the user).
    /// If false, the memo is recorded with agent authority
    /// (the agent is recording on its own initiative).
    pub async fn record_memo(
        &self,
        content: &str,
        user_directed: bool,
        actor: Option<&str>,
    ) -> Result<Memo> {
        #[derive(Serialize)]
        struct Params<'a> {
            content: &'a str,
            user_directed: bool,
            actor: Option<&'a str>,
        }

        let result = self
            .client
            .request(
                "record_memo",
                Params {
                    content,
                    user_directed,
                    actor,
                },
            )
            .await
            .context("Failed to record memo")?;

        serde_json::from_value(result).context("Failed to parse memo response")
    }

    /// List memos with optional limit
    pub async fn list_memos(&self, limit: Option<usize>) -> Result<Vec<Memo>> {
        #[derive(Serialize)]
        struct Params {
            limit: Option<usize>,
        }

        let result = self
            .client
            .request("list_memos", Params { limit })
            .await
            .context("Failed to list memos")?;

        serde_json::from_value(result).context("Failed to parse memos response")
    }

    /// Get a memo by ID
    pub async fn get_memo(&self, id: &str) -> Result<Option<Memo>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("get_memo", Params { id })
            .await
            .context("Failed to get memo")?;

        if result.is_null() {
            return Ok(None);
        }

        let memo: Memo = serde_json::from_value(result).context("Failed to parse memo response")?;
        Ok(Some(memo))
    }

    /// Delete a memo
    pub async fn delete_memo(&self, id: &str) -> Result<Option<Memo>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("delete_memo", Params { id })
            .await
            .context("Failed to delete memo")?;

        if result.is_null() {
            return Ok(None);
        }

        let memo: Memo =
            serde_json::from_value(result).context("Failed to parse memo response")?;
        Ok(Some(memo))
    }
}

/// Client for event operations via the daemon
#[derive(Debug, Clone)]
pub struct EventClient {
    client: IpcClient,
}

impl EventClient {
    /// Create a new client for the given socket path
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            client: IpcClient::new(socket_path),
        }
    }

    /// List events with optional filters
    pub async fn list_events(
        &self,
        event_type_prefix: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Event>> {
        #[derive(Serialize)]
        struct Params<'a> {
            event_type_prefix: Option<&'a str>,
            limit: Option<usize>,
        }

        let result = self
            .client
            .request(
                "list_events",
                Params {
                    event_type_prefix,
                    limit,
                },
            )
            .await
            .context("Failed to list events")?;

        serde_json::from_value(result).context("Failed to parse events response")
    }

    /// Get an event by ID
    pub async fn get_event(&self, id: &str) -> Result<Option<Event>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("get_event", Params { id })
            .await
            .context("Failed to get event")?;

        if result.is_null() {
            return Ok(None);
        }

        let event: Event =
            serde_json::from_value(result).context("Failed to parse event response")?;
        Ok(Some(event))
    }
}

/// Response from search_knowledge
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SearchResult {
    pub query: String,
    pub results: Vec<serde_json::Value>,
    pub count: usize,
}

/// Response from query_knowledge
#[derive(Debug, Clone, serde::Deserialize)]
pub struct QueryResult {
    pub query: String,
    pub answer: String,
    pub facts_used: usize,
}

/// Response from extract_facts IPC call
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExtractFactsResult {
    pub memos_processed: usize,
    pub facts_created: usize,
    pub entities_created: usize,
    #[serde(default)]
    pub links_created: usize,
}

/// Result of backfilling embeddings
#[derive(Debug, Clone, Deserialize)]
pub struct BackfillResult {
    pub facts_processed: usize,
    pub facts_updated: usize,
    pub facts_remaining: usize,
}

/// Result of a knowledge rebuild
#[derive(Debug, Clone, Deserialize)]
pub struct RebuildResult {
    pub facts_deleted: usize,
    pub entities_deleted: usize,
    pub memos_processed: usize,
    #[serde(default)]
    pub tasks_processed: usize,
    #[serde(default)]
    pub notes_processed: usize,
    pub facts_created: usize,
    pub entities_created: usize,
    #[serde(default)]
    pub links_created: usize,
}

/// Client for knowledge operations via the daemon
#[derive(Debug, Clone)]
pub struct KnowledgeClient {
    client: IpcClient,
}

impl KnowledgeClient {
    /// Create a new client for the given socket path
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            client: IpcClient::new(socket_path),
        }
    }

    /// Query knowledge and get an LLM-summarized answer
    pub async fn query(
        &self,
        query: &str,
        project: Option<&str>,
        limit: Option<usize>,
    ) -> Result<QueryResult> {
        self.query_with_context(query, project, limit, None).await
    }

    /// Query knowledge with optional graph context from a record
    ///
    /// If record_id is provided, rules and skills that apply to that record
    /// will be included in the query context for the LLM.
    pub async fn query_with_context(
        &self,
        query: &str,
        project: Option<&str>,
        limit: Option<usize>,
        record_id: Option<&str>,
    ) -> Result<QueryResult> {
        #[derive(Serialize)]
        struct Params<'a> {
            query: &'a str,
            project: Option<&'a str>,
            limit: Option<usize>,
            record_id: Option<&'a str>,
        }

        let result = self
            .client
            .request(
                "query_knowledge",
                Params {
                    query,
                    project,
                    limit,
                    record_id,
                },
            )
            .await
            .context("Failed to query knowledge")?;

        serde_json::from_value(result).context("Failed to parse query response")
    }

    /// Search for raw facts matching a query
    pub async fn search(
        &self,
        query: &str,
        project: Option<&str>,
        limit: Option<usize>,
    ) -> Result<SearchResult> {
        #[derive(Serialize)]
        struct Params<'a> {
            query: &'a str,
            project: Option<&'a str>,
            limit: Option<usize>,
        }

        let result = self
            .client
            .request(
                "search_knowledge",
                Params {
                    query,
                    project,
                    limit,
                },
            )
            .await
            .context("Failed to search knowledge")?;

        serde_json::from_value(result).context("Failed to parse search response")
    }

    /// Extract facts from memos (for backfill)
    pub async fn extract_facts(
        &self,
        project: Option<&str>,
        batch_size: Option<usize>,
    ) -> Result<ExtractFactsResult> {
        #[derive(Serialize)]
        struct Params<'a> {
            project: Option<&'a str>,
            batch_size: Option<usize>,
        }

        let result = self
            .client
            .request(
                "extract_facts",
                Params {
                    project,
                    batch_size,
                },
            )
            .await
            .context("Failed to extract facts")?;

        serde_json::from_value(result).context("Failed to parse extraction response")
    }

    /// Backfill embeddings for facts that are missing them
    ///
    /// Unlike rebuild, this preserves existing facts and only adds embeddings.
    pub async fn backfill_embeddings(&self, batch_size: Option<usize>) -> Result<BackfillResult> {
        #[derive(Serialize)]
        struct Params {
            batch_size: Option<usize>,
        }

        let result = self
            .client
            .request("backfill_embeddings", Params { batch_size })
            .await
            .context("Failed to backfill embeddings")?;

        serde_json::from_value(result).context("Failed to parse backfill response")
    }

    /// Rebuild the knowledge graph from scratch
    ///
    /// Deletes all derived data and re-extracts from all memos.
    pub async fn rebuild(&self, project: Option<&str>) -> Result<RebuildResult> {
        #[derive(Serialize)]
        struct Params<'a> {
            project: Option<&'a str>,
        }

        let result = self
            .client
            .request("rebuild_knowledge", Params { project })
            .await
            .context("Failed to rebuild knowledge")?;

        serde_json::from_value(result).context("Failed to parse rebuild response")
    }

    /// Get knowledge system status (diagnostic)
    pub async fn status(&self) -> Result<KnowledgeStatus> {
        let result = self
            .client
            .request("knowledge_status", serde_json::json!({}))
            .await
            .context("Failed to get knowledge status")?;

        serde_json::from_value(result).context("Failed to parse status response")
    }

    /// List entities with optional filtering
    pub async fn list_entities(
        &self,
        project: Option<&str>,
        entity_type: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>> {
        #[derive(Serialize)]
        struct Params<'a> {
            project: Option<&'a str>,
            entity_type: Option<&'a str>,
            limit: Option<usize>,
        }

        let result = self
            .client
            .request(
                "list_entities",
                Params {
                    project,
                    entity_type,
                    limit,
                },
            )
            .await
            .context("Failed to list entities")?;

        serde_json::from_value(result).context("Failed to parse entities")
    }

    /// Get facts related to an entity
    pub async fn get_entity_facts(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> Result<EntityFactsResult> {
        #[derive(Serialize)]
        struct Params<'a> {
            name: &'a str,
            project: Option<&'a str>,
        }

        let result = self
            .client
            .request("get_entity_facts", Params { name, project })
            .await
            .context("Failed to get entity facts")?;

        serde_json::from_value(result).context("Failed to parse entity facts")
    }

    /// Get facts related to a given fact via shared entities
    ///
    /// Returns facts that share entities with the given fact, enabling
    /// graph traversal through the knowledge graph.
    pub async fn get_related_facts(
        &self,
        fact_id: &str,
        limit: Option<usize>,
    ) -> Result<RelatedFactsResult> {
        #[derive(Serialize)]
        struct Params<'a> {
            fact_id: &'a str,
            limit: Option<usize>,
        }

        let result = self
            .client
            .request("get_related_facts", Params { fact_id, limit })
            .await
            .context("Failed to get related facts")?;

        serde_json::from_value(result).context("Failed to parse related facts")
    }
}

/// Result of getting facts for an entity
#[derive(Debug, Clone, Deserialize)]
pub struct EntityFactsResult {
    pub entity: String,
    pub facts: Vec<Fact>,
    pub count: usize,
}

/// Result of getting related facts via graph traversal
#[derive(Debug, Clone, Deserialize)]
pub struct RelatedFactsResult {
    pub fact_id: String,
    pub related_facts: Vec<Fact>,
    pub count: usize,
}

/// Knowledge system status
#[derive(Debug, Clone, Deserialize)]
pub struct KnowledgeStatus {
    pub facts: FactStats,
    pub llm_configured: bool,
}

/// Fact statistics
#[derive(Debug, Clone, Deserialize)]
pub struct FactStats {
    pub total: usize,
    pub with_embeddings: usize,
    pub without_embeddings: usize,
}

// ============================================================================
// Record Client - Graph/Record management
// ============================================================================

use crate::record::{ContextAssembly, Record, RecordEdge};

/// Client for managing records and edges in the graph
pub struct RecordClient {
    client: IpcClient,
}

impl RecordClient {
    /// Create a new client for the given socket path
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            client: IpcClient::new(socket_path),
        }
    }

    /// List records with optional filters
    pub async fn list_records(
        &self,
        record_type: Option<&str>,
        include_deleted: bool,
        limit: Option<usize>,
    ) -> Result<Vec<Record>> {
        #[derive(Serialize)]
        struct Params<'a> {
            record_type: Option<&'a str>,
            include_deleted: bool,
            limit: Option<usize>,
        }

        let result = self
            .client
            .request(
                "list_records",
                Params {
                    record_type,
                    include_deleted,
                    limit,
                },
            )
            .await
            .context("Failed to list records")?;

        serde_json::from_value(result).context("Failed to parse records")
    }

    /// Get a record by ID
    pub async fn get_record(&self, id: &str) -> Result<Option<Record>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("get_record", Params { id })
            .await
            .context("Failed to get record")?;

        serde_json::from_value(result).context("Failed to parse record")
    }

    /// Create a new record
    pub async fn create_record(
        &self,
        record_type: &str,
        name: &str,
        description: Option<&str>,
        content: Option<serde_json::Value>,
    ) -> Result<Record> {
        #[derive(Serialize)]
        struct Params<'a> {
            record_type: &'a str,
            name: &'a str,
            description: Option<&'a str>,
            content: Option<serde_json::Value>,
        }

        let result = self
            .client
            .request(
                "create_record",
                Params {
                    record_type,
                    name,
                    description,
                    content,
                },
            )
            .await
            .context("Failed to create record")?;

        serde_json::from_value(result).context("Failed to parse created record")
    }

    /// Update a record
    pub async fn update_record(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        content: Option<serde_json::Value>,
    ) -> Result<Option<Record>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
            name: Option<&'a str>,
            description: Option<&'a str>,
            content: Option<serde_json::Value>,
        }

        let result = self
            .client
            .request(
                "update_record",
                Params {
                    id,
                    name,
                    description,
                    content,
                },
            )
            .await
            .context("Failed to update record")?;

        serde_json::from_value(result).context("Failed to parse updated record")
    }

    /// Delete a record (soft-delete)
    pub async fn delete_record(&self, id: &str) -> Result<Option<Record>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("delete_record", Params { id })
            .await
            .context("Failed to delete record")?;

        serde_json::from_value(result).context("Failed to parse deleted record")
    }

    /// Create an edge between two records
    pub async fn create_edge(
        &self,
        source: &str,
        target: &str,
        relation: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<RecordEdge> {
        #[derive(Serialize)]
        struct Params<'a> {
            source: &'a str,
            target: &'a str,
            relation: &'a str,
            metadata: Option<serde_json::Value>,
        }

        let result = self
            .client
            .request(
                "create_edge",
                Params {
                    source,
                    target,
                    relation,
                    metadata,
                },
            )
            .await
            .context("Failed to create edge")?;

        serde_json::from_value(result).context("Failed to parse created edge")
    }

    /// List edges from/to a record
    pub async fn list_edges(
        &self,
        id: &str,
        direction: &str,
    ) -> Result<Vec<RecordEdge>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
            direction: &'a str,
        }

        let result = self
            .client
            .request("list_edges", Params { id, direction })
            .await
            .context("Failed to list edges")?;

        serde_json::from_value(result).context("Failed to parse edges")
    }

    /// Delete an edge
    pub async fn delete_edge(&self, id: &str) -> Result<Option<RecordEdge>> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
        }

        let result = self
            .client
            .request("delete_edge", Params { id })
            .await
            .context("Failed to delete edge")?;

        serde_json::from_value(result).context("Failed to parse deleted edge")
    }

    /// Assemble context from a record
    pub async fn assemble_context(
        &self,
        id: &str,
        depth: usize,
    ) -> Result<ContextAssembly> {
        #[derive(Serialize)]
        struct Params<'a> {
            id: &'a str,
            depth: usize,
        }

        let result = self
            .client
            .request("assemble_context", Params { id, depth })
            .await
            .context("Failed to assemble context")?;

        serde_json::from_value(result).context("Failed to parse context assembly")
    }
}
