//! `tsgo_testrunner`: the compiler/conformance test runner.
//!
//! Ports Go's `internal/testrunner` (the conformance/compiler test-case parser
//! and runner that drives `harnessutil` over the `tests/cases` corpus and
//! compares against committed baselines).
//!
//! This P10 foundation round ports the reachable subset:
//! - [`test_case_parser`]: the `// @option:` / `// @filename:` test-file parser
//!   (pure string logic, fully reachable).
//! - [`compiler_runner`]: drives the parser through `harnessutil` and produces
//!   the `.errors.txt` baseline string (the formatted diagnostics).
//! - [`runner`]: the `Runner` trait shared by the (future) suite runners.
//!
//! DEFER(P10): the full `tests/cases` corpus walk + baseline comparison, the
//! module/target variation matrix, `.types`/`.symbols`/`.js` baselines, and
//! fourslash. blocked-by: the language-service type writer (P7), declaration
//! emit, and the separate fourslash crate.

mod compiler_runner;
mod failure_category;
mod runner;
mod test_case_parser;

pub use compiler_runner::*;
pub use failure_category::*;
pub use runner::*;
pub use test_case_parser::*;

/// Posix-style path the test sources live under in the harness file system.
///
/// Side effects: none (constant).
// Go: internal/testrunner/compiler_runner.go:srcFolder
pub(crate) const SRC_FOLDER: &str = "/.src";
