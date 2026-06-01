//! `tsgo_execute`: the `tsc` program-build orchestration and CLI driver.
//!
//! Ports Go's `internal/execute` package (the `tsc.go` build/emit orchestration,
//! `--build` mode, watch loop, and the `cmd/tsgo` entry's command execution).
//! Skeleton crate registered ahead of P9; the build orchestration is filled in by
//! the P9 rounds.
