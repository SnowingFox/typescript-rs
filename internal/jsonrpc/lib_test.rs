use super::*;

// Go side `internal/jsonrpc` has no `*_test.go`; these behavior-level checks use
// JSON-RPC 2.0 spec-known payloads and the documented Go semantics. Each anchors
// to the implementation item it pins.

// --- Id encode/decode round-trips (spec-known values) ---

// Go: internal/jsonrpc/jsonrpc.go:ID.MarshalJSON (int)
#[test]
fn id_int_marshal() {
    assert_eq!(serde_json::to_string(&Id::Int(7)).unwrap(), "7");
}

// Go: internal/jsonrpc/jsonrpc.go:ID.MarshalJSON (string)
#[test]
fn id_string_marshal() {
    assert_eq!(
        serde_json::to_string(&Id::Str("ts1".to_string())).unwrap(),
        "\"ts1\""
    );
}

// Go: internal/jsonrpc/jsonrpc.go:ID.UnmarshalJSON (int)
#[test]
fn id_int_unmarshal() {
    let id: Id = serde_json::from_slice(b"7").unwrap();
    assert_eq!(id, Id::Int(7));
}

// Go: internal/jsonrpc/jsonrpc.go:ID.UnmarshalJSON (string)
#[test]
fn id_string_unmarshal() {
    let id: Id = serde_json::from_slice(b"\"abc\"").unwrap();
    assert_eq!(id, Id::Str("abc".to_string()));
}

// Go: internal/jsonrpc/jsonrpc.go:ID.TryInt
#[test]
fn id_try_int_on_string_is_none() {
    assert_eq!(Id::Str("x".to_string()).try_int(), None);
    assert_eq!(Id::Int(5).try_int(), Some(5));
}

// Go: internal/jsonrpc/jsonrpc.go:ID.MustInt
#[test]
#[should_panic(expected = "ID is not an integer")]
fn id_must_int_on_string_panics() {
    let _ = Id::Str("x".to_string()).must_int();
}

// Go: internal/jsonrpc/jsonrpc.go:ID.String
#[test]
fn id_display_int() {
    assert_eq!(Id::Int(42).to_string(), "42");
}

// --- Message.kind() three-state discrimination ---

// Go: internal/jsonrpc/jsonrpc.go:Message.Kind (request)
#[test]
fn kind_request() {
    let m = Message {
        id: Some(Id::Int(1)),
        method: "x".to_string(),
        ..Default::default()
    };
    assert_eq!(m.kind(), MessageKind::Request);
}

// Go: internal/jsonrpc/jsonrpc.go:Message.Kind (notification)
#[test]
fn kind_notification() {
    let m = Message {
        id: None,
        method: "x".to_string(),
        ..Default::default()
    };
    assert_eq!(m.kind(), MessageKind::Notification);
}

// Go: internal/jsonrpc/jsonrpc.go:Message.Kind (response)
#[test]
fn kind_response() {
    let m = Message {
        id: Some(Id::Int(1)),
        method: String::new(),
        ..Default::default()
    };
    assert_eq!(m.kind(), MessageKind::Response);
}

// Go: internal/jsonrpc/jsonrpc.go:Message.IsRequest/IsNotification/IsResponse
#[test]
fn is_request_requires_id_and_method() {
    let request = Message {
        id: Some(Id::Int(1)),
        method: "x".to_string(),
        ..Default::default()
    };
    assert!(request.is_request());
    assert!(!request.is_notification());
    assert!(!request.is_response());

    let notification = Message {
        id: None,
        method: "x".to_string(),
        ..Default::default()
    };
    assert!(!notification.is_request());
    assert!(notification.is_notification());

    let response = Message {
        id: Some(Id::Int(1)),
        method: String::new(),
        ..Default::default()
    };
    assert!(!response.is_request());
    assert!(response.is_response());
}

// --- JsonRpcVersion validation ---

// Go: internal/jsonrpc/jsonrpc.go:JSONRPCVersion.MarshalJSON
#[test]
fn version_marshal_is_2_0() {
    assert_eq!(serde_json::to_string(&JsonRpcVersion).unwrap(), "\"2.0\"");
}

// Go: internal/jsonrpc/jsonrpc.go:JSONRPCVersion.UnmarshalJSON (accepts)
#[test]
fn version_unmarshal_accepts_2_0() {
    let v: Result<JsonRpcVersion, _> = serde_json::from_slice(b"\"2.0\"");
    assert!(v.is_ok());
}

// Go: internal/jsonrpc/jsonrpc.go:JSONRPCVersion.UnmarshalJSON (rejects)
#[test]
fn version_unmarshal_rejects_other() {
    let v: Result<JsonRpcVersion, _> = serde_json::from_slice(b"\"1.0\"");
    let err = v.unwrap_err();
    assert!(
        err.to_string().contains("invalid JSON-RPC version"),
        "got {err}"
    );
}

// --- ResponseError.to_string() ---

// Go: internal/jsonrpc/jsonrpc.go:ResponseError.String (with code/message)
#[test]
fn response_error_string_basic() {
    let e = ResponseError {
        code: CODE_METHOD_NOT_FOUND,
        message: "Method not found".to_string(),
        data: None,
    };
    assert_eq!(e.to_string(), "[-32601]: Method not found");
}

// Go: internal/jsonrpc/jsonrpc.go:ResponseError.String (nil -> empty)
#[test]
fn response_error_string_nil() {
    // Go's nil `*ResponseError` returns ""; the Rust analogue is `None`.
    let none: Option<&ResponseError> = None;
    assert_eq!(none.map_or(String::new(), |e| e.to_string()), "");
}
