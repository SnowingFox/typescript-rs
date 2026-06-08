use std::rc::Rc;

use tsgo_ast::{Kind, NodeData, NodeId};

use super::*;
use crate::core::test_support::StubProgram;

fn child_nodes(arena: &tsgo_ast::NodeArena, root: NodeId) -> Vec<NodeId> {
    match arena.data(root) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        NodeData::ModuleBlock(d) => d.statements.nodes.clone(),
        NodeData::ModuleDeclaration(d) => d.body.into_iter().collect(),
        NodeData::EnumDeclaration(d) => d.members.nodes.clone(),
        NodeData::Block(d) => d.list.nodes.clone(),
        NodeData::ImportDeclaration(d) => {
            let mut nodes = Vec::new();
            if let Some(c) = d.import_clause {
                nodes.push(c);
            }
            nodes.push(d.module_specifier);
            nodes
        }
        NodeData::VariableStatement(d) => match arena.data(d.declaration_list) {
            NodeData::VariableDeclarationList(vdl) => vdl.declarations.nodes.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn first_node_of_kind(arena: &tsgo_ast::NodeArena, root: NodeId, kind: Kind) -> Option<NodeId> {
    if arena.kind(root) == kind {
        return Some(root);
    }
    for child in child_nodes(arena, root) {
        if let Some(found) = first_node_of_kind(arena, child, kind) {
            return Some(found);
        }
    }
    None
}

// Go: internal/checker/checker.go:Checker.computeEnumMemberValues (computed name)
#[test]
fn enum_computed_property_name_reports_1164() {
    let p = StubProgram::parse_and_bind("/a.ts", "enum E { [x] = 1 }");
    let enum_decl =
        first_node_of_kind(p.arena(), p.root(), Kind::EnumDeclaration).expect("enum declaration");
    let mut c = Checker::new();
    c.compute_enum_member_values(&p, enum_decl);
    let diags = c.get_diagnostics(p.root());
    let d = diags.iter().find(|d| d.code == 1164);
    assert!(
        d.is_some(),
        "expected TS1164 for computed enum member name; got: {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.computeEnumMemberValues (numeric name)
#[test]
fn enum_numeric_member_name_reports_2452() {
    let p = StubProgram::parse_and_bind("/a.ts", "enum E { 0 = 1 }");
    let enum_decl =
        first_node_of_kind(p.arena(), p.root(), Kind::EnumDeclaration).expect("enum declaration");
    let mut c = Checker::new();
    c.compute_enum_member_values(&p, enum_decl);
    let diags = c.get_diagnostics(p.root());
    let d = diags.iter().find(|d| d.code == 2452);
    assert!(
        d.is_some(),
        "expected TS2452 for numeric enum member name; got: {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkEnumDeclaration (merged omit-init)
#[test]
fn merged_enum_duplicate_omit_initializer_reports_2432() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A }\nenum E { B }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2432);
    assert!(
        d.is_some(),
        "expected TS2432 for merged enum omitting two first-member initializers; got: {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkExternalImportOrExportDeclaration
#[test]
fn import_in_namespace_with_module_specifier_reports_1147() {
    let p = StubProgram::parse_and_bind("/a.ts", "namespace N { import x from \"m\"; }");
    let import_decl = first_node_of_kind(p.arena(), p.root(), Kind::ImportDeclaration)
        .expect("import declaration");
    let mut c = Checker::new();
    c.check_import_declaration(&p, import_decl);
    let diags = c.get_diagnostics(p.root());
    let d = diags.iter().find(|d| d.code == 1147);
    assert!(
        d.is_some(),
        "expected TS1147 for import with module specifier inside namespace; got: {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkImportDeclaration
#[test]
fn import_inside_block_reports_1232() {
    let p = StubProgram::parse_and_bind("/a.ts", "{\n  import x from \"m\";\n}");
    let import_decl = first_node_of_kind(p.arena(), p.root(), Kind::ImportDeclaration)
        .expect("import declaration");
    let mut c = Checker::new();
    c.check_import_declaration(&p, import_decl);
    let diags = c.get_diagnostics(p.root());
    let d = diags.iter().find(|d| d.code == 1232);
    assert!(
        d.is_some(),
        "expected TS1232 for import outside module context; got: {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkExportDeclaration
#[test]
fn export_in_namespace_without_specifier_reports_1194() {
    let p = StubProgram::parse_and_bind("/a.ts", "namespace N { export { x }; }");
    let export_decl = first_node_of_kind(p.arena(), p.root(), Kind::ExportDeclaration)
        .expect("export declaration");
    let mut c = Checker::new();
    c.check_export_declaration(&p, export_decl);
    let diags = c.get_diagnostics(p.root());
    let d = diags.iter().find(|d| d.code == 1194);
    assert!(
        d.is_some(),
        "expected TS1194 for export in namespace; got: {diags:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarImportClause
#[test]
fn type_only_import_default_and_named_reports_1363() {
    let p = StubProgram::parse_and_bind("/a.ts", "import type foo, { bar } from \"m\";");
    let import_decl = first_node_of_kind(p.arena(), p.root(), Kind::ImportDeclaration)
        .expect("import declaration");
    let import_clause = match p.arena().data(import_decl) {
        NodeData::ImportDeclaration(d) => d.import_clause.expect("import clause"),
        _ => panic!("import declaration"),
    };
    let mut c = Checker::new();
    assert!(
        c.check_grammar_import_clause(&p, import_clause),
        "expected grammar error for type-only import with default and named bindings"
    );
    let diags = c.get_diagnostics(p.root());
    let d = diags.iter().find(|d| d.code == 1363);
    assert!(
        d.is_some(),
        "expected TS1363 for type-only import with both default and named; got: {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue(23876)
#[test]
fn const_enum_forward_ref_initializer_reports_2474() {
    let p = StubProgram::parse_and_bind("/a.ts", "const enum E { A = E.B, B = 1 }");
    let enum_decl =
        first_node_of_kind(p.arena(), p.root(), Kind::EnumDeclaration).expect("enum declaration");
    let mut c = Checker::new();
    c.compute_enum_member_values(&p, enum_decl);
    let diags = c.get_diagnostics(p.root());
    assert!(
        diags.iter().any(|d| d.code == 2474),
        "expected TS2474 for non-constant const enum initializer; got: {diags:?}"
    );
}
