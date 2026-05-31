use super::*;
use tsgo_printer::EmitContext;
use tsgo_testutil_parsetestutil::parse_type_script;

// Go: internal/testutil/emittestutil/emittestutil.go:CheckEmit
// Behavior: emitting a clean `.ts` source file round-trips to the expected text
// and re-parses without diagnostics.
#[test]
fn check_emit_matches_expected_ts() {
    let parsed = parse_type_script("const x = 1;", false);
    let source_file = parsed.source_file;
    let text = parsed.text.clone();
    let ec = EmitContext::with_arena(parsed.arena);
    check_emit(&ec, source_file, &text, "const x = 1;");
}

// Go: internal/testutil/emittestutil/emittestutil.go:CheckEmit
// Behavior: a wrong `expected` makes the equality assertion fail (Go's
// `assert.Equal` -> our panic).
#[test]
#[should_panic(expected = "emit mismatch")]
fn check_emit_panics_on_mismatch() {
    let parsed = parse_type_script("const x = 1;", false);
    let source_file = parsed.source_file;
    let text = parsed.text.clone();
    let ec = EmitContext::with_arena(parsed.arena);
    check_emit(&ec, source_file, &text, "const x = 2;");
}

// Go: internal/testutil/emittestutil/emittestutil.go:CheckEmit
// Behavior: a JSX source file re-parses under the JSX variant (driven by the
// source file's `language_variant`), so the round-trip stays diagnostic-free.
#[test]
fn check_emit_handles_jsx_variant() {
    let parsed = parse_type_script("const e = <div></div>;", true);
    let source_file = parsed.source_file;
    let text = parsed.text.clone();
    let ec = EmitContext::with_arena(parsed.arena);
    check_emit(&ec, source_file, &text, "const e = <div></div>;");
}
