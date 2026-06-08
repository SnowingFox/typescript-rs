//! Test-only stub program: parse a single source string with `tsgo_parser` and
//! bind it with `tsgo_binder`, exposing the result through [`BoundProgram`].
//!
//! This is the minimal in-memory stand-in for the real multi-file
//! `compiler.Program` (Phase 6), used to drive the 4b symbol-query tests
//! (notably the port of Go's `TestGetSymbolAtLocation`). It lives behind
//! `cfg(test)`, so `tsgo_parser`/`tsgo_binder` are dev-dependencies only.

use std::rc::Rc;
use std::sync::OnceLock;

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
    text: String,
    has_parse_diagnostics: bool,
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

    /// Parses `text` as a `.tsx` file (JSX enabled) named `file_name`, binds it,
    /// and carries `options` so option-gated JSX behavior (e.g. `noImplicitAny`
    /// gating TS7026) can be driven from a test.
    pub(crate) fn parse_and_bind_tsx_with_options(
        file_name: &str,
        text: &str,
        options: CompilerOptions,
    ) -> StubProgram {
        StubProgram::parse_and_bind_with(file_name, text, ScriptKind::Tsx, options)
    }

    /// Parses `text` as a `.js` file named `file_name` and binds it, so the
    /// parser marks every node with `NodeFlags::JAVA_SCRIPT_FILE` (the
    /// JS-file context the `require(...)` resolution path keys off).
    pub(crate) fn parse_and_bind_js(file_name: &str, text: &str) -> StubProgram {
        StubProgram::parse_and_bind_with(
            file_name,
            text,
            ScriptKind::Js,
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
        let has_parse_diagnostics = !parsed.diagnostics.is_empty();
        let bind = bind_source_file(&mut parsed.arena, parsed.source_file);
        StubProgram {
            arena: parsed.arena,
            source_file: parsed.source_file,
            bind,
            options,
            text: text.to_string(),
            has_parse_diagnostics,
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

    fn source_text(&self) -> Option<&str> {
        Some(&self.text)
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

    fn has_parse_diagnostics(&self) -> bool {
        self.has_parse_diagnostics
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
    file_name: String,
    file_symbol: Option<SymbolId>,
    file_index: usize,
    all_file_names: Rc<Vec<String>>,
    all_file_symbols: Rc<Vec<Option<SymbolId>>>,
    symbols: Rc<Vec<Symbol>>,
    globals: Rc<SymbolTable>,
    node_symbol: Rc<FxHashMap<NodeId, SymbolId>>,
    locals: Rc<FxHashMap<NodeId, SymbolTable>>,
    node_flow: Rc<FxHashMap<NodeId, FlowNodeId>>,
    flow_nodes: Rc<Vec<FlowNode>>,
    flow_lists: Rc<Vec<FlowList>>,
    flow_switch: Rc<FxHashMap<FlowNodeId, FlowSwitchClauseData>>,
    /// Every file view in a multi-file program (filled after all views are built).
    all_views: Rc<OnceLock<Rc<Vec<Rc<FileView>>>>>,
    /// Merged-symbol id ranges per file (parallel to `all_views`), for
    /// [`BoundProgram::view_for_symbol`].
    symbol_ranges: Rc<Vec<(u32, u32)>>,
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

    fn resolve_module_symbol(&self, _importing_file: NodeId, specifier: &str) -> Option<SymbolId> {
        resolve_test_module_symbol(
            &self.file_name,
            specifier,
            &self.all_file_names,
            &self.all_file_symbols,
        )
    }

    fn source_files(&self) -> Vec<NodeId> {
        self.all_views
            .get()
            .map(|views| views.iter().map(|view| view.handle).collect())
            .unwrap_or_else(|| vec![self.handle])
    }

    fn file_view(&self, file: NodeId) -> Option<Rc<dyn BoundProgram>> {
        let index = (file.0 >> FILE_INDEX_SHIFT) as usize;
        self.all_views
            .get()
            .and_then(|views| views.get(index).map(|view| Rc::clone(view) as Rc<dyn BoundProgram>))
    }

    fn view_for_symbol(&self, symbol: SymbolId) -> Option<Rc<dyn BoundProgram>> {
        let index = self
            .symbol_ranges
            .iter()
            .position(|&(start, end)| symbol.0 >= start && symbol.0 < end)?;
        self.all_views.get().and_then(|views| {
            views
                .get(index)
                .map(|view| Rc::clone(view) as Rc<dyn BoundProgram>)
        })
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
    file_names: Vec<String>,
    file_symbols: Vec<Option<SymbolId>>,
    /// `[start, end)` merged-symbol-id range owned by each file, parallel to
    /// `views`, used to map a merged symbol back to its declaring file.
    symbol_ranges: Vec<(u32, u32)>,
}

impl MultiFileProgram {
    /// Parses and binds each `(file_name, text)` and joins them into one
    /// multi-file program (all files as global/script `.ts` files).
    pub(crate) fn build(files: &[(&str, &str)]) -> MultiFileProgram {
        let file_names: Vec<String> = files.iter().map(|(name, _)| (*name).to_string()).collect();
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

        // The merged global table: union of every file's top-level locals, with
        // same-named globals merged so declaration lists accumulate (enough for
        // cross-file clodule / namespace-merge diagnostics like TS2433).
        let mut merged_globals = SymbolTable::default();
        let mut symbol_redirect: FxHashMap<SymbolId, SymbolId> = FxHashMap::default();
        for (i, (_, root, bind)) in parsed.iter().enumerate() {
            let off = offsets[i];
            if let Some(locals) = bind.locals.get(root) {
                for (name, &sid) in locals {
                    let merged_sid = SymbolId(sid.0 + off);
                    match merged_globals.entry(name.clone()) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(merged_sid);
                        }
                        std::collections::hash_map::Entry::Occupied(entry) => {
                            let canonical = *entry.get();
                            let source = merged_symbols[merged_sid.0 as usize].clone();
                            merge_symbols(&mut merged_symbols[canonical.0 as usize], &source);
                            symbol_redirect.insert(merged_sid, canonical);
                        }
                    }
                }
            }
        }
        let merged_symbols = Rc::new(merged_symbols);
        let merged_globals = Rc::new(merged_globals);

        let mut file_symbols: Vec<Option<SymbolId>> = Vec::with_capacity(parsed.len());
        for (i, (_, _, bind)) in parsed.iter().enumerate() {
            let off = offsets[i];
            file_symbols.push(
                bind.file_symbol
                    .map(|s| SymbolId(s.0 + off)),
            );
        }
        let all_file_names = Rc::new(file_names.clone());
        let all_file_symbols = Rc::new(file_symbols.clone());
        let all_views_slot: Rc<OnceLock<Rc<Vec<Rc<FileView>>>>> = Rc::new(OnceLock::new());
        let mut views = Vec::with_capacity(parsed.len());
        let mut symbol_range_vec = Vec::with_capacity(parsed.len());
        for (i, (arena, root, bind)) in parsed.into_iter().enumerate() {
            let off = offsets[i];
            let node_symbol: FxHashMap<NodeId, SymbolId> = bind
                .node_symbol
                .iter()
                .map(|(&node, &sid)| {
                    let mut merged = SymbolId(sid.0 + off);
                    if let Some(&canonical) = symbol_redirect.get(&merged) {
                        merged = canonical;
                    }
                    (node, merged)
                })
                .collect();
            let locals: FxHashMap<NodeId, SymbolTable> = bind
                .locals
                .iter()
                .map(|(&container, table)| (container, remap_table(table, off)))
                .collect();
            symbol_range_vec.push((off, off + bind.symbols.len() as u32));
            views.push(Rc::new(FileView {
                arena: Rc::new(arena),
                root,
                handle: encode_file_handle(i, root),
                file_name: file_names[i].clone(),
                file_symbol: file_symbols[i],
                file_index: i,
                all_file_names: Rc::clone(&all_file_names),
                all_file_symbols: Rc::clone(&all_file_symbols),
                symbols: Rc::clone(&merged_symbols),
                globals: Rc::clone(&merged_globals),
                node_symbol: Rc::new(node_symbol),
                locals: Rc::new(locals),
                node_flow: Rc::new(bind.node_flow),
                flow_nodes: Rc::new(bind.flow_nodes),
                flow_lists: Rc::new(bind.flow_lists),
                flow_switch: Rc::new(bind.flow_switch_data),
                all_views: Rc::clone(&all_views_slot),
                symbol_ranges: Rc::new(symbol_range_vec.clone()),
            }));
        }
        let _ = all_views_slot.set(Rc::new(views.clone()));
        let symbol_ranges = symbol_range_vec;
        MultiFileProgram {
            views,
            merged_globals,
            file_names,
            file_symbols,
            symbol_ranges,
        }
    }
}

fn normalize_module_base(path: &str) -> &str {
    path.strip_suffix(".d.ts")
        .or_else(|| path.strip_suffix(".tsx"))
        .or_else(|| path.strip_suffix(".ts"))
        .unwrap_or(path)
}

fn resolve_relative_module_path(from: &str, specifier: &str) -> String {
    if let Some(rest) = specifier.strip_prefix("./") {
        let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if dir.is_empty() {
            format!("/{rest}")
        } else {
            format!("{dir}/{rest}")
        }
    } else if let Some(rest) = specifier.strip_prefix("../") {
        let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let parent = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if parent.is_empty() {
            format!("/{rest}")
        } else {
            format!("{parent}/{rest}")
        }
    } else {
        specifier.to_string()
    }
}

#[cfg(test)]
mod multi_file_tests {
    use super::*;
    use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_EXPORT_EQUALS;
    use tsgo_ast::SymbolFlags;

    #[test]
    fn export_equals_is_recorded_on_module_exports() {
        let p = MultiFileProgram::build(&[("/foo.ts", "function foo(): void {}\nexport = foo;")]);
        let module_sym = p.file_symbols[0].expect("file module symbol");
        assert!(
            p.views[0]
                .symbol(module_sym)
                .exports
                .contains_key(INTERNAL_SYMBOL_NAME_EXPORT_EQUALS),
            "exports: {:?}",
            p.views[0].symbol(module_sym).exports.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn namespace_export_resolves_from_importer_context() {
        use crate::core::declared_types::resolve_external_module_symbol;
        use crate::Checker;
        let p = std::rc::Rc::new(MultiFileProgram::build(&[
            ("/foo.ts", "function foo(): void {}\nexport = foo;"),
            ("/index.ts", "import * as ns from \"./foo\";"),
        ]));
        let index = p.source_files()[1];
        let view = p.file_view(index).unwrap();
        let module_sym = view
            .resolve_module_symbol(index, "./foo")
            .expect("module symbol");
        let mut c = Checker::new_checker(std::rc::Rc::clone(&p) as std::rc::Rc<dyn BoundProgram>);
        let resolved = resolve_external_module_symbol(&mut c, view.as_ref(), module_sym);
        assert!(
            c.resolved_symbol_flags(view.as_ref(), resolved).intersects(SymbolFlags::FUNCTION),
            "from importer context flags={:?}",
            c.resolved_symbol_flags(view.as_ref(), resolved)
        );
    }

    #[test]
    fn export_equals_alias_resolves_to_exported_value() {
        use crate::core::declared_types::resolve_external_module_symbol;
        use crate::Checker;
        let p = std::rc::Rc::new(MultiFileProgram::build(&[(
            "/foo.ts",
            "function foo(): void {}\nexport = foo;",
        )]));
        let module_sym = p.file_symbols[0].expect("file module symbol");
        let mut c = Checker::new_checker(std::rc::Rc::clone(&p) as std::rc::Rc<dyn BoundProgram>);
        let resolved = resolve_external_module_symbol(&mut c, p.as_ref(), module_sym);
        assert!(
            c.resolved_symbol_flags(p.as_ref(), resolved)
                .intersects(SymbolFlags::FUNCTION),
            "export= must resolve to the function, got flags on resolved={resolved:?}"
        );
    }

    #[test]
    fn resolve_module_symbol_finds_sibling_file() {
        let p = MultiFileProgram::build(&[
            ("/foo.ts", "function foo(): void {}\nexport = foo;"),
            ("/index.ts", "import * as ns from \"./foo\";"),
        ]);
        let index = p.source_files()[1];
        let view = p.file_view(index).unwrap();
        assert!(
            view.resolve_module_symbol(index, "./foo").is_some(),
            "module resolution must find /foo.ts"
        );
    }
}

fn resolve_test_module_symbol(
    importing_file_name: &str,
    specifier: &str,
    file_names: &[String],
    file_symbols: &[Option<SymbolId>],
) -> Option<SymbolId> {
    let resolved = resolve_relative_module_path(importing_file_name, specifier);
    let resolved_base = normalize_module_base(&resolved);
    file_names.iter().position(|name| {
        normalize_module_base(name) == resolved_base || name == &resolved
    })
    .and_then(|index| file_symbols[index])
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

    fn resolve_module_symbol(&self, importing_file: NodeId, specifier: &str) -> Option<SymbolId> {
        let index = (importing_file.0 >> FILE_INDEX_SHIFT) as usize;
        let importing_name = self.file_names.get(index)?;
        resolve_test_module_symbol(
            importing_name,
            specifier,
            &self.file_names,
            &self.file_symbols,
        )
    }
}

/// Merges `source` into `target` for cross-file global declaration merging.
fn merge_symbols(target: &mut Symbol, source: &Symbol) {
    target.flags |= source.flags;
    for &decl in &source.declarations {
        if !target.declarations.contains(&decl) {
            target.declarations.push(decl);
        }
    }
    if target.value_declaration.is_none() {
        target.value_declaration = source.value_declaration;
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
