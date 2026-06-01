use super::*;
use std::sync::Arc;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

// Builds a `VfsSystem` over a single-file in-memory project rooted at `/p`,
// returning the system and a handle to the shared file system (so emitted
// outputs can be read back).
fn single_file_sys(file_name: &str, text: &str) -> (VfsSystem, Arc<dyn Fs + Send + Sync>) {
    let abs = format!("/p/{file_name}");
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([(abs, text)], true));
    let sys = VfsSystem::new(fs.clone(), "/p", "/lib");
    (sys, fs)
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

// Slice 1: a clean program compiles to exit 0 and writes its `.js`.
//
// Go ground truth (`cmd/tsgo index.ts`): exit 0, empty output, emits
// `index.js`. (Go also prepends a `"use strict";` prologue the port's emitter
// does not yet add — a P6 emitter divergence, out of scope here; the
// orchestration behavior — exit code + a `.js` written — matches.)
#[test]
fn clean_program_exits_zero_and_emits_js() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["index.ts"]));
    assert_eq!(result.status, ExitStatus::Success);
    assert_eq!(sys.output(), "");
    assert!(fs.file_exists("/p/index.js"));
    let js = fs.read_file("/p/index.js").expect("emitted js");
    assert!(js.contains("const x = 1"), "unexpected js: {js:?}");
}

// Slice 2: a type error reports TS2322 and exits 1 (outputs still generated).
//
// Go ground truth: exit 1; output `index.ts(1,7): error TS2322: Type 'string'
// is not assignable to type 'number'.`; `index.js` still written.
//
// DIVERGENCE(port): the column is `6`, not Go's `7` — the `tsgo_checker`
// diagnostic span for the variable declaration starts at `start=5` (the space
// before `x`) where Go uses `6` (the `x`). The checker lives in an out-of-scope
// crate; the code (TS2322), message, and exit code all match Go exactly.
#[test]
fn type_error_reports_ts2322_and_exits_one() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = \"s\";\n");
    let result = execute(&sys, &args(&["index.ts"]));
    assert_eq!(
        result.status,
        ExitStatus::DiagnosticsPresentOutputsGenerated
    );
    assert_eq!(
        sys.output(),
        "index.ts(1,6): error TS2322: Type 'string' is not assignable to type 'number'.\n"
    );
    // Outputs are still generated when emit is not blocked.
    assert!(fs.file_exists("/p/index.js"));
}

// Slice 3: `--noEmit` on a clean file checks only — exit 0, no `.js` written.
//
// Go ground truth (`--noEmit index.ts`): exit 0, empty output, no `index.js`.
#[test]
fn no_emit_clean_exits_zero_without_writing_js() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["--noEmit", "index.ts"]));
    assert_eq!(result.status, ExitStatus::Success);
    assert_eq!(sys.output(), "");
    assert!(!fs.file_exists("/p/index.js"));
}

// Slice 4: `--noEmit` on an errored file reports the error, exits 2, no output.
//
// Go ground truth (`--noEmit index.ts` with a type error): exit 2 (outputs
// skipped), reports TS2322, no `index.js`.
#[test]
fn no_emit_errored_exits_two_without_writing_js() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = \"s\";\n");
    let result = execute(&sys, &args(&["--noEmit", "index.ts"]));
    assert_eq!(result.status, ExitStatus::DiagnosticsPresentOutputsSkipped);
    assert_eq!(
        sys.output(),
        "index.ts(1,6): error TS2322: Type 'string' is not assignable to type 'number'.\n"
    );
    assert!(!fs.file_exists("/p/index.js"));
}

// Slice 5: `--sourceMap` on a clean file emits both `.js` and `.js.map`.
//
// Go ground truth (`--sourceMap index.ts`): exit 0, emits `index.js` (with a
// trailing `//# sourceMappingURL=index.js.map`) and `index.js.map`.
#[test]
fn source_map_clean_emits_js_and_map() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["--sourceMap", "index.ts"]));
    assert_eq!(result.status, ExitStatus::Success);
    assert!(fs.file_exists("/p/index.js"));
    assert!(fs.file_exists("/p/index.js.map"));
    let js = fs.read_file("/p/index.js").expect("emitted js");
    assert!(
        js.contains("//# sourceMappingURL=index.js.map"),
        "missing sourceMappingURL comment: {js:?}"
    );
}

// Slice 6: in the default plain (non-TTY) path there is no `Found N errors`
// summary line — matching Go, whose `CreateReportErrorSummary` is a no-op unless
// pretty. Only the compact diagnostic line is printed.
#[test]
fn plain_mode_prints_no_error_summary() {
    let (sys, _fs) = single_file_sys("index.ts", "const x: number = \"s\";\n");
    let _ = execute(&sys, &args(&["index.ts"]));
    let output = sys.output();
    assert!(
        !output.contains("Found"),
        "plain mode must not print a summary: {output:?}"
    );
    // Exactly the one diagnostic line was printed.
    assert_eq!(output.lines().count(), 1);
}

// Slice 6 (pretty): `--pretty` enables the `Found N errors in <file>` summary,
// matching Go's pretty output (`Found 1 error in index.ts`).
#[test]
fn pretty_mode_prints_found_errors_summary() {
    let (sys, _fs) = single_file_sys("index.ts", "const x: number = \"s\";\n");
    let _ = execute(&sys, &args(&["--pretty", "index.ts"]));
    let output = sys.output();
    assert!(output.contains("TS2322"), "missing code: {output:?}");
    assert!(
        output.contains("Found 1 error in index.ts"),
        "missing pretty summary: {output:?}"
    );
}

// Options/config diagnostics: an unknown compiler option surfaces TS5023 and
// exits 2 before any program is built.
//
// Go ground truth (`--badOption index.ts`): exit 2; output
// `error TS5023: Unknown compiler option '--badOption'.` (no file location, as
// it is a global command-line error).
#[test]
fn unknown_option_reports_ts5023_and_exits_two() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["--badOption", "index.ts"]));
    assert_eq!(result.status, ExitStatus::DiagnosticsPresentOutputsSkipped);
    assert_eq!(
        sys.output(),
        "error TS5023: Unknown compiler option '--badOption'.\n"
    );
    // No build happened, so nothing was emitted.
    assert!(!fs.file_exists("/p/index.js"));
}

// Options diagnostics from `verifyCompilerOptions`: a removed option
// (`--target ES5`) surfaces TS5108 and exits 1 (outputs still generated).
//
// Go ground truth (`--target ES5 index.ts`): exit 1; output
// `error TS5108: Option 'target=ES5' has been removed. Please remove it from
// your configuration.`; `index.js` still emitted.
#[test]
fn removed_option_reports_ts5108_and_exits_one() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["--target", "ES5", "index.ts"]));
    assert_eq!(
        result.status,
        ExitStatus::DiagnosticsPresentOutputsGenerated
    );
    assert_eq!(
        sys.output(),
        "error TS5108: Option 'target=ES5' has been removed. Please remove it from your configuration.\n"
    );
    assert!(fs.file_exists("/p/index.js"));
}

// `perform_compilation` can be driven directly with a pre-parsed command line,
// mirroring Go's `performCompilation` entry.
#[test]
fn perform_compilation_entry_compiles_clean_program() {
    use tsgo_locale::parse;
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let config = tsgo_tsoptions::new_parsed_command_line(
        tsgo_core::compileroptions::CompilerOptions::default(),
        vec!["/p/index.ts".to_string()],
        tsgo_tspath::ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/p".into(),
        },
    );
    let locale = parse("en").unwrap();
    let report_diagnostic = create_diagnostic_reporter(&sys, &locale, config.compiler_options());
    let report_error_summary =
        create_report_error_summary(&sys, &locale, config.compiler_options());
    let result = perform_compilation(
        &sys,
        config,
        &report_diagnostic,
        &report_error_summary,
        &locale,
    );
    assert_eq!(result.status, ExitStatus::Success);
    assert!(fs.file_exists("/p/index.js"));
}

// Watch dispatch: `--watch` routes through `tsc_compilation` to the watch loop.
// A plain `VfsSystem` reports no changes (the default `wait_for_change` returns
// `false`), so the loop runs the initial build, prints the watch-mode status
// lines, emits the `.js`, then exits.
//
// Go ground truth: `tscCompilation` enters the watch branch when
// `CompilerOptions().Watch.IsTrue()`.
#[test]
fn watch_flag_dispatches_to_watch_loop() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["--watch", "index.ts"]));
    assert_eq!(result.status, ExitStatus::Success);
    let out = sys.output();
    assert!(
        out.contains("Starting compilation in watch mode..."),
        "missing watch-start status: {out:?}"
    );
    assert!(
        out.contains("Found 0 errors. Watching for file changes."),
        "missing post-build status: {out:?}"
    );
    assert!(fs.file_exists("/p/index.js"));
}

// Watch dispatch via the `-w` short flag routes to the watch loop as well.
#[test]
fn watch_short_flag_dispatches_to_watch_loop() {
    let (sys, _fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["-w", "index.ts"]));
    assert_eq!(result.status, ExitStatus::Success);
    assert!(
        sys.output()
            .contains("Starting compilation in watch mode..."),
        "missing watch-start status: {:?}",
        sys.output()
    );
}

// `--watch` and `--listFilesOnly` cannot be combined: TS6370 is reported and the
// run exits 2 before any build.
//
// Go ground truth: `tscCompilation` reports `Options_0_and_1_cannot_be_combined`
// ("watch", "listFilesOnly") and returns
// `ExitStatusDiagnosticsPresent_OutputsSkipped`.
#[test]
fn watch_with_list_files_only_reports_ts6370_and_exits_two() {
    let (sys, fs) = single_file_sys("index.ts", "const x: number = 1;\n");
    let result = execute(&sys, &args(&["--watch", "--listFilesOnly", "index.ts"]));
    assert_eq!(result.status, ExitStatus::DiagnosticsPresentOutputsSkipped);
    assert_eq!(
        sys.output(),
        "error TS6370: Options 'watch' and 'listFilesOnly' cannot be combined.\n"
    );
    // No build happened, so nothing was emitted and no watch status printed.
    assert!(!fs.file_exists("/p/index.js"));
}
