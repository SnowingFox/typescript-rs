use super::*;

use tsgo_core::tristate::Tristate;

/// Minimal in-memory [`OutputPathsHost`] for behavior tests (no file I/O).
struct FakeHost {
    common_source_directory: String,
    current_directory: String,
    use_case_sensitive_file_names: bool,
}

impl OutputPathsHost for FakeHost {
    fn common_source_directory(&self) -> String {
        self.common_source_directory.clone()
    }
    fn get_current_directory(&self) -> String {
        self.current_directory.clone()
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        self.use_case_sensitive_file_names
    }
}

/// Minimal [`SourceFileLike`] carrying just a name and script kind.
struct FakeSourceFile {
    file_name: String,
    script_kind: ScriptKind,
}

impl SourceFileLike for FakeSourceFile {
    fn file_name(&self) -> &str {
        &self.file_name
    }
    fn script_kind(&self) -> ScriptKind {
        self.script_kind
    }
}

// Go: internal/outputpaths/outputpaths.go:getOwnEmitOutputFilePath (outDir re-root)
#[test]
fn js_path_with_outdir() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let source_file = FakeSourceFile {
        file_name: "/src/a/b.ts".into(),
        script_kind: ScriptKind::Ts,
    };
    let options = CompilerOptions {
        out_dir: "/out".into(),
        ..Default::default()
    };
    let paths = get_output_paths_for(&source_file, &options, &host, false);
    assert_eq!(paths.js_file_path(), "/out/a/b.js");
}

// Go: internal/outputpaths/outputpaths.go:GetDeclarationEmitOutputFilePath (declarationDir wins)
#[test]
fn decl_path_with_declaration_dir() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions {
        out_dir: "/out".into(),
        declaration_dir: "/types".into(),
        ..Default::default()
    };
    assert_eq!(
        get_declaration_emit_output_file_path("/src/a/b.ts", &options, &host),
        "/types/a/b.d.ts"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetOutputPathsFor (declarationMapPath = decl + ".map")
#[test]
fn decl_map_path() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let source_file = FakeSourceFile {
        file_name: "/src/a/b.ts".into(),
        script_kind: ScriptKind::Ts,
    };
    let options = CompilerOptions {
        declaration: Tristate::True,
        declaration_map: Tristate::True,
        ..Default::default()
    };
    let paths = get_output_paths_for(&source_file, &options, &host, false);
    assert_eq!(paths.declaration_file_path(), "/src/a/b.d.ts");
    assert_eq!(paths.declaration_map_path(), "/src/a/b.d.ts.map");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputPathsFor (emitDeclarationOnly suppresses js)
#[test]
fn emit_declaration_only_no_js() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let source_file = FakeSourceFile {
        file_name: "/src/a/b.ts".into(),
        script_kind: ScriptKind::Ts,
    };
    let options = CompilerOptions {
        emit_declaration_only: Tristate::True,
        ..Default::default()
    };
    let paths = get_output_paths_for(&source_file, &options, &host, false);
    assert_eq!(paths.js_file_path(), "");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputPathsFor (json same-location skip)
#[test]
fn json_emitted_same_location_skip() {
    let host = FakeHost {
        common_source_directory: "/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let source_file = FakeSourceFile {
        file_name: "/a.json".into(),
        script_kind: ScriptKind::Json,
    };
    let options = CompilerOptions::default();
    let paths = get_output_paths_for(&source_file, &options, &host, false);
    assert_eq!(paths.js_file_path(), "");
}

// Go: internal/outputpaths/outputpaths.go:ForEachEmittedFile (visits every file)
#[test]
fn for_each_emitted_file_visits_all() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions {
        out_dir: "/out".into(),
        ..Default::default()
    };
    let files = vec![
        FakeSourceFile {
            file_name: "/src/a.ts".into(),
            script_kind: ScriptKind::Ts,
        },
        FakeSourceFile {
            file_name: "/src/b.ts".into(),
            script_kind: ScriptKind::Ts,
        },
    ];
    let mut visited: Vec<String> = Vec::new();
    let result = for_each_emitted_file(
        &host,
        &options,
        |paths, _sf| {
            visited.push(paths.js_file_path().to_string());
            false
        },
        &files,
        false,
    );
    assert!(!result);
    assert_eq!(
        visited,
        vec!["/out/a.js".to_string(), "/out/b.js".to_string()]
    );
}

// Go: internal/outputpaths/outputpaths.go:ForEachEmittedFile (short-circuits on true)
#[test]
fn for_each_emitted_file_short_circuits() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions {
        out_dir: "/out".into(),
        ..Default::default()
    };
    let files = vec![
        FakeSourceFile {
            file_name: "/src/a.ts".into(),
            script_kind: ScriptKind::Ts,
        },
        FakeSourceFile {
            file_name: "/src/b.ts".into(),
            script_kind: ScriptKind::Ts,
        },
    ];
    let mut count = 0;
    let result = for_each_emitted_file(
        &host,
        &options,
        |_paths, _sf| {
            count += 1;
            true
        },
        &files,
        false,
    );
    assert!(result);
    assert_eq!(count, 1);
}

// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName (not incremental/build -> "")
#[test]
fn build_info_none_when_not_incremental() {
    let opts = ComparePathsOptions {
        use_case_sensitive_file_names: true,
        current_directory: "/".into(),
    };
    let options = CompilerOptions::default();
    assert_eq!(get_build_info_file_name(&options, &opts), "");
}

// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName (explicit tsBuildInfoFile)
#[test]
fn build_info_explicit_file() {
    let opts = ComparePathsOptions {
        use_case_sensitive_file_names: true,
        current_directory: "/".into(),
    };
    let options = CompilerOptions {
        incremental: Tristate::True,
        ts_build_info_file: "/x.tsbuildinfo".into(),
        ..Default::default()
    };
    assert_eq!(get_build_info_file_name(&options, &opts), "/x.tsbuildinfo");
}

// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName (outDir + rootDir remap)
#[test]
fn build_info_from_config_outdir_rootdir() {
    let opts = ComparePathsOptions {
        use_case_sensitive_file_names: true,
        current_directory: "/".into(),
    };
    let options = CompilerOptions {
        incremental: Tristate::True,
        config_file_path: "/p/tsconfig.json".into(),
        out_dir: "/out".into(),
        root_dir: "/p".into(),
        ..Default::default()
    };
    assert_eq!(
        get_build_info_file_name(&options, &opts),
        "/out/tsconfig.tsbuildinfo"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName (outDir, no rootDir -> base name)
#[test]
fn build_info_from_config_outdir_no_rootdir() {
    let opts = ComparePathsOptions {
        use_case_sensitive_file_names: true,
        current_directory: "/".into(),
    };
    let options = CompilerOptions {
        incremental: Tristate::True,
        config_file_path: "/p/sub/tsconfig.json".into(),
        out_dir: "/out".into(),
        ..Default::default()
    };
    assert_eq!(
        get_build_info_file_name(&options, &opts),
        "/out/tsconfig.tsbuildinfo"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName (no outDir -> config dir)
#[test]
fn build_info_from_config_no_outdir() {
    let opts = ComparePathsOptions {
        use_case_sensitive_file_names: true,
        current_directory: "/".into(),
    };
    let options = CompilerOptions {
        incremental: Tristate::True,
        config_file_path: "/p/tsconfig.json".into(),
        ..Default::default()
    };
    assert_eq!(
        get_build_info_file_name(&options, &opts),
        "/p/tsconfig.tsbuildinfo"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName (incremental but no config -> "")
#[test]
fn build_info_incremental_no_config() {
    let opts = ComparePathsOptions {
        use_case_sensitive_file_names: true,
        current_directory: "/".into(),
    };
    let options = CompilerOptions {
        incremental: Tristate::True,
        ..Default::default()
    };
    assert_eq!(get_build_info_file_name(&options, &opts), "");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputJSFileName (outDir re-root)
#[test]
fn get_output_js_file_name_outdir() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions {
        out_dir: "/out".into(),
        ..Default::default()
    };
    assert_eq!(
        get_output_js_file_name("/src/a.ts", &options, &host),
        "/out/a.js"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetOutputJSFileName (emitDeclarationOnly -> "")
#[test]
fn get_output_js_file_name_emit_declaration_only() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions {
        emit_declaration_only: Tristate::True,
        ..Default::default()
    };
    assert_eq!(get_output_js_file_name("/src/a.ts", &options, &host), "");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputJSFileName (json written to same location -> "")
#[test]
fn get_output_js_file_name_json_same_location() {
    let host = FakeHost {
        common_source_directory: "/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions::default();
    assert_eq!(get_output_js_file_name("/a.json", &options, &host), "");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputJSFileNameWorker
#[test]
fn get_output_js_file_name_worker_outdir() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions {
        out_dir: "/out".into(),
        ..Default::default()
    };
    assert_eq!(
        get_output_js_file_name_worker("/src/a/b.ts", &options, &host),
        "/out/a/b.js"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetOutputDeclarationFileNameWorker
#[test]
fn get_output_declaration_file_name_worker_outdir() {
    let host = FakeHost {
        common_source_directory: "/src/".into(),
        current_directory: "/".into(),
        use_case_sensitive_file_names: true,
    };
    let options = CompilerOptions {
        out_dir: "/out".into(),
        ..Default::default()
    };
    assert_eq!(
        get_output_declaration_file_name_worker("/src/a/b.ts", &options, &host),
        "/out/a/b.d.ts"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetSourceMapFilePath (sourceMap on)
#[test]
fn source_map_path_enabled() {
    let options = CompilerOptions {
        source_map: Tristate::True,
        ..Default::default()
    };
    assert_eq!(
        get_source_map_file_path("/out/a.js", &options),
        "/out/a.js.map"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetSourceMapFilePath (inlineSourceMap suppresses)
#[test]
fn source_map_path_inline_off() {
    let options = CompilerOptions {
        source_map: Tristate::True,
        inline_source_map: Tristate::True,
        ..Default::default()
    };
    assert_eq!(get_source_map_file_path("/out/a.js", &options), "");
}

// The two re-rooting functions must keep their *different* prefix tests:
// `GetSourceFilePathInNewDir` uses component-wise `ContainsPath` (after adding a
// trailing separator), while `GetSourceFilePathInNewDirWorker` uses a raw
// `strings.HasPrefix`. With `commonSourceDirectory = "/src"` (no separator) and
// `file = "/src2/a.ts"`, the two disagree: `ContainsPath` rejects "/src2/..."
// while `HasPrefix("/src2/a.ts", "/src")` accepts it. The pair below pins that
// divergence so the functions can never be silently unified.

// Go: internal/outputpaths/outputpaths.go:GetSourceFilePathInNewDir
#[test]
fn source_file_path_in_new_dir_uses_contains_path() {
    // "/src2" is not contained in "/src/" component-wise, so the path is kept
    // absolute and CombinePaths returns it unchanged.
    assert_eq!(
        get_source_file_path_in_new_dir("/src2/a.ts", "/out", "/", "/src", true),
        "/src2/a.ts"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetSourceFilePathInNewDirWorker
#[test]
fn source_file_path_in_new_dir_worker_uses_has_prefix() {
    // "/src2/a.ts" string-starts-with "/src", so 4 bytes are stripped, yielding
    // "2/a.ts" which is then combined under "/out".
    assert_eq!(
        get_source_file_path_in_new_dir_worker("/src2/a.ts", "/out", "/", "/src", true),
        "/out/2/a.ts"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetSourceFilePathInNewDir (inside common dir)
#[test]
fn source_file_path_in_new_dir_inside_common_dir() {
    assert_eq!(
        get_source_file_path_in_new_dir("/src/a/b.ts", "/out", "/", "/src/", true),
        "/out/a/b.ts"
    );
}

// Go: internal/outputpaths/outputpaths.go:GetOutputExtension (default branch)
#[test]
fn get_output_extension_js() {
    assert_eq!(get_output_extension("/a/b.ts", JsxEmit::None), ".js");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputExtension (json branch)
#[test]
fn get_output_extension_json() {
    assert_eq!(get_output_extension("/a/b.json", JsxEmit::None), ".json");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputExtension (jsx preserve branch)
#[test]
fn get_output_extension_jsx_preserve() {
    assert_eq!(get_output_extension("/a/b.tsx", JsxEmit::Preserve), ".jsx");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputExtension (mts/mjs branch)
#[test]
fn get_output_extension_mts() {
    assert_eq!(get_output_extension("/a/b.mts", JsxEmit::None), ".mjs");
}

// Go: internal/outputpaths/outputpaths.go:GetOutputExtension (cts/cjs branch)
#[test]
fn get_output_extension_cts() {
    assert_eq!(get_output_extension("/a/b.cts", JsxEmit::None), ".cjs");
}
