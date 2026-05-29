use super::*;

/// Silences the panic hook (avoids noisy test output); set once per process.
fn silence_panics() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|_| {}));
    });
}

/// Equivalent of Go `testutil.AssertPanics`: runs `f` and asserts its panic
/// message equals `expected` byte-for-byte.
fn assert_panics(f: impl FnOnce() + std::panic::UnwindSafe, expected: &str) {
    silence_panics();
    let result = std::panic::catch_unwind(f);
    match result {
        Ok(()) => panic!("expected panic with message {expected:?}, but no panic occurred"),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            assert_eq!(msg, expected);
        }
    }
}

struct MockNode {
    kind: String,
}
impl KindString for MockNode {
    fn kind_string(&self) -> String {
        self.kind.clone()
    }
}
impl std::fmt::Display for MockNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)
    }
}

struct MockStringer {
    s: String,
}
impl std::fmt::Display for MockStringer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.s)
    }
}

// Go: internal/debug/debug_test.go:TestFailEmptyReason
#[test]
fn fail_empty_reason() {
    assert_panics(|| fail(""), "Debug failure.");
}

// Go: internal/debug/debug_test.go:TestFailWithReason
#[test]
fn fail_with_reason() {
    assert_panics(
        || fail("something went wrong"),
        "Debug failure. something went wrong",
    );
}

// Go: internal/debug/debug_test.go:TestFailBadSyntaxKindNoMessage
#[test]
fn fail_bad_syntax_kind_no_message() {
    assert_panics(
        || {
            fail_bad_syntax_kind(
                &MockNode {
                    kind: "FooNode".into(),
                },
                None,
            )
        },
        "Debug failure. Unexpected node.\nNode FooNode was unexpected.",
    );
}

// Go: internal/debug/debug_test.go:TestFailBadSyntaxKindWithMessage
#[test]
fn fail_bad_syntax_kind_with_message() {
    assert_panics(
        || {
            fail_bad_syntax_kind(
                &MockNode {
                    kind: "BarNode".into(),
                },
                Some("custom message"),
            )
        },
        "Debug failure. custom message\nNode BarNode was unexpected.",
    );
}

// Go: internal/debug/debug_test.go:TestAssertNeverDefaultMessageKindString
#[test]
fn assert_never_default_message_kind_string() {
    assert_panics(
        || {
            assert_never(
                MockNode {
                    kind: "TestNode".into(),
                },
                None,
            )
        },
        "Debug failure. Illegal value: TestNode",
    );
}

// Go: internal/debug/debug_test.go:TestAssertNeverCustomMessageKindString
#[test]
fn assert_never_custom_message_kind_string() {
    assert_panics(
        || {
            assert_never(
                MockNode {
                    kind: "TestNode".into(),
                },
                Some("bad value:"),
            )
        },
        "Debug failure. bad value: TestNode",
    );
}

// Go: internal/debug/debug_test.go:TestAssertNeverStringer
#[test]
fn assert_never_stringer() {
    assert_panics(
        || assert_never(MockStringer { s: "hello".into() }, None),
        "Debug failure. Illegal value: hello",
    );
}

// Go: internal/debug/debug_test.go:TestAssertNeverFallback
#[test]
fn assert_never_fallback() {
    assert_panics(
        || assert_never(42, None),
        "Debug failure. Illegal value: 42",
    );
}

// Go: internal/debug/debug_test.go:TestAssertTrue
#[test]
fn assert_true() {
    assert(true, None);
}

// Go: internal/debug/debug_test.go:TestAssertTrueWithMessage
#[test]
fn assert_true_with_message() {
    assert(true, Some("this should not trigger"));
}

// Go: internal/debug/debug_test.go:TestAssertFalseNoMessage
#[test]
fn assert_false_no_message() {
    assert_panics(|| assert(false, None), "Debug failure. False expression.");
}

// Go: internal/debug/debug_test.go:TestAssertFalseWithMessage
#[test]
fn assert_false_with_message() {
    assert_panics(
        || assert(false, Some("expected x > 0")),
        "Debug failure. False expression: expected x > 0",
    );
}
