//! `tsgo_vfs` — file-system abstraction (`Fs` trait) plus several
//! implementations: real disk ([`osvfs`]), an in-memory test FS ([`vfstest`]),
//! and wrappers for caching ([`cachedvfs`]), dependency tracking
//! ([`trackingvfs`]), and method replacement ([`wrapvfs`]). It also provides
//! tsconfig `include`/`exclude` glob matching ([`vfsmatch`]) and polling file
//! watching ([`vfswatch`]).
//!
//! 1:1 port of Go `internal/vfs`.
//!
//! DIVERGENCE(port): Go layers everything on `io/fs.FS` (`os.DirFS`,
//! `fstest.MapFS`, `iovfs.From`). Rust's standard library has no `io/fs.FS`
//! abstraction, so the [`Fs`] trait declares all methods directly and the
//! `iovfs`/`internal`/`vfstest` in-memory paths are merged into one
//! self-written in-memory FS. See `docs/rust-rewrite/phase-1-foundation/vfs`.

use std::collections::HashSet;
use std::fmt;
use std::time::SystemTime;

pub mod internal;
pub mod vfstest;
// Subsequent submodules are declared as they are implemented (see TDD order in
// docs/rust-rewrite/phase-1-foundation/vfs/impl.md).
pub mod cachedvfs;
pub mod iovfs;
pub mod osvfs;
pub mod trackingvfs;
pub mod vfsmatch;
pub mod vfsmock;
pub mod vfswatch;
pub mod wrapvfs;

/// The kind of a filesystem entry: regular file, directory, or symbolic link.
///
/// DIVERGENCE(port): a tiny subset of Go's `fs.FileMode`; only the bits the
/// compiler's VFS layer inspects (`IsDir`/`IsRegular`/symlink) are modeled.
///
/// # Examples
/// ```
/// use tsgo_vfs::FileMode;
/// assert!(FileMode::DIR.is_dir());
/// assert!(FileMode::REGULAR.is_regular());
/// assert!(FileMode::SYMLINK.is_symlink());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfs.go:FileInfo (fs.FileMode subset)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileMode(u32);

impl FileMode {
    const DIR_BIT: u32 = 1 << 0;
    const SYMLINK_BIT: u32 = 1 << 1;

    /// A regular file.
    pub const REGULAR: FileMode = FileMode(0);
    /// A directory.
    pub const DIR: FileMode = FileMode(Self::DIR_BIT);
    /// A symbolic link.
    pub const SYMLINK: FileMode = FileMode(Self::SYMLINK_BIT);

    /// Reports whether the entry is a directory.
    ///
    /// Side effects: none (pure).
    pub fn is_dir(&self) -> bool {
        self.0 & Self::DIR_BIT != 0
    }

    /// Reports whether the entry is a symbolic link.
    ///
    /// Side effects: none (pure).
    pub fn is_symlink(&self) -> bool {
        self.0 & Self::SYMLINK_BIT != 0
    }

    /// Reports whether the entry is a regular file (neither directory nor
    /// symlink).
    ///
    /// Side effects: none (pure).
    pub fn is_regular(&self) -> bool {
        self.0 == 0
    }
}

/// Metadata about a filesystem entry, returned by [`Fs::stat`].
///
/// DIVERGENCE(port): replaces Go's `fs.FileInfo` interface with a concrete
/// owned struct holding only the fields the VFS layer needs.
///
/// # Examples
/// ```
/// use tsgo_vfs::{FileInfo, FileMode};
/// let fi = FileInfo::new("foo.ts".into(), 12, FileMode::REGULAR, std::time::SystemTime::UNIX_EPOCH);
/// assert_eq!(fi.name(), "foo.ts");
/// assert_eq!(fi.size(), 12);
/// assert!(!fi.is_dir());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfs.go:FileInfo
#[derive(Clone, Debug)]
pub struct FileInfo {
    name: String,
    size: i64,
    mode: FileMode,
    mod_time: SystemTime,
}

impl FileInfo {
    /// Constructs file metadata from its parts.
    ///
    /// Side effects: none (pure).
    pub fn new(name: String, size: i64, mode: FileMode, mod_time: SystemTime) -> Self {
        FileInfo {
            name,
            size,
            mode,
            mod_time,
        }
    }

    /// The base name of the entry.
    ///
    /// Side effects: none (pure).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The size of the entry in bytes (0 for directories).
    ///
    /// Side effects: none (pure).
    pub fn size(&self) -> i64 {
        self.size
    }

    /// The entry's mode (file/dir/symlink).
    ///
    /// Side effects: none (pure).
    pub fn mode(&self) -> FileMode {
        self.mode
    }

    /// Reports whether the entry is a directory.
    ///
    /// Side effects: none (pure).
    pub fn is_dir(&self) -> bool {
        self.mode.is_dir()
    }

    /// The last modification time.
    ///
    /// Side effects: none (pure).
    pub fn mod_time(&self) -> SystemTime {
        self.mod_time
    }
}

/// The files and directories directly contained in a directory, as returned by
/// [`Fs::get_accessible_entries`].
///
/// `symlinks` holds the names (present in `files` or `directories`) that were
/// originally symbolic links on disk; `None` means symlink information is not
/// available.
///
/// # Examples
/// ```
/// use tsgo_vfs::Entries;
/// let e = Entries::default();
/// assert!(e.files.is_empty());
/// assert!(e.symlinks.is_none());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfs.go:Entries
#[derive(Clone, Debug, Default)]
pub struct Entries {
    /// Base names of the regular files in the directory.
    pub files: Vec<String>,
    /// Base names of the subdirectories in the directory.
    pub directories: Vec<String>,
    /// Names (subset of `files`/`directories`) that were symbolic links.
    pub symlinks: Option<HashSet<String>>,
}

/// An error produced by a fallible [`Fs`] operation.
///
/// DIVERGENCE(port): replaces Go's `error` plus the sentinel `fs.ErrNotExist`
/// and the package-private `brokenSymlinkError`. The `Display` text matches the
/// Go messages that tests assert on.
///
/// # Examples
/// ```
/// use tsgo_vfs::FsError;
/// assert_eq!(FsError::NotExist.to_string(), "file does not exist");
/// assert!(FsError::NotExist.is_not_exist());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfs.go:ErrNotExist
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsError {
    /// The file or directory does not exist (mirrors `fs.ErrNotExist`).
    NotExist,
    /// A symlink could not be followed to an existing target.
    BrokenSymlink {
        /// The link path that could not be resolved.
        from: String,
        /// The (missing) target it pointed to.
        to: String,
    },
    /// Any other error, carrying its message.
    Other(String),
}

impl FsError {
    /// Reports whether this error means the path does not exist.
    ///
    /// Side effects: none (pure).
    pub fn is_not_exist(&self) -> bool {
        matches!(self, FsError::NotExist)
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::NotExist => write!(f, "file does not exist"),
            FsError::BrokenSymlink { from, to } => {
                write!(f, "broken symlink {from:?} -> {to:?}")
            }
            FsError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for FsError {}

/// Convenience alias for results of fallible [`Fs`] operations.
pub type FsResult<T> = Result<T, FsError>;

/// Controls a [`Fs::walk_dir`] traversal from the visitor callback.
///
/// Mirrors the meaning of `fs.SkipDir` / `fs.SkipAll` return values.
///
/// # Examples
/// ```
/// use tsgo_vfs::WalkControl;
/// let c = WalkControl::Continue;
/// assert_eq!(c, WalkControl::Continue);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfs.go:SkipAll/SkipDir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkControl {
    /// Keep walking.
    Continue,
    /// Skip the rest of the current directory (`fs.SkipDir`).
    SkipDir,
    /// Stop the entire walk (`fs.SkipAll`).
    SkipAll,
}

/// The visitor callback passed to [`Fs::walk_dir`].
///
/// It receives the entry's path and metadata and returns how the traversal
/// should proceed, or an error to abort with.
pub type WalkDirFunc<'a> = dyn FnMut(&str, &FileInfo) -> FsResult<WalkControl> + 'a;

/// A file system abstraction shared by the compiler and language service.
///
/// All paths are tspath-normalized, `/`-separated absolute paths. Methods take
/// `&self`; mutable implementations use interior mutability so a single `Fs`
/// can be shared across threads.
///
/// Side effects: implementations may perform real disk I/O.
// Go: internal/vfs/vfs.go:FS
pub trait Fs {
    /// Reports whether the file system distinguishes file names by case.
    ///
    /// Side effects: none.
    // Go: internal/vfs/vfs.go:FS.UseCaseSensitiveFileNames
    fn use_case_sensitive_file_names(&self) -> bool;

    /// Reports whether a regular file exists at `path`.
    ///
    /// Side effects: reads file metadata.
    // Go: internal/vfs/vfs.go:FS.FileExists
    fn file_exists(&self, path: &str) -> bool;

    /// Reads the file at `path`, returning its decoded contents, or `None` if it
    /// could not be read.
    ///
    /// Side effects: reads file contents.
    // Go: internal/vfs/vfs.go:FS.ReadFile
    fn read_file(&self, path: &str) -> Option<String>;

    /// Writes `data` to `path`, creating intermediate directories as needed.
    ///
    /// Side effects: writes a file (and possibly directories).
    // Go: internal/vfs/vfs.go:FS.WriteFile
    fn write_file(&self, path: &str, data: &str) -> FsResult<()>;

    /// Appends `data` to the file at `path`, creating it if absent.
    ///
    /// Side effects: writes a file (and possibly directories).
    // Go: internal/vfs/vfs.go:FS.AppendFile
    fn append_file(&self, path: &str, data: &str) -> FsResult<()>;

    /// Removes `path` and all of its contents.
    ///
    /// Side effects: deletes files/directories.
    // Go: internal/vfs/vfs.go:FS.Remove
    fn remove(&self, path: &str) -> FsResult<()>;

    /// Changes the access and modification times of `path`.
    ///
    /// Side effects: updates file timestamps.
    // Go: internal/vfs/vfs.go:FS.Chtimes
    fn chtimes(&self, path: &str, atime: SystemTime, mtime: SystemTime) -> FsResult<()>;

    /// Reports whether a directory exists at `path`.
    ///
    /// Side effects: reads file metadata.
    // Go: internal/vfs/vfs.go:FS.DirectoryExists
    fn directory_exists(&self, path: &str) -> bool;

    /// Returns the files and subdirectories directly contained in `path`,
    /// following symlinked entries.
    ///
    /// Side effects: reads directory entries.
    // Go: internal/vfs/vfs.go:FS.GetAccessibleEntries
    fn get_accessible_entries(&self, path: &str) -> Entries;

    /// Returns metadata for `path`, or `None` if it does not exist.
    ///
    /// Side effects: reads file metadata.
    // Go: internal/vfs/vfs.go:FS.Stat
    fn stat(&self, path: &str) -> Option<FileInfo>;

    /// Walks the tree rooted at `root`, invoking `walk_fn` for each entry in
    /// lexical order.
    ///
    /// Side effects: reads directory entries.
    // Go: internal/vfs/vfs.go:FS.WalkDir
    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()>;

    /// Returns the canonicalized real path of `path`, following symlinks and
    /// correcting casing; returns `path` unchanged if it cannot be resolved.
    ///
    /// Side effects: reads file metadata.
    // Go: internal/vfs/vfs.go:FS.Realpath
    fn realpath(&self, path: &str) -> String;
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
