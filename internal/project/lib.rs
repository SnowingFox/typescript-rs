//! `tsgo_project` — 1:1 Rust port of Go `internal/project`.
//!
//! The project system manages TypeScript projects (configured and inferred),
//! their lifecycle, caches, and the bridge between the LSP protocol layer
//! and the compiler/language-service layer.
//!
//! This crate currently contains the P8 Slice 1 leaf modules — pure-function
//! and data-structure types with no external state dependencies.

pub mod background;
pub mod client;
pub mod filechange;
pub mod kind;
pub mod parsecache;
pub mod refcountcache;
