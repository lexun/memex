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

    /// Search score
    #[serde(default)]
    pub score: f64,
}

fn default_confidence() -> f32 {
    1.0
}

