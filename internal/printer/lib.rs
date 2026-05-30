//! `tsgo_printer` — 1:1 Rust port of Go `internal/printer`.
//!
//! The printer turns a (possibly transformed) AST back into TypeScript/JavaScript
//! source text. It owns the emit context, the emit-aware node factory, the name
//! generator, the parenthesizer rules, and the family of text writers.
//!
//! # Ownership model (read this first)
//!
//! Go keys its many emit-time side tables on `*ast.Node` pointers and relies on
//! pointer identity for caches. Following `tsgo_ast`, every node reference here
//! is a [`tsgo_ast::NodeId`] index into a [`tsgo_ast::NodeArena`]; the side
//! tables in [`EmitContext`] are keyed on `NodeId`, and caches that depend on
//! Go's "object identity" use `NodeId` equality instead. This is a deliberate,
//! structure-preserving deviation (see PORTING.md §5).

mod emit_declarations;
mod emit_expressions;
mod emit_jsx;
mod emit_statements;
mod emit_types;
pub mod emitcontext;
pub mod emitflags;
pub mod emittextwriter;
pub mod factory;
pub mod generatedidentifierflags;
mod list_format;
mod literal_text;
pub mod namegenerator;
mod parenthesizer;
pub mod printer;
pub mod textwriter;
pub mod utilities;

pub use emitcontext::EmitContext;
pub use emitflags::EmitFlags;
pub use emittextwriter::EmitTextWriter;
pub use factory::NodeFactory;
pub use generatedidentifierflags::GeneratedIdentifierFlags;
pub use list_format::ListFormat;
pub use namegenerator::NameGenerator;
pub use printer::{PrintHandlers, Printer, PrinterOptions, WriteKind};
pub use textwriter::{get_default_indent_size, new_text_writer, TextWriter};

#[cfg(test)]
pub(crate) mod test_support;
