//! Up-to-date checking: decide whether a project's outputs are current with
//! respect to its inputs and its `.tsbuildinfo`.
//!
//! Reachable subset of Go `internal/execute/build/uptodatestatus.go` +
//! `buildtask.go:getUpToDateStatus`. The full `--build` orchestration (output
//! timestamps, upstream project references, pseudo-builds, watch) is DEFER to
//! P9 (`tsgo_execute`); this models the buildinfo-presence + input-newer-than-
//! buildinfo decision that backs it.

use std::time::SystemTime;

use crate::build_info::BuildInfo;
use crate::snapshot::compute_hash;

/// The reachable subset of Go's `upToDateStatusType` outcomes.
// Go: internal/execute/build/uptodatestatus.go:upToDateStatusType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpToDateStatusType {
    /// Everything is current; no work to do.
    // Go: upToDateStatusTypeUpToDate
    UpToDate,
    /// An input is newer than the buildinfo, but its text is unchanged, so only
    /// timestamps would need touching (still effectively up to date).
    // Go: upToDateStatusTypeUpToDateWithInputFileText
    UpToDateWithInputFileText,
    /// The `.tsbuildinfo` (or another output) is missing.
    // Go: upToDateStatusTypeOutputMissing
    OutputMissing,
    /// An input file is missing.
    // Go: upToDateStatusTypeInputFileMissing
    InputFileMissing,
    /// An input file is newer than the buildinfo and its text changed.
    // Go: upToDateStatusTypeInputFileNewer
    InputFileNewer,
    /// The buildinfo was written by a different compiler version.
    // Go: upToDateStatusTypeTsVersionOutputOfDate
    TsVersionOutputOfDate,
}

impl UpToDateStatusType {
    /// Whether this status means no real rebuild is needed (a true up-to-date or
    /// an input-text-unchanged pseudo-build).
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/build/uptodatestatus.go:upToDateStatus.isPseudoBuild (+ UpToDate)
    pub fn is_up_to_date(self) -> bool {
        matches!(self, Self::UpToDate | Self::UpToDateWithInputFileText)
    }
}

/// An input (root) file with the on-disk modification time and current text the
/// up-to-date check needs.
// Go: internal/execute/build/buildtask.go:getUpToDateStatus (per-input loop)
#[derive(Debug, Clone)]
pub struct InputFile {
    /// The input file name as it appears in the buildinfo's `fileNames`.
    pub file_name: String,
    /// The file's on-disk mtime, or `None` if the file is missing.
    pub mtime: Option<SystemTime>,
    /// The file's current text, or `None` if it could not be read.
    pub current_text: Option<String>,
}

/// Decides the up-to-date status of a project from its inputs, its parsed
/// `.tsbuildinfo` (`None` when missing), and the buildinfo's own mtime.
///
/// Reachable subset of Go `getUpToDateStatus`:
/// - no buildinfo -> [`UpToDateStatusType::OutputMissing`] (out of date);
/// - buildinfo from another compiler version -> [`UpToDateStatusType::TsVersionOutputOfDate`];
/// - a missing input -> [`UpToDateStatusType::InputFileMissing`];
/// - an input newer than the buildinfo whose text hash differs from the stored
///   `version` -> [`UpToDateStatusType::InputFileNewer`] (out of date); if the
///   text hash is unchanged -> [`UpToDateStatusType::UpToDateWithInputFileText`];
/// - otherwise [`UpToDateStatusType::UpToDate`].
///
/// Side effects: none (pure; mtimes/text are passed in by the caller).
// Go: internal/execute/build/buildtask.go:getUpToDateStatus
pub fn get_up_to_date_status(
    inputs: &[InputFile],
    build_info: Option<&BuildInfo>,
    build_info_time: SystemTime,
) -> UpToDateStatusType {
    // Check the build info: missing -> output missing (needs build).
    let Some(build_info) = build_info else {
        return UpToDateStatusType::OutputMissing;
    };
    // Build info version must match the current compiler version.
    if !build_info.is_valid_version() {
        return UpToDateStatusType::TsVersionOutputOfDate;
    }

    // The oldest output starts as the buildinfo itself; an input newer than it
    // is suspect unless its text hash still matches the stored version.
    let mut input_text_unchanged = false;
    for input in inputs {
        let Some(input_time) = input.mtime else {
            return UpToDateStatusType::InputFileMissing;
        };
        if input_time > build_info_time {
            let stored_version = build_info.version_of(&input.file_name);
            let current_version = input
                .current_text
                .as_deref()
                .map(|t| compute_hash(t, false));
            match (stored_version, current_version) {
                (Some(stored), Some(current)) if stored == current => {
                    // Newer mtime but identical text: only timestamps differ.
                    input_text_unchanged = true;
                }
                _ => return UpToDateStatusType::InputFileNewer,
            }
        }
    }

    if input_text_unchanged {
        UpToDateStatusType::UpToDateWithInputFileText
    } else {
        UpToDateStatusType::UpToDate
    }
}

#[cfg(test)]
#[path = "uptodatestatus_test.rs"]
mod tests;
