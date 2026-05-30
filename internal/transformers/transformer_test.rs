use super::*;
use crate::test_support::{emit, parse_shared};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, VisitOptions};

// Go: internal/transformers/transformer.go:Transformer.TransformSourceFile
// Tracer bullet: an identity transform over a parsed SourceFile yields a
// structurally-equal SourceFile (re-emits to the same text), proving the
// EmitContext + visit driver + factory wiring end-to-end.
#[test]
fn identity_transform_round_trips_source_file() {
    let input = "const x = 1;\nf(x);";
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_transformer(Box::new(|_ec, node| node), Some(Rc::clone(&ec)));
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), input);
}

// Recursively rewrites every identifier `a` to `x`, rebuilding interior nodes
// through `visit_each_child` so the driver exercises arena node creation.
fn rename_a_to_x(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if arena.kind(node) == Kind::Identifier && arena.text(node) == "a" {
        return arena.new_identifier("x");
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| rename_a_to_x(a, c))
}

// Go: internal/transformers/transformer.go:Transformer.TransformSourceFile
// A non-identity transform rebuilds the tree (here renaming `a` -> `x`),
// proving the driver threads the EmitContext arena through `visit_each_child`
// and that factory-created replacement nodes flow back into emit.
#[test]
fn rewriting_transform_rebuilds_tree() {
    let input = "a;\nb;";
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_transformer(
        Box::new(|ec, node| rename_a_to_x(ec.arena_mut(), node)),
        Some(Rc::clone(&ec)),
    );
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "x;\nb;");
}

// Reports whether `node` is `<name>;` (an expression statement of identifier `name`).
fn is_expr_stmt_ident(arena: &NodeArena, node: NodeId, name: &str) -> bool {
    if arena.kind(node) != Kind::ExpressionStatement {
        return false;
    }
    let expr = match arena.data(node) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => return false,
    };
    arena.kind(expr) == Kind::Identifier && arena.text(expr) == name
}

// Drops the `a;` statement from a SourceFile via `visit_nodes_removable`.
fn drop_statement_a(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if arena.kind(node) != Kind::SourceFile {
        return node;
    }
    let (file_name, script_kind, language_variant, statements, eof) = match arena.data(node) {
        NodeData::SourceFile(d) => (
            d.file_name.clone(),
            d.script_kind,
            d.language_variant,
            d.statements.clone(),
            d.end_of_file_token,
        ),
        _ => unreachable!(),
    };
    let statements = arena.visit_nodes_removable(&statements, &mut |a, child| {
        if is_expr_stmt_ident(a, child, "a") {
            None
        } else {
            Some(child)
        }
    });
    arena.new_source_file(&file_name, script_kind, language_variant, statements, eof)
}

// Go: internal/ast/visitor.go:NodeVisitor.VisitNodes (nil-drop, end-to-end)
// Tracer bullet for removal-aware visiting: a transform drops one statement from
// a SourceFile and the result re-emits without it.
#[test]
fn transform_can_drop_a_statement() {
    let input = "a;\nb;";
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_transformer(
        Box::new(|ec, node| drop_statement_a(ec.arena_mut(), node)),
        Some(Rc::clone(&ec)),
    );
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "b;");
}
