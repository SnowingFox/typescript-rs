//! File change types.
//!
//! 1:1 port of Go `internal/project/filechange.go`.
//! Data types that represent individual file changes and batched change
//! summaries flowing from the LSP transport into the project system.

use tsgo_collections::Set;
use tsgo_lsproto::{DocumentUri, LanguageKind, TextDocumentContentChangePartialOrWholeDocument};

/// Threshold above which watch-event handling switches to a bulk invalidation
/// strategy.
// Go: internal/project/filechange.go:excessiveChangeThreshold
pub const EXCESSIVE_CHANGE_THRESHOLD: usize = 1000;

/// The kind of a single file change event.
///
/// # Examples
/// ```
/// use tsgo_project::filechange::FileChangeKind;
/// assert!(FileChangeKind::WatchCreate.is_watch_kind());
/// assert!(!FileChangeKind::Open.is_watch_kind());
/// ```
// Go: internal/project/filechange.go:FileChangeKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FileChangeKind {
    Open = 0,
    Close = 1,
    Change = 2,
    Save = 3,
    WatchCreate = 4,
    WatchChange = 5,
    WatchDelete = 6,
}

impl FileChangeKind {
    /// Reports whether this is a file-watcher event (create/change/delete).
    // Go: internal/project/filechange.go:IsWatchKind
    pub fn is_watch_kind(self) -> bool {
        matches!(
            self,
            FileChangeKind::WatchCreate | FileChangeKind::WatchChange | FileChangeKind::WatchDelete
        )
    }
}

/// A single file change event from the LSP client.
///
/// # Examples
/// ```
/// use tsgo_project::filechange::{FileChange, FileChangeKind};
/// use tsgo_lsproto::DocumentUri;
/// let change = FileChange {
///     kind: FileChangeKind::Open,
///     uri: DocumentUri("file:///a.ts".to_string()),
///     version: 1,
///     content: "let x = 1;".to_string(),
///     language_kind: Default::default(),
///     changes: Vec::new(),
/// };
/// assert_eq!(change.kind, FileChangeKind::Open);
/// ```
// Go: internal/project/filechange.go:FileChange
#[derive(Debug, Clone)]
pub struct FileChange {
    /// The kind of change.
    pub kind: FileChangeKind,
    /// The document URI.
    pub uri: DocumentUri,
    /// Document version (only set for Open/Change).
    pub version: i32,
    /// Full document content (only set for Open).
    pub content: String,
    /// Language identifier (only set for Open).
    pub language_kind: LanguageKind,
    /// Incremental changes (only set for Change).
    pub changes: Vec<TextDocumentContentChangePartialOrWholeDocument>,
}

/// A summary of file changes aggregated from a batch of [`FileChange`] events.
///
/// # Examples
/// ```
/// use tsgo_project::filechange::FileChangeSummary;
/// let s = FileChangeSummary::default();
/// assert!(s.is_empty());
/// ```
// Go: internal/project/filechange.go:FileChangeSummary
#[derive(Debug, Clone, Default)]
pub struct FileChangeSummary {
    /// Only one file can be opened at a time per request.
    pub opened: DocumentUri,
    /// Set if a close and open occurred for the same file in a single batch.
    pub reopened: DocumentUri,
    /// URIs of files that were closed.
    pub closed: Set<DocumentUri>,
    /// URIs of files that were changed.
    pub changed: Set<DocumentUri>,
    /// URIs of files that were created (only set when file watching is enabled).
    pub created: Set<DocumentUri>,
    /// URIs of files that were deleted (only set when file watching is enabled).
    pub deleted: Set<DocumentUri>,
    /// `true` if the summary includes a watch event outside `node_modules`.
    pub includes_watch_change_outside_node_modules: bool,
    /// Indicates that all cached file state should be discarded.
    pub invalidate_all: bool,
}

impl FileChangeSummary {
    /// Reports whether this summary is empty (no changes).
    // Go: internal/project/filechange.go:IsEmpty
    pub fn is_empty(&self) -> bool {
        !self.invalidate_all
            && self.opened.0.is_empty()
            && self.reopened.0.is_empty()
            && self.closed.len() == 0
            && self.changed.len() == 0
            && self.created.len() == 0
            && self.deleted.len() == 0
    }

    /// Reports whether the volume of watch events exceeds the bulk threshold.
    // Go: internal/project/filechange.go:HasExcessiveWatchEvents
    pub fn has_excessive_watch_events(&self) -> bool {
        self.invalidate_all
            || self.created.len() + self.deleted.len() + self.changed.len()
                > EXCESSIVE_CHANGE_THRESHOLD
    }

    /// Like [`Self::has_excessive_watch_events`] but ignores created files.
    // Go: internal/project/filechange.go:HasExcessiveNonCreateWatchEvents
    pub fn has_excessive_non_create_watch_events(&self) -> bool {
        self.invalidate_all || self.deleted.len() + self.changed.len() > EXCESSIVE_CHANGE_THRESHOLD
    }
}

/// Merges `src` into `dst`, combining their change sets.
///
/// # Side effects
/// Mutates `dst` in place.
// Go: internal/project/filechange.go:mergeFileChangeSummary
pub fn merge_file_change_summary(dst: &mut FileChangeSummary, src: FileChangeSummary) {
    if src.is_empty() {
        return;
    }
    if src.invalidate_all {
        dst.invalidate_all = true;
    }
    for uri in src.changed.keys() {
        dst.changed.add(uri.clone());
    }
    for uri in src.created.keys() {
        dst.created.add(uri.clone());
    }
    for uri in src.deleted.keys() {
        dst.deleted.add(uri.clone());
    }
    if src.includes_watch_change_outside_node_modules {
        dst.includes_watch_change_outside_node_modules = true;
    }
}

#[cfg(test)]
#[path = "filechange_test.rs"]
mod tests;
