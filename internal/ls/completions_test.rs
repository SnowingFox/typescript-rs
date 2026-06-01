use super::*;

use tsgo_lsproto::Position;

use crate::test_support::build_service;

/// Collects the completion-item labels (already sorted by the provider).
fn labels(list: &CompletionList) -> Vec<String> {
    list.items.iter().map(|item| item.label.clone()).collect()
}

/// Returns the kind of the item with `label`, if present.
fn kind_of(list: &CompletionList, label: &str) -> Option<CompletionItemKind> {
    list.items
        .iter()
        .find(|item| item.label == label)
        .and_then(|item| item.kind)
}

// Go: internal/ls/completions.go:getTypeScriptMemberSymbols — completing after
// the `.` on `o` (an object literal `{ a, b }`) lists its properties `a` and `b`
// as Field-kind entries.
#[test]
fn provide_completions_member_lists_object_literal_properties() {
    let src = "const o = { a: 1, b: \"x\" }; o.";
    // The trailing `.` is the last byte; the cursor sits right after it.
    assert_eq!(src.len(), 30);
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    let list = ls
        .provide_completions(
            "/m.ts",
            Position {
                line: 0,
                character: 30,
            },
        )
        .expect("a member completion list for `o.`");
    assert_eq!(labels(&list), vec!["a".to_string(), "b".to_string()]);
    // Object-literal members are properties -> `Field`.
    assert_eq!(kind_of(&list, "a"), Some(kinds::FIELD));
    assert_eq!(kind_of(&list, "b"), Some(kinds::FIELD));
    assert!(!list.is_incomplete);
}

// Go: internal/ls/completions.go:getTypeScriptMemberSymbols — a method-bearing
// type's member entry carries the Method kind (the interface method `m` maps to
// `MemberFunctionElement` -> `Method`, while the property `p` is `Field`).
#[test]
fn provide_completions_member_method_has_method_kind() {
    let src = "interface I { m(): void; p: number; }\ndeclare const o: I;\no.";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // `o.` is on line 2; the cursor sits right after the dot at character 2.
    let list = ls
        .provide_completions(
            "/m.ts",
            Position {
                line: 2,
                character: 2,
            },
        )
        .expect("a member completion list for `o.`");
    assert_eq!(labels(&list), vec!["m".to_string(), "p".to_string()]);
    assert_eq!(kind_of(&list, "m"), Some(kinds::METHOD));
    assert_eq!(kind_of(&list, "p"), Some(kinds::FIELD));
}

// A member completion on a receiver with no resolvable symbol/type yields an
// empty (non-`None`) list — never a panic (Go's empty-`symbols`
// `completionInfoFromData`).
// Go: internal/ls/completions.go:getTypeScriptMemberSymbols (nil symbol / error type)
#[test]
fn provide_completions_member_on_unresolved_receiver_is_empty() {
    // `q` is undefined, so it has no symbol/type.
    let mut ls = build_service(&[("/m.ts", "q.")], "/", &["/m.ts"]);
    let list = ls
        .provide_completions(
            "/m.ts",
            Position {
                line: 0,
                character: 2,
            },
        )
        .expect("an (empty) member completion list for `q.`");
    assert!(list.items.is_empty());
}

// A member completion on a primitive type with no lib loaded has no apparent
// properties, so the list is empty (no panic).
// Go: internal/ls/completions.go:addTypeProperties (GetApparentProperties empty)
#[test]
fn provide_completions_member_on_primitive_without_lib_is_empty() {
    // `n: number`; with no lib.d.ts the `number` apparent type has no members.
    let mut ls = build_service(&[("/m.ts", "const n: number = 1; n.")], "/", &["/m.ts"]);
    let list = ls
        .provide_completions(
            "/m.ts",
            Position {
                line: 0,
                character: 23,
            },
        )
        .expect("an (empty) member completion list for `n.`");
    assert!(list.items.is_empty());
}

// Go: internal/ls/completions.go:getGlobalCompletions / getSymbolsInScope —
// inside a function body the scope completions include the outer `const x` and
// `function f` (from globals) plus the parameter `p` (from the function locals).
#[test]
fn provide_completions_scope_lists_locals_and_globals() {
    let src = "const x = 1; function f(p) { p }";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // The `p` use inside the body is at byte/character 29.
    let list = ls
        .provide_completions(
            "/m.ts",
            Position {
                line: 0,
                character: 29,
            },
        )
        .expect("a scope completion list inside `f`'s body");
    let labels = labels(&list);
    assert!(labels.contains(&"x".to_string()), "labels: {labels:?}");
    assert!(labels.contains(&"f".to_string()), "labels: {labels:?}");
    assert!(labels.contains(&"p".to_string()), "labels: {labels:?}");
    // `x` is a (block-scoped) variable, `f` is a function, `p` is a parameter.
    assert_eq!(kind_of(&list, "x"), Some(kinds::VARIABLE));
    assert_eq!(kind_of(&list, "f"), Some(kinds::FUNCTION));
    assert_eq!(kind_of(&list, "p"), Some(kinds::VARIABLE));
}

// Go: internal/ls/completions.go:getGlobalCompletions / getSymbolsInScope — the
// scope walk sees inner + outer symbols (the function's own locals plus the
// globals) but never a *sibling* scope's locals.
#[test]
fn provide_completions_scope_excludes_sibling_scope_locals() {
    let src = "const x = 1;\nfunction f(p) { const y = 2; y }\nfunction g(q) { const z = 3; }";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // The `y` use is on line 1 at character 29.
    let list = ls
        .provide_completions(
            "/m.ts",
            Position {
                line: 1,
                character: 29,
            },
        )
        .expect("a scope completion list inside `f`'s body");
    let labels = labels(&list);
    // Visible: outer `x`/`f`/`g`, the param `p`, and the inner `const y`.
    for visible in ["x", "f", "g", "p", "y"] {
        assert!(
            labels.contains(&visible.to_string()),
            "expected `{visible}` in {labels:?}"
        );
    }
    // Not visible: the sibling scope `g`'s param `q` and its local `z`.
    for hidden in ["q", "z"] {
        assert!(
            !labels.contains(&hidden.to_string()),
            "did not expect `{hidden}` in {labels:?}"
        );
    }
}

// An unknown file yields no completion list (no panic).
// Go: internal/ls/completions.go:getProgramAndFile (missing file)
#[test]
fn provide_completions_unknown_file_is_none() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x")], "/", &["/m.ts"]);
    assert!(ls
        .provide_completions(
            "/missing.ts",
            Position {
                line: 0,
                character: 0
            }
        )
        .is_none());
}

// Unit: the `getCompletionsSymbolKind` mapping (ScriptElementKind ->
// CompletionItemKind) for the reachable kinds.
// Go: internal/ls/completions.go:getCompletionsSymbolKind
#[test]
fn completion_item_kind_maps_each_script_element_kind() {
    use tsgo_ls_lsutil::ScriptElementKind as K;
    assert_eq!(completion_item_kind(K::Keyword), kinds::KEYWORD);
    assert_eq!(completion_item_kind(K::PrimitiveType), kinds::KEYWORD);
    assert_eq!(completion_item_kind(K::VariableElement), kinds::VARIABLE);
    assert_eq!(completion_item_kind(K::ConstElement), kinds::VARIABLE);
    assert_eq!(completion_item_kind(K::LetElement), kinds::VARIABLE);
    assert_eq!(completion_item_kind(K::ParameterElement), kinds::VARIABLE);
    assert_eq!(completion_item_kind(K::Alias), kinds::VARIABLE);
    assert_eq!(completion_item_kind(K::MemberVariableElement), kinds::FIELD);
    assert_eq!(
        completion_item_kind(K::MemberGetAccessorElement),
        kinds::FIELD
    );
    assert_eq!(
        completion_item_kind(K::MemberSetAccessorElement),
        kinds::FIELD
    );
    assert_eq!(completion_item_kind(K::FunctionElement), kinds::FUNCTION);
    assert_eq!(
        completion_item_kind(K::LocalFunctionElement),
        kinds::FUNCTION
    );
    assert_eq!(
        completion_item_kind(K::MemberFunctionElement),
        kinds::METHOD
    );
    assert_eq!(completion_item_kind(K::CallSignatureElement), kinds::METHOD);
    assert_eq!(
        completion_item_kind(K::ConstructSignatureElement),
        kinds::METHOD
    );
    assert_eq!(
        completion_item_kind(K::IndexSignatureElement),
        kinds::METHOD
    );
    assert_eq!(completion_item_kind(K::EnumElement), kinds::ENUM);
    assert_eq!(
        completion_item_kind(K::EnumMemberElement),
        kinds::ENUM_MEMBER
    );
    assert_eq!(completion_item_kind(K::ModuleElement), kinds::MODULE);
    assert_eq!(completion_item_kind(K::ClassElement), kinds::CLASS);
    assert_eq!(completion_item_kind(K::TypeElement), kinds::CLASS);
    assert_eq!(completion_item_kind(K::InterfaceElement), kinds::INTERFACE);
    assert_eq!(completion_item_kind(K::Warning), kinds::TEXT);
    assert_eq!(completion_item_kind(K::ScriptElement), kinds::FILE);
    assert_eq!(completion_item_kind(K::Directory), kinds::FOLDER);
    assert_eq!(completion_item_kind(K::String), kinds::CONSTANT);
    // Unmapped kinds (e.g. a type parameter) fall through to `Property`.
    assert_eq!(
        completion_item_kind(K::TypeParameterElement),
        kinds::PROPERTY
    );
    assert_eq!(completion_item_kind(K::Unknown), kinds::PROPERTY);
}
