//! Tests for `tsgo_astnav`.
//!
//! Ported from Go `internal/astnav/tokens_test.go`. The baseline / go-json
//! parity sub-tests there require the TypeScript submodule + Node.js and are
//! deferred to P10 (see `tests.md`); the deterministic inline cases
//! (`TestUnitFindPrecedingToken`, plus the JSDoc / pointer-equality cases of
//! `TestGetTokenAtPosition`) are ported here, alongside per-function unit tests
//! (PORTING §8.6) for the navigation helpers.

use super::*;

use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

/// Parses `text` as TypeScript and wraps it in a navigation [`SourceFile`].
fn parse_ts(text: &str) -> SourceFile {
    let opts = SourceFileParseOptions {
        file_name: "/file.ts".to_string(),
    };
    let r = parse_source_file(opts, text, ScriptKind::Ts);
    SourceFile::new(r.arena, r.source_file, text.to_string())
}

/// Parses `text` as JavaScript and wraps it in a navigation [`SourceFile`].
fn parse_js(text: &str) -> SourceFile {
    let opts = SourceFileParseOptions {
        file_name: "/test.js".to_string(),
    };
    let r = parse_source_file(opts, text, ScriptKind::Js);
    SourceFile::new(r.arena, r.source_file, text.to_string())
}

/// Returns the first top-level statement id.
fn first_statement(file: &SourceFile) -> NodeId {
    match file.arena().data(file.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!("root is a SourceFile"),
    }
}

// ---- TestGetTokenAtPosition (deterministic sub-cases) ----

// Go: internal/astnav/tokens_test.go:TestGetTokenAtPosition/pointer equality
#[test]
fn get_token_at_position_pointer_equality() {
    let text = "\n\t\t\tfunction foo() {\n\t\t\t\treturn 0;\n\t\t\t}\n\t\t";
    let mut file = parse_ts(text);
    // The same position must yield the same (memoized) synthesized token id.
    assert_eq!(
        get_token_at_position(&mut file, 0),
        get_token_at_position(&mut file, 0)
    );
}

// Go: internal/astnav/tokens_test.go:TestGetTokenAtPosition/JSDoc type assertion
#[test]
fn get_token_at_position_jsdoc_type_assertion() {
    let text = "function foo(x) {\n    const s = /**@type {string}*/(x)\n}";
    let mut file = parse_js(text);
    // Position of `x` inside the parenthesized expression (position 52). This
    // must not panic; it returns either the identifier or the containing
    // parenthesized expression.
    let token = get_touching_property_name(&mut file, 52);
    let kind = file.kind(token);
    assert!(
        kind == Kind::Identifier || kind == Kind::ParenthesizedExpression,
        "expected identifier or parenthesized expression, got {kind}"
    );
}

// Go: internal/astnav/tokens_test.go:TestGetTokenAtPosition/JSDoc type assertion with comment
#[test]
fn get_token_at_position_jsdoc_type_assertion_with_comment() {
    let text =
        "function foo(x) {\n    const s = /**@type {string}*/(x)  // Go-to-definition on x causes panic\n}";
    let mut file = parse_js(text);
    // Should not panic and should return a token.
    let token = get_touching_property_name(&mut file, 52);
    let kind = file.kind(token);
    assert!(kind == Kind::Identifier || kind == Kind::ParenthesizedExpression);
}

// ---- TestUnitFindPrecedingToken (table-driven, fully portable) ----

// Go: internal/astnav/tokens_test.go:TestUnitFindPrecedingToken/after dot in jsdoc
#[test]
fn find_preceding_token_after_dot_in_jsdoc() {
    let file_content = r#"import {
    CharacterCodes,
    compareStringsCaseInsensitive,
    compareStringsCaseSensitive,
    compareValues,
    Comparison,
    Debug,
    endsWith,
    equateStringsCaseInsensitive,
    equateStringsCaseSensitive,
    GetCanonicalFileName,
    getDeclarationFileExtension,
    getStringComparer,
    identity,
    lastOrUndefined,
    Path,
    some,
    startsWith,
} from "./_namespaces/ts.js";

/**
 * Internally, we represent paths as strings with '/' as the directory separator.
 * When we make system calls (eg: LanguageServiceHost.getDirectory()),
 * we expect the host to correctly handle paths in our specified format.
 *
 * @internal
 */
export const directorySeparator = "/";
/** @internal */
export const altDirectorySeparator = "\\";
const urlSchemeSeparator = "://";
const backslashRegExp = /\\/g;


backslashRegExp.

//Path Tests

/**
 * Determines whether a charCode corresponds to '/' or '\'.
 *
 * @internal
 */
export function isAnyDirectorySeparator(charCode: number): boolean {
    return charCode === CharacterCodes.slash || charCode === CharacterCodes.backslash;
}"#;
    let mut file = parse_ts(file_content);
    let token = find_preceding_token(&mut file, 839).expect("a preceding token");
    assert_eq!(file.kind(token), Kind::DotToken);
}

// Go: internal/astnav/tokens_test.go:TestUnitFindPrecedingToken/after comma in parameter list
#[test]
fn find_preceding_token_after_comma_in_param_list() {
    let mut file = parse_ts("takesCb((n, s, ))");
    let token = find_preceding_token(&mut file, 15).expect("a preceding token");
    assert_eq!(file.kind(token), Kind::CommaToken);
}

// ---- Supplementary behavior tests (deterministic; Go-measured semantics) ----

// Hits a real AST node (no scanner synthesis needed).
#[test]
fn token_at_position_on_identifier() {
    let mut file = parse_ts("const x = 1;");
    let token = get_token_at_position(&mut file, 6);
    assert_eq!(file.kind(token), Kind::Identifier);
    assert_eq!(file.end(token), 7);
}

// Lands on punctuation that is not a standalone node, exercising scanner
// synthesis: the `;` of an expression statement.
#[test]
fn token_at_position_on_punctuation_synthesized() {
    let mut file = parse_ts("a + b;");
    let token = get_token_at_position(&mut file, 5);
    assert_eq!(file.kind(token), Kind::SemicolonToken);
    assert_eq!(file.pos(token), 5);
    assert_eq!(file.end(token), 6);
}

// `get_touching_token` rejects positions in leading trivia but still finds the
// token the cursor touches.
#[test]
fn touching_token_on_identifier() {
    let mut file = parse_ts("const x = 1;");
    let token = get_touching_token(&mut file, 6);
    assert_eq!(file.kind(token), Kind::Identifier);
}

#[test]
fn find_next_token_basic() {
    let mut file = parse_ts("a.b");
    let a = get_token_at_position(&mut file, 0);
    let root = file.root();
    let next = find_next_token(&mut file, a, root).expect("a next token");
    assert_eq!(file.kind(next), Kind::DotToken);
}

#[test]
fn find_child_of_kind_brace() {
    let mut file = parse_ts("function f(){}");
    let func = first_statement(&file);
    let block = match file.arena().data(func) {
        NodeData::FunctionDeclaration(d) => d.body.expect("function body"),
        _ => unreachable!("first statement is a function declaration"),
    };
    let brace = find_child_of_kind(&mut file, block, Kind::OpenBraceToken).expect("an open brace");
    assert_eq!(file.kind(brace), Kind::OpenBraceToken);
    assert_eq!(file.pos(brace), 12);
}

#[test]
fn find_child_of_kind_absent_returns_none() {
    let mut file = parse_ts("function f(){}");
    let func = first_statement(&file);
    // There is no `class` keyword anywhere in the function declaration.
    assert!(find_child_of_kind(&mut file, func, Kind::ClassKeyword).is_none());
}

// ---- Per-function unit tests for helpers (PORTING §8.6) ----

#[test]
fn visit_each_child_and_jsdoc_visits_children_in_order() {
    let file = parse_ts("a + b");
    let mut kinds = Vec::new();
    visit_each_child_and_jsdoc(&file, file.root(), &mut |c| kinds.push(file.kind(c)));
    // One statement plus the end-of-file token.
    assert_eq!(kinds, vec![Kind::ExpressionStatement, Kind::EndOfFile]);
}

#[test]
fn collect_children_matches_visit() {
    let file = parse_ts("a + b");
    let children = collect_children(&file, file.root());
    assert_eq!(children.len(), 2);
    assert_eq!(file.kind(children[0]), Kind::ExpressionStatement);
    assert_eq!(file.kind(children[1]), Kind::EndOfFile);
}

#[test]
fn get_start_of_node_skips_leading_trivia() {
    let file = parse_ts("  const x = 1;");
    // The statement's leading whitespace is skipped.
    assert_eq!(get_start_of_node(&file, file.root(), true), 2);
}

#[test]
fn get_token_pos_of_node_skips_trivia() {
    let file = parse_ts("  const x = 1;");
    assert_eq!(get_token_pos_of_node(&file, file.root(), true), 2);
}

#[test]
fn get_position_honors_allow_in_leading_trivia() {
    let file = parse_ts("  const x = 1;");
    let root = file.root();
    assert_eq!(get_position(&file, root, true), 0);
    assert_eq!(get_position(&file, root, false), 2);
}

#[test]
fn is_jsdoc_kind_classifies_kinds() {
    assert!(is_jsdoc_kind(Kind::JSDoc));
    assert!(is_jsdoc_kind(Kind::JSDocText));
    assert!(!is_jsdoc_kind(Kind::Identifier));
    assert!(!is_jsdoc_kind(Kind::SourceFile));
}

#[test]
fn is_jsdoc_node_false_for_real_nodes() {
    let file = parse_ts("a;");
    assert!(!is_jsdoc_node(&file, file.root()));
}

#[test]
fn is_non_whitespace_token_distinguishes_tokens() {
    let file = parse_ts("a;");
    let a = get_first_identifier(&file);
    assert!(is_non_whitespace_token(&file, a));
    assert!(!is_non_whitespace_token(&file, file.root()));
}

#[test]
fn is_whitespace_only_jsx_text_false_for_identifier() {
    let file = parse_ts("a;");
    assert!(!is_whitespace_only_jsx_text(
        &file,
        get_first_identifier(&file)
    ));
}

#[test]
fn is_jsx_child_false_for_non_jsx() {
    let file = parse_ts("a;");
    assert!(!is_jsx_child(&file, file.root()));
}

#[test]
fn is_property_name_literal_classifies() {
    let file = parse_ts("a;");
    assert!(is_property_name_literal(&file, get_first_identifier(&file)));
    assert!(!is_property_name_literal(&file, file.root()));
}

#[test]
fn is_private_identifier_false_for_identifier() {
    let file = parse_ts("a;");
    assert!(!is_private_identifier(&file, get_first_identifier(&file)));
}

#[test]
fn is_valid_preceding_node_rules() {
    let file = parse_ts("a;");
    // A real, non-empty node is valid; the end-of-file token (with no JSDoc) is
    // not.
    assert!(is_valid_preceding_node(&file, file.root()));
    let eof = match file.arena().data(file.root()) {
        NodeData::SourceFile(d) => d.end_of_file_token,
        _ => unreachable!(),
    };
    assert!(!is_valid_preceding_node(&file, eof));
}

#[test]
fn should_skip_child_false_for_real_node() {
    let file = parse_ts("a;");
    assert!(!should_skip_child(&file, file.root()));
}

#[test]
fn should_rescan_false_outside_jsx() {
    let file = parse_ts("a;");
    assert!(!should_rescan_less_than_less_than_token(
        &file,
        file.root(),
        Kind::LessThanLessThanToken
    ));
}

#[test]
fn scan_navigation_token_returns_plain_token() {
    let file = parse_ts("a.b");
    let mut scanner = get_scanner_for_source_file(file.text(), file.language_variant, 0);
    assert_eq!(
        scan_navigation_token(&mut scanner, &file, file.root()),
        Kind::Identifier
    );
}

#[test]
fn get_scanner_for_source_file_positions_at_offset() {
    let file = parse_ts("a.b");
    let scanner = get_scanner_for_source_file(file.text(), file.language_variant, 1);
    assert_eq!(scanner.token(), Kind::DotToken);
    assert_eq!(scanner.token_start(), 1);
}

#[test]
fn test_node_three_way_compare() {
    let file = parse_ts("const x = 1;");
    let mut prev = None;
    // The whole file contains position 6.
    assert_eq!(
        test_node(
            &file,
            file.root(),
            6,
            true,
            PrecedingTokenFilter::None,
            &mut prev
        ),
        0
    );
    // The first identifier ends before position 11.
    let x = get_first_identifier(&file);
    assert_eq!(
        test_node(&file, x, 11, true, PrecedingTokenFilter::None, &mut prev),
        -1
    );
}

#[test]
fn preceding_token_filter_applies() {
    let file = parse_ts("a;");
    let a = get_first_identifier(&file);
    assert!(PrecedingTokenFilter::PropertyName.applies(&file, a));
    assert!(!PrecedingTokenFilter::None.applies(&file, a));
    assert!(PrecedingTokenFilter::None.is_none());
    assert!(!PrecedingTokenFilter::PropertyName.is_none());
}

#[test]
fn find_last_visible_node_returns_last_non_reparsed() {
    let file = parse_ts("a;");
    let children = collect_children(&file, file.root());
    let last = find_last_visible_node(&file, &children).expect("a visible node");
    assert_eq!(file.kind(last), Kind::EndOfFile);
    assert!(find_last_visible_node(&file, &[]).is_none());
}

#[test]
fn find_rightmost_node_descends_to_rightmost_leaf() {
    let file = parse_ts("a.b");
    let stmt = first_statement(&file);
    let prop = match file.arena().data(stmt) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => unreachable!(),
    };
    // The rightmost visible descendant of `a.b` is the name `b`.
    let rightmost = find_rightmost_node(&file, prop);
    assert_eq!(file.kind(rightmost), Kind::Identifier);
    assert_eq!(file.pos(rightmost), 2);
}

// ---- Shared-borrow navigation surface (the LS keystone) ----

// Slice 1: a navigation handle built from a shared `Rc<NodeArena>` (as the
// program hands it to the language service) must answer `get_token_at_position`
// identically to the existing owned `&mut SourceFile` API — using only `&self`
// (shared) access, with synthesized tokens living behind interior mutability.
#[test]
fn shared_rc_get_token_at_position_matches_mut_api() {
    use std::rc::Rc;
    let text = "const x = 1;";
    // The owned, `&mut`-based reference result.
    let mut owned = parse_ts(text);
    let expected_kind = file_kind_at(&mut owned, 6);

    // Re-parse into a fresh arena, share it as the program would, and run the
    // query over the shared borrow with only `&self`.
    let r = parse_source_file(
        SourceFileParseOptions {
            file_name: "/file.ts".to_string(),
        },
        text,
        ScriptKind::Ts,
    );
    let arena = Rc::new(r.arena);
    let nav = RcSourceFile::from_rc_arena(Rc::clone(&arena), r.source_file, text.to_string());
    let token = nav.get_token_at_position(6);
    assert_eq!(nav.kind(token), Kind::Identifier);
    assert_eq!(nav.kind(token), expected_kind);
    // The shared arena is still shared (the handle did not consume it).
    assert_eq!(Rc::strong_count(&arena), 2);
}

// Slice 1: a borrowed-arena handle (`&NodeArena`) answers a punctuation query
// (which forces scanner synthesis) the same as the owned `&mut` API.
#[test]
fn shared_borrow_get_token_at_position_synthesizes_punctuation() {
    let mut owned = parse_ts("a + b;");
    let owned_kind = file_kind_at(&mut owned, 5);

    let r = parse_source_file(
        SourceFileParseOptions {
            file_name: "/file.ts".to_string(),
        },
        "a + b;",
        ScriptKind::Ts,
    );
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, "a + b;".to_string());
    let token = nav.get_token_at_position(5);
    assert_eq!(nav.kind(token), Kind::SemicolonToken);
    assert_eq!(nav.kind(token), owned_kind);
    assert_eq!(nav.pos(token), 5);
    assert_eq!(nav.end(token), 6);
    // Repeated shared queries return the same memoized synthesized id.
    assert_eq!(nav.get_token_at_position(5), token);
}

// Slice 4: a query that synthesizes a token must not corrupt real-node lookups.
// Synthesized ids carry the high tag bit and resolve out of the side store, so
// they never alias a real arena id that shares their low bits.
#[test]
fn synthesized_token_id_does_not_collide_with_real_nodes() {
    // The `;` at position 5 is not a standalone parsed node, so querying it
    // forces scanner synthesis.
    let mut file = parse_ts("a + b;");
    let root = file.root();
    let root_kind_before = file.kind(root);
    // The first real arena node (untagged id 0).
    let real0 = NodeId(0);
    let real0_kind_before = file.kind(real0);

    let semi = get_token_at_position(&mut file, 5);
    assert_eq!(file.kind(semi), Kind::SemicolonToken);

    // The synthesized id is tagged; real ids never are.
    assert_ne!(
        semi.0 & SYNTHESIZED_NODE_TAG,
        0,
        "synthesized id must be tagged"
    );
    assert_eq!(
        real0.0 & SYNTHESIZED_NODE_TAG,
        0,
        "real id must be untagged"
    );
    assert_ne!(semi, real0);

    // Real-node lookups are unchanged after synthesis (no corruption).
    assert_eq!(file.kind(root), root_kind_before);
    assert_eq!(file.kind(real0), real0_kind_before);

    // Stripping the tag bit yields a *real* id that resolves to a different node
    // than the synthesized token (the source has no real `;` node).
    let aliased_real = NodeId(semi.0 & !SYNTHESIZED_NODE_TAG);
    assert_eq!(aliased_real.0 & SYNTHESIZED_NODE_TAG, 0);
    assert_ne!(file.kind(aliased_real), Kind::SemicolonToken);
}

/// Returns the kind of the token at `pos` via the owned `&mut` API.
fn file_kind_at(file: &mut SourceFile, pos: i32) -> Kind {
    let t = get_token_at_position(file, pos);
    file.kind(t)
}

/// Returns the first identifier node by walking into the first statement.
fn get_first_identifier(file: &SourceFile) -> NodeId {
    let stmt = first_statement(file);
    // Find the first descendant identifier via a depth-first walk.
    fn walk(file: &SourceFile, id: NodeId) -> Option<NodeId> {
        if file.kind(id) == Kind::Identifier {
            return Some(id);
        }
        let mut found = None;
        file.arena().for_each_child(id, &mut |c| {
            if let Some(f) = walk(file, c) {
                found = Some(f);
                true
            } else {
                false
            }
        });
        found
    }
    walk(file, stmt).expect("an identifier")
}
