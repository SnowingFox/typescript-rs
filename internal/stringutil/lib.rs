//! `tsgo_stringutil` — character/string primitives needed to parse/print
//! JavaScript.
//!
//! 1:1 port of Go `internal/stringutil` (`util.go` + `compare.go`).

mod compare;
mod util;

pub use compare::*;
pub use util::*;
