use super::*;
use tsgo_lsproto::Position;

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
