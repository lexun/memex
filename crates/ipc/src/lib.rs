//! IPC protocol and client for Memex daemon communication
//!
//! This crate provides the low-level infrastructure for communicating with
//! the Memex daemon over Unix sockets. It has no domain knowledge - it just
//! handles JSON-RPC style request/response messaging.
//!
//! # Architecture
//!
//! ```text
//! Domain Crate (forge, atlas)     IPC Crate
//! ┌─────────────────────────┐    ┌─────────────────────┐
//! │  TaskClient / AtlasClient│───>│  Client             │
//! │  (typed wrapper)         │    │  (generic JSON-RPC) │
//! └─────────────────────────┘    └──────────┬──────────┘
//!                                           │
//!                                           v
//!                                    Unix Socket
//!                                           │
//!                                           v
//!                                    ┌──────────────┐
//!                                    │   Daemon     │
//!                                    └──────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use ipc::Client;
//!
//! let client = Client::new("/path/to/socket.sock");
//!
//! // Check if daemon is running
//! if client.health_check().await? {
//!     // Send a request
//!     let result = client.request("some_method", json!({"key": "value"})).await?;
//! }
//! ```

mod client;
mod protocol;

pub use client::Client;
pub use protocol::{Error, ErrorCode, Request, Response};
