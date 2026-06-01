//! The rule context predicates.
//!
//! 1:1 port of Go `internal/format/rulecontext.go`: the option selectors, the
//! higher-order predicate builders, and the `contextPredicate`s that narrow when
//! a rule applies. All read the [`FormattingContext`] projection (see
//! `context.rs`); each Go body translates directly
//! (`context.contextNode.Kind == ast.KindForStatement` becomes
//! `ctx.context_node_kind == Kind::ForStatement`).
//!
//! Items are `pub(crate)` (consumed by the rules table in `rules.rs`), not part
//! of the crate's public API.
//!
//! # Deferred predicates (return a documented, default-safe value)
//!
//! - `isSemicolonDeletionContext` / `isSemicolonInsertionContext`: need
//!   `astnav.FindNextToken`/`FindPrecedingToken` and
//!   `lsutil.PositionIsASICandidate` (deferred in `tsgo_ls_lsutil`). They gate
//!   only the `Semicolons == Remove`/`Insert` rules, which are off by default,
//!   so they return `false`. blocked-by: astnav navigation + ASI port.
//! - `isEndOfDecoratorContextOnSameLine`: needs an `IsExpression` parent walk.
//!   Gates only the decorator-spacing rule (decorators are deferred per the task
//!   scope); returns `false`. blocked-by: arena parent walk.

use crate::context::{ContextPredicate, FormattingContext};
use crate::format_code_settings::{FormatCodeSettings, SemicolonPreference};
use tsgo_ast::Kind;
use tsgo_core::tristate::Tristate;

//
// Option selectors (Go: optionSelector / anyOptionSelector)
//

// Go: internal/format/rulecontext.go:semicolonOption
pub(crate) fn semicolon_option(options: &FormatCodeSettings) -> SemicolonPreference {
    options.semicolons
}

// Go: internal/format/rulecontext.go:insertSpaceAfterCommaDelimiterOption
pub(crate) fn insert_space_after_comma_delimiter_option(options: &FormatCodeSettings) -> Tristate {
    options.insert_space_after_comma_delimiter
}

// Go: internal/format/rulecontext.go:insertSpaceAfterSemicolonInForStatementsOption
pub(crate) fn insert_space_after_semicolon_in_for_statements_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_semicolon_in_for_statements
}

// Go: internal/format/rulecontext.go:insertSpaceBeforeAndAfterBinaryOperatorsOption
pub(crate) fn insert_space_before_and_after_binary_operators_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_before_and_after_binary_operators
}

// Go: internal/format/rulecontext.go:insertSpaceAfterConstructorOption
pub(crate) fn insert_space_after_constructor_option(options: &FormatCodeSettings) -> Tristate {
    options.insert_space_after_constructor
}

// Go: internal/format/rulecontext.go:insertSpaceAfterKeywordsInControlFlowStatementsOption
pub(crate) fn insert_space_after_keywords_in_control_flow_statements_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_keywords_in_control_flow_statements
}

// Go: internal/format/rulecontext.go:insertSpaceAfterFunctionKeywordForAnonymousFunctionsOption
pub(crate) fn insert_space_after_function_keyword_for_anonymous_functions_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_function_keyword_for_anonymous_functions
}

// Go: internal/format/rulecontext.go:insertSpaceAfterOpeningAndBeforeClosingNonemptyParenthesisOption
pub(crate) fn insert_space_after_opening_and_before_closing_nonempty_parenthesis_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_opening_and_before_closing_nonempty_parenthesis
}

// Go: internal/format/rulecontext.go:insertSpaceAfterOpeningAndBeforeClosingNonemptyBracketsOption
pub(crate) fn insert_space_after_opening_and_before_closing_nonempty_brackets_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_opening_and_before_closing_nonempty_brackets
}

// Go: internal/format/rulecontext.go:insertSpaceAfterOpeningAndBeforeClosingNonemptyBracesOption
pub(crate) fn insert_space_after_opening_and_before_closing_nonempty_braces_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_opening_and_before_closing_nonempty_braces
}

// Go: internal/format/rulecontext.go:insertSpaceAfterOpeningAndBeforeClosingEmptyBracesOption
pub(crate) fn insert_space_after_opening_and_before_closing_empty_braces_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_opening_and_before_closing_empty_braces
}

// Go: internal/format/rulecontext.go:insertSpaceAfterOpeningAndBeforeClosingTemplateStringBracesOption
pub(crate) fn insert_space_after_opening_and_before_closing_template_string_braces_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_opening_and_before_closing_template_string_braces
}

// Go: internal/format/rulecontext.go:insertSpaceAfterOpeningAndBeforeClosingJsxExpressionBracesOption
pub(crate) fn insert_space_after_opening_and_before_closing_jsx_expression_braces_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_after_opening_and_before_closing_jsx_expression_braces
}

// Go: internal/format/rulecontext.go:insertSpaceAfterTypeAssertionOption
pub(crate) fn insert_space_after_type_assertion_option(options: &FormatCodeSettings) -> Tristate {
    options.insert_space_after_type_assertion
}

// Go: internal/format/rulecontext.go:insertSpaceBeforeFunctionParenthesisOption
pub(crate) fn insert_space_before_function_parenthesis_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.insert_space_before_function_parenthesis
}

// Go: internal/format/rulecontext.go:placeOpenBraceOnNewLineForFunctionsOption
pub(crate) fn place_open_brace_on_new_line_for_functions_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.place_open_brace_on_new_line_for_functions
}

// Go: internal/format/rulecontext.go:placeOpenBraceOnNewLineForControlBlocksOption
pub(crate) fn place_open_brace_on_new_line_for_control_blocks_option(
    options: &FormatCodeSettings,
) -> Tristate {
    options.place_open_brace_on_new_line_for_control_blocks
}

// Go: internal/format/rulecontext.go:insertSpaceBeforeTypeAnnotationOption
pub(crate) fn insert_space_before_type_annotation_option(options: &FormatCodeSettings) -> Tristate {
    options.insert_space_before_type_annotation
}

//
// Higher-order predicate builders (Go: optionEquals/isOptionEnabled/...)
//

// Go: internal/format/rulecontext.go:optionEquals
pub(crate) fn option_equals<T: PartialEq + Copy + Send + Sync + 'static>(
    option_name: fn(&FormatCodeSettings) -> T,
    option_value: T,
) -> ContextPredicate {
    Box::new(move |context| option_name(&context.options) == option_value)
}

// Go: internal/format/rulecontext.go:isOptionEnabled
pub(crate) fn is_option_enabled(
    option_name: fn(&FormatCodeSettings) -> Tristate,
) -> ContextPredicate {
    Box::new(move |context| option_name(&context.options).is_true())
}

// Go: internal/format/rulecontext.go:isOptionDisabled
pub(crate) fn is_option_disabled(
    option_name: fn(&FormatCodeSettings) -> Tristate,
) -> ContextPredicate {
    Box::new(move |context| option_name(&context.options).is_false())
}

// Go: internal/format/rulecontext.go:isOptionDisabledOrUndefined
pub(crate) fn is_option_disabled_or_undefined(
    option_name: fn(&FormatCodeSettings) -> Tristate,
) -> ContextPredicate {
    Box::new(move |context| option_name(&context.options).is_false_or_unknown())
}

// Go: internal/format/rulecontext.go:isOptionDisabledOrUndefinedOrTokensOnSameLine
pub(crate) fn is_option_disabled_or_undefined_or_tokens_on_same_line(
    option_name: fn(&FormatCodeSettings) -> Tristate,
) -> ContextPredicate {
    Box::new(move |context| {
        option_name(&context.options).is_false_or_unknown() || context.tokens_are_on_same_line()
    })
}

// Go: internal/format/rulecontext.go:isOptionEnabledOrUndefined
pub(crate) fn is_option_enabled_or_undefined(
    option_name: fn(&FormatCodeSettings) -> Tristate,
) -> ContextPredicate {
    Box::new(move |context| option_name(&context.options).is_true_or_unknown())
}

//
// Kind-level helpers (ported from `ast` for the projection)
//

// Go: internal/ast/utilities.go:isFunctionLikeDeclarationKind
fn is_function_like_declaration_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::FunctionExpression
            | Kind::ArrowFunction
    )
}

// Go: internal/ast/utilities.go:IsFunctionLikeKind
fn is_function_like_kind(kind: Kind) -> bool {
    if matches!(
        kind,
        Kind::MethodSignature
            | Kind::CallSignature
            | Kind::JSDocSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::FunctionType
            | Kind::ConstructorType
    ) {
        return true;
    }
    is_function_like_declaration_kind(kind)
}

//
// Context predicates (Go: rulecontext.go)
//

// Go: internal/format/rulecontext.go:isForContext
pub(crate) fn is_for_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ForStatement
}

// Go: internal/format/rulecontext.go:isNotForContext
pub(crate) fn is_not_for_context(context: &FormattingContext) -> bool {
    !is_for_context(context)
}

// Go: internal/format/rulecontext.go:isBinaryOpContext
pub(crate) fn is_binary_op_context(context: &FormattingContext) -> bool {
    match context.context_node_kind {
        Kind::BinaryExpression => context.binary_operator_token_kind != Kind::CommaToken,
        Kind::ConditionalExpression
        | Kind::ConditionalType
        | Kind::AsExpression
        | Kind::ExportSpecifier
        | Kind::ImportSpecifier
        | Kind::TypePredicate
        | Kind::UnionType
        | Kind::IntersectionType
        | Kind::SatisfiesExpression => true,

        // equals in binding elements, type aliases, import equals, export
        // assignment, variable declaration, parameters, enum members, property
        // (declaration|signature)
        Kind::BindingElement
        | Kind::TypeAliasDeclaration
        | Kind::ImportEqualsDeclaration
        | Kind::ExportAssignment
        | Kind::VariableDeclaration
        | Kind::Parameter
        | Kind::EnumMember
        | Kind::PropertyDeclaration
        | Kind::PropertySignature => {
            context.current_token_kind == Kind::EqualsToken
                || context.next_token_kind == Kind::EqualsToken
        }
        // "in" keyword in for-in / mapped type parameter
        Kind::ForInStatement | Kind::TypeParameter => {
            context.current_token_kind == Kind::InKeyword
                || context.next_token_kind == Kind::InKeyword
                || context.current_token_kind == Kind::EqualsToken
                || context.next_token_kind == Kind::EqualsToken
        }
        // "of" is formatted like "in"
        Kind::ForOfStatement => {
            context.current_token_kind == Kind::OfKeyword
                || context.next_token_kind == Kind::OfKeyword
        }
        _ => false,
    }
}

// Go: internal/format/rulecontext.go:isNotBinaryOpContext
pub(crate) fn is_not_binary_op_context(context: &FormattingContext) -> bool {
    !is_binary_op_context(context)
}

// Go: internal/format/rulecontext.go:isNotTypeAnnotationContext
pub(crate) fn is_not_type_annotation_context(context: &FormattingContext) -> bool {
    !is_type_annotation_context(context)
}

// Go: internal/format/rulecontext.go:isTypeAnnotationContext
pub(crate) fn is_type_annotation_context(context: &FormattingContext) -> bool {
    let context_kind = context.context_node_kind;
    context_kind == Kind::PropertyDeclaration
        || context_kind == Kind::PropertySignature
        || context_kind == Kind::Parameter
        || context_kind == Kind::VariableDeclaration
        || is_function_like_kind(context_kind)
}

// Go: internal/format/rulecontext.go:isOptionalPropertyContext
pub(crate) fn is_optional_property_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::PropertyDeclaration
        && context.context_node_has_question_token
}

// Go: internal/format/rulecontext.go:isNonOptionalPropertyContext
pub(crate) fn is_non_optional_property_context(context: &FormattingContext) -> bool {
    !is_optional_property_context(context)
}

// Go: internal/format/rulecontext.go:isConditionalOperatorContext
pub(crate) fn is_conditional_operator_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ConditionalExpression
        || context.context_node_kind == Kind::ConditionalType
}

// Go: internal/format/rulecontext.go:isSameLineTokenOrBeforeBlockContext
pub(crate) fn is_same_line_token_or_before_block_context(context: &FormattingContext) -> bool {
    context.tokens_are_on_same_line() || is_before_block_context(context)
}

// Go: internal/format/rulecontext.go:isBraceWrappedContext
pub(crate) fn is_brace_wrapped_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ObjectBindingPattern
        || context.context_node_kind == Kind::MappedType
        || is_single_line_block_context(context)
}

// Go: internal/format/rulecontext.go:isBeforeMultilineBlockContext
pub(crate) fn is_before_multiline_block_context(context: &FormattingContext) -> bool {
    is_before_block_context(context)
        && !(context.next_node_all_on_same_line() || context.next_node_block_is_on_one_line())
}

// Go: internal/format/rulecontext.go:isMultilineBlockContext
pub(crate) fn is_multiline_block_context(context: &FormattingContext) -> bool {
    is_block_context(context)
        && !(context.context_node_all_on_same_line() || context.context_node_block_is_on_one_line())
}

// Go: internal/format/rulecontext.go:isSingleLineBlockContext
pub(crate) fn is_single_line_block_context(context: &FormattingContext) -> bool {
    is_block_context(context)
        && (context.context_node_all_on_same_line() || context.context_node_block_is_on_one_line())
}

// Go: internal/format/rulecontext.go:isBlockContext
pub(crate) fn is_block_context(context: &FormattingContext) -> bool {
    node_is_block_context(context.context_node_kind)
}

// Go: internal/format/rulecontext.go:isBeforeBlockContext
pub(crate) fn is_before_block_context(context: &FormattingContext) -> bool {
    node_is_block_context(context.next_token_parent_kind)
}

// Go: internal/format/rulecontext.go:nodeIsBlockContext
fn node_is_block_context(kind: Kind) -> bool {
    if node_is_typescript_decl_with_block_context(kind) {
        return true;
    }
    matches!(
        kind,
        Kind::Block | Kind::CaseBlock | Kind::ObjectLiteralExpression | Kind::ModuleBlock
    )
}

// Go: internal/format/rulecontext.go:isFunctionDeclContext
pub(crate) fn is_function_decl_context(context: &FormattingContext) -> bool {
    matches!(
        context.context_node_kind,
        Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::MethodSignature
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::CallSignature
            | Kind::FunctionExpression
            | Kind::Constructor
            | Kind::ArrowFunction
            // not truly a function, but formats like one
            | Kind::InterfaceDeclaration
    )
}

// Go: internal/format/rulecontext.go:isNotFunctionDeclContext
pub(crate) fn is_not_function_decl_context(context: &FormattingContext) -> bool {
    !is_function_decl_context(context)
}

// Go: internal/format/rulecontext.go:isFunctionDeclarationOrFunctionExpressionContext
pub(crate) fn is_function_declaration_or_function_expression_context(
    context: &FormattingContext,
) -> bool {
    context.context_node_kind == Kind::FunctionDeclaration
        || context.context_node_kind == Kind::FunctionExpression
}

// Go: internal/format/rulecontext.go:isTypeScriptDeclWithBlockContext
pub(crate) fn is_typescript_decl_with_block_context(context: &FormattingContext) -> bool {
    node_is_typescript_decl_with_block_context(context.context_node_kind)
}

// Go: internal/format/rulecontext.go:nodeIsTypeScriptDeclWithBlockContext
fn node_is_typescript_decl_with_block_context(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::ClassDeclaration
            | Kind::ClassExpression
            | Kind::InterfaceDeclaration
            | Kind::EnumDeclaration
            | Kind::TypeLiteral
            | Kind::ModuleDeclaration
            | Kind::ExportDeclaration
            | Kind::NamedExports
            | Kind::ImportDeclaration
            | Kind::NamedImports
    )
}

// Go: internal/format/rulecontext.go:isAfterCodeBlockContext
pub(crate) fn is_after_code_block_context(context: &FormattingContext) -> bool {
    match context.current_token_parent_kind {
        Kind::ClassDeclaration
        | Kind::ModuleDeclaration
        | Kind::EnumDeclaration
        | Kind::CatchClause
        | Kind::ModuleBlock
        | Kind::SwitchStatement => true,
        Kind::Block => {
            let block_parent = context.current_token_parent_parent_kind;
            // In a codefix scenario, parents may not be set, so default to true.
            match block_parent {
                None => true,
                Some(k) => k != Kind::ArrowFunction && k != Kind::FunctionExpression,
            }
        }
        _ => false,
    }
}

// Go: internal/format/rulecontext.go:isControlDeclContext
pub(crate) fn is_control_decl_context(context: &FormattingContext) -> bool {
    matches!(
        context.context_node_kind,
        Kind::IfStatement
            | Kind::SwitchStatement
            | Kind::ForStatement
            | Kind::ForInStatement
            | Kind::ForOfStatement
            | Kind::WhileStatement
            | Kind::TryStatement
            | Kind::DoStatement
            | Kind::WithStatement
            | Kind::CatchClause
    )
}

// Go: internal/format/rulecontext.go:isObjectContext
pub(crate) fn is_object_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ObjectLiteralExpression
}

// Go: internal/format/rulecontext.go:isFunctionCallContext
pub(crate) fn is_function_call_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::CallExpression
}

// Go: internal/format/rulecontext.go:isNewContext
pub(crate) fn is_new_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::NewExpression
}

// Go: internal/format/rulecontext.go:isFunctionCallOrNewContext
pub(crate) fn is_function_call_or_new_context(context: &FormattingContext) -> bool {
    is_function_call_context(context) || is_new_context(context)
}

// Go: internal/format/rulecontext.go:isPreviousTokenNotComma
pub(crate) fn is_previous_token_not_comma(context: &FormattingContext) -> bool {
    context.current_token_kind != Kind::CommaToken
}

// Go: internal/format/rulecontext.go:isNextTokenNotCloseBracket
pub(crate) fn is_next_token_not_close_bracket(context: &FormattingContext) -> bool {
    context.next_token_kind != Kind::CloseBracketToken
}

// Go: internal/format/rulecontext.go:isNextTokenNotCloseParen
pub(crate) fn is_next_token_not_close_paren(context: &FormattingContext) -> bool {
    context.next_token_kind != Kind::CloseParenToken
}

// Go: internal/format/rulecontext.go:isArrowFunctionContext
pub(crate) fn is_arrow_function_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ArrowFunction
}

// Go: internal/format/rulecontext.go:isImportTypeContext
pub(crate) fn is_import_type_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ImportType
}

// Go: internal/format/rulecontext.go:isNonJsxSameLineTokenContext
pub(crate) fn is_non_jsx_same_line_token_context(context: &FormattingContext) -> bool {
    context.tokens_are_on_same_line() && context.context_node_kind != Kind::JsxText
}

// Go: internal/format/rulecontext.go:isNonJsxTextContext
pub(crate) fn is_non_jsx_text_context(context: &FormattingContext) -> bool {
    context.context_node_kind != Kind::JsxText
}

// Go: internal/format/rulecontext.go:isNonJsxElementOrFragmentContext
pub(crate) fn is_non_jsx_element_or_fragment_context(context: &FormattingContext) -> bool {
    context.context_node_kind != Kind::JsxElement && context.context_node_kind != Kind::JsxFragment
}

// Go: internal/format/rulecontext.go:isJsxExpressionContext
pub(crate) fn is_jsx_expression_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::JsxExpression
        || context.context_node_kind == Kind::JsxSpreadAttribute
}

// Go: internal/format/rulecontext.go:isNextTokenParentJsxAttribute
pub(crate) fn is_next_token_parent_jsx_attribute(context: &FormattingContext) -> bool {
    context.next_token_parent_kind == Kind::JsxAttribute
        || (context.next_token_parent_kind == Kind::JsxNamespacedName
            && context.next_token_parent_parent_kind == Some(Kind::JsxAttribute))
}

// Go: internal/format/rulecontext.go:isJsxAttributeContext
pub(crate) fn is_jsx_attribute_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::JsxAttribute
}

// Go: internal/format/rulecontext.go:isNextTokenParentNotJsxNamespacedName
pub(crate) fn is_next_token_parent_not_jsx_namespaced_name(context: &FormattingContext) -> bool {
    context.next_token_parent_kind != Kind::JsxNamespacedName
}

// Go: internal/format/rulecontext.go:isNextTokenParentJsxNamespacedName
pub(crate) fn is_next_token_parent_jsx_namespaced_name(context: &FormattingContext) -> bool {
    context.next_token_parent_kind == Kind::JsxNamespacedName
}

// Go: internal/format/rulecontext.go:isJsxSelfClosingElementContext
pub(crate) fn is_jsx_self_closing_element_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::JsxSelfClosingElement
}

// Go: internal/format/rulecontext.go:isNotBeforeBlockInFunctionDeclarationContext
pub(crate) fn is_not_before_block_in_function_declaration_context(
    context: &FormattingContext,
) -> bool {
    !is_function_decl_context(context) && !is_before_block_context(context)
}

// Go: internal/format/rulecontext.go:isEndOfDecoratorContextOnSameLine
//
// DEFER(phase-7): needs an `IsExpression` parent walk (`nodeIsInDecoratorContext`)
// over real AST nodes, which the projection cannot express. Gates only the
// decorator-spacing rule (decorators deferred per task scope).
// blocked-by: arena parent walk.
pub(crate) fn is_end_of_decorator_context_on_same_line(_context: &FormattingContext) -> bool {
    false
}

// Go: internal/format/rulecontext.go:isStartOfVariableDeclarationList
pub(crate) fn is_start_of_variable_declaration_list(context: &FormattingContext) -> bool {
    context.current_token_parent_kind == Kind::VariableDeclarationList
        && context.current_token_is_start_of_variable_declaration_list
}

// Go: internal/format/rulecontext.go:isNotFormatOnEnter
pub(crate) fn is_not_format_on_enter(context: &FormattingContext) -> bool {
    context.formatting_request_kind != crate::context::FormatRequestKind::FormatOnEnter
}

// Go: internal/format/rulecontext.go:isModuleDeclContext
pub(crate) fn is_module_decl_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ModuleDeclaration
}

// Go: internal/format/rulecontext.go:isObjectTypeContext
pub(crate) fn is_object_type_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::TypeLiteral
}

// Go: internal/format/rulecontext.go:isConstructorSignatureContext
pub(crate) fn is_constructor_signature_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::ConstructSignature
}

// Go: internal/format/rulecontext.go:isTypeArgumentOrParameterOrAssertion
fn is_type_argument_or_parameter_or_assertion(token_kind: Kind, parent_kind: Kind) -> bool {
    if token_kind != Kind::LessThanToken && token_kind != Kind::GreaterThanToken {
        return false;
    }
    matches!(
        parent_kind,
        Kind::TypeReference
            | Kind::TypeAssertionExpression
            | Kind::TypeAliasDeclaration
            | Kind::ClassDeclaration
            | Kind::ClassExpression
            | Kind::InterfaceDeclaration
            | Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::MethodDeclaration
            | Kind::MethodSignature
            | Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::CallExpression
            | Kind::NewExpression
            | Kind::ExpressionWithTypeArguments
    )
}

// Go: internal/format/rulecontext.go:isTypeArgumentOrParameterOrAssertionContext
pub(crate) fn is_type_argument_or_parameter_or_assertion_context(
    context: &FormattingContext,
) -> bool {
    is_type_argument_or_parameter_or_assertion(
        context.current_token_kind,
        context.current_token_parent_kind,
    ) || is_type_argument_or_parameter_or_assertion(
        context.next_token_kind,
        context.next_token_parent_kind,
    )
}

// Go: internal/format/rulecontext.go:isTypeAssertionContext
pub(crate) fn is_type_assertion_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::TypeAssertionExpression
}

// Go: internal/format/rulecontext.go:isNonTypeAssertionContext
pub(crate) fn is_non_type_assertion_context(context: &FormattingContext) -> bool {
    !is_type_assertion_context(context)
}

// Go: internal/format/rulecontext.go:isVoidOpContext
pub(crate) fn is_void_op_context(context: &FormattingContext) -> bool {
    context.current_token_kind == Kind::VoidKeyword
        && context.current_token_parent_kind == Kind::VoidExpression
}

// Go: internal/format/rulecontext.go:isYieldOrYieldStarWithOperand
pub(crate) fn is_yield_or_yield_star_with_operand(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::YieldExpression && context.context_node_has_expression
}

// Go: internal/format/rulecontext.go:isNonNullAssertionContext
pub(crate) fn is_non_null_assertion_context(context: &FormattingContext) -> bool {
    context.context_node_kind == Kind::NonNullExpression
}

// Go: internal/format/rulecontext.go:isNotStatementConditionContext
pub(crate) fn is_not_statement_condition_context(context: &FormattingContext) -> bool {
    !is_statement_condition_context(context)
}

// Go: internal/format/rulecontext.go:isStatementConditionContext
pub(crate) fn is_statement_condition_context(context: &FormattingContext) -> bool {
    matches!(
        context.context_node_kind,
        Kind::IfStatement
            | Kind::ForStatement
            | Kind::ForInStatement
            | Kind::ForOfStatement
            | Kind::DoStatement
            | Kind::WhileStatement
    )
}

// Go: internal/format/rulecontext.go:isSemicolonDeletionContext
//
// DEFER(phase-7): needs `astnav.FindNextToken`/`lsutil.GetFirstToken` plus
// `scanner.GetECMALineOfPosition` to inspect the following real token. Gates
// only the `Semicolons == Remove` rule (off by default), so returns `false`.
// blocked-by: astnav navigation + scanner line API.
pub(crate) fn is_semicolon_deletion_context(_context: &FormattingContext) -> bool {
    false
}

// Go: internal/format/rulecontext.go:isSemicolonInsertionContext
//
// DEFER(phase-7): needs `lsutil.PositionIsASICandidate`, which `tsgo_ls_lsutil`
// defers (it requires `astnav` token-cache navigation). Gates only the
// `Semicolons == Insert` rule (off by default), so returns `false`.
// blocked-by: lsutil PositionIsASICandidate port.
pub(crate) fn is_semicolon_insertion_context(_context: &FormattingContext) -> bool {
    false
}

// Go: internal/format/rulecontext.go:isNotPropertyAccessOnIntegerLiteral
pub(crate) fn is_not_property_access_on_integer_literal(context: &FormattingContext) -> bool {
    !context.context_node_is_property_access_on_integer_literal
}

#[cfg(test)]
#[path = "rulecontext_test.rs"]
mod tests;
