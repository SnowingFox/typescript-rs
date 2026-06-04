//! Port of Go package `internal/compiler`.
//!
//! The compiler orchestrates a full program build: it loads and parses source
//! files (`fileloader`/`filesparser`), resolves modules, builds the binder/
//! checker pool, and drives emit (`emitter`) through the printer + transform
//! pipeline. `program.go` is the central `Program` type that ties these
//! together and provides the real host the checker's `new_checker` needs.
//!
//! Ported incrementally under strict TDD (see `docs/rust-rewrite/references/tdd.md`
//! and the `/tdd` skill). Foundation first (host, file loading/parsing, the
//! `Program` skeleton); the emitter (which wires in `tsgo_transformers`) comes
//! last.

pub mod boundfile;
pub mod checkerpool;
pub mod emit_host;
pub mod emitter;
pub mod file_include;
pub mod fileloader;
pub mod filesparser;
pub mod host;
pub mod includeprocessor;
pub mod multifile;
pub mod processing_diagnostic;
pub mod program;
pub mod projectreference;
pub mod verify_options;

pub use boundfile::BoundFile;
pub use checkerpool::{checker_count, CompilerCheckerPool};
pub use emit_host::EmitHost;
pub use emitter::EmitOnly;
pub use file_include::{
    AutomaticTypeDirectiveData, FileIncludeKind, FileIncludeReason, FileIncludeReasonData,
    PackageId, ReferencedFileData,
};
pub use fileloader::{
    get_default_lib_file_priority, import_syntax_affects_module_resolution,
    process_all_program_files, ProcessedFiles,
};
pub use host::{new_compiler_host, CompilerHost, CompilerHostImpl, ParsedFile};
pub use includeprocessor::IncludeProcessor;
pub use multifile::MultiFileBoundProgram;
pub use processing_diagnostic::{ProcessingDiagnostic, ProcessingDiagnosticKind};
pub use program::{
    combine_emit_results, new_program, EmitOptions, EmitResult, Program, ProgramOptions,
    SourceMapEmitResult, WriteFileCallback, WriteFileData,
};
pub use projectreference::{
    check_source_files_belong_to_root_dir, get_output_declaration_file_name,
    get_output_js_file_name, get_resolved_project_reference, resolve_project_references,
    BuildOrder, ProjectReferenceDiagnostic, ResolvedProjectReferences,
};
pub use verify_options::{verify_compiler_options, OptionsDiagnostic};
