//! `tsgo_testutil` — shared test-utility helpers.
//!
//! 1:1 port of Go `internal/testutil/testutil.go`.
//!
//! Provides [`assert_panics`] (verify that a closure panics with an expected
//! message) and [`test_program_is_single_threaded`] (honour the
//! `TS_TEST_PROGRAM_SINGLE_THREADED` env var to disable multi-threaded
//! compilation in tests).

use std::any::Any;
use std::sync::LazyLock;

/// Asserts that `f` panics and the panic payload is a `&str` equal to
/// `expected`.
///
/// # Panics
///
/// Panics with a descriptive message if `f` does not panic, or panics with a
/// different payload.
///
/// # Examples
/// ```
/// use tsgo_testutil::assert_panics;
/// assert_panics(|| panic!("oh no"), "oh no");
/// ```
// Go: internal/testutil/testutil.go:AssertPanics
pub fn assert_panics(f: impl FnOnce() + std::panic::UnwindSafe, expected: &str) {
    let result = std::panic::catch_unwind(f);
    match result {
        Ok(()) => panic!("did not panic (expected: {:?})", expected),
        Err(payload) => {
            let msg = payload_to_string(&payload);
            assert!(
                msg == expected,
                "panic value mismatch: got {:?}, expected {:?}",
                msg,
                expected,
            );
        }
    }
}

fn payload_to_string(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    format!("{:?}", payload)
}

/// Whether the program should run in single-threaded mode for tests.
///
/// Reads the `TS_TEST_PROGRAM_SINGLE_THREADED` environment variable. If set
/// and parseable as a boolean, that wins. Otherwise defaults to
/// `!race::ENABLED` (single-threaded unless the race detector is active).
///
/// This is computed once via [`LazyLock`] and cached for the process lifetime.
// Go: internal/testutil/testutil.go:TestProgramIsSingleThreaded
pub fn test_program_is_single_threaded() -> bool {
    static CACHED: LazyLock<bool> = LazyLock::new(compute_test_program_is_single_threaded);
    *CACHED
}

/// The raw computation behind [`test_program_is_single_threaded`], exposed so
/// tests can call it directly without the `LazyLock` caching.
// Go: internal/testutil/testutil.go:testProgramIsSingleThreaded (sync.OnceValue body)
pub fn compute_test_program_is_single_threaded() -> bool {
    if let Ok(v) = std::env::var("TS_TEST_PROGRAM_SINGLE_THREADED") {
        if !v.is_empty() {
            if let Ok(b) = v.parse::<bool>() {
                return b;
            }
        }
    }
    !tsgo_testutil_race::ENABLED
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
