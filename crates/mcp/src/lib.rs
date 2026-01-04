//! MCP server for Memex
//!
//! Exposes Memex functionality via the Model Context Protocol (MCP),
//! allowing AI assistants to manage tasks, record memos, and query context.
//!
//! # Tools Provided
//!
//! - **Task management**: create, list, update, close, delete tasks
//! - **Notes**: add, edit, delete task notes
//! - **Dependencies**: manage task relationships
//! - **Memos**: record and query knowledge base memos
//! - **Events**: view the event log
//! - **Context discovery**: search across all stored knowledge
//!
//! # Usage
//!
//! The server runs on stdio and connects to the Memex daemon via Unix socket:
//!
//! ```bash
//! memex mcp serve
//! ```

mod server;

pub use server::start_server;
