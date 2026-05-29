use super::*;

// Go: internal/diagnostics/diagnostics_generated.go:keyToMessage
#[test]
fn key_to_message_known_returns_some() {
    let m = key_to_message("Identifier_expected_1003").expect("known key");
    assert_eq!(m.code(), 1003);
}

// Go: internal/diagnostics/diagnostics_generated.go:keyToMessage
#[test]
fn key_to_message_unknown_returns_none() {
    assert!(key_to_message("nope").is_none());
}
