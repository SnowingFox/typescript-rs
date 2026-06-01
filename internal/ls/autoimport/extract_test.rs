use super::*;
use crate::export::ExportSyntax;
use crate::index::Index;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};
use tsgo_tspath::to_path;

/// Parses `src` as a `.ts` file and extracts its top-level exports.
fn extract(file_name: &str, src: &str) -> Vec<Export> {
    let result = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        src,
        ScriptKind::Ts,
    );
    let path = to_path(file_name, "/", true);
    extract_top_level_exports(&result.arena, result.source_file, &path)
}

fn by_name<'a>(exports: &'a [Export], name: &str) -> &'a Export {
    exports
        .iter()
        .find(|e| e.name() == name)
        .unwrap_or_else(|| panic!("no export named {name:?} in {:?}", names(exports)))
}

fn names(exports: &[Export]) -> Vec<String> {
    exports.iter().map(Export::name).collect()
}

// Go: internal/ls/autoimport/extract.go:exportExtractor.extractFromFile
// (reachable AST-walking analog; task TDD slice 1)
#[test]
fn extracts_const_function_class() {
    let exports = extract(
        "/src/a.ts",
        "export const a = 1;\nexport function b(){}\nexport class C{}\n",
    );
    assert_eq!(names(&exports), vec!["a", "b", "C"]);

    let a = by_name(&exports, "a");
    assert_eq!(a.script_element_kind, ScriptElementKind::VariableElement);
    assert_eq!(a.flags, SymbolFlags::BLOCK_SCOPED_VARIABLE);
    assert_eq!(a.syntax, ExportSyntax::Modifier);
    assert_eq!(a.id.module_id, ModuleId::new("/src/a.ts"));
    assert_eq!(a.module_file_name, "/src/a.ts");

    let b = by_name(&exports, "b");
    assert_eq!(b.script_element_kind, ScriptElementKind::FunctionElement);
    assert_eq!(b.flags, SymbolFlags::FUNCTION);
    assert_eq!(b.syntax, ExportSyntax::Modifier);

    let c = by_name(&exports, "C");
    assert_eq!(c.script_element_kind, ScriptElementKind::ClassElement);
    assert_eq!(c.flags, SymbolFlags::CLASS);
    assert_eq!(c.syntax, ExportSyntax::Modifier);
}

// The extracted exports index by their names.
#[test]
fn extracted_exports_index_by_name() {
    let exports = extract(
        "/src/a.ts",
        "export const a = 1;\nexport function b(){}\nexport class C{}\n",
    );
    let mut idx: Index<Export> = Index::default();
    for e in exports {
        idx.insert_as_words(e);
    }
    assert_eq!(idx.find("a", true).len(), 1);
    assert_eq!(idx.find("b", true).len(), 1);
    assert_eq!(idx.find("C", true).len(), 1);
    assert_eq!(idx.find("missing", true).len(), 0);
}

// Non-exported declarations are not extracted.
#[test]
fn skips_non_exported_declarations() {
    let exports = extract(
        "/src/a.ts",
        "const hidden = 1;\nexport const shown = 2;\nfunction alsoHidden(){}\n",
    );
    assert_eq!(names(&exports), vec!["shown"]);
}

// `let` / `var` are also block- or function-scoped variables.
#[test]
fn extracts_let_and_var() {
    let exports = extract("/src/a.ts", "export let l = 1;\nexport var v = 2;\n");
    let l = by_name(&exports, "l");
    assert_eq!(l.flags, SymbolFlags::BLOCK_SCOPED_VARIABLE);
    assert_eq!(l.script_element_kind, ScriptElementKind::VariableElement);
    let v = by_name(&exports, "v");
    assert_eq!(v.flags, SymbolFlags::FUNCTION_SCOPED_VARIABLE);
    assert_eq!(v.script_element_kind, ScriptElementKind::VariableElement);
}

// --- Slice 2: `export { x }`, `export default`, `export =`, `export *`. ---

// Go: internal/ls/autoimport/extract.go:getSyntax (KindExportSpecifier)
#[test]
fn extracts_named_export() {
    let exports = extract("/src/a.ts", "const x = 1;\nexport { x };\n");
    let x = by_name(&exports, "x");
    assert_eq!(x.syntax, ExportSyntax::Named);
    assert_eq!(x.flags, SymbolFlags::ALIAS);
    assert_eq!(x.id.export_name, "x");
}

#[test]
fn extracts_named_export_with_alias() {
    let exports = extract("/src/a.ts", "const x = 1;\nexport { x as y };\n");
    // Exported under the renamed binding `y`.
    assert_eq!(names(&exports), vec!["y"]);
    let y = by_name(&exports, "y");
    assert_eq!(y.syntax, ExportSyntax::Named);
}

// Go: internal/ls/autoimport/extract.go:getSyntax (ModifierFlagsDefault)
#[test]
fn extracts_default_function() {
    let exports = extract("/src/a.ts", "export default function foo(){}\n");
    let foo = by_name(&exports, "foo");
    assert_eq!(foo.id.export_name, "default");
    assert_eq!(foo.syntax, ExportSyntax::DefaultModifier);
    assert_eq!(foo.script_element_kind, ScriptElementKind::FunctionElement);
    assert_eq!(foo.flags, SymbolFlags::FUNCTION);
}

#[test]
fn extracts_default_class() {
    let exports = extract("/src/a.ts", "export default class Bar {}\n");
    let bar = by_name(&exports, "Bar");
    assert_eq!(bar.id.export_name, "default");
    assert_eq!(bar.syntax, ExportSyntax::DefaultModifier);
    assert_eq!(bar.script_element_kind, ScriptElementKind::ClassElement);
}

// Go: internal/ls/autoimport/extract.go:getSyntax (KindExportAssignment, !IsExportEquals)
#[test]
fn extracts_default_assignment_identifier() {
    let exports = extract("/src/a.ts", "const thing = 1;\nexport default thing;\n");
    let e = by_name(&exports, "thing");
    assert_eq!(e.id.export_name, "default");
    assert_eq!(e.syntax, ExportSyntax::DefaultDeclaration);
}

// Anonymous default falls back to a file-name-derived identifier.
#[test]
fn extracts_anonymous_default_uses_file_name() {
    let exports = extract("/src/widget.ts", "export default 42;\n");
    let e = by_name(&exports, "widget");
    assert_eq!(e.id.export_name, "default");
    assert_eq!(e.syntax, ExportSyntax::DefaultDeclaration);
}

// Go: internal/ls/autoimport/extract.go:getSyntax (KindExportAssignment, IsExportEquals)
#[test]
fn extracts_export_equals() {
    let exports = extract("/src/a.ts", "const api = {};\nexport = api;\n");
    let e = by_name(&exports, "api");
    assert_eq!(e.id.export_name, "export=");
    assert_eq!(e.syntax, ExportSyntax::Equals);
}

// Go: internal/ls/autoimport/extract.go:getSyntax (ExportSyntaxModifier on type decls)
#[test]
fn extracts_interface_type_enum_namespace() {
    let exports = extract(
        "/src/a.ts",
        "export interface I {}\nexport type T = number;\nexport enum E { A }\nexport namespace N {}\n",
    );
    assert_eq!(
        by_name(&exports, "I").script_element_kind,
        ScriptElementKind::InterfaceElement
    );
    assert_eq!(by_name(&exports, "I").flags, SymbolFlags::INTERFACE);
    assert_eq!(
        by_name(&exports, "T").script_element_kind,
        ScriptElementKind::TypeElement
    );
    assert_eq!(by_name(&exports, "T").flags, SymbolFlags::TYPE_ALIAS);
    assert_eq!(
        by_name(&exports, "E").script_element_kind,
        ScriptElementKind::EnumElement
    );
    assert_eq!(
        by_name(&exports, "N").script_element_kind,
        ScriptElementKind::ModuleElement
    );
}

// `export * from "..."` produces no directly indexable name; its specifier is
// collected for cross-file resolution.
#[test]
fn export_star_is_not_indexable_but_collected() {
    let file_name = "/src/a.ts";
    let result = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        "export * from \"./b\";\n",
        ScriptKind::Ts,
    );
    let path = to_path(file_name, "/", true);
    let exports = extract_top_level_exports(&result.arena, result.source_file, &path);
    assert_eq!(
        exports.len(),
        0,
        "bare export-star yields no indexable export"
    );

    let specifiers = collect_star_reexport_specifiers(&result.arena, result.source_file);
    assert_eq!(specifiers, vec!["./b".to_string()]);
}
