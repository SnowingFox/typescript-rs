//! Baseline-tracking bookkeeping used to detect unused reference baselines.
//!
//! 1:1 port of Go `internal/testutil/baseline/testmain.go`.
//!
//! DIVERGENCE(port): Go hashes the runtime call stack (`runtime.Callers`) to
//! derive a per-package tracking filename, since every Go package runs in one
//! `TestMain`. Rust has no stable run-time caller-stack reflection, but every
//! crate's tests compile into a distinct test binary, so we hash the current
//! executable path instead — a faithful per-package analogue. Tracking is only
//! used to flag unused baselines; it is not on the comparison hot path.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tsgo_collections::SyncSet;

use crate::Harness;

/// Directory where tracking files are written. Empty disables tracking.
///
/// Mirrors Go's package-level `trackingDir = os.Getenv(...)`; read once.
fn tracking_dir() -> &'static str {
    static DIR: OnceLock<String> = OnceLock::new();
    DIR.get_or_init(|| std::env::var("TSGO_BASELINE_TRACKING_DIR").unwrap_or_default())
        .as_str()
}

/// Set to `true` once [`track`] has been called.
static TRACKING_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// All baseline paths written during the test run (deduplicated, concurrent).
fn recorded_baselines() -> &'static SyncSet<String> {
    static R: OnceLock<SyncSet<String>> = OnceLock::new();
    R.get_or_init(SyncSet::default)
}

/// What [`record_baseline`] should do for the current tracking configuration.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RecordAction {
    /// Tracking disabled: ignore the baseline path.
    Ignore,
    /// Tracking enabled but `track()` was never called: report an error.
    MissingTrackInit,
    /// Tracking enabled and initialized: record the baseline path.
    Record,
}

/// Pure decision for [`record_baseline`], extracted for testability.
///
/// Side effects: none (pure).
// Go: internal/testutil/baseline/testmain.go:recordBaseline (decision)
pub(crate) fn record_action(tracking_dir: &str, initialized: bool) -> RecordAction {
    if tracking_dir.is_empty() {
        RecordAction::Ignore
    } else if !initialized {
        RecordAction::MissingTrackInit
    } else {
        RecordAction::Record
    }
}

/// Sets up baseline tracking and returns a cleanup closure that writes the
/// tracking file. Call it from a test entry point and run the returned closure
/// at teardown (Go uses `defer baseline.Track()()`).
///
/// When tracking is disabled (no `TSGO_BASELINE_TRACKING_DIR`), the returned
/// closure is a no-op.
///
/// # Examples
/// ```
/// let cleanup = tsgo_testutil_baseline::track();
/// cleanup(); // no-op unless TSGO_BASELINE_TRACKING_DIR is set
/// ```
///
/// Side effects: marks tracking as initialized; the returned closure may write
/// a tracking file under `TSGO_BASELINE_TRACKING_DIR`.
// Go: internal/testutil/baseline/testmain.go:Track
pub fn track() -> Box<dyn FnOnce()> {
    TRACKING_INITIALIZED.store(true, Ordering::SeqCst);

    let dir = tracking_dir();
    if dir.is_empty() {
        return Box::new(|| {});
    }

    // DIVERGENCE(port): hash the test binary path (per-crate) instead of the Go
    // call stack to obtain a stable per-package tracking filename.
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let h = fnv64a(exe.as_bytes());
    let tracking_path = Path::new(dir).join(format!("{h:016x}.txt"));

    Box::new(move || write_recorded_baselines(&tracking_path))
}

/// Records a baseline file path (relative to `baselines/reference`) for
/// unused-baseline tracking, reporting an error on `harness` if tracking is
/// enabled but [`track`] was never called.
///
/// Side effects: may mutate the shared recorded-baselines set; may push a
/// failure onto `harness`.
// Go: internal/testutil/baseline/testmain.go:recordBaseline
pub(crate) fn record_baseline(harness: &mut Harness, relative_path: &str) {
    match record_action(tracking_dir(), TRACKING_INITIALIZED.load(Ordering::SeqCst)) {
        RecordAction::Ignore => {}
        RecordAction::MissingTrackInit => harness.error(
            "baseline: package uses baselines but TestMain did not call baseline.Track(). \
             Please add a TestMain function with: defer baseline.Track()()",
        ),
        RecordAction::Record => recorded_baselines().add(relative_path.to_string()),
    }
}

/// Writes recorded baselines to `tracking_path`, aborting the process on I/O
/// error (mirrors Go's `os.Exit(1)`).
///
/// Side effects: writes `tracking_path`; may terminate the process.
// Go: internal/testutil/baseline/testmain.go:writeRecordedBaselines
fn write_recorded_baselines(tracking_path: &Path) {
    let baselines = recorded_baselines().keys();
    if baselines.is_empty() {
        return;
    }
    if let Err(e) = do_write_recorded_baselines(tracking_path, &baselines) {
        eprintln!(
            "baseline: failed to write tracking file {}: {e}",
            tracking_path.display()
        );
        std::process::exit(1);
    }
}

/// Writes one baseline path per line to `tracking_path`.
///
/// Side effects: writes (creates/truncates) `tracking_path`.
// Go: internal/testutil/baseline/testmain.go:doWriteRecordedBaselines
fn do_write_recorded_baselines(tracking_path: &Path, baselines: &[String]) -> std::io::Result<()> {
    let mut content = String::new();
    for b in baselines {
        content.push_str(b);
        content.push('\n');
    }
    std::fs::write(tracking_path, content)
}

/// FNV-1a 64-bit hash (mirrors Go `hash/fnv.New64a`).
///
/// Side effects: none (pure).
fn fnv64a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

#[cfg(test)]
#[path = "testmain_test.rs"]
mod tests;
