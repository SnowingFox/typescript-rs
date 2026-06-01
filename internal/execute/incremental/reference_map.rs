//! The `referencedMap`: for each file, the set of files it references (imports,
//! triple-slash refs, type refs, module augmentations, ambient modules), plus
//! the lazily-derived inverse (`referencedBy`).
//!
//! 1:1 port of the reachable subset of Go
//! `internal/execute/incremental/referencemap.go`.

use rustc_hash::FxHashMap;
use tsgo_collections::Set;
use tsgo_tspath::Path;

/// Maps each file path to the set of files it references, and answers the
/// inverse "who references this file?" query used by the affected-files walk.
// Go: internal/execute/incremental/referencemap.go:referenceMap
#[derive(Debug, Default)]
pub struct ReferenceMap {
    references: FxHashMap<Path, Set<Path>>,
}

impl ReferenceMap {
    /// An empty reference map.
    ///
    /// Side effects: none.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `path` references every file in `refs`.
    ///
    /// Side effects: stores into the map.
    // Go: internal/execute/incremental/referencemap.go:referenceMap.storeReferences
    pub fn store_references(&mut self, path: Path, refs: Set<Path>) {
        self.references.insert(path, refs);
    }

    /// The set of files `path` references, if recorded.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/incremental/referencemap.go:referenceMap.getReferences
    pub fn get_references(&self, path: &Path) -> Option<&Set<Path>> {
        self.references.get(path)
    }

    /// All paths that have recorded references, sorted for determinism.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/incremental/referencemap.go:referenceMap.getPathsWithReferences
    pub fn get_paths_with_references(&self) -> Vec<Path> {
        let mut keys: Vec<Path> = self.references.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// The files that reference `path` (the inverse of [`Self::store_references`]),
    /// sorted for determinism.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/incremental/referencemap.go:referenceMap.getReferencedBy
    pub fn get_referenced_by(&self, path: &Path) -> Vec<Path> {
        // Go builds the full inverse map lazily (sync.Once); for the reachable
        // single-threaded subset we scan on demand. Output is sorted so the
        // affected-files walk is deterministic regardless of map iteration order.
        // PERF(port): cache the inverse map if this becomes hot.
        let mut referrers: Vec<Path> = self
            .references
            .iter()
            .filter(|(_, refs)| refs.has(path))
            .map(|(key, _)| key.clone())
            .collect();
        referrers.sort();
        referrers
    }
}

#[cfg(test)]
#[path = "reference_map_test.rs"]
mod tests;
