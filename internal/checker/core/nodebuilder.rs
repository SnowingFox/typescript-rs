//! Type → string serialization (the reachable core of Go's node builder +
//! `printer.go` `typeToString`).
//!
//! Produces the human-readable type text used in diagnostics. The 4a–4i
//! placeholder (`Checker::type_to_string`) handled intrinsics/literals/unions
//! but printed object types as `{ ... }` and type parameters as `T`; this module
//! adds faithful names, type references, member literals, and union recursion.
//!
//! Symbol names live in the bound program (not the checker), so the faithful
//! serializer takes a [`BoundProgram`]. It also resolves member types lazily
//! (Go's `typeToString` triggers resolution), so it takes `&mut Checker`.
//!
//! DEFER(phase-4-checker-4k): function/construct signatures (`(x: T) => U`),
//! array types, mapped/conditional types, alias names, and optional member
//! adornments — those need type kinds not yet constructed and the full
//! node-builder scope machinery. (4v adds intersection printing: `A & B`; 4bi
//! adds fixed-arity tuple printing `[A, B]` / `readonly [A, B]` and the
//! `readonly` adornment on a const object-literal property.)

use std::cell::Cell;

use tsgo_ast::{Kind, SymbolFlags, SymbolId};

use super::declared_types::{get_declared_type_of_symbol, get_type_of_symbol};
use super::program::BoundProgram;
use super::type_facts::TypeFacts;
use super::types::{LiteralValue, ObjectFlags, TypeData, TypeFlags, TypeId};
use super::Checker;

/// Maximum recursion depth for [`type_to_string`]. Go uses an identity-based
/// recursion check (repeated types on the same stack are truncated); we use a
/// simpler depth cap that prevents stack overflow without changing semantics for
/// the common case (realistic types never nest > 20 levels deep in their
/// printed form).
// Go: internal/checker/nodebuilderimpl.go:typeToTypeNodeWorker (recursion identity guard)
const MAX_TYPE_TO_STRING_DEPTH: u32 = 50;

thread_local! {
    /// Tracks current `type_to_string` recursion depth. `pub(crate)` for testing.
    pub(crate) static TYPE_TO_STRING_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// A syntactic *type node* produced for a type by the node builder, serialized
/// as a closed descriptor the declaration transformer reconstructs into AST.
///
/// Go's `NodeBuilderImpl.typeToTypeNode` builds an `*ast.TypeNode` directly in
/// the emit context's factory. This port cannot hand back an `ast::NodeId`: the
/// checker's [`BoundProgram`] arena and the transformer's `EmitContext` arena
/// are independent (the two-arena split documented across this crate). So,
/// mirroring the existing [`SerializedTypeNode`](super::emit_resolver::SerializedTypeNode)
/// bridge for `design:type` metadata, `type_to_type_node` *names* the type node
/// it would build and the transformer reconstructs the real AST in its own
/// arena. Each variant maps one-to-one to a Go `typeToTypeNode` result for the
/// reachable declaration-emit subset.
///
/// # Examples
/// ```
/// use tsgo_checker::SynthesizedTypeNode;
/// use tsgo_ast::Kind;
/// // `number` serializes to a keyword type node.
/// assert_eq!(
///     SynthesizedTypeNode::Keyword(Kind::NumberKeyword),
///     SynthesizedTypeNode::Keyword(Kind::NumberKeyword),
/// );
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/nodebuilderimpl.go:NodeBuilderImpl.typeToTypeNode
#[derive(Clone, Debug, PartialEq)]
pub enum SynthesizedTypeNode {
    /// A keyword type node: `number`/`string`/`boolean`/`bigint`/`symbol`/
    /// `void`/`undefined`/`never`/`any`/`unknown`/`object`. Go:
    /// `b.f.NewKeywordTypeNode(kind)`.
    Keyword(Kind),
    /// A numeric literal type (`1` / `-1`); `text` is the unsigned literal text,
    /// `negative` records a leading unary minus. Go:
    /// `NewLiteralTypeNode(NewNumericLiteral(..))` (with `NewPrefixUnaryExpression`
    /// for a negative).
    NumberLiteral { text: String, negative: bool },
    /// A string literal type (`"a"`); `value` is the unescaped text. Go:
    /// `NewLiteralTypeNode(newStringLiteral(value))`.
    StringLiteral(String),
    /// A boolean literal type (`true`/`false`). Go:
    /// `NewLiteralTypeNode(NewKeywordExpression(True/FalseKeyword))`.
    BooleanLiteral(bool),
    /// The `null` literal type. Go:
    /// `NewLiteralTypeNode(NewKeywordExpression(NullKeyword))`.
    Null,
    /// An array type (`T[]`). Go: `b.f.NewArrayTypeNode(elementType)`.
    Array(Box<SynthesizedTypeNode>),
    /// A type reference (`Foo` / `Foo<A, B>`); `name` is the entity name, `args`
    /// the type arguments. Go: `b.f.NewTypeReferenceNode(name, typeArguments)`.
    TypeReference {
        /// The referenced entity's printed name.
        name: String,
        /// The type-argument nodes (`<A, B>`), empty for a bare reference.
        args: Vec<SynthesizedTypeNode>,
    },
    /// A `typeof X` type query (the value side of a namespace/module symbol).
    /// Go: `b.f.NewTypeQueryNode(name, nil)` (the value-module arm).
    TypeQuery(String),
    /// A union type (`A | B`). Go: `b.f.NewUnionTypeNode(types)`.
    Union(Vec<SynthesizedTypeNode>),
    /// An intersection type (`A & B`). Go: `b.f.NewIntersectionTypeNode(types)`.
    Intersection(Vec<SynthesizedTypeNode>),
    /// A fixed-arity tuple type (`[A, B]` / `readonly [A, B]`). Go:
    /// the tuple-type-node arm of `typeToTypeNode`.
    Tuple {
        /// The positional element type nodes.
        elements: Vec<SynthesizedTypeNode>,
        /// Whether the tuple is `readonly` (an `as const` tuple).
        readonly: bool,
    },
    /// An anonymous object/type-literal (`{ a: number; }`). Go:
    /// `createAnonymousTypeNode` -> `NewTypeLiteralNode(members)`.
    TypeLiteral(Vec<SynthesizedProperty>),
}

/// One member of a [`SynthesizedTypeNode::TypeLiteral`] (`a: number`).
///
/// # Examples
/// ```
/// use tsgo_checker::{SynthesizedProperty, SynthesizedTypeNode};
/// use tsgo_ast::Kind;
/// let p = SynthesizedProperty {
///     name: "a".to_string(),
///     type_node: SynthesizedTypeNode::Keyword(Kind::NumberKeyword),
///     readonly: false,
///     optional: false,
/// };
/// assert_eq!(p.name, "a");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/nodebuilderimpl.go (createTypeNodesFromResolvedType property member)
#[derive(Clone, Debug, PartialEq)]
pub struct SynthesizedProperty {
    /// The property name (an identifier-named member in the reachable subset).
    pub name: String,
    /// The property's type node.
    pub type_node: SynthesizedTypeNode,
    /// Whether the property is `readonly` (an `as const` member).
    pub readonly: bool,
    /// Whether the property is optional (`a?: T`).
    pub optional: bool,
}

/// Returns the printed name of `symbol` (Go's `symbolToString` for the simple
/// declaration-name case).
///
/// DEFER(phase-4-checker-4k): qualified names, computed names, and the
/// `SymbolTracker`/accessibility-aware path.
///
/// # Examples
/// ```
/// use tsgo_checker::{symbol_to_string, BoundProgram};
/// # fn demo<P: BoundProgram>(p: &P, s: tsgo_ast::SymbolId) -> String {
/// symbol_to_string(p, s)
/// # }
/// ```
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.symbolToString
pub fn symbol_to_string(checker: &Checker, program: &dyn BoundProgram, symbol: SymbolId) -> String {
    checker.resolved_symbol_name(program, symbol)
}

/// Returns the printed form of `ty` (Go's `typeToString`).
///
/// Resolves names/members through `program` and triggers lazy member-type
/// resolution (hence `&mut Checker`). Intrinsics, literals, and unions of those
/// delegate to the program-less [`Checker::type_to_string`].
///
/// # Examples
/// ```
/// use tsgo_checker::{type_to_string, BoundProgram, Checker};
/// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, t: tsgo_checker::TypeId) -> String {
/// type_to_string(c, p, t)
/// # }
/// ```
///
/// Side effects: may resolve and cache member types.
// Go: internal/checker/checker.go:Checker.typeToString
pub fn type_to_string(checker: &mut Checker, program: &dyn BoundProgram, ty: TypeId) -> String {
    // Depth guard: prevent stack overflow when printing deeply recursive types.
    // Go: internal/checker/nodebuilderimpl.go:typeToTypeNodeWorker (recursion identity guard)
    let depth = TYPE_TO_STRING_DEPTH.with(|d| {
        let v = d.get();
        d.set(v + 1);
        v
    });
    if depth >= MAX_TYPE_TO_STRING_DEPTH {
        TYPE_TO_STRING_DEPTH.with(|d| d.set(d.get() - 1));
        return "...".to_string();
    }
    let result = type_to_string_inner(checker, program, ty);
    TYPE_TO_STRING_DEPTH.with(|d| d.set(d.get() - 1));
    result
}

fn type_to_string_inner(checker: &mut Checker, program: &dyn BoundProgram, ty: TypeId) -> String {
    // An enum-like type (enum union, computed enum, or an enum member literal)
    // prints by its symbol, not its structure: an enum member literal prints
    // `E.A` (parent enum `.` member), except when the enum's declared type *is*
    // that member (a single-member enum), where it prints just `E`; the enum
    // union / computed enum prints the enum name `E`. Checked before the union
    // arm because the enum union also carries `TypeData::Union`.
    // Go: internal/checker/nodebuilderimpl.go:typeToTypeNodeWorker (EnumLike arm)
    if checker
        .get_type(ty)
        .flags()
        .intersects(TypeFlags::ENUM_LIKE)
    {
        if let Some(symbol) = checker.get_type(ty).symbol {
            if checker
                .resolved_symbol_flags(program, symbol)
                .intersects(SymbolFlags::ENUM_MEMBER)
            {
                if let Some(parent) = program.symbol(symbol).parent {
                    let parent_name = symbol_to_string(checker, program, parent);
                    let globals = program.globals();
                    if get_declared_type_of_symbol(checker, program, parent, globals) == ty {
                        return parent_name;
                    }
                    let member_name = symbol_to_string(checker, program, symbol);
                    return format!("{parent_name}.{member_name}");
                }
            }
            return symbol_to_string(checker, program, symbol);
        }
    }
    // A union prints its constituents (each program-aware) joined by ` | `.
    if let TypeData::Union(u) = &checker.get_type(ty).data {
        let members = u.types.clone();
        let parts: Vec<String> = members
            .iter()
            .map(|&m| type_to_string(checker, program, m))
            .collect();
        return parts.join(" | ");
    }
    // An intersection prints its constituents (each program-aware) joined by
    // ` & `, so named members render as `A & B` rather than `{ ... } & { ... }`.
    if let TypeData::Intersection(i) = &checker.get_type(ty).data {
        let members = i.types.clone();
        let parts: Vec<String> = members
            .iter()
            .map(|&m| type_to_string(checker, program, m))
            .collect();
        return parts.join(" & ");
    }
    // A fixed-arity tuple prints as `[e0, e1]` (or `readonly [e0, e1]` for an
    // `as const` readonly tuple), with the positional element types printed in
    // order. Checked before the type-reference/anonymous paths because a tuple
    // carries its elements in `resolved_type_arguments` with no `target`.
    if checker
        .get_type(ty)
        .object_flags()
        .contains(ObjectFlags::TUPLE)
    {
        return serialize_tuple(checker, program, ty);
    }
    // A deferred `keyof X` index type prints `keyof <operand>`, naming the
    // operand through the program-aware printer.
    if let Some(d) = checker.get_type(ty).as_index().cloned() {
        let target = type_to_string(checker, program, d.target);
        return format!("keyof {target}");
    }
    // A deferred `X[Y]` indexed-access type prints `<object>[<index>]`.
    if let Some(d) = checker.get_type(ty).as_indexed_access().cloned() {
        let object = type_to_string(checker, program, d.object_type);
        let index = type_to_string(checker, program, d.index_type);
        return format!("{object}[{index}]");
    }
    // A deferred conditional type prints `<check> extends <extends> ? X : Y`,
    // naming the instantiated check/extends operands and resolving the branch
    // type nodes through the program (Go's node-builder conditional arm).
    if let Some(d) = checker.get_type(ty).as_conditional().cloned() {
        let check = type_to_string(checker, program, d.check_type);
        let extends = type_to_string(checker, program, d.extends_type);
        let (true_node, false_node) = match program.arena().data(d.root.node) {
            tsgo_ast::NodeData::ConditionalType(c) => (c.true_type, c.false_type),
            _ => return format!("{check} extends {extends} ? ... : ..."),
        };
        let true_ty =
            super::declared_types::get_type_from_type_node(checker, program, true_node, None);
        let false_ty =
            super::declared_types::get_type_from_type_node(checker, program, false_node, None);
        let true_str = type_to_string(checker, program, true_ty);
        let false_str = type_to_string(checker, program, false_ty);
        return format!("{check} extends {extends} ? {true_str} : {false_str}");
    }
    // A deferred template literal type prints `` `t0${T0}t1...` ``, naming the
    // placeholder operands program-aware.
    if let Some(d) = checker.get_type(ty).as_template_literal().cloned() {
        let mut out = String::from("`");
        out.push_str(&d.texts[0]);
        for (i, &t) in d.types.iter().enumerate() {
            out.push_str("${");
            let s = type_to_string(checker, program, t);
            out.push_str(&s);
            out.push('}');
            out.push_str(&d.texts[i + 1]);
        }
        out.push('`');
        return out;
    }
    // A deferred string-mapping type prints `Uppercase<target>`.
    if let Some(d) = checker.get_type(ty).as_string_mapping().cloned() {
        let target = type_to_string(checker, program, d.target);
        return format!("{}<{}>", d.kind.intrinsic_name(), target);
    }
    let symbol = checker.get_type(ty).symbol;
    let object_info = match &checker.get_type(ty).data {
        TypeData::Object(o) => Some((o.target, o.resolved_type_arguments.clone())),
        _ => None,
    };
    if let Some((target, type_arguments)) = object_info {
        // A type reference (`Foo<...>`) prints as `target<args>`.
        if let Some(target) = target {
            let name = checker
                .get_type(target)
                .symbol
                .map(|s| symbol_to_string(checker, program, s))
                .unwrap_or_default();
            if type_arguments.is_empty() {
                return name;
            }
            let args: Vec<String> = type_arguments
                .iter()
                .map(|&a| type_to_string(checker, program, a))
                .collect();
            return format!("{name}<{}>", args.join(", "));
        }
        // A named interface/class/enum type prints as its name. An anonymous
        // type-literal/object-literal symbol carries an internal name (the
        // `\u{FE}`-prefixed `__type`/`__object`); such a type serializes its
        // member literal instead (Go's `createAnonymousTypeNode` only emits a
        // type-reference node for a symbol with a real name).
        if let Some(symbol) = symbol {
            let name = symbol_to_string(checker, program, symbol);
            if !name.starts_with(tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_PREFIX) {
                // A namespace/module value type prints as `typeof N` (Go emits a
                // `typeQuery` node for a value-module symbol's anonymous type in
                // `typeToTypeNodeWorker`), distinguishing the value side from the
                // namespace's type side.
                if checker
                    .resolved_symbol_flags(program, symbol)
                    .intersects(SymbolFlags::MODULE)
                {
                    return format!("typeof {name}");
                }
                return name;
            }
        }
        // A bare function/constructor type (a single call or construct
        // signature, no properties/index/other signatures) prints in arrow
        // shorthand: `(x: T) => R` / `new (x: T) => R`.
        if let Some(shorthand) = serialize_signature_shorthand(checker, program, ty) {
            return shorthand;
        }
        // An anonymous object type prints its member literal.
        return serialize_members(checker, program, ty);
    }
    // Intrinsics/literals/unions (and not-yet-handled kinds) use the
    // program-less printer.
    checker.type_to_string(ty)
}

/// Builds a syntactic *type node* descriptor for `ty` (Go's
/// `NodeBuilderImpl.typeToTypeNode`), for the reachable declaration-emit subset.
///
/// The result is a [`SynthesizedTypeNode`] the declaration transformer
/// reconstructs into AST in its own arena (see [`SynthesizedTypeNode`] for the
/// two-arena rationale). This mirrors Go's `typeToTypeNode` switch one arm at a
/// time: primitive keyword types, literal types, arrays (`T[]`), bare type
/// references (`Foo<...>`), unions / intersections, fixed-arity tuples, and
/// anonymous object type-literals (`{ a: number; }`). Reuses the same
/// type-walking the program-aware [`type_to_string`] already performs.
///
/// Returns `None` for the deferred type kinds (`keyof`/indexed-access/
/// conditional/template-literal/string-mapping, generic type parameters,
/// function/constructor types, bigint literals, import types); the caller falls
/// back to an `any` keyword node, mirroring Go's `serializeTypeForDeclaration`
/// `result == nil -> NewKeywordTypeNode(any)` tail.
///
/// # Examples
/// ```
/// use tsgo_checker::{type_to_type_node, BoundProgram, Checker, SynthesizedTypeNode};
/// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, t: tsgo_checker::TypeId) -> Option<SynthesizedTypeNode> {
/// type_to_type_node(c, p, t)
/// # }
/// ```
///
/// Side effects: may resolve and cache member/element types.
// Go: internal/checker/nodebuilderimpl.go:NodeBuilderImpl.typeToTypeNode
pub fn type_to_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    ty: TypeId,
) -> Option<SynthesizedTypeNode> {
    let flags = checker.get_type(ty).flags();
    // Go's leading primitive arms (`t.flags & TypeFlagsAny/Unknown/String/...`).
    // The error type carries `TypeFlagsAny`, so an unresolved type degrades to
    // the `any` keyword exactly as Go's `typeToTypeNode(errorType)` does.
    if flags.intersects(TypeFlags::ANY) {
        return Some(SynthesizedTypeNode::Keyword(Kind::AnyKeyword));
    }
    if flags.intersects(TypeFlags::UNKNOWN) {
        return Some(SynthesizedTypeNode::Keyword(Kind::UnknownKeyword));
    }
    if flags.intersects(TypeFlags::STRING) {
        return Some(SynthesizedTypeNode::Keyword(Kind::StringKeyword));
    }
    if flags.intersects(TypeFlags::NUMBER) {
        return Some(SynthesizedTypeNode::Keyword(Kind::NumberKeyword));
    }
    if flags.intersects(TypeFlags::BIG_INT) {
        return Some(SynthesizedTypeNode::Keyword(Kind::BigIntKeyword));
    }
    if flags.intersects(TypeFlags::BOOLEAN) {
        return Some(SynthesizedTypeNode::Keyword(Kind::BooleanKeyword));
    }
    // Go's `typeToTypeNode` keys `boolean` on `t.flags & TypeFlagsBoolean`; this
    // port represents `boolean` as the singleton `false | true` union (no
    // `BOOLEAN` flag on the union), so a widened boolean literal is recognized by
    // its singleton type id before the generic union arm turns it into
    // `false | true`.
    if ty == checker.boolean_type() {
        return Some(SynthesizedTypeNode::Keyword(Kind::BooleanKeyword));
    }
    // An enum-like type prints by its symbol name (Go's `EnumLike` arm). The
    // reachable subset emits a bare type reference to that name; the dotted
    // `E.A` enum-member form and the union-expansion are deferred.
    if flags.intersects(TypeFlags::ENUM_LIKE) {
        if let Some(symbol) = checker.get_type(ty).symbol {
            return Some(SynthesizedTypeNode::TypeReference {
                name: symbol_to_string(checker, program, symbol),
                args: Vec::new(),
            });
        }
    }
    if flags.intersects(TypeFlags::STRING_LITERAL) {
        if let Some(LiteralValue::String(s)) = checker.get_type(ty).literal_value() {
            return Some(SynthesizedTypeNode::StringLiteral(s.clone()));
        }
    }
    if flags.intersects(TypeFlags::NUMBER_LITERAL) {
        if let Some(LiteralValue::Number(n)) = checker.get_type(ty).literal_value() {
            let value = f64::from(*n);
            // Go splits a negative numeric literal into a unary-minus over the
            // unsigned text (`NewPrefixUnaryExpression(Minus, NewNumericLiteral)`).
            let negative = value < 0.0;
            let text = if negative {
                (-value).to_string()
            } else {
                value.to_string()
            };
            return Some(SynthesizedTypeNode::NumberLiteral { text, negative });
        }
    }
    if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
        if let Some(LiteralValue::Boolean(b)) = checker.get_type(ty).literal_value() {
            return Some(SynthesizedTypeNode::BooleanLiteral(*b));
        }
    }
    if flags.intersects(TypeFlags::VOID) {
        return Some(SynthesizedTypeNode::Keyword(Kind::VoidKeyword));
    }
    if flags.intersects(TypeFlags::UNDEFINED) {
        return Some(SynthesizedTypeNode::Keyword(Kind::UndefinedKeyword));
    }
    if flags.intersects(TypeFlags::NULL) {
        return Some(SynthesizedTypeNode::Null);
    }
    if flags.intersects(TypeFlags::NEVER) {
        return Some(SynthesizedTypeNode::Keyword(Kind::NeverKeyword));
    }
    if flags.intersects(TypeFlags::ES_SYMBOL) {
        return Some(SynthesizedTypeNode::Keyword(Kind::SymbolKeyword));
    }
    if flags.intersects(TypeFlags::NON_PRIMITIVE) {
        return Some(SynthesizedTypeNode::Keyword(Kind::ObjectKeyword));
    }
    // A fixed-arity tuple becomes a tuple type node (`[A, B]` / `readonly
    // [A, B]`), checked before the type-reference/anonymous arms because a tuple
    // carries its elements in `resolved_type_arguments` with no `target`.
    if checker
        .get_type(ty)
        .object_flags()
        .contains(ObjectFlags::TUPLE)
    {
        let (elements, readonly) = match &checker.get_type(ty).data {
            TypeData::Object(o) => (o.resolved_type_arguments.clone(), o.readonly),
            _ => return None,
        };
        let element_nodes: Option<Vec<SynthesizedTypeNode>> = elements
            .iter()
            .map(|&e| type_to_type_node(checker, program, e))
            .collect();
        return Some(SynthesizedTypeNode::Tuple {
            elements: element_nodes?,
            readonly,
        });
    }
    // Union / intersection map their constituents (Go's union/intersection arm);
    // a single-element list unwraps to that element.
    if let TypeData::Union(u) = &checker.get_type(ty).data {
        let members = u.types.clone();
        if members.len() == 1 {
            return type_to_type_node(checker, program, members[0]);
        }
        let nodes: Option<Vec<SynthesizedTypeNode>> = members
            .iter()
            .map(|&m| type_to_type_node(checker, program, m))
            .collect();
        return Some(SynthesizedTypeNode::Union(nodes?));
    }
    if let TypeData::Intersection(i) = &checker.get_type(ty).data {
        let members = i.types.clone();
        if members.len() == 1 {
            return type_to_type_node(checker, program, members[0]);
        }
        let nodes: Option<Vec<SynthesizedTypeNode>> = members
            .iter()
            .map(|&m| type_to_type_node(checker, program, m))
            .collect();
        return Some(SynthesizedTypeNode::Intersection(nodes?));
    }
    let symbol = checker.get_type(ty).symbol;
    let object_info = match &checker.get_type(ty).data {
        TypeData::Object(o) => Some((o.target, o.resolved_type_arguments.clone())),
        _ => None,
    };
    if let Some((target, type_arguments)) = object_info {
        // A type reference (`Foo<...>`). Go's `typeReferenceToTypeNode` special-
        // cases the global `Array` type with a single argument as `T[]`; the
        // reachable stand-in detects it by the target symbol's name being
        // `Array` (the same by-name resolution `createArrayLiteralType` uses,
        // since the real `globalArrayType` needs lib globals — P6).
        if let Some(target) = target {
            let target_name = checker
                .get_type(target)
                .symbol
                .map(|s| symbol_to_string(checker, program, s))
                .unwrap_or_default();
            if target_name == "Array" && type_arguments.len() == 1 {
                let element = type_to_type_node(checker, program, type_arguments[0])?;
                return Some(SynthesizedTypeNode::Array(Box::new(element)));
            }
            let args: Option<Vec<SynthesizedTypeNode>> = type_arguments
                .iter()
                .map(|&a| type_to_type_node(checker, program, a))
                .collect();
            return Some(SynthesizedTypeNode::TypeReference {
                name: target_name,
                args: args?,
            });
        }
        // A named interface/class/enum type prints as its name; a module value
        // type prints as `typeof N`. An anonymous type-literal symbol carries an
        // internal `__type`/`__object` name and serializes its member literal.
        if let Some(symbol) = symbol {
            let name = symbol_to_string(checker, program, symbol);
            if !name.starts_with(tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_PREFIX) {
                if checker
                    .resolved_symbol_flags(program, symbol)
                    .intersects(SymbolFlags::MODULE)
                {
                    return Some(SynthesizedTypeNode::TypeQuery(name));
                }
                return Some(SynthesizedTypeNode::TypeReference {
                    name,
                    args: Vec::new(),
                });
            }
        }
        // An anonymous object type becomes a type-literal of its members. The
        // bare function/constructor shorthand and mixed signature forms are
        // deferred (no test slice reaches them; they fall to `None` -> `any`).
        return synthesize_members(checker, program, ty);
    }
    // Deferred kinds (type parameters, function/constructor types, `keyof`,
    // indexed-access, conditional, template-literal, string-mapping, bigint
    // literals, import types) degrade to the `any` keyword via the caller.
    None
}

// Builds a type-literal descriptor for an anonymous object type's members
// (`{ a: number; }`), mirroring [`serialize_members`] but emitting
// [`SynthesizedProperty`] nodes instead of text. Returns `None` (caller -> `any`)
// when a member type cannot be synthesized.
// Go: internal/checker/nodebuilderimpl.go:createTypeNodesFromResolvedType (properties)
fn synthesize_members(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    ty: TypeId,
) -> Option<SynthesizedTypeNode> {
    let properties = match &checker.get_type(ty).data {
        TypeData::Object(o) => o.properties.clone(),
        _ => return None,
    };
    let mut members = Vec::with_capacity(properties.len());
    for property in properties {
        // A synthesized (object-literal) property's name lives in the checker's
        // transient arena, and an `as const` member carries the `Readonly` check
        // flag; a program (interface/class) member reads its name from the
        // program. Mirrors [`serialize_members`].
        let (name, readonly, optional) =
            property_name_readonly_optional(checker, program, property);
        let property_type = printable_property_type(checker, program, property, optional);
        let type_node = type_to_type_node(checker, program, property_type)?;
        members.push(SynthesizedProperty {
            name,
            type_node,
            readonly,
            optional,
        });
    }
    Some(SynthesizedTypeNode::TypeLiteral(members))
}

// Prints a bare function/constructor type in arrow shorthand (Go's
// `typeToString` of an object type with exactly one call (or one construct)
// signature and no properties, index signatures, or other-kind signatures):
// `(x: T) => R` for a call signature, `new (x: T) => R` for a construct
// signature. Returns `None` when the type is not a bare function/constructor
// type, so the caller falls back to the `{ ... }` member literal.
//
// DEFER(phase-4-checker-C-A+): the `{ (x): R; new (): S; a: T; }` mixed form
// (an object carrying signatures alongside properties/index infos), overloaded
// signatures (multiple call/construct signatures), generic type parameters
// (`<T>(x: T) => T`), optional (`x?: T`) and rest (`...args: T[]`) parameters,
// and the `this` parameter. blocked-by: full signature-node serialization.
// Go: internal/checker/nodebuilderimpl.go (signatureToString / function type node)
fn serialize_signature_shorthand(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    ty: TypeId,
) -> Option<String> {
    let (properties, calls, constructs, index_infos) = match &checker.get_type(ty).data {
        TypeData::Object(o) => (
            o.properties.clone(),
            o.call_signatures.clone(),
            o.construct_signatures.clone(),
            o.index_infos.clone(),
        ),
        _ => return None,
    };
    if !properties.is_empty() || !index_infos.is_empty() {
        return None;
    }
    if calls.len() == 1 && constructs.is_empty() {
        return Some(serialize_signature(checker, program, calls[0], false));
    }
    if constructs.len() == 1 && calls.is_empty() {
        return Some(serialize_signature(checker, program, constructs[0], true));
    }
    None
}

// Prints one signature in arrow shorthand: `(p0: T0, p1: T1) => R`, prefixed
// with `new ` for a construct signature.
// Go: internal/checker/nodebuilderimpl.go (signatureToString)
fn serialize_signature(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    sig: super::signatures::SignatureId,
    is_construct: bool,
) -> String {
    let parameters = checker.signature(sig).parameters.clone();
    let return_type = checker
        .signature(sig)
        .resolved_return_type
        .unwrap_or_else(|| checker.any_type());
    let mut parts = Vec::with_capacity(parameters.len());
    for param in parameters {
        let name = program.symbol(param).name.clone();
        let param_type = get_type_of_symbol(checker, program, param, None);
        let printed = type_to_string(checker, program, param_type);
        parts.push(format!("{name}: {printed}"));
    }
    let return_str = type_to_string(checker, program, return_type);
    let prefix = if is_construct { "new " } else { "" };
    format!("{prefix}({}) => {return_str}", parts.join(", "))
}

fn property_name_readonly_optional(
    checker: &Checker,
    program: &dyn BoundProgram,
    property: SymbolId,
) -> (String, bool, bool) {
    if super::is_synthesized_symbol(property) {
        (
            checker.synthesized_symbol_name(property).to_string(),
            checker
                .synthesized_symbol_check_flags(property)
                .contains(tsgo_ast::CheckFlags::READONLY),
            checker
                .synthesized_symbol_flags(property)
                .contains(SymbolFlags::OPTIONAL),
        )
    } else {
        let sym = program.symbol(property);
        (
            sym.name.clone(),
            false,
            sym.flags.contains(SymbolFlags::OPTIONAL),
        )
    }
}

fn printable_property_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    property: SymbolId,
    optional: bool,
) -> TypeId {
    let property_type = get_type_of_symbol(checker, program, property, None);
    if optional {
        checker.get_type_with_facts(property_type, TypeFacts::NE_UNDEFINED)
    } else {
        property_type
    }
}

// Prints an anonymous object type's members as `{ a: A; b: B; }` (or `{}`).
// Go: internal/checker/nodebuilderimpl.go (type-literal member serialization)
fn serialize_members(checker: &mut Checker, program: &dyn BoundProgram, ty: TypeId) -> String {
    let properties = match &checker.get_type(ty).data {
        TypeData::Object(o) => o.properties.clone(),
        _ => return checker.type_to_string(ty),
    };
    if properties.is_empty() {
        return "{}".to_string();
    }
    let mut parts = Vec::with_capacity(properties.len());
    for property in properties {
        // An object-literal property is a checker-synthesized (transient)
        // symbol whose name lives in the checker's transient arena, not the
        // program (which would panic on the tagged id). A synthesized property
        // carries the `Readonly` check flag in a const context (`as const`), so
        // it prints with a leading `readonly ` (Go's `isReadonlySymbol`).
        //
        // DEFER(phase-4-checker-4bi+): the readonly modifier on a program (non
        // synthesized) member symbol (interface/class `readonly` field).
        // blocked-by: declaration-modifier readonly on bound symbols.
        let (name, readonly, optional) =
            property_name_readonly_optional(checker, program, property);
        let property_type = printable_property_type(checker, program, property, optional);
        let printed = type_to_string(checker, program, property_type);
        let prefix = if readonly { "readonly " } else { "" };
        let separator = if optional { "?: " } else { ": " };
        parts.push(format!("{prefix}{name}{separator}{printed}"));
    }
    format!("{{ {}; }}", parts.join("; "))
}

// Prints a fixed-arity tuple type as `[e0, e1]`, prefixed with `readonly ` when
// the tuple is readonly (an `[...] as const` tuple). The positional element
// types come from `resolved_type_arguments`, printed in order (Go's tuple type
// node serialization).
// Go: internal/checker/nodebuilderimpl.go (tuple type node) / typeToString
fn serialize_tuple(checker: &mut Checker, program: &dyn BoundProgram, ty: TypeId) -> String {
    let (elements, readonly) = match &checker.get_type(ty).data {
        TypeData::Object(o) => (o.resolved_type_arguments.clone(), o.readonly),
        _ => return checker.type_to_string(ty),
    };
    let parts: Vec<String> = elements
        .iter()
        .map(|&e| type_to_string(checker, program, e))
        .collect();
    let body = format!("[{}]", parts.join(", "));
    if readonly {
        format!("readonly {body}")
    } else {
        body
    }
}

#[cfg(test)]
#[path = "nodebuilder_test.rs"]
mod tests;
