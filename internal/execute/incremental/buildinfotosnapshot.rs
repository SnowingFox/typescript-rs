//! Rebuilds a [`Snapshot`] from a parsed [`BuildInfo`], to seed the next build's
//! reuse (the prior file versions, signatures, and import graph).
//!
//! Reachable subset of Go `internal/execute/incremental/buildinfotosnapshot.go`:
//! resolves each file name back to a canonical [`Path`] (relative names against
//! the buildinfo directory, bare `lib.*` names against the default-library
//! path), then restores `file_infos` and the `referencedMap`. Restoring the
//! compiler options from the serialized map, emit signatures, and the
//! diagnostics/change/pending-emit caches is DEFER.

use tsgo_collections::Set;
use tsgo_tspath::{combine_paths, get_directory_path, get_normalized_absolute_path, to_path, Path};

use crate::build_info::{BuildInfo, BuildInfoFileId, BuildInfoFileIdListId};
use crate::snapshot::Snapshot;

/// Rebuilds a [`Snapshot`] from `build_info`, resolving names against the
/// buildinfo directory (relative names) or `default_library_path` (bare lib
/// names), exactly as Go `buildInfoToSnapshot` does.
///
/// Side effects: none (pure transform).
// Go: internal/execute/incremental/buildinfotosnapshot.go:buildInfoToSnapshot
pub fn build_info_to_snapshot(
    build_info: &BuildInfo,
    build_info_file_name: &str,
    current_directory: &str,
    use_case_sensitive_file_names: bool,
    default_library_path: &str,
) -> Snapshot {
    let build_info_directory = get_directory_path(&get_normalized_absolute_path(
        build_info_file_name,
        current_directory,
    ));

    let file_paths: Vec<Path> = build_info
        .file_names
        .iter()
        .map(|file_name| {
            if !file_name.starts_with('.') {
                to_path(
                    &combine_paths(default_library_path, &[file_name]),
                    current_directory,
                    use_case_sensitive_file_names,
                )
            } else {
                to_path(
                    file_name,
                    &build_info_directory,
                    use_case_sensitive_file_names,
                )
            }
        })
        .collect();

    let to_file_path = |id: BuildInfoFileId| file_paths[(id.0 - 1) as usize].clone();
    let to_file_path_set = |id: BuildInfoFileIdListId| -> Set<Path> {
        Set::from_items(
            build_info.file_ids_list[(id.0 - 1) as usize]
                .iter()
                .map(|&fid| to_file_path(fid)),
        )
    };

    let mut snapshot = Snapshot::default();

    // setFileInfoAndEmitSignatures (file infos; emit signatures DEFER).
    for (index, build_info_file_info) in build_info.file_infos.iter().enumerate() {
        snapshot.file_infos.insert(
            file_paths[index].clone(),
            build_info_file_info.get_file_info(),
        );
    }

    // setReferencedMap.
    for entry in &build_info.referenced_map {
        snapshot.referenced_map.store_references(
            to_file_path(entry.file_id),
            to_file_path_set(entry.file_id_list_id),
        );
    }

    snapshot
}
