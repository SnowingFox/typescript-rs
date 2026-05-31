//! `tsgo_parser` — recursive-descent parser porting Go `internal/parser`.
//!
//! This crate turns a token stream from [`tsgo_scanner`] into an `ast`
//! [`NodeArena`] tree rooted at a `SourceFile`. It owns a [`Scanner`] and a
//! [`NodeArena`] and drives them with the classic recursive-descent + operator-
//! precedence algorithm, recording diagnostics for syntax errors without
//! aborting the parse.
//!
//! # Ownership / scanner-error wiring (read this first)
//!
//! Go stores the parser's diagnostics on the returned `*ast.SourceFile` and
//! wires the scanner's `OnError` callback directly to `p.scanError`. In safe
//! Rust the scanner is owned by the parser, so a callback cannot also borrow
//! `&mut Parser`. Instead, the scanner pushes raw scan errors into a shared
//! `Rc<RefCell<Vec<ScanError>>>`; the parser drains that buffer right after
//! each scan and funnels every entry through `parse_error_at_range`
//! (preserving Go's de-duplication and ordering). The arena is returned to the
//! caller in [`ParseResult`] because, unlike Go's GC graph, the Rust arena owns
//! every node.

pub mod types;
pub mod utilities;

pub use types::ParseFlags;

use std::cell::RefCell;
use std::rc::Rc;

use tsgo_ast::utilities::{
    is_assignment_operator, is_class_member_modifier, is_keyword,
    is_left_hand_side_expression_kind, is_modifier_kind, modifier_to_flag, node_is_present,
};
use tsgo_ast::{Kind, ModifierFlags, ModifierList, NodeArena, NodeFlags, NodeId, NodeList};
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::text::TextRange;
use tsgo_core::tristate::Tristate;
use tsgo_diagnostics::{self as diagnostics, Message};
use tsgo_scanner::Scanner;

use utilities::{get_language_variant, token_is_identifier_or_keyword};

/// Options describing the file being parsed.
///
/// This is a minimal port of Go `ast.SourceFileParseOptions`; only the fields
/// the current parser slice needs are present. The [`file_name`](Self::file_name)
/// drives declaration-file detection (`.d.ts` suffix) and is stored on the
/// resulting [`SourceFile`](tsgo_ast::NodeData::SourceFile) node.
///
/// # Examples
/// ```
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let result = parse_source_file(
///     SourceFileParseOptions { file_name: "lib.d.ts".into() },
///     "declare const x: number;",
///     ScriptKind::Ts,
/// );
/// match result.arena.data(result.source_file) {
///     tsgo_ast::NodeData::SourceFile(d) => assert!(d.is_declaration_file),
///     _ => panic!("expected SourceFile"),
/// }
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ast/parseoptions.go:SourceFileParseOptions
#[derive(Clone, Debug, Default)]
pub struct SourceFileParseOptions {
    /// The file name (used for declaration-file detection and diagnostics).
    pub file_name: String,
}

/// A syntactic diagnostic produced during parsing.
///
/// A minimal stand-in for Go `ast.Diagnostic`; it carries the source range, the
/// (static) message, and the already-stringified format arguments. The full
/// `ast.Diagnostic` (message chains, related info, file back-pointer) is ported
/// in a later phase. Diagnostics at the same start offset are de-duplicated, matching
/// Go `parseErrorAtRange`.
///
/// # Examples
/// ```
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let result = parse_source_file(
///     SourceFileParseOptions::default(),
///     "{ 'a': 1 }",
///     ScriptKind::Json,
/// );
/// assert_eq!(result.diagnostics.len(), 1);
/// assert_eq!(result.diagnostics[0].pos(), 2);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ast/diagnostic.go:Diagnostic
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// The source range the diagnostic applies to.
    pub loc: TextRange,
    /// The (static) diagnostic message.
    pub message: &'static Message,
    /// Stringified message arguments.
    pub args: Vec<String>,
}

impl Diagnostic {
    /// Returns the diagnostic's start offset (mirrors Go `Diagnostic.Pos()`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// use tsgo_core::scriptkind::ScriptKind;
    /// let result = parse_source_file(
    ///     SourceFileParseOptions::default(),
    ///     "{ 'a': 1 }",
    ///     ScriptKind::Json,
    /// );
    /// assert_eq!(result.diagnostics[0].pos(), 2);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/diagnostic.go:Diagnostic.Pos
    pub fn pos(&self) -> i32 {
        self.loc.pos()
    }
}

/// The result of parsing: the owning arena, the `SourceFile` node id, and the
/// collected diagnostics.
///
/// Because Rust's arena owns every node (unlike Go's GC graph), callers receive
/// the [`NodeArena`](tsgo_ast::NodeArena) alongside the root [`source_file`](Self::source_file)
/// id. Syntactic diagnostics are returned in source order.
///
/// # Examples
/// ```
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::Kind;
/// let result = parse_source_file(SourceFileParseOptions::default(), "", ScriptKind::Ts);
/// assert!(result.diagnostics.is_empty());
/// assert_eq!(result.arena.kind(result.source_file), Kind::SourceFile);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/parser/parser.go:ParseSourceFile (return bundle)
#[derive(Debug)]
pub struct ParseResult {
    /// The arena owning every parsed node.
    pub arena: NodeArena,
    /// The id of the root `SourceFile` node.
    pub source_file: NodeId,
    /// Syntactic diagnostics, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// One raw scanner error, buffered until the parser drains it.
///
/// Side effects: none (pure value type).
// Go: internal/scanner/scanner.go:ErrorCallback
#[derive(Clone, Debug)]
struct ScanError {
    message: &'static Message,
    pos: i32,
    length: i32,
}

type ScanErrorSink = Rc<RefCell<Vec<ScanError>>>;

/// Parsing contexts, mirroring Go's `ParsingContext` `iota` enum. The
/// discriminant values must match Go because they index the `parsing_contexts`
/// bit set.
///
/// The full set of contexts is enumerated to keep the bit values aligned with
/// Go; productions that consume the not-yet-ported contexts are added in later
/// slices, so the unused variants are tolerated.
///
/// Side effects: none (pure value type).
// Go: internal/parser/parser.go:ParsingContext
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum ParsingContext {
    SourceElements = 0,
    BlockStatements,
    SwitchClauses,
    SwitchClauseStatements,
    TypeMembers,
    ClassMembers,
    EnumMembers,
    HeritageClauseElement,
    VariableDeclarations,
    ObjectBindingElements,
    ArrayBindingElements,
    ArgumentExpressions,
    ObjectLiteralMembers,
    JsxAttributes,
    JsxChildren,
    ArrayLiteralMembers,
    Parameters,
    JSDocParameters,
    RestProperties,
    TypeParameters,
    TypeArguments,
    TupleElementTypes,
    HeritageClauses,
    ImportOrExportSpecifiers,
    ImportAttributes,
    JSDocComment,
}

/// The recursive-descent parser: a short-lived mutable state machine over a
/// [`Scanner`] and a [`NodeArena`].
///
/// Side effects: `parse_*` methods drive the scanner and mutate the arena.
// Go: internal/parser/parser.go:Parser
struct Parser {
    scanner: Scanner,
    arena: NodeArena,
    opts: SourceFileParseOptions,
    source_text: String,
    script_kind: ScriptKind,
    language_variant: LanguageVariant,
    diagnostics: Vec<Diagnostic>,
    token: Kind,
    context_flags: NodeFlags,
    parsing_contexts: u32,
    has_parse_error: bool,
    identifier_count: usize,
    scan_errors: ScanErrorSink,
}

/// A snapshot of mutable parser state for speculative parsing (`look_ahead`).
///
/// Side effects: none (pure value type).
// Go: internal/parser/parser.go:ParserState
struct ParserState {
    scanner_state: tsgo_scanner::ScannerStateSnapshot,
    context_flags: NodeFlags,
    diagnostics_len: usize,
    has_parse_error: bool,
}

/// Parses `source_text` as a source file of the given `script_kind`.
///
/// # Examples
/// ```
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let result = parse_source_file(SourceFileParseOptions::default(), "", ScriptKind::Ts);
/// assert!(result.diagnostics.is_empty());
/// ```
///
/// Side effects: allocates a fresh arena and scanner for the parse.
// Go: internal/parser/parser.go:ParseSourceFile
pub fn parse_source_file(
    opts: SourceFileParseOptions,
    source_text: &str,
    script_kind: ScriptKind,
) -> ParseResult {
    let mut p = Parser::new();
    p.initialize_state(opts, source_text, script_kind);
    p.next_token();
    p.parse_source_file_worker()
}

/// Parses an isolated entity name (`a`, `a.b.c`). Returns the owning arena and
/// the entity-name node id, or `None` if the input is not exactly one entity
/// name followed by end-of-file with no diagnostics.
///
/// # Examples
/// ```
/// use tsgo_parser::parse_isolated_entity_name;
/// assert!(parse_isolated_entity_name("a.b.c").is_some());
/// assert!(parse_isolated_entity_name("a.=").is_none());
/// ```
///
/// Side effects: allocates a fresh arena and scanner for the parse.
// Go: internal/parser/parser.go:ParseIsolatedEntityName
pub fn parse_isolated_entity_name(text: &str) -> Option<(NodeArena, NodeId)> {
    let mut p = Parser::new();
    p.initialize_state(SourceFileParseOptions::default(), text, ScriptKind::Js);
    p.next_token();
    let entity_name = p.parse_entity_name(true, None);
    if p.token == Kind::EndOfFile && p.diagnostics.is_empty() {
        Some((p.arena, entity_name))
    } else {
        None
    }
}

impl Parser {
    fn new() -> Parser {
        Parser {
            scanner: Scanner::new(),
            arena: NodeArena::new(),
            opts: SourceFileParseOptions::default(),
            source_text: String::new(),
            script_kind: ScriptKind::Unknown,
            language_variant: LanguageVariant::Standard,
            diagnostics: Vec::new(),
            token: Kind::Unknown,
            context_flags: NodeFlags::NONE,
            parsing_contexts: 0,
            has_parse_error: false,
            identifier_count: 0,
            scan_errors: Rc::new(RefCell::new(Vec::new())),
        }
    }

    // Go: internal/parser/parser.go:initializeState
    fn initialize_state(
        &mut self,
        opts: SourceFileParseOptions,
        source_text: &str,
        script_kind: ScriptKind,
    ) {
        if script_kind == ScriptKind::Unknown {
            panic!(
                "ScriptKind must be specified when parsing source file: {}",
                opts.file_name
            );
        }
        self.opts = opts;
        self.source_text = source_text.to_string();
        self.script_kind = script_kind;
        self.language_variant = get_language_variant(script_kind);
        self.context_flags = match script_kind {
            ScriptKind::Js | ScriptKind::Jsx => NodeFlags::JAVA_SCRIPT_FILE,
            ScriptKind::Json => NodeFlags::JAVA_SCRIPT_FILE | NodeFlags::JSON_FILE,
            _ => NodeFlags::NONE,
        };
        self.scanner.set_text(source_text.to_string());
        let sink = self.scan_errors.clone();
        self.scanner
            .set_on_error(Some(Box::new(move |message, pos, length| {
                sink.borrow_mut().push(ScanError {
                    message,
                    pos,
                    length,
                });
            })));
        self.scanner.set_language_variant(self.language_variant);
    }

    // Go: internal/parser/parser.go:parseSourceFileWorker
    fn parse_source_file_worker(&mut self) -> ParseResult {
        if self.script_kind == ScriptKind::Json {
            return self.parse_json_text();
        }
        let is_declaration_file = tsgo_tspath::is_declaration_file_name(&self.opts.file_name);
        if is_declaration_file {
            self.context_flags |= NodeFlags::AMBIENT;
        }
        let pos = self.node_pos();
        let statements = self.parse_list_index(ParsingContext::SourceElements, |p| {
            p.parse_toplevel_statement()
        });
        let end = self.node_pos();
        let eof = self.parse_token_node();
        if self.arena.kind(eof) != Kind::EndOfFile {
            panic!("Expected end of file token from scanner.");
        }
        let statement_list = self.new_node_list(TextRange::new(pos, end), statements);
        let node = self.arena.new_source_file(
            &self.opts.file_name,
            self.script_kind,
            self.language_variant,
            statement_list,
            eof,
        );
        let node = self.finish_node(node, pos);
        self.set_is_declaration_file(node, is_declaration_file);
        self.finish_source_file(node);
        ParseResult {
            arena: std::mem::take(&mut self.arena),
            source_file: node,
            diagnostics: std::mem::take(&mut self.diagnostics),
        }
    }

    // Go: internal/parser/parser.go:finishSourceFile (subset)
    fn finish_source_file(&mut self, source_file: NodeId) {
        self.collect_external_module_references(source_file);
        self.set_external_module_indicator(source_file);
        // DEFER(phase-4): comment directives, pragmas, `@jsImportTag` reparse,
        // CommonJS module indicator, and top-level-await reparse.
        // blocked-by: JSDoc/pragma scanning subsystem.
    }

    // Go: internal/parser/references.go:collectExternalModuleReferences (subset)
    fn collect_external_module_references(&mut self, file: NodeId) {
        let statements = match self.arena.data(file) {
            tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes.clone(),
            _ => return,
        };
        let mut imports = Vec::new();
        for stmt in statements {
            self.collect_module_references(&mut imports, stmt);
        }
        // DEFER(phase-4): dynamic import/require call collection + ambient
        // module augmentations + node-core-module URI style tracking.
        if let tsgo_ast::NodeData::SourceFile(d) = self.arena.data_mut(file) {
            d.imports = imports;
        }
    }

    // Go: internal/parser/references.go:collectModuleReferences (subset)
    fn collect_module_references(&mut self, imports: &mut Vec<NodeId>, node: NodeId) {
        if self.is_any_import_or_re_export(node) {
            if let Some(name_expr) = self.get_external_module_name(node) {
                if self.arena.kind(name_expr) == Kind::StringLiteral
                    && !self.arena.text(name_expr).is_empty()
                {
                    imports.push(name_expr);
                }
            }
        }
    }

    // Go: internal/ast/utilities.go:IsAnyImportOrReExport
    fn is_any_import_or_re_export(&self, node: NodeId) -> bool {
        matches!(
            self.arena.kind(node),
            Kind::ImportDeclaration | Kind::ImportEqualsDeclaration | Kind::ExportDeclaration
        )
    }

    // Go: internal/ast/utilities.go:GetExternalModuleName
    fn get_external_module_name(&self, node: NodeId) -> Option<NodeId> {
        match self.arena.data(node) {
            tsgo_ast::NodeData::ImportDeclaration(d) => Some(d.module_specifier),
            tsgo_ast::NodeData::ExportDeclaration(d) => d.module_specifier,
            tsgo_ast::NodeData::ImportEqualsDeclaration(d) => {
                match self.arena.data(d.module_reference) {
                    tsgo_ast::NodeData::ExternalModuleReference(e) => Some(e.expression),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // Go: internal/parser/parser.go:isExternalModuleIndicator (subset)
    fn set_external_module_indicator(&mut self, file: NodeId) {
        let statements = match self.arena.data(file) {
            tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes.clone(),
            _ => return,
        };
        let mut indicator = None;
        for stmt in statements {
            if self.is_an_external_module_indicator(stmt) {
                indicator = Some(stmt);
                break;
            }
        }
        // DEFER(phase-4): `import.meta` indicator + ExternalModuleIndicatorOptions
        // (Force/JSX) fallbacks.
        if let tsgo_ast::NodeData::SourceFile(d) = self.arena.data_mut(file) {
            d.external_module_indicator = indicator;
        }
    }

    // Go: internal/parser/parser.go:isAnExternalModuleIndicatorNode
    fn is_an_external_module_indicator(&self, node: NodeId) -> bool {
        if self.has_export_modifier(node) {
            return true;
        }
        match self.arena.kind(node) {
            Kind::ImportEqualsDeclaration => match self.arena.data(node) {
                tsgo_ast::NodeData::ImportEqualsDeclaration(d) => {
                    self.arena.kind(d.module_reference) == Kind::ExternalModuleReference
                }
                _ => false,
            },
            Kind::ImportDeclaration | Kind::ExportAssignment | Kind::ExportDeclaration => true,
            _ => false,
        }
    }

    // Go: internal/ast/utilities.go:HasSyntacticModifier(ModifierFlagsExport)
    fn has_export_modifier(&self, node: NodeId) -> bool {
        let modifiers = match self.arena.data(node) {
            tsgo_ast::NodeData::VariableStatement(d) => d.modifiers.as_ref(),
            tsgo_ast::NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
            tsgo_ast::NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
            tsgo_ast::NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
            tsgo_ast::NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
            tsgo_ast::NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
            tsgo_ast::NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
            tsgo_ast::NodeData::ImportEqualsDeclaration(d) => d.modifiers.as_ref(),
            _ => None,
        };
        modifiers.is_some_and(|m| m.modifier_flags.contains(ModifierFlags::EXPORT))
    }

    fn set_is_declaration_file(&mut self, source_file: NodeId, value: bool) {
        if let tsgo_ast::NodeData::SourceFile(d) = self.arena.data_mut(source_file) {
            d.is_declaration_file = value;
        }
    }

    // Go: internal/parser/parser.go:parseJSONText
    fn parse_json_text(&mut self) -> ParseResult {
        let pos = self.node_pos();
        let statements;
        let eof;
        if self.token == Kind::EndOfFile {
            statements = self.new_node_list(TextRange::new(pos, self.node_pos()), Vec::new());
            eof = self.parse_token_node();
        } else {
            let mut expressions: Vec<NodeId> = Vec::new();
            while self.token != Kind::EndOfFile {
                let token = self.token;
                let expression = match token {
                    Kind::OpenBracketToken => self.parse_array_literal_expression(),
                    Kind::TrueKeyword | Kind::FalseKeyword | Kind::NullKeyword => {
                        self.parse_token_node()
                    }
                    Kind::MinusToken => {
                        if self.look_ahead(|p| {
                            p.next_token() == Kind::NumericLiteral
                                && p.next_token() != Kind::ColonToken
                        }) {
                            self.parse_prefix_unary_expression()
                        } else {
                            self.parse_object_literal_expression()
                        }
                    }
                    Kind::NumericLiteral | Kind::StringLiteral
                        if self.look_ahead(|p| p.next_token() != Kind::ColonToken) =>
                    {
                        self.parse_literal_expression()
                    }
                    _ => self.parse_object_literal_expression(),
                };
                let was_empty = expressions.is_empty();
                expressions.push(expression);
                if was_empty && self.token != Kind::EndOfFile {
                    self.parse_error_at_current_token(&diagnostics::UNEXPECTED_TOKEN, Vec::new());
                }
            }
            let expression = if expressions.len() > 1 {
                let list = self.new_node_list(TextRange::new(pos, self.node_pos()), expressions);
                let arr = self.arena.new_array_literal_expression(list);
                self.finish_node(arr, pos)
            } else {
                expressions[0]
            };
            let stmt = self.arena.new_expression_statement(expression);
            let statement = self.finish_node(stmt, pos);
            statements = self.new_node_list(TextRange::new(pos, self.node_pos()), vec![statement]);
            eof = self.parse_expected_token(Kind::EndOfFile);
        }
        let node = self.arena.new_source_file(
            &self.opts.file_name,
            self.script_kind,
            self.language_variant,
            statements,
            eof,
        );
        let node = self.finish_node(node, pos);
        // Validate the single top-level JSON value, if present.
        let first_expr = match self.arena.data(node) {
            tsgo_ast::NodeData::SourceFile(d) => {
                d.statements
                    .nodes
                    .first()
                    .and_then(|&s| match self.arena.data(s) {
                        tsgo_ast::NodeData::ExpressionStatement(e) => Some(e.expression),
                        _ => None,
                    })
            }
            _ => None,
        };
        if let Some(expr) = first_expr {
            self.validate_json_value(node, expr);
        }
        let is_declaration_file = tsgo_tspath::is_declaration_file_name(&self.opts.file_name);
        self.set_is_declaration_file(node, is_declaration_file);
        self.finish_source_file(node);
        ParseResult {
            arena: std::mem::take(&mut self.arena),
            source_file: node,
            diagnostics: std::mem::take(&mut self.diagnostics),
        }
    }

    // Go: internal/parser/parser.go:getErrorSpanForNode
    fn get_error_span_for_node(&self, node: NodeId) -> TextRange {
        let loc = self.arena.loc(node);
        let pos = tsgo_scanner::skip_trivia(&self.source_text, loc.pos());
        TextRange::new(pos, loc.end())
    }

    // Go: internal/parser/parser.go:isDoubleQuotedString
    fn is_double_quoted_string(&self, node: NodeId) -> bool {
        match self.arena.data(node) {
            tsgo_ast::NodeData::StringLiteral(d) => {
                !d.token_flags.contains(tsgo_ast::TokenFlags::SINGLE_QUOTE)
            }
            _ => false,
        }
    }

    // Go: internal/parser/parser.go:validateJsonValue
    fn validate_json_value(&mut self, source_file: NodeId, value_expression: NodeId) {
        match self.arena.kind(value_expression) {
            Kind::TrueKeyword | Kind::FalseKeyword | Kind::NullKeyword | Kind::NumericLiteral => {
                return
            }
            Kind::StringLiteral => {
                if !self.is_double_quoted_string(value_expression) {
                    let span = self.get_error_span_for_node(value_expression);
                    self.parse_error_at(
                        span.pos(),
                        span.end(),
                        &diagnostics::STRING_LITERAL_WITH_DOUBLE_QUOTES_EXPECTED,
                        Vec::new(),
                    );
                }
                return;
            }
            Kind::PrefixUnaryExpression => {
                let valid = match self.arena.data(value_expression) {
                    tsgo_ast::NodeData::PrefixUnaryExpression(d) => {
                        d.operator == Kind::MinusToken
                            && self.arena.kind(d.operand) == Kind::NumericLiteral
                    }
                    _ => false,
                };
                if valid {
                    return;
                }
            }
            Kind::ObjectLiteralExpression => {
                self.validate_json_object_literal(source_file, value_expression);
                return;
            }
            Kind::ArrayLiteralExpression => {
                let elements = match self.arena.data(value_expression) {
                    tsgo_ast::NodeData::ArrayLiteralExpression(d) => d.list.nodes.clone(),
                    _ => Vec::new(),
                };
                for element in elements {
                    self.validate_json_value(source_file, element);
                }
                return;
            }
            _ => {}
        }
        let span = self.get_error_span_for_node(value_expression);
        self.parse_error_at(
            span.pos(),
            span.end(),
            &diagnostics::PROPERTY_VALUE_CAN_ONLY_BE_STRING_LITERAL_NUMERIC_LITERAL_TRUE_FALSE_NULL_OBJECT_LITERAL_OR_ARRAY_LITERAL,
            Vec::new(),
        );
    }

    // Go: internal/parser/parser.go:validateJsonObjectLiteral
    fn validate_json_object_literal(&mut self, source_file: NodeId, node: NodeId) {
        let properties = match self.arena.data(node) {
            tsgo_ast::NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
            _ => Vec::new(),
        };
        for element in properties {
            if self.arena.kind(element) != Kind::PropertyAssignment {
                let span = self.get_error_span_for_node(element);
                self.parse_error_at(
                    span.pos(),
                    span.end(),
                    &diagnostics::PROPERTY_ASSIGNMENT_EXPECTED,
                    Vec::new(),
                );
                continue;
            }
            let (name, initializer) = match self.arena.data(element) {
                tsgo_ast::NodeData::PropertyAssignment(d) => (Some(d.name), d.initializer),
                _ => (None, None),
            };
            if let Some(name) = name {
                if !self.is_double_quoted_string(name) {
                    let span = self.get_error_span_for_node(name);
                    self.parse_error_at(
                        span.pos(),
                        span.end(),
                        &diagnostics::STRING_LITERAL_WITH_DOUBLE_QUOTES_EXPECTED,
                        Vec::new(),
                    );
                }
            }
            if let Some(initializer) = initializer {
                self.validate_json_value(source_file, initializer);
            }
        }
    }

    // ---- token stream + diagnostics ----

    // Go: internal/parser/parser.go:nextToken
    fn next_token(&mut self) -> Kind {
        if is_keyword(self.token)
            && (self.scanner.has_unicode_escape() || self.scanner.has_extended_unicode_escape())
        {
            self.parse_error_at_current_token(
                &diagnostics::KEYWORDS_CANNOT_CONTAIN_ESCAPE_CHARACTERS,
                Vec::new(),
            );
        }
        self.next_token_without_check()
    }

    // Go: internal/parser/parser.go:nextTokenWithoutCheck
    fn next_token_without_check(&mut self) -> Kind {
        self.token = self.scanner.scan();
        self.drain_scan_errors();
        self.token
    }

    /// Drains buffered scanner errors into the diagnostics list, mirroring Go's
    /// synchronous `scanError` callback.
    fn drain_scan_errors(&mut self) {
        let errors = std::mem::take(&mut *self.scan_errors.borrow_mut());
        for e in errors {
            self.parse_error_at_range(
                TextRange::new(e.pos, e.pos + e.length),
                e.message,
                Vec::new(),
            );
        }
    }

    // Go: internal/parser/parser.go:nodePos
    fn node_pos(&self) -> i32 {
        self.scanner.token_full_start()
    }

    // Go: internal/parser/parser.go:hasPrecedingLineBreak
    fn has_preceding_line_break(&self) -> bool {
        self.scanner.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:parseErrorAt
    fn parse_error_at(&mut self, pos: i32, end: i32, message: &'static Message, args: Vec<String>) {
        self.parse_error_at_range(TextRange::new(pos, end), message, args);
    }

    // Go: internal/parser/parser.go:parseErrorAtCurrentToken
    fn parse_error_at_current_token(&mut self, message: &'static Message, args: Vec<String>) {
        self.parse_error_at_range(self.scanner.token_range(), message, args);
    }

    // Go: internal/parser/parser.go:parseErrorAtRange
    fn parse_error_at_range(
        &mut self,
        loc: TextRange,
        message: &'static Message,
        args: Vec<String>,
    ) {
        // Don't report another error at the same location as the previous one.
        if self.diagnostics.last().is_none_or(|d| d.pos() != loc.pos()) {
            self.diagnostics.push(Diagnostic { loc, message, args });
        }
        self.has_parse_error = true;
    }

    // ---- speculation ----

    // Go: internal/parser/parser.go:mark
    fn mark(&self) -> ParserState {
        ParserState {
            scanner_state: self.scanner.mark(),
            context_flags: self.context_flags,
            diagnostics_len: self.diagnostics.len(),
            has_parse_error: self.has_parse_error,
        }
    }

    // Go: internal/parser/parser.go:rewind
    fn rewind(&mut self, state: ParserState) {
        self.scanner.rewind(state.scanner_state);
        self.token = self.scanner.token();
        self.context_flags = state.context_flags;
        self.diagnostics.truncate(state.diagnostics_len);
        self.has_parse_error = state.has_parse_error;
    }

    // Go: internal/parser/parser.go:lookAhead (always rewinds)
    fn look_ahead<R>(&mut self, callback: impl FnOnce(&mut Parser) -> R) -> R {
        let state = self.mark();
        let result = callback(self);
        self.rewind(state);
        result
    }

    // ---- token helpers ----

    // Go: internal/parser/parser.go:parseOptional
    fn parse_optional(&mut self, token: Kind) -> bool {
        if self.token == token {
            self.next_token();
            true
        } else {
            false
        }
    }

    // Go: internal/parser/parser.go:parseExpected
    fn parse_expected(&mut self, kind: Kind) -> bool {
        self.parse_expected_with_diagnostic(kind, None, true)
    }

    // Go: internal/parser/parser.go:parseExpectedWithDiagnostic
    fn parse_expected_with_diagnostic(
        &mut self,
        kind: Kind,
        message: Option<&'static Message>,
        should_advance: bool,
    ) -> bool {
        if self.token == kind {
            if should_advance {
                self.next_token();
            }
            return true;
        }
        match message {
            Some(m) => self.parse_error_at_current_token(m, Vec::new()),
            None => self.parse_error_at_current_token(
                &diagnostics::X_0_EXPECTED,
                vec![tsgo_scanner::token_to_string(kind).to_string()],
            ),
        }
        false
    }

    // Go: internal/parser/parser.go:parseTokenNode
    fn parse_token_node(&mut self) -> NodeId {
        let pos = self.node_pos();
        let kind = self.token;
        self.next_token();
        let node = self.arena.new_token(kind);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseOptionalToken
    fn parse_optional_token(&mut self, kind: Kind) -> Option<NodeId> {
        if self.token == kind {
            Some(self.parse_token_node())
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseExpectedToken
    fn parse_expected_token(&mut self, kind: Kind) -> NodeId {
        match self.parse_optional_token(kind) {
            Some(token) => token,
            None => {
                self.parse_error_at_current_token(
                    &diagnostics::X_0_EXPECTED,
                    vec![tsgo_scanner::token_to_string(kind).to_string()],
                );
                let pos = self.node_pos();
                let node = self.arena.new_token(kind);
                self.finish_node_with_end(node, pos, pos)
            }
        }
    }

    // ---- node finishing ----

    // Go: internal/parser/parser.go:newNodeList
    fn new_node_list(&mut self, loc: TextRange, nodes: Vec<NodeId>) -> NodeList {
        NodeList { loc, nodes }
    }

    // Go: internal/parser/parser.go:finishNode
    fn finish_node(&mut self, id: NodeId, pos: i32) -> NodeId {
        let end = self.node_pos();
        self.finish_node_with_end(id, pos, end)
    }

    // Go: internal/parser/parser.go:finishNodeWithEnd
    fn finish_node_with_end(&mut self, id: NodeId, pos: i32, end: i32) -> NodeId {
        self.arena.set_loc(id, TextRange::new(pos, end));
        self.arena.add_flags(id, self.context_flags);
        if self.has_parse_error {
            self.arena.add_flags(id, NodeFlags::THIS_NODE_HAS_ERROR);
            self.has_parse_error = false;
        }
        self.override_parent_in_immediate_children(id);
        id
    }

    // Go: internal/parser/parser.go:overrideParentInImmediateChildren
    fn override_parent_in_immediate_children(&mut self, id: NodeId) {
        let mut children = Vec::new();
        self.arena.for_each_child(id, &mut |c| {
            children.push(c);
            false
        });
        for c in children {
            self.arena.set_parent(c, Some(id));
        }
    }

    // ---- list parsing ----

    // Go: internal/parser/parser.go:parseListIndex
    fn parse_list_index(
        &mut self,
        kind: ParsingContext,
        mut parse_element: impl FnMut(&mut Parser) -> NodeId,
    ) -> Vec<NodeId> {
        let save_parsing_contexts = self.parsing_contexts;
        self.parsing_contexts |= 1 << (kind as u32);
        let mut list = Vec::new();
        while !self.is_list_terminator(kind) {
            if self.is_list_element(kind, false) {
                let elt = parse_element(self);
                list.push(elt);
                continue;
            }
            if self.abort_parsing_list_or_move_to_next_token(kind) {
                break;
            }
        }
        self.parsing_contexts = save_parsing_contexts;
        list
    }

    // Go: internal/parser/parser.go:parseDelimitedList (argument/array subset)
    fn parse_delimited_list(
        &mut self,
        kind: ParsingContext,
        mut parse_element: impl FnMut(&mut Parser) -> NodeId,
    ) -> NodeList {
        self.parse_delimited_list_opt(kind, |p| Some(parse_element(p)))
            .expect("non-optional delimited list element must not fail")
    }

    // Go: internal/parser/parser.go:parseDelimitedList
    //
    // Returns `None` when `parse_element` returns `None` (used by speculative
    // parameter parsing where an ambiguous element aborts the whole list).
    fn parse_delimited_list_opt(
        &mut self,
        kind: ParsingContext,
        mut parse_element: impl FnMut(&mut Parser) -> Option<NodeId>,
    ) -> Option<NodeList> {
        let pos = self.node_pos();
        let save_parsing_contexts = self.parsing_contexts;
        self.parsing_contexts |= 1 << (kind as u32);
        let mut list = Vec::new();
        loop {
            if self.is_list_element(kind, false) {
                let start_pos = self.node_pos();
                match parse_element(self) {
                    Some(element) => list.push(element),
                    None => {
                        self.parsing_contexts = save_parsing_contexts;
                        return None;
                    }
                }
                if self.parse_optional(Kind::CommaToken) {
                    continue;
                }
                if self.is_list_terminator(kind) {
                    break;
                }
                self.parse_expected(Kind::CommaToken);
                // Recover from a stray semicolon between object/import-attribute members.
                if (kind == ParsingContext::ObjectLiteralMembers
                    || kind == ParsingContext::ImportAttributes)
                    && self.token == Kind::SemicolonToken
                    && !self.has_preceding_line_break()
                {
                    self.next_token();
                }
                if start_pos == self.node_pos() {
                    self.next_token();
                }
                continue;
            }
            if self.is_list_terminator(kind) {
                break;
            }
            if self.abort_parsing_list_or_move_to_next_token(kind) {
                break;
            }
        }
        self.parsing_contexts = save_parsing_contexts;
        let end = self.node_pos();
        Some(self.new_node_list(TextRange::new(pos, end), list))
    }

    // Go: internal/parser/parser.go:abortParsingListOrMoveToNextToken (simplified)
    fn abort_parsing_list_or_move_to_next_token(&mut self, kind: ParsingContext) -> bool {
        self.parsing_context_errors(kind);
        if self.is_in_some_parsing_context() {
            return true;
        }
        self.next_token();
        false
    }

    // Go: internal/parser/parser.go:parsingContextErrors (subset)
    fn parsing_context_errors(&mut self, _kind: ParsingContext) {
        // The full Go version emits a context-specific message; for the parser
        // slice ported so far the generic "Declaration or statement expected"
        // covers the reachable source/block-statement contexts.
        self.parse_error_at_current_token(
            &diagnostics::DECLARATION_OR_STATEMENT_EXPECTED,
            Vec::new(),
        );
    }

    // Go: internal/parser/parser.go:isInSomeParsingContext (subset)
    fn is_in_some_parsing_context(&mut self) -> bool {
        // We only consult the contexts the current slice can be inside of.
        for ctx in [
            ParsingContext::SourceElements,
            ParsingContext::BlockStatements,
            ParsingContext::SwitchClauses,
            ParsingContext::SwitchClauseStatements,
            ParsingContext::ArgumentExpressions,
            ParsingContext::ArrayLiteralMembers,
            ParsingContext::VariableDeclarations,
            ParsingContext::ObjectBindingElements,
            ParsingContext::ArrayBindingElements,
            ParsingContext::TypeArguments,
            ParsingContext::TypeParameters,
            ParsingContext::Parameters,
            ParsingContext::ClassMembers,
            ParsingContext::HeritageClauses,
            ParsingContext::HeritageClauseElement,
            ParsingContext::TypeMembers,
            ParsingContext::EnumMembers,
            ParsingContext::ImportOrExportSpecifiers,
            ParsingContext::ImportAttributes,
            ParsingContext::ObjectLiteralMembers,
            ParsingContext::TupleElementTypes,
            ParsingContext::JsxAttributes,
            ParsingContext::JsxChildren,
        ] {
            if self.parsing_contexts & (1 << (ctx as u32)) != 0
                && (self.is_list_element(ctx, true) || self.is_list_terminator(ctx))
            {
                return true;
            }
        }
        false
    }

    // Go: internal/parser/parser.go:isListElement (subset)
    fn is_list_element(
        &mut self,
        parsing_context: ParsingContext,
        in_error_recovery: bool,
    ) -> bool {
        match parsing_context {
            ParsingContext::SourceElements
            | ParsingContext::BlockStatements
            | ParsingContext::SwitchClauseStatements => {
                !(self.token == Kind::SemicolonToken && in_error_recovery)
                    && self.is_start_of_statement()
            }
            ParsingContext::SwitchClauses => {
                self.token == Kind::CaseKeyword || self.token == Kind::DefaultKeyword
            }
            ParsingContext::ArrayLiteralMembers => {
                if self.token == Kind::CommaToken || self.token == Kind::DotToken {
                    return true;
                }
                self.token == Kind::DotDotDotToken || self.is_start_of_expression()
            }
            ParsingContext::ArgumentExpressions => {
                self.token == Kind::DotDotDotToken || self.is_start_of_expression()
            }
            ParsingContext::TypeArguments | ParsingContext::TupleElementTypes => {
                self.token == Kind::CommaToken || self.is_start_of_type()
            }
            ParsingContext::VariableDeclarations => {
                self.is_binding_identifier_or_private_identifier_or_pattern()
            }
            ParsingContext::ObjectBindingElements => {
                self.token == Kind::OpenBracketToken
                    || self.token == Kind::DotDotDotToken
                    || self.is_literal_property_name()
            }
            ParsingContext::ArrayBindingElements => {
                self.token == Kind::CommaToken
                    || self.token == Kind::DotDotDotToken
                    || self.is_binding_identifier_or_private_identifier_or_pattern()
            }
            ParsingContext::Parameters => self.is_start_of_parameter(),
            ParsingContext::TypeParameters => {
                self.token == Kind::InKeyword
                    || self.token == Kind::ConstKeyword
                    || self.is_identifier()
            }
            ParsingContext::ClassMembers => {
                self.look_ahead(|p| p.scan_class_member_start())
                    || (self.token == Kind::SemicolonToken && !in_error_recovery)
            }
            ParsingContext::HeritageClauses => self.is_heritage_clause(),
            ParsingContext::HeritageClauseElement => {
                if self.token == Kind::OpenBraceToken {
                    return self.is_valid_heritage_clause_object_literal();
                }
                if !in_error_recovery {
                    self.is_start_of_left_hand_side_expression()
                        && !self.is_heritage_clause_extends_or_implements_keyword()
                } else {
                    self.is_identifier() && !self.is_heritage_clause_extends_or_implements_keyword()
                }
            }
            ParsingContext::TypeMembers => self.look_ahead(|p| p.scan_type_member_start()),
            ParsingContext::EnumMembers => {
                self.token == Kind::OpenBracketToken || self.is_literal_property_name()
            }
            ParsingContext::ImportOrExportSpecifiers | ParsingContext::ImportAttributes => {
                self.token == Kind::StringLiteral || token_is_identifier_or_keyword(self.token)
            }
            ParsingContext::ObjectLiteralMembers => match self.token {
                Kind::OpenBracketToken
                | Kind::AsteriskToken
                | Kind::DotDotDotToken
                | Kind::DotToken => true,
                _ => self.is_literal_property_name(),
            },
            ParsingContext::JsxAttributes => {
                token_is_identifier_or_keyword(self.token) || self.token == Kind::OpenBraceToken
            }
            ParsingContext::JsxChildren => true,
            _ => panic!("Unhandled case in is_list_element"),
        }
    }

    // Go: internal/parser/parser.go:isListTerminator (subset)
    fn is_list_terminator(&mut self, kind: ParsingContext) -> bool {
        if self.token == Kind::EndOfFile {
            return true;
        }
        match kind {
            ParsingContext::SourceElements => false,
            ParsingContext::BlockStatements | ParsingContext::SwitchClauses => {
                self.token == Kind::CloseBraceToken
            }
            ParsingContext::SwitchClauseStatements => {
                self.token == Kind::CloseBraceToken
                    || self.token == Kind::CaseKeyword
                    || self.token == Kind::DefaultKeyword
            }
            ParsingContext::ArrayLiteralMembers => self.token == Kind::CloseBracketToken,
            ParsingContext::ArgumentExpressions => {
                self.token == Kind::CloseParenToken || self.token == Kind::SemicolonToken
            }
            // Tokens other than ',' terminate a type-argument list.
            ParsingContext::TypeArguments => self.token != Kind::CommaToken,
            ParsingContext::VariableDeclarations => {
                self.can_parse_semicolon()
                    || self.token == Kind::InKeyword
                    || self.token == Kind::OfKeyword
                    || self.token == Kind::EqualsGreaterThanToken
            }
            ParsingContext::ObjectBindingElements => self.token == Kind::CloseBraceToken,
            ParsingContext::ArrayBindingElements | ParsingContext::TupleElementTypes => {
                self.token == Kind::CloseBracketToken
            }
            ParsingContext::Parameters => {
                self.token == Kind::CloseParenToken || self.token == Kind::CloseBracketToken
            }
            ParsingContext::TypeParameters => {
                self.token == Kind::GreaterThanToken
                    || self.token == Kind::OpenParenToken
                    || self.token == Kind::OpenBraceToken
                    || self.token == Kind::ExtendsKeyword
                    || self.token == Kind::ImplementsKeyword
            }
            ParsingContext::ClassMembers => self.token == Kind::CloseBraceToken,
            ParsingContext::HeritageClauses => {
                self.token == Kind::OpenBraceToken || self.token == Kind::CloseBraceToken
            }
            ParsingContext::HeritageClauseElement => {
                self.token == Kind::OpenBraceToken
                    || self.token == Kind::ExtendsKeyword
                    || self.token == Kind::ImplementsKeyword
            }
            ParsingContext::TypeMembers
            | ParsingContext::EnumMembers
            | ParsingContext::ImportOrExportSpecifiers
            | ParsingContext::ImportAttributes
            | ParsingContext::ObjectLiteralMembers => self.token == Kind::CloseBraceToken,
            ParsingContext::JsxAttributes => {
                self.token == Kind::GreaterThanToken || self.token == Kind::SlashToken
            }
            ParsingContext::JsxChildren => {
                self.token == Kind::LessThanToken && self.look_ahead(|p| p.next_token_is_slash())
            }
            _ => false,
        }
    }

    // Go: internal/parser/parser.go:nextTokenIsSlash
    fn next_token_is_slash(&mut self) -> bool {
        self.next_token() == Kind::SlashToken
    }

    // ---- statements ----

    // Go: internal/parser/parser.go:parseToplevelStatement
    fn parse_toplevel_statement(&mut self) -> NodeId {
        self.parse_statement()
    }

    // Go: internal/parser/parser.go:parseStatement (subset)
    fn parse_statement(&mut self) -> NodeId {
        let token = self.token;
        match token {
            Kind::AtToken => self.parse_declaration(),
            Kind::SemicolonToken => self.parse_empty_statement(),
            Kind::OpenBraceToken => self.parse_block(false, None),
            Kind::VarKeyword => {
                let pos = self.node_pos();
                self.parse_variable_statement(pos, None)
            }
            Kind::LetKeyword if self.is_let_declaration() => {
                let pos = self.node_pos();
                self.parse_variable_statement(pos, None)
            }
            Kind::FunctionKeyword => {
                let pos = self.node_pos();
                self.parse_function_declaration(pos, None)
            }
            Kind::ClassKeyword => {
                let pos = self.node_pos();
                self.parse_class_declaration(pos, None)
            }
            Kind::ForKeyword => self.parse_for_or_for_in_or_for_of_statement(),
            Kind::TryKeyword | Kind::CatchKeyword | Kind::FinallyKeyword => {
                self.parse_try_statement()
            }
            Kind::ConstKeyword
            | Kind::ExportKeyword
            | Kind::AsyncKeyword
            | Kind::DeclareKeyword
            | Kind::InterfaceKeyword
            | Kind::TypeKeyword
            | Kind::EnumKeyword
            | Kind::ModuleKeyword
            | Kind::NamespaceKeyword
            | Kind::GlobalKeyword
            | Kind::ImportKeyword
                if self.is_start_of_declaration() =>
            {
                self.parse_declaration()
            }
            Kind::IfKeyword => self.parse_if_statement(),
            Kind::DoKeyword => self.parse_do_statement(),
            Kind::WhileKeyword => self.parse_while_statement(),
            Kind::ContinueKeyword => self.parse_continue_statement(),
            Kind::BreakKeyword => self.parse_break_statement(),
            Kind::ReturnKeyword => self.parse_return_statement(),
            Kind::WithKeyword => self.parse_with_statement(),
            Kind::SwitchKeyword => self.parse_switch_statement(),
            Kind::ThrowKeyword => self.parse_throw_statement(),
            Kind::DebuggerKeyword => self.parse_debugger_statement(),
            // DEFER(phase-3): for/try and declaration-keyword statements land in
            // later slices (variables, declarations).
            // blocked-by: variable declaration list + declaration parser.
            _ => self.parse_expression_or_labeled_statement(),
        }
    }

    // Go: internal/parser/parser.go:parseBlock (subset)
    fn parse_block(
        &mut self,
        ignore_missing_open_brace: bool,
        diagnostic: Option<&'static Message>,
    ) -> NodeId {
        let pos = self.node_pos();
        let open_brace_position = self.scanner.token_start();
        let open_brace_parsed =
            self.parse_expected_with_diagnostic(Kind::OpenBraceToken, diagnostic, true);
        if open_brace_parsed || ignore_missing_open_brace {
            let statements =
                self.parse_list(ParsingContext::BlockStatements, |p| p.parse_statement());
            self.parse_expected_matching_brackets(
                Kind::OpenBraceToken,
                Kind::CloseBraceToken,
                open_brace_parsed,
                open_brace_position,
            );
            let node = self.arena.new_block(statements);
            return self.finish_node(node, pos);
        }
        let empty =
            self.new_node_list(TextRange::new(self.node_pos(), self.node_pos()), Vec::new());
        let node = self.arena.new_block(empty);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseIfStatement
    fn parse_if_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::IfKeyword);
        let open_paren_position = self.scanner.token_start();
        let open_paren_parsed = self.parse_expected(Kind::OpenParenToken);
        let expression = self.parse_expression_allow_in();
        self.parse_expected_matching_brackets(
            Kind::OpenParenToken,
            Kind::CloseParenToken,
            open_paren_parsed,
            open_paren_position,
        );
        let then_statement = self.parse_statement();
        let else_statement = if self.parse_optional(Kind::ElseKeyword) {
            Some(self.parse_statement())
        } else {
            None
        };
        let node = self
            .arena
            .new_if_statement(expression, then_statement, else_statement);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseDoStatement
    fn parse_do_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::DoKeyword);
        let statement = self.parse_statement();
        self.parse_expected(Kind::WhileKeyword);
        let open_paren_position = self.scanner.token_start();
        let open_paren_parsed = self.parse_expected(Kind::OpenParenToken);
        let expression = self.parse_expression_allow_in();
        self.parse_expected_matching_brackets(
            Kind::OpenParenToken,
            Kind::CloseParenToken,
            open_paren_parsed,
            open_paren_position,
        );
        self.parse_optional(Kind::SemicolonToken);
        let node = self.arena.new_do_statement(statement, expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseWhileStatement
    fn parse_while_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::WhileKeyword);
        let open_paren_position = self.scanner.token_start();
        let open_paren_parsed = self.parse_expected(Kind::OpenParenToken);
        let expression = self.parse_expression_allow_in();
        self.parse_expected_matching_brackets(
            Kind::OpenParenToken,
            Kind::CloseParenToken,
            open_paren_parsed,
            open_paren_position,
        );
        let statement = self.parse_statement();
        let node = self.arena.new_while_statement(expression, statement);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseWithStatement
    fn parse_with_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::WithKeyword);
        let open_paren_position = self.scanner.token_start();
        let open_paren_parsed = self.parse_expected(Kind::OpenParenToken);
        let expression = self.parse_expression_allow_in();
        self.parse_expected_matching_brackets(
            Kind::OpenParenToken,
            Kind::CloseParenToken,
            open_paren_parsed,
            open_paren_position,
        );
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::IN_WITH_STATEMENT, true);
        let statement = self.parse_statement();
        self.context_flags = save;
        let node = self.arena.new_with_statement(expression, statement);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseSwitchStatement
    fn parse_switch_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::SwitchKeyword);
        self.parse_expected(Kind::OpenParenToken);
        let expression = self.parse_expression_allow_in();
        self.parse_expected(Kind::CloseParenToken);
        let case_block = self.parse_case_block();
        let node = self.arena.new_switch_statement(expression, case_block);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseCaseBlock
    fn parse_case_block(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBraceToken);
        let clauses = self.parse_list(ParsingContext::SwitchClauses, |p| {
            p.parse_case_or_default_clause()
        });
        self.parse_expected(Kind::CloseBraceToken);
        let node = self.arena.new_case_block(clauses);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseCaseOrDefaultClause
    fn parse_case_or_default_clause(&mut self) -> NodeId {
        if self.token == Kind::CaseKeyword {
            self.parse_case_clause()
        } else {
            self.parse_default_clause()
        }
    }

    // Go: internal/parser/parser.go:parseCaseClause
    fn parse_case_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::CaseKeyword);
        let expression = self.parse_expression_allow_in();
        self.parse_expected(Kind::ColonToken);
        let statements = self.parse_list(ParsingContext::SwitchClauseStatements, |p| {
            p.parse_statement()
        });
        let node =
            self.arena
                .new_case_or_default_clause(Kind::CaseClause, Some(expression), statements);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseDefaultClause
    fn parse_default_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::DefaultKeyword);
        self.parse_expected(Kind::ColonToken);
        let statements = self.parse_list(ParsingContext::SwitchClauseStatements, |p| {
            p.parse_statement()
        });
        let node = self
            .arena
            .new_case_or_default_clause(Kind::DefaultClause, None, statements);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseThrowStatement
    fn parse_throw_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::ThrowKeyword);
        let expression = if !self.has_preceding_line_break() {
            self.parse_expression_allow_in()
        } else {
            self.create_missing_identifier()
        };
        if !self.try_parse_semicolon() {
            self.parse_error_for_missing_semicolon_after();
        }
        let node = self.arena.new_throw_statement(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseReturnStatement
    fn parse_return_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::ReturnKeyword);
        let expression = if !self.can_parse_semicolon() {
            Some(self.parse_expression_allow_in())
        } else {
            None
        };
        self.parse_semicolon();
        let node = self.arena.new_return_statement(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseBreakStatement
    fn parse_break_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::BreakKeyword);
        let label = self.parse_identifier_unless_at_semicolon();
        self.parse_semicolon();
        let node = self.arena.new_break_statement(label);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseContinueStatement
    fn parse_continue_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::ContinueKeyword);
        let label = self.parse_identifier_unless_at_semicolon();
        self.parse_semicolon();
        let node = self.arena.new_continue_statement(label);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseIdentifierUnlessAtSemicolon
    fn parse_identifier_unless_at_semicolon(&mut self) -> Option<NodeId> {
        if !self.can_parse_semicolon() {
            Some(self.parse_identifier())
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseDebuggerStatement
    fn parse_debugger_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::DebuggerKeyword);
        self.parse_semicolon();
        let node = self.arena.new_debugger_statement();
        self.finish_node(node, pos)
    }

    // ---- variables / binding patterns / for / try ----

    // Go: internal/parser/parser.go:isLetDeclaration
    fn is_let_declaration(&mut self) -> bool {
        self.look_ahead(|p| {
            p.next_token();
            p.is_binding_identifier()
                || p.token == Kind::OpenBraceToken
                || p.token == Kind::OpenBracketToken
        })
    }

    // Go: internal/parser/parser.go:parseVariableStatement
    fn parse_variable_statement(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        let declaration_list = self.parse_variable_declaration_list(false);
        self.parse_semicolon();
        let node = self
            .arena
            .new_variable_statement(modifiers, declaration_list);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseVariableDeclarationList
    fn parse_variable_declaration_list(&mut self, in_for_statement_initializer: bool) -> NodeId {
        let pos = self.node_pos();
        let flags = match self.token {
            Kind::LetKeyword => NodeFlags::LET,
            Kind::ConstKeyword => NodeFlags::CONST,
            Kind::UsingKeyword => NodeFlags::USING,
            // DEFER(phase-3): `await using` declaration lists.
            // blocked-by: await-using lookahead.
            _ => NodeFlags::NONE,
        };
        self.next_token();
        let declarations = if self.token == Kind::OfKeyword
            && self.look_ahead(|p| p.next_is_identifier_and_close_paren())
        {
            // `for (let of x)`: empty declaration list, `of` is the loop keyword.
            self.new_node_list(TextRange::new(self.node_pos(), self.node_pos()), Vec::new())
        } else {
            let save = self.context_flags;
            self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, in_for_statement_initializer);
            let result = if in_for_statement_initializer {
                self.parse_delimited_list(ParsingContext::VariableDeclarations, |p| {
                    p.parse_variable_declaration()
                })
            } else {
                self.parse_delimited_list(ParsingContext::VariableDeclarations, |p| {
                    p.parse_variable_declaration_allow_exclamation()
                })
            };
            self.context_flags = save;
            result
        };
        let node = self.arena.new_variable_declaration_list(declarations);
        self.arena.add_flags(node, flags);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseVariableDeclaration
    fn parse_variable_declaration(&mut self) -> NodeId {
        self.parse_variable_declaration_worker(false)
    }

    // Go: internal/parser/parser.go:parseVariableDeclarationAllowExclamation
    fn parse_variable_declaration_allow_exclamation(&mut self) -> NodeId {
        self.parse_variable_declaration_worker(true)
    }

    // Go: internal/parser/parser.go:parseVariableDeclarationWorker
    fn parse_variable_declaration_worker(&mut self, allow_exclamation: bool) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_identifier_or_pattern();
        let exclamation_token = if allow_exclamation
            && self.arena.kind(name) == Kind::Identifier
            && self.token == Kind::ExclamationToken
            && !self.has_preceding_line_break()
        {
            Some(self.parse_token_node())
        } else {
            None
        };
        let type_node = self.parse_type_annotation();
        let initializer = if self.token != Kind::InKeyword && self.token != Kind::OfKeyword {
            self.parse_initializer()
        } else {
            None
        };
        let node =
            self.arena
                .new_variable_declaration(name, exclamation_token, type_node, initializer);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseInitializer
    fn parse_initializer(&mut self) -> Option<NodeId> {
        if self.parse_optional(Kind::EqualsToken) {
            Some(self.parse_assignment_expression_or_higher())
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseIdentifierOrPattern
    fn parse_identifier_or_pattern(&mut self) -> NodeId {
        match self.token {
            Kind::OpenBracketToken => self.parse_array_binding_pattern(),
            Kind::OpenBraceToken => self.parse_object_binding_pattern(),
            _ => self.parse_binding_identifier(),
        }
    }

    // Go: internal/parser/parser.go:parseBindingIdentifier
    fn parse_binding_identifier(&mut self) -> NodeId {
        let is_identifier = self.is_binding_identifier();
        self.create_identifier_with_diagnostic(is_identifier, None)
    }

    // Go: internal/parser/parser.go:parseArrayBindingPattern
    fn parse_array_binding_pattern(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBracketToken);
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
        let elements = self.parse_delimited_list(ParsingContext::ArrayBindingElements, |p| {
            p.parse_array_binding_element()
        });
        self.context_flags = save;
        self.parse_expected(Kind::CloseBracketToken);
        let node = self
            .arena
            .new_binding_pattern(Kind::ArrayBindingPattern, elements);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseArrayBindingElement
    fn parse_array_binding_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        let (dot_dot_dot_token, name, initializer) = if self.token != Kind::CommaToken {
            let dot = self.parse_optional_token(Kind::DotDotDotToken);
            let name = self.parse_identifier_or_pattern();
            let init = self.parse_initializer();
            (dot, Some(name), init)
        } else {
            (None, None, None)
        };
        let node = self
            .arena
            .new_binding_element(dot_dot_dot_token, None, name, initializer);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseObjectBindingPattern
    fn parse_object_binding_pattern(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBraceToken);
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
        let elements = self.parse_delimited_list(ParsingContext::ObjectBindingElements, |p| {
            p.parse_object_binding_element()
        });
        self.context_flags = save;
        self.parse_expected(Kind::CloseBraceToken);
        let node = self
            .arena
            .new_binding_pattern(Kind::ObjectBindingPattern, elements);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseObjectBindingElement
    fn parse_object_binding_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        let dot_dot_dot_token = self.parse_optional_token(Kind::DotDotDotToken);
        let token_is_identifier = self.is_binding_identifier();
        let mut property_name = Some(self.parse_property_name());
        let name = if token_is_identifier && self.token != Kind::ColonToken {
            property_name.take()
        } else {
            self.parse_expected(Kind::ColonToken);
            Some(self.parse_identifier_or_pattern())
        };
        let initializer = self.parse_initializer();
        let node =
            self.arena
                .new_binding_element(dot_dot_dot_token, property_name, name, initializer);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parsePropertyName
    fn parse_property_name(&mut self) -> NodeId {
        self.parse_property_name_worker(true)
    }

    // Go: internal/parser/parser.go:parsePropertyNameWorker
    fn parse_property_name_worker(&mut self, allow_computed_property_names: bool) -> NodeId {
        if self.token == Kind::StringLiteral
            || self.token == Kind::NumericLiteral
            || self.token == Kind::BigIntLiteral
        {
            return self.parse_literal_expression();
        }
        if allow_computed_property_names && self.token == Kind::OpenBracketToken {
            return self.parse_computed_property_name();
        }
        if self.token == Kind::PrivateIdentifier {
            return self.parse_private_identifier();
        }
        self.parse_identifier_name()
    }

    // Go: internal/parser/parser.go:parseComputedPropertyName
    fn parse_computed_property_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBracketToken);
        let expression = self.parse_expression_allow_in();
        self.parse_expected(Kind::CloseBracketToken);
        let node = self.arena.new_computed_property_name(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parsePrivateIdentifier
    fn parse_private_identifier(&mut self) -> NodeId {
        let pos = self.node_pos();
        let text = self.scanner.token_value().to_string();
        self.next_token();
        let node = self.arena.new_private_identifier(&text);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseForOrForInOrForOfStatement
    fn parse_for_or_for_in_or_for_of_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::ForKeyword);
        let await_token = self.parse_optional_token(Kind::AwaitKeyword);
        self.parse_expected(Kind::OpenParenToken);
        let initializer = if self.token != Kind::SemicolonToken {
            if self.token == Kind::VarKeyword
                || self.token == Kind::LetKeyword
                || self.token == Kind::ConstKeyword
            {
                Some(self.parse_variable_declaration_list(true))
            } else {
                let save = self.context_flags;
                self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, true);
                let e = self.parse_expression();
                self.context_flags = save;
                Some(e)
            }
        } else {
            None
        };
        if await_token.is_some() && self.parse_expected(Kind::OfKeyword)
            || await_token.is_none() && self.parse_optional(Kind::OfKeyword)
        {
            let save = self.context_flags;
            self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
            let expression = self.parse_assignment_expression_or_higher();
            self.context_flags = save;
            self.parse_expected(Kind::CloseParenToken);
            let statement = self.parse_statement();
            let node = self.arena.new_for_in_or_of_statement(
                Kind::ForOfStatement,
                await_token,
                initializer.expect("for-of requires an initializer"),
                expression,
                statement,
            );
            self.finish_node(node, pos)
        } else if self.parse_optional(Kind::InKeyword) {
            let expression = self.parse_expression_allow_in();
            self.parse_expected(Kind::CloseParenToken);
            let statement = self.parse_statement();
            let node = self.arena.new_for_in_or_of_statement(
                Kind::ForInStatement,
                None,
                initializer.expect("for-in requires an initializer"),
                expression,
                statement,
            );
            self.finish_node(node, pos)
        } else {
            self.parse_expected(Kind::SemicolonToken);
            let condition =
                if self.token != Kind::SemicolonToken && self.token != Kind::CloseParenToken {
                    Some(self.parse_expression_allow_in())
                } else {
                    None
                };
            self.parse_expected(Kind::SemicolonToken);
            let incrementor = if self.token != Kind::CloseParenToken {
                Some(self.parse_expression_allow_in())
            } else {
                None
            };
            self.parse_expected(Kind::CloseParenToken);
            let statement = self.parse_statement();
            let node = self
                .arena
                .new_for_statement(initializer, condition, incrementor, statement);
            self.finish_node(node, pos)
        }
    }

    // Go: internal/parser/parser.go:parseTryStatement
    fn parse_try_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::TryKeyword);
        let try_block = self.parse_block(false, None);
        let catch_clause = if self.token == Kind::CatchKeyword {
            Some(self.parse_catch_clause())
        } else {
            None
        };
        let finally_block = if catch_clause.is_none() || self.token == Kind::FinallyKeyword {
            self.parse_expected_with_diagnostic(Kind::FinallyKeyword, None, true);
            Some(self.parse_block(false, None))
        } else {
            None
        };
        let node = self
            .arena
            .new_try_statement(try_block, catch_clause, finally_block);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseCatchClause
    fn parse_catch_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::CatchKeyword);
        let variable_declaration = if self.parse_optional(Kind::OpenParenToken) {
            let v = self.parse_variable_declaration();
            self.parse_expected(Kind::CloseParenToken);
            Some(v)
        } else {
            None
        };
        let block = self.parse_block(false, None);
        let node = self.arena.new_catch_clause(variable_declaration, block);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:nextIsIdentifierAndCloseParen
    fn next_is_identifier_and_close_paren(&mut self) -> bool {
        self.next_token_is_identifier() && self.next_token() == Kind::CloseParenToken
    }

    // Go: internal/parser/parser.go:nextTokenIsIdentifier
    fn next_token_is_identifier(&mut self) -> bool {
        self.next_token();
        self.is_identifier()
    }

    // Go: internal/parser/parser.go:parseExpressionAllowIn
    fn parse_expression_allow_in(&mut self) -> NodeId {
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
        let result = self.parse_expression();
        self.context_flags = save;
        result
    }

    // Go: internal/parser/parser.go:parseSemicolon
    fn parse_semicolon(&mut self) -> bool {
        self.try_parse_semicolon() || self.parse_expected(Kind::SemicolonToken)
    }

    // Go: internal/parser/parser.go:parseExpectedMatchingBrackets (subset)
    fn parse_expected_matching_brackets(
        &mut self,
        _open_kind: Kind,
        close_kind: Kind,
        _open_parsed: bool,
        _open_position: i32,
    ) {
        if self.token == close_kind {
            self.next_token();
            return;
        }
        self.parse_error_at_current_token(
            &diagnostics::X_0_EXPECTED,
            vec![tsgo_scanner::token_to_string(close_kind).to_string()],
        );
    }

    // Go: internal/parser/parser.go:parseList
    fn parse_list(
        &mut self,
        kind: ParsingContext,
        parse_element: impl FnMut(&mut Parser) -> NodeId,
    ) -> NodeList {
        let pos = self.node_pos();
        let nodes = self.parse_list_index(kind, parse_element);
        let end = self.node_pos();
        self.new_node_list(TextRange::new(pos, end), nodes)
    }

    // Go: internal/parser/parser.go:parseEmptyStatement
    fn parse_empty_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::SemicolonToken);
        let node = self.arena.new_empty_statement();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseExpressionOrLabeledStatement (subset)
    fn parse_expression_or_labeled_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = self.parse_expression();
        if self.arena.kind(expression) == Kind::Identifier && self.parse_optional(Kind::ColonToken)
        {
            let statement = self.parse_statement();
            let node = self.arena.new_labeled_statement(expression, statement);
            return self.finish_node(node, pos);
        }
        if !self.try_parse_semicolon() {
            self.parse_error_for_missing_semicolon_after();
        }
        let node = self.arena.new_expression_statement(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseErrorForMissingSemicolonAfter (subset)
    fn parse_error_for_missing_semicolon_after(&mut self) {
        self.parse_error_at_current_token(
            &diagnostics::X_0_EXPECTED,
            vec![tsgo_scanner::token_to_string(Kind::SemicolonToken).to_string()],
        );
    }

    // ---- expressions ----

    // Go: internal/parser/parser.go:parseExpression
    fn parse_expression(&mut self) -> NodeId {
        let save_context_flags = self.context_flags;
        self.context_flags &= !NodeFlags::DECORATOR_CONTEXT;
        let pos = self.node_pos();
        let mut expr = self.parse_assignment_expression_or_higher();
        loop {
            let operator_token = self.parse_optional_token(Kind::CommaToken);
            match operator_token {
                None => break,
                Some(op) => {
                    let right = self.parse_assignment_expression_or_higher();
                    expr = self.make_binary_expression(expr, op, right, pos);
                }
            }
        }
        self.context_flags = save_context_flags;
        expr
    }

    // Go: internal/parser/parser.go:parseAssignmentExpressionOrHigher
    fn parse_assignment_expression_or_higher(&mut self) -> NodeId {
        self.parse_assignment_expression_or_higher_worker(true)
    }

    // Go: internal/parser/parser.go:parseAssignmentExpressionOrHigherWorker (subset)
    //
    // DEFER(phase-3): yield expressions and arrow-function productions are not
    // yet ported (blocked-by: parameter/binding-pattern parsing + the type
    // parser). For non-arrow, non-yield input the remaining productions match
    // Go exactly.
    fn parse_assignment_expression_or_higher_worker(
        &mut self,
        allow_return_type_in_arrow_function: bool,
    ) -> NodeId {
        if self.is_yield_expression() {
            return self.parse_yield_expression();
        }
        if let Some(arrow) = self
            .try_parse_parenthesized_arrow_function_expression(allow_return_type_in_arrow_function)
        {
            return arrow;
        }
        if let Some(arrow) = self
            .try_parse_async_simple_arrow_function_expression(allow_return_type_in_arrow_function)
        {
            return arrow;
        }
        let pos = self.node_pos();
        let expr = self.parse_binary_expression_or_higher(tsgo_ast::OperatorPrecedence::LOWEST);
        // Single un-parenthesized parameter arrow: `x => ...`.
        if self.arena.kind(expr) == Kind::Identifier && self.token == Kind::EqualsGreaterThanToken {
            return self.parse_simple_arrow_function_expression(
                pos,
                expr,
                allow_return_type_in_arrow_function,
                None,
            );
        }
        if is_left_hand_side_expression_kind(self.arena.kind(expr))
            && is_assignment_operator(self.re_scan_greater_than_token())
        {
            let op = self.parse_token_node();
            let right = self
                .parse_assignment_expression_or_higher_worker(allow_return_type_in_arrow_function);
            return self.make_binary_expression(expr, op, right, pos);
        }
        self.parse_conditional_expression_rest(expr, pos, allow_return_type_in_arrow_function)
    }

    // Go: internal/parser/parser.go:isParenthesizedArrowFunctionExpression
    fn is_parenthesized_arrow_function_expression(&mut self) -> Tristate {
        match self.token {
            Kind::OpenParenToken | Kind::LessThanToken | Kind::AsyncKeyword => {
                let state = self.mark();
                let result = self.next_is_parenthesized_arrow_function_expression();
                self.rewind(state);
                result
            }
            // ERROR RECOVERY: a standalone `=>` is treated as an arrow.
            Kind::EqualsGreaterThanToken => Tristate::True,
            _ => Tristate::False,
        }
    }

    // Go: internal/parser/parser.go:nextIsParenthesizedArrowFunctionExpression (non-JSX subset)
    fn next_is_parenthesized_arrow_function_expression(&mut self) -> Tristate {
        if self.token == Kind::AsyncKeyword {
            self.next_token();
            if self.has_preceding_line_break() {
                return Tristate::False;
            }
            if self.token != Kind::OpenParenToken && self.token != Kind::LessThanToken {
                return Tristate::False;
            }
        }
        let first = self.token;
        let second = self.next_token();
        if first == Kind::OpenParenToken {
            match second {
                Kind::CloseParenToken => match self.next_token() {
                    Kind::EqualsGreaterThanToken | Kind::ColonToken | Kind::OpenBraceToken => {
                        Tristate::True
                    }
                    _ => Tristate::False,
                },
                Kind::OpenBracketToken | Kind::OpenBraceToken => Tristate::Unknown,
                Kind::DotDotDotToken => Tristate::True,
                _ => {
                    if is_modifier_kind(second)
                        && second != Kind::AsyncKeyword
                        && self.look_ahead(|p| {
                            p.next_token();
                            p.is_identifier()
                        })
                    {
                        if self.next_token() == Kind::AsKeyword {
                            return Tristate::False;
                        }
                        return Tristate::True;
                    }
                    if !self.is_identifier() && second != Kind::ThisKeyword {
                        return Tristate::False;
                    }
                    match self.next_token() {
                        Kind::ColonToken => Tristate::True,
                        Kind::QuestionToken => {
                            self.next_token();
                            if self.token == Kind::ColonToken
                                || self.token == Kind::CommaToken
                                || self.token == Kind::EqualsToken
                                || self.token == Kind::CloseParenToken
                            {
                                Tristate::True
                            } else {
                                Tristate::False
                            }
                        }
                        Kind::CommaToken | Kind::EqualsToken | Kind::CloseParenToken => {
                            Tristate::Unknown
                        }
                        _ => Tristate::False,
                    }
                }
            }
        } else {
            // first == LessThan (generic arrow). JSX overrides are DEFER(phase-3).
            if !self.is_identifier() && self.token != Kind::ConstKeyword {
                return Tristate::False;
            }
            Tristate::Unknown
        }
    }

    // Go: internal/parser/parser.go:tryParseParenthesizedArrowFunctionExpression
    fn try_parse_parenthesized_arrow_function_expression(
        &mut self,
        allow_return_type_in_arrow_function: bool,
    ) -> Option<NodeId> {
        let tristate = self.is_parenthesized_arrow_function_expression();
        if tristate == Tristate::False {
            return None;
        }
        if tristate == Tristate::True {
            return self.parse_parenthesized_arrow_function_expression(true, true);
        }
        let state = self.mark();
        let result = self.parse_parenthesized_arrow_function_expression(
            false,
            allow_return_type_in_arrow_function,
        );
        if result.is_none() {
            self.rewind(state);
        }
        result
    }

    // Go: internal/parser/parser.go:parseParenthesizedArrowFunctionExpression (subset)
    fn parse_parenthesized_arrow_function_expression(
        &mut self,
        allow_ambiguity: bool,
        allow_return_type_in_arrow_function: bool,
    ) -> Option<NodeId> {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers_for_arrow_function();
        let is_async = modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::ASYNC));
        let signature_flags = if is_async {
            ParseFlags::AWAIT
        } else {
            ParseFlags::NONE
        };
        let type_parameters = self.parse_type_parameters();
        let parameters = if !self.parse_expected(Kind::OpenParenToken) {
            if !allow_ambiguity {
                return None;
            }
            self.create_missing_list()
        } else {
            let p = if allow_ambiguity {
                self.parse_parameters_worker(signature_flags, allow_ambiguity)
                    .expect("ambiguous parameter parse always yields a list")
            } else {
                self.parse_parameters_worker(signature_flags, allow_ambiguity)?
            };
            if !self.parse_expected(Kind::CloseParenToken) && !allow_ambiguity {
                return None;
            }
            p
        };
        let has_return_colon = self.token == Kind::ColonToken;
        let return_type = self.parse_return_type(Kind::ColonToken, false);
        // DEFER(phase-3): typeHasArrowFunctionBlockingParseError refinement.
        // blocked-by: full function/constructor type-node parsing.
        if !allow_ambiguity
            && self.token != Kind::EqualsGreaterThanToken
            && self.token != Kind::OpenBraceToken
        {
            return None;
        }
        let last_token = self.token;
        let equals_greater_than_token = self.parse_expected_token(Kind::EqualsGreaterThanToken);
        let body = if last_token == Kind::EqualsGreaterThanToken
            || last_token == Kind::OpenBraceToken
        {
            self.parse_arrow_function_expression_body(is_async, allow_return_type_in_arrow_function)
        } else {
            self.parse_identifier()
        };
        if !allow_return_type_in_arrow_function
            && has_return_colon
            && self.token != Kind::ColonToken
        {
            return None;
        }
        let node = self.arena.new_arrow_function(
            modifiers,
            type_parameters,
            parameters,
            return_type,
            None,
            equals_greater_than_token,
            body,
        );
        Some(self.finish_node(node, pos))
    }

    // Go: internal/parser/parser.go:parseModifiersForArrowFunction
    fn parse_modifiers_for_arrow_function(&mut self) -> Option<ModifierList> {
        if self.token == Kind::AsyncKeyword {
            let pos = self.node_pos();
            self.next_token();
            let modifier = self.arena.new_token(Kind::AsyncKeyword);
            let modifier = self.finish_node(modifier, pos);
            let loc = self.arena.loc(modifier);
            Some(self.new_modifier_list(loc, vec![modifier], ModifierFlags::ASYNC))
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:tryParseAsyncSimpleArrowFunctionExpression
    fn try_parse_async_simple_arrow_function_expression(
        &mut self,
        allow_return_type_in_arrow_function: bool,
    ) -> Option<NodeId> {
        if self.token == Kind::AsyncKeyword
            && self.look_ahead(|p| p.next_is_un_parenthesized_async_arrow_function())
        {
            let pos = self.node_pos();
            let async_modifier = self.parse_modifiers_for_arrow_function();
            let expr = self.parse_binary_expression_or_higher(tsgo_ast::OperatorPrecedence::LOWEST);
            return Some(self.parse_simple_arrow_function_expression(
                pos,
                expr,
                allow_return_type_in_arrow_function,
                async_modifier,
            ));
        }
        None
    }

    // Go: internal/parser/parser.go:nextIsUnParenthesizedAsyncArrowFunction
    fn next_is_un_parenthesized_async_arrow_function(&mut self) -> bool {
        if self.token == Kind::AsyncKeyword {
            self.next_token();
            if self.has_preceding_line_break() || self.token == Kind::EqualsGreaterThanToken {
                return false;
            }
            let expr = self.parse_binary_expression_or_higher(tsgo_ast::OperatorPrecedence::LOWEST);
            if !self.has_preceding_line_break()
                && self.arena.kind(expr) == Kind::Identifier
                && self.token == Kind::EqualsGreaterThanToken
            {
                return true;
            }
        }
        false
    }

    // Go: internal/parser/parser.go:parseSimpleArrowFunctionExpression
    fn parse_simple_arrow_function_expression(
        &mut self,
        pos: i32,
        identifier: NodeId,
        allow_return_type_in_arrow_function: bool,
        async_modifier: Option<ModifierList>,
    ) -> NodeId {
        let param_pos = self.arena.loc(identifier).pos();
        let parameter = self
            .arena
            .new_parameter_declaration(None, None, identifier, None, None, None);
        let parameter = self.finish_node(parameter, param_pos);
        let parameters = self.new_node_list(self.arena.loc(parameter), vec![parameter]);
        let is_async = async_modifier.is_some();
        let equals_greater_than_token = self.parse_expected_token(Kind::EqualsGreaterThanToken);
        let body = self
            .parse_arrow_function_expression_body(is_async, allow_return_type_in_arrow_function);
        let node = self.arena.new_arrow_function(
            async_modifier,
            None,
            parameters,
            None,
            None,
            equals_greater_than_token,
            body,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseArrowFunctionExpressionBody (subset)
    fn parse_arrow_function_expression_body(
        &mut self,
        is_async: bool,
        allow_return_type_in_arrow_function: bool,
    ) -> NodeId {
        if self.token == Kind::OpenBraceToken {
            let flags = if is_async {
                ParseFlags::AWAIT
            } else {
                ParseFlags::NONE
            };
            return self.parse_function_block(flags);
        }
        // DEFER(phase-3): the missing-open-brace statement-recovery branch.
        // blocked-by: full isStartOfStatement/isStartOfExpressionStatement parity.
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::AWAIT_CONTEXT, is_async);
        self.set_context_flags(NodeFlags::YIELD_CONTEXT, false);
        let node =
            self.parse_assignment_expression_or_higher_worker(allow_return_type_in_arrow_function);
        self.context_flags = save;
        node
    }

    // Go: internal/parser/parser.go:parseParametersWorker
    fn parse_parameters_worker(
        &mut self,
        flags: ParseFlags,
        allow_ambiguity: bool,
    ) -> Option<NodeList> {
        let in_await_context = self.in_context(NodeFlags::AWAIT_CONTEXT);
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::YIELD_CONTEXT, flags.contains(ParseFlags::YIELD));
        self.set_context_flags(NodeFlags::AWAIT_CONTEXT, flags.contains(ParseFlags::AWAIT));
        let parameters = self.parse_delimited_list_opt(ParsingContext::Parameters, |p| {
            p.parse_parameter_ex(in_await_context, allow_ambiguity)
        });
        self.context_flags = save;
        parameters
    }

    // Go: internal/parser/parser.go:isYieldExpression
    fn is_yield_expression(&mut self) -> bool {
        if self.token == Kind::YieldKeyword {
            if self.in_yield_context() {
                return true;
            }
            return self
                .look_ahead(|p| p.next_token_is_identifier_or_keyword_or_literal_on_same_line());
        }
        false
    }

    // Go: internal/parser/parser.go:parseYieldExpression
    fn parse_yield_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let node = if !self.has_preceding_line_break()
            && (self.token == Kind::AsteriskToken || self.is_start_of_expression())
        {
            let asterisk_token = self.parse_optional_token(Kind::AsteriskToken);
            let expression = self.parse_assignment_expression_or_higher();
            self.arena
                .new_yield_expression(asterisk_token, Some(expression))
        } else {
            self.arena.new_yield_expression(None, None)
        };
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:nextTokenIsIdentifierOrKeywordOnSameLine
    fn next_token_is_identifier_or_keyword_on_same_line(&mut self) -> bool {
        self.next_token();
        token_is_identifier_or_keyword(self.token) && !self.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:nextTokenIsIdentifierOrKeywordOrLiteralOnSameLine
    fn next_token_is_identifier_or_keyword_or_literal_on_same_line(&mut self) -> bool {
        self.next_token();
        (token_is_identifier_or_keyword(self.token)
            || self.token == Kind::NumericLiteral
            || self.token == Kind::BigIntLiteral
            || self.token == Kind::StringLiteral)
            && !self.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:parseConditionalExpressionRest
    fn parse_conditional_expression_rest(
        &mut self,
        left_operand: NodeId,
        pos: i32,
        allow_return_type_in_arrow_function: bool,
    ) -> NodeId {
        let question_token = match self.parse_optional_token(Kind::QuestionToken) {
            None => return left_operand,
            Some(q) => q,
        };
        let save_context_flags = self.context_flags;
        self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
        let true_expression = self.parse_assignment_expression_or_higher_worker(false);
        self.context_flags = save_context_flags;
        let colon_token = self.parse_expected_token(Kind::ColonToken);
        let false_expression = if node_is_present(&self.arena, colon_token) {
            self.parse_assignment_expression_or_higher_worker(allow_return_type_in_arrow_function)
        } else {
            self.create_missing_identifier()
        };
        let node = self.arena.new_conditional_expression(
            left_operand,
            question_token,
            true_expression,
            colon_token,
            false_expression,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseBinaryExpressionOrHigher
    fn parse_binary_expression_or_higher(
        &mut self,
        precedence: tsgo_ast::OperatorPrecedence,
    ) -> NodeId {
        let pos = self.node_pos();
        let left_operand = self.parse_unary_expression_or_higher();
        self.parse_binary_expression_rest(precedence, left_operand, pos)
    }

    // Go: internal/parser/parser.go:parseBinaryExpressionRest (subset)
    fn parse_binary_expression_rest(
        &mut self,
        precedence: tsgo_ast::OperatorPrecedence,
        mut left_operand: NodeId,
        pos: i32,
    ) -> NodeId {
        loop {
            self.re_scan_greater_than_token();
            let new_precedence = tsgo_ast::precedence::get_binary_operator_precedence(self.token);
            let consume_current_operator = if self.token == Kind::AsteriskAsteriskToken {
                new_precedence >= precedence
            } else {
                new_precedence > precedence
            };
            if !consume_current_operator {
                break;
            }
            if self.token == Kind::InKeyword && self.in_disallow_in_context() {
                break;
            }
            if self.token == Kind::AsKeyword || self.token == Kind::SatisfiesKeyword {
                if self.has_preceding_line_break() {
                    break;
                }
                let keyword_kind = self.token;
                self.next_token();
                let type_node = self.parse_type();
                left_operand = if keyword_kind == Kind::SatisfiesKeyword {
                    self.make_satisfies_expression(left_operand, type_node)
                } else {
                    self.make_as_expression(left_operand, type_node)
                };
                continue;
            }
            let op = self.parse_token_node();
            let right = self.parse_binary_expression_or_higher(new_precedence);
            left_operand = self.make_binary_expression(left_operand, op, right, pos);
        }
        left_operand
    }

    // Go: internal/parser/parser.go:makeAsExpression
    fn make_as_expression(&mut self, left: NodeId, right_type: NodeId) -> NodeId {
        let pos = self.arena.loc(left).pos();
        let node = self.arena.new_as_expression(left, right_type);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:makeSatisfiesExpression
    fn make_satisfies_expression(&mut self, left: NodeId, right_type: NodeId) -> NodeId {
        let pos = self.arena.loc(left).pos();
        let node = self.arena.new_satisfies_expression(left, right_type);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:makeBinaryExpression
    fn make_binary_expression(
        &mut self,
        left: NodeId,
        operator_token: NodeId,
        right: NodeId,
        pos: i32,
    ) -> NodeId {
        let node = self
            .arena
            .new_binary_expression(left, operator_token, right);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseUnaryExpressionOrHigher (subset)
    fn parse_unary_expression_or_higher(&mut self) -> NodeId {
        if self.is_update_expression() {
            let pos = self.node_pos();
            let update_expression = self.parse_update_expression();
            if self.token == Kind::AsteriskAsteriskToken {
                let prec = tsgo_ast::precedence::get_binary_operator_precedence(self.token);
                return self.parse_binary_expression_rest(prec, update_expression, pos);
            }
            return update_expression;
        }
        self.parse_simple_unary_expression()
    }

    // Go: internal/parser/parser.go:isUpdateExpression
    fn is_update_expression(&self) -> bool {
        match self.token {
            Kind::PlusToken
            | Kind::MinusToken
            | Kind::TildeToken
            | Kind::ExclamationToken
            | Kind::DeleteKeyword
            | Kind::TypeOfKeyword
            | Kind::VoidKeyword
            | Kind::AwaitKeyword => false,
            Kind::LessThanToken => self.language_variant == LanguageVariant::Jsx,
            _ => true,
        }
    }

    // Go: internal/parser/parser.go:parseUpdateExpression (subset)
    fn parse_update_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.token == Kind::PlusPlusToken || self.token == Kind::MinusMinusToken {
            let operator = self.token;
            self.next_token();
            let operand = self.parse_left_hand_side_expression_or_higher();
            let node = self.arena.new_prefix_unary_expression(operator, operand);
            return self.finish_node(node, pos);
        }
        if self.language_variant == LanguageVariant::Jsx
            && self.token == Kind::LessThanToken
            && self.look_ahead(|p| p.next_token_is_identifier_or_keyword_or_greater_than())
        {
            // A JSX element is part of the primary expression.
            return self
                .parse_jsx_element_or_self_closing_element_or_fragment(true, -1, None, false);
        }
        let expression = self.parse_left_hand_side_expression_or_higher();
        if (self.token == Kind::PlusPlusToken || self.token == Kind::MinusMinusToken)
            && !self.has_preceding_line_break()
        {
            let operator = self.token;
            self.next_token();
            let node = self
                .arena
                .new_postfix_unary_expression(expression, operator);
            return self.finish_node(node, pos);
        }
        expression
    }

    // ---- JSX ----

    // Go: internal/parser/parser.go:scanJsxText
    fn scan_jsx_text(&mut self) -> Kind {
        self.token = self.scanner.scan_jsx_token();
        self.token
    }

    // Go: internal/parser/parser.go:scanJsxIdentifier
    fn scan_jsx_identifier(&mut self) -> Kind {
        self.token = self.scanner.scan_jsx_identifier();
        self.token
    }

    // Go: internal/parser/parser.go:scanJsxAttributeValue
    fn scan_jsx_attribute_value(&mut self) -> Kind {
        self.token = self.scanner.scan_jsx_attribute_value();
        self.token
    }

    // Go: internal/parser/parser.go:nextTokenIsIdentifierOrKeywordOrGreaterThan
    fn next_token_is_identifier_or_keyword_or_greater_than(&mut self) -> bool {
        self.next_token();
        self.token == Kind::GreaterThanToken || token_is_identifier_or_keyword(self.token)
    }

    // Go: internal/ast/utilities.go:TagNamesAreEquivalent
    fn tag_names_are_equivalent(&self, lhs: NodeId, rhs: NodeId) -> bool {
        let lk = self.arena.kind(lhs);
        if lk != self.arena.kind(rhs) {
            return false;
        }
        match lk {
            Kind::Identifier => self.arena.text(lhs) == self.arena.text(rhs),
            Kind::ThisKeyword => true,
            Kind::JsxNamespacedName => match (self.arena.data(lhs), self.arena.data(rhs)) {
                (
                    tsgo_ast::NodeData::JsxNamespacedName(l),
                    tsgo_ast::NodeData::JsxNamespacedName(r),
                ) => {
                    self.arena.text(l.namespace) == self.arena.text(r.namespace)
                        && self.arena.text(l.name) == self.arena.text(r.name)
                }
                _ => false,
            },
            Kind::PropertyAccessExpression => match (self.arena.data(lhs), self.arena.data(rhs)) {
                (
                    tsgo_ast::NodeData::PropertyAccessExpression(l),
                    tsgo_ast::NodeData::PropertyAccessExpression(r),
                ) => {
                    self.arena.text(l.name) == self.arena.text(r.name)
                        && self.tag_names_are_equivalent(l.expression, r.expression)
                }
                _ => false,
            },
            _ => false,
        }
    }

    // Returns the tag name of a JSX opening/self-closing/closing element.
    fn jsx_tag_name(&self, node: NodeId) -> NodeId {
        match self.arena.data(node) {
            tsgo_ast::NodeData::JsxOpeningElement(d)
            | tsgo_ast::NodeData::JsxSelfClosingElement(d) => d.tag_name,
            tsgo_ast::NodeData::JsxClosingElement(d) => d.tag_name,
            other => unreachable!("expected JSX element, got {other:?}"),
        }
    }

    // Go: internal/parser/parser.go:parseJsxElementOrSelfClosingElementOrFragment (subset)
    fn parse_jsx_element_or_self_closing_element_or_fragment(
        &mut self,
        in_expression_context: bool,
        _top_invalid_node_position: i32,
        _opening_tag: Option<NodeId>,
        _must_be_unary: bool,
    ) -> NodeId {
        let pos = self.node_pos();
        let opening = self
            .parse_jsx_opening_or_self_closing_element_or_opening_fragment(in_expression_context);
        let result = match self.arena.kind(opening) {
            Kind::JsxOpeningElement => {
                let children = self.parse_jsx_children(opening);
                let closing_element =
                    self.parse_jsx_closing_element(opening, in_expression_context);
                let opening_tag_name = self.jsx_tag_name(opening);
                let closing_tag_name = self.jsx_tag_name(closing_element);
                if !self.tag_names_are_equivalent(opening_tag_name, closing_tag_name) {
                    let loc = self.arena.loc(closing_tag_name);
                    self.parse_error_at(
                        loc.pos(),
                        loc.end(),
                        &diagnostics::EXPECTED_CORRESPONDING_JSX_CLOSING_TAG_FOR_0,
                        vec![self.text_of_node(opening_tag_name)],
                    );
                }
                let node = self
                    .arena
                    .new_jsx_element(opening, children, closing_element);
                self.finish_node(node, pos)
            }
            Kind::JsxOpeningFragment => {
                let children = self.parse_jsx_children(opening);
                let closing = self.parse_jsx_closing_fragment(in_expression_context);
                let node = self.arena.new_jsx_fragment(opening, children, closing);
                self.finish_node(node, pos)
            }
            Kind::JsxSelfClosingElement => opening,
            other => panic!("Unhandled case in JSX element parse: {other:?}"),
        };
        // DEFER(phase-4): unclosed-tag restructure + `<a/><b/>` binary-comma recovery.
        // blocked-by: tag-mismatch recovery (well-formed JSX parses without it).
        result
    }

    // Best-effort source text of a node (for diagnostics).
    fn text_of_node(&self, node: NodeId) -> String {
        let loc = self.arena.loc(node);
        let start = loc.pos().max(0) as usize;
        let end = (loc.end().max(0) as usize).min(self.source_text.len());
        if start <= end {
            self.source_text[start..end].trim().to_string()
        } else {
            String::new()
        }
    }

    // Go: internal/parser/parser.go:parseJsxOpeningOrSelfClosingElementOrOpeningFragment
    fn parse_jsx_opening_or_self_closing_element_or_opening_fragment(
        &mut self,
        in_expression_context: bool,
    ) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::LessThanToken);
        if self.token == Kind::GreaterThanToken {
            self.scan_jsx_text();
            let node = self.arena.new_jsx_opening_fragment();
            return self.finish_node(node, pos);
        }
        let tag_name = self.parse_jsx_element_name();
        let type_arguments = if !self.in_context(NodeFlags::JAVA_SCRIPT_FILE) {
            self.parse_type_arguments()
        } else {
            None
        };
        let attributes = self.parse_jsx_attributes();
        let result = if self.token == Kind::GreaterThanToken {
            self.scan_jsx_text();
            self.arena
                .new_jsx_opening_element(tag_name, type_arguments, attributes)
        } else {
            self.parse_expected(Kind::SlashToken);
            if self.parse_expected_with_diagnostic(Kind::GreaterThanToken, None, false) {
                if in_expression_context {
                    self.next_token();
                } else {
                    self.scan_jsx_text();
                }
            }
            self.arena
                .new_jsx_self_closing_element(tag_name, type_arguments, attributes)
        };
        self.finish_node(result, pos)
    }

    // Go: internal/parser/parser.go:parseJsxElementName
    fn parse_jsx_element_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        let initial_expression = self.parse_jsx_tag_name();
        if self.arena.kind(initial_expression) == Kind::JsxNamespacedName {
            return initial_expression;
        }
        let mut expression = initial_expression;
        while self.parse_optional(Kind::DotToken) {
            let name = self.parse_right_side_of_dot(true, false);
            let node = self
                .arena
                .new_property_access_expression(expression, None, name);
            expression = self.finish_node(node, pos);
        }
        expression
    }

    // Go: internal/parser/parser.go:parseJsxTagName
    fn parse_jsx_tag_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.scan_jsx_identifier();
        let is_this = self.token == Kind::ThisKeyword;
        let tag_name = self.parse_identifier_name();
        if self.parse_optional(Kind::ColonToken) {
            self.scan_jsx_identifier();
            let name = self.parse_identifier_name();
            let node = self.arena.new_jsx_namespaced_name(tag_name, name);
            return self.finish_node(node, pos);
        }
        if is_this {
            let node = self.arena.new_keyword_expression(Kind::ThisKeyword);
            return self.finish_node(node, pos);
        }
        tag_name
    }

    // Go: internal/parser/parser.go:parseJsxAttributes
    fn parse_jsx_attributes(&mut self) -> NodeId {
        let pos = self.node_pos();
        let properties =
            self.parse_list(ParsingContext::JsxAttributes, |p| p.parse_jsx_attribute());
        let node = self.arena.new_jsx_attributes(properties);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseJsxAttribute
    fn parse_jsx_attribute(&mut self) -> NodeId {
        if self.token == Kind::OpenBraceToken {
            return self.parse_jsx_spread_attribute();
        }
        let pos = self.node_pos();
        let name = self.parse_jsx_attribute_name();
        let initializer = self.parse_jsx_attribute_value();
        let node = self.arena.new_jsx_attribute(name, initializer);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseJsxSpreadAttribute
    fn parse_jsx_spread_attribute(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBraceToken);
        self.parse_expected(Kind::DotDotDotToken);
        let expression = self.parse_expression();
        self.parse_expected(Kind::CloseBraceToken);
        let node = self.arena.new_jsx_spread_attribute(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseJsxAttributeName
    fn parse_jsx_attribute_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.scan_jsx_identifier();
        let attr_name = self.parse_identifier_name();
        if self.parse_optional(Kind::ColonToken) {
            self.scan_jsx_identifier();
            let name = self.parse_identifier_name();
            let node = self.arena.new_jsx_namespaced_name(attr_name, name);
            return self.finish_node(node, pos);
        }
        attr_name
    }

    // Go: internal/parser/parser.go:parseJsxAttributeValue
    fn parse_jsx_attribute_value(&mut self) -> Option<NodeId> {
        if self.token == Kind::EqualsToken {
            if self.scan_jsx_attribute_value() == Kind::StringLiteral {
                return Some(self.parse_literal_expression());
            }
            if self.token == Kind::OpenBraceToken {
                return Some(self.parse_jsx_expression(true));
            }
            if self.token == Kind::LessThanToken {
                return Some(
                    self.parse_jsx_element_or_self_closing_element_or_fragment(
                        true, -1, None, false,
                    ),
                );
            }
            self.parse_error_at_current_token(&diagnostics::X_OR_JSX_ELEMENT_EXPECTED, Vec::new());
        }
        None
    }

    // Go: internal/parser/parser.go:parseJsxChildren
    fn parse_jsx_children(&mut self, opening_tag: NodeId) -> NodeList {
        let pos = self.node_pos();
        let save = self.parsing_contexts;
        self.parsing_contexts |= 1 << (ParsingContext::JsxChildren as u32);
        let mut list = Vec::new();
        loop {
            let current_token = self.scanner.re_scan_jsx_token(true);
            self.token = current_token;
            match self.parse_jsx_child(opening_tag, current_token) {
                None => break,
                Some(child) => list.push(child),
            }
        }
        self.parsing_contexts = save;
        let end = self.node_pos();
        self.new_node_list(TextRange::new(pos, end), list)
    }

    // Go: internal/parser/parser.go:parseJsxChild
    fn parse_jsx_child(&mut self, opening_tag: NodeId, token: Kind) -> Option<NodeId> {
        match token {
            Kind::EndOfFile => {
                // DEFER(phase-4): missing-closing-tag diagnostics on EOF.
                None
            }
            Kind::LessThanSlashToken | Kind::ConflictMarkerTrivia => None,
            Kind::JsxText | Kind::JsxTextAllWhiteSpaces => Some(self.parse_jsx_text()),
            Kind::OpenBraceToken => Some(self.parse_jsx_expression(false)),
            Kind::LessThanToken => {
                Some(self.parse_jsx_element_or_self_closing_element_or_fragment(
                    false,
                    -1,
                    Some(opening_tag),
                    false,
                ))
            }
            other => panic!("Unhandled case in parse_jsx_child: {other:?}"),
        }
    }

    // Go: internal/parser/parser.go:parseJsxText
    fn parse_jsx_text(&mut self) -> NodeId {
        let pos = self.node_pos();
        let text = self.scanner.token_value().to_string();
        let contains_only_trivia_white_spaces = self.token == Kind::JsxTextAllWhiteSpaces;
        let node = self
            .arena
            .new_jsx_text(&text, contains_only_trivia_white_spaces);
        self.scan_jsx_text();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseJsxExpression
    fn parse_jsx_expression(&mut self, in_expression_context: bool) -> NodeId {
        let pos = self.node_pos();
        if !self.parse_expected(Kind::OpenBraceToken) {
            let node = self.arena.new_jsx_expression(None, None);
            return self.finish_node(node, pos);
        }
        let mut dot_dot_dot_token = None;
        let mut expression = None;
        if self.token != Kind::CloseBraceToken {
            if !in_expression_context {
                dot_dot_dot_token = self.parse_optional_token(Kind::DotDotDotToken);
            }
            expression = Some(self.parse_expression());
        }
        if in_expression_context {
            self.parse_expected(Kind::CloseBraceToken);
        } else if self.parse_expected_with_diagnostic(Kind::CloseBraceToken, None, false) {
            self.scan_jsx_text();
        }
        let node = self.arena.new_jsx_expression(dot_dot_dot_token, expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseJsxClosingElement
    fn parse_jsx_closing_element(&mut self, open: NodeId, in_expression_context: bool) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::LessThanSlashToken);
        let tag_name = self.parse_jsx_element_name();
        if self.parse_expected_with_diagnostic(Kind::GreaterThanToken, None, false) {
            let open_tag_name = self.jsx_tag_name(open);
            if in_expression_context || !self.tag_names_are_equivalent(open_tag_name, tag_name) {
                self.next_token();
            } else {
                self.scan_jsx_text();
            }
        }
        let node = self.arena.new_jsx_closing_element(tag_name);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseJsxClosingFragment
    fn parse_jsx_closing_fragment(&mut self, in_expression_context: bool) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::LessThanSlashToken);
        if self.parse_expected_with_diagnostic(
            Kind::GreaterThanToken,
            Some(&diagnostics::EXPECTED_CORRESPONDING_CLOSING_TAG_FOR_JSX_FRAGMENT),
            false,
        ) {
            if in_expression_context {
                self.next_token();
            } else {
                self.scan_jsx_text();
            }
        }
        let node = self.arena.new_jsx_closing_fragment();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseSimpleUnaryExpression (subset)
    fn parse_simple_unary_expression(&mut self) -> NodeId {
        let token = self.token;
        match token {
            Kind::PlusToken | Kind::MinusToken | Kind::TildeToken | Kind::ExclamationToken => {
                self.parse_prefix_unary_expression()
            }
            Kind::DeleteKeyword => self.parse_delete_expression(),
            Kind::TypeOfKeyword => self.parse_type_of_expression(),
            Kind::VoidKeyword => self.parse_void_expression(),
            Kind::AwaitKeyword if self.is_await_expression() => self.parse_await_expression(),
            Kind::LessThanToken if self.language_variant == LanguageVariant::Jsx => {
                self.parse_jsx_element_or_self_closing_element_or_fragment(true, -1, None, true)
            }
            Kind::LessThanToken => self.parse_type_assertion(),
            _ => self.parse_update_expression(),
        }
    }

    // Go: internal/parser/parser.go:parseTypeAssertion
    fn parse_type_assertion(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::LessThanToken);
        let type_node = self.parse_type();
        self.parse_expected(Kind::GreaterThanToken);
        let expression = self.parse_simple_unary_expression();
        let node = self.arena.new_type_assertion(type_node, expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseDeleteExpression
    fn parse_delete_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let operand = self.parse_simple_unary_expression();
        let node = self.arena.new_delete_expression(operand);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeOfExpression
    fn parse_type_of_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let operand = self.parse_simple_unary_expression();
        let node = self.arena.new_type_of_expression(operand);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseVoidExpression
    fn parse_void_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let operand = self.parse_simple_unary_expression();
        let node = self.arena.new_void_expression(operand);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:isAwaitExpression
    fn is_await_expression(&mut self) -> bool {
        if self.token == Kind::AwaitKeyword {
            if self.in_await_context() {
                return true;
            }
            return self
                .look_ahead(|p| p.next_token_is_identifier_or_keyword_or_literal_on_same_line());
        }
        false
    }

    // Go: internal/parser/parser.go:parseAwaitExpression
    fn parse_await_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let operand = self.parse_simple_unary_expression();
        let node = self.arena.new_await_expression(operand);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parsePrefixUnaryExpression
    fn parse_prefix_unary_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        let operator = self.token;
        self.next_token();
        let operand = self.parse_simple_unary_expression();
        let node = self.arena.new_prefix_unary_expression(operator, operand);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseLeftHandSideExpressionOrHigher (subset)
    fn parse_left_hand_side_expression_or_higher(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = if self.token == Kind::ImportKeyword
            && self.look_ahead(|p| p.next_token_is_open_paren_or_less_than())
        {
            // Dynamic `import(...)`: parse the `import` keyword as a bare keyword
            // expression so `parseCallExpressionRest` can consume the argument
            // list. We look ahead for `(`/`<` so a leading `import` that actually
            // starts an import statement (e.g. `import * as foo`) is not eagerly
            // consumed here.
            // DEFER(phase-3): set sourceFlags |= PossiblyContainsDynamicImport.
            // blocked-by: finishSourceFile / source-flags tracking.
            self.parse_keyword_expression()
        } else if self.token == Kind::ImportKeyword
            && self.look_ahead(|p| p.next_token() == Kind::DotToken)
        {
            // `import.meta` metaproperty.
            self.next_token();
            self.next_token();
            let name = self.parse_identifier_name();
            let node = self.arena.new_meta_property(Kind::ImportKeyword, name);
            self.finish_node(node, pos)
        } else {
            self.parse_member_expression_or_higher()
        };
        self.parse_call_expression_rest(pos, expression)
    }

    // Go: internal/parser/parser.go:parseMemberExpressionOrHigher
    fn parse_member_expression_or_higher(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = self.parse_primary_expression();
        self.parse_member_expression_rest(pos, expression, true)
    }

    // Go: internal/parser/parser.go:parseMemberExpressionRest (subset)
    fn parse_member_expression_rest(
        &mut self,
        pos: i32,
        mut expression: NodeId,
        allow_optional_chain: bool,
    ) -> NodeId {
        loop {
            let mut question_dot_token = None;
            let is_property_access;
            if allow_optional_chain && self.is_start_of_optional_property_or_element_access_chain()
            {
                question_dot_token = Some(self.parse_expected_token(Kind::QuestionDotToken));
                is_property_access = token_is_identifier_or_keyword(self.token);
            } else {
                is_property_access = self.parse_optional(Kind::DotToken);
            }
            if is_property_access {
                expression =
                    self.parse_property_access_expression_rest(pos, expression, question_dot_token);
                continue;
            }
            if (question_dot_token.is_some() || !self.in_decorator_context())
                && self.parse_optional(Kind::OpenBracketToken)
            {
                expression =
                    self.parse_element_access_expression_rest(pos, expression, question_dot_token);
                continue;
            }
            if self.is_template_start_of_tagged_template() {
                // Absorb type arguments into the tagged template when the preceding
                // expression is an `ExpressionWithTypeArguments` (`f<T>` followed by a template).
                if question_dot_token.is_none()
                    && self.arena.kind(expression) == Kind::ExpressionWithTypeArguments
                {
                    let (inner, type_args) = self.expression_with_type_arguments_parts(expression);
                    expression =
                        self.parse_tagged_template_rest(pos, inner, question_dot_token, type_args);
                } else {
                    expression =
                        self.parse_tagged_template_rest(pos, expression, question_dot_token, None);
                }
                continue;
            }
            if question_dot_token.is_none() {
                if self.token == Kind::ExclamationToken && !self.has_preceding_line_break() {
                    self.next_token();
                    let node = self.arena.new_non_null_expression(expression);
                    expression = self.finish_node(node, pos);
                    continue;
                }
                let type_arguments = self.try_parse_type_arguments_in_expression();
                if type_arguments.is_some() {
                    let node = self
                        .arena
                        .new_expression_with_type_arguments(expression, type_arguments);
                    expression = self.finish_node(node, pos);
                    continue;
                }
            }
            return expression;
        }
    }

    // Returns the inner expression + type arguments of an `ExpressionWithTypeArguments`.
    // Go: internal/parser/parser.go (expression.AsExpressionWithTypeArguments())
    fn expression_with_type_arguments_parts(&self, node: NodeId) -> (NodeId, Option<NodeList>) {
        match self.arena.data(node) {
            tsgo_ast::NodeData::ExpressionWithTypeArguments(d) => {
                (d.expression, d.type_arguments.clone())
            }
            other => unreachable!("expected ExpressionWithTypeArguments, got {other:?}"),
        }
    }

    // Go: internal/parser/parser.go:tryParseTypeArgumentsInExpression
    fn try_parse_type_arguments_in_expression(&mut self) -> Option<NodeList> {
        // Type arguments must not be parsed in JavaScript files (binary-operator ambiguity).
        let state = self.mark();
        if !self.in_context(NodeFlags::JAVA_SCRIPT_FILE)
            && self.re_scan_less_than_token() == Kind::LessThanToken
        {
            self.next_token();
            let type_arguments =
                self.parse_delimited_list(ParsingContext::TypeArguments, |p| p.parse_type());
            if self.re_scan_greater_than_token() == Kind::GreaterThanToken {
                self.next_token();
                if self.can_follow_type_arguments_in_expression() {
                    return Some(type_arguments);
                }
            }
        }
        self.rewind(state);
        None
    }

    // Go: internal/parser/parser.go:canFollowTypeArgumentsInExpression
    fn can_follow_type_arguments_in_expression(&mut self) -> bool {
        match self.token {
            Kind::OpenParenToken | Kind::NoSubstitutionTemplateLiteral | Kind::TemplateHead => true,
            Kind::LessThanToken | Kind::GreaterThanToken | Kind::PlusToken | Kind::MinusToken => {
                false
            }
            _ => {
                self.has_preceding_line_break()
                    || self.is_binary_operator()
                    || !self.is_start_of_expression()
            }
        }
    }

    // Go: internal/parser/parser.go:tryReparseOptionalChain
    fn try_reparse_optional_chain(&mut self, node: NodeId) -> bool {
        if self.arena.flags(node).contains(NodeFlags::OPTIONAL_CHAIN) {
            return true;
        }
        // Check for an optional chain hidden inside a run of non-null expressions.
        if self.arena.kind(node) == Kind::NonNullExpression {
            let mut expr = self.non_null_expression_operand(node);
            while self.arena.kind(expr) == Kind::NonNullExpression
                && !self.arena.flags(expr).contains(NodeFlags::OPTIONAL_CHAIN)
            {
                expr = self.non_null_expression_operand(expr);
            }
            if self.arena.flags(expr).contains(NodeFlags::OPTIONAL_CHAIN) {
                let mut n = node;
                while self.arena.kind(n) == Kind::NonNullExpression {
                    self.arena.add_flags(n, NodeFlags::OPTIONAL_CHAIN);
                    n = self.non_null_expression_operand(n);
                }
                return true;
            }
        }
        false
    }

    // Returns the operand of a non-null expression (`expr!`).
    fn non_null_expression_operand(&self, node: NodeId) -> NodeId {
        match self.arena.data(node) {
            tsgo_ast::NodeData::NonNullExpression(d) => d.expression,
            other => unreachable!("expected NonNullExpression, got {other:?}"),
        }
    }

    // Go: internal/parser/parser.go:isStartOfOptionalPropertyOrElementAccessChain
    fn is_start_of_optional_property_or_element_access_chain(&mut self) -> bool {
        self.token == Kind::QuestionDotToken
            && self.look_ahead(|p| {
                p.next_token();
                token_is_identifier_or_keyword(p.token)
                    || p.token == Kind::OpenBracketToken
                    || p.is_template_start_of_tagged_template()
            })
    }

    // Go: internal/parser/parser.go:isTemplateStartOfTaggedTemplate
    fn is_template_start_of_tagged_template(&self) -> bool {
        self.token == Kind::NoSubstitutionTemplateLiteral || self.token == Kind::TemplateHead
    }

    // Go: internal/parser/parser.go:parsePropertyAccessExpressionRest (subset)
    fn parse_property_access_expression_rest(
        &mut self,
        pos: i32,
        expression: NodeId,
        question_dot_token: Option<NodeId>,
    ) -> NodeId {
        let name = self.parse_right_side_of_dot(true, true);
        let is_optional_chain =
            question_dot_token.is_some() || self.try_reparse_optional_chain(expression);
        let node = self
            .arena
            .new_property_access_expression(expression, question_dot_token, name);
        if is_optional_chain {
            self.arena.add_flags(node, NodeFlags::OPTIONAL_CHAIN);
        }
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseElementAccessExpressionRest (subset)
    fn parse_element_access_expression_rest(
        &mut self,
        pos: i32,
        expression: NodeId,
        question_dot_token: Option<NodeId>,
    ) -> NodeId {
        let argument_expression = if self.token == Kind::CloseBracketToken {
            self.parse_error_at(
                self.node_pos(),
                self.node_pos(),
                &diagnostics::AN_ELEMENT_ACCESS_EXPRESSION_SHOULD_TAKE_AN_ARGUMENT,
                Vec::new(),
            );
            self.create_missing_identifier()
        } else {
            self.parse_expression()
        };
        self.parse_expected(Kind::CloseBracketToken);
        let is_optional_chain =
            question_dot_token.is_some() || self.try_reparse_optional_chain(expression);
        let node = self.arena.new_element_access_expression(
            expression,
            question_dot_token,
            argument_expression,
        );
        if is_optional_chain {
            self.arena.add_flags(node, NodeFlags::OPTIONAL_CHAIN);
        }
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTaggedTemplateRest
    fn parse_tagged_template_rest(
        &mut self,
        pos: i32,
        tag: NodeId,
        question_dot_token: Option<NodeId>,
        type_arguments: Option<NodeList>,
    ) -> NodeId {
        let template = if self.token == Kind::NoSubstitutionTemplateLiteral {
            self.token = self.scanner.re_scan_template_token(true);
            self.parse_literal_expression()
        } else {
            self.parse_template_expression(true)
        };
        let is_optional_chain = question_dot_token.is_some();
        let node = self.arena.new_tagged_template_expression(
            tag,
            question_dot_token,
            type_arguments,
            template,
        );
        if is_optional_chain {
            self.arena.add_flags(node, NodeFlags::OPTIONAL_CHAIN);
        }
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseCallExpressionRest
    fn parse_call_expression_rest(&mut self, pos: i32, mut expression: NodeId) -> NodeId {
        loop {
            expression = self.parse_member_expression_rest(pos, expression, true);
            let mut type_arguments: Option<NodeList> = None;
            let question_dot_token = self.parse_optional_token(Kind::QuestionDotToken);
            if question_dot_token.is_some() {
                type_arguments = self.try_parse_type_arguments_in_expression();
                if self.is_template_start_of_tagged_template() {
                    expression = self.parse_tagged_template_rest(
                        pos,
                        expression,
                        question_dot_token,
                        type_arguments,
                    );
                    continue;
                }
            }
            if type_arguments.is_some() || self.token == Kind::OpenParenToken {
                // Absorb type arguments into the call when the expression is `f<T>`.
                if question_dot_token.is_none()
                    && self.arena.kind(expression) == Kind::ExpressionWithTypeArguments
                {
                    let (inner, ta) = self.expression_with_type_arguments_parts(expression);
                    type_arguments = ta;
                    expression = inner;
                }
                let argument_list = self.parse_argument_list();
                let is_optional_chain =
                    question_dot_token.is_some() || self.try_reparse_optional_chain(expression);
                let flags = if is_optional_chain {
                    NodeFlags::OPTIONAL_CHAIN
                } else {
                    NodeFlags::NONE
                };
                let node = self.arena.new_call_expression(
                    expression,
                    question_dot_token,
                    type_arguments,
                    argument_list,
                    flags,
                );
                expression = self.finish_node(node, pos);
                continue;
            }
            if question_dot_token.is_some() {
                // We parsed `?.` but then failed to parse anything: report a missing identifier.
                self.parse_error_at_current_token(&diagnostics::IDENTIFIER_EXPECTED, Vec::new());
                let name = self.create_missing_identifier();
                let node =
                    self.arena
                        .new_property_access_expression(expression, question_dot_token, name);
                self.arena.add_flags(node, NodeFlags::OPTIONAL_CHAIN);
                expression = self.finish_node(node, pos);
            }
            break;
        }
        expression
    }

    // Go: internal/parser/parser.go:parseNewExpressionOrNewDotTarget (subset)
    fn parse_new_expression_or_new_dot_target(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::NewKeyword);
        if self.parse_optional(Kind::DotToken) {
            let name = self.parse_identifier_name();
            let node = self.arena.new_meta_property(Kind::NewKeyword, name);
            return self.finish_node(node, pos);
        }
        let expression_pos = self.node_pos();
        let primary = self.parse_primary_expression();
        let expression = self.parse_member_expression_rest(expression_pos, primary, false);
        let argument_list = if self.token == Kind::OpenParenToken {
            Some(self.parse_argument_list())
        } else {
            None
        };
        let node = self
            .arena
            .new_new_expression(expression, None, argument_list);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTemplateExpression
    fn parse_template_expression(&mut self, is_tagged_template: bool) -> NodeId {
        let pos = self.node_pos();
        let head = self.parse_template_head(is_tagged_template);
        let template_spans = self.parse_template_spans(is_tagged_template);
        let node = self.arena.new_template_expression(head, template_spans);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTemplateSpans
    fn parse_template_spans(&mut self, is_tagged_template: bool) -> NodeList {
        let pos = self.node_pos();
        let mut list = Vec::new();
        loop {
            let span = self.parse_template_span(is_tagged_template);
            let literal = match self.arena.data(span) {
                tsgo_ast::NodeData::TemplateSpan(d) => d.literal,
                _ => unreachable!("parse_template_span returns a TemplateSpan"),
            };
            let is_middle = self.arena.kind(literal) == Kind::TemplateMiddle;
            list.push(span);
            if !is_middle {
                break;
            }
        }
        let end = self.node_pos();
        self.new_node_list(TextRange::new(pos, end), list)
    }

    // Go: internal/parser/parser.go:parseTemplateSpan
    fn parse_template_span(&mut self, is_tagged_template: bool) -> NodeId {
        let pos = self.node_pos();
        let expression = self.parse_expression_allow_in();
        let literal = self.parse_literal_of_template_span(is_tagged_template);
        let node = self.arena.new_template_span(expression, literal);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTemplateHead
    fn parse_template_head(&mut self, is_tagged_template: bool) -> NodeId {
        if !is_tagged_template
            && self
                .scanner
                .token_flags()
                .contains(tsgo_ast::TokenFlags::IS_INVALID)
        {
            self.scanner.re_scan_template_token(false);
        }
        let pos = self.node_pos();
        let text = self.scanner.token_value().to_string();
        let token_flags = self.scanner.token_flags();
        let node =
            self.arena
                .new_template_literal_like_node(Kind::TemplateHead, &text, token_flags);
        self.next_token();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseLiteralOfTemplateSpan
    fn parse_literal_of_template_span(&mut self, is_tagged_template: bool) -> NodeId {
        if self.token == Kind::CloseBraceToken {
            self.token = self.scanner.re_scan_template_token(is_tagged_template);
            self.parse_template_middle_or_tail()
        } else {
            self.parse_error_at_current_token(
                &diagnostics::X_0_EXPECTED,
                vec![tsgo_scanner::token_to_string(Kind::CloseBraceToken).to_string()],
            );
            let pos = self.node_pos();
            let node = self.arena.new_template_literal_like_node(
                Kind::TemplateTail,
                "",
                tsgo_ast::TokenFlags::NONE,
            );
            self.finish_node_with_end(node, pos, pos)
        }
    }

    // Go: internal/parser/parser.go:parseTemplateMiddleOrTail
    fn parse_template_middle_or_tail(&mut self) -> NodeId {
        let pos = self.node_pos();
        let kind = if self.token == Kind::TemplateMiddle {
            Kind::TemplateMiddle
        } else {
            Kind::TemplateTail
        };
        let text = self.scanner.token_value().to_string();
        let token_flags = self.scanner.token_flags();
        let node = self
            .arena
            .new_template_literal_like_node(kind, &text, token_flags);
        self.next_token();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseArgumentList (subset)
    fn parse_argument_list(&mut self) -> NodeList {
        self.parse_expected(Kind::OpenParenToken);
        let result = self.parse_delimited_list(ParsingContext::ArgumentExpressions, |p| {
            p.parse_argument_expression()
        });
        self.parse_expected(Kind::CloseParenToken);
        result
    }

    // Go: internal/parser/parser.go:parseArgumentOrArrayLiteralElement
    fn parse_argument_expression(&mut self) -> NodeId {
        match self.token {
            Kind::DotDotDotToken => self.parse_spread_element(),
            Kind::CommaToken => {
                let pos = self.node_pos();
                let node = self.arena.new_omitted_expression();
                self.finish_node(node, pos)
            }
            _ => self.parse_assignment_expression_or_higher(),
        }
    }

    // Go: internal/parser/parser.go:parseSpreadElement
    fn parse_spread_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::DotDotDotToken);
        let expression = self.parse_assignment_expression_or_higher();
        let node = self.arena.new_spread_element(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parsePrimaryExpression (subset)
    fn parse_primary_expression(&mut self) -> NodeId {
        let token = self.token;
        match token {
            Kind::NumericLiteral | Kind::BigIntLiteral | Kind::StringLiteral => {
                self.parse_literal_expression()
            }
            Kind::ThisKeyword
            | Kind::SuperKeyword
            | Kind::NullKeyword
            | Kind::TrueKeyword
            | Kind::FalseKeyword => self.parse_keyword_expression(),
            Kind::NoSubstitutionTemplateLiteral | Kind::RegularExpressionLiteral => {
                self.parse_literal_expression()
            }
            Kind::OpenParenToken => self.parse_parenthesized_expression(),
            Kind::OpenBracketToken => self.parse_array_literal_expression(),
            Kind::OpenBraceToken => self.parse_object_literal_expression(),
            Kind::ClassKeyword => self.parse_class_expression(),
            Kind::FunctionKeyword => self.parse_function_expression(),
            Kind::AsyncKeyword
                if self.look_ahead(|p| p.next_token_is_function_keyword_on_same_line()) =>
            {
                self.parse_function_expression()
            }
            Kind::NewKeyword => self.parse_new_expression_or_new_dot_target(),
            Kind::AtToken => self.parse_decorated_expression(),
            Kind::TemplateHead => self.parse_template_expression(false),
            Kind::PrivateIdentifier => self.parse_private_identifier(),
            Kind::SlashToken | Kind::SlashEqualsToken
                if self.re_scan_slash_token() == Kind::RegularExpressionLiteral =>
            {
                self.parse_literal_expression()
            }
            // DEFER(phase-3): JSX primary expressions.
            _ => self.parse_identifier_with_diagnostic(Some(&diagnostics::EXPRESSION_EXPECTED)),
        }
    }

    // Go: internal/parser/parser.go:parseParenthesizedExpression
    fn parse_parenthesized_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenParenToken);
        let expression = self.parse_expression();
        self.parse_expected(Kind::CloseParenToken);
        let node = self.arena.new_parenthesized_expression(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseArrayLiteralExpression (subset)
    fn parse_array_literal_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBracketToken);
        let elements = self.parse_delimited_list(ParsingContext::ArrayLiteralMembers, |p| {
            p.parse_argument_expression()
        });
        self.parse_expected(Kind::CloseBracketToken);
        let node = self.arena.new_array_literal_expression(elements);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseObjectLiteralExpression
    fn parse_object_literal_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBraceToken);
        let properties = self.parse_delimited_list(ParsingContext::ObjectLiteralMembers, |p| {
            p.parse_object_literal_element()
        });
        self.parse_expected(Kind::CloseBraceToken);
        let node = self.arena.new_object_literal_expression(properties);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseObjectLiteralElement
    fn parse_object_literal_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.parse_optional(Kind::DotDotDotToken) {
            let expression = self.parse_assignment_expression_or_higher();
            let node = self.arena.new_spread_assignment(expression);
            return self.finish_node(node, pos);
        }
        let modifiers = self.parse_modifiers_ex(true, false, false);
        if self.parse_contextual_modifier(Kind::GetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, Kind::GetAccessor);
        }
        if self.parse_contextual_modifier(Kind::SetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, Kind::SetAccessor);
        }
        let asterisk_token = self.parse_optional_token(Kind::AsteriskToken);
        let token_is_identifier = self.is_identifier();
        let name = self.parse_property_name();
        let mut postfix_token = self.parse_optional_token(Kind::QuestionToken);
        if postfix_token.is_none() {
            postfix_token = self.parse_optional_token(Kind::ExclamationToken);
        }
        if asterisk_token.is_some()
            || self.token == Kind::OpenParenToken
            || self.token == Kind::LessThanToken
        {
            return self.parse_method_declaration(
                pos,
                modifiers,
                asterisk_token,
                name,
                postfix_token,
            );
        }
        let is_shorthand = token_is_identifier && self.token != Kind::ColonToken;
        let node = if is_shorthand {
            let equals_token = self.parse_optional_token(Kind::EqualsToken);
            let initializer = if equals_token.is_some() {
                let save = self.context_flags;
                self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
                let i = self.parse_assignment_expression_or_higher();
                self.context_flags = save;
                Some(i)
            } else {
                None
            };
            self.arena.new_shorthand_property_assignment(
                modifiers,
                name,
                postfix_token,
                None,
                equals_token,
                initializer,
            )
        } else {
            self.parse_expected(Kind::ColonToken);
            let save = self.context_flags;
            self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
            let initializer = self.parse_assignment_expression_or_higher();
            self.context_flags = save;
            self.arena.new_property_assignment(
                modifiers,
                name,
                postfix_token,
                None,
                Some(initializer),
            )
        };
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseFunctionExpression (subset)
    fn parse_function_expression(&mut self) -> NodeId {
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::DECORATOR_CONTEXT, false);
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers();
        self.parse_expected(Kind::FunctionKeyword);
        let asterisk_token = self.parse_optional_token(Kind::AsteriskToken);
        let signature_flags = self.signature_flags(asterisk_token, &modifiers);
        let name = self.parse_optional_binding_identifier();
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters();
        let return_type = self.parse_return_type(Kind::ColonToken, false);
        let body = self.parse_function_block(signature_flags);
        self.context_flags = save;
        let node = self.arena.new_function_expression(
            modifiers,
            asterisk_token,
            name,
            type_parameters,
            parameters,
            return_type,
            None,
            Some(body),
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseOptionalBindingIdentifier
    fn parse_optional_binding_identifier(&mut self) -> Option<NodeId> {
        if self.is_binding_identifier() {
            Some(self.parse_binding_identifier())
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseKeywordExpression
    fn parse_keyword_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        let kind = self.token;
        let node = self.arena.new_keyword_expression(kind);
        self.next_token();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseLiteralExpression (subset, intern=false)
    fn parse_literal_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        let text = self.scanner.token_value().to_string();
        let token_flags = self.scanner.token_flags();
        let node = match self.token {
            Kind::StringLiteral => self.arena.new_string_literal(&text, token_flags),
            Kind::NumericLiteral => self.arena.new_numeric_literal(&text, token_flags),
            Kind::BigIntLiteral => self.arena.new_big_int_literal(&text, token_flags),
            Kind::RegularExpressionLiteral => self
                .arena
                .new_regular_expression_literal(&text, token_flags),
            Kind::NoSubstitutionTemplateLiteral => self
                .arena
                .new_no_substitution_template_literal(&text, token_flags),
            other => panic!("Unhandled case in parse_literal_expression: {other:?}"),
        };
        self.next_token();
        self.finish_node(node, pos)
    }

    // ---- modifiers / declarations / functions ----

    // Go: internal/parser/parser.go:newModifierList
    fn new_modifier_list(
        &self,
        loc: TextRange,
        nodes: Vec<NodeId>,
        modifier_flags: ModifierFlags,
    ) -> ModifierList {
        ModifierList {
            list: NodeList { loc, nodes },
            modifier_flags,
        }
    }

    // Go: internal/parser/parser.go:parseModifiers
    fn parse_modifiers(&mut self) -> Option<ModifierList> {
        self.parse_modifiers_ex(false, false, false)
    }

    // Go: internal/parser/parser.go:parseModifiersEx
    fn parse_modifiers_ex(
        &mut self,
        allow_decorators: bool,
        permit_const_as_modifier: bool,
        stop_on_start_of_class_static_block: bool,
    ) -> Option<ModifierList> {
        let pos = self.node_pos();
        let mut list = Vec::new();
        let mut flags = ModifierFlags::empty();
        let mut has_leading_modifier = false;
        let mut has_trailing_decorator = false;
        let mut has_trailing_modifier = false;
        let mut has_static_modifier = false;
        loop {
            if allow_decorators && self.token == Kind::AtToken && !has_trailing_modifier {
                let decorator = self.parse_decorator();
                flags |= ModifierFlags::DECORATOR;
                list.push(decorator);
                if has_leading_modifier {
                    has_trailing_decorator = true;
                }
            } else {
                match self.try_parse_modifier(
                    has_static_modifier,
                    permit_const_as_modifier,
                    stop_on_start_of_class_static_block,
                ) {
                    None => break,
                    Some(modifier) => {
                        if self.arena.kind(modifier) == Kind::StaticKeyword {
                            has_static_modifier = true;
                        }
                        flags |= modifier_to_flag(self.arena.kind(modifier));
                        list.push(modifier);
                        if has_trailing_decorator {
                            has_trailing_modifier = true;
                        } else {
                            has_leading_modifier = true;
                        }
                    }
                }
            }
        }
        if list.is_empty() {
            None
        } else {
            Some(self.new_modifier_list(TextRange::new(pos, self.node_pos()), list, flags))
        }
    }

    // Go: internal/parser/parser.go:tryParseModifier
    fn try_parse_modifier(
        &mut self,
        has_seen_static_modifier: bool,
        permit_const_as_modifier: bool,
        stop_on_start_of_class_static_block: bool,
    ) -> Option<NodeId> {
        let pos = self.node_pos();
        let kind = self.token;
        if self.token == Kind::ConstKeyword && permit_const_as_modifier {
            // Subsequent modifiers must be on the same line so a standalone `const`
            // declaration is not mistaken for a modifier.
            if !self.look_ahead(|p| p.next_token_is_on_same_line_and_can_follow_modifier()) {
                return None;
            }
            self.next_token();
        } else {
            let stops_on_static_block = stop_on_start_of_class_static_block
                && self.token == Kind::StaticKeyword
                && self.look_ahead(|p| p.next_token() == Kind::OpenBraceToken);
            let duplicate_static = has_seen_static_modifier && self.token == Kind::StaticKeyword;
            if stops_on_static_block || duplicate_static || !self.parse_any_contextual_modifier() {
                return None;
            }
        }
        let node = self.arena.new_token(kind);
        Some(self.finish_node(node, pos))
    }

    // Go: internal/parser/parser.go:parseDecoratedExpression (subset)
    fn parse_decorated_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers_ex(true, false, false);
        if self.token == Kind::ClassKeyword {
            return self.parse_class_declaration_or_expression(
                pos,
                modifiers,
                Kind::ClassExpression,
            );
        }
        self.parse_error_at(
            self.node_pos(),
            self.node_pos(),
            &diagnostics::EXPRESSION_EXPECTED,
            Vec::new(),
        );
        // DEFER(phase-4): MissingDeclaration node for a non-class decorated head.
        // blocked-by: MissingDeclaration AST node.
        self.create_missing_identifier()
    }

    // Go: internal/parser/parser.go:parseDecorator
    fn parse_decorator(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::AtToken);
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::DECORATOR_CONTEXT, true);
        let expression = self.parse_decorator_expression();
        self.context_flags = save;
        let node = self.arena.new_decorator(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseDecoratorExpression
    fn parse_decorator_expression(&mut self) -> NodeId {
        if self.in_await_context() && self.token == Kind::AwaitKeyword {
            // `@await` is disallowed in an await context; recover by parsing a
            // missing identifier and continuing.
            let pos = self.node_pos();
            let await_expression =
                self.parse_identifier_with_diagnostic(Some(&diagnostics::EXPRESSION_EXPECTED));
            self.next_token();
            let member = self.parse_member_expression_rest(pos, await_expression, true);
            return self.parse_call_expression_rest(pos, member);
        }
        self.parse_left_hand_side_expression_or_higher()
    }

    // Go: internal/parser/parser.go:parseAnyContextualModifier
    fn parse_any_contextual_modifier(&mut self) -> bool {
        let state = self.mark();
        if is_modifier_kind(self.token) && self.next_token_can_follow_modifier() {
            return true;
        }
        self.rewind(state);
        false
    }

    // Go: internal/parser/parser.go:nextTokenCanFollowModifier
    fn next_token_can_follow_modifier(&mut self) -> bool {
        match self.token {
            Kind::ConstKeyword => self.next_token() == Kind::EnumKeyword,
            Kind::ExportKeyword => {
                self.next_token();
                if self.token == Kind::DefaultKeyword {
                    return self.look_ahead(|p| p.next_token_can_follow_default_keyword());
                }
                if self.token == Kind::TypeKeyword {
                    return self.look_ahead(|p| p.next_token_can_follow_export_modifier());
                }
                self.can_follow_export_modifier()
            }
            Kind::DefaultKeyword => self.next_token_can_follow_default_keyword(),
            Kind::StaticKeyword => {
                self.next_token();
                self.can_follow_modifier()
            }
            Kind::GetKeyword | Kind::SetKeyword => {
                self.next_token();
                self.can_follow_get_or_set_keyword()
            }
            _ => self.next_token_is_on_same_line_and_can_follow_modifier(),
        }
    }

    // Go: internal/parser/parser.go:nextTokenCanFollowDefaultKeyword
    fn next_token_can_follow_default_keyword(&mut self) -> bool {
        match self.next_token() {
            Kind::ClassKeyword | Kind::FunctionKeyword | Kind::InterfaceKeyword | Kind::AtToken => {
                true
            }
            Kind::AbstractKeyword => {
                self.look_ahead(|p| p.next_token_is_class_keyword_on_same_line())
            }
            Kind::AsyncKeyword => {
                self.look_ahead(|p| p.next_token_is_function_keyword_on_same_line())
            }
            _ => false,
        }
    }

    // Go: internal/parser/parser.go:nextTokenCanFollowExportModifier
    fn next_token_can_follow_export_modifier(&mut self) -> bool {
        self.next_token();
        self.can_follow_export_modifier()
    }

    // Go: internal/parser/parser.go:canFollowExportModifier
    fn can_follow_export_modifier(&mut self) -> bool {
        self.token == Kind::AtToken
            || self.token != Kind::AsteriskToken
                && self.token != Kind::AsKeyword
                && self.token != Kind::OpenBraceToken
                && self.can_follow_modifier()
    }

    // Go: internal/parser/parser.go:canFollowModifier
    fn can_follow_modifier(&self) -> bool {
        self.token == Kind::OpenBracketToken
            || self.token == Kind::OpenBraceToken
            || self.token == Kind::AsteriskToken
            || self.token == Kind::DotDotDotToken
            || self.is_literal_property_name()
    }

    // Go: internal/parser/parser.go:canFollowGetOrSetKeyword
    fn can_follow_get_or_set_keyword(&self) -> bool {
        self.token == Kind::OpenBracketToken || self.is_literal_property_name()
    }

    // Go: internal/parser/parser.go:nextTokenIsOnSameLineAndCanFollowModifier
    fn next_token_is_on_same_line_and_can_follow_modifier(&mut self) -> bool {
        self.next_token();
        if self.has_preceding_line_break() {
            return false;
        }
        self.can_follow_modifier()
    }

    // Go: internal/parser/parser.go:nextTokenIsClassKeywordOnSameLine
    fn next_token_is_class_keyword_on_same_line(&mut self) -> bool {
        self.next_token() == Kind::ClassKeyword && !self.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:nextTokenIsFunctionKeywordOnSameLine
    fn next_token_is_function_keyword_on_same_line(&mut self) -> bool {
        self.next_token() == Kind::FunctionKeyword && !self.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:nextTokenIsIdentifierOnSameLine
    fn next_token_is_identifier_on_same_line(&mut self) -> bool {
        self.next_token();
        self.is_identifier() && !self.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:nextTokenIsIdentifierOrStringLiteralOnSameLine
    fn next_token_is_identifier_or_string_literal_on_same_line(&mut self) -> bool {
        self.next_token();
        (self.is_identifier() || self.token == Kind::StringLiteral)
            && !self.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:isStartOfDeclaration
    fn is_start_of_declaration(&mut self) -> bool {
        self.look_ahead(|p| p.scan_start_of_declaration())
    }

    // Go: internal/parser/parser.go:scanStartOfDeclaration (subset)
    fn scan_start_of_declaration(&mut self) -> bool {
        loop {
            match self.token {
                Kind::VarKeyword
                | Kind::LetKeyword
                | Kind::ConstKeyword
                | Kind::FunctionKeyword
                | Kind::ClassKeyword
                | Kind::EnumKeyword => return true,
                Kind::InterfaceKeyword | Kind::TypeKeyword => {
                    return self.next_token_is_identifier_on_same_line()
                }
                Kind::ModuleKeyword | Kind::NamespaceKeyword => {
                    return self.next_token_is_identifier_or_string_literal_on_same_line()
                }
                Kind::AbstractKeyword
                | Kind::AccessorKeyword
                | Kind::AsyncKeyword
                | Kind::DeclareKeyword
                | Kind::PrivateKeyword
                | Kind::ProtectedKeyword
                | Kind::PublicKeyword
                | Kind::ReadonlyKeyword => {
                    let previous_token = self.token;
                    self.next_token();
                    if self.has_preceding_line_break() {
                        return false;
                    }
                    if previous_token == Kind::DeclareKeyword && self.token == Kind::TypeKeyword {
                        return true;
                    }
                    continue;
                }
                Kind::ImportKeyword => {
                    self.next_token();
                    return self.token == Kind::StringLiteral
                        || self.token == Kind::AsteriskToken
                        || self.token == Kind::OpenBraceToken
                        || token_is_identifier_or_keyword(self.token);
                }
                Kind::ExportKeyword => {
                    self.next_token();
                    if self.token == Kind::EqualsToken
                        || self.token == Kind::AsteriskToken
                        || self.token == Kind::OpenBraceToken
                        || self.token == Kind::DefaultKeyword
                        || self.token == Kind::AsKeyword
                        || self.token == Kind::AtToken
                    {
                        return true;
                    }
                    if self.token == Kind::TypeKeyword {
                        self.next_token();
                        return self.token == Kind::AsteriskToken
                            || self.token == Kind::OpenBraceToken
                            || self.is_identifier() && !self.has_preceding_line_break();
                    }
                    continue;
                }
                Kind::StaticKeyword => {
                    self.next_token();
                    continue;
                }
                _ => return false,
            }
        }
    }

    // Go: internal/parser/parser.go:parseDeclaration (subset)
    fn parse_declaration(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers_ex(true, false, false);
        let is_ambient = modifiers.as_ref().is_some_and(|m| {
            m.list
                .nodes
                .iter()
                .any(|&n| self.arena.kind(n) == Kind::DeclareKeyword)
        });
        if is_ambient {
            let save = self.context_flags;
            self.set_context_flags(NodeFlags::AMBIENT, true);
            let result = self.parse_declaration_worker(pos, modifiers);
            self.context_flags = save;
            result
        } else {
            self.parse_declaration_worker(pos, modifiers)
        }
    }

    // Go: internal/parser/parser.go:parseDeclarationWorker (subset)
    fn parse_declaration_worker(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        match self.token {
            Kind::VarKeyword | Kind::LetKeyword | Kind::ConstKeyword | Kind::UsingKeyword => {
                self.parse_variable_statement(pos, modifiers)
            }
            Kind::FunctionKeyword => self.parse_function_declaration(pos, modifiers),
            Kind::ClassKeyword => self.parse_class_declaration(pos, modifiers),
            Kind::InterfaceKeyword => self.parse_interface_declaration(pos, modifiers),
            Kind::TypeKeyword => self.parse_type_alias_declaration(pos, modifiers),
            Kind::EnumKeyword => self.parse_enum_declaration(pos, modifiers),
            Kind::GlobalKeyword | Kind::ModuleKeyword | Kind::NamespaceKeyword => {
                self.parse_module_declaration(pos, modifiers)
            }
            Kind::ImportKeyword => {
                self.parse_import_declaration_or_import_equals_declaration(pos, modifiers)
            }
            Kind::ExportKeyword => {
                self.next_token();
                match self.token {
                    Kind::DefaultKeyword | Kind::EqualsToken => {
                        self.parse_export_assignment(pos, modifiers)
                    }
                    Kind::AsKeyword => self.parse_namespace_export_declaration(pos, modifiers),
                    _ => self.parse_export_declaration(pos, modifiers),
                }
            }
            // DEFER(phase-3): MissingDeclaration recovery / `await using` / decorator heads.
            _ => todo!("declaration worker for token {:?}", self.token),
            // blocked-by: MissingDeclaration node + decorator/await-using parsing.
        }
    }

    // Go: internal/parser/parser.go:parseFunctionDeclaration (subset)
    fn parse_function_declaration(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        self.parse_expected(Kind::FunctionKeyword);
        let asterisk_token = self.parse_optional_token(Kind::AsteriskToken);
        let has_default = modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::DEFAULT));
        let name = if !has_default || self.is_binding_identifier() {
            Some(self.parse_binding_identifier())
        } else {
            None
        };
        let signature_flags = self.signature_flags(asterisk_token, &modifiers);
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters();
        let return_type = self.parse_return_type(Kind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(signature_flags);
        let node = self.arena.new_function_declaration(
            modifiers,
            asterisk_token,
            name,
            type_parameters,
            parameters,
            return_type,
            None,
            body,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeParameters
    fn parse_type_parameters(&mut self) -> Option<NodeList> {
        if self.token == Kind::LessThanToken {
            return Some(self.parse_bracketed_list(
                ParsingContext::TypeParameters,
                |p| p.parse_type_parameter(),
                Kind::LessThanToken,
                Kind::GreaterThanToken,
            ));
        }
        None
    }

    // Go: internal/parser/parser.go:parseTypeParameter (subset: no improper-constraint expression)
    fn parse_type_parameter(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers();
        let name = self.parse_identifier();
        let constraint = if self.parse_optional(Kind::ExtendsKeyword) {
            Some(self.parse_type())
        } else {
            None
        };
        let default_type = if self.parse_optional(Kind::EqualsToken) {
            Some(self.parse_type())
        } else {
            None
        };
        let node = self.arena.new_type_parameter_declaration(
            modifiers,
            name,
            constraint,
            None,
            default_type,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseParameters
    fn parse_parameters(&mut self) -> NodeList {
        if self.parse_expected(Kind::OpenParenToken) {
            let parameters = self
                .parse_parameters_worker(ParseFlags::NONE, true)
                .expect("non-speculative parameters never fail");
            self.parse_expected(Kind::CloseParenToken);
            parameters
        } else {
            self.create_missing_list()
        }
    }

    // Go: internal/parser/parser.go:parseParameter
    fn parse_parameter(&mut self) -> NodeId {
        self.parse_parameter_ex(false, true)
            .expect("non-ambiguous parameter never fails")
    }

    // Go: internal/parser/parser.go:parseParameterEx (subset: no JSDoc/decorator parsing)
    fn parse_parameter_ex(
        &mut self,
        in_outer_await_context: bool,
        allow_ambiguity: bool,
    ) -> Option<NodeId> {
        let pos = self.node_pos();
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::AWAIT_CONTEXT, in_outer_await_context);
        let modifiers = self.parse_modifiers_ex(true, false, false);
        self.context_flags = save;
        if self.token == Kind::ThisKeyword {
            let name = self.create_identifier_with_diagnostic(true, None);
            let type_node = self.parse_type_annotation();
            let node = self
                .arena
                .new_parameter_declaration(modifiers, None, name, None, type_node, None);
            return Some(self.finish_node(node, pos));
        }
        let dot_dot_dot_token = self.parse_optional_token(Kind::DotDotDotToken);
        if !allow_ambiguity && !self.is_parameter_name_start() {
            return None;
        }
        let name = self.parse_name_of_parameter();
        let question_token = self.parse_optional_token(Kind::QuestionToken);
        let type_node = self.parse_type_annotation();
        let initializer = self.parse_initializer();
        let node = self.arena.new_parameter_declaration(
            modifiers,
            dot_dot_dot_token,
            name,
            question_token,
            type_node,
            initializer,
        );
        Some(self.finish_node(node, pos))
    }

    // Go: internal/parser/parser.go:isParameterNameStart
    fn is_parameter_name_start(&mut self) -> bool {
        self.is_binding_identifier()
            || self.token == Kind::OpenBracketToken
            || self.token == Kind::OpenBraceToken
    }

    // Go: internal/parser/parser.go:parseNameOfParameter (subset)
    fn parse_name_of_parameter(&mut self) -> NodeId {
        let name = self.parse_identifier_or_pattern();
        if self.arena.loc(name).end() == self.arena.loc(name).pos() && is_modifier_kind(self.token)
        {
            // Recover from `function foo(static)` by advancing past the stuck token.
            self.next_token();
        }
        name
    }

    // Go: internal/parser/parser.go:parseReturnType
    fn parse_return_type(&mut self, return_token: Kind, is_type: bool) -> Option<NodeId> {
        if self.should_parse_return_type(return_token, is_type) {
            Some(self.parse_type_or_type_predicate())
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseTypeOrTypePredicate
    fn parse_type_or_type_predicate(&mut self) -> NodeId {
        if self.is_identifier() {
            let state = self.mark();
            let pos = self.node_pos();
            let id = self.parse_identifier();
            if self.token == Kind::IsKeyword && !self.has_preceding_line_break() {
                self.next_token();
                let type_node = self.parse_type();
                let node = self
                    .arena
                    .new_type_predicate_node(None, id, Some(type_node));
                return self.finish_node(node, pos);
            }
            self.rewind(state);
        }
        self.parse_type()
    }

    // Go: internal/parser/parser.go:shouldParseReturnType (subset)
    fn should_parse_return_type(&mut self, return_token: Kind, is_type: bool) -> bool {
        if return_token == Kind::EqualsGreaterThanToken {
            self.parse_expected(return_token);
            return true;
        }
        if self.parse_optional(Kind::ColonToken) {
            return true;
        }
        if is_type && self.token == Kind::EqualsGreaterThanToken {
            self.parse_error_at_current_token(
                &diagnostics::X_0_EXPECTED,
                vec![tsgo_scanner::token_to_string(Kind::ColonToken).to_string()],
            );
            self.next_token();
            return true;
        }
        false
    }

    // Go: internal/parser/parser.go:parseFunctionBlockOrSemicolon (subset: no Type flag)
    fn parse_function_block_or_semicolon(&mut self, flags: ParseFlags) -> Option<NodeId> {
        if self.token != Kind::OpenBraceToken && self.can_parse_semicolon() {
            self.parse_semicolon();
            return None;
        }
        Some(self.parse_function_block(flags))
    }

    // Go: internal/parser/parser.go:parseFunctionBlock
    fn parse_function_block(&mut self, flags: ParseFlags) -> NodeId {
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::YIELD_CONTEXT, flags.contains(ParseFlags::YIELD));
        self.set_context_flags(NodeFlags::AWAIT_CONTEXT, flags.contains(ParseFlags::AWAIT));
        self.set_context_flags(NodeFlags::DECORATOR_CONTEXT, false);
        let block = self.parse_block(false, None);
        self.context_flags = save;
        block
    }

    // Computes the yield/await signature flags for a function-like body.
    // Go: internal/parser/parser.go (signatureFlags expressions inline)
    fn signature_flags(
        &self,
        asterisk_token: Option<NodeId>,
        modifiers: &Option<ModifierList>,
    ) -> ParseFlags {
        let mut flags = ParseFlags::NONE;
        if asterisk_token.is_some() {
            flags |= ParseFlags::YIELD;
        }
        if modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::ASYNC))
        {
            flags |= ParseFlags::AWAIT;
        }
        flags
    }

    // Go: internal/parser/parser.go:createMissingList (empty-list stand-in)
    //
    // DEFER(phase-3): missing-list sentinel (`isMissingNodeList`).
    // blocked-by: NodeList missing-flag representation.
    fn create_missing_list(&mut self) -> NodeList {
        self.new_node_list(TextRange::new(self.node_pos(), self.node_pos()), Vec::new())
    }

    // ---- classes ----

    // Go: internal/parser/parser.go:parseClassDeclaration
    fn parse_class_declaration(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        self.parse_class_declaration_or_expression(pos, modifiers, Kind::ClassDeclaration)
    }

    // Go: internal/parser/parser.go:parseClassExpression
    fn parse_class_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_class_declaration_or_expression(pos, None, Kind::ClassExpression)
    }

    // Go: internal/parser/parser.go:parseClassDeclarationOrExpression
    fn parse_class_declaration_or_expression(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
        kind: Kind,
    ) -> NodeId {
        let save = self.context_flags;
        self.parse_expected(Kind::ClassKeyword);
        let name = self.parse_name_of_class_declaration_or_expression();
        let type_parameters = self.parse_type_parameters();
        if modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::EXPORT))
        {
            self.set_context_flags(NodeFlags::AWAIT_CONTEXT, true);
        }
        let heritage_clauses = self.parse_heritage_clauses();
        let members = if self.parse_expected(Kind::OpenBraceToken) {
            let m = self.parse_list(ParsingContext::ClassMembers, |p| p.parse_class_element());
            self.parse_expected(Kind::CloseBraceToken);
            m
        } else {
            self.create_missing_list()
        };
        self.context_flags = save;
        let node = self.arena.new_class_like(
            kind,
            modifiers,
            name,
            type_parameters,
            heritage_clauses,
            members,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseNameOfClassDeclarationOrExpression
    fn parse_name_of_class_declaration_or_expression(&mut self) -> Option<NodeId> {
        if self.is_binding_identifier() && !self.is_implements_clause() {
            let is_id = self.is_binding_identifier();
            Some(self.create_identifier_with_diagnostic(is_id, None))
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:isImplementsClause
    fn is_implements_clause(&mut self) -> bool {
        self.token == Kind::ImplementsKeyword
            && self.look_ahead(|p| {
                p.next_token();
                token_is_identifier_or_keyword(p.token)
            })
    }

    // Go: internal/parser/parser.go:parseHeritageClauses
    fn parse_heritage_clauses(&mut self) -> Option<NodeList> {
        if self.is_heritage_clause() {
            Some(self.parse_list(ParsingContext::HeritageClauses, |p| {
                p.parse_heritage_clause()
            }))
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseHeritageClause
    fn parse_heritage_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        let kind = self.token;
        self.next_token();
        let types = self.parse_delimited_list(ParsingContext::HeritageClauseElement, |p| {
            p.parse_expression_with_type_arguments()
        });
        let node = self.arena.new_heritage_clause(kind, types);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseExpressionWithTypeArguments
    fn parse_expression_with_type_arguments(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = self.parse_left_hand_side_expression_or_higher();
        if self.arena.kind(expression) == Kind::ExpressionWithTypeArguments {
            return expression;
        }
        let type_arguments = self.parse_type_arguments();
        let node = self
            .arena
            .new_expression_with_type_arguments(expression, type_arguments);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseClassElement (subset)
    fn parse_class_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.token == Kind::SemicolonToken {
            self.next_token();
            let node = self.arena.new_semicolon_class_element();
            return self.finish_node(node, pos);
        }
        let modifiers = self.parse_modifiers_ex(true, true, true);
        if self.token == Kind::StaticKeyword
            && self.look_ahead(|p| p.next_token() == Kind::OpenBraceToken)
        {
            return self.parse_class_static_block_declaration(pos, modifiers);
        }
        if self.parse_contextual_modifier(Kind::GetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, Kind::GetAccessor);
        }
        if self.parse_contextual_modifier(Kind::SetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, Kind::SetAccessor);
        }
        if self.token == Kind::ConstructorKeyword {
            return self.parse_constructor_declaration(pos, modifiers);
        }
        if self.is_index_signature() {
            return self.parse_index_signature_declaration(pos, modifiers);
        }
        if token_is_identifier_or_keyword(self.token)
            || self.token == Kind::StringLiteral
            || self.token == Kind::NumericLiteral
            || self.token == Kind::BigIntLiteral
            || self.token == Kind::AsteriskToken
            || self.token == Kind::OpenBracketToken
        {
            return self.parse_property_or_method_declaration(pos, modifiers);
        }
        if modifiers.is_some() {
            self.parse_error_at(
                self.node_pos(),
                self.node_pos(),
                &diagnostics::DECLARATION_EXPECTED,
                Vec::new(),
            );
            let name = self.create_missing_identifier();
            return self.parse_property_declaration(pos, modifiers, name, None);
        }
        panic!("Should not have attempted to parse class member declaration.");
    }

    // Go: internal/parser/parser.go:parseClassStaticBlockDeclaration
    fn parse_class_static_block_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        self.parse_expected_token(Kind::StaticKeyword);
        let body = self.parse_class_static_block_body();
        let node = self
            .arena
            .new_class_static_block_declaration(modifiers, body);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseClassStaticBlockBody
    fn parse_class_static_block_body(&mut self) -> NodeId {
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::YIELD_CONTEXT, false);
        self.set_context_flags(NodeFlags::AWAIT_CONTEXT, true);
        let body = self.parse_block(false, None);
        self.context_flags = save;
        body
    }

    // Go: internal/parser/parser.go:tryParseConstructorDeclaration (subset: keyword only)
    fn parse_constructor_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        self.parse_expected(Kind::ConstructorKeyword);
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters();
        let return_type = self.parse_return_type(Kind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(ParseFlags::NONE);
        let node = self.arena.new_constructor_declaration(
            modifiers,
            type_parameters,
            parameters,
            return_type,
            None,
            body,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parsePropertyOrMethodDeclaration
    fn parse_property_or_method_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        let asterisk_token = self.parse_optional_token(Kind::AsteriskToken);
        let name = self.parse_property_name();
        let question_token = self.parse_optional_token(Kind::QuestionToken);
        if asterisk_token.is_some()
            || self.token == Kind::OpenParenToken
            || self.token == Kind::LessThanToken
        {
            return self.parse_method_declaration(
                pos,
                modifiers,
                asterisk_token,
                name,
                question_token,
            );
        }
        self.parse_property_declaration(pos, modifiers, name, question_token)
    }

    // Go: internal/parser/parser.go:parseMethodDeclaration
    fn parse_method_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
        asterisk_token: Option<NodeId>,
        name: NodeId,
        postfix_token: Option<NodeId>,
    ) -> NodeId {
        let signature_flags = self.signature_flags(asterisk_token, &modifiers);
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters();
        let type_node = self.parse_return_type(Kind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(signature_flags);
        let node = self.arena.new_method_declaration(
            modifiers,
            asterisk_token,
            name,
            postfix_token,
            type_parameters,
            parameters,
            type_node,
            None,
            body,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parsePropertyDeclaration (subset)
    fn parse_property_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
        name: NodeId,
        question_token: Option<NodeId>,
    ) -> NodeId {
        let postfix_token = if question_token.is_none() && !self.has_preceding_line_break() {
            self.parse_optional_token(Kind::ExclamationToken)
        } else {
            question_token
        };
        let type_node = self.parse_type_annotation();
        let save = self.context_flags;
        self.set_context_flags(
            NodeFlags::YIELD_CONTEXT | NodeFlags::AWAIT_CONTEXT | NodeFlags::DISALLOW_IN_CONTEXT,
            false,
        );
        let initializer = self.parse_initializer();
        self.context_flags = save;
        // DEFER(phase-3): full parseSemicolonAfterPropertyName recovery messages.
        // blocked-by: grammar-error reporting parity.
        self.parse_semicolon();
        let node = self.arena.new_property_declaration(
            modifiers,
            name,
            postfix_token,
            type_node,
            initializer,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseAccessorDeclaration (subset)
    fn parse_accessor_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
        kind: Kind,
    ) -> NodeId {
        let name = self.parse_property_name();
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters();
        let return_type = self.parse_return_type(Kind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(ParseFlags::NONE);
        let node = self.arena.new_accessor_declaration(
            kind,
            modifiers,
            name,
            type_parameters,
            parameters,
            return_type,
            None,
            body,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseIndexSignatureDeclaration
    fn parse_index_signature_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        let parameters = self.parse_bracketed_list(
            ParsingContext::Parameters,
            |p| p.parse_parameter(),
            Kind::OpenBracketToken,
            Kind::CloseBracketToken,
        );
        let type_node = self.parse_type_annotation();
        self.parse_type_member_semicolon();
        let node = self
            .arena
            .new_index_signature_declaration(modifiers, parameters, type_node);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeMemberSemicolon
    fn parse_type_member_semicolon(&mut self) {
        if self.parse_optional(Kind::CommaToken) {
            return;
        }
        self.parse_semicolon();
    }

    // Go: internal/parser/parser.go:parseContextualModifier
    fn parse_contextual_modifier(&mut self, t: Kind) -> bool {
        let state = self.mark();
        if self.token == t && self.next_token_can_follow_modifier() {
            return true;
        }
        self.rewind(state);
        false
    }

    // Go: internal/parser/parser.go:isIndexSignature
    fn is_index_signature(&mut self) -> bool {
        self.token == Kind::OpenBracketToken
            && self.look_ahead(|p| p.next_is_unambiguously_index_signature())
    }

    // Go: internal/parser/parser.go:nextIsUnambiguouslyIndexSignature
    fn next_is_unambiguously_index_signature(&mut self) -> bool {
        self.next_token();
        if self.token == Kind::DotDotDotToken || self.token == Kind::CloseBracketToken {
            return true;
        }
        if is_modifier_kind(self.token) {
            self.next_token();
            if self.is_identifier() {
                return true;
            }
        } else if !self.is_identifier() {
            return false;
        } else {
            self.next_token();
        }
        if self.token == Kind::ColonToken || self.token == Kind::CommaToken {
            return true;
        }
        if self.token != Kind::QuestionToken {
            return false;
        }
        self.next_token();
        self.token == Kind::ColonToken
            || self.token == Kind::CommaToken
            || self.token == Kind::CloseBracketToken
    }

    // Go: internal/parser/parser.go:isHeritageClause
    fn is_heritage_clause(&self) -> bool {
        self.token == Kind::ExtendsKeyword || self.token == Kind::ImplementsKeyword
    }

    // Go: internal/parser/parser.go:isHeritageClauseExtendsOrImplementsKeyword (subset)
    fn is_heritage_clause_extends_or_implements_keyword(&self) -> bool {
        self.token == Kind::ImplementsKeyword || self.token == Kind::ExtendsKeyword
    }

    // Go: internal/parser/parser.go:isValidHeritageClauseObjectLiteral
    fn is_valid_heritage_clause_object_literal(&mut self) -> bool {
        self.look_ahead(|p| {
            if p.next_token() == Kind::CloseBraceToken {
                let next = p.next_token();
                next == Kind::CommaToken
                    || next == Kind::OpenBraceToken
                    || next == Kind::ExtendsKeyword
                    || next == Kind::ImplementsKeyword
            } else {
                true
            }
        })
    }

    // Go: internal/parser/parser.go:scanClassMemberStart
    fn scan_class_member_start(&mut self) -> bool {
        let mut id_token = Kind::Unknown;
        if self.token == Kind::AtToken {
            return true;
        }
        while is_modifier_kind(self.token) {
            id_token = self.token;
            if is_class_member_modifier(id_token) {
                return true;
            }
            self.next_token();
        }
        if self.token == Kind::AsteriskToken {
            return true;
        }
        if self.is_literal_property_name() {
            id_token = self.token;
            self.next_token();
        }
        if self.token == Kind::OpenBracketToken {
            return true;
        }
        if id_token != Kind::Unknown {
            if !is_keyword(id_token) || id_token == Kind::SetKeyword || id_token == Kind::GetKeyword
            {
                return true;
            }
            match self.token {
                Kind::OpenParenToken
                | Kind::LessThanToken
                | Kind::ExclamationToken
                | Kind::ColonToken
                | Kind::EqualsToken
                | Kind::QuestionToken => return true,
                _ => {}
            }
            return self.can_parse_semicolon();
        }
        false
    }

    // ---- interface / type alias / enum / type members ----

    // Go: internal/parser/parser.go:parseInterfaceDeclaration
    fn parse_interface_declaration(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        self.parse_expected(Kind::InterfaceKeyword);
        let name = self.parse_identifier();
        let type_parameters = self.parse_type_parameters();
        let heritage_clauses = self.parse_heritage_clauses();
        let members = self.parse_object_type_members();
        let node = self.arena.new_interface_declaration(
            modifiers,
            Some(name),
            type_parameters,
            heritage_clauses,
            members,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeAliasDeclaration (subset: no intrinsic)
    fn parse_type_alias_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        self.parse_expected(Kind::TypeKeyword);
        let name = self.parse_identifier();
        let type_parameters = self.parse_type_parameters();
        self.parse_expected(Kind::EqualsToken);
        let type_node = self.parse_type();
        self.parse_semicolon();
        let node =
            self.arena
                .new_type_alias_declaration(modifiers, name, type_parameters, type_node);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseEnumDeclaration
    fn parse_enum_declaration(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        self.parse_expected(Kind::EnumKeyword);
        let name = self.parse_identifier();
        let members = if self.parse_expected(Kind::OpenBraceToken) {
            let save = self.context_flags;
            self.set_context_flags(NodeFlags::YIELD_CONTEXT | NodeFlags::AWAIT_CONTEXT, false);
            let m =
                self.parse_delimited_list(ParsingContext::EnumMembers, |p| p.parse_enum_member());
            self.context_flags = save;
            self.parse_expected(Kind::CloseBraceToken);
            m
        } else {
            self.create_missing_list()
        };
        let node = self.arena.new_enum_declaration(modifiers, name, members);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseEnumMember
    fn parse_enum_member(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_property_name();
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::DISALLOW_IN_CONTEXT, false);
        let initializer = self.parse_initializer();
        self.context_flags = save;
        let node = self.arena.new_enum_member(name, initializer);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseObjectTypeMembers
    fn parse_object_type_members(&mut self) -> NodeList {
        if self.parse_expected(Kind::OpenBraceToken) {
            let members = self.parse_list(ParsingContext::TypeMembers, |p| p.parse_type_member());
            self.parse_expected(Kind::CloseBraceToken);
            members
        } else {
            self.create_missing_list()
        }
    }

    // Go: internal/parser/parser.go:parseTypeLiteral
    fn parse_type_literal(&mut self) -> NodeId {
        let pos = self.node_pos();
        let members = self.parse_object_type_members();
        let node = self.arena.new_type_literal_node(members);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeMember
    fn parse_type_member(&mut self) -> NodeId {
        if self.token == Kind::OpenParenToken || self.token == Kind::LessThanToken {
            return self.parse_signature_member(Kind::CallSignature);
        }
        if self.token == Kind::NewKeyword
            && self.look_ahead(|p| p.next_token_is_open_paren_or_less_than())
        {
            return self.parse_signature_member(Kind::ConstructSignature);
        }
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers();
        if self.parse_contextual_modifier(Kind::GetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, Kind::GetAccessor);
        }
        if self.parse_contextual_modifier(Kind::SetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, Kind::SetAccessor);
        }
        if self.is_index_signature() {
            return self.parse_index_signature_declaration(pos, modifiers);
        }
        self.parse_property_or_method_signature(pos, modifiers)
    }

    // Go: internal/parser/parser.go:nextTokenIsOpenParenOrLessThan
    fn next_token_is_open_paren_or_less_than(&mut self) -> bool {
        self.next_token();
        self.token == Kind::OpenParenToken || self.token == Kind::LessThanToken
    }

    // Go: internal/parser/parser.go:parseSignatureMember
    fn parse_signature_member(&mut self, kind: Kind) -> NodeId {
        let pos = self.node_pos();
        if kind == Kind::ConstructSignature {
            self.parse_expected(Kind::NewKeyword);
        }
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters();
        let type_node = self.parse_return_type(Kind::ColonToken, true);
        self.parse_type_member_semicolon();
        let node =
            self.arena
                .new_signature_declaration(kind, type_parameters, parameters, type_node);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parsePropertyOrMethodSignature
    fn parse_property_or_method_signature(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        let name = self.parse_property_name();
        let question_token = self.parse_optional_token(Kind::QuestionToken);
        let node = if self.token == Kind::OpenParenToken || self.token == Kind::LessThanToken {
            let type_parameters = self.parse_type_parameters();
            let parameters = self.parse_parameters();
            let return_type = self.parse_return_type(Kind::ColonToken, true);
            self.arena.new_method_signature(
                modifiers,
                name,
                question_token,
                type_parameters,
                parameters,
                return_type,
            )
        } else {
            let type_node = self.parse_type_annotation();
            let initializer = if self.token == Kind::EqualsToken {
                self.parse_initializer()
            } else {
                None
            };
            self.arena.new_property_signature(
                modifiers,
                name,
                question_token,
                type_node,
                initializer,
            )
        };
        self.parse_type_member_semicolon();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:scanTypeMemberStart
    fn scan_type_member_start(&mut self) -> bool {
        if self.token == Kind::OpenParenToken
            || self.token == Kind::LessThanToken
            || self.token == Kind::GetKeyword
            || self.token == Kind::SetKeyword
        {
            return true;
        }
        let mut id_token = false;
        while is_modifier_kind(self.token) {
            id_token = true;
            self.next_token();
        }
        if self.token == Kind::OpenBracketToken {
            return true;
        }
        if self.is_literal_property_name() {
            id_token = true;
            self.next_token();
        }
        if id_token {
            return self.token == Kind::OpenParenToken
                || self.token == Kind::LessThanToken
                || self.token == Kind::QuestionToken
                || self.token == Kind::ColonToken
                || self.token == Kind::CommaToken
                || self.can_parse_semicolon();
        }
        false
    }

    // ---- module / namespace ----

    // Go: internal/parser/parser.go:parseModuleDeclaration
    fn parse_module_declaration(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        let mut keyword = Kind::ModuleKeyword;
        if self.token == Kind::GlobalKeyword {
            return self.parse_ambient_external_module_declaration(pos, modifiers);
        } else if self.parse_optional(Kind::NamespaceKeyword) {
            keyword = Kind::NamespaceKeyword;
        } else {
            self.parse_expected(Kind::ModuleKeyword);
            if self.token == Kind::StringLiteral {
                return self.parse_ambient_external_module_declaration(pos, modifiers);
            }
        }
        self.parse_module_or_namespace_declaration(pos, modifiers, false, keyword)
    }

    // Go: internal/parser/parser.go:parseAmbientExternalModuleDeclaration
    fn parse_ambient_external_module_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        let (name, keyword) = if self.token == Kind::GlobalKeyword {
            (self.parse_identifier(), Kind::GlobalKeyword)
        } else {
            (self.parse_literal_expression(), Kind::ModuleKeyword)
        };
        let body = if self.token == Kind::OpenBraceToken {
            Some(self.parse_module_block())
        } else {
            self.parse_semicolon();
            None
        };
        let node = self
            .arena
            .new_module_declaration(modifiers, keyword, name, body);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseModuleBlock
    fn parse_module_block(&mut self) -> NodeId {
        let pos = self.node_pos();
        let statements = if self.parse_expected(Kind::OpenBraceToken) {
            let s = self.parse_list(ParsingContext::BlockStatements, |p| p.parse_statement());
            self.parse_expected(Kind::CloseBraceToken);
            s
        } else {
            self.create_missing_list()
        };
        let node = self.arena.new_module_block(statements);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseModuleOrNamespaceDeclaration
    fn parse_module_or_namespace_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
        nested: bool,
        keyword: Kind,
    ) -> NodeId {
        let name = if nested {
            self.parse_identifier_name()
        } else {
            self.parse_identifier()
        };
        let body = if self.parse_optional(Kind::DotToken) {
            // Nested `a.b` namespaces get an implicit `export` modifier.
            let mpos = self.node_pos();
            let export_token = self.arena.new_token(Kind::ExportKeyword);
            self.arena.set_loc(export_token, TextRange::new(mpos, mpos));
            self.arena.add_flags(export_token, NodeFlags::REPARSED);
            let implicit = self.new_modifier_list(
                TextRange::new(mpos, mpos),
                vec![export_token],
                ModifierFlags::EXPORT,
            );
            let inner_pos = self.node_pos();
            self.parse_module_or_namespace_declaration(inner_pos, Some(implicit), true, keyword)
        } else {
            self.parse_module_block()
        };
        let node = self
            .arena
            .new_module_declaration(modifiers, keyword, name, Some(body));
        self.finish_node(node, pos)
    }

    // ---- import / export ----
    //
    // DEFER(phase-3): import attributes (`with { ... }`), `defer` phase modifier,
    // and the full specifier-level `type as as` disambiguation.
    // blocked-by: import-attributes parser + specifier type-modifier disambiguation.

    // Go: internal/parser/parser.go:parseImportDeclarationOrImportEqualsDeclaration (subset)
    fn parse_import_declaration_or_import_equals_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        self.parse_expected(Kind::ImportKeyword);
        let after_import_pos = self.node_pos();
        let mut identifier = if self.is_identifier() {
            Some(self.parse_identifier())
        } else {
            None
        };
        let mut phase_modifier = Kind::Unknown;
        if let Some(id) = identifier {
            if self.arena.text(id) == "type"
                && (self.is_identifier()
                    || self.token == Kind::AsteriskToken
                    || self.token == Kind::OpenBraceToken)
                && self.token != Kind::FromKeyword
            {
                phase_modifier = Kind::TypeKeyword;
                identifier = if self.is_identifier() {
                    Some(self.parse_identifier())
                } else {
                    None
                };
            }
        }
        if let Some(id) = identifier {
            if self.token != Kind::CommaToken && self.token != Kind::FromKeyword {
                return self.parse_import_equals_declaration(
                    pos,
                    modifiers,
                    id,
                    phase_modifier == Kind::TypeKeyword,
                );
            }
        }
        let import_clause =
            self.try_parse_import_clause(identifier, after_import_pos, phase_modifier);
        let module_specifier = self.parse_module_specifier();
        let attributes = self.try_parse_import_attributes();
        self.parse_semicolon();
        let node = self.arena.new_import_declaration(
            modifiers,
            import_clause,
            module_specifier,
            attributes,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:tryParseImportAttributes
    fn try_parse_import_attributes(&mut self) -> Option<NodeId> {
        if self.token == Kind::WithKeyword
            || (self.token == Kind::AssertKeyword && !self.has_preceding_line_break())
        {
            if self.token == Kind::AssertKeyword {
                self.parse_error_at_current_token(
                    &diagnostics::IMPORT_ASSERTIONS_HAVE_BEEN_REPLACED_BY_IMPORT_ATTRIBUTES_USE_WITH_INSTEAD_OF_ASSERT,
                    Vec::new(),
                );
            }
            let token = self.token;
            Some(self.parse_import_attributes(token, false))
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseImportAttributes
    fn parse_import_attributes(&mut self, token: Kind, skip_keyword: bool) -> NodeId {
        let pos = self.node_pos();
        if !skip_keyword {
            self.parse_expected(token);
        }
        let mut multiline = false;
        let elements = if self.parse_expected(Kind::OpenBraceToken) {
            multiline = self.has_preceding_line_break();
            let elements = self.parse_delimited_list(ParsingContext::ImportAttributes, |p| {
                p.parse_import_attribute()
            });
            self.parse_expected(Kind::CloseBraceToken);
            elements
        } else {
            self.new_node_list(TextRange::new(self.node_pos(), self.node_pos()), Vec::new())
        };
        let node = self.arena.new_import_attributes(token, elements, multiline);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseImportAttribute
    fn parse_import_attribute(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = if token_is_identifier_or_keyword(self.token) {
            Some(self.parse_identifier_name())
        } else if self.token == Kind::StringLiteral {
            Some(self.parse_literal_expression())
        } else {
            None
        };
        if name.is_some() {
            self.parse_expected(Kind::ColonToken);
        } else {
            self.parse_error_at_current_token(
                &diagnostics::IDENTIFIER_OR_STRING_LITERAL_EXPECTED,
                Vec::new(),
            );
        }
        let value = self.parse_assignment_expression_or_higher();
        let node = self.arena.new_import_attribute(name, value);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseImportEqualsDeclaration
    fn parse_import_equals_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
        identifier: NodeId,
        is_type_only: bool,
    ) -> NodeId {
        self.parse_expected(Kind::EqualsToken);
        let module_reference = self.parse_module_reference();
        self.parse_semicolon();
        let node = self.arena.new_import_equals_declaration(
            modifiers,
            is_type_only,
            identifier,
            module_reference,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseModuleReference
    fn parse_module_reference(&mut self) -> NodeId {
        if self.token == Kind::RequireKeyword
            && self.look_ahead(|p| p.next_token() == Kind::OpenParenToken)
        {
            self.parse_external_module_reference()
        } else {
            self.parse_entity_name(false, None)
        }
    }

    // Go: internal/parser/parser.go:parseExternalModuleReference
    fn parse_external_module_reference(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::RequireKeyword);
        self.parse_expected(Kind::OpenParenToken);
        let expression = self.parse_module_specifier();
        self.parse_expected(Kind::CloseParenToken);
        let node = self.arena.new_external_module_reference(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseModuleSpecifier
    fn parse_module_specifier(&mut self) -> NodeId {
        if self.token == Kind::StringLiteral {
            self.parse_literal_expression()
        } else {
            self.parse_expression()
        }
    }

    // Go: internal/parser/parser.go:tryParseImportClause
    fn try_parse_import_clause(
        &mut self,
        identifier: Option<NodeId>,
        pos: i32,
        phase_modifier: Kind,
    ) -> Option<NodeId> {
        if identifier.is_some()
            || self.token == Kind::AsteriskToken
            || self.token == Kind::OpenBraceToken
        {
            let import_clause = self.parse_import_clause(identifier, pos, phase_modifier);
            self.parse_expected(Kind::FromKeyword);
            Some(import_clause)
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseImportClause
    fn parse_import_clause(
        &mut self,
        identifier: Option<NodeId>,
        pos: i32,
        phase_modifier: Kind,
    ) -> NodeId {
        let named_bindings = if identifier.is_none() || self.parse_optional(Kind::CommaToken) {
            if self.token == Kind::AsteriskToken {
                Some(self.parse_namespace_import())
            } else {
                Some(self.parse_named_imports())
            }
        } else {
            None
        };
        let node = self
            .arena
            .new_import_clause(phase_modifier, identifier, named_bindings);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseNamespaceImport
    fn parse_namespace_import(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::AsteriskToken);
        self.parse_expected(Kind::AsKeyword);
        let name = self.parse_identifier();
        let node = self.arena.new_namespace_import(name);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseNamedImports
    fn parse_named_imports(&mut self) -> NodeId {
        let pos = self.node_pos();
        let imports = self.parse_bracketed_list(
            ParsingContext::ImportOrExportSpecifiers,
            |p| p.parse_import_specifier(),
            Kind::OpenBraceToken,
            Kind::CloseBraceToken,
        );
        let node = self.arena.new_named_imports(imports);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseImportSpecifier
    fn parse_import_specifier(&mut self) -> NodeId {
        let pos = self.node_pos();
        let (is_type_only, property_name, name) =
            self.parse_import_or_export_specifier(Kind::ImportSpecifier);
        let node = self
            .arena
            .new_import_specifier(is_type_only, property_name, name);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseExportSpecifier
    fn parse_export_specifier(&mut self) -> NodeId {
        let pos = self.node_pos();
        let (is_type_only, property_name, name) =
            self.parse_import_or_export_specifier(Kind::ExportSpecifier);
        let node = self
            .arena
            .new_export_specifier(is_type_only, property_name, name);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseImportOrExportSpecifier (subset)
    fn parse_import_or_export_specifier(&mut self, kind: Kind) -> (bool, Option<NodeId>, NodeId) {
        let disallow_keywords = kind == Kind::ImportSpecifier;
        let mut is_type_only = false;
        let mut property_name = None;
        let mut name = self.parse_module_export_name(disallow_keywords);
        if self.arena.kind(name) == Kind::Identifier
            && self.arena.text(name) == "type"
            && self.token != Kind::AsKeyword
            && self.can_parse_module_export_name()
        {
            is_type_only = true;
            name = self.parse_module_export_name(disallow_keywords);
        }
        if self.token == Kind::AsKeyword {
            property_name = Some(name);
            self.parse_expected(Kind::AsKeyword);
            name = self.parse_module_export_name(disallow_keywords);
        }
        (is_type_only, property_name, name)
    }

    // Go: internal/parser/parser.go:canParseModuleExportName
    fn can_parse_module_export_name(&self) -> bool {
        token_is_identifier_or_keyword(self.token) || self.token == Kind::StringLiteral
    }

    // Go: internal/parser/parser.go:parseModuleExportName (subset)
    fn parse_module_export_name(&mut self, _disallow_keywords: bool) -> NodeId {
        if self.token == Kind::StringLiteral {
            self.parse_literal_expression()
        } else {
            self.parse_identifier_name()
        }
    }

    // Go: internal/parser/parser.go:parseExportAssignment
    fn parse_export_assignment(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::AWAIT_CONTEXT, true);
        let is_export_equals = if self.parse_optional(Kind::EqualsToken) {
            true
        } else {
            self.parse_expected(Kind::DefaultKeyword);
            false
        };
        let expression = self.parse_assignment_expression_or_higher();
        self.parse_semicolon();
        self.context_flags = save;
        let node = self
            .arena
            .new_export_assignment(modifiers, is_export_equals, None, expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseNamespaceExportDeclaration
    fn parse_namespace_export_declaration(
        &mut self,
        pos: i32,
        modifiers: Option<ModifierList>,
    ) -> NodeId {
        self.parse_expected(Kind::AsKeyword);
        self.parse_expected(Kind::NamespaceKeyword);
        let name = self.parse_identifier();
        self.parse_semicolon();
        let node = self.arena.new_namespace_export_declaration(modifiers, name);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseExportDeclaration
    fn parse_export_declaration(&mut self, pos: i32, modifiers: Option<ModifierList>) -> NodeId {
        let save = self.context_flags;
        self.set_context_flags(NodeFlags::AWAIT_CONTEXT, true);
        let is_type_only = self.parse_optional(Kind::TypeKeyword);
        let namespace_export_pos = self.node_pos();
        let mut export_clause = None;
        let mut module_specifier = None;
        if self.parse_optional(Kind::AsteriskToken) {
            if self.parse_optional(Kind::AsKeyword) {
                export_clause = Some(self.parse_namespace_export(namespace_export_pos));
            }
            self.parse_expected(Kind::FromKeyword);
            module_specifier = Some(self.parse_module_specifier());
        } else {
            export_clause = Some(self.parse_named_exports());
            if self.token == Kind::FromKeyword
                || (self.token == Kind::StringLiteral && !self.has_preceding_line_break())
            {
                self.parse_expected(Kind::FromKeyword);
                module_specifier = Some(self.parse_module_specifier());
            }
        }
        let attributes = if module_specifier.is_some()
            && (self.token == Kind::WithKeyword || self.token == Kind::AssertKeyword)
            && !self.has_preceding_line_break()
        {
            if self.token == Kind::AssertKeyword {
                self.parse_error_at_current_token(
                    &diagnostics::IMPORT_ASSERTIONS_HAVE_BEEN_REPLACED_BY_IMPORT_ATTRIBUTES_USE_WITH_INSTEAD_OF_ASSERT,
                    Vec::new(),
                );
            }
            let token = self.token;
            Some(self.parse_import_attributes(token, false))
        } else {
            None
        };
        self.parse_semicolon();
        self.context_flags = save;
        let node = self.arena.new_export_declaration(
            modifiers,
            is_type_only,
            export_clause,
            module_specifier,
            attributes,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseNamespaceExport
    fn parse_namespace_export(&mut self, pos: i32) -> NodeId {
        let export_name = self.parse_module_export_name(false);
        let node = self.arena.new_namespace_export(export_name);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseNamedExports
    fn parse_named_exports(&mut self) -> NodeId {
        let pos = self.node_pos();
        let exports = self.parse_bracketed_list(
            ParsingContext::ImportOrExportSpecifiers,
            |p| p.parse_export_specifier(),
            Kind::OpenBraceToken,
            Kind::CloseBraceToken,
        );
        let node = self.arena.new_named_exports(exports);
        self.finish_node(node, pos)
    }

    // ---- types ----
    //
    // DEFER(phase-3): JSDoc-style postfix types (`?T`/`!T`/`*`), negative literal
    // types (`-1`), and import-type attributes (`with { ... }`).
    // blocked-by: JSDoc type parsing + import-attributes parser.

    // Go: internal/parser/parser.go:parseTypeAnnotation
    fn parse_type_annotation(&mut self) -> Option<NodeId> {
        if self.parse_optional(Kind::ColonToken) {
            Some(self.parse_type())
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseType
    fn parse_type(&mut self) -> NodeId {
        if self.is_start_of_function_type_or_constructor_type() {
            return self.parse_function_or_constructor_type();
        }
        let pos = self.node_pos();
        let type_node = self.parse_union_type_or_higher();
        if !self.in_context(NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT)
            && !self.has_preceding_line_break()
            && self.parse_optional(Kind::ExtendsKeyword)
        {
            let save = self.context_flags;
            self.set_context_flags(NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT, true);
            let extends_type = self.parse_type();
            self.context_flags = save;
            self.parse_expected(Kind::QuestionToken);
            self.set_context_flags(NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT, false);
            let true_type = self.parse_type();
            self.context_flags = save;
            self.parse_expected(Kind::ColonToken);
            self.set_context_flags(NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT, false);
            let false_type = self.parse_type();
            self.context_flags = save;
            let node = self.arena.new_conditional_type_node(
                type_node,
                extends_type,
                true_type,
                false_type,
            );
            return self.finish_node(node, pos);
        }
        type_node
    }

    // Go: internal/parser/parser.go:isStartOfFunctionTypeOrConstructorType
    fn is_start_of_function_type_or_constructor_type(&mut self) -> bool {
        self.token == Kind::LessThanToken
            || (self.token == Kind::OpenParenToken
                && self.look_ahead(|p| p.next_is_unambiguously_start_of_function_type()))
            || self.token == Kind::NewKeyword
            || (self.token == Kind::AbstractKeyword
                && self.look_ahead(|p| p.next_token() == Kind::NewKeyword))
    }

    // Go: internal/parser/parser.go:nextIsUnambiguouslyStartOfFunctionType
    fn next_is_unambiguously_start_of_function_type(&mut self) -> bool {
        self.next_token();
        if self.token == Kind::CloseParenToken || self.token == Kind::DotDotDotToken {
            return true;
        }
        if self.skip_parameter_start() {
            if self.token == Kind::ColonToken
                || self.token == Kind::CommaToken
                || self.token == Kind::QuestionToken
                || self.token == Kind::EqualsToken
            {
                return true;
            }
            if self.token == Kind::CloseParenToken
                && self.next_token() == Kind::EqualsGreaterThanToken
            {
                return true;
            }
        }
        false
    }

    // Go: internal/parser/parser.go:skipParameterStart
    fn skip_parameter_start(&mut self) -> bool {
        if is_modifier_kind(self.token) {
            self.parse_modifiers();
        }
        self.parse_optional(Kind::DotDotDotToken);
        if self.is_identifier() || self.token == Kind::ThisKeyword {
            self.next_token();
            return true;
        }
        if self.token == Kind::OpenBracketToken || self.token == Kind::OpenBraceToken {
            let previous_error_count = self.diagnostics.len();
            self.parse_identifier_or_pattern();
            return previous_error_count == self.diagnostics.len();
        }
        false
    }

    // Go: internal/parser/parser.go:parseFunctionOrConstructorType
    fn parse_function_or_constructor_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers_for_constructor_type();
        let is_constructor_type = self.parse_optional(Kind::NewKeyword);
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters();
        let return_type = self.parse_return_type(Kind::EqualsGreaterThanToken, false);
        let node = if is_constructor_type {
            self.arena.new_constructor_type_node(
                modifiers,
                type_parameters,
                parameters,
                return_type,
            )
        } else {
            self.arena
                .new_function_type_node(type_parameters, parameters, return_type)
        };
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseModifiersForConstructorType
    fn parse_modifiers_for_constructor_type(&mut self) -> Option<ModifierList> {
        if self.token == Kind::AbstractKeyword {
            let pos = self.node_pos();
            let modifier = self.arena.new_token(Kind::AbstractKeyword);
            self.next_token();
            let modifier = self.finish_node(modifier, pos);
            let loc = self.arena.loc(modifier);
            Some(self.new_modifier_list(loc, vec![modifier], ModifierFlags::ABSTRACT))
        } else {
            None
        }
    }

    // Go: internal/parser/parser.go:parseUnionTypeOrHigher
    fn parse_union_type_or_higher(&mut self) -> NodeId {
        self.parse_union_or_intersection_type(
            Kind::BarToken,
            Parser::parse_intersection_type_or_higher,
        )
    }

    // Go: internal/parser/parser.go:parseIntersectionTypeOrHigher
    fn parse_intersection_type_or_higher(&mut self) -> NodeId {
        self.parse_union_or_intersection_type(
            Kind::AmpersandToken,
            Parser::parse_type_operator_or_higher,
        )
    }

    // Go: internal/parser/parser.go:parseTypeOperatorOrHigher
    fn parse_type_operator_or_higher(&mut self) -> NodeId {
        match self.token {
            Kind::KeyOfKeyword | Kind::UniqueKeyword | Kind::ReadonlyKeyword => {
                self.parse_type_operator(self.token)
            }
            Kind::InferKeyword => self.parse_infer_type(),
            _ => {
                let save = self.context_flags;
                self.set_context_flags(NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT, false);
                let result = self.parse_postfix_type_or_higher();
                self.context_flags = save;
                result
            }
        }
    }

    // Go: internal/parser/parser.go:parseTypeOperator
    fn parse_type_operator(&mut self, operator: Kind) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(operator);
        let operand = self.parse_type_operator_or_higher();
        let node = self.arena.new_type_operator_node(operator, operand);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseInferType
    fn parse_infer_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::InferKeyword);
        let type_parameter = self.parse_type_parameter_of_infer_type();
        let node = self.arena.new_infer_type_node(type_parameter);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeParameterOfInferType
    fn parse_type_parameter_of_infer_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_identifier();
        let constraint = self.try_parse_constraint_of_infer_type();
        let node = self
            .arena
            .new_type_parameter_declaration(None, name, constraint, None, None);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:tryParseConstraintOfInferType
    fn try_parse_constraint_of_infer_type(&mut self) -> Option<NodeId> {
        let state = self.mark();
        if self.parse_optional(Kind::ExtendsKeyword) {
            let save = self.context_flags;
            self.set_context_flags(NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT, true);
            let constraint = self.parse_type();
            self.context_flags = save;
            if self.in_context(NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT)
                || self.token != Kind::QuestionToken
            {
                return Some(constraint);
            }
        }
        self.rewind(state);
        None
    }

    // Go: internal/parser/parser.go:parseUnionOrIntersectionType (subset)
    fn parse_union_or_intersection_type(
        &mut self,
        operator: Kind,
        parse_constituent: fn(&mut Parser) -> NodeId,
    ) -> NodeId {
        let pos = self.node_pos();
        let has_leading_operator = self.parse_optional(operator);
        let mut type_node = parse_constituent(self);
        if self.token == operator || has_leading_operator {
            let mut types = vec![type_node];
            while self.parse_optional(operator) {
                types.push(parse_constituent(self));
            }
            let list = self.new_node_list(TextRange::new(pos, self.node_pos()), types);
            let node = if operator == Kind::BarToken {
                self.arena.new_union_type_node(list)
            } else {
                self.arena.new_intersection_type_node(list)
            };
            type_node = self.finish_node(node, pos);
        }
        type_node
    }

    // Go: internal/parser/parser.go:parsePostfixTypeOrHigher (subset: array/indexed access)
    fn parse_postfix_type_or_higher(&mut self) -> NodeId {
        let pos = self.node_pos();
        let mut type_node = self.parse_non_array_type();
        while !self.has_preceding_line_break() && self.token == Kind::OpenBracketToken {
            self.parse_expected(Kind::OpenBracketToken);
            if self.is_start_of_type() {
                let index_type = self.parse_type();
                self.parse_expected(Kind::CloseBracketToken);
                let node = self
                    .arena
                    .new_indexed_access_type_node(type_node, index_type);
                type_node = self.finish_node(node, pos);
            } else {
                self.parse_expected(Kind::CloseBracketToken);
                let node = self.arena.new_array_type_node(type_node);
                type_node = self.finish_node(node, pos);
            }
        }
        type_node
    }

    // Go: internal/parser/parser.go:parseNonArrayType (subset)
    fn parse_non_array_type(&mut self) -> NodeId {
        let token = self.token;
        match token {
            Kind::AnyKeyword
            | Kind::UnknownKeyword
            | Kind::StringKeyword
            | Kind::NumberKeyword
            | Kind::BigIntKeyword
            | Kind::SymbolKeyword
            | Kind::BooleanKeyword
            | Kind::UndefinedKeyword
            | Kind::NeverKeyword
            | Kind::ObjectKeyword
            | Kind::VoidKeyword => self.parse_keyword_type_node(),
            Kind::StringLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword => self.parse_literal_type_node(false),
            Kind::MinusToken
                if self.look_ahead(|p| p.next_token_is_numeric_or_big_int_literal()) =>
            {
                self.parse_literal_type_node(true)
            }
            Kind::OpenParenToken => self.parse_parenthesized_type(),
            Kind::OpenBraceToken => {
                if self.look_ahead(|p| p.next_is_start_of_mapped_type()) {
                    self.parse_mapped_type()
                } else {
                    self.parse_type_literal()
                }
            }
            Kind::ThisKeyword => {
                let this_keyword = self.parse_this_type_node();
                if self.token == Kind::IsKeyword && !self.has_preceding_line_break() {
                    self.parse_this_type_predicate(this_keyword)
                } else {
                    this_keyword
                }
            }
            Kind::TypeOfKeyword => {
                if self.look_ahead(|p| p.next_is_start_of_type_of_import_type()) {
                    self.parse_import_type()
                } else {
                    self.parse_type_query()
                }
            }
            Kind::OpenBracketToken => self.parse_tuple_type(),
            Kind::ImportKeyword => self.parse_import_type(),
            Kind::AssertsKeyword
                if self.look_ahead(|p| p.next_token_is_identifier_or_keyword_on_same_line()) =>
            {
                self.parse_asserts_type_predicate()
            }
            Kind::TemplateHead => self.parse_template_type(),
            // DEFER(phase-3): JSDoc `*`/`?`/`!` and negative-literal type starts.
            // blocked-by: JSDoc type parsing.
            _ => self.parse_type_reference(),
        }
    }

    // Go: internal/parser/parser.go:parseThisTypeNode
    fn parse_this_type_node(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let node = self.arena.new_this_type_node();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseThisTypePredicate
    fn parse_this_type_predicate(&mut self, lhs: NodeId) -> NodeId {
        let pos = self.arena.loc(lhs).pos();
        self.next_token();
        let type_node = self.parse_type();
        let node = self
            .arena
            .new_type_predicate_node(None, lhs, Some(type_node));
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseAssertsTypePredicate
    fn parse_asserts_type_predicate(&mut self) -> NodeId {
        let pos = self.node_pos();
        let asserts_modifier = Some(self.parse_expected_token(Kind::AssertsKeyword));
        let parameter_name = if self.token == Kind::ThisKeyword {
            self.parse_this_type_node()
        } else {
            self.parse_identifier()
        };
        let type_node = if self.parse_optional(Kind::IsKeyword) {
            Some(self.parse_type())
        } else {
            None
        };
        let node = self
            .arena
            .new_type_predicate_node(asserts_modifier, parameter_name, type_node);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeQuery
    fn parse_type_query(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::TypeOfKeyword);
        let entity_name = self.parse_entity_name(true, None);
        let type_arguments = if !self.has_preceding_line_break() {
            self.parse_type_arguments()
        } else {
            None
        };
        let node = self.arena.new_type_query_node(entity_name, type_arguments);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go (nextIsStartOfTypeOfImportType)
    fn next_is_start_of_type_of_import_type(&mut self) -> bool {
        self.next_token();
        self.token == Kind::ImportKeyword
    }

    // Go: internal/parser/parser.go:parseImportType
    fn parse_import_type(&mut self) -> NodeId {
        // DEFER(phase-3): set sourceFlags |= PossiblyContainsDynamicImport.
        // blocked-by: finishSourceFile / source-flags tracking.
        let pos = self.node_pos();
        let is_type_of = self.parse_optional(Kind::TypeOfKeyword);
        self.parse_expected(Kind::ImportKeyword);
        self.parse_expected(Kind::OpenParenToken);
        let argument = self.parse_type();
        let attributes = if self.parse_optional(Kind::CommaToken) {
            self.parse_expected(Kind::OpenBraceToken);
            let current_token = self.token;
            if current_token == Kind::WithKeyword || current_token == Kind::AssertKeyword {
                if current_token == Kind::AssertKeyword {
                    self.parse_error_at_current_token(
                        &diagnostics::IMPORT_ASSERTIONS_HAVE_BEEN_REPLACED_BY_IMPORT_ATTRIBUTES_USE_WITH_INSTEAD_OF_ASSERT,
                        Vec::new(),
                    );
                }
                self.next_token();
            } else {
                self.parse_error_at_current_token(
                    &diagnostics::X_0_EXPECTED,
                    vec![tsgo_scanner::token_to_string(Kind::WithKeyword).to_string()],
                );
            }
            self.parse_expected(Kind::ColonToken);
            let attrs = self.parse_import_attributes(current_token, true);
            self.parse_optional(Kind::CommaToken);
            self.parse_expected(Kind::CloseBraceToken);
            Some(attrs)
        } else {
            None
        };
        self.parse_expected(Kind::CloseParenToken);
        let qualifier = if self.parse_optional(Kind::DotToken) {
            Some(self.parse_entity_name_of_type_reference())
        } else {
            None
        };
        let type_arguments = self.parse_type_arguments_of_type_reference();
        let node = self.arena.new_import_type_node(
            is_type_of,
            argument,
            attributes,
            qualifier,
            type_arguments,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTupleType
    fn parse_tuple_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let elements = self.parse_bracketed_list(
            ParsingContext::TupleElementTypes,
            Parser::parse_tuple_element_name_or_tuple_element_type,
            Kind::OpenBracketToken,
            Kind::CloseBracketToken,
        );
        let node = self.arena.new_tuple_type_node(elements);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTupleElementNameOrTupleElementType
    fn parse_tuple_element_name_or_tuple_element_type(&mut self) -> NodeId {
        if self.look_ahead(|p| p.scan_start_of_named_tuple_element()) {
            let pos = self.node_pos();
            let dot_dot_dot_token = self.parse_optional_token(Kind::DotDotDotToken);
            let name = self.parse_identifier_name();
            let question_token = self.parse_optional_token(Kind::QuestionToken);
            self.parse_expected(Kind::ColonToken);
            let type_node = self.parse_tuple_element_type();
            let node = self.arena.new_named_tuple_member(
                dot_dot_dot_token,
                name,
                question_token,
                type_node,
            );
            return self.finish_node(node, pos);
        }
        self.parse_tuple_element_type()
    }

    // Go: internal/parser/parser.go:scanStartOfNamedTupleElement
    fn scan_start_of_named_tuple_element(&mut self) -> bool {
        if self.token == Kind::DotDotDotToken {
            return token_is_identifier_or_keyword(self.next_token())
                && self.next_token_is_colon_or_question_colon();
        }
        token_is_identifier_or_keyword(self.token) && self.next_token_is_colon_or_question_colon()
    }

    // Go: internal/parser/parser.go:nextTokenIsColonOrQuestionColon
    fn next_token_is_colon_or_question_colon(&mut self) -> bool {
        self.next_token() == Kind::ColonToken
            || (self.token == Kind::QuestionToken && self.next_token() == Kind::ColonToken)
    }

    // Go: internal/parser/parser.go:parseTupleElementType (subset: no JSDoc optional)
    fn parse_tuple_element_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.parse_optional(Kind::DotDotDotToken) {
            let type_node = self.parse_type();
            let node = self.arena.new_rest_type_node(type_node);
            return self.finish_node(node, pos);
        }
        self.parse_type()
    }

    // Go: internal/parser/parser.go:nextIsStartOfMappedType
    fn next_is_start_of_mapped_type(&mut self) -> bool {
        self.next_token();
        if self.token == Kind::PlusToken || self.token == Kind::MinusToken {
            return self.next_token() == Kind::ReadonlyKeyword;
        }
        if self.token == Kind::ReadonlyKeyword {
            self.next_token();
        }
        self.token == Kind::OpenBracketToken
            && self.next_token_is_identifier()
            && self.next_token() == Kind::InKeyword
    }

    // Go: internal/parser/parser.go:parseMappedType
    fn parse_mapped_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenBraceToken);
        let readonly_token = if self.token == Kind::ReadonlyKeyword
            || self.token == Kind::PlusToken
            || self.token == Kind::MinusToken
        {
            let t = self.parse_token_node();
            if self.arena.kind(t) != Kind::ReadonlyKeyword {
                self.parse_expected(Kind::ReadonlyKeyword);
            }
            Some(t)
        } else {
            None
        };
        self.parse_expected(Kind::OpenBracketToken);
        let type_parameter = self.parse_mapped_type_parameter();
        let name_type = if self.parse_optional(Kind::AsKeyword) {
            Some(self.parse_type())
        } else {
            None
        };
        self.parse_expected(Kind::CloseBracketToken);
        let question_token = if self.token == Kind::QuestionToken
            || self.token == Kind::PlusToken
            || self.token == Kind::MinusToken
        {
            let t = self.parse_token_node();
            if self.arena.kind(t) != Kind::QuestionToken {
                self.parse_expected(Kind::QuestionToken);
            }
            Some(t)
        } else {
            None
        };
        let type_node = self.parse_type_annotation();
        self.parse_semicolon();
        let members = self.parse_list(ParsingContext::TypeMembers, |p| p.parse_type_member());
        self.parse_expected(Kind::CloseBraceToken);
        let node = self.arena.new_mapped_type_node(
            readonly_token,
            type_parameter,
            name_type,
            question_token,
            type_node,
            members,
        );
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseMappedTypeParameter
    fn parse_mapped_type_parameter(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_identifier_name();
        self.parse_expected(Kind::InKeyword);
        let type_node = self.parse_type();
        let node =
            self.arena
                .new_type_parameter_declaration(None, name, Some(type_node), None, None);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTemplateType
    fn parse_template_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let head = self.parse_template_head(false);
        let template_spans = self.parse_template_type_spans();
        let node = self
            .arena
            .new_template_literal_type_node(head, template_spans);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTemplateTypeSpans
    fn parse_template_type_spans(&mut self) -> NodeList {
        let pos = self.node_pos();
        let mut list = Vec::new();
        loop {
            let span = self.parse_template_type_span();
            let literal = match self.arena.data(span) {
                tsgo_ast::NodeData::TemplateLiteralTypeSpan(d) => d.literal,
                _ => unreachable!("parse_template_type_span returns a TemplateLiteralTypeSpan"),
            };
            let is_middle = self.arena.kind(literal) == Kind::TemplateMiddle;
            list.push(span);
            if !is_middle {
                break;
            }
        }
        let end = self.node_pos();
        self.new_node_list(TextRange::new(pos, end), list)
    }

    // Go: internal/parser/parser.go:parseTemplateTypeSpan
    fn parse_template_type_span(&mut self) -> NodeId {
        let pos = self.node_pos();
        let type_node = self.parse_type();
        let literal = self.parse_literal_of_template_span(false);
        let node = self
            .arena
            .new_template_literal_type_span(type_node, literal);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseKeywordTypeNode
    //
    // A keyword type (`number`, `string`, ...) is represented as a kind-carrying
    // token node: Go's `KeywordTypeNode` has no fields or children, so the
    // child-less `Token` payload is structurally identical (the keyword kind is
    // preserved on `Node.kind`).
    fn parse_keyword_type_node(&mut self) -> NodeId {
        let pos = self.node_pos();
        let kind = self.token;
        let node = self.arena.new_token(kind);
        self.next_token();
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseLiteralTypeNode (subset: non-negative)
    fn parse_literal_type_node(&mut self, negative: bool) -> NodeId {
        let pos = self.node_pos();
        if negative {
            self.next_token();
        }
        let mut expression = if self.token == Kind::TrueKeyword
            || self.token == Kind::FalseKeyword
            || self.token == Kind::NullKeyword
        {
            self.parse_keyword_expression()
        } else {
            self.parse_literal_expression()
        };
        if negative {
            let node = self
                .arena
                .new_prefix_unary_expression(Kind::MinusToken, expression);
            expression = self.finish_node(node, pos);
        }
        let node = self.arena.new_literal_type_node(expression);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:nextTokenIsNumericOrBigIntLiteral
    fn next_token_is_numeric_or_big_int_literal(&mut self) -> bool {
        self.next_token();
        self.token == Kind::NumericLiteral || self.token == Kind::BigIntLiteral
    }

    // Go: internal/parser/parser.go:parseParenthesizedType
    fn parse_parenthesized_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(Kind::OpenParenToken);
        let type_node = self.parse_type();
        self.parse_expected(Kind::CloseParenToken);
        let node = self.arena.new_parenthesized_type_node(type_node);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseTypeReference
    fn parse_type_reference(&mut self) -> NodeId {
        let pos = self.node_pos();
        let type_name = self.parse_entity_name_of_type_reference();
        let type_arguments = self.parse_type_arguments_of_type_reference();
        let node = self
            .arena
            .new_type_reference_node(type_name, type_arguments);
        self.finish_node(node, pos)
    }

    // Go: internal/parser/parser.go:parseEntityNameOfTypeReference
    fn parse_entity_name_of_type_reference(&mut self) -> NodeId {
        self.parse_entity_name(true, Some(&diagnostics::TYPE_EXPECTED))
    }

    // Go: internal/parser/parser.go:parseTypeArgumentsOfTypeReference
    fn parse_type_arguments_of_type_reference(&mut self) -> Option<NodeList> {
        if !self.has_preceding_line_break() && self.re_scan_less_than_token() == Kind::LessThanToken
        {
            return self.parse_type_arguments();
        }
        None
    }

    // Go: internal/parser/parser.go:parseTypeArguments
    fn parse_type_arguments(&mut self) -> Option<NodeList> {
        if self.token == Kind::LessThanToken {
            return Some(self.parse_bracketed_list(
                ParsingContext::TypeArguments,
                |p| p.parse_type(),
                Kind::LessThanToken,
                Kind::GreaterThanToken,
            ));
        }
        None
    }

    // Go: internal/parser/parser.go:parseBracketedList (subset)
    fn parse_bracketed_list(
        &mut self,
        kind: ParsingContext,
        parse_element: impl FnMut(&mut Parser) -> NodeId,
        opening: Kind,
        closing: Kind,
    ) -> NodeList {
        if self.parse_expected(opening) {
            let result = self.parse_delimited_list(kind, parse_element);
            self.parse_expected(closing);
            return result;
        }
        // DEFER(phase-3): missing-list sentinel (`isMissingNodeList`).
        // blocked-by: NodeList missing-flag representation.
        self.new_node_list(TextRange::new(self.node_pos(), self.node_pos()), Vec::new())
    }

    // Go: internal/parser/parser.go:reScanLessThanToken
    fn re_scan_less_than_token(&mut self) -> Kind {
        self.token = self.scanner.re_scan_less_than_token();
        self.token
    }

    // Go: internal/parser/parser.go:isStartOfType (subset)
    fn is_start_of_type(&mut self) -> bool {
        match self.token {
            Kind::AnyKeyword
            | Kind::UnknownKeyword
            | Kind::StringKeyword
            | Kind::NumberKeyword
            | Kind::BigIntKeyword
            | Kind::BooleanKeyword
            | Kind::ReadonlyKeyword
            | Kind::KeyOfKeyword
            | Kind::SymbolKeyword
            | Kind::UniqueKeyword
            | Kind::VoidKeyword
            | Kind::UndefinedKeyword
            | Kind::NullKeyword
            | Kind::ThisKeyword
            | Kind::TypeOfKeyword
            | Kind::NeverKeyword
            | Kind::OpenBraceToken
            | Kind::OpenBracketToken
            | Kind::LessThanToken
            | Kind::BarToken
            | Kind::AmpersandToken
            | Kind::NewKeyword
            | Kind::StringLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::ObjectKeyword
            | Kind::AsteriskToken
            | Kind::QuestionToken
            | Kind::ExclamationToken
            | Kind::DotDotDotToken
            | Kind::InferKeyword
            | Kind::ImportKeyword
            | Kind::AssertsKeyword
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::TemplateHead
            | Kind::OpenParenToken => true,
            _ => self.is_identifier(),
        }
    }

    // ---- identifiers / entity names ----

    // Go: internal/parser/parser.go:parseEntityName
    fn parse_entity_name(
        &mut self,
        allow_reserved_words: bool,
        diagnostic_message: Option<&'static Message>,
    ) -> NodeId {
        let pos = self.node_pos();
        let mut entity = if allow_reserved_words {
            self.parse_identifier_name_with_diagnostic(diagnostic_message)
        } else {
            self.parse_identifier_with_diagnostic(diagnostic_message)
        };
        while self.parse_optional(Kind::DotToken) {
            if self.token == Kind::LessThanToken {
                break;
            }
            let right = self.parse_right_side_of_dot(allow_reserved_words, false);
            let node = self.arena.new_qualified_name(entity, right);
            entity = self.finish_node(node, pos);
        }
        entity
    }

    // Go: internal/parser/parser.go:parseRightSideOfDot (subset)
    fn parse_right_side_of_dot(
        &mut self,
        allow_identifier_names: bool,
        _allow_private_identifiers: bool,
    ) -> NodeId {
        // DEFER(phase-3): private-identifier and ASI-related recovery branches.
        if allow_identifier_names {
            return self.parse_identifier_name();
        }
        self.parse_identifier()
    }

    // Go: internal/parser/parser.go:parseIdentifierName
    fn parse_identifier_name(&mut self) -> NodeId {
        self.parse_identifier_name_with_diagnostic(None)
    }

    // Go: internal/parser/parser.go:parseIdentifierNameWithDiagnostic
    fn parse_identifier_name_with_diagnostic(
        &mut self,
        diagnostic_message: Option<&'static Message>,
    ) -> NodeId {
        let is_identifier = token_is_identifier_or_keyword(self.token);
        self.create_identifier_with_diagnostic(is_identifier, diagnostic_message)
    }

    // Go: internal/parser/parser.go:parseIdentifier
    fn parse_identifier(&mut self) -> NodeId {
        self.parse_identifier_with_diagnostic(None)
    }

    // Go: internal/parser/parser.go:parseIdentifierWithDiagnostic
    fn parse_identifier_with_diagnostic(
        &mut self,
        diagnostic_message: Option<&'static Message>,
    ) -> NodeId {
        let is_identifier = self.is_identifier();
        self.create_identifier_with_diagnostic(is_identifier, diagnostic_message)
    }

    // Go: internal/parser/parser.go:createIdentifierWithDiagnostic (subset)
    fn create_identifier_with_diagnostic(
        &mut self,
        is_identifier: bool,
        diagnostic_message: Option<&'static Message>,
    ) -> NodeId {
        if is_identifier {
            let pos = self.node_pos();
            let text = self.scanner.token_value().to_string();
            self.next_token_without_check();
            let node = self.new_identifier(&text);
            return self.finish_node(node, pos);
        }
        // DEFER(phase-3): private-identifier and reserved-word specific messages.
        let report_at_current_position = self.token == Kind::EndOfFile;
        let message = diagnostic_message.unwrap_or(&diagnostics::IDENTIFIER_EXPECTED);
        if report_at_current_position {
            let pos = self.scanner.token_full_start();
            self.parse_error_at(pos, pos, message, Vec::new());
        } else {
            self.parse_error_at_current_token(message, Vec::new());
        }
        self.create_missing_identifier()
    }

    // Go: internal/parser/parser.go:newIdentifier
    fn new_identifier(&mut self, text: &str) -> NodeId {
        self.identifier_count += 1;
        self.arena.new_identifier(text)
    }

    // Go: internal/parser/parser.go:createMissingIdentifier
    fn create_missing_identifier(&mut self) -> NodeId {
        let pos = self.node_pos();
        let node = self.new_identifier("");
        self.finish_node(node, pos)
    }

    // ---- predicates / context flags ----

    // Go: internal/parser/parser.go:setContextFlags
    fn set_context_flags(&mut self, flag: NodeFlags, value: bool) {
        if value {
            self.context_flags |= flag;
        } else {
            self.context_flags &= !flag;
        }
    }

    // Go: internal/parser/parser.go:inContext
    fn in_context(&self, flags: NodeFlags) -> bool {
        self.context_flags.contains(flags)
    }

    // Go: internal/parser/parser.go:inDisallowInContext
    fn in_disallow_in_context(&self) -> bool {
        self.in_context(NodeFlags::DISALLOW_IN_CONTEXT)
    }

    // Go: internal/parser/parser.go:inYieldContext
    fn in_yield_context(&self) -> bool {
        self.in_context(NodeFlags::YIELD_CONTEXT)
    }

    // Go: internal/parser/parser.go:inAwaitContext
    fn in_await_context(&self) -> bool {
        self.in_context(NodeFlags::AWAIT_CONTEXT)
    }

    // Go: internal/parser/parser.go:inDecoratorContext
    fn in_decorator_context(&self) -> bool {
        self.in_context(NodeFlags::DECORATOR_CONTEXT)
    }

    // Go: internal/parser/parser.go:reScanSlashToken
    fn re_scan_slash_token(&mut self) -> Kind {
        self.token = self.scanner.re_scan_slash_token(true);
        self.token
    }

    // Go: internal/parser/parser.go:reScanGreaterThanToken
    fn re_scan_greater_than_token(&mut self) -> Kind {
        self.token = self.scanner.re_scan_greater_than_token();
        self.token
    }

    // Go: internal/parser/parser.go:canParseSemicolon
    fn can_parse_semicolon(&self) -> bool {
        self.token == Kind::SemicolonToken
            || self.token == Kind::CloseBraceToken
            || self.token == Kind::EndOfFile
            || self.has_preceding_line_break()
    }

    // Go: internal/parser/parser.go:tryParseSemicolon
    fn try_parse_semicolon(&mut self) -> bool {
        if !self.can_parse_semicolon() {
            return false;
        }
        if self.token == Kind::SemicolonToken {
            self.next_token();
        }
        true
    }

    // Go: internal/parser/parser.go:isStartOfStatement (subset)
    fn is_start_of_statement(&mut self) -> bool {
        match self.token {
            Kind::AtToken
            | Kind::SemicolonToken
            | Kind::OpenBraceToken
            | Kind::VarKeyword
            | Kind::LetKeyword
            | Kind::ConstKeyword
            | Kind::FunctionKeyword
            | Kind::ClassKeyword
            | Kind::EnumKeyword
            | Kind::IfKeyword
            | Kind::DoKeyword
            | Kind::WhileKeyword
            | Kind::ForKeyword
            | Kind::ContinueKeyword
            | Kind::BreakKeyword
            | Kind::ReturnKeyword
            | Kind::WithKeyword
            | Kind::SwitchKeyword
            | Kind::ThrowKeyword
            | Kind::TryKeyword
            | Kind::DebuggerKeyword
            | Kind::CatchKeyword
            | Kind::FinallyKeyword => true,
            // `async`/`declare`/`interface`/`type` are valid statement starts
            // either as a declaration or as an identifier expression.
            Kind::AsyncKeyword
            | Kind::DeclareKeyword
            | Kind::InterfaceKeyword
            | Kind::TypeKeyword
            | Kind::ModuleKeyword
            | Kind::NamespaceKeyword
            | Kind::GlobalKeyword => true,
            Kind::ExportKeyword => self.is_start_of_declaration(),
            Kind::ImportKeyword => {
                self.is_start_of_declaration()
                    || self.is_next_token_open_paren_or_less_than_or_dot()
            }
            // DEFER(phase-3): remaining declaration-keyword statement starts
            // (class/enum/interface/type/import/module/namespace/...).
            // blocked-by: declaration parser slices.
            _ => self.is_start_of_expression(),
        }
    }

    // Go: internal/parser/parser.go:isStartOfExpression (subset)
    fn is_start_of_expression(&mut self) -> bool {
        if self.is_start_of_left_hand_side_expression() {
            return true;
        }
        match self.token {
            Kind::PlusToken
            | Kind::MinusToken
            | Kind::TildeToken
            | Kind::ExclamationToken
            | Kind::DeleteKeyword
            | Kind::TypeOfKeyword
            | Kind::VoidKeyword
            | Kind::PlusPlusToken
            | Kind::MinusMinusToken
            | Kind::LessThanToken
            | Kind::AwaitKeyword
            | Kind::YieldKeyword
            | Kind::PrivateIdentifier
            | Kind::AtToken => true,
            _ => self.is_binary_operator() || self.is_identifier(),
        }
    }

    // Go: internal/parser/parser.go:isStartOfLeftHandSideExpression (subset)
    fn is_start_of_left_hand_side_expression(&mut self) -> bool {
        match self.token {
            Kind::ThisKeyword
            | Kind::SuperKeyword
            | Kind::NullKeyword
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::StringLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::TemplateHead
            | Kind::OpenParenToken
            | Kind::OpenBracketToken
            | Kind::OpenBraceToken
            | Kind::FunctionKeyword
            | Kind::ClassKeyword
            | Kind::NewKeyword
            | Kind::SlashToken
            | Kind::SlashEqualsToken
            | Kind::Identifier => true,
            Kind::ImportKeyword => self.is_next_token_open_paren_or_less_than_or_dot(),
            _ => self.is_identifier(),
        }
    }

    // Go: internal/parser/parser.go:isNextTokenOpenParenOrLessThanOrDot
    fn is_next_token_open_paren_or_less_than_or_dot(&mut self) -> bool {
        self.look_ahead(|p| {
            matches!(
                p.next_token(),
                Kind::OpenParenToken | Kind::LessThanToken | Kind::DotToken
            )
        })
    }

    // Go: internal/parser/parser.go:isIdentifier
    fn is_identifier(&self) -> bool {
        if self.token == Kind::Identifier {
            return true;
        }
        if self.token == Kind::YieldKeyword && self.in_yield_context()
            || self.token == Kind::AwaitKeyword && self.in_await_context()
        {
            return false;
        }
        self.token > Kind::LAST_RESERVED_WORD
    }

    // Go: internal/parser/parser.go:isBindingIdentifier
    fn is_binding_identifier(&self) -> bool {
        self.token == Kind::Identifier || self.token > Kind::LAST_RESERVED_WORD
    }

    // Go: internal/parser/parser.go:isLiteralPropertyName
    fn is_literal_property_name(&self) -> bool {
        token_is_identifier_or_keyword(self.token)
            || self.token == Kind::StringLiteral
            || self.token == Kind::NumericLiteral
            || self.token == Kind::BigIntLiteral
    }

    // Go: internal/parser/parser.go:isStartOfParameter (subset)
    fn is_start_of_parameter(&mut self) -> bool {
        self.token == Kind::DotDotDotToken
            || self.is_binding_identifier_or_private_identifier_or_pattern()
            || is_modifier_kind(self.token)
            || self.token == Kind::AtToken
            || self.is_start_of_type()
    }

    // Go: internal/parser/parser.go:isBindingIdentifierOrPrivateIdentifierOrPattern
    fn is_binding_identifier_or_private_identifier_or_pattern(&self) -> bool {
        self.token == Kind::OpenBraceToken
            || self.token == Kind::OpenBracketToken
            || self.token == Kind::PrivateIdentifier
            || self.is_binding_identifier()
    }

    // Go: internal/parser/parser.go:isBinaryOperator
    fn is_binary_operator(&self) -> bool {
        if self.in_disallow_in_context() && self.token == Kind::InKeyword {
            return false;
        }
        tsgo_ast::precedence::get_binary_operator_precedence(self.token)
            != tsgo_ast::OperatorPrecedence::Invalid
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;

#[cfg(test)]
#[path = "deepclone_test.rs"]
mod deepclone_tests;
