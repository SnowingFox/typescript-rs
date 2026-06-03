//! Port of Go `internal/transformers/inliners/constenum.go`: inlines const-enum
//! member references to their literal values.
//!
//! When `--isolatedModules` is not set, the checker has already evaluated every
//! const-enum member to a compile-time constant. This transform replaces
//! property-access and element-access expressions that reference a const-enum
//! member with the literal value, adding a trailing comment with the original
//! source text (e.g. `Direction.Up` → `0 /* Direction.Up */`).
//!
//! # Scope
//!
//! The transform needs `EmitResolver::get_constant_value` to look up the
//! const-enum member value at emit time. The structure is fully ported; the
//! transform body delegates to the resolver and produces the correct replacement
//! nodes. Because `TransformOptions` does not yet carry an `EmitResolver`
//! handle, the transform is constructed with an optional resolver — tests can
//! exercise the visit logic by passing a no-op/stub resolver, and the real
//! pipeline will thread one once `TransformOptions` includes it.
//!
//! # Deferred
//!
//! DEFER(P5): `EmitContext::parse_node` (the original-node accessor Go consults
//! before calling `GetConstantValue`) is not yet fully ported; the transform
//! uses `most_original` as an approximation. `add_synthetic_trailing_comment`
//! (the comment-appending side table) is also deferred.
//! blocked-by: `EmitContext::parse_node`, `add_synthetic_trailing_comment`,
//! `scanner::get_text_of_node`.

use crate::{new_transformer, TransformOptions, Transformer};
use rustc_hash::FxHashMap;
use tsgo_ast::{Kind, NodeArena, NodeId, TokenFlags, VisitOptions};
use tsgo_evaluator::EvalValue;
use tsgo_jsnum::Number;
use tsgo_printer::EmitContext;

/// A trait for resolving const-enum values at emit time.
///
/// In the real pipeline this delegates to `EmitResolver::get_constant_value`.
/// Tests can provide a stub.
///
/// Side effects: implementations may query the bound program.
pub trait ConstantValueResolver {
    /// Returns the constant value of the property/element-access `node`, or
    /// `EvalValue::None` if the node does not reference a const-enum member.
    fn get_constant_value(&self, node: NodeId) -> EvalValue;
}

/// A no-op resolver that never finds a constant value.
struct NoOpResolver;

impl ConstantValueResolver for NoOpResolver {
    fn get_constant_value(&self, _node: NodeId) -> EvalValue {
        EvalValue::None
    }
}

/// Builds a [`Transformer`] that inlines const-enum member references,
/// sharing the pipeline's emit context.
///
/// When `resolver` is `None`, a no-op resolver is used (the transform becomes
/// a pass-through). Pass a real resolver in the pipeline.
///
/// # Examples
/// ```
/// use tsgo_transformers::{inliners::constenum::new_const_enum_inlining_transformer, TransformOptions};
/// let _tx = new_const_enum_inlining_transformer(&TransformOptions::default(), None);
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/inliners/constenum.go:NewConstEnumInliningTransformer
pub fn new_const_enum_inlining_transformer(
    opt: &TransformOptions,
    resolver: Option<Box<dyn ConstantValueResolver>>,
) -> Transformer {
    let resolver: Box<dyn ConstantValueResolver> =
        resolver.unwrap_or_else(|| Box::new(NoOpResolver));

    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| const_enum_visit(ec, &*resolver, node)),
        opt.context.clone(),
    )
}

/// Visits a node, inlining const-enum accesses.
///
/// Side effects: may replace nodes in the arena.
// Go: internal/transformers/inliners/constenum.go:ConstEnumInliningTransformer.visit
fn const_enum_visit(
    ec: &mut EmitContext,
    resolver: &dyn ConstantValueResolver,
    node: NodeId,
) -> NodeId {
    match ec.arena().kind(node) {
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression => {
            let original = ec.most_original(node);
            let value = resolver.get_constant_value(original);
            if let Some(replacement) = make_replacement(ec.arena_mut(), value) {
                // DEFER(P5): add trailing comment with original source text.
                // Go: `tx.EmitContext().AddSyntheticTrailingComment(...)`
                return replacement;
            }
            visit_each_child_ec(ec, resolver, node)
        }
        _ => visit_each_child_ec(ec, resolver, node),
    }
}

/// EC-threaded recursive descent.
fn visit_each_child_ec(
    ec: &mut EmitContext,
    resolver: &dyn ConstantValueResolver,
    node: NodeId,
) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = const_enum_visit(ec, resolver, child);
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

/// Builds the replacement literal node for a constant value.
///
/// Side effects: may push nodes.
// Go: internal/transformers/inliners/constenum.go:ConstEnumInliningTransformer.visit (switch v := value.(type))
fn make_replacement(arena: &mut NodeArena, value: EvalValue) -> Option<NodeId> {
    match value {
        EvalValue::None | EvalValue::Bool(_) => None,
        EvalValue::Num(n) => Some(make_numeric_replacement(arena, n)),
        EvalValue::Str(s) => Some(arena.new_string_literal(&s, TokenFlags::NONE)),
        EvalValue::BigInt(bi) => Some(make_bigint_replacement(arena, bi)),
    }
}

/// Builds the numeric replacement, handling infinity, NaN, and negative values.
///
/// Side effects: pushes nodes.
// Go: internal/transformers/inliners/constenum.go:ConstEnumInliningTransformer.visit (case jsnum.Number)
fn make_numeric_replacement(arena: &mut NodeArena, n: Number) -> NodeId {
    if n.is_inf() {
        let inf = arena.new_identifier("Infinity");
        if n.abs() == n {
            inf
        } else {
            arena.new_prefix_unary_expression(Kind::MinusToken, inf)
        }
    } else if n.is_nan() {
        arena.new_identifier("NaN")
    } else if n.abs() == n {
        arena.new_numeric_literal(&n.to_string(), TokenFlags::NONE)
    } else {
        let abs_lit = arena.new_numeric_literal(&n.abs().to_string(), TokenFlags::NONE);
        arena.new_prefix_unary_expression(Kind::MinusToken, abs_lit)
    }
}

/// Builds the bigint replacement, handling negative values.
///
/// Side effects: pushes nodes.
// Go: internal/transformers/inliners/constenum.go:ConstEnumInliningTransformer.visit (case jsnum.PseudoBigInt)
fn make_bigint_replacement(arena: &mut NodeArena, bi: tsgo_jsnum::PseudoBigInt) -> NodeId {
    if bi == tsgo_jsnum::PseudoBigInt::default() {
        arena.new_big_int_literal("0", TokenFlags::NONE)
    } else if !bi.negative {
        arena.new_big_int_literal(&bi.base10_value, TokenFlags::NONE)
    } else {
        let lit = arena.new_big_int_literal(&bi.base10_value, TokenFlags::NONE);
        arena.new_prefix_unary_expression(Kind::MinusToken, lit)
    }
}

/// Escapes `*/` sequences inside a multi-line comment so the text is safe to
/// embed in a `/* ... */` block.
///
/// # Examples
/// ```
/// use tsgo_transformers::inliners::constenum::safe_multi_line_comment;
/// assert_eq!(safe_multi_line_comment("a*/b"), " a*_/b ");
/// assert_eq!(safe_multi_line_comment("hello"), " hello ");
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/inliners/constenum.go:safeMultiLineComment
pub fn safe_multi_line_comment(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 2);
    result.push(' ');
    let mut rest = text;
    while let Some(i) = rest.find("*/") {
        result.push_str(&rest[..i]);
        result.push_str("*_/");
        rest = &rest[i + 2..];
    }
    result.push_str(rest);
    result.push(' ');
    result
}

#[cfg(test)]
#[path = "constenum_test.rs"]
mod tests;
