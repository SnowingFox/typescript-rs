//! Port of Go `internal/project/logging/logcollector.go`.
//!
//! Defines the [`LogCollector`] capability (a [`Logger`] whose output can be
//! read back) and [`new_test_logger`], an in-memory logger with a fixed
//! timestamp for deterministic test assertions.

use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use crate::{format_time, Logger, TimestampLogger};

/// The fixed instant pinned by Go's `NewTestLogger`: `time.Unix(1349085672, 0)`.
const TEST_INSTANT_SECS: u64 = 1_349_085_672;

/// A [`Logger`] that also exposes its accumulated output via [`fmt::Display`].
///
/// Mirrors Go's `LogCollector` (`fmt.Stringer` + `Logger`). Any type that is
/// both [`Logger`] and [`fmt::Display`] is a `LogCollector`, including
/// [`LogTree`](crate::LogTree) and the value from [`new_test_logger`].
// Go: internal/project/logging/logcollector.go:LogCollector
pub trait LogCollector: Logger + fmt::Display {}

impl<T: Logger + fmt::Display + ?Sized> LogCollector for T {}

/// In-memory sink shared between a logger and its reader.
#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedBuffer {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// Logger that buffers its output for retrieval (Go's private `logCollector`).
struct TestLogger {
    inner: TimestampLogger,
    buffer: SharedBuffer,
}

impl Logger for TestLogger {
    fn log(&self, message: &str) {
        self.inner.log(message);
    }

    fn logf(&self, args: fmt::Arguments<'_>) {
        self.inner.logf(args);
    }

    fn verbose(&self) -> Option<&dyn Logger> {
        self.inner.verbose()
    }

    fn is_verbose(&self) -> bool {
        self.inner.is_verbose()
    }

    fn set_verbose(&self, verbose: bool) {
        self.inner.set_verbose(verbose);
    }
}

impl fmt::Display for TestLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.buffer.contents())
    }
}

/// Creates a [`LogCollector`] that buffers output in memory and stamps every
/// line with a fixed timestamp, for deterministic tests.
///
/// The pinned instant matches Go's `NewTestLogger` (`time.Unix(1349085672, 0)`,
/// rendered as `[10:01:12.000]` in UTC).
///
/// # Examples
/// ```
/// use tsgo_project_logging::{new_test_logger, Logger};
///
/// let logger = new_test_logger();
/// logger.log("ready");
/// assert_eq!(logger.to_string(), "[10:01:12.000] ready\n");
/// ```
///
/// Side effects: buffers each log call in memory.
// Go: internal/project/logging/logcollector.go:NewTestLogger
pub fn new_test_logger() -> impl LogCollector {
    let buffer = SharedBuffer::default();
    let inner = TimestampLogger::with_prefix(
        Box::new(buffer.clone()),
        Box::new(|| format_time(UNIX_EPOCH + Duration::from_secs(TEST_INSTANT_SECS))),
    );
    TestLogger { inner, buffer }
}

#[cfg(test)]
#[path = "logcollector_test.rs"]
mod tests;
