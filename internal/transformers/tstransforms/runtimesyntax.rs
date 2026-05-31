//! Port of Go `internal/transformers/tstransforms/runtimesyntax.go`: the
//! `RuntimeSyntaxTransformer`, which lowers TypeScript runtime syntax (enums and
//! instantiated namespaces) into plain JavaScript IIFEs.
//!
//! # Scope (round 6n)
//!
//! Two independent lowerings, both reachable without the checker:
//!
//! - **enum -> IIFE**: a non-const `enum E { A, B }` becomes
//!   `var E;\n(function (E) {\n    E[E["A"] = 0] = "A";\n    E[E["B"] = 1] = "B";\n})(E || (E = {}));`,
//!   handling auto-numbered members, explicit numeric initializers (with
//!   auto-increment continuation), and string-initialized members (the
//!   `E["X"] = "v"` form, without the numeric reverse mapping).
//! - **namespace/module -> IIFE**: an instantiated
//!   `namespace N { export const x = 1; }` becomes
//!   `var N;\n(function (N) {\n    N.x = 1;\n})(N || (N = {}));`; an
//!   uninstantiated (type-only) namespace is omitted.
//!
//! ## Divergences from Go (checker-free reachable subset)
//!
//! - Go computes enum member values via the checker
//!   (`emitResolver.GetEnumMemberValue`); here they are evaluated
//!   **syntactically** (auto-increment + numeric/string literal initializers).
//!   Constant-folding of non-literal member initializers and `E.A`-style member
//!   reference folding are DEFER (blocked-by: checker constant evaluation).
//! - Go names the IIFE container/parameter with `NewGeneratedNameForNode` and
//!   rewrites in-namespace exported references via the binder
//!   `ReferenceResolver`. Here, the reachable subset (top-level, non-merged,
//!   non-exported declarations) uses the declaration's name text directly, and
//!   exported namespace members are lowered straight to `N.x = init` rather than
//!   relying on identifier rewriting.
//!
//! ## DEFER (see `tstransforms/mod.rs`)
//!
//! - `const enum` member-reference inlining (the declaration itself is omitted
//!   here when `preserveConstEnums` is off). blocked-by: checker constant
//!   evaluation.
//! - Exported enums/namespaces, merged (multi-declaration) names, nested/dotted
//!   namespaces, `export =` interplay, parameter properties, and `import=`
//!   lowering. blocked-by: the binder `ReferenceResolver` and merged-scope
//!   tracking.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{
    Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId, NodeList, VisitOptions,
};
use tsgo_printer::{EmitContext, EmitFlags};

/// Per-run configuration captured from the [`TransformOptions`].
#[derive(Clone, Copy)]
struct Config {
    /// Whether `const enum` declarations should be preserved (emitted as runtime
    /// enums) rather than omitted.
    preserve_const_enums: bool,
}

/// Builds a [`Transformer`] that lowers enum and namespace runtime syntax,
/// sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{tstransforms::runtimesyntax::new_runtime_syntax_transformer, TransformOptions};
/// let _tx = new_runtime_syntax_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/tstransforms/runtimesyntax.go:NewRuntimeSyntaxTransformer
pub fn new_runtime_syntax_transformer(opt: &TransformOptions) -> Transformer {
    let cfg = Config {
        preserve_const_enums: opt.compiler_options.should_preserve_const_enums(),
    };
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| runtime_syntax_visit(ec, node, cfg)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit: lowers enum and instantiated-namespace
/// declarations into IIFEs, recursing through container nodes via
/// [`visit_each_child_ec`].
///
/// Side effects: may push rebuilt nodes; may set emit flags.
// Go: internal/transformers/tstransforms/runtimesyntax.go:RuntimeSyntaxTransformer.visit
fn runtime_syntax_visit(ec: &mut EmitContext, node: NodeId, cfg: Config) -> NodeId {
    match ec.arena().kind(node) {
        Kind::EnumDeclaration => visit_enum_declaration(ec, node, cfg),
        Kind::ModuleDeclaration => visit_module_declaration(ec, node, cfg),
        _ => visit_each_child_ec(ec, node, cfg),
    }
}

/// Emit-context-threaded `VisitEachChild`: recursively runs the visit over each
/// child (so a nested declaration reachable through container nodes — a source
/// file's statements, etc. — is lowered), then rebuilds the node with the
/// transformed children. The node is returned unchanged when no child changed.
///
/// Side effects: may push rebuilt nodes; may set emit flags through recursion.
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId, cfg: Config) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: Vec<(NodeId, NodeId)> = Vec::new();
    for child in children {
        let transformed = runtime_syntax_visit(ec, child, cfg);
        if transformed != child {
            replacements.push((child, transformed));
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
            replacements
                .iter()
                .find_map(|&(from, to)| (from == child).then_some(to))
                .unwrap_or(child)
        })
}

/// Lowers a non-const `enum E { ... }` to the merged `var E;` + IIFE form.
///
/// Side effects: pushes rebuilt nodes; sets emit flags on the IIFE body.
// Go: internal/transformers/tstransforms/runtimesyntax.go:visitEnumDeclaration
fn visit_enum_declaration(ec: &mut EmitContext, node: NodeId, cfg: Config) -> NodeId {
    let (modifiers, name, members) = match ec.arena().data(node) {
        NodeData::EnumDeclaration(d) => (d.modifiers.clone(), d.name, d.members.clone()),
        _ => unreachable!("kind/data mismatch"),
    };
    // A `const enum` has no runtime form unless `preserveConstEnums` is set; it
    // is omitted here (member-reference inlining is handled elsewhere; DEFER).
    let is_const = modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::CONST));
    if is_const && !cfg.preserve_const_enums {
        return ec.arena_mut().new_not_emitted_statement();
    }
    let name_text = ec.arena().text(name).to_string();

    // var E;
    let var_statement = make_var_statement(ec.arena_mut(), &name_text);

    // The member assignment statements inside the IIFE body.
    let mut member_statements = Vec::new();
    let mut auto_value: i64 = 0;
    for member in members.nodes.iter().copied() {
        let stmt = transform_enum_member(ec.arena_mut(), &name_text, member, &mut auto_value);
        member_statements.push(stmt);
    }

    wrap_in_iife(ec, &name_text, var_statement, member_statements)
}

/// Lowers an instantiated `namespace N { ... }` to the merged `var N;` + IIFE
/// form. Uninstantiated (type-only) namespaces are omitted.
///
/// Side effects: pushes rebuilt nodes; sets emit flags on the IIFE body.
// Go: internal/transformers/tstransforms/runtimesyntax.go:visitModuleDeclaration
fn visit_module_declaration(ec: &mut EmitContext, node: NodeId, cfg: Config) -> NodeId {
    if !is_instantiated_module(ec.arena(), node, cfg.preserve_const_enums) {
        return ec.arena_mut().new_not_emitted_statement();
    }
    let (name, body) = match ec.arena().data(node) {
        NodeData::ModuleDeclaration(d) => (d.name, d.body),
        _ => unreachable!("kind/data mismatch"),
    };
    let name_text = ec.arena().text(name).to_string();

    // var N;
    let var_statement = make_var_statement(ec.arena_mut(), &name_text);

    let member_statements = transform_module_body(ec, &name_text, body, cfg);

    wrap_in_iife(ec, &name_text, var_statement, member_statements)
}

/// Transforms the body of an instantiated namespace, lowering exported value
/// declarations to namespace-qualified assignments (`export const x = 1;` ->
/// `N.x = 1;`). Other statements are visited recursively.
///
/// Side effects: may push rebuilt nodes.
// Go: internal/transformers/tstransforms/runtimesyntax.go:transformModuleBody
fn transform_module_body(
    ec: &mut EmitContext,
    name_text: &str,
    body: Option<NodeId>,
    cfg: Config,
) -> Vec<NodeId> {
    let statements = match body.map(|b| ec.arena().data(b)) {
        Some(NodeData::ModuleBlock(d)) => d.statements.nodes.clone(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for stmt in statements {
        if let Some(mut assignments) = lower_exported_variable_statement(ec, name_text, stmt, cfg) {
            out.append(&mut assignments);
        } else {
            out.push(runtime_syntax_visit(ec, stmt, cfg));
        }
    }
    out
}

/// If `stmt` is an `export`ed variable statement, lowers each initialized,
/// simply-named declaration to a namespace-qualified assignment statement
/// (`N.x = init;`) and returns them; otherwise returns `None`.
///
/// Side effects: may push rebuilt nodes.
// Go: internal/transformers/tstransforms/runtimesyntax.go:visitVariableStatement
fn lower_exported_variable_statement(
    ec: &mut EmitContext,
    name_text: &str,
    stmt: NodeId,
    cfg: Config,
) -> Option<Vec<NodeId>> {
    let (modifiers, declaration_list) = match ec.arena().data(stmt) {
        NodeData::VariableStatement(d) => (d.modifiers.clone(), d.declaration_list),
        _ => return None,
    };
    let is_exported = modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::EXPORT));
    if !is_exported {
        return None;
    }
    let declarations = match ec.arena().data(declaration_list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
        _ => return None,
    };
    let mut out = Vec::new();
    for decl in declarations {
        let (decl_name, initializer) = match ec.arena().data(decl) {
            NodeData::VariableDeclaration(d) => (d.name, d.initializer),
            _ => continue,
        };
        // DEFER: binding-pattern exports (need destructuring flattening).
        if ec.arena().kind(decl_name) != Kind::Identifier {
            continue;
        }
        let Some(initializer) = initializer else {
            continue;
        };
        let member_text = ec.arena().text(decl_name).to_string();
        let initializer = runtime_syntax_visit(ec, initializer, cfg);
        // N.x = init
        let container = ec.arena_mut().new_identifier(name_text);
        let member = ec.arena_mut().new_identifier(&member_text);
        let prop = ec
            .arena_mut()
            .new_property_access_expression(container, None, member);
        let expr = assignment(ec.arena_mut(), prop, initializer);
        let assignment_stmt = ec.arena_mut().new_expression_statement(expr);
        out.push(assignment_stmt);
    }
    Some(out)
}

/// Wraps the lowered member statements of an enum/namespace `name` in the IIFE
/// form, prefixed by the `var name;` statement, returning a
/// `SyntaxList[var, callStatement]`.
///
/// Side effects: pushes rebuilt nodes; sets the `MULTI_LINE` flag on the body.
// Go: internal/transformers/tstransforms/runtimesyntax.go:visitEnumDeclaration / visitModuleDeclaration
fn wrap_in_iife(
    ec: &mut EmitContext,
    name_text: &str,
    var_statement: NodeId,
    member_statements: Vec<NodeId>,
) -> NodeId {
    let body = ec.arena_mut().new_block(NodeList::new(member_statements));
    // Force the IIFE body to emit multi-line (Go sets `Block.MultiLine`).
    ec.set_emit_flags(body, EmitFlags::MULTI_LINE);

    // (function (name) { <body> })
    let param = make_parameter(ec.arena_mut(), name_text);
    let func = ec.arena_mut().new_function_expression(
        None,
        None,
        None,
        None,
        NodeList::new(vec![param]),
        None,
        None,
        Some(body),
    );
    let callee = ec.arena_mut().new_parenthesized_expression(func);

    // name || (name = {})
    let arg = make_iife_argument(ec.arena_mut(), name_text);
    let call = ec.arena_mut().new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![arg]),
        NodeFlags::NONE,
    );
    let call_statement = ec.arena_mut().new_expression_statement(call);

    ec.arena_mut()
        .new_syntax_list(NodeList::new(vec![var_statement, call_statement]))
}

/// Syntactic (checker-free) port of Go's `IsInstantiatedModule` for the
/// reachable subset: a namespace is instantiated unless its body contains only
/// non-instantiating (type-only) statements. A module with no body is
/// instantiated.
///
/// Covered non-instantiating statements: `interface` and `type` aliases. (Const
/// enum-only bodies and import/export alias-target analysis are DEFER.)
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsInstantiatedModule / getModuleInstanceState
fn is_instantiated_module(arena: &NodeArena, node: NodeId, _preserve_const_enums: bool) -> bool {
    let body = match arena.data(node) {
        NodeData::ModuleDeclaration(d) => d.body,
        _ => return true,
    };
    let Some(body) = body else {
        return true;
    };
    match arena.data(body) {
        NodeData::ModuleBlock(d) => d
            .statements
            .nodes
            .iter()
            .any(|&s| is_instantiating_statement(arena, s)),
        // Nested/dotted module bodies: assume instantiated (DEFER deep analysis).
        _ => true,
    }
}

/// Reports whether a namespace-body statement contributes a runtime (value)
/// form, i.e. is not purely a type declaration.
///
/// Side effects: none (reads the arena).
fn is_instantiating_statement(arena: &NodeArena, node: NodeId) -> bool {
    !matches!(
        arena.kind(node),
        Kind::InterfaceDeclaration | Kind::TypeAliasDeclaration
    )
}

/// Transforms a single enum member into its assignment statement. Auto-numbers
/// members from `auto_value`, emitting the numeric reverse mapping
/// (`E[E["A"] = 0] = "A";`).
///
/// Side effects: pushes rebuilt nodes; advances `auto_value`.
// Go: internal/transformers/tstransforms/runtimesyntax.go:transformEnumMember
fn transform_enum_member(
    arena: &mut NodeArena,
    enum_name: &str,
    member: NodeId,
    auto_value: &mut i64,
) -> NodeId {
    let (name, initializer) = match arena.data(member) {
        NodeData::EnumMember(d) => (d.name, d.initializer),
        _ => unreachable!("kind/data mismatch"),
    };
    let property_name = expression_for_property_name(arena, name);

    // The member value and whether the numeric reverse mapping applies:
    //  - explicit numeric-literal initializer: sets the value and continues
    //    auto-numbering from it; reverse-mapped.
    //  - no initializer: the running ordinal; reverse-mapped.
    //  - string-literal initializer: the string value; *not* reverse-mapped
    //    (string enums have no reverse mapping).
    // (Non-literal initializer constant-folding is DEFER: checker.)
    let (value, use_explicit_reverse_mapping) = match initializer {
        Some(init) if arena.kind(init) == Kind::NumericLiteral => {
            let n: i64 = arena.text(init).parse().unwrap_or(*auto_value);
            *auto_value = n + 1;
            let v = arena.new_numeric_literal(&n.to_string(), tsgo_ast::TokenFlags::NONE);
            (v, true)
        }
        Some(init) if arena.kind(init) == Kind::StringLiteral => {
            let text = arena.text(init).to_string();
            let v = arena.new_string_literal(&text, tsgo_ast::TokenFlags::NONE);
            (v, false)
        }
        _ => {
            let n = *auto_value;
            *auto_value += 1;
            let v = arena.new_numeric_literal(&n.to_string(), tsgo_ast::TokenFlags::NONE);
            (v, true)
        }
    };

    // E["A"] = <value>
    let qualified = enum_qualified_element(arena, enum_name, property_name);
    let inner = assignment(arena, qualified, value);

    // For reverse-mappable (numeric) members: E[E["A"] = <value>] = "A".
    let expression = if use_explicit_reverse_mapping {
        let container = arena.new_identifier(enum_name);
        let reverse_target = arena.new_element_access_expression(container, None, inner);
        let reverse_key = expression_for_property_name(arena, name);
        assignment(arena, reverse_target, reverse_key)
    } else {
        inner
    };

    arena.new_expression_statement(expression)
}

/// Builds `E["A"]`: an element access of the enum container by the member's
/// property-name expression.
///
/// Side effects: pushes rebuilt nodes.
// Go: internal/transformers/tstransforms/runtimesyntax.go:getEnumQualifiedElement
fn enum_qualified_element(arena: &mut NodeArena, enum_name: &str, key: NodeId) -> NodeId {
    let container = arena.new_identifier(enum_name);
    arena.new_element_access_expression(container, None, key)
}

/// Builds the property-name expression for an enum member: an identifier or
/// string-literal name becomes a string literal `"name"`.
///
/// Side effects: pushes rebuilt nodes.
// Go: internal/transformers/tstransforms/runtimesyntax.go:getExpressionForPropertyName
fn expression_for_property_name(arena: &mut NodeArena, name: NodeId) -> NodeId {
    match arena.kind(name) {
        Kind::Identifier | Kind::StringLiteral => {
            let text = arena.text(name).to_string();
            arena.new_string_literal(&text, tsgo_ast::TokenFlags::NONE)
        }
        _ => name,
    }
}

/// Builds `var <name>;` (a `var` declaration with no initializer).
///
/// Side effects: pushes rebuilt nodes.
// Go: internal/transformers/tstransforms/runtimesyntax.go:addVarForDeclaration
fn make_var_statement(arena: &mut NodeArena, name: &str) -> NodeId {
    let ident = arena.new_identifier(name);
    let decl = arena.new_variable_declaration(ident, None, None, None);
    let list = arena.new_variable_declaration_list(NodeList::new(vec![decl]));
    arena.new_variable_statement(None, list)
}

/// Builds the IIFE parameter `(<name>)`.
///
/// Side effects: pushes rebuilt nodes.
fn make_parameter(arena: &mut NodeArena, name: &str) -> NodeId {
    let ident = arena.new_identifier(name);
    arena.new_parameter_declaration(None, None, ident, None, None, None)
}

/// Builds the IIFE argument `<name> || (<name> = {})`.
///
/// Side effects: pushes rebuilt nodes.
// Go: internal/transformers/tstransforms/runtimesyntax.go:visitEnumDeclaration (enumArg)
fn make_iife_argument(arena: &mut NodeArena, name: &str) -> NodeId {
    let left = arena.new_identifier(name);
    let assign_target = arena.new_identifier(name);
    let empty_object = arena.new_object_literal_expression(NodeList::new(vec![]));
    let assign = assignment(arena, assign_target, empty_object);
    let bar_bar = arena.new_token(Kind::BarBarToken);
    arena.new_binary_expression(left, bar_bar, assign)
}

/// Builds `left = right`.
///
/// Side effects: pushes rebuilt nodes.
fn assignment(arena: &mut NodeArena, left: NodeId, right: NodeId) -> NodeId {
    let equals = arena.new_token(Kind::EqualsToken);
    arena.new_binary_expression(left, equals, right)
}

#[cfg(test)]
#[path = "runtimesyntax_test.rs"]
mod tests;
