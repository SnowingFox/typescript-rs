//! Embedded `bundled:///` virtual file system.
//!
//! 1:1 port of Go `internal/bundled/embed.go` (the default `!noembed` build).
//! Paths under the `bundled:///` scheme are served from the lib set compiled
//! into the binary; all other paths delegate to an inner [`Fs`].
//!
//! DIVERGENCE(port): Go implements `wrappedFS` directly against `vfs.FS` rather
//! than `io/fs.FS` to keep contents as strings. The Rust [`Fs`] trait is also
//! string-based, so the same direct approach is used here.

use std::sync::LazyLock;
use std::time::SystemTime;

use tsgo_vfs::{Entries, FileInfo, FileMode, Fs, FsResult, WalkControl, WalkDirFunc};

use crate::embed_generated::{EMBEDDED_CONTENTS, EMBEDDED_FILES};
use crate::LIB_NAMES;

const SCHEME: &str = "bundled:///";

// Splits the `bundled:///` scheme off `path`, yielding the remainder when present.
// Go: internal/bundled/embed.go:splitPath
fn split_path(path: &str) -> Option<&str> {
    path.strip_prefix(SCHEME)
}

/// Returns the virtual path of the directory holding the bundled lib files.
///
/// # Examples
/// ```
/// assert_eq!(tsgo_bundled::lib_path(), "bundled:///libs");
/// ```
///
/// Side effects: none (pure).
// Go: internal/bundled/embed.go:libPath
pub fn lib_path() -> String {
    format!("{SCHEME}libs")
}

/// Reports whether `path` addresses the embedded bundled file system.
///
/// # Examples
/// ```
/// assert!(tsgo_bundled::is_bundled("bundled:///libs/lib.d.ts"));
/// assert!(!tsgo_bundled::is_bundled("/usr/lib/lib.d.ts"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/bundled/embed.go:IsBundled
pub fn is_bundled(path: &str) -> bool {
    split_path(path).is_some()
}

// Children of the embedded `libs` directory, in [`LIB_NAMES`] order.
// Go: internal/bundled/embed.go:libsEntries
static LIBS_ENTRIES: LazyLock<Vec<FileInfo>> = LazyLock::new(|| {
    EMBEDDED_FILES
        .iter()
        .map(|(key, contents)| {
            let name = key.strip_prefix("libs/").unwrap_or(key);
            FileInfo::new(
                name.to_string(),
                contents.len() as i64,
                FileMode::REGULAR,
                SystemTime::UNIX_EPOCH,
            )
        })
        .collect()
});

// The single entry (`libs`) directly under the embedded root.
// Go: internal/bundled/embed.go:rootEntries
static ROOT_ENTRIES: LazyLock<Vec<FileInfo>> = LazyLock::new(|| {
    vec![FileInfo::new(
        "libs".to_string(),
        0,
        FileMode::DIR,
        SystemTime::UNIX_EPOCH,
    )]
});

/// A [`Fs`] wrapper that serves `bundled:///` paths from the embedded lib set
/// and forwards every other path to the wrapped inner file system.
///
/// # Examples
/// ```
/// use tsgo_vfs::Fs;
/// let fs = tsgo_bundled::wrap_fs(tsgo_vfs::osvfs::fs());
/// assert!(fs.file_exists("bundled:///libs/lib.d.ts"));
/// ```
///
/// Side effects: methods on embedded paths read in-memory data; methods on other
/// paths defer to the inner file system's side effects.
// Go: internal/bundled/embed.go:wrappedFS
pub struct WrappedFs<F: Fs> {
    inner: F,
}

/// Wraps `fs` so that embedded `bundled:///` paths resolve to the in-binary lib
/// files while all other paths pass through to `fs`.
///
/// # Examples
/// ```
/// use tsgo_vfs::Fs;
/// let fs = tsgo_bundled::wrap_fs(tsgo_vfs::osvfs::fs());
/// assert!(fs.read_file("bundled:///libs/lib.d.ts").is_some());
/// ```
///
/// Side effects: none at construction.
// Go: internal/bundled/bundled.go:WrapFS / embed.go:wrapFS
pub fn wrap_fs<F: Fs>(fs: F) -> WrappedFs<F> {
    WrappedFs { inner: fs }
}

// Walks the embedded tree rooted at `rest`, returning the most significant
// control flow seen (so `SkipAll` can propagate up to abort the whole walk).
// Go: internal/bundled/embed.go:walkDir
fn walk_embedded(rest: &str, walk_fn: &mut WalkDirFunc) -> FsResult<WalkControl> {
    let entries: &[FileInfo] = match rest {
        "" => ROOT_ENTRIES.as_slice(),
        "libs" => LIBS_ENTRIES.as_slice(),
        _ => return Ok(WalkControl::Continue),
    };

    for entry in entries {
        let name = format!("{rest}/{}", entry.name());
        match walk_fn(&format!("{SCHEME}{name}"), entry)? {
            WalkControl::Continue => {}
            WalkControl::SkipAll => return Ok(WalkControl::SkipAll),
            WalkControl::SkipDir => continue,
        }
        if entry.is_dir() {
            let child = name.strip_prefix('/').unwrap_or(&name);
            if walk_embedded(child, walk_fn)? == WalkControl::SkipAll {
                return Ok(WalkControl::SkipAll);
            }
        }
    }

    Ok(WalkControl::Continue)
}

impl<F: Fs> Fs for WrappedFs<F> {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn file_exists(&self, path: &str) -> bool {
        if let Some(rest) = split_path(path) {
            return EMBEDDED_CONTENTS.contains_key(rest);
        }
        self.inner.file_exists(path)
    }

    fn read_file(&self, path: &str) -> Option<String> {
        if let Some(rest) = split_path(path) {
            return EMBEDDED_CONTENTS
                .get(rest)
                .map(|contents| contents.to_string());
        }
        self.inner.read_file(path)
    }

    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        if split_path(path).is_some() {
            panic!("cannot write to embedded file system");
        }
        self.inner.write_file(path, data)
    }

    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        if split_path(path).is_some() {
            panic!("cannot write to embedded file system");
        }
        self.inner.append_file(path, data)
    }

    fn remove(&self, path: &str) -> FsResult<()> {
        if split_path(path).is_some() {
            panic!("cannot remove from embedded file system");
        }
        self.inner.remove(path)
    }

    fn chtimes(&self, path: &str, atime: SystemTime, mtime: SystemTime) -> FsResult<()> {
        if split_path(path).is_some() {
            panic!("cannot change times on embedded file system");
        }
        self.inner.chtimes(path, atime, mtime)
    }

    fn directory_exists(&self, path: &str) -> bool {
        if let Some(rest) = split_path(path) {
            return rest == "libs";
        }
        self.inner.directory_exists(path)
    }

    fn get_accessible_entries(&self, path: &str) -> Entries {
        if let Some(rest) = split_path(path) {
            let mut result = Entries::default();
            if rest.is_empty() {
                result.directories = vec!["libs".to_string()];
            } else if rest == "libs" {
                result.files = LIB_NAMES.iter().map(|name| name.to_string()).collect();
            }
            return result;
        }
        self.inner.get_accessible_entries(path)
    }

    fn stat(&self, path: &str) -> Option<FileInfo> {
        if let Some(rest) = split_path(path) {
            if rest.is_empty() || rest == "libs" {
                return Some(FileInfo::new(
                    rest.to_string(),
                    0,
                    FileMode::DIR,
                    SystemTime::UNIX_EPOCH,
                ));
            }
            if let Some(lib) = EMBEDDED_CONTENTS.get(rest) {
                let lib_name = rest.strip_prefix("libs/").unwrap_or(rest);
                return Some(FileInfo::new(
                    lib_name.to_string(),
                    lib.len() as i64,
                    FileMode::REGULAR,
                    SystemTime::UNIX_EPOCH,
                ));
            }
            return None;
        }
        self.inner.stat(path)
    }

    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        if let Some(rest) = split_path(root) {
            // `SkipAll` from the visitor is mapped to a normal completion.
            walk_embedded(rest, walk_fn)?;
            return Ok(());
        }
        self.inner.walk_dir(root, walk_fn)
    }

    fn realpath(&self, path: &str) -> String {
        if split_path(path).is_some() {
            return path.to_string();
        }
        self.inner.realpath(path)
    }
}

#[cfg(test)]
#[path = "embed_test.rs"]
mod tests;
