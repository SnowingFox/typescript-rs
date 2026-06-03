//! Late-binding infrastructure for computed property names.

use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_INDEX;
use tsgo_ast::{NodeData, NodeId, SymbolFlags, SymbolId};

use super::check::is_type_usable_as_property_name;
use super::declared_types;
use super::program::BoundProgram;
use super::types::{LiteralValue, TypeFlags, TypeId};
use super::Checker;

// Go: internal/checker/utilities.go:getPropertyNameFromType
#[allow(dead_code)]
pub(crate) fn get_property_name_from_type(checker: &Checker, t: TypeId) -> String {
    let ty = checker.get_type(t);
    let flags = ty.flags();
    if let Some(val) = ty.literal_value() {
        match val {
            LiteralValue::String(s) if flags.intersects(TypeFlags::STRING_LITERAL) => {
                return s.clone()
            }
            LiteralValue::Number(n) if flags.intersects(TypeFlags::NUMBER_LITERAL) => {
                return n.to_string()
            }
            _ => {}
        }
    }
    panic!("Unhandled case in get_property_name_from_type")
}

// Go: internal/checker/checker.go:isLateBindableAST
pub(crate) fn is_late_bindable_ast(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    let expr = match arena.data(node) {
        NodeData::ComputedPropertyName(d) => Some(d.expression),
        NodeData::ElementAccessExpression(d) => Some(d.argument_expression),
        _ => None,
    };
    expr.is_some_and(|e| is_entity_name_expression(arena, e))
}

fn is_entity_name_expression(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::Identifier(_) => true,
        NodeData::PropertyAccessExpression(d) => is_entity_name_expression(arena, d.expression),
        _ => false,
    }
}

// Go: internal/checker/checker.go:Checker.hasLateBindableName
#[allow(dead_code)]
pub(crate) fn has_late_bindable_name(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> bool {
    let Some(name) = super::symbols_query::name_of_declaration(program.arena(), node) else {
        return false;
    };
    is_late_bindable_name(checker, program, name)
}

// Go: internal/checker/checker.go:Checker.isLateBindableName
#[allow(dead_code)]
pub(crate) fn is_late_bindable_name(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> bool {
    if !is_late_bindable_ast(program.arena(), node) {
        return false;
    }
    match program.arena().data(node) {
        NodeData::ComputedPropertyName(_) => {
            let t = checker.check_computed_property_name(program, node);
            is_type_usable_as_property_name(checker.get_type(t).flags())
        }
        NodeData::ElementAccessExpression(d) => {
            let arg = d.argument_expression;
            let t = checker.check_expression(program, arg);
            is_type_usable_as_property_name(checker.get_type(t).flags())
        }
        _ => false,
    }
}

// Go: internal/checker/checker.go:Checker.hasLateBindableIndexSignature
#[allow(dead_code)]
pub(crate) fn has_late_bindable_index_signature(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> bool {
    let Some(name) = super::symbols_query::name_of_declaration(program.arena(), node) else {
        return false;
    };
    is_late_bindable_index_signature(checker, program, name)
}

// Go: internal/checker/checker.go:Checker.isLateBindableIndexSignature
#[allow(dead_code)]
pub(crate) fn is_late_bindable_index_signature(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> bool {
    if !is_late_bindable_ast(program.arena(), node) {
        return false;
    }
    let t = match program.arena().data(node) {
        NodeData::ComputedPropertyName(_) => checker.check_computed_property_name(program, node),
        NodeData::ElementAccessExpression(d) => {
            let arg = d.argument_expression;
            checker.check_expression(program, arg)
        }
        _ => return false,
    };
    let string = checker.string_type();
    let number = checker.number_type();
    let es_symbol = checker.es_symbol_type();
    let string_number_symbol = checker.get_union_type(&[string, number, es_symbol]);
    checker.is_type_assignable_to(program, t, string_number_symbol)
}

// Go: internal/checker/checker.go:getExcludedSymbolFlags
#[allow(dead_code)]
pub(crate) fn get_excluded_symbol_flags(flags: SymbolFlags) -> SymbolFlags {
    let mut result = SymbolFlags::empty();
    if flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
        result |= SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE) {
        result |= SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::PROPERTY) {
        result |= SymbolFlags::PROPERTY_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::ENUM_MEMBER) {
        result |= SymbolFlags::ENUM_MEMBER_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::FUNCTION) {
        result |= SymbolFlags::FUNCTION_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::CLASS) {
        result |= SymbolFlags::CLASS_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::INTERFACE) {
        result |= SymbolFlags::INTERFACE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::REGULAR_ENUM) {
        result |= SymbolFlags::REGULAR_ENUM_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::CONST_ENUM) {
        result |= SymbolFlags::CONST_ENUM_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::VALUE_MODULE) {
        result |= SymbolFlags::VALUE_MODULE_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::METHOD) {
        result |= SymbolFlags::METHOD_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::GET_ACCESSOR) {
        result |= SymbolFlags::GET_ACCESSOR_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::SET_ACCESSOR) {
        result |= SymbolFlags::SET_ACCESSOR_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
        result |= SymbolFlags::TYPE_PARAMETER_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::TYPE_ALIAS) {
        result |= SymbolFlags::TYPE_ALIAS_EXCLUDES;
    }
    if flags.intersects(SymbolFlags::ALIAS) {
        result |= SymbolFlags::ALIAS_EXCLUDES;
    }
    result
}

// Go: internal/checker/utilities.go:getMembersOfDeclaration
#[allow(dead_code)]
pub(crate) fn get_members_of_declaration(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::InterfaceDeclaration(d)
        | NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d) => d.members.nodes.clone(),
        NodeData::TypeLiteral(d) => d.members.nodes.clone(),
        NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
        _ => Vec::new(),
    }
}

// Go: internal/checker/checker.go:Checker.getIndexSymbol
#[allow(dead_code)]
pub(crate) fn get_index_symbol(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> Option<SymbolId> {
    let globals = program.globals();
    let dt = declared_types::get_declared_type_of_symbol(checker, program, symbol, globals);
    let members = declared_types::resolve_structured_type_members(checker, dt);
    members.get(INTERNAL_SYMBOL_NAME_INDEX).copied()
}

// Go: internal/checker/checker.go:Checker.getLateBoundSymbol
#[allow(dead_code)]
pub(crate) fn get_late_bound_symbol(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> SymbolId {
    let sym = program.symbol(symbol);
    if !sym.flags.intersects(SymbolFlags::CLASS_MEMBER)
        || sym.name != tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_COMPUTED
    {
        return symbol;
    }
    if let Some(late) = checker
        .late_bound_links
        .try_get(&symbol)
        .and_then(|l| l.late_symbol)
    {
        return late;
    }
    let has_bindable = sym
        .declarations
        .iter()
        .any(|&d| has_late_bindable_name(checker, program, d));
    if has_bindable {
        // DEFER(phase-4-checker): full late-binding with getMembersOfSymbol/getExportsOfSymbol.
    }
    let late = checker
        .late_bound_links
        .try_get(&symbol)
        .and_then(|l| l.late_symbol)
        .unwrap_or(symbol);
    checker.late_bound_links.get(symbol).late_symbol = Some(late);
    late
}

#[cfg(test)]
#[path = "late_binding_test.rs"]
mod tests;
