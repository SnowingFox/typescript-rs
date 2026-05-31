//! Behavior tests for the pointer-based `ClientCapabilities` request tree and
//! `ClientCapabilities::resolve()`.
//!
//! Go has no `*_test.go` covering `Resolve()` (it is produced internally and
//! rarely (de)serialized), so these are behavior-level tests written per
//! PORTING §8.6: deserialize a representative client-capabilities JSON, call
//! `resolve()`, and assert the resolved fields against Go's resolve/default
//! semantics (`derefOr` => present value, absent => zero value).

use super::*;

// Used only by the semantic-tokens `full` union assertion below.
use crate::ClientSemanticTokensRequestFullDelta;

// Go: internal/lsp/lsproto/lsp_generated.go:(*ClientCapabilities).Resolve (_vs_ scalars)
#[test]
fn resolve_maps_vs_supported_snippet_version_scalar() {
    let cc: ClientCapabilities =
        serde_json::from_str(r#"{"_vs_supportedSnippetVersion":3}"#).unwrap();
    let resolved = cc.resolve();
    assert_eq!(resolved.vs_supported_snippet_version, 3);
}

// === Window group ===

// Go: internal/lsp/lsproto/lsp_generated.go:(*WindowClientCapabilities).resolve
#[test]
fn resolve_window_present_reflects_capabilities() {
    let cc: ClientCapabilities = serde_json::from_str(
        r#"{"window":{"workDoneProgress":true,"showDocument":{"support":true}}}"#,
    )
    .unwrap();
    let resolved = cc.resolve();
    assert!(resolved.window.work_done_progress);
    assert!(resolved.window.show_document.support);
}

// Go: internal/lsp/lsproto/lsp_generated.go:(*WindowClientCapabilities).resolve (nil receiver)
// green-on-arrival: absent group resolves to the all-default value struct.
#[test]
fn resolve_window_absent_is_default() {
    let cc: ClientCapabilities = serde_json::from_str("{}").unwrap();
    assert_eq!(
        cc.resolve().window,
        ResolvedWindowClientCapabilities::default()
    );
}

// === General group ===

// Go: internal/lsp/lsproto/lsp_generated.go:(*GeneralClientCapabilities).resolve
#[test]
fn resolve_general_present_reflects_capabilities() {
    let cc: ClientCapabilities = serde_json::from_str(
        r#"{"general":{"markdown":{"parser":"marked","version":"1.0"},"positionEncodings":["utf-8"]}}"#,
    )
    .unwrap();
    let resolved = cc.resolve();
    assert_eq!(resolved.general.markdown.parser, "marked");
    assert_eq!(resolved.general.markdown.version, "1.0");
    assert_eq!(
        resolved.general.position_encodings,
        vec![PositionEncodingKind::UTF8]
    );
}

// === Workspace group ===

// Go: internal/lsp/lsproto/lsp_generated.go:(*WorkspaceClientCapabilities).resolve
#[test]
fn resolve_workspace_present_reflects_capabilities() {
    let cc: ClientCapabilities = serde_json::from_str(
        r#"{"workspace":{
            "applyEdit":true,
            "workspaceEdit":{"resourceOperations":["create","rename"],"failureHandling":"textOnlyTransactional"},
            "symbol":{"symbolKind":{"valueSet":[1,12]}},
            "fileOperations":{"didCreate":true}
        }}"#,
    )
    .unwrap();
    let r = cc.resolve();
    assert!(r.workspace.apply_edit);
    assert_eq!(
        r.workspace.workspace_edit.failure_handling,
        FailureHandlingKind::TEXT_ONLY_TRANSACTIONAL
    );
    assert_eq!(
        r.workspace.workspace_edit.resource_operations,
        vec![ResourceOperationKind::CREATE, ResourceOperationKind::RENAME]
    );
    assert_eq!(
        r.workspace.symbol.symbol_kind.value_set,
        vec![SymbolKind::FILE, SymbolKind::FUNCTION]
    );
    assert!(r.workspace.file_operations.did_create);
}

// === TextDocument group ===

// Go: internal/lsp/lsproto/lsp_generated.go:(*TextDocumentClientCapabilities).resolve
#[test]
fn resolve_text_document_present_reflects_capabilities() {
    let cc: ClientCapabilities = serde_json::from_str(
        r#"{"textDocument":{
            "completion":{"completionItem":{"snippetSupport":true},"insertTextMode":2},
            "hover":{"contentFormat":["markdown","plaintext"]},
            "rename":{"prepareSupportDefaultBehavior":1},
            "semanticTokens":{"requests":{"full":{"delta":true}}},
            "foldingRange":{"rangeLimit":5000}
        }}"#,
    )
    .unwrap();
    let r = cc.resolve();
    assert!(r.text_document.completion.completion_item.snippet_support);
    assert_eq!(
        r.text_document.completion.insert_text_mode,
        InsertTextMode::ADJUST_INDENTATION
    );
    assert_eq!(
        r.text_document.hover.content_format,
        vec![MarkupKind::MARKDOWN, MarkupKind::PLAIN_TEXT]
    );
    assert_eq!(
        r.text_document.rename.prepare_support_default_behavior,
        PrepareSupportDefaultBehavior::IDENTIFIER
    );
    assert_eq!(
        r.text_document.semantic_tokens.requests.full,
        BooleanOrClientSemanticTokensRequestFullDelta {
            boolean: None,
            client_semantic_tokens_request_full_delta: Some(ClientSemanticTokensRequestFullDelta {
                delta: Some(true),
            }),
        }
    );
    assert_eq!(r.text_document.folding_range.range_limit, 5000);
}

// green-on-arrival: absent groups resolve to the all-default value structs.
#[test]
fn resolve_absent_groups_are_default() {
    let cc: ClientCapabilities = serde_json::from_str("{}").unwrap();
    let r = cc.resolve();
    assert_eq!(r.workspace, ResolvedWorkspaceClientCapabilities::default());
    assert_eq!(r.general, ResolvedGeneralClientCapabilities::default());
    assert_eq!(
        r.text_document,
        ResolvedTextDocumentClientCapabilities::default()
    );
}

// === End-to-end Resolve() ===

// Go: internal/lsp/lsproto/lsp_generated.go:(*ClientCapabilities).Resolve (empty)
// An empty client-capabilities object resolves to the all-default resolved tree.
#[test]
fn resolve_empty_is_all_default() {
    let cc: ClientCapabilities = serde_json::from_str("{}").unwrap();
    assert_eq!(cc.resolve(), ResolvedClientCapabilities::default());
}

// A representative client-capabilities subset resolves into the matching deep
// fields across all four groups in a single pass.
#[test]
fn resolve_real_client_capabilities_subset() {
    let cc: ClientCapabilities = serde_json::from_str(
        r#"{
            "workspace":{"applyEdit":true,"configuration":true},
            "textDocument":{"hover":{"dynamicRegistration":true}},
            "window":{"workDoneProgress":true},
            "general":{"markdown":{"parser":"marked"}},
            "_vs_supportsVisualStudioExtensions":true
        }"#,
    )
    .unwrap();
    let r = cc.resolve();
    assert!(r.workspace.apply_edit);
    assert!(r.workspace.configuration);
    assert!(r.text_document.hover.dynamic_registration);
    assert!(r.window.work_done_progress);
    assert_eq!(r.general.markdown.parser, "marked");
    assert!(r.vs_supports_visual_studio_extensions);
}

// The request tree round-trips through serde (public API stays usable for
// `InitializeParams.capabilities`).
#[test]
fn client_capabilities_serde_round_trip() {
    let cc = ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            hover: Some(HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::MARKDOWN]),
                ..Default::default()
            }),
            ..Default::default()
        }),
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(true),
            ..Default::default()
        }),
        vs_supported_snippet_version: Some(3),
        ..Default::default()
    };
    let json = serde_json::to_string(&cc).unwrap();
    let back: ClientCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(cc, back);
    // resolve() agrees before and after the round-trip.
    assert_eq!(cc.resolve(), back.resolve());
}
