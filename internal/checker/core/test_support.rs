//! Test-only stub program: parse a single source string with `tsgo_parser` and
//! bind it with `tsgo_binder`, exposing the result through [`BoundProgram`].
//!
//! This is the minimal in-memory stand-in for the real multi-file
//! `compiler.Program` (Phase 6), used to drive the 4b symbol-query tests
//! (notably the port of Go's `TestGetSymbolAtLocation`). It lives behind
//! `cfg(test)`, so `tsgo_parser`/`tsgo_binder` are dev-dependencies only.

use std::rc::Rc;

use rustc_hash::FxHashMap;
use tsgo_ast::flow::{FlowList, FlowListId, FlowNode, FlowNodeId, FlowSwitchClauseData};
use tsgo_ast::{NodeArena, NodeId, Symbol, SymbolId, SymbolTable};
use tsgo_binder::{bind_source_file, BindResult};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

use super::program::BoundProgram;

/// A parsed-and-bound single source file.
pub(crate) struct StubProgram {
    arena: NodeArena,
    source_file: NodeId,
    bind: tsgo_binder::BindResult,
    options: CompilerOptions,
}

impl StubProgram {
    /// Parses `text` as a `.ts` file named `file_name` and binds it (with
    /// all-defaults compiler options).
    pub(crate) fn parse_and_bind(file_name: &str, text: &str) -> StubProgram {
        StubProgram::parse_and_bind_with(
            file_name,
            text,
            ScriptKind::Ts,
            CompilerOptions::default(),
        )
    }

    /// Parses `text` as a `.tsx` file (JSX enabled) named `file_name` and binds it.
    pub(crate) fn parse_and_bind_tsx(file_name: &str, text: &str) -> StubProgram {
        StubProgram::parse_and_bind_with(
            file_name,
            text,
            ScriptKind::Tsx,
            CompilerOptions::default(),
        )
    }

    /// Parses `text` as a `.ts` file named `file_name`, binds it, and carries
    /// `options` so option-gated checker behavior (e.g. `strictNullChecks`, the
    /// `--target`/`--downlevelIteration` iteration gating) can be driven from a
    /// test (the test stand-in for `program.Options()`).
    pub(crate) fn parse_and_bind_with_options(
        file_name: &str,
        text: &str,
        options: CompilerOptions,
    ) -> StubProgram {
        StubProgram::parse_and_bind_with(file_name, text, ScriptKind::Ts, options)
    }

    fn parse_and_bind_with(
        file_name: &str,
        text: &str,
        script_kind: ScriptKind,
        options: CompilerOptions,
    ) -> StubProgram {
        let opts = SourceFileParseOptions {
            file_name: file_name.to_string(),
        };
        let mut parsed = parse_source_file(opts, text, script_kind);
        let bind = bind_source_file(&mut parsed.arena, parsed.source_file);
        StubProgram {
            arena: parsed.arena,
            source_file: parsed.source_file,
            bind,
            options,
        }
    }
}

impl BoundProgram for StubProgram {
    fn arena(&self) -> &NodeArena {
        &self.arena
    }

    fn root(&self) -> NodeId {
        self.source_file
    }

    fn symbol_of_node(&self, node: NodeId) -> Option<SymbolId> {
        self.bind.node_symbol.get(&node).copied()
    }

    fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.bind.symbols[id.index()]
    }

    fn locals(&self, container: NodeId) -> Option<&SymbolTable> {
        self.bind.locals.get(&container)
    }

    fn globals(&self) -> Option<&SymbolTable> {
        // For a script (non-module) source file the top-level `locals` are the
        // program's globals (Go merges every global file's `Locals` into
        // `c.globals`). A single bound file is the synthetic-globals stand-in
        // until real cross-file merge + lib.d.ts loading land in P6.
        self.bind.locals.get(&self.source_file)
    }

    fn flow_node_of(&self, node: NodeId) -> Option<FlowNodeId> {
        self.bind.node_flow.get(&node).copied()
    }

    fn flow_node(&self, id: FlowNodeId) -> FlowNode {
        self.bind.flow_nodes[id.0 as usize]
    }

    fn flow_list(&self, id: FlowListId) -> FlowList {
        self.bind.flow_lists[id.0 as usize]
    }

    fn flow_switch_clause_data(&self, id: FlowNodeId) -> Option<FlowSwitchClauseData> {
        self.bind.flow_switch_data.get(&id).copied()
    }

    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }
}

/// The high-bit shift used to fold a file index into a multi-file source-file
/// handle, so two separately-parsed files (whose raw arena ids both start at 0)
/// never collide as program file handles. The low 24 bits hold the file's raw
/// root node id; the upper bits hold the file index.
///
/// This caps a file at `2^24` nodes, which is far beyond any test input.
const FILE_INDEX_SHIFT: u32 = 24;

/// An owned, shareable single-file [`BoundProgram`] view into one file of a
/// [`MultiFileProgram`].
///
/// Its [`arena`](BoundProgram::arena) is *this* file's node arena (so its
/// file-local node ids resolve), while [`symbol`](BoundProgram::symbol) indexes
/// the program-wide *merged* symbol vector and [`globals`](BoundProgram::globals)
/// is the program-wide *merged* global table — so a reference into another
/// file's globals (e.g. the lib file's `String`) resolves while checking this
/// file. Every symbol id this view hands out (via `symbol_of_node`/`locals`/
/// `globals`) is already a merged (file-offset) id, so the checker's per-symbol
/// caches never collide across files.
pub(crate) struct FileView {
    arena: Rc<NodeArena>,
    root: NodeId,
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
}

/// A test multi-file bound program: several parsed-and-bound source files joined
/// into one program with a single *merged* global table and a single *merged*
/// symbol space, the in-memory stand-in for the real multi-file
/// `compiler.Program` (Phase 6).
///
/// Because the parser mints a fresh arena (and fresh, 0-based symbol ids) per
/// file, the files keep *separate node arenas* but share *one offset-merged
/// symbol vector*: file `i`'s symbol ids are shifted by the sum of the previous
/// files' symbol counts, and every symbol-id-bearing field (members/exports,
/// `parent`, `export_symbol`, the `locals`/`node_symbol` maps, and the merged
/// `globals`) is rewritten to that merged id. Node ids stay file-local and are
/// reached only through the owning file's [`FileView::arena`].
///
/// The checker drives it per file via [`BoundProgram::source_files`] +
/// [`BoundProgram::file_view`]; cross-file global type building uses
/// [`BoundProgram::view_for_symbol`].
pub(crate) struct MultiFileProgram {
    views: Vec<Rc<FileView>>,
    merged_globals: Rc<SymbolTable>,
    /// `[start, end)` merged-symbol-id range owned by each file, parallel to
    /// `views`, used to map a merged symbol back to its declaring file.
    symbol_ranges: Vec<(u32, u32)>,
}

impl MultiFileProgram {
    /// Parses and binds each `(file_name, text)` and joins them into one
    /// multi-file program (all files as global/script `.ts` files).
    pub(crate) fn build(files: &[(&str, &str)]) -> MultiFileProgram {
        let parsed: Vec<(NodeArena, NodeId, BindResult)> = files
            .iter()
            .map(|(name, text)| {
                let opts = SourceFileParseOptions {
                    file_name: name.to_string(),
                };
                let mut p = parse_source_file(opts, text, ScriptKind::Ts);
                let bind = bind_source_file(&mut p.arena, p.source_file);
                (p.arena, p.source_file, bind)
            })
            .collect();

        // Each file's symbol ids are offset by the running symbol total, so the
        // merged symbol space is collision-free.
        let mut offsets: Vec<u32> = Vec::with_capacity(parsed.len());
        let mut running = 0u32;
        for (_, _, bind) in &parsed {
            offsets.push(running);
            running += bind.symbols.len() as u32;
        }

        // The merged symbol vector: every file's symbols re-mapped to merged ids.
        let mut merged_symbols: Vec<Symbol> = Vec::with_capacity(running as usize);
        for (i, (_, _, bind)) in parsed.iter().enumerate() {
            let off = offsets[i];
            for symbol in &bind.symbols {
                merged_symbols.push(remap_symbol(symbol, off));
            }
        }
        let merged_symbols = Rc::new(merged_symbols);

        // The merged global table: the union of every file's top-level (root
        // `locals`) declarations, with merged ids (Go's `Checker.globals`).
        // DEFER(phase-4-checker-4ab): cross-file symbol MERGE for same-named
        // declarations (declaration merging) — first file wins here.
        // blocked-by: `mergeSymbol`/`mergeSymbolTable` (the binder's cross-file
        // merge) + `globalThis`.
        let mut merged_globals = SymbolTable::default();
        for (i, (_, root, bind)) in parsed.iter().enumerate() {
            let off = offsets[i];
            if let Some(locals) = bind.locals.get(root) {
                for (name, &sid) in locals {
                    merged_globals
                        .entry(name.clone())
                        .or_insert(SymbolId(sid.0 + off));
                }
            }
        }
        let merged_globals = Rc::new(merged_globals);

        let mut views = Vec::with_capacity(parsed.len());
        let mut symbol_ranges = Vec::with_capacity(parsed.len());
        for (i, (arena, root, bind)) in parsed.into_iter().enumerate() {
            let off = offsets[i];
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
            symbol_ranges.push((off, off + bind.symbols.len() as u32));
            views.push(Rc::new(FileView {
                arena: Rc::new(arena),
                root,
                handle: encode_file_handle(i, root),
                symbols: Rc::clone(&merged_symbols),
                globals: Rc::clone(&merged_globals),
                node_symbol: Rc::new(node_symbol),
                locals: Rc::new(locals),
                node_flow: Rc::new(bind.node_flow),
                flow_nodes: Rc::new(bind.flow_nodes),
                flow_lists: Rc::new(bind.flow_lists),
                flow_switch: Rc::new(bind.flow_switch_data),
            }));
        }
        MultiFileProgram {
            views,
            merged_globals,
            symbol_ranges,
        }
    }
}

/// Folds a file index and its raw root node id into a collision-free file
/// handle (see [`FILE_INDEX_SHIFT`]).
fn encode_file_handle(index: usize, raw_root: NodeId) -> NodeId {
    NodeId(((index as u32) << FILE_INDEX_SHIFT) | raw_root.0)
}

impl BoundProgram for MultiFileProgram {
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
}

/// Re-maps a symbol's id-bearing fields into a merged symbol space by `offset`.
///
/// Symbol-id fields (`members`/`exports` table values, `parent`,
/// `export_symbol`) are shifted; declaration *node* ids stay file-local (they
/// are only ever read through the owning file's arena).
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
