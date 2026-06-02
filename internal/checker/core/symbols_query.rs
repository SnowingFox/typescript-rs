//! Symbol queries for the language service / external tools.
//!
//! Ports the subset of Go's `getSymbolAtLocation` reachable so far: the
//! declaration-name path (interface/variable/... names) and an `ident.member`
//! property-access path. As of 4c the property path goes through the real
//! type-backed property lookup (`get_type_of_symbol` -> `get_property_of_type`)
//! rather than 4b's structural annotation walk. The full routine handles many
//! more node shapes and routes property access through
//! `checkPropertyAccessExpression`, which lands in later sub-phases.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, SymbolFlags, SymbolId, SymbolTable};

use super::declared_types::{get_property_of_type, get_type_of_symbol};
use super::program::BoundProgram;
use super::symbols::resolve_name;
use super::Checker;

/// Returns *some* symbol associated with `node`, or `None`.
///
/// Mirrors Go's deliberately "fuzzy" `getSymbolAtLocation` (an API for tooling,
/// not for type checking). 4b handles declaration names and a structural
/// `ident.member` property access; `globals` is the outermost scope consulted
/// by name resolution.
///
/// DEFER(phase-4-checker-4g): the many remaining node shapes (`this`/`super`,
/// string/numeric literal index access, meta-properties, JSX names, import/
/// export specifiers, ...) and element-access / full expression-checked
/// property resolution are not yet ported.
/// blocked-by: `checkExpression`/`checkPropertyAccessExpression` land in 4g.
///
/// # Examples
/// ```
/// use tsgo_checker::{get_symbol_at_location, BoundProgram, Checker};
/// use tsgo_ast::{NodeId, SymbolId};
/// fn symbol_at<P: BoundProgram>(c: &mut Checker, p: &P, node: NodeId) -> Option<SymbolId> {
///     get_symbol_at_location(c, p, node, None)
/// }
/// ```
///
/// Side effects: may build declared/value types via the property path.
// Go: internal/checker/checker.go:Checker.getSymbolAtLocation
pub fn get_symbol_at_location(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
    globals: Option<&SymbolTable>,
) -> Option<SymbolId> {
    // A declaration's name resolves to the declaration's own symbol.
    if is_declaration_name(program.arena(), node) {
        let parent = program.arena().parent(node)?;
        return get_symbol_of_declaration(program, parent);
    }

    match program.arena().kind(node) {
        Kind::Identifier | Kind::PropertyAccessExpression => {
            get_symbol_of_name_or_property_access_expression(checker, program, node, globals)
        }
        _ => None,
    }
}

/// Returns the declaration symbol bound to `node`.
///
/// Mirrors Go's `getSymbolOfDeclaration`. Merged-symbol canonicalization and
/// late-bound (computed) member resolution are identity in 4b (single file, no
/// merges).
///
/// DEFER(phase-4-checker-4c): merged-symbol and late-bound resolution.
/// blocked-by: cross-file symbol merging and computed-member late binding land
/// with multi-file programs (P6) and member resolution (4c).
///
/// # Examples
/// ```
/// use tsgo_checker::{get_symbol_of_declaration, BoundProgram};
/// use tsgo_ast::{NodeId, SymbolId};
/// fn decl_symbol<P: BoundProgram>(p: &P, decl: NodeId) -> Option<SymbolId> {
///     get_symbol_of_declaration(p, decl)
/// }
/// ```
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.getSymbolOfDeclaration
pub fn get_symbol_of_declaration(program: &dyn BoundProgram, node: NodeId) -> Option<SymbolId> {
    program.symbol_of_node(node)
}

// Go: internal/checker/checker.go:Checker.getSymbolOfNameOrPropertyAccessExpression
fn get_symbol_of_name_or_property_access_expression(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
    globals: Option<&SymbolTable>,
) -> Option<SymbolId> {
    match program.arena().kind(node) {
        // A bare identifier in expression position resolves as a value.
        Kind::Identifier => {
            let name = program.arena().text(node);
            resolve_name(program, node, name, SymbolFlags::VALUE, false, globals)
        }
        Kind::PropertyAccessExpression => {
            get_symbol_of_property_access(checker, program, node, globals)
        }
        _ => None,
    }
}

// Resolves `receiver.member`: resolve the receiver value symbol, compute its
// type, and look the member up on that type via `get_property_of_type`.
//
// DEFER(phase-4-checker-4g): the faithful path runs the receiver through
// `checkPropertyAccessExpression` (unions, inherited and index members, optional
// chains, element access, and non-identifier receivers). 4c handles
// `<identifier>.<name>`.
// blocked-by: expression checking (4g) and relations/instantiation (4d).
// Go: internal/checker/checker.go:Checker.getSymbolOfNameOrPropertyAccessExpression
fn get_symbol_of_property_access(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
    globals: Option<&SymbolTable>,
) -> Option<SymbolId> {
    let (expression, name) = match program.arena().data(node) {
        NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
        _ => return None,
    };
    if program.arena().kind(expression) != Kind::Identifier {
        return None;
    }
    let receiver = resolve_name(
        program,
        expression,
        program.arena().text(expression),
        SymbolFlags::VALUE,
        false,
        globals,
    )?;
    let receiver_type = get_type_of_symbol(checker, program, receiver, globals);
    let member_name = program.arena().text(name);
    get_property_of_type(checker, receiver_type, member_name)
}

// Reports whether `node` is the name of a declaration whose name field is
// `node` itself (the subset of Go's `IsDeclarationName` 4b needs).
// Go: internal/ast/utilities.go:IsDeclarationName
fn is_declaration_name(arena: &NodeArena, node: NodeId) -> bool {
    if arena.kind(node) != Kind::Identifier {
        return false;
    }
    match arena.parent(node) {
        Some(parent) => name_of_declaration(arena, parent) == Some(node),
        None => false,
    }
}

// Returns the "name" child of a declaration node, for the declaration kinds 4b
// resolves. More kinds are added as their queries are needed.
// Go: internal/ast/utilities.go:getNameOfDeclaration (subset)
pub(crate) fn name_of_declaration(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::InterfaceDeclaration(d)
        | NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d) => d.name,
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => Some(d.name),
        NodeData::MethodDeclaration(d) => Some(d.name),
        _ => None,
    }
}

#[cfg(test)]
#[path = "symbols_query_test.rs"]
mod tests;
