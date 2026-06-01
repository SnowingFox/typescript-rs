use super::*;
use crate::BuildInfo;
use tsgo_collections::Set;
use tsgo_tspath::Path;

fn p(s: &str) -> Path {
    Path(s.to_string())
}

// Go: internal/execute/incremental/affectedfileshandler.go:getFilesAffectedBy
// (A imports B; B changes -> {B, A}; unrelated C -> {C})
#[test]
fn affected_set_includes_direct_referrers() {
    let mut map = ReferenceMap::new();
    map.store_references(p("/a.ts"), Set::from_items([p("/b.ts")]));

    // B changed -> B and its referrer A are affected.
    assert_eq!(
        get_files_affected_by(&map, &p("/b.ts")),
        vec![p("/a.ts"), p("/b.ts")]
    );
    // A changed -> only A (nothing references A).
    assert_eq!(get_files_affected_by(&map, &p("/a.ts")), vec![p("/a.ts")]);
    // Unrelated C -> only C.
    assert_eq!(get_files_affected_by(&map, &p("/c.ts")), vec![p("/c.ts")]);
}

// Go: internal/execute/incremental/affectedfileshandler.go:forEachFileReferencedBy
// (transitive: A imports B imports C; C changes -> {C, B, A})
#[test]
fn affected_set_is_transitive() {
    let mut map = ReferenceMap::new();
    map.store_references(p("/a.ts"), Set::from_items([p("/b.ts")]));
    map.store_references(p("/b.ts"), Set::from_items([p("/c.ts")]));

    assert_eq!(
        get_files_affected_by(&map, &p("/c.ts")),
        vec![p("/a.ts"), p("/b.ts"), p("/c.ts")]
    );
    // Changing B only affects B and A (not C, which B depends on).
    assert_eq!(
        get_files_affected_by(&map, &p("/b.ts")),
        vec![p("/a.ts"), p("/b.ts")]
    );
}

// Go: internal/execute/incremental/affectedfileshandler.go (cycle terminates)
#[test]
fn affected_set_terminates_on_cycle() {
    // A imports B, B imports A (cycle).
    let mut map = ReferenceMap::new();
    map.store_references(p("/a.ts"), Set::from_items([p("/b.ts")]));
    map.store_references(p("/b.ts"), Set::from_items([p("/a.ts")]));

    assert_eq!(
        get_files_affected_by(&map, &p("/a.ts")),
        vec![p("/a.ts"), p("/b.ts")]
    );
}

// Go: internal/execute/incremental/programtosnapshot.go (referencedMap from build info)
// Ground truth: real `.tsbuildinfo` where a.ts (id 2) imports b.ts (id 1).
#[test]
fn affected_set_from_build_info_referenced_map() {
    let json = concat!(
        r#"{"version":"7.0.0-dev","root":[[1,2]],"fileNames":["./b.ts","./a.ts"],"#,
        r#""fileInfos":["90312e1cbc42534115cfa9601aa41950","b6df5f2b27e276d9e3e67069347c11a5"],"#,
        r#""fileIdsList":[[1]],"referencedMap":[[2,1]]}"#,
    );
    let build_info: BuildInfo = serde_json::from_str(json).unwrap();
    let map = build_info.reference_map_by_name();

    // Changing b.ts affects b.ts and its importer a.ts.
    assert_eq!(
        get_files_affected_by(&map, &p("./b.ts")),
        vec![p("./a.ts"), p("./b.ts")]
    );
    // Changing a.ts affects only a.ts.
    assert_eq!(get_files_affected_by(&map, &p("./a.ts")), vec![p("./a.ts")]);
}

// Go: internal/execute/incremental/affectedfileshandler.go:collectAllAffectedFiles
#[test]
fn collect_all_affected_files_unions_changed_set() {
    let mut map = ReferenceMap::new();
    map.store_references(p("/a.ts"), Set::from_items([p("/b.ts")]));
    map.store_references(p("/x.ts"), Set::from_items([p("/y.ts")]));

    assert_eq!(
        collect_all_affected_files(&map, &[p("/b.ts"), p("/y.ts")]),
        vec![p("/a.ts"), p("/b.ts"), p("/x.ts"), p("/y.ts")]
    );
}
