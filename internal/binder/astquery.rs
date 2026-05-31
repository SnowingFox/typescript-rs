//! Binder-local AST query helpers.
//!
//! The Go binder calls a large set of `ast.IsXxx` / `ast.GetXxx` predicates that
//! are not yet ported into the `tsgo_ast` crate. Rather than expand `tsgo_ast`,
//! we port the small subset the binder needs here as `pub(crate)` free functions
//! over a [`NodeArena`]. Each mirrors its Go upstream (see the `// Go:` anchors)
//! and is unit-tested in `astquery_test.rs`.

use tsgo_ast::{Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeFlags, NodeId};

/// Returns the modifier list carried by a node, if any.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Modifiers
pub(crate) fn modifiers_of(arena: &NodeArena, id: NodeId) -> Option<&ModifierList> {
    match arena.data(id) {
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.modifiers.as_ref(),
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeParameterDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => d.modifiers.as_ref(),
        NodeData::PropertyAssignment(d) => d.modifiers.as_ref(),
        NodeData::MethodSignature(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        NodeData::ConstructorDeclaration(d) => d.modifiers.as_ref(),
        NodeData::IndexSignatureDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassStaticBlockDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportEqualsDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportAssignment(d) => d.modifiers.as_ref(),
        NodeData::NamespaceExportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ArrowFunction(d) => d.modifiers.as_ref(),
        NodeData::ShorthandPropertyAssignment(d) => d.modifiers.as_ref(),
        NodeData::FunctionType(d) | NodeData::ConstructorType(d) => d.modifiers.as_ref(),
        _ => None,
    }
}

/// Returns the syntactic modifier flags directly on a node.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.ModifierFlags
pub(crate) fn node_modifier_flags(arena: &NodeArena, id: NodeId) -> ModifierFlags {
    modifiers_of(arena, id).map_or(ModifierFlags::NONE, |m| m.modifier_flags)
}

/// Reports whether a node carries the given syntactic modifier flags.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:HasSyntacticModifier
pub(crate) fn has_syntactic_modifier(arena: &NodeArena, id: NodeId, flags: ModifierFlags) -> bool {
    node_modifier_flags(arena, id).intersects(flags)
}

/// Returns the binding name of a declaration node, if it has one.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Name
pub(crate) fn declaration_name(arena: &NodeArena, id: NodeId) -> Option<NodeId> {
    match arena.data(id) {
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.name,
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.name,
        NodeData::TypeAliasDeclaration(d) => Some(d.name),
        NodeData::EnumDeclaration(d) => Some(d.name),
        NodeData::EnumMember(d) => Some(d.name),
        NodeData::ModuleDeclaration(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => Some(d.name),
        NodeData::PropertyAssignment(d) => Some(d.name),
        NodeData::MethodDeclaration(d) => Some(d.name),
        NodeData::MethodSignature(d) => Some(d.name),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => Some(d.name),
        NodeData::ParameterDeclaration(d) => Some(d.name),
        NodeData::TypeParameterDeclaration(d) => Some(d.name),
        NodeData::BindingElement(d) => d.name,
        NodeData::ShorthandPropertyAssignment(d) => Some(d.name),
        NodeData::ImportClause(d) => d.name,
        NodeData::NamespaceImport(d) | NodeData::NamespaceExport(d) => Some(d.name),
        NodeData::ImportSpecifier(d) | NodeData::ExportSpecifier(d) => Some(d.name),
        NodeData::ImportEqualsDeclaration(d) => Some(d.name),
        NodeData::NamespaceExportDeclaration(d) => Some(d.name),
        _ => None,
    }
}

/// Returns the name node of a declaration (the non-assigned form).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetNameOfDeclaration
pub(crate) fn name_of_declaration(arena: &NodeArena, id: NodeId) -> Option<NodeId> {
    match arena.data(id) {
        NodeData::ExportAssignment(d) => {
            if is_identifier(arena, d.expression) {
                Some(d.expression)
            } else {
                None
            }
        }
        // JS assignment declarations (`a.b = ...`) name resolution.
        NodeData::BinaryExpression(_) | NodeData::CallExpression(_) => None,
        _ => declaration_name(arena, id),
    }
}

/// Reports whether a node is an identifier.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsIdentifier
pub(crate) fn is_identifier(arena: &NodeArena, id: NodeId) -> bool {
    matches!(arena.data(id), NodeData::Identifier(_))
}

/// Reports whether a node is a private identifier.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsPrivateIdentifier
pub(crate) fn is_private_identifier(arena: &NodeArena, id: NodeId) -> bool {
    matches!(arena.data(id), NodeData::PrivateIdentifier(_))
}

/// Reports whether a node is a computed property name.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsComputedPropertyName
pub(crate) fn is_computed_property_name(arena: &NodeArena, id: NodeId) -> bool {
    matches!(arena.data(id), NodeData::ComputedPropertyName(_))
}

/// Reports whether a node can serve as a property-name literal.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsPropertyNameLiteral
pub(crate) fn is_property_name_literal(arena: &NodeArena, id: NodeId) -> bool {
    matches!(
        arena.kind(id),
        Kind::Identifier
            | Kind::StringLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::NumericLiteral
    )
}

/// Reports whether a node is a string or numeric literal-like leaf.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsStringOrNumericLiteralLike
pub(crate) fn is_string_or_numeric_literal_like(arena: &NodeArena, id: NodeId) -> bool {
    matches!(
        arena.kind(id),
        Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral | Kind::NumericLiteral
    )
}

/// Reports whether a node is a signed numeric literal (`+1` / `-1`).
///
/// Side effects: none (pure).
// Go: internal/binder/binder.go:isSignedNumericLiteral
pub(crate) fn is_signed_numeric_literal(arena: &NodeArena, id: NodeId) -> bool {
    if let NodeData::PrefixUnaryExpression(d) = arena.data(id) {
        (d.operator == Kind::PlusToken || d.operator == Kind::MinusToken)
            && arena.kind(d.operand) == Kind::NumericLiteral
    } else {
        false
    }
}

/// Reports whether a declaration carries a dynamic (computed, non-literal) name.
///
/// A dynamic name is one whose runtime value is not statically known, e.g.
/// `[Symbol.iterator]` or `[expr]`. Such declarations are bound anonymously
/// under `InternalSymbolNameComputed` rather than by their text, which is why
/// the binder must consult this before naming a member.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:HasDynamicName
pub(crate) fn has_dynamic_name(arena: &NodeArena, declaration: NodeId) -> bool {
    match name_of_declaration(arena, declaration) {
        Some(name) => is_dynamic_name(arena, name),
        None => false,
    }
}

/// Reports whether a name node is dynamic (a computed property name whose
/// expression is neither a string/numeric literal nor a signed numeric literal).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsDynamicName
pub(crate) fn is_dynamic_name(arena: &NodeArena, name: NodeId) -> bool {
    let expr = match arena.data(name) {
        NodeData::ComputedPropertyName(d) => d.expression,
        // The `ElementAccessExpression` name arm is reached only via JS expando
        // assignment declarations (`a[b] = ...`), which are deferred in this port.
        // DEFER: JS expando assignment names. // Go: utilities.go:IsDynamicName
        _ => return false,
    };
    !is_string_or_numeric_literal_like(arena, expr) && !is_signed_numeric_literal(arena, expr)
}

/// Reports whether a node is a class declaration or class expression.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsClassLike
pub(crate) fn is_class_like(arena: &NodeArena, id: NodeId) -> bool {
    matches!(
        arena.kind(id),
        Kind::ClassDeclaration | Kind::ClassExpression
    )
}

/// Reports whether a kind is function-like (including signatures).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsFunctionLikeKind
pub(crate) fn is_function_like_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::MethodSignature
            | Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::FunctionType
            | Kind::ConstructorType
            | Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::FunctionExpression
            | Kind::ArrowFunction
    )
}

/// Reports whether a node is function-like (including signatures).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsFunctionLike
pub(crate) fn is_function_like(arena: &NodeArena, id: NodeId) -> bool {
    is_function_like_kind(arena.kind(id))
}

/// Reports whether a node is a class element.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsClassElement
pub(crate) fn is_class_element(arena: &NodeArena, id: NodeId) -> bool {
    matches!(
        arena.kind(id),
        Kind::Constructor
            | Kind::PropertyDeclaration
            | Kind::MethodDeclaration
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::IndexSignature
            | Kind::ClassStaticBlockDeclaration
            | Kind::SemicolonClassElement
    )
}

/// Reports whether a node is a class static block declaration.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsClassStaticBlockDeclaration
pub(crate) fn is_class_static_block(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::ClassStaticBlockDeclaration
}

/// Reports whether a node is `static` (by modifier or being a static block).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsStatic
pub(crate) fn is_static(arena: &NodeArena, id: NodeId) -> bool {
    is_class_element(arena, id) && has_syntactic_modifier(arena, id, ModifierFlags::STATIC)
        || is_class_static_block(arena, id)
}

/// Reports whether a method/accessor lives in an object literal or class expression.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsObjectLiteralOrClassExpressionMethodOrAccessor
pub(crate) fn is_object_literal_or_class_expression_method_or_accessor(
    arena: &NodeArena,
    id: NodeId,
) -> bool {
    let is_member = matches!(
        arena.kind(id),
        Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor
    );
    is_member
        && arena.parent(id).is_some_and(|p| {
            matches!(
                arena.kind(p),
                Kind::ObjectLiteralExpression | Kind::ClassExpression
            )
        })
}

/// Reports whether a property declaration carries an `accessor` modifier.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsAutoAccessorPropertyDeclaration
pub(crate) fn is_auto_accessor_property_declaration(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::PropertyDeclaration
        && has_syntactic_modifier(arena, id, ModifierFlags::ACCESSOR)
}

/// Returns the root declaration, walking out of binding elements.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetRootDeclaration
pub(crate) fn get_root_declaration(arena: &NodeArena, mut id: NodeId) -> NodeId {
    while arena.kind(id) == Kind::BindingElement {
        // node = node.Parent.Parent
        match arena.parent(id).and_then(|p| arena.parent(p)) {
            Some(grand) => id = grand,
            None => break,
        }
    }
    id
}

/// Returns the combined node flags of a declaration, folding in the flags of an
/// enclosing variable declaration list / statement.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetCombinedNodeFlags
pub(crate) fn combined_node_flags(arena: &NodeArena, id: NodeId) -> NodeFlags {
    let mut node = get_root_declaration(arena, id);
    let mut flags = arena.flags(node);
    if arena.kind(node) == Kind::VariableDeclaration {
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableDeclarationList {
        flags |= arena.flags(node);
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableStatement {
        flags |= arena.flags(node);
    }
    flags
}

/// Returns the combined modifier flags of a declaration, folding in modifiers of
/// an enclosing variable declaration list / statement.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetCombinedModifierFlags
pub(crate) fn combined_modifier_flags(arena: &NodeArena, id: NodeId) -> ModifierFlags {
    let mut node = get_root_declaration(arena, id);
    let mut flags = node_modifier_flags(arena, node);
    if arena.kind(node) == Kind::VariableDeclaration {
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableDeclarationList {
        flags |= node_modifier_flags(arena, node);
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableStatement {
        flags |= node_modifier_flags(arena, node);
    }
    flags
}

/// Reports whether a declaration is block- or catch-scoped.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsBlockOrCatchScoped
pub(crate) fn is_block_or_catch_scoped(arena: &NodeArena, id: NodeId) -> bool {
    combined_node_flags(arena, id).intersects(NodeFlags::BLOCK_SCOPED)
        || is_catch_clause_variable_declaration_or_binding_element(arena, id)
}

/// Reports whether a declaration is the variable of a `catch` clause.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsCatchClauseVariableDeclarationOrBindingElement
pub(crate) fn is_catch_clause_variable_declaration_or_binding_element(
    arena: &NodeArena,
    id: NodeId,
) -> bool {
    let node = get_root_declaration(arena, id);
    arena.kind(node) == Kind::VariableDeclaration
        && arena
            .parent(node)
            .is_some_and(|p| arena.kind(p) == Kind::CatchClause)
}

/// Reports whether a statement is a string-literal prologue directive.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsPrologueDirective
pub(crate) fn is_prologue_directive(arena: &NodeArena, id: NodeId) -> bool {
    if let NodeData::ExpressionStatement(d) = arena.data(id) {
        arena.kind(d.expression) == Kind::StringLiteral
    } else {
        false
    }
}

/// Returns the innermost containing class (searching ancestors of the node).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetContainingClass
pub(crate) fn get_containing_class(arena: &NodeArena, id: NodeId) -> Option<NodeId> {
    let mut cur = arena.parent(id);
    while let Some(n) = cur {
        if is_class_like(arena, n) {
            return Some(n);
        }
        cur = arena.parent(n);
    }
    None
}

/// Reports whether a node is an ambient (`declare module "x"` / `global`) module.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsAmbientModule
pub(crate) fn is_ambient_module(arena: &NodeArena, id: NodeId) -> bool {
    if let NodeData::ModuleDeclaration(d) = arena.data(id) {
        arena.kind(d.name) == Kind::StringLiteral || d.keyword == Kind::GlobalKeyword
    } else {
        false
    }
}

/// Reports whether a node is a `global` scope augmentation.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsGlobalScopeAugmentation
pub(crate) fn is_global_scope_augmentation(arena: &NodeArena, id: NodeId) -> bool {
    matches!(arena.data(id), NodeData::ModuleDeclaration(d) if d.keyword == Kind::GlobalKeyword)
}

/// Reports whether a node is an export assignment (`export =` / `export default`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsExportAssignment
pub(crate) fn is_export_assignment(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::ExportAssignment
}

/// Reports whether a node is an export declaration.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsExportDeclaration
pub(crate) fn is_export_declaration(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::ExportDeclaration
}

/// Reports whether the root declaration of a node is a parameter.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsPartOfParameterDeclaration
pub(crate) fn is_part_of_parameter_declaration(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(get_root_declaration(arena, id)) == Kind::Parameter
}

/// Reports whether a node is a binding pattern (object/array).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsBindingPattern
pub(crate) fn is_binding_pattern(arena: &NodeArena, id: NodeId) -> bool {
    matches!(
        arena.kind(id),
        Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
    )
}

/// Reports whether an expression node aliases another binding.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:ExpressionIsAlias
pub(crate) fn expression_is_alias(arena: &NodeArena, id: NodeId) -> bool {
    is_entity_name_expression(arena, id) || arena.kind(id) == Kind::ClassExpression
}

/// Reports whether a node is an entity-name expression (`a`, `a.b`, ...).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsEntityNameExpression
pub(crate) fn is_entity_name_expression(arena: &NodeArena, id: NodeId) -> bool {
    if is_identifier(arena, id) {
        return true;
    }
    if let NodeData::PropertyAccessExpression(d) = arena.data(id) {
        return is_identifier(arena, d.name) && is_entity_name_expression(arena, d.expression);
    }
    false
}

/// Reports whether a node might be executed at runtime (used for unreachable
/// code marking).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsPotentiallyExecutableNode
pub(crate) fn is_potentially_executable_node(arena: &NodeArena, id: NodeId) -> bool {
    let kind = arena.kind(id);
    if kind >= Kind::FIRST_STATEMENT && kind <= Kind::LAST_STATEMENT {
        if let NodeData::VariableStatement(d) = arena.data(id) {
            let list = d.declaration_list;
            if combined_node_flags(arena, list).intersects(NodeFlags::BLOCK_SCOPED) {
                return true;
            }
            if let NodeData::VariableDeclarationList(ld) = arena.data(list) {
                return ld.declarations.nodes.iter().any(|&decl| {
                    matches!(arena.data(decl), NodeData::VariableDeclaration(vd) if vd.initializer.is_some())
                });
            }
            return false;
        }
        return true;
    }
    matches!(
        kind,
        Kind::ClassDeclaration | Kind::EnumDeclaration | Kind::ModuleDeclaration
    )
}

/// Returns the postfix `?`/`!` token of a member declaration, if any.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.PostfixToken
pub(crate) fn postfix_token(arena: &NodeArena, id: NodeId) -> Option<NodeId> {
    match arena.data(id) {
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => d.postfix_token,
        NodeData::MethodDeclaration(d) => d.postfix_token,
        NodeData::MethodSignature(d) => d.postfix_token,
        NodeData::ShorthandPropertyAssignment(d) => d.postfix_token,
        _ => None,
    }
}

/// Returns the text of an identifier or literal-like name node.
///
/// Mirrors a narrow slice of Go `scanner.DeclarationNameToString` sufficient for
/// the binder's display names.
///
/// Side effects: none (pure).
// Go: internal/scanner/utilities.go:DeclarationNameToString
pub(crate) fn declaration_name_to_string(arena: &NodeArena, id: NodeId) -> String {
    match arena.data(id) {
        NodeData::Identifier(d) => d.text.clone(),
        NodeData::PrivateIdentifier(d) => d.text.clone(),
        NodeData::StringLiteral(d) | NodeData::NumericLiteral(d) => d.text.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
#[path = "astquery_test.rs"]
mod tests;
