use super::*;
use crate::core::test_support::StubProgram;

// Go: internal/compiler/program.go:Program (bound-file query surface)
#[test]
fn bound_program_exposes_arena_root_locals_and_symbols() {
    let p = StubProgram::parse_and_bind("/a.ts", "var x;");
    assert!(p.arena().node_count() > 0);

    let root = p.root();
    let table = p.locals(root).expect("source file has a locals table");
    let x = *table.get("x").expect("x is a file local");
    assert_eq!(p.symbol(x).name, "x");

    // The variable declaration node maps back to the same symbol.
    if let Some(decl) = p.symbol(x).value_declaration {
        assert_eq!(p.symbol_of_node(decl), Some(x));
    }
}

// Go: internal/compiler/program.go:Program (flow-graph query surface)
#[test]
fn bound_program_exposes_flow_nodes() {
    use tsgo_ast::flow::FlowFlags;
    use tsgo_ast::NodeData;
    let p = StubProgram::parse_and_bind("/a.ts", "var x;\nx;");
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let usage = match arena.data(stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    // A reachable reference has a flow node, and we can read it back.
    let flow = p.flow_node_of(usage).expect("usage has a flow node");
    let node = p.flow_node(flow);
    assert!(!node.flags.contains(FlowFlags::UNREACHABLE));
}

// Go: internal/compiler/program.go:Program (missing lookups are None)
#[test]
fn bound_program_missing_lookups_are_none() {
    let p = StubProgram::parse_and_bind("/a.ts", "var x;");
    let root = p.root();
    assert!(p
        .locals(root)
        .map(|t| t.get("nope").is_none())
        .unwrap_or(true));
}
