use super::*;

// Go: internal/testutil/baseline/testmain.go:recordBaseline (disabled -> ignore)
#[test]
fn record_action_ignores_when_no_tracking_dir() {
    assert_eq!(record_action("", false), RecordAction::Ignore);
    assert_eq!(record_action("", true), RecordAction::Ignore);
}

// Go: internal/testutil/baseline/testmain.go:recordBaseline (enabled, uninitialized -> error)
#[test]
fn record_action_errors_when_uninitialized() {
    assert_eq!(
        record_action("/some/tracking/dir", false),
        RecordAction::MissingTrackInit
    );
}

// Go: internal/testutil/baseline/testmain.go:recordBaseline (enabled, initialized -> record)
#[test]
fn record_action_records_when_initialized() {
    assert_eq!(
        record_action("/some/tracking/dir", true),
        RecordAction::Record
    );
}

// Go: internal/testutil/baseline/testmain.go:doWriteRecordedBaselines
#[test]
fn do_write_recorded_baselines_writes_one_per_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tracking.txt");
    let baselines = vec!["a/x.txt".to_string(), "b/y.txt".to_string()];
    do_write_recorded_baselines(&path, &baselines).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "a/x.txt\nb/y.txt\n"
    );
}

// Go: hash/fnv.New64a — FNV-1a 64-bit known vectors.
#[test]
fn fnv64a_known_vectors() {
    assert_eq!(fnv64a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv64a(b"a"), 0xaf63_dc4c_8601_ec8c);
}

// Go: internal/testutil/baseline/testmain.go:Track (disabled -> no-op cleanup)
#[test]
fn track_returns_runnable_cleanup() {
    // With TSGO_BASELINE_TRACKING_DIR unset in the test environment, the
    // returned cleanup must run without panicking or writing files.
    let cleanup = track();
    cleanup();
}

// Go: internal/testutil/baseline/testmain.go:recordBaseline (no failure when disabled)
#[test]
fn record_baseline_noop_when_disabled() {
    let mut h = Harness::new();
    record_baseline(&mut h, "some/path.txt");
    assert!(
        h.failures().is_empty(),
        "tracking disabled must not record a failure, got {:?}",
        h.failures()
    );
}
