use super::*;

// Go: internal/tspath/extension.go:ExtensionIsTs (behavior-level supplement)
#[test]
fn extension_is_ts_cases() {
    assert!(extension_is_ts(".ts"));
    assert!(extension_is_ts(".tsx"));
    assert!(extension_is_ts(".d.ts"));
    assert!(extension_is_ts(".mts"));
    assert!(extension_is_ts(".d.json.ts"));
    assert!(!extension_is_ts(".js"));
    assert!(!extension_is_ts(".json"));
}

// Go: internal/tspath/extension.go:RemoveFileExtension (behavior-level supplement)
#[test]
fn remove_file_extension_cases() {
    assert_eq!(remove_file_extension("foo.ts"), "foo");
    assert_eq!(remove_file_extension("foo.d.ts"), "foo");
    assert_eq!(remove_file_extension("foo.bar"), "foo.bar");
}

// Go: internal/tspath/extension.go:TryGetExtensionFromPath (behavior-level supplement)
#[test]
fn try_get_extension_from_path_cases() {
    assert_eq!(try_get_extension_from_path("foo.ts"), ".ts");
    assert_eq!(try_get_extension_from_path("foo.d.ts"), ".d.ts");
    assert_eq!(try_get_extension_from_path("foo.bar"), "");
}

// Go: internal/tspath/extension.go:RemoveExtension (behavior-level supplement)
#[test]
fn remove_extension_cases() {
    assert_eq!(remove_extension("foo.ts", ".ts"), "foo");
}

// Go: internal/tspath/extension.go:FileExtensionIsOneOf (behavior-level supplement)
#[test]
fn file_extension_is_one_of_cases() {
    assert!(file_extension_is_one_of("foo.ts", &[".js", ".ts"]));
    assert!(!file_extension_is_one_of("foo.ts", &[".js", ".json"]));
}

// Go: internal/tspath/extension.go:TryExtractTSExtension (behavior-level supplement)
#[test]
fn try_extract_ts_extension_cases() {
    assert_eq!(try_extract_ts_extension("foo.ts"), ".ts");
    assert_eq!(try_extract_ts_extension("foo.js"), "");
}

// Go: internal/tspath/extension.go:HasTSFileExtension/HasJSFileExtension/HasJSONFileExtension (behavior-level supplement)
#[test]
fn has_file_extension_cases() {
    assert!(has_ts_file_extension("a.ts"));
    assert!(!has_ts_file_extension("a.js"));
    assert!(has_js_file_extension("a.js"));
    assert!(!has_js_file_extension("a.ts"));
    assert!(has_json_file_extension("a.json"));
    assert!(!has_json_file_extension("a.ts"));
}

// Go: internal/tspath/extension.go:HasImplementationTSFileExtension (behavior-level supplement)
#[test]
fn has_implementation_ts_file_extension_cases() {
    assert!(has_implementation_ts_file_extension("a.ts"));
    assert!(has_implementation_ts_file_extension("a.tsx"));
    assert!(!has_implementation_ts_file_extension("a.d.ts"));
    assert!(!has_implementation_ts_file_extension("a.js"));
}

// Go: internal/tspath/extension.go:IsDeclarationFileName/ExtensionIsOneOf (behavior-level supplement)
#[test]
fn declaration_and_one_of_cases() {
    assert!(is_declaration_file_name("a.d.ts"));
    assert!(!is_declaration_file_name("a.ts"));
    assert!(extension_is_one_of(".ts", &[".ts", ".js"]));
    assert!(!extension_is_one_of(".json", &[".ts", ".js"]));
}

// Go: internal/tspath/extension.go:GetDeclarationFileExtension (behavior-level supplement)
#[test]
fn get_declaration_file_extension_cases() {
    assert_eq!(get_declaration_file_extension("foo.d.ts"), ".d.ts");
    assert_eq!(
        get_declaration_file_extension("foo.d.json.ts"),
        ".d.json.ts"
    );
    assert_eq!(get_declaration_file_extension("foo.ts"), "");
}

// Go: internal/tspath/extension.go:GetDeclarationEmitExtensionForPath (behavior-level supplement)
#[test]
fn get_declaration_emit_extension_for_path_cases() {
    assert_eq!(get_declaration_emit_extension_for_path("a.mts"), ".d.mts");
    assert_eq!(get_declaration_emit_extension_for_path("a.cts"), ".d.cts");
    assert_eq!(get_declaration_emit_extension_for_path("a.ts"), ".d.ts");
    assert_eq!(
        get_declaration_emit_extension_for_path("a.json"),
        ".d.json.ts"
    );
}

// Go: internal/tspath/extension.go:ChangeAnyExtension/ChangeExtension/ChangeFullExtension (behavior-level supplement)
#[test]
fn change_extension_cases() {
    assert_eq!(
        change_any_extension("/path/to/file.ext", ".js", &[".ext"], false),
        "/path/to/file.js"
    );
    assert_eq!(
        change_any_extension("/path/to/file.ext", ".js", &[".ts"], false),
        "/path/to/file.ext"
    );
    assert_eq!(change_extension("foo.ts", ".js"), "foo.js");
    assert_eq!(change_full_extension("file.d.ts", ".js"), "file.js");
    assert_eq!(change_full_extension("file.ts", ".js"), "file.js");
}

// Go: internal/tspath/extension.go:GetPossibleOriginalInputExtensionForExtension (behavior-level supplement)
#[test]
fn get_possible_original_input_extension_for_extension_cases() {
    assert_eq!(
        get_possible_original_input_extension_for_extension("a.d.mts"),
        vec![".mts", ".mjs"]
    );
    assert_eq!(
        get_possible_original_input_extension_for_extension("a.d.json.ts"),
        vec![".json"]
    );
    assert_eq!(
        get_possible_original_input_extension_for_extension("a.ts"),
        vec![".tsx", ".ts", ".jsx", ".js"]
    );
}
