//! Port of the pointer-based `ClientCapabilities` request tree and its
//! `(*ClientCapabilities).Resolve()` from Go
//! `internal/lsp/lsproto/lsp_generated.go`.
//!
//! [`ClientCapabilities`] is the over-the-wire "request" shape sent by an LSP
//! client in `InitializeParams.capabilities`: every nested capability group is
//! an optional pointer (Go `*T` with `json:",omitzero"`). [`ClientCapabilities::resolve`]
//! flattens it into the value-typed [`crate::ResolvedClientCapabilities`] tree
//! (defined in `resolved.rs`), filling defaults for absent capabilities so deep
//! fields can be read without nil checks.
//!
//! # Divergence from Go
//! - Go models each optional field as a typed pointer (`*bool`, `*[]T`,
//!   `*SubStruct`) plus a few non-pointer required fields. This port uniformly
//!   models every optional field as `Option<T>` (see [`request_object!`]); the
//!   handful of Go non-pointer fields (e.g. `ShowDocumentClientCapabilities.support`)
//!   become `Option<T>` too. This is behavior-preserving for `resolve()` (absent
//!   → zero value) and only differs in deserialize strictness / explicit-empty
//!   round-trip, which these request types do not rely on.
//! - Like the resolved tree, the per-field `errNull` null-rejection text is not
//!   reproduced; an explicit `null` for a sub-object fails with serde's default
//!   message instead of Go's exact `errNull`.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    BooleanOrClientSemanticTokensRequestFullDelta, BooleanOrEmptyObject, CodeActionKind,
    CodeActionTag, CompletionItemKind, CompletionItemTag, DiagnosticTag, FailureHandlingKind,
    FoldingRangeKind, InsertTextMode, MarkupKind, PositionEncodingKind,
    PrepareSupportDefaultBehavior, ResolvedCallHierarchyClientCapabilities,
    ResolvedChangeAnnotationsSupportOptions, ResolvedClientCapabilities,
    ResolvedClientCodeActionKindOptions, ResolvedClientCodeActionLiteralOptions,
    ResolvedClientCodeActionResolveOptions, ResolvedClientCodeLensResolveOptions,
    ResolvedClientCompletionItemInsertTextModeOptions, ResolvedClientCompletionItemOptions,
    ResolvedClientCompletionItemOptionsKind, ResolvedClientCompletionItemResolveOptions,
    ResolvedClientDiagnosticsTagOptions, ResolvedClientFoldingRangeKindOptions,
    ResolvedClientFoldingRangeOptions, ResolvedClientInlayHintResolveOptions,
    ResolvedClientSemanticTokensRequestOptions, ResolvedClientShowMessageActionItemOptions,
    ResolvedClientSignatureInformationOptions, ResolvedClientSignatureParameterInformationOptions,
    ResolvedClientSymbolKindOptions, ResolvedClientSymbolResolveOptions,
    ResolvedClientSymbolTagOptions, ResolvedCodeActionClientCapabilities,
    ResolvedCodeActionTagOptions, ResolvedCodeLensClientCapabilities,
    ResolvedCodeLensWorkspaceClientCapabilities, ResolvedCompletionClientCapabilities,
    ResolvedCompletionItemTagOptions, ResolvedCompletionListCapabilities,
    ResolvedDeclarationClientCapabilities, ResolvedDefinitionClientCapabilities,
    ResolvedDiagnosticClientCapabilities, ResolvedDiagnosticWorkspaceClientCapabilities,
    ResolvedDidChangeConfigurationClientCapabilities,
    ResolvedDidChangeWatchedFilesClientCapabilities, ResolvedDocumentColorClientCapabilities,
    ResolvedDocumentFormattingClientCapabilities, ResolvedDocumentHighlightClientCapabilities,
    ResolvedDocumentLinkClientCapabilities, ResolvedDocumentOnTypeFormattingClientCapabilities,
    ResolvedDocumentRangeFormattingClientCapabilities, ResolvedDocumentSymbolClientCapabilities,
    ResolvedExecuteCommandClientCapabilities, ResolvedFileOperationClientCapabilities,
    ResolvedFoldingRangeClientCapabilities, ResolvedFoldingRangeWorkspaceClientCapabilities,
    ResolvedGeneralClientCapabilities, ResolvedHoverClientCapabilities,
    ResolvedImplementationClientCapabilities, ResolvedInlayHintClientCapabilities,
    ResolvedInlayHintWorkspaceClientCapabilities, ResolvedInlineCompletionClientCapabilities,
    ResolvedInlineValueClientCapabilities, ResolvedInlineValueWorkspaceClientCapabilities,
    ResolvedLinkedEditingRangeClientCapabilities, ResolvedMarkdownClientCapabilities,
    ResolvedMonikerClientCapabilities, ResolvedPublishDiagnosticsClientCapabilities,
    ResolvedReferenceClientCapabilities, ResolvedRegularExpressionsClientCapabilities,
    ResolvedRenameClientCapabilities, ResolvedSelectionRangeClientCapabilities,
    ResolvedSemanticTokensClientCapabilities, ResolvedSemanticTokensWorkspaceClientCapabilities,
    ResolvedShowDocumentClientCapabilities, ResolvedShowMessageRequestClientCapabilities,
    ResolvedSignatureHelpClientCapabilities, ResolvedStaleRequestSupportOptions,
    ResolvedTextDocumentClientCapabilities, ResolvedTextDocumentContentClientCapabilities,
    ResolvedTextDocumentFilterClientCapabilities, ResolvedTextDocumentSyncClientCapabilities,
    ResolvedTypeDefinitionClientCapabilities, ResolvedTypeHierarchyClientCapabilities,
    ResolvedWindowClientCapabilities, ResolvedWorkspaceClientCapabilities,
    ResolvedWorkspaceEditClientCapabilities, ResolvedWorkspaceSymbolClientCapabilities,
    ResourceOperationKind, SymbolKind, SymbolTag, TokenFormat,
};

/// Generates a pointer-based "request capability" object type plus its
/// `resolve()` mapping into the matching `Resolved*` value struct.
///
/// Each field is modelled as `Option<T>` (Go's optional pointer / `,omitzero`):
///
/// - **Serialize**: a field is written only when `Some`.
/// - **Deserialize**: present keys decode into `Some`, absent keys stay `None`,
///   unknown keys are ignored (Go's decoder behavior).
/// - **`resolve()`**: maps into `$resolved`. A `val` field becomes
///   `self.field.clone().unwrap_or_default()` (Go `derefOr`); a `sub` field
///   becomes `self.field.as_ref().map(|x| x.resolve()).unwrap_or_default()`
///   (Go `v.Field.resolve()`).
///
/// The listed fields must exactly cover `$resolved`'s fields, with matching
/// names, so the generated literal is complete.
macro_rules! request_object {
    (
        $(#[$smeta:meta])*
        $name:ident => $resolved:ident {
            $( [$doc:literal] $kind:ident $rust:ident : $ty:ty => $json:literal , )*
        }
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            $( #[doc = $doc] pub $rust : ::core::option::Option<$ty>, )*
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut __m = serializer.serialize_map(None)?;
                $(
                    if let Some(ref __v) = self.$rust {
                        __m.serialize_entry($json, __v)?;
                    }
                )*
                __m.end()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct __V;
                impl<'de> Visitor<'de> for __V {
                    type Value = $name;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str(concat!("a ", stringify!($name), " object"))
                    }
                    fn visit_map<A: MapAccess<'de>>(
                        self,
                        mut map: A,
                    ) -> Result<Self::Value, A::Error> {
                        let mut __out = $name::default();
                        while let Some(__k) = map.next_key::<String>()? {
                            match __k.as_str() {
                                $( $json => { __out.$rust = Some(map.next_value()?); } )*
                                _ => {
                                    map.next_value::<de::IgnoredAny>()?;
                                }
                            }
                        }
                        Ok(__out)
                    }
                }
                deserializer.deserialize_map(__V)
            }
        }

        impl $name {
            /// Flattens this request capability into its `Resolved*` value view,
            /// filling defaults for absent fields (Go `resolve()`).
            pub fn resolve(&self) -> $resolved {
                $resolved {
                    $( $rust: request_object!(@res self $kind $rust), )*
                }
            }
        }
    };

    (@res $self:ident val $rust:ident) => {
        $self.$rust.clone().unwrap_or_default()
    };
    (@res $self:ident sub $rust:ident) => {
        $self.$rust.as_ref().map(|__x| __x.resolve()).unwrap_or_default()
    };
}

/// The capabilities provided by an LSP client (request shape).
///
/// This is the pointer-based tree decoded from `InitializeParams.capabilities`.
/// Use [`ClientCapabilities::resolve`] to obtain the flattened
/// [`crate::ResolvedClientCapabilities`] value view.
///
/// # Examples
/// ```
/// let cc: tsgo_lsproto::ClientCapabilities =
///     serde_json::from_str(r#"{"_vs_supportedSnippetVersion":3}"#).unwrap();
/// assert_eq!(cc.resolve().vs_supported_snippet_version, 3);
/// ```
// Go: internal/lsp/lsproto/lsp_generated.go:ClientCapabilities
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientCapabilities {
    /// Workspace-specific client capabilities.
    pub workspace: Option<WorkspaceClientCapabilities>,
    /// Text-document-specific client capabilities.
    pub text_document: Option<TextDocumentClientCapabilities>,
    /// Window-specific client capabilities.
    pub window: Option<WindowClientCapabilities>,
    /// General client capabilities.
    pub general: Option<GeneralClientCapabilities>,
    /// Whether the client supports Visual Studio extensions.
    pub vs_supports_visual_studio_extensions: Option<bool>,
    /// The snippet version supported by the client.
    pub vs_supported_snippet_version: Option<i32>,
    /// Whether the client supports not including text in didOpen notifications.
    pub vs_supports_not_including_text_in_text_document_did_open: Option<bool>,
    /// Whether the client supports icon extensions.
    pub vs_supports_icon_extensions: Option<bool>,
    /// Whether the client supports diagnostic requests.
    pub vs_supports_diagnostic_requests: Option<bool>,
}

impl ClientCapabilities {
    /// Flattens the pointer-based request capabilities into the value-typed
    /// [`crate::ResolvedClientCapabilities`], filling defaults for absent
    /// capability groups and scalars.
    ///
    /// # Examples
    /// ```
    /// let cc = tsgo_lsproto::ClientCapabilities::default();
    /// assert_eq!(cc.resolve(), tsgo_lsproto::ResolvedClientCapabilities::default());
    /// ```
    // Go: internal/lsp/lsproto/lsp_generated.go:(*ClientCapabilities).Resolve
    pub fn resolve(&self) -> ResolvedClientCapabilities {
        ResolvedClientCapabilities {
            workspace: self
                .workspace
                .as_ref()
                .map(|w| w.resolve())
                .unwrap_or_default(),
            text_document: self
                .text_document
                .as_ref()
                .map(|t| t.resolve())
                .unwrap_or_default(),
            window: self
                .window
                .as_ref()
                .map(|w| w.resolve())
                .unwrap_or_default(),
            general: self
                .general
                .as_ref()
                .map(|g| g.resolve())
                .unwrap_or_default(),
            vs_supports_visual_studio_extensions: self
                .vs_supports_visual_studio_extensions
                .unwrap_or_default(),
            vs_supported_snippet_version: self.vs_supported_snippet_version.unwrap_or_default(),
            vs_supports_not_including_text_in_text_document_did_open: self
                .vs_supports_not_including_text_in_text_document_did_open
                .unwrap_or_default(),
            vs_supports_icon_extensions: self.vs_supports_icon_extensions.unwrap_or_default(),
            vs_supports_diagnostic_requests: self
                .vs_supports_diagnostic_requests
                .unwrap_or_default(),
        }
    }
}

impl Serialize for ClientCapabilities {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut m = serializer.serialize_map(None)?;
        if let Some(ref v) = self.workspace {
            m.serialize_entry("workspace", v)?;
        }
        if let Some(ref v) = self.text_document {
            m.serialize_entry("textDocument", v)?;
        }
        if let Some(ref v) = self.window {
            m.serialize_entry("window", v)?;
        }
        if let Some(ref v) = self.general {
            m.serialize_entry("general", v)?;
        }
        if let Some(ref v) = self.vs_supports_visual_studio_extensions {
            m.serialize_entry("_vs_supportsVisualStudioExtensions", v)?;
        }
        if let Some(ref v) = self.vs_supported_snippet_version {
            m.serialize_entry("_vs_supportedSnippetVersion", v)?;
        }
        if let Some(ref v) = self.vs_supports_not_including_text_in_text_document_did_open {
            m.serialize_entry("_vs_supportsNotIncludingTextInTextDocumentDidOpen", v)?;
        }
        if let Some(ref v) = self.vs_supports_icon_extensions {
            m.serialize_entry("_vs_supportsIconExtensions", v)?;
        }
        if let Some(ref v) = self.vs_supports_diagnostic_requests {
            m.serialize_entry("_vs_supportsDiagnosticRequests", v)?;
        }
        m.end()
    }
}

impl<'de> Deserialize<'de> for ClientCapabilities {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ClientCapabilities;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a ClientCapabilities object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = ClientCapabilities::default();
                while let Some(k) = map.next_key::<String>()? {
                    match k.as_str() {
                        "workspace" => {
                            out.workspace = Some(map.next_value()?);
                        }
                        "textDocument" => {
                            out.text_document = Some(map.next_value()?);
                        }
                        "window" => {
                            out.window = Some(map.next_value()?);
                        }
                        "general" => {
                            out.general = Some(map.next_value()?);
                        }
                        "_vs_supportsVisualStudioExtensions" => {
                            out.vs_supports_visual_studio_extensions = Some(map.next_value()?);
                        }
                        "_vs_supportedSnippetVersion" => {
                            out.vs_supported_snippet_version = Some(map.next_value()?);
                        }
                        "_vs_supportsNotIncludingTextInTextDocumentDidOpen" => {
                            out.vs_supports_not_including_text_in_text_document_did_open =
                                Some(map.next_value()?);
                        }
                        "_vs_supportsIconExtensions" => {
                            out.vs_supports_icon_extensions = Some(map.next_value()?);
                        }
                        "_vs_supportsDiagnosticRequests" => {
                            out.vs_supports_diagnostic_requests = Some(map.next_value()?);
                        }
                        _ => {
                            map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(out)
            }
        }
        deserializer.deserialize_map(V)
    }
}

// === Window capability subtree ===

request_object! {
    /// Capabilities specific to the `MessageActionItem` type (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientShowMessageActionItemOptions
    ClientShowMessageActionItemOptions => ResolvedClientShowMessageActionItemOptions {
        ["Whether the client supports additional attributes preserved on the action item."]
        val additional_properties_support: bool => "additionalPropertiesSupport",
    }
}

request_object! {
    /// Show-message-request client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ShowMessageRequestClientCapabilities
    ShowMessageRequestClientCapabilities => ResolvedShowMessageRequestClientCapabilities {
        ["Capabilities specific to the `MessageActionItem` type."]
        sub message_action_item: ClientShowMessageActionItemOptions => "messageActionItem",
    }
}

request_object! {
    /// Show-document client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ShowDocumentClientCapabilities
    ShowDocumentClientCapabilities => ResolvedShowDocumentClientCapabilities {
        ["The client has support for the showDocument request."]
        val support: bool => "support",
    }
}

request_object! {
    /// Window-specific client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:WindowClientCapabilities
    WindowClientCapabilities => ResolvedWindowClientCapabilities {
        ["Whether the client supports server-initiated progress via `window/workDoneProgress/create`."]
        val work_done_progress: bool => "workDoneProgress",
        ["Capabilities specific to the showMessage request."]
        sub show_message: ShowMessageRequestClientCapabilities => "showMessage",
        ["Capabilities specific to the showDocument request."]
        sub show_document: ShowDocumentClientCapabilities => "showDocument",
    }
}

// === Workspace capability subtree ===

request_object! {
    /// Whether the client groups edits with equal labels into tree nodes
    /// (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ChangeAnnotationsSupportOptions
    ChangeAnnotationsSupportOptions => ResolvedChangeAnnotationsSupportOptions {
        ["Whether the client groups edits with equal labels into tree nodes."]
        val groups_on_label: bool => "groupsOnLabel",
    }
}

request_object! {
    /// Capabilities specific to `WorkspaceEdit`s (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceEditClientCapabilities
    WorkspaceEditClientCapabilities => ResolvedWorkspaceEditClientCapabilities {
        ["The client supports versioned document changes in `WorkspaceEdit`s."]
        val document_changes: bool => "documentChanges",
        ["The resource operations the client supports."]
        val resource_operations: Vec<ResourceOperationKind> => "resourceOperations",
        ["The failure-handling strategy if applying the workspace edit fails."]
        val failure_handling: FailureHandlingKind => "failureHandling",
        ["Whether the client normalizes line endings to the client-specific setting."]
        val normalizes_line_endings: bool => "normalizesLineEndings",
        ["Whether the client supports change annotations on text edits and resource operations."]
        sub change_annotation_support: ChangeAnnotationsSupportOptions => "changeAnnotationSupport",
        ["Whether the client supports `WorkspaceEditMetadata` in `WorkspaceEdit`s."]
        val metadata_support: bool => "metadataSupport",
        ["Whether the client supports snippets as text edits."]
        val snippet_edit_support: bool => "snippetEditSupport",
    }
}

request_object! {
    /// Capabilities specific to `workspace/didChangeConfiguration` (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DidChangeConfigurationClientCapabilities
    DidChangeConfigurationClientCapabilities => ResolvedDidChangeConfigurationClientCapabilities {
        ["Did change configuration notification supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Capabilities specific to `workspace/didChangeWatchedFiles` (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DidChangeWatchedFilesClientCapabilities
    DidChangeWatchedFilesClientCapabilities => ResolvedDidChangeWatchedFilesClientCapabilities {
        ["Did change watched files notification supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client has support for relative patterns."]
        val relative_pattern_support: bool => "relativePatternSupport",
    }
}

request_object! {
    /// Specific capabilities for the `SymbolKind` in a symbol request (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientSymbolKindOptions
    ClientSymbolKindOptions => ResolvedClientSymbolKindOptions {
        ["The symbol-kind values the client supports."]
        val value_set: Vec<SymbolKind> => "valueSet",
    }
}

request_object! {
    /// The symbol tags the client supports (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientSymbolTagOptions
    ClientSymbolTagOptions => ResolvedClientSymbolTagOptions {
        ["The tags supported by the client."]
        val value_set: Vec<SymbolTag> => "valueSet",
    }
}

request_object! {
    /// The properties a client can resolve lazily for a workspace symbol
    /// (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientSymbolResolveOptions
    ClientSymbolResolveOptions => ResolvedClientSymbolResolveOptions {
        ["The properties that a client can resolve lazily."]
        val properties: Vec<String> => "properties",
    }
}

request_object! {
    /// Client capabilities for a `workspace/symbol` request (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceSymbolClientCapabilities
    WorkspaceSymbolClientCapabilities => ResolvedWorkspaceSymbolClientCapabilities {
        ["Symbol request supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Specific capabilities for the `SymbolKind` in the request."]
        sub symbol_kind: ClientSymbolKindOptions => "symbolKind",
        ["The client supports tags on `SymbolInformation`."]
        sub tag_support: ClientSymbolTagOptions => "tagSupport",
        ["The client supports partial workspace symbols via resolve."]
        sub resolve_support: ClientSymbolResolveOptions => "resolveSupport",
    }
}

request_object! {
    /// The client capabilities of a `workspace/executeCommand` request (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ExecuteCommandClientCapabilities
    ExecuteCommandClientCapabilities => ResolvedExecuteCommandClientCapabilities {
        ["Execute command supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Workspace-scoped semantic-tokens capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:SemanticTokensWorkspaceClientCapabilities
    SemanticTokensWorkspaceClientCapabilities => ResolvedSemanticTokensWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        val refresh_support: bool => "refreshSupport",
    }
}

request_object! {
    /// Workspace-scoped code-lens capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CodeLensWorkspaceClientCapabilities
    CodeLensWorkspaceClientCapabilities => ResolvedCodeLensWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        val refresh_support: bool => "refreshSupport",
    }
}

request_object! {
    /// Capabilities relating to file-operation events (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:FileOperationClientCapabilities
    FileOperationClientCapabilities => ResolvedFileOperationClientCapabilities {
        ["Whether the client supports dynamic registration for file requests/notifications."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The client supports sending didCreateFiles notifications."]
        val did_create: bool => "didCreate",
        ["The client supports sending willCreateFiles requests."]
        val will_create: bool => "willCreate",
        ["The client supports sending didRenameFiles notifications."]
        val did_rename: bool => "didRename",
        ["The client supports sending willRenameFiles requests."]
        val will_rename: bool => "willRename",
        ["The client supports sending didDeleteFiles notifications."]
        val did_delete: bool => "didDelete",
        ["The client supports sending willDeleteFiles requests."]
        val will_delete: bool => "willDelete",
    }
}

request_object! {
    /// Workspace-scoped inline-value capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:InlineValueWorkspaceClientCapabilities
    InlineValueWorkspaceClientCapabilities => ResolvedInlineValueWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        val refresh_support: bool => "refreshSupport",
    }
}

request_object! {
    /// Workspace-scoped inlay-hint capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:InlayHintWorkspaceClientCapabilities
    InlayHintWorkspaceClientCapabilities => ResolvedInlayHintWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        val refresh_support: bool => "refreshSupport",
    }
}

request_object! {
    /// Workspace-scoped diagnostic capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticWorkspaceClientCapabilities
    DiagnosticWorkspaceClientCapabilities => ResolvedDiagnosticWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        val refresh_support: bool => "refreshSupport",
    }
}

request_object! {
    /// Workspace-scoped folding-range capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:FoldingRangeWorkspaceClientCapabilities
    FoldingRangeWorkspaceClientCapabilities => ResolvedFoldingRangeWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        val refresh_support: bool => "refreshSupport",
    }
}

request_object! {
    /// Client capabilities for a text-document content provider (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentContentClientCapabilities
    TextDocumentContentClientCapabilities => ResolvedTextDocumentContentClientCapabilities {
        ["Text document content provider supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Workspace-specific client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceClientCapabilities
    WorkspaceClientCapabilities => ResolvedWorkspaceClientCapabilities {
        ["The client supports `workspace/applyEdit`."]
        val apply_edit: bool => "applyEdit",
        ["Capabilities specific to `WorkspaceEdit`s."]
        sub workspace_edit: WorkspaceEditClientCapabilities => "workspaceEdit",
        ["Capabilities specific to `workspace/didChangeConfiguration`."]
        sub did_change_configuration: DidChangeConfigurationClientCapabilities => "didChangeConfiguration",
        ["Capabilities specific to `workspace/didChangeWatchedFiles`."]
        sub did_change_watched_files: DidChangeWatchedFilesClientCapabilities => "didChangeWatchedFiles",
        ["Capabilities specific to `workspace/symbol`."]
        sub symbol: WorkspaceSymbolClientCapabilities => "symbol",
        ["Capabilities specific to `workspace/executeCommand`."]
        sub execute_command: ExecuteCommandClientCapabilities => "executeCommand",
        ["The client has support for workspace folders."]
        val workspace_folders: bool => "workspaceFolders",
        ["The client supports `workspace/configuration` requests."]
        val configuration: bool => "configuration",
        ["Workspace-scoped semantic-tokens capabilities."]
        sub semantic_tokens: SemanticTokensWorkspaceClientCapabilities => "semanticTokens",
        ["Workspace-scoped code-lens capabilities."]
        sub code_lens: CodeLensWorkspaceClientCapabilities => "codeLens",
        ["File-operation capabilities."]
        sub file_operations: FileOperationClientCapabilities => "fileOperations",
        ["Workspace-scoped inline-value capabilities."]
        sub inline_value: InlineValueWorkspaceClientCapabilities => "inlineValue",
        ["Workspace-scoped inlay-hint capabilities."]
        sub inlay_hint: InlayHintWorkspaceClientCapabilities => "inlayHint",
        ["Workspace-scoped diagnostic capabilities."]
        sub diagnostics: DiagnosticWorkspaceClientCapabilities => "diagnostics",
        ["Workspace-scoped folding-range capabilities."]
        sub folding_range: FoldingRangeWorkspaceClientCapabilities => "foldingRange",
        ["Capabilities specific to `workspace/textDocumentContent`."]
        sub text_document_content: TextDocumentContentClientCapabilities => "textDocumentContent",
    }
}

// === TextDocument capability subtree ===

request_object! {
    /// Text-document synchronization capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentSyncClientCapabilities
    TextDocumentSyncClientCapabilities => ResolvedTextDocumentSyncClientCapabilities {
        ["Whether text document synchronization supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The client supports sending will-save notifications."]
        val will_save: bool => "willSave",
        ["The client supports sending a will-save-wait-until request."]
        val will_save_wait_until: bool => "willSaveWaitUntil",
        ["The client supports did-save notifications."]
        val did_save: bool => "didSave",
    }
}

request_object! {
    /// Text-document filter capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentFilterClientCapabilities
    TextDocumentFilterClientCapabilities => ResolvedTextDocumentFilterClientCapabilities {
        ["The client supports relative patterns."]
        val relative_pattern_support: bool => "relativePatternSupport",
    }
}

request_object! {
    /// The completion-item tags the client supports (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CompletionItemTagOptions
    CompletionItemTagOptions => ResolvedCompletionItemTagOptions {
        ["The tags supported by the client."]
        val value_set: Vec<CompletionItemTag> => "valueSet",
    }
}

request_object! {
    /// The properties a client can resolve lazily for a completion item
    /// (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCompletionItemResolveOptions
    ClientCompletionItemResolveOptions => ResolvedClientCompletionItemResolveOptions {
        ["The properties that a client can resolve lazily."]
        val properties: Vec<String> => "properties",
    }
}

request_object! {
    /// The insert-text modes the client supports on completion items
    /// (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCompletionItemInsertTextModeOptions
    ClientCompletionItemInsertTextModeOptions => ResolvedClientCompletionItemInsertTextModeOptions {
        ["The insert-text modes supported by the client."]
        val value_set: Vec<InsertTextMode> => "valueSet",
    }
}

request_object! {
    /// `CompletionItem`-specific capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCompletionItemOptions
    ClientCompletionItemOptions => ResolvedClientCompletionItemOptions {
        ["Client supports snippets as insert text."]
        val snippet_support: bool => "snippetSupport",
        ["Client supports commit characters on a completion item."]
        val commit_characters_support: bool => "commitCharactersSupport",
        ["Content formats supported for the documentation property."]
        val documentation_format: Vec<MarkupKind> => "documentationFormat",
        ["Client supports the deprecated property on a completion item."]
        val deprecated_support: bool => "deprecatedSupport",
        ["Client supports the preselect property on a completion item."]
        val preselect_support: bool => "preselectSupport",
        ["Client supports the tag property on a completion item."]
        sub tag_support: CompletionItemTagOptions => "tagSupport",
        ["Client supports insert-replace edits."]
        val insert_replace_support: bool => "insertReplaceSupport",
        ["Properties a client can resolve lazily on a completion item."]
        sub resolve_support: ClientCompletionItemResolveOptions => "resolveSupport",
        ["The client supports the `insertTextMode` property."]
        sub insert_text_mode_support: ClientCompletionItemInsertTextModeOptions => "insertTextModeSupport",
        ["The client has support for completion-item label details."]
        val label_details_support: bool => "labelDetailsSupport",
    }
}

request_object! {
    /// Specific capabilities for the `CompletionItemKind` (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCompletionItemOptionsKind
    ClientCompletionItemOptionsKind => ResolvedClientCompletionItemOptionsKind {
        ["The completion-item-kind values the client supports."]
        val value_set: Vec<CompletionItemKind> => "valueSet",
    }
}

request_object! {
    /// `CompletionList`-specific capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CompletionListCapabilities
    CompletionListCapabilities => ResolvedCompletionListCapabilities {
        ["The item-default property names the client supports."]
        val item_defaults: Vec<String> => "itemDefaults",
        ["Whether the client supports `CompletionList.applyKind`."]
        val apply_kind_support: bool => "applyKindSupport",
    }
}

request_object! {
    /// Completion client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CompletionClientCapabilities
    CompletionClientCapabilities => ResolvedCompletionClientCapabilities {
        ["Whether completion supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["`CompletionItem`-specific capabilities."]
        sub completion_item: ClientCompletionItemOptions => "completionItem",
        ["`CompletionItemKind`-specific capabilities."]
        sub completion_item_kind: ClientCompletionItemOptionsKind => "completionItemKind",
        ["How the client handles whitespace/indentation on multi-line completions."]
        val insert_text_mode: InsertTextMode => "insertTextMode",
        ["The client supports sending additional completion context."]
        val context_support: bool => "contextSupport",
        ["`CompletionList`-specific capabilities."]
        sub completion_list: CompletionListCapabilities => "completionList",
    }
}

request_object! {
    /// Hover client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:HoverClientCapabilities
    HoverClientCapabilities => ResolvedHoverClientCapabilities {
        ["Whether hover supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Content formats supported for the content property."]
        val content_format: Vec<MarkupKind> => "contentFormat",
        ["Whether the client supports the verbosity-level properties."]
        val verbosity_level: bool => "verbosityLevel",
    }
}

request_object! {
    /// Parameter-information capabilities for signature help (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientSignatureParameterInformationOptions
    ClientSignatureParameterInformationOptions => ResolvedClientSignatureParameterInformationOptions {
        ["The client supports processing label offsets instead of a label string."]
        val label_offset_support: bool => "labelOffsetSupport",
    }
}

request_object! {
    /// `SignatureInformation`-specific capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientSignatureInformationOptions
    ClientSignatureInformationOptions => ResolvedClientSignatureInformationOptions {
        ["Content formats supported for the documentation property."]
        val documentation_format: Vec<MarkupKind> => "documentationFormat",
        ["Client capabilities specific to parameter information."]
        sub parameter_information: ClientSignatureParameterInformationOptions => "parameterInformation",
        ["The client supports the `activeParameter` property on `SignatureInformation`."]
        val active_parameter_support: bool => "activeParameterSupport",
        ["The client supports a `null` `activeParameter`."]
        val no_active_parameter_support: bool => "noActiveParameterSupport",
    }
}

request_object! {
    /// Signature-help client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:SignatureHelpClientCapabilities
    SignatureHelpClientCapabilities => ResolvedSignatureHelpClientCapabilities {
        ["Whether signature help supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["`SignatureInformation`-specific capabilities."]
        sub signature_information: ClientSignatureInformationOptions => "signatureInformation",
        ["The client supports sending additional signature-help context."]
        val context_support: bool => "contextSupport",
    }
}

request_object! {
    /// Declaration client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DeclarationClientCapabilities
    DeclarationClientCapabilities => ResolvedDeclarationClientCapabilities {
        ["Whether declaration supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of declaration links."]
        val link_support: bool => "linkSupport",
    }
}

request_object! {
    /// Definition client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DefinitionClientCapabilities
    DefinitionClientCapabilities => ResolvedDefinitionClientCapabilities {
        ["Whether definition supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of definition links."]
        val link_support: bool => "linkSupport",
    }
}

request_object! {
    /// Type-definition client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:TypeDefinitionClientCapabilities
    TypeDefinitionClientCapabilities => ResolvedTypeDefinitionClientCapabilities {
        ["Whether type definition supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of definition links."]
        val link_support: bool => "linkSupport",
    }
}

request_object! {
    /// Implementation client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ImplementationClientCapabilities
    ImplementationClientCapabilities => ResolvedImplementationClientCapabilities {
        ["Whether implementation supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of definition links."]
        val link_support: bool => "linkSupport",
    }
}

request_object! {
    /// References client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ReferenceClientCapabilities
    ReferenceClientCapabilities => ResolvedReferenceClientCapabilities {
        ["Whether references supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Document-highlight client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentHighlightClientCapabilities
    DocumentHighlightClientCapabilities => ResolvedDocumentHighlightClientCapabilities {
        ["Whether document highlight supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Document-symbol client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentSymbolClientCapabilities
    DocumentSymbolClientCapabilities => ResolvedDocumentSymbolClientCapabilities {
        ["Whether document symbol supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Specific capabilities for the `SymbolKind`."]
        sub symbol_kind: ClientSymbolKindOptions => "symbolKind",
        ["The client supports hierarchical document symbols."]
        val hierarchical_document_symbol_support: bool => "hierarchicalDocumentSymbolSupport",
        ["The client supports tags on `SymbolInformation`."]
        sub tag_support: ClientSymbolTagOptions => "tagSupport",
        ["The client supports an additional label presented in the UI."]
        val label_support: bool => "labelSupport",
    }
}

request_object! {
    /// The code-action kinds the client supports (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCodeActionKindOptions
    ClientCodeActionKindOptions => ResolvedClientCodeActionKindOptions {
        ["The code-action kind values the client supports."]
        val value_set: Vec<CodeActionKind> => "valueSet",
    }
}

request_object! {
    /// Code-action literal support (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCodeActionLiteralOptions
    ClientCodeActionLiteralOptions => ResolvedClientCodeActionLiteralOptions {
        ["The code-action kind is supported with the following value set."]
        sub code_action_kind: ClientCodeActionKindOptions => "codeActionKind",
    }
}

request_object! {
    /// The properties a client can resolve lazily for a code action
    /// (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCodeActionResolveOptions
    ClientCodeActionResolveOptions => ResolvedClientCodeActionResolveOptions {
        ["The properties that a client can resolve lazily."]
        val properties: Vec<String> => "properties",
    }
}

request_object! {
    /// The code-action tags the client supports (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CodeActionTagOptions
    CodeActionTagOptions => ResolvedCodeActionTagOptions {
        ["The tags supported by the client."]
        val value_set: Vec<CodeActionTag> => "valueSet",
    }
}

request_object! {
    /// Code-action client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CodeActionClientCapabilities
    CodeActionClientCapabilities => ResolvedCodeActionClientCapabilities {
        ["Whether code action supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The client supports code-action literals."]
        sub code_action_literal_support: ClientCodeActionLiteralOptions => "codeActionLiteralSupport",
        ["Whether code action supports the `isPreferred` property."]
        val is_preferred_support: bool => "isPreferredSupport",
        ["Whether code action supports the `disabled` property."]
        val disabled_support: bool => "disabledSupport",
        ["Whether code action supports the `data` property."]
        val data_support: bool => "dataSupport",
        ["Whether the client supports resolving additional code-action properties."]
        sub resolve_support: ClientCodeActionResolveOptions => "resolveSupport",
        ["Whether the client honors change annotations in code-action edits."]
        val honors_change_annotations: bool => "honorsChangeAnnotations",
        ["Whether the client supports documentation for a class of code actions."]
        val documentation_support: bool => "documentationSupport",
        ["The client supports the tag property on a code action."]
        sub tag_support: CodeActionTagOptions => "tagSupport",
    }
}

request_object! {
    /// The properties a client can resolve lazily for a code lens (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCodeLensResolveOptions
    ClientCodeLensResolveOptions => ResolvedClientCodeLensResolveOptions {
        ["The properties that a client can resolve lazily."]
        val properties: Vec<String> => "properties",
    }
}

request_object! {
    /// Code-lens client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CodeLensClientCapabilities
    CodeLensClientCapabilities => ResolvedCodeLensClientCapabilities {
        ["Whether code lens supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports resolving additional code-lens properties."]
        sub resolve_support: ClientCodeLensResolveOptions => "resolveSupport",
    }
}

request_object! {
    /// Document-link client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentLinkClientCapabilities
    DocumentLinkClientCapabilities => ResolvedDocumentLinkClientCapabilities {
        ["Whether document link supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports the `tooltip` property on `DocumentLink`."]
        val tooltip_support: bool => "tooltipSupport",
    }
}

request_object! {
    /// Document-color client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentColorClientCapabilities
    DocumentColorClientCapabilities => ResolvedDocumentColorClientCapabilities {
        ["Whether document color supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Document-formatting client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentFormattingClientCapabilities
    DocumentFormattingClientCapabilities => ResolvedDocumentFormattingClientCapabilities {
        ["Whether formatting supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Document-range-formatting client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentRangeFormattingClientCapabilities
    DocumentRangeFormattingClientCapabilities => ResolvedDocumentRangeFormattingClientCapabilities {
        ["Whether range formatting supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports formatting multiple ranges at once."]
        val ranges_support: bool => "rangesSupport",
    }
}

request_object! {
    /// Document-on-type-formatting client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentOnTypeFormattingClientCapabilities
    DocumentOnTypeFormattingClientCapabilities => ResolvedDocumentOnTypeFormattingClientCapabilities {
        ["Whether on-type formatting supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Rename client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:RenameClientCapabilities
    RenameClientCapabilities => ResolvedRenameClientCapabilities {
        ["Whether rename supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Client supports testing validity of rename operations before execution."]
        val prepare_support: bool => "prepareSupport",
        ["The default behavior used by the client."]
        val prepare_support_default_behavior: PrepareSupportDefaultBehavior => "prepareSupportDefaultBehavior",
        ["Whether the client honors change annotations in the rename edit."]
        val honors_change_annotations: bool => "honorsChangeAnnotations",
    }
}

request_object! {
    /// The folding-range kinds the client supports (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientFoldingRangeKindOptions
    ClientFoldingRangeKindOptions => ResolvedClientFoldingRangeKindOptions {
        ["The folding-range kind values the client supports."]
        val value_set: Vec<FoldingRangeKind> => "valueSet",
    }
}

request_object! {
    /// Specific options for folding ranges (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientFoldingRangeOptions
    ClientFoldingRangeOptions => ResolvedClientFoldingRangeOptions {
        ["Whether the client supports setting collapsedText on folding ranges."]
        val collapsed_text: bool => "collapsedText",
    }
}

request_object! {
    /// Folding-range client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:FoldingRangeClientCapabilities
    FoldingRangeClientCapabilities => ResolvedFoldingRangeClientCapabilities {
        ["Whether folding range supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["The maximum number of folding ranges the client prefers per document."]
        val range_limit: u32 => "rangeLimit",
        ["Whether the client only supports folding complete lines."]
        val line_folding_only: bool => "lineFoldingOnly",
        ["Specific options for the folding-range kind."]
        sub folding_range_kind: ClientFoldingRangeKindOptions => "foldingRangeKind",
        ["Specific options for the folding range."]
        sub folding_range: ClientFoldingRangeOptions => "foldingRange",
    }
}

request_object! {
    /// Selection-range client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:SelectionRangeClientCapabilities
    SelectionRangeClientCapabilities => ResolvedSelectionRangeClientCapabilities {
        ["Whether selection range supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// The diagnostic tags the client supports (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientDiagnosticsTagOptions
    ClientDiagnosticsTagOptions => ResolvedClientDiagnosticsTagOptions {
        ["The tags supported by the client."]
        val value_set: Vec<DiagnosticTag> => "valueSet",
    }
}

request_object! {
    /// Publish-diagnostics client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:PublishDiagnosticsClientCapabilities
    PublishDiagnosticsClientCapabilities => ResolvedPublishDiagnosticsClientCapabilities {
        ["Whether the client accepts diagnostics with related information."]
        val related_information: bool => "relatedInformation",
        ["The diagnostic tags the client supports."]
        sub tag_support: ClientDiagnosticsTagOptions => "tagSupport",
        ["Whether the client supports a `codeDescription` property."]
        val code_description_support: bool => "codeDescriptionSupport",
        ["Whether the client supports the `data` property on a diagnostic."]
        val data_support: bool => "dataSupport",
        ["Whether the client interprets the `version` property of the notification."]
        val version_support: bool => "versionSupport",
    }
}

request_object! {
    /// Call-hierarchy client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:CallHierarchyClientCapabilities
    CallHierarchyClientCapabilities => ResolvedCallHierarchyClientCapabilities {
        ["Whether call hierarchy supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Which semantic-token requests the client supports (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientSemanticTokensRequestOptions
    ClientSemanticTokensRequestOptions => ResolvedClientSemanticTokensRequestOptions {
        ["The client will send the `.../range` request if the server provides a handler."]
        val range: BooleanOrEmptyObject => "range",
        ["The client will send the `.../full` request if the server provides a handler."]
        val full: BooleanOrClientSemanticTokensRequestFullDelta => "full",
    }
}

request_object! {
    /// Semantic-tokens client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:SemanticTokensClientCapabilities
    SemanticTokensClientCapabilities => ResolvedSemanticTokensClientCapabilities {
        ["Whether semantic tokens supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Which requests the client supports."]
        sub requests: ClientSemanticTokensRequestOptions => "requests",
        ["The token types the client supports."]
        val token_types: Vec<String> => "tokenTypes",
        ["The token modifiers the client supports."]
        val token_modifiers: Vec<String> => "tokenModifiers",
        ["The token formats the client supports."]
        val formats: Vec<TokenFormat> => "formats",
        ["Whether the client supports overlapping tokens."]
        val overlapping_token_support: bool => "overlappingTokenSupport",
        ["Whether the client supports multi-line tokens."]
        val multiline_token_support: bool => "multilineTokenSupport",
        ["Whether the client allows the server to cancel a semantic-token request."]
        val server_cancel_support: bool => "serverCancelSupport",
        ["Whether the client augments existing syntax tokens with semantic tokens."]
        val augments_syntax_tokens: bool => "augmentsSyntaxTokens",
    }
}

request_object! {
    /// Linked-editing-range client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:LinkedEditingRangeClientCapabilities
    LinkedEditingRangeClientCapabilities => ResolvedLinkedEditingRangeClientCapabilities {
        ["Whether linked editing range supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Moniker client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:MonikerClientCapabilities
    MonikerClientCapabilities => ResolvedMonikerClientCapabilities {
        ["Whether moniker supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Type-hierarchy client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:TypeHierarchyClientCapabilities
    TypeHierarchyClientCapabilities => ResolvedTypeHierarchyClientCapabilities {
        ["Whether type hierarchy supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Inline-value client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:InlineValueClientCapabilities
    InlineValueClientCapabilities => ResolvedInlineValueClientCapabilities {
        ["Whether inline value supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// The properties a client can resolve lazily for an inlay hint
    /// (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientInlayHintResolveOptions
    ClientInlayHintResolveOptions => ResolvedClientInlayHintResolveOptions {
        ["The properties that a client can resolve lazily."]
        val properties: Vec<String> => "properties",
    }
}

request_object! {
    /// Inlay-hint client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:InlayHintClientCapabilities
    InlayHintClientCapabilities => ResolvedInlayHintClientCapabilities {
        ["Whether inlay hints support dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Properties a client can resolve lazily on an inlay hint."]
        sub resolve_support: ClientInlayHintResolveOptions => "resolveSupport",
    }
}

request_object! {
    /// Diagnostic (pull-model) client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticClientCapabilities
    DiagnosticClientCapabilities => ResolvedDiagnosticClientCapabilities {
        ["Whether the client accepts diagnostics with related information."]
        val related_information: bool => "relatedInformation",
        ["The diagnostic tags the client supports."]
        sub tag_support: ClientDiagnosticsTagOptions => "tagSupport",
        ["Whether the client supports a `codeDescription` property."]
        val code_description_support: bool => "codeDescriptionSupport",
        ["Whether the client supports the `data` property on a diagnostic."]
        val data_support: bool => "dataSupport",
        ["Whether diagnostic supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports related documents for document diagnostic pulls."]
        val related_document_support: bool => "relatedDocumentSupport",
    }
}

request_object! {
    /// Inline-completion client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:InlineCompletionClientCapabilities
    InlineCompletionClientCapabilities => ResolvedInlineCompletionClientCapabilities {
        ["Whether inline completion supports dynamic registration."]
        val dynamic_registration: bool => "dynamicRegistration",
    }
}

request_object! {
    /// Text-document-specific client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentClientCapabilities
    TextDocumentClientCapabilities => ResolvedTextDocumentClientCapabilities {
        ["Synchronization capabilities."]
        sub synchronization: TextDocumentSyncClientCapabilities => "synchronization",
        ["Filter capabilities."]
        sub filters: TextDocumentFilterClientCapabilities => "filters",
        ["Capabilities specific to `textDocument/completion`."]
        sub completion: CompletionClientCapabilities => "completion",
        ["Capabilities specific to `textDocument/hover`."]
        sub hover: HoverClientCapabilities => "hover",
        ["Capabilities specific to `textDocument/signatureHelp`."]
        sub signature_help: SignatureHelpClientCapabilities => "signatureHelp",
        ["Capabilities specific to `textDocument/declaration`."]
        sub declaration: DeclarationClientCapabilities => "declaration",
        ["Capabilities specific to `textDocument/definition`."]
        sub definition: DefinitionClientCapabilities => "definition",
        ["Capabilities specific to `textDocument/typeDefinition`."]
        sub type_definition: TypeDefinitionClientCapabilities => "typeDefinition",
        ["Capabilities specific to `textDocument/implementation`."]
        sub implementation: ImplementationClientCapabilities => "implementation",
        ["Capabilities specific to `textDocument/references`."]
        sub references: ReferenceClientCapabilities => "references",
        ["Capabilities specific to `textDocument/documentHighlight`."]
        sub document_highlight: DocumentHighlightClientCapabilities => "documentHighlight",
        ["Capabilities specific to `textDocument/documentSymbol`."]
        sub document_symbol: DocumentSymbolClientCapabilities => "documentSymbol",
        ["Capabilities specific to `textDocument/codeAction`."]
        sub code_action: CodeActionClientCapabilities => "codeAction",
        ["Capabilities specific to `textDocument/codeLens`."]
        sub code_lens: CodeLensClientCapabilities => "codeLens",
        ["Capabilities specific to `textDocument/documentLink`."]
        sub document_link: DocumentLinkClientCapabilities => "documentLink",
        ["Capabilities specific to `textDocument/documentColor`."]
        sub color_provider: DocumentColorClientCapabilities => "colorProvider",
        ["Capabilities specific to `textDocument/formatting`."]
        sub formatting: DocumentFormattingClientCapabilities => "formatting",
        ["Capabilities specific to `textDocument/rangeFormatting`."]
        sub range_formatting: DocumentRangeFormattingClientCapabilities => "rangeFormatting",
        ["Capabilities specific to `textDocument/onTypeFormatting`."]
        sub on_type_formatting: DocumentOnTypeFormattingClientCapabilities => "onTypeFormatting",
        ["Capabilities specific to `textDocument/rename`."]
        sub rename: RenameClientCapabilities => "rename",
        ["Capabilities specific to `textDocument/foldingRange`."]
        sub folding_range: FoldingRangeClientCapabilities => "foldingRange",
        ["Capabilities specific to `textDocument/selectionRange`."]
        sub selection_range: SelectionRangeClientCapabilities => "selectionRange",
        ["Capabilities specific to `textDocument/publishDiagnostics`."]
        sub publish_diagnostics: PublishDiagnosticsClientCapabilities => "publishDiagnostics",
        ["Capabilities specific to the call-hierarchy requests."]
        sub call_hierarchy: CallHierarchyClientCapabilities => "callHierarchy",
        ["Capabilities specific to the semantic-token requests."]
        sub semantic_tokens: SemanticTokensClientCapabilities => "semanticTokens",
        ["Capabilities specific to `textDocument/linkedEditingRange`."]
        sub linked_editing_range: LinkedEditingRangeClientCapabilities => "linkedEditingRange",
        ["Capabilities specific to `textDocument/moniker`."]
        sub moniker: MonikerClientCapabilities => "moniker",
        ["Capabilities specific to the type-hierarchy requests."]
        sub type_hierarchy: TypeHierarchyClientCapabilities => "typeHierarchy",
        ["Capabilities specific to `textDocument/inlineValue`."]
        sub inline_value: InlineValueClientCapabilities => "inlineValue",
        ["Capabilities specific to `textDocument/inlayHint`."]
        sub inlay_hint: InlayHintClientCapabilities => "inlayHint",
        ["Capabilities specific to the diagnostic pull model."]
        sub diagnostic: DiagnosticClientCapabilities => "diagnostic",
        ["Capabilities specific to inline completions."]
        sub inline_completion: InlineCompletionClientCapabilities => "inlineCompletion",
    }
}

// === General capability subtree ===

request_object! {
    /// How the client handles stale requests (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:StaleRequestSupportOptions
    StaleRequestSupportOptions => ResolvedStaleRequestSupportOptions {
        ["The client will actively cancel the request."]
        val cancel: bool => "cancel",
        ["The requests for which the client will retry on `ContentModified`."]
        val retry_on_content_modified: Vec<String> => "retryOnContentModified",
    }
}

request_object! {
    /// Client capabilities specific to regular expressions (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:RegularExpressionsClientCapabilities
    RegularExpressionsClientCapabilities => ResolvedRegularExpressionsClientCapabilities {
        ["The engine's name."]
        val engine: String => "engine",
        ["The engine's version."]
        val version: String => "version",
    }
}

request_object! {
    /// Client capabilities specific to the markdown parser (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:MarkdownClientCapabilities
    MarkdownClientCapabilities => ResolvedMarkdownClientCapabilities {
        ["The name of the parser."]
        val parser: String => "parser",
        ["The version of the parser."]
        val version: String => "version",
        ["HTML tags the client allows in Markdown."]
        val allowed_tags: Vec<String> => "allowedTags",
    }
}

request_object! {
    /// General client capabilities (request shape).
    // Go: internal/lsp/lsproto/lsp_generated.go:GeneralClientCapabilities
    GeneralClientCapabilities => ResolvedGeneralClientCapabilities {
        ["How the client handles stale requests."]
        sub stale_request_support: StaleRequestSupportOptions => "staleRequestSupport",
        ["Client capabilities specific to regular expressions."]
        sub regular_expressions: RegularExpressionsClientCapabilities => "regularExpressions",
        ["Client capabilities specific to the markdown parser."]
        sub markdown: MarkdownClientCapabilities => "markdown",
        ["The position encodings supported by the client."]
        val position_encodings: Vec<PositionEncodingKind> => "positionEncodings",
    }
}

#[cfg(test)]
#[path = "capabilities_test.rs"]
mod tests;
