use super::ContextFlags;
use crate::core::declared_types::get_type_of_symbol;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use std::rc::Rc;
use tsgo_ast::{NodeData, NodeId};

// Returns the statements of the source file.
fn source_statements(p: &StubProgram) -> Vec<NodeId> {
    match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    }
}

// Returns the initializer expression of the first variable declaration of the
// `idx`-th top-level statement (a `VariableStatement`).
fn var_decl_initializer(p: &StubProgram, idx: usize) -> NodeId {
    let arena = p.arena();
    let list = match arena.data(source_statements(p)[idx]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
        _ => panic!("variable declaration"),
    }
}

// Returns the `idx`-th parameter node of an arrow function.
fn arrow_parameter(p: &StubProgram, arrow: NodeId, idx: usize) -> NodeId {
    match p.arena().data(arrow) {
        NodeData::ArrowFunction(d) => d.parameters.nodes[idx],
        _ => panic!("arrow function"),
    }
}

// Returns the expression of the first expression-statement inside an arrow's
// block body.
fn arrow_block_first_expr(p: &StubProgram, arrow: NodeId) -> NodeId {
    let arena = p.arena();
    let body = match arena.data(arrow) {
        NodeData::ArrowFunction(d) => d.body,
        _ => panic!("arrow function"),
    };
    let stmts = match arena.data(body) {
        NodeData::Block(d) => d.list.nodes.clone(),
        _ => panic!("block body"),
    };
    match arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    }
}

// 4bj slice 1 tracer (genuine RED): an un-annotated arrow parameter is
// contextually typed by the function-type annotation of the variable it
// initializes. `const f: (x: number) => void = (x) => { x; };` types `x` (read
// inside the body) as `number`. Before this round an un-annotated parameter had
// no contextual source, so `get_type_of_symbol` fell through to `any`.
// Go: internal/checker/checker.go:Checker.getContextuallyTypedParameterType
#[test]
fn arrow_parameter_is_contextually_typed_by_variable_annotation() {
    let p = StubProgram::parse_and_bind("/a.ts", "const f: (x: number) => void = (x) => { x; };");
    let arrow = var_decl_initializer(&p, 0);
    let x_ref = arrow_block_first_expr(&p, arrow);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, x_ref), number);
}

// 4bj slice 2 (genuine RED before slice 1's impl): the contextual parameter
// type really flows into the body, so a body statement that misuses it surfaces
// a diagnostic. `const f: (x: number) => void = (x) => { const s: string = x; };`
// assigns the contextually-`number` `x` to a `string`, reporting `2322`. Before
// this round `x` was `any` (assignable to `string`), so no diagnostic appeared
// (0 ≠ 1 -> genuine RED).
// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (2322)
#[test]
fn contextual_parameter_type_flows_into_body_and_surfaces_mismatch() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f: (x: number) => void = (x) => { const s: string = x; };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// 4bj slice 2 (positive control / green-on-arrival): assigning the
// contextually-`number` parameter to a `number` produces no diagnostic — the
// contextual type is correct, not merely present.
// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration
#[test]
fn contextual_parameter_type_flows_into_body_assignable_ok() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f: (x: number) => void = (x) => { const n: number = x; };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(c.get_diagnostics(root).is_empty());
}

// 4bj slice 3 (guard, green-on-arrival): an explicit parameter annotation wins
// over the contextual type — Go never overrides an explicit annotation.
// `const f: (x: number) => void = (x: string) => { x; };` types `x` as `string`
// (its annotation), not `number`. The contextual path only touches un-annotated
// parameters (`assignContextualParameterTypes` skips annotated ones,
// `getTypeForVariableLikeDeclaration` reads the annotation first).
// Go: internal/checker/checker.go:Checker.assignContextualParameterTypes (declaration.Type() == nil)
#[test]
fn explicit_parameter_annotation_overrides_contextual_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const f: (x: number) => void = (x: string) => { x; };",
    );
    let arrow = var_decl_initializer(&p, 0);
    let x_ref = arrow_block_first_expr(&p, arrow);
    let mut c = Checker::new();
    let string = c.string_type();
    assert_eq!(c.check_expression(&p, x_ref), string);
}

// 4bj slice 3 (guard, no regression): a bare arrow with no contextual type
// keeps its un-annotated parameter's no-context type (`any`) — there is no
// contextual signature, so the parameter is not contextually typed (and the
// body still checks without panicking). `const g = (x) => x;` -> `x` is `any`.
// Go: internal/checker/checker.go:Checker.getContextuallyTypedParameterType (nil contextual signature)
#[test]
fn bare_arrow_parameter_without_context_stays_any() {
    let p = StubProgram::parse_and_bind("/a.ts", "const g = (x) => x;");
    let arrow = var_decl_initializer(&p, 0);
    let param = arrow_parameter(&p, arrow, 0);
    let symbol = p.symbol_of_node(param).expect("parameter symbol");
    let mut c = Checker::new();
    let any = c.any_type();
    assert_eq!(
        get_type_of_symbol(&mut c, &p, symbol, p.globals()),
        any,
        "an un-annotated parameter with no contextual type stays `any`"
    );
}

// 4bj unit: `get_contextual_type` returns the variable's annotation type for the
// initializer expression of an annotated variable declaration. For
// `const x: number = 0;` the initializer `0` has contextual type `number`.
// Go: internal/checker/checker.go:Checker.getContextualType (KindVariableDeclaration)
#[test]
fn get_contextual_type_of_variable_initializer_is_annotation_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x: number = 0;");
    let initializer = var_decl_initializer(&p, 0);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(
        c.get_contextual_type(&p, initializer, ContextFlags::NONE),
        Some(number)
    );
}

// 4bj unit: `get_contextual_type` returns `None` for an expression with no
// contextual source (a bare expression statement).
// Go: internal/checker/checker.go:Checker.getContextualType (default -> nil)
#[test]
fn get_contextual_type_of_unconstrained_expression_is_none() {
    let p = StubProgram::parse_and_bind("/a.ts", "0;");
    let expr = match p.arena().data(source_statements(&p)[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let mut c = Checker::new();
    assert_eq!(c.get_contextual_type(&p, expr, ContextFlags::NONE), None);
}

// 4bj unit: `get_contextual_signature` extracts the single call signature of a
// contextual function type, and `get_contextually_typed_parameter_type` reads
// the parameter type at the matching position. For
// `const f: (a: string, b: number) => void = (a, b) => {};` the second
// parameter `b` is contextually `number`.
// Go: internal/checker/checker.go:Checker.getContextualSignature / getContextuallyTypedParameterType
#[test]
fn get_contextually_typed_parameter_type_uses_positional_signature_parameter() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const f: (a: string, b: number) => void = (a, b) => {};",
    );
    let arrow = var_decl_initializer(&p, 0);
    let a = arrow_parameter(&p, arrow, 0);
    let b = arrow_parameter(&p, arrow, 1);
    let mut c = Checker::new();
    let string = c.string_type();
    let number = c.number_type();
    assert_eq!(c.get_contextually_typed_parameter_type(&p, a), Some(string));
    assert_eq!(c.get_contextually_typed_parameter_type(&p, b), Some(number));
    // The contextual signature itself has two parameters.
    let sig = c
        .get_contextual_signature(&p, arrow)
        .expect("contextual signature");
    assert_eq!(c.signature(sig).parameters.len(), 2);
}

// 4bj unit: the eager `assign_contextual_parameter_types` caches the contextual
// parameter type on the parameter symbol's value links, so a later
// `get_type_of_symbol` reads it without recomputation. Verifies the eager path
// independently of the lazy `getTypeForVariableLikeDeclaration` fallback.
// Go: internal/checker/checker.go:Checker.assignContextualParameterTypes / assignParameterType
#[test]
fn assign_contextual_parameter_types_caches_on_symbol_links() {
    let p = StubProgram::parse_and_bind("/a.ts", "const f: (x: number) => void = (x) => {};");
    let arrow = var_decl_initializer(&p, 0);
    let param = arrow_parameter(&p, arrow, 0);
    let symbol = p.symbol_of_node(param).expect("parameter symbol");
    let mut c = Checker::new();
    // Before assignment, no resolved type is cached.
    assert!(c
        .value_symbol_links
        .try_get(&symbol)
        .and_then(|l| l.resolved_type)
        .is_none());
    let sig = c
        .get_contextual_signature(&p, arrow)
        .expect("contextual signature");
    c.assign_contextual_parameter_types(&p, arrow, sig);
    let number = c.number_type();
    assert_eq!(
        c.value_symbol_links
            .try_get(&symbol)
            .and_then(|l| l.resolved_type),
        Some(number),
        "the eager assignment caches the contextual parameter type"
    );
}

// 4bj unit: a function type node (`(x: number) => void`) resolves to an object
// type carrying a single call signature whose parameter is `number` and whose
// return type is `void`. This is the contextual function type that drives
// parameter typing.
// Go: internal/checker/checker.go:Checker.getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode
#[test]
fn function_type_node_resolves_to_object_with_call_signature() {
    let p = StubProgram::parse_and_bind("/a.ts", "let f: (x: number) => void;");
    // Navigate to the variable declaration's type node.
    let arena = p.arena();
    let list = match arena.data(source_statements(&p)[0]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    let type_node = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("type annotation"),
        _ => panic!("variable declaration"),
    };
    let mut c = Checker::new();
    let globals = p.globals();
    let t = crate::core::declared_types::get_type_from_type_node(&mut c, &p, type_node, globals);
    let sigs = c.get_signatures_of_type(t);
    assert_eq!(sigs.len(), 1, "a function type has one call signature");
    let number = c.number_type();
    let void = c.void_type();
    assert_eq!(c.try_get_type_at_position(&p, sigs[0], 0), Some(number));
    assert_eq!(c.signature(sigs[0]).resolved_return_type, Some(void));
}

// 4bj unit: the assignment-RHS arm of `get_contextual_type`. The right operand
// of an assignment to an annotated identifier target is contextually typed by
// the target's declared type. For `let x: number; x = 0;` the `0` has
// contextual type `number`.
// Go: internal/checker/checker.go:Checker.getContextualTypeForAssignmentExpression
#[test]
fn get_contextual_type_of_assignment_rhs_is_target_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "let x: number;\nx = 0;");
    // The second statement is `x = 0;` (an expression statement wrapping a
    // binary assignment); its right operand is the contextually typed node.
    let assignment = match p.arena().data(source_statements(&p)[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let right = match p.arena().data(assignment) {
        NodeData::BinaryExpression(d) => d.right,
        _ => panic!("binary expression"),
    };
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(
        c.get_contextual_type(&p, right, ContextFlags::NONE),
        Some(number)
    );
}

// 4bj unit: an un-annotated parameter of an arrow assigned to an annotated
// identifier (the assignment-RHS contextual path) is contextually typed too.
// For `let f: (x: number) => void;\nf = (x) => { x; };` the parameter `x` is
// `number`.
// Go: internal/checker/checker.go:Checker.getContextualTypeForAssignmentExpression
#[test]
fn arrow_parameter_is_contextually_typed_via_assignment_rhs() {
    let p = StubProgram::parse_and_bind("/a.ts", "let f: (x: number) => void;\nf = (x) => { x; };");
    let assignment = match p.arena().data(source_statements(&p)[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let arrow = match p.arena().data(assignment) {
        NodeData::BinaryExpression(d) => d.right,
        _ => panic!("binary expression"),
    };
    let x_ref = arrow_block_first_expr(&p, arrow);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, x_ref), number);
}

// Returns the value expression of the `idx`-th property assignment of an object
// literal initializer of the first variable declaration.
fn object_literal_property_value(p: &StubProgram, member_idx: usize) -> NodeId {
    let arena = p.arena();
    let literal = var_decl_initializer(p, 0);
    let members = match arena.data(literal) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
        _ => panic!("object literal"),
    };
    match arena.data(members[member_idx]) {
        NodeData::PropertyAssignment(d) => d.initializer.expect("property value"),
        _ => panic!("property assignment"),
    }
}

// 4bk unit: the object-literal-element arm of `get_contextual_type`. A property
// value in `const o: { a: "x" } = { a: "x" };` is contextually typed by the
// annotation's matching property type `"x"`.
// Go: internal/checker/checker.go:Checker.getContextualTypeForObjectLiteralElement
#[test]
fn get_contextual_type_of_object_literal_property_value_is_property_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o: { a: \"x\" } = { a: \"x\" };");
    let value = object_literal_property_value(&p, 0);
    let mut c = Checker::new();
    let lit = c.get_string_literal_type("x");
    assert_eq!(
        c.get_contextual_type(&p, value, ContextFlags::NONE),
        Some(lit)
    );
}

// 4bk unit: a property value whose name is absent from the contextual type has
// no contextual type. In `const o: { a: number } = { b: 1 };` the value of `b`
// has no matching property/index signature in `{ a: number }`, so the
// object-literal-element arm yields `None`.
// Go: internal/checker/checker.go:Checker.getTypeOfPropertyOfContextualType (nil)
#[test]
fn get_contextual_type_of_unknown_object_literal_property_is_none() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o: { a: number } = { b: 1 };");
    let value = object_literal_property_value(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.get_contextual_type(&p, value, ContextFlags::NONE), None);
}

// 4bk unit: the array-literal-element arm of `get_contextual_type`. The element
// of `const xs: "a"[] = ["a"];` is contextually typed by the iterated element
// type of the contextual array `"a"[]`, i.e. `"a"`. A synthetic `interface
// Array<T>` stands in for the lib type.
// Go: internal/checker/checker.go:Checker.getContextualTypeForElementExpression
#[test]
fn get_contextual_type_of_array_literal_element_is_element_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst xs: \"a\"[] = [\"a\"];",
    );
    // The variable statement is the 2nd top-level statement; reuse the local
    // navigation by finding the array literal directly.
    let arena = p.arena();
    let list = match arena.data(source_statements(&p)[1]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    let literal = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
        _ => panic!("variable declaration"),
    };
    let element = match arena.data(literal) {
        NodeData::ArrayLiteralExpression(d) => d.list.nodes[0],
        _ => panic!("array literal"),
    };
    let mut c = Checker::new();
    let lit = c.get_string_literal_type("a");
    assert_eq!(
        c.get_contextual_type(&p, element, ContextFlags::NONE),
        Some(lit)
    );
}

// 4bk unit: `is_literal_of_contextual_type` is true when a literal candidate
// sits in a context that is a literal of the same primitive kind, false for a
// mismatched kind and for an absent context.
// Go: internal/checker/checker.go:Checker.isLiteralOfContextualType
#[test]
fn is_literal_of_contextual_type_matches_same_kind_only() {
    let mut c = Checker::new();
    let str_x = c.get_string_literal_type("x");
    let num_one = c.get_number_literal_type(tsgo_jsnum::Number::from(1.0));
    // Same kind (string literal in a string-literal context) -> literal context.
    assert!(c.is_literal_of_contextual_type(str_x, Some(str_x)));
    // Mismatched kind (string-literal candidate, number-literal context) -> not.
    assert!(!c.is_literal_of_contextual_type(str_x, Some(num_one)));
    // A non-literal context (the `string` primitive) is not a literal context.
    let string = c.string_type();
    assert!(!c.is_literal_of_contextual_type(str_x, Some(string)));
    // No contextual type at all.
    assert!(!c.is_literal_of_contextual_type(str_x, None));
}

// Returns the call expression of the `idx`-th top-level expression statement.
fn call_expression_of_statement(p: &StubProgram, idx: usize) -> NodeId {
    match p.arena().data(source_statements(p)[idx]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    }
}

// Returns the `idx`-th argument node of a call expression.
fn call_argument(p: &StubProgram, call: NodeId, idx: usize) -> NodeId {
    match p.arena().data(call) {
        NodeData::CallExpression(d) => d.arguments.nodes[idx],
        _ => panic!("call expression"),
    }
}

// 4bl slice 1 tracer (genuine RED): a callback argument's un-annotated
// parameter is contextually typed by the resolved call signature's parameter at
// that position. `function f(cb: (x: number) => void) {} f((x) => { x; });`
// types the arrow's `x` (read inside the body) as `number`. Before this round
// the call-argument parent arm of `get_contextual_type` was absent, so the
// arrow had no contextual signature and `x` fell through to `any`. Exercises
// the *lazy* channel (`check_expression(x)` directly, without checking the
// call first).
// Go: internal/checker/checker.go:Checker.getContextualTypeForArgument
#[test]
fn callback_argument_parameter_is_contextually_typed_by_call_signature() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function f(cb: (x: number) => void) {}\nf((x) => { x; });",
    );
    let call = call_expression_of_statement(&p, 1);
    let arrow = call_argument(&p, call, 0);
    let x_ref = arrow_block_first_expr(&p, arrow);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, x_ref), number);
}

// 4bl slice 2 (genuine RED before the call-argument arm): the contextual
// parameter type really flows into the callback body, so a body statement that
// misuses it surfaces a diagnostic. `function f(cb: (x: number) => void) {}
// f((x) => { const s: string = x; });` assigns the contextually-`number` `x`
// to a `string`, reporting `2322`. Exercises the *eager* channel
// (`check_source_file` -> `check_arrow_function` ->
// `contextually_check_function_expression`, then the body check). Before this
// round `x` was `any` (assignable to `string`), so no diagnostic appeared.
// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (2322)
#[test]
fn callback_argument_parameter_type_flows_into_body_and_surfaces_mismatch() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(cb: (x: number) => void) {}\nf((x) => { const s: string = x; });",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// 4bl slice 2 (positive control / green-on-arrival): assigning the
// contextually-`number` callback parameter to a `number` produces no
// diagnostic — the contextual type is correct, not merely present.
// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration
#[test]
fn callback_argument_parameter_type_flows_into_body_assignable_ok() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(cb: (x: number) => void) {}\nf((x) => { const n: number = x; });",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(c.get_diagnostics(root).is_empty());
}

// 4bl slice 3 (guard, no regression): a plain call with no callback argument
// still checks cleanly. `function g(n: number) {} g(1);` reports nothing — the
// call-argument contextual arm only engages for function/arrow arguments and
// does not perturb ordinary argument checking.
// Go: internal/checker/checker.go:Checker.checkCallExpression
#[test]
fn plain_call_without_callback_still_checks_clean() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function g(n: number) {}\ng(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(c.get_diagnostics(root).is_empty());
}

// 4bl slice 3 (guard, no crash): a non-function argument to a callback
// parameter does not crash and does not spuriously diagnose. `function f(cb:
// (x: number) => void) {} f(1 as any);` passes `any` to the callback
// parameter, which is assignable, so the call reports nothing — and the
// contextual arm (which only resolves a signature for the lookup) is unaffected
// by the argument not being a function.
// Go: internal/checker/checker.go:Checker.checkCallExpression
#[test]
fn call_with_non_function_argument_does_not_crash() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(cb: (x: number) => void) {}\nf(1 as any);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(c.get_diagnostics(root).is_empty());
}

// 4bl slice 3 (guard, recursion): a callback whose body contains another call
// to the same function (with its own callback) terminates — the contextual
// lookup resolves each call's signature by typing only its callee, never its
// arguments, so it never re-enters argument checking. `function f(cb: (x:
// number) => void) {} f((x) => { f((y) => { y; }); });` types both `x` and `y`
// as `number` and reports nothing (would stack-overflow if the contextual
// lookup re-checked arguments).
// Go: internal/checker/checker.go:Checker.getResolvedSignature (resolvingSignature guard)
#[test]
fn nested_callback_call_does_not_recurse_infinitely() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(cb: (x: number) => void) {}\nf((x) => { f((y) => { y; }); });",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(c.get_diagnostics(root).is_empty());
}

// 4bl unit: the call-argument arm of `get_contextual_type`. For `function f(cb:
// (x: number) => void) {} f((x) => {});` the arrow argument's contextual type
// is the resolved signature's first parameter type — the function type `(x:
// number) => void`, which carries a single call signature whose parameter is
// `number` and whose return type is `void`.
// Go: internal/checker/checker.go:Checker.getContextualTypeForArgumentAtIndex
#[test]
fn get_contextual_type_of_call_argument_is_signature_parameter_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function f(cb: (x: number) => void) {}\nf((x) => {});",
    );
    let call = call_expression_of_statement(&p, 1);
    let arrow = call_argument(&p, call, 0);
    let mut c = Checker::new();
    let ctx = c
        .get_contextual_type(&p, arrow, ContextFlags::NONE)
        .expect("call argument has a contextual type");
    // The contextual type is the parameter function type `(x: number) => void`.
    let sigs = c.get_signatures_of_type(ctx);
    assert_eq!(sigs.len(), 1, "the contextual type is a function type");
    let number = c.number_type();
    let void = c.void_type();
    assert_eq!(c.try_get_type_at_position(&p, sigs[0], 0), Some(number));
    assert_eq!(c.signature(sigs[0]).resolved_return_type, Some(void));
}

// 4bl unit: the callee expression of a call shares the call node as its parent,
// but it is not one of the arguments, so it has no call-argument contextual
// type (Go's `slices.Index(args, arg) == -1` early-out). For `function f(cb: (x:
// number) => void) {} f((x) => {});` the callee `f` has contextual type `None`.
// Go: internal/checker/checker.go:Checker.getContextualTypeForArgument (argIndex == -1)
#[test]
fn get_contextual_type_of_callee_is_none() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function f(cb: (x: number) => void) {}\nf((x) => {});",
    );
    let call = call_expression_of_statement(&p, 1);
    let callee = match p.arena().data(call) {
        NodeData::CallExpression(d) => d.expression,
        _ => panic!("call expression"),
    };
    let mut c = Checker::new();
    assert_eq!(c.get_contextual_type(&p, callee, ContextFlags::NONE), None);
}

// 4bl unit (DEFER guard): an overloaded callee (more than one call signature)
// is deferred — disambiguating the overload would need the argument types, so
// the reachable subset returns no contextual type. For `declare function f(a:
// number): void; declare function f(a: string): void; f((x) => {});` the arrow
// argument's contextual type is `None`.
// Go: internal/checker/checker.go:Checker.getResolvedSignature (overload resolution, deferred)
#[test]
fn get_contextual_type_of_overloaded_call_argument_is_none() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(a: number): void;\ndeclare function f(a: string): void;\nf((x) => {});",
    );
    let call = call_expression_of_statement(&p, 2);
    let arrow = call_argument(&p, call, 0);
    let mut c = Checker::new();
    assert_eq!(c.get_contextual_type(&p, arrow, ContextFlags::NONE), None);
}

// 4bl unit: an argument past the (non-rest) parameter list has contextual type
// `any` (Go's `getTypeAtPosition` fallback). For `function f() {} f((x) => {});`
// the extra arrow argument's contextual type is `any`.
// Go: internal/checker/relater.go:Checker.getTypeAtPosition (any fallback)
#[test]
fn get_contextual_type_of_extra_call_argument_is_any() {
    let p = StubProgram::parse_and_bind("/a.ts", "function f() {}\nf((x) => {});");
    let call = call_expression_of_statement(&p, 1);
    let arrow = call_argument(&p, call, 0);
    let mut c = Checker::new();
    let any = c.any_type();
    assert_eq!(
        c.get_contextual_type(&p, arrow, ContextFlags::NONE),
        Some(any)
    );
}

// 4bl unit (DEFER guard): a `new` expression's argument has no contextual type
// when the constructor has no call signature — construct signatures are not yet
// collected, so `get_signatures_of_type` (which returns only call signatures)
// yields none. For `declare const C: any;\nnew C((x) => {});` the arrow
// argument's contextual type is `None` (exercises the `NewExpression` argument
// arm).
// Go: internal/checker/checker.go:Checker.getContextualTypeForArgument (NewExpression)
#[test]
fn get_contextual_type_of_new_expression_argument_without_construct_signature_is_none() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const C: any;\nnew C((x) => {});");
    let new_expr = call_expression_of_statement(&p, 1);
    let arrow = match p.arena().data(new_expr) {
        NodeData::NewExpression(d) => d.arguments.as_ref().expect("arguments").nodes[0],
        _ => panic!("new expression"),
    };
    let mut c = Checker::new();
    assert_eq!(c.get_contextual_type(&p, arrow, ContextFlags::NONE), None);
}

// 4bl slice 3 (guard, no double-report): an unresolved callee with an arrow
// argument reports its "Cannot find name" error exactly once. The contextual
// lookup re-resolves the callee's signature, so it must not re-emit the callee
// diagnostic. `g((x) => { x; });` (no `g` in scope) reports a single `2304`.
// Go: internal/checker/checker.go:Checker.resolveCallExpression (checkExpressionCached callee)
#[test]
fn unresolved_callee_with_callback_reports_2304_once() {
    let p = Rc::new(StubProgram::parse_and_bind("/a.ts", "g((x) => { x; });"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "the callee error is reported exactly once");
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'g'.");
}

// ---- T1-E batch 31: object-literal accessors/methods, computed-name contextual typing, getWidenedUniqueESSymbolType ----

// Returns the `idx`-th member of an object-literal initializer of the first
// variable declaration.
fn object_literal_member(p: &StubProgram, member_idx: usize) -> NodeId {
    let literal = var_decl_initializer(p, 0);
    match p.arena().data(literal) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes[member_idx],
        _ => panic!("object literal"),
    }
}

// Go: internal/checker/checker.go:Checker.getContextualTypeForObjectLiteralMethod(29640)
#[test]
fn get_contextual_type_of_object_literal_method_is_property_type() {
    let p =
        StubProgram::parse_and_bind("/a.ts", "const o: { m(): number } = { m() { return 1 } };");
    let method = object_literal_member(&p, 0);
    let mut c = Checker::new();
    let ctx = c
        .get_contextual_type(&p, method, ContextFlags::NONE)
        .expect("object-literal method has a contextual type");
    let sigs = c.get_signatures_of_type(ctx);
    assert_eq!(sigs.len(), 1, "the contextual type is a method signature");
    assert_eq!(c.get_return_type_of_signature(sigs[0]), c.number_type());
}

// Go: internal/checker/checker.go:Checker.getContextualTypeForObjectLiteralElement(29596)
#[test]
fn get_contextual_type_of_object_literal_get_accessor_is_property_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { get x(): number } = { get x() { return 1 } };",
    );
    let getter = object_literal_member(&p, 0);
    let mut c = Checker::new();
    assert_eq!(
        c.get_contextual_type(&p, getter, ContextFlags::NONE),
        Some(c.number_type())
    );
}

// Go: internal/checker/checker.go:Checker.isLiteralOfContextualType(25381)
#[test]
fn is_literal_of_contextual_type_matches_unique_es_symbol() {
    let mut c = Checker::new();
    let unique = c.new_unique_es_symbol_type(tsgo_ast::SymbolId(1), "\u{fe}@sym@1");
    assert!(c.is_literal_of_contextual_type(unique, Some(unique)));
    assert!(!c.is_literal_of_contextual_type(unique, Some(c.string_type())));
}

// Go: internal/checker/checker.go:Checker.getWidenedUniqueESSymbolType(25364)
#[test]
fn get_widened_literal_like_type_for_contextual_type_widens_unique_es_symbol() {
    let mut c = Checker::new();
    let unique = c.new_unique_es_symbol_type(tsgo_ast::SymbolId(1), "\u{fe}@sym@1");
    let es_symbol = c.es_symbol_type();
    assert_eq!(
        c.get_widened_literal_like_type_for_contextual_type(unique, None),
        es_symbol
    );
    assert_eq!(
        c.get_widened_literal_like_type_for_contextual_type(unique, Some(unique)),
        unique
    );
}

// 4bk unit: `get_widened_literal_like_type_for_contextual_type` preserves a
// literal in a matching literal context (returning the regular literal), but
// widens a fresh literal when there is no contextual type or the context's kind
// does not match.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralLikeTypeForContextualType
#[test]
fn get_widened_literal_like_type_for_contextual_type_preserves_or_widens() {
    let mut c = Checker::new();
    let regular_x = c.get_string_literal_type("x");
    let fresh_x = c.get_fresh_type_of_literal_type(regular_x);
    let string = c.string_type();
    // In a matching literal context the fresh literal is preserved as the
    // regular literal (freshness stripped, value kept).
    assert_eq!(
        c.get_widened_literal_like_type_for_contextual_type(fresh_x, Some(regular_x)),
        regular_x
    );
    // With no contextual type the fresh literal widens to `string`.
    assert_eq!(
        c.get_widened_literal_like_type_for_contextual_type(fresh_x, None),
        string
    );
    // With a non-matching context (the `string` primitive is not a literal) the
    // fresh literal widens too.
    assert_eq!(
        c.get_widened_literal_like_type_for_contextual_type(fresh_x, Some(string)),
        string
    );
}

// ---- T1-E batch 32: object-literal method/accessor members, unique symbol in type literals ----

// Go: internal/checker/checker.go:Checker.getContextualTypeForObjectLiteralElement(29596)
#[test]
fn get_contextual_type_of_object_literal_set_accessor_is_property_type() {
    let p =
        StubProgram::parse_and_bind("/a.ts", "const o: { set x(v: number) } = { set x(v) { } };");
    let setter = object_literal_member(&p, 0);
    let mut c = Checker::new();
    assert_eq!(
        c.get_contextual_type(&p, setter, ContextFlags::NONE),
        Some(c.number_type())
    );
}
