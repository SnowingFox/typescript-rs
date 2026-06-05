use std::sync::Mutex;

use super::*;
use tsgo_project_logging::Logger;

/// Test sink that collects (MessageType, String) pairs.
struct TestSink {
    messages: Mutex<Vec<(MessageType, String)>>,
}

impl TestSink {
    fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
    }

    fn messages(&self) -> Vec<(MessageType, String)> {
        self.messages.lock().unwrap().clone()
    }
}

impl LogSink for &'static TestSink {
    fn send_log_message(&self, msg_type: MessageType, message: &str) {
        self.messages
            .lock()
            .unwrap()
            .push((msg_type, message.to_string()));
    }
}

// Use a wrapper to make it easier to test with non-static references.
struct OwnedTestSink(Mutex<Vec<(MessageType, String)>>);

impl OwnedTestSink {
    fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    fn messages(&self) -> Vec<(MessageType, String)> {
        self.0.lock().unwrap().clone()
    }
}

impl LogSink for std::sync::Arc<OwnedTestSink> {
    fn send_log_message(&self, msg_type: MessageType, message: &str) {
        self.0.lock().unwrap().push((msg_type, message.to_string()));
    }
}

fn make_logger() -> (LspLogger, std::sync::Arc<OwnedTestSink>) {
    let sink = std::sync::Arc::new(OwnedTestSink::new());
    let logger = LspLogger::new(sink.clone());
    (logger, sink)
}

#[test]
fn log_sends_message_type_log() {
    let (logger, sink) = make_logger();
    logger.log("hello world");
    let msgs = sink.messages();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].0, MessageType::Log);
    assert_eq!(msgs[0].1, "hello world");
}

#[test]
fn logf_sends_formatted_message() {
    let (logger, sink) = make_logger();
    logger.logf(format_args!("count: {}", 42));
    let msgs = sink.messages();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].0, MessageType::Log);
    assert_eq!(msgs[0].1, "count: 42");
}

#[test]
fn error_sends_message_type_error() {
    let (logger, sink) = make_logger();
    logger.error("something failed");
    let msgs = sink.messages();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].0, MessageType::Error);
    assert_eq!(msgs[0].1, "something failed");
}

#[test]
fn errorf_sends_formatted_error() {
    let (logger, sink) = make_logger();
    logger.errorf(format_args!("err: {}", "oops"));
    let msgs = sink.messages();
    assert_eq!(msgs[0].0, MessageType::Error);
    assert_eq!(msgs[0].1, "err: oops");
}

#[test]
fn warn_sends_message_type_warning() {
    let (logger, sink) = make_logger();
    logger.warn("be careful");
    let msgs = sink.messages();
    assert_eq!(msgs[0].0, MessageType::Warning);
}

#[test]
fn warnf_sends_formatted_warning() {
    let (logger, sink) = make_logger();
    logger.warnf(format_args!("warn: {}", "slow"));
    let msgs = sink.messages();
    assert_eq!(msgs[0].0, MessageType::Warning);
    assert_eq!(msgs[0].1, "warn: slow");
}

#[test]
fn info_sends_message_type_info() {
    let (logger, sink) = make_logger();
    logger.info("starting up");
    let msgs = sink.messages();
    assert_eq!(msgs[0].0, MessageType::Info);
    assert_eq!(msgs[0].1, "starting up");
}

#[test]
fn infof_sends_formatted_info() {
    let (logger, sink) = make_logger();
    logger.infof(format_args!("version: {}", "1.0"));
    let msgs = sink.messages();
    assert_eq!(msgs[0].0, MessageType::Info);
    assert_eq!(msgs[0].1, "version: 1.0");
}

#[test]
fn verbose_returns_none_by_default() {
    let (logger, _sink) = make_logger();
    assert!(logger.verbose().is_none());
    assert!(!logger.is_verbose());
}

#[test]
fn verbose_returns_some_after_set_verbose() {
    let (logger, _sink) = make_logger();
    logger.set_verbose(true);
    assert!(logger.verbose().is_some());
    assert!(logger.is_verbose());
}

#[test]
fn verbose_returns_none_after_disable() {
    let (logger, _sink) = make_logger();
    logger.set_verbose(true);
    logger.set_verbose(false);
    assert!(logger.verbose().is_none());
    assert!(!logger.is_verbose());
}

#[test]
fn verbose_logger_can_log() {
    let (logger, sink) = make_logger();
    logger.set_verbose(true);
    if let Some(v) = logger.verbose() {
        v.log("verbose message");
    }
    let msgs = sink.messages();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].1, "verbose message");
}
