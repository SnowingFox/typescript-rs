use super::*;
use crate::format_code_settings::get_default_format_code_settings;
use tsgo_core::tristate::Tristate;

// Go: internal/format/span.go:getIndentationString (spaces)
#[test]
fn indentation_string_spaces_default() {
    let opts = get_default_format_code_settings(); // ConvertTabsToSpaces = True
    assert_eq!(get_indentation_string(0, &opts), "");
    assert_eq!(get_indentation_string(4, &opts), "    ");
    assert_eq!(get_indentation_string(2, &opts), "  ");
}

// Go: internal/format/span.go:getIndentationString (tabs + remainder spaces)
#[test]
fn indentation_string_tabs() {
    let mut opts = get_default_format_code_settings();
    opts.editor.convert_tabs_to_spaces = Tristate::False;
    opts.editor.tab_size = 4;
    assert_eq!(get_indentation_string(0, &opts), "");
    assert_eq!(get_indentation_string(4, &opts), "\t");
    assert_eq!(get_indentation_string(8, &opts), "\t\t");
    // 6 = one tab (4) + two spaces
    assert_eq!(get_indentation_string(6, &opts), "\t  ");
}

// Go: internal/format/span.go:getIndentationString (tab size 0 -> empty)
#[test]
fn indentation_string_tab_size_zero() {
    let mut opts = get_default_format_code_settings();
    opts.editor.convert_tabs_to_spaces = Tristate::False;
    opts.editor.tab_size = 0;
    assert_eq!(get_indentation_string(8, &opts), "");
}
