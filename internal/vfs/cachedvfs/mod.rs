//! A caching [`Fs`] wrapper.
//!
//! 1:1 port of Go `internal/vfs/cachedvfs/cachedvfs.go`.
//!
//! Caches `directory_exists`, `file_exists`, `get_accessible_entries`,
//! `realpath`, and `stat`. Read/write/walk operations are always forwarded.
//!
//! DIVERGENCE(port): Go uses `collections.SyncMap` and `atomic.Bool`; here the
//! caches are `Mutex<HashMap>` and the toggle is an `AtomicBool`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::{Entries, FileInfo, Fs, FsResult, WalkDirFunc};

/// A caching wrapper over a shared inner [`Fs`].
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_vfs::Fs;
/// use tsgo_vfs::cachedvfs::CachedFs;
/// use tsgo_vfs::vfstest::MapFs;
/// let inner: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/a.ts", "x")], true));
/// let cached = CachedFs::from(inner);
/// assert!(cached.file_exists("/a.ts"));
/// ```
///
/// Side effects: maintains in-memory caches; forwards I/O to the inner FS.
// Go: internal/vfs/cachedvfs/cachedvfs.go:FS
pub struct CachedFs {
    fs: Arc<dyn Fs + Send + Sync>,
    enabled: AtomicBool,
    directory_exists_cache: Mutex<HashMap<String, bool>>,
    file_exists_cache: Mutex<HashMap<String, bool>>,
    get_accessible_entries_cache: Mutex<HashMap<String, Entries>>,
    realpath_cache: Mutex<HashMap<String, String>>,
    stat_cache: Mutex<HashMap<String, Option<FileInfo>>>,
}

impl CachedFs {
    /// Wraps `fs` with caching enabled.
    ///
    /// Side effects: none at construction.
    // Go: internal/vfs/cachedvfs/cachedvfs.go:From
    pub fn from(fs: Arc<dyn Fs + Send + Sync>) -> Self {
        CachedFs {
            fs,
            enabled: AtomicBool::new(true),
            directory_exists_cache: Mutex::new(HashMap::new()),
            file_exists_cache: Mutex::new(HashMap::new()),
            get_accessible_entries_cache: Mutex::new(HashMap::new()),
            realpath_cache: Mutex::new(HashMap::new()),
            stat_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Disables caching and clears all caches (only if it was enabled).
    ///
    /// Side effects: empties the caches.
    // Go: internal/vfs/cachedvfs/cachedvfs.go:DisableAndClearCache
    pub fn disable_and_clear_cache(&self) {
        if self
            .enabled
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.clear_cache();
        }
    }

    /// Re-enables caching.
    ///
    /// Side effects: none.
    // Go: internal/vfs/cachedvfs/cachedvfs.go:Enable
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Empties every cache.
    ///
    /// Side effects: empties the caches.
    // Go: internal/vfs/cachedvfs/cachedvfs.go:ClearCache
    pub fn clear_cache(&self) {
        self.directory_exists_cache.lock().unwrap().clear();
        self.file_exists_cache.lock().unwrap().clear();
        self.get_accessible_entries_cache.lock().unwrap().clear();
        self.realpath_cache.lock().unwrap().clear();
        self.stat_cache.lock().unwrap().clear();
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}

impl Fs for CachedFs {
    // Go: internal/vfs/cachedvfs/cachedvfs.go:UseCaseSensitiveFileNames
    fn use_case_sensitive_file_names(&self) -> bool {
        self.fs.use_case_sensitive_file_names()
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:FileExists
    fn file_exists(&self, path: &str) -> bool {
        if self.is_enabled() {
            if let Some(v) = self.file_exists_cache.lock().unwrap().get(path) {
                return *v;
            }
        }
        let ret = self.fs.file_exists(path);
        if self.is_enabled() {
            self.file_exists_cache
                .lock()
                .unwrap()
                .insert(path.to_string(), ret);
        }
        ret
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:ReadFile
    fn read_file(&self, path: &str) -> Option<String> {
        self.fs.read_file(path)
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:WriteFile
    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.fs.write_file(path, data)
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:AppendFile
    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.fs.append_file(path, data)
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:Remove
    fn remove(&self, path: &str) -> FsResult<()> {
        self.fs.remove(path)
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:Chtimes
    fn chtimes(&self, path: &str, atime: SystemTime, mtime: SystemTime) -> FsResult<()> {
        self.fs.chtimes(path, atime, mtime)
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:DirectoryExists
    fn directory_exists(&self, path: &str) -> bool {
        if self.is_enabled() {
            if let Some(v) = self.directory_exists_cache.lock().unwrap().get(path) {
                return *v;
            }
        }
        let ret = self.fs.directory_exists(path);
        if self.is_enabled() {
            self.directory_exists_cache
                .lock()
                .unwrap()
                .insert(path.to_string(), ret);
        }
        ret
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:GetAccessibleEntries
    fn get_accessible_entries(&self, path: &str) -> Entries {
        if self.is_enabled() {
            if let Some(v) = self.get_accessible_entries_cache.lock().unwrap().get(path) {
                return v.clone();
            }
        }
        let ret = self.fs.get_accessible_entries(path);
        if self.is_enabled() {
            self.get_accessible_entries_cache
                .lock()
                .unwrap()
                .insert(path.to_string(), ret.clone());
        }
        ret
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:Stat
    fn stat(&self, path: &str) -> Option<FileInfo> {
        if self.is_enabled() {
            if let Some(v) = self.stat_cache.lock().unwrap().get(path) {
                return v.clone();
            }
        }
        let ret = self.fs.stat(path);
        if self.is_enabled() {
            self.stat_cache
                .lock()
                .unwrap()
                .insert(path.to_string(), ret.clone());
        }
        ret
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:WalkDir
    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        self.fs.walk_dir(root, walk_fn)
    }

    // Go: internal/vfs/cachedvfs/cachedvfs.go:Realpath
    fn realpath(&self, path: &str) -> String {
        if self.is_enabled() {
            if let Some(v) = self.realpath_cache.lock().unwrap().get(path) {
                return v.clone();
            }
        }
        let ret = self.fs.realpath(path);
        if self.is_enabled() {
            self.realpath_cache
                .lock()
                .unwrap()
                .insert(path.to_string(), ret.clone());
        }
        ret
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
