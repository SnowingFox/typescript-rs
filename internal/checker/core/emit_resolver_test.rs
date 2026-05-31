use crate::core::emit_resolver::SerializedTypeNode;
use crate::core::program::BoundProgram;
use crate::core::symbols_query::get_symbol_of_declaration;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::{NodeData, NodeId};

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

// Go: internal/checker/emitresolver.go:EmitResolver.IsDeclarationVisible (tracer)
#[test]
fn exported_declaration_is_visible() {
    let p = StubProgram::parse_and_bind("/a.ts", "export function f() {}\nfunction g() {}");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let f = statement(&p, 0);
    let g = statement(&p, 1);
    // The exported `f` is visible to declaration emit; the bare `g` is not.
    assert!(resolver.is_declaration_visible(&p, f));
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
