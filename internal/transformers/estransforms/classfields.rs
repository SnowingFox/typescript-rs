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
//! Deferred (DEFER(P5), see `estransforms/mod.rs`): the **named-helper-import**
//! private form (`__classPrivateFieldGet/Set`), **private static fields**,
//! **private methods/accessors** (`WeakSet` brand checks), **`accessor` fields**
//! (auto-accessor → backing field + get/set redirectors; needs the name
//! generator + a second-pass result visitor), **class expressions** (statement
//! hoisting needs IIFE/comma-sequence wrapping), **parameter properties**
//! (a TS-transform concern), constructor **prologue directives**,
//! **anonymous-class** static/private members (need a generated class name),
//! name-generator-backed **temp/brand uniqueness**, and
//! `--target`/`useDefineForClassFields` gating — these need helper-library emit,
//! the emit-context name generator, and/or checker info not yet ported.

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

    // Build the class-scoped private environment (private field name -> WeakMap
    // brand variable name). Returns `None` (leaving the class for the deferred
    // fuller port) when the class uses private *methods*/*accessors* (WeakSet
    // brand checks, not yet ported) or is anonymous with private members.
    let private_env = build_private_env(arena, &members, name)?;

    let mut field_assignments = Vec::new();
    let mut static_assignments = Vec::new();
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
        && static_assignments.is_empty()
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
        Kind::ClassDeclaration,
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
    if pre_statements.is_empty() && static_assignments.is_empty() {
        Some(class)
    } else {
        // Emit hoisted statements before, and `C.x = ...` assignments after, the
        // class via a SyntaxList of statements.
        let mut statements =
            Vec::with_capacity(pre_statements.len() + 1 + static_assignments.len());
        statements.extend(pre_statements);
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
