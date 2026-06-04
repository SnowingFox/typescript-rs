//! Editor overlay filesystem.
//!
//! 1:1 port of Go `internal/project/overlayfs.go`.
//!
//! Manages the overlay layer that sits between the LSP client and the
//! underlying disk filesystem. Open files are tracked as [`Overlay`]s
//! with content that may differ from what is on disk. The
//! [`OverlayFS::process_changes`] method coalesces a batch of
//! [`FileChange`] events into a [`FileChangeSummary`] and an updated
//! set of overlays.

use std::collections::HashMap;

use tsgo_core::scriptkind::ScriptKind;
use tsgo_lsproto::DocumentUri;
use tsgo_tspath::Path;
use tsgo_vfs::Fs;

use crate::filechange::{FileChange, FileChangeKind, FileChangeSummary};

/// Content-access interface shared by disk files and overlays.
// Go: internal/project/overlayfs.go:FileContent
pub trait FileContent {
    /// Returns the text content.
    fn content(&self) -> &str;
    /// Returns a 128-bit content hash.
    fn hash(&self) -> u128;
}

/// Handle for a file tracked by the overlay filesystem.
// Go: internal/project/overlayfs.go:FileHandle
pub trait FileHandle: FileContent {
    /// The absolute file name.
    fn file_name(&self) -> &str;
    /// Document version (0 for disk files).
    fn version(&self) -> i32;
    /// Whether the overlay content matches the on-disk text.
    fn matches_disk_text(&self) -> bool;
    /// Whether this is an editor overlay (as opposed to a disk file).
    fn is_overlay(&self) -> bool;
    /// The inferred script kind based on file extension.
    fn kind(&self) -> ScriptKind;
}

/// A file read from disk (not opened in the editor).
// Go: internal/project/overlayfs.go:diskFile
#[derive(Debug, Clone)]
pub struct DiskFile {
    file_name: String,
    text: String,
    content_hash: u128,
    needs_reload: bool,
}

impl DiskFile {
    /// Creates a new disk file handle.
    // Go: internal/project/overlayfs.go:newDiskFile
    pub fn new(file_name: &str, content: &str) -> Self {
        Self {
            file_name: file_name.to_string(),
            text: content.to_string(),
            content_hash: hash_content(content),
            needs_reload: false,
        }
    }
}

impl FileContent for DiskFile {
    fn content(&self) -> &str {
        &self.text
    }

    fn hash(&self) -> u128 {
        self.content_hash
    }
}

impl FileHandle for DiskFile {
    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn version(&self) -> i32 {
        0
    }

    fn matches_disk_text(&self) -> bool {
        !self.needs_reload
    }

    fn is_overlay(&self) -> bool {
        false
    }

    fn kind(&self) -> ScriptKind {
        tsgo_core::get_script_kind_from_file_name(&self.file_name)
    }
}

/// An open-in-editor file with content potentially diverging from disk.
// Go: internal/project/overlayfs.go:Overlay
#[derive(Debug, Clone)]
pub struct Overlay {
    file_name: String,
    text: String,
    content_hash: u128,
    ver: i32,
    script_kind: ScriptKind,
    matches_disk: bool,
}

impl Overlay {
    /// Creates a new overlay.
    // Go: internal/project/overlayfs.go:newOverlay
    pub fn new(file_name: String, content: String, version: i32, kind: ScriptKind) -> Self {
        let content_hash = hash_content(&content);
        Self {
            file_name,
            text: content,
            content_hash,
            ver: version,
            script_kind: kind,
            matches_disk: false,
        }
    }

    /// Checks whether this overlay's content matches the file on disk.
    ///
    /// May return false negatives, but never false positives.
    // Go: internal/project/overlayfs.go:Overlay.computeMatchesDiskText
    pub fn compute_matches_disk_text(&self, fs: &dyn Fs) -> (bool, bool) {
        if tsgo_tspath::is_dynamic_file_name(&self.file_name) {
            return (false, false);
        }
        match fs.read_file(&self.file_name) {
            Some(disk_content) => (hash_content(&disk_content) == self.content_hash, true),
            None => (false, false),
        }
    }
}

impl FileContent for Overlay {
    fn content(&self) -> &str {
        &self.text
    }

    fn hash(&self) -> u128 {
        self.content_hash
    }
}

impl FileHandle for Overlay {
    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn version(&self) -> i32 {
        self.ver
    }

    fn matches_disk_text(&self) -> bool {
        self.matches_disk
    }

    fn is_overlay(&self) -> bool {
        true
    }

    fn kind(&self) -> ScriptKind {
        self.script_kind
    }
}

/// A boxed file handle (either DiskFile or Overlay).
pub type BoxedFileHandle = Box<dyn FileHandle + Send + Sync>;

/// The overlay filesystem merges editor-open files over the real FS.
// Go: internal/project/overlayfs.go:overlayFS
pub struct OverlayFS {
    to_path: Box<dyn Fn(&str) -> Path + Send + Sync>,
    fs: Box<dyn Fs + Send + Sync>,
    overlays: HashMap<Path, Overlay>,
}

impl OverlayFS {
    /// Creates a new overlay FS.
    // Go: internal/project/overlayfs.go:newOverlayFS
    pub fn new(
        fs: Box<dyn Fs + Send + Sync>,
        overlays: HashMap<Path, Overlay>,
        to_path: impl Fn(&str) -> Path + Send + Sync + 'static,
    ) -> Self {
        Self {
            fs,
            overlays,
            to_path: Box::new(to_path),
        }
    }

    /// Returns a reference to the current overlays.
    // Go: internal/project/overlayfs.go:overlayFS.Overlays
    pub fn overlays(&self) -> &HashMap<Path, Overlay> {
        &self.overlays
    }

    /// Checks if a file exists in either overlays or the underlying FS.
    // Go: internal/project/overlayfs.go:overlayFS.FileExists (implicit)
    pub fn file_exists(&self, file_name: &str) -> bool {
        let path = (self.to_path)(file_name);
        if self.overlays.contains_key(&path) {
            return true;
        }
        self.fs.file_exists(file_name)
    }

    /// Looks up a file by name. Returns the overlay if open, otherwise reads from disk.
    // Go: internal/project/overlayfs.go:overlayFS.getFile
    pub fn get_file(&self, file_name: &str) -> Option<BoxedFileHandle> {
        let path = (self.to_path)(file_name);
        if let Some(overlay) = self.overlays.get(&path) {
            return Some(Box::new(overlay.clone()));
        }
        self.fs
            .read_file(file_name)
            .map(|content| Box::new(DiskFile::new(file_name, &content)) as BoxedFileHandle)
    }

    /// Coalesces a batch of file changes into a summary and new overlays.
    ///
    /// # Panics
    /// - If more than one file is opened in a single batch.
    /// - If changes arrive after a close event for the same file.
    /// - If changes arrive for a file with no overlay.
    // Go: internal/project/overlayfs.go:overlayFS.processChanges
    pub fn process_changes(
        &mut self,
        changes: &[FileChange],
    ) -> (FileChangeSummary, HashMap<Path, Overlay>) {
        let mut result = FileChangeSummary::default();
        let mut new_overlays = self.overlays.clone();

        // Per-file event accumulator
        struct FileEvents {
            open_change: Option<usize>,
            close_change: Option<usize>,
            watch_changed: bool,
            changes: Vec<usize>,
            saved: bool,
            created: bool,
            deleted: bool,
        }

        let mut file_event_map: HashMap<DocumentUri, FileEvents> = HashMap::new();

        for (idx, change) in changes.iter().enumerate() {
            let uri = &change.uri;
            let events = file_event_map.entry(uri.clone()).or_insert(FileEvents {
                open_change: None,
                close_change: None,
                watch_changed: false,
                changes: Vec::new(),
                saved: false,
                created: false,
                deleted: false,
            });

            if events.open_change.is_some() {
                panic!("should see no changes after open");
            }

            if !result.includes_watch_change_outside_node_modules
                && change.kind.is_watch_kind()
                && !uri.0.contains("/node_modules/")
            {
                result.includes_watch_change_outside_node_modules = true;
            }

            match change.kind {
                FileChangeKind::Open => {
                    if events.close_change.is_some() {
                        events.close_change = None;
                    }
                    events.open_change = Some(idx);
                    events.watch_changed = false;
                    events.changes.clear();
                    events.saved = false;
                    events.created = false;
                    events.deleted = false;
                }
                FileChangeKind::Close => {
                    events.close_change = Some(idx);
                    events.changes.clear();
                    events.saved = false;
                    events.watch_changed = false;
                }
                FileChangeKind::Change => {
                    if events.close_change.is_some() {
                        panic!("should see no changes after close");
                    }
                    events.changes.push(idx);
                    events.saved = false;
                    events.watch_changed = false;
                }
                FileChangeKind::Save => {
                    events.saved = true;
                }
                FileChangeKind::WatchCreate => {
                    if events.deleted {
                        events.deleted = false;
                        events.watch_changed = true;
                    } else {
                        events.created = true;
                    }
                }
                FileChangeKind::WatchChange => {
                    if !events.created {
                        events.watch_changed = true;
                        events.saved = false;
                    }
                }
                FileChangeKind::WatchDelete => {
                    events.watch_changed = false;
                    events.saved = false;
                    if events.created {
                        events.created = false;
                    } else {
                        events.deleted = true;
                    }
                }
            }
        }

        // Process deduplicated events per file
        for (uri, events) in &file_event_map {
            let path = uri.path(self.fs.use_case_sensitive_file_names());
            let o = new_overlays.get(&path).cloned();

            if let Some(open_idx) = events.open_change {
                let open_change = &changes[open_idx];
                if !result.opened.0.is_empty() || !result.reopened.0.is_empty() {
                    panic!("can only process one file open event at a time");
                }
                if let Some(ref existing) = o {
                    if existing.content() != open_change.content {
                        result.changed.add(uri.clone());
                    } else {
                        result.reopened = uri.clone();
                    }
                } else {
                    result.opened = uri.clone();
                }
                new_overlays.insert(
                    path.clone(),
                    Overlay::new(
                        uri.file_name(),
                        open_change.content.clone(),
                        open_change.version,
                        language_kind_to_script_kind(open_change.language_kind.clone()),
                    ),
                );
                continue;
            }

            if events.close_change.is_some() {
                if o.is_none() {
                    panic!("overlay not found for closed file: {}", uri.0);
                }
                result.closed.add(uri.clone());
                new_overlays.remove(&path);
            }
            let o = new_overlays.get(&path).cloned();

            if events.watch_changed {
                if o.is_none() {
                    result.changed.add(uri.clone());
                } else if let Some(ref existing) = o {
                    if !events.saved {
                        let (matches_disk, _) =
                            existing.compute_matches_disk_text(self.fs.as_ref());
                        if matches_disk != existing.matches_disk_text() {
                            let mut updated = Overlay::new(
                                existing.file_name().to_string(),
                                existing.content().to_string(),
                                existing.version(),
                                existing.kind(),
                            );
                            updated.matches_disk = matches_disk;
                            new_overlays.insert(path.clone(), updated);
                        }
                    }
                }
            }

            // Incremental text changes are deferred — they require Converters (lsconv)
            // which needs the full text change application pipeline.
            // DEFER(phase-8): port incremental text change application
            if !events.changes.is_empty() {
                result.changed.add(uri.clone());
                if o.is_none() {
                    panic!("overlay not found for changed file: {}", uri.0);
                }
                // For now, each change replaces the whole document content if WholeDocument is set
                let mut current = o.unwrap();
                for &change_idx in &events.changes {
                    let change = &changes[change_idx];
                    for text_change in &change.changes {
                        if let Some(ref whole) = text_change.whole_document {
                            current = Overlay::new(
                                current.file_name.clone(),
                                whole.text.clone(),
                                change.version,
                                current.script_kind,
                            );
                        }
                        // Partial changes deferred — needs lsconv Converters
                    }
                    if !change.changes.is_empty() {
                        current.ver = change.version;
                        current.content_hash = hash_content(&current.text);
                        current.matches_disk = false;
                        new_overlays.insert(path.clone(), current.clone());
                    }
                }
            }

            if events.saved {
                if let Some(existing) = new_overlays.get(&path) {
                    let mut updated = Overlay::new(
                        existing.file_name().to_string(),
                        existing.content().to_string(),
                        existing.version(),
                        existing.kind(),
                    );
                    updated.matches_disk = true;
                    new_overlays.insert(path.clone(), updated);
                } else if !events.watch_changed {
                    result.changed.add(uri.clone());
                }
            }

            if events.created && !new_overlays.contains_key(&path) {
                result.created.add(uri.clone());
            }

            if events.deleted && !new_overlays.contains_key(&path) {
                result.deleted.add(uri.clone());
            }
        }

        self.overlays = new_overlays.clone();
        (result, new_overlays)
    }
}

/// Map LSP LanguageKind to core ScriptKind.
// Go: internal/ls/lsconv/lsconv.go:LanguageKindToScriptKind
fn language_kind_to_script_kind(lk: tsgo_lsproto::LanguageKind) -> ScriptKind {
    if lk == tsgo_lsproto::LanguageKind::TYPE_SCRIPT {
        ScriptKind::Ts
    } else if lk == tsgo_lsproto::LanguageKind::JAVA_SCRIPT {
        ScriptKind::Js
    } else if lk == tsgo_lsproto::LanguageKind::TYPE_SCRIPT_REACT {
        ScriptKind::Tsx
    } else if lk == tsgo_lsproto::LanguageKind::JAVA_SCRIPT_REACT {
        ScriptKind::Jsx
    } else {
        ScriptKind::Unknown
    }
}

/// Compute a 128-bit content hash.
///
/// Uses the standard library's `DefaultHasher` to produce a stable hash
/// consistent across runs. We use two independent seeds to fill 128 bits.
fn hash_content(s: &str) -> u128 {
    use std::hash::{Hash, Hasher};
    let mut h1 = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h1);
    let lo = h1.finish() as u128;
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    s.len().hash(&mut h2);
    s.hash(&mut h2);
    let hi = h2.finish() as u128;
    (hi << 64) | lo
}

#[cfg(test)]
#[path = "overlayfs_test.rs"]
mod tests;
