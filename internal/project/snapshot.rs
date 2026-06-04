//! Snapshot — the immutable view of all projects at a point in time.
//!
//! 1:1 port of Go `internal/project/snapshot.go`.
//!
//! A [`Snapshot`] holds a [`ProjectCollection`], and provides accessors
//! for querying projects, files, and language-service delegation. Each
//! snapshot is identified by a monotonically increasing `id`.
//!
//! ## Simplifications (P8 MVP)
//! - No ref-counting / clone-on-write (deferred until full session lifecycle).
//! - No `SnapshotFS` layer; the session's `OverlayFS` is used directly.

use crate::project::Project;
use crate::projectcollection::ProjectCollection;
use tsgo_tspath::Path;

/// An immutable snapshot of all projects and their state.
///
/// # Examples
/// ```
/// use tsgo_project::snapshot::Snapshot;
/// use tsgo_project::projectcollection::ProjectCollection;
/// use tsgo_project::configfileregistry::ConfigFileRegistry;
///
/// let pc = ProjectCollection::new(
///     Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
///     ConfigFileRegistry::new(),
/// );
/// let snap = Snapshot::new(1, pc);
/// assert_eq!(snap.id(), 1);
/// ```
///
/// Side effects: none (pure data structure).
// Go: internal/project/snapshot.go:Snapshot
pub struct Snapshot {
    id: u64,
    project_collection: ProjectCollection,
    // DEFER(phase-8): SnapshotFS, converters, auto-imports, user preferences, etc.
}

impl Snapshot {
    /// Creates a new snapshot with the given id and project collection.
    ///
    /// # Examples
    /// ```
    /// use tsgo_project::snapshot::Snapshot;
    /// use tsgo_project::projectcollection::ProjectCollection;
    /// use tsgo_project::configfileregistry::ConfigFileRegistry;
    ///
    /// let pc = ProjectCollection::new(
    ///     Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
    ///     ConfigFileRegistry::new(),
    /// );
    /// let snap = Snapshot::new(0, pc);
    /// assert_eq!(snap.id(), 0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/project/snapshot.go:NewSnapshot
    pub fn new(id: u64, project_collection: ProjectCollection) -> Self {
        Self {
            id,
            project_collection,
        }
    }

    /// Returns the snapshot id.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/snapshot.go:Snapshot.ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns a reference to the project collection.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/snapshot.go:Snapshot.ProjectCollection
    pub fn project_collection(&self) -> &ProjectCollection {
        &self.project_collection
    }

    /// Returns a mutable reference to the project collection.
    ///
    /// Side effects: none (pure).
    pub fn project_collection_mut(&mut self) -> &mut ProjectCollection {
        &mut self.project_collection
    }

    /// Returns the default project for a given file path.
    ///
    /// Delegates to [`ProjectCollection::get_default_project`].
    ///
    /// # Examples
    /// ```
    /// use tsgo_project::snapshot::Snapshot;
    /// use tsgo_project::projectcollection::ProjectCollection;
    /// use tsgo_project::configfileregistry::ConfigFileRegistry;
    ///
    /// let pc = ProjectCollection::new(
    ///     Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
    ///     ConfigFileRegistry::new(),
    /// );
    /// let snap = Snapshot::new(1, pc);
    /// let path = tsgo_tspath::Path("/a.ts".to_string());
    /// assert!(snap.get_default_project(&path).is_none());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/project/snapshot.go:Snapshot.GetDefaultProject
    pub fn get_default_project(&self, path: &Path) -> Option<&Project> {
        self.project_collection.get_default_project(path)
    }
}

#[cfg(test)]
#[path = "snapshot_test.rs"]
mod tests;
