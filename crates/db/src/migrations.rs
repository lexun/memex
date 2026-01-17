//! Database migrations
//!
//! Migrations are organized per-database and use unix timestamps as version numbers.
//! Schema version is simply the timestamp of the last applied migration.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tracing::info;

/// Schema version record stored in the database
#[derive(Debug, Serialize, Deserialize)]
struct SchemaVersion {
    version: i64,
}

/// A migration with its timestamp and SQL content
struct Migration {
    timestamp: i64,
    name: &'static str,
    sql: &'static str,
}

/// Forge migrations
const FORGE_MIGRATIONS: &[Migration] = &[
    Migration {
        timestamp: 1735689600,
        name: "initial_schema",
        sql: include_str!("../migrations/forge/1735689600_initial_schema.surql"),
    },
    Migration {
        timestamp: 1768780800,
        name: "worker_schema",
        sql: include_str!("../migrations/forge/1768780800_worker_schema.surql"),
    },
];

/// Atlas migrations
const ATLAS_MIGRATIONS: &[Migration] = &[
    Migration {
        timestamp: 1735776000,
        name: "memo_schema",
        sql: include_str!("../migrations/atlas/1735776000_memo_schema.surql"),
    },
    Migration {
        timestamp: 1767398400,
        name: "event_schema",
        sql: include_str!("../migrations/atlas/1767398400_event_schema.surql"),
    },
    Migration {
        timestamp: 1767571200,
        name: "fact_extraction",
        sql: include_str!("../migrations/atlas/1767571200_fact_extraction.surql"),
    },
];

/// Run all pending migrations for the given database
pub async fn run_migrations(db: &Surreal<Any>, database_name: &str) -> Result<()> {
    let migrations = match database_name {
        "forge" => FORGE_MIGRATIONS,
        "atlas" => ATLAS_MIGRATIONS,
        _ => {
            info!("No migrations defined for database: {}", database_name);
            return Ok(());
        }
    };

    if migrations.is_empty() {
        return Ok(());
    }

    let current = get_current_version(db).await?;
    let pending: Vec<_> = migrations.iter().filter(|m| m.timestamp > current).collect();

    if pending.is_empty() {
        info!(
            "Database '{}' schema is up to date (version {})",
            database_name, current
        );
        return Ok(());
    }

    info!(
        "Running {} migration(s) for '{}' (from version {})",
        pending.len(),
        database_name,
        current
    );

    for migration in &pending {
        info!(
            "Applying migration {}: {}",
            migration.timestamp, migration.name
        );
        db.query(migration.sql)
            .await
            .with_context(|| format!("Failed to apply migration {}", migration.timestamp))?;
        set_version(db, migration.timestamp).await?;
    }

    info!(
        "Migrations complete for '{}' (now at version {})",
        database_name,
        pending.last().map(|m| m.timestamp).unwrap_or(current)
    );

    Ok(())
}

/// Get the current schema version from the database
async fn get_current_version(db: &Surreal<Any>) -> Result<i64> {
    // Ensure schema_version table exists
    db.query("DEFINE TABLE IF NOT EXISTS schema_version SCHEMAFULL; DEFINE FIELD IF NOT EXISTS version ON schema_version TYPE int;")
        .await
        .context("Failed to ensure schema_version table")?;

    let result: Option<SchemaVersion> = db
        .select(("schema_version", "current"))
        .await
        .context("Failed to query schema version")?;

    Ok(result.map(|r| r.version).unwrap_or(0))
}

/// Set the schema version in the database
async fn set_version(db: &Surreal<Any>, version: i64) -> Result<()> {
    let _: Option<SchemaVersion> = db
        .upsert(("schema_version", "current"))
        .content(SchemaVersion { version })
        .await
        .context("Failed to update schema version")?;

    Ok(())
}
