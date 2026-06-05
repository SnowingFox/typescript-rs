use std::sync::Mutex;
use std::time::Duration;

use super::*;

/// Test reporter that records all calls.
struct TestReporter {
    created_tokens: Mutex<Vec<String>>,
    progress_messages: Mutex<Vec<(String, WorkDoneProgressBeginOrReportOrEnd)>>,
}

impl TestReporter {
    fn new() -> Self {
        Self {
            created_tokens: Mutex::new(Vec::new()),
            progress_messages: Mutex::new(Vec::new()),
        }
    }

    fn created_tokens(&self) -> Vec<String> {
        self.created_tokens.lock().unwrap().clone()
    }

    fn progress_count(&self) -> usize {
        self.progress_messages.lock().unwrap().len()
    }

    fn last_progress(&self) -> Option<(String, WorkDoneProgressBeginOrReportOrEnd)> {
        self.progress_messages.lock().unwrap().last().cloned()
    }
}

impl ProgressReporter for TestReporter {
    fn localize(&self, key: &str) -> String {
        key.to_string()
    }

    fn create_work_done_progress(&self, token: &str) {
        self.created_tokens.lock().unwrap().push(token.to_string());
    }

    fn send_progress(&self, token: &str, value: WorkDoneProgressBeginOrReportOrEnd) {
        self.progress_messages
            .lock()
            .unwrap()
            .push((token.to_string(), value));
    }
}

// === Zero-delay tests (immediate progress) ===

#[test]
fn zero_delay_start_creates_token_and_begins() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::ZERO, "Loading");

    state.process_event(
        &ProgressEvent {
            message: "project A".to_string(),
            finish: false,
        },
        &reporter,
    );

    assert_eq!(reporter.created_tokens(), vec!["tsgo-loading-1"]);
    assert_eq!(reporter.progress_count(), 1);
    let (token, value) = reporter.last_progress().unwrap();
    assert_eq!(token, "tsgo-loading-1");
    assert!(value.begin.is_some());
    assert_eq!(value.begin.unwrap().title, "Loading");
}

#[test]
fn zero_delay_second_start_sends_report() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::ZERO, "Loading");

    state.process_event(
        &ProgressEvent {
            message: "project A".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "project B".to_string(),
            finish: false,
        },
        &reporter,
    );

    assert_eq!(reporter.progress_count(), 2);
    let (_, value) = reporter.last_progress().unwrap();
    assert!(value.report.is_some());
    assert_eq!(value.report.unwrap().message.as_deref(), Some("project B"));
}

#[test]
fn zero_delay_finish_sends_end_when_empty() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::ZERO, "Loading");

    state.process_event(
        &ProgressEvent {
            message: "project A".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "project A".to_string(),
            finish: true,
        },
        &reporter,
    );

    assert_eq!(reporter.progress_count(), 2);
    let (_, value) = reporter.last_progress().unwrap();
    assert!(value.end.is_some());
}

#[test]
fn zero_delay_finish_with_remaining_sends_report() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::ZERO, "Loading");

    state.process_event(
        &ProgressEvent {
            message: "project A".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "project B".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "project A".to_string(),
            finish: true,
        },
        &reporter,
    );

    assert_eq!(reporter.progress_count(), 3);
    let (_, value) = reporter.last_progress().unwrap();
    assert!(value.report.is_some());
    assert_eq!(value.report.unwrap().message.as_deref(), Some("project B"));
}

#[test]
fn zero_delay_ref_counting_start_finish() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::ZERO, "Loading");

    state.process_event(
        &ProgressEvent {
            message: "same".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "same".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "same".to_string(),
            finish: true,
        },
        &reporter,
    );

    assert!(state.is_loading());

    state.process_event(
        &ProgressEvent {
            message: "same".to_string(),
            finish: true,
        },
        &reporter,
    );

    assert!(!state.is_loading());
}

// === Delayed progress tests ===

#[test]
fn delayed_start_does_not_create_token_immediately() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::from_millis(100), "Loading");

    state.process_event(
        &ProgressEvent {
            message: "project A".to_string(),
            finish: false,
        },
        &reporter,
    );

    assert!(reporter.created_tokens().is_empty());
    assert_eq!(reporter.progress_count(), 0);
}

#[test]
fn delayed_finish_before_tick_sends_nothing() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::from_millis(100), "Loading");

    state.process_event(
        &ProgressEvent {
            message: "fast".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "fast".to_string(),
            finish: true,
        },
        &reporter,
    );

    assert!(reporter.created_tokens().is_empty());
    assert_eq!(reporter.progress_count(), 0);
    assert!(!state.is_loading());
}

#[test]
fn new_token_after_complete_cycle() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::ZERO, "Loading");

    state.process_event(
        &ProgressEvent {
            message: "first".to_string(),
            finish: false,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "first".to_string(),
            finish: true,
        },
        &reporter,
    );
    state.process_event(
        &ProgressEvent {
            message: "second".to_string(),
            finish: false,
        },
        &reporter,
    );

    let tokens = reporter.created_tokens();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0], "tsgo-loading-1");
    assert_eq!(tokens[1], "tsgo-loading-2");
}

#[test]
fn finish_without_start_is_harmless() {
    let reporter = TestReporter::new();
    let mut state = ProgressState::new(Duration::ZERO, "Loading");

    state.process_event(
        &ProgressEvent {
            message: "orphan".to_string(),
            finish: true,
        },
        &reporter,
    );

    assert_eq!(reporter.progress_count(), 0);
    assert!(!state.is_loading());
}

#[test]
fn token_returns_empty_when_no_progress() {
    let state = ProgressState::new(Duration::ZERO, "Loading");
    assert_eq!(state.token(), "");
}
