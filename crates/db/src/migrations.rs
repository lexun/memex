//! Database migrations
//!
//! Migrations are organized per-database and use unix timestamps as version numbers.
//! Schema version is simply the timestamp of the last applied migration.
//!
//! ## Schema Stability
//!
//! To maintain reliability, follow these guidelines:
//! 1. New migrations should be additive when possible (add fields, don't remove)
//! 2. Use `IF NOT EXISTS` for idempotent migrations
//! 3. Run `validate_schema` after migrations to verify expected tables exist
//! 4. Never modify existing migration files - create new ones instead

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tracing::{info, warn};

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
        timestamp: 1737158400,
        name: "record_schema",
        sql: include_str!("../migrations/atlas/1737158400_record_schema.surql"),
    },
    Migration {
        timestamp: 1737244800,
        name: "flexible_schema",
        sql: include_str!("../migrations/atlas/1737244800_flexible_schema.surql"),
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
    Migration {
        timestamp: 1769179600,
        name: "purge_support",
        sql: include_str!("../migrations/atlas/1769179600_purge_support.surql"),
    },
    Migration {
        timestamp: 1769340000,
        name: "record_aliases",
        sql: include_str!("../migrations/atlas/1769340000_record_aliases.surql"),
    },
    Migration {
        timestamp: 1769429008,
        name: "backfill_aliases",
        sql: include_str!("../migrations/atlas/1769429008_backfill_aliases.surql"),
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

/// Expected tables for each database
const ATLAS_EXPECTED_TABLES: &[&str] = &[
    "memo",
    "event",
    "fact",
    "entity",
    "fact_entity",
    "record",
    "record_edge",
];

const FORGE_EXPECTED_TABLES: &[&str] = &["task", "task_update", "goal", "worker", "agent_message"];

/// Schema status for a database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaStatus {
    /// Current schema version (timestamp of last migration)
    pub version: i64,
    /// Latest available migration version
    pub latest_version: i64,
    /// Whether schema is up to date
    pub up_to_date: bool,
    /// Number of pending migrations
    pub pending_migrations: usize,
    /// Tables that exist as expected
    pub valid_tables: Vec<String>,
    /// Tables that are missing
    pub missing_tables: Vec<String>,
    /// Whether schema validation passed
    pub valid: bool,
}

/// Get the schema status for a database
pub async fn get_schema_status(db: &Surreal<Any>, database_name: &str) -> Result<SchemaStatus> {
    let (migrations, expected_tables) = match database_name {
        "forge" => (FORGE_MIGRATIONS, FORGE_EXPECTED_TABLES),
        "atlas" => (ATLAS_MIGRATIONS, ATLAS_EXPECTED_TABLES),
        _ => {
            return Ok(SchemaStatus {
                version: 0,
                latest_version: 0,
                up_to_date: true,
                pending_migrations: 0,
                valid_tables: vec![],
                missing_tables: vec![],
                valid: true,
            });
        }
    };

    let current = get_current_version(db).await?;
    let latest = migrations.last().map(|m| m.timestamp).unwrap_or(0);
    let pending = migrations.iter().filter(|m| m.timestamp > current).count();

    // Check which tables exist
    let mut valid_tables = Vec::new();
    let mut missing_tables = Vec::new();

    for table in expected_tables {
        let sql = format!("INFO FOR TABLE {}", table);
        match db.query(&sql).await {
            Ok(_) => valid_tables.push(table.to_string()),
            Err(_) => missing_tables.push(table.to_string()),
        }
    }

    let valid = missing_tables.is_empty() && pending == 0;

    Ok(SchemaStatus {
        version: current,
        latest_version: latest,
        up_to_date: pending == 0,
        pending_migrations: pending,
        valid_tables,
        missing_tables,
        valid,
    })
}

/// Validate that all expected tables exist after migrations
///
/// Returns Ok(()) if validation passes, or an error listing missing tables.
pub async fn validate_schema(db: &Surreal<Any>, database_name: &str) -> Result<()> {
    let status = get_schema_status(db, database_name).await?;

    if !status.missing_tables.is_empty() {
        warn!(
            "Schema validation failed for '{}': missing tables {:?}",
            database_name, status.missing_tables
        );
        anyhow::bail!(
            "Schema validation failed: missing tables: {:?}",
            status.missing_tables
        );
    }

    if !status.up_to_date {
        warn!(
            "Schema for '{}' has {} pending migrations",
            database_name, status.pending_migrations
        );
    }

    info!(
        "Schema validation passed for '{}' (version {}, {} tables)",
        database_name,
        status.version,
        status.valid_tables.len()
    );

    Ok(())
}
