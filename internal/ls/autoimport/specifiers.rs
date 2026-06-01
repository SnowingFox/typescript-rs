//! Module-specifier selection for an export.
//!
//! Reachable port of Go `internal/ls/autoimport/specifiers.go`
//! (`View.GetModuleSpecifier`): given the export to import and the importing
//! file, pick the import-path string. Ambient (bare) module ids are returned
//! verbatim; otherwise [`tsgo_modulespecifiers`] computes the candidate
//! specifiers and the first non-`node_modules` candidate wins.
//!
//! DEFER(phase-checker / program): the `PackageName != ""` entrypoint branch
//! (needs the registry's resolved `node_modules` entrypoints + the program's
//! resolution conditions) and the per-importing-file specifier cache are
//! omitted. blocked-by: `tsgo_compiler` program + the full `Registry`.

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_modulespecifiers::{
    get_module_specifiers_for_file_with_info, is_excluded_by_regex, path_is_bare_specifier,
    ModuleSpecifierGenerationHost, ModuleSpecifierOptions, ResultKind,
    SourceFileForSpecifierGeneration, UserPreferences,
};

use crate::export::Export;

/// Computes the import specifier and [`ResultKind`] for importing `export` into
/// `importing_file`.
///
/// Mirrors `View.GetModuleSpecifier`: a bare/ambient module id is returned as
/// the specifier (unless excluded by the user's regexes); otherwise the first
/// generated candidate that does not pass through `/node_modules/` is returned.
///
/// Side effects: reads `host` (file system / symlink cache / package.json).
// Go: internal/ls/autoimport/specifiers.go:View.GetModuleSpecifier
pub fn get_module_specifier(
    export: &Export,
    importing_file: &dyn SourceFileForSpecifierGeneration,
    compiler_options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
    user_preferences: &UserPreferences,
) -> (String, ResultKind) {
    // Ambient (bare) module: the module id is itself the specifier.
    if path_is_bare_specifier(export.id.module_id.as_str()) {
        let specifier = export.id.module_id.0.clone();
        if is_excluded_by_regex(
            &specifier,
            &user_preferences.auto_import_specifier_exclude_regexes,
        ) {
            return (String::new(), ResultKind::None);
        }
        return (specifier, ResultKind::Ambient);
    }

    // DEFER(phase-checker / program): the `export.PackageName != ""` branch
    // (resolving a `node_modules` entrypoint) and the per-file specifier cache.

    let (specifiers, kind) = get_module_specifiers_for_file_with_info(
        importing_file,
        &export.module_file_name,
        compiler_options,
        host,
        user_preferences,
        ModuleSpecifierOptions::default(),
        true,
    );
    // Prefer the first candidate that does not route through node_modules.
    for specifier in specifiers {
        if specifier.contains("/node_modules/") {
            continue;
        }
        return (specifier, kind);
    }
    (String::new(), ResultKind::None)
}

#[cfg(test)]
#[path = "specifiers_test.rs"]
mod tests;
