//! Port of Go's `internal/compiler/includeprocessor.go`.

use std::collections::HashMap;

use crate::file_include::FileIncludeReason;
use crate::processing_diagnostic::{IncludeExplainingDiagnostic, ProcessingDiagnostic};
use tsgo_tspath::Path;

// Go: internal/compiler/includeprocessor.go:includeProcessor
pub struct IncludeProcessor {
    file_include_reasons: HashMap<Path, Vec<FileIncludeReason>>,
    processing_diagnostics: Vec<ProcessingDiagnostic>,
}

impl std::fmt::Debug for IncludeProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IncludeProcessor")
            .field("file_count", &self.file_include_reasons.len())
            .field("diagnostic_count", &self.processing_diagnostics.len())
            .finish()
    }
}

impl Default for IncludeProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl IncludeProcessor {
    pub fn new() -> Self {
        Self {
            file_include_reasons: HashMap::new(),
            processing_diagnostics: Vec::new(),
        }
    }

    pub fn file_include_reasons(&self) -> &HashMap<Path, Vec<FileIncludeReason>> {
        &self.file_include_reasons
    }

    pub fn reasons_for(&self, path: &Path) -> Option<&[FileIncludeReason]> {
        self.file_include_reasons.get(path).map(|v| v.as_slice())
    }

    pub fn add_file_include_reason(&mut self, path: Path, reason: FileIncludeReason) {
        self.file_include_reasons
            .entry(path)
            .or_default()
            .push(reason);
    }

    pub fn add_processing_diagnostic(&mut self, diag: ProcessingDiagnostic) {
        self.processing_diagnostics.push(diag);
    }

    // Go: addProcessingDiagnosticsForFileCasing
    pub fn add_file_casing_diagnostic(
        &mut self,
        file: Path,
        existing_casing: &str,
        current_casing: &str,
        reason: FileIncludeReason,
    ) {
        let has_referenced = !reason.is_referenced_file()
            && self
                .file_include_reasons
                .get(&file)
                .is_some_and(|rs| rs.iter().any(|r| r.is_referenced_file()));

        let (message, args) = if has_referenced {
            (
                &tsgo_diagnostics::ALREADY_INCLUDED_FILE_NAME_0_DIFFERS_FROM_FILE_NAME_1_ONLY_IN_CASING,
                vec![existing_casing.to_string(), current_casing.to_string()],
            )
        } else {
            (
                &tsgo_diagnostics::FILE_NAME_0_DIFFERS_FROM_ALREADY_INCLUDED_FILE_NAME_1_ONLY_IN_CASING,
                vec![current_casing.to_string(), existing_casing.to_string()],
            )
        };

        self.add_processing_diagnostic(ProcessingDiagnostic::explaining_file_include(
            IncludeExplainingDiagnostic {
                file,
                diagnostic_reason: Some(Box::new(reason)),
                message,
                args,
            },
        ));
    }

    pub fn processing_diagnostic_count(&self) -> usize {
        self.processing_diagnostics.len()
    }

    pub fn processing_diagnostics(&self) -> &[ProcessingDiagnostic] {
        &self.processing_diagnostics
    }

    pub fn file_count(&self) -> usize {
        self.file_include_reasons.len()
    }

    // Go: updateFileIncludeProcessor
    pub fn updated_from(other: Self) -> Self {
        Self {
            file_include_reasons: other.file_include_reasons,
            processing_diagnostics: other.processing_diagnostics,
        }
    }
}

#[cfg(test)]
#[path = "includeprocessor_test.rs"]
mod tests;
