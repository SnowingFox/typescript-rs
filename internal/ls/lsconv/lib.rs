//! `tsgo_ls_lsconv` — 1:1 Rust port of Go `internal/ls/lsconv`.
//!
//! The language-service conversion layer that bridges the compiler's internal
//! UTF-8 byte offsets and the LSP protocol's 0-based `(line, UTF-16 character)`
//! positions, plus file-name <-> `DocumentUri` conversion.
//!
//! # Divergence from Go
//! - `Script::text` returns `&[u8]` (not `&str`): Go `string` is a byte
//!   sequence and the position conversions are defined over raw bytes, so a
//!   document may contain invalid UTF-8 (see `TestConvertersInvalidUTF8`).
//!   Rust `&str` cannot represent such input, so text is modeled as bytes and
//!   decoded with a port of Go `utf8.DecodeRuneInString`.
//! - `PositionEncodingKind` is mirrored locally as a temporary shim because
//!   `tsgo_lsproto` has not yet ported it. See `converters.rs`.
//! - The diagnostic-conversion API (`DiagnosticToLSP{Pull,Push}`) is deferred:
//!   it depends on `ast::Diagnostic` (unported) and the still-deferred
//!   `diagnosticwriter::write_flattened_ast_diagnostic_message`.

mod converters;
mod linemap;

pub use converters::{file_name_to_document_uri, Converters, PositionEncodingKind, Script};
pub use linemap::{compute_lsp_line_starts, LSPLineMap, LSPLineStarts};
