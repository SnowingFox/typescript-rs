//! `tsgo`: the command-line entry point (ports Go's `cmd/tsgo/main.go`).
//!
//! Thin argv dispatcher that routes to the `tsgo_execute` `tsc` path (and, later,
//! the LSP server / API modes). Skeleton registered ahead of the P9 cmd round;
//! the dispatch is filled in by that round.

fn main() {
    std::process::exit(0);
}
