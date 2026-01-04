//! Database connection management
//!
//! Uses SurrealDB's `Surreal<Any>` for runtime connection type selection.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use tracing::info;

use crate::config::DatabaseConfig;
use crate::migrations;

/// Database connection wrapper
///
/// Wraps a `Surreal<Any>` connection that can be either embedded or remote.
#[derive(Clone)]
pub struct Database {
    client: Surreal<Any>,
    database_name: String,
}

impl Database {
    /// Connect to the database and run migrations
    ///
    /// # Arguments
    /// * `config` - Database configuration
    /// * `database_name` - The database name to use (e.g., "forge" or "atlas")
    /// * `default_path` - Default path for embedded database if not specified in config
    pub async fn connect(
        config: &DatabaseConfig,
        database_name: &str,
        default_path: Option<PathBuf>,
    ) -> Result<Self> {
        // Validate config
        if config.url.is_some() && config.path.is_some() {
            bail!("Database config has both 'url' and 'path' set - this is ambiguous");
        }

        let client = if let Some(url) = &config.url {
            // Remote connection
            info!("Connecting to remote database: {}", url);
            let db = surrealdb::engine::any::connect(url)
                .await
                .context("Failed to connect to remote database")?;

            // Authenticate
            let username = config
                .username
                .as_deref()
                .context("Remote database requires 'username'")?;
            let password = config
                .password
                .as_deref()
                .context("Remote database requires 'password'")?;

            db.signin(Root { username, password })
                .await
                .context("Failed to authenticate with remote database")?;

            db
        } else {
            // Embedded connection
            let path = config
                .path
                .clone()
                .or(default_path)
                .context("No database path specified and no default provided")?;

            info!("Opening embedded database at: {}", path.display());

            let connection_string = format!("rocksdb://{}", path.display());
            surrealdb::engine::any::connect(&connection_string)
                .await
                .context("Failed to open embedded database")?
        };

        // Select namespace and database
        let namespace = config.namespace();
        client
            .use_ns(namespace)
            .use_db(database_name)
            .await
            .context("Failed to select namespace/database")?;

        info!(
            "Connected to database: {}/{}",
            namespace, database_name
        );

        let db = Self {
            client,
            database_name: database_name.to_string(),
        };

        // Run migrations
        db.run_migrations().await?;

        Ok(db)
    }

    /// Get a reference to the SurrealDB client
    pub fn client(&self) -> &Surreal<Any> {
        &self.client
    }

    /// Get the database name
    pub fn database_name(&self) -> &str {
        &self.database_name
    }

    /// Run pending migrations for this database
    async fn run_migrations(&self) -> Result<()> {
        migrations::run_migrations(&self.client, &self.database_name).await
    }

    /// Create a new Database using the same underlying connection but different database
    ///
    /// This is useful for embedded mode where only one RocksDB connection can exist.
    /// The new Database shares the underlying client but selects a different database.
    pub async fn with_database(&self, database_name: &str) -> Result<Self> {
        // Clone the client (shares the underlying connection)
        let client = self.client.clone();

        // Select the new database (namespace is shared)
        client
            .use_db(database_name)
            .await
            .context("Failed to select database")?;

        let db = Self {
            client,
            database_name: database_name.to_string(),
        };

        // Run migrations for this database
        db.run_migrations().await?;

        Ok(db)
    }
}
