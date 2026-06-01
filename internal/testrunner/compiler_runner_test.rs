use super::*;

use std::rc::Rc;
use tsgo_diagnostics::Category;
use tsgo_testutil_harnessutil::{HarnessDiagnostic, HarnessFile, TestFile};

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
