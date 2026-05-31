use super::*;
use tsgo_ast::Kind;

// Go: internal/testutil/parsetestutil/parsetestutil.go:ParseTypeScript
// Behavior: a well-formed `.ts` snippet parses into a `SourceFile` with no
// diagnostics, and the original text is preserved.
#[test]
fn parse_type_script_parses_clean_ts() {
    let file = parse_type_script("let x = 1;", false);
    assert_eq!(file.arena.kind(file.source_file), Kind::SourceFile);
    assert!(file.diagnostics.is_empty());
    assert_eq!(file.text, "let x = 1;");
    assert_eq!(file.language_variant, LanguageVariant::Standard);
}

// Go: internal/testutil/parsetestutil/parsetestutil.go:ParseTypeScript
// Behavior: with `jsx = true` the file is named `/main.tsx`, so the parser
// detects the JSX language variant.
#[test]
fn parse_type_script_detects_jsx_variant() {
    let file = parse_type_script("const e = <div/>;", true);
    assert_eq!(file.language_variant, LanguageVariant::Jsx);
    assert!(file.diagnostics.is_empty());
}

// Go: internal/testutil/parsetestutil/parsetestutil.go:CheckDiagnostics
// Behavior: a clean parse produces no diagnostics, so the check does not panic.
#[test]
fn check_diagnostics_passes_on_clean_file() {
    let file = parse_type_script("let x = 1;", false);
    check_diagnostics(&file);
}

// Go: internal/testutil/parsetestutil/parsetestutil.go:CheckDiagnostics
// Behavior: a syntactically invalid parse yields diagnostics, so the check
// panics (the Go helper calls `t.Error`).
#[test]
#[should_panic(expected = "TS")]
fn check_diagnostics_panics_on_parse_error() {
    let file = parse_type_script("let x =", false);
    assert!(!file.diagnostics.is_empty(), "expected a parse diagnostic");
    check_diagnostics(&file);
}

// Go: internal/testutil/parsetestutil/parsetestutil.go:CheckDiagnosticsMessage
// Behavior: the panic message is prefixed with the supplied message.
#[test]
#[should_panic(expected = "error on reparse: ")]
fn check_diagnostics_message_prefixes_message() {
    let file = parse_type_script("let x =", false);
    check_diagnostics_message(&file, "error on reparse: ");
}

// Go: internal/testutil/parsetestutil/parsetestutil.go:MarkSyntheticRecursive
// Behavior: after marking, the node and every descendant carry the undefined
// range (-1, -1).
#[test]
fn mark_synthetic_recursive_clears_all_locations() {
    let mut file = parse_type_script("let x = 1;", false);
    // Locations start as real (non-negative) source ranges.
    assert!(file.arena.loc(file.source_file).pos() >= 0);

    mark_synthetic_recursive(&mut file.arena, file.source_file);

    // Collect the whole subtree and assert every location is undefined.
    let mut stack = vec![file.source_file];
    let mut visited = 0usize;
    while let Some(id) = stack.pop() {
        let loc = file.arena.loc(id);
        assert_eq!(loc.pos(), -1, "node {id:?} pos not cleared");
        assert_eq!(loc.end(), -1, "node {id:?} end not cleared");
        visited += 1;
        stack.extend(file.arena.get_children(id));
    }
    assert!(visited > 1, "expected to visit the subtree, saw {visited}");
}
