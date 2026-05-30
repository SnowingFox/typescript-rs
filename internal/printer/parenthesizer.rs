//! The parenthesizer is implemented as precedence-driven parenthesization inside
//! the `emit_*` functions (`emit_expression`'s outer parens, `emit_types`'
//! type-precedence parens, and the special cases in `emit_callee`,
//! `emit_new_expression`, `emit_expression_statement`, and `emit_concise_body`).
//!
//! This module hosts the parenthesizer tests, which build *synthetic* ASTs via
//! the node arena (no source text) and assert the emitted parentheses, mirroring
//! Go's `TestParenthesize*`.

#[cfg(test)]
#[path = "parenthesizer_test.rs"]
mod tests;
