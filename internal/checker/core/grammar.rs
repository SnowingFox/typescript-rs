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
use tsgo_ast::{Kind, ModifierFlags, NodeData, NodeId};

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
            ) && flags.intersects(ModifierFlags::ACCESSIBILITY_MODIFIER)
            {
                self.error(
                    program,
                    modifier,
                    &tsgo_diagnostics::ACCESSIBILITY_MODIFIER_ALREADY_SEEN,
                    &[],
                );
                return true;
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
            flags |= flag;
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
