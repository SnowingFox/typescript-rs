//! Port of the `Resolved*ClientCapabilities` tree from Go
//! `internal/lsp/lsproto/lsp_generated.go`.
//!
//! [`ResolvedClientCapabilities`] is the "resolved" (normalized) view of the
//! client capabilities: every nested field is a plain value rather than a
//! pointer, so deeply nested capabilities can be read without nil checks. Go
//! produces it via `(*ClientCapabilities).Resolve()`.
//!
//! # Divergence from Go
//! - Every resolved field carries Go's `json:"...,omitzero"` tag: a field is
//!   omitted on serialize when it equals its zero value (`false`, `0`, `""`,
//!   an empty slice, or an all-default nested struct). This is reproduced by a
//!   hand-written `serde` impl (the [`resolved_object!`] macro) rather than a
//!   derive, since `serde` has no built-in "omit zero value" for non-`Option`
//!   fields.
//! - The `(*ClientCapabilities).Resolve()` conversion is NOT ported here: it
//!   reads the pointer-based `ClientCapabilities` tree, which this crate still
//!   models as an open object ([`crate::ClientCapabilities`]). See the worklog
//!   `DEFER` note. The resolved value structs are self-contained.
//! - The new `*Kind`/`*Tag` enums and the two `Boolean*` unions live here
//!   (next to their only current consumers) rather than in `generated.rs`; the
//!   generator pass will own the full enum set later.

use std::borrow::Cow;
use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    CompletionItemKind, DiagnosticTag, FoldingRangeKind, PositionEncodingKind, SymbolKind,
};

// DEFER(phase-8): the `(*ClientCapabilities).Resolve()` conversion and the
// per-type `resolve()` methods are not ported here.
// blocked-by: the pointer-based `ClientCapabilities` tree (still an open
// object, `crate::ClientCapabilities`). The resolved value structs below are
// self-contained; the conversion lands with the generator pass.

/// Generates a "resolved capability" object type with hand-written `serde`
/// impls that reproduce Go's `json:",omitzero"` value-struct behavior:
///
/// - **Serialize**: each field is written only when it differs from its zero
///   value (`Default`); an all-default object serializes to `{}`. Nested
///   resolved structs recurse, so an all-default nested struct is omitted by
///   the parent.
/// - **Deserialize**: present keys are decoded into their field; absent keys
///   keep the zero value; unknown keys are ignored (Go's decoder behavior).
///
/// Every field type must implement `Default + PartialEq + Serialize +
/// Deserialize`.
macro_rules! resolved_object {
    (
        $(#[$smeta:meta])*
        $name:ident {
            $( [$doc:literal] $rust:ident : $ty:ty => $json:literal , )*
        }
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            $( #[doc = $doc] pub $rust : $ty, )*
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut __m = serializer.serialize_map(None)?;
                $(
                    if self.$rust != <$ty as ::core::default::Default>::default() {
                        __m.serialize_entry($json, &self.$rust)?;
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
                                $( $json => { __out.$rust = map.next_value()?; } )*
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
    };
}

/// Generates an integer-backed LSP enum newtype (zero value = `0`, matching
/// Go's `iota`/`uint32` enums), (de)serializing as a JSON integer.
macro_rules! int_enum {
    (
        $(#[$smeta:meta])*
        $name:ident { $( $(#[$cmeta:meta])* $cn:ident = $cv:literal , )* }
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name(pub u32);

        impl $name {
            $( $(#[$cmeta])* pub const $cn: $name = $name($cv); )*
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u32(self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Ok($name(u32::deserialize(deserializer)?))
            }
        }
    };
}

/// Generates a string-backed LSP enum newtype (zero value = `""`), with
/// `const`-constructible predefined values, (de)serializing as a JSON string.
/// Unknown values round-trip as their raw string.
macro_rules! string_enum {
    (
        $(#[$smeta:meta])*
        $name:ident { $( $(#[$cmeta:meta])* $cn:ident = $cv:literal , )* }
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
        pub struct $name(pub Cow<'static, str>);

        impl $name {
            $( $(#[$cmeta])* pub const $cn: $name = $name(Cow::Borrowed($cv)); )*
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Ok($name(Cow::Owned(String::deserialize(deserializer)?)))
            }
        }
    };
}

// === Enums referenced by the resolved-capabilities tree ===

string_enum! {
    /// The kind of resource operations a client supports (LSP
    /// `ResourceOperationKind`).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResourceOperationKind
    ResourceOperationKind {
        /// Supports creating new files and folders.
        CREATE = "create",
        /// Supports renaming existing files and folders.
        RENAME = "rename",
        /// Supports deleting existing files and folders.
        DELETE = "delete",
    }
}

string_enum! {
    /// The failure-handling strategy of a client applying a workspace edit
    /// (LSP `FailureHandlingKind`).
    // Go: internal/lsp/lsproto/lsp_generated.go:FailureHandlingKind
    FailureHandlingKind {
        /// Applying the workspace change is aborted on the first failure;
        /// operations before it stay applied.
        ABORT = "abort",
        /// All operations are executed transactionally (all or nothing).
        TRANSACTIONAL = "transactional",
        /// Textual changes are transactional; resource changes abort.
        TEXT_ONLY_TRANSACTIONAL = "textOnlyTransactional",
        /// The client tries to undo already-executed operations.
        UNDO = "undo",
    }
}

string_enum! {
    /// Describes the content type that a client supports in various result
    /// literals like `Hover` or `ParameterInformation` (LSP `MarkupKind`).
    // Go: internal/lsp/lsproto/lsp_generated.go:MarkupKind
    MarkupKind {
        /// Plain text is supported as a content format.
        PLAIN_TEXT = "plaintext",
        /// Markdown is supported as a content format.
        MARKDOWN = "markdown",
    }
}

string_enum! {
    /// A set of predefined code-action kinds (LSP `CodeActionKind`).
    // Go: internal/lsp/lsproto/lsp_generated.go:CodeActionKind
    CodeActionKind {
        /// Empty kind.
        EMPTY = "",
        /// Base kind for quickfix actions: `quickfix`.
        QUICK_FIX = "quickfix",
        /// Base kind for refactoring actions: `refactor`.
        REFACTOR = "refactor",
        /// Base kind for refactoring extraction actions: `refactor.extract`.
        REFACTOR_EXTRACT = "refactor.extract",
        /// Base kind for refactoring inline actions: `refactor.inline`.
        REFACTOR_INLINE = "refactor.inline",
        /// Base kind for refactoring move actions: `refactor.move`.
        REFACTOR_MOVE = "refactor.move",
        /// Base kind for refactoring rewrite actions: `refactor.rewrite`.
        REFACTOR_REWRITE = "refactor.rewrite",
        /// Base kind for source actions: `source`.
        SOURCE = "source",
        /// Base kind for an organize-imports source action.
        SOURCE_ORGANIZE_IMPORTS = "source.organizeImports",
        /// Base kind for auto-fix source actions: `source.fixAll`.
        SOURCE_FIX_ALL = "source.fixAll",
    }
}

string_enum! {
    /// The format of semantic tokens the client supports (LSP `TokenFormat`).
    // Go: internal/lsp/lsproto/lsp_generated.go:TokenFormat
    TokenFormat {
        /// Tokens are encoded relative to the previous token.
        RELATIVE = "relative",
    }
}

int_enum! {
    /// A symbol tag (LSP `SymbolTag`).
    // Go: internal/lsp/lsproto/lsp_generated.go:SymbolTag
    SymbolTag {
        /// Render a symbol as obsolete, usually using a strike-out.
        DEPRECATED = 1,
    }
}

int_enum! {
    /// A completion-item tag (LSP `CompletionItemTag`).
    // Go: internal/lsp/lsproto/lsp_generated.go:CompletionItemTag
    CompletionItemTag {
        /// Render a completion as obsolete, usually using a strike-out.
        DEPRECATED = 1,
    }
}

int_enum! {
    /// How whitespace and indentation is handled during completion-item
    /// insertion (LSP `InsertTextMode`).
    // Go: internal/lsp/lsproto/lsp_generated.go:InsertTextMode
    InsertTextMode {
        /// The insertion/replace string is taken as-is.
        AS_IS = 1,
        /// Leading whitespace of new lines is adjusted to match indentation.
        ADJUST_INDENTATION = 2,
    }
}

int_enum! {
    /// A code-action tag (LSP `CodeActionTag`).
    // Go: internal/lsp/lsproto/lsp_generated.go:CodeActionTag
    CodeActionTag {
        /// Marks the code action as LLM-generated.
        LLM_GENERATED = 1,
    }
}

int_enum! {
    /// The default behavior used by the client when a rename prepare request
    /// returns a default (LSP `PrepareSupportDefaultBehavior`).
    // Go: internal/lsp/lsproto/lsp_generated.go:PrepareSupportDefaultBehavior
    PrepareSupportDefaultBehavior {
        /// Select the identifier according to the language's syntax rule.
        IDENTIFIER = 1,
    }
}

// === Helper object/union types used by the resolved tree ===

/// An empty object placeholder (Go `struct{}`): serializes to `{}` and accepts
/// any JSON object (ignoring its members).
// Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrEmptyObject (EmptyObject field)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmptyObject;

impl Serialize for EmptyObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_map(Some(0))?.end()
    }
}

impl<'de> Deserialize<'de> for EmptyObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = EmptyObject;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                while map.next_key::<de::IgnoredAny>()?.is_some() {
                    map.next_value::<de::IgnoredAny>()?;
                }
                Ok(EmptyObject)
            }
        }
        deserializer.deserialize_map(V)
    }
}

resolved_object! {
    /// The object variant of the semantic-tokens `full` request capability
    /// (LSP `ClientSemanticTokensRequestFullDelta`).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientSemanticTokensRequestFullDelta
    ClientSemanticTokensRequestFullDelta {
        ["The client will send the `.../full/delta` request if the server provides a handler."]
        delta: Option<bool> => "delta",
    }
}

/// A union of a boolean or an [`EmptyObject`] (LSP `boolean | {}`).
///
/// Exactly one field is set; mirrors Go's pointer-pair representation.
// Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrEmptyObject
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BooleanOrEmptyObject {
    /// The boolean variant.
    pub boolean: Option<bool>,
    /// The empty-object variant.
    pub empty_object: Option<EmptyObject>,
}

impl Serialize for BooleanOrEmptyObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.boolean, &self.empty_object) {
            (Some(b), None) => serializer.serialize_bool(b),
            (None, Some(o)) => o.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of BooleanOrEmptyObject should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for BooleanOrEmptyObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BooleanOrEmptyObject;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean or an empty object")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(BooleanOrEmptyObject {
                    boolean: Some(v),
                    empty_object: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let o = EmptyObject::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(BooleanOrEmptyObject {
                    boolean: None,
                    empty_object: Some(o),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A union of a boolean or a [`ClientSemanticTokensRequestFullDelta`]
/// (LSP `boolean | ClientSemanticTokensRequestFullDelta`).
// Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrClientSemanticTokensRequestFullDelta
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BooleanOrClientSemanticTokensRequestFullDelta {
    /// The boolean variant.
    pub boolean: Option<bool>,
    /// The structured `full`/`delta` variant.
    pub client_semantic_tokens_request_full_delta: Option<ClientSemanticTokensRequestFullDelta>,
}

impl Serialize for BooleanOrClientSemanticTokensRequestFullDelta {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.boolean, &self.client_semantic_tokens_request_full_delta) {
            (Some(b), None) => serializer.serialize_bool(b),
            (None, Some(o)) => o.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of BooleanOrClientSemanticTokensRequestFullDelta should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for BooleanOrClientSemanticTokensRequestFullDelta {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BooleanOrClientSemanticTokensRequestFullDelta;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean or a semantic-tokens full/delta object")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(BooleanOrClientSemanticTokensRequestFullDelta {
                    boolean: Some(v),
                    client_semantic_tokens_request_full_delta: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let o = ClientSemanticTokensRequestFullDelta::deserialize(
                    de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(BooleanOrClientSemanticTokensRequestFullDelta {
                    boolean: None,
                    client_semantic_tokens_request_full_delta: Some(o),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

// === Workspace capability leaves ===

resolved_object! {
    /// Whether the client groups edits with equal labels into tree nodes
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedChangeAnnotationsSupportOptions
    ResolvedChangeAnnotationsSupportOptions {
        ["Whether the client groups edits with equal labels into tree nodes."]
        groups_on_label: bool => "groupsOnLabel",
    }
}

resolved_object! {
    /// Capabilities specific to `WorkspaceEdit`s (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedWorkspaceEditClientCapabilities
    ResolvedWorkspaceEditClientCapabilities {
        ["The client supports versioned document changes in `WorkspaceEdit`s."]
        document_changes: bool => "documentChanges",
        ["The resource operations the client supports."]
        resource_operations: Vec<ResourceOperationKind> => "resourceOperations",
        ["The failure-handling strategy if applying the workspace edit fails."]
        failure_handling: FailureHandlingKind => "failureHandling",
        ["Whether the client normalizes line endings to the client-specific setting."]
        normalizes_line_endings: bool => "normalizesLineEndings",
        ["Whether the client supports change annotations on text edits and resource operations."]
        change_annotation_support: ResolvedChangeAnnotationsSupportOptions => "changeAnnotationSupport",
        ["Whether the client supports `WorkspaceEditMetadata` in `WorkspaceEdit`s."]
        metadata_support: bool => "metadataSupport",
        ["Whether the client supports snippets as text edits."]
        snippet_edit_support: bool => "snippetEditSupport",
    }
}

resolved_object! {
    /// Capabilities specific to the `workspace/didChangeConfiguration`
    /// notification (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDidChangeConfigurationClientCapabilities
    ResolvedDidChangeConfigurationClientCapabilities {
        ["Did change configuration notification supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Capabilities specific to the `workspace/didChangeWatchedFiles`
    /// notification (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDidChangeWatchedFilesClientCapabilities
    ResolvedDidChangeWatchedFilesClientCapabilities {
        ["Did change watched files notification supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client has support for relative patterns."]
        relative_pattern_support: bool => "relativePatternSupport",
    }
}

resolved_object! {
    /// Specific capabilities for the `SymbolKind` in a symbol request
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientSymbolKindOptions
    ResolvedClientSymbolKindOptions {
        ["The symbol-kind values the client supports."]
        value_set: Vec<SymbolKind> => "valueSet",
    }
}

resolved_object! {
    /// The symbol tags the client supports (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientSymbolTagOptions
    ResolvedClientSymbolTagOptions {
        ["The tags supported by the client."]
        value_set: Vec<SymbolTag> => "valueSet",
    }
}

resolved_object! {
    /// The properties a client can resolve lazily for a workspace symbol
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientSymbolResolveOptions
    ResolvedClientSymbolResolveOptions {
        ["The properties that a client can resolve lazily."]
        properties: Vec<String> => "properties",
    }
}

resolved_object! {
    /// Client capabilities for a `workspace/symbol` request (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedWorkspaceSymbolClientCapabilities
    ResolvedWorkspaceSymbolClientCapabilities {
        ["Symbol request supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Specific capabilities for the `SymbolKind` in the request."]
        symbol_kind: ResolvedClientSymbolKindOptions => "symbolKind",
        ["The client supports tags on `SymbolInformation`."]
        tag_support: ResolvedClientSymbolTagOptions => "tagSupport",
        ["The client supports partial workspace symbols via resolve."]
        resolve_support: ResolvedClientSymbolResolveOptions => "resolveSupport",
    }
}

resolved_object! {
    /// The client capabilities of an `workspace/executeCommand` request
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedExecuteCommandClientCapabilities
    ResolvedExecuteCommandClientCapabilities {
        ["Execute command supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Workspace-scoped semantic-tokens capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedSemanticTokensWorkspaceClientCapabilities
    ResolvedSemanticTokensWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        refresh_support: bool => "refreshSupport",
    }
}

resolved_object! {
    /// Workspace-scoped code-lens capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCodeLensWorkspaceClientCapabilities
    ResolvedCodeLensWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        refresh_support: bool => "refreshSupport",
    }
}

resolved_object! {
    /// Capabilities relating to file-operation events (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedFileOperationClientCapabilities
    ResolvedFileOperationClientCapabilities {
        ["Whether the client supports dynamic registration for file requests/notifications."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The client supports sending didCreateFiles notifications."]
        did_create: bool => "didCreate",
        ["The client supports sending willCreateFiles requests."]
        will_create: bool => "willCreate",
        ["The client supports sending didRenameFiles notifications."]
        did_rename: bool => "didRename",
        ["The client supports sending willRenameFiles requests."]
        will_rename: bool => "willRename",
        ["The client supports sending didDeleteFiles notifications."]
        did_delete: bool => "didDelete",
        ["The client supports sending willDeleteFiles requests."]
        will_delete: bool => "willDelete",
    }
}

resolved_object! {
    /// Workspace-scoped inline-value capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedInlineValueWorkspaceClientCapabilities
    ResolvedInlineValueWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        refresh_support: bool => "refreshSupport",
    }
}

resolved_object! {
    /// Workspace-scoped inlay-hint capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedInlayHintWorkspaceClientCapabilities
    ResolvedInlayHintWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        refresh_support: bool => "refreshSupport",
    }
}

resolved_object! {
    /// Workspace-scoped diagnostic capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDiagnosticWorkspaceClientCapabilities
    ResolvedDiagnosticWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        refresh_support: bool => "refreshSupport",
    }
}

resolved_object! {
    /// Workspace-scoped folding-range capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedFoldingRangeWorkspaceClientCapabilities
    ResolvedFoldingRangeWorkspaceClientCapabilities {
        ["Whether the client supports a server-initiated refresh request."]
        refresh_support: bool => "refreshSupport",
    }
}

resolved_object! {
    /// Client capabilities for a text-document content provider (resolved
    /// view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedTextDocumentContentClientCapabilities
    ResolvedTextDocumentContentClientCapabilities {
        ["Text document content provider supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Workspace-specific client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedWorkspaceClientCapabilities
    ResolvedWorkspaceClientCapabilities {
        ["The client supports `workspace/applyEdit`."]
        apply_edit: bool => "applyEdit",
        ["Capabilities specific to `WorkspaceEdit`s."]
        workspace_edit: ResolvedWorkspaceEditClientCapabilities => "workspaceEdit",
        ["Capabilities specific to `workspace/didChangeConfiguration`."]
        did_change_configuration: ResolvedDidChangeConfigurationClientCapabilities => "didChangeConfiguration",
        ["Capabilities specific to `workspace/didChangeWatchedFiles`."]
        did_change_watched_files: ResolvedDidChangeWatchedFilesClientCapabilities => "didChangeWatchedFiles",
        ["Capabilities specific to `workspace/symbol`."]
        symbol: ResolvedWorkspaceSymbolClientCapabilities => "symbol",
        ["Capabilities specific to `workspace/executeCommand`."]
        execute_command: ResolvedExecuteCommandClientCapabilities => "executeCommand",
        ["The client has support for workspace folders."]
        workspace_folders: bool => "workspaceFolders",
        ["The client supports `workspace/configuration` requests."]
        configuration: bool => "configuration",
        ["Workspace-scoped semantic-tokens capabilities."]
        semantic_tokens: ResolvedSemanticTokensWorkspaceClientCapabilities => "semanticTokens",
        ["Workspace-scoped code-lens capabilities."]
        code_lens: ResolvedCodeLensWorkspaceClientCapabilities => "codeLens",
        ["File-operation capabilities."]
        file_operations: ResolvedFileOperationClientCapabilities => "fileOperations",
        ["Workspace-scoped inline-value capabilities."]
        inline_value: ResolvedInlineValueWorkspaceClientCapabilities => "inlineValue",
        ["Workspace-scoped inlay-hint capabilities."]
        inlay_hint: ResolvedInlayHintWorkspaceClientCapabilities => "inlayHint",
        ["Workspace-scoped diagnostic capabilities."]
        diagnostics: ResolvedDiagnosticWorkspaceClientCapabilities => "diagnostics",
        ["Workspace-scoped folding-range capabilities."]
        folding_range: ResolvedFoldingRangeWorkspaceClientCapabilities => "foldingRange",
        ["Capabilities specific to `workspace/textDocumentContent`."]
        text_document_content: ResolvedTextDocumentContentClientCapabilities => "textDocumentContent",
    }
}

// === Text-document capability leaves and groups ===

resolved_object! {
    /// Text-document synchronization capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedTextDocumentSyncClientCapabilities
    ResolvedTextDocumentSyncClientCapabilities {
        ["Whether text document synchronization supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The client supports sending will-save notifications."]
        will_save: bool => "willSave",
        ["The client supports sending a will-save-wait-until request."]
        will_save_wait_until: bool => "willSaveWaitUntil",
        ["The client supports did-save notifications."]
        did_save: bool => "didSave",
    }
}

resolved_object! {
    /// Text-document filter capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedTextDocumentFilterClientCapabilities
    ResolvedTextDocumentFilterClientCapabilities {
        ["The client supports relative patterns."]
        relative_pattern_support: bool => "relativePatternSupport",
    }
}

resolved_object! {
    /// The completion-item tags the client supports (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCompletionItemTagOptions
    ResolvedCompletionItemTagOptions {
        ["The tags supported by the client."]
        value_set: Vec<CompletionItemTag> => "valueSet",
    }
}

resolved_object! {
    /// The properties a client can resolve lazily for a completion item
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCompletionItemResolveOptions
    ResolvedClientCompletionItemResolveOptions {
        ["The properties that a client can resolve lazily."]
        properties: Vec<String> => "properties",
    }
}

resolved_object! {
    /// The insert-text modes the client supports on completion items
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCompletionItemInsertTextModeOptions
    ResolvedClientCompletionItemInsertTextModeOptions {
        ["The insert-text modes supported by the client."]
        value_set: Vec<InsertTextMode> => "valueSet",
    }
}

resolved_object! {
    /// `CompletionItem`-specific capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCompletionItemOptions
    ResolvedClientCompletionItemOptions {
        ["Client supports snippets as insert text."]
        snippet_support: bool => "snippetSupport",
        ["Client supports commit characters on a completion item."]
        commit_characters_support: bool => "commitCharactersSupport",
        ["Content formats supported for the documentation property."]
        documentation_format: Vec<MarkupKind> => "documentationFormat",
        ["Client supports the deprecated property on a completion item."]
        deprecated_support: bool => "deprecatedSupport",
        ["Client supports the preselect property on a completion item."]
        preselect_support: bool => "preselectSupport",
        ["Client supports the tag property on a completion item."]
        tag_support: ResolvedCompletionItemTagOptions => "tagSupport",
        ["Client supports insert-replace edits."]
        insert_replace_support: bool => "insertReplaceSupport",
        ["Properties a client can resolve lazily on a completion item."]
        resolve_support: ResolvedClientCompletionItemResolveOptions => "resolveSupport",
        ["The client supports the `insertTextMode` property."]
        insert_text_mode_support: ResolvedClientCompletionItemInsertTextModeOptions => "insertTextModeSupport",
        ["The client has support for completion-item label details."]
        label_details_support: bool => "labelDetailsSupport",
    }
}

resolved_object! {
    /// Specific capabilities for the `CompletionItemKind` (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCompletionItemOptionsKind
    ResolvedClientCompletionItemOptionsKind {
        ["The completion-item-kind values the client supports."]
        value_set: Vec<CompletionItemKind> => "valueSet",
    }
}

resolved_object! {
    /// `CompletionList`-specific capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCompletionListCapabilities
    ResolvedCompletionListCapabilities {
        ["The item-default property names the client supports."]
        item_defaults: Vec<String> => "itemDefaults",
        ["Whether the client supports `CompletionList.applyKind`."]
        apply_kind_support: bool => "applyKindSupport",
    }
}

resolved_object! {
    /// Completion client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCompletionClientCapabilities
    ResolvedCompletionClientCapabilities {
        ["Whether completion supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["`CompletionItem`-specific capabilities."]
        completion_item: ResolvedClientCompletionItemOptions => "completionItem",
        ["`CompletionItemKind`-specific capabilities."]
        completion_item_kind: ResolvedClientCompletionItemOptionsKind => "completionItemKind",
        ["How the client handles whitespace/indentation on multi-line completions."]
        insert_text_mode: InsertTextMode => "insertTextMode",
        ["The client supports sending additional completion context."]
        context_support: bool => "contextSupport",
        ["`CompletionList`-specific capabilities."]
        completion_list: ResolvedCompletionListCapabilities => "completionList",
    }
}

resolved_object! {
    /// Hover client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedHoverClientCapabilities
    ResolvedHoverClientCapabilities {
        ["Whether hover supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Content formats supported for the content property."]
        content_format: Vec<MarkupKind> => "contentFormat",
        ["Whether the client supports the verbosity-level properties."]
        verbosity_level: bool => "verbosityLevel",
    }
}

resolved_object! {
    /// Parameter-information capabilities for signature help (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientSignatureParameterInformationOptions
    ResolvedClientSignatureParameterInformationOptions {
        ["The client supports processing label offsets instead of a label string."]
        label_offset_support: bool => "labelOffsetSupport",
    }
}

resolved_object! {
    /// `SignatureInformation`-specific capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientSignatureInformationOptions
    ResolvedClientSignatureInformationOptions {
        ["Content formats supported for the documentation property."]
        documentation_format: Vec<MarkupKind> => "documentationFormat",
        ["Client capabilities specific to parameter information."]
        parameter_information: ResolvedClientSignatureParameterInformationOptions => "parameterInformation",
        ["The client supports the `activeParameter` property on `SignatureInformation`."]
        active_parameter_support: bool => "activeParameterSupport",
        ["The client supports a `null` `activeParameter`."]
        no_active_parameter_support: bool => "noActiveParameterSupport",
    }
}

resolved_object! {
    /// Signature-help client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedSignatureHelpClientCapabilities
    ResolvedSignatureHelpClientCapabilities {
        ["Whether signature help supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["`SignatureInformation`-specific capabilities."]
        signature_information: ResolvedClientSignatureInformationOptions => "signatureInformation",
        ["The client supports sending additional signature-help context."]
        context_support: bool => "contextSupport",
    }
}

resolved_object! {
    /// Declaration client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDeclarationClientCapabilities
    ResolvedDeclarationClientCapabilities {
        ["Whether declaration supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of declaration links."]
        link_support: bool => "linkSupport",
    }
}

resolved_object! {
    /// Definition client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDefinitionClientCapabilities
    ResolvedDefinitionClientCapabilities {
        ["Whether definition supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of definition links."]
        link_support: bool => "linkSupport",
    }
}

resolved_object! {
    /// Type-definition client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedTypeDefinitionClientCapabilities
    ResolvedTypeDefinitionClientCapabilities {
        ["Whether type definition supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of definition links."]
        link_support: bool => "linkSupport",
    }
}

resolved_object! {
    /// Implementation client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedImplementationClientCapabilities
    ResolvedImplementationClientCapabilities {
        ["Whether implementation supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The client supports additional metadata in the form of definition links."]
        link_support: bool => "linkSupport",
    }
}

resolved_object! {
    /// References client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedReferenceClientCapabilities
    ResolvedReferenceClientCapabilities {
        ["Whether references supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Document-highlight client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDocumentHighlightClientCapabilities
    ResolvedDocumentHighlightClientCapabilities {
        ["Whether document highlight supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Document-symbol client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDocumentSymbolClientCapabilities
    ResolvedDocumentSymbolClientCapabilities {
        ["Whether document symbol supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Specific capabilities for the `SymbolKind`."]
        symbol_kind: ResolvedClientSymbolKindOptions => "symbolKind",
        ["The client supports hierarchical document symbols."]
        hierarchical_document_symbol_support: bool => "hierarchicalDocumentSymbolSupport",
        ["The client supports tags on `SymbolInformation`."]
        tag_support: ResolvedClientSymbolTagOptions => "tagSupport",
        ["The client supports an additional label presented in the UI."]
        label_support: bool => "labelSupport",
    }
}

resolved_object! {
    /// The code-action kinds the client supports (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCodeActionKindOptions
    ResolvedClientCodeActionKindOptions {
        ["The code-action kind values the client supports."]
        value_set: Vec<CodeActionKind> => "valueSet",
    }
}

resolved_object! {
    /// Code-action literal support (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCodeActionLiteralOptions
    ResolvedClientCodeActionLiteralOptions {
        ["The code-action kind is supported with the following value set."]
        code_action_kind: ResolvedClientCodeActionKindOptions => "codeActionKind",
    }
}

resolved_object! {
    /// The properties a client can resolve lazily for a code action
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCodeActionResolveOptions
    ResolvedClientCodeActionResolveOptions {
        ["The properties that a client can resolve lazily."]
        properties: Vec<String> => "properties",
    }
}

resolved_object! {
    /// The code-action tags the client supports (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCodeActionTagOptions
    ResolvedCodeActionTagOptions {
        ["The tags supported by the client."]
        value_set: Vec<CodeActionTag> => "valueSet",
    }
}

resolved_object! {
    /// Code-action client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCodeActionClientCapabilities
    ResolvedCodeActionClientCapabilities {
        ["Whether code action supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The client supports code-action literals."]
        code_action_literal_support: ResolvedClientCodeActionLiteralOptions => "codeActionLiteralSupport",
        ["Whether code action supports the `isPreferred` property."]
        is_preferred_support: bool => "isPreferredSupport",
        ["Whether code action supports the `disabled` property."]
        disabled_support: bool => "disabledSupport",
        ["Whether code action supports the `data` property."]
        data_support: bool => "dataSupport",
        ["Whether the client supports resolving additional code-action properties."]
        resolve_support: ResolvedClientCodeActionResolveOptions => "resolveSupport",
        ["Whether the client honors change annotations in code-action edits."]
        honors_change_annotations: bool => "honorsChangeAnnotations",
        ["Whether the client supports documentation for a class of code actions."]
        documentation_support: bool => "documentationSupport",
        ["The client supports the tag property on a code action."]
        tag_support: ResolvedCodeActionTagOptions => "tagSupport",
    }
}

resolved_object! {
    /// The properties a client can resolve lazily for a code lens
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCodeLensResolveOptions
    ResolvedClientCodeLensResolveOptions {
        ["The properties that a client can resolve lazily."]
        properties: Vec<String> => "properties",
    }
}

resolved_object! {
    /// Code-lens client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCodeLensClientCapabilities
    ResolvedCodeLensClientCapabilities {
        ["Whether code lens supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports resolving additional code-lens properties."]
        resolve_support: ResolvedClientCodeLensResolveOptions => "resolveSupport",
    }
}

resolved_object! {
    /// Document-link client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDocumentLinkClientCapabilities
    ResolvedDocumentLinkClientCapabilities {
        ["Whether document link supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports the `tooltip` property on `DocumentLink`."]
        tooltip_support: bool => "tooltipSupport",
    }
}

resolved_object! {
    /// Document-color client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDocumentColorClientCapabilities
    ResolvedDocumentColorClientCapabilities {
        ["Whether document color supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Document-formatting client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDocumentFormattingClientCapabilities
    ResolvedDocumentFormattingClientCapabilities {
        ["Whether formatting supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Document-range-formatting client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDocumentRangeFormattingClientCapabilities
    ResolvedDocumentRangeFormattingClientCapabilities {
        ["Whether range formatting supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports formatting multiple ranges at once."]
        ranges_support: bool => "rangesSupport",
    }
}

resolved_object! {
    /// Document-on-type-formatting client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDocumentOnTypeFormattingClientCapabilities
    ResolvedDocumentOnTypeFormattingClientCapabilities {
        ["Whether on-type formatting supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Rename client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedRenameClientCapabilities
    ResolvedRenameClientCapabilities {
        ["Whether rename supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Client supports testing validity of rename operations before execution."]
        prepare_support: bool => "prepareSupport",
        ["The default behavior used by the client."]
        prepare_support_default_behavior: PrepareSupportDefaultBehavior => "prepareSupportDefaultBehavior",
        ["Whether the client honors change annotations in the rename edit."]
        honors_change_annotations: bool => "honorsChangeAnnotations",
    }
}

resolved_object! {
    /// The folding-range kinds the client supports (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientFoldingRangeKindOptions
    ResolvedClientFoldingRangeKindOptions {
        ["The folding-range kind values the client supports."]
        value_set: Vec<FoldingRangeKind> => "valueSet",
    }
}

resolved_object! {
    /// Specific options for folding ranges (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientFoldingRangeOptions
    ResolvedClientFoldingRangeOptions {
        ["Whether the client supports setting collapsedText on folding ranges."]
        collapsed_text: bool => "collapsedText",
    }
}

resolved_object! {
    /// Folding-range client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedFoldingRangeClientCapabilities
    ResolvedFoldingRangeClientCapabilities {
        ["Whether folding range supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["The maximum number of folding ranges the client prefers per document."]
        range_limit: u32 => "rangeLimit",
        ["Whether the client only supports folding complete lines."]
        line_folding_only: bool => "lineFoldingOnly",
        ["Specific options for the folding-range kind."]
        folding_range_kind: ResolvedClientFoldingRangeKindOptions => "foldingRangeKind",
        ["Specific options for the folding range."]
        folding_range: ResolvedClientFoldingRangeOptions => "foldingRange",
    }
}

resolved_object! {
    /// Selection-range client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedSelectionRangeClientCapabilities
    ResolvedSelectionRangeClientCapabilities {
        ["Whether selection range supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// The diagnostic tags the client supports (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientDiagnosticsTagOptions
    ResolvedClientDiagnosticsTagOptions {
        ["The tags supported by the client."]
        value_set: Vec<DiagnosticTag> => "valueSet",
    }
}

resolved_object! {
    /// Publish-diagnostics client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedPublishDiagnosticsClientCapabilities
    ResolvedPublishDiagnosticsClientCapabilities {
        ["Whether the client accepts diagnostics with related information."]
        related_information: bool => "relatedInformation",
        ["The diagnostic tags the client supports."]
        tag_support: ResolvedClientDiagnosticsTagOptions => "tagSupport",
        ["Whether the client supports a `codeDescription` property."]
        code_description_support: bool => "codeDescriptionSupport",
        ["Whether the client supports the `data` property on a diagnostic."]
        data_support: bool => "dataSupport",
        ["Whether the client interprets the `version` property of the notification."]
        version_support: bool => "versionSupport",
    }
}

resolved_object! {
    /// Call-hierarchy client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedCallHierarchyClientCapabilities
    ResolvedCallHierarchyClientCapabilities {
        ["Whether call hierarchy supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Which semantic-token requests the client supports (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientSemanticTokensRequestOptions
    ResolvedClientSemanticTokensRequestOptions {
        ["The client will send the `.../range` request if the server provides a handler."]
        range: BooleanOrEmptyObject => "range",
        ["The client will send the `.../full` request if the server provides a handler."]
        full: BooleanOrClientSemanticTokensRequestFullDelta => "full",
    }
}

resolved_object! {
    /// Semantic-tokens client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedSemanticTokensClientCapabilities
    ResolvedSemanticTokensClientCapabilities {
        ["Whether semantic tokens supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Which requests the client supports."]
        requests: ResolvedClientSemanticTokensRequestOptions => "requests",
        ["The token types the client supports."]
        token_types: Vec<String> => "tokenTypes",
        ["The token modifiers the client supports."]
        token_modifiers: Vec<String> => "tokenModifiers",
        ["The token formats the client supports."]
        formats: Vec<TokenFormat> => "formats",
        ["Whether the client supports overlapping tokens."]
        overlapping_token_support: bool => "overlappingTokenSupport",
        ["Whether the client supports multi-line tokens."]
        multiline_token_support: bool => "multilineTokenSupport",
        ["Whether the client allows the server to cancel a semantic-token request."]
        server_cancel_support: bool => "serverCancelSupport",
        ["Whether the client augments existing syntax tokens with semantic tokens."]
        augments_syntax_tokens: bool => "augmentsSyntaxTokens",
    }
}

resolved_object! {
    /// Linked-editing-range client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedLinkedEditingRangeClientCapabilities
    ResolvedLinkedEditingRangeClientCapabilities {
        ["Whether linked editing range supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Moniker client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedMonikerClientCapabilities
    ResolvedMonikerClientCapabilities {
        ["Whether moniker supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Type-hierarchy client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedTypeHierarchyClientCapabilities
    ResolvedTypeHierarchyClientCapabilities {
        ["Whether type hierarchy supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Inline-value client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedInlineValueClientCapabilities
    ResolvedInlineValueClientCapabilities {
        ["Whether inline value supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// The properties a client can resolve lazily for an inlay hint
    /// (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientInlayHintResolveOptions
    ResolvedClientInlayHintResolveOptions {
        ["The properties that a client can resolve lazily."]
        properties: Vec<String> => "properties",
    }
}

resolved_object! {
    /// Inlay-hint client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedInlayHintClientCapabilities
    ResolvedInlayHintClientCapabilities {
        ["Whether inlay hints support dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Properties a client can resolve lazily on an inlay hint."]
        resolve_support: ResolvedClientInlayHintResolveOptions => "resolveSupport",
    }
}

resolved_object! {
    /// Diagnostic (pull-model) client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedDiagnosticClientCapabilities
    ResolvedDiagnosticClientCapabilities {
        ["Whether the client accepts diagnostics with related information."]
        related_information: bool => "relatedInformation",
        ["The diagnostic tags the client supports."]
        tag_support: ResolvedClientDiagnosticsTagOptions => "tagSupport",
        ["Whether the client supports a `codeDescription` property."]
        code_description_support: bool => "codeDescriptionSupport",
        ["Whether the client supports the `data` property on a diagnostic."]
        data_support: bool => "dataSupport",
        ["Whether diagnostic supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
        ["Whether the client supports related documents for document diagnostic pulls."]
        related_document_support: bool => "relatedDocumentSupport",
    }
}

resolved_object! {
    /// Inline-completion client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedInlineCompletionClientCapabilities
    ResolvedInlineCompletionClientCapabilities {
        ["Whether inline completion supports dynamic registration."]
        dynamic_registration: bool => "dynamicRegistration",
    }
}

resolved_object! {
    /// Text-document-specific client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedTextDocumentClientCapabilities
    ResolvedTextDocumentClientCapabilities {
        ["Synchronization capabilities."]
        synchronization: ResolvedTextDocumentSyncClientCapabilities => "synchronization",
        ["Filter capabilities."]
        filters: ResolvedTextDocumentFilterClientCapabilities => "filters",
        ["Capabilities specific to `textDocument/completion`."]
        completion: ResolvedCompletionClientCapabilities => "completion",
        ["Capabilities specific to `textDocument/hover`."]
        hover: ResolvedHoverClientCapabilities => "hover",
        ["Capabilities specific to `textDocument/signatureHelp`."]
        signature_help: ResolvedSignatureHelpClientCapabilities => "signatureHelp",
        ["Capabilities specific to `textDocument/declaration`."]
        declaration: ResolvedDeclarationClientCapabilities => "declaration",
        ["Capabilities specific to `textDocument/definition`."]
        definition: ResolvedDefinitionClientCapabilities => "definition",
        ["Capabilities specific to `textDocument/typeDefinition`."]
        type_definition: ResolvedTypeDefinitionClientCapabilities => "typeDefinition",
        ["Capabilities specific to `textDocument/implementation`."]
        implementation: ResolvedImplementationClientCapabilities => "implementation",
        ["Capabilities specific to `textDocument/references`."]
        references: ResolvedReferenceClientCapabilities => "references",
        ["Capabilities specific to `textDocument/documentHighlight`."]
        document_highlight: ResolvedDocumentHighlightClientCapabilities => "documentHighlight",
        ["Capabilities specific to `textDocument/documentSymbol`."]
        document_symbol: ResolvedDocumentSymbolClientCapabilities => "documentSymbol",
        ["Capabilities specific to `textDocument/codeAction`."]
        code_action: ResolvedCodeActionClientCapabilities => "codeAction",
        ["Capabilities specific to `textDocument/codeLens`."]
        code_lens: ResolvedCodeLensClientCapabilities => "codeLens",
        ["Capabilities specific to `textDocument/documentLink`."]
        document_link: ResolvedDocumentLinkClientCapabilities => "documentLink",
        ["Capabilities specific to `textDocument/documentColor`."]
        color_provider: ResolvedDocumentColorClientCapabilities => "colorProvider",
        ["Capabilities specific to `textDocument/formatting`."]
        formatting: ResolvedDocumentFormattingClientCapabilities => "formatting",
        ["Capabilities specific to `textDocument/rangeFormatting`."]
        range_formatting: ResolvedDocumentRangeFormattingClientCapabilities => "rangeFormatting",
        ["Capabilities specific to `textDocument/onTypeFormatting`."]
        on_type_formatting: ResolvedDocumentOnTypeFormattingClientCapabilities => "onTypeFormatting",
        ["Capabilities specific to `textDocument/rename`."]
        rename: ResolvedRenameClientCapabilities => "rename",
        ["Capabilities specific to `textDocument/foldingRange`."]
        folding_range: ResolvedFoldingRangeClientCapabilities => "foldingRange",
        ["Capabilities specific to `textDocument/selectionRange`."]
        selection_range: ResolvedSelectionRangeClientCapabilities => "selectionRange",
        ["Capabilities specific to `textDocument/publishDiagnostics`."]
        publish_diagnostics: ResolvedPublishDiagnosticsClientCapabilities => "publishDiagnostics",
        ["Capabilities specific to the call-hierarchy requests."]
        call_hierarchy: ResolvedCallHierarchyClientCapabilities => "callHierarchy",
        ["Capabilities specific to the semantic-token requests."]
        semantic_tokens: ResolvedSemanticTokensClientCapabilities => "semanticTokens",
        ["Capabilities specific to `textDocument/linkedEditingRange`."]
        linked_editing_range: ResolvedLinkedEditingRangeClientCapabilities => "linkedEditingRange",
        ["Capabilities specific to `textDocument/moniker`."]
        moniker: ResolvedMonikerClientCapabilities => "moniker",
        ["Capabilities specific to the type-hierarchy requests."]
        type_hierarchy: ResolvedTypeHierarchyClientCapabilities => "typeHierarchy",
        ["Capabilities specific to `textDocument/inlineValue`."]
        inline_value: ResolvedInlineValueClientCapabilities => "inlineValue",
        ["Capabilities specific to `textDocument/inlayHint`."]
        inlay_hint: ResolvedInlayHintClientCapabilities => "inlayHint",
        ["Capabilities specific to the diagnostic pull model."]
        diagnostic: ResolvedDiagnosticClientCapabilities => "diagnostic",
        ["Capabilities specific to inline completions."]
        inline_completion: ResolvedInlineCompletionClientCapabilities => "inlineCompletion",
    }
}

// === Window capability leaves and group ===

resolved_object! {
    /// Capabilities specific to the `MessageActionItem` type (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientShowMessageActionItemOptions
    ResolvedClientShowMessageActionItemOptions {
        ["Whether the client supports additional attributes preserved on the action item."]
        additional_properties_support: bool => "additionalPropertiesSupport",
    }
}

resolved_object! {
    /// Show-message-request client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedShowMessageRequestClientCapabilities
    ResolvedShowMessageRequestClientCapabilities {
        ["Capabilities specific to the `MessageActionItem` type."]
        message_action_item: ResolvedClientShowMessageActionItemOptions => "messageActionItem",
    }
}

resolved_object! {
    /// Show-document client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedShowDocumentClientCapabilities
    ResolvedShowDocumentClientCapabilities {
        ["The client has support for the showDocument request."]
        support: bool => "support",
    }
}

resolved_object! {
    /// Window-specific client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedWindowClientCapabilities
    ResolvedWindowClientCapabilities {
        ["Whether the client supports server-initiated progress via `window/workDoneProgress/create`."]
        work_done_progress: bool => "workDoneProgress",
        ["Capabilities specific to the showMessage request."]
        show_message: ResolvedShowMessageRequestClientCapabilities => "showMessage",
        ["Capabilities specific to the showDocument request."]
        show_document: ResolvedShowDocumentClientCapabilities => "showDocument",
    }
}

// === General capability leaves and group ===

resolved_object! {
    /// How the client handles stale requests (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedStaleRequestSupportOptions
    ResolvedStaleRequestSupportOptions {
        ["The client will actively cancel the request."]
        cancel: bool => "cancel",
        ["The requests for which the client will retry on `ContentModified`."]
        retry_on_content_modified: Vec<String> => "retryOnContentModified",
    }
}

resolved_object! {
    /// Client capabilities specific to regular expressions (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedRegularExpressionsClientCapabilities
    ResolvedRegularExpressionsClientCapabilities {
        ["The engine's name."]
        engine: String => "engine",
        ["The engine's version."]
        version: String => "version",
    }
}

resolved_object! {
    /// Client capabilities specific to the markdown parser (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedMarkdownClientCapabilities
    ResolvedMarkdownClientCapabilities {
        ["The name of the parser."]
        parser: String => "parser",
        ["The version of the parser."]
        version: String => "version",
        ["HTML tags the client allows in Markdown."]
        allowed_tags: Vec<String> => "allowedTags",
    }
}

resolved_object! {
    /// General client capabilities (resolved view).
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedGeneralClientCapabilities
    ResolvedGeneralClientCapabilities {
        ["How the client handles stale requests."]
        stale_request_support: ResolvedStaleRequestSupportOptions => "staleRequestSupport",
        ["Client capabilities specific to regular expressions."]
        regular_expressions: ResolvedRegularExpressionsClientCapabilities => "regularExpressions",
        ["Client capabilities specific to the markdown parser."]
        markdown: ResolvedMarkdownClientCapabilities => "markdown",
        ["The position encodings supported by the client."]
        position_encodings: Vec<PositionEncodingKind> => "positionEncodings",
    }
}

// === Top-level resolved capabilities ===

resolved_object! {
    /// A normalized view of [`crate::ClientCapabilities`] where every nested
    /// field is a value (not a pointer), for easy access to deeply nested
    /// capabilities. Produced in Go via `(*ClientCapabilities).Resolve()`.
    // Go: internal/lsp/lsproto/lsp_generated.go:ResolvedClientCapabilities
    ResolvedClientCapabilities {
        ["Workspace-specific client capabilities."]
        workspace: ResolvedWorkspaceClientCapabilities => "workspace",
        ["Text-document-specific client capabilities."]
        text_document: ResolvedTextDocumentClientCapabilities => "textDocument",
        ["Window-specific client capabilities."]
        window: ResolvedWindowClientCapabilities => "window",
        ["General client capabilities."]
        general: ResolvedGeneralClientCapabilities => "general",
        ["Whether the client supports Visual Studio extensions."]
        vs_supports_visual_studio_extensions: bool => "_vs_supportsVisualStudioExtensions",
        ["The snippet version supported by the client."]
        vs_supported_snippet_version: i32 => "_vs_supportedSnippetVersion",
        ["Whether the client supports not including text in didOpen notifications."]
        vs_supports_not_including_text_in_text_document_did_open: bool => "_vs_supportsNotIncludingTextInTextDocumentDidOpen",
        ["Whether the client supports icon extensions."]
        vs_supports_icon_extensions: bool => "_vs_supportsIconExtensions",
        ["Whether the client supports diagnostic requests."]
        vs_supports_diagnostic_requests: bool => "_vs_supportsDiagnosticRequests",
    }
}

#[cfg(test)]
#[path = "resolved_test.rs"]
mod tests;
