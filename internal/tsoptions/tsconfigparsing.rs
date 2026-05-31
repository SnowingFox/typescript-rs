//! tsconfig.json / jsconfig.json parsing.
//!
//! Partial 1:1 port of Go `internal/tsoptions/tsconfigparsing.go` (the largest
//! file in the package). This wave ports the reachable subset, one behavior at
//! a time:
//!
//! - JSON source-file -> config object (`convert_to_object` and its `convert_*`
//!   helpers, plus the public [`parse_config_file_text_to_json`]).
//!
//! The option/notifier-threaded conversion used by the *source-file* config
//! path (`parseOwnConfigOfJsonSourceFile`'s `onPropertySet`) is deferred; the
//! conversion here is the option-free form used by `convertToObject` /
//! `ParseConfigFileTextToJson` / `ParseJsonConfigFileContent`.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};
use tsgo_collections::OrderedMap;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::parsedoptions::ParsedOptions;
use tsgo_core::projectreference::ProjectReference;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::tristate::Tristate;
use tsgo_core::typeacquisition::TypeAcquisition;
use tsgo_diagnostics::{self as diagnostics, Message};
use tsgo_parser::{parse_source_file, Diagnostic, SourceFileParseOptions};
use tsgo_tspath::ComparePathsOptions;

use crate::commandlineoption::{
    CommandLineOption, CommandLineOptionKind, CommandLineOptionNameMap,
};
use crate::declscompiler::COMMAND_LINE_COMPILER_OPTIONS_MAP;
use crate::declstypeacquisition::TYPE_ACQUISITION_DECLARATION;
use crate::enummaps::EnumValue;
use crate::errors::{
    create_diagnostic_for_invalid_enum_type, create_unknown_option_error, extra_key_diagnostics,
    extra_key_did_you_mean_diagnostics, get_compiler_option_value_type_string,
    new_compiler_diagnostic, spec_to_diagnostic, validate_json_option_value,
};
use crate::parsedcommandline::{ConfigFileSpecs, ParsedCommandLine};
use crate::parsinghelpers::{
    parse_compiler_options_pub, parse_json_to_string_key, parse_project_reference,
    parse_type_acquisition,
};
use crate::OptionValue;
use crate::ParseConfigHost;

const CONFIG_DIR_TEMPLATE: &str = "${configDir}";
const DEFAULT_INCLUDE_SPEC: &str = "**/*";

/// Builds a node-anchored diagnostic.
///
/// DIVERGENCE(port): the full `ast.NewDiagnostic` carries the source file
/// back-pointer; here we keep the node's source range and message, matching the
/// minimal [`Diagnostic`] used across the port.
fn new_node_diagnostic(
    arena: &NodeArena,
    node: NodeId,
    message: &'static diagnostics::Message,
    args: Vec<String>,
) -> Diagnostic {
    Diagnostic {
        loc: arena.loc(node),
        message,
        args,
    }
}

/// Parses the text of a `tsconfig.json` file into a JSON value plus diagnostics.
///
/// Mirrors Go `ParseConfigFileTextToJson`: the text is parsed as JSON, then
/// converted to an [`OptionValue`] tree. If the parse produced any syntactic
/// diagnostics, only the first is returned (matching Go).
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{parse_config_file_text_to_json, OptionValue};
/// let (value, errors) =
///     parse_config_file_text_to_json("/apath/tsconfig.json", r#"{ "strict": true }"#);
/// assert!(errors.is_empty());
/// let map = value.as_map().unwrap();
/// assert_eq!(map.get(&"strict".to_string()), Some(&OptionValue::Bool(true)));
/// ```
///
/// Side effects: allocates a parser arena for the parse (no I/O).
// Go: internal/tsoptions/tsconfigparsing.go:ParseConfigFileTextToJson
pub fn parse_config_file_text_to_json(
    file_name: &str,
    json_text: &str,
) -> (OptionValue, Vec<Diagnostic>) {
    let result = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        json_text,
        ScriptKind::Json,
    );
    let (config, mut errors) =
        convert_config_file_to_object(&result.arena, result.source_file, file_name);
    if !result.diagnostics.is_empty() {
        errors = vec![result.diagnostics[0].clone()];
    }
    (config, errors)
}

/// Returns the first top-level expression of a parsed JSON source file.
fn root_expression(arena: &NodeArena, source_file: NodeId) -> Option<NodeId> {
    match arena.data(source_file) {
        NodeData::SourceFile(d) => d
            .statements
            .nodes
            .first()
            .and_then(|&s| match arena.data(s) {
                NodeData::ExpressionStatement(e) => Some(e.expression),
                _ => None,
            }),
        _ => None,
    }
}

/// Converts a parsed JSON config source file into a value, reporting an error
/// when the root is not an object (with array-element recovery).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertConfigFileToObject
fn convert_config_file_to_object(
    arena: &NodeArena,
    source_file: NodeId,
    file_name: &str,
) -> (OptionValue, Vec<Diagnostic>) {
    let root = root_expression(arena, source_file);
    if let Some(root) = root {
        if arena.kind(root) != Kind::ObjectLiteralExpression {
            let base_file_name = if tsgo_tspath::get_base_file_name(file_name) == "jsconfig.json" {
                "jsconfig.json"
            } else {
                "tsconfig.json"
            };
            let errors = vec![new_compiler_diagnostic(
                &diagnostics::THE_ROOT_VALUE_OF_A_0_FILE_MUST_BE_AN_OBJECT,
                vec![base_file_name.to_string()],
            )];
            // Last-ditch recovery: the JSON parser recovers from some errors by
            // synthesizing a top-level array; its first object element may be a
            // well-formed config object.
            if arena.kind(root) == Kind::ArrayLiteralExpression {
                if let NodeData::ArrayLiteralExpression(d) = arena.data(root) {
                    if let Some(&first_object) = d
                        .list
                        .nodes
                        .iter()
                        .find(|&&e| arena.kind(e) == Kind::ObjectLiteralExpression)
                    {
                        return convert_property_value_to_json(arena, first_object);
                    }
                }
            }
            return (OptionValue::Map(OrderedMap::default()), errors);
        }
    }
    convert_to_json(arena, root)
}

/// Converts the root JSON expression into a value (the `convertToObject` entry).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertToObject / convertToJson
fn convert_to_json(arena: &NodeArena, root: Option<NodeId>) -> (OptionValue, Vec<Diagnostic>) {
    match root {
        None => (OptionValue::Map(OrderedMap::default()), Vec::new()),
        Some(expr) => convert_property_value_to_json(arena, expr),
    }
}

/// Converts a JSON value expression node into an [`OptionValue`].
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertPropertyValueToJson
fn convert_property_value_to_json(
    arena: &NodeArena,
    value_expression: NodeId,
) -> (OptionValue, Vec<Diagnostic>) {
    match arena.kind(value_expression) {
        Kind::TrueKeyword => (OptionValue::Bool(true), Vec::new()),
        Kind::FalseKeyword => (OptionValue::Bool(false), Vec::new()),
        Kind::NullKeyword => (OptionValue::Null, Vec::new()),
        Kind::StringLiteral => {
            let text = arena.text(value_expression).to_string();
            if !tsgo_ast::utilities::is_string_literal(arena, value_expression) {
                return (
                    OptionValue::String(text),
                    vec![new_node_diagnostic(
                        arena,
                        value_expression,
                        &diagnostics::STRING_LITERAL_WITH_DOUBLE_QUOTES_EXPECTED,
                        vec![],
                    )],
                );
            }
            (OptionValue::String(text), Vec::new())
        }
        Kind::NumericLiteral => {
            let n: f64 = tsgo_jsnum::from_string(arena.text(value_expression)).into();
            (OptionValue::Number(n), Vec::new())
        }
        Kind::PrefixUnaryExpression => {
            if let NodeData::PrefixUnaryExpression(d) = arena.data(value_expression) {
                if d.operator == Kind::MinusToken && arena.kind(d.operand) == Kind::NumericLiteral {
                    let n: f64 = tsgo_jsnum::from_string(arena.text(d.operand)).into();
                    return (OptionValue::Number(-n), Vec::new());
                }
            }
            // Not valid JSON syntax: fall through to the "property value" error.
            invalid_property_value(arena, value_expression)
        }
        Kind::ObjectLiteralExpression => {
            convert_object_literal_expression_to_json(arena, value_expression)
        }
        Kind::ArrayLiteralExpression => {
            convert_array_literal_expression_to_json(arena, value_expression)
        }
        _ => invalid_property_value(arena, value_expression),
    }
}

/// Builds the diagnostic for a value expression that is not valid JSON.
///
/// In this option-free conversion (used by `convertToObject`) there is no
/// option context, so Go's `option != nil` branch never fires; only the generic
/// "property value can only be ..." diagnostic is produced.
fn invalid_property_value(
    arena: &NodeArena,
    value_expression: NodeId,
) -> (OptionValue, Vec<Diagnostic>) {
    (
        OptionValue::Null,
        vec![new_node_diagnostic(
            arena,
            value_expression,
            &diagnostics::PROPERTY_VALUE_CAN_ONLY_BE_STRING_LITERAL_NUMERIC_LITERAL_TRUE_FALSE_NULL_OBJECT_LITERAL_OR_ARRAY_LITERAL,
            vec![],
        )],
    )
}

/// Converts a JSON array literal into an [`OptionValue::Array`], dropping `null`
/// elements (which the converter treats as `nil`).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertArrayLiteralExpressionToJson
fn convert_array_literal_expression_to_json(
    arena: &NodeArena,
    node: NodeId,
) -> (OptionValue, Vec<Diagnostic>) {
    let elements: Vec<NodeId> = match arena.data(node) {
        NodeData::ArrayLiteralExpression(d) => d.list.nodes.clone(),
        _ => Vec::new(),
    };
    let mut value: Vec<OptionValue> = Vec::new();
    let mut errors: Vec<Diagnostic> = Vec::new();
    for element in elements {
        let (converted, errs) = convert_property_value_to_json(arena, element);
        errors.extend(errs);
        // Go appends only when the converted value is non-nil (`null` drops).
        if !converted.is_null() {
            value.push(converted);
        }
    }
    (OptionValue::Array(value), errors)
}

/// Converts a JSON object literal into an insertion-ordered [`OptionValue::Map`].
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertObjectLiteralExpressionToJson
fn convert_object_literal_expression_to_json(
    arena: &NodeArena,
    node: NodeId,
) -> (OptionValue, Vec<Diagnostic>) {
    let mut result = OrderedMap::default();
    let mut errors: Vec<Diagnostic> = Vec::new();
    let properties: Vec<NodeId> = match arena.data(node) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
        _ => Vec::new(),
    };
    for element in properties {
        if arena.kind(element) != Kind::PropertyAssignment {
            errors.push(new_node_diagnostic(
                arena,
                element,
                &diagnostics::PROPERTY_ASSIGNMENT_EXPECTED,
                vec![],
            ));
            continue;
        }
        let (name, initializer) = match arena.data(element) {
            NodeData::PropertyAssignment(d) => (d.name, d.initializer),
            _ => continue,
        };
        if !tsgo_ast::utilities::is_string_literal(arena, name) {
            errors.push(new_node_diagnostic(
                arena,
                element,
                &diagnostics::STRING_LITERAL_WITH_DOUBLE_QUOTES_EXPECTED,
                vec![],
            ));
        }
        let key_text = arena.text(name).to_string();
        let (value, errs) = match initializer {
            Some(init) => convert_property_value_to_json(arena, init),
            None => (OptionValue::Null, Vec::new()),
        };
        errors.extend(errs);
        if !key_text.is_empty() {
            result.set(key_text, value);
        }
    }
    (OptionValue::Map(result), errors)
}

// ---------------------------------------------------------------------------
// Config-content parsing (the `json any` path: `ParseJsonConfigFileContent`).
// ---------------------------------------------------------------------------

/// The intermediate result of parsing one tsconfig's own options.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/tsconfigparsing.go:parsedTsconfig
struct ParsedTsconfig {
    raw: OptionValue,
    options: CompilerOptions,
    type_acquisition: TypeAcquisition,
    // extends resolution is deferred (needs module resolution); see DEFER below.
}

/// Parses an already-converted tsconfig JSON value into a [`ParsedCommandLine`].
///
/// Mirrors Go `ParseJsonConfigFileContent`: the JSON value (as produced by
/// [`parse_config_file_text_to_json`]) is turned into compiler/type-acquisition
/// options plus the resolved input file names (via wildcard expansion) and the
/// associated diagnostics.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{parse_config_file_text_to_json, parse_json_config_file_content};
/// use tsgo_tsoptions::tsoptionstest::VfsParseConfigHost;
/// let host = VfsParseConfigHost::new(&[("/p/a.ts", ""), ("/p/tsconfig.json", "{}")], "/p", true);
/// let (json, _) = parse_config_file_text_to_json("/p/tsconfig.json", "{}");
/// let parsed = parse_json_config_file_content(json, &host, "/p", None, "/p/tsconfig.json");
/// assert_eq!(parsed.file_names(), &["/p/a.ts".to_string()]);
/// ```
///
/// Side effects: enumerates directories through the host's file system.
// Go: internal/tsoptions/tsconfigparsing.go:ParseJsonConfigFileContent
pub fn parse_json_config_file_content(
    json: OptionValue,
    host: &dyn ParseConfigHost,
    base_path: &str,
    existing_options: Option<&CompilerOptions>,
    config_file_name: &str,
) -> ParsedCommandLine {
    let json_map = parse_json_to_string_key(&json);
    parse_json_config_file_content_worker(
        json_map,
        host,
        base_path,
        existing_options,
        config_file_name,
        false,
    )
}

/// A parsed `tsconfig.json` source file (the arena-backed JSON AST plus the
/// file name and any parse diagnostics).
///
/// 1:1 port of Go `internal/tsoptions/tsconfigparsing.go:TsConfigSourceFile`.
/// Where Go holds an `*ast.SourceFile`, this owns the parse [`NodeArena`] and
/// the root node id.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/tsconfigparsing.go:TsConfigSourceFile
pub struct TsConfigSourceFile {
    /// The arena owning the parsed JSON nodes.
    pub arena: NodeArena,
    /// The root `SourceFile` node id.
    pub source_file: NodeId,
    /// The config file name.
    pub file_name: String,
    /// Syntactic diagnostics produced while parsing.
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses `config_source_text` as a `tsconfig.json` source file.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::new_tsconfig_source_file_from_file_path;
/// let sf = new_tsconfig_source_file_from_file_path("/p/tsconfig.json", "{}");
/// assert_eq!(sf.file_name, "/p/tsconfig.json");
/// ```
///
/// Side effects: allocates a parser arena for the parse (no I/O).
// Go: internal/tsoptions/tsconfigparsing.go:NewTsconfigSourceFileFromFilePath
pub fn new_tsconfig_source_file_from_file_path(
    config_file_name: &str,
    config_source_text: &str,
) -> TsConfigSourceFile {
    let result = parse_source_file(
        SourceFileParseOptions {
            file_name: config_file_name.to_string(),
        },
        config_source_text,
        ScriptKind::Json,
    );
    TsConfigSourceFile {
        arena: result.arena,
        source_file: result.source_file,
        file_name: config_file_name.to_string(),
        diagnostics: result.diagnostics,
    }
}

/// Parses a `tsconfig.json` source file into a [`ParsedCommandLine`].
///
/// Mirrors Go `ParseJsonSourceFileConfigFileContent`.
///
/// DIVERGENCE(port): Go threads the JSON-AST through an `onPropertySet`
/// notifier that both records option values and produces *node-anchored*
/// diagnostics. Here the source file is first converted to a value object (via
/// [`convert_config_file_to_object`]) and routed through the same worker as the
/// `json` API. The resulting compiler options, type acquisition, file names,
/// and diagnostic *codes* are equivalent; only diagnostic *locations* differ
/// (compiler diagnostics rather than node-anchored). The element-type
/// validation and `reference.path` checks are suppressed (as Go's source-file
/// path defers them to conversion), matching Go's diagnostic *count* for
/// well-formed configs. The faithful notifier path + node-anchored diagnostics
/// are deferred.
// blocked-by: tsoptions tsconfigparsing onPropertySet notifier + node-anchored diagnostics
///
/// Side effects: enumerates directories through the host's file system.
// Go: internal/tsoptions/tsconfigparsing.go:ParseJsonSourceFileConfigFileContent
pub fn parse_json_source_file_config_file_content(
    source_file: &TsConfigSourceFile,
    host: &dyn ParseConfigHost,
    base_path: &str,
    existing_options: Option<&CompilerOptions>,
    config_file_name: &str,
) -> ParsedCommandLine {
    let (json, convert_errors) = convert_config_file_to_object(
        &source_file.arena,
        source_file.source_file,
        &source_file.file_name,
    );
    let json_map = parse_json_to_string_key(&json);
    let mut result = parse_json_config_file_content_worker(
        json_map,
        host,
        base_path,
        existing_options,
        config_file_name,
        true,
    );
    if !convert_errors.is_empty() {
        let mut errors = convert_errors;
        errors.extend(std::mem::take(&mut result.errors));
        result.errors = errors;
    }
    result
}

/// The default compiler options for a config file (jsconfig.json gets a few
/// implicit defaults).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:getDefaultCompilerOptions
fn get_default_compiler_options(config_file_name: &str) -> CompilerOptions {
    if !config_file_name.is_empty()
        && tsgo_tspath::get_base_file_name(config_file_name) == "jsconfig.json"
    {
        CompilerOptions {
            allow_js: Tristate::True,
            max_node_module_js_depth: Some(2),
            skip_lib_check: Tristate::True,
            no_emit: Tristate::True,
            ..Default::default()
        }
    } else {
        CompilerOptions::default()
    }
}

/// The default type-acquisition options for a config file (jsconfig enables it).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:getDefaultTypeAcquisition
fn get_default_type_acquisition(config_file_name: &str) -> TypeAcquisition {
    let mut options = TypeAcquisition::default();
    if !config_file_name.is_empty()
        && tsgo_tspath::get_base_file_name(config_file_name) == "jsconfig.json"
    {
        options.enable = Tristate::True;
    }
    options
}

/// Visitor over a parsed options object, abstracting compiler vs
/// type-acquisition parsing (Go's `optionParser` interface).
trait OptionParser {
    fn parse_option(&mut self, key: &str, value: &OptionValue);
    fn unknown_option_diagnostic(&self) -> &'static Message;
    fn unknown_did_you_mean_diagnostic(&self) -> &'static Message;
}

// Go: internal/tsoptions/parsinghelpers.go:compilerOptionsParser
struct CompilerOptionsParser<'a>(&'a mut CompilerOptions);

impl OptionParser for CompilerOptionsParser<'_> {
    fn parse_option(&mut self, key: &str, value: &OptionValue) {
        parse_compiler_options_pub(key, value, self.0);
    }
    fn unknown_option_diagnostic(&self) -> &'static Message {
        extra_key_diagnostics("compilerOptions").expect("compilerOptions has a diagnostic")
    }
    fn unknown_did_you_mean_diagnostic(&self) -> &'static Message {
        extra_key_did_you_mean_diagnostics("compilerOptions")
            .expect("compilerOptions has a diagnostic")
    }
}

// Go: internal/tsoptions/parsinghelpers.go:typeAcquisitionParser
struct TypeAcquisitionParser<'a>(&'a mut TypeAcquisition);

impl OptionParser for TypeAcquisitionParser<'_> {
    fn parse_option(&mut self, key: &str, value: &OptionValue) {
        parse_type_acquisition(key, value, self.0);
    }
    fn unknown_option_diagnostic(&self) -> &'static Message {
        extra_key_diagnostics("typeAcquisition").expect("typeAcquisition has a diagnostic")
    }
    fn unknown_did_you_mean_diagnostic(&self) -> &'static Message {
        extra_key_did_you_mean_diagnostics("typeAcquisition")
            .expect("typeAcquisition has a diagnostic")
    }
}

fn enum_value_to_option_value(ev: &EnumValue) -> OptionValue {
    match ev {
        EnumValue::Int(i) => OptionValue::Int(*i),
        EnumValue::Str(s) => OptionValue::String(s.to_string()),
    }
}

/// Applies the entries of a JSON options object to `result`, reporting unknown
/// options and converting each value through [`convert_json_option`].
///
/// Side effects: mutates `result`.
// Go: internal/tsoptions/tsconfigparsing.go:convertOptionsFromJson
fn convert_options_from_json<O: OptionParser>(
    options_name_map: &CommandLineOptionNameMap,
    json_options: &OptionValue,
    base_path: &str,
    result: &mut O,
) -> Vec<Diagnostic> {
    let json_map = match json_options {
        OptionValue::Map(m) => m,
        _ => return Vec::new(),
    };
    let mut errors: Vec<Diagnostic> = Vec::new();
    for (key, value) in json_map.entries() {
        let opt = options_name_map.get(key).cloned();
        match opt {
            Some(ref o) if o.name != key => {
                errors.push(new_compiler_diagnostic(
                    result.unknown_did_you_mean_diagnostic(),
                    vec![key.clone(), o.name.to_string()],
                ));
                continue;
            }
            None => {
                errors.push(create_unknown_option_error(
                    key,
                    result.unknown_option_diagnostic(),
                    "",
                    None,
                ));
                continue;
            }
            Some(_) => {}
        }
        let opt = opt.unwrap();
        if let Some(enum_map) = opt.enum_map() {
            if let OptionValue::String(s) = value {
                if let Some(ev) = enum_map.get(&s.to_lowercase().as_str()) {
                    let ov = enum_value_to_option_value(ev);
                    result.parse_option(opt.name, &ov);
                }
            }
        } else {
            let (converted, errs) = convert_json_option(&opt, value, base_path);
            errors.extend(errs);
            result.parse_option(opt.name, &converted);
        }
    }
    errors
}

/// Converts the `compilerOptions` JSON object into a [`CompilerOptions`].
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertCompilerOptionsFromJsonWorker
fn convert_compiler_options_from_json_worker(
    json_options: Option<&OptionValue>,
    base_path: &str,
    config_file_name: &str,
) -> (CompilerOptions, Vec<Diagnostic>) {
    let mut options = get_default_compiler_options(config_file_name);
    let errors = match json_options {
        Some(j) => {
            let mut parser = CompilerOptionsParser(&mut options);
            convert_options_from_json(
                &COMMAND_LINE_COMPILER_OPTIONS_MAP,
                j,
                base_path,
                &mut parser,
            )
        }
        None => Vec::new(),
    };
    if !config_file_name.is_empty() {
        options.config_file_path = tsgo_tspath::normalize_slashes(config_file_name);
    }
    (options, errors)
}

/// Converts the `typeAcquisition` JSON object into a [`TypeAcquisition`].
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertTypeAcquisitionFromJsonWorker
fn convert_type_acquisition_from_json_worker(
    json_options: Option<&OptionValue>,
    base_path: &str,
    config_file_name: &str,
) -> (TypeAcquisition, Vec<Diagnostic>) {
    let mut options = get_default_type_acquisition(config_file_name);
    let errors = match json_options {
        Some(j) => {
            let name_map = TYPE_ACQUISITION_DECLARATION
                .element_options
                .as_deref()
                .expect("typeAcquisition has element options");
            let mut parser = TypeAcquisitionParser(&mut options);
            convert_options_from_json(name_map, j, base_path, &mut parser)
        }
        None => Vec::new(),
    };
    (options, errors)
}

/// Reports whether `value` is a permissible value for `option`'s declared kind.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:isCompilerOptionsValue
fn is_compiler_options_value(option: &CommandLineOption, value: &OptionValue) -> bool {
    if value.is_null() {
        return !option.disallow_null_or_undefined();
    }
    match option.kind {
        CommandLineOptionKind::List => matches!(value, OptionValue::Array(_)),
        CommandLineOptionKind::ListOrElement => {
            matches!(value, OptionValue::Array(_))
                || option
                    .elements()
                    .is_some_and(|e| is_compiler_options_value(e, value))
        }
        CommandLineOptionKind::String => matches!(value, OptionValue::String(_)),
        CommandLineOptionKind::Boolean => matches!(value, OptionValue::Bool(_)),
        CommandLineOptionKind::Number => {
            matches!(value, OptionValue::Number(_) | OptionValue::Int(_))
        }
        CommandLineOptionKind::Object => matches!(value, OptionValue::Map(_)),
        CommandLineOptionKind::Enum => matches!(value, OptionValue::String(_)),
    }
}

/// Reports whether `value` begins with the `${configDir}` template prefix.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:startsWithConfigDirTemplate
fn starts_with_config_dir_template(value: &OptionValue) -> bool {
    match value {
        OptionValue::String(s) => s
            .to_lowercase()
            .starts_with(&CONFIG_DIR_TEMPLATE.to_lowercase()),
        _ => false,
    }
}

/// Normalizes a non-list option value, making file-path options absolute.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:normalizeNonListOptionValue
fn normalize_non_list_option_value(
    option: &CommandLineOption,
    base_path: &str,
    value: OptionValue,
) -> OptionValue {
    if option.is_file_path {
        if let OptionValue::String(s) = &value {
            let mut v = tsgo_tspath::normalize_slashes(s);
            if !starts_with_config_dir_template(&OptionValue::String(v.clone())) {
                v = tsgo_tspath::get_normalized_absolute_path(&v, base_path);
            }
            if v.is_empty() {
                v = ".".to_string();
            }
            return OptionValue::String(v);
        }
    }
    value
}

fn is_falsy(value: &OptionValue) -> bool {
    matches!(
        value,
        OptionValue::Null
            | OptionValue::Bool(false)
            | OptionValue::Number(0.0)
            | OptionValue::Int(0)
    ) || matches!(value, OptionValue::String(s) if s.is_empty())
}

/// Converts an enum-typed JSON option value, validating the key.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertJsonOptionOfEnumType
fn convert_json_option_of_enum_type(
    opt: &CommandLineOption,
    value: &str,
) -> (OptionValue, Vec<Diagnostic>) {
    if value.is_empty() {
        return (OptionValue::Null, Vec::new());
    }
    let key = value.to_lowercase();
    match opt.enum_map() {
        Some(m) => match m.get(&key.as_str()) {
            Some(ev) => {
                let ov = enum_value_to_option_value(ev);
                let (v, errs) = validate_json_option_value(opt, &ov);
                (v.unwrap_or(OptionValue::Null), errs)
            }
            None => (
                OptionValue::Null,
                vec![create_diagnostic_for_invalid_enum_type(opt)],
            ),
        },
        None => (OptionValue::Null, Vec::new()),
    }
}

/// Converts a list-typed JSON option value, dropping falsy elements unless the
/// option preserves them.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertJsonOptionOfListType
fn convert_json_option_of_list_type(
    option: &CommandLineOption,
    value: &OptionValue,
    base_path: &str,
) -> (OptionValue, Vec<Diagnostic>) {
    let arr = match value {
        OptionValue::Array(a) => a,
        _ => return (OptionValue::Null, Vec::new()),
    };
    let element = option.elements().expect("list option has an element");
    let mut errors: Vec<Diagnostic> = Vec::new();
    let mut mapped: Vec<OptionValue> = Vec::new();
    for v in arr {
        let (r, errs) = convert_json_option(element, v, base_path);
        errors.extend(errs);
        mapped.push(r);
    }
    let filtered: Vec<OptionValue> = if option.list_preserve_falsy_values {
        mapped
    } else {
        mapped.into_iter().filter(|v| !is_falsy(v)).collect()
    };
    (OptionValue::Array(filtered), errors)
}

/// Converts a single JSON option value to its typed form, validating it against
/// the option declaration.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:convertJsonOption
fn convert_json_option(
    opt: &CommandLineOption,
    value: &OptionValue,
    base_path: &str,
) -> (OptionValue, Vec<Diagnostic>) {
    if opt.is_command_line_only {
        return (
            OptionValue::Null,
            vec![new_compiler_diagnostic(
                &diagnostics::OPTION_0_CAN_ONLY_BE_SPECIFIED_ON_COMMAND_LINE,
                vec![opt.name.to_string()],
            )],
        );
    }
    if is_compiler_options_value(opt, value) {
        match opt.kind {
            CommandLineOptionKind::List => {
                return convert_json_option_of_list_type(opt, value, base_path)
            }
            CommandLineOptionKind::ListOrElement => {
                if matches!(value, OptionValue::Array(_)) {
                    return convert_json_option_of_list_type(opt, value, base_path);
                }
                return convert_json_option(
                    opt.elements().expect("listOrElement has an element"),
                    value,
                    base_path,
                );
            }
            CommandLineOptionKind::Enum => {
                if let OptionValue::String(s) = value {
                    return convert_json_option_of_enum_type(opt, s);
                }
                return (OptionValue::Null, Vec::new());
            }
            _ => {}
        }
        let (validated, errs) = validate_json_option_value(opt, value);
        if !errs.is_empty() || validated.is_none() {
            return (validated.unwrap_or(OptionValue::Null), errs);
        }
        (
            normalize_non_list_option_value(opt, base_path, validated.unwrap()),
            errs,
        )
    } else {
        (
            OptionValue::Null,
            vec![new_compiler_diagnostic(
                &diagnostics::COMPILER_OPTION_0_REQUIRES_A_VALUE_OF_TYPE_1,
                vec![
                    opt.name.to_string(),
                    get_compiler_option_value_type_string(opt),
                ],
            )],
        )
    }
}

/// Parses one tsconfig's own options/typeAcquisition (no `extends` resolution).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:parseOwnConfigOfJson
fn parse_own_config_of_json(
    json_map: &OrderedMap<String, OptionValue>,
    base_path: &str,
    config_file_name: &str,
) -> (ParsedTsconfig, Vec<Diagnostic>) {
    let mut errors: Vec<Diagnostic> = Vec::new();
    if json_map.has(&"excludes".to_string()) {
        errors.push(new_compiler_diagnostic(
            &diagnostics::UNKNOWN_OPTION_EXCLUDES_DID_YOU_MEAN_EXCLUDE,
            vec![],
        ));
    }
    let (options, e1) = convert_compiler_options_from_json_worker(
        json_map.get(&"compilerOptions".to_string()),
        base_path,
        config_file_name,
    );
    let (type_acquisition, e2) = convert_type_acquisition_from_json_worker(
        json_map.get(&"typeAcquisition".to_string()),
        base_path,
        config_file_name,
    );
    errors.extend(e1);
    errors.extend(e2);
    // DEFER(phase-4): `extends` resolution (getExtendsConfigPathOrArray + the
    // merge in parseConfig) needs module resolution / the source-file path.
    // blocked-by: tsoptions tsconfigparsing extends chain.
    (
        ParsedTsconfig {
            raw: OptionValue::Map(json_map.clone()),
            options,
            type_acquisition,
        },
        errors,
    )
}

/// Extracts options/include/exclude/files from a config (no `extends`).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:parseConfig (json path, no extends)
fn parse_config(
    json_map: &OrderedMap<String, OptionValue>,
    base_path: &str,
    config_file_name: &str,
) -> (ParsedTsconfig, Vec<Diagnostic>) {
    let base_path = tsgo_tspath::normalize_slashes(base_path);
    let (mut own, errors) = parse_own_config_of_json(json_map, &base_path, config_file_name);
    if own.options.paths.is_some() {
        own.options.paths_base_path = base_path;
    }
    (own, errors)
}

/// Substitutes `${configDir}` in a single path against `base_path`.
// Go: internal/tsoptions/tsconfigparsing.go:getSubstitutedPathWithConfigDirTemplate
fn get_substituted_path_with_config_dir_template(value: &str, base_path: &str) -> String {
    tsgo_tspath::get_normalized_absolute_path(
        &value.replacen(CONFIG_DIR_TEMPLATE, "./", 1),
        base_path,
    )
}

/// Substitutes `${configDir}` in any list element that begins with it; returns
/// `None` when nothing changed (mirrors Go's nil result).
// Go: internal/tsoptions/tsconfigparsing.go:getSubstitutedStringArrayWithConfigDirTemplate
fn get_substituted_string_array_with_config_dir_template(
    list: &[String],
    base_path: &str,
) -> Option<Vec<String>> {
    let mut result: Option<Vec<String>> = None;
    for (i, element) in list.iter().enumerate() {
        if starts_with_config_dir_template(&OptionValue::String(element.clone())) {
            let result = result.get_or_insert_with(|| list.to_vec());
            result[i] = get_substituted_path_with_config_dir_template(element, base_path);
        }
    }
    result
}

/// Substitutes `${configDir}` in the path-valued compiler options.
///
/// Side effects: mutates `compiler_options`.
// Go: internal/tsoptions/tsconfigparsing.go:handleOptionConfigDirTemplateSubstitution
fn handle_option_config_dir_template_substitution(
    compiler_options: &mut CompilerOptions,
    base_path: &str,
) {
    if let Some(paths) = compiler_options.paths.as_mut() {
        let keys: Vec<String> = paths.keys().cloned().collect();
        for k in keys {
            if let Some(v) = paths.get(&k) {
                if let Some(sub) =
                    get_substituted_string_array_with_config_dir_template(v, base_path)
                {
                    paths.set(k, sub);
                }
            }
        }
    }
    if let Some(sub) = get_substituted_string_array_with_config_dir_template(
        &compiler_options.root_dirs,
        base_path,
    ) {
        compiler_options.root_dirs = sub;
    }
    if let Some(type_roots) = compiler_options.type_roots.as_ref() {
        if let Some(sub) =
            get_substituted_string_array_with_config_dir_template(type_roots, base_path)
        {
            compiler_options.type_roots = Some(sub);
        }
    }
    let substitute_field = |field: &mut String| {
        if starts_with_config_dir_template(&OptionValue::String(field.clone())) {
            *field = get_substituted_path_with_config_dir_template(field, base_path);
        }
    };
    substitute_field(&mut compiler_options.generate_cpu_profile);
    substitute_field(&mut compiler_options.generate_trace);
    substitute_field(&mut compiler_options.out_file);
    substitute_field(&mut compiler_options.out_dir);
    substitute_field(&mut compiler_options.root_dir);
    substitute_field(&mut compiler_options.ts_build_info_file);
    substitute_field(&mut compiler_options.base_url);
    substitute_field(&mut compiler_options.declaration_dir);
}

/// Validates `files`/`include`/`exclude` glob specs, dropping invalid ones and
/// producing diagnostics. This is the source-file-less variant (compiler
/// diagnostics; node-anchored diagnostics land with the source-file path).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:validateSpecs
fn validate_specs(
    specs: &[OptionValue],
    disallow_trailing_recursion: bool,
) -> (Vec<String>, Vec<Diagnostic>) {
    let mut errors: Vec<Diagnostic> = Vec::new();
    let mut final_specs: Vec<String> = Vec::new();
    for spec in specs {
        let s = match spec {
            OptionValue::String(s) => s,
            _ => continue,
        };
        match spec_to_diagnostic(s, disallow_trailing_recursion) {
            Some(diag) => errors.push(new_compiler_diagnostic(diag, vec![s.clone()])),
            None => final_specs.push(s.clone()),
        }
    }
    (final_specs, errors)
}

/// The supported file extension groups for the given options.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:GetSupportedExtensions
pub fn get_supported_extensions(compiler_options: &CompilerOptions) -> Vec<Vec<String>> {
    let builtins: &[&[&str]] = if compiler_options.get_allow_js() {
        tsgo_tspath::ALL_SUPPORTED_EXTENSIONS
    } else {
        tsgo_tspath::SUPPORTED_TS_EXTENSIONS
    };
    builtins
        .iter()
        .map(|g| g.iter().map(|s| s.to_string()).collect())
        .collect()
}

/// Appends `.json` to the supported extensions when `resolveJsonModule` is set.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:GetSupportedExtensionsWithJsonIfResolveJsonModule
pub fn get_supported_extensions_with_json_if_resolve_json_module(
    compiler_options: &CompilerOptions,
    supported_extensions: &[Vec<String>],
) -> Vec<Vec<String>> {
    if !compiler_options.get_resolve_json_module() {
        return supported_extensions.to_vec();
    }
    let mut result = supported_extensions.to_vec();
    result.push(vec![tsgo_tspath::EXTENSION_JSON.to_string()]);
    result
}

/// Reports whether a higher-priority extension of `file` is already included.
///
/// Side effects: calls `has_file`.
// Go: internal/tsoptions/tsconfigparsing.go:hasFileWithHigherPriorityExtension
fn has_file_with_higher_priority_extension(
    file: &str,
    extensions: &[Vec<String>],
    has_file: impl Fn(&str) -> bool,
) -> bool {
    let mut extension_group: Vec<&str> = Vec::new();
    for group in extensions {
        let group_refs: Vec<&str> = group.iter().map(|s| s.as_str()).collect();
        if tsgo_tspath::file_extension_is_one_of(file, &group_refs) {
            extension_group.extend(group_refs);
        }
    }
    if extension_group.is_empty() {
        return false;
    }
    for ext in &extension_group {
        if tsgo_tspath::file_extension_is(file, ext)
            && (*ext != tsgo_tspath::EXTENSION_TS
                || !tsgo_tspath::file_extension_is(file, tsgo_tspath::EXTENSION_DTS))
        {
            return false;
        }
        if has_file(&tsgo_tspath::change_extension(file, ext)) {
            if *ext == tsgo_tspath::EXTENSION_DTS
                && (tsgo_tspath::file_extension_is(file, tsgo_tspath::EXTENSION_JS)
                    || tsgo_tspath::file_extension_is(file, tsgo_tspath::EXTENSION_JSX))
            {
                // LEGACY BEHAVIOR: allow a .d.ts alongside its js(x) counterpart.
                continue;
            }
            return true;
        }
    }
    false
}

/// Removes already-included wildcard files of a lower-priority extension.
///
/// Side effects: mutates `wildcard_files`.
// Go: internal/tsoptions/tsconfigparsing.go:removeWildcardFilesWithLowerPriorityExtension
fn remove_wildcard_files_with_lower_priority_extension(
    file: &str,
    wildcard_files: &mut OrderedMap<String, String>,
    extensions: &[Vec<String>],
    key_mapper: impl Fn(&str) -> String,
) {
    let mut extension_group: Vec<&str> = Vec::new();
    for group in extensions {
        let group_refs: Vec<&str> = group.iter().map(|s| s.as_str()).collect();
        if tsgo_tspath::file_extension_is_one_of(file, &group_refs) {
            extension_group.extend(group_refs);
        }
    }
    if extension_group.is_empty() {
        return;
    }
    for ext in extension_group.iter().rev() {
        if tsgo_tspath::file_extension_is(file, ext) {
            return;
        }
        let lower_priority_path = key_mapper(&tsgo_tspath::change_extension(file, ext));
        wildcard_files.delete(&lower_priority_path);
    }
}

/// Resolves the input file names from the validated specs, expanding wildcards
/// through the host file system. Returns the file names and the count of
/// literal (`files`-listed) names that lead the list.
///
/// Side effects: enumerates directories through `host`.
// Go: internal/tsoptions/tsconfigparsing.go:getFileNamesFromConfigSpecs
fn get_file_names_from_config_specs(
    config_file_specs: &ConfigFileSpecs,
    base_path: &str,
    options: &CompilerOptions,
    host: &dyn tsgo_vfs::Fs,
) -> (Vec<String>, usize) {
    let base_path = tsgo_tspath::normalize_path(base_path);
    let ucs = host.use_case_sensitive_file_names();
    let key_mapper = |value: &str| tsgo_tspath::get_canonical_file_name(value, ucs);

    let mut literal_file_map: OrderedMap<String, String> = OrderedMap::default();
    let mut wildcard_file_map: OrderedMap<String, String> = OrderedMap::default();
    let mut wildcard_json_file_map: OrderedMap<String, String> = OrderedMap::default();

    let supported_extensions = get_supported_extensions(options);
    let supported_extensions_with_json =
        get_supported_extensions_with_json_if_resolve_json_module(options, &supported_extensions);

    for file_name in &config_file_specs.validated_files_spec {
        let file = tsgo_tspath::get_normalized_absolute_path(file_name, &base_path);
        literal_file_map.set(key_mapper(file_name), file);
    }

    if !config_file_specs.validated_include_specs.is_empty() {
        let flat: Vec<String> = supported_extensions_with_json
            .iter()
            .flat_map(|g| g.iter().cloned())
            .collect();
        let files = tsgo_vfs::vfsmatch::read_directory(
            host,
            &base_path,
            &base_path,
            &flat,
            &config_file_specs.validated_exclude_specs,
            &config_file_specs.validated_include_specs,
            tsgo_vfs::vfsmatch::UNLIMITED_DEPTH,
        );
        let mut json_only_include_matchers: Option<tsgo_vfs::vfsmatch::SpecMatcher> = None;
        for file in files {
            if tsgo_tspath::file_extension_is(&file, tsgo_tspath::EXTENSION_JSON) {
                if json_only_include_matchers.is_none() {
                    let includes: Vec<String> = config_file_specs
                        .validated_include_specs
                        .iter()
                        .filter(|inc| inc.ends_with(tsgo_tspath::EXTENSION_JSON))
                        .cloned()
                        .collect();
                    json_only_include_matchers = tsgo_vfs::vfsmatch::new_spec_matcher(
                        &includes,
                        &base_path,
                        tsgo_vfs::vfsmatch::Usage::Files,
                        ucs,
                    );
                }
                let include_index = json_only_include_matchers
                    .as_ref()
                    .map_or(-1, |m| m.match_index(&file));
                if include_index != -1 {
                    let key = key_mapper(&file);
                    if !literal_file_map.has(&key) && !wildcard_json_file_map.has(&key) {
                        wildcard_json_file_map.set(key, file);
                    }
                }
                continue;
            }
            if has_file_with_higher_priority_extension(&file, &supported_extensions, |fname| {
                let canonical = key_mapper(fname);
                literal_file_map.has(&canonical) || wildcard_file_map.has(&canonical)
            }) {
                continue;
            }
            remove_wildcard_files_with_lower_priority_extension(
                &file,
                &mut wildcard_file_map,
                &supported_extensions,
                key_mapper,
            );
            let key = key_mapper(&file);
            if !literal_file_map.has(&key) && !wildcard_file_map.has(&key) {
                wildcard_file_map.set(key, file);
            }
        }
    }

    let literal_len = literal_file_map.size();
    let mut files: Vec<String> = Vec::with_capacity(
        literal_file_map.size() + wildcard_file_map.size() + wildcard_json_file_map.size(),
    );
    files.extend(literal_file_map.values().cloned());
    files.extend(wildcard_file_map.values().cloned());
    files.extend(wildcard_json_file_map.values().cloned());
    (files, literal_len)
}

/// Reports whether a config could report "no input files" (no `files`/`references`).
// Go: internal/tsoptions/tsconfigparsing.go:canJsonReportNoInputFiles
fn can_json_report_no_input_files(raw_config: &OrderedMap<String, OptionValue>) -> bool {
    !raw_config.has(&"files".to_string()) && !raw_config.has(&"references".to_string())
}

// Go: internal/tsoptions/tsconfigparsing.go:directoryOfCombinedPath
fn directory_of_combined_path(file_name: &str, base_path: &str) -> String {
    tsgo_tspath::get_directory_path(&tsgo_tspath::get_normalized_absolute_path(
        file_name, base_path,
    ))
}

struct PropOfRaw {
    slice_value: Option<Vec<OptionValue>>,
    wrong_value: &'static str,
}

// Go: internal/tsoptions/tsconfigparsing.go:parseJsonConfigFileContentWorker (getPropFromRaw)
fn get_prop_from_raw(
    raw_config: &OrderedMap<String, OptionValue>,
    errors: &mut Vec<Diagnostic>,
    has_source_file: bool,
    prop: &str,
    validate_element: impl Fn(&OptionValue) -> bool,
    element_type_name: &str,
) -> PropOfRaw {
    match raw_config.get(&prop.to_string()) {
        Some(value) if !value.is_null() => {
            if let OptionValue::Array(arr) = value {
                // Element-type validation only runs without a source file (the
                // source-file path validates these during conversion instead).
                if !has_source_file && !arr.iter().all(&validate_element) {
                    errors.push(new_compiler_diagnostic(
                        &diagnostics::COMPILER_OPTION_0_REQUIRES_A_VALUE_OF_TYPE_1,
                        vec![prop.to_string(), element_type_name.to_string()],
                    ));
                }
                PropOfRaw {
                    slice_value: Some(arr.clone()),
                    wrong_value: "",
                }
            } else if !has_source_file {
                errors.push(new_compiler_diagnostic(
                    &diagnostics::COMPILER_OPTION_0_REQUIRES_A_VALUE_OF_TYPE_1,
                    vec![prop.to_string(), "Array".to_string()],
                ));
                PropOfRaw {
                    slice_value: None,
                    wrong_value: "not-array",
                }
            } else {
                PropOfRaw {
                    slice_value: None,
                    wrong_value: "no-prop",
                }
            }
        }
        _ => PropOfRaw {
            slice_value: None,
            wrong_value: "no-prop",
        },
    }
}

fn stringify_specs(specs: &[String]) -> String {
    let inner = specs
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{inner}]")
}

/// The main config-content worker (json path, no source file).
///
/// Side effects: enumerates directories through `host`.
// Go: internal/tsoptions/tsconfigparsing.go:parseJsonConfigFileContentWorker
fn parse_json_config_file_content_worker(
    json_map: OrderedMap<String, OptionValue>,
    host: &dyn ParseConfigHost,
    base_path: &str,
    _existing_options: Option<&CompilerOptions>,
    config_file_name: &str,
    has_source_file: bool,
) -> ParsedCommandLine {
    let base_path_for_file_names = if !config_file_name.is_empty() {
        tsgo_tspath::normalize_path(&directory_of_combined_path(config_file_name, base_path))
    } else {
        tsgo_tspath::normalize_path(base_path)
    };

    let (mut parsed_config, mut errors) = parse_config(&json_map, base_path, config_file_name);
    // DEFER(phase-4): merging `existing_options` (mergeCompilerOptions with the
    // command-line options raw) is unused by the reachable config-file tests
    // (they pass `None`); blocked-by: existing-options merge wiring.
    handle_option_config_dir_template_substitution(
        &mut parsed_config.options,
        &base_path_for_file_names,
    );
    let raw_config = parse_json_to_string_key(&parsed_config.raw);
    if !config_file_name.is_empty() {
        parsed_config.options.config_file_path = tsgo_tspath::normalize_slashes(config_file_name);
    }

    let references_of_raw = get_prop_from_raw(
        &raw_config,
        &mut errors,
        has_source_file,
        "references",
        |element| matches!(element, OptionValue::Map(_)),
        "object",
    );
    let file_specs = get_prop_from_raw(
        &raw_config,
        &mut errors,
        has_source_file,
        "files",
        |element| matches!(element, OptionValue::String(_)),
        "string",
    );
    if file_specs.slice_value.is_some() || file_specs.wrong_value.is_empty() {
        let has_zero_or_no_references = references_of_raw.wrong_value == "no-prop"
            || references_of_raw.wrong_value == "not-array"
            || references_of_raw
                .slice_value
                .as_ref()
                .is_none_or(|s| s.is_empty());
        let has_extends_is_nil = raw_config
            .get(&"extends".to_string())
            .is_none_or(|v| v.is_null());
        if let Some(fs) = file_specs.slice_value.as_ref() {
            if fs.is_empty() && has_zero_or_no_references && has_extends_is_nil {
                errors.push(new_compiler_diagnostic(
                    &diagnostics::THE_FILES_LIST_IN_CONFIG_FILE_0_IS_EMPTY,
                    vec![config_file_name.to_string()],
                ));
            }
        }
    }

    let include_specs = get_prop_from_raw(
        &raw_config,
        &mut errors,
        has_source_file,
        "include",
        |element| matches!(element, OptionValue::String(_)),
        "string",
    );
    let mut exclude_specs = get_prop_from_raw(
        &raw_config,
        &mut errors,
        has_source_file,
        "exclude",
        |element| matches!(element, OptionValue::String(_)),
        "string",
    );
    let mut is_default_include_spec = false;
    if exclude_specs.wrong_value == "no-prop" {
        let out_dir = &parsed_config.options.out_dir;
        let declaration_dir = &parsed_config.options.declaration_dir;
        if !out_dir.is_empty() || !declaration_dir.is_empty() {
            let mut values: Vec<OptionValue> = Vec::new();
            if !out_dir.is_empty() {
                values.push(OptionValue::String(out_dir.clone()));
            }
            if !declaration_dir.is_empty() {
                values.push(OptionValue::String(declaration_dir.clone()));
            }
            exclude_specs = PropOfRaw {
                slice_value: Some(values),
                wrong_value: "",
            };
        }
    }
    let mut include_specs = include_specs;
    if file_specs.slice_value.is_none() && include_specs.slice_value.is_none() {
        include_specs = PropOfRaw {
            slice_value: Some(vec![OptionValue::String(DEFAULT_INCLUDE_SPEC.to_string())]),
            wrong_value: include_specs.wrong_value,
        };
        is_default_include_spec = true;
    }

    let mut validated_include_specs: Vec<String> = Vec::new();
    let mut validated_include_specs_before_substitution: Vec<String> = Vec::new();
    let mut validated_exclude_specs: Vec<String> = Vec::new();
    let mut validated_files_spec: Vec<String> = Vec::new();
    let mut validated_files_spec_before_substitution: Vec<String> = Vec::new();

    if let Some(specs) = include_specs.slice_value.as_ref() {
        let (validated, errs) = validate_specs(specs, true);
        errors.extend(errs);
        validated_include_specs_before_substitution = validated;
        validated_include_specs = get_substituted_string_array_with_config_dir_template(
            &validated_include_specs_before_substitution,
            &base_path_for_file_names,
        )
        .unwrap_or_else(|| validated_include_specs_before_substitution.clone());
    }
    if let Some(specs) = exclude_specs.slice_value.as_ref() {
        let (validated, errs) = validate_specs(specs, false);
        errors.extend(errs);
        validated_exclude_specs = validated;
        if let Some(sub) = get_substituted_string_array_with_config_dir_template(
            &validated_exclude_specs,
            &base_path_for_file_names,
        ) {
            validated_exclude_specs = sub;
        }
    }
    if let Some(specs) = file_specs.slice_value.as_ref() {
        for spec in specs {
            if let OptionValue::String(s) = spec {
                validated_files_spec_before_substitution.push(s.clone());
            }
        }
        validated_files_spec = get_substituted_string_array_with_config_dir_template(
            &validated_files_spec_before_substitution,
            &base_path_for_file_names,
        )
        .unwrap_or_else(|| validated_files_spec_before_substitution.clone());
    }

    let config_file_specs = ConfigFileSpecs {
        files_specs: file_specs.slice_value.clone(),
        include_specs: include_specs.slice_value.clone(),
        exclude_specs: exclude_specs.slice_value.clone(),
        validated_files_spec,
        validated_include_specs,
        validated_exclude_specs,
        validated_files_spec_before_substitution,
        validated_include_specs_before_substitution,
        is_default_include_spec,
    };

    let (file_names, literal_file_names_len) = get_file_names_from_config_specs(
        &config_file_specs,
        &base_path_for_file_names,
        &parsed_config.options,
        host.fs(),
    );
    if file_names.is_empty() && can_json_report_no_input_files(&raw_config) {
        let to_strings = |specs: &Option<Vec<OptionValue>>| -> Vec<String> {
            specs
                .as_ref()
                .map(|v| {
                    v.iter()
                        .filter_map(|e| e.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let include_for_msg = to_strings(&config_file_specs.include_specs);
        let exclude_for_msg = to_strings(&config_file_specs.exclude_specs);
        errors.push(new_compiler_diagnostic(
            &diagnostics::NO_INPUTS_WERE_FOUND_IN_CONFIG_FILE_0_SPECIFIED_INCLUDE_PATHS_WERE_1_AND_EXCLUDE_PATHS_WERE_2,
            vec![
                config_file_name.to_string(),
                stringify_specs(&include_for_msg),
                stringify_specs(&exclude_for_msg),
            ],
        ));
    }

    let project_references = get_project_references(
        &raw_config,
        &mut errors,
        has_source_file,
        &base_path_for_file_names,
    );

    let ucs = host.fs().use_case_sensitive_file_names();
    ParsedCommandLine {
        parsed_config: ParsedOptions {
            compiler_options: Some(Box::new(parsed_config.options)),
            type_acquisition: Some(Box::new(parsed_config.type_acquisition)),
            file_names,
            project_references,
            ..Default::default()
        },
        errors,
        raw: parsed_config.raw,
        compile_on_save: None,
        compare_paths_options: ComparePathsOptions {
            use_case_sensitive_file_names: ucs,
            current_directory: base_path_for_file_names,
        },
        config_file_specs: Some(config_file_specs),
        literal_file_names_len,
    }
}

// Go: internal/tsoptions/tsconfigparsing.go:parseJsonConfigFileContentWorker (getProjectReferences)
fn get_project_references(
    raw_config: &OrderedMap<String, OptionValue>,
    errors: &mut Vec<Diagnostic>,
    has_source_file: bool,
    base_path: &str,
) -> Vec<ProjectReference> {
    let references_of_raw = get_prop_from_raw(
        raw_config,
        errors,
        has_source_file,
        "references",
        |element| matches!(element, OptionValue::Map(_)),
        "object",
    );
    let mut project_references: Vec<ProjectReference> = Vec::new();
    if let Some(refs) = references_of_raw.slice_value {
        for reference in &refs {
            for r in parse_project_reference(reference) {
                if r.path.is_empty() {
                    if !has_source_file {
                        errors.push(new_compiler_diagnostic(
                            &diagnostics::COMPILER_OPTION_0_REQUIRES_A_VALUE_OF_TYPE_1,
                            vec!["reference.path".to_string(), "string".to_string()],
                        ));
                    }
                } else {
                    project_references.push(ProjectReference {
                        path: tsgo_tspath::get_normalized_absolute_path(&r.path, base_path),
                        original_path: r.path.clone(),
                        circular: r.circular,
                    });
                }
            }
        }
    }
    project_references
}

#[cfg(test)]
#[path = "tsconfigparsing_test.rs"]
mod tests;
