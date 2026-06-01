//! Failure categorization for the compiler-baseline corpus runner.
//!
//! Given a produced vs committed `.errors.txt` pair for a FAILED parity case,
//! this module classifies the mismatch into actionable categories — which
//! diagnostic codes the committed baseline expects that we do not emit
//! (`missing_diagnostic`), which extra codes we emit (`extra_diagnostic`), and
//! the at-the-right-place divergences (`wrong_code` / `wrong_span` /
//! `wrong_message`) — plus the coarse case-level kind (`no_baseline_but_errors`,
//! `missing_all_errors`, `divergent`).
//!
//! Aggregated across a subset, the [`CategoryHistogram`] is the headline
//! "prioritized backlog": the TOP mismatched diagnostic codes that should drive
//! the next checker/parser parity work (e.g. "missing TS2304 ×12, extra
//! TS2339 ×7").
//!
//! The categorizer parses the `.errors.txt` text on both sides so it is
//! symmetric (the committed reference is only available as bytes). The compact
//! top-of-baseline diagnostic list yields each diagnostic's
//! `(file, line, col, code, message)`; the per-file squiggle sections yield a
//! best-effort `span` (the underline tilde count) used only for `wrong_span`.

/// A single diagnostic parsed out of an `.errors.txt` baseline.
///
/// Side effects: none (plain data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineDiag {
    /// The (test-path-stripped) file name the diagnostic is reported against,
    /// or empty for a global (file-less) diagnostic.
    pub file: String,
    /// The 1-based line of the diagnostic start (`0` for a file-less one).
    pub line: u32,
    /// The 1-based column of the diagnostic start (`0` for a file-less one).
    pub col: u32,
    /// The bare `TSxxxx` numeric code.
    pub code: u32,
    /// The first line of the (flattened) diagnostic message.
    pub message: String,
    /// The squiggle underline length (tilde count) when determinable from the
    /// per-file section, else `None`.
    pub span: Option<u32>,
}

/// Parses the diagnostics out of an `.errors.txt` baseline string.
///
/// Each compact top-of-baseline line (`path(line,col): category TSxxxx: msg`,
/// or `category TSxxxx: msg` for a global diagnostic) yields one
/// [`BaselineDiag`]. The `!!! ...` detail lines, the `==== file ====` headers,
/// the source lines, and the squiggle underlines are not counted as separate
/// diagnostics (only the compact lines match), so multi-line message chains and
/// `!!! related` lines never inflate the count.
///
/// Side effects: none (pure).
pub fn parse_error_baseline(text: &str) -> Vec<BaselineDiag> {
    let re = compact_diag_regex();
    let spans = parse_squiggle_spans(text);
    let mut diags: Vec<BaselineDiag> = Vec::new();
    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if let Some(caps) = re.captures(line) {
            let file = caps
                .name("loc")
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let parse_u32 = |name: &str| -> u32 {
                caps.name(name)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(0)
            };
            let (line_no, col) = (parse_u32("line"), parse_u32("col"));
            let span = spans.get(&(file.clone(), line_no, col)).copied();
            diags.push(BaselineDiag {
                file,
                line: line_no,
                col,
                code: parse_u32("code"),
                message: caps
                    .name("msg")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                span,
            });
        }
    }
    diags
}

/// Builds a `(file, line, col) -> span` map from the per-file `==== ====`
/// sections by reading the squiggle (`~~~`) underlines.
///
/// Within a file section the source lines (1-based) are counted and each
/// squiggle line attaches to the most recent source line; its leading-space
/// run is the 0-based column (`+1` to match the compact 1-based column) and its
/// tilde count is the span. For a multi-line span only the first line's tilde
/// run is recorded — a deterministic proxy that is symmetric across the
/// committed and produced sides (used only to flag `wrong_span`).
fn parse_squiggle_spans(text: &str) -> indexmap::IndexMap<(String, u32, u32), u32> {
    let header = file_header_regex();
    let mut spans = indexmap::IndexMap::new();
    let mut current_file: Option<String> = None;
    let mut current_line: u32 = 0;
    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if let Some(caps) = header.captures(line) {
            current_file = Some(
                caps.name("file")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
            );
            current_line = 0;
            continue;
        }
        let Some(ref file) = current_file else {
            continue;
        };
        let Some(content) = line.strip_prefix("    ") else {
            continue;
        };
        if let Some((col, span)) = squiggle_col_and_span(content) {
            spans
                .entry((file.clone(), current_line, col))
                .or_insert(span);
        } else {
            current_line += 1;
        }
    }
    spans
}

/// If `content` (a source/squiggle line with its 4-space prefix already
/// removed) is a squiggle underline (only spaces and `~`, with at least one
/// `~`), returns its `(col, span)` — the 1-based start column and tilde count.
fn squiggle_col_and_span(content: &str) -> Option<(u32, u32)> {
    let mut leading = 0u32;
    let mut tildes = 0u32;
    let mut seen_tilde = false;
    for ch in content.chars() {
        match ch {
            ' ' if !seen_tilde => leading += 1,
            '~' => {
                seen_tilde = true;
                tildes += 1;
            }
            _ => return None,
        }
    }
    if seen_tilde {
        Some((leading + 1, tildes))
    } else {
        None
    }
}

/// The `==== <file> (N errors) ====` per-file section header regex.
fn file_header_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"^==== (?P<file>.*) \(\d+ errors\) ====$").expect("valid header regex")
    })
}

/// The compact diagnostic-line regex: an optional `path(line,col): ` location
/// prefix followed by `category TSxxxx: message`.
fn compact_diag_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"^(?:(?P<loc>\S.*?)\((?P<line>\d+),(?P<col>\d+)\): )?(?P<cat>error|warning|message) TS(?P<code>\d+): (?P<msg>.*)$",
        )
        .expect("valid compact diagnostic regex")
    })
}

/// The kind of a single code-level mismatch between the committed (expected)
/// and produced (actual) baselines.
///
/// Side effects: none (plain data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    /// The committed baseline expects a diagnostic code we do not emit.
    Missing,
    /// We emit a diagnostic code the committed baseline does not expect.
    Extra,
    /// At a shared location, we emit a different code than expected.
    WrongCode,
    /// At a shared location with the same code, our squiggle span differs.
    WrongSpan,
    /// At a shared location with the same code, our message text differs.
    WrongMessage,
}

/// A single code-level mismatch: a [`MismatchKind`] plus the diagnostic code it
/// concerns.
///
/// For [`WrongCode`](MismatchKind::WrongCode), `code` is the EXPECTED
/// (committed) code and `actual_code` is the code we produced; for every other
/// kind `actual_code` is `None`.
///
/// Side effects: none (plain data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeMismatch {
    /// Which kind of mismatch this is.
    pub kind: MismatchKind,
    /// The diagnostic code (the expected code for `WrongCode`).
    pub code: u32,
    /// The produced code for `WrongCode`, else `None`.
    pub actual_code: Option<u32>,
}

/// The coarse, case-level failure kind, layered over the per-code
/// [`CodeMismatch`] list.
///
/// Side effects: none (plain data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseCategory {
    /// No committed `.errors.txt` exists (the case is expected clean) but we
    /// produced errors.
    NoBaselineButErrors,
    /// A committed `.errors.txt` exists but we produced no errors at all.
    MissingAllErrors,
    /// Both sides have errors but they diverge.
    Divergent,
}

/// The categorized diff for one FAILED parity case: the coarse
/// [`CaseCategory`] plus the ordered list of per-code [`CodeMismatch`]es.
///
/// Side effects: none (plain data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseDiff {
    /// The coarse case-level kind.
    pub category: CaseCategory,
    /// The per-code mismatches (wrong-* in expected order, then missing, then
    /// extra).
    pub mismatches: Vec<CodeMismatch>,
}

/// Categorizes the difference between an `expected` (committed) and `actual`
/// (produced) list of parsed diagnostics into per-code mismatches.
///
/// The algorithm first removes byte-identical diagnostics (same file, line,
/// col, code, message and span) so only genuine divergences remain, then pairs
/// the leftovers by location (same file/line/col) into `wrong_code` /
/// `wrong_span` / `wrong_message`, and finally reports the still-unpaired
/// expected diagnostics as `missing` and the still-unpaired produced ones as
/// `extra`.
///
/// Side effects: none (pure).
pub fn categorize_diags(expected: &[BaselineDiag], actual: &[BaselineDiag]) -> Vec<CodeMismatch> {
    let mut exp_used = vec![false; expected.len()];
    let mut act_used = vec![false; actual.len()];
    let mut out: Vec<CodeMismatch> = Vec::new();

    // Pass 1: remove byte-identical diagnostics (these are correct, not a
    // mismatch).
    for (i, e) in expected.iter().enumerate() {
        for (j, a) in actual.iter().enumerate() {
            if !act_used[j] && e == a {
                exp_used[i] = true;
                act_used[j] = true;
                break;
            }
        }
    }

    // Pass 2: pair the leftovers by location (same file/line/col). A same-code
    // partner (a `wrong_span` / `wrong_message` divergence) is preferred over a
    // different-code one (`wrong_code`) so co-located diagnostics pair up the way
    // a human would read them.
    for (i, e) in expected.iter().enumerate() {
        if exp_used[i] {
            continue;
        }
        let same_loc =
            |a: &BaselineDiag| (e.file.as_str(), e.line, e.col) == (a.file.as_str(), a.line, a.col);
        let mut same_code: Option<usize> = None;
        let mut other_code: Option<usize> = None;
        for (j, a) in actual.iter().enumerate() {
            if act_used[j] || !same_loc(a) {
                continue;
            }
            if e.code == a.code {
                same_code = Some(j);
                break;
            } else if other_code.is_none() {
                other_code = Some(j);
            }
        }
        let Some(j) = same_code.or(other_code) else {
            continue;
        };
        let a = &actual[j];
        let mismatch = if e.code != a.code {
            Some(CodeMismatch {
                kind: MismatchKind::WrongCode,
                code: e.code,
                actual_code: Some(a.code),
            })
        } else if e.span.is_some() && a.span.is_some() && e.span != a.span {
            Some(CodeMismatch {
                kind: MismatchKind::WrongSpan,
                code: e.code,
                actual_code: None,
            })
        } else if e.message != a.message {
            Some(CodeMismatch {
                kind: MismatchKind::WrongMessage,
                code: e.code,
                actual_code: None,
            })
        } else {
            None
        };
        if let Some(m) = mismatch {
            out.push(m);
        }
        exp_used[i] = true;
        act_used[j] = true;
    }

    // Remaining expected -> `missing`.
    for (i, e) in expected.iter().enumerate() {
        if !exp_used[i] {
            out.push(CodeMismatch {
                kind: MismatchKind::Missing,
                code: e.code,
                actual_code: None,
            });
        }
    }
    // Remaining actual -> `extra`.
    for (j, a) in actual.iter().enumerate() {
        if !act_used[j] {
            out.push(CodeMismatch {
                kind: MismatchKind::Extra,
                code: a.code,
                actual_code: None,
            });
        }
    }

    out
}

/// Parses a baseline side, treating the [`NO_CONTENT`](tsgo_testutil_baseline::NO_CONTENT)
/// sentinel (and a blank string) as "no diagnostics".
fn parse_side(text: &str) -> Vec<BaselineDiag> {
    if text == tsgo_testutil_baseline::NO_CONTENT || text.trim().is_empty() {
        Vec::new()
    } else {
        parse_error_baseline(text)
    }
}

/// Categorizes a FAILED parity case from its produced and committed
/// `.errors.txt` text.
///
/// Side effects: none (pure).
pub fn categorize_failure(produced: &str, committed: Option<&str>) -> CaseDiff {
    let actual = parse_side(produced);
    let (expected, committed_present) = match committed {
        Some(c) => (parse_side(c), true),
        None => (Vec::new(), false),
    };
    let mismatches = categorize_diags(&expected, &actual);
    let category = if !committed_present {
        CaseCategory::NoBaselineButErrors
    } else if actual.is_empty() && !expected.is_empty() {
        CaseCategory::MissingAllErrors
    } else {
        CaseCategory::Divergent
    };
    CaseDiff {
        category,
        mismatches,
    }
}

/// An aggregated tally of failure categories across a batch of FAILED cases —
/// the prioritized backlog histogram.
///
/// The per-code maps (`missing`, `extra`, `wrong_code`, `wrong_span`,
/// `wrong_message`) count how many times each `TSxxxx` code was the subject of
/// that mismatch kind; the three scalars count the coarse case-level kinds.
/// The headline signal is [`top_missing`](Self::top_missing) /
/// [`top_extra`](Self::top_extra): the dominant missing/extra codes that should
/// drive the next checker/parser parity work.
///
/// Side effects: none (plain data).
#[derive(Debug, Clone, Default)]
pub struct CategoryHistogram {
    /// Per-code counts of `missing_diagnostic` mismatches.
    pub missing: indexmap::IndexMap<u32, usize>,
    /// Per-code counts of `extra_diagnostic` mismatches.
    pub extra: indexmap::IndexMap<u32, usize>,
    /// Per-(expected-)code counts of `wrong_code` mismatches.
    pub wrong_code: indexmap::IndexMap<u32, usize>,
    /// Per-code counts of `wrong_span` mismatches.
    pub wrong_span: indexmap::IndexMap<u32, usize>,
    /// Per-code counts of `wrong_message` mismatches.
    pub wrong_message: indexmap::IndexMap<u32, usize>,
    /// Cases whose committed baseline is absent but which produced errors.
    pub no_baseline_but_errors: usize,
    /// Cases whose committed baseline exists but which produced no errors.
    pub missing_all_errors: usize,
    /// Cases where both sides have errors but they diverge.
    pub divergent: usize,
}

impl CategoryHistogram {
    /// Folds one case's [`CaseDiff`] into the histogram.
    ///
    /// Side effects: mutates `self` in place.
    pub fn add_case_diff(&mut self, diff: &CaseDiff) {
        match diff.category {
            CaseCategory::NoBaselineButErrors => self.no_baseline_but_errors += 1,
            CaseCategory::MissingAllErrors => self.missing_all_errors += 1,
            CaseCategory::Divergent => self.divergent += 1,
        }
        for m in &diff.mismatches {
            let bucket = match m.kind {
                MismatchKind::Missing => &mut self.missing,
                MismatchKind::Extra => &mut self.extra,
                MismatchKind::WrongCode => &mut self.wrong_code,
                MismatchKind::WrongSpan => &mut self.wrong_span,
                MismatchKind::WrongMessage => &mut self.wrong_message,
            };
            *bucket.entry(m.code).or_insert(0) += 1;
        }
    }

    /// Aggregates an iterator of [`CaseDiff`]s into a histogram.
    ///
    /// Side effects: none (pure).
    pub fn from_case_diffs<'a, I>(diffs: I) -> CategoryHistogram
    where
        I: IntoIterator<Item = &'a CaseDiff>,
    {
        let mut hist = CategoryHistogram::default();
        for diff in diffs {
            hist.add_case_diff(diff);
        }
        hist
    }

    /// The top `n` missing codes, sorted by count descending then code
    /// ascending.
    ///
    /// Side effects: none (pure).
    pub fn top_missing(&self, n: usize) -> Vec<(u32, usize)> {
        top_codes(&self.missing, n)
    }

    /// The top `n` extra codes, sorted by count descending then code ascending.
    ///
    /// Side effects: none (pure).
    pub fn top_extra(&self, n: usize) -> Vec<(u32, usize)> {
        top_codes(&self.extra, n)
    }

    /// Renders the prioritized-backlog histogram: the case-level tally plus the
    /// per-kind code histograms (each sorted by count descending then code).
    ///
    /// Side effects: none (pure).
    pub fn report(&self) -> String {
        let mut out = format!(
            "category histogram: no_baseline_but_errors ×{}, missing_all_errors ×{}, divergent ×{}",
            self.no_baseline_but_errors, self.missing_all_errors, self.divergent,
        );
        for (label, map) in [
            ("missing", &self.missing),
            ("extra", &self.extra),
            ("wrong_code", &self.wrong_code),
            ("wrong_span", &self.wrong_span),
            ("wrong_message", &self.wrong_message),
        ] {
            if map.is_empty() {
                continue;
            }
            let codes = top_codes(map, usize::MAX)
                .into_iter()
                .map(|(code, count)| format!("TS{code} ×{count}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("\n  {label}: {codes}"));
        }
        out
    }
}

/// Sorts a code→count map by count descending, then code ascending, and returns
/// the top `n`.
fn top_codes(map: &indexmap::IndexMap<u32, usize>, n: usize) -> Vec<(u32, usize)> {
    let mut entries: Vec<(u32, usize)> = map.iter().map(|(&code, &count)| (code, count)).collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.truncate(n);
    entries
}

#[cfg(test)]
#[path = "failure_category_test.rs"]
mod tests;
