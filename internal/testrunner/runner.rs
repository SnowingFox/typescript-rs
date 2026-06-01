//! Port of Go `internal/testrunner/runner.go`: the [`Runner`] trait every test
//! suite runner implements.

/// A test suite runner: enumerates its test files and runs them.
///
/// DIVERGENCE(port): Go's `RunTests(t *testing.T)` threads the Go testing
/// handle through to accumulate failures and spawn sub-tests. The Rust port has
/// no library-level testing handle, so a runner instead accumulates failures
/// into the baseline [`Harness`](tsgo_testutil_harnessutil) the caller inspects;
/// the concrete signature lands with the corpus-walking runner (a later P10
/// round).
///
/// Side effects: implementations read test files and write baselines.
// Go: internal/testrunner/runner.go:Runner
pub trait Runner {
    /// Returns the absolute paths of the test files this runner owns.
    ///
    /// Side effects: reads the test-case directory.
    // Go: internal/testrunner/runner.go:Runner.EnumerateTestFiles
    fn enumerate_test_files(&self) -> Vec<String>;
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod tests;
