//! `tsgo_testutil_jstest` — 1:1 Rust port of Go `internal/testutil/jstest`.
//!
//! Locates and runs a Node.js binary to evaluate small ES-module scripts whose
//! default export is a single (optionally async) function, then deserializes
//! the JSON-stringified return value. Used by tests that cross-check Go/Rust
//! behavior against the reference JavaScript implementation.

mod node;
pub use node::*;
