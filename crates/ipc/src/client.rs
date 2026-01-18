//! Base IPC client for daemon communication
//!
//! Provides low-level socket communication with JSON-RPC style messages.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{debug, trace};

use crate::protocol::{Error, ErrorCode, Request, Response};

/// Base IPC client for communicating with the daemon
///
/// This client has no domain knowledge - it just sends requests and receives
/// responses over a Unix socket using newline-delimited JSON.
#[derive(Debug, Clone)]
pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    /// Create a new client for the given socket path
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    /// Get the socket path
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Check if the daemon is reachable
    pub async fn health_check(&self) -> Result<bool> {
        match self.request("health_check", ()).await {
            Ok(_) => Ok(true),
            Err(e) => {
                debug!("Health check failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Send a request to the daemon and wait for a response
    ///
    /// This creates a new connection for each request (connection-per-request model).
    /// This is simpler and avoids connection state management issues.
    pub async fn request<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> Result<serde_json::Value> {
        let request = Request::new(method, params)
            .context("Failed to create request")?;

        trace!("Sending request: {} (id={})", method, request.id);

        let response = self.send_request(&request).await?;

        if response.id != request.id {
            anyhow::bail!(
                "Response ID mismatch: expected {}, got {}",
                request.id,
                response.id
            );
        }

        response.into_result().map_err(|e| anyhow::anyhow!(e))
    }

    /// Send a request with no parameters
    pub async fn request_empty(&self, method: &str) -> Result<serde_json::Value> {
        self.request(method, ()).await
    }

    /// Low-level: send a request and receive a response
    async fn send_request(&self, request: &Request) -> Result<Response> {
        let total_start = Instant::now();

        // Connect to socket
        let connect_start = Instant::now();
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!(
                "Failed to connect to daemon at {}. Is the daemon running?",
                self.socket_path.display()
            ))?;
        let connect_elapsed = connect_start.elapsed();

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Serialize and send request
        let serialize_start = Instant::now();
        let mut request_json = serde_json::to_string(request)
            .context("Failed to serialize request")?;
        request_json.push('\n');
        let serialize_elapsed = serialize_start.elapsed();

        let write_start = Instant::now();
        writer
            .write_all(request_json.as_bytes())
            .await
            .context("Failed to write request")?;
        writer.flush().await.context("Failed to flush request")?;
        let write_elapsed = write_start.elapsed();

        trace!("Request sent, waiting for response");

        // Read response
        let read_start = Instant::now();
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .await
            .context("Failed to read response")?;
        let read_elapsed = read_start.elapsed();

        if response_line.is_empty() {
            return Ok(Response::error(
                &request.id,
                Error::new(ErrorCode::ConnectionError, "Connection closed by daemon"),
            ));
        }

        let deserialize_start = Instant::now();
        let response: Response = serde_json::from_str(&response_line)
            .context("Failed to parse response")?;
        let deserialize_elapsed = deserialize_start.elapsed();

        let total_elapsed = total_start.elapsed();

        // Log detailed timing breakdown
        debug!(
            method = %request.method,
            total_ms = total_elapsed.as_micros() as f64 / 1000.0,
            connect_ms = connect_elapsed.as_micros() as f64 / 1000.0,
            serialize_ms = serialize_elapsed.as_micros() as f64 / 1000.0,
            write_ms = write_elapsed.as_micros() as f64 / 1000.0,
            read_ms = read_elapsed.as_micros() as f64 / 1000.0,
            deserialize_ms = deserialize_elapsed.as_micros() as f64 / 1000.0,
            "IPC timing breakdown"
        );

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = Client::new("/tmp/test.sock");
        assert_eq!(client.socket_path(), Path::new("/tmp/test.sock"));
    }
}
