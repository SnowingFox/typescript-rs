//! Unreachable-code detection (`TS7027`).
//!
//! Ports Go's `checkSourceElementUnreachable` / `isSourceElementUnreachable`
//! (`internal/checker/checker.go`) plus the AST predicates they consult that
//! Go houses in `internal/ast/utilities.go`
//! (`IsPotentiallyExecutableNode` / `GetModuleInstanceState` /
//! `IsInstantiatedModule` / `IsEnumConst`).
//!
//! The binder already stamps [`NodeFlags::UNREACHABLE`] on every
//! potentially-executable statement bound while the control flow is the
//! unreachable flow (`tsgo_binder::Binder::bind_children`), and exposes a
//! statement's flow node through [`BoundProgram::flow_node_of`]. The checker
//! reads those to report `Unreachable code detected.` on the FIRST statement of
//! each maximal unreachable run (the run is collapsed into one diagnostic that
//! spans from the first statement to the last), gated on
//! `allowUnreachableCode != true`.
//!
//! Category: Go routes the diagnostic through `addErrorOrSuggestion(isError =
//! allowUnreachableCode == false, ...)`. When `allowUnreachableCode` is
//! explicitly `false` it is an ERROR (and lands in the `.errors.txt` baseline);
//! when it is unset it is a SUGGESTION that Go stores in a SEPARATE
//! `suggestionDiagnostics` collection (never part of `.errors.txt`). This port
//! models only the error collection, so the suggestion variant is computed-then-
//! dropped (see [`Checker::check_source_element_unreachable`]); the error path
//! is byte-faithful.

use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId};

use super::check::Diagnostic;
use super::declared_types::combined_node_flags;
use super::program::BoundProgram;
use super::Checker;

impl Checker {
    /// Reports `TS7027 Unreachable code detected.` on `node` when it is the
    /// first statement of an unreachable run, returning whether `node` is
    /// unreachable (so the caller suppresses re-reporting inside its subtree).
    ///
    /// Mirrors Go's `checkSourceElementUnreachable`: a non-potentially-executable
    /// node (an interface / type alias / etc.) is never unreachable; a node
    /// already swallowed by an earlier run's forward scan is unreachable but not
    /// re-reported; otherwise, when the node is unreachable, the maximal
    /// contiguous run of unreachable potentially-executable siblings in the
    /// enclosing statement list is collapsed into ONE diagnostic spanning the
    /// first statement's token start to the last statement's end.
    ///
    /// Side effects: records the run's nodes in
    /// `reported_unreachable_nodes`; adds the diagnostic (only when it is an
    /// error — `allowUnreachableCode == false`).
    // Go: internal/checker/checker.go:Checker.checkSourceElementUnreachable(2374)
    pub(crate) fn check_source_element_unreachable(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        if !is_potentially_executable_node(program, node) {
            return false;
        }
        if self.reported_unreachable_nodes.contains(&node) {
            return true;
        }
        if !self.is_source_element_unreachable(program, node) {
            return false;
        }
        self.reported_unreachable_nodes.insert(node);

        let arena = program.arena();
        let mut start_node = node;
        let mut end_node = node;

        // Collapse the maximal run of consecutive unreachable, potentially-
        // executable siblings into a single diagnostic (Go scans forward from
        // the node's offset; the backward scan is gated behind region
        // diagnostics, which are not yet supported, so it stays disabled — see
        // Go's commented-out block).
        if let Some(parent) = arena.parent(node) {
            if can_have_statements(arena.kind(parent)) {
                let statements = statements_of(arena, parent);
                if let Some(offset) = statements.iter().position(|&s| s == node) {
                    let mut last = offset;
                    for (i, &next) in statements.iter().enumerate().skip(offset + 1) {
                        if !is_potentially_executable_node(program, next)
                            || !self.is_source_element_unreachable(program, next)
                        {
                            break;
                        }
                        self.reported_unreachable_nodes.insert(next);
                        last = i;
                    }
                    start_node = statements[offset];
                    end_node = statements[last];
                }
            }
        }

        // Span: `GetTokenPosOfNode(startNode)` (skip leading trivia) to
        // `endNode.End()`. A statement node is neither missing nor JSX text, so
        // `GetTokenPosOfNode` reduces to `SkipTrivia(text, pos)`.
        let arena = program.arena();
        let start = match program.source_text() {
            Some(text) => tsgo_scanner::skip_trivia(text, arena.loc(start_node).pos()),
            None => arena.loc(start_node).pos(),
        };
        let end = arena.loc(end_node).end();

        // `addErrorOrSuggestion(isError = allowUnreachableCode == false, ...)`:
        // the suggestion variant lives in a separate collection that is not part
        // of the error baseline, so it is computed-then-dropped here (no
        // suggestion sink is modeled — DEFER).
        let is_error =
            self.compiler_options().allow_unreachable_code == tsgo_core::tristate::Tristate::False;
        if is_error {
            let message = &tsgo_diagnostics::UNREACHABLE_CODE_DETECTED;
            let diagnostic = Diagnostic {
                code: message.code(),
                category: message.category(),
                message: tsgo_diagnostics::format(&message.to_string(), &[]),
                start,
                length: end - start,
                related_information: Vec::new(),
                message_chain: Vec::new(),
            };
            self.add_diagnostic(program, diagnostic);
        }

        true
    }

    /// Reports whether `node` is unreachable (Go's `isSourceElementUnreachable`).
    ///
    /// Precondition: `is_potentially_executable_node(node)` (the caller checks
    /// it). A node the binder flagged [`NodeFlags::UNREACHABLE`] is unreachable
    /// unless it is a const enum without `preserveConstEnums`, or an
    /// uninstantiated module; a node the binder did not flag is unreachable when
    /// its flow node cannot reach the control-flow start.
    ///
    /// Side effects: none (reads the arena / flow graph).
    // Go: internal/checker/checker.go:Checker.isSourceElementUnreachable(2435)
    fn is_source_element_unreachable(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        if arena.flags(node).contains(NodeFlags::UNREACHABLE) {
            // The binder has determined that this code is unreachable. Ignore
            // const enums unless preserveConstEnums is set, and uninstantiated
            // modules.
            match arena.kind(node) {
                Kind::EnumDeclaration => {
                    !is_enum_const(arena, node)
                        || self.compiler_options().should_preserve_const_enums()
                }
                Kind::ModuleDeclaration => is_instantiated_module(
                    arena,
                    node,
                    self.compiler_options().should_preserve_const_enums(),
                ),
                _ => true,
            }
        } else if let Some(flow) = program.flow_node_of(node) {
            // For code the binder doesn't know is unreachable, use control flow.
            !self.is_reachable_flow_node(program, flow)
        } else {
            false
        }
    }
}

/// Reports whether `node` might be executed at runtime (Go's
/// `IsPotentiallyExecutableNode`): every statement except a `var`/`let`/`const`
/// declaration with neither a block-scoped binding nor an initialized
/// declarator, plus class / enum / module declarations.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsPotentiallyExecutableNode(4186)
pub(crate) fn is_potentially_executable_node(program: &dyn BoundProgram, node: NodeId) -> bool {
    let arena = program.arena();
    let kind = arena.kind(node);
    if kind >= Kind::FIRST_STATEMENT && kind <= Kind::LAST_STATEMENT {
        if let NodeData::VariableStatement(d) = arena.data(node) {
            let list = d.declaration_list;
            if combined_node_flags(program, list).intersects(NodeFlags::BLOCK_SCOPED) {
                return true;
            }
            if let NodeData::VariableDeclarationList(ld) = arena.data(list) {
                return ld.declarations.nodes.iter().any(|&decl| {
                    matches!(
                        arena.data(decl),
                        NodeData::VariableDeclaration(vd) if vd.initializer.is_some()
                    )
                });
            }
            return false;
        }
        return true;
    }
    matches!(
        kind,
        Kind::ClassDeclaration | Kind::EnumDeclaration | Kind::ModuleDeclaration
    )
}

/// Reports whether `kind` can hold a statement list (Go's
/// `Node.CanHaveStatements`): source files, blocks, module blocks, and
/// case / default clauses.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.CanHaveStatements(592)
fn can_have_statements(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::SourceFile | Kind::Block | Kind::ModuleBlock | Kind::CaseClause | Kind::DefaultClause
    )
}

/// Returns the statements of a statement-list container (Go's
/// `Node.Statements`), or an empty vector for any other kind.
///
/// Side effects: none (clones the node-id list).
// Go: internal/ast/ast.go:Node.Statements(584)
fn statements_of(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    match arena.data(node) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        NodeData::Block(d) => d.list.nodes.clone(),
        NodeData::ModuleBlock(d) => d.statements.nodes.clone(),
        NodeData::CaseOrDefaultClause(d) => d.statements.nodes.clone(),
        _ => Vec::new(),
    }
}

/// The instantiation state of a namespace body (Go's `ModuleInstanceState`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModuleInstanceState {
    /// Contains only type declarations (no runtime form).
    NonInstantiated,
    /// Contains only const enums (a runtime form only under
    /// `preserveConstEnums`).
    ConstEnumOnly,
    /// Has a runtime form.
    Instantiated,
}

/// Reports whether a `ModuleDeclaration` `node` is bound by the binder as a
/// `ValueModule` (a runtime "value" namespace) rather than a type-only
/// `NamespaceModule`.
///
/// Mirrors the binder's `declareModuleSymbol` classification, which uses
/// `instantiated := GetModuleInstanceState(node) != NonInstantiated` to pick
/// `ValueModule` vs `NamespaceModule` — so a const-enum-only module is a
/// ValueModule too (independent of `preserveConstEnums`, unlike the emit-time
/// [`is_instantiated_module`]). The TS2309 export-conflict membership predicate
/// uses this to undo the Rust binder's over-broad `VALUE_MODULE` assignment
/// (the `ValueModule`-vs-`NamespaceModule` split is DEFERRED in the binder).
///
/// Side effects: none (reads the arena).
// Go: internal/binder/binder.go:Binder.declareModuleSymbol (instantiated boolean)
pub(crate) fn module_is_value_module(arena: &NodeArena, node: NodeId) -> bool {
    module_instance_state(arena, node) != ModuleInstanceState::NonInstantiated
}

/// Reports whether `node` (a `ModuleDeclaration`) produces a runtime form (Go's
/// `IsInstantiatedModule`): instantiated, or const-enum-only under
/// `preserve_const_enums`.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsInstantiatedModule(2409)
fn is_instantiated_module(arena: &NodeArena, node: NodeId, preserve_const_enums: bool) -> bool {
    let state = module_instance_state(arena, node);
    state == ModuleInstanceState::Instantiated
        || (preserve_const_enums && state == ModuleInstanceState::ConstEnumOnly)
}

/// Computes a module declaration's instantiation state (Go's
/// `getModuleInstanceState`): the state of its body, or `Instantiated` when it
/// has no body (an ambient/external module reference).
// Go: internal/ast/utilities.go:getModuleInstanceState(2292)
fn module_instance_state(arena: &NodeArena, node: NodeId) -> ModuleInstanceState {
    let body = match arena.data(node) {
        NodeData::ModuleDeclaration(d) => d.body,
        _ => return ModuleInstanceState::Instantiated,
    };
    match body {
        Some(body) => module_instance_state_worker(arena, body),
        None => ModuleInstanceState::Instantiated,
    }
}

/// The body of Go's `getModuleInstanceStateWorker`: a module body is
/// uninstantiated when it contains only interfaces / type aliases (and certain
/// non-exported imports), const-enum-only when its sole runtime forms are const
/// enums, else instantiated.
///
/// DEFER(phase-4-checker): the `export { x }` named-export specifier's
/// alias-target analysis (`getModuleInstanceStateForAliasTarget`) — it resolves
/// each specifier to its declaration and folds its state; the port falls through
/// to Go's conservative `Instantiated` default for that arm (none of the corpus
/// unreachable-namespace cases exercise it). blocked-by: alias-target
/// resolution over the enclosing statement list.
// Go: internal/ast/utilities.go:getModuleInstanceStateWorker(2318)
fn module_instance_state_worker(arena: &NodeArena, node: NodeId) -> ModuleInstanceState {
    match arena.kind(node) {
        Kind::InterfaceDeclaration | Kind::TypeAliasDeclaration | Kind::JSTypeAliasDeclaration => {
            ModuleInstanceState::NonInstantiated
        }
        Kind::EnumDeclaration => {
            if is_enum_const(arena, node) {
                ModuleInstanceState::ConstEnumOnly
            } else {
                ModuleInstanceState::Instantiated
            }
        }
        Kind::ImportDeclaration | Kind::JSImportDeclaration | Kind::ImportEqualsDeclaration => {
            if !node_modifier_flags(arena, node).contains(ModifierFlags::EXPORT) {
                ModuleInstanceState::NonInstantiated
            } else {
                ModuleInstanceState::Instantiated
            }
        }
        Kind::ModuleBlock => {
            // Reduce over the block's children: any instantiated child makes the
            // block instantiated (Go returns early); a const-enum child raises
            // the state to const-enum-only; a non-instantiated child leaves it.
            let statements = statements_of(arena, node);
            let mut state = ModuleInstanceState::NonInstantiated;
            for child in statements {
                match module_instance_state_worker(arena, child) {
                    ModuleInstanceState::NonInstantiated => {}
                    ModuleInstanceState::ConstEnumOnly => {
                        state = ModuleInstanceState::ConstEnumOnly;
                    }
                    ModuleInstanceState::Instantiated => {
                        return ModuleInstanceState::Instantiated;
                    }
                }
            }
            state
        }
        Kind::ModuleDeclaration => module_instance_state(arena, node),
        _ => ModuleInstanceState::Instantiated,
    }
}

/// Reports whether `node` is a `const enum` declaration (Go's `IsEnumConst`).
// Go: internal/ast/utilities.go:IsEnumConst(1834)
fn is_enum_const(arena: &NodeArena, node: NodeId) -> bool {
    arena.kind(node) == Kind::EnumDeclaration
        && node_modifier_flags(arena, node).contains(ModifierFlags::CONST)
}

/// Returns the modifier flags of the declaration kinds consulted by the
/// module-instance-state walk (enum / import / export / import-equals), or empty
/// when `node` bears no modifier list.
fn node_modifier_flags(arena: &NodeArena, node: NodeId) -> ModifierFlags {
    let modifiers = match arena.data(node) {
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportEqualsDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers
        .map(|m| m.modifier_flags)
        .unwrap_or(ModifierFlags::empty())
}

#[cfg(test)]
#[path = "reachability_test.rs"]
mod tests;
