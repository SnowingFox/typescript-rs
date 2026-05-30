use super::*;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

/// In-memory sink shared between the logger (which owns one clone) and the
/// test (which reads another). Lets us observe what a `new_logger` wrote.
#[derive(Clone, Default)]
struct SharedBuf(Arc<StdMutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedBuf {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

/// Asserts `out` is `[HH:MM:SS.mmm] <message>\n` without pinning the clock.
fn assert_timestamped_line(out: &str, message: &str) {
    let (prefix, rest) = out.split_once(' ').expect("space-separated prefix");
    assert_eq!(rest, format!("{message}\n"));
    let bytes = prefix.as_bytes();
    assert_eq!(prefix.len(), 14, "prefix {prefix:?}");
    for (i, b) in bytes.iter().enumerate() {
        match i {
            0 => assert_eq!(*b, b'['),
            13 => assert_eq!(*b, b']'),
            3 | 6 => assert_eq!(*b, b':'),
            9 => assert_eq!(*b, b'.'),
            _ => assert!(b.is_ascii_digit(), "expected digit at {i} in {prefix:?}"),
        }
    }
}

// Go: internal/project/logging/logger.go has no `*_test.go`; behavior covered
// by P10 parity. These behavior-level tests exercise the public API with
// values derived from the Go semantics (each anchored to its Go source item).

// Go: internal/project/logging/logger.go:formatTime
#[test]
fn format_time_renders_fixed_unix_instant() {
    // Go's `NewTestLogger` pins the clock to `time.Unix(1349085672, 0)`.
    // In UTC that instant is 10:01:12.000.
    let t = UNIX_EPOCH + Duration::from_secs(1_349_085_672);
    assert_eq!(format_time(t), "[10:01:12.000]");
}

// Go: internal/project/logging/logger.go:logger.Log
#[test]
fn log_writes_timestamped_message_line() {
    let buf = SharedBuf::default();
    let logger = new_logger(buf.clone());
    logger.log("hello world");
    assert_timestamped_line(&buf.contents(), "hello world");
}

// Go: internal/project/logging/logger.go:logger.Logf
#[test]
fn logf_writes_formatted_message_line() {
    let buf = SharedBuf::default();
    let logger = new_logger(buf.clone());
    logger.logf(format_args!("count={}", 5));
    assert_timestamped_line(&buf.contents(), "count=5");
}

// Go: internal/project/logging/logger.go:logger.Verbose
#[test]
fn verbose_gating_reflects_set_verbose() {
    let logger = new_logger(SharedBuf::default());

    // Fresh loggers are not verbose; `verbose()` yields nothing to log to.
    assert!(!logger.is_verbose());
    assert!(logger.verbose().is_none());

    logger.set_verbose(true);
    assert!(logger.is_verbose());
    assert!(logger.verbose().is_some());

    logger.set_verbose(false);
    assert!(!logger.is_verbose());
    assert!(logger.verbose().is_none());
}

// Go: internal/project/logging/logger.go:logger.Error (Warn/Info forward identically)
#[test]
fn level_methods_forward_to_log() {
    // The plain levels forward to `log`, the `*f` levels to `logf`; none add a
    // level marker, so every variant renders the same single timestamped line.
    fn rendered(call: impl FnOnce(&dyn Logger)) -> String {
        let buf = SharedBuf::default();
        let logger = new_logger(buf.clone());
        call(&logger);
        buf.contents()
    }
    assert_timestamped_line(&rendered(|l| l.error("msg")), "msg");
    assert_timestamped_line(&rendered(|l| l.warn("msg")), "msg");
    assert_timestamped_line(&rendered(|l| l.info("msg")), "msg");
    assert_timestamped_line(&rendered(|l| l.errorf(format_args!("ms{}", "g"))), "msg");
    assert_timestamped_line(&rendered(|l| l.warnf(format_args!("ms{}", "g"))), "msg");
    assert_timestamped_line(&rendered(|l| l.infof(format_args!("ms{}", "g"))), "msg");
}
