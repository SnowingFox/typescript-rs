//! Port of Go's `internal/compiler/emitHost.go`.
//!
//! The [`EmitHost`] trait bridges the `Program` to the emitter / declaration
//! emit pipeline.

use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};
use tsgo_tspath::Path;

/// The host interface the emitter uses during emit.
// Go: internal/compiler/emitHost.go:EmitHost
pub trait EmitHost: Send + Sync {
    fn options(&self) -> &CompilerOptions;
    fn use_case_sensitive_file_names(&self) -> bool;
    fn get_current_directory(&self) -> &str;
    fn common_source_directory(&self) -> &str;
    fn is_emit_blocked(&self, file: &str) -> bool;
    fn file_exists(&self, path: &str) -> bool;
    fn write_file(&self, file_name: &str, text: &str) -> std::io::Result<()>;
    fn get_emit_module_format_of_file(&self, file_path: &Path) -> ModuleKind;
}

#[cfg(test)]
#[path = "emit_host_test.rs"]
mod tests;
