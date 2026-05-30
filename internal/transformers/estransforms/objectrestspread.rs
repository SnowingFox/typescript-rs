//! Port of Go `internal/transformers/estransforms/objectrestspread.go`: lowers
//! ES2018 object spread in object literals to `Object.assign` calls and object
//! **rest** bindings in variable declarations to the `__rest` helper.
//!
//! # Scope (rounds 6d / 6g)
//!
//! - Object **spread** inside an object literal (6d): `{ ...x }` →
//!   `Object.assign({}, x)`, chunking adjacent non-spread properties and folding
//!   them pairwise (matching Go's `NewAssignHelper`).
//! - Object **rest** binding in a variable declaration (6g): `var { ...rest } =
//!   o;` → `var rest = __rest(o, []);` and `var { a, ...rest } = o;` →
//!   `var { a } = o, rest = __rest(o, ["a"]);`, requesting the `__rest` helper
//!   (whose definition is emitted in the module prologue). This is the reachable
//!   subset of Go's `FlattenDestructuringBinding` object-rest arm: a top-level
//!   variable declaration whose name is an object binding pattern ending in a
//!   rest element, with simple identifier leading bindings (and literal property
//!   keys) over a simple-copiable initializer.
//!
//! Deferred (DEFER, see `estransforms/mod.rs`): the generic
//! `FlattenDestructuringBinding` (nested/array patterns, defaults, computed
//! property keys needing temp caching, non-simple initializers needing a hoisted
//! temp) — that is the round-6g `destructuring.go` port; rest in **parameters** /
//! `for-of` / `catch` / assignment-destructuring patterns (need the parameter /
//! assignment flatteners and `FlattenDestructuringAssignment`).

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, TokenFlags, VisitOptions};
use tsgo_printer::emithelpers::REST_HELPER;
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers object spread and object-rest bindings,
/// sharing the pipeline's emit context.
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
        Box::new(|ec: &mut EmitContext, node: NodeId| object_rest_spread_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit: lowers object-rest variable declarations (which
/// request the `__rest` helper) and, at the source-file boundary, attaches the
/// requested helpers. Object-literal spread lowering is purely structural and
/// runs arena-only.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
// Go: internal/transformers/estransforms/objectrestspread.go:objectRestSpreadTransformer.visit
fn object_rest_spread_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::VariableStatement => {
            if let Some(lowered) = try_lower_variable_statement_with_rest(ec, node) {
                lowered
            } else {
                // No reachable object-rest binding here; lower any object-literal
                // spreads in the subtree (arena-only).
                object_spread_visit(ec.arena_mut(), node)
            }
        }
        _ => object_spread_visit(ec.arena_mut(), node),
    }
}

/// Visits the source file's top-level statements with the emit-context-threaded
/// visitor, then attaches the helpers requested during the visit so the printer
/// emits them in the prologue.
///
/// Side effects: rebuilds the source file; attaches emit helpers.
// Go: internal/transformers/estransforms/objectrestspread.go:visitSourceFile
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
        .map(|s| object_rest_spread_visit(ec, s))
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

/// Lowers a variable statement whose declaration list contains an object-rest
/// binding, returning the rebuilt statement. Returns `None` when no declaration
/// is a reachable object-rest binding (so the caller can fall back to the
/// arena-only object-literal lowering), or when a rest binding falls outside the
/// reachable subset (the statement is then left unchanged — DEFER).
///
/// Side effects: may push rebuilt nodes; may request the `__rest` helper.
// Go: internal/transformers/estransforms/objectrestspread.go:visitVariableStatement / visitVariableDeclarationWorker
fn try_lower_variable_statement_with_rest(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let (modifiers, list) = match ec.arena().data(node) {
        NodeData::VariableStatement(d) => (d.modifiers.clone(), d.declaration_list),
        _ => return None,
    };
    let declarations = match ec.arena().data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.clone(),
        _ => return None,
    };
    if !declarations
        .nodes
        .iter()
        .any(|&d| declaration_has_object_rest(ec.arena(), d))
    {
        return None;
    }
    let mut new_decls: Vec<NodeId> = Vec::new();
    for &decl in &declarations.nodes {
        if declaration_has_object_rest(ec.arena(), decl) {
            // Unreachable rest shape: bail entirely and leave the statement
            // unchanged (no helper requested, no partial lowering).
            let mut lowered = lower_object_rest_declaration(ec, decl)?;
            new_decls.append(&mut lowered);
        } else {
            let visited = object_spread_visit(ec.arena_mut(), decl);
            new_decls.push(visited);
        }
    }
    let block_scoped = ec.arena().flags(list) & NodeFlags::BLOCK_SCOPED;
    let new_list = ec
        .arena_mut()
        .new_variable_declaration_list(NodeList::new(new_decls));
    ec.arena_mut().add_flags(new_list, block_scoped);
    Some(ec.arena_mut().new_variable_statement(modifiers, new_list))
}

/// Flattens one object-rest variable declaration `{ <leading>, ...rest } = init`
/// into `{ <leading> } = init` (when leading bindings are present) plus
/// `rest = __rest(init, [<keys>])`. Returns `None` for shapes outside the
/// reachable subset (see module docs); no helper is requested in that case.
///
/// Side effects: may push rebuilt nodes; requests the `__rest` helper on success.
// Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern (object-rest arm)
fn lower_object_rest_declaration(ec: &mut EmitContext, decl: NodeId) -> Option<Vec<NodeId>> {
    let (name, initializer) = match ec.arena().data(decl) {
        NodeData::VariableDeclaration(d) => (d.name, d.initializer),
        _ => return None,
    };
    let initializer = initializer?;
    let elements = match ec.arena().data(name) {
        NodeData::ObjectBindingPattern(d) => d.elements.nodes.clone(),
        _ => return None,
    };
    let (rest_element, leading) = elements.split_last()?;
    let rest_target = reachable_rest_target(ec.arena(), *rest_element)?;
    let mut keys: Vec<String> = Vec::with_capacity(leading.len());
    for &element in leading {
        keys.push(reachable_leading_key(ec.arena(), element)?);
    }
    // With leading bindings, `init` is referenced twice (the leading pattern and
    // the `__rest` call), so it must be duplicable without a hoisted temp.
    if !leading.is_empty() && !is_simple_copiable(ec.arena(), initializer) {
        return None;
    }

    // Reachable: commit. Lower any object-literal spread in the initializer.
    let value = object_spread_visit(ec.arena_mut(), initializer);
    let property_names: Vec<NodeId> = keys
        .iter()
        .map(|key| ec.arena_mut().new_string_literal(key, TokenFlags::NONE))
        .collect();
    let rest_call = new_rest_helper(ec, value, property_names);

    let mut decls = Vec::new();
    if !leading.is_empty() {
        let leading_pattern = ec
            .arena_mut()
            .new_binding_pattern(Kind::ObjectBindingPattern, NodeList::new(leading.to_vec()));
        decls.push(ec.arena_mut().new_variable_declaration(
            leading_pattern,
            None,
            None,
            Some(value),
        ));
    }
    decls.push(
        ec.arena_mut()
            .new_variable_declaration(rest_target, None, None, Some(rest_call)),
    );
    Some(decls)
}

/// Reports whether `decl` is a variable declaration whose name is an object
/// binding pattern ending in a rest element (`{ ..., ...rest }`).
///
/// Side effects: none (reads the arena).
fn declaration_has_object_rest(arena: &NodeArena, decl: NodeId) -> bool {
    let name = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.name,
        _ => return false,
    };
    let elements = match arena.data(name) {
        NodeData::ObjectBindingPattern(d) => &d.elements,
        _ => return false,
    };
    elements
        .nodes
        .last()
        .copied()
        .is_some_and(|last| has_rest_token(arena, last))
}

/// Returns the rest element's binding target identifier if the element is a
/// simple rest (`...rest`, no property rename / default), else `None`.
///
/// Side effects: none (reads the arena).
fn reachable_rest_target(arena: &NodeArena, element: NodeId) -> Option<NodeId> {
    let fields = binding_element_fields(arena, element)?;
    if fields.dot_dot_dot_token.is_none()
        || fields.property_name.is_some()
        || fields.initializer.is_some()
    {
        return None;
    }
    let name = fields.name?;
    (arena.kind(name) == Kind::Identifier).then_some(name)
}

/// Returns the property key (the name to exclude from `__rest`) for a simple
/// leading binding element (`{ a }` → `"a"`, `{ a: b }` → `"a"`), or `None` for
/// shapes outside the reachable subset (rest, default, nested pattern, computed
/// or numeric key).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:TryGetPropertyNameOfBindingOrAssignmentElement (literal-key subset)
fn reachable_leading_key(arena: &NodeArena, element: NodeId) -> Option<String> {
    let fields = binding_element_fields(arena, element)?;
    if fields.dot_dot_dot_token.is_some() || fields.initializer.is_some() {
        return None;
    }
    let name = fields.name?;
    if arena.kind(name) != Kind::Identifier {
        return None;
    }
    match fields.property_name {
        None => Some(arena.text(name).to_string()),
        Some(property_name) => match arena.kind(property_name) {
            Kind::Identifier | Kind::StringLiteral => Some(arena.text(property_name).to_string()),
            _ => None,
        },
    }
}

/// The four child slots of a [`NodeData::BindingElement`].
struct BindingElementFields {
    dot_dot_dot_token: Option<NodeId>,
    property_name: Option<NodeId>,
    name: Option<NodeId>,
    initializer: Option<NodeId>,
}

/// Reads the fields of a [`NodeData::BindingElement`], or `None` if `node` is
/// not a binding element.
///
/// Side effects: none (reads the arena).
fn binding_element_fields(arena: &NodeArena, node: NodeId) -> Option<BindingElementFields> {
    match arena.data(node) {
        NodeData::BindingElement(d) => Some(BindingElementFields {
            dot_dot_dot_token: d.dot_dot_dot_token,
            property_name: d.property_name,
            name: d.name,
            initializer: d.initializer,
        }),
        _ => None,
    }
}

/// Reports whether a binding element carries a `...` rest token.
///
/// Side effects: none (reads the arena).
fn has_rest_token(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::BindingElement(d) if d.dot_dot_dot_token.is_some())
}

/// Reports whether `node` is a simple, side-effect-free expression that can be
/// duplicated (here: referenced by both the leading pattern and the `__rest`
/// call) without a hoisted temp.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/utilities.go:IsSimpleCopiableExpression
fn is_simple_copiable(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::Identifier
            | Kind::NumericLiteral
            | Kind::StringLiteral
            | Kind::BigIntLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::ThisKeyword
            | Kind::SuperKeyword
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
    )
}

/// Builds `__rest(value, [<property_names>])`, requesting the `__rest` helper so
/// its definition is emitted in the module prologue.
///
/// Side effects: pushes the call/array nodes; requests the `__rest` helper.
// Go: internal/printer/factory.go:NodeFactory.NewRestHelper
fn new_rest_helper(ec: &mut EmitContext, value: NodeId, property_names: Vec<NodeId>) -> NodeId {
    ec.request_emit_helper(&REST_HELPER);
    let rest_name = ec.factory().new_unscoped_helper_name("__rest");
    let prop_array = ec
        .arena_mut()
        .new_array_literal_expression(NodeList::new(property_names));
    ec.arena_mut().new_call_expression(
        rest_name,
        None,
        None,
        NodeList::new(vec![value, prop_array]),
        NodeFlags::NONE,
    )
}

/// Lowers object spread in the subtree rooted at `node` (arena-only; no helper).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/objectrestspread.go:objectRestSpreadTransformer.visit (object-literal arm)
fn object_spread_visit(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if arena.kind(node) == Kind::ObjectLiteralExpression && object_literal_has_spread(arena, node) {
        return visit_object_literal_expression(arena, node);
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| object_spread_visit(a, c))
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
            let target = object_spread_visit(arena, target);
            objects.push(target);
        } else {
            let visited = object_spread_visit(arena, element);
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
