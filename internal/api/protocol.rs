//! JSON-RPC and MessagePack protocol surface (`protocol.go`).

pub use tsgo_jsonrpc::Message;
use tsgo_jsonrpc::{Id, ResponseError};

/// Reads and writes API messages on a bidirectional connection.
///
/// Implemented by `JSONRPCProtocol` and `MessagePackProtocol` (both deferred).
///
/// Side effects: performs I/O on the underlying stream (implementations).
// Go: internal/api/protocol.go:Protocol
pub trait Protocol {
    /// Reads the next message from the connection.
    ///
    /// # Examples
    /// ```ignore
    /// // Exercised once `JSONRPCProtocol` / `MessagePackProtocol` are ported.
    /// ```
    ///
    /// Side effects: reads from the connection.
    // Go: internal/api/protocol.go:Protocol.ReadMessage
    fn read_message(&mut self) -> Result<Message, ProtocolError>;

    /// Writes a request message.
    ///
    /// Side effects: writes to the connection.
    // Go: internal/api/protocol.go:Protocol.WriteRequest
    fn write_request(
        &mut self,
        id: &Id,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), ProtocolError>;

    /// Writes a notification (no response id).
    ///
    /// Side effects: writes to the connection.
    // Go: internal/api/protocol.go:Protocol.WriteNotification
    fn write_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), ProtocolError>;

    /// Writes a successful response.
    ///
    /// Side effects: writes to the connection.
    // Go: internal/api/protocol.go:Protocol.WriteResponse
    fn write_response(&mut self, id: &Id, result: serde_json::Value) -> Result<(), ProtocolError>;

    /// Writes an error response.
    ///
    /// Side effects: writes to the connection.
    // Go: internal/api/protocol.go:Protocol.WriteError
    fn write_error(&mut self, id: &Id, err: &ResponseError) -> Result<(), ProtocolError>;
}

/// Protocol-layer failures (I/O and framing); refined when codecs land.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Underlying I/O error.
    #[error("protocol I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Framing or parse error with a stable message.
    #[error("{0}")]
    Other(String),
}
