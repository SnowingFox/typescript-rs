//! The `--build` (`tsc -b`) orchestration: resolve a project-reference graph,
//! compute its topological build order, and build each out-of-date project in
//! turn (skipping the up-to-date ones), reusing the single-project
//! compile/emit path plus the `.tsbuildinfo` machinery.
//!
//! Ports the reachable subset of Go's `internal/execute/build` package
//! (`orchestrator.go` / `buildtask.go` / `uptodatestatus.go`) together with the
//! `internal/execute/tsc.go:tscBuildCompilation` dispatch. The full feature set
//! (watch downstream tracking, parallel builders, pseudo-builds /
//! `UpToDateWithUpstreamTypes`, `--clean`, output-timestamp touches, the
//! incremental skip-emit reuse) is DEFER; see the per-item notes in
//! [`orchestrator`].

mod orchestrator;
pub mod parse_cache;
pub mod uptodatestatus;

pub use orchestrator::perform_build;
pub use parse_cache::ParseCache;
pub use uptodatestatus::{
    InputOutputFileAndTime, InputOutputName, UpToDateStatus, UpToDateStatusData,
    UpToDateStatusKind, UpstreamErrors,
};
