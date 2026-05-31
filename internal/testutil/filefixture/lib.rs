//! `tsgo_testutil_filefixture` — 1:1 Rust port of Go
//! `internal/testutil/filefixture`.
//!
//! A small abstraction over a test fixture's contents, sourced either from a
//! file on disk (read lazily and cached) or from an in-memory string.
//!
//! # Harness shim (divergence from Go)
//!
//! Go's `SkipIfNotExist(testing.TB)` and `ReadFile(testing.TB)` take a test
//! handle and call `tb.Skipf` / `tb.Fatalf`. Rust libraries have no such handle,
//! so this port exposes [`Fixture::should_skip`] (the caller decides whether to
//! skip) and makes [`Fixture::read_file`] `panic!` on an I/O error (the analog
//! of `Fatalf`).

use std::sync::OnceLock;

/// A named test fixture whose contents can be read on demand.
///
/// Mirrors Go's `filefixture.Fixture` interface.
// Go: internal/testutil/filefixture/filefixture.go:Fixture
pub trait Fixture {
    /// The fixture's logical name.
    fn name(&self) -> &str;
    /// The fixture's path (a real filesystem path for file-backed fixtures, or
    /// an arbitrary label for string-backed ones).
    fn path(&self) -> &str;
    /// Reports whether the test should be skipped because the fixture's backing
    /// file does not exist. Always `false` for string-backed fixtures.
    fn should_skip(&self) -> bool;
    /// Reads the fixture's contents, panicking on an I/O error.
    fn read_file(&self) -> String;
}

/// A fixture backed by a file on disk; its contents (and any read error) are
/// read once and cached.
// Go: internal/testutil/filefixture/filefixture.go:fromFile
struct FromFile {
    name: String,
    path: String,
    contents: OnceLock<Result<String, String>>,
}

impl Fixture for FromFile {
    fn name(&self) -> &str {
        &self.name
    }
    fn path(&self) -> &str {
        &self.path
    }
    // Go's `fromFile.SkipIfNotExist` stats the path and skips when the stat
    // fails; here a missing/unreadable path makes `should_skip` true.
    fn should_skip(&self) -> bool {
        std::fs::metadata(&self.path).is_err()
    }
    fn read_file(&self) -> String {
        // Cache the contents (and any error) on first read, mirroring Go's
        // `sync.OnceValues`. The error is stringified so the cached value is
        // `Clone`/`Send`-friendly.
        let cached = self
            .contents
            .get_or_init(|| std::fs::read_to_string(&self.path).map_err(|err| err.to_string()));
        match cached {
            Ok(contents) => contents.clone(),
            Err(err) => panic!("Failed to read test fixture {:?}: {err}", self.path),
        }
    }
}

/// Returns a [`Fixture`] that reads `path` lazily, caching the contents and any
/// error on first read.
///
/// # Examples
/// ```
/// use tsgo_testutil_filefixture::from_file;
/// let f = from_file("missing", "/no/such/path");
/// assert!(f.should_skip());
/// ```
///
/// Side effects: none until [`Fixture::read_file`] / [`Fixture::should_skip`]
/// touch the filesystem.
// Go: internal/testutil/filefixture/filefixture.go:FromFile
pub fn from_file(name: &str, path: &str) -> Box<dyn Fixture> {
    Box::new(FromFile {
        name: name.to_string(),
        path: path.to_string(),
        contents: OnceLock::new(),
    })
}

/// A fixture backed by an in-memory string.
// Go: internal/testutil/filefixture/filefixture.go:fromString
struct FromString {
    name: String,
    path: String,
    contents: String,
}

impl Fixture for FromString {
    fn name(&self) -> &str {
        &self.name
    }
    fn path(&self) -> &str {
        &self.path
    }
    // Go's `fromString.SkipIfNotExist` is a no-op, so a string fixture is never
    // skipped.
    fn should_skip(&self) -> bool {
        false
    }
    fn read_file(&self) -> String {
        self.contents.clone()
    }
}

/// Returns a [`Fixture`] whose contents are the supplied `contents` string.
///
/// # Examples
/// ```
/// use tsgo_testutil_filefixture::from_string;
/// let f = from_string("greeting", "/virtual/greeting.txt", "hello");
/// assert_eq!(f.read_file(), "hello");
/// assert!(!f.should_skip());
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/filefixture/filefixture.go:FromString
pub fn from_string(name: &str, path: &str, contents: &str) -> Box<dyn Fixture> {
    Box::new(FromString {
        name: name.to_string(),
        path: path.to_string(),
        contents: contents.to_string(),
    })
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
