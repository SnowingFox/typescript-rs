//! `tsgo_testutil_race` — reports whether the race detector is enabled.
//!
//! 1:1 port of Go `internal/testutil/race` (`race.go` + `norace.go`).
//!
//! ## Build-tag -> cfg divergence
//!
//! Go selects [`ENABLED`] via the `//go:build race` / `//go:build !race`
//! build tags, which `go test -race` toggles. Rust has no `-race` build tag,
//! so there is no exact equivalent. We mirror the two-valued, compile-time
//! behavior with an opt-in `race` Cargo feature:
//!
//! * default build (feature off) -> `false`, matching `norace.go`;
//! * `--features race` -> `true`, matching `race.go`.
//!
//! This is a deliberate, documented divergence: the *observable* contract
//! (`enabled() -> bool`, resolved at compile time) is preserved, but the
//! mechanism that flips it differs from Go's race-instrumentation tag.

/// Whether the race detector is enabled, resolved at compile time.
///
/// `true` when built with the `race` feature, otherwise `false`. See the
/// module-level docs for the build-tag -> cfg divergence.
// Go: internal/testutil/race/race.go:Enabled (+ norace.go)
#[cfg(feature = "race")]
pub const ENABLED: bool = true;

/// Whether the race detector is enabled, resolved at compile time.
///
/// `true` when built with the `race` feature, otherwise `false`. See the
/// module-level docs for the build-tag -> cfg divergence.
// Go: internal/testutil/race/norace.go:Enabled
#[cfg(not(feature = "race"))]
pub const ENABLED: bool = false;

/// Reports whether the race detector is enabled.
///
/// Accessor form of [`ENABLED`], mirroring how Go callers read the package's
/// `Enabled` constant.
///
/// # Examples
/// ```
/// use tsgo_testutil_race::enabled;
/// // Default build has no `race` feature, so this is `false`.
/// assert_eq!(enabled(), cfg!(feature = "race"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/race/race.go:Enabled
pub fn enabled() -> bool {
    ENABLED
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
