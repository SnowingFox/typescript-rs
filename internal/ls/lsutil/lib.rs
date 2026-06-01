//! `tsgo_ls_lsutil` — 1:1 Rust port of Go `internal/ls/lsutil`.
//!
//! The shared language-service helper package the language service builds on:
//! AST/token position helpers, node-kind predicates, identifier/name helpers,
//! and the automatic-semicolon-insertion (ASI) syntax classification. These are
//! the syntactic building blocks reused by `format`, `ls/change`,
//! `ls/autoimport`, and the `ls` root.
//!
//! # Scope of this port
//!
//! `lsutil` is mostly syntactic, but several functions depend on layers that are
//! not yet ported. This crate ports the reachable syntactic subset; the rest is
//! deferred with precise `blocked-by` notes (see the per-item `// DEFER` anchors
//! and the phase-7 worklog):
//!
//! - **Checker-dependent** (`GetSymbolKind`, `GetSymbolModifiers`): need
//!   `*checker.Checker`, which is not ported. Deferred to the `ls` root.
//! - **Program-dependent** (`ShouldUseUriStyleNodeCoreModules`): needs
//!   `*compiler.Program`.
//! - **`UserPreferences` machinery** (`organizeimports.go`, `userpreferences.go`,
//!   `formatcodeoptions.go`, `GetQuotePreference`): depend on Go's reflection
//!   based config marshaling, ICU collation (`golang.org/x/text/collate`),
//!   `modulespecifiers`, `vfsmatch`, `lsproto`, and `printer`.
//! - **Token-cache navigation that needs `astnav`** (`IsCompletedNode`,
//!   `hasChildOfKind`, `PositionBelongsToNode`, `NodeIsASICandidate`,
//!   `PositionIsASICandidate`, `ProbablyUsesSemicolons`): need
//!   `astnav.FindChildOfKind`/`FindNextToken` (which require
//!   `tsgo_astnav::SourceFile`, an arena-owning context incompatible with this
//!   crate's own arena-owning [`SourceFile`]) and/or the deferred
//!   `scanner::GetECMALineOfPosition`.
//!
//! # Navigation context
//!
//! Go's helpers take `(node *ast.Node, sourceFile *ast.SourceFile)`, where the
//! `*ast.SourceFile` carries both the source text and the synthesized-token
//! cache. The Rust `tsgo_ast` `SourceFile` node stores neither, so — exactly as
//! `tsgo_astnav` does — this crate models the navigation context as a dedicated
//! [`SourceFile`] struct that owns the [`NodeArena`](tsgo_ast::NodeArena), the
//! root id, the text, the language variant, and the token cache.

mod asi;
mod children;
mod symbol_display;
mod userpreferences;
mod utilities;

pub use asi::{
    syntax_may_be_asi_candidate, syntax_requires_trailing_comma_or_semicolon_or_asi,
    syntax_requires_trailing_function_block_or_semicolon_or_asi,
    syntax_requires_trailing_module_block_or_semicolon_or_asi,
    syntax_requires_trailing_semicolon_or_asi,
};
pub use children::{
    assert_has_real_position, get_first_token, get_last_child, get_last_token,
    get_last_visited_child, SourceFile,
};
pub use symbol_display::{
    ScriptElementKind, ScriptElementKindModifier, FILE_EXTENSION_KIND_MODIFIERS,
};
pub use userpreferences::QuotePreference;
pub use utilities::{
    is_non_contextual_keyword, module_specifier_to_valid_identifier,
    module_symbol_to_valid_identifier, quote_preference_from_string,
};
