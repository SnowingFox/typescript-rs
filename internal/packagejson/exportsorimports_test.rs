use super::*;

// Go: internal/packagejson/exportsorimports_test.go:testExports
//
// Parses one document with an `imports` block keyed by `#foo` and an `exports`
// block keyed by `.` / `./...`, then asserts the `objectKind` classification.
const DOC: &str = r##"{
    "imports": {
        "#foo": {
            "import": "./foo.ts"
        }
    },
    "exports": {
        ".": {
            "import": "./test.ts",
            "default": "./test.ts"
        },
        "./test": [
            "./test1.ts",
            "./test2.ts",
            null
        ],
        "./null": null
    }
}"##;

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Exports {
    imports: ExportsOrImports,
    exports: ExportsOrImports,
}

fn parsed() -> Exports {
    tsgo_json::unmarshal(DOC.as_bytes()).expect("valid json")
}

fn child<'a>(value: &'a ExportsOrImports, key: &str) -> &'a ExportsOrImports {
    value
        .as_object()
        .get(&key.to_string())
        .unwrap_or_else(|| panic!("missing key {key}"))
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/exports-subpaths
#[test]
fn eoi_exports_is_subpaths() {
    let e = parsed();
    assert!(e.exports.is_subpaths());
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/exports-size
#[test]
fn eoi_exports_size() {
    let e = parsed();
    assert_eq!(e.exports.as_object().size(), 3);
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/dot-conditions
#[test]
fn eoi_dot_is_conditions() {
    let e = parsed();
    assert!(child(&e.exports, ".").is_conditions());
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/dot-import-string
#[test]
fn eoi_condition_value_string() {
    let e = parsed();
    let dot = child(&e.exports, ".");
    assert_eq!(child(dot, "import").value_type(), JsonValueType::String);
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/test-array-null-tail
#[test]
fn eoi_array_null_tail() {
    let e = parsed();
    let arr = child(&e.exports, "./test").as_array();
    assert_eq!(arr[2].value_type(), JsonValueType::Null);
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/null-subpath
#[test]
fn eoi_null_subpath() {
    let e = parsed();
    assert_eq!(
        child(&e.exports, "./null").value_type(),
        JsonValueType::Null
    );
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/imports-is-imports
#[test]
fn eoi_imports_is_imports() {
    let e = parsed();
    assert!(e.imports.is_imports());
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/imports-size
#[test]
fn eoi_imports_size() {
    let e = parsed();
    assert_eq!(e.imports.as_object().size(), 1);
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/import-foo-conditions
#[test]
fn eoi_import_foo_is_conditions() {
    let e = parsed();
    assert!(child(&e.imports, "#foo").is_conditions());
}

// Go: internal/packagejson/exportsorimports_test.go:testExports/import-foo-import-string
#[test]
fn eoi_import_foo_value_string() {
    let e = parsed();
    let foo = child(&e.imports, "#foo");
    assert_eq!(child(foo, "import").value_type(), JsonValueType::String);
}
