use super::*;
use tsgo_ast::Kind;

// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingSemicolonOrASI
#[test]
fn semicolon_or_asi_covers_all_terminated_statements() {
    for kind in [
        Kind::VariableStatement,
        Kind::ExpressionStatement,
        Kind::DoStatement,
        Kind::ContinueStatement,
        Kind::BreakStatement,
        Kind::ReturnStatement,
        Kind::ThrowStatement,
        Kind::DebuggerStatement,
        Kind::PropertyDeclaration,
        Kind::TypeAliasDeclaration,
        Kind::ImportDeclaration,
        Kind::ImportEqualsDeclaration,
        Kind::ExportDeclaration,
        Kind::NamespaceExportDeclaration,
        Kind::ExportAssignment,
    ] {
        assert!(
            syntax_requires_trailing_semicolon_or_asi(kind),
            "{kind:?} should require trailing semicolon/ASI"
        );
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingSemicolonOrASI
#[test]
fn semicolon_or_asi_rejects_others() {
    for kind in [
        Kind::FunctionDeclaration,
        Kind::ModuleDeclaration,
        Kind::PropertySignature,
        Kind::SourceFile,
        Kind::Block,
        Kind::Identifier,
    ] {
        assert!(!syntax_requires_trailing_semicolon_or_asi(kind), "{kind:?}");
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingCommaOrSemicolonOrASI
#[test]
fn comma_or_semicolon_or_asi_covers_type_member_signatures() {
    for kind in [
        Kind::CallSignature,
        Kind::ConstructSignature,
        Kind::IndexSignature,
        Kind::PropertySignature,
        Kind::MethodSignature,
    ] {
        assert!(
            syntax_requires_trailing_comma_or_semicolon_or_asi(kind),
            "{kind:?}"
        );
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingCommaOrSemicolonOrASI
#[test]
fn comma_or_semicolon_or_asi_rejects_others() {
    for kind in [
        Kind::VariableStatement,
        Kind::FunctionDeclaration,
        Kind::MethodDeclaration,
        Kind::ModuleDeclaration,
    ] {
        assert!(
            !syntax_requires_trailing_comma_or_semicolon_or_asi(kind),
            "{kind:?}"
        );
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingFunctionBlockOrSemicolonOrASI
#[test]
fn function_block_or_semicolon_or_asi_covers_function_like() {
    for kind in [
        Kind::FunctionDeclaration,
        Kind::Constructor,
        Kind::MethodDeclaration,
        Kind::GetAccessor,
        Kind::SetAccessor,
    ] {
        assert!(
            syntax_requires_trailing_function_block_or_semicolon_or_asi(kind),
            "{kind:?}"
        );
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingFunctionBlockOrSemicolonOrASI
#[test]
fn function_block_or_semicolon_or_asi_rejects_others() {
    for kind in [
        Kind::MethodSignature,
        Kind::CallSignature,
        Kind::ModuleDeclaration,
        Kind::FunctionExpression,
    ] {
        assert!(
            !syntax_requires_trailing_function_block_or_semicolon_or_asi(kind),
            "{kind:?}"
        );
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingModuleBlockOrSemicolonOrASI
#[test]
fn module_block_or_semicolon_or_asi_only_module_declaration() {
    assert!(syntax_requires_trailing_module_block_or_semicolon_or_asi(
        Kind::ModuleDeclaration
    ));
    for kind in [
        Kind::FunctionDeclaration,
        Kind::Block,
        Kind::ModuleBlock,
        Kind::NamespaceExportDeclaration,
    ] {
        assert!(
            !syntax_requires_trailing_module_block_or_semicolon_or_asi(kind),
            "{kind:?}"
        );
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxMayBeASICandidate
#[test]
fn may_be_asi_candidate_is_union_of_the_four() {
    // One representative from each of the four classes.
    for kind in [
        Kind::PropertySignature,   // comma-or-semicolon
        Kind::FunctionDeclaration, // function-block
        Kind::ModuleDeclaration,   // module-block
        Kind::ReturnStatement,     // semicolon
    ] {
        assert!(syntax_may_be_asi_candidate(kind), "{kind:?}");
    }
}

// Go: internal/ls/lsutil/asi.go:SyntaxMayBeASICandidate
#[test]
fn may_be_asi_candidate_rejects_non_candidates() {
    for kind in [
        Kind::SourceFile,
        Kind::Block,
        Kind::Identifier,
        Kind::IfStatement,
    ] {
        assert!(!syntax_may_be_asi_candidate(kind), "{kind:?}");
    }
}
