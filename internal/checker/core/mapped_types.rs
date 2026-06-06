//! Mapped-type resolution and generic-instantiation symbol typing.
//!
//! Ports Go's `getTypeFromMappedTypeNode`, `getModifiersTypeFromMappedType`,
//! `getTemplateTypeFromMappedType`, `getConstraintTypeFromMappedType`,
//! `resolveMappedTypeMembers`, `getTypeOfMappedSymbol`, and
//! `getTypeOfInstantiatedSymbol`.

use tsgo_ast::{CheckFlags, Kind, NodeData, SymbolFlags, SymbolId, SymbolTable};

use super::declared_types::{
    get_constraint_of_type_parameter, get_declared_type_of_type_parameter, get_properties_of_type,
    get_type_from_type_node, get_type_of_symbol,
};
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::type_facts::TypeFacts;
use super::types::{MappedTypeModifiers, ObjectFlags, TypeFlags, TypeId};
use super::Checker;

/// Resolves a `MappedType` AST node to a mapped-type object.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_mapped_type, Checker};
/// use tsgo_ast::NodeId;
/// fn demo<P: tsgo_checker::BoundProgram>(c: &mut Checker, p: &P, node: NodeId) {
///     let _ = get_mapped_type(c, p, node);
/// }
/// ```
///
/// Side effects: allocates a mapped-type object.
// Go: internal/checker/checker.go:Checker.getTypeFromMappedTypeNode
pub fn get_mapped_type(
    checker: &mut Checker,
    _program: &dyn BoundProgram,
    node: tsgo_ast::NodeId,
) -> TypeId {
    checker.new_mapped_type(node, None)
}

/// Returns the instantiated type of an `instantiateSymbol` property.
///
/// Side effects: may allocate and cache the instantiated type.
// Go: internal/checker/checker.go:Checker.getTypeOfInstantiatedSymbol
pub fn get_type_of_instantiated_symbol(
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
    let links = checker.value_symbol_links.get(symbol);
    let target = links.target.expect("instantiated symbol target");
    let mapper = links.mapper.clone().expect("instantiated symbol mapper");
    let base = get_type_of_symbol(checker, program, target, None);
    let t = checker.instantiate_type(base, &mapper);
    checker.value_symbol_links.get(symbol).resolved_type = Some(t);
    t
}

/// Returns the constraint type of mapped type `t`'s type parameter.
///
/// Side effects: may cache the constraint on `t`.
// Go: internal/checker/checker.go:Checker.getConstraintTypeFromMappedType
pub fn get_constraint_type_from_mapped_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
) -> TypeId {
    if let Some(cached) = checker
        .mapped_type_links
        .get(&t)
        .and_then(|l| l.constraint_type)
    {
        return cached;
    }
    let declaration = checker.mapped_type_declaration(t).expect("mapped type");
    let tp = type_parameter_from_declaration(checker, program, declaration);
    let constraint = get_constraint_of_type_parameter(checker, program, tp)
        .unwrap_or_else(|| checker.error_type());
    checker
        .mapped_type_links
        .entry(t)
        .or_default()
        .constraint_type = Some(constraint);
    constraint
}

/// Returns the template type `V` of mapped type `t`.
///
/// Side effects: may cache the template type on `t`.
// Go: internal/checker/checker.go:Checker.getTemplateTypeFromMappedType
pub fn get_template_type_from_mapped_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
) -> TypeId {
    if let Some(cached) = checker
        .mapped_type_links
        .get(&t)
        .and_then(|l| l.template_type)
    {
        return cached;
    }
    let declaration = checker.mapped_type_declaration(t).expect("mapped type");
    let template_node = match program.arena().data(declaration) {
        NodeData::MappedType(d) => d.type_node,
        _ => return checker.error_type(),
    };
    let globals = program.globals().cloned();
    let base = match template_node {
        Some(node) => get_type_from_type_node(checker, program, node, globals.as_ref()),
        None => checker.error_type(),
    };
    let include_optional =
        mapped_type_modifiers(program, declaration).contains(MappedTypeModifiers::INCLUDE_OPTIONAL);
    let with_optional = checker.add_optionality_ex(base, true, include_optional);
    let template = match checker.mapped_type_mapper(t) {
        Some(mapper) => checker.instantiate_type(with_optional, &mapper),
        None => with_optional,
    };
    checker
        .mapped_type_links
        .entry(t)
        .or_default()
        .template_type = Some(template);
    template
}

/// Returns the modifiers type `T` in `{ [K in keyof T]: V }`.
///
/// Side effects: may cache the modifiers type on `t`.
// Go: internal/checker/checker.go:Checker.getModifiersTypeFromMappedType
pub fn get_modifiers_type_from_mapped_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
) -> TypeId {
    if let Some(cached) = checker
        .mapped_type_links
        .get(&t)
        .and_then(|l| l.modifiers_type)
    {
        return cached;
    }
    let declaration = checker.mapped_type_declaration(t).expect("mapped type");
    let owned_mapper = checker.mapped_type_mapper(t);
    let modifiers =
        modifiers_type_from_declaration(checker, program, declaration, owned_mapper.as_ref());
    checker
        .mapped_type_links
        .entry(t)
        .or_default()
        .modifiers_type = Some(modifiers);
    modifiers
}

/// Lazily resolves mapped-type members when needed by
/// [`super::declared_types::resolve_structured_type_members`].
///
/// Side effects: mutates the mapped type's member table.
// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers
pub(crate) fn resolve_mapped_type_members_lazy(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    mapped_type: TypeId,
    declaration: tsgo_ast::NodeId,
) {
    if checker
        .get_type(mapped_type)
        .object_flags()
        .contains(ObjectFlags::MEMBERS_RESOLVED)
    {
        return;
    }
    if !is_keyof_mapped_type(program, declaration) {
        return;
    }
    let modifiers_type = get_modifiers_type_from_mapped_type(checker, program, mapped_type);
    let modifiers_flags = checker.get_type(modifiers_type).flags();
    if modifiers_flags.intersects(TypeFlags::INSTANTIABLE)
        || super::declared_types::is_generic_object_type(checker, modifiers_type)
    {
        return;
    }
    let template_modifiers = mapped_type_modifiers(program, declaration);
    let type_parameter = type_parameter_from_declaration(checker, program, declaration);
    let props = get_properties_of_type(checker, modifiers_type);
    let mut members = SymbolTable::default();
    let mut properties = Vec::with_capacity(props.len());
    for (name, modifiers_prop) in props {
        let key_type = checker.get_string_literal_type(&name);
        let prop_optional = checker
            .resolved_symbol_flags(program, modifiers_prop)
            .contains(SymbolFlags::OPTIONAL);
        let prop_readonly = is_readonly_source(checker, modifiers_prop);
        let is_optional = template_modifiers.contains(MappedTypeModifiers::INCLUDE_OPTIONAL)
            || (!template_modifiers.contains(MappedTypeModifiers::EXCLUDE_OPTIONAL)
                && prop_optional);
        let is_readonly = template_modifiers.contains(MappedTypeModifiers::INCLUDE_READONLY)
            || (!template_modifiers.contains(MappedTypeModifiers::EXCLUDE_READONLY)
                && prop_readonly);
        let mut flags = SymbolFlags::PROPERTY;
        if is_optional {
            flags |= SymbolFlags::OPTIONAL;
        }
        let mut check_flags = CheckFlags::MAPPED;
        if is_readonly {
            check_flags |= CheckFlags::READONLY;
        }
        if checker.strict_null_checks() && !is_optional && prop_optional {
            check_flags |= CheckFlags::STRIP_OPTIONAL;
        }
        let prop = checker.new_synthesized_property(&name, flags, check_flags, mapped_type);
        checker.value_symbol_links.get(prop).containing_type = Some(mapped_type);
        checker.mapped_symbol_links.get(prop).key_type = Some(key_type);
        members.insert(name.clone(), prop);
        properties.push(prop);
        let _ = type_parameter;
    }
    if let Some(obj) = checker.types.get_mut(mapped_type).as_object_mut() {
        obj.members = members;
        obj.properties = properties;
    }
    checker.types.get_mut(mapped_type).object_flags |= ObjectFlags::MEMBERS_RESOLVED;
    checker.structured_members_cache.remove(&mapped_type);
}

/// Returns the deferred property type of a mapped-type member symbol.
///
/// Side effects: may allocate and cache the property type.
// Go: internal/checker/checker.go:Checker.getTypeOfMappedSymbol
pub fn get_type_of_mapped_symbol(
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
    let containing = checker
        .value_symbol_links
        .get(symbol)
        .containing_type
        .expect("mapped symbol containing type");
    let key_type = checker
        .mapped_symbol_links
        .get(symbol)
        .key_type
        .expect("mapped symbol key type");
    let declaration = checker
        .mapped_type_declaration(containing)
        .expect("mapped type declaration");
    let type_parameter = type_parameter_from_declaration(checker, program, declaration);
    let template_type = get_template_type_from_mapped_type(checker, program, containing);
    let mapper = checker.mapped_type_mapper(containing);
    let template_mapper = TypeMapper::append_mapping(mapper, type_parameter, key_type);
    let mut prop_type = checker.instantiate_type(template_type, &template_mapper);
    let check_flags = checker.resolved_symbol_check_flags(program, symbol);
    if checker.strict_null_checks()
        && checker
            .resolved_symbol_flags(program, symbol)
            .contains(SymbolFlags::OPTIONAL)
        && !checker
            .get_type(prop_type)
            .flags()
            .intersects(TypeFlags::UNDEFINED | TypeFlags::VOID)
    {
        prop_type = checker.get_optional_type(prop_type, true);
    } else if check_flags.contains(CheckFlags::STRIP_OPTIONAL) {
        prop_type = checker.get_type_with_facts(prop_type, TypeFacts::NE_UNDEFINED);
    }
    checker.value_symbol_links.get(symbol).resolved_type = Some(prop_type);
    prop_type
}

/// Returns modifiers type `T` for declaration `declaration`, optionally mapped.
// Go: internal/checker/checker.go:Checker.getModifiersTypeFromMappedType
pub(crate) fn modifiers_type_from_declaration(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declaration: tsgo_ast::NodeId,
    mapper: Option<&TypeMapper>,
) -> TypeId {
    if !is_keyof_mapped_type(program, declaration) {
        return checker.unknown_type();
    }
    let Some(constraint) = constraint_declaration(program, declaration) else {
        return checker.unknown_type();
    };
    let operand = match program.arena().data(constraint) {
        NodeData::TypeOperator(d) => d.type_node,
        _ => return checker.unknown_type(),
    };
    let globals = program.globals().cloned();
    let base = get_type_from_type_node(checker, program, operand, globals.as_ref());
    match mapper {
        Some(m) => checker.instantiate_type(base, m),
        None => base,
    }
}

fn type_parameter_from_declaration(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    declaration: tsgo_ast::NodeId,
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

fn mapped_type_modifiers(
    program: &dyn BoundProgram,
    declaration: tsgo_ast::NodeId,
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

fn is_keyof_mapped_type(program: &dyn BoundProgram, declaration: tsgo_ast::NodeId) -> bool {
    match constraint_declaration(program, declaration) {
        Some(constraint) => matches!(
            program.arena().data(constraint),
            NodeData::TypeOperator(d) if d.operator == Kind::KeyOfKeyword
        ),
        None => false,
    }
}

fn constraint_declaration(
    program: &dyn BoundProgram,
    declaration: tsgo_ast::NodeId,
) -> Option<tsgo_ast::NodeId> {
    let tp_decl = match program.arena().data(declaration) {
        NodeData::MappedType(d) => d.type_parameter,
        _ => return None,
    };
    match program.arena().data(tp_decl) {
        NodeData::TypeParameterDeclaration(d) => d.constraint,
        _ => None,
    }
}

fn is_readonly_source(checker: &Checker, symbol: SymbolId) -> bool {
    if super::is_synthesized_symbol(symbol) {
        return checker
            .synthesized_symbol_check_flags(symbol)
            .contains(CheckFlags::READONLY);
    }
    false
}

#[cfg(test)]
#[path = "mapped_types_test.rs"]
mod tests;
