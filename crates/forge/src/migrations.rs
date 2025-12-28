//! Database migrations for Forge
//!
//! Migrations are embedded at compile time and run sequentially on startup.
//! Version tracking ensures migrations only run once.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tracing::info;

/// Current schema version - increment when adding new migrations
const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Schema version record stored in the database
#[derive(Debug, Serialize, Deserialize)]
struct SchemaVersion {
    version: u32,
}

/// Migration 001: Initial task schema
const MIGRATION_001: &str = include_str!("../migrations/001_initial_schema.surql");

/// Migration 002: Memo schema for Atlas
const MIGRATION_002: &str = include_str!("../migrations/002_memo_schema.surql");

/// Run all pending migrations
pub async fn run_migrations(db: &Surreal<Db>) -> Result<()> {
    let current = get_current_version(db).await?;

    if current >= CURRENT_SCHEMA_VERSION {
        info!("Database schema is up to date (version {})", current);
        return Ok(());
    }

    info!(
        "Running migrations from version {} to {}",
        current, CURRENT_SCHEMA_VERSION
    );

    // Run migrations sequentially
    if current < 1 {
        info!("Applying migration 001: Initial task schema");
        db.query(MIGRATION_001)
            .await
            .context("Failed to apply migration 001")?;
        set_version(db, 1).await?;
    }

    if current < 2 {
        info!("Applying migration 002: Memo schema");
        db.query(MIGRATION_002)
            .await
            .context("Failed to apply migration 002")?;
        set_version(db, 2).await?;
    }

    info!("Migrations complete (now at version {})", CURRENT_SCHEMA_VERSION);
    Ok(())
}

/// Get the current schema version from the database
async fn get_current_version(db: &Surreal<Db>) -> Result<u32> {
    let result: Option<SchemaVersion> = db
        .select(("schema_version", "current"))
        .await
        .context("Failed to query schema version")?;

    Ok(result.map(|r| r.version).unwrap_or(0))
}

/// Set the schema version in the database
async fn set_version(db: &Surreal<Db>, version: u32) -> Result<()> {
    let _: Option<SchemaVersion> = db
        .upsert(("schema_version", "current"))
        .content(SchemaVersion { version })
        .await
        .context("Failed to update schema version")?;

    Ok(())
}
