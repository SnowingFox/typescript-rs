use super::*;
use crate::children::SourceFile;
use tsgo_ast::{Kind, NodeArena, NodeId};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

/// Parses `text` as TypeScript into an owned navigation [`SourceFile`].
fn make(text: &str) -> SourceFile {
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    SourceFile::new(r.arena, r.source_file, text.to_string())
}

fn statement(file: &SourceFile, index: usize) -> NodeId {
    match file.arena().data(file.root()) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[index],
        _ => unreachable!(),
    }
}

fn find_first_of_kind(arena: &NodeArena, root: NodeId, kind: Kind) -> Option<NodeId> {
    if arena.kind(root) == kind {
        return Some(root);
    }
    let mut found = None;
    arena.for_each_child(root, &mut |c| {
        if let Some(f) = find_first_of_kind(arena, c, kind) {
            found = Some(f);
            true
        } else {
            false
        }
    });
    found
}

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

// ---- Node-level ASI candidacy (the shared-arena navigation slice) ----

// Go: internal/ls/lsutil/asi.go:NodeIsASICandidate
// An unterminated statement whose next token is on a later line IS an ASI
// candidate.
#[test]
fn node_is_asi_candidate_true_for_unterminated_before_newline() {
    let mut file = make("let a = 1\nlet b = 2;");
    let stmt0 = statement(&file, 0);
    assert!(node_is_asi_candidate(&mut file, stmt0));
}

// Go: internal/ls/lsutil/asi.go:NodeIsASICandidate
// A statement that already ends with `;` is not an ASI candidate.
#[test]
fn node_is_asi_candidate_false_when_semicolon_present() {
    let mut file = make("let a = 1;\nlet b = 2;");
    let stmt0 = statement(&file, 0);
    assert!(!node_is_asi_candidate(&mut file, stmt0));
}

// Go: internal/ls/lsutil/asi.go:NodeIsASICandidate
// A function declaration terminated by its block body is not an ASI candidate.
#[test]
fn node_is_asi_candidate_false_for_function_with_block() {
    let mut file = make("function f() {}");
    let func = statement(&file, 0);
    assert!(!node_is_asi_candidate(&mut file, func));
}

// Go: internal/ls/lsutil/asi.go:NodeIsASICandidate
// A kind that requires no trailing terminator is never an ASI candidate.
#[test]
fn node_is_asi_candidate_false_for_non_candidate_kind() {
    let mut file = make("function f() {}");
    let block = find_first_of_kind(file.arena(), file.root(), Kind::Block).expect("a block");
    assert!(!node_is_asi_candidate(&mut file, block));
}

// Go: internal/ls/lsutil/asi.go:PositionIsASICandidate
// The end position of an unterminated statement (before a newline) is an ASI
// candidate position.
#[test]
fn position_is_asi_candidate_at_statement_end() {
    let mut file = make("let a = 1\nlet b = 2;");
    let stmt0 = statement(&file, 0);
    let end = file.arena().loc(stmt0).end();
    assert!(position_is_asi_candidate(&mut file, end, stmt0));
}

// Go: internal/ls/lsutil/asi.go:PositionIsASICandidate
// A position that is not the end of any may-be-ASI ancestor quits the walk.
#[test]
fn position_is_asi_candidate_false_mid_statement() {
    let mut file = make("let a = 1\nlet b = 2;");
    let stmt0 = statement(&file, 0);
    let end = file.arena().loc(stmt0).end();
    assert!(!position_is_asi_candidate(&mut file, end - 1, stmt0));
}
