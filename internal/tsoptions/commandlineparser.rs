//! Command-line (`tsc` / `tsc -b`) argument parsing.
//!
//! 1:1 port of Go `internal/tsoptions/commandlineparser.go`. Token scanning,
//! response-file expansion, per-type value consumption, and the conversion of
//! the accumulated options map into typed `CompilerOptions`/`WatchOptions`/
//! `BuildOptions`.

use tsgo_collections::OrderedMap;
use tsgo_core::buildoptions::BuildOptions;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::watchoptions::WatchOptions;
use tsgo_diagnostics::{self as diagnostics, Message};
use tsgo_parser::Diagnostic;
use tsgo_tspath::{self as tspath, ComparePathsOptions};
use tsgo_vfs::Fs;

use crate::commandlineoption::{CommandLineOption, CommandLineOptionKind};
use crate::declscompiler::COMMAND_LINE_COMPILER_OPTIONS_MAP;
use crate::diagnostics::{
    ParseCommandLineWorkerDiagnostics, BUILD_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS,
    COMPILER_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS, WATCH_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS,
};
use crate::enummaps::EnumValue;
use crate::errors::{
    create_diagnostic_for_invalid_enum_type, create_unknown_option_error,
    get_compiler_option_value_type_string, new_compiler_diagnostic, validate_json_option_value,
};
use crate::namemap::{get_name_map_from_list, NameMap, BUILD_NAME_MAP, COMPILER_NAME_MAP};
use crate::parsedbuildcommandline::ParsedBuildCommandLine;
use crate::parsedcommandline::{new_parsed_command_line, ParsedCommandLine};
use crate::parsinghelpers::{
    convert_to_options_with_absolute_paths, parse_build_options, parse_compiler_options_pub,
    parse_watch_options,
};
use crate::OptionValue;

/// A host providing the file system and current directory for parsing.
///
/// Side effects: none (interface).
// Go: internal/tsoptions/tsconfigparsing.go:ParseConfigHost
pub trait ParseConfigHost {
    /// The file system to read response files / config files from.
    fn fs(&self) -> &dyn Fs;
    /// The current working directory used to resolve relative paths.
    fn get_current_directory(&self) -> &str;
}

/// Accumulator while scanning command-line tokens.
struct CommandLineParser<'a> {
    worker_diagnostics: &'a ParseCommandLineWorkerDiagnostics,
    options_map: NameMap,
    fs: Option<&'a dyn Fs>,
    current_directory: String,
    options: OrderedMap<String, OptionValue>,
    file_names: Vec<String>,
    errors: Vec<Diagnostic>,
}

/// The raw result of [`parse_command_line_test_worker`]: file names, the options
/// map, and diagnostics (mirrors Go's `export_test.go` `TestCommandLineParser`).
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/export_test.go:TestCommandLineParser
#[derive(Clone, Debug, Default)]
pub struct CommandLineParseTestResult {
    /// The collected non-option file names.
    pub file_names: Vec<String>,
    /// The accumulated raw options.
    pub options: OrderedMap<String, OptionValue>,
    /// The diagnostics produced.
    pub errors: Vec<Diagnostic>,
}

/// Parses a `tsc` command line into a [`ParsedCommandLine`].
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{parse_command_line, ParseConfigHost};
/// use tsgo_tsoptions::tsoptionstest::VfsParseConfigHost;
/// let host = VfsParseConfigHost::new(&[("/p/a.ts", "")], "/p", true);
/// let parsed = parse_command_line(&["--strict".to_string(), "a.ts".to_string()], &host);
/// assert_eq!(parsed.file_names(), &["a.ts".to_string()]);
/// assert!(parsed.compiler_options().strict.is_true());
/// ```
///
/// Side effects: reads response/config files through the host's file system.
// Go: internal/tsoptions/commandlineparser.go:ParseCommandLine
pub fn parse_command_line(
    command_line: &[String],
    host: &dyn ParseConfigHost,
) -> ParsedCommandLine {
    let parser = parse_command_line_worker(
        &COMPILER_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS,
        command_line,
        Some(host.fs()),
        host.get_current_directory(),
    );
    let options_with_absolute_paths = convert_to_options_with_absolute_paths(
        parser.options.clone(),
        &COMMAND_LINE_COMPILER_OPTIONS_MAP,
        host.get_current_directory(),
    );
    let compiler_options = convert_map_to_compiler_options(&options_with_absolute_paths);
    let watch_options = convert_map_to_watch_options(&options_with_absolute_paths);
    let mut result = new_parsed_command_line(
        compiler_options,
        parser.file_names,
        ComparePathsOptions {
            use_case_sensitive_file_names: host.fs().use_case_sensitive_file_names(),
            current_directory: host.get_current_directory().to_string(),
        },
    );
    result.parsed_config.watch_options = Some(Box::new(watch_options));
    result.errors = parser.errors;
    result.raw = OptionValue::Map(parser.options);
    result
}

/// Parses a `tsc -b` command line into a [`ParsedBuildCommandLine`].
///
/// Side effects: reads response files through the host's file system.
// Go: internal/tsoptions/commandlineparser.go:ParseBuildCommandLine
pub fn parse_build_command_line(
    command_line: &[String],
    host: &dyn ParseConfigHost,
) -> ParsedBuildCommandLine {
    let parser = parse_command_line_worker(
        &BUILD_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS,
        command_line,
        Some(host.fs()),
        host.get_current_directory(),
    );
    let mut compiler_options = CompilerOptions::default();
    for (key, value) in parser.options.entries() {
        let build_name = BUILD_NAME_MAP.get(key).map(|o| o.name);
        let compiler_name = COMPILER_NAME_MAP.get(key).map(|o| o.name);
        if build_name == Some("build") || (build_name.is_some() && build_name == compiler_name) {
            parse_compiler_options_pub(key, value, &mut compiler_options);
        }
    }

    let build_options = convert_map_to_build_options(&parser.options);
    let watch_options = convert_map_to_watch_options(&parser.options);
    let mut projects = parser.file_names;
    if projects.is_empty() {
        // `tsc -b` with no extra arguments behaves like `tsc -b .`.
        projects.push(".".to_string());
    }

    let mut errors = parser.errors;
    // Nonsensical combinations.
    let combine = |a: &str, b: &str| {
        new_compiler_diagnostic(
            &diagnostics::OPTIONS_0_AND_1_CANNOT_BE_COMBINED,
            vec![a.to_string(), b.to_string()],
        )
    };
    if build_options.clean.is_true() && build_options.force.is_true() {
        errors.push(combine("clean", "force"));
    }
    if build_options.clean.is_true() && build_options.verbose.is_true() {
        errors.push(combine("clean", "verbose"));
    }
    if build_options.clean.is_true() && compiler_options.watch.is_true() {
        errors.push(combine("clean", "watch"));
    }
    if compiler_options.watch.is_true() && build_options.dry.is_true() {
        errors.push(combine("watch", "dry"));
    }

    ParsedBuildCommandLine {
        build_options,
        compiler_options,
        watch_options,
        projects,
        errors,
        raw: OptionValue::Map(parser.options),
        compare_paths_options: ComparePathsOptions {
            use_case_sensitive_file_names: host.fs().use_case_sensitive_file_names(),
            current_directory: host.get_current_directory().to_string(),
        },
    }
}

/// Test-only worker mirroring Go's `ParseCommandLineTestWorker`: parses with the
/// given declarations (or the compiler defaults) and exposes the raw result.
///
/// Side effects: reads response files through `fs`.
// Go: internal/tsoptions/export_test.go:ParseCommandLineTestWorker
pub fn parse_command_line_test_worker(
    decls: Option<Vec<CommandLineOption>>,
    command_line: &[String],
    fs: Option<&dyn Fs>,
    current_directory: &str,
) -> CommandLineParseTestResult {
    let owned;
    let worker: &ParseCommandLineWorkerDiagnostics = match decls {
        Some(d) if !d.is_empty() => {
            owned = crate::diagnostics::get_parse_command_line_worker_diagnostics(d);
            &owned
        }
        _ => &COMPILER_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS,
    };
    let parser = parse_command_line_worker(worker, command_line, fs, current_directory);
    CommandLineParseTestResult {
        file_names: parser.file_names,
        options: parser.options,
        errors: parser.errors,
    }
}

// Go: internal/tsoptions/commandlineparser.go:parseCommandLineWorker
fn parse_command_line_worker<'a>(
    worker_diagnostics: &'a ParseCommandLineWorkerDiagnostics,
    command_line: &[String],
    fs: Option<&'a dyn Fs>,
    current_directory: &str,
) -> CommandLineParser<'a> {
    let options_map = get_name_map_from_list(&worker_diagnostics.did_you_mean.option_declarations);
    let mut parser = CommandLineParser {
        worker_diagnostics,
        options_map,
        fs,
        current_directory: current_directory.to_string(),
        options: OrderedMap::default(),
        file_names: Vec::new(),
        errors: Vec::new(),
    };
    parser.parse_strings(command_line);
    parser
}

/// Removes at most two leading `-` characters.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/commandlineparser.go:getInputOptionName
fn get_input_option_name(input: &str) -> &str {
    let s = input.strip_prefix('-').unwrap_or(input);
    s.strip_prefix('-').unwrap_or(s)
}

impl CommandLineParser<'_> {
    // Go: internal/tsoptions/commandlineparser.go:parseStrings
    fn parse_strings(&mut self, args: &[String]) {
        let mut i = 0;
        while i < args.len() {
            let s = args[i].clone();
            i += 1;
            if s.is_empty() {
                continue;
            }
            match s.as_bytes()[0] {
                b'@' => self.parse_response_file(&s[1..]),
                b'-' => {
                    let input_option_name = get_input_option_name(&s).to_string();
                    if let Some(opt) = self
                        .options_map
                        .get_option_declaration_from_name(&input_option_name, true)
                        .cloned()
                    {
                        let diag = self.worker_diagnostics.option_type_mismatch_diagnostic;
                        i = self.parse_option_value(args, i, &opt, diag);
                    } else if let Some(watch_opt) = crate::namemap::WATCH_NAME_MAP
                        .get_option_declaration_from_name(&input_option_name, true)
                        .cloned()
                    {
                        let diag =
                            WATCH_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS.option_type_mismatch_diagnostic;
                        i = self.parse_option_value(args, i, &watch_opt, diag);
                    } else {
                        let err = create_unknown_option_error(
                            &input_option_name,
                            self.worker_diagnostics
                                .did_you_mean
                                .unknown_option_diagnostic,
                            &s,
                            self.worker_diagnostics.did_you_mean.alternate_mode.as_ref(),
                        );
                        self.errors.push(err);
                    }
                }
                _ => self.file_names.push(s),
            }
        }
    }

    // Go: internal/tsoptions/commandlineparser.go:parseResponseFile
    fn parse_response_file(&mut self, file_name: &str) {
        let file_name = tspath::get_normalized_absolute_path(file_name, &self.current_directory);
        let file_contents = self.try_read_file(&file_name);
        if file_contents.is_empty() {
            return;
        }

        let mut args: Vec<String> = Vec::new();
        let text: Vec<char> = file_contents.chars().collect();
        let text_length = text.len();
        let mut pos = 0;
        while pos < text_length {
            while pos < text_length && text[pos] <= ' ' {
                pos += 1;
            }
            if pos >= text_length {
                break;
            }
            let start = pos;
            if text[pos] == '"' {
                pos += 1;
                while pos < text_length && text[pos] != '"' {
                    pos += 1;
                }
                if pos < text_length {
                    args.push(text[start + 1..pos].iter().collect());
                    pos += 1;
                } else {
                    self.errors.push(new_compiler_diagnostic(
                        &diagnostics::UNTERMINATED_QUOTED_STRING_IN_RESPONSE_FILE_0,
                        vec![file_name.clone()],
                    ));
                }
            } else {
                while pos < text_length && text[pos] > ' ' {
                    pos += 1;
                }
                args.push(text[start..pos].iter().collect());
            }
        }
        self.parse_strings(&args);
    }

    // Go: internal/tsoptions/commandlineparser.go:tryReadFile
    fn try_read_file(&mut self, file_name: &str) -> String {
        let text = self.fs.and_then(|fs| fs.read_file(file_name));
        match text {
            Some(t) if !t.is_empty() => t,
            _ => {
                // !!! Divergence (matches Go): the message does not include the
                // underlying read error.
                self.errors.push(new_compiler_diagnostic(
                    &diagnostics::CANNOT_READ_FILE_0,
                    vec![file_name.to_string()],
                ));
                String::new()
            }
        }
    }

    // Go: internal/tsoptions/commandlineparser.go:parseOptionValue
    fn parse_option_value(
        &mut self,
        args: &[String],
        mut i: usize,
        opt: &CommandLineOption,
        diag: &'static Message,
    ) -> usize {
        if opt.is_tsconfig_only {
            let opt_value = args.get(i).cloned().unwrap_or_default();
            if opt_value == "null" {
                self.options.set(opt.name.to_string(), OptionValue::Null);
                i += 1;
            } else if opt.kind == CommandLineOptionKind::Boolean {
                if opt_value == "false" {
                    self.options
                        .set(opt.name.to_string(), OptionValue::Bool(false));
                    i += 1;
                } else {
                    if opt_value == "true" {
                        i += 1;
                    }
                    self.errors.push(new_compiler_diagnostic(
                        &diagnostics::OPTION_0_CAN_ONLY_BE_SPECIFIED_IN_TSCONFIG_JSON_FILE_OR_SET_TO_FALSE_OR_NULL_ON_COMMAND_LINE,
                        vec![opt.name.to_string()],
                    ));
                }
            } else {
                self.errors.push(new_compiler_diagnostic(
                    &diagnostics::OPTION_0_CAN_ONLY_BE_SPECIFIED_IN_TSCONFIG_JSON_FILE_OR_SET_TO_NULL_ON_COMMAND_LINE,
                    vec![opt.name.to_string()],
                ));
                if !opt_value.is_empty() && !opt_value.starts_with('-') {
                    i += 1;
                }
            }
            return i;
        }

        // No argument provided (option is last, or followed by another option
        // that the boolean branch handles separately).
        if i >= args.len() {
            if opt.kind != CommandLineOptionKind::Boolean {
                self.errors.push(new_compiler_diagnostic(
                    diag,
                    vec![
                        opt.name.to_string(),
                        get_compiler_option_value_type_string(opt),
                    ],
                ));
                if opt.kind == CommandLineOptionKind::List {
                    self.options
                        .set(opt.name.to_string(), OptionValue::Array(vec![]));
                } else if opt.kind == CommandLineOptionKind::Enum {
                    self.errors
                        .push(create_diagnostic_for_invalid_enum_type(opt));
                }
            } else {
                self.options
                    .set(opt.name.to_string(), OptionValue::Bool(true));
            }
            return i;
        }

        if args[i] != "null" {
            match opt.kind {
                CommandLineOptionKind::Number => {
                    match args[i].parse::<i32>() {
                        Ok(num) => {
                            if num >= opt.min_value {
                                self.options
                                    .set(opt.name.to_string(), OptionValue::Int(num));
                            } else {
                                self.errors.push(new_compiler_diagnostic(
                                    &diagnostics::OPTION_0_REQUIRES_VALUE_TO_BE_GREATER_THAN_1,
                                    vec![opt.name.to_string(), opt.min_value.to_string()],
                                ));
                            }
                        }
                        Err(_) => {
                            self.errors.push(new_compiler_diagnostic(
                                diag,
                                vec![opt.name.to_string(), "number".to_string()],
                            ));
                        }
                    }
                    i += 1;
                }
                CommandLineOptionKind::Boolean => {
                    let opt_value = &args[i];
                    if opt_value == "false" {
                        self.options
                            .set(opt.name.to_string(), OptionValue::Bool(false));
                    } else {
                        self.options
                            .set(opt.name.to_string(), OptionValue::Bool(true));
                    }
                    if opt_value == "false" || opt_value == "true" {
                        i += 1;
                    }
                }
                CommandLineOptionKind::String => {
                    let (val, errs) =
                        validate_json_option_value(opt, &OptionValue::String(args[i].clone()));
                    if errs.is_empty() {
                        if let Some(v) = val {
                            self.options.set(opt.name.to_string(), v);
                        }
                    } else {
                        self.errors.extend(errs);
                    }
                    i += 1;
                }
                CommandLineOptionKind::List => {
                    let (result, errs) = parse_list_type_option(opt, &args[i]);
                    let result_len = result.len();
                    self.options
                        .set(opt.name.to_string(), OptionValue::Array(result));
                    let err_len = errs.len();
                    self.errors.extend(errs);
                    if result_len > 0 || err_len > 0 {
                        i += 1;
                    }
                }
                CommandLineOptionKind::ListOrElement => {
                    panic!("listOrElement not supported here");
                }
                _ => {
                    // enum / object
                    let (val, errs) = convert_json_option_of_enum_type(opt, args[i].trim());
                    self.options.set(opt.name.to_string(), val);
                    self.errors.extend(errs);
                    i += 1;
                }
            }
        } else {
            self.options.set(opt.name.to_string(), OptionValue::Null);
            i += 1;
        }
        i
    }
}

/// Splits a list-option value into element values, validating each.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/commandlineparser.go:ParseListTypeOption
pub fn parse_list_type_option(
    opt: &CommandLineOption,
    value: &str,
) -> (Vec<OptionValue>, Vec<Diagnostic>) {
    let value = value.trim();
    let mut errors: Vec<Diagnostic> = Vec::new();
    if value.starts_with('-') {
        return (vec![], errors);
    }
    if opt.kind == CommandLineOptionKind::ListOrElement && !value.contains(',') {
        let (val, errs) = validate_json_option_value(opt, &OptionValue::String(value.to_string()));
        if !errs.is_empty() {
            return (vec![], errs);
        }
        return (val.into_iter().collect(), errors);
    }
    if value.is_empty() {
        return (vec![], errors);
    }
    let values: Vec<&str> = value.split(',').collect();
    let element = opt
        .elements()
        .expect("list option has an element declaration");
    match element.kind {
        CommandLineOptionKind::String => {
            let mut result = Vec::new();
            for v in values {
                let (val, errs) =
                    validate_json_option_value(element, &OptionValue::String(v.to_string()));
                if let Some(OptionValue::String(s)) = &val {
                    if errs.is_empty() && !s.is_empty() {
                        result.push(OptionValue::String(s.clone()));
                        continue;
                    }
                }
                errors.extend(errs);
            }
            (result, errors)
        }
        CommandLineOptionKind::Boolean
        | CommandLineOptionKind::Object
        | CommandLineOptionKind::Number => {
            panic!("List of this element kind is not yet supported.");
        }
        _ => {
            let mut result = Vec::new();
            for v in values {
                let (val, errs) = convert_json_option_of_enum_type(element, v.trim());
                if let OptionValue::String(s) = &val {
                    if errs.is_empty() && !s.is_empty() {
                        result.push(OptionValue::String(s.clone()));
                        continue;
                    }
                }
                errors.extend(errs);
            }
            (result, errors)
        }
    }
}

// Go: internal/tsoptions/commandlineparser.go:convertJsonOptionOfEnumType
fn convert_json_option_of_enum_type(
    opt: &CommandLineOption,
    value: &str,
) -> (OptionValue, Vec<Diagnostic>) {
    if value.is_empty() {
        return (OptionValue::Null, vec![]);
    }
    let key = value.to_lowercase();
    let type_map = match opt.enum_map() {
        Some(m) => m,
        None => return (OptionValue::Null, vec![]),
    };
    match type_map.get(&key.as_str()) {
        Some(ev) => {
            let ov = match ev {
                EnumValue::Int(i) => OptionValue::Int(*i),
                EnumValue::Str(s) => OptionValue::String(s.to_string()),
            };
            let (v, errs) = validate_json_option_value(opt, &ov);
            (v.unwrap_or(OptionValue::Null), errs)
        }
        None => (
            OptionValue::Null,
            vec![create_diagnostic_for_invalid_enum_type(opt)],
        ),
    }
}

fn convert_map_to_compiler_options(map: &OrderedMap<String, OptionValue>) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    for (key, value) in map.entries() {
        parse_compiler_options_pub(key, value, &mut options);
    }
    options
}

fn convert_map_to_watch_options(map: &OrderedMap<String, OptionValue>) -> WatchOptions {
    let mut options = WatchOptions::default();
    for (key, value) in map.entries() {
        parse_watch_options(key, value, &mut options);
    }
    options
}

fn convert_map_to_build_options(map: &OrderedMap<String, OptionValue>) -> BuildOptions {
    let mut options = BuildOptions::default();
    for (key, value) in map.entries() {
        parse_build_options(key, value, &mut options);
    }
    options
}

#[cfg(test)]
#[path = "commandlineparser_test.rs"]
mod tests;
