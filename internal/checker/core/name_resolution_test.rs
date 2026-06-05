use super::{get_spelling_suggestion_for_name, mark_as_synthetic_name, node_is_synthesized};
use crate::core::program::BoundProgram;
use crate::core::test_support::{MultiFileProgram, StubProgram};
use crate::core::Checker;
use tsgo_ast::NodeData;

// Go: internal/checker/checker.go:Checker.checkResolvedBlockScopedVariable
#[test]
fn block_scoped_let_used_before_declaration_reports_2448() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "{ x; let x = 1; }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2448
            && d.message == "Block-scoped variable 'x' used before its declaration."),
        "expected TS2448: {diags:?}",
    );
}

// Go: internal/checker/checker.go:Checker.checkResolvedBlockScopedVariable
#[test]
fn class_used_before_declaration_reports_2449() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "{ new C(); class C {} }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2449),
        "expected TS2449: {diags:?}",
    );
}

// Go: internal/checker/checker.go:Checker.getSuggestedSymbolForNonexistentModule
#[test]
fn namespace_typo_suggests_closest_export_member() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { export const foo = 1; }\nN.fooo;",
    ));
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let access = match arena.data(stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let mut c = Checker::new_checker(p.clone());
    let _ = c.resolve_entity_name(
        p.as_ref(),
        access,
        tsgo_ast::SymbolFlags::VALUE,
        false,
        false,
        None,
    );
    let diags = c.peek_recorded_diagnostics(p.root());
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2724 && d.message.contains("Did you mean 'foo'?")),
        "expected TS2724: {diags:?}",
    );
}

// Go: internal/ast/utilities.go:IsValidTypeOnlyAliasUseSite
#[test]
fn type_only_alias_value_use_reports_1361() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/m.ts", "export type T = number;"),
        ("/a.ts", "import type { T } from \"./m\";\nT;"),
    ]));
    let file_a = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(file_a);
    assert!(
        diags.iter().any(|d| d.code == 1361),
        "expected TS1361: {diags:?}",
    );
}

// Go: internal/ast/utilities.go:NodeIsSynthesized
#[test]
fn node_is_synthesized_detects_negative_positions() {
    let mut arena = tsgo_ast::NodeArena::new();
    let id = arena.new_identifier("x");
    arena.set_loc(id, tsgo_core::text::TextRange::new(0, 1));
    assert!(!node_is_synthesized(&arena, id));
    mark_as_synthetic_name(&mut arena, id);
    assert!(node_is_synthesized(&arena, id));
}

// Go: internal/checker/checker.go:Checker.getSpellingSuggestionForName
#[test]
fn get_spelling_suggestion_for_name_picks_closest_export() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { export const alpha = 1; export const beta = 2; }",
    );
    let n_sym = *p.locals(p.root()).unwrap().get("N").unwrap();
    let exports: Vec<_> = p.symbol(n_sym).exports.values().copied().collect();
    let suggested = get_spelling_suggestion_for_name(
        &p,
        "alfha",
        &exports,
        tsgo_ast::SymbolFlags::MODULE_MEMBER,
    )
    .unwrap();
    assert_eq!(p.symbol(suggested).name, "alpha");
}

// Go: internal/checker/checker.go:Checker.resolveEntityName
#[test]
fn resolve_entity_name_uses_specialized_cannot_find_name_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "process;"));
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let process_ref = match arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let mut c = Checker::new_checker(p.clone());
    let _ = c.resolve_entity_name(
        p.as_ref(),
        process_ref,
        tsgo_ast::SymbolFlags::VALUE,
        false,
        false,
        None,
    );
    let diags = c.get_diagnostics(p.root());
    assert!(
        diags.iter().any(|d| d.code == 2591),
        "expected node-hint 2591 for process: {diags:?}",
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier
#[test]
fn let_declared_after_use_in_function_reports_2448_via_check_identifier() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { x; let x = 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2448),
        "expected TS2448 via check_identifier: {diags:?}",
    );
}
