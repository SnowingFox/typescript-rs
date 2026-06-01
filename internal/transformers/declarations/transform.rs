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
//! # Deferred (with blocked-by)
//!
//! - **Inferred types (no annotation) → D-F2.** `ensure_type` returns the
//!   existing annotation; an un-annotated declaration yields `None` (no
//!   synthesized type). blocked-by: `EmitResolver::create_type_of_declaration`
//!   + the syntactic type-node builder + `SymbolTracker`.
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
use tsgo_ast::{Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeId, NodeList};
use tsgo_printer::EmitContext;

use super::util::{
    effective_declaration_flags, get_first_constructor_with_body, get_this_parameter,
    has_parameter_property_modifier, is_always_type, is_modifier, is_optional_parameter,
    mask_modifier_flags, modifier_kinds_from_flags, modifiers_to_flags, node_modifiers,
};

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
}

/// Builds a [`Transformer`] that lowers a source file to its `.d.ts` shape,
/// sharing the pipeline's emit context. `resolver`, when present, drives the
/// reachable emit-resolver queries (overload-implementation elision).
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
    let mut state = DeclarationsTransformer {
        needs_declare: false,
        resolver,
    };
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            state.visit_source_file(ec.arena_mut(), node)
        }),
        opt.context.clone(),
    )
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
        let mut out: Vec<NodeId> = Vec::with_capacity(statements.nodes.len());
        for &stmt in &statements.nodes {
            // Go: visitSourceFile sets needsDeclare = true; each top-level
            // declaration runs under that context.
            self.needs_declare = true;
            if let Some(result) = self.transform_top_level_statement(arena, stmt) {
                match arena.data(result) {
                    // A transform that produced multiple statements bundles them
                    // in a SyntaxList; flatten it into the output.
                    NodeData::SyntaxList(d) => out.extend(d.list.nodes.iter().copied()),
                    _ => out.push(result),
                }
            }
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
        arena.new_variable_declaration(name, None, type_node, self.ensure_no_initializer())
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
                    let prop = arena.new_property_declaration(
                        pmods,
                        pname,
                        question_token,
                        ptype,
                        self.ensure_no_initializer(),
                    );
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
        Some(arena.new_property_declaration(
            modifiers,
            name,
            postfix_token,
            type_node,
            self.ensure_no_initializer(),
        ))
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
    /// annotation (annotated path), or `None` when the type would be inferred
    /// (deferred) or the node is `private` and not exempt.
    ///
    /// Side effects: none (reads `arena`).
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureType
    fn ensure_type(&self, arena: &NodeArena, node: NodeId, ignore_private: bool) -> Option<NodeId> {
        if !ignore_private
            && effective_declaration_flags(arena, node, ModifierFlags::PRIVATE)
                .contains(ModifierFlags::PRIVATE)
        {
            // Private nodes emit no types (except private parameter properties,
            // whose parameter types are visible — handled via `ignore_private`).
            return None;
        }
        // DEFER(D-F1): `shouldPrintWithInitializer` (literal-const) is treated as
        // false, so a literal const keeps no initializer-in-place-of-type.
        //
        // Annotated path: copy the existing annotation as-is (DEFER(D-F3): the
        // entity-name visibility checks and import-type module-specifier
        // rewriting Go's `Visitor().Visit(type)` performs). A `None` result is
        // the inferred-type case — DEFER(D-F2): `createTypeOfDeclaration` + the
        // syntactic type-node builder.
        node_type_annotation(arena, node)
    }

    /// Returns the initializer to keep for a declaration, always `None` in the
    /// reachable subset (initializers are stripped).
    ///
    /// Side effects: none.
    // Go: internal/transformers/declarations/transform.go:DeclarationTransformer.ensureNoInitializer
    fn ensure_no_initializer(&self) -> Option<NodeId> {
        // DEFER(D-F1): the literal-const carve-out (`shouldPrintWithInitializer`)
        // keeps a `const x = 1` initializer; not modelled here.
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
        arena.new_parameter_declaration(
            None,
            dot_dot_dot_token,
            name,
            question_token,
            type_node,
            self.ensure_no_initializer(),
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

#[cfg(test)]
#[path = "transform_test.rs"]
mod tests;
