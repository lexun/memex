//! Database store for Atlas knowledge management
//!
//! Handles memo and event storage, plus future knowledge graph operations.

use anyhow::{Context, Result};
use db::Database;

use crate::event::{Event, EventSource};
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

    /// Search memos by content (full-text search)
    pub async fn search_memos(&self, query: &str, limit: Option<usize>) -> Result<Vec<Memo>> {
        let limit_clause = limit.map(|n| format!(" LIMIT {}", n)).unwrap_or_default();
        let sql = format!(
            "SELECT * FROM memo WHERE content @@ $query ORDER BY created_at DESC{}",
            limit_clause
        );

        let mut response = self
            .db
            .client()
            .query(&sql)
            .bind(("query", query))
            .await
            .context("Failed to search memos")?;
        let memos: Vec<Memo> = response.take(0).context("Failed to parse memos")?;

        Ok(memos)
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
}
