use tsgo_lsproto::{Location, Position, Range};

use crate::test_support::build_service;
use crate::RenameInfo;

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

fn range(line: u32, start: u32, end: u32) -> Range {
    Range {
        start: Position {
            line,
            character: start,
        },
        end: Position {
            line,
            character: end,
        },
    }
}

fn loc(line: u32, start: u32, end: u32) -> Location {
    Location {
        uri: tsgo_ls_lsconv::file_name_to_document_uri("/m.ts"),
        range: Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        },
    }
}

// Go: internal/ls/rename.go:ProvideRename — rename on `const x = 1; x; x;`
// returns the declaration plus both uses (3 locations to edit).
#[test]
fn provide_rename_locations_returns_declaration_and_all_uses() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x; x;")], "/", &["/m.ts"]);
    let locations = ls.provide_rename_locations(
        "/m.ts",
        Position {
            line: 0,
            character: 6,
        },
    );
    assert_eq!(
        locations,
        vec![loc(0, 6, 7), loc(0, 13, 14), loc(0, 16, 17)]
    );
}

// Go: internal/ls/rename.go:symbolAndEntriesToRename (getReferencedSymbolsForNode)
// — scope-aware resolution means renaming the inner `x` touches only the inner
// declaration + inner use, never the shadowed outer `x`.
#[test]
fn provide_rename_locations_respects_shadowing() {
    let src = "const x=1; function f(){ const x=2; x; } x;";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    let locations = ls.provide_rename_locations("/m.ts", pos(0, 36));
    assert_eq!(locations, vec![loc(0, 31, 32), loc(0, 36, 37)]);
}

// A function symbol renames its declaration name and every call site.
// Go: internal/ls/rename.go:ProvideRename
#[test]
fn provide_rename_locations_function_across_call_sites() {
    let mut ls = build_service(&[("/m.ts", "function f(){}\nf();\nf();")], "/", &["/m.ts"]);
    let locations = ls.provide_rename_locations("/m.ts", pos(0, 9));
    assert_eq!(locations, vec![loc(0, 9, 10), loc(1, 0, 1), loc(2, 0, 1)]);
}

// A non-identifier token (the `const` keyword) is not renamable: no locations.
// Go: internal/ls/rename.go:nodeIsEligibleForRename (not eligible)
#[test]
fn provide_rename_locations_on_keyword_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let locations = ls.provide_rename_locations("/m.ts", pos(0, 0));
    assert!(locations.is_empty());
}

// An unknown file yields no rename locations (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn provide_rename_locations_unknown_file_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let locations = ls.provide_rename_locations("/missing.ts", pos(0, 6));
    assert!(locations.is_empty());
}

// Go: internal/ls/rename.go:GetRenameInfo / getRenameInfoSuccess — prepare-rename
// at an identifier reports it is renamable, with the trigger span on the name
// and the symbol's display name.
#[test]
fn get_rename_info_accepts_identifier_with_trigger_span_and_display_name() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x; x;")], "/", &["/m.ts"]);
    let info = ls.get_rename_info("/m.ts", pos(0, 6));
    assert_eq!(
        info,
        RenameInfo {
            can_rename: true,
            localized_error_message: String::new(),
            display_name: "x".to_string(),
            trigger_span: range(0, 6, 7),
        }
    );
}

// Querying from a *use* still resolves the symbol and spans the use token.
// Go: internal/ls/rename.go:getRenameInfoForNode
#[test]
fn get_rename_info_from_a_use_spans_the_use_token() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x; x;")], "/", &["/m.ts"]);
    let info = ls.get_rename_info("/m.ts", pos(0, 13));
    assert!(info.can_rename);
    assert_eq!(info.display_name, "x");
    assert_eq!(info.trigger_span, range(0, 13, 14));
}

// Go: internal/ls/rename.go:GetRenameInfo (getRenameInfoError) — prepare-rename
// at a keyword reports the localized "cannot rename" message.
#[test]
fn get_rename_info_rejects_keyword_with_cannot_rename_message() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let info = ls.get_rename_info("/m.ts", pos(0, 0));
    assert!(!info.can_rename);
    assert_eq!(
        info.localized_error_message,
        "You cannot rename this element."
    );
    assert!(info.display_name.is_empty());
}

// An unknown file is reported as not renamable (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn get_rename_info_unknown_file_cannot_rename() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let info = ls.get_rename_info("/missing.ts", pos(0, 6));
    assert!(!info.can_rename);
    assert_eq!(
        info.localized_error_message,
        "You cannot rename this element."
    );
}
