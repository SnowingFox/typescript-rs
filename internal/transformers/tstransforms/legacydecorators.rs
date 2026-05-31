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
//! # Deferred (DEFER(P5))
//!
//! This first slice covers only property decorators. Deferred, each with its
//! blocker:
//!
//! - **Class decorators** (`@dec class C {}` → `let C = class C {}; C =
//!   __decorate([dec], C);`), including the self-reference class-alias rewrite.
//!   blocked-by: `getLocalName`/`GetDeclarationName` emit-name forms + the
//!   `classAliases` substitution and `getReferencedValueDeclaration`.
//! - **Method / accessor decorators** (`design:type` = `Function`,
//!   `design:returntype`). blocked-by: the method/accessor decoration shape and
//!   `getAllAccessorDeclarations`.
//! - **Parameter decorators** (`__param(i, dec)`) and `design:paramtypes`.
//!   blocked-by: `getDecoratorsOfParameters` + `serializeParameterTypesOfNode`.
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
    // DEFER: a decorator on the class itself (`@dec class C {}`) needs the
    // `let C = class C {}; C = __decorate([dec], C);` wrapping; leave it for a
    // later slice (the class passes through unchanged here).
    let has_class_decorator = modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::DECORATOR));
    if has_class_decorator {
        return visit_each_child_ec(ec, node, cfg);
    }

    let decorated_members: Vec<NodeId> = members
        .nodes
        .iter()
        .copied()
        .filter(|&m| property_has_decorators(ec.arena(), m))
        .collect();
    if decorated_members.is_empty() {
        // No member decorators in the reachable subset; recurse unchanged.
        return visit_each_child_ec(ec, node, cfg);
    }

    let name_text = name
        .map(|n| ec.arena().text(n).to_string())
        .unwrap_or_default();

    // Rebuild members, stripping decorators (and the type annotation) from each
    // decorated property (Go's `visitPropertyDeclaration`).
    let new_members: Vec<NodeId> = members
        .nodes
        .iter()
        .copied()
        .map(|m| {
            if property_has_decorators(ec.arena(), m) {
                rebuild_property_without_decorators(ec.arena_mut(), m)
            } else {
                m
            }
        })
        .collect();
    let updated_class = ec.arena_mut().new_class_like(
        Kind::ClassDeclaration,
        modifiers,
        name,
        None,
        heritage_clauses,
        NodeList::new(new_members),
    );

    // Build a `__decorate(...)` statement per decorated property.
    let mut statements = vec![updated_class];
    for member in decorated_members {
        if let Some(stmt) = generate_class_element_decoration_statement(ec, cfg, &name_text, member)
        {
            statements.push(stmt);
        }
    }

    ec.arena_mut().new_syntax_list(NodeList::new(statements))
}

/// Reports whether `member` is a property declaration carrying at least one
/// decorator.
///
/// Side effects: none (reads the arena).
fn property_has_decorators(arena: &NodeArena, member: NodeId) -> bool {
    matches!(
        arena.data(member),
        NodeData::PropertyDeclaration(d)
            if d.modifiers.as_ref().is_some_and(|m| m.modifier_flags.contains(ModifierFlags::DECORATOR))
    )
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

/// Builds the `__decorate([...], <prefix>, "<name>", void 0);` statement for a
/// decorated property `member` of class `class_name` (Go's
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
) -> Option<NodeId> {
    let mut decorator_expressions = decorator_expressions_of(ec.arena(), member);
    if decorator_expressions.is_empty() {
        return None;
    }

    // With `--emitDecoratorMetadata`, append a `design:type` metadata decorator
    // *after* the real decorators (Go's `transformAllDecoratorsOfDeclaration`
    // orders metadata last). The runtime constructor comes from the checker's
    // type serialization over the property's type annotation; a property with no
    // annotation serializes to `Object` (Go's `serializeTypeNode(nil)`).
    if cfg.emit_decorator_metadata {
        if let Some(resolver) = &cfg.resolver {
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
    // A property (not an accessor) uses `void 0` so `__decorate` invokes
    // `Object.defineProperty` directly.
    let descriptor = make_void_zero(ec);

    let decorate = new_decorate_helper(
        ec,
        decorator_expressions,
        prefix,
        Some(member_name),
        Some(descriptor),
    );
    Some(ec.arena_mut().new_expression_statement(decorate))
}

/// Reports whether a property member carries the `static` modifier.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsStatic
fn is_static_member(arena: &NodeArena, member: NodeId) -> bool {
    matches!(
        arena.data(member),
        NodeData::PropertyDeclaration(d)
            if d.modifiers.as_ref().is_some_and(|m| m.modifier_flags.contains(ModifierFlags::STATIC))
    )
}

/// Collects the (real) decorator expressions of a property member, in source
/// order.
///
/// Side effects: none (reads the arena).
fn decorator_expressions_of(arena: &NodeArena, member: NodeId) -> Vec<NodeId> {
    let modifiers = match arena.data(member) {
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
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

/// Builds the member-name expression for a property: an identifier name becomes
/// a string literal `"name"` (Go's `getExpressionForPropertyName`).
///
/// Side effects: may push the string-literal node.
// Go: internal/transformers/tstransforms/legacydecorators.go:getExpressionForPropertyName
fn expression_for_property_name(ec: &mut EmitContext, member: NodeId) -> NodeId {
    let name = match ec.arena().data(member) {
        NodeData::PropertyDeclaration(d) => d.name,
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
