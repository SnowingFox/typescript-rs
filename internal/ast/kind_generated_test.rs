use super::*;

// Go: internal/ast/kind_generated.go:Kind (ordinals match Go iota)
#[test]
fn kind_ordinals_match_go() {
    assert_eq!(Kind::Unknown as i16, 0);
    assert_eq!(Kind::EndOfFile as i16, 1);
    assert_eq!(Kind::NumericLiteral as i16, 8);
    assert_eq!(Kind::Identifier as i16, 79);
    assert_eq!(Kind::QualifiedName as i16, 167);
    assert_eq!(Kind::TypePredicate as i16, 183);
    assert_eq!(Kind::ImportType as i16, 206);
    assert_eq!(Kind::VariableStatement as i16, 244);
    assert_eq!(Kind::DebuggerStatement as i16, 260);
    assert_eq!(Kind::SourceFile as i16, 307);
    assert_eq!(Kind::JSDocImportTag as i16, 343);
    assert_eq!(Kind::NotEmittedTypeElement as i16, 350);
    assert_eq!(Kind::Count as i16, 351);
}

// Go: internal/ast/kind_generated.go (range constants)
#[test]
fn kind_range_constants() {
    assert_eq!(Kind::FIRST_STATEMENT, Kind::VariableStatement);
    assert_eq!(Kind::LAST_STATEMENT, Kind::DebuggerStatement);
    assert_eq!(Kind::FIRST_TYPE_NODE, Kind::TypePredicate);
    assert_eq!(Kind::LAST_TYPE_NODE, Kind::ImportType);
    assert_eq!(Kind::FIRST_KEYWORD, Kind::BreakKeyword);
    assert_eq!(Kind::LAST_KEYWORD, Kind::DeferKeyword);
    assert_eq!(Kind::FIRST_ASSIGNMENT, Kind::EqualsToken);
    assert_eq!(Kind::LAST_ASSIGNMENT, Kind::CaretEqualsToken);
    assert_eq!(Kind::FIRST_NODE, Kind::QualifiedName);
}

// Go: internal/ast/kind_stringer_generated.go:Kind.String
#[test]
fn kind_display_matches_stringer() {
    assert_eq!(Kind::Unknown.to_string(), "KindUnknown");
    assert_eq!(Kind::SourceFile.to_string(), "KindSourceFile");
    assert_eq!(Kind::CallExpression.to_string(), "KindCallExpression");
}
