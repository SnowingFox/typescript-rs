//! Behavior tests for watch options (Go has no `_test.go`; behavior-level).

use super::*;
use std::time::Duration;

// Go: internal/core/watchoptions.go:WatchInterval (behavior-level; no Go _test.go)
#[test]
fn watch_interval_defaults_to_2000ms_when_unset() {
    assert_eq!(
        WatchOptions::default().watch_interval(),
        Duration::from_millis(2000)
    );
}

// Go: internal/core/watchoptions.go:WatchInterval (behavior-level; no Go _test.go)
#[test]
fn watch_interval_uses_explicit_value() {
    let opts = WatchOptions {
        interval: Some(500),
        ..Default::default()
    };
    assert_eq!(opts.watch_interval(), Duration::from_millis(500));
}
