use crate::core::emit_resolver::{SerializedTypeNode, TypeReferenceSerializationKind};
use crate::core::program::BoundProgram;
use crate::core::symbols_query::get_symbol_of_declaration;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::{NodeData, NodeId};
use tsgo_evaluator::EvalValue;

// Returns the `idx`-th top-level statement node.
fn statement(p: &StubProgram, idx: usize) -> NodeId {
    match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[idx],
        _ => panic!("source file"),
    }
}

// Returns the statement nodes of a function declaration's body block.
fn function_body_statements(p: &StubProgram, func: NodeId) -> Vec<NodeId> {
    let arena = p.arena();
    let body = match arena.data(func) {
        NodeData::FunctionDeclaration(d) => d.body.expect("function has a body"),
        _ => panic!("function declaration"),
    };
    match arena.data(body) {
        NodeData::Block(d) => d.list.nodes.clone(),
        _ => panic!("function body block"),
    }
}

// Returns the first `VariableDeclaration` of a `VariableStatement` node.
fn var_decl_of_statement(p: &StubProgram, stmt: NodeId) -> NodeId {
    let arena = p.arena();
    let list = match arena.data(stmt) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    match arena.data(list) {
        NodeData::VariableDeclarationList(l) => l.declarations.nodes[0],
        _ => panic!("variable declaration list"),
    }
}

// Go: internal/checker/emitresolver.go:EmitResolver.determineIfDeclarationIsVisible
// (a `VariableDeclaration`'s visibility uses its COMBINED modifier flags, which
// fold in the wrapping `VariableStatement`'s `export`, then recurses on the
// declaration container — so `export const x = 1`'s declaration is visible).
#[test]
fn is_declaration_visible_for_exported_variable_declaration() {
    // `export const x = 1;` (exported) vs `const y = 2;` (non-exported local).
    let p = StubProgram::parse_and_bind("/a.ts", "export const x = 1;\nconst y = 2;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    // The `export` lives on the VariableStatement; the VariableDeclaration `x`
    // inherits it via combined modifier flags, so the declaration is visible.
    let x = first_var_declaration(&p, 0);
    assert!(resolver.is_declaration_visible(&p, x));
    // A non-exported local declaration is not visible (the reachable subset
    // conservatively treats the file as a module; the global-script-file case is
    // DEFER, blocked-by external-module detection).
    let y = first_var_declaration(&p, 1);
    assert!(!resolver.is_declaration_visible(&p, y));
}

// Go: internal/checker/emitresolver.go:EmitResolver.IsDeclarationVisible (tracer)
#[test]
fn exported_declaration_is_visible() {
    let p = StubProgram::parse_and_bind("/a.ts", "export function f() {}\nfunction g() {}");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let f = statement(&p, 0);
    let g = statement(&p, 1);
    // The exported `f` is visible to declaration emit; the bare `g` is not
    // (the file is an external module because of `export function f`).
    assert!(resolver.is_declaration_visible(&p, f));
    assert!(!resolver.is_declaration_visible(&p, g));
}

// Go: internal/checker/emitresolver.go:EmitResolver.determineIfDeclarationIsVisible
// (the non-exported branch returns `IsGlobalSourceFile(parent)`). In a SCRIPT
// (no `import`/`export`, so no external-module indicator) every top-level
// declaration is a global and therefore visible to declaration emit, even
// without an `export` modifier.
#[test]
fn nonexported_declaration_is_visible_in_a_script() {
    // No module syntax -> the file is a global script.
    let p = StubProgram::parse_and_bind("/a.ts", "function g() {}\nconst y = 2;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let g = statement(&p, 0);
    assert!(resolver.is_declaration_visible(&p, g));
    let y = first_var_declaration(&p, 1);
    assert!(resolver.is_declaration_visible(&p, y));
}

// In a MODULE (here made one by a top-level `export`), a non-exported top-level
// declaration is NOT a global, so it is not visible to declaration emit.
#[test]
fn nonexported_declaration_is_not_visible_in_a_module() {
    let p = StubProgram::parse_and_bind("/a.ts", "export const e = 1;\nfunction g() {}");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let g = statement(&p, 1);
    assert!(!resolver.is_declaration_visible(&p, g));
}

// Go: internal/checker/checker.go:Checker.resolveName (innermost scope wins)
#[test]
fn resolve_reference_picks_innermost_shadowing_declaration() {
    // An outer/global `a`, and an inner function-scoped `a` that shadows it;
    // the use `a;` inside `f` must resolve to the inner declaration.
    let p =
        StubProgram::parse_and_bind("/a.ts", "var a = 1;\nfunction f() {\n  var a = 2;\n  a;\n}");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();

    // The outer (global) `var a` declaration symbol (top-level statement 0).
    let outer_decl = var_decl_of_statement(&p, statement(&p, 0));
    let outer_sym = get_symbol_of_declaration(&p, outer_decl).expect("outer a symbol");

    // Inside `f`'s body: the inner `var a` (stmt 0) and the use `a;` (stmt 1).
    let body = function_body_statements(&p, statement(&p, 1));
    let inner_decl = var_decl_of_statement(&p, body[0]);
    let inner_sym = get_symbol_of_declaration(&p, inner_decl).expect("inner a symbol");
    let use_a = match p.arena().data(body[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected `a;` expression statement"),
    };

    assert_ne!(
        inner_sym, outer_sym,
        "inner and outer `a` are distinct symbols"
    );
    // The use resolves to the inner (shadowing) `a`, not the outer/global one.
    assert_eq!(resolver.resolve_reference(&p, use_a), Some(inner_sym));
}

// Returns the expression of an `ExpressionStatement` (e.g. the use `x` in `x;`).
fn expression_statement_expr(p: &StubProgram, stmt: NodeId) -> NodeId {
    match p.arena().data(stmt) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected an expression statement"),
    }
}

// Go: internal/binder/referenceresolver.go:GetReferencedExportContainer
// (a use of a top-level exported *variable* binding returns the source file)
#[test]
fn get_referenced_export_container_source_file_for_exported_value_use() {
    // `export const x = 1; x;` — the use `x` resolves to a top-level export of
    // the current module, so its container is the source file (the node the
    // CommonJS transform qualifies as `exports.x`).
    let p = StubProgram::parse_and_bind("/a.ts", "export const x = 1;\nx;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let use_x = expression_statement_expr(&p, statement(&p, 1));
    assert_eq!(
        resolver.get_referenced_export_container(&p, use_x, false),
        Some(p.root())
    );
}

// Go: internal/binder/referenceresolver.go:GetReferencedExportContainer
// (an exported *function* binding owns a local non-variable declaration, so the
// `ExportHasLocal && !Variable` guard returns None when prefix_locals is false)
#[test]
fn get_referenced_export_container_none_for_exported_function_use() {
    // `export function f() {} f;` — `f` is exported but its kind is
    // `ExportHasLocal` and not a variable, so a non-prefixed use is not
    // rewritten to `exports.f` (Go returns nil).
    let p = StubProgram::parse_and_bind("/a.ts", "export function f() {}\nf;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let use_f = expression_statement_expr(&p, statement(&p, 1));
    assert_eq!(
        resolver.get_referenced_export_container(&p, use_f, false),
        None
    );
}

// Go: internal/binder/referenceresolver.go:GetReferencedExportContainer
// (a non-exported top-level local has no export container)
#[test]
fn get_referenced_export_container_none_for_non_exported_local() {
    // `const y = 1; y;` (a script file) — `y` is a plain local, not an export,
    // so it has no export container.
    let p = StubProgram::parse_and_bind("/a.ts", "const y = 1;\ny;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let use_y = expression_statement_expr(&p, statement(&p, 1));
    assert_eq!(
        resolver.get_referenced_export_container(&p, use_y, false),
        None
    );
}

// Go: internal/binder/referenceresolver.go:GetReferencedExportContainer
// (resolution is scope-correct: a use shadowed by an inner local resolves to
// that local, not the outer export, so there is no export container)
#[test]
fn get_referenced_export_container_none_for_shadowing_local() {
    // The outer `x` is an export, but the inner `x` (a function local) shadows
    // it at the use site, so the use resolves to the non-exported inner binding.
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "export const x = 1;\nfunction f() {\n  const x = 2;\n  x;\n}",
    );
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let body = function_body_statements(&p, statement(&p, 1));
    let inner_use_x = expression_statement_expr(&p, body[1]);
    assert_eq!(
        resolver.get_referenced_export_container(&p, inner_use_x, false),
        None
    );
}

// Navigates `import { <spec> } from "m";` (statement 0) to its first named
// import specifier node.
fn first_named_import_specifier(p: &StubProgram) -> NodeId {
    let arena = p.arena();
    let import_clause = match arena.data(statement(p, 0)) {
        NodeData::ImportDeclaration(d) => d.import_clause.expect("import clause"),
        _ => panic!("import declaration"),
    };
    let named_bindings = match arena.data(import_clause) {
        NodeData::ImportClause(d) => d.named_bindings.expect("named bindings"),
        _ => panic!("import clause"),
    };
    match arena.data(named_bindings) {
        NodeData::NamedImports(d) => d.elements.nodes[0],
        _ => panic!("named imports"),
    }
}

// Go: internal/checker/checker.go:Checker.isReferenced (used import binding)
#[test]
fn is_referenced_true_for_used_import_binding() {
    // `x` is imported and then used in value position.
    let p = StubProgram::parse_and_bind("/a.ts", "import { x } from \"m\";\nx;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let spec = first_named_import_specifier(&p);
    assert!(resolver.is_referenced(&p, spec));
}

// Go: internal/checker/checker.go:Checker.isReferenced (unused import binding)
#[test]
fn is_referenced_false_for_unused_import_binding() {
    // `y` is imported but never used; its own name node must not count.
    let p = StubProgram::parse_and_bind("/a.ts", "import { y } from \"m\";");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let spec = first_named_import_specifier(&p);
    assert!(!resolver.is_referenced(&p, spec));
}

// Go: internal/checker/checker.go:Checker.isReferenced (scope-correct, not a
// textual name match: a use shadowed by an inner binding of the same name does
// not reference the outer import)
#[test]
fn is_referenced_is_scope_correct_not_name_match() {
    // The only `x` use is shadowed by an inner `var x`, so the import is unused;
    // a name-match stand-in would wrongly report it as referenced.
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "import { x } from \"m\";\nfunction f() {\n  var x = 1;\n  x;\n}",
    );
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let spec = first_named_import_specifier(&p);
    assert!(!resolver.is_referenced(&p, spec));
}

// Go: internal/checker/checker.go:Checker.isReferenced + getNameOfDeclaration
// for an `ImportEqualsDeclaration` (its own binding name must be excluded from
// the use-scan, so an unused `import =` is correctly unreferenced).
#[test]
fn is_referenced_false_for_unused_import_equals() {
    // `import x = require("m");` with no use of `x`: its own binding name node
    // must not count as a reference to itself, so the import-equals is unused.
    let p = StubProgram::parse_and_bind("/a.ts", "import x = require(\"m\");");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let import_eq = statement(&p, 0);
    assert!(!resolver.is_referenced(&p, import_eq));
}

// Go: internal/checker/checker.go:Checker.isReferenced (used `import =` binding)
#[test]
fn is_referenced_true_for_used_import_equals() {
    // `import x = require("m"); x;`: the value-position use of `x` resolves to
    // the import-equals symbol, so it is referenced.
    let p = StubProgram::parse_and_bind("/a.ts", "import x = require(\"m\");\nx;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let import_eq = statement(&p, 0);
    assert!(resolver.is_referenced(&p, import_eq));
}

// Navigates `export { <spec> };` (the given statement) to its first export
// specifier node.
fn first_export_specifier(p: &StubProgram, stmt_idx: usize) -> NodeId {
    let arena = p.arena();
    let export_clause = match arena.data(statement(p, stmt_idx)) {
        NodeData::ExportDeclaration(d) => d.export_clause.expect("export clause"),
        _ => panic!("export declaration"),
    };
    match arena.data(export_clause) {
        NodeData::NamedExports(d) => d.elements.nodes[0],
        _ => panic!("named exports"),
    }
}

// Go: internal/checker/emitresolver.go:EmitResolver.isValueAliasDeclarationWorker
// (export specifier aliasing a local value)
#[test]
fn is_value_alias_declaration_true_for_exported_value() {
    // `export { f }` where `f` is a local function: the specifier aliases a
    // value, so it is a value alias.
    let p = StubProgram::parse_and_bind("/a.ts", "function f() {}\nexport { f };");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let spec = first_export_specifier(&p, 1);
    assert!(resolver.is_value_alias_declaration(&p, spec));
}

// Go: internal/checker/emitresolver.go:EmitResolver.isValueAliasDeclarationWorker
// (export specifier aliasing a type-only binding)
#[test]
fn is_value_alias_declaration_false_for_exported_type_only() {
    // `export { I }` where `I` is an interface (type-only): the specifier does
    // not alias a value, so it is not a value alias.
    let p = StubProgram::parse_and_bind("/a.ts", "interface I {}\nexport { I };");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let spec = first_export_specifier(&p, 1);
    assert!(!resolver.is_value_alias_declaration(&p, spec));
}

// Go: internal/checker/emitresolver.go:EmitResolver.isValueAliasDeclarationWorker
// (ExportAssignment arm: `export = <value identifier>`)
#[test]
fn is_value_alias_declaration_true_for_export_assignment_value() {
    // `export = f` where `f` is a local function: the export-assignment's
    // expression identifier resolves to a value, so it is a value alias.
    let p = StubProgram::parse_and_bind("/a.ts", "function f() {}\nexport = f;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let export_assign = statement(&p, 1);
    assert!(resolver.is_value_alias_declaration(&p, export_assign));
}

// Go: internal/checker/emitresolver.go:EmitResolver.isValueAliasDeclarationWorker
// (ExportAssignment arm: `export = <type-only identifier>` is not a value alias)
#[test]
fn is_value_alias_declaration_false_for_export_assignment_type_only() {
    // `export = I` where `I` is an interface (type-only): the expression
    // identifier does not resolve with value meaning, so it is not a value
    // alias (Go resolves the expression identifier via `isAliasResolvedToValue`).
    let p = StubProgram::parse_and_bind("/a.ts", "interface I {}\nexport = I;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let export_assign = statement(&p, 1);
    assert!(!resolver.is_value_alias_declaration(&p, export_assign));
}

// Go: internal/checker/emitresolver.go:EmitResolver.IsReferencedAliasDeclaration
// (referenced import binding)
#[test]
fn is_referenced_alias_declaration_true_for_used_import() {
    // `x` is imported (an alias symbol declaration) and used in value position.
    let p = StubProgram::parse_and_bind("/a.ts", "import { x } from \"m\";\nx;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let spec = first_named_import_specifier(&p);
    assert!(resolver.is_referenced_alias_declaration(&p, spec));
}

// Go: internal/checker/emitresolver.go:EmitResolver.IsReferencedAliasDeclaration
// (a referenced non-alias declaration is still not an alias declaration)
#[test]
fn is_referenced_alias_declaration_false_for_non_alias() {
    // `f` is a referenced function, not an alias symbol declaration, so it is
    // never a referenced *alias* declaration (Go's IsAliasSymbolDeclaration
    // guard).
    let p = StubProgram::parse_and_bind("/a.ts", "function f() {}\nf();");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let f = statement(&p, 0);
    assert!(resolver.is_referenced(&p, f));
    assert!(!resolver.is_referenced_alias_declaration(&p, f));
}

// Navigates a `VariableStatement` to its first `VariableDeclaration`.
fn first_var_declaration(p: &StubProgram, stmt_idx: usize) -> NodeId {
    let arena = p.arena();
    let stmt = statement(p, stmt_idx);
    let list = match arena.data(stmt) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    match arena.data(list) {
        NodeData::VariableDeclarationList(l) => l.declarations.nodes[0],
        _ => panic!("variable declaration list"),
    }
}

// Go: internal/checker/emitresolver.go:EmitResolver.SerializeTypeOfDeclaration
#[test]
fn serialize_type_of_declaration_uses_real_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Foo {}\ndeclare const x: Foo;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let x_decl = first_var_declaration(&p, 1);
    // The declaration's type serializes to the interface name (4j node builder).
    assert_eq!(
        resolver.serialize_type_of_declaration(&mut c, &p, x_decl),
        "Foo"
    );
}

// Returns the type-annotation node of the first variable declaration in the
// statement at `stmt_idx` (e.g. the `: T` type node of `declare const x: T;`).
fn var_type_annotation(p: &StubProgram, stmt_idx: usize) -> NodeId {
    let decl = first_var_declaration(p, stmt_idx);
    match p.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("type annotation"),
        _ => panic!("variable declaration"),
    }
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (tracer: the `number` keyword type serializes to the global `Number` ctor)
#[test]
fn serialize_type_node_number_keyword_is_number() {
    // `: number` (a `NumberKeyword` type node) serializes to `Number`.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: number;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Number
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (the `string` keyword type serializes to the global `String` ctor)
#[test]
fn serialize_type_node_string_keyword_is_string() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: string;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::String
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (the `boolean` keyword type serializes to the global `Boolean` ctor)
#[test]
fn serialize_type_node_boolean_keyword_is_boolean() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: boolean;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Boolean
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (`void`/`undefined`/`never` all serialize to the `void 0` ("undefined")
// expression — Go's `case KindVoidKeyword, KindUndefinedKeyword,
// KindNeverKeyword -> NewVoidZeroExpression`)
#[test]
fn serialize_type_node_void_undefined_never_are_void_zero() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: void;\ndeclare const b: undefined;\ndeclare const c: never;",
    );
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 0)),
        SerializedTypeNode::VoidZero
    );
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 1)),
        SerializedTypeNode::VoidZero
    );
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 2)),
        SerializedTypeNode::VoidZero
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// -> serializeLiteralOfLiteralTypeNode (a `null` literal type serializes to the
// `void 0` expression — Go's `case KindNullKeyword -> NewVoidZeroExpression`)
#[test]
fn serialize_type_node_null_literal_is_void_zero() {
    // `: null` parses as a `LiteralType` whose literal is the `null` keyword.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: null;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::LiteralType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::VoidZero
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (the `bigint` keyword type serializes to the global `BigInt` ctor)
#[test]
fn serialize_type_node_bigint_keyword_is_bigint() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: bigint;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::BigInt
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (the `symbol` keyword type serializes to the global `Symbol` ctor)
#[test]
fn serialize_type_node_symbol_keyword_is_symbol() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: symbol;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Symbol
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (`any`/`unknown`/`object` serialize to the global `Object` ctor — Go's
// `KindObjectKeyword` arm + the `KindAnyKeyword, KindUnknownKeyword` break
// group that falls through to the `NewIdentifier("Object")` switch tail; both
// routes converge on the conservative `Object` default in this port).
// Green-on-arrival coverage guard (no new arm; locks Go's "anything else ->
// Object" default for these kinds), not a fabricated RED.
#[test]
fn serialize_type_node_any_unknown_object_are_object() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: any;\ndeclare const b: unknown;\ndeclare const c: object;",
    );
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 0)),
        SerializedTypeNode::Object
    );
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 1)),
        SerializedTypeNode::Object
    );
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 2)),
        SerializedTypeNode::Object
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (Go applies `ast.SkipTypeParentheses(node)` *before* the switch, so a
// parenthesized type `(number)` unwraps to its inner `number` keyword and
// serializes to the global `Number` ctor — not the conservative `Object`)
#[test]
fn serialize_type_node_parenthesized_unwraps_to_inner() {
    // `: (number)` parses as a `ParenthesizedType` wrapping a `NumberKeyword`.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: (number);");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::ParenthesizedType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Number
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (`case KindTemplateLiteralType, KindStringKeyword -> NewIdentifier("String")`:
// a template literal *type* with substitutions serializes to the global
// `String` ctor)
#[test]
fn serialize_type_node_template_literal_type_is_string() {
    // `` : `a${string}b` `` (with a substitution) parses as a
    // `TemplateLiteralType` node, distinct from the `String` keyword.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: `a${string}b`;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::TemplateLiteralType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::String
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeLiteralOfLiteralTypeNode
// (`case KindStringLiteral, KindNoSubstitutionTemplateLiteral ->
// NewIdentifier("String")`: a string-literal type `"a"` serializes to `String`)
#[test]
fn serialize_type_node_string_literal_type_is_string() {
    // `: "a"` parses as a `LiteralType` whose literal is a `StringLiteral`.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: \"a\";");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::LiteralType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::String
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeLiteralOfLiteralTypeNode
// (`case KindNumericLiteral -> NewIdentifier("Number")`: a numeric-literal type
// `1` serializes to the global `Number` ctor)
#[test]
fn serialize_type_node_numeric_literal_type_is_number() {
    // `: 1` parses as a `LiteralType` whose literal is a `NumericLiteral`.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: 1;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::LiteralType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Number
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeLiteralOfLiteralTypeNode
// (`case KindTrueKeyword, KindFalseKeyword -> NewIdentifier("Boolean")`: the
// `true`/`false` literal types serialize to the global `Boolean` ctor)
#[test]
fn serialize_type_node_boolean_literal_types_are_boolean() {
    // `: true` / `: false` parse as `LiteralType`s whose literal is the
    // `true`/`false` keyword.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const a: true;\ndeclare const b: false;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 0)),
        SerializedTypeNode::Boolean
    );
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, var_type_annotation(&p, 1)),
        SerializedTypeNode::Boolean
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeLiteralOfLiteralTypeNode
// (`case KindBigIntLiteral -> NewIdentifier("BigInt")`: a bigint-literal type
// `1n` serializes to the global `BigInt` ctor)
#[test]
fn serialize_type_node_bigint_literal_type_is_bigint() {
    // `: 1n` parses as a `LiteralType` whose literal is a `BigIntLiteral`.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: 1n;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::LiteralType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::BigInt
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeLiteralOfLiteralTypeNode
// (`case KindPrefixUnaryExpression` recurses on the operand: a negative
// numeric-literal type `-1` serializes to the global `Number` ctor)
#[test]
fn serialize_type_node_negative_numeric_literal_type_is_number() {
    // `: -1` parses as a `LiteralType` whose literal is a
    // `PrefixUnaryExpression` (`-`) over a `NumericLiteral`.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: -1;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::LiteralType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Number
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (`case KindArrayType, KindTupleType -> NewIdentifier("Array")`: an array type
// `number[]` serializes to the global `Array` ctor)
#[test]
fn serialize_type_node_array_type_is_array() {
    // `: number[]` parses as an `ArrayType` node.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: number[];");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::ArrayType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Array
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (`case KindArrayType, KindTupleType -> NewIdentifier("Array")`: a tuple type
// `[number, string]` serializes to the global `Array` ctor, grouped with the
// array type)
#[test]
fn serialize_type_node_tuple_type_is_array() {
    // `: [number, string]` parses as a `TupleType` node.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: [number, string];");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::TupleType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Array
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (`case KindFunctionType, KindConstructorType -> NewIdentifier("Function")`: a
// function type `() => void` serializes to the global `Function` ctor)
#[test]
fn serialize_type_node_function_type_is_function() {
    // `: () => void` parses as a `FunctionType` node.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: () => void;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::FunctionType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Function
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode
// (`case KindFunctionType, KindConstructorType -> NewIdentifier("Function")`: a
// constructor type `new () => C` is grouped with the function type and also
// serializes to the global `Function` ctor)
#[test]
fn serialize_type_node_constructor_type_is_function() {
    // `: new () => C` parses as a `ConstructorType` node.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: new () => C;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::ConstructorType);
    assert_eq!(
        resolver.serialize_type_node_for_metadata(&p, ty),
        SerializedTypeNode::Function
    );
}

// Go: internal/checker/emitresolver.go:EmitResolver.GetTypeReferenceSerializationKind
// (tracer: a `TypeReference` to a local class resolves both as a value and as a
// type to the same class symbol — a runtime constructor — so it classifies as
// `TypeWithConstructSignatureAndValue`, the kind whose `design:type` emit is the
// class identifier itself)
#[test]
fn type_reference_to_local_class_is_construct_signature_and_value() {
    // `class C {}` then `declare const x: C;`: the `: C` type reference's entity
    // name resolves to the class `C` (which has both value and type meaning),
    // so the reference carries a runtime constructor.
    let p = StubProgram::parse_and_bind("/a.ts", "class C {}\ndeclare const x: C;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 1);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::TypeReference);
    assert_eq!(
        resolver.get_type_reference_serialization_kind(&mut c, &p, ty),
        TypeReferenceSerializationKind::TypeWithConstructSignatureAndValue
    );
}

// Go: internal/checker/emitresolver.go:EmitResolver.GetTypeReferenceSerializationKind
// (a `TypeReference` to a type-only interface resolves only as a type, not as a
// value, so it carries no runtime constructor and classifies as `ObjectType`)
#[test]
fn type_reference_to_interface_is_object_type() {
    // `interface I {}` then `declare const x: I;`: `I` has type meaning only;
    // the resolved declared type is a plain object type → `ObjectType`.
    let p = StubProgram::parse_and_bind("/a.ts", "interface I {}\ndeclare const x: I;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 1);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::TypeReference);
    assert_eq!(
        resolver.get_type_reference_serialization_kind(&mut c, &p, ty),
        TypeReferenceSerializationKind::ObjectType
    );
}

// Go: internal/checker/emitresolver.go:EmitResolver.GetTypeReferenceSerializationKind
// (a `TypeReference` to an object-literal type alias resolves only as a type;
// its declared type is a plain object type → `ObjectType`. Green-on-arrival
// coverage guard: Go classifies an interface and a type-alias-to-object
// identically through `getDeclaredTypeOfSymbol` → the `else` tail, so the S2
// arm already covers it — not a fabricated RED.)
#[test]
fn type_reference_to_type_alias_is_object_type() {
    // `type T = {};` then `declare const x: T;`: `T` has type meaning only and
    // its declared type is the anonymous object `{}` → `ObjectType`.
    let p = StubProgram::parse_and_bind("/a.ts", "type T = {};\ndeclare const x: T;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 1);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::TypeReference);
    assert_eq!(
        resolver.get_type_reference_serialization_kind(&mut c, &p, ty),
        TypeReferenceSerializationKind::ObjectType
    );
}

// Go: internal/checker/emitresolver.go:EmitResolver.GetTypeReferenceSerializationKind
// (a `TypeReference` to a name with no declaration resolves neither as a value
// nor as a type, so `resolvedTypeSymbol == nil` → `Unknown`. Green-on-arrival
// coverage guard: the conservative `Unknown` fallback already covers an
// unresolved name — not a fabricated RED.)
#[test]
fn type_reference_to_unresolved_name_is_unknown() {
    // `declare const x: Missing;`: `Missing` has no declaration in scope (and no
    // lib globals), so neither the value nor the type resolution finds it.
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: Missing;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let ty = var_type_annotation(&p, 0);
    assert_eq!(p.arena().kind(ty), tsgo_ast::Kind::TypeReference);
    assert_eq!(
        resolver.get_type_reference_serialization_kind(&mut c, &p, ty),
        TypeReferenceSerializationKind::Unknown
    );
}

// Go: internal/checker/emitresolver.go:EmitResolver.IsImplementationOfOverload
#[test]
fn implementation_of_overload_is_detected() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function foo(a: string): string;\nfunction foo(a: number): number;\nfunction foo(a: any) {\n  return a;\n}",
    );
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    // The body-bearing third `foo` is the overload implementation.
    assert!(resolver.is_implementation_of_overload(&p, statement(&p, 2)));
    // A bodyless overload signature is not.
    assert!(!resolver.is_implementation_of_overload(&p, statement(&p, 0)));

    // A plain (non-overloaded) function is not an overload implementation.
    let p2 = StubProgram::parse_and_bind("/b.ts", "function bar() {}");
    assert!(!resolver.is_implementation_of_overload(&p2, statement(&p2, 0)));
}

// Returns the `idx`-th member node of the `EnumDeclaration` at `stmt_idx`.
fn enum_member_node(p: &StubProgram, stmt_idx: usize, idx: usize) -> NodeId {
    match p.arena().data(statement(p, stmt_idx)) {
        NodeData::EnumDeclaration(d) => d.members.nodes[idx],
        _ => panic!("enum declaration"),
    }
}

// Go: internal/checker/services.go:Checker.GetConstantValue (tracer: an EnumMember
// node yields its constant value directly, regardless of const-ness — Go's first
// `node.Kind == KindEnumMember` branch returns `getEnumMemberValue(node).Value`).
#[test]
fn get_constant_value_of_enum_member_node() {
    // `const enum E { A = 10 }`: the member `A` evaluates to the number 10.
    let p = StubProgram::parse_and_bind("/a.ts", "const enum E { A = 10 }");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let a = enum_member_node(&p, 0, 0);
    assert_eq!(
        resolver.get_constant_value(&p, a),
        EvalValue::Num(10.0.into())
    );
}

// Returns the initializer expression of the first variable declaration in the
// statement at `stmt_idx` (e.g. the `E.A` of `var x = E.A;`).
fn var_initializer(p: &StubProgram, stmt_idx: usize) -> NodeId {
    let decl = var_decl_of_statement(p, statement(p, stmt_idx));
    match p.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer.expect("variable initializer"),
        _ => panic!("variable declaration"),
    }
}

// Go: internal/checker/services.go:Checker.GetConstantValue (a const-enum member
// reference `E.A` inlines to its value; a non-const member reference does not).
// Verified against `cmd/tsgo --target esnext --module preserve`:
//   `const enum E { A = 10, B = A + 5 } var x = E.A; var y = E.B;`
//     -> `var x = 10 /* E.A */; var y = 15 /* E.B */;`
//   non-const `enum E { A = 10 } var x = E.A;` keeps `var x = E.A;`.
#[test]
fn get_constant_value_of_const_enum_property_access() {
    // `const enum E { A = 10, B = A + 5 }`: `E.A` -> 10, `E.B` -> 15 (A + 5).
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const enum E { A = 10, B = A + 5 }\nvar x = E.A;\nvar y = E.B;",
    );
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let use_ea = var_initializer(&p, 1);
    let use_eb = var_initializer(&p, 2);
    assert_eq!(
        p.arena().kind(use_ea),
        tsgo_ast::Kind::PropertyAccessExpression
    );
    assert_eq!(
        resolver.get_constant_value(&p, use_ea),
        EvalValue::Num(10.0.into())
    );
    assert_eq!(
        resolver.get_constant_value(&p, use_eb),
        EvalValue::Num(15.0.into())
    );
}

// Go: internal/checker/services.go:Checker.GetConstantValue (a non-const enum
// member reference is NOT inlined — Go's `ast.IsEnumConst(member.Parent)` guard
// fails, so it returns nil and the runtime `E.A` access is preserved).
#[test]
fn get_constant_value_of_non_const_enum_property_access_is_none() {
    // `enum E { A = 10 }` (not const): `E.A` is not inlined.
    let p = StubProgram::parse_and_bind("/a.ts", "enum E { A = 10 }\nvar x = E.A;");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let use_ea = var_initializer(&p, 1);
    assert_eq!(
        p.arena().kind(use_ea),
        tsgo_ast::Kind::PropertyAccessExpression
    );
    assert_eq!(resolver.get_constant_value(&p, use_ea), EvalValue::None);
}

// ── D-F2: create_type_of_declaration + type_to_type_node ───────────────────

use crate::core::emit_resolver::LiteralConstValue;
use crate::core::nodebuilder::{SynthesizedProperty, SynthesizedTypeNode};
use tsgo_ast::Kind;

// Go: internal/checker/emitresolver.go:EmitResolver.CreateTypeOfDeclaration
// (tracer: an un-annotated mutable `let n = 1` widens its inferred literal `1`
// to the `number` keyword type node — the headline inferred-type-node case).
#[test]
fn create_type_of_declaration_widens_let_number() {
    let p = StubProgram::parse_and_bind("/a.ts", "let n = 1;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let n = first_var_declaration(&p, 0);
    assert_eq!(
        resolver.create_type_of_declaration(&mut c, &p, n),
        Some(SynthesizedTypeNode::Keyword(Kind::NumberKeyword))
    );
}

// Go: CreateTypeOfDeclaration — a `let s = "a"` widens to `string`, `let b =
// true` to `boolean` (the widened-literal keyword arms of typeToTypeNode).
#[test]
fn create_type_of_declaration_widens_let_string_and_boolean() {
    let p = StubProgram::parse_and_bind("/a.ts", "let s = \"a\";\nlet b = true;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let s = first_var_declaration(&p, 0);
    let b = first_var_declaration(&p, 1);
    assert_eq!(
        resolver.create_type_of_declaration(&mut c, &p, s),
        Some(SynthesizedTypeNode::Keyword(Kind::StringKeyword))
    );
    assert_eq!(
        resolver.create_type_of_declaration(&mut c, &p, b),
        Some(SynthesizedTypeNode::Keyword(Kind::BooleanKeyword))
    );
}

// Go: CreateTypeOfDeclaration — even a `const x = 1` *widens* its fresh literal
// to `number` here (`serializeTypeForDeclaration` applies `getWidenedLiteralType`
// unconditionally). The literal-const initializer carve-out lives in the
// transformer (`shouldPrintWithInitializer`), not here.
#[test]
fn create_type_of_declaration_widens_const_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let x = first_var_declaration(&p, 0);
    assert_eq!(
        resolver.create_type_of_declaration(&mut c, &p, x),
        Some(SynthesizedTypeNode::Keyword(Kind::NumberKeyword))
    );
}

// Go: typeToTypeNode object reference arm — a `const xs = [1, 2]` (with a global
// `Array` in scope) serializes to a `number[]` array type node.
#[test]
fn create_type_of_declaration_array_is_array_type_node() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Array<T> {}\nconst xs = [1, 2];");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let xs = first_var_declaration(&p, 1);
    assert_eq!(
        resolver.create_type_of_declaration(&mut c, &p, xs),
        Some(SynthesizedTypeNode::Array(Box::new(
            SynthesizedTypeNode::Keyword(Kind::NumberKeyword)
        )))
    );
}

// Go: createAnonymousTypeNode — a `const o = { a: 1 }` serializes to a
// `{ a: number; }` type-literal node (the property value widened to `number`).
#[test]
fn create_type_of_declaration_object_literal_is_type_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 };");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let o = first_var_declaration(&p, 0);
    assert_eq!(
        resolver.create_type_of_declaration(&mut c, &p, o),
        Some(SynthesizedTypeNode::TypeLiteral(vec![
            SynthesizedProperty {
                name: "a".to_string(),
                type_node: SynthesizedTypeNode::Keyword(Kind::NumberKeyword),
                readonly: false,
                optional: false,
            }
        ]))
    );
}

// Go: EmitResolver.CreateReturnTypeOfSignatureDeclaration — an un-annotated
// `function f() { return 1; }` infers the `number` return type node from its
// body.
#[test]
fn create_return_type_of_signature_declaration_infers_from_body() {
    let p = StubProgram::parse_and_bind("/a.ts", "function f() { return 1; }");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let f = statement(&p, 0);
    assert_eq!(
        resolver.create_return_type_of_signature_declaration(&mut c, &p, f),
        Some(SynthesizedTypeNode::Keyword(Kind::NumberKeyword))
    );
}

// Go: EmitResolver.IsLiteralConstDeclaration — a `const` whose symbol type is a
// fresh primitive literal is a literal const (`const x = 1`/`const s = "a"`/
// `const b = true`); a `let`, or a `const` array/object (non-literal type), is
// not.
#[test]
fn is_literal_const_declaration_detects_fresh_primitive_const() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const x = 1;\nlet n = 1;\nconst o = { a: 1 };\ninterface Array<T> {}\nconst xs = [1];",
    );
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let x = first_var_declaration(&p, 0);
    let n = first_var_declaration(&p, 1);
    let o = first_var_declaration(&p, 2);
    let xs = first_var_declaration(&p, 4);
    assert!(resolver.is_literal_const_declaration(&mut c, &p, x));
    // A `let` is not a const, so not a literal const.
    assert!(!resolver.is_literal_const_declaration(&mut c, &p, n));
    // A `const` object literal's type is not a fresh primitive literal.
    assert!(!resolver.is_literal_const_declaration(&mut c, &p, o));
    // A `const` array's type is not a fresh primitive literal.
    assert!(!resolver.is_literal_const_declaration(&mut c, &p, xs));
}

// Go: EmitResolver.CreateLiteralConstValue — returns the literal value kept in
// place of a type for a literal const (`1`/`"a"`/`true`).
#[test]
fn create_literal_const_value_reachable_primitives() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;\nconst s = \"a\";\nconst b = true;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let x = first_var_declaration(&p, 0);
    let s = first_var_declaration(&p, 1);
    let b = first_var_declaration(&p, 2);
    assert_eq!(
        resolver.create_literal_const_value(&mut c, &p, x),
        Some(LiteralConstValue::Number {
            text: "1".to_string(),
            negative: false
        })
    );
    assert_eq!(
        resolver.create_literal_const_value(&mut c, &p, s),
        Some(LiteralConstValue::String("a".to_string()))
    );
    assert_eq!(
        resolver.create_literal_const_value(&mut c, &p, b),
        Some(LiteralConstValue::Boolean(true))
    );
}
