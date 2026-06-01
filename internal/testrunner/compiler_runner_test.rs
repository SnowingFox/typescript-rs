use super::*;

use crate::{CaseCategory, MismatchKind};
use std::path::Path;
use std::rc::Rc;
use std::sync::Mutex;

use tempfile::TempDir;
use tsgo_diagnostics::Category;
use tsgo_testutil_harnessutil::{HarnessDiagnostic, HarnessFile, TestFile};

/// The exact `.errors.txt` baseline our compiler produces for the canonical
/// TS2322 inline case (see `error_baseline_for_ts2322_matches_go_format`),
/// rendered with the case file named `errored.ts`.
const TS2322_ERRORED_BASELINE: &str = concat!(
    "errored.ts(1,4): error TS2322: Type 'string' is not assignable to type 'number'.\r\n",
    "\r\n",
    "\r\n",
    "==== errored.ts (1 errors) ====\r\n",
    "    var x: number = \"s\";\r\n",
    "       ~~~~~~~~~~~~~~~~\r\n",
    "!!! error TS2322: Type 'string' is not assignable to type 'number'.",
);

// Serializes the tests that swap the global panic hook so concurrent test
// threads do not race on the hook.
static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

/// Runs `f` with the panic hook silenced, so a deliberately panicking corpus
/// case (caught by `catch_unwind`) does not spam the test log.
fn with_silenced_panics<R>(f: impl FnOnce() -> R) -> R {
    let _guard = PANIC_HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = f();
    std::panic::set_hook(prev);
    result
}

fn write_case(root: &Path, suite: &str, name: &str, content: &str) {
    let dir = root.join("tests").join("cases").join(suite);
    std::fs::create_dir_all(&dir).expect("create cases dir");
    std::fs::write(dir.join(name), content).expect("write case file");
}

fn write_reference(root: &Path, suite: &str, name: &str, content: &str) {
    let dir = root.join("baselines").join("reference").join(suite);
    std::fs::create_dir_all(&dir).expect("create reference dir");
    std::fs::write(dir.join(name), content).expect("write reference file");
}

// Slice 5 (RED->GREEN): the `.errors.txt` baseline for an errored inline case
// matches Go's byte format: a compact top-of-file diagnostic line, two blank
// lines, the `==== file (N errors) ====` header, the source line, the squiggle
// underline, and the `!!! error TSxxxx:` message.
// Go: internal/testutil/tsbaseline/error_baseline.go:GetErrorBaseline
#[test]
fn error_baseline_for_ts2322_matches_go_format() {
    let baseline = error_baseline_for_test("var x: number = \"s\";", "errored.ts");
    let expected = concat!(
        "errored.ts(1,4): error TS2322: Type 'string' is not assignable to type 'number'.\r\n",
        "\r\n",
        "\r\n",
        "==== errored.ts (1 errors) ====\r\n",
        "    var x: number = \"s\";\r\n",
        "       ~~~~~~~~~~~~~~~~\r\n",
        "!!! error TS2322: Type 'string' is not assignable to type 'number'.",
    );
    assert_eq!(baseline, expected);
}

// A clean inline case produces no baseline content (Go writes `NoContent`).
// Go: internal/testutil/tsbaseline/error_baseline.go:DoErrorBaseline (no errors)
#[test]
fn error_baseline_for_clean_case_is_no_content() {
    let baseline = error_baseline_for_test("const ok: number = 1;", "clean.ts");
    assert_eq!(baseline, "<no content>");
}

// Slice 6 (RED->GREEN): the ported `.errors.txt` formatter reproduces a real
// committed reference baseline byte-for-byte. The diagnostic is constructed
// directly (the partial parser does not yet emit the TS1233 grammar error; see
// the DEFER note), so this validates the formatter against committed bytes
// independently of the compile path (which slice 5 covers end-to-end).
//
// DEFER(P10): drive this case through `error_baseline_for_test` (full
// parse->compile->baseline). blocked-by: the parser's "export declaration can
// only be used at the top level" (TS1233) grammar diagnostic, not yet ported.
// Go: internal/testrunner/compiler_runner.go:RunTests (committed-baseline parity)
#[test]
fn error_baseline_matches_committed_reference() {
    // The committed test case source (`testdata/tests/cases/compiler/
    // typeOnlyExportAsIfBody.ts`), with the trailing newline that yields the
    // baseline's trailing blank source line.
    let source = "if (true) export type {};\n";
    let file = Rc::new(HarnessFile::new(
        "/.src/typeOnlyExportAsIfBody.ts".to_string(),
        source.to_string(),
    ));
    // TS1233 spans the `export` keyword: byte 10 ("if (true) " is 10 bytes),
    // length 6.
    let diag = HarnessDiagnostic::new(
        Some(file),
        1233,
        Category::Error,
        "An export declaration can only be used at the top level of a namespace or module."
            .to_string(),
        10,
        6,
    );
    let input_files = vec![TestFile {
        unit_name: "/.src/typeOnlyExportAsIfBody.ts".to_string(),
        content: source.to_string(),
    }];

    let baseline = get_error_baseline(&input_files, &[diag], false);

    let reference_path = std::path::Path::new(tsgo_repo::test_data_path())
        .join("baselines")
        .join("reference")
        .join("compiler")
        .join("typeOnlyExportAsIfBody.errors.txt");
    let reference = std::fs::read_to_string(&reference_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", reference_path.display()));

    assert_eq!(baseline, reference);
}

// `remove_test_path_prefixes` strips the harness virtual-path prefixes.
// Go: internal/testutil/tsbaseline/util.go:removeTestPathPrefixes
#[test]
fn remove_test_path_prefixes_strips_src() {
    assert_eq!(remove_test_path_prefixes("/.src/a.ts(1,7)"), "a.ts(1,7)");
    assert_eq!(remove_test_path_prefixes("/.lib/lib.d.ts"), "lib.d.ts");
    assert_eq!(
        remove_test_path_prefixes("bundled:///libs/lib.es5.d.ts"),
        "lib.es5.d.ts"
    );
}

// `CompilerTestType` reports its suite directory/baseline name.
// Go: internal/testrunner/compiler_runner.go:CompilerTestType.String
#[test]
fn compiler_test_type_names() {
    assert_eq!(CompilerTestType::Conformance.name(), "conformance");
    assert_eq!(CompilerTestType::Regression.name(), "compiler");
}

// === parity comparison core (pure) ===========================================

// Compare slice: no committed baseline + produced no content -> Passed (the
// case is expected to produce no errors, and it produced none).
// Go: internal/testutil/baseline/baseline.go:writeComparison (no reference, NoContent)
#[test]
fn compare_no_baseline_no_content_passes() {
    assert_eq!(
        compare_error_baseline("<no content>", None),
        ParityOutcome::Passed
    );
}

// Compare slice: no committed baseline + produced errors -> Failed (the case
// was expected clean but our compiler reported errors).
#[test]
fn compare_no_baseline_with_errors_fails() {
    match compare_error_baseline("a.ts(1,1): error TS1: x", None) {
        ParityOutcome::Failed { detail } => {
            assert!(detail.contains("no committed"), "detail: {detail}");
            assert!(detail.contains("error TS1"), "detail: {detail}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// Compare slice: committed baseline + byte-equal produced -> Passed.
#[test]
fn compare_with_baseline_byte_equal_passes() {
    assert_eq!(
        compare_error_baseline("same\r\nbytes", Some("same\r\nbytes")),
        ParityOutcome::Passed
    );
}

// Compare slice: committed baseline + produced no content -> Failed (errors
// the reference expects went missing).
#[test]
fn compare_with_baseline_but_no_content_fails() {
    match compare_error_baseline("<no content>", Some("a.ts(1,1): error TS1: x")) {
        ParityOutcome::Failed { detail } => {
            assert!(
                detail.contains("no errors were produced"),
                "detail: {detail}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// Compare slice: committed baseline + differing produced -> Failed with a short
// unified diff that names both sides.
#[test]
fn compare_with_baseline_mismatch_fails_with_diff() {
    match compare_error_baseline("a\nLINE-NEW\nc", Some("a\nLINE-OLD\nc")) {
        ParityOutcome::Failed { detail } => {
            assert!(detail.contains("committed.errors.txt"), "detail: {detail}");
            assert!(detail.contains("produced.errors.txt"), "detail: {detail}");
            assert!(detail.contains("-LINE-OLD"), "detail: {detail}");
            assert!(detail.contains("+LINE-NEW"), "detail: {detail}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// === CompilerBaselineRunner over the corpus ==================================

// Slice 1 (RED->GREEN): a real conformance case our compiler handles cleanly
// (`conformance/simpleTest.ts` is `1 + 2;`, with no committed `.errors.txt`)
// runs to a parity PASS.
// Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.runTest (clean)
#[test]
fn run_clean_conformance_case_passes() {
    let runner = CompilerBaselineRunner::new(
        CompilerTestType::Conformance,
        Path::new(tsgo_repo::test_data_path()),
    );
    let result = runner.run_case("simpleTest.ts");
    assert_eq!(result.name, "simpleTest.ts");
    assert_eq!(
        result.outcome,
        ParityOutcome::Passed,
        "expected clean conformance case to pass, got {:?}",
        result.outcome
    );
}

// Slice 2 (RED->GREEN): a case whose committed `.errors.txt` baseline our
// compiler reproduces byte-for-byte runs to a parity PASS (driven through the
// real file-system runner: read case + read reference + byte-compare).
// Go: internal/testrunner/compiler_runner.go:compilerTest.verifyDiagnostics (match)
#[test]
fn run_case_reproducing_committed_baseline_passes() {
    let tmp = TempDir::new().expect("temp dir");
    write_case(
        tmp.path(),
        "compiler",
        "errored.ts",
        "var x: number = \"s\";",
    );
    write_reference(
        tmp.path(),
        "compiler",
        "errored.errors.txt",
        TS2322_ERRORED_BASELINE,
    );

    let runner = CompilerBaselineRunner::new(CompilerTestType::Regression, tmp.path());
    let result = runner.run_case("errored.ts");
    assert_eq!(
        result.outcome,
        ParityOutcome::Passed,
        "expected reproduced baseline to pass, got {:?}",
        result.outcome
    );
}

// Slice 3 (RED->GREEN): a case whose produced baseline does NOT match the
// committed reference is reported as FAILED (not a crash), with a short diff.
// Go: internal/testutil/baseline/baseline.go:writeComparison (mismatch)
#[test]
fn run_case_mismatch_reports_failure_with_diff() {
    let tmp = TempDir::new().expect("temp dir");
    write_case(
        tmp.path(),
        "compiler",
        "errored.ts",
        "var x: number = \"s\";",
    );
    // Commit a deliberately wrong baseline (wrong error code).
    let wrong = TS2322_ERRORED_BASELINE.replace("TS2322", "TS9999");
    write_reference(tmp.path(), "compiler", "errored.errors.txt", &wrong);

    let runner = CompilerBaselineRunner::new(CompilerTestType::Regression, tmp.path());
    let result = runner.run_case("errored.ts");
    match result.outcome {
        ParityOutcome::Failed { detail } => {
            assert!(
                detail.contains("TS2322"),
                "detail should show ours: {detail}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// Builds the canonical TS2322 `.errors.txt` baseline for a case named `file`,
/// optionally with the code swapped to `code` (to force a `wrong_code`).
fn ts_number_baseline(file: &str, code: &str) -> String {
    format!(
        "{file}(1,4): error {code}: Type 'string' is not assignable to type 'number'.\r\n\
         \r\n\
         \r\n\
         ==== {file} (1 errors) ====\r\n    \
         var x: number = \"s\";\r\n       \
         ~~~~~~~~~~~~~~~~\r\n\
         !!! error {code}: Type 'string' is not assignable to type 'number'."
    )
}

// Wiring slice (RED->GREEN): a failed case carries a categorized `diff`, and
// `ParitySummary::histogram` aggregates the per-code backlog over a real runner
// batch — a `wrong_code` (committed TS9999 vs produced TS2322 at the same span)
// plus a `no_baseline_but_errors` (extra TS2322).
#[test]
fn run_cases_populates_diff_and_histogram() {
    let tmp = TempDir::new().expect("temp dir");
    write_case(
        tmp.path(),
        "compiler",
        "wrongcode.ts",
        "var x: number = \"s\";",
    );
    write_reference(
        tmp.path(),
        "compiler",
        "wrongcode.errors.txt",
        &ts_number_baseline("wrongcode.ts", "TS9999"),
    );
    write_case(tmp.path(), "compiler", "extra.ts", "var x: number = \"s\";");

    let runner = CompilerBaselineRunner::new(CompilerTestType::Regression, tmp.path());
    let summary = runner.run_cases(["wrongcode.ts", "extra.ts"]);

    assert_eq!(summary.counts().failed, 2, "both cases fail parity");

    let wrong = summary
        .results()
        .iter()
        .find(|r| r.name == "wrongcode.ts")
        .expect("wrongcode result");
    let diff = wrong.diff.as_ref().expect("failed case carries a diff");
    assert_eq!(diff.category, CaseCategory::Divergent);
    assert!(
        diff.mismatches
            .iter()
            .any(|m| m.kind == MismatchKind::WrongCode
                && m.code == 9999
                && m.actual_code == Some(2322)),
        "expected wrong_code 9999->2322: {:?}",
        diff.mismatches
    );

    let hist = summary.histogram();
    assert_eq!(hist.wrong_code.get(&9999), Some(&1));
    assert_eq!(hist.extra.get(&2322), Some(&1));
    assert_eq!(hist.no_baseline_but_errors, 1);
    assert_eq!(hist.divergent, 1);
    // The report embeds the prioritized-backlog histogram.
    assert!(
        summary.report().contains("category histogram:"),
        "report:\n{}",
        summary.report()
    );
}

// Slice 4 (RED->GREEN): a case that PANICS the parse/compile pipeline is caught
// and counted `errored`, and the batch run continues past it (the clean case
// after it still passes).
// Go: internal/testutil/testutil.go:RecoverAndFail (panic isolation)
#[test]
fn run_panicking_case_is_errored_and_batch_continues() {
    let tmp = TempDir::new().expect("temp dir");
    // Non-comment content before the first `// @filename` directive panics the
    // test-file parser (mirrors Go's hard error).
    write_case(
        tmp.path(),
        "compiler",
        "boom.ts",
        "const x = 1;\n// @filename: a.ts\nexport {};",
    );
    write_case(tmp.path(), "compiler", "clean.ts", "const ok: number = 1;");
    write_case(
        tmp.path(),
        "compiler",
        "errored.ts",
        "var x: number = \"s\";",
    );
    write_reference(
        tmp.path(),
        "compiler",
        "errored.errors.txt",
        TS2322_ERRORED_BASELINE,
    );

    let runner = CompilerBaselineRunner::new(CompilerTestType::Regression, tmp.path());
    let summary = with_silenced_panics(|| runner.run_cases(["boom.ts", "clean.ts", "errored.ts"]));

    let results = summary.results();
    assert_eq!(
        results.len(),
        3,
        "all three cases ran (none aborted the batch)"
    );
    assert!(
        matches!(results[0].outcome, ParityOutcome::Errored { .. }),
        "boom.ts should be errored, got {:?}",
        results[0].outcome
    );
    assert_eq!(results[1].outcome, ParityOutcome::Passed, "clean.ts");
    assert_eq!(results[2].outcome, ParityOutcome::Passed, "errored.ts");

    let counts = summary.counts();
    assert_eq!(
        counts,
        ParityCounts {
            passed: 2,
            failed: 0,
            errored: 1
        }
    );
}

// A missing case file is reported as errored (rather than panicking the
// runner).
#[test]
fn run_missing_case_is_errored() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(tmp.path().join("tests").join("cases").join("compiler"))
        .expect("mkdir");
    let runner = CompilerBaselineRunner::new(CompilerTestType::Regression, tmp.path());
    let result = runner.run_case("does-not-exist.ts");
    assert!(
        matches!(result.outcome, ParityOutcome::Errored { .. }),
        "got {:?}",
        result.outcome
    );
}

// `enumerate_test_files` walks the suite directory recursively for `.ts`/`.tsx`
// files and returns them sorted.
// Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.EnumerateTestFiles
#[test]
fn enumerate_test_files_walks_recursively_sorted() {
    let tmp = TempDir::new().expect("temp dir");
    write_case(tmp.path(), "conformance", "b.ts", "");
    write_case(tmp.path(), "conformance", "a.tsx", "");
    write_case(tmp.path(), "conformance", "note.txt", "");
    // A nested subdirectory file is also enumerated.
    let nested = tmp
        .path()
        .join("tests")
        .join("cases")
        .join("conformance")
        .join("sub");
    std::fs::create_dir_all(&nested).expect("mkdir nested");
    std::fs::write(nested.join("c.ts"), "").expect("write nested");

    let runner = CompilerBaselineRunner::new(CompilerTestType::Conformance, tmp.path());
    let files = runner.enumerate_test_files();
    let names: Vec<String> = files
        .iter()
        .map(|f| {
            Path::new(f)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, vec!["a.tsx", "b.ts", "c.ts"]);
}

// `curated_subset` selects sorted `.ts`/`.tsx` cases at most `max_lines` long,
// excluding the denylist, capped at `limit` — deterministically.
#[test]
fn curated_subset_is_deterministic_and_filtered() {
    let tmp = TempDir::new().expect("temp dir");
    write_case(tmp.path(), "compiler", "a.ts", "1\n2\n3");
    write_case(tmp.path(), "compiler", "b.ts", &"x\n".repeat(30));
    write_case(tmp.path(), "compiler", "c.ts", "1\n2\n3\n4\n5");
    write_case(tmp.path(), "compiler", "d.tsx", "1\n2");
    write_case(tmp.path(), "compiler", "skip.ts", "1");
    write_case(tmp.path(), "compiler", "note.txt", "1");

    let runner = CompilerBaselineRunner::new(CompilerTestType::Regression, tmp.path());
    // <=10 lines, exclude skip.ts, cap at 2: candidates a.ts(3) c.ts(5) d.tsx(2)
    // [b.ts is 30 lines -> excluded], sorted -> first two.
    let subset = runner.curated_subset(10, 2, &["skip.ts"]);
    assert_eq!(subset, vec!["a.ts".to_string(), "c.ts".to_string()]);

    // Stable across calls.
    assert_eq!(subset, runner.curated_subset(10, 2, &["skip.ts"]));
}

// === curated smoke subset (characterization) ================================

/// A deterministic, reproducible subset of the `tests/cases/compiler` corpus:
/// the first 30 cases (sorted by name) whose source is at most ~14 lines, so
/// the smoke run exercises reachable features without pulling in the heaviest
/// emit/recursive-type cases. This is a MEASUREMENT subset — most cases are
/// EXPECTED to fail parity because the port is a reachable subset of tsc.
const CURATED_SMOKE_CASES: &[&str] = &[
    "allowSyntheticDefaultImports9.ts",
    "anonymousClassDecoratorEs2022.ts",
    "assertionWithNoArgument.ts",
    "asyncFunctionReturnNonPromiseThenable.ts",
    "awaitObjectLiteral.ts",
    "backslashBeforeNonSpecialChar.ts",
    "bindingPatternOptionalParameterCached.ts",
    "blockedScopeVariableNotUnused1.ts",
    "catchClauseRestProperties.ts",
    "checkInheritedProperty.ts",
    "circularDestructuring.ts",
    "classExpressionWithComputedPropertyInLoop.ts",
    "classFieldsAssignmentNamedEvaluation.ts",
    "conditionalContextualReturnSubstitutionCache.ts",
    "constEnumInEmbeddedStatements.ts",
    "constructSignatureWithInferReturnType.ts",
    "contextuallyTypedFunctionOptionalAndRest.ts",
    "declarationEmitBigInt.ts",
    "declarationEmitConstObjectLiteralGenericMethod1.ts",
    "declarationEmitEnumNaN.ts",
    "declarationEmitExpandoOverloads.ts",
    "declarationEmitMethodShadowsClassTypeParameter.ts",
    "declarationEmitObjectLiteralMethodGenericNoSuffix.ts",
    "declarationEmitReadonlyAsConst.ts",
    "declarationEmitStringNamedPropertyConsistency.ts",
    "declarationMapInlineSourcesContent.ts",
    "decoratedClassWithStaticAccessor.ts",
    "destructuringEmptyBinding.ts",
    "emitDecoratorMetadataParamDecoratorNoModifiers.ts",
    "emitEndOfFileJSDocComments.ts",
];

// Slice 5 (RED->GREEN): the curated-subset parity SMOKE summary. This is a
// characterization test — it asserts the ACTUAL measured `{passed, failed,
// errored}` counts over a deterministic subset of the real compiler corpus, not
// 100% pass. Update the expected counts (only ever upward on passed) as parity
// improves. The compile batch runs on a large-stack thread so a deep checker
// recursion does not overflow the small default test-thread stack.
// Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.RunTests
#[test]
fn curated_compiler_subset_parity_smoke() {
    let runner = CompilerBaselineRunner::new(
        CompilerTestType::Regression,
        Path::new(tsgo_repo::test_data_path()),
    );
    let cases: Vec<String> = CURATED_SMOKE_CASES.iter().map(|s| s.to_string()).collect();

    let summary = with_silenced_panics(|| {
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(move || runner.run_cases(cases))
            .expect("spawn smoke thread")
            .join()
            .expect("smoke thread panicked")
    });

    // Print the full parity report so the measured signal (and the per-case
    // failure detail that directs future work) is visible with `--nocapture`.
    println!("{}", summary.report());

    let counts = summary.counts();
    assert_eq!(counts.total(), CURATED_SMOKE_CASES.len());
    // Measured parity on this subset (characterization; see the worklog for the
    // failure categories behind these numbers). This is EXPECTED to be far from
    // 100% — the port is a reachable subset of tsc. Bump `passed` upward (and
    // lower `failed`/`errored`) only as real parity improves.
    //
    // The panic-robustness triage round drove `errored` 5 -> 0 (no compiler /
    // parser / checker / emit panic on these inputs): the three emit/arena cases
    // (`classExpressionWithComputedPropertyInLoop`,
    // `declarationMapInlineSourcesContent`, `emitEndOfFileJSDocComments`) now
    // PASS, while the two whose underlying feature is still a reachable gap
    // (`awaitObjectLiteral` top-level await; `allowSyntheticDefaultImports9`
    // synthetic-default import) degrade gracefully to a FAIL rather than a panic.
    assert_eq!(
        counts,
        ParityCounts {
            passed: 18,
            failed: 12,
            errored: 0,
        },
        "parity counts drifted; measured report:\n{}",
        summary.report()
    );
}

/// Cases excluded from the expanded subset: unbounded-recursion / combinatorial
/// stress cases that tsc handles via internal complexity limits we have not yet
/// ported, so they can abort the harness with a stack overflow (which
/// `catch_unwind` cannot catch) or hang. Documented + deterministic.
const EXPANDED_DENYLIST: &[&str] = &[
    // `const f = () => 42 satisfies typeof f;` — self-referential `typeof`.
    "noTypeToStringStackOverflow.ts",
    // 49-fold template literal: a combinatorial union explosion tsc rejects with
    // TS2590; without the complexity guard the union is materialized (hang/OOM).
    "templateLiteralTypeTooComplex.ts",
];

// Expanded characterization (RED->GREEN): the parity SMOKE over the LARGER
// deterministic subset — the sorted `tests/cases/compiler` cases at most 25
// lines (minus the documented stress denylist), capped at 150. Asserts the
// ACTUAL measured `{passed, failed, errored}` AND the top mismatched diagnostic
// codes (the prioritized backlog). This is a MEASUREMENT: most cases are
// EXPECTED to fail parity because the port is a reachable subset of tsc; the
// value is the categorized histogram, not a pass rate. Update the expected
// numbers as parity improves — never assert 100%.
// Go: internal/testrunner/compiler_runner.go:CompilerBaselineRunner.RunTests
#[test]
fn expanded_compiler_subset_parity_smoke() {
    let runner = CompilerBaselineRunner::new(
        CompilerTestType::Regression,
        Path::new(tsgo_repo::test_data_path()),
    );
    let cases = runner.curated_subset(25, 150, EXPANDED_DENYLIST);
    assert_eq!(cases.len(), 150, "deterministic subset size");

    // The compile batch runs on a large-stack thread so a deep checker recursion
    // does not overflow the small default test-thread stack; `catch_unwind`
    // inside `run_case` turns any unwinding panic into an `Errored` verdict so a
    // single bad case never aborts the batch.
    let summary = with_silenced_panics(|| {
        std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .spawn(move || runner.run_cases(cases))
            .expect("spawn smoke thread")
            .join()
            .expect("smoke thread panicked")
    });

    // Print the full parity report (counts + prioritized-backlog histogram +
    // per-case detail) so the measured signal is visible with `--nocapture`.
    println!("{}", summary.report());

    let counts = summary.counts();
    assert_eq!(counts.total(), 150);
    // Measured parity on the larger subset (characterization; see the worklog).
    // EXPECTED to be far from 100% — the port is a reachable subset of tsc. Bump
    // `passed` upward (and lower `failed`) only as real parity improves.
    assert_eq!(
        counts,
        ParityCounts {
            passed: 61,
            failed: 89,
            errored: 0,
        },
        "parity counts drifted; measured report:\n{}",
        summary.report()
    );

    // The prioritized-backlog histogram (the headline that directs the next
    // checker/parser work). The dominant FALSE POSITIVES are cascading
    // unresolved-name / missing-property errors (TS2304 / TS2339); the JSX
    // intrinsic-elements false negative (TS7026) is cleared in Round 6 (below),
    // leaving `TS2874 ×7` (the DEFERRED React-in-scope check) as the top miss.
    //
    // Round 3 drove the cascade down from `extra TS2304 ×82 + TS2339 ×76`
    // (55 pass) to `extra TS2304 ×62 + TS2339 ×18` (60 pass) via two checker
    // root fixes (merged-globals lookup; any-like-receiver short-circuit).
    //
    // Round 4 (this round) clears the `require` sub-cluster: `checkIdentifier`
    // now resolves a bare `require` that is the callee of a `require(...)` call
    // in a JS file to the synthetic `require` symbol (type `any`), exactly as
    // Go's `resolveName` does — so CommonJS `const a = require("./x")` no longer
    // reports a spurious 2304. This drops `extra TS2304 ×62 -> ×57` (−5). One
    // case lost its only extras and shifted divergent -> missing_all_errors.
    //
    // ROOT-CAUSE CORRECTION (verified against the Go source + committed
    // baselines): the prior round's note that "tsc skips un-`checkJs` JS files"
    // is WRONG for this repo. Go's `canIncludeBindAndCheckDiagnostics` returns
    // true for plain JS (`checkJs` unset) AND checkJs JS; the committed
    // baselines prove tsc type-checks these files (it emits TS2591/TS2339/
    // TS6424/TS2306 in them). The remaining `extra TS2304 ×57` is dominated by
    // `module`/`exports` (a deferred CommonJS-module-binding root:
    // `setCommonJSModuleIndicator` + `declareCommonJSVariable`) and TS
    // `import x = require()`/`export =` alias resolution — both DEFERRED (see
    // the worklog). The program-level `SkipTypeChecking` gate this round ports
    // (faithfully) only skips `checkJs: false` / `@ts-nocheck` JS, of which the
    // corpus has none — so it is parity-neutral but corrects a real gap.
    //
    // Round 6 (this round) lands the JSX intrinsic-element implicit-any check
    // (TS7026): an intrinsic `.tsx` tag with no `JSX.IntrinsicElements` in scope
    // and `noImplicitAny` (the default) reports TS7026 on the element node
    // (opening + closing for a paired element), exactly as Go's
    // `getIntrinsicTagSymbol`. This flips `jsxMultilineAttributeStringValues2`
    // (passed 60 -> 61) and clears the entire `missing TS7026 ×15` false
    // negative (the remaining JSX cases — `jsxEntityDecoder*`,
    // `jsxElementTypeUnexpectedType`, ... — still FAIL because they ALSO need
    // TS2874/TS2875, which are DEFERRED behind `@jsx`-pragma scanning and the
    // implicit jsx-runtime import; see the worklog). No new `extra TS7026`
    // (the spans are trivia-skipped to match `tsc` byte-for-byte), and the
    // dominant `extra TS2304 ×57`/`TS2339 ×18` are unchanged.
    let hist = summary.histogram();
    assert_eq!(
        hist.no_baseline_but_errors + hist.missing_all_errors + hist.divergent,
        counts.failed,
        "every failed case is categorized"
    );
    assert_eq!(hist.no_baseline_but_errors, 31);
    assert_eq!(hist.missing_all_errors, 32);
    assert_eq!(hist.divergent, 26);

    // Round 7 (getCannotFindNameDiagnosticForName): an unresolved identifier
    // now emits tsc's SPECIALIZED "cannot find name" code instead of the bare
    // TS2304 — `module`/`require`/`process`/`Buffer`/`NodeJS` -> TS2591 (the
    // "@types/node" hint), `document`/`console` -> TS2584, the target-lib
    // globals (`Map`/`Set`/`Promise`/...) -> TS2583, and an undefined shorthand
    // property -> TS18004 (Go's `getCannotFindNameDiagnosticForName`). On THIS
    // subset tsc resolves `module` (via CommonJS binding), so our `module`
    // diagnostics are still false positives — this round RELABELS them from
    // `extra TS2304` to the Go-faithful `extra TS2591` (parity-neutral: passed
    // stays 61), dropping `extra TS2304 ×57 -> ×44` and surfacing
    // `extra TS2591 ×12`. The genuine fix (resolving `module`/`exports`) is the
    // DEFERRED CommonJS-module-binding root.
    assert_eq!(
        hist.top_extra(2),
        vec![(2304, 44), (2339, 18)],
        "top extra (false-positive) codes; histogram:\n{}",
        hist.report()
    );
    // The JSX intrinsic-element false negative (`missing TS7026 ×15`) is cleared;
    // the top remaining false negative is `TS2874 ×7` (the React-in-scope check,
    // DEFERRED). There must be NO `extra TS7026` (over-firing) anywhere.
    assert_eq!(
        hist.top_missing(1),
        vec![(2874, 7)],
        "top missing (false-negative) code; histogram:\n{}",
        hist.report()
    );
    assert_eq!(
        hist.missing.get(&7026),
        None,
        "the JSX intrinsic-element false negative (missing TS7026) is cleared; histogram:\n{}",
        hist.report()
    );
    assert_eq!(
        hist.extra.get(&7026),
        None,
        "TS7026 must match tsc's spans exactly — no over-firing (extra TS7026); histogram:\n{}",
        hist.report()
    );
}

// `ParitySummary::report` renders deterministic per-case lines with the tally
// header.
#[test]
fn parity_summary_report_is_deterministic() {
    let summary = ParitySummary::from_results(vec![
        CaseResult {
            name: "a.ts".into(),
            outcome: ParityOutcome::Passed,
            diff: None,
        },
        CaseResult {
            name: "b.ts".into(),
            outcome: ParityOutcome::Failed {
                detail: "diff line".into(),
            },
            diff: None,
        },
        CaseResult {
            name: "c.ts".into(),
            outcome: ParityOutcome::Errored {
                message: "boom".into(),
            },
            diff: None,
        },
    ]);
    let report = summary.report();
    assert!(report.starts_with("parity: 3 cases — passed 1, failed 1, errored 1"));
    assert!(report.contains("\nPASS a.ts"));
    assert!(report.contains("\nFAIL b.ts\n    diff line"));
    assert!(report.contains("\nERR  c.ts\n    boom"));
}
