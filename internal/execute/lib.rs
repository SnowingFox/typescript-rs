//! `tsgo_execute`: the `tsc` program-build orchestration and CLI driver.
//!
//! Ports Go's `internal/execute` package (the `tsc.go` build/emit
//! orchestration). This first P9 chunk implements the **single-project**
//! build/check/emit/report path: [`execute`] parses a command line, builds a
//! [`Program`](tsgo_compiler::Program), collects and reports diagnostics, emits
//! outputs (unless `--noEmit`), and returns the [`ExitStatus`].
//!
//! ## Layout note (mega-package split, PORTING.md §2)
//!
//! Go has both a `tsc.go` file in `package execute` and a `tsc/` sub-package.
//! Rust cannot have both a `tsc.rs` module file and a `tsc/` module directory,
//! so the package-level `tsc.go` orchestration (`CommandLine` /
//! `tscCompilation` / `performCompilation`) is ported here in the crate root
//! [`lib.rs`](self), while the `tsc/` sub-package (its `compile.go` /
//! `emit.go` / `diagnostics.go` helpers) maps to the [`tsc`] sub-module.
//!
//! ## Deferred (blocked-by, later P9 chunks)
//!
//! - `-b`/`--build` orchestration + project-reference build loop (uses P6-9a).
//! - `--watch` (the p9-watcher chunk).
//! - Incremental skip-emit reuse wiring (P6-9b primitives).
//! - The `cmd/tsgo` argv binary entry (p9-cmd; this chunk is the library `fn`).
//! - `tsconfig.json` discovery, `--project`, `--init`/`--version`/`--help`/
//!   `--showConfig`, and `--locale` (the message locale plumbing).
//! - Pretty colour styling beyond what [`tsgo_diagnosticwriter`] already
//!   renders, and `--listFiles`/`--diagnostics` statistics.

use std::sync::Arc;

use tsgo_compiler::{new_compiler_host, new_program, CompilerHost, ProgramOptions};
use tsgo_locale::Locale;
use tsgo_tsoptions::{
    parse_build_command_line, parse_command_line, ParseConfigHost, ParsedBuildCommandLine,
    ParsedCommandLine,
};
use tsgo_vfs::Fs;

pub mod build;
pub mod sys;
pub mod tsc;
pub mod watch;

pub use build::perform_build;
pub use sys::{System, VfsSystem};
pub use tsc::{
    create_diagnostic_reporter, create_report_error_summary, create_watch_status_reporter,
    emit_and_report_statistics, emit_files_and_report_errors, sort_and_deduplicate_diagnostics,
    CommandLineResult, CompileAndEmitResult, DiagFile, DiagnosticReporter, ExitStatus,
    ReportErrorSummary, ReportedDiagnostic, WatchStatusReporter,
};
pub use watch::perform_watch;

/// Parses `args` into a command line and runs the single-project build/check/
/// emit path, returning the process [`ExitStatus`] in a [`CommandLineResult`].
///
/// Mirrors Go's `CommandLine` for the reachable subset: the `-b`/`--build` and
/// `--watch` routings are deferred, so every invocation flows to
/// [`tsc_compilation`].
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_execute::{execute, ExitStatus, System, VfsSystem};
/// use tsgo_vfs::vfstest::MapFs;
/// use tsgo_vfs::Fs;
///
/// let fs: Arc<dyn Fs + Send + Sync> =
///     Arc::new(MapFs::from_map([("/p/index.ts", "const x: number = 1;\n")], true));
/// let sys = VfsSystem::new(fs.clone(), "/p", "/lib");
/// let result = execute(&sys, &["index.ts".to_string()]);
/// assert_eq!(result.status, ExitStatus::Success);
/// // A clean program emits its `.js` output.
/// assert!(fs.file_exists("/p/index.js"));
/// ```
///
/// Side effects: reads the program's files, emits outputs, and writes
/// diagnostics — all through `sys`.
// Go: internal/execute/tsc.go:CommandLine
pub fn execute(sys: &dyn System, args: &[String]) -> CommandLineResult {
    // `-b`/`--build` routes to the project-reference build orchestrator; every
    // other invocation flows to the single-project compilation path. The
    // `--watch` routing is still DEFER (blocked-by: the p9-watcher chunk).
    if let Some(first) = args.first() {
        match first.to_lowercase().as_str() {
            "-b" | "--b" | "-build" | "--build" => {
                let build_command = parse_build_args(sys, args);
                return perform_build(sys, build_command);
            }
            _ => {}
        }
    }
    let parsed = parse_args(sys, args);
    tsc_compilation(sys, parsed)
}

/// Parses a `tsc -b` command line through a [`SysParseConfigHost`] over `sys`.
///
/// The leading `-b`/`--build` flag is consumed by the build-options parser; the
/// remaining positional arguments become the projects to build.
///
/// Side effects: reads response files through `sys`'s file system.
// Go: internal/execute/tsc.go:CommandLine (tsoptions.ParseBuildCommandLine call)
fn parse_build_args(sys: &dyn System, args: &[String]) -> ParsedBuildCommandLine {
    let host = SysParseConfigHost {
        fs: sys.fs(),
        current_directory: sys.get_current_directory().to_string(),
    };
    parse_build_command_line(args, &host)
}

/// Runs the single-project compilation for an already-parsed command line:
/// reports up-front command-line errors (and exits `2`), otherwise builds and
/// compiles the program.
///
/// Mirrors Go's `tscCompilation` reachable subset. The config-file discovery,
/// `--init`/`--version`/`--help`/`--showConfig`, `--project`, `--watch`, and
/// incremental branches are deferred (see the crate docs).
///
/// Side effects: writes diagnostics to `sys`; on the build path, emits outputs.
// Go: internal/execute/tsc.go:tscCompilation
pub fn tsc_compilation(sys: &dyn System, command_line: ParsedCommandLine) -> CommandLineResult {
    let locale = run_locale(&command_line);
    let report_diagnostic =
        create_diagnostic_reporter(sys, &locale, command_line.compiler_options());

    if !command_line.errors().is_empty() {
        // Unrecoverable command-line errors (e.g. an unknown option): report them
        // and exit. Go: `len(commandLine.Errors) > 0`.
        for error in command_line.errors() {
            report_diagnostic.report(sys, &ReportedDiagnostic::from_parser(error, None, &locale));
        }
        return CommandLineResult {
            status: ExitStatus::DiagnosticsPresentOutputsSkipped,
        };
    }

    // `--watch` and `--listFilesOnly` cannot be combined.
    // Go: `commandLine.CompilerOptions().Watch.IsTrue() && ...ListFilesOnly.IsTrue()`.
    if command_line.compiler_options().watch.is_true()
        && command_line.compiler_options().list_files_only.is_true()
    {
        report_diagnostic.report(
            sys,
            &ReportedDiagnostic::from_compiler_message(
                &tsgo_diagnostics::OPTIONS_0_AND_1_CANNOT_BE_COMBINED,
                &["watch".to_string(), "listFilesOnly".to_string()],
                &locale,
            ),
        );
        return CommandLineResult {
            status: ExitStatus::DiagnosticsPresentOutputsSkipped,
        };
    }

    // DEFER(P9): `tsconfig.json` discovery, `--project`, `--init`/`--version`/
    // `--help`/`--showConfig`, and incremental. blocked-by: those chunks. The
    // reachable path either enters the watch loop or builds a single program
    // from the parsed command line.
    let report_error_summary =
        create_report_error_summary(sys, &locale, command_line.compiler_options());

    // `--watch`/`-w` routes to the watch loop instead of the one-shot build.
    // Go: `if configForCompilation.CompilerOptions().Watch.IsTrue()`.
    if command_line.compiler_options().watch.is_true() {
        return perform_watch(
            sys,
            command_line,
            &report_diagnostic,
            &report_error_summary,
            &locale,
        );
    }

    perform_compilation(
        sys,
        command_line,
        &report_diagnostic,
        &report_error_summary,
        &locale,
    )
}

/// Builds the program from `config`, runs the checks, emits (unless `--noEmit`),
/// reports diagnostics, and returns the computed [`ExitStatus`].
///
/// Mirrors Go's `performCompilation`: it constructs the compiler host and
/// program, then drives [`emit_and_report_statistics`]. Tracing and the
/// `CommandLineTesting` hooks are deferred.
///
/// Side effects: reads the program's files, emits outputs (through the host file
/// system), and writes the reported diagnostics to `sys`.
// Go: internal/execute/tsc.go:performCompilation
pub fn perform_compilation(
    sys: &dyn System,
    config: ParsedCommandLine,
    report_diagnostic: &DiagnosticReporter,
    report_error_summary: &ReportErrorSummary,
    locale: &Locale,
) -> CommandLineResult {
    let host: Arc<dyn CompilerHost> = Arc::new(new_compiler_host(
        sys.get_current_directory().to_string(),
        sys.fs(),
        sys.default_library_path().to_string(),
    ));
    let config = Arc::new(config);
    // The checker pool retains an `Rc` (single-threaded) program, so the program
    // must be built single-threaded (PORTING §6).
    let mut program = new_program(ProgramOptions {
        host,
        config: config.clone(),
        single_threaded: true,
    });
    let result = emit_and_report_statistics(
        sys,
        &mut program,
        config.as_ref(),
        report_diagnostic,
        report_error_summary,
        locale,
    );
    CommandLineResult {
        status: result.status,
    }
}

/// A [`ParseConfigHost`] adapter over a [`System`], so the `tsoptions` parser
/// can read response/config files and resolve relative paths.
struct SysParseConfigHost {
    fs: Arc<dyn Fs + Send + Sync>,
    current_directory: String,
}

impl ParseConfigHost for SysParseConfigHost {
    fn fs(&self) -> &dyn Fs {
        self.fs.as_ref()
    }
    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }
}

/// Parses `args` through a [`SysParseConfigHost`] over `sys`.
///
/// Side effects: reads response files through `sys`'s file system.
// Go: internal/execute/tsc.go:CommandLine (tsoptions.ParseCommandLine call)
fn parse_args(sys: &dyn System, args: &[String]) -> ParsedCommandLine {
    let host = SysParseConfigHost {
        fs: sys.fs(),
        current_directory: sys.get_current_directory().to_string(),
    };
    parse_command_line(args, &host)
}

/// The run's message locale.
///
/// DEFER(P9): honor the `--locale` option (Go's `commandLine.Locale()`).
/// blocked-by: the `--locale` option plumbing on `ParsedCommandLine`. The
/// reachable subset always uses `en`.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsedcommandline.go:ParsedCommandLine.Locale
fn run_locale(_config: &ParsedCommandLine) -> Locale {
    tsgo_locale::parse("en").expect("en locale is always available")
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
