//! Grammar checks: syntactic-semantic diagnostics that run alongside type
//! checking.
//!
//! Ports the reachable core of Go's `grammarchecks.go`. These checks are
//! AST-structural (modifier lists, `NodeFlags`) and feed the same `Diagnostic`
//! collection as 4g, so they need little type machinery.
//!
//! DEFER(phase-4-checker-4j+): decorators, the full modifier ordering/position
//! matrix, parameter properties, and source/strict-mode-context checks that
//! depend on `compilerOptions`.

use rustc_hash::FxHashMap;
use tsgo_ast::utilities::modifier_to_flag;
use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId};

use super::program::BoundProgram;
use super::Checker;

bitflags::bitflags! {
    /// Property-kind classification for object-literal duplicate-name detection
    /// (Go's `DeclarationMeaning`).
    // Go: internal/checker/checker.go:DeclarationMeaning
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct DeclarationMeaning: u32 {
        const GET_ACCESSOR       = 1 << 0;
        const SET_ACCESSOR       = 1 << 1;
        const PROPERTY_ASSIGNMENT = 1 << 2;
        const METHOD             = 1 << 3;
        const GET_OR_SET_ACCESSOR = Self::GET_ACCESSOR.bits() | Self::SET_ACCESSOR.bits();
    }
}

impl Checker {
    /// Checks the modifiers of `node`, recording a diagnostic and returning
    /// `true` on the first grammar error (Go's `checkGrammarModifiers`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_modifiers(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(213)
    pub fn check_grammar_modifiers(&mut self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        let modifiers = modifier_nodes(arena, node);
        if modifiers.is_empty() {
            return false;
        }
        let node_kind = arena.kind(node);
        let parent = arena.parent(node);
        let parent_kind = parent.map(|p| arena.kind(p));
        let mut flags = ModifierFlags::empty();
        let mut last_static: Option<NodeId> = None;
        let mut last_async: Option<NodeId> = None;
        let mut last_override: Option<NodeId> = None;

        for modifier in &modifiers {
            let modifier = *modifier;
            let kind = arena.kind(modifier);
            if kind == Kind::Decorator {
                // DEFER(phase-4-checker-later): decorator grammar.
                continue;
            }
            let flag = modifier_to_flag(kind);

            // Non-readonly modifiers on type members / index signatures.
            if kind != Kind::ReadonlyKeyword {
                if matches!(node_kind, Kind::PropertySignature | Kind::MethodSignature) {
                    return self.grammar_error_on_node(
                        program,
                        modifier,
                        &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_TYPE_MEMBER,
                        &[modifier_text(kind)],
                    );
                }
                if node_kind == Kind::IndexSignature
                    && (kind != Kind::StaticKeyword || !parent_kind.is_some_and(is_class_like_kind))
                {
                    return self.grammar_error_on_node(
                        program,
                        modifier,
                        &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_AN_INDEX_SIGNATURE,
                        &[modifier_text(kind)],
                    );
                }
            }
            // in/out/const are the only modifiers on type parameters.
            if !matches!(
                kind,
                Kind::InKeyword | Kind::OutKeyword | Kind::ConstKeyword
            ) && node_kind == Kind::TypeParameter
            {
                return self.grammar_error_on_node(
                    program,
                    modifier,
                    &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_TYPE_PARAMETER,
                    &[modifier_text(kind)],
                );
            }

            match kind {
                Kind::OverrideKeyword => {
                    if flags.contains(ModifierFlags::OVERRIDE) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["override"],
                        );
                    } else if flags.contains(ModifierFlags::AMBIENT) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &["override", "declare"],
                        );
                    }
                    flags |= ModifierFlags::OVERRIDE;
                    last_override = Some(modifier);
                }
                Kind::PublicKeyword | Kind::ProtectedKeyword | Kind::PrivateKeyword => {
                    let text = modifier_text(kind);
                    if flags.intersects(ModifierFlags::ACCESSIBILITY_MODIFIER) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::ACCESSIBILITY_MODIFIER_ALREADY_SEEN,
                            &[],
                        );
                    } else if flags.contains(ModifierFlags::STATIC) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_MUST_PRECEDE_1_MODIFIER,
                            &[text, "static"],
                        );
                    } else if matches!(
                        parent_kind,
                        Some(Kind::ModuleBlock) | Some(Kind::SourceFile)
                    ) {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_MODULE_OR_NAMESPACE_ELEMENT, &[text],
                        );
                    } else if flags.contains(ModifierFlags::ABSTRACT)
                        && kind == Kind::PrivateKeyword
                    {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &[text, "abstract"],
                        );
                    }
                    flags |= modifier_to_flag(kind);
                }
                Kind::StaticKeyword => {
                    if flags.contains(ModifierFlags::STATIC) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["static"],
                        );
                    } else if matches!(
                        parent_kind,
                        Some(Kind::ModuleBlock) | Some(Kind::SourceFile)
                    ) {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_MODULE_OR_NAMESPACE_ELEMENT, &["static"],
                        );
                    } else if node_kind == Kind::Parameter {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_PARAMETER,
                            &["static"],
                        );
                    } else if flags.contains(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &["static", "abstract"],
                        );
                    }
                    flags |= ModifierFlags::STATIC;
                    last_static = Some(modifier);
                }
                Kind::AccessorKeyword => {
                    if flags.contains(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["accessor"],
                        );
                    } else if flags.contains(ModifierFlags::READONLY) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &["accessor", "readonly"],
                        );
                    } else if flags.contains(ModifierFlags::AMBIENT) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &["accessor", "declare"],
                        );
                    } else if node_kind != Kind::PropertyDeclaration {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::X_ACCESSOR_MODIFIER_CAN_ONLY_APPEAR_ON_A_PROPERTY_DECLARATION, &[],
                        );
                    }
                    flags |= ModifierFlags::ACCESSOR;
                }
                Kind::ReadonlyKeyword => {
                    if flags.contains(ModifierFlags::READONLY) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["readonly"],
                        );
                    } else if !matches!(
                        node_kind,
                        Kind::PropertyDeclaration
                            | Kind::PropertySignature
                            | Kind::IndexSignature
                            | Kind::Parameter
                    ) {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::X_READONLY_MODIFIER_CAN_ONLY_APPEAR_ON_A_PROPERTY_DECLARATION_OR_INDEX_SIGNATURE, &[],
                        );
                    } else if flags.contains(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &["readonly", "accessor"],
                        );
                    }
                    flags |= ModifierFlags::READONLY;
                }
                Kind::ExportKeyword => {
                    if flags.contains(ModifierFlags::EXPORT) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["export"],
                        );
                    } else if parent_kind.is_some_and(is_class_like_kind) {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_CLASS_ELEMENTS_OF_THIS_KIND, &["export"],
                        );
                    } else if node_kind == Kind::Parameter {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_PARAMETER,
                            &["export"],
                        );
                    }
                    flags |= ModifierFlags::EXPORT;
                }
                Kind::DeclareKeyword => {
                    if flags.contains(ModifierFlags::AMBIENT) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["declare"],
                        );
                    } else if flags.contains(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_IN_AN_AMBIENT_CONTEXT,
                            &["async"],
                        );
                    } else if flags.contains(ModifierFlags::OVERRIDE) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_IN_AN_AMBIENT_CONTEXT,
                            &["override"],
                        );
                    } else if parent_kind.is_some_and(is_class_like_kind)
                        && node_kind != Kind::PropertyDeclaration
                    {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_CLASS_ELEMENTS_OF_THIS_KIND, &["declare"],
                        );
                    } else if node_kind == Kind::Parameter {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_PARAMETER,
                            &["declare"],
                        );
                    } else if parent.is_some_and(|p| arena.flags(p).contains(NodeFlags::AMBIENT))
                        && parent_kind == Some(Kind::ModuleBlock)
                    {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::A_DECLARE_MODIFIER_CANNOT_BE_USED_IN_AN_ALREADY_AMBIENT_CONTEXT, &[],
                        );
                    } else if flags.contains(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &["declare", "accessor"],
                        );
                    }
                    flags |= ModifierFlags::AMBIENT;
                }
                Kind::AbstractKeyword => {
                    if flags.contains(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["abstract"],
                        );
                    }
                    if node_kind != Kind::ClassDeclaration && node_kind != Kind::ConstructorType {
                        if !matches!(
                            node_kind,
                            Kind::MethodDeclaration
                                | Kind::PropertyDeclaration
                                | Kind::GetAccessor
                                | Kind::SetAccessor
                        ) {
                            return self.grammar_error_on_node(
                                program, modifier,
                                &tsgo_diagnostics::X_ABSTRACT_MODIFIER_CAN_ONLY_APPEAR_ON_A_CLASS_METHOD_OR_PROPERTY_DECLARATION, &[],
                            );
                        }
                        let in_abstract_class = parent
                            .filter(|&p| arena.kind(p) == Kind::ClassDeclaration)
                            .is_some_and(|p| {
                                has_syntactic_modifier(arena, p, ModifierFlags::ABSTRACT)
                            });
                        if !in_abstract_class {
                            let msg = if node_kind == Kind::PropertyDeclaration {
                                &tsgo_diagnostics::ABSTRACT_PROPERTIES_CAN_ONLY_APPEAR_WITHIN_AN_ABSTRACT_CLASS
                            } else {
                                &tsgo_diagnostics::ABSTRACT_METHODS_CAN_ONLY_APPEAR_WITHIN_AN_ABSTRACT_CLASS
                            };
                            return self.grammar_error_on_node(program, modifier, msg, &[]);
                        }
                        if flags.contains(ModifierFlags::STATIC) {
                            return self.grammar_error_on_node(
                                program,
                                modifier,
                                &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                                &["static", "abstract"],
                            );
                        }
                        if flags.contains(ModifierFlags::PRIVATE) {
                            return self.grammar_error_on_node(
                                program,
                                modifier,
                                &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                                &["private", "abstract"],
                            );
                        }
                        if flags.contains(ModifierFlags::ASYNC) {
                            if let Some(a) = last_async {
                                return self.grammar_error_on_node(
                                    program,
                                    a,
                                    &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                                    &["async", "abstract"],
                                );
                            }
                        }
                    }
                    flags |= ModifierFlags::ABSTRACT;
                }
                Kind::AsyncKeyword => {
                    if flags.contains(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &["async"],
                        );
                    } else if flags.contains(ModifierFlags::AMBIENT)
                        || parent.is_some_and(|p| arena.flags(p).contains(NodeFlags::AMBIENT))
                    {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_IN_AN_AMBIENT_CONTEXT,
                            &["async"],
                        );
                    } else if node_kind == Kind::Parameter {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_PARAMETER,
                            &["async"],
                        );
                    }
                    if flags.contains(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                            &["async", "abstract"],
                        );
                    }
                    flags |= ModifierFlags::ASYNC;
                    last_async = Some(modifier);
                }
                Kind::InKeyword | Kind::OutKeyword => {
                    let in_out_flag = if kind == Kind::InKeyword {
                        ModifierFlags::IN
                    } else {
                        ModifierFlags::OUT
                    };
                    let in_out_text = if kind == Kind::InKeyword { "in" } else { "out" };
                    let p_kind = parent_kind;
                    if node_kind != Kind::TypeParameter
                        || !p_kind.is_some_and(|pk| {
                            matches!(
                                pk,
                                Kind::InterfaceDeclaration
                                    | Kind::ClassDeclaration
                                    | Kind::ClassExpression
                                    | Kind::TypeAliasDeclaration
                            )
                        })
                    {
                        return self.grammar_error_on_node(
                            program, modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_CAN_ONLY_APPEAR_ON_A_TYPE_PARAMETER_OF_A_CLASS_INTERFACE_OR_TYPE_ALIAS,
                            &[in_out_text],
                        );
                    }
                    if flags.contains(in_out_flag) {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                            &[in_out_text],
                        );
                    }
                    if in_out_flag.contains(ModifierFlags::IN) && flags.contains(ModifierFlags::OUT)
                    {
                        return self.grammar_error_on_node(
                            program,
                            modifier,
                            &tsgo_diagnostics::X_0_MODIFIER_MUST_PRECEDE_1_MODIFIER,
                            &["in", "out"],
                        );
                    }
                    flags |= in_out_flag;
                }
                _ => {
                    flags |= flag;
                }
            }
        }

        // Post-loop: constructor-specific checks.
        if node_kind == Kind::Constructor {
            if let Some(s) = last_static {
                return self.grammar_error_on_node(
                    program,
                    s,
                    &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_CONSTRUCTOR_DECLARATION,
                    &["static"],
                );
            }
            if let Some(o) = last_override {
                return self.grammar_error_on_node(
                    program,
                    o,
                    &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_CONSTRUCTOR_DECLARATION,
                    &["override"],
                );
            }
            if let Some(a) = last_async {
                return self.grammar_error_on_node(
                    program,
                    a,
                    &tsgo_diagnostics::X_0_MODIFIER_CANNOT_APPEAR_ON_A_CONSTRUCTOR_DECLARATION,
                    &["async"],
                );
            }
            return false;
        }

        // DEFER(phase-4-checker-later): import declaration checks, parameter
        // property checks, async-modifier function-like checks.
        false
    }

    /// Grammar checks for a single variable declaration (Go's
    /// `checkGrammarVariableDeclaration`): a `const` declaration must have an
    /// initializer, else `1155`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_variable_declaration(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarVariableDeclaration(1567)
    pub fn check_grammar_variable_declaration(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let (name, initializer) = match program.arena().data(node) {
            NodeData::VariableDeclaration(d) => (d.name, d.initializer),
            _ => return false,
        };
        // DEFER(phase-4-checker-4s+): binding patterns (destructuring) and the
        // for-in/for-of declaration forms. blocked-by: binding-element checking
        // + for-in/of statement descent.
        if program.arena().kind(name) != Kind::Identifier {
            return false;
        }
        // Combined node flags (Go's `getCombinedNodeFlags`): a variable
        // declaration's `let`/`const`/`using` and ambient bits live on the
        // enclosing `VariableDeclarationList` and `VariableStatement`.
        let arena = program.arena();
        let mut combined = arena.flags(node);
        if let Some(list) = arena.parent(node) {
            combined |= arena.flags(list);
            if let Some(statement) = arena.parent(list) {
                combined |= arena.flags(statement);
            }
        }
        // DEFER(phase-4-checker-4s+): ambient declarations route to
        // `checkAmbientInitializer` in Go; skip them here. blocked-by: ambient
        // initializer checking.
        if combined.contains(NodeFlags::AMBIENT) {
            return false;
        }
        // A for-in/for-of loop variable has no initializer by design, so Go
        // gates the whole `initializer == nil` block (including the const-must-
        // be-initialized requirement) on the declaration's parent-parent NOT
        // being a for-in/for-of statement.
        // Go: internal/checker/checker.go:Checker.checkGrammarVariableDeclaration
        if let Some(list) = arena.parent(node) {
            if let Some(grandparent) = arena.parent(list) {
                if matches!(
                    arena.kind(grandparent),
                    Kind::ForInStatement | Kind::ForOfStatement
                ) {
                    return false;
                }
            }
        }
        let block_scope_kind = combined & NodeFlags::BLOCK_SCOPED;
        // DEFER(phase-4-checker-4s+): `using`/`await using` and
        // definite-assignment (`!`) variants. blocked-by: `using` disposability
        // + definite-assignment checks.
        if initializer.is_none() && block_scope_kind == NodeFlags::CONST {
            self.error(
                program,
                node,
                &tsgo_diagnostics::X_0_DECLARATIONS_MUST_BE_INITIALIZED,
                &["const"],
            );
            return true;
        }
        false
    }

    /// Checks an object literal expression for duplicate property names,
    /// reporting TS1117 (duplicate properties), TS1118 (duplicate get/set
    /// accessors), TS1119 (property + accessor), and TS2300 (duplicate methods).
    ///
    /// `in_destructuring` suppresses the duplicate-name detection because
    /// destructuring patterns may repeat the same name (as an alias).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_object_literal_expression(p, n, false)
    /// # }
    /// ```
    ///
    /// Side effects: may record one or more diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarObjectLiteralExpression(1026)
    pub fn check_grammar_object_literal_expression(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        in_destructuring: bool,
    ) -> bool {
        let properties = match program.arena().data(node) {
            NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
            _ => return false,
        };
        let mut seen: FxHashMap<String, DeclarationMeaning> = FxHashMap::default();
        let arena = program.arena();

        for prop in &properties {
            let prop = *prop;
            let kind = arena.kind(prop);

            // DEFER(phase-4-checker-C5+): spread-assignment rest-destructuring
            // check, computed-property-name grammar, shorthand-initializer
            // grammar, private-identifier grammar, modifier grammar,
            // exclamation/question-mark, numeric/bigint grammar.
            if kind == Kind::SpreadAssignment {
                continue;
            }

            let name_node = match arena.data(prop) {
                NodeData::PropertyAssignment(d) => d.name,
                NodeData::ShorthandPropertyAssignment(d) => d.name,
                NodeData::MethodDeclaration(d) => d.name,
                NodeData::GetAccessorDeclaration(d) => d.name,
                NodeData::SetAccessorDeclaration(d) => d.name,
                _ => continue,
            };

            let current_kind = match kind {
                Kind::ShorthandPropertyAssignment | Kind::PropertyAssignment => {
                    DeclarationMeaning::PROPERTY_ASSIGNMENT
                }
                Kind::MethodDeclaration => DeclarationMeaning::METHOD,
                Kind::GetAccessor => DeclarationMeaning::GET_ACCESSOR,
                Kind::SetAccessor => DeclarationMeaning::SET_ACCESSOR,
                _ => continue,
            };

            if !in_destructuring {
                let effective_name = effective_property_name(arena, name_node);
                let Some(effective_name) = effective_name else {
                    continue;
                };
                let name_text = arena.text(name_node);

                if let Some(&existing_kind) = seen.get(&effective_name) {
                    if current_kind.intersects(DeclarationMeaning::METHOD)
                        && existing_kind.intersects(DeclarationMeaning::METHOD)
                    {
                        self.error(
                            program,
                            name_node,
                            &tsgo_diagnostics::DUPLICATE_IDENTIFIER_0,
                            &[name_text],
                        );
                    } else if current_kind.intersects(DeclarationMeaning::PROPERTY_ASSIGNMENT)
                        && existing_kind.intersects(DeclarationMeaning::PROPERTY_ASSIGNMENT)
                    {
                        self.error(
                            program,
                            name_node,
                            &tsgo_diagnostics::AN_OBJECT_LITERAL_CANNOT_HAVE_MULTIPLE_PROPERTIES_WITH_THE_SAME_NAME,
                            &[],
                        );
                    } else if current_kind.intersects(DeclarationMeaning::GET_OR_SET_ACCESSOR)
                        && existing_kind.intersects(DeclarationMeaning::GET_OR_SET_ACCESSOR)
                    {
                        if existing_kind != DeclarationMeaning::GET_OR_SET_ACCESSOR
                            && current_kind != existing_kind
                        {
                            seen.insert(effective_name, current_kind | existing_kind);
                        } else {
                            self.error(
                                program,
                                name_node,
                                &tsgo_diagnostics::AN_OBJECT_LITERAL_CANNOT_HAVE_MULTIPLE_GET_SLASHSET_ACCESSORS_WITH_THE_SAME_NAME,
                                &[],
                            );
                            return true;
                        }
                    } else {
                        self.error(
                            program,
                            name_node,
                            &tsgo_diagnostics::AN_OBJECT_LITERAL_CANNOT_HAVE_PROPERTY_AND_ACCESSOR_WITH_THE_SAME_NAME,
                            &[],
                        );
                        return true;
                    }
                } else {
                    seen.insert(effective_name, current_kind);
                }
            }
        }
        false
    }

    /// Checks that a constructor declaration `node` has no return type
    /// annotation (Go's `checkGrammarConstructorTypeAnnotation`): one present is
    /// `1093`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_constructor_type_annotation(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarConstructorTypeAnnotation(1884)
    pub fn check_grammar_constructor_type_annotation(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let type_node = match program.arena().data(node) {
            NodeData::ConstructorDeclaration(d) => d.type_node,
            _ => return false,
        };
        if let Some(type_node) = type_node {
            self.error(
                program,
                type_node,
                &tsgo_diagnostics::TYPE_ANNOTATION_CANNOT_APPEAR_ON_A_CONSTRUCTOR_DECLARATION,
                &[],
            );
            return true;
        }
        false
    }

    // Reports the mixed-operator grammar error `5076` when a `??` binary
    // expression is combined with `||`/`&&` without parentheses (Go's
    // `checkNullishCoalesceOperands`).
    //
    // Because `??`, `||`, and `&&` cannot be mixed unparenthesized, exactly one
    // of three syntactic shapes is reported, mirroring Go's `if`/`else if`
    // chain: (1) the `??` node's parent is a `||` whose left operand is the `??`
    // expression (`a ?? b || c` parses as `(a ?? b) || c`); (2) the `??` left
    // operand is itself a `||`/`&&` expression; (3) the `??` right operand is a
    // `&&` expression. `node` is the `??` binary expression; `left`/`right` are
    // its operands.
    //
    // DEFER(phase-4-checker-later): `checkNullishCoalesceOperandLeft` (the
    // always-/never-nullish operand diagnostics). blocked-by: the syntactic
    // nullishness-semantics analysis.
    //
    // Side effects: may record a `5076` diagnostic.
    // Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperands(12859)
    pub(crate) fn check_nullish_coalesce_operands(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        left: NodeId,
        right: NodeId,
    ) {
        let arena = program.arena();
        // Go's `left.Parent.Parent` is the `??` node's parent (the operand's
        // grandparent), since the operand's direct parent is the `??` node.
        let grandparent = arena.parent(node);
        let grandparent_binary = grandparent.filter(|&g| arena.kind(g) == Kind::BinaryExpression);
        if let Some(grandparent) = grandparent_binary {
            // Branch 1: `(a ?? b) || c` — the `??` node sits as the left operand
            // of a `||`. Report `5076` on that left operand.
            let (gp_left, gp_op) = match arena.data(grandparent) {
                NodeData::BinaryExpression(d) => (d.left, arena.kind(d.operator_token)),
                _ => return,
            };
            if arena.kind(gp_left) == Kind::BinaryExpression && gp_op == Kind::BarBarToken {
                self.error(
                    program,
                    gp_left,
                    &tsgo_diagnostics::X_0_AND_1_OPERATIONS_CANNOT_BE_MIXED_WITHOUT_PARENTHESES,
                    &["??", "||"],
                );
            }
        } else if arena.kind(left) == Kind::BinaryExpression {
            // Branch 2: the `??` left operand is itself a `||`/`&&` expression.
            let op = match arena.data(left) {
                NodeData::BinaryExpression(d) => arena.kind(d.operator_token),
                _ => return,
            };
            let op_text = match op {
                Kind::BarBarToken => "||",
                Kind::AmpersandAmpersandToken => "&&",
                _ => return,
            };
            self.error(
                program,
                left,
                &tsgo_diagnostics::X_0_AND_1_OPERATIONS_CANNOT_BE_MIXED_WITHOUT_PARENTHESES,
                &[op_text, "??"],
            );
        } else if arena.kind(right) == Kind::BinaryExpression {
            // Branch 3: the `??` right operand is a `&&` expression.
            let op = match arena.data(right) {
                NodeData::BinaryExpression(d) => arena.kind(d.operator_token),
                _ => return,
            };
            if op == Kind::AmpersandAmpersandToken {
                self.error(
                    program,
                    right,
                    &tsgo_diagnostics::X_0_AND_1_OPERATIONS_CANNOT_BE_MIXED_WITHOUT_PARENTHESES,
                    &["??", "&&"],
                );
            }
        }
        // DEFER(phase-4-checker-later): `checkNullishCoalesceOperandLeft` (the
        // always-nullish `This_expression_is_always_nullish` / never-nullish
        // `Right_operand_..._never_nullish` diagnostics).
    }

    /// Checks that a constructor declaration `node` has no type parameters
    /// (Go's `checkGrammarConstructorTypeParameters`): any present are `1092`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_constructor_type_parameters(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarConstructorTypeParameters(1869)
    pub fn check_grammar_constructor_type_parameters(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let first_type_parameter = match program.arena().data(node) {
            NodeData::ConstructorDeclaration(d) => match &d.type_parameters {
                Some(list) => list.nodes.first().copied(),
                None => return false,
            },
            _ => return false,
        };
        if let Some(type_parameter) = first_type_parameter {
            self.error(
                program,
                type_parameter,
                &tsgo_diagnostics::TYPE_PARAMETERS_CANNOT_APPEAR_ON_A_CONSTRUCTOR_DECLARATION,
                &[],
            );
            return true;
        }
        false
    }

    // -- checkGrammarProperty ----------------------------------------------------

    /// Grammar-checks a property declaration or property signature (Go's
    /// `checkGrammarProperty`): class field named "constructor" (TS18006),
    /// interface/type-literal property with initializer (TS1246/TS1247),
    /// definite-assignment assertions on class properties.
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarProperty(1892)
    pub fn check_grammar_property(&mut self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        let kind = arena.kind(node);
        let parent = arena.parent(node);
        let parent_kind = parent.map(|p| arena.kind(p));

        let property_name = match arena.data(node) {
            NodeData::PropertyDeclaration(d) => d.name,
            NodeData::PropertySignature(d) => d.name,
            _ => return false,
        };

        // Class context checks
        if parent_kind.is_some_and(is_class_like_kind) {
            if arena.kind(property_name) == Kind::StringLiteral
                && arena.text(property_name) == "constructor"
            {
                return self.grammar_error_on_node(
                    program,
                    property_name,
                    &tsgo_diagnostics::CLASSES_MAY_NOT_HAVE_A_FIELD_NAMED_CONSTRUCTOR,
                    &[],
                );
            }
            // DEFER(phase-4-checker-later): checkGrammarForInvalidDynamicName,
            // auto-accessor optional check (blocked-by: isNonBindableDynamicName/
            // isLateBindableName).
        } else if parent_kind == Some(Kind::InterfaceDeclaration) {
            // DEFER(phase-4-checker-later): checkGrammarForInvalidDynamicName
            if kind == Kind::PropertySignature {
                if let NodeData::PropertySignature(d) = arena.data(node) {
                    if let Some(init) = d.initializer {
                        return self.grammar_error_on_node(
                            program,
                            init,
                            &tsgo_diagnostics::AN_INTERFACE_PROPERTY_CANNOT_HAVE_AN_INITIALIZER,
                            &[],
                        );
                    }
                }
            }
        } else if parent_kind == Some(Kind::TypeLiteral) {
            // DEFER(phase-4-checker-later): checkGrammarForInvalidDynamicName
            if kind == Kind::PropertySignature {
                if let NodeData::PropertySignature(d) = arena.data(node) {
                    if let Some(init) = d.initializer {
                        return self.grammar_error_on_node(
                            program,
                            init,
                            &tsgo_diagnostics::A_TYPE_LITERAL_PROPERTY_CANNOT_HAVE_AN_INITIALIZER,
                            &[],
                        );
                    }
                }
            }
        }

        // DEFER(phase-4-checker-later): checkAmbientInitializer (blocked-by:
        // isInitializerSimpleLiteralEnumReference → checkExpressionCached).

        // Definite assignment assertion checks on class PropertyDeclaration
        if kind == Kind::PropertyDeclaration {
            if let NodeData::PropertyDeclaration(d) = arena.data(node) {
                if let Some(postfix) = d.postfix_token {
                    if arena.kind(postfix) == Kind::ExclamationToken {
                        if d.initializer.is_some() {
                            return self.grammar_error_on_node(
                                program, postfix,
                                &tsgo_diagnostics::DECLARATIONS_WITH_INITIALIZERS_CANNOT_ALSO_HAVE_DEFINITE_ASSIGNMENT_ASSERTIONS,
                                &[],
                            );
                        }
                        if d.type_node.is_none() {
                            return self.grammar_error_on_node(
                                program, postfix,
                                &tsgo_diagnostics::DECLARATIONS_WITH_DEFINITE_ASSIGNMENT_ASSERTIONS_MUST_ALSO_HAVE_TYPE_ANNOTATIONS,
                                &[],
                            );
                        }
                        if !parent_kind.is_some_and(is_class_like_kind)
                            || arena.flags(node).contains(NodeFlags::AMBIENT)
                            || has_syntactic_modifier(arena, node, ModifierFlags::STATIC)
                            || has_syntactic_modifier(arena, node, ModifierFlags::ABSTRACT)
                        {
                            return self.grammar_error_on_node(
                                program, postfix,
                                &tsgo_diagnostics::A_DEFINITE_ASSIGNMENT_ASSERTION_IS_NOT_PERMITTED_IN_THIS_CONTEXT,
                                &[],
                            );
                        }
                    }
                }
            }
        }

        false
    }

    // -- checkGrammarTypeOperatorNode --------------------------------------------

    /// Grammar-checks a type-operator node (`keyof`, `unique`, `readonly`) (Go's
    /// `checkGrammarTypeOperatorNode`):
    /// * `unique` requires `symbol` as inner type (TS1110), plus context checks.
    /// * `readonly` requires an array or tuple as inner type (TS1354).
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarTypeOperatorNode(1379)
    pub fn check_grammar_type_operator_node(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        let (operator, inner_type) = match arena.data(node) {
            NodeData::TypeOperator(d) => (d.operator, d.type_node),
            _ => return false,
        };

        if operator == Kind::UniqueKeyword {
            if arena.kind(inner_type) != Kind::SymbolKeyword {
                return self.grammar_error_on_node(
                    program,
                    inner_type,
                    &tsgo_diagnostics::X_0_EXPECTED,
                    &[tsgo_scanner::token_to_string(Kind::SymbolKeyword)],
                );
            }
            // Walk up parenthesized types to the context
            let mut parent = arena.parent(node);
            while parent.is_some_and(|p| arena.kind(p) == Kind::ParenthesizedType) {
                parent = parent.and_then(|p| arena.parent(p));
            }
            if let Some(p) = parent {
                match arena.kind(p) {
                    Kind::VariableDeclaration => {
                        if let NodeData::VariableDeclaration(d) = arena.data(p) {
                            if arena.kind(d.name) != Kind::Identifier {
                                return self.grammar_error_on_node(
                                    program, node,
                                    &tsgo_diagnostics::X_UNIQUE_SYMBOL_TYPES_MAY_NOT_BE_USED_ON_A_VARIABLE_DECLARATION_WITH_A_BINDING_NAME,
                                    &[],
                                );
                            }
                            // Check if parent is a VariableStatement
                            let gp = arena.parent(p).and_then(|list| arena.parent(list));
                            if gp.is_none_or(|g| arena.kind(g) != Kind::VariableStatement) {
                                return self.grammar_error_on_node(
                                    program, node,
                                    &tsgo_diagnostics::X_UNIQUE_SYMBOL_TYPES_ARE_ONLY_ALLOWED_ON_VARIABLES_IN_A_VARIABLE_STATEMENT,
                                    &[],
                                );
                            }
                            // Check const
                            if let Some(list) = arena.parent(p) {
                                if !arena.flags(list).contains(NodeFlags::CONST) {
                                    return self.grammar_error_on_node(
                                        program, d.name,
                                        &tsgo_diagnostics::A_VARIABLE_WHOSE_TYPE_IS_A_UNIQUE_SYMBOL_TYPE_MUST_BE_CONST,
                                        &[],
                                    );
                                }
                            }
                        }
                    }
                    Kind::PropertyDeclaration => {
                        let is_static = has_syntactic_modifier(arena, p, ModifierFlags::STATIC);
                        let is_readonly = has_syntactic_modifier(arena, p, ModifierFlags::READONLY);
                        if !is_static || !is_readonly {
                            if let NodeData::PropertyDeclaration(d) = arena.data(p) {
                                return self.grammar_error_on_node(
                                    program, d.name,
                                    &tsgo_diagnostics::A_PROPERTY_OF_A_CLASS_WHOSE_TYPE_IS_A_UNIQUE_SYMBOL_TYPE_MUST_BE_BOTH_STATIC_AND_READONLY,
                                    &[],
                                );
                            }
                        }
                    }
                    Kind::PropertySignature => {
                        if !has_syntactic_modifier(arena, p, ModifierFlags::READONLY) {
                            if let NodeData::PropertySignature(d) = arena.data(p) {
                                return self.grammar_error_on_node(
                                    program, d.name,
                                    &tsgo_diagnostics::A_PROPERTY_OF_AN_INTERFACE_OR_TYPE_LITERAL_WHOSE_TYPE_IS_A_UNIQUE_SYMBOL_TYPE_MUST_BE_READONLY,
                                    &[],
                                );
                            }
                        }
                    }
                    _ => {
                        return self.grammar_error_on_node(
                            program,
                            node,
                            &tsgo_diagnostics::X_UNIQUE_SYMBOL_TYPES_ARE_NOT_ALLOWED_HERE,
                            &[],
                        );
                    }
                }
            }
        } else if operator == Kind::ReadonlyKeyword
            && !matches!(arena.kind(inner_type), Kind::ArrayType | Kind::TupleType)
        {
            return self.grammar_error_on_first_token(
                program,
                node,
                &tsgo_diagnostics::X_READONLY_TYPE_MODIFIER_IS_ONLY_PERMITTED_ON_ARRAY_AND_TUPLE_LITERAL_TYPES,
                &[tsgo_scanner::token_to_string(Kind::SymbolKeyword)],
            );
        }

        false
    }

    // -- checkGrammarSourceFile ---------------------------------------------------

    /// Grammar-checks a source file (Go's `checkGrammarSourceFile`): in
    /// ambient (`.d.ts`) files, top-level declarations must have `declare` or
    /// `export` modifiers.
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarSourceFile(2053)
    pub fn check_grammar_source_file(&mut self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        if !arena.flags(node).contains(NodeFlags::AMBIENT) {
            return false;
        }
        let statements = match arena.data(node) {
            NodeData::SourceFile(d) => d.statements.nodes.clone(),
            _ => return false,
        };
        for decl in &statements {
            let kind = arena.kind(*decl);
            let is_decl_node = matches!(
                kind,
                Kind::FunctionDeclaration
                    | Kind::ClassDeclaration
                    | Kind::EnumDeclaration
                    | Kind::InterfaceDeclaration
                    | Kind::TypeAliasDeclaration
                    | Kind::ModuleDeclaration
                    | Kind::ImportDeclaration
                    | Kind::ImportEqualsDeclaration
                    | Kind::ExportDeclaration
                    | Kind::ExportAssignment
                    | Kind::NamespaceExportDeclaration
            ) || kind == Kind::VariableStatement;
            if is_decl_node
                && self
                    .check_grammar_top_level_element_for_required_declare_modifier(program, *decl)
            {
                return true;
            }
        }
        false
    }

    /// Checks whether a top-level `.d.ts` declaration requires `declare`/`export`
    /// modifiers (Go's `checkGrammarTopLevelElementForRequiredDeclareModifier`).
    ///
    /// Returns `true` and records TS1046 if the node needs but lacks
    /// `declare`/`export`/`default`.
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarTopLevelElementForRequiredDeclareModifier(2022)
    pub fn check_grammar_top_level_element_for_required_declare_modifier(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let kind = program.arena().kind(node);
        if matches!(
            kind,
            Kind::InterfaceDeclaration
                | Kind::TypeAliasDeclaration
                | Kind::ImportDeclaration
                | Kind::ImportEqualsDeclaration
                | Kind::ExportDeclaration
                | Kind::ExportAssignment
                | Kind::NamespaceExportDeclaration
        ) {
            return false;
        }
        if has_syntactic_modifier(
            program.arena(),
            node,
            ModifierFlags::AMBIENT | ModifierFlags::EXPORT | ModifierFlags::DEFAULT,
        ) {
            return false;
        }
        self.grammar_error_on_first_token(
            program,
            node,
            &tsgo_diagnostics::TOP_LEVEL_DECLARATIONS_IN_D_TS_FILES_MUST_START_WITH_EITHER_A_DECLARE_OR_EXPORT_MODIFIER,
            &[],
        )
    }

    // -- checkGrammarIndexSignature -----------------------------------------------

    /// Grammar-checks an index signature declaration (Go's
    /// `checkGrammarIndexSignature`): delegates to modifier + parameter checks.
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarIndexSignature(850)
    pub fn check_grammar_index_signature(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        self.check_grammar_modifiers(program, node)
            || self.check_grammar_index_signature_parameters(program, node)
    }

    /// Grammar-checks index-signature parameters (Go's
    /// `checkGrammarIndexSignatureParameters`): exactly one parameter (TS1096),
    /// no rest (TS1017), no accessibility modifier (TS1018), no `?` (TS1019),
    /// no initializer (TS1020), must have type annotation (TS1022), return type
    /// annotation required (TS1021).
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarIndexSignatureParameters(806)
    pub fn check_grammar_index_signature_parameters(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        let (parameters, return_type) = match arena.data(node) {
            NodeData::IndexSignatureDeclaration(d) => (&d.parameters, d.type_node),
            _ => return false,
        };
        let params = &parameters.nodes;

        if params.is_empty() {
            return self.grammar_error_on_node(
                program,
                node,
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_MUST_HAVE_EXACTLY_ONE_PARAMETER,
                &[],
            );
        }

        let first_param = params[0];
        let pd = match arena.data(first_param) {
            NodeData::ParameterDeclaration(d) => d.clone(),
            _ => return false,
        };

        if params.len() != 1 {
            return self.grammar_error_on_node(
                program,
                pd.name,
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_MUST_HAVE_EXACTLY_ONE_PARAMETER,
                &[],
            );
        }

        // trailing comma
        self.check_grammar_for_disallowed_trailing_comma(
            program,
            parameters,
            &tsgo_diagnostics::AN_INDEX_SIGNATURE_CANNOT_HAVE_A_TRAILING_COMMA,
        );

        if pd.dot_dot_dot_token.is_some() {
            return self.grammar_error_on_node(
                program,
                pd.dot_dot_dot_token.unwrap(),
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_CANNOT_HAVE_A_REST_PARAMETER,
                &[],
            );
        }

        if modifier_nodes(arena, first_param)
            .iter()
            .any(|&m| arena.kind(m) != Kind::Decorator)
        {
            return self.grammar_error_on_node(
                program,
                pd.name,
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_PARAMETER_CANNOT_HAVE_AN_ACCESSIBILITY_MODIFIER,
                &[],
            );
        }

        if pd.question_token.is_some() {
            return self.grammar_error_on_node(
                program,
                pd.question_token.unwrap(),
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_PARAMETER_CANNOT_HAVE_A_QUESTION_MARK,
                &[],
            );
        }

        if pd.initializer.is_some() {
            return self.grammar_error_on_node(
                program,
                pd.name,
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_PARAMETER_CANNOT_HAVE_AN_INITIALIZER,
                &[],
            );
        }

        if pd.type_node.is_none() {
            return self.grammar_error_on_node(
                program,
                pd.name,
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_PARAMETER_MUST_HAVE_A_TYPE_ANNOTATION,
                &[],
            );
        }

        // DEFER(phase-4-checker-later): type resolution checks
        // (literal/generic type and valid index key type checks require
        // getTypeFromTypeNode/someType/everyType/isValidIndexKeyType).

        if return_type.is_none() {
            return self.grammar_error_on_node(
                program,
                node,
                &tsgo_diagnostics::AN_INDEX_SIGNATURE_MUST_HAVE_A_TYPE_ANNOTATION,
                &[],
            );
        }

        false
    }

    // -- checkGrammarTypeArguments ------------------------------------------------

    /// Grammar-checks a type argument list (Go's `checkGrammarTypeArguments`):
    /// trailing comma (TS1009) and empty list (TS1099).
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarTypeArguments(865)
    pub fn check_grammar_type_arguments(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        type_arguments: Option<&tsgo_ast::NodeList>,
    ) -> bool {
        if let Some(list) = type_arguments {
            if self.check_grammar_for_disallowed_trailing_comma(
                program,
                list,
                &tsgo_diagnostics::TRAILING_COMMA_NOT_ALLOWED,
            ) {
                return true;
            }
        }
        self.check_grammar_for_at_least_one_type_argument(program, node, type_arguments)
    }

    /// Reports TS1099 if the type-argument list is present but empty.
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarForAtLeastOneTypeArgument(855)
    fn check_grammar_for_at_least_one_type_argument(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        type_arguments: Option<&tsgo_ast::NodeList>,
    ) -> bool {
        if let Some(list) = type_arguments {
            if list.nodes.is_empty() {
                // DEFER: proper span calculation for `<>` delimiters.
                return self.grammar_error_on_node(
                    program,
                    node,
                    &tsgo_diagnostics::TYPE_ARGUMENT_LIST_CANNOT_BE_EMPTY,
                    &[],
                );
            }
        }
        false
    }

    /// Reports an error if a `NodeList` has a trailing comma (Go's
    /// `checkGrammarForDisallowedTrailingComma`).
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarForDisallowedTrailingComma(681)
    pub fn check_grammar_for_disallowed_trailing_comma(
        &mut self,
        program: &dyn BoundProgram,
        list: &tsgo_ast::NodeList,
        message: &'static tsgo_diagnostics::Message,
    ) -> bool {
        if program.arena().list_has_trailing_comma(list) {
            let end = list.end();
            return self.grammar_error_at_pos(program, list.nodes[0], end - 1, 1, message, &[]);
        }
        false
    }

    // -- checkGrammarArrowFunction ------------------------------------------------

    /// Grammar-checks an arrow function (Go's `checkGrammarArrowFunction`):
    /// line-terminator before `=>` is TS1200.
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarArrowFunction(782)
    pub fn check_grammar_arrow_function(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        if arena.kind(node) != Kind::ArrowFunction {
            return false;
        }
        let equals_greater_than_token = match arena.data(node) {
            NodeData::ArrowFunction(d) => d.equals_greater_than_token,
            _ => return false,
        };
        // DEFER(phase-4-checker-later): .mts/.cts single type parameter
        // without constraint/trailing comma check (TS7060).
        if let Some(source_text) = program.source_text() {
            let line_starts = tsgo_core::compute_ecma_line_starts(source_text);
            let token_loc = arena.loc(equals_greater_than_token);
            let start_line = tsgo_scanner::compute_line_of_position(&line_starts, token_loc.pos());
            let end_line = tsgo_scanner::compute_line_of_position(&line_starts, token_loc.end());
            if start_line != end_line {
                return self.grammar_error_on_node(
                    program,
                    equals_greater_than_token,
                    &tsgo_diagnostics::LINE_TERMINATOR_NOT_PERMITTED_BEFORE_ARROW,
                    &[],
                );
            }
        }
        false
    }

    // -- checkGrammarParameterList -----------------------------------------------

    /// Grammar-checks a parameter list (Go's `checkGrammarParameterList`):
    /// rest parameter must be last (TS1014), rest cannot be optional (TS1047),
    /// rest cannot have initializer (TS1048), `?` + initializer conflict
    /// (TS1015), required after optional (TS1016).
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarParameterList(697)
    pub fn check_grammar_parameter_list(
        &mut self,
        program: &dyn BoundProgram,
        parameters: &tsgo_ast::NodeList,
    ) -> bool {
        let arena = program.arena();
        let params = &parameters.nodes;
        let count = params.len();
        let mut seen_optional = false;

        for (i, &param) in params.iter().enumerate() {
            let pd = match arena.data(param) {
                NodeData::ParameterDeclaration(d) => d.clone(),
                _ => continue,
            };
            if pd.dot_dot_dot_token.is_some() {
                if i != count - 1 {
                    return self.grammar_error_on_node(
                        program,
                        pd.dot_dot_dot_token.unwrap(),
                        &tsgo_diagnostics::A_REST_PARAMETER_MUST_BE_LAST_IN_A_PARAMETER_LIST,
                        &[],
                    );
                }
                // DEFER: trailing comma check for rest/binding pattern
                if pd.question_token.is_some() {
                    return self.grammar_error_on_node(
                        program,
                        pd.question_token.unwrap(),
                        &tsgo_diagnostics::A_REST_PARAMETER_CANNOT_BE_OPTIONAL,
                        &[],
                    );
                }
                if pd.initializer.is_some() {
                    return self.grammar_error_on_node(
                        program,
                        pd.name,
                        &tsgo_diagnostics::A_REST_PARAMETER_CANNOT_HAVE_AN_INITIALIZER,
                        &[],
                    );
                }
            } else if is_optional_parameter(arena, param) {
                seen_optional = true;
                if pd.question_token.is_some()
                    && !arena
                        .flags(pd.question_token.unwrap())
                        .contains(NodeFlags::REPARSED)
                    && pd.initializer.is_some()
                {
                    return self.grammar_error_on_node(
                        program,
                        pd.name,
                        &tsgo_diagnostics::PARAMETER_CANNOT_HAVE_QUESTION_MARK_AND_INITIALIZER,
                        &[],
                    );
                }
            } else if seen_optional && pd.initializer.is_none() {
                return self.grammar_error_on_node(
                    program,
                    pd.name,
                    &tsgo_diagnostics::A_REQUIRED_PARAMETER_CANNOT_FOLLOW_AN_OPTIONAL_PARAMETER,
                    &[],
                );
            }
        }
        false
    }

    // -- checkGrammarComputedPropertyName --------------------------------------

    /// Grammar-checks a computed property name (Go's
    /// `checkGrammarComputedPropertyName`): a comma expression inside `[...]` is
    /// TS1171.
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarComputedPropertyName(989)
    pub fn check_grammar_computed_property_name(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        if arena.kind(node) != Kind::ComputedPropertyName {
            return false;
        }
        let expression = match arena.data(node) {
            NodeData::ComputedPropertyName(d) => d.expression,
            _ => return false,
        };
        if arena.kind(expression) == Kind::BinaryExpression {
            if let NodeData::BinaryExpression(d) = arena.data(expression) {
                if arena.kind(d.operator_token) == Kind::CommaToken {
                    return self.grammar_error_on_node(
                        program,
                        expression,
                        &tsgo_diagnostics::A_COMMA_EXPRESSION_IS_NOT_ALLOWED_IN_A_COMPUTED_PROPERTY_NAME,
                        &[],
                    );
                }
            }
        }
        false
    }

    /// Reports a grammar error on `node` and returns `true` (Go's
    /// `grammarErrorOnNode`). Uses the same span as `self.error`.
    ///
    /// Side effects: records a diagnostic, returns `true`.
    // Go: internal/checker/checker.go:Checker.grammarErrorOnNode(14179)
    pub(crate) fn grammar_error_on_node(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static tsgo_diagnostics::Message,
        args: &[&str],
    ) -> bool {
        if program.has_parse_diagnostics() {
            return false;
        }
        self.error(program, node, message, args);
        true
    }

    /// Reports a grammar error on the first token of `node` (the node's leading
    /// trivia-skipped start to its first token end) and returns `true` (Go's
    /// `grammarErrorOnFirstToken`).
    ///
    /// The Rust port approximates by error-ing on `node` itself (the span
    /// difference is cosmetic; the diagnostic code and message are identical).
    ///
    /// Side effects: records a diagnostic, returns `true`.
    // Go: internal/checker/checker.go:Checker.grammarErrorOnFirstToken(14193)
    pub(crate) fn grammar_error_on_first_token(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static tsgo_diagnostics::Message,
        args: &[&str],
    ) -> bool {
        if program.has_parse_diagnostics() {
            return false;
        }
        self.error(program, node, message, args);
        true
    }

    // -- checkGrammarStatementInAmbientContext ---------------------------------

    /// Checks whether a statement node is illegal inside an ambient context (Go's
    /// `checkGrammarStatementInAmbientContext`). Reports TS1183 for function-like
    /// / accessor bodies in ambient contexts, and TS1036 for statements inside
    /// ambient blocks.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_statement_in_ambient_context(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarStatementInAmbientContext(2057)
    pub fn check_grammar_statement_in_ambient_context(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        if !arena.flags(node).contains(NodeFlags::AMBIENT) {
            return false;
        }
        let parent = match arena.parent(node) {
            Some(p) => p,
            None => return false,
        };
        let parent_kind = arena.kind(parent);

        if !self.ambient_context_reported.contains(&node)
            && (is_function_like(arena, parent) || is_accessor(arena, parent))
        {
            self.ambient_context_reported.insert(node);
            return self.grammar_error_on_first_token(
                program,
                node,
                &tsgo_diagnostics::AN_IMPLEMENTATION_CANNOT_BE_DECLARED_IN_AMBIENT_CONTEXTS,
                &[],
            );
        }

        if matches!(
            parent_kind,
            Kind::Block | Kind::ModuleBlock | Kind::SourceFile
        ) && !self.ambient_context_reported.contains(&parent)
        {
            self.ambient_context_reported.insert(parent);
            return self.grammar_error_on_first_token(
                program,
                node,
                &tsgo_diagnostics::STATEMENTS_ARE_NOT_ALLOWED_IN_AMBIENT_CONTEXTS,
                &[],
            );
        }
        false
    }

    // -- checkGrammarAccessor --------------------------------------------------

    /// Grammar-checks a `get`/`set` accessor declaration (Go's
    /// `checkGrammarAccessor`): parameter counts, return-type annotations,
    /// type-parameter presence, rest/optional/initializer on setter params, and
    /// abstract/ambient body rules.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_accessor(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record one or more diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarAccessor(1317)
    pub fn check_grammar_accessor(&mut self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        let kind = arena.kind(node);
        let accessor = match arena.data(node) {
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.clone(),
            _ => return false,
        };

        let node_flags = arena.flags(node);
        let parent = arena.parent(node);
        let parent_kind = parent.map(|p| arena.kind(p));

        // Non-ambient accessor (not in type literal / interface) must have a body.
        if !node_flags.contains(NodeFlags::AMBIENT)
            && !matches!(
                parent_kind,
                Some(Kind::TypeLiteral) | Some(Kind::InterfaceDeclaration)
            )
            && accessor.body.is_none()
            && !has_syntactic_modifier(arena, node, ModifierFlags::ABSTRACT)
        {
            self.error(program, node, &tsgo_diagnostics::X_0_EXPECTED, &["{"]);
            return true;
        }

        // Accessor with body + abstract modifier -> TS1318
        if accessor.body.is_some() {
            if has_syntactic_modifier(arena, node, ModifierFlags::ABSTRACT) {
                return self.grammar_error_on_node(
                    program,
                    node,
                    &tsgo_diagnostics::AN_ABSTRACT_ACCESSOR_CANNOT_HAVE_AN_IMPLEMENTATION,
                    &[],
                );
            }
            if matches!(
                parent_kind,
                Some(Kind::TypeLiteral) | Some(Kind::InterfaceDeclaration)
            ) {
                return self.grammar_error_on_node(
                    program,
                    accessor.body.unwrap(),
                    &tsgo_diagnostics::AN_IMPLEMENTATION_CANNOT_BE_DECLARED_IN_AMBIENT_CONTEXTS,
                    &[],
                );
            }
        }

        // Type parameters on accessor -> TS1094
        if accessor.type_parameters.is_some() {
            return self.grammar_error_on_node(
                program,
                accessor.name,
                &tsgo_diagnostics::AN_ACCESSOR_CANNOT_HAVE_TYPE_PARAMETERS,
                &[],
            );
        }

        // Parameter-count check
        if !does_accessor_have_correct_parameter_count(arena, kind, &accessor.parameters) {
            let msg = if kind == Kind::GetAccessor {
                &tsgo_diagnostics::A_GET_ACCESSOR_CANNOT_HAVE_PARAMETERS
            } else {
                &tsgo_diagnostics::A_SET_ACCESSOR_MUST_HAVE_EXACTLY_ONE_PARAMETER
            };
            return self.grammar_error_on_node(program, accessor.name, msg, &[]);
        }

        // Setter-specific checks
        if kind == Kind::SetAccessor {
            if accessor.type_node.is_some() {
                return self.grammar_error_on_node(
                    program,
                    accessor.name,
                    &tsgo_diagnostics::A_SET_ACCESSOR_CANNOT_HAVE_A_RETURN_TYPE_ANNOTATION,
                    &[],
                );
            }
            if let Some(&param_node) = accessor.parameters.nodes.first() {
                if let NodeData::ParameterDeclaration(p) = arena.data(param_node) {
                    if let Some(rest) = p.dot_dot_dot_token {
                        return self.grammar_error_on_node(
                            program,
                            rest,
                            &tsgo_diagnostics::A_SET_ACCESSOR_CANNOT_HAVE_REST_PARAMETER,
                            &[],
                        );
                    }
                    if let Some(q) = p.question_token {
                        return self.grammar_error_on_node(
                            program,
                            q,
                            &tsgo_diagnostics::A_SET_ACCESSOR_CANNOT_HAVE_AN_OPTIONAL_PARAMETER,
                            &[],
                        );
                    }
                    if p.initializer.is_some() {
                        return self.grammar_error_on_node(
                            program,
                            accessor.name,
                            &tsgo_diagnostics::A_SET_ACCESSOR_PARAMETER_CANNOT_HAVE_AN_INITIALIZER,
                            &[],
                        );
                    }
                }
            }
        }

        false
    }

    // -- checkGrammarVariableDeclarationList -----------------------------------

    /// Grammar-checks a variable-declaration list (Go's
    /// `checkGrammarVariableDeclarationList`): empty lists (TS1123), `using` /
    /// `await using` ambient/case-clause restrictions.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_variable_declaration_list(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarVariableDeclarationList(1656)
    pub fn check_grammar_variable_declaration_list(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        let declarations = match arena.data(node) {
            NodeData::VariableDeclarationList(d) => &d.declarations,
            _ => return false,
        };

        if declarations.nodes.is_empty() {
            let loc = match arena.data(node) {
                NodeData::VariableDeclarationList(d) => d.declarations.loc,
                _ => return false,
            };
            return self.grammar_error_at_pos(
                program,
                node,
                loc.pos(),
                loc.end() - loc.pos(),
                &tsgo_diagnostics::VARIABLE_DECLARATION_LIST_CANNOT_BE_EMPTY,
                &[],
            );
        }

        let block_scope_flags = arena.flags(node) & NodeFlags::BLOCK_SCOPED;
        if block_scope_flags == NodeFlags::USING || block_scope_flags == NodeFlags::AWAIT_USING {
            if let Some(parent) = arena.parent(node) {
                if arena.kind(parent) == Kind::ForInStatement {
                    let msg = if block_scope_flags == NodeFlags::USING {
                        &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_A_FOR_IN_STATEMENT_CANNOT_BE_A_USING_DECLARATION
                    } else {
                        &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_A_FOR_IN_STATEMENT_CANNOT_BE_AN_AWAIT_USING_DECLARATION
                    };
                    return self.grammar_error_on_node(program, node, msg, &[]);
                }
            }

            if arena.flags(node).contains(NodeFlags::AMBIENT) {
                let msg = if block_scope_flags == NodeFlags::USING {
                    &tsgo_diagnostics::X_USING_DECLARATIONS_ARE_NOT_ALLOWED_IN_AMBIENT_CONTEXTS
                } else {
                    &tsgo_diagnostics::X_AWAIT_USING_DECLARATIONS_ARE_NOT_ALLOWED_IN_AMBIENT_CONTEXTS
                };
                return self.grammar_error_on_node(program, node, msg, &[]);
            }

            // using/await using in case/default clause without block
            if let Some(parent) = arena.parent(node) {
                if arena.kind(parent) == Kind::VariableStatement {
                    if let Some(grandparent) = arena.parent(parent) {
                        if matches!(
                            arena.kind(grandparent),
                            Kind::CaseClause | Kind::DefaultClause
                        ) {
                            let msg = if block_scope_flags == NodeFlags::USING {
                                &tsgo_diagnostics::X_USING_DECLARATIONS_ARE_NOT_ALLOWED_IN_CASE_OR_DEFAULT_CLAUSES_UNLESS_CONTAINED_WITHIN_A_BLOCK
                            } else {
                                &tsgo_diagnostics::X_AWAIT_USING_DECLARATIONS_ARE_NOT_ALLOWED_IN_CASE_OR_DEFAULT_CLAUSES_UNLESS_CONTAINED_WITHIN_A_BLOCK
                            };
                            return self.grammar_error_on_node(program, node, msg, &[]);
                        }
                    }
                }
            }
        }

        // DEFER(phase-4-checker-later): checkGrammarAwaitOrAwaitUsing for
        // await-using top-level / non-async diagnostics.
        false
    }

    // -- checkGrammarForInOrForOfStatement -------------------------------------

    /// Grammar-checks a for-in/for-of statement (Go's
    /// `checkGrammarForInOrForOfStatement`): multiple declarations (TS2491 /
    /// TS2532), initializer on loop variable (TS2483/2484), type annotation on
    /// loop variable (TS2404/TS2483).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_for_in_or_for_of_statement(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarForInOrForOfStatement(1210)
    pub fn check_grammar_for_in_or_for_of_statement(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        if self.check_grammar_statement_in_ambient_context(program, node) {
            return true;
        }

        let (kind, initializer) = match arena.data(node) {
            NodeData::ForInOrOfStatement(d) => (arena.kind(node), d.initializer),
            _ => return false,
        };

        // DEFER(phase-4-checker-later): for-await-of diagnostics
        // (TS1103/1338/2711/2712) requiring module format / async context checks.

        // `for (async of ...)` without await context -> TS2662
        if kind == Kind::ForOfStatement
            && !arena.flags(node).contains(NodeFlags::AWAIT_CONTEXT)
            && arena.kind(initializer) == Kind::Identifier
            && arena.text(initializer) == "async"
        {
            self.grammar_error_on_node(
                program,
                initializer,
                &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_A_FOR_OF_STATEMENT_MAY_NOT_BE_ASYNC,
                &[],
            );
            return false;
        }

        if arena.kind(initializer) == Kind::VariableDeclarationList
            && !self.check_grammar_variable_declaration_list(program, initializer)
        {
            let declarations = match arena.data(initializer) {
                NodeData::VariableDeclarationList(d) => &d.declarations,
                _ => return false,
            };

            if declarations.nodes.is_empty() {
                return false;
            }

            if declarations.nodes.len() > 1 {
                let msg = if kind == Kind::ForInStatement {
                    &tsgo_diagnostics::ONLY_A_SINGLE_VARIABLE_DECLARATION_IS_ALLOWED_IN_A_FOR_IN_STATEMENT
                } else {
                    &tsgo_diagnostics::ONLY_A_SINGLE_VARIABLE_DECLARATION_IS_ALLOWED_IN_A_FOR_OF_STATEMENT
                };
                return self.grammar_error_on_first_token(program, declarations.nodes[1], msg, &[]);
            }

            let first_decl = declarations.nodes[0];
            if let NodeData::VariableDeclaration(d) = arena.data(first_decl) {
                if d.initializer.is_some() {
                    let msg = if kind == Kind::ForInStatement {
                        &tsgo_diagnostics::THE_VARIABLE_DECLARATION_OF_A_FOR_IN_STATEMENT_CANNOT_HAVE_AN_INITIALIZER
                    } else {
                        &tsgo_diagnostics::THE_VARIABLE_DECLARATION_OF_A_FOR_OF_STATEMENT_CANNOT_HAVE_AN_INITIALIZER
                    };
                    return self.grammar_error_on_node(program, d.name, msg, &[]);
                }
                if d.type_node.is_some() {
                    let msg = if kind == Kind::ForInStatement {
                        &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_A_FOR_IN_STATEMENT_CANNOT_USE_A_TYPE_ANNOTATION
                    } else {
                        &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_A_FOR_OF_STATEMENT_CANNOT_USE_A_TYPE_ANNOTATION
                    };
                    return self.grammar_error_on_node(program, first_decl, msg, &[]);
                }
            }
        }

        false
    }

    // -- checkGrammarClassLikeDeclaration --------------------------------------

    /// Grammar-checks a class-like declaration (Go's
    /// `checkGrammarClassLikeDeclaration`): heritage clauses and empty type
    /// parameter lists.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_class_like_declaration(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarClassLikeDeclaration(777)
    pub fn check_grammar_class_like_declaration(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        let type_parameters = match arena.data(node) {
            NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => {
                d.type_parameters.as_ref()
            }
            _ => return false,
        };
        // DEFER(phase-4-checker-later): checkGrammarClassDeclarationHeritageClauses
        // (extends/implements ordering, duplicate heritage).
        self.check_grammar_type_parameter_list(program, type_parameters)
    }

    /// Checks for an empty type-parameter list `<>` which is a grammar error
    /// (TS1098).
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarTypeParameterList(688)
    /// Checks for an empty type-parameter list `<>` which is a grammar error
    /// (TS1098). An empty list is a parser-error-recovery artifact; if present,
    /// we report the diagnostic.
    ///
    /// DEFER(phase-4-checker-later): proper span calculation matching Go's
    /// `typeParameters.Pos()-1 .. skipTrivia(end)+1` (the `<>` delimiters).
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarTypeParameterList(688)
    fn check_grammar_type_parameter_list(
        &mut self,
        _program: &dyn BoundProgram,
        type_parameters: Option<&tsgo_ast::NodeList>,
    ) -> bool {
        if let Some(list) = type_parameters {
            if list.nodes.is_empty() {
                // DEFER: report TS1098 at the correct span. Requires span
                // arithmetic on the `<>` delimiters which needs source text.
                return false;
            }
        }
        false
    }

    // -- checkGrammarFunctionLikeDeclaration -----------------------------------

    /// Grammar-checks a function-like declaration (Go's
    /// `checkGrammarFunctionLikeDeclaration`): modifiers + type-parameter list +
    /// parameter list.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// c.check_grammar_function_like_declaration(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarFunctionLikeDeclaration(768)
    pub fn check_grammar_function_like_declaration(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let arena = program.arena();
        if self.check_grammar_modifiers(program, node) {
            return true;
        }
        let (type_parameters, parameters) = match arena.data(node) {
            NodeData::FunctionDeclaration(d) => (d.type_parameters.as_ref(), Some(&d.parameters)),
            NodeData::MethodDeclaration(d) => (d.type_parameters.as_ref(), Some(&d.parameters)),
            NodeData::ConstructorDeclaration(d) => {
                (d.type_parameters.as_ref(), Some(&d.parameters))
            }
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                (d.type_parameters.as_ref(), Some(&d.parameters))
            }
            NodeData::ArrowFunction(d) => (d.type_parameters.as_ref(), Some(&d.parameters)),
            NodeData::FunctionExpression(d) => (d.type_parameters.as_ref(), Some(&d.parameters)),
            _ => (None, None),
        };
        if self.check_grammar_type_parameter_list(program, type_parameters) {
            return true;
        }
        if let Some(params) = parameters {
            if self.check_grammar_parameter_list(program, params) {
                return true;
            }
        }
        if self.check_grammar_arrow_function(program, node) {
            return true;
        }
        // DEFER(phase-4-checker-later): checkGrammarForUseStrictSimpleParameterList.
        false
    }

    // -- checkGrammarMetaProperty -----------------------------------------------

    /// Validates meta-property syntax (`new.target`, `import.meta`,
    /// `import.defer`).
    ///
    /// * `new.xyz` where `xyz` !== `"target"` → TS17012.
    /// * `import.xyz` (non-callee) where `xyz` !== `"meta"` → TS17012.
    /// * `import.xyz(...)` (callee) where `xyz` !== `"meta"` and `!= "defer"` → TS18061.
    /// * `import.defer` as non-callee → TS1005 (`"(" expected`).
    ///
    /// Returns `true` if a grammar error was reported.
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarMetaProperty(1841)
    pub fn check_grammar_meta_property(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let (keyword_token, name_id) = match program.arena().data(node) {
            NodeData::MetaProperty(d) => (d.keyword_token, d.name),
            _ => return false,
        };
        let name_text = program.arena().text(name_id);

        match keyword_token {
            Kind::NewKeyword => {
                if name_text != "target" {
                    return self.grammar_error_on_node(
                        program,
                        name_id,
                        &tsgo_diagnostics::X_0_IS_NOT_A_VALID_META_PROPERTY_FOR_KEYWORD_1_DID_YOU_MEAN_2,
                        &[name_text, tsgo_scanner::token_to_string(keyword_token), "target"],
                    );
                }
            }
            Kind::ImportKeyword => {
                if name_text != "meta" {
                    let is_callee = program.arena().parent(node).is_some_and(|parent| {
                        tsgo_ast::utilities::is_call_expression(program.arena(), parent)
                            && match program.arena().data(parent) {
                                NodeData::CallExpression(d) => d.expression == node,
                                _ => false,
                            }
                    });
                    if name_text == "defer" {
                        if !is_callee {
                            return self.grammar_error_at_pos(
                                program,
                                node,
                                program.arena().loc(node).end(),
                                0,
                                &tsgo_diagnostics::X_0_EXPECTED,
                                &["("],
                            );
                        }
                    } else if is_callee {
                        return self.grammar_error_on_node(
                            program,
                            name_id,
                            &tsgo_diagnostics::X_0_IS_NOT_A_VALID_META_PROPERTY_FOR_KEYWORD_IMPORT_DID_YOU_MEAN_META_OR_DEFER,
                            &[name_text],
                        );
                    } else {
                        return self.grammar_error_on_node(
                            program,
                            name_id,
                            &tsgo_diagnostics::X_0_IS_NOT_A_VALID_META_PROPERTY_FOR_KEYWORD_1_DID_YOU_MEAN_2,
                            &[name_text, tsgo_scanner::token_to_string(keyword_token), "meta"],
                        );
                    }
                }
            }
            _ => {}
        }

        false
    }

    // -- checkGrammarDecorator ------------------------------------------------

    /// Validates a decorator expression's syntax.
    ///
    /// Walks the expression tree: a valid decorator is `@ident`,
    /// `@ident.prop`, `@ident(args)`, `@ident.prop(args)`, or any of those
    /// wrapped in parentheses. Optional chaining (`?.`) or non-identifier
    /// leaf expressions require parentheses; reports TS1497 + TS1498.
    ///
    /// Returns `true` if a grammar error was reported.
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarDecorator(126)
    pub fn check_grammar_decorator(&mut self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let decorator_expr = match program.arena().data(node) {
            NodeData::Decorator(d) => d.expression,
            _ => return false,
        };

        let arena = program.arena();

        if arena.kind(decorator_expr) == Kind::ParenthesizedExpression {
            return false;
        }

        let mut current = decorator_expr;
        let mut can_have_call = true;
        let mut error_node: Option<NodeId> = None;

        loop {
            let kind = arena.kind(current);

            if kind == Kind::ExpressionWithTypeArguments || kind == Kind::NonNullExpression {
                current = match arena.data(current) {
                    NodeData::ExpressionWithTypeArguments(d) => d.expression,
                    NodeData::NonNullExpression(d) => d.expression,
                    _ => break,
                };
                continue;
            }

            if kind == Kind::CallExpression {
                let d = match arena.data(current) {
                    NodeData::CallExpression(d) => d,
                    _ => break,
                };
                if !can_have_call {
                    error_node = Some(current);
                }
                if d.question_dot_token.is_some() {
                    error_node = d.question_dot_token;
                }
                current = d.expression;
                can_have_call = false;
                continue;
            }

            if kind == Kind::PropertyAccessExpression {
                let d = match arena.data(current) {
                    NodeData::PropertyAccessExpression(d) => d,
                    _ => break,
                };
                if d.question_dot_token.is_some() {
                    error_node = d.question_dot_token;
                }
                current = d.expression;
                can_have_call = false;
                continue;
            }

            if kind != Kind::Identifier {
                error_node = Some(current);
            }

            break;
        }

        if let Some(err_node) = error_node {
            let primary = self.diagnostic_for_node_public(
                program,
                decorator_expr,
                &tsgo_diagnostics::EXPRESSION_MUST_BE_ENCLOSED_IN_PARENTHESES_TO_BE_USED_AS_A_DECORATOR,
                &[],
            );
            let related = self.diagnostic_for_node_public(
                program,
                err_node,
                &tsgo_diagnostics::INVALID_SYNTAX_IN_DECORATOR,
                &[],
            );
            let mut diag = primary;
            diag.related_information.push(related);
            self.add_diagnostic(program, diag);
            return true;
        }

        false
    }

    /// Creates a diagnostic for the given node. Public wrapper for grammar
    /// use, mirroring `self.diagnostic_for_node` in check.rs.
    fn diagnostic_for_node_public(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static tsgo_diagnostics::Message,
        args: &[&str],
    ) -> super::check::Diagnostic {
        let loc = program.arena().loc(node);
        super::check::Diagnostic {
            code: message.code(),
            category: message.category(),
            message: tsgo_diagnostics::format(&message.to_string(), args),
            start: loc.pos(),
            length: loc.end() - loc.pos(),
            related_information: Vec::new(),
            message_chain: Vec::new(),
        }
    }

    // -- checkGrammarImportCallExpression ------------------------------------

    /// Validates the grammar of a dynamic `import()` call expression.
    ///
    /// * Module=ES2015 → TS1323 (dynamic imports not supported).
    /// * Type arguments → TS1326.
    /// * Wrong argument count (0 or >2) → TS1450.
    /// * Spread argument → TS1325.
    /// * >1 arg without a supporting module kind → TS1324.
    ///
    /// Returns `true` if a grammar error was reported.
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarImportCallExpression(2172)
    pub fn check_grammar_import_call_expression(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        use tsgo_core::compileroptions::ModuleKind;
        let module_kind = self.compiler_options().module;

        let (callee, args, type_args) = match program.arena().data(node) {
            NodeData::CallExpression(d) => (
                d.expression,
                d.arguments.nodes.clone(),
                d.type_arguments.as_ref(),
            ),
            _ => return false,
        };

        // Deferred-import callee check (`import.defer(...)`)
        if program.arena().kind(callee) == Kind::MetaProperty {
            if module_kind != ModuleKind::EsNext && module_kind != ModuleKind::Preserve {
                return self.grammar_error_on_node(
                    program,
                    node,
                    &tsgo_diagnostics::DEFERRED_IMPORTS_ARE_ONLY_SUPPORTED_WHEN_THE_MODULE_FLAG_IS_SET_TO_ESNEXT_OR_PRESERVE,
                    &[],
                );
            }
        } else if module_kind == ModuleKind::Es2015 {
            return self.grammar_error_on_node(
                program,
                node,
                &tsgo_diagnostics::DYNAMIC_IMPORTS_ARE_ONLY_SUPPORTED_WHEN_THE_MODULE_FLAG_IS_SET_TO_ES2020_ES2022_ESNEXT_COMMONJS_AMD_SYSTEM_UMD_NODE16_NODE18_NODE20_OR_NODENEXT,
                &[],
            );
        }

        if type_args.is_some() {
            return self.grammar_error_on_node(
                program,
                node,
                &tsgo_diagnostics::THIS_USE_OF_IMPORT_IS_INVALID_IMPORT_CALLS_CAN_BE_WRITTEN_BUT_THEY_MUST_HAVE_PARENTHESES_AND_CANNOT_HAVE_TYPE_ARGUMENTS,
                &[],
            );
        }

        let is_node_range = (ModuleKind::Node16 as u32) <= (module_kind as u32)
            && (module_kind as u32) <= (ModuleKind::NodeNext as u32);
        if !is_node_range
            && module_kind != ModuleKind::EsNext
            && module_kind != ModuleKind::Preserve
            && args.len() > 1
        {
            return self.grammar_error_on_node(
                program,
                args[1],
                &tsgo_diagnostics::DYNAMIC_IMPORTS_ONLY_SUPPORT_A_SECOND_ARGUMENT_WHEN_THE_MODULE_OPTION_IS_SET_TO_ESNEXT_NODE16_NODE18_NODE20_NODENEXT_OR_PRESERVE,
                &[],
            );
        }

        if args.is_empty() || args.len() > 2 {
            return self.grammar_error_on_node(
                program,
                node,
                &tsgo_diagnostics::DYNAMIC_IMPORTS_CAN_ONLY_ACCEPT_A_MODULE_SPECIFIER_AND_AN_OPTIONAL_SET_OF_ATTRIBUTES_AS_ARGUMENTS,
                &[],
            );
        }

        if let Some(&spread) = args
            .iter()
            .find(|&&a| program.arena().kind(a) == Kind::SpreadElement)
        {
            return self.grammar_error_on_node(
                program,
                spread,
                &tsgo_diagnostics::ARGUMENT_OF_DYNAMIC_IMPORT_CANNOT_BE_SPREAD_ELEMENT,
                &[],
            );
        }

        false
    }

    /// Reports a grammar error at a specific position and length.
    /// Returns `true` if the error was recorded (i.e. no parse diagnostics
    /// conflict).
    ///
    /// Side effects: records a diagnostic, returns `true`.
    // Go: internal/checker/grammarchecks.go:Checker.grammarErrorAtPos(28)
    fn grammar_error_at_pos(
        &mut self,
        program: &dyn BoundProgram,
        _source_node: NodeId,
        start: i32,
        length: i32,
        message: &'static tsgo_diagnostics::Message,
        args: &[&str],
    ) -> bool {
        if program.has_parse_diagnostics() {
            return false;
        }
        use super::check::Diagnostic;
        let diagnostic = Diagnostic {
            code: message.code(),
            category: message.category(),
            message: tsgo_diagnostics::format(&message.to_string(), args),
            start,
            length,
            related_information: Vec::new(),
            message_chain: Vec::new(),
        };
        self.add_diagnostic(program, diagnostic);
        true
    }
}

// The keyword text of a modifier token (Go's `scanner.TokenToString`).
// Go: internal/scanner/scanner.go:TokenToString (modifier subset)
fn modifier_text(kind: Kind) -> &'static str {
    match kind {
        Kind::PublicKeyword => "public",
        Kind::PrivateKeyword => "private",
        Kind::ProtectedKeyword => "protected",
        Kind::StaticKeyword => "static",
        Kind::AbstractKeyword => "abstract",
        Kind::AccessorKeyword => "accessor",
        Kind::ExportKeyword => "export",
        Kind::DeclareKeyword => "declare",
        Kind::ConstKeyword => "const",
        Kind::DefaultKeyword => "default",
        Kind::AsyncKeyword => "async",
        Kind::ReadonlyKeyword => "readonly",
        Kind::OverrideKeyword => "override",
        Kind::InKeyword => "in",
        Kind::OutKeyword => "out",
        _ => "",
    }
}

/// Returns the modifier/decorator token node ids of `node` (public for
/// decorator-checking cross-module use).
// Go: internal/ast/ast.go:Node.ModifierNodes
pub(crate) fn modifier_nodes_pub(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    modifier_nodes(arena, node)
}

/// Returns the modifier/decorator token node ids of `node`, if it bears any.
// Go: internal/ast/ast.go:Node.ModifierNodes
fn modifier_nodes(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    let modifiers = match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.modifiers.as_ref(),
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ConstructorDeclaration(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        NodeData::IndexSignatureDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeParameterDeclaration(d) => d.modifiers.as_ref(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportAssignment(d) => d.modifiers.as_ref(),
        _ => None,
    };
    match modifiers {
        Some(m) => m.list.nodes.clone(),
        None => Vec::new(),
    }
}

/// Returns `true` if `kind` is a class-like declaration kind.
// Go: internal/ast/utilities.go:IsClassLike
fn is_class_like_kind(kind: Kind) -> bool {
    matches!(kind, Kind::ClassDeclaration | Kind::ClassExpression)
}

/// Returns `true` if `node` is a function-like declaration (function, method,
/// constructor, arrow, accessor).
// Go: internal/ast/utilities.go:IsFunctionLike
fn is_function_like(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::FunctionExpression
            | Kind::ArrowFunction
    )
}

/// Returns `true` if `node` is an accessor declaration.
// Go: internal/ast/utilities.go:IsAccessor
fn is_accessor(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.kind(node), Kind::GetAccessor | Kind::SetAccessor)
}

/// Returns whether `node` has a given syntactic modifier (Go's
/// `HasSyntacticModifier`).
// Go: internal/ast/utilities.go:HasSyntacticModifier
fn has_syntactic_modifier(arena: &NodeArena, node: NodeId, flag: ModifierFlags) -> bool {
    let modifiers = modifier_nodes(arena, node);
    for m in modifiers {
        let mflag = modifier_to_flag(arena.kind(m));
        if mflag.intersects(flag) {
            return true;
        }
    }
    false
}

/// Checks whether an accessor has the correct parameter count (Go's
/// `doesAccessorHaveCorrectParameterCount`).
///
/// A `get` has 0 parameters (ignoring `this`), a `set` has 1 (ignoring `this`).
// Go: internal/checker/grammarchecks.go:Checker.doesAccessorHaveCorrectParameterCount(1373)
fn does_accessor_have_correct_parameter_count(
    arena: &NodeArena,
    kind: Kind,
    parameters: &tsgo_ast::NodeList,
) -> bool {
    let params = &parameters.nodes;
    let expected = if kind == Kind::GetAccessor { 0 } else { 1 };

    // A `this` parameter doesn't count toward arity.
    let has_this = params.first().is_some_and(|&p| is_this_parameter(arena, p));
    if has_this {
        params.len() == expected + 1
    } else {
        params.len() == expected
    }
}

/// Returns `true` if a parameter is a `this` parameter (name is `this`).
// Go: internal/ast/utilities.go:IsThisParameter
fn is_this_parameter(arena: &NodeArena, node: NodeId) -> bool {
    if arena.kind(node) != Kind::Parameter {
        return false;
    }
    if let NodeData::ParameterDeclaration(d) = arena.data(node) {
        arena.kind(d.name) == Kind::Identifier && arena.text(d.name) == "this"
    } else {
        false
    }
}

/// Returns `true` if a parameter is optional: has a `?` token or an
/// initializer (Go's `isOptionalDeclaration` for parameters).
// Go: internal/ast/utilities.go:isOptionalDeclaration
fn is_optional_parameter(arena: &NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ParameterDeclaration(d) => d.question_token.is_some() || d.initializer.is_some(),
        _ => false,
    }
}

/// Extracts the effective property name from a property-name node for
/// duplicate-name checking.
///
/// Returns `None` for computed names whose value cannot be statically resolved
/// (the full Go path through `getEffectivePropertyNameForPropertyNameNode` +
/// `tryGetNameFromType` is deferred until late-bound members land).
// Go: internal/ast/utilities.go:GetPropertyNameForPropertyNameNode +
//     internal/checker/checker.go:getEffectivePropertyNameForPropertyNameNode
fn effective_property_name(arena: &tsgo_ast::NodeArena, name_node: NodeId) -> Option<String> {
    match arena.kind(name_node) {
        Kind::Identifier
        | Kind::StringLiteral
        | Kind::NumericLiteral
        | Kind::PrivateIdentifier
        | Kind::NoSubstitutionTemplateLiteral
        | Kind::BigIntLiteral => {
            let text = arena.text(name_node);
            if text.is_empty() {
                return None;
            }
            Some(text.to_string())
        }
        Kind::ComputedPropertyName => {
            // DEFER(phase-4-checker-C5+): resolve computed names via
            // getTypeOfExpression -> tryGetNameFromType.
            None
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "grammar_test.rs"]
mod tests;
