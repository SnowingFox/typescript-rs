//! A [`Fs`] wrapper that overrides selected methods with closures.
//!
//! 1:1 port of Go `internal/vfs/wrapvfs/wrapvfs.go`. Each method with a
//! configured replacement uses it; otherwise the call is delegated to the
//! wrapped FS.
//!
//! DIVERGENCE(port): Go's `Replacements` struct holds nilable function values;
//! here they are `Option<Box<dyn Fn ...>>`.

use std::time::SystemTime;

use crate::{Entries, FileInfo, Fs, FsResult, WalkControl, WalkDirFunc};

type BoolFn = Box<dyn Fn() -> bool + Send + Sync>;
type PathBoolFn = Box<dyn Fn(&str) -> bool + Send + Sync>;
type ReadFn = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;
type WriteFn = Box<dyn Fn(&str, &str) -> FsResult<()> + Send + Sync>;
type RemoveFn = Box<dyn Fn(&str) -> FsResult<()> + Send + Sync>;
type ChtimesFn = Box<dyn Fn(&str, SystemTime, SystemTime) -> FsResult<()> + Send + Sync>;
type EntriesFn = Box<dyn Fn(&str) -> Entries + Send + Sync>;
type StatFn = Box<dyn Fn(&str) -> Option<FileInfo> + Send + Sync>;
type RealpathFn = Box<dyn Fn(&str) -> String + Send + Sync>;
type WalkFn = Box<
    dyn for<'a> Fn(
            &str,
            &mut (dyn FnMut(&str, &FileInfo) -> FsResult<WalkControl> + 'a),
        ) -> FsResult<()>
        + Send
        + Sync,
>;

/// Optional per-method overrides for [`wrap`].
///
/// Every field defaults to `None` (delegate to the wrapped FS).
///
/// # Examples
/// ```
/// use tsgo_vfs::wrapvfs::Replacements;
/// let mut r = Replacements::default();
/// r.file_exists = Some(Box::new(|_p| true));
/// assert!(r.read_file.is_none());
/// ```
///
/// Side effects: none (holds closures).
// Go: internal/vfs/wrapvfs/wrapvfs.go:Replacements
#[derive(Default)]
pub struct Replacements {
    /// Override for `use_case_sensitive_file_names`.
    pub use_case_sensitive_file_names: Option<BoolFn>,
    /// Override for `file_exists`.
    pub file_exists: Option<PathBoolFn>,
    /// Override for `read_file`.
    pub read_file: Option<ReadFn>,
    /// Override for `write_file`.
    pub write_file: Option<WriteFn>,
    /// Override for `append_file`.
    pub append_file: Option<WriteFn>,
    /// Override for `remove`.
    pub remove: Option<RemoveFn>,
    /// Override for `chtimes`.
    pub chtimes: Option<ChtimesFn>,
    /// Override for `directory_exists`.
    pub directory_exists: Option<PathBoolFn>,
    /// Override for `get_accessible_entries`.
    pub get_accessible_entries: Option<EntriesFn>,
    /// Override for `stat`.
    pub stat: Option<StatFn>,
    /// Override for `walk_dir`.
    pub walk_dir: Option<WalkFn>,
    /// Override for `realpath`.
    pub realpath: Option<RealpathFn>,
}

/// Wraps `fs`, overriding methods that have a configured replacement.
///
/// # Examples
/// ```
/// use tsgo_vfs::Fs;
/// use tsgo_vfs::wrapvfs::{wrap, Replacements};
/// use tsgo_vfs::vfstest::MapFs;
/// let mut r = Replacements::default();
/// r.file_exists = Some(Box::new(|_p| true));
/// let fs = wrap(MapFs::from_map(Vec::<(&str, &str)>::new(), true), r);
/// assert!(fs.file_exists("/anything"));
/// ```
///
/// Side effects: none at construction.
// Go: internal/vfs/wrapvfs/wrapvfs.go:Wrap
pub fn wrap<F: Fs + Send + Sync + 'static>(fs: F, replacements: Replacements) -> WrappedFs {
    WrappedFs {
        fs: Box::new(fs),
        replacements,
    }
}

/// The result of [`wrap`].
///
/// Side effects: none (delegates to the wrapped FS or a replacement).
// Go: internal/vfs/wrapvfs/wrapvfs.go:wrappedFS
pub struct WrappedFs {
    fs: Box<dyn Fs + Send + Sync>,
    replacements: Replacements,
}

impl Fs for WrappedFs {
    // Go: internal/vfs/wrapvfs/wrapvfs.go:UseCaseSensitiveFileNames
    fn use_case_sensitive_file_names(&self) -> bool {
        match &self.replacements.use_case_sensitive_file_names {
            Some(f) => f(),
            None => self.fs.use_case_sensitive_file_names(),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:FileExists
    fn file_exists(&self, path: &str) -> bool {
        match &self.replacements.file_exists {
            Some(f) => f(path),
            None => self.fs.file_exists(path),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:ReadFile
    fn read_file(&self, path: &str) -> Option<String> {
        match &self.replacements.read_file {
            Some(f) => f(path),
            None => self.fs.read_file(path),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:WriteFile
    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        match &self.replacements.write_file {
            Some(f) => f(path, data),
            None => self.fs.write_file(path, data),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:AppendFile
    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        match &self.replacements.append_file {
            Some(f) => f(path, data),
            None => self.fs.append_file(path, data),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:Remove
    fn remove(&self, path: &str) -> FsResult<()> {
        match &self.replacements.remove {
            Some(f) => f(path),
            None => self.fs.remove(path),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:Chtimes
    fn chtimes(&self, path: &str, atime: SystemTime, mtime: SystemTime) -> FsResult<()> {
        match &self.replacements.chtimes {
            Some(f) => f(path, atime, mtime),
            None => self.fs.chtimes(path, atime, mtime),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:DirectoryExists
    fn directory_exists(&self, path: &str) -> bool {
        match &self.replacements.directory_exists {
            Some(f) => f(path),
            None => self.fs.directory_exists(path),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:GetAccessibleEntries
    fn get_accessible_entries(&self, path: &str) -> Entries {
        match &self.replacements.get_accessible_entries {
            Some(f) => f(path),
            None => self.fs.get_accessible_entries(path),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:Stat
    fn stat(&self, path: &str) -> Option<FileInfo> {
        match &self.replacements.stat {
            Some(f) => f(path),
            None => self.fs.stat(path),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:WalkDir
    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        match &self.replacements.walk_dir {
            Some(f) => f(root, walk_fn),
            None => self.fs.walk_dir(root, walk_fn),
        }
    }

    // Go: internal/vfs/wrapvfs/wrapvfs.go:Realpath
    fn realpath(&self, path: &str) -> String {
        match &self.replacements.realpath {
            Some(f) => f(path),
            None => self.fs.realpath(path),
        }
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
