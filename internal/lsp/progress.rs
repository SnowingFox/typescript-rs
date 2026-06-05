//! Port of Go `internal/lsp/progress.go`.
//!
//! Manages LSP WorkDoneProgress indicators for long-running operations.
//! A single persistent task processes start/finish events, maintains a
//! ref-counted map of active operations, and sends progress messages.
//!
//! To avoid flickering on fast operations, the indicator is not shown until a
//! configurable delay has elapsed since the first start event.
//!
//! # Divergence from Go
//! - Go uses a goroutine + channels. This port uses a synchronous
//!   [`ProgressState`] that processes events one at a time, suitable for
//!   single-threaded test harnesses. A future async adapter can wrap this.
//! - Go has `*diagnostics.Message`; this port uses a plain string key so the
//!   progress logic can be tested without pulling in the diagnostics crate.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tsgo_lsproto::{
    WorkDoneProgressBegin, WorkDoneProgressBeginOrReportOrEnd, WorkDoneProgressEnd,
    WorkDoneProgressReport,
};

/// Trait abstracting the LSP transport operations needed by progress tracking.
// Go: internal/lsp/progress.go:progressReporter
pub trait ProgressReporter {
    /// Converts a message key to a display string.
    fn localize(&self, key: &str) -> String;
    /// Asks the client to create a progress token.
    fn create_work_done_progress(&self, token: &str);
    /// Sends a `$/progress` notification.
    fn send_progress(&self, token: &str, value: WorkDoneProgressBeginOrReportOrEnd);
}

/// A progress event: either starting or finishing an operation.
#[derive(Debug, Clone)]
pub struct ProgressEvent {
    /// The message key identifying this operation.
    pub message: String,
    /// `true` if this is a finish event, `false` for a start event.
    pub finish: bool,
}

/// Synchronous state machine for project-loading progress reporting.
///
/// Call [`ProgressState::process_event`] for each start/finish event.
/// When a delay is configured and a start event arrives, progress UI is deferred
/// until [`ProgressState::tick`] is called after the delay elapses.
// Go: internal/lsp/progress.go:projectLoadingProgress (the `run` goroutine state)
pub struct ProgressState {
    /// Ordered map: message text → ref count.
    loading: VecDeque<(String, usize)>,
    /// Current progress token; empty = no active progress.
    token: String,
    token_id: u32,
    /// Whether `begin` has been sent for the current token.
    begun: bool,
    /// Whether the delay timer has fired (or is zero).
    delay_fired: bool,
    /// The configured delay before showing progress.
    delay: Duration,
    /// The instant when the delay timer was started (if active).
    delay_start: Option<Instant>,
    /// Title used for the "begin" message (localized from "Loading").
    loading_title: String,
}

impl ProgressState {
    /// Creates a new progress state with the given delay.
    pub fn new(delay: Duration, loading_title: impl Into<String>) -> Self {
        Self {
            loading: VecDeque::new(),
            token: String::new(),
            token_id: 0,
            begun: false,
            delay_fired: false,
            delay,
            delay_start: None,
            loading_title: loading_title.into(),
        }
    }

    /// Processes a start or finish event, issuing progress notifications
    /// through the reporter as needed.
    // Go: internal/lsp/progress.go:projectLoadingProgress.run (event branch)
    pub fn process_event(&mut self, ev: &ProgressEvent, reporter: &dyn ProgressReporter) {
        let text = reporter.localize(&ev.message);
        if !ev.finish {
            self.handle_start(&text, reporter);
        } else {
            self.handle_finish(&text, reporter);
        }
    }

    /// Should be called periodically; fires the delay timer if elapsed.
    // Go: internal/lsp/progress.go:projectLoadingProgress.run (delayC branch)
    pub fn tick(&mut self, reporter: &dyn ProgressReporter) {
        if self.delay_fired {
            return;
        }
        if let Some(start) = self.delay_start {
            if start.elapsed() >= self.delay {
                self.delay_start = None;
                self.delay_fired = true;
                if !self.token.is_empty() && !self.loading.is_empty() {
                    reporter.create_work_done_progress(&self.token);
                    let first = self
                        .loading
                        .front()
                        .map(|(k, _)| k.clone())
                        .unwrap_or_default();
                    self.begun =
                        self.begin_or_report(&self.token.clone(), &first, self.begun, reporter);
                }
            }
        }
    }

    /// Returns `true` if there are any active loading operations.
    pub fn is_loading(&self) -> bool {
        !self.loading.is_empty()
    }

    /// Returns the current progress token (empty if none active).
    pub fn token(&self) -> &str {
        &self.token
    }

    fn handle_start(&mut self, text: &str, reporter: &dyn ProgressReporter) {
        if let Some(entry) = self.loading.iter_mut().find(|(k, _)| k == text) {
            entry.1 += 1;
        } else {
            self.loading.push_back((text.to_string(), 1));
        }

        if self.token.is_empty() {
            self.token_id += 1;
            self.token = format!("tsgo-loading-{}", self.token_id);
            self.begun = false;
            if self.delay.is_zero() {
                self.delay_fired = true;
                reporter.create_work_done_progress(&self.token);
            } else {
                self.delay_fired = false;
                self.delay_start = Some(Instant::now());
            }
        }

        if self.delay_fired {
            self.begun = self.begin_or_report(&self.token.clone(), text, self.begun, reporter);
        }
    }

    fn handle_finish(&mut self, text: &str, reporter: &dyn ProgressReporter) {
        if let Some(pos) = self.loading.iter().position(|(k, _)| k == text) {
            let count = self.loading[pos].1;
            if count <= 1 {
                self.loading.remove(pos);
            } else {
                self.loading[pos].1 -= 1;
            }
        }

        if self.token.is_empty() {
            return;
        }

        if self.loading.is_empty() {
            if self.begun {
                reporter.send_progress(
                    &self.token,
                    WorkDoneProgressBeginOrReportOrEnd {
                        end: Some(WorkDoneProgressEnd {
                            kind: Default::default(),
                            message: None,
                        }),
                        ..Default::default()
                    },
                );
            }
            self.delay_start = None;
            self.token.clear();
        } else if self.delay_fired {
            let first = self
                .loading
                .front()
                .map(|(k, _)| k.clone())
                .unwrap_or_default();
            reporter.send_progress(
                &self.token,
                WorkDoneProgressBeginOrReportOrEnd {
                    report: Some(WorkDoneProgressReport {
                        kind: Default::default(),
                        cancellable: None,
                        message: Some(first),
                        percentage: None,
                    }),
                    ..Default::default()
                },
            );
        }
    }

    fn begin_or_report(
        &self,
        token: &str,
        text: &str,
        begun: bool,
        reporter: &dyn ProgressReporter,
    ) -> bool {
        if !begun {
            reporter.send_progress(
                token,
                WorkDoneProgressBeginOrReportOrEnd {
                    begin: Some(WorkDoneProgressBegin {
                        kind: Default::default(),
                        title: self.loading_title.clone(),
                        message: Some(text.to_string()),
                        cancellable: None,
                        percentage: None,
                    }),
                    ..Default::default()
                },
            );
        } else {
            reporter.send_progress(
                token,
                WorkDoneProgressBeginOrReportOrEnd {
                    report: Some(WorkDoneProgressReport {
                        kind: Default::default(),
                        cancellable: None,
                        message: Some(text.to_string()),
                        percentage: None,
                    }),
                    ..Default::default()
                },
            );
        }
        true
    }
}

#[cfg(test)]
#[path = "progress_test.rs"]
mod tests;
