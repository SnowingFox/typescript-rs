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

use tsgo_ast::utilities::modifier_to_flag;
use tsgo_ast::{Kind, ModifierFlags, NodeData, NodeFlags, NodeId};

use super::program::BoundProgram;
use super::Checker;

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

#[cfg(test)]
#[path = "grammar_test.rs"]
mod tests;
