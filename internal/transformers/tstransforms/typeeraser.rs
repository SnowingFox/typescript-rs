//! Port of Go `internal/transformers/tstransforms/typeeraser.go`: the
//! `TypeEraserTransformer`, which strips TypeScript-only syntax so the result
//! prints as plain JavaScript.
//!
//! # Scope
//!
//! [`type_eraser_visit`] is removal-aware: it returns `Option<NodeId>` where
//! `None` means "elide this node from its containing list" (Go's `visit`
//! returning `nil`). List rebuilds use
//! [`NodeArena::visit_nodes_removable`](tsgo_ast::NodeArena::visit_nodes_removable)
//! to drop the `None`s; the default arm recurses unchanged via
//! [`NodeArena::visit_each_child`](tsgo_ast::NodeArena::visit_each_child).
//!
//! Covered: type-annotation / type-parameter / type-argument / return-type
//! stripping for all function-like nodes (FunctionDeclaration,
//! FunctionExpression, ArrowFunction, MethodDeclaration, Constructor,
//! GetAccessor, SetAccessor); statement elision of type-only declarations and
//! ambient (`declare`) statements via [`Kind::NotEmittedStatement`]; type-only
//! modifier, `implements`-clause, index-signature, `this`-parameter, and
//! overload elision; per-specifier `import { type … }` elision; and
//! `as`/`satisfies`/`<T>x`/`x!` lowering to
//! [`Kind::PartiallyEmittedExpression`].
//!
//! Deferred (see `tstransforms/mod.rs`): export specifier elision needing the
//! checker `EmitResolver`, and the `compilerOptions`-driven branches
//! (`verbatimModuleSyntax`, `experimentalDecorators`, const-enum preservation).

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::utilities::modifier_to_flag;
use tsgo_ast::{
    Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeId, NodeList, VisitOptions,
};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that erases TypeScript-only syntax from a source
/// file, sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{tstransforms::typeeraser::new_type_eraser_transformer, TransformOptions};
/// // Constructed with a fresh context (no shared pipeline context).
/// let _tx = new_type_eraser_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/tstransforms/typeeraser.go:NewTypeEraserTransformer
pub fn new_type_eraser_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        // The root SourceFile is never elided, so `None` cannot occur at the top.
        Box::new(|ec: &mut EmitContext, node: NodeId| {
            type_eraser_visit(ec.arena_mut(), node).unwrap_or(node)
        }),
        opt.context.clone(),
    )
}

/// Erases TypeScript-only syntax from the subtree rooted at `node`. Returns
/// `Some(rebuilt)` for kept/rewritten nodes and `None` to elide the node from
/// its containing list (Go's `visit` returning `nil`).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/tstransforms/typeeraser.go:TypeEraserTransformer.visit
fn type_eraser_visit(arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
    // Ambient (`declare`) statements are elided to a non-emitted placeholder.
    if is_ambient_statement(arena, node) {
        return Some(arena.new_not_emitted_statement());
    }
    match arena.kind(node) {
        // TypeScript accessibility / `readonly` / `declare` / `const` modifiers
        // are erased (dropped from their modifier list).
        Kind::PublicKeyword
        | Kind::PrivateKeyword
        | Kind::ProtectedKeyword
        | Kind::AbstractKeyword
        | Kind::OverrideKeyword
        | Kind::ConstKeyword
        | Kind::DeclareKeyword
        | Kind::ReadonlyKeyword => None,
        // TypeScript type-only declarations are elided.
        Kind::InterfaceDeclaration | Kind::TypeAliasDeclaration => {
            Some(arena.new_not_emitted_statement())
        }
        // `export as namespace N;` (UMD global) has no runtime form.
        Kind::NamespaceExportDeclaration => Some(arena.new_not_emitted_statement()),
        // `import type x = require(...)` is type-only and elided.
        Kind::ImportEqualsDeclaration => {
            let is_type_only = match arena.data(node) {
                NodeData::ImportEqualsDeclaration(d) => d.is_type_only,
                _ => unreachable!("kind/data mismatch"),
            };
            if is_type_only {
                return Some(arena.new_not_emitted_statement());
            }
            Some(visit_each_child_erase(arena, node))
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindImportDeclaration
        Kind::ImportDeclaration => {
            let (import_clause, module_specifier, attributes) = match arena.data(node) {
                NodeData::ImportDeclaration(d) => {
                    (d.import_clause, d.module_specifier, d.attributes)
                }
                _ => unreachable!("kind/data mismatch"),
            };
            // Side-effect-only import: `import "foo";` — always keep.
            let Some(clause) = import_clause else {
                return Some(node);
            };
            // Visit the clause; if it was fully elided, drop the import.
            let visited_clause = type_eraser_visit(arena, clause);
            match visited_clause {
                None => Some(arena.new_not_emitted_statement()),
                Some(c) => {
                    let attributes = attributes.map(|a| visit_required(arena, a));
                    Some(arena.new_import_declaration(None, Some(c), module_specifier, attributes))
                }
            }
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindImportClause
        Kind::ImportClause => {
            let (phase_modifier, name, named_bindings) = match arena.data(node) {
                NodeData::ImportClause(d) => (d.phase_modifier, d.name, d.named_bindings),
                _ => unreachable!("kind/data mismatch"),
            };
            // `import type ...` is always fully elided.
            if phase_modifier == Kind::TypeKeyword {
                return None;
            }
            let named_bindings = named_bindings.and_then(|nb| type_eraser_visit(arena, nb));
            if name.is_none() && named_bindings.is_none() {
                return None;
            }
            Some(arena.new_import_clause(phase_modifier, name, named_bindings))
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindNamedImports
        Kind::NamedImports => {
            let elements = match arena.data(node) {
                NodeData::NamedImports(d) => d.elements.clone(),
                _ => unreachable!("kind/data mismatch"),
            };
            if elements.nodes.is_empty() {
                return Some(node);
            }
            let visited = visit_nodes(arena, &elements);
            if visited.nodes.is_empty() {
                return None;
            }
            Some(arena.new_named_imports(visited))
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindImportSpecifier
        Kind::ImportSpecifier => {
            let is_type_only = match arena.data(node) {
                NodeData::ImportSpecifier(d) => d.is_type_only,
                _ => unreachable!("kind/data mismatch"),
            };
            if is_type_only {
                return None;
            }
            Some(node)
        }
        Kind::VariableDeclaration => {
            // Drop the `!` definite-assignment token and the type annotation.
            let (name, initializer) = match arena.data(node) {
                NodeData::VariableDeclaration(d) => (d.name, d.initializer),
                _ => unreachable!("kind/data mismatch"),
            };
            let name = visit_required(arena, name);
            let initializer = initializer.map(|i| visit_required(arena, i));
            Some(arena.new_variable_declaration(name, None, None, initializer))
        }
        Kind::CallExpression => {
            // Drop the type arguments.
            let (expression, question_dot_token, arguments) = match arena.data(node) {
                NodeData::CallExpression(d) => {
                    (d.expression, d.question_dot_token, d.arguments.clone())
                }
                _ => unreachable!("kind/data mismatch"),
            };
            let flags = arena.flags(node);
            let expression = visit_required(arena, expression);
            let arguments = visit_nodes(arena, &arguments);
            Some(arena.new_call_expression(expression, question_dot_token, None, arguments, flags))
        }
        Kind::NewExpression => {
            // Drop the type arguments.
            let (expression, arguments) = match arena.data(node) {
                NodeData::NewExpression(d) => (d.expression, d.arguments.clone()),
                _ => unreachable!("kind/data mismatch"),
            };
            let expression = visit_required(arena, expression);
            let arguments = visit_opt_nodes(arena, arguments.as_ref());
            Some(arena.new_new_expression(expression, None, arguments))
        }
        Kind::Parameter => {
            // `this` parameters are TypeScript-only and removed entirely.
            if is_this_parameter(arena, node) {
                return None;
            }
            // Drop the `?` optional token and the type; keep param-property
            // modifiers (handled later by the runtime transform), `...`, name,
            // and initializer.
            let (modifiers, dot_dot_dot_token, name, initializer) = match arena.data(node) {
                NodeData::ParameterDeclaration(d) => (
                    d.modifiers.clone(),
                    d.dot_dot_dot_token,
                    d.name,
                    d.initializer,
                ),
                _ => unreachable!("kind/data mismatch"),
            };
            let name = visit_required(arena, name);
            let initializer = initializer.map(|i| visit_required(arena, i));
            Some(arena.new_parameter_declaration(
                modifiers,
                dot_dot_dot_token,
                name,
                None,
                None,
                initializer,
            ))
        }
        Kind::HeritageClause => {
            // `implements` clauses are TypeScript-only and elided; `extends`
            // clauses are kept with their types visited.
            let (token, types) = match arena.data(node) {
                NodeData::HeritageClause(d) => (d.token, d.types.clone()),
                _ => unreachable!("kind/data mismatch"),
            };
            if token == Kind::ImplementsKeyword {
                return None;
            }
            let types = visit_nodes(arena, &types);
            Some(arena.new_heritage_clause(token, types))
        }
        Kind::PropertyDeclaration => {
            let (modifiers, name, initializer) = match arena.data(node) {
                NodeData::PropertyDeclaration(d) => (d.modifiers.clone(), d.name, d.initializer),
                _ => unreachable!("kind/data mismatch"),
            };
            // `declare`/`abstract` fields have no runtime form and are removed.
            let flags = modifiers
                .as_ref()
                .map_or(ModifierFlags::empty(), |m| m.modifier_flags);
            if flags.intersects(ModifierFlags::AMBIENT | ModifierFlags::ABSTRACT) {
                return None;
            }
            let modifiers = visit_modifiers(arena, modifiers.as_ref());
            let name = visit_required(arena, name);
            let initializer = initializer.map(|i| visit_required(arena, i));
            // Drop the postfix `?`/`!` token and the type annotation.
            Some(arena.new_property_declaration(modifiers, name, None, None, initializer))
        }
        // Type assertions and non-null assertions lower to their inner
        // expression, wrapped in a `PartiallyEmittedExpression` so the printer
        // keeps the inner expression's position/comments.
        Kind::NonNullExpression
        | Kind::TypeAssertionExpression
        | Kind::AsExpression
        | Kind::SatisfiesExpression => {
            let inner = match arena.data(node) {
                NodeData::NonNullExpression(d) => d.expression,
                NodeData::AsExpression(d) | NodeData::SatisfiesExpression(d) => d.expression,
                NodeData::TypeAssertionExpression(d) => d.expression,
                _ => unreachable!("kind/data mismatch"),
            };
            let inner = visit_required(arena, inner);
            Some(arena.new_partially_emitted_expression(inner))
        }
        Kind::ExpressionWithTypeArguments => {
            // Drop the type arguments (`f<T>` -> `f`).
            let expression = match arena.data(node) {
                NodeData::ExpressionWithTypeArguments(d) => d.expression,
                _ => unreachable!("kind/data mismatch"),
            };
            let expression = visit_required(arena, expression);
            Some(arena.new_expression_with_type_arguments(expression, None))
        }
        Kind::ClassDeclaration | Kind::ClassExpression => {
            // Drop type parameters; keep modifiers, name, heritage, members.
            let kind = arena.kind(node);
            let (modifiers, name, heritage_clauses, members) = match arena.data(node) {
                NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => (
                    d.modifiers.clone(),
                    d.name,
                    d.heritage_clauses.clone(),
                    d.members.clone(),
                ),
                _ => unreachable!("kind/data mismatch"),
            };
            let modifiers = visit_modifiers(arena, modifiers.as_ref());
            let name = name.map(|n| visit_required(arena, n));
            let heritage_clauses = visit_opt_nodes(arena, heritage_clauses.as_ref());
            let members = visit_nodes(arena, &members);
            Some(arena.new_class_like(kind, modifiers, name, None, heritage_clauses, members))
        }
        Kind::FunctionDeclaration => {
            // Drop type parameters, the return type, and the (TypeScript-only)
            // full signature; keep modifiers, name, parameters, and body.
            let (modifiers, asterisk_token, name, parameters, body) = match arena.data(node) {
                NodeData::FunctionDeclaration(d) => (
                    d.modifiers.clone(),
                    d.asterisk_token,
                    d.name,
                    d.parameters.clone(),
                    d.body,
                ),
                _ => unreachable!("kind/data mismatch"),
            };
            // A bodyless function declaration is a TypeScript overload signature.
            if body.is_none() {
                return Some(arena.new_not_emitted_statement());
            }
            let modifiers = visit_modifiers(arena, modifiers.as_ref());
            let name = name.map(|n| visit_required(arena, n));
            let parameters = visit_nodes(arena, &parameters);
            let body = body.map(|b| visit_required(arena, b));
            Some(arena.new_function_declaration(
                modifiers,
                asterisk_token,
                name,
                None,
                parameters,
                None,
                None,
                body,
            ))
        }
        Kind::FunctionExpression => {
            // Drop type parameters, the return type, and the full signature.
            let (modifiers, asterisk_token, name, parameters, body) = match arena.data(node) {
                NodeData::FunctionExpression(d) => (
                    d.modifiers.clone(),
                    d.asterisk_token,
                    d.name,
                    d.parameters.clone(),
                    d.body,
                ),
                _ => unreachable!("kind/data mismatch"),
            };
            let modifiers = visit_modifiers(arena, modifiers.as_ref());
            let name = name.map(|n| visit_required(arena, n));
            let parameters = visit_nodes(arena, &parameters);
            let body = body.map(|b| visit_required(arena, b));
            Some(arena.new_function_expression(
                modifiers,
                asterisk_token,
                name,
                None,
                parameters,
                None,
                None,
                body,
            ))
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindMethodDeclaration
        Kind::MethodDeclaration => {
            // A bodyless method is a TypeScript overload signature; elide it.
            let (modifiers, asterisk_token, name, parameters, body) = match arena.data(node) {
                NodeData::MethodDeclaration(d) => (
                    d.modifiers.clone(),
                    d.asterisk_token,
                    d.name,
                    d.parameters.clone(),
                    d.body,
                ),
                _ => unreachable!("kind/data mismatch"),
            };
            body?;
            let modifiers = visit_modifiers(arena, modifiers.as_ref());
            let name = visit_required(arena, name);
            let parameters = visit_nodes(arena, &parameters);
            let body = body.map(|b| visit_required(arena, b));
            Some(arena.new_method_declaration(
                modifiers,
                asterisk_token,
                name,
                None,
                None,
                parameters,
                None,
                None,
                body,
            ))
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindConstructor
        Kind::Constructor => {
            // A bodyless constructor is a TypeScript overload signature; elide it.
            let (parameters, body) = match arena.data(node) {
                NodeData::ConstructorDeclaration(d) => (d.parameters.clone(), d.body),
                _ => unreachable!("kind/data mismatch"),
            };
            body?;
            let parameters = visit_nodes(arena, &parameters);
            let body = body.map(|b| visit_required(arena, b));
            Some(arena.new_constructor_declaration(None, None, parameters, None, None, body))
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindGetAccessor,KindSetAccessor
        Kind::GetAccessor | Kind::SetAccessor => {
            let kind = arena.kind(node);
            let (modifiers, name, parameters, body) = match arena.data(node) {
                NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                    (d.modifiers.clone(), d.name, d.parameters.clone(), d.body)
                }
                _ => unreachable!("kind/data mismatch"),
            };
            let modifiers = visit_modifiers(arena, modifiers.as_ref());
            let name = visit_required(arena, name);
            let parameters = visit_nodes(arena, &parameters);
            let body = body.map(|b| visit_required(arena, b));
            // Go provides an empty block when the body is missing (non-abstract).
            let body = body.or_else(|| Some(arena.new_block(NodeList::new(vec![]))));
            Some(arena.new_accessor_declaration(
                kind, modifiers, name, None, parameters, None, None, body,
            ))
        }
        // Go: internal/transformers/tstransforms/typeeraser.go:visit/KindArrowFunction
        Kind::ArrowFunction => {
            // Drop type parameters, return type, and full signature.
            let (modifiers, parameters, equals_greater_than_token, body) = match arena.data(node) {
                NodeData::ArrowFunction(d) => (
                    d.modifiers.clone(),
                    d.parameters.clone(),
                    d.equals_greater_than_token,
                    d.body,
                ),
                _ => unreachable!("kind/data mismatch"),
            };
            let modifiers = visit_modifiers(arena, modifiers.as_ref());
            let parameters = visit_nodes(arena, &parameters);
            let body = visit_required(arena, body);
            Some(arena.new_arrow_function(
                modifiers,
                None,
                parameters,
                None,
                None,
                equals_greater_than_token,
                body,
            ))
        }
        _ => Some(visit_each_child_erase(arena, node)),
    }
}

/// Reports whether parameter `node` is the synthetic `this` parameter
/// (`function f(this: T)`), which is TypeScript-only.
// Go: internal/ast/utilities.go:IsThisParameter
fn is_this_parameter(arena: &NodeArena, node: NodeId) -> bool {
    let name = match arena.data(node) {
        NodeData::ParameterDeclaration(d) => d.name,
        _ => return false,
    };
    arena.kind(name) == Kind::Identifier && arena.text(name) == "this"
}

/// Reports whether `node` is a statement carrying the `declare` (ambient)
/// modifier, which the eraser elides wholesale.
// Go: internal/ast/utilities.go:HasSyntacticModifier(node, ModifierFlagsAmbient)
fn is_ambient_statement(arena: &NodeArena, node: NodeId) -> bool {
    let modifiers = match arena.data(node) {
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers.is_some_and(|m| m.modifier_flags.contains(ModifierFlags::AMBIENT))
}

/// Visits `node` in a position where the result must stay (not be elided),
/// keeping the original if a visit somehow returns `None`.
///
/// Side effects: may push rebuilt nodes.
fn visit_required(arena: &mut NodeArena, node: NodeId) -> NodeId {
    type_eraser_visit(arena, node).unwrap_or(node)
}

/// Visits every node in `list`, dropping elided (`None`) results.
///
/// Side effects: may push rebuilt nodes.
fn visit_nodes(arena: &mut NodeArena, list: &NodeList) -> NodeList {
    arena.visit_nodes_removable(list, &mut |a, c| type_eraser_visit(a, c))
}

/// Like [`visit_nodes`] for an optional list.
///
/// Side effects: may push rebuilt nodes.
fn visit_opt_nodes(arena: &mut NodeArena, list: Option<&NodeList>) -> Option<NodeList> {
    list.map(|l| visit_nodes(arena, l))
}

/// Visits a modifier list, dropping erased (TypeScript-only) modifiers and
/// recomputing the flag union; returns `None` when nothing remains.
///
/// Side effects: may push rebuilt nodes.
fn visit_modifiers(
    arena: &mut NodeArena,
    modifiers: Option<&ModifierList>,
) -> Option<ModifierList> {
    let modifiers = modifiers?;
    let list = arena.visit_nodes_removable(&modifiers.list, &mut |a, c| type_eraser_visit(a, c));
    if list.nodes.is_empty() {
        return None;
    }
    let modifier_flags = list.nodes.iter().fold(ModifierFlags::empty(), |acc, &n| {
        acc | modifier_to_flag(arena.kind(n))
    });
    Some(ModifierList {
        list,
        modifier_flags,
    })
}

/// Recurses through children with the eraser, dropping elided list elements.
///
/// Side effects: may push rebuilt nodes.
fn visit_each_child_erase(arena: &mut NodeArena, node: NodeId) -> NodeId {
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| type_eraser_visit(a, c).unwrap_or(c))
}

#[cfg(test)]
#[path = "typeeraser_test.rs"]
mod tests;
