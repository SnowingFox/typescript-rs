//! `tsgo_collections` — ordered/unordered maps and sets, copy-on-write
//! containers, multimaps, and concurrent containers.
//!
//! 1:1 port of Go `internal/collections`.

mod cow;
mod multimap;
mod ordered_map;
mod ordered_set;
mod set;
mod syncmap;
mod syncset;

pub use cow::*;
pub use multimap::*;
pub use ordered_map::*;
pub use ordered_set::*;
pub use set::*;
pub use syncmap::*;
pub use syncset::*;
