//! Node query/utility helpers (`set_parent_in_children`, `is_*` predicates).
//!
//! Go's `utilities.go` is ~417 functions; this phase ports the parent-wiring
//! helper used by deep clone plus a representative spread of kind predicates.
//! More are pulled in by upstream phases as their callers land.

use crate::{Kind, ModifierFlags, NodeArena, NodeId};

/// Maps a modifier keyword `kind` to its [`ModifierFlags`] bit (empty if `kind`
/// is not a modifier keyword).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:ModifierToFlag
pub fn modifier_to_flag(kind: Kind) -> ModifierFlags {
    match kind {
        Kind::StaticKeyword => ModifierFlags::STATIC,
        Kind::PublicKeyword => ModifierFlags::PUBLIC,
        Kind::ProtectedKeyword => ModifierFlags::PROTECTED,
        Kind::PrivateKeyword => ModifierFlags::PRIVATE,
        Kind::AbstractKeyword => ModifierFlags::ABSTRACT,
        Kind::AccessorKeyword => ModifierFlags::ACCESSOR,
        Kind::ExportKeyword => ModifierFlags::EXPORT,
        Kind::DeclareKeyword => ModifierFlags::AMBIENT,
        Kind::ConstKeyword => ModifierFlags::CONST,
        Kind::DefaultKeyword => ModifierFlags::DEFAULT,
        Kind::AsyncKeyword => ModifierFlags::ASYNC,
        Kind::ReadonlyKeyword => ModifierFlags::READONLY,
        Kind::OverrideKeyword => ModifierFlags::OVERRIDE,
        Kind::InKeyword => ModifierFlags::IN,
        Kind::OutKeyword => ModifierFlags::OUT,
        Kind::Decorator => ModifierFlags::DECORATOR,
        _ => ModifierFlags::empty(),
    }
}

impl NodeArena {
    /// Sets the `parent` of every node in the subtree rooted at `root` to its
    /// enclosing node, recursively. The root's own parent is left unchanged.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// let mut arena = NodeArena::new();
    /// let a = arena.new_identifier("a");
    /// let b = arena.new_identifier("b");
    /// let qn = arena.new_qualified_name(a, b);
    /// arena.set_parent_in_children(qn);
    /// assert_eq!(arena.parent(a), Some(qn));
    /// assert_eq!(arena.parent(qn), None);
    /// ```
    ///
    /// Side effects: mutates the `parent` of descendant nodes.
    // Go: internal/ast/utilities.go:SetParentInChildren
    pub fn set_parent_in_children(&mut self, root: NodeId) {
        let mut children = Vec::new();
        self.for_each_child(root, &mut |c| {
            children.push(c);
            false
        });
        for child in children {
            self.set_parent(child, Some(root));
            self.set_parent_in_children(child);
        }
    }
}

/// Reports whether node `id` is an identifier.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsIdentifier
pub fn is_identifier(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::Identifier
}

/// Reports whether node `id` is a call expression.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsCallExpression
pub fn is_call_expression(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::CallExpression
}

/// Reports whether node `id` is a property access expression.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsPropertyAccessExpression
pub fn is_property_access_expression(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::PropertyAccessExpression
}

/// Reports whether node `id` is a string literal.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsStringLiteral
pub fn is_string_literal(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::StringLiteral
}

/// Reports whether node `id` is "missing": an empty range (`pos == end`) at a
/// real (non-synthetic) position that is not the end-of-file token.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:NodeIsMissing
pub fn node_is_missing(arena: &NodeArena, id: NodeId) -> bool {
    let loc = arena.loc(id);
    loc.pos() == loc.end() && loc.pos() >= 0 && arena.kind(id) != Kind::EndOfFile
}

/// Reports whether node `id` is present (not [`node_is_missing`]).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:NodeIsPresent
pub fn node_is_present(arena: &NodeArena, id: NodeId) -> bool {
    !node_is_missing(arena, id)
}

/// Reports whether `token` is a keyword.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsKeyword
pub fn is_keyword(token: Kind) -> bool {
    Kind::FIRST_KEYWORD <= token && token <= Kind::LAST_KEYWORD
}

/// Reports whether `kind` is a keyword kind.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsKeywordKind
pub fn is_keyword_kind(kind: Kind) -> bool {
    Kind::FIRST_KEYWORD <= kind && kind <= Kind::LAST_KEYWORD
}

/// Reports whether `kind` is a punctuation kind.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsPunctuationKind
pub fn is_punctuation_kind(kind: Kind) -> bool {
    Kind::FIRST_PUNCTUATION <= kind && kind <= Kind::LAST_PUNCTUATION
}

/// Reports whether `kind` is a token kind (token or keyword).
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsTokenKind
pub fn is_token_kind(kind: Kind) -> bool {
    Kind::FIRST_TOKEN <= kind && kind <= Kind::LAST_TOKEN
}

/// Reports whether `kind` is a modifier keyword.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsModifierKind
pub fn is_modifier_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::AbstractKeyword
            | Kind::AccessorKeyword
            | Kind::AsyncKeyword
            | Kind::ConstKeyword
            | Kind::DeclareKeyword
            | Kind::DefaultKeyword
            | Kind::ExportKeyword
            | Kind::InKeyword
            | Kind::PrivateKeyword
            | Kind::ProtectedKeyword
            | Kind::PublicKeyword
            | Kind::ReadonlyKeyword
            | Kind::OutKeyword
            | Kind::OverrideKeyword
            | Kind::StaticKeyword
    )
}

/// Reports whether `kind` is a parameter-property modifier
/// (`public`/`private`/`protected`/`readonly`/`override`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsParameterPropertyModifier
pub fn is_parameter_property_modifier(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::PublicKeyword
            | Kind::PrivateKeyword
            | Kind::ProtectedKeyword
            | Kind::ReadonlyKeyword
            | Kind::OverrideKeyword
    )
}

/// Reports whether `kind` is a class-member modifier (a parameter-property
/// modifier, `static`, or `accessor`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsClassMemberModifier
pub fn is_class_member_modifier(kind: Kind) -> bool {
    is_parameter_property_modifier(kind)
        || kind == Kind::StaticKeyword
        || kind == Kind::AccessorKeyword
}

/// Reports whether `kind` is an assignment operator token.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsAssignmentOperator
pub fn is_assignment_operator(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::EqualsToken
            | Kind::PlusEqualsToken
            | Kind::MinusEqualsToken
            | Kind::AsteriskAsteriskEqualsToken
            | Kind::AsteriskEqualsToken
            | Kind::SlashEqualsToken
            | Kind::PercentEqualsToken
            | Kind::AmpersandEqualsToken
            | Kind::BarEqualsToken
            | Kind::CaretEqualsToken
            | Kind::LessThanLessThanEqualsToken
            | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanEqualsToken
            | Kind::BarBarEqualsToken
            | Kind::AmpersandAmpersandEqualsToken
            | Kind::QuestionQuestionEqualsToken
    )
}

/// Reports whether `kind` denotes a left-hand-side expression node.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:isLeftHandSideExpressionKind
pub fn is_left_hand_side_expression_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::PropertyAccessExpression
            | Kind::ElementAccessExpression
            | Kind::NewExpression
            | Kind::CallExpression
            | Kind::JsxElement
            | Kind::JsxSelfClosingElement
            | Kind::JsxFragment
            | Kind::TaggedTemplateExpression
            | Kind::ArrayLiteralExpression
            | Kind::ParenthesizedExpression
            | Kind::ObjectLiteralExpression
            | Kind::ClassExpression
            | Kind::FunctionExpression
            | Kind::Identifier
            | Kind::PrivateIdentifier
            | Kind::RegularExpressionLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::StringLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::TemplateExpression
            | Kind::FalseKeyword
            | Kind::NullKeyword
            | Kind::ThisKeyword
            | Kind::TrueKeyword
            | Kind::SuperKeyword
            | Kind::NonNullExpression
            | Kind::ExpressionWithTypeArguments
            | Kind::MetaProperty
            | Kind::ImportKeyword
            | Kind::MissingDeclaration
    )
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
