//! API session over `project::Session` (`session.go` — skeleton only).

/// Options for [`Session`].
///
/// Side effects: none (pure data).
// Go: internal/api/session.go:SessionOptions
#[derive(Debug, Clone, Default)]
pub struct SessionOptions {
    /// When true, responses use the binary AST encoder (msgpack path).
    // Go: internal/api/session.go:SessionOptions.UseBinaryResponses
    pub use_binary_responses: bool,
}

/// Per-connection API handler and snapshot handle registry (stub).
///
/// Side effects: defined once handlers are ported.
// Go: internal/api/session.go:Session
pub struct Session {
    options: SessionOptions,
}

impl Session {
    /// Creates a new API session (project wiring deferred).
    ///
    /// # Examples
    /// ```
    /// let session = tsgo_api::Session::new(tsgo_api::SessionOptions::default());
    /// ```
    ///
    /// Side effects: none until project integration lands.
    // Go: internal/api/session.go:NewSession
    pub fn new(options: SessionOptions) -> Self {
        Self { options }
    }

    /// Releases session resources.
    ///
    /// Side effects: none in skeleton.
    // Go: internal/api/session.go:Session.Close
    pub fn close(self) {}

    /// Whether binary AST responses are enabled.
    ///
    /// Side effects: none (pure).
    pub fn use_binary_responses(&self) -> bool {
        self.options.use_binary_responses
    }
}
