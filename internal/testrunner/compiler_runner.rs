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
use std::path::{Path, PathBuf};

use tsgo_diagnosticwriter::{
    flatten_diagnostic_message, write_format_diagnostics, Diagnostic as DiagnosticTrait,
    FormattingOptions,
};
use tsgo_locale::Locale;
use tsgo_testutil_harnessutil::{compile_files, HarnessDiagnostic, TestFile};
use tsgo_tspath::{get_base_file_name, get_normalized_absolute_path, ComparePathsOptions};

use crate::{
    categorize_failure, extract_compiler_settings, make_units_from_test, CaseDiff,
    CategoryHistogram, Runner, SRC_FOLDER,
};

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

/// The parity outcome of comparing one case's produced `.errors.txt` baseline
/// against its committed reference.
///
/// This is the per-case verdict of the "identical-to-tsc" error-baseline gate:
/// a case [`Passed`](Self::Passed) when the produced baseline matches the
/// committed reference byte-for-byte (or both are empty), [`Failed`](Self::Failed)
/// when it differs, and [`Errored`](Self::Errored) when the compile pipeline
/// panicked.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/compiler_runner.go:compilerTest.verifyDiagnostics (verdict)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParityOutcome {
    /// The produced baseline matched the committed reference (or both empty).
    Passed,
    /// The produced baseline differs from the committed reference; `detail`
    /// holds a short, human-readable diff summary.
    Failed {
        /// A short diff summary (the first lines of the unified diff, or a note
        /// when the case produced unexpected / missing errors).
        detail: String,
    },
    /// The compile pipeline panicked; `message` holds the panic payload text.
    Errored {
        /// The panic message (downcast from the panic payload, when a string).
        message: String,
    },
}

/// The result of running one named compiler/conformance case.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/compiler_runner.go:compilerTest (per-case result)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// The case's base file name (e.g. `simpleTest.ts`).
    pub name: String,
    /// The parity verdict for this case.
    pub outcome: ParityOutcome,
    /// The categorized failure diff, present only for a
    /// [`Failed`](ParityOutcome::Failed) case (the input to the parity
    /// [`CategoryHistogram`]); `None` for passed/errored cases.
    pub diff: Option<CaseDiff>,
}

/// The tallied counts of the three parity outcomes over a batch of cases.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.RunTests (tally)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParityCounts {
    /// Cases whose produced baseline matched the committed reference.
    pub passed: usize,
    /// Cases whose produced baseline differed from the committed reference.
    pub failed: usize,
    /// Cases whose compile pipeline panicked.
    pub errored: usize,
}

impl ParityCounts {
    /// The total number of cases tallied.
    ///
    /// # Examples
    /// ```
    /// use tsgo_testrunner::ParityCounts;
    /// let c = ParityCounts { passed: 2, failed: 3, errored: 1 };
    /// assert_eq!(c.total(), 6);
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn total(self) -> usize {
        self.passed + self.failed + self.errored
    }
}

/// The summary of a corpus batch run: every [`CaseResult`] in run order, with
/// tally + report helpers.
///
/// Side effects: none (owns the per-case results).
// Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.RunTests (summary)
#[derive(Debug, Clone, Default)]
pub struct ParitySummary {
    results: Vec<CaseResult>,
}

impl ParitySummary {
    /// The per-case results, in the order the cases were run.
    ///
    /// Side effects: none (pure).
    pub fn results(&self) -> &[CaseResult] {
        &self.results
    }

    /// The tallied `{passed, failed, errored}` counts.
    ///
    /// # Examples
    /// ```
    /// use tsgo_testrunner::{CaseResult, ParityOutcome, ParitySummary};
    /// let summary = ParitySummary::from_results(vec![
    ///     CaseResult { name: "a.ts".into(), outcome: ParityOutcome::Passed, diff: None },
    ///     CaseResult {
    ///         name: "b.ts".into(),
    ///         outcome: ParityOutcome::Failed { detail: "x".into() },
    ///         diff: None,
    ///     },
    /// ]);
    /// let counts = summary.counts();
    /// assert_eq!(counts.passed, 1);
    /// assert_eq!(counts.failed, 1);
    /// assert_eq!(counts.errored, 0);
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn counts(&self) -> ParityCounts {
        let mut counts = ParityCounts::default();
        for result in &self.results {
            match result.outcome {
                ParityOutcome::Passed => counts.passed += 1,
                ParityOutcome::Failed { .. } => counts.failed += 1,
                ParityOutcome::Errored { .. } => counts.errored += 1,
            }
        }
        counts
    }

    /// Builds a summary directly from a list of results (test/helper
    /// constructor).
    ///
    /// Side effects: none (pure).
    pub fn from_results(results: Vec<CaseResult>) -> ParitySummary {
        ParitySummary { results }
    }

    /// Aggregates the per-case [`CaseDiff`]s of every failed case into the
    /// prioritized-backlog [`CategoryHistogram`].
    ///
    /// Side effects: none (pure).
    pub fn histogram(&self) -> CategoryHistogram {
        CategoryHistogram::from_case_diffs(self.results.iter().filter_map(|r| r.diff.as_ref()))
    }

    /// Renders a deterministic, human-readable parity report: a header with the
    /// counts, the prioritized-backlog category histogram, then one line per
    /// case (`PASS` / `FAIL` / `ERR`) with a short indented detail under each
    /// failure / error.
    ///
    /// Side effects: none (pure).
    pub fn report(&self) -> String {
        let counts = self.counts();
        let mut out = format!(
            "parity: {} cases — passed {}, failed {}, errored {}",
            counts.total(),
            counts.passed,
            counts.failed,
            counts.errored,
        );
        push_indented(&mut out, &self.histogram().report());
        for result in &self.results {
            match &result.outcome {
                ParityOutcome::Passed => {
                    out.push_str(&format!("\nPASS {}", result.name));
                }
                ParityOutcome::Failed { detail } => {
                    out.push_str(&format!("\nFAIL {}", result.name));
                    push_indented(&mut out, detail);
                }
                ParityOutcome::Errored { message } => {
                    out.push_str(&format!("\nERR  {}", result.name));
                    push_indented(&mut out, message);
                }
            }
        }
        out
    }
}

/// Appends each line of `detail` to `out`, indented under a case header.
fn push_indented(out: &mut String, detail: &str) {
    for line in detail.split('\n') {
        out.push_str("\n    ");
        out.push_str(line);
    }
}

/// Compares a case's `produced` `.errors.txt` baseline against its `committed`
/// reference (`None` when no `.errors.txt` is committed, i.e. the case is
/// expected to produce no errors).
///
/// The verdict mirrors the baseline framework's accept rule:
/// - no committed baseline + produced [`NO_CONTENT`](tsgo_testutil_baseline::NO_CONTENT)
///   → [`Passed`](ParityOutcome::Passed);
/// - no committed baseline + produced errors → [`Failed`](ParityOutcome::Failed);
/// - committed baseline + produced `NO_CONTENT` → `Failed` (errors went missing);
/// - committed baseline + byte-equal produced → `Passed`;
/// - committed baseline + differing produced → `Failed` with a short diff.
///
/// # Examples
/// ```
/// use tsgo_testrunner::{compare_error_baseline, ParityOutcome};
/// assert_eq!(compare_error_baseline("<no content>", None), ParityOutcome::Passed);
/// assert_eq!(compare_error_baseline("x", Some("x")), ParityOutcome::Passed);
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/baseline/baseline.go:writeComparison (compare branch)
pub fn compare_error_baseline(produced: &str, committed: Option<&str>) -> ParityOutcome {
    let no_content = tsgo_testutil_baseline::NO_CONTENT;
    match committed {
        None => {
            if produced == no_content {
                ParityOutcome::Passed
            } else {
                ParityOutcome::Failed {
                    detail: format!(
                        "produced errors but no committed `.errors.txt` baseline exists:\n{}",
                        head_lines(produced, 12),
                    ),
                }
            }
        }
        Some(reference) => {
            if produced == no_content {
                ParityOutcome::Failed {
                    detail: "a committed `.errors.txt` baseline exists but no errors were produced"
                        .to_string(),
                }
            } else if produced == reference {
                ParityOutcome::Passed
            } else {
                ParityOutcome::Failed {
                    detail: short_baseline_diff(reference, produced),
                }
            }
        }
    }
}

/// Renders a short unified diff (first lines only) between the `committed`
/// reference and the `produced` baseline.
fn short_baseline_diff(committed: &str, produced: &str) -> String {
    let full = tsgo_testutil_baseline::diff_text(
        "committed.errors.txt",
        "produced.errors.txt",
        committed,
        produced,
    );
    head_lines(&full, 16)
}

/// Returns the first `max_lines` lines of `text`, with a trailing
/// `... (truncated)` marker when more lines were dropped.
fn head_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let shown = lines.len().min(max_lines);
    let mut out = lines[..shown].join("\n");
    if lines.len() > max_lines {
        out.push_str("\n... (truncated)");
    }
    out
}

/// Replaces a `.ts` / `.tsx` extension on `basename` with `.errors.txt`
/// (mirrors Go's `tsExtension.ReplaceAllString(name, ".errors.txt")`).
// Go: internal/testutil/tsbaseline/util.go:tsExtension
fn baseline_name_for(basename: &str) -> String {
    if let Some(stem) = basename.strip_suffix(".tsx") {
        format!("{stem}.errors.txt")
    } else if let Some(stem) = basename.strip_suffix(".ts") {
        format!("{stem}.errors.txt")
    } else {
        format!("{basename}.errors.txt")
    }
}

/// Extracts a printable message from a caught panic payload.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with non-string payload".to_string()
    }
}

/// Drives the `.errors.txt` baseline over real `tests/cases` corpus cases and
/// compares the produced baseline against the committed reference baseline.
///
/// This is the corpus-walking foundation of the "identical-to-tsc" acceptance
/// gate: it locates a suite's case directory and reference-baseline directory
/// under a `testdata` root, runs each case through
/// [`error_baseline_for_test`], and reports a [`ParityOutcome`] per case. A
/// panicking case is caught (counted [`Errored`](ParityOutcome::Errored)) so one
/// bad case never aborts the run.
///
/// DEFER(P10): the `.js`/`.types`/`.symbols`/sourcemap baselines, the
/// module/target variation matrix, and in-test `tsconfig.json`/symlinks; this
/// round compares only `.errors.txt`. blocked-by: the language-service type
/// writer (P7) + the JS/sourcemap baseline harness + VFS config-host wiring.
///
/// Side effects: reads case files and reference baselines under the configured
/// `testdata` root (no writes).
// Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner
pub struct CompilerBaselineRunner {
    test_type: CompilerTestType,
    base_path: PathBuf,
    reference_dir: PathBuf,
}

impl CompilerBaselineRunner {
    /// Builds a runner for `test_type`, rooting the case directory at
    /// `<testdata_root>/tests/cases/<suite>` and the committed reference
    /// baselines at `<testdata_root>/baselines/reference/<suite>`.
    ///
    /// # Examples
    /// ```
    /// use std::path::Path;
    /// use tsgo_testrunner::{CompilerBaselineRunner, CompilerTestType};
    /// let runner = CompilerBaselineRunner::new(
    ///     CompilerTestType::Conformance,
    ///     Path::new("/tmp/testdata"),
    /// );
    /// assert_eq!(runner.test_type(), CompilerTestType::Conformance);
    /// ```
    ///
    /// Side effects: none (pure; no file system access until a case is run).
    // Go: internal/testrunner/compiler_runner.go:NewCompilerBaselineRunner
    pub fn new(test_type: CompilerTestType, testdata_root: &Path) -> CompilerBaselineRunner {
        let suite = test_type.name();
        CompilerBaselineRunner {
            test_type,
            base_path: testdata_root.join("tests").join("cases").join(suite),
            reference_dir: testdata_root
                .join("baselines")
                .join("reference")
                .join(suite),
        }
    }

    /// The suite this runner serves.
    ///
    /// Side effects: none (pure).
    pub fn test_type(&self) -> CompilerTestType {
        self.test_type
    }

    /// The directory the runner's case files live under.
    ///
    /// Side effects: none (pure).
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Selects a deterministic, reproducible curated subset of the suite's
    /// top-level case files: the sorted `.ts`/`.tsx` basenames whose source is
    /// at most `max_lines` lines, excluding any name in `denylist`, capped at
    /// `limit`.
    ///
    /// The selection is a pure function of the committed corpus (sorted name +
    /// on-disk line count), so it produces the same subset on every run — the
    /// stable input to the parity characterization. The `denylist` excludes the
    /// handful of unbounded-recursion stress cases that can abort the harness
    /// with a stack overflow (which `catch_unwind` cannot catch) or hang.
    ///
    /// Side effects: reads the suite case directory and each candidate file's
    /// length (no writes).
    pub fn curated_subset(&self, max_lines: usize, limit: usize, denylist: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.base_path) else {
            return names;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let is_ts = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "ts" || e == "tsx");
            if !is_ts {
                continue;
            }
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if denylist.contains(&name.as_str()) {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(content) if content.lines().count() <= max_lines => names.push(name),
                _ => {}
            }
        }
        names.sort();
        names.truncate(limit);
        names
    }

    /// Runs a single case (a base file name like `simpleTest.ts`, or a
    /// suite-relative path, or an absolute path) and returns its parity result.
    ///
    /// Reads the case source and (if present) the committed reference baseline,
    /// then runs `error_baseline_for_test` under [`catch_unwind`](std::panic::catch_unwind)
    /// so a parser/checker panic on advanced syntax is reported as
    /// [`Errored`](ParityOutcome::Errored) rather than aborting the batch.
    ///
    /// Side effects: reads the case file and its reference baseline.
    // Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.runTest
    pub fn run_case(&self, case_file: impl AsRef<Path>) -> CaseResult {
        let case_file = case_file.as_ref();
        let case_path = if case_file.is_absolute() {
            case_file.to_path_buf()
        } else {
            self.base_path.join(case_file)
        };
        let basename = case_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let content = match std::fs::read_to_string(&case_path) {
            Ok(content) => content,
            Err(e) => {
                return CaseResult {
                    name: basename,
                    outcome: ParityOutcome::Errored {
                        message: format!("could not read case file {}: {e}", case_path.display()),
                    },
                    diff: None,
                };
            }
        };

        let reference_path = self.reference_dir.join(baseline_name_for(&basename));
        let committed = std::fs::read_to_string(&reference_path).ok();

        let baseline_basename = get_base_file_name(&basename);
        let produced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            error_baseline_for_test(&content, &baseline_basename)
        }));

        let (outcome, diff) = match produced {
            Ok(baseline) => {
                let outcome = compare_error_baseline(&baseline, committed.as_deref());
                // Categorize only a true parity failure (the input to the
                // prioritized-backlog histogram); a pass or panic has no diff.
                let diff = match &outcome {
                    ParityOutcome::Failed { .. } => {
                        Some(categorize_failure(&baseline, committed.as_deref()))
                    }
                    _ => None,
                };
                (outcome, diff)
            }
            Err(payload) => (
                ParityOutcome::Errored {
                    message: panic_message(payload),
                },
                None,
            ),
        };

        CaseResult {
            name: basename,
            outcome,
            diff,
        }
    }

    /// Runs each named case in order and tallies the outcomes into a
    /// [`ParitySummary`].
    ///
    /// Side effects: reads each case file and its reference baseline.
    // Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.RunTests
    pub fn run_cases<I>(&self, cases: I) -> ParitySummary
    where
        I: IntoIterator,
        I::Item: AsRef<Path>,
    {
        let results = cases.into_iter().map(|c| self.run_case(c)).collect();
        ParitySummary { results }
    }
}

impl Runner for CompilerBaselineRunner {
    // Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.EnumerateTestFiles
    fn enumerate_test_files(&self) -> Vec<String> {
        let mut files: Vec<String> = Vec::new();
        collect_ts_files(&self.base_path, &mut files);
        files.sort();
        files
    }
}

/// Recursively collects `.ts` / `.tsx` file paths under `dir` into `out`
/// (mirrors Go's `harnessutil.EnumerateFiles(dir, \.tsx?$, recursive)`).
// Go: internal/testutil/harnessutil/harnessutil.go:EnumerateFiles
fn collect_ts_files(dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ts_files(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "ts" || e == "tsx")
        {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

#[cfg(test)]
#[path = "compiler_runner_test.rs"]
mod tests;
