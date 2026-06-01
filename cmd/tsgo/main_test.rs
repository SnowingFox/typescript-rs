use super::*;
use tsgo_execute::VfsSystem;
use tsgo_vfs::vfstest::MapFs;

// Builds a `VfsSystem` over a single-file in-memory project rooted at `/p`,
// returning the system and a handle to the shared file system (so emitted
// outputs can be read back). Mirrors the proven setup in
// `internal/execute/lib_test.rs`.
fn single_file_sys(file_name: &str, text: &str) -> (VfsSystem, Arc<dyn Fs + Send + Sync>) {
    let abs = format!("/p/{file_name}");
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([(abs, text)], true));
    let sys = VfsSystem::new(fs.clone(), "/p", "/lib");
    (sys, fs)
}

// A system whose file system has no source files — for argv paths (version,
// help, mode stubs) that never read the disk.
fn empty_sys() -> VfsSystem {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/p/.keep", "")], true));
    VfsSystem::new(fs, "/p", "/lib")
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

// Slice 1: `tsgo --version` prints the compiler version and exits 0.
//
// Go ground truth (`tsgo --version`, NO_COLOR=1): exit 0, stdout
// `Version 7.0.0-dev\n` (from `tsc.PrintVersion` -> `Version_0` localized with
// `core.Version()`).
#[test]
fn version_flag_prints_version_and_exits_zero() {
    let sys = empty_sys();
    let status = run(&argv(&["--version"]), &sys);
    assert_eq!(status, ExitStatus::Success);
    assert_eq!(sys.output(), "Version 7.0.0-dev\n");
}

// Slice 2: `tsgo good.ts` compiles a clean program — exit 0, the `.js` is
// emitted to the (real, here in-memory) file system. This is the binary's
// delegation tracer: `run` forwards the default path to the `tsgo_execute`
// build driver.
//
// Go ground truth (`tsgo good.ts`, NO_COLOR=1): exit 0, empty output, emits
// `good.js` (Go prepends `"use strict";`, which the port's emitter does not yet
// add — a P6 emitter divergence; the orchestration result — exit code + a `.js`
// written containing `const x = 1` — matches Go).
#[test]
fn good_file_compiles_clean_exits_zero_and_emits_js() {
    let (sys, fs) = single_file_sys("good.ts", "const x: number = 1;\n");
    let status = run(&argv(&["good.ts"]), &sys);
    assert_eq!(status, ExitStatus::Success);
    assert_eq!(sys.output(), "");
    assert!(fs.file_exists("/p/good.js"));
    let js = fs.read_file("/p/good.js").expect("emitted js");
    assert!(js.contains("const x = 1"), "unexpected js: {js:?}");
}

// Slice 3: `tsgo bad.ts` reports a type error and exits 1; the `.js` is still
// emitted (emit is not blocked by `--noEmit`).
//
// Go ground truth (`tsgo bad.ts`, NO_COLOR=1): exit 1; stdout
// `bad.ts(1,7): error TS2322: Type 'string' is not assignable to type
// 'number'.`; `bad.js` still written.
//
// DIVERGENCE(port): the reported column is `6`, not Go's `7` — the
// `tsgo_checker` diagnostic span for the declaration starts one character
// earlier. The code (TS2322), message, exit code, and "still emitted" behavior
// all match Go. (Same divergence documented in `internal/execute`.)
#[test]
fn bad_file_reports_ts2322_and_exits_one() {
    let (sys, fs) = single_file_sys("bad.ts", "const x: number = \"s\";\n");
    let status = run(&argv(&["bad.ts"]), &sys);
    assert_eq!(status, ExitStatus::DiagnosticsPresentOutputsGenerated);
    assert_eq!(
        sys.output(),
        "bad.ts(1,6): error TS2322: Type 'string' is not assignable to type 'number'.\n"
    );
    assert!(fs.file_exists("/p/bad.js"));
}

// Slice 4: `tsgo --noEmit bad.ts` checks only — the type error is reported,
// exit code is 2 (outputs skipped), and no `.js` is written.
//
// Go ground truth (`tsgo --noEmit bad.ts`, NO_COLOR=1): exit 2; same TS2322
// line; no `bad.js`.
#[test]
fn no_emit_bad_file_exits_two_without_writing_js() {
    let (sys, fs) = single_file_sys("bad.ts", "const x: number = \"s\";\n");
    let status = run(&argv(&["--noEmit", "bad.ts"]), &sys);
    assert_eq!(status, ExitStatus::DiagnosticsPresentOutputsSkipped);
    assert_eq!(
        sys.output(),
        "bad.ts(1,6): error TS2322: Type 'string' is not assignable to type 'number'.\n"
    );
    assert!(!fs.file_exists("/p/bad.js"));
}

// Slice 5a: `tsgo --lsp` is recognized by arg0 (like Go's `runMain`) and routed
// to a clear "not yet implemented" stub. The real LSP server is P8.
//
// Go ground truth: Go actually runs the language server; there is no fixed
// stdout to match, so the port emits a clear deferral message and a
// not-implemented exit status. blocked-by: the `internal/lsp` port (P8).
#[test]
fn lsp_mode_is_deferred_with_clear_message() {
    let sys = empty_sys();
    let status = run(&argv(&["--lsp", "--stdio"]), &sys);
    assert_eq!(status, ExitStatus::NotImplemented);
    let out = sys.output();
    assert!(out.contains("--lsp"), "stub must name the mode: {out:?}");
    assert!(
        out.contains("not yet implemented"),
        "stub must be clear: {out:?}"
    );
}

// Slice 5b: `tsgo --api` is likewise recognized by arg0 and routed to a clear
// "not yet implemented" stub. The real API server is P8.
//
// Go ground truth: Go runs the API server (blocking on stdin); the port emits a
// clear deferral message + not-implemented exit. blocked-by: `internal/api`
// (P8).
#[test]
fn api_mode_is_deferred_with_clear_message() {
    let sys = empty_sys();
    let status = run(&argv(&["--api"]), &sys);
    assert_eq!(status, ExitStatus::NotImplemented);
    let out = sys.output();
    assert!(out.contains("--api"), "stub must name the mode: {out:?}");
    assert!(
        out.contains("not yet implemented"),
        "stub must be clear: {out:?}"
    );
}

// Slice 6: `tsgo --help` is reachable only for its version header; the full
// option-table generator is deferred. The binary prints the version line plus a
// clear deferral notice and exits 0 (matching Go's `--help` exit code).
//
// Go ground truth (`tsgo --help`, NO_COLOR=1): exit 0; prints the full
// `tsc: The TypeScript Compiler - Version ...` help with every command/option.
// blocked-by: `tsc.PrintHelp` + the `tsoptions` help machinery, which
// `tsgo_execute` does not expose.
#[test]
fn help_flag_prints_version_header_and_is_deferred() {
    let sys = empty_sys();
    let status = run(&argv(&["--help"]), &sys);
    assert_eq!(status, ExitStatus::Success);
    let out = sys.output();
    assert!(
        out.contains("Version 7.0.0-dev"),
        "missing version: {out:?}"
    );
    assert!(
        out.contains("not yet implemented"),
        "help stub must be clear: {out:?}"
    );
}

// `-v` is the short alias for `--version` (parsed by `tsoptions`), so it prints
// the version and exits 0 just like `--version`.
#[test]
fn short_version_flag_prints_version() {
    let sys = empty_sys();
    let status = run(&argv(&["-v"]), &sys);
    assert_eq!(status, ExitStatus::Success);
    assert_eq!(sys.output(), "Version 7.0.0-dev\n");
}

// `-h` is the short alias for `--help`, routed to the same deferred help stub.
#[test]
fn short_help_flag_is_deferred() {
    let sys = empty_sys();
    let status = run(&argv(&["-h"]), &sys);
    assert_eq!(status, ExitStatus::Success);
    assert!(
        sys.output().contains("Version 7.0.0-dev"),
        "missing version: {:?}",
        sys.output()
    );
}

// An unknown compiler option is a command-line error: it is reported (TS5023)
// and exits 2, before any program is built. Confirms `run` delegates
// command-line errors to the build driver.
//
// Go ground truth (`tsgo --badOption good.ts`): exit 2; output
// `error TS5023: Unknown compiler option '--badOption'.`; no `good.js`.
#[test]
fn unknown_option_reports_ts5023_and_exits_two() {
    let (sys, fs) = single_file_sys("good.ts", "const x: number = 1;\n");
    let status = run(&argv(&["--badOption", "good.ts"]), &sys);
    assert_eq!(status, ExitStatus::DiagnosticsPresentOutputsSkipped);
    assert_eq!(
        sys.output(),
        "error TS5023: Unknown compiler option '--badOption'.\n"
    );
    assert!(!fs.file_exists("/p/good.js"));
}

// Precedence: command-line errors win over `--version` (Go checks
// `len(Errors) > 0` before the `Version.IsTrue()` branch). So
// `--version --badOption` reports TS5023 and exits 2 — it does NOT print the
// version.
//
// Go ground truth (`tsgo --version --badOption`): exit 2; output
// `error TS5023: Unknown compiler option '--badOption'.`.
#[test]
fn command_line_errors_take_precedence_over_version() {
    let sys = empty_sys();
    let status = run(&argv(&["--version", "--badOption"]), &sys);
    assert_eq!(status, ExitStatus::DiagnosticsPresentOutputsSkipped);
    let out = sys.output();
    assert!(out.contains("TS5023"), "expected TS5023: {out:?}");
    assert!(
        !out.contains("Version 7.0.0-dev"),
        "version must not print when errors present: {out:?}"
    );
}

// `--version` is detected anywhere on the command line (it is a parsed option,
// not an arg0 token): `good.ts --version` prints the version, exits 0, and does
// NOT build/emit (the version branch returns before compilation).
//
// Go ground truth (`tsgo good.ts --version`): exit 0; prints `Version ...`; no
// `good.js`.
#[test]
fn version_detected_after_file_name_skips_build() {
    let (sys, fs) = single_file_sys("good.ts", "const x: number = 1;\n");
    let status = run(&argv(&["good.ts", "--version"]), &sys);
    assert_eq!(status, ExitStatus::Success);
    assert_eq!(sys.output(), "Version 7.0.0-dev\n");
    assert!(!fs.file_exists("/p/good.js"));
}

// Mode dispatch is arg0-only, exactly like Go's `runMain`: `--lsp` after a file
// name is NOT treated as the LSP mode (it flows to the build driver, where it
// is an unknown option), so the LSP stub is never printed.
#[test]
fn lsp_mode_dispatch_is_arg0_only() {
    let (sys, _fs) = single_file_sys("good.ts", "const x: number = 1;\n");
    let status = run(&argv(&["good.ts", "--lsp"]), &sys);
    assert_ne!(status, ExitStatus::NotImplemented);
    assert!(
        !sys.output().contains("language server"),
        "non-arg0 --lsp must not trigger the LSP stub: {:?}",
        sys.output()
    );
}

// Exercises the exact production wiring `main()` builds: a `System` backed by
// the real bundled file system (`tsgo_bundled::wrap_fs(osvfs)`) and the bundled
// `lib_path()`. The `--version` path is reachable without touching the disk or
// loading libs, so this confirms the bundled FS + lib-path construction is
// sound for the dispatch paths. (The real-binary compile path is exercised
// out-of-band; see the worklog for the documented downstream limitation.)
#[test]
fn version_works_over_real_bundled_file_system() {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(tsgo_bundled::wrap_fs(tsgo_vfs::osvfs::fs()));
    let sys = VfsSystem::new(fs, "/p", tsgo_bundled::lib_path());
    let status = run(&argv(&["--version"]), &sys);
    assert_eq!(status, ExitStatus::Success);
    assert_eq!(sys.output(), "Version 7.0.0-dev\n");
}
