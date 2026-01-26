//! Database connection and migration management for Memex
//!
//! Provides a unified interface for connecting to SurrealDB, supporting both
//! embedded (RocksDB) and remote (WebSocket) connections.
//!
//! # Connection Modes
//!
//! - **Embedded**: Uses RocksDB for local storage, ideal for single-machine setups
//! - **Remote**: Connects via WebSocket to SurrealDB Cloud or self-hosted instances
//!
//! The mode is determined by the `DatabaseConfig`:
//! - If `url` is set, connects remotely
//! - Otherwise, uses the local `path` for embedded storage
//!
//! # Migrations
//!
//! Each namespace (forge, atlas) has its own migration set in `migrations/<namespace>/`.
//! Migrations are applied automatically on connection.

mod config;
mod connection;
mod migrations;

pub use config::DatabaseConfig;
pub use connection::Database;
pub use migrations::{get_schema_status, validate_schema, SchemaStatus};
