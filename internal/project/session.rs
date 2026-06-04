//! Session — manages the mutable state of an LSP session.
//!
//! 1:1 port of Go `internal/project/session.go` (MVP subset).
//!
//! A [`Session`] receives textDocument events (`did_open_file`, `did_close_file`,
//! `did_change_file`) and maintains an immutable [`Snapshot`] that is rebuilt
//! on each state transition. Callers query project ownership via
//! [`Session::get_language_service`].
//!
//! ## Simplifications (P8 MVP)
//! - No background queue, debouncing, or watch management.
//! - No ATA (Automatic Type Acquisition).
//! - No telemetry or performance metrics.
//! - `get_language_service` returns project info rather than full LS (blocked
//!   on `tsgo_ls` crate availability).

use std::sync::Arc;

use tsgo_lsproto::{DocumentUri, LanguageKind};
use tsgo_tspath::Path;

use crate::client::Client;
use crate::configfileregistry::ConfigFileRegistry;
use crate::filechange::{FileChange, FileChangeKind};
use crate::overlayfs::{FileHandle, OverlayFS};
use crate::project::Project;
use crate::projectcollection::ProjectCollection;
use crate::snapshot::Snapshot;

/// Describes why a snapshot update was triggered.
///
/// # Examples
/// ```
/// use tsgo_project::session::UpdateReason;
/// assert_ne!(UpdateReason::Unknown as i32, UpdateReason::DidOpenFile as i32);
/// ```
///
/// Side effects: none (pure enum).
// Go: internal/project/session.go:UpdateReason
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum UpdateReason {
    /// No specific reason.
    Unknown = 0,
    /// A file was opened in the editor.
    DidOpenFile = 1,
    /// A file was closed in the editor.
    DidCloseFile = 2,
    /// Compiler options for inferred projects changed.
    DidChangeCompilerOptionsForInferredProjects = 3,
    /// A language service was requested and there are pending file changes.
    RequestedLanguageServicePendingChanges = 4,
    /// A language service was requested for a project not yet loaded.
    RequestedLanguageServiceProjectNotLoaded = 5,
    /// A language service was requested for a file that is not open.
    RequestedLanguageServiceForFileNotOpen = 6,
    /// A language service was requested for a dirty project.
    RequestedLanguageServiceProjectDirty = 7,
    /// A project tree load was requested.
    RequestedLoadProjectTree = 8,
    /// A language service with auto-imports was requested.
    RequestedLanguageServiceWithAutoImports = 9,
    /// Idle disk cache cleaning was triggered.
    IdleCleanDiskCache = 10,
}

/// Immutable initialization options for a session.
///
/// # Examples
/// ```
/// use tsgo_project::session::SessionOptions;
/// let opts = SessionOptions {
///     current_directory: "/app".to_string(),
/// };
/// assert_eq!(opts.current_directory, "/app");
/// ```
///
/// Side effects: none (pure data).
// Go: internal/project/session.go:SessionOptions
#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// The workspace root directory.
    pub current_directory: String,
    // DEFER(phase-8): DefaultLibraryPath, TypingsLocation, PositionEncoding,
    // WatchEnabled, LoggingEnabled, TelemetryEnabled, PushDiagnosticsEnabled,
    // DebounceDelay, Locale.
}

/// Manages the mutable state of an LSP session.
///
/// Receives textDocument events and maintains an immutable [`Snapshot`]
/// that is rebuilt on each state transition.
///
/// # Examples
/// ```
/// use tsgo_project::session::{Session, SessionOptions};
/// use tsgo_project::overlayfs::OverlayFS;
/// use tsgo_project::client::NoopClient;
/// use std::sync::Arc;
///
/// let empty: Vec<(&str, &str)> = Vec::new();
/// let fs: Box<dyn tsgo_vfs::Fs + Send + Sync> =
///     Box::new(tsgo_vfs::vfstest::MapFs::from_map(empty, false));
/// let overlay_fs = OverlayFS::new(fs, std::collections::HashMap::new(),
///     |s: &str| tsgo_tspath::Path(s.to_lowercase()));
/// let opts = SessionOptions { current_directory: "/".to_string() };
/// let session = Session::new(opts, overlay_fs, Arc::new(NoopClient));
/// assert_eq!(session.snapshot().id(), 0);
/// ```
///
/// Side effects: none on construction; mutations via `did_*` methods update
/// internal overlay and snapshot state.
// Go: internal/project/session.go:Session
pub struct Session {
    options: SessionOptions,
    overlay_fs: OverlayFS,
    snapshot: Snapshot,
    snapshot_id: u64,
    #[allow(dead_code)]
    client: Arc<dyn Client>,
    to_path: Box<dyn Fn(&str) -> Path + Send + Sync>,
}

impl Session {
    /// Creates a new session with an empty initial snapshot.
    ///
    /// Side effects: none (pure construction).
    // Go: internal/project/session.go:NewSession
    pub fn new(options: SessionOptions, overlay_fs: OverlayFS, client: Arc<dyn Client>) -> Self {
        let current_directory = options.current_directory.clone();
        let to_path: Box<dyn Fn(&str) -> Path + Send + Sync> = Box::new(move |file_name: &str| {
            tsgo_tspath::to_path(file_name, &current_directory, false)
        });

        let pc = ProjectCollection::new(
            Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
            ConfigFileRegistry::new(),
        );
        let snapshot = Snapshot::new(0, pc);

        Self {
            options,
            overlay_fs,
            snapshot,
            snapshot_id: 0,
            client,
            to_path,
        }
    }

    /// Returns a reference to the current immutable snapshot.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/session.go:Session.Snapshot
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Checks if the overlay filesystem has a file with the given URI.
    ///
    /// Side effects: none (pure read).
    pub fn overlay_has_file(&self, uri_str: &str) -> bool {
        let uri = DocumentUri(uri_str.to_string());
        let path = uri.path(false);
        self.overlay_fs.overlays().contains_key(&path)
    }

    /// Handles a `textDocument/didOpen` notification from the LSP client.
    ///
    /// Adds the file to the overlay filesystem, creates an inferred project
    /// if no configured project matches, and rebuilds the snapshot.
    ///
    /// Side effects: mutates internal overlay and snapshot state.
    // Go: internal/project/session.go:Session.DidOpenFile
    pub fn did_open_file(
        &mut self,
        uri_str: &str,
        version: i32,
        content: &str,
        language_kind: LanguageKind,
    ) {
        let uri = DocumentUri(uri_str.to_string());

        let changes = vec![FileChange {
            kind: FileChangeKind::Open,
            uri: uri.clone(),
            version,
            content: content.to_string(),
            language_kind,
            changes: Vec::new(),
        }];
        self.overlay_fs.process_changes(&changes);

        self.snapshot_id += 1;
        let mut pc = self.snapshot.project_collection().shallow_clone();

        self.ensure_project_for_file(&uri, &mut pc);

        self.snapshot = Snapshot::new(self.snapshot_id, pc);
    }

    /// Handles a `textDocument/didClose` notification from the LSP client.
    ///
    /// Removes the file from the overlay filesystem. If the file was the
    /// last one in the inferred project, the inferred project is removed.
    ///
    /// Side effects: mutates internal overlay and snapshot state.
    // Go: internal/project/session.go:Session.DidCloseFile
    pub fn did_close_file(&mut self, uri_str: &str) {
        let uri = DocumentUri(uri_str.to_string());

        let changes = vec![FileChange {
            kind: FileChangeKind::Close,
            uri: uri.clone(),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        }];
        self.overlay_fs.process_changes(&changes);

        self.snapshot_id += 1;
        let mut pc = self.snapshot.project_collection().shallow_clone();

        if self.overlay_fs.overlays().is_empty() {
            pc.clear_inferred_project();
        } else {
            let has_inferred_files = self.overlay_fs.overlays().values().any(|o| {
                let file_name = o.file_name();
                self.find_config_for_file(file_name).is_none()
            });
            if !has_inferred_files {
                pc.clear_inferred_project();
            }
        }

        self.snapshot = Snapshot::new(self.snapshot_id, pc);
    }

    /// Handles a `textDocument/didChange` notification from the LSP client.
    ///
    /// Updates the overlay content for the file and rebuilds the snapshot.
    /// In the MVP, whole-document replacement is used.
    ///
    /// Side effects: mutates internal overlay and snapshot state.
    // Go: internal/project/session.go:Session.DidChangeFile
    pub fn did_change_file(&mut self, uri_str: &str, version: i32, new_content: &str) {
        let uri = DocumentUri(uri_str.to_string());

        let changes = vec![FileChange {
            kind: FileChangeKind::Change,
            uri: uri.clone(),
            version,
            content: String::new(),
            language_kind: Default::default(),
            changes: vec![
                tsgo_lsproto::TextDocumentContentChangePartialOrWholeDocument {
                    partial: None,
                    whole_document: Some(tsgo_lsproto::TextDocumentContentChangeWholeDocument {
                        text: new_content.to_string(),
                    }),
                },
            ],
        }];
        self.overlay_fs.process_changes(&changes);

        self.snapshot_id += 1;
        let pc = self.snapshot.project_collection().shallow_clone();
        self.snapshot = Snapshot::new(self.snapshot_id, pc);
    }

    /// Returns project info for the given file URI.
    ///
    /// In the full implementation this returns a `LanguageService`; in the MVP
    /// it returns the project name as a proof of reachability.
    ///
    /// # Errors
    /// Returns an error if no project is found for the URI.
    ///
    /// Side effects: none (pure read).
    // Go: internal/project/session.go:Session.GetLanguageService
    pub fn get_language_service(&self, uri_str: &str) -> Result<String, String> {
        let uri = DocumentUri(uri_str.to_string());
        let path = uri.path(false);
        if let Some(project) = self.snapshot.get_default_project(&path) {
            return Ok(project.name().to_string());
        }
        Err(format!("no project found for URI {}", uri_str))
    }

    /// Ensures a project exists for the given file URI. If no configured
    /// project matches, an inferred project is created.
    ///
    /// Side effects: may mutate the provided `ProjectCollection`.
    // Go: internal/project/projectcollectionbuilder.go:createInferredProject
    fn ensure_project_for_file(&self, uri: &DocumentUri, pc: &mut ProjectCollection) {
        let file_path = uri.path(false);

        if pc.get_default_project(&file_path).is_some() {
            return;
        }

        let file_name = uri.file_name();
        if let Some(config_path) = self.find_config_for_file(&file_name) {
            let project = Project::new_configured_skeleton(
                &config_path.to_string(),
                (self.to_path)(&config_path.to_string()),
            );
            let project_id = project.id().clone();
            pc.set_configured_project(project_id.clone(), project);
            pc.set_file_default_project(file_path, project_id);
        } else {
            if pc.inferred_project().is_none() {
                let inferred = Project::new_inferred_skeleton(&self.options.current_directory);
                pc.set_inferred_project(inferred);
            }
            pc.set_file_default_project(
                file_path,
                Path(crate::project::INFERRED_PROJECT_NAME.to_string()),
            );
        }
    }

    /// Finds the project (tsconfig.json) for a given file path by walking up
    /// parent directories. Returns the config file path if found, None otherwise.
    ///
    /// Side effects: reads from the overlay filesystem.
    // Go: internal/project/configfileregistry.go:findConfigFileNameForFile (simplified)
    pub fn find_project_for_file(&self, file_name: &str) -> Option<String> {
        let mut dir = tsgo_tspath::get_directory_path(file_name);
        loop {
            let candidate = format!(
                "{}{}tsconfig.json",
                dir,
                if dir.ends_with('/') { "" } else { "/" }
            );
            if self.overlay_fs.file_exists(&candidate) {
                return Some(candidate);
            }
            let parent = tsgo_tspath::get_directory_path(&dir);
            if parent == dir || parent.is_empty() {
                break;
            }
            dir = parent;
        }
        None
    }

    /// Callback invoked when a watched config file changes on disk.
    /// Marks the config for reload in the registry and rebuilds the snapshot.
    ///
    /// Side effects: mutates the config registry and snapshot.
    // Go: configFileRegistryBuilder.DidChangeFiles + session.UpdateSnapshot
    pub fn on_config_file_changed(&mut self, config_file_name: &str) {
        let config_path = (self.to_path)(config_file_name);
        let mut pc = self.snapshot.project_collection().shallow_clone();
        pc.config_file_registry_mut().update_config(&config_path);

        self.snapshot_id += 1;
        self.snapshot = Snapshot::new(self.snapshot_id, pc);
    }

    /// Explicitly rebuilds the snapshot (increments snapshot ID).
    /// Used when external state changes require a fresh snapshot.
    ///
    /// Side effects: increments snapshot_id, rebuilds snapshot.
    // Go: session.UpdateSnapshot (simplified)
    pub fn update_snapshot(&mut self) {
        self.snapshot_id += 1;
        let pc = self.snapshot.project_collection().shallow_clone();
        self.snapshot = Snapshot::new(self.snapshot_id, pc);
    }

    /// Internal config search used by ensure_project_for_file.
    fn find_config_for_file(&self, file_name: &str) -> Option<String> {
        self.find_project_for_file(file_name)
    }
}

#[cfg(test)]
#[path = "session_test.rs"]
mod tests;
