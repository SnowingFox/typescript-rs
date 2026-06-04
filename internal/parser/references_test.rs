use super::references::*;
use super::*;

fn parse(src: &str) -> ParseResult {
    parse_source_file(
        SourceFileParseOptions {
            file_name: "test.ts".into(),
        },
        src,
        tsgo_core::scriptkind::ScriptKind::Ts,
    )
}

#[test]
fn test_is_any_import_or_reexport_import_decl() {
    let r = parse("import { x } from 'mod';");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    assert!(is_any_import_or_reexport(&r.arena, stmts.nodes[0]));
}

#[test]
fn test_is_any_import_or_reexport_export_decl() {
    let r = parse("export { x } from 'mod';");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    assert!(is_any_import_or_reexport(&r.arena, stmts.nodes[0]));
}

#[test]
fn test_is_any_import_or_reexport_variable_stmt() {
    let r = parse("const x = 1;");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    assert!(!is_any_import_or_reexport(&r.arena, stmts.nodes[0]));
}

#[test]
fn test_get_external_module_name_import() {
    let r = parse("import { x } from 'my-module';");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    let name = get_external_module_name(&r.arena, stmts.nodes[0]);
    assert!(name.is_some());
    let name_id = name.unwrap();
    assert_eq!(r.arena.kind(name_id), tsgo_ast::Kind::StringLiteral);
}

#[test]
fn test_get_external_module_name_export() {
    let r = parse("export { x } from 'other';");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    let name = get_external_module_name(&r.arena, stmts.nodes[0]);
    assert!(name.is_some());
}

#[test]
fn test_get_external_module_name_no_specifier() {
    let r = parse("export { x };");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    let name = get_external_module_name(&r.arena, stmts.nodes[0]);
    assert!(name.is_none());
}

#[test]
fn test_is_ambient_module_string_literal_name() {
    let r = parse("declare module 'foo' { }");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    assert!(is_ambient_module(&r.arena, stmts.nodes[0]));
}

#[test]
fn test_is_ambient_module_identifier_name() {
    let r = parse("declare module Foo { }");
    let stmts = match r.arena.data(r.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements,
        _ => unreachable!(),
    };
    assert!(!is_ambient_module(&r.arena, stmts.nodes[0]));
}

#[test]
fn test_collect_external_module_refs_simple_import() {
    let r = parse("import { x } from 'my-mod'; const y = 1;");
    let refs = collect_external_module_references(&r.arena, r.source_file);
    assert_eq!(refs.imports.len(), 1);
    let imp_id = refs.imports[0];
    assert_eq!(r.arena.kind(imp_id), tsgo_ast::Kind::StringLiteral);
}

#[test]
fn test_collect_external_module_refs_multiple_imports() {
    let r = parse("import 'a'; import 'b'; export * from 'c';");
    let refs = collect_external_module_references(&r.arena, r.source_file);
    assert_eq!(refs.imports.len(), 3);
}

#[test]
fn test_collect_external_module_refs_no_imports() {
    let r = parse("const x = 1;");
    let refs = collect_external_module_references(&r.arena, r.source_file);
    assert!(refs.imports.is_empty());
}
