//! Port of Go `internal/transformers/declarations/transform.go`: the
//! `DeclarationTransformer`, which turns a checked TypeScript source file into
//! its `.d.ts` (declaration) shape — function/method bodies removed, variable
//! initializers removed, `declare` synthesized at the top level, `export`
//! preserved, type annotations kept.
//!
//! # Scope (round D-F1: declarations CORE)
//!
//! This round ports the *annotated-declaration* core: declarations whose type
//! node already exists in the source are copied as-is (no inferred-type
//! synthesis). Covered top-level kinds: function declaration → ambient
//! signature, variable statement → declared variable (initializer stripped),
//! class declaration → ambient class (member bodies/initializers stripped,
//! parameter properties hoisted, accessors/constructor handled, `private`
//! members emitted name-only), and interface / type-alias passthrough. Modifier
//! handling mirrors Go's `ensureModifiers`/`ensureModifierFlags`/
//! `maskModifierFlags`: `export` preserved, `declare` added once at the top
//! level (never on interfaces / class members / `default` exports), `public`/
//! `async`/`override` dropped.
//!
//! The [`EmitResolver`](tsgo_checker::EmitResolver) is wired through the
//! [`EmitReferenceResolver`](crate::EmitReferenceResolver) handle (the same
//! pattern import-elision uses) for the reachable
//! [`is_implementation_of_overload`](crate::EmitReferenceResolver::is_implementation_of_overload)
//! query, which elides an overload set's implementation signature.
//!
//! # Round D-F2: inferred-type node synthesis
//!
//! `ensure_type` now fills the inferred-type hole: when a declaration has no
//! explicit annotation, it asks the [`EmitReferenceResolver`] to synthesize one
//! (`create_type_of_declaration` for a variable / property / parameter,
//! `create_return_type_of_signature_declaration` for a function-like return
//! type) and reconstructs the returned
//! [`SynthesizedTypeNode`](tsgo_checker::SynthesizedTypeNode) descriptor into the
//! `.d.ts` arena. A *literal* `const` instead keeps its initializer verbatim
//! (`should_print_with_initializer` / `ensure_no_initializer` consult
//! `is_literal_const_declaration` + `create_literal_const_value`), mirroring Go.
//! So `let n = 1` → `declare let n: number;`, `const x = 1` →
//! `declare const x = 1;`, `function f() { return 1; }` →
//! `declare function f(): number;`, `class C { x = 1; }` → `x: number;`.
//!
//! # Deferred (with blocked-by)
//!
//! - **Visibility / accessibility gating + isolatedDeclarations diagnostics →
//!   D-F3.** Every reachable top-level declaration is emitted; non-exported
//!   declarations are not elided (the script-vs-module visibility split and the
//!   late-painted import-alias dance are not modelled), and no diagnostics are
//!   produced. blocked-by: `EmitResolver::is_declaration_visible` script-file
//!   case + `PrecalculateDeclarationEmitVisibility` + the `SymbolTracker`
//!   (`tracker.go` / `diagnostics.go`).
//! - **Literal-const initializer preservation** (`declare const x = 1`): the
//!   `shouldPrintWithInitializer` path is treated as `false`, so initializers
//!   are always stripped. blocked-by: `EmitResolver::is_literal_const_declaration`
//!   + `CreateLiteralConstValue`.
//! - **Enum, namespace/module, import/export, `export =`/`export default`,
//!   index signatures, declaration-map, triple-slash directives, JS/expando,
//!   binding-pattern variables/parameter-properties.** blocked-by: enum
//!   constant folding wiring / module-specifier rewriting / the resolver host.

use crate::{new_transformer, EmitReferenceResolver, TransformOptions, Transformer};
use std::cell::RefCell;
use std::rc::Rc;
use tsgo_ast::{
    Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeId, NodeList, TokenFlags,
};
use tsgo_checker::{Diagnostic, LiteralConstValue, SynthesizedProperty, SynthesizedTypeNode};
use tsgo_printer::EmitContext;

use super::diagnostics::{
    get_error_by_declaration_kind, get_related_suggestion_by_declaration_kind,
    get_symbol_accessibility_diagnostic_message,
};
use super::tracker::{create_diagnostic_for_node, SymbolTracker};
use super::util::{
    effective_declaration_flags, get_binding_name_visible, get_first_constructor_with_body,
    get_this_parameter, has_parameter_property_modifier, is_always_type,
    is_declaration_and_not_visible, is_external_module, is_external_module_indicator, is_modifier,
    is_optional_parameter, is_scope_marker, mask_modifier_flags, modifier_kinds_from_flags,
    modifiers_to_flags, needs_scope_marker, node_modifiers,
};

/// A shared handle over the declaration-emit diagnostics the transform produces
/// (Go's `DeclarationTransformer.GetDiagnostics` backing slice).
///
/// Side effects: none (a type alias).
pub type DeclarationDiagnostics = Rc<RefCell<Vec<Diagnostic>>>;

/// The stateful declaration (`.d.ts`) transform (Go's `DeclarationTransformer`).
///
/// Holds the running `needs_declare` flag (whether a synthesized `declare` is
/// required for the current context) and the optional reference resolver for the
/// reachable emit-resolver queries.
///
/// Side effects: [`transform_source_file`](Transformer::transform_source_file)
/// mutates the shared emit context's arena.
// Go: internal/transformers/declarations/transform.go:DeclarationTransformer
pub struct DeclarationsTransformer {
    /// Whether the current declaration needs a synthesized `declare` (ambient)
    /// modifier — true at the top level of a non-declaration file.
    needs_declare: bool,
    /// The reachable emit-resolver handle, when wired (Go threads the resolver
    /// through the `DeclarationEmitHost`).
    resolver: Option<EmitReferenceResolver>,
    /// Whether the output already carries an external-module indicator (an
    /// import/export declaration, an export assignment, or an `export`-modified
    /// statement). Drives the trailing `export {};` scope-fix marker.
    result_has_external_module_indicator: bool,
    /// Whether the output already carries a scope marker (an export declaration
    /// or export assignment), Go's `resultHasScopeMarker`.
    result_has_scope_marker: bool,
    /// Whether the output contains a non-exported (and non-import) statement
    /// that needs a scope-fix marker to stay module-local, Go's
    /// `needsScopeFixMarker`.
    needs_scope_fix_marker: bool,
    /// Whether `--isolatedDeclarations` is enabled (drives the
    /// explicit-annotation diagnostics).
    isolated_declarations: bool,
    /// The accessibility / inference-fallback diagnostics sink (Go's
    /// `SymbolTrackerImpl` + shared state).
    tracker: SymbolTracker,
}

/// Builds a [`Transformer`] that lowers a source file to its `.d.ts` shape,
/// sharing the pipeline's emit context. `resolver`, when present, drives the
/// reachable emit-resolver queries (overload-implementation elision, non-exported
/// elision, import/export visibility, accessibility diagnostics).
///
/// # Examples
/// ```
/// use tsgo_transformers::{declarations::transform::new_declarations_transformer, TransformOptions};
/// // Constructed with a fresh context and no resolver.
/// let _tx = new_declarations_transformer(&TransformOptions::default(), None);
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/declarations/transform.go:NewDeclarationTransformer
pub fn new_declarations_transformer(
    opt: &TransformOptions,
    resolver: Option<EmitReferenceResolver>,
) -> Transformer {
    new_declarations_transformer_with_diagnostics(opt, resolver).0
}

/// Like [`new_declarations_transformer`], but additionally returns the shared
/// [`DeclarationDiagnostics`] sink the transform records accessibility /
/// `--isolatedDeclarations` diagnostics into (Go's
/// `DeclarationTransformer.GetDiagnostics`). `--isolatedDeclarations` is read
/// from `opt.compiler_options.isolated_declarations`.
///
/// # Examples
/// ```
/// use tsgo_transformers::{declarations::transform::new_declarations_transformer_with_diagnostics, TransformOptions};
/// let (_tx, diags) = new_declarations_transformer_with_diagnostics(&TransformOptions::default(), None);
/// assert!(diags.borrow().is_empty());
/// ```
///
/// Side effects: allocates a transformer over the shared context plus a shared
/// diagnostics sink.
// Go: internal/transformers/declarations/transform.go:NewDeclarationTransformer + GetDiagnostics
pub fn new_declarations_transformer_with_diagnostics(
    opt: &TransformOptions,
    resolver: Option<EmitReferenceResolver>,
) -> (Transformer, DeclarationDiagnostics) {
    let diagnostics: DeclarationDiagnostics = Rc::new(RefCell::new(Vec::new()));
    let mut state = DeclarationsTransformer {
        needs_declare: false,
        resolver,
        result_has_external_module_indicator: false,
        result_has_scope_marker: false,
        needs_scope_fix_marker: false,
        isolated_declarations: opt.compiler_options.isolated_declarations.is_true(),
        tracker: SymbolTracker::new(Rc::clone(&diagnostics)),
    };
    let transformer = new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            state.visit_source_file(ec.arena_mut(), node)
        }),
        opt.context.clone(),
    );
    (transformer, diagnostics)
}

impl DeclarationsTransformer {
    /// Visits the root source file, rebuilding it as a declaration file: each
    /// top-level statement is transformed (or elided), and the result is marked
    /// `is_declaration_file`.
    ///
    /// Side effects: pushes the rebuilt statements and source file onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.visitSourceFile / transformSourceFile
    fn visit_source_file(&mut self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let (file_name, script_kind, language_variant, statements, end_of_file_token, is_dts) =
            match arena.data(node) {
                NodeData::SourceFile(d) => (
                    d.file_name.clone(),
                    d.script_kind,
                    d.language_variant,
                    d.statements.clone(),
                    d.end_of_file_token,
                    d.is_declaration_file,
                ),
                _ => return node,
            };
        // Go: a declaration file is returned unchanged.
        if is_dts {
            return node;
        }
        // Reset the per-file scope/module-marker bookkeeping (Go's visitSourceFile).
        self.result_has_external_module_indicator = false;
        self.result_has_scope_marker = false;
        self.needs_scope_fix_marker = false;
        let is_module = is_external_module(arena, node);
        let mut out: Vec<NodeId> = Vec::with_capacity(statements.nodes.len());
        for &stmt in &statements.nodes {
            // Go: visitSourceFile sets needsDeclare = true; each top-level
            // declaration runs under that context.
            self.needs_declare = true;
            if let Some(result) = self.transform_top_level_statement(arena, stmt) {
                match arena.data(result) {
                    // A transform that produced multiple statements bundles them
                    // in a SyntaxList; flatten it into the output.
                    NodeData::SyntaxList(d) => {
                        for elem in d.list.nodes.clone() {
                            self.record_marker_flags(arena, elem);
                            out.push(elem);
                        }
                    }
                    _ => {
                        self.record_marker_flags(arena, result);
                        out.push(result);
                    }
                }
            }
        }
        // Go: transformSourceFile appends an empty `export {};` marker to a
        // module whose output has no external-module indicator, or whose
        // non-exported (scope-fixed) statements need one to stay module-local.
        if is_module
            && (!self.result_has_external_module_indicator
                || (self.needs_scope_fix_marker && !self.result_has_scope_marker))
        {
            let marker = create_empty_exports(arena);
            out.push(marker);
        }
        let new_sf = arena.new_source_file(
            &file_name,
            script_kind,
            language_variant,
            NodeList::new(out),
            end_of_file_token,
        );
        if let NodeData::SourceFile(d) = arena.data_mut(new_sf) {
            d.is_declaration_file = true;
        }
        new_sf
    }

    /// Records the scope/module-marker flags contributed by emitted statement
    /// `stmt` (Go's per-statement inspection in
    /// `transformAndReplaceLatePaintedStatements`).
    ///
    /// Side effects: updates the transformer's marker flags.
    fn record_marker_flags(&mut self, arena: &NodeArena, stmt: NodeId) {
        if is_external_module_indicator(arena, stmt) {
            self.result_has_external_module_indicator = true;
        }
        if needs_scope_marker(arena, stmt) {
            self.needs_scope_fix_marker = true;
        }
        if is_scope_marker(arena, stmt) {
            self.result_has_scope_marker = true;
        }
    }

    /// Transforms a direct child of the source file into its replacement
    /// declaration statement, or `None` when it has no `.d.ts` form.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.visit / transformTopLevelDeclaration
    fn transform_top_level_statement(
        &mut self,
        arena: &mut NodeArena,
        node: NodeId,
    ) -> Option<NodeId> {
        // Go: transformTopLevelDeclaration elides a declaration that exists but
        // is not visible to declaration emit (a non-exported top-level
        // declaration in a module; a global script keeps it). Only applied when
        // a resolver is wired — the bare-context path keeps every declaration
        // (the pre-D-F3 behavior).
        if let Some(resolver) = self.resolver.as_ref() {
            if is_declaration_and_not_visible(arena, resolver, node) {
                return None;
            }
        }
        match arena.kind(node) {
            Kind::FunctionDeclaration => {
                // Go: transformTopLevelDeclaration elides the implementation
                // signature of an overload set.
                let elide = self
                    .resolver
                    .as_ref()
                    .is_some_and(|r| r.is_implementation_of_overload(node));
                if elide {
                    return None;
                }
                Some(self.transform_function_declaration(arena, node))
            }
            Kind::VariableStatement => self.transform_variable_statement(arena, node),
            Kind::ClassDeclaration => Some(self.transform_class_declaration(arena, node)),
            Kind::InterfaceDeclaration => Some(self.transform_interface_declaration(arena, node)),
            Kind::TypeAliasDeclaration => Some(self.transform_type_alias_declaration(arena, node)),
            Kind::ImportDeclaration => self.transform_import_declaration(arena, node),
            Kind::ExportDeclaration => Some(self.transform_export_declaration(arena, node)),
            Kind::ExportAssignment => self.transform_export_assignment(arena, node),
            // Statements with no declaration form are elided (Go's `visit`
            // elision list).
            _ => None,
        }
    }

    // --- top-level declaration kinds -------------------------------------

    /// Transforms a function declaration into its ambient signature: body
    /// removed, `declare`/`export` ensured, type parameters / parameters /
    /// return type kept (annotated path).
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformFunctionDeclaration
    fn transform_function_declaration(&self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let (name, type_parameters, parameters) = match arena.data(node) {
            NodeData::FunctionDeclaration(d) => {
                (d.name, d.type_parameters.clone(), d.parameters.clone())
            }
            _ => unreachable!("kind/data mismatch"),
        };
        // Go: transformFunctionDeclaration -> ensureType -> the node builder
        // reports an inference fallback under --isolatedDeclarations when the
        // return type needs an explicit annotation.
        self.report_isolated_declarations_return_type(arena, node);
        let modifiers = self.ensure_modifiers(arena, node);
        let type_parameters = self.ensure_type_params(arena, node, type_parameters);
        let parameters = self.update_param_list(arena, node, &parameters);
        let ret = self.ensure_type(arena, node, false);
        arena.new_function_declaration(
            modifiers,
            None,
            name,
            type_parameters,
            parameters,
            ret,
            None,
            None,
        )
    }

    /// Reports the `--isolatedDeclarations` "must have an explicit return type
    /// annotation" diagnostic (`9007` for a function, `9008` for a method) when
    /// `node` (a function-like declaration) has no return-type annotation and a
    /// body whose return type is not syntactically derivable.
    ///
    /// Reachable subset: a body with no value-returning `return` statement (a
    /// `void` body). DEFER(D-F3): the conditional / multiple-return cases (Go
    /// still reports `9007`), the non-literal single-return `9013`, and the
    /// private-name-in-return `9039`/parameter `9011` family — those need the
    /// pseudo-type node builder's per-construct inference-fallback reporting.
    ///
    /// Side effects: may record a diagnostic (with a related suggestion) on the
    /// tracker.
    // Go: internal/transformers/declarations/{tracker.go:ReportInferenceFallback, diagnostics.go:createReturnTypeError}
    fn report_isolated_declarations_return_type(&self, arena: &NodeArena, node: NodeId) {
        if !self.isolated_declarations {
            return;
        }
        // An explicit return-type annotation suppresses the diagnostic.
        if node_type_annotation(arena, node).is_some() {
            return;
        }
        // Only a body-bearing declaration infers a return type.
        let Some(body) = function_body(arena, node) else {
            return;
        };
        // A value-returning body yields a (possibly serializable) type; the
        // reachable subset reports only the void / no-value-return case.
        if body_has_value_return(arena, body) {
            return;
        }
        let Some(message) = get_error_by_declaration_kind(arena.kind(node)) else {
            return;
        };
        let mut diag = create_diagnostic_for_node(arena, node, message, &[]);
        if let Some(suggestion) = get_related_suggestion_by_declaration_kind(arena.kind(node)) {
            diag.add_related_info(create_diagnostic_for_node(arena, node, suggestion, &[]));
        }
        self.tracker.add_diagnostic(diag);
    }

    /// Transforms a variable statement into its declared form: initializers
    /// removed, `declare`/`export` ensured, type annotations kept. Returns
    /// `None` when the statement declares nothing emittable.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformVariableStatement
    fn transform_variable_statement(&self, arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
        let declaration_list = match arena.data(node) {
            NodeData::VariableStatement(d) => d.declaration_list,
            _ => unreachable!("kind/data mismatch"),
        };
        let (decls, list_flags) = match arena.data(declaration_list) {
            NodeData::VariableDeclarationList(d) => {
                (d.declarations.nodes.clone(), arena.flags(declaration_list))
            }
            _ => unreachable!("kind/data mismatch"),
        };
        // Go: transformVariableStatement elides the whole statement when none of
        // its declarations binds a name visible to declaration emit (so a
        // non-exported `const b = 2;` in a module is dropped). Gated on a wired
        // resolver — the bare-context path keeps every declaration.
        if let Some(resolver) = self.resolver.as_ref() {
            let any_visible = decls
                .iter()
                .any(|&decl| get_binding_name_visible(arena, resolver, decl));
            if !any_visible {
                return None;
            }
        }
        let new_decls: Vec<NodeId> = decls
            .iter()
            .map(|&decl| self.transform_variable_declaration(arena, decl))
            .collect();
        if new_decls.is_empty() {
            return None;
        }
        let new_list = arena.new_variable_declaration_list(NodeList::new(new_decls));
        // Go: UpdateVariableDeclarationList preserves the list's `let`/`const`
        // node flags (which the printer reads to choose the keyword).
        arena.set_flags(new_list, list_flags);
        let modifiers = self.ensure_modifiers(arena, node);
        Some(arena.new_variable_statement(modifiers, new_list))
    }

    /// Transforms a single variable declaration: type annotation kept,
    /// initializer and `!` token removed.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformVariableDeclaration
    fn transform_variable_declaration(&self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let name = match arena.data(node) {
            NodeData::VariableDeclaration(d) => d.name,
            _ => unreachable!("kind/data mismatch"),
        };
        let type_node = self.ensure_type(arena, node, false);
        let initializer = self.ensure_no_initializer(arena, node);
        // Go: ensureType visits the annotation, whose entity-name nodes run
        // through checkEntityNameVisibility (reporting a private-name error).
        self.check_declaration_type_visibility(arena, node);
        arena.new_variable_declaration(name, None, type_node, initializer)
    }

    /// Checks the entity names of `node`'s type annotation for accessibility,
    /// recording a "has or is using private name" diagnostic when one references
    /// a symbol not visible to declaration emit (Go's `checkEntityNameVisibility`
    /// reached through `ensureType`'s annotation visit). Reachable subset:
    /// `typeof <name>` type queries.
    ///
    /// Side effects: may record diagnostics on the tracker.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.checkEntityNameVisibility
    fn check_declaration_type_visibility(&self, arena: &NodeArena, node: NodeId) {
        let Some(resolver) = self.resolver.as_ref() else {
            return;
        };
        let Some(annotation) = node_type_annotation(arena, node) else {
            return;
        };
        self.walk_type_entity_names(arena, resolver, annotation, node);
    }

    /// Walks type node `type_node`, checking each `typeof <name>` query's entity
    /// name for visibility against the declaration `context` (whose name and
    /// kind select the diagnostic). DEFER(D-F3): type references / heritage /
    /// import types (top-level type references late-paint rather than error).
    ///
    /// Side effects: may record diagnostics on the tracker.
    fn walk_type_entity_names(
        &self,
        arena: &NodeArena,
        resolver: &EmitReferenceResolver,
        type_node: NodeId,
        context: NodeId,
    ) {
        if arena.kind(type_node) == Kind::TypeQuery {
            let expr_name = match arena.data(type_node) {
                NodeData::TypeQuery(d) => d.expr_name,
                _ => return,
            };
            if let Some(first) = first_identifier(arena, expr_name) {
                if let Some(error_name) = resolver.entity_name_accessibility(first) {
                    self.report_inaccessible_entity_name(arena, expr_name, context, &error_name);
                }
            }
            return;
        }
        for child in collect_children(arena, type_node) {
            self.walk_type_entity_names(arena, resolver, child, context);
        }
    }

    /// Records the "has or is using private name" diagnostic for declaration
    /// `context` whose annotation references the inaccessible entity name at
    /// `error_node` (named `error_name`), Go's
    /// `handleSymbolAccessibilityError` reachable subset.
    ///
    /// Side effects: may record a diagnostic on the tracker.
    fn report_inaccessible_entity_name(
        &self,
        arena: &NodeArena,
        error_node: NodeId,
        context: NodeId,
        error_name: &str,
    ) {
        let Some(message) = get_symbol_accessibility_diagnostic_message(arena, context) else {
            return;
        };
        let type_name = declaration_name_text(arena, context);
        let diag =
            create_diagnostic_for_node(arena, error_node, message, &[&type_name, error_name]);
        self.tracker.add_diagnostic(diag);
    }

    /// Transforms a class declaration into an ambient class: member bodies and
    /// initializers removed, parameter properties hoisted to fields, `private`
    /// members emitted name-only, `declare`/`export` ensured.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformClassDeclaration
    fn transform_class_declaration(&self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let (name, type_parameters, heritage_clauses, members) = match arena.data(node) {
            NodeData::ClassDeclaration(d) => (
                d.name,
                d.type_parameters.clone(),
                d.heritage_clauses.clone(),
                d.members.nodes.clone(),
            ),
            _ => unreachable!("kind/data mismatch"),
        };
        let modifiers = self.ensure_modifiers(arena, node);
        let type_parameters = self.ensure_type_params(arena, node, type_parameters);

        let mut member_nodes: Vec<NodeId> = Vec::new();
        // Hoist constructor parameter properties to class fields (in source
        // order), before the rest of the members.
        if let Some(ctor) = get_first_constructor_with_body(arena, &members) {
            let params = match arena.data(ctor) {
                NodeData::ConstructorDeclaration(d) => d.parameters.nodes.clone(),
                _ => unreachable!("kind/data mismatch"),
            };
            for param in params {
                if !has_parameter_property_modifier(arena, param) {
                    continue;
                }
                let (pname, question_token) = match arena.data(param) {
                    NodeData::ParameterDeclaration(d) => (d.name, d.question_token),
                    _ => unreachable!("kind/data mismatch"),
                };
                // Only identifier-named parameter properties are emitted here;
                // binding-pattern parameter properties are deferred.
                if arena.kind(pname) == Kind::Identifier {
                    let pmods = self.ensure_modifiers(arena, param);
                    let ptype = self.ensure_type(arena, param, false);
                    let pinit = self.ensure_no_initializer(arena, param);
                    let prop =
                        arena.new_property_declaration(pmods, pname, question_token, ptype, pinit);
                    member_nodes.push(prop);
                }
            }
        }

        for member in members {
            if let Some(tm) = self.transform_class_member(arena, member) {
                member_nodes.push(tm);
            }
        }

        // Heritage clauses (`extends`/`implements`) are kept as-is in the
        // reachable subset (the base-expression hoisting is deferred).
        arena.new_class_like(
            Kind::ClassDeclaration,
            modifiers,
            name,
            type_parameters,
            heritage_clauses,
            NodeList::new(member_nodes),
        )
    }

    /// Transforms an interface declaration: `export` preserved (no `declare`
    /// synthesized — interfaces are always types), members and heritage kept.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformInterfaceDeclaration
    fn transform_interface_declaration(&self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let (name, type_parameters, heritage_clauses, members) = match arena.data(node) {
            NodeData::InterfaceDeclaration(d) => (
                d.name,
                d.type_parameters.clone(),
                d.heritage_clauses.clone(),
                d.members.clone(),
            ),
            _ => unreachable!("kind/data mismatch"),
        };
        let modifiers = self.ensure_modifiers(arena, node);
        arena.new_interface_declaration(modifiers, name, type_parameters, heritage_clauses, members)
    }

    /// Transforms a type-alias declaration: `export` preserved (no `declare`),
    /// name / type parameters / aliased type kept.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformTypeAliasDeclaration
    fn transform_type_alias_declaration(&mut self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let (name, type_parameters, type_node) = match arena.data(node) {
            NodeData::TypeAliasDeclaration(d) => (d.name, d.type_parameters.clone(), d.type_node),
            _ => unreachable!("kind/data mismatch"),
        };
        // Go: transformTypeAliasDeclaration sets needsDeclare = false (a type
        // alias never gets `declare`).
        self.needs_declare = false;
        let modifiers = self.ensure_modifiers(arena, node);
        arena.new_type_alias_declaration(modifiers, name, type_parameters, type_node)
    }

    /// Transforms an import declaration for `.d.ts` emit: each binding is kept
    /// iff it is *referenced* (via the resolver's `is_referenced`, the reachable
    /// stand-in for Go's on-demand `IsDeclarationVisible` link), and the whole
    /// import is elided when nothing visible remains. A side-effect import
    /// (`import "m";`) is kept as-is. Without a resolver every binding is kept
    /// (pass-through, the bare-context path).
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformImportDeclaration
    fn transform_import_declaration(&self, arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
        let (modifiers, import_clause, module_specifier, attributes) = match arena.data(node) {
            NodeData::ImportDeclaration(d) => (
                d.modifiers.clone(),
                d.import_clause,
                d.module_specifier,
                d.attributes,
            ),
            _ => unreachable!("kind/data mismatch"),
        };
        // `import "mod";` (no clause) — kept for side effects.
        let Some(clause) = import_clause else {
            return Some(arena.new_import_declaration(
                modifiers,
                None,
                module_specifier,
                attributes,
            ));
        };
        let (phase_modifier, name, named_bindings) = match arena.data(clause) {
            NodeData::ImportClause(d) => (d.phase_modifier, d.name, d.named_bindings),
            _ => unreachable!("kind/data mismatch"),
        };
        // Go: visitDeclaration sets the phase modifier `defer` back to none.
        let phase_modifier = if phase_modifier == Kind::DeferKeyword {
            Kind::Unknown
        } else {
            phase_modifier
        };
        let resolver = self.resolver.as_ref();
        // The default binding's visibility tracks the import clause itself.
        let visible_default = name.filter(|_| resolver.is_none_or(|r| r.is_referenced(clause)));

        match named_bindings {
            // No named bindings: default-only (or elided).
            None => {
                visible_default?;
                let new_clause = arena.new_import_clause(phase_modifier, visible_default, None);
                Some(arena.new_import_declaration(
                    modifiers,
                    Some(new_clause),
                    module_specifier,
                    attributes,
                ))
            }
            // `import * as ns from "m"` (optionally with a default).
            Some(nb) if arena.kind(nb) == Kind::NamespaceImport => {
                let ns_visible = resolver.is_none_or(|r| r.is_referenced(nb));
                let kept_ns = ns_visible.then_some(nb);
                if visible_default.is_none() && kept_ns.is_none() {
                    return None;
                }
                let new_clause = arena.new_import_clause(phase_modifier, visible_default, kept_ns);
                Some(arena.new_import_declaration(
                    modifiers,
                    Some(new_clause),
                    module_specifier,
                    attributes,
                ))
            }
            // `import { a, b } from "m"` (optionally with a default).
            Some(nb) => {
                let specifiers = match arena.data(nb) {
                    NodeData::NamedImports(d) => d.elements.nodes.clone(),
                    _ => unreachable!("kind/data mismatch"),
                };
                let kept: Vec<NodeId> = specifiers
                    .into_iter()
                    .filter(|&s| resolver.is_none_or(|r| r.is_referenced(s)))
                    .collect();
                if kept.is_empty() && visible_default.is_none() {
                    return None;
                }
                let named_imports =
                    (!kept.is_empty()).then(|| arena.new_named_imports(NodeList::new(kept)));
                let new_clause =
                    arena.new_import_clause(phase_modifier, visible_default, named_imports);
                Some(arena.new_import_declaration(
                    modifiers,
                    Some(new_clause),
                    module_specifier,
                    attributes,
                ))
            }
        }
    }

    /// Transforms an export declaration (`export { x };` / `export * from "m";`)
    /// for `.d.ts` emit: kept as-is in the reachable subset (Go rewrites the
    /// module specifier, a no-op here, and records the scope/module markers).
    ///
    /// Side effects: may push a rebuilt node onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.visitDeclarationStatements (ExportDeclaration arm)
    fn transform_export_declaration(&self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let (modifiers, is_type_only, export_clause, module_specifier, attributes) =
            match arena.data(node) {
                NodeData::ExportDeclaration(d) => (
                    d.modifiers.clone(),
                    d.is_type_only,
                    d.export_clause,
                    d.module_specifier,
                    d.attributes,
                ),
                _ => unreachable!("kind/data mismatch"),
            };
        arena.new_export_declaration(
            modifiers,
            is_type_only,
            export_clause,
            module_specifier,
            attributes,
        )
    }

    /// Transforms an export assignment (`export = e;` / `export default e;`) for
    /// `.d.ts` emit. The reachable subset keeps an identifier expression as-is
    /// (Go's `transformExportAssignment` identifier fast path); a non-identifier
    /// expression (which Go rewrites into a synthesized `_default` variable) is
    /// deferred.
    ///
    /// Side effects: may push a rebuilt node onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformExportAssignment
    fn transform_export_assignment(&self, arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
        let (modifiers, is_export_equals, expression) = match arena.data(node) {
            NodeData::ExportAssignment(d) => {
                (d.modifiers.clone(), d.is_export_equals, d.expression)
            }
            _ => unreachable!("kind/data mismatch"),
        };
        // DEFER(D-F3): a non-identifier export expression is rewritten by Go into
        // a synthesized `declare const _default: T; export default _default;`
        // (needs the `_default` unique-name + type synthesis dance). The
        // reachable subset keeps both the identifier fast path and (unchanged)
        // the non-identifier expression.
        Some(arena.new_export_assignment(modifiers, is_export_equals, None, expression))
    }

    // --- class members ---------------------------------------------------

    /// Transforms a class member into its `.d.ts` form, or `None` when it has no
    /// declaration form (semicolon element, static block, private-identifier
    /// member, deferred member kind).
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.visitDeclarationSubtree (member dispatch)
    fn transform_class_member(&self, arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
        match arena.kind(node) {
            Kind::PropertyDeclaration => self.transform_property_declaration(arena, node),
            Kind::MethodDeclaration => self.transform_method_declaration(arena, node),
            Kind::Constructor => Some(self.transform_constructor_declaration(arena, node)),
            Kind::GetAccessor | Kind::SetAccessor => {
                self.transform_accessor_declaration(arena, node)
            }
            // No declaration form: a lone `;`, a static block (no observable
            // type), and (deferred) index signatures / unhandled members.
            _ => None,
        }
    }

    /// Transforms a class property: type kept (omitted for `private`),
    /// initializer and `!` token removed, modifiers normalized. Returns `None`
    /// for a `#private` field member.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformPropertyDeclaration
    fn transform_property_declaration(
        &self,
        arena: &mut NodeArena,
        node: NodeId,
    ) -> Option<NodeId> {
        let (name, postfix_token) = match arena.data(node) {
            NodeData::PropertyDeclaration(d) => (d.name, d.postfix_token),
            _ => unreachable!("kind/data mismatch"),
        };
        if arena.kind(name) == Kind::PrivateIdentifier {
            return None;
        }
        // Drop the definite-assignment `!`; keep a `?`.
        let postfix_token = postfix_token.filter(|&t| arena.kind(t) != Kind::ExclamationToken);
        let modifiers = self.ensure_modifiers(arena, node);
        let type_node = self.ensure_type(arena, node, false);
        let initializer = self.ensure_no_initializer(arena, node);
        Some(arena.new_property_declaration(modifiers, name, postfix_token, type_node, initializer))
    }

    /// Transforms a class method into its signature (body removed), or a
    /// name-only field for a `private` method (Go's `omitPrivateMethodType`).
    /// Returns `None` for a `#private` method.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformMethodDeclaration
    fn transform_method_declaration(&self, arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
        let (name, postfix_token, type_parameters, parameters) = match arena.data(node) {
            NodeData::MethodDeclaration(d) => (
                d.name,
                d.postfix_token,
                d.type_parameters.clone(),
                d.parameters.clone(),
            ),
            _ => unreachable!("kind/data mismatch"),
        };
        if effective_declaration_flags(arena, node, ModifierFlags::PRIVATE)
            .contains(ModifierFlags::PRIVATE)
        {
            return Some(self.omit_private_method_type(arena, node, name));
        }
        if arena.kind(name) == Kind::PrivateIdentifier {
            return None;
        }
        // Go: a non-private method's return type is inferred via ensureType, so
        // it reports the --isolatedDeclarations explicit-annotation diagnostic.
        self.report_isolated_declarations_return_type(arena, node);
        let modifiers = self.ensure_modifiers(arena, node);
        let type_parameters = self.ensure_type_params(arena, node, type_parameters);
        let parameters = self.update_param_list(arena, node, &parameters);
        let type_node = self.ensure_type(arena, node, false);
        Some(arena.new_method_declaration(
            modifiers,
            None,
            name,
            postfix_token,
            type_parameters,
            parameters,
            type_node,
            None,
            None,
        ))
    }

    /// Emits a `private` method as a name-only field (a `private` method has no
    /// observable signature in `.d.ts`).
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.omitPrivateMethodType
    fn omit_private_method_type(
        &self,
        arena: &mut NodeArena,
        node: NodeId,
        name: NodeId,
    ) -> NodeId {
        // DEFER(D-F3): Go also returns `nil` for a non-first overloaded private
        // method declaration (`symbol.Declarations[0] != input`); that
        // de-duplication needs the symbol's declaration list.
        let modifiers = self.ensure_modifiers(arena, node);
        arena.new_property_declaration(modifiers, name, None, None, None)
    }

    /// Transforms a constructor into its signature: parameter property modifiers
    /// stripped, body and return type removed.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformConstructorDeclaration
    fn transform_constructor_declaration(&self, arena: &mut NodeArena, node: NodeId) -> NodeId {
        let parameters = match arena.data(node) {
            NodeData::ConstructorDeclaration(d) => d.parameters.clone(),
            _ => unreachable!("kind/data mismatch"),
        };
        let modifiers = self.ensure_modifiers(arena, node);
        let parameters = self.update_param_list(arena, node, &parameters);
        arena.new_constructor_declaration(modifiers, None, parameters, None, None, None)
    }

    /// Transforms a get/set accessor into its signature (body removed), or
    /// `None` for a `#private` accessor.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.transformGetAccesorDeclaration / transformSetAccessorDeclaration
    fn transform_accessor_declaration(
        &self,
        arena: &mut NodeArena,
        node: NodeId,
    ) -> Option<NodeId> {
        let kind = arena.kind(node);
        let name = match arena.data(node) {
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.name,
            _ => unreachable!("kind/data mismatch"),
        };
        if arena.kind(name) == Kind::PrivateIdentifier {
            return None;
        }
        let is_private = effective_declaration_flags(arena, node, ModifierFlags::PRIVATE)
            .contains(ModifierFlags::PRIVATE);
        let modifiers = self.ensure_modifiers(arena, node);
        let parameters = self.update_accessor_param_list(arena, node, is_private);
        // A getter keeps its (annotated) return type; a setter never has one.
        let type_node = if kind == Kind::GetAccessor {
            self.ensure_type(arena, node, false)
        } else {
            None
        };
        Some(arena.new_accessor_declaration(
            kind, modifiers, name, None, parameters, type_node, None, None,
        ))
    }

    /// Builds the parameter list of a get/set accessor (Go's
    /// `updateAccessorParamList`): a getter has at most a `this` parameter; a
    /// setter keeps its value parameter (synthesizing `value` when missing).
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.updateAccessorParamList
    fn update_accessor_param_list(
        &self,
        arena: &mut NodeArena,
        node: NodeId,
        is_private: bool,
    ) -> NodeList {
        let (is_set, params) = match arena.data(node) {
            NodeData::GetAccessorDeclaration(d) => (false, d.parameters.nodes.clone()),
            NodeData::SetAccessorDeclaration(d) => (true, d.parameters.nodes.clone()),
            _ => unreachable!("kind/data mismatch"),
        };
        let mut new_params: Vec<NodeId> = Vec::new();
        if !is_private {
            if let Some(this_param) = get_this_parameter(arena, &params) {
                new_params.push(self.ensure_parameter(arena, this_param));
            }
        }
        if is_set {
            let mut value_param: Option<NodeId> = None;
            if !is_private {
                if new_params.len() == 1 && params.len() >= 2 {
                    value_param = Some(self.ensure_parameter(arena, params[1]));
                } else if new_params.is_empty() && !params.is_empty() {
                    value_param = Some(self.ensure_parameter(arena, params[0]));
                }
            }
            let value_param = value_param.unwrap_or_else(|| {
                // Synthesize a `value` parameter (typed `any` only when public).
                let t = if !is_private {
                    Some(arena.new_keyword_expression(Kind::AnyKeyword))
                } else {
                    None
                };
                let name = arena.new_identifier("value");
                arena.new_parameter_declaration(None, None, name, None, t, None)
            });
            new_params.push(value_param);
        }
        NodeList::new(new_params)
    }

    // --- shared helpers --------------------------------------------------

    /// Returns the type node to emit for declaration `node`: the existing
    /// annotation (annotated path), a synthesized type for an inferred
    /// declaration (D-F2), or `None` when the node is a literal `const` (whose
    /// initializer is kept instead) or `private` and not exempt.
    ///
    /// The inferred path (D-F2) asks the resolver to synthesize a type: a
    /// variable / property / parameter takes its widened declared type
    /// (`create_type_of_declaration`), a function-like takes its inferred return
    /// type (`create_return_type_of_signature_declaration`); the returned
    /// [`SynthesizedTypeNode`] descriptor is reconstructed into `arena`. When no
    /// resolver is wired (the bare-context path), an un-annotated declaration
    /// emits no type, preserving the pre-D-F2 behavior.
    ///
    /// Side effects: may push synthesized type nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureType
    fn ensure_type(
        &self,
        arena: &mut NodeArena,
        node: NodeId,
        ignore_private: bool,
    ) -> Option<NodeId> {
        if !ignore_private
            && effective_declaration_flags(arena, node, ModifierFlags::PRIVATE)
                .contains(ModifierFlags::PRIVATE)
        {
            // Private nodes emit no types (except private parameter properties,
            // whose parameter types are visible — handled via `ignore_private`).
            return None;
        }
        // A literal `const` keeps its initializer (`ensure_no_initializer`)
        // rather than a type (Go's `shouldPrintWithInitializer`).
        if self.should_print_with_initializer(arena, node) {
            return None;
        }
        // Annotated path: copy the existing annotation as-is (DEFER(D-F3): the
        // entity-name visibility checks and import-type module-specifier
        // rewriting Go's `Visitor().Visit(type)` performs).
        if let Some(annotation) = node_type_annotation(arena, node) {
            return Some(annotation);
        }
        // Inferred path (D-F2): synthesize a type via the resolver. Without a
        // resolver the type cannot be inferred, so no type is emitted (the
        // pre-D-F2 behavior).
        let resolver = self.resolver.as_ref()?;
        let synthesized = if has_inferred_type(arena, node) {
            // Go: ast.HasInferredType(node) -> CreateTypeOfDeclaration.
            resolver.create_type_of_declaration(node)
        } else if is_function_like(arena, node) {
            // Go: ast.IsFunctionLike(node) -> CreateReturnTypeOfSignatureDeclaration.
            resolver.create_return_type_of_signature_declaration(node)
        } else {
            None
        };
        // Go: `if typeNode == nil { return NewKeywordTypeNode(KindAnyKeyword) }`.
        Some(match synthesized {
            Some(t) => synthesized_type_node_to_ast(arena, &t),
            None => arena.new_keyword_expression(Kind::AnyKeyword),
        })
    }

    /// Reports whether declaration `node`'s initializer is kept verbatim in the
    /// `.d.ts` (a literal `const`, e.g. `const x = 1` -> `declare const x = 1;`),
    /// Go's `shouldPrintWithInitializer`.
    ///
    /// Side effects: may build and cache the declaration's type (via the
    /// resolver).
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.shouldPrintWithInitializer
    fn should_print_with_initializer(&self, arena: &NodeArena, node: NodeId) -> bool {
        let Some(resolver) = self.resolver.as_ref() else {
            // No resolver: cannot determine literal-const-ness, so the
            // initializer is always stripped (the pre-D-F2 behavior).
            return false;
        };
        can_have_literal_initializer(arena, node)
            && node_initializer(arena, node).is_some()
            && resolver.is_literal_const_declaration(node)
    }

    /// Returns the initializer to keep for a declaration: the literal value for a
    /// literal `const` (`= 1`), else `None` (initializers are stripped).
    ///
    /// Side effects: may push the rebuilt literal value onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureNoInitializer
    fn ensure_no_initializer(&self, arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
        if self.should_print_with_initializer(arena, node) {
            // DEFER(D-F3): Go's `ReportInferenceFallback` when the unwrapped
            // initializer is not a primitive literal value (an isolatedModules
            // diagnostic); not modelled here.
            if let Some(resolver) = self.resolver.as_ref() {
                if let Some(value) = resolver.create_literal_const_value(node) {
                    return Some(literal_const_value_to_ast(arena, &value));
                }
            }
        }
        None
    }

    /// Returns the type parameters to emit for `node`: kept as-is, or `None`
    /// when the node is `private`.
    ///
    /// Side effects: none (reads `arena`).
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureTypeParams
    fn ensure_type_params(
        &self,
        arena: &NodeArena,
        node: NodeId,
        params: Option<NodeList>,
    ) -> Option<NodeList> {
        if effective_declaration_flags(arena, node, ModifierFlags::PRIVATE)
            .contains(ModifierFlags::PRIVATE)
        {
            return None;
        }
        // DEFER(D-F3): Go visits each type parameter (entity-name visibility);
        // the reachable subset keeps them unchanged.
        params
    }

    /// Rebuilds a parameter list for declaration emit: empty for a `private`
    /// owner, else each parameter normalized via [`ensure_parameter`].
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.updateParamList
    fn update_param_list(
        &self,
        arena: &mut NodeArena,
        node: NodeId,
        params: &NodeList,
    ) -> NodeList {
        if effective_declaration_flags(arena, node, ModifierFlags::PRIVATE)
            .contains(ModifierFlags::PRIVATE)
            || params.nodes.is_empty()
        {
            return NodeList::new(vec![]);
        }
        let new_params: Vec<NodeId> = params
            .nodes
            .clone()
            .iter()
            .map(|&p| self.ensure_parameter(arena, p))
            .collect();
        NodeList::new(new_params)
    }

    /// Normalizes a parameter for declaration emit: modifiers and initializer
    /// removed, type kept (even for private parameter properties), optional `?`
    /// added when the parameter is optional.
    ///
    /// Side effects: may push rebuilt nodes onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureParameter
    fn ensure_parameter(&self, arena: &mut NodeArena, param: NodeId) -> NodeId {
        let (dot_dot_dot_token, name, question_token) = match arena.data(param) {
            NodeData::ParameterDeclaration(d) => (d.dot_dot_dot_token, d.name, d.question_token),
            _ => unreachable!("kind/data mismatch"),
        };
        let question_token = if is_optional_parameter(arena, param) {
            Some(question_token.unwrap_or_else(|| arena.new_token(Kind::QuestionToken)))
        } else {
            None
        };
        let type_node = self.ensure_type(arena, param, true);
        let initializer = self.ensure_no_initializer(arena, param);
        arena.new_parameter_declaration(
            None,
            dot_dot_dot_token,
            name,
            question_token,
            type_node,
            initializer,
        )
    }

    /// Computes the modifier list for declaration emit (Go's `ensureModifiers`):
    /// when the masked flags match the original, the source modifiers are kept
    /// (decorators filtered out); otherwise a fresh modifier list is built from
    /// the computed flags.
    ///
    /// Side effects: may push modifier tokens onto `arena`.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureModifiers
    fn ensure_modifiers(&self, arena: &mut NodeArena, node: NodeId) -> Option<ModifierList> {
        let current_flags = effective_declaration_flags(arena, node, ModifierFlags::ALL);
        let new_flags = self.ensure_modifier_flags(arena, node);
        if current_flags == new_flags {
            // Keep the original modifier nodes, eliding decorators.
            let mods = node_modifiers(arena, node)?;
            let kept: Vec<NodeId> = mods
                .list
                .nodes
                .iter()
                .copied()
                .filter(|&n| is_modifier(arena.kind(n)))
                .collect();
            let modifier_flags = modifiers_to_flags(arena, &kept);
            return Some(ModifierList {
                list: NodeList::new(kept),
                modifier_flags,
            });
        }
        let kinds = modifier_kinds_from_flags(new_flags);
        if kinds.is_empty() {
            return None;
        }
        let nodes: Vec<NodeId> = kinds.iter().map(|&k| arena.new_token(k)).collect();
        Some(ModifierList {
            list: NodeList::new(nodes),
            modifier_flags: new_flags,
        })
    }

    /// Computes the modifier flags for declaration emit (Go's
    /// `ensureModifierFlags`): drops `public`/`async`/`override`, adds `declare`
    /// at the top level of a non-ambient file (never on a type-only declaration
    /// or a non-top-level member), and applies the `default`-export fixups.
    ///
    /// Side effects: none (reads `arena`).
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureModifierFlags
    fn ensure_modifier_flags(&self, arena: &NodeArena, node: NodeId) -> ModifierFlags {
        // No `async`/`override`/`public` modifiers in declaration files.
        let mut mask = ModifierFlags::ALL
            & !(ModifierFlags::PUBLIC | ModifierFlags::ASYNC | ModifierFlags::OVERRIDE);
        let mut additions = if self.needs_declare && !is_always_type(arena, node) {
            ModifierFlags::AMBIENT
        } else {
            ModifierFlags::empty()
        };
        let parent_is_file = arena
            .parent(node)
            .is_some_and(|p| arena.kind(p) == Kind::SourceFile);
        if !parent_is_file {
            // A non-top-level declaration (e.g. a class member) is never
            // `declare`'d, and any existing `declare` is dropped.
            mask &= !ModifierFlags::AMBIENT;
            additions = ModifierFlags::empty();
        }
        // DEFER: `IsImplicitlyExportedJSTypeAlias` (JS files) adds `export`.
        mask_modifier_flags(arena, node, mask, additions)
    }
}

/// Builds the empty re-export marker `export {};` declaration emit appends to a
/// module whose output would otherwise look like a script (Go's
/// `createEmptyExports`).
///
/// Side effects: pushes the rebuilt nodes onto `arena`.
// Go: internal/transformers/declarations/transform.go:createEmptyExports
fn create_empty_exports(arena: &mut NodeArena) -> NodeId {
    let named_exports = arena.new_named_exports(NodeList::new(vec![]));
    arena.new_export_declaration(None, false, Some(named_exports), None, None)
}

/// Returns the body block of a function-like declaration, for the kinds the
/// `--isolatedDeclarations` return-type check inspects (Go's `node.Body()`).
///
/// Side effects: none (pure).
fn function_body(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.body,
        NodeData::MethodDeclaration(d) => d.body,
        _ => None,
    }
}

/// Reports whether `node`'s subtree contains a `return <expr>;` (a value
/// return), not descending into nested function scopes (whose returns belong to
/// them). Used as the reachable stand-in for "the body yields a syntactically
/// derivable return type" in the `--isolatedDeclarations` check.
///
/// Side effects: none (reads `arena`).
fn body_has_value_return(arena: &NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::ReturnStatement => {
            matches!(arena.data(node), NodeData::ReturnStatement(d) if d.expression.is_some())
        }
        // Nested function scopes have their own return-type inference.
        Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::ArrowFunction
        | Kind::MethodDeclaration
        | Kind::GetAccessor
        | Kind::SetAccessor
        | Kind::Constructor => false,
        _ => collect_children(arena, node)
            .iter()
            .any(|&child| body_has_value_return(arena, child)),
    }
}

/// Returns the leftmost identifier of an entity name (`a` of `a.b.c`), Go's
/// `ast.GetFirstIdentifier`. Reachable subset: identifiers and qualified names.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetFirstIdentifier
fn first_identifier(arena: &NodeArena, entity: NodeId) -> Option<NodeId> {
    match arena.data(entity) {
        NodeData::Identifier(_) => Some(entity),
        NodeData::QualifiedName(d) => first_identifier(arena, d.left),
        _ => None,
    }
}

/// Collects the direct child node ids of `node` (in source order) via the
/// arena's child walk, so callers can recurse without a borrow-conflicting
/// closure.
///
/// Side effects: none (reads `arena`).
fn collect_children(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    let mut children = Vec::new();
    arena.for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    children
}

/// Returns the printed text of declaration `node`'s name (Go's
/// `GetTextOfNode(GetNameOfDeclaration(node))`), for the diagnostic's exported-
/// name argument. Reachable subset: a variable declaration's identifier name.
///
/// Side effects: none (reads `arena`).
fn declaration_name_text(arena: &NodeArena, node: NodeId) -> String {
    let name = match arena.data(node) {
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) => Some(d.name),
        NodeData::FunctionDeclaration(d) => d.name,
        _ => None,
    };
    name.map(|n| arena.text(n).to_string()).unwrap_or_default()
}

/// Returns the type annotation node of declaration `node`, for the kinds the
/// transform reaches (Go's `node.Type()`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Type
fn node_type_annotation(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::VariableDeclaration(d) => d.type_node,
        NodeData::ParameterDeclaration(d) => d.type_node,
        NodeData::PropertyDeclaration(d) => d.type_node,
        NodeData::PropertySignature(d) => d.type_node,
        NodeData::FunctionDeclaration(d) => d.type_node,
        NodeData::MethodDeclaration(d) => d.type_node,
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.type_node,
        _ => None,
    }
}

/// Returns the initializer node of declaration `node`, for the kinds that can
/// carry a literal const initializer (Go's `node.Initializer()`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Initializer
fn node_initializer(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::VariableDeclaration(d) => d.initializer,
        NodeData::ParameterDeclaration(d) => d.initializer,
        NodeData::PropertyDeclaration(d) => d.initializer,
        NodeData::PropertySignature(d) => d.initializer,
        _ => None,
    }
}

/// Reports whether declaration `node`'s type is *inferred* when unannotated, so
/// declaration emit synthesizes it via `CreateTypeOfDeclaration` (Go's
/// `ast.HasInferredType`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:HasInferredType
fn has_inferred_type(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::Parameter
            | Kind::PropertySignature
            | Kind::PropertyDeclaration
            | Kind::BindingElement
            | Kind::PropertyAccessExpression
            | Kind::ElementAccessExpression
            | Kind::BinaryExpression
            | Kind::CallExpression
            | Kind::VariableDeclaration
            | Kind::ExportAssignment
            | Kind::PropertyAssignment
            | Kind::ShorthandPropertyAssignment
    )
}

/// Reports whether `node` is a function-like declaration (so declaration emit
/// synthesizes its return type via `CreateReturnTypeOfSignatureDeclaration`),
/// Go's `ast.IsFunctionLike`.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsFunctionLike
fn is_function_like(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::MethodSignature
            | Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::FunctionType
            | Kind::ConstructorType
            | Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::FunctionExpression
            | Kind::ArrowFunction
    )
}

/// Reports whether declaration `node` may keep a literal const initializer in
/// the `.d.ts` (Go's `canHaveLiteralInitializer`): a non-`private` property /
/// property signature, a parameter, or a variable declaration.
///
/// Side effects: none (reads `arena`).
// Go: internal/transformers/declarations/util.go:canHaveLiteralInitializer
fn can_have_literal_initializer(arena: &NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::PropertyDeclaration | Kind::PropertySignature => {
            !effective_declaration_flags(arena, node, ModifierFlags::PRIVATE)
                .contains(ModifierFlags::PRIVATE)
        }
        Kind::Parameter | Kind::VariableDeclaration => true,
        _ => false,
    }
}

/// Reconstructs a [`SynthesizedTypeNode`] descriptor (from the checker's node
/// builder) into a `.d.ts` AST type node in `arena`.
///
/// The checker hands back a closed descriptor (its arena and the transform's are
/// independent); this rebuilds the equivalent AST through the arena's type-node
/// constructors, mirroring the nodes Go's `typeToTypeNode` builds directly.
///
/// Side effects: pushes the rebuilt type node(s) onto `arena`.
// Go: internal/checker/nodebuilderimpl.go:NodeBuilderImpl.typeToTypeNode (the built node)
fn synthesized_type_node_to_ast(arena: &mut NodeArena, node: &SynthesizedTypeNode) -> NodeId {
    match node {
        // A keyword type node is represented (as elsewhere in this transform)
        // by a keyword expression node carrying the keyword kind, which the
        // printer emits in type position.
        SynthesizedTypeNode::Keyword(kind) => arena.new_keyword_expression(*kind),
        SynthesizedTypeNode::NumberLiteral { text, negative } => {
            let literal = arena.new_numeric_literal(text, TokenFlags::NONE);
            let expr = if *negative {
                arena.new_prefix_unary_expression(Kind::MinusToken, literal)
            } else {
                literal
            };
            arena.new_literal_type_node(expr)
        }
        SynthesizedTypeNode::StringLiteral(value) => {
            let literal = arena.new_string_literal(value, TokenFlags::NONE);
            arena.new_literal_type_node(literal)
        }
        SynthesizedTypeNode::BooleanLiteral(value) => {
            let keyword = arena.new_keyword_expression(if *value {
                Kind::TrueKeyword
            } else {
                Kind::FalseKeyword
            });
            arena.new_literal_type_node(keyword)
        }
        SynthesizedTypeNode::Null => {
            let keyword = arena.new_keyword_expression(Kind::NullKeyword);
            arena.new_literal_type_node(keyword)
        }
        SynthesizedTypeNode::Array(element) => {
            let element = synthesized_type_node_to_ast(arena, element);
            arena.new_array_type_node(element)
        }
        SynthesizedTypeNode::TypeReference { name, args } => {
            let name_node = arena.new_identifier(name);
            let type_arguments = if args.is_empty() {
                None
            } else {
                let nodes: Vec<NodeId> = args
                    .iter()
                    .map(|a| synthesized_type_node_to_ast(arena, a))
                    .collect();
                Some(NodeList::new(nodes))
            };
            arena.new_type_reference_node(name_node, type_arguments)
        }
        SynthesizedTypeNode::TypeQuery(name) => {
            let name_node = arena.new_identifier(name);
            arena.new_type_query_node(name_node, None)
        }
        SynthesizedTypeNode::Union(types) => {
            let nodes: Vec<NodeId> = types
                .iter()
                .map(|t| synthesized_type_node_to_ast(arena, t))
                .collect();
            arena.new_union_type_node(NodeList::new(nodes))
        }
        SynthesizedTypeNode::Intersection(types) => {
            let nodes: Vec<NodeId> = types
                .iter()
                .map(|t| synthesized_type_node_to_ast(arena, t))
                .collect();
            arena.new_intersection_type_node(NodeList::new(nodes))
        }
        SynthesizedTypeNode::Tuple { elements, readonly } => {
            let nodes: Vec<NodeId> = elements
                .iter()
                .map(|e| synthesized_type_node_to_ast(arena, e))
                .collect();
            let tuple = arena.new_tuple_type_node(NodeList::new(nodes));
            if *readonly {
                arena.new_type_operator_node(Kind::ReadonlyKeyword, tuple)
            } else {
                tuple
            }
        }
        SynthesizedTypeNode::TypeLiteral(properties) => {
            let members: Vec<NodeId> = properties
                .iter()
                .map(|p| synthesized_property_to_ast(arena, p))
                .collect();
            arena.new_type_literal_node(NodeList::new(members))
        }
    }
}

/// Reconstructs one [`SynthesizedProperty`] into a `PropertySignature` member of
/// a type-literal (`a: number` / `readonly a?: T`).
///
/// Side effects: pushes the rebuilt member node(s) onto `arena`.
// Go: internal/checker/nodebuilderimpl.go (createTypeNodesFromResolvedType property member)
fn synthesized_property_to_ast(arena: &mut NodeArena, property: &SynthesizedProperty) -> NodeId {
    let name = arena.new_identifier(&property.name);
    let type_node = synthesized_type_node_to_ast(arena, &property.type_node);
    let postfix_token = property
        .optional
        .then(|| arena.new_token(Kind::QuestionToken));
    let modifiers = property.readonly.then(|| {
        let token = arena.new_token(Kind::ReadonlyKeyword);
        ModifierList {
            list: NodeList::new(vec![token]),
            modifier_flags: ModifierFlags::READONLY,
        }
    });
    arena.new_property_signature(modifiers, name, postfix_token, Some(type_node), None)
}

/// Reconstructs a [`LiteralConstValue`] descriptor into the `.d.ts` initializer
/// expression a literal `const` keeps (`= 1` / `= "a"` / `= true`).
///
/// Side effects: pushes the rebuilt expression node(s) onto `arena`.
// Go: internal/checker/emitresolver.go:EmitResolver.CreateLiteralConstValue (the built expr)
fn literal_const_value_to_ast(arena: &mut NodeArena, value: &LiteralConstValue) -> NodeId {
    match value {
        LiteralConstValue::Number { text, negative } => {
            let literal = arena.new_numeric_literal(text, TokenFlags::NONE);
            if *negative {
                arena.new_prefix_unary_expression(Kind::MinusToken, literal)
            } else {
                literal
            }
        }
        LiteralConstValue::String(value) => arena.new_string_literal(value, TokenFlags::NONE),
        LiteralConstValue::Boolean(value) => arena.new_keyword_expression(if *value {
            Kind::TrueKeyword
        } else {
            Kind::FalseKeyword
        }),
    }
}

#[cfg(test)]
#[path = "transform_test.rs"]
mod tests;
