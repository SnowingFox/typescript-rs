//! The formatting context the rule predicates consult.
//!
//! Port of Go `internal/format/context.go` (the `FormattingContext` type and its
//! line-relationship accessors) plus `internal/format/api.go`'s
//! `FormatRequestKind`.
//!
//! # Projection model (documented divergence)
//!
//! Go's `FormattingContext` holds raw `*ast.Node` pointers
//! (`currentTokenSpan`/`nextTokenSpan`/`contextNode`/`currentTokenParent`/
//! `nextTokenParent`) and the predicates call accessors on them. As explained in
//! the crate root, the Rust `tsgo_ast` arena can't be threaded that way through a
//! recursive worker, so this [`FormattingContext`] instead stores the
//! *projection* of the AST the predicates read: token/node kinds, a few derived
//! booleans Go computes lazily from positions/accessors, and the option values.
//! Each predicate body still translates 1:1.

use crate::format_code_settings::FormatCodeSettings;
use tsgo_ast::Kind;

/// Which formatting operation triggered this run.
///
/// Mirrors Go's `FormatRequestKind` iota enum (defined in `api.go`).
///
/// # Examples
/// ```
/// use tsgo_format::context::FormatRequestKind;
/// assert_eq!(FormatRequestKind::FormatDocument as i32, 0);
/// assert_eq!(FormatRequestKind::FormatOnClosingCurlyBrace as i32, 5);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/format/api.go:FormatRequestKind
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
#[repr(i32)]
pub enum FormatRequestKind {
    /// Format the whole document (`FormatRequestKindFormatDocument`), default.
    #[default]
    FormatDocument = 0,
    /// Format a selection (`FormatRequestKindFormatSelection`).
    FormatSelection = 1,
    /// Format on Enter (`FormatRequestKindFormatOnEnter`).
    FormatOnEnter = 2,
    /// Format on `;` (`FormatRequestKindFormatOnSemicolon`).
    FormatOnSemicolon = 3,
    /// Format on `{` (`FormatRequestKindFormatOnOpeningCurlyBrace`).
    FormatOnOpeningCurlyBrace = 4,
    /// Format on `}` (`FormatRequestKindFormatOnClosingCurlyBrace`).
    FormatOnClosingCurlyBrace = 5,
}

/// A predicate that narrows the context in which a rule applies.
///
/// Mirrors Go's `contextPredicate = func(ctx *FormattingContext) bool`. The
/// `Send + Sync` bound lets the built rules map be cached in a `OnceLock`; all
/// predicates close over `Copy` data (kinds / option selectors), so the bound is
/// always satisfiable.
///
/// Side effects: none (pure).
// Go: internal/format/rule.go:contextPredicate
pub type ContextPredicate = Box<dyn Fn(&FormattingContext) -> bool + Send + Sync>;

/// Returns the empty predicate list (`anyContext`): a rule with no predicates
/// applies to any matching token pair.
///
/// # Examples
/// ```
/// use tsgo_format::context::any_context;
/// assert!(any_context().is_empty());
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rule.go:anyContext
pub fn any_context() -> Vec<ContextPredicate> {
    Vec::new()
}

/// The projected context two adjacent tokens are evaluated in.
///
/// See the module-level note on the projection model. Fields mirror the data the
/// Go predicates read off `*ast.Node`s; the AST-walking worker (deferred) is
/// responsible for populating them (mirroring Go's `UpdateContext`).
///
/// Side effects: none (pure value type).
// Go: internal/format/context.go:FormattingContext
#[derive(Clone, Debug)]
pub struct FormattingContext {
    /// The kind of the left/current token (`currentTokenSpan.Kind`).
    pub current_token_kind: Kind,
    /// The kind of the right/next token (`nextTokenSpan.Kind`).
    pub next_token_kind: Kind,
    /// The kind of the common parent node (`contextNode.Kind`).
    pub context_node_kind: Kind,
    /// The kind of the current token's parent (`currentTokenParent.Kind`).
    pub current_token_parent_kind: Kind,
    /// The kind of the next token's parent (`nextTokenParent.Kind`).
    pub next_token_parent_kind: Kind,
    /// The kind of the current token's grandparent, if any
    /// (`currentTokenParent.Parent.Kind`).
    pub current_token_parent_parent_kind: Option<Kind>,
    /// The kind of the next token's grandparent, if any
    /// (`nextTokenParent.Parent.Kind`).
    pub next_token_parent_parent_kind: Option<Kind>,

    /// When `context_node_kind` is `BinaryExpression`, the operator token kind
    /// (`contextNode.AsBinaryExpression().OperatorToken.Kind`).
    pub binary_operator_token_kind: Kind,
    /// Whether the context node has a question token
    /// (`ast.HasQuestionToken(contextNode)`).
    pub context_node_has_question_token: bool,
    /// Whether the context node has an expression child
    /// (`contextNode.Expression() != nil`), used by the yield rule.
    pub context_node_has_expression: bool,
    /// Whether the context node has decorators (`ast.HasDecorators(contextNode)`).
    pub context_node_has_decorators: bool,
    /// Precomputed `isStartOfVariableDeclarationList` (Go derives it from
    /// `scanner.GetTokenPosOfNode`; the projection carries the result).
    pub current_token_is_start_of_variable_declaration_list: bool,
    /// Precomputed "context node is a property access whose expression is a
    /// numeric literal without a dot" (drives
    /// `isNotPropertyAccessOnIntegerLiteral`).
    pub context_node_is_property_access_on_integer_literal: bool,

    /// `tokensAreOnSameLine` cache, exposed via [`Self::tokens_are_on_same_line`].
    pub tokens_are_on_same_line: bool,
    /// `contextNodeAllOnSameLine` cache.
    pub context_node_all_on_same_line: bool,
    /// `nextNodeAllOnSameLine` cache.
    pub next_node_all_on_same_line: bool,
    /// `contextNodeBlockIsOnOneLine` cache.
    pub context_node_block_is_on_one_line: bool,
    /// `nextNodeBlockIsOnOneLine` cache.
    pub next_node_block_is_on_one_line: bool,

    /// The active formatting request kind (`FormattingRequestKind`).
    pub formatting_request_kind: FormatRequestKind,
    /// The formatter options (`Options`).
    pub options: FormatCodeSettings,
}

impl FormattingContext {
    /// Creates a context with the given `options` and request `kind`; all
    /// node/line projection fields start empty (kinds `Unknown`, flags false).
    ///
    /// Mirrors Go's `NewFormattingContext`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_format::context::{FormattingContext, FormatRequestKind};
    /// use tsgo_format::format_code_settings::get_default_format_code_settings;
    /// let ctx = FormattingContext::new(
    ///     get_default_format_code_settings(),
    ///     FormatRequestKind::FormatDocument,
    /// );
    /// assert_eq!(ctx.current_token_kind, tsgo_ast::Kind::Unknown);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/format/context.go:NewFormattingContext
    pub fn new(options: FormatCodeSettings, kind: FormatRequestKind) -> FormattingContext {
        FormattingContext {
            current_token_kind: Kind::Unknown,
            next_token_kind: Kind::Unknown,
            context_node_kind: Kind::Unknown,
            current_token_parent_kind: Kind::Unknown,
            next_token_parent_kind: Kind::Unknown,
            current_token_parent_parent_kind: None,
            next_token_parent_parent_kind: None,
            binary_operator_token_kind: Kind::Unknown,
            context_node_has_question_token: false,
            context_node_has_expression: false,
            context_node_has_decorators: false,
            current_token_is_start_of_variable_declaration_list: false,
            context_node_is_property_access_on_integer_literal: false,
            tokens_are_on_same_line: false,
            context_node_all_on_same_line: false,
            next_node_all_on_same_line: false,
            context_node_block_is_on_one_line: false,
            next_node_block_is_on_one_line: false,
            formatting_request_kind: kind,
            options,
        }
    }

    /// Whether the current and next tokens are on the same source line.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/context.go:TokensAreOnSameLine
    pub fn tokens_are_on_same_line(&self) -> bool {
        self.tokens_are_on_same_line
    }

    /// Whether the entire context node is on one line.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/context.go:ContextNodeAllOnSameLine
    pub fn context_node_all_on_same_line(&self) -> bool {
        self.context_node_all_on_same_line
    }

    /// Whether the entire next-token parent node is on one line.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/context.go:NextNodeAllOnSameLine
    pub fn next_node_all_on_same_line(&self) -> bool {
        self.next_node_all_on_same_line
    }

    /// Whether the context node's brace block is on one line.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/context.go:ContextNodeBlockIsOnOneLine
    pub fn context_node_block_is_on_one_line(&self) -> bool {
        self.context_node_block_is_on_one_line
    }

    /// Whether the next-token parent node's brace block is on one line.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/context.go:NextNodeBlockIsOnOneLine
    pub fn next_node_block_is_on_one_line(&self) -> bool {
        self.next_node_block_is_on_one_line
    }
}

#[cfg(test)]
#[path = "context_test.rs"]
mod tests;
