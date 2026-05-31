//! `tsgo_lsproto` — 1:1 Rust port of Go `internal/lsp/lsproto`.
//!
//! The LSP protocol type model: the request/response/notification parameter
//! and result types, the `IntegerOrString`/`IntegerOrNull` style unions, the
//! enum-as-integer kinds, the string-literal discriminator types, and the
//! `DocumentUri` newtype. (De)serialization is implemented with hand-written
//! `serde` impls so that Go's exact null/missing/unknown-field and union
//! dispatch behavior is reproduced 1:1 (rather than pulling in `lsp-types`).
//!
//! # Divergence from Go
//! - Go decodes with the `go-json-experiment/json` streaming decoder and its
//!   generated `UnmarshalJSONFrom` methods. This port uses `serde`/`serde_json`
//!   with custom `Deserialize`/`Serialize` impls that mirror the same control
//!   flow (peek-kind union dispatch, `errNull`/`errMissing` text, omit-zero).
//! - The full generated type set (`lsp_generated.go`, ~38k lines) is ported
//!   incrementally: this crate currently covers the behavior exercised by the
//!   Go `*_test.go` files plus the shared union/enum/literal patterns. The
//!   remaining types and the `Method`-based param/result dispatch are deferred
//!   to a generator pass (see `_generate/generate.mts`).

mod baseproto;
mod generated;
mod lsp;
mod resolved;
mod util;

pub use baseproto::{BaseReader, BaseWriter};
pub use generated::*;
pub use lsp::*;
pub use resolved::*;
pub use util::{compare_positions, compare_ranges};
