//! Operator precedence (`OperatorPrecedence`) and precedence queries.

use crate::{Kind, NodeArena, NodeData, NodeFlags, NodeId};

/// Operator precedence levels, from lowest (`Comma`) to highest (`Parentheses`).
///
/// Mirrors Go `OperatorPrecedence` (an `int` `iota` enum). `Invalid` is `-1`,
/// lower than all real precedences, and stops binary-expression parsing.
///
/// # Examples
/// ```
/// use tsgo_ast::precedence::OperatorPrecedence;
/// assert!(OperatorPrecedence::Additive > OperatorPrecedence::Comma);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ast/precedence.go:OperatorPrecedence
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(i32)]
pub enum OperatorPrecedence {
    /// `Invalid` (`-1`): lower than all others; stops binary parsing.
    Invalid = -1,
    /// `,` comma.
    Comma = 0,
    /// `...` spread.
    Spread,
    /// `yield`.
    Yield,
    /// Assignment operators.
    Assignment,
    /// Conditional `?:`.
    Conditional,
    /// Logical OR `||`.
    LogicalOR,
    /// Logical AND `&&`.
    LogicalAND,
    /// Bitwise OR `|`.
    BitwiseOR,
    /// Bitwise XOR `^`.
    BitwiseXOR,
    /// Bitwise AND `&`.
    BitwiseAND,
    /// Equality `== != === !==`.
    Equality,
    /// Relational `< > <= >= instanceof in as satisfies`.
    Relational,
    /// Shift `<< >> >>>`.
    Shift,
    /// Additive `+ -`.
    Additive,
    /// Multiplicative `* / %`.
    Multiplicative,
    /// Exponentiation `**`.
    Exponentiation,
    /// Unary operators.
    Unary,
    /// Update `++ --` (postfix).
    Update,
    /// Left-hand-side expressions.
    LeftHandSide,
    /// Optional chains.
    OptionalChain,
    /// Member access / calls.
    Member,
    /// Primary expressions.
    Primary,
    /// Parenthesized expressions.
    Parentheses,
}

impl OperatorPrecedence {
    /// Lowest precedence (`Comma`).
    // Go: internal/ast/precedence.go:OperatorPrecedenceLowest
    pub const LOWEST: OperatorPrecedence = OperatorPrecedence::Comma;
    /// Highest precedence (`Parentheses`).
    // Go: internal/ast/precedence.go:OperatorPrecedenceHighest
    pub const HIGHEST: OperatorPrecedence = OperatorPrecedence::Parentheses;
    /// The precedence below which comma is disallowed (`Yield`).
    // Go: internal/ast/precedence.go:OperatorPrecedenceDisallowComma
    pub const DISALLOW_COMMA: OperatorPrecedence = OperatorPrecedence::Yield;
    /// Coalesce `??` precedence (same as `LogicalOR`).
    // Go: internal/ast/precedence.go:OperatorPrecedenceCoalesce
    pub const COALESCE: OperatorPrecedence = OperatorPrecedence::LogicalOR;
}

bitflags::bitflags! {
    /// Flags refining how an expression's precedence is computed.
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/precedence.go:OperatorPrecedenceFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct OperatorPrecedenceFlags: i32 {
        /// No flags.
        const NONE = 0;
        /// A `new` expression without an argument list.
        const NEW_WITHOUT_ARGUMENTS = 1 << 0;
        /// The expression is part of an optional chain.
        const OPTIONAL_CHAIN = 1 << 1;
    }
}

/// Returns the precedence of a binary operator token, or `Invalid` if `kind`
/// is not a binary operator.
///
/// # Examples
/// ```
/// use tsgo_ast::Kind;
/// use tsgo_ast::precedence::{get_binary_operator_precedence, OperatorPrecedence};
/// assert_eq!(get_binary_operator_precedence(Kind::PlusToken), OperatorPrecedence::Additive);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ast/precedence.go:GetBinaryOperatorPrecedence
pub fn get_binary_operator_precedence(kind: Kind) -> OperatorPrecedence {
    match kind {
        Kind::QuestionQuestionToken => OperatorPrecedence::COALESCE,
        Kind::BarBarToken => OperatorPrecedence::LogicalOR,
        Kind::AmpersandAmpersandToken => OperatorPrecedence::LogicalAND,
        Kind::BarToken => OperatorPrecedence::BitwiseOR,
        Kind::CaretToken => OperatorPrecedence::BitwiseXOR,
        Kind::AmpersandToken => OperatorPrecedence::BitwiseAND,
        Kind::EqualsEqualsToken
        | Kind::ExclamationEqualsToken
        | Kind::EqualsEqualsEqualsToken
        | Kind::ExclamationEqualsEqualsToken => OperatorPrecedence::Equality,
        Kind::LessThanToken
        | Kind::GreaterThanToken
        | Kind::LessThanEqualsToken
        | Kind::GreaterThanEqualsToken
        | Kind::InstanceOfKeyword
        | Kind::InKeyword
        | Kind::AsKeyword
        | Kind::SatisfiesKeyword => OperatorPrecedence::Relational,
        Kind::LessThanLessThanToken
        | Kind::GreaterThanGreaterThanToken
        | Kind::GreaterThanGreaterThanGreaterThanToken => OperatorPrecedence::Shift,
        Kind::PlusToken | Kind::MinusToken => OperatorPrecedence::Additive,
        Kind::AsteriskToken | Kind::SlashToken | Kind::PercentToken => {
            OperatorPrecedence::Multiplicative
        }
        Kind::AsteriskAsteriskToken => OperatorPrecedence::Exponentiation,
        _ => OperatorPrecedence::Invalid,
    }
}

/// Returns the precedence of an operator given the containing node kind, the
/// operator kind, and refining flags.
///
/// Side effects: none (pure).
// Go: internal/ast/precedence.go:GetOperatorPrecedence
pub fn get_operator_precedence(
    node_kind: Kind,
    operator_kind: Kind,
    flags: OperatorPrecedenceFlags,
) -> OperatorPrecedence {
    match node_kind {
        Kind::SpreadElement => OperatorPrecedence::Spread,
        Kind::YieldExpression => OperatorPrecedence::Yield,
        Kind::ArrowFunction => OperatorPrecedence::Assignment,
        Kind::ConditionalExpression => OperatorPrecedence::Conditional,
        Kind::BinaryExpression => match operator_kind {
            Kind::CommaToken => OperatorPrecedence::Comma,
            Kind::EqualsToken
            | Kind::PlusEqualsToken
            | Kind::MinusEqualsToken
            | Kind::AsteriskAsteriskEqualsToken
            | Kind::AsteriskEqualsToken
            | Kind::SlashEqualsToken
            | Kind::PercentEqualsToken
            | Kind::LessThanLessThanEqualsToken
            | Kind::GreaterThanGreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
            | Kind::AmpersandEqualsToken
            | Kind::CaretEqualsToken
            | Kind::BarEqualsToken
            | Kind::BarBarEqualsToken
            | Kind::AmpersandAmpersandEqualsToken
            | Kind::QuestionQuestionEqualsToken => OperatorPrecedence::Assignment,
            _ => get_binary_operator_precedence(operator_kind),
        },
        Kind::TypeAssertionExpression
        | Kind::NonNullExpression
        | Kind::PrefixUnaryExpression
        | Kind::TypeOfExpression
        | Kind::VoidExpression
        | Kind::DeleteExpression
        | Kind::AwaitExpression => OperatorPrecedence::Unary,
        Kind::PostfixUnaryExpression => OperatorPrecedence::Update,
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression | Kind::CallExpression => {
            if flags.intersects(OperatorPrecedenceFlags::OPTIONAL_CHAIN) {
                OperatorPrecedence::OptionalChain
            } else {
                OperatorPrecedence::Member
            }
        }
        Kind::NewExpression => {
            if flags.intersects(OperatorPrecedenceFlags::NEW_WITHOUT_ARGUMENTS) {
                OperatorPrecedence::LeftHandSide
            } else {
                OperatorPrecedence::Member
            }
        }
        Kind::TaggedTemplateExpression | Kind::MetaProperty | Kind::ExpressionWithTypeArguments => {
            OperatorPrecedence::Member
        }
        Kind::AsExpression | Kind::SatisfiesExpression => OperatorPrecedence::Relational,
        Kind::ThisKeyword
        | Kind::SuperKeyword
        | Kind::ImportKeyword
        | Kind::Identifier
        | Kind::PrivateIdentifier
        | Kind::NullKeyword
        | Kind::TrueKeyword
        | Kind::FalseKeyword
        | Kind::NumericLiteral
        | Kind::BigIntLiteral
        | Kind::StringLiteral
        | Kind::ArrayLiteralExpression
        | Kind::ObjectLiteralExpression
        | Kind::FunctionExpression
        | Kind::ClassExpression
        | Kind::RegularExpressionLiteral
        | Kind::NoSubstitutionTemplateLiteral
        | Kind::TemplateExpression
        | Kind::OmittedExpression
        | Kind::JsxElement
        | Kind::JsxSelfClosingElement
        | Kind::JsxFragment
        | Kind::MissingDeclaration => OperatorPrecedence::Primary,
        Kind::ParenthesizedExpression => OperatorPrecedence::Parentheses,
        _ => OperatorPrecedence::Invalid,
    }
}

/// Returns the operator kind that determines `id`'s precedence.
///
/// Side effects: none (pure).
// Go: internal/ast/precedence.go:getOperator
pub fn get_operator(arena: &NodeArena, id: NodeId) -> Kind {
    match arena.data(id) {
        NodeData::BinaryExpression(d) => arena.kind(d.operator_token),
        NodeData::PrefixUnaryExpression(d) | NodeData::PostfixUnaryExpression(d) => d.operator,
        _ => arena.kind(id),
    }
}

/// Returns the precedence of the expression node `id`.
///
/// Side effects: none (pure).
// Go: internal/ast/precedence.go:GetExpressionPrecedence
pub fn get_expression_precedence(arena: &NodeArena, id: NodeId) -> OperatorPrecedence {
    let kind = arena.kind(id);
    let operator = get_operator(arena, id);
    let mut flags = OperatorPrecedenceFlags::NONE;
    if kind == Kind::NewExpression && new_expression_has_no_arguments(arena, id) {
        flags = OperatorPrecedenceFlags::NEW_WITHOUT_ARGUMENTS;
    } else if is_optional_chain(arena, id) {
        flags = OperatorPrecedenceFlags::OPTIONAL_CHAIN;
    }
    get_operator_precedence(kind, operator, flags)
}

/// Reports whether a `new` expression node has no argument list.
fn new_expression_has_no_arguments(arena: &NodeArena, id: NodeId) -> bool {
    matches!(arena.data(id), NodeData::NewExpression(d) if d.arguments.is_none())
}

/// Reports whether `id` is (the root of) an optional chain.
// Go: internal/ast/utilities.go:IsOptionalChain
fn is_optional_chain(arena: &NodeArena, id: NodeId) -> bool {
    if !arena.flags(id).intersects(NodeFlags::OPTIONAL_CHAIN) {
        return false;
    }
    matches!(
        arena.kind(id),
        Kind::PropertyAccessExpression
            | Kind::ElementAccessExpression
            | Kind::CallExpression
            | Kind::NonNullExpression
    )
}

#[cfg(test)]
#[path = "precedence_test.rs"]
mod tests;
