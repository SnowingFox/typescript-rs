use super::*;

// Go: internal/execute/incremental/snapshot.go:ComputeHash
// Ground truth: `cmd/tsgo --incremental` writes these exact version hashes into
// `.tsbuildinfo` for the two-file project (`b.ts`, `a.ts`).
#[test]
fn compute_hash_matches_go_tsbuildinfo_versions() {
    assert_eq!(
        compute_hash("export const b = 1;\n", false),
        "90312e1cbc42534115cfa9601aa41950"
    );
    assert_eq!(
        compute_hash(
            "import { b } from \"./b\";\nexport const a = b + 1;\n",
            false
        ),
        "b6df5f2b27e276d9e3e67069347c11a5"
    );
}

// Go: internal/execute/incremental/programtosnapshot.go:computeProgramFileChanges
// (version := t.snapshot.computeHash(file.Text()))
#[test]
fn compute_file_version_is_stable_and_text_sensitive() {
    let v1 = compute_file_version("const x = 1;");
    // Stable: same text -> same hash.
    assert_eq!(v1, compute_file_version("const x = 1;"));
    // It is exactly the text hash (no `hash_with_text`).
    assert_eq!(v1, compute_hash("const x = 1;", false));
    // Text-sensitive: changing the text changes the version.
    assert_ne!(v1, compute_file_version("const x = 2;"));
}

// Go: internal/execute/incremental/snapshot.go:computeSignatureWithDiagnostics
// (reachable text-based d.ts signature approximation)
#[test]
fn compute_signature_hashes_declaration_text() {
    let dts = "export declare const x: number;\n";
    assert_eq!(compute_signature(dts), compute_hash(dts, false));
    // A different declaration shape yields a different signature.
    assert_ne!(
        compute_signature(dts),
        compute_signature("export declare const x: string;\n")
    );
}

// Go: internal/execute/incremental/programtosnapshot.go:computeProgramFileChanges
// (fresh build: signature = version)
#[test]
fn fresh_file_info_uses_version_as_signature() {
    let info = FileInfo::for_fresh_text("export const b = 1;\n", false, RESOLUTION_MODE_COMMON_JS);
    assert_eq!(info.version, "90312e1cbc42534115cfa9601aa41950");
    // On a fresh build there is no prior d.ts signature, so it defaults to version.
    assert_eq!(info.signature, info.version);
    assert!(!info.affects_global_scope);
    assert_eq!(info.implied_node_format, RESOLUTION_MODE_COMMON_JS);
}
