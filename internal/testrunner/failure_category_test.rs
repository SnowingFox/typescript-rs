use super::*;

// Parser slice 1 (RED->GREEN): a single compact file-ful diagnostic line is
// parsed into its (file, line, col, code, message).
#[test]
fn parse_single_compact_diagnostic() {
    let text = concat!(
        "errored.ts(1,4): error TS2322: Type 'string' is not assignable to type 'number'.\r\n",
        "\r\n",
        "\r\n",
        "==== errored.ts (1 errors) ====\r\n",
        "    var x: number = \"s\";\r\n",
        "       ~~~~~~~~~~~~~~~~\r\n",
        "!!! error TS2322: Type 'string' is not assignable to type 'number'.",
    );
    let diags = parse_error_baseline(text);
    assert_eq!(diags.len(), 1, "exactly one diagnostic: {diags:?}");
    let d = &diags[0];
    assert_eq!(d.file, "errored.ts");
    assert_eq!(d.line, 1);
    assert_eq!(d.col, 4);
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Parser slice 2 (RED->GREEN): the per-file squiggle underline yields each
// diagnostic's `span` (tilde count), and the `==== header ====` / source /
// squiggle / `!!! ...` lines are NOT over-counted as diagnostics. Driven by the
// real committed `destructuringEmptyBinding.errors.txt` (two errors, one file).
#[test]
fn parse_multi_diagnostic_with_spans() {
    let text = concat!(
        "a.ts(1,16): error TS1003: Identifier expected.\r\n",
        "a.ts(1,23): error TS2304: Cannot find name 'x'.\r\n",
        "\r\n",
        "\r\n",
        "==== a.ts (2 errors) ====\r\n",
        "    export var {...{ }} = x;\r\n",
        "                   ~\r\n",
        "!!! error TS1003: Identifier expected.\r\n",
        "                          ~\r\n",
        "!!! error TS2304: Cannot find name 'x'.\r\n",
        "    ",
    );
    let diags = parse_error_baseline(text);
    assert_eq!(diags.len(), 2, "two diagnostics, no over-count: {diags:?}");

    assert_eq!(diags[0].code, 1003);
    assert_eq!((diags[0].line, diags[0].col), (1, 16));
    assert_eq!(diags[0].span, Some(1), "TS1003 single-tilde span");

    assert_eq!(diags[1].code, 2304);
    assert_eq!((diags[1].line, diags[1].col), (1, 23));
    assert_eq!(diags[1].span, Some(1), "TS2304 single-tilde span");
}

// Parser slice 3 (RED->GREEN): a multi-character squiggle yields the full tilde
// count as the span (the canonical TS2322 baseline underlines 16 columns).
#[test]
fn parse_span_counts_all_tildes() {
    let text = concat!(
        "errored.ts(1,4): error TS2322: Type 'string' is not assignable to type 'number'.\r\n",
        "\r\n",
        "\r\n",
        "==== errored.ts (1 errors) ====\r\n",
        "    var x: number = \"s\";\r\n",
        "       ~~~~~~~~~~~~~~~~\r\n",
        "!!! error TS2322: Type 'string' is not assignable to type 'number'.",
    );
    let diags = parse_error_baseline(text);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].span, Some(16));
}

// === categorizer ============================================================

/// Builds a compact-only `.errors.txt` body from compact diagnostic lines.
fn baseline(lines: &[&str]) -> String {
    lines.join("\r\n")
}

fn diag(
    file: &str,
    line: u32,
    col: u32,
    code: u32,
    message: &str,
    span: Option<u32>,
) -> BaselineDiag {
    BaselineDiag {
        file: file.to_string(),
        line,
        col,
        code,
        message: message.to_string(),
        span,
    }
}

fn count_kind(mismatches: &[CodeMismatch], kind: MismatchKind, code: u32) -> usize {
    mismatches
        .iter()
        .filter(|m| m.kind == kind && m.code == code)
        .count()
}

// Categorizer slice 1 (RED->GREEN): a committed baseline expects a TS2304 we do
// not emit (while a co-located TS2322 matches exactly) -> classified as a single
// `missing_diagnostic{2304}` (the matched TS2322 produces no mismatch).
#[test]
fn categorize_missing_diagnostic() {
    let committed = baseline(&[
        "a.ts(1,1): error TS2304: Cannot find name 'x'.",
        "a.ts(2,1): error TS2322: Type 'string' is not assignable to type 'number'.",
    ]);
    let produced =
        baseline(&["a.ts(2,1): error TS2322: Type 'string' is not assignable to type 'number'."]);
    let diff = categorize_failure(&produced, Some(&committed));
    assert_eq!(diff.category, CaseCategory::Divergent);
    assert_eq!(
        count_kind(&diff.mismatches, MismatchKind::Missing, 2304),
        1,
        "expected one missing TS2304: {:?}",
        diff.mismatches
    );
    assert_eq!(
        diff.mismatches.len(),
        1,
        "only the missing 2304: {:?}",
        diff.mismatches
    );
}

// Categorizer slice 2 (RED->GREEN): we emit a TS2339 the committed baseline
// does not expect (alongside a matching TS2304) -> `extra_diagnostic{2339}`.
#[test]
fn categorize_extra_diagnostic() {
    let committed = baseline(&["a.ts(1,1): error TS2304: Cannot find name 'x'."]);
    let produced = baseline(&[
        "a.ts(1,1): error TS2304: Cannot find name 'x'.",
        "a.ts(5,3): error TS2339: Property 'a' does not exist on type 'error'.",
    ]);
    let diff = categorize_failure(&produced, Some(&committed));
    assert_eq!(diff.category, CaseCategory::Divergent);
    assert_eq!(count_kind(&diff.mismatches, MismatchKind::Extra, 2339), 1);
    assert_eq!(
        diff.mismatches.len(),
        1,
        "only the extra 2339: {:?}",
        diff.mismatches
    );
}

// Categorizer slice 3 (RED->GREEN): no committed baseline (case expected clean)
// but we produced errors -> `no_baseline_but_errors`, with every produced code
// recorded as `extra`.
#[test]
fn categorize_no_baseline_but_errors() {
    let produced = baseline(&[
        "a.ts(3,41): error TS2304: Cannot find name 'A'.",
        "a.ts(4,3): error TS2339: Property 'a' does not exist on type 'error'.",
    ]);
    let diff = categorize_failure(&produced, None);
    assert_eq!(diff.category, CaseCategory::NoBaselineButErrors);
    assert_eq!(count_kind(&diff.mismatches, MismatchKind::Extra, 2304), 1);
    assert_eq!(count_kind(&diff.mismatches, MismatchKind::Extra, 2339), 1);
    assert_eq!(diff.mismatches.len(), 2);
}

// Categorizer slice 4 (RED->GREEN): a committed baseline exists but we produced
// nothing -> `missing_all_errors`, with the expected code recorded as missing.
#[test]
fn categorize_missing_all_errors() {
    let committed = baseline(&["a.ts(1,1): error TS2304: Cannot find name 'x'."]);
    let diff = categorize_failure("<no content>", Some(&committed));
    assert_eq!(diff.category, CaseCategory::MissingAllErrors);
    assert_eq!(count_kind(&diff.mismatches, MismatchKind::Missing, 2304), 1);
}

// Categorizer slice 5 (RED->GREEN): at the SAME location, the committed baseline
// expects TS2304 but we emit TS2345 -> `wrong_code{expected:2304, actual:2345}`
// (not a separate missing + extra pair).
#[test]
fn categorize_wrong_code_same_location() {
    let committed = baseline(&["a.ts(2,5): error TS2304: Cannot find name 'x'."]);
    let produced = baseline(&["a.ts(2,5): error TS2345: Argument of type 'x' is not assignable."]);
    let diff = categorize_failure(&produced, Some(&committed));
    assert_eq!(
        diff.mismatches.len(),
        1,
        "single wrong_code: {:?}",
        diff.mismatches
    );
    let m = diff.mismatches[0];
    assert_eq!(m.kind, MismatchKind::WrongCode);
    assert_eq!(m.code, 2304, "expected code");
    assert_eq!(m.actual_code, Some(2345), "produced code");
}

// Categorizer slice 6 (RED->GREEN): same location, same code, same message, but
// a different squiggle span -> `wrong_span{code}` (built from synthetic diags so
// the span is controlled exactly).
#[test]
fn categorize_wrong_span_same_code() {
    let expected = vec![diag("a.ts", 1, 5, 2322, "msg", Some(10))];
    let actual = vec![diag("a.ts", 1, 5, 2322, "msg", Some(4))];
    let mismatches = categorize_diags(&expected, &actual);
    assert_eq!(mismatches.len(), 1, "single wrong_span: {mismatches:?}");
    assert_eq!(mismatches[0].kind, MismatchKind::WrongSpan);
    assert_eq!(mismatches[0].code, 2322);
}

// Categorizer slice 7 (RED->GREEN): same location, same code, same span, but a
// different message -> `wrong_message{code}`.
#[test]
fn categorize_wrong_message_same_code() {
    let expected = vec![diag("a.ts", 1, 5, 2345, "Argument of type 'a'.", Some(4))];
    let actual = vec![diag("a.ts", 1, 5, 2345, "Argument of type 'b'.", Some(4))];
    let mismatches = categorize_diags(&expected, &actual);
    assert_eq!(mismatches.len(), 1, "single wrong_message: {mismatches:?}");
    assert_eq!(mismatches[0].kind, MismatchKind::WrongMessage);
    assert_eq!(mismatches[0].code, 2345);
}

// === histogram ==============================================================

fn mismatch(kind: MismatchKind, code: u32, actual_code: Option<u32>) -> CodeMismatch {
    CodeMismatch {
        kind,
        code,
        actual_code,
    }
}

// Histogram slice (RED->GREEN): aggregating a few synthetic CaseDiffs tallies
// the per-code missing/extra/wrong_code histograms and the case-level kinds,
// and `top_missing`/`top_extra` surface the dominant codes (the prioritized
// backlog headline).
#[test]
fn category_histogram_aggregates_and_ranks() {
    let diffs = vec![
        CaseDiff {
            category: CaseCategory::NoBaselineButErrors,
            mismatches: vec![
                mismatch(MismatchKind::Extra, 2304, None),
                mismatch(MismatchKind::Extra, 2339, None),
            ],
        },
        CaseDiff {
            category: CaseCategory::Divergent,
            mismatches: vec![
                mismatch(MismatchKind::Missing, 2304, None),
                mismatch(MismatchKind::Extra, 2339, None),
                mismatch(MismatchKind::WrongCode, 2729, Some(2339)),
            ],
        },
        CaseDiff {
            category: CaseCategory::MissingAllErrors,
            mismatches: vec![mismatch(MismatchKind::Missing, 2304, None)],
        },
    ];

    let hist = CategoryHistogram::from_case_diffs(&diffs);

    assert_eq!(hist.no_baseline_but_errors, 1);
    assert_eq!(hist.missing_all_errors, 1);
    assert_eq!(hist.divergent, 1);

    assert_eq!(hist.missing.get(&2304), Some(&2));
    assert_eq!(hist.extra.get(&2339), Some(&2));
    assert_eq!(hist.extra.get(&2304), Some(&1));
    assert_eq!(hist.wrong_code.get(&2729), Some(&1));

    assert_eq!(hist.top_missing(1), vec![(2304, 2)]);
    assert_eq!(hist.top_extra(1), vec![(2339, 2)]);

    let report = hist.report();
    assert!(report.contains("missing: TS2304 ×2"), "report: {report}");
    assert!(report.contains("extra: TS2339 ×2"), "report: {report}");
    assert!(
        report.contains("no_baseline_but_errors ×1"),
        "report: {report}"
    );
}
