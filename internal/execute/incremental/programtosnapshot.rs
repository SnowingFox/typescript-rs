//! Builds a [`Snapshot`] from a freshly-built [`tsgo_compiler::Program`]:
//! per-file content versions plus the `referencedMap` import graph.
//!
//! Reachable subset of Go `internal/execute/incremental/programtosnapshot.go`.
//! The old-program reuse/diff (`reuseFromOldProgram`, `computeProgramFileChanges`
//! change detection, pending-emit/check) is DEFER (P6-9b old-state reuse).

use tsgo_collections::Set;
use tsgo_compiler::Program as CompilerProgram;
use tsgo_tspath::{combine_paths, get_directory_path, Path};

use crate::snapshot::{compute_file_version, FileInfo, Snapshot, RESOLUTION_MODE_COMMON_JS};

/// Candidate extensions appended to a relative import specifier when resolving
/// it against the program's file set (reachable subset of module resolution).
const RESOLVE_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx", ".json", ""];

/// Builds a fresh-build [`Snapshot`] from a compiler program: each source file's
/// content `version` (with `signature == version`) and the import graph derived
/// from its module specifiers.
///
/// Mirrors Go `programToSnapshot` for the `oldProgram == nil` path. Per-file
/// `affectsGlobalScope` and `impliedNodeFormat` use the reachable defaults
/// (`false` / CommonJS); see the module DEFER note.
///
/// Side effects: none (reads the already-parsed program).
// Go: internal/execute/incremental/programtosnapshot.go:programToSnapshot
pub fn program_to_snapshot(program: &CompilerProgram) -> Snapshot {
    let mut snapshot = Snapshot {
        options: program.options().clone(),
        ..Snapshot::default()
    };

    for file in program.source_files() {
        let path = program.to_path(file.file_name());
        let version = compute_file_version(file.text());

        // Resolve this file's references (its imports) to paths in the program.
        // Go uses the checker's resolved symbols (`getReferencedFiles`); the
        // reachable subset re-resolves relative specifiers against the file set.
        let referenced = referenced_files(program, file.file_name(), &file.import_specifiers());
        if !referenced.is_empty() {
            let refs = Set::from_items(referenced);
            snapshot.referenced_map.store_references(path.clone(), refs);
        }

        // Fresh build: signature defaults to version (Go: oldProgram == nil).
        snapshot.file_infos.insert(
            path,
            FileInfo {
                signature: version.clone(),
                version,
                affects_global_scope: false,
                implied_node_format: RESOLUTION_MODE_COMMON_JS,
            },
        );
    }

    snapshot
}

/// Resolves the relative module specifiers of `importer` to the set of program
/// file paths they reference.
///
/// Side effects: none (pure path arithmetic + program membership checks).
// Go: internal/execute/incremental/programtosnapshot.go:getReferencedFiles
// DEFER(P6-9b): use the checker's resolved module symbols (handles bare module
// specifiers, triple-slash/type refs, ambient/augmentation modules); reachable
// subset only follows relative specifiers that resolve into the program.
fn referenced_files(
    program: &CompilerProgram,
    importer_file_name: &str,
    specifiers: &[String],
) -> Vec<Path> {
    let importer_dir = get_directory_path(importer_file_name);
    let mut result = Vec::new();
    for spec in specifiers {
        if !spec.starts_with('.') {
            // Bare/non-relative specifiers need the checker's resolver: DEFER.
            continue;
        }
        let base = combine_paths(&importer_dir, &[spec]);
        for ext in RESOLVE_EXTENSIONS {
            let candidate = format!("{base}{ext}");
            let path = program.to_path(&candidate);
            if program.get_source_file_by_path(&path).is_some() {
                result.push(path);
                break;
            }
        }
    }
    result
}
