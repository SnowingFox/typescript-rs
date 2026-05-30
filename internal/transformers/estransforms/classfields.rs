//! Port of Go `internal/transformers/estransforms/classfields.go`: lowers class
//! field declarations to assignments.
//!
//! # Scope (rounds 6c-1 + 6c-2)
//!
//! Lowers **instance field initializers → constructor assignments** for class
//! declarations, covering the full constructor-insertion family: a synthesized
//! constructor when none exists (`class C { x = 1 }` →
//! `class C { constructor() { this.x = 1; } }`), insertion into an existing
//! constructor body, and — for `extends` (derived) classes — a synthesized
//! `super(...arguments)` forward or insertion immediately after an existing
//! `super(...)` call. Only plain identifier-named instance fields with
//! initializers are handled; any other shape returns `None`, leaving the class
//! for the deferred fuller port.
//!
//! Round 6c-3 also lowers **static fields**: `class C { static x = 1 }` becomes
//! the class declaration followed by `C.x = 1;` (the transform returns a
//! [`Kind::SyntaxList`](tsgo_ast::Kind::SyntaxList) of `[class, assignment...]`).
//!
//! Deferred (DEFER(P5), see `estransforms/mod.rs`): **private names** (`#x` →
//! WeakMap/WeakSet brand-check), **`accessor` fields**, **computed property
//! names**, **class expressions**, **parameter properties**, constructor
//! **prologue directives**, **anonymous-class static fields**, and
//! `--target`/`useDefineForClassFields` gating — these need a private-environment
//! map + private-access rewriting (private names), helper-library emit, and/or
//! checker info not yet ported.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{
    Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId, NodeList, VisitOptions,
};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers class fields, sharing the pipeline's
/// emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::classfields::new_class_fields_transformer, TransformOptions};
/// let _tx = new_class_fields_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/classfields.go:newClassFieldsTransformer
pub fn new_class_fields_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| class_fields_visit(ec.arena_mut(), node)),
        opt.context.clone(),
    )
}

/// Lowers class fields in the subtree rooted at `node`.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.visit
fn class_fields_visit(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if arena.kind(node) == Kind::ClassDeclaration {
        if let Some(lowered) = try_lower_simple_class(arena, node) {
            return lowered;
        }
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| class_fields_visit(a, c))
}

/// Lowers a class whose instance fields (plain identifier-named, with
/// initializers) are hoisted into a constructor as `this.x = init` statements.
/// Works for a synthesized constructor (no constructor present) or by inserting
/// into an existing constructor body. Returns `None` (leaving the class for the
/// deferred fuller port) for any shape outside this slice (static / private /
/// computed / accessor fields, fields without initializers, multiple
/// constructors).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.transformClassMembers
fn try_lower_simple_class(arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
    let (modifiers, name, type_parameters, heritage_clauses, members) = match arena.data(node) {
        NodeData::ClassDeclaration(d) => (
            d.modifiers.clone(),
            d.name,
            d.type_parameters.clone(),
            d.heritage_clauses.clone(),
            d.members.clone(),
        ),
        _ => return None,
    };

    let mut field_assignments = Vec::new();
    let mut static_assignments = Vec::new();
    let mut existing_constructor: Option<NodeId> = None;
    // `None` marks the position of the (existing) constructor; `Some(id)` is a
    // kept, already-visited member.
    let mut member_plan: Vec<Option<NodeId>> = Vec::new();
    for &member in &members.nodes {
        match arena.kind(member) {
            Kind::PropertyDeclaration => {
                let (field_modifiers, field_name, initializer) = match arena.data(member) {
                    NodeData::PropertyDeclaration(d) => {
                        (d.modifiers.clone(), d.name, d.initializer)
                    }
                    _ => return None,
                };
                // Private / computed names need different lowering -> deferred.
                if arena.kind(field_name) != Kind::Identifier {
                    return None;
                }
                // Fields without an initializer need `--useDefineForClassFields`
                // gating -> deferred.
                let initializer = initializer?;
                let initializer = class_fields_visit(arena, initializer);
                let is_static = field_modifiers
                    .as_ref()
                    .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::STATIC));
                if is_static {
                    // `static x = 1` -> `C.x = 1;` after the class declaration.
                    // An anonymous class would need a generated name -> deferred.
                    let class_name = name?;
                    static_assignments.push(make_static_assignment(
                        arena,
                        class_name,
                        field_name,
                        initializer,
                    ));
                } else {
                    field_assignments.push(make_this_assignment(arena, field_name, initializer));
                }
            }
            Kind::Constructor => {
                // Multiple constructors are not valid; bail defensively.
                if existing_constructor.is_some() {
                    return None;
                }
                existing_constructor = Some(member);
                member_plan.push(None);
            }
            _ => member_plan.push(Some(class_fields_visit(arena, member))),
        }
    }
    if field_assignments.is_empty() && static_assignments.is_empty() {
        return None;
    }

    // A constructor is only synthesized/updated when there are instance field
    // initializers to move or a constructor already exists.
    let new_members = if !field_assignments.is_empty() || existing_constructor.is_some() {
        let is_derived = heritage_has_extends(arena, heritage_clauses.as_ref());
        let body =
            build_constructor_body(arena, existing_constructor, is_derived, field_assignments);
        let parameters = match existing_constructor {
            Some(constructor) => match arena.data(constructor) {
                NodeData::ConstructorDeclaration(d) => d.parameters.clone(),
                _ => NodeList::new(vec![]),
            },
            None => NodeList::new(vec![]),
        };
        let constructor =
            arena.new_constructor_declaration(None, None, parameters, None, None, Some(body));
        let mut members = Vec::with_capacity(1 + member_plan.len());
        if existing_constructor.is_none() {
            members.push(constructor);
        }
        for plan in member_plan {
            members.push(plan.unwrap_or(constructor));
        }
        members
    } else {
        // Static fields only: no constructor; keep the remaining members.
        member_plan.into_iter().flatten().collect()
    };

    let class = arena.new_class_like(
        Kind::ClassDeclaration,
        modifiers,
        name,
        type_parameters,
        heritage_clauses,
        NodeList::new(new_members),
    );
    if static_assignments.is_empty() {
        Some(class)
    } else {
        // Emit `C.x = ...` assignments after the class via a SyntaxList.
        let mut statements = Vec::with_capacity(1 + static_assignments.len());
        statements.push(class);
        statements.extend(static_assignments);
        Some(arena.new_syntax_list(NodeList::new(statements)))
    }
}

/// Builds a `C.<name> = <initializer>;` assignment statement referencing the
/// class by its (local) name.
///
/// Side effects: pushes the reference/access/assignment/statement nodes.
fn make_static_assignment(
    arena: &mut NodeArena,
    class_name: NodeId,
    field_name: NodeId,
    initializer: NodeId,
) -> NodeId {
    let class_text = arena.text(class_name).to_string();
    let class_reference = arena.new_identifier(&class_text);
    let target = arena.new_property_access_expression(class_reference, None, field_name);
    let equals = arena.new_token(Kind::EqualsToken);
    let assignment = arena.new_binary_expression(target, equals, initializer);
    arena.new_expression_statement(assignment)
}

/// Builds the constructor body block: field initializer statements followed by
/// (for an existing constructor) the visited original body statements. When no
/// constructor exists, the body is just the field initializers.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.transformConstructorBody
fn build_constructor_body(
    arena: &mut NodeArena,
    existing_constructor: Option<NodeId>,
    is_derived: bool,
    field_assignments: Vec<NodeId>,
) -> NodeId {
    let mut statements = Vec::new();
    if let Some(constructor) = existing_constructor {
        let body_statements = constructor_body_statements(arena, constructor);
        if let Some(super_index) = find_super_statement_index(arena, &body_statements) {
            // Insert the field initializers immediately after `super(...)`.
            for &stmt in &body_statements[..=super_index] {
                statements.push(class_fields_visit(arena, stmt));
            }
            statements.extend(field_assignments);
            for &stmt in &body_statements[super_index + 1..] {
                statements.push(class_fields_visit(arena, stmt));
            }
        } else {
            // No `super()`: field initializers go to the top of the body.
            statements.extend(field_assignments);
            for stmt in body_statements {
                statements.push(class_fields_visit(arena, stmt));
            }
        }
    } else {
        // A synthesized constructor for a derived class must forward to the base
        // constructor first: `super(...arguments);`.
        if is_derived {
            statements.push(make_super_spread_arguments(arena));
        }
        statements.extend(field_assignments);
    }
    arena.new_block(NodeList::new(statements))
}

/// Reports whether the class has an `extends` heritage clause (is derived).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:GetClassExtendsHeritageElement
fn heritage_has_extends(arena: &NodeArena, heritage_clauses: Option<&NodeList>) -> bool {
    let Some(list) = heritage_clauses else {
        return false;
    };
    list.nodes.iter().any(|&clause| {
        matches!(arena.data(clause), NodeData::HeritageClause(d) if d.token == Kind::ExtendsKeyword)
    })
}

/// Builds a `super(...arguments);` statement for a synthesized derived
/// constructor.
///
/// Side effects: pushes the super/spread/call/statement nodes onto the arena.
fn make_super_spread_arguments(arena: &mut NodeArena) -> NodeId {
    let super_keyword = arena.new_keyword_expression(Kind::SuperKeyword);
    let arguments = arena.new_identifier("arguments");
    let spread = arena.new_spread_element(arguments);
    let call = arena.new_call_expression(
        super_keyword,
        None,
        None,
        NodeList::new(vec![spread]),
        NodeFlags::NONE,
    );
    arena.new_expression_statement(call)
}

/// Returns the statements of a constructor's body block (empty when the
/// constructor is a bodyless overload).
///
/// Side effects: none (reads the arena).
fn constructor_body_statements(arena: &NodeArena, constructor: NodeId) -> Vec<NodeId> {
    let body = match arena.data(constructor) {
        NodeData::ConstructorDeclaration(d) => d.body,
        _ => return Vec::new(),
    };
    match body.map(|b| arena.data(b)) {
        Some(NodeData::Block(d)) => d.list.nodes.clone(),
        _ => Vec::new(),
    }
}

/// Returns the index of the first statement in `statements` that is a
/// `super(...)` call statement, if any.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/utilities.go:FindSuperStatementIndexPath
fn find_super_statement_index(arena: &NodeArena, statements: &[NodeId]) -> Option<usize> {
    statements
        .iter()
        .position(|&stmt| is_super_call_statement(arena, stmt))
}

/// Reports whether `stmt` is an expression statement of a `super(...)` call.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/utilities.go:GetSuperCallFromStatement
fn is_super_call_statement(arena: &NodeArena, stmt: NodeId) -> bool {
    let expression = match arena.data(stmt) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => return false,
    };
    let expression = skip_parentheses(arena, expression);
    if arena.kind(expression) != Kind::CallExpression {
        return false;
    }
    let callee = match arena.data(expression) {
        NodeData::CallExpression(d) => d.expression,
        _ => return false,
    };
    arena.kind(callee) == Kind::SuperKeyword
}

/// Unwraps parenthesized expressions (`((x))` -> `x`).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:SkipParentheses (parentheses-only subset)
fn skip_parentheses(arena: &NodeArena, mut node: NodeId) -> NodeId {
    while arena.kind(node) == Kind::ParenthesizedExpression {
        node = match arena.data(node) {
            NodeData::ParenthesizedExpression(d) => d.expression,
            _ => break,
        };
    }
    node
}

/// Builds a `this.<name> = <initializer>;` expression statement.
///
/// Side effects: pushes the access/assignment/statement nodes onto the arena.
fn make_this_assignment(arena: &mut NodeArena, name: NodeId, initializer: NodeId) -> NodeId {
    let this = arena.new_keyword_expression(Kind::ThisKeyword);
    let target = arena.new_property_access_expression(this, None, name);
    let equals = arena.new_token(Kind::EqualsToken);
    let assignment = arena.new_binary_expression(target, equals, initializer);
    arena.new_expression_statement(assignment)
}

#[cfg(test)]
#[path = "classfields_test.rs"]
mod tests;
