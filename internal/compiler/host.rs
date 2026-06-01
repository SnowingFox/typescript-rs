//! Port of Go `internal/compiler/host.go`: the [`CompilerHost`] abstraction
//! the program builds on, plus an in-memory/disk-backed default implementation.
//!
//! The host is the program's window onto the outside world: it owns the
//! [`Fs`](tsgo_vfs::Fs), the current working directory, and the default-library
//! location, and it is the factory that turns a file name into a parsed
//! [`ParsedFile`].
//!
//! # Divergence from Go
//!
//! Go's `CompilerHost.GetSourceFile` returns a rich `*ast.SourceFile`. The Rust
//! `tsgo_ast` crate has no equivalent owned source-file type yet (the AST lives
//! in a `NodeArena` keyed by `NodeId`), so the compiler defines [`ParsedFile`]
//! here: it bundles the parse arena, the root `SourceFile` node id, the original
//! text, and the syntactic diagnostics. This is the program's source-file
//! representation until a richer `ast::SourceFile` is ported.
//!
//! ## Ownership divergence: `Rc`-held arena + bind result (P6-4)
//!
//! The checker retains its program behind `Rc<dyn BoundProgram + 'static>`
//! (round 4l mirrored Go's `NewChecker(program)` retain model), so the bound
//! view the checker pool hands it must own / `'static`-share its data — a
//! borrowing `BoundFile<'a>` no longer satisfies the API. Following PORTING §3
//! (a shared, non-owning pointer maps to `Rc<T>`) and §5 (the arena owns the
//! nodes), [`ParsedFile`] holds its arena and bind result behind `Rc`
//! (`Rc<NodeArena>` / `Rc<BindResult>`). The crate is still single-threaded
//! (the checker is `Rc`, not `Arc`; see PORTING §6), so `Rc` — not `Arc` — is
//! the faithful choice. A bound [`ParsedFile`] can then mint a cheap, owned
//! [`BoundFile`](crate::BoundFile) that clones those `Rc` handles, and K
//! checkers share one program by `Rc::clone`-ing it (Go: one `*Program` per
//! pool). This diverges from Go, where the GC owns the program and the checker
//! holds a bare pointer.

use std::rc::Rc;
use std::sync::Arc;

use tsgo_ast::{NodeArena, NodeData, NodeId};
use tsgo_binder::{bind_source_file, BindResult};
use tsgo_core::get_script_kind_from_file_name;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, Diagnostic, SourceFileParseOptions};
use tsgo_vfs::Fs;

/// A parsed source file: the owning arena, the root `SourceFile` node id, the
/// original text, and the syntactic diagnostics.
///
/// Stands in for Go's `*ast.SourceFile` (see the module-level divergence note).
///
/// Side effects: none (owns parse output).
// Go: internal/ast/ast.go:SourceFile
#[derive(Debug)]
pub struct ParsedFile {
    arena: Rc<NodeArena>,
    node: NodeId,
    file_name: String,
    text: String,
    diagnostics: Vec<Diagnostic>,
    bind: Option<Rc<BindResult>>,
}

impl ParsedFile {
    /// Bundles parse output (arena + root node id + diagnostics) with the file's
    /// name and original text.
    ///
    /// Side effects: none.
    pub fn new(
        file_name: String,
        text: String,
        arena: NodeArena,
        node: NodeId,
        diagnostics: Vec<Diagnostic>,
    ) -> ParsedFile {
        ParsedFile {
            arena: Rc::new(arena),
            node,
            file_name,
            text,
            diagnostics,
            bind: None,
        }
    }

    /// Binds this file (idempotently), producing its symbol and control-flow
    /// graphs, and returns the [`BindResult`].
    ///
    /// Binding mutates the arena, so it must run before the arena is shared with
    /// any [`BoundFile`](crate::BoundFile)/checker; this is the case in practice
    /// (the program binds every file, then builds the checker pool).
    ///
    /// Side effects: runs `tsgo_binder::bind_source_file`, which may set
    /// reachability/`this`/export-context `NodeFlags` on this file's arena.
    // Go: internal/compiler/program.go:BindSourceFiles (per file)
    pub fn bind(&mut self) -> &BindResult {
        if self.bind.is_none() {
            // The arena is still uniquely held before binding (no checker shares
            // it yet), so `get_mut` succeeds; the binder needs `&mut NodeArena`.
            let arena = Rc::get_mut(&mut self.arena)
                .expect("arena must be uniquely held before binding (no shared BoundFile yet)");
            self.bind = Some(Rc::new(bind_source_file(arena, self.node)));
        }
        self.bind.as_deref().expect("just bound")
    }

    /// The bind result, if this file has been bound (see [`Self::bind`]).
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:SourceFile (binder state)
    pub fn bind_result(&self) -> Option<&BindResult> {
        self.bind.as_deref()
    }

    /// A shared handle to this file's parse arena, for an owned
    /// [`BoundFile`](crate::BoundFile) the checker can retain.
    ///
    /// Side effects: none (clones an `Rc`).
    pub(crate) fn arena_rc(&self) -> Rc<NodeArena> {
        Rc::clone(&self.arena)
    }

    /// A shared handle to this file's bind result, or `None` if it has not been
    /// bound yet (see [`Self::bind`]).
    ///
    /// Side effects: none (clones an `Rc`).
    pub(crate) fn bind_rc(&self) -> Option<Rc<BindResult>> {
        self.bind.clone()
    }

    /// Reports whether this file has been bound.
    ///
    /// Side effects: none (pure).
    pub fn is_bound(&self) -> bool {
        self.bind.is_some()
    }

    /// The file's normalized name.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:SourceFile.FileName
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// The file's original (pre-parse) source text.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:SourceFile.Text
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The parse arena owning every node of this file.
    ///
    /// Side effects: none (pure).
    pub fn arena(&self) -> &NodeArena {
        &self.arena
    }

    /// The root `SourceFile` node id.
    ///
    /// Side effects: none (pure).
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// The syntactic diagnostics produced while parsing this file.
    ///
    /// Side effects: none (pure).
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The module-specifier strings of this file's top-level imports/re-exports
    /// and dynamic `import`/`require` calls, in source order.
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    /// use tsgo_compiler::new_compiler_host;
    /// use tsgo_compiler::CompilerHost;
    /// use tsgo_parser::SourceFileParseOptions;
    /// use tsgo_vfs::vfstest::MapFs;
    /// use tsgo_vfs::Fs;
    ///
    /// let fs: Arc<dyn Fs + Send + Sync> =
    ///     Arc::new(MapFs::from_map([("/a.ts", "import * as b from \"./b\";")], true));
    /// let host = new_compiler_host("/", fs, "/lib");
    /// let opts = SourceFileParseOptions { file_name: "/a.ts".into() };
    /// let parsed = host.get_source_file(&opts).unwrap();
    /// assert_eq!(parsed.import_specifiers(), vec!["./b".to_string()]);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:SourceFile.Imports
    pub fn import_specifiers(&self) -> Vec<String> {
        match self.arena.data(self.node) {
            NodeData::SourceFile(data) => data
                .imports
                .iter()
                .map(|&id| self.arena.text(id).to_string())
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// Lets [`tsgo_outputpaths`] compute a file's output paths from its name and
/// script kind.
///
/// The script kind is derived from the file name (JSON detection only needs the
/// extension), matching how the host parses the file.
///
/// Side effects: none (accessors).
// Go: internal/ast/ast.go:SourceFile (FileName + ScriptKind)
impl tsgo_outputpaths::SourceFileLike for ParsedFile {
    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn script_kind(&self) -> ScriptKind {
        get_script_kind_from_file_name(&self.file_name)
    }
}

/// The host abstraction the program builds on.
///
/// Side effects: implementations may read the file system.
// Go: internal/compiler/host.go:CompilerHost
pub trait CompilerHost: Send + Sync {
    /// The file system used for all I/O, as a shareable handle.
    ///
    /// Side effects: none (returns a shared handle).
    // Go: internal/compiler/host.go:CompilerHost.FS
    fn fs(&self) -> Arc<dyn Fs + Send + Sync>;

    /// The directory holding the bundled default `lib.*.d.ts` files.
    ///
    /// Side effects: none.
    // Go: internal/compiler/host.go:CompilerHost.DefaultLibraryPath
    fn default_library_path(&self) -> &str;

    /// The current working directory, used to root relative paths.
    ///
    /// Side effects: none.
    // Go: internal/compiler/host.go:CompilerHost.GetCurrentDirectory
    fn get_current_directory(&self) -> &str;

    /// Reads and parses `opts.file_name`, or returns `None` if it cannot be read.
    ///
    /// Side effects: reads the file system.
    // Go: internal/compiler/host.go:compilerHost.GetSourceFile
    fn get_source_file(&self, opts: &SourceFileParseOptions) -> Option<ParsedFile>;
}

/// The default [`CompilerHost`] over a shared [`Fs`].
///
/// Side effects: none at construction.
// Go: internal/compiler/host.go:compilerHost
pub struct CompilerHostImpl {
    current_directory: String,
    fs: Arc<dyn Fs + Send + Sync>,
    default_library_path: String,
}

/// Creates a [`CompilerHostImpl`] from the current directory, file system, and
/// default-library path.
///
/// Side effects: none (stores the handles).
// Go: internal/compiler/host.go:NewCompilerHost
pub fn new_compiler_host(
    current_directory: impl Into<String>,
    fs: Arc<dyn Fs + Send + Sync>,
    default_library_path: impl Into<String>,
) -> CompilerHostImpl {
    CompilerHostImpl {
        current_directory: current_directory.into(),
        fs,
        default_library_path: default_library_path.into(),
    }
}

/// Parses `text` as the file named by `opts`, choosing the script kind from the
/// file extension (mirroring Go's `compilerHost.GetSourceFile`).
///
/// # Panics
/// Panics if the file name has no recognized script-kind extension, matching
/// `tsgo_parser::parse_source_file`'s contract (Go panics the same way).
///
/// Side effects: allocates a fresh parse arena.
// Go: internal/compiler/host.go:compilerHost.GetSourceFile
pub(crate) fn parse_file(opts: SourceFileParseOptions, text: String) -> ParsedFile {
    let script_kind = effective_script_kind(&opts.file_name);
    let file_name = opts.file_name.clone();
    let result = parse_source_file(opts, &text, script_kind);
    ParsedFile::new(
        file_name,
        text,
        result.arena,
        result.source_file,
        result.diagnostics,
    )
}

/// Picks the script kind for `file_name`, falling back to `Ts` for extensions
/// the scanner does not recognize so parsing never panics on the foundation's
/// reachable inputs.
///
/// This is the script kind a file is actually parsed with, so the bind-and-check
/// gate (`Program::can_include_bind_and_check_diagnostics`) keys off it just as
/// Go reads `SourceFile.ScriptKind`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:GetScriptKindFromFileName
pub(crate) fn effective_script_kind(file_name: &str) -> ScriptKind {
    match get_script_kind_from_file_name(file_name) {
        ScriptKind::Unknown => ScriptKind::Ts,
        kind => kind,
    }
}

impl CompilerHost for CompilerHostImpl {
    fn fs(&self) -> Arc<dyn Fs + Send + Sync> {
        self.fs.clone()
    }

    fn default_library_path(&self) -> &str {
        &self.default_library_path
    }

    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }

    fn get_source_file(&self, opts: &SourceFileParseOptions) -> Option<ParsedFile> {
        let text = self.fs.read_file(&opts.file_name)?;
        Some(parse_file(opts.clone(), text))
    }
}

#[cfg(test)]
#[path = "host_test.rs"]
mod tests;
