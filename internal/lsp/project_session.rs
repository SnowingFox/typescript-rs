//! Adapter bridging `tsgo_project::session::Session` to the LSP
//! [`server::Session`](crate::server::Session) trait.
//!
//! The LSP server's `Session` trait uses `&self` for all methods (because the
//! `Server` may invoke handlers from a shared reference). The project system's
//! `Session` requires `&mut self` for state-mutating operations (`did_open_file`,
//! `did_close_file`, `did_change_file`). This adapter bridges the gap with
//! `RefCell`-based interior mutability.
//!
//! # Divergence from Go
//! - Go holds a `*project.Session` as a plain field on `Server`, calling methods
//!   via pointer receiver. Rust ownership rules require the interior-mutability
//!   wrapper.
//!
//! Side effects: construction is pure; `Session` trait methods delegate to the
//! inner `tsgo_project::session::Session` and may mutate its state.
// Go: internal/lsp/server.go (session field + handler delegation)

use std::cell::RefCell;

use tsgo_lsproto::LanguageKind;

use crate::server::Session;

/// Adapter wrapping a real [`tsgo_project::session::Session`] to implement
/// the LSP [`Session`] trait.
///
/// Side effects: none on construction; trait method calls mutate the inner
/// project session state.
// Go: internal/lsp/server.go:Server.session (*project.Session usage)
pub struct ProjectSession {
    inner: RefCell<tsgo_project::session::Session>,
}

impl ProjectSession {
    /// Creates a new adapter from a project session.
    ///
    /// Side effects: none (pure construction).
    pub fn new(session: tsgo_project::session::Session) -> Self {
        Self {
            inner: RefCell::new(session),
        }
    }

    /// Returns whether the overlay filesystem tracks the given URI.
    ///
    /// Useful for test assertions.
    ///
    /// Side effects: none (pure read via borrow).
    pub fn overlay_has_file(&self, uri: &str) -> bool {
        self.inner.borrow().overlay_has_file(uri)
    }

    /// Returns the current snapshot ID from the inner session.
    ///
    /// Useful for test assertions to verify state transitions.
    ///
    /// Side effects: none (pure read via borrow).
    pub fn snapshot_id(&self) -> u64 {
        self.inner.borrow().snapshot().id()
    }
}

impl Session for ProjectSession {
    // Go: internal/lsp/server.go:Server.handleDidOpen → session.DidOpenFile
    fn did_open_file(&self, uri: &str, version: i32, text: &str) {
        self.inner
            .borrow_mut()
            .did_open_file(uri, version, text, LanguageKind::TYPE_SCRIPT);
    }

    // Go: internal/lsp/server.go:Server.handleDidChange → session.DidChangeFile
    fn did_change_file(&self, uri: &str, version: i32, changes: &serde_json::Value) {
        let text = changes
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("");
        self.inner.borrow_mut().did_change_file(uri, version, text);
    }

    // Go: internal/lsp/server.go:Server.handleDidClose → session.DidCloseFile
    fn did_close_file(&self, uri: &str) {
        self.inner.borrow_mut().did_close_file(uri);
    }

    // Go: internal/lsp/server.go:handlers() → "textDocument/hover"
    fn hover(&self, uri: &str, _position: &serde_json::Value) -> Option<serde_json::Value> {
        let session = self.inner.borrow();
        match session.get_language_service(uri) {
            Ok(project_name) => Some(serde_json::json!({
                "contents": {
                    "kind": "plaintext",
                    "value": format!("Hover from project: {}", project_name)
                }
            })),
            Err(_) => None,
        }
    }

    // Go: internal/lsp/server.go:handlers() → "textDocument/completion"
    fn completion(&self, uri: &str, _position: &serde_json::Value) -> Option<serde_json::Value> {
        let session = self.inner.borrow();
        match session.get_language_service(uri) {
            Ok(_) => Some(serde_json::json!({
                "isIncomplete": false,
                "items": []
            })),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "project_session_test.rs"]
mod tests;
