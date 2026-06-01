use tsgo_lsproto::{DiagnosticSeverity, IntegerOrString, Position, Range};

use crate::test_support::build_service;

// Go: internal/ls/diagnostics.go:getAllDiagnostics (GetSemanticDiagnostics) — a
// `const x: number = "s";` reports one TS2322 mapped to an lsproto::Diagnostic.
#[test]
fn get_semantic_diagnostics_reports_ts2322() {
    let mut ls = build_service(&[("/m.ts", "const x: number = \"s\";")], "/", &["/m.ts"]);
    let diags = ls.get_semantic_diagnostics("/m.ts");
    assert_eq!(diags.len(), 1, "exactly one semantic diagnostic: {diags:?}");
    let d = &diags[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
        d.code,
        Some(IntegerOrString {
            integer: Some(2322),
            string: None
        })
    );
    assert_eq!(d.source.as_deref(), Some("ts"));
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    // The span is the `x: number = "s"` variable declaration (Go reports the
    // type-not-assignable error at the declaration node). The node's full start
    // is the byte after `const` (byte 5, the leading space) and it ends after
    // the initializer (byte 21). On an ASCII single line the UTF-16 character
    // offset equals the byte offset.
    assert_eq!(
        d.range,
        Range {
            start: Position {
                line: 0,
                character: 5
            },
            end: Position {
                line: 0,
                character: 21
            },
        }
    );
}

// A file whose initializer is assignable to its annotation reports no semantic
// diagnostics.
// Go: internal/ls/diagnostics.go:getAllDiagnostics (no diagnostics)
#[test]
fn get_semantic_diagnostics_clean_file_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x: number = 1;")], "/", &["/m.ts"]);
    assert!(ls.get_semantic_diagnostics("/m.ts").is_empty());
}

// An unknown file yields no diagnostics (no panic).
// Go: internal/ls/languageservice.go:tryGetProgramAndFile (nil file)
#[test]
fn get_semantic_diagnostics_unknown_file_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x: number = 1;")], "/", &["/m.ts"]);
    assert!(ls.get_semantic_diagnostics("/missing.ts").is_empty());
}

// Go: internal/ls/diagnostics.go:getAllDiagnostics (GetSyntacticDiagnostics) — a
// parse error (missing initializer expression) surfaces TS1109 mapped to an
// lsproto::Diagnostic with the `"ts"` source.
#[test]
fn get_syntactic_diagnostics_reports_parse_error() {
    let ls = build_service(&[("/m.ts", "let x = ;")], "/", &["/m.ts"]);
    let diags = ls.get_syntactic_diagnostics("/m.ts");
    assert_eq!(
        diags.len(),
        1,
        "exactly one syntactic diagnostic: {diags:?}"
    );
    let d = &diags[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
        d.code,
        Some(IntegerOrString {
            integer: Some(1109),
            string: None
        })
    );
    assert_eq!(d.source.as_deref(), Some("ts"));
    assert_eq!(d.message, "Expression expected.");
    // The error is at the `;` token (byte 8). On an ASCII single line the UTF-16
    // character offset equals the byte offset.
    assert_eq!(
        d.range,
        Range {
            start: Position {
                line: 0,
                character: 8
            },
            end: Position {
                line: 0,
                character: 9
            },
        }
    );
}

// A syntactically valid file reports no syntactic diagnostics.
// Go: internal/ls/diagnostics.go:getAllDiagnostics (no syntactic diagnostics)
#[test]
fn get_syntactic_diagnostics_clean_file_is_empty() {
    let ls = build_service(&[("/m.ts", "const x = 1;")], "/", &["/m.ts"]);
    assert!(ls.get_syntactic_diagnostics("/m.ts").is_empty());
}
