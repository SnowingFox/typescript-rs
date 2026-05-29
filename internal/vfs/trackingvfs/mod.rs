//! A [`Fs`] wrapper that records every read-like access path.
//!
//! 1:1 port of Go `internal/vfs/trackingvfs/trackingvfs.go`. Watch mode uses the
//! recorded set to know which files/directories the compiler depended on,
//! including non-existent paths from failed module resolution. Write operations
//! are not tracked since they are outputs, not dependencies.
//!
//! DIVERGENCE(port): Go's exported `SeenFiles collections.SyncSet[string]` is a
//! `Mutex<HashSet<String>>` here, queried via [`TrackingFs::seen`].

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::SystemTime;

use crate::{Entries, FileInfo, Fs, FsResult, WalkDirFunc};

/// Wraps an inner [`Fs`], recording the path of every read-like operation.
///
/// # Examples
/// ```
/// use tsgo_vfs::Fs;
/// use tsgo_vfs::trackingvfs::TrackingFs;
/// use tsgo_vfs::vfstest::MapFs;
/// let fs = TrackingFs::new(MapFs::from_map([("/a.ts", "x")], true));
/// fs.file_exists("/a.ts");
/// assert!(fs.seen("/a.ts"));
/// ```
///
/// Side effects: records accessed paths; forwards I/O to the inner FS.
// Go: internal/vfs/trackingvfs/trackingvfs.go:FS
pub struct TrackingFs {
    inner: Box<dyn Fs + Send + Sync>,
    seen_files: Mutex<HashSet<String>>,
}

impl TrackingFs {
    /// Wraps `inner` with access tracking.
    ///
    /// Side effects: none at construction.
    pub fn new<F: Fs + Send + Sync + 'static>(inner: F) -> Self {
        TrackingFs {
            inner: Box::new(inner),
            seen_files: Mutex::new(HashSet::new()),
        }
    }

    /// Reports whether `path` was recorded by a read-like operation.
    ///
    /// Side effects: none.
    pub fn seen(&self, path: &str) -> bool {
        self.seen_files.lock().unwrap().contains(path)
    }

    fn record(&self, path: &str) {
        self.seen_files.lock().unwrap().insert(path.to_string());
    }
}

impl Fs for TrackingFs {
    // Go: internal/vfs/trackingvfs/trackingvfs.go:UseCaseSensitiveFileNames
    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:FileExists
    fn file_exists(&self, path: &str) -> bool {
        self.record(path);
        self.inner.file_exists(path)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:ReadFile
    fn read_file(&self, path: &str) -> Option<String> {
        self.record(path);
        self.inner.read_file(path)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:WriteFile
    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.inner.write_file(path, data)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:AppendFile
    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.inner.append_file(path, data)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:Remove
    fn remove(&self, path: &str) -> FsResult<()> {
        self.inner.remove(path)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:Chtimes
    fn chtimes(&self, path: &str, atime: SystemTime, mtime: SystemTime) -> FsResult<()> {
        self.inner.chtimes(path, atime, mtime)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:DirectoryExists
    fn directory_exists(&self, path: &str) -> bool {
        self.record(path);
        self.inner.directory_exists(path)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:GetAccessibleEntries
    fn get_accessible_entries(&self, path: &str) -> Entries {
        self.record(path);
        self.inner.get_accessible_entries(path)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:Stat
    fn stat(&self, path: &str) -> Option<FileInfo> {
        self.record(path);
        self.inner.stat(path)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:WalkDir
    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        self.record(root);
        let mut wrapped = |path: &str, info: &FileInfo| {
            self.record(path);
            walk_fn(path, info)
        };
        self.inner.walk_dir(root, &mut wrapped)
    }

    // Go: internal/vfs/trackingvfs/trackingvfs.go:Realpath
    fn realpath(&self, path: &str) -> String {
        self.inner.realpath(path)
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
