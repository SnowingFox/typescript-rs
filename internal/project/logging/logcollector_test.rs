use super::*;

// Go: internal/project/logging/logcollector.go has no `*_test.go`; behavior
// covered by P10 parity. These behavior-level tests exercise the public API
// with values derived from the Go semantics (fixed `time.Unix(1349085672, 0)`).

// Go: internal/project/logging/logcollector.go:NewTestLogger
#[test]
fn test_logger_captures_line_with_fixed_timestamp() {
    let logger = new_test_logger();
    logger.log("ready");
    assert_eq!(logger.to_string(), "[10:01:12.000] ready\n");
}

// Go: internal/project/logging/logcollector.go:logCollector.String
#[test]
fn test_logger_accumulates_lines_in_order() {
    let logger = new_test_logger();
    logger.log("a");
    logger.logf(format_args!("b={}", 2));
    logger.error("c");
    assert_eq!(
        logger.to_string(),
        "[10:01:12.000] a\n[10:01:12.000] b=2\n[10:01:12.000] c\n"
    );
}

// Go: internal/project/logging/logtree.go `var _ LogCollector = (*LogTree)(nil)`
#[test]
fn log_tree_satisfies_log_collector() {
    fn assert_collector<T: LogCollector>() {}
    assert_collector::<crate::LogTree>();
}
