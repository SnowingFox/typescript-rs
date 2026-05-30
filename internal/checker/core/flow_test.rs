use crate::core::declared_types::get_declared_type_of_symbol;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::SymbolId;

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
