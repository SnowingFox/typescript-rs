//! Unit tests for the binder-local AST query helpers.

use super::*;
use tsgo_ast::{NodeData, NodeId};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

fn parse(src: &str) -> (tsgo_ast::NodeArena, NodeId) {
    let r = parse_source_file(SourceFileParseOptions::default(), src, ScriptKind::Ts);
    (r.arena, r.source_file)
}

fn statements(arena: &tsgo_ast::NodeArena, sf: NodeId) -> Vec<NodeId> {
    match arena.data(sf) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => unreachable!(),
    }
}

// Go: internal/ast/utilities.go:GetNameOfDeclaration
#[test]
fn name_of_declaration_function() {
    let (arena, sf) = parse("function f(){}");
    let func = statements(&arena, sf)[0];
    let name = name_of_declaration(&arena, func).expect("function has a name");
    assert_eq!(arena.text(name), "f");
}

// Go: internal/ast/utilities.go:IsPropertyNameLiteral
#[test]
fn is_property_name_literal_identifier() {
    let (arena, sf) = parse("function f(){}");
    let func = statements(&arena, sf)[0];
    let name = name_of_declaration(&arena, func).unwrap();
    assert!(is_property_name_literal(&arena, name));
}

// Go: internal/ast/utilities.go:IsBlockOrCatchScoped
#[test]
fn is_block_or_catch_scoped_let() {
    let (arena, sf) = parse("let x = 1;");
    let var_stmt = statements(&arena, sf)[0];
    let list = match arena.data(var_stmt) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => unreachable!(),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => unreachable!(),
    };
    assert!(is_block_or_catch_scoped(&arena, decl));
}

// Go: internal/ast/utilities.go:IsPotentiallyExecutableNode
#[test]
fn is_potentially_executable_let_statement() {
    let (arena, sf) = parse("let x;");
    let var_stmt = statements(&arena, sf)[0];
    assert!(is_potentially_executable_node(&arena, var_stmt));
}

// Go: internal/scanner/utilities.go:DeclarationNameToString
#[test]
fn declaration_name_to_string_identifier() {
    let (arena, sf) = parse("function f(){}");
    let func = statements(&arena, sf)[0];
    let name = name_of_declaration(&arena, func).unwrap();
    assert_eq!(declaration_name_to_string(&arena, name), "f");
}
