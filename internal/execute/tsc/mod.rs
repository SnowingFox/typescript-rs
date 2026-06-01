//! The `tsc` orchestration sub-package: exit-status / result types
//! ([`compile`]), diagnostic reporting ([`diagnostics`]), and the
//! emit-and-report pass ([`emit`]).
//!
//! Ports Go's `internal/execute/tsc` package (the helpers `internal/execute`
//! drives). There is no `tsc/tsc.go` in the Go tree, so this `mod.rs` is a
//! synthetic module root that declares the sub-modules and re-exports their
//! public surface; the package-level `internal/execute/tsc.go` orchestration
//! itself lives in the crate root (see the crate docs for the naming note).

mod compile;
mod diagnostics;
mod emit;

pub use compile::{CommandLineResult, CompileAndEmitResult, ExitStatus};
pub use diagnostics::{
    create_diagnostic_reporter, create_report_error_summary, sort_and_deduplicate_diagnostics,
    DiagFile, DiagnosticReporter, ReportErrorSummary, ReportedDiagnostic,
};
pub use emit::{emit_and_report_statistics, emit_files_and_report_errors};
