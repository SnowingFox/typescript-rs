//! `tsgo_api` — 1:1 Rust port of Go `internal/api`.
//!
//! Compiler-as-a-service RPC over JSON-RPC or a custom MessagePack tuple protocol,
//! on top of `project::Session`. Exposes program/symbol/type/signature/AST queries
//! and optional client-side virtual FS callbacks.
//!
//! # Crate layout (in progress)
//! - [`protocol`]: `Protocol` trait and [`Message`] alias.
//! - [`conn`]: `Conn` / [`Handler`] traits and connection errors.
//! - [`protocol_msgpack`]: MessagePack wire enums (full codec deferred).
//! - [`server`]: [`StdioServer`] entry point (wiring deferred).
//! - [`session`]: API [`Session`] over a project session (handlers deferred).
//!
//! # Divergence from Go
//! - `context.Context` is not modeled yet on trait methods; cancellation wiring
//!   is deferred until `conn_async` / `conn_sync` land.
//! - Handle types that embed Go pointers will use snapshot registries (see
//!   phase-8 `api/impl.md`); not present in this skeleton.

mod conn;
mod protocol;
mod protocol_msgpack;
mod server;
mod session;
#[cfg(unix)]
mod transport_unix;
#[cfg(windows)]
mod transport_windows;

pub use conn::{unmarshal_params, Conn, Handler, ERR_CONN_CLOSED, ERR_REQUEST_TIMEOUT};
pub use protocol::{Message, Protocol};
pub use protocol_msgpack::MessageType;
pub use server::{StdioServer, StdioServerOptions};
pub use session::{Session, SessionOptions};

#[cfg(unix)]
pub use transport_unix::{generate_pipe_path, new_pipe_listener};
#[cfg(windows)]
pub use transport_windows::generate_pipe_path;

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
