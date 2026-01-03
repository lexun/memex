//! IPC client for memo operations
//!
//! Provides access to memo storage via the daemon.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

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
