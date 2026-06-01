use super::*;
use crate::context::{FormatRequestKind, FormattingContext};
use crate::format_code_settings::get_default_format_code_settings;
use tsgo_ast::Kind;

fn ctx() -> FormattingContext {
    FormattingContext::new(
        get_default_format_code_settings(),
        FormatRequestKind::FormatDocument,
    )
}

// Go: internal/format/rulecontext.go:isNonJsxSameLineTokenContext
#[test]
fn non_jsx_same_line_token_context() {
    let mut c = ctx();
    c.tokens_are_on_same_line = true;
    c.context_node_kind = Kind::ArrayLiteralExpression;
    assert!(is_non_jsx_same_line_token_context(&c));

    c.context_node_kind = Kind::JsxText;
    assert!(!is_non_jsx_same_line_token_context(&c));

    c.context_node_kind = Kind::ArrayLiteralExpression;
    c.tokens_are_on_same_line = false;
    assert!(!is_non_jsx_same_line_token_context(&c));
}

// Go: internal/format/rulecontext.go:isForContext / isNotForContext
#[test]
fn for_context() {
    let mut c = ctx();
    c.context_node_kind = Kind::ForStatement;
    assert!(is_for_context(&c));
    assert!(!is_not_for_context(&c));
    c.context_node_kind = Kind::WhileStatement;
    assert!(!is_for_context(&c));
    assert!(is_not_for_context(&c));
}

// Go: internal/format/rulecontext.go:isBinaryOpContext
#[test]
fn binary_op_context() {
    let mut c = ctx();
    c.context_node_kind = Kind::BinaryExpression;
    c.binary_operator_token_kind = Kind::PlusToken;
    assert!(is_binary_op_context(&c));
    // a comma operator is NOT a binary-op context
    c.binary_operator_token_kind = Kind::CommaToken;
    assert!(!is_binary_op_context(&c));
    // conditional expression is a binary-op context
    c.context_node_kind = Kind::ConditionalExpression;
    assert!(is_binary_op_context(&c));
    // variable declaration: only when an equals token is involved
    c.context_node_kind = Kind::VariableDeclaration;
    c.current_token_kind = Kind::EqualsToken;
    assert!(is_binary_op_context(&c));
    c.current_token_kind = Kind::Identifier;
    c.next_token_kind = Kind::Identifier;
    assert!(!is_binary_op_context(&c));
}

// Go: internal/format/rulecontext.go:isBlockContext / nodeIsBlockContext
#[test]
fn block_context() {
    let mut c = ctx();
    c.context_node_kind = Kind::Block;
    assert!(is_block_context(&c));
    // TypeScript decl with block (class) is also a block context
    c.context_node_kind = Kind::ClassDeclaration;
    assert!(is_block_context(&c));
    c.context_node_kind = Kind::Identifier;
    assert!(!is_block_context(&c));
}

// Go: internal/format/rulecontext.go:isControlDeclContext
#[test]
fn control_decl_context() {
    let mut c = ctx();
    c.context_node_kind = Kind::IfStatement;
    assert!(is_control_decl_context(&c));
    c.context_node_kind = Kind::CatchClause;
    assert!(is_control_decl_context(&c));
    c.context_node_kind = Kind::Block;
    assert!(!is_control_decl_context(&c));
}

// Go: internal/format/rulecontext.go:isFunctionDeclContext
#[test]
fn function_decl_context() {
    let mut c = ctx();
    c.context_node_kind = Kind::FunctionDeclaration;
    assert!(is_function_decl_context(&c));
    // interface acts like a function for formatting
    c.context_node_kind = Kind::InterfaceDeclaration;
    assert!(is_function_decl_context(&c));
    c.context_node_kind = Kind::Block;
    assert!(!is_function_decl_context(&c));
}

// Go: internal/format/rulecontext.go:isTypeAnnotationContext
#[test]
fn type_annotation_context() {
    let mut c = ctx();
    c.context_node_kind = Kind::PropertyDeclaration;
    assert!(is_type_annotation_context(&c));
    // function-like kinds count
    c.context_node_kind = Kind::FunctionDeclaration;
    assert!(is_type_annotation_context(&c));
    c.context_node_kind = Kind::Identifier;
    assert!(!is_type_annotation_context(&c));
}

// Go: internal/format/rulecontext.go:isOptionEnabled / isOptionDisabledOrUndefined
#[test]
fn option_builders_read_options() {
    let c = ctx(); // default: insert_space_after_comma_delimiter = True
    let enabled = is_option_enabled(insert_space_after_comma_delimiter_option);
    assert!(enabled(&c));
    let disabled = is_option_disabled_or_undefined(insert_space_after_comma_delimiter_option);
    assert!(!disabled(&c));
}

// Go: internal/format/rulecontext.go:optionEquals
#[test]
fn option_equals_compares_semicolon_preference() {
    let mut c = ctx();
    let is_remove = option_equals(semicolon_option, SemicolonPreference::Remove);
    assert!(!is_remove(&c));
    c.options.semicolons = SemicolonPreference::Remove;
    assert!(is_remove(&c));
}

// Go: internal/format/rulecontext.go:isStatementConditionContext
#[test]
fn statement_condition_context() {
    let mut c = ctx();
    c.context_node_kind = Kind::WhileStatement;
    assert!(is_statement_condition_context(&c));
    assert!(!is_not_statement_condition_context(&c));
    c.context_node_kind = Kind::Block;
    assert!(!is_statement_condition_context(&c));
    assert!(is_not_statement_condition_context(&c));
}

// Go: internal/format/rulecontext.go:isTypeArgumentOrParameterOrAssertionContext
#[test]
fn type_argument_or_parameter_or_assertion_context() {
    let mut c = ctx();
    c.current_token_kind = Kind::LessThanToken;
    c.current_token_parent_kind = Kind::TypeReference;
    assert!(is_type_argument_or_parameter_or_assertion_context(&c));
    // wrong parent kind -> false
    c.current_token_parent_kind = Kind::Block;
    c.next_token_kind = Kind::Identifier;
    c.next_token_parent_kind = Kind::Block;
    assert!(!is_type_argument_or_parameter_or_assertion_context(&c));
}
