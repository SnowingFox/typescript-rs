//! Diagnostic reporting: a renderable diagnostic wrapper
//! ([`ReportedDiagnostic`]) plus the reporter factories that decide pretty vs.
//! plain output and write through the [`System`].
//!
//! Ports the reachable subset of Go `internal/execute/tsc/diagnostics.go`. The
//! wrapper is this crate's counterpart of Go's
//! `diagnosticwriter.WrapASTDiagnostic`: it adapts a parser/checker/options
//! diagnostic to the [`tsgo_diagnosticwriter`] `Diagnostic`/`FileLike` traits so
//! the exact Go rendering (`file(line,col): error TSxxxx: message`) is reused.
//!
//! DEFER(P9-watcher): the colour helpers (`createColors`/`bold`/`blue`) and the
//! builder/watch status reporters (`CreateBuilderStatusReporter`/
//! `CreateWatchStatusReporter`). blocked-by: the watch loop + `--build`
//! orchestration. The pretty error path is reached through
//! [`tsgo_diagnosticwriter`]'s existing renderers; bespoke colour styling
//! beyond that is deferred.

use std::time::{Duration, SystemTime};

use tsgo_compiler::{OptionsDiagnostic, ParsedFile};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::compute_ecma_line_starts;
use tsgo_core::text::TextPos;
use tsgo_diagnostics::{Category, Message};
use tsgo_diagnosticwriter::{
    format_diagnostic_with_color_and_context, format_diagnostics_status_and_time,
    format_diagnostics_status_with_color_and_time, write_error_summary_text,
    write_format_diagnostic, Diagnostic as DwDiagnostic, FileLike, FormattingOptions,
};
use tsgo_locale::Locale;
use tsgo_tspath::ComparePathsOptions;

use crate::sys::System;

/// A source file as seen by the diagnostic renderers: its name, full text, and
/// the ECMA line-start map derived from the text.
///
/// Mirrors how Go's wrapped `ast.Diagnostic` exposes its `FileLike`; the line
/// map is computed once from the text via
/// [`compute_ecma_line_starts`](tsgo_core::compute_ecma_line_starts).
///
/// Side effects: none (pure value type).
// Go: internal/diagnosticwriter/diagnosticwriter.go:FileLike
#[derive(Debug, Clone)]
pub struct DiagFile {
    file_name: String,
    text: String,
    line_map: Vec<TextPos>,
}

impl DiagFile {
    /// Builds a [`DiagFile`] from a file name and text, precomputing the ECMA
    /// line-start map.
    ///
    /// Side effects: none (allocates the line map).
    fn new(file_name: &str, text: &str) -> DiagFile {
        DiagFile {
            file_name: file_name.to_string(),
            text: text.to_string(),
            line_map: compute_ecma_line_starts(text),
        }
    }
}

impl FileLike for DiagFile {
    fn file_name(&self) -> &str {
        &self.file_name
    }
    fn text(&self) -> &str {
        &self.text
    }
    fn ecma_line_map(&self) -> &[TextPos] {
        &self.line_map
    }
}

/// A renderable diagnostic adapted from a parser, checker, or options
/// diagnostic.
///
/// This is the crate's counterpart of Go's wrapped `ast.Diagnostic`: it owns the
/// already-localized message, the source span, the optional owning [`DiagFile`],
/// and the nested message-chain / related-information sub-diagnostics, and
/// implements the [`tsgo_diagnosticwriter`] `Diagnostic` trait so the renderers
/// produce byte-identical output to Go.
///
/// DIVERGENCE(port): the port's parser/checker `Diagnostic` types carry no file
/// back-pointer, so the owning file is attached by the orchestration when it
/// knows the source file (per-file syntactic diagnostics; the single root file
/// for semantic diagnostics in the reachable single-file subset). Multi-file
/// semantic-diagnostic attribution is deferred (see [`from_checker`]).
///
/// Side effects: none (plain value type).
// Go: internal/diagnosticwriter/diagnosticwriter.go:Diagnostic (WrapASTDiagnostic)
#[allow(clippy::len_without_is_empty)]
#[derive(Debug, Clone)]
pub struct ReportedDiagnostic {
    /// The numeric diagnostic code (the `xxxx` in `TSxxxx`).
    pub code: i32,
    /// The diagnostic category (error/warning/...).
    pub category: Category,
    /// The already-localized, argument-substituted message text.
    pub message: String,
    /// The start byte offset of the span.
    pub pos: i32,
    /// The length in bytes of the span.
    pub len: i32,
    /// The owning source file, or `None` for a global (file-less) diagnostic.
    pub file: Option<DiagFile>,
    /// The nested elaboration chain (Go's `messageChain`).
    pub message_chain: Vec<ReportedDiagnostic>,
    /// The related-information sub-diagnostics (Go's `relatedInformation`).
    pub related_information: Vec<ReportedDiagnostic>,
}

impl ReportedDiagnostic {
    /// Wraps a parser (syntactic / command-line) [`Diagnostic`](tsgo_parser::Diagnostic),
    /// attaching `file` as the owning source file when known (`None` for a
    /// global command-line error).
    ///
    /// Side effects: none (allocates the wrapper + any line map).
    // Go: internal/diagnosticwriter/diagnosticwriter.go:WrapASTDiagnostic (parser diagnostic)
    pub fn from_parser(
        diagnostic: &tsgo_parser::Diagnostic,
        file: Option<&ParsedFile>,
        locale: &Locale,
    ) -> ReportedDiagnostic {
        let args: Vec<&str> = diagnostic.args.iter().map(String::as_str).collect();
        ReportedDiagnostic {
            code: diagnostic.message.code(),
            category: diagnostic.message.category(),
            message: diagnostic.message.localize(locale, &args),
            pos: diagnostic.loc.pos(),
            len: diagnostic.loc.end() - diagnostic.loc.pos(),
            file: file.map(|f| DiagFile::new(f.file_name(), f.text())),
            message_chain: Vec::new(),
            related_information: Vec::new(),
        }
    }

    /// Wraps a checker (semantic) [`Diagnostic`](tsgo_checker::Diagnostic),
    /// attaching `file` as the owning source file and recursively wrapping its
    /// message chain and related information.
    ///
    /// DEFER(P9): multi-file semantic-diagnostic attribution. blocked-by: the
    /// checker `Diagnostic` (in the `tsgo_checker` crate, not editable here)
    /// carries no file back-pointer, so the orchestration attributes semantic
    /// diagnostics to the single root source file (the reachable single-file
    /// subset).
    ///
    /// Side effects: none (allocates the wrapper + line map).
    // Go: internal/diagnosticwriter/diagnosticwriter.go:WrapASTDiagnostic (checker diagnostic)
    pub fn from_checker(
        diagnostic: &tsgo_checker::Diagnostic,
        file: &ParsedFile,
    ) -> ReportedDiagnostic {
        ReportedDiagnostic {
            code: diagnostic.code,
            category: diagnostic.category,
            message: diagnostic.message.clone(),
            pos: diagnostic.start,
            len: diagnostic.length,
            file: Some(DiagFile::new(file.file_name(), file.text())),
            message_chain: diagnostic
                .message_chain
                .iter()
                .map(Self::from_message_chain)
                .collect(),
            related_information: diagnostic
                .related_information
                .iter()
                .map(|related| Self::from_checker(related, file))
                .collect(),
        }
    }

    /// Wraps one node of a checker diagnostic's elaboration chain. Chain entries
    /// carry no span/file (Go's flattener uses only their message + nested
    /// children).
    ///
    /// Side effects: none (allocates the wrapper).
    // Go: internal/diagnosticwriter/diagnosticwriter.go:flattenDiagnosticMessageChain
    fn from_message_chain(chain: &tsgo_checker::DiagnosticMessageChain) -> ReportedDiagnostic {
        ReportedDiagnostic {
            code: chain.code,
            category: chain.category,
            message: chain.message.clone(),
            pos: 0,
            len: 0,
            file: None,
            message_chain: chain.next.iter().map(Self::from_message_chain).collect(),
            related_information: Vec::new(),
        }
    }

    /// Builds a global (file-less) compiler diagnostic from a static `message`
    /// and its `args`, localizing it with the run `locale`.
    ///
    /// Mirrors Go's `ast.NewCompilerDiagnostic` followed by
    /// `diagnosticwriter.WrapASTDiagnostic`: it is how the watch/builder status
    /// reporters turn a message constant (e.g. "Starting compilation in watch
    /// mode...") into a renderable diagnostic.
    ///
    /// Side effects: none (allocates the wrapper).
    // Go: internal/ast/diagnostic.go:NewCompilerDiagnostic
    pub fn from_compiler_message(
        message: &'static Message,
        args: &[String],
        locale: &Locale,
    ) -> ReportedDiagnostic {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        ReportedDiagnostic {
            code: message.code(),
            category: message.category(),
            message: message.localize(locale, &arg_refs),
            pos: 0,
            len: 0,
            file: None,
            message_chain: Vec::new(),
            related_information: Vec::new(),
        }
    }

    /// Wraps a compiler option-consistency [`OptionsDiagnostic`]. These are
    /// global (no file).
    ///
    /// Side effects: none (allocates the wrapper).
    // Go: internal/diagnosticwriter/diagnosticwriter.go:WrapASTDiagnostic (options diagnostic)
    pub fn from_options(diagnostic: &OptionsDiagnostic, locale: &Locale) -> ReportedDiagnostic {
        let args: Vec<&str> = diagnostic.args.iter().map(String::as_str).collect();
        ReportedDiagnostic {
            code: diagnostic.message.code(),
            category: diagnostic.message.category(),
            message: diagnostic.message.localize(locale, &args),
            pos: 0,
            len: 0,
            file: None,
            message_chain: Vec::new(),
            related_information: Vec::new(),
        }
    }
}

impl DwDiagnostic for ReportedDiagnostic {
    fn file(&self) -> Option<&dyn FileLike> {
        self.file.as_ref().map(|f| f as &dyn FileLike)
    }
    fn pos(&self) -> i32 {
        self.pos
    }
    fn end(&self) -> i32 {
        self.pos + self.len
    }
    fn len(&self) -> i32 {
        self.len
    }
    fn code(&self) -> i32 {
        self.code
    }
    fn category(&self) -> Category {
        self.category
    }
    fn localize(&self, _locale: &Locale) -> String {
        // DIVERGENCE(port): the message is already localized at wrap time (the
        // checker localizes eagerly, and the parser/options message is localized
        // with the run locale in `from_parser`/`from_options`). Re-localizing is
        // a no-op, so the stored text is returned verbatim.
        self.message.clone()
    }
    fn message_chain(&self) -> Vec<&dyn DwDiagnostic> {
        self.message_chain
            .iter()
            .map(|c| c as &dyn DwDiagnostic)
            .collect()
    }
    fn related_information(&self) -> Vec<&dyn DwDiagnostic> {
        self.related_information
            .iter()
            .map(|r| r as &dyn DwDiagnostic)
            .collect()
    }
}

/// Builds the [`FormattingOptions`] from the system (newline, path comparison,
/// locale), mirroring Go's `getFormatOptsOfSys`.
///
/// Side effects: none (reads the system's directory + case sensitivity).
// Go: internal/execute/tsc/diagnostics.go:getFormatOptsOfSys
fn get_format_opts_of_sys(sys: &dyn System, locale: &Locale) -> FormattingOptions {
    FormattingOptions {
        new_line: "\n".to_string(),
        compare_paths_options: ComparePathsOptions {
            current_directory: sys.get_current_directory().to_string(),
            use_case_sensitive_file_names: sys.fs().use_case_sensitive_file_names(),
        },
        locale: locale.clone(),
    }
}

/// Reports whether colour output is the default for `sys`: `NO_COLOR` disables
/// it, `FORCE_COLOR` enables it, otherwise it follows whether the output is a
/// TTY.
///
/// Side effects: none (reads the system environment / TTY flag).
// Go: internal/execute/tsc/diagnostics.go:defaultIsPretty
fn default_is_pretty(sys: &dyn System) -> bool {
    if !sys.get_environment_variable("NO_COLOR").is_empty() {
        return false;
    }
    if !sys.get_environment_variable("FORCE_COLOR").is_empty() {
        return true;
    }
    sys.write_output_is_tty()
}

/// Reports whether diagnostics should be rendered in pretty (colour + context)
/// form: the explicit `--pretty` option wins, otherwise the default is used.
///
/// Side effects: none.
// Go: internal/execute/tsc/diagnostics.go:shouldBePretty
fn should_be_pretty(sys: &dyn System, options: &CompilerOptions) -> bool {
    if options.pretty.is_unknown() {
        default_is_pretty(sys)
    } else {
        options.pretty.is_true()
    }
}

/// A reporter that renders a single diagnostic to the system writer.
///
/// Mirrors Go's `DiagnosticReporter` closure: it is created once for a run
/// (capturing the quiet/pretty decision and formatting options) and called for
/// each diagnostic.
///
/// Side effects: [`report`](DiagnosticReporter::report) writes to the system.
// Go: internal/execute/tsc/diagnostics.go:DiagnosticReporter
pub struct DiagnosticReporter {
    quiet: bool,
    pretty: bool,
    format_opts: FormattingOptions,
}

impl DiagnosticReporter {
    /// Renders `diagnostic` to `sys`'s output: nothing when quiet, the pretty
    /// colour+context form when pretty, otherwise the compact one-line form.
    ///
    /// Side effects: writes the rendered diagnostic to `sys`.
    // Go: internal/execute/tsc/diagnostics.go:CreateDiagnosticReporter (the returned closure)
    pub fn report(&self, sys: &dyn System, diagnostic: &ReportedDiagnostic) {
        if self.quiet {
            return;
        }
        let mut out = String::new();
        if self.pretty {
            format_diagnostic_with_color_and_context(&mut out, diagnostic, &self.format_opts);
            out.push_str(&self.format_opts.new_line);
        } else {
            write_format_diagnostic(&mut out, diagnostic, &self.format_opts);
        }
        sys.write(&out);
    }
}

/// Creates the per-diagnostic reporter for a run, deciding quiet/pretty from the
/// options and system.
///
/// Side effects: none (reads the system; the returned reporter writes on use).
// Go: internal/execute/tsc/diagnostics.go:CreateDiagnosticReporter
pub fn create_diagnostic_reporter(
    sys: &dyn System,
    locale: &Locale,
    options: &CompilerOptions,
) -> DiagnosticReporter {
    DiagnosticReporter {
        quiet: options.quiet.is_true(),
        pretty: should_be_pretty(sys, options),
        format_opts: get_format_opts_of_sys(sys, locale),
    }
}

/// A reporter that renders the trailing `Found N errors …` summary.
///
/// Mirrors Go's `DiagnosticsReporter`: in plain mode it is a no-op (the summary
/// is only printed in pretty mode), so the reachable plain path emits no
/// summary line — matching `tsc` piped/non-TTY output.
///
/// Side effects: [`report`](ReportErrorSummary::report) writes to the system in
/// pretty mode.
// Go: internal/execute/tsc/diagnostics.go:DiagnosticsReporter
pub struct ReportErrorSummary {
    pretty: bool,
    format_opts: FormattingOptions,
}

impl ReportErrorSummary {
    /// A reporter that never writes anything, used per-project by the `--build`
    /// orchestrator (which emits a single summary over all projects at the end).
    ///
    /// Side effects: none (construction); [`report`](Self::report) is a no-op.
    // Go: internal/execute/tsc/diagnostics.go:QuietDiagnosticsReporter
    pub fn quiet() -> ReportErrorSummary {
        ReportErrorSummary {
            pretty: false,
            format_opts: FormattingOptions {
                locale: Locale::default(),
                compare_paths_options: ComparePathsOptions::default(),
                new_line: "\n".to_string(),
            },
        }
    }

    /// Writes the localized error summary (and, for multiple erroring files, the
    /// per-file table) in pretty mode; a no-op in plain mode.
    ///
    /// Side effects: writes the summary to `sys` in pretty mode.
    // Go: internal/execute/tsc/diagnostics.go:CreateReportErrorSummary (the returned closure)
    pub fn report(&self, sys: &dyn System, diagnostics: &[ReportedDiagnostic]) {
        if !self.pretty {
            return;
        }
        let refs: Vec<&dyn DwDiagnostic> =
            diagnostics.iter().map(|d| d as &dyn DwDiagnostic).collect();
        let mut out = String::new();
        write_error_summary_text(&mut out, &refs, &self.format_opts);
        sys.write(&out);
    }
}

/// Creates the error-summary reporter for a run.
///
/// Side effects: none (reads the system; the returned reporter writes on use).
// Go: internal/execute/tsc/diagnostics.go:CreateReportErrorSummary
pub fn create_report_error_summary(
    sys: &dyn System,
    locale: &Locale,
    options: &CompilerOptions,
) -> ReportErrorSummary {
    ReportErrorSummary {
        pretty: should_be_pretty(sys, options),
        format_opts: get_format_opts_of_sys(sys, locale),
    }
}

/// A reporter that renders a time-stamped watch-mode status line
/// (`HH:MM:SS PM - <message>`), used for the watch loop's lifecycle messages
/// ("Starting compilation in watch mode...", "File change detected...",
/// "Found N errors. Watching for file changes.").
///
/// Mirrors Go's `CreateWatchStatusReporter` returned closure. Unlike the error
/// summary, the watch status is written in both plain and pretty modes (only
/// the formatting differs); it is not gated by `--quiet`.
///
/// DEFER(P9): the `TryClearScreen` screen-clearing the pretty watch UI does
/// before each fresh compilation, and the `CommandLineTesting`
/// `OnWatchStatusReportStart`/`End` hooks. blocked-by: the pretty watch UI +
/// the testing-hook chunk.
///
/// Side effects: [`report`](WatchStatusReporter::report) writes to the system.
// Go: internal/execute/tsc/diagnostics.go:CreateWatchStatusReporter
pub struct WatchStatusReporter {
    pretty: bool,
    format_opts: FormattingOptions,
}

impl WatchStatusReporter {
    /// Renders `diagnostic` as a time-stamped status line (stamped with
    /// `sys.now()`), followed by a blank line, matching Go's
    /// `writeStatus(...); fmt.Fprint(writer, newLine, newLine)`.
    ///
    /// Side effects: reads `sys.now()` and writes the status line to `sys`.
    // Go: internal/execute/tsc/diagnostics.go:CreateWatchStatusReporter (the returned closure)
    pub fn report(&self, sys: &dyn System, diagnostic: &ReportedDiagnostic) {
        let time = format_status_time(sys.now());
        let mut out = String::new();
        // DEFER(P9): `diagnosticwriter.TryClearScreen` runs here in Go before
        // the status line; the screen-clearing pretty watch UI is deferred.
        if self.pretty {
            format_diagnostics_status_with_color_and_time(
                &mut out,
                &time,
                diagnostic,
                &self.format_opts,
            );
        } else {
            format_diagnostics_status_and_time(&mut out, &time, diagnostic, &self.format_opts);
        }
        out.push_str(&self.format_opts.new_line);
        out.push_str(&self.format_opts.new_line);
        sys.write(&out);
    }
}

/// Creates the watch-mode status reporter for a run, deciding plain/pretty from
/// the options and system.
///
/// Side effects: none (reads the system; the returned reporter writes on use).
// Go: internal/execute/tsc/diagnostics.go:CreateWatchStatusReporter
pub fn create_watch_status_reporter(
    sys: &dyn System,
    locale: &Locale,
    options: &CompilerOptions,
) -> WatchStatusReporter {
    WatchStatusReporter {
        pretty: should_be_pretty(sys, options),
        format_opts: get_format_opts_of_sys(sys, locale),
    }
}

/// Formats `time` as Go's `03:04:05 PM` (zero-padded 12-hour clock), computed
/// from the UTC time-of-day so it needs no calendar/timezone dependency.
///
/// Shared by the watch ([`WatchStatusReporter`]) and `--build` status
/// reporters, both of which stamp their lines with `sys.Now().Format(...)`.
///
/// Side effects: none (pure).
// Go: internal/execute/tsc/diagnostics.go:CreateWatchStatusReporter (sys.Now().Format)
pub(crate) fn format_status_time(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let secs_of_day = (secs % 86_400) as u32;
    let hour24 = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    let period = if hour24 < 12 { "AM" } else { "PM" };
    let hour12 = match hour24 % 12 {
        0 => 12,
        h => h,
    };
    format!("{hour12:02}:{minute:02}:{second:02} {period}")
}

/// Sorts diagnostics into stable report order and removes exact duplicates.
///
/// Mirrors Go's `compiler.SortAndDeduplicateDiagnostics`: the reachable order is
/// by file name, then start position, then length, then code, then message; two
/// diagnostics that match on all of those are considered duplicates.
///
/// # Examples
/// ```
/// use tsgo_execute::ReportedDiagnostic;
/// use tsgo_diagnostics::Category;
///
/// let make = |code: i32, pos: i32| ReportedDiagnostic {
///     code,
///     category: Category::Error,
///     message: "m".into(),
///     pos,
///     len: 1,
///     file: None,
///     message_chain: Vec::new(),
///     related_information: Vec::new(),
/// };
/// let sorted = tsgo_execute::sort_and_deduplicate_diagnostics(vec![make(2, 5), make(1, 1), make(1, 1)]);
/// assert_eq!(sorted.len(), 2);
/// assert_eq!(sorted[0].pos, 1);
/// assert_eq!(sorted[1].pos, 5);
/// ```
///
/// Side effects: none (pure).
// Go: internal/compiler/diagnostics.go:SortAndDeduplicateDiagnostics
pub fn sort_and_deduplicate_diagnostics(
    mut diagnostics: Vec<ReportedDiagnostic>,
) -> Vec<ReportedDiagnostic> {
    diagnostics.sort_by(compare_diagnostics);
    diagnostics.dedup_by(|a, b| diagnostics_equal(a, b));
    diagnostics
}

// Orders two diagnostics by (file name, start, length, code, message).
// Go: internal/compiler/diagnostics.go:compareDiagnostics
fn compare_diagnostics(a: &ReportedDiagnostic, b: &ReportedDiagnostic) -> std::cmp::Ordering {
    let a_file = a.file.as_ref().map(|f| f.file_name.as_str()).unwrap_or("");
    let b_file = b.file.as_ref().map(|f| f.file_name.as_str()).unwrap_or("");
    a_file
        .cmp(b_file)
        .then(a.pos.cmp(&b.pos))
        .then(a.len.cmp(&b.len))
        .then(a.code.cmp(&b.code))
        .then(a.message.cmp(&b.message))
}

// Reports whether two diagnostics are exact duplicates for de-duplication.
// Go: internal/compiler/diagnostics.go:equalDiagnostics
fn diagnostics_equal(a: &ReportedDiagnostic, b: &ReportedDiagnostic) -> bool {
    compare_diagnostics(a, b) == std::cmp::Ordering::Equal
}

#[cfg(test)]
#[path = "diagnostics_test.rs"]
mod tests;
