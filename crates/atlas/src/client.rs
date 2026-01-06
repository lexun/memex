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

/// Result of a knowledge rebuild
#[derive(Debug, Clone, Deserialize)]
pub struct RebuildResult {
    pub facts_deleted: usize,
    pub entities_deleted: usize,
    pub memos_processed: usize,
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
        #[derive(Serialize)]
        struct Params<'a> {
            query: &'a str,
            project: Option<&'a str>,
            limit: Option<usize>,
        }

        let result = self
            .client
            .request(
                "query_knowledge",
                Params {
                    query,
                    project,
                    limit,
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
}

/// Result of getting facts for an entity
#[derive(Debug, Clone, Deserialize)]
pub struct EntityFactsResult {
    pub entity: String,
    pub facts: Vec<Fact>,
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
