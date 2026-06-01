//! Building an export index over a set of files.
//!
//! This is the **reachable analog** of the bucket-building in Go
//! `internal/ls/autoimport/registry.go` (`buildProjectBucket`): parse every
//! file, extract its top-level exports, and insert them into a single
//! [`Index<Export>`]. The full `Registry` — incremental dirty-map buckets, the
//! `node_modules` discovery/extraction pipeline, project-reference redirects,
//! and the checker-driven cross-package alias/ambient-module resolution — is
//! deferred (see the crate worklog).
//!
//! As a small reachable extension this also resolves a single level of
//! `export * from "./relative"` against the supplied file set, since that needs
//! only the parsed ASTs (Go uses the checker's `GetExportsOfModule`).
//! DEFER(phase-checker): recursive / cross-package `export *` enumeration and
//! alias-target resolution. blocked-by: `tsgo_checker`.

use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};
use tsgo_tspath::{get_directory_path, remove_file_extension, resolve_path, to_path, Path};

use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_EXPORT_STAR;

use crate::export::{Export, ExportId, ExportSyntax, ModuleId};
use crate::extract::{collect_star_reexport_specifiers, extract_top_level_exports};
use crate::index::Index;

/// One in-memory file to index: its name and full text.
///
/// Side effects: none (plain data).
#[derive(Clone, Copy, Debug)]
pub struct FileInput<'a> {
    /// The file's name (drives path/script-kind and the export's `ModuleId`).
    pub file_name: &'a str,
    /// The file's source text.
    pub text: &'a str,
}

impl<'a> FileInput<'a> {
    /// Convenience constructor.
    ///
    /// Side effects: none (pure).
    pub fn new(file_name: &'a str, text: &'a str) -> FileInput<'a> {
        FileInput { file_name, text }
    }
}

/// Parses and extracts every file in `files`, returning a single combined
/// [`Index<Export>`] that maps each exported name to the file it lives in.
///
/// `current_dir` and `use_case_sensitive` drive canonical-path computation
/// (`tspath::to_path`), exactly as the language-service host would.
///
/// Side effects: none (parses in-memory text only).
// Go: internal/ls/autoimport/registry.go:registryBuilder.buildProjectBucket
pub fn build_index_for_files(
    files: &[FileInput],
    current_dir: &str,
    use_case_sensitive: bool,
) -> Index<Export> {
    // Phase 1: parse + extract each file's direct exports and star specifiers.
    let extracted: Vec<ExtractedFile> = files
        .iter()
        .map(|f| {
            let result = parse_source_file(
                SourceFileParseOptions {
                    file_name: f.file_name.to_string(),
                },
                f.text,
                script_kind_for(f.file_name),
            );
            let path = to_path(f.file_name, current_dir, use_case_sensitive);
            let exports = extract_top_level_exports(&result.arena, result.source_file, &path);
            let star_specifiers =
                collect_star_reexport_specifiers(&result.arena, result.source_file);
            ExtractedFile {
                path,
                file_name: f.file_name.to_string(),
                exports,
                star_specifiers,
            }
        })
        .collect();

    // Phase 2: build the index from direct exports, then add a single level of
    // re-exports for each `export * from "./relative"` resolvable in the set.
    let mut idx: Index<Export> = Index::default();
    for file in &extracted {
        for export in &file.exports {
            idx.insert_as_words(export.clone());
        }
        for specifier in &file.star_specifiers {
            let from_dir = get_directory_path(&file.file_name);
            let Some(target) = resolve_relative_file(&from_dir, specifier, &extracted) else {
                continue;
            };
            for export in &target.exports {
                // Re-export the target's name *through* this file: keep the
                // exported name but attribute it to this file's module id, with
                // the original recorded as the target.
                let mut reexport = Export {
                    id: ExportId {
                        module_id: ModuleId(file.path.0.clone()),
                        export_name: export.id.export_name.clone(),
                    },
                    module_file_name: file.file_name.clone(),
                    syntax: ExportSyntax::Star,
                    flags: export.flags,
                    target: export.id.clone(),
                    script_element_kind: export.script_element_kind,
                    path: file.path.clone(),
                    ..Default::default()
                };
                // Found via `export *`, matching Go's `export.through`.
                reexport.through = INTERNAL_SYMBOL_NAME_EXPORT_STAR.to_string();
                idx.insert_as_words(reexport);
            }
        }
    }
    idx
}

/// Maps a file name to the `ScriptKind` the parser should use, by extension.
fn script_kind_for(file_name: &str) -> ScriptKind {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".tsx") {
        ScriptKind::Tsx
    } else if lower.ends_with(".jsx") {
        ScriptKind::Jsx
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs") {
        ScriptKind::Js
    } else if lower.ends_with(".json") {
        ScriptKind::Json
    } else {
        ScriptKind::Ts
    }
}

/// One parsed-and-extracted file.
struct ExtractedFile {
    path: Path,
    file_name: String,
    exports: Vec<Export>,
    star_specifiers: Vec<String>,
}

/// Resolves a relative `export *` specifier against the parsed file set,
/// returning the matching file's index, or `None`.
fn resolve_relative_file<'a>(
    from_dir: &str,
    specifier: &str,
    files: &'a [ExtractedFile],
) -> Option<&'a ExtractedFile> {
    let resolved = remove_file_extension(&resolve_path(from_dir, &[specifier])).to_string();
    files
        .iter()
        .find(|f| remove_file_extension(f.path.as_str()) == resolved)
}

#[cfg(test)]
#[path = "registry_test.rs"]
mod tests;
