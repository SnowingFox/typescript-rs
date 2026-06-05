//! MessagePack tuple protocol types (`protocol_msgpack.go` + `stringer_generated.go`).

use std::fmt;

/// Message kind on the custom msgpack wire format (Go `MessageType uint8`).
///
/// # Examples
/// ```
/// use tsgo_api::MessageType;
/// assert!(MessageType::REQUEST.is_valid());
/// assert!(!MessageType::UNKNOWN.is_valid());
/// assert_eq!(MessageType::REQUEST.to_string(), "MessageTypeRequest");
/// assert_eq!(MessageType(99).to_string(), "MessageType(99)");
/// ```
///
/// Side effects: none (pure).
// Go: internal/api/protocol_msgpack.go:MessageType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageType(pub u8);

impl MessageType {
    /// Unknown / invalid on wire.
    // Go: internal/api/protocol_msgpack.go:MessageTypeUnknown
    pub const UNKNOWN: Self = Self(0);
    /// Incoming client request.
    // Go: internal/api/protocol_msgpack.go:MessageTypeRequest
    pub const REQUEST: Self = Self(1);
    /// Response to a server-initiated `Call`.
    // Go: internal/api/protocol_msgpack.go:MessageTypeCallResponse
    pub const CALL_RESPONSE: Self = Self(2);
    /// Error response to a server-initiated `Call`.
    // Go: internal/api/protocol_msgpack.go:MessageTypeCallError
    pub const CALL_ERROR: Self = Self(3);
    /// Successful response to a client request.
    // Go: internal/api/protocol_msgpack.go:MessageTypeResponse
    pub const RESPONSE: Self = Self(4);
    /// Error response to a client request.
    // Go: internal/api/protocol_msgpack.go:MessageTypeError
    pub const ERROR: Self = Self(5);
    /// Server-initiated call to the client.
    // Go: internal/api/protocol_msgpack.go:MessageTypeCall
    pub const CALL: Self = Self(6);
}

/// Go `stringer` name table for [`MessageType`] (for tests and logging).
// Go: internal/api/stringer_generated.go:_MessageType_name
pub const MESSAGE_TYPE_NAME_TABLE: &[&str] = &[
    "MessageTypeUnknown",
    "MessageTypeRequest",
    "MessageTypeCallResponse",
    "MessageTypeCallError",
    "MessageTypeResponse",
    "MessageTypeError",
    "MessageTypeCall",
];

impl MessageType {
    /// Returns whether `self` is a known on-wire variant (Go `IsValid`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_api::MessageType;
    /// assert!(MessageType::CALL.is_valid());
    /// assert!(!MessageType::UNKNOWN.is_valid());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/api/protocol_msgpack.go:MessageType.IsValid
    pub fn is_valid(self) -> bool {
        (Self::REQUEST.0..=Self::CALL.0).contains(&self.0)
    }

    /// Wire discriminant.
    ///
    /// Side effects: none (pure).
    pub fn as_u8(self) -> u8 {
        self.0
    }
}

impl fmt::Display for MessageType {
    // Go: internal/api/stringer_generated.go:MessageType.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let i = self.0;
        let idx = i as usize;
        if i <= Self::CALL.0 && idx < MESSAGE_TYPE_NAME_TABLE.len() {
            f.write_str(MESSAGE_TYPE_NAME_TABLE[idx])
        } else {
            write!(f, "MessageType({i})")
        }
    }
}

/// MessagePack layout constants (codec deferred; used once `MessagePackProtocol` lands).
// Go: internal/api/protocol_msgpack.go:msgpackFixedArray3
#[allow(dead_code)]
pub const MSGPACK_FIXED_ARRAY3: u8 = 0x93;
// Go: internal/api/protocol_msgpack.go:msgpackBin8
#[allow(dead_code)]
pub const MSGPACK_BIN8: u8 = 0xC4;
// Go: internal/api/protocol_msgpack.go:msgpackBin16
#[allow(dead_code)]
pub const MSGPACK_BIN16: u8 = 0xC5;
// Go: internal/api/protocol_msgpack.go:msgpackBin32
#[allow(dead_code)]
pub const MSGPACK_BIN32: u8 = 0xC6;
// Go: internal/api/protocol_msgpack.go:msgpackU8
#[allow(dead_code)]
pub const MSGPACK_U8: u8 = 0xCC;

#[cfg(test)]
#[path = "protocol_msgpack_test.rs"]
mod tests;
