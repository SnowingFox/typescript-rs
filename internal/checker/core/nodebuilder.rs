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
//! array/tuple types, mapped/conditional types, alias names, and
//! optional/readonly member adornments — those need type kinds not yet
//! constructed and the full node-builder scope machinery. (4v adds intersection
//! printing: `A & B`.)

use tsgo_ast::SymbolId;

use super::declared_types::get_type_of_symbol;
use super::program::BoundProgram;
use super::types::{TypeData, TypeId};
use super::Checker;

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
pub fn symbol_to_string(program: &dyn BoundProgram, symbol: SymbolId) -> String {
    program.symbol(symbol).name.clone()
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
                .map(|s| symbol_to_string(program, s))
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
            let name = symbol_to_string(program, symbol);
            if !name.starts_with(tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_PREFIX) {
                return name;
            }
        }
        // An anonymous object type prints its member literal.
        return serialize_members(checker, program, ty);
    }
    // Intrinsics/literals/unions (and not-yet-handled kinds) use the
    // program-less printer.
    checker.type_to_string(ty)
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
        let name = program.symbol(property).name.clone();
        let property_type = get_type_of_symbol(checker, program, property, None);
        let printed = type_to_string(checker, program, property_type);
        parts.push(format!("{name}: {printed}"));
    }
    format!("{{ {}; }}", parts.join("; "))
}

#[cfg(test)]
#[path = "nodebuilder_test.rs"]
mod tests;
