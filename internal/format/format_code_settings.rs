//! Formatter configuration (`FormatCodeSettings`).
//!
//! Port of Go `internal/ls/lsutil/formatcodeoptions.go`.
//!
//! # Location divergence (documented)
//!
//! In Go, `FormatCodeSettings` lives in `lsutil`. The Rust `tsgo_ls_lsutil`
//! port explicitly **deferred** this type (it depends on Go reflection-based
//! config marshaling, `lsproto`, and `printer`), and the P7 `format`
//! parallel-safety boundary forbids editing `tsgo_ls_lsutil`. Since `format` is
//! the consumer here, the data model + defaults are ported into this crate. The
//! `lsproto`-dependent helpers (`FromLSFormatOptions`/`ToLSFormatOptions`) are
//! deferred (blocked-by: `tsgo_lsproto::FormattingOptions` wiring).

use tsgo_core::tristate::Tristate;

/// Default indent size used when none is configured (4).
///
/// Mirrors `printer.GetDefaultIndentSize()`; inlined as a constant to avoid a
/// heavy `tsgo_printer` build dependency.
// Go: internal/printer/textwriter.go:GetDefaultIndentSize
pub const DEFAULT_INDENT_SIZE: i32 = 4;

/// How the editor computes indentation.
///
/// Mirrors Go's `IndentStyle` iota enum.
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::IndentStyle;
/// assert_eq!(IndentStyle::None as i32, 0);
/// assert_eq!(IndentStyle::Smart as i32, 2);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ls/lsutil/formatcodeoptions.go:IndentStyle
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
#[repr(i32)]
pub enum IndentStyle {
    /// No indentation (`IndentStyleNone`).
    None = 0,
    /// Match the previous non-whitespace line (`IndentStyleBlock`).
    Block = 1,
    /// Syntax-aware "smart" indentation (`IndentStyleSmart`), the default.
    #[default]
    Smart = 2,
}

/// Parses an [`IndentStyle`] from a string, case-insensitively.
///
/// Unknown values fall back to [`IndentStyle::Smart`], mirroring Go's default
/// branch (the numeric-value branches of Go's `parseIndentStyle` are handled by
/// the config layer and are out of scope here).
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::{parse_indent_style_str, IndentStyle};
/// assert_eq!(parse_indent_style_str("block"), IndentStyle::Block);
/// assert_eq!(parse_indent_style_str("bogus"), IndentStyle::Smart);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/formatcodeoptions.go:parseIndentStyle
pub fn parse_indent_style_str(value: &str) -> IndentStyle {
    match value.to_lowercase().as_str() {
        "none" => IndentStyle::None,
        "block" => IndentStyle::Block,
        "smart" => IndentStyle::Smart,
        _ => IndentStyle::Smart,
    }
}

/// Whether the formatter inserts/removes trailing semicolons.
///
/// Mirrors Go's `SemicolonPreference` (a string-valued type).
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::SemicolonPreference;
/// assert_eq!(SemicolonPreference::Insert.as_str(), "insert");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ls/lsutil/formatcodeoptions.go:SemicolonPreference
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum SemicolonPreference {
    /// Leave semicolons as-is (`SemicolonPreferenceIgnore`), the default.
    #[default]
    Ignore,
    /// Insert missing semicolons (`SemicolonPreferenceInsert`).
    Insert,
    /// Remove unnecessary semicolons (`SemicolonPreferenceRemove`).
    Remove,
}

impl SemicolonPreference {
    /// Returns the string form Go uses for this preference.
    ///
    /// # Examples
    /// ```
    /// use tsgo_format::format_code_settings::SemicolonPreference;
    /// assert_eq!(SemicolonPreference::Remove.as_str(), "remove");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsutil/formatcodeoptions.go:SemicolonPreference
    pub fn as_str(self) -> &'static str {
        match self {
            SemicolonPreference::Ignore => "ignore",
            SemicolonPreference::Insert => "insert",
            SemicolonPreference::Remove => "remove",
        }
    }
}

/// Parses a [`SemicolonPreference`] from a string, case-insensitively.
///
/// Unknown values fall back to [`SemicolonPreference::Ignore`], mirroring Go's
/// default branch.
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::{parse_semicolon_preference_str, SemicolonPreference};
/// assert_eq!(parse_semicolon_preference_str("remove"), SemicolonPreference::Remove);
/// assert_eq!(parse_semicolon_preference_str("bogus"), SemicolonPreference::Ignore);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/formatcodeoptions.go:parseSemicolonPreference
pub fn parse_semicolon_preference_str(value: &str) -> SemicolonPreference {
    match value.to_lowercase().as_str() {
        "ignore" => SemicolonPreference::Ignore,
        "insert" => SemicolonPreference::Insert,
        "remove" => SemicolonPreference::Remove,
        _ => SemicolonPreference::Ignore,
    }
}

/// Editor-level indentation/whitespace settings.
///
/// Mirrors Go's `EditorSettings`, which is embedded into [`FormatCodeSettings`].
/// Rust has no struct embedding, so it is held as the [`FormatCodeSettings::editor`]
/// field (`options.IndentSize` in Go becomes `options.editor.indent_size`).
///
/// Side effects: none (pure value type).
// Go: internal/ls/lsutil/formatcodeoptions.go:EditorSettings
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct EditorSettings {
    /// Base indentation added to every computed indent (`BaseIndentSize`).
    pub base_indent_size: i32,
    /// Number of columns per indentation level (`IndentSize`).
    pub indent_size: i32,
    /// Width of a tab in columns (`TabSize`).
    pub tab_size: i32,
    /// Newline string to insert (`NewLineCharacter`).
    pub new_line_character: String,
    /// Whether to emit spaces instead of tabs (`ConvertTabsToSpaces`).
    pub convert_tabs_to_spaces: Tristate,
    /// The indentation style (`IndentStyle`).
    pub indent_style: IndentStyle,
    /// Whether to trim trailing whitespace (`TrimTrailingWhitespace`).
    pub trim_trailing_whitespace: Tristate,
}

/// The full set of formatter options consulted by the rules engine and indenter.
///
/// Mirrors Go's `FormatCodeSettings` (which embeds `EditorSettings`).
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::get_default_format_code_settings;
/// use tsgo_core::tristate::Tristate;
/// let s = get_default_format_code_settings();
/// assert_eq!(s.insert_space_after_comma_delimiter, Tristate::True);
/// assert_eq!(s.editor.indent_size, 4);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ls/lsutil/formatcodeoptions.go:FormatCodeSettings
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FormatCodeSettings {
    /// Embedded editor settings (Go embedding flattened to a field).
    pub editor: EditorSettings,
    /// `insertSpaceAfterCommaDelimiter`.
    pub insert_space_after_comma_delimiter: Tristate,
    /// `insertSpaceAfterSemicolonInForStatements`.
    pub insert_space_after_semicolon_in_for_statements: Tristate,
    /// `insertSpaceBeforeAndAfterBinaryOperators`.
    pub insert_space_before_and_after_binary_operators: Tristate,
    /// `insertSpaceAfterConstructor`.
    pub insert_space_after_constructor: Tristate,
    /// `insertSpaceAfterKeywordsInControlFlowStatements`.
    pub insert_space_after_keywords_in_control_flow_statements: Tristate,
    /// `insertSpaceAfterFunctionKeywordForAnonymousFunctions`.
    pub insert_space_after_function_keyword_for_anonymous_functions: Tristate,
    /// `insertSpaceAfterOpeningAndBeforeClosingNonemptyParenthesis`.
    pub insert_space_after_opening_and_before_closing_nonempty_parenthesis: Tristate,
    /// `insertSpaceAfterOpeningAndBeforeClosingNonemptyBrackets`.
    pub insert_space_after_opening_and_before_closing_nonempty_brackets: Tristate,
    /// `insertSpaceAfterOpeningAndBeforeClosingNonemptyBraces`.
    pub insert_space_after_opening_and_before_closing_nonempty_braces: Tristate,
    /// `insertSpaceAfterOpeningAndBeforeClosingEmptyBraces`.
    pub insert_space_after_opening_and_before_closing_empty_braces: Tristate,
    /// `insertSpaceAfterOpeningAndBeforeClosingTemplateStringBraces`.
    pub insert_space_after_opening_and_before_closing_template_string_braces: Tristate,
    /// `insertSpaceAfterOpeningAndBeforeClosingJsxExpressionBraces`.
    pub insert_space_after_opening_and_before_closing_jsx_expression_braces: Tristate,
    /// `insertSpaceAfterTypeAssertion`.
    pub insert_space_after_type_assertion: Tristate,
    /// `insertSpaceBeforeFunctionParenthesis`.
    pub insert_space_before_function_parenthesis: Tristate,
    /// `placeOpenBraceOnNewLineForFunctions`.
    pub place_open_brace_on_new_line_for_functions: Tristate,
    /// `placeOpenBraceOnNewLineForControlBlocks`.
    pub place_open_brace_on_new_line_for_control_blocks: Tristate,
    /// `insertSpaceBeforeTypeAnnotation`.
    pub insert_space_before_type_annotation: Tristate,
    /// `indentMultiLineObjectLiteralBeginningOnBlankLine`.
    pub indent_multi_line_object_literal_beginning_on_blank_line: Tristate,
    /// `semicolons`.
    pub semicolons: SemicolonPreference,
    /// `indentSwitchCase`.
    pub indent_switch_case: Tristate,
}

/// Returns the default formatter settings, matching Go's
/// `GetDefaultFormatCodeSettings`.
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::get_default_format_code_settings;
/// use tsgo_format::format_code_settings::{IndentStyle, SemicolonPreference};
/// use tsgo_core::tristate::Tristate;
/// let s = get_default_format_code_settings();
/// assert_eq!(s.editor.indent_style, IndentStyle::Smart);
/// assert_eq!(s.semicolons, SemicolonPreference::Ignore);
/// assert_eq!(s.insert_space_before_and_after_binary_operators, Tristate::True);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/formatcodeoptions.go:GetDefaultFormatCodeSettings
pub fn get_default_format_code_settings() -> FormatCodeSettings {
    FormatCodeSettings {
        editor: EditorSettings {
            base_indent_size: 0,
            indent_size: DEFAULT_INDENT_SIZE,
            tab_size: DEFAULT_INDENT_SIZE,
            new_line_character: "\n".to_string(),
            convert_tabs_to_spaces: Tristate::True,
            indent_style: IndentStyle::Smart,
            trim_trailing_whitespace: Tristate::True,
        },
        insert_space_after_constructor: Tristate::False,
        insert_space_after_comma_delimiter: Tristate::True,
        insert_space_after_semicolon_in_for_statements: Tristate::True,
        insert_space_before_and_after_binary_operators: Tristate::True,
        insert_space_after_keywords_in_control_flow_statements: Tristate::True,
        insert_space_after_function_keyword_for_anonymous_functions: Tristate::False,
        insert_space_after_opening_and_before_closing_nonempty_parenthesis: Tristate::False,
        insert_space_after_opening_and_before_closing_nonempty_brackets: Tristate::False,
        insert_space_after_opening_and_before_closing_nonempty_braces: Tristate::True,
        insert_space_after_opening_and_before_closing_empty_braces: Tristate::default(),
        insert_space_after_opening_and_before_closing_template_string_braces: Tristate::default(),
        insert_space_after_opening_and_before_closing_jsx_expression_braces: Tristate::default(),
        insert_space_after_type_assertion: Tristate::default(),
        insert_space_before_function_parenthesis: Tristate::False,
        place_open_brace_on_new_line_for_functions: Tristate::False,
        place_open_brace_on_new_line_for_control_blocks: Tristate::False,
        insert_space_before_type_annotation: Tristate::default(),
        indent_multi_line_object_literal_beginning_on_blank_line: Tristate::default(),
        semicolons: SemicolonPreference::Ignore,
        indent_switch_case: Tristate::True,
    }
}

#[cfg(test)]
#[path = "format_code_settings_test.rs"]
mod tests;
