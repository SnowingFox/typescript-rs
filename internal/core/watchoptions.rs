//! Watch options (`WatchOptions`) and related enums.
//!
//! 1:1 port of Go `internal/core/watchoptions.go`.

use std::time::Duration;

use crate::tristate::Tristate;

/// How files are watched.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum WatchFileKind {
    /// No watching.
    #[default]
    None = 0,
    /// Fixed polling interval.
    FixedPollingInterval = 1,
    /// Priority polling interval.
    PriorityPollingInterval = 2,
    /// Dynamic priority polling.
    DynamicPriorityPolling = 3,
    /// Fixed chunk-size polling.
    FixedChunkSizePolling = 4,
    /// Use filesystem events.
    UseFsEvents = 5,
    /// Use filesystem events on the parent directory.
    UseFsEventsOnParentDirectory = 6,
}

/// How directories are watched.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum WatchDirectoryKind {
    /// No watching.
    #[default]
    None = 0,
    /// Use filesystem events.
    UseFsEvents = 1,
    /// Fixed polling interval.
    FixedPollingInterval = 2,
    /// Dynamic priority polling.
    DynamicPriorityPolling = 3,
    /// Fixed chunk-size polling.
    FixedChunkSizePolling = 4,
}

/// Fallback polling strategy.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum PollingKind {
    /// No polling.
    #[default]
    None = 0,
    /// Fixed interval.
    FixedInterval = 1,
    /// Priority interval.
    PriorityInterval = 2,
    /// Dynamic priority.
    DynamicPriority = 3,
    /// Fixed chunk size.
    FixedChunkSize = 4,
}

/// Options controlling file/directory watching.
#[derive(Clone, Debug, Default)]
pub struct WatchOptions {
    /// Polling interval in milliseconds (if set).
    pub interval: Option<i32>,
    /// How files are watched.
    pub file_kind: WatchFileKind,
    /// How directories are watched.
    pub directory_kind: WatchDirectoryKind,
    /// Fallback polling strategy.
    pub fallback_polling: PollingKind,
    /// Whether directory watching is synchronous.
    pub sync_watch_dir: Tristate,
    /// Directories to exclude.
    pub exclude_dir: Vec<String>,
    /// Files to exclude.
    pub exclude_files: Vec<String>,
}

impl WatchOptions {
    /// Returns the effective watch interval (default 2000ms).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/watchoptions.go:WatchInterval
    pub fn watch_interval(&self) -> Duration {
        match self.interval {
            Some(ms) => Duration::from_millis(ms as u64),
            None => Duration::from_millis(2000),
        }
    }
}

#[cfg(test)]
#[path = "watchoptions_test.rs"]
mod tests;
