//! `tsgo_jsonrpc` — 1:1 Rust port of Go `internal/jsonrpc`.
//!
//! Generic JSON-RPC 2.0 types plus the LSP base protocol (`Content-Length`
//! framing), shared by `lsproto`, `api`, and other JSON-RPC based protocols.
//! This crate does not decode method-specific params/result; it only models a
//! message and its framing.
//!
//! # Divergence from Go
//! - Go's `ID` stores `str`/`int` and treats `str == ""` as "use int". This
//!   port uses an explicit [`Id`] enum, which is unambiguous (e.g. `Id::Int(0)`
//!   is clearly the integer `0`).
//! - `Reader::read` reports a clean end-of-stream as `Ok(None)` rather than
//!   Go's `(nil, io.EOF)`.
//! - `Message.Params`/`Result` (`json.Value`, raw JSON) map to
//!   `Option<Box<serde_json::value::RawValue>>`.
//! - Underlying parse-error text differs from Go's `strconv`; the framing
//!   wrapper text is preserved verbatim (see `baseproto`).

mod baseproto;

pub use baseproto::{BaseProtoError, Reader, Writer};

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

/// The JSON-RPC version field, which is always `"2.0"`.
///
/// A zero-sized type that serializes to the literal `"2.0"` and rejects any
/// other value on deserialize.
///
/// # Examples
/// ```
/// let s = serde_json::to_string(&tsgo_jsonrpc::JsonRpcVersion).unwrap();
/// assert_eq!(s, "\"2.0\"");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JsonRpcVersion;

impl Serialize for JsonRpcVersion {
    // Go: internal/jsonrpc/jsonrpc.go:JSONRPCVersion.MarshalJSON
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for JsonRpcVersion {
    // Go: internal/jsonrpc/jsonrpc.go:JSONRPCVersion.UnmarshalJSON
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s == "2.0" {
            Ok(JsonRpcVersion)
        } else {
            Err(serde::de::Error::custom("invalid JSON-RPC version"))
        }
    }
}

/// A JSON-RPC message ID, either a string or an integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Id {
    /// A string ID.
    Str(String),
    /// An integer ID.
    Int(i32),
}

/// A helper for constructing an [`Id`] from one of two value kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegerOrString {
    /// An integer value.
    Integer(i32),
    /// A string value.
    String(String),
}

impl Id {
    /// Builds an [`Id`] from an [`IntegerOrString`].
    // Go: internal/jsonrpc/jsonrpc.go:NewID
    pub fn new(raw: IntegerOrString) -> Id {
        match raw {
            IntegerOrString::String(s) => Id::Str(s),
            IntegerOrString::Integer(i) => Id::Int(i),
        }
    }

    /// Builds a string [`Id`].
    // Go: internal/jsonrpc/jsonrpc.go:NewIDString
    pub fn new_string(s: String) -> Id {
        Id::Str(s)
    }

    /// Builds an integer [`Id`].
    // Go: internal/jsonrpc/jsonrpc.go:NewIDInt
    pub fn new_int(i: i32) -> Id {
        Id::Int(i)
    }

    /// Returns the integer value, or [`None`] for a string ID.
    // Go: internal/jsonrpc/jsonrpc.go:ID.TryInt
    pub fn try_int(&self) -> Option<i32> {
        match self {
            Id::Int(i) => Some(*i),
            Id::Str(_) => None,
        }
    }

    /// Returns the integer value, panicking for a string ID.
    ///
    /// # Panics
    /// Panics with `"ID is not an integer"` if this is a string ID.
    // Go: internal/jsonrpc/jsonrpc.go:ID.MustInt
    pub fn must_int(&self) -> i32 {
        match self {
            Id::Int(i) => *i,
            Id::Str(_) => panic!("ID is not an integer"),
        }
    }
}

impl fmt::Display for Id {
    // Go: internal/jsonrpc/jsonrpc.go:ID.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Id::Str(s) => f.write_str(s),
            Id::Int(i) => write!(f, "{i}"),
        }
    }
}

impl Serialize for Id {
    // Go: internal/jsonrpc/jsonrpc.go:ID.MarshalJSON
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Id::Str(s) => serializer.serialize_str(s),
            Id::Int(i) => serializer.serialize_i32(*i),
        }
    }
}

impl<'de> Deserialize<'de> for Id {
    // Go: internal/jsonrpc/jsonrpc.go:ID.UnmarshalJSON
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(Id::Str(s)),
            serde_json::Value::Number(n) => {
                let i = n
                    .as_i64()
                    .ok_or_else(|| serde::de::Error::custom("invalid ID number"))?;
                Ok(Id::Int(i as i32))
            }
            _ => Err(serde::de::Error::custom("invalid ID")),
        }
    }
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    /// The numeric error code.
    pub code: i32,
    /// A short, human-readable error message.
    pub message: String,
    /// Optional, structured error detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl fmt::Display for ResponseError {
    // Go: internal/jsonrpc/jsonrpc.go:ResponseError.String
    //
    // Go marshals `Data` first and only includes it on a (practically
    // unreachable) marshal failure; the normal path prints `[code]: message`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}]: {}", self.code, self.message)
    }
}

impl std::error::Error for ResponseError {}

/// JSON-RPC parse-error code (`-32700`).
pub const CODE_PARSE_ERROR: i32 = -32700;
/// JSON-RPC invalid-request code (`-32600`).
pub const CODE_INVALID_REQUEST: i32 = -32600;
/// JSON-RPC method-not-found code (`-32601`).
pub const CODE_METHOD_NOT_FOUND: i32 = -32601;
/// JSON-RPC invalid-params code (`-32602`).
pub const CODE_INVALID_PARAMS: i32 = -32602;
/// JSON-RPC internal-error code (`-32603`).
pub const CODE_INTERNAL_ERROR: i32 = -32603;

/// The kind of a JSON-RPC message.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// A notification (method, no ID).
    Notification = 0,
    /// A request (method and ID).
    Request = 1,
    /// A response (ID, no method).
    Response = 2,
}

/// A raw JSON-RPC message that may be a request, notification, or response.
///
/// Unlike a typed LSP message, `params`/`result` stay as raw JSON so callers can
/// decode them based on `method`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Message {
    /// The protocol version (always `"2.0"`).
    pub jsonrpc: JsonRpcVersion,
    /// The message ID (absent for notifications).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// The method name (absent for responses).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub method: String,
    /// Raw request/notification params, decoded by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Box<RawValue>>,
    /// Raw response result, decoded by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Box<RawValue>>,
    /// Response error, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

impl Message {
    /// Returns the [`MessageKind`] of this message.
    // Go: internal/jsonrpc/jsonrpc.go:Message.Kind
    pub fn kind(&self) -> MessageKind {
        if self.id.is_some() && self.method.is_empty() {
            MessageKind::Response
        } else if self.id.is_none() {
            MessageKind::Notification
        } else {
            MessageKind::Request
        }
    }

    /// Reports whether this is a request (has an ID and a method).
    // Go: internal/jsonrpc/jsonrpc.go:Message.IsRequest
    pub fn is_request(&self) -> bool {
        self.id.is_some() && !self.method.is_empty()
    }

    /// Reports whether this is a notification (has a method but no ID).
    // Go: internal/jsonrpc/jsonrpc.go:Message.IsNotification
    pub fn is_notification(&self) -> bool {
        self.id.is_none() && !self.method.is_empty()
    }

    /// Reports whether this is a response (has an ID but no method).
    // Go: internal/jsonrpc/jsonrpc.go:Message.IsResponse
    pub fn is_response(&self) -> bool {
        self.id.is_some() && self.method.is_empty()
    }
}

/// A convenience type for building request/notification messages.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RequestMessage {
    /// The protocol version (always `"2.0"`).
    pub jsonrpc: JsonRpcVersion,
    /// The message ID (absent for notifications).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// The method name.
    pub method: String,
    /// Optional params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A convenience type for building response messages.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ResponseMessage {
    /// The protocol version (always `"2.0"`).
    pub jsonrpc: JsonRpcVersion,
    /// The message ID being responded to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// The successful result, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// The error, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
