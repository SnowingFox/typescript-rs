//! Incremental compilation: `.tsbuildinfo` emit/read, program-state diff
//! (file versions/signatures), affected-files recompute, and up-to-date
//! checking.
//!
//! This crate is a 1:1 Rust port of Go's `internal/execute/incremental/`
//! package (the `.tsbuildinfo` machinery that backs `tsc --incremental`,
//! `--composite`, and `tsc -b`). It depends on [`tsgo_compiler`] just like the
//! Go package depends on `internal/compiler`.
//!
//! Reachable subset (P6-9b): `BuildInfo` serde (de)serialization matching Go's
//! compact `.tsbuildinfo` JSON (1-based file ids), the file-version/signature
//! hash ([`compute_hash`](snapshot::compute_hash)), the affected-files
//! transitive walk over the `referencedMap`, the up-to-date predicate, and the
//! [`Program`](program::Program) integration that emits a `.tsbuildinfo` after a
//! build and reads it back to seed the next build's reuse.
//!
//! Layout note: the upstream Go package lives at
//! `internal/execute/incremental/` (not `internal/incremental/` as some briefs
//! assume), so these `.rs` files sit side-by-side with the Go source per the
//! porting convention.

mod affectedfileshandler;
mod build_info;
mod buildinfotosnapshot;
mod program;
mod programtosnapshot;
mod reference_map;
mod snapshot;
mod snapshottobuildinfo;
mod uptodatestatus;

pub use affectedfileshandler::{collect_all_affected_files, get_files_affected_by};
pub use buildinfotosnapshot::build_info_to_snapshot;
pub use program::{
    read_build_info, read_build_info_program, BuildInfoReader, EmitBuildInfoResult,
    HostBuildInfoReader, Program,
};
pub use programtosnapshot::program_to_snapshot;
pub use snapshottobuildinfo::snapshot_to_build_info;
pub use uptodatestatus::{get_up_to_date_status, InputFile, UpToDateStatusType};

pub use build_info::{
    BuildInfo, BuildInfoFileId, BuildInfoFileIdListId, BuildInfoFileInfo,
    BuildInfoReferenceMapEntry, BuildInfoRoot, BuildInfoSemanticDiagnostic,
};
pub use reference_map::ReferenceMap;
pub use snapshot::{
    compute_file_version, compute_hash, compute_signature, FileInfo, RESOLUTION_MODE_COMMON_JS,
};
