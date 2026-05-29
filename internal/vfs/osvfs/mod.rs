//! Real-disk file system backed by `std::fs`.
//!
//! 1:1 port of Go `internal/vfs/osvfs/os.go` plus its platform-specific
//! `realpath_*.go` / `reparsepoint_*.go` files.
//!
//! DIVERGENCE(port): Go layers on `os.DirFS` + `io/fs`; here methods call
//! `std::fs` directly. Go throttles syscalls with `LimitedSemaphore` (read 128 /
//! write 32 / blocking 128); that is a pure performance optimization with no
//! behavioral effect and is omitted (`// PERF(port)`). `realpath` uses
//! `std::fs::canonicalize`; Go's macOS `F_GETPATH` and Linux `/proc/self/fd`
//! fast paths are perf-only and are not ported.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;
use std::time::SystemTime;

use tsgo_tspath::{
    combine_paths, get_directory_path, normalize_path, normalize_slashes,
    remove_trailing_directory_separator,
};

use crate::internal::{decode_bytes, root_length};
use crate::{Entries, FileInfo, FileMode, Fs, FsError, FsResult, WalkControl, WalkDirFunc};

/// Returns the singleton real-disk [`Fs`].
///
/// # Examples
/// ```no_run
/// use tsgo_vfs::Fs;
/// let fs = tsgo_vfs::osvfs::fs();
/// let _ = fs.use_case_sensitive_file_names();
/// ```
///
/// Side effects: none at construction; methods perform disk I/O.
// Go: internal/vfs/osvfs/os.go:FS
pub fn fs() -> OsFs {
    OsFs
}

/// The real-disk file system.
///
/// Side effects: methods perform disk I/O.
// Go: internal/vfs/osvfs/os.go:osFS
#[derive(Clone, Copy, Debug, Default)]
pub struct OsFs;

// Swaps the case of each character, mirroring Go's swapCase.
// Go: internal/vfs/osvfs/os.go:swapCase
fn swap_case(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_uppercase() {
                c.to_lowercase().collect::<Vec<_>>()
            } else if c.is_lowercase() {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

#[cfg(windows)]
fn compute_case_sensitive() -> bool {
    false
}

#[cfg(not(windows))]
fn compute_case_sensitive() -> bool {
    // Probe by checking whether this executable exists under a case-swapped path.
    match std::env::current_exe() {
        Ok(exe) => {
            let swapped = swap_case(&exe.to_string_lossy());
            // If the swapped-case path exists, the file system is case-insensitive.
            !Path::new(&swapped).exists()
        }
        // DIVERGENCE(port): Go panics if the executable path is unavailable;
        // here we conservatively assume case-sensitive.
        Err(_) => true,
    }
}

static IS_CASE_SENSITIVE: LazyLock<bool> = LazyLock::new(compute_case_sensitive);

#[cfg(target_os = "macos")]
fn realpath_native(path: &Path) -> std::io::Result<std::path::PathBuf> {
    // PERF(port): Go uses open(O_EVTONLY)+fcntl(F_GETPATH) for O(1) resolution.
    std::fs::canonicalize(path)
}

#[cfg(target_os = "linux")]
fn realpath_native(path: &Path) -> std::io::Result<std::path::PathBuf> {
    // PERF(port): Go uses open(O_PATH)+readlink(/proc/self/fd) for O(1) resolution.
    std::fs::canonicalize(path)
}

#[cfg(windows)]
fn realpath_native(path: &Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::canonicalize(path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn realpath_native(path: &Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::canonicalize(path)
}

#[cfg(windows)]
fn is_reparse_point(path: &str) -> bool {
    // Approximation of Go's IsReparsePoint: treat symlinks/junctions as reparse
    // points. A full junction probe would require Windows reparse-tag inspection.
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_reparse_point(_path: &str) -> bool {
    // Only Windows has reparse points; mirrors Go's nil hook on other OSes.
    false
}

fn base_name(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[i + 1..].to_string(),
        None => path.to_string(),
    }
}

fn info_from_metadata(name: String, md: &std::fs::Metadata) -> FileInfo {
    let mode = if md.is_dir() {
        FileMode::DIR
    } else {
        FileMode::REGULAR
    };
    FileInfo::new(
        name,
        md.len() as i64,
        mode,
        md.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    )
}

impl OsFs {
    fn stat_path(&self, path: &str) -> Option<FileInfo> {
        let md = std::fs::metadata(path).ok()?;
        Some(info_from_metadata(base_name(path), &md))
    }

    fn write_with_flag(&self, path: &str, content: &str, append: bool) -> std::io::Result<()> {
        let mut opts = OpenOptions::new();
        opts.write(true).create(true);
        if append {
            opts.append(true);
        } else {
            opts.truncate(true);
        }
        let mut file = opts.open(path)?;
        file.write_all(content.as_bytes())
    }

    fn write_file_ensuring_dir(&self, path: &str, content: &str, append: bool) -> FsResult<()> {
        let _ = root_length(path);
        if self.write_with_flag(path, content, append).is_ok() {
            return Ok(());
        }
        let dir = get_directory_path(&normalize_path(path));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Err(FsError::Other(e.to_string()));
        }
        self.write_with_flag(path, content, append)
            .map_err(|e| FsError::Other(e.to_string()))
    }

    fn walk_node(
        &self,
        native: &Path,
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
        let mut entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(native) {
            Ok(rd) => rd.filter_map(Result::ok).collect(),
            Err(_) => return Ok(WalkSignal::None),
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let child_native = entry.path();
            let child_display = if display.ends_with('/') {
                format!("{display}{name}")
            } else {
                format!("{display}/{name}")
            };
            let mode = if file_type.is_dir() {
                FileMode::DIR
            } else if file_type.is_symlink() {
                FileMode::SYMLINK
            } else {
                FileMode::REGULAR
            };
            let size = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
            let mod_time = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let child_info = FileInfo::new(name, size, mode, mod_time);
            match self.walk_node(&child_native, &child_display, &child_info, walk_fn)? {
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

impl Fs for OsFs {
    // Go: internal/vfs/osvfs/os.go:UseCaseSensitiveFileNames
    fn use_case_sensitive_file_names(&self) -> bool {
        *IS_CASE_SENSITIVE
    }

    // Go: internal/vfs/osvfs/os.go:FileExists
    fn file_exists(&self, path: &str) -> bool {
        let _ = root_length(&normalize_path(path));
        match self.stat_path(path) {
            Some(info) => !info.is_dir(),
            None => false,
        }
    }

    // Go: internal/vfs/osvfs/os.go:ReadFile
    fn read_file(&self, path: &str) -> Option<String> {
        let _ = root_length(&normalize_path(path));
        std::fs::read(path).ok().map(|bytes| decode_bytes(&bytes))
    }

    // Go: internal/vfs/osvfs/os.go:WriteFile
    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.write_file_ensuring_dir(path, data, false)
    }

    // Go: internal/vfs/osvfs/os.go:AppendFile
    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.write_file_ensuring_dir(path, data, true)
    }

    // Go: internal/vfs/osvfs/os.go:Remove
    fn remove(&self, path: &str) -> FsResult<()> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(FsError::Other(e.to_string())),
            Ok(md) => {
                let result = if md.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
                result.map_err(|e| FsError::Other(e.to_string()))
            }
        }
    }

    // Go: internal/vfs/osvfs/os.go:Chtimes
    fn chtimes(&self, path: &str, _atime: SystemTime, mtime: SystemTime) -> FsResult<()> {
        // PERF(port): Go's os.Chtimes sets both atime and mtime; std only exposes
        // set_modified, so atime is left unchanged.
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .map_err(|e| FsError::Other(e.to_string()))?;
        file.set_modified(mtime)
            .map_err(|e| FsError::Other(e.to_string()))
    }

    // Go: internal/vfs/osvfs/os.go:DirectoryExists
    fn directory_exists(&self, path: &str) -> bool {
        let _ = root_length(&normalize_path(path));
        match self.stat_path(path) {
            Some(info) => info.is_dir(),
            None => false,
        }
    }

    // Go: internal/vfs/internal/internal.go:Common.GetAccessibleEntries
    fn get_accessible_entries(&self, path: &str) -> Entries {
        let normalized = normalize_path(path);
        let _ = root_length(&normalized);
        let mut result = Entries {
            symlinks: Some(std::collections::HashSet::new()),
            ..Default::default()
        };
        let mut entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(&normalized) {
            Ok(rd) => rd.filter_map(Result::ok).collect(),
            Err(_) => return result,
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                result.directories.push(name);
                continue;
            }
            if file_type.is_file() {
                result.files.push(name);
                continue;
            }
            let is_link = file_type.is_symlink() || {
                let full = format!("{normalized}/{name}");
                is_reparse_point(&full)
            };
            if is_link {
                // std::fs::metadata follows symlinks (DirEntry::metadata does not).
                if let Ok(md) = std::fs::metadata(entry.path()) {
                    if md.is_dir() {
                        result.directories.push(name.clone());
                    } else if md.is_file() {
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

    // Go: internal/vfs/osvfs/os.go:Stat
    fn stat(&self, path: &str) -> Option<FileInfo> {
        let _ = root_length(&normalize_path(path));
        self.stat_path(path)
    }

    // Go: internal/vfs/osvfs/os.go:WalkDir
    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        let normalized = remove_trailing_directory_separator(&normalize_path(root)).to_string();
        let _ = root_length(&normalized);
        let md = std::fs::metadata(&normalized).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FsError::NotExist
            } else {
                FsError::Other(e.to_string())
            }
        })?;
        let info = info_from_metadata(base_name(&normalized), &md);
        self.walk_node(Path::new(&normalized), &normalized, &info, walk_fn)?;
        Ok(())
    }

    // Go: internal/vfs/osvfs/os.go:Realpath / osFSRealpath
    fn realpath(&self, path: &str) -> String {
        let _ = root_length(path);
        match realpath_native(Path::new(path)) {
            Ok(resolved) => normalize_slashes(&resolved.to_string_lossy()),
            Err(_) => path.to_string(),
        }
    }
}

/// Returns the OS-specific location for the global typings cache.
///
/// # Examples
/// ```no_run
/// let _ = tsgo_vfs::osvfs::get_global_typings_cache_location();
/// ```
///
/// Side effects: reads environment variables to locate the user cache directory.
// Go: internal/vfs/osvfs/os.go:GetGlobalTypingsCacheLocation
pub fn get_global_typings_cache_location() -> String {
    let cache_dir =
        user_cache_dir().unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
    let subdir = if cfg!(windows) {
        "Microsoft/TypeScript"
    } else {
        "typescript"
    };
    combine_paths(
        &cache_dir,
        &[subdir, tsgo_core::version::version_major_minor()],
    )
}

// DIVERGENCE(port): std has no `os.UserCacheDir`; this approximates it per
// platform from environment variables.
fn user_cache_dir() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|home| format!("{home}/Library/Caches"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("LocalAppData").ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var("XDG_CACHE_HOME").ok().or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| format!("{home}/.cache"))
        })
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
