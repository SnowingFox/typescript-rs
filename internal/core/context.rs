//! Request-scoped context.
//!
//! Go `internal/core/context.go` stashes a request id inside a
//! `context.Context`. Per the port's "no implicit context" rule (PORTING.md
//! §3), we model it as an explicit lightweight value passed by callers instead
//! of relying on ambient/thread-local context.

/// A request-scoped context carrying an optional request id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestContext {
    request_id: Option<String>,
}

impl RequestContext {
    /// Returns a copy of this context with `id` set as the request id.
    ///
    /// # Examples
    /// ```
    /// use tsgo_core::context::RequestContext;
    /// let ctx = RequestContext::default().with_request_id("abc");
    /// assert_eq!(ctx.request_id(), "abc");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/core/context.go:WithRequestID
    pub fn with_request_id(&self, id: impl Into<String>) -> RequestContext {
        RequestContext {
            request_id: Some(id.into()),
        }
    }

    /// Returns the request id, or `""` when unset (mirrors Go `GetRequestID`).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/context.go:GetRequestID
    pub fn request_id(&self) -> &str {
        self.request_id.as_deref().unwrap_or("")
    }
}

#[cfg(test)]
#[path = "context_test.rs"]
mod tests;
