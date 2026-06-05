//! A real multi-file [`BoundProgram`] view the checker pool drives per file.
//!
//! Where [`BoundFile`](crate::BoundFile) bridges *one* bound source file, this
//! module joins *every* bound [`ParsedFile`] of a program (lib + sources) into a
//! single program with one *merged* global table and one *merged* symbol space,
//! and implements the multi-file surface the checker's round-4aa `BoundProgram`
//! exposes: [`source_files`](BoundProgram::source_files),
//! [`file_handle`](BoundProgram::file_handle),
//! [`file_view`](BoundProgram::file_view), and
//! [`view_for_symbol`](BoundProgram::view_for_symbol).
//!
//! # Why offset-merge (mirrors the checker's `MultiFileProgram` harness)
//!
//! The parser mints a fresh arena (and fresh, 0-based [`SymbolId`]s and
//! [`NodeId`]s) per file, so two files' raw ids collide. The files keep
//! *separate node arenas* (a node id is only ever read through its owning file's
//! [`FileView::arena`]) but share *one offset-merged symbol vector*: file `i`'s
//! symbol ids are shifted by the sum of the previous files' symbol counts, and
//! every symbol-id-bearing field (members/exports, `parent`, `export_symbol`,
//! the `locals`/`node_symbol` maps, and the merged `globals`) is rewritten to
//! that merged id. Source-file *handles* fold the file index into the high bits
//! so they never collide either.
//!
//! This is the compiler-side counterpart of the checker's test-only
//! `MultiFileProgram`; it consumes the program's already-bound [`ParsedFile`]s
//! rather than parsing strings.
//!
//! DEFER(P6): cross-file declaration MERGE for same-named declarations (first
//! file wins here, as in the harness) and the `/// <reference lib>` graph.
//! blocked-by: the binder's cross-file `mergeSymbol`/`mergeSymbolTable` +
//! triple-slash lib-reference resolution (P6-8).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use rustc_hash::FxHashMap;
use tsgo_ast::flow::{FlowList, FlowListId, FlowNode, FlowNodeId, FlowSwitchClauseData};
use tsgo_ast::{NodeArena, NodeId, Symbol, SymbolFlags, SymbolId, SymbolTable};
use tsgo_checker::BoundProgram;
use tsgo_core::compileroptions::CompilerOptions;

use crate::host::ParsedFile;

/// The high-bit shift used to fold a file index into a multi-file source-file
/// handle, so two separately-parsed files (whose raw arena ids both start at 0)
/// never collide as program file handles. The low 24 bits hold the file's raw
/// root node id; the upper bits hold the file index.
///
/// This caps a file at `2^24` nodes, far beyond any reachable input. It matches
/// the checker harness's `FILE_INDEX_SHIFT` so handles are encoded identically.
const FILE_INDEX_SHIFT: u32 = 24;

/// An owned, shareable single-file [`BoundProgram`] view into one file of a
/// [`MultiFileBoundProgram`].
///
/// Its [`arena`](BoundProgram::arena) is *this* file's node arena (so its
/// file-local node ids resolve), while [`symbol`](BoundProgram::symbol) indexes
/// the program-wide *merged* symbol vector and [`globals`](BoundProgram::globals)
/// is the program-wide *merged* global table — so a reference into another
/// file's globals (e.g. the lib file's `String`) resolves while checking this
/// file.
///
/// Side effects: none (shares the program's arenas / symbols via `Rc`).
// Go: internal/compiler/program.go:Program (per-file checking context)
/// Cross-file resolution registry shared by every [`FileView`] of a program, so
/// a per-file view (the context the checker pool actually drives) can still
/// resolve a *sibling* file's view — needed when a declaration's nodes live in
/// another file's arena (e.g. a lib type alias resolved lazily while checking a
/// user file). The program holds the views as `Rc` (strong); the registry holds
/// them as [`Weak`] so `FileView -> registry -> FileView` is not a reference
/// cycle. Upgrades always succeed while the owning program is alive.
struct ViewRegistry {
    /// `[start, end)` merged-symbol-id range per file (parallel to `views`).
    ranges: Vec<(u32, u32)>,
    /// Back-pointers to each file's view, filled after all views are built.
    views: RefCell<Vec<Weak<FileView>>>,
    /// The specifier → module-symbol bridge: `(importing file index, specifier
    /// text)` → the (merged) `ValueModule` symbol of the resolved target file.
    /// Built from the program's per-import resolutions; the basis of the
    /// checker's `resolveExternalModuleName` (see
    /// [`BoundProgram::resolve_module_symbol`]).
    // Go: internal/compiler/program.go:Program.resolvedModules + GetSourceFileForResolvedModule
    module_resolutions: FxHashMap<(usize, String), SymbolId>,
}

pub(crate) struct FileView {
    arena: Rc<NodeArena>,
    /// This file's source text, so the checker can reproduce Go's trivia-skipped
    /// diagnostic spans (see [`BoundProgram::source_text`]).
    text: Rc<str>,
    root: NodeId,
    /// Shared cross-file resolution registry (see [`ViewRegistry`]); lets this
    /// per-file view answer [`view_for_symbol`](BoundProgram::view_for_symbol) /
    /// [`file_view`](BoundProgram::file_view) for sibling files.
    registry: Rc<ViewRegistry>,
    /// The collision-free program file handle (see [`encode_file_handle`]); the
    /// diagnostics partition key, distinct from the raw arena `root`.
    handle: NodeId,
    symbols: Rc<Vec<Symbol>>,
    globals: Rc<SymbolTable>,
    node_symbol: Rc<FxHashMap<NodeId, SymbolId>>,
    locals: Rc<FxHashMap<NodeId, SymbolTable>>,
    node_flow: Rc<FxHashMap<NodeId, FlowNodeId>>,
    flow_nodes: Rc<Vec<FlowNode>>,
    flow_lists: Rc<Vec<FlowList>>,
    flow_switch: Rc<FxHashMap<FlowNodeId, FlowSwitchClauseData>>,
    /// The program's real compiler options, shared by every view so the
    /// checker's option-gated diagnostics read the actual `--target` /
    /// `--downlevelIteration` (see [`BoundProgram::compiler_options`]).
    options: Rc<CompilerOptions>,
    /// True when this file's parse pass recorded syntactic diagnostics.
    has_parse_diagnostics: bool,
}

impl BoundProgram for FileView {
    fn arena(&self) -> &NodeArena {
        &self.arena
    }

    fn root(&self) -> NodeId {
        self.root
    }

    fn file_handle(&self) -> NodeId {
        self.handle
    }

    fn symbol_of_node(&self, node: NodeId) -> Option<SymbolId> {
        self.node_symbol.get(&node).copied()
    }

    fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.index()]
    }

    fn locals(&self, container: NodeId) -> Option<&SymbolTable> {
        self.locals.get(&container)
    }

    fn source_text(&self) -> Option<&str> {
        Some(&self.text)
    }

    fn globals(&self) -> Option<&SymbolTable> {
        Some(&self.globals)
    }

    fn flow_node_of(&self, node: NodeId) -> Option<FlowNodeId> {
        self.node_flow.get(&node).copied()
    }

    fn flow_node(&self, id: FlowNodeId) -> FlowNode {
        self.flow_nodes[id.0 as usize]
    }

    fn flow_list(&self, id: FlowListId) -> FlowList {
        self.flow_lists[id.0 as usize]
    }

    fn flow_switch_clause_data(&self, id: FlowNodeId) -> Option<FlowSwitchClauseData> {
        self.flow_switch.get(&id).copied()
    }

    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn has_parse_diagnostics(&self) -> bool {
        self.has_parse_diagnostics
    }

    fn file_view(&self, file: NodeId) -> Option<Rc<dyn BoundProgram>> {
        let index = (file.0 >> FILE_INDEX_SHIFT) as usize;
        self.registry
            .views
            .borrow()
            .get(index)?
            .upgrade()
            .map(|view| view as Rc<dyn BoundProgram>)
    }

    fn view_for_symbol(&self, symbol: SymbolId) -> Option<Rc<dyn BoundProgram>> {
        let index = self
            .registry
            .ranges
            .iter()
            .position(|&(start, end)| symbol.0 >= start && symbol.0 < end)?;
        self.registry
            .views
            .borrow()
            .get(index)?
            .upgrade()
            .map(|view| view as Rc<dyn BoundProgram>)
    }

    fn resolve_module_symbol(&self, importing_file: NodeId, specifier: &str) -> Option<SymbolId> {
        // The shared registry resolves for any importing file, so the lookup is
        // independent of which view answers it.
        resolve_module_symbol_in(&self.registry, importing_file, specifier)
    }
}

/// The shared `resolve_module_symbol` lookup: maps the `(importing file index,
/// specifier)` of `importing_file` (a file handle) to the resolved module
/// symbol recorded in `registry`.
fn resolve_module_symbol_in(
    registry: &ViewRegistry,
    importing_file: NodeId,
    specifier: &str,
) -> Option<SymbolId> {
    let index = (importing_file.0 >> FILE_INDEX_SHIFT) as usize;
    registry
        .module_resolutions
        .get(&(index, specifier.to_string()))
        .copied()
}

/// A real multi-file bound program: every bound [`ParsedFile`] of the program
/// joined into one program with one *merged* global table and one *merged*
/// symbol space — the production counterpart of the checker's test-only
/// `MultiFileProgram`.
///
/// The checker pool drives it per file via [`BoundProgram::source_files`] +
/// [`BoundProgram::file_view`]; cross-file global type building uses
/// [`BoundProgram::view_for_symbol`].
///
/// Side effects: none (shares the files' arenas / bind results via `Rc`).
// Go: internal/compiler/program.go:Program (the checker's multi-file view)
pub struct MultiFileBoundProgram {
    views: Vec<Rc<FileView>>,
    merged_globals: Rc<SymbolTable>,
    /// `[start, end)` merged-symbol-id range owned by each file, parallel to
    /// `views`, used to map a merged symbol back to its declaring file.
    symbol_ranges: Vec<(u32, u32)>,
    /// The program's real compiler options (see
    /// [`BoundProgram::compiler_options`]).
    options: Rc<CompilerOptions>,
}

impl MultiFileBoundProgram {
    /// Joins every *bound* file of `files` into one multi-file program with
    /// all-defaults compiler options.
    ///
    /// This is the additive, options-free overload kept for callers that do not
    /// carry a program (and for the existing P6 tests); the checker then sees
    /// all-defaults options. To surface the program's REAL options to the
    /// checker's option-gated diagnostics, use [`Self::new_with_options`].
    ///
    /// Side effects: none (offset-merges the files' symbol spaces; shares their
    /// arenas via `Rc`).
    // Go: internal/compiler/program.go:Program (built from processedFiles)
    pub fn new(files: &[ParsedFile]) -> MultiFileBoundProgram {
        MultiFileBoundProgram::new_with_options(files, Rc::new(CompilerOptions::default()))
    }

    /// Joins every *bound* file of `files` into one multi-file program carrying
    /// the program's real `options` (unbound files are skipped — the program
    /// binds every file before building the checker pool, so in practice all are
    /// included).
    ///
    /// The shared `options` are returned by
    /// [`BoundProgram::compiler_options`] (on the program and every per-file
    /// view), so the checker's option-gated diagnostics (e.g. `2802` for-of
    /// downlevel-iteration gating) read the program's actual
    /// `--target`/`--downlevelIteration`/`--strict` config end-to-end.
    ///
    /// Side effects: none (offset-merges the files' symbol spaces; shares their
    /// arenas and options via `Rc`).
    // Go: internal/compiler/program.go:Program (built from processedFiles; c.compilerOptions = program.Options())
    pub fn new_with_options(
        files: &[ParsedFile],
        options: Rc<CompilerOptions>,
    ) -> MultiFileBoundProgram {
        MultiFileBoundProgram::new_with_options_and_modules(files, options, &[])
    }

    /// Like [`Self::new_with_options`], but also builds the specifier →
    /// module-symbol bridge from the program's per-import resolutions
    /// `(containing file name, specifier text, resolved file name)`, so the
    /// checker's `resolveExternalModuleName` can map an `import`/`export`
    /// specifier to the target file's `ValueModule` symbol (and its `exports`).
    ///
    /// A resolution whose containing or resolved file is not among the bound
    /// files (e.g. an unbound file the partial binder skipped) is dropped — the
    /// bridge only links files the checker actually sees.
    ///
    /// Side effects: none (offset-merges the files' symbol spaces; shares their
    /// arenas via `Rc`).
    // Go: internal/compiler/program.go:Program (resolvedModules threaded to the checker)
    pub fn new_with_options_and_modules(
        files: &[ParsedFile],
        options: Rc<CompilerOptions>,
        resolved_modules: &[(String, String, String)],
    ) -> MultiFileBoundProgram {
        // Only bound files contribute (their symbol/flow side maps exist).
        let bound: Vec<&ParsedFile> = files.iter().filter(|f| f.is_bound()).collect();

        // Each file's symbol ids are offset by the running symbol total, so the
        // merged symbol space is collision-free.
        let mut offsets: Vec<u32> = Vec::with_capacity(bound.len());
        let mut running = 0u32;
        for file in &bound {
            offsets.push(running);
            running += file.bind_result().expect("bound").symbols.len() as u32;
        }

        // The merged symbol vector: every file's symbols re-mapped to merged ids.
        let mut merged_symbols: Vec<Symbol> = Vec::with_capacity(running as usize);
        for (i, file) in bound.iter().enumerate() {
            let off = offsets[i];
            for symbol in &file.bind_result().expect("bound").symbols {
                merged_symbols.push(remap_symbol(symbol, off));
            }
        }

        // The merged global table: the union of every file's top-level (root
        // `locals`) declarations, with merged ids (Go's `Checker.globals`).
        //
        // Cross-file DECLARATION MERGING (Go's `mergeGlobalSymbol` ->
        // `mergeSymbol`): when a global name is declared in more than one file
        // and the declarations are *mergeable* (e.g. a global `interface`
        // augmented across lib files — `ObjectConstructor` is declared in
        // `lib.es5.d.ts`, `lib.es2015.core.d.ts`, `lib.es2017.object.d.ts`, ...),
        // the FIRST file's symbol is the merge TARGET and each later same-named
        // symbol's MEMBERS are unioned into it, so a property declared only in a
        // later lib file (`Object.entries`/`Object.values` on `ObjectConstructor`)
        // still resolves. A non-mergeable collision keeps the first symbol (the
        // duplicate-identifier diagnostic is DEFER'd).
        //
        // DEFER(P6): merging the `declarations` list / the `exports` table and
        // the recursive same-named member merge (the rest of Go's `mergeSymbol`).
        // Only the member-table union (+ OR-ed flags) is ported — exactly what
        // cross-file lib-interface property resolution needs. blocked-by: a
        // per-declaration owning-view switch in the declared-type builders (a
        // cross-file declaration node must be read through its own file's arena,
        // not the merge target's) + namespace export merging + `globalThis`.
        let mut merged_globals = SymbolTable::default();
        for (i, file) in bound.iter().enumerate() {
            let off = offsets[i];
            let bind = file.bind_result().expect("bound");
            if let Some(locals) = bind.locals.get(&file.node()) {
                for (name, &sid) in locals {
                    // The CommonJS `module`/`exports` file locals (Go's
                    // `declareCommonJSVariable`, `SymbolFlagsModuleExports`) are
                    // per-file constructs that must NEVER leak into the program
                    // globals — otherwise `module`/`exports` would resolve in
                    // every sibling file (including ES modules and `.ts`),
                    // masking the TS2304/TS2591 tsc reports there. They resolve
                    // only through their own file's `locals` scope walk. Go's
                    // globals merge likewise excludes external/CommonJS module
                    // files entirely (`!IsExternalOrCommonJSModule`).
                    if bind.symbols[sid.index()]
                        .flags
                        .contains(SymbolFlags::MODULE_EXPORTS)
                    {
                        continue;
                    }
                    let source = SymbolId(sid.0 + off);
                    match merged_globals.get(name).copied() {
                        None => {
                            merged_globals.insert(name.clone(), source);
                        }
                        Some(target) => {
                            merge_global_symbol(&mut merged_symbols, target, source);
                        }
                    }
                }
            }
        }
        let merged_symbols = Rc::new(merged_symbols);
        let merged_globals = Rc::new(merged_globals);

        // Per-file merged-symbol ranges (parallel to `views`), used to map a
        // merged symbol back to its declaring file.
        let mut symbol_ranges = Vec::with_capacity(bound.len());
        for (i, file) in bound.iter().enumerate() {
            let off = offsets[i];
            let bind = file.bind_result().expect("bound");
            symbol_ranges.push((off, off + bind.symbols.len() as u32));
        }
        // The specifier -> module-symbol bridge: map each importing file's
        // resolved specifiers to the (merged) `ValueModule` symbol of the target
        // file (its bound `file_symbol`, offset into the merged space). A
        // resolution whose endpoints are not both bound files is dropped.
        let file_index_by_name: FxHashMap<&str, usize> = bound
            .iter()
            .enumerate()
            .map(|(i, file)| (file.file_name(), i))
            .collect();
        let module_symbol_of = |index: usize| -> Option<SymbolId> {
            bound[index]
                .bind_result()
                .expect("bound")
                .file_symbol
                .map(|s| SymbolId(s.0 + offsets[index]))
        };
        let mut module_resolutions: FxHashMap<(usize, String), SymbolId> = FxHashMap::default();
        for (containing, specifier, resolved) in resolved_modules {
            let (Some(&importing_idx), Some(&resolved_idx)) = (
                file_index_by_name.get(containing.as_str()),
                file_index_by_name.get(resolved.as_str()),
            ) else {
                continue;
            };
            if let Some(module_symbol) = module_symbol_of(resolved_idx) {
                module_resolutions.insert((importing_idx, specifier.clone()), module_symbol);
            }
        }

        // The shared cross-file resolution registry: ranges are known now; the
        // `views` weak back-pointers are filled after the views are built.
        let registry = Rc::new(ViewRegistry {
            ranges: symbol_ranges.clone(),
            views: RefCell::new(Vec::with_capacity(bound.len())),
            module_resolutions,
        });

        let mut views = Vec::with_capacity(bound.len());
        for (i, file) in bound.iter().enumerate() {
            let off = offsets[i];
            let bind = file.bind_result().expect("bound");
            let root = file.node();
            let node_symbol: FxHashMap<NodeId, SymbolId> = bind
                .node_symbol
                .iter()
                .map(|(&node, &sid)| (node, SymbolId(sid.0 + off)))
                .collect();
            let locals: FxHashMap<NodeId, SymbolTable> = bind
                .locals
                .iter()
                .map(|(&container, table)| (container, remap_table(table, off)))
                .collect();
            views.push(Rc::new(FileView {
                arena: file.arena_rc(),
                text: Rc::from(file.text()),
                root,
                registry: Rc::clone(&registry),
                handle: encode_file_handle(i, root),
                symbols: Rc::clone(&merged_symbols),
                globals: Rc::clone(&merged_globals),
                node_symbol: Rc::new(node_symbol),
                locals: Rc::new(locals),
                node_flow: Rc::new(bind.node_flow.clone()),
                flow_nodes: Rc::new(bind.flow_nodes.clone()),
                flow_lists: Rc::new(bind.flow_lists.clone()),
                flow_switch: Rc::new(bind.flow_switch_data.clone()),
                options: Rc::clone(&options),
                has_parse_diagnostics: !file.diagnostics().is_empty(),
            }));
        }

        // Fill the registry's weak back-pointers now that every view exists, so
        // a per-file view can resolve a sibling file's view (`view_for_symbol` /
        // `file_view`). Weak avoids a `FileView -> registry -> FileView` cycle.
        *registry.views.borrow_mut() = views.iter().map(Rc::downgrade).collect();

        MultiFileBoundProgram {
            views,
            merged_globals,
            symbol_ranges,
            options,
        }
    }
}

/// Folds a file index and its raw root node id into a collision-free file
/// handle (see [`FILE_INDEX_SHIFT`]).
fn encode_file_handle(index: usize, raw_root: NodeId) -> NodeId {
    NodeId(((index as u32) << FILE_INDEX_SHIFT) | raw_root.0)
}

impl BoundProgram for MultiFileBoundProgram {
    fn arena(&self) -> &NodeArena {
        // A degenerate single-file accessor (delegates to the first file); the
        // checker reaches a specific file's arena via `file_view`.
        self.views[0].arena()
    }

    fn root(&self) -> NodeId {
        self.views[0].root()
    }

    fn symbol_of_node(&self, node: NodeId) -> Option<SymbolId> {
        self.views[0].symbol_of_node(node)
    }

    fn symbol(&self, id: SymbolId) -> &Symbol {
        // The symbol vector is shared (merged), so any view resolves any id.
        self.views[0].symbol(id)
    }

    fn locals(&self, container: NodeId) -> Option<&SymbolTable> {
        self.views[0].locals(container)
    }

    fn source_text(&self) -> Option<&str> {
        // Degenerate single-file accessor; the checker reaches a specific file's
        // text via `file_view(...).source_text()`.
        self.views[0].source_text()
    }

    fn globals(&self) -> Option<&SymbolTable> {
        Some(&self.merged_globals)
    }

    fn flow_node_of(&self, node: NodeId) -> Option<FlowNodeId> {
        self.views[0].flow_node_of(node)
    }

    fn flow_node(&self, id: FlowNodeId) -> FlowNode {
        self.views[0].flow_node(id)
    }

    fn flow_list(&self, id: FlowListId) -> FlowList {
        self.views[0].flow_list(id)
    }

    fn source_files(&self) -> Vec<NodeId> {
        self.views.iter().map(|view| view.file_handle()).collect()
    }

    fn file_view(&self, file: NodeId) -> Option<Rc<dyn BoundProgram>> {
        let index = (file.0 >> FILE_INDEX_SHIFT) as usize;
        self.views
            .get(index)
            .map(|view| Rc::clone(view) as Rc<dyn BoundProgram>)
    }

    fn view_for_symbol(&self, symbol: SymbolId) -> Option<Rc<dyn BoundProgram>> {
        let index = self
            .symbol_ranges
            .iter()
            .position(|&(start, end)| symbol.0 >= start && symbol.0 < end)?;
        Some(Rc::clone(&self.views[index]) as Rc<dyn BoundProgram>)
    }

    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn resolve_module_symbol(&self, importing_file: NodeId, specifier: &str) -> Option<SymbolId> {
        // The registry is shared by every view; any view answers for any
        // importing file (the lookup keys on the file index in the handle).
        self.views
            .first()
            .and_then(|view| resolve_module_symbol_in(&view.registry, importing_file, specifier))
    }
}

/// Re-maps a symbol's id-bearing fields into a merged symbol space by `offset`.
///
/// Symbol-id fields (`members`/`exports` table values, `parent`,
/// `export_symbol`) are shifted; declaration *node* ids stay file-local (they
/// are only ever read through the owning file's arena).
/// Merges a later same-named global symbol's MEMBERS into the first-encountered
/// (`target`) symbol — the member-table half of Go's `mergeSymbol`
/// (`target.Flags |= source.Flags` + `mergeSymbolTable(target.Members,
/// source.Members)`). A global `interface` augmented across files (e.g.
/// `ObjectConstructor` declared in the es5/es2015/es2017 lib files) thus exposes
/// every declaration's members, so `Object.entries`/`Object.values` resolve.
///
/// The merge is gated by the same mergeability test Go uses
/// ([`excluded_symbol_flags`]): two interfaces merge, but e.g. an interface and a
/// block-scoped `let` do not. A non-mergeable collision is left as first-wins
/// (the duplicate-identifier diagnostic is DEFER'd, as before). Member names
/// already on the target win (the target is the base declaration); a member
/// present only on `source` is added (Go's `mergeSymbolTable` inserts the
/// not-yet-present source members and recursively merges colliding ones — the
/// recursive same-named member merge is DEFER'd here).
///
/// Only the member table and the OR-ed flags are merged — NOT `declarations` or
/// `exports` (see the globals-merge note for the blocked-by).
// Go: internal/checker/checker.go:Checker.mergeSymbol (Flags |= ; mergeSymbolTable(Members))
fn merge_global_symbol(symbols: &mut [Symbol], target: SymbolId, source: SymbolId) {
    if target == source {
        return;
    }
    let target_flags = symbols[target.index()].flags;
    let source_flags = symbols[source.index()].flags;
    // Go: `target.Flags & getExcludedSymbolFlags(source.Flags) == 0 ||
    //      (source.Flags | target.Flags) & SymbolFlagsAssignment != 0`
    let mergeable = (target_flags & excluded_symbol_flags(source_flags)) == SymbolFlags::NONE
        || (source_flags | target_flags).contains(SymbolFlags::ASSIGNMENT);
    if !mergeable {
        return;
    }
    // The source and target both live in `symbols`, so snapshot the source's
    // (already merge-id-remapped) members before mutating the target.
    let source_members: Vec<(String, SymbolId)> = symbols[source.index()]
        .members
        .iter()
        .map(|(name, &member)| (name.clone(), member))
        .collect();
    let target_sym = &mut symbols[target.index()];
    target_sym.flags |= source_flags;
    for (name, member) in source_members {
        target_sym.members.entry(name).or_insert(member);
    }
}

/// Ports Go's `getExcludedSymbolFlags`: the symbol-flag bits a declaration with
/// `flags` cannot merge with. Used to gate cross-file global declaration merging
/// (two `interface`s merge; an `interface` and a `let` do not).
// Go: internal/checker/checker.go:getExcludedSymbolFlags
fn excluded_symbol_flags(flags: SymbolFlags) -> SymbolFlags {
    let mut result = SymbolFlags::NONE;
    if flags.contains(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
        result |= SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES;
    }
    if flags.contains(SymbolFlags::FUNCTION_SCOPED_VARIABLE) {
        result |= SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES;
    }
    if flags.contains(SymbolFlags::PROPERTY) {
        result |= SymbolFlags::PROPERTY_EXCLUDES;
    }
    if flags.contains(SymbolFlags::ENUM_MEMBER) {
        result |= SymbolFlags::ENUM_MEMBER_EXCLUDES;
    }
    if flags.contains(SymbolFlags::FUNCTION) {
        result |= SymbolFlags::FUNCTION_EXCLUDES;
    }
    if flags.contains(SymbolFlags::CLASS) {
        result |= SymbolFlags::CLASS_EXCLUDES;
    }
    if flags.contains(SymbolFlags::INTERFACE) {
        result |= SymbolFlags::INTERFACE_EXCLUDES;
    }
    if flags.contains(SymbolFlags::REGULAR_ENUM) {
        result |= SymbolFlags::REGULAR_ENUM_EXCLUDES;
    }
    if flags.contains(SymbolFlags::CONST_ENUM) {
        result |= SymbolFlags::CONST_ENUM_EXCLUDES;
    }
    if flags.contains(SymbolFlags::VALUE_MODULE) {
        result |= SymbolFlags::VALUE_MODULE_EXCLUDES;
    }
    if flags.contains(SymbolFlags::METHOD) {
        result |= SymbolFlags::METHOD_EXCLUDES;
    }
    if flags.contains(SymbolFlags::GET_ACCESSOR) {
        result |= SymbolFlags::GET_ACCESSOR_EXCLUDES;
    }
    if flags.contains(SymbolFlags::SET_ACCESSOR) {
        result |= SymbolFlags::SET_ACCESSOR_EXCLUDES;
    }
    if flags.contains(SymbolFlags::TYPE_PARAMETER) {
        result |= SymbolFlags::TYPE_PARAMETER_EXCLUDES;
    }
    if flags.contains(SymbolFlags::TYPE_ALIAS) {
        result |= SymbolFlags::TYPE_ALIAS_EXCLUDES;
    }
    if flags.contains(SymbolFlags::ALIAS) {
        result |= SymbolFlags::ALIAS_EXCLUDES;
    }
    result
}

fn remap_symbol(symbol: &Symbol, offset: u32) -> Symbol {
    Symbol {
        flags: symbol.flags,
        check_flags: symbol.check_flags,
        name: symbol.name.clone(),
        declarations: symbol.declarations.clone(),
        value_declaration: symbol.value_declaration,
        members: remap_table(&symbol.members, offset),
        exports: remap_table(&symbol.exports, offset),
        parent: symbol.parent.map(|p| SymbolId(p.0 + offset)),
        export_symbol: symbol.export_symbol.map(|p| SymbolId(p.0 + offset)),
    }
}

/// Re-maps a symbol table's values into the merged symbol space by `offset`.
fn remap_table(table: &SymbolTable, offset: u32) -> SymbolTable {
    table
        .iter()
        .map(|(name, &sid)| (name.clone(), SymbolId(sid.0 + offset)))
        .collect()
}

#[cfg(test)]
#[path = "multifile_test.rs"]
mod tests;
