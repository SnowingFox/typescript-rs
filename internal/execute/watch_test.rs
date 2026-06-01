use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::sys::{System, VfsSystem};
use crate::tsc::{create_diagnostic_reporter, create_report_error_summary, ExitStatus};

// A deterministic, finite watch system for tests: it wraps a `VfsSystem`,
// delegating every `System` facet to it, and drives the watch loop by applying
// one queued file edit per `wait_for_change` call (returning `true`), then
// signalling "no more changes" (`false`) once the queue is empty so the loop
// terminates. This mirrors how Go's tests drive `Watcher.DoCycle` after editing
// files, without a real OS file-watcher.
struct WatchTestSystem {
    inner: VfsSystem,
    edits: RefCell<VecDeque<(String, String)>>,
}

impl WatchTestSystem {
    fn new(inner: VfsSystem) -> WatchTestSystem {
        WatchTestSystem {
            inner,
            edits: RefCell::new(VecDeque::new()),
        }
    }

    // Queues a file edit (path -> new content) applied on the next watch cycle.
    fn push_edit(&self, path: impl Into<String>, content: impl Into<String>) {
        self.edits
            .borrow_mut()
            .push_back((path.into(), content.into()));
    }

    fn output(&self) -> String {
        self.inner.output()
    }
}

impl System for WatchTestSystem {
    fn fs(&self) -> Arc<dyn Fs + Send + Sync> {
        self.inner.fs()
    }
    fn default_library_path(&self) -> &str {
        self.inner.default_library_path()
    }
    fn get_current_directory(&self) -> &str {
        self.inner.get_current_directory()
    }
    fn write(&self, s: &str) {
        self.inner.write(s)
    }
    fn write_output_is_tty(&self) -> bool {
        self.inner.write_output_is_tty()
    }
    fn get_environment_variable(&self, name: &str) -> String {
        self.inner.get_environment_variable(name)
    }
    fn now(&self) -> SystemTime {
        self.inner.now()
    }
    fn wait_for_change(&self) -> bool {
        let next = self.edits.borrow_mut().pop_front();
        match next {
            Some((path, content)) => {
                self.fs()
                    .write_file(&path, &content)
                    .expect("test edit write");
                true
            }
            None => false,
        }
    }
}

// Builds a `WatchTestSystem` over a single-file in-memory project rooted at
// `/p`, returning the system and a handle to the shared file system.
fn watch_sys(file_name: &str, text: &str) -> (WatchTestSystem, Arc<dyn Fs + Send + Sync>) {
    let abs = format!("/p/{file_name}");
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([(abs, text)], true));
    let sys = WatchTestSystem::new(VfsSystem::new(fs.clone(), "/p", "/lib"));
    (sys, fs)
}

// Drives `perform_watch` directly with a pre-parsed config over `sys`.
fn run_watch(sys: &dyn System, file_names: &[&str]) -> CommandLineResult {
    let config = tsgo_tsoptions::new_parsed_command_line(
        tsgo_core::compileroptions::CompilerOptions::default(),
        file_names.iter().map(|s| s.to_string()).collect(),
        tsgo_tspath::ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/p".into(),
        },
    );
    let locale = tsgo_locale::parse("en").unwrap();
    let report_diagnostic = create_diagnostic_reporter(sys, &locale, config.compiler_options());
    let report_error_summary = create_report_error_summary(sys, &locale, config.compiler_options());
    perform_watch(
        sys,
        config,
        &report_diagnostic,
        &report_error_summary,
        &locale,
    )
}

// Slice 1: a watch run with no queued changes does exactly one (initial) build,
// reports the "Starting compilation in watch mode..." status and the post-build
// "Found 0 errors. Watching for file changes." status, emits the `.js`, then
// exits (the fake sys signals no more changes).
//
// Go ground truth (`internal/diagnostics`): 6031 = "Starting compilation in
// watch mode...", 6194 = "Found {0} errors. Watching for file changes."
#[test]
fn initial_build_in_watch_mode_reports_status_then_exits() {
    let (sys, fs) = watch_sys("index.ts", "const x: number = 1;\n");
    let result = run_watch(&sys, &["/p/index.ts"]);
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
    // Exactly one build cycle: a single "Watching for file changes." line and no
    // "File change detected" cycle line.
    assert_eq!(
        out.matches("Watching for file changes.").count(),
        1,
        "expected exactly one build cycle: {out:?}"
    );
    assert!(
        !out.contains("File change detected"),
        "no change cycle expected: {out:?}"
    );
    // The clean program still emitted its output.
    assert!(fs.file_exists("/p/index.js"));
}

// Slice 2: a single queued change drives one rebuild cycle. The loop reports
// the watch-start status, builds, then (on the one change) reports "File change
// detected. Starting incremental compilation..." and builds again, before
// exiting. Asserts exactly two build cycles + the change-cycle status.
//
// Go ground truth (`internal/diagnostics`): 6032 = "File change detected.
// Starting incremental compilation...".
#[test]
fn one_change_drives_a_second_build_cycle() {
    let (sys, _fs) = watch_sys("index.ts", "const x: number = 1;\n");
    // Edit the file once (still clean) -> exactly one extra build cycle.
    sys.push_edit("/p/index.ts", "const y: number = 2;\n");
    let result = run_watch(&sys, &["/p/index.ts"]);
    assert_eq!(result.status, ExitStatus::Success);

    let out = sys.output();
    assert!(
        out.contains("Starting compilation in watch mode..."),
        "missing watch-start status: {out:?}"
    );
    assert!(
        out.contains("File change detected. Starting incremental compilation..."),
        "missing change-cycle status: {out:?}"
    );
    // Two build cycles: the post-build "Watching for file changes." line twice,
    // and exactly one "File change detected" cycle line.
    assert_eq!(
        out.matches("Watching for file changes.").count(),
        2,
        "expected two build cycles: {out:?}"
    );
    assert_eq!(
        out.matches("File change detected").count(),
        1,
        "expected exactly one change cycle: {out:?}"
    );
}

// Slice 3: an error->fix cycle. Build 1 has a type error: it reports TS2322 and
// the "Found 1 error. Watching for file changes." status. A queued edit fixes
// the file, so build 2 is clean and reports "Found 0 errors. Watching for file
// changes." Verifies both status texts (and their order) + the diagnostic.
//
// Go ground truth (`internal/diagnostics`): 6193 = "Found 1 error. Watching for
// file changes.", 6194 = "Found {0} errors. Watching for file changes.".
#[test]
fn error_then_fix_cycle_reports_one_then_zero_errors() {
    let (sys, _fs) = watch_sys("index.ts", "const x: number = \"s\";\n");
    // The change fixes the type error -> the second build is clean.
    sys.push_edit("/p/index.ts", "const x: number = 1;\n");
    let result = run_watch(&sys, &["/p/index.ts"]);
    assert_eq!(result.status, ExitStatus::Success);

    let out = sys.output();
    // The first build's type error is reported (TS2322).
    assert!(
        out.contains("error TS2322: Type 'string' is not assignable to type 'number'."),
        "missing TS2322 from the first build: {out:?}"
    );
    // Build 1 status: one error.
    assert!(
        out.contains("Found 1 error. Watching for file changes."),
        "missing 'Found 1 error' status: {out:?}"
    );
    // Build 2 status (after the fix): zero errors.
    assert!(
        out.contains("Found 0 errors. Watching for file changes."),
        "missing 'Found 0 errors' status: {out:?}"
    );
    // The "1 error" status precedes the "0 errors" status (build 1 then 2).
    let one = out.find("Found 1 error").expect("found-1");
    let zero = out.find("Found 0 errors").expect("found-0");
    assert!(one < zero, "build order wrong: {out:?}");
}
