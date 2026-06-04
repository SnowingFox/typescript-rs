//! `tsgo_lsp` — 1:1 Rust port of Go `internal/lsp`.
//!
//! The LSP server skeleton: JSON-RPC dispatch loop that handles the LSP
//! lifecycle (`initialize`/`initialized`/`shutdown`/`exit`), text document
//! synchronization (`didOpen`/`didChange`/`didClose`), and forwards language
//! feature requests (hover, definition, completion, ...) to the in-process
//! [`LanguageService`](tsgo_ls::LanguageService) via the project session.
//!
//! This crate is the P8 Slice 5 MVP entry point for the Rust rewrite's LSP
//! integration. It consumes [`tsgo_lsproto`] (protocol types), [`tsgo_jsonrpc`]
//! (base-protocol framing + message types), and delegates feature work to
//! [`tsgo_ls`] through a session-like abstraction.
//!
//! # Divergence from Go
//! - Go uses goroutines + channels for the concurrent read/dispatch/write loops.
//!   This port starts with a synchronous single-threaded dispatch loop suitable
//!   for testing (the async split is deferred to the CLI `--lsp` wiring).
//! - The Go `Server` holds a real `*project.Session`; this port uses a trait
//!   [`Session`] so tests can plug in a stub without the full project system.

pub mod project_session;
mod server;

pub use project_session::ProjectSession;
pub use server::{Server, ServerError, Session};

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
