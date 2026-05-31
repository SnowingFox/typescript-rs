use super::*;

// These resolved capability types have no Go `*_test.go` (Go produces them via
// `Resolve()` and rarely (de)serializes them). Per PORTING §8.6 each public
// type still gets a behavior-level serde test; expected shapes are the Go
// `json:"...,omitzero"` tags and the LSP spec literals.

fn round_trip<T>(value: T)
where
    T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_vec(&value).unwrap();
    let back: T = serde_json::from_slice(&json).unwrap();
    assert_eq!(value, back);
}

// === omitzero serialization (Go `json:",omitzero"` on value fields) ===

// Go: lsp_generated.go:ResolvedDidChangeConfigurationClientCapabilities
// A resolved capability with a single zero-valued bool serializes to `{}`.
#[test]
fn resolved_did_change_config_default_serializes_empty() {
    let v = ResolvedDidChangeConfigurationClientCapabilities::default();
    assert_eq!(serde_json::to_string(&v).unwrap(), "{}");
}

// A set bool field is emitted under its camelCase JSON key.
#[test]
fn resolved_did_change_config_set_bool_serializes() {
    let v = ResolvedDidChangeConfigurationClientCapabilities {
        dynamic_registration: true,
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"dynamicRegistration":true}"#
    );
}

// Absent keys decode to the zero value; unknown keys are ignored.
#[test]
fn resolved_did_change_config_deserialize_missing_and_unknown() {
    let v: ResolvedDidChangeConfigurationClientCapabilities =
        serde_json::from_str(r#"{"unknownFuture": 42}"#).unwrap();
    assert!(!v.dynamic_registration);
}

// === nested-struct omitzero (an all-default nested struct is omitted) ===

// Go: lsp_generated.go:ResolvedShowMessageRequestClientCapabilities
#[test]
fn nested_all_default_is_omitted() {
    let v = ResolvedShowMessageRequestClientCapabilities::default();
    assert_eq!(serde_json::to_string(&v).unwrap(), "{}");
}

#[test]
fn nested_non_default_is_emitted() {
    let v = ResolvedShowMessageRequestClientCapabilities {
        message_action_item: ResolvedClientShowMessageActionItemOptions {
            additional_properties_support: true,
        },
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"messageActionItem":{"additionalPropertiesSupport":true}}"#
    );
    round_trip(v);
}

// === Vec<enum> field (empty omitted; non-empty serialized as JSON array) ===

// Go: lsp_generated.go:ResolvedClientSymbolKindOptions
#[test]
fn vec_enum_empty_is_omitted() {
    let v = ResolvedClientSymbolKindOptions::default();
    assert_eq!(serde_json::to_string(&v).unwrap(), "{}");
}

#[test]
fn vec_enum_non_empty_serializes_as_array() {
    let v = ResolvedClientSymbolKindOptions {
        value_set: vec![SymbolKind::FILE, SymbolKind::FUNCTION],
    };
    assert_eq!(serde_json::to_string(&v).unwrap(), r#"{"valueSet":[1,12]}"#);
    round_trip(v);
}

// === u32 scalar omitzero ===

// Go: lsp_generated.go:ResolvedFoldingRangeClientCapabilities
#[test]
fn u32_zero_is_omitted_nonzero_emitted() {
    let zero = ResolvedFoldingRangeClientCapabilities::default();
    assert_eq!(serde_json::to_string(&zero).unwrap(), "{}");

    let v = ResolvedFoldingRangeClientCapabilities {
        range_limit: 5000,
        line_folding_only: true,
        ..Default::default()
    };
    let s = serde_json::to_string(&v).unwrap();
    assert!(s.contains(r#""rangeLimit":5000"#), "got: {s}");
    assert!(s.contains(r#""lineFoldingOnly":true"#), "got: {s}");
    assert!(!s.contains("dynamicRegistration"), "got: {s}");
    round_trip(v);
}

// === string scalar omitzero + new string enum (direct field + Vec) ===

// Go: lsp_generated.go:ResolvedWorkspaceEditClientCapabilities
#[test]
fn workspace_edit_string_enum_and_vec() {
    let v = ResolvedWorkspaceEditClientCapabilities {
        document_changes: true,
        resource_operations: vec![
            ResourceOperationKind::CREATE,
            ResourceOperationKind::RENAME,
            ResourceOperationKind::DELETE,
        ],
        failure_handling: FailureHandlingKind::TEXT_ONLY_TRANSACTIONAL,
        ..Default::default()
    };
    let s = serde_json::to_string(&v).unwrap();
    assert!(s.contains(r#""documentChanges":true"#), "got: {s}");
    assert!(
        s.contains(r#""resourceOperations":["create","rename","delete"]"#),
        "got: {s}"
    );
    assert!(
        s.contains(r#""failureHandling":"textOnlyTransactional""#),
        "got: {s}"
    );
    // Defaulted bool/enum/nested fields are omitted.
    assert!(!s.contains("normalizesLineEndings"), "got: {s}");
    assert!(!s.contains("changeAnnotationSupport"), "got: {s}");
    round_trip(v);
}

// Default string enum ("") is omitted by omitzero.
#[test]
fn workspace_edit_default_failure_handling_omitted() {
    let v = ResolvedWorkspaceEditClientCapabilities::default();
    assert_eq!(serde_json::to_string(&v).unwrap(), "{}");
}

// === new int enum as a direct omitzero field ===

// Go: lsp_generated.go:ResolvedRenameClientCapabilities
#[test]
fn rename_int_enum_field() {
    let zero = ResolvedRenameClientCapabilities::default();
    assert_eq!(serde_json::to_string(&zero).unwrap(), "{}");

    let v = ResolvedRenameClientCapabilities {
        prepare_support: true,
        prepare_support_default_behavior: PrepareSupportDefaultBehavior::IDENTIFIER,
        ..Default::default()
    };
    let s = serde_json::to_string(&v).unwrap();
    assert!(s.contains(r#""prepareSupport":true"#), "got: {s}");
    assert!(
        s.contains(r#""prepareSupportDefaultBehavior":1"#),
        "got: {s}"
    );
    round_trip(v);
}

// === new enums: predefined values + JSON round-trip ===

#[test]
fn string_enum_values_and_serde() {
    assert_eq!(ResourceOperationKind::CREATE.0, "create");
    assert_eq!(ResourceOperationKind::RENAME.0, "rename");
    assert_eq!(ResourceOperationKind::DELETE.0, "delete");
    assert_eq!(FailureHandlingKind::ABORT.0, "abort");
    assert_eq!(FailureHandlingKind::TRANSACTIONAL.0, "transactional");
    assert_eq!(
        FailureHandlingKind::TEXT_ONLY_TRANSACTIONAL.0,
        "textOnlyTransactional"
    );
    assert_eq!(FailureHandlingKind::UNDO.0, "undo");
    assert_eq!(MarkupKind::PLAIN_TEXT.0, "plaintext");
    assert_eq!(MarkupKind::MARKDOWN.0, "markdown");
    assert_eq!(CodeActionKind::EMPTY.0, "");
    assert_eq!(CodeActionKind::QUICK_FIX.0, "quickfix");
    assert_eq!(CodeActionKind::REFACTOR_EXTRACT.0, "refactor.extract");
    assert_eq!(
        CodeActionKind::SOURCE_ORGANIZE_IMPORTS.0,
        "source.organizeImports"
    );
    assert_eq!(TokenFormat::RELATIVE.0, "relative");

    assert_eq!(
        serde_json::to_string(&MarkupKind::MARKDOWN).unwrap(),
        r#""markdown""#
    );
    let k: CodeActionKind = serde_json::from_str(r#""quickfix""#).unwrap();
    assert_eq!(k, CodeActionKind::QUICK_FIX);
    // Unknown values round-trip as their raw string.
    let other: ResourceOperationKind = serde_json::from_str(r#""future""#).unwrap();
    assert_eq!(other.0, "future");
}

#[test]
fn int_enum_values_and_serde() {
    assert_eq!(SymbolTag::DEPRECATED.0, 1);
    assert_eq!(CompletionItemTag::DEPRECATED.0, 1);
    assert_eq!(InsertTextMode::AS_IS.0, 1);
    assert_eq!(InsertTextMode::ADJUST_INDENTATION.0, 2);
    assert_eq!(CodeActionTag::LLM_GENERATED.0, 1);
    assert_eq!(PrepareSupportDefaultBehavior::IDENTIFIER.0, 1);

    assert_eq!(
        serde_json::to_string(&InsertTextMode::ADJUST_INDENTATION).unwrap(),
        "2"
    );
    let m: InsertTextMode = serde_json::from_str("1").unwrap();
    assert_eq!(m, InsertTextMode::AS_IS);
}

// === union types ===

// Go: lsp_generated.go:EmptyObject (serializes to `{}`)
#[test]
fn empty_object_serializes_to_braces() {
    assert_eq!(serde_json::to_string(&EmptyObject).unwrap(), "{}");
    let _v: EmptyObject = serde_json::from_str(r#"{"ignored": 1}"#).unwrap();
}

// Go: lsp_generated.go:BooleanOrEmptyObject
#[test]
fn boolean_or_empty_object_bool_variant() {
    let v: BooleanOrEmptyObject = serde_json::from_str("true").unwrap();
    assert_eq!(v.boolean, Some(true));
    assert!(v.empty_object.is_none());
    assert_eq!(serde_json::to_string(&v).unwrap(), "true");
}

#[test]
fn boolean_or_empty_object_object_variant() {
    let v: BooleanOrEmptyObject = serde_json::from_str("{}").unwrap();
    assert!(v.boolean.is_none());
    assert_eq!(v.empty_object, Some(EmptyObject));
    assert_eq!(serde_json::to_string(&v).unwrap(), "{}");
}

#[test]
fn boolean_or_empty_object_rejects_string() {
    assert!(serde_json::from_str::<BooleanOrEmptyObject>(r#""x""#).is_err());
}

// Go: lsp_generated.go:BooleanOrClientSemanticTokensRequestFullDelta
#[test]
fn boolean_or_full_delta_bool_variant() {
    let v: BooleanOrClientSemanticTokensRequestFullDelta = serde_json::from_str("false").unwrap();
    assert_eq!(v.boolean, Some(false));
    assert_eq!(serde_json::to_string(&v).unwrap(), "false");
}

#[test]
fn boolean_or_full_delta_object_variant() {
    let v: BooleanOrClientSemanticTokensRequestFullDelta =
        serde_json::from_str(r#"{"delta": true}"#).unwrap();
    assert!(v.boolean.is_none());
    assert_eq!(
        v.client_semantic_tokens_request_full_delta,
        Some(ClientSemanticTokensRequestFullDelta { delta: Some(true) })
    );
    assert_eq!(serde_json::to_string(&v).unwrap(), r#"{"delta":true}"#);
}

// Go: lsp_generated.go:ResolvedClientSemanticTokensRequestOptions (union fields)
#[test]
fn semantic_tokens_request_options_round_trip() {
    let v = ResolvedClientSemanticTokensRequestOptions {
        range: BooleanOrEmptyObject {
            boolean: Some(true),
            empty_object: None,
        },
        full: BooleanOrClientSemanticTokensRequestFullDelta {
            boolean: None,
            client_semantic_tokens_request_full_delta: Some(ClientSemanticTokensRequestFullDelta {
                delta: Some(true),
            }),
        },
    };
    let s = serde_json::to_string(&v).unwrap();
    assert_eq!(s, r#"{"range":true,"full":{"delta":true}}"#);
    round_trip(v);
}

// === top-level ResolvedClientCapabilities ===

// Go: lsp_generated.go:ResolvedClientCapabilities (zero value -> `{}`)
#[test]
fn resolved_client_capabilities_default_empty() {
    assert_eq!(
        serde_json::to_string(&ResolvedClientCapabilities::default()).unwrap(),
        "{}"
    );
}

// Only a populated nested group is emitted; all sibling groups are omitted
// (recursive omitzero).
#[test]
fn resolved_client_capabilities_only_window_group() {
    let v = ResolvedClientCapabilities {
        window: ResolvedWindowClientCapabilities {
            work_done_progress: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"window":{"workDoneProgress":true}}"#
    );
}

// The Visual Studio extension scalars use their `_vs_*` JSON keys.
#[test]
fn resolved_client_capabilities_vs_scalars() {
    let v = ResolvedClientCapabilities {
        vs_supports_visual_studio_extensions: true,
        vs_supported_snippet_version: 3,
        vs_supports_diagnostic_requests: true,
        ..Default::default()
    };
    let s = serde_json::to_string(&v).unwrap();
    assert!(
        s.contains(r#""_vs_supportsVisualStudioExtensions":true"#),
        "got: {s}"
    );
    assert!(s.contains(r#""_vs_supportedSnippetVersion":3"#), "got: {s}");
    assert!(
        s.contains(r#""_vs_supportsDiagnosticRequests":true"#),
        "got: {s}"
    );
    // A zeroed VS scalar is omitted.
    assert!(!s.contains("_vs_supportsIconExtensions"), "got: {s}");
    round_trip(v);
}

// A representative, deeply nested capabilities object round-trips and exposes
// fields by value (the point of the "resolved" view).
#[test]
fn resolved_client_capabilities_deep_round_trip() {
    let v = ResolvedClientCapabilities {
        workspace: ResolvedWorkspaceClientCapabilities {
            apply_edit: true,
            workspace_edit: ResolvedWorkspaceEditClientCapabilities {
                document_changes: true,
                resource_operations: vec![ResourceOperationKind::CREATE],
                ..Default::default()
            },
            ..Default::default()
        },
        text_document: ResolvedTextDocumentClientCapabilities {
            completion: ResolvedCompletionClientCapabilities {
                completion_item: ResolvedClientCompletionItemOptions {
                    snippet_support: true,
                    documentation_format: vec![MarkupKind::MARKDOWN, MarkupKind::PLAIN_TEXT],
                    ..Default::default()
                },
                ..Default::default()
            },
            semantic_tokens: ResolvedSemanticTokensClientCapabilities {
                token_types: vec!["namespace".to_string(), "type".to_string()],
                formats: vec![TokenFormat::RELATIVE],
                ..Default::default()
            },
            ..Default::default()
        },
        general: ResolvedGeneralClientCapabilities {
            position_encodings: vec![PositionEncodingKind::UTF16, PositionEncodingKind::UTF8],
            ..Default::default()
        },
        vs_supported_snippet_version: 1,
        ..Default::default()
    };
    round_trip(v.clone());

    // Deep field access works without nil checks.
    assert!(v.text_document.completion.completion_item.snippet_support);
    assert_eq!(
        v.text_document
            .completion
            .completion_item
            .documentation_format[0],
        MarkupKind::MARKDOWN
    );
}

// Deserializing a realistic client-capabilities subset populates nested values.
#[test]
fn resolved_client_capabilities_from_client_json() {
    let json = r#"{
        "workspace": {"applyEdit": true, "workspaceEdit": {"documentChanges": true}},
        "textDocument": {
            "completion": {"completionItem": {"snippetSupport": true, "insertReplaceSupport": true}},
            "hover": {"contentFormat": ["markdown", "plaintext"]}
        },
        "general": {"positionEncodings": ["utf-16", "utf-8"]},
        "_vs_supportedSnippetVersion": 2,
        "unknownClientField": {"x": 1}
    }"#;
    let v: ResolvedClientCapabilities = serde_json::from_str(json).unwrap();
    assert!(v.workspace.apply_edit);
    assert!(v.workspace.workspace_edit.document_changes);
    assert!(v.text_document.completion.completion_item.snippet_support);
    assert!(
        v.text_document
            .completion
            .completion_item
            .insert_replace_support
    );
    assert_eq!(
        v.text_document.hover.content_format,
        vec![MarkupKind::MARKDOWN, MarkupKind::PLAIN_TEXT]
    );
    assert_eq!(
        v.general.position_encodings,
        vec![PositionEncodingKind::UTF16, PositionEncodingKind::UTF8]
    );
    assert_eq!(v.vs_supported_snippet_version, 2);
}

// === per-type coverage: every resolved struct's zero value -> `{}` ===

// PORTING §8.6: each public resolved type gets at least one behavior test.
// The omitzero invariant (all-default -> `{}`) holds for every resolved struct
// and is the load-bearing serialization behavior, so it is asserted for each.
fn assert_default_empty<T: Default + Serialize>() {
    assert_eq!(serde_json::to_string(&T::default()).unwrap(), "{}");
}

#[test]
fn every_resolved_type_default_serializes_empty() {
    assert_default_empty::<ResolvedChangeAnnotationsSupportOptions>();
    assert_default_empty::<ResolvedWorkspaceEditClientCapabilities>();
    assert_default_empty::<ResolvedDidChangeConfigurationClientCapabilities>();
    assert_default_empty::<ResolvedDidChangeWatchedFilesClientCapabilities>();
    assert_default_empty::<ResolvedClientSymbolKindOptions>();
    assert_default_empty::<ResolvedClientSymbolTagOptions>();
    assert_default_empty::<ResolvedClientSymbolResolveOptions>();
    assert_default_empty::<ResolvedWorkspaceSymbolClientCapabilities>();
    assert_default_empty::<ResolvedExecuteCommandClientCapabilities>();
    assert_default_empty::<ResolvedSemanticTokensWorkspaceClientCapabilities>();
    assert_default_empty::<ResolvedCodeLensWorkspaceClientCapabilities>();
    assert_default_empty::<ResolvedFileOperationClientCapabilities>();
    assert_default_empty::<ResolvedInlineValueWorkspaceClientCapabilities>();
    assert_default_empty::<ResolvedInlayHintWorkspaceClientCapabilities>();
    assert_default_empty::<ResolvedDiagnosticWorkspaceClientCapabilities>();
    assert_default_empty::<ResolvedFoldingRangeWorkspaceClientCapabilities>();
    assert_default_empty::<ResolvedTextDocumentContentClientCapabilities>();
    assert_default_empty::<ResolvedWorkspaceClientCapabilities>();
    assert_default_empty::<ResolvedTextDocumentSyncClientCapabilities>();
    assert_default_empty::<ResolvedTextDocumentFilterClientCapabilities>();
    assert_default_empty::<ResolvedCompletionItemTagOptions>();
    assert_default_empty::<ResolvedClientCompletionItemResolveOptions>();
    assert_default_empty::<ResolvedClientCompletionItemInsertTextModeOptions>();
    assert_default_empty::<ResolvedClientCompletionItemOptions>();
    assert_default_empty::<ResolvedClientCompletionItemOptionsKind>();
    assert_default_empty::<ResolvedCompletionListCapabilities>();
    assert_default_empty::<ResolvedCompletionClientCapabilities>();
    assert_default_empty::<ResolvedHoverClientCapabilities>();
    assert_default_empty::<ResolvedClientSignatureParameterInformationOptions>();
    assert_default_empty::<ResolvedClientSignatureInformationOptions>();
    assert_default_empty::<ResolvedSignatureHelpClientCapabilities>();
    assert_default_empty::<ResolvedDeclarationClientCapabilities>();
    assert_default_empty::<ResolvedDefinitionClientCapabilities>();
    assert_default_empty::<ResolvedTypeDefinitionClientCapabilities>();
    assert_default_empty::<ResolvedImplementationClientCapabilities>();
    assert_default_empty::<ResolvedReferenceClientCapabilities>();
    assert_default_empty::<ResolvedDocumentHighlightClientCapabilities>();
    assert_default_empty::<ResolvedDocumentSymbolClientCapabilities>();
    assert_default_empty::<ResolvedClientCodeActionKindOptions>();
    assert_default_empty::<ResolvedClientCodeActionLiteralOptions>();
    assert_default_empty::<ResolvedClientCodeActionResolveOptions>();
    assert_default_empty::<ResolvedCodeActionTagOptions>();
    assert_default_empty::<ResolvedCodeActionClientCapabilities>();
    assert_default_empty::<ResolvedClientCodeLensResolveOptions>();
    assert_default_empty::<ResolvedCodeLensClientCapabilities>();
    assert_default_empty::<ResolvedDocumentLinkClientCapabilities>();
    assert_default_empty::<ResolvedDocumentColorClientCapabilities>();
    assert_default_empty::<ResolvedDocumentFormattingClientCapabilities>();
    assert_default_empty::<ResolvedDocumentRangeFormattingClientCapabilities>();
    assert_default_empty::<ResolvedDocumentOnTypeFormattingClientCapabilities>();
    assert_default_empty::<ResolvedRenameClientCapabilities>();
    assert_default_empty::<ResolvedClientFoldingRangeKindOptions>();
    assert_default_empty::<ResolvedClientFoldingRangeOptions>();
    assert_default_empty::<ResolvedFoldingRangeClientCapabilities>();
    assert_default_empty::<ResolvedSelectionRangeClientCapabilities>();
    assert_default_empty::<ResolvedClientDiagnosticsTagOptions>();
    assert_default_empty::<ResolvedPublishDiagnosticsClientCapabilities>();
    assert_default_empty::<ResolvedCallHierarchyClientCapabilities>();
    assert_default_empty::<ResolvedClientSemanticTokensRequestOptions>();
    assert_default_empty::<ResolvedSemanticTokensClientCapabilities>();
    assert_default_empty::<ResolvedLinkedEditingRangeClientCapabilities>();
    assert_default_empty::<ResolvedMonikerClientCapabilities>();
    assert_default_empty::<ResolvedTypeHierarchyClientCapabilities>();
    assert_default_empty::<ResolvedInlineValueClientCapabilities>();
    assert_default_empty::<ResolvedClientInlayHintResolveOptions>();
    assert_default_empty::<ResolvedInlayHintClientCapabilities>();
    assert_default_empty::<ResolvedDiagnosticClientCapabilities>();
    assert_default_empty::<ResolvedInlineCompletionClientCapabilities>();
    assert_default_empty::<ResolvedTextDocumentClientCapabilities>();
    assert_default_empty::<ResolvedClientShowMessageActionItemOptions>();
    assert_default_empty::<ResolvedShowMessageRequestClientCapabilities>();
    assert_default_empty::<ResolvedShowDocumentClientCapabilities>();
    assert_default_empty::<ResolvedWindowClientCapabilities>();
    assert_default_empty::<ResolvedStaleRequestSupportOptions>();
    assert_default_empty::<ResolvedRegularExpressionsClientCapabilities>();
    assert_default_empty::<ResolvedMarkdownClientCapabilities>();
    assert_default_empty::<ResolvedGeneralClientCapabilities>();
    assert_default_empty::<ResolvedClientCapabilities>();
    assert_default_empty::<ClientSemanticTokensRequestFullDelta>();
    assert_default_empty::<EmptyObject>();
}
