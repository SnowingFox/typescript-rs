//! Affected-files recompute: given a changed file, the transitive set of files
//! whose semantic diagnostics / emit must be recomputed, walked over the
//! `referencedMap`.
//!
//! 1:1 port of the reachable subset of Go
//! `internal/execute/incremental/affectedfileshandler.go`.

use tsgo_collections::Set;
use tsgo_tspath::Path;

use crate::reference_map::ReferenceMap;

/// Computes the transitive set of files affected by a change to `changed`.
///
/// Starting from the changed file, this walks the inverse reference graph
/// (`referencedBy`): every file that (transitively) references the changed file
/// is affected, since its semantic diagnostics / emit may need to be recomputed.
/// The changed file itself is always included. The result is sorted for
/// determinism.
///
/// This is the reachable subset of Go's `getFilesAffectedBy` /
/// `forEachFileReferencedBy`: Go additionally gates propagation on per-file
/// shape-signature changes (`updateShapeSignature`) and global-scope effects;
/// here every transitive referrer is treated as affected (the case when the
/// changed file's `.d.ts` signature changed).
///
/// # Examples
/// ```
/// use tsgo_incremental::{get_files_affected_by, ReferenceMap};
/// use tsgo_collections::Set;
/// use tsgo_tspath::Path;
///
/// // a.ts imports b.ts.
/// let mut map = ReferenceMap::new();
/// map.store_references(Path("/a.ts".into()), Set::from_items([Path("/b.ts".into())]));
///
/// // Changing b.ts affects {b.ts, a.ts}.
/// assert_eq!(
///     get_files_affected_by(&map, &Path("/b.ts".into())),
///     vec![Path("/a.ts".into()), Path("/b.ts".into())]
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/execute/incremental/affectedfileshandler.go:getFilesAffectedBy
// DEFER(P6-9b): shape-signature gating (`updateShapeSignature`),
// `affectsGlobalScope` "all files" expansion, isolatedModules fast path.
// blocked-by: declaration-emit signatures + checker const-enum/alias surface.
pub fn get_files_affected_by(reference_map: &ReferenceMap, changed: &Path) -> Vec<Path> {
    // Mirror `forEachFileReferencedBy`: seed with the changed file, then walk
    // the inverse graph (referencedBy). `seen` doubles as the cycle guard.
    let mut seen: Set<Path> = Set::default();
    seen.add(changed.clone());
    let mut queue: Vec<Path> = reference_map.get_referenced_by(changed);
    while let Some(current) = queue.pop() {
        if seen.add_if_absent(current.clone()) {
            queue.extend(reference_map.get_referenced_by(&current));
        }
    }
    let mut affected: Vec<Path> = seen.keys().cloned().collect();
    affected.sort();
    affected
}

/// Like [`get_files_affected_by`] but for a whole set of changed files, taking
/// the union of each file's affected set (Go `collectAllAffectedFiles`).
///
/// Side effects: none (pure).
// Go: internal/execute/incremental/affectedfileshandler.go:collectAllAffectedFiles
pub fn collect_all_affected_files(reference_map: &ReferenceMap, changed: &[Path]) -> Vec<Path> {
    let mut result: Set<Path> = Set::default();
    for file in changed {
        for affected in get_files_affected_by(reference_map, file) {
            result.add(affected);
        }
    }
    let mut out: Vec<Path> = result.keys().cloned().collect();
    out.sort();
    out
}

#[cfg(test)]
#[path = "affectedfileshandler_test.rs"]
mod tests;
