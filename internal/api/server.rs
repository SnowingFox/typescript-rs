//! STDIO / pipe API server entry point (`server.go` — skeleton).

use std::path::PathBuf;

use thiserror::Error;

use crate::session::{Session, SessionOptions};

/// Configures the STDIO-based API server.
///
/// Side effects: none (pure data).
// Go: internal/api/server.go:StdioServerOptions
#[derive(Debug, Clone, Default)]
pub struct StdioServerOptions {
    /// Working directory (required).
    // Go: internal/api/server.go:StdioServerOptions.Cwd
    pub cwd: String,
    /// Override for default lib path.
    // Go: internal/api/server.go:StdioServerOptions.DefaultLibraryPath
    pub default_library_path: String,
    /// Named pipe (Windows) or Unix socket path instead of stdin/stdout.
    // Go: internal/api/server.go:StdioServerOptions.PipePath
    pub pipe_path: Option<PathBuf>,
    /// FS operations delegated to the client (`readFile`, `fileExists`, …).
    // Go: internal/api/server.go:StdioServerOptions.Callbacks
    pub callbacks: Vec<String>,
    /// When true, use JSON-RPC + async conn; otherwise MessagePack + sync conn.
    // Go: internal/api/server.go:StdioServerOptions.Async
    pub async_mode: bool,
}

/// STDIO / pipe API server (wiring deferred).
///
/// Side effects: `run` will start I/O once transport and project are wired.
// Go: internal/api/server.go:StdioServer
pub struct StdioServer {
    options: StdioServerOptions,
}

/// Server startup failures.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Missing required option (Go panics on empty `Cwd`).
    #[error("StdioServerOptions.Cwd is required")]
    CwdRequired,
    /// Not yet ported.
    #[error("{0}")]
    NotImplemented(&'static str),
}

impl StdioServer {
    /// Creates a new server; panics if `cwd` is empty (matches Go).
    ///
    /// # Panics
    /// If `options.cwd` is empty.
    ///
    /// # Examples
    /// ```
    /// let server = tsgo_api::StdioServer::new(tsgo_api::StdioServerOptions {
    ///     cwd: "/tmp".into(),
    ///     ..Default::default()
    /// });
    /// let _ = server;
    /// ```
    ///
    /// Side effects: none.
    // Go: internal/api/server.go:NewStdioServer
    pub fn new(options: StdioServerOptions) -> Self {
        if options.cwd.is_empty() {
            panic!("StdioServerOptions.Cwd is required");
        }
        Self { options }
    }

    /// Starts the server and blocks until the connection closes.
    ///
    /// Side effects: deferred — will accept transport, run `Conn`, and use `project`.
    // Go: internal/api/server.go:StdioServer.Run
    pub fn run(&self) -> Result<(), ServerError> {
        let _session = Session::new(SessionOptions {
            use_binary_responses: !self.options.async_mode,
        });
        Err(ServerError::NotImplemented(
            "StdioServer::run — DEFER(phase-8): blocked-by transport, conn, project",
        ))
    }
}
