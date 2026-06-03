//! Port of Go `internal/transformers/estransforms/using.go`: lowers `using` and
//! `await using` declarations to `try`/`finally` with
//! `__addDisposableResource`/`__disposeResources` helpers.
//!
//! DEFER(P5/parser): the `tsgo_parser` crate does not parse statement-level
//! `using x = expr;` (reports "';' expected"), so the stage cannot be exercised
//! through the parse→transform→emit path and the parser is out of this round's
//! edit scope. The structure (types + helper utilities) is ported so call sites
//! can reference them; the transform body is a no-op pass-through.
//! blocked-by: parser `using` declaration support, `NodeFactory` helper
//! constructors (`NewAddDisposableResourceHelper`, `NewDisposeResourcesHelper`),
//! `EmitContext` variable environment start/end, export binding hoisting.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_printer::EmitContext;

/// The kind of `using` declaration.
///
/// Side effects: none (pure value type).
// Go: internal/transformers/estransforms/using.go:usingKind
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum UsingKind {
    /// Not a using declaration.
    None,
    /// `using x = expr;`
    Sync,
    /// `await using x = expr;`
    Async,
}

/// Builds a [`Transformer`] that lowers `using`/`await using` declarations.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::using::new_using_declaration_transformer, TransformOptions};
/// let _tx = new_using_declaration_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/using.go:newUsingDeclarationTransformer
pub fn new_using_declaration_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|_ec: &mut EmitContext, node: NodeId| {
            // DEFER(P5/parser): pass-through until parser supports `using`.
            node
        }),
        opt.context.clone(),
    )
}

/// Reports whether a variable declaration list is a `using` or `await using`
/// declaration.
///
/// # Examples
/// ```
/// use tsgo_transformers::estransforms::using::get_using_kind_of_flags;
/// use tsgo_ast::NodeFlags;
/// assert_eq!(get_using_kind_of_flags(NodeFlags::USING), tsgo_transformers::estransforms::using::UsingKind::Sync);
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/estransforms/using.go:getUsingKindOfVariableDeclarationList
pub fn get_using_kind_of_flags(flags: NodeFlags) -> UsingKind {
    let block_scoped = flags & NodeFlags::BLOCK_SCOPED;
    if block_scoped == NodeFlags::AWAIT_USING {
        UsingKind::Async
    } else if block_scoped == NodeFlags::USING {
        UsingKind::Sync
    } else {
        UsingKind::None
    }
}

/// Reports the `UsingKind` of a statement (if it is a variable statement with
/// `using`/`await using` flags).
///
/// Side effects: none (pure).
// Go: internal/transformers/estransforms/using.go:getUsingKind
pub fn get_using_kind(arena: &NodeArena, statement: NodeId) -> UsingKind {
    if arena.kind(statement) != Kind::VariableStatement {
        return UsingKind::None;
    }
    let decl_list = match arena.data(statement) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => return UsingKind::None,
    };
    let flags = arena.flags(decl_list);
    get_using_kind_of_flags(flags)
}

/// Reports the highest `UsingKind` across a slice of statements. If any
/// statement is `await using`, returns `Async`.
///
/// Side effects: none (pure).
// Go: internal/transformers/estransforms/using.go:getUsingKindOfStatements
pub fn get_using_kind_of_statements(arena: &NodeArena, statements: &[NodeId]) -> UsingKind {
    let mut result = UsingKind::None;
    for &stmt in statements {
        let kind = get_using_kind(arena, stmt);
        if kind == UsingKind::Async {
            return UsingKind::Async;
        }
        if kind > result {
            result = kind;
        }
    }
    result
}

#[cfg(test)]
#[path = "using_test.rs"]
mod tests;
