use super::*;
use crate::textwriter::new_text_writer;

// Go: internal/printer/emittextwriter.go:EmitTextWriter
#[test]
fn classified_writes_all_append_text() {
    let mut w = new_text_writer("\n", 4);
    let sink: &mut dyn EmitTextWriter = &mut w;
    sink.write_keyword("const");
    sink.write_space(" ");
    sink.write_property("x");
    sink.write_operator(" = ");
    sink.write_string_literal("\"y\"");
    sink.write_trailing_semicolon(";");
    assert_eq!(sink.get_text(), "const x = \"y\";");
}

// Go: internal/printer/emittextwriter.go:EmitTextWriter.WriteSymbol
#[test]
fn write_symbol_appends_text_ignoring_symbol() {
    let mut w = new_text_writer("\n", 4);
    let symbol = tsgo_ast::Symbol::default();
    let sink: &mut dyn EmitTextWriter = &mut w;
    sink.write_symbol("name", &symbol);
    assert_eq!(sink.get_text(), "name");
}
