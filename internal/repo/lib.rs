//! `tsgo_repo` — locates the repository root plus the TypeScript submodule and
//! `testdata` directories used by tests.
//!
//! 1:1 port of Go `internal/repo`. This crate is test infrastructure only: it
//! is not part of the compiler runtime.

mod paths;
pub use paths::*;
