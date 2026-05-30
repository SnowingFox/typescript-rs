//! `tsgo_evaluator` — 1:1 Rust port of Go `internal/evaluator`.
//!
//! Evaluates the syntactically constant-foldable subset of expressions used by
//! the checker (enum member initializers, string/number literals, template
//! strings, the `+ - ~` unary operators, and the binary arithmetic/bitwise
//! operators plus string concatenation) into a [`Result`].
//!
//! Entities that need symbol resolution (`Identifier`, property access, element
//! access) are not resolved here; instead the caller supplies an
//! `evaluate_entity` callback (the checker's real, symbol-aware evaluator). The
//! evaluator itself is therefore a pure function of `(arena, expr, location)`.
//!
//! # Ownership note (port deviation)
//!
//! Go's `evaluator` is a recursive closure over `*ast.Node` pointers. Rust
//! cannot express a self-referential closure safely, so the recursion lives on
//! [`Evaluator`], a struct holding the `evaluate_entity` callback. AST access
//! goes through a [`tsgo_ast::NodeArena`] handle plus [`tsgo_ast::NodeId`]
//! indices rather than pointers (see PORTING.md §5). The callback signature
//! gains an explicit `&NodeArena` for the same reason.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};
use tsgo_core::if_else;
use tsgo_jsnum::{from_string, Number, PseudoBigInt};

/// A constant value produced by evaluation, replacing Go's `Value any`.
///
/// Go stores the result in an `any` that is in practice one of `nil`, `string`,
/// `jsnum.Number`, `bool`, or `jsnum.PseudoBigInt`. The discriminated enum makes
/// those cases explicit; [`EvalValue::None`] is Go's `nil` (not foldable). The
/// evaluator itself only ever yields `None`, `Str`, or `Num`; `Bool` and
/// `BigInt` appear only as inputs to [`any_to_string`] / [`is_truthy`] (a caller
/// may pass them in).
///
/// Side effects: none (pure value type).
// Go: internal/evaluator/evaluator.go:Result.Value
#[derive(Clone, Debug, PartialEq)]
pub enum EvalValue {
    /// Not foldable (Go's `nil`).
    None,
    /// A string value.
    Str(String),
    /// A JavaScript number value.
    Num(Number),
    /// A boolean value.
    Bool(bool),
    /// A bigint value.
    BigInt(PseudoBigInt),
}

/// The outcome of evaluating an expression: a value plus the bookkeeping flags
/// the checker uses for enum/`isolatedModules` decisions.
///
/// Side effects: none (pure value type).
// Go: internal/evaluator/evaluator.go:Result
#[derive(Clone, Debug, PartialEq)]
pub struct Result {
    /// The folded value, or [`EvalValue::None`] if not foldable.
    pub value: EvalValue,
    /// Whether the expression is a string by syntax (string literal, template,
    /// or a `+` whose operands are syntactically strings).
    pub is_syntactically_string: bool,
    /// Whether evaluating any sub-entity resolved a declaration in another file.
    pub resolved_other_files: bool,
    /// Whether evaluation crossed an external (imported) reference.
    pub has_external_references: bool,
}

/// Builds a [`Result`] from its four fields.
///
/// # Examples
/// ```
/// use tsgo_evaluator::{new_result, EvalValue};
/// let r = new_result(EvalValue::Bool(true), false, false, false);
/// assert_eq!(r.value, EvalValue::Bool(true));
/// ```
///
/// Side effects: none (pure).
// Go: internal/evaluator/evaluator.go:NewResult
pub fn new_result(
    value: EvalValue,
    is_syntactically_string: bool,
    resolved_other_files: bool,
    has_external_references: bool,
) -> Result {
    Result {
        value,
        is_syntactically_string,
        resolved_other_files,
        has_external_references,
    }
}

/// Bit set of "outer expression" kinds that evaluation transparently skips
/// before folding, mirroring Go `ast.OuterExpressionKinds`.
///
/// The evaluator always unions in [`OuterExpressionKinds::PARENTHESES`]. Only
/// the parenthesized case is honored today because the assertion-like kinds
/// (`as`, `satisfies`, non-null, ...) have no ported `NodeData` in the AST
/// subset yet; the remaining constants exist for API parity with Go.
///
/// Side effects: none (pure value type).
// Go: internal/ast/utilities.go:OuterExpressionKinds
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OuterExpressionKinds(i16);

impl OuterExpressionKinds {
    /// The empty set.
    pub const NONE: OuterExpressionKinds = OuterExpressionKinds(0);
    /// Parenthesized expressions (`(x)`).
    pub const PARENTHESES: OuterExpressionKinds = OuterExpressionKinds(1 << 0);
    /// `<T>x` / `x as T` type assertions.
    pub const TYPE_ASSERTIONS: OuterExpressionKinds = OuterExpressionKinds(1 << 1);
    /// `x!` non-null assertions.
    pub const NON_NULL_ASSERTIONS: OuterExpressionKinds = OuterExpressionKinds(1 << 2);
    /// Partially emitted expressions (a transform artifact).
    pub const PARTIALLY_EMITTED_EXPRESSIONS: OuterExpressionKinds = OuterExpressionKinds(1 << 3);
    /// `x<T>` expressions with type arguments.
    pub const EXPRESSIONS_WITH_TYPE_ARGUMENTS: OuterExpressionKinds = OuterExpressionKinds(1 << 4);
    /// `x satisfies T` expressions.
    pub const SATISFIES: OuterExpressionKinds = OuterExpressionKinds(1 << 5);
    /// Excludes JSDoc type assertions when set.
    pub const EXCLUDE_JSDOC_TYPE_ASSERTION: OuterExpressionKinds = OuterExpressionKinds(1 << 6);
    /// `TYPE_ASSERTIONS | NON_NULL_ASSERTIONS | SATISFIES`.
    pub const ASSERTIONS: OuterExpressionKinds = OuterExpressionKinds(
        Self::TYPE_ASSERTIONS.0 | Self::NON_NULL_ASSERTIONS.0 | Self::SATISFIES.0,
    );
    /// `PARENTHESES | ASSERTIONS | PARTIALLY_EMITTED_EXPRESSIONS | EXPRESSIONS_WITH_TYPE_ARGUMENTS`.
    pub const ALL: OuterExpressionKinds = OuterExpressionKinds(
        Self::PARENTHESES.0
            | Self::ASSERTIONS.0
            | Self::PARTIALLY_EMITTED_EXPRESSIONS.0
            | Self::EXPRESSIONS_WITH_TYPE_ARGUMENTS.0,
    );

    /// Reports whether every bit of `other` is present in `self`.
    ///
    /// Side effects: none (pure).
    pub const fn contains(self, other: OuterExpressionKinds) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for OuterExpressionKinds {
    type Output = OuterExpressionKinds;
    fn bitor(self, rhs: OuterExpressionKinds) -> OuterExpressionKinds {
        OuterExpressionKinds(self.0 | rhs.0)
    }
}

/// A reusable constant-expression evaluator holding the caller's entity-resolution
/// callback, mirroring the closure returned by Go's `NewEvaluator`.
///
/// `F` is the `evaluate_entity` callback invoked for `Identifier`, property
/// access, and element access nodes that resolve to entity names. It takes the
/// arena explicitly (a port deviation; see the module docs) plus the expression
/// node and an optional location node, returning a [`Result`].
///
/// Side effects: none (pure); methods only read the supplied arena.
// Go: internal/evaluator/evaluator.go:Evaluator
pub struct Evaluator<F> {
    evaluate_entity: F,
    outer_expressions_to_skip: OuterExpressionKinds,
}

/// Builds an [`Evaluator`] from an entity-resolution callback and the set of
/// outer-expression kinds to skip.
///
/// # Examples
/// ```
/// use tsgo_ast::{NodeArena, TokenFlags};
/// use tsgo_evaluator::{new_evaluator, EvalValue, OuterExpressionKinds};
///
/// let mut arena = NodeArena::new();
/// let lit = arena.new_numeric_literal("123", TokenFlags::NONE);
/// let eval = new_evaluator(
///     |_: &NodeArena, _, _| tsgo_evaluator::new_result(EvalValue::None, false, false, false),
///     OuterExpressionKinds::NONE,
/// );
/// assert_eq!(eval.evaluate(&arena, lit, None).value, EvalValue::Num(123.0.into()));
/// ```
///
/// Side effects: none (pure).
// Go: internal/evaluator/evaluator.go:NewEvaluator
pub fn new_evaluator<F>(
    evaluate_entity: F,
    outer_expressions_to_skip: OuterExpressionKinds,
) -> Evaluator<F>
where
    F: Fn(&NodeArena, NodeId, Option<NodeId>) -> Result,
{
    Evaluator {
        evaluate_entity,
        outer_expressions_to_skip,
    }
}

impl<F> Evaluator<F>
where
    F: Fn(&NodeArena, NodeId, Option<NodeId>) -> Result,
{
    /// Evaluates `expr` (a node in `arena`) to a [`Result`], recursing into
    /// operands and delegating entity names to the stored callback.
    ///
    /// Side effects: none (pure); only reads `arena` and calls the callback.
    // Go: internal/evaluator/evaluator.go:NewEvaluator (the returned `evaluate` closure)
    pub fn evaluate(&self, arena: &NodeArena, expr: NodeId, location: Option<NodeId>) -> Result {
        let is_syntactically_string = false;
        let resolved_other_files = false;
        let has_external_references = false;
        // It's unclear when/whether we should consider skipping other kinds of
        // outer expressions; we conservatively always skip parentheses so the
        // errors we emit stay aligned with Babel's evaluation (see the Go
        // source for the full rationale).
        let expr = skip_outer_expressions(
            arena,
            expr,
            self.outer_expressions_to_skip | OuterExpressionKinds::PARENTHESES,
        );
        match arena.kind(expr) {
            Kind::PrefixUnaryExpression => {
                let (operator, operand) = match arena.data(expr) {
                    NodeData::PrefixUnaryExpression(d) => (d.operator, d.operand),
                    _ => unreachable!("kind PrefixUnaryExpression implies that data"),
                };
                let result = self.evaluate(arena, operand, location);
                let resolved_other_files = result.resolved_other_files;
                let has_external_references = result.has_external_references;
                if let EvalValue::Num(value) = result.value {
                    match operator {
                        Kind::PlusToken => {
                            return Result {
                                value: EvalValue::Num(value),
                                is_syntactically_string,
                                resolved_other_files,
                                has_external_references,
                            };
                        }
                        Kind::MinusToken => {
                            return Result {
                                value: EvalValue::Num(Number::from(-f64::from(value))),
                                is_syntactically_string,
                                resolved_other_files,
                                has_external_references,
                            };
                        }
                        Kind::TildeToken => {
                            return Result {
                                value: EvalValue::Num(value.bitwise_not()),
                                is_syntactically_string,
                                resolved_other_files,
                                has_external_references,
                            };
                        }
                        _ => {}
                    }
                }
                Result {
                    value: EvalValue::None,
                    is_syntactically_string,
                    resolved_other_files,
                    has_external_references,
                }
            }
            Kind::BinaryExpression => {
                let (left_id, operator_token, right_id) = match arena.data(expr) {
                    NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
                    _ => unreachable!("kind BinaryExpression implies that data"),
                };
                let left = self.evaluate(arena, left_id, location);
                let right = self.evaluate(arena, right_id, location);
                let operator = arena.kind(operator_token);
                let is_syntactically_string = (left.is_syntactically_string
                    || right.is_syntactically_string)
                    && operator == Kind::PlusToken;
                let resolved_other_files = left.resolved_other_files || right.resolved_other_files;
                let has_external_references =
                    left.has_external_references || right.has_external_references;
                let left_num = match left.value {
                    EvalValue::Num(n) => Some(n),
                    _ => None,
                };
                let right_num = match right.value {
                    EvalValue::Num(n) => Some(n),
                    _ => None,
                };
                if let (Some(l), Some(r)) = (left_num, right_num) {
                    let folded = match operator {
                        Kind::BarToken => Some(l.bitwise_or(r)),
                        Kind::AmpersandToken => Some(l.bitwise_and(r)),
                        Kind::GreaterThanGreaterThanToken => Some(l.signed_right_shift(r)),
                        Kind::GreaterThanGreaterThanGreaterThanToken => {
                            Some(l.unsigned_right_shift(r))
                        }
                        Kind::LessThanLessThanToken => Some(l.left_shift(r)),
                        Kind::CaretToken => Some(l.bitwise_xor(r)),
                        Kind::AsteriskToken => Some(Number::from(f64::from(l) * f64::from(r))),
                        Kind::SlashToken => Some(Number::from(f64::from(l) / f64::from(r))),
                        Kind::PlusToken => Some(Number::from(f64::from(l) + f64::from(r))),
                        Kind::MinusToken => Some(Number::from(f64::from(l) - f64::from(r))),
                        Kind::PercentToken => Some(l.remainder(r)),
                        Kind::AsteriskAsteriskToken => Some(l.exponentiate(r)),
                        _ => None,
                    };
                    if let Some(value) = folded {
                        return Result {
                            value: EvalValue::Num(value),
                            is_syntactically_string,
                            resolved_other_files,
                            has_external_references,
                        };
                    }
                }
                let left_str = match &left.value {
                    EvalValue::Str(s) => Some(s.clone()),
                    _ => None,
                };
                let right_str = match &right.value {
                    EvalValue::Str(s) => Some(s.clone()),
                    _ => None,
                };
                if (left_str.is_some() || left_num.is_some())
                    && (right_str.is_some() || right_num.is_some())
                    && operator == Kind::PlusToken
                {
                    // When a side is a number, stringify it with JS `toString`;
                    // otherwise it is guaranteed (by the guard) to be a string.
                    let l = match left_num {
                        Some(n) => n.to_string(),
                        None => left_str.unwrap_or_default(),
                    };
                    let r = match right_num {
                        Some(n) => n.to_string(),
                        None => right_str.unwrap_or_default(),
                    };
                    return Result {
                        value: EvalValue::Str(format!("{l}{r}")),
                        is_syntactically_string,
                        resolved_other_files,
                        has_external_references,
                    };
                }
                Result {
                    value: EvalValue::None,
                    is_syntactically_string,
                    resolved_other_files,
                    has_external_references,
                }
            }
            // Go also has a `KindTemplateExpression` arm calling
            // `evaluateTemplateExpression`. The template `NodeData`
            // (TemplateExpression / TemplateSpan / NoSubstitutionTemplateLiteral)
            // is not in the ported AST subset yet, so template inputs fall
            // through to the `None` default below.
            // DEFER(phase-4): template-expression folding; blocked-by: tsgo_ast template NodeData not yet ported.
            //
            // `NoSubstitutionTemplateLiteral` is grouped with `StringLiteral` in
            // Go via `expr.Text()`; for the same reason only `StringLiteral`
            // reaches this arm in practice.
            Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral => Result {
                value: EvalValue::Str(arena.text(expr).to_string()),
                is_syntactically_string: true,
                resolved_other_files: false,
                has_external_references: false,
            },
            Kind::NumericLiteral => Result {
                value: EvalValue::Num(from_string(arena.text(expr))),
                is_syntactically_string: false,
                resolved_other_files: false,
                has_external_references: false,
            },
            Kind::Identifier => (self.evaluate_entity)(arena, expr, location),
            Kind::ElementAccessExpression | Kind::PropertyAccessExpression => {
                if expression_of(arena, expr)
                    .is_some_and(|inner| is_entity_name_expression(arena, inner))
                {
                    return (self.evaluate_entity)(arena, expr, location);
                }
                Result {
                    value: EvalValue::None,
                    is_syntactically_string,
                    resolved_other_files,
                    has_external_references,
                }
            }
            _ => Result {
                value: EvalValue::None,
                is_syntactically_string,
                resolved_other_files,
                has_external_references,
            },
        }
    }
}

/// Descends through any leading outer expressions of the given `kinds`,
/// returning the first inner node that is not one.
///
/// Only parenthesized expressions are currently skippable (the only ported
/// outer-expression `NodeData`); other kinds in `kinds` are inert.
// Go: internal/ast/utilities.go:SkipOuterExpressions
fn skip_outer_expressions(
    arena: &NodeArena,
    mut node: NodeId,
    kinds: OuterExpressionKinds,
) -> NodeId {
    while is_outer_expression(arena, node, kinds) {
        match expression_of(arena, node) {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

// Go: internal/ast/utilities.go:IsOuterExpression
fn is_outer_expression(arena: &NodeArena, node: NodeId, kinds: OuterExpressionKinds) -> bool {
    match arena.kind(node) {
        Kind::ParenthesizedExpression => kinds.contains(OuterExpressionKinds::PARENTHESES),
        _ => false,
    }
}

/// Returns the `.expression` child of the node kinds this crate inspects
/// (parenthesized, property access, element access), or `None` otherwise.
// Go: internal/ast/ast.go:Node.Expression
fn expression_of(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::ParenthesizedExpression(d) => Some(d.expression),
        NodeData::PropertyAccessExpression(d) => Some(d.expression),
        NodeData::ElementAccessExpression(d) => Some(d.expression),
        _ => None,
    }
}

/// Returns the `.name` child of a property access node, or `None` otherwise.
// Go: internal/ast/ast.go:Node.Name
fn name_of(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::PropertyAccessExpression(d) => Some(d.name),
        _ => None,
    }
}

/// Reports whether `node` is an entity-name expression (`a`, `a.b`, `a.b.c`),
/// i.e. an identifier or a property access whose name is an identifier and
/// whose object is itself an entity name.
///
/// Ports the `allowJS = false` path of Go's `IsEntityNameExpression`; the
/// `this`/element-access JS branches are omitted (they require AST kinds not in
/// the ported subset and are not reachable here).
// Go: internal/ast/utilities.go:IsEntityNameExpression
fn is_entity_name_expression(arena: &NodeArena, node: NodeId) -> bool {
    use tsgo_ast::utilities::{is_identifier, is_property_access_expression};
    is_identifier(arena, node)
        || (is_property_access_expression(arena, node)
            && name_of(arena, node).is_some_and(|n| is_identifier(arena, n))
            && expression_of(arena, node).is_some_and(|e| is_entity_name_expression(arena, e)))
}

/// Converts a folded value to its string form, as the template-string and
/// concatenation paths require.
///
/// # Examples
/// ```
/// use tsgo_evaluator::{any_to_string, EvalValue};
/// assert_eq!(any_to_string(&EvalValue::Str("x".to_string())), "x");
/// ```
///
/// # Panics
/// Panics on [`EvalValue::None`] (Go panics on an unhandled `any`).
///
/// Side effects: none (pure).
// Go: internal/evaluator/evaluator.go:AnyToString
pub fn any_to_string(v: &EvalValue) -> String {
    match v {
        EvalValue::Str(s) => s.clone(),
        EvalValue::Num(n) => n.to_string(),
        EvalValue::Bool(b) => if_else(*b, "true", "false").to_string(),
        EvalValue::BigInt(b) => b.to_string(),
        EvalValue::None => panic!("Unhandled case in AnyToString"),
    }
}

/// Reports whether a folded value is truthy by JavaScript's rules.
///
/// # Examples
/// ```
/// use tsgo_evaluator::{is_truthy, EvalValue};
/// assert!(is_truthy(&EvalValue::Str("a".to_string())));
/// assert!(!is_truthy(&EvalValue::Str(String::new())));
/// ```
///
/// # Panics
/// Panics on [`EvalValue::None`] (Go panics on an unhandled `any`).
///
/// Side effects: none (pure).
// Go: internal/evaluator/evaluator.go:IsTruthy
pub fn is_truthy(v: &EvalValue) -> bool {
    match v {
        EvalValue::Str(s) => !s.is_empty(),
        EvalValue::Num(n) => *n != Number::from(0.0) && !n.is_nan(),
        EvalValue::Bool(b) => *b,
        EvalValue::BigInt(b) => *b != PseudoBigInt::default(),
        EvalValue::None => panic!("Unhandled case in IsTruthy"),
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
