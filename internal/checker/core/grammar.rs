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
use tsgo_ast::{Kind, ModifierFlags, NodeData, NodeFlags, NodeId};

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
        let modifiers = modifier_nodes(program.arena(), node);
        if modifiers.is_empty() {
            return false;
        }
        let node_kind = program.arena().kind(node);
        let mut flags = ModifierFlags::empty();
        for modifier in modifiers {
            let kind = program.arena().kind(modifier);
            // DEFER(phase-4-checker-4j+): decorator grammar.
            if kind == Kind::Decorator {
                continue;
            }
            let flag = modifier_to_flag(kind);
            if matches!(
                kind,
                Kind::PublicKeyword | Kind::ProtectedKeyword | Kind::PrivateKeyword
            ) {
                if flags.intersects(ModifierFlags::ACCESSIBILITY_MODIFIER) {
                    self.error(
                        program,
                        modifier,
                        &tsgo_diagnostics::ACCESSIBILITY_MODIFIER_ALREADY_SEEN,
                        &[],
                    );
                    return true;
                }
                // An accessibility modifier must precede `static`.
                if flags.contains(ModifierFlags::STATIC) {
                    self.error(
                        program,
                        modifier,
                        &tsgo_diagnostics::X_0_MODIFIER_MUST_PRECEDE_1_MODIFIER,
                        &[modifier_text(kind), "static"],
                    );
                    return true;
                }
            }
            if kind == Kind::AsyncKeyword && flags.contains(ModifierFlags::AMBIENT) {
                self.error(
                    program,
                    modifier,
                    &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_IN_AN_AMBIENT_CONTEXT,
                    &["async"],
                );
                return true;
            }
            if !flag.is_empty() && flags.contains(flag) {
                self.error(
                    program,
                    modifier,
                    &tsgo_diagnostics::X_0_MODIFIER_ALREADY_SEEN,
                    &[modifier_text(kind)],
                );
                return true;
            }
            // The `accessor` modifier cannot be combined with `readonly`.
            if kind == Kind::AccessorKeyword && flags.contains(ModifierFlags::READONLY) {
                self.error(
                    program,
                    modifier,
                    &tsgo_diagnostics::X_0_MODIFIER_CANNOT_BE_USED_WITH_1_MODIFIER,
                    &["accessor", "readonly"],
                );
                return true;
            }
            // The `accessor` modifier may only appear on a property declaration.
            if kind == Kind::AccessorKeyword && node_kind != Kind::PropertyDeclaration {
                self.error(
                    program,
                    modifier,
                    &tsgo_diagnostics::X_ACCESSOR_MODIFIER_CAN_ONLY_APPEAR_ON_A_PROPERTY_DECLARATION,
                    &[],
                );
                return true;
            }
            // The `abstract` modifier may appear on a class declaration (and a
            // constructor type), or on a class member (method/property/accessor)
            // whose enclosing class is itself `abstract`; otherwise it reports
            // 1244 "Abstract methods can only appear within an abstract class."
            // (Go's `checkGrammarModifiers` abstract arm, reachable subset).
            //
            // DEFER(phase-4-checker-C-D2+): the "abstract modifier can only
            // appear on a class, method, or property declaration" (1242), the
            // `abstract` + `static`/`private`/`async`/`readonly`/`accessor`/
            // `override`/`declare` incompatibilities, and the abstract-property
            // initializer / abstract-constructor checks. blocked-by: the full
            // modifier-compatibility matrix.
            if kind == Kind::AbstractKeyword
                && node_kind != Kind::ClassDeclaration
                && node_kind != Kind::ConstructorType
            {
                let parent_is_abstract_class = program
                    .arena()
                    .parent(node)
                    .filter(|&p| program.arena().kind(p) == Kind::ClassDeclaration)
                    .is_some_and(|p| {
                        modifier_nodes(program.arena(), p)
                            .iter()
                            .any(|&m| program.arena().kind(m) == Kind::AbstractKeyword)
                    });
                if !parent_is_abstract_class {
                    self.error(
                        program,
                        modifier,
                        &tsgo_diagnostics::ABSTRACT_METHODS_CAN_ONLY_APPEAR_WITHIN_AN_ABSTRACT_CLASS,
                        &[],
                    );
                    return true;
                }
            }
            // `readonly` may only appear on a property declaration or index
            // signature (a parameter property is handled by `checkParameter`).
            if kind == Kind::ReadonlyKeyword
                && !matches!(
                    node_kind,
                    Kind::PropertyDeclaration
                        | Kind::PropertySignature
                        | Kind::IndexSignature
                        | Kind::Parameter
                )
            {
                self.error(
                    program,
                    modifier,
                    &tsgo_diagnostics::X_READONLY_MODIFIER_CAN_ONLY_APPEAR_ON_A_PROPERTY_DECLARATION_OR_INDEX_SIGNATURE,
                    &[],
                );
                return true;
            }
            flags |= flag;
        }
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
                // Report on the first type parameter; Go reports over the whole
                // (trivia-trimmed) `<...>` list span.
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

// Returns the modifier/decorator token node ids of `node`, if it bears any.
// Go: internal/ast/ast.go:Node.ModifierNodes
fn modifier_nodes(arena: &tsgo_ast::NodeArena, node: NodeId) -> Vec<NodeId> {
    let modifiers = match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    match modifiers {
        Some(m) => m.list.nodes.clone(),
        None => Vec::new(),
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
