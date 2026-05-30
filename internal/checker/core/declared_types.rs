//! Declared types and the type of value symbols.
//!
//! Ports the 4c subset of Go's declared-type machinery: building the declared
//! type of an interface/class/enum/type-alias symbol, resolving the type of a
//! value/property symbol from its annotation, mapping a type node to a type,
//! computing apparent types, and looking up properties on a type.
//!
//! These are free functions taking `&mut Checker` plus a [`BoundProgram`]
//! (rather than `Checker` methods) because the checker owns the type arena and
//! caches while the bound program owns the AST and symbols; Phase 6 merges them
//! when the real `Program` is integrated.

use tsgo_ast::{Kind, NodeData, SymbolFlags, SymbolId, SymbolTable};

use super::program::BoundProgram;
use super::symbols::resolve_name;
use super::types::{ObjectFlags, ObjectType, TypeData, TypeId};
use super::Checker;

/// Builds (or returns the cached) declared type of a type-introducing symbol.
///
/// Handles interface/class (an object type whose members come from the symbol's
/// `members` table), type alias (its right-hand-side type node), and enum (an
/// object type from the symbol's `exports`). Anything else yields the error
/// type, matching Go's `getDeclaredTypeOfSymbol` fallthrough.
///
/// DEFER(phase-4-checker-4d): type parameters, heritage/base types, the `this`
/// type, and generic instantiation. Enum's faithful declared type is a union of
/// member literal types (needs the evaluator), DEFER(phase-4-checker-4g).
/// blocked-by: instantiation/relations (4d); enum-member evaluation (4g).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_declared_type_of_symbol, BoundProgram};
/// use tsgo_ast::SymbolId;
/// // Generic over any bound program (type-checks without instantiation).
/// fn declared<P: BoundProgram>(c: &mut tsgo_checker::Checker, p: &P, s: SymbolId) {
///     let _ = get_declared_type_of_symbol(c, p, s, None);
/// }
/// ```
///
/// Side effects: may allocate a type and populate the declared-type caches.
// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfSymbol / tryGetDeclaredTypeOfSymbol
pub fn get_declared_type_of_symbol(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let flags = program.symbol(symbol).flags;
    if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
        return get_declared_type_of_class_or_interface(checker, program, symbol, globals);
    }
    if flags.contains(SymbolFlags::TYPE_ALIAS) {
        return get_declared_type_of_type_alias(checker, program, symbol, globals);
    }
    if flags.intersects(SymbolFlags::ENUM) {
        return get_declared_type_of_enum(checker, program, symbol);
    }
    if flags.contains(SymbolFlags::TYPE_PARAMETER) {
        return get_declared_type_of_type_parameter(checker, symbol);
    }
    // DEFER(phase-4-checker-4f+): enum members and non-local aliases.
    // blocked-by: enum-member evaluation (4g) and module alias resolution.
    checker.error_type()
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfTypeParameter
fn get_declared_type_of_type_parameter(checker: &mut Checker, symbol: SymbolId) -> TypeId {
    if let Some(cached) = checker
        .declared_type_links
        .try_get(&symbol)
        .and_then(|l| l.declared_type)
    {
        return cached;
    }
    let t = checker.new_type_parameter(Some(symbol));
    checker.declared_type_links.get(symbol).declared_type = Some(t);
    t
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfClassOrInterface
fn get_declared_type_of_class_or_interface(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    if let Some(cached) = checker
        .declared_type_links
        .try_get(&symbol)
        .and_then(|l| l.declared_type)
    {
        return cached;
    }
    let sym = program.symbol(symbol);
    let kind = if sym.flags.contains(SymbolFlags::CLASS) {
        ObjectFlags::CLASS
    } else {
        ObjectFlags::INTERFACE
    };
    let own_members = sym.members.clone();
    let declarations = sym.declarations.clone();

    // Local type parameters of a generic interface/class.
    let type_parameters = collect_local_type_parameters(checker, program, &declarations);
    // The synthesized `this` type parameter.
    let this_type = checker.new_type_parameter(Some(symbol));
    if let TypeData::TypeParameter(tp) = &mut checker.types.get_mut(this_type).data {
        tp.is_this_type = true;
    }

    // Allocate with own members first and cache it, so cyclic `extends`
    // resolution terminates.
    let properties: Vec<SymbolId> = own_members.values().copied().collect();
    let object = ObjectType {
        members: own_members.clone(),
        properties,
        type_parameters,
        this_type: Some(this_type),
        ..Default::default()
    };
    let t = checker.new_object_type(kind, Some(symbol), object);
    checker.declared_type_links.get(symbol).declared_type = Some(t);

    // Resolve and merge base (extends) members.
    let base_types = resolve_base_types(checker, program, &declarations, globals);
    if !base_types.is_empty() {
        let mut merged = SymbolTable::default();
        for &base in &base_types {
            for (name, &member) in resolve_structured_type_members(checker, base).iter() {
                merged.insert(name.clone(), member);
            }
        }
        // Own members override inherited ones.
        for (name, &member) in own_members.iter() {
            merged.insert(name.clone(), member);
        }
        if let Some(obj) = checker.types.get_mut(t).as_object_mut() {
            obj.properties = merged.values().copied().collect();
            obj.members = merged;
            obj.base_types = base_types;
        }
    }
    t
}

// Creates a type parameter per `<...>` declared on the symbol's declarations.
// Go: internal/checker/checker.go:Checker.appendLocalTypeParametersOfClassOrInterfaceOrTypeAlias
fn collect_local_type_parameters(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declarations: &[tsgo_ast::NodeId],
) -> Vec<TypeId> {
    let mut result = Vec::new();
    for &decl in declarations {
        let type_param_nodes = match program.arena().data(decl) {
            NodeData::InterfaceDeclaration(d)
            | NodeData::ClassDeclaration(d)
            | NodeData::ClassExpression(d) => d.type_parameters.clone(),
            _ => None,
        };
        if let Some(list) = type_param_nodes {
            for node in list.nodes {
                match program.symbol_of_node(node) {
                    Some(sym) => result.push(get_declared_type_of_type_parameter(checker, sym)),
                    None => result.push(checker.new_type_parameter(None)),
                }
            }
        }
    }
    result
}

// Resolves the `extends` heritage of an interface/class to base type ids.
// Go: internal/checker/checker.go:Checker.getBaseTypes (extends portion)
fn resolve_base_types(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declarations: &[tsgo_ast::NodeId],
    globals: Option<&SymbolTable>,
) -> Vec<TypeId> {
    let mut bases = Vec::new();
    for &decl in declarations {
        let heritage = match program.arena().data(decl) {
            NodeData::InterfaceDeclaration(d)
            | NodeData::ClassDeclaration(d)
            | NodeData::ClassExpression(d) => d.heritage_clauses.clone(),
            _ => None,
        };
        let Some(clauses) = heritage else { continue };
        for clause in clauses.nodes {
            let (token, types) = match program.arena().data(clause) {
                NodeData::HeritageClause(h) => (h.token, h.types.nodes.clone()),
                _ => continue,
            };
            if token != Kind::ExtendsKeyword {
                continue;
            }
            for type_node in types {
                let expression = match program.arena().data(type_node) {
                    NodeData::ExpressionWithTypeArguments(e) => e.expression,
                    _ => continue,
                };
                if program.arena().kind(expression) != Kind::Identifier {
                    continue;
                }
                let name = program.arena().text(expression).to_string();
                if let Some(base_symbol) = resolve_name(
                    program,
                    expression,
                    &name,
                    SymbolFlags::TYPE,
                    false,
                    globals,
                ) {
                    bases.push(get_declared_type_of_symbol(
                        checker,
                        program,
                        base_symbol,
                        globals,
                    ));
                }
            }
        }
    }
    bases
}

/// Returns the resolved member table of a structured type.
///
/// For an object/interface type this is its (already base-merged) members; for
/// a type reference (`Foo<...>`) it is the target's members. Mirrors Go's
/// `resolveStructuredTypeMembers` for the cases 4d builds.
///
/// DEFER(phase-4-checker-4e): per-reference member-type instantiation and
/// index-signature collection.
/// blocked-by: member-type instantiation (4e).
///
/// # Examples
/// ```
/// use tsgo_checker::{resolve_structured_type_members, Checker};
/// let c = Checker::new();
/// // A primitive has no structured members.
/// assert!(resolve_structured_type_members(&c, c.string_type()).is_empty());
/// ```
///
/// Side effects: none (reads the already-resolved member tables).
// Go: internal/checker/checker.go:Checker.resolveStructuredTypeMembers
pub fn resolve_structured_type_members(checker: &Checker, t: TypeId) -> SymbolTable {
    if let Some(obj) = checker.get_type(t).as_object() {
        if let Some(target) = obj.target {
            return resolve_structured_type_members(checker, target);
        }
        return obj.members.clone();
    }
    SymbolTable::default()
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfTypeAlias
fn get_declared_type_of_type_alias(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    if let Some(cached) = checker
        .type_alias_links
        .try_get(&symbol)
        .and_then(|l| l.declared_type)
    {
        return cached;
    }
    let type_node = program
        .symbol(symbol)
        .declarations
        .iter()
        .find_map(|&decl| match program.arena().data(decl) {
            NodeData::TypeAliasDeclaration(d) => Some(d.type_node),
            _ => None,
        });
    let t = match type_node {
        Some(node) => get_type_from_type_node(checker, program, node, globals),
        None => checker.error_type(),
    };
    checker.type_alias_links.get(symbol).declared_type = Some(t);
    t
}

// Builds a members-bearing object type for an enum from its `exports` table.
//
// DEFER(phase-4-checker-4g): Go's enum declared type is a union of the member
// literal types, which needs constant-value evaluation. 4c models it as an
// object type so `E.member` resolves to the member symbol.
// blocked-by: enum-member constant evaluation (`evaluator`) wires in at 4g.
// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfEnum
fn get_declared_type_of_enum(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> TypeId {
    if let Some(cached) = checker
        .declared_type_links
        .try_get(&symbol)
        .and_then(|l| l.declared_type)
    {
        return cached;
    }
    let members = program.symbol(symbol).exports.clone();
    let properties: Vec<SymbolId> = members.values().copied().collect();
    let object = ObjectType {
        members,
        properties,
        ..Default::default()
    };
    let t = checker.new_object_type(ObjectFlags::ANONYMOUS, Some(symbol), object);
    checker.declared_type_links.get(symbol).declared_type = Some(t);
    t
}

/// Returns the type of a value/property symbol, resolving its annotation.
///
/// For a variable/property with a type annotation, this is the type of that
/// annotation; without one, 4c returns `any` (initializer inference is later).
/// A class/interface/enum symbol falls back to its declared type.
///
/// DEFER(phase-4-checker-4g): initializer-based inference for un-annotated
/// declarations; the constructor/static-side type of a class value symbol.
/// blocked-by: expression checking (4g).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_type_of_symbol, BoundProgram};
/// use tsgo_ast::SymbolId;
/// fn type_of<P: BoundProgram>(c: &mut tsgo_checker::Checker, p: &P, s: SymbolId) {
///     let _ = get_type_of_symbol(c, p, s, None);
/// }
/// ```
///
/// Side effects: may allocate types and populate the value-symbol cache.
// Go: internal/checker/checker.go:Checker.getTypeOfSymbol
pub fn get_type_of_symbol(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let flags = program.symbol(symbol).flags;
    if flags.intersects(SymbolFlags::VARIABLE | SymbolFlags::PROPERTY) {
        return get_type_of_variable_or_property(checker, program, symbol, globals);
    }
    if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE | SymbolFlags::ENUM) {
        return get_declared_type_of_symbol(checker, program, symbol, globals);
    }
    // DEFER(phase-4-checker-4g): function/method/accessor/alias/module value
    // types. blocked-by: signature resolution (4d) and expression checking (4g).
    checker.error_type()
}

// Go: internal/checker/checker.go:Checker.getTypeOfVariableOrParameterOrProperty
fn get_type_of_variable_or_property(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    if let Some(cached) = checker
        .value_symbol_links
        .try_get(&symbol)
        .and_then(|l| l.resolved_type)
    {
        return cached;
    }
    let sym = program.symbol(symbol);
    let declaration = sym
        .value_declaration
        .or_else(|| sym.declarations.first().copied());
    let type_node = declaration.and_then(|decl| match program.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node,
        NodeData::PropertySignature(d) | NodeData::PropertyDeclaration(d) => d.type_node,
        _ => None,
    });
    let t = match type_node {
        Some(node) => get_type_from_type_node(checker, program, node, globals),
        // DEFER(phase-4-checker-4g): infer from initializer when un-annotated.
        // blocked-by: expression checking (4g).
        None => checker.any_type(),
    };
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

/// Maps a type node to a type.
///
/// 4c handles the primitive keyword types and (identifier) type references; all
/// other type-node shapes yield the error type.
///
/// DEFER(phase-4-checker-4d): array/tuple/union/intersection/function/mapped/
/// conditional/literal/qualified-name type nodes and type arguments.
/// blocked-by: those type constructors land across 4d+.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_type_from_type_node, BoundProgram};
/// use tsgo_ast::NodeId;
/// fn from_node<P: BoundProgram>(c: &mut tsgo_checker::Checker, p: &P, n: NodeId) {
///     let _ = get_type_from_type_node(c, p, n, None);
/// }
/// ```
///
/// Side effects: may build declared types for referenced symbols.
// Go: internal/checker/checker.go:Checker.getTypeFromTypeNode
pub fn get_type_from_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    match program.arena().kind(node) {
        Kind::StringKeyword => checker.string_type(),
        Kind::NumberKeyword => checker.number_type(),
        Kind::BooleanKeyword => checker.boolean_type(),
        Kind::BigIntKeyword => checker.bigint_type(),
        Kind::SymbolKeyword => checker.es_symbol_type(),
        Kind::AnyKeyword => checker.any_type(),
        Kind::UnknownKeyword => checker.unknown_type(),
        Kind::VoidKeyword => checker.void_type(),
        Kind::UndefinedKeyword => checker.undefined_type(),
        Kind::NeverKeyword => checker.never_type(),
        Kind::ObjectKeyword => checker.non_primitive_type(),
        Kind::TypeReference => get_type_from_type_reference(checker, program, node, globals),
        // DEFER(phase-4-checker-4d): remaining type-node kinds.
        // blocked-by: their type constructors land across 4d+.
        _ => checker.error_type(),
    }
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeReference
fn get_type_from_type_reference(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let type_name = match program.arena().data(node) {
        NodeData::TypeReference(d) => d.type_name,
        _ => return checker.error_type(),
    };
    if program.arena().kind(type_name) != Kind::Identifier {
        // DEFER(phase-4-checker-4d): qualified-name type references.
        // blocked-by: namespace member resolution (4d).
        return checker.error_type();
    }
    let type_arguments = match program.arena().data(node) {
        NodeData::TypeReference(d) => d.type_arguments.clone(),
        _ => None,
    };
    let name = program.arena().text(type_name).to_string();
    let symbol = match resolve_name(program, node, &name, SymbolFlags::TYPE, false, globals) {
        Some(symbol) => symbol,
        None => return checker.error_type(),
    };
    let target = get_declared_type_of_symbol(checker, program, symbol, globals);
    // `Foo<A, B>` becomes a generic type reference; `Foo` stays the bare target.
    if let Some(list) = type_arguments {
        if !list.nodes.is_empty() {
            let args: Vec<TypeId> = list
                .nodes
                .iter()
                .map(|&arg| get_type_from_type_node(checker, program, arg, globals))
                .collect();
            return checker.create_type_reference(target, args);
        }
    }
    target
}

/// Returns the apparent type of `t`.
///
/// 4c returns `t` unchanged: object/union types are already their own apparent
/// type, and the primitive-to-wrapper mapping (`string` -> the global `String`
/// interface, etc.) needs library globals that are not yet loaded.
///
/// DEFER(phase-4-checker-4j): map primitive/index/instantiable types to their
/// apparent forms (global wrapper interfaces, base constraints).
/// blocked-by: library globals (lib.d.ts loading, P6) and base-constraint
/// resolution (4d).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_apparent_type, Checker};
/// let c = Checker::new();
/// assert_eq!(get_apparent_type(&c, c.string_type()), c.string_type());
/// ```
///
/// Side effects: none (pure in 4c).
// Go: internal/checker/checker.go:Checker.getApparentType
pub fn get_apparent_type(checker: &Checker, t: TypeId) -> TypeId {
    let _ = checker;
    t
}

/// Looks up the property named `name` on type `t`, returning its symbol.
///
/// Resolves through [`get_apparent_type`] and, for object types, the type's
/// member table. This replaces 4b's structural member lookup with a real
/// type-backed property path.
///
/// Inherited (`extends`) members resolve too, because base members are merged
/// into a derived interface's member table at declared-type construction, and a
/// generic type reference (`Foo<...>`) delegates to its target's members.
///
/// DEFER(phase-4-checker-4e+): synthetic union/intersection properties,
/// index-signature lookup, and `Object`/`Function` global augmentation.
/// blocked-by: union-property synthesis (4e) and library globals (P6).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_property_of_type, Checker};
/// let c = Checker::new();
/// // A primitive intrinsic has no own members in 4d.
/// assert_eq!(get_property_of_type(&c, c.string_type(), "length"), None);
/// ```
///
/// Side effects: none (pure read over the type arena).
// Go: internal/checker/checker.go:Checker.getPropertyOfType
pub fn get_property_of_type(checker: &Checker, t: TypeId, name: &str) -> Option<SymbolId> {
    let apparent = get_apparent_type(checker, t);
    let obj = checker.get_type(apparent).as_object()?;
    match obj.target {
        Some(target) => get_property_of_type(checker, target, name),
        None => obj.members.get(name).copied(),
    }
}

/// Returns the type of the property named `name` on type `t`.
///
/// For a generic type reference (`Foo<string>`), the property's declared type
/// is instantiated through the reference's `type parameters -> type arguments`
/// mapper, so e.g. a `value: T` member of `Box<string>` yields `string`. Builds
/// on 4d's `create_type_reference`.
///
/// DEFER(phase-4-checker-4f+): union/intersection property-type synthesis and
/// index-signature value types.
/// blocked-by: union-property synthesis (later) and index signatures.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_type_of_property_of_type, BoundProgram, Checker, TypeId};
/// fn demo<P: BoundProgram>(c: &mut Checker, p: &P, t: TypeId) {
///     let _ = get_type_of_property_of_type(c, p, t, "value");
/// }
/// ```
///
/// Side effects: may build the property type and instantiate it.
// Go: internal/checker/checker.go:Checker.getTypeOfPropertyOfType
pub fn get_type_of_property_of_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
    name: &str,
) -> Option<TypeId> {
    let prop = get_property_of_type(checker, t, name)?;
    let prop_type = get_type_of_symbol(checker, program, prop, None);
    // Instantiate through the reference's type-argument mapper, if any.
    let reference = checker.get_type(t).as_object().and_then(|o| {
        o.target
            .map(|target| (target, o.resolved_type_arguments.clone()))
    });
    if let Some((target, args)) = reference {
        let params = checker
            .get_type(target)
            .as_object()
            .map(|o| o.type_parameters.clone())
            .unwrap_or_default();
        if !params.is_empty() && params.len() == args.len() {
            let mapper = super::mapper::TypeMapper::Array {
                sources: params,
                targets: args,
            };
            return Some(checker.instantiate_type(prop_type, &mapper));
        }
    }
    Some(prop_type)
}

/// Returns the resolved properties of a type as `(name, symbol)` pairs.
///
/// Used by the relation engine to iterate a type's members; delegates through
/// [`resolve_structured_type_members`] (so references and inherited members are
/// included).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_properties_of_type, Checker};
/// let c = Checker::new();
/// assert!(get_properties_of_type(&c, c.string_type()).is_empty());
/// ```
///
/// Side effects: none (pure read over the type arena).
// Go: internal/checker/checker.go:Checker.getPropertiesOfType
pub fn get_properties_of_type(checker: &Checker, t: TypeId) -> Vec<(String, SymbolId)> {
    let apparent = get_apparent_type(checker, t);
    resolve_structured_type_members(checker, apparent)
        .into_iter()
        .collect()
}

/// Resolves and builds the global type named `name` from the `globals` table.
///
/// Looks the name up among the globals, and (if it introduces a type) builds and
/// caches its declared type. This is the wired-up form of the 4a placeholder.
///
/// DEFER(phase-4-checker-P6): the real `getGlobalType` reports an error and has
/// arity checking; it also relies on `globals` being populated from lib.d.ts.
/// blocked-by: library globals loading (P6) and diagnostics (4g).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_global_type, BoundProgram};
/// use tsgo_ast::SymbolTable;
/// fn global<P: BoundProgram>(c: &mut tsgo_checker::Checker, p: &P, g: &SymbolTable) {
///     let _ = get_global_type(c, p, "Array", g);
/// }
/// ```
///
/// Side effects: may build a declared type and populate the global-type cache.
// Go: internal/checker/checker.go:Checker.getGlobalType
pub fn get_global_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    name: &str,
    globals: &SymbolTable,
) -> Option<TypeId> {
    if let Some(&cached) = checker.global_types.get(name) {
        return Some(cached);
    }
    let symbol = *globals.get(name)?;
    if !program.symbol(symbol).flags.intersects(SymbolFlags::TYPE) {
        return None;
    }
    let t = get_declared_type_of_symbol(checker, program, symbol, Some(globals));
    checker.global_types.insert(name.to_string(), t);
    Some(t)
}

#[cfg(test)]
#[path = "declared_types_test.rs"]
mod tests;
