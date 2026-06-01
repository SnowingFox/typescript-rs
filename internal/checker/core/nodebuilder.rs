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

use tsgo_ast::SymbolId;

use super::declared_types::get_type_of_symbol;
use super::program::BoundProgram;
use super::types::{ObjectFlags, TypeData, TypeId};
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
        let (name, readonly) = if super::is_synthesized_symbol(property) {
            (
                checker.synthesized_symbol_name(property),
                checker
                    .synthesized_symbol_check_flags(property)
                    .contains(tsgo_ast::CheckFlags::READONLY),
            )
        } else {
            (program.symbol(property).name.clone(), false)
        };
        let property_type = get_type_of_symbol(checker, program, property, None);
        let printed = type_to_string(checker, program, property_type);
        let prefix = if readonly { "readonly " } else { "" };
        parts.push(format!("{prefix}{name}: {printed}"));
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
