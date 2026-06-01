//! Port of Go `internal/transformers/declarations/util.go`: the pure,
//! stateless helpers the declaration (`.d.ts`) transformer leans on, plus the
//! reachable subset of the `ast`/`checker` modifier-flag utilities it calls
//! through the `DeclarationEmitHost` in Go.
//!
//! # Scope (round D-F1)
//!
//! This module ports the helpers the *core* declaration transform needs:
//! `is_always_type`, the modifier-flag masking
//! ([`mask_modifier_flags`]/[`effective_declaration_flags`]/[`combined_modifier_flags`]),
//! the flag→keyword reconstruction ([`modifier_kinds_from_flags`]), and the
//! structural parameter/constructor predicates. The `SymbolTracker`-dependent
//! helpers (`needs_scope_marker`, `can_have_literal_initializer`,
//! `is_declaration_and_not_visible`, `get_binding_name_visible`,
//! `can_produce_diagnostics`, ...) are deferred with the visibility/diagnostics
//! work — see `transform.rs`'s module-level DEFER list.

use tsgo_ast::utilities::modifier_to_flag;
use tsgo_ast::{Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeId};

/// Reports whether `node` is a declaration that is *always* a type and so never
/// needs a synthesized `declare` keyword in `.d.ts` output (only an
/// interface, in the reachable subset).
///
/// # Examples
/// ```
/// use tsgo_transformers::declarations::util::is_always_type;
/// use tsgo_ast::{Kind, NodeArena, NodeList};
/// let mut a = NodeArena::new();
/// let name = a.new_identifier("I");
/// let iface = a.new_interface_declaration(None, Some(name), None, None, NodeList::new(vec![]));
/// assert!(is_always_type(&a, iface));
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/declarations/util.go:isAlwaysType
pub fn is_always_type(arena: &NodeArena, node: NodeId) -> bool {
    arena.kind(node) == Kind::InterfaceDeclaration
}

/// Returns `node`'s own modifier list (the `export`/`declare`/`public`/...
/// keywords and decorators attached to it), cloned, or `None` when the node
/// kind carries no modifiers.
///
/// Mirrors Go's `node.Modifiers()` for the declaration kinds the transform
/// reaches.
///
/// # Examples
/// ```
/// use tsgo_transformers::declarations::util::node_modifiers;
/// use tsgo_ast::{NodeArena, NodeList};
/// let mut a = NodeArena::new();
/// let name = a.new_identifier("f");
/// let f = a.new_function_declaration(None, None, Some(name), None, NodeList::new(vec![]), None, None, None);
/// assert!(node_modifiers(&a, f).is_none());
/// ```
///
/// Side effects: none (clones the list).
// Go: internal/ast/ast.go:Node.Modifiers
pub fn node_modifiers(arena: &NodeArena, node: NodeId) -> Option<ModifierList> {
    match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.modifiers.clone(),
        NodeData::VariableStatement(d) => d.modifiers.clone(),
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.modifiers.clone(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.clone(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.clone(),
        NodeData::EnumDeclaration(d) => d.modifiers.clone(),
        NodeData::ModuleDeclaration(d) => d.modifiers.clone(),
        NodeData::PropertyDeclaration(d) => d.modifiers.clone(),
        NodeData::PropertySignature(d) => d.modifiers.clone(),
        NodeData::MethodDeclaration(d) => d.modifiers.clone(),
        NodeData::MethodSignature(d) => d.modifiers.clone(),
        NodeData::ConstructorDeclaration(d) => d.modifiers.clone(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.clone()
        }
        NodeData::ParameterDeclaration(d) => d.modifiers.clone(),
        NodeData::IndexSignatureDeclaration(d) => d.modifiers.clone(),
        NodeData::TypeParameterDeclaration(d) => d.modifiers.clone(),
        _ => None,
    }
}

/// Returns the union of `node`'s own modifier flags (empty if it bears none).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.ModifierFlags
fn own_modifier_flags(arena: &NodeArena, node: NodeId) -> ModifierFlags {
    node_modifiers(arena, node).map_or(ModifierFlags::empty(), |m| m.modifier_flags)
}

/// Returns the root declaration of `node`, walking up through binding elements
/// (Go's `GetRootDeclaration`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetRootDeclaration
fn get_root_declaration(arena: &NodeArena, node: NodeId) -> NodeId {
    let mut current = node;
    while arena.kind(current) == Kind::BindingElement {
        let Some(pattern) = arena.parent(current) else {
            break;
        };
        let Some(decl) = arena.parent(pattern) else {
            break;
        };
        current = decl;
    }
    current
}

/// Returns the combined modifier flags of `node`: its own flags unioned with
/// its wrapping `VariableDeclarationList`/`VariableStatement` flags, so a
/// variable declaration inherits the `export` from its statement (Go's
/// `GetCombinedModifierFlags`). Re-implemented here because the `ast` crate has
/// not yet ported `GetCombinedModifierFlags`.
///
/// # Examples
/// ```
/// use tsgo_transformers::declarations::util::combined_modifier_flags;
/// use tsgo_ast::{ModifierFlags, NodeArena, NodeList};
/// let mut a = NodeArena::new();
/// let name = a.new_identifier("f");
/// let f = a.new_function_declaration(None, None, Some(name), None, NodeList::new(vec![]), None, None, None);
/// assert_eq!(combined_modifier_flags(&a, f), ModifierFlags::empty());
/// ```
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetCombinedModifierFlags
pub fn combined_modifier_flags(arena: &NodeArena, node: NodeId) -> ModifierFlags {
    let node = get_root_declaration(arena, node);
    let mut flags = own_modifier_flags(arena, node);
    let mut current = node;
    if arena.kind(current) == Kind::VariableDeclaration {
        if let Some(parent) = arena.parent(current) {
            current = parent;
        }
    }
    if arena.kind(current) == Kind::VariableDeclarationList {
        flags |= own_modifier_flags(arena, current);
        if let Some(parent) = arena.parent(current) {
            current = parent;
        }
    }
    if arena.kind(current) == Kind::VariableStatement {
        flags |= own_modifier_flags(arena, current);
    }
    flags
}

/// Returns `node`'s effective declaration flags masked to `flags_to_check`
/// (Go's `Checker.getEffectiveDeclarationFlags`).
///
/// Reachable subset: the combined modifier flags masked. The ambient-export-
/// context auto-export adjustment (a declaration nested in an ambient export
/// context becomes implicitly exported / ambient) is deferred with namespace
/// emit.
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.getEffectiveDeclarationFlags
pub fn effective_declaration_flags(
    arena: &NodeArena,
    node: NodeId,
    flags_to_check: ModifierFlags,
) -> ModifierFlags {
    combined_modifier_flags(arena, node) & flags_to_check
}

/// Masks `node`'s effective flags to `modifier_mask`, adds `modifier_additions`,
/// then applies the `default`-export fixups (Go's `maskModifierFlags`): a
/// non-exported `default` regains `export` (a default export must keep its
/// `export` to be syntactically valid), and `declare` is dropped alongside
/// `default` (the two are never combined).
///
/// # Examples
/// ```
/// use tsgo_transformers::declarations::util::mask_modifier_flags;
/// use tsgo_ast::{ModifierFlags, NodeArena, NodeList};
/// let mut a = NodeArena::new();
/// let name = a.new_identifier("f");
/// let f = a.new_function_declaration(None, None, Some(name), None, NodeList::new(vec![]), None, None, None);
/// // A top-level function gains `declare` (ambient) when requested.
/// assert_eq!(
///     mask_modifier_flags(&a, f, ModifierFlags::ALL, ModifierFlags::AMBIENT),
///     ModifierFlags::AMBIENT
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/declarations/util.go:maskModifierFlags
pub fn mask_modifier_flags(
    arena: &NodeArena,
    node: NodeId,
    modifier_mask: ModifierFlags,
    modifier_additions: ModifierFlags,
) -> ModifierFlags {
    let mut flags = effective_declaration_flags(arena, node, modifier_mask) | modifier_additions;
    if flags.contains(ModifierFlags::DEFAULT) && !flags.contains(ModifierFlags::EXPORT) {
        // A non-exported default is a non-sequitur: a default export must retain
        // its `export` modifier to be syntactically valid.
        flags ^= ModifierFlags::EXPORT;
    }
    if flags.contains(ModifierFlags::DEFAULT) && flags.contains(ModifierFlags::AMBIENT) {
        // `declare` is never required alongside `default` (and would be an error
        // if printed).
        flags ^= ModifierFlags::AMBIENT;
    }
    flags
}

/// Reports whether `kind` is a modifier keyword (excludes decorators), mirroring
/// Go's `ast.IsModifier` / `IsModifierKind`.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsModifierKind
pub fn is_modifier(kind: Kind) -> bool {
    kind != Kind::Decorator && modifier_to_flag(kind) != ModifierFlags::empty()
}

/// Returns the union of modifier flags carried by the keyword nodes in `list`
/// (decorators contribute nothing).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:ModifiersToFlags
pub fn modifiers_to_flags(arena: &NodeArena, nodes: &[NodeId]) -> ModifierFlags {
    nodes.iter().fold(ModifierFlags::empty(), |acc, &n| {
        acc | modifier_to_flag(arena.kind(n))
    })
}

/// Returns the modifier keyword kinds for `flags`, in the canonical declaration
/// order (`export`, `declare`, `default`, `const`, `public`, `private`,
/// `protected`, `abstract`, `static`, `override`, `readonly`, `accessor`,
/// `async`, `in`, `out`), the input to rebuilding a modifier list from flags.
/// Re-implemented here because the `ast` crate has not yet ported
/// `CreateModifiersFromModifierFlags`.
///
/// # Examples
/// ```
/// use tsgo_transformers::declarations::util::modifier_kinds_from_flags;
/// use tsgo_ast::{Kind, ModifierFlags};
/// assert_eq!(
///     modifier_kinds_from_flags(ModifierFlags::EXPORT | ModifierFlags::AMBIENT),
///     vec![Kind::ExportKeyword, Kind::DeclareKeyword],
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:CreateModifiersFromModifierFlags
pub fn modifier_kinds_from_flags(flags: ModifierFlags) -> Vec<Kind> {
    let mut result = Vec::new();
    let mut push_if = |flag: ModifierFlags, kind: Kind| {
        if flags.contains(flag) {
            result.push(kind);
        }
    };
    push_if(ModifierFlags::EXPORT, Kind::ExportKeyword);
    push_if(ModifierFlags::AMBIENT, Kind::DeclareKeyword);
    push_if(ModifierFlags::DEFAULT, Kind::DefaultKeyword);
    push_if(ModifierFlags::CONST, Kind::ConstKeyword);
    push_if(ModifierFlags::PUBLIC, Kind::PublicKeyword);
    push_if(ModifierFlags::PRIVATE, Kind::PrivateKeyword);
    push_if(ModifierFlags::PROTECTED, Kind::ProtectedKeyword);
    push_if(ModifierFlags::ABSTRACT, Kind::AbstractKeyword);
    push_if(ModifierFlags::STATIC, Kind::StaticKeyword);
    push_if(ModifierFlags::OVERRIDE, Kind::OverrideKeyword);
    push_if(ModifierFlags::READONLY, Kind::ReadonlyKeyword);
    push_if(ModifierFlags::ACCESSOR, Kind::AccessorKeyword);
    push_if(ModifierFlags::ASYNC, Kind::AsyncKeyword);
    push_if(ModifierFlags::IN, Kind::InKeyword);
    push_if(ModifierFlags::OUT, Kind::OutKeyword);
    result
}

/// Reports whether parameter `node` is the synthetic `this` parameter
/// (`function f(this: T)`), which is TypeScript-only.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsThisParameter
pub fn is_this_parameter(arena: &NodeArena, node: NodeId) -> bool {
    let name = match arena.data(node) {
        NodeData::ParameterDeclaration(d) => d.name,
        _ => return false,
    };
    arena.kind(name) == Kind::Identifier && arena.text(name) == "this"
}

/// Returns the `this` parameter of signature `node`, if its first parameter is
/// one (Go's `GetThisParameter`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetThisParameter
pub fn get_this_parameter(arena: &NodeArena, params: &[NodeId]) -> Option<NodeId> {
    params
        .first()
        .copied()
        .filter(|&p| is_this_parameter(arena, p))
}

/// Reports whether constructor `node` (a member node) is a constructor that has
/// a body, the implementation whose parameter-property parameters the class
/// transform hoists (Go's `GetFirstConstructorWithBody`, scoped to the members
/// it scans).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetFirstConstructorWithBody
pub fn get_first_constructor_with_body(arena: &NodeArena, members: &[NodeId]) -> Option<NodeId> {
    members
        .iter()
        .copied()
        .find(|&m| matches!(arena.data(m), NodeData::ConstructorDeclaration(d) if d.body.is_some()))
}

/// Reports whether parameter `node` carries a parameter-property modifier
/// (`public`/`private`/`protected`/`readonly`/`override`), i.e. it declares a
/// class field (Go's `HasSyntacticModifier(node, ParameterPropertyModifier)`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:HasSyntacticModifier
pub fn has_parameter_property_modifier(arena: &NodeArena, node: NodeId) -> bool {
    own_modifier_flags(arena, node).intersects(ModifierFlags::PARAMETER_PROPERTY_MODIFIER)
}

/// Reports whether parameter `node` is optional for `.d.ts` emit: it bears a
/// `?` token or a default initializer (and is not a rest parameter). The
/// reachable structural stand-in for the checker's `IsOptionalParameter`.
///
/// # Examples
/// ```
/// use tsgo_transformers::declarations::util::is_optional_parameter;
/// use tsgo_ast::NodeArena;
/// let mut a = NodeArena::new();
/// let name = a.new_identifier("x");
/// let q = a.new_token(tsgo_ast::Kind::QuestionToken);
/// let p = a.new_parameter_declaration(None, None, name, Some(q), None, None);
/// assert!(is_optional_parameter(&a, p));
/// ```
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.isOptionalParameter (reachable subset)
pub fn is_optional_parameter(arena: &NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ParameterDeclaration(d) => {
            d.dot_dot_dot_token.is_none() && (d.question_token.is_some() || d.initializer.is_some())
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
