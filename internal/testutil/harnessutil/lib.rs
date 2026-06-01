//! `tsgo_testutil_harnessutil`: the compiler test-harness infrastructure.
//!
//! Ports Go's `internal/testutil/harnessutil` (compile a test case through the
//! compiler, record outputs, and produce baseline-comparable results).
//!
//! This P10 foundation round ports the reachable subset:
//! - [`recorderfs`]: the [`OutputRecorderFs`] that records emitted files.
//! - [`harnessutil`]: [`compile_files`] / [`compile_files_ex`] (build a
//!   [`tsgo_compiler::Program`] over a `MapFs` + bundled libs, collect
//!   diagnostics, emit) and the [`CompilationResult`] bundle.
//!
//! DEFER(P10): the source-map record/baseline, declaration & suggestion
//! diagnostics, `.types`/`.symbols` baselines, in-test `tsconfig.json`,
//! symlinks, and `@libFiles`. blocked-by: the language-service type writer (P7),
//! declaration emit, and VFS symlink/config-host wiring (later P10 rounds).

mod harnessutil;
mod recorderfs;

pub use harnessutil::*;
pub use recorderfs::*;
