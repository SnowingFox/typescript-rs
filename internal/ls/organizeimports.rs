//! Port of Go `internal/ls/organizeimports.go`: the organize-imports feature.
//!
//! Go's `OrganizeImports` removes unused imports, coalesces imports from the
//! same module specifier, and sorts imports — with three sub-kinds:
//! `source.organizeImports`, `source.removeUnusedImports`, and
//! `source.sortImports`.
//!
//! # Reachable subset
//!
//! This round ports the framework types and the pure helper functions:
//! - [`OrganizeImportsMode`]: the three organize-imports sub-kinds.
//! - [`OrganizeImportsComparerSettings`]: the comparer configuration.
//! - [`ImportGroup`] / [`CategorizedImports`] / [`CategorizedExports`]: the
//!   grouping types used by the organize workers.
//! - [`group_by_module_specifier`]: groups imports by their module specifier
//!   string.
//! - [`get_import_attributes_key`]: produces a grouping key from an import's
//!   attributes.
//!
//! DEFER(phase-7-ls): the full `OrganizeImports` method (requires
//! `tsgo_ls_change::Tracker`, `tsgo_ls_lsutil::FilterImportDeclarations`,
//! `tsgo_ls_lsutil::GetDetectionLists`, `tsgo_checker::IsDeclarationUsed`, and
//! the AST factory's `UpdateImportDeclaration`), the newline-contiguous grouping
//! (`groupByNewlineContiguous` — requires `tsgo_scanner::SkipTrivia`), and the
//! coalesce/sort workers (`coalesceImportsWorker`, `organizeExportsWorker`).

use std::collections::HashMap;

/// The three sub-kinds for the organize-imports code action, matching
/// `lsproto.CodeActionKind*` constants from Go.
// Go: inferred from organizeimports.go usage
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OrganizeImportsMode {
    /// Organize: remove unused, coalesce, and sort.
    OrganizeImports,
    /// Only remove unused imports.
    RemoveUnusedImports,
    /// Only sort imports (and coalesce).
    SortImports,
}

impl OrganizeImportsMode {
    /// Returns `true` if this mode should sort imports.
    pub fn should_sort(self) -> bool {
        matches!(
            self,
            OrganizeImportsMode::SortImports | OrganizeImportsMode::OrganizeImports
        )
    }

    /// Returns `true` if this mode should remove unused imports.
    pub fn should_remove(self) -> bool {
        matches!(
            self,
            OrganizeImportsMode::RemoveUnusedImports | OrganizeImportsMode::OrganizeImports
        )
    }

    /// Returns `true` if this mode should coalesce imports (same condition as
    /// `should_sort` in Go).
    pub fn should_combine(self) -> bool {
        self.should_sort()
    }
}

/// Comparer settings for organizing imports.
///
/// Side effects: none (a configuration record).
// Go: internal/ls/organizeimports.go:organizeImportsComparerSettings
#[derive(Clone, Debug, Default)]
pub struct OrganizeImportsComparerSettings {
    /// The type ordering preference (`"first"`, `"last"`, `"inline"`, or
    /// `"auto"`).
    pub type_order: String,
}

/// A group of import declarations, categorized by their binding type.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/organizeimports.go:importGroup
#[derive(Clone, Debug, Default)]
pub struct ImportGroup {
    /// Imports with a default binding.
    pub default_imports: Vec<String>,
    /// Imports with a namespace binding (`* as ns`).
    pub namespace_imports: Vec<String>,
    /// Imports with named bindings (`{ a, b }`).
    pub named_imports: Vec<String>,
}

impl ImportGroup {
    /// Returns `true` if this group has no imports in any category.
    // Go: internal/ls/organizeimports.go:importGroup.isEmpty
    pub fn is_empty(&self) -> bool {
        self.default_imports.is_empty()
            && self.namespace_imports.is_empty()
            && self.named_imports.is_empty()
    }
}

/// Categorized imports split into side-effect-only, regular, and type-only
/// groups.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/organizeimports.go:categorizedImports
#[derive(Clone, Debug, Default)]
pub struct CategorizedImports {
    /// The first import with no import clause (side-effect-only).
    pub import_without_clause: Option<String>,
    /// Type-only imports (`import type { ... }`).
    pub type_only_imports: ImportGroup,
    /// Regular imports.
    pub regular_imports: ImportGroup,
}

/// Categorized exports split into bare re-exports, named exports, and
/// type-only exports.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/organizeimports.go:categorizedExports
#[derive(Clone, Debug, Default)]
pub struct CategorizedExports {
    /// The first export with no export clause (`export * from "mod"`).
    pub export_without_clause: Option<String>,
    /// Named exports.
    pub named_exports: Vec<String>,
    /// Type-only exports.
    pub type_only_exports: Vec<String>,
}

/// Groups a list of import specifier strings by their module specifier,
/// preserving insertion order.
///
/// Returns a `Vec` of `(specifier, imports)` groups in the order the specifiers
/// first appear.
///
/// Side effects: none.
// Go: internal/ls/organizeimports.go:groupByModuleSpecifier
pub fn group_by_module_specifier(imports: &[(&str, &str)]) -> Vec<(String, Vec<String>)> {
    let mut groups: HashMap<&str, Vec<String>> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();

    for &(specifier, import_text) in imports {
        if !groups.contains_key(specifier) {
            order.push(specifier);
        }
        groups
            .entry(specifier)
            .or_default()
            .push(import_text.to_string());
    }

    order
        .into_iter()
        .map(|key| {
            let entries = groups.remove(key).unwrap_or_default();
            (key.to_string(), entries)
        })
        .collect()
}

/// Returns a grouping key string from an import's attribute pairs, suitable
/// for grouping imports by their assertion/attribute clauses.
///
/// The key is deterministic: attribute pairs are sorted by name, each rendered
/// as `name:value `, prefixed by the assertion token (e.g., `assert` or
/// `with`).
///
/// Returns an empty string if no attributes are present.
///
/// Side effects: none.
// Go: internal/ls/organizeimports.go:getImportAttributesKey
pub fn get_import_attributes_key(token: &str, attributes: &[(&str, &str)]) -> String {
    if attributes.is_empty() {
        return String::new();
    }

    let mut sorted_attrs: Vec<(&str, &str)> = attributes.to_vec();
    sorted_attrs.sort_by_key(|&(name, _)| name);

    let mut key = String::with_capacity(token.len() + 1 + sorted_attrs.len() * 16);
    key.push_str(token);
    key.push(' ');

    for (name, value) in &sorted_attrs {
        key.push_str(name);
        key.push(':');
        key.push('"');
        key.push_str(value);
        key.push('"');
        key.push(' ');
    }

    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organize_mode_should_sort() {
        assert!(OrganizeImportsMode::OrganizeImports.should_sort());
        assert!(OrganizeImportsMode::SortImports.should_sort());
        assert!(!OrganizeImportsMode::RemoveUnusedImports.should_sort());
    }

    #[test]
    fn organize_mode_should_remove() {
        assert!(OrganizeImportsMode::OrganizeImports.should_remove());
        assert!(OrganizeImportsMode::RemoveUnusedImports.should_remove());
        assert!(!OrganizeImportsMode::SortImports.should_remove());
    }

    #[test]
    fn organize_mode_should_combine_equals_should_sort() {
        assert!(OrganizeImportsMode::OrganizeImports.should_combine());
        assert!(OrganizeImportsMode::SortImports.should_combine());
        assert!(!OrganizeImportsMode::RemoveUnusedImports.should_combine());
    }

    #[test]
    fn import_group_is_empty() {
        let g = ImportGroup::default();
        assert!(g.is_empty());

        let g2 = ImportGroup {
            default_imports: vec!["foo".to_string()],
            ..Default::default()
        };
        assert!(!g2.is_empty());
    }

    #[test]
    fn group_by_module_specifier_preserves_order() {
        let imports = vec![
            ("react", "import React from 'react'"),
            ("lodash", "import _ from 'lodash'"),
            ("react", "import { useState } from 'react'"),
        ];
        let grouped = group_by_module_specifier(&imports);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "react");
        assert_eq!(grouped[0].1.len(), 2);
        assert_eq!(grouped[1].0, "lodash");
        assert_eq!(grouped[1].1.len(), 1);
    }

    #[test]
    fn group_by_module_specifier_empty() {
        let grouped = group_by_module_specifier(&[]);
        assert!(grouped.is_empty());
    }

    #[test]
    fn get_import_attributes_key_empty_attrs() {
        assert_eq!(get_import_attributes_key("assert", &[]), "");
    }

    #[test]
    fn get_import_attributes_key_sorts_by_name() {
        let attrs = vec![("type", "json"), ("encoding", "utf-8")];
        let key = get_import_attributes_key("with", &attrs);
        assert!(key.starts_with("with "));
        let encoding_pos = key.find("encoding:").unwrap();
        let type_pos = key.find("type:").unwrap();
        assert!(encoding_pos < type_pos, "encoding should sort before type");
    }

    #[test]
    fn get_import_attributes_key_single_attr() {
        let attrs = vec![("type", "json")];
        let key = get_import_attributes_key("assert", &attrs);
        assert_eq!(key, "assert type:\"json\" ");
    }

    #[test]
    fn categorized_imports_default_is_empty() {
        let cat = CategorizedImports::default();
        assert!(cat.import_without_clause.is_none());
        assert!(cat.type_only_imports.is_empty());
        assert!(cat.regular_imports.is_empty());
    }
}
