use super::*;

// Tests live in per-module test files; this file covers top-level smoke tests.

/// Verify the crate's public re-exports compile.
// Go: internal/lsp/server.go (compile check)
#[test]
fn crate_public_surface_compiles() {
    let _: fn() -> ServerError = || ServerError::Shutdown;
}
