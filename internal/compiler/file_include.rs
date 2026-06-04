//! Port of Go's `internal/compiler/fileInclude.go`.
//!
//! Defines [`FileIncludeKind`] and [`FileIncludeReason`] — the types that
//! record *why* a file was included in the program. Threaded through the
//! file-loading pipeline, later used for "why was this file included?"
//! diagnostics.

use std::sync::Once;

use tsgo_tspath::Path;

/// Why a file was included in a program.
// Go: internal/compiler/fileInclude.go:fileIncludeKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileIncludeKind {
    Import,
    ReferenceFile,
    TypeReferenceDirective,
    LibReferenceDirective,
    RootFile,
    LibFile,
    AutomaticTypeDirectiveFile,
}

impl FileIncludeKind {
    // Go: FileIncludeReason.isReferencedFile
    pub fn is_referenced_file(self) -> bool {
        matches!(
            self,
            FileIncludeKind::Import
                | FileIncludeKind::ReferenceFile
                | FileIncludeKind::TypeReferenceDirective
                | FileIncludeKind::LibReferenceDirective
        )
    }
}

// Go: internal/compiler/fileInclude.go:referencedFileData
#[derive(Debug, Clone)]
pub struct ReferencedFileData {
    pub file: Path,
    pub index: usize,
    pub synthetic_text: Option<String>,
}

// Go: internal/compiler/fileInclude.go:automaticTypeDirectiveFileData
#[derive(Debug, Clone)]
pub struct AutomaticTypeDirectiveData {
    pub type_reference: String,
    pub package_id: Option<PackageId>,
}

// Go: internal/module/types.go:PackageId
#[derive(Debug, Clone, Default)]
pub struct PackageId {
    pub name: String,
    pub sub_module_name: String,
    pub version: String,
}

impl PackageId {
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }
}

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.version)?;
        if !self.sub_module_name.is_empty() {
            write!(f, "/{}", self.sub_module_name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum FileIncludeReasonData {
    Index(usize),
    ReferencedFile(ReferencedFileData),
    AutomaticTypeDirective(AutomaticTypeDirectiveData),
    DefaultLib,
}

// Go: internal/compiler/fileInclude.go:FileIncludeReason
pub struct FileIncludeReason {
    pub kind: FileIncludeKind,
    pub data: FileIncludeReasonData,
    _relative_diag_once: Once,
    _diag_once: Once,
}

impl std::fmt::Debug for FileIncludeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileIncludeReason")
            .field("kind", &self.kind)
            .field("data", &self.data)
            .finish()
    }
}

impl FileIncludeReason {
    pub fn root_file(index: usize) -> Self {
        Self {
            kind: FileIncludeKind::RootFile,
            data: FileIncludeReasonData::Index(index),
            _relative_diag_once: Once::new(),
            _diag_once: Once::new(),
        }
    }

    pub fn lib_file(index: usize) -> Self {
        Self {
            kind: FileIncludeKind::LibFile,
            data: FileIncludeReasonData::Index(index),
            _relative_diag_once: Once::new(),
            _diag_once: Once::new(),
        }
    }

    pub fn default_lib() -> Self {
        Self {
            kind: FileIncludeKind::LibFile,
            data: FileIncludeReasonData::DefaultLib,
            _relative_diag_once: Once::new(),
            _diag_once: Once::new(),
        }
    }

    pub fn referenced_file(kind: FileIncludeKind, data: ReferencedFileData) -> Self {
        debug_assert!(kind.is_referenced_file());
        Self {
            kind,
            data: FileIncludeReasonData::ReferencedFile(data),
            _relative_diag_once: Once::new(),
            _diag_once: Once::new(),
        }
    }

    pub fn automatic_type_directive(data: AutomaticTypeDirectiveData) -> Self {
        Self {
            kind: FileIncludeKind::AutomaticTypeDirectiveFile,
            data: FileIncludeReasonData::AutomaticTypeDirective(data),
            _relative_diag_once: Once::new(),
            _diag_once: Once::new(),
        }
    }

    pub fn is_referenced_file(&self) -> bool {
        self.kind.is_referenced_file()
    }

    pub fn as_index(&self) -> Option<usize> {
        match &self.data {
            FileIncludeReasonData::Index(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_lib_file_index(&self) -> Option<usize> {
        match &self.data {
            FileIncludeReasonData::Index(i) if self.kind == FileIncludeKind::LibFile => Some(*i),
            _ => None,
        }
    }

    pub fn as_referenced_file_data(&self) -> Option<&ReferencedFileData> {
        match &self.data {
            FileIncludeReasonData::ReferencedFile(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_automatic_type_directive_data(&self) -> Option<&AutomaticTypeDirectiveData> {
        match &self.data {
            FileIncludeReasonData::AutomaticTypeDirective(d) => Some(d),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "file_include_test.rs"]
mod tests;
