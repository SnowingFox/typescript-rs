//! Port of Go `internal/ls/codeactions.go`: the code-actions feature.
//!
//! Go's `ProvideCodeActions` collects quick-fix and source-action code actions
//! for a given range and diagnostic context: it iterates registered
//! [`CodeFixProvider`]s, matches them against error codes from the diagnostics
//! in the request, and builds [`lsproto::CodeAction`]-style responses.
//!
//! # Reachable subset
//!
//! This round ports the framework types and the entry-point signature:
//! [`CodeFixProvider`], [`CodeFixContext`], [`CodeAction`],
//! [`CombinedCodeActions`], the hierarchical kind matching helpers
//! ([`code_action_kind_contains`], [`is_fix_all_kind`], [`wants_quick_fixes`]),
//! and [`get_organize_imports_actions_for_kind`].
//!
//! DEFER(phase-7-ls): the actual provider implementations
//! (`ImportFixProvider`, `IsolatedDeclarationsFixProvider`,
//! `FixClassIncorrectlyImplementsInterfaceProvider`), the `createFixAllAction`
//! logic that aggregates all providers, the `getFixAllQuickFixes` deduplication,
//! and the full `ProvideCodeActions` orchestration — these require the change
//! tracker, the checker's diagnostics API, and the organize-imports
//! infrastructure.

use std::cmp::Ordering;

/// A registered code-fix provider that can supply quick-fix actions for a set
/// of error codes, and optionally a "fix all" action.
///
/// Side effects: none (a configuration record; callbacks are deferred).
// Go: internal/ls/codeactions.go:CodeFixProvider
#[derive(Clone, Debug)]
pub struct CodeFixProvider {
    /// The diagnostic error codes this provider handles.
    pub error_codes: Vec<i32>,
    /// Identifiers for "fix all in file" grouping.
    pub fix_ids: Vec<String>,
}

impl CodeFixProvider {
    /// Returns `true` if this provider handles the given error code.
    // Go: internal/ls/codeactions.go:containsErrorCode
    pub fn handles_error_code(&self, code: i32) -> bool {
        self.error_codes.contains(&code)
    }
}

/// The context passed to a code-fix provider when generating fixes.
///
/// Contains the source file, the span of the diagnostic, the error code, and
/// references to the program and language service.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/codeactions.go:CodeFixContext
#[derive(Clone, Debug)]
pub struct CodeFixContext {
    /// The position span of the error.
    pub span_start: i32,
    pub span_end: i32,
    /// The diagnostic error code.
    pub error_code: i32,
}

/// A single code action fix result.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/codeactions.go:CodeAction
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeAction {
    /// A human-readable description of the fix.
    pub description: String,
    /// A stable identifier used for "fix all" grouping. Empty if not
    /// applicable.
    pub fix_id: String,
    /// A human-readable description for the "fix all" variant. Empty if not
    /// applicable.
    pub fix_all_description: String,
}

impl CodeAction {
    /// Defines a total ordering for `CodeAction` values by comparing
    /// `description` first (lexicographic), then `fix_id`.
    // Go: internal/ls/codeactions.go:CodeAction.Compare
    pub fn compare(&self, other: &CodeAction) -> Ordering {
        self.description
            .cmp(&other.description)
            .then_with(|| self.fix_id.cmp(&other.fix_id))
    }
}

impl PartialOrd for CodeAction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(std::cmp::Ord::cmp(self, other))
    }
}

impl Ord for CodeAction {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other)
    }
}

/// Combined code actions for "fix all in file" scenarios.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/codeactions.go:CombinedCodeActions
#[derive(Clone, Debug, Default)]
pub struct CombinedCodeActions {
    /// A human-readable description of the combined fix.
    pub description: String,
}

/// The LSP `CodeActionKind` values used by the organize-imports and fix-all
/// features, matching `lsproto.CodeActionKind*` constants from Go.
pub mod code_action_kind {
    pub const QUICK_FIX: &str = "quickfix";
    pub const SOURCE_ORGANIZE_IMPORTS: &str = "source.organizeImports";
    pub const SOURCE_REMOVE_UNUSED_IMPORTS: &str = "source.removeUnusedImports";
    pub const SOURCE_SORT_IMPORTS: &str = "source.sortImports";
    pub const SOURCE_FIX_ALL: &str = "source.fixAll";
}

/// Returns `true` if `requested_kind` equals or is a hierarchical parent of
/// `action_kind`, using `'.'` as the separator. This matches the semantics of
/// VS Code's `HierarchicalKind.contains`.
///
/// Side effects: none (pure string comparison).
// Go: internal/ls/codeactions.go:codeActionKindContains
pub fn code_action_kind_contains(requested_kind: &str, action_kind: &str) -> bool {
    requested_kind == action_kind
        || requested_kind.is_empty()
        || action_kind.starts_with(&format!("{}.", requested_kind))
}

/// Returns `true` if the requested kind matches `source.fixAll`.
///
/// Side effects: none.
// Go: internal/ls/codeactions.go:isFixAllKind
pub fn is_fix_all_kind(kind: &str) -> bool {
    code_action_kind_contains(kind, code_action_kind::SOURCE_FIX_ALL)
}

/// Returns `true` if the `only` filter is `None`/empty (meaning all kinds are
/// wanted) or explicitly includes the `quickfix` kind.
///
/// Side effects: none.
// Go: internal/ls/codeactions.go:wantsQuickFixes
pub fn wants_quick_fixes(only: Option<&[String]>) -> bool {
    match only {
        None => true,
        Some([]) => true,
        Some(kinds) => kinds
            .iter()
            .any(|k| code_action_kind_contains(k, code_action_kind::QUICK_FIX)),
    }
}

/// Returns the organize-imports code-action kinds that should be returned for
/// the given requested kind.
///
/// Side effects: none.
// Go: internal/ls/codeactions.go:getOrganizeImportsActionsForKind
pub fn get_organize_imports_actions_for_kind(requested_kind: &str) -> Vec<&str> {
    let organize_kinds: &[&str] = &[
        code_action_kind::SOURCE_ORGANIZE_IMPORTS,
        code_action_kind::SOURCE_REMOVE_UNUSED_IMPORTS,
        code_action_kind::SOURCE_SORT_IMPORTS,
    ];

    let result: Vec<&str> = organize_kinds
        .iter()
        .filter(|&&kind| code_action_kind_contains(requested_kind, kind))
        .copied()
        .collect();

    if result.contains(&requested_kind) {
        return vec![requested_kind];
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_action_kind_contains_exact_match() {
        assert!(code_action_kind_contains("quickfix", "quickfix"));
    }

    #[test]
    fn code_action_kind_contains_empty_requested_matches_anything() {
        assert!(code_action_kind_contains("", "quickfix"));
        assert!(code_action_kind_contains("", "source.organizeImports"));
    }

    #[test]
    fn code_action_kind_contains_hierarchical_parent() {
        assert!(code_action_kind_contains("source", "source.fixAll"));
        assert!(code_action_kind_contains(
            "source",
            "source.organizeImports"
        ));
    }

    #[test]
    fn code_action_kind_contains_non_match() {
        assert!(!code_action_kind_contains("quickfix", "source.fixAll"));
        assert!(!code_action_kind_contains(
            "source.fixAll",
            "source.fixAllInFile"
        ));
    }

    #[test]
    fn is_fix_all_kind_matches() {
        assert!(is_fix_all_kind("source.fixAll"));
        assert!(is_fix_all_kind("source"));
        assert!(is_fix_all_kind(""));
        assert!(!is_fix_all_kind("quickfix"));
    }

    #[test]
    fn wants_quick_fixes_none_returns_true() {
        assert!(wants_quick_fixes(None));
    }

    #[test]
    fn wants_quick_fixes_empty_returns_true() {
        assert!(wants_quick_fixes(Some(&[])));
    }

    #[test]
    fn wants_quick_fixes_with_quickfix_returns_true() {
        let kinds = vec!["quickfix".to_string()];
        assert!(wants_quick_fixes(Some(&kinds)));
    }

    #[test]
    fn wants_quick_fixes_without_quickfix_returns_false() {
        let kinds = vec!["source.organizeImports".to_string()];
        assert!(!wants_quick_fixes(Some(&kinds)));
    }

    #[test]
    fn get_organize_imports_actions_exact_match() {
        let result =
            get_organize_imports_actions_for_kind(code_action_kind::SOURCE_ORGANIZE_IMPORTS);
        assert_eq!(result, vec![code_action_kind::SOURCE_ORGANIZE_IMPORTS]);
    }

    #[test]
    fn get_organize_imports_actions_parent_kind() {
        let result = get_organize_imports_actions_for_kind("source");
        assert_eq!(result.len(), 3);
        assert!(result.contains(&code_action_kind::SOURCE_ORGANIZE_IMPORTS));
        assert!(result.contains(&code_action_kind::SOURCE_REMOVE_UNUSED_IMPORTS));
        assert!(result.contains(&code_action_kind::SOURCE_SORT_IMPORTS));
    }

    #[test]
    fn get_organize_imports_actions_no_match() {
        let result = get_organize_imports_actions_for_kind("quickfix");
        assert!(result.is_empty());
    }

    #[test]
    fn code_action_ordering() {
        let a = CodeAction {
            description: "Add import".to_string(),
            fix_id: "addImport".to_string(),
            fix_all_description: String::new(),
        };
        let b = CodeAction {
            description: "Remove unused".to_string(),
            fix_id: "removeUnused".to_string(),
            fix_all_description: String::new(),
        };
        assert_eq!(a.compare(&a), Ordering::Equal);
        assert_eq!(a.compare(&b), Ordering::Less);
        assert_eq!(b.compare(&a), Ordering::Greater);
    }

    #[test]
    fn code_fix_provider_handles_error_code() {
        let provider = CodeFixProvider {
            error_codes: vec![1001, 1002, 1003],
            fix_ids: vec!["fix1".to_string()],
        };
        assert!(provider.handles_error_code(1001));
        assert!(provider.handles_error_code(1003));
        assert!(!provider.handles_error_code(9999));
    }

    #[test]
    fn code_action_sort_stability() {
        let mut actions = vec![
            CodeAction {
                description: "Z fix".to_string(),
                fix_id: "a".to_string(),
                fix_all_description: String::new(),
            },
            CodeAction {
                description: "A fix".to_string(),
                fix_id: "b".to_string(),
                fix_all_description: String::new(),
            },
            CodeAction {
                description: "A fix".to_string(),
                fix_id: "a".to_string(),
                fix_all_description: String::new(),
            },
        ];
        actions.sort();
        assert_eq!(actions[0].fix_id, "a");
        assert_eq!(actions[0].description, "A fix");
        assert_eq!(actions[1].fix_id, "b");
        assert_eq!(actions[1].description, "A fix");
        assert_eq!(actions[2].description, "Z fix");
    }
}
