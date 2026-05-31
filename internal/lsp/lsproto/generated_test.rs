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
