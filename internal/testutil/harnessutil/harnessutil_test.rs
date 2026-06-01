use super::*;

fn unit(name: &str, content: &str) -> TestFile {
    TestFile {
        unit_name: name.to_string(),
        content: content.to_string(),
    }
}

// Slice 3 (RED->GREEN): a clean inline case compiles with zero diagnostics and
// emits its JavaScript (type annotation erased), recorded as an output.
// Go: internal/testutil/harnessutil/harnessutil.go:CompileFiles (clean case)
#[test]
fn compile_clean_case_emits_js_with_no_diagnostics() {
    let result = compile_files(
        vec![unit("/.src/a.ts", "const x: number = 1;")],
        vec![],
        &TestConfiguration::new(),
        "/.src",
    );

    assert!(
        result.diagnostics().is_empty(),
        "expected no diagnostics, got {:?}",
        result
            .diagnostics()
            .iter()
            .map(|d| (d.code(), d.message().to_string()))
            .collect::<Vec<_>>()
    );

    let js = result.get_output("/.src/a.js").expect("emitted /.src/a.js");
    // The harness defaults to CRLF newlines; the annotation is erased.
    assert_eq!(js.content, "const x = 1;\r\n");
}

// Slice 4 (RED->GREEN): an errored inline case surfaces the checker's TS2322.
// Go: internal/testutil/harnessutil/harnessutil.go:CompileFiles (errored case)
#[test]
fn compile_errored_case_reports_ts2322() {
    let result = compile_files(
        vec![unit("/.src/a.ts", "var x: number = \"s\";")],
        vec![],
        &TestConfiguration::new(),
        "/.src",
    );

    let diags = result.diagnostics();
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
    assert_eq!(diags[0].code(), 2322);
    assert_eq!(
        diags[0].message(),
        "Type 'string' is not assignable to type 'number'."
    );
    // The diagnostic is attributed to the user source file.
    assert_eq!(diags[0].file_name(), Some("/.src/a.ts"));
}

// `compile_files_ex` honors explicit options: `--noEmit` skips emit.
// Go: internal/testutil/harnessutil/harnessutil.go:CompileFilesEx (noEmit)
#[test]
fn compile_files_ex_no_emit_skips_output() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;

    let options = CompilerOptions {
        no_emit: Tristate::True,
        ..Default::default()
    };

    let result = compile_files_ex(
        vec![unit("/.src/a.ts", "const x: number = 1;")],
        vec![],
        HarnessOptions::default(),
        options,
        "/.src",
    );
    assert!(result.emit_skipped());
    assert!(result.outputs().is_empty());
}

// A boolean compiler setting from the test config is applied (here `noEmit`).
// Go: internal/testutil/harnessutil/harnessutil.go:SetOptionsFromTestConfig (boolean)
#[test]
fn set_options_from_test_config_applies_boolean() {
    let mut config = TestConfiguration::new();
    config.insert("noEmit".to_string(), "true".to_string());

    let result = compile_files(
        vec![unit("/.src/a.ts", "const x: number = 1;")],
        vec![],
        &config,
        "/.src",
    );
    assert!(result.emit_skipped());
    assert!(result.outputs().is_empty());
}

// Panic-robustness (P10 corpus triage, category c): in a multi-file compile,
// each semantic diagnostic must be attributed to the file it actually belongs
// to — not blanket-attributed to the first user file. Reproduces
// `allowSyntheticDefaultImports9.ts` (a `b.d.ts` + a `a.ts`): a checker
// diagnostic located in the longer `a.ts` was attributed to the shorter
// `b.d.ts`, so its byte offset (48) fell outside `b.d.ts` (47 bytes) and the
// diagnostic writer sliced `text[line_start..48]` out of bounds. The fix
// attributes each diagnostic to its declaring file (per-file semantic
// diagnostics), so every diagnostic's span stays within its own file's text.
// Go: internal/testutil/harnessutil/harnessutil.go:CompilationResult (per-file diagnostic attribution)
#[test]
fn multi_file_semantic_diagnostics_stay_within_their_own_file() {
    let mut config = TestConfiguration::new();
    config.insert(
        "allowSyntheticDefaultImports".to_string(),
        "true".to_string(),
    );
    config.insert("module".to_string(), "commonjs".to_string());

    let files = vec![
        unit(
            "/.src/b.d.ts",
            "export function foo();\n\nexport function bar();\n",
        ),
        unit(
            "/.src/a.ts",
            "import { default as Foo } from \"./b\";\nFoo.bar();\nFoo.foo();",
        ),
    ];
    let lengths: std::collections::HashMap<String, usize> = files
        .iter()
        .map(|f| (f.unit_name.clone(), f.content.len()))
        .collect();

    let result = compile_files(files, vec![], &config, "/.src");

    for diag in result.diagnostics() {
        if let Some(name) = diag.file_name() {
            if let Some(&len) = lengths.get(name) {
                let end = (diag.start() + diag.length()) as usize;
                assert!(
                    end <= len,
                    "diagnostic TS{} attributed to {name} spans {}..{} but the file is only {len} bytes",
                    diag.code(),
                    diag.start(),
                    end,
                );
            }
        }
    }
}

// Panic-robustness (P10 corpus triage, category a): a relative `outDir` must be
// rooted to the current directory before emit, so the in-memory VFS (which
// rejects non-absolute paths) accepts the output path. Reproduces
// `declarationMapInlineSourcesContent.ts` / `emitEndOfFileJSDocComments.ts`,
// where `outDir: dist` produced the relative output path
// `dist/x.js[.map]` and `MapFs::write_file` panicked (`path "dist/x.js" is not
// absolute`). Mirrors Go's `CompileFilesEx`, which roots `OutDir` (and the
// other path-typed options) via `GetNormalizedAbsolutePath(value, currentDirectory)`.
// Go: internal/testutil/harnessutil/harnessutil.go:CompileFilesEx (rooting of OutDir et al.)
#[test]
fn relative_out_dir_is_rooted_before_emit() {
    let mut config = TestConfiguration::new();
    config.insert("outDir".to_string(), "dist".to_string());

    let result = compile_files(
        vec![unit(
            "/.src/index.ts",
            "export const greeting = \"hello\";\n",
        )],
        vec![],
        &config,
        "/.src",
    );

    // The relative `dist` is rooted to `/.src`, so the emit is written to (and
    // recorded at) the absolute `/.src/dist/index.js` rather than panicking.
    assert!(
        result.get_output("/.src/dist/index.js").is_some(),
        "expected rooted output /.src/dist/index.js, got outputs: {:?}",
        result
            .outputs()
            .iter()
            .map(|o| o.unit_name.clone())
            .collect::<Vec<_>>()
    );
}

// `get_config_name_from_file_name` recognizes ts/jsconfig files only.
// Go: internal/testutil/harnessutil/harnessutil.go:GetConfigNameFromFileName
#[test]
fn config_name_from_file_name() {
    assert_eq!(
        get_config_name_from_file_name("/x/tsconfig.json"),
        "tsconfig.json"
    );
    assert_eq!(
        get_config_name_from_file_name("/x/jsconfig.json"),
        "jsconfig.json"
    );
    assert_eq!(get_config_name_from_file_name("/x/a.ts"), "");
}
