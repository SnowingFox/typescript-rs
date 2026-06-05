use super::*;
use tsgo_json::unmarshal;
use tsgo_lsproto::DocumentUri;

// Go: internal/api/proto_test.go:TestDocumentIdentifierUnmarshalJSON/plain string
#[test]
fn doc_id_plain_string() {
    let d: DocumentIdentifier = unmarshal(br#""foo.ts""#).unwrap();
    assert_eq!(d.file_name, "foo.ts");
    assert_eq!(d.uri, DocumentUri::default());
}

// Go: internal/api/proto_test.go:TestDocumentIdentifierUnmarshalJSON/uri object
#[test]
fn doc_id_uri_object() {
    let d: DocumentIdentifier = unmarshal(br#"{"uri":"file:///foo.ts"}"#).unwrap();
    assert_eq!(d.file_name, "");
    assert_eq!(d.uri, DocumentUri("file:///foo.ts".into()));
}

// Go: internal/api/proto_test.go:TestDocumentIdentifierUnmarshalJSON/uri object with unknown fields
#[test]
fn doc_id_uri_object_unknown_fields() {
    let d: DocumentIdentifier = unmarshal(br#"{"uri":"file:///foo.ts","extra":true}"#).unwrap();
    assert_eq!(d.file_name, "");
    assert_eq!(d.uri, DocumentUri("file:///foo.ts".into()));
}

// Go: internal/api/proto_test.go:TestDocumentIdentifierUnmarshalJSON/empty object
#[test]
fn doc_id_empty_object() {
    let d: DocumentIdentifier = unmarshal(br#"{}"#).unwrap();
    assert_eq!(d.file_name, "");
    assert_eq!(d.uri, DocumentUri::default());
}

// Go: internal/api/proto_test.go:TestDocumentIdentifierUnmarshalJSON/invalid type
#[test]
fn doc_id_invalid_type() {
    let err = unmarshal::<DocumentIdentifier>(b"42").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("expected string or object, got number"),
        "unexpected error: {msg}"
    );
}

// Go: internal/api/proto.go:DocumentIdentifier.ToFileName
#[test]
fn doc_id_to_file_name_prefers_uri() {
    let d = DocumentIdentifier {
        uri: DocumentUri("file:///foo.ts".into()),
        ..Default::default()
    };
    assert_eq!(d.to_file_name(), "/foo.ts");
}

// Go: internal/api/proto.go:DocumentIdentifier.ToFileName
#[test]
fn doc_id_to_file_name_uses_file_name() {
    let d = DocumentIdentifier {
        file_name: "bar.ts".into(),
        ..Default::default()
    };
    assert_eq!(d.to_file_name(), "bar.ts");
}

// Go: internal/api/proto.go:DocumentIdentifier.ToURI
#[test]
fn doc_id_to_uri_returns_existing_uri() {
    let d = DocumentIdentifier {
        uri: DocumentUri("file:///foo.ts".into()),
        ..Default::default()
    };
    assert_eq!(d.to_uri(), DocumentUri("file:///foo.ts".into()));
}

// Go: internal/api/proto.go:DocumentIdentifier.ToURI
#[test]
fn doc_id_to_uri_converts_file_name() {
    let d = DocumentIdentifier {
        file_name: "/path/to/file.ts".into(),
        ..Default::default()
    };
    assert_eq!(d.to_uri(), DocumentUri("file:///path/to/file.ts".into()));
}

// Go: internal/api/proto.go:DocumentIdentifier.ToAbsoluteFileName
#[test]
fn doc_id_to_absolute_file_name_from_uri() {
    let d = DocumentIdentifier {
        uri: DocumentUri("file:///foo.ts".into()),
        ..Default::default()
    };
    assert_eq!(d.to_absolute_file_name("/cwd"), "/foo.ts");
}

// Go: internal/api/proto.go:DocumentIdentifier.ToAbsoluteFileName
#[test]
fn doc_id_to_absolute_file_name_from_relative_file_name() {
    let d = DocumentIdentifier {
        file_name: "bar.ts".into(),
        ..Default::default()
    };
    assert_eq!(d.to_absolute_file_name("/cwd"), "/cwd/bar.ts");
}

// Go: internal/api/proto.go:DocumentIdentifier.String
#[test]
fn doc_id_string_prefers_uri() {
    let d = DocumentIdentifier {
        uri: DocumentUri("file:///foo.ts".into()),
        ..Default::default()
    };
    assert_eq!(d.to_string(), "file:///foo.ts");
}

// Go: internal/api/proto.go:DocumentIdentifier.String
#[test]
fn doc_id_string_uses_file_name() {
    let d = DocumentIdentifier {
        file_name: "bar.ts".into(),
        ..Default::default()
    };
    assert_eq!(d.to_string(), "bar.ts");
}
