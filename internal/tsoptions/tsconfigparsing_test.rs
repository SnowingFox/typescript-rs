use super::*;
use crate::tsoptionstest::VfsParseConfigHost;

// Helper mirroring Go `getParsedWithJsonApi`: parse text -> json, then content.
fn parse_json_api(
    json_text: &str,
    files: &[(&str, &str)],
    base_path: &str,
    config_file_name: &str,
) -> ParsedCommandLine {
    let host = VfsParseConfigHost::new(files, base_path, true);
    let (json, _) = parse_config_file_text_to_json(config_file_name, json_text);
    parse_json_config_file_content(json, &host, base_path, None, config_file_name)
}

fn error_codes(parsed: &ParsedCommandLine) -> Vec<i32> {
    parsed.errors.iter().map(|e| e.message.code()).collect()
}

// Go: internal/tsoptions/tsconfigparsing_test.go:TestParseConfigFileTextToJson
// (the Go cases write golden baselines that need the TS submodule; these are
// assertion-level ports of the same inputs, checking the converted value tree).

// Tracer: a nested object with a boolean leaf round-trips into a Map.
#[test]
fn text_to_json_nested_object_bool() {
    let (value, errors) = parse_config_file_text_to_json(
        "/apath/tsconfig.json",
        r#"{ "compilerOptions": { "strict": true } }"#,
    );
    assert!(errors.is_empty());
    let map = value.as_map().expect("root is a map");
    let co = map
        .get(&"compilerOptions".to_string())
        .and_then(|v| v.as_map())
        .expect("compilerOptions is a map");
    assert_eq!(
        co.get(&"strict".to_string()),
        Some(&OptionValue::Bool(true))
    );
}

// ---- Phase B: ParseJsonConfigFileContent (json api) -----------------------

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"ignore dotted files and folders"
#[test]
fn cfg_ignore_dotted_files_and_folders() {
    let parsed = parse_json_api(
        "{}",
        &[
            ("/apath/test.ts", ""),
            ("/apath/.git/a.ts", ""),
            ("/apath/.b.ts", ""),
            ("/apath/..c.ts", ""),
        ],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(
        parsed.errors.is_empty(),
        "errors: {:?}",
        error_codes(&parsed)
    );
    assert_eq!(parsed.file_names(), &["/apath/test.ts".to_string()]);
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"implicitly exclude common package folders"
#[test]
fn cfg_implicitly_exclude_package_folders() {
    let parsed = parse_json_api(
        "{}",
        &[
            ("/node_modules/a.ts", ""),
            ("/bower_components/b.ts", ""),
            ("/jspm_packages/c.ts", ""),
            ("/d.ts", ""),
            ("/folder/e.ts", ""),
        ],
        "/",
        "/tsconfig.json",
    );
    assert!(
        parsed.errors.is_empty(),
        "errors: {:?}",
        error_codes(&parsed)
    );
    assert_eq!(
        parsed.file_names(),
        &["/d.ts".to_string(), "/folder/e.ts".to_string()]
    );
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"generates errors for empty files list"
#[test]
fn cfg_empty_files_list_errors() {
    let parsed = parse_json_api(
        r#"{ "files": [] }"#,
        &[("/apath/a.ts", "")],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(error_codes(&parsed)
        .contains(&tsgo_diagnostics::THE_FILES_LIST_IN_CONFIG_FILE_0_IS_EMPTY.code()));
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"generates errors for empty files list when no references are provided"
#[test]
fn cfg_empty_files_list_no_refs_errors() {
    let parsed = parse_json_api(
        r#"{ "files": [], "references": [] }"#,
        &[("/apath/a.ts", "")],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(error_codes(&parsed)
        .contains(&tsgo_diagnostics::THE_FILES_LIST_IN_CONFIG_FILE_0_IS_EMPTY.code()));
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"does not generate errors for empty files list when one or more references are provided"
#[test]
fn cfg_empty_files_with_refs_ok() {
    let parsed = parse_json_api(
        r#"{ "files": [], "references": [{ "path": "/apath" }] }"#,
        &[("/apath/a.ts", "")],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(!error_codes(&parsed)
        .contains(&tsgo_diagnostics::THE_FILES_LIST_IN_CONFIG_FILE_0_IS_EMPTY.code()));
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"generates errors for directory with no .ts files"
#[test]
fn cfg_directory_with_no_ts_files_errors() {
    let parsed = parse_json_api(
        "{}",
        &[("/apath/a.js", "")],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(error_codes(&parsed).contains(
        &tsgo_diagnostics::NO_INPUTS_WERE_FOUND_IN_CONFIG_FILE_0_SPECIFIED_INCLUDE_PATHS_WERE_1_AND_EXCLUDE_PATHS_WERE_2
            .code()
    ));
    assert!(parsed.file_names().is_empty());
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"generates errors for empty include"
#[test]
fn cfg_empty_include_errors() {
    let parsed = parse_json_api(
        r#"{ "include": [] }"#,
        &[("/apath/a.ts", "")],
        "tests/cases/unittests",
        "/apath/tsconfig.json",
    );
    assert!(error_codes(&parsed).contains(
        &tsgo_diagnostics::NO_INPUTS_WERE_FOUND_IN_CONFIG_FILE_0_SPECIFIED_INCLUDE_PATHS_WERE_1_AND_EXCLUDE_PATHS_WERE_2
            .code()
    ));
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"reports error for an unknown option"
#[test]
fn cfg_unknown_compiler_option_errors() {
    let parsed = parse_json_api(
        r#"{ "compilerOptions": { "unknown": true } }"#,
        &[("/apath/a.ts", "")],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(error_codes(&parsed).contains(&tsgo_diagnostics::UNKNOWN_COMPILER_OPTION_0.code()));
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"returns error when tsconfig have excludes"
#[test]
fn cfg_excludes_typo_errors() {
    let parsed = parse_json_api(
        r#"{ "excludes": ["foge.ts"] }"#,
        &[("/apath/a.ts", "")],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(error_codes(&parsed)
        .contains(&tsgo_diagnostics::UNKNOWN_OPTION_EXCLUDES_DID_YOU_MEAN_EXCLUDE.code()));
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"generates errors when commandline option is in tsconfig"
#[test]
fn cfg_commandline_only_option_in_tsconfig_errors() {
    let parsed = parse_json_api(
        r#"{ "compilerOptions": { "help": true } }"#,
        &[("/apath/a.ts", "")],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(error_codes(&parsed)
        .contains(&tsgo_diagnostics::OPTION_0_CAN_ONLY_BE_SPECIFIED_ON_COMMAND_LINE.code()));
}

// Go: tsconfigparsing_test.go:parseJsonConfigFileTests/"parses tsconfig with compilerOptions, files, include, and exclude"
#[test]
fn cfg_full_options() {
    use tsgo_core::compileroptions::{JsxEmit, ModuleKind, ModuleResolutionKind, ScriptTarget};
    use tsgo_core::tristate::Tristate;
    let parsed = parse_json_api(
        r#"{
  "compilerOptions": {
    "outDir": "./dist",
    "strict": true,
    "noImplicitAny": true,
    "target": "ES2017",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react",
    "maxNodeModuleJsDepth": 1,
    "paths": { "jquery": ["./vendor/jquery/dist/jquery"] }
  },
  "files": ["/apath/src/index.ts", "/apath/src/app.ts"],
  "include": ["/apath/src/**/*"],
  "exclude": ["/apath/node_modules", "/apath/dist"]
}"#,
        &[
            ("/apath/src/index.ts", ""),
            ("/apath/src/app.ts", ""),
            ("/apath/node_modules/module.ts", ""),
            ("/apath/dist/output.js", ""),
        ],
        "/apath",
        "/apath/tsconfig.json",
    );
    assert!(
        parsed.errors.is_empty(),
        "errors: {:?}",
        error_codes(&parsed)
    );
    let co = parsed.compiler_options();
    assert_eq!(co.out_dir, "/apath/dist");
    assert_eq!(co.strict, Tristate::True);
    assert_eq!(co.no_implicit_any, Tristate::True);
    assert_eq!(co.target, ScriptTarget::Es2017);
    assert_eq!(co.module, ModuleKind::EsNext);
    assert_eq!(co.module_resolution, ModuleResolutionKind::Bundler);
    assert_eq!(co.jsx, JsxEmit::React);
    assert_eq!(co.max_node_module_js_depth, Some(1));
    // The two literal files lead, then the wildcard-expanded ones (deduped).
    assert!(parsed
        .file_names()
        .contains(&"/apath/src/index.ts".to_string()));
    assert!(parsed
        .file_names()
        .contains(&"/apath/src/app.ts".to_string()));
    assert_eq!(parsed.literal_file_names_len, 2);
}

// Go: tsconfigparsing_test.go:parseConfigFileTextToJsonTests/"returns config object without comments"
#[test]
fn text_to_json_strips_comments_and_reads_string_array() {
    let (value, errors) = parse_config_file_text_to_json(
        "/apath/tsconfig.json",
        "{ // Excluded files\n            \"exclude\": [\n                // Exclude d.ts\n                \"file.d.ts\"\n            ]\n        }",
    );
    assert!(errors.is_empty());
    let map = value.as_map().unwrap();
    let exclude = map
        .get(&"exclude".to_string())
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(exclude, &[OptionValue::String("file.d.ts".to_string())]);
}

// Go: tsconfigparsing_test.go:parseConfigFileTextToJsonTests/"keeps string content untouched"
#[test]
fn text_to_json_keeps_string_content_untouched() {
    let (value, errors) = parse_config_file_text_to_json(
        "/apath/tsconfig.json",
        "{\n            \"exclude\": [\n                \"xx//file.d.ts\"\n            ]\n        }",
    );
    assert!(errors.is_empty());
    let map = value.as_map().unwrap();
    let exclude = map
        .get(&"exclude".to_string())
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(exclude, &[OptionValue::String("xx//file.d.ts".to_string())]);
}

// Go: tsconfigparsing_test.go:parseConfigFileTextToJsonTests/"returns object when users correctly specify library"
#[test]
fn text_to_json_lib_array() {
    let (value, errors) = parse_config_file_text_to_json(
        "/apath/tsconfig.json",
        "{\n            \"compilerOptions\": {\n                \"lib\": [\"es5\", \"es6\"]\n            }\n        }",
    );
    assert!(errors.is_empty());
    let map = value.as_map().unwrap();
    let co = map
        .get(&"compilerOptions".to_string())
        .and_then(|v| v.as_map())
        .unwrap();
    let lib = co
        .get(&"lib".to_string())
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(
        lib,
        &[
            OptionValue::String("es5".to_string()),
            OptionValue::String("es6".to_string())
        ]
    );
}

// Go: tsconfigparsing_test.go:parseConfigFileTextToJsonTests/"returns empty config when config is empty object"
#[test]
fn text_to_json_empty_object() {
    let (value, errors) = parse_config_file_text_to_json("/apath/tsconfig.json", "{}");
    assert!(errors.is_empty());
    assert_eq!(value.as_map().unwrap().size(), 0);
}

// Go: tsconfigparsing_test.go:parseConfigFileTextToJsonTests/"returns empty config for file with only whitespaces"
#[test]
fn text_to_json_only_whitespace_is_empty() {
    for input in ["", " "] {
        let (value, errors) = parse_config_file_text_to_json("/apath/tsconfig.json", input);
        assert!(errors.is_empty(), "input {input:?} produced errors");
        assert_eq!(value.as_map().unwrap().size(), 0);
    }
}

// Go: convertPropertyValueToJson KindNumericLiteral (number -> float64).
#[test]
fn text_to_json_number_value() {
    let (value, errors) =
        parse_config_file_text_to_json("/apath/tsconfig.json", r#"{ "maxNodeModuleJsDepth": 1 }"#);
    assert!(errors.is_empty());
    let map = value.as_map().unwrap();
    assert_eq!(
        map.get(&"maxNodeModuleJsDepth".to_string()),
        Some(&OptionValue::Number(1.0))
    );
}

// Go: convertPropertyValueToJson KindPrefixUnaryExpression (negative number).
#[test]
fn text_to_json_negative_number_value() {
    let (value, errors) = parse_config_file_text_to_json("/apath/tsconfig.json", r#"{ "n": -2 }"#);
    assert!(errors.is_empty());
    let map = value.as_map().unwrap();
    assert_eq!(map.get(&"n".to_string()), Some(&OptionValue::Number(-2.0)));
}

// Go: convertPropertyValueToJson KindFalseKeyword / KindNullKeyword.
#[test]
fn text_to_json_false_and_null_values() {
    let (value, errors) =
        parse_config_file_text_to_json("/apath/tsconfig.json", r#"{ "a": false, "b": null }"#);
    assert!(errors.is_empty());
    let map = value.as_map().unwrap();
    assert_eq!(map.get(&"a".to_string()), Some(&OptionValue::Bool(false)));
    assert_eq!(map.get(&"b".to_string()), Some(&OptionValue::Null));
}

// Go: convertConfigFileToObject (root value of a tsconfig file must be an object).
#[test]
fn text_to_json_non_object_root_errors() {
    let (value, errors) =
        parse_config_file_text_to_json("/apath/tsconfig.json", r#""just a string""#);
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].message.code(),
        tsgo_diagnostics::THE_ROOT_VALUE_OF_A_0_FILE_MUST_BE_AN_OBJECT.code()
    );
    assert_eq!(value.as_map().unwrap().size(), 0);
}

// Go: tsconfigparsing_test.go:parseConfigFileTextToJsonTests/"returns empty config for file with comments only"
#[test]
fn text_to_json_comments_only_is_empty() {
    for input in ["// Comment", "/* Comment*/"] {
        let (value, errors) = parse_config_file_text_to_json("/apath/tsconfig.json", input);
        assert!(errors.is_empty(), "input {input:?} produced errors");
        assert_eq!(value.as_map().unwrap().size(), 0);
    }
}
