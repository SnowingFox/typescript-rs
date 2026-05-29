//! Adapter that presents a map of relative paths as an `Fs`.
//!
//! 1:1 port of Go `internal/vfs/iovfs/iofs.go`.
//!
//! DIVERGENCE(port): Go's `iovfs.From` wraps an arbitrary `io/fs.FS`. Rust has
//! no `io/fs.FS`, so the in-memory backing store is the merged `MapFs`; `from`
//! constructs one from relative-keyed entries (the form Go feeds to
//! `fstest.MapFS`). The rooted-access semantics (unrooted paths panic,
//! `/`-prefixed lookups, walk ordering) live in `MapFs`.

use std::sync::Arc;

use crate::vfstest::{Clock, MapFile, MapFs, SystemClock};

/// Builds an [`Fs`](crate::Fs) from relative-keyed entries.
///
/// For a path like `c:/foo/bar`, the backing store is treated as rooted at `/`,
/// so callers access it as `/c:/foo/bar` is access `c:/foo/bar` after the root
/// prefix is stripped.
///
/// # Examples
/// ```
/// use tsgo_vfs::Fs;
/// let fs = tsgo_vfs::iovfs::from([("foo.ts", "hello")], true);
/// assert_eq!(fs.read_file("/foo.ts").as_deref(), Some("hello"));
/// ```
///
/// Side effects: allocates the in-memory tree.
// Go: internal/vfs/iovfs/iofs.go:From
pub fn from<I, K, V>(entries: I, use_case_sensitive_file_names: bool) -> MapFs
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<MapFile>,
{
    from_with_clock(
        entries,
        use_case_sensitive_file_names,
        Arc::new(SystemClock::new()),
    )
}

/// Like [`from`] but with an explicit [`Clock`].
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_vfs::vfstest::SystemClock;
/// let fs = tsgo_vfs::iovfs::from_with_clock([("a.ts", "x")], true, Arc::new(SystemClock::new()));
/// let _ = fs;
/// ```
///
/// Side effects: allocates the in-memory tree.
// Go: internal/vfs/iovfs/iofs.go:From
pub fn from_with_clock<I, K, V>(
    entries: I,
    use_case_sensitive_file_names: bool,
    clock: Arc<dyn Clock>,
) -> MapFs
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<MapFile>,
{
    let seeds = entries
        .into_iter()
        .map(|(k, v)| {
            let (data, mode) = v.into().into_parts();
            (k.into(), data, mode)
        })
        .collect();
    MapFs::convert_map_fs(seeds, use_case_sensitive_file_names, clock)
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
