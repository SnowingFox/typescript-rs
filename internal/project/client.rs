//! Client trait — the interface between the LSP transport and the project system.
//!
//! 1:1 port of Go `internal/project/client.go`.
//!
//! The [`Client`] trait defines the callbacks that the project system invokes
//! on the LSP connection: file watching, diagnostics publishing, progress
//! reporting, and telemetry.
//!
//! # DEFER notes
//! Several method signatures reference LSP protocol types that are not yet
//! generated in `tsgo_lsproto` (e.g. `FileSystemWatcher`,
//! `PublishDiagnosticsParams`, `TelemetryEvent`). These are represented as
//! placeholder type aliases in this module and will be replaced once the
//! lsproto generator covers them.

use tsgo_diagnostics::Message;

/// Opaque watcher identifier used to register/unregister file watches.
///
/// # Examples
/// ```
/// use tsgo_project::client::WatcherID;
/// let id = WatcherID("w-1".to_string());
/// assert_eq!(id.0, "w-1");
/// ```
// Go: internal/project/watch.go:WatcherID
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WatcherID(pub String);

// DEFER(phase-7): Replace these placeholders with tsgo_lsproto generated types
// once the lsproto generator covers them.

/// Placeholder for `lsproto.FileSystemWatcher` (not yet generated).
pub type FileSystemWatcher = serde_json::Value;

/// Placeholder for `lsproto.PublishDiagnosticsParams` (not yet generated).
pub type PublishDiagnosticsParams = serde_json::Value;

/// Placeholder for `lsproto.TelemetryEvent` (not yet generated).
pub type TelemetryEvent = serde_json::Value;

/// The interface between the project system and the LSP transport.
///
/// Implementations push information to the client (e.g. publish diagnostics,
/// register file watchers) and report lifecycle events.
///
/// # Examples
/// ```
/// use tsgo_project::client::{Client, NoopClient};
/// let client = NoopClient;
/// assert!(client.is_active());
/// ```
// Go: internal/project/client.go:Client
pub trait Client: Send + Sync {
    /// Registers file watchers for the given patterns.
    // Go: internal/project/client.go:Client.WatchFiles
    fn watch_files(
        &self,
        id: WatcherID,
        watchers: Vec<FileSystemWatcher>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Unregisters a previously registered file watcher.
    // Go: internal/project/client.go:Client.UnwatchFiles
    fn unwatch_files(&self, id: WatcherID) -> Result<(), Box<dyn std::error::Error>>;

    /// Requests the client to refresh all diagnostics.
    // Go: internal/project/client.go:Client.RefreshDiagnostics
    fn refresh_diagnostics(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Publishes diagnostics for a specific document.
    // Go: internal/project/client.go:Client.PublishDiagnostics
    fn publish_diagnostics(
        &self,
        params: PublishDiagnosticsParams,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Requests the client to refresh inlay hints.
    // Go: internal/project/client.go:Client.RefreshInlayHints
    fn refresh_inlay_hints(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Requests the client to refresh code lenses.
    // Go: internal/project/client.go:Client.RefreshCodeLens
    fn refresh_code_lens(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Signals the start of a progress operation.
    // Go: internal/project/client.go:Client.ProgressStart
    fn progress_start(&self, message: &Message, args: &[&str]);

    /// Signals the end of a progress operation.
    // Go: internal/project/client.go:Client.ProgressFinish
    fn progress_finish(&self, message: &Message, args: &[&str]);

    /// Sends a telemetry event to the client.
    // Go: internal/project/client.go:Client.SendTelemetry
    fn send_telemetry(&self, telemetry: TelemetryEvent) -> Result<(), Box<dyn std::error::Error>>;

    /// Reports whether the client connection is still active.
    // Go: internal/project/client.go:Client.IsActive
    fn is_active(&self) -> bool;
}

/// A no-op [`Client`] implementation useful for testing.
// Go: internal/project/extendedconfigcache_test.go:noopClient
pub struct NoopClient;

impl Client for NoopClient {
    fn watch_files(
        &self,
        _id: WatcherID,
        _watchers: Vec<FileSystemWatcher>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn unwatch_files(&self, _id: WatcherID) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn refresh_diagnostics(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn publish_diagnostics(
        &self,
        _params: PublishDiagnosticsParams,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn refresh_inlay_hints(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn refresh_code_lens(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn progress_start(&self, _message: &Message, _args: &[&str]) {}

    fn progress_finish(&self, _message: &Message, _args: &[&str]) {}

    fn send_telemetry(&self, _telemetry: TelemetryEvent) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn is_active(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "client_test.rs"]
mod tests;
