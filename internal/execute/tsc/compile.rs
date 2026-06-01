//! Core orchestration value types: the process [`ExitStatus`], the
//! [`CommandLineResult`] an `execute` run returns, and the
//! [`CompileAndEmitResult`] the emit/report pass produces.
//!
//! Ports the reachable subset of Go `internal/execute/tsc/compile.go`. The
//! `System` interface lives in the crate's [`sys`](crate::sys) module; the
//! `Watcher`/`CommandLineTesting`/`CompileTimes` facets are deferred with the
//! watch loop and `--diagnostics` statistics (later P9 chunks).

use tsgo_compiler::EmitResult;

use super::diagnostics::ReportedDiagnostic;

/// The process exit status of a `tsc` run.
///
/// Mirrors Go's `ExitStatus` iota exactly (0 = clean, 1/2 = diagnostics present
/// with outputs generated/skipped, 3/4 = invalid project / reference cycle,
/// 5 = not implemented). [`command_line`](crate::command_line) and
/// [`perform_compilation`](crate::perform_compilation) return these as the
/// process exit code.
///
/// # Examples
/// ```
/// use tsgo_execute::ExitStatus;
/// assert_eq!(ExitStatus::Success as i32, 0);
/// assert_eq!(ExitStatus::DiagnosticsPresentOutputsGenerated as i32, 1);
/// assert_eq!(ExitStatus::DiagnosticsPresentOutputsSkipped as i32, 2);
/// ```
///
/// Side effects: none (plain enum).
// Go: internal/execute/tsc/compile.go:ExitStatus
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum ExitStatus {
    /// No diagnostics: a clean build (Go `ExitStatusSuccess`).
    #[default]
    Success = 0,
    /// Diagnostics were present but outputs were still generated (Go
    /// `ExitStatusDiagnosticsPresent_OutputsGenerated`).
    DiagnosticsPresentOutputsGenerated = 1,
    /// Diagnostics were present and outputs were skipped, e.g. `--noEmit` (Go
    /// `ExitStatusDiagnosticsPresent_OutputsSkipped`).
    DiagnosticsPresentOutputsSkipped = 2,
    /// An invalid project caused outputs to be skipped (Go
    /// `ExitStatusInvalidProject_OutputsSkipped`).
    InvalidProjectOutputsSkipped = 3,
    /// A project-reference cycle caused outputs to be skipped (Go
    /// `ExitStatusProjectReferenceCycle_OutputsSkipped`).
    ProjectReferenceCycleOutputsSkipped = 4,
    /// A requested feature is not implemented (Go `ExitStatusNotImplemented`).
    NotImplemented = 5,
}

/// The result of a command-line `execute` run: the exit status to return to the
/// process.
///
/// This is the reachable subset of Go's `CommandLineResult`; the `Watcher`
/// field (returned only in watch mode) is deferred.
///
/// Side effects: none (plain data).
// Go: internal/execute/tsc/compile.go:CommandLineResult
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineResult {
    /// The exit status the build/check produced.
    pub status: ExitStatus,
}

/// The output of the emit-and-report pass: the (sorted, de-duplicated)
/// diagnostics that were reported, the compiler's emit result, and the computed
/// [`ExitStatus`].
///
/// This is the reachable subset of Go's `CompileAndEmitResult`; the
/// `CompileTimes` field is deferred with `--diagnostics` statistics.
///
/// Side effects: none (plain data; owns the emit result).
// Go: internal/execute/tsc/compile.go:CompileAndEmitResult
#[derive(Debug, Default)]
pub struct CompileAndEmitResult {
    /// The diagnostics reported by the pass, in sorted order.
    pub diagnostics: Vec<ReportedDiagnostic>,
    /// The compiler's emit result (which files were written / whether emit was
    /// skipped).
    pub emit_result: EmitResult,
    /// The computed exit status.
    pub status: ExitStatus,
}
