//! `tsgo_tspath` — TypeScript path model and path utilities (unified `/`
//! separator).
//!
//! 1:1 port of Go `internal/tspath` (`path.go` + `extension.go` +
//! `ignoredpaths.go`).

mod extension;
mod ignoredpaths;
mod path;

pub use extension::*;
pub use ignoredpaths::*;
pub use path::*;
