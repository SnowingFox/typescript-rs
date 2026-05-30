//! Port of Go `internal/transformers/estransforms/exponentiation.go`: lowers the
//! ES2016 exponentiation operator (`**`, `**=`) to `Math.pow` calls.
//!
//! # Scope (rounds 6c-1 + 6c-3)
//!
//! Lowers `a ** b` → `Math.pow(a, b)` and `a **= b` → `a = Math.pow(a, b)`.
//! For a property-access assignment target (`a.x **= b`) a temp is hoisted for
//! the receiver, yielding `(_a = a).x = Math.pow(_a.x, b)` with a hoisted
//! `var _a;` at the top of the (top-level) scope, using the
//! [`EmitContext`](tsgo_printer::EmitContext) variable environment.
//!
//! The top-level statement path (`SourceFile` → `ExpressionStatement` →
//! `BinaryExpression`) threads the emit context so temps can be allocated and
//! hoisted; descent into other nodes is arena-only (handling `**` and
//! identifier `**=` everywhere, deferring temp-hoisting `**=` targets nested in
//! non-top-level scopes — see `estransforms/mod.rs`).

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, VisitOptions};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers `**`/`**=` to `Math.pow`, sharing the
/// pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::exponentiation::new_exponentiation_transformer, TransformOptions};
/// let _tx = new_exponentiation_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/exponentiation.go:newExponentiationTransformer
pub fn new_exponentiation_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| exponentiation_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded lowering for the top-level statement path so temps for
/// `a.x **= b` can be hoisted to the enclosing scope.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/exponentiation.go:exponentiationTransformer.visit
fn exponentiation_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::ExpressionStatement => {
            let expression = match ec.arena().data(node) {
                NodeData::ExpressionStatement(d) => d.expression,
                _ => unreachable!("kind/data mismatch"),
            };
            let expression = exponentiation_visit(ec, expression);
            ec.arena_mut().new_expression_statement(expression)
        }
        Kind::BinaryExpression => visit_binary_expression(ec, node),
        _ => {
            // Descent into non-top-level nodes is arena-only: `**` and identifier
            // `**=` are still lowered; temp-hoisting `**=` targets nested here are
            // deferred.
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut()
                .visit_each_child(node, opts, &mut |a, c| exponentiation_visit_arena(a, c))
        }
    }
}

/// Wraps the source file's statements in a variable environment so hoisted
/// temporaries are emitted as a leading `var ...;` statement.
///
/// Side effects: pushes/pops a variable environment; rebuilds the source file.
// Go: internal/printer/emitcontext.go:EmitContext.VisitVariableEnvironment (top-level statements)
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
    ec.start_variable_environment();
    let mut visited = Vec::with_capacity(statements.nodes.len());
    for &statement in &statements.nodes {
        visited.push(exponentiation_visit(ec, statement));
    }
    let mut all = ec.end_variable_environment();
    all.extend(visited);
    ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(all),
        end_of_file_token,
    )
}

/// Lowers a `**`/`**=` binary expression (emit-context path).
///
/// Side effects: may push rebuilt nodes; may hoist a temp for `a.x **= b`.
// Go: internal/transformers/estransforms/exponentiation.go:visitBinaryExpression
fn visit_binary_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (left, operator_token, right) = match ec.arena().data(node) {
        NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
        _ => unreachable!("kind/data mismatch"),
    };
    match ec.arena().kind(operator_token) {
        Kind::AsteriskAsteriskToken => {
            let left = exponentiation_visit(ec, left);
            let right = exponentiation_visit(ec, right);
            math_pow(ec.arena_mut(), left, right)
        }
        Kind::AsteriskAsteriskEqualsToken if ec.arena().kind(left) == Kind::Identifier => {
            // `a **= b` -> `a = Math.pow(a, b)`
            let left = exponentiation_visit(ec, left);
            let right = exponentiation_visit(ec, right);
            let pow = math_pow(ec.arena_mut(), left, right);
            let equals = ec.arena_mut().new_token(Kind::EqualsToken);
            ec.arena_mut().new_binary_expression(left, equals, pow)
        }
        Kind::AsteriskAsteriskEqualsToken
            if ec.arena().kind(left) == Kind::PropertyAccessExpression =>
        {
            // `a.x **= b` -> `(_a = a).x = Math.pow(_a.x, b)`
            let (object, name) = match ec.arena().data(left) {
                NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
                _ => unreachable!("kind/data mismatch"),
            };
            let object = exponentiation_visit(ec, object);
            let right = exponentiation_visit(ec, right);
            let temp = ec.factory().new_temp_variable();
            ec.add_variable_declaration(temp);
            // `(_a = a)`
            let equals1 = ec.arena_mut().new_token(Kind::EqualsToken);
            let object_assignment = ec.arena_mut().new_binary_expression(temp, equals1, object);
            // `(_a = a).x`
            let target =
                ec.arena_mut()
                    .new_property_access_expression(object_assignment, None, name);
            // `_a.x`
            let value = ec
                .arena_mut()
                .new_property_access_expression(temp, None, name);
            let pow = math_pow(ec.arena_mut(), value, right);
            let equals2 = ec.arena_mut().new_token(Kind::EqualsToken);
            ec.arena_mut().new_binary_expression(target, equals2, pow)
        }
        Kind::AsteriskAsteriskEqualsToken
            if ec.arena().kind(left) == Kind::ElementAccessExpression =>
        {
            // `a[x] **= b` -> `(_a = a)[_b = x] = Math.pow(_a[_b], b)`
            let (object, argument) = match ec.arena().data(left) {
                NodeData::ElementAccessExpression(d) => (d.expression, d.argument_expression),
                _ => unreachable!("kind/data mismatch"),
            };
            let object = exponentiation_visit(ec, object);
            let argument = exponentiation_visit(ec, argument);
            let right = exponentiation_visit(ec, right);
            let object_temp = ec.factory().new_temp_variable();
            ec.add_variable_declaration(object_temp);
            let argument_temp = ec.factory().new_temp_variable();
            ec.add_variable_declaration(argument_temp);
            // `(_a = a)` and `(_b = x)`
            let equals_object = ec.arena_mut().new_token(Kind::EqualsToken);
            let object_assignment =
                ec.arena_mut()
                    .new_binary_expression(object_temp, equals_object, object);
            let equals_argument = ec.arena_mut().new_token(Kind::EqualsToken);
            let argument_assignment =
                ec.arena_mut()
                    .new_binary_expression(argument_temp, equals_argument, argument);
            // `(_a = a)[_b = x]`
            let target = ec.arena_mut().new_element_access_expression(
                object_assignment,
                None,
                argument_assignment,
            );
            // `_a[_b]`
            let value =
                ec.arena_mut()
                    .new_element_access_expression(object_temp, None, argument_temp);
            let pow = math_pow(ec.arena_mut(), value, right);
            let equals_result = ec.arena_mut().new_token(Kind::EqualsToken);
            ec.arena_mut()
                .new_binary_expression(target, equals_result, pow)
        }
        _ => {
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut()
                .visit_each_child(node, opts, &mut |a, c| exponentiation_visit_arena(a, c))
        }
    }
}

/// Arena-only lowering used for descent into non-top-level nodes: handles
/// `a ** b` and identifier `a **= b`; other nodes recurse unchanged.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/exponentiation.go:exponentiationTransformer.visit
fn exponentiation_visit_arena(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if arena.kind(node) == Kind::BinaryExpression {
        let (left, operator_token, right) = match arena.data(node) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            _ => unreachable!("kind/data mismatch"),
        };
        match arena.kind(operator_token) {
            Kind::AsteriskAsteriskToken => {
                let left = exponentiation_visit_arena(arena, left);
                let right = exponentiation_visit_arena(arena, right);
                return math_pow(arena, left, right);
            }
            Kind::AsteriskAsteriskEqualsToken if arena.kind(left) == Kind::Identifier => {
                let left = exponentiation_visit_arena(arena, left);
                let right = exponentiation_visit_arena(arena, right);
                let pow = math_pow(arena, left, right);
                let equals = arena.new_token(Kind::EqualsToken);
                return arena.new_binary_expression(left, equals, pow);
            }
            _ => {}
        }
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| exponentiation_visit_arena(a, c))
}

/// Builds a `Math.pow(left, right)` call expression.
///
/// Side effects: pushes the `Math`/`pow`/access/call nodes onto the arena.
// Go: internal/printer/factory.go:NodeFactory.NewGlobalMethodCall("Math", "pow", ...)
fn math_pow(arena: &mut NodeArena, left: NodeId, right: NodeId) -> NodeId {
    let math = arena.new_identifier("Math");
    let pow = arena.new_identifier("pow");
    let callee = arena.new_property_access_expression(math, None, pow);
    arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![left, right]),
        NodeFlags::NONE,
    )
}

#[cfg(test)]
#[path = "exponentiation_test.rs"]
mod tests;
