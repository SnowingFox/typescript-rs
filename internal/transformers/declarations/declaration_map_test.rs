use super::*;

// ── T4-終 slice 7: declaration map stub ─────────────────────────────────────
// The DeclarationMapGenerator wraps the sourcemap generator to produce a
// `.d.ts.map` file. The stub generates a structurally valid source map with
// the correct `file` and `sources` fields.

// The generated source map has version 3 and the correct `file` field (the
// `.d.ts` file name without directory).
#[test]
fn declaration_map_has_correct_version_and_file() {
    let mut gen = DeclarationMapGenerator::new("/out/main.d.ts", "/src/main.ts");
    let map = gen.to_raw_source_map();
    assert_eq!(map.version, 3);
    assert_eq!(map.file, "main.d.ts");
}

// The `sources` array contains the original `.ts` file path, relative to the
// `.d.ts` output directory.
#[test]
fn declaration_map_has_relative_source_path() {
    let mut gen = DeclarationMapGenerator::new("/out/main.d.ts", "/src/main.ts");
    let map = gen.to_raw_source_map();
    assert_eq!(map.sources.len(), 1);
    assert_eq!(map.sources[0], "../src/main.ts");
}

// The source index is 0 for a single-file emit.
#[test]
fn declaration_map_source_index_is_zero() {
    let gen = DeclarationMapGenerator::new("/out/main.d.ts", "/src/main.ts");
    assert_eq!(gen.source_index(), SourceIndex(0));
}

// A declaration map for a nested output path computes the correct relative
// source path.
#[test]
fn declaration_map_nested_paths() {
    let mut gen = DeclarationMapGenerator::new(
        "/project/dist/types/utils/helper.d.ts",
        "/project/src/utils/helper.ts",
    );
    let map = gen.to_raw_source_map();
    assert_eq!(map.file, "helper.d.ts");
    assert_eq!(map.sources[0], "../../../src/utils/helper.ts");
}
