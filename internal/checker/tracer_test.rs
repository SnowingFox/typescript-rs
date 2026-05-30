use super::*;

// Go: internal/checker/tracer.go:NewTracer
#[test]
fn checker_index_roundtrips() {
    assert_eq!(Tracer::new(7).checker_index(), 7);
    assert_eq!(Tracer::new(0).checker_index(), 0);
}

// Go: internal/checker/tracer.go:Tracer.copyWithCheckerIndex
#[test]
fn copy_with_checker_index_adds_checker_id() {
    let tracer = Tracer::new(7);
    let out = tracer.copy_with_checker_index(&Args::new());
    assert_eq!(out.get("checkerId"), Some(&ArgValue::Int(7)));
    assert_eq!(out.len(), 1);
}

// Go: internal/checker/tracer_test.go:TestTracerPushPreservesEndArgMutations
// (the portable invariant: injecting checkerId never mutates the caller's args)
#[test]
fn copy_with_checker_index_preserves_input_and_does_not_mutate_it() {
    let tracer = Tracer::new(7);
    let mut args = Args::new();
    args.insert("id".to_string(), ArgValue::Int(1));

    let out = tracer.copy_with_checker_index(&args);
    assert_eq!(out.get("id"), Some(&ArgValue::Int(1)));
    assert_eq!(out.get("checkerId"), Some(&ArgValue::Int(7)));

    // Mirrors the Go assertion `!hasCheckerID` on the caller's args.
    assert!(!args.contains_key("checkerId"));
    assert_eq!(args.len(), 1);
}

// Go: internal/checker/tracer.go:Tracer.copyWithCheckerIndex (overwrites existing)
#[test]
fn copy_with_checker_index_overwrites_existing_checker_id() {
    let tracer = Tracer::new(7);
    let mut args = Args::new();
    args.insert("checkerId".to_string(), ArgValue::Int(99));
    let out = tracer.copy_with_checker_index(&args);
    assert_eq!(out.get("checkerId"), Some(&ArgValue::Int(7)));
}
