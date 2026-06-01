//! Serializes a [`Snapshot`] into a [`BuildInfo`] for an incremental program.
//!
//! Reachable subset of Go `internal/execute/incremental/snapshottobuildinfo.go`
//! (the incremental-program branch): file names (relativized to the buildinfo
//! directory), compact file infos, root id ranges, the `module`/`target`
//! options subset, the referenced map, and the fresh-build
//! `semanticDiagnosticsPerFile` (every file id, no cached diagnostics).
//! Emit signatures, emit/change diagnostics, pending emit, `latestChangedDtsFile`
//! and the non-incremental branch are DEFER.

use rustc_hash::FxHashMap;
use tsgo_collections::Set;
use tsgo_compiler::Program as CompilerProgram;
use tsgo_core::compileroptions::{ModuleKind, ScriptTarget};
use tsgo_tspath::{
    ensure_path_is_non_module_name, get_directory_path, get_relative_path_from_directory,
    ComparePathsOptions, Path,
};

use crate::build_info::{
    BuildInfo, BuildInfoFileId, BuildInfoFileIdListId, BuildInfoFileInfo,
    BuildInfoReferenceMapEntry, BuildInfoRoot, BuildInfoSemanticDiagnostic,
};
use crate::snapshot::Snapshot;

/// Converts a [`Snapshot`] + its compiler program into the serializable
/// [`BuildInfo`], assigning 1-based file ids in source-file order and
/// relativizing every name to the buildinfo directory.
///
/// Side effects: none (pure transform).
// Go: internal/execute/incremental/snapshottobuildinfo.go:snapshotToBuildInfo
pub fn snapshot_to_build_info(
    snapshot: &Snapshot,
    program: &CompilerProgram,
    build_info_file_name: &str,
) -> BuildInfo {
    let mut to = ToBuildInfo {
        snapshot,
        program,
        build_info: BuildInfo {
            version: tsgo_core::version::version().to_string(),
            ..BuildInfo::default()
        },
        build_info_directory: get_directory_path(build_info_file_name),
        compare_paths_options: ComparePathsOptions {
            current_directory: program.host().get_current_directory().to_string(),
            use_case_sensitive_file_names: program.host().fs().use_case_sensitive_file_names(),
        },
        file_name_to_id: FxHashMap::default(),
        file_ids_list_key_to_id: FxHashMap::default(),
    };

    to.set_file_info();
    to.set_root_of_incremental_program();
    to.set_compiler_options();
    to.set_referenced_map();
    to.set_semantic_diagnostics();
    to.build_info
}

struct ToBuildInfo<'a> {
    snapshot: &'a Snapshot,
    program: &'a CompilerProgram,
    build_info: BuildInfo,
    build_info_directory: String,
    compare_paths_options: ComparePathsOptions,
    file_name_to_id: FxHashMap<String, BuildInfoFileId>,
    file_ids_list_key_to_id: FxHashMap<String, BuildInfoFileIdListId>,
}

impl ToBuildInfo<'_> {
    // Go: snapshottobuildinfo.go:toBuildInfo.relativeToBuildInfo
    fn relative_to_build_info(&self, path: &str) -> String {
        ensure_path_is_non_module_name(&get_relative_path_from_directory(
            &self.build_info_directory,
            path,
            &self.compare_paths_options,
        ))
    }

    // Go: snapshottobuildinfo.go:toBuildInfo.toFileId
    // DEFER(P6-9b): GetDefaultLibFile name special-casing (lib files keep their
    // bare `lib.*.d.ts` name instead of being relativized).
    fn file_id_for(&mut self, path: &Path) -> BuildInfoFileId {
        if let Some(id) = self.file_name_to_id.get(path.as_str()) {
            return *id;
        }
        self.build_info
            .file_names
            .push(self.relative_to_build_info(path.as_str()));
        let id = BuildInfoFileId(self.build_info.file_names.len() as u32);
        self.file_name_to_id.insert(path.as_str().to_string(), id);
        id
    }

    // Go: snapshottobuildinfo.go:toBuildInfo.toFileIdListId
    fn file_id_list_id_for(&mut self, set: &Set<Path>) -> BuildInfoFileIdListId {
        let mut file_ids: Vec<BuildInfoFileId> = set
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .iter()
            .map(|p| self.file_id_for(p))
            .collect();
        file_ids.sort();
        let key = file_ids
            .iter()
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if let Some(id) = self.file_ids_list_key_to_id.get(&key) {
            return *id;
        }
        self.build_info.file_ids_list.push(file_ids);
        let id = BuildInfoFileIdListId(self.build_info.file_ids_list.len() as u32);
        self.file_ids_list_key_to_id.insert(key, id);
        id
    }

    // Go: snapshottobuildinfo.go:toBuildInfo.setFileInfoAndEmitSignatures
    // (emit signatures DEFER; assigns file ids in source-file order)
    fn set_file_info(&mut self) {
        for i in 0..self.program.source_files().len() {
            let path = self
                .program
                .to_path(self.program.source_files()[i].file_name());
            let info = self
                .snapshot
                .file_infos
                .get(&path)
                .cloned()
                .unwrap_or_default();
            self.file_id_for(&path);
            self.build_info
                .file_infos
                .push(BuildInfoFileInfo::from_file_info(&info));
        }
    }

    // Go: snapshottobuildinfo.go:toBuildInfo.setRootOfIncrementalProgram
    // (ResolvedRoot, i.e. root != resolved, is DEFER)
    fn set_root_of_incremental_program(&mut self) {
        // Roots: command-line file names that resolve to a source file, keyed by
        // their resolved source-file path, ordered by resolved file id.
        let mut resolved_ids: Vec<BuildInfoFileId> = Vec::new();
        for file_name in self.program.command_line().file_names() {
            if let Some(file) = self.program.get_source_file(file_name) {
                let path = self.program.to_path(file.file_name());
                resolved_ids.push(self.file_id_for(&path));
            }
        }
        resolved_ids.sort();
        resolved_ids.dedup();

        for resolved in resolved_ids {
            match self.build_info.root.last_mut() {
                None => self.build_info.root.push(BuildInfoRoot::single(resolved)),
                Some(last) => {
                    if (last.end.0 != 0 && last.end.0 == resolved.0 - 1)
                        || (last.end.0 == 0 && last.start.0 == resolved.0 - 1)
                    {
                        last.end = resolved;
                    } else {
                        self.build_info.root.push(BuildInfoRoot::single(resolved));
                    }
                }
            }
        }
    }

    // Go: snapshottobuildinfo.go:toBuildInfo.setCompilerOptions
    // DEFER(P6-9b): full `ForEachCompilerOptionValue` + `AffectsBuildInfo` set,
    // file-path relativization, and declaration order. Reachable subset emits
    // the `module` and `target` enum ints that appear in the captured buildinfo.
    fn set_compiler_options(&mut self) {
        let mut options = indexmap::IndexMap::new();
        if self.snapshot.options.module != ModuleKind::None {
            options.insert(
                "module".to_string(),
                serde_json::json!(self.snapshot.options.module as i32),
            );
        }
        if self.snapshot.options.target != ScriptTarget::None {
            options.insert(
                "target".to_string(),
                serde_json::json!(self.snapshot.options.target as i32),
            );
        }
        if !options.is_empty() {
            self.build_info.options = Some(options);
        }
    }

    // Go: snapshottobuildinfo.go:toBuildInfo.setReferencedMap
    fn set_referenced_map(&mut self) {
        let keys = self.snapshot.referenced_map.get_paths_with_references();
        for path in keys {
            let refs = self
                .snapshot
                .referenced_map
                .get_references(&path)
                .cloned()
                .unwrap_or_default();
            let file_id = self.file_id_for(&path);
            let file_id_list_id = self.file_id_list_id_for(&refs);
            self.build_info
                .referenced_map
                .push(BuildInfoReferenceMapEntry {
                    file_id,
                    file_id_list_id,
                });
        }
    }

    // Go: snapshottobuildinfo.go:toBuildInfo.setSemanticDiagnostics
    // Fresh build: every file has no cached per-file diagnostics and is not in
    // the change set, so it gets a bare-`fileId` entry. The cached-diagnostics
    // object form is DEFER (needs serialized `ast.Diagnostic`).
    fn set_semantic_diagnostics(&mut self) {
        for i in 0..self.program.source_files().len() {
            let path = self
                .program
                .to_path(self.program.source_files()[i].file_name());
            let file_id = self.file_id_for(&path);
            self.build_info
                .semantic_diagnostics_per_file
                .push(BuildInfoSemanticDiagnostic::file(file_id));
        }
    }
}
