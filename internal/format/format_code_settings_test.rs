use super::*;
use tsgo_core::tristate::Tristate;

// Go: internal/format/../ls/lsutil/formatcodeoptions.go:GetDefaultFormatCodeSettings
#[test]
fn default_editor_settings_match_go() {
    let s = get_default_format_code_settings();
    assert_eq!(s.editor.indent_size, 4);
    assert_eq!(s.editor.tab_size, 4);
    assert_eq!(s.editor.new_line_character, "\n");
    assert_eq!(s.editor.convert_tabs_to_spaces, Tristate::True);
    assert_eq!(s.editor.indent_style, IndentStyle::Smart);
    assert_eq!(s.editor.trim_trailing_whitespace, Tristate::True);
    assert_eq!(s.editor.base_indent_size, 0);
}

// Go: ...formatcodeoptions.go:GetDefaultFormatCodeSettings
#[test]
fn default_format_flags_match_go() {
    let s = get_default_format_code_settings();
    assert_eq!(s.insert_space_after_constructor, Tristate::False);
    assert_eq!(s.insert_space_after_comma_delimiter, Tristate::True);
    assert_eq!(
        s.insert_space_after_semicolon_in_for_statements,
        Tristate::True
    );
    assert_eq!(
        s.insert_space_before_and_after_binary_operators,
        Tristate::True
    );
    assert_eq!(
        s.insert_space_after_keywords_in_control_flow_statements,
        Tristate::True
    );
    assert_eq!(
        s.insert_space_after_function_keyword_for_anonymous_functions,
        Tristate::False
    );
    assert_eq!(
        s.insert_space_after_opening_and_before_closing_nonempty_parenthesis,
        Tristate::False
    );
    assert_eq!(
        s.insert_space_after_opening_and_before_closing_nonempty_brackets,
        Tristate::False
    );
    assert_eq!(
        s.insert_space_after_opening_and_before_closing_nonempty_braces,
        Tristate::True
    );
    assert_eq!(s.insert_space_before_function_parenthesis, Tristate::False);
    assert_eq!(
        s.place_open_brace_on_new_line_for_functions,
        Tristate::False
    );
    assert_eq!(
        s.place_open_brace_on_new_line_for_control_blocks,
        Tristate::False
    );
    assert_eq!(s.semicolons, SemicolonPreference::Ignore);
    assert_eq!(s.indent_switch_case, Tristate::True);
}

// Go: ...formatcodeoptions.go:parseIndentStyle
#[test]
fn parse_indent_style_matches_go() {
    assert_eq!(parse_indent_style_str("none"), IndentStyle::None);
    assert_eq!(parse_indent_style_str("block"), IndentStyle::Block);
    assert_eq!(parse_indent_style_str("smart"), IndentStyle::Smart);
    assert_eq!(parse_indent_style_str("NONE"), IndentStyle::None);
    // unknown -> Smart (Go default branch)
    assert_eq!(parse_indent_style_str("bogus"), IndentStyle::Smart);
}

// Go: ...formatcodeoptions.go:IndentStyle iota
#[test]
fn indent_style_discriminants_match_go() {
    assert_eq!(IndentStyle::None as i32, 0);
    assert_eq!(IndentStyle::Block as i32, 1);
    assert_eq!(IndentStyle::Smart as i32, 2);
}

// Go: ...formatcodeoptions.go:parseSemicolonPreference
#[test]
fn parse_semicolon_preference_matches_go() {
    assert_eq!(
        parse_semicolon_preference_str("ignore"),
        SemicolonPreference::Ignore
    );
    assert_eq!(
        parse_semicolon_preference_str("insert"),
        SemicolonPreference::Insert
    );
    assert_eq!(
        parse_semicolon_preference_str("remove"),
        SemicolonPreference::Remove
    );
    assert_eq!(
        parse_semicolon_preference_str("INSERT"),
        SemicolonPreference::Insert
    );
    // unknown -> Ignore (Go default branch)
    assert_eq!(
        parse_semicolon_preference_str("bogus"),
        SemicolonPreference::Ignore
    );
}

// Go: ...formatcodeoptions.go:SemicolonPreference string values
#[test]
fn semicolon_preference_string_values_match_go() {
    assert_eq!(SemicolonPreference::Ignore.as_str(), "ignore");
    assert_eq!(SemicolonPreference::Insert.as_str(), "insert");
    assert_eq!(SemicolonPreference::Remove.as_str(), "remove");
}
