//! Port of Go `internal/transformers/estransforms/namedevaluation.go`: assigns a
//! runtime `.name` to an anonymous function/class bound to a name, via the
//! `__setFunctionName` helper.
//!
//! # Scope (round 6d-2)
//!
//! This is the validation tracer for the 6d-2 emit-helper infrastructure: a
//! `var f = function () {}` (anonymous function definition bound to an
//! identifier) is rewritten to `var f = __setFunctionName(function () {}, "f")`,
//! and the `__setFunctionName` helper is requested so its definition is emitted
//! in the module prologue.
//!
//! Deferred (DEFER(P5)): the full `isNamedEvaluation` surface (property/shorthand
//! assignments, parameters, binding elements, property declarations, export
//! assignments, computed-name `__propKey` caching, anonymous class `static {
//! __setFunctionName(this, …) }` blocks) and the `useDefineForClassFields`/
//! `--target` gating — these need the `EmitContext` assigned-name tracking and
//! the broader class-fields/decorator integration, not yet ported.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeData, NodeFlags, NodeId, NodeList, TokenFlags, VisitOptions};
use tsgo_printer::emithelpers::SET_FUNCTION_NAME_HELPER;
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that applies named evaluation, sharing the
/// pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::namedevaluation::new_named_evaluation_transformer, TransformOptions};
/// let _tx = new_named_evaluation_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/namedevaluation.go:newNamedEvaluationTransformer
pub fn new_named_evaluation_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| named_evaluation_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit: rewrites named-evaluation variable declarations
/// and, at the source-file boundary, attaches requested helpers.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
// Go: internal/transformers/estransforms/namedevaluation.go:namedEvaluationTransformer.visit
fn named_evaluation_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::VariableStatement => {
            let (modifiers, declaration_list) = match ec.arena().data(node) {
                NodeData::VariableStatement(d) => (d.modifiers.clone(), d.declaration_list),
                _ => unreachable!("kind/data mismatch"),
            };
            let declaration_list = named_evaluation_visit(ec, declaration_list);
            ec.arena_mut()
                .new_variable_statement(modifiers, declaration_list)
        }
        Kind::VariableDeclarationList => {
            let declarations = match ec.arena().data(node) {
                NodeData::VariableDeclarationList(d) => d.declarations.clone(),
                _ => unreachable!("kind/data mismatch"),
            };
            let visited: Vec<NodeId> = declarations
                .nodes
                .iter()
                .copied()
                .map(|d| named_evaluation_visit(ec, d))
                .collect();
            ec.arena_mut()
                .new_variable_declaration_list(NodeList::new(visited))
        }
        Kind::VariableDeclaration => visit_variable_declaration(ec, node),
        _ => {
            // Deeper nodes are not on the tracer's named-evaluation path; recurse
            // structurally (arena-only) to preserve them.
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut().visit_each_child(node, opts, &mut |_, c| c)
        }
    }
}

/// Manages the source file's requested helpers: visits the statements, then
/// reads and attaches the helpers requested during the visit to the rebuilt
/// source file so the printer emits them in the prologue.
///
/// Side effects: rebuilds the source file; attaches emit helpers.
// Go: internal/transformers/estransforms/namedevaluation.go (AddEmitHelper(ReadEmitHelpers()))
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
        .map(|s| named_evaluation_visit(ec, s))
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

/// Rewrites `var <id> = <anonymous fn>` to `var <id> = __setFunctionName(<fn>,
/// "<id>")`, requesting the `__setFunctionName` helper. Leaves other shapes
/// untouched.
///
/// Side effects: may push rebuilt nodes; may request the helper.
// Go: internal/transformers/estransforms/namedevaluation.go:transformNamedEvaluationOfVariableDeclaration
fn visit_variable_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (name, initializer) = match ec.arena().data(node) {
        NodeData::VariableDeclaration(d) => (d.name, d.initializer),
        _ => unreachable!("kind/data mismatch"),
    };
    let Some(initializer) = initializer else {
        return node;
    };
    if ec.arena().kind(name) != Kind::Identifier
        || !is_anonymous_function_definition(ec.arena(), initializer)
    {
        return node;
    }
    let assigned_name = ec.arena().text(name).to_string();
    ec.request_emit_helper(&SET_FUNCTION_NAME_HELPER);
    let callee = ec.factory().new_unscoped_helper_name("__setFunctionName");
    let name_literal = ec
        .factory()
        .new_string_literal(&assigned_name, TokenFlags::NONE);
    let call = ec.arena_mut().new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![initializer, name_literal]),
        NodeFlags::NONE,
    );
    // Matches Go `UpdateVariableDeclaration(node, name, nil, nil, initializer)`:
    // the exclamation token and type annotation are dropped.
    ec.arena_mut()
        .new_variable_declaration(name, None, None, Some(call))
}

/// Reports whether `node` is an anonymous function definition (an unnamed
/// function expression or an arrow function). Anonymous class expressions are
/// deferred (they lower via a `static {}` helper block).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/estransforms/namedevaluation.go:isAnonymousFunctionDefinition
fn is_anonymous_function_definition(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::FunctionExpression => {
            matches!(arena.data(node), NodeData::FunctionExpression(d) if d.name.is_none())
        }
        Kind::ArrowFunction => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "namedevaluation_test.rs"]
mod tests;
