//! Database store for Atlas knowledge management
//!
//! Handles memo, event, fact, and entity storage.

use std::time::Instant;

use anyhow::{Context, Result};
use db::Database;
use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;
use tracing::debug;

use crate::event::Event;
use crate::fact::{Entity, Fact, FactType};
use crate::memo::{Memo, MemoSource};
use crate::record::{ContextAssembly, EdgeRelation, Record, RecordEdge};

/// Normalize an identity query for matching
///
/// Strips common prefixes (@, #) and lowercases the input.
/// This allows "Luke", "@luke", and "LUKE" to all match the same record.
pub fn normalize_identity_query(query: &str) -> String {
    let trimmed = query.trim();
    let stripped = trimmed
        .strip_prefix('@')
        .or_else(|| trimmed.strip_prefix('#'))
        .unwrap_or(trimmed);
    stripped.to_lowercase()
}

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
        let start = Instant::now();

        let query = match limit {
            Some(n) => format!("SELECT * FROM memo ORDER BY created_at DESC LIMIT {}", n),
            None => "SELECT * FROM memo ORDER BY created_at DESC".to_string(),
        };

        let query_start = Instant::now();
        let mut response = self
            .db
            .client()
            .query(&query)
            .await
            .context("Failed to query memos")?;
        let query_elapsed = query_start.elapsed();

        let parse_start = Instant::now();
        let memos: Vec<Memo> = response.take(0).context("Failed to parse memos")?;
        let parse_elapsed = parse_start.elapsed();

        let total_elapsed = start.elapsed();
        debug!(
            operation = "list_memos",
            total_ms = total_elapsed.as_micros() as f64 / 1000.0,
            query_ms = query_elapsed.as_micros() as f64 / 1000.0,
            parse_ms = parse_elapsed.as_micros() as f64 / 1000.0,
            memo_count = memos.len(),
            "DB timing breakdown"
        );

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
        self.search_facts_temporal(query, project, limit, None, None).await
    }

    /// Search facts using full-text search with optional temporal filtering
    ///
    /// By default, only returns current (non-superseded) facts.
    pub async fn search_facts_temporal(
        &self,
        query: &str,
        project: Option<&str>,
        limit: Option<usize>,
        date_start: Option<chrono::DateTime<chrono::Utc>>,
        date_end: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<FactSearchResult>> {
        // Only return current facts (not superseded)
        let mut sql = String::from(
            "SELECT *, search::score(1) AS score FROM fact WHERE content @1@ $query AND superseded_by IS NONE",
        );

        if project.is_some() {
            sql.push_str(" AND project = $project");
        }

        if date_start.is_some() {
            sql.push_str(" AND created_at >= $date_start");
        }

        if date_end.is_some() {
            sql.push_str(" AND created_at < $date_end");
        }

        sql.push_str(" ORDER BY score DESC");

        if let Some(n) = limit {
            sql.push_str(&format!(" LIMIT {}", n));
        }

        tracing::info!("Searching facts with query: {}", sql);

        // Convert to SurrealDB datetime format
        let date_start_surreal = date_start.map(|d| surrealdb::sql::Datetime::from(d));
        let date_end_surreal = date_end.map(|d| surrealdb::sql::Datetime::from(d));

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("query", query.to_string()))
            .bind(("project", project.map(|s| s.to_string())))
            .bind(("date_start", date_start_surreal))
            .bind(("date_end", date_end_surreal))
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

    /// Supersede a fact with a new one
    ///
    /// This marks the old fact as superseded (sets valid_until and superseded_by)
    /// and creates a new fact with the updated content. Used when knowledge changes over time.
    ///
    /// Example: "TechFlow uses PostgreSQL" → "TechFlow uses SurrealDB"
    pub async fn supersede_fact(
        &self,
        old_fact_id: &str,
        new_content: &str,
        via_event: Option<&str>,
    ) -> Result<Fact> {
        // Get the old fact
        let old_fact = self
            .get_fact(old_fact_id)
            .await?
            .context("Old fact not found")?;

        // Create the new fact with same metadata but new content
        let fact_type: FactType = old_fact.fact_type.parse().unwrap_or_default();
        let new_fact = Fact::new(new_content, fact_type, old_fact.confidence)
            .with_project(old_fact.project.as_deref().unwrap_or(""));

        // Add source episodes from the old fact
        let mut new_fact = new_fact;
        new_fact.source_episodes = old_fact.source_episodes.clone();

        // Create the new fact
        let created_fact = self.create_fact(new_fact).await?;
        let new_fact_id = created_fact.id.clone();

        // Update the old fact to mark it as superseded
        let now = surrealdb::sql::Datetime::default();
        let sql = r#"
            UPDATE type::thing('fact', $old_id) SET
                valid_until = $now,
                superseded_by = $new_id,
                superseded_via = $via_event
        "#;

        self.db
            .client()
            .query(sql)
            .bind(("old_id", old_fact_id.to_string()))
            .bind(("now", now))
            .bind(("new_id", new_fact_id))
            .bind(("via_event", via_event.map(|s| s.to_string())))
            .await
            .context("Failed to update old fact")?;

        Ok(created_fact)
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

    /// Find entity by name (case-insensitive match) within project
    ///
    /// When project is None, searches across all projects.
    /// When project is Some, only matches entities in that project.
    pub async fn find_entity_by_name(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> Result<Option<Entity>> {
        // Use case-insensitive matching; when project is None, search all projects
        let sql = if project.is_some() {
            "SELECT * FROM entity WHERE string::lowercase(name) = string::lowercase($name) AND project = $project LIMIT 1"
        } else {
            "SELECT * FROM entity WHERE string::lowercase(name) = string::lowercase($name) LIMIT 1"
        };

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

    /// Search entities by name (case-insensitive partial match)
    ///
    /// Returns entities whose name contains the search term (case-insensitive).
    /// Used for entity-focused query expansion.
    pub async fn search_entities_by_name(
        &self,
        query: &str,
        project: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>> {
        let limit = limit.unwrap_or(5);

        // Case-insensitive contains match using string::lowercase
        let sql = if project.is_some() {
            r#"SELECT * FROM entity
               WHERE string::lowercase(name) CONTAINS string::lowercase($query)
               AND project = $project
               LIMIT $limit"#
        } else {
            r#"SELECT * FROM entity
               WHERE string::lowercase(name) CONTAINS string::lowercase($query)
               LIMIT $limit"#
        };

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("query", query.to_string()))
            .bind(("project", project.map(|s| s.to_string())))
            .bind(("limit", limit as i64))
            .await
            .context("Failed to search entities")?;

        let entities: Vec<Entity> = response.take(0).unwrap_or_default();
        Ok(entities)
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
    ///
    /// First tries exact match (case-insensitive), then falls back to partial search.
    /// This allows users to query with partial names or different casing.
    pub async fn get_facts_for_entity_name(
        &self,
        name: &str,
        project: Option<&str>,
    ) -> Result<Vec<Fact>> {
        // First try exact match (case-insensitive)
        let entity = self.find_entity_by_name(name, project).await?;

        if let Some(e) = entity {
            let entity_id = e.id_str().unwrap_or_default();
            return self.get_facts_for_entity(&entity_id).await;
        }

        // Fall back to partial search if exact match fails
        let entities = self.search_entities_by_name(name, project, Some(1)).await?;

        match entities.into_iter().next() {
            Some(e) => {
                let entity_id = e.id_str().unwrap_or_default();
                self.get_facts_for_entity(&entity_id).await
            }
            None => Ok(Vec::new()),
        }
    }

    /// Expand query by finding entity-linked facts
    ///
    /// Searches for entities matching query terms, then returns facts linked
    /// to those entities with a discounted score (for ranking below direct matches).
    pub async fn expand_via_entities(
        &self,
        query: &str,
        project: Option<&str>,
        entity_score: f64,
        limit: Option<usize>,
    ) -> Result<Vec<FactSearchResult>> {
        // Search for entities matching the query term
        let entities = self.search_entities_by_name(query, project, Some(3)).await?;

        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let limit = limit.unwrap_or(5);
        let mut results = Vec::new();

        for entity in entities {
            let entity_id = entity.id_str().unwrap_or_default();
            let facts = self.get_facts_for_entity(&entity_id).await?;

            for fact in facts {
                // Convert Fact to FactSearchResult with entity-based score
                results.push(FactSearchResult {
                    id: fact.id,
                    content: fact.content,
                    fact_type: fact.fact_type.to_string(),
                    confidence: fact.confidence,
                    project: fact.project,
                    source_episodes: fact.source_episodes,
                    created_at: fact.created_at,
                    updated_at: fact.updated_at,
                    access_count: fact.access_count,
                    last_accessed: fact.last_accessed,
                    embedding: fact.embedding,
                    score: entity_score, // Fixed score for entity-linked facts
                });

                if results.len() >= limit {
                    break;
                }
            }

            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
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
        self.hybrid_search_facts_temporal(text_query, query_embedding, project, limit, None, None).await
    }

    /// Hybrid search with optional temporal filtering
    pub async fn hybrid_search_facts_temporal(
        &self,
        text_query: &str,
        query_embedding: Option<&[f32]>,
        project: Option<&str>,
        limit: Option<usize>,
        date_start: Option<chrono::DateTime<chrono::Utc>>,
        date_end: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<FactSearchResult>> {
        let limit = limit.unwrap_or(10);

        // BM25 search
        let bm25_results = self.search_facts_temporal(text_query, project, Some(limit * 2), date_start, date_end).await?;
        tracing::info!(
            "BM25 search for '{}': {} results (temporal: {:?} - {:?})",
            text_query,
            bm25_results.len(),
            date_start,
            date_end
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
            .vector_search_facts_temporal(query_embedding, project, Some(limit * 2), date_start, date_end)
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
    pub async fn vector_search_facts(
        &self,
        query_embedding: &[f32],
        project: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<FactSearchResult>> {
        self.vector_search_facts_temporal(query_embedding, project, limit, None, None).await
    }

    /// Search facts using vector similarity with temporal filtering
    ///
    /// By default, only returns current (non-superseded) facts.
    pub async fn vector_search_facts_temporal(
        &self,
        query_embedding: &[f32],
        project: Option<&str>,
        limit: Option<usize>,
        date_start: Option<chrono::DateTime<chrono::Utc>>,
        date_end: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<FactSearchResult>> {
        // SurrealDB vector search uses the KNN operator <|K,D|>
        // K = number of results, D = distance (optional, defaults to index distance)
        let k = limit.unwrap_or(10);

        // Only return current facts (not superseded)
        let mut sql = format!(
            "SELECT *, vector::similarity::cosine(embedding, $embedding) AS score \
             FROM fact WHERE embedding <|{k}|> $embedding AND superseded_by IS NONE"
        );

        if project.is_some() {
            sql.push_str(" AND project = $project");
        }

        if date_start.is_some() {
            sql.push_str(" AND created_at >= $date_start");
        }

        if date_end.is_some() {
            sql.push_str(" AND created_at < $date_end");
        }

        sql.push_str(" ORDER BY score DESC");

        tracing::info!(
            "Vector search: k={}, embedding_dims={}, query: {}",
            k,
            query_embedding.len(),
            sql
        );

        // Convert to SurrealDB datetime format
        let date_start_surreal = date_start.map(|d| surrealdb::sql::Datetime::from(d));
        let date_end_surreal = date_end.map(|d| surrealdb::sql::Datetime::from(d));

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("embedding", query_embedding.to_vec()))
            .bind(("project", project.map(|s| s.to_string())))
            .bind(("date_start", date_start_surreal))
            .bind(("date_end", date_end_surreal))
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
        // Count total facts
        let sql_total = "SELECT count() FROM fact GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(sql_total)
            .await
            .context("Failed to count facts")?;
        let total_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let total = total_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Count facts with non-empty embeddings (handle null with coalesce)
        let sql_with = "SELECT count() FROM fact WHERE array::len(embedding ?? []) > 0 GROUP ALL";
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

        let without_embeddings = total.saturating_sub(with_embeddings);

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

    // ========== Backfill Operations ==========

    /// Get facts that are missing embeddings
    pub async fn get_facts_without_embeddings(&self, limit: Option<usize>) -> Result<Vec<Fact>> {
        let mut sql = "SELECT * FROM fact WHERE embedding IS NONE OR array::len(embedding) = 0".to_string();

        if let Some(n) = limit {
            sql.push_str(&format!(" LIMIT {}", n));
        }

        let mut response = self
            .db
            .client()
            .query(&sql)
            .await
            .context("Failed to query facts without embeddings")?;

        let facts: Vec<Fact> = response.take(0).context("Failed to parse facts")?;
        Ok(facts)
    }

    /// Update a fact's embedding
    pub async fn update_fact_embedding(
        &self,
        fact_id: &surrealdb::sql::Thing,
        embedding: Vec<f32>,
    ) -> Result<()> {
        let sql = "UPDATE $fact_id SET embedding = $embedding, updated_at = time::now()";

        self.db
            .client()
            .query(sql)
            .bind(("fact_id", fact_id.clone()))
            .bind(("embedding", embedding))
            .await
            .context("Failed to update fact embedding")?;

        Ok(())
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

// ========== Record Operations (in Store impl) ==========

impl Store {
    // ========== Record CRUD Operations ==========

    /// Create a new record
    pub async fn create_record(&self, record: Record) -> Result<Record> {
        let created: Option<Record> = self
            .db
            .client()
            .create("record")
            .content(record)
            .await
            .context("Failed to create record")?;

        created.context("Record creation returned no result")
    }

    /// Get a record by ID
    pub async fn get_record(&self, id: &str) -> Result<Option<Record>> {
        let record: Option<Record> = self
            .db
            .client()
            .select(("record", id))
            .await
            .context("Failed to get record")?;

        Ok(record)
    }

    /// Update a record's fields
    pub async fn update_record(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        content: Option<serde_json::Value>,
        record_type: Option<&str>,
    ) -> Result<Option<Record>> {
        let mut updates = vec!["updated_at = time::now()".to_string()];

        if let Some(n) = name {
            updates.push(format!("name = '{}'", n.replace('\'', "\\'")));
        }
        if let Some(d) = description {
            updates.push(format!("description = '{}'", d.replace('\'', "\\'")));
        }
        if content.is_some() {
            updates.push("content = $content".to_string());
        }
        if let Some(rt) = record_type {
            updates.push(format!("record_type = '{}'", rt.replace('\'', "\\'")));
        }

        let sql = format!(
            "UPDATE record:{} SET {}",
            id,
            updates.join(", ")
        );

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("content", content))
            .await
            .context("Failed to update record")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Update a record's fields including aliases
    ///
    /// This is an extended version of update_record that also allows updating aliases.
    /// Used primarily by merge operations where we need to preserve identity aliases.
    pub async fn update_record_with_aliases(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        content: Option<serde_json::Value>,
        record_type: Option<&str>,
        aliases: Option<Vec<String>>,
    ) -> Result<Option<Record>> {
        let mut updates = vec!["updated_at = time::now()".to_string()];

        if let Some(n) = name {
            updates.push(format!("name = '{}'", n.replace('\'', "\\'")));
        }
        if let Some(d) = description {
            updates.push(format!("description = '{}'", d.replace('\'', "\\'")));
        }
        if content.is_some() {
            updates.push("content = $content".to_string());
        }
        if let Some(rt) = record_type {
            updates.push(format!("record_type = '{}'", rt.replace('\'', "\\'")));
        }
        if aliases.is_some() {
            updates.push("aliases = $aliases".to_string());
        }

        let sql = format!(
            "UPDATE record:{} SET {}",
            id,
            updates.join(", ")
        );

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("content", content))
            .bind(("aliases", aliases))
            .await
            .context("Failed to update record")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Add an alias to a record
    ///
    /// Adds a new alias to an existing record's alias list.
    /// Does nothing if the alias already exists (case-insensitive check).
    pub async fn add_record_alias(&self, id: &str, alias: &str) -> Result<Option<Record>> {
        // First get the current record to check existing aliases
        let record = self.get_record(id).await?;
        let Some(record) = record else {
            return Ok(None);
        };

        // Check if alias already exists (case-insensitive)
        let alias_lower = alias.to_lowercase();
        if record.aliases.iter().any(|a| a.to_lowercase() == alias_lower) {
            return Ok(Some(record));
        }
        if record.name.to_lowercase() == alias_lower {
            return Ok(Some(record));
        }

        // Add the new alias
        let mut new_aliases = record.aliases;
        new_aliases.push(alias.to_string());

        self.update_record_with_aliases(id, None, None, None, None, Some(new_aliases))
            .await
    }

    /// Soft-delete a record (sets deleted_at timestamp)
    pub async fn delete_record(&self, id: &str) -> Result<Option<Record>> {
        let sql = "UPDATE type::thing('record', $id) SET deleted_at = time::now(), updated_at = time::now()";

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("id", id.to_string()))
            .await
            .context("Failed to soft-delete record")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Get a record by type and name (unique index)
    pub async fn get_record_by_type_name(
        &self,
        record_type: &str,
        name: &str,
    ) -> Result<Option<Record>> {
        let sql = "SELECT * FROM record WHERE record_type = $type AND name = $name LIMIT 1";

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("type", record_type.to_string()))
            .bind(("name", name.to_string()))
            .await
            .context("Failed to get record by type+name")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Get a record by its forge_id (stored in content.forge_id)
    /// Used for dual-write sync between Forge tasks and Records
    pub async fn get_record_by_forge_id(&self, forge_id: &str) -> Result<Option<Record>> {
        let sql = "SELECT * FROM record WHERE content.forge_id = $forge_id AND deleted_at IS NONE LIMIT 1";

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("forge_id", forge_id.to_string()))
            .await
            .context("Failed to get record by forge_id")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// List records with optional filters
    pub async fn list_records(
        &self,
        record_type: Option<&str>,
        include_deleted: bool,
        limit: Option<usize>,
    ) -> Result<Vec<Record>> {
        let mut query = String::from("SELECT * FROM record");
        let mut conditions = Vec::new();

        if !include_deleted {
            conditions.push("deleted_at IS NONE");
        }
        if record_type.is_some() {
            conditions.push("record_type = $record_type");
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
            .bind(("record_type", record_type.map(|s| s.to_string())))
            .await
            .context("Failed to query records")?;

        let records: Vec<Record> = response.take(0).context("Failed to parse records")?;
        Ok(records)
    }

    /// Search records by name (case-insensitive partial match)
    pub async fn search_records(
        &self,
        query: &str,
        record_type: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Record>> {
        let mut sql = String::from(
            "SELECT * FROM record WHERE deleted_at IS NONE AND \
             (string::lowercase(name) CONTAINS string::lowercase($query) OR \
              string::lowercase(description ?? '') CONTAINS string::lowercase($query))"
        );

        if record_type.is_some() {
            sql.push_str(" AND record_type = $record_type");
        }

        sql.push_str(" ORDER BY name ASC");

        if let Some(n) = limit {
            sql.push_str(&format!(" LIMIT {}", n));
        }

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("query", query.to_string()))
            .bind(("record_type", record_type.map(|s| s.to_string())))
            .await
            .context("Failed to search records")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records)
    }

    /// Find record by type and name (exact match)
    pub async fn find_record_by_name(
        &self,
        record_type: &str,
        name: &str,
    ) -> Result<Option<Record>> {
        let sql = "SELECT * FROM record WHERE record_type = $record_type AND name = $name AND deleted_at IS NONE LIMIT 1";

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("record_type", record_type.to_string()))
            .bind(("name", name.to_string()))
            .await
            .context("Failed to find record")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Find record by name or alias with case-insensitive matching
    ///
    /// Searches for a record where either:
    /// - The name matches (case-insensitive)
    /// - One of the aliases matches (case-insensitive)
    ///
    /// Common prefixes (@, #) are stripped during matching, so:
    /// - "Luke" matches "luke", "@luke", "Luke"
    /// - "@lexun" matches "lexun", "@lexun"
    ///
    /// Optionally filter by record type for more precise matching.
    pub async fn find_record_by_name_or_alias(
        &self,
        query: &str,
        record_type: Option<&str>,
    ) -> Result<Option<Record>> {
        // Normalize the query: strip common prefixes and lowercase
        let normalized = normalize_identity_query(query);

        // Build SQL query that checks both name and aliases (case-insensitive)
        let mut sql = String::from(
            r#"SELECT * FROM record WHERE deleted_at IS NONE AND (
                string::lowercase(name) = $normalized OR
                $normalized IN aliases.map(|$a| string::lowercase($a)) OR
                string::lowercase(name) = $query_lower OR
                $query_lower IN aliases.map(|$a| string::lowercase($a))
            )"#
        );

        if record_type.is_some() {
            sql.push_str(" AND record_type = $record_type");
        }

        sql.push_str(" LIMIT 1");

        let query_lower = query.to_lowercase();

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("normalized", normalized))
            .bind(("query_lower", query_lower))
            .bind(("record_type", record_type.map(|s| s.to_string())))
            .await
            .context("Failed to find record by name or alias")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Get extraction context with existing records for disambiguation
    ///
    /// Returns summaries of existing records grouped by type, which can be
    /// injected into the extraction prompt to help the LLM link to existing
    /// records rather than creating duplicates.
    pub async fn get_extraction_context(&self) -> Result<crate::record_extraction::ExtractionContext> {
        use crate::record_extraction::{ExtractionContext, RecordSummary};

        // Helper to convert records to summaries (including aliases for disambiguation)
        fn to_summaries(records: Vec<Record>) -> Vec<RecordSummary> {
            records
                .into_iter()
                .map(|r| RecordSummary {
                    id: r.id_str().unwrap_or_default(),
                    name: r.name,
                    aliases: r.aliases,
                    description: r.description,
                })
                .collect()
        }

        // Fetch all record types in parallel would be nice, but sequential is fine for now
        let repos = self.list_records(Some("repo"), false, None).await?;
        let projects = self.list_records(Some("initiative"), false, None).await?;
        let people = self.list_records(Some("person"), false, None).await?;
        let teams = self.list_records(Some("team"), false, None).await?;
        let rules = self.list_records(Some("rule"), false, None).await?;
        let companies = self.list_records(Some("company"), false, None).await?;

        Ok(ExtractionContext {
            repos: to_summaries(repos),
            projects: to_summaries(projects),
            people: to_summaries(people),
            teams: to_summaries(teams),
            rules: to_summaries(rules),
            companies: to_summaries(companies),
        })
    }

    /// Process extraction results: create/update records and edges
    ///
    /// Returns a summary of what was created/updated plus any questions that
    /// need human clarification.
    pub async fn process_extraction_results(
        &self,
        results: &crate::record_extraction::RecordExtractionResult,
        confidence_threshold: f32,
    ) -> Result<crate::record_extraction::ExtractionProcessingResult> {
        use crate::record_extraction::{RecordAction, ExtractionProcessingResult};
        use std::collections::HashMap;

        let mut created_records = Vec::new();
        let mut updated_records = Vec::new();
        let mut created_edges = Vec::new();
        let mut skipped_low_confidence = Vec::new();

        // Map from name -> record ID for linking
        let mut name_to_id: HashMap<String, String> = HashMap::new();

        // Process records
        for extracted in &results.records {
            if extracted.confidence < confidence_threshold {
                skipped_low_confidence.push(extracted.name.clone());
                continue;
            }

            match extracted.action {
                RecordAction::Create => {
                    let record_type: crate::record::RecordType = extracted
                        .record_type
                        .parse()
                        .unwrap_or(crate::record::RecordType::Document);

                    let mut record = Record::new(record_type, &extracted.name);
                    if let Some(desc) = &extracted.description {
                        record = record.with_description(desc);
                    }
                    if !extracted.content.is_null() {
                        record = record.with_content(extracted.content.clone());
                    }

                    match self.create_record(record).await {
                        Ok(created) => {
                            if let Some(id) = created.id_str() {
                                name_to_id.insert(extracted.name.clone(), id);
                            }
                            created_records.push(created.name);
                        }
                        Err(e) => {
                            debug!("Failed to create record {}: {}", extracted.name, e);
                        }
                    }
                }
                RecordAction::Update => {
                    if let Some(existing_id) = &extracted.existing_id {
                        name_to_id.insert(extracted.name.clone(), existing_id.clone());

                        if let Err(e) = self
                            .update_record(
                                existing_id,
                                None, // Don't update name
                                extracted.description.as_deref(),
                                if extracted.content.is_null() {
                                    None
                                } else {
                                    Some(extracted.content.clone())
                                },
                                None, // Don't update record_type
                            )
                            .await
                        {
                            debug!("Failed to update record {}: {}", existing_id, e);
                        } else {
                            updated_records.push(extracted.name.clone());
                        }
                    }
                }
                RecordAction::Reference => {
                    if let Some(existing_id) = &extracted.existing_id {
                        name_to_id.insert(extracted.name.clone(), existing_id.clone());
                    }
                }
            }
        }

        // Process links
        for link in &results.links {
            if link.confidence < confidence_threshold {
                continue;
            }

            // Resolve source and target to IDs
            let source_id = name_to_id.get(&link.source).cloned().unwrap_or_else(|| link.source.clone());
            let target_id = name_to_id.get(&link.target).cloned().unwrap_or_else(|| link.target.clone());

            let relation: EdgeRelation = link
                .relation
                .parse()
                .unwrap_or(EdgeRelation::RelatedTo);

            match self.create_edge(&source_id, &target_id, relation, None).await {
                Ok(_) => {
                    created_edges.push(format!("{} --{}-> {}", link.source, link.relation, link.target));
                }
                Err(e) => {
                    debug!("Failed to create edge {} -> {}: {}", source_id, target_id, e);
                }
            }
        }

        Ok(ExtractionProcessingResult {
            created_records,
            updated_records,
            created_edges,
            skipped_low_confidence,
            questions: results.questions.clone(),
        })
    }

    // ========== Edge Operations ==========

    /// Create an edge between two records
    pub async fn create_edge(
        &self,
        source_id: &str,
        target_id: &str,
        relation: EdgeRelation,
        metadata: Option<serde_json::Value>,
    ) -> Result<RecordEdge> {
        let source = Thing::from(("record", source_id));
        let target = Thing::from(("record", target_id));

        let edge = RecordEdge::new(source, target, relation);
        let edge = if let Some(m) = metadata {
            edge.with_metadata(m)
        } else {
            edge
        };

        let created: Option<RecordEdge> = self
            .db
            .client()
            .create("record_edge")
            .content(edge)
            .await
            .context("Failed to create edge")?;

        created.context("Edge creation returned no result")
    }

    /// Create an edge from a pre-built RecordEdge (for migrations)
    pub async fn create_edge_raw(&self, edge: RecordEdge) -> Result<RecordEdge> {
        let created: Option<RecordEdge> = self
            .db
            .client()
            .create("record_edge")
            .content(edge)
            .await
            .context("Failed to create edge")?;

        created.context("Edge creation returned no result")
    }

    /// Create an edge with custom relation string and timestamp (for migrations)
    pub async fn create_edge_with_details(
        &self,
        source_id: &str,
        target_id: &str,
        relation: &str,
        created_at: surrealdb::sql::Datetime,
    ) -> Result<RecordEdge> {
        let source = Thing::from(("record", source_id));
        let target = Thing::from(("record", target_id));

        let mut edge = RecordEdge::new(source, target, EdgeRelation::RelatedTo)
            .with_created_at(created_at);
        edge.relation = relation.to_string();

        let created: Option<RecordEdge> = self
            .db
            .client()
            .create("record_edge")
            .content(edge)
            .await
            .context("Failed to create edge")?;

        created.context("Edge creation returned no result")
    }

    /// Delete an edge by ID
    pub async fn delete_edge(&self, id: &str) -> Result<Option<RecordEdge>> {
        let deleted: Option<RecordEdge> = self
            .db
            .client()
            .delete(("record_edge", id))
            .await
            .context("Failed to delete edge")?;

        Ok(deleted)
    }

    /// Supersede an edge with a new one
    ///
    /// This marks the old edge as superseded (sets valid_until and superseded_by)
    /// and creates a new edge. Used when relationships change over time.
    ///
    /// Example: "TechFlow uses PostgreSQL" → "TechFlow uses SurrealDB"
    pub async fn supersede_edge(
        &self,
        old_edge_id: &str,
        new_target_id: &str,
        via_event: Option<&str>,
    ) -> Result<RecordEdge> {
        // Get the old edge
        let old_edge: Option<RecordEdge> = self
            .db
            .client()
            .select(("record_edge", old_edge_id))
            .await
            .context("Failed to fetch old edge")?;

        let old_edge = old_edge.context("Old edge not found")?;

        // Create the new edge with the same source and relation but new target
        let source_id = old_edge.source.id.to_raw();
        let relation: EdgeRelation = old_edge.relation.parse()
            .map_err(|e: String| anyhow::anyhow!(e))?;

        let new_edge = self.create_edge(&source_id, new_target_id, relation, None).await?;
        let new_edge_id = new_edge.id.clone();

        // Update the old edge to mark it as superseded
        let now = surrealdb::sql::Datetime::default();
        let sql = r#"
            UPDATE type::thing('record_edge', $old_id) SET
                valid_until = $now,
                superseded_by = $new_id,
                superseded_via = $via_event
        "#;

        self.db
            .client()
            .query(sql)
            .bind(("old_id", old_edge_id.to_string()))
            .bind(("now", now))
            .bind(("new_id", new_edge_id))
            .bind(("via_event", via_event.map(|s| s.to_string())))
            .await
            .context("Failed to update old edge")?;

        Ok(new_edge)
    }

    /// Get all edges from a source record
    ///
    /// If `current_only` is true, only returns edges that haven't been superseded
    /// (no valid_until set). This is typically what you want for context assembly.
    pub async fn get_edges_from(
        &self,
        source_id: &str,
        relation: Option<EdgeRelation>,
        current_only: bool,
    ) -> Result<Vec<RecordEdge>> {
        let mut sql = String::from(
            "SELECT * FROM record_edge WHERE source = type::thing('record', $source_id)"
        );

        if relation.is_some() {
            sql.push_str(" AND relation = $relation");
        }

        if current_only {
            sql.push_str(" AND superseded_by IS NONE");
        }

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("source_id", source_id.to_string()))
            .bind(("relation", relation.map(|r| r.to_string())))
            .await
            .context("Failed to get edges from record")?;

        let edges: Vec<RecordEdge> = response.take(0).unwrap_or_default();
        Ok(edges)
    }

    /// Get all edges to a target record
    ///
    /// If `current_only` is true, only returns edges that haven't been superseded.
    pub async fn get_edges_to(
        &self,
        target_id: &str,
        relation: Option<EdgeRelation>,
        current_only: bool,
    ) -> Result<Vec<RecordEdge>> {
        let mut sql = String::from(
            "SELECT * FROM record_edge WHERE target = type::thing('record', $target_id)"
        );

        if relation.is_some() {
            sql.push_str(" AND relation = $relation");
        }

        if current_only {
            sql.push_str(" AND superseded_by IS NONE");
        }

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("target_id", target_id.to_string()))
            .bind(("relation", relation.map(|r| r.to_string())))
            .await
            .context("Failed to get edges to record")?;

        let edges: Vec<RecordEdge> = response.take(0).unwrap_or_default();
        Ok(edges)
    }

    // ========== Graph Traversal for Context Assembly ==========

    /// Traverse the record graph from a starting point to collect context
    ///
    /// This is the core capability for autonomous agent orchestration. Given a starting
    /// record (typically a Repo or Task), it traverses the knowledge graph to collect
    /// all relevant context needed for an agent to work autonomously.
    ///
    /// # Edge Traversal Rules
    ///
    /// The algorithm uses BFS with asymmetric edge traversal:
    ///
    /// ## Outgoing edges (current record → target)
    ///
    /// Only follows **hierarchy** edges upward:
    /// - `belongs_to`: repo → team, team → company, task → project
    /// - `part_of`: component → project, sub-task → parent-task
    ///
    /// ## Incoming edges (source → current record)
    ///
    /// Follows **applicability** edges to find what applies to this record:
    /// - `applies_to`: rule → repo (rules that govern this codebase)
    /// - `available_to`: skill → repo (capabilities for this codebase)
    /// - `member_of`: person → team (team members)
    /// - `part_of`: document → project (documents in this project)
    /// - `owns`: person → repo (ownership)
    /// - `contains`: project → document (containment)
    ///
    /// # Typical Graph Structure for Worker Context
    ///
    /// ```text
    /// Starting from a Repo:
    ///
    ///   ┌─────────────────────────────────────────────┐
    ///   │                   Company                   │
    ///   └────────────────────▲────────────────────────┘
    ///                        │ belongs_to
    ///   ┌────────────────────┴────────────────────────┐
    ///   │                    Team                     │
    ///   └────────────────────▲────────────────────────┘
    ///         member_of ┌────┴────┐ belongs_to
    ///                   │         │
    ///   ┌───────────────┴──┐  ┌───┴───────────────────┐
    ///   │      Person      │  │         Repo          │◄── START
    ///   └──────────────────┘  └───▲───────────▲───────┘
    ///                             │           │
    ///                   applies_to│           │available_to
    ///                   ┌─────────┴──┐   ┌────┴────────────┐
    ///                   │    Rule    │   │      Skill      │
    ///                   └────────────┘   └─────────────────┘
    /// ```
    ///
    /// # Output
    ///
    /// Returns a [`ContextAssembly`] containing:
    /// - All collected records (rules, skills, people, repos, teams, etc.)
    /// - A traversal path for debugging
    ///
    /// The assembly can be converted to a system prompt via [`ContextAssembly::to_system_prompt()`]
    /// which formats rules, skills, and team info for agent consumption.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let context = store.assemble_context(&repo_id, 3).await?;
    /// let system_prompt = context.to_system_prompt();
    /// let mcp_configs = context.mcp_server_configs();
    /// ```
    pub async fn assemble_context(
        &self,
        start_record_id: &str,
        max_depth: usize,
    ) -> Result<ContextAssembly> {
        let mut assembly = ContextAssembly::default();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut to_visit = vec![(start_record_id.to_string(), 0usize)];

        while let Some((record_id, depth)) = to_visit.pop() {
            if visited.contains(&record_id) || depth > max_depth {
                continue;
            }
            visited.insert(record_id.clone());

            // Get the record
            if let Some(record) = self.get_record(&record_id).await? {
                if record.is_deleted() {
                    continue;
                }

                assembly.traversal_path.push(format!(
                    "{}:{} (depth {})",
                    record.record_type, record.name, depth
                ));
                assembly.records.push(record);
            }

            // Get outgoing edges (this record -> targets)
            // Only traverse current edges (not superseded ones)
            let edges_out = self.get_edges_from(&record_id, None, true).await?;
            for edge in edges_out {
                let target_id = edge.target.id.to_raw();
                if !visited.contains(&target_id) {
                    // Follow hierarchy edges upward:
                    // - belongs_to (repo belongs to team, team belongs to company)
                    // - part_of (component is part of project, task is part of goal)
                    if edge.relation == "belongs_to" || edge.relation == "part_of" {
                        to_visit.push((target_id, depth + 1));
                    }
                }
            }

            // Get incoming edges (sources -> this record)
            // Only traverse current edges (not superseded ones)
            let edges_in = self.get_edges_to(&record_id, None, true).await?;
            for edge in edges_in {
                let source_id = edge.source.id.to_raw();
                if !visited.contains(&source_id) {
                    // Collect records that point TO this record:
                    // - rules/skills that apply_to or are available_to this record
                    // - people who are member_of (if this is a team)
                    // - components/documents that are part_of this record (hierarchy)
                    // - people/teams that own this record
                    // - documents/entries that are contains in this record
                    if edge.relation == "applies_to"
                        || edge.relation == "available_to"
                        || edge.relation == "member_of"
                        || edge.relation == "part_of"
                        || edge.relation == "owns"
                        || edge.relation == "contains"
                    {
                        to_visit.push((source_id, depth + 1));
                    }
                }
            }
        }

        Ok(assembly)
    }

    // ========== Task Operations ==========

    /// Create a new task record
    ///
    /// This is a convenience method that creates a Record with record_type = "task"
    /// and the appropriate content structure.
    pub async fn create_task(
        &self,
        title: &str,
        description: Option<&str>,
        project: Option<&str>,
        priority: i32,
    ) -> Result<Record> {
        use crate::record::{RecordType, TaskContent, TaskStatus};

        let content = TaskContent {
            status: TaskStatus::Pending,
            priority,
            impact: crate::record::Impact::from_priority(priority),
            urgency: crate::record::Urgency::default(),
            project: project.map(|s| s.to_string()),
            completed_at: None,
        };

        let mut record = Record::new(RecordType::Task, title)
            .with_content(content.to_json());

        if let Some(desc) = description {
            record = record.with_description(desc);
        }

        self.create_record(record).await
    }

    /// List tasks with optional project and status filters
    pub async fn list_tasks(
        &self,
        project: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<Record>> {
        let mut query = String::from(
            "SELECT * FROM record WHERE record_type = 'task' AND deleted_at IS NONE"
        );

        if project.is_some() {
            query.push_str(" AND content.project = $project");
        }
        if status.is_some() {
            query.push_str(" AND content.status = $status");
        }

        query.push_str(" ORDER BY content.priority ASC, created_at DESC");

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("project", project.map(|s| s.to_string())))
            .bind(("status", status.map(|s| s.to_string())))
            .await
            .context("Failed to query tasks")?;

        let tasks: Vec<Record> = response.take(0).context("Failed to parse tasks")?;
        Ok(tasks)
    }

    /// Get tasks that are ready to work on (pending with no blocking dependencies)
    pub async fn ready_tasks(&self, project: Option<&str>) -> Result<Vec<Record>> {
        use crate::record::task_relations;

        // Find tasks that are pending and not blocked by any incomplete task
        let query = if project.is_some() {
            format!(
                r#"
                SELECT * FROM record
                WHERE record_type = 'task'
                AND deleted_at IS NONE
                AND content.status = 'pending'
                AND content.project = $project
                AND id NOT IN (
                    SELECT source FROM record_edge
                    WHERE relation = '{}'
                    AND (SELECT content.status FROM record WHERE id = target)[0] NOT IN ['completed', 'cancelled']
                )
                ORDER BY content.priority ASC, created_at ASC
                "#,
                task_relations::BLOCKED_BY
            )
        } else {
            format!(
                r#"
                SELECT * FROM record
                WHERE record_type = 'task'
                AND deleted_at IS NONE
                AND content.status = 'pending'
                AND id NOT IN (
                    SELECT source FROM record_edge
                    WHERE relation = '{}'
                    AND (SELECT content.status FROM record WHERE id = target)[0] NOT IN ['completed', 'cancelled']
                )
                ORDER BY content.priority ASC, created_at ASC
                "#,
                task_relations::BLOCKED_BY
            )
        };

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("project", project.map(|s| s.to_string())))
            .await
            .context("Failed to query ready tasks")?;

        let tasks: Vec<Record> = response.take(0).context("Failed to parse ready tasks")?;
        Ok(tasks)
    }

    /// Update a task's fields
    ///
    /// For status and priority, pass the new values.
    /// For description and project, use `Some(Some("value"))` to set,
    /// `Some(None)` to clear, and `None` to leave unchanged.
    pub async fn update_task(
        &self,
        id: &str,
        status: Option<&str>,
        priority: Option<i32>,
        title: Option<&str>,
        description: Option<Option<&str>>,
        project: Option<Option<&str>>,
    ) -> Result<Option<Record>> {
        use surrealdb::sql::Datetime;

        // Get existing record
        let existing = self.get_record(id).await?;
        let Some(record) = existing else {
            return Ok(None);
        };

        // Parse current content
        let mut content: serde_json::Value = record.content.clone();

        // Update status
        if let Some(s) = status {
            content["status"] = serde_json::json!(s);
            // Set completed_at when completing
            if s == "completed" || s == "cancelled" {
                content["completed_at"] = serde_json::json!(Datetime::default().to_string());
            }
        }

        // Update priority
        if let Some(p) = priority {
            content["priority"] = serde_json::json!(p);
        }

        // Update project
        if let Some(proj_opt) = project {
            match proj_opt {
                Some(p) => content["project"] = serde_json::json!(p),
                None => {
                    if let Some(obj) = content.as_object_mut() {
                        obj.remove("project");
                    }
                }
            }
        }

        // Update name (title)
        let new_name = title.map(|s| s.to_string());

        // Update description
        let new_desc = match description {
            Some(Some(d)) => Some(d.to_string()),
            Some(None) => None, // Clear
            None => record.description.clone(), // Keep
        };
        let desc_update = match description {
            Some(_) => new_desc.as_deref(),
            None => None, // No change to description
        };

        // Build update
        let mut updates = vec!["updated_at = time::now()".to_string()];
        updates.push("content = $content".to_string());

        if let Some(ref name) = new_name {
            updates.push(format!("name = '{}'", name.replace('\'', "\\'")));
        }

        // Handle description specially
        if let Some(_) = description {
            if let Some(d) = desc_update {
                updates.push(format!("description = '{}'", d.replace('\'', "\\'")));
            } else {
                updates.push("description = NONE".to_string());
            }
        }

        let sql = format!(
            "UPDATE record:{} SET {}",
            id,
            updates.join(", ")
        );

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("content", content))
            .await
            .context("Failed to update task")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Close a task (mark as completed or cancelled)
    pub async fn close_task(
        &self,
        id: &str,
        status: Option<&str>,
        _reason: Option<&str>,
    ) -> Result<Option<Record>> {
        let final_status = status.unwrap_or("completed");

        // Validate status
        let final_status = match final_status {
            "completed" | "cancelled" => final_status,
            _ => "completed",
        };

        self.update_task(id, Some(final_status), None, None, None, None)
            .await
    }

    /// Convenience method to update just the task status
    pub async fn update_task_status(&self, id: &str, status: &str) -> Result<Option<Record>> {
        self.update_task(id, Some(status), None, None, None, None)
            .await
    }

    /// Permanently delete a task and its notes/dependencies
    pub async fn delete_task(&self, id: &str) -> Result<Option<Record>> {
        use crate::record::TASK_NOTE_RELATION;

        // First, delete task notes (records linked via has_note edge)
        let note_query = format!(
            "SELECT target FROM record_edge WHERE source = type::thing('record', $id) AND relation = '{}'",
            TASK_NOTE_RELATION
        );

        let mut response = self
            .db
            .client()
            .query(&note_query)
            .bind(("id", id.to_string()))
            .await
            .context("Failed to find task notes")?;

        #[derive(Deserialize)]
        struct EdgeTarget {
            target: Thing,
        }

        let targets: Vec<EdgeTarget> = response.take(0).unwrap_or_default();
        for target in targets {
            // Delete the note record
            let note_id = target.target.id.to_raw();
            let _: Option<Record> = self
                .db
                .client()
                .delete(("record", note_id.as_str()))
                .await
                .context("Failed to delete task note")?;
        }

        // Delete all edges from/to this task
        self.db
            .client()
            .query("DELETE FROM record_edge WHERE source = type::thing('record', $id) OR target = type::thing('record', $id)")
            .bind(("id", id.to_string()))
            .await
            .context("Failed to delete task edges")?;

        // Hard delete the task record (not soft delete)
        let deleted: Option<Record> = self
            .db
            .client()
            .delete(("record", id))
            .await
            .context("Failed to delete task")?;

        Ok(deleted)
    }

    // ========== Task Note Operations ==========

    /// Add a note to a task
    ///
    /// Task notes are Document records linked to the task via a "has_note" edge.
    pub async fn add_task_note(&self, task_id: &str, content: &str) -> Result<Record> {
        use crate::record::{RecordType, TASK_NOTE_RELATION};

        // Fetch the task to get its name for a meaningful note title
        let task = self.get_record(task_id).await?
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_id))?;

        let note_name = format!("Note on: {}", task.name);

        // Create a document record for the note
        let note = Record::new(RecordType::Document, &note_name)
            .with_content(serde_json::json!({
                "content": content,
                "task_id": task_id
            }));

        let note = self.create_record(note).await?;
        let note_id = note.id_str().expect("note should have id");

        // Create edge from task to note
        self.create_edge(task_id, &note_id, EdgeRelation::RelatedTo, None).await?;

        // Also create the has_note edge for easier querying
        let edge = RecordEdge::new(
            Thing::from(("record", task_id)),
            Thing::from(("record", note_id.as_str())),
            EdgeRelation::RelatedTo, // Using RelatedTo but with custom relation string
        );
        let mut edge_with_relation = edge;
        edge_with_relation.relation = TASK_NOTE_RELATION.to_string();

        let _: Option<RecordEdge> = self
            .db
            .client()
            .create("record_edge")
            .content(edge_with_relation)
            .await
            .context("Failed to create task note edge")?;

        Ok(note)
    }

    /// Add a note to a task with preserved timestamps (for migrations)
    pub async fn add_task_note_with_timestamps(
        &self,
        task_id: &str,
        content: &str,
        created_at: surrealdb::sql::Datetime,
        updated_at: surrealdb::sql::Datetime,
    ) -> Result<Record> {
        use crate::record::{RecordType, TASK_NOTE_RELATION};

        // Fetch the task to get its name for a meaningful note title
        let task = self.get_record(task_id).await?
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_id))?;

        let note_name = format!("Note on: {}", task.name);

        // Create a document record for the note with preserved timestamps
        let note = Record::new(RecordType::Document, &note_name)
            .with_content(serde_json::json!({
                "content": content,
                "task_id": task_id
            }))
            .with_timestamps(created_at.clone(), updated_at);

        let note = self.create_record(note).await?;
        let note_id = note.id_str().expect("note should have id");

        // Create edge from task to note with original timestamp
        let edge = RecordEdge::new(
            Thing::from(("record", task_id)),
            Thing::from(("record", note_id.as_str())),
            EdgeRelation::RelatedTo,
        )
        .with_created_at(created_at);

        let mut edge_with_relation = edge;
        edge_with_relation.relation = TASK_NOTE_RELATION.to_string();

        let _: Option<RecordEdge> = self
            .db
            .client()
            .create("record_edge")
            .content(edge_with_relation)
            .await
            .context("Failed to create task note edge")?;

        Ok(note)
    }

    /// Get notes for a task
    pub async fn get_task_notes(&self, task_id: &str) -> Result<Vec<Record>> {
        use crate::record::TASK_NOTE_RELATION;

        // First, get the target IDs from edges
        let edge_query = format!(
            "SELECT target FROM record_edge WHERE source = type::thing('record', $task_id) AND relation = '{}'",
            TASK_NOTE_RELATION
        );

        let mut response = self
            .db
            .client()
            .query(&edge_query)
            .bind(("task_id", task_id.to_string()))
            .await
            .context("Failed to query task note edges")?;

        #[derive(serde::Deserialize)]
        struct EdgeTarget {
            target: surrealdb::sql::Thing,
        }

        let targets: Vec<EdgeTarget> = response.take(0).unwrap_or_default();

        // Then fetch each note record
        let mut notes = Vec::new();
        for target in targets {
            let note_id = target.target.id.to_raw();
            if let Some(record) = self.get_record(&note_id).await? {
                notes.push(record);
            }
        }

        // Sort by created_at
        notes.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        Ok(notes)
    }

    /// Edit a task note
    pub async fn edit_task_note(&self, note_id: &str, content: &str) -> Result<Option<Record>> {
        let content_json = serde_json::json!({
            "content": content
        });

        // Merge the new content while preserving task_id
        let sql = r#"
            UPDATE type::thing('record', $id) SET
                content = object::merge(content, $new_content),
                updated_at = time::now()
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("id", note_id.to_string()))
            .bind(("new_content", content_json))
            .await
            .context("Failed to update task note")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Delete a task note
    pub async fn delete_task_note(&self, note_id: &str) -> Result<Option<Record>> {
        // Delete the edge first
        self.db
            .client()
            .query("DELETE FROM record_edge WHERE target = type::thing('record', $id)")
            .bind(("id", note_id.to_string()))
            .await
            .context("Failed to delete note edges")?;

        // Then delete the note record
        let deleted: Option<Record> = self
            .db
            .client()
            .delete(("record", note_id))
            .await
            .context("Failed to delete note")?;

        Ok(deleted)
    }

    // ========== Task Dependency Operations ==========

    /// Add a dependency between tasks
    ///
    /// Valid relation types: "blocks", "blocked_by", "relates_to"
    pub async fn add_task_dependency(
        &self,
        from_task_id: &str,
        to_task_id: &str,
        relation: &str,
    ) -> Result<RecordEdge> {
        let edge = RecordEdge::new(
            Thing::from(("record", from_task_id)),
            Thing::from(("record", to_task_id)),
            EdgeRelation::DependsOn, // Use DependsOn as base, but customize relation
        );

        let mut edge_with_relation = edge;
        edge_with_relation.relation = relation.to_string();

        let created: Option<RecordEdge> = self
            .db
            .client()
            .create("record_edge")
            .content(edge_with_relation)
            .await
            .context("Failed to create task dependency")?;

        created.context("Dependency creation returned no result")
    }

    /// Remove a dependency between tasks
    pub async fn remove_task_dependency(
        &self,
        from_task_id: &str,
        to_task_id: &str,
        relation: &str,
    ) -> Result<bool> {
        self.db
            .client()
            .query(
                "DELETE FROM record_edge WHERE \
                 source = type::thing('record', $from) AND \
                 target = type::thing('record', $to) AND \
                 relation = $rel"
            )
            .bind(("from", from_task_id.to_string()))
            .bind(("to", to_task_id.to_string()))
            .bind(("rel", relation.to_string()))
            .await
            .context("Failed to delete task dependency")?;

        Ok(true)
    }

    /// Get dependencies for a task
    pub async fn get_task_dependencies(&self, task_id: &str) -> Result<Vec<RecordEdge>> {
        use crate::record::task_relations;

        let query = format!(
            r#"
            SELECT * FROM record_edge
            WHERE (source = type::thing('record', $task_id) OR target = type::thing('record', $task_id))
            AND relation IN ['{}', '{}', '{}']
            "#,
            task_relations::BLOCKS,
            task_relations::BLOCKED_BY,
            task_relations::RELATES_TO,
        );

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("task_id", task_id.to_string()))
            .await
            .context("Failed to query task dependencies")?;

        let deps: Vec<RecordEdge> = response.take(0).context("Failed to parse dependencies")?;
        Ok(deps)
    }

    // ========== Message Operations ==========

    /// Create a new agent message
    ///
    /// Messages are records with type "message" that facilitate inter-agent communication.
    /// The `from` and `to` fields identify agents (worker IDs, "coordinator", or "primary").
    pub async fn create_message(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
        message_type: crate::record::MessageType,
        thread_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<Record> {
        use crate::record::{MessageContent, RecordType};

        let mut content = MessageContent::new(from, to)
            .with_type(message_type);

        if let Some(tid) = thread_id {
            content = content.with_thread(tid);
        }
        if let Some(taid) = task_id {
            content = content.with_task(taid);
        }

        let record = Record::new(RecordType::Message, subject)
            .with_description(body)
            .with_content(content.to_json());

        self.create_record(record).await
    }

    /// List messages for a specific recipient
    ///
    /// Returns messages addressed to the given agent, optionally filtered by read status.
    pub async fn list_messages_for(
        &self,
        recipient: &str,
        unread_only: bool,
        limit: Option<usize>,
    ) -> Result<Vec<Record>> {
        let mut query = String::from(
            "SELECT * FROM record WHERE record_type = 'message' AND deleted_at IS NONE AND content.to = $to"
        );

        if unread_only {
            query.push_str(" AND content.read = false");
        }

        query.push_str(" ORDER BY created_at DESC");

        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("to", recipient.to_string()))
            .await
            .context("Failed to query messages")?;

        let messages: Vec<Record> = response.take(0).context("Failed to parse messages")?;
        Ok(messages)
    }

    /// List messages sent by a specific sender
    pub async fn list_messages_from(
        &self,
        sender: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Record>> {
        let mut query = String::from(
            "SELECT * FROM record WHERE record_type = 'message' AND deleted_at IS NONE AND content.from = $from ORDER BY created_at DESC"
        );

        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("from", sender.to_string()))
            .await
            .context("Failed to query messages")?;

        let messages: Vec<Record> = response.take(0).context("Failed to parse messages")?;
        Ok(messages)
    }

    /// Mark a message as read
    pub async fn mark_message_read(&self, message_id: &str) -> Result<Option<Record>> {
        use surrealdb::sql::Datetime;

        let sql = r#"
            UPDATE type::thing('record', $id) SET
                content.read = true,
                content.read_at = $read_at,
                updated_at = time::now()
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("id", message_id.to_string()))
            .bind(("read_at", Datetime::default().to_string()))
            .await
            .context("Failed to mark message as read")?;

        let updated: Option<Record> = response.take(0).ok().and_then(|v: Vec<Record>| v.into_iter().next());
        Ok(updated)
    }

    /// Get unread message count for a recipient
    pub async fn unread_message_count(&self, recipient: &str) -> Result<usize> {
        let query = "SELECT count() as count FROM record WHERE record_type = 'message' AND deleted_at IS NONE AND content.to = $to AND content.read = false GROUP ALL";

        let mut response = self
            .db
            .client()
            .query(query)
            .bind(("to", recipient.to_string()))
            .await
            .context("Failed to count unread messages")?;

        #[derive(Deserialize)]
        struct CountResult {
            count: i64,
        }

        let counts: Vec<CountResult> = response.take(0).unwrap_or_default();
        Ok(counts.first().map(|c| c.count as usize).unwrap_or(0))
    }

    /// Get messages in a thread
    pub async fn get_thread_messages(&self, thread_id: &str) -> Result<Vec<Record>> {
        let query = "SELECT * FROM record WHERE record_type = 'message' AND deleted_at IS NONE AND content.thread_id = $thread_id ORDER BY created_at ASC";

        let mut response = self
            .db
            .client()
            .query(query)
            .bind(("thread_id", thread_id.to_string()))
            .await
            .context("Failed to query thread messages")?;

        let messages: Vec<Record> = response.take(0).context("Failed to parse messages")?;
        Ok(messages)
    }

    // ========== Thread/Entry Operations (Transcript Capture) ==========

    /// Create a new thread for transcript capture
    ///
    /// Threads group conversation entries (e.g., Claude session transcripts).
    /// Returns the created thread record.
    pub async fn create_thread(
        &self,
        name: &str,
        source: crate::record::ThreadSource,
        session_id: Option<&str>,
        cwd: Option<&str>,
        task_id: Option<&str>,
        worker_id: Option<&str>,
    ) -> Result<Record> {
        use crate::record::{RecordType, ThreadContent};
        use surrealdb::sql::Datetime;

        let mut content = ThreadContent::new(source);
        if let Some(sid) = session_id {
            content = content.with_session_id(sid);
        }
        if let Some(path) = cwd {
            content = content.with_cwd(path);
        }
        if let Some(tid) = task_id {
            content = content.with_task_id(tid);
        }
        if let Some(wid) = worker_id {
            content = content.with_worker_id(wid);
        }
        content = content.with_started_at(Datetime::default());

        let record = Record::new(RecordType::Thread, name)
            .with_content(content.to_json());

        self.create_record(record).await
    }

    /// Create a new entry within a thread
    ///
    /// Entries are individual turns in a conversation.
    /// Returns the created entry record and links it to the thread.
    pub async fn create_entry(
        &self,
        thread_id: &str,
        role: crate::record::EntryRole,
        turn_number: i32,
        content_text: &str,
        tokens: Option<i32>,
        duration_ms: Option<i64>,
        model: Option<&str>,
        tools_used: Vec<String>,
    ) -> Result<Record> {
        use crate::record::{RecordType, EntryContent};
        use surrealdb::sql::Datetime;

        // Create a brief summary for the entry name (first 50 chars of content)
        let name = if content_text.len() > 50 {
            format!("{}...", &content_text[..47])
        } else {
            content_text.to_string()
        };

        let mut entry_content = EntryContent::new(role, turn_number)
            .with_timestamp(Datetime::default())
            .with_thread_id(thread_id)
            .with_tool_calls(tools_used.len() as i32)
            .with_tools_used(tools_used);

        if let Some(t) = tokens {
            entry_content = entry_content.with_tokens(t);
        }
        if let Some(d) = duration_ms {
            entry_content = entry_content.with_duration_ms(d);
        }
        if let Some(m) = model {
            entry_content = entry_content.with_model(m);
        }

        let record = Record::new(RecordType::Entry, &name)
            .with_description(content_text)
            .with_content(entry_content.to_json());

        let entry = self.create_record(record).await?;
        let entry_id = entry.id_str().expect("entry should have id");

        // Create "contains" edge from thread to entry
        self.create_edge(thread_id, &entry_id, EdgeRelation::Contains, None).await?;

        // Update thread's entry_count
        let sql = r#"
            UPDATE type::thing('record', $thread_id) SET
                content.entry_count = content.entry_count + 1,
                updated_at = time::now()
        "#;
        let _ = self.db.client()
            .query(sql)
            .bind(("thread_id", thread_id.to_string()))
            .await;

        Ok(entry)
    }

    /// Get a thread by ID with its entries
    pub async fn get_thread_with_entries(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> Result<(Option<Record>, Vec<Record>)> {
        // Get the thread record
        let thread = self.get_record(thread_id).await?;

        // Get entries in this thread, ordered by turn number
        let limit_clause = limit.map(|l| format!(" LIMIT {}", l)).unwrap_or_default();
        let query = format!(
            "SELECT * FROM record WHERE record_type = 'entry' AND deleted_at IS NONE AND content.thread_id = $thread_id ORDER BY content.turn_number ASC{}",
            limit_clause
        );

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("thread_id", thread_id.to_string()))
            .await
            .context("Failed to query thread entries")?;

        let entries: Vec<Record> = response.take(0).context("Failed to parse entries")?;
        Ok((thread, entries))
    }

    /// List threads with optional filters
    pub async fn list_threads(
        &self,
        worker_id: Option<&str>,
        task_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Record>> {
        let mut query = String::from(
            "SELECT * FROM record WHERE record_type = 'thread' AND deleted_at IS NONE"
        );

        if worker_id.is_some() {
            query.push_str(" AND content.worker_id = $worker_id");
        }
        if task_id.is_some() {
            query.push_str(" AND content.task_id = $task_id");
        }

        query.push_str(" ORDER BY created_at DESC");

        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }

        let mut response = self
            .db
            .client()
            .query(&query)
            .bind(("worker_id", worker_id.map(|s| s.to_string())))
            .bind(("task_id", task_id.map(|s| s.to_string())))
            .await
            .context("Failed to query threads")?;

        let threads: Vec<Record> = response.take(0).context("Failed to parse threads")?;
        Ok(threads)
    }

    /// Get thread by worker ID (most recent)
    pub async fn get_thread_by_worker(&self, worker_id: &str) -> Result<Option<Record>> {
        let query = "SELECT * FROM record WHERE record_type = 'thread' AND deleted_at IS NONE AND content.worker_id = $worker_id ORDER BY created_at DESC LIMIT 1";

        let mut response = self
            .db
            .client()
            .query(query)
            .bind(("worker_id", worker_id.to_string()))
            .await
            .context("Failed to query thread by worker")?;

        let threads: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(threads.into_iter().next())
    }

    /// Update thread to mark it as ended
    pub async fn end_thread(&self, thread_id: &str) -> Result<Option<Record>> {
        use surrealdb::sql::Datetime;

        let sql = r#"
            UPDATE type::thing('record', $id) SET
                content.ended_at = $ended_at,
                updated_at = time::now()
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("id", thread_id.to_string()))
            .bind(("ended_at", Datetime::default().to_string()))
            .await
            .context("Failed to end thread")?;

        let records: Vec<Record> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().next())
    }

    /// Update thread with token count
    pub async fn update_thread_tokens(&self, thread_id: &str, tokens: i64) -> Result<()> {
        let sql = r#"
            UPDATE type::thing('record', $id) SET
                content.total_tokens = (content.total_tokens ?? 0) + $tokens,
                updated_at = time::now()
        "#;

        let _ = self.db.client()
            .query(sql)
            .bind(("id", thread_id.to_string()))
            .bind(("tokens", tokens))
            .await
            .context("Failed to update thread tokens")?;

        Ok(())
    }

    // ========== Purge Operations ==========
    //
    // These operations support complete data purge for sensitive information.
    // Unlike normal deletes, purge removes ALL traces including:
    // - The source record (memo, task, etc.)
    // - All events with source_record matching the purged record
    // - All facts derived from the source (via source_episodes)
    // - All fact_entity links for those facts
    // - Orphaned entities (entities with no remaining source_episodes)
    // - All record edges involving the purged record

    /// Delete an event by ID (hard delete)
    ///
    /// This breaks the immutability of events, but is necessary for data purge
    /// when sensitive information was accidentally recorded.
    pub async fn delete_event(&self, id: &str) -> Result<Option<crate::event::Event>> {
        let deleted: Option<crate::event::Event> = self
            .db
            .client()
            .delete(("event", id))
            .await
            .context("Failed to delete event")?;

        Ok(deleted)
    }

    /// Delete all events with a specific source_record
    ///
    /// Returns the number of events deleted.
    pub async fn delete_events_by_source(&self, source_record: &str) -> Result<usize> {
        // First count how many will be deleted
        let count_sql = "SELECT count() FROM event WHERE source_record = $source GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(count_sql)
            .bind(("source", source_record.to_string()))
            .await
            .context("Failed to count events")?;

        let count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let count = count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Delete the events
        self.db
            .client()
            .query("DELETE FROM event WHERE source_record = $source")
            .bind(("source", source_record.to_string()))
            .await
            .context("Failed to delete events by source")?;

        Ok(count)
    }

    /// Delete a fact by ID (hard delete)
    pub async fn delete_fact(&self, id: &str) -> Result<Option<crate::fact::Fact>> {
        // First delete fact_entity links
        self.db
            .client()
            .query("DELETE FROM fact_entity WHERE fact = type::thing('fact', $id)")
            .bind(("id", id.to_string()))
            .await
            .context("Failed to delete fact_entity links")?;

        // Then delete the fact
        let deleted: Option<crate::fact::Fact> = self
            .db
            .client()
            .delete(("fact", id))
            .await
            .context("Failed to delete fact")?;

        Ok(deleted)
    }

    /// Delete all facts with a specific source episode
    ///
    /// Returns the number of facts deleted.
    pub async fn delete_facts_by_source_episode(
        &self,
        episode_type: &str,
        episode_id: &str,
    ) -> Result<usize> {
        // Find facts with this source episode
        let sql = r#"
            SELECT * FROM fact WHERE source_episodes CONTAINS {
                episode_type: $episode_type,
                episode_id: $episode_id
            }
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("episode_type", episode_type.to_string()))
            .bind(("episode_id", episode_id.to_string()))
            .await
            .context("Failed to find facts by source episode")?;

        let facts: Vec<crate::fact::Fact> = response.take(0).unwrap_or_default();
        let count = facts.len();

        // Delete each fact (including their fact_entity links)
        for fact in facts {
            if let Some(id) = fact.id.as_ref().map(|t| t.id.to_raw()) {
                self.delete_fact(&id).await?;
            }
        }

        Ok(count)
    }

    /// Delete an entity by ID (hard delete)
    pub async fn delete_entity(&self, id: &str) -> Result<Option<crate::fact::Entity>> {
        // First delete fact_entity links
        self.db
            .client()
            .query("DELETE FROM fact_entity WHERE entity = type::thing('entity', $id)")
            .bind(("id", id.to_string()))
            .await
            .context("Failed to delete fact_entity links for entity")?;

        // Then delete the entity
        let deleted: Option<crate::fact::Entity> = self
            .db
            .client()
            .delete(("entity", id))
            .await
            .context("Failed to delete entity")?;

        Ok(deleted)
    }

    /// Delete entities that have no remaining source episodes after purge
    ///
    /// Returns the number of entities deleted.
    pub async fn delete_orphaned_entities(&self, purged_episode_type: &str, purged_episode_id: &str) -> Result<usize> {
        // Find entities that ONLY have the purged episode as their source
        // This is conservative - we only delete if they would have NO sources left
        let sql = r#"
            SELECT * FROM entity WHERE array::len(source_episodes) = 1
            AND source_episodes[0].episode_type = $episode_type
            AND source_episodes[0].episode_id = $episode_id
        "#;

        let mut response = self
            .db
            .client()
            .query(sql)
            .bind(("episode_type", purged_episode_type.to_string()))
            .bind(("episode_id", purged_episode_id.to_string()))
            .await
            .context("Failed to find orphaned entities")?;

        let entities: Vec<crate::fact::Entity> = response.take(0).unwrap_or_default();
        let count = entities.len();

        // Delete each orphaned entity
        for entity in entities {
            if let Some(id) = entity.id.as_ref().map(|t| t.id.to_raw()) {
                self.delete_entity(&id).await?;
            }
        }

        // Also remove the source episode from entities that have multiple sources
        let update_sql = r#"
            UPDATE entity SET
                source_episodes = array::filter(source_episodes, |$ep|
                    $ep.episode_type != $episode_type OR $ep.episode_id != $episode_id
                ),
                updated_at = time::now()
            WHERE array::len(source_episodes) > 1
            AND source_episodes CONTAINS {
                episode_type: $episode_type,
                episode_id: $episode_id
            }
        "#;

        let _ = self
            .db
            .client()
            .query(update_sql)
            .bind(("episode_type", purged_episode_type.to_string()))
            .bind(("episode_id", purged_episode_id.to_string()))
            .await;

        Ok(count)
    }

    /// Hard delete a record and all its edges
    pub async fn hard_delete_record(&self, id: &str) -> Result<Option<crate::record::Record>> {
        // Delete all edges from/to this record
        self.db
            .client()
            .query("DELETE FROM record_edge WHERE source = type::thing('record', $id) OR target = type::thing('record', $id)")
            .bind(("id", id.to_string()))
            .await
            .context("Failed to delete record edges")?;

        // Hard delete the record
        let deleted: Option<crate::record::Record> = self
            .db
            .client()
            .delete(("record", id))
            .await
            .context("Failed to hard delete record")?;

        Ok(deleted)
    }

    /// Purge a memo and all derived data
    ///
    /// This completely removes:
    /// - The memo itself
    /// - All events with source_record = "memo:{id}"
    /// - All facts with source_episodes containing this memo
    /// - All fact_entity links for those facts
    /// - Orphaned entities (only sourced from this memo)
    ///
    /// Returns a summary of what was deleted.
    pub async fn purge_memo(&self, memo_id: &str) -> Result<PurgeResult> {
        let source_record = format!("memo:{}", memo_id);

        // 1. Delete events with this source_record
        let events_deleted = self.delete_events_by_source(&source_record).await?;

        // 2. Delete facts with this memo as source episode
        let facts_deleted = self.delete_facts_by_source_episode("memo", memo_id).await?;

        // 3. Delete orphaned entities
        let entities_deleted = self.delete_orphaned_entities("memo", memo_id).await?;

        // 4. Delete the memo itself
        let memo = self.delete_memo(memo_id).await?;

        Ok(PurgeResult {
            source_type: "memo".to_string(),
            source_id: memo_id.to_string(),
            source_deleted: memo.is_some(),
            events_deleted,
            facts_deleted,
            entities_deleted,
            records_deleted: 0,
            edges_deleted: 0,
        })
    }

    /// Purge a record and all related data (for records that generated events/facts)
    ///
    /// This is used for records like tasks that may have events and derived facts.
    pub async fn purge_record(&self, record_id: &str) -> Result<PurgeResult> {
        let source_record = format!("record:{}", record_id);

        // 1. Delete events with this source_record
        let events_deleted = self.delete_events_by_source(&source_record).await?;

        // 2. Delete facts with this record as source episode
        let facts_deleted = self.delete_facts_by_source_episode("record", record_id).await?;

        // 3. Delete orphaned entities
        let entities_deleted = self.delete_orphaned_entities("record", record_id).await?;

        // 4. Count edges before deletion
        let edge_count_sql = "SELECT count() FROM record_edge WHERE source = type::thing('record', $id) OR target = type::thing('record', $id) GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(edge_count_sql)
            .bind(("id", record_id.to_string()))
            .await
            .context("Failed to count edges")?;
        let edge_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let edges_deleted = edge_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // 5. Hard delete the record and edges
        let record = self.hard_delete_record(record_id).await?;

        Ok(PurgeResult {
            source_type: "record".to_string(),
            source_id: record_id.to_string(),
            source_deleted: record.is_some(),
            events_deleted,
            facts_deleted,
            entities_deleted,
            records_deleted: if record.is_some() { 1 } else { 0 },
            edges_deleted,
        })
    }

    /// Preview what would be purged without actually deleting
    ///
    /// Returns a PurgeResult with counts of what would be affected.
    pub async fn preview_purge_memo(&self, memo_id: &str) -> Result<PurgeResult> {
        let source_record = format!("memo:{}", memo_id);

        // Count events
        let event_sql = "SELECT count() FROM event WHERE source_record = $source GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(event_sql)
            .bind(("source", source_record.clone()))
            .await?;
        let event_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let events_deleted = event_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Count facts
        let fact_sql = r#"
            SELECT count() FROM fact WHERE source_episodes CONTAINS {
                episode_type: 'memo',
                episode_id: $id
            } GROUP ALL
        "#;
        let mut response = self
            .db
            .client()
            .query(fact_sql)
            .bind(("id", memo_id.to_string()))
            .await?;
        let fact_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let facts_deleted = fact_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Count orphaned entities
        let entity_sql = r#"
            SELECT count() FROM entity WHERE array::len(source_episodes) = 1
            AND source_episodes[0].episode_type = 'memo'
            AND source_episodes[0].episode_id = $id GROUP ALL
        "#;
        let mut response = self
            .db
            .client()
            .query(entity_sql)
            .bind(("id", memo_id.to_string()))
            .await?;
        let entity_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let entities_deleted = entity_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Check if memo exists
        let memo = self.get_memo(memo_id).await?;

        Ok(PurgeResult {
            source_type: "memo".to_string(),
            source_id: memo_id.to_string(),
            source_deleted: memo.is_some(),
            events_deleted,
            facts_deleted,
            entities_deleted,
            records_deleted: 0,
            edges_deleted: 0,
        })
    }

    // ========== Record Merge Operations ==========

    /// Merge two records, combining edges and marking the source as superseded
    ///
    /// This is used for deduplication. When duplicates are discovered:
    /// 1. All edges FROM the source record are redirected to point FROM the target
    /// 2. All edges TO the source record are redirected to point TO the target
    /// 3. Content/description can be optionally merged
    /// 4. The source record is marked as superseded_by the target
    /// 5. A record.merged event is emitted for provenance
    ///
    /// Returns a MergeResult summarizing what was changed.
    pub async fn merge_records(
        &self,
        source_id: &str,
        target_id: &str,
        merge_content: bool,
        merge_description: bool,
    ) -> Result<MergeResult> {
        // 1. Get both records
        let source = self.get_record(source_id).await?
            .ok_or_else(|| anyhow::anyhow!("Source record not found: {}", source_id))?;
        let target = self.get_record(target_id).await?
            .ok_or_else(|| anyhow::anyhow!("Target record not found: {}", target_id))?;

        // Validate: records should be same type
        if source.record_type != target.record_type {
            anyhow::bail!(
                "Cannot merge records of different types: {} vs {}",
                source.record_type,
                target.record_type
            );
        }

        // 2. Get all edges from/to the source record
        let edges_from = self.get_edges_from(source_id, None, true).await?;
        let edges_to = self.get_edges_to(source_id, None, true).await?;

        let mut edges_redirected = 0;
        let mut edges_skipped = 0;

        // 3. Redirect edges FROM the source to be FROM the target
        for edge in &edges_from {
            let edge_id = edge.id_str().unwrap_or_default();
            let target_record_id = edge.target.id.to_raw();
            let relation: EdgeRelation = edge.relation.parse()
                .map_err(|e: String| anyhow::anyhow!(e))?;

            // Skip if this would create a self-loop
            if target_record_id == target_id {
                edges_skipped += 1;
                continue;
            }

            // Check if target already has this edge (to avoid duplicates)
            let existing = self.get_edges_from(target_id, Some(relation), true).await?;
            let already_exists = existing.iter().any(|e| e.target.id.to_raw() == target_record_id);

            if already_exists {
                // Supersede the old edge without creating a new one
                self.supersede_edge_without_replacement(&edge_id).await?;
                edges_skipped += 1;
            } else {
                // Create new edge from target and supersede the old one
                let new_edge = self.create_edge(
                    target_id,
                    &target_record_id,
                    relation,
                    edge.metadata.clone(),
                ).await?;

                // Mark old edge as superseded by the new one
                let now = surrealdb::sql::Datetime::default();
                let sql = r#"
                    UPDATE type::thing('record_edge', $old_id) SET
                        valid_until = $now,
                        superseded_by = $new_id,
                        superseded_via = $via_event
                "#;
                self.db
                    .client()
                    .query(sql)
                    .bind(("old_id", edge_id.clone()))
                    .bind(("now", now))
                    .bind(("new_id", new_edge.id.clone()))
                    .bind(("via_event", Some(format!("merge:{}→{}", source_id, target_id))))
                    .await
                    .context("Failed to supersede edge")?;

                edges_redirected += 1;
            }
        }

        // 4. Redirect edges TO the source to point TO the target
        for edge in &edges_to {
            let edge_id = edge.id_str().unwrap_or_default();
            let source_record_id = edge.source.id.to_raw();
            let relation: EdgeRelation = edge.relation.parse()
                .map_err(|e: String| anyhow::anyhow!(e))?;

            // Skip if this would create a self-loop
            if source_record_id == target_id {
                edges_skipped += 1;
                continue;
            }

            // Check if target already has this incoming edge
            let existing = self.get_edges_to(target_id, Some(relation), true).await?;
            let already_exists = existing.iter().any(|e| e.source.id.to_raw() == source_record_id);

            if already_exists {
                self.supersede_edge_without_replacement(&edge_id).await?;
                edges_skipped += 1;
            } else {
                // Create new edge to target and supersede the old one
                let new_edge = self.create_edge(
                    &source_record_id,
                    target_id,
                    relation,
                    edge.metadata.clone(),
                ).await?;

                let now = surrealdb::sql::Datetime::default();
                let sql = r#"
                    UPDATE type::thing('record_edge', $old_id) SET
                        valid_until = $now,
                        superseded_by = $new_id,
                        superseded_via = $via_event
                "#;
                self.db
                    .client()
                    .query(sql)
                    .bind(("old_id", edge_id.clone()))
                    .bind(("now", now))
                    .bind(("new_id", new_edge.id.clone()))
                    .bind(("via_event", Some(format!("merge:{}→{}", source_id, target_id))))
                    .await
                    .context("Failed to supersede edge")?;

                edges_redirected += 1;
            }
        }

        // 5. Always merge aliases (source's name becomes an alias of target)
        // This preserves identity resolution: if someone was found by source's name,
        // they should still be findable after the merge.
        let mut merged_aliases: Vec<String> = target.aliases.clone();

        // Add source's name as an alias (if not already present)
        let source_name_lower = source.name.to_lowercase();
        if !merged_aliases.iter().any(|a| a.to_lowercase() == source_name_lower)
            && target.name.to_lowercase() != source_name_lower
        {
            merged_aliases.push(source.name.clone());
        }

        // Merge source's aliases into target's aliases
        for alias in &source.aliases {
            let alias_lower = alias.to_lowercase();
            if !merged_aliases.iter().any(|a| a.to_lowercase() == alias_lower)
                && target.name.to_lowercase() != alias_lower
            {
                merged_aliases.push(alias.clone());
            }
        }

        let aliases_merged = merged_aliases.len() > target.aliases.len();

        // 6. Optionally merge content and description
        let mut content_merged = false;
        let mut description_merged = false;
        let mut new_content = target.content.clone();
        let mut new_description = target.description.clone();

        if merge_content {
            // Deep merge: source values fill in where target has null/missing
            if let (Some(target_obj), Some(source_obj)) =
                (new_content.as_object_mut(), source.content.as_object())
            {
                for (key, value) in source_obj {
                    if !target_obj.contains_key(key) || target_obj.get(key) == Some(&serde_json::Value::Null) {
                        target_obj.insert(key.clone(), value.clone());
                        content_merged = true;
                    }
                }
            }
        }

        if merge_description {
            // Append source description if target doesn't have one
            if target.description.is_none() && source.description.is_some() {
                new_description = source.description.clone();
                description_merged = true;
            } else if let (Some(ref target_desc), Some(ref source_desc)) = (&target.description, &source.description) {
                // Both have descriptions - append source with separator if different
                if target_desc != source_desc && !target_desc.contains(source_desc) {
                    new_description = Some(format!("{}\n\n---\n\n{}", target_desc, source_desc));
                    description_merged = true;
                }
            }
        }

        // Update target record if anything changed (aliases always need updating on merge)
        if aliases_merged || content_merged || description_merged {
            self.update_record_with_aliases(
                target_id,
                None,
                new_description.as_deref(),
                if content_merged { Some(new_content) } else { None },
                None, // Don't change record_type during merge
                Some(merged_aliases),
            ).await?;
        }

        // 6. Mark source as superseded by target
        let now = surrealdb::sql::Datetime::default();
        let via_event = format!("merge:{}→{}", source_id, target_id);
        let sql = r#"
            UPDATE type::thing('record', $id) SET
                superseded_by = type::thing('record', $target_id),
                superseded_via = $via_event,
                deleted_at = $now,
                updated_at = $now
        "#;
        self.db
            .client()
            .query(sql)
            .bind(("id", source_id.to_string()))
            .bind(("target_id", target_id.to_string()))
            .bind(("via_event", via_event))
            .bind(("now", now))
            .await
            .context("Failed to mark source as superseded")?;

        Ok(MergeResult {
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            source_type: source.record_type.clone(),
            edges_redirected,
            edges_skipped,
            aliases_merged,
            content_merged,
            description_merged,
        })
    }

    /// Supersede an edge without creating a replacement
    /// Used when an equivalent edge already exists on the target
    async fn supersede_edge_without_replacement(&self, edge_id: &str) -> Result<()> {
        let now = surrealdb::sql::Datetime::default();
        let sql = r#"
            UPDATE type::thing('record_edge', $edge_id) SET
                valid_until = $now,
                superseded_via = 'merge:duplicate'
        "#;
        self.db
            .client()
            .query(sql)
            .bind(("edge_id", edge_id.to_string()))
            .bind(("now", now))
            .await
            .context("Failed to supersede duplicate edge")?;

        Ok(())
    }

    /// Preview what a merge would do without executing it
    pub async fn preview_merge_records(
        &self,
        source_id: &str,
        target_id: &str,
    ) -> Result<MergePreview> {
        // Get both records
        let source = self.get_record(source_id).await?
            .ok_or_else(|| anyhow::anyhow!("Source record not found: {}", source_id))?;
        let target = self.get_record(target_id).await?
            .ok_or_else(|| anyhow::anyhow!("Target record not found: {}", target_id))?;

        // Validate types match
        if source.record_type != target.record_type {
            anyhow::bail!(
                "Cannot merge records of different types: {} vs {}",
                source.record_type,
                target.record_type
            );
        }

        // Count edges
        let edges_from = self.get_edges_from(source_id, None, true).await?;
        let edges_to = self.get_edges_to(source_id, None, true).await?;

        let mut would_redirect = 0;
        let mut would_skip = 0;

        // Check edges from source
        for edge in &edges_from {
            let target_record_id = edge.target.id.to_raw();
            let relation: EdgeRelation = edge.relation.parse()
                .map_err(|e: String| anyhow::anyhow!(e))?;

            if target_record_id == target_id {
                would_skip += 1;
                continue;
            }

            let existing = self.get_edges_from(target_id, Some(relation), true).await?;
            if existing.iter().any(|e| e.target.id.to_raw() == target_record_id) {
                would_skip += 1;
            } else {
                would_redirect += 1;
            }
        }

        // Check edges to source
        for edge in &edges_to {
            let source_record_id = edge.source.id.to_raw();
            let relation: EdgeRelation = edge.relation.parse()
                .map_err(|e: String| anyhow::anyhow!(e))?;

            if source_record_id == target_id {
                would_skip += 1;
                continue;
            }

            let existing = self.get_edges_to(target_id, Some(relation), true).await?;
            if existing.iter().any(|e| e.source.id.to_raw() == source_record_id) {
                would_skip += 1;
            } else {
                would_redirect += 1;
            }
        }

        // Check what content would merge
        let mut content_fields_to_merge = Vec::new();
        if let (Some(target_obj), Some(source_obj)) =
            (target.content.as_object(), source.content.as_object())
        {
            for (key, _) in source_obj {
                if !target_obj.contains_key(key) || target_obj.get(key) == Some(&serde_json::Value::Null) {
                    content_fields_to_merge.push(key.clone());
                }
            }
        }

        let would_merge_description = source.description.is_some() &&
            (target.description.is_none() ||
             (target.description.as_ref() != source.description.as_ref() &&
              !target.description.as_ref().map(|d| d.contains(source.description.as_ref().unwrap_or(&String::new()))).unwrap_or(false)));

        Ok(MergePreview {
            source_id: source_id.to_string(),
            source_name: source.name,
            target_id: target_id.to_string(),
            target_name: target.name,
            record_type: source.record_type,
            edges_would_redirect: would_redirect,
            edges_would_skip: would_skip,
            content_fields_to_merge,
            would_merge_description,
        })
    }

    /// Preview what would be purged for a record
    pub async fn preview_purge_record(&self, record_id: &str) -> Result<PurgeResult> {
        let source_record = format!("record:{}", record_id);

        // Count events
        let event_sql = "SELECT count() FROM event WHERE source_record = $source GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(event_sql)
            .bind(("source", source_record.clone()))
            .await?;
        let event_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let events_deleted = event_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Count facts
        let fact_sql = r#"
            SELECT count() FROM fact WHERE source_episodes CONTAINS {
                episode_type: 'record',
                episode_id: $id
            } GROUP ALL
        "#;
        let mut response = self
            .db
            .client()
            .query(fact_sql)
            .bind(("id", record_id.to_string()))
            .await?;
        let fact_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let facts_deleted = fact_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Count orphaned entities
        let entity_sql = r#"
            SELECT count() FROM entity WHERE array::len(source_episodes) = 1
            AND source_episodes[0].episode_type = 'record'
            AND source_episodes[0].episode_id = $id GROUP ALL
        "#;
        let mut response = self
            .db
            .client()
            .query(entity_sql)
            .bind(("id", record_id.to_string()))
            .await?;
        let entity_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let entities_deleted = entity_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Count edges
        let edge_sql = "SELECT count() FROM record_edge WHERE source = type::thing('record', $id) OR target = type::thing('record', $id) GROUP ALL";
        let mut response = self
            .db
            .client()
            .query(edge_sql)
            .bind(("id", record_id.to_string()))
            .await?;
        let edge_count: Option<serde_json::Value> = response.take(0).ok().flatten();
        let edges_deleted = edge_count
            .and_then(|v| v.get("count").and_then(|c| c.as_u64()))
            .unwrap_or(0) as usize;

        // Check if record exists
        let record = self.get_record(record_id).await?;

        Ok(PurgeResult {
            source_type: "record".to_string(),
            source_id: record_id.to_string(),
            source_deleted: record.is_some(),
            events_deleted,
            facts_deleted,
            entities_deleted,
            records_deleted: if record.is_some() { 1 } else { 0 },
            edges_deleted,
        })
    }
}

/// Result of a purge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeResult {
    /// Type of the source being purged (memo, record, task)
    pub source_type: String,
    /// ID of the source being purged
    pub source_id: String,
    /// Whether the source record was found and deleted
    pub source_deleted: bool,
    /// Number of events deleted
    pub events_deleted: usize,
    /// Number of facts deleted
    pub facts_deleted: usize,
    /// Number of entities deleted (orphaned only)
    pub entities_deleted: usize,
    /// Number of records deleted
    pub records_deleted: usize,
    /// Number of edges deleted
    pub edges_deleted: usize,
}

impl PurgeResult {
    /// Returns true if anything was actually deleted
    pub fn any_deleted(&self) -> bool {
        self.source_deleted
            || self.events_deleted > 0
            || self.facts_deleted > 0
            || self.entities_deleted > 0
            || self.records_deleted > 0
            || self.edges_deleted > 0
    }

    /// Total count of items deleted
    pub fn total_deleted(&self) -> usize {
        let source = if self.source_deleted { 1 } else { 0 };
        source
            + self.events_deleted
            + self.facts_deleted
            + self.entities_deleted
            + self.records_deleted
            + self.edges_deleted
    }
}

/// Result of a record merge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    /// ID of the source record (the duplicate being merged away)
    pub source_id: String,
    /// ID of the target record (the survivor)
    pub target_id: String,
    /// Type of records merged
    pub source_type: String,
    /// Number of edges redirected to the target
    pub edges_redirected: usize,
    /// Number of edges skipped (duplicates or self-loops)
    pub edges_skipped: usize,
    /// Whether aliases were merged from source to target
    pub aliases_merged: bool,
    /// Whether content was merged from source to target
    pub content_merged: bool,
    /// Whether description was merged from source to target
    pub description_merged: bool,
}

impl MergeResult {
    /// Summary message of the merge
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("Merged {} → {}", self.source_id, self.target_id),
            format!("  Type: {}", self.source_type),
            format!("  Edges redirected: {}", self.edges_redirected),
            format!("  Edges skipped: {}", self.edges_skipped),
        ];
        if self.aliases_merged {
            parts.push("  Aliases: merged".to_string());
        }
        if self.content_merged {
            parts.push("  Content: merged".to_string());
        }
        if self.description_merged {
            parts.push("  Description: merged".to_string());
        }
        parts.join("\n")
    }
}

/// Preview of what a merge would do
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePreview {
    /// ID of the source record
    pub source_id: String,
    /// Name of the source record
    pub source_name: String,
    /// ID of the target record
    pub target_id: String,
    /// Name of the target record
    pub target_name: String,
    /// Type of records being merged
    pub record_type: String,
    /// Number of edges that would be redirected
    pub edges_would_redirect: usize,
    /// Number of edges that would be skipped
    pub edges_would_skip: usize,
    /// Content fields that would be copied from source
    pub content_fields_to_merge: Vec<String>,
    /// Whether description would be merged
    pub would_merge_description: bool,
}

impl MergePreview {
    /// Summary message of what merge would do
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("Merge preview: {} → {}", self.source_name, self.target_name),
            format!("  Source: {} ({})", self.source_id, self.source_name),
            format!("  Target: {} ({})", self.target_id, self.target_name),
            format!("  Type: {}", self.record_type),
            format!("  Edges to redirect: {}", self.edges_would_redirect),
            format!("  Edges to skip: {}", self.edges_would_skip),
        ];
        if !self.content_fields_to_merge.is_empty() {
            parts.push(format!("  Content fields to merge: {}", self.content_fields_to_merge.join(", ")));
        }
        if self.would_merge_description {
            parts.push("  Would merge description".to_string());
        }
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{EdgeRelation, Record, RecordType};
    use db::DatabaseConfig;
    use serde_json::json;
    use std::path::PathBuf;

    /// Integration test for record CRUD and graph traversal
    /// Run with: cargo test -p atlas record_integration -- --nocapture --ignored
    #[tokio::test]
    #[ignore] // Requires running database
    async fn record_integration() -> Result<()> {
        // Connect to local dev database
        let config = DatabaseConfig::embedded(PathBuf::from("./.memex/db"));
        let db = db::Database::connect(&config, "atlas", None).await?;
        let store = Store::new(db);

        // Clean up any existing test records (by looking them up first)
        async fn cleanup(store: &Store, record_type: &str, name: &str) {
            if let Ok(Some(r)) = store.get_record_by_type_name(record_type, name).await {
                if let Some(id) = r.id_str() {
                    let _ = store.delete_record(&id).await;
                }
            }
        }
        cleanup(&store, "rule", "test-rust-style").await;
        cleanup(&store, "repo", "test-memex").await;
        cleanup(&store, "team", "test-core-team").await;

        // 1. Create a rule record
        let rule = Record::new(RecordType::Rule, "test-rust-style")
            .with_description("Rust coding standards")
            .with_content(json!({
                "content": "Use snake_case for functions",
                "severity": "warning",
                "auto_apply": true
            }));
        let rule = store.create_record(rule).await?;
        let rule_id = rule.id_str().expect("rule should have id");
        println!("Created rule: {}", rule_id);

        // 2. Create a repo record
        let repo = Record::new(RecordType::Repo, "test-memex")
            .with_description("Knowledge management system")
            .with_content(json!({
                "path": "/Users/luke/workspace/lexun/memex",
                "default_branch": "main",
                "languages": ["rust"]
            }));
        let repo = store.create_record(repo).await?;
        let repo_id = repo.id_str().expect("repo should have id");
        println!("Created repo: {}", repo_id);

        // 3. Create a team record
        let team = Record::new(RecordType::Team, "test-core-team")
            .with_description("Core development team");
        let team = store.create_record(team).await?;
        let team_id = team.id_str().expect("team should have id");
        println!("Created team: {}", team_id);

        // 4. Create edges: rule applies_to repo, repo belongs_to team
        store.create_edge(&rule_id, &repo_id, EdgeRelation::AppliesTo, None).await?;
        println!("Created edge: rule --applies_to--> repo");

        store.create_edge(&repo_id, &team_id, EdgeRelation::BelongsTo, None).await?;
        println!("Created edge: repo --belongs_to--> team");

        // 5. Test context assembly from repo
        let context = store.assemble_context(&repo_id, 3).await?;
        println!("\nContext assembly from repo (depth 3):");
        println!("  Records found: {}", context.records.len());
        println!("  Rules: {}", context.rules().len());
        for record in &context.records {
            println!("    - {} ({}): {}", record.name, record.record_type, record.description.as_deref().unwrap_or(""));
        }

        // Verify we found the rule via applies_to traversal
        assert!(!context.rules().is_empty(), "Should find rules that apply_to the repo");

        // 6. Clean up
        store.delete_record(&rule_id).await?;
        store.delete_record(&repo_id).await?;
        store.delete_record(&team_id).await?;
        println!("\nCleaned up test records");

        Ok(())
    }

    /// Test that task notes get meaningful names based on task title
    /// Run with: cargo test -p atlas task_note_naming -- --nocapture --ignored
    #[tokio::test]
    #[ignore] // Requires running database
    async fn task_note_naming() -> Result<()> {
        // Connect to local dev database
        let config = DatabaseConfig::embedded(PathBuf::from("./.memex/db"));
        let db = db::Database::connect(&config, "atlas", None).await?;
        let store = Store::new(db);

        // Create a test task
        let task = Record::new(RecordType::Task, "Fix authentication bug")
            .with_description("Users cannot log in with OAuth")
            .with_content(json!({
                "status": "in_progress",
                "priority": 1
            }));
        let task = store.create_record(task).await?;
        let task_id = task.id_str().expect("task should have id");
        println!("Created task: {} - {}", task_id, task.name);

        // Add a note to the task
        let note = store.add_task_note(&task_id, "Found the issue in oauth.rs:42").await?;
        let note_id = note.id_str().expect("note should have id");
        println!("Created note: {} - {}", note_id, note.name);

        // Verify the note has a meaningful name
        assert_eq!(note.name, "Note on: Fix authentication bug",
            "Note should have a meaningful name based on task title");
        assert_eq!(note.record_type, "document",
            "Note should be a Document record");

        // Verify note content is stored correctly
        assert_eq!(note.content["content"], "Found the issue in oauth.rs:42");
        assert_eq!(note.content["task_id"], task_id);

        // Clean up
        store.delete_task(&task_id).await?;
        println!("Cleaned up test task and note");

        Ok(())
    }

    #[test]
    fn test_purge_result_any_deleted() {
        let empty = PurgeResult {
            source_type: "memo".to_string(),
            source_id: "test".to_string(),
            source_deleted: false,
            events_deleted: 0,
            facts_deleted: 0,
            entities_deleted: 0,
            records_deleted: 0,
            edges_deleted: 0,
        };
        assert!(!empty.any_deleted());

        let with_source = PurgeResult {
            source_deleted: true,
            ..empty.clone()
        };
        assert!(with_source.any_deleted());

        let with_events = PurgeResult {
            events_deleted: 5,
            ..empty.clone()
        };
        assert!(with_events.any_deleted());

        let with_facts = PurgeResult {
            facts_deleted: 3,
            ..empty.clone()
        };
        assert!(with_facts.any_deleted());
    }

    #[test]
    fn test_purge_result_total_deleted() {
        let result = PurgeResult {
            source_type: "memo".to_string(),
            source_id: "test".to_string(),
            source_deleted: true,
            events_deleted: 2,
            facts_deleted: 3,
            entities_deleted: 1,
            records_deleted: 0,
            edges_deleted: 0,
        };
        // 1 (source) + 2 (events) + 3 (facts) + 1 (entities) = 7
        assert_eq!(result.total_deleted(), 7);

        let result_no_source = PurgeResult {
            source_deleted: false,
            ..result.clone()
        };
        // 0 (source) + 2 (events) + 3 (facts) + 1 (entities) = 6
        assert_eq!(result_no_source.total_deleted(), 6);
    }

    #[test]
    fn test_normalize_identity_query_basic() {
        // Basic lowercase normalization
        assert_eq!(normalize_identity_query("Luke"), "luke");
        assert_eq!(normalize_identity_query("ALICE"), "alice");
        assert_eq!(normalize_identity_query("Bob Smith"), "bob smith");
    }

    #[test]
    fn test_normalize_identity_query_strip_prefix() {
        // Strip @ prefix (common for social handles)
        assert_eq!(normalize_identity_query("@luke"), "luke");
        assert_eq!(normalize_identity_query("@LexUN"), "lexun");

        // Strip # prefix (common for channels/tags)
        assert_eq!(normalize_identity_query("#channel"), "channel");
        assert_eq!(normalize_identity_query("#TeamName"), "teamname");
    }

    #[test]
    fn test_normalize_identity_query_trim_whitespace() {
        assert_eq!(normalize_identity_query("  luke  "), "luke");
        assert_eq!(normalize_identity_query("  @alice  "), "alice");
    }

    #[test]
    fn test_normalize_identity_query_preserves_internal_chars() {
        // Only strips leading @ or #, preserves internal ones
        assert_eq!(normalize_identity_query("foo@bar.com"), "foo@bar.com");
        assert_eq!(normalize_identity_query("@user@domain"), "user@domain");
    }

    /// Comprehensive integration test for context assembly graph traversal.
    ///
    /// Tests the full graph structure expected for autonomous agent orchestration:
    ///   - Repo as entry point
    ///   - Rules that apply_to the repo
    ///   - Skills that are available_to the repo
    ///   - Team that the repo belongs_to
    ///   - People who are member_of the team
    ///   - MCP servers for the repo
    ///
    /// Run with: cargo test -p atlas context_assembly_full_graph -- --nocapture --ignored
    #[tokio::test]
    #[ignore] // Requires running database
    async fn context_assembly_full_graph() -> Result<()> {
        use crate::record::McpServerContent;

        let config = DatabaseConfig::embedded(PathBuf::from("./.memex/db"));
        let db = db::Database::connect(&config, "atlas", None).await?;
        let store = Store::new(db);

        // Helper to clean up test records
        async fn cleanup(store: &Store, record_type: &str, name: &str) {
            if let Ok(Some(r)) = store.get_record_by_type_name(record_type, name).await {
                if let Some(id) = r.id_str() {
                    let _ = store.delete_record(&id).await;
                }
            }
        }

        // Clean up from any previous test runs
        let test_records = vec![
            ("repo", "test-context-repo"),
            ("team", "test-context-team"),
            ("person", "test-person-alice"),
            ("person", "test-person-bob"),
            ("rule", "test-rule-commits"),
            ("rule", "test-rule-testing"),
            ("skill", "test-skill-git"),
            ("mcp_server", "test-mcp-github"),
        ];
        for (record_type, name) in &test_records {
            cleanup(&store, record_type, name).await;
        }

        // 1. Create team
        let team = Record::new(RecordType::Team, "test-context-team")
            .with_description("Test development team");
        let team = store.create_record(team).await?;
        let team_id = team.id_str().unwrap();

        // 2. Create people who are member_of the team
        let alice = Record::new(RecordType::Person, "test-person-alice")
            .with_description("Lead developer, Rust expert");
        let alice = store.create_record(alice).await?;
        let alice_id = alice.id_str().unwrap();
        store.create_edge(&alice_id, &team_id, EdgeRelation::MemberOf, None).await?;

        let bob = Record::new(RecordType::Person, "test-person-bob")
            .with_description("Backend developer");
        let bob = store.create_record(bob).await?;
        let bob_id = bob.id_str().unwrap();
        store.create_edge(&bob_id, &team_id, EdgeRelation::MemberOf, None).await?;

        // 3. Create repo that belongs_to the team
        let repo = Record::new(RecordType::Repo, "test-context-repo")
            .with_description("Test repository for context assembly")
            .with_content(json!({
                "path": "/path/to/test-repo",
                "default_branch": "main"
            }));
        let repo = store.create_record(repo).await?;
        let repo_id = repo.id_str().unwrap();
        store.create_edge(&repo_id, &team_id, EdgeRelation::BelongsTo, None).await?;

        // 4. Create rules that apply_to the repo
        let rule_commits = Record::new(RecordType::Rule, "test-rule-commits")
            .with_description("Use single-line commit messages under 50 characters");
        let rule_commits = store.create_record(rule_commits).await?;
        let rule_commits_id = rule_commits.id_str().unwrap();
        store.create_edge(&rule_commits_id, &repo_id, EdgeRelation::AppliesTo, None).await?;

        let rule_testing = Record::new(RecordType::Rule, "test-rule-testing")
            .with_description("All code must have unit tests with >80% coverage");
        let rule_testing = store.create_record(rule_testing).await?;
        let rule_testing_id = rule_testing.id_str().unwrap();
        store.create_edge(&rule_testing_id, &repo_id, EdgeRelation::AppliesTo, None).await?;

        // 5. Create skill that is available_to the repo
        let skill_git = Record::new(RecordType::Skill, "test-skill-git")
            .with_description("Can perform git operations including commits, branches, and merges");
        let skill_git = store.create_record(skill_git).await?;
        let skill_git_id = skill_git.id_str().unwrap();
        store.create_edge(&skill_git_id, &repo_id, EdgeRelation::AvailableTo, None).await?;

        // 6. Create MCP server record
        let mcp_content = McpServerContent {
            command: "gh".to_string(),
            args: vec!["mcp".to_string()],
            env: None,
        };
        let mcp_server = Record::new(RecordType::McpServer, "test-mcp-github")
            .with_description("GitHub CLI MCP server")
            .with_content(mcp_content.to_json());
        let mcp_server = store.create_record(mcp_server).await?;
        let mcp_server_id = mcp_server.id_str().unwrap();
        store.create_edge(&mcp_server_id, &repo_id, EdgeRelation::AvailableTo, None).await?;

        // NOW TEST CONTEXT ASSEMBLY
        println!("\n=== Testing context assembly from repo ===");

        let context = store.assemble_context(&repo_id, 3).await?;

        println!("Traversal path:");
        for path in &context.traversal_path {
            println!("  {}", path);
        }

        println!("\nRecords found: {}", context.records.len());
        for record in &context.records {
            println!(
                "  [{}] {} - {}",
                record.record_type,
                record.name,
                record.description.as_deref().unwrap_or("")
            );
        }

        // Verify we found all expected records
        assert_eq!(context.rules().len(), 2, "Should find 2 rules");
        assert_eq!(context.skills().len(), 1, "Should find 1 skill");
        assert_eq!(context.people().len(), 2, "Should find 2 people");
        assert_eq!(context.mcp_servers().len(), 1, "Should find 1 MCP server");

        // Verify specific records were found
        let rule_names: Vec<&str> = context.rules().iter().map(|r| r.name.as_str()).collect();
        assert!(rule_names.contains(&"test-rule-commits"), "Should find commits rule");
        assert!(rule_names.contains(&"test-rule-testing"), "Should find testing rule");

        let people_names: Vec<&str> = context.people().iter().map(|r| r.name.as_str()).collect();
        assert!(people_names.contains(&"test-person-alice"), "Should find Alice");
        assert!(people_names.contains(&"test-person-bob"), "Should find Bob");

        // Verify system prompt generation
        println!("\n=== Generated system prompt ===");
        let prompt = context.to_system_prompt();
        println!("{}", prompt);

        assert!(prompt.contains("# Context from Knowledge Base"));
        assert!(prompt.contains("## Rules"));
        assert!(prompt.contains("test-rule-commits"));
        assert!(prompt.contains("test-rule-testing"));
        assert!(prompt.contains("## Available Skills"));
        assert!(prompt.contains("test-skill-git"));
        assert!(prompt.contains("## Team"));
        assert!(prompt.contains("test-person-alice"));
        assert!(prompt.contains("test-person-bob"));

        // Verify MCP server config extraction
        println!("\n=== MCP server configs ===");
        let mcp_configs = context.mcp_server_configs();
        assert_eq!(mcp_configs.len(), 1, "Should find 1 MCP config");
        let config_json = &mcp_configs[0];
        println!("{}", config_json);

        // Parse and verify the config
        let parsed: serde_json::Value = serde_json::from_str(config_json)?;
        assert!(parsed["mcpServers"]["test-mcp-github"].is_object());
        assert_eq!(parsed["mcpServers"]["test-mcp-github"]["command"], "gh");

        // Cleanup
        for (record_type, name) in &test_records {
            cleanup(&store, record_type, name).await;
        }
        println!("\n=== Cleaned up test records ===");

        Ok(())
    }

    /// Test context assembly depth limiting.
    ///
    /// Verifies that the max_depth parameter correctly limits traversal depth.
    ///
    /// Run with: cargo test -p atlas context_assembly_depth_limit -- --nocapture --ignored
    #[tokio::test]
    #[ignore] // Requires running database
    async fn context_assembly_depth_limit() -> Result<()> {
        let config = DatabaseConfig::embedded(PathBuf::from("./.memex/db"));
        let db = db::Database::connect(&config, "atlas", None).await?;
        let store = Store::new(db);

        // Helper to clean up test records
        async fn cleanup(store: &Store, record_type: &str, name: &str) {
            if let Ok(Some(r)) = store.get_record_by_type_name(record_type, name).await {
                if let Some(id) = r.id_str() {
                    let _ = store.delete_record(&id).await;
                }
            }
        }

        // Clean up
        let test_records = vec![
            ("repo", "depth-repo"),
            ("team", "depth-team"),
            ("person", "depth-person"),
            ("rule", "depth-rule"),
        ];
        for (record_type, name) in &test_records {
            cleanup(&store, record_type, name).await;
        }

        // Create a chain: repo -> team -> person (via member_of)
        //                 rule -> repo (via applies_to)
        let team = Record::new(RecordType::Team, "depth-team");
        let team = store.create_record(team).await?;
        let team_id = team.id_str().unwrap();

        let person = Record::new(RecordType::Person, "depth-person");
        let person = store.create_record(person).await?;
        let person_id = person.id_str().unwrap();
        store.create_edge(&person_id, &team_id, EdgeRelation::MemberOf, None).await?;

        let repo = Record::new(RecordType::Repo, "depth-repo");
        let repo = store.create_record(repo).await?;
        let repo_id = repo.id_str().unwrap();
        store.create_edge(&repo_id, &team_id, EdgeRelation::BelongsTo, None).await?;

        let rule = Record::new(RecordType::Rule, "depth-rule");
        let rule = store.create_record(rule).await?;
        let rule_id = rule.id_str().unwrap();
        store.create_edge(&rule_id, &repo_id, EdgeRelation::AppliesTo, None).await?;

        // Test depth 0: should only get the starting record
        let ctx0 = store.assemble_context(&repo_id, 0).await?;
        println!("Depth 0: {} records", ctx0.records.len());
        assert_eq!(ctx0.records.len(), 1, "Depth 0 should only include the start record");
        assert_eq!(ctx0.records[0].name, "depth-repo");

        // Test depth 1: should get repo + immediate neighbors (team, rule)
        let ctx1 = store.assemble_context(&repo_id, 1).await?;
        println!("Depth 1: {} records", ctx1.records.len());
        assert!(ctx1.records.len() >= 2, "Depth 1 should include immediate neighbors");
        let names: Vec<&str> = ctx1.records.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"depth-repo"));
        assert!(names.contains(&"depth-team") || names.contains(&"depth-rule"));

        // Test depth 3: should get all records
        let ctx3 = store.assemble_context(&repo_id, 3).await?;
        println!("Depth 3: {} records", ctx3.records.len());
        assert_eq!(ctx3.records.len(), 4, "Depth 3 should include all records");
        let names: Vec<&str> = ctx3.records.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"depth-repo"));
        assert!(names.contains(&"depth-team"));
        assert!(names.contains(&"depth-person"));
        assert!(names.contains(&"depth-rule"));

        // Cleanup
        for (record_type, name) in &test_records {
            cleanup(&store, record_type, name).await;
        }

        Ok(())
    }
}

