//! Port of Go `internal/transformers/declarations/diagnostics.go`: the
//! declaration-emit diagnostic *message-selection* tables — which message a
//! given declaration node maps to for a symbol-accessibility failure
//! (`getSymbolAccessibilityDiagnostic` family) or an `--isolatedDeclarations`
//! missing-annotation failure (`getErrorByDeclarationKind` /
//! `getRelatedSuggestionByDeclarationKind`).
//!
//! # Scope (round D-F3, reachable subset)
//!
//! Go's `diagnostics.go` is a large dispatch over every declaration kind that
//! can host an inaccessible type, selecting among the "from external module but
//! cannot be named" (4023-family) / "from private module" (4024-family) /
//! "private name" (4025-family) variants. This port covers the reachable
//! single-file subset: a variable declaration whose type names a private name
//! maps to `4025` (the non-module-name variant), and the
//! `--isolatedDeclarations` function/method missing-return-annotation maps to
//! `9007`/`9008` with their `9031`/`9034` "add a return type" suggestions.
//!
//! # Deferred (with blocked-by)
//!
//! - The property/method/accessor/parameter/type-parameter/heritage/`import =`/
//!   type-alias arms of `createGetSymbolAccessibilityDiagnosticForNode`, plus
//!   the `_from_external_module_..._but_cannot_be_named` (4023) and
//!   `_from_private_module_` (4024) variants (which need the cross-module
//!   `ErrorModuleName`). blocked-by: `GetEffectiveDeclarationFlags` member
//!   visibility + cross-module symbol-accessibility (`SymbolAccessibilityResult`).
//! - The full `getErrorByDeclarationKind` set beyond function/method return
//!   types (parameter 9011, variable 9010, property 9012, accessor 9009,
//!   computed-name / spread / array-literal / default-export forms). blocked-by:
//!   the pseudo-type node builder's per-construct inference-fallback reporting.

use tsgo_ast::{Kind, NodeArena, NodeId};
use tsgo_diagnostics::Message;

/// Selects the symbol-accessibility diagnostic message for declaration `node`
/// when its emitted type references an inaccessible (private) name — Go's
/// `createGetSymbolAccessibilityDiagnosticForNode` reachable subset.
///
/// Returns the message used to report "exported X has or is using private name
/// Y". The reachable subset covers a variable declaration → `4025`
/// (`Exported variable '{0}' has or is using private name '{1}'.`); other
/// declaration kinds and the from-module variants are deferred (`None`).
///
/// # Examples
/// ```
/// use tsgo_ast::NodeArena;
/// use tsgo_transformers::declarations::diagnostics::get_symbol_accessibility_diagnostic_message;
/// let mut a = NodeArena::new();
/// let name = a.new_identifier("b");
/// let decl = a.new_variable_declaration(name, None, None, None);
/// let message = get_symbol_accessibility_diagnostic_message(&a, decl).unwrap();
/// assert_eq!(message.code(), 4025);
/// ```
///
/// Side effects: none (reads `arena`).
// Go: internal/transformers/declarations/diagnostics.go:createGetSymbolAccessibilityDiagnosticForNode / getVariableDeclarationTypeVisibilityDiagnosticMessage
pub fn get_symbol_accessibility_diagnostic_message(
    arena: &NodeArena,
    node: NodeId,
) -> Option<&'static Message> {
    match arena.kind(node) {
        // Go: getVariableDeclarationTypeVisibilityDiagnosticMessage's
        // VariableDeclaration/BindingElement arm, non-module-name variant.
        Kind::VariableDeclaration | Kind::BindingElement => {
            Some(&tsgo_diagnostics::EXPORTED_VARIABLE_0_HAS_OR_IS_USING_PRIVATE_NAME_1)
        }
        _ => None,
    }
}

/// Selects the `--isolatedDeclarations` "must have an explicit type annotation"
/// error message for declaration kind `kind` — Go's `getErrorByDeclarationKind`
/// reachable subset.
///
/// Covers a function (`9007`) and a method / construct signature (`9008`) that
/// lack an inferable explicit return type; the remaining kinds (parameter,
/// variable, property, accessor, computed name, spread, array literal, default
/// export) are deferred (`None`).
///
/// # Examples
/// ```
/// use tsgo_ast::Kind;
/// use tsgo_transformers::declarations::diagnostics::get_error_by_declaration_kind;
/// assert_eq!(
///     get_error_by_declaration_kind(Kind::FunctionDeclaration).unwrap().code(),
///     9007
/// );
/// assert_eq!(
///     get_error_by_declaration_kind(Kind::MethodDeclaration).unwrap().code(),
///     9008
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/declarations/diagnostics.go:getErrorByDeclarationKind
pub fn get_error_by_declaration_kind(kind: Kind) -> Option<&'static Message> {
    match kind {
        Kind::FunctionDeclaration | Kind::FunctionExpression | Kind::ArrowFunction => {
            Some(&tsgo_diagnostics::FUNCTION_MUST_HAVE_AN_EXPLICIT_RETURN_TYPE_ANNOTATION_WITH_ISOLATEDDECLARATIONS)
        }
        Kind::MethodDeclaration | Kind::ConstructSignature => {
            Some(&tsgo_diagnostics::METHOD_MUST_HAVE_AN_EXPLICIT_RETURN_TYPE_ANNOTATION_WITH_ISOLATEDDECLARATIONS)
        }
        _ => None,
    }
}

/// Selects the related "add a return type to ..." suggestion message for
/// declaration kind `kind` — Go's `getRelatedSuggestionByDeclarationKind`
/// reachable subset (function → `9031`, method → `9034`).
///
/// # Examples
/// ```
/// use tsgo_ast::Kind;
/// use tsgo_transformers::declarations::diagnostics::get_related_suggestion_by_declaration_kind;
/// assert_eq!(
///     get_related_suggestion_by_declaration_kind(Kind::FunctionDeclaration).unwrap().code(),
///     9031
/// );
/// assert_eq!(
///     get_related_suggestion_by_declaration_kind(Kind::MethodDeclaration).unwrap().code(),
///     9034
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/declarations/diagnostics.go:getRelatedSuggestionByDeclarationKind
pub fn get_related_suggestion_by_declaration_kind(kind: Kind) -> Option<&'static Message> {
    match kind {
        Kind::FunctionDeclaration | Kind::ConstructSignature => {
            Some(&tsgo_diagnostics::ADD_A_RETURN_TYPE_TO_THE_FUNCTION_DECLARATION)
        }
        Kind::MethodDeclaration => Some(&tsgo_diagnostics::ADD_A_RETURN_TYPE_TO_THE_METHOD),
        _ => None,
    }
}
