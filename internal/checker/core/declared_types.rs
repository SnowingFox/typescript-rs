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

use rustc_hash::FxHashMap;

use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_INDEX;
use tsgo_ast::{
    CheckFlags, Kind, ModifierFlags, NodeArena, NodeData, NodeId, SymbolFlags, SymbolId,
    SymbolTable,
};

use super::program::BoundProgram;
use super::signatures::{IndexInfo, IndexInfoId, Signature, SignatureFlags, SignatureId};
use super::symbols::resolve_name;
use super::types::{LiteralValue, ObjectFlags, ObjectType, TypeData, TypeFlags, TypeId};
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
            obj.base_types = base_types.clone();
        }
    }

    // Late-bind well-known-symbol computed members (`[Symbol.iterator]`) that
    // the binder bound anonymously as `__computed` (they are attached to the
    // member node, not the interface's member table).
    let late_members =
        collect_late_bound_well_known_members(checker, program, &declarations, globals);
    if !late_members.is_empty() {
        if let Some(obj) = checker.types.get_mut(t).as_object_mut() {
            for (name, member) in late_members {
                if obj.members.insert(name, member).is_none() {
                    obj.properties.push(member);
                }
            }
        }
    }

    // Own index signatures plus any inherited from `extends` clauses.
    let type_param_map = build_type_parameter_name_map(checker, program, &declarations);
    let mut index_infos =
        collect_index_infos_of_members(checker, program, &own_members, &type_param_map, globals);
    for &base in &base_types {
        for id in get_index_infos_of_structured_type(checker, base) {
            if !index_infos.iter().any(|&existing| {
                checker.index_info(existing).key_type == checker.index_info(id).key_type
            }) {
                index_infos.push(id);
            }
        }
    }
    if let Some(obj) = checker.types.get_mut(t).as_object_mut() {
        obj.index_infos = index_infos;
    }
    t
}

// Collects the late-bound well-known-symbol members declared across an
// interface/class symbol's declarations, paired with the late-bound internal
// name they resolve under (`[Symbol.iterator]` -> `__@iterator`).
//
// 4ag subset of Go's `getResolvedMembersOrExportsOfSymbol`/`lateBindMember`:
// it handles only the syntactic well-known-symbol form (a computed name whose
// expression is `Symbol.<name>` on the global `Symbol`) and reuses the binder's
// anonymous `__computed` member symbol rather than minting a fresh late symbol.
//
// DEFER(phase-4-checker-4ah+): minting a distinct late-bound symbol (Go's
// `newSymbolEx(..., CheckFlagsLate)`), accessor merging, conflict diagnostics
// (`Duplicate_identifier_0`), static-vs-instance split, index-signature
// late-binding, and the `unique symbol` typing path (so the name carries the
// `@<id>` suffix). blocked-by: late-bound symbol arena + unique-ES-symbol types.
// Go: internal/checker/checker.go:Checker.getResolvedMembersOrExportsOfSymbol / lateBindMember
fn collect_late_bound_well_known_members(
    checker: &Checker,
    program: &dyn BoundProgram,
    declarations: &[NodeId],
    globals: Option<&SymbolTable>,
) -> Vec<(String, SymbolId)> {
    let arena = program.arena();
    let mut result = Vec::new();
    for &decl in declarations {
        let member_nodes = match arena.data(decl) {
            NodeData::InterfaceDeclaration(d)
            | NodeData::ClassDeclaration(d)
            | NodeData::ClassExpression(d) => d.members.nodes.clone(),
            _ => continue,
        };
        for member in member_nodes {
            let Some(name_node) = member_name_node(arena, member) else {
                continue;
            };
            if arena.kind(name_node) != Kind::ComputedPropertyName {
                continue;
            }
            let expr = match arena.data(name_node) {
                NodeData::ComputedPropertyName(d) => d.expression,
                _ => continue,
            };
            let Some(symbol_name) = well_known_symbol_name(program, arena, expr, globals) else {
                continue;
            };
            // The member's symbol is the binder's anonymous `__computed` symbol.
            let Some(member_symbol) = program.symbol_of_node(member) else {
                continue;
            };
            let late_name = checker.get_property_name_for_known_symbol_name(&symbol_name);
            result.push((late_name, member_symbol));
        }
    }
    result
}

// Returns the name node of an interface/class/type-literal member declaration,
// if it has one.
// Go: internal/ast/utilities.go:getNameOfDeclaration (member subset)
fn member_name_node(arena: &NodeArena, member: NodeId) -> Option<NodeId> {
    match arena.data(member) {
        NodeData::MethodSignature(d) => Some(d.name),
        NodeData::PropertySignature(d) => Some(d.name),
        NodeData::MethodDeclaration(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) => Some(d.name),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => Some(d.name),
        _ => None,
    }
}

// Reports the well-known symbol name (`iterator`) when `expr` is a property
// access `Symbol.<name>` whose `Symbol` identifier resolves to the global
// `Symbol` value, so a synthetic `declare var Symbol: SymbolConstructor` is what
// drives the late-binding. Returns `None` otherwise.
//
// 4ag subset of Go's late-binding gate: Go reaches the same outcome via
// `checkComputedPropertyName` producing a `unique symbol`/literal property-name
// type, which requires `Symbol` to be the genuine global; here we model that
// requirement syntactically plus the global-`Symbol` identity check.
// Go: internal/checker/checker.go:Checker.isSymbolOrSymbolForCall (global-`Symbol` check)
fn well_known_symbol_name(
    program: &dyn BoundProgram,
    arena: &NodeArena,
    expr: NodeId,
    globals: Option<&SymbolTable>,
) -> Option<String> {
    if arena.kind(expr) != Kind::PropertyAccessExpression {
        return None;
    }
    let (object, name) = match arena.data(expr) {
        NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
        _ => return None,
    };
    if arena.kind(object) != Kind::Identifier
        || arena.text(object) != "Symbol"
        || arena.kind(name) != Kind::Identifier
    {
        return None;
    }
    // Make sure `Symbol` is the global symbol (Go's `isSymbolOrSymbolForCall`):
    // the identifier must resolve to the same value symbol the globals expose.
    let global_symbol = globals.and_then(|g| g.get("Symbol")).copied()?;
    if !program
        .symbol(global_symbol)
        .flags
        .intersects(SymbolFlags::VALUE)
    {
        return None;
    }
    let resolved = resolve_name(
        program,
        object,
        "Symbol",
        SymbolFlags::VALUE,
        false,
        globals,
    );
    if resolved != Some(global_symbol) {
        return None;
    }
    Some(arena.text(name).to_string())
}

// Maps each local type parameter name (`T` in `interface Foo<T>`) to its type id.
fn build_type_parameter_name_map(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declarations: &[NodeId],
) -> FxHashMap<String, TypeId> {
    let mut map = FxHashMap::default();
    for &decl in declarations {
        let type_param_nodes = match program.arena().data(decl) {
            NodeData::InterfaceDeclaration(d)
            | NodeData::ClassDeclaration(d)
            | NodeData::ClassExpression(d) => d.type_parameters.clone(),
            _ => None,
        };
        if let Some(list) = type_param_nodes {
            for node in list.nodes {
                if let Some(sym) = program.symbol_of_node(node) {
                    let name = program.symbol(sym).name.clone();
                    map.insert(name, get_declared_type_of_type_parameter(checker, sym));
                }
            }
        }
    }
    map
}

// Resolves a type node, substituting in-scope type parameters by name when the
// node is a bare `T` reference inside a generic interface/class body.
fn get_type_from_type_node_with_type_params(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
    type_params: &FxHashMap<String, TypeId>,
    globals: Option<&SymbolTable>,
) -> TypeId {
    if program.arena().kind(node) == Kind::TypeReference {
        if let NodeData::TypeReference(d) = program.arena().data(node) {
            if program.arena().kind(d.type_name) == Kind::Identifier {
                let name = program.arena().text(d.type_name).to_string();
                if let Some(&tp) = type_params.get(&name) {
                    return tp;
                }
            }
        }
    }
    get_type_from_type_node(checker, program, node, globals)
}

// Builds [`IndexInfo`] entries from the interface/class index-symbol member.
// Go: internal/checker/checker.go:Checker.getIndexInfosOfIndexSymbol
fn collect_index_infos_of_members(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    members: &SymbolTable,
    type_params: &FxHashMap<String, TypeId>,
    globals: Option<&SymbolTable>,
) -> Vec<IndexInfoId> {
    let Some(&index_symbol) = members.get(INTERNAL_SYMBOL_NAME_INDEX) else {
        return Vec::new();
    };
    let mut index_infos = Vec::new();
    for &decl in &program.symbol(index_symbol).declarations {
        if program.arena().kind(decl) != Kind::IndexSignature {
            continue;
        }
        let (param_nodes, type_node, modifiers) = match program.arena().data(decl) {
            NodeData::IndexSignatureDeclaration(d) => {
                (&d.parameters.nodes, d.type_node, d.modifiers.as_ref())
            }
            _ => continue,
        };
        if param_nodes.len() != 1 {
            continue;
        }
        let key_type_node = match program.arena().data(param_nodes[0]) {
            NodeData::ParameterDeclaration(d) => d.type_node,
            _ => None,
        };
        let Some(key_node) = key_type_node else {
            continue;
        };
        let key_type = get_type_from_type_node_with_type_params(
            checker,
            program,
            key_node,
            type_params,
            globals,
        );
        let value_type = match type_node {
            Some(node) => get_type_from_type_node_with_type_params(
                checker,
                program,
                node,
                type_params,
                globals,
            ),
            None => checker.any_type(),
        };
        let is_readonly = modifiers
            .map(|m| m.modifier_flags.contains(ModifierFlags::READONLY))
            .unwrap_or(false);
        if index_infos
            .iter()
            .any(|&existing| checker.index_info(existing).key_type == key_type)
        {
            continue;
        }
        let mut info = IndexInfo::new(key_type, value_type, is_readonly);
        info.declaration = Some(decl);
        index_infos.push(checker.new_index_info(info));
    }
    index_infos
}

// Returns index signatures declared on a structured type, instantiating through
// generic type references when needed.
// Go: internal/checker/checker.go:Checker.getIndexInfosOfStructuredType
fn get_index_infos_of_structured_type(checker: &mut Checker, t: TypeId) -> Vec<IndexInfoId> {
    let Some(obj) = checker.get_type(t).as_object().cloned() else {
        return Vec::new();
    };
    if let Some(target) = obj.target {
        let params = checker
            .get_type(target)
            .as_object()
            .map(|o| o.type_parameters.clone())
            .unwrap_or_default();
        let args = obj.resolved_type_arguments.clone();
        let target_infos = get_index_infos_of_structured_type(checker, target);
        if !params.is_empty() && params.len() == args.len() {
            return instantiate_index_infos(
                checker,
                &target_infos,
                &super::mapper::TypeMapper::Array {
                    sources: params,
                    targets: args,
                },
            );
        }
        return target_infos;
    }
    let mut infos = obj.index_infos.clone();
    for &base in &obj.base_types {
        for id in get_index_infos_of_structured_type(checker, base) {
            if !infos.iter().any(|&existing| {
                checker.index_info(existing).key_type == checker.index_info(id).key_type
            }) {
                infos.push(id);
            }
        }
    }
    infos
}

// Applies a mapper to each index signature's value type.
// Go: internal/checker/checker.go:Checker.instantiateIndexInfo
fn instantiate_index_infos(
    checker: &mut Checker,
    infos: &[IndexInfoId],
    mapper: &super::mapper::TypeMapper,
) -> Vec<IndexInfoId> {
    infos
        .iter()
        .map(|&id| {
            let (key_type, value_type, is_readonly) = {
                let info = checker.index_info(id);
                (info.key_type, info.value_type, info.is_readonly)
            };
            let instantiated_value = checker.instantiate_type(value_type, mapper);
            checker.new_index_info(IndexInfo::new(key_type, instantiated_value, is_readonly))
        })
        .collect()
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
    // A synthesized union/intersection property carries its combined type in the
    // checker's transient arena, not in the program (Go's transient symbol with
    // `valueSymbolLinks.resolvedType`).
    if super::is_synthesized_symbol(symbol) {
        return get_type_of_synthesized_symbol(checker, program, symbol, globals);
    }
    let flags = program.symbol(symbol).flags;
    if flags.intersects(SymbolFlags::VARIABLE | SymbolFlags::PROPERTY) {
        return get_type_of_variable_or_property(checker, program, symbol, globals);
    }
    if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE | SymbolFlags::ENUM) {
        return get_declared_type_of_symbol(checker, program, symbol, globals);
    }
    // A function or method symbol's type is an anonymous object type carrying
    // its call signatures (Go's `getTypeOfFuncClassEnumModule`), so a call
    // expression can resolve those signatures (4q), and the iterator protocol
    // (4ah) can read a `[Symbol.iterator]()`/`next()` method's return type.
    if flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD) {
        return get_type_of_func_class_enum_module(checker, program, symbol);
    }
    // DEFER(phase-4-checker-4q+): accessor/class-value/alias/module value types.
    // blocked-by: accessor signature collection, the constructor/static-side
    // type of a class value symbol, and alias/module resolution.
    checker.error_type()
}

// Builds (or returns the cached) type of a function symbol: an anonymous object
// type whose call signatures come from the symbol's function-like declarations
// (Go's `getTypeOfFuncClassEnumModule` -> `resolveAnonymousTypeMembers` for the
// function case, where `d.signatures = getSignaturesOfSymbol(symbol)`).
//
// 4q resolves the signatures eagerly at construction rather than lazily on
// member resolution; the call/property surfaces only read `call_signatures`.
//
// DEFER(phase-4-checker-4q+): class (construct signatures + static side),
// enum, and value-module symbols; the optional-property `undefined` widening.
// blocked-by: class construct-signature collection + enum/module value types +
// `strictNullChecks` wiring.
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModule
fn get_type_of_func_class_enum_module(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> TypeId {
    if let Some(cached) = checker
        .value_symbol_links
        .try_get(&symbol)
        .and_then(|l| l.resolved_type)
    {
        return cached;
    }
    let declarations = program.symbol(symbol).declarations.clone();
    let call_signatures = get_signatures_of_symbol(checker, program, &declarations);
    let object = ObjectType {
        call_signatures,
        ..Default::default()
    };
    let t = checker.new_object_type(ObjectFlags::ANONYMOUS, Some(symbol), object);
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

// Returns the call signatures declared by a symbol's function-like declarations
// (Go's `getSignaturesOfSymbol`).
//
// DEFER(phase-4-checker-4q+): overload-implementation de-duplication (skipping
// the body node when multiple declarations are present), and method/accessor/
// function-expression/arrow/constructor declarations. blocked-by: overload
// resolution + those declaration kinds.
// Go: internal/checker/checker.go:Checker.getSignaturesOfSymbol
fn get_signatures_of_symbol(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declarations: &[NodeId],
) -> Vec<SignatureId> {
    let mut result = Vec::new();
    for &decl in declarations {
        // A method member contributes its `MethodSignature`/`MethodDeclaration`
        // call signature (4ah), alongside the function-declaration case (4q).
        if !matches!(
            program.arena().kind(decl),
            Kind::FunctionDeclaration | Kind::MethodSignature | Kind::MethodDeclaration
        ) {
            continue;
        }
        result.push(get_signature_from_declaration(checker, program, decl));
    }
    result
}

// Builds a [`Signature`] from a function-like declaration (Go's
// `getSignatureFromDeclaration`): collects the parameter symbols, the minimum
// required argument count, and the return type (from its annotation).
//
// DEFER(phase-4-checker-4q+): optional/rest parameters' arity contribution is
// handled in the arity slices via `min_argument_count`; here 4q records the
// per-parameter required tracking. Type parameters (generic signatures), the
// `this` parameter, constructor/construct flags, and full-signature (JSDoc)
// types are deferred. Body-based return-type inference is deferred (an
// un-annotated function yields `any`). blocked-by: generics call-site inference,
// `this`-typing, construct signatures, and return-type-from-body inference.
// Go: internal/checker/checker.go:Checker.getSignatureFromDeclaration
fn get_signature_from_declaration(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declaration: NodeId,
) -> SignatureId {
    let (param_nodes, return_type_node) = match program.arena().data(declaration) {
        NodeData::FunctionDeclaration(d) => (d.parameters.nodes.clone(), d.type_node),
        // A method signature/declaration contributes its parameters and return
        // type the same way (4ah), so `[Symbol.iterator]()`/`next()` resolve.
        NodeData::MethodSignature(d) => (d.parameters.nodes.clone(), d.type_node),
        NodeData::MethodDeclaration(d) => (d.parameters.nodes.clone(), d.type_node),
        // DEFER(phase-4-checker-4q+): accessor/function-expression/arrow/
        // constructor declarations. blocked-by: those declaration kinds.
        _ => (Vec::new(), None),
    };
    let mut parameters = Vec::with_capacity(param_nodes.len());
    let mut min_argument_count = 0i32;
    for &param in &param_nodes {
        if let Some(sym) = program.symbol_of_node(param) {
            parameters.push(sym);
        }
        // Record a new minimum argument count for each non-optional parameter,
        // so a trailing optional/`?`/initializer/rest parameter lowers the
        // minimum (Go: `if !isOptionalParameter { minArgumentCount = len(parameters) }`).
        if !is_optional_parameter(program, param) {
            min_argument_count = parameters.len() as i32;
        }
    }
    // The return type comes from the annotation; an un-annotated function defers
    // body-based inference and yields `any`.
    let resolved_return_type = match return_type_node {
        Some(node) => get_type_from_type_node(checker, program, node, None),
        None => checker.any_type(),
    };
    let mut signature = Signature::new(SignatureFlags::NONE);
    signature.declaration = Some(declaration);
    signature.parameters = parameters;
    signature.min_argument_count = min_argument_count;
    signature.resolved_return_type = Some(resolved_return_type);
    checker.new_signature(signature)
}

// Reports whether a parameter declaration is optional for arity purposes (Go's
// `isOptionalParameter` subset): a `?` token, a default initializer, or a rest
// (`...`) parameter makes it optional.
//
// DEFER(phase-4-checker-4q+): the IIFE-argument and `void`-type optionality
// rules. blocked-by: IIFE detection + `void`-accepting parameter trimming.
// Go: internal/checker/checker.go:Checker.isOptionalParameter
fn is_optional_parameter(program: &dyn BoundProgram, param: NodeId) -> bool {
    match program.arena().data(param) {
        NodeData::ParameterDeclaration(d) => {
            d.question_token.is_some() || d.initializer.is_some() || d.dot_dot_dot_token.is_some()
        }
        _ => false,
    }
}

// Resolves (and caches) the combined type of a synthesized union/intersection
// property: the union (for a union containing type) or intersection (for an
// intersection containing type) of the property's type across the constituents
// that declare it.
//
// DIVERGENCE(port): Go computes this eagerly in
// `createUnionOrIntersectionProperty` (it has `*Checker`); here it is computed
// lazily because the minting entry point `get_property_of_type` is `&Checker`.
// The lazy result is identical (keyed by the synthesized symbol).
// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty
// (links.resolvedType = getUnionType/getIntersectionType(propTypes))
fn get_type_of_synthesized_symbol(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    if let Some(cached) = checker.synthesized_symbol_resolved_type(symbol) {
        return cached;
    }
    let containing = checker.synthesized_symbol_containing_type(symbol);
    let name = checker.synthesized_symbol_name(symbol);
    let is_union = checker
        .get_type(containing)
        .flags()
        .contains(TypeFlags::UNION);
    let constituents: Vec<TypeId> = if is_union {
        checker
            .get_type(containing)
            .union_types()
            .unwrap_or(&[])
            .to_vec()
    } else {
        checker
            .get_type(containing)
            .intersection_types()
            .unwrap_or(&[])
            .to_vec()
    };
    let mut prop_types: Vec<TypeId> = Vec::new();
    for member in constituents {
        if let Some(prop) = get_property_of_type(checker, member, &name) {
            prop_types.push(get_type_of_symbol(checker, program, prop, globals));
        }
    }
    let resolved = if is_union {
        checker.get_union_type(&prop_types)
    } else {
        checker.get_intersection_type(&prop_types)
    };
    checker.set_synthesized_symbol_resolved_type(symbol, resolved);
    resolved
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
        // A parameter symbol's type comes from its annotation (Go's
        // `getTypeOfParameter` -> `getTypeOfSymbol` -> annotation), so a
        // signature's parameter types resolve for call checking (4q).
        NodeData::ParameterDeclaration(d) => d.type_node,
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
        Kind::ArrayType => get_type_from_array_type_node(checker, program, node, globals),
        Kind::TupleType => get_type_from_tuple_type_node(checker, program, node, globals),
        Kind::TypeLiteral => get_type_from_type_literal_node(checker, program, node),
        Kind::TypeOperator => get_type_from_type_operator_node(checker, program, node, globals),
        Kind::UnionType => get_type_from_union_type_node(checker, program, node, globals),
        Kind::IntersectionType => {
            get_type_from_intersection_type_node(checker, program, node, globals)
        }
        // DEFER(phase-4-checker-4d): remaining type-node kinds.
        // blocked-by: their type constructors land across 4d+.
        _ => checker.error_type(),
    }
}

// Resolves an `A | B` type node to the constructed union type: each constituent
// node is resolved, then combined via `get_union_type` (Go's literal-reduction
// union of the mapped constituents).
//
// DEFER(phase-4-checker-later): the `UnionReductionLiteral` mode and type-alias
// attribution (Go's `getAliasForTypeNode`).
// blocked-by: union literal/subtype reduction and type-alias attribution wiring.
// Go: internal/checker/checker.go:Checker.getTypeFromUnionTypeNode
fn get_type_from_union_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let constituents = match program.arena().data(node) {
        NodeData::UnionType(d) => d.types.nodes.clone(),
        _ => return checker.error_type(),
    };
    let members: Vec<TypeId> = constituents
        .iter()
        .map(|&n| get_type_from_type_node(checker, program, n, globals))
        .collect();
    checker.get_union_type(&members)
}

// Resolves an `A & B` type node to the constructed intersection type: each
// constituent node is resolved, then combined via `get_intersection_type`.
//
// DEFER(phase-4-checker-later): the `X & {}` no-supertype-reduction special
// case and alias attribution (Go's `getAliasForTypeNode`).
// blocked-by: the empty-object literal type and type-alias attribution wiring.
// Go: internal/checker/checker.go:Checker.getTypeFromIntersectionTypeNode
fn get_type_from_intersection_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let constituents = match program.arena().data(node) {
        NodeData::IntersectionType(d) => d.types.nodes.clone(),
        _ => return checker.error_type(),
    };
    let members: Vec<TypeId> = constituents
        .iter()
        .map(|&n| get_type_from_type_node(checker, program, n, globals))
        .collect();
    checker.get_intersection_type(&members)
}

// Resolves an `ArrayTypeNode` (`T[]`) to the global `Array<T>` reference, or to
// `ReadonlyArray<T>` when the node sits under a `readonly` type operator
// (`readonly T[]`): the element node is resolved, then combined with the chosen
// global interface as its sole type argument (Go's `getArrayType` ->
// `createArrayType` -> `createTypeReference(globalArrayType, [elementType])`,
// and `getArrayOrTupleTargetType` selecting `globalReadonlyArrayType` when
// `isReadonlyTypeOperator(node.Parent)`). The target is resolved by name through
// the scope chain (synthetic top-level `interface Array<T>` /
// `interface ReadonlyArray<T>` stand in for the lib types until P6 loads
// lib.d.ts); when no such type is in scope the node yields the error type.
//
// Go: internal/checker/checker.go:Checker.getTypeFromArrayOrTupleTypeNode / getArrayOrTupleTargetType
fn get_type_from_array_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let element_node = match program.arena().data(node) {
        NodeData::ArrayType(d) => d.element_type,
        _ => return checker.error_type(),
    };
    let element_type = get_type_from_type_node(checker, program, element_node, globals);
    let global_name = if is_readonly_type_operator_parent(program, node) {
        "ReadonlyArray"
    } else {
        "Array"
    };
    let array_symbol = match resolve_name(
        program,
        node,
        global_name,
        SymbolFlags::TYPE,
        false,
        globals,
    ) {
        Some(symbol) => symbol,
        // DEFER(phase-4-checker-P6): no global `Array`/`ReadonlyArray` in
        // scope (lib.d.ts not loaded). blocked-by: library globals (P6).
        None => return checker.error_type(),
    };
    let array_target = get_declared_type_of_symbol(checker, program, array_symbol, globals);
    checker.create_type_reference(array_target, vec![element_type])
}

// Reports whether `node`'s parent is a `readonly` type operator (`readonly T[]`).
// Go: internal/checker/checker.go:Checker.isReadonlyTypeOperator
fn is_readonly_type_operator_parent(program: &dyn BoundProgram, node: tsgo_ast::NodeId) -> bool {
    let Some(parent) = program.arena().parent(node) else {
        return false;
    };
    matches!(
        program.arena().data(parent),
        NodeData::TypeOperator(d) if d.operator == Kind::ReadonlyKeyword
    )
}

// Resolves a `TypeOperator` node (`keyof`/`unique`/`readonly T`). The `readonly`
// operator over an array/tuple is transparent at the type level: Go returns the
// operand's type (`getTypeFromTypeNode(argType)`) and the array node itself
// selects `globalReadonlyArrayType` by inspecting its parent operator.
//
// DEFER(phase-4-checker-later): `keyof` (`getIndexType`) and `unique symbol`
// (`getESSymbolLikeTypeForNode`). blocked-by: index-type construction and
// unique-ES-symbol typing.
// Go: internal/checker/checker.go:Checker.getTypeFromTypeOperatorNode
fn get_type_from_type_operator_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let (operator, operand) = match program.arena().data(node) {
        NodeData::TypeOperator(d) => (d.operator, d.type_node),
        _ => return checker.error_type(),
    };
    match operator {
        Kind::ReadonlyKeyword => get_type_from_type_node(checker, program, operand, globals),
        // DEFER(phase-4-checker-later): `keyof` / `unique symbol`.
        // blocked-by: `getIndexType` + unique-ES-symbol typing.
        _ => checker.error_type(),
    }
}

// Resolves a `TupleType` node (`[A, B]`) to a fixed-arity tuple type whose
// element types are stored by position. Go builds a tuple type as a type
// reference to a generated/global tuple target carrying the element types as
// its type arguments (`getTypeFromArrayOrTupleTypeNode` -> the
// `target.objectFlags & ObjectFlagsTuple` branch -> `createNormalizedTupleType`);
// the FIXED-arity subset here stores the mapped element types directly on a
// `TUPLE`-flagged object type (in `resolved_type_arguments`, the positional
// element types), which is enough for element access by a literal index.
//
// DEFER(phase-4-checker-4ae+): variadic (`[...T[]]`), optional (`[a?, b]`),
// labeled (`[x: string]`) and rest tuple elements, the full generated tuple
// target with `TupleElementInfo`/`length`/`[number]` members, tuple-to-array
// assignability, and `as const`. blocked-by: `createNormalizedTupleType` +
// `getTupleTargetType` + tuple element flags + the iterator/spread machinery.
// Go: internal/checker/checker.go:Checker.getTypeFromArrayOrTupleTypeNode (tuple branch)
fn get_type_from_tuple_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let element_nodes = match program.arena().data(node) {
        NodeData::TupleType(d) => d.types.nodes.clone(),
        _ => return checker.error_type(),
    };
    // DEFER(phase-4-checker-4ae+): variadic/optional/labeled element kinds
    // (`RestType`/`OptionalType`/`NamedTupleMember`). The fixed-arity subset
    // resolves each element node to its plain type.
    // blocked-by: tuple element flags + `getTupleElementInfo`.
    let element_types: Vec<TypeId> = element_nodes
        .iter()
        .map(|&n| get_type_from_type_node(checker, program, n, globals))
        .collect();
    checker.create_tuple_type(element_types)
}

// Resolves a `TypeLiteral` type node (`{ value: T }`) to an anonymous object
// type carrying the members the binder collected onto its `__type` symbol
// (Go's `getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode` ->
// `getResolvedTypeOfTypeLiteral`). Member *types* are resolved lazily by
// `get_type_of_symbol` (so a `value: T` member stays the type parameter until
// instantiated through an enclosing reference).
//
// DEFER(phase-4-checker-4ah+): call/construct/index signatures of a type
// literal (`{ (): void }` / `{ [k: string]: V }`), and the per-reference deep
// instantiation of an anonymous type literal's members. blocked-by: anonymous
// object signature collection + anonymous-type instantiation.
// Go: internal/checker/checker.go:Checker.getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode
fn get_type_from_type_literal_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
) -> TypeId {
    let Some(symbol) = program.symbol_of_node(node) else {
        return checker.error_type();
    };
    let members = program.symbol(symbol).members.clone();
    let properties: Vec<SymbolId> = members.values().copied().collect();
    let object = ObjectType {
        members,
        properties,
        ..Default::default()
    };
    checker.new_object_type(ObjectFlags::ANONYMOUS, Some(symbol), object)
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
        // The binder does not place a generic declaration's type parameters in
        // its `locals`, so a bare `T` reference inside a generic interface/class/
        // method/function body is resolved by scanning the enclosing
        // type-parameter lists for a matching name (Go's `resolveName` finds
        // these in `locals`). This lets a `value: T` member of `Iterator<T>`
        // carry the type parameter (4ah), which is then instantiated through the
        // enclosing reference's type-argument mapper.
        None => match resolve_type_parameter_in_scope(program, node, &name) {
            Some(tp) => tp,
            None => return checker.error_type(),
        },
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

// Resolves a bare type-reference name to an enclosing generic declaration's
// type parameter by walking the parent chain and scanning each generic
// container's `<...>` list for a matching name. Mirrors the type-parameter
// reachability Go gets for free from binding type parameters into a
// declaration's `locals`.
// Go: internal/checker/checker.go:Checker.resolveName (type-parameter meaning)
fn resolve_type_parameter_in_scope(
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    name: &str,
) -> Option<SymbolId> {
    let arena = program.arena();
    let mut current = arena.parent(node);
    while let Some(n) = current {
        if let Some(list) = type_parameter_list_of(program, n) {
            for tp in list.nodes {
                if let NodeData::TypeParameterDeclaration(d) = arena.data(tp) {
                    if arena.kind(d.name) == Kind::Identifier && arena.text(d.name) == name {
                        return program.symbol_of_node(tp);
                    }
                }
            }
        }
        current = arena.parent(n);
    }
    None
}

// Returns the `<...>` type-parameter list of a generic declaration container, if
// it has one.
fn type_parameter_list_of(
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
) -> Option<tsgo_ast::NodeList> {
    match program.arena().data(node) {
        NodeData::InterfaceDeclaration(d)
        | NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d) => d.type_parameters.clone(),
        NodeData::FunctionDeclaration(d) => d.type_parameters.clone(),
        NodeData::MethodSignature(d) => d.type_parameters.clone(),
        NodeData::MethodDeclaration(d) => d.type_parameters.clone(),
        NodeData::TypeAliasDeclaration(d) => d.type_parameters.clone(),
        _ => None,
    }
}

/// Returns the apparent type of `t`.
///
/// A string-like type (`string` or a string literal) maps to the global
/// `String` wrapper interface when it has been built into the checker's
/// global-type cache (Go's `getApparentType` mapping primitives to
/// `globalStringType`/`globalNumberType`/...). This is what lets a property
/// access on a primitive resolve a wrapper member (`"abc".length`). When the
/// wrapper has not been built, or `t` is not string-like, `t` is returned
/// unchanged (object/union/intersection types are already their own apparent
/// type).
///
/// DEFER(phase-4-checker-P6): the remaining primitive wrappers
/// (`Number`/`Boolean`/`BigInt`/`Symbol`), index/instantiable apparent forms,
/// and base-constraint resolution. The wrapper is read from the global-type
/// cache, so a caller must have populated it via [`get_global_type`]; the
/// automatic build during `NewChecker` (and the real lib.d.ts `String`) is a
/// P6 concern.
/// blocked-by: library globals (lib.d.ts loading, P6) and base-constraint
/// resolution (4d).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_apparent_type, Checker};
/// let c = Checker::new();
/// // With no global `String` built, `string` is its own apparent type.
/// assert_eq!(get_apparent_type(&c, c.string_type()), c.string_type());
/// ```
///
/// Side effects: none (a read over the checker's global-type cache).
// Go: internal/checker/checker.go:Checker.getApparentType
// Builds the global primitive wrapper interface backing `t`'s apparent type on
// demand (Go's `getApparentType` -> `getGlobalStringType`), so a property
// access on a primitive resolves a wrapper member end to end through the
// checker. 4aa wires the `string` -> global `String` mapping (the only apparent
// wrapper `get_apparent_type` reads), built against the view of the file that
// DECLARES `String` (cross-file safe via `Checker::get_global_type`).
//
// DEFER(phase-4-checker-P6): the `Number`/`Boolean`/`BigInt`/`Symbol` wrappers
// and `globalThis`. blocked-by: library globals (lib.d.ts loading, P6).
// Go: internal/checker/checker.go:Checker.getApparentType (global*Type build)
fn ensure_primitive_apparent_wrapper(checker: &mut Checker, t: TypeId) {
    if checker
        .get_type(t)
        .flags()
        .intersects(TypeFlags::STRING_LIKE)
        && !checker.global_types.contains_key("String")
    {
        let _ = checker.get_global_type("String");
    }
}

pub fn get_apparent_type(checker: &Checker, t: TypeId) -> TypeId {
    let flags = checker.get_type(t).flags();
    if flags.intersects(TypeFlags::STRING_LIKE) {
        if let Some(&wrapper) = checker.global_types.get("String") {
            return wrapper;
        }
    }
    t
}

/// Returns the index signatures of `t` (after apparent-type mapping and generic
/// reference instantiation).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_index_infos_of_type, Checker};
/// let mut c = Checker::new();
/// let s = c.string_type();
/// assert!(get_index_infos_of_type(&mut c, s).is_empty());
/// ```
///
/// Side effects: may instantiate index signature value types.
// Go: internal/checker/checker.go:Checker.getIndexInfosOfType
pub fn get_index_infos_of_type(checker: &mut Checker, t: TypeId) -> Vec<IndexInfoId> {
    get_index_infos_of_structured_type(checker, get_apparent_type(checker, t))
}

/// Returns whether an index of type `source` can use an index signature keyed by
/// `target`.
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.isApplicableIndexType
fn is_applicable_index_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    source: TypeId,
    target: TypeId,
) -> bool {
    if checker.is_type_assignable_to(program, source, target) {
        return true;
    }
    if checker.get_type(target).flags().contains(TypeFlags::STRING)
        && checker.is_type_assignable_to(program, source, checker.number_type())
    {
        return true;
    }
    if checker.get_type(target).flags().contains(TypeFlags::NUMBER)
        && checker
            .get_type(source)
            .flags()
            .contains(TypeFlags::NUMBER_LITERAL)
    {
        return true;
    }
    false
}

/// Returns the index signature applicable to `key_type` on `t`.
///
/// Side effects: may instantiate index infos for generic references.
// Go: internal/checker/checker.go:Checker.getApplicableIndexInfo / findApplicableIndexInfo
fn get_applicable_index_info(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
    key_type: TypeId,
) -> Option<IndexInfoId> {
    let index_infos = get_index_infos_of_type(checker, t);
    let string_type = checker.string_type();
    let mut string_index: Option<IndexInfoId> = None;
    let mut applicable = Vec::new();
    for id in index_infos {
        let info = checker.index_info(id);
        if info.key_type == string_type {
            string_index = Some(id);
        } else if is_applicable_index_type(checker, program, key_type, info.key_type) {
            applicable.push(id);
        }
    }
    match applicable.len() {
        0 => {
            if let Some(id) = string_index {
                if is_applicable_index_type(checker, program, key_type, string_type) {
                    return Some(id);
                }
            }
            None
        }
        1 => Some(applicable[0]),
        // DEFER(phase-4-checker-4ad+): synthesize an intersection index info when
        // multiple signatures apply. blocked-by: intersection index typing.
        _ => Some(applicable[0]),
    }
}

/// Resolves `object_type[index_type]` to the element/indexed value type when an
/// applicable index signature exists.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_indexed_access_type, BoundProgram, Checker, TypeId};
/// use tsgo_ast::NodeId;
/// fn access<P: BoundProgram>(c: &mut Checker, p: &P, obj: TypeId, key: TypeId) {
///     let _ = get_indexed_access_type(c, p, obj, key);
/// }
/// ```
///
/// Side effects: may instantiate index signature value types.
// Go: internal/checker/checker.go:Checker.getIndexedAccessTypeOrUndefined / getPropertyTypeForIndexType
pub fn get_indexed_access_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    object_type: TypeId,
    index_type: TypeId,
) -> Option<TypeId> {
    ensure_primitive_apparent_wrapper(checker, object_type);
    let apparent = get_apparent_type(checker, object_type);
    let flags = checker.get_type(apparent).flags();
    if flags.intersects(TypeFlags::ANY | TypeFlags::NEVER) {
        return Some(apparent);
    }
    if let Some(element) = get_tuple_element_by_literal_index(checker, apparent, index_type) {
        return Some(element);
    }
    if let Some(union) = get_tuple_number_index_type(checker, apparent, index_type) {
        return Some(union);
    }
    if !checker
        .get_type(index_type)
        .flags()
        .intersects(TypeFlags::STRING_LIKE | TypeFlags::NUMBER_LIKE)
    {
        return None;
    }
    let info = get_applicable_index_info(checker, program, apparent, index_type)?;
    Some(checker.index_info(info).value_type)
}

// Resolves `tuple[index]` to the element type at a constant position when the
// object is a fixed-arity tuple and `index_type` is an in-range non-negative
// integer literal (`[string, number][0]` -> `string`). Go reads tuple elements
// positionally from the tuple reference's type arguments; the fixed-arity
// subset stores those element types in `resolved_type_arguments`.
//
// DEFER(phase-4-checker-4ae+): a non-literal `number` index (union of all
// element types), out-of-bounds / negative indices, and variadic/rest tuples.
// blocked-by: tuple element union typing + rest-element handling.
// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (tuple element)
fn get_tuple_element_by_literal_index(
    checker: &Checker,
    object_type: TypeId,
    index_type: TypeId,
) -> Option<TypeId> {
    let obj = checker.get_type(object_type).as_object()?;
    if !checker
        .get_type(object_type)
        .object_flags()
        .contains(ObjectFlags::TUPLE)
    {
        return None;
    }
    let elements = obj.resolved_type_arguments.clone();
    let LiteralValue::Number(n) = checker.get_type(index_type).literal_value()? else {
        return None;
    };
    let value = f64::from(*n);
    if value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    let position = value as usize;
    elements.get(position).copied()
}

// Resolves `tuple[index]` to the union of all element types when the object is a
// fixed-arity tuple and `index_type` is the (non-literal) `number` primitive
// (`[string, number][i]` with `i: number` -> `string | number`). Go derives this
// from the tuple's `[number]` index info, whose value type is the union of the
// element types.
//
// DEFER(phase-4-checker-4af+): `noUncheckedIndexedAccess` (which unions
// `undefined` in) and variadic/rest tuples. blocked-by: option wiring + the
// generated tuple target's rest element.
// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (tuple number index)
fn get_tuple_number_index_type(
    checker: &mut Checker,
    object_type: TypeId,
    index_type: TypeId,
) -> Option<TypeId> {
    let elements = {
        let obj = checker.get_type(object_type).as_object()?;
        if !checker
            .get_type(object_type)
            .object_flags()
            .contains(ObjectFlags::TUPLE)
        {
            return None;
        }
        obj.resolved_type_arguments.clone()
    };
    // A literal index is handled positionally by the caller; only the `number`
    // primitive distributes over all elements here.
    if !checker
        .get_type(index_type)
        .flags()
        .contains(TypeFlags::NUMBER)
    {
        return None;
    }
    Some(checker.get_union_type(&elements))
}

// Returns the numeric literal type for a fixed-arity tuple's `length` (the tuple
// arity), or `None` when `object_type` is not a `TUPLE`-flagged object. Go's
// generated tuple target gives `length` the union of literal lengths
// `minLength..=arity`; for a fixed-arity tuple that union is the single numeric
// literal equal to the arity.
//
// DEFER(phase-4-checker-4af+): variadic/optional tuples whose `length` is the
// `number` primitive or a union of several literal lengths. blocked-by: the
// generated tuple target with element flags + `minLength`/`fixedLength`.
// Go: internal/checker/checker.go:Checker.createTupleTargetType (length member)
fn get_tuple_length_type(checker: &mut Checker, object_type: TypeId) -> Option<TypeId> {
    let arity = {
        let obj = checker.get_type(object_type).as_object()?;
        if !checker
            .get_type(object_type)
            .object_flags()
            .contains(ObjectFlags::TUPLE)
        {
            return None;
        }
        obj.resolved_type_arguments.len()
    };
    Some(checker.new_literal_type(
        TypeFlags::NUMBER_LITERAL,
        LiteralValue::Number(tsgo_jsnum::Number::from(arity as f64)),
        None,
    ))
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
/// For an intersection type, the property is synthesized from its
/// constituents: a name present in any constituent resolves (Go's
/// `getPropertyOfUnionOrIntersectionType` intersection branch). When the name
/// lives in a single constituent, that constituent's own symbol is returned;
/// when it appears in two or more, a transient property symbol is minted whose
/// type is the intersection of the per-constituent types.
///
/// For a union type, the property resolves only if present on *every*
/// constituent (otherwise it is partial and filtered out); when the
/// constituents contribute distinct symbols, a transient property symbol is
/// minted whose type is the union of the per-constituent types.
///
/// Side effects: may mint a synthesized property symbol via the checker's
/// transient arena (through interior mutability) for multi-constituent names;
/// the combined property type itself is resolved lazily by `get_type_of_symbol`.
///
/// The synthesized symbol carries the propagated optional flag (union OR /
/// intersection AND of the constituents).
///
/// DEFER(phase-4-checker-later): index-signature lookup, `Object`/`Function`
/// global augmentation, the union partial-property machinery beyond the
/// "present on all" rule (index-info / object-literal `undefined` widening,
/// private/protected discriminant filtering), and accessor/readonly flag
/// propagation onto the synthesized symbol.
/// blocked-by: index signatures + library globals (P6) + accessor flag
/// plumbing and `isReadonlySymbol` (declaration-modifier / `CheckFlags`
/// readonly) infrastructure on synthesized symbols.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_property_of_type, Checker};
/// let c = Checker::new();
/// // A primitive intrinsic has no own members in 4d.
/// assert_eq!(get_property_of_type(&c, c.string_type(), "length"), None);
/// ```
// Go: internal/checker/checker.go:Checker.getPropertyOfType
pub fn get_property_of_type(checker: &Checker, t: TypeId, name: &str) -> Option<SymbolId> {
    let apparent = get_apparent_type(checker, t);
    if checker.get_type(apparent).intersection_types().is_some() {
        return get_intersection_property(checker, apparent, name);
    }
    if checker.get_type(apparent).union_types().is_some() {
        return get_union_property(checker, apparent, name);
    }
    let obj = checker.get_type(apparent).as_object()?;
    match obj.target {
        Some(target) => get_property_of_type(checker, target, name),
        None => obj.members.get(name).copied(),
    }
}

// Resolves `name` on an intersection type. A name found in exactly one
// constituent returns that constituent's own symbol (Go's `singleProp` return);
// a name found in two or more constituents mints a synthesized property whose
// type is the intersection of the per-constituent types.
// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty (intersection)
fn get_intersection_property(checker: &Checker, t: TypeId, name: &str) -> Option<SymbolId> {
    if let Some(cached) = checker.cached_synthesized_property(t, name) {
        return cached;
    }
    let members = checker
        .get_type(t)
        .intersection_types()
        .unwrap_or(&[])
        .to_vec();
    let mut distinct: Vec<SymbolId> = Vec::new();
    for member in members {
        if let Some(prop) = get_property_of_type(checker, member, name) {
            if !distinct.contains(&prop) {
                distinct.push(prop);
            }
        }
    }
    let result = match distinct.len() {
        0 => None,
        1 => Some(distinct[0]),
        // Go: `optionalFlag` starts as `SymbolFlagsOptional` and is AND-ed with
        // each constituent (`optionalFlag &= prop.Flags`) — an intersection
        // property is optional only if optional in *all* constituents.
        _ => {
            let optional = intersection_optional_flag(checker, &distinct);
            Some(checker.new_synthesized_property(
                name,
                SymbolFlags::PROPERTY | optional,
                CheckFlags::SYNTHETIC_PROPERTY,
                t,
            ))
        }
    };
    checker.cache_synthesized_property(t, name, result);
    result
}

// Computes the optional flag of a synthesized *intersection* property: optional
// only if it is optional in every constituent that declares it (Go starts
// `optionalFlag` at `SymbolFlagsOptional` and ANDs each `prop.Flags`).
//
// Reads each constituent property's meaning flags through the checker's
// retained program; when no program is retained the flag cannot be determined
// and is treated as absent.
// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty (!isUnion optionalFlag)
fn intersection_optional_flag(checker: &Checker, props: &[SymbolId]) -> SymbolFlags {
    let Some(program) = checker.program() else {
        return SymbolFlags::NONE;
    };
    let mut optional = SymbolFlags::OPTIONAL;
    for &prop in props {
        optional &= checker.resolved_symbol_flags(program, prop);
    }
    optional & SymbolFlags::OPTIONAL
}

// Resolves `name` on a union type. The property is present only if *every*
// constituent has it; a constituent missing it makes the property partial,
// which is filtered out and treated as absent (Go's `CheckFlagsReadPartial` ->
// `getPropertyOfUnionOrIntersectionType` returning nil). When all constituents
// contribute the same symbol it is returned directly; distinct symbols mint a
// synthesized property whose type is the union of the per-constituent types.
// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty (union)
fn get_union_property(checker: &Checker, t: TypeId, name: &str) -> Option<SymbolId> {
    if let Some(cached) = checker.cached_synthesized_property(t, name) {
        return cached;
    }
    let members = checker.get_type(t).union_types().unwrap_or(&[]).to_vec();
    let mut distinct: Vec<SymbolId> = Vec::new();
    let mut present_on_all = true;
    for member in members {
        match get_property_of_type(checker, member, name) {
            Some(prop) => {
                if !distinct.contains(&prop) {
                    distinct.push(prop);
                }
            }
            None => {
                present_on_all = false;
                break;
            }
        }
    }
    let result = if !present_on_all || distinct.is_empty() {
        None
    } else if distinct.len() == 1 {
        Some(distinct[0])
    } else {
        // Go: `optionalFlag |= prop.Flags & SymbolFlagsOptional` — a union
        // property is optional if it is optional in *any* constituent.
        let optional = union_optional_flag(checker, &distinct);
        Some(checker.new_synthesized_property(
            name,
            SymbolFlags::PROPERTY | optional,
            CheckFlags::SYNTHETIC_PROPERTY,
            t,
        ))
    };
    checker.cache_synthesized_property(t, name, result);
    result
}

// Computes the optional flag of a synthesized *union* property: optional if it
// is optional in any constituent that declares it (Go OR-s
// `prop.Flags & SymbolFlagsOptional` over the union's constituents).
//
// Reads each constituent property's meaning flags through the checker's
// retained program (the synthesized-aware `resolved_symbol_flags`); when no
// program is retained the flag cannot be determined and is treated as absent.
// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty (isUnion optionalFlag)
fn union_optional_flag(checker: &Checker, props: &[SymbolId]) -> SymbolFlags {
    let Some(program) = checker.program() else {
        return SymbolFlags::NONE;
    };
    let mut optional = SymbolFlags::NONE;
    for &prop in props {
        optional |= checker.resolved_symbol_flags(program, prop) & SymbolFlags::OPTIONAL;
    }
    optional
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
    ensure_primitive_apparent_wrapper(checker, t);
    // A fixed-arity tuple's `length` is the numeric literal type of its arity
    // (Go's generated tuple target gives `length` the union of literal lengths,
    // which collapses to a single literal for a fixed tuple). The 4af subset's
    // tuples carry no member table, so this is resolved directly.
    if name == "length" {
        if let Some(length) = get_tuple_length_type(checker, t) {
            return Some(length);
        }
    }
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
/// included). For an intersection it unions every constituent's properties
/// (Go's `getPropertiesOfUnionOrIntersectionType`), keeping the first
/// constituent that declares each name.
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
    if let Some(members) = checker.get_type(apparent).intersection_types() {
        let members = members.to_vec();
        let mut props: Vec<(String, SymbolId)> = Vec::new();
        for m in members {
            for (name, sym) in get_properties_of_type(checker, m) {
                if !props.iter().any(|(existing, _)| existing == &name) {
                    props.push((name, sym));
                }
            }
        }
        return props;
    }
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
