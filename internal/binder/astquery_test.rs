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

/// Returns the first member declaration of an interface/class statement.
fn first_member(arena: &tsgo_ast::NodeArena, sf: NodeId) -> NodeId {
    match arena.data(statements(arena, sf)[0]) {
        NodeData::InterfaceDeclaration(d) | NodeData::ClassDeclaration(d) => d.members.nodes[0],
        _ => unreachable!(),
    }
}

// Go: internal/ast/utilities.go:IsDynamicName (well-known symbol computed name)
#[test]
fn is_dynamic_name_well_known_symbol_true() {
    let (arena, sf) = parse("interface I { [Symbol.iterator](): void }");
    let name = name_of_declaration(&arena, first_member(&arena, sf)).unwrap();
    assert!(is_dynamic_name(&arena, name));
}

// Go: internal/ast/utilities.go:IsDynamicName (literal computed name is not dynamic)
#[test]
fn is_dynamic_name_literal_false() {
    let (arena, sf) = parse("class C { [\"foo\"]: number }");
    let name = name_of_declaration(&arena, first_member(&arena, sf)).unwrap();
    assert!(!is_dynamic_name(&arena, name));
}

// Go: internal/ast/utilities.go:IsDynamicName (plain identifier name is not dynamic)
#[test]
fn is_dynamic_name_identifier_false() {
    let (arena, sf) = parse("function f(){}");
    let name = name_of_declaration(&arena, statements(&arena, sf)[0]).unwrap();
    assert!(!is_dynamic_name(&arena, name));
}

// Go: internal/ast/utilities.go:HasDynamicName
#[test]
fn has_dynamic_name_computed_method_true() {
    let (arena, sf) = parse("interface I { [Symbol.iterator](): void }");
    assert!(has_dynamic_name(&arena, first_member(&arena, sf)));
}

// Go: internal/ast/utilities.go:HasDynamicName (literal-named member is not dynamic)
#[test]
fn has_dynamic_name_literal_member_false() {
    let (arena, sf) = parse("class C { [\"foo\"]: number }");
    assert!(!has_dynamic_name(&arena, first_member(&arena, sf)));
}

// ── CommonJS assignment-declaration predicates (Round 8) ─────────────────────

/// Parses `src` as a `.js` file and returns `(arena, source_file)`.
fn parse_js(src: &str) -> (tsgo_ast::NodeArena, NodeId) {
    let r = parse_source_file(
        SourceFileParseOptions {
            file_name: "a.js".to_string(),
        },
        src,
        ScriptKind::Js,
    );
    (r.arena, r.source_file)
}

/// Returns the expression of the first expression statement.
fn first_expression(arena: &tsgo_ast::NodeArena, sf: NodeId) -> NodeId {
    match arena.data(statements(arena, sf)[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => unreachable!(),
    }
}

// Go: internal/ast/utilities.go:IsInJSFile
#[test]
fn is_in_js_file_true_for_js_false_for_ts() {
    let (js_arena, js_sf) = parse_js("module.exports = {};");
    assert!(is_in_js_file(&js_arena, js_sf));
    let (ts_arena, ts_sf) = parse("var x = 1;");
    assert!(!is_in_js_file(&ts_arena, ts_sf));
}

// Go: internal/ast/utilities.go:IsRequireCall
#[test]
fn is_require_call_recognizes_single_arg_require() {
    let (arena, sf) = parse_js("require('y');");
    assert!(is_require_call(&arena, first_expression(&arena, sf)));
    // Wrong callee / arity are rejected.
    let (a2, s2) = parse_js("notRequire('y');");
    assert!(!is_require_call(&a2, first_expression(&a2, s2)));
    let (a3, s3) = parse_js("require('y', 'z');");
    assert!(!is_require_call(&a3, first_expression(&a3, s3)));
}

// Go: internal/ast/utilities.go:IsModuleIdentifier / IsExportsIdentifier
#[test]
fn module_and_exports_identifier_predicates() {
    let (arena, sf) = parse_js("module;");
    let module_id = first_expression(&arena, sf);
    assert!(is_module_identifier(&arena, module_id));
    assert!(!is_exports_identifier(&arena, module_id));

    let (a2, s2) = parse_js("exports;");
    let exports_id = first_expression(&a2, s2);
    assert!(is_exports_identifier(&a2, exports_id));
    assert!(!is_module_identifier(&a2, exports_id));
}

// Go: internal/ast/utilities.go:IsModuleExportsAccessExpression
#[test]
fn is_module_exports_access_expression_true() {
    let (arena, sf) = parse_js("module.exports;");
    assert!(is_module_exports_access_expression(
        &arena,
        first_expression(&arena, sf)
    ));
    // `exports` alone (or `module.foo`) is not the `module.exports` access.
    let (a2, s2) = parse_js("module.foo;");
    assert!(!is_module_exports_access_expression(
        &a2,
        first_expression(&a2, s2)
    ));
}

// Go: internal/ast/utilities.go:GetElementOrPropertyAccessName
#[test]
fn get_element_or_property_access_name_property_and_element() {
    let (arena, sf) = parse_js("a.b;");
    let name = get_element_or_property_access_name(&arena, first_expression(&arena, sf))
        .expect("property access has a name");
    assert_eq!(arena.text(name), "b");

    let (a2, s2) = parse_js("a[1];");
    let elem_name = get_element_or_property_access_name(&a2, first_expression(&a2, s2))
        .expect("numeric element access has a literal name");
    assert_eq!(a2.text(elem_name), "1");
}

// Go: internal/ast/utilities.go:GetAssignmentDeclarationKind (module.exports = X)
#[test]
fn get_assignment_declaration_kind_module_exports() {
    let (arena, sf) = parse_js("module.exports = {};");
    assert_eq!(
        get_assignment_declaration_kind(&arena, first_expression(&arena, sf)),
        JsDeclarationKind::ModuleExports
    );
}

// Go: internal/ast/utilities.go:GetAssignmentDeclarationKind (exports.x / module.exports.x)
#[test]
fn get_assignment_declaration_kind_exports_property() {
    let (arena, sf) = parse_js("exports.foo = 1;");
    assert_eq!(
        get_assignment_declaration_kind(&arena, first_expression(&arena, sf)),
        JsDeclarationKind::ExportsProperty
    );
    let (a2, s2) = parse_js("module.exports.foo = 1;");
    assert_eq!(
        get_assignment_declaration_kind(&a2, first_expression(&a2, s2)),
        JsDeclarationKind::ExportsProperty
    );
    let (a3, s3) = parse_js("exports[1] = 2;");
    assert_eq!(
        get_assignment_declaration_kind(&a3, first_expression(&a3, s3)),
        JsDeclarationKind::ExportsProperty
    );
}

// Go: internal/ast/utilities.go:GetAssignmentDeclarationKind (F.x expando / this.x)
#[test]
fn get_assignment_declaration_kind_property_and_this() {
    let (arena, sf) = parse_js("f.x = 1;");
    assert_eq!(
        get_assignment_declaration_kind(&arena, first_expression(&arena, sf)),
        JsDeclarationKind::Property
    );
    let (a2, s2) = parse_js("this.x = 1;");
    assert_eq!(
        get_assignment_declaration_kind(&a2, first_expression(&a2, s2)),
        JsDeclarationKind::ThisProperty
    );
}

// Go: internal/ast/utilities.go:GetAssignmentDeclarationKind
// (`module.exports = exports` is NOT a module-exports declaration; a TS file is
// never a JS assignment declaration; a non-assignment is None.)
#[test]
fn get_assignment_declaration_kind_none_cases() {
    // `module.exports = exports` is explicitly excluded from ModuleExports.
    let (arena, sf) = parse_js("module.exports = exports;");
    assert_ne!(
        get_assignment_declaration_kind(&arena, first_expression(&arena, sf)),
        JsDeclarationKind::ModuleExports
    );
    // In a `.ts` file the JS-only `ModuleExports`/`ExportsProperty` branches
    // never fire, but the (non-JS-gated) expando branch still classifies
    // `module.exports = {}` as a plain `Property` assignment — which the binder
    // dispatches to the deferred expando handler, so NO CommonJS indicator is
    // set (matching Go; the `ts_module_exports_assignment_does_not_declare_*`
    // guard confirms `module`/`exports` are not injected).
    let (a2, s2) = parse("module.exports = {};");
    assert_eq!(
        get_assignment_declaration_kind(&a2, first_expression(&a2, s2)),
        JsDeclarationKind::Property
    );
    // A non-binary expression is None.
    let (a3, s3) = parse_js("module;");
    assert_eq!(
        get_assignment_declaration_kind(&a3, first_expression(&a3, s3)),
        JsDeclarationKind::None
    );
}
