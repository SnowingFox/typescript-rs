//! Port of Go `internal/transformers/tstransforms/legacydecorators.go` (plus the
//! `metadata.go` / `typeserializer.go` metadata injection it consumes): the
//! legacy (`--experimentalDecorators`) decorator transform.
//!
//! # Scope (round 6al — first slice)
//!
//! Lowers a **decorated class member** (the reachable subset: an instance or
//! static *property* with decorators) into a trailing `__decorate(...)` call,
//! matching Go/`tsc --experimentalDecorators` output:
//!
//! ```text
//! class C { @dec x: number; }
//! =>
//! class C {
//!     x;
//! }
//! __decorate([dec], C.prototype, "x", void 0);
//! ```
//!
//! When `--emitDecoratorMetadata` is also set, a `design:type` metadata
//! decorator is appended to the decorator array, serialized from the property's
//! type annotation via the checker (4at's
//! [`serialize_type_node_for_metadata`](crate::EmitReferenceResolver::serialize_type_node_for_metadata)):
//!
//! ```text
//! class C { @dec x: number; }   // + emitDecoratorMetadata
//! =>
//! __decorate([dec, __metadata("design:type", Number)], C.prototype, "x", void 0);
//! ```
//!
//! # Pipeline fold (divergence from Go's two transformers)
//!
//! Go runs two transformers in sequence: `MetadataTransformer` injects a
//! synthetic `@__metadata("design:type", T)` decorator into a decorated member's
//! modifier list, then `LegacyDecoratorsTransformer` collects all decorators
//! (real + injected) into the `__decorate([...])` array. This port folds both
//! into one pass: [`generate_class_element_decoration_expression`] builds the
//! decorator-expression list directly as `[<real decorators>, <metadata>]`
//! (metadata last, exactly as Go's `transformAllDecoratorsOfDeclaration` orders
//! them). The emitted text is identical.
//!
//! # Class declarations with class decorators (round 6ar)
//!
//! A class that is itself decorated (a class decorator, or a decorated
//! constructor parameter — Go's `ClassOrConstructorParameterIsDecorated`) is
//! lowered into a `let`-bound class expression followed by a trailing
//! `__decorate` assignment (Go's `transformClassDeclarationWithClassDecorators`):
//!
//! ```text
//! @dec class C {}
//! =>
//! let C = class C {
//! };
//! C = __decorate([dec], C);
//! ```
//!
//! Constructor parameter decorators contribute `__param(i, dec)` entries to the
//! CLASS `__decorate` array (Go's `getConstructorDecorationStatement` ->
//! `getAllDecoratorsOfClass` -> `getDecoratorsOfParameters(constructor)`), and
//! with `--emitDecoratorMetadata` a `design:paramtypes` metadata for the
//! constructor parameters is appended last (Go's `shouldAddParamTypesMetadata`
//! is true for a class with a constructor body; `design:type` /
//! `design:returntype` are member-only):
//!
//! ```text
//! class C { constructor(@pdec a: number) {} }
//! =>
//! let C = class C {
//!     constructor(a) { }
//! };
//! C = __decorate([__param(0, pdec)], C);
//! ```
//!
//! # Deferred (DEFER(P5))
//!
//! Deferred, each with its blocker:
//!
//! - **The self-reference class-alias rewrite** (`let C = C_1 = class C {…}`,
//!   rewriting `C` references inside the class body to `C_1`). The reachable
//!   subset has no self-reference. blocked-by: `getReferencedValueDeclaration` +
//!   `NewUniqueName` / the `classAliases` substitution + per-node
//!   `ConstructorReference` flags.
//! - **`export` / `export default` decorated classes** (the trailing
//!   `export { C };` / `export default C;` statement and the `default_1` rename
//!   for an anonymous default export). blocked-by: the export/default modifier
//!   handling + `GetLocalName`/generated-name emit-name forms.
//! - **Accessor decorators** landed in 6aq: a decorated get/set accessor lowers
//!   to `__decorate([...], C.prototype, "x", null)` (4th arg `null`, like a
//!   method); a get/set pair merges into a single `__decorate` owned by the
//!   first accessor with decorators (`getAllAccessorDeclarations` +
//!   `getAllDecoratorsOfAccessors`); with `--emitDecoratorMetadata` an accessor
//!   emits `design:type` (from `getAccessorTypeNode`: the setter's value-param
//!   type if a setter exists, else the getter's return type) *and*
//!   `design:paramtypes` (`shouldAddParamTypesMetadata` is true for accessors),
//!   but NOT `design:returntype` (method-only). Still deferred for accessors:
//!   computed/private accessor names (need `getPropertyNameForPropertyNameNode` /
//!   checker symbol pairing); parameter decorators on a setter
//!   (`getDecoratorsOfParameters(setAccessor)` — `member_is_decorated` only sees
//!   the accessor's own decorator); the `this`-parameter body erasure
//!   (`set x(this: T, v)` keeps `this` in the lowered body); and the
//!   both-get-and-set-decorated case (a grammar error in valid TS). blocked-by:
//!   computed-name resolution + accessor parameter-decorator detection.
//!   (Method decorators landed in 6ao: `design:type` = `Function` +
//!   `design:returntype`.)
//! - **`this`-parameter offset + rest-parameter element type** for the 6ap
//!   `design:paramtypes` / `__param` paths. blocked-by: `IsThisParameter` skip +
//!   `GetRestParameterElementType`. (The reachable subset — plain leading
//!   parameters — emits the `design:paramtypes` array and `__param(i, dec)`
//!   entries; see `serialize_parameter_types` / `append_param_decorators`.)
//!   (Constructor parameter decorators landed in 6ar: `__param(i, dec)` in the
//!   CLASS `__decorate` array via `getConstructorDecorationStatement` /
//!   `getAllDecoratorsOfClass`.)
//! - **`TypeReference` `design:type`** for lib-globals/qualified-name entities.
//!   Round 4ax/6an wires the reachable single-file `TypeReference` dispatch
//!   (consuming checker 4aw's `get_type_reference_serialization_kind`): a
//!   class-typed reference (`: C`) emits the class identifier `C`, an
//!   interface/type-alias reference emits `Object`, and an unresolved name emits
//!   `Object`. Still deferred: the lib-globals kinds the checker classifies as
//!   `ObjectType` (`Promise`/array/primitive), qualified-name (`A.B`) entities,
//!   and the full `Unknown` `typeof`-conditional guard. blocked-by: checker lib
//!   global types + qualified-name `resolveEntityName` + `NewTempVariable`.
//! - **Computed property names**, decorator expression evaluation order edge
//!   cases, and decorators on overloads. blocked-by: `pendingExpressions`
//!   inlining + per-node `ConstructorReference` flags.

use crate::{new_transformer, EmitReferenceResolver, TransformOptions, Transformer};
use tsgo_ast::utilities::modifier_to_flag;
use tsgo_ast::{
    Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeFlags, NodeId, NodeList,
    TokenFlags, VisitOptions,
};
use tsgo_checker::{SerializedTypeNode, TypeReferenceSerializationKind};
use tsgo_printer::emithelpers::EmitHelper;
use tsgo_printer::EmitContext;

/// TypeScript `__decorate` — applies a list of decorators to a target member.
/// Defined here (not in `tsgo_printer`) because the printer crate is out of this
/// round's edit scope, mirroring `estransforms/spread.rs`. Text and priority are
/// verbatim from Go `internal/printer/helpers.go:decorateHelper`.
// Go: internal/printer/helpers.go:decorateHelper
pub static DECORATE_HELPER: EmitHelper = EmitHelper {
    name: "typescript:decorate",
    import_name: "__decorate",
    scoped: false,
    priority: Some(2),
    dependencies: &[],
    text: r#"var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};"#,
};

/// TypeScript `__metadata` — emits a `Reflect.metadata(k, v)` decorator for
/// `design:*` reflection metadata. Text and priority verbatim from Go
/// `internal/printer/helpers.go:metadataHelper`.
// Go: internal/printer/helpers.go:metadataHelper
pub static METADATA_HELPER: EmitHelper = EmitHelper {
    name: "typescript:metadata",
    import_name: "__metadata",
    scoped: false,
    priority: Some(3),
    dependencies: &[],
    text: r#"var __metadata = (this && this.__metadata) || function (k, v) {
    if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(k, v);
};"#,
};

/// TypeScript `__param` — adapts a parameter decorator into a member decorator
/// that forwards the parameter index. Emitted for each decorated parameter as
/// `__param(index, decorator)`. Text and priority (4) verbatim from Go
/// `internal/printer/helpers.go:paramHelper`.
// Go: internal/printer/helpers.go:paramHelper
pub static PARAM_HELPER: EmitHelper = EmitHelper {
    name: "typescript:param",
    import_name: "__param",
    scoped: false,
    priority: Some(4),
    dependencies: &[],
    text: r#"var __param = (this && this.__param) || function (paramIndex, decorator) {
    return function (target, key) { decorator(target, key, paramIndex); }
};"#,
};

/// Per-run configuration captured from the [`TransformOptions`] plus the
/// (optional) reference resolver the metadata serialization needs.
#[derive(Clone)]
struct Config {
    /// Whether the legacy (`--experimentalDecorators`) transform is enabled.
    experimental_decorators: bool,
    /// Whether `design:*` metadata should be emitted (`--emitDecoratorMetadata`).
    emit_decorator_metadata: bool,
    /// The checker's reference query, required to serialize `design:type`
    /// metadata from a property's type annotation. `None` when constructed via
    /// the resolver-free factory (the metadata path is then inactive).
    resolver: Option<EmitReferenceResolver>,
}

/// Builds a [`Transformer`] that lowers legacy decorators, sharing the
/// pipeline's emit context. The metadata path is inactive (no resolver), so this
/// factory suits `--experimentalDecorators` without `--emitDecoratorMetadata`.
///
/// # Examples
/// ```
/// use tsgo_transformers::{tstransforms::legacydecorators::new_legacy_decorators_transformer, TransformOptions};
/// let _tx = new_legacy_decorators_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/tstransforms/legacydecorators.go:NewLegacyDecoratorsTransformer
pub fn new_legacy_decorators_transformer(opt: &TransformOptions) -> Transformer {
    build(opt, None)
}

/// Like [`new_legacy_decorators_transformer`] but threads `resolver` so
/// `design:type` metadata can be serialized from type annotations when
/// `--emitDecoratorMetadata` is set. The resolver is an *additive* parameter
/// (not a [`TransformOptions`] field); see [`EmitReferenceResolver`] for why.
///
/// # Examples
/// ```
/// use tsgo_transformers::{
///     tstransforms::legacydecorators::new_legacy_decorators_transformer_with_resolver,
///     EmitReferenceResolver, TransformOptions,
/// };
/// # fn demo(resolver: EmitReferenceResolver) {
/// let _tx = new_legacy_decorators_transformer_with_resolver(&TransformOptions::default(), resolver);
/// # }
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/tstransforms/legacydecorators.go:NewLegacyDecoratorsTransformer (+ metadata.go)
pub fn new_legacy_decorators_transformer_with_resolver(
    opt: &TransformOptions,
    resolver: EmitReferenceResolver,
) -> Transformer {
    build(opt, Some(resolver))
}

/// Shared factory body for both public constructors.
fn build(opt: &TransformOptions, resolver: Option<EmitReferenceResolver>) -> Transformer {
    let cfg = Config {
        experimental_decorators: opt.compiler_options.experimental_decorators.is_true(),
        emit_decorator_metadata: opt.compiler_options.emit_decorator_metadata.is_true(),
        resolver,
    };
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| legacy_decorators_visit(ec, node, &cfg)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit. The source-file boundary attaches the helpers
/// requested during the visit; class declarations are lowered when the legacy
/// transform is enabled; everything else recurses.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
// Go: internal/transformers/tstransforms/legacydecorators.go:LegacyDecoratorsTransformer.visit
fn legacy_decorators_visit(ec: &mut EmitContext, node: NodeId, cfg: &Config) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node, cfg),
        Kind::ClassDeclaration if cfg.experimental_decorators => {
            visit_class_declaration(ec, node, cfg)
        }
        _ => visit_each_child_ec(ec, node, cfg),
    }
}

/// Visits the source file's statements, then attaches the helpers requested
/// during the visit so the printer emits them in the prologue.
///
/// Side effects: rebuilds the source file; attaches emit helpers.
fn visit_source_file(ec: &mut EmitContext, node: NodeId, cfg: &Config) -> NodeId {
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
        .map(|s| legacy_decorators_visit(ec, s, cfg))
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

/// Emit-context-threaded `VisitEachChild`: recursively visits each child, then
/// rebuilds the node with the transformed children (unchanged when nothing
/// changed).
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId, cfg: &Config) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: Vec<(NodeId, NodeId)> = Vec::new();
    for child in children {
        let transformed = legacy_decorators_visit(ec, child, cfg);
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

/// Lowers a class declaration with decorated members into the class (with
/// member decorators stripped) followed by the trailing `__decorate(...)`
/// statements, returned as a `SyntaxList` (Go's
/// `transformClassDeclarationWithoutClassDecorators`).
///
/// Reachable subset: a *property* decorator on an instance member. A class
/// decorator (`@dec class C {}`) and any non-property member decorator fall
/// through to [`visit_each_child_ec`] unchanged (DEFER, see module docs).
///
/// Side effects: pushes rebuilt nodes; may request/attach emit helpers.
// Go: internal/transformers/tstransforms/legacydecorators.go:visitClassDeclaration
fn visit_class_declaration(ec: &mut EmitContext, node: NodeId, cfg: &Config) -> NodeId {
    let (modifiers, name, heritage_clauses, members) = match ec.arena().data(node) {
        NodeData::ClassDeclaration(d) => (
            d.modifiers.clone(),
            d.name,
            d.heritage_clauses.clone(),
            d.members.clone(),
        ),
        _ => unreachable!("kind/data mismatch"),
    };

    // Go's `visitClassDeclaration`: a class that is itself decorated (a class
    // decorator on the class node, or a decorated constructor parameter —
    // `ClassOrConstructorParameterIsDecorated`) is lowered through
    // `transformClassDeclarationWithClassDecorators` — the
    // `let C = class C {…}; C = __decorate([…], C);` wrapping. A class with only
    // decorated *members* takes the trailing-`__decorate` member path
    // (`transformClassDeclarationWithoutClassDecorators`); an undecorated class
    // recurses unchanged.
    if class_or_constructor_parameter_is_decorated(ec.arena(), node) {
        return transform_class_declaration_with_class_decorators(
            ec,
            cfg,
            node,
            modifiers,
            name,
            heritage_clauses,
            members,
        );
    }

    let decorated_members: Vec<NodeId> = members
        .nodes
        .iter()
        .copied()
        .filter(|&m| member_is_decorated(ec.arena(), m))
        .collect();
    if decorated_members.is_empty() {
        // No member or parameter decorators in the reachable subset; recurse
        // unchanged.
        return visit_each_child_ec(ec, node, cfg);
    }

    let name_text = name
        .map(|n| ec.arena().text(n).to_string())
        .unwrap_or_default();

    // Rebuild members, stripping decorators (and the type annotation) from each
    // decorated member (Go's per-kind `visit*Declaration`).
    let new_members: Vec<NodeId> = members
        .nodes
        .iter()
        .copied()
        .map(|m| rebuild_class_member(ec, m))
        .collect();
    let updated_class = ec.arena_mut().new_class_like(
        Kind::ClassDeclaration,
        modifiers,
        name,
        None,
        heritage_clauses,
        NodeList::new(new_members),
    );

    // Build a `__decorate(...)` statement per decorated member. The *original*
    // member ids (with type annotations intact) are threaded through so accessor
    // metadata can pair a get/set accessor (`getAllAccessorDeclarations`) and
    // read the un-stripped setter parameter / getter return type.
    let member_ids: Vec<NodeId> = members.nodes.clone();
    let mut statements = vec![updated_class];
    for member in decorated_members {
        if let Some(stmt) =
            generate_class_element_decoration_statement(ec, cfg, &name_text, member, &member_ids)
        {
            statements.push(stmt);
        }
    }

    ec.arena_mut().new_syntax_list(NodeList::new(statements))
}

/// Lowers a class declaration that is itself decorated (a class decorator, or —
/// after round 6ar slice 3 — a decorated constructor parameter) into the
/// `let <name> = class <name> {…};` binding followed by the member-decoration
/// statements and the trailing `<name> = __decorate([…], <name>);` assignment,
/// returned as a `SyntaxList` (Go's
/// `transformClassDeclarationWithClassDecorators`).
///
/// Reachable subset: a named, non-`export`/`default` class. DEFER (each with its
/// blocker, see module docs): the self-reference class-alias rewrite
/// (`let C = C_1 = class C {…}`) and the `export` / `export default` trailing
/// statements.
///
/// Side effects: pushes the rebuilt class expression / `let` statement /
/// `__decorate` assignment nodes; requests the `__decorate` (and, with metadata,
/// `__metadata`/`__param`) helpers.
// Go: internal/transformers/tstransforms/legacydecorators.go:transformClassDeclarationWithClassDecorators
fn transform_class_declaration_with_class_decorators(
    ec: &mut EmitContext,
    cfg: &Config,
    node: NodeId,
    modifiers: Option<ModifierList>,
    name: Option<NodeId>,
    heritage_clauses: Option<NodeList>,
    members: NodeList,
) -> NodeId {
    // DEFER: the `export` / `export default` trailing statements and the
    // self-reference class-alias rewrite. The reachable subset has no
    // `export`/`default`, so stripping the class decorator is all that is needed
    // (Go filters out export/default/decorator via `isNotExportOrDefaultOrDecorator`).
    let class_modifiers = strip_decorators(ec.arena(), modifiers.as_ref());

    let name_text = name
        .map(|n| ec.arena().text(n).to_string())
        .unwrap_or_default();

    // Rebuild every member (Go's `VisitNodes(members)`), stripping member
    // decorators / type annotations (and, for a constructor, its parameters'
    // decorators / type annotations).
    let member_ids: Vec<NodeId> = members.nodes.clone();
    let new_members: Vec<NodeId> = members
        .nodes
        .iter()
        .copied()
        .map(|m| rebuild_class_member(ec, m))
        .collect();

    // `... = class C { <members> }` — the class expression keeps the class name
    // (Go's `exprName = name` for a non-generated name).
    let class_expression = ec.arena_mut().new_class_like(
        Kind::ClassExpression,
        class_modifiers,
        name,
        None,
        heritage_clauses,
        NodeList::new(new_members),
    );

    // `let C = class C { … };` — Go's `declName = GetLocalName(node)` binding,
    // emitted with the `let` block-scope flag.
    let decl_name = ec.arena_mut().new_identifier(&name_text);
    let var_declaration =
        ec.arena_mut()
            .new_variable_declaration(decl_name, None, None, Some(class_expression));
    let var_declaration_list = ec
        .arena_mut()
        .new_variable_declaration_list(NodeList::new(vec![var_declaration]));
    ec.arena_mut()
        .add_flags(var_declaration_list, NodeFlags::LET);
    let var_statement = ec
        .arena_mut()
        .new_variable_statement(None, var_declaration_list);

    let mut statements = vec![var_statement];

    // Member decoration statements, in declaration order (Go runs instance then
    // static; single-member tests are insensitive to that ordering).
    for member in members.nodes.iter().copied() {
        if member_is_decorated(ec.arena(), member) {
            if let Some(stmt) = generate_class_element_decoration_statement(
                ec,
                cfg,
                &name_text,
                member,
                &member_ids,
            ) {
                statements.push(stmt);
            }
        }
    }

    // Trailing class-level `C = __decorate([…], C);` (Go's
    // `getConstructorDecorationStatement`).
    if let Some(stmt) = get_constructor_decoration_statement(ec, cfg, &name_text, node) {
        statements.push(stmt);
    }

    if statements.len() == 1 {
        return statements[0];
    }
    ec.arena_mut().new_syntax_list(NodeList::new(statements))
}

/// Rebuilds a class member with its decorators (and type annotations) stripped,
/// dispatching per kind (Go's per-kind `visit*Declaration` reached via
/// `VisitNodes(members)`). Undecorated property/method members pass through
/// unchanged in this reachable subset; accessors are always rebuilt so an
/// undecorated `set x(value: number)` partner lowers to `set x(value)`.
///
/// Side effects: may push the rebuilt member node.
fn rebuild_class_member(ec: &mut EmitContext, member: NodeId) -> NodeId {
    match ec.arena().kind(member) {
        Kind::PropertyDeclaration if member_is_decorated(ec.arena(), member) => {
            rebuild_property_without_decorators(ec.arena_mut(), member)
        }
        Kind::MethodDeclaration if member_is_decorated(ec.arena(), member) => {
            rebuild_method_without_decorators(ec.arena_mut(), member)
        }
        Kind::GetAccessor | Kind::SetAccessor => {
            rebuild_accessor_without_decorators(ec.arena_mut(), member)
        }
        // Go's `visitConstructorDeclaration` strips the constructor's parameter
        // decorators / type annotations (its parameters are visited via
        // `visitParamerDeclaration`); the constructor itself can carry no
        // decorator. Reached when the class is lowered through the class-decorator
        // path (a class decorator or a decorated constructor parameter).
        Kind::Constructor => rebuild_constructor_without_decorators(ec.arena_mut(), member),
        _ => member,
    }
}

/// Rebuilds a constructor declaration with its type parameters / return type /
/// full signature dropped and each parameter rebuilt (stripping parameter
/// decorators and type annotations), keeping the modifiers and body (Go's
/// `LegacyDecoratorsTransformer.visitConstructorDeclaration`).
///
/// Side effects: pushes the rebuilt constructor node.
// Go: internal/transformers/tstransforms/legacydecorators.go:visitConstructorDeclaration
fn rebuild_constructor_without_decorators(arena: &mut NodeArena, member: NodeId) -> NodeId {
    let (modifiers, parameters, body) = match arena.data(member) {
        NodeData::ConstructorDeclaration(d) => (d.modifiers.clone(), d.parameters.clone(), d.body),
        _ => return member,
    };
    let new_parameters: Vec<NodeId> = parameters
        .nodes
        .iter()
        .copied()
        .map(|p| rebuild_parameter_without_decorators(arena, p))
        .collect();
    arena.new_constructor_declaration(
        modifiers,
        None,
        NodeList::new(new_parameters),
        None,
        None,
        body,
    )
}

/// Reports whether the class node itself carries a decorator (Go's
/// `HasDecorators` on the class — the `decorated` arm of `visitClassDeclaration`
/// in its reachable subset).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:HasDecorators
fn class_has_decorator(arena: &NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ClassDeclaration(d) => d
            .modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::DECORATOR)),
        _ => false,
    }
}

/// Reports whether the class is itself decorated: it carries a class decorator,
/// or its first constructor with a body has at least one decorated parameter
/// (Go's `ClassOrConstructorParameterIsDecorated`, reachable subset). A class
/// matching this takes the `let C = class C {…}; C = __decorate([…], C);` path.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:ClassOrConstructorParameterIsDecorated
fn class_or_constructor_parameter_is_decorated(arena: &NodeArena, node: NodeId) -> bool {
    class_has_decorator(arena, node) || constructor_has_decorated_parameter(arena, node)
}

/// Returns the first constructor member of the class that has a body (Go's
/// `GetFirstConstructorWithBody`).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:GetFirstConstructorWithBody
fn first_constructor_with_body(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    let members = match arena.data(node) {
        NodeData::ClassDeclaration(d) => &d.members,
        _ => return None,
    };
    members.nodes.iter().copied().find(|&m| {
        arena.kind(m) == Kind::Constructor
            && matches!(arena.data(m), NodeData::ConstructorDeclaration(d) if d.body.is_some())
    })
}

/// Reports whether the class's first constructor with a body has at least one
/// decorated parameter (Go's `ChildIsDecorated` constructor arm reached from
/// `ClassOrConstructorParameterIsDecorated`).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:ChildIsDecorated (KindConstructor arm)
fn constructor_has_decorated_parameter(arena: &NodeArena, node: NodeId) -> bool {
    let Some(ctor) = first_constructor_with_body(arena, node) else {
        return false;
    };
    let parameters = match arena.data(ctor) {
        NodeData::ConstructorDeclaration(d) => &d.parameters,
        _ => return false,
    };
    parameters
        .nodes
        .iter()
        .copied()
        .any(|p| parameter_has_decorators(arena, p))
}

/// Collects the (real) class decorator expressions, in source order (the
/// `decorators` half of Go's `getAllDecoratorsOfClass`).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/legacydecorators.go:getAllDecoratorsOfClass
fn class_decorator_expressions(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    let modifiers = match arena.data(node) {
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    let Some(modifiers) = modifiers else {
        return Vec::new();
    };
    modifiers
        .list
        .nodes
        .iter()
        .copied()
        .filter(|&n| arena.kind(n) == Kind::Decorator)
        .map(|n| match arena.data(n) {
            NodeData::Decorator(d) => d.expression,
            _ => unreachable!("kind checked above"),
        })
        .collect()
}

/// Wraps [`generate_constructor_decoration_expression`] in an expression
/// statement (Go's `getConstructorDecorationStatement`). Returns `None` when the
/// class has no class decorators or decorated constructor parameters.
///
/// Side effects: pushes the statement / assignment nodes; requests helpers.
// Go: internal/transformers/tstransforms/legacydecorators.go:getConstructorDecorationStatement
fn get_constructor_decoration_statement(
    ec: &mut EmitContext,
    cfg: &Config,
    class_name: &str,
    node: NodeId,
) -> Option<NodeId> {
    let expression = generate_constructor_decoration_expression(ec, cfg, class_name, node)?;
    Some(ec.arena_mut().new_expression_statement(expression))
}

/// Builds the `C = __decorate([<class decorators>], C)` assignment for the class
/// constructor (Go's `generateConstructorDecorationExpression`). Returns `None`
/// when there are no class decorators (and — slice 3 — no decorated constructor
/// parameters) to apply.
///
/// Side effects: pushes the `__decorate` / assignment nodes; requests the
/// `__decorate` helper.
// Go: internal/transformers/tstransforms/legacydecorators.go:generateConstructorDecorationExpression
fn generate_constructor_decoration_expression(
    ec: &mut EmitContext,
    cfg: &Config,
    class_name: &str,
    node: NodeId,
) -> Option<NodeId> {
    let mut decorator_expressions = class_decorator_expressions(ec.arena(), node);
    // Constructor parameter decorators contribute `__param(index, dec)` entries
    // *after* the class decorators (Go's `transformAllDecoratorsOfDeclaration`
    // order: class decorators, then `__param`, then metadata).
    append_constructor_param_decorators(ec, node, &mut decorator_expressions);
    if decorator_expressions.is_empty() {
        return None;
    }

    // With `--emitDecoratorMetadata`, append the class `design:paramtypes`
    // metadata *after* the real decorators / `__param` entries (Go folds the
    // `MetadataTransformer` injection here; metadata is ordered last).
    if cfg.emit_decorator_metadata {
        if let Some(resolver) = &cfg.resolver {
            append_class_type_metadata(ec, resolver, node, &mut decorator_expressions);
        }
    }

    // `__decorate([…], C)` — the class constructor is the target, with no member
    // name / descriptor (Go's `NewDecorateHelper(decoratorExpressions, localName,
    // nil, nil)`).
    let local_name = ec.arena_mut().new_identifier(class_name);
    let decorate = new_decorate_helper(ec, decorator_expressions, local_name, None, None);

    // `C = __decorate([…], C)` — Go's `NewAssignmentExpression(localName, decorate)`.
    let target = ec.arena_mut().new_identifier(class_name);
    let equals = ec.arena_mut().new_token(Kind::EqualsToken);
    Some(
        ec.arena_mut()
            .new_binary_expression(target, equals, decorate),
    )
}

/// Appends a `__param(index, decorator)` entry for each decorated parameter of
/// the class's first constructor with a body, in parameter order (Go's
/// `getDecoratorsOfParameters(GetFirstConstructorWithBody(node))` fed through
/// `transformDecoratorsOfParameters`). These follow the class's own decorators
/// in the constructor `__decorate` array.
///
/// DEFER(phase-6): the `this`-parameter offset (Go's `getDecoratorsOfParameters`
/// skips a leading `this` parameter when computing the index); not reachable in
/// the current single-file subset. blocked-by: `IsThisParameter` skip.
///
/// Side effects: pushes the `__param` call nodes; requests the `__param` helper.
// Go: internal/transformers/tstransforms/legacydecorators.go:transformDecoratorsOfParameters (constructor)
fn append_constructor_param_decorators(
    ec: &mut EmitContext,
    node: NodeId,
    decorator_expressions: &mut Vec<NodeId>,
) {
    let Some(ctor) = first_constructor_with_body(ec.arena(), node) else {
        return;
    };
    let parameters = match ec.arena().data(ctor) {
        NodeData::ConstructorDeclaration(d) => d.parameters.clone(),
        _ => return,
    };
    for (index, param) in parameters.nodes.iter().copied().enumerate() {
        for decorator in parameter_decorator_expressions(ec.arena(), param) {
            let helper = new_param_helper(ec, index, decorator);
            decorator_expressions.push(helper);
        }
    }
}

/// Appends the class's `design:*` reflection metadata to `decorator_expressions`
/// (Go's `MetadataTransformer.getOldTypeMetadata` for a class, folded in). A
/// class emits ONLY `design:paramtypes` (no `design:type` / `design:returntype`,
/// which are member-only), and only when it has a constructor with a body (Go's
/// `shouldAddParamTypesMetadata`: `GetFirstConstructorWithBody(node) != nil`).
///
/// Side effects: pushes the metadata call / array nodes; requests the
/// `__metadata` helper.
// Go: internal/transformers/tstransforms/metadata.go:getOldTypeMetadata (class)
fn append_class_type_metadata(
    ec: &mut EmitContext,
    resolver: &EmitReferenceResolver,
    node: NodeId,
    decorator_expressions: &mut Vec<NodeId>,
) {
    if first_constructor_with_body(ec.arena(), node).is_none() {
        return;
    }
    let paramtypes = serialize_constructor_parameter_types(ec, resolver, node);
    let metadata = new_metadata_helper(ec, "design:paramtypes", paramtypes);
    decorator_expressions.push(metadata);
}

/// Builds the `design:paramtypes` array for a decorated class: one serialized
/// type per parameter of the first constructor with a body, in order (Go's
/// `serializeParameterTypesOfNode` over the class's constructor). Each
/// parameter's annotation is serialized through the same checker path as a
/// property's `design:type`; an unannotated parameter serializes to `Object`.
///
/// DEFER(phase-6): the rest-parameter element type (Go's
/// `GetRestParameterElementType`); not reachable in the current subset. The
/// leading-`this` parameter is skipped here.
///
/// Side effects: pushes the array / serialized-type nodes.
// Go: internal/transformers/tstransforms/typeserializer.go:serializeParameterTypesOfNode (class)
fn serialize_constructor_parameter_types(
    ec: &mut EmitContext,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> NodeId {
    let parameters = match first_constructor_with_body(ec.arena(), node) {
        Some(ctor) => match ec.arena().data(ctor) {
            NodeData::ConstructorDeclaration(d) => d.parameters.clone(),
            _ => NodeList::new(Vec::new()),
        },
        None => NodeList::new(Vec::new()),
    };
    let mut expressions: Vec<NodeId> = Vec::new();
    for (index, param) in parameters.nodes.iter().copied().enumerate() {
        if index == 0 && is_this_parameter(ec.arena(), param) {
            continue;
        }
        let type_node = match ec.arena().data(param) {
            NodeData::ParameterDeclaration(d) => d.type_node,
            _ => None,
        };
        let value = match type_node {
            Some(type_node) => serialize_type_node(ec, resolver, type_node),
            None => ec.arena_mut().new_identifier("Object"),
        };
        expressions.push(value);
    }
    ec.arena_mut()
        .new_array_literal_expression(NodeList::new(expressions))
}

/// Reports whether `member` is a property or method declaration carrying at
/// least one decorator (the reachable subset of Go's
/// `NodeOrChildIsDecorated`).
///
/// Side effects: none (reads the arena).
fn member_has_decorators(arena: &NodeArena, member: NodeId) -> bool {
    let modifiers = match arena.data(member) {
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        _ => None,
    };
    modifiers.is_some_and(|m| m.modifier_flags.contains(ModifierFlags::DECORATOR))
}

/// Reports whether `member` carries a decorator itself, or (for a method) has at
/// least one decorated parameter (the reachable subset of Go's
/// `NodeOrChildIsDecorated`: `NodeIsDecorated || ChildIsDecorated`).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:NodeOrChildIsDecorated
fn member_is_decorated(arena: &NodeArena, member: NodeId) -> bool {
    member_has_decorators(arena, member) || method_has_decorated_parameter(arena, member)
}

/// Reports whether `member` is a method declaration with at least one decorated
/// parameter (Go's `ChildIsDecorated` arm for `KindMethodDeclaration`).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:ChildIsDecorated
fn method_has_decorated_parameter(arena: &NodeArena, member: NodeId) -> bool {
    let parameters = match arena.data(member) {
        NodeData::MethodDeclaration(d) => &d.parameters,
        _ => return false,
    };
    parameters
        .nodes
        .iter()
        .copied()
        .any(|p| parameter_has_decorators(arena, p))
}

/// Reports whether a parameter declaration carries at least one decorator.
///
/// Side effects: none (reads the arena).
fn parameter_has_decorators(arena: &NodeArena, param: NodeId) -> bool {
    match arena.data(param) {
        NodeData::ParameterDeclaration(d) => d
            .modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::DECORATOR)),
        _ => false,
    }
}

/// Rebuilds a property declaration with its decorators removed from the modifier
/// list and its type annotation / postfix token dropped (Go's
/// `LegacyDecoratorsTransformer.visitPropertyDeclaration`, which passes `nil`
/// for the type).
///
/// Side effects: pushes the rebuilt property node.
// Go: internal/transformers/tstransforms/legacydecorators.go:visitPropertyDeclaration
fn rebuild_property_without_decorators(arena: &mut NodeArena, member: NodeId) -> NodeId {
    let (modifiers, name, initializer) = match arena.data(member) {
        NodeData::PropertyDeclaration(d) => (d.modifiers.clone(), d.name, d.initializer),
        _ => unreachable!("kind/data mismatch"),
    };
    let modifiers = strip_decorators(arena, modifiers.as_ref());
    arena.new_property_declaration(modifiers, name, None, None, initializer)
}

/// Rebuilds a method declaration with its decorators removed from the modifier
/// list and its type parameters / return type / postfix token / full signature
/// dropped, keeping the asterisk, name, parameters, and body (Go's
/// `LegacyDecoratorsTransformer.visitMethodDeclaration`, which passes `nil` for
/// the type parameters, return type, postfix, and full signature).
///
/// Side effects: pushes the rebuilt method node.
// Go: internal/transformers/tstransforms/legacydecorators.go:visitMethodDeclaration
fn rebuild_method_without_decorators(arena: &mut NodeArena, member: NodeId) -> NodeId {
    let (modifiers, asterisk_token, name, parameters, body) = match arena.data(member) {
        NodeData::MethodDeclaration(d) => (
            d.modifiers.clone(),
            d.asterisk_token,
            d.name,
            d.parameters.clone(),
            d.body,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let modifiers = strip_decorators(arena, modifiers.as_ref());
    // Go's `visitMethodDeclaration` visits every parameter via
    // `visitParamerDeclaration`, which elides parameter modifiers (decorators)
    // and drops the type annotation / `?` token. Rebuild each parameter to keep
    // the lowered method body free of types and parameter decorators
    // (`m(@pdec a: number)` -> `m(a)`).
    let new_parameters: Vec<NodeId> = parameters
        .nodes
        .iter()
        .copied()
        .map(|p| rebuild_parameter_without_decorators(arena, p))
        .collect();
    arena.new_method_declaration(
        modifiers,
        asterisk_token,
        name,
        None,
        None,
        NodeList::new(new_parameters),
        None,
        None,
        body,
    )
}

/// Rebuilds a parameter declaration with its modifiers (parameter decorators)
/// elided and its type annotation / `?` token dropped, keeping the `...` rest
/// token, name, and initializer (Go's
/// `LegacyDecoratorsTransformer.visitParamerDeclaration`, which passes `nil` for
/// the type and `?` token and elides modifiers).
///
/// Side effects: pushes the rebuilt parameter node.
// Go: internal/transformers/tstransforms/legacydecorators.go:visitParamerDeclaration
fn rebuild_parameter_without_decorators(arena: &mut NodeArena, param: NodeId) -> NodeId {
    let (dot_dot_dot_token, name, initializer) = match arena.data(param) {
        NodeData::ParameterDeclaration(d) => (d.dot_dot_dot_token, d.name, d.initializer),
        _ => return param,
    };
    arena.new_parameter_declaration(None, dot_dot_dot_token, name, None, None, initializer)
}

/// Rebuilds a get/set accessor declaration with its decorators removed from the
/// modifier list and its type parameters / return type / full signature dropped,
/// rebuilding each parameter (Go's
/// `LegacyDecoratorsTransformer.visitGetAccessorDeclaration` /
/// `visitSetAccessorDeclaration`, which pass `nil` for the type parameters,
/// type, and full signature and visit each parameter via
/// `visitParamerDeclaration`). Applied to both the decorated accessor and its
/// undecorated partner so a `set x(value: number)` lowers to `set x(value)`.
///
/// Side effects: pushes the rebuilt accessor node.
// Go: internal/transformers/tstransforms/legacydecorators.go:visitGetAccessorDeclaration / visitSetAccessorDeclaration
fn rebuild_accessor_without_decorators(arena: &mut NodeArena, member: NodeId) -> NodeId {
    let (kind, modifiers, name, parameters, body) = match arena.data(member) {
        NodeData::GetAccessorDeclaration(d) => (
            Kind::GetAccessor,
            d.modifiers.clone(),
            d.name,
            d.parameters.clone(),
            d.body,
        ),
        NodeData::SetAccessorDeclaration(d) => (
            Kind::SetAccessor,
            d.modifiers.clone(),
            d.name,
            d.parameters.clone(),
            d.body,
        ),
        _ => return member,
    };
    let modifiers = strip_decorators(arena, modifiers.as_ref());
    let new_parameters: Vec<NodeId> = parameters
        .nodes
        .iter()
        .copied()
        .map(|p| rebuild_parameter_without_decorators(arena, p))
        .collect();
    arena.new_accessor_declaration(
        kind,
        modifiers,
        name,
        None,
        NodeList::new(new_parameters),
        None,
        None,
        body,
    )
}

/// Returns a copy of `modifiers` with decorator entries removed and the flag
/// union recomputed; `None` when nothing remains (Go's `VisitModifiers`, which
/// elides `KindDecorator`).
///
/// Side effects: none (builds a value list; no arena push).
fn strip_decorators(arena: &NodeArena, modifiers: Option<&ModifierList>) -> Option<ModifierList> {
    let modifiers = modifiers?;
    let kept: Vec<NodeId> = modifiers
        .list
        .nodes
        .iter()
        .copied()
        .filter(|&n| arena.kind(n) != Kind::Decorator)
        .collect();
    if kept.is_empty() {
        return None;
    }
    let modifier_flags = kept.iter().fold(ModifierFlags::empty(), |acc, &n| {
        acc | modifier_to_flag(arena.kind(n))
    });
    Some(ModifierList {
        list: NodeList::new(kept),
        modifier_flags,
    })
}

/// Appends the `design:*` reflection-metadata decorators for `member` to
/// `decorator_expressions` (the folded equivalent of Go's `MetadataTransformer`
/// injecting `@__metadata(...)` into the member's modifiers, in
/// `getOldTypeMetadata` order: `design:type`, then `design:returntype`).
///
/// - A **property** appends `design:type`, serialized from its type annotation
///   (or `Object` when absent — Go's `serializeTypeNode(nil)`).
/// - A **method** appends `design:type` = `Function` (Go's `serializeTypeOfNode`
///   hard-codes `Function` for `KindMethodDeclaration`, no checker), a
///   `design:paramtypes` array (6ap; one serialized type per parameter via
///   `serialize_parameter_types`), and a `design:returntype`
///   (`shouldAddReturnTypeMetadata` is true for *every* method); with no return
///   annotation the return type serializes to `void 0` (Go's
///   `serializeReturnTypeOfNode`). The order is `design:type` ->
///   `design:paramtypes` -> `design:returntype` (Go's `getOldTypeMetadata`).
///
/// DEFER: the async-method `Promise` return form (Go's
/// `serializeReturnTypeOfNode` `IsAsyncFunction` arm). blocked-by:
/// `IsAsyncFunction` modifier detection.
///
/// Side effects: pushes the metadata call/identifier nodes; requests the
/// `__metadata` helper.
// Go: internal/transformers/tstransforms/metadata.go:getOldTypeMetadata
fn append_type_metadata(
    ec: &mut EmitContext,
    resolver: &EmitReferenceResolver,
    member: NodeId,
    members: &[NodeId],
    decorator_expressions: &mut Vec<NodeId>,
) {
    match ec.arena().kind(member) {
        Kind::GetAccessor | Kind::SetAccessor => {
            // Go: `shouldAddTypeMetadata` is true for accessors -> `design:type`
            // = `serializeTypeNode(getAccessorTypeNode(...))`, i.e. the setter's
            // value-parameter type (if a setter exists) else the getter's return
            // type, and `Object` when that annotation is absent.
            let type_node = accessor_type_node(ec.arena(), member, members);
            let value = match type_node {
                Some(type_node) => serialize_type_node(ec, resolver, type_node),
                None => ec.arena_mut().new_identifier("Object"),
            };
            let type_meta = new_metadata_helper(ec, "design:type", value);
            decorator_expressions.push(type_meta);
            // Go: `shouldAddParamTypesMetadata` is *also* true for accessors, so
            // a `design:paramtypes` array follows. The parameters come from the
            // setter (for a getter with a setter partner) else the accessor's own
            // parameters (`getParametersOfDecoratedDeclaration`). Note: Go does
            // NOT emit `design:returntype` for accessors
            // (`shouldAddReturnTypeMetadata` is method-only).
            let paramtypes = serialize_accessor_parameter_types(ec, resolver, member, members);
            let paramtypes_meta = new_metadata_helper(ec, "design:paramtypes", paramtypes);
            decorator_expressions.push(paramtypes_meta);
        }
        Kind::PropertyDeclaration => {
            let type_node = match ec.arena().data(member) {
                NodeData::PropertyDeclaration(d) => d.type_node,
                _ => None,
            };
            let value = match type_node {
                Some(type_node) => serialize_type_node(ec, resolver, type_node),
                None => ec.arena_mut().new_identifier("Object"),
            };
            let metadata = new_metadata_helper(ec, "design:type", value);
            decorator_expressions.push(metadata);
        }
        Kind::MethodDeclaration => {
            // Go: `serializeTypeOfNode` returns `NewIdentifier("Function")` for a
            // method declaration — a fixed runtime constructor, not a checker query.
            let function = ec.arena_mut().new_identifier("Function");
            let type_meta = new_metadata_helper(ec, "design:type", function);
            decorator_expressions.push(type_meta);
            // Go: `shouldAddParamTypesMetadata` is true for a method, so
            // `design:paramtypes` is appended between `design:type` and
            // `design:returntype` (the `getOldTypeMetadata` order).
            let paramtypes = serialize_parameter_types(ec, resolver, member);
            let paramtypes_meta = new_metadata_helper(ec, "design:paramtypes", paramtypes);
            decorator_expressions.push(paramtypes_meta);
            // `design:returntype`: Go's `serializeReturnTypeOfNode` serializes the
            // return-type annotation when present (routed through the same checker
            // serialization as a property's `design:type`), else `void 0`.
            let return_type = match ec.arena().data(member) {
                NodeData::MethodDeclaration(d) => d.type_node,
                _ => None,
            };
            let value = match return_type {
                Some(return_type) => serialize_type_node(ec, resolver, return_type),
                None => make_void_zero(ec),
            };
            let return_meta = new_metadata_helper(ec, "design:returntype", value);
            decorator_expressions.push(return_meta);
        }
        _ => {}
    }
}

/// Builds the `design:paramtypes` array expression for a decorated method: one
/// serialized type per parameter, in order (Go's
/// `serializeParameterTypesOfNode`). Each parameter's type annotation is
/// serialized through the same checker path as a property's `design:type`
/// (`serializeTypeOfNode(parameter)` -> `serializeTypeNode(parameter.Type())`);
/// a parameter with no annotation serializes to `Object` (Go's
/// `serializeTypeNode(nil)`). With no parameters the array is empty (`[]`).
///
/// DEFER(phase-6): the `this`-parameter offset (Go's
/// `serializeParameterTypesOfNode` skips a leading `this` parameter) and the
/// rest-parameter element-type serialization (Go's
/// `GetRestParameterElementType` over `...args: T[]`); neither is reachable in
/// the current single-file subset. blocked-by: `IsThisParameter` skip + rest
/// element-type extraction.
///
/// Side effects: pushes the array / serialized-type nodes.
// Go: internal/transformers/tstransforms/typeserializer.go:serializeParameterTypesOfNode
fn serialize_parameter_types(
    ec: &mut EmitContext,
    resolver: &EmitReferenceResolver,
    member: NodeId,
) -> NodeId {
    let parameters = match ec.arena().data(member) {
        NodeData::MethodDeclaration(d) => d.parameters.clone(),
        _ => NodeList::new(Vec::new()),
    };
    let mut expressions: Vec<NodeId> = Vec::new();
    for param in parameters.nodes.iter().copied() {
        let type_node = match ec.arena().data(param) {
            NodeData::ParameterDeclaration(d) => d.type_node,
            _ => None,
        };
        let value = match type_node {
            Some(type_node) => serialize_type_node(ec, resolver, type_node),
            None => ec.arena_mut().new_identifier("Object"),
        };
        expressions.push(value);
    }
    ec.arena_mut()
        .new_array_literal_expression(NodeList::new(expressions))
}

/// A get/set accessor pair sharing a property name and static-ness within a
/// class body (the reachable subset of Go's `getAllAccessorDeclarations`).
struct AccessorPair {
    /// The matched accessors in declaration (member) order — used to pick the
    /// *first* accessor that owns the emitted `__decorate`.
    ordered: Vec<NodeId>,
    /// The get accessor of the pair, if present.
    get_accessor: Option<NodeId>,
    /// The set accessor of the pair, if present.
    set_accessor: Option<NodeId>,
}

/// Returns the identifier text of an accessor's property name, or `None` for a
/// computed / private / string-literal name (DEFER: those need
/// `getPropertyNameForPropertyNameNode` / checker symbol lookup).
///
/// Side effects: none (reads the arena).
fn accessor_name_text(arena: &NodeArena, member: NodeId) -> Option<String> {
    let name = match arena.data(member) {
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.name,
        _ => return None,
    };
    if arena.kind(name) == Kind::Identifier {
        Some(arena.text(name).to_string())
    } else {
        None
    }
}

/// Pairs an accessor with its same-named, same-static-ness partner among the
/// class `members` (Go's `getAllAccessorDeclarations` ->
/// `getAllAccessorDeclarationsForDeclaration`). A computed / private name cannot
/// be paired syntactically, so the accessor is treated as standalone (Go's
/// `HasDynamicName` early return).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:GetAllAccessorDeclarations
fn find_accessor_pair(arena: &NodeArena, members: &[NodeId], accessor: NodeId) -> AccessorPair {
    let mut pair = AccessorPair {
        ordered: Vec::new(),
        get_accessor: None,
        set_accessor: None,
    };
    let Some(name) = accessor_name_text(arena, accessor) else {
        pair.ordered.push(accessor);
        match arena.kind(accessor) {
            Kind::GetAccessor => pair.get_accessor = Some(accessor),
            Kind::SetAccessor => pair.set_accessor = Some(accessor),
            _ => {}
        }
        return pair;
    };
    let is_static = is_static_member(arena, accessor);
    for &m in members {
        let kind = arena.kind(m);
        if kind != Kind::GetAccessor && kind != Kind::SetAccessor {
            continue;
        }
        if is_static_member(arena, m) != is_static {
            continue;
        }
        if accessor_name_text(arena, m).as_deref() != Some(name.as_str()) {
            continue;
        }
        pair.ordered.push(m);
        match kind {
            Kind::GetAccessor if pair.get_accessor.is_none() => pair.get_accessor = Some(m),
            Kind::SetAccessor if pair.set_accessor.is_none() => pair.set_accessor = Some(m),
            _ => {}
        }
    }
    pair
}

/// Reports whether `accessor` is the first accessor (in declaration order) of
/// its pair that carries decorators — the one that owns the single emitted
/// `__decorate` (Go's `getAllDecoratorsOfAccessors`: returns `nil` for any
/// accessor that is not `firstAccessorWithDecorators`).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/legacydecorators.go:getAllDecoratorsOfAccessors
fn accessor_owns_decoration(arena: &NodeArena, accessor: NodeId, members: &[NodeId]) -> bool {
    let pair = find_accessor_pair(arena, members, accessor);
    let first = pair
        .ordered
        .iter()
        .copied()
        .find(|&a| member_has_decorators(arena, a));
    first == Some(accessor)
}

/// Reports whether a parameter is the contextual `this` parameter (an identifier
/// named `this`), which is excluded from the accessor's value parameter and the
/// `design:paramtypes` array (Go's `IsThisParameter`).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsThisParameter
fn is_this_parameter(arena: &NodeArena, param: NodeId) -> bool {
    let name = match arena.data(param) {
        NodeData::ParameterDeclaration(d) => d.name,
        _ => return false,
    };
    arena.kind(name) == Kind::Identifier && arena.text(name) == "this"
}

/// Returns the set accessor's value parameter — its first parameter, skipping a
/// leading `this` parameter (Go's `GetSetAccessorValueParameter`).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/metadata.go:GetSetAccessorValueParameter
fn set_accessor_value_parameter(arena: &NodeArena, set: NodeId) -> Option<NodeId> {
    let params = match arena.data(set) {
        NodeData::SetAccessorDeclaration(d) => &d.parameters.nodes,
        _ => return None,
    };
    let first = *params.first()?;
    if params.len() >= 2 && is_this_parameter(arena, first) {
        Some(params[1])
    } else {
        Some(first)
    }
}

/// Returns the type annotation of a set accessor's value parameter, or `None`
/// when absent (Go's `getSetAccessorTypeAnnotationNode`).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/metadata.go:getSetAccessorTypeAnnotationNode
fn set_accessor_type_annotation_node(arena: &NodeArena, set: NodeId) -> Option<NodeId> {
    let p = set_accessor_value_parameter(arena, set)?;
    match arena.data(p) {
        NodeData::ParameterDeclaration(d) => d.type_node,
        _ => None,
    }
}

/// Returns the type node used for an accessor's `design:type`: the set accessor's
/// value-parameter type when a setter exists, else the getter's return type
/// (Go's `getAccessorTypeNode`). `None` (-> `Object` at the call site) when the
/// chosen annotation is absent.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/metadata.go:getAccessorTypeNode
fn accessor_type_node(arena: &NodeArena, accessor: NodeId, members: &[NodeId]) -> Option<NodeId> {
    let pair = find_accessor_pair(arena, members, accessor);
    if let Some(set) = pair.set_accessor {
        return set_accessor_type_annotation_node(arena, set);
    }
    if let Some(get) = pair.get_accessor {
        return match arena.data(get) {
            NodeData::GetAccessorDeclaration(d) => d.type_node,
            _ => None,
        };
    }
    None
}

/// Returns the parameters whose types form an accessor's `design:paramtypes`
/// array: the setter's parameters when pairing a getter with a setter, else the
/// accessor's own parameters (Go's `getParametersOfDecoratedDeclaration`).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/typeserializer.go:getParametersOfDecoratedDeclaration
fn parameters_of_decorated_accessor(
    arena: &NodeArena,
    accessor: NodeId,
    members: &[NodeId],
) -> Vec<NodeId> {
    if arena.kind(accessor) == Kind::GetAccessor {
        let pair = find_accessor_pair(arena, members, accessor);
        if let Some(set) = pair.set_accessor {
            return match arena.data(set) {
                NodeData::SetAccessorDeclaration(d) => d.parameters.nodes.clone(),
                _ => Vec::new(),
            };
        }
    }
    match arena.data(accessor) {
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.parameters.nodes.clone()
        }
        _ => Vec::new(),
    }
}

/// Builds the `design:paramtypes` array for a decorated accessor: one serialized
/// type per parameter (Go's `serializeParameterTypesOfNode`). The parameters are
/// the setter's (for a getter+setter pair) else the accessor's own; a getter
/// with no setter yields `[]`. Each parameter's annotation is serialized through
/// the same checker path as a property's `design:type`; an unannotated parameter
/// serializes to `Object`.
///
/// DEFER(phase-6): the rest-parameter element type (Go's
/// `GetRestParameterElementType`); not reachable for accessors in the current
/// subset (a setter takes exactly one value parameter). blocked-by: rest
/// element-type extraction. (The leading-`this` parameter is skipped here.)
///
/// Side effects: pushes the array / serialized-type nodes.
// Go: internal/transformers/tstransforms/typeserializer.go:serializeParameterTypesOfNode
fn serialize_accessor_parameter_types(
    ec: &mut EmitContext,
    resolver: &EmitReferenceResolver,
    accessor: NodeId,
    members: &[NodeId],
) -> NodeId {
    let params = parameters_of_decorated_accessor(ec.arena(), accessor, members);
    let mut expressions: Vec<NodeId> = Vec::new();
    for (index, param) in params.iter().copied().enumerate() {
        if index == 0 && is_this_parameter(ec.arena(), param) {
            continue;
        }
        let type_node = match ec.arena().data(param) {
            NodeData::ParameterDeclaration(d) => d.type_node,
            _ => None,
        };
        let value = match type_node {
            Some(type_node) => serialize_type_node(ec, resolver, type_node),
            None => ec.arena_mut().new_identifier("Object"),
        };
        expressions.push(value);
    }
    ec.arena_mut()
        .new_array_literal_expression(NodeList::new(expressions))
}

/// Builds the `__decorate([...], <prefix>, "<name>", <descriptor>);` statement
/// for a decorated property or method `member` of class `class_name` (Go's
/// `generateClassElementDecorationExpression` wrapped in an expression
/// statement). Returns `None` when the member is not actually decorated.
///
/// Side effects: pushes rebuilt nodes; requests the `__decorate` (and, with
/// metadata, `__metadata`) helper.
// Go: internal/transformers/tstransforms/legacydecorators.go:generateClassElementDecorationExpression
fn generate_class_element_decoration_statement(
    ec: &mut EmitContext,
    cfg: &Config,
    class_name: &str,
    member: NodeId,
    members: &[NodeId],
) -> Option<NodeId> {
    // Go's `getAllDecoratorsOfAccessors`: a get/set pair emits a single
    // `__decorate`, owned by the *first* accessor (in declaration order) that
    // carries decorators. The other accessor returns no decoration expression.
    if matches!(
        ec.arena().kind(member),
        Kind::GetAccessor | Kind::SetAccessor
    ) && !accessor_owns_decoration(ec.arena(), member, members)
    {
        return None;
    }

    let mut decorator_expressions = decorator_expressions_of(ec.arena(), member);
    // Parameter decorators contribute `__param(index, dec)` entries *after* the
    // member's own decorators and *before* any metadata (Go's
    // `transformAllDecoratorsOfDeclaration` order: decorators, then `__param`,
    // then metadata).
    append_param_decorators(ec, member, &mut decorator_expressions);
    if decorator_expressions.is_empty() {
        return None;
    }

    // With `--emitDecoratorMetadata`, append the `design:*` metadata decorators
    // *after* the real decorators (Go's `transformAllDecoratorsOfDeclaration`
    // orders metadata last).
    if cfg.emit_decorator_metadata {
        if let Some(resolver) = &cfg.resolver {
            append_type_metadata(ec, resolver, member, members, &mut decorator_expressions);
        }
    }

    // A static member decorates the constructor directly (`C`); an instance
    // member decorates the prototype (`C.prototype`) — Go's
    // `getClassMemberPrefix`.
    let prefix = if is_static_member(ec.arena(), member) {
        ec.arena_mut().new_identifier(class_name)
    } else {
        let c = ec.arena_mut().new_identifier(class_name);
        let proto = ec.arena_mut().new_identifier("prototype");
        ec.arena_mut()
            .new_property_access_expression(c, None, proto)
    };

    let member_name = expression_for_property_name(ec, member);
    // Go's `generateClassElementDecorationExpression`: a property (not an
    // accessor) uses `void 0` so `__decorate` invokes `Object.defineProperty`
    // directly; every other member (a method or a get/set accessor) uses `null`
    // so `__decorate` invokes `Object.getOwnPropertyDescriptor` directly. (An
    // auto-accessor field — a `PropertyDeclaration` with the `accessor` modifier,
    // which Go's `HasAccessorModifier` would route to `null` — is DEFER, so a
    // plain property maps to `void 0` here.)
    let descriptor = if ec.arena().kind(member) == Kind::PropertyDeclaration {
        make_void_zero(ec)
    } else {
        ec.arena_mut().new_keyword_expression(Kind::NullKeyword)
    };

    let decorate = new_decorate_helper(
        ec,
        decorator_expressions,
        prefix,
        Some(member_name),
        Some(descriptor),
    );
    Some(ec.arena_mut().new_expression_statement(decorate))
}

/// Reports whether a property or method member carries the `static` modifier.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsStatic
fn is_static_member(arena: &NodeArena, member: NodeId) -> bool {
    let modifiers = match arena.data(member) {
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        _ => None,
    };
    modifiers.is_some_and(|m| m.modifier_flags.contains(ModifierFlags::STATIC))
}

/// Collects the (real) decorator expressions of a property or method member, in
/// source order.
///
/// Side effects: none (reads the arena).
fn decorator_expressions_of(arena: &NodeArena, member: NodeId) -> Vec<NodeId> {
    let modifiers = match arena.data(member) {
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        _ => None,
    };
    let Some(modifiers) = modifiers else {
        return Vec::new();
    };
    modifiers
        .list
        .nodes
        .iter()
        .copied()
        .filter(|&n| arena.kind(n) == Kind::Decorator)
        .map(|n| match arena.data(n) {
            NodeData::Decorator(d) => d.expression,
            _ => unreachable!("kind checked above"),
        })
        .collect()
}

/// Appends a `__param(index, decorator)` entry for each decorated parameter of a
/// method `member`, in parameter order (Go's `transformDecoratorsOfParameters`).
/// These follow the member's own decorators and precede any `design:*` metadata.
///
/// DEFER(phase-6): the `this`-parameter offset (Go's `getDecoratorsOfParameters`
/// skips a leading `this` parameter when computing the index); not reachable in
/// the current single-file subset. blocked-by: `IsThisParameter` skip.
///
/// Side effects: pushes the `__param` call nodes; requests the `__param` helper.
// Go: internal/transformers/tstransforms/legacydecorators.go:transformDecoratorsOfParameters
fn append_param_decorators(
    ec: &mut EmitContext,
    member: NodeId,
    decorator_expressions: &mut Vec<NodeId>,
) {
    let parameters = match ec.arena().data(member) {
        NodeData::MethodDeclaration(d) => d.parameters.clone(),
        _ => return,
    };
    for (index, param) in parameters.nodes.iter().copied().enumerate() {
        for decorator in parameter_decorator_expressions(ec.arena(), param) {
            let helper = new_param_helper(ec, index, decorator);
            decorator_expressions.push(helper);
        }
    }
}

/// Collects the decorator expressions of a parameter, in source order.
///
/// Side effects: none (reads the arena).
fn parameter_decorator_expressions(arena: &NodeArena, param: NodeId) -> Vec<NodeId> {
    let modifiers = match arena.data(param) {
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    let Some(modifiers) = modifiers else {
        return Vec::new();
    };
    modifiers
        .list
        .nodes
        .iter()
        .copied()
        .filter(|&n| arena.kind(n) == Kind::Decorator)
        .map(|n| match arena.data(n) {
            NodeData::Decorator(d) => d.expression,
            _ => unreachable!("kind checked above"),
        })
        .collect()
}

/// Builds the member-name expression for a property or method: an identifier
/// name becomes a string literal `"name"` (Go's `getExpressionForPropertyName`).
///
/// Side effects: may push the string-literal node.
// Go: internal/transformers/tstransforms/legacydecorators.go:getExpressionForPropertyName
fn expression_for_property_name(ec: &mut EmitContext, member: NodeId) -> NodeId {
    let name = match ec.arena().data(member) {
        NodeData::PropertyDeclaration(d) => d.name,
        NodeData::MethodDeclaration(d) => d.name,
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.name,
        _ => unreachable!("kind/data mismatch"),
    };
    match ec.arena().kind(name) {
        Kind::Identifier => {
            let text = ec.arena().text(name).to_string();
            ec.arena_mut().new_string_literal(&text, TokenFlags::NONE)
        }
        // String-literal names pass through; computed / private names are DEFER.
        _ => name,
    }
}

/// Builds `__decorate([<decorators>], <target>, <member_name?>, <descriptor?>)`,
/// requesting the `__decorate` helper so its definition is emitted in the module
/// prologue (Go's `NodeFactory.NewDecorateHelper`).
///
/// Side effects: pushes the call/array nodes; requests the `__decorate` helper.
// Go: internal/printer/factory.go:NodeFactory.NewDecorateHelper
fn new_decorate_helper(
    ec: &mut EmitContext,
    decorator_expressions: Vec<NodeId>,
    target: NodeId,
    member_name: Option<NodeId>,
    descriptor: Option<NodeId>,
) -> NodeId {
    ec.request_emit_helper(&DECORATE_HELPER);
    let array = ec
        .arena_mut()
        .new_array_literal_expression(NodeList::new(decorator_expressions));
    let mut arguments = vec![array, target];
    if let Some(member_name) = member_name {
        arguments.push(member_name);
        if let Some(descriptor) = descriptor {
            arguments.push(descriptor);
        }
    }
    let name = ec.factory().new_unscoped_helper_name("__decorate");
    ec.arena_mut()
        .new_call_expression(name, None, None, NodeList::new(arguments), NodeFlags::NONE)
}

/// Builds `__metadata("<key>", <value>)`, requesting the `__metadata` helper
/// (Go's `NodeFactory.NewMetadataHelper`).
///
/// Side effects: pushes the call/string nodes; requests the `__metadata` helper.
// Go: internal/printer/factory.go:NodeFactory.NewMetadataHelper
fn new_metadata_helper(ec: &mut EmitContext, key: &str, value: NodeId) -> NodeId {
    ec.request_emit_helper(&METADATA_HELPER);
    let key_literal = ec.arena_mut().new_string_literal(key, TokenFlags::NONE);
    let name = ec.factory().new_unscoped_helper_name("__metadata");
    ec.arena_mut().new_call_expression(
        name,
        None,
        None,
        NodeList::new(vec![key_literal, value]),
        NodeFlags::NONE,
    )
}

/// Builds `__param(<index>, <decorator>)`, requesting the `__param` helper (Go's
/// `NodeFactory.NewParamHelper`). The index is a numeric literal, `decorator` is
/// the parameter decorator's expression.
///
/// Side effects: pushes the call/numeric nodes; requests the `__param` helper.
// Go: internal/printer/factory.go:NodeFactory.NewParamHelper
fn new_param_helper(ec: &mut EmitContext, index: usize, decorator: NodeId) -> NodeId {
    ec.request_emit_helper(&PARAM_HELPER);
    let index_literal = ec
        .arena_mut()
        .new_numeric_literal(&index.to_string(), TokenFlags::NONE);
    let name = ec.factory().new_unscoped_helper_name("__param");
    ec.arena_mut().new_call_expression(
        name,
        None,
        None,
        NodeList::new(vec![index_literal, decorator]),
        NodeFlags::NONE,
    )
}

/// Serializes the property type-annotation node `type_node` to the
/// `__metadata("design:type", ..)` expression (Go's `serializeTypeNode`).
///
/// A `TypeReference` (`: SomeClass` / `: SomeInterface`) is dispatched to
/// [`serialize_type_reference_node`], which consults the checker's
/// classification so a class-typed member emits the class identifier itself.
/// Every other (keyword / structural) annotation is serialized by the checker's
/// [`serialize_type_node_for_metadata`](EmitReferenceResolver::serialize_type_node_for_metadata)
/// into a [`SerializedTypeNode`] and turned into its global constructor /
/// `void 0`.
///
/// Side effects: pushes the result expression nodes.
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode
fn serialize_type_node(
    ec: &mut EmitContext,
    resolver: &EmitReferenceResolver,
    type_node: NodeId,
) -> NodeId {
    // Go: `node = ast.SkipTypeParentheses(node)` runs before the switch, so a
    // parenthesized reference (`(C)`) still dispatches on the inner reference.
    let mut type_node = type_node;
    while let NodeData::ParenthesizedType(d) = ec.arena().data(type_node) {
        type_node = d.type_node;
    }
    // Go: `case KindTypeReference: return s.serializeTypeReferenceNode(...)`.
    if ec.arena().kind(type_node) == Kind::TypeReference {
        return serialize_type_reference_node(ec, resolver, type_node);
    }
    let serialized = resolver.serialize_type_node_for_metadata(type_node);
    serialized_type_to_expression(ec, serialized)
}

/// Serializes a `TypeReference` annotation to its `design:type` expression,
/// switching on the checker's [`TypeReferenceSerializationKind`] (Go's
/// `serializeTypeReferenceNode`).
///
/// The reachable single-file classifications (round 4ax/6an): a class-typed
/// reference is [`TypeWithConstructSignatureAndValue`](TypeReferenceSerializationKind::TypeWithConstructSignatureAndValue)
/// and emits the entity name as an expression (the class identifier `C`); an
/// interface/type-alias reference is [`ObjectType`](TypeReferenceSerializationKind::ObjectType)
/// and an unresolved name is [`Unknown`](TypeReferenceSerializationKind::Unknown),
/// both emitting `Object`.
///
/// DEFER(phase-6): the `Unknown` arm's full Go form is a
/// `typeof (_a = A) === "function" ? _a : Object` conditional guard; the
/// reachable port emits the `Object` tail (Go's `serializingConditionalTypeBranch`
/// result). The lib-globals-dependent kinds
/// (`Number`/`String`/`Boolean`/`BigInt`/`Symbol`/`Array`/`Function`/`Promise`/
/// `void 0`) are build-complete here, faithfully mirroring Go's switch, but the
/// checker (round 4aw) still classifies those types as `ObjectType`, so they are
/// not yet produced.
/// blocked-by: checker lib global types + construct/call-signature collection
/// (the conditional guard also needs `NewTempVariable` / `AddVariableDeclaration`).
///
/// Side effects: pushes the result expression nodes.
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeReferenceNode
fn serialize_type_reference_node(
    ec: &mut EmitContext,
    resolver: &EmitReferenceResolver,
    type_node: NodeId,
) -> NodeId {
    let kind = resolver.get_type_reference_serialization_kind(type_node);
    match kind {
        // Go: `case TypeWithConstructSignatureAndValue: return
        // s.serializeEntityNameAsExpression(node.TypeName)` — the class
        // identifier itself carries the runtime constructor.
        TypeReferenceSerializationKind::TypeWithConstructSignatureAndValue => {
            let type_name = match ec.arena().data(type_node) {
                NodeData::TypeReference(d) => d.type_name,
                _ => unreachable!("kind checked above"),
            };
            serialize_entity_name_as_expression(ec, type_name)
        }
        // Go: `case Unknown:` — outside a conditional-type branch Go emits a
        // `typeof`/conditional guard; the reachable port emits the `Object` tail
        // (the `serializingConditionalTypeBranch` result). DEFER the guard.
        TypeReferenceSerializationKind::Unknown => ec.arena_mut().new_identifier("Object"),
        // Go: `case ObjectType: return s.f.NewIdentifier("Object")`.
        TypeReferenceSerializationKind::ObjectType => ec.arena_mut().new_identifier("Object"),
        // Go: `case VoidNullableOrNeverType: return s.f.NewVoidZeroExpression()`.
        TypeReferenceSerializationKind::VoidNullableOrNeverType => make_void_zero(ec),
        // Go: `case NumberLikeType: return s.f.NewIdentifier("Number")`.
        TypeReferenceSerializationKind::NumberLikeType => ec.arena_mut().new_identifier("Number"),
        // Go: `case BigIntLikeType: return s.f.NewIdentifier("BigInt")`.
        TypeReferenceSerializationKind::BigIntLikeType => ec.arena_mut().new_identifier("BigInt"),
        // Go: `case StringLikeType: return s.f.NewIdentifier("String")`.
        TypeReferenceSerializationKind::StringLikeType => ec.arena_mut().new_identifier("String"),
        // Go: `case BooleanType: return s.f.NewIdentifier("Boolean")`.
        TypeReferenceSerializationKind::BooleanType => ec.arena_mut().new_identifier("Boolean"),
        // Go: `case ArrayLikeType: return s.f.NewIdentifier("Array")`.
        TypeReferenceSerializationKind::ArrayLikeType => ec.arena_mut().new_identifier("Array"),
        // Go: `case ESSymbolType: return s.f.NewIdentifier("Symbol")`.
        TypeReferenceSerializationKind::ESSymbolType => ec.arena_mut().new_identifier("Symbol"),
        // Go: `case TypeWithCallSignature: return s.f.NewIdentifier("Function")`.
        TypeReferenceSerializationKind::TypeWithCallSignature => {
            ec.arena_mut().new_identifier("Function")
        }
        // Go: `case Promise: return s.f.NewIdentifier("Promise")`.
        TypeReferenceSerializationKind::Promise => ec.arena_mut().new_identifier("Promise"),
    }
}

/// Serializes an entity name to an expression for decorator type metadata (Go's
/// `serializeEntityNameAsExpression`): a bare identifier `C` becomes a fresh
/// identifier expression `C`.
///
/// DEFER(phase-6): a qualified name (`A.B`) becomes a property-access chain;
/// the checker only classifies a bare identifier as
/// `TypeWithConstructSignatureAndValue` (qualified names resolve to `Unknown`),
/// so this arm is not reachable yet.
/// blocked-by: qualified-name `resolveEntityName` + namespace resolution.
///
/// Side effects: pushes the identifier node.
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeEntityNameAsExpression
fn serialize_entity_name_as_expression(ec: &mut EmitContext, entity_name: NodeId) -> NodeId {
    let text = ec.arena().text(entity_name).to_string();
    ec.arena_mut().new_identifier(&text)
}

/// Maps a checker [`SerializedTypeNode`] to the AST expression the
/// `__metadata("design:type", ..)` decorator carries: a global constructor
/// identifier, or the `void 0` expression.
///
/// Side effects: pushes the identifier / void nodes.
// Go: internal/transformers/tstransforms/typeserializer.go:serializeTypeNode (result construction)
fn serialized_type_to_expression(ec: &mut EmitContext, serialized: SerializedTypeNode) -> NodeId {
    match serialized {
        SerializedTypeNode::Number => ec.arena_mut().new_identifier("Number"),
        SerializedTypeNode::String => ec.arena_mut().new_identifier("String"),
        SerializedTypeNode::Boolean => ec.arena_mut().new_identifier("Boolean"),
        SerializedTypeNode::BigInt => ec.arena_mut().new_identifier("BigInt"),
        SerializedTypeNode::Symbol => ec.arena_mut().new_identifier("Symbol"),
        SerializedTypeNode::Object => ec.arena_mut().new_identifier("Object"),
        SerializedTypeNode::Array => ec.arena_mut().new_identifier("Array"),
        SerializedTypeNode::Function => ec.arena_mut().new_identifier("Function"),
        SerializedTypeNode::VoidZero => make_void_zero(ec),
    }
}

/// Builds the `void 0` expression (the property descriptor argument).
///
/// Side effects: pushes the literal / void nodes.
// Go: internal/printer/factory.go:NodeFactory.NewVoidZeroExpression
fn make_void_zero(ec: &mut EmitContext) -> NodeId {
    let zero = ec.arena_mut().new_numeric_literal("0", TokenFlags::NONE);
    ec.arena_mut().new_void_expression(zero)
}

#[cfg(test)]
#[path = "legacydecorators_test.rs"]
mod tests;
