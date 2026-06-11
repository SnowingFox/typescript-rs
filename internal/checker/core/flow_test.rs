use crate::core::declared_types::get_declared_type_of_symbol;
use crate::core::program::BoundProgram;
use std::rc::Rc;
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
    assert_eq!(c.narrow_type_by_in(&p, union, "a", true), a_ty);
    assert_eq!(c.narrow_type_by_in(&p, union, "a", false), b_ty);
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

// Go: internal/checker/flow.go:Checker.getTypeAtFlowAssignment (for-in non-null)
#[test]
fn flow_for_in_non_null_narrows_nullable_object() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let obj: { [key: string]: number } | null;\n\
         for (const _ in obj) {\n  const o: { [key: string]: number } = obj;\n}",
    );
    let arena = p.arena();
    let for_stmt = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[1],
        _ => panic!("source file"),
    };
    let body = match arena.data(for_stmt) {
        NodeData::ForInOrOfStatement(d) => d.statement,
        _ => panic!("for-in"),
    };
    let body_stmt = match arena.data(body) {
        NodeData::Block(d) => d.list.nodes[0],
        _ => panic!("block"),
    };
    let init = match arena.data(body_stmt) {
        NodeData::VariableStatement(d) => {
            let list = d.declaration_list;
            let decl = match arena.data(list) {
                NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
                _ => panic!("vdl"),
            };
            match arena.data(decl) {
                NodeData::VariableDeclaration(d) => d.initializer.expect("init"),
                _ => panic!("decl"),
            }
        }
        _ => panic!("var stmt"),
    };
    let usage = match arena.data(init) {
        NodeData::Identifier(_) => init,
        _ => panic!("obj id"),
    };
    assert!(
        p.flow_node_of(usage).is_some(),
        "for-in body reference should have a flow node"
    );
    let p = std::rc::Rc::new(p);
    let view = std::rc::Rc::clone(&p);
    let mut c = Checker::new_checker(p);
    let declared = get_declared_type_of_symbol(&mut c, view.as_ref(), sym(view.as_ref(), "obj"), None);
    let narrowed = c.get_flow_type_of_reference(view.as_ref(), usage, declared);
    assert!(
        !c.get_type(narrowed)
            .flags()
            .intersects(crate::core::types::TypeFlags::NULLABLE),
        "expected non-null obj in for-in body, got {:?}",
        narrowed
    );
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowAssignment (containsMatchingReference)
#[test]
fn flow_parent_assignment_resets_child_property() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let obj: { a: number | string };\n\
         if (typeof obj.a === \"number\") {\n\
           obj = { a: \"hello\" };\n\
           obj.a;\n\
         }",
    );
    let arena = p.arena();
    let if_stmt = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[1],
        _ => panic!("source file"),
    };
    let then_stmt = match arena.data(if_stmt) {
        NodeData::IfStatement(d) => d.then_statement,
        _ => panic!("if"),
    };
    let block_stmts = match arena.data(then_stmt) {
        NodeData::Block(d) => d.list.nodes.clone(),
        _ => panic!("block"),
    };
    let usage = match arena.data(block_stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expr stmt"),
    };
    let mut c = Checker::new();
    let number = c.number_type();
    let string = c.string_type();
    let declared = c.get_union_type(&[number, string]);
    let narrowed = c.get_flow_type_of_reference(&p, usage, declared);
    assert_eq!(
        narrowed, declared,
        "parent assignment must reset child property to declared union, got {:?}",
        narrowed
    );
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTruthiness (optional-chain containment)
#[test]
fn flow_optional_chain_truthiness_narrows_object() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let obj: { foo: number } | null | undefined;\n\
         if (obj?.foo) {\n  obj;\n}",
    );
    let usage = first_then_block_usage(&p, 1);
    let mut c = Checker::new();
    let declared = get_declared_type_of_symbol(&mut c, &p, sym(&p, "obj"), None);
    let narrowed = c.get_flow_type_of_reference(&p, usage, declared);
    assert!(
        !c.get_type(narrowed)
            .flags()
            .intersects(crate::core::types::TypeFlags::NULLABLE),
        "expected non-nullish obj type, got {:?}",
        narrowed
    );
}

// ---- T1-E batch 135: evolving-array flow narrowing ----

const EVOLVING_ARRAY_LIB: &str = "interface Array<T> {\n  [n: number]: T;\n  length: number;\n  push(...items: T[]): number;\n  unshift(...items: T[]): number;\n}\n";

fn stmt_expr(p: &StubProgram, idx: usize) -> tsgo_ast::NodeId {
    use tsgo_ast::NodeData;
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    match arena.data(stmts[idx]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    }
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowArrayMutation
#[test]
fn flow_push_narrows_to_number_array() {
    let stub = StubProgram::parse_and_bind(
        "/a.ts",
        &format!("{EVOLVING_ARRAY_LIB}let arr = [];\narr.push(1);\narr;"),
    );
    let usage = stmt_expr(&stub, 3);
    let p: std::rc::Rc<dyn BoundProgram> = std::rc::Rc::new(stub);
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p));
    let declared = c.auto_array_type();
    let narrowed = c.get_flow_type_of_reference(p.as_ref(), usage, declared);
    let array_target = c.get_global_type("Array").expect("Array global");
    let expected = c.create_type_reference(array_target, vec![c.number_type()]);
    assert_eq!(narrowed, expected);
}

// Go: internal/checker/flow.go:Checker.getEvolvingArrayType
#[test]
fn evolving_array_type_interns_by_element_type() {
    let mut c = Checker::new();
    let n = c.number_type();
    let s = c.string_type();
    let e1 = c.get_evolving_array_type(n);
    let e2 = c.get_evolving_array_type(n);
    let e3 = c.get_evolving_array_type(s);
    assert_eq!(e1, e2);
    assert_ne!(e1, e3);
    assert!(c.is_evolving_array_type(e1));
}

// Go: internal/checker/flow.go:Checker.finalizeEvolvingArrayType
#[test]
fn finalize_evolving_array_never_element_becomes_auto_array() {
    let mut c = Checker::new();
    let p = empty();
    let evolving = c.get_evolving_array_type(c.never_type());
    let finalized = c.finalize_evolving_array_type(&p, evolving);
    assert_eq!(finalized, c.auto_array_type());
}

// Go: internal/checker/flow.go:Checker.isEvolvingArrayOperationTarget
#[test]
fn evolving_array_operation_target_recognizes_push_receiver() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        &format!("{EVOLVING_ARRAY_LIB}let arr = [];\narr.push(1);"),
    );
    let push_call = stmt_expr(&p, 2);
    let arena = p.arena();
    let arr_ref = match arena.data(push_call) {
        NodeData::CallExpression(d) => match arena.data(d.expression) {
            NodeData::PropertyAccessExpression(pa) => pa.expression,
            _ => panic!("property access"),
        },
        _ => panic!("call"),
    };
    let mut c = Checker::new();
    assert!(c.is_evolving_array_operation_target(&p, arr_ref));
}

// Go: internal/checker/flow.go:Checker.isEmptyArrayAssignment
#[test]
fn empty_array_initializer_starts_evolving_array_in_flow() {
    use rustc_hash::FxHashMap;
    let stub = StubProgram::parse_and_bind(
        "/a.ts",
        &format!("{EVOLVING_ARRAY_LIB}let arr = [];\narr;"),
    );
    let usage = stmt_expr(&stub, 2);
    let p: std::rc::Rc<dyn BoundProgram> = std::rc::Rc::new(stub);
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p));
    let declared = c.auto_array_type();
    let flow_id = p.flow_node_of(usage).expect("usage flow");
    let mut cache = FxHashMap::default();
    let flow_type = c.get_type_at_flow_node(p.as_ref(), usage, declared, flow_id, &mut cache);
    assert!(
        c.is_evolving_array_type(flow_type),
        "expected evolving array after `let arr = []`"
    );
    assert_eq!(
        c.finalize_evolving_array_type(p.as_ref(), flow_type),
        c.auto_array_type()
    );
}

// ---- T1-E batch 137: union-of-evolving-array at loop/branch junctions ----

// Go: internal/checker/flow.go:isEvolvingArrayTypeList(1499)
#[test]
fn is_evolving_array_type_list_requires_all_non_never_evolving() {
    let mut c = Checker::new();
    let n = c.number_type();
    let s = c.string_type();
    let e1 = c.get_evolving_array_type(n);
    let e2 = c.get_evolving_array_type(s);
    assert!(c.is_evolving_array_type_list(&[e1, e2]));
    assert!(!c.is_evolving_array_type_list(&[e1, c.auto_array_type()]));
    assert!(!c.is_evolving_array_type_list(&[c.auto_array_type()]));
    assert!(!c.is_evolving_array_type_list(&[]));
    assert!(!c.is_evolving_array_type_list(&[c.never_type()]));
}

// Go: internal/checker/flow.go:Checker.getUnionOrEvolvingArrayType(1286)
#[test]
fn union_or_evolving_array_combines_element_types() {
    let mut c = Checker::new();
    let p = empty();
    let n = c.number_type();
    let s = c.string_type();
    let e_num = c.get_evolving_array_type(n);
    let e_str = c.get_evolving_array_type(s);
    let combined = c.get_union_or_evolving_array_type(
        &p,
        c.auto_array_type(),
        &[e_num, e_str],
        false,
    );
    assert!(c.is_evolving_array_type(combined));
    let union_elem = c.get_union_type(&[n, s]);
    let expected = c.create_array_type(union_elem);
    assert_eq!(c.finalize_evolving_array_type(&p, combined), expected);
}

// Go: internal/checker/flow.go:Checker.getUnionOrEvolvingArrayType(1286)
#[test]
fn union_or_evolving_array_finalizes_mixed_antecedents() {
    let mut c = Checker::new();
    let p = empty();
    let n = c.number_type();
    let e_num = c.get_evolving_array_type(n);
    let number_array = c.create_array_type(n);
    let combined = c.get_union_or_evolving_array_type(
        &p,
        c.auto_array_type(),
        &[e_num, number_array],
        false,
    );
    assert!(!c.is_evolving_array_type(combined));
    assert_eq!(combined, number_array);
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowLoopLabel(1297)
#[test]
fn loop_push_narrows_to_number_array_at_use() {
    let stub = StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{EVOLVING_ARRAY_LIB}let arr = [];\n\
             while (true) {{\n  arr.push(1);\n  break;\n}}\narr;"
        ),
    );
    let usage = stmt_expr(&stub, 3);
    let p: std::rc::Rc<dyn BoundProgram> = std::rc::Rc::new(stub);
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p));
    let declared = c.auto_array_type();
    let narrowed = c.get_flow_type_of_reference(p.as_ref(), usage, declared);
    let array_target = c.get_global_type("Array").expect("Array");
    let expected = c.create_type_reference(array_target, vec![c.number_type()]);
    assert_eq!(narrowed, expected);
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowBranchLabel(1225)
#[test]
fn branch_push_unions_evolving_element_types() {
    let stub = StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{EVOLVING_ARRAY_LIB}declare const cond: boolean;\n\
             let arr = [];\n\
             if (cond) {{ arr.push(1); }} else {{ arr.push(\"a\"); }}\n\
             arr;"
        ),
    );
    let usage = stmt_expr(&stub, 4);
    let p: std::rc::Rc<dyn BoundProgram> = std::rc::Rc::new(stub);
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p));
    let declared = c.auto_array_type();
    let narrowed = c.get_flow_type_of_reference(p.as_ref(), usage, declared);
    let union_elem = c.get_union_type(&[c.number_type(), c.string_type()]);
    let array_target = c.get_global_type("Array").expect("Array");
    let expected = c.create_type_reference(array_target, vec![union_elem]);
    assert_eq!(narrowed, expected);
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowBranchLabel(1225)
#[test]
fn branch_one_arm_push_finalizes_to_number_array() {
    let stub = StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{EVOLVING_ARRAY_LIB}declare const cond: boolean;\n\
             let arr = [];\n\
             if (cond) {{ arr.push(1); }}\n\
             arr;"
        ),
    );
    let usage = stmt_expr(&stub, 4);
    let p: std::rc::Rc<dyn BoundProgram> = std::rc::Rc::new(stub);
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p));
    let declared = c.auto_array_type();
    let narrowed = c.get_flow_type_of_reference(p.as_ref(), usage, declared);
    let array_target = c.get_global_type("Array").expect("Array");
    let expected = c.create_type_reference(array_target, vec![c.number_type()]);
    assert_eq!(narrowed, expected);
}

// ---- T1-E batch 136: unreachable-never flow type ----

use crate::core::types::TypeFlags;
use tsgo_ast::flow::FlowNodeId;
use tsgo_ast::FlowFlags;

// Go: internal/checker/checker.go:Checker.unreachableNeverType
#[test]
fn unreachable_never_type_is_distinct_intrinsic_never() {
    let c = Checker::new();
    assert_ne!(c.unreachable_never_type(), c.never_type());
    assert!(
        c.get_type(c.unreachable_never_type())
            .flags()
            .contains(TypeFlags::NEVER)
    );
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowNode (UNREACHABLE sentinel)
#[test]
fn unreachable_flow_node_yields_unreachable_never() {
    let p = StubProgram::parse_and_bind("/a.ts", "function f(){ return; }");
    let unreachable = (0..64)
        .map(FlowNodeId)
        .find(|&id| p.flow_node(id).flags.contains(FlowFlags::UNREACHABLE))
        .expect("binder emits an UNREACHABLE flow sentinel");
    let mut c = Checker::new();
    let declared = c.string_type();
    let mut cache = rustc_hash::FxHashMap::default();
    let usage = p.root();
    let narrowed = c.get_type_at_flow_node(&p, usage, declared, unreachable, &mut cache);
    assert_eq!(narrowed, c.unreachable_never_type());
}

// Go: internal/checker/flow.go:Checker.getFlowTypeOfReference (post-process)
#[test]
fn unreachable_flow_post_processing_restores_declared_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\nfunction f() { return; x = \"a\"; x; }",
    );
    let arena = p.arena();
    let func = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[1],
        _ => panic!("function"),
    };
    let body = match arena.data(func) {
        NodeData::FunctionDeclaration(d) => d.body.expect("body"),
        _ => panic!("function body"),
    };
    let usage = match arena.data(body) {
        NodeData::Block(d) => match arena.data(*d.list.nodes.last().unwrap()) {
            NodeData::ExpressionStatement(es) => es.expression,
            _ => panic!("usage stmt"),
        },
        _ => panic!("block"),
    };
    let mut c = Checker::new();
    let declared = c.string_or_number_type();
    let flow_type = c.get_flow_type_of_reference(&p, usage, declared);
    assert_eq!(flow_type, declared);
}

// Go: internal/checker/flow.go:Checker.getFlowTypeOfReference (non-null parent)
#[test]
fn non_null_parent_truthy_never_facts_restore_declared() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: null | undefined;\nif (x) { const y = x!; y; }",
    );
    let arena = p.arena();
    let if_stmt = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[1],
        _ => panic!("if"),
    };
    let then_block = match arena.data(if_stmt) {
        NodeData::IfStatement(d) => d.then_statement,
        _ => panic!("if then"),
    };
    let usage = match arena.data(then_block) {
        NodeData::Block(d) => match arena.data(*d.list.nodes.last().unwrap()) {
            NodeData::ExpressionStatement(es) => es.expression,
            _ => panic!("usage"),
        },
        _ => panic!("block"),
    };
    let mut c = Checker::new();
    let declared = get_declared_type_of_symbol(&mut c, &p, sym(&p, "x"), None);
    let flow_type = c.get_flow_type_of_reference(&p, usage, declared);
    assert_eq!(flow_type, declared);
}

// Go: internal/checker/flow.go:Checker.narrowTypeByAssertion (`false`)
#[test]
fn assertion_false_yields_unreachable_never_internally() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare let x: string;\nfalse;");
    let arena = p.arena();
    let x = match arena.data(p.root()) {
        NodeData::SourceFile(d) => match arena.data(d.statements.nodes[0]) {
            NodeData::VariableStatement(vs) => {
                let list = match arena.data(vs.declaration_list) {
                    NodeData::VariableDeclarationList(dl) => dl.declarations.nodes[0],
                    _ => panic!("decl"),
                };
                match arena.data(list) {
                    NodeData::VariableDeclaration(vd) => vd.name,
                    _ => panic!("var"),
                }
            }
            _ => panic!("stmt"),
        },
        _ => panic!("sf"),
    };
    let false_kw = match arena.data(p.root()) {
        NodeData::SourceFile(d) => match arena.data(d.statements.nodes[1]) {
            NodeData::ExpressionStatement(es) => es.expression,
            _ => panic!("false"),
        },
        _ => panic!("sf2"),
    };
    let mut c = Checker::new();
    let declared = c.string_type();
    let narrowed = c.narrow_type_by_assertion(&p, x, declared, false_kw);
    assert_eq!(narrowed, c.unreachable_never_type());
    assert_ne!(narrowed, c.never_type());
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowAssignment (unreachable match)
#[test]
fn unreachable_direct_assignment_flow_restores_declared() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\nfunction f() { return; x = 1; x; }",
    );
    let arena = p.arena();
    let func = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[1],
        _ => panic!("function"),
    };
    let body = match arena.data(func) {
        NodeData::FunctionDeclaration(d) => d.body.expect("body"),
        _ => panic!("body"),
    };
    let usage = match arena.data(body) {
        NodeData::Block(d) => match arena.data(*d.list.nodes.last().unwrap()) {
            NodeData::ExpressionStatement(es) => es.expression,
            _ => panic!("usage"),
        },
        _ => panic!("block"),
    };
    let mut c = Checker::new();
    let declared = c.string_or_number_type();
    assert_eq!(c.get_flow_type_of_reference(&p, usage, declared), declared);
}

