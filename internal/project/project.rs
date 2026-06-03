//! Project struct — represents a single TypeScript project.
//!
//! 1:1 port of Go `internal/project/project.go`.
//!
//! A [`Project`] is either a configured project (backed by a tsconfig.json)
//! or an inferred project (created as a fallback). It owns a reference to
//! the compiler [`Program`](DEFER) and manages its lifecycle.

use tsgo_tspath::Path;

use crate::kind::{Kind, ProgramUpdateKind};

/// The path used for inferred projects (lowercase so toPath is a no-op).
// Go: internal/project/project.go:inferredProjectName
pub const INFERRED_PROJECT_NAME: &str = "/dev/null/inferred";

/// Visual separator for project printing.
const HR: &str = "-----------------------------------------------";

/// A TypeScript project (configured or inferred).
///
/// # Examples
/// ```
/// use tsgo_project::project::Project;
/// use tsgo_project::kind::Kind;
/// let p = Project::new_skeleton("tsconfig.json", Kind::Configured, "/app");
/// assert_eq!(p.name(), "tsconfig.json");
/// assert_eq!(p.kind(), Kind::Configured);
/// ```
// Go: internal/project/project.go:Project
#[derive(Debug, Clone)]
pub struct Project {
    kind: Kind,
    current_directory: String,
    config_file_name: String,
    config_file_path: Path,

    dirty: bool,
    #[allow(dead_code)]
    dirty_file_path: Path,

    /// The kind of update that was performed on the program last time.
    pub program_update_kind: ProgramUpdateKind,
    /// The ID of the snapshot that last updated the program.
    pub program_last_update: u64,
    // DEFER(phase-8): host, CommandLine, Program, checkerPool, watches, etc.
}

impl Project {
    /// Creates a minimal project skeleton (no host, program, or watches).
    ///
    /// Full construction needs `ProjectCollectionBuilder` (DEFER).
    // Go: internal/project/project.go:NewProject (skeleton)
    pub fn new_skeleton(config_file_name: &str, kind: Kind, current_directory: &str) -> Self {
        Self {
            kind,
            current_directory: current_directory.to_string(),
            config_file_name: config_file_name.to_string(),
            config_file_path: Path(config_file_name.to_lowercase()),
            dirty: true,
            dirty_file_path: Path::default(),
            program_update_kind: ProgramUpdateKind::None,
            program_last_update: 0,
        }
    }

    /// Creates a configured project skeleton.
    // Go: internal/project/project.go:NewConfiguredProject
    pub fn new_configured_skeleton(config_file_name: &str, config_file_path: Path) -> Self {
        let current_directory = tsgo_tspath::get_directory_path(config_file_name);
        Self {
            kind: Kind::Configured,
            current_directory,
            config_file_name: config_file_name.to_string(),
            config_file_path,
            dirty: true,
            dirty_file_path: Path::default(),
            program_update_kind: ProgramUpdateKind::None,
            program_last_update: 0,
        }
    }

    /// Creates an inferred project skeleton.
    // Go: internal/project/project.go:NewInferredProject
    pub fn new_inferred_skeleton(current_directory: &str) -> Self {
        Self {
            kind: Kind::Inferred,
            current_directory: current_directory.to_string(),
            config_file_name: INFERRED_PROJECT_NAME.to_string(),
            config_file_path: Path(INFERRED_PROJECT_NAME.to_string()),
            dirty: true,
            dirty_file_path: Path::default(),
            program_update_kind: ProgramUpdateKind::None,
            program_last_update: 0,
        }
    }

    /// Returns the project name (config file name).
    // Go: internal/project/project.go:Project.Name
    pub fn name(&self) -> &str {
        &self.config_file_name
    }

    /// Returns a short, human-readable display name relative to `cwd`.
    // Go: internal/project/project.go:Project.DisplayName
    pub fn display_name(&self, cwd: &str) -> String {
        if self.kind == Kind::Inferred {
            return tsgo_tspath::get_base_file_name(&self.current_directory);
        }
        tsgo_tspath::convert_to_relative_path(
            &self.config_file_name,
            &tsgo_tspath::ComparePathsOptions {
                current_directory: cwd.to_string(),
                use_case_sensitive_file_names: true,
            },
        )
    }

    /// Returns the project's identity path.
    // Go: internal/project/project.go:Project.ID
    pub fn id(&self) -> &Path {
        &self.config_file_path
    }

    /// Returns the project kind.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Returns the config file name.
    ///
    /// # Panics
    /// Panics if the project is not configured.
    // Go: internal/project/project.go:Project.ConfigFileName
    pub fn config_file_name(&self) -> &str {
        if self.kind != Kind::Configured {
            panic!("ConfigFileName called on non-configured project");
        }
        &self.config_file_name
    }

    /// Returns the config file path.
    ///
    /// # Panics
    /// Panics if the project is not configured.
    // Go: internal/project/project.go:Project.ConfigFilePath
    pub fn config_file_path(&self) -> &Path {
        if self.kind != Kind::Configured {
            panic!("ConfigFilePath called on non-configured project");
        }
        &self.config_file_path
    }

    /// Returns the current working directory of this project.
    pub fn current_directory(&self) -> &str {
        &self.current_directory
    }

    /// Returns whether this project is dirty (needs rebuild).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Prints project info into the builder.
    // Go: internal/project/project.go:Project.print
    pub fn print(&self, _write_file_names: bool, builder: &mut String) {
        builder.push_str(&format!("\nProject '{}'\n", self.name()));
        // DEFER(phase-8): program-aware printing
        builder.push_str("\tFiles (0) NoProgram\n");
        builder.push_str(HR);
    }
}

#[cfg(test)]
#[path = "project_test.rs"]
mod tests;
