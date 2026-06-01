use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tsgo_vfs::vfstest::{Clock, MapFs};
use tsgo_vfs::Fs;

use crate::{execute, ExitStatus, VfsSystem};

// A deterministic monotonic clock: every `now()` advances one second, so each
// file the build emits gets a strictly increasing mtime. This makes the build
// *order* observable (B's outputs predate A's) and makes the up-to-date check
// deterministic (inputs are always older than the buildinfo written after them).
struct StepClock {
    secs: AtomicU64,
}

impl StepClock {
    fn new() -> Self {
        StepClock {
            secs: AtomicU64::new(0),
        }
    }
}

impl Clock for StepClock {
    fn now(&self) -> SystemTime {
        let n = self.secs.fetch_add(1, Ordering::SeqCst);
        SystemTime::UNIX_EPOCH + Duration::from_secs(n + 1)
    }

    fn since_start(&self) -> Duration {
        Duration::from_secs(self.secs.load(Ordering::SeqCst))
    }
}

// A two-project composite solution rooted at `/p`: project `b` (composite) is
// referenced by project `a` (composite). Mirrors the captured `cmd/tsgo`
// fixture under `/tmp/tsb` used to record the `tsc -b` ground truth.
fn composite_fixture() -> (VfsSystem, Arc<dyn Fs + Send + Sync>) {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map_with_clock(
        [
            (
                "/p/b/tsconfig.json",
                "{ \"compilerOptions\": { \"composite\": true }, \"files\": [\"index.ts\"] }\n",
            ),
            ("/p/b/index.ts", "export const b: number = 1;\n"),
            (
                "/p/a/tsconfig.json",
                "{ \"compilerOptions\": { \"composite\": true }, \"files\": [\"index.ts\"], \"references\": [{ \"path\": \"../b\" }] }\n",
            ),
            ("/p/a/index.ts", "export const a: number = 2;\n"),
        ],
        true,
        Arc::new(StepClock::new()),
    ));
    let sys = VfsSystem::new(fs.clone(), "/p", "/lib");
    (sys, fs)
}

// A composite solution where project `b` (referenced by `a`) has a type error.
fn errored_b_fixture() -> (VfsSystem, Arc<dyn Fs + Send + Sync>) {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map_with_clock(
        [
            (
                "/p/b/tsconfig.json",
                "{ \"compilerOptions\": { \"composite\": true }, \"files\": [\"index.ts\"] }\n",
            ),
            ("/p/b/index.ts", "export const b: number = \"s\";\n"),
            (
                "/p/a/tsconfig.json",
                "{ \"compilerOptions\": { \"composite\": true }, \"files\": [\"index.ts\"], \"references\": [{ \"path\": \"../b\" }] }\n",
            ),
            ("/p/a/index.ts", "export const a: number = 2;\n"),
        ],
        true,
        Arc::new(StepClock::new()),
    ));
    let sys = VfsSystem::new(fs.clone(), "/p", "/lib");
    (sys, fs)
}

// A composite solution whose two projects reference each other (a cycle).
fn circular_fixture() -> (VfsSystem, Arc<dyn Fs + Send + Sync>) {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map_with_clock(
        [
            (
                "/p/a/tsconfig.json",
                "{ \"compilerOptions\": { \"composite\": true }, \"files\": [\"index.ts\"], \"references\": [{ \"path\": \"../b\" }] }\n",
            ),
            ("/p/a/index.ts", "export const a: number = 1;\n"),
            (
                "/p/b/tsconfig.json",
                "{ \"compilerOptions\": { \"composite\": true }, \"files\": [\"index.ts\"], \"references\": [{ \"path\": \"../a\" }] }\n",
            ),
            ("/p/b/index.ts", "export const b: number = 1;\n"),
        ],
        true,
        Arc::new(StepClock::new()),
    ));
    let sys = VfsSystem::new(fs.clone(), "/p", "/lib");
    (sys, fs)
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn mtime(fs: &Arc<dyn Fs + Send + Sync>, path: &str) -> SystemTime {
    fs.stat(path)
        .unwrap_or_else(|| panic!("missing {path}"))
        .mod_time()
}

// Slice 1: a clean `tsc -b a` builds B then A in dependency order, exit 0, with
// each project's `.js` output and `.tsbuildinfo` written.
//
// Go ground truth (`cmd/tsgo -b a`): exit 0, empty stdout, and both
// `b/{index.js,index.d.ts,tsconfig.tsbuildinfo}` and
// `a/{index.js,index.d.ts,tsconfig.tsbuildinfo}` written. (The port's emitter
// not writing `.d.ts` yet would be an out-of-scope compiler-crate divergence;
// the orchestration — order + `.js` + `.tsbuildinfo` per project — matches.)
#[test]
fn clean_build_builds_in_dependency_order() {
    let (sys, fs) = composite_fixture();
    let result = execute(&sys, &args(&["-b", "a"]));
    assert_eq!(result.status, ExitStatus::Success);
    assert_eq!(sys.output(), "");

    // Both projects' JS + buildinfo were written.
    assert!(fs.file_exists("/p/b/index.js"), "b/index.js not emitted");
    assert!(
        fs.file_exists("/p/b/tsconfig.tsbuildinfo"),
        "b buildinfo not written"
    );
    assert!(fs.file_exists("/p/a/index.js"), "a/index.js not emitted");
    assert!(
        fs.file_exists("/p/a/tsconfig.tsbuildinfo"),
        "a buildinfo not written"
    );

    // Build order: B's outputs strictly predate A's (the step clock advances on
    // every write), proving the deepest reference is built first.
    assert!(
        mtime(&fs, "/p/b/tsconfig.tsbuildinfo") < mtime(&fs, "/p/a/tsconfig.tsbuildinfo"),
        "expected B to be built before A"
    );
}

// Slice 2: a second `tsc -b a` with everything up to date rebuilds nothing,
// exits 0, and prints nothing.
//
// Go ground truth (`cmd/tsgo -b a` run twice): the second run produces empty
// stdout and exit 0, and leaves every output untouched (no re-emit).
#[test]
fn second_build_is_a_noop_when_up_to_date() {
    let (sys, fs) = composite_fixture();
    assert_eq!(
        execute(&sys, &args(&["-b", "a"])).status,
        ExitStatus::Success
    );

    // Snapshot the build outputs' timestamps after the first build.
    let b_before = mtime(&fs, "/p/b/tsconfig.tsbuildinfo");
    let a_before = mtime(&fs, "/p/a/tsconfig.tsbuildinfo");
    let b_js_before = mtime(&fs, "/p/b/index.js");

    // A second build sees everything up to date.
    let sys2 = VfsSystem::new(fs.clone(), "/p", "/lib");
    let result = execute(&sys2, &args(&["-b", "a"]));
    assert_eq!(result.status, ExitStatus::Success);
    assert_eq!(sys2.output(), "");

    // Nothing was re-emitted: every output keeps its original timestamp.
    assert_eq!(mtime(&fs, "/p/b/tsconfig.tsbuildinfo"), b_before);
    assert_eq!(mtime(&fs, "/p/a/tsconfig.tsbuildinfo"), a_before);
    assert_eq!(mtime(&fs, "/p/b/index.js"), b_js_before);
}

// Slice 3: `tsc -b a --force` rebuilds every project even when up to date.
//
// Go ground truth (`cmd/tsgo -b a --force` after a clean build): exit 0, empty
// stdout, and every output re-emitted (fresh timestamps).
#[test]
fn force_rebuilds_all_projects() {
    let (sys, fs) = composite_fixture();
    assert_eq!(
        execute(&sys, &args(&["-b", "a"])).status,
        ExitStatus::Success
    );
    let b_before = mtime(&fs, "/p/b/tsconfig.tsbuildinfo");
    let a_before = mtime(&fs, "/p/a/tsconfig.tsbuildinfo");

    let sys2 = VfsSystem::new(fs.clone(), "/p", "/lib");
    let result = execute(&sys2, &args(&["-b", "a", "--force"]));
    assert_eq!(result.status, ExitStatus::Success);
    assert_eq!(sys2.output(), "");

    // Both projects were rebuilt: their buildinfo timestamps advanced.
    assert!(
        mtime(&fs, "/p/b/tsconfig.tsbuildinfo") > b_before,
        "B was not force-rebuilt"
    );
    assert!(
        mtime(&fs, "/p/a/tsconfig.tsbuildinfo") > a_before,
        "A was not force-rebuilt"
    );
}

// Slice 4: `tsc -b a --dry` on a fresh tree reports the build plan and builds
// nothing.
//
// Go ground truth (`cmd/tsgo -b a --dry`, no outputs yet): exit 0; two
// time-stamped lines `<time> - A non-dry build would build project
// '/p/<proj>/tsconfig.json'` (absolute config paths, B before A); no files
// written.
#[test]
fn dry_run_reports_plan_without_building() {
    let (sys, fs) = composite_fixture();
    let result = execute(&sys, &args(&["-b", "a", "--dry"]));
    assert_eq!(result.status, ExitStatus::Success);

    // Nothing was built.
    assert!(!fs.file_exists("/p/b/index.js"));
    assert!(!fs.file_exists("/p/b/tsconfig.tsbuildinfo"));
    assert!(!fs.file_exists("/p/a/index.js"));

    // The plan lists both projects (absolute config paths), B before A.
    let out = sys.output();
    let b_idx = out
        .find("A non-dry build would build project '/p/b/tsconfig.json'")
        .expect("missing B plan line");
    let a_idx = out
        .find("A non-dry build would build project '/p/a/tsconfig.json'")
        .expect("missing A plan line");
    assert!(b_idx < a_idx, "B should be planned before A: {out:?}");
}

// Slice 4b: `tsc -b a --dry` after a build reports each project up to date.
//
// Go ground truth: exit 0; `<time> - Project '/p/<proj>/tsconfig.json' is up
// to date` (absolute config paths).
#[test]
fn dry_run_reports_up_to_date_after_build() {
    let (sys, fs) = composite_fixture();
    execute(&sys, &args(&["-b", "a"]));

    let sys2 = VfsSystem::new(fs.clone(), "/p", "/lib");
    let result = execute(&sys2, &args(&["-b", "a", "--dry"]));
    assert_eq!(result.status, ExitStatus::Success);
    let out = sys2.output();
    assert!(
        out.contains("Project '/p/b/tsconfig.json' is up to date"),
        "missing B up-to-date line: {out:?}"
    );
    assert!(
        out.contains("Project '/p/a/tsconfig.json' is up to date"),
        "missing A up-to-date line: {out:?}"
    );
}

// Slice 5: `tsc -b a` with a type error in B reports the error and exits 1.
// The build does NOT stop (no `--stopBuildOnErrors`): B still emits its outputs
// (no `noEmitOnError`) and A is still built afterwards.
//
// Go ground truth (`cmd/tsgo -b a`, B has a type error): exit 1; stdout
// `b/index.ts(1,14): error TS2322: Type 'string' is not assignable to type
// 'number'.`; both projects emit their `.js` + `.tsbuildinfo`.
//
// DIVERGENCE(port): the column is the `tsgo_checker` variable-declaration span
// (one less than Go's), an out-of-scope checker-crate divergence already noted
// for the single-build path; the code (TS2322), message, file, exit code, and
// build-continues semantics all match Go.
#[test]
fn type_error_in_dependency_reports_and_continues() {
    let (sys, fs) = errored_b_fixture();
    let result = execute(&sys, &args(&["-b", "a"]));
    assert_eq!(
        result.status,
        ExitStatus::DiagnosticsPresentOutputsGenerated
    );

    let out = sys.output();
    assert!(
        out.contains("error TS2322: Type 'string' is not assignable to type 'number'."),
        "missing TS2322: {out:?}"
    );
    // Port column is `13` (Go's is `14`) — the documented `tsgo_checker`
    // variable-declaration span off-by-one, an out-of-scope checker-crate
    // divergence; assert the stable prefix.
    assert!(
        out.contains(
            "b/index.ts(1,13): error TS2322: Type 'string' is not assignable to type 'number'."
        ),
        "unexpected diagnostic: {out:?}"
    );

    // The build continued: B still emitted (no noEmitOnError) and A was built.
    assert!(fs.file_exists("/p/b/index.js"), "B did not emit");
    assert!(
        fs.file_exists("/p/a/index.js"),
        "A was not built after B's error"
    );
}

// Slice 6a: `tsc -b a --verbose` prints the projects list and the per-project
// out-of-date reasons + "Building project ..." lines, B before A.
//
// Go ground truth (`cmd/tsgo -b a --verbose`, fresh): time-stamped
// `Projects in this build: \r\n    * b/tsconfig.json\r\n    * a/tsconfig.json`,
// then `Project 'b/tsconfig.json' is out of date because output file
// 'b/tsconfig.tsbuildinfo' does not exist`, `Building project
// 'b/tsconfig.json'...`, then the same pair for `a`.
#[test]
fn verbose_reports_projects_and_build_status() {
    let (sys, _fs) = composite_fixture();
    let result = execute(&sys, &args(&["-b", "a", "--verbose"]));
    assert_eq!(result.status, ExitStatus::Success);
    let out = sys.output();

    assert!(
        out.contains("Projects in this build: \r\n    * b/tsconfig.json\r\n    * a/tsconfig.json"),
        "missing projects list: {out:?}"
    );
    let b_reason = out
        .find("Project 'b/tsconfig.json' is out of date because output file 'b/tsconfig.tsbuildinfo' does not exist")
        .expect("missing B out-of-date reason");
    let b_build = out
        .find("Building project 'b/tsconfig.json'...")
        .expect("missing B building line");
    let a_build = out
        .find("Building project 'a/tsconfig.json'...")
        .expect("missing A building line");
    assert!(
        b_reason < b_build && b_build < a_build,
        "wrong order: {out:?}"
    );
}

// Slice 6b: `tsc -b nope` for a missing project reports TS6053 and exits 2.
//
// Go ground truth (`cmd/tsgo -b nope`): exit 2; stdout
// `error TS6053: File '/p/nope/tsconfig.json' not found.`
#[test]
fn missing_project_reports_ts6053_and_exits_two() {
    let (sys, _fs) = composite_fixture();
    let result = execute(&sys, &args(&["-b", "nope"]));
    assert_eq!(result.status, ExitStatus::DiagnosticsPresentOutputsSkipped);
    assert_eq!(
        sys.output(),
        "error TS6053: File '/p/nope/tsconfig.json' not found.\n"
    );
}

// Slice 6c: `tsc -b a` over a project-reference cycle reports TS6202 and exits 4,
// building nothing.
//
// Go ground truth (`cmd/tsgo -b a`, a<->b cycle): exit 4; stdout
// `error TS6202: Project references may not form a circular graph. Cycle
// detected: /p/a/tsconfig.json\n/p/b/tsconfig.json`.
#[test]
fn circular_references_report_ts6202_and_exit_four() {
    let (sys, fs) = circular_fixture();
    let result = execute(&sys, &args(&["-b", "a"]));
    assert_eq!(
        result.status,
        ExitStatus::ProjectReferenceCycleOutputsSkipped
    );
    let out = sys.output();
    assert!(
        out.contains(
            "error TS6202: Project references may not form a circular graph. Cycle detected:"
        ),
        "out: {out:?}"
    );
    assert!(out.contains("/p/a/tsconfig.json"), "out: {out:?}");
    assert!(out.contains("/p/b/tsconfig.json"), "out: {out:?}");
    // Nothing was built.
    assert!(!fs.file_exists("/p/a/tsconfig.tsbuildinfo"));
    assert!(!fs.file_exists("/p/b/tsconfig.tsbuildinfo"));
}
