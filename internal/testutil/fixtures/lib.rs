//! `tsgo_testutil_fixtures` — 1:1 Rust port of Go
//! `internal/testutil/fixtures`.
//!
//! Provides the canonical set of benchmark fixtures (`bench_fixtures`) used by
//! the compiler's micro-benchmarks: a small empty file plus a few real, large
//! TypeScript sources read from the vendored TypeScript submodule.

mod benchfixtures;
pub use benchfixtures::*;
