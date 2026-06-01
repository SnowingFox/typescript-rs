//! The emit-and-report pass: collect diagnostics, emit outputs, report, and
//! compute the exit status.
//!
//! Ports the reachable subset of Go `internal/execute/tsc/emit.go`
//! (`EmitFilesAndReportErrors` + `EmitAndReportStatistics`). The
//! `CommandLineTesting` hooks, `--listFiles`/`--explainFiles` listing,
//! `--diagnostics` statistics, and tracing are deferred (later P9 chunks).

use tsgo_compiler::{EmitOptions, EmitResult, Program};
use tsgo_locale::Locale;
use tsgo_tsoptions::ParsedCommandLine;

use super::compile::{CompileAndEmitResult, ExitStatus};
use super::diagnostics::{
    sort_and_deduplicate_diagnostics, DiagnosticReporter, ReportErrorSummary, ReportedDiagnostic,
};
use crate::sys::System;

/// Collects the program's diagnostics, emits its outputs (unless
/// `--listFilesOnly`), reports every diagnostic, and returns the
/// [`CompileAndEmitResult`] with [`ExitStatus::Success`] (the diagnostics-driven
/// status is computed by [`emit_and_report_statistics`]).
///
/// The diagnostics gathered mirror Go's `GetDiagnosticsOfAnyProgram` reachable
/// subset: option-consistency diagnostics (global), per-file syntactic
/// diagnostics, the whole-program semantic diagnostics, and any emit
/// diagnostics. They are sorted and de-duplicated before reporting, matching
/// Go's `SortAndDeduplicateDiagnostics`.
///
/// DEFER(P9): `--listFiles`/`--explainFiles` listing, the `CommandLineTesting`
/// emit hooks, and tracing. blocked-by: the watch loop + `--diagnostics`
/// statistics chunks.
///
/// Side effects: emits output files through the program's host file system
/// (unless `--noEmit`/`--listFilesOnly`), and writes the reported diagnostics to
/// `sys`.
// Go: internal/execute/tsc/emit.go:EmitFilesAndReportErrors
pub fn emit_files_and_report_errors(
    sys: &dyn System,
    program: &mut Program,
    config: &ParsedCommandLine,
    report_diagnostic: &DiagnosticReporter,
    report_error_summary: &ReportErrorSummary,
    locale: &Locale,
) -> CompileAndEmitResult {
    let mut all_diagnostics: Vec<ReportedDiagnostic> = Vec::new();

    // Option-consistency diagnostics (global, no file).
    for options_diagnostic in program.options_diagnostics() {
        all_diagnostics.push(ReportedDiagnostic::from_options(options_diagnostic, locale));
    }

    // Per-file syntactic diagnostics.
    for file in program.source_files() {
        for diagnostic in file.diagnostics() {
            all_diagnostics.push(ReportedDiagnostic::from_parser(
                diagnostic,
                Some(file),
                locale,
            ));
        }
    }

    // Whole-program semantic diagnostics, attributed to the root source file.
    // DEFER(P9): per-file semantic attribution for multi-file programs.
    // blocked-by: the `tsgo_checker` `Diagnostic` carries no file back-pointer.
    let semantic = program.semantic_diagnostics();
    if !semantic.is_empty() {
        if let Some(root) = config.file_names().first().cloned() {
            if let Some(file) = program.get_source_file(&root) {
                for diagnostic in &semantic {
                    all_diagnostics.push(ReportedDiagnostic::from_checker(diagnostic, file));
                }
            }
        }
    }

    // Emit (skipped entirely under `--listFilesOnly`; `--noEmit` is handled
    // inside `Program::emit`, which sets `emit_skipped` and writes nothing).
    let list_files_only = program.options().list_files_only.is_true();
    let emit_result = if list_files_only {
        EmitResult {
            emit_skipped: true,
            ..Default::default()
        }
    } else {
        program.emit(EmitOptions::default())
    };

    // Emit diagnostics (global; none in the reachable subset).
    for diagnostic in &emit_result.diagnostics {
        all_diagnostics.push(ReportedDiagnostic::from_parser(diagnostic, None, locale));
    }

    let all_diagnostics = sort_and_deduplicate_diagnostics(all_diagnostics);
    for diagnostic in &all_diagnostics {
        report_diagnostic.report(sys, diagnostic);
    }
    report_error_summary.report(sys, &all_diagnostics);

    CompileAndEmitResult {
        diagnostics: all_diagnostics,
        emit_result,
        status: ExitStatus::Success,
    }
}

/// Runs [`emit_files_and_report_errors`] and computes the diagnostics-driven
/// exit status: outputs-skipped (`2`) when emit was skipped with diagnostics
/// present, outputs-generated (`1`) when diagnostics are present but outputs
/// were written, and success (`0`) otherwise.
///
/// DEFER(P9): the `--diagnostics`/`--extendedDiagnostics` statistics report.
/// blocked-by: the statistics/tracing chunk.
///
/// Side effects: same as [`emit_files_and_report_errors`].
// Go: internal/execute/tsc/emit.go:EmitAndReportStatistics
pub fn emit_and_report_statistics(
    sys: &dyn System,
    program: &mut Program,
    config: &ParsedCommandLine,
    report_diagnostic: &DiagnosticReporter,
    report_error_summary: &ReportErrorSummary,
    locale: &Locale,
) -> CompileAndEmitResult {
    let mut result = emit_files_and_report_errors(
        sys,
        program,
        config,
        report_diagnostic,
        report_error_summary,
        locale,
    );
    if result.status != ExitStatus::Success {
        // The compile exited early.
        return result;
    }

    if result.emit_result.emit_skipped && !result.diagnostics.is_empty() {
        result.status = ExitStatus::DiagnosticsPresentOutputsSkipped;
    } else if !result.diagnostics.is_empty() {
        result.status = ExitStatus::DiagnosticsPresentOutputsGenerated;
    }
    result
}
