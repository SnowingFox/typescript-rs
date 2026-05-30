//! Port of Go `internal/transformers/estransforms/objectrestspread.go`: lowers
//! ES2018 object spread in object literals to `Object.assign` calls.
//!
//! # Scope (round 6d)
//!
//! Lowers object **spread** inside an object literal: `{ ...x }` →
//! `Object.assign({}, x)`, `{ ...x, y }` → `Object.assign(Object.assign({}, x),
//! { y })`, chunking adjacent non-spread properties into object literals and
//! folding them pairwise through `Object.assign` (matching Go's
//! `NewAssignHelper`, which emits `Object.assign` directly — no helper import).
//!
//! Deferred (DEFER(P5), see `estransforms/mod.rs`): object **rest** binding
//! (`const { a, ...rest } = o` → `__rest`), rest in parameters / `for-of` /
//! `catch` / assignment destructuring patterns — these need the `__rest`
//! helper-library emit and the destructuring transformer, not yet ported.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, VisitOptions};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers object spread, sharing the pipeline's
/// emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::objectrestspread::new_object_rest_spread_transformer, TransformOptions};
/// let _tx = new_object_rest_spread_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/objectrestspread.go:newObjectRestSpreadTransformer
pub fn new_object_rest_spread_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| {
            object_rest_spread_visit(ec.arena_mut(), node)
        }),
        opt.context.clone(),
    )
}

/// Lowers object spread in the subtree rooted at `node`.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/objectrestspread.go:objectRestSpreadTransformer.visit
fn object_rest_spread_visit(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if arena.kind(node) == Kind::ObjectLiteralExpression && object_literal_has_spread(arena, node) {
        return visit_object_literal_expression(arena, node);
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| object_rest_spread_visit(a, c))
}

/// Reports whether an object literal has at least one spread element.
///
/// Side effects: none (reads the arena).
fn object_literal_has_spread(arena: &NodeArena, node: NodeId) -> bool {
    let properties = match arena.data(node) {
        NodeData::ObjectLiteralExpression(d) => &d.list,
        _ => return false,
    };
    properties
        .nodes
        .iter()
        .any(|&p| arena.kind(p) == Kind::SpreadAssignment)
}

/// Lowers a spread-containing object literal to `Object.assign(...)` calls.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/objectrestspread.go:visitObjectLiteralExpression
fn visit_object_literal_expression(arena: &mut NodeArena, node: NodeId) -> NodeId {
    let properties = match arena.data(node) {
        NodeData::ObjectLiteralExpression(d) => d.list.clone(),
        _ => unreachable!("kind checked above"),
    };
    let mut objects = chunk_object_literal_elements(arena, &properties);
    // If the first chunk is not an object literal (the literal opens with a
    // spread), prepend an empty `{}` so `Object.assign` has a fresh target.
    if objects
        .first()
        .is_none_or(|&o| arena.kind(o) != Kind::ObjectLiteralExpression)
    {
        let empty = arena.new_object_literal_expression(NodeList::new(vec![]));
        objects.insert(0, empty);
    }
    if objects.len() > 1 {
        let mut expression = objects[0];
        for &obj in &objects[1..] {
            expression = assign_helper(arena, vec![expression, obj]);
        }
        expression
    } else {
        assign_helper(arena, objects)
    }
}

/// Chunks an object literal's properties: runs of non-spread elements collapse
/// into object literals, and each spread element's target is emitted in place,
/// yielding the ordered `Object.assign` argument segments.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/objectrestspread.go:chunkObjectLiteralElements
fn chunk_object_literal_elements(arena: &mut NodeArena, list: &NodeList) -> Vec<NodeId> {
    let mut chunk_object: Vec<NodeId> = Vec::new();
    let mut objects: Vec<NodeId> = Vec::new();
    for &element in &list.nodes {
        if arena.kind(element) == Kind::SpreadAssignment {
            if !chunk_object.is_empty() {
                let chunk = arena.new_object_literal_expression(NodeList::new(std::mem::take(
                    &mut chunk_object,
                )));
                objects.push(chunk);
            }
            let target = match arena.data(element) {
                NodeData::SpreadAssignment(d) => d.expression,
                _ => continue,
            };
            let target = object_rest_spread_visit(arena, target);
            objects.push(target);
        } else {
            let visited = object_rest_spread_visit(arena, element);
            chunk_object.push(visited);
        }
    }
    if !chunk_object.is_empty() {
        let chunk = arena.new_object_literal_expression(NodeList::new(chunk_object));
        objects.push(chunk);
    }
    objects
}

/// Builds `Object.assign(<args>)`.
///
/// Side effects: pushes the access/call nodes onto the arena.
// Go: internal/printer/factory.go:NodeFactory.NewAssignHelper (Object.assign form)
fn assign_helper(arena: &mut NodeArena, args: Vec<NodeId>) -> NodeId {
    let object = arena.new_identifier("Object");
    let assign = arena.new_identifier("assign");
    let callee = arena.new_property_access_expression(object, None, assign);
    arena.new_call_expression(callee, None, None, NodeList::new(args), NodeFlags::NONE)
}

#[cfg(test)]
#[path = "objectrestspread_test.rs"]
mod tests;
