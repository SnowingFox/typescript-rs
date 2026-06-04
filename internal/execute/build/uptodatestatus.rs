//! Port of Go's `internal/execute/build/uptodatestatus.go`.
//!
//! Defines the [`UpToDateStatusKind`] enum for the `--build` orchestrator
//! (all the possible states a project can be in: up-to-date, out-of-date with
//! a specific reason, error, etc.) and the associated status types that carry
//! the relevant data.

use std::time::SystemTime;

/// The status of a project in a `--build` graph.
///
/// The variants mirror Go's `upToDateStatusType` iota exactly, grouped into
/// errors, up-to-date, pseudo-builds, and needs-build categories.
///
/// Side effects: none (plain enum).
// Go: internal/execute/build/uptodatestatus.go:upToDateStatusType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum UpToDateStatusKind {
    // ----- Errors -----
    /// Config file not found.
    ConfigFileNotFound = 0,
    /// Build errors.
    BuildErrors,
    /// Did not build because upstream has errors (with `--stopBuildOnErrors`).
    UpstreamErrors,

    // ----- Up to date -----
    /// Fully up to date.
    UpToDate,

    // ----- Pseudo-builds (touch timestamps only) -----
    /// Appears out-of-date but all outputs are newer than previous .d.ts
    /// inputs; can pseudo-build.
    UpToDateWithUpstreamTypes,
    /// Up to date but input text unchanged; just need timestamp updates.
    UpToDateWithInputFileText,

    // ----- Needs build -----
    /// Input file missing.
    InputFileMissing,
    /// Output file missing.
    OutputMissing,
    /// Input file is newer than output.
    InputFileNewer,
    /// Build info pending emit.
    OutOfDateBuildInfoWithPendingEmit,
    /// Build info indicates errors to report.
    OutOfDateBuildInfoWithErrors,
    /// Build info indicates compiler options changed.
    OutOfDateOptions,
    /// A root file was removed.
    OutOfDateRoots,
    /// Build info version mismatch.
    TsVersionOutOfDate,
    /// Build because `--force`.
    ForceBuild,

    // ----- Solution -----
    /// This is a solution-style project (no files, only references).
    Solution,
}

impl UpToDateStatusKind {
    /// Returns `true` for error states (config not found, build errors,
    /// upstream errors).
    // Go: internal/execute/build/uptodatestatus.go:upToDateStatus.isError
    pub fn is_error(self) -> bool {
        matches!(
            self,
            UpToDateStatusKind::ConfigFileNotFound
                | UpToDateStatusKind::BuildErrors
                | UpToDateStatusKind::UpstreamErrors
        )
    }

    /// Returns `true` for pseudo-build states (upstream-types or input-text
    /// unchanged).
    // Go: internal/execute/build/uptodatestatus.go:upToDateStatus.isPseudoBuild
    pub fn is_pseudo_build(self) -> bool {
        matches!(
            self,
            UpToDateStatusKind::UpToDateWithUpstreamTypes
                | UpToDateStatusKind::UpToDateWithInputFileText
        )
    }
}

/// A pair of input file name and output file name.
// Go: internal/execute/build/uptodatestatus.go:inputOutputName
#[derive(Debug, Clone)]
pub struct InputOutputName {
    pub input: String,
    pub output: String,
}

/// A file name and its last-modified time.
// Go: internal/execute/build/uptodatestatus.go:fileAndTime
#[derive(Debug, Clone)]
pub struct FileAndTime {
    pub file: String,
    pub time: SystemTime,
}

/// The newest input and oldest output file+time pair, plus the build info
/// path.
// Go: internal/execute/build/uptodatestatus.go:inputOutputFileAndTime
#[derive(Debug, Clone)]
pub struct InputOutputFileAndTime {
    pub input: FileAndTime,
    pub output: FileAndTime,
    pub build_info: String,
}

/// Upstream error data: the reference path and whether the upstream itself has
/// upstream errors.
// Go: internal/execute/build/uptodatestatus.go:upstreamErrors
#[derive(Debug, Clone)]
pub struct UpstreamErrors {
    pub ref_path: String,
    pub ref_has_upstream_errors: bool,
}

/// The associated data of an [`UpToDateStatus`], typed by the status kind.
// Go: internal/execute/build/uptodatestatus.go:upToDateStatus.data
#[derive(Debug, Clone)]
pub enum UpToDateStatusData {
    None,
    /// A single string (file name, path, build info path, or version string).
    String(String),
    /// Input/output name pair.
    InputOutput(InputOutputName),
    /// Input/output file-and-time pair.
    InputOutputFileAndTime(Box<InputOutputFileAndTime>),
    /// Upstream errors data.
    UpstreamErrors(UpstreamErrors),
}

/// The up-to-date status of a project in a `--build` graph.
// Go: internal/execute/build/uptodatestatus.go:upToDateStatus
#[derive(Debug, Clone)]
pub struct UpToDateStatus {
    pub kind: UpToDateStatusKind,
    pub data: UpToDateStatusData,
}

impl UpToDateStatus {
    /// Creates a status with no associated data.
    pub fn simple(kind: UpToDateStatusKind) -> Self {
        Self {
            kind,
            data: UpToDateStatusData::None,
        }
    }

    /// Creates a status with a string payload.
    pub fn with_string(kind: UpToDateStatusKind, s: impl Into<String>) -> Self {
        Self {
            kind,
            data: UpToDateStatusData::String(s.into()),
        }
    }

    /// Creates a status with an input/output name pair.
    pub fn with_input_output(kind: UpToDateStatusKind, input: String, output: String) -> Self {
        Self {
            kind,
            data: UpToDateStatusData::InputOutput(InputOutputName { input, output }),
        }
    }

    /// Whether this is an error status.
    pub fn is_error(&self) -> bool {
        self.kind.is_error()
    }

    /// Whether this is a pseudo-build status.
    pub fn is_pseudo_build(&self) -> bool {
        self.kind.is_pseudo_build()
    }

    /// Returns the input/output file-and-time data, if present.
    // Go: internal/execute/build/uptodatestatus.go:upToDateStatus.inputOutputFileAndTime
    pub fn as_input_output_file_and_time(&self) -> Option<&InputOutputFileAndTime> {
        match &self.data {
            UpToDateStatusData::InputOutputFileAndTime(d) => Some(d),
            _ => None,
        }
    }

    /// Returns the input/output name data, if present.
    // Go: internal/execute/build/uptodatestatus.go:upToDateStatus.inputOutputName
    pub fn as_input_output_name(&self) -> Option<&InputOutputName> {
        match &self.data {
            UpToDateStatusData::InputOutput(d) => Some(d),
            _ => None,
        }
    }

    /// Returns the upstream-errors data, if present.
    // Go: internal/execute/build/uptodatestatus.go:upToDateStatus.upstreamErrors
    pub fn as_upstream_errors(&self) -> Option<&UpstreamErrors> {
        match &self.data {
            UpToDateStatusData::UpstreamErrors(d) => Some(d),
            _ => None,
        }
    }

    /// Returns the "oldest output file name" for pseudo-build or up-to-date
    /// statuses.
    // Go: internal/execute/build/uptodatestatus.go:upToDateStatus.oldestOutputFileName
    pub fn oldest_output_file_name(&self) -> Option<&str> {
        if !self.is_pseudo_build() && self.kind != UpToDateStatusKind::UpToDate {
            return None;
        }
        if let Some(iofa) = self.as_input_output_file_and_time() {
            return Some(&iofa.output.file);
        }
        if let Some(io) = self.as_input_output_name() {
            return Some(&io.output);
        }
        match &self.data {
            UpToDateStatusData::String(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_statuses() {
        assert!(UpToDateStatusKind::ConfigFileNotFound.is_error());
        assert!(UpToDateStatusKind::BuildErrors.is_error());
        assert!(UpToDateStatusKind::UpstreamErrors.is_error());
        assert!(!UpToDateStatusKind::UpToDate.is_error());
        assert!(!UpToDateStatusKind::ForceBuild.is_error());
    }

    #[test]
    fn pseudo_build_statuses() {
        assert!(UpToDateStatusKind::UpToDateWithUpstreamTypes.is_pseudo_build());
        assert!(UpToDateStatusKind::UpToDateWithInputFileText.is_pseudo_build());
        assert!(!UpToDateStatusKind::UpToDate.is_pseudo_build());
        assert!(!UpToDateStatusKind::BuildErrors.is_pseudo_build());
    }

    #[test]
    fn simple_status() {
        let s = UpToDateStatus::simple(UpToDateStatusKind::ForceBuild);
        assert_eq!(s.kind, UpToDateStatusKind::ForceBuild);
        assert!(!s.is_error());
        assert!(!s.is_pseudo_build());
    }

    #[test]
    fn status_with_string() {
        let s = UpToDateStatus::with_string(UpToDateStatusKind::OutputMissing, "/out/a.js");
        assert_eq!(s.kind, UpToDateStatusKind::OutputMissing);
        match &s.data {
            UpToDateStatusData::String(v) => assert_eq!(v, "/out/a.js"),
            _ => panic!("expected String data"),
        }
    }

    #[test]
    fn oldest_output_from_string() {
        let s = UpToDateStatus::with_string(UpToDateStatusKind::UpToDate, "/out/a.js");
        assert_eq!(s.oldest_output_file_name(), Some("/out/a.js"));
    }

    #[test]
    fn oldest_output_from_input_output_name() {
        let s = UpToDateStatus::with_input_output(
            UpToDateStatusKind::UpToDate,
            "/src/a.ts".into(),
            "/out/a.js".into(),
        );
        assert_eq!(s.oldest_output_file_name(), Some("/out/a.js"));
    }

    #[test]
    fn oldest_output_none_for_non_uptodate() {
        let s = UpToDateStatus::simple(UpToDateStatusKind::ForceBuild);
        assert!(s.oldest_output_file_name().is_none());
    }

    #[test]
    fn upstream_errors_roundtrip() {
        let s = UpToDateStatus {
            kind: UpToDateStatusKind::UpstreamErrors,
            data: UpToDateStatusData::UpstreamErrors(UpstreamErrors {
                ref_path: "/lib/tsconfig.json".into(),
                ref_has_upstream_errors: true,
            }),
        };
        let ue = s.as_upstream_errors().unwrap();
        assert_eq!(ue.ref_path, "/lib/tsconfig.json");
        assert!(ue.ref_has_upstream_errors);
    }
}
