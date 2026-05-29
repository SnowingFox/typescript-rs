//! Compiler version string and its `major.minor` prefix.
//!
//! 1:1 port of Go `internal/core/version.go`.

use std::sync::LazyLock;

// The compiler version. In Go this is a `var` overridable via `-ldflags`;
// here it is overridable at build time via the `TSGO_VERSION` env var.
const VERSION: &str = match option_env!("TSGO_VERSION") {
    Some(v) => v,
    None => "7.0.0-dev",
};

/// Returns the full compiler version string (e.g. `"7.0.0-dev"`).
///
/// # Examples
/// ```
/// assert_eq!(tsgo_core::version::version(), "7.0.0-dev");
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/version.go:Version
pub fn version() -> &'static str {
    VERSION
}

// Computes the `major.minor` prefix by finding the second `.` in `v`.
//
// Side effects: panics if `v` contains fewer than two `.`-separated segments.
fn compute_major_minor(v: &str) -> &str {
    let mut seen_major = false;
    for (i, c) in v.char_indices() {
        if c == '.' {
            if seen_major {
                return &v[..i];
            }
            seen_major = true;
        }
    }
    panic!("invalid version string: {v}");
}

static VERSION_MAJOR_MINOR: LazyLock<&'static str> = LazyLock::new(|| compute_major_minor(VERSION));

/// Returns the `major.minor` prefix of [`version`] (e.g. `"7.0"`).
///
/// # Examples
/// ```
/// assert_eq!(tsgo_core::version::version_major_minor(), "7.0");
/// ```
///
/// Side effects: none at runtime (computed once); panics at first use if the
/// version string is malformed.
// Go: internal/core/version.go:VersionMajorMinor
pub fn version_major_minor() -> &'static str {
    &VERSION_MAJOR_MINOR
}

#[cfg(test)]
#[path = "version_test.rs"]
mod tests;
