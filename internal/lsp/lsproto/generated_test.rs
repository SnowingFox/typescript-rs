use super::*;
use crate::URI;

// === Union types: IntegerOrString / IntegerOrNull / DocumentUriOrNull ===

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypes/IntegerOrString with integer
#[test]
fn union_integer_or_string_int() {
    let v: IntegerOrString = serde_json::from_str("42").unwrap();
    assert_eq!(v.integer, Some(42));
    assert_eq!(v.string, None);
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypes/IntegerOrString with string
#[test]
fn union_integer_or_string_str() {
    let v: IntegerOrString = serde_json::from_str("\"hello\"").unwrap();
    assert_eq!(v.string.as_deref(), Some("hello"));
    assert_eq!(v.integer, None);
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypes/IntegerOrNull with integer
#[test]
fn union_integer_or_null_int() {
    let v: IntegerOrNull = serde_json::from_str("42").unwrap();
    assert_eq!(v.integer, Some(42));
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypes/IntegerOrNull with null
#[test]
fn union_integer_or_null_null() {
    let v: IntegerOrNull = serde_json::from_str("null").unwrap();
    assert_eq!(v.integer, None);
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypes/DocumentUriOrNull with string
#[test]
fn union_document_uri_or_null_str() {
    let v: DocumentUriOrNull = serde_json::from_str("\"file:///test.ts\"").unwrap();
    assert_eq!(
        v.document_uri,
        Some(DocumentUri("file:///test.ts".to_string()))
    );
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypes/DocumentUriOrNull with null
#[test]
fn union_document_uri_or_null_null() {
    let v: DocumentUriOrNull = serde_json::from_str("null").unwrap();
    assert_eq!(v.document_uri, None);
}

// Go: lsp/lsproto/lsp_json_test.go:TestMarshalUnionTypes/IntegerOrNull with value
#[test]
fn marshal_integer_or_null_value() {
    let v = IntegerOrNull { integer: Some(42) };
    assert_eq!(serde_json::to_string(&v).unwrap(), "42");
}

// Go: lsp/lsproto/lsp_json_test.go:TestMarshalUnionTypes/IntegerOrNull with null
#[test]
fn marshal_integer_or_null_null() {
    let v = IntegerOrNull { integer: None };
    assert_eq!(serde_json::to_string(&v).unwrap(), "null");
}

// Go: lsp/lsproto/lsp_json_test.go:TestMarshalUnionTypes/IntegerOrString with integer
#[test]
fn marshal_integer_or_string_int() {
    let v = IntegerOrString {
        integer: Some(7),
        string: None,
    };
    assert_eq!(serde_json::to_string(&v).unwrap(), "7");
}

// Go: lsp/lsproto/lsp_json_test.go:TestMarshalUnionTypes/IntegerOrString with string
#[test]
fn marshal_integer_or_string_str() {
    let v = IntegerOrString {
        integer: None,
        string: Some("tok".to_string()),
    };
    assert_eq!(serde_json::to_string(&v).unwrap(), "\"tok\"");
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypeWrongKind/IntegerOrString rejects boolean
#[test]
fn union_wrong_int_or_string_bool() {
    assert!(serde_json::from_str::<IntegerOrString>("true").is_err());
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypeWrongKind/IntegerOrString rejects null
#[test]
fn union_wrong_int_or_string_null() {
    assert!(serde_json::from_str::<IntegerOrString>("null").is_err());
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypeWrongKind/IntegerOrString rejects object
#[test]
fn union_wrong_int_or_string_object() {
    assert!(serde_json::from_str::<IntegerOrString>("{}").is_err());
}

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalUnionTypeWrongKind/IntegerOrString rejects array
#[test]
fn union_wrong_int_or_string_array() {
    assert!(serde_json::from_str::<IntegerOrString>("[]").is_err());
}

// === Core structs: Location / InlayHint / FoldingRange ===

// --- TestUnmarshalRejectsNullForOptionalNonNullableFields ---

// Go: .../TestUnmarshalRejectsNullForOptionalNonNullableFields/InlayHint kind null
#[test]
fn null_rejected_inlayhint_kind() {
    let err = serde_json::from_str::<InlayHint>(
        r#"{"position": {"line": 0, "character": 0}, "label": "foo", "kind": null}"#,
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains(r#"null value is not allowed for field "kind""#));
}

// Go: .../InlayHint textEdits null
#[test]
fn null_rejected_inlayhint_text_edits() {
    let err = serde_json::from_str::<InlayHint>(
        r#"{"position": {"line": 0, "character": 0}, "label": "foo", "textEdits": null}"#,
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains(r#"null value is not allowed for field "textEdits""#));
}

// Go: .../InlayHint paddingLeft null
#[test]
fn null_rejected_inlayhint_padding_left() {
    let err = serde_json::from_str::<InlayHint>(
        r#"{"position": {"line": 0, "character": 0}, "label": "foo", "paddingLeft": null}"#,
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains(r#"null value is not allowed for field "paddingLeft""#));
}

// Go: .../FoldingRange kind null
#[test]
fn null_rejected_foldingrange_kind() {
    let err =
        serde_json::from_str::<FoldingRange>(r#"{"startLine": 0, "endLine": 10, "kind": null}"#)
            .unwrap_err();
    assert!(err
        .to_string()
        .contains(r#"null value is not allowed for field "kind""#));
}

// Go: .../FoldingRange startCharacter null
#[test]
fn null_rejected_foldingrange_start_character() {
    let err = serde_json::from_str::<FoldingRange>(
        r#"{"startLine": 0, "endLine": 10, "startCharacter": null}"#,
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains(r#"null value is not allowed for field "startCharacter""#));
}

// --- TestUnmarshalAcceptsOmittedOptionalFields ---

// Go: .../TestUnmarshalAcceptsOmittedOptionalFields/InlayHint with only required fields
#[test]
fn omitted_optional_inlayhint() {
    let hint: InlayHint =
        serde_json::from_str(r#"{"position": {"line": 1, "character": 5}, "label": "test"}"#)
            .unwrap();
    assert!(hint.kind.is_none());
    assert!(hint.text_edits.is_none());
    assert!(hint.tooltip.is_none());
    assert!(hint.padding_left.is_none());
    assert!(hint.padding_right.is_none());
    assert!(hint.data.is_none());
    assert_eq!(hint.position.line, 1);
    assert_eq!(hint.position.character, 5);
}

// Go: .../FoldingRange with only required fields
#[test]
fn omitted_optional_foldingrange() {
    let fr: FoldingRange = serde_json::from_str(r#"{"startLine": 5, "endLine": 10}"#).unwrap();
    assert!(fr.kind.is_none());
    assert!(fr.start_character.is_none());
    assert!(fr.end_character.is_none());
    assert!(fr.collapsed_text.is_none());
    assert_eq!(fr.start_line, 5);
    assert_eq!(fr.end_line, 10);
}

// --- TestUnmarshalRejectsIncompleteObjects ---

// Go: .../TestUnmarshalRejectsIncompleteObjects/InlayHint missing position
#[test]
fn incomplete_inlayhint_missing_position() {
    let err = serde_json::from_str::<InlayHint>(r#"{"label": "test"}"#).unwrap_err();
    assert!(err
        .to_string()
        .contains("missing required properties: position"));
}

// Go: .../InlayHint missing label
#[test]
fn incomplete_inlayhint_missing_label() {
    let err = serde_json::from_str::<InlayHint>(r#"{"position": {"line": 0, "character": 0}}"#)
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("missing required properties: label"));
}

// Go: .../Location missing uri
#[test]
fn incomplete_location_missing_uri() {
    let err = serde_json::from_str::<Location>(
        r#"{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}}"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("missing required properties: uri"));
}

// Go: .../Location empty object
#[test]
fn incomplete_location_empty() {
    let err = serde_json::from_str::<Location>(r#"{}"#).unwrap_err();
    assert!(err
        .to_string()
        .contains("missing required properties: uri, range"));
}

// --- TestMarshalUnmarshalRoundTrip (subset for these types) ---

// Go: .../TestMarshalUnmarshalRoundTrip/InlayHint with kind
#[test]
fn roundtrip_inlayhint_with_kind() {
    let v = InlayHint {
        position: Position {
            line: 1,
            character: 5,
        },
        label: StringOrInlayHintLabelParts {
            string: Some("param".to_string()),
            inlay_hint_label_parts: None,
        },
        kind: Some(InlayHintKind::PARAMETER),
        ..Default::default()
    };
    let data = serde_json::to_vec(&v).unwrap();
    let result: InlayHint = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// Go: .../InlayHint minimal
#[test]
fn roundtrip_inlayhint_minimal() {
    let v = InlayHint {
        position: Position {
            line: 0,
            character: 0,
        },
        label: StringOrInlayHintLabelParts {
            string: Some("x".to_string()),
            inlay_hint_label_parts: None,
        },
        ..Default::default()
    };
    let data = serde_json::to_vec(&v).unwrap();
    let result: InlayHint = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// Go: .../FoldingRange with all fields
#[test]
fn roundtrip_foldingrange_all() {
    let v = FoldingRange {
        start_line: 1,
        start_character: Some(0),
        end_line: 10,
        end_character: Some(5),
        kind: Some(FoldingRangeKind::region()),
        collapsed_text: Some("...".to_string()),
    };
    let data = serde_json::to_vec(&v).unwrap();
    let result: FoldingRange = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// Go: lsp/lsproto/lsp_generated.go:SelectionRange (nested parent chain)
#[test]
fn roundtrip_selection_range_nested() {
    let rng = |sl, sc, el, ec| Range {
        start: Position {
            line: sl,
            character: sc,
        },
        end: Position {
            line: el,
            character: ec,
        },
    };
    let v = SelectionRange {
        range: rng(0, 1, 0, 2),
        parent: Some(Box::new(SelectionRange {
            range: rng(0, 0, 0, 5),
            parent: None,
        })),
    };
    let data = serde_json::to_vec(&v).unwrap();
    let result: SelectionRange = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// Go: lsp/lsproto/lsp_generated.go:SelectionRange (parent omitted when absent)
#[test]
fn roundtrip_selection_range_no_parent() {
    let v = SelectionRange {
        range: Range {
            start: Position {
                line: 2,
                character: 3,
            },
            end: Position {
                line: 2,
                character: 7,
            },
        },
        parent: None,
    };
    let data = serde_json::to_vec(&v).unwrap();
    // `parent` is omitted when `None` (Go `omitzero`).
    assert!(!String::from_utf8(data.clone()).unwrap().contains("parent"));
    let result: SelectionRange = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// Go: lsp/lsproto/lsp_generated.go:LinkedEditingRanges (ranges + word pattern)
#[test]
fn roundtrip_linked_editing_ranges_with_word_pattern() {
    let rng = |sl, sc, el, ec| Range {
        start: Position {
            line: sl,
            character: sc,
        },
        end: Position {
            line: el,
            character: ec,
        },
    };
    let v = LinkedEditingRanges {
        ranges: vec![rng(0, 1, 0, 4), rng(0, 7, 0, 10)],
        word_pattern: Some(r"[a-zA-Z0-9:\-\._$]*".to_string()),
    };
    let data = serde_json::to_vec(&v).unwrap();
    let result: LinkedEditingRanges = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// Go: lsp/lsproto/lsp_generated.go:LinkedEditingRanges (wordPattern omitted when absent)
#[test]
fn roundtrip_linked_editing_ranges_no_word_pattern() {
    let v = LinkedEditingRanges {
        ranges: vec![Range {
            start: Position {
                line: 1,
                character: 2,
            },
            end: Position {
                line: 1,
                character: 5,
            },
        }],
        word_pattern: None,
    };
    let data = serde_json::to_vec(&v).unwrap();
    // `wordPattern` is omitted when `None` (Go `omitzero`).
    assert!(!String::from_utf8(data.clone())
        .unwrap()
        .contains("wordPattern"));
    let result: LinkedEditingRanges = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// Go: .../Location
#[test]
fn roundtrip_location() {
    let v = Location {
        uri: DocumentUri("file:///test.ts".to_string()),
        range: Range {
            start: Position {
                line: 1,
                character: 2,
            },
            end: Position {
                line: 3,
                character: 4,
            },
        },
    };
    let data = serde_json::to_vec(&v).unwrap();
    let result: Location = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// --- TestUnmarshalIgnoresUnknownFields ---

// Go: .../TestUnmarshalIgnoresUnknownFields/Location with extra fields
#[test]
fn ignore_unknown_location() {
    let loc: Location = serde_json::from_str(
        r#"{
            "uri": "file:///test.ts",
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
            "someUnknownField": 42,
            "anotherUnknown": {"nested": true}
        }"#,
    )
    .unwrap();
    assert_eq!(loc.uri, DocumentUri("file:///test.ts".to_string()));
}

// Go: .../InlayHint with extra fields
#[test]
fn ignore_unknown_inlayhint() {
    let _hint: InlayHint = serde_json::from_str(
        r#"{
            "position": {"line": 0, "character": 0},
            "label": "x",
            "futureField": [1, 2, 3]
        }"#,
    )
    .unwrap();
}

// --- TestUnmarshalRejectsWrongTypes ---

// Go: .../TestUnmarshalRejectsWrongTypes/Location receives array
#[test]
fn wrong_type_location_array() {
    assert!(serde_json::from_str::<Location>("[]").is_err());
}

// Go: .../Location receives string
#[test]
fn wrong_type_location_string() {
    assert!(serde_json::from_str::<Location>(r#""not an object""#).is_err());
}

// Go: .../Location receives number
#[test]
fn wrong_type_location_number() {
    assert!(serde_json::from_str::<Location>("42").is_err());
}

// Go: .../Location receives null
#[test]
fn wrong_type_location_null() {
    assert!(serde_json::from_str::<Location>("null").is_err());
}

// Go: .../FoldingRange receives boolean
#[test]
fn wrong_type_foldingrange_bool() {
    assert!(serde_json::from_str::<FoldingRange>("true").is_err());
}

// --- TestMarshalOmitsZeroOptionalFields ---

// Go: .../TestMarshalOmitsZeroOptionalFields/InlayHint omits nil fields
#[test]
fn marshal_omits_inlayhint() {
    let hint = InlayHint {
        position: Position {
            line: 0,
            character: 0,
        },
        label: StringOrInlayHintLabelParts {
            string: Some("x".to_string()),
            inlay_hint_label_parts: None,
        },
        ..Default::default()
    };
    let s = serde_json::to_string(&hint).unwrap();
    assert!(!s.contains("kind"), "got: {s}");
    assert!(!s.contains("textEdits"), "got: {s}");
    assert!(!s.contains("paddingLeft"), "got: {s}");
    assert!(s.contains("position"), "got: {s}");
    assert!(s.contains("label"), "got: {s}");
}

// Go: .../FoldingRange omits nil optional fields
#[test]
fn marshal_omits_foldingrange() {
    let fr = FoldingRange {
        start_line: 1,
        end_line: 10,
        ..Default::default()
    };
    let s = serde_json::to_string(&fr).unwrap();
    assert!(!s.contains("kind"), "got: {s}");
    assert!(!s.contains("startCharacter"), "got: {s}");
    assert!(s.contains("startLine"), "got: {s}");
    assert!(s.contains("endLine"), "got: {s}");
}

// --- TestUnmarshalFieldOrdering ---

// Go: .../TestUnmarshalFieldOrdering/Location with reversed field order
#[test]
fn field_order_location_reversed() {
    let loc: Location = serde_json::from_str(
        r#"{
            "range": {"start": {"line": 1, "character": 2}, "end": {"line": 3, "character": 4}},
            "uri": "file:///test.ts"
        }"#,
    )
    .unwrap();
    assert_eq!(loc.uri, DocumentUri("file:///test.ts".to_string()));
    assert_eq!(loc.range.start.line, 1);
}

// Go: .../InlayHint with kind before label
#[test]
fn field_order_inlayhint_kind_first() {
    let hint: InlayHint = serde_json::from_str(
        r#"{
            "kind": 1,
            "label": "x",
            "position": {"line": 0, "character": 0}
        }"#,
    )
    .unwrap();
    assert_eq!(hint.kind, Some(InlayHintKind::TYPE));
}

// --- TestUnmarshalStringOrArrayUnion ---

// Go: .../TestUnmarshalStringOrArrayUnion/StringOrInlayHintLabelParts with string
#[test]
fn string_or_array_string() {
    let v: StringOrInlayHintLabelParts = serde_json::from_str(r#""hello""#).unwrap();
    assert_eq!(v.string.as_deref(), Some("hello"));
    assert!(v.inlay_hint_label_parts.is_none());
}

// Go: .../StringOrInlayHintLabelParts with array
#[test]
fn string_or_array_array() {
    let v: StringOrInlayHintLabelParts =
        serde_json::from_str(r#"[{"value": "param"}, {"value": ": "}, {"value": "string"}]"#)
            .unwrap();
    assert!(v.string.is_none());
    let parts = v.inlay_hint_label_parts.as_ref().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].value, "param");
}

// Go: .../TestUnmarshalUnionTypeWrongKind/StringOrInlayHintLabelParts rejects number
#[test]
fn union_wrong_string_or_parts_number() {
    assert!(serde_json::from_str::<StringOrInlayHintLabelParts>("42").is_err());
}

// Go: .../StringOrInlayHintLabelParts rejects boolean
#[test]
fn union_wrong_string_or_parts_bool() {
    assert!(serde_json::from_str::<StringOrInlayHintLabelParts>("true").is_err());
}

// --- TestEnumStringValues ---

// Go: .../TestEnumStringValues/InlayHintKind values
#[test]
fn enum_inlayhintkind_strings() {
    assert_eq!(InlayHintKind::TYPE.to_string(), "Type");
    assert_eq!(InlayHintKind::PARAMETER.to_string(), "Parameter");
}

// Go: .../unknown enum value
#[test]
fn enum_unknown_value() {
    assert!(InlayHintKind(999).to_string().contains("999"));
}

// Go: .../TestEnumStringValues/SymbolKind values
#[test]
fn enum_symbolkind_strings() {
    assert_eq!(SymbolKind::FILE.to_string(), "File");
    assert_eq!(SymbolKind::FUNCTION.to_string(), "Function");
    assert_eq!(SymbolKind::VARIABLE.to_string(), "Variable");
}

// === String enums: PositionEncodingKind / LanguageKind ===

// Go: lsp_generated.go:PositionEncodingKind (predefined values + JSON string round-trip)
#[test]
fn position_encoding_kind_consts_and_round_trip() {
    assert_eq!(PositionEncodingKind::UTF8.0, "utf-8");
    assert_eq!(PositionEncodingKind::UTF16.0, "utf-16");
    assert_eq!(PositionEncodingKind::UTF32.0, "utf-32");
    assert_eq!(
        serde_json::to_string(&PositionEncodingKind::UTF16).unwrap(),
        "\"utf-16\""
    );
    let k: PositionEncodingKind = serde_json::from_str("\"utf-8\"").unwrap();
    assert_eq!(k, PositionEncodingKind::UTF8);
}

// Go: lsp_generated.go:LanguageKind (predefined values + JSON string round-trip)
#[test]
fn language_kind_consts_and_round_trip() {
    assert_eq!(LanguageKind::TYPE_SCRIPT.0, "typescript");
    assert_eq!(LanguageKind::TYPE_SCRIPT_REACT.0, "typescriptreact");
    assert_eq!(LanguageKind::JAVA_SCRIPT.0, "javascript");
    assert_eq!(LanguageKind::JAVA_SCRIPT_REACT.0, "javascriptreact");
    assert_eq!(LanguageKind::JSON.0, "json");
    // Go aliases several names onto a shared value.
    assert_eq!(LanguageKind::DELPHI.0, "pascal");
    assert_eq!(LanguageKind::PASCAL.0, "pascal");
    assert_eq!(LanguageKind::PUG.0, "jade");
    assert_eq!(LanguageKind::GIT_REBASE.0, "rebase");
    assert_eq!(LanguageKind::VISUAL_BASIC.0, "vb");
    // serde round-trips as a plain JSON string, including unknown ids.
    assert_eq!(
        serde_json::to_string(&LanguageKind::RUST).unwrap(),
        "\"rust\""
    );
    let k: LanguageKind = serde_json::from_str("\"typescript\"").unwrap();
    assert_eq!(k, LanguageKind::TYPE_SCRIPT);
    let other: LanguageKind = serde_json::from_str("\"made-up\"").unwrap();
    assert_eq!(other.0, "made-up");
}

// === Text document content changes ===

// Go: lsp_generated.go:TextDocumentContentChangePartial (round-trip with all fields)
#[test]
fn text_change_partial_round_trip() {
    let v = TextDocumentContentChangePartial {
        range: Range {
            start: Position {
                line: 1,
                character: 2,
            },
            end: Position {
                line: 3,
                character: 4,
            },
        },
        range_length: Some(7),
        text: "hi".to_string(),
    };
    let data = serde_json::to_vec(&v).unwrap();
    let got: TextDocumentContentChangePartial = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, got);
}

// Go: .../TextDocumentContentChangePartial rejects null rangeLength
#[test]
fn text_change_partial_rejects_null_range_length() {
    assert_null_rejected::<TextDocumentContentChangePartial>(
        r#"{"range": {"start": {"line":0,"character":0}, "end": {"line":0,"character":0}}, "text": "", "rangeLength": null}"#,
        "rangeLength",
    );
}

// Go: .../TextDocumentContentChangePartial missing required text
#[test]
fn text_change_partial_missing_text() {
    let err = serde_json::from_str::<TextDocumentContentChangePartial>(
        r#"{"range": {"start": {"line":0,"character":0}, "end": {"line":0,"character":0}}}"#,
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("missing required properties: text"));
}

// Go: lsp_generated.go:TextDocumentContentChangeWholeDocument (round-trip)
#[test]
fn text_change_whole_round_trip() {
    let v = TextDocumentContentChangeWholeDocument {
        text: "whole".to_string(),
    };
    let got: TextDocumentContentChangeWholeDocument =
        serde_json::from_str(r#"{"text":"whole"}"#).unwrap();
    assert_eq!(v, got);
    assert_eq!(serde_json::to_string(&v).unwrap(), r#"{"text":"whole"}"#);
}

// Go: .../TextDocumentContentChangePartialOrWholeDocument (range present -> partial)
#[test]
fn text_change_union_partial_via_range() {
    let v: TextDocumentContentChangePartialOrWholeDocument = serde_json::from_str(
        r#"{"range": {"start": {"line":0,"character":0}, "end": {"line":0,"character":1}}, "text": "x"}"#,
    )
    .unwrap();
    assert!(v.partial.is_some());
    assert!(v.whole_document.is_none());
    assert_eq!(v.partial.unwrap().text, "x");
}

// Go: .../TextDocumentContentChangePartialOrWholeDocument (no range -> whole document)
#[test]
fn text_change_union_whole_without_range() {
    let v: TextDocumentContentChangePartialOrWholeDocument =
        serde_json::from_str(r#"{"text": "whole"}"#).unwrap();
    assert!(v.partial.is_none());
    assert!(v.whole_document.is_some());
    assert_eq!(v.whole_document.unwrap().text, "whole");
}

// Go: .../TextDocumentContentChangePartialOrWholeDocument marshal whole document
#[test]
fn text_change_union_marshal_whole() {
    let v = TextDocumentContentChangePartialOrWholeDocument {
        partial: None,
        whole_document: Some(TextDocumentContentChangeWholeDocument {
            text: "w".to_string(),
        }),
    };
    assert_eq!(serde_json::to_string(&v).unwrap(), r#"{"text":"w"}"#);
}

// === Diagnostics ===

// Go: lsp_generated.go:DiagnosticSeverity (String() values + JSON integer)
#[test]
fn diagnostic_severity_display_and_serde() {
    assert_eq!(DiagnosticSeverity::ERROR.to_string(), "Error");
    assert_eq!(DiagnosticSeverity::WARNING.to_string(), "Warning");
    assert_eq!(DiagnosticSeverity::INFORMATION.to_string(), "Information");
    assert_eq!(DiagnosticSeverity::HINT.to_string(), "Hint");
    assert!(DiagnosticSeverity(99).to_string().contains("99"));
    assert_eq!(
        serde_json::to_string(&DiagnosticSeverity::WARNING).unwrap(),
        "2"
    );
    let s: DiagnosticSeverity = serde_json::from_str("4").unwrap();
    assert_eq!(s, DiagnosticSeverity::HINT);
}

// Go: lsp_generated.go:DiagnosticTag (String() values + JSON integer)
#[test]
fn diagnostic_tag_display_and_serde() {
    assert_eq!(DiagnosticTag::UNNECESSARY.to_string(), "Unnecessary");
    assert_eq!(DiagnosticTag::DEPRECATED.to_string(), "Deprecated");
    assert!(DiagnosticTag(0).to_string().contains('0'));
    assert_eq!(
        serde_json::to_string(&DiagnosticTag::DEPRECATED).unwrap(),
        "2"
    );
}

// Go: lsp_generated.go:CodeDescription (round-trip)
#[test]
fn code_description_round_trip() {
    let v: CodeDescription = serde_json::from_str(r#"{"href":"https://x/y"}"#).unwrap();
    assert_eq!(v.href, URI("https://x/y".to_string()));
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"href":"https://x/y"}"#
    );
}

// Go: .../CodeDescription missing required href
#[test]
fn code_description_missing_href() {
    let err = serde_json::from_str::<CodeDescription>("{}").unwrap_err();
    assert!(err
        .to_string()
        .contains("missing required properties: href"));
}

// Go: lsp_generated.go:DiagnosticRelatedInformation (round-trip)
#[test]
fn diagnostic_related_information_round_trip() {
    let v = DiagnosticRelatedInformation {
        location: Location {
            uri: DocumentUri("file:///a.ts".to_string()),
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
        },
        message: "see here".to_string(),
    };
    let data = serde_json::to_vec(&v).unwrap();
    let got: DiagnosticRelatedInformation = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, got);
}

// Go: lsp_generated.go:DiagnosticData (empty placeholder object)
#[test]
fn diagnostic_data_empty_object() {
    let _v: DiagnosticData = serde_json::from_str("{}").unwrap();
    assert_eq!(serde_json::to_string(&DiagnosticData).unwrap(), "{}");
}

// Go: lsp_generated.go:Diagnostic (round-trip with severity/code/tags/related)
#[test]
fn diagnostic_round_trip() {
    let v = Diagnostic {
        range: Range {
            start: Position {
                line: 1,
                character: 2,
            },
            end: Position {
                line: 1,
                character: 6,
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(IntegerOrString {
            integer: Some(2304),
            string: None,
        }),
        source: Some("ts".to_string()),
        message: "Cannot find name".to_string(),
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        related_information: Some(vec![DiagnosticRelatedInformation {
            location: Location {
                uri: DocumentUri("file:///b.ts".to_string()),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                },
            },
            message: "related".to_string(),
        }]),
        ..Default::default()
    };
    let data = serde_json::to_vec(&v).unwrap();
    let got: Diagnostic = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, got);
}

// Go: .../Diagnostic missing required range+message
#[test]
fn diagnostic_missing_required() {
    let err = serde_json::from_str::<Diagnostic>("{}").unwrap_err();
    assert!(err
        .to_string()
        .contains("missing required properties: range, message"));
}

// Go: .../Diagnostic rejects null severity
#[test]
fn diagnostic_rejects_null_severity() {
    assert_null_rejected::<Diagnostic>(
        r#"{"range": {"start":{"line":0,"character":0},"end":{"line":0,"character":0}}, "message": "", "severity": null}"#,
        "severity",
    );
}

// === Boolean union: BooleanOrHoverOptions ===

// Go: .../TestUnmarshalBooleanUnionTypes/BooleanOrHoverOptions with true
#[test]
fn bool_union_true() {
    let v: BooleanOrHoverOptions = serde_json::from_str("true").unwrap();
    assert_eq!(v.boolean, Some(true));
    assert!(v.hover_options.is_none());
}

// Go: .../BooleanOrHoverOptions with false
#[test]
fn bool_union_false() {
    let v: BooleanOrHoverOptions = serde_json::from_str("false").unwrap();
    assert_eq!(v.boolean, Some(false));
    assert!(v.hover_options.is_none());
}

// Go: .../BooleanOrHoverOptions with object
#[test]
fn bool_union_object() {
    let v: BooleanOrHoverOptions = serde_json::from_str("{}").unwrap();
    assert!(v.boolean.is_none());
    assert!(v.hover_options.is_some());
}

// Go: .../BooleanOrHoverOptions rejects string
#[test]
fn bool_union_rejects_string() {
    assert!(serde_json::from_str::<BooleanOrHoverOptions>(r#""nope""#).is_err());
}

// === Discriminator union: WorkDoneProgressBeginOrReportOrEnd ===

// Go: .../TestUnmarshalDiscriminatorUnion/WorkDoneProgressBegin
#[test]
fn discriminator_begin() {
    let v: WorkDoneProgressBeginOrReportOrEnd =
        serde_json::from_str(r#"{"kind": "begin", "title": "Indexing"}"#).unwrap();
    assert!(v.begin.is_some());
    assert!(v.report.is_none());
    assert!(v.end.is_none());
    assert_eq!(v.begin.unwrap().title, "Indexing");
}

// Go: .../WorkDoneProgressReport
#[test]
fn discriminator_report() {
    let v: WorkDoneProgressBeginOrReportOrEnd =
        serde_json::from_str(r#"{"kind": "report", "message": "50%"}"#).unwrap();
    assert!(v.begin.is_none());
    assert!(v.report.is_some());
    assert!(v.end.is_none());
    assert_eq!(v.report.unwrap().message.as_deref(), Some("50%"));
}

// Go: .../WorkDoneProgressEnd
#[test]
fn discriminator_end() {
    let v: WorkDoneProgressBeginOrReportOrEnd = serde_json::from_str(r#"{"kind": "end"}"#).unwrap();
    assert!(v.begin.is_none());
    assert!(v.report.is_none());
    assert!(v.end.is_some());
}

// Go: .../invalid discriminator
#[test]
fn discriminator_invalid() {
    assert!(
        serde_json::from_str::<WorkDoneProgressBeginOrReportOrEnd>(r#"{"kind": "invalid"}"#)
            .is_err()
    );
}

// === Presence discriminator union: TextEditOrInsertReplaceEdit ===

// Go: .../TestUnmarshalPresenceDiscriminatorUnion/TextEdit via range field
#[test]
fn presence_text_edit_via_range() {
    let v: TextEditOrInsertReplaceEdit = serde_json::from_str(
        r#"{
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "newText": "x"
        }"#,
    )
    .unwrap();
    assert!(v.text_edit.is_some());
    assert!(v.insert_replace_edit.is_none());
    assert_eq!(v.text_edit.unwrap().new_text, "x");
}

// Go: .../InsertReplaceEdit via insert field
#[test]
fn presence_insert_replace_via_insert() {
    let v: TextEditOrInsertReplaceEdit = serde_json::from_str(
        r#"{
            "insert": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "replace": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 2}},
            "newText": "y"
        }"#,
    )
    .unwrap();
    assert!(v.text_edit.is_none());
    assert!(v.insert_replace_edit.is_some());
    assert_eq!(v.insert_replace_edit.unwrap().new_text, "y");
}

// === Document edit union: TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile ===

// Go: .../TestUnmarshalDocumentEditUnion/TextDocumentEdit without kind
#[test]
fn doc_edit_text_document_edit() {
    let v: TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile = serde_json::from_str(
        r#"{
            "textDocument": {"uri": "file:///a.ts", "version": 1},
            "edits": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "x"}]
        }"#,
    )
    .unwrap();
    assert!(v.text_document_edit.is_some());
    assert!(v.create_file.is_none());
    assert!(v.rename_file.is_none());
    assert!(v.delete_file.is_none());
}

// Go: .../CreateFile with kind create
#[test]
fn doc_edit_create() {
    let v: TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile =
        serde_json::from_str(r#"{"kind": "create", "uri": "file:///new.ts"}"#).unwrap();
    assert!(v.text_document_edit.is_none());
    assert!(v.create_file.is_some());
    assert_eq!(
        v.create_file.unwrap().uri,
        DocumentUri("file:///new.ts".to_string())
    );
}

// Go: .../RenameFile with kind rename
#[test]
fn doc_edit_rename() {
    let v: TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile = serde_json::from_str(
        r#"{"kind": "rename", "oldUri": "file:///old.ts", "newUri": "file:///new.ts"}"#,
    )
    .unwrap();
    assert!(v.rename_file.is_some());
    assert_eq!(
        v.rename_file.unwrap().old_uri,
        DocumentUri("file:///old.ts".to_string())
    );
}

// Go: .../DeleteFile with kind delete
#[test]
fn doc_edit_delete() {
    let v: TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile =
        serde_json::from_str(r#"{"kind": "delete", "uri": "file:///gone.ts"}"#).unwrap();
    assert!(v.delete_file.is_some());
    assert_eq!(
        v.delete_file.unwrap().uri,
        DocumentUri("file:///gone.ts".to_string())
    );
}

// === Literal types: StringLiteralCreate ===

// Go: .../TestLiteralTypes/StringLiteralCreate marshal
#[test]
fn literal_create_marshal() {
    assert_eq!(
        serde_json::to_string(&StringLiteralCreate).unwrap(),
        r#""create""#
    );
}

// Go: .../StringLiteralCreate unmarshal
#[test]
fn literal_create_unmarshal() {
    let _v: StringLiteralCreate = serde_json::from_str(r#""create""#).unwrap();
}

// Go: .../StringLiteralCreate rejects wrong value
#[test]
fn literal_create_rejects_wrong_value() {
    assert!(serde_json::from_str::<StringLiteralCreate>(r#""delete""#).is_err());
}

// Go: .../StringLiteralCreate rejects wrong type
#[test]
fn literal_create_rejects_wrong_type() {
    assert!(serde_json::from_str::<StringLiteralCreate>("42").is_err());
}

// === Remaining null-rejection / accepts-null / empty-object / round-trip ===

fn assert_null_rejected<T: serde::de::DeserializeOwned + std::fmt::Debug>(
    input: &str,
    field: &str,
) {
    let err = serde_json::from_str::<T>(input).unwrap_err();
    let want = format!("null value is not allowed for field \"{field}\"");
    assert!(err.to_string().contains(&want), "got: {err}");
}

// Go: .../TestUnmarshalRejectsNullForOptionalNonNullableFields/Hover range null
#[test]
fn null_rejected_hover_range() {
    assert_null_rejected::<Hover>(
        r#"{"contents": {"kind": "plaintext", "value": "hi"}, "range": null}"#,
        "range",
    );
}

// Go: .../WorkDoneProgressOptions workDoneProgress null
#[test]
fn null_rejected_workdoneprogressoptions_work_done_progress() {
    assert_null_rejected::<WorkDoneProgressOptions>(
        r#"{"workDoneProgress": null}"#,
        "workDoneProgress",
    );
}

// Go: .../CallHierarchyIncomingCallsParams item null
#[test]
fn null_rejected_callhierarchy_incoming_params_item() {
    assert_null_rejected::<CallHierarchyIncomingCallsParams>(r#"{"item": null}"#, "item");
}

// Go: .../CallHierarchyIncomingCall from null
#[test]
fn null_rejected_callhierarchy_incoming_call_from() {
    assert_null_rejected::<CallHierarchyIncomingCall>(
        r#"{"from": null, "fromRanges": []}"#,
        "from",
    );
}

// Go: .../InitializeParams capabilities null
#[test]
fn null_rejected_initialize_params_capabilities() {
    assert_null_rejected::<InitializeParams>(
        r#"{"processId": null, "rootUri": null, "capabilities": null}"#,
        "capabilities",
    );
}

// Go: .../InitializeResult capabilities null
#[test]
fn null_rejected_initialize_result_capabilities() {
    assert_null_rejected::<InitializeResult>(r#"{"capabilities": null}"#, "capabilities");
}

// Go: .../SemanticTokens data null (required slice)
#[test]
fn null_rejected_semantictokens_data() {
    assert_null_rejected::<SemanticTokens>(r#"{"data": null}"#, "data");
}

// Go: .../TextDocumentEdit edits null (required slice)
#[test]
fn null_rejected_textdocumentedit_edits() {
    assert_null_rejected::<TextDocumentEdit>(
        r#"{"textDocument": {"uri": "file:///a.ts", "version": 1}, "edits": null}"#,
        "edits",
    );
}

// --- TestUnmarshalAcceptsNullForNullableFields ---

// Go: .../TestUnmarshalAcceptsNullForNullableFields/InitializeParams rootUri null
#[test]
fn null_accepted_initialize_root_uri() {
    let _v: InitializeParams =
        serde_json::from_str(r#"{"processId": null, "rootUri": null, "capabilities": {}}"#)
            .unwrap();
}

// Go: .../InitializeParams workspaceFolders null
#[test]
fn null_accepted_initialize_workspace_folders() {
    let _v: InitializeParams = serde_json::from_str(
        r#"{"processId": null, "rootUri": null, "capabilities": {}, "workspaceFolders": null}"#,
    )
    .unwrap();
}

// Go: .../InitializeParams processId null
#[test]
fn null_accepted_initialize_process_id() {
    let _v: InitializeParams =
        serde_json::from_str(r#"{"processId": null, "rootUri": null, "capabilities": {}}"#)
            .unwrap();
}

// Go: .../InitializationOptions userPreferences null
#[test]
fn null_accepted_initialization_options_user_preferences() {
    let _v: InitializationOptions = serde_json::from_str(r#"{"userPreferences": null}"#).unwrap();
}

// --- TestUnmarshalEmptyObject ---

// Go: .../TestUnmarshalEmptyObject/WorkDoneProgressOptions empty
#[test]
fn empty_workdoneprogressoptions() {
    let v: WorkDoneProgressOptions = serde_json::from_str("{}").unwrap();
    assert!(v.work_done_progress.is_none());
}

// Go: .../InitializationOptions empty
#[test]
fn empty_initialization_options() {
    let _v: InitializationOptions = serde_json::from_str("{}").unwrap();
}

// Go: .../ClientCapabilities empty
#[test]
fn empty_client_capabilities() {
    let _v: ClientCapabilities = serde_json::from_str("{}").unwrap();
}

// Go: .../ServerCapabilities empty
#[test]
fn empty_server_capabilities() {
    let _v: ServerCapabilities = serde_json::from_str("{}").unwrap();
}

// Go: lsp_generated.go:ServerCapabilities
// A default (server-produced) capabilities value omits every absent provider,
// matching the Go `json:",omitzero"` pointer fields.
#[test]
fn server_capabilities_default_serializes_empty() {
    let v = ServerCapabilities::default();
    assert_eq!(serde_json::to_string(&v).unwrap(), "{}");
}

// Tracer (real RED -> GREEN): the typed tree replaces the open object, so a set
// `hoverProvider` boolean variant is serialized instead of being dropped to `{}`.
// Go: lsp_generated.go:ServerCapabilities (hoverProvider *BooleanOrHoverOptions)
#[test]
fn server_capabilities_hover_provider_bool_serializes() {
    let v = ServerCapabilities {
        hover_provider: Some(BooleanOrHoverOptions {
            boolean: Some(true),
            hover_options: None,
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"hoverProvider":true}"#
    );
}

// === textDocumentSync provider group ===

// `textDocumentSync` is a `TextDocumentSyncOptions | TextDocumentSyncKind` union;
// the bare-number variant serializes as the integer kind.
// Go: lsp_generated.go:ServerCapabilities (textDocumentSync) / TextDocumentSyncOptionsOrKind
#[test]
fn server_capabilities_text_document_sync_kind() {
    let v = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncOptionsOrKind {
            kind: Some(TextDocumentSyncKind::INCREMENTAL),
            options: None,
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"textDocumentSync":2}"#
    );
}

// The detailed-options variant serializes the nested object and round-trips,
// including the `save` boolean union and the `change` kind.
// Go: lsp_generated.go:TextDocumentSyncOptions
#[test]
fn text_document_sync_options_round_trip() {
    let v = TextDocumentSyncOptions {
        open_close: Some(true),
        change: Some(TextDocumentSyncKind::FULL),
        save: Some(BooleanOrSaveOptions {
            boolean: Some(true),
            save_options: None,
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"openClose":true,"change":1,"save":true}"#);
    let back: TextDocumentSyncOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `SaveOptions` carries an optional `includeText` flag (object variant of the
// `boolean | SaveOptions` save union).
// Go: lsp_generated.go:SaveOptions / BooleanOrSaveOptions
#[test]
fn save_options_object_variant_serde() {
    let v = BooleanOrSaveOptions {
        boolean: None,
        save_options: Some(SaveOptions {
            include_text: Some(true),
        }),
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"includeText":true}"#
    );
    let back: BooleanOrSaveOptions = serde_json::from_str(r#"{"includeText":true}"#).unwrap();
    assert_eq!(v, back);
}

// === completion provider group ===

// `completionProvider` is a plain `CompletionOptions` object (no boolean union);
// trigger characters, resolve support and nested completion-item options all
// round-trip.
// Go: lsp_generated.go:ServerCapabilities (completionProvider) / CompletionOptions
#[test]
fn server_capabilities_completion_provider_round_trip() {
    let v = ServerCapabilities {
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_string(), "\"".to_string()]),
            resolve_provider: Some(true),
            completion_item: Some(ServerCompletionItemOptions {
                label_details_support: Some(true),
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"completionProvider":{"triggerCharacters":[".","\""],"resolveProvider":true,"completionItem":{"labelDetailsSupport":true}}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// === signatureHelp / definition / references provider groups ===

// `signatureHelpProvider` is a plain `SignatureHelpOptions` object.
// Go: lsp_generated.go:ServerCapabilities (signatureHelpProvider) / SignatureHelpOptions
#[test]
fn server_capabilities_signature_help_provider() {
    let v = ServerCapabilities {
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"signatureHelpProvider":{"triggerCharacters":["(",","]}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `definitionProvider` is a `boolean | DefinitionOptions` union; the boolean
// variant serializes as a bare `true`.
// Go: lsp_generated.go:ServerCapabilities (definitionProvider) / BooleanOrDefinitionOptions
#[test]
fn server_capabilities_definition_provider_bool() {
    let v = ServerCapabilities {
        definition_provider: Some(BooleanOrDefinitionOptions {
            boolean: Some(true),
            definition_options: None,
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"definitionProvider":true}"#
    );
}

// `referencesProvider`'s object variant decodes into `ReferenceOptions`.
// Go: lsp_generated.go:ServerCapabilities (referencesProvider) / BooleanOrReferenceOptions
#[test]
fn server_capabilities_references_provider_options() {
    let v = ServerCapabilities {
        references_provider: Some(BooleanOrReferenceOptions {
            boolean: None,
            reference_options: Some(ReferenceOptions {
                work_done_progress: Some(true),
            }),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"referencesProvider":{"workDoneProgress":true}}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// === documentSymbol / codeAction provider groups ===

// `documentSymbolProvider`'s object variant carries an optional outline `label`.
// Go: lsp_generated.go:ServerCapabilities (documentSymbolProvider) / BooleanOrDocumentSymbolOptions
#[test]
fn server_capabilities_document_symbol_provider_options() {
    let v = ServerCapabilities {
        document_symbol_provider: Some(BooleanOrDocumentSymbolOptions {
            boolean: None,
            document_symbol_options: Some(DocumentSymbolOptions {
                label: Some("TS".to_string()),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"documentSymbolProvider":{"label":"TS"}}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `codeActionProvider`'s object variant carries the supported `codeActionKinds`
// (a `Vec<CodeActionKind>`) and `resolveProvider`.
// Go: lsp_generated.go:ServerCapabilities (codeActionProvider) / BooleanOrCodeActionOptions
#[test]
fn server_capabilities_code_action_provider_options() {
    let v = ServerCapabilities {
        code_action_provider: Some(BooleanOrCodeActionOptions {
            boolean: None,
            code_action_options: Some(CodeActionOptions {
                code_action_kinds: Some(vec![CodeActionKind::QUICK_FIX, CodeActionKind::REFACTOR]),
                resolve_provider: Some(true),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"codeActionProvider":{"codeActionKinds":["quickfix","refactor"],"resolveProvider":true}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// === documentFormatting / rename / workspaceSymbol provider groups ===

// `documentFormattingProvider` boolean shorthand.
// Go: lsp_generated.go:ServerCapabilities (documentFormattingProvider) / BooleanOrDocumentFormattingOptions
#[test]
fn server_capabilities_document_formatting_provider_bool() {
    let v = ServerCapabilities {
        document_formatting_provider: Some(BooleanOrDocumentFormattingOptions {
            boolean: Some(true),
            document_formatting_options: None,
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"documentFormattingProvider":true}"#
    );
}

// `renameProvider`'s object variant carries `prepareProvider`.
// Go: lsp_generated.go:ServerCapabilities (renameProvider) / BooleanOrRenameOptions
#[test]
fn server_capabilities_rename_provider_options() {
    let v = ServerCapabilities {
        rename_provider: Some(BooleanOrRenameOptions {
            boolean: None,
            rename_options: Some(RenameOptions {
                prepare_provider: Some(true),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"renameProvider":{"prepareProvider":true}}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `workspaceSymbolProvider`'s object variant carries `resolveProvider`.
// Go: lsp_generated.go:ServerCapabilities (workspaceSymbolProvider) / BooleanOrWorkspaceSymbolOptions
#[test]
fn server_capabilities_workspace_symbol_provider_options() {
    let v = ServerCapabilities {
        workspace_symbol_provider: Some(BooleanOrWorkspaceSymbolOptions {
            boolean: None,
            workspace_symbol_options: Some(WorkspaceSymbolOptions {
                resolve_provider: Some(true),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"workspaceSymbolProvider":{"resolveProvider":true}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// === semanticTokens provider group ===

// `semanticTokensProvider`'s options variant serializes the required `legend`
// plus the `range` (boolean) and `full` (delta) sub-unions, and round-trips.
// Go: lsp_generated.go:ServerCapabilities (semanticTokensProvider)
//     / SemanticTokensOptionsOrRegistrationOptions / SemanticTokensOptions
#[test]
fn server_capabilities_semantic_tokens_provider_options() {
    let v = ServerCapabilities {
        semantic_tokens_provider: Some(SemanticTokensOptionsOrRegistrationOptions {
            options: Some(SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: vec!["namespace".to_string()],
                    token_modifiers: vec![],
                },
                range: Some(BooleanOrEmptyObject {
                    boolean: Some(true),
                    empty_object: None,
                }),
                full: Some(BooleanOrSemanticTokensFullDelta {
                    boolean: None,
                    semantic_tokens_full_delta: Some(SemanticTokensFullDelta { delta: Some(true) }),
                }),
                ..Default::default()
            }),
            registration_options: None,
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"semanticTokensProvider":{"legend":{"tokenTypes":["namespace"],"tokenModifiers":[]},"range":true,"full":{"delta":true}}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `legend.tokenTypes` is a required property; decoding without it reports the
// Go `errMissing` text.
// Go: lsp_generated.go:SemanticTokensLegend (missingTokenTypes)
#[test]
fn semantic_tokens_legend_requires_token_types() {
    let err = serde_json::from_str::<SemanticTokensLegend>(r#"{"tokenModifiers":[]}"#)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing required properties: tokenTypes"),
        "unexpected error: {err}"
    );
}

// The registration-options variant (object carrying `documentSelector`) is
// dispatched into the deferred raw-JSON variant.
// Go: lsp_generated.go:SemanticTokensOptionsOrRegistrationOptions (documentSelector key)
#[test]
fn semantic_tokens_provider_registration_variant() {
    let parsed: SemanticTokensOptionsOrRegistrationOptions = serde_json::from_str(
        r#"{"documentSelector":[{"language":"typescript"}],"legend":{"tokenTypes":[],"tokenModifiers":[]}}"#,
    )
    .unwrap();
    assert!(parsed.options.is_none());
    assert!(parsed.registration_options.is_some());
}

// === positionEncoding + remaining (deferred / boolean) provider fields ===

// `positionEncoding` is a `PositionEncodingKind` string enum.
// Go: lsp_generated.go:ServerCapabilities (positionEncoding *PositionEncodingKind)
#[test]
fn server_capabilities_position_encoding() {
    let v = ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"positionEncoding":"utf-16"}"#
    );
}

// Deferred provider fields (raw JSON) and the typescript-go boolean providers
// round-trip and serialize in the Go field-declaration order.
// Go: lsp_generated.go:ServerCapabilities
//     (executeCommandProvider / customSourceDefinitionProvider / _vs_referencesProvider)
#[test]
fn server_capabilities_deferred_and_bool_fields_round_trip() {
    let v = ServerCapabilities {
        execute_command_provider: Some(ExecuteCommandOptions {
            work_done_progress: None,
            commands: vec!["foo.bar".to_string()],
        }),
        custom_source_definition_provider: Some(true),
        vs_references_provider: Some(true),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"executeCommandProvider":{"commands":["foo.bar"]},"customSourceDefinitionProvider":true,"_vs_referencesProvider":true}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `executeCommandProvider` is a typed `ExecuteCommandOptions` (required
// `commands`, optional `workDoneProgress`). Round-trips through the typed tree.
// Go: lsp_generated.go:ExecuteCommandOptions
#[test]
fn server_capabilities_execute_command_provider_options() {
    let v = ServerCapabilities {
        execute_command_provider: Some(ExecuteCommandOptions {
            work_done_progress: None,
            commands: vec!["foo.bar".to_string(), "foo.baz".to_string()],
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"executeCommandProvider":{"commands":["foo.bar","foo.baz"]}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `documentOnTypeFormattingProvider` is a typed `DocumentOnTypeFormattingOptions`
// (required `firstTriggerCharacter`, optional `moreTriggerCharacter`).
// Go: lsp_generated.go:DocumentOnTypeFormattingOptions
#[test]
fn server_capabilities_document_on_type_formatting_provider_options() {
    let v = ServerCapabilities {
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: "{".to_string(),
            more_trigger_character: Some(vec![";".to_string(), "\n".to_string()]),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"documentOnTypeFormattingProvider":{"firstTriggerCharacter":"{","moreTriggerCharacter":[";","\n"]}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `firstTriggerCharacter` is required: decoding without it yields the Go
// `errMissing` text.
// Go: lsp_generated.go:DocumentOnTypeFormattingOptions (missingFirstTriggerCharacter)
#[test]
fn document_on_type_formatting_requires_first_trigger_character() {
    let err = serde_json::from_str::<DocumentOnTypeFormattingOptions>(
        r#"{"moreTriggerCharacter":[";"]}"#,
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("missing required properties: firstTriggerCharacter"),
        "unexpected error: {err}"
    );
}

// `codeLensProvider` is a typed `CodeLensOptions` (workDoneProgress +
// resolveProvider). Round-trips through the typed tree.
// Go: lsp_generated.go:CodeLensOptions
#[test]
fn server_capabilities_code_lens_provider_options() {
    let v = ServerCapabilities {
        code_lens_provider: Some(CodeLensOptions {
            work_done_progress: None,
            resolve_provider: Some(true),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"codeLensProvider":{"resolveProvider":true}}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `documentLinkProvider` is a typed `DocumentLinkOptions` (workDoneProgress +
// resolveProvider).
// Go: lsp_generated.go:DocumentLinkOptions
#[test]
fn server_capabilities_document_link_provider_options() {
    let v = ServerCapabilities {
        document_link_provider: Some(DocumentLinkOptions {
            work_done_progress: Some(true),
            resolve_provider: Some(false),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"documentLinkProvider":{"workDoneProgress":true,"resolveProvider":false}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `_vs_onAutoInsertProvider` is a typed `VsOnAutoInsertOptions` (required
// `_vs_triggerCharacters`).
// Go: lsp_generated.go:VsOnAutoInsertOptions
#[test]
fn server_capabilities_vs_on_auto_insert_provider_options() {
    let v = ServerCapabilities {
        vs_on_auto_insert_provider: Some(VsOnAutoInsertOptions {
            vs_trigger_characters: vec![">".to_string(), "/".to_string()],
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"_vs_onAutoInsertProvider":{"_vs_triggerCharacters":[">","/"]}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `documentHighlightProvider` is a `boolean | DocumentHighlightOptions` union;
// the bare-boolean variant serializes as a JSON boolean.
// Go: lsp_generated.go:BooleanOrDocumentHighlightOptions
#[test]
fn server_capabilities_document_highlight_provider_bool() {
    let v = ServerCapabilities {
        document_highlight_provider: Some(BooleanOrDocumentHighlightOptions {
            boolean: Some(true),
            document_highlight_options: None,
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"documentHighlightProvider":true}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `documentRangeFormattingProvider` options variant carries `rangesSupport`.
// Go: lsp_generated.go:DocumentRangeFormattingOptions / BooleanOrDocumentRangeFormattingOptions
#[test]
fn server_capabilities_document_range_formatting_provider_options() {
    let v = ServerCapabilities {
        document_range_formatting_provider: Some(BooleanOrDocumentRangeFormattingOptions {
            boolean: None,
            document_range_formatting_options: Some(DocumentRangeFormattingOptions {
                work_done_progress: None,
                ranges_support: Some(true),
            }),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"documentRangeFormattingProvider":{"rangesSupport":true}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `inlineCompletionProvider` is a `boolean | InlineCompletionOptions` union;
// the options variant round-trips through `workDoneProgress`.
// Go: lsp_generated.go:BooleanOrInlineCompletionOptions
#[test]
fn server_capabilities_inline_completion_provider_options() {
    let v = ServerCapabilities {
        inline_completion_provider: Some(BooleanOrInlineCompletionOptions {
            boolean: None,
            inline_completion_options: Some(InlineCompletionOptions {
                work_done_progress: Some(true),
            }),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"inlineCompletionProvider":{"workDoneProgress":true}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `diagnosticProvider` options variant: `DiagnosticOptions` always serializes
// the required non-pointer bools `interFileDependencies`/`workspaceDiagnostics`.
// Go: lsp_generated.go:DiagnosticOptions / DiagnosticOptionsOrRegistrationOptions
#[test]
fn server_capabilities_diagnostic_provider_options() {
    let v = ServerCapabilities {
        diagnostic_provider: Some(DiagnosticOptionsOrRegistrationOptions {
            options: Some(DiagnosticOptions {
                work_done_progress: None,
                identifier: Some("ts".to_string()),
                inter_file_dependencies: true,
                workspace_diagnostics: false,
            }),
            registration_options: None,
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"diagnosticProvider":{"identifier":"ts","interFileDependencies":true,"workspaceDiagnostics":false}}"#
    );
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `diagnosticProvider` dispatches an object carrying `documentSelector` to the
// registration-options variant (kept as raw JSON), mirroring the Go dispatch.
// Go: lsp_generated.go:DiagnosticOptionsOrRegistrationOptions.UnmarshalJSONFrom
#[test]
fn diagnostic_provider_registration_variant() {
    let input = r#"{"diagnosticProvider":{"documentSelector":[{"language":"typescript"}],"interFileDependencies":true,"workspaceDiagnostics":false,"id":"reg1"}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let d = caps.diagnostic_provider.unwrap();
    assert!(d.options.is_none());
    assert!(d.registration_options.is_some());
}

// `declarationProvider` is a `boolean | DeclarationOptions |
// DeclarationRegistrationOptions` triple union. The bare-boolean variant
// serializes as a JSON boolean.
// Go: lsp_generated.go:BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions
#[test]
fn server_capabilities_declaration_provider_bool() {
    let v = ServerCapabilities {
        declaration_provider: Some(
            BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions {
                boolean: Some(true),
                declaration_options: None,
                registration_options: None,
            },
        ),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"declarationProvider":true}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// The options variant (an object without `documentSelector`) round-trips
// through the typed `DeclarationOptions`.
// Go: lsp_generated.go:DeclarationOptions
#[test]
fn server_capabilities_declaration_provider_options() {
    let v = ServerCapabilities {
        declaration_provider: Some(
            BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions {
                boolean: None,
                declaration_options: Some(DeclarationOptions {
                    work_done_progress: Some(true),
                }),
                registration_options: None,
            },
        ),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"declarationProvider":{"workDoneProgress":true}}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// An object carrying `documentSelector` dispatches to the registration-options
// variant (kept as raw JSON), mirroring the Go `jsonObjectHasKey` dispatch.
// Go: lsp_generated.go:BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions.UnmarshalJSONFrom
#[test]
fn declaration_provider_registration_variant() {
    let input =
        r#"{"declarationProvider":{"documentSelector":[{"language":"typescript"}],"id":"reg1"}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let d = caps.declaration_provider.unwrap();
    assert!(d.boolean.is_none());
    assert!(d.declaration_options.is_none());
    assert!(d.registration_options.is_some());
}

// `inlayHintProvider` options variant carries `resolveProvider` in addition to
// `workDoneProgress` (the one triple-union whose options type is non-trivial).
// Go: lsp_generated.go:InlayHintOptions / BooleanOrInlayHintOptionsOrInlayHintRegistrationOptions
#[test]
fn server_capabilities_inlay_hint_provider_options() {
    let v = ServerCapabilities {
        inlay_hint_provider: Some(BooleanOrInlayHintOptionsOrInlayHintRegistrationOptions {
            boolean: None,
            inlay_hint_options: Some(InlayHintOptions {
                work_done_progress: None,
                resolve_provider: Some(true),
            }),
            registration_options: None,
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"inlayHintProvider":{"resolveProvider":true}}"#);
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `colorProvider` boolean variant serializes as a JSON boolean (shared macro).
// Go: lsp_generated.go:BooleanOrDocumentColorOptionsOrDocumentColorRegistrationOptions
#[test]
fn server_capabilities_color_provider_bool() {
    let v = ServerCapabilities {
        color_provider: Some(
            BooleanOrDocumentColorOptionsOrDocumentColorRegistrationOptions {
                boolean: Some(true),
                document_color_options: None,
                registration_options: None,
            },
        ),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"colorProvider":true}"#
    );
}

// `typeDefinitionProvider` dispatches an object with `documentSelector` to the
// raw-JSON registration variant (shared macro dispatch).
// Go: lsp_generated.go:BooleanOrTypeDefinitionOptionsOrTypeDefinitionRegistrationOptions
#[test]
fn type_definition_provider_registration_variant() {
    let input = r#"{"typeDefinitionProvider":{"documentSelector":[{"language":"typescript"}]}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let t = caps.type_definition_provider.unwrap();
    assert!(t.boolean.is_none());
    assert!(t.type_definition_options.is_none());
    assert!(t.registration_options.is_some());
}

// All remaining triple-union options round-trip via the shared macro: a deeply
// populated `ServerCapabilities` survives serialize → deserialize unchanged.
// Go: lsp_generated.go:ServerCapabilities (provider groups)
#[test]
fn server_capabilities_all_triple_union_providers_round_trip() {
    let opt_true = || Some(true);
    let v = ServerCapabilities {
        implementation_provider: Some(
            BooleanOrImplementationOptionsOrImplementationRegistrationOptions {
                boolean: None,
                implementation_options: Some(ImplementationOptions {
                    work_done_progress: opt_true(),
                }),
                registration_options: None,
            },
        ),
        folding_range_provider: Some(
            BooleanOrFoldingRangeOptionsOrFoldingRangeRegistrationOptions {
                boolean: opt_true(),
                folding_range_options: None,
                registration_options: None,
            },
        ),
        selection_range_provider: Some(
            BooleanOrSelectionRangeOptionsOrSelectionRangeRegistrationOptions {
                boolean: None,
                selection_range_options: Some(SelectionRangeOptions {
                    work_done_progress: None,
                }),
                registration_options: None,
            },
        ),
        call_hierarchy_provider: Some(
            BooleanOrCallHierarchyOptionsOrCallHierarchyRegistrationOptions {
                boolean: opt_true(),
                call_hierarchy_options: None,
                registration_options: None,
            },
        ),
        linked_editing_range_provider: Some(
            BooleanOrLinkedEditingRangeOptionsOrLinkedEditingRangeRegistrationOptions {
                boolean: None,
                linked_editing_range_options: Some(LinkedEditingRangeOptions {
                    work_done_progress: opt_true(),
                }),
                registration_options: None,
            },
        ),
        moniker_provider: Some(BooleanOrMonikerOptionsOrMonikerRegistrationOptions {
            boolean: opt_true(),
            moniker_options: None,
            registration_options: None,
        }),
        type_hierarchy_provider: Some(
            BooleanOrTypeHierarchyOptionsOrTypeHierarchyRegistrationOptions {
                boolean: None,
                type_hierarchy_options: Some(TypeHierarchyOptions {
                    work_done_progress: None,
                }),
                registration_options: None,
            },
        ),
        inline_value_provider: Some(
            BooleanOrInlineValueOptionsOrInlineValueRegistrationOptions {
                boolean: opt_true(),
                inline_value_options: None,
                registration_options: None,
            },
        ),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// A fully populated mix serializes provider keys in Go declaration order
// (positionEncoding first, then the provider groups in order).
// Go: lsp_generated.go:ServerCapabilities (field order)
#[test]
fn server_capabilities_field_order() {
    let v = ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF8),
        completion_provider: Some(CompletionOptions::default()),
        hover_provider: Some(BooleanOrHoverOptions {
            boolean: Some(true),
            hover_options: None,
        }),
        definition_provider: Some(BooleanOrDefinitionOptions {
            boolean: Some(true),
            definition_options: None,
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"positionEncoding":"utf-8","completionProvider":{},"hoverProvider":true,"definitionProvider":true}"#
    );
}

// Per-type omit coverage (PORTING §8.6): every all-optional server-capability
// option struct serializes its default value to `{}`.
// Go: lsp_generated.go (the corresponding `*Options` structs, all `,omitzero`).
#[test]
fn every_simple_server_option_default_serializes_empty() {
    fn assert_empty<T: Default + Serialize>() {
        assert_eq!(serde_json::to_string(&T::default()).unwrap(), "{}");
    }
    assert_empty::<ServerCapabilities>();
    assert_empty::<TextDocumentSyncOptions>();
    assert_empty::<SaveOptions>();
    assert_empty::<CompletionOptions>();
    assert_empty::<ServerCompletionItemOptions>();
    assert_empty::<SignatureHelpOptions>();
    assert_empty::<DefinitionOptions>();
    assert_empty::<ReferenceOptions>();
    assert_empty::<DocumentSymbolOptions>();
    assert_empty::<CodeActionOptions>();
    assert_empty::<DocumentFormattingOptions>();
    assert_empty::<RenameOptions>();
    assert_empty::<WorkspaceSymbolOptions>();
    assert_empty::<SemanticTokensFullDelta>();
    // Provider option trees landed in the registration-options round.
    assert_empty::<CodeLensOptions>();
    assert_empty::<DocumentLinkOptions>();
    assert_empty::<DocumentHighlightOptions>();
    assert_empty::<DocumentRangeFormattingOptions>();
    assert_empty::<InlineCompletionOptions>();
    assert_empty::<DeclarationOptions>();
    assert_empty::<TypeDefinitionOptions>();
    assert_empty::<ImplementationOptions>();
    assert_empty::<DocumentColorOptions>();
    assert_empty::<FoldingRangeOptions>();
    assert_empty::<SelectionRangeOptions>();
    assert_empty::<CallHierarchyOptions>();
    assert_empty::<LinkedEditingRangeOptions>();
    assert_empty::<MonikerOptions>();
    assert_empty::<TypeHierarchyOptions>();
    assert_empty::<InlineValueOptions>();
    assert_empty::<InlayHintOptions>();
    // WorkspaceOptions subtree members that are all-optional.
    assert_empty::<WorkspaceOptions>();
    assert_empty::<WorkspaceFoldersServerCapabilities>();
    assert_empty::<FileOperationOptions>();
    assert_empty::<FileOperationPatternOptions>();
}

// Go: .../TestMarshalUnmarshalRoundTrip/InitializeParams with null processId
#[test]
fn roundtrip_initialize_params_null_process_id() {
    let v = InitializeParams {
        process_id: IntegerOrNull { integer: None },
        root_uri: DocumentUriOrNull {
            document_uri: Some(DocumentUri("file:///workspace".to_string())),
        },
        capabilities: ClientCapabilities::default(),
        ..Default::default()
    };
    let data = serde_json::to_vec(&v).unwrap();
    let result: InitializeParams = serde_json::from_slice(&data).unwrap();
    assert_eq!(v, result);
}

// === CompletionItem (complex decode) ===

// Go: lsp/lsproto/lsp_json_test.go:TestUnmarshalRejectsNull.../CompletionItem insertTextFormat null
#[test]
fn null_rejected_completionitem_insert_text_format() {
    assert_null_rejected::<CompletionItem>(
        r#"{"label": "test", "insertTextFormat": null}"#,
        "insertTextFormat",
    );
}

// Go: lsp/lsproto/lsp_test.go:TestUnmarshalCompletionItem
#[test]
fn unmarshal_completion_item() {
    let message = r#"{
        "label": "pageXOffset",
        "insertTextFormat": 1,
        "textEdit": {
            "newText": "pageXOffset",
            "insert": {
                "start": {"line": 4, "character": 0},
                "end": {"line": 4, "character": 4}
            },
            "replace": {
                "start": {"line": 4, "character": 0},
                "end": {"line": 4, "character": 4}
            }
        },
        "kind": 6,
        "sortText": "15",
        "commitCharacters": [".", ",", ";"]
    }"#;

    let result: CompletionItem = serde_json::from_str(message).unwrap();

    let edit_range = Range {
        start: Position {
            line: 4,
            character: 0,
        },
        end: Position {
            line: 4,
            character: 4,
        },
    };
    let expected = CompletionItem {
        label: "pageXOffset".to_string(),
        insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
        text_edit: Some(TextEditOrInsertReplaceEdit {
            text_edit: None,
            insert_replace_edit: Some(InsertReplaceEdit {
                new_text: "pageXOffset".to_string(),
                insert: edit_range.clone(),
                replace: edit_range,
            }),
        }),
        kind: Some(CompletionItemKind::VARIABLE),
        sort_text: Some("15".to_string()),
        commit_characters: Some(vec![".".to_string(), ",".to_string(), ";".to_string()]),
        ..Default::default()
    };
    assert_eq!(result, expected);
}

// === registration-options base tree ===

// `StaticRegistrationOptions` carries an optional `id` (used to register /
// deregister a request). A set `id` round-trips; the default omits it.
// Go: lsp_generated.go:StaticRegistrationOptions
#[test]
fn static_registration_options_round_trips() {
    let v = StaticRegistrationOptions {
        id: Some("reg1".to_string()),
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"id":"reg1"}"#);
    let back: StaticRegistrationOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `id` is optional (`,omitzero`): the default value serializes to `{}` and an
// empty object decodes back to an absent `id`.
// Go: lsp_generated.go:StaticRegistrationOptions (id *string `json:"id,omitzero"`)
#[test]
fn static_registration_options_default_is_empty() {
    assert_eq!(
        serde_json::to_string(&StaticRegistrationOptions::default()).unwrap(),
        "{}"
    );
    let back: StaticRegistrationOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(back.id, None);
}

// `PatternOrRelativePattern` dispatches a bare JSON string to the (ported) glob
// `pattern` variant and round-trips it.
// Go: lsp_generated.go:PatternOrRelativePattern.UnmarshalJSONFrom (string case)
#[test]
fn pattern_or_relative_pattern_string_variant() {
    let v: PatternOrRelativePattern = serde_json::from_str(r#""**/*.ts""#).unwrap();
    assert_eq!(v.pattern.as_deref(), Some("**/*.ts"));
    assert!(v.relative_pattern.is_none());
    assert_eq!(serde_json::to_string(&v).unwrap(), r#""**/*.ts""#);
}

// A JSON object dispatches to the deferred raw-JSON `RelativePattern` variant
// (its `baseUri: WorkspaceFolderOrURI` tree is not yet ported) and round-trips.
// Go: lsp_generated.go:PatternOrRelativePattern.UnmarshalJSONFrom (object case)
#[test]
fn pattern_or_relative_pattern_relative_variant_raw() {
    let input = r#"{"baseUri":"file:///ws","pattern":"*.ts"}"#;
    let v: PatternOrRelativePattern = serde_json::from_str(input).unwrap();
    assert!(v.pattern.is_none());
    assert!(v.relative_pattern.is_some());
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// `TextDocumentFilterLanguage` (the `language`-required document-filter variant)
// requires `language` and keeps `scheme`/`pattern` optional; it round-trips.
// Go: lsp_generated.go:TextDocumentFilterLanguage
#[test]
fn text_document_filter_language_round_trips() {
    let v = TextDocumentFilterLanguage {
        language: "typescript".to_string(),
        scheme: Some("file".to_string()),
        pattern: None,
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"language":"typescript","scheme":"file"}"#);
    let back: TextDocumentFilterLanguage = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `language` is required: decoding an object without it reports Go `errMissing`.
// Go: lsp_generated.go:TextDocumentFilterLanguage (missingLanguage)
#[test]
fn text_document_filter_language_requires_language() {
    let err = serde_json::from_str::<TextDocumentFilterLanguage>(r#"{"scheme":"file"}"#)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing required properties: language"),
        "unexpected error: {err}"
    );
}

// `TextDocumentFilterScheme` requires `scheme`; `TextDocumentFilterPattern`
// requires `pattern`. Both keep the other fields optional and round-trip.
// Go: lsp_generated.go:TextDocumentFilterScheme / TextDocumentFilterPattern
#[test]
fn text_document_filter_scheme_and_pattern_variants() {
    let scheme = TextDocumentFilterScheme {
        language: None,
        scheme: "file".to_string(),
        pattern: None,
    };
    assert_eq!(
        serde_json::to_string(&scheme).unwrap(),
        r#"{"scheme":"file"}"#
    );

    let pattern = TextDocumentFilterPattern {
        language: None,
        scheme: None,
        pattern: PatternOrRelativePattern {
            pattern: Some("**/*.ts".to_string()),
            relative_pattern: None,
        },
    };
    let json = serde_json::to_string(&pattern).unwrap();
    assert_eq!(json, r#"{"pattern":"**/*.ts"}"#);
    let back: TextDocumentFilterPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(pattern, back);
}

// The `DocumentFilter` union (`TextDocumentFilterLanguageOrSchemeOrPattern`)
// dispatches an object to its variant by which discriminator field is present:
// `language` first, then `scheme`, then `pattern` (mirroring the Go try-order).
// Go: lsp_generated.go:TextDocumentFilterLanguageOrSchemeOrPattern.UnmarshalJSONFrom
#[test]
fn document_filter_union_dispatch() {
    let lang: TextDocumentFilterLanguageOrSchemeOrPattern =
        serde_json::from_str(r#"{"language":"typescript"}"#).unwrap();
    assert!(lang.language.is_some());
    assert!(lang.scheme.is_none() && lang.pattern.is_none());
    assert_eq!(
        serde_json::to_string(&lang).unwrap(),
        r#"{"language":"typescript"}"#
    );

    let scheme: TextDocumentFilterLanguageOrSchemeOrPattern =
        serde_json::from_str(r#"{"scheme":"file"}"#).unwrap();
    assert!(scheme.scheme.is_some());
    assert!(scheme.language.is_none() && scheme.pattern.is_none());

    let pat: TextDocumentFilterLanguageOrSchemeOrPattern =
        serde_json::from_str(r#"{"pattern":"**/*.ts"}"#).unwrap();
    assert!(pat.pattern.is_some());
    assert!(pat.language.is_none() && pat.scheme.is_none());
}

// `DocumentSelectorOrNull` is `[]DocumentFilter | null`: a JSON array decodes to
// the present selector and round-trips as an array.
// Go: lsp_generated.go:DocumentSelectorOrNull.UnmarshalJSONFrom (array case)
#[test]
fn document_selector_or_null_array_variant() {
    let input = r#"[{"language":"typescript"},{"scheme":"file"}]"#;
    let v: DocumentSelectorOrNull = serde_json::from_str(input).unwrap();
    let sel = v.document_selector.as_ref().unwrap();
    assert_eq!(sel.len(), 2);
    assert!(sel[0].language.is_some());
    assert!(sel[1].scheme.is_some());
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// A JSON `null` decodes to an absent selector, and the default value serializes
// back to `null` (Go: a nil `*[]...` marshals as `null`).
// Go: lsp_generated.go:DocumentSelectorOrNull.MarshalJSONTo / UnmarshalJSONFrom (null)
#[test]
fn document_selector_or_null_null_variant() {
    let v: DocumentSelectorOrNull = serde_json::from_str("null").unwrap();
    assert!(v.document_selector.is_none());
    assert_eq!(serde_json::to_string(&v).unwrap(), "null");
    assert_eq!(
        serde_json::to_string(&DocumentSelectorOrNull::default()).unwrap(),
        "null"
    );
}

// `TextDocumentRegistrationOptions` carries the required `documentSelector`
// (`DocumentSelectorOrNull`). A populated selector round-trips.
// Go: lsp_generated.go:TextDocumentRegistrationOptions
#[test]
fn text_document_registration_options_round_trips() {
    let v = TextDocumentRegistrationOptions {
        document_selector: DocumentSelectorOrNull {
            document_selector: Some(vec![TextDocumentFilterLanguageOrSchemeOrPattern {
                language: Some(TextDocumentFilterLanguage {
                    language: "typescript".to_string(),
                    scheme: None,
                    pattern: None,
                }),
                scheme: None,
                pattern: None,
            }]),
        },
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"documentSelector":[{"language":"typescript"}]}"#);
    let back: TextDocumentRegistrationOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `documentSelector` is required but always emitted (no `,omitzero`): a `null`
// selector decodes to an absent inner selector and the default serializes the
// key as `null`.
// Go: lsp_generated.go:TextDocumentRegistrationOptions (documentSelector, missing flag)
#[test]
fn text_document_registration_options_null_selector_and_missing() {
    let v: TextDocumentRegistrationOptions =
        serde_json::from_str(r#"{"documentSelector":null}"#).unwrap();
    assert!(v.document_selector.document_selector.is_none());
    assert_eq!(
        serde_json::to_string(&TextDocumentRegistrationOptions::default()).unwrap(),
        r#"{"documentSelector":null}"#
    );

    let err = serde_json::from_str::<TextDocumentRegistrationOptions>("{}")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing required properties: documentSelector"),
        "unexpected error: {err}"
    );
}

// Tracer: the `declarationProvider` registration variant now decodes into the
// *typed* `DeclarationRegistrationOptions` (flattened workDoneProgress +
// documentSelector + id) instead of raw JSON, and round-trips byte-for-byte in
// the Go field-declaration order (workDoneProgress, documentSelector, id).
// Go: lsp_generated.go:DeclarationRegistrationOptions
#[test]
fn declaration_provider_registration_variant_typed() {
    let input = r#"{"declarationProvider":{"workDoneProgress":true,"documentSelector":[{"language":"typescript"}],"id":"reg1"}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let reg = caps
        .declaration_provider
        .as_ref()
        .unwrap()
        .registration_options
        .as_ref()
        .unwrap();
    assert_eq!(reg.id.as_deref(), Some("reg1"));
    assert_eq!(reg.work_done_progress, Some(true));
    assert!(reg.document_selector.document_selector.is_some());
    assert_eq!(serde_json::to_string(&caps).unwrap(), input);
}

// The `diagnosticProvider` registration variant now decodes into the typed
// `DiagnosticRegistrationOptions` (the required non-pointer bools
// `interFileDependencies`/`workspaceDiagnostics` plus documentSelector/id), and
// round-trips byte-for-byte in Go field-declaration order.
// Go: lsp_generated.go:DiagnosticRegistrationOptions
#[test]
fn diagnostic_provider_registration_variant_typed() {
    let input = r#"{"diagnosticProvider":{"documentSelector":[{"language":"typescript"}],"workDoneProgress":true,"identifier":"ts","interFileDependencies":true,"workspaceDiagnostics":false,"id":"reg1"}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let reg = caps
        .diagnostic_provider
        .as_ref()
        .unwrap()
        .registration_options
        .as_ref()
        .unwrap();
    assert_eq!(reg.identifier.as_deref(), Some("ts"));
    assert!(reg.inter_file_dependencies);
    assert!(!reg.workspace_diagnostics);
    assert_eq!(reg.id.as_deref(), Some("reg1"));
    assert!(reg.document_selector.document_selector.is_some());
    assert_eq!(serde_json::to_string(&caps).unwrap(), input);
}

// The `semanticTokensProvider` registration variant now decodes into the typed
// `SemanticTokensRegistrationOptions` (required `legend` + documentSelector/id),
// and round-trips byte-for-byte in Go field-declaration order.
// Go: lsp_generated.go:SemanticTokensRegistrationOptions
#[test]
fn semantic_tokens_provider_registration_variant_typed() {
    let input = r#"{"semanticTokensProvider":{"documentSelector":[{"language":"typescript"}],"legend":{"tokenTypes":["namespace"],"tokenModifiers":[]},"range":true,"id":"reg1"}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let reg = caps
        .semantic_tokens_provider
        .as_ref()
        .unwrap()
        .registration_options
        .as_ref()
        .unwrap();
    assert_eq!(reg.legend.token_types, vec!["namespace".to_string()]);
    assert_eq!(reg.id.as_deref(), Some("reg1"));
    assert!(reg.range.is_some());
    assert!(reg.document_selector.document_selector.is_some());
    assert_eq!(serde_json::to_string(&caps).unwrap(), input);
}

// Builds a one-language document selector for the registration round-trip tests.
fn sample_document_selector() -> DocumentSelectorOrNull {
    DocumentSelectorOrNull {
        document_selector: Some(vec![TextDocumentFilterLanguageOrSchemeOrPattern {
            language: Some(TextDocumentFilterLanguage {
                language: "typescript".to_string(),
                scheme: None,
                pattern: None,
            }),
            scheme: None,
            pattern: None,
        }]),
    }
}

// Every triple-union provider's *typed* registration variant survives a
// serialize -> deserialize round-trip (the 12 macro-generated unions sharing the
// upgraded `boolean_or_options_or_registration!` registration arm).
// Go: lsp_generated.go:ServerCapabilities (triple-union provider groups)
#[test]
fn all_triple_union_registration_variants_round_trip() {
    let v = ServerCapabilities {
        declaration_provider: Some(
            BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions {
                registration_options: Some(DeclarationRegistrationOptions {
                    work_done_progress: Some(true),
                    document_selector: sample_document_selector(),
                    id: Some("d".to_string()),
                }),
                ..Default::default()
            },
        ),
        type_definition_provider: Some(
            BooleanOrTypeDefinitionOptionsOrTypeDefinitionRegistrationOptions {
                registration_options: Some(TypeDefinitionRegistrationOptions {
                    document_selector: sample_document_selector(),
                    work_done_progress: None,
                    id: Some("td".to_string()),
                }),
                ..Default::default()
            },
        ),
        implementation_provider: Some(
            BooleanOrImplementationOptionsOrImplementationRegistrationOptions {
                registration_options: Some(ImplementationRegistrationOptions {
                    document_selector: sample_document_selector(),
                    work_done_progress: Some(false),
                    id: None,
                }),
                ..Default::default()
            },
        ),
        color_provider: Some(
            BooleanOrDocumentColorOptionsOrDocumentColorRegistrationOptions {
                registration_options: Some(DocumentColorRegistrationOptions {
                    document_selector: sample_document_selector(),
                    work_done_progress: None,
                    id: None,
                }),
                ..Default::default()
            },
        ),
        folding_range_provider: Some(
            BooleanOrFoldingRangeOptionsOrFoldingRangeRegistrationOptions {
                registration_options: Some(FoldingRangeRegistrationOptions {
                    document_selector: DocumentSelectorOrNull::default(),
                    work_done_progress: None,
                    id: Some("fr".to_string()),
                }),
                ..Default::default()
            },
        ),
        selection_range_provider: Some(
            BooleanOrSelectionRangeOptionsOrSelectionRangeRegistrationOptions {
                registration_options: Some(SelectionRangeRegistrationOptions {
                    work_done_progress: Some(true),
                    document_selector: sample_document_selector(),
                    id: None,
                }),
                ..Default::default()
            },
        ),
        call_hierarchy_provider: Some(
            BooleanOrCallHierarchyOptionsOrCallHierarchyRegistrationOptions {
                registration_options: Some(CallHierarchyRegistrationOptions {
                    document_selector: sample_document_selector(),
                    work_done_progress: None,
                    id: None,
                }),
                ..Default::default()
            },
        ),
        linked_editing_range_provider: Some(
            BooleanOrLinkedEditingRangeOptionsOrLinkedEditingRangeRegistrationOptions {
                registration_options: Some(LinkedEditingRangeRegistrationOptions {
                    document_selector: sample_document_selector(),
                    work_done_progress: None,
                    id: None,
                }),
                ..Default::default()
            },
        ),
        moniker_provider: Some(BooleanOrMonikerOptionsOrMonikerRegistrationOptions {
            registration_options: Some(MonikerRegistrationOptions {
                document_selector: sample_document_selector(),
                work_done_progress: Some(true),
            }),
            ..Default::default()
        }),
        type_hierarchy_provider: Some(
            BooleanOrTypeHierarchyOptionsOrTypeHierarchyRegistrationOptions {
                registration_options: Some(TypeHierarchyRegistrationOptions {
                    document_selector: sample_document_selector(),
                    work_done_progress: None,
                    id: None,
                }),
                ..Default::default()
            },
        ),
        inline_value_provider: Some(
            BooleanOrInlineValueOptionsOrInlineValueRegistrationOptions {
                registration_options: Some(InlineValueRegistrationOptions {
                    work_done_progress: None,
                    document_selector: sample_document_selector(),
                    id: None,
                }),
                ..Default::default()
            },
        ),
        inlay_hint_provider: Some(BooleanOrInlayHintOptionsOrInlayHintRegistrationOptions {
            registration_options: Some(InlayHintRegistrationOptions {
                work_done_progress: None,
                resolve_provider: Some(true),
                document_selector: sample_document_selector(),
                id: Some("ih".to_string()),
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    let back: ServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `inlayHintProvider`'s registration variant carries `resolveProvider` and
// serializes in Go field order (workDoneProgress, resolveProvider,
// documentSelector, id). A `documentSelector:null` still dispatches to the
// registration variant (the `documentSelector` key is present).
// Go: lsp_generated.go:InlayHintRegistrationOptions
#[test]
fn inlay_hint_registration_variant_field_order() {
    let input = r#"{"inlayHintProvider":{"workDoneProgress":true,"resolveProvider":true,"documentSelector":null,"id":"x"}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let reg = caps
        .inlay_hint_provider
        .as_ref()
        .unwrap()
        .registration_options
        .as_ref()
        .unwrap();
    assert_eq!(reg.resolve_provider, Some(true));
    assert!(reg.document_selector.document_selector.is_none());
    assert_eq!(serde_json::to_string(&caps).unwrap(), input);
}

// `MonikerRegistrationOptions` has no `id` field in the Go model: an incoming
// `id` key is ignored (unknown field) and never re-emitted.
// Go: lsp_generated.go:MonikerRegistrationOptions (no Id field)
#[test]
fn moniker_registration_variant_has_no_id() {
    let input = r#"{"monikerProvider":{"documentSelector":[{"language":"typescript"}],"workDoneProgress":true,"id":"ignored"}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let reg = caps
        .moniker_provider
        .as_ref()
        .unwrap()
        .registration_options
        .as_ref()
        .unwrap();
    assert_eq!(reg.work_done_progress, Some(true));
    // Re-serialization drops the unknown `id` key.
    assert_eq!(
        serde_json::to_string(&caps).unwrap(),
        r#"{"monikerProvider":{"documentSelector":[{"language":"typescript"}],"workDoneProgress":true}}"#
    );
}

// Per-type coverage (PORTING §8.6): the required `documentSelector` is always
// emitted, so each registration-options struct's default serializes the
// `documentSelector` key as `null` (plus any required non-pointer fields).
// Go: lsp_generated.go (the `*RegistrationOptions` structs)
#[test]
fn every_registration_options_default_serializes_document_selector() {
    fn assert_doc_sel_null<T: Default + Serialize>() {
        assert_eq!(
            serde_json::to_string(&T::default()).unwrap(),
            r#"{"documentSelector":null}"#
        );
    }
    assert_doc_sel_null::<TextDocumentRegistrationOptions>();
    assert_doc_sel_null::<DeclarationRegistrationOptions>();
    assert_doc_sel_null::<TypeDefinitionRegistrationOptions>();
    assert_doc_sel_null::<ImplementationRegistrationOptions>();
    assert_doc_sel_null::<DocumentColorRegistrationOptions>();
    assert_doc_sel_null::<FoldingRangeRegistrationOptions>();
    assert_doc_sel_null::<SelectionRangeRegistrationOptions>();
    assert_doc_sel_null::<CallHierarchyRegistrationOptions>();
    assert_doc_sel_null::<LinkedEditingRangeRegistrationOptions>();
    assert_doc_sel_null::<MonikerRegistrationOptions>();
    assert_doc_sel_null::<TypeHierarchyRegistrationOptions>();
    assert_doc_sel_null::<InlineValueRegistrationOptions>();
    assert_doc_sel_null::<InlayHintRegistrationOptions>();

    // `DiagnosticRegistrationOptions` also always emits its required non-pointer
    // bools; `SemanticTokensRegistrationOptions` always emits its required legend.
    assert_eq!(
        serde_json::to_string(&DiagnosticRegistrationOptions::default()).unwrap(),
        r#"{"documentSelector":null,"interFileDependencies":false,"workspaceDiagnostics":false}"#
    );
    assert_eq!(
        serde_json::to_string(&SemanticTokensRegistrationOptions::default()).unwrap(),
        r#"{"documentSelector":null,"legend":{"tokenTypes":[],"tokenModifiers":[]}}"#
    );
}

// === WorkspaceOptions subtree (ServerCapabilities.workspace) ===

// tracer (real RED->GREEN): `WorkspaceFoldersServerCapabilities` replaces a
// not-yet-existing type. `supported` (an optional bool) round-trips, and the
// `changeNotifications` field carries the `StringOrBoolean` union.
// Go: lsp_generated.go:WorkspaceFoldersServerCapabilities
#[test]
fn workspace_folders_server_capabilities_supported_round_trip() {
    let v = WorkspaceFoldersServerCapabilities {
        supported: Some(true),
        change_notifications: None,
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"supported":true}"#);
    let back: WorkspaceFoldersServerCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `changeNotifications` accepts a registration-id string (the `StringOrBoolean`
// union string arm) and round-trips. (green-on-arrival: union + macro landed.)
// Go: lsp_generated.go:WorkspaceFoldersServerCapabilities (changeNotifications)
#[test]
fn workspace_folders_change_notifications_string_variant() {
    let input = r#"{"changeNotifications":"workspace/didChangeWorkspaceFolders"}"#;
    let v: WorkspaceFoldersServerCapabilities = serde_json::from_str(input).unwrap();
    assert_eq!(
        v.change_notifications.as_ref().unwrap().string.as_deref(),
        Some("workspace/didChangeWorkspaceFolders")
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// `changeNotifications` also accepts a boolean (the union boolean arm).
// (green-on-arrival.)
// Go: lsp_generated.go:WorkspaceFoldersServerCapabilities (changeNotifications)
#[test]
fn workspace_folders_change_notifications_bool_variant() {
    let input = r#"{"supported":true,"changeNotifications":true}"#;
    let v: WorkspaceFoldersServerCapabilities = serde_json::from_str(input).unwrap();
    assert_eq!(v.change_notifications.as_ref().unwrap().boolean, Some(true));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// The `StringOrBoolean` union dispatches a string vs. a boolean and rejects any
// other JSON kind (mirroring the Go `PeekKind` switch). (green-on-arrival.)
// Go: lsp_generated.go:StringOrBoolean.UnmarshalJSONFrom
#[test]
fn string_or_boolean_union_dispatch() {
    let s: StringOrBoolean = serde_json::from_str(r#""x""#).unwrap();
    assert_eq!(s.string.as_deref(), Some("x"));
    assert!(s.boolean.is_none());

    let b: StringOrBoolean = serde_json::from_str("false").unwrap();
    assert_eq!(b.boolean, Some(false));
    assert!(b.string.is_none());

    // A number is neither a string nor a boolean: rejected.
    assert!(serde_json::from_str::<StringOrBoolean>("42").is_err());
}

// Per-type coverage (PORTING §8.6): all-optional, so default serializes to `{}`.
// Go: lsp_generated.go:WorkspaceFoldersServerCapabilities
#[test]
fn workspace_folders_server_capabilities_default_empty() {
    assert_eq!(
        serde_json::to_string(&WorkspaceFoldersServerCapabilities::default()).unwrap(),
        "{}"
    );
}

// tracer (real RED->GREEN): `FileOperationPattern` (the bottom of the
// fileOperations chain) with its required `glob`, optional `matches`
// (`FileOperationPatternKind` string enum) and optional `options`
// (`FileOperationPatternOptions`) round-trips in Go field order.
// Go: lsp_generated.go:FileOperationPattern
#[test]
fn file_operation_pattern_round_trip() {
    let v = FileOperationPattern {
        glob: "**/*.ts".to_string(),
        matches: Some(FileOperationPatternKind::FILE),
        options: Some(FileOperationPatternOptions {
            ignore_case: Some(true),
        }),
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"glob":"**/*.ts","matches":"file","options":{"ignoreCase":true}}"#
    );
    let back: FileOperationPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// real RED->GREEN: `FileOperationFilter` wraps a (required) `pattern` plus an
// optional `scheme`; it round-trips in Go field order (scheme, pattern).
// Go: lsp_generated.go:FileOperationFilter
#[test]
fn file_operation_filter_round_trip() {
    let v = FileOperationFilter {
        scheme: Some("file".to_string()),
        pattern: FileOperationPattern {
            glob: "**/*.ts".to_string(),
            matches: Some(FileOperationPatternKind::FOLDER),
            options: None,
        },
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"scheme":"file","pattern":{"glob":"**/*.ts","matches":"folder"}}"#
    );
    let back: FileOperationFilter = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// real RED->GREEN: `FileOperationRegistrationOptions` carries the required
// `filters` array of `FileOperationFilter`; it round-trips.
// Go: lsp_generated.go:FileOperationRegistrationOptions
#[test]
fn file_operation_registration_options_round_trip() {
    let v = FileOperationRegistrationOptions {
        filters: vec![FileOperationFilter {
            scheme: None,
            pattern: FileOperationPattern {
                glob: "**/*.ts".to_string(),
                matches: None,
                options: None,
            },
        }],
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"filters":[{"pattern":{"glob":"**/*.ts"}}]}"#);
    let back: FileOperationRegistrationOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// `filters` is required: decoding `{}` reports Go `errMissing`.
// Go: lsp_generated.go:FileOperationRegistrationOptions (missingFilters)
#[test]
fn file_operation_registration_options_requires_filters() {
    let err = serde_json::from_str::<FileOperationRegistrationOptions>(r#"{}"#)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing required properties: filters"),
        "unexpected error: {err}"
    );
}

// real RED->GREEN: `FileOperationOptions` exposes the six per-operation
// registration slots (all optional); set ones round-trip in Go field order.
// Go: lsp_generated.go:FileOperationOptions
#[test]
fn file_operation_options_round_trip() {
    let reg = || FileOperationRegistrationOptions {
        filters: vec![FileOperationFilter {
            scheme: None,
            pattern: FileOperationPattern {
                glob: "**/*.ts".to_string(),
                matches: None,
                options: None,
            },
        }],
    };
    let v = FileOperationOptions {
        did_create: Some(reg()),
        will_rename: Some(reg()),
        ..Default::default()
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"didCreate":{"filters":[{"pattern":{"glob":"**/*.ts"}}]},"willRename":{"filters":[{"pattern":{"glob":"**/*.ts"}}]}}"#
    );
    let back: FileOperationOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// Per-type coverage (PORTING §8.6): all-optional, so default serializes to `{}`.
// Go: lsp_generated.go:FileOperationOptions
#[test]
fn file_operation_options_default_empty() {
    assert_eq!(
        serde_json::to_string(&FileOperationOptions::default()).unwrap(),
        "{}"
    );
}

// `glob` is required on `FileOperationPattern`; decoding `{}` reports
// Go `errMissing`. (green-on-arrival.)
// Go: lsp_generated.go:FileOperationPattern (missingGlob)
#[test]
fn file_operation_pattern_requires_glob() {
    let err = serde_json::from_str::<FileOperationPattern>(r#"{"matches":"file"}"#)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing required properties: glob"),
        "unexpected error: {err}"
    );
}

// `pattern` is required on `FileOperationFilter`, and (being a `reqnn` slot)
// rejects an explicit `null`. (green-on-arrival.)
// Go: lsp_generated.go:FileOperationFilter (missingPattern / errNull)
#[test]
fn file_operation_filter_pattern_required_and_rejects_null() {
    let missing = serde_json::from_str::<FileOperationFilter>(r#"{"scheme":"file"}"#)
        .unwrap_err()
        .to_string();
    assert!(
        missing.contains("missing required properties: pattern"),
        "unexpected error: {missing}"
    );
    assert!(serde_json::from_str::<FileOperationFilter>(r#"{"pattern":null}"#).is_err());
}

// tracer (real RED->GREEN): `TextDocumentContentOptions` carries the required
// `schemes` array; it round-trips and `schemes` is required.
// Go: lsp_generated.go:TextDocumentContentOptions
#[test]
fn text_document_content_options_round_trip() {
    let v = TextDocumentContentOptions {
        schemes: vec!["myscheme".to_string(), "other".to_string()],
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"schemes":["myscheme","other"]}"#);
    let back: TextDocumentContentOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);

    let err = serde_json::from_str::<TextDocumentContentOptions>(r#"{}"#)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing required properties: schemes"),
        "unexpected error: {err}"
    );
}

// `TextDocumentContentRegistrationOptions` adds an optional `id` next to the
// required `schemes`; it round-trips in Go field order (schemes, id).
// Go: lsp_generated.go:TextDocumentContentRegistrationOptions
#[test]
fn text_document_content_registration_options_round_trip() {
    let v = TextDocumentContentRegistrationOptions {
        schemes: vec!["myscheme".to_string()],
        id: Some("reg-1".to_string()),
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"schemes":["myscheme"],"id":"reg-1"}"#);
    let back: TextDocumentContentRegistrationOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// The `TextDocumentContentOptionsOrRegistrationOptions` union tries the plain
// options variant first (Go try-order). Because the plain options decode
// ignores an extra `id` key, it wins even when `id` is present — exactly as Go
// does — so the registration variant is effectively only reachable when
// constructed. (green-on-arrival.)
// Go: lsp_generated.go:TextDocumentContentOptionsOrRegistrationOptions.UnmarshalJSONFrom
#[test]
fn text_document_content_union_prefers_options() {
    let plain: TextDocumentContentOptionsOrRegistrationOptions =
        serde_json::from_str(r#"{"schemes":["x"]}"#).unwrap();
    assert!(plain.options.is_some());
    assert!(plain.registration_options.is_none());

    // Go tries the options variant first; an extra `id` is ignored, so options
    // still wins (matching Go's fall-through order).
    let with_id: TextDocumentContentOptionsOrRegistrationOptions =
        serde_json::from_str(r#"{"schemes":["x"],"id":"reg"}"#).unwrap();
    assert!(with_id.options.is_some());

    // The registration variant still serializes when explicitly constructed.
    let reg = TextDocumentContentOptionsOrRegistrationOptions {
        options: None,
        registration_options: Some(TextDocumentContentRegistrationOptions {
            schemes: vec!["x".to_string()],
            id: Some("reg".to_string()),
        }),
    };
    assert_eq!(
        serde_json::to_string(&reg).unwrap(),
        r#"{"schemes":["x"],"id":"reg"}"#
    );
}

// `WorkspaceOptions` assembles the three workspace members; set ones round-trip
// in Go field order (workspaceFolders, fileOperations, textDocumentContent).
// Go: lsp_generated.go:WorkspaceOptions
#[test]
fn workspace_options_round_trip() {
    let v = WorkspaceOptions {
        workspace_folders: Some(WorkspaceFoldersServerCapabilities {
            supported: Some(true),
            change_notifications: None,
        }),
        file_operations: Some(FileOperationOptions {
            did_create: Some(FileOperationRegistrationOptions {
                filters: vec![FileOperationFilter {
                    scheme: None,
                    pattern: FileOperationPattern {
                        glob: "**/*.ts".to_string(),
                        matches: None,
                        options: None,
                    },
                }],
            }),
            ..Default::default()
        }),
        text_document_content: Some(TextDocumentContentOptionsOrRegistrationOptions {
            options: Some(TextDocumentContentOptions {
                schemes: vec!["myscheme".to_string()],
            }),
            registration_options: None,
        }),
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(
        json,
        r#"{"workspaceFolders":{"supported":true},"fileOperations":{"didCreate":{"filters":[{"pattern":{"glob":"**/*.ts"}}]}},"textDocumentContent":{"schemes":["myscheme"]}}"#
    );
    let back: WorkspaceOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

// tracer (real RED->GREEN): `ServerCapabilities.workspace` is now typed
// `WorkspaceOptions` (was raw `serde_json::Value`). Decoding reaches the typed
// members and the round-trip is byte-for-byte.
// Go: lsp_generated.go:ServerCapabilities (workspace)
#[test]
fn server_capabilities_workspace_typed() {
    let input = r#"{"workspace":{"workspaceFolders":{"supported":true,"changeNotifications":"id-1"},"fileOperations":{"didRename":{"filters":[{"pattern":{"glob":"**/*.ts"}}]}}}}"#;
    let caps: ServerCapabilities = serde_json::from_str(input).unwrap();
    let ws = caps.workspace.as_ref().unwrap();
    assert_eq!(ws.workspace_folders.as_ref().unwrap().supported, Some(true));
    assert_eq!(
        ws.workspace_folders
            .as_ref()
            .unwrap()
            .change_notifications
            .as_ref()
            .unwrap()
            .string
            .as_deref(),
        Some("id-1")
    );
    assert!(ws.file_operations.as_ref().unwrap().did_rename.is_some());
    assert_eq!(serde_json::to_string(&caps).unwrap(), input);
}

// Per-type coverage (PORTING §8.6): all-optional, so default serializes to `{}`.
// Go: lsp_generated.go:WorkspaceOptions
#[test]
fn workspace_options_default_empty() {
    assert_eq!(
        serde_json::to_string(&WorkspaceOptions::default()).unwrap(),
        "{}"
    );
}

// === RelativePattern object variant of PatternOrRelativePattern ===

// tracer (real RED->GREEN): `WorkspaceFolder` (required `uri` + `name`)
// round-trips in Go field order, and both fields are required.
// Go: lsp_generated.go:WorkspaceFolder
#[test]
fn workspace_folder_round_trip() {
    let v = WorkspaceFolder {
        uri: URI("file:///ws".to_string()),
        name: "ws".to_string(),
    };
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, r#"{"uri":"file:///ws","name":"ws"}"#);
    let back: WorkspaceFolder = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);

    let err = serde_json::from_str::<WorkspaceFolder>(r#"{"uri":"file:///ws"}"#)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing required properties: name"),
        "unexpected error: {err}"
    );
}

// `WorkspaceFolderOrURI` dispatches a JSON object to the `WorkspaceFolder`
// variant and a JSON string to the bare `URI` variant (Go `PeekKind` switch).
// Go: lsp_generated.go:WorkspaceFolderOrURI.UnmarshalJSONFrom
#[test]
fn workspace_folder_or_uri_dispatch() {
    let folder: WorkspaceFolderOrURI =
        serde_json::from_str(r#"{"uri":"file:///ws","name":"ws"}"#).unwrap();
    assert!(folder.workspace_folder.is_some());
    assert!(folder.uri.is_none());
    assert_eq!(
        serde_json::to_string(&folder).unwrap(),
        r#"{"uri":"file:///ws","name":"ws"}"#
    );

    let uri: WorkspaceFolderOrURI = serde_json::from_str(r#""file:///ws""#).unwrap();
    assert_eq!(uri.uri.as_ref().unwrap().0, "file:///ws");
    assert!(uri.workspace_folder.is_none());
    assert_eq!(serde_json::to_string(&uri).unwrap(), r#""file:///ws""#);

    // A number is neither an object nor a string: rejected.
    assert!(serde_json::from_str::<WorkspaceFolderOrURI>("7").is_err());
}

// `RelativePattern` (required `baseUri` + `pattern`) round-trips with both
// `baseUri` arms: a bare URI string and a `WorkspaceFolder` object.
// Go: lsp_generated.go:RelativePattern
#[test]
fn relative_pattern_round_trip_both_base_uri_arms() {
    let uri_base = RelativePattern {
        base_uri: WorkspaceFolderOrURI {
            workspace_folder: None,
            uri: Some(URI("file:///ws".to_string())),
        },
        pattern: "*.ts".to_string(),
    };
    let json = serde_json::to_string(&uri_base).unwrap();
    assert_eq!(json, r#"{"baseUri":"file:///ws","pattern":"*.ts"}"#);
    let back: RelativePattern = serde_json::from_str(&json).unwrap();
    assert_eq!(uri_base, back);

    let folder_base = RelativePattern {
        base_uri: WorkspaceFolderOrURI {
            workspace_folder: Some(WorkspaceFolder {
                uri: URI("file:///ws".to_string()),
                name: "ws".to_string(),
            }),
            uri: None,
        },
        pattern: "**/*.js".to_string(),
    };
    let json = serde_json::to_string(&folder_base).unwrap();
    assert_eq!(
        json,
        r#"{"baseUri":{"uri":"file:///ws","name":"ws"},"pattern":"**/*.js"}"#
    );
    let back: RelativePattern = serde_json::from_str(&json).unwrap();
    assert_eq!(folder_base, back);
}

// tracer (real RED->GREEN): the `relative_pattern` arm of
// `PatternOrRelativePattern` is now the typed `RelativePattern` (was raw
// `serde_json::Value`). Decoding an object reaches the typed `base_uri`/`pattern`
// and the round-trip is byte-for-byte.
// Go: lsp_generated.go:PatternOrRelativePattern.UnmarshalJSONFrom (object case)
#[test]
fn pattern_or_relative_pattern_relative_variant_typed() {
    let input = r#"{"baseUri":"file:///ws","pattern":"*.ts"}"#;
    let v: PatternOrRelativePattern = serde_json::from_str(input).unwrap();
    assert!(v.pattern.is_none());
    let rel = v.relative_pattern.as_ref().unwrap();
    assert_eq!(rel.pattern, "*.ts");
    assert_eq!(rel.base_uri.uri.as_ref().unwrap().0, "file:///ws");
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// === request/result param + notification raw-JSON tightening ===
//
// This round tightens the remaining concrete-typed `serde_json::Value` slots in
// the request/result param + notification types. Go models these fields with a
// concrete struct/union/enum (not `LSPAny`), so the port replaces the raw value
// with the typed shape while preserving the serde behavior (field name /
// optionality). Genuinely-`any` fields (`LSPAny`, `initializationOptions`,
// `Command.arguments`, the `*Data` carriers) stay raw.

// === Slice 1: MarkupContent + Hover.contents union ===

// tracer (real RED->GREEN): `Hover.contents` is now the typed union
// `MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings` (was raw
// `serde_json::Value`). Decoding an object with a `kind` key reaches the typed
// `MarkupContent` arm, and the round-trip is byte-for-byte.
// Go: lsp_generated.go:Hover (Contents MarkupContentOrString...) / MarkupContent
#[test]
fn hover_contents_markup_content_variant() {
    let input = r##"{"contents":{"kind":"markdown","value":"# hi"}}"##;
    let v: Hover = serde_json::from_str(input).unwrap();
    let mc = v.contents.markup_content.as_ref().unwrap();
    assert_eq!(mc.kind, MarkupKind::MARKDOWN);
    assert_eq!(mc.value, "# hi");
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: the bare-string arm of the hover-content union. A JSON
// string is dispatched to the `string` field; round-trip is byte-for-byte.
// Go: lsp_generated.go:MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings (string case)
#[test]
fn hover_contents_string_variant() {
    let input = r#"{"contents":"plain hover text"}"#;
    let v: Hover = serde_json::from_str(input).unwrap();
    assert_eq!(v.contents.string.as_deref(), Some("plain hover text"));
    assert!(v.contents.markup_content.is_none());
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: the `MarkedStringWithLanguage` arm. An object with a
// `language` key (and no `kind`) dispatches to that arm.
// Go: lsp_generated.go:...MarkedStrings (jsonObjectHasKey "language")
#[test]
fn hover_contents_marked_string_with_language_variant() {
    let input = r#"{"contents":{"language":"typescript","value":"const x = 1"}}"#;
    let v: Hover = serde_json::from_str(input).unwrap();
    let ms = v.contents.marked_string_with_language.as_ref().unwrap();
    assert_eq!(ms.language, "typescript");
    assert_eq!(ms.value, "const x = 1");
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: the `MarkedString[]` arm. A JSON array dispatches to
// `marked_strings`, whose elements are themselves the `string | object` union.
// Go: lsp_generated.go:...MarkedStrings (array case) / StringOrMarkedStringWithLanguage
#[test]
fn hover_contents_marked_strings_array_variant() {
    let input = r#"{"contents":["text one",{"language":"ts","value":"x"}]}"#;
    let v: Hover = serde_json::from_str(input).unwrap();
    let arr = v.contents.marked_strings.as_ref().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].string.as_deref(), Some("text one"));
    assert_eq!(
        arr[1]
            .marked_string_with_language
            .as_ref()
            .unwrap()
            .language,
        "ts"
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `MarkupContent` reports missing required `value` with Go's
// `errMissing` wording.
// Go: lsp_generated.go:MarkupContent.UnmarshalJSONFrom (missingValue)
#[test]
fn markup_content_requires_value() {
    let err = serde_json::from_str::<MarkupContent>(r#"{"kind":"markdown"}"#).unwrap_err();
    assert!(
        err.to_string()
            .contains("missing required properties: value"),
        "got: {err}"
    );
}

// === Slice 2: StringOrMarkupContent for *.documentation / tooltip ===

// tracer (real RED->GREEN): `CompletionItem.documentation` is now the typed
// union `StringOrMarkupContent` (was raw `serde_json::Value`). The
// `MarkupContent` object arm is reachable and round-trips byte-for-byte.
// Go: lsp_generated.go:CompletionItem (Documentation *StringOrMarkupContent)
#[test]
fn completion_item_documentation_markup_content_variant() {
    let input = r#"{"label":"x","documentation":{"kind":"plaintext","value":"docs"}}"#;
    let v: CompletionItem = serde_json::from_str(input).unwrap();
    let doc = v.documentation.as_ref().unwrap();
    let mc = doc.markup_content.as_ref().unwrap();
    assert_eq!(mc.kind, MarkupKind::PLAIN_TEXT);
    assert_eq!(mc.value, "docs");
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: the bare-string arm of `StringOrMarkupContent` on
// `CompletionItem.documentation`.
// Go: lsp_generated.go:StringOrMarkupContent.UnmarshalJSONFrom (string case)
#[test]
fn completion_item_documentation_string_variant() {
    let input = r#"{"label":"x","documentation":"plain docs"}"#;
    let v: CompletionItem = serde_json::from_str(input).unwrap();
    assert_eq!(
        v.documentation.as_ref().unwrap().string.as_deref(),
        Some("plain docs")
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `InlayHint.tooltip` shares the `StringOrMarkupContent` shape
// and round-trips with a markup object.
// Go: lsp_generated.go:InlayHint (Tooltip *StringOrMarkupContent)
#[test]
fn inlay_hint_tooltip_markup_variant() {
    let input = r#"{"position":{"line":0,"character":0},"label":"x","tooltip":{"kind":"markdown","value":"t"}}"#;
    let v: InlayHint = serde_json::from_str(input).unwrap();
    assert_eq!(
        v.tooltip
            .as_ref()
            .unwrap()
            .markup_content
            .as_ref()
            .unwrap()
            .value,
        "t"
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `InlayHintLabelPart.tooltip` shares the same union and
// round-trips with a plain string.
// Go: lsp_generated.go:InlayHintLabelPart (Tooltip *StringOrMarkupContent)
#[test]
fn inlay_hint_label_part_tooltip_string_variant() {
    let input = r#"{"value":"lp","tooltip":"hello"}"#;
    let v: InlayHintLabelPart = serde_json::from_str(input).unwrap();
    assert_eq!(v.tooltip.as_ref().unwrap().string.as_deref(), Some("hello"));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// === Slice 3: ClientInfo + TraceValue + ServerInfo ===

// tracer (real RED->GREEN): `InitializeParams.client_info` is now the typed
// `ClientInfo { name, version? }` and `trace` is the `TraceValue` string enum
// (was raw `serde_json::Value`). Both decode to typed fields and round-trip.
// Go: lsp_generated.go:InitializeParams (ClientInfo *ClientInfo, Trace *TraceValue)
#[test]
fn initialize_params_client_info_and_trace_typed() {
    let input = r#"{"processId":null,"clientInfo":{"name":"vscode","version":"1.9"},"rootUri":null,"capabilities":{},"trace":"verbose"}"#;
    let v: InitializeParams = serde_json::from_str(input).unwrap();
    let ci = v.client_info.as_ref().unwrap();
    assert_eq!(ci.name, "vscode");
    assert_eq!(ci.version.as_deref(), Some("1.9"));
    assert_eq!(v.trace, Some(TraceValue::VERBOSE));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `TraceValue` const values match the LSP spec literals and
// (de)serialize as a plain JSON string; unknown values round-trip raw.
// Go: lsp_generated.go:TraceValue (TraceValueOff/Messages/Verbose)
#[test]
fn trace_value_const_values_and_serde() {
    assert_eq!(TraceValue::OFF.0, "off");
    assert_eq!(TraceValue::MESSAGES.0, "messages");
    assert_eq!(TraceValue::VERBOSE.0, "verbose");
    assert_eq!(
        serde_json::to_string(&TraceValue::MESSAGES).unwrap(),
        r#""messages""#
    );
    let v: TraceValue = serde_json::from_str(r#""future""#).unwrap();
    assert_eq!(v.0, "future");
}

// green-on-arrival: `ClientInfo` rejects an explicit `null` version (Go's
// `errNull` guard) and reports a missing required `name`.
// Go: lsp_generated.go:ClientInfo.UnmarshalJSONFrom (version errNull / missingName)
#[test]
fn client_info_version_null_rejected_and_name_required() {
    let err = serde_json::from_str::<ClientInfo>(r#"{"name":"c","version":null}"#).unwrap_err();
    assert!(
        err.to_string()
            .contains("null value is not allowed for field \"version\""),
        "got: {err}"
    );
    let err2 = serde_json::from_str::<ClientInfo>(r#"{"version":"1"}"#).unwrap_err();
    assert!(
        err2.to_string()
            .contains("missing required properties: name"),
        "got: {err2}"
    );
}

// green-on-arrival: `InitializeResult.server_info` is the typed `ServerInfo`;
// a populated value round-trips byte-for-byte and the field is omitted by
// default.
// Go: lsp_generated.go:InitializeResult (ServerInfo *ServerInfo)
#[test]
fn initialize_result_server_info_typed() {
    let input = r#"{"capabilities":{},"serverInfo":{"name":"tsgo","version":"0.1"}}"#;
    let v: InitializeResult = serde_json::from_str(input).unwrap();
    let si = v.server_info.as_ref().unwrap();
    assert_eq!(si.name, "tsgo");
    assert_eq!(si.version.as_deref(), Some("0.1"));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);

    let bare = r#"{"capabilities":{}}"#;
    let v2: InitializeResult = serde_json::from_str(bare).unwrap();
    assert!(v2.server_info.is_none());
    assert_eq!(serde_json::to_string(&v2).unwrap(), bare);
}

// green-on-arrival: `InitializeParams.trace` rejects an explicit `null`
// (Go's `errNull` guard).
// Go: lsp_generated.go:InitializeParams.UnmarshalJSONFrom (trace errNull)
#[test]
fn initialize_params_trace_null_rejected() {
    assert_null_rejected::<InitializeParams>(
        r#"{"processId":null,"rootUri":null,"capabilities":{},"trace":null}"#,
        "trace",
    );
}

// === Slice 4: StringOrNull (rootPath) + WorkspaceFoldersOrNull ===

// tracer (real RED->GREEN): `InitializeParams.root_path` is now the typed
// `StringOrNull` and `workspace_folders` is `WorkspaceFoldersOrNull`
// (`[]WorkspaceFolder | null`); both were raw `serde_json::Value`. The
// populated arms decode to typed fields and round-trip byte-for-byte.
// Go: lsp_generated.go:InitializeParams (RootPath *StringOrNull, WorkspaceFolders *WorkspaceFoldersOrNull)
#[test]
fn initialize_params_root_path_and_workspace_folders_typed() {
    let input = r#"{"processId":null,"rootPath":"/ws","rootUri":null,"capabilities":{},"workspaceFolders":[{"uri":"file:///a","name":"a"}]}"#;
    let v: InitializeParams = serde_json::from_str(input).unwrap();
    assert_eq!(v.root_path.as_ref().unwrap().string.as_deref(), Some("/ws"));
    let folders = v
        .workspace_folders
        .as_ref()
        .unwrap()
        .workspace_folders
        .as_ref()
        .unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].uri.0, "file:///a");
    assert_eq!(folders[0].name, "a");
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `StringOrNull` accepts `null` (-> `None`) and a string, and
// rejects any other JSON kind; both arms round-trip.
// Go: lsp_generated.go:StringOrNull.UnmarshalJSONFrom
#[test]
fn string_or_null_dispatch() {
    let n: StringOrNull = serde_json::from_str("null").unwrap();
    assert!(n.string.is_none());
    assert_eq!(serde_json::to_string(&n).unwrap(), "null");
    let s: StringOrNull = serde_json::from_str(r#""abc""#).unwrap();
    assert_eq!(s.string.as_deref(), Some("abc"));
    assert_eq!(serde_json::to_string(&s).unwrap(), r#""abc""#);
    assert!(serde_json::from_str::<StringOrNull>("42").is_err());
}

// green-on-arrival: `WorkspaceFoldersOrNull` accepts `null` (-> `None`) and an
// array; the `null` form is what `workspaceFolders: null` decodes to and it
// re-serializes as `null`.
// Go: lsp_generated.go:WorkspaceFoldersOrNull.UnmarshalJSONFrom
#[test]
fn workspace_folders_or_null_dispatch() {
    let n: WorkspaceFoldersOrNull = serde_json::from_str("null").unwrap();
    assert!(n.workspace_folders.is_none());
    assert_eq!(serde_json::to_string(&n).unwrap(), "null");
    let a: WorkspaceFoldersOrNull =
        serde_json::from_str(r#"[{"uri":"file:///a","name":"a"}]"#).unwrap();
    assert_eq!(a.workspace_folders.as_ref().unwrap().len(), 1);
    assert!(serde_json::from_str::<WorkspaceFoldersOrNull>("42").is_err());
}

// === Slice 5: CompletionItem labelDetails / tags / insertTextMode ===

// tracer (real RED->GREEN): `CompletionItem.label_details`,
// `tags` (`Vec<CompletionItemTag>`) and `insert_text_mode` (`InsertTextMode`)
// are now typed (were raw `serde_json::Value`). All decode to typed fields and
// round-trip byte-for-byte.
// Go: lsp_generated.go:CompletionItem (LabelDetails/Tags/InsertTextMode)
#[test]
fn completion_item_label_details_tags_insert_text_mode_typed() {
    let input = r#"{"label":"x","labelDetails":{"detail":" detail","description":"desc"},"tags":[1],"insertTextMode":2}"#;
    let v: CompletionItem = serde_json::from_str(input).unwrap();
    let ld = v.label_details.as_ref().unwrap();
    assert_eq!(ld.detail.as_deref(), Some(" detail"));
    assert_eq!(ld.description.as_deref(), Some("desc"));
    assert_eq!(
        v.tags.as_ref().unwrap(),
        &vec![CompletionItemTag::DEPRECATED]
    );
    assert_eq!(v.insert_text_mode, Some(InsertTextMode::ADJUST_INDENTATION));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `CompletionItemLabelDetails` omits both fields by default
// and rejects an explicit `null` detail (Go's `errNull` guard).
// Go: lsp_generated.go:CompletionItemLabelDetails
#[test]
fn completion_item_label_details_default_and_null() {
    assert_eq!(
        serde_json::to_string(&CompletionItemLabelDetails::default()).unwrap(),
        "{}"
    );
    let err = serde_json::from_str::<CompletionItemLabelDetails>(r#"{"detail":null}"#).unwrap_err();
    assert!(
        err.to_string()
            .contains("null value is not allowed for field \"detail\""),
        "got: {err}"
    );
}

// green-on-arrival: `CompletionItem.tags` rejects an explicit `null` (Go's
// `errNull` guard) on the now-typed `Vec<CompletionItemTag>`.
// Go: lsp_generated.go:CompletionItem.UnmarshalJSONFrom (tags errNull)
#[test]
fn completion_item_tags_null_rejected() {
    assert_null_rejected::<CompletionItem>(r#"{"label":"x","tags":null}"#, "tags");
}

// === Slice 6: Command for CompletionItem.command + InlayHintLabelPart.command ===

// tracer (real RED->GREEN): `CompletionItem.command` is now the typed
// `Command { title, tooltip?, command, arguments? }` (was raw
// `serde_json::Value`). The `arguments` element type stays `LSPAny`
// (`serde_json::Value`), faithful to Go's `*[]any`. Round-trip is byte-for-byte
// in Go declaration order (title, tooltip, command, arguments).
// Go: lsp_generated.go:CompletionItem (Command *Command) / Command
#[test]
fn completion_item_command_typed() {
    let input =
        r#"{"label":"x","command":{"title":"Save","command":"ts.save","arguments":[1,"a"]}}"#;
    let v: CompletionItem = serde_json::from_str(input).unwrap();
    let cmd = v.command.as_ref().unwrap();
    assert_eq!(cmd.title, "Save");
    assert_eq!(cmd.command, "ts.save");
    assert_eq!(
        cmd.arguments.as_ref().unwrap(),
        &vec![serde_json::json!(1), serde_json::json!("a")]
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `Command` reports missing required `title`/`command`.
// Go: lsp_generated.go:Command.UnmarshalJSONFrom (missingTitle/missingCommand)
#[test]
fn command_requires_title_and_command() {
    let err = serde_json::from_str::<Command>(r#"{"title":"t"}"#).unwrap_err();
    assert!(
        err.to_string()
            .contains("missing required properties: command"),
        "got: {err}"
    );
}

// green-on-arrival: `InlayHintLabelPart.command` shares the typed `Command` and
// round-trips (no `arguments` is omitted).
// Go: lsp_generated.go:InlayHintLabelPart (Command *Command)
#[test]
fn inlay_hint_label_part_command_typed() {
    let input = r#"{"value":"lp","command":{"title":"Go","command":"ts.go"}}"#;
    let v: InlayHintLabelPart = serde_json::from_str(input).unwrap();
    let cmd = v.command.as_ref().unwrap();
    assert_eq!(cmd.title, "Go");
    assert_eq!(cmd.command, "ts.go");
    assert!(cmd.arguments.is_none());
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// === Slice 7: CallHierarchyItem for incoming-call item / from ===

// tracer (real RED->GREEN): `CallHierarchyIncomingCall.from` is now the typed
// `CallHierarchyItem` (was raw `serde_json::Value`). Decoding reaches the typed
// name/kind/uri/selectionRange and the round-trip is byte-for-byte in Go field
// order. The `data` carrier stays raw (`CallHierarchyItemData` deferred).
// Go: lsp_generated.go:CallHierarchyIncomingCall (From CallHierarchyItem) / CallHierarchyItem
#[test]
fn call_hierarchy_incoming_call_from_typed() {
    let input = r#"{"from":{"name":"f","kind":12,"uri":"file:///a.ts","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"selectionRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}},"fromRanges":[]}"#;
    let v: CallHierarchyIncomingCall = serde_json::from_str(input).unwrap();
    assert_eq!(v.from.name, "f");
    assert_eq!(v.from.kind, SymbolKind::FUNCTION);
    assert_eq!(v.from.uri.0, "file:///a.ts");
    assert_eq!(v.from.selection_range.end.character, 1);
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `CallHierarchyItem` reports a missing required `name`/`kind`
// with Go's `errMissing` wording.
// Go: lsp_generated.go:CallHierarchyItem.UnmarshalJSONFrom (missingName)
#[test]
fn call_hierarchy_item_requires_name() {
    let err = serde_json::from_str::<CallHierarchyItem>(
        r#"{"kind":12,"uri":"file:///a","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"selectionRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}"#,
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("missing required properties: name"),
        "got: {err}"
    );
}

// green-on-arrival: `CallHierarchyIncomingCallsParams.item` is the typed
// `CallHierarchyItem` and decodes a populated value (still rejecting `null`,
// covered by `null_rejected_callhierarchy_incoming_params_item`).
// Go: lsp_generated.go:CallHierarchyIncomingCallsParams (Item CallHierarchyItem)
#[test]
fn call_hierarchy_incoming_params_item_typed() {
    let input = r#"{"item":{"name":"f","kind":12,"uri":"file:///a","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"selectionRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}}"#;
    let v: CallHierarchyIncomingCallsParams = serde_json::from_str(input).unwrap();
    assert_eq!(v.item.name, "f");
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// === Slice 8: Create/Rename/DeleteFileOptions for resource-op options ===

// tracer (real RED->GREEN): `CreateFile.options` is now the typed
// `CreateFileOptions { overwrite?, ignoreIfExists? }` (was raw
// `serde_json::Value`). It decodes to typed fields and round-trips.
// Go: lsp_generated.go:CreateFile (Options *CreateFileOptions) / CreateFileOptions
#[test]
fn create_file_options_typed() {
    let input = r#"{"kind":"create","uri":"file:///a","options":{"overwrite":true,"ignoreIfExists":false}}"#;
    let v: CreateFile = serde_json::from_str(input).unwrap();
    let o = v.options.as_ref().unwrap();
    assert_eq!(o.overwrite, Some(true));
    assert_eq!(o.ignore_if_exists, Some(false));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `RenameFile.options` is the typed `RenameFileOptions`.
// Go: lsp_generated.go:RenameFile (Options *RenameFileOptions)
#[test]
fn rename_file_options_typed() {
    let input = r#"{"kind":"rename","oldUri":"file:///a","newUri":"file:///b","options":{"overwrite":true}}"#;
    let v: RenameFile = serde_json::from_str(input).unwrap();
    assert_eq!(v.options.as_ref().unwrap().overwrite, Some(true));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: `DeleteFile.options` is the typed `DeleteFileOptions`
// (distinct field names `recursive`/`ignoreIfNotExists`).
// Go: lsp_generated.go:DeleteFile (Options *DeleteFileOptions) / DeleteFileOptions
#[test]
fn delete_file_options_typed() {
    let input = r#"{"kind":"delete","uri":"file:///a","options":{"recursive":true,"ignoreIfNotExists":true}}"#;
    let v: DeleteFile = serde_json::from_str(input).unwrap();
    let o = v.options.as_ref().unwrap();
    assert_eq!(o.recursive, Some(true));
    assert_eq!(o.ignore_if_not_exists, Some(true));
    assert_eq!(serde_json::to_string(&v).unwrap(), input);
}

// green-on-arrival: the file-options structs omit every field by default
// (omitzero invariant).
// Go: lsp_generated.go:CreateFileOptions/RenameFileOptions/DeleteFileOptions
#[test]
fn file_options_default_serialize_empty() {
    assert_eq!(
        serde_json::to_string(&CreateFileOptions::default()).unwrap(),
        "{}"
    );
    assert_eq!(
        serde_json::to_string(&RenameFileOptions::default()).unwrap(),
        "{}"
    );
    assert_eq!(
        serde_json::to_string(&DeleteFileOptions::default()).unwrap(),
        "{}"
    );
}

// === AnnotatedTextEdit / SnippetTextEdit / TextEditOrAnnotatedTextEditOrSnippetTextEdit ===

// Tracer (real RED -> GREEN): `AnnotatedTextEdit` does not exist yet, so this
// test fails to compile until the type is added; once present it round-trips
// the three required fields in Go declaration order (range, newText,
// annotationId).
// Go: lsp_generated.go:AnnotatedTextEdit
#[test]
fn annotated_text_edit_round_trip() {
    let json = r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"abc","annotationId":"ann1"}"#;
    let v: AnnotatedTextEdit = serde_json::from_str(json).unwrap();
    assert_eq!(v.new_text, "abc");
    assert_eq!(v.annotation_id, "ann1");
    assert_eq!(v.range.end.character, 3);
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
}

// green-on-arrival: `annotationId` is required (Go errMissing), distinguishing
// `AnnotatedTextEdit` from a plain `TextEdit`.
// Go: lsp_generated.go:AnnotatedTextEdit (missingAnnotationId)
#[test]
fn annotated_text_edit_requires_annotation_id() {
    let err = serde_json::from_str::<AnnotatedTextEdit>(
        r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"x"}"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("annotationId"), "got: {err}");
}

// Tracer (real RED -> GREEN): `SnippetTextEdit` and `StringValue` do not exist
// yet. Once present, an edit with a snippet round-trips in Go declaration order
// (range, snippet, annotationId) with the nested `StringValue` discriminator.
// Go: lsp_generated.go:SnippetTextEdit / StringValue
#[test]
fn snippet_text_edit_round_trip() {
    let json = r#"{"range":{"start":{"line":1,"character":0},"end":{"line":1,"character":0}},"snippet":{"kind":"snippet","value":"$1"},"annotationId":"ann2"}"#;
    let v: SnippetTextEdit = serde_json::from_str(json).unwrap();
    assert_eq!(v.snippet.value, "$1");
    assert_eq!(v.annotation_id.as_deref(), Some("ann2"));
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
}

// green-on-arrival: `annotationId` is optional (omitzero) and omitted when None.
// Go: lsp_generated.go:SnippetTextEdit (AnnotationId *string `json:",omitzero"`)
#[test]
fn snippet_text_edit_omits_optional_annotation_id() {
    let json = r#"{"range":{"start":{"line":2,"character":1},"end":{"line":2,"character":1}},"snippet":{"kind":"snippet","value":"${1:name}"}}"#;
    let v: SnippetTextEdit = serde_json::from_str(json).unwrap();
    assert_eq!(v.annotation_id, None);
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
}

// green-on-arrival: `snippet` is required (Go errMissing).
// Go: lsp_generated.go:SnippetTextEdit (missingSnippet)
#[test]
fn snippet_text_edit_requires_snippet() {
    let err = serde_json::from_str::<SnippetTextEdit>(
        r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("snippet"), "got: {err}");
}

// green-on-arrival: a `null` snippet is rejected (Go errNull guard / reqnn).
// Go: lsp_generated.go:SnippetTextEdit (errNull("snippet"))
#[test]
fn snippet_text_edit_rejects_null_snippet() {
    assert_null_rejected::<SnippetTextEdit>(
        r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"snippet":null}"#,
        "snippet",
    );
}

// green-on-arrival: `StringValue` round-trips and its `kind` is fixed to the
// `"snippet"` literal; a wrong literal is rejected.
// Go: lsp_generated.go:StringValue / StringLiteralSnippet
#[test]
fn string_value_round_trip_and_rejects_wrong_kind() {
    let json = r#"{"kind":"snippet","value":"$0"}"#;
    let v: StringValue = serde_json::from_str(json).unwrap();
    assert_eq!(v.value, "$0");
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
    assert!(serde_json::from_str::<StringValue>(r#"{"kind":"literal","value":"x"}"#).is_err());
}

// Tracer (real RED -> GREEN): the 3-arm union does not exist yet. Go dispatches
// by scanning for a `snippet` key first, so an object carrying `snippet` decodes
// to the `SnippetTextEdit` arm.
// Go: lsp_generated.go:TextEditOrAnnotatedTextEditOrSnippetTextEdit (case 0: snippet)
#[test]
fn text_edit_union_snippet_variant() {
    let v: TextEditOrAnnotatedTextEditOrSnippetTextEdit = serde_json::from_str(
        r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"snippet":{"kind":"snippet","value":"$1"}}"#,
    )
    .unwrap();
    assert!(v.text_edit.is_none());
    assert!(v.annotated_text_edit.is_none());
    assert_eq!(v.snippet_text_edit.unwrap().snippet.value, "$1");
}

// An object with `annotationId` but no `snippet` decodes to the
// `AnnotatedTextEdit` arm (Go case 1: annotationId).
// Go: lsp_generated.go:TextEditOrAnnotatedTextEditOrSnippetTextEdit (case 1)
#[test]
fn text_edit_union_annotated_variant() {
    let v: TextEditOrAnnotatedTextEditOrSnippetTextEdit = serde_json::from_str(
        r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":2}},"newText":"hi","annotationId":"a1"}"#,
    )
    .unwrap();
    assert!(v.text_edit.is_none());
    assert!(v.snippet_text_edit.is_none());
    assert_eq!(v.annotated_text_edit.unwrap().annotation_id, "a1");
}

// An object with neither `snippet` nor `annotationId` decodes to the plain
// `TextEdit` arm (Go default case).
// Go: lsp_generated.go:TextEditOrAnnotatedTextEditOrSnippetTextEdit (default)
#[test]
fn text_edit_union_plain_variant() {
    let v: TextEditOrAnnotatedTextEditOrSnippetTextEdit = serde_json::from_str(
        r#"{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"newText":"z"}"#,
    )
    .unwrap();
    assert!(v.annotated_text_edit.is_none());
    assert!(v.snippet_text_edit.is_none());
    assert_eq!(v.text_edit.unwrap().new_text, "z");
}

// green-on-arrival: serialization emits the single set variant's object, and a
// union with zero/multiple arms set is an error (Go assertOnlyOne).
// Go: lsp_generated.go:TextEditOrAnnotatedTextEditOrSnippetTextEdit.MarshalJSONTo
#[test]
fn text_edit_union_serializes_set_variant() {
    let only_annotated = TextEditOrAnnotatedTextEditOrSnippetTextEdit {
        annotated_text_edit: Some(AnnotatedTextEdit {
            range: Range::default(),
            new_text: "x".to_string(),
            annotation_id: "a".to_string(),
        }),
        ..Default::default()
    };
    let out = serde_json::to_string(&only_annotated).unwrap();
    assert!(out.contains(r#""annotationId":"a""#), "got: {out}");
    assert!(out.contains(r#""newText":"x""#), "got: {out}");

    assert!(
        serde_json::to_string(&TextEditOrAnnotatedTextEditOrSnippetTextEdit::default()).is_err()
    );
}

// Headline (real RED -> GREEN): `TextDocumentEdit.edits` is tightened from raw
// `Vec<serde_json::Value>` to the typed 3-arm union, so element fields are
// typed and mixed plain/snippet edits round-trip byte-for-byte.
// Go: lsp_generated.go:TextDocumentEdit (Edits []TextEditOrAnnotatedTextEditOrSnippetTextEdit)
#[test]
fn text_document_edit_edits_typed() {
    let json = r#"{"textDocument":{"uri":"file:///a.ts","version":1},"edits":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"x"},{"range":{"start":{"line":1,"character":0},"end":{"line":1,"character":0}},"snippet":{"kind":"snippet","value":"$0"}}]}"#;
    let v: TextDocumentEdit = serde_json::from_str(json).unwrap();
    assert_eq!(v.edits.len(), 2);
    assert_eq!(v.edits[0].text_edit.as_ref().unwrap().new_text, "x");
    assert_eq!(
        v.edits[1].snippet_text_edit.as_ref().unwrap().snippet.value,
        "$0"
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
}

// green-on-arrival: an annotated edit inside `edits` decodes to its typed arm
// and round-trips, confirming the swap covers all three union arms in the vec.
// Go: lsp_generated.go:TextDocumentEdit (AnnotatedTextEdit element)
#[test]
fn text_document_edit_annotated_edit_in_vec() {
    let json = r#"{"textDocument":{"uri":"file:///b.ts","version":2},"edits":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"newText":"q","annotationId":"a7"}]}"#;
    let v: TextDocumentEdit = serde_json::from_str(json).unwrap();
    assert_eq!(
        v.edits[0]
            .annotated_text_edit
            .as_ref()
            .unwrap()
            .annotation_id,
        "a7"
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
}

// Headline (real RED -> GREEN): the `textDocument/_vs_onAutoInsert` response
// item (the result of the auto-close-JSX-tag provider) carries the text-edit
// format and the snippet edit; it round-trips byte-for-byte in Go field order.
// Go: lsp_generated.go:VsOnAutoInsertResponseItem
#[test]
fn vs_on_auto_insert_response_item_round_trip() {
    let v = VsOnAutoInsertResponseItem {
        vs_text_edit_format: InsertTextFormat::SNIPPET,
        vs_text_edit: TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 5,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            },
            new_text: "$0</div>".to_string(),
        },
    };
    let json = r#"{"_vs_textEditFormat":2,"_vs_textEdit":{"range":{"start":{"line":0,"character":5},"end":{"line":0,"character":5}},"newText":"$0</div>"}}"#;
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
    let back: VsOnAutoInsertResponseItem = serde_json::from_str(json).unwrap();
    assert_eq!(v, back);
}

// green-on-arrival: `_vs_textEdit` is a non-nullable pointer in Go (it rejects
// an explicit `null`), and both fields are required.
// Go: lsp_generated.go:VsOnAutoInsertResponseItem.UnmarshalJSONFrom
#[test]
fn vs_on_auto_insert_response_item_rejects_null_text_edit_and_requires_fields() {
    let null_edit = r#"{"_vs_textEditFormat":2,"_vs_textEdit":null}"#;
    assert!(serde_json::from_str::<VsOnAutoInsertResponseItem>(null_edit).is_err());
    let missing_edit = r#"{"_vs_textEditFormat":2}"#;
    assert!(serde_json::from_str::<VsOnAutoInsertResponseItem>(missing_edit).is_err());
    let missing_format = r#"{"_vs_textEdit":{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"x"}}"#;
    assert!(serde_json::from_str::<VsOnAutoInsertResponseItem>(missing_format).is_err());
}
