use super::*;

// Go: internal/ls/autoimport/registry.go:registryBuilder.buildProjectBucket
// (reachable AST analog; task TDD slice 3)
#[test]
fn multi_file_index_maps_name_to_file() {
    let files = [
        FileInput::new("/src/a.ts", "export const a = 1;\n"),
        FileInput::new("/src/b.ts", "export function b(){}\n"),
    ];
    let idx = build_index_for_files(&files, "/", true);

    let a = idx.find("a", true);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].id.module_id, ModuleId::new("/src/a.ts"));

    let b = idx.find("b", true);
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].id.module_id, ModuleId::new("/src/b.ts"));

    // A name exported by neither file is absent.
    assert_eq!(idx.find("c", true).len(), 0);
}

// Each file's own exports are independent (three files, three names).
#[test]
fn three_files_three_names() {
    let files = [
        FileInput::new("/src/a.ts", "export const alpha = 1;\n"),
        FileInput::new("/src/b.ts", "export class Beta {}\n"),
        FileInput::new("/src/c.ts", "export function gamma(){}\n"),
    ];
    let idx = build_index_for_files(&files, "/", true);
    assert_eq!(
        idx.find("alpha", true)[0].id.module_id,
        ModuleId::new("/src/a.ts")
    );
    assert_eq!(
        idx.find("Beta", true)[0].id.module_id,
        ModuleId::new("/src/b.ts")
    );
    assert_eq!(
        idx.find("gamma", true)[0].id.module_id,
        ModuleId::new("/src/c.ts")
    );
}

// Reachable cross-file `export *` resolution: `a` re-exports everything from
// `b`, so `bVal` is importable from both `b` (directly) and `a` (re-exported).
#[test]
fn resolves_export_star_across_files() {
    let files = [
        FileInput::new("/src/a.ts", "export * from \"./b\";\n"),
        FileInput::new("/src/b.ts", "export const bVal = 1;\n"),
    ];
    let idx = build_index_for_files(&files, "/", true);
    let found = idx.find("bVal", true);
    assert!(
        found
            .iter()
            .any(|e| e.id.module_id == ModuleId::new("/src/b.ts")),
        "bVal should be importable directly from b"
    );
    let reexported = found
        .iter()
        .find(|e| e.id.module_id == ModuleId::new("/src/a.ts"))
        .expect("bVal should be re-exported through a via export-star");
    assert_eq!(reexported.syntax, ExportSyntax::Star);
    // The re-export records that it was found through `export *`.
    assert_eq!(
        reexported.through(),
        tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_EXPORT_STAR
    );
    // Its target points back at b's own export.
    assert_eq!(reexported.target.module_id, ModuleId::new("/src/b.ts"));
}

// A nested directory still produces a correct module id.
#[test]
fn nested_directory_paths() {
    let files = [FileInput::new("/src/lib/b.ts", "export const deep = 1;\n")];
    let idx = build_index_for_files(&files, "/", true);
    assert_eq!(
        idx.find("deep", true)[0].id.module_id,
        ModuleId::new("/src/lib/b.ts")
    );
}
