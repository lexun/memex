//! Database store for Atlas knowledge management
//!
//! Handles memo, event, fact, and entity storage.

use anyhow::{Context, Result};
use db::Database;
use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;

use crate::event::Event;
use crate::fact::{Entity, Fact};
use crate::memo::{Memo, MemoSource};

/// Database store for Atlas
#[derive(Clone)]
pub struct Store {
    db: Database,
}

impl Store {
    /// Create a new store with the given database connection
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get the database connection
    pub fn db(&self) -> &Database {
        &self.db
    }

    // ========== Memo Operations ==========

    /// Record a new memo
    pub async fn record_memo(&self, content: &str, source: MemoSource) -> Result<Memo> {
        let memo = Memo::new(content, source);

        let created: Option<Memo> = self
            .db
            .client()
            .create("memo")
            .content(memo)
            .await
            .context("Failed to create memo")?;

        created.context("Memo creation returned no result")
    }

    /// List memos with optional limit
    pub async fn list_memos(&self, limit: Option<usize>) -> Result<Vec<Memo>> {
        let query = match limit {
            Some(n) => format!("SELECT * FROM memo ORDER BY created_at DESC LIMIT {}", n),
            None => "SELECT * FROM memo ORDER BY created_at DESC".to_string(),
        };

        let mut response = self
            .db
            .client()
            .query(&query)
            .await
            .context("Failed to query memos")?;
        let memos: Vec<Memo> = response.take(0).context("Failed to parse memos")?;

        Ok(memos)
    }

    /// Get a memo by ID
    pub async fn get_memo(&self, id: &str) -> Result<Option<Memo>> {
        let memo: Option<Memo> = self
            .db
            .client()
            .select(("memo", id))
            .await
            .context("Failed to get memo")?;

        Ok(memo)
    }

    /// Delete a memo
    pub async fn delete_memo(&self, id: &str) -> Result<Option<Memo>> {
        let deleted: Option<Memo> = self
            .db
            .client()
            .delete(("memo", id))
            .await
            .context("Failed to delete memo")?;

        Ok(deleted)
    }

    // ========== Event Operations ==========

    /// Record a new event
    pub async fn record_event(&self, event: Event) -> Result<Event> {
        let created: Option<Event> = self
            .db
            .client()
            .create("event")
            .content(event)
            .await
            .context("Failed to create event")?;

        created.context("Event creation returned no result")
    }

    /// List events with optional filters
    pub async fn list_events(
        &self,
        event_type_prefix: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Event>> {
        let mut query = String::from("SELECT * FROM event");

        if let Some(prefix) = event_type_prefix {
            query.push_str(&format!(
                " WHERE string::starts_with(event_type, '{}')",
                prefix
            ));
        }

        query.push_str(" ORDER BY timestamp DESC");

        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }

        let mut response = self
            .db
            .client()
            .query(&query)
            .await
            .context("Failed to query events")?;
        let events: Vec<Event> = response.take(0).context("Failed to parse events")?;

        Ok(events)
    }

    /// Get an event by ID
    pub async fn get_event(&self, id: &str) -> Result<Option<Event>> {
        let event: Option<Event> = self
            .db
            .client()
            .select(("event", id))
            .await
            .context("Failed to get event")?;

        Ok(event)
    }

    // ========== Fact Operations ==========

    /// Create a new fact
    pub async fn create_fact(&self, fact: Fact) -> Result<Fact> {
        let created: Option<Fact> = self
            .db
            .client()
            .create("fact")
            .content(fact)
            .await
            .context("Failed to create fact")?;

        created.context("Fact creation returned no result")
    }

    /// Search facts using full-text search
    pub async fn search_facts(
        &self,
        query: &str,
        project: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<FactSearchResult>> {
        let mut sql = String::from(
            "SELECT *, search::score(1) AS score FROM fact WHERE content @1@ $query",
        );

        if project.is_some() {
            sql.push_str(" AND project = $project");
        }

        sql.push_str(" ORDER BY score DESC");

        if let Some(n) = limit {
            sql.push_str(&format!(" LIMIT {}", n));
        }

        tracing::info!("Searching facts with query: {}", sql);

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("query", query.to_string()))
            .bind(("project", project.map(|s| s.to_string())))
            .await
            .context("Failed to search facts")?;

        // Deserialize directly to the target type
        let results: Vec<FactSearchResult> = response
            .take(0)
            .context("Failed to parse search results")?;

        Ok(results)
    }

    /// Get a fact by ID
    pub async fn get_fact(&self, id: &str) -> Result<Option<Fact>> {
        let fact: Option<Fact> = self
            .db
            .client()
            .select(("fact", id))
            .await
            .context("Failed to get fact")?;

        Ok(fact)
    }

    /// List facts with optional project filter
    pub async fn list_facts(&self, project: Option<&str>, limit: Option<usize>) -> Result<Vec<Fact>> {
        let mut query = String::from("SELECT * FROM fact");

        if project.is_some() {
            query.push_str(" WHERE project = $project");
        }

        query.push_str(" ORDER BY created_at DESC");

        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }

        tracing::info!("Listing facts with query: {}", query);

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("project", project.map(|s| s.to_string())))
            .await
            .context("Failed to query facts")?;

        let facts: Vec<Fact> = response.take(0).context("Failed to parse facts")?;
        tracing::info!("Found {} facts", facts.len());
        Ok(facts)
    }

    // ========== Entity Operations ==========

    /// Create a new entity (or return existing if name+project match)
    pub async fn create_entity(&self, entity: Entity) -> Result<Entity> {
        let created: Option<Entity> = self
            .db
            .client()
            .create("entity")
            .content(entity)
            .await
            .context("Failed to create entity")?;

        created.context("Entity creation returned no result")
    }

    /// Find entity by name (exact match) within project
    pub async fn find_entity_by_name(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> Result<Option<Entity>> {
        let sql = "SELECT * FROM entity WHERE name = $name AND project = $project LIMIT 1";

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("name", name.to_string()))
            .bind(("project", project.map(|s| s.to_string())))
            .await
            .context("Failed to query entity")?;

        let entities: Vec<Entity> = response.take(0).context("Failed to parse entity")?;
        Ok(entities.into_iter().next())
    }

    /// Get an entity by ID
    pub async fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        let entity: Option<Entity> = self
            .db
            .client()
            .select(("entity", id))
            .await
            .context("Failed to get entity")?;

        Ok(entity)
    }

    /// Add source episodes to an existing entity
    ///
    /// This is used to merge provenance when the same entity is mentioned
    /// in multiple memos/episodes.
    pub async fn add_entity_source_episodes(
        &self,
        entity_id: &Thing,
        new_episodes: &[crate::fact::EpisodeRef],
    ) -> Result<()> {
        if new_episodes.is_empty() {
            return Ok(());
        }

        // Use SURQL UPDATE to append to array, avoiding duplicates
        let sql = r#"
            UPDATE $entity_id SET
                source_episodes += $new_episodes,
                updated_at = time::now()
        "#;

        self.db
            .client()
            .query(sql)
            .bind(("entity_id", entity_id.clone()))
            .bind(("new_episodes", new_episodes.to_vec()))
            .await
            .context("Failed to add source episodes to entity")?;

        Ok(())
    }

    /// List entities with optional project and type filter
    pub async fn list_entities(
        &self,
        project: Option<&str>,
        entity_type: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>> {
        let mut query = String::from("SELECT * FROM entity");
        let mut conditions = Vec::new();

        if project.is_some() {
            conditions.push("project = $project");
        }
        if entity_type.is_some() {
            conditions.push("entity_type = $entity_type");
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        query.push_str(" ORDER BY name ASC");

        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("project", project.map(|s| s.to_string())))
            .bind(("entity_type", entity_type.map(|s| s.to_string())))
            .await
            .context("Failed to query entities")?;

        let entities: Vec<Entity> = response.take(0).context("Failed to parse entities")?;
        Ok(entities)
    }

    /// Link a fact to an entity
    pub async fn link_fact_entity(
        &self,
        fact_id: &Thing,
        entity_id: &Thing,
        role: &str,
    ) -> Result<()> {
        // Use regular CREATE for compatibility with SCHEMAFULL table
        let sql = "CREATE fact_entity SET fact = $fact, entity = $entity, role = $role";

        self.db
            .client()
            .query(sql)
            .bind(("fact", fact_id.clone()))
            .bind(("entity", entity_id.clone()))
            .bind(("role", role.to_string()))
            .await
            .context("Failed to link fact to entity")?;

        Ok(())
    }

    /// Get facts related to an entity
    pub async fn get_facts_for_entity(&self, entity_id: &str) -> Result<Vec<Fact>> {
        // Query via fact_entity join table
        let sql = r#"
            SELECT * FROM fact WHERE id IN (
                SELECT fact FROM fact_entity WHERE entity = type::thing("entity", $entity_id)
            )
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("entity_id", entity_id.to_string()))
            .await
            .context("Failed to get facts for entity")?;

        let facts: Vec<Fact> = response.take(0).unwrap_or_default();
        Ok(facts)
    }

    /// Get facts related to an entity by name
    pub async fn get_facts_for_entity_name(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> Result<Vec<Fact>> {
        // First find the entity
        let entity = self.find_entity_by_name(name, project).await?;

        match entity {
            Some(e) => {
                let entity_id = e.id_str().unwrap_or_default();
                self.get_facts_for_entity(&entity_id).await
            }
            None => Ok(Vec::new()),
        }
    }

    /// Get entities mentioned by a fact
    pub async fn get_entities_for_fact(&self, fact_id: &str) -> Result<Vec<Entity>> {
        // Query via fact_entity join table
        let sql = r#"
            SELECT * FROM entity WHERE id IN (
                SELECT entity FROM fact_entity WHERE fact = type::thing("fact", $fact_id)
            )
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("fact_id", fact_id.to_string()))
            .await
            .context("Failed to get entities for fact")?;

        let entities: Vec<Entity> = response.take(0).unwrap_or_default();
        Ok(entities)
    }

    /// Find facts related to a given fact via shared entities
    ///
    /// Returns facts that share at least one entity with the source fact.
    pub async fn get_related_facts(&self, fact_id: &str, limit: Option<usize>) -> Result<Vec<Fact>> {
        let limit = limit.unwrap_or(10);

        // Find facts that share entities with the given fact via join table
        // 1. Get entities linked to this fact
        // 2. Get other facts linked to those entities
        let sql = r#"
            SELECT * FROM fact WHERE id IN (
                SELECT fact FROM fact_entity WHERE entity IN (
                    SELECT entity FROM fact_entity WHERE fact = type::thing("fact", $fact_id)
                )
            ) AND id != type::thing("fact", $fact_id)
            LIMIT $limit
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("fact_id", fact_id.to_string()))
            .bind(("limit", limit))
            .await
            .context("Failed to get related facts")?;

        let facts: Vec<Fact> = response.take(0).unwrap_or_default();
        Ok(facts)
    }

    /// Hybrid search combining BM25 text search and vector similarity
    ///
    /// Uses reciprocal rank fusion (RRF) to combine results from both searches.
    /// Falls back to BM25-only if no query embedding is provided.
    pub async fn hybrid_search_facts(
        &self,
        text_query: &str,
        query_embedding: Option<&[f32]>,
        project: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<FactSearchResult>> {
        let limit = limit.unwrap_or(10);

        // BM25 search
        let bm25_results = self.search_facts(text_query, project, Some(limit * 2)).await?;
        tracing::info!(
            "BM25 search for '{}': {} results",
            text_query,
            bm25_results.len()
        );

        // If no embedding, return BM25 results only
        let query_embedding = match query_embedding {
            Some(e) if !e.is_empty() => {
                tracing::info!("Query embedding provided: {} dimensions", e.len());
                e
            }
            _ => {
                tracing::info!("No query embedding, returning BM25-only results");
                return Ok(bm25_results.into_iter().take(limit).collect());
            }
        };

        // Vector similarity search (gracefully handle failures)
        let vector_results = match self
            .vector_search_facts(query_embedding, project, Some(limit * 2))
            .await
        {
            Ok(results) => {
                tracing::info!("Vector search: {} results", results.len());
                results
            }
            Err(e) => {
                tracing::warn!("Vector search failed, using BM25 only: {}", e);
                return Ok(bm25_results.into_iter().take(limit).collect());
            }
        };

        // Combine using reciprocal rank fusion
        let combined = self.fuse_results(bm25_results, vector_results, limit);
        tracing::info!("Combined results after RRF: {}", combined.len());
        Ok(combined)
    }

    /// Search facts using vector similarity
    async fn vector_search_facts(
        &self,
        query_embedding: &[f32],
        project: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<FactSearchResult>> {
        // SurrealDB vector search uses the KNN operator <|K,D|>
        // K = number of results, D = distance (optional, defaults to index distance)
        let k = limit.unwrap_or(10);

        let mut sql = format!(
            "SELECT *, vector::similarity::cosine(embedding, $embedding) AS score \
             FROM fact WHERE embedding <|{k}|> $embedding"
        );

        if project.is_some() {
            sql.push_str(" AND project = $project");
        }

        sql.push_str(" ORDER BY score DESC");

        tracing::info!(
            "Vector search: k={}, embedding_dims={}, query: {}",
            k,
            query_embedding.len(),
            sql
        );

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("embedding", query_embedding.to_vec()))
            .bind(("project", project.map(|s| s.to_string())))
            .await
            .context("Failed to vector search facts")?;

        let results: Vec<FactSearchResult> = response
            .take(0)
            .context("Failed to parse vector search results")?;

        tracing::info!("Vector search returned {} results", results.len());
        Ok(results)
    }

    /// Count facts with and without embeddings (diagnostic)
    pub async fn count_fact_embeddings(&self) -> Result<(usize, usize)> {
        // Count facts with non-empty embeddings
        let sql_with = "SELECT count() FROM fact WHERE array::len(embedding) > 0 GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(sql_with)
            .await
            .context("Failed to count facts with embeddings")?;
        let with_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let with_embeddings = with_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Count facts with empty embeddings
        let sql_without = "SELECT count() FROM fact WHERE array::len(embedding) = 0 GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(sql_without)
            .await
            .context("Failed to count facts without embeddings")?;
        let without_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let without_embeddings = without_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        Ok((with_embeddings, without_embeddings))
    }

    /// Combine results using Reciprocal Rank Fusion (RRF)
    ///
    /// Uses RRF for ranking but preserves original scores for display.
    /// Results appearing in both lists get boosted in ranking.
    fn fuse_results(
        &self,
        bm25_results: Vec<FactSearchResult>,
        vector_results: Vec<FactSearchResult>,
        limit: usize,
    ) -> Vec<FactSearchResult> {
        use std::collections::HashMap;

        const K: f64 = 60.0; // RRF constant

        // Map: id -> (result, rrf_score, bm25_score, vector_score)
        let mut scores: HashMap<String, (FactSearchResult, f64, f64, f64)> = HashMap::new();

        // Add BM25 results
        for (rank, result) in bm25_results.into_iter().enumerate() {
            let id = result
                .id
                .as_ref()
                .map(|t| t.id.to_raw())
                .unwrap_or_default();
            let rrf_score = 1.0 / (K + rank as f64 + 1.0);
            let bm25_score = result.score;
            scores.insert(id, (result, rrf_score, bm25_score, 0.0));
        }

        // Add vector results
        for (rank, result) in vector_results.into_iter().enumerate() {
            let id = result
                .id
                .as_ref()
                .map(|t| t.id.to_raw())
                .unwrap_or_default();
            let rrf_score = 1.0 / (K + rank as f64 + 1.0);
            let vector_score = result.score;

            scores
                .entry(id.clone())
                .and_modify(|(_, existing_rrf, _, existing_vector)| {
                    *existing_rrf += rrf_score;
                    *existing_vector = vector_score; // Capture vector score
                })
                .or_insert((result, rrf_score, 0.0, vector_score)); // Vector-only
        }

        // Sort by combined RRF score and take top N
        let mut results: Vec<_> = scores.into_values().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        results
            .into_iter()
            .take(limit)
            .map(|(mut result, _rrf_score, bm25_score, vector_score)| {
                // Display scoring logic:
                // - BM25 scores can be negative with small corpora
                // - Vector similarity is 0.0-1.0 (cosine)
                // - Prefer positive scores, boost if found by both methods
                let in_both = bm25_score != 0.0 && vector_score != 0.0;

                result.score = if in_both {
                    // Found by both - use the better score, boosted
                    let best = if bm25_score > 0.0 { bm25_score } else { vector_score };
                    best * 1.5
                } else if bm25_score > 0.0 {
                    bm25_score
                } else if vector_score > 0.0 {
                    vector_score
                } else {
                    0.01 // Fallback
                };
                result
            })
            .collect()
    }

    // ========== Rebuild Operations ==========

    /// Delete all derived data (facts, entities, fact_entity relationships)
    ///
    /// This preserves immutable data (memos, events) while clearing
    /// the knowledge graph for rebuild.
    pub async fn delete_derived_data(&self, project: Option<&str>) -> Result<(usize, usize)> {
        let (fact_sql, entity_sql, link_sql) = if let Some(p) = project {
            (
                format!("DELETE FROM fact WHERE project = '{}'", p),
                format!("DELETE FROM entity WHERE project = '{}'", p),
                format!("DELETE FROM fact_entity WHERE fact.project = '{}'", p),
            )
        } else {
            (
                "DELETE FROM fact".to_string(),
                "DELETE FROM entity".to_string(),
                "DELETE FROM fact_entity".to_string(),
            )
        };

        // Delete fact_entity links first (references facts and entities)
        self.db
            .client()
            .query(&link_sql)
            .await
            .context("Failed to delete fact_entity links")?;

        // Count before deleting
        let fact_count = self.db.client()
            .query("SELECT count() FROM fact GROUP ALL")
            .await
            .ok()
            .and_then(|mut r| r.take::<Option<serde_json::Value>>(0).ok().flatten())
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        let entity_count = self.db.client()
            .query("SELECT count() FROM entity GROUP ALL")
            .await
            .ok()
            .and_then(|mut r| r.take::<Option<serde_json::Value>>(0).ok().flatten())
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Delete facts
        self.db
            .client()
            .query(&fact_sql)
            .await
            .context("Failed to delete facts")?;

        // Delete entities
        self.db
            .client()
            .query(&entity_sql)
            .await
            .context("Failed to delete entities")?;

        Ok((fact_count, entity_count))
    }
}

/// Search result with score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactSearchResult {
    /// Unique identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<surrealdb::sql::Thing>,

    /// The fact content
    pub content: String,

    /// Fact type
    #[serde(default)]
    pub fact_type: String,

    /// Confidence score
    #[serde(default = "default_confidence")]
    pub confidence: f32,

    /// Project context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// Source episodes
    #[serde(default)]
    pub source_episodes: Vec<crate::fact::EpisodeRef>,

    /// When created
    pub created_at: surrealdb::sql::Datetime,

    /// When updated
    pub updated_at: surrealdb::sql::Datetime,

    /// Access count
    #[serde(default)]
    pub access_count: u32,

    /// Last accessed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<surrealdb::sql::Datetime>,

    /// Vector embedding
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embedding: Vec<f32>,

    /// Search score
    #[serde(default)]
    pub score: f64,
}

fn default_confidence() -> f32 {
    1.0
}

