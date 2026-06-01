//! Turning an unresolved name into ranked auto-import candidates.
//!
//! Reachable port of the search/candidate half of Go
//! `internal/ls/autoimport/view.go` (`View.Search` + the candidate-building of
//! `View.GetCompletions`): search the export [`Index`] for a typed name and,
//! for each hit, compute the module specifier to import it by.
//!
//! DEFER(phase-checker / program / ls-root): the full `GetCompletions`
//! grouping/ranking (`CompareFixesForRanking` / `CompareFixesForSorting`), the
//! per-`node_modules`-bucket shadowing walk, the `package.json`-dependency
//! allow-list, and the actual `Fix` (text edit) construction all need the
//! `Registry` buckets, the program, and `ls/change`. blocked-by:
//! `tsgo_compiler` + `tsgo_checker` + the `ls` root.

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_ls_lsutil::ScriptElementKind;
use tsgo_modulespecifiers::{
    ModuleSpecifierGenerationHost, ResultKind, SourceFileForSpecifierGeneration, UserPreferences,
};

use crate::export::Export;
use crate::index::Index;
use crate::specifiers::get_module_specifier;

/// How the typed text is matched against indexed export names.
///
/// Side effects: none (plain data).
// Go: internal/ls/autoimport/view.go:QueryKind
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    /// Fuzzy word-prefix match (`bar` matches `fooBar`).
    WordPrefix,
    /// Exact, case-sensitive match.
    ExactMatch,
    /// Exact, case-insensitive match.
    CaseInsensitiveMatch,
}

/// One auto-import suggestion: the name to import, the specifier to import it
/// from, and the export's element kind + the strategy that produced the
/// specifier.
///
/// Side effects: none (plain data).
// Go: internal/ls/autoimport/view.go:FixAndExport (reachable subset)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportCandidate {
    /// The name to import (the export's display name).
    pub name: String,
    /// The module specifier to import it from.
    pub specifier: String,
    /// The export's element kind (variable / function / class / ...).
    pub kind: ScriptElementKind,
    /// Which strategy produced the specifier (relative / ambient / ...).
    pub result_kind: ResultKind,
}

/// Searches `index` for exports matching `query` under `kind`, excluding any
/// export declared in `importing_file` itself.
///
/// Side effects: none (pure).
// Go: internal/ls/autoimport/view.go:View.search
pub fn search_index<'a>(
    index: &'a Index<Export>,
    query: &str,
    kind: QueryKind,
    importing_file: &dyn SourceFileForSpecifierGeneration,
) -> Vec<&'a Export> {
    let matches = match kind {
        QueryKind::WordPrefix => index.search_word_prefix(query),
        QueryKind::ExactMatch => index.find(query, true),
        QueryKind::CaseInsensitiveMatch => index.find(query, false),
    };
    let importing_path = importing_file.path();
    matches
        .into_iter()
        // Don't auto-import from the importing file itself.
        .filter(|e| e.id.module_id.as_str() != importing_path.as_str())
        .collect()
}

/// Searches `index` for `query` and computes an [`ImportCandidate`] for every
/// match that can be imported (drops self-imports and exports with no usable
/// specifier).
///
/// Side effects: reads `host` for specifier generation.
// Go: internal/ls/autoimport/view.go:View.GetCompletions (reachable subset)
pub fn find_import_candidates(
    index: &Index<Export>,
    query: &str,
    kind: QueryKind,
    importing_file: &dyn SourceFileForSpecifierGeneration,
    compiler_options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
    user_preferences: &UserPreferences,
) -> Vec<ImportCandidate> {
    let mut candidates = Vec::new();
    for export in search_index(index, query, kind, importing_file) {
        let (specifier, result_kind) = get_module_specifier(
            export,
            importing_file,
            compiler_options,
            host,
            user_preferences,
        );
        // An empty specifier means the export cannot be imported here.
        if specifier.is_empty() {
            continue;
        }
        candidates.push(ImportCandidate {
            name: export.name(),
            specifier,
            kind: export.script_element_kind,
            result_kind,
        });
    }
    candidates
}

#[cfg(test)]
#[path = "view_test.rs"]
mod tests;
