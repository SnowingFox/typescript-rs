use super::*;

// Go: internal/testutil/race/race.go:Enabled (+ norace.go)
//
// Build-tag -> cfg divergence: Go selects the `Enabled` constant via the
// `//go:build race` / `//go:build !race` build tags (set by `go test -race`).
// Rust has no `-race` build tag, so we mirror the two-valued behavior with the
// `race` Cargo feature. The default build (feature off) matches `norace.go`
// (`Enabled = false`); building with `--features race` matches `race.go`.

/// The reported flag must agree with the cfg-selected compile-time value.
#[test]
fn enabled_matches_cfg_selected_value() {
    if cfg!(feature = "race") {
        // Mirrors race.go: `Enabled = true`.
        assert!(enabled());
    } else {
        // Mirrors norace.go: `Enabled = false` (the default).
        assert!(!enabled());
    }
}

/// The `enabled()` accessor must report exactly the `ENABLED` constant, so the
/// const form and the Go-style accessor never drift apart.
#[test]
fn const_and_accessor_agree() {
    assert_eq!(enabled(), ENABLED);
}
