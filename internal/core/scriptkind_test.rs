use super::*;

// Go: internal/core/scriptkind_stringer_generated.go:String
#[test]
fn scriptkind_display() {
    assert_eq!(ScriptKind::Unknown.to_string(), "ScriptKindUnknown");
    assert_eq!(ScriptKind::Js.to_string(), "ScriptKindJS");
    assert_eq!(ScriptKind::Jsx.to_string(), "ScriptKindJSX");
    assert_eq!(ScriptKind::Ts.to_string(), "ScriptKindTS");
    assert_eq!(ScriptKind::Tsx.to_string(), "ScriptKindTSX");
    assert_eq!(ScriptKind::External.to_string(), "ScriptKindExternal");
    assert_eq!(ScriptKind::Json.to_string(), "ScriptKindJSON");
    assert_eq!(ScriptKind::Deferred.to_string(), "ScriptKindDeferred");
}
