use super::*;
use serde_json::json;

// Ground truth: `cmd/tsgo -p tsconfig.json` (two-file `a.ts` imports `b.ts`,
// `incremental:true`, `noLib:true`) writes exactly this `.tsbuildinfo`.
// Go: internal/execute/incremental/buildInfo.go:BuildInfo (compact JSON)
const GO_TSBUILDINFO: &str = concat!(
    r#"{"version":"7.0.0-dev","errors":true,"root":[[1,2]],"#,
    r#""fileNames":["./b.ts","./a.ts"],"#,
    r#""fileInfos":["90312e1cbc42534115cfa9601aa41950","b6df5f2b27e276d9e3e67069347c11a5"],"#,
    r#""fileIdsList":[[1]],"options":{"module":99,"target":99},"#,
    r#""referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[1,2]}"#,
);

fn sample_build_info() -> BuildInfo {
    let mut options = indexmap::IndexMap::new();
    options.insert("module".to_string(), json!(99));
    options.insert("target".to_string(), json!(99));
    BuildInfo {
        version: "7.0.0-dev".to_string(),
        errors: true,
        root: vec![BuildInfoRoot::range(BuildInfoFileId(1), BuildInfoFileId(2))],
        file_names: vec!["./b.ts".to_string(), "./a.ts".to_string()],
        file_infos: vec![
            BuildInfoFileInfo::signature("90312e1cbc42534115cfa9601aa41950"),
            BuildInfoFileInfo::signature("b6df5f2b27e276d9e3e67069347c11a5"),
        ],
        file_ids_list: vec![vec![BuildInfoFileId(1)]],
        options: Some(options),
        referenced_map: vec![BuildInfoReferenceMapEntry {
            file_id: BuildInfoFileId(2),
            file_id_list_id: BuildInfoFileIdListId(1),
        }],
        semantic_diagnostics_per_file: vec![
            BuildInfoSemanticDiagnostic::file(BuildInfoFileId(1)),
            BuildInfoSemanticDiagnostic::file(BuildInfoFileId(2)),
        ],
        ..BuildInfo::default()
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfo.MarshalJSON (compact shape)
#[test]
fn serializes_to_go_compact_tsbuildinfo_shape() {
    let json = serde_json::to_string(&sample_build_info()).unwrap();
    assert_eq!(json, GO_TSBUILDINFO);
}

// Go: internal/execute/incremental/buildInfo.go (round-trip: marshal -> unmarshal -> equal)
#[test]
fn round_trips_through_json() {
    let bi = sample_build_info();
    let json = serde_json::to_string(&bi).unwrap();
    let parsed: BuildInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, bi);
}

// Go: internal/execute/incremental/buildInfo.go (1-based file ids parsed from real .tsbuildinfo)
#[test]
fn parses_go_tsbuildinfo_with_one_based_file_ids() {
    let bi: BuildInfo = serde_json::from_str(GO_TSBUILDINFO).unwrap();
    assert_eq!(bi.version, "7.0.0-dev");
    assert!(bi.errors);
    assert_eq!(
        bi.file_names,
        vec!["./b.ts".to_string(), "./a.ts".to_string()]
    );
    // referencedMap: file 2 (a.ts) references list 1 == [file 1 (b.ts)]; all ids 1-based.
    let entry = &bi.referenced_map[0];
    assert_eq!(entry.file_id, BuildInfoFileId(2));
    assert_eq!(entry.file_id_list_id, BuildInfoFileIdListId(1));
    assert_eq!(bi.file_ids_list[0], vec![BuildInfoFileId(1)]);
    // Compact bare-string fileInfo decodes back to its version hash.
    assert_eq!(
        bi.file_infos[0].get_file_info().version,
        "90312e1cbc42534115cfa9601aa41950"
    );
    // root [[1,2]] decodes to a start/end range.
    assert_eq!(
        bi.root[0],
        BuildInfoRoot::range(BuildInfoFileId(1), BuildInfoFileId(2))
    );
}
