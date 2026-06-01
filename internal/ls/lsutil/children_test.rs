use super::*;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::text::TextRange;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

fn parse(text: &str) -> tsgo_parser::ParseResult {
    parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts)
}

fn first_statement(arena: &NodeArena, root: NodeId) -> NodeId {
    match arena.data(root) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!(),
    }
}

// Go: internal/ls/lsutil/children.go:AssertHasRealPosition
#[test]
fn assert_has_real_position_ok_for_parsed_node() {
    let r = parse("let a = 1;");
    assert_has_real_position(&r.arena, r.source_file);
    let stmt = first_statement(&r.arena, r.source_file);
    assert_has_real_position(&r.arena, stmt);
}

// Go: internal/ls/lsutil/children.go:AssertHasRealPosition
#[test]
#[should_panic(expected = "Node must have a real position")]
fn assert_has_real_position_panics_on_synthesized() {
    let mut arena = NodeArena::new();
    let tok = arena.new_token(Kind::SemicolonToken);
    arena.set_loc(tok, TextRange::new(-1, -1));
    assert_has_real_position(&arena, tok);
}

// Go: internal/ls/lsutil/children.go:GetLastVisitedChild
#[test]
fn last_visited_child_is_declaration_list() {
    let r = parse("let a = 1;");
    let stmt = first_statement(&r.arena, r.source_file);
    let last = get_last_visited_child(&r.arena, stmt).unwrap();
    assert_eq!(r.arena.kind(last), Kind::VariableDeclarationList);
}

// Go: internal/ls/lsutil/children.go:GetLastVisitedChild
#[test]
fn last_visited_child_none_for_leaf_token() {
    // An identifier has no visited children.
    let r = parse("a;");
    let expr_stmt = first_statement(&r.arena, r.source_file);
    // The identifier `a` is nested; find it.
    let ident = {
        let mut found = None;
        fn go(a: &NodeArena, id: NodeId, out: &mut Option<NodeId>) {
            if out.is_some() {
                return;
            }
            if a.kind(id) == Kind::Identifier {
                *out = Some(id);
                return;
            }
            a.for_each_child(id, &mut |c| {
                go(a, c, out);
                out.is_some()
            });
        }
        go(&r.arena, expr_stmt, &mut found);
        found.unwrap()
    };
    assert_eq!(get_last_visited_child(&r.arena, ident), None);
}

// Go: internal/ls/lsutil/children.go:GetLastChild
#[test]
fn last_child_is_trailing_semicolon() {
    let text = "let a = 1;";
    let r = parse(text);
    let stmt = first_statement(&r.arena, r.source_file);
    let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    let last = get_last_child(&mut sf, stmt).unwrap();
    assert_eq!(sf.arena().kind(last), Kind::SemicolonToken);
}

// Go: internal/ls/lsutil/children.go:GetLastChild
#[test]
fn last_child_without_trailing_token_is_last_visited() {
    // No trailing `;`: the last child is the declaration list itself.
    let text = "let a = 1";
    let r = parse(text);
    let stmt = first_statement(&r.arena, r.source_file);
    let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    let last = get_last_child(&mut sf, stmt).unwrap();
    assert_eq!(sf.arena().kind(last), Kind::VariableDeclarationList);
}

// Go: internal/ls/lsutil/children.go:GetLastToken
#[test]
fn last_token_descends_to_semicolon() {
    let text = "let a = 1;";
    let r = parse(text);
    let stmt = first_statement(&r.arena, r.source_file);
    let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    let last = get_last_token(&mut sf, stmt).unwrap();
    assert_eq!(sf.arena().kind(last), Kind::SemicolonToken);
}

// Go: internal/ls/lsutil/children.go:GetLastToken
#[test]
fn last_token_none_for_identifier() {
    let text = "a;";
    let r = parse(text);
    let ident = {
        let mut found = None;
        fn go(a: &NodeArena, id: NodeId, out: &mut Option<NodeId>) {
            if out.is_some() {
                return;
            }
            if a.kind(id) == Kind::Identifier {
                *out = Some(id);
                return;
            }
            a.for_each_child(id, &mut |c| {
                go(a, c, out);
                out.is_some()
            });
        }
        go(&r.arena, r.source_file, &mut found);
        found.unwrap()
    };
    let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    assert_eq!(get_last_token(&mut sf, ident), None);
}

// Go: internal/ls/lsutil/children.go:GetFirstToken
#[test]
fn first_token_is_let_keyword() {
    let text = "let a = 1;";
    let r = parse(text);
    let stmt = first_statement(&r.arena, r.source_file);
    let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    let first = get_first_token(&mut sf, stmt).unwrap();
    assert_eq!(sf.arena().kind(first), Kind::LetKeyword);
}

// Go: internal/ls/lsutil/children.go:GetFirstToken
#[test]
fn first_token_none_for_identifier() {
    let text = "a;";
    let r = parse(text);
    let ident = {
        let mut found = None;
        fn go(a: &NodeArena, id: NodeId, out: &mut Option<NodeId>) {
            if out.is_some() {
                return;
            }
            if a.kind(id) == Kind::Identifier {
                *out = Some(id);
                return;
            }
            a.for_each_child(id, &mut |c| {
                go(a, c, out);
                out.is_some()
            });
        }
        go(&r.arena, r.source_file, &mut found);
        found.unwrap()
    };
    let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    assert_eq!(get_first_token(&mut sf, ident), None);
}

// Go: internal/ls/lsutil/children.go:GetOrCreateToken (pointer-equality via cache)
#[test]
fn last_token_is_cached_stable() {
    let text = "let a = 1;";
    let r = parse(text);
    let stmt = first_statement(&r.arena, r.source_file);
    let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    let a = get_last_token(&mut sf, stmt).unwrap();
    let b = get_last_token(&mut sf, stmt).unwrap();
    assert_eq!(
        a, b,
        "repeated queries must return the same synthesized token"
    );
}
