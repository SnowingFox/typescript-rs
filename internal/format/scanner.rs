//! The trivia-aware formatting scanner.
//!
//! 1:1 port of Go `internal/format/scanner.go` (`formattingScanner`,
//! `tokenInfo`, `TextRangeWithKind`, the `scanAction` rescan state machine).
//! It walks tokens across a span over a [`tsgo_scanner::Scanner`] (with
//! `skip_trivia` disabled), attaching leading/trailing trivia to each token.
//!
//! # Divergence from Go's constructor
//!
//! Go's `newFormattingScanner` builds the scanner *and* immediately runs the
//! worker (`worker.execute(fmtScn)`), returning the worker's edits. Here the
//! worker owns the scanner directly (`FormatSpanWorker::run`) and constructs it
//! via [`FormattingScanner::new`]; the "run worker then reset" wrapper is folded
//! into the worker, so this module only models the scanner state machine.
//!
//! # Rescan context (`TokenRescanContext`)
//!
//! Go's `readTokenInfo(n *ast.Node)` reads the node to decide rescans
//! (`shouldRescanGreaterThanToken`, JSX identifier, etc.) and to `fixTokenKind`.
//! This port passes a small [`TokenRescanContext`] (the node's kind + parent
//! kind) instead of threading an `*ast.Node`, since the scanner does not hold the
//! navigation context.

use tsgo_ast::Kind;
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::text::TextRange;
use tsgo_scanner::Scanner;

/// A source range tagged with the syntax kind of the token/trivia spanning it.
///
/// Mirrors Go's `TextRangeWithKind`.
///
/// Side effects: none (pure value type).
// Go: internal/format/scanner.go:TextRangeWithKind
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRangeWithKind {
    /// The token/trivia source range.
    pub loc: TextRange,
    /// The token/trivia kind.
    pub kind: Kind,
}

impl TextRangeWithKind {
    /// Creates a [`TextRangeWithKind`] from `pos`, `end`, and `kind`.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/scanner.go:NewTextRangeWithKind
    pub fn new(pos: i32, end: i32, kind: Kind) -> TextRangeWithKind {
        TextRangeWithKind {
            loc: TextRange::new(pos, end),
            kind,
        }
    }

    /// The "empty" sentinel `NewTextRangeWithKind(0, 0, 0)` Go compares against to
    /// detect an unset `previousRange`.
    ///
    /// Side effects: none (pure).
    pub fn empty() -> TextRangeWithKind {
        TextRangeWithKind::new(0, 0, Kind::Unknown)
    }
}

/// A token together with its attached leading and trailing trivia.
///
/// Mirrors Go's `tokenInfo`.
///
/// Side effects: none (pure value type).
// Go: internal/format/scanner.go:tokenInfo
#[derive(Clone, Debug, Default)]
pub struct TokenInfo {
    /// Leading trivia ranges (comments / whitespace / newlines) before the token.
    pub leading_trivia: Vec<TextRangeWithKind>,
    /// The token range itself.
    pub token: TextRangeWithKind,
    /// Trailing trivia ranges, up to and including a single trailing newline.
    pub trailing_trivia: Vec<TextRangeWithKind>,
}

impl Default for TextRangeWithKind {
    fn default() -> TextRangeWithKind {
        TextRangeWithKind::empty()
    }
}

/// The rescan decision context for [`FormattingScanner::read_token_info`].
///
/// Carries the node kind and parent kind Go reads off `n *ast.Node` to choose a
/// [`ScanAction`] and to run `fixTokenKind`.
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, Debug)]
pub struct TokenRescanContext {
    /// The node's kind (`n.Kind`).
    pub node_kind: Kind,
    /// The node's parent kind (`n.Parent.Kind`), if any.
    pub node_parent_kind: Option<Kind>,
}

/// The scanner's greedy-rescan choices, mirroring Go's `scanAction` iota.
///
/// Side effects: none (pure value type).
// Go: internal/format/scanner.go:scanAction
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanAction {
    Scan,
    RescanGreaterThanToken,
    RescanSlashToken,
    RescanTemplateToken,
    RescanJsxIdentifier,
    RescanJsxText,
    RescanJsxAttributeValue,
}

/// Reports whether `kind` is trivia (`ast.IsTrivia`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsTrivia
fn is_trivia(kind: Kind) -> bool {
    Kind::FIRST_TRIVIA_TOKEN <= kind && kind <= Kind::LAST_TRIVIA_TOKEN
}

/// Reports whether `kind` is a token kind (`ast.IsTokenKind`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsTokenKind
fn is_token_kind(kind: Kind) -> bool {
    Kind::FIRST_TOKEN <= kind && kind <= Kind::LAST_TOKEN
}

/// Reports whether a node of `kind` should rescan a `>` into a compound
/// greater-than operator.
///
/// Side effects: none (pure).
// Go: internal/format/scanner.go:shouldRescanGreaterThanToken
fn should_rescan_greater_than_token(node_kind: Kind) -> bool {
    matches!(
        node_kind,
        Kind::GreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanGreaterThanToken
            | Kind::GreaterThanGreaterThanToken
    )
}

/// Reports whether the JSX identifier rescan applies, based on the parent kind.
///
/// Side effects: none (pure).
// Go: internal/format/scanner.go:shouldRescanJsxIdentifier
fn should_rescan_jsx_identifier(node_kind: Kind, node_parent_kind: Option<Kind>) -> bool {
    if let Some(parent) = node_parent_kind {
        if matches!(
            parent,
            Kind::JsxAttribute
                | Kind::JsxOpeningElement
                | Kind::JsxClosingElement
                | Kind::JsxSelfClosingElement
                | Kind::JsxNamespacedName
        ) {
            return is_keyword_kind(node_kind) || node_kind == Kind::Identifier;
        }
    }
    false
}

/// Reports whether `kind` is a keyword (`ast.IsKeywordKind`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsKeywordKind
fn is_keyword_kind(kind: Kind) -> bool {
    Kind::FIRST_KEYWORD <= kind && kind <= Kind::LAST_KEYWORD
}

/// Reports whether the slash-token rescan applies.
///
/// Side effects: none (pure).
// Go: internal/format/scanner.go:shouldRescanSlashToken
fn should_rescan_slash_token(node_kind: Kind) -> bool {
    node_kind == Kind::RegularExpressionLiteral
}

/// Reports whether the template-token rescan applies.
///
/// Side effects: none (pure).
// Go: internal/format/scanner.go:shouldRescanTemplateToken
fn should_rescan_template_token(node_kind: Kind) -> bool {
    node_kind == Kind::TemplateMiddle || node_kind == Kind::TemplateTail
}

/// Reports whether the current token starts with a slash.
///
/// Side effects: none (pure).
// Go: internal/format/scanner.go:startsWithSlashToken
fn starts_with_slash_token(t: Kind) -> bool {
    t == Kind::SlashToken || t == Kind::SlashEqualsToken
}

/// The trivia-aware scanner driving the formatting walk.
///
/// Side effects: owns and advances a [`tsgo_scanner::Scanner`].
// Go: internal/format/scanner.go:formattingScanner
pub struct FormattingScanner {
    s: Scanner,
    start_pos: i32,
    end_pos: i32,
    saved_pos: i32,
    has_last_token_info: bool,
    last_token_info: TokenInfo,
    last_scan_action: ScanAction,
    leading_trivia: Vec<TextRangeWithKind>,
    trailing_trivia: Vec<TextRangeWithKind>,
    was_new_line: bool,
}

impl FormattingScanner {
    /// Builds a formatting scanner over `text[start_pos..end_pos)`.
    ///
    /// Mirrors the scanner setup inside Go's `newFormattingScanner`.
    ///
    /// Side effects: allocates a scanner owning a copy of `text`.
    // Go: internal/format/scanner.go:newFormattingScanner
    pub fn new(
        text: &str,
        language_variant: LanguageVariant,
        start_pos: i32,
        end_pos: i32,
    ) -> FormattingScanner {
        let mut s = Scanner::new();
        s.set_skip_trivia(false);
        s.set_language_variant(language_variant);
        s.set_text(text.to_string());
        s.reset_token_state(start_pos);
        FormattingScanner {
            s,
            start_pos,
            end_pos,
            saved_pos: 0,
            has_last_token_info: false,
            last_token_info: TokenInfo::default(),
            last_scan_action: ScanAction::Scan,
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
            was_new_line: true,
        }
    }

    /// Advances past the current token to the next token's leading trivia.
    ///
    /// Side effects: scans tokens; records leading trivia; updates `was_new_line`.
    // Go: internal/format/scanner.go:formattingScanner.advance
    pub fn advance(&mut self) {
        self.has_last_token_info = false;
        let is_started = self.s.token_full_start() != self.start_pos;

        if is_started {
            self.was_new_line = self
                .trailing_trivia
                .last()
                .is_some_and(|t| t.kind == Kind::NewLineTrivia);
        } else {
            self.s.scan();
        }

        self.leading_trivia.clear();
        self.trailing_trivia.clear();

        let mut pos = self.s.token_full_start();

        // Read leading trivia and token.
        while pos < self.end_pos {
            let t = self.s.token();
            if !is_trivia(t) {
                break;
            }
            // consume leading trivia
            self.s.scan();
            let item = TextRangeWithKind::new(pos, self.s.token_full_start(), t);
            pos = self.s.token_full_start();
            self.leading_trivia.push(item);
        }

        self.saved_pos = self.s.token_full_start();
    }

    /// Coerces the token kind to the container kind when the container is itself
    /// a token (`fixTokenKind`).
    ///
    /// Side effects: none (pure).
    // Go: internal/format/scanner.go:fixTokenKind
    fn fix_token_kind(mut token_info: TokenInfo, container_kind: Kind) -> TokenInfo {
        if is_token_kind(container_kind) && token_info.token.kind != container_kind {
            token_info.token.kind = container_kind;
        }
        token_info
    }

    /// Reads the current token (with rescans appropriate to `ctx`) and its
    /// trailing trivia, caching the result.
    ///
    /// # Panics
    /// Panics if not currently on a token (mirrors Go's `debug.Assert`).
    ///
    /// Side effects: scans tokens; caches `last_token_info`.
    // Go: internal/format/scanner.go:formattingScanner.readTokenInfo
    pub fn read_token_info(&mut self, ctx: TokenRescanContext) -> TokenInfo {
        assert!(
            self.is_on_token(),
            "readTokenInfo called when not on a token"
        );

        let expected_scan_action = if should_rescan_greater_than_token(ctx.node_kind) {
            ScanAction::RescanGreaterThanToken
        } else if should_rescan_slash_token(ctx.node_kind) {
            ScanAction::RescanSlashToken
        } else if should_rescan_template_token(ctx.node_kind) {
            ScanAction::RescanTemplateToken
        } else if should_rescan_jsx_identifier(ctx.node_kind, ctx.node_parent_kind) {
            ScanAction::RescanJsxIdentifier
        } else if self.should_rescan_jsx_text(ctx.node_kind) {
            ScanAction::RescanJsxText
        } else if Self::should_rescan_jsx_attribute_value(ctx.node_parent_kind) {
            ScanAction::RescanJsxAttributeValue
        } else {
            ScanAction::Scan
        };

        if self.has_last_token_info && expected_scan_action == self.last_scan_action {
            // Same expected scan action as last time: reuse the cached token info.
            self.last_token_info =
                Self::fix_token_kind(self.last_token_info.clone(), ctx.node_kind);
            return self.last_token_info.clone();
        }

        if self.s.token_full_start() != self.saved_pos {
            // Scan action differs from a prior call at this position: rescan text.
            self.s.reset_token_state(self.saved_pos);
            self.s.scan();
        }

        let current_token = self.get_next_token(ctx.node_kind, expected_scan_action);

        let token =
            TextRangeWithKind::new(self.s.token_full_start(), self.s.token_end(), current_token);

        // consume trailing trivia
        self.trailing_trivia.clear();
        while self.s.token_full_start() < self.end_pos {
            let t = self.s.scan();
            if !is_trivia(t) {
                break;
            }
            let trivia = TextRangeWithKind::new(self.s.token_full_start(), self.s.token_end(), t);
            self.trailing_trivia.push(trivia);
            if t == Kind::NewLineTrivia {
                // move past the new line
                self.s.scan();
                break;
            }
        }

        self.has_last_token_info = true;
        self.last_token_info = TokenInfo {
            leading_trivia: self.leading_trivia.clone(),
            token,
            trailing_trivia: self.trailing_trivia.clone(),
        };
        self.last_token_info = Self::fix_token_kind(self.last_token_info.clone(), ctx.node_kind);

        self.last_token_info.clone()
    }

    /// Applies the rescan dictated by `expected_scan_action`, returning the
    /// (possibly rescanned) current token kind.
    ///
    /// Side effects: may rescan the current token.
    // Go: internal/format/scanner.go:formattingScanner.getNextToken
    fn get_next_token(&mut self, node_kind: Kind, expected_scan_action: ScanAction) -> Kind {
        let token = self.s.token();
        self.last_scan_action = ScanAction::Scan;
        match expected_scan_action {
            ScanAction::RescanGreaterThanToken => {
                if token == Kind::GreaterThanToken {
                    self.last_scan_action = ScanAction::RescanGreaterThanToken;
                    let new_token = self.s.re_scan_greater_than_token();
                    debug_assert_eq!(node_kind, new_token);
                    return new_token;
                }
            }
            ScanAction::RescanSlashToken => {
                if starts_with_slash_token(token) {
                    self.last_scan_action = ScanAction::RescanSlashToken;
                    let new_token = self.s.re_scan_slash_token(false);
                    debug_assert_eq!(node_kind, new_token);
                    return new_token;
                }
            }
            ScanAction::RescanTemplateToken => {
                if token == Kind::CloseBraceToken {
                    self.last_scan_action = ScanAction::RescanTemplateToken;
                    return self.s.re_scan_template_token(false);
                }
            }
            ScanAction::RescanJsxIdentifier => {
                self.last_scan_action = ScanAction::RescanJsxIdentifier;
                return self.s.scan_jsx_identifier();
            }
            ScanAction::RescanJsxText => {
                self.last_scan_action = ScanAction::RescanJsxText;
                return self.s.re_scan_jsx_token(false);
            }
            ScanAction::RescanJsxAttributeValue => {
                self.last_scan_action = ScanAction::RescanJsxAttributeValue;
                return self.s.re_scan_jsx_attribute_value();
            }
            ScanAction::Scan => {}
        }
        token
    }

    /// Reports whether the JSX text rescan applies for a node of `node_kind`.
    ///
    /// Side effects: none (reads cached token info).
    // Go: internal/format/scanner.go:formattingScanner.shouldRescanJsxText
    fn should_rescan_jsx_text(&self, node_kind: Kind) -> bool {
        if node_kind == Kind::JsxText {
            return true;
        }
        if node_kind != Kind::JsxElement || !self.has_last_token_info {
            return false;
        }
        self.last_token_info.token.kind == Kind::JsxText
    }

    /// Reports whether the JSX attribute-value rescan applies.
    ///
    /// The precise Go check (`node.Parent.Initializer() == node`) needs the JSX
    /// attribute initializer accessor; with no JSX in the reachable round, this
    /// conservatively reports `false` whenever the parent is not a JSX attribute.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/scanner.go:shouldRescanJsxAttributeValue
    // DEFER(phase-7): precise `node.Parent.Initializer() == node` check.
    // blocked-by: JsxAttribute initializer accessor (out of round scope).
    fn should_rescan_jsx_attribute_value(_node_parent_kind: Option<Kind>) -> bool {
        false
    }

    /// Returns the EOF token range.
    ///
    /// # Panics
    /// Panics if not on EOF (mirrors Go's `debug.Assert`).
    ///
    /// Side effects: none (reads scanner state).
    // Go: internal/format/scanner.go:formattingScanner.readEOFTokenRange
    pub fn read_eof_token_range(&self) -> TextRangeWithKind {
        assert!(self.is_on_eof(), "readEOFTokenRange called when not on EOF");
        TextRangeWithKind::new(
            self.s.token_full_start(),
            self.s.token_end(),
            Kind::EndOfFile,
        )
    }

    /// Reports whether the scanner is currently on a non-trivia, non-EOF token.
    ///
    /// Side effects: none.
    // Go: internal/format/scanner.go:formattingScanner.isOnToken
    pub fn is_on_token(&self) -> bool {
        let mut current = self.s.token();
        if self.has_last_token_info {
            current = self.last_token_info.token.kind;
        }
        current != Kind::EndOfFile && !is_trivia(current)
    }

    /// Reports whether the scanner is currently on the EOF token.
    ///
    /// Side effects: none.
    // Go: internal/format/scanner.go:formattingScanner.isOnEOF
    pub fn is_on_eof(&self) -> bool {
        let mut current = self.s.token();
        if self.has_last_token_info {
            current = self.last_token_info.token.kind;
        }
        current == Kind::EndOfFile
    }

    /// Repositions the scanner just past `r`.
    ///
    /// Side effects: resets scanner position and trivia state.
    // Go: internal/format/scanner.go:formattingScanner.skipToEndOf
    pub fn skip_to_end_of(&mut self, r: TextRange) {
        self.s.reset_token_state(r.end());
        self.saved_pos = self.s.token_full_start();
        self.last_scan_action = ScanAction::Scan;
        self.has_last_token_info = false;
        self.was_new_line = false;
        self.leading_trivia.clear();
        self.trailing_trivia.clear();
    }

    /// Repositions the scanner to the start of `r`.
    ///
    /// Side effects: resets scanner position and trivia state.
    // Go: internal/format/scanner.go:formattingScanner.skipToStartOf
    pub fn skip_to_start_of(&mut self, r: TextRange) {
        self.s.reset_token_state(r.pos());
        self.saved_pos = self.s.token_full_start();
        self.last_scan_action = ScanAction::Scan;
        self.has_last_token_info = false;
        self.was_new_line = false;
        self.leading_trivia.clear();
        self.trailing_trivia.clear();
    }

    /// Returns the leading trivia of the current token.
    ///
    /// Side effects: none.
    // Go: internal/format/scanner.go:formattingScanner.getCurrentLeadingTrivia
    pub fn get_current_leading_trivia(&self) -> &[TextRangeWithKind] {
        &self.leading_trivia
    }

    /// Reports whether the most recent trailing trivia ended with a newline.
    ///
    /// Side effects: none.
    // Go: internal/format/scanner.go:formattingScanner.lastTrailingTriviaWasNewLine
    pub fn last_trailing_trivia_was_new_line(&self) -> bool {
        self.was_new_line
    }

    /// Returns the full start of the current token (cached when available).
    ///
    /// Side effects: none.
    // Go: internal/format/scanner.go:formattingScanner.getTokenFullStart
    pub fn get_token_full_start(&self) -> i32 {
        if self.has_last_token_info {
            return self.last_token_info.token.loc.pos();
        }
        self.s.token_full_start()
    }

    /// Resets the underlying scanner (Go's trailing `scan.Reset()`).
    ///
    /// Side effects: clears scanner state.
    // Go: internal/scanner/scanner.go:Scanner.Reset
    pub fn reset(&mut self) {
        self.has_last_token_info = false;
        self.s.reset();
    }
}

#[cfg(test)]
#[path = "scanner_test.rs"]
mod tests;
