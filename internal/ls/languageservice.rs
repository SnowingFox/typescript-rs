//! Port of Go `internal/ls/languageservice.go`: the [`LanguageService`] — the
//! per-project object that wraps a compiler [`Program`] and answers per-file
//! editor queries (diagnostics, quick-info/hover, ...).
//!
//! # Reachable shape
//!
//! Go's `LanguageService` holds the project path, the [`Host`](crate::LanguageServiceHost),
//! the resolved user preferences, the `*compiler.Program`, the position
//! [`Converters`], and a source-map document-position-mapper cache. The
//! reachable LS-root round keeps the project path, host, program, and
//! converters; the preferences and source-map caches are deferred with the
//! features that need them (see [`crate::host`]).
//!
//! # How a feature reaches a token + a type checker
//!
//! The compiler's checker pool is internal to the [`Program`], so a feature that
//! needs a [`Checker`] for a file builds one the same way the pool does: it
//! binds the program's files, joins every bound file into one
//! [`MultiFileBoundProgram`] (the production multi-file [`BoundProgram`] view),
//! takes that file's per-file [`view`](BoundProgram::file_view), and constructs
//! a [`Checker`] over the shared program. The file's [`arena`](BoundProgram::arena)
//! and root feed [`astnav`](tsgo_astnav)'s shared-borrow [`NavSourceFile`] so a
//! byte position resolves to a token whose node id is consistent with the
//! checker's symbol/flow side tables. See [`LanguageService::file_check_context`].

use std::collections::HashMap;
use std::rc::Rc;

use tsgo_ast::NodeId;
use tsgo_checker::{BoundProgram, Checker};
use tsgo_compiler::{MultiFileBoundProgram, Program};
use tsgo_core::text::TextPos;
use tsgo_ls_lsconv::{
    compute_lsp_line_starts, Converters, LSPLineMap, PositionEncodingKind, Script,
};
use tsgo_tspath::Path;

use crate::host::LanguageServiceHost;

/// The language service for one project: it wraps a compiler [`Program`] and
/// answers per-file editor queries through the per-feature methods on this type
/// (see [`crate::diagnostics`] and [`crate::hover`]).
///
/// Side effects: none at construction beyond computing the per-file line maps
/// the converters use.
// Go: internal/ls/languageservice.go:LanguageService
pub struct LanguageService {
    project_path: Path,
    host: Rc<dyn LanguageServiceHost>,
    program: Program,
    converters: Rc<Converters>,
}

impl LanguageService {
    /// Builds a [`LanguageService`] over `program`, reading its outside-world
    /// state through `host`.
    ///
    /// The position [`Converters`] are built here from the program's own file
    /// snapshots (UTF-16 character offsets, the LSP default), mirroring Go where
    /// the project builds the host's converters from the same file set (see the
    /// [`crate::host`] divergence note).
    ///
    /// Side effects: computes one [`LSPLineMap`] per source file for the
    /// converters.
    // Go: internal/ls/languageservice.go:NewLanguageService
    pub fn new(
        project_path: Path,
        program: Program,
        host: Rc<dyn LanguageServiceHost>,
    ) -> LanguageService {
        let converters = build_converters(&program);
        LanguageService {
            project_path,
            host,
            program,
            converters,
        }
    }

    /// The compiler program this service wraps.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/languageservice.go:LanguageService.GetProgram
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Mutable access to the wrapped program, for features that drive
    /// (idempotent) binding / checking (e.g. semantic diagnostics).
    ///
    /// Side effects: none on its own; callers may bind/check through it.
    // Go: internal/ls/languageservice.go:LanguageService.GetProgram (mutable use)
    pub(crate) fn program_mut(&mut self) -> &mut Program {
        &mut self.program
    }

    /// The project path this service was constructed with.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/languageservice.go:LanguageService (projectPath)
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    /// The position converters (internal byte offset <-> LSP `(line, character)`).
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/languageservice.go:LanguageService (converters)
    pub fn converters(&self) -> &Converters {
        &self.converters
    }

    /// Whether the host compares file names case-sensitively.
    ///
    /// Side effects: none (delegates to the host).
    // Go: internal/ls/languageservice.go:LanguageService.UseCaseSensitiveFileNames
    pub fn use_case_sensitive_file_names(&self) -> bool {
        self.host.use_case_sensitive_file_names()
    }

    /// Reads `file_name`'s current contents through the host.
    ///
    /// Side effects: delegates to the host's file read.
    // Go: internal/ls/languageservice.go:LanguageService.ReadFile
    pub fn read_file(&self, file_name: &str) -> Option<String> {
        self.host.read_file(file_name)
    }

    /// Canonicalizes `file_name` to a project-rooted [`Path`], using the
    /// program's current directory and the host's case sensitivity.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/languageservice.go:LanguageService.toPath
    pub fn to_path(&self, file_name: &str) -> Path {
        self.program.to_path(file_name)
    }

    /// A [`Script`] (file name + raw bytes) for `file_name`, the input the
    /// [`Converters`] need, or `None` if the program has no such file.
    ///
    /// Side effects: none (pure).
    pub(crate) fn document_script(&self, file_name: &str) -> Option<DocumentScript> {
        let file = self.program.get_source_file(file_name)?;
        Some(DocumentScript {
            file_name: file.file_name().to_string(),
            text: file.text().as_bytes().to_vec(),
        })
    }

    /// Binds the program's files, joins them into one [`MultiFileBoundProgram`],
    /// and returns the per-file checking context for `file_name`: a fresh
    /// [`Checker`] over the shared program, that file's per-file
    /// [`view`](BoundProgram), its source text, and its root node id.
    ///
    /// Returns `None` if the program has no such file or the file did not bind.
    /// This is the language-service analogue of Go's
    /// `program.GetTypeCheckerForFile` + `tryGetProgramAndFile`: the compiler's
    /// pool is internal, so the service reconstructs the multi-file bound view
    /// (as the pool does) to get a checker for the file.
    ///
    /// Side effects: binds every program file (idempotent; mutates per-file
    /// arenas) and allocates a checker.
    // Go: internal/ls/languageservice.go:LanguageService.tryGetProgramAndFile (+ program.GetTypeCheckerForFile)
    pub(crate) fn file_check_context(&mut self, file_name: &str) -> Option<FileCheckContext> {
        let (canonical, text) = {
            let file = self.program.get_source_file(file_name)?;
            (file.file_name().to_string(), file.text().to_string())
        };

        // Bind every file (the pool's precondition), then join the bound files
        // into one multi-file program carrying the program's real options.
        self.program.bind_source_files();
        let options = Rc::new(self.program.options().clone());
        let files = self.program.source_files();
        let view_index = files
            .iter()
            .filter(|f| f.is_bound())
            .position(|f| f.file_name() == canonical)?;
        let program: Rc<dyn BoundProgram> =
            Rc::new(MultiFileBoundProgram::new_with_options(files, options));
        let handle = *program.source_files().get(view_index)?;
        let view = program.file_view(handle)?;
        let root = view.root();
        let checker = Checker::new_checker(Rc::clone(&program));
        Some(FileCheckContext {
            checker,
            view,
            text,
            root,
        })
    }
}

/// The per-file checking context returned by
/// [`LanguageService::file_check_context`]: a checker plus that file's bound
/// view, text, and root node id.
///
/// All fields are owned (the checker retains its program by `Rc`, the view is an
/// `Rc` clone), so the context outlives the `&mut LanguageService` borrow that
/// produced it.
///
/// Side effects: none (owns the checker + shared view).
pub(crate) struct FileCheckContext {
    /// A checker over the multi-file bound program (Go's `GetTypeCheckerForFile`).
    pub checker: Checker,
    /// The file's per-file bound view (its own arena, the merged symbol space /
    /// globals).
    pub view: Rc<dyn BoundProgram>,
    /// The file's source text (for [`astnav`](tsgo_astnav) scanning).
    pub text: String,
    /// The file's root `SourceFile` node id (in the view's arena).
    pub root: NodeId,
}

/// A source document the [`Converters`] operate on: its file name and raw bytes.
///
/// Side effects: none (owns the bytes).
// Go: internal/ls/lsconv/converters.go:Script (the language-service document)
pub(crate) struct DocumentScript {
    file_name: String,
    text: Vec<u8>,
}

impl Script for DocumentScript {
    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn text(&self) -> &[u8] {
        &self.text
    }
}

/// Builds the project's [`Converters`] from the program's file snapshots,
/// measuring character offsets in UTF-16 code units (the LSP default).
///
/// Each source file's [`LSPLineMap`] is precomputed and captured in the
/// `get_line_map` callback; a file the program does not know maps to a single
/// empty line (so a conversion never panics).
///
/// Side effects: computes one line map per source file.
// Go: internal/ls/languageservice.go:NewLanguageService (host.Converters)
fn build_converters(program: &Program) -> Rc<Converters> {
    let mut maps: HashMap<String, Rc<LSPLineMap>> = HashMap::new();
    for file in program.source_files() {
        maps.insert(
            file.file_name().to_string(),
            Rc::new(compute_lsp_line_starts(file.text().as_bytes())),
        );
    }
    let maps = Rc::new(maps);
    let fallback = Rc::new(LSPLineMap {
        line_starts: vec![TextPos(0)],
        ascii_only: true,
    });
    Rc::new(Converters::new(
        PositionEncodingKind::utf16(),
        move |name: &str| {
            maps.get(name)
                .map(Rc::clone)
                .unwrap_or_else(|| Rc::clone(&fallback))
        },
    ))
}

#[cfg(test)]
#[path = "languageservice_test.rs"]
mod tests;
