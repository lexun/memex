//! Database store for Atlas knowledge management
//!
//! Handles memo storage and future knowledge graph operations.

use anyhow::{Context, Result};
use db::Database;

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
}
