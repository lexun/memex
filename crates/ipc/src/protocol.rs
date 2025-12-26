//! IPC protocol types for daemon communication
//!
//! Uses a JSON-RPC inspired format with newline-delimited messages.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A request to the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Unique request identifier
    pub id: String,
    /// Method name to invoke
    pub method: String,
    /// Method parameters as JSON value
    #[serde(default)]
    pub params: serde_json::Value,
}

impl Request {
    /// Create a new request with auto-generated ID
    pub fn new(method: impl Into<String>, params: impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            method: method.into(),
            params: serde_json::to_value(params)?,
        })
    }

    /// Create a request with no parameters
    pub fn empty(method: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            method: method.into(),
            params: serde_json::Value::Null,
        }
    }
}

/// A response from the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Request ID this response corresponds to
    pub id: String,
    /// Result value on success
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error on failure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

impl Response {
    /// Create a success response
    pub fn success(id: impl Into<String>, result: impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id: id.into(),
            result: Some(serde_json::to_value(result)?),
            error: None,
        })
    }

    /// Create a success response with no result value
    pub fn ok(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            result: Some(serde_json::Value::Null),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: impl Into<String>, error: Error) -> Self {
        Self {
            id: id.into(),
            result: None,
            error: Some(error),
        }
    }

    /// Check if this response is an error
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get the result, returning an error if the response was an error
    pub fn into_result(self) -> Result<serde_json::Value, Error> {
        if let Some(err) = self.error {
            Err(err)
        } else {
            Ok(self.result.unwrap_or(serde_json::Value::Null))
        }
    }
}

/// Error information in a response
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    /// Error code
    pub code: ErrorCode,
    /// Human-readable error message
    pub message: String,
    /// Optional additional data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Error {
    /// Create a new error
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create a not found error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    /// Create an invalid params error
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, message)
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// Create a method not found error
    pub fn method_not_found(method: &str) -> Self {
        Self::new(ErrorCode::MethodNotFound, format!("Unknown method: {}", method))
    }

    /// Add optional data to the error
    pub fn with_data(mut self, data: impl Serialize) -> Self {
        self.data = serde_json::to_value(data).ok();
        self
    }
}

/// Error codes for IPC errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Parse error - invalid JSON
    ParseError,
    /// Invalid request structure
    InvalidRequest,
    /// Method not found
    MethodNotFound,
    /// Invalid method parameters
    InvalidParams,
    /// Internal error
    Internal,
    /// Resource not found
    NotFound,
    /// Connection error
    ConnectionError,
    /// Timeout
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = Request::new("test_method", serde_json::json!({"key": "value"})).unwrap();
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("test_method"));
        assert!(json.contains("key"));
    }

    #[test]
    fn test_response_success() {
        let resp = Response::success("123", "result_value").unwrap();
        assert!(!resp.is_error());
        assert_eq!(resp.result.unwrap(), serde_json::json!("result_value"));
    }

    #[test]
    fn test_response_error() {
        let resp = Response::error("123", Error::not_found("Task not found"));
        assert!(resp.is_error());
        assert!(resp.error.unwrap().message.contains("not found"));
    }
}
