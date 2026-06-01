//! The incremental [`Program`]: builds a snapshot from a compiler program, emits
//! the `.tsbuildinfo`, and reads one back to seed the next build's reuse.
//!
//! Reachable subset of Go `internal/execute/incremental/program.go` +
//! `incremental.go`. The affected-files recompute is driven from the snapshot's
//! referenced map ([`crate::get_files_affected_by`]); the full
//! `collectSemanticDiagnosticsOfAffectedFiles` / `emitBuildInfo` pending-emit
//! orchestration (and `--noEmit`/`HandleNoEmitOnError` gating) is DEFER (P9).

use tsgo_compiler::{CompilerHost, Program as CompilerProgram};
use tsgo_outputpaths::get_build_info_file_name;
use tsgo_tspath::ComparePathsOptions;

use crate::build_info::BuildInfo;
use crate::buildinfotosnapshot::build_info_to_snapshot;
use crate::programtosnapshot::program_to_snapshot;
use crate::snapshot::Snapshot;
use crate::snapshottobuildinfo::snapshot_to_build_info;

/// An incremental program: a freshly-built compiler program plus the
/// [`Snapshot`] of its state (file versions/signatures + import graph) that
/// gets serialized to `.tsbuildinfo`.
// Go: internal/execute/incremental/program.go:Program
pub struct Program<'a> {
    snapshot: Snapshot,
    program: &'a CompilerProgram,
}

impl<'a> Program<'a> {
    /// Builds the incremental program for a freshly-built compiler program
    /// (the `oldProgram == nil` path; old-state reuse is DEFER).
    ///
    /// Side effects: none (reads the already-parsed program).
    // Go: internal/execute/incremental/program.go:NewProgram
    pub fn new(program: &'a CompilerProgram) -> Self {
        Self {
            snapshot: program_to_snapshot(program),
            program,
        }
    }

    /// The state snapshot (file infos + referenced map).
    ///
    /// Side effects: none (pure).
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The `.tsbuildinfo` output path for this program, or `""` when the program
    /// is not incremental (no `incremental`/`composite`/`tsBuildInfoFile`).
    ///
    /// Side effects: none (pure).
    // Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName
    pub fn build_info_file_name(&self) -> String {
        get_build_info_file_name(self.program.options(), &self.compare_paths_options())
    }

    /// Builds the serializable [`BuildInfo`] for this program, or `None` when it
    /// is not incremental.
    ///
    /// Side effects: none (pure transform).
    // Go: internal/execute/incremental/snapshottobuildinfo.go:snapshotToBuildInfo
    pub fn build_info(&self) -> Option<BuildInfo> {
        let file_name = self.build_info_file_name();
        if file_name.is_empty() {
            return None;
        }
        Some(snapshot_to_build_info(
            &self.snapshot,
            self.program,
            &file_name,
        ))
    }

    /// Emits the `.tsbuildinfo` to the host file system (when incremental),
    /// returning the path written and its JSON text.
    ///
    /// Side effects: writes the `.tsbuildinfo` file via the host's file system.
    /// No other outputs are written.
    // Go: internal/execute/incremental/program.go:emitBuildInfo
    pub fn emit_build_info(&self) -> Option<EmitBuildInfoResult> {
        let file_name = self.build_info_file_name();
        if file_name.is_empty() {
            return None;
        }
        let build_info = snapshot_to_build_info(&self.snapshot, self.program, &file_name);
        let text = serde_json::to_string(&build_info).expect("BuildInfo always serializes");
        self.program
            .host()
            .fs()
            .write_file(&file_name, &text)
            .ok()?;
        Some(EmitBuildInfoResult { file_name, text })
    }

    fn compare_paths_options(&self) -> ComparePathsOptions {
        ComparePathsOptions {
            current_directory: self.program.host().get_current_directory().to_string(),
            use_case_sensitive_file_names: self.program.host().fs().use_case_sensitive_file_names(),
        }
    }
}

/// The result of emitting a `.tsbuildinfo`: the path written and its JSON text.
// Go: internal/compiler/program.go:EmitResult (EmittedFiles for the buildinfo)
#[derive(Debug, Clone, PartialEq)]
pub struct EmitBuildInfoResult {
    /// The path the build info was written to.
    pub file_name: String,
    /// The serialized JSON written.
    pub text: String,
}

/// Reads and parses a `.tsbuildinfo` file via a host.
// Go: internal/execute/incremental/incremental.go:BuildInfoReader
pub trait BuildInfoReader {
    /// Reads the build info at `build_info_file_name`, or `None` if it cannot be
    /// read/parsed.
    fn read_build_info(&self, build_info_file_name: &str) -> Option<BuildInfo>;
}

/// A [`BuildInfoReader`] backed by a [`CompilerHost`]'s file system.
// Go: internal/execute/incremental/incremental.go:buildInfoReader
pub struct HostBuildInfoReader<'a> {
    host: &'a dyn CompilerHost,
}

impl<'a> HostBuildInfoReader<'a> {
    /// Creates a host-backed build info reader.
    // Go: internal/execute/incremental/incremental.go:NewBuildInfoReader
    pub fn new(host: &'a dyn CompilerHost) -> Self {
        Self { host }
    }
}

impl BuildInfoReader for HostBuildInfoReader<'_> {
    fn read_build_info(&self, build_info_file_name: &str) -> Option<BuildInfo> {
        read_build_info(self.host, build_info_file_name)
    }
}

/// Reads and parses the `.tsbuildinfo` at `build_info_file_name` via `host`'s
/// file system, returning `None` if the path is empty, unreadable, or invalid
/// JSON.
///
/// Side effects: reads the host file system.
// Go: internal/execute/incremental/incremental.go:buildInfoReader.ReadBuildInfo
pub fn read_build_info(host: &dyn CompilerHost, build_info_file_name: &str) -> Option<BuildInfo> {
    if build_info_file_name.is_empty() {
        return None;
    }
    let data = host.fs().read_file(build_info_file_name)?;
    serde_json::from_str::<BuildInfo>(&data).ok()
}

/// Reads a `.tsbuildinfo` back into a [`Snapshot`] that seeds the next build's
/// reuse, or `None` if it is missing, from a different compiler version, or not
/// an incremental program.
///
/// Side effects: reads the host file system.
// Go: internal/execute/incremental/incremental.go:ReadBuildInfoProgram
pub fn read_build_info_program(
    host: &dyn CompilerHost,
    build_info_file_name: &str,
) -> Option<Snapshot> {
    let build_info = read_build_info(host, build_info_file_name)?;
    if !build_info.is_valid_version() || !build_info.is_incremental() {
        return None;
    }
    Some(build_info_to_snapshot(
        &build_info,
        build_info_file_name,
        host.get_current_directory(),
        host.fs().use_case_sensitive_file_names(),
        host.default_library_path(),
    ))
}

#[cfg(test)]
#[path = "program_test.rs"]
mod tests;
