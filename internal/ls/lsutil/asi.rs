//! Automatic-semicolon-insertion (ASI) syntax classification.
//!
//! 1:1 port of Go `internal/ls/lsutil/asi.go`. These predicates classify a
//! syntax [`Kind`] by what terminator the grammar expects after it (semicolon,
//! comma-or-semicolon, function/module block, ...), which drives ASI-related
//! decisions in the formatter and code-fix layers.
//!
//! The node-level entry points `NodeIsASICandidate`/`PositionIsASICandidate`
//! are deferred (see crate docs): they need `astnav.FindNextToken` over a shared
//! arena and the deferred `scanner::GetECMALineOfPosition`.

use tsgo_ast::Kind;

/// Reports whether `kind` may be an ASI candidate, i.e. whether any of the more
/// specific terminator classifications applies.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_may_be_asi_candidate;
/// use tsgo_ast::Kind;
/// assert!(syntax_may_be_asi_candidate(Kind::VariableStatement));
/// assert!(syntax_may_be_asi_candidate(Kind::FunctionDeclaration));
/// assert!(!syntax_may_be_asi_candidate(Kind::SourceFile));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxMayBeASICandidate
pub fn syntax_may_be_asi_candidate(kind: Kind) -> bool {
    syntax_requires_trailing_comma_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_function_block_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_module_block_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_semicolon_or_asi(kind)
}

/// Reports whether the grammar allows a trailing comma, semicolon, or ASI after
/// a node of this kind (the type-member signatures).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_comma_or_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_comma_or_semicolon_or_asi(Kind::PropertySignature));
/// assert!(!syntax_requires_trailing_comma_or_semicolon_or_asi(Kind::VariableStatement));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingCommaOrSemicolonOrASI
pub fn syntax_requires_trailing_comma_or_semicolon_or_asi(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::PropertySignature
            | Kind::MethodSignature
    )
}

/// Reports whether the grammar allows a trailing function block, semicolon, or
/// ASI after a node of this kind (function-like declarations).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_function_block_or_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_function_block_or_semicolon_or_asi(Kind::FunctionDeclaration));
/// assert!(!syntax_requires_trailing_function_block_or_semicolon_or_asi(Kind::ModuleDeclaration));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingFunctionBlockOrSemicolonOrASI
pub fn syntax_requires_trailing_function_block_or_semicolon_or_asi(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::Constructor
            | Kind::MethodDeclaration
            | Kind::GetAccessor
            | Kind::SetAccessor
    )
}

/// Reports whether the grammar allows a trailing module block, semicolon, or
/// ASI after a node of this kind (module declarations).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_module_block_or_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_module_block_or_semicolon_or_asi(Kind::ModuleDeclaration));
/// assert!(!syntax_requires_trailing_module_block_or_semicolon_or_asi(Kind::FunctionDeclaration));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingModuleBlockOrSemicolonOrASI
pub fn syntax_requires_trailing_module_block_or_semicolon_or_asi(kind: Kind) -> bool {
    kind == Kind::ModuleDeclaration
}

/// Reports whether the grammar requires a trailing semicolon (or ASI) after a
/// node of this kind (statements and declarations terminated by `;`).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_semicolon_or_asi(Kind::ReturnStatement));
/// assert!(!syntax_requires_trailing_semicolon_or_asi(Kind::FunctionDeclaration));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingSemicolonOrASI
pub fn syntax_requires_trailing_semicolon_or_asi(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::VariableStatement
            | Kind::ExpressionStatement
            | Kind::DoStatement
            | Kind::ContinueStatement
            | Kind::BreakStatement
            | Kind::ReturnStatement
            | Kind::ThrowStatement
            | Kind::DebuggerStatement
            | Kind::PropertyDeclaration
            | Kind::TypeAliasDeclaration
            | Kind::ImportDeclaration
            | Kind::ImportEqualsDeclaration
            | Kind::ExportDeclaration
            | Kind::NamespaceExportDeclaration
            | Kind::ExportAssignment
    )
}

#[cfg(test)]
#[path = "asi_test.rs"]
mod tests;
