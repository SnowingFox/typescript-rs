//! `tsgo_fourslash` — Rust port of Go `internal/fourslash` (the language-service
//! test harness), **foundation round**.
//!
//! A fourslash test case is a TypeScript source file annotated with markup
//! (`/*marker*/`, `[|range|]`, `{| object |}`) plus a sequence of assertions
//! against the language service. This crate ports the *foundation*: the markup
//! parser ([`test_parser`]) and the test-driver skeleton ([`FourslashTest`]),
//! with a first verify command ([`FourslashTest::verify_quick_info_at`]).
//!
//! # Divergence from Go (foundation round)
//!
//! Go's `FourslashTest` drives a full in-memory **LSP server** over channels
//! (`lsptestutil.NewLSPClient` + `internal/lsp` + `internal/project`). Those
//! crates are P8 and not yet ported, so this foundation drives the in-process
//! [`tsgo_ls::LanguageService`] directly (the same way the `tsgo_ls` feature
//! tests build a service over an in-memory program). The markup grammar and the
//! navigation/verify semantics match Go; the LSP transport, the project layer,
//! and the baseline machinery are deferred (see the crate worklog).

mod driver;
mod test_parser;

pub use driver::{new_fourslash, try_new_fourslash, FourslashTest};
pub use test_parser::{
    parse_test_data, FourslashError, Marker, MarkerOrRange, RangeMarker, TestData, TestFileInfo,
};
