use super::{MessageType, MESSAGE_TYPE_NAME_TABLE, MSGPACK_BIN8, MSGPACK_FIXED_ARRAY3};

// Go: internal/api/protocol_msgpack.go:MessageType.IsValid
#[test]
fn message_type_is_valid_matches_go() {
    assert!(!MessageType::UNKNOWN.is_valid());
    assert!(MessageType::REQUEST.is_valid());
    assert!(MessageType::CALL_RESPONSE.is_valid());
    assert!(MessageType::CALL_ERROR.is_valid());
    assert!(MessageType::RESPONSE.is_valid());
    assert!(MessageType::ERROR.is_valid());
    assert!(MessageType::CALL.is_valid());
}

// Go: internal/api/stringer_generated.go:MessageType.String
#[test]
fn message_type_display_matches_stringer() {
    for (i, name) in MESSAGE_TYPE_NAME_TABLE.iter().enumerate() {
        let ty = MessageType(i as u8);
        assert_eq!(ty.to_string(), *name);
    }
    assert_eq!(MessageType(99).to_string(), "MessageType(99)");
}

// Go: internal/api/protocol_msgpack.go — layout constants
#[test]
fn msgpack_constants() {
    assert_eq!(MSGPACK_FIXED_ARRAY3, 0x93);
    assert_eq!(MSGPACK_BIN8, 0xC4);
}
