use std::sync::Arc;

use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use super::*;
use crate::host::new_compiler_host;

/// Builds a [`CompilerHost`](crate::host::CompilerHost) over an in-memory file
/// system seeded with `(path, contents)` pairs (case-sensitive), rooted at
/// `cwd`.
fn host_with(cwd: &str, files: &[(&str, &str)]) -> impl CompilerHost {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.to_vec(), true));
    new_compiler_host(cwd, fs, "/bundled/libs")
}

// Go: internal/compiler/host.go:compilerHost.GetResolvedProjectReference
#[test]
fn get_resolved_project_reference_parses_config() {
    let host = host_with(
        "/app",
        &[
            (
                "/lib/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["index.ts"] }"#,
            ),
            ("/lib/index.ts", "export const x = 1;"),
        ],
    );

    let resolved =
        get_resolved_project_reference(&host, "/lib/tsconfig.json").expect("config should resolve");
    assert!(resolved.compiler_options().composite.is_true());
    assert_eq!(resolved.file_names(), &["/lib/index.ts".to_string()]);

    // A missing config resolves to `None`.
    assert!(get_resolved_project_reference(&host, "/missing/tsconfig.json").is_none());
}

// Go: internal/compiler/projectreferenceparser.go:projectReferenceParser.parse
#[test]
fn resolves_single_project_reference() {
    let host = host_with(
        "/app",
        &[
            (
                "/app/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../lib" }] }"#,
            ),
            ("/app/a.ts", "export const a = 1;"),
            (
                "/lib/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["index.ts"] }"#,
            ),
            ("/lib/index.ts", "export const x = 1;"),
        ],
    );

    let root = get_resolved_project_reference(&host, "/app/tsconfig.json").expect("root resolves");
    let graph = resolve_project_references(&host, "/app/tsconfig.json", &root);

    let refs = graph.get_resolved_project_references();
    assert_eq!(refs.len(), 1, "root has one direct reference");
    let lib = refs[0].expect("../lib resolves to /lib/tsconfig.json");
    assert!(lib.compiler_options().composite.is_true());
    assert_eq!(lib.file_names(), &["/lib/index.ts".to_string()]);

    // The same config is reachable by its canonical path.
    let lib_path = graph.to_path("/lib/tsconfig.json");
    let by_path = graph
        .get_resolved_reference_for(&lib_path)
        .expect("lib reachable by path");
    assert_eq!(by_path.file_names(), &["/lib/index.ts".to_string()]);
    // An unreached config is `None`.
    let other = graph.to_path("/nope/tsconfig.json");
    assert!(graph.get_resolved_reference_for(&other).is_none());
}

/// Lays down three composite projects A -> B -> C (each referencing the next)
/// rooted at `/proj`, returning a host over them.
fn linear_chain_host() -> impl CompilerHost {
    host_with(
        "/proj",
        &[
            (
                "/proj/a/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../b" }] }"#,
            ),
            ("/proj/a/a.ts", "export const a = 1;"),
            (
                "/proj/b/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["b.ts"], "references": [{ "path": "../c" }] }"#,
            ),
            ("/proj/b/b.ts", "export const b = 1;"),
            (
                "/proj/c/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["c.ts"] }"#,
            ),
            ("/proj/c/c.ts", "export const c = 1;"),
        ],
    )
}

// Go: internal/execute/build/orchestrator.go:Orchestrator.setupBuildTask
#[test]
fn build_order_is_topological() {
    let host = linear_chain_host();
    let root = get_resolved_project_reference(&host, "/proj/a/tsconfig.json").expect("root");
    let graph = resolve_project_references(&host, "/proj/a/tsconfig.json", &root);

    let build = graph.get_build_order();
    assert_eq!(
        build.order,
        vec![
            "/proj/c/tsconfig.json".to_string(),
            "/proj/b/tsconfig.json".to_string(),
            "/proj/a/tsconfig.json".to_string(),
        ],
        "deepest reference (C) is built first, the root (A) last"
    );
    assert!(build.circular_diagnostics.is_empty());
}

// Go: internal/execute/build/orchestrator.go:Orchestrator.setupBuildTask (cycle)
#[test]
fn build_order_reports_circular_graph() {
    // A -> B -> A (each composite, referencing the other).
    let host = host_with(
        "/proj",
        &[
            (
                "/proj/a/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../b" }] }"#,
            ),
            ("/proj/a/a.ts", "export const a = 1;"),
            (
                "/proj/b/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["b.ts"], "references": [{ "path": "../a" }] }"#,
            ),
            ("/proj/b/b.ts", "export const b = 1;"),
        ],
    );
    let root = get_resolved_project_reference(&host, "/proj/a/tsconfig.json").expect("root");
    let graph = resolve_project_references(&host, "/proj/a/tsconfig.json", &root);

    let build = graph.get_build_order();
    assert_eq!(build.circular_diagnostics.len(), 1, "one cycle detected");
    let diag = &build.circular_diagnostics[0];
    assert_eq!(diag.code(), 6202);
    assert_eq!(
        diag.text(),
        "Project references may not form a circular graph. Cycle detected: \
         /proj/a/tsconfig.json\n/proj/b/tsconfig.json"
    );
}

// Go: internal/execute/build/orchestrator.go:Orchestrator.setupBuildTask (3-node cycle)
#[test]
fn build_order_reports_circular_graph_three_nodes() {
    // A -> B -> C -> A; the cycle message lists the full analyzing chain.
    let host = host_with(
        "/proj",
        &[
            (
                "/proj/a/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../b" }] }"#,
            ),
            ("/proj/a/a.ts", "export const a = 1;"),
            (
                "/proj/b/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["b.ts"], "references": [{ "path": "../c" }] }"#,
            ),
            ("/proj/b/b.ts", "export const b = 1;"),
            (
                "/proj/c/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["c.ts"], "references": [{ "path": "../a" }] }"#,
            ),
            ("/proj/c/c.ts", "export const c = 1;"),
        ],
    );
    let root = get_resolved_project_reference(&host, "/proj/a/tsconfig.json").expect("root");
    let graph = resolve_project_references(&host, "/proj/a/tsconfig.json", &root);

    let build = graph.get_build_order();
    assert_eq!(build.circular_diagnostics.len(), 1);
    assert_eq!(
        build.circular_diagnostics[0].text(),
        "Project references may not form a circular graph. Cycle detected: \
         /proj/a/tsconfig.json\n/proj/b/tsconfig.json\n/proj/c/tsconfig.json"
    );
    // Even with a cycle the order still collects each reachable project once.
    assert_eq!(
        build.order,
        vec![
            "/proj/c/tsconfig.json".to_string(),
            "/proj/b/tsconfig.json".to_string(),
            "/proj/a/tsconfig.json".to_string(),
        ]
    );
}

/// Builds a host where `/app` references `/lib`; `/lib` is composite iff
/// `lib_composite`.
fn app_referencing_lib(lib_composite: bool) -> impl CompilerHost {
    let lib_options = if lib_composite {
        r#"{ "composite": true }"#
    } else {
        "{}"
    };
    let lib_config = format!(r#"{{ "compilerOptions": {lib_options}, "files": ["index.ts"] }}"#);
    let files: Vec<(String, String)> = vec![
        (
            "/app/tsconfig.json".to_string(),
            r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../lib" }] }"#.to_string(),
        ),
        ("/app/a.ts".to_string(), "export const a = 1;".to_string()),
        ("/lib/tsconfig.json".to_string(), lib_config),
        ("/lib/index.ts".to_string(), "export const x = 1;".to_string()),
    ];
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(refs, true));
    new_compiler_host("/app", fs, "/bundled/libs")
}

// Go: internal/compiler/program.go:verifyProjectReferences (composite required)
#[test]
fn verify_reports_missing_composite() {
    let host = app_referencing_lib(false);
    let root = get_resolved_project_reference(&host, "/app/tsconfig.json").expect("root");
    let graph = resolve_project_references(&host, "/app/tsconfig.json", &root);

    let diags = graph.verify_project_references();
    assert_eq!(diags.len(), 1, "non-composite reference reported");
    assert_eq!(diags[0].code(), 6306);
    assert_eq!(
        diags[0].text(),
        "Referenced project '/lib' must have setting \"composite\": true."
    );
}

// Go: internal/compiler/program.go:verifyProjectReferences (composite satisfied)
#[test]
fn verify_accepts_composite_reference() {
    let host = app_referencing_lib(true);
    let root = get_resolved_project_reference(&host, "/app/tsconfig.json").expect("root");
    let graph = resolve_project_references(&host, "/app/tsconfig.json", &root);

    assert!(
        graph.verify_project_references().is_empty(),
        "a composite reference is accepted"
    );
}

// Go: internal/tsoptions/parsedcommandline.go:getOutputDeclarationAndSourceFileNames
#[test]
fn computes_reference_output_paths_with_out_dir_and_root_dir() {
    // `/app` references a composite `/lib` whose sources under `./src` emit to
    // `./bin` (verified against `cmd/tsgo -p` emit: src/index.ts -> bin/index.js
    // + bin/index.d.ts, src/sub/util.ts -> bin/sub/util.{js,d.ts}).
    let host = host_with(
        "/app",
        &[
            (
                "/app/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../lib" }] }"#,
            ),
            ("/app/a.ts", "export const a = 1;"),
            (
                "/lib/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true, "declaration": true, "outDir": "./bin", "rootDir": "./src" }, "files": ["src/index.ts", "src/sub/util.ts"] }"#,
            ),
            ("/lib/src/index.ts", "export const index = 1;"),
            ("/lib/src/sub/util.ts", "export const util = 2;"),
        ],
    );

    let root = get_resolved_project_reference(&host, "/app/tsconfig.json").expect("root");
    let graph = resolve_project_references(&host, "/app/tsconfig.json", &root);
    let lib = graph.get_resolved_project_references()[0].expect("lib resolves");

    assert_eq!(
        get_output_js_file_name(lib, "/lib/src/index.ts"),
        "/lib/bin/index.js"
    );
    assert_eq!(
        get_output_declaration_file_name(lib, "/lib/src/index.ts"),
        "/lib/bin/index.d.ts"
    );
    // A nested source keeps its relative layout under `outDir`.
    assert_eq!(
        get_output_js_file_name(lib, "/lib/src/sub/util.ts"),
        "/lib/bin/sub/util.js"
    );
    assert_eq!(
        get_output_declaration_file_name(lib, "/lib/src/sub/util.ts"),
        "/lib/bin/sub/util.d.ts"
    );
}

// Go: internal/tsoptions/parsedcommandline.go:checkSourceFilesBelongToPath (TS6059)
#[test]
fn reports_file_outside_root_dir() {
    // `other/x.ts` lies outside `rootDir: ./src` -> TS6059 (NOT TS6307; see
    // the function doc + the cmd/tsgo `-p` capture).
    let host = host_with(
        "/proj",
        &[
            (
                "/proj/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true, "outDir": "./bin", "rootDir": "./src", "declaration": true }, "files": ["src/index.ts", "other/x.ts"] }"#,
            ),
            ("/proj/src/index.ts", "export const index = 1;"),
            ("/proj/other/x.ts", "export const x = 3;"),
        ],
    );
    let config = get_resolved_project_reference(&host, "/proj/tsconfig.json").expect("config");

    let diags = check_source_files_belong_to_root_dir(&config);
    assert_eq!(diags.len(), 1, "one file outside rootDir");
    assert_eq!(diags[0].code(), 6059);
    assert_eq!(
        diags[0].text(),
        "File '/proj/other/x.ts' is not under 'rootDir' '/proj/src'. \
         'rootDir' is expected to contain all source files."
    );
}

// Go: internal/tsoptions/parsedcommandline.go:checkSourceFilesBelongToPath (all inside)
#[test]
fn accepts_files_inside_root_dir() {
    let host = host_with(
        "/proj",
        &[
            (
                "/proj/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true, "outDir": "./bin", "rootDir": "./src", "declaration": true }, "files": ["src/index.ts", "src/sub/util.ts"] }"#,
            ),
            ("/proj/src/index.ts", "export const index = 1;"),
            ("/proj/src/sub/util.ts", "export const util = 2;"),
        ],
    );
    let config = get_resolved_project_reference(&host, "/proj/tsconfig.json").expect("config");
    assert!(
        check_source_files_belong_to_root_dir(&config).is_empty(),
        "all sources under rootDir"
    );
}
