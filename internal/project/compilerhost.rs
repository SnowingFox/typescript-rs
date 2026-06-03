//! Compiler host — bridges the project system and the compiler.
//!
//! 1:1 port of Go `internal/project/compilerhost.go`.
//!
//! The [`CompilerHost`] adapts the project's file system, parse cache,
//! and config registry into the interface expected by `compiler::NewProgram`.
//!
//! # DEFER notes
//! The full `compiler::CompilerHost` trait implementation requires the
//! `tsgo_compiler` crate's `Program` and `CompilerHost` types, which
//! have deep dependencies on the checker/binder pipeline. This module
//! provides the skeleton data structure; the trait impl will land when
//! the compiler pipeline is integrated.

use tsgo_tspath::Path;

/// The compiler host for a project — adapts FS, parse cache, and config
/// into what `compiler::NewProgram` needs.
///
/// # Examples
/// ```
/// use tsgo_project::compilerhost::CompilerHost;
/// let host = CompilerHost::new_skeleton(
///     tsgo_tspath::Path("/tsconfig.json".to_string()),
///     "/app",
///     "/lib",
/// );
/// assert_eq!(host.default_library_path(), "/lib");
/// ```
// Go: internal/project/compilerhost.go:compilerHost
#[derive(Debug, Clone)]
pub struct CompilerHost {
    config_file_path: Path,
    current_directory: String,
    default_lib_path: String,
    frozen: bool,
    // DEFER(phase-8): sourceFS, configFileRegistry, project, builder, logger
}

impl CompilerHost {
    /// Creates a minimal skeleton (no FS, builder, or project references).
    // Go: internal/project/compilerhost.go:newCompilerHost (skeleton)
    pub fn new_skeleton(
        config_file_path: Path,
        current_directory: &str,
        default_library_path: &str,
    ) -> Self {
        Self {
            config_file_path,
            current_directory: current_directory.to_string(),
            default_lib_path: default_library_path.to_string(),
            frozen: false,
        }
    }

    /// Returns the default library path.
    // Go: internal/project/compilerhost.go:compilerHost.DefaultLibraryPath
    pub fn default_library_path(&self) -> &str {
        &self.default_lib_path
    }

    /// Returns the current directory.
    // Go: internal/project/compilerhost.go:compilerHost.GetCurrentDirectory
    pub fn get_current_directory(&self) -> &str {
        &self.current_directory
    }

    /// Returns the config file path.
    pub fn config_file_path(&self) -> &Path {
        &self.config_file_path
    }

    /// Freezes the host, preventing post-snapshot mutations.
    ///
    /// # Panics
    /// Panics if called more than once.
    // Go: internal/project/compilerhost.go:compilerHost.freeze
    pub fn freeze(&mut self) {
        if self.frozen {
            panic!("freeze can only be called once");
        }
        self.frozen = true;
    }

    /// Returns whether the host is frozen.
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Panics if the host is frozen.
    // Go: internal/project/compilerhost.go:compilerHost.ensureAlive
    #[allow(dead_code)]
    fn ensure_alive(&self) {
        if self.frozen {
            panic!("method must not be called after snapshot initialization");
        }
    }
}

#[cfg(test)]
#[path = "compilerhost_test.rs"]
mod tests;
