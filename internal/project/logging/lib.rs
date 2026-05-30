//! `tsgo_project_logging` — 1:1 Rust port of Go `internal/project/logging`.
//!
//! Provides the project-side logging abstraction used by the language service:
//! a timestamped [`Logger`] writing to an arbitrary sink, a tree-structured
//! collector ([`LogTree`]), and an in-memory test logger ([`new_test_logger`]).

use std::fmt;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Leveled logging interface used throughout the project package.
///
/// Mirrors Go's `logging.Logger` interface. The `*f` variants accept
/// pre-built [`std::fmt::Arguments`] (use [`format_args!`]); the plain
/// variants accept an already-rendered message. The error/warn/info levels
/// all forward to [`Logger::log`]/[`Logger::logf`], matching the Go source
/// where every level is rendered identically.
///
/// Go models an optional logger via a nil interface that is safe to call;
/// the Rust port instead returns [`Option`] from [`Logger::verbose`], so
/// callers pattern-match rather than relying on nil-receiver no-ops.
// Go: internal/project/logging/logger.go:Logger
pub trait Logger {
    /// Writes `message` to the sink, prefixed with a timestamp header.
    fn log(&self, message: &str);

    /// Writes formatted `args` to the sink, prefixed with a timestamp header.
    fn logf(&self, args: fmt::Arguments<'_>);

    /// Logs an error message. Forwards to [`Logger::log`].
    fn error(&self, message: &str) {
        self.log(message);
    }

    /// Logs a formatted error message. Forwards to [`Logger::logf`].
    fn errorf(&self, args: fmt::Arguments<'_>) {
        self.logf(args);
    }

    /// Logs a warning message. Forwards to [`Logger::log`].
    fn warn(&self, message: &str) {
        self.log(message);
    }

    /// Logs a formatted warning message. Forwards to [`Logger::logf`].
    fn warnf(&self, args: fmt::Arguments<'_>) {
        self.logf(args);
    }

    /// Logs an info message. Forwards to [`Logger::log`].
    fn info(&self, message: &str) {
        self.log(message);
    }

    /// Logs a formatted info message. Forwards to [`Logger::logf`].
    fn infof(&self, args: fmt::Arguments<'_>) {
        self.logf(args);
    }

    /// Returns the logger if verbose logging is enabled, otherwise [`None`].
    fn verbose(&self) -> Option<&dyn Logger>;

    /// Returns whether verbose logging is enabled.
    fn is_verbose(&self) -> bool;

    /// Enables or disables verbose logging.
    fn set_verbose(&self, verbose: bool);
}

struct TimestampState {
    verbose: bool,
    writer: Box<dyn Write + Send>,
}

/// Timestamped logger writing to an arbitrary sink (Go's private `logger`).
///
/// The `prefix` closure mirrors Go's `prefix func() string` field: it is
/// recomputed on every line, allowing the test logger to inject a fixed
/// timestamp while the production logger reads the wall clock.
struct TimestampLogger {
    state: Mutex<TimestampState>,
    prefix: Box<dyn Fn() -> String + Send + Sync>,
}

impl TimestampLogger {
    /// Builds a logger from an explicit sink and prefix source.
    ///
    /// The prefix closure is recomputed per line, mirroring Go's `prefix`
    /// field; [`new_logger`] passes a wall-clock source while the test logger
    /// passes a fixed timestamp.
    fn with_prefix(
        writer: Box<dyn Write + Send>,
        prefix: Box<dyn Fn() -> String + Send + Sync>,
    ) -> TimestampLogger {
        TimestampLogger {
            state: Mutex::new(TimestampState {
                verbose: false,
                writer,
            }),
            prefix,
        }
    }
}

impl Logger for TimestampLogger {
    fn log(&self, message: &str) {
        let mut state = self.state.lock().unwrap();
        let prefix = (self.prefix)();
        let _ = writeln!(state.writer, "{prefix} {message}");
    }

    fn logf(&self, args: fmt::Arguments<'_>) {
        let mut state = self.state.lock().unwrap();
        let prefix = (self.prefix)();
        let _ = writeln!(state.writer, "{prefix} {args}");
    }

    fn verbose(&self) -> Option<&dyn Logger> {
        if self.state.lock().unwrap().verbose {
            Some(self)
        } else {
            None
        }
    }

    fn is_verbose(&self) -> bool {
        self.state.lock().unwrap().verbose
    }

    fn set_verbose(&self, verbose: bool) {
        self.state.lock().unwrap().verbose = verbose;
    }
}

/// Creates a [`Logger`] that writes timestamped lines to `output`.
///
/// Each line is rendered as `<[HH:MM:SS.mmm]> <message>` followed by a
/// newline, where the timestamp is the current wall-clock time.
///
/// # Examples
/// ```
/// use tsgo_project_logging::{new_logger, Logger};
///
/// // Writes "[HH:MM:SS.mmm] hello\n" to the sink. To inspect captured output
/// // deterministically, use `new_test_logger` instead.
/// let logger = new_logger(std::io::sink());
/// logger.log("hello");
/// assert!(!logger.is_verbose());
/// ```
///
/// Side effects: writes to `output` on every log call.
// Go: internal/project/logging/logger.go:NewLogger
pub fn new_logger<W: Write + Send + 'static>(output: W) -> impl Logger {
    TimestampLogger::with_prefix(
        Box::new(output),
        Box::new(|| format_time(SystemTime::now())),
    )
}

/// Formats `t` as `[HH:MM:SS.mmm]`.
///
/// Side effects: none (pure).
///
/// Diverges from Go's `formatTime`, which renders in the host's local time
/// zone via `time.Time.Format`; the Rust port renders in UTC so output is
/// deterministic and the crate stays dependency-free. The textual structure
/// (`[HH:MM:SS.mmm]`) is identical.
// Go: internal/project/logging/logger.go:formatTime
fn format_time(t: SystemTime) -> String {
    let elapsed = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds_of_day = elapsed.as_secs() % 86_400;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day % 3_600) / 60;
    let seconds = seconds_of_day % 60;
    let millis = elapsed.subsec_millis();
    format!("[{hours:02}:{minutes:02}:{seconds:02}.{millis:03}]")
}

mod logcollector;
mod logtree;

pub use logcollector::{new_test_logger, LogCollector};
pub use logtree::LogTree;

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
