use super::*;
use crate::test_support::parse_shared;

// Collects the analysis for a parsed module `input`.
fn analyze(input: &str) -> ExternalModuleInfo {
    let (ec, source_file) = parse_shared(input);
    let ecb = ec.borrow();
    collect_external_module_info(ecb.arena(), source_file)
}

// Go: externalmoduleinfo.go:collect (KindImportDeclaration)
// Tracer bullet: a named import is recorded as one external import.
#[test]
fn named_import_is_an_external_import() {
    let info = analyze("import { x } from \"m\";");
    assert_eq!(info.external_imports.len(), 1);
}

// Go: externalmoduleinfo.go:collect (KindExportDeclaration, ExportClause == nil)
// `export * from "m"` is an external import and sets the export-star flag.
#[test]
fn export_star_sets_flag_and_is_external_import() {
    let info = analyze("export * from \"m\";");
    assert_eq!(info.external_imports.len(), 1);
    assert!(info.has_export_stars_to_export_values);
}

// Go: externalmoduleinfo.go:addExportedNamesForExportDeclaration
// `export { x }` (no module specifier) records `x` as an exported name and is
// NOT an external import.
#[test]
fn local_named_export_records_exported_name() {
    let (ec, source_file) = parse_shared("const x = 1; export { x };");
    let ecb = ec.borrow();
    let info = collect_external_module_info(ecb.arena(), source_file);
    assert_eq!(info.external_imports.len(), 0);
    assert_eq!(info.exported_names.len(), 1);
    assert_eq!(ecb.arena().text(info.exported_names[0]), "x");
}

// Go: externalmoduleinfo.go:collect (KindExportAssignment, IsExportEquals)
// `export = x` is recorded as the module's `export =` assignment.
#[test]
fn export_equals_is_recorded() {
    let info = analyze("const x = 1; export = x;");
    assert!(info.export_equals.is_some());
}

// Go: externalmoduleinfo.go:collectExportedVariableInfo
// `export const y = 1` records `y` as an exported name.
#[test]
fn exported_const_records_exported_name() {
    let (ec, source_file) = parse_shared("export const y = 1;");
    let ecb = ec.borrow();
    let info = collect_external_module_info(ecb.arena(), source_file);
    assert_eq!(info.exported_names.len(), 1);
    assert_eq!(ecb.arena().text(info.exported_names[0]), "y");
}
