//! Port of Go `internal/transformers/modifiervisitor.go`: filtering a modifier
//! list down to an allowed subset.

use tsgo_ast::utilities::modifier_to_flag;
use tsgo_ast::{ModifierFlags, ModifierList, NodeId, NodeList};
use tsgo_printer::EmitContext;

/// Returns a copy of `modifiers` keeping only modifiers/decorators whose flag is
/// within `allowed` (non-modifier nodes, with an empty flag, are always kept).
///
/// Mirrors Go's `modifierVisitor` filter driven through `VisitModifiers`: an
/// unchanged list is returned as-is, and a filtered list keeps the original
/// list range with recomputed [`ModifierFlags`]. Returns `None` when `modifiers`
/// is `None`.
///
/// # Examples
/// ```
/// use tsgo_transformers::extract_modifiers;
/// use tsgo_ast::ModifierFlags;
/// use tsgo_printer::EmitContext;
/// let ec = EmitContext::new();
/// // An absent modifier list passes through as `None`.
/// assert!(extract_modifiers(&ec, None, ModifierFlags::EXPORT).is_none());
/// ```
///
/// Side effects: none (reads node kinds; allocates a new list value when
/// filtering, but does not touch the arena).
// Go: internal/transformers/modifiervisitor.go:ExtractModifiers
pub fn extract_modifiers(
    emit_context: &EmitContext,
    modifiers: Option<&ModifierList>,
    allowed: ModifierFlags,
) -> Option<ModifierList> {
    let modifiers = modifiers?;
    let arena = emit_context.arena();
    let mut kept: Vec<NodeId> = Vec::with_capacity(modifiers.list.nodes.len());
    for &node in &modifiers.list.nodes {
        let flag = modifier_to_flag(arena.kind(node));
        if flag.is_empty() || !(flag & allowed).is_empty() {
            kept.push(node);
        }
    }
    if kept.len() == modifiers.list.nodes.len() {
        return Some(modifiers.clone());
    }
    let modifier_flags = kept.iter().fold(ModifierFlags::empty(), |acc, &node| {
        acc | modifier_to_flag(arena.kind(node))
    });
    Some(ModifierList {
        list: NodeList {
            loc: modifiers.list.loc,
            nodes: kept,
        },
        modifier_flags,
    })
}

#[cfg(test)]
#[path = "modifiervisitor_test.rs"]
mod tests;
