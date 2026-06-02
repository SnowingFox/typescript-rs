use super::*;
use tsgo_lsproto::{Location, Position};

// `new_fourslash` parses the markup and builds a service exposing the markers
// and the first file as active.
// Go: internal/fourslash/fourslash.go:NewFourslash
#[test]
fn new_fourslash_builds_service_and_exposes_markers() {
    let f = new_fourslash("/*a*/const x: number = 1;");
    assert_eq!(f.active_filename(), "/mainFile.ts");
    assert_eq!(f.markers().len(), 1);
    assert!(f.marker_by_name("a").is_some());
    assert_eq!(
        f.current_caret_position(),
        Position {
            line: 0,
            character: 0
        }
    );
    assert_eq!(f.last_known_marker_name(), None);
}

// `go_to_marker` moves the caret to the marker's file + LSP position and records
// the marker name.
// Go: internal/fourslash/fourslash.go:FourslashTest.GoToMarker
#[test]
fn go_to_marker_sets_active_file_and_caret() {
    let mut f = new_fourslash("const x: number = 1; /*x*/x");
    f.go_to_marker("x").unwrap();
    assert_eq!(f.active_filename(), "/mainFile.ts");
    assert_eq!(
        f.current_caret_position(),
        Position {
            line: 0,
            character: 21
        }
    );
    assert_eq!(f.last_known_marker_name(), Some("x"));
}

// `go_to_marker` across `// @filename:` files switches the active file.
// Go: internal/fourslash/fourslash.go:FourslashTest.ensureActiveFile
#[test]
fn go_to_marker_switches_active_file() {
    let content = "// @filename: a.ts\nconst a: number = 1;\n\
                   // @filename: b.ts\nconst b: string = \"x\"; /*bMarker*/b";
    let mut f = new_fourslash(content);
    assert_eq!(f.active_filename(), "/a.ts");
    f.go_to_marker("bMarker").unwrap();
    assert_eq!(f.active_filename(), "/b.ts");
    assert_eq!(f.last_known_marker_name(), Some("bMarker"));
}

// `go_to_marker` on an unknown marker is an error.
// Go: internal/fourslash/fourslash.go:FourslashTest.GoToMarker (Marker not found)
#[test]
fn go_to_marker_unknown_is_error() {
    let mut f = new_fourslash("const x: number = 1;");
    let err = f.go_to_marker("nope").unwrap_err();
    assert!(err.0.contains("Marker 'nope' not found"), "got: {}", err.0);
}

// Smoke: `quick_info_at` drives the language service and returns the resolved
// type string for the token at the marker.
// Go: internal/fourslash/fourslash.go:FourslashTest.getQuickInfoAtCurrentPosition
#[test]
fn quick_info_at_marker_resolves_type() {
    let mut f = new_fourslash("const x: number = 1; /*x*/x");
    let qi = f.quick_info_at("x").unwrap().expect("quick info at `x`");
    assert_eq!(qi.text, "number");
}

// Smoke (headline command): `verify_quick_info_at` passes when the LS type
// matches the expectation.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoAt
#[test]
fn verify_quick_info_at_passes_for_correct_type() {
    let mut f = new_fourslash("const x: number = 1; /*x*/x");
    f.verify_quick_info_at("x", "number")
        .expect("quick info matches `number`");
}

// Negative guard: a wrong expectation makes `verify_quick_info_at` fail.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyHoverMarkdown (mismatch)
#[test]
fn verify_quick_info_at_fails_for_wrong_type() {
    let mut f = new_fourslash("const x: number = 1; /*x*/x");
    let err = f.verify_quick_info_at("x", "string").unwrap_err();
    assert!(err.0.contains("Quick info mismatch"), "got: {}", err.0);
    assert!(err.0.contains("At marker 'x'"), "got: {}", err.0);
}

// Negative guard: a token with no resolvable symbol (the `const` keyword) yields
// no quick info, so `verify_quick_info_at` fails.
// Go: internal/fourslash/fourslash.go:FourslashTest.getQuickInfoAtCurrentPosition (nil hover)
#[test]
fn verify_quick_info_at_fails_without_quick_info() {
    let mut f = new_fourslash("/*k*/const x: number = 1;");
    let err = f.verify_quick_info_at("k", "anything").unwrap_err();
    assert!(err.0.contains("got none"), "got: {}", err.0);
}

// `verify_quick_info_at` across a multi-file case resolves in the marker's file.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoAt (multi-file)
#[test]
fn verify_quick_info_at_resolves_in_marker_file() {
    let content = "// @filename: a.ts\nconst a: number = 1;\n\
                   // @filename: b.ts\nconst b: string = \"x\"; /*bMarker*/b";
    let mut f = new_fourslash(content);
    f.verify_quick_info_at("bMarker", "string")
        .expect("quick info matches `string` in b.ts");
}

// `try_new_fourslash` surfaces a parse error instead of panicking.
// Go: internal/fourslash/fourslash.go:NewFourslash (ParseTestData error)
#[test]
fn try_new_fourslash_reports_parse_error() {
    match try_new_fourslash("[|unterminated") {
        Err(err) => assert!(err.0.contains("Unterminated range"), "got: {}", err.0),
        Ok(_) => panic!("expected a parse error"),
    }
}

// `ranges()` / `test_data()` expose the parsed ranges through the driver.
// Go: internal/fourslash/fourslash.go:FourslashTest.Ranges
#[test]
fn driver_exposes_ranges() {
    let f = new_fourslash("const x = [|1|];");
    assert_eq!(f.ranges().len(), 1);
    assert_eq!(f.test_data().files.len(), 1);
}

// === Quick-info variants (current-position form) ===========================

// `verify_quick_info_is` passes when the LS type at the current caret matches.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoIs
#[test]
fn verify_quick_info_is_passes_for_correct_type() {
    let mut f = new_fourslash("const x: number = 1; /*x*/x");
    f.go_to_marker("x").unwrap();
    f.verify_quick_info_is("number")
        .expect("quick info matches `number` at the current caret");
}

// Negative guard: a wrong expectation makes `verify_quick_info_is` fail.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyHoverMarkdown (mismatch)
#[test]
fn verify_quick_info_is_fails_for_wrong_type() {
    let mut f = new_fourslash("const x: number = 1; /*x*/x");
    f.go_to_marker("x").unwrap();
    let err = f.verify_quick_info_is("string").unwrap_err();
    assert!(err.0.contains("Quick info mismatch"), "got: {}", err.0);
    assert!(err.0.contains("At marker 'x'"), "got: {}", err.0);
}

// `verify_quick_info_exists` passes when there is quick info at the caret.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoExists
#[test]
fn verify_quick_info_exists_passes_when_present() {
    let mut f = new_fourslash("const x: number = 1; /*x*/x");
    f.go_to_marker("x").unwrap();
    f.verify_quick_info_exists()
        .expect("quick info exists at `x`");
}

// Negative guard: at a token with no resolvable symbol (the `const` keyword)
// there is no quick info, so `verify_quick_info_exists` fails.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoExists (empty)
#[test]
fn verify_quick_info_exists_fails_on_keyword() {
    let mut f = new_fourslash("/*k*/const x: number = 1;");
    f.go_to_marker("k").unwrap();
    let err = f.verify_quick_info_exists().unwrap_err();
    assert!(err.0.contains("got none"), "got: {}", err.0);
}

// === Completions ===========================================================

// `verify_completions_include` passes when every expected label is present.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsItems (Includes)
#[test]
fn verify_completions_include_passes() {
    let mut f = new_fourslash("const o = { a: 1 }; o./*1*/");
    f.verify_completions_include("1", &["a"])
        .expect("`a` is a member completion of `o`");
}

// Negative guard: a missing label fails `verify_completions_include`.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsItems (Includes not found)
#[test]
fn verify_completions_include_fails_for_missing_label() {
    let mut f = new_fourslash("const o = { a: 1 }; o./*1*/");
    let err = f.verify_completions_include("1", &["zzz"]).unwrap_err();
    assert!(err.0.contains("Label 'zzz' not found"), "got: {}", err.0);
}

// `verify_completions_exact` passes when the labels equal the expected set (in
// label order).
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsAreExactly
#[test]
fn verify_completions_exact_passes() {
    let mut f = new_fourslash("const o = { a: 1, b: 2 }; o./*m*/");
    f.verify_completions_exact("m", &["a", "b"])
        .expect("`o`'s members are exactly `a`, `b`");
}

// Negative guard: a wrong (too small) expected set fails `verify_completions_exact`.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsAreExactly (mismatch)
#[test]
fn verify_completions_exact_fails_for_wrong_set() {
    let mut f = new_fourslash("const o = { a: 1, b: 2 }; o./*m*/");
    let err = f.verify_completions_exact("m", &["a"]).unwrap_err();
    assert!(err.0.contains("Labels mismatch"), "got: {}", err.0);
}

// `verify_completions_excludes` passes when none of the labels are present.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsItems (Excludes)
#[test]
fn verify_completions_excludes_passes() {
    let mut f = new_fourslash("const o = { a: 1 }; o./*1*/");
    f.verify_completions_excludes("1", &["b"])
        .expect("`b` is not a member completion of `o`");
}

// Negative guard: a present label fails `verify_completions_excludes`.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsItems (Excludes found)
#[test]
fn verify_completions_excludes_fails_when_present() {
    let mut f = new_fourslash("const o = { a: 1 }; o./*1*/");
    let err = f.verify_completions_excludes("1", &["a"]).unwrap_err();
    assert!(
        err.0.contains("should not be in actual items"),
        "got: {}",
        err.0
    );
}

// === Go-to-definition ======================================================

// `verify_go_to_definition` resolves a use to its declaration-name marker.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyBaselineGoToDefinition
#[test]
fn verify_go_to_definition_resolves_to_declaration() {
    let mut f = new_fourslash("const /*def*/x = 1; /*use*/x;");
    f.verify_go_to_definition("use", &["def"])
        .expect("definition of `x` use is the `x` declaration name");
}

// `verify_definition_at` is the single-target convenience form.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyBaselineGoToDefinition (single)
#[test]
fn verify_definition_at_single_target() {
    let mut f = new_fourslash("const /*def*/x = 1; /*use*/x;");
    f.verify_definition_at("use", "def")
        .expect("definition of `x` use is the `x` declaration name");
}

// Negative guard: a wrong target marker fails `verify_go_to_definition`.
// Go: internal/fourslash/fourslash.go:FourslashTest.verifyBaselineDefinitions (mismatch)
#[test]
fn verify_go_to_definition_fails_for_wrong_target() {
    let mut f = new_fourslash("const /*def*/x = 1; /*use*/x;");
    let err = f.verify_go_to_definition("use", &["use"]).unwrap_err();
    assert!(
        err.0.contains("Go-to-definition mismatch"),
        "got: {}",
        err.0
    );
}

// === Find-all-references ===================================================

// `verify_references_at` matches the symbol's references against the `[|ranges|]`
// (declaration + both uses).
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyBaselineFindAllReferences
#[test]
fn verify_references_at_matches_ranges() {
    let mut f = new_fourslash("const [|/*r*/x|] = 1; [|x|]; [|x|];");
    let expected: Vec<Location> = f.ranges().iter().map(|r| r.ls_location()).collect();
    assert_eq!(expected.len(), 3);
    f.verify_references_at("r", &expected)
        .expect("references of `x` are the three ranges");
}

// Negative guard: a missing expected range fails `verify_references_at`.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyBaselineFindAllReferences (mismatch)
#[test]
fn verify_references_at_fails_for_wrong_ranges() {
    let mut f = new_fourslash("const [|/*r*/x|] = 1; [|x|]; [|x|];");
    let expected: Vec<Location> = f.ranges().iter().map(|r| r.ls_location()).collect();
    let err = f.verify_references_at("r", &expected[..2]).unwrap_err();
    assert!(
        err.0.contains("Find-all-references mismatch"),
        "got: {}",
        err.0
    );
}

// === Diagnostics ===========================================================

// `verify_number_of_errors_in_current_file` counts the semantic error (TS2322).
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyNumberOfErrorsInCurrentFile
#[test]
fn verify_number_of_errors_counts_semantic_error() {
    let mut f = new_fourslash("const x: number = \"s\";");
    f.verify_number_of_errors_in_current_file(1)
        .expect("one semantic error in the current file");
}

// Negative guard: a wrong expected count fails.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyNumberOfErrorsInCurrentFile (mismatch)
#[test]
fn verify_number_of_errors_fails_for_wrong_count() {
    let mut f = new_fourslash("const x: number = \"s\";");
    let err = f.verify_number_of_errors_in_current_file(0).unwrap_err();
    assert!(
        err.0.contains("Expected 0 errors") && err.0.contains("got 1"),
        "got: {}",
        err.0
    );
}

// `verify_no_errors` passes for a clean file.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyNoErrors
#[test]
fn verify_no_errors_passes_for_clean_file() {
    let mut f = new_fourslash("const x: number = 1;");
    f.verify_no_errors().expect("a clean file has no errors");
}

// Negative guard: an error file fails `verify_no_errors`.
// Go: internal/fourslash/fourslash.go:FourslashTest.VerifyNoErrors (errors found)
#[test]
fn verify_no_errors_fails_for_error_file() {
    let mut f = new_fourslash("const x: number = \"s\";");
    let err = f.verify_no_errors().unwrap_err();
    assert!(err.0.contains("Expected no errors"), "got: {}", err.0);
}
