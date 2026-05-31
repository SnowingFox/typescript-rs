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
//! Round 6c-4 adds two more shapes:
//! * **Private instance fields** (`#x`) — direct `WeakMap` form: a class-scoped
//!   private environment maps each `#x` to a `_C_x` brand; `var _C_x = new
//!   WeakMap();` is hoisted before the class (via the `SyntaxList`), the field
//!   initializer becomes `_C_x.set(this, init)` in the constructor, and private
//!   accesses in member bodies are rewritten (`obj.#x` → `_C_x.get(obj)`,
//!   `obj.#x = e` → `_C_x.set(obj, e)`).
//! * **Computed instance-field names** (`[k] = init`) — the key is cached in a
//!   `var <temp> = k;` hoisted before the class (so it is evaluated once at
//!   class-definition time) and the initializer becomes `this[<temp>] = init`.
//!
//! Round 6o extends the instance-field lowering to **class *expressions***
//! (`const C = class { x = 1 };` → `const C = class { constructor() { this.x =
//! 1; } };`): the same lowering runs in expression position whenever it produces
//! a single node (instance fields only).
//!
//! Round 6r lowers a class expression with **static field(s)** by wrapping the
//! class in a comma sequence with a hoisted temp
//! (`const C = class { static x = 1 };` →
//! `var _a;` + `const C = (_a = class {}, _a.x = 1, _a);`): the lowering core is
//! shared via [`lower_class_parts`], and the expression-position consumer
//! [`try_lower_class_expression`] allocates the temp with the round-6p
//! [`new_temp_variable`](tsgo_printer::factory::NodeFactory::new_temp_variable)
//! generator (reused in each position so it materializes the same `_a`) and
//! hoists `var _a;` into the source file's variable environment (rounds 6c-3/6i).
//! A class expression that would hoist statements *other than* static
//! assignments (computed-name temp caches or `var _C_x = new WeakMap();` private
//! brands) is still left unchanged, since Go threads those through
//! `pendingExpressions` (comma-sequence inlining) which is not yet ported.
//!
//! Deferred (DEFER(P5), see `estransforms/mod.rs`): the **named-helper-import**
//! private form (`__classPrivateFieldGet/Set`), **private static fields**,
//! **private methods/accessors** (`WeakSet` brand checks), **`accessor` fields**
//! (auto-accessor → backing field + get/set redirectors; needs the name
//! generator + a second-pass result visitor), **class-expression statement
//! hoisting for computed/private fields** (these need Go's `pendingExpressions`
//! comma-sequence inlining, not yet ported) and **nested-scope** class-expression
//! static hoisting (only the source file opens a variable environment here),
//! **parameter properties**
//! (a TS-transform concern), constructor **prologue directives**,
//! **anonymous-class** static/private members (need a generated class name),
//! name-generator-backed **temp/brand uniqueness**, and
//! `--target`/`useDefineForClassFields` gating — these need helper-library emit,
//! the emit-context name generator, and/or checker info not yet ported.

use crate::{new_transformer, TransformOptions, Transformer};
use rustc_hash::FxHashMap;
use tsgo_ast::{
    Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId, NodeList, VisitOptions,
};
use tsgo_printer::emitcontext::AutoGenerateOptions;
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
        Box::new(|ec: &mut EmitContext, node: NodeId| class_fields_visit_ec(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded entry that reaches class declarations/expressions while
/// threading the [`EmitContext`] (so the round-6p node-based name generator is
/// available for **auto-accessor** lowering, which allocates a generated private
/// backing-field name). Classes that contain an `accessor` member are routed to
/// [`try_lower_auto_accessor_class`]; every other class falls back to the
/// arena-only [`try_lower_simple_class`] (rounds 6c/6o). Non-class nodes recurse
/// through [`visit_each_child_ec`] so a class nested in a statement is still
/// reached.
///
/// Side effects: may push rebuilt nodes; allocates generated names.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.visit
fn class_fields_visit_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
    // The source file opens a variable environment so a temp hoisted for a
    // class-expression static-field comma-sequence (`var _a;`) lands as a leading
    // declaration at module top (round 6r).
    if ec.arena().kind(node) == Kind::SourceFile {
        return visit_source_file_ec(ec, node);
    }
    if matches!(
        ec.arena().kind(node),
        Kind::ClassDeclaration | Kind::ClassExpression
    ) {
        if class_has_auto_accessor(ec.arena(), node) {
            // A class with an auto-accessor is lowered via the generated-name
            // path; an unsupported accessor shape (static / computed / decorated)
            // is left unchanged for the deferred fuller port rather than being
            // mis-handled by the plain-field path.
            return try_lower_auto_accessor_class(ec, node).unwrap_or(node);
        }
        if ec.arena().kind(node) == Kind::ClassExpression {
            // Class expressions take the emit-context path so a static-field
            // comma-sequence can allocate + hoist a temp (round 6r). Instance-only
            // expressions (round 6o) are rebuilt in place there too.
            if let Some(lowered) = try_lower_class_expression(ec, node) {
                return lowered;
            }
        } else if let Some(lowered) = try_lower_simple_class(ec.arena_mut(), node) {
            return lowered;
        }
    }
    visit_each_child_ec(ec, node)
}

/// Wraps the source file's statements in a variable environment so a temp
/// hoisted while lowering a class-expression static field is emitted as a
/// leading `var _a;` statement (mirrors the exponentiation transformer's
/// source-file handling).
///
/// Side effects: pushes/pops a variable environment; rebuilds the source file.
// Go: internal/printer/emitcontext.go:EmitContext.VisitVariableEnvironment (top-level statements)
fn visit_source_file_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
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
        visited.push(class_fields_visit_ec(ec, statement));
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

/// Emit-context-threaded `VisitEachChild`: recurses [`class_fields_visit_ec`]
/// over every child, then rebuilds the node with the transformed children
/// (returning it unchanged when nothing changed, preserving source positions).
///
/// Side effects: may push rebuilt nodes through the recursive visits.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.visit (default: VisitEachChild)
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = class_fields_visit_ec(ec, child);
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

/// Lowers class fields in the subtree rooted at `node`.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.visit
fn class_fields_visit(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if matches!(
        arena.kind(node),
        Kind::ClassDeclaration | Kind::ClassExpression
    ) {
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

/// The receiver-agnostic pieces of a lowered class, produced by
/// [`lower_class_parts`] and consumed by the two call sites: the
/// statement-position [`try_lower_simple_class`] (which emits a `SyntaxList`
/// with `C.x = ...` assignments using the class name) and the
/// expression-position [`try_lower_class_expression`] (which wraps the class in
/// a comma sequence with a temp, `_a.x = ...`).
struct ClassLoweringParts {
    /// The rebuilt class node (declaration or expression, per the input kind),
    /// with instance fields moved into a constructor and static fields removed.
    class: NodeId,
    /// Statements that must be hoisted *before* the class: computed-name temp
    /// caches (`var _a = k;`) then `var _C_x = new WeakMap();` brand declarations.
    pre_statements: Vec<NodeId>,
    /// Each static field as a `(name, initializer)` pair, kept receiver-agnostic
    /// so the caller can build `C.x = init` (statement) or `_a.x = init`
    /// (expression).
    static_fields: Vec<(NodeId, NodeId)>,
}

/// Lowers a class's members into [`ClassLoweringParts`]: instance fields (plain
/// identifier-named, with initializers) are hoisted into a constructor as
/// `this.x = init` statements (synthesized or inserted into an existing
/// constructor body); private instance fields become `WeakMap` brands; computed
/// names are cached in temps; static fields are returned as receiver-agnostic
/// `(name, init)` pairs. Returns `None` (leaving the class for the deferred
/// fuller port) for any shape outside this slice (private static fields,
/// non-identifier/computed static names, fields without initializers, multiple
/// constructors, private methods/accessors).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.transformClassMembers
fn lower_class_parts(arena: &mut NodeArena, node: NodeId) -> Option<ClassLoweringParts> {
    let kind = arena.kind(node);
    let (modifiers, name, type_parameters, heritage_clauses, members) = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => (
            d.modifiers.clone(),
            d.name,
            d.type_parameters.clone(),
            d.heritage_clauses.clone(),
            d.members.clone(),
        ),
        _ => return None,
    };

    // Build the class-scoped private environment (private field name -> WeakMap
    // brand variable name). Returns `None` (leaving the class for the deferred
    // fuller port) when the class uses private *methods*/*accessors* (WeakSet
    // brand checks, not yet ported) or is anonymous with private members.
    let private_env = build_private_env(arena, &members, name)?;

    let mut field_assignments = Vec::new();
    // Static fields kept as `(name, initializer)` pairs; the receiver (class
    // name `C` or temp `_a`) is chosen by the caller.
    let mut static_fields: Vec<(NodeId, NodeId)> = Vec::new();
    // `var <temp> = <computed-key>;` statements hoisted before the class so each
    // computed property name is evaluated once at class-definition time.
    let mut computed_temp_decls = Vec::new();
    let mut computed_temp_index = 0usize;
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
                let is_static = field_modifiers
                    .as_ref()
                    .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::STATIC));
                // Fields without an initializer need `--useDefineForClassFields`
                // gating -> deferred.
                let initializer = initializer?;
                let initializer = class_fields_visit(arena, initializer);
                if is_private_name(arena, field_name) {
                    // `#x = init` -> `_C_x.set(this, init);` in the constructor,
                    // with `var _C_x = new WeakMap();` hoisted before the class.
                    // Private *static* fields need a static-block lowering -> deferred.
                    if is_static {
                        return None;
                    }
                    let brand =
                        private_env_brand(&private_env, arena.text(field_name))?.to_string();
                    field_assignments.push(make_private_set(arena, &brand, initializer));
                } else if arena.kind(field_name) == Kind::Identifier {
                    if is_static {
                        // `static x = 1` -> `<receiver>.x = 1` (statement after the
                        // class declaration, or expression in the comma sequence).
                        static_fields.push((field_name, initializer));
                    } else {
                        field_assignments.push(make_this_assignment(
                            arena,
                            field_name,
                            initializer,
                        ));
                    }
                } else if arena.kind(field_name) == Kind::ComputedPropertyName {
                    // `[k] = init` -> cache the key in `var <temp> = k;` before the
                    // class, then `this[<temp>] = init` in the constructor. Static
                    // computed fields need `C[<temp>] = init` placement -> deferred.
                    if is_static {
                        return None;
                    }
                    let key = match arena.data(field_name) {
                        NodeData::ComputedPropertyName(d) => d.expression,
                        _ => return None,
                    };
                    let key = class_fields_visit(arena, key);
                    let temp = computed_temp_name(computed_temp_index);
                    computed_temp_index += 1;
                    computed_temp_decls.push(make_temp_var(arena, &temp, key));
                    field_assignments.push(make_this_element_assignment(arena, &temp, initializer));
                } else {
                    // Other property-name shapes need different lowering -> deferred.
                    return None;
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
            _ => {
                // Method/accessor/etc.: rewrite private accesses in its body when
                // the class has a private environment; otherwise visit normally.
                let visited = if private_env.is_empty() {
                    class_fields_visit(arena, member)
                } else {
                    lower_private_access(arena, member, &private_env)
                };
                member_plan.push(Some(visited));
            }
        }
    }
    if field_assignments.is_empty()
        && static_fields.is_empty()
        && private_env.is_empty()
        && computed_temp_decls.is_empty()
    {
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
        kind,
        modifiers,
        name,
        type_parameters,
        heritage_clauses,
        NodeList::new(new_members),
    );
    // Statements hoisted before the class: computed-name temp caches, then
    // `var _C_x = new WeakMap();` brand declarations.
    let mut pre_statements = computed_temp_decls;
    for (_, brand) in &private_env {
        let decl = make_weakmap_brand_var(arena, brand);
        pre_statements.push(decl);
    }
    Some(ClassLoweringParts {
        class,
        pre_statements,
        static_fields,
    })
}

/// Lowers a class **declaration** (or an instance-field-only class expression):
/// instance fields move into a constructor, and static/private/computed members
/// that need hoisting are emitted via a `SyntaxList` of statements
/// (`var ...; class C {...} C.x = ...;`). Returns `None` (leaving the class for
/// the deferred fuller port) for shapes outside this slice, and for a class
/// *expression* that needs statement hoisting (handled by
/// [`try_lower_class_expression`] instead).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.visitClassDeclaration
fn try_lower_simple_class(arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
    let kind = arena.kind(node);
    let name = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.name,
        _ => return None,
    };
    let parts = lower_class_parts(arena, node)?;
    if parts.pre_statements.is_empty() && parts.static_fields.is_empty() {
        // Instance-field-only (round 6o for class expressions): no hoist needed.
        return Some(parts.class);
    }
    if kind == Kind::ClassExpression {
        // A class *expression* that needs statement hoisting is wrapped in a comma
        // sequence with a temp by `try_lower_class_expression` (round 6r); this
        // arena-only path only builds the statement-position `SyntaxList`.
        return None;
    }
    // `static x = 1` -> `C.x = 1;` after the class declaration. An anonymous
    // declaration would need a generated class name -> deferred.
    let class_name = name?;
    let mut statements =
        Vec::with_capacity(parts.pre_statements.len() + 1 + parts.static_fields.len());
    statements.extend(parts.pre_statements);
    statements.push(parts.class);
    for (field_name, initializer) in parts.static_fields {
        statements.push(make_static_assignment(
            arena,
            class_name,
            field_name,
            initializer,
        ));
    }
    Some(arena.new_syntax_list(NodeList::new(statements)))
}

/// Lowers a class **expression** (emit-context path). Instance-field-only
/// expressions stay a class expression (round 6o). A class expression with
/// **static field(s)** is wrapped in a comma sequence with a hoisted temp
/// (round 6r):
///
/// ```text
/// const C = class { static x = 1 };
/// ->
/// var _a;
/// const C = (_a = class {}, _a.x = 1, _a);
/// ```
///
/// The temp (`_a`) is allocated via the round-6p emit-context name generator
/// ([`new_temp_variable`](tsgo_printer::factory::NodeFactory::new_temp_variable))
/// and declared with a `var _a;` hoisted into the enclosing scope's variable
/// environment ([`add_variable_declaration`](EmitContext::add_variable_declaration)),
/// matching Go's `AddVariableDeclaration` + `InlineExpressions` of
/// `[temp = class, static assignments..., temp]`.
///
/// Returns `None` (leaving the class expression unchanged) for shapes still
/// needing un-ported infra: any class expression whose lowering would hoist
/// **statements other than static assignments** (computed-name temp caches or
/// `var _C_x = new WeakMap();` private brands) — Go threads those through
/// `pendingExpressions`, which is not yet ported.
///
/// Side effects: may push rebuilt nodes; may hoist a `var` declaration.
// Go: internal/transformers/estransforms/classfields.go:visitClassExpressionInNewClassLexicalEnvironment
fn try_lower_class_expression(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let parts = lower_class_parts(ec.arena_mut(), node)?;
    if parts.pre_statements.is_empty() && parts.static_fields.is_empty() {
        // Instance-field-only (round 6o): the class expression is rebuilt in
        // place with a constructor; no statement hoisting is needed.
        return Some(parts.class);
    }
    if !parts.pre_statements.is_empty() {
        // Computed-name temp caches / private WeakMap brands in expression
        // position need Go's `pendingExpressions` threading (comma-sequence
        // initializers), which is not yet ported. Defer (leave unchanged).
        return None;
    }
    // Static field(s): `(_a = class {...}, _a.x = init, ..., _a)`. The temp is
    // reused (same node id) so the name generator materializes the same `_a` in
    // each position.
    let temp = ec.factory().new_temp_variable();
    ec.add_variable_declaration(temp);
    let equals = ec.arena_mut().new_token(Kind::EqualsToken);
    let mut sequence = ec
        .arena_mut()
        .new_binary_expression(temp, equals, parts.class);
    for (field_name, initializer) in parts.static_fields {
        let assignment = make_temp_static_assignment(ec.arena_mut(), temp, field_name, initializer);
        let comma = ec.arena_mut().new_token(Kind::CommaToken);
        sequence = ec
            .arena_mut()
            .new_binary_expression(sequence, comma, assignment);
    }
    let comma = ec.arena_mut().new_token(Kind::CommaToken);
    Some(ec.arena_mut().new_binary_expression(sequence, comma, temp))
}

/// Reports whether `member` is an **auto-accessor** property declaration
/// (`accessor x`): a `PropertyDeclaration` carrying the `accessor` modifier.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsAutoAccessorPropertyDeclaration
fn is_auto_accessor_property(arena: &NodeArena, member: NodeId) -> bool {
    matches!(arena.data(member), NodeData::PropertyDeclaration(d) if d
        .modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::ACCESSOR)))
}

/// Reports whether a class declares at least one auto-accessor member.
///
/// Side effects: none (reads the arena).
fn class_has_auto_accessor(arena: &NodeArena, node: NodeId) -> bool {
    let members = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => &d.members,
        _ => return false,
    };
    members
        .nodes
        .iter()
        .any(|&member| is_auto_accessor_property(arena, member))
}

/// Lowers a class whose members include auto-accessors (`accessor x = init`) to
/// the **ES2022-native** shape: each auto-accessor becomes a private backing
/// field plus a `get`/`set` redirector pair, e.g.
///
/// ```text
/// class C { accessor x = 1; }
/// ->
/// class C {
///     #x_accessor_storage = 1;
///     get x() { return this.#x_accessor_storage; }
///     set x(value) { this.#x_accessor_storage = value; }
/// }
/// ```
///
/// The backing-field name is allocated with the round-6p emit-context node-based
/// generated *private* name generator
/// ([`new_generated_private_name_for_node_ex`](tsgo_printer::factory::NodeFactory::new_generated_private_name_for_node_ex)
/// with the `_accessor_storage` suffix), keyed to the accessor's name node so the
/// field, getter, and setter all materialize the same `#x_accessor_storage` at
/// emit time.
///
/// Returns `None` (leaving the class unchanged for the deferred fuller port) for
/// any shape outside this slice: a **static** auto-accessor, a non-identifier
/// (computed / string / private) accessor name, a **decorated** accessor, or a
/// class that also contains other field/constructor members needing the WeakMap
/// / constructor-insertion lowering.
///
/// # Divergence from Go
/// Go re-runs a second-pass `accessorFieldResultVisitor` over the backing field
/// and redirectors, which (at downlevel targets) would lower the private backing
/// field to a `WeakMap` / `__classPrivateFieldGet`. This port lands only the
/// **native** form (`shouldTransformPrivateElements` off), so the backing field
/// stays a private class member; the WeakMap/static-block forms are DEFER'd.
///
/// Side effects: may push rebuilt nodes; allocates generated private names.
// Go: internal/transformers/estransforms/classfields.go:transformAutoAccessor
fn try_lower_auto_accessor_class(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let kind = ec.arena().kind(node);
    let (modifiers, name, type_parameters, heritage_clauses, members) = match ec.arena().data(node)
    {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => (
            d.modifiers.clone(),
            d.name,
            d.type_parameters.clone(),
            d.heritage_clauses.clone(),
            d.members.clone(),
        ),
        _ => return None,
    };

    let mut new_members = Vec::with_capacity(members.nodes.len() + 2);
    for &member in &members.nodes {
        if is_auto_accessor_property(ec.arena(), member) {
            let expanded = expand_auto_accessor(ec, member)?;
            new_members.extend(expanded);
        } else {
            // Mixing with other lowerable members (plain/static/private fields,
            // constructors) needs the constructor-insertion / WeakMap paths to
            // run together with the accessor expansion -> deferred. Members that
            // never need lowering (methods, regular accessors) are kept as-is.
            match ec.arena().kind(member) {
                Kind::PropertyDeclaration | Kind::Constructor => return None,
                _ => new_members.push(member),
            }
        }
    }

    Some(ec.arena_mut().new_class_like(
        kind,
        modifiers,
        name,
        type_parameters,
        heritage_clauses,
        NodeList::new(new_members),
    ))
}

/// Expands one auto-accessor property into `[backing field, getter, setter]`.
/// Returns `None` for shapes outside the simplest slice (static, non-identifier
/// name, or extra modifiers/decorators).
///
/// Side effects: pushes rebuilt nodes; allocates generated private names.
// Go: internal/transformers/estransforms/classfields.go:transformAutoAccessor
fn expand_auto_accessor(ec: &mut EmitContext, member: NodeId) -> Option<Vec<NodeId>> {
    let (modifiers, accessor_name, initializer) = match ec.arena().data(member) {
        NodeData::PropertyDeclaration(d) => (d.modifiers.clone(), d.name, d.initializer),
        _ => return None,
    };
    // Simplest reachable shape: an accessor whose only modifiers are `accessor`
    // and (optionally) `static` — no decorators, visibility, or `readonly` — and
    // a plain identifier name.
    let modifier_flags = modifiers
        .as_ref()
        .map(|m| m.modifier_flags)
        .unwrap_or_default();
    if !(modifier_flags & !(ModifierFlags::ACCESSOR | ModifierFlags::STATIC)).is_empty() {
        return None;
    }
    if ec.arena().kind(accessor_name) != Kind::Identifier {
        return None;
    }

    // Keep the surviving modifiers (i.e. `static`) on the backing field and both
    // redirectors; the `accessor` keyword is dropped. Go mirrors this with its
    // `visitModifier` (drops `accessor`, keeps real modifiers).
    let result_modifiers = strip_accessor_modifier(ec, modifiers.as_ref());

    let backing_field = {
        let backing_name = new_accessor_backing_name(ec, accessor_name);
        ec.arena_mut().new_property_declaration(
            result_modifiers.clone(),
            backing_name,
            None,
            None,
            initializer,
        )
    };
    let getter = build_accessor_get_redirector(ec, accessor_name, result_modifiers.clone());
    let setter = build_accessor_set_redirector(ec, accessor_name, result_modifiers);
    Some(vec![backing_field, getter, setter])
}

/// Returns the auto-accessor's modifiers with the `accessor` keyword removed,
/// keeping `static`. An empty result is normalized to `None` so a modifier-free
/// member emits no modifier list.
///
/// Side effects: none (reads node kinds; allocates a list value).
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.visitModifier
fn strip_accessor_modifier(
    ec: &EmitContext,
    modifiers: Option<&tsgo_ast::ModifierList>,
) -> Option<tsgo_ast::ModifierList> {
    let stripped = crate::extract_modifiers(ec, modifiers, ModifierFlags::STATIC)?;
    if stripped.list.nodes.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

/// Allocates the private backing-field name for an auto-accessor, derived from
/// the accessor's name node so every reference materializes the same
/// `#<name>_accessor_storage` text at emit time (round-6p generator).
///
/// Side effects: appends a node and records a node-based auto-generate entry.
// Go: internal/transformers/estransforms/classfields.go:createAccessorPropertyBackingField (name)
fn new_accessor_backing_name(ec: &mut EmitContext, accessor_name: NodeId) -> NodeId {
    ec.factory().new_generated_private_name_for_node_ex(
        accessor_name,
        AutoGenerateOptions {
            suffix: "_accessor_storage".to_string(),
            ..Default::default()
        },
    )
}

/// Builds `get <name>() { return this.#<name>_accessor_storage; }`.
///
/// Side effects: pushes rebuilt nodes; allocates a generated private name.
// Go: internal/transformers/estransforms/classfields.go:createAccessorPropertyGetRedirector
fn build_accessor_get_redirector(
    ec: &mut EmitContext,
    accessor_name: NodeId,
    modifiers: Option<tsgo_ast::ModifierList>,
) -> NodeId {
    let backing_name = new_accessor_backing_name(ec, accessor_name);
    let this = ec.arena_mut().new_keyword_expression(Kind::ThisKeyword);
    let access = ec
        .arena_mut()
        .new_property_access_expression(this, None, backing_name);
    let return_stmt = ec.arena_mut().new_return_statement(Some(access));
    let body = ec.arena_mut().new_block(NodeList::new(vec![return_stmt]));
    ec.arena_mut().new_accessor_declaration(
        Kind::GetAccessor,
        modifiers,
        accessor_name,
        None,
        NodeList::new(vec![]),
        None,
        None,
        Some(body),
    )
}

/// Builds `set <name>(value) { this.#<name>_accessor_storage = value; }`.
///
/// Side effects: pushes rebuilt nodes; allocates a generated private name.
// Go: internal/transformers/estransforms/classfields.go:createAccessorPropertySetRedirector
fn build_accessor_set_redirector(
    ec: &mut EmitContext,
    accessor_name: NodeId,
    modifiers: Option<tsgo_ast::ModifierList>,
) -> NodeId {
    let backing_name = new_accessor_backing_name(ec, accessor_name);
    let value_param_name = ec.arena_mut().new_identifier("value");
    let value_param =
        ec.arena_mut()
            .new_parameter_declaration(None, None, value_param_name, None, None, None);
    let this = ec.arena_mut().new_keyword_expression(Kind::ThisKeyword);
    let access = ec
        .arena_mut()
        .new_property_access_expression(this, None, backing_name);
    let value_ref = ec.arena_mut().new_identifier("value");
    let equals = ec.arena_mut().new_token(Kind::EqualsToken);
    let assignment = ec
        .arena_mut()
        .new_binary_expression(access, equals, value_ref);
    let stmt = ec.arena_mut().new_expression_statement(assignment);
    let body = ec.arena_mut().new_block(NodeList::new(vec![stmt]));
    ec.arena_mut().new_accessor_declaration(
        Kind::SetAccessor,
        modifiers,
        accessor_name,
        None,
        NodeList::new(vec![value_param]),
        None,
        None,
        Some(body),
    )
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

/// Builds a `<temp>.<name> = <initializer>` assignment *expression* (no trailing
/// statement) for a static field in the class-expression comma-sequence form,
/// where `<temp>` is the wrapper temp holding the class value (`_a.x = 1`).
///
/// Side effects: pushes the access/assignment nodes.
// Go: internal/transformers/estransforms/classfields.go:transformPropertyOrClassStaticBlock (static, temp receiver)
fn make_temp_static_assignment(
    arena: &mut NodeArena,
    temp: NodeId,
    field_name: NodeId,
    initializer: NodeId,
) -> NodeId {
    let target = arena.new_property_access_expression(temp, None, field_name);
    let equals = arena.new_token(Kind::EqualsToken);
    arena.new_binary_expression(target, equals, initializer)
}

/// A class-scoped private environment: maps each private field name (`#x`) to
/// the name of the module-scope `WeakMap` brand variable backing it (`_C_x`).
///
/// # Divergence from Go
/// Go's classfields transform stores per-name `PrivateIdentifierInfo` and emits
/// the `__classPrivateFieldGet/Set` named-helper-import form with a
/// name-generator-allocated brand. This port lands the equivalent **direct**
/// `WeakMap.get/.set` form with a deterministic `_<Class>_<field>` brand name;
/// the named-helper-import form and name-generator uniqueness are DEFER'd.
// Go: internal/transformers/estransforms/classfields.go:PrivateEnvironment (direct-WeakMap subset)
type PrivateEnv = Vec<(String, String)>;

/// Reports whether `name` is a private identifier (`#x`). The parser models a
/// private *declaration* name as `PrivateIdentifier` and a private *access*
/// name as an `Identifier`, but both carry the leading `#`.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsPrivateIdentifier
fn is_private_name(arena: &NodeArena, name: NodeId) -> bool {
    matches!(arena.kind(name), Kind::Identifier | Kind::PrivateIdentifier)
        && arena.text(name).starts_with('#')
}

/// Looks up the WeakMap brand variable name for a private field name in `env`.
///
/// Side effects: none.
fn private_env_brand<'a>(env: &'a PrivateEnv, private_text: &str) -> Option<&'a str> {
    env.iter()
        .find(|(p, _)| p == private_text)
        .map(|(_, b)| b.as_str())
}

/// Returns the (possibly private) name of a method/accessor member, if any.
///
/// Side effects: none (reads the arena).
fn method_like_name(arena: &NodeArena, member: NodeId) -> Option<NodeId> {
    match arena.data(member) {
        NodeData::MethodDeclaration(d) => Some(d.name),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => Some(d.name),
        _ => None,
    }
}

/// Builds the class-scoped private environment by scanning members for private
/// field declarations. Returns `None` (deferring the whole class) when the
/// class uses private *methods*/*accessors* (WeakSet brand checks, not yet
/// ported) or is anonymous while declaring private members (the brand name
/// needs the class name).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.startClassLexicalEnvironment
fn build_private_env(
    arena: &NodeArena,
    members: &NodeList,
    class_name: Option<NodeId>,
) -> Option<PrivateEnv> {
    let mut env = PrivateEnv::new();
    for &member in &members.nodes {
        let name = match arena.kind(member) {
            Kind::PropertyDeclaration => match arena.data(member) {
                NodeData::PropertyDeclaration(d) => d.name,
                _ => continue,
            },
            Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor => {
                if method_like_name(arena, member).is_some_and(|n| is_private_name(arena, n)) {
                    // Private methods/accessors -> WeakSet brand check -> deferred.
                    return None;
                }
                continue;
            }
            _ => continue,
        };
        if is_private_name(arena, name) {
            let class_name = class_name?;
            let class_text = arena.text(class_name).to_string();
            let private_text = arena.text(name).to_string();
            // `#x` -> `_C_x` (strip the leading `#`).
            let brand = format!("_{}_{}", class_text, &private_text[1..]);
            env.push((private_text, brand));
        }
    }
    Some(env)
}

/// Builds a `var <brand> = new WeakMap();` statement for a private field brand.
///
/// Side effects: pushes the identifier/new/declaration/statement nodes.
// Go: internal/transformers/estransforms/classfields.go:createPrivateInstanceFieldInitializer (WeakMap brand)
fn make_weakmap_brand_var(arena: &mut NodeArena, brand: &str) -> NodeId {
    let brand_name = arena.new_identifier(brand);
    let weak_map = arena.new_identifier("WeakMap");
    let new_weak_map = arena.new_new_expression(weak_map, None, Some(NodeList::new(vec![])));
    let declaration = arena.new_variable_declaration(brand_name, None, None, Some(new_weak_map));
    let declaration_list = arena.new_variable_declaration_list(NodeList::new(vec![declaration]));
    arena.new_variable_statement(None, declaration_list)
}

/// Builds a `<brand>.set(this, <value>);` statement for a private field
/// initializer (the direct-WeakMap form of the field assignment).
///
/// Side effects: pushes the access/call/statement nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:transformPrivateFieldInitializer (direct .set form)
fn make_private_set(arena: &mut NodeArena, brand: &str, value: NodeId) -> NodeId {
    let this = arena.new_keyword_expression(Kind::ThisKeyword);
    let call = make_private_set_call(arena, brand, this, value);
    arena.new_expression_statement(call)
}

/// Builds a `<brand>.set(<receiver>, <value>)` call expression for a private
/// field store.
///
/// Side effects: pushes the access/call nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:createPrivateIdentifierAssignment (direct .set form)
fn make_private_set_call(
    arena: &mut NodeArena,
    brand: &str,
    receiver: NodeId,
    value: NodeId,
) -> NodeId {
    let brand_name = arena.new_identifier(brand);
    let set = arena.new_identifier("set");
    let callee = arena.new_property_access_expression(brand_name, None, set);
    arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![receiver, value]),
        NodeFlags::NONE,
    )
}

/// Recursively rewrites private field *reads* `obj.#x` into `<brand>.get(obj)`
/// within the subtree rooted at `node`, using the class-scoped `env`. Nodes that
/// are not private accesses are rebuilt structurally (their children visited).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:classFieldsTransformer.visitPropertyAccessExpression
fn lower_private_access(arena: &mut NodeArena, node: NodeId, env: &PrivateEnv) -> NodeId {
    // `obj.#x = e` -> `<brand>.set(obj, e)` (handle the assignment before the
    // bare read so the left-hand private access is not lowered to a `.get`).
    if arena.kind(node) == Kind::BinaryExpression {
        let (left, op, right) = match arena.data(node) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            _ => unreachable!("kind checked above"),
        };
        if arena.kind(op) == Kind::EqualsToken && arena.kind(left) == Kind::PropertyAccessExpression
        {
            let (receiver, name) = match arena.data(left) {
                NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
                _ => unreachable!("kind checked above"),
            };
            if is_private_name(arena, name) {
                if let Some(brand) = private_env_brand(env, arena.text(name)) {
                    let brand = brand.to_string();
                    let receiver = lower_private_access(arena, receiver, env);
                    let right = lower_private_access(arena, right, env);
                    return make_private_set_call(arena, &brand, receiver, right);
                }
            }
        }
    }
    if arena.kind(node) == Kind::PropertyAccessExpression {
        let (receiver, name) = match arena.data(node) {
            NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
            _ => unreachable!("kind checked above"),
        };
        if is_private_name(arena, name) {
            if let Some(brand) = private_env_brand(env, arena.text(name)) {
                let brand = brand.to_string();
                let receiver = lower_private_access(arena, receiver, env);
                return make_private_get(arena, &brand, receiver);
            }
        }
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| lower_private_access(a, c, env))
}

/// Builds a `<brand>.get(<receiver>)` call expression for a private read.
///
/// Side effects: pushes the access/call nodes onto the arena.
// Go: internal/transformers/estransforms/classfields.go:createPrivateIdentifierAccess (direct .get form)
fn make_private_get(arena: &mut NodeArena, brand: &str, receiver: NodeId) -> NodeId {
    let brand_name = arena.new_identifier(brand);
    let get = arena.new_identifier("get");
    let callee = arena.new_property_access_expression(brand_name, None, get);
    arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![receiver]),
        NodeFlags::NONE,
    )
}

/// Returns the temp-variable name for the `index`-th computed property name in a
/// class (`_a`, `_b`, ...).
///
/// # Divergence from Go
/// Go allocates a collision-free temp via the emit-context name generator
/// (`NewTempVariable`/`NewGeneratedNameForNode`). This port uses a deterministic
/// `_a`/`_b`/... scheme; name-generator-backed uniqueness is DEFER'd.
///
/// Side effects: none.
fn computed_temp_name(index: usize) -> String {
    if index < 26 {
        format!("_{}", (b'a' + index as u8) as char)
    } else {
        format!("_a{index}")
    }
}

/// Builds a `var <name> = <initializer>;` statement.
///
/// Side effects: pushes the identifier/declaration/statement nodes.
fn make_temp_var(arena: &mut NodeArena, name: &str, initializer: NodeId) -> NodeId {
    let temp = arena.new_identifier(name);
    let declaration = arena.new_variable_declaration(temp, None, None, Some(initializer));
    let declaration_list = arena.new_variable_declaration_list(NodeList::new(vec![declaration]));
    arena.new_variable_statement(None, declaration_list)
}

/// Builds a `this[<name>] = <initializer>;` statement for a computed instance
/// field whose key has been cached in the temp variable `name`.
///
/// Side effects: pushes the access/assignment/statement nodes onto the arena.
fn make_this_element_assignment(arena: &mut NodeArena, name: &str, initializer: NodeId) -> NodeId {
    let this = arena.new_keyword_expression(Kind::ThisKeyword);
    let key = arena.new_identifier(name);
    let target = arena.new_element_access_expression(this, None, key);
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
