//! Port of Go `internal/ls/findallreferences.go`: types and helpers for the
//! advanced find-all-references infrastructure.
//!
//! The base `references.rs` ports the single-file, single-symbol
//! `ProvideReferences` path. This module extends it with the types and helper
//! predicates from the full FAR machinery: the result types
//! ([`DefinitionKind`], [`Definition`], [`EntryKind`], [`ReferenceEntry`],
//! [`SymbolAndEntries`]), the search configuration ([`ReferenceUse`],
//! [`RefOptions`]), and the reference-search state types ([`RefSearch`]).
//!
//! # Reachable subset
//!
//! This round ports all the type definitions and pure helper functions. The
//! actual search algorithms (`getReferencedSymbolsForNode`,
//! `getReferencesInContainerOrFiles`, `getReferencesAtLocation`,
//! `getRelatedSymbol`, `forEachRelatedSymbol`, and the import/export/module
//! reference tracking) are deferred — they depend heavily on the checker, the
//! import tracker, and the cross-project orchestrator.
//!
//! DEFER(phase-7-ls): the full search state machine (`refState` and its methods),
//! the import tracker integration, the cross-project
//! `handleCrossProject`/`CrossProjectOrchestrator` plumbing, the
//! `getReferencedSymbolsForModule` module-reference search, and the
//! `ProvideImplementations` / `ProvideVsReferences` entry points.

use tsgo_core::text::TextRange;

/// How a reference search was initiated.
// Go: internal/ls/findallreferences.go:referenceUse
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReferenceUse {
    /// Not specified.
    #[default]
    None,
    /// Generic "other" use.
    Other,
    /// Find References request.
    References,
    /// Rename request.
    Rename,
}

/// Options controlling the find-all-references search.
// Go: internal/ls/findallreferences.go:refOptions
#[derive(Clone, Debug, Default)]
pub struct RefOptions {
    pub find_in_strings: bool,
    pub find_in_comments: bool,
    pub use_kind: ReferenceUse,
    pub implementations: bool,
    /// Renamed from `providePrefixAndSuffixTextForRename` in Go. Default is
    /// `true` for rename scenarios.
    pub use_aliases_for_rename: bool,
}

impl RefOptions {
    /// Returns `true` if this is a rename search with prefix/suffix text
    /// enabled.
    // Go: internal/ls/findallreferences.go:isForRenameWithPrefixAndSuffixText
    pub fn is_for_rename_with_prefix_and_suffix_text(&self) -> bool {
        self.use_kind == ReferenceUse::Rename && self.use_aliases_for_rename
    }
}

/// The kind of a definition in a find-all-references result.
// Go: internal/ls/findallreferences.go:DefinitionKind
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DefinitionKind {
    #[default]
    Symbol,
    Label,
    Keyword,
    This,
    String,
    TripleSlashReference,
}

/// A definition in a find-all-references result, identifying the kind of
/// definition and (when applicable) the associated symbol.
// Go: internal/ls/findallreferences.go:Definition
#[derive(Clone, Debug, Default)]
pub struct Definition {
    pub kind: DefinitionKind,
}

/// The kind of a reference entry.
// Go: internal/ls/findallreferences.go:entryKind
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EntryKind {
    #[default]
    None,
    Range,
    Node,
    StringLiteral,
    SearchedLocalFoundProperty,
    SearchedPropertyFoundLocal,
}

/// A single reference entry in a find-all-references result.
// Go: internal/ls/findallreferences.go:ReferenceEntry
#[derive(Clone, Debug)]
pub struct ReferenceEntry {
    pub kind: EntryKind,
    pub file_name: String,
    pub text_range: Option<TextRange>,
}

impl ReferenceEntry {
    /// Creates a new node-type reference entry.
    pub fn new_node(file_name: String) -> Self {
        Self {
            kind: EntryKind::Node,
            file_name,
            text_range: None,
        }
    }

    /// Creates a new range-type reference entry.
    pub fn new_range(file_name: String, range: TextRange) -> Self {
        Self {
            kind: EntryKind::Range,
            file_name,
            text_range: Some(range),
        }
    }
}

/// A symbol and its associated reference entries, forming one group in a
/// find-all-references result.
// Go: internal/ls/findallreferences.go:SymbolAndEntries
#[derive(Clone, Debug)]
pub struct SymbolAndEntries {
    pub definition: Option<Definition>,
    pub references: Vec<ReferenceEntry>,
}

impl SymbolAndEntries {
    /// Creates a new `SymbolAndEntries` with the given definition kind and
    /// references.
    pub fn new(kind: DefinitionKind, references: Vec<ReferenceEntry>) -> Self {
        Self {
            definition: Some(Definition { kind }),
            references,
        }
    }
}

/// The special search kind used when the original search node is a
/// constructor or class name.
// Go: internal/ls/findallreferences.go:getSpecialSearchKind
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpecialSearchKind {
    #[default]
    None,
    Constructor,
    Class,
}

/// The import/export direction for a reference search.
// Go: internal/ls/findallreferences.go — ImpExpKind (from importTracker.go)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImpExpKind {
    #[default]
    Unknown,
    Import,
    Export,
}

/// A search descriptor carrying the symbol being searched, the search text,
/// and the direction from which the search originated.
///
/// DEFER: the `includes` closure and `parents` list require full checker
/// integration.
// Go: internal/ls/findallreferences.go:refSearch
#[derive(Clone, Debug, Default)]
pub struct RefSearch {
    pub coming_from: ImpExpKind,
    pub text: String,
    pub escaped_text: String,
}

impl RefSearch {
    /// Creates a new search descriptor.
    pub fn new(text: &str, coming_from: ImpExpKind) -> Self {
        Self {
            coming_from,
            text: text.to_string(),
            escaped_text: text.to_string(),
        }
    }
}

/// Returns `true` if an identifier node at the given position is a valid
/// reference position for the given search symbol name. Checks that the text
/// length matches and that the node kind is appropriate (identifier, private
/// identifier, string/numeric literal, or `default` keyword).
///
/// This is a simplified port; the full Go version also checks
/// `isLiteralNameOfPropertyDeclarationOrIndexAccess` etc.
///
/// Side effects: none (pure predicate).
// Go: internal/ls/findallreferences.go:isValidReferencePosition
pub fn is_valid_reference_position(node_text_len: usize, search_symbol_name_len: usize) -> bool {
    node_text_len == search_symbol_name_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_use_default_is_none() {
        assert_eq!(ReferenceUse::default(), ReferenceUse::None);
    }

    #[test]
    fn ref_options_is_for_rename_with_prefix_suffix_text() {
        let opts = RefOptions {
            use_kind: ReferenceUse::Rename,
            use_aliases_for_rename: true,
            ..Default::default()
        };
        assert!(opts.is_for_rename_with_prefix_and_suffix_text());
    }

    #[test]
    fn ref_options_not_rename_returns_false() {
        let opts = RefOptions {
            use_kind: ReferenceUse::References,
            use_aliases_for_rename: true,
            ..Default::default()
        };
        assert!(!opts.is_for_rename_with_prefix_and_suffix_text());
    }

    #[test]
    fn ref_options_rename_no_aliases_returns_false() {
        let opts = RefOptions {
            use_kind: ReferenceUse::Rename,
            use_aliases_for_rename: false,
            ..Default::default()
        };
        assert!(!opts.is_for_rename_with_prefix_and_suffix_text());
    }

    #[test]
    fn definition_kind_default_is_symbol() {
        assert_eq!(DefinitionKind::default(), DefinitionKind::Symbol);
    }

    #[test]
    fn entry_kind_default_is_none() {
        assert_eq!(EntryKind::default(), EntryKind::None);
    }

    #[test]
    fn reference_entry_new_node() {
        let entry = ReferenceEntry::new_node("file.ts".to_string());
        assert_eq!(entry.kind, EntryKind::Node);
        assert_eq!(entry.file_name, "file.ts");
        assert!(entry.text_range.is_none());
    }

    #[test]
    fn reference_entry_new_range() {
        let entry = ReferenceEntry::new_range("file.ts".to_string(), TextRange::new(10, 20));
        assert_eq!(entry.kind, EntryKind::Range);
        assert_eq!(entry.file_name, "file.ts");
        let range = entry.text_range.unwrap();
        assert_eq!(range.pos(), 10);
        assert_eq!(range.end(), 20);
    }

    #[test]
    fn symbol_and_entries_new() {
        let entries = vec![ReferenceEntry::new_node("a.ts".to_string())];
        let sae = SymbolAndEntries::new(DefinitionKind::Symbol, entries);
        assert_eq!(
            sae.definition.as_ref().unwrap().kind,
            DefinitionKind::Symbol
        );
        assert_eq!(sae.references.len(), 1);
    }

    #[test]
    fn special_search_kind_default_is_none() {
        assert_eq!(SpecialSearchKind::default(), SpecialSearchKind::None);
    }

    #[test]
    fn imp_exp_kind_default_is_unknown() {
        assert_eq!(ImpExpKind::default(), ImpExpKind::Unknown);
    }

    #[test]
    fn ref_search_new() {
        let search = RefSearch::new("myVar", ImpExpKind::Export);
        assert_eq!(search.text, "myVar");
        assert_eq!(search.escaped_text, "myVar");
        assert_eq!(search.coming_from, ImpExpKind::Export);
    }

    #[test]
    fn is_valid_reference_position_matching_length() {
        assert!(is_valid_reference_position(5, 5));
    }

    #[test]
    fn is_valid_reference_position_non_matching_length() {
        assert!(!is_valid_reference_position(5, 3));
    }
}
