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
