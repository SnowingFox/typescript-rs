use super::*;
use crate::format_code_settings::get_default_format_code_settings;
use tsgo_ast::Kind;

// Go: internal/format/context.go:NewFormattingContext
#[test]
fn new_formatting_context_carries_options_and_kind() {
    let ctx = FormattingContext::new(
        get_default_format_code_settings(),
        FormatRequestKind::FormatDocument,
    );
    assert_eq!(
        ctx.formatting_request_kind,
        FormatRequestKind::FormatDocument
    );
    assert_eq!(
        ctx.options.insert_space_after_comma_delimiter,
        tsgo_core::tristate::Tristate::True
    );
    // defaults: kinds are Unknown, line flags false
    assert_eq!(ctx.current_token_kind, Kind::Unknown);
    assert!(!ctx.tokens_are_on_same_line());
}

// Go: internal/format/context.go:TokensAreOnSameLine
#[test]
fn line_relationship_accessors_read_projection() {
    let mut ctx = FormattingContext::new(
        get_default_format_code_settings(),
        FormatRequestKind::FormatDocument,
    );
    ctx.tokens_are_on_same_line = true;
    ctx.context_node_all_on_same_line = true;
    ctx.next_node_all_on_same_line = true;
    ctx.context_node_block_is_on_one_line = true;
    ctx.next_node_block_is_on_one_line = true;
    assert!(ctx.tokens_are_on_same_line());
    assert!(ctx.context_node_all_on_same_line());
    assert!(ctx.next_node_all_on_same_line());
    assert!(ctx.context_node_block_is_on_one_line());
    assert!(ctx.next_node_block_is_on_one_line());
}

// Go: internal/format/api.go:FormatRequestKind
#[test]
fn format_request_kind_discriminants_match_go() {
    assert_eq!(FormatRequestKind::FormatDocument as i32, 0);
    assert_eq!(FormatRequestKind::FormatSelection as i32, 1);
    assert_eq!(FormatRequestKind::FormatOnEnter as i32, 2);
    assert_eq!(FormatRequestKind::FormatOnSemicolon as i32, 3);
    assert_eq!(FormatRequestKind::FormatOnOpeningCurlyBrace as i32, 4);
    assert_eq!(FormatRequestKind::FormatOnClosingCurlyBrace as i32, 5);
}
