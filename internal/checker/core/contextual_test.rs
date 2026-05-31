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
