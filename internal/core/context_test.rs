//! Behavior tests for request context (Go has no `_test.go`; behavior-level).

use super::*;

// Go: internal/core/context.go:WithRequestID/GetRequestID (behavior-level; no Go _test.go)
#[test]
fn request_id_defaults_to_empty() {
    assert_eq!(RequestContext::default().request_id(), "");
}

// Go: internal/core/context.go:WithRequestID/GetRequestID (behavior-level; no Go _test.go)
#[test]
fn with_request_id_round_trips() {
    let ctx = RequestContext::default().with_request_id("req-1");
    assert_eq!(ctx.request_id(), "req-1");
    // Original is unchanged (value semantics, like deriving a new context).
    assert_eq!(RequestContext::default().request_id(), "");
}
