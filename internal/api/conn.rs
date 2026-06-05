//! Connection and handler traits (`conn.go`).

use serde::de::DeserializeOwned;
use tsgo_json::Error as JsonError;

/// Returned when the API connection has been closed.
///
/// # Examples
/// ```
/// assert_eq!(
///     tsgo_api::ERR_CONN_CLOSED.to_string(),
///     "api: connection closed"
/// );
/// ```
///
/// Side effects: none (pure constant).
// Go: internal/api/conn.go:ErrConnClosed
pub const ERR_CONN_CLOSED: &str = "api: connection closed";

/// Returned when a client `Call` times out waiting for a response.
///
/// # Examples
/// ```
/// assert_eq!(
///     tsgo_api::ERR_REQUEST_TIMEOUT.to_string(),
///     "api: request timeout"
/// );
/// ```
///
/// Side effects: none (pure constant).
// Go: internal/api/conn.go:ErrRequestTimeout
pub const ERR_REQUEST_TIMEOUT: &str = "api: request timeout";

/// Processes incoming API requests and notifications.
///
/// Side effects: defined by implementors (typically mutates session state).
// Go: internal/api/conn.go:Handler
pub trait Handler: Send + Sync {
    /// Handles a request and returns a JSON-serializable result.
    ///
    /// Side effects: defined by implementor.
    // Go: internal/api/conn.go:Handler.HandleRequest
    fn handle_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Result<Option<serde_json::Value>, JsonError>;

    /// Handles a notification (no response).
    ///
    /// Side effects: defined by implementor.
    // Go: internal/api/conn.go:Handler.HandleNotification
    fn handle_notification(&self, method: &str, params: &[u8]) -> Result<(), JsonError>;
}

/// Bidirectional API connection (`Run` / `Call` / `Notify`).
///
/// Side effects: defined by implementors (I/O and concurrency).
// Go: internal/api/conn.go:Conn
pub trait Conn: Send {
    /// Processes messages until cancelled or an error occurs.
    ///
    /// Side effects: reads/writes the connection; may spawn workers in async mode.
    // Go: internal/api/conn.go:Conn.Run
    fn run(&mut self) -> Result<(), ConnError>;

    /// Sends a request to the peer and waits for a JSON result.
    ///
    /// Side effects: writes request, blocks on response.
    // Go: internal/api/conn.go:Conn.Call
    fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ConnError>;

    /// Sends a notification (no response).
    ///
    /// Side effects: writes to the connection.
    // Go: internal/api/conn.go:Conn.Notify
    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), ConnError>;
}

/// Connection-level error (closed, timeout, I/O).
#[derive(Debug, thiserror::Error)]
pub enum ConnError {
    /// Connection closed (`ERR_CONN_CLOSED`).
    #[error("{0}")]
    Closed(&'static str),
    /// Request timed out (`ERR_REQUEST_TIMEOUT`).
    #[error("{0}")]
    Timeout(&'static str),
    /// JSON or protocol error.
    #[error("json: {0}")]
    Json(#[from] JsonError),
    /// Other failure.
    #[error("{0}")]
    Other(String),
}

/// Unmarshals JSON params into `T`; empty params yield `None` (Go `nil` pointer).
///
/// # Examples
/// ```
/// #[derive(serde::Deserialize, PartialEq, Debug)]
/// struct P { pub x: i32 }
/// let p: P = tsgo_api::unmarshal_params(br#"{"x":1}"#).unwrap().unwrap();
/// assert_eq!(p.x, 1);
/// assert!(tsgo_api::unmarshal_params::<P>(b"").unwrap().is_none());
/// ```
///
/// Side effects: none (pure).
// Go: internal/api/conn.go:UnmarshalParams
pub fn unmarshal_params<T: DeserializeOwned>(params: &[u8]) -> Result<Option<T>, JsonError> {
    if params.is_empty() {
        return Ok(None);
    }
    let v = tsgo_json::unmarshal(params)?;
    Ok(Some(v))
}

#[cfg(test)]
#[path = "conn_test.rs"]
mod tests;
