//! Port of Go `internal/transformers/tstransforms/metadata.go`: the decorator
//! metadata transformer that injects `design:type` / `design:paramtypes` /
//! `design:returntype` metadata decorators.
//!
//! # Scope
//!
//! This round lands the **structural dispatch** of the metadata transformer:
//! the visitor that walks decorated class declarations, class expressions,
//! property declarations, methods, and accessors; plus the `should_add_*`
//! predicates that decide which metadata kinds to emit.
//!
//! The full metadata injection (calling the type serializer to produce the
//! runtime-constructor expressions, building `__metadata(…)` calls, and
//! injecting them into the modifier list) is partially structural; the
//! `serialize_type_node` function is ported in `typeserializer.rs`.
//!
//! # Deferred (DEFER(P5))
//!
//! * Full integration with the checker's emit resolver for type reference
//!   resolution.
//! * The `injectClassTypeMetadata` / `injectClassElementTypeMetadata` modifier
//!   list rebuilding (needs the factory's `NewMetadataHelper`, `NewDecorator`,
//!   and the `EmitHelper` plumbing).
//! * The `USE_NEW_TYPE_METADATA_FORMAT` code path.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, VisitOptions};
use tsgo_printer::EmitContext;

/// Whether to use the new type metadata format.
///
/// This is a compile-time constant in Go (`const USE_NEW_TYPE_METADATA_FORMAT = false`).
#[allow(dead_code)]
pub const USE_NEW_TYPE_METADATA_FORMAT: bool = false;

/// Builds a [`Transformer`] that injects decorator type metadata
/// (`design:type`, `design:paramtypes`, `design:returntype`).
///
/// Only active when `experimentalDecorators` is enabled.
///
/// # Examples
/// ```
/// use tsgo_transformers::tstransforms::metadata::new_metadata_transformer;
/// use tsgo_transformers::TransformOptions;
/// let tx = new_metadata_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/tstransforms/metadata.go:NewMetadataTransformer
pub fn new_metadata_transformer(opt: &TransformOptions) -> Transformer {
    let legacy_decorators = opt.compiler_options.experimental_decorators.is_true();
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            metadata_visit(ec, node, legacy_decorators)
        }),
        opt.context.clone(),
    )
}

/// The main visitor: dispatches decorated class/member nodes.
///
/// The Go version short-circuits on `node.SubtreeFacts() & SubtreeContainsDecorators`;
/// the Rust parser does not yet expose subtree facts on `NodeArena`, so the
/// visitor walks all nodes (correctness preserved; just less early-exit).
///
/// Side effects: may push rebuilt nodes.
// Go: internal/transformers/tstransforms/metadata.go:MetadataTransformer.visit
fn metadata_visit(ec: &mut EmitContext, node: NodeId, legacy_decorators: bool) -> NodeId {
    match ec.arena().kind(node) {
        Kind::ClassDeclaration
        | Kind::ClassExpression
        | Kind::PropertyDeclaration
        | Kind::MethodDeclaration
        | Kind::SetAccessor
        | Kind::GetAccessor
        | Kind::SourceFile
        | Kind::ModuleBlock
        | Kind::Block
        | Kind::CaseBlock => {
            // Full metadata injection is DEFER(P5); structural pass-through.
            visit_each_child_metadata(ec, node, legacy_decorators)
        }
        _ => visit_each_child_metadata(ec, node, legacy_decorators),
    }
}

/// Arena-only visit-each-child for the metadata pass.
fn visit_each_child_metadata(
    ec: &mut EmitContext,
    node: NodeId,
    legacy_decorators: bool,
) -> NodeId {
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    ec.arena_mut()
        .visit_each_child(node, opts, &mut |arena, child| {
            metadata_visit_arena(arena, child, legacy_decorators)
        })
}

/// Arena-only metadata visitor (without `EmitContext` access).
fn metadata_visit_arena(arena: &mut NodeArena, node: NodeId, _legacy_decorators: bool) -> NodeId {
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| {
        metadata_visit_arena(a, c, _legacy_decorators)
    })
}

/// Determines whether to emit `design:type` metadata for this node kind.
///
/// The caller should have already tested whether the node has decorators
/// and whether the `emitDecoratorMetadata` compiler option is set.
///
/// Side effects: none (pure).
// Go: internal/transformers/tstransforms/metadata.go:MetadataTransformer.shouldAddTypeMetadata
pub fn should_add_type_metadata(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor | Kind::PropertyDeclaration
    )
}

/// Determines whether to emit `design:returntype` metadata for this node kind.
///
/// Side effects: none (pure).
// Go: internal/transformers/tstransforms/metadata.go:MetadataTransformer.shouldAddReturnTypeMetadata
pub fn should_add_return_type_metadata(kind: Kind) -> bool {
    kind == Kind::MethodDeclaration
}

/// Determines whether to emit `design:paramtypes` metadata for this node kind.
///
/// Side effects: none (pure).
// Go: internal/transformers/tstransforms/metadata.go:MetadataTransformer.shouldAddParamTypesMetadata
pub fn should_add_param_types_metadata(arena: &NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::ClassDeclaration | Kind::ClassExpression => has_constructor_with_body(arena, node),
        Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor => true,
        _ => false,
    }
}

/// Checks whether a class-like node has a constructor with a body.
fn has_constructor_with_body(arena: &NodeArena, node: NodeId) -> bool {
    let members = match arena.data(node) {
        NodeData::ClassDeclaration(d) => &d.members.nodes,
        NodeData::ClassExpression(d) => &d.members.nodes,
        _ => return false,
    };
    members.iter().any(|&member| {
        if arena.kind(member) != Kind::Constructor {
            return false;
        }
        match arena.data(member) {
            NodeData::ConstructorDeclaration(d) => d.body.is_some(),
            _ => false,
        }
    })
}

#[cfg(test)]
#[path = "metadata_test.rs"]
mod tests;
