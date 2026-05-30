//! Behavior tests for control-flow graph construction.
//!
//! Flow-graph shape expectations follow the Go binder's flow routines.

use super::*;
use crate::{bind_source_file, BindResult, Binder};
use tsgo_ast::flow::FlowFlags;
use tsgo_ast::{NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

fn bind(src: &str) -> (NodeArena, NodeId, BindResult) {
    let r = parse_source_file(SourceFileParseOptions::default(), src, ScriptKind::Ts);
    let mut arena = r.arena;
    let sf = r.source_file;
    let result = bind_source_file(&mut arena, sf);
    (arena, sf, result)
}

// Go: internal/binder/binder.go:finishFlowLabel (single antecedent folds away)
#[test]
fn flow_finish_label_single_folds() {
    let mut arena = NodeArena::new();
    let mut b = Binder::new(&mut arena);
    let label = b.create_branch_label();
    let antecedent = b.new_flow_node(FlowFlags::ASSIGNMENT);
    b.add_antecedent(label, antecedent);
    assert_eq!(b.finish_flow_label(label), antecedent);
}

// Go: internal/binder/binder.go:addAntecedent (de-duplicates antecedents)
#[test]
fn flow_add_antecedent_dedup() {
    let mut arena = NodeArena::new();
    let mut b = Binder::new(&mut arena);
    let label = b.create_branch_label();
    let antecedent = b.new_flow_node(FlowFlags::ASSIGNMENT);
    b.add_antecedent(label, antecedent);
    b.add_antecedent(label, antecedent);
    let mut count = 0;
    let mut cur = b.flow_nodes[label.0 as usize].antecedents;
    while let Some(l) = cur {
        count += 1;
        cur = b.flow_lists[l.0 as usize].next;
    }
    assert_eq!(count, 1);
}

// Go: internal/binder/binder.go:bindIfStatement/createFlowCondition
#[test]
fn flow_if_creates_condition_nodes() {
    let (_arena, _sf, result) = bind("let x; if (x) {} else {}");
    let has_true = result
        .flow_nodes
        .iter()
        .any(|f| f.flags.contains(FlowFlags::TRUE_CONDITION));
    let has_false = result
        .flow_nodes
        .iter()
        .any(|f| f.flags.contains(FlowFlags::FALSE_CONDITION));
    let has_branch = result
        .flow_nodes
        .iter()
        .any(|f| f.flags.contains(FlowFlags::BRANCH_LABEL));
    assert!(has_true, "expected a true-condition flow node");
    assert!(has_false, "expected a false-condition flow node");
    assert!(has_branch, "expected a branch-label flow node");
}

// Go: internal/binder/binder.go:bindReturnStatement/bindChildren (unreachable marking)
#[test]
fn flow_unreachable_after_return() {
    let (arena, sf, result) = bind("function f(){ return; let y; }");
    let func = match arena.data(sf) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!(),
    };
    let body = match arena.data(func) {
        NodeData::FunctionDeclaration(d) => d.body.expect("f has a body"),
        _ => unreachable!(),
    };
    let var_stmt = match arena.data(body) {
        NodeData::Block(d) => d.list.nodes[1],
        _ => unreachable!(),
    };
    assert!(arena.flags(var_stmt).contains(NodeFlags::UNREACHABLE));
    // The binder produced an unreachable flow node (the shared sentinel).
    assert!(result
        .flow_nodes
        .iter()
        .any(|f| f.flags.contains(FlowFlags::UNREACHABLE)));
}

// Go: internal/binder/binder.go:isNarrowingExpression
#[test]
fn is_narrowing_expression_identifier_vs_keyword() {
    let (arena, sf) = {
        let r = parse_source_file(
            SourceFileParseOptions::default(),
            "x; true;",
            ScriptKind::Ts,
        );
        (r.arena, r.source_file)
    };
    let stmts = match arena.data(sf) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => unreachable!(),
    };
    let ident = match arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => unreachable!(),
    };
    let true_kw = match arena.data(stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => unreachable!(),
    };
    assert!(is_narrowing_expression(&arena, ident));
    assert!(!is_narrowing_expression(&arena, true_kw));
}

// Go: internal/binder/binder.go:bindWhileStatement (loop label)
#[test]
fn flow_while_loop_label() {
    let (_arena, _sf, result) = bind("while (true) { break; }");
    assert!(result
        .flow_nodes
        .iter()
        .any(|f| f.flags.contains(FlowFlags::LOOP_LABEL)));
}
