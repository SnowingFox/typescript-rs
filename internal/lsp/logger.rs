//! Port of Go `internal/lsp/logger.go`.
//!
//! An LSP-aware [`Logger`](tsgo_project_logging::Logger) that forwards log
//! messages as `window/logMessage` notifications to the LSP client.
//!
//! # Divergence from Go
//! - Go's `logger` holds a direct `*Server` reference. This port accepts a
//!   generic `LogSink` trait (a thin abstraction over message delivery) so the
//!   logger can be tested without a full Server instance.
//! - Go uses nil-receiver checks (`if l == nil`); Rust uses `Option<&LspLogger>`
//!   at the call site instead.

use std::sync::Mutex;

use tsgo_lsproto::MessageType;
use tsgo_project_logging::Logger;

/// Trait abstracting how the logger delivers messages.
///
/// The production implementation sends `window/logMessage` notifications; tests
/// can collect messages into a vec.
pub trait LogSink: Send + Sync {
    /// Delivers a log message with the given severity level.
    fn send_log_message(&self, msg_type: MessageType, message: &str);
}

/// An LSP logger that forwards messages to a [`LogSink`] as
/// `window/logMessage` notifications.
// Go: internal/lsp/logger.go:logger
pub struct LspLogger {
    sink: Box<dyn LogSink>,
    verbose: Mutex<bool>,
}

impl LspLogger {
    // Go: internal/lsp/logger.go:newLogger
    pub fn new(sink: impl LogSink + 'static) -> Self {
        Self {
            sink: Box::new(sink),
            verbose: Mutex::new(false),
        }
    }

    fn send(&self, msg_type: MessageType, message: &str) {
        self.sink.send_log_message(msg_type, message);
    }
}

impl Logger for LspLogger {
    fn log(&self, message: &str) {
        self.send(MessageType::Log, message);
    }

    fn logf(&self, args: std::fmt::Arguments<'_>) {
        self.send(MessageType::Log, &format!("{args}"));
    }

    fn error(&self, message: &str) {
        self.send(MessageType::Error, message);
    }

    fn errorf(&self, args: std::fmt::Arguments<'_>) {
        self.send(MessageType::Error, &format!("{args}"));
    }

    fn warn(&self, message: &str) {
        self.send(MessageType::Warning, message);
    }

    fn warnf(&self, args: std::fmt::Arguments<'_>) {
        self.send(MessageType::Warning, &format!("{args}"));
    }

    fn info(&self, message: &str) {
        self.send(MessageType::Info, message);
    }

    fn infof(&self, args: std::fmt::Arguments<'_>) {
        self.send(MessageType::Info, &format!("{args}"));
    }

    fn verbose(&self) -> Option<&dyn Logger> {
        if *self.verbose.lock().unwrap() {
            Some(self)
        } else {
            None
        }
    }

    fn is_verbose(&self) -> bool {
        *self.verbose.lock().unwrap()
    }

    fn set_verbose(&self, verbose: bool) {
        *self.verbose.lock().unwrap() = verbose;
    }
}

#[cfg(test)]
#[path = "logger_test.rs"]
mod tests;
