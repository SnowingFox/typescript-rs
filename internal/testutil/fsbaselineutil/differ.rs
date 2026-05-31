use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use regex::Regex;
use tsgo_collections::SyncSet;
use tsgo_vfs::FileMode;

/// A single captured file-system entry in a snapshot.
///
/// Mirrors Go's `DiffEntry`. `mtime` is `None` for entries (such as symlinks)
/// that have no meaningful modification time, modeling Go's zero `time.Time`.
///
/// # Examples
/// ```
/// use tsgo_testutil_fsbaselineutil::DiffEntry;
/// let e = DiffEntry { content: "x".into(), ..Default::default() };
/// assert_eq!(e.content, "x");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/testutil/fsbaselineutil/differ.go:DiffEntry
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffEntry {
    /// The (symbol-sanitized) file contents for regular files.
    pub content: String,
    /// The modification time, or `None` for entries without one.
    pub mtime: Option<SystemTime>,
    /// Whether the file was (re)written since the last baseline.
    pub is_written: bool,
    /// The link target for symbolic links, otherwise empty.
    pub symlink_target: String,
}

/// A captured snapshot of an in-memory file system.
///
/// Mirrors Go's `Snapshot`.
///
/// # Examples
/// ```
/// use tsgo_testutil_fsbaselineutil::Snapshot;
/// let s = Snapshot::default();
/// assert!(s.snap.is_empty());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/testutil/fsbaselineutil/differ.go:Snapshot
#[derive(Default)]
pub struct Snapshot {
    /// Map of absolute path to its captured entry.
    pub snap: HashMap<String, DiffEntry>,
    /// The set of default-library paths at capture time.
    pub default_libs: SyncSet<String>,
}

/// One raw file-system entry as observed from the underlying map FS.
///
/// DIVERGENCE(port): in Go this is `*fstest.MapFile` plus the path yielded by
/// `MapFS.Entries()`. Rust models just the fields the differ reads.
///
/// # Examples
/// ```
/// use tsgo_testutil_fsbaselineutil::MapEntry;
/// use tsgo_vfs::FileMode;
/// let e = MapEntry { path: "/a.ts".into(), mode: FileMode::REGULAR, data: b"x".to_vec(), mod_time: std::time::SystemTime::UNIX_EPOCH };
/// assert!(e.mode.is_regular());
/// ```
///
/// Side effects: none (pure value type).
pub struct MapEntry {
    /// The absolute path of the entry.
    pub path: String,
    /// The entry's file mode (regular / symlink / dir).
    pub mode: FileMode,
    /// The raw byte contents (or, for symlinks, the link target).
    pub data: Vec<u8>,
    /// The entry's modification time.
    pub mod_time: SystemTime,
}

/// The view of an in-memory map file system that [`FsDiffer`] needs.
///
/// DIVERGENCE(port): Go's `FSDiffer` holds an `iovfs.FsWithSys` and downcasts
/// it to `*vfstest.MapFS`, calling `Entries()`, `GetTargetOfSymlink()`, and
/// `GetFileInfo()`. The Rust `tsgo_vfs::vfstest::MapFs` does not (yet) expose
/// these, and editing `tsgo_vfs` is out of scope for this lane, so the surface
/// the differ needs is captured by this trait. Wiring a real `tsgo_vfs::MapFs`
/// to this trait is deferred until that crate exposes the equivalent methods.
// Go: internal/testutil/fsbaselineutil/differ.go:FSDiffer.MapFs (used surface)
pub trait MapFsView {
    /// All entries, ordered by path the way Go's `MapFS.Entries()` yields them
    /// (segment-wise, parents before children).
    ///
    /// Side effects: reads the in-memory file system.
    fn entries(&self) -> Vec<MapEntry>;

    /// The target of the symbolic link at `path`, or `None` if `path` is not a
    /// symlink. The returned target is absolute (leading `/`), matching Go.
    ///
    /// Side effects: reads the in-memory file system.
    fn get_target_of_symlink(&self, path: &str) -> Option<String>;

    /// Whether an entry exists at `path` (mirrors Go's `GetFileInfo(path) != nil`).
    ///
    /// Side effects: reads the in-memory file system.
    fn has_entry(&self, path: &str) -> bool;
}

/// A closure that returns the current set of default-library paths, or `None`.
///
/// Mirrors Go's `DefaultLibs func() *collections.SyncSet[string]`.
pub type DefaultLibsFn = Box<dyn Fn() -> Option<Arc<SyncSet<String>>>>;

/// Baselines an in-memory file system and diffs it against the previous
/// snapshot.
///
/// Mirrors Go's `FSDiffer`.
///
/// Side effects: see [`FsDiffer::baseline_fs_with_diff`].
// Go: internal/testutil/fsbaselineutil/differ.go:FSDiffer
pub struct FsDiffer<F: MapFsView> {
    /// The underlying map file system view.
    pub fs: F,
    /// Optional accessor for the current default-library set.
    pub default_libs: Option<DefaultLibsFn>,
    /// Paths written since the last baseline (reset after each baseline).
    pub written_files: SyncSet<String>,
    serialized_diff: Option<Snapshot>,
}

impl<F: MapFsView> FsDiffer<F> {
    /// Creates a differ over `fs` with no default-libs accessor and an empty
    /// written-files set.
    ///
    /// # Examples
    /// ```
    /// # use tsgo_testutil_fsbaselineutil::*;
    /// # use tsgo_vfs::FileMode;
    /// struct Empty;
    /// impl MapFsView for Empty {
    ///     fn entries(&self) -> Vec<MapEntry> { vec![] }
    ///     fn get_target_of_symlink(&self, _: &str) -> Option<String> { None }
    ///     fn has_entry(&self, _: &str) -> bool { false }
    /// }
    /// let d = FsDiffer::new(Empty);
    /// assert!(d.serialized_diff().is_none());
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn new(fs: F) -> Self {
        FsDiffer {
            fs,
            default_libs: None,
            written_files: SyncSet::default(),
            serialized_diff: None,
        }
    }

    /// Returns the most recently captured snapshot, if any.
    ///
    /// Side effects: none (pure).
    // Go: internal/testutil/fsbaselineutil/differ.go:FSDiffer.SerializedDiff
    pub fn serialized_diff(&self) -> Option<&Snapshot> {
        self.serialized_diff.as_ref()
    }

    /// Baselines the entire file system, writing a diff against the previous
    /// snapshot to `baseline`, then records the new snapshot and resets the
    /// written-files set.
    ///
    /// Side effects: writes the diff to `baseline`; mutates `self`
    /// (`serialized_diff` and `written_files`).
    // Go: internal/testutil/fsbaselineutil/differ.go:FSDiffer.BaselineFSwithDiff
    pub fn baseline_fs_with_diff(&mut self, baseline: &mut dyn Write) {
        // todo: baselines the entire fs, possibly doesn't correctly diff all
        // cases of emitted files (matching the Go caveat).
        let mut snap: HashMap<String, DiffEntry> = HashMap::new();
        let mut diffs: HashMap<String, String> = HashMap::new();

        for entry in self.fs.entries() {
            let path = entry.path;
            if entry.mode.is_symlink() {
                let target = self
                    .fs
                    .get_target_of_symlink(&path)
                    .unwrap_or_else(|| panic!("Failed to resolve symlink target: {path}"));
                let new_entry = DiffEntry {
                    symlink_target: target,
                    ..Default::default()
                };
                self.add_fs_entry_diff(&mut diffs, Some(&new_entry), &path);
                snap.insert(path, new_entry);
            } else if entry.mode.is_regular() {
                let content = sanitize_internal_symbol_name(&String::from_utf8_lossy(&entry.data));
                let new_entry = DiffEntry {
                    content,
                    mtime: Some(entry.mod_time),
                    is_written: self.written_files.has(&path),
                    symlink_target: String::new(),
                };
                self.add_fs_entry_diff(&mut diffs, Some(&new_entry), &path);
                snap.insert(path, new_entry);
            }
        }

        // Report entries that existed in the previous snapshot but are gone now.
        let stale: Vec<String> = match &self.serialized_diff {
            Some(prev) => prev
                .snap
                .keys()
                .filter(|p| !self.fs.has_entry(p))
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        for path in &stale {
            self.add_fs_entry_diff(&mut diffs, None, path);
        }

        let default_libs = SyncSet::default();
        if let Some(f) = &self.default_libs {
            if let Some(libs) = f() {
                libs.range(|lib| {
                    default_libs.add(lib.clone());
                    true
                });
            }
        }
        self.serialized_diff = Some(Snapshot { snap, default_libs });

        let mut diff_keys: Vec<&String> = diffs.keys().collect();
        diff_keys.sort();
        for path in diff_keys {
            writeln!(baseline, "//// [{}] {}", path, diffs[path]).expect("write baseline");
        }
        writeln!(baseline).expect("write baseline");

        // Reset written files after baseline.
        self.written_files = SyncSet::default();
    }

    // Classifies the change at `path` (new / deleted / modified / rewrite /
    // mtime / lib) and records the rendered diff line in `diffs`.
    // Go: internal/testutil/fsbaselineutil/differ.go:FSDiffer.addFsEntryDiff
    fn add_fs_entry_diff(
        &self,
        diffs: &mut HashMap<String, String>,
        new_dir_content: Option<&DiffEntry>,
        path: &str,
    ) {
        let old = self.serialized_diff.as_ref().and_then(|s| s.snap.get(path));
        match (old, new_dir_content) {
            (None, _) => {
                // A path that is currently a default library is not reported as
                // *new* (mirrors Go's guard).
                let is_default_lib = self
                    .default_libs
                    .as_ref()
                    .and_then(|f| f())
                    .is_some_and(|libs| libs.has(&path.to_string()));
                if !is_default_lib {
                    let nc = new_dir_content.expect("new entry must be present when old is absent");
                    if !nc.symlink_target.is_empty() {
                        diffs.insert(path.to_string(), format!("-> {} *new*", nc.symlink_target));
                    } else {
                        diffs.insert(path.to_string(), format!("*new* \n{}", nc.content));
                    }
                }
            }
            (Some(_old), None) => {
                diffs.insert(path.to_string(), "*deleted*".to_string());
            }
            (Some(old), Some(nc)) => {
                if nc.content != old.content {
                    diffs.insert(path.to_string(), format!("*modified* \n{}", nc.content));
                } else if nc.is_written {
                    diffs.insert(path.to_string(), "*rewrite with same content*".to_string());
                } else if nc.mtime != old.mtime {
                    diffs.insert(path.to_string(), "*mTime changed*".to_string());
                } else {
                    // A lib file that was previously a default lib but is no
                    // longer one (i.e. it was actually read this run).
                    let old_libs_has = self
                        .serialized_diff
                        .as_ref()
                        .is_some_and(|s| s.default_libs.has(&path.to_string()));
                    let new_libs = self.default_libs.as_ref().and_then(|f| f());
                    let new_libs_has = new_libs
                        .as_ref()
                        .is_some_and(|libs| libs.has(&path.to_string()));
                    if old_libs_has && new_libs.is_some() && !new_libs_has {
                        diffs.insert(path.to_string(), format!("*Lib*\n{}", nc.content));
                    }
                }
            }
        }
    }
}

/// Replaces internal symbol names of shape `\u{FFFD}@symbolName@123` with
/// `\u{FFFD}@symbolName@<symbolId>`, so baselines don't churn on symbol ids
/// that vary between runs.
///
/// # Examples
/// ```
/// use tsgo_testutil_fsbaselineutil::sanitize_internal_symbol_name;
/// assert_eq!(sanitize_internal_symbol_name("plain"), "plain");
/// assert_eq!(
///     sanitize_internal_symbol_name("\u{FFFD}@foo@42"),
///     "\u{FFFD}@foo@<symbolId>"
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/fsbaselineutil/differ.go:SanitizeInternalSymbolName
pub fn sanitize_internal_symbol_name(s: &str) -> String {
    if !s.contains("\u{FFFD}@") {
        return s.to_string();
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\x{FFFD}@[^@]+@[0-9]+").expect("valid regex"));
    re.replace_all(s, |caps: &regex::Captures| {
        let m = &caps[0];
        let id_start = m.rfind('@').expect("match contains '@'");
        format!("{}@<symbolId>", &m[..id_start])
    })
    .into_owned()
}

#[cfg(test)]
#[path = "differ_test.rs"]
mod tests;
