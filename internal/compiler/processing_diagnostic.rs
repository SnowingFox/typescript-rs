//! Port of Go's `internal/compiler/processingDiagnostic.go`.

use crate::file_include::FileIncludeReason;
use tsgo_diagnostics::Message;
use tsgo_tspath::Path;

// Go: processingDiagnosticKind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingDiagnosticKind {
    UnknownReference,
    ExplainingFileInclude,
}

// Go: includeExplainingDiagnostic
#[derive(Debug)]
pub struct IncludeExplainingDiagnostic {
    pub file: Path,
    pub diagnostic_reason: Option<Box<FileIncludeReason>>,
    pub message: &'static Message,
    pub args: Vec<String>,
}

#[derive(Debug)]
pub enum ProcessingDiagnosticData {
    FileIncludeReason(Box<FileIncludeReason>),
    IncludeExplaining(Box<IncludeExplainingDiagnostic>),
}

// Go: processingDiagnostic
#[derive(Debug)]
pub struct ProcessingDiagnostic {
    pub kind: ProcessingDiagnosticKind,
    pub data: ProcessingDiagnosticData,
}

impl ProcessingDiagnostic {
    pub fn unknown_reference(reason: FileIncludeReason) -> Self {
        Self {
            kind: ProcessingDiagnosticKind::UnknownReference,
            data: ProcessingDiagnosticData::FileIncludeReason(Box::new(reason)),
        }
    }

    pub fn explaining_file_include(explaining: IncludeExplainingDiagnostic) -> Self {
        Self {
            kind: ProcessingDiagnosticKind::ExplainingFileInclude,
            data: ProcessingDiagnosticData::IncludeExplaining(Box::new(explaining)),
        }
    }

    pub fn as_file_include_reason(&self) -> Option<&FileIncludeReason> {
        match &self.data {
            ProcessingDiagnosticData::FileIncludeReason(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_include_explaining(&self) -> Option<&IncludeExplainingDiagnostic> {
        match &self.data {
            ProcessingDiagnosticData::IncludeExplaining(d) => Some(d),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "processing_diagnostic_test.rs"]
mod tests;
