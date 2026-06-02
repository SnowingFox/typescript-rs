//! A thin abstraction over a bound source program.
//!
//! Symbol queries (`resolve_name`, `get_symbol_at_location`, ...) need the
//! parsed node arena plus the binder's per-node symbol map and per-container
//! `locals` tables. Rather than depend on a concrete program/host (the real one
//! is `compiler.Program`, ported in Phase 6) or on `tsgo_binder`'s `BindResult`
//! type directly, the query layer talks to this trait, which exposes only
//! `tsgo_ast` types.
//!
//! In tests a minimal stub (parse via `tsgo_parser` + bind via `tsgo_binder`)
//! implements it; later phases plug in the real program.

use std::rc::Rc;
use std::sync::OnceLock;

use tsgo_ast::flow::{FlowList, FlowListId, FlowNode, FlowNodeId, FlowSwitchClauseData};
use tsgo_ast::{NodeArena, NodeId, Symbol, SymbolId, SymbolTable};
use tsgo_core::compileroptions::CompilerOptions;

/// The shared, all-defaults [`CompilerOptions`] a single-file program reports
/// until a real program (or test harness) supplies its own (Go's
/// `program.Options()`).
///
/// Side effects: none (lazily initializes a process-wide default on first use).
pub(crate) fn default_compiler_options() -> &'static CompilerOptions {
    static DEFAULT: OnceLock<CompilerOptions> = OnceLock::new();
    DEFAULT.get_or_init(CompilerOptions::default)
}

/// A bound source file the checker can answer symbol queries against.
///
/// This is the minimal surface the 4b symbol queries require; it grows as later
/// sub-phases need more (declared types, flow, diagnostics, multi-file lookup).
///
/// # Examples
/// ```
/// use tsgo_checker::BoundProgram;
/// // The query layer is generic over any bound program.
/// fn statement_count<P: BoundProgram>(p: &P) -> usize {
///     p.arena().node_count() // total nodes parsed
/// }
/// ```
///
/// Side effects: implementations are read-only views over already-bound data.
// Go: internal/compiler/program.go:Program (the subset the checker queries)
pub trait BoundProgram {
    /// The node arena owning every parsed node.
    fn arena(&self) -> &NodeArena;

    /// The root `SourceFile` node id.
    fn root(&self) -> NodeId;

    /// The declaration symbol bound to `node` (Go's `node.Symbol()`), if any.
    fn symbol_of_node(&self, node: NodeId) -> Option<SymbolId>;

    /// The symbol record for `id`.
    fn symbol(&self, id: SymbolId) -> &Symbol;

    /// The `locals` table of a locals-bearing container node, if it has one.
    fn locals(&self, container: NodeId) -> Option<&SymbolTable>;

    /// The raw source text of the file this program/view is checking (Go's
    /// `SourceFile.Text()`), if available.
    ///
    /// Byte offsets in this file's [`arena`](BoundProgram::arena) (node
    /// `pos`/`end`) index into this string. The checker needs it to reproduce
    /// Go's trivia-skipped diagnostic start (`scanner.GetErrorRangeForNode`,
    /// which does `SkipTrivia(text, node.Pos())`) — a node's `pos` is its
    /// FULL-start (leading trivia included), so an error span that must match
    /// `tsc`'s committed baseline byte-for-byte (e.g. a JSX element's TS7026)
    /// has to skip the leading whitespace before the first token.
    ///
    /// The default returns `None`: such a diagnostic then falls back to the raw
    /// node `pos` (correct whenever the node has no leading trivia). A program
    /// that owns the file's text overrides this.
    ///
    /// Side effects: none (a read-only view).
    // Go: internal/ast/ast.go:SourceFile.Text / internal/scanner/scanner.go:GetErrorRangeForNode
    fn source_text(&self) -> Option<&str> {
        None
    }

    /// The program's merged global symbol table (Go's `Checker.globals`), if the
    /// program exposes one.
    ///
    /// Go's `NewChecker` builds `c.globals` by merging the top-level `locals` of
    /// every global (non-module / script) source file. The checker resolves
    /// global types/values (`getGlobalSymbol`/`getGlobalType`) against it. The
    /// real cross-file merge plus lib.d.ts population is a P6 compiler concern;
    /// until then the default returns `None` and a test program drives synthetic
    /// globals (the top-level declarations of a script source file).
    ///
    /// Side effects: none (a read-only view).
    // Go: internal/checker/checker.go:Checker.globals (built in initializeChecker)
    // blocked-by: cross-file global merge + lib.d.ts loading (P6).
    fn globals(&self) -> Option<&SymbolTable> {
        None
    }

    /// The compiler options this program was created with (Go's
    /// `program.Options()`, which `NewChecker` reads into `c.compilerOptions`).
    ///
    /// The checker reads option-gated behavior (e.g. `strictNullChecks`, the
    /// `--target`/`--downlevelIteration` iteration gating) off these. The
    /// default returns a shared all-defaults [`CompilerOptions`], so existing
    /// single-file implementations (the compiler's `BoundFile`, the test
    /// `StubProgram`) compile unchanged and behave as the all-defaults case; a
    /// program that carries options overrides this.
    ///
    /// Side effects: none (a read-only view).
    // Go: internal/compiler/program.go:Program.Options
    fn compiler_options(&self) -> &CompilerOptions {
        default_compiler_options()
    }

    /// The control-flow node attached to `node` (Go's `node.FlowNode`), if any.
    ///
    /// The binder sets this on narrowable references (identifiers, `this`,
    /// property/element access) and removes it for unreachable code.
    fn flow_node_of(&self, node: NodeId) -> Option<FlowNodeId>;

    /// The flow node for `id` in the binder's control-flow graph.
    fn flow_node(&self, id: FlowNodeId) -> FlowNode;

    /// The flow-list cell for `id` (a cons cell of label antecedents).
    fn flow_list(&self, id: FlowListId) -> FlowList;

    /// The synthetic switch-clause data for a `SWITCH_CLAUSE` flow node `id`, if
    /// the binder recorded one (Go's `flow.Node.AsFlowSwitchClauseData()`).
    ///
    /// The default returns `None` for programs that do not track switch flow;
    /// the bound stub overrides it. Side effects: none.
    fn flow_switch_clause_data(&self, id: FlowNodeId) -> Option<FlowSwitchClauseData> {
        let _ = id;
        None
    }

    /// The root `SourceFile` ids of every file in the program (Go's
    /// `program.SourceFiles()`), as the opaque file handles the checker drives
    /// per file.
    ///
    /// Each id is accepted by [`BoundProgram::file_view`],
    /// [`Checker::check_source_file`](crate::Checker::check_source_file), and
    /// [`Checker::get_diagnostics`](crate::Checker::get_diagnostics). The default
    /// is a single-file program: just [`root`](BoundProgram::root). A multi-file
    /// program returns one handle per file (which may be encoded so distinct
    /// files never collide even though each file's raw arena ids start at 0).
    ///
    /// Side effects: none (a read-only view).
    // Go: internal/compiler/program.go:Program.SourceFiles
    fn source_files(&self) -> Vec<NodeId> {
        vec![self.root()]
    }

    /// The source-file handle this program is a view of, used as the partition
    /// key for diagnostics (so `get_diagnostics(file)` returns only this file's
    /// diagnostics).
    ///
    /// For a single-file program this is its [`root`](BoundProgram::root). For a
    /// per-file view of a multi-file program this is the same (collision-free)
    /// handle the view was obtained by via [`source_files`](BoundProgram::source_files)
    /// / [`file_view`](BoundProgram::file_view) — distinct from the raw
    /// [`root`](BoundProgram::root), which indexes the file's own arena.
    ///
    /// Side effects: none (a read-only view).
    // Go: internal/ast/diagnostic.go:Diagnostic.File (the file a diagnostic belongs to)
    fn file_handle(&self) -> NodeId {
        self.root()
    }

    /// A per-file [`BoundProgram`] view for the file handle `file` (its own node
    /// arena, plus the program-wide merged symbol space and
    /// [`globals`](BoundProgram::globals)).
    ///
    /// This is how a multi-file program hands the checker the single-file view it
    /// type-checks one file against: the view's [`arena`](BoundProgram::arena) is
    /// that file's arena (so its file-local node ids resolve), while
    /// [`symbol`](BoundProgram::symbol)/[`globals`](BoundProgram::globals) span
    /// the whole program (so a reference into another file's globals resolves).
    /// The default returns `None`: a single-file program is already its own view.
    ///
    /// Side effects: none (clones a shared view handle).
    // Go: internal/compiler/program.go:Program (per-file checking context)
    fn file_view(&self, file: NodeId) -> Option<Rc<dyn BoundProgram>> {
        let _ = file;
        None
    }

    /// The per-file [`BoundProgram`] view of the file that *declares* `symbol`
    /// (so its declared type can be built against the arena that owns its
    /// declaration nodes).
    ///
    /// A global type/value declared in file A but referenced while checking file
    /// B must have its declared type built against file A's arena; this returns
    /// file A's view for a (merged) global symbol id. The default returns `None`:
    /// a single-file program owns every symbol, so it is its own owning view.
    ///
    /// Side effects: none (clones a shared view handle).
    // Go: internal/compiler/program.go:Program (a symbol's declaring file)
    fn view_for_symbol(&self, symbol: SymbolId) -> Option<Rc<dyn BoundProgram>> {
        let _ = symbol;
        None
    }

    /// Resolves a module-specifier string written at an import/export in the file
    /// `importing_file` (a [`file_handle`](BoundProgram::file_handle)) to the
    /// (merged) symbol of the target external module — its source file's
    /// `ValueModule` symbol, whose [`exports`](tsgo_ast::Symbol::exports) table is
    /// the module's exported names.
    ///
    /// This is the specifier → module-symbol bridge the checker's
    /// `resolveExternalModuleName` needs: the compiler resolves and loads every
    /// `import`/`export` specifier during program construction, and a multi-file
    /// program records, per importing file, which target file each specifier
    /// resolved to. The default returns `None` (a single-file program / stub has
    /// no cross-module imports); a multi-file program overrides it.
    ///
    /// Side effects: none (a read-only lookup).
    // Go: internal/checker/checker.go:Checker.resolveExternalModule
    //     (program.GetResolvedModule -> GetSourceFileForResolvedModule -> file.Symbol)
    fn resolve_module_symbol(&self, importing_file: NodeId, specifier: &str) -> Option<SymbolId> {
        let _ = (importing_file, specifier);
        None
    }
}

#[cfg(test)]
#[path = "program_test.rs"]
mod tests;
