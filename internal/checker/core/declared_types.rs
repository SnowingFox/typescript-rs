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

use tsgo_ast::symbol::{
    INTERNAL_SYMBOL_NAME_CALL, INTERNAL_SYMBOL_NAME_INDEX, INTERNAL_SYMBOL_NAME_NEW,
    INTERNAL_SYMBOL_NAME_PREFIX,
};
use tsgo_ast::{
    CheckFlags, Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId, SymbolFlags, SymbolId,
    SymbolTable,
};

use super::conditional_types::is_distributive_conditional_type;
use super::grammar;
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::signatures::{IndexInfo, IndexInfoId, Signature, SignatureFlags, SignatureId};
use super::symbols::resolve_name;
use super::symbols_query::get_symbol_of_declaration;
use super::types::{
    AccessFlags, ConditionalRoot, IndexFlags, LiteralValue, MappedTypeModifiers, ObjectFlags,
    ObjectType, StringMappingKind, TypeData, TypeFlags, TypeId,
};
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
    // Resolve the declared type against the view of the file that DECLARES the
    // symbol. A multi-file program may declare a type alias / enum / class /
    // interface in a lib file whose declaration nodes are NOT in
    // `program.arena()`; reading them through the current arena would index out
    // of bounds. Mirrors `Checker::get_global_type`'s owning-view switch and is
    // guarded by `file_handle()` so the switch happens at most once (the owning
    // view returns itself for `symbol`, so no infinite recursion). For a
    // single-file program `view_for_symbol` is `None` and this is a no-op.
    if let Some(view) = program.view_for_symbol(symbol) {
        if view.file_handle() != program.file_handle() {
            let owner_globals = view.globals();
            return get_declared_type_of_symbol(checker, view.as_ref(), symbol, owner_globals);
        }
    }
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
pub(crate) fn get_declared_type_of_type_parameter(
    checker: &mut Checker,
    symbol: SymbolId,
) -> TypeId {
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

/// Returns the resolved `extends` constraint of a type parameter, or `None`
/// when it is unconstrained (Go's `getConstraintOfTypeParameter` ->
/// `getConstraintFromTypeParameter`).
///
/// The constraint type node (`T extends U`) is resolved through the declaring
/// type-parameter symbol and cached per type id (Go caches on
/// `TypeParameter.constraint`).
///
/// DEFER(phase-4-checker-C-C): the inferred constraint (`infer T`), the mapped
/// type key constraint, and instantiated-target constraint chaining
/// (`tp.target`/`tp.mapper`). blocked-by: conditional/mapped types + nested
/// generic instantiation.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_constraint_of_type_parameter, BoundProgram, Checker, TypeId};
/// fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tp: TypeId) {
///     let _ = get_constraint_of_type_parameter(c, p, tp);
/// }
/// ```
///
/// Side effects: may build the constraint type and populate the constraint cache.
// Go: internal/checker/checker.go:Checker.getConstraintOfTypeParameter / getConstraintFromTypeParameter
pub fn get_constraint_of_type_parameter(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    type_parameter: TypeId,
) -> Option<TypeId> {
    if let Some(&cached) = checker.type_parameter_constraints.get(&type_parameter) {
        return cached;
    }
    let symbol = checker
        .get_type(type_parameter)
        .as_type_parameter()
        .and_then(|d| d.symbol);
    // Read the type-parameter declaration (and resolve its constraint node)
    // through the view of the file that DECLARES the symbol: the parameter may
    // belong to a lib generic (e.g. `Array<T>`) whose nodes are not in the
    // file-under-check's arena. For a single-file program this is `program`.
    let owner = symbol.and_then(|sym| program.view_for_symbol(sym));
    let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
    let constraint_node = symbol.and_then(|sym| {
        prog.symbol(sym)
            .declarations
            .iter()
            .find_map(|&decl| match prog.arena().data(decl) {
                NodeData::TypeParameterDeclaration(d) => d.constraint,
                _ => None,
            })
    });
    let result = constraint_node.map(|node| get_type_from_type_node(checker, prog, node, None));
    checker
        .type_parameter_constraints
        .insert(type_parameter, result);
    result
}

/// Returns the resolved `= Default` of a type parameter, or `None` when it has
/// no default (Go's `getDefaultFromTypeParameter`).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_default_from_type_parameter, BoundProgram, Checker, TypeId};
/// fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tp: TypeId) {
///     let _ = get_default_from_type_parameter(c, p, tp);
/// }
/// ```
///
/// Side effects: may build the default type and populate the default cache.
// Go: internal/checker/checker.go:Checker.getDefaultFromTypeParameter / getResolvedTypeParameterDefault
pub fn get_default_from_type_parameter(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    type_parameter: TypeId,
) -> Option<TypeId> {
    if let Some(&cached) = checker.type_parameter_defaults.get(&type_parameter) {
        return cached;
    }
    let symbol = checker
        .get_type(type_parameter)
        .as_type_parameter()
        .and_then(|d| d.symbol);
    // Read through the declaring file's view (see `get_constraint_of_type_parameter`).
    let owner = symbol.and_then(|sym| program.view_for_symbol(sym));
    let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
    let default_node = symbol.and_then(|sym| {
        prog.symbol(sym)
            .declarations
            .iter()
            .find_map(|&decl| match prog.arena().data(decl) {
                NodeData::TypeParameterDeclaration(d) => d.default_type,
                _ => None,
            })
    });
    let result = default_node.map(|node| get_type_from_type_node(checker, prog, node, None));
    checker
        .type_parameter_defaults
        .insert(type_parameter, result);
    result
}

// Reports whether a type parameter declares a `= Default` (Go's
// `hasTypeParameterDefault`): true when any declaring `TypeParameterDeclaration`
// has a default type node.
// Go: internal/checker/checker.go:Checker.hasTypeParameterDefault
fn has_type_parameter_default(
    checker: &Checker,
    program: &dyn BoundProgram,
    type_parameter: TypeId,
) -> bool {
    let Some(symbol) = checker
        .get_type(type_parameter)
        .as_type_parameter()
        .and_then(|d| d.symbol)
    else {
        return false;
    };
    // Read through the declaring file's view (see `get_constraint_of_type_parameter`).
    let owner = program.view_for_symbol(symbol);
    let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
    prog.symbol(symbol)
        .declarations
        .iter()
        .any(|&decl| match prog.arena().data(decl) {
            NodeData::TypeParameterDeclaration(d) => d.default_type.is_some(),
            _ => false,
        })
}

/// Returns the minimum number of type arguments a generic declaration requires:
/// the count up to (and including) the last type parameter without a default
/// (Go's `getMinTypeArgumentCount`).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_min_type_argument_count, BoundProgram, Checker};
/// fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tps: &[tsgo_checker::TypeId]) {
///     let _ = get_min_type_argument_count(c, p, tps);
/// }
/// ```
///
/// Side effects: none (reads declarations).
// Go: internal/checker/checker.go:Checker.getMinTypeArgumentCount
pub fn get_min_type_argument_count(
    checker: &Checker,
    program: &dyn BoundProgram,
    type_parameters: &[TypeId],
) -> usize {
    let mut min = 0;
    for (i, &tp) in type_parameters.iter().enumerate() {
        if !has_type_parameter_default(checker, program, tp) {
            min = i + 1;
        }
    }
    min
}

/// Pads a type-argument list with defaults (and the `unknown` base default) so
/// it matches the type-parameter arity (Go's `fillMissingTypeArguments`).
///
/// When fewer arguments than parameters are supplied, each missing slot is
/// filled with the corresponding type parameter's default — instantiated
/// through the partially-filled mapper, so a later default can reference an
/// earlier parameter (`<T, U = T>`) — or `unknown` when there is no default.
///
/// # Examples
/// ```
/// use tsgo_checker::{fill_missing_type_arguments, BoundProgram, Checker};
/// fn demo<P: BoundProgram>(c: &mut Checker, p: &P, args: &[tsgo_checker::TypeId], tps: &[tsgo_checker::TypeId]) {
///     let _ = fill_missing_type_arguments(c, p, args, tps);
/// }
/// ```
///
/// Side effects: may build default types and instantiate them.
// Go: internal/checker/checker.go:Checker.fillMissingTypeArguments
pub fn fill_missing_type_arguments(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    type_arguments: &[TypeId],
    type_parameters: &[TypeId],
) -> Vec<TypeId> {
    let num_parameters = type_parameters.len();
    if num_parameters == 0 {
        return Vec::new();
    }
    let num_arguments = type_arguments.len();
    if num_arguments >= num_parameters {
        return type_arguments.to_vec();
    }
    let mut result = type_arguments.to_vec();
    // Go maps invalid forward references in default types to the error type.
    result.resize(num_parameters, checker.error_type());
    let base_default = checker.unknown_type();
    for i in num_arguments..num_parameters {
        let default = get_default_from_type_parameter(checker, program, type_parameters[i]);
        result[i] = match default {
            Some(default_type) => {
                let mapper = super::mapper::TypeMapper::new(type_parameters, &result);
                checker.instantiate_type(default_type, &mapper)
            }
            None => base_default,
        };
    }
    result
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
    let mut own_members = sym.members.clone();
    let declarations = sym.declarations.clone();

    // The binder places a generic declaration's type parameters into the
    // symbol's member table (the same way Go's binder does). Type parameters are
    // NOT value members, so Go excludes them from a type's property list (and
    // from `getPropertyOfType`) via `getNamedMembers`/`getPropertyOfObjectType`'s
    // `symbolIsValue` check. The port's property iterators (`get_properties_of_type`)
    // take only a `&Checker` (no program to resolve symbol flags), so the
    // equivalent filtering happens here, when the program is in hand: drop the
    // type-parameter symbols from the constructed member table, which yields the
    // same observable membership (a `Box<T>` reference exposes `v`, not `T`).
    // Go: internal/checker/checker.go:Checker.symbolIsValue / getNamedMembers
    own_members.retain(|_, &mut member| {
        !program
            .symbol(member)
            .flags
            .contains(SymbolFlags::TYPE_PARAMETER)
    });

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

    // Resolve and merge base (extends) members. Classes use constructor-based
    // resolution (Go's `resolveBaseTypesOfClass`); interfaces keep the heritage
    // identifier lookup path.
    let base_types = if kind.contains(ObjectFlags::CLASS) {
        resolve_base_types_of_class(checker, program, t, &declarations, globals)
    } else {
        resolve_base_types(checker, program, t, &declarations, globals)
    };
    if !base_types.is_empty() {
        let mut merged = SymbolTable::default();
        for &base in &base_types {
            for (name, &member) in
                resolve_structured_type_members(checker, Some(program), base).iter()
            {
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
    // The `__index` member may have been merged in from ANOTHER file (cross-file
    // declaration merging of a global interface — see
    // `MultiFileBoundProgram`'s globals merge); its `IndexSignature` declaration
    // node lives in that file's arena, so read it through the view of the file
    // that DECLARES the index symbol, mirroring the owning-view switch in
    // `get_declared_type_of_symbol`. For a single-file program `view_for_symbol`
    // is `None` and this is a no-op (the index symbol is local).
    let owner = program.view_for_symbol(index_symbol);
    let prog: &dyn BoundProgram = match owner.as_deref() {
        Some(view) if view.file_handle() != program.file_handle() => view,
        _ => program,
    };
    let mut index_infos = Vec::new();
    for &decl in &prog.symbol(index_symbol).declarations {
        if prog.arena().kind(decl) != Kind::IndexSignature {
            continue;
        }
        let (param_nodes, type_node, modifiers) = match prog.arena().data(decl) {
            NodeData::IndexSignatureDeclaration(d) => {
                (&d.parameters.nodes, d.type_node, d.modifiers.as_ref())
            }
            _ => continue,
        };
        if param_nodes.len() != 1 {
            continue;
        }
        let key_type_node = match prog.arena().data(param_nodes[0]) {
            NodeData::ParameterDeclaration(d) => d.type_node,
            _ => None,
        };
        let Some(key_node) = key_type_node else {
            continue;
        };
        let key_type =
            get_type_from_type_node_with_type_params(checker, prog, key_node, type_params, globals);
        let value_type = match type_node {
            Some(node) => {
                get_type_from_type_node_with_type_params(checker, prog, node, type_params, globals)
            }
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
    get_index_infos_of_structured_type_impl(checker, t, &mut FxHashMap::default())
}

fn get_index_infos_of_structured_type_impl(
    checker: &mut Checker,
    t: TypeId,
    visited: &mut FxHashMap<TypeId, ()>,
) -> Vec<IndexInfoId> {
    if visited.contains_key(&t) {
        return Vec::new();
    }
    visited.insert(t, ());
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
        let target_infos = get_index_infos_of_structured_type_impl(checker, target, visited);
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
        for id in get_index_infos_of_structured_type_impl(checker, base, visited) {
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
            NodeData::TypeAliasDeclaration(d) => d.type_parameters.clone(),
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
// Resolves a class's `extends` clause to the base instance type via the base
// constructor expression (Go's `resolveBaseTypesOfClass`).
// Go: internal/checker/checker.go:Checker.resolveBaseTypesOfClass(19075)
fn resolve_base_types_of_class(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    class_type: TypeId,
    declarations: &[tsgo_ast::NodeId],
    globals: Option<&SymbolTable>,
) -> Vec<TypeId> {
    let base_type_node = declarations
        .iter()
        .find_map(|&decl| extends_heritage_clause_element(program, decl));
    let Some(base_type_node) = base_type_node else {
        return Vec::new();
    };
    let base_constructor_type =
        checker.get_base_constructor_type_of_class(program, class_type);
    if base_constructor_type == checker.undefined_type()
        || base_constructor_type == checker.error_type()
    {
        return Vec::new();
    }
    let static_base = get_apparent_type(checker, base_constructor_type);
    let flags = checker.get_type(static_base).flags();
    if !flags.intersects(TypeFlags::OBJECT | TypeFlags::INTERSECTION | TypeFlags::ANY) {
        return Vec::new();
    }
    let base_type = if let Some(sym) = checker.get_type(static_base).symbol {
        if program.symbol(sym).flags.contains(SymbolFlags::CLASS) {
            resolve_class_extends_instance_type(
                checker,
                program,
                base_type_node,
                sym,
                globals,
            )
        } else {
            let constructors = checker.get_instantiated_constructors_for_type_arguments(
                program,
                static_base,
                base_type_node,
            );
            if constructors.is_empty() {
                return Vec::new();
            }
            checker.get_return_type_of_signature(constructors[0])
        }
    } else {
        let constructors = checker.get_instantiated_constructors_for_type_arguments(
            program,
            static_base,
            base_type_node,
        );
        if constructors.is_empty() {
            return Vec::new();
        }
        checker.get_return_type_of_signature(constructors[0])
    };
    if base_type == checker.error_type() || !is_valid_base_type_for_extends(checker, base_type) {
        return Vec::new();
    }
    if class_type == base_type {
        return Vec::new();
    }
    vec![base_type]
}

// Resolves the instance type of a class `extends` heritage element (Go's
// `getTypeFromClassOrInterfaceReference` on the heritage node).
fn resolve_class_extends_instance_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    base_type_node: NodeId,
    base_class_sym: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let target = get_declared_type_of_symbol(checker, program, base_class_sym, globals);
    let type_parameters = checker
        .get_type(target)
        .as_object()
        .map(|o| o.type_parameters.clone())
        .unwrap_or_default();
    let provided_args: Vec<TypeId> = type_arguments_from_heritage_node(program, base_type_node)
        .iter()
        .map(|&n| get_type_from_type_node(checker, program, n, globals))
        .collect();
    if type_parameters.is_empty() {
        return target;
    }
    let filled = fill_missing_type_arguments(checker, program, &provided_args, &type_parameters);
    checker.create_type_reference(target, filled)
}

fn type_arguments_from_heritage_node(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::ExpressionWithTypeArguments(e) => e
            .type_arguments
            .as_ref()
            .map(|list| list.nodes.clone())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn is_valid_base_type_for_extends(checker: &Checker, type_id: TypeId) -> bool {
    let ty = checker.get_type(type_id);
    let flags = ty.flags();
    if flags.intersects(TypeFlags::OBJECT | TypeFlags::ANY | TypeFlags::NON_PRIMITIVE) {
        return true;
    }
    if flags.intersects(TypeFlags::INTERSECTION) {
        return ty
            .intersection_types()
            .map(|types| {
                types
                    .iter()
                    .all(|&t| is_valid_base_type_for_extends(checker, t))
            })
            .unwrap_or(false);
    }
    false
}

fn extends_heritage_clause_element(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let heritage = match program.arena().data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.heritage_clauses.clone(),
        _ => None,
    }?;
    for clause in heritage.nodes {
        if let NodeData::HeritageClause(h) = program.arena().data(clause) {
            if h.token == Kind::ExtendsKeyword {
                return h.types.nodes.first().copied();
            }
        }
    }
    None
}

fn resolve_base_types(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    self_type: TypeId,
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
                let Some(base_symbol) = resolve_name(
                    program,
                    expression,
                    &name,
                    SymbolFlags::TYPE,
                    false,
                    globals,
                ) else {
                    continue;
                };
                let base_type = get_declared_type_of_symbol(checker, program, base_symbol, globals);
                if base_type == checker.error_type() {
                    continue;
                }
                if self_type != base_type && !has_base_type(checker, base_type, self_type) {
                    bases.push(base_type);
                } else {
                    report_circular_base_type(checker, program, type_node, self_type);
                }
            }
        }
    }
    bases
}

// Returns the target of a class/interface type reference.
// Go: internal/checker/checker.go:getTargetType
fn get_target_type(checker: &Checker, t: TypeId) -> TypeId {
    checker
        .get_type(t)
        .as_object()
        .and_then(|obj| obj.target)
        .unwrap_or(t)
}

// Returns whether `t` has `check_base` in its extends chain.
// Go: internal/checker/checker.go:Checker.hasBaseType
pub(crate) fn has_base_type(checker: &Checker, t: TypeId, check_base: TypeId) -> bool {
    fn check(checker: &Checker, t: TypeId, check_base: TypeId) -> bool {
        let ty = checker.get_type(t);
        if let Some(members) = ty.intersection_types() {
            return members
                .iter()
                .any(|&member| check(checker, member, check_base));
        }
        if ty
            .object_flags()
            .intersects(ObjectFlags::CLASS_OR_INTERFACE | ObjectFlags::REFERENCE)
        {
            let target = get_target_type(checker, t);
            if target == check_base {
                return true;
            }
            if let Some(target_obj) = checker.get_type(target).as_object() {
                return target_obj
                    .base_types
                    .iter()
                    .any(|&base| check(checker, base, check_base));
            }
        }
        false
    }
    check(checker, t, check_base)
}

// Reports TS2310 when an interface/class `extends` clause would make the type
// its own base.
// Go: internal/checker/checker.go:Checker.reportCircularBaseType
fn report_circular_base_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
    t: TypeId,
) {
    let type_str = super::nodebuilder::type_to_string(checker, program, t);
    checker.error(
        program,
        node,
        &tsgo_diagnostics::TYPE_0_RECURSIVELY_REFERENCES_ITSELF_AS_A_BASE_TYPE,
        &[type_str.as_str()],
    );
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
pub fn resolve_structured_type_members(
    checker: &mut Checker,
    program: Option<&dyn BoundProgram>,
    t: TypeId,
) -> SymbolTable {
    if let Some(cached) = checker.structured_members_cache.get(&t) {
        return cached.clone();
    }
    if let Some(obj) = checker.get_type(t).as_object() {
        if let Some(target) = obj.target {
            let result = resolve_structured_type_members(checker, program, target);
            checker.structured_members_cache.insert(t, result.clone());
            return result;
        }
        let object_flags = checker.get_type(t).object_flags();
        if object_flags.contains(ObjectFlags::MAPPED)
            && !object_flags.contains(ObjectFlags::MEMBERS_RESOLVED)
        {
            if let (Some(program), Some(declaration)) =
                (program, checker.mapped_type_declaration(t))
            {
                super::mapped_types::resolve_mapped_type_members_lazy(
                    checker,
                    program,
                    t,
                    declaration,
                );
            }
        }
        let members = checker.get_type(t).as_object().map(|o| o.members.clone());
        if let Some(members) = members {
            checker.structured_members_cache.insert(t, members.clone());
            return members;
        }
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
    // Break circular references (`type T = T[] | ...`): if we are already
    // resolving this alias's declared type, a re-entrant resolve returns
    // `errorType` rather than recursing forever (Go's `pushTypeResolution`
    // returning false for the `DeclaredType` property -> circularity ->
    // `errorType`; the 2456 diagnostic itself is DEFER'd).
    if !checker.type_aliases_resolving.insert(symbol) {
        return checker.error_type();
    }
    let type_node = program
        .symbol(symbol)
        .declarations
        .iter()
        .find_map(|&decl| match program.arena().data(decl) {
            NodeData::TypeAliasDeclaration(d) => Some(d.type_node),
            _ => None,
        });
    // Record the alias's local type parameters (Go's
    // `typeAliasLinks.typeParameters`), so a generic-alias reference can check
    // its type-argument arity and constraints. Done before resolving the RHS so
    // the parameter types are interned by symbol.
    let declarations = program.symbol(symbol).declarations.clone();
    let type_parameters = collect_local_type_parameters(checker, program, &declarations);
    checker.type_alias_links.get(symbol).type_parameters = type_parameters;
    let t = match type_node {
        Some(node) => get_type_from_type_node(checker, program, node, globals),
        None => checker.error_type(),
    };
    checker.type_aliases_resolving.remove(&symbol);
    checker.type_alias_links.get(symbol).declared_type = Some(t);
    t
}

// Builds the declared (type-position) type of an enum: the union of its member
// literal types (`E.A | E.B`), tagged `ENUM_LITERAL` with the enum symbol so it
// prints as `E`. Each member's literal type is computed from its constant value
// (auto-increment / explicit / constant-foldable initializer) and cached as that
// member's declared type. A single-member enum collapses to that member's
// literal (Go's `getUnionTypeEx` returns the lone member), which prints as `E`
// via the node-builder's `getDeclaredTypeOfSymbol(parent) == t` rule.
//
// DEFER(phase-4-checker-C-D2): the fresh enum-literal pairing
// (`getFreshTypeOfLiteralType`), the `UnionReductionLiteral`/alias attribution,
// enum merging across declarations beyond a simple concat, and computed
// (non-constant) members. blocked-by: fresh enum types + alias attribution +
// computed-member evaluation.
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
    let declarations = program.symbol(symbol).declarations.clone();
    let mut member_type_list: Vec<TypeId> = Vec::new();
    for declaration in declarations {
        if program.arena().kind(declaration) != Kind::EnumDeclaration {
            continue;
        }
        for (member_node, value) in compute_enum_member_values(program, declaration) {
            // A member without a bound symbol has no bindable name (Go's
            // `hasBindableName` guard); skip it.
            let Some(member_symbol) = program.symbol_of_node(member_node) else {
                continue;
            };
            let member_type = match value {
                tsgo_evaluator::EvalValue::Num(n) => checker.new_enum_literal_type(
                    TypeFlags::NUMBER_LITERAL | TypeFlags::ENUM_LITERAL,
                    LiteralValue::Number(n),
                    member_symbol,
                ),
                tsgo_evaluator::EvalValue::Str(s) => checker.new_enum_literal_type(
                    TypeFlags::STRING_LITERAL | TypeFlags::ENUM_LITERAL,
                    LiteralValue::String(s),
                    member_symbol,
                ),
                // A non-constant member yields a computed enum type (DEFER).
                _ => checker.new_computed_enum_type(member_symbol),
            };
            checker.declared_type_links.get(member_symbol).declared_type = Some(member_type);
            member_type_list.push(member_type);
        }
    }
    let enum_type = if member_type_list.is_empty() {
        checker.new_computed_enum_type(symbol)
    } else {
        let union = checker.get_union_type(&member_type_list);
        // A genuine (multi-member) union is tagged as the enum type so it prints
        // as `E` and relates via `isEnumTypeRelatedTo`; a single member stays
        // the lone literal (Go skips the tag for a non-union).
        if checker.get_type(union).flags().contains(TypeFlags::UNION) {
            checker.mark_enum_union(union, symbol);
        }
        union
    };
    checker.declared_type_links.get(symbol).declared_type = Some(enum_type);
    enum_type
}

// Returns each enum member node paired with its evaluated constant value, in
// declaration order, applying numeric auto-increment and constant-foldable
// initializers (Go's `computeEnumMemberValues` / `computeEnumMemberValue`).
//
// Entity references in an initializer (`A | 1`, where `A` is an earlier member)
// resolve against the enum's own already-computed members by name — the
// reachable subset of Go's `evaluateEntity`/`evaluateEnumMember`. Cross-enum /
// variable references and the per-member diagnostics (computed-name, numeric
// name, "member must have initializer", const-enum/ambient/isolatedModules
// errors) are DEFER.
//
// DEFER(phase-4-checker-C-D2): `evaluateEnumMember` use-before-assigned (2474),
// the computed-member checks, and cross-file resolution.
// blocked-by: full `resolveEntityName` + enum diagnostics.
// Resolves a bare identifier to a file-local `const` variable's foldable value,
// when the variable has a constant-foldable initializer (Go `evaluateEntity` /
// `isConstantVariable` subset).
fn try_evaluate_const_variable_reference(
    program: &dyn BoundProgram,
    location: NodeId,
    name: &str,
) -> Option<tsgo_evaluator::Result> {
    use tsgo_evaluator::{new_evaluator, new_result, EvalValue, OuterExpressionKinds};
    let globals = program.globals();
    let sym = resolve_name(
        program,
        location,
        name,
        SymbolFlags::VALUE,
        true,
        globals,
    )?;
    let decl = program.symbol(sym).value_declaration?;
    let init = match program.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer?,
        _ => return None,
    };
    let flags = program.symbol(sym).flags;
    if !flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
        return None;
    }
    let eval = new_evaluator(
        |_, _, _| new_result(EvalValue::None, false, false, false),
        OuterExpressionKinds::NONE,
    );
    let result = eval.evaluate(program.arena(), init, Some(location));
    if matches!(result.value, EvalValue::None) {
        None
    } else {
        Some(result)
    }
}

// Entity resolver for enum-member initializer evaluation (Go `evaluateEntity` subset).
fn evaluate_enum_entity(
    program: &dyn BoundProgram,
    arena: &NodeArena,
    expr: NodeId,
    loc: Option<NodeId>,
    by_name: &FxHashMap<String, tsgo_evaluator::EvalValue>,
) -> tsgo_evaluator::Result {
    use tsgo_evaluator::{new_result, EvalValue};
    let name = match arena.data(expr) {
        NodeData::Identifier(_) => Some(arena.text(expr).to_string()),
        NodeData::PropertyAccessExpression(d) => Some(arena.text(d.name).to_string()),
        _ => None,
    };
    if let Some(n) = name {
        if let Some(v) = by_name.get(&n) {
            return new_result(v.clone(), false, false, false);
        }
        if matches!(arena.data(expr), NodeData::Identifier(_)) {
            if n == "Infinity" || n == "NaN" {
                return new_result(
                    EvalValue::Num(tsgo_jsnum::from_string(&n)),
                    false,
                    false,
                    false,
                );
            }
            if let Some(loc) = loc {
                if let Some(result) = try_evaluate_const_variable_reference(program, loc, &n) {
                    return result;
                }
            }
        }
    }
    new_result(EvalValue::None, false, false, false)
}

// Evaluates one enum member initializer with prior-member name resolution.
// Go: internal/checker/checker.go:Checker.evaluate (enum member context)
pub(crate) fn evaluate_enum_member_initializer(
    program: &dyn BoundProgram,
    member: NodeId,
    by_name: &FxHashMap<String, tsgo_evaluator::EvalValue>,
) -> tsgo_evaluator::Result {
    use tsgo_evaluator::{new_evaluator, OuterExpressionKinds};
    let init = match program.arena().data(member) {
        NodeData::EnumMember(d) => d.initializer,
        _ => return tsgo_evaluator::new_result(tsgo_evaluator::EvalValue::None, false, false, false),
    };
    let Some(init) = init else {
        return tsgo_evaluator::new_result(tsgo_evaluator::EvalValue::None, false, false, false);
    };
    let evaluator = new_evaluator(
        |arena: &NodeArena, expr: NodeId, loc: Option<NodeId>| {
            evaluate_enum_entity(program, arena, expr, loc, by_name)
        },
        OuterExpressionKinds::NONE,
    );
    evaluator.evaluate(program.arena(), init, Some(member))
}

// Go: internal/checker/checker.go:Checker.computeEnumMemberValues
pub(crate) fn compute_enum_member_values(
    program: &dyn BoundProgram,
    enum_declaration: NodeId,
) -> Vec<(NodeId, tsgo_evaluator::EvalValue)> {
    use tsgo_evaluator::EvalValue;
    let members = match program.arena().data(enum_declaration) {
        NodeData::EnumDeclaration(d) => d.members.nodes.clone(),
        _ => return Vec::new(),
    };
    let mut auto_value: Option<f64> = Some(0.0);
    let mut by_name: FxHashMap<String, EvalValue> = FxHashMap::default();
    let mut result: Vec<(NodeId, EvalValue)> = Vec::with_capacity(members.len());
    for member in members {
        let initializer = match program.arena().data(member) {
            NodeData::EnumMember(d) => d.initializer,
            _ => None,
        };
        let value = if initializer.is_some() {
            evaluate_enum_member_initializer(program, member, &by_name).value
        } else if let Some(v) = auto_value {
            // No initializer: the auto-incremented numeric value.
            EvalValue::Num(tsgo_jsnum::Number::from(v))
        } else {
            // A member following a non-numeric member with no initializer is an
            // error in Go (1061); reachable tests do not hit this. Treat as
            // non-foldable here (yields a computed enum type).
            EvalValue::None
        };
        auto_value = match &value {
            EvalValue::Num(n) => Some(f64::from(*n) + 1.0),
            _ => None,
        };
        if let Some(name) = enum_member_name_text(program, member) {
            by_name.insert(name, value.clone());
        }
        result.push((member, value));
    }
    result
}

// Returns the evaluated constant value of an enum member node (Go's
// `getEnumMemberValue`): computes the parent enum's member values and reads back
// this member's value. A node whose parent is not an enum declaration, or that
// is absent from the computed members, yields the non-foldable `None` (Go's
// `enumMemberLinks.Get(node).value` defaulting to a nil `Result.Value`).
//
// Reuses [`compute_enum_member_values`], so the same reachable subset applies
// (auto-increment + constant-foldable initializers + intra-enum entity
// references); the eager `enumMemberLinks` caching and the per-member
// diagnostics are still DEFER (see `compute_enum_member_values`).
// Go: internal/checker/checker.go:Checker.getEnumMemberValue
pub(crate) fn get_enum_member_value(
    program: &dyn BoundProgram,
    member: NodeId,
) -> tsgo_evaluator::EvalValue {
    let Some(parent) = program.arena().parent(member) else {
        return tsgo_evaluator::EvalValue::None;
    };
    if program.arena().kind(parent) != Kind::EnumDeclaration {
        return tsgo_evaluator::EvalValue::None;
    }
    for (member_node, value) in compute_enum_member_values(program, parent) {
        if member_node == member {
            return value;
        }
    }
    tsgo_evaluator::EvalValue::None
}

// Returns the textual name of an enum member (its identifier/string-literal
// name), used to key prior members for entity-reference resolution.
fn enum_member_name_text(program: &dyn BoundProgram, member: NodeId) -> Option<String> {
    let name = match program.arena().data(member) {
        NodeData::EnumMember(d) => d.name,
        _ => return None,
    };
    if matches!(
        program.arena().data(name),
        NodeData::ComputedPropertyName(_)
    ) {
        return None;
    }
    Some(program.arena().text(name).to_string())
}

// Builds (or returns the cached) value-position type of an enum: an anonymous
// object whose members are the enum's exports, so `E.A` (a property access)
// reads member `A`'s literal type (Go's `getTypeOfFuncClassEnumModule` for an
// enum symbol). This is distinct from the declared (type-position) union built
// by `get_declared_type_of_enum`.
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModule (enum)
fn get_type_of_enum_object(
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
    let members = program.symbol(symbol).exports.clone();
    let properties: Vec<SymbolId> = members.values().copied().collect();
    let object = ObjectType {
        members,
        properties,
        ..Default::default()
    };
    let t = checker.new_object_type(ObjectFlags::ANONYMOUS, Some(symbol), object);
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

// Returns the type of an enum member symbol (Go's `getTypeOfEnumMember` ->
// `getDeclaredTypeOfEnumMember`): the member's literal type, resolved by
// building the parent enum's declared type (which populates each member's
// declared type) and reading it back.
// Go: internal/checker/checker.go:Checker.getTypeOfEnumMember / getDeclaredTypeOfEnumMember
fn get_type_of_enum_member(
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
    let t = get_declared_type_of_enum_member(checker, program, symbol);
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

// Returns the declared type of an enum member: building the parent enum's
// declared type populates each member's `declared_type` link with its literal
// type; this reads it back (falling back to the enum type if unset).
// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfEnumMember
fn get_declared_type_of_enum_member(
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
    let enum_type = match program.symbol(symbol).parent {
        Some(parent) => get_declared_type_of_enum(checker, program, parent),
        None => checker.error_type(),
    };
    match checker
        .declared_type_links
        .try_get(&symbol)
        .and_then(|l| l.declared_type)
    {
        Some(t) => t,
        None => {
            checker.declared_type_links.get(symbol).declared_type = Some(enum_type);
            enum_type
        }
    }
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
    if checker
        .resolved_symbol_check_flags(program, symbol)
        .contains(CheckFlags::INSTANTIATED)
        || checker
            .value_symbol_links
            .try_get(&symbol)
            .is_some_and(|l| l.target.is_some() && l.mapper.is_some())
    {
        return super::mapped_types::get_type_of_instantiated_symbol(checker, program, symbol);
    }
    if checker
        .resolved_symbol_check_flags(program, symbol)
        .contains(CheckFlags::MAPPED)
    {
        return super::mapped_types::get_type_of_mapped_symbol(checker, program, symbol);
    }
    // A synthesized union/intersection property carries its combined type in the
    // checker's transient arena, not in the program (Go's transient symbol with
    // `valueSymbolLinks.resolvedType`).
    if super::is_synthesized_symbol(symbol) {
        return get_type_of_synthesized_symbol(checker, program, symbol, globals);
    }
    // Build the value type against the view of the file that DECLARES the
    // symbol. In a multi-file program a value symbol (variable / property /
    // function / method) may be declared in another file — most commonly a lib
    // file, e.g. `Array.push` and its parameters in `lib.es5.d.ts` — whose
    // declaration nodes are NOT in `program.arena()`. Reading them through the
    // current arena (in `get_type_of_variable_or_property` /
    // `get_type_of_func_class_enum_module`) indexes out of bounds. Mirror the
    // owning-view switch already used by `get_declared_type_of_symbol` /
    // `get_constraint_of_type_parameter`; it is guarded by `file_handle()` so it
    // happens at most once (the owning view returns itself for `symbol`, so no
    // infinite recursion). For a single-file program `view_for_symbol` is `None`
    // and this is a no-op.
    if let Some(view) = program.view_for_symbol(symbol) {
        if view.file_handle() != program.file_handle() {
            let owner_globals = view.globals();
            return get_type_of_symbol(checker, view.as_ref(), symbol, owner_globals);
        }
    }
    let flags = program.symbol(symbol).flags;
    if flags.intersects(SymbolFlags::VARIABLE | SymbolFlags::PROPERTY) {
        return get_type_of_variable_or_property(checker, program, symbol, globals);
    }
    // An enum member's value type is its literal type (`E.A`); the enum object's
    // value type is the members-bearing object that `E.A` reads from.
    if flags.intersects(SymbolFlags::ENUM_MEMBER) {
        return get_type_of_enum_member(checker, program, symbol);
    }
    if flags.intersects(SymbolFlags::ENUM) {
        return get_type_of_enum_object(checker, program, symbol);
    }
    // A class referenced as a VALUE has the static (constructor) side type: an
    // anonymous object type whose members are the class's STATIC members (the
    // binder's `exports` table). The INSTANCE type (the declared type, used at
    // type-reference positions via `get_declared_type_of_symbol`) is a SEPARATE
    // type, so `Other.Baz` (a static member access on the class value) reads off
    // the static side while `x: Other` (a type reference) reads the instance
    // type. `get_type_of_func_class_enum_module` builds the anonymous object from
    // `symbol.exports` (no call signatures, since a `ClassDeclaration` is not a
    // signature-bearing declaration), exactly Go's class arm.
    //
    // DEFER(phase-4-checker-later): construct signatures (so the class value's
    // `new`-applicability and the `instanceof` Function-subtype path read the
    // class's signatures), static-member inheritance from a base class's static
    // side, and the `extends <type-parameter>` static-side intersection
    // (`getBaseTypeVariableOfClass`). blocked-by: construct-signature collection
    // + base-class static merge. `new C(...)` reads the constructed INSTANCE type
    // via `get_declared_type_of_symbol` directly (see `check_new_expression`), so
    // it is unaffected by this static-side routing.
    // Go: internal/checker/checker.go:Checker.getTypeOfSymbol (SymbolFlagsClass)
    //     -> getTypeOfFuncClassEnumModuleWorker
    if flags.intersects(SymbolFlags::CLASS) {
        return get_type_of_func_class_enum_module(checker, program, symbol);
    }
    // A pure interface symbol has no value type; the reachable subset keeps the
    // declared (instance) type here (Go falls through to `errorType`, but the
    // declared type is the closest reachable behavior for the rare value-position
    // reference of an interface-only symbol).
    if flags.intersects(SymbolFlags::INTERFACE) {
        return get_declared_type_of_symbol(checker, program, symbol, globals);
    }
    // A function or method symbol's type is an anonymous object type carrying
    // its call signatures (Go's `getTypeOfFuncClassEnumModule`), so a call
    // expression can resolve those signatures (4q), and the iterator protocol
    // (4ah) can read a `[Symbol.iterator]()`/`next()` method's return type.
    if flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD) {
        return get_type_of_func_class_enum_module(checker, program, symbol);
    }
    if flags.intersects(SymbolFlags::ACCESSOR) {
        return get_type_of_accessors(checker, program, symbol, globals);
    }
    // A namespace/module symbol's value type is an anonymous object whose
    // members are the namespace's exports, so `N.x` reads an exported member's
    // type (Go's `getTypeOfFuncClassEnumModuleWorker`, module case). A merged
    // namespace (`namespace N {...} namespace N {...}`) has the binder
    // accumulate every export into the one symbol's `exports` table, so its
    // value type sees the combined members.
    if flags.intersects(SymbolFlags::MODULE) {
        return get_type_of_module(checker, program, symbol);
    }
    // An `import`/`export` alias's value type is the type of the symbol it
    // resolves to (Go's `getTypeOfSymbol` -> alias arm -> `getTypeOfSymbol(
    // resolveAlias(symbol))`): a named/default import yields the imported
    // declaration's type, a namespace import (`import * as ns`) yields the
    // target module's object type so `ns.x` reads an export.
    // Go: internal/checker/checker.go:Checker.getTypeOfSymbol (SymbolFlagsAlias)
    if flags.intersects(SymbolFlags::ALIAS) {
        return match resolve_alias(checker, program, symbol) {
            Some(target) => get_type_of_symbol(checker, program, target, globals),
            None => checker.error_type(),
        };
    }
    checker.error_type()
}

// Builds (or returns the cached) value type of a namespace/module symbol: an
// anonymous object type whose members are the namespace's exports (Go's
// `getTypeOfFuncClassEnumModuleWorker` module case, which delegates member
// resolution to `getExportsOfSymbol`). The object's symbol is the namespace,
// so it prints as `typeof N`.
//
// DEFER(phase-4-checker-C-D2+): the shorthand-ambient-module `any`, the
// CommonJS `export =` re-resolution, and `strictNullChecks` optional widening.
// blocked-by: ambient/CommonJS module resolution + alias targets.
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModuleWorker
fn get_type_of_module(
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
    let members = program.symbol(symbol).exports.clone();
    let properties: Vec<SymbolId> = members.values().copied().collect();
    let object = ObjectType {
        members,
        properties,
        ..Default::default()
    };
    let t = checker.new_object_type(ObjectFlags::ANONYMOUS, Some(symbol), object);
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

// === Cross-module import/alias resolution (Round 14) ===
//
// Ports the reachable subset of Go's `resolveAlias` chain so a reference to a
// name imported with `import { x } from "m"` / `import d from "m"` / `import *
// as ns from "m"` / `import x = require("m")` resolves to the target module's
// export instead of cascading into TS2304 (and clears the TS2339 amplified by
// it on namespace/default member access). The specifier -> module-symbol bridge
// is `BoundProgram::resolve_module_symbol` (the compiler records each import's
// resolved target file).
//
// DEFER(phase-4): `export { x } from "m"` re-export specifiers, `export =`
// supplemental type exports, `export *` star re-exports, synthetic-default /
// esModuleInterop wrappers, and the type-only alias markers. blocked-by: the
// re-export + synthetic-default surfaces (a later round).

/// Reports whether `node` is an import/export ALIAS declaration — the local
/// name node whose binder symbol carries [`SymbolFlags::ALIAS`].
// Go: internal/checker/checker.go:isAliasSymbolDeclaration (reachable subset)
fn is_alias_symbol_declaration(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::ImportEqualsDeclaration
            | Kind::NamespaceImport
            | Kind::ImportSpecifier
            | Kind::ExportSpecifier
            | Kind::ImportClause
            | Kind::NamespaceExport
    )
}

/// The alias DECLARATION node carrying `symbol` (Go's
/// `getDeclarationOfAliasSymbol`): the first declaration that is an
/// import/export alias declaration.
// Go: internal/checker/checker.go:Checker.getDeclarationOfAliasSymbol
pub(crate) fn get_declaration_of_alias_symbol(
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> Option<NodeId> {
    // Read declaration nodes through the view that owns `symbol`: in a
    // multi-file program each file keeps its own arena, so a merged symbol's
    // `declarations` may reference nodes in another file (e.g. the exported
    // function behind `import { foo } from "./other"`). Using
    // `program.arena()` (the file currently under check) would index OOB.
    let owner = program.view_for_symbol(symbol);
    let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
    prog.symbol(symbol)
        .declarations
        .iter()
        .rev()
        .copied()
        .find(|&d| is_alias_symbol_declaration(prog.arena(), d))
}

/// Resolves an alias `symbol` to the non-alias symbol it ultimately denotes
/// (Go's `resolveAlias`), caching the result (a missing export is cached as
/// `None`, Go's `unknownSymbol`, after its TS2305 is reported once). A circular
/// import alias resolves to `None`.
// Go: internal/checker/checker.go:Checker.resolveAlias
pub(crate) fn resolve_alias(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> Option<SymbolId> {
    if let Some(&cached) = checker.alias_targets.get(&symbol) {
        return cached;
    }
    // Circularity guard (Go's pushTypeResolution(AliasTarget)): a re-entrant
    // resolve of the same alias yields `unknown` rather than recursing forever.
    if !checker.aliases_resolving.insert(symbol) {
        checker.alias_targets.insert(symbol, None);
        return None;
    }
    let target = get_declaration_of_alias_symbol(program, symbol).and_then(|node| {
        let owner = program.view_for_symbol(symbol);
        let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
        get_target_of_alias_declaration(checker, prog, node)
    });
    checker.aliases_resolving.remove(&symbol);
    checker.alias_targets.insert(symbol, target);
    target
}

/// `resolveSymbolEx(symbol, dontResolveAlias=false)`: a non-local alias is
/// followed to its target; anything else is returned unchanged.
// Go: internal/checker/checker.go:Checker.resolveSymbolEx
fn resolve_symbol(checker: &mut Checker, program: &dyn BoundProgram, symbol: SymbolId) -> SymbolId {
    if program.symbol(symbol).flags.contains(SymbolFlags::ALIAS) {
        if let Some(target) = resolve_alias(checker, program, symbol) {
            return target;
        }
    }
    symbol
}

/// Dispatches an alias declaration to its target resolver (Go's
/// `getTargetOfAliasDeclaration`).
// Go: internal/checker/checker.go:Checker.getTargetOfAliasDeclaration
fn get_target_of_alias_declaration(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> Option<SymbolId> {
    match program.arena().kind(node) {
        Kind::ImportSpecifier => get_target_of_import_specifier(checker, program, node),
        Kind::ImportClause => get_target_of_import_clause(checker, program, node),
        Kind::NamespaceImport => get_target_of_namespace_import(checker, program, node),
        Kind::ImportEqualsDeclaration => {
            get_target_of_import_equals_declaration(checker, program, node)
        }
        // DEFER(phase-4): ExportSpecifier / NamespaceExport / ExportAssignment.
        _ => None,
    }
}

/// Resolves `import { x } from "m"` / `import { x as y } from "m"` to the
/// module's exported `x` (Go's `getTargetOfImportSpecifier` ->
/// `getExternalModuleMember`).
// Go: internal/checker/checker.go:Checker.getTargetOfImportSpecifier
fn get_target_of_import_specifier(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> Option<SymbolId> {
    let import_decl = enclosing_import_or_export_declaration(program.arena(), node)?;
    // `import { default as X } from "m"` imports the module's DEFAULT export
    // (subject to synthetic-default semantics), NOT a member literally named
    // `default`; Go routes it through `getTargetOfModuleDefault`. Handling it as
    // a plain named member would falsely report TS2305 when the default is
    // synthetic (`allowSyntheticDefaultImports`/`esModuleInterop`).
    // Go: internal/checker/checker.go:Checker.getTargetOfImportSpecifier (ModuleExportNameIsDefault)
    if let Some((_, name_text)) = import_specifier_name(program.arena(), node) {
        if name_text == "default" {
            if let Some(specifier) = module_specifier_text(program.arena(), import_decl) {
                if let Some(module_symbol) = resolve_external_module_name(program, &specifier) {
                    return get_target_of_module_default(
                        checker,
                        program,
                        module_symbol,
                        &specifier,
                        node,
                    );
                }
            }
            return None;
        }
    }
    get_external_module_member(checker, program, import_decl, node)
}

/// Resolves the default import binding `import d from "m"` to the module's
/// `default` export (Go's `getTargetOfImportClause` ->
/// `getTargetOfModuleDefault`).
// Go: internal/checker/checker.go:Checker.getTargetOfImportClause
fn get_target_of_import_clause(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> Option<SymbolId> {
    let import_decl = program.arena().parent(node)?;
    let specifier = module_specifier_text(program.arena(), import_decl)?;
    let module_symbol = resolve_external_module_name(program, &specifier)?;
    get_target_of_module_default(checker, program, module_symbol, &specifier, node)
}

/// The `default` export of a module, reported (TS1192) when absent (Go's
/// `getTargetOfModuleDefault` reachable subset — synthetic-default /
/// esModuleInterop wrappers DEFER).
// Go: internal/checker/checker.go:Checker.getTargetOfModuleDefault
fn get_target_of_module_default(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    module_symbol: SymbolId,
    _specifier: &str,
    _node: NodeId,
) -> Option<SymbolId> {
    let resolved = resolve_external_module_symbol(checker, program, module_symbol);
    if let Some(default) = get_export_of_module(checker, program, resolved, "default") {
        return Some(default);
    }
    // No explicit `default` export. Go would either resolve a SYNTHETIC default
    // (under `allowSyntheticDefaultImports`/`esModuleInterop`, e.g. a CommonJS
    // module imported as its whole shape) or report TS1192/TS2613. Both branches
    // need the synthetic-default analysis, which is DEFER'd this round; until
    // then the default target is left unresolved WITHOUT a diagnostic, so a
    // synthetic-default case (tsc: no error) is not turned into a false positive.
    // The referenced binding still resolves (to the unresolved alias), so it
    // does not cascade a TS2304 either.
    // DEFER(phase-4): canHaveSyntheticDefault + the no-default TS1192/TS2613
    // diagnostics. blocked-by: `esModuleInterop`/`allowSyntheticDefaultImports`
    // synthetic-default analysis.
    None
}

/// Resolves `import * as ns from "m"` to the module symbol itself, so `ns.x`
/// reads the module's exports (Go's `getTargetOfNamespaceImport` ->
/// `resolveESModuleSymbol`; the synthetic-default wrapper DEFER).
// Go: internal/checker/checker.go:Checker.getTargetOfNamespaceImport
fn get_target_of_namespace_import(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> Option<SymbolId> {
    // node: NamespaceImport -> ImportClause -> ImportDeclaration
    let import_clause = program.arena().parent(node)?;
    let import_decl = program.arena().parent(import_clause)?;
    let specifier = module_specifier_text(program.arena(), import_decl)?;
    let module_symbol = resolve_external_module_name(program, &specifier)?;
    Some(resolve_external_module_symbol(
        checker,
        program,
        module_symbol,
    ))
}

/// Resolves `import x = require("m")` (Go's
/// `getTargetOfImportEqualsDeclaration`, reachable subset: the
/// `require`/external-module form; entity-name `import x = M.N` DEFER).
// Go: internal/checker/checker.go:Checker.getTargetOfImportEqualsDeclaration
fn get_target_of_import_equals_declaration(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> Option<SymbolId> {
    let NodeData::ImportEqualsDeclaration(d) = program.arena().data(node) else {
        return None;
    };
    let module_reference = d.module_reference;
    // Only the `require("m")` / external-module form is ported; an entity-name
    // module reference (`import x = M.N`) needs `resolveEntityName` alias
    // chaining (DEFER).
    let specifier = external_module_reference_text(program.arena(), module_reference)?;
    let module_symbol = resolve_external_module_name(program, &specifier)?;
    Some(resolve_external_module_symbol(
        checker,
        program,
        module_symbol,
    ))
}

/// `getExternalModuleMember`: looks the import specifier's exported name up in
/// the resolved module's exports, reporting TS2305 when the module has no such
/// exported member (the GUARD against a silent resolve).
// Go: internal/checker/checker.go:Checker.getExternalModuleMember
fn get_external_module_member(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    import_decl: NodeId,
    specifier_node: NodeId,
) -> Option<SymbolId> {
    let specifier = module_specifier_text(program.arena(), import_decl)?;
    let (name_node, name_text) = import_specifier_name(program.arena(), specifier_node)?;
    // The module must resolve+load for the alias to have a target. When it does
    // not (DEFER: TS2307), the alias is left unresolved.
    let module_symbol = resolve_external_module_name(program, &specifier)?;
    let target = resolve_external_module_symbol(checker, program, module_symbol);
    match get_export_of_module(checker, program, target, &name_text) {
        Some(symbol) => Some(symbol),
        None => {
            // GUARD: a named import of a NON-EXISTENT export is TS2305, never a
            // silent resolve (and never a TS2304 on the reference).
            let module_name = format!("\"{specifier}\"");
            checker.error_skipping_leading_trivia(
                program,
                name_node,
                &tsgo_diagnostics::MODULE_0_HAS_NO_EXPORTED_MEMBER_1,
                &[&module_name, &name_text],
            );
            None
        }
    }
}

/// `resolveExternalModuleName`: maps an import specifier string to the target
/// module's symbol via the program's specifier -> module-symbol bridge.
// Go: internal/checker/checker.go:Checker.resolveExternalModuleName (resolveExternalModule)
pub(crate) fn resolve_external_module_name(
    program: &dyn BoundProgram,
    specifier: &str,
) -> Option<SymbolId> {
    program.resolve_module_symbol(program.file_handle(), specifier)
}

/// `resolveExternalModuleSymbol`: a module defined by `export = x` resolves to
/// that export's symbol; otherwise the module symbol itself.
// Go: internal/checker/checker.go:Checker.resolveExternalModuleSymbol
pub(crate) fn resolve_external_module_symbol(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    module_symbol: SymbolId,
) -> SymbolId {
    if let Some(&export_equals) = program
        .symbol(module_symbol)
        .exports
        .get(tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_EXPORT_EQUALS)
    {
        return resolve_symbol(checker, program, export_equals);
    }
    module_symbol
}

/// Looks `name` up in a (resolved) module symbol's exports and follows it
/// through any alias chain (Go's `getExportOfModule` over `getExportsOfModule`;
/// the `export *` visit is DEFER).
// Go: internal/checker/checker.go:Checker.getExportOfModule / getExportsOfModule
fn get_export_of_module(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    module_symbol: SymbolId,
    name: &str,
) -> Option<SymbolId> {
    let export = *program.symbol(module_symbol).exports.get(name)?;
    Some(resolve_symbol(checker, program, export))
}

/// Walks up from an import/export specifier to its enclosing `ImportDeclaration`
/// / `ExportDeclaration` (the node carrying the module specifier).
fn enclosing_import_or_export_declaration(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    let mut current = arena.parent(node);
    while let Some(n) = current {
        match arena.kind(n) {
            Kind::ImportDeclaration | Kind::ExportDeclaration => return Some(n),
            _ => current = arena.parent(n),
        }
    }
    None
}

/// The (unquoted) module specifier text of an `ImportDeclaration` /
/// `ExportDeclaration`, if it has one.
fn module_specifier_text(arena: &NodeArena, node: NodeId) -> Option<String> {
    match arena.data(node) {
        NodeData::ImportDeclaration(d) => Some(arena.text(d.module_specifier).to_string()),
        NodeData::ExportDeclaration(d) => d.module_specifier.map(|m| arena.text(m).to_string()),
        _ => None,
    }
}

/// The exported name an `ImportSpecifier` refers to (its `propertyName` if
/// present, else its `name`) plus the node to anchor a not-found diagnostic on.
fn import_specifier_name(arena: &NodeArena, node: NodeId) -> Option<(NodeId, String)> {
    match arena.data(node) {
        NodeData::ImportSpecifier(d) => {
            let name_node = d.property_name.unwrap_or(d.name);
            Some((name_node, arena.text(name_node).to_string()))
        }
        _ => None,
    }
}

/// The (unquoted) module specifier of an `ExternalModuleReference`
/// (`require("m")`), if `node` is one.
fn external_module_reference_text(arena: &NodeArena, node: NodeId) -> Option<String> {
    match arena.data(node) {
        NodeData::ExternalModuleReference(d) => {
            let expr = d.expression;
            if arena.kind(expr) == Kind::StringLiteral {
                return Some(arena.text(expr).to_string());
            }
            None
        }
        _ => None,
    }
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
    let call_signatures = get_signatures_of_symbol(checker, program, symbol);
    // Expando members (`function f(){}; f.x = v`) live in the function symbol's
    // `exports` table (the binder's `bindDeferredExpandoAssignment`), and a
    // function/class/enum/module value type resolves its members from that table
    // (Go's `getTypeOfFuncClassEnumModuleWorker` -> anonymous object type whose
    // members are the symbol's exports). Exposing them as properties is what lets
    // `f.x` resolve instead of reporting a spurious TS2339.
    let sym = program.symbol(symbol);
    let members = sym.exports.clone();
    let properties: Vec<SymbolId> = members.values().copied().collect();
    let mut index_infos = Vec::new();
    if sym.flags.intersects(SymbolFlags::CLASS) {
        let type_param_map =
            build_type_parameter_name_map(checker, program, &sym.declarations);
        index_infos = collect_index_infos_of_members(
            checker,
            program,
            &members,
            &type_param_map,
            program.globals(),
        );
    }
    let object = ObjectType {
        call_signatures,
        members,
        properties,
        index_infos,
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
    symbol: SymbolId,
) -> Vec<SignatureId> {
    // Declaration nodes may live in another file's arena (lib globals, merged
    // cross-file symbols). Read them through the view that owns `symbol`.
    let owner = program.view_for_symbol(symbol);
    let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
    let declarations = prog.symbol(symbol).declarations.clone();
    let mut result = Vec::new();
    for (i, &decl) in declarations.iter().enumerate() {
        // A method member contributes its `MethodSignature`/`MethodDeclaration`
        // call signature (4ah), alongside the function-declaration case (4q). A
        // function/constructor *type* node (`(x: number) => void`) contributes
        // its call signature too (4bj), so a contextual function type yields the
        // signature that types an assigned arrow/function expression.
        if !matches!(
            prog.arena().kind(decl),
            Kind::FunctionDeclaration
                | Kind::MethodSignature
                | Kind::MethodDeclaration
                | Kind::FunctionType
                | Kind::ConstructorType
        ) {
            continue;
        }
        // Skip the *implementation* of an overloaded function/method: a node
        // with a body whose immediately-preceding sibling declaration has the
        // same parent and kind and ends where this one begins is the
        // implementation that follows the overload signatures, and is not a
        // callable signature (Go's `getSignaturesOfSymbol`). This is what makes
        // `function f(x:number):void; function f(x:string):void; function f(x:any){}`
        // expose exactly the two overloads, so an unmatched call elaborates
        // against them rather than silently resolving against the `any`
        // implementation.
        if i > 0 && function_like_has_body(prog, decl) {
            let previous = declarations[i - 1];
            if prog.arena().parent(decl) == prog.arena().parent(previous)
                && prog.arena().kind(decl) == prog.arena().kind(previous)
                && prog.arena().loc(decl).pos() == prog.arena().loc(previous).end()
            {
                continue;
            }
        }
        result.push(get_signature_from_declaration(checker, prog, decl));
    }
    result
}

// Reports whether a function-like declaration has a body (Go's `decl.Body() !=
// nil`), used to detect an overload implementation node.
fn function_like_has_body(program: &dyn BoundProgram, decl: NodeId) -> bool {
    match program.arena().data(decl) {
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.body.is_some(),
        NodeData::MethodDeclaration(d) => d.body.is_some(),
        NodeData::ConstructorDeclaration(d) => d.body.is_some(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.body.is_some()
        }
        _ => false,
    }
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
        // A function/constructor type node (`(x: number) => void`) contributes
        // its parameters and (return) type the same way (4bj), so a contextual
        // function type's call signature exposes its parameter types.
        NodeData::FunctionType(d) | NodeData::ConstructorType(d) => {
            (d.parameters.nodes.clone(), d.type_node)
        }
        // DEFER(phase-4-checker-4q+): accessor/function-expression/arrow
        // declarations. blocked-by: those declaration kinds.
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
    // A constructor-type / construct-signature declaration produces a construct
    // signature (Go's `getSignatureFromDeclaration` sets `SignatureFlagsConstruct`
    // via `isConstructSignatureDeclaration`), which selects the construct-signature
    // relation and the `new (...)` printed form.
    let mut flags = if matches!(
        program.arena().kind(declaration),
        Kind::ConstructorType | Kind::ConstructSignature
    ) {
        SignatureFlags::CONSTRUCT
    } else {
        SignatureFlags::NONE
    };
    // A trailing rest parameter (`...args`) marks the signature so the
    // call/argument path expands the rest element type per position and the
    // arity check accepts any number of trailing arguments (Go's
    // `getSignatureFromDeclaration` sets `SignatureFlagsHasRestParameter` via
    // `hasRestParameter`).
    // Go: internal/checker/checker.go:getSignatureFromDeclaration (hasRestParameter)
    if has_rest_parameter(program, &param_nodes) {
        flags |= SignatureFlags::HAS_REST_PARAMETER;
    }
    if signature_declaration_has_abstract_modifier(program, declaration) {
        flags |= SignatureFlags::ABSTRACT;
    }
    let mut signature = Signature::new(flags);
    signature.declaration = Some(declaration);
    signature.type_parameters = get_signature_type_parameters(checker, program, declaration);
    signature.parameters = parameters;
    signature.min_argument_count = min_argument_count;
    signature.resolved_return_type = Some(resolved_return_type);
    checker.new_signature(signature)
}

// Collects the type-parameter types declared by a function-like declaration's
// `<...>` list (Go's `getSignatureFromDeclaration` populating
// `signature.typeParameters` from `getEffectiveTypeParameterDeclarations`).
// Each declared type-parameter symbol is mapped to its (cached) type-parameter
// type, so the same `T` referenced inside the parameter/return annotations —
// resolved by name through `resolve_type_parameter_in_scope` ->
// `get_declared_type_of_type_parameter` — shares the type id stored here.
//
// DEFER(phase-4-checker-C-B2+): the inherited (outer) type parameters of a
// nested generic declaration and JSDoc type-parameter tags.
// blocked-by: outer-type-parameter threading + JSDoc reparse.
// Go: internal/checker/checker.go:Checker.getSignatureFromDeclaration (typeParameters)
fn get_signature_type_parameters(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declaration: NodeId,
) -> Vec<TypeId> {
    let list = match program.arena().data(declaration) {
        NodeData::FunctionDeclaration(d) => d.type_parameters.clone(),
        NodeData::MethodSignature(d) => d.type_parameters.clone(),
        NodeData::MethodDeclaration(d) => d.type_parameters.clone(),
        NodeData::FunctionType(d) | NodeData::ConstructorType(d) => d.type_parameters.clone(),
        _ => None,
    };
    let mut result = Vec::new();
    if let Some(list) = list {
        for node in list.nodes {
            match program.symbol_of_node(node) {
                Some(sym) => result.push(get_declared_type_of_type_parameter(checker, sym)),
                None => result.push(checker.new_type_parameter(None)),
            }
        }
    }
    result
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

fn signature_declaration_has_abstract_modifier(
    program: &dyn BoundProgram,
    declaration: NodeId,
) -> bool {
    let modifiers = match program.arena().data(declaration) {
        NodeData::ConstructorType(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers
        .map(|m| m.modifier_flags.contains(ModifierFlags::ABSTRACT))
        .unwrap_or(false)
}

// Reports whether a signature declaration's LAST parameter is a rest parameter
// (`...args`), the predicate Go's `getSignatureFromDeclaration` uses to set
// `SignatureFlagsHasRestParameter` (Go's `hasRestParameter` + `isRestParameter`:
// the last parameter carries a `...` token).
// Go: internal/checker/checker.go:hasRestParameter / isRestParameter
fn has_rest_parameter(program: &dyn BoundProgram, param_nodes: &[NodeId]) -> bool {
    match param_nodes.last() {
        Some(&last) => matches!(
            program.arena().data(last),
            NodeData::ParameterDeclaration(d) if d.dot_dot_dot_token.is_some()
        ),
        None => false,
    }
}

// Reports whether a variable-like declaration is optional for the `| undefined`
// injection (Go's `isOptionalDeclaration`): a parameter with a `?` question
// token, or a property signature/declaration whose postfix token is `?`. (A
// parameter initializer/rest only affects arity, not the read optionality, so
// unlike `is_optional_parameter` they do not count here.)
// Go: internal/ast/utilities.go:isOptionalDeclaration
fn is_optional_declaration(program: &dyn BoundProgram, declaration: NodeId) -> bool {
    match program.arena().data(declaration) {
        NodeData::ParameterDeclaration(d) => d.question_token.is_some(),
        NodeData::PropertySignature(d) | NodeData::PropertyDeclaration(d) => d
            .postfix_token
            .is_some_and(|tok| program.arena().kind(tok) == Kind::QuestionToken),
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

fn declaration_of_kind(
    program: &dyn BoundProgram,
    symbol: SymbolId,
    kind: Kind,
) -> Option<NodeId> {
    program
        .symbol(symbol)
        .declarations
        .iter()
        .find(|&&d| program.arena().kind(d) == kind)
        .copied()
}

// Returns the auto-accessor property declaration for a symbol, if any (Go's
// `IsAutoAccessorPropertyDeclaration` on `GetDeclarationOfKind(..., PropertyDeclaration)`).
fn auto_accessor_property_declaration(
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> Option<NodeId> {
    declaration_of_kind(program, symbol, Kind::PropertyDeclaration).filter(|&decl| {
        is_auto_accessor_property_declaration(program.arena(), decl)
    })
}

fn is_auto_accessor_property_declaration(arena: &NodeArena, node: NodeId) -> bool {
    if arena.kind(node) != Kind::PropertyDeclaration {
        return false;
    }
    let modifiers = match arena.data(node) {
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
        _ => return false,
    };
    modifiers
        .map(|m| m.modifier_flags.contains(ModifierFlags::ACCESSOR))
        .unwrap_or(false)
}

// Returns the explicit read type of a getter symbol from a getter and/or setter
// annotation (not body inference). Mirrors the annotation-resolution prefix of
// Go's `getTypeOfAccessors`.
// Go: internal/checker/checker.go:Checker.getAnnotatedAccessorType(19953)
pub(crate) fn get_explicit_accessor_return_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> Option<TypeId> {
    let getter = declaration_of_kind(program, symbol, Kind::GetAccessor);
    let setter = declaration_of_kind(program, symbol, Kind::SetAccessor);
    let auto_accessor = auto_accessor_property_declaration(program, symbol);
    getter
        .and_then(|g| get_annotated_accessor_type(checker, program, g, globals))
        .or_else(|| {
            setter.and_then(|s| get_annotated_accessor_type(checker, program, s, globals))
        })
        .or_else(|| {
            auto_accessor.and_then(|a| get_annotated_accessor_type(checker, program, a, globals))
        })
}

// Returns the type-annotation node of an accessor declaration, if any (Go's
// `getAnnotatedAccessorTypeNode`).
// Go: internal/checker/checker.go:Checker.getAnnotatedAccessorTypeNode(19961)
fn get_annotated_accessor_type_node(program: &dyn BoundProgram, accessor: NodeId) -> Option<NodeId> {
    match program.arena().data(accessor) {
        NodeData::GetAccessorDeclaration(d) => d.type_node,
        NodeData::PropertyDeclaration(d) => d.type_node,
        NodeData::SetAccessorDeclaration(d) => d.parameters.nodes.first().and_then(|&param| {
            match program.arena().data(param) {
                NodeData::ParameterDeclaration(p) => p.type_node,
                _ => None,
            }
        }),
        _ => None,
    }
}

// Returns the annotated type of an accessor declaration, if any (Go's
// `getAnnotatedAccessorType`).
// Go: internal/checker/checker.go:Checker.getAnnotatedAccessorType(19953)
fn get_annotated_accessor_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    accessor: NodeId,
    globals: Option<&SymbolTable>,
) -> Option<TypeId> {
    let type_node = get_annotated_accessor_type_node(program, accessor)?;
    Some(get_type_from_type_node(checker, program, type_node, globals))
}

// Resolves the read type of an accessor symbol: getter annotation, else setter
// annotation, else getter body return-type inference (Go's `getTypeOfAccessors`).
// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18370)
pub fn get_type_of_accessors(
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
    if !checker.accessors_type_resolving.insert(symbol) {
        checker.accessor_type_resolution_cyclic = true;
        return checker.error_type();
    }
    let auto_accessor = auto_accessor_property_declaration(program, symbol);
    let getter = declaration_of_kind(program, symbol, Kind::GetAccessor);
    let setter = declaration_of_kind(program, symbol, Kind::SetAccessor);
    let mut t = getter.and_then(|g| get_annotated_accessor_type(checker, program, g, globals));
    if t.is_none() {
        t = setter.and_then(|s| get_annotated_accessor_type(checker, program, s, globals));
    }
    if t.is_none() {
        t = auto_accessor
            .and_then(|a| get_annotated_accessor_type(checker, program, a, globals));
    }
    if t.is_none() {
        if let Some(getter) = getter {
            let has_body = match program.arena().data(getter) {
                NodeData::GetAccessorDeclaration(d) => d.body.is_some(),
                _ => false,
            };
            if has_body {
                t = Some(checker.get_return_type_from_body(program, getter));
            }
        }
    }
    if t.is_none() {
        if let Some(accessor) = auto_accessor {
            if let Some(initializer) = variable_declaration_initializer(program, accessor) {
                let initializer_type = checker.check_expression(program, initializer);
                let inferred = get_widened_literal_type_for_initializer(
                    checker,
                    program,
                    accessor,
                    initializer_type,
                );
                t = Some(checker.get_widened_type(inferred));
            }
        }
    }
    let mut t = match t {
        Some(t) => t,
        None => {
            let sym_name = super::nodebuilder::symbol_to_string(program, symbol);
            if let Some(setter) = setter {
                checker.add_error_or_suggestion(
                    program,
                    checker.no_implicit_any(),
                    setter,
                    &tsgo_diagnostics::PROPERTY_0_IMPLICITLY_HAS_TYPE_ANY_BECAUSE_ITS_SET_ACCESSOR_LACKS_A_PARAMETER_TYPE_ANNOTATION,
                    &[sym_name.as_str()],
                );
            } else if let Some(getter) = getter {
                checker.add_error_or_suggestion(
                    program,
                    checker.no_implicit_any(),
                    getter,
                    &tsgo_diagnostics::PROPERTY_0_IMPLICITLY_HAS_TYPE_ANY_BECAUSE_ITS_GET_ACCESSOR_LACKS_A_RETURN_TYPE_ANNOTATION,
                    &[sym_name.as_str()],
                );
            } else if let Some(accessor) = auto_accessor {
                checker.add_error_or_suggestion(
                    program,
                    checker.no_implicit_any(),
                    accessor,
                    &tsgo_diagnostics::MEMBER_0_IMPLICITLY_HAS_AN_1_TYPE,
                    &[sym_name.as_str(), "any"],
                );
            }
            checker.any_type()
        }
    };
    checker.accessors_type_resolving.remove(&symbol);
    if checker.accessor_type_resolution_cyclic {
        let sym_name = super::nodebuilder::symbol_to_string(program, symbol);
        if getter
            .and_then(|g| get_annotated_accessor_type_node(program, g))
            .is_some()
        {
            checker.error(
                program,
                getter.expect("getter annotation implies getter decl"),
                &tsgo_diagnostics::X_0_IS_REFERENCED_DIRECTLY_OR_INDIRECTLY_IN_ITS_OWN_TYPE_ANNOTATION,
                &[sym_name.as_str()],
            );
        } else if setter
            .and_then(|s| get_annotated_accessor_type_node(program, s))
            .is_some()
        {
            checker.error(
                program,
                setter.expect("setter annotation implies setter decl"),
                &tsgo_diagnostics::X_0_IS_REFERENCED_DIRECTLY_OR_INDIRECTLY_IN_ITS_OWN_TYPE_ANNOTATION,
                &[sym_name.as_str()],
            );
        } else if auto_accessor
            .and_then(|a| get_annotated_accessor_type_node(program, a))
            .is_some()
        {
            checker.error(
                program,
                auto_accessor.expect("auto-accessor annotation implies property decl"),
                &tsgo_diagnostics::X_0_IS_REFERENCED_DIRECTLY_OR_INDIRECTLY_IN_ITS_OWN_TYPE_ANNOTATION,
                &[sym_name.as_str()],
            );
        } else if let Some(getter) = getter {
            if checker.no_implicit_any() {
                checker.error(
                    program,
                    getter,
                    &tsgo_diagnostics::FUNCTION_IMPLICITLY_HAS_RETURN_TYPE_ANY_BECAUSE_IT_DOES_NOT_HAVE_A_RETURN_TYPE_ANNOTATION_AND_IS_REFERENCED_DIRECTLY_OR_INDIRECTLY_IN_ONE_OF_ITS_RETURN_EXPRESSIONS,
                    &[sym_name.as_str()],
                );
            }
        }
        t = checker.any_type();
        checker.accessor_type_resolution_cyclic = false;
    }
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

// Resolves the write type of an accessor symbol (setter annotation, else read
// type). Go's `getWriteTypeOfAccessors`.
// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18429)
pub fn get_write_type_of_accessors(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    if let Some(cached) = checker
        .value_symbol_links
        .try_get(&symbol)
        .and_then(|l| l.write_type)
    {
        return cached;
    }
    if !checker.accessors_write_type_resolving.insert(symbol) {
        checker.accessor_write_type_resolution_cyclic = true;
        return checker.error_type();
    }
    let mut setter = declaration_of_kind(program, symbol, Kind::SetAccessor);
    if setter.is_none() {
        setter = auto_accessor_property_declaration(program, symbol);
    }
    let mut write_type = setter.and_then(|s| get_annotated_accessor_type(checker, program, s, globals));
    checker.accessors_write_type_resolving.remove(&symbol);
    if checker.accessor_write_type_resolution_cyclic {
        let sym_name = super::nodebuilder::symbol_to_string(program, symbol);
        if let Some(setter) = setter {
            if get_annotated_accessor_type_node(program, setter).is_some() {
                checker.error(
                    program,
                    setter,
                    &tsgo_diagnostics::X_0_IS_REFERENCED_DIRECTLY_OR_INDIRECTLY_IN_ITS_OWN_TYPE_ANNOTATION,
                    &[sym_name.as_str()],
                );
            }
        }
        write_type = Some(checker.any_type());
        checker.accessor_write_type_resolution_cyclic = false;
    }
    let write_type = match write_type {
        Some(t) => t,
        None => get_type_of_accessors(checker, program, symbol, globals),
    };
    checker.value_symbol_links.get(symbol).write_type = Some(write_type);
    write_type
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
    if declaration.is_some_and(|decl| {
        matches!(
            program.arena().kind(decl),
            Kind::GetAccessor | Kind::SetAccessor
        )
    }) {
        return get_type_of_accessors(checker, program, symbol, globals);
    }
    // An assignment-declaration symbol (`f.x = v` / `this.x = v`, a binary
    // expression value declaration) takes its type from the assigned value(s)
    // (Go's `getTypeOfVariableOrParameterOrPropertyWorker` KindBinaryExpression
    // arm -> `getWidenedTypeForAssignmentDeclaration`), not a type annotation.
    if declaration.is_some_and(|decl| program.arena().kind(decl) == Kind::BinaryExpression) {
        let t = get_widened_type_for_assignment_declaration(checker, program, symbol);
        checker.value_symbol_links.get(symbol).resolved_type = Some(t);
        return t;
    }
    let type_node = declaration.and_then(|decl| match program.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node,
        NodeData::PropertySignature(d) | NodeData::PropertyDeclaration(d) => d.type_node,
        NodeData::GetAccessorDeclaration(d) => d.type_node,
        // A setter's declared type is its value-parameter annotation (Go's
        // `getAnnotatedAccessorType` / `getEffectiveSetAccessorTypeAnnotationNode`).
        NodeData::SetAccessorDeclaration(d) => d.parameters.nodes.first().and_then(|&param| {
            match program.arena().data(param) {
                NodeData::ParameterDeclaration(p) => p.type_node,
                _ => None,
            }
        }),
        // A parameter symbol's type comes from its annotation (Go's
        // `getTypeOfParameter` -> `getTypeOfSymbol` -> annotation), so a
        // signature's parameter types resolve for call checking (4q).
        NodeData::ParameterDeclaration(d) => d.type_node,
        _ => None,
    });
    // Optionality (`| undefined`) is injected for an optional property/parameter
    // (`{ a?: number }`/`function f(x?: number)`) under strictNullChecks. Go's
    // `getTypeForVariableLikeDeclaration` applies `addOptionalityEx(t, isProperty,
    // includeOptionality && isOptionalDeclaration(decl))`; this worker is the
    // `includeOptionality=true` path.
    // Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration
    let is_property = declaration.is_some_and(|decl| {
        matches!(
            program.arena().kind(decl),
            Kind::PropertySignature | Kind::PropertyDeclaration
        )
    });
    let is_optional = declaration.is_some_and(|decl| is_optional_declaration(program, decl));
    let t = if let Some(node) = type_node {
        let declared = get_type_from_type_node(checker, program, node, globals);
        checker.add_optionality_ex(declared, is_property, is_optional)
    } else if let Some(parameter) =
        declaration.filter(|&decl| program.arena().kind(decl) == Kind::Parameter)
    {
        // An un-annotated parameter takes its type from the contextual signature
        // of the containing arrow/function expression (Go's
        // `getTypeForVariableLikeDeclaration` parameter branch ->
        // `getContextuallyTypedParameterType`), falling back to `any` when there
        // is no contextual type.
        //
        // DEFER(phase-4-checker-4bk+): the `this`-parameter contextual type, the
        // default-initializer inference for an un-contextual parameter, and the
        // `addOptionalityEx` optional widening. blocked-by: `this`-typing +
        // parameter initializer inference + optional-parameter widening.
        checker
            .get_contextually_typed_parameter_type(program, parameter)
            .unwrap_or_else(|| checker.any_type())
    } else if let Some((decl, initializer)) = declaration
        .and_then(|decl| variable_declaration_initializer(program, decl).map(|i| (decl, i)))
    {
        // Un-annotated variable with an initializer: the declared type is the
        // initializer's type, widened in a mutable (`let`/`var`) binding (Go's
        // `getTypeForVariableLikeDeclaration` ->
        // `widenTypeInferredFromInitializer` -> `getWidenedLiteralTypeForInitializer`).
        //
        // DEFER(phase-4-checker-later): the object-literal/widening-type pass of
        // `getWidenedType` (only `getWidenedLiteralType` is applied here, which
        // is a no-op for non-literal types), parameter/property initializer
        // inference, binding patterns, and the circular-initializer resolution
        // stack. blocked-by: object-literal widening + binding-element typing +
        // `pushTypeResolution`.
        if program.arena().kind(initializer) == Kind::ObjectLiteralExpression
            && checker.object_literals_resolving.contains(&initializer)
            && !checker.accessors_type_resolving.is_empty()
        {
            // An accessor inside the object literal references the outer binding
            // (`const o = { get x() { return o.x; } }`) while its read type is
            // still being inferred — mark the cycle for TS7024 instead of
            // recursing through `check_object_literal` again.
            checker.accessor_type_resolution_cyclic = true;
            checker.error_type()
        } else {
            let initializer_type = checker.check_expression(program, initializer);
            get_widened_literal_type_for_initializer(checker, program, decl, initializer_type)
        }
    } else {
        // No annotation and nothing to infer from (Go falls back to `any`).
        checker.any_type()
    };
    // Widen the declaration's type (Go's `widenTypeForVariableLikeDeclaration` ->
    // `getWidenedType`). This strips object-literal freshness, so an object
    // literal assigned to a variable no longer triggers excess-property checks
    // when read back through the variable. It is identity for annotation types
    // and non-literal types.
    // Go: internal/checker/checker.go:Checker.widenTypeForVariableLikeDeclaration(18101)
    let t = checker.get_widened_type(t);
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

// Computes the widened type of an expando / `this`-property assignment
// declaration symbol from its assigned value(s) (the reachable subset of Go's
// `getWidenedTypeForAssignmentDeclaration`): the union of the widened (mutable-
// location) types of every `F.x = <rhs>` / `this.x = <rhs>` right-hand side,
// widened, defaulting to `any` when nothing is assignable.
//
// A self-referential assignment (`this.x = f(this.x)`) is broken by a resolving
// guard that returns `any` on re-entry (Go uses `pushTypeResolution` +
// `containsSameNamedThisProperty`).
//
// DEFER(phase-4): the constructor-`this` CFA typing
// (`isConstructorDeclaredThisProperty` / `getFlowTypeInConstructor`), the
// type-annotated binary expression (`f.x = v as T`), the empty-array / all-
// nullable implicit-any reports (TS7008 `Member '{0}' implicitly has an
// '{1}' type` / TS7022), and the `containsSameNamedThisProperty` self-ref
// filtering. blocked-by: constructor CFA + the implicit-any reporting surface +
// `noImplicitAny` gating.
// Go: internal/checker/checker.go:Checker.getWidenedTypeForAssignmentDeclaration
fn get_widened_type_for_assignment_declaration(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> TypeId {
    // Re-entry guard: a self-referential assignment yields `any` rather than
    // recursing forever.
    if !checker.assignment_declaration_resolving.insert(symbol) {
        return checker.any_type();
    }
    let declarations = program.symbol(symbol).declarations.clone();
    let mut types: Vec<TypeId> = Vec::new();
    for decl in declarations {
        let rhs = match program.arena().data(decl) {
            NodeData::BinaryExpression(d) => Some(d.right),
            _ => None,
        };
        if let Some(rhs) = rhs {
            // An empty-array RHS (`f.x = []` / `this.x = []`) widens to `any[]`
            // in Go (`getAssignmentDeclarationInitializerType` empty-array
            // branch), not the strict `never[]` an empty array literal types to.
            // We contribute `any` here (deferring the precise `any[]` shape + the
            // TS7008 implicit-any report) so a later `this.x.push(v)` does not
            // produce a spurious TS2345 against a `never` element type.
            let contributed = if is_empty_array_literal_rhs(program, rhs) {
                checker.any_type()
            } else {
                // `checkExpressionForMutableLocation` widens a fresh literal to
                // its base primitive (the target is a mutable property).
                let assigned = checker.check_expression(program, rhs);
                checker.get_widened_literal_type(assigned)
            };
            if !types.contains(&contributed) {
                types.push(contributed);
            }
        }
    }
    let t = if types.is_empty() {
        checker.any_type()
    } else {
        checker.get_union_type(&types)
    };
    let t = checker.get_widened_type(t);
    checker.assignment_declaration_resolving.remove(&symbol);
    t
}

// Reports whether `node` (after unwrapping parentheses) is an empty array
// literal (`[]`), the assigned value Go's
// `getAssignmentDeclarationInitializerType` widens to `any[]`.
fn is_empty_array_literal_rhs(program: &dyn BoundProgram, node: NodeId) -> bool {
    let mut n = node;
    while let NodeData::ParenthesizedExpression(d) = program.arena().data(n) {
        n = d.expression;
    }
    matches!(
        program.arena().data(n),
        NodeData::ArrayLiteralExpression(d) if d.list.nodes.is_empty()
    )
}

// Returns the initializer node of a variable or class-property declaration, if
// it has one (the reachable subset of Go's variable-like initializer handling).
// An un-annotated class field `x = 1` infers its (widened) initializer type
// `number`, so `this.x` reads `number` (the C-D2 this-type headline).
//
// DEFER(phase-4-checker-later): parameter initializer inference and binding
// patterns. blocked-by: parameter contextual/default inference + binding-element
// typing.
fn variable_declaration_initializer(program: &dyn BoundProgram, decl: NodeId) -> Option<NodeId> {
    match program.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer,
        NodeData::PropertyDeclaration(d) => d.initializer,
        _ => None,
    }
}

// Keeps a literal initializer's type for a `const`/`using`/readonly binding, or
// widens its fresh literal to the base primitive for a mutable (`let`/`var`)
// binding (Go's `getWidenedLiteralTypeForInitializer`). Const-ness is read from
// the declaration's combined node flags (Go's `getCombinedNodeFlagsCached &
// NodeFlagsConstant`).
//
// DEFER(phase-4-checker-later): `isDeclarationReadonly` (readonly property /
// parameter properties). blocked-by: readonly modifier resolution on
// properties.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralTypeForInitializer(16756)
fn get_widened_literal_type_for_initializer(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declaration: NodeId,
    t: TypeId,
) -> TypeId {
    if combined_node_flags(program, declaration).intersects(NodeFlags::CONSTANT) {
        return t;
    }
    checker.get_widened_literal_type(t)
}

// Computes the combined node flags of a variable declaration (Go's
// `getCombinedNodeFlags`): a declaration's `let`/`const`/`using` bits live on
// the enclosing `VariableDeclarationList`, with ambient/export bits on the
// `VariableStatement`, so the flags are OR-folded up that chain.
// Go: internal/ast/utilities.go:getCombinedNodeFlags / getCombinedFlags
pub(crate) fn combined_node_flags(program: &dyn BoundProgram, node: NodeId) -> NodeFlags {
    let arena = program.arena();
    let mut combined = arena.flags(node);
    if let Some(list) = arena.parent(node) {
        if arena.kind(list) == Kind::VariableDeclarationList {
            combined |= arena.flags(list);
            if let Some(statement) = arena.parent(list) {
                combined |= arena.flags(statement);
            }
        }
    }
    combined
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
        Kind::ThisType => checker.get_this_type_from_node(program, node),
        Kind::TypeReference => get_type_from_type_reference(checker, program, node, globals),
        Kind::ArrayType => get_type_from_array_type_node(checker, program, node, globals),
        Kind::TupleType => get_type_from_tuple_type_node(checker, program, node, globals),
        Kind::TypeLiteral => get_type_from_type_literal_node(checker, program, node),
        Kind::FunctionType | Kind::ConstructorType => {
            get_type_from_function_or_constructor_type_node(checker, program, node)
        }
        Kind::TypeOperator => get_type_from_type_operator_node(checker, program, node, globals),
        Kind::IndexedAccessType => {
            get_type_from_indexed_access_type_node(checker, program, node, globals)
        }
        Kind::ConditionalType => {
            get_type_from_conditional_type_node(checker, program, node, globals)
        }
        Kind::TemplateLiteralType => {
            get_type_from_template_type_node(checker, program, node, globals)
        }
        Kind::MappedType => get_type_from_mapped_type_node(checker, program, node),
        Kind::InferType => get_type_from_infer_type_node(checker, program, node),
        Kind::UnionType => get_type_from_union_type_node(checker, program, node, globals),
        Kind::IntersectionType => {
            get_type_from_intersection_type_node(checker, program, node, globals)
        }
        Kind::LiteralType => get_type_from_literal_type_node(checker, program, node),
        // A parenthesized type `(T)` has the type of its inner type node (Go's
        // `getTypeFromTypeNode` `KindParenthesizedType` arm, which recurses on
        // `node.Type()`). This makes e.g. `(number | string)[]` resolve to
        // `Array<number | string>` instead of `Array<error>`.
        Kind::ParenthesizedType => {
            let inner = match program.arena().data(node) {
                NodeData::ParenthesizedType(d) => d.type_node,
                _ => return checker.error_type(),
            };
            get_type_from_type_node(checker, program, inner, globals)
        }
        // DEFER(phase-4-checker-4d): remaining type-node kinds.
        // blocked-by: their type constructors land across 4d+.
        _ => checker.error_type(),
    }
}

// Resolves a literal type node. A `LiteralType` wrapping a `NullKeyword`
// resolves to `null_type`; the string/number/boolean literal type nodes
// (`"a"`/`1`/`true` in type position) route through `checkExpression` +
// `getRegularTypeOfLiteralType` (a literal in type position carries the
// *regular*, not fresh, literal type).
//
// DEFER(phase-4-checker-later): the negative-numeric (`-1`) and bigint literal
// type nodes (a `PrefixUnaryExpression`/`BigIntLiteral` operand, not yet typed
// by `check_expression`) and the `links.resolvedType` memoization Go keeps.
// blocked-by: prefix-unary/bigint literal expression typing.
// Go: internal/checker/checker.go:Checker.getTypeFromLiteralTypeNode(22781)
fn get_type_from_literal_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
) -> TypeId {
    let literal = match program.arena().data(node) {
        NodeData::LiteralType(d) => d.literal,
        _ => return checker.error_type(),
    };
    if program.arena().kind(literal) == Kind::NullKeyword {
        return checker.null_type();
    }
    let t = checker.check_expression(program, literal);
    checker.regular_type_of_literal_type(t)
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

// Resolves an `IndexedAccessType` node (`T[K]`) to the indexed-access type: each
// operand node is resolved, then combined via `get_indexed_access_type`. A
// generic operand yields a deferred indexed-access type; concrete operands
// resolve to the selected property/element/index-signature value type. When
// nothing resolves (a missing property on a concrete object), Go's
// `getIndexedAccessTypeEx` with an access node yields the error type.
//
// DEFER(phase-4-checker-later): the `2536`/`2537` index diagnostics, the type
// alias attribution (`getAliasForTypeNode`), and the `links.resolvedType`
// memoization. blocked-by: the access-node error machinery + alias attribution +
// type-node memoization.
// Go: internal/checker/checker.go:Checker.getTypeFromIndexedAccessTypeNode
fn get_type_from_indexed_access_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let (object_node, index_node) = match program.arena().data(node) {
        NodeData::IndexedAccessType(d) => (d.object_type, d.index_type),
        _ => return checker.error_type(),
    };
    let object_type = get_type_from_type_node(checker, program, object_node, globals);
    let index_type = get_type_from_type_node(checker, program, index_node, globals);
    get_indexed_access_type(checker, program, object_type, index_type)
        .unwrap_or(checker.error_type())
}

// Resolves a `ConditionalType` node (`T extends U ? X : Y`) to its conditional
// type, memoizing the result on the node (Go's per-node
// `typeNodeLinks.resolvedType`). A concrete check type resolves a branch
// eagerly; a generic (type-parameter / instantiable) check type yields a
// deferred conditional type that re-resolves on instantiation.
//
// DEFER(phase-4-checker-C-C3): the alias attribution (`getAliasForTypeNode`) and
// the simple-tuple deferral (`[X] extends [Y]`). blocked-by: type-alias
// attribution wiring + generic tuples.
// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode
fn get_type_from_conditional_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    if let Some(cached) = checker.conditional_node_type(node) {
        return cached;
    }
    let (check_type_node, extends_type_node) = match program.arena().data(node) {
        NodeData::ConditionalType(d) => (d.check_type, d.extends_type),
        _ => return checker.error_type(),
    };
    let check_type = get_type_from_type_node(checker, program, check_type_node, globals);
    let extends_type = get_type_from_type_node(checker, program, extends_type_node, globals);
    // A conditional whose check type is a naked type parameter distributes over
    // a union check type (Go: `checkType.flags&TypeFlagsTypeParameter != 0`).
    let is_distributive = is_distributive_conditional_type(checker, check_type);
    let infer_type_parameters = get_infer_type_parameters(checker, program, node);
    let outer_type_parameters = get_outer_type_parameters(checker, program, node);
    let root = ConditionalRoot {
        node,
        check_type,
        extends_type,
        is_distributive,
        infer_type_parameters,
        outer_type_parameters,
    };
    let resolved = get_conditional_type(checker, program, &root, None);
    checker.set_conditional_node_type(node, resolved);
    resolved
}

// Resolves an `infer R` type node to its (fresh, symbol-interned) declared type
// parameter (Go's `getTypeFromInferTypeNode`).
// Go: internal/checker/checker.go:Checker.getTypeFromInferTypeNode
fn get_type_from_infer_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> TypeId {
    let tp_decl = match program.arena().data(node) {
        NodeData::InferType(d) => d.type_parameter,
        _ => return checker.error_type(),
    };
    match program.symbol_of_node(tp_decl) {
        Some(sym) => get_declared_type_of_type_parameter(checker, sym),
        None => checker.error_type(),
    }
}

// Resolves a `TemplateLiteralType` node (`` `a${T}b` ``) to its template literal
// type: read the head + each span's placeholder type and trailing literal text,
// then combine via [`get_template_literal_type`]. Concrete placeholders fold
// into a string literal; a union placeholder distributes; a generic placeholder
// yields a deferred template literal type (Go's `getTypeFromTemplateTypeNode`).
// Go: internal/checker/checker.go:Checker.getTypeFromTemplateTypeNode
fn get_type_from_template_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
    globals: Option<&SymbolTable>,
) -> TypeId {
    let (head, spans) = match program.arena().data(node) {
        NodeData::TemplateLiteralType(d) => (d.head, d.template_spans.nodes.clone()),
        _ => return checker.error_type(),
    };
    let mut texts: Vec<String> = Vec::with_capacity(spans.len() + 1);
    texts.push(program.arena().text(head).to_string());
    let mut types: Vec<TypeId> = Vec::with_capacity(spans.len());
    for span in spans {
        let (type_node, literal) = match program.arena().data(span) {
            NodeData::TemplateLiteralTypeSpan(d) => (d.expression, d.literal),
            _ => return checker.error_type(),
        };
        let span_type = get_type_from_type_node(checker, program, type_node, globals);
        types.push(span_type);
        texts.push(program.arena().text(literal).to_string());
    }
    get_template_literal_type(checker, &texts, &types)
}

// Resolves a `MappedType` node (`{ [K in C]: V }`) to a mapped-type object
// (Go's `getTypeFromMappedTypeNode`). An un-instantiated mapped type carries
// only the declaration node (no mapper); its members are produced when the type
// is instantiated over a concrete modifiers type (see [`instantiate_mapped_type`]).
//
// DEFER(phase-4-checker-C-C3): the eager constraint-type resolution Go performs
// here to surface a self-referential-constraint error. blocked-by: the
// circular-constraint diagnostic path.
// Go: internal/checker/checker.go:Checker.getTypeFromMappedTypeNode
fn get_type_from_mapped_type_node(
    checker: &mut Checker,
    _program: &dyn BoundProgram,
    node: NodeId,
) -> TypeId {
    checker.new_mapped_type(node, None)
}

/// Instantiates a mapped-type object `t` (`{ [K in C]: V }`) under `mapper`.
///
/// For a homomorphic mapped type (constraint `keyof T`) instantiated over a
/// concrete object, eagerly resolves to an anonymous object carrying one
/// property per key of the (instantiated) modifiers type — each with the
/// template type `V` (with `K` bound to that key) and the `+`/`-` `readonly`/`?`
/// modifiers applied. A mapped type whose modifiers type is still generic is
/// kept as a deferred instantiated mapped type.
///
/// DEFER(phase-4-checker-later): the non-keyof (`Record`-style) constraint, the
/// homomorphic distribution over a union/array/tuple modifiers type, the `as`
/// name-type filtering (`as never` key removal), the reverse-mapped inference,
/// and the lazy member resolution Go keeps (`resolveMappedTypeMembers`). The
/// port resolves members eagerly at instantiation, which is observably
/// equivalent for the reachable concrete-object subset. blocked-by: those
/// machinery pieces + generic-mapped relation/inference.
///
/// # Examples
/// ```
/// use tsgo_checker::{instantiate_mapped_type, BoundProgram, Checker, TypeId, TypeMapper};
/// fn demo<P: BoundProgram>(c: &mut Checker, p: &P, mapped: TypeId, m: &TypeMapper) {
///     let _ = instantiate_mapped_type(c, p, mapped, m);
/// }
/// ```
///
/// Side effects: may allocate property symbols and an anonymous object type.
// Go: internal/checker/checker.go:Checker.instantiateMappedType / resolveMappedTypeMembers
pub fn instantiate_mapped_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
    mapper: &TypeMapper,
) -> TypeId {
    let Some(declaration) = checker.mapped_type_declaration(t) else {
        return t;
    };
    let combined = match checker.mapped_type_mapper(t) {
        Some(own) => TypeMapper::merge(own, mapper.clone()),
        None => mapper.clone(),
    };
    // The reachable subset only resolves homomorphic `{ [K in keyof T]: V }`
    // mapped types; a non-keyof (`Record`-style) constraint stays deferred.
    if !is_mapped_type_with_keyof_constraint(program, declaration) {
        return checker.new_mapped_type(declaration, Some(combined));
    }
    let modifiers_type = super::mapped_types::modifiers_type_from_declaration(
        checker,
        program,
        declaration,
        Some(&combined),
    );
    // A still-generic modifiers type keeps the mapped type deferred (Go's
    // `instantiateMappedType` returns the instantiated mapped type when the
    // homomorphic type variable is unchanged).
    let modifiers_flags = checker.get_type(modifiers_type).flags();
    if modifiers_flags.intersects(TypeFlags::INSTANTIABLE)
        || is_generic_object_type(checker, modifiers_type)
    {
        return checker.new_mapped_type(declaration, Some(combined));
    }
    resolve_mapped_type_members_eager(checker, program, declaration, modifiers_type, &combined)
}

// Eagerly resolves a homomorphic mapped type's members over a concrete
// `modifiers_type`, producing an anonymous object type. Mirrors the keyof
// branch of Go's `resolveMappedTypeMembers` (with `getTypeOfMappedSymbol`
// resolution inlined): one property per modifiers-type key, typed by the
// template `V` with `K -> key`, carrying the `+`/`-` `readonly`/`?` modifiers.
// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers / getTypeOfMappedSymbol
fn resolve_mapped_type_members_eager(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declaration: NodeId,
    modifiers_type: TypeId,
    combined: &TypeMapper,
) -> TypeId {
    let template_modifiers = get_mapped_type_modifiers(program, declaration);
    let type_parameter = get_type_parameter_from_mapped_type(checker, program, declaration);
    let template_node = match program.arena().data(declaration) {
        NodeData::MappedType(d) => d.type_node,
        _ => None,
    };
    let name_type_node = match program.arena().data(declaration) {
        NodeData::MappedType(d) => d.name_type,
        _ => None,
    };
    let globals = program.globals().cloned();
    let props = get_properties_of_type(checker, modifiers_type);
    let mut members = SymbolTable::default();
    let mut properties: Vec<SymbolId> = Vec::with_capacity(props.len());
    for (name, modifiers_prop) in props {
        let key_type = checker.get_string_literal_type(&name);
        // The property name: `as N` remaps the key through the name type
        // (`K -> key`), then prints it as the new property name; without `as`
        // the key itself is the name.
        let prop_name = match name_type_node {
            Some(nt) => {
                let name_mapper =
                    TypeMapper::append_mapping(Some(combined.clone()), type_parameter, key_type);
                let base = get_type_from_type_node(checker, program, nt, globals.as_ref());
                let remapped = checker.instantiate_type(base, &name_mapper);
                match property_name_from_remapped_type(checker, remapped) {
                    Some(n) => n,
                    // A non-string-literal remapped key (e.g. `as never`) drops
                    // the property. DEFER: full `as` filtering.
                    None => continue,
                }
            }
            None => name.clone(),
        };
        let prop_optional = checker
            .resolved_symbol_flags(program, modifiers_prop)
            .contains(SymbolFlags::OPTIONAL);
        let prop_readonly = is_readonly_source_symbol(checker, modifiers_prop);
        let is_optional = template_modifiers.contains(MappedTypeModifiers::INCLUDE_OPTIONAL)
            || (!template_modifiers.contains(MappedTypeModifiers::EXCLUDE_OPTIONAL)
                && prop_optional);
        let is_readonly = template_modifiers.contains(MappedTypeModifiers::INCLUDE_READONLY)
            || (!template_modifiers.contains(MappedTypeModifiers::EXCLUDE_READONLY)
                && prop_readonly);
        let prop_type = match template_node {
            Some(tn) => {
                let template_mapper =
                    TypeMapper::append_mapping(Some(combined.clone()), type_parameter, key_type);
                let base = get_type_from_type_node(checker, program, tn, globals.as_ref());
                checker.instantiate_type(base, &template_mapper)
            }
            None => checker.error_type(),
        };
        // `-?` (ExcludeOptional) on a source optional property strips the
        // `| undefined` that the optional source contributes to `T[K]` under
        // strictNullChecks (Go's `CheckFlagsStripOptional` ->
        // `getTypeWithFacts(propType, NEUndefined)` in `getTypeOfMappedSymbol`).
        // Without this, `Required<{ a?: number }>` would read `a` as
        // `number | undefined` (C-D1 now injects the optional `| undefined`).
        // Go: internal/checker/checker.go:resolveMappedTypeMembers (stripOptional)
        let strip_optional = checker.strict_null_checks() && !is_optional && prop_optional;
        let prop_type = if strip_optional {
            checker.get_type_with_facts(prop_type, super::type_facts::TypeFacts::NE_UNDEFINED)
        } else {
            prop_type
        };
        let mut flags = SymbolFlags::PROPERTY;
        if is_optional {
            flags |= SymbolFlags::OPTIONAL;
        }
        let check_flags = if is_readonly {
            CheckFlags::READONLY
        } else {
            CheckFlags::empty()
        };
        let prop = checker.new_object_literal_property(&prop_name, flags, check_flags, prop_type);
        members.insert(prop_name.clone(), prop);
        properties.push(prop);
    }
    let object = ObjectType {
        members,
        properties,
        ..Default::default()
    };
    checker.new_object_type(ObjectFlags::ANONYMOUS, None, object)
}

// Returns the property name a remapped `as` key type denotes, for the reachable
// subset (a string literal). A non-string-literal (e.g. `never`) yields `None`,
// which drops the property.
fn property_name_from_remapped_type(checker: &Checker, t: TypeId) -> Option<String> {
    match checker.get_type(t).literal_value() {
        Some(LiteralValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

// Reports whether a source (modifiers-type) property is `readonly`, for the
// reachable subset (a synthesized property carrying the `Readonly` check flag).
//
// DEFER(phase-4-checker-later): a program (interface/type-literal) member's
// `readonly` modifier. blocked-by: declaration-modifier `readonly` reading on
// bound property symbols. The reachable mapped-type slices have no readonly
// source properties, so this only affects `{ readonly a }` *sources*, not the
// mapped type's own `readonly` modifier (which is handled separately).
// Go: internal/checker/checker.go:Checker.isReadonlySymbol
fn is_readonly_source_symbol(checker: &Checker, symbol: SymbolId) -> bool {
    if super::is_synthesized_symbol(symbol) {
        return checker
            .synthesized_symbol_check_flags(symbol)
            .contains(CheckFlags::READONLY);
    }
    false
}

// Returns the mapped type's declared type parameter `K` (Go's
// `getTypeParameterFromMappedType`).
// Go: internal/checker/checker.go:Checker.getTypeParameterFromMappedType
fn get_type_parameter_from_mapped_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declaration: NodeId,
) -> TypeId {
    let tp_decl = match program.arena().data(declaration) {
        NodeData::MappedType(d) => d.type_parameter,
        _ => return checker.error_type(),
    };
    match program.symbol_of_node(tp_decl) {
        Some(sym) => get_declared_type_of_type_parameter(checker, sym),
        None => checker.error_type(),
    }
}

// Returns the `+`/`-` `readonly`/`?` modifiers declared on a mapped type (Go's
// `getMappedTypeModifiers`): a `-` token strips, a `readonly`/`?`/`+` token adds.
// Go: internal/checker/checker.go:getMappedTypeModifiers
fn get_mapped_type_modifiers(
    program: &dyn BoundProgram,
    declaration: NodeId,
) -> MappedTypeModifiers {
    let (readonly_token, question_token) = match program.arena().data(declaration) {
        NodeData::MappedType(d) => (d.readonly_token, d.question_token),
        _ => return MappedTypeModifiers::NONE,
    };
    let mut modifiers = MappedTypeModifiers::NONE;
    if let Some(rt) = readonly_token {
        modifiers |= if program.arena().kind(rt) == Kind::MinusToken {
            MappedTypeModifiers::EXCLUDE_READONLY
        } else {
            MappedTypeModifiers::INCLUDE_READONLY
        };
    }
    if let Some(qt) = question_token {
        modifiers |= if program.arena().kind(qt) == Kind::MinusToken {
            MappedTypeModifiers::EXCLUDE_OPTIONAL
        } else {
            MappedTypeModifiers::INCLUDE_OPTIONAL
        };
    }
    modifiers
}

// Reports whether a mapped type's constraint declaration is a `keyof T` node
// (Go's `isMappedTypeWithKeyofConstraintDeclaration`), i.e. the mapped type is
// homomorphic.
// Go: internal/checker/checker.go:Checker.isMappedTypeWithKeyofConstraintDeclaration
fn is_mapped_type_with_keyof_constraint(program: &dyn BoundProgram, declaration: NodeId) -> bool {
    match constraint_declaration_for_mapped_type(program, declaration) {
        Some(constraint) => matches!(
            program.arena().data(constraint),
            NodeData::TypeOperator(d) if d.operator == Kind::KeyOfKeyword
        ),
        None => false,
    }
}

// Returns the `in C` constraint node of a mapped type's type parameter (Go's
// `getConstraintDeclarationForMappedType`).
// Go: internal/checker/checker.go:Checker.getConstraintDeclarationForMappedType
fn constraint_declaration_for_mapped_type(
    program: &dyn BoundProgram,
    declaration: NodeId,
) -> Option<NodeId> {
    let tp_decl = match program.arena().data(declaration) {
        NodeData::MappedType(d) => d.type_parameter,
        _ => return None,
    };
    match program.arena().data(tp_decl) {
        NodeData::TypeParameterDeclaration(d) => d.constraint,
        _ => None,
    }
}

pub use super::conditional_types::{
    get_conditional_type, get_string_mapping_type, get_template_literal_type,
};

// Collects the `infer R` type parameters declared in a conditional type's
// `extends` clause (Go's `getInferTypeParameters`).
// Go: internal/checker/checker.go:Checker.getInferTypeParameters
fn get_infer_type_parameters(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> Vec<TypeId> {
    let extends_type_node = match program.arena().data(node) {
        NodeData::ConditionalType(d) => d.extends_type,
        _ => return Vec::new(),
    };
    let mut decls = Vec::new();
    collect_infer_type_parameter_declarations(program, extends_type_node, &mut decls);
    let mut result = Vec::new();
    for decl in decls {
        if let Some(sym) = program.symbol_of_node(decl) {
            let t = get_declared_type_of_type_parameter(checker, sym);
            if !result.contains(&t) {
                result.push(t);
            }
        }
    }
    result
}

// Walks `node`'s subtree in source order, collecting the type-parameter
// declaration node of each `infer R` it contains. Does not descend into a
// nested conditional type (whose `infer`s belong to that conditional's scope).
fn collect_infer_type_parameter_declarations(
    program: &dyn BoundProgram,
    node: NodeId,
    out: &mut Vec<NodeId>,
) {
    match program.arena().kind(node) {
        Kind::InferType => {
            if let NodeData::InferType(d) = program.arena().data(node) {
                out.push(d.type_parameter);
            }
        }
        // A nested conditional type opens its own `infer` scope.
        Kind::ConditionalType => {}
        _ => {
            program.arena().for_each_child(node, &mut |child| {
                collect_infer_type_parameter_declarations(program, child, out);
                false
            });
        }
    }
}

// Collects the enclosing (outer) type parameters in scope at a conditional type
// node: the type parameters of each enclosing generic container, plus an
// enclosing conditional's `infer` parameters (Go's `getOuterTypeParameters`,
// reachable subset without the `this`-type, context-sensitive-signature, and
// mapped-type arms and without the `isTypeParameterPossiblyReferenced` filter —
// an unreferenced outer parameter only widens the instantiation cache key).
// Go: internal/checker/checker.go:Checker.getOuterTypeParameters
fn get_outer_type_parameters(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> Vec<TypeId> {
    let mut current = program.arena().parent(node);
    while let Some(n) = current {
        let kind = program.arena().kind(n);
        if is_generic_container_kind(kind) {
            let mut outer = get_outer_type_parameters(checker, program, n);
            if kind == Kind::ConditionalType {
                outer.extend(get_infer_type_parameters(checker, program, n));
                return outer;
            }
            append_own_type_parameters(checker, program, n, &mut outer);
            return outer;
        }
        current = program.arena().parent(n);
    }
    Vec::new()
}

// Reports whether `kind` is a generic-parameter-introducing container (Go's
// `getOuterTypeParameters` switch arms).
fn is_generic_container_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::ClassDeclaration
            | Kind::ClassExpression
            | Kind::InterfaceDeclaration
            | Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::MethodSignature
            | Kind::FunctionType
            | Kind::ConstructorType
            | Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::TypeAliasDeclaration
            | Kind::MappedType
            | Kind::ConditionalType
    )
}

// Appends `container`'s own declared type parameters (by symbol) to `out`,
// skipping duplicates.
fn append_own_type_parameters(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    container: NodeId,
    out: &mut Vec<TypeId>,
) {
    if let Some(list) = type_parameter_list_of(program, container) {
        for tp in list.nodes {
            if let Some(sym) = program.symbol_of_node(tp) {
                let t = get_declared_type_of_type_parameter(checker, sym);
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
    }
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
        // `keyof T` resolves to the index type of the operand.
        Kind::KeyOfKeyword => {
            let operand_type = get_type_from_type_node(checker, program, operand, globals);
            get_index_type(checker, operand_type)
        }
        Kind::UniqueKeyword => {
            if program.arena().kind(operand) == Kind::SymbolKeyword {
                let parent = program.arena().parent(node).unwrap_or(node);
                get_es_symbol_like_type_for_node(
                    checker,
                    program,
                    walk_up_parenthesized_types(program.arena(), parent),
                )
            } else {
                checker.error_type()
            }
        }
        _ => checker.error_type(),
    }
}

// Walks up `ParenthesizedType` wrappers (Go's `WalkUpParenthesizedTypes`).
fn walk_up_parenthesized_types(arena: &NodeArena, mut node: NodeId) -> NodeId {
    while arena.kind(node) == Kind::ParenthesizedType {
        node = arena.parent(node).expect("parenthesized type has parent");
    }
    node
}

// Reports whether `node` is a declaration that may carry a `unique symbol` type
// (Go's `isValidESSymbolDeclaration`).
// Go: internal/checker/utilities.go:isValidESSymbolDeclaration(923)
fn is_valid_es_symbol_declaration(program: &dyn BoundProgram, node: NodeId) -> bool {
    let arena = program.arena();
    match arena.kind(node) {
        Kind::VariableDeclaration => {
            combined_node_flags(program, node).intersects(NodeFlags::CONSTANT)
                && matches!(
                    arena.data(node),
                    NodeData::VariableDeclaration(d) if arena.kind(d.name) == Kind::Identifier
                )
                && arena.parent(node).is_some_and(|list| {
                    arena.kind(list) == Kind::VariableDeclarationList
                        && arena.parent(list).is_some_and(|stmt| {
                            arena.kind(stmt) == Kind::VariableStatement
                        })
                })
        }
        Kind::PropertyDeclaration => {
            node_has_modifier(arena, node, Kind::ReadonlyKeyword)
                && node_has_modifier(arena, node, Kind::StaticKeyword)
        }
        Kind::PropertySignature => node_has_modifier(arena, node, Kind::ReadonlyKeyword),
        _ => false,
    }
}

fn node_has_modifier(arena: &NodeArena, node: NodeId, modifier: Kind) -> bool {
    grammar::modifier_nodes_pub(arena, node)
        .into_iter()
        .any(|m| arena.kind(m) == modifier)
}

// Resolves `unique symbol` for a valid declaration, else the intrinsic `symbol`
// type (Go's `getESSymbolLikeTypeForNode`).
// Go: internal/checker/checker.go:Checker.getESSymbolLikeTypeForNode(22841)
pub(crate) fn get_es_symbol_like_type_for_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> TypeId {
    if !is_valid_es_symbol_declaration(program, node) {
        return checker.es_symbol_type();
    }
    let Some(symbol) = get_symbol_of_declaration(program, node) else {
        return checker.es_symbol_type();
    };
    if let Some(&cached) = checker.unique_es_symbol_types.get(&symbol) {
        return cached;
    }
    let sym = program.symbol(symbol);
    let name = format!(
        "{INTERNAL_SYMBOL_NAME_PREFIX}@{}@{}",
        sym.name, symbol.0
    );
    let unique_type = checker.new_unique_es_symbol_type(symbol, &name);
    checker
        .unique_es_symbol_types
        .insert(symbol, unique_type);
    unique_type
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

// Resolves a function/constructor type node (`(x: number) => void`) to an
// anonymous object type carrying its call signatures (Go's
// `getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode` ->
// `resolveAnonymousTypeMembers`, where the call signatures come from the
// `__call` member the binder placed on the node's `__type` symbol). The
// signatures are resolved eagerly here (4q's pattern), so a contextual function
// type yields a call signature whose parameter types contextually type an
// assigned arrow/function expression's parameters (4bj).
//
// DEFER(phase-4-checker-C-A+): generic type parameters, the `this` parameter,
// and the per-node `links.resolvedType` memoization Go keeps. blocked-by:
// generic signatures + `this`-typing + type-node memoization. (Construct
// signatures — the `__new` member of a `ConstructorType` — land in C-A.)
// Go: internal/checker/checker.go:Checker.getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode
fn get_type_from_function_or_constructor_type_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
) -> TypeId {
    let Some(symbol) = program.symbol_of_node(node) else {
        return checker.error_type();
    };
    // The `__type` symbol's `__call`/`__new` members declare the call/construct
    // signatures; their declarations are the function/constructor-type node.
    let call_signatures = signatures_of_member(checker, program, symbol, INTERNAL_SYMBOL_NAME_CALL);
    let construct_signatures =
        signatures_of_member(checker, program, symbol, INTERNAL_SYMBOL_NAME_NEW);
    let object = ObjectType {
        call_signatures,
        construct_signatures,
        ..Default::default()
    };
    checker.new_object_type(ObjectFlags::ANONYMOUS, Some(symbol), object)
}

// Resolves the signatures declared by a synthetic signature member (`__call` /
// `__new`) of an anonymous type-node symbol, or an empty list when absent.
// Go: internal/checker/checker.go:Checker.getSignaturesOfSymbol (member subset)
fn signatures_of_member(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    member_name: &str,
) -> Vec<SignatureId> {
    match program.symbol(symbol).members.get(member_name) {
        Some(&member) => get_signatures_of_symbol(checker, program, member),
        None => Vec::new(),
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
    let type_arguments = match program.arena().data(node) {
        NodeData::TypeReference(d) => d.type_arguments.clone(),
        _ => None,
    };
    // A qualified name (`N.T`) resolves the member type `T` of namespace `N`
    // through the entity-name resolver (Go's `getTypeFromTypeReference` ->
    // `resolveEntityName` for a non-identifier type name), so `const t: N.T` is
    // typed as the exported alias/interface/class of `N`.
    let name = if program.arena().kind(type_name) == Kind::Identifier {
        program.arena().text(type_name).to_string()
    } else {
        // The printed name of the rightmost identifier, used for the
        // intrinsic/string-mapping name dispatch below.
        let right = qualified_name_right_text(program, type_name);
        right.unwrap_or_default()
    };
    let symbol = if program.arena().kind(type_name) == Kind::Identifier {
        match resolve_name(program, node, &name, SymbolFlags::TYPE, false, globals) {
            Some(symbol) => symbol,
            // The binder does not place a generic declaration's type parameters
            // in its `locals`, so a bare `T` reference inside a generic
            // interface/class/method/function body is resolved by scanning the
            // enclosing type-parameter lists for a matching name (Go's
            // `resolveName` finds these in `locals`). This lets a `value: T`
            // member of `Iterator<T>` carry the type parameter (4ah), which is
            // then instantiated through the enclosing reference's type-argument
            // mapper.
            None => match resolve_type_parameter_in_scope(program, node, &name) {
                Some(tp) => tp,
                None => return checker.error_type(),
            },
        }
    } else {
        match resolve_entity_name(program, type_name, SymbolFlags::TYPE, globals) {
            Some(symbol) => symbol,
            None => return checker.error_type(),
        }
    };
    let symbol_flags = program.symbol(symbol).flags;
    let target = get_declared_type_of_symbol(checker, program, symbol, globals);
    let provided_args: Vec<TypeId> = type_arguments
        .as_ref()
        .map(|list| {
            list.nodes
                .iter()
                .map(|&arg| get_type_from_type_node(checker, program, arg, globals))
                .collect()
        })
        .unwrap_or_default();
    // A generic class/interface reference fills any missing local type arguments
    // from their defaults (or `unknown`), then forms a type reference — so `C`
    // with `interface C<T = number>` resolves to `C<number>`, and reading `c.v`
    // instantiates `T -> number` (Go's `getTypeFromClassOrInterfaceReference`
    // -> `fillMissingTypeArguments` -> `createTypeReference`). The arity/2314
    // diagnostic itself is emitted separately by `check_type_reference_node`.
    if symbol_flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
        let type_parameters = checker
            .get_type(target)
            .as_object()
            .map(|o| o.type_parameters.clone())
            .unwrap_or_default();
        if !type_parameters.is_empty() {
            let filled =
                fill_missing_type_arguments(checker, program, &provided_args, &type_parameters);
            return checker.create_type_reference(target, filled);
        }
    }
    // A generic type alias reference (`IsString<string>`) instantiates the
    // alias's declared type: map the alias's type parameters to the supplied
    // arguments (filling defaults/`unknown` for any missing trailing slots) and
    // instantiate. This is what re-resolves a deferred conditional/indexed type
    // in the alias body — `type IsString<T> = T extends string ? "yes" : "no"`
    // applied to `string` resolves to `"yes"` (Go's `getTypeFromTypeAliasReference`
    // -> `getTypeAliasInstantiation`). The arity/2315 diagnostic is emitted
    // separately by `check_type_reference_node`.
    if symbol_flags.contains(SymbolFlags::TYPE_ALIAS) {
        // Intrinsic string-mapping aliases (`Uppercase`/`Lowercase`/`Capitalize`/
        // `Uncapitalize`): Go declares these as `type Uppercase<S> = intrinsic`
        // and dispatches in `getTypeAliasInstantiation` when the alias's declared
        // type is the intrinsic marker and it has one type argument. The port has
        // no `intrinsic` keyword (parser is out of scope), so it keys on the
        // alias *name* (a same-named user alias is also intercepted; documented
        // divergence).
        if provided_args.len() == 1 {
            if let Some(kind) = StringMappingKind::from_name(&name) {
                return get_string_mapping_type(checker, kind, provided_args[0]);
            }
        }
        let type_parameters = checker
            .type_alias_links
            .try_get(&symbol)
            .map(|l| l.type_parameters.clone())
            .unwrap_or_default();
        if !type_parameters.is_empty() {
            let filled =
                fill_missing_type_arguments(checker, program, &provided_args, &type_parameters);
            let mapper = TypeMapper::new(&type_parameters, &filled);
            return checker.instantiate_type(target, &mapper);
        }
    }
    // `Foo<A, B>` (a non-generic-target or alias) with explicit arguments forms a
    // plain reference; a bare name stays the target.
    if !provided_args.is_empty() {
        return checker.create_type_reference(target, provided_args);
    }
    target
}

// Resolves an entity name (an `Identifier` or a `QualifiedName`) to the symbol
// matching `meaning` (Go's `resolveEntityName`). An identifier resolves by name
// in scope; a qualified name `N.x` resolves `N` as a namespace then looks the
// rightmost name up in its exports. Used by qualified-name type references
// (`N.T`).
//
// DEFER(phase-4-checker-C-D2+): alias-chain resolution (`resolveAlias`),
// CommonJS `require`/`export =` re-resolution, the not-found diagnostics
// (2694 `Namespace_0_has_no_exported_member_1` etc.), and `PropertyAccess`
// entity names. blocked-by: alias targets + import-equals + the suggestion
// machinery.
// Go: internal/checker/checker.go:Checker.resolveEntityName
pub(crate) fn resolve_entity_name(
    program: &dyn BoundProgram,
    name_node: tsgo_ast::NodeId,
    meaning: SymbolFlags,
    globals: Option<&SymbolTable>,
) -> Option<SymbolId> {
    match program.arena().kind(name_node) {
        Kind::Identifier => {
            let text = program.arena().text(name_node).to_string();
            resolve_name(program, name_node, &text, meaning, false, globals)
        }
        Kind::QualifiedName => {
            let (left, right) = match program.arena().data(name_node) {
                NodeData::QualifiedName(d) => (d.left, d.right),
                _ => return None,
            };
            resolve_qualified_name(program, left, right, meaning, globals)
        }
        _ => None,
    }
}

// Resolves a qualified name `left.right` (Go's `resolveQualifiedName`): the
// `left` entity is resolved as a namespace, then `right` is looked up in that
// namespace's exports filtered by `meaning`.
// Go: internal/checker/checker.go:Checker.resolveQualifiedName
fn resolve_qualified_name(
    program: &dyn BoundProgram,
    left: tsgo_ast::NodeId,
    right: tsgo_ast::NodeId,
    meaning: SymbolFlags,
    globals: Option<&SymbolTable>,
) -> Option<SymbolId> {
    let namespace = resolve_entity_name(program, left, SymbolFlags::NAMESPACE, globals)?;
    let text = program.arena().text(right).to_string();
    get_symbol_from_table(program, &program.symbol(namespace).exports, &text, meaning)
}

// Looks a name up in a symbol table filtered by `meaning` (Go's `getSymbol`):
// returns the symbol only if it carries one of the requested meaning flags.
// Go: internal/checker/checker.go:Checker.getSymbol
fn get_symbol_from_table(
    program: &dyn BoundProgram,
    table: &SymbolTable,
    name: &str,
    meaning: SymbolFlags,
) -> Option<SymbolId> {
    let symbol = *table.get(name)?;
    if program.symbol(symbol).flags.intersects(meaning) {
        Some(symbol)
    } else {
        None
    }
}

// Returns the printed text of the rightmost identifier of a qualified name
// (`A.B.c` -> `"c"`), used for the intrinsic-name dispatch in
// `get_type_from_type_reference`.
fn qualified_name_right_text(
    program: &dyn BoundProgram,
    name_node: tsgo_ast::NodeId,
) -> Option<String> {
    match program.arena().data(name_node) {
        NodeData::QualifiedName(d) => Some(program.arena().text(d.right).to_string()),
        _ => None,
    }
}

// Resolves a bare type-reference name to an enclosing generic declaration's
// type parameter by walking the parent chain and scanning each generic
// container's `<...>` list for a matching name. Mirrors the type-parameter
// reachability Go gets for free from binding type parameters into a
// declaration's `locals`.
//
// An enclosing conditional type also contributes its `infer R` parameters
// (declared in its `extends` clause): a `R` reference in a conditional branch
// resolves to that inferred type parameter. The Rust binder scopes an `infer`
// parameter anonymously (`bindAnonymousDeclaration`) instead of into the
// conditional node's locals, so this lookup recovers the Go scoping.
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
        if arena.kind(n) == Kind::ConditionalType {
            if let Some(sym) = find_infer_type_parameter_in_scope(program, n, name) {
                return Some(sym);
            }
        }
        current = arena.parent(n);
    }
    None
}

// Finds an `infer R` type parameter named `name` declared in a conditional
// type's `extends` clause, returning its symbol (so a `R` reference in a branch
// resolves to it).
fn find_infer_type_parameter_in_scope(
    program: &dyn BoundProgram,
    conditional_node: tsgo_ast::NodeId,
    name: &str,
) -> Option<SymbolId> {
    let extends_node = match program.arena().data(conditional_node) {
        NodeData::ConditionalType(d) => d.extends_type,
        _ => return None,
    };
    let mut decls = Vec::new();
    collect_infer_type_parameter_declarations(program, extends_node, &mut decls);
    for decl in decls {
        if let NodeData::TypeParameterDeclaration(d) = program.arena().data(decl) {
            if program.arena().kind(d.name) == Kind::Identifier
                && program.arena().text(d.name) == name
            {
                return program.symbol_of_node(decl);
            }
        }
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

/// Strips call/construct signatures from an object or intersection type.
///
/// Used when comparing class static sides: the static base type is compared
/// without its constructor signatures (Go's `getTypeWithoutSignatures`).
// Go: internal/checker/checker.go:Checker.getTypeWithoutSignatures(4493)
pub fn get_type_without_signatures(checker: &mut Checker, t: TypeId) -> TypeId {
    if let TypeData::Intersection(i) = &checker.get_type(t).data {
        let members = i.types.clone();
        let types: Vec<TypeId> = members
            .iter()
            .map(|&ty| get_type_without_signatures(checker, ty))
            .collect();
        return checker.get_intersection_type(&types);
    }
    if let Some(obj) = checker.get_type(t).as_object() {
        if !obj.call_signatures.is_empty() || !obj.construct_signatures.is_empty() {
            let stripped = ObjectType {
                members: obj.members.clone(),
                properties: obj.properties.clone(),
                index_infos: obj.index_infos.clone(),
                ..Default::default()
            };
            let sym = checker.get_type(t).symbol;
            return checker.new_object_type(
                ObjectFlags::ANONYMOUS | ObjectFlags::MEMBERS_RESOLVED,
                sym,
                stripped,
            );
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
pub(crate) fn get_applicable_index_info(
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

/// Returns every index signature on `t` applicable to `key_type`.
///
/// Side effects: may instantiate index infos for generic references.
// Go: internal/checker/checker.go:Checker.getApplicableIndexInfos(31751)
pub(crate) fn get_applicable_index_infos(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
    key_type: TypeId,
) -> Vec<IndexInfoId> {
    get_index_infos_of_type(checker, t)
        .into_iter()
        .filter(|&id| {
            let info = checker.index_info(id);
            is_applicable_index_type(checker, program, key_type, info.key_type)
        })
        .collect()
}

// Returns the index signature of `t` applicable to a property named `name`,
// mapping the name to its string-literal key type and deferring to
// `get_applicable_index_info`. Used by the excess-property check to treat a
// property covered by an index signature as a known property.
//
// DEFER(phase-4-checker-4bg+): the late-bound-name arm keys the lookup by
// `esSymbolType` instead of the name's string-literal type.
// blocked-by: late binding (`isLateBoundName`) + the global ES symbol type.
// Go: internal/checker/checker.go:Checker.getApplicableIndexInfoForName
pub(crate) fn get_applicable_index_info_for_name(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
    name: &str,
) -> Option<IndexInfoId> {
    let key = checker.get_string_literal_type(name);
    get_applicable_index_info(checker, program, t, key)
}

/// Returns the index signature of `t` keyed exactly by `key_type`.
///
/// Side effects: may instantiate index infos for generic references.
// Go: internal/checker/checker.go:Checker.getIndexInfoOfType
pub fn get_index_info_of_type(
    checker: &mut Checker,
    t: TypeId,
    key_type: TypeId,
) -> Option<IndexInfoId> {
    let index_infos = get_index_infos_of_type(checker, t);
    find_index_info(checker, &index_infos, key_type)
}

/// Returns the value type of the index signature of `t` keyed by `key_type`.
///
/// Side effects: may instantiate index infos for generic references.
// Go: internal/checker/checker.go:Checker.getIndexTypeOfType
pub fn get_index_type_of_type(
    checker: &mut Checker,
    t: TypeId,
    key_type: TypeId,
) -> Option<TypeId> {
    get_index_info_of_type(checker, t, key_type).map(|id| checker.index_info(id).value_type)
}

// Go: internal/checker/checker.go:findIndexInfo
fn find_index_info(
    checker: &Checker,
    index_infos: &[IndexInfoId],
    key_type: TypeId,
) -> Option<IndexInfoId> {
    index_infos
        .iter()
        .copied()
        .find(|&id| checker.index_info(id).key_type == key_type)
}

/// Returns the index type `keyof t` (Go's `getIndexType`).
///
/// For a concrete object type this is the union of its property-name literal
/// types plus any string/number index-signature key types; for a generic /
/// instantiable type (a type parameter, an indexed access, a conditional, ...)
/// it is a deferred [`TypeData::Index`] type that recomputes `keyof` once the
/// target is instantiated. A union distributes to an intersection of its
/// constituents' index types; an intersection distributes to a union.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_index_type, Checker, TypeFlags};
/// let mut c = Checker::new();
/// // `keyof T` for a type parameter is a deferred index type.
/// let tp = c.new_type_parameter(None);
/// let k = get_index_type(&mut c, tp);
/// assert!(c.get_type(k).flags().contains(TypeFlags::INDEX));
/// ```
///
/// Side effects: may allocate a union/index type and populate the index cache.
// Go: internal/checker/checker.go:Checker.getIndexType / getIndexTypeEx
pub fn get_index_type(checker: &mut Checker, t: TypeId) -> TypeId {
    get_index_type_ex(checker, t, IndexFlags::NONE)
}

// Go: internal/checker/checker.go:Checker.getIndexTypeEx
fn get_index_type_ex(checker: &mut Checker, t: TypeId, index_flags: IndexFlags) -> TypeId {
    // Go calls `getReducedType(t)` first; that is the identity for the reachable
    // subset (no never-reducible intersections constructed here).
    if should_defer_index_type(checker, t, index_flags) {
        return get_index_type_for_generic_type(checker, t, index_flags);
    }
    let flags = checker.get_type(t).flags();
    // `keyof (A | B)` == `keyof A & keyof B`.
    if flags.contains(TypeFlags::UNION) {
        let members = checker.get_type(t).union_types().unwrap_or(&[]).to_vec();
        let mapped: Vec<TypeId> = members
            .iter()
            .map(|&m| get_index_type_ex(checker, m, index_flags))
            .collect();
        return checker.get_intersection_type(&mapped);
    }
    // `keyof (A & B)` == `keyof A | keyof B`.
    if flags.contains(TypeFlags::INTERSECTION) {
        let members = checker
            .get_type(t)
            .intersection_types()
            .unwrap_or(&[])
            .to_vec();
        let mapped: Vec<TypeId> = members
            .iter()
            .map(|&m| get_index_type_ex(checker, m, index_flags))
            .collect();
        return checker.get_union_type(&mapped);
    }
    if flags.contains(TypeFlags::UNKNOWN) {
        return checker.never_type();
    }
    // DEFER(phase-4-checker-C-C3): the `keyof any`/`keyof never` arm
    // (`stringNumberSymbolType`), the wildcard arm, and the mapped-type arm
    // (`getIndexTypeForMappedType`). blocked-by: the global `string | number |
    // symbol` type, the wildcard type, and mapped types (C-C3).
    let include = TypeFlags::STRING_LIKE | TypeFlags::NUMBER_LIKE | TypeFlags::ES_SYMBOL_LIKE;
    get_literal_type_from_properties(checker, t, include)
}

// Reports whether `keyof t` should be deferred to a generic [`TypeData::Index`]
// type rather than resolved to a union of keys.
//
// DEFER(phase-4-checker-C-C3): the generic-tuple, generic-mapped, reducible-
// union, and instantiable-intersection arms. blocked-by: generic tuples,
// mapped types (C-C3), and union/intersection reducibility machinery.
// Go: internal/checker/checker.go:Checker.shouldDeferIndexType
fn should_defer_index_type(checker: &Checker, t: TypeId, _index_flags: IndexFlags) -> bool {
    checker
        .get_type(t)
        .flags()
        .intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
}

// Returns (and interns) the deferred `keyof t` index type for a generic target.
// Go: internal/checker/checker.go:Checker.getIndexTypeForGenericType
fn get_index_type_for_generic_type(
    checker: &mut Checker,
    t: TypeId,
    index_flags: IndexFlags,
) -> TypeId {
    if let Some(&cached) = checker.index_types.get(&t) {
        return cached;
    }
    let index = checker.new_index_type(t, index_flags & IndexFlags::STRINGS_ONLY);
    checker.index_types.insert(t, index);
    index
}

// Builds the union of a structured type's property-name literal types plus its
// included index-signature key types (Go's `getLiteralTypeFromProperties`).
//
// DEFER(phase-4-checker-C-C3): the origin-index alias, the `includeOrigin`
// caching key, and the enum-number index-info exclusion. blocked-by: index-type
// origins + the properties-types cache + enum number indexing.
// Go: internal/checker/checker.go:Checker.getLiteralTypeFromProperties
fn get_literal_type_from_properties(
    checker: &mut Checker,
    t: TypeId,
    include: TypeFlags,
) -> TypeId {
    let props = get_properties_of_type(checker, t);
    let infos = get_index_infos_of_type(checker, t);
    let mut types: Vec<TypeId> = Vec::with_capacity(props.len() + infos.len());
    for (name, _prop) in props {
        types.push(get_literal_type_from_property(checker, &name, include));
    }
    let string_type = checker.string_type();
    for info in infos {
        let key_type = checker.index_info(info).key_type;
        if is_key_type_included(checker, key_type, include) {
            if key_type == string_type && include.contains(TypeFlags::NUMBER) {
                types.push(checker.string_or_number_type());
            } else {
                types.push(key_type);
            }
        }
    }
    checker.get_union_type(&types)
}

// Returns the property-name literal type for `name` when its kind is in
// `include`, else `never` (Go's `getLiteralTypeFromProperty`).
//
// DEFER(phase-4-checker-C-C3): the `valueSymbolLinks.nameType` path (computed /
// late-bound names), the numeric-literal property name (`{ 0: T }` -> `0`), the
// `default` export name, and the non-public-accessibility filter. blocked-by:
// computed/late-bound name types, numeric-literal property names, and
// accessibility-modifier plumbing on synthesized members.
// Go: internal/checker/checker.go:Checker.getLiteralTypeFromProperty
fn get_literal_type_from_property(checker: &mut Checker, name: &str, include: TypeFlags) -> TypeId {
    let t = checker.get_string_literal_type(name);
    if checker.get_type(t).flags().intersects(include) {
        t
    } else {
        checker.never_type()
    }
}

// Reports whether an index-signature key type contributes to a `keyof` result
// under `include` (Go's `isKeyTypeIncluded`).
// Go: internal/checker/checker.go:Checker.isKeyTypeIncluded
fn is_key_type_included(checker: &Checker, key_type: TypeId, include: TypeFlags) -> bool {
    let flags = checker.get_type(key_type).flags();
    if flags.intersects(include) {
        return true;
    }
    if flags.contains(TypeFlags::INTERSECTION) {
        let members = checker
            .get_type(key_type)
            .intersection_types()
            .unwrap_or(&[])
            .to_vec();
        return members
            .iter()
            .any(|&m| is_key_type_included(checker, m, include));
    }
    false
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
    // A generic object and/or index defers to an interned `IndexedAccess` type
    // (a higher-order access whose members cannot be resolved yet). An
    // `any`/`unknown` object short-circuits to itself.
    if should_defer_indexed_access_type(checker, object_type, index_type) {
        if checker
            .get_type(object_type)
            .flags()
            .intersects(TypeFlags::ANY | TypeFlags::UNKNOWN)
        {
            return Some(object_type);
        }
        return Some(get_or_create_indexed_access_type(
            checker,
            object_type,
            index_type,
        ));
    }
    // A (non-boolean) union index distributes: `T["a" | "b"]` == `T["a"] |
    // T["b"]`. A missing constituent property aborts to `None` (Go's
    // `accessNode == nil` early return).
    if checker
        .get_type(index_type)
        .flags()
        .contains(TypeFlags::UNION)
        && index_type != checker.boolean_type()
    {
        let members = checker
            .get_type(index_type)
            .union_types()
            .unwrap_or(&[])
            .to_vec();
        let mut prop_types: Vec<TypeId> = Vec::with_capacity(members.len());
        for m in members {
            match checker.get_property_type_for_index_type(program, object_type, m) {
                Some(pt) => prop_types.push(pt),
                None => return None,
            }
        }
        return Some(checker.get_union_type(&prop_types));
    }
    checker.get_property_type_for_index_type(program, object_type, index_type)
}

// Resolves `objectType[indexType]` to the type of the property/element selected
// by `indexType` on the (apparent) object type — a named property when the index
// is a string/number literal, a tuple element by a literal/number index, or the
// value type of an applicable index signature. Returns `None` when nothing
// applies (the caller turns that into a diagnostic or the error type).
//
// DEFER(phase-4-checker-later): the `accessNode`-driven error reporting
// (`2536`/`2538`/`7053`), the `Contextual`/`Writing`/`IncludeUndefined` access
// flags, numeric-literal property names on a non-tuple object, and the
// constraint-of-index-type fallback for a residual generic object. blocked-by:
// the access-node error machinery + access flags + numeric property names.
// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType
pub(crate) fn get_property_type_for_index_type(
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
    // A string/number-literal index names a property: select it by name (Go's
    // `getPropertyTypeForIndexType` `hasPropName` branch). This is what resolves
    // a type-node access `T["a"]` and each constituent of a distributed union
    // index to the property type.
    if let Some(name) = get_property_name_from_index(checker, index_type) {
        if let Some(t) = get_type_of_property_of_type(checker, program, apparent, &name) {
            return Some(t);
        }
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

// Reports whether `objectType[indexType]` should be deferred to an
// `IndexedAccess` type (a higher-order access over generic operands).
//
// DEFER(phase-4-checker-later): the `accessNode`-kind distinction (an
// element-access *expression* eagerly resolves a generic object indexed by a
// non-generic key via its constraint, whereas a type node defers) and the
// generic-tuple fixed-index eager resolution. The unified form here matches the
// type-node path; the element-access cases reachable in the ported tests have
// either concrete operands or a generic index, so both produce the Go result.
// blocked-by: threading the access-node kind + generic tuples + constraint-based
// eager resolution.
// Go: internal/checker/checker.go:Checker.shouldDeferIndexedAccessType
fn should_defer_indexed_access_type(
    checker: &Checker,
    object_type: TypeId,
    index_type: TypeId,
) -> bool {
    is_generic_index_type(checker, index_type) || is_generic_object_type(checker, object_type)
}

// Reports whether `t` is a generic (instantiable) index type — i.e. usable as a
// deferred `keyof`/index value (Go's `isGenericIndexType`, reachable subset).
//
// DEFER(phase-4-checker-C-C3): `isGenericStringLikeType` (template-literal /
// string-mapping types) and the substitution-type arm. blocked-by: template
// literal + string mapping + substitution types (C-C2/C-C3).
// Go: internal/checker/checker.go:Checker.isGenericIndexType / getGenericObjectFlags
fn is_generic_index_type(checker: &Checker, t: TypeId) -> bool {
    let flags = checker.get_type(t).flags();
    if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
        let members = union_or_intersection_members(checker, t);
        return members.iter().any(|&m| is_generic_index_type(checker, m));
    }
    flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE | TypeFlags::INDEX)
}

// Reports whether `t` is a generic (instantiable) object type (Go's
// `isGenericObjectType`, reachable subset).
//
// DEFER(phase-4-checker-C-C3): generic mapped and generic tuple types.
// blocked-by: mapped types (C-C3) + generic tuples.
// Go: internal/checker/checker.go:Checker.isGenericObjectType / getGenericObjectFlags
pub(crate) fn is_generic_object_type(checker: &Checker, t: TypeId) -> bool {
    let flags = checker.get_type(t).flags();
    if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
        let members = union_or_intersection_members(checker, t);
        return members.iter().any(|&m| is_generic_object_type(checker, m));
    }
    flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
}

// Returns a union's or intersection's constituent type ids.
fn union_or_intersection_members(checker: &Checker, t: TypeId) -> Vec<TypeId> {
    let ty = checker.get_type(t);
    if let Some(members) = ty.union_types() {
        members.to_vec()
    } else if let Some(members) = ty.intersection_types() {
        members.to_vec()
    } else {
        Vec::new()
    }
}

// Returns (and interns) the deferred `objectType[indexType]` indexed-access type.
// Go: internal/checker/checker.go:Checker.getIndexedAccessTypeOrUndefined (defer branch)
fn get_or_create_indexed_access_type(
    checker: &mut Checker,
    object_type: TypeId,
    index_type: TypeId,
) -> TypeId {
    let key = (object_type, index_type);
    if let Some(&cached) = checker.indexed_access_types.get(&key) {
        return cached;
    }
    // The reachable subset carries no persistent access flags.
    let t = checker.new_indexed_access_type(object_type, index_type, AccessFlags::NONE);
    checker.indexed_access_types.insert(key, t);
    t
}

// Returns the property name a string/number-literal index selects (Go's
// `getPropertyNameFromIndex` / `getPropertyNameFromType` for the literal cases).
//
// Resolves a string/number/`unique symbol` literal index to a property name.
//
// DEFER(phase-4-checker-later): the `accessNode`-driven computed/private name
// cases. blocked-by: access-node plumbing.
// Go: internal/checker/checker.go:Checker.getPropertyNameFromIndex
fn get_property_name_from_index(checker: &Checker, index_type: TypeId) -> Option<String> {
    if super::check::is_type_usable_as_property_name(checker.get_type(index_type).flags()) {
        return Some(super::late_binding::get_property_name_from_type(
            checker, index_type,
        ));
    }
    None
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
    // Go uses `getNumberLiteralType` here (createTupleTargetType), so the
    // tuple's `length` literal is interned by value like every other `N`.
    Some(checker.get_number_literal_type(tsgo_jsnum::Number::from(arity as f64)))
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

/// Resolves `name` on a union or intersection type.
///
/// Side effects: may cache a synthesized property symbol.
// Go: internal/checker/checker.go:Checker.getPropertyOfUnionOrIntersectionType
pub(crate) fn get_property_of_union_or_intersection_type(
    checker: &Checker,
    t: TypeId,
    name: &str,
) -> Option<SymbolId> {
    if checker.get_type(t).intersection_types().is_some() {
        return get_intersection_property(checker, t, name);
    }
    if checker.get_type(t).union_types().is_some() {
        return get_union_property(checker, t, name);
    }
    None
}

// Resolves `name` on an intersection type. A name found in exactly one
// constituent returns that constituent's own symbol (Go's `singleProp` return);
// a name found in two or more constituents mints a synthesized property whose
// type is the intersection of the per-constituent types.
// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty (intersection)
pub(crate) fn get_intersection_property(
    checker: &Checker,
    t: TypeId,
    name: &str,
) -> Option<SymbolId> {
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
pub(crate) fn get_union_property(checker: &Checker, t: TypeId, name: &str) -> Option<SymbolId> {
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
/// included), filtering out reserved-name members (`__index` / `__call` /
/// `__new`) the binder stores for signatures (Go's `getNamedMembers`). For an
/// intersection it unions every constituent's properties (Go's
/// `getPropertiesOfUnionOrIntersectionType`), keeping the first constituent that
/// declares each name.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_properties_of_type, Checker};
/// let c = Checker::new();
/// assert!(get_properties_of_type(&c, c.string_type()).is_empty());
/// ```
///
/// Side effects: none (pure read over the type arena).
// Go: internal/checker/checker.go:Checker.getPropertiesOfType / getNamedMembers(21907)
pub fn get_properties_of_type(checker: &mut Checker, t: TypeId) -> Vec<(String, SymbolId)> {
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
    let program = checker.retained_program();
    resolve_structured_type_members(checker, program.as_deref(), apparent)
        .into_iter()
        .filter(|(name, _)| !is_reserved_member_name(name))
        .collect()
}

// Reports whether `name` is a reserved internal member name the binder uses for
// signatures (`__index` / `__call` / `__new`, etc.), which `getNamedMembers`
// excludes from a type's property list. Well-known-symbol (`__@iterator`) and
// private (`__#x`) names are NOT reserved — they are real named members.
//
// The Rust port stores the internal prefix as the char `\u{FE}` (Go uses the
// raw byte `\xFE`); the rule is "prefix followed by a character other than
// `@` or `#`".
// Go: internal/checker/utilities.go:isReservedMemberName(1584)
fn is_reserved_member_name(name: &str) -> bool {
    let mut chars = name.chars();
    if chars.next() != Some('\u{FE}') {
        return false;
    }
    matches!(chars.next(), Some(second) if second != '@' && second != '#')
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
