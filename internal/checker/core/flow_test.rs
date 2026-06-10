use crate::core::declared_types::get_declared_type_of_symbol;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::{NodeData, SymbolId};

fn empty() -> StubProgram {
    StubProgram::parse_and_bind("/a.ts", "")
}

fn sym(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof (tracer bullet)
#[test]
fn narrow_typeof_string_on_string_or_number() {
    let mut c = Checker::new();
    let p = empty();
    let s = c.string_type();
    let n = c.number_type();
    let union = c.string_or_number_type();
    // `typeof x === "string"` keeps string; the false branch keeps number.
    assert_eq!(c.narrow_type_by_typeof(&p, union, "string", true), s);
    assert_eq!(c.narrow_type_by_typeof(&p, union, "string", false), n);
    // `typeof x === "number"` keeps number.
    assert_eq!(c.narrow_type_by_typeof(&p, union, "number", true), n);
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTruthiness
#[test]
fn narrow_truthiness_boolean_and_nullable() {
    let mut c = Checker::new();
    let boolean = c.boolean_type();
    let rt = c.regular_true_type();
    let rf = c.regular_false_type();
    // `if (b)` keeps `true`; `else` keeps `false`.
    assert_eq!(c.narrow_type_by_truthiness(boolean, true), rt);
    assert_eq!(c.narrow_type_by_truthiness(boolean, false), rf);
    // `string | undefined` truthy removes `undefined`.
    let s = c.string_type();
    let u = c.undefined_type();
    let su = c.get_union_type(&[s, u]);
    assert_eq!(c.narrow_type_by_truthiness(su, true), s);
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTruthiness (falsy literal subtype)
#[test]
fn narrow_truthiness_drops_empty_string_literal() {
    use crate::core::types::{LiteralValue, TypeFlags};
    let mut c = Checker::new();
    let empty = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String(String::new()),
        None,
    );
    let a = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    let union = c.get_union_type(&[empty, a]);
    // `"" | "a"` truthy drops the empty-string literal.
    assert_eq!(c.narrow_type_by_truthiness(union, true), a);
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (literal discriminant)
#[test]
fn narrow_equality_literal_union() {
    use crate::core::types::{LiteralValue, TypeFlags};
    let mut c = Checker::new();
    let p = empty();
    let a = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    let b = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("b".into()),
        None,
    );
    let union = c.get_union_type(&[a, b]);
    // A distinct `"a"` literal (compared by value, not identity).
    let value = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    assert_eq!(c.narrow_type_by_equality(&p, union, value, true), a);
    assert_eq!(c.narrow_type_by_equality(&p, union, value, false), b);
}

// Go: internal/checker/flow.go:Checker.narrowTypeByInKeyword
#[test]
fn narrow_in_object_union() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  a: number;\n}\ninterface B {\n  b: number;\n}",
    );
    let mut c = Checker::new();
    let a_ty = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b_ty = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let union = c.get_union_type(&[a_ty, b_ty]);
    // `"a" in x` keeps the constituent that has property `a`.
    assert_eq!(c.narrow_type_by_in(union, "a", true), a_ty);
    assert_eq!(c.narrow_type_by_in(union, "a", false), b_ty);
}

// Walks SourceFile -> if-statement -> then-block -> expression statement to the
// reference identifier the binder attached a flow node to.
fn first_then_block_usage(p: &StubProgram, if_index: usize) -> tsgo_ast::NodeId {
    use tsgo_ast::NodeData;
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("expected a source file"),
    };
    let then_stmt = match arena.data(stmts[if_index]) {
        NodeData::IfStatement(d) => d.then_statement,
        _ => panic!("expected an if statement"),
    };
    let block_stmts = match arena.data(then_stmt) {
        NodeData::Block(d) => d.list.nodes.clone(),
        _ => panic!("expected a block"),
    };
    match arena.data(block_stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected an expression statement"),
    }
}

// Go: internal/checker/flow.go:Checker.getFlowTypeOfReference (typeof guard)
#[test]
fn flow_typeof_narrows_in_then_branch() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | number;\nif (typeof x === \"string\") {\n  x;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let declared = c.string_or_number_type();
    let s = c.string_type();
    // Inside the `typeof x === "string"` branch, `x` narrows to `string`.
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), s);
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (in the flow walk)
#[test]
fn flow_equality_narrows_literal_union() {
    use crate::core::types::{LiteralValue, TypeFlags};
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string;\nif (x === \"a\") {\n  x;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let a = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    let b = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("b".into()),
        None,
    );
    let union = c.get_union_type(&[a, b]);
    // Inside `if (x === "a")`, the `"a" | "b"` reference narrows to `"a"`.
    assert_eq!(c.get_flow_type_of_reference(&p, usage, union), a);
}

// 4az slice C (genuine RED): a loose `x == null` guard narrows a nullable union
// by the `EQUndefinedOrNull` fact in the true branch, keeping BOTH `null` and
// `undefined` (loose `== null` matches both). Go's `narrowTypeByEquality` takes
// the nullable-value branch (`valueType.flags & Nullable`) and, for `==`/`!=`
// (double-equals), uses `EQUndefinedOrNull`/`NEUndefinedOrNull` via
// `getTypeWithFacts`. The old literal/subtype `equality_overlap` path kept only
// the exact `null` constituent, so this diverges.
// Go: internal/checker/flow.go:Checker.narrowTypeByEquality(549) (nullable branch)
#[test]
fn flow_equality_loose_null_keeps_both_nullables() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | null | undefined;\nif (x == null) {\n  x;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    let null = c.null_type();
    let u = c.undefined_type();
    let declared = c.get_union_type(&[s, null, u]);
    let expected = c.get_union_type(&[null, u]);
    // `x == null` true branch: `EQUndefinedOrNull` keeps `null | undefined`.
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), expected);
}

// 4az slice C guard (green-on-arrival): the task's primary example — a strict
// `x !== undefined` guard narrows `string | undefined` to `string` in the true
// branch (Go `narrowTypeByEquality` nullable branch -> `NEUndefined`). Rides the
// fact path now that the value operand `undefined` resolves (slice B).
// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (NEUndefined)
#[test]
fn flow_equality_ne_undefined_narrows_to_string() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nif (x !== undefined) {\n  x;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    let u = c.undefined_type();
    let declared = c.get_union_type(&[s, u]);
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), s);
}

// 4az slice C guard (green-on-arrival): the strict `x === undefined` true branch
// narrows `string | undefined` to `undefined` (`EQUndefined`).
// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (EQUndefined)
#[test]
fn flow_equality_eq_undefined_narrows_to_undefined() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nif (x === undefined) {\n  x;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    let u = c.undefined_type();
    let declared = c.get_union_type(&[s, u]);
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), u);
}

// 4az slice C guard (green-on-arrival): mirror for `null` — `x !== null` narrows
// `string | null` to `string` (`NENull`).
// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (NENull)
#[test]
fn flow_equality_ne_null_narrows_to_string() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | null;\nif (x !== null) {\n  x;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    let null = c.null_type();
    let declared = c.get_union_type(&[s, null]);
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), s);
}

// 4az slice C guard (green-on-arrival): the strict `x === null` true branch
// narrows `string | null` to `null` (`EQNull`).
// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (EQNull)
#[test]
fn flow_equality_eq_null_narrows_to_null() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | null;\nif (x === null) {\n  x;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    let null = c.null_type();
    let declared = c.get_union_type(&[s, null]);
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), null);
}

// Resolves the expression of a top-level expression statement at `index`.
fn top_level_expression(p: &StubProgram, index: usize) -> tsgo_ast::NodeId {
    use tsgo_ast::NodeData;
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("expected a source file"),
    };
    match arena.data(stmts[index]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected an expression statement"),
    }
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowAssignment (assignment reduction)
#[test]
fn flow_assignment_narrows_union() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare let x: string | number;\nx = \"a\";\nx;");
    let usage = top_level_expression(&p, 2);
    let mut c = Checker::new();
    let declared = c.string_or_number_type();
    let s = c.string_type();
    // After `x = "a"` (a string), the `string | number` reference narrows to
    // `string` (the constituent the assigned type is assignable to).
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), s);
}

// Resolves the first expression statement inside the `clause_index`-th clause
// of the top-level `switch` statement at `switch_index`.
fn switch_clause_usage(
    p: &StubProgram,
    switch_index: usize,
    clause_index: usize,
) -> tsgo_ast::NodeId {
    use tsgo_ast::NodeData;
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("expected a source file"),
    };
    let case_block = match arena.data(stmts[switch_index]) {
        NodeData::SwitchStatement(d) => d.case_block,
        _ => panic!("expected a switch statement"),
    };
    let clauses = match arena.data(case_block) {
        NodeData::CaseBlock(d) => d.clauses.nodes.clone(),
        _ => panic!("expected a case block"),
    };
    let clause_stmts = match arena.data(clauses[clause_index]) {
        NodeData::CaseOrDefaultClause(d) => d.statements.nodes.clone(),
        _ => panic!("expected a clause"),
    };
    match arena.data(clause_stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected an expression statement"),
    }
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminant (case)
#[test]
fn flow_switch_case_narrows_literal_union() {
    use crate::core::types::{LiteralValue, TypeFlags};
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string;\nswitch (x) {\n  case \"a\":\n    x;\n}",
    );
    let usage = switch_clause_usage(&p, 1, 0);
    let mut c = Checker::new();
    let a = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    let b = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("b".into()),
        None,
    );
    let union = c.get_union_type(&[a, b]);
    // Inside `case "a":`, the `"a" | "b"` reference narrows to `"a"`.
    assert_eq!(c.get_flow_type_of_reference(&p, usage, union), a);
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminant (default)
#[test]
fn flow_switch_default_narrows_complement() {
    use crate::core::types::{LiteralValue, TypeFlags};
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string;\nswitch (x) {\n  case \"a\":\n    break;\n  default:\n    x;\n}",
    );
    let usage = switch_clause_usage(&p, 1, 1);
    let mut c = Checker::new();
    let a = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    let b = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("b".into()),
        None,
    );
    let union = c.get_union_type(&[a, b]);
    // In the `default:` clause, `"a" | "b"` narrows to the complement `"b"`.
    assert_eq!(c.get_flow_type_of_reference(&p, usage, union), b);
}

// Go: internal/checker/flow.go:Checker.getFlowTypeOfReference (no guard)
#[test]
fn flow_no_condition_returns_declared() {
    use tsgo_ast::NodeData;
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: string | number;\nx;");
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let usage = match arena.data(stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let mut c = Checker::new();
    let declared = c.string_or_number_type();
    // With no narrowing condition, the flow type is the declared type.
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), declared);
}

// Go: internal/checker/flow.go:Checker.isReachableFlowNode
#[test]
fn reachable_flow_node_after_if() {
    use tsgo_ast::NodeData;
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const cond: boolean;\nlet x = 1;\nif (cond) {\n}\nx;",
    );
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    // The final `x;` usage flows through the post-`if` branch label.
    let usage = match arena.data(stmts[3]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let flow = p.flow_node_of(usage).expect("usage has a flow node");
    let c = Checker::new();
    assert!(c.is_reachable_flow_node(&p, flow));
}

// Go: internal/checker/flow.go:Checker.getPropertyNameForKnownSymbolName
#[test]
fn property_name_for_known_symbol_name_uses_at_prefixed_internal_name() {
    use tsgo_ast::symbol::{escape_all_internal_symbol_names, INTERNAL_SYMBOL_NAME_PREFIX};
    let c = Checker::new();
    // Go: with no global `Symbol` constructor providing a `unique symbol`, the
    // late name falls back to `InternalSymbolNamePrefix + "@" + name`.
    assert_eq!(
        c.get_property_name_for_known_symbol_name("iterator"),
        format!("{INTERNAL_SYMBOL_NAME_PREFIX}@iterator"),
    );
    assert_eq!(
        c.get_property_name_for_known_symbol_name("asyncIterator"),
        format!("{INTERNAL_SYMBOL_NAME_PREFIX}@asyncIterator"),
    );
    // The escaped (display) form matches Go's `"__@iterator"` exactly.
    assert_eq!(
        escape_all_internal_symbol_names(&c.get_property_name_for_known_symbol_name("iterator")),
        "__@iterator",
    );
}

// Go: internal/checker/flow.go:Checker.getTypePredicateArgument(2419)
#[test]
fn get_type_predicate_argument_returns_indexed_parameter() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare function isString(x: unknown): x is string;\ndeclare const v: unknown;\nisString(v);",
    );
    let call = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => match p.arena().data(d.statements.nodes[2]) {
            NodeData::ExpressionStatement(d) => d.expression,
            _ => panic!("expression statement"),
        },
        _ => panic!("source file"),
    };
    let c = Checker::new();
    let predicate = crate::core::flow::TypePredicateInfo {
        kind: crate::core::flow::TypePredicateKind::Identifier,
        parameter_index: 0,
        predicate_type: None,
    };
    let arg = c
        .get_type_predicate_argument(&p, &predicate, call)
        .expect("first argument");
    assert_eq!(p.arena().text(arg), "v");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypePredicate(309)
#[test]
fn narrow_type_by_type_predicate_narrows_matching_reference() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\ndeclare function isString(v: unknown): v is string;\nisString(x);",
    );
    let mut c = Checker::new();
    let x_sym = *p.locals(p.root()).expect("locals").get("x").expect("x");
    let x_decl = p.symbol(x_sym).declarations[0];
    let x_ref = match p.arena().data(x_decl) {
        NodeData::VariableDeclaration(d) => d.name,
        _ => panic!("variable declaration"),
    };
    let call = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => match p.arena().data(d.statements.nodes[2]) {
            NodeData::ExpressionStatement(d) => d.expression,
            _ => panic!("call statement"),
        },
        _ => panic!("source file"),
    };
    let union = c.get_union_type(&[c.string_type(), c.number_type()]);
    let predicate = crate::core::flow::TypePredicateInfo {
        kind: crate::core::flow::TypePredicateKind::Identifier,
        parameter_index: 0,
        predicate_type: Some(c.string_type()),
    };
    let narrowed = c.narrow_type_by_type_predicate(&p, x_ref, union, &predicate, call, true);
    assert_eq!(narrowed, c.string_type());
}
