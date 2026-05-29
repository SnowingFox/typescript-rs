//! In-memory file system for tests, with case-(in)sensitivity, symlinks, and a
//! mockable clock.
//!
//! 1:1 port of Go `internal/vfs/vfstest/vfstest.go`, merged with the in-memory
//! responsibilities of `internal/vfs/iovfs` and `internal/vfs/internal`.
//!
//! DIVERGENCE(port): Go builds on `testing/fstest.MapFS` and `io/fs`. Rust has
//! neither, so the node map (`canonical path -> node`) and symlink table are
//! written directly. Entries are keyed by canonical (case-folded) path and each
//! node remembers its original-cased `realpath`, exactly like the Go `sys`
//! shim. `unsafe.String`/`unsafe.Slice` zero-copy paths become owned
//! `String`/`Vec` (`// PERF(port)`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use tsgo_tspath::{
    get_canonical_file_name, get_directory_path, is_rooted_disk_path, normalize_path,
    remove_trailing_directory_separator,
};

use crate::internal::decode_bytes;
use crate::{Entries, FileInfo, FileMode, Fs, FsError, FsResult, WalkControl, WalkDirFunc};

/// A wall-clock abstraction so tests can supply deterministic time.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfstest::{Clock, SystemClock};
/// let c = SystemClock::new();
/// let _ = c.now();
/// ```
///
/// Side effects: implementations may read the system clock.
// Go: internal/vfs/vfstest/vfstest.go:Clock
pub trait Clock: Send + Sync {
    /// The current time.
    ///
    /// Side effects: may read the system clock.
    fn now(&self) -> SystemTime;

    /// Time elapsed since the clock was created.
    ///
    /// Side effects: may read the system clock.
    fn since_start(&self) -> Duration;
}

/// The default [`Clock`], backed by the real system clock.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfstest::{Clock, SystemClock};
/// assert!(SystemClock::new().since_start().as_nanos() < u128::MAX);
/// ```
///
/// Side effects: reads the system clock.
// Go: internal/vfs/vfstest/vfstest.go:clockImpl
pub struct SystemClock {
    start: SystemTime,
}

impl SystemClock {
    /// Creates a clock whose start time is now.
    ///
    /// Side effects: reads the system clock.
    pub fn new() -> Self {
        SystemClock {
            start: SystemTime::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn since_start(&self) -> Duration {
        self.now()
            .duration_since(self.start)
            .unwrap_or(Duration::ZERO)
    }
}

/// A file entry to seed [`MapFs::from_map`]: text, raw bytes, or a symlink.
///
/// DIVERGENCE(port): replaces Go's `any` file value (`string` | `[]byte` |
/// `*fstest.MapFile`). Rust's type system rejects invalid file types at compile
/// time, so the Go `invalid file type` runtime panic has no equivalent.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfstest::MapFile;
/// let _f = MapFile::text("hello");
/// let _l = MapFile::symlink("/target");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfstest/vfstest.go:FromMap (file value)
#[derive(Clone, Debug)]
pub struct MapFile {
    data: Vec<u8>,
    mode: FileMode,
}

impl MapFile {
    /// A regular file with UTF-8 text contents.
    ///
    /// Side effects: none (pure).
    pub fn text(s: &str) -> Self {
        MapFile {
            data: s.as_bytes().to_vec(),
            mode: FileMode::REGULAR,
        }
    }

    /// A regular file with raw byte contents.
    ///
    /// Side effects: none (pure).
    pub fn bytes(b: Vec<u8>) -> Self {
        MapFile {
            data: b,
            mode: FileMode::REGULAR,
        }
    }

    /// A symbolic link pointing at `target`.
    ///
    /// Side effects: none (pure).
    // Go: internal/vfs/vfstest/vfstest.go:Symlink
    pub fn symlink(target: &str) -> Self {
        MapFile {
            data: target.as_bytes().to_vec(),
            mode: FileMode::SYMLINK,
        }
    }

    pub(crate) fn into_parts(self) -> (Vec<u8>, FileMode) {
        (self.data, self.mode)
    }
}

impl From<&str> for MapFile {
    fn from(s: &str) -> Self {
        MapFile::text(s)
    }
}

impl From<String> for MapFile {
    fn from(s: String) -> Self {
        MapFile::text(&s)
    }
}

impl From<Vec<u8>> for MapFile {
    fn from(b: Vec<u8>) -> Self {
        MapFile::bytes(b)
    }
}

#[derive(Clone, Debug)]
struct Node {
    // Original-cased relative path (no leading slash); mirrors Go's `sys.realpath`.
    realpath: String,
    data: Vec<u8>,
    mode: FileMode,
    mod_time: SystemTime,
}

struct State {
    // Keyed by canonical (case-folded) relative path.
    m: HashMap<String, Node>,
    // Canonical link path -> canonical target path.
    symlinks: HashMap<String, String>,
    use_case_sensitive: bool,
    clock: Arc<dyn Clock>,
}

/// The in-memory file system.
///
/// # Examples
/// ```
/// use tsgo_vfs::Fs;
/// use tsgo_vfs::vfstest::MapFs;
/// let fs = MapFs::from_map([("/foo.ts", "hello")], false);
/// assert_eq!(fs.read_file("/foo.ts").as_deref(), Some("hello"));
/// ```
///
/// Side effects: holds all file contents in memory.
// Go: internal/vfs/vfstest/vfstest.go:MapFS
pub struct MapFs {
    state: RwLock<State>,
    use_case_sensitive: bool,
}

// Splits `s` at the first '/' at or after `offset`, returning (before, after).
// Go: internal/vfs/vfstest/vfstest.go:splitPath
fn split_path_offset(s: &str, offset: usize) -> (String, String) {
    match s[offset..].find('/') {
        None => (s.to_string(), String::new()),
        Some(idx) => (
            s[..idx + offset].to_string(),
            s[idx + 1 + offset..].to_string(),
        ),
    }
}

// Directory portion of a relative path ("" if none).
// Go: internal/vfs/vfstest/vfstest.go:dirName
fn dir_name(p: &str) -> String {
    match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

// Base name of a relative path.
// Go: internal/vfs/vfstest/vfstest.go:baseName
fn base_name(p: &str) -> String {
    match p.rfind('/') {
        Some(i) => p[i + 1..].to_string(),
        None => p.to_string(),
    }
}

// Compares two paths segment by segment, mirroring Go's comparePathsByParts so
// that intermediate directories are created before their children.
// Go: internal/vfs/vfstest/vfstest.go:comparePathsByParts
fn compare_paths_by_parts(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        match (a.split_once('/'), b.split_once('/')) {
            (Some((a_start, a_end)), Some((b_start, b_end))) => {
                let r = a_start.cmp(b_start);
                if r != std::cmp::Ordering::Equal {
                    return r;
                }
                a = a_end;
                b = b_end;
            }
            _ => return a.cmp(b),
        }
    }
}

impl State {
    fn get_canonical(&self, p: &str) -> String {
        get_canonical_file_name(p, self.use_case_sensitive)
    }

    // Follows symlinks from canonical path `p` to a real (non-symlink) node.
    // Returns the resolved canonical key on success; on failure returns the
    // deepest canonical reached plus the error (mirrors Go's third return).
    // Go: internal/vfs/vfstest/vfstest.go:getFollowingSymlinksWorker
    fn follow(&self, p: &str) -> Result<String, (String, FsError)> {
        self.follow_worker(p, "", "")
    }

    fn follow_worker(&self, p: &str, from: &str, to: &str) -> Result<String, (String, FsError)> {
        if let Some(node) = self.m.get(p) {
            if !node.mode.is_symlink() {
                return Ok(p.to_string());
            }
        }
        if let Some(target) = self.symlinks.get(p) {
            let target = target.clone();
            return self.follow_worker(&target, p, &target);
        }
        // The path could live underneath a symlinked directory.
        let mut redirect = None;
        for (other, target) in &self.symlinks {
            if other.len() < p.len()
                && p.as_bytes()[other.len()] == b'/'
                && &p[..other.len()] == other.as_str()
            {
                redirect = Some((
                    format!("{}{}", target, &p[other.len()..]),
                    other.clone(),
                    target.clone(),
                ));
                break;
            }
        }
        if let Some((new_p, other, target)) = redirect {
            return self.follow_worker(&new_p, &other, &target);
        }
        let err = if from.is_empty() {
            FsError::NotExist
        } else {
            FsError::BrokenSymlink {
                from: from.to_string(),
                to: to.to_string(),
            }
        };
        Err((p.to_string(), err))
    }

    // Go: internal/vfs/vfstest/vfstest.go:setEntry
    fn set_entry(&mut self, realpath: &str, canonical: &str, data: Vec<u8>, mode: FileMode) {
        if realpath.is_empty() || canonical.is_empty() {
            panic!("empty path");
        }
        if mode.is_symlink() {
            let target = String::from_utf8_lossy(&data).into_owned();
            self.symlinks
                .insert(canonical.to_string(), self.get_canonical(&target));
        }
        self.m.insert(
            canonical.to_string(),
            Node {
                realpath: realpath.to_string(),
                data,
                mode,
                mod_time: self.clock.now(),
            },
        );
    }

    // Go: internal/vfs/vfstest/vfstest.go:mkdirAll
    fn mkdir_all(&mut self, p: &str) -> FsResult<()> {
        if p.is_empty() {
            panic!("empty path");
        }
        // Fast path: already exists as a directory.
        if let Ok(resolved) = self.follow(&self.get_canonical(p)) {
            return if self.m[&resolved].mode.is_dir() {
                Ok(())
            } else {
                Err(FsError::Other(format!(
                    "mkdir {p:?}: path exists but is not a directory"
                )))
            };
        }

        let mut to_create: Vec<String> = Vec::new();
        let mut p = p.to_string();
        let mut offset = 0usize;
        loop {
            let (dir, rest) = split_path_offset(&p, offset);
            let canonical = self.get_canonical(&dir);
            match self.follow(&canonical) {
                Err((_, e)) => {
                    if !e.is_not_exist() {
                        return Err(e);
                    }
                    to_create.push(dir.clone());
                }
                Ok(resolved) => {
                    if !self.m[&resolved].mode.is_dir() {
                        return Err(FsError::Other(format!(
                            "mkdir {resolved:?}: path exists but is not a directory"
                        )));
                    }
                    if canonical != resolved {
                        // Symlinked parent: restart from the real path.
                        p = format!("{}/{}", self.m[&resolved].realpath, rest);
                        to_create.clear();
                        offset = 0;
                        continue;
                    }
                }
            }
            if rest.is_empty() {
                break;
            }
            offset = dir.len() + 1;
        }

        for dir in to_create {
            let canonical = self.get_canonical(&dir);
            self.set_entry(&dir, &canonical, Vec::new(), FileMode::DIR);
        }
        Ok(())
    }

    // Go: internal/vfs/vfstest/vfstest.go:WriteFile
    fn write_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        let parent = dir_name(path);
        if !parent.is_empty() {
            let canonical = self.get_canonical(&parent);
            match self.follow(&canonical) {
                Err((_, e)) => return Err(FsError::Other(format!("write {path:?}: {e}"))),
                Ok(resolved) => {
                    if !self.m[&resolved].mode.is_dir() {
                        return Err(FsError::Other(format!(
                            "write {path:?}: parent path exists but is not a directory"
                        )));
                    }
                }
            }
        }

        let cp = match self.follow(&self.get_canonical(path)) {
            Ok(resolved) => {
                if !self.m[&resolved].mode.is_regular() {
                    return Err(FsError::Other(format!(
                        "write {path:?}: path exists but is not a regular file"
                    )));
                }
                resolved
            }
            Err((cp, e)) => {
                if !e.is_not_exist() && !matches!(e, FsError::BrokenSymlink { .. }) {
                    panic!("{e}");
                }
                cp
            }
        };

        // PERF(port): Go aliases `data` via `unsafe.Slice`; here we copy.
        self.set_entry(path, &cp, data.to_vec(), FileMode::REGULAR);
        Ok(())
    }

    // Go: internal/vfs/vfstest/vfstest.go:AppendFile
    fn append_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        let parent = dir_name(path);
        if !parent.is_empty() {
            let canonical = self.get_canonical(&parent);
            match self.follow(&canonical) {
                Err((_, e)) => return Err(FsError::Other(format!("append {path:?}: {e}"))),
                Ok(resolved) => {
                    if !self.m[&resolved].mode.is_dir() {
                        return Err(FsError::Other(format!(
                            "append {path:?}: parent path exists but is not a directory"
                        )));
                    }
                }
            }
        }

        let (mut existing, cp) = match self.follow(&self.get_canonical(path)) {
            Ok(resolved) => {
                if !self.m[&resolved].mode.is_regular() {
                    return Err(FsError::Other(format!(
                        "append {path:?}: path exists but is not a regular file"
                    )));
                }
                (self.m[&resolved].data.clone(), resolved)
            }
            Err((cp, e)) => {
                if !e.is_not_exist() && !matches!(e, FsError::BrokenSymlink { .. }) {
                    panic!("{e}");
                }
                (Vec::new(), cp)
            }
        };

        existing.extend_from_slice(data);
        self.set_entry(path, &cp, existing, FileMode::REGULAR);
        Ok(())
    }

    // Go: internal/vfs/vfstest/vfstest.go:remove
    fn remove(&mut self, path: &str) -> FsResult<()> {
        let canonical = self.get_canonical(path);
        let Some(node) = self.m.get(&canonical) else {
            return Ok(());
        };
        let is_dir = node.mode.is_dir();
        self.m.remove(&canonical);
        self.symlinks.remove(&canonical);
        if is_dir {
            let prefix = format!("{canonical}/");
            self.m.retain(|k, _| !k.starts_with(&prefix));
            self.symlinks.retain(|k, _| !k.starts_with(&prefix));
        }
        Ok(())
    }

    // Go: internal/vfs/vfstest/vfstest.go:Chtimes
    fn chtimes(&mut self, path: &str, mtime: SystemTime) -> FsResult<()> {
        let canonical = self.get_canonical(path);
        match self.m.get_mut(&canonical) {
            Some(node) => {
                node.mod_time = mtime;
                Ok(())
            }
            None => Err(FsError::NotExist),
        }
    }

    // Resolves `rel` to a directory's canonical key (""=root).
    fn resolve_dir(&self, rel: &str) -> Option<String> {
        if rel.is_empty() || rel == "." {
            return Some(String::new());
        }
        let canonical = self.get_canonical(rel);
        let resolved = self.follow(&canonical).ok()?;
        if self.m[&resolved].mode.is_dir() {
            Some(resolved)
        } else {
            None
        }
    }

    // Canonical keys of the direct children of `dir_canonical`, sorted.
    fn child_keys(&self, dir_canonical: &str) -> Vec<String> {
        let prefix = if dir_canonical.is_empty() {
            String::new()
        } else {
            format!("{dir_canonical}/")
        };
        let mut keys: Vec<String> = self
            .m
            .keys()
            .filter(|k| {
                if dir_canonical.is_empty() {
                    !k.is_empty() && !k.contains('/')
                } else {
                    k.len() > prefix.len()
                        && k.starts_with(&prefix)
                        && !k[prefix.len()..].contains('/')
                }
            })
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn file_info_of(&self, canonical: &str) -> FileInfo {
        let node = &self.m[canonical];
        FileInfo::new(
            base_name(&node.realpath),
            node.data.len() as i64,
            node.mode,
            node.mod_time,
        )
    }
}

impl MapFs {
    /// Builds an in-memory FS from rooted, normalized paths.
    ///
    /// Paths must be absolute and normalized (no trailing separator), and all
    /// POSIX-style (`/x`) or all Windows-style (`c:/x`), not a mix.
    ///
    /// # Examples
    /// ```
    /// use tsgo_vfs::Fs;
    /// use tsgo_vfs::vfstest::MapFs;
    /// let fs = MapFs::from_map([("/a.ts", "x")], false);
    /// assert!(fs.file_exists("/a.ts"));
    /// ```
    ///
    /// Side effects: allocates the in-memory tree.
    ///
    /// # Panics
    /// Panics on non-rooted, non-normalized, mixed, or duplicate paths.
    // Go: internal/vfs/vfstest/vfstest.go:FromMap
    pub fn from_map<I, K, V>(entries: I, use_case_sensitive_file_names: bool) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<MapFile>,
    {
        Self::from_map_with_clock(
            entries,
            use_case_sensitive_file_names,
            Arc::new(SystemClock::new()),
        )
    }

    /// Like [`MapFs::from_map`] but with an explicit [`Clock`].
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    /// use tsgo_vfs::vfstest::{MapFs, SystemClock};
    /// let fs = MapFs::from_map_with_clock([("/a.ts", "x")], false, Arc::new(SystemClock::new()));
    /// let _ = fs;
    /// ```
    ///
    /// Side effects: allocates the in-memory tree.
    ///
    /// # Panics
    /// Panics on non-rooted, non-normalized, mixed, or duplicate paths.
    // Go: internal/vfs/vfstest/vfstest.go:FromMapWithClock
    pub fn from_map_with_clock<I, K, V>(
        entries: I,
        use_case_sensitive_file_names: bool,
        clock: Arc<dyn Clock>,
    ) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<MapFile>,
    {
        let mut posix = false;
        let mut windows = false;
        let check_path = |p: &str, posix: &mut bool, windows: &mut bool| {
            if !is_rooted_disk_path(p) {
                panic!("non-rooted path {p:?}");
            }
            if remove_trailing_directory_separator(&normalize_path(p)) != p {
                panic!("non-normalized path {p:?}");
            }
            if p.starts_with('/') {
                *posix = true;
            } else {
                *windows = true;
            }
        };

        let mut seeds: Vec<(String, MapFile)> = entries
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        // Sorted creation so modification times are ordered (Go does the same).
        seeds.sort_by(|a, b| compare_paths_by_parts(&a.0, &b.0));

        let mut rel_seeds: Vec<(String, Vec<u8>, FileMode)> = Vec::with_capacity(seeds.len());
        for (p, file) in seeds {
            check_path(&p, &mut posix, &mut windows);
            let (data, mode) = if file.mode.is_symlink() {
                let target = String::from_utf8_lossy(&file.data).into_owned();
                check_path(&target, &mut posix, &mut windows);
                let target = target.strip_prefix('/').unwrap_or(&target).to_string();
                (target.into_bytes(), FileMode::SYMLINK)
            } else {
                (file.data, file.mode)
            };
            let rel = p.strip_prefix('/').unwrap_or(&p).to_string();
            rel_seeds.push((rel, data, mode));
        }

        if posix && windows {
            panic!("mixed posix and windows paths");
        }

        Self::convert_map_fs(rel_seeds, use_case_sensitive_file_names, clock)
    }

    // Builds the FS from relative-keyed seeds, creating intermediate directories
    // and rejecting canonical-path collisions.
    // Go: internal/vfs/vfstest/vfstest.go:convertMapFS
    pub(crate) fn convert_map_fs(
        mut seeds: Vec<(String, Vec<u8>, FileMode)>,
        use_case_sensitive_file_names: bool,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let mut state = State {
            m: HashMap::new(),
            symlinks: HashMap::new(),
            use_case_sensitive: use_case_sensitive_file_names,
            clock,
        };

        // Reject duplicate canonical paths.
        let mut seen: HashMap<String, String> = HashMap::new();
        for (rel, _, _) in &seeds {
            let canonical = state.get_canonical(rel);
            if let Some(other) = seen.get(&canonical) {
                let (lo, hi) = if rel < other {
                    (rel.clone(), other.clone())
                } else {
                    (other.clone(), rel.clone())
                };
                panic!("duplicate path: {lo:?} and {hi:?} have the same canonical path");
            }
            seen.insert(canonical, rel.clone());
        }

        seeds.sort_by(|a, b| compare_paths_by_parts(&a.0, &b.0));
        for (rel, data, mode) in seeds {
            let dir = dir_name(&rel);
            if !dir.is_empty() {
                if let Err(e) = state.mkdir_all(&dir) {
                    panic!("failed to create intermediate directories for {rel:?}: {e}");
                }
            }
            let canonical = state.get_canonical(&rel);
            state.set_entry(&rel, &canonical, data, mode);
        }

        MapFs {
            use_case_sensitive: use_case_sensitive_file_names,
            state: RwLock::new(state),
        }
    }

    // Converts a rooted, normalized path into its relative key and whether it had
    // a leading slash (POSIX). Asserts the path is rooted.
    fn rooted_rel_normalized(&self, path: &str) -> (String, bool) {
        let normalized = normalize_path(path);
        let _ = crate::internal::root_length(&normalized);
        let had_slash = normalized.starts_with('/');
        let trimmed = remove_trailing_directory_separator(&normalized);
        let rel = trimmed.strip_prefix('/').unwrap_or(trimmed).to_string();
        (rel, had_slash)
    }

    // Relative key for write-family methods (Go uses the raw path, not normalized).
    fn rooted_rel_raw(&self, path: &str) -> String {
        let _ = crate::internal::root_length(path);
        path.strip_prefix('/').unwrap_or(path).to_string()
    }

    // ---- relative (io/fs-level) helpers used by the convertMapFS tests ----

    fn read_bytes_rel(&self, rel: &str) -> Option<Vec<u8>> {
        let state = self.state.read().expect("vfstest state poisoned");
        let canonical = state.get_canonical(rel);
        let resolved = state.follow(&canonical).ok()?;
        let node = &state.m[&resolved];
        if node.mode.is_regular() {
            Some(node.data.clone())
        } else {
            None
        }
    }

    fn stat_rel(&self, rel: &str) -> Option<FileInfo> {
        let state = self.state.read().expect("vfstest state poisoned");
        if rel.is_empty() || rel == "." {
            return Some(FileInfo::new(
                ".".into(),
                0,
                FileMode::DIR,
                SystemTime::UNIX_EPOCH,
            ));
        }
        let canonical = state.get_canonical(rel);
        let resolved = state.follow(&canonical).ok()?;
        Some(state.file_info_of(&resolved))
    }

    fn realpath_rel(&self, rel: &str) -> FsResult<String> {
        let state = self.state.read().expect("vfstest state poisoned");
        let canonical = state.get_canonical(rel);
        match state.follow(&canonical) {
            Ok(resolved) => Ok(state.m[&resolved].realpath.clone()),
            Err((_, e)) => Err(e),
        }
    }

    /// Lists the directory entries at the relative path `rel` (test helper used
    /// by vfstest fixtures; mirrors the rooted read-dir lookup).
    ///
    /// Side effects: none (reads in-memory state).
    pub fn read_dir_rel(&self, rel: &str) -> FsResult<Vec<FileInfo>> {
        let state = self.state.read().expect("vfstest state poisoned");
        let dir = state.resolve_dir(rel).ok_or(FsError::NotExist)?;
        Ok(state
            .child_keys(&dir)
            .iter()
            .map(|k| state.file_info_of(k))
            .collect())
    }

    fn walk_node(
        state: &State,
        canonical: &str,
        display: &str,
        info: &FileInfo,
        walk_fn: &mut WalkDirFunc,
    ) -> FsResult<WalkSignal> {
        match walk_fn(display, info)? {
            WalkControl::Continue => {}
            WalkControl::SkipDir => {
                return Ok(if info.is_dir() {
                    WalkSignal::None
                } else {
                    WalkSignal::SkipDir
                });
            }
            WalkControl::SkipAll => return Ok(WalkSignal::SkipAll),
        }
        if !info.is_dir() {
            return Ok(WalkSignal::None);
        }
        for child in state.child_keys(canonical) {
            let child_info = state.file_info_of(&child);
            let child_display = join_display(display, child_info.name());
            match Self::walk_node(state, &child, &child_display, &child_info, walk_fn)? {
                WalkSignal::None => {}
                WalkSignal::SkipDir => break,
                WalkSignal::SkipAll => return Ok(WalkSignal::SkipAll),
            }
        }
        Ok(WalkSignal::None)
    }
}

#[derive(Clone, Copy)]
enum WalkSignal {
    None,
    SkipDir,
    SkipAll,
}

// Joins a display path with a child name, avoiding a double slash after a root.
fn join_display(display: &str, name: &str) -> String {
    if display.ends_with('/') {
        format!("{display}{name}")
    } else {
        format!("{display}/{name}")
    }
}

impl Fs for MapFs {
    // Go: internal/vfs/iovfs/iofs.go:ioFS.UseCaseSensitiveFileNames
    fn use_case_sensitive_file_names(&self) -> bool {
        self.use_case_sensitive
    }

    // Go: internal/vfs/internal/internal.go:Common.FileExists
    fn file_exists(&self, path: &str) -> bool {
        match self.stat(path) {
            Some(info) => !info.is_dir(),
            None => false,
        }
    }

    // Go: internal/vfs/internal/internal.go:Common.ReadFile
    fn read_file(&self, path: &str) -> Option<String> {
        let (rel, _) = self.rooted_rel_normalized(path);
        self.read_bytes_rel(&rel).map(|bytes| decode_bytes(&bytes))
    }

    // Go: internal/vfs/iovfs/iofs.go:ioFS.WriteFile
    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        let rel_raw = self.rooted_rel_raw(path);
        {
            let mut state = self.state.write().expect("vfstest state poisoned");
            if state.write_file(&rel_raw, data.as_bytes()).is_ok() {
                return Ok(());
            }
        }
        let mkdir_dir = get_directory_path(&normalize_path(path));
        let mkdir_rel = mkdir_dir
            .strip_prefix('/')
            .unwrap_or(&mkdir_dir)
            .to_string();
        {
            let mut state = self.state.write().expect("vfstest state poisoned");
            if !mkdir_rel.is_empty() {
                state.mkdir_all(&mkdir_rel)?;
            }
        }
        let mut state = self.state.write().expect("vfstest state poisoned");
        state.write_file(&rel_raw, data.as_bytes())
    }

    // Go: internal/vfs/iovfs/iofs.go:ioFS.AppendFile
    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        let rel_raw = self.rooted_rel_raw(path);
        {
            let mut state = self.state.write().expect("vfstest state poisoned");
            if state.append_file(&rel_raw, data.as_bytes()).is_ok() {
                return Ok(());
            }
        }
        let mkdir_dir = get_directory_path(&normalize_path(path));
        let mkdir_rel = mkdir_dir
            .strip_prefix('/')
            .unwrap_or(&mkdir_dir)
            .to_string();
        {
            let mut state = self.state.write().expect("vfstest state poisoned");
            if !mkdir_rel.is_empty() {
                state.mkdir_all(&mkdir_rel)?;
            }
        }
        let mut state = self.state.write().expect("vfstest state poisoned");
        state.append_file(&rel_raw, data.as_bytes())
    }

    // Go: internal/vfs/vfstest/vfstest.go:Remove
    fn remove(&self, path: &str) -> FsResult<()> {
        let rel = self.rooted_rel_raw(path);
        let mut state = self.state.write().expect("vfstest state poisoned");
        state.remove(&rel)
    }

    // Go: internal/vfs/vfstest/vfstest.go:Chtimes
    fn chtimes(&self, path: &str, _atime: SystemTime, mtime: SystemTime) -> FsResult<()> {
        let rel = self.rooted_rel_raw(path);
        let mut state = self.state.write().expect("vfstest state poisoned");
        state.chtimes(&rel, mtime)
    }

    // Go: internal/vfs/internal/internal.go:Common.DirectoryExists
    fn directory_exists(&self, path: &str) -> bool {
        match self.stat(path) {
            Some(info) => info.is_dir(),
            None => false,
        }
    }

    // Go: internal/vfs/internal/internal.go:Common.GetAccessibleEntries
    fn get_accessible_entries(&self, path: &str) -> Entries {
        let (rel, _) = self.rooted_rel_normalized(path);
        let state = self.state.read().expect("vfstest state poisoned");
        let mut result = Entries {
            symlinks: Some(std::collections::HashSet::new()),
            ..Default::default()
        };
        let Some(dir) = state.resolve_dir(&rel) else {
            return result;
        };
        for child in state.child_keys(&dir) {
            let node = &state.m[&child];
            let name = base_name(&node.realpath);
            if node.mode.is_dir() {
                result.directories.push(name);
            } else if node.mode.is_regular() {
                result.files.push(name);
            } else if node.mode.is_symlink() {
                if let Ok(resolved) = state.follow(&child) {
                    let target = &state.m[&resolved];
                    if target.mode.is_dir() {
                        result.directories.push(name.clone());
                    } else if target.mode.is_regular() {
                        result.files.push(name.clone());
                    } else {
                        continue;
                    }
                    if let Some(set) = result.symlinks.as_mut() {
                        set.insert(name);
                    }
                }
            }
        }
        result
    }

    // Go: internal/vfs/internal/internal.go:Common.Stat
    fn stat(&self, path: &str) -> Option<FileInfo> {
        let (rel, _) = self.rooted_rel_normalized(path);
        self.stat_rel(&rel)
    }

    // Go: internal/vfs/internal/internal.go:Common.WalkDir
    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        let (rel, had_slash) = self.rooted_rel_normalized(root);
        let state = self.state.read().expect("vfstest state poisoned");
        let (canonical, info) = if rel.is_empty() || rel == "." {
            (
                String::new(),
                FileInfo::new(".".into(), 0, FileMode::DIR, SystemTime::UNIX_EPOCH),
            )
        } else {
            let canonical = state.get_canonical(&rel);
            let resolved = state.follow(&canonical).map_err(|(_, e)| e)?;
            (resolved.clone(), state.file_info_of(&resolved))
        };
        let display = if had_slash {
            format!("/{rel}")
        } else {
            rel.clone()
        };
        Self::walk_node(&state, &canonical, &display, &info, walk_fn)?;
        Ok(())
    }

    // Go: internal/vfs/iovfs/iofs.go:ioFS.Realpath
    fn realpath(&self, path: &str) -> String {
        let (rel, had_slash) = self.rooted_rel_normalized(path);
        match self.realpath_rel(&rel) {
            Ok(rp) => {
                if had_slash {
                    format!("/{rp}")
                } else {
                    rp
                }
            }
            Err(_) => path.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
