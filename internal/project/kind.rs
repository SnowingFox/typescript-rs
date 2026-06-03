//! Project kind enums and their Display impls.
//!
//! 1:1 port of Go `internal/project/project.go` (enum definitions) and
//! `internal/project/project_stringer_generated.go` (Display for `Kind`).

use std::fmt;

/// The kind of a TypeScript project.
///
/// # Examples
/// ```
/// use tsgo_project::kind::Kind;
/// assert_eq!(Kind::Inferred.to_string(), "Inferred");
/// assert_eq!(Kind::Configured.to_string(), "Configured");
/// ```
// Go: internal/project/project.go:Kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Kind {
    Inferred = 0,
    Configured = 1,
}

// Go: internal/project/project_stringer_generated.go:String
impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Inferred => write!(f, "Inferred"),
            Kind::Configured => write!(f, "Configured"),
        }
    }
}

/// Describes how the program was updated during the last snapshot update.
///
/// # Examples
/// ```
/// use tsgo_project::kind::ProgramUpdateKind;
/// assert_eq!(ProgramUpdateKind::None as i32, 0);
/// ```
// Go: internal/project/project.go:ProgramUpdateKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ProgramUpdateKind {
    None = 0,
    Cloned = 1,
    SameFileNames = 2,
    NewFiles = 3,
}

/// Indicates the type of pending reload for a project.
///
/// # Examples
/// ```
/// use tsgo_project::kind::PendingReload;
/// assert_eq!(PendingReload::None as i32, 0);
/// ```
// Go: internal/project/project.go:PendingReload
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum PendingReload {
    None = 0,
    FileNames = 1,
    Full = 2,
}

#[cfg(test)]
#[path = "kind_test.rs"]
mod tests;
