use super::*;

use tsgo_core::tristate::Tristate;

// Go: internal/tsoptions/parsinghelpers.go:ParseTristate (behavior-level)
#[test]
fn parse_tristate_cases() {
    assert_eq!(parse_tristate(&OptionValue::Null), Tristate::Unknown);
    assert_eq!(parse_tristate(&OptionValue::Bool(true)), Tristate::True);
    assert_eq!(parse_tristate(&OptionValue::Bool(false)), Tristate::False);
    // Any non-true value parses to false (mirrors Go's `value == true` check).
    assert_eq!(
        parse_tristate(&OptionValue::String("yes".into())),
        Tristate::False
    );
}

// Go: internal/tsoptions/parsinghelpers.go:ParseString
#[test]
fn parse_string_cases() {
    assert_eq!(parse_string(&OptionValue::String("hello".into())), "hello");
    assert_eq!(parse_string(&OptionValue::Null), "");
    assert_eq!(parse_string(&OptionValue::Bool(true)), "");
}

// Go: internal/tsoptions/parsinghelpers.go:ParseStringArray
#[test]
fn parse_string_array_filters_strings() {
    let v = OptionValue::Array(vec![
        OptionValue::String("a".into()),
        OptionValue::Number(3.0),
        OptionValue::String("b".into()),
    ]);
    assert_eq!(
        parse_string_array(&v),
        Some(vec!["a".to_string(), "b".to_string()])
    );
}

#[test]
fn parse_string_array_non_array_is_none() {
    assert_eq!(parse_string_array(&OptionValue::Null), None);
    assert_eq!(parse_string_array(&OptionValue::String("x".into())), None);
}

#[test]
fn parse_string_array_empty_array_is_some_empty() {
    assert_eq!(
        parse_string_array(&OptionValue::Array(vec![])),
        Some(vec![])
    );
}

// Go: internal/tsoptions/parsinghelpers.go:parseNumber
#[test]
fn parse_number_cases() {
    assert_eq!(parse_number(&OptionValue::Number(2.0)), Some(2));
    // Truncation toward zero (Go: int(num)).
    assert_eq!(parse_number(&OptionValue::Number(2.9)), Some(2));
    assert_eq!(parse_number(&OptionValue::Int(7)), Some(7));
    assert_eq!(parse_number(&OptionValue::Null), None);
    assert_eq!(parse_number(&OptionValue::String("3".into())), None);
}

use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};

// Go: internal/tsoptions/parsinghelpers_test.go:TestParseCompilerOptionNoMissingFields
// Every exported CompilerOptions field's JSON name must be handled by the
// `parse_compiler_options` switch.
#[test]
fn parse_compiler_option_no_missing_fields() {
    let mut missing: Vec<&str> = Vec::new();
    for (_go, json) in crate::compiler_option_field_names() {
        let mut co = CompilerOptions::default();
        if !parse_compiler_options(json, &OptionValue::Null, &mut co) {
            missing.push(json);
        }
    }
    assert!(
        missing.is_empty(),
        "keys missing from parse_compiler_options switch: {missing:?}"
    );
}

// Go: internal/tsoptions/parsinghelpers.go:parseCompilerOptions (behavior)
#[test]
fn parse_compiler_options_sets_fields() {
    let mut co = CompilerOptions::default();
    assert!(parse_compiler_options(
        "strict",
        &OptionValue::Bool(true),
        &mut co
    ));
    assert_eq!(co.strict, Tristate::True);

    assert!(parse_compiler_options(
        "outDir",
        &OptionValue::String("bin".into()),
        &mut co
    ));
    assert_eq!(co.out_dir, "bin");

    // Enum value flows in as its i32 discriminant.
    assert!(parse_compiler_options(
        "target",
        &OptionValue::Int(ScriptTarget::Es2017 as i32),
        &mut co
    ));
    assert_eq!(co.target, ScriptTarget::Es2017);

    assert!(!parse_compiler_options(
        "notAnOption",
        &OptionValue::Bool(true),
        &mut co
    ));
}

// Go: internal/tsoptions/parsinghelpers.go:ParseTypeAcquisition (behavior)
#[test]
fn parse_type_acquisition_sets_fields() {
    use tsgo_core::typeacquisition::TypeAcquisition;
    let mut ta = TypeAcquisition::default();
    parse_type_acquisition("enable", &OptionValue::Bool(true), &mut ta);
    parse_type_acquisition(
        "include",
        &OptionValue::Array(vec![OptionValue::String("a.d.ts".into())]),
        &mut ta,
    );
    assert_eq!(ta.enable, Tristate::True);
    assert_eq!(ta.include, vec!["a.d.ts".to_string()]);
}

// Go: internal/tsoptions/parsinghelpers.go:ParseBuildOptions (behavior)
#[test]
fn parse_build_options_sets_fields() {
    use tsgo_core::buildoptions::BuildOptions;
    let mut bo = BuildOptions::default();
    parse_build_options("verbose", &OptionValue::Bool(true), &mut bo);
    parse_build_options("builders", &OptionValue::Number(2.0), &mut bo);
    assert_eq!(bo.verbose, Tristate::True);
    assert_eq!(bo.builders, Some(2));
}

// Go: internal/tsoptions/parsinghelpers.go:ParseWatchOptions (behavior)
#[test]
fn parse_watch_options_sets_fields() {
    use tsgo_core::watchoptions::{WatchFileKind, WatchOptions};
    let mut wo = WatchOptions::default();
    parse_watch_options(
        "watchFile",
        &OptionValue::Int(WatchFileKind::UseFsEvents as i32),
        &mut wo,
    );
    assert_eq!(wo.file_kind, WatchFileKind::UseFsEvents);
}

// Go: internal/tsoptions/parsinghelpers.go:mergeCompilerOptions (behavior)
#[test]
fn merge_compiler_options_copies_non_zero_and_zeroes_explicit_null() {
    use tsgo_collections::Set;
    let mut target = CompilerOptions {
        out_dir: "old".into(),
        strict: Tristate::True,
        ..Default::default()
    };
    let source = CompilerOptions {
        out_dir: "new".into(),
        ..Default::default()
    };
    // Without explicit nulls: non-zero source fields overwrite; zero fields keep target.
    crate::merge_compiler_options(&mut target, &source, &Set::default());
    assert_eq!(target.out_dir, "new");
    assert_eq!(target.strict, Tristate::True);

    // Explicit null zeroes the target field even though source is zero.
    let mut explicit_null = Set::default();
    explicit_null.add("strict".to_string());
    crate::merge_compiler_options(&mut target, &CompilerOptions::default(), &explicit_null);
    assert_eq!(target.strict, Tristate::Unknown);
}

// Go: internal/tsoptions/parsinghelpers.go:ConvertOptionToAbsolutePath (behavior)
#[test]
fn convert_option_to_absolute_path_for_file_path() {
    let result = convert_option_to_absolute_path(
        "outDir",
        &OptionValue::String("bin".into()),
        &crate::declscompiler::COMMAND_LINE_COMPILER_OPTIONS_MAP,
        "/home/project",
    );
    assert_eq!(
        result,
        Some(OptionValue::String("/home/project/bin".into()))
    );
    // Non-file-path option returns None.
    assert!(convert_option_to_absolute_path(
        "strict",
        &OptionValue::Bool(true),
        &crate::declscompiler::COMMAND_LINE_COMPILER_OPTIONS_MAP,
        "/home/project",
    )
    .is_none());
}
