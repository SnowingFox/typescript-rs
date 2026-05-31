//! Test helpers for `tsoptions` (a [`ParseConfigHost`] backed by an in-memory
//! file system).
//!
//! 1:1 port of Go `internal/tsoptions/tsoptionstest`. Exposed as `pub mod` (not
//! `#[cfg(test)]`) because Go's external `_test` package consumes it.
//!
//! ## File mapping (PORTING.md §2)
//!
//! | Go file | Rust file |
//! |---|---|
//! | `vfsparseconfighost.go` | [`vfsparseconfighost`] |
//! | `parsedcommandline.go` | [`parsedcommandline`] |
//!
//! [`ParseConfigHost`]: crate::ParseConfigHost

mod parsedcommandline;
mod vfsparseconfighost;

pub use parsedcommandline::*;
pub use vfsparseconfighost::*;
