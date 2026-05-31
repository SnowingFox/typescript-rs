//! ES2015 **array-literal** and **call-argument** spread down-leveling
//! (`[...a, b]`, `f(...args)`) for pre-ES2015 targets, via the `__spreadArray`
//! runtime helper and `Function.prototype.apply`.
//!
//! # Ground truth
//!
//! The Go port (`internal/transformers/estransforms`) does **not** yet contain
//! the ES2015 spread transform — its `GetESTransformer` chain stops at
//! `NewES2016Transformer`, so older targets are not down-leveled at all. The
//! upstream reference is therefore `microsoft/TypeScript`
//! `src/compiler/transformers/es2015.ts:transformAndSpreadElements` /
//! `visitArrayLiteralExpression` / `visitCallExpression`, and the **exact**
//! emitted shapes are verified against `tsc --target es5` (recorded inline in
//! the tests). The emit-helper plumbing reuses the 6d-2 infrastructure
//! (`request_emit_helper` + unscoped helper name + source-file prologue attach),
//! exactly as `objectrestspread.rs` does for `__rest`.
//!
//! # Scope (round 6aa)
//!
//! - **Array-literal spread** (`isArgumentList = false`): an array literal with
//!   at least one spread element lowers to the nested `__spreadArray` segment
//!   form. Consecutive non-spread elements collapse into a literal segment; each
//!   spread element is its own segment. The accumulator starts at `[]` when the
//!   literal opens with a spread, else at the first literal segment, and each
//!   remaining segment folds in via `__spreadArray(acc, seg, pack)`. The `pack`
//!   flag is `true` for a spread segment and `false` for a literal segment.
//!   E.g. `[...a, b]` → `__spreadArray(__spreadArray([], a, true), [b], false)`,
//!   `[...a]` → `__spreadArray([], a, true)`,
//!   `[1, ...a, 2]` → `__spreadArray(__spreadArray([1], a, true), [2], false)`.
//! - **Call-argument spread** (`isArgumentList = true`): a call whose argument
//!   list contains a spread lowers to `<target>.apply(<thisArg>, <args>)`, where
//!   `<args>` is the spread form of the argument list. In an argument list the
//!   `pack` flag is **always** `false` (unlike array literals), and a lone
//!   spread argument that is not a packed array literal is passed directly
//!   (no `__spreadArray` wrapper). The receiver `this` is `void 0` for a plain
//!   identifier callee (`f(...args)` → `f.apply(void 0, args)`) and the captured
//!   receiver for a simple member callee (`o.m(...args)` → `o.m.apply(o, args)`).
//!
//! Deferred (DEFER(P5), see `estransforms/mod.rs`):
//!
//! - `new C(...args)` spread (`new (C.bind.apply(C, __spreadArray([void 0],
//!   args, false)))()`) — needs the construct + bind form. blocked-by: the
//!   `new`-target bind machinery.
//! - `super(...args)` spread — blocked-by: `super` receiver capture.
//! - Call spread with a **non-simple receiver** (`foo().m(...args)`,
//!   `a.b.m(...args)`) needing a hoisted temp for the captured `this` — the
//!   reachable subset captures only a plain identifier receiver. blocked-by:
//!   the `createCallBinding` temp-capture (variable environment threading).
//! - `--downlevelIteration` (`__read`/`__spread`) form. blocked-by: the
//!   iteration helpers.
//! - Object spread (`{ ...x }` → `Object.assign`) — already landed in 6d/6g
//!   (`objectrestspread.rs`).
//! - Wiring into `GetESTransformer` (no `NewES2015Transformer` chain exists
//!   yet) — DEFER with the `definitions` dispatch port.

use crate::{new_transformer, TransformOptions, Transformer};
use rustc_hash::FxHashMap;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, TokenFlags, VisitOptions};
use tsgo_printer::emithelpers::EmitHelper;
use tsgo_printer::EmitContext;

/// ES2015 `__spreadArray` — concatenates array-like segments, packing holes in
/// unpacked spread segments. Defined here (not in `tsgo_printer`) because the
/// printer crate is out of this round's edit scope, mirroring `forawait.rs`.
/// Text is verbatim from `tsc --target es5` (and `microsoft/TypeScript`
/// `src/compiler/factory/emitHelpers.ts:spreadArrayHelper`).
// Go: (no Go port yet) microsoft/TypeScript src/compiler/factory/emitHelpers.ts:spreadArrayHelper
pub static SPREAD_ARRAY_HELPER: EmitHelper = EmitHelper {
    name: "typescript:spreadArray",
    import_name: "__spreadArray",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};"#,
};

/// Builds a [`Transformer`] that lowers ES2015 array-literal and call-argument
/// spread, sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::spread::new_spread_transformer, TransformOptions};
/// let _tx = new_spread_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts (spread feature)
pub fn new_spread_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| spread_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit: lowers spread-containing array literals and
/// calls, and at the source-file boundary attaches the helpers requested during
/// the visit so the printer emits them in the prologue.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
fn spread_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::ArrayLiteralExpression if array_literal_has_spread(ec.arena(), node) => {
            visit_array_literal_expression(ec, node)
        }
        Kind::CallExpression if call_has_spread_argument(ec.arena(), node) => {
            // A spread-containing call lowers to `<target>.apply(<this>, <args>)`
            // when the callee is reachable (a plain identifier or a simple
            // member access). Otherwise (non-simple receiver, `super`, ...) we
            // recurse so any nested array spreads still lower — DEFER the apply
            // form itself.
            try_lower_call_with_spread(ec, node).unwrap_or_else(|| visit_each_child_ec(ec, node))
        }
        _ => visit_each_child_ec(ec, node),
    }
}

/// One segment of a partitioned element list: either a literal segment (a run of
/// consecutive non-spread elements collapsed into an array literal) or a spread
/// segment (the inner expression of a `...x` element).
struct Segment {
    /// The segment's expression (a literal `[...]` or a spread's inner value).
    expression: NodeId,
    /// Whether this segment came from a spread element.
    is_spread: bool,
}

/// Reports whether an array literal has at least one spread element.
///
/// Side effects: none (reads the arena).
fn array_literal_has_spread(arena: &NodeArena, node: NodeId) -> bool {
    let elements = match arena.data(node) {
        NodeData::ArrayLiteralExpression(d) => &d.list,
        _ => return false,
    };
    elements
        .nodes
        .iter()
        .any(|&e| arena.kind(e) == Kind::SpreadElement)
}

/// Lowers a spread-containing array literal to the nested `__spreadArray` segment
/// form (`isArgumentList = false`, so spread segments pack).
///
/// Side effects: may push rebuilt nodes; requests the `__spreadArray` helper.
// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:visitArrayLiteralExpression
fn visit_array_literal_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let elements = match ec.arena().data(node) {
        NodeData::ArrayLiteralExpression(d) => d.list.nodes.clone(),
        _ => unreachable!("kind checked above"),
    };
    transform_and_spread_elements(ec, &elements, false)
}

/// Partitions `elements` into spread / literal segments, then folds them into the
/// `__spreadArray` accumulator form. The accumulator starts at `[]` when the
/// list opens with a spread, else at the first literal segment; each remaining
/// segment folds in via `__spreadArray(acc, seg, pack)`, where `pack` is `true`
/// only for a spread segment in an array literal (`!is_argument_list`).
///
/// A single non-shortcut segment still folds (e.g. `[...a]` →
/// `__spreadArray([], a, true)`); the argument-list single-spread shortcut
/// (returning the bare segment expression) is handled by the call lowering.
///
/// Side effects: may push rebuilt nodes; requests the `__spreadArray` helper.
// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:transformAndSpreadElements
fn transform_and_spread_elements(
    ec: &mut EmitContext,
    elements: &[NodeId],
    is_argument_list: bool,
) -> NodeId {
    let segments = partition_segments(ec, elements);
    // Argument-list single-segment shortcut: a lone spread argument (not a
    // packed array literal, which is DEFER) is passed directly to `.apply`
    // without a `__spreadArray` wrapper. `tsc`: `f(...args)` -> `..., args)`.
    if segments.len() == 1 && is_argument_list {
        return segments[0].expression;
    }
    let starts_with_spread = segments[0].is_spread;
    let mut expression = if starts_with_spread {
        ec.arena_mut()
            .new_array_literal_expression(NodeList::new(vec![]))
    } else {
        segments[0].expression
    };
    let start = if starts_with_spread { 0 } else { 1 };
    for segment in &segments[start..] {
        let pack = segment.is_spread && !is_argument_list;
        expression = spread_array_helper(ec, expression, segment.expression, pack);
    }
    expression
}

/// Partitions an element list: runs of non-spread elements collapse into array
/// literal segments, and each spread element becomes its own segment carrying
/// its (visited) inner expression. Nested spreads in either are lowered.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:transformAndSpreadElements (spanMap)
fn partition_segments(ec: &mut EmitContext, elements: &[NodeId]) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut chunk: Vec<NodeId> = Vec::new();
    for &element in elements {
        if ec.arena().kind(element) == Kind::SpreadElement {
            if !chunk.is_empty() {
                let literal = ec
                    .arena_mut()
                    .new_array_literal_expression(NodeList::new(std::mem::take(&mut chunk)));
                segments.push(Segment {
                    expression: literal,
                    is_spread: false,
                });
            }
            let inner = match ec.arena().data(element) {
                NodeData::SpreadElement(d) => d.expression,
                _ => unreachable!("kind checked above"),
            };
            let inner = spread_visit(ec, inner);
            segments.push(Segment {
                expression: inner,
                is_spread: true,
            });
        } else {
            let visited = spread_visit(ec, element);
            chunk.push(visited);
        }
    }
    if !chunk.is_empty() {
        let literal = ec
            .arena_mut()
            .new_array_literal_expression(NodeList::new(chunk));
        segments.push(Segment {
            expression: literal,
            is_spread: false,
        });
    }
    segments
}

/// Reports whether a call expression's argument list contains a spread element.
///
/// Side effects: none (reads the arena).
fn call_has_spread_argument(arena: &NodeArena, node: NodeId) -> bool {
    let arguments = match arena.data(node) {
        NodeData::CallExpression(d) => &d.arguments,
        _ => return false,
    };
    arguments
        .nodes
        .iter()
        .any(|&a| arena.kind(a) == Kind::SpreadElement)
}

/// Lowers a spread-containing call `<callee>(<args...>)` to
/// `<target>.apply(<thisArg>, <args>)`, where `<args>` is the argument list's
/// spread form. Returns `None` for callees outside the reachable subset (so the
/// caller recurses and the call is left structurally unchanged — DEFER):
///
/// - a plain identifier callee `f` → `f.apply(void 0, <args>)`;
/// - a simple member callee `o.m` (identifier receiver) → `o.m.apply(o, <args>)`,
///   reusing the receiver identifier as the captured `this` (no temp needed).
///
/// `new`/`super` calls and non-simple member receivers (needing a hoisted temp)
/// are DEFER.
///
/// Side effects: may push rebuilt nodes; may request the `__spreadArray` helper.
// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:visitCallExpression
fn try_lower_call_with_spread(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let (callee, arguments) = match ec.arena().data(node) {
        NodeData::CallExpression(d) => (d.expression, d.arguments.nodes.clone()),
        _ => return None,
    };
    let (target, this_arg) = match ec.arena().kind(callee) {
        Kind::Identifier => (callee, make_void_zero(ec)),
        Kind::PropertyAccessExpression => {
            let receiver = match ec.arena().data(callee) {
                NodeData::PropertyAccessExpression(d) => d.expression,
                _ => unreachable!("kind checked above"),
            };
            // Only a plain identifier receiver is duplicable without a temp; a
            // non-simple receiver (`foo().m`, `a.b.m`) needs a hoisted capture
            // temp — DEFER.
            if ec.arena().kind(receiver) != Kind::Identifier {
                return None;
            }
            (callee, receiver)
        }
        _ => return None,
    };
    let args = transform_and_spread_elements(ec, &arguments, true);
    let apply_name = ec.arena_mut().new_identifier("apply");
    let apply_access = ec
        .arena_mut()
        .new_property_access_expression(target, None, apply_name);
    Some(ec.arena_mut().new_call_expression(
        apply_access,
        None,
        None,
        NodeList::new(vec![this_arg, args]),
        NodeFlags::NONE,
    ))
}

/// Builds a `void 0` expression (the `apply` receiver for a non-member callee).
///
/// Side effects: pushes the literal/void nodes onto the arena.
// Go: internal/printer/factory.go:NodeFactory.NewVoidZeroExpression
fn make_void_zero(ec: &mut EmitContext) -> NodeId {
    let zero = ec.arena_mut().new_numeric_literal("0", TokenFlags::NONE);
    ec.arena_mut().new_void_expression(zero)
}

/// Builds `__spreadArray(to, from, pack)`, requesting the `__spreadArray` helper
/// so its definition is emitted in the module prologue. `pack` is emitted as the
/// `true`/`false` keyword.
///
/// Side effects: pushes the call/keyword nodes; requests the `__spreadArray` helper.
// Go: (no Go port yet) microsoft/TypeScript src/compiler/factory/emitHelpers.ts:createSpreadArrayHelper
fn spread_array_helper(ec: &mut EmitContext, to: NodeId, from: NodeId, pack: bool) -> NodeId {
    ec.request_emit_helper(&SPREAD_ARRAY_HELPER);
    let name = ec.factory().new_unscoped_helper_name("__spreadArray");
    let pack_kind = if pack {
        Kind::TrueKeyword
    } else {
        Kind::FalseKeyword
    };
    let pack_node = ec.arena_mut().new_keyword_expression(pack_kind);
    ec.arena_mut().new_call_expression(
        name,
        None,
        None,
        NodeList::new(vec![to, from, pack_node]),
        NodeFlags::NONE,
    )
}

/// Emit-context-threaded `VisitEachChild`: recursively runs [`spread_visit`] over
/// every child, then rebuilds the node with the transformed children. The node
/// is returned unchanged when no child changed.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = spread_visit(ec, child);
        if transformed != child {
            replacements.insert(child, transformed);
        }
    }
    if replacements.is_empty() {
        return node;
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    ec.arena_mut()
        .visit_each_child(node, opts, &mut |_, child| {
            replacements.get(&child).copied().unwrap_or(child)
        })
}

/// Visits the source file's statements, then attaches the helpers requested
/// during the visit so the printer emits them in the prologue.
///
/// Side effects: rebuilds the source file; attaches emit helpers.
fn visit_source_file(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (file_name, script_kind, language_variant, statements, end_of_file_token) =
        match ec.arena().data(node) {
            NodeData::SourceFile(d) => (
                d.file_name.clone(),
                d.script_kind,
                d.language_variant,
                d.statements.clone(),
                d.end_of_file_token,
            ),
            _ => unreachable!("kind/data mismatch"),
        };
    let visited: Vec<NodeId> = statements
        .nodes
        .iter()
        .copied()
        .map(|s| spread_visit(ec, s))
        .collect();
    let new_source_file = ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(visited),
        end_of_file_token,
    );
    for helper in ec.read_emit_helpers() {
        ec.add_emit_helper(new_source_file, helper);
    }
    new_source_file
}

#[cfg(test)]
#[path = "spread_test.rs"]
mod tests;
