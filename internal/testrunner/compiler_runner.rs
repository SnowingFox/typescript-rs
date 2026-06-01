//! Port of Go `internal/testrunner/compiler_runner.go` (reachable subset) plus
//! the `.errors.txt` baseline producer ported from
//! `internal/testutil/tsbaseline/error_baseline.go`.
//!
//! Drives a parsed test case through `harnessutil` and produces the
//! `.errors.txt` baseline string (the compact top-of-file diagnostic list
//! followed by per-file source + squiggle sections). The full corpus walk, the
//! module/target variation matrix, and the `.js`/`.types`/`.symbols` baselines
//! are deferred (see the crate-level `DEFER`); this round wires
//! parser → compile → error baseline so tsc parity can be measured on inline
//! cases.

use std::cmp::Ordering;

use tsgo_diagnosticwriter::{
    flatten_diagnostic_message, write_format_diagnostics, Diagnostic as DiagnosticTrait,
    FormattingOptions,
};
use tsgo_locale::Locale;
use tsgo_testutil_harnessutil::{compile_files, HarnessDiagnostic, TestFile};
use tsgo_tspath::{get_normalized_absolute_path, ComparePathsOptions};

use crate::{extract_compiler_settings, make_units_from_test, SRC_FOLDER};

/// The harness newline sequence (`\r\n`) used throughout the baselines.
// Go: internal/testutil/tsbaseline/error_baseline.go:harnessNewLine
const HARNESS_NEW_LINE: &str = "\r\n";

/// Which compiler test suite a runner serves.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/compiler_runner.go:CompilerTestType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerTestType {
    /// The `conformance` suite.
    Conformance,
    /// The `compiler` (regression) suite.
    Regression,
}

impl CompilerTestType {
    /// The suite's directory/baseline name (`conformance` / `compiler`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_testrunner::CompilerTestType;
    /// assert_eq!(CompilerTestType::Conformance.name(), "conformance");
    /// assert_eq!(CompilerTestType::Regression.name(), "compiler");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/testrunner/compiler_runner.go:CompilerTestType.String
    pub fn name(self) -> &'static str {
        match self {
            CompilerTestType::Conformance => "conformance",
            CompilerTestType::Regression => "compiler",
        }
    }
}

/// Compiles the inline test `code` (parsed by [`make_units_from_test`]) and
/// returns its `.errors.txt` baseline string, or [`baseline::NO_CONTENT`] when
/// there are no diagnostics.
///
/// This wires the foundation pipeline end-to-end (parse → compile → format),
/// the way `compiler_runner.go`'s `verifyDiagnostics` does via
/// `tsbaseline.DoErrorBaseline`.
///
/// # Examples
/// ```
/// use tsgo_testrunner::error_baseline_for_test;
/// let baseline = error_baseline_for_test("const ok = 1;", "clean.ts");
/// assert_eq!(baseline, "<no content>");
/// ```
///
/// DEFER(P10): pretty (`--pretty`) error baselines, related-information
/// rendering, and `.types`/`.symbols`/`.js` baselines.
/// blocked-by: related-info location rendering (no reachable inline case emits
/// it) + the language-service type writer (P7).
///
/// Side effects: compiles the code in memory (see
/// [`compile_files`](tsgo_testutil_harnessutil::compile_files)).
// Go: internal/testrunner/compiler_runner.go:compilerTest.verifyDiagnostics
//     + internal/testutil/tsbaseline/error_baseline.go:DoErrorBaseline
pub fn error_baseline_for_test(code: &str, file_name: &str) -> String {
    let units = make_units_from_test(code, file_name).test_unit_data;
    let input_files: Vec<TestFile> = units
        .iter()
        .map(|unit| TestFile {
            unit_name: get_normalized_absolute_path(&unit.name, SRC_FOLDER),
            content: unit.content.clone(),
        })
        .collect();
    let settings = extract_compiler_settings(code);
    let result = compile_files(input_files.clone(), Vec::new(), &settings, SRC_FOLDER);

    if result.diagnostics().is_empty() {
        return tsgo_testutil_baseline::NO_CONTENT.to_string();
    }
    get_error_baseline(&input_files, result.diagnostics(), false)
}

/// Produces the `.errors.txt` baseline string for `diagnostics` against
/// `input_files` (non-pretty).
///
/// Side effects: none (pure).
// Go: internal/testutil/tsbaseline/error_baseline.go:GetErrorBaseline
pub fn get_error_baseline(
    input_files: &[TestFile],
    diagnostics: &[HarnessDiagnostic],
    pretty: bool,
) -> String {
    iterate_error_baseline(input_files, diagnostics, pretty).join("")
}

/// The harness UI locale (always `en`).
fn default_locale() -> Locale {
    tsgo_locale::parse("en").expect("en locale is always available")
}

/// The non-colored formatting options (CRLF newline, zero-value path options),
/// matching Go's `tsbaseline.formatOpts`.
// Go: internal/testutil/tsbaseline/error_baseline.go:formatOpts
fn format_opts(locale: Locale) -> FormattingOptions {
    FormattingOptions {
        locale,
        compare_paths_options: ComparePathsOptions::default(),
        new_line: HARNESS_NEW_LINE.to_string(),
    }
}

/// Returns the next inter-line separator, empty for the very first line.
// Go: internal/testutil/tsbaseline/error_baseline.go:iterateErrorBaseline (newLine)
fn next_newline(first_line: &mut bool) -> &'static str {
    if *first_line {
        *first_line = false;
        ""
    } else {
        HARNESS_NEW_LINE
    }
}

/// Sorts diagnostics by file, then position, then span length, then code
/// (the reachable subset of Go's `CompareASTDiagnostics`).
// Go: internal/ast/diagnostic.go:CompareDiagnostics
fn compare_diagnostics(a: &&HarnessDiagnostic, b: &&HarnessDiagnostic) -> Ordering {
    a.file_name()
        .cmp(&b.file_name())
        .then_with(|| a.start().cmp(&b.start()))
        .then_with(|| a.length().cmp(&b.length()))
        .then_with(|| a.code().cmp(&b.code()))
}

/// Renders the top-of-baseline compact diagnostic list.
// Go: internal/testutil/tsbaseline/error_baseline.go:minimalDiagnosticsToString
fn minimal_diagnostics_to_string(
    diagnostics: &[&HarnessDiagnostic],
    pretty: bool,
    opts: &FormattingOptions,
) -> String {
    // DEFER(P10): pretty output (color + source context). blocked-by: a reachable
    // pretty error-baseline case; the compiler tests run with `--pretty false`.
    let _ = pretty;
    let widened: Vec<&dyn DiagnosticTrait> = diagnostics
        .iter()
        .map(|d| *d as &dyn DiagnosticTrait)
        .collect();
    let mut output = String::new();
    write_format_diagnostics(&mut output, &widened, opts);
    output
}

/// Appends the `!!! <category> TS<code>: <message>` lines for `diag`.
// Go: internal/testutil/tsbaseline/error_baseline.go:outputErrorText
fn output_error_text(
    output_lines: &mut String,
    first_line: &mut bool,
    diag: &HarnessDiagnostic,
    locale: &Locale,
) {
    let message = flatten_diagnostic_message(diag, HARNESS_NEW_LINE, locale);
    let stripped = remove_test_path_prefixes(&message);
    let mut err_lines: Vec<String> = Vec::new();
    for line in stripped.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        err_lines.push(format!(
            "!!! {} TS{}: {}",
            diag.category().name(),
            diag.code(),
            line
        ));
    }
    // DEFER(P10): related-information lines (`!!! related TS...`). blocked-by: a
    // reachable inline case that produces related info (none in this round).
    for line in &err_lines {
        output_lines.push_str(next_newline(first_line));
        output_lines.push_str(line);
    }
}

/// The ported body of `iterateErrorBaseline`: returns the baseline as the
/// ordered list of section strings (joined by [`get_error_baseline`]).
// Go: internal/testutil/tsbaseline/error_baseline.go:iterateErrorBaseline
fn iterate_error_baseline(
    input_files: &[TestFile],
    diagnostics: &[HarnessDiagnostic],
    pretty: bool,
) -> Vec<String> {
    let locale = default_locale();
    let opts = format_opts(default_locale());

    let mut sorted: Vec<&HarnessDiagnostic> = diagnostics.iter().collect();
    sorted.sort_by(compare_diagnostics);

    let mut output_lines = String::new();
    let mut first_line = true;
    let mut result: Vec<String> = Vec::new();

    // Top-of-file compact diagnostics, with test-path prefixes and lib(line,col)
    // prefixes normalized.
    let mut top = minimal_diagnostics_to_string(&sorted, pretty, &opts);
    top = remove_test_path_prefixes(&top);
    top = replace_diagnostics_location_prefix(&top);
    result.push(format!("{top}{HARNESS_NEW_LINE}{HARNESS_NEW_LINE}"));

    // Global (file-less) errors.
    for diag in &sorted {
        if diag.file_name().is_none() {
            output_error_text(&mut output_lines, &mut first_line, diag, &locale);
        }
    }
    result.push(std::mem::take(&mut output_lines));

    // Merge each input file's lines with the errors that fall on them.
    for input_file in input_files {
        let file_errors: Vec<&HarnessDiagnostic> = sorted
            .iter()
            .copied()
            .filter(|e| {
                e.file_name().is_some_and(|f| {
                    remove_test_path_prefixes(f) == remove_test_path_prefixes(&input_file.unit_name)
                })
            })
            .collect();

        output_lines.push_str(&format!(
            "{}==== {} ({} errors) ====",
            next_newline(&mut first_line),
            remove_test_path_prefixes(&input_file.unit_name),
            file_errors.len(),
        ));

        let line_starts = tsgo_core::compute_ecma_line_starts(&input_file.content);
        let lines = split_lines(&input_file.content);

        for (line_index, raw_line) in lines.iter().enumerate() {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let this_line_start = line_starts[line_index].0;
            let next_line_start = if line_index == lines.len() - 1 {
                input_file.content.len() as i32
            } else {
                line_starts[line_index + 1].0
            };

            output_lines.push_str(next_newline(&mut first_line));
            output_lines.push_str("    ");
            output_lines.push_str(line);

            for err in &file_errors {
                let err_start = err.start();
                let end = err_start + err.length();
                if end >= this_line_start
                    && (err_start < next_line_start || line_index == lines.len() - 1)
                {
                    let relative_offset = err_start - this_line_start;
                    let length = (end - err_start) - (this_line_start - err_start).max(0);
                    let squiggle_start = relative_offset.max(0) as usize;
                    let line_len = line.len() as i32;
                    let squiggle_end = squiggle_start
                        .max(((squiggle_start as i32 + length).min(line_len)).max(0) as usize);

                    output_lines.push_str(next_newline(&mut first_line));
                    output_lines.push_str("    ");
                    output_lines.push_str(&blank_non_whitespace(&line[..squiggle_start]));
                    let tilde_count = line[squiggle_start..squiggle_end].chars().count();
                    output_lines.push_str(&"~".repeat(tilde_count));

                    if line_index == lines.len() - 1 || next_line_start > end {
                        output_error_text(&mut output_lines, &mut first_line, err, &locale);
                    }
                }
            }
        }

        result.push(std::mem::take(&mut output_lines));
    }

    result
}

/// Replaces every non-whitespace character with a space (Go's
/// `nonWhitespace.ReplaceAllString(s, " ")`), preserving whitespace so the
/// squiggle indent aligns with the source line.
// Go: internal/testutil/tsbaseline/util.go:nonWhitespace
fn blank_non_whitespace(s: &str) -> String {
    s.chars().map(|c| if is_ws(c) { c } else { ' ' }).collect()
}

/// Matches RE2's `\s` class (`[\t\n\f\r ]`).
fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0c}')
}

/// Splits `s` on `\r?\n` (Go's `lineDelimiter`).
// Go: internal/testutil/tsbaseline/util.go:lineDelimiter
fn split_lines(s: &str) -> Vec<&str> {
    s.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Strips the harness virtual-path prefixes (`/.src/`, `/.lib/`, `/.ts/`,
/// `bundled:///libs/`, and the `file:///./{ts,lib,src}/` forms) from `text`.
///
/// # Examples
/// ```
/// use tsgo_testrunner::remove_test_path_prefixes;
/// assert_eq!(remove_test_path_prefixes("/.src/a.ts(1,7)"), "a.ts(1,7)");
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/tsbaseline/util.go:removeTestPathPrefixes
pub fn remove_test_path_prefixes(text: &str) -> String {
    // The patterns are mutually non-overlapping, so a sequential per-pattern
    // replace matches Go's simultaneous `strings.NewReplacer`.
    text.replace("/.ts/", "")
        .replace("/.lib/", "")
        .replace("/.src/", "")
        .replace("bundled:///libs/", "")
        .replace("file:///./ts/", "file:///")
        .replace("file:///./lib/", "file:///")
        .replace("file:///./src/", "file:///")
}

/// Rewrites a leading `lib*.d.ts(line,col)` location to `lib*.d.ts(--,--)` so
/// library diagnostics do not churn baselines on lib version bumps.
// Go: internal/testutil/tsbaseline/error_baseline.go:diagnosticsLocationPrefix
fn replace_diagnostics_location_prefix(text: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?im)^(lib.*\.d\.ts)\(\d+,\d+\)").expect("valid regex")
    });
    re.replace_all(text, "$1(--,--)").into_owned()
}

#[cfg(test)]
#[path = "compiler_runner_test.rs"]
mod tests;
