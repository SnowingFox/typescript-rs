//! `tsgo_binder` — 1:1 Rust port of Go `internal/binder`.
//!
//! The binder walks a parsed [`SourceFile`](tsgo_ast::NodeData::SourceFile),
//! creates [`Symbol`]s and symbol tables (`locals` / `exports` / `members`),
//! declares and merges symbols (reporting conflicts), tracks container and
//! block scopes, and builds the control-flow graph (`FlowNode`s) used by the
//! checker for narrowing.
//!
//! # Ownership model (read this first)
//!
//! Go's binder mutates `ast.Node`/`ast.SourceFile` in place (setting
//! `node.Symbol`, `node.FlowNode`, container `locals`, ...). The Rust `ast`
//! crate keeps nodes immutable apart from flags/parent, so the binder owns a
//! second graph alongside the node arena:
//!
//! - `symbols: Vec<Symbol>` indexed by [`SymbolId`]; each [`Symbol`] owns its
//!   `members`/`exports` tables.
//! - `flow_nodes`/`flow_lists` arenas indexed by [`FlowNodeId`]/`FlowListId`.
//! - Side maps replace per-node fields: `node_symbol` (`node.Symbol`),
//!   `node_local_symbol` (`node.LocalSymbol`), `node_flow` (`node.FlowNode`),
//!   and `locals` (a container's `locals` table).
//!
//! All cross-graph references are arena indices, never Rust references, so the
//! cyclic/aliased/mutable binder graph stays 100% safe Rust (zero `unsafe`).
//! This is a deliberate, structure-preserving deviation from Go's pointer
//! syntax (`b.declareSymbol(GetExports(sym), ...)` becomes
//! `self.declare_symbol(TableLoc::Exports(sym), ...)`).

mod astquery;
mod flow;
pub mod nameresolver;
pub mod referenceresolver;
mod symbols;

use rustc_hash::{FxHashMap, FxHashSet};
use tsgo_ast::flow::{
    FlowFlags, FlowList, FlowNode, FlowNodeId, FlowReduceLabelData, FlowSwitchClauseData,
};
use tsgo_ast::{
    Kind, NodeArena, NodeData, NodeFlags, NodeId, Symbol, SymbolFlags, SymbolId, SymbolTable,
};
use tsgo_core::text::TextRange;
use tsgo_diagnostics::Message;

pub use nameresolver::NameResolver;
pub use referenceresolver::{
    new_reference_resolver, ReferenceResolver, ReferenceResolverHooks, ReferenceResolverImpl,
};

bitflags::bitflags! {
    /// Classifies how the binder should treat a node when recursing into it:
    /// as a container, a block scope, a control-flow root, a function, ...
    ///
    /// Mirrors Go `ContainerFlags` (an `int32` `iota` enum).
    ///
    /// # Examples
    /// ```
    /// use tsgo_binder::ContainerFlags;
    /// let f = ContainerFlags::IS_CONTAINER | ContainerFlags::HAS_LOCALS;
    /// assert!(f.contains(ContainerFlags::IS_CONTAINER));
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/binder/binder.go:ContainerFlags
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct ContainerFlags: i32 {
        /// Not a container.
        const NONE = 0;
        /// Sets the current container (and block container).
        const IS_CONTAINER = 1 << 0;
        /// Sets the current block-scope container.
        const IS_BLOCK_SCOPED_CONTAINER = 1 << 1;
        /// Saves/restores control flow and starts a fresh flow.
        const IS_CONTROL_FLOW_CONTAINER = 1 << 2;
        /// The node is function-like.
        const IS_FUNCTION_LIKE = 1 << 3;
        /// The node is a function expression.
        const IS_FUNCTION_EXPRESSION = 1 << 4;
        /// The node owns a `locals` table.
        const HAS_LOCALS = 1 << 5;
        /// The node is an interface.
        const IS_INTERFACE = 1 << 6;
        /// The node is an object-literal/class-expression method or accessor.
        const IS_OBJECT_LITERAL_OR_CLASS_EXPRESSION_METHOD_OR_ACCESSOR = 1 << 7;
        /// The node is a `this` container.
        const IS_THIS_CONTAINER = 1 << 8;
        /// The node propagates a seen `this` keyword to its parent.
        const PROPAGATES_THIS_KEYWORD = 1 << 9;
    }
}

/// A diagnostic produced by the binder.
///
/// A minimal stand-in for Go `ast.Diagnostic`: it carries the source range, the
/// static message, the stringified arguments, and any related-information
/// children. The precise error span uses the node's loc (Go uses
/// `GetErrorRangeForNode`); refining the span is deferred.
///
/// Side effects: none (pure value type).
// Go: internal/ast/diagnostic.go:Diagnostic
#[derive(Clone, Debug)]
pub struct BinderDiagnostic {
    /// The source range the diagnostic applies to.
    pub loc: TextRange,
    /// The static diagnostic message.
    pub message: &'static Message,
    /// Stringified message arguments.
    pub args: Vec<String>,
    /// Related-information diagnostics attached to this one.
    pub related: Vec<BinderDiagnostic>,
}

/// The result of binding a source file: the symbol/flow graphs plus the side
/// maps that replace Go's per-node binder fields.
///
/// Side effects: none (pure value type).
// Go: internal/binder/binder.go:bindSourceFile (outputs)
#[derive(Debug, Default)]
pub struct BindResult {
    /// All symbols created during binding, indexed by [`SymbolId`].
    pub symbols: Vec<Symbol>,
    /// All flow nodes created during binding, indexed by [`FlowNodeId`].
    pub flow_nodes: Vec<FlowNode>,
    /// All flow-list cells created during binding, indexed by `FlowListId`.
    pub flow_lists: Vec<FlowList>,
    /// Synthetic switch-clause data for `SWITCH_CLAUSE` flow nodes.
    pub flow_switch_data: FxHashMap<FlowNodeId, FlowSwitchClauseData>,
    /// Synthetic reduce-label data for `REDUCE_LABEL` flow nodes.
    pub flow_reduce_data: FxHashMap<FlowNodeId, FlowReduceLabelData>,
    /// `node.Symbol`: the declaration symbol of a node.
    pub node_symbol: FxHashMap<NodeId, SymbolId>,
    /// `node.LocalSymbol`: the local symbol of an exported declaration.
    pub node_local_symbol: FxHashMap<NodeId, SymbolId>,
    /// `node.FlowNode`: the flow node attached to a node.
    pub node_flow: FxHashMap<NodeId, FlowNodeId>,
    /// `locals` table for each locals-bearing container node.
    pub locals: FxHashMap<NodeId, SymbolTable>,
    /// The source file's own symbol (set only for external/CommonJS modules).
    pub file_symbol: Option<SymbolId>,
    /// Bind diagnostics, in creation order.
    pub diagnostics: Vec<BinderDiagnostic>,
    /// The number of symbols created.
    pub symbol_count: usize,
    /// Names that are semantically classifiable.
    pub classifiable_names: FxHashSet<String>,
}

impl BindResult {
    /// Looks up `name` in the `locals` table of `container`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_binder::bind_source_file;
    /// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// use tsgo_core::scriptkind::ScriptKind;
    /// let mut r = parse_source_file(SourceFileParseOptions::default(), "var x;", ScriptKind::Ts);
    /// let result = bind_source_file(&mut r.arena, r.source_file);
    /// assert!(result.local(r.source_file, "x").is_some());
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn local(&self, container: NodeId, name: &str) -> Option<SymbolId> {
        self.locals
            .get(&container)
            .and_then(|t| t.get(name).copied())
    }

    /// Looks up `name` in the `exports` table of the symbol `symbol`.
    ///
    /// Side effects: none (pure).
    pub fn export(&self, symbol: SymbolId, name: &str) -> Option<SymbolId> {
        self.symbols[symbol.index()].exports.get(name).copied()
    }

    /// Looks up `name` in the `members` table of the symbol `symbol`.
    ///
    /// Side effects: none (pure).
    pub fn member(&self, symbol: SymbolId, name: &str) -> Option<SymbolId> {
        self.symbols[symbol.index()].members.get(name).copied()
    }

    /// Reports whether any diagnostic uses the given message.
    ///
    /// Side effects: none (pure).
    pub fn has_diagnostic(&self, message: &'static Message) -> bool {
        self.diagnostics
            .iter()
            .any(|d| std::ptr::eq(d.message, message))
    }
}

/// Identifies which symbol table a declaration is being added to.
///
/// Go passes the `ast.SymbolTable` (a map) directly; because Rust tables live
/// inside the binder's arena/maps, we pass a location handle to avoid aliasing
/// `&mut self`.
#[derive(Clone, Copy)]
enum TableLoc {
    /// The `locals` table of a container node.
    Locals(NodeId),
    /// The `members` table of a symbol.
    Members(SymbolId),
    /// The `exports` table of a symbol.
    Exports(SymbolId),
}

/// One entry on the active labeled-statement chain.
///
/// Side effects: none (pure value type).
// Go: internal/binder/binder.go:ActiveLabel
struct ActiveLabel {
    name: String,
    break_target: FlowNodeId,
    continue_target: Option<FlowNodeId>,
    referenced: bool,
}

/// The binder: a short-lived state machine over a borrowed [`NodeArena`] that
/// owns the symbol and flow graphs it produces.
///
/// Side effects: `bind_*` methods mutate the binder's graphs and may set node
/// flags on the arena.
// Go: internal/binder/binder.go:Binder
struct Binder<'a> {
    arena: &'a mut NodeArena,
    file: NodeId,
    file_name: String,
    is_declaration_file: bool,
    external_module_indicator: Option<NodeId>,
    common_js_module_indicator: Option<NodeId>,

    symbols: Vec<Symbol>,
    flow_nodes: Vec<FlowNode>,
    flow_lists: Vec<FlowList>,
    flow_switch_data: FxHashMap<FlowNodeId, FlowSwitchClauseData>,
    flow_reduce_data: FxHashMap<FlowNodeId, FlowReduceLabelData>,
    node_symbol: FxHashMap<NodeId, SymbolId>,
    node_local_symbol: FxHashMap<NodeId, SymbolId>,
    node_flow: FxHashMap<NodeId, FlowNodeId>,
    locals: FxHashMap<NodeId, SymbolTable>,
    file_symbol: Option<SymbolId>,
    diagnostics: Vec<BinderDiagnostic>,
    classifiable_names: FxHashSet<String>,
    symbol_count: usize,

    unreachable_flow: FlowNodeId,
    container: Option<NodeId>,
    this_container: Option<NodeId>,
    block_scope_container: Option<NodeId>,
    last_container: Option<NodeId>,
    current_flow: FlowNodeId,
    current_break_target: Option<FlowNodeId>,
    current_continue_target: Option<FlowNodeId>,
    current_return_target: Option<FlowNodeId>,
    current_true_target: Option<FlowNodeId>,
    current_false_target: Option<FlowNodeId>,
    current_exception_target: Option<FlowNodeId>,
    pre_switch_case_flow: Option<FlowNodeId>,
    active_label_list: Vec<ActiveLabel>,
    emit_flags: NodeFlags,
    seen_this_keyword: bool,
    has_explicit_return: bool,
    has_flow_effects: bool,
    in_assignment_pattern: bool,
}

/// Binds a parsed source file, returning the symbol and flow graphs.
///
/// Walks the tree rooted at `file`, creating symbols + symbol tables, declaring
/// and merging declarations (reporting conflicts), and building the
/// control-flow graph.
///
/// # Examples
/// ```
/// use tsgo_binder::bind_source_file;
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let mut r = parse_source_file(SourceFileParseOptions::default(), "function f(){}", ScriptKind::Ts);
/// let result = bind_source_file(&mut r.arena, r.source_file);
/// assert!(result.local(r.source_file, "f").is_some());
/// ```
///
/// Side effects: may set `NodeFlags` (export context, unreachable, reachability)
/// on nodes in `arena`; otherwise produces a fresh [`BindResult`].
// Go: internal/binder/binder.go:BindSourceFile/bindSourceFile
pub fn bind_source_file(arena: &mut NodeArena, file: NodeId) -> BindResult {
    let mut b = Binder::new(arena);
    b.bind_source_file_inner(file);
    b.into_result()
}

/// Returns the internal symbol name for a private identifier `description`
/// within the class whose symbol is `class_symbol`.
///
/// The format is `<prefix>#<symbolId>@<description>`, matching Go's
/// `__#<id>@#name` once internal prefixes are escaped.
///
/// # Examples
/// ```
/// use tsgo_binder::get_symbol_name_for_private_identifier;
/// use tsgo_ast::SymbolId;
/// assert_eq!(
///     get_symbol_name_for_private_identifier(SymbolId(3), "#x"),
///     "\u{FE}#3@#x"
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/binder/binder.go:GetSymbolNameForPrivateIdentifier
pub fn get_symbol_name_for_private_identifier(class_symbol: SymbolId, description: &str) -> String {
    format!(
        "{}#{}@{}",
        tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_PREFIX,
        class_symbol.0,
        description
    )
}

/// Returns the first `"use strict"` prologue directive in `statements`, if the
/// directive prologue contains one.
///
/// DIVERGENCE(port): Go compares the raw source text (`"use strict"` with
/// quotes, disallowing escapes). Lacking the source text here, we compare the
/// string literal's cooked value to `use strict`.
///
/// # Examples
/// ```
/// use tsgo_binder::find_use_strict_prologue;
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::NodeData;
/// let r = parse_source_file(SourceFileParseOptions::default(), "\"use strict\"; var x;", ScriptKind::Ts);
/// let stmts = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes.clone(),
///     _ => unreachable!(),
/// };
/// assert!(find_use_strict_prologue(&r.arena, &stmts).is_some());
/// ```
///
/// Side effects: none (pure).
// Go: internal/binder/binder.go:FindUseStrictPrologue
pub fn find_use_strict_prologue(arena: &NodeArena, statements: &[NodeId]) -> Option<NodeId> {
    for &statement in statements {
        if astquery::is_prologue_directive(arena, statement) {
            if let NodeData::ExpressionStatement(d) = arena.data(statement) {
                if arena.text(d.expression) == "use strict" {
                    return Some(statement);
                }
            }
        } else {
            return None;
        }
    }
    None
}

/// Sets `symbol`'s value declaration to `node`, applying Go's precedence rules
/// (non-assignment over assignment; non-namespace over namespace).
///
/// # Examples
/// ```
/// use tsgo_binder::set_value_declaration;
/// use tsgo_ast::{NodeArena, Symbol, SymbolId};
/// let mut arena = NodeArena::new();
/// let node = arena.new_identifier("x");
/// let mut symbols = vec![Symbol::default()];
/// set_value_declaration(&mut symbols, &arena, SymbolId(0), node);
/// assert_eq!(symbols[0].value_declaration, Some(node));
/// ```
///
/// Side effects: mutates `symbols[symbol].value_declaration`.
// Go: internal/binder/binder.go:SetValueDeclaration
pub fn set_value_declaration(
    symbols: &mut [Symbol],
    arena: &NodeArena,
    symbol: SymbolId,
    node: NodeId,
) {
    let value_declaration = symbols[symbol.index()].value_declaration;
    let replace = match value_declaration {
        None => true,
        Some(vd) => {
            (is_assignment_declaration(arena, vd) && !is_assignment_declaration(arena, node))
                || (arena.kind(vd) != arena.kind(node)
                    && is_effective_module_declaration(arena, vd))
        }
    };
    if replace {
        symbols[symbol.index()].value_declaration = Some(node);
    }
}

/// Computes the [`ContainerFlags`] for a node, classifying how the binder
/// should treat it during recursion.
///
/// # Examples
/// ```
/// use tsgo_binder::{get_container_flags, ContainerFlags};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::NodeData;
/// let r = parse_source_file(SourceFileParseOptions::default(), "function f(){}", ScriptKind::Ts);
/// let f = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// assert!(get_container_flags(&r.arena, f).contains(ContainerFlags::IS_FUNCTION_LIKE));
/// ```
///
/// Side effects: none (pure).
// Go: internal/binder/binder.go:GetContainerFlags
pub fn get_container_flags(arena: &NodeArena, node: NodeId) -> ContainerFlags {
    use ContainerFlags as C;
    match arena.kind(node) {
        Kind::ClassExpression
        | Kind::ClassDeclaration
        | Kind::EnumDeclaration
        | Kind::ObjectLiteralExpression
        | Kind::TypeLiteral
        | Kind::JsxAttributes => C::IS_CONTAINER,
        Kind::InterfaceDeclaration => C::IS_CONTAINER | C::IS_INTERFACE,
        Kind::ModuleDeclaration
        | Kind::TypeAliasDeclaration
        | Kind::MappedType
        | Kind::IndexSignature => C::IS_CONTAINER | C::HAS_LOCALS,
        Kind::SourceFile => C::IS_CONTAINER | C::IS_CONTROL_FLOW_CONTAINER | C::HAS_LOCALS,
        Kind::GetAccessor | Kind::SetAccessor | Kind::MethodDeclaration => {
            if astquery::is_object_literal_or_class_expression_method_or_accessor(arena, node) {
                C::IS_CONTAINER
                    | C::IS_CONTROL_FLOW_CONTAINER
                    | C::HAS_LOCALS
                    | C::IS_FUNCTION_LIKE
                    | C::IS_OBJECT_LITERAL_OR_CLASS_EXPRESSION_METHOD_OR_ACCESSOR
                    | C::IS_THIS_CONTAINER
            } else {
                C::IS_CONTAINER
                    | C::IS_CONTROL_FLOW_CONTAINER
                    | C::HAS_LOCALS
                    | C::IS_FUNCTION_LIKE
                    | C::IS_THIS_CONTAINER
            }
        }
        Kind::Constructor | Kind::FunctionDeclaration | Kind::ClassStaticBlockDeclaration => {
            C::IS_CONTAINER
                | C::IS_CONTROL_FLOW_CONTAINER
                | C::HAS_LOCALS
                | C::IS_FUNCTION_LIKE
                | C::IS_THIS_CONTAINER
        }
        Kind::MethodSignature
        | Kind::CallSignature
        | Kind::FunctionType
        | Kind::ConstructSignature
        | Kind::ConstructorType => {
            C::IS_CONTAINER
                | C::IS_CONTROL_FLOW_CONTAINER
                | C::HAS_LOCALS
                | C::IS_FUNCTION_LIKE
                | C::PROPAGATES_THIS_KEYWORD
        }
        Kind::FunctionExpression => {
            C::IS_CONTAINER
                | C::IS_CONTROL_FLOW_CONTAINER
                | C::HAS_LOCALS
                | C::IS_FUNCTION_LIKE
                | C::IS_FUNCTION_EXPRESSION
                | C::IS_THIS_CONTAINER
        }
        Kind::ArrowFunction => {
            C::IS_CONTAINER
                | C::IS_CONTROL_FLOW_CONTAINER
                | C::HAS_LOCALS
                | C::IS_FUNCTION_LIKE
                | C::IS_FUNCTION_EXPRESSION
                | C::PROPAGATES_THIS_KEYWORD
        }
        Kind::ModuleBlock => C::IS_CONTROL_FLOW_CONTAINER,
        Kind::PropertyDeclaration => {
            let has_init = matches!(
                arena.data(node),
                NodeData::PropertyDeclaration(d) if d.initializer.is_some()
            );
            if has_init {
                C::IS_CONTROL_FLOW_CONTAINER | C::IS_THIS_CONTAINER
            } else {
                C::NONE
            }
        }
        Kind::CatchClause
        | Kind::ForStatement
        | Kind::ForInStatement
        | Kind::ForOfStatement
        | Kind::CaseBlock => C::IS_BLOCK_SCOPED_CONTAINER | C::HAS_LOCALS,
        Kind::Block => {
            let parent_function_like = arena.parent(node).is_some_and(|p| {
                astquery::is_function_like(arena, p) || astquery::is_class_static_block(arena, p)
            });
            if parent_function_like {
                C::NONE
            } else {
                C::IS_BLOCK_SCOPED_CONTAINER | C::HAS_LOCALS
            }
        }
        _ => C::NONE,
    }
}

// Go: internal/binder/binder.go:isAssignmentDeclaration
fn is_assignment_declaration(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::BinaryExpression
            | Kind::PropertyAccessExpression
            | Kind::ElementAccessExpression
            | Kind::Identifier
            | Kind::CallExpression
    )
}

// Go: internal/binder/binder.go:isEffectiveModuleDeclaration
fn is_effective_module_declaration(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.kind(node), Kind::ModuleDeclaration | Kind::Identifier)
}

impl<'a> Binder<'a> {
    /// Creates a fresh binder over `arena`, allocating the shared
    /// `unreachable` flow node.
    fn new(arena: &'a mut NodeArena) -> Binder<'a> {
        let mut b = Binder {
            arena,
            file: NodeId(0),
            file_name: String::new(),
            is_declaration_file: false,
            external_module_indicator: None,
            common_js_module_indicator: None,
            symbols: Vec::new(),
            flow_nodes: Vec::new(),
            flow_lists: Vec::new(),
            flow_switch_data: FxHashMap::default(),
            flow_reduce_data: FxHashMap::default(),
            node_symbol: FxHashMap::default(),
            node_local_symbol: FxHashMap::default(),
            node_flow: FxHashMap::default(),
            locals: FxHashMap::default(),
            file_symbol: None,
            diagnostics: Vec::new(),
            classifiable_names: FxHashSet::default(),
            symbol_count: 0,
            unreachable_flow: FlowNodeId(0),
            container: None,
            this_container: None,
            block_scope_container: None,
            last_container: None,
            current_flow: FlowNodeId(0),
            current_break_target: None,
            current_continue_target: None,
            current_return_target: None,
            current_true_target: None,
            current_false_target: None,
            current_exception_target: None,
            pre_switch_case_flow: None,
            active_label_list: Vec::new(),
            emit_flags: NodeFlags::NONE,
            seen_this_keyword: false,
            has_explicit_return: false,
            has_flow_effects: false,
            in_assignment_pattern: false,
        };
        b.unreachable_flow = b.new_flow_node(FlowFlags::UNREACHABLE);
        b.current_flow = b.unreachable_flow;
        b
    }

    /// Moves the binder's owned graphs into a [`BindResult`].
    fn into_result(self) -> BindResult {
        BindResult {
            symbols: self.symbols,
            flow_nodes: self.flow_nodes,
            flow_lists: self.flow_lists,
            flow_switch_data: self.flow_switch_data,
            flow_reduce_data: self.flow_reduce_data,
            node_symbol: self.node_symbol,
            node_local_symbol: self.node_local_symbol,
            node_flow: self.node_flow,
            locals: self.locals,
            file_symbol: self.file_symbol,
            diagnostics: self.diagnostics,
            symbol_count: self.symbol_count,
            classifiable_names: self.classifiable_names,
        }
    }

    // Go: internal/binder/binder.go:bindSourceFile
    fn bind_source_file_inner(&mut self, file: NodeId) {
        self.file = file;
        if let NodeData::SourceFile(d) = self.arena.data(file) {
            self.file_name = d.file_name.clone();
            self.is_declaration_file = d.is_declaration_file;
            self.external_module_indicator = d.external_module_indicator;
        }
        self.bind(file);
        // bindDeferredExpandoAssignments is JS/CommonJS only and is deferred.
    }

    // ── Symbol-table location helpers ────────────────────────────────────────

    /// Returns the `TableLoc` for a container's `locals`, ensuring it exists.
    // Go: internal/ast/utilities.go:GetLocals
    fn get_locals(&mut self, container: NodeId) -> TableLoc {
        self.locals.entry(container).or_default();
        TableLoc::Locals(container)
    }

    fn table_get(&self, loc: TableLoc, name: &str) -> Option<SymbolId> {
        match loc {
            TableLoc::Locals(n) => self.locals.get(&n).and_then(|t| t.get(name).copied()),
            TableLoc::Members(s) => self.symbols[s.index()].members.get(name).copied(),
            TableLoc::Exports(s) => self.symbols[s.index()].exports.get(name).copied(),
        }
    }

    fn table_set(&mut self, loc: TableLoc, name: String, sym: SymbolId) {
        match loc {
            TableLoc::Locals(n) => {
                self.locals.entry(n).or_default().insert(name, sym);
            }
            TableLoc::Members(s) => {
                self.symbols[s.index()].members.insert(name, sym);
            }
            TableLoc::Exports(s) => {
                self.symbols[s.index()].exports.insert(name, sym);
            }
        }
    }

    /// `node.Symbol`: the declaration symbol attached to a node.
    fn symbol_of(&self, node: NodeId) -> Option<SymbolId> {
        self.node_symbol.get(&node).copied()
    }

    // Go: internal/ast/utilities.go:IsExternalOrCommonJSModule
    fn is_external_or_commonjs_module(&self) -> bool {
        self.external_module_indicator.is_some() || self.common_js_module_indicator.is_some()
    }

    // ── Flow arena allocation ────────────────────────────────────────────────

    // Go: internal/binder/binder.go:newFlowNode
    fn new_flow_node(&mut self, flags: FlowFlags) -> FlowNodeId {
        let id = FlowNodeId(self.flow_nodes.len() as u32);
        self.flow_nodes.push(FlowNode {
            flags,
            node: None,
            antecedent: None,
            antecedents: None,
        });
        id
    }

    // Go: internal/binder/binder.go:newFlowNodeEx
    fn new_flow_node_ex(
        &mut self,
        flags: FlowFlags,
        node: Option<NodeId>,
        antecedent: Option<FlowNodeId>,
    ) -> FlowNodeId {
        let id = self.new_flow_node(flags);
        self.flow_nodes[id.0 as usize].node = node;
        self.flow_nodes[id.0 as usize].antecedent = antecedent;
        id
    }

    fn flow_flags(&self, id: FlowNodeId) -> FlowFlags {
        self.flow_nodes[id.0 as usize].flags
    }

    // ── Diagnostics ──────────────────────────────────────────────────────────

    // Go: internal/binder/binder.go:errorOnNode
    fn error_on_node(&mut self, node: NodeId, message: &'static Message, args: Vec<String>) {
        let diag = self.create_diagnostic_for_node(node, message, args);
        self.add_diagnostic(diag);
    }

    // Go: internal/binder/binder.go:createDiagnosticForNode
    fn create_diagnostic_for_node(
        &self,
        node: NodeId,
        message: &'static Message,
        args: Vec<String>,
    ) -> BinderDiagnostic {
        BinderDiagnostic {
            loc: self.arena.loc(node),
            message,
            args,
            related: Vec::new(),
        }
    }

    // Go: internal/binder/binder.go:addDiagnostic
    fn add_diagnostic(&mut self, diagnostic: BinderDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    // ── Dispatch ─────────────────────────────────────────────────────────────

    /// Binds a node: first creates its symbol (if it is a declaration), then
    /// recurses into its children (as a plain child walk or a container walk).
    // Go: internal/binder/binder.go:bind
    fn bind(&mut self, node: NodeId) {
        let kind = self.arena.kind(node);
        match kind {
            Kind::Identifier => {
                self.set_node_flow(node);
            }
            Kind::ThisKeyword | Kind::SuperKeyword => {
                if kind == Kind::ThisKeyword {
                    self.seen_this_keyword = true;
                }
                self.set_node_flow(node);
            }
            Kind::MetaProperty => {
                self.set_node_flow(node);
            }
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression => {
                if flow::is_narrowable_reference(self.arena, node) {
                    self.set_node_flow(node);
                }
            }
            Kind::BinaryExpression => {
                // CommonJS assignment-declaration recognition. Only the patterns
                // that set the module indicator are handled here; the expando
                // (`F.x = Y`) and `this.x = Y` cases declare nothing in this
                // port (their symbol creation is deferred), and the strict-mode
                // binary-expression check is also deferred.
                match astquery::get_assignment_declaration_kind(self.arena, node) {
                    astquery::JsDeclarationKind::ModuleExports => {
                        self.bind_module_exports_assignment(node)
                    }
                    astquery::JsDeclarationKind::ExportsProperty => {
                        self.bind_exports_or_object_define_property(node)
                    }
                    _ => {}
                }
            }
            Kind::CallExpression => {
                // `Object.defineProperty(...)` assignment-declaration cases are
                // deferred; the require-call indicator is the parity-relevant
                // path. Go: internal/binder/binder.go:bind (KindCallExpression).
                if astquery::is_in_js_file(self.arena, node) {
                    self.bind_call_expression(node);
                }
            }
            Kind::TypeParameter => self.bind_type_parameter(node),
            Kind::Parameter => self.bind_parameter(node),
            Kind::VariableDeclaration => self.bind_variable_declaration_or_binding_element(node),
            Kind::BindingElement => {
                self.set_node_flow(node);
                self.bind_variable_declaration_or_binding_element(node);
            }
            Kind::PropertyDeclaration | Kind::PropertySignature => self.bind_property_worker(node),
            Kind::PropertyAssignment | Kind::ShorthandPropertyAssignment => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::PROPERTY,
                    SymbolFlags::PROPERTY_EXCLUDES,
                );
            }
            Kind::EnumMember => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::ENUM_MEMBER,
                    SymbolFlags::ENUM_MEMBER_EXCLUDES,
                );
            }
            Kind::CallSignature | Kind::ConstructSignature | Kind::IndexSignature => {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::SIGNATURE,
                    SymbolFlags::NONE,
                );
            }
            Kind::MethodDeclaration | Kind::MethodSignature => {
                let excludes = if astquery::is_object_literal_or_class_expression_method_or_accessor(
                    self.arena, node,
                ) && self.arena.kind(node) == Kind::MethodDeclaration
                    && self
                        .arena
                        .parent(node)
                        .is_some_and(|p| self.arena.kind(p) == Kind::ObjectLiteralExpression)
                {
                    SymbolFlags::VALUE
                } else {
                    SymbolFlags::METHOD_EXCLUDES
                };
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::METHOD | self.get_optional_symbol_flag_for_node(node),
                    excludes,
                );
            }
            Kind::FunctionDeclaration => self.bind_function_declaration(node),
            Kind::Constructor => {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::CONSTRUCTOR,
                    SymbolFlags::NONE,
                );
            }
            Kind::GetAccessor => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::GET_ACCESSOR,
                    SymbolFlags::GET_ACCESSOR_EXCLUDES,
                );
            }
            Kind::SetAccessor => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::SET_ACCESSOR,
                    SymbolFlags::SET_ACCESSOR_EXCLUDES,
                );
            }
            Kind::FunctionType | Kind::ConstructorType => {
                self.bind_function_or_constructor_type(node)
            }
            Kind::TypeLiteral | Kind::MappedType => {
                self.bind_anonymous_declaration(
                    node,
                    SymbolFlags::TYPE_LITERAL,
                    tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_TYPE.to_string(),
                );
            }
            Kind::ObjectLiteralExpression => {
                self.bind_anonymous_declaration(
                    node,
                    SymbolFlags::OBJECT_LITERAL,
                    tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_OBJECT.to_string(),
                );
            }
            Kind::FunctionExpression | Kind::ArrowFunction => self.bind_function_expression(node),
            Kind::ClassExpression | Kind::ClassDeclaration => {
                self.bind_class_like_declaration(node)
            }
            Kind::InterfaceDeclaration => {
                self.bind_block_scoped_declaration(
                    node,
                    SymbolFlags::INTERFACE,
                    SymbolFlags::INTERFACE_EXCLUDES,
                );
            }
            Kind::TypeAliasDeclaration => {
                self.bind_block_scoped_declaration(
                    node,
                    SymbolFlags::TYPE_ALIAS,
                    SymbolFlags::TYPE_ALIAS_EXCLUDES,
                );
            }
            Kind::EnumDeclaration => self.bind_enum_declaration(node),
            Kind::ModuleDeclaration => self.bind_module_declaration(node),
            Kind::ImportEqualsDeclaration
            | Kind::NamespaceImport
            | Kind::ImportSpecifier
            | Kind::ExportSpecifier => {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::ALIAS,
                    SymbolFlags::ALIAS_EXCLUDES,
                );
            }
            Kind::NamespaceExportDeclaration => self.bind_namespace_export_declaration(node),
            Kind::ImportClause => self.bind_import_clause(node),
            Kind::ExportDeclaration => self.bind_export_declaration(node),
            Kind::ExportAssignment => self.bind_export_assignment(node),
            Kind::SourceFile => self.bind_source_file_if_external_module(),
            Kind::JsxAttributes => self.bind_jsx_attributes(node),
            Kind::JsxAttribute => self.bind_jsx_attribute(node),
            _ => {}
        }
        // Recurse into children.
        if kind > Kind::LAST_TOKEN {
            let container_flags = get_container_flags(self.arena, node);
            if container_flags == ContainerFlags::NONE {
                self.bind_children(node);
            } else {
                self.bind_container(node, container_flags);
            }
        }
    }

    /// `node.FlowNode = currentFlow` for nodes that carry flow.
    // Go: internal/binder/binder.go:setFlowNode
    fn set_node_flow(&mut self, node: NodeId) {
        self.node_flow.insert(node, self.current_flow);
    }

    // Go: internal/binder/binder.go:bindSourceFileIfExternalModule
    fn bind_source_file_if_external_module(&mut self) {
        let file = self.file;
        self.set_export_context_flag(file);
        if self.is_external_or_commonjs_module() {
            self.bind_source_file_as_external_module();
        }
        // JSON source file handling is deferred.
    }

    // Go: internal/binder/binder.go:bindSourceFileAsExternalModule
    fn bind_source_file_as_external_module(&mut self) {
        let stem = remove_file_extension(&self.file_name);
        let name = format!("\"{stem}\"");
        let file = self.file;
        self.bind_anonymous_declaration(file, SymbolFlags::VALUE_MODULE, name);
        self.file_symbol = self.symbol_of(file);
    }

    // Go: internal/binder/binder.go:bindCallExpression
    fn bind_call_expression(&mut self, node: NodeId) {
        // We're only inspecting call expressions to detect CommonJS modules, so
        // we can skip this check if we've already seen the module indicator.
        if self.common_js_module_indicator.is_none() && astquery::is_require_call(self.arena, node)
        {
            self.set_common_js_module_indicator(node);
        }
    }

    // Go: internal/binder/binder.go:setCommonJSModuleIndicator
    fn set_common_js_module_indicator(&mut self, node: NodeId) -> bool {
        // A real external-module indicator (a non-file `import`/`export`) means
        // the file is an ES module, not CommonJS.
        if let Some(indicator) = self.external_module_indicator {
            if indicator != self.file {
                return false;
            }
        }
        if self.common_js_module_indicator.is_none() {
            self.common_js_module_indicator = Some(node);
            if self.external_module_indicator.is_none() {
                self.bind_source_file_as_external_module();
            }
        }
        true
    }

    // Go: internal/binder/binder.go:bindModuleExportsAssignment
    fn bind_module_exports_assignment(&mut self, node: NodeId) {
        // Setting the indicator (and synthesizing the file symbol) is the
        // resolution-relevant effect. DEFER(phase-4): `trackNestedCJSExport`
        // (declaration-emit serialization tracking) and the `module.exports`
        // export-symbol declaration on the file symbol.
        // blocked-by: CommonJS export-symbol shape + declaration emit.
        self.set_common_js_module_indicator(node);
    }

    // Go: internal/binder/binder.go:bindExportsOrObjectDefineProperty
    fn bind_exports_or_object_define_property(&mut self, node: NodeId) {
        // DEFER(phase-4): the `exports.x` export-symbol declaration on the file
        // symbol; only the module indicator is set here.
        // blocked-by: CommonJS export-symbol shape.
        self.set_common_js_module_indicator(node);
    }

    // Go: internal/binder/binder.go:setExportContextFlag
    fn set_export_context_flag(&mut self, node: NodeId) {
        if self.arena.flags(node).contains(NodeFlags::AMBIENT)
            && !self.has_export_declarations(node)
        {
            self.arena.add_flags(node, NodeFlags::EXPORT_CONTEXT);
        } else {
            let flags = self.arena.flags(node) & !NodeFlags::EXPORT_CONTEXT;
            self.arena.set_flags(node, flags);
        }
    }

    // Go: internal/binder/binder.go:hasExportDeclarations
    fn has_export_declarations(&self, node: NodeId) -> bool {
        let statements: Vec<NodeId> = match self.arena.data(node) {
            NodeData::SourceFile(d) => d.statements.nodes.clone(),
            NodeData::ModuleDeclaration(d) => match d.body {
                Some(body) => match self.arena.data(body) {
                    NodeData::ModuleBlock(b) => b.statements.nodes.clone(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        statements.iter().any(|&s| {
            astquery::is_export_declaration(self.arena, s)
                || astquery::is_export_assignment(self.arena, s)
        })
    }

    fn bind_jsx_attributes(&mut self, node: NodeId) {
        self.bind_anonymous_declaration(
            node,
            SymbolFlags::OBJECT_LITERAL,
            tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_JSX_ATTRIBUTES.to_string(),
        );
    }

    fn bind_jsx_attribute(&mut self, node: NodeId) {
        self.declare_symbol_and_add_to_symbol_table(
            node,
            SymbolFlags::PROPERTY,
            SymbolFlags::PROPERTY_EXCLUDES,
        );
    }

    // Go: internal/binder/binder.go:bindNamespaceExportDeclaration (partial)
    fn bind_namespace_export_declaration(&mut self, node: NodeId) {
        // The full Go routine validates placement and declares the alias; the
        // alias declaration is the part observable to symbol-table consumers.
        if let Some(container) = self.container {
            if let Some(sym) = self.symbol_of(container) {
                self.declare_symbol(
                    TableLoc::Exports(sym),
                    Some(sym),
                    node,
                    SymbolFlags::ALIAS,
                    SymbolFlags::ALIAS_EXCLUDES,
                );
            }
        }
    }

    // Go: internal/binder/binder.go:bindImportClause
    fn bind_import_clause(&mut self, node: NodeId) {
        let has_name =
            matches!(self.arena.data(node), NodeData::ImportClause(d) if d.name.is_some());
        if has_name {
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::ALIAS,
                SymbolFlags::ALIAS_EXCLUDES,
            );
        }
    }

    // Go: internal/binder/binder.go:bindExportDeclaration
    fn bind_export_declaration(&mut self, node: NodeId) {
        let export_clause = match self.arena.data(node) {
            NodeData::ExportDeclaration(d) => d.export_clause,
            _ => None,
        };
        let container_symbol = self.container.and_then(|c| self.symbol_of(c));
        match container_symbol {
            None => {
                let name = self.get_declaration_name(node);
                self.bind_anonymous_declaration(node, SymbolFlags::EXPORT_STAR, name);
            }
            Some(sym) => {
                if export_clause.is_none() {
                    self.declare_symbol(
                        TableLoc::Exports(sym),
                        Some(sym),
                        node,
                        SymbolFlags::EXPORT_STAR,
                        SymbolFlags::NONE,
                    );
                } else if let Some(clause) = export_clause {
                    if self.arena.kind(clause) == Kind::NamespaceExport {
                        self.declare_symbol(
                            TableLoc::Exports(sym),
                            Some(sym),
                            clause,
                            SymbolFlags::ALIAS,
                            SymbolFlags::ALIAS_EXCLUDES,
                        );
                    }
                }
            }
        }
    }

    // Go: internal/binder/binder.go:bindExportAssignment
    fn bind_export_assignment(&mut self, node: NodeId) {
        let container = self
            .container
            .expect("export assignment requires a container");
        let container_symbol = self.symbol_of(container);
        let expression = match self.arena.data(node) {
            NodeData::ExportAssignment(d) => d.expression,
            _ => unreachable!(),
        };
        let is_export_equals = matches!(
            self.arena.data(node),
            NodeData::ExportAssignment(d) if d.is_export_equals
        );
        match container_symbol {
            None => {
                let name = self.get_declaration_name(node);
                self.bind_anonymous_declaration(node, SymbolFlags::VALUE, name);
            }
            Some(sym) => {
                let flags = if astquery::expression_is_alias(self.arena, expression) {
                    SymbolFlags::ALIAS
                } else {
                    SymbolFlags::PROPERTY
                };
                let symbol = self.declare_symbol(
                    TableLoc::Exports(sym),
                    Some(sym),
                    node,
                    flags,
                    SymbolFlags::ALL,
                );
                if is_export_equals {
                    set_value_declaration(&mut self.symbols, self.arena, symbol, node);
                }
            }
        }
    }

    // Go: internal/binder/binder.go:bindModuleDeclaration
    //
    // Go creates the module's symbol on EVERY path: the non-ambient branch via
    // `declareModuleSymbol`, and the ambient branch via either
    // `declareModuleSymbol` (`IsModuleAugmentationExternal`) or
    // `declareSymbolAndAddToSymbolTable`. All of them funnel through
    // `declareSymbolAndAddToSymbolTable` with `ValueModule` flags and identical
    // symbol-table placement, so the symbol creation is unconditional here.
    //
    // Creating it is the correctness-critical effect: an ambient module
    // container (`declare global { … }` / `declare module "…" { … }`) must own a
    // symbol BEFORE its members are bound, or `declareModuleMember`'s
    // `symbol_of(container).unwrap()` hits `None` and panics. (A prior port
    // returned early for ambient modules, leaving the `declare global`
    // augmentation in the bundled `lib.es2025.iterator.d.ts` — and any
    // `.d.ts`/`@types` `declare module "…"` — without a container symbol.)
    //
    // DEFER(phase-4): the `ValueModule`-vs-`NamespaceModule` instance-state
    // selection (`declareModuleSymbol`/`GetModuleInstanceState`), the
    // const-enum-only-module bookkeeping, the `export`-modifier TS2668 error, and
    // the string-literal `module "…"` pattern tracking (`TryParsePattern` /
    // `PatternAmbientModules`). None of these change which symbol table the
    // module symbol lands in for the bundled libs; the `ValueModule`
    // simplification matches the pre-existing non-ambient path.
    // blocked-by: `GetModuleInstanceState` + `core.TryParsePattern` ports.
    fn bind_module_declaration(&mut self, node: NodeId) {
        self.set_export_context_flag(node);
        self.declare_symbol_and_add_to_symbol_table(
            node,
            SymbolFlags::VALUE_MODULE,
            SymbolFlags::VALUE_MODULE_EXCLUDES,
        );
    }

    // ── Container / child traversal ──────────────────────────────────────────

    // Go: internal/binder/binder.go:bindContainer
    fn bind_container(&mut self, node: NodeId, container_flags: ContainerFlags) {
        let save_container = self.container;
        let save_this_container = self.this_container;
        let saved_block_scope_container = self.block_scope_container;

        if container_flags.contains(ContainerFlags::IS_CONTAINER) {
            self.container = Some(node);
            self.block_scope_container = Some(node);
            if container_flags.contains(ContainerFlags::HAS_LOCALS) {
                self.locals.entry(node).or_default();
                self.add_to_container_chain(node);
            }
        } else if container_flags.contains(ContainerFlags::IS_BLOCK_SCOPED_CONTAINER) {
            self.block_scope_container = Some(node);
            self.add_to_container_chain(node);
        }
        if container_flags.contains(ContainerFlags::IS_THIS_CONTAINER) {
            self.this_container = Some(node);
        }

        if container_flags.contains(ContainerFlags::IS_CONTROL_FLOW_CONTAINER) {
            let save_current_flow = self.current_flow;
            let save_break_target = self.current_break_target;
            let save_continue_target = self.current_continue_target;
            let save_return_target = self.current_return_target;
            let save_exception_target = self.current_exception_target;
            let save_active_label_list = std::mem::take(&mut self.active_label_list);
            let save_has_explicit_return = self.has_explicit_return;
            let save_seen_this_keyword = self.seen_this_keyword;

            // IIFE/return-flow detection is deferred; treat every control-flow
            // container as non-immediately-invoked for now.
            let is_immediately_invoked = false;
            if !is_immediately_invoked {
                let flow_start = self.new_flow_node(FlowFlags::START);
                self.current_flow = flow_start;
                if container_flags.intersects(
                    ContainerFlags::IS_FUNCTION_EXPRESSION
                        | ContainerFlags::IS_OBJECT_LITERAL_OR_CLASS_EXPRESSION_METHOD_OR_ACCESSOR,
                ) {
                    self.flow_nodes[flow_start.0 as usize].node = Some(node);
                }
            }
            self.current_return_target = if self.arena.kind(node) == Kind::Constructor {
                Some(self.new_flow_node(FlowFlags::BRANCH_LABEL))
            } else {
                None
            };
            self.current_exception_target = None;
            self.current_break_target = None;
            self.current_continue_target = None;
            self.has_explicit_return = false;
            self.seen_this_keyword = false;
            self.bind_children(node);

            let reset = self.arena.flags(node)
                & !(NodeFlags::REACHABILITY_AND_EMIT_FLAGS | NodeFlags::CONTAINS_THIS);
            self.arena.set_flags(node, reset);
            if !self
                .flow_flags(self.current_flow)
                .contains(FlowFlags::UNREACHABLE)
                && container_flags.contains(ContainerFlags::IS_FUNCTION_LIKE)
                && self.has_function_body(node)
            {
                self.arena.add_flags(node, NodeFlags::HAS_IMPLICIT_RETURN);
                if self.has_explicit_return {
                    self.arena.add_flags(node, NodeFlags::HAS_EXPLICIT_RETURN);
                }
            }
            if self.seen_this_keyword {
                self.arena.add_flags(node, NodeFlags::CONTAINS_THIS);
            }
            if self.arena.kind(node) == Kind::SourceFile {
                self.arena.add_flags(node, self.emit_flags);
            }
            if let Some(return_target) = self.current_return_target {
                let cur = self.current_flow;
                self.add_antecedent(return_target, cur);
                self.current_flow = self.finish_flow_label(return_target);
            }
            self.current_flow = save_current_flow;
            self.current_break_target = save_break_target;
            self.current_continue_target = save_continue_target;
            self.current_return_target = save_return_target;
            self.current_exception_target = save_exception_target;
            self.active_label_list = save_active_label_list;
            self.has_explicit_return = save_has_explicit_return;
            if container_flags.contains(ContainerFlags::PROPAGATES_THIS_KEYWORD) {
                self.seen_this_keyword = save_seen_this_keyword || self.seen_this_keyword;
            } else {
                self.seen_this_keyword = save_seen_this_keyword;
            }
        } else if container_flags.contains(ContainerFlags::IS_INTERFACE) {
            let save_seen_this_keyword = self.seen_this_keyword;
            self.seen_this_keyword = false;
            self.bind_children(node);
            if self.seen_this_keyword {
                self.arena.add_flags(node, NodeFlags::CONTAINS_THIS);
            } else {
                let f = self.arena.flags(node) & !NodeFlags::CONTAINS_THIS;
                self.arena.set_flags(node, f);
            }
            self.seen_this_keyword = save_seen_this_keyword;
        } else {
            self.bind_children(node);
        }

        // After binding a JS source file's children, declare the CommonJS
        // `module`/`exports` file locals if a CommonJS module indicator was seen
        // during that walk (a `require(...)` call or a `module.exports` /
        // `exports.x` assignment). This makes them resolve through the normal
        // scope lookup so the checker does not report TS2304/TS2591 for them.
        // DEFER(phase-4): the deferred top-level JSTypeAliasDeclaration binding
        // and `bindCommonJSTypeExports` (export `=` promotion). blocked-by: JS
        // type-alias reparsing + CommonJS export-symbol shape.
        // Go: internal/binder/binder.go:bindContainer (SourceFile finalizer)
        if self.arena.kind(node) == Kind::SourceFile
            && astquery::is_in_js_file(self.arena, node)
            && self.common_js_module_indicator.is_some()
        {
            self.declare_common_js_variable("module");
            self.declare_common_js_variable("exports");
        }

        self.container = save_container;
        self.this_container = save_this_container;
        self.block_scope_container = saved_block_scope_container;
    }

    fn has_function_body(&self, node: NodeId) -> bool {
        let body = match self.arena.data(node) {
            NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.body,
            NodeData::MethodDeclaration(d) => d.body,
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.body,
            NodeData::ConstructorDeclaration(d) => d.body,
            NodeData::ArrowFunction(d) => Some(d.body),
            _ => None,
        };
        body.is_some_and(|b| tsgo_ast::utilities::node_is_present(self.arena, b))
    }

    // Go: internal/binder/binder.go:addToContainerChain
    fn add_to_container_chain(&mut self, next: NodeId) {
        // The intrusive `NextContainer` chain is not observed by the test
        // surface; tracking `last_container` preserves the Go cursor semantics.
        self.last_container = Some(next);
    }

    // Go: internal/binder/binder.go:bindChildren
    fn bind_children(&mut self, node: NodeId) {
        let save_in_assignment_pattern = self.in_assignment_pattern;
        self.in_assignment_pattern = false;

        if self.current_flow == self.unreachable_flow {
            self.node_flow.remove(&node);
            if astquery::is_potentially_executable_node(self.arena, node) {
                self.arena.add_flags(node, NodeFlags::UNREACHABLE);
            }
            self.bind_each_child(node);
            self.in_assignment_pattern = save_in_assignment_pattern;
            return;
        }

        let kind = self.arena.kind(node);
        if kind >= Kind::FIRST_STATEMENT && kind <= Kind::LAST_STATEMENT {
            self.node_flow.insert(node, self.current_flow);
        }

        match kind {
            Kind::WhileStatement => self.bind_while_statement(node),
            Kind::DoStatement => self.bind_do_statement(node),
            Kind::ForStatement => self.bind_for_statement(node),
            Kind::ForInStatement | Kind::ForOfStatement => {
                self.bind_for_in_or_for_of_statement(node)
            }
            Kind::IfStatement => self.bind_if_statement(node),
            Kind::ReturnStatement => self.bind_return_statement(node),
            Kind::ThrowStatement => self.bind_throw_statement(node),
            Kind::BreakStatement => self.bind_break_statement(node),
            Kind::ContinueStatement => self.bind_continue_statement(node),
            Kind::TryStatement => self.bind_try_statement(node),
            Kind::SwitchStatement => self.bind_switch_statement(node),
            Kind::CaseBlock => self.bind_case_block(node),
            Kind::CaseClause | Kind::DefaultClause => self.bind_case_or_default_clause(node),
            Kind::ExpressionStatement => self.bind_expression_statement(node),
            Kind::LabeledStatement => self.bind_labeled_statement(node),
            Kind::PrefixUnaryExpression => self.bind_prefix_unary_expression_flow(node),
            Kind::PostfixUnaryExpression => self.bind_postfix_unary_expression_flow(node),
            Kind::BinaryExpression => {
                if flow::is_destructuring_assignment(self.arena, node) {
                    self.in_assignment_pattern = save_in_assignment_pattern;
                    self.bind_destructuring_assignment_flow(node);
                    return;
                }
                self.bind_binary_expression_flow(node);
            }
            Kind::ConditionalExpression => self.bind_conditional_expression_flow(node),
            Kind::VariableDeclaration => self.bind_variable_declaration_flow(node),
            Kind::SourceFile => {
                let (statements, eof) = match self.arena.data(node) {
                    NodeData::SourceFile(d) => (d.statements.nodes.clone(), d.end_of_file_token),
                    _ => unreachable!(),
                };
                self.bind_each_statement_functions_first(&statements);
                self.bind(eof);
            }
            Kind::Block | Kind::ModuleBlock => {
                let statements = self.statement_list(node);
                self.bind_each_statement_functions_first(&statements);
            }
            Kind::BindingElement => self.bind_binding_element_flow(node),
            Kind::Parameter => self.bind_parameter_flow(node),
            _ => self.bind_each_child(node),
        }
        self.in_assignment_pattern = save_in_assignment_pattern;
    }

    fn statement_list(&self, node: NodeId) -> Vec<NodeId> {
        match self.arena.data(node) {
            NodeData::Block(d) => d.list.nodes.clone(),
            NodeData::ModuleBlock(d) => d.statements.nodes.clone(),
            _ => Vec::new(),
        }
    }

    // Go: internal/binder/binder.go:bindEachChild
    fn bind_each_child(&mut self, node: NodeId) {
        let mut kids = Vec::new();
        self.arena.for_each_child(node, &mut |c| {
            kids.push(c);
            false
        });
        for c in kids {
            self.bind(c);
        }
    }

    // Go: internal/binder/binder.go:bindEach
    fn bind_each(&mut self, nodes: &[NodeId]) {
        for &n in nodes {
            self.bind(n);
        }
    }

    // Go: internal/binder/binder.go:bindEachStatementFunctionsFirst
    fn bind_each_statement_functions_first(&mut self, statements: &[NodeId]) {
        for &n in statements {
            if self.arena.kind(n) == Kind::FunctionDeclaration {
                self.bind(n);
            }
        }
        for &n in statements {
            if self.arena.kind(n) != Kind::FunctionDeclaration {
                self.bind(n);
            }
        }
    }
}

/// Removes a known TypeScript/JavaScript file extension from `path`.
///
/// A minimal stand-in for `tspath.RemoveFileExtension` sufficient for the
/// module name the binder synthesizes for an external module.
// Go: internal/tspath/extension.go:RemoveFileExtension
fn remove_file_extension(path: &str) -> String {
    for ext in [
        ".d.ts", ".d.mts", ".d.cts", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs",
        ".json",
    ] {
        if let Some(stem) = path.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    path.to_string()
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
