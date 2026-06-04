//! `tsgo_project` — 1:1 Rust port of Go `internal/project`.
//!
//! The project system manages TypeScript projects (configured and inferred),
//! their lifecycle, caches, and the bridge between the LSP protocol layer
//! and the compiler/language-service layer.
//!
//! This crate contains:
//! - **Slice 1** (W3): leaf modules — `kind`, `background`, `refcountcache`,
//!   `filechange`, `client`, `parsecache`.
//! - **Slice 2** (W4): FS layer — `overlayfs`, `configfileregistry`.
//! - **Slice 3** (W4): project core — `project`, `compilerhost`,
//!   `projectcollection`.
//! - **Slice 4** (W7): snapshot + session basics — `snapshot`, `session`.

pub mod background;
pub mod client;
pub mod compilerhost;
pub mod configfileregistry;
pub mod filechange;
pub mod kind;
pub mod overlayfs;
pub mod parsecache;
pub mod project;
pub mod projectcollection;
pub mod refcountcache;
pub mod session;
pub mod snapshot;
