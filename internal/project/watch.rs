//! Port of Go `internal/project/watch.go`.
//!
//! File-system watch infrastructure: a ref-counted [`WatchRegistry`] that tracks
//! active watcher subscriptions, the generic [`WatchedFiles`] type that computes
//! and caches file-system watcher glob patterns, and helper functions for
//! constructing recursive-directory glob patterns.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use tsgo_lsproto::{
    FileSystemWatcher, PatternOrRelativePattern, RelativePattern, WatchKind, WorkspaceFolderOrURI,
    URI,
};

/// Minimum depth for a watch location used in common-parent computation.
// Go: internal/project/watch.go:minWatchLocationDepth
pub const MIN_WATCH_LOCATION_DEPTH: usize = 2;

// ---------------------------------------------------------------------------
// WatcherID
// ---------------------------------------------------------------------------

/// Opaque identifier for a registered watcher.
// Go: internal/project/watch.go:WatcherID
pub type WatcherID = String;

/// Global monotonically-increasing watcher counter.
// Go: internal/project/watch.go:watcherID
static WATCHER_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_watcher_id() -> u64 {
    WATCHER_ID_COUNTER.fetch_add(1, Ordering::Relaxed) + 1
}

// ---------------------------------------------------------------------------
// WatchRegistry
// ---------------------------------------------------------------------------

/// Deduplication key for a file-system watcher (pattern + kind).
// Go: internal/project/watch.go:fileSystemWatcherKey
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileSystemWatcherKey {
    pattern: String,
    kind: WatchKind,
}

/// Ref-counted entry inside the registry.
// Go: internal/project/watch.go:fileSystemWatcherValue
#[derive(Debug)]
struct FileSystemWatcherValue {
    count: usize,
    id: WatcherID,
}

/// A ref-counted registry of active file-system watchers.
///
/// All methods are safe for concurrent use; locking is handled internally.
// Go: internal/project/watch.go:watchRegistry
pub struct WatchRegistry {
    entries: Mutex<HashMap<FileSystemWatcherKey, FileSystemWatcherValue>>,
    pending: Mutex<HashMap<WatcherID, ()>>,
}

impl Default for WatchRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchRegistry {
    // Go: internal/project/watch.go:newWatchRegistry
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Increments the ref count for a watcher. Returns `true` if this is the
    /// first reference (count transitions from 0 to 1), meaning the caller
    /// should register the watcher with the client.
    // Go: internal/project/watch.go:watchRegistry.Acquire
    pub fn acquire(&self, watcher: &FileSystemWatcher, id: WatcherID) -> bool {
        let key = to_file_system_watcher_key(watcher);
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .entry(key)
            .or_insert(FileSystemWatcherValue { count: 0, id });
        entry.count += 1;
        entry.count == 1
    }

    /// Decrements the ref count for a watcher. If no references remain, the
    /// entry is removed and `(id, true)` is returned so the caller can
    /// unregister from the client. Otherwise returns `("", false)`.
    // Go: internal/project/watch.go:watchRegistry.Release
    pub fn release(&self, watcher: &FileSystemWatcher) -> (WatcherID, bool) {
        let key = to_file_system_watcher_key(watcher);
        let mut entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get_mut(&key) else {
            return (String::new(), false);
        };
        if entry.count <= 1 {
            let id = entries.remove(&key).unwrap().id;
            return (id, true);
        }
        entry.count -= 1;
        (String::new(), false)
    }

    /// Records that a watcher's registration failed and needs retry.
    // Go: internal/project/watch.go:watchRegistry.MarkPending
    pub fn mark_pending(&self, id: WatcherID) {
        self.pending.lock().unwrap().insert(id, ());
    }

    /// Removes a watcher from the pending set after successful registration.
    // Go: internal/project/watch.go:watchRegistry.ClearPending
    pub fn clear_pending(&self, id: &WatcherID) {
        self.pending.lock().unwrap().remove(id);
    }

    /// Returns `true` if the watcher needs retry due to a previous failure.
    // Go: internal/project/watch.go:watchRegistry.IsPending
    pub fn is_pending(&self, id: &WatcherID) -> bool {
        self.pending.lock().unwrap().contains_key(id)
    }
}

// ---------------------------------------------------------------------------
// PatternsAndIgnored
// ---------------------------------------------------------------------------

/// Result of computing glob patterns: workspace-internal patterns,
/// directories outside the workspace, and an ignored set.
// Go: internal/project/watch.go:PatternsAndIgnored
#[derive(Debug, Clone, Default)]
pub struct PatternsAndIgnored {
    pub directories_outside_workspace: Vec<String>,
    pub patterns_inside_workspace: Vec<String>,
    pub ignored: HashMap<String, ()>,
}

// ---------------------------------------------------------------------------
// WatchedFiles
// ---------------------------------------------------------------------------

/// Holds the result of `WatchedFiles::watchers()`.
// Go: internal/project/watch.go:Watchers
#[derive(Debug, Clone)]
pub struct Watchers {
    pub watcher_id: WatcherID,
    pub workspace_watchers: Vec<FileSystemWatcher>,
    pub outside_workspace_watchers: Vec<FileSystemWatcher>,
    pub ignored_paths: HashMap<String, ()>,
}

/// Generic watcher container that lazily computes glob patterns from input
/// data of type `T`.
// Go: internal/project/watch.go:WatchedFiles
pub struct WatchedFiles<T: Send + Sync> {
    name: String,
    watch_kind: WatchKind,
    has_relative_pattern_capability: bool,
    compute_glob_patterns: Box<dyn Fn(&T) -> PatternsAndIgnored + Send + Sync>,
    inner: RwLock<WatchedFilesInner>,
    input: RwLock<Option<T>>,
    id: AtomicU64,
}

struct WatchedFilesInner {
    computed: bool,
    workspace_watchers: Vec<FileSystemWatcher>,
    outside_workspace_watchers: Vec<FileSystemWatcher>,
    ignored: HashMap<String, ()>,
}

impl<T: Send + Sync> WatchedFiles<T> {
    // Go: internal/project/watch.go:NewWatchedFiles
    pub fn new(
        name: impl Into<String>,
        watch_kind: WatchKind,
        has_relative_pattern_capability: bool,
        compute_glob_patterns: impl Fn(&T) -> PatternsAndIgnored + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            watch_kind,
            has_relative_pattern_capability,
            compute_glob_patterns: Box::new(compute_glob_patterns),
            inner: RwLock::new(WatchedFilesInner {
                computed: false,
                workspace_watchers: Vec::new(),
                outside_workspace_watchers: Vec::new(),
                ignored: HashMap::new(),
            }),
            input: RwLock::new(None),
            id: AtomicU64::new(next_watcher_id()),
        }
    }

    /// Sets the input data used to compute glob patterns.
    pub fn set_input(&self, input: T) {
        *self.input.write().unwrap() = Some(input);
    }

    /// Returns the computed watchers, lazily computing if not yet done.
    // Go: internal/project/watch.go:WatchedFiles.Watchers
    pub fn watchers(&self) -> Watchers {
        self.ensure_computed();
        let inner = self.inner.read().unwrap();
        Watchers {
            watcher_id: format!("{} watcher {}", self.name, self.id.load(Ordering::Relaxed)),
            workspace_watchers: inner.workspace_watchers.clone(),
            outside_workspace_watchers: inner.outside_workspace_watchers.clone(),
            ignored_paths: inner.ignored.clone(),
        }
    }

    /// Returns the watcher ID (computing watchers if needed).
    // Go: internal/project/watch.go:WatchedFiles.ID
    pub fn id(&self) -> WatcherID {
        self.watchers().watcher_id
    }

    /// Returns the watcher name.
    // Go: internal/project/watch.go:WatchedFiles.Name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the watch kind.
    // Go: internal/project/watch.go:WatchedFiles.WatchKind
    pub fn watch_kind(&self) -> WatchKind {
        self.watch_kind
    }

    fn ensure_computed(&self) {
        {
            let inner = self.inner.read().unwrap();
            if inner.computed {
                return;
            }
        }
        let mut inner = self.inner.write().unwrap();
        if inner.computed {
            return;
        }
        let input_guard = self.input.read().unwrap();
        let Some(input) = input_guard.as_ref() else {
            inner.computed = true;
            return;
        };
        let result = (self.compute_glob_patterns)(input);

        let mut globs: Vec<String> = result.patterns_inside_workspace;
        globs.sort();
        globs.dedup();

        let mut changed = false;
        if !watchers_equal_globs(&inner.workspace_watchers, &globs) {
            inner.workspace_watchers = globs
                .into_iter()
                .map(|glob| FileSystemWatcher {
                    glob_pattern: PatternOrRelativePattern {
                        pattern: Some(glob),
                        relative_pattern: None,
                    },
                    kind: Some(self.watch_kind),
                })
                .collect();
            changed = true;
        }

        let mut dirs_outside: Vec<String> = result.directories_outside_workspace;
        dirs_outside.sort();
        dirs_outside.dedup();

        if !watchers_equal_outside_dirs(
            &inner.outside_workspace_watchers,
            &dirs_outside,
            self.has_relative_pattern_capability,
        ) {
            inner.outside_workspace_watchers = dirs_outside
                .into_iter()
                .map(|dir| {
                    new_recursive_directory_watcher(
                        &dir,
                        self.watch_kind,
                        self.has_relative_pattern_capability,
                    )
                })
                .collect();
            changed = true;
        }

        inner.ignored = result.ignored;
        if changed {
            self.id.store(next_watcher_id(), Ordering::Relaxed);
        }
        inner.computed = true;
    }
}

fn watchers_equal_globs(watchers: &[FileSystemWatcher], globs: &[String]) -> bool {
    if watchers.len() != globs.len() {
        return false;
    }
    watchers
        .iter()
        .zip(globs.iter())
        .all(|(w, g)| w.glob_pattern.pattern.as_deref() == Some(g.as_str()))
}

fn watchers_equal_outside_dirs(
    watchers: &[FileSystemWatcher],
    dirs: &[String],
    use_relative_pattern: bool,
) -> bool {
    if watchers.len() != dirs.len() {
        return false;
    }
    watchers.iter().zip(dirs.iter()).all(|(w, d)| {
        file_system_watcher_glob_string(w)
            == recursive_directory_glob_pattern(d, use_relative_pattern)
    })
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Produces a deduplication key for a file-system watcher.
// Go: internal/project/watch.go:toFileSystemWatcherKey
fn to_file_system_watcher_key(w: &FileSystemWatcher) -> FileSystemWatcherKey {
    let kind = w
        .kind
        .unwrap_or(WatchKind::CREATE | WatchKind::CHANGE | WatchKind::DELETE);
    let pattern = if let Some(p) = &w.glob_pattern.pattern {
        p.clone()
    } else if let Some(rp) = &w.glob_pattern.relative_pattern {
        let base = if let Some(uri) = &rp.base_uri.uri {
            uri.0.clone()
        } else {
            panic!("workspace folder-based relative patterns not implemented")
        };
        format!("{}/{}", base, rp.pattern)
    } else {
        String::new()
    };
    FileSystemWatcherKey { pattern, kind }
}

/// Returns the string form of a file-system watcher's glob pattern.
// Go: internal/project/watch.go:fileSystemWatcherGlobString
pub fn file_system_watcher_glob_string(w: &FileSystemWatcher) -> String {
    if let Some(p) = &w.glob_pattern.pattern {
        return p.clone();
    }
    if let Some(rp) = &w.glob_pattern.relative_pattern {
        let base = if let Some(uri) = &rp.base_uri.uri {
            uri.0.clone()
        } else {
            panic!("workspace folder-based relative patterns not implemented")
        };
        return format!("{}/{}", base, rp.pattern);
    }
    String::new()
}

/// Produces the recursive glob pattern for a given directory.
// Go: internal/project/watch.go:getRecursiveGlobPattern
pub fn get_recursive_glob_pattern(directory: &str) -> String {
    let dir = tsgo_tspath::remove_trailing_directory_separator(directory);
    format!("{dir}/**/*")
}

/// Returns the string form of a recursive watcher for the given directory.
// Go: internal/project/watch.go:recursiveDirectoryGlobPattern
pub fn recursive_directory_glob_pattern(directory: &str, use_relative_pattern: bool) -> String {
    if use_relative_pattern {
        let uri = tsgo_ls_lsconv::file_name_to_document_uri(directory);
        format!("{}/**/*", uri.0)
    } else {
        get_recursive_glob_pattern(directory)
    }
}

/// Creates a `FileSystemWatcher` for recursively watching a directory.
// Go: internal/project/watch.go:newRecursiveDirectoryWatcher
pub fn new_recursive_directory_watcher(
    directory: &str,
    kind: WatchKind,
    use_relative_pattern: bool,
) -> FileSystemWatcher {
    if use_relative_pattern {
        let doc_uri = tsgo_ls_lsconv::file_name_to_document_uri(directory);
        let base_uri = URI(doc_uri.0);
        FileSystemWatcher {
            glob_pattern: PatternOrRelativePattern {
                pattern: None,
                relative_pattern: Some(RelativePattern {
                    base_uri: WorkspaceFolderOrURI {
                        workspace_folder: None,
                        uri: Some(base_uri),
                    },
                    pattern: "**/*".to_string(),
                }),
            },
            kind: Some(kind),
        }
    } else {
        let glob = get_recursive_glob_pattern(directory);
        FileSystemWatcher {
            glob_pattern: PatternOrRelativePattern {
                pattern: Some(glob),
                relative_pattern: None,
            },
            kind: Some(kind),
        }
    }
}

/// Returns path components suitable for watch-location grouping.
// Go: internal/project/watch.go:perceivedOsRootLengthForWatching
pub fn perceived_os_root_length_for_watching(path_components: &[&str]) -> usize {
    let length = path_components.len();
    if length <= 1 {
        return length;
    }
    if path_components[0].starts_with("//") {
        return 2;
    }
    let first = path_components[0].as_bytes();
    if first.len() == 3
        && tsgo_tspath::is_volume_character(first[0])
        && first[1] == b':'
        && first[2] == b'/'
    {
        if path_components[1].eq_ignore_ascii_case("users") {
            return std::cmp::min(3, length);
        }
        return 1;
    }
    if path_components[1] == "home" {
        return std::cmp::min(3, length);
    }
    1
}

#[cfg(test)]
#[path = "watch_test.rs"]
mod tests;
