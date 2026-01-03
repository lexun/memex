//! Database connection and migration management for Memex
//!
//! Provides a unified interface for connecting to SurrealDB, supporting both
//! embedded RocksDB and remote WebSocket connections.

mod config;
mod connection;
mod migrations;

pub use config::DatabaseConfig;
pub use connection::Database;
