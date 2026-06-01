//! The `--watch` loop: the initial build plus the change-driven rebuild cycle.
//!
//! Ports the reachable subset of Go `internal/execute/watcher.go` (the
//! `Watcher` struct + `start`/`DoCycle`/`doBuild`). The loop is driven through
//! the [`System::wait_for_change`](crate::sys::System::wait_for_change) seam so
//! it is unit-testable without a real OS file-watcher: a fake system reports a
//! finite sequence of changes and the loop terminates deterministically.
//!
//! ## Deferred (blocked-by, later P9 chunks)
//!
//! - The real OS file-watching backend (`vfswatch.FileWatcher`: poll interval,
//!   watched-file/wildcard-directory state, debounce). blocked-by: the
//!   `internal/vfs/vfswatch` package is not yet ported.
//! - Ctrl-C / signal handling that ends the production loop. blocked-by: signal
//!   handling.
//! - `--watch -b` (build-mode watch with downstream tracking). blocked-by: the
//!   `--build` watch chunk.
//! - Incremental skip-emit reuse across cycles (Go reuses the prior
//!   `incremental.Program` + a source-file cache keyed by mtime). blocked-by:
//!   more of P6-9b; each cycle here rebuilds the program from the config.
//! - `recheckTsConfig` (re-reading a changed `tsconfig.json`). blocked-by:
//!   `tsconfig.json` discovery in `tsc_compilation`.

use std::sync::Arc;

use tsgo_compiler::{new_compiler_host, new_program, CompilerHost, ProgramOptions};
use tsgo_diagnostics::{
    FILE_CHANGE_DETECTED_STARTING_INCREMENTAL_COMPILATION,
    FOUND_0_ERRORS_WATCHING_FOR_FILE_CHANGES, FOUND_1_ERROR_WATCHING_FOR_FILE_CHANGES,
    STARTING_COMPILATION_IN_WATCH_MODE,
};
use tsgo_locale::Locale;
use tsgo_tsoptions::ParsedCommandLine;

use crate::sys::System;
use crate::tsc::{
    create_watch_status_reporter, emit_and_report_statistics, CommandLineResult,
    DiagnosticReporter, ExitStatus, ReportErrorSummary, ReportedDiagnostic, WatchStatusReporter,
};

/// Runs the watch loop: an initial build/report, then a rebuild for each change
/// the system reports, until [`System::wait_for_change`] signals no more
/// changes. Always returns [`ExitStatus::Success`] (Go returns the watcher in
/// `CommandLineResult`; the run's exit status is success once watching starts).
///
/// Mirrors Go's `createWatcher(...).start()`.
///
/// Side effects: builds + emits the program (through `sys`'s file system) on
/// the initial build and every change cycle, and writes diagnostics + watch
/// status lines to `sys`.
// Go: internal/execute/tsc.go:tscCompilation (the watch branch) + watcher.go:Watcher.start
pub fn perform_watch(
    sys: &dyn System,
    config: ParsedCommandLine,
    report_diagnostic: &DiagnosticReporter,
    report_error_summary: &ReportErrorSummary,
    locale: &Locale,
) -> CommandLineResult {
    let report_watch_status = create_watch_status_reporter(sys, locale, config.compiler_options());
    let mut watcher = Watcher {
        sys,
        config: Arc::new(config),
        report_diagnostic,
        report_error_summary,
        report_watch_status,
        locale,
    };
    watcher.start();
    // Go: `return tsc.CommandLineResult{Status: tsc.ExitStatusSuccess, Watcher: watcher}`.
    CommandLineResult {
        status: ExitStatus::Success,
    }
}

/// The reachable subset of Go's `Watcher`: it owns the run's reporters and
/// config, runs the initial build, and reruns the build for each change the
/// [`System`] reports.
///
/// DEFER(P9): the incremental program reuse + source-file cache, the
/// `vfswatch.FileWatcher` state, `recheckTsConfig`, and the
/// `CommandLineTesting` `OnProgram` hook. blocked-by: more of P6-9b + config
/// discovery + the testing-hook chunk; each cycle rebuilds from the config.
// Go: internal/execute/watcher.go:Watcher
struct Watcher<'a> {
    sys: &'a dyn System,
    config: Arc<ParsedCommandLine>,
    report_diagnostic: &'a DiagnosticReporter,
    report_error_summary: &'a ReportErrorSummary,
    report_watch_status: WatchStatusReporter,
    locale: &'a Locale,
}

impl Watcher<'_> {
    /// Reports the watch-start status, runs the initial build, then loops:
    /// waits for a change and runs a rebuild cycle, until the system signals no
    /// more changes.
    // Go: internal/execute/watcher.go:Watcher.start (+ fileWatcher.Run loop)
    fn start(&mut self) {
        self.report_watch_status(&STARTING_COMPILATION_IN_WATCH_MODE, &[]);
        self.do_build();
        // Go runs `w.fileWatcher.Run(w.sys.Now)`, which polls and calls
        // `w.DoCycle` per change. Here the loop is driven through the testable
        // `wait_for_change` seam (the production OS file-watcher is DEFER).
        while self.sys.wait_for_change() {
            self.do_cycle();
        }
    }

    /// Runs one change cycle: reports the "File change detected..." status, then
    /// rebuilds.
    // Go: internal/execute/watcher.go:Watcher.DoCycle
    fn do_cycle(&mut self) {
        self.report_watch_status(&FILE_CHANGE_DETECTED_STARTING_INCREMENTAL_COMPILATION, &[]);
        self.do_build();
    }

    /// Builds a fresh program from the config, emits + reports its diagnostics,
    /// then reports the trailing "Found N error(s). Watching for file changes."
    /// status.
    ///
    /// DEFER(P9): Go reuses the prior `incremental.Program` and an mtime-keyed
    /// source-file cache + tracks watched files/wildcard directories here; this
    /// reachable subset rebuilds the program from scratch each cycle (a fresh
    /// uncached host so on-disk edits are picked up).
    // Go: internal/execute/watcher.go:Watcher.doBuild
    fn do_build(&mut self) {
        let host: Arc<dyn CompilerHost> = Arc::new(new_compiler_host(
            self.sys.get_current_directory().to_string(),
            self.sys.fs(),
            self.sys.default_library_path().to_string(),
        ));
        // The checker pool retains an `Rc` (single-threaded) program, so build
        // single-threaded (PORTING §6), matching `perform_compilation`.
        let mut program = new_program(ProgramOptions {
            host,
            config: self.config.clone(),
            single_threaded: true,
        });
        let result = emit_and_report_statistics(
            self.sys,
            &mut program,
            self.config.as_ref(),
            self.report_diagnostic,
            self.report_error_summary,
            self.locale,
        );

        // Go: `errorCount := len(result.Diagnostics)`; the "1 error" message has
        // no placeholder, every other count uses the "{0} errors" message.
        let error_count = result.diagnostics.len();
        if error_count == 1 {
            self.report_watch_status(&FOUND_1_ERROR_WATCHING_FOR_FILE_CHANGES, &[]);
        } else {
            self.report_watch_status(
                &FOUND_0_ERRORS_WATCHING_FOR_FILE_CHANGES,
                &[error_count.to_string()],
            );
        }
    }

    /// Builds a global compiler diagnostic from `message`/`args` and renders it
    /// through the watch status reporter.
    // Go: internal/execute/watcher.go:Watcher.reportWatchStatus (ast.NewCompilerDiagnostic)
    fn report_watch_status(&self, message: &'static tsgo_diagnostics::Message, args: &[String]) {
        let diagnostic = ReportedDiagnostic::from_compiler_message(message, args, self.locale);
        self.report_watch_status.report(self.sys, &diagnostic);
    }
}

#[cfg(test)]
#[path = "watch_test.rs"]
mod tests;
