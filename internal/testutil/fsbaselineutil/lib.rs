//! `tsgo_testutil_fsbaselineutil` — 1:1 Rust port of Go
//! `internal/testutil/fsbaselineutil`.
//!
//! Helpers for baselining an in-memory file system and diffing it against a
//! previously-captured snapshot, plus sanitization of internal symbol names so
//! baselines are stable across runs.

mod differ;
pub use differ::*;
