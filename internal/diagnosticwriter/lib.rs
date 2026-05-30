//! `tsgo_diagnosticwriter` — 1:1 Rust port of Go `internal/diagnosticwriter`.
//!
//! Renders diagnostics (`ast.Diagnostic` / LSP diagnostics) into human-readable
//! text: pretty output with colored source context and a line-number gutter,
//! the compact one-line `file(line,col): error TSxxxx: message` form, flattened
//! message chains, and the per-file error summary table.

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::text::TextPos;
use tsgo_core::{utf16_len, Utf16Offset};
use tsgo_diagnostics::{
    Category, ERRORS_FILES, FILE_APPEARS_TO_BE_BINARY,
    FILE_CHANGE_DETECTED_STARTING_INCREMENTAL_COMPILATION, FOUND_0_ERRORS,
    FOUND_0_ERRORS_IN_1_FILES, FOUND_0_ERRORS_IN_THE_SAME_FILE_STARTING_AT_COLON_1, FOUND_1_ERROR,
    FOUND_1_ERROR_IN_0, STARTING_COMPILATION_IN_WATCH_MODE,
};
use tsgo_locale::Locale;
use tsgo_scanner::{compute_line_of_position, compute_position_of_line_and_byte_offset};
use tsgo_tspath::{convert_to_relative_path, path_is_absolute, ComparePathsOptions};

/// A source file as seen by the diagnostic renderers: just enough to resolve a
/// byte position to a line/column and to slice out a line of context.
///
/// Mirrors Go's `FileLike` interface (`FileName`/`Text`/`ECMALineMap`).
// Go: internal/diagnosticwriter/diagnosticwriter.go:FileLike
pub trait FileLike {
    /// Returns the file's name (as recorded by the program, possibly absolute).
    fn file_name(&self) -> &str;
    /// Returns the full UTF-8 source text.
    fn text(&self) -> &str;
    /// Returns the byte offsets at which each line begins (ECMA line terminators).
    fn ecma_line_map(&self) -> &[TextPos];
}

/// A renderable diagnostic, abstracting over `ast.Diagnostic` and LSP
/// diagnostics.
///
/// Mirrors Go's `Diagnostic` interface. `message_chain` and
/// `related_information` borrow their child diagnostics from `self` (Go returns
/// freshly wrapped interface values; the borrowed form is sufficient for every
/// renderer here and avoids per-call allocation).
// Go: internal/diagnosticwriter/diagnosticwriter.go:Diagnostic
// `len` here is a source-span length (mirroring Go's `Len()`), not a collection
// size, so the companion `is_empty` clippy expects does not apply.
#[allow(clippy::len_without_is_empty)]
pub trait Diagnostic {
    /// The file the diagnostic refers to, or `None` for a global diagnostic.
    fn file(&self) -> Option<&dyn FileLike>;
    /// The start byte offset of the diagnostic span.
    fn pos(&self) -> i32;
    /// The end byte offset of the diagnostic span.
    fn end(&self) -> i32;
    /// The length in bytes of the diagnostic span.
    fn len(&self) -> i32;
    /// The numeric diagnostic code (the `xxxx` in `TSxxxx`).
    fn code(&self) -> i32;
    /// The diagnostic [`Category`].
    fn category(&self) -> Category;
    /// The localized primary message text for `locale`.
    fn localize(&self, locale: &Locale) -> String;
    /// The nested message chain (each entry rendered on its own indented line).
    fn message_chain(&self) -> Vec<&dyn Diagnostic>;
    /// Related diagnostics rendered as secondary, indented context.
    fn related_information(&self) -> Vec<&dyn Diagnostic>;
}

const FOREGROUND_COLOR_ESCAPE_GREY: &str = "\u{1b}[90m";
const FOREGROUND_COLOR_ESCAPE_RED: &str = "\u{1b}[91m";
const FOREGROUND_COLOR_ESCAPE_YELLOW: &str = "\u{1b}[93m";
const FOREGROUND_COLOR_ESCAPE_BLUE: &str = "\u{1b}[94m";
const FOREGROUND_COLOR_ESCAPE_CYAN: &str = "\u{1b}[96m";

const GUTTER_STYLE_SEQUENCE: &str = "\u{1b}[7m";
const GUTTER_SEPARATOR: &str = " ";
const RESET_ESCAPE_SEQUENCE: &str = "\u{1b}[0m";
const ELLIPSIS: &str = "...";

/// Options controlling diagnostic formatting: the UI locale, path-relativization
/// settings, and the newline sequence to emit.
///
/// Mirrors Go's `FormattingOptions`, which embeds `tspath.ComparePathsOptions`;
/// the embedded fields are reached here through the named
/// [`compare_paths_options`](FormattingOptions::compare_paths_options) field.
// Go: internal/diagnosticwriter/diagnosticwriter.go:FormattingOptions
pub struct FormattingOptions {
    /// The UI language used to localize message text.
    pub locale: Locale,
    /// Case-sensitivity and current-directory settings for relativizing paths.
    pub compare_paths_options: ComparePathsOptions,
    /// The newline sequence inserted between lines of output.
    pub new_line: String,
}

/// A pluggable styling strategy: writes `text` to the output wrapped in
/// `format_style` and a reset (or, for colorless output, just the text).
///
/// Mirrors Go's `FormattedWriter` function type; injected into
/// [`write_location`] so the same layout serves both colored and plain output.
// Go: internal/diagnosticwriter/diagnosticwriter.go:FormattedWriter
pub type FormattedWriter = fn(&mut dyn fmt::Write, &str, &str);

// Writes `text` wrapped in `format_style` and a trailing reset escape.
// Matches the [`FormattedWriter`] signature so it can be passed as the default.
// Go: internal/diagnosticwriter/diagnosticwriter.go:writeWithStyleAndReset
fn write_with_style_and_reset(out: &mut dyn fmt::Write, text: &str, format_style: &str) {
    write_str(out, format_style);
    write_str(out, text);
    write_str(out, RESET_ESCAPE_SEQUENCE);
}

// Returns the 0-based line and UTF-16 character offset (from the line start) of
// `pos` within `file`, using the file's ECMA line map.
// Go: internal/scanner/scanner.go:GetECMALineAndUTF16CharacterOfPosition
fn get_ecma_line_and_utf16_character_of_position(
    file: &dyn FileLike,
    pos: i32,
) -> (i32, Utf16Offset) {
    let line_map = file.ecma_line_map();
    let line = compute_line_of_position(line_map, pos);
    let line_start = line_map[line as usize].0;
    let character = utf16_len(&file.text()[line_start as usize..pos as usize]);
    (line, character)
}

// Returns the 0-based line containing `pos` within `file`.
// Go: internal/scanner/scanner.go:GetECMALineOfPosition
fn get_ecma_line_of_position(file: &dyn FileLike, pos: i32) -> i32 {
    compute_line_of_position(file.ecma_line_map(), pos)
}

// Returns the absolute byte position of `byte_offset` within line `line`.
// Go: internal/scanner/scanner.go:GetECMAPositionOfLineAndByteOffset
fn get_ecma_position_of_line_and_byte_offset(
    file: &dyn FileLike,
    line: i32,
    byte_offset: i32,
) -> i32 {
    compute_position_of_line_and_byte_offset(file.ecma_line_map(), line, byte_offset)
}

/// Writes a colorized source location (`relativePath:line:col`, 1-based) for
/// `pos` in `file`, using `writer` to apply (or drop) styling.
///
/// DIVERGENCE(port): Go's `WriteLocation` accepts a nullable `formatOpts` and
/// falls back to the raw file name when it is `nil`; every caller here passes a
/// real `FormattingOptions`, so the path is always relativized.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteLocation
pub fn write_location(
    out: &mut dyn fmt::Write,
    file: &dyn FileLike,
    pos: i32,
    format_opts: &FormattingOptions,
    writer: FormattedWriter,
) {
    let (first_line, first_char) = get_ecma_line_and_utf16_character_of_position(file, pos);
    let relative_file_name =
        convert_to_relative_path(file.file_name(), &format_opts.compare_paths_options);
    writer(out, &relative_file_name, FOREGROUND_COLOR_ESCAPE_CYAN);
    write_str(out, ":");
    writer(
        out,
        &(first_line + 1).to_string(),
        FOREGROUND_COLOR_ESCAPE_YELLOW,
    );
    write_str(out, ":");
    writer(
        out,
        &(first_char.0 + 1).to_string(),
        FOREGROUND_COLOR_ESCAPE_YELLOW,
    );
}

// Writes `s` to `out`, discarding any error (mirrors Go's `fmt.Fprint`, which
// ignores write failures to in-memory writers).
fn write_str(out: &mut dyn fmt::Write, s: &str) {
    let _ = out.write_str(s);
}

/// Flattens a diagnostic (its primary message plus any message chain) into a
/// single string, with each chain level indented by two spaces.
///
/// # Examples
/// ```
/// # use tsgo_diagnosticwriter::*;
/// # use tsgo_diagnostics::Category;
/// # use tsgo_core::text::TextPos;
/// struct G;
/// impl Diagnostic for G {
///     fn file(&self) -> Option<&dyn FileLike> { None }
///     fn pos(&self) -> i32 { 0 }
///     fn end(&self) -> i32 { 0 }
///     fn len(&self) -> i32 { 0 }
///     fn code(&self) -> i32 { 2304 }
///     fn category(&self) -> Category { Category::Error }
///     fn localize(&self, _: &tsgo_locale::Locale) -> String { "Cannot find name 'x'.".into() }
///     fn message_chain(&self) -> Vec<&dyn Diagnostic> { Vec::new() }
///     fn related_information(&self) -> Vec<&dyn Diagnostic> { Vec::new() }
/// }
/// let en = tsgo_locale::parse("en").unwrap();
/// assert_eq!(flatten_diagnostic_message(&G, "\n", &en), "Cannot find name 'x'.");
/// ```
///
/// Side effects: none (pure).
// Go: internal/diagnosticwriter/diagnosticwriter.go:FlattenDiagnosticMessage
pub fn flatten_diagnostic_message(d: &dyn Diagnostic, new_line: &str, locale: &Locale) -> String {
    let mut output = String::new();
    write_flattened_diagnostic_message(&mut output, d, new_line, locale);
    output
}

/// Writes the flattened form of `diagnostic` (primary message followed by its
/// indented message chain) to `out`.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteFlattenedDiagnosticMessage
pub fn write_flattened_diagnostic_message(
    out: &mut dyn fmt::Write,
    diagnostic: &dyn Diagnostic,
    new_line: &str,
    locale: &Locale,
) {
    write_str(out, &diagnostic.localize(locale));
    for chain in diagnostic.message_chain() {
        flatten_diagnostic_message_chain(out, chain, new_line, locale, 1);
    }
}

// Recursively writes one message-chain entry on a fresh line, indented by two
// spaces per `level`, then recurses into its children at `level + 1`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:flattenDiagnosticMessageChain
fn flatten_diagnostic_message_chain(
    out: &mut dyn fmt::Write,
    chain: &dyn Diagnostic,
    new_line: &str,
    locale: &Locale,
    level: i32,
) {
    write_str(out, new_line);
    for _ in 0..level {
        write_str(out, "  ");
    }
    write_str(out, &chain.localize(locale));
    for child in chain.message_chain() {
        flatten_diagnostic_message_chain(out, child, new_line, locale, level + 1);
    }
}

/// Writes each diagnostic in compact one-line form (see
/// [`write_format_diagnostic`]).
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteFormatDiagnostics
pub fn write_format_diagnostics(
    out: &mut dyn fmt::Write,
    diagnostics: &[&dyn Diagnostic],
    format_opts: &FormattingOptions,
) {
    for diagnostic in diagnostics {
        write_format_diagnostic(out, *diagnostic, format_opts);
    }
}

/// Writes a diagnostic in the compact, non-colored form
/// `path(line,col): category TSxxxx: message` followed by a newline.
///
/// The location prefix is omitted for a global (file-less) diagnostic. Used for
/// non-TTY output and `--pretty false`.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteFormatDiagnostic
pub fn write_format_diagnostic(
    out: &mut dyn fmt::Write,
    diagnostic: &dyn Diagnostic,
    format_opts: &FormattingOptions,
) {
    if let Some(file) = diagnostic.file() {
        let (line, character) =
            get_ecma_line_and_utf16_character_of_position(file, diagnostic.pos());
        let relative_file_name =
            convert_to_relative_path(file.file_name(), &format_opts.compare_paths_options);
        let _ = write!(
            out,
            "{}({},{}): ",
            relative_file_name,
            line + 1,
            character.0 + 1
        );
    }

    let _ = write!(
        out,
        "{} TS{}: ",
        diagnostic.category().name(),
        diagnostic.code()
    );
    write_flattened_diagnostic_message(out, diagnostic, &format_opts.new_line, &format_opts.locale);
    write_str(out, &format_opts.new_line);
}

/// Writes the diagnostics in `diagnostics` in pretty form (colors + source
/// context), separating consecutive entries with a newline.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:FormatDiagnosticsWithColorAndContext
pub fn format_diagnostics_with_color_and_context(
    out: &mut dyn fmt::Write,
    diagnostics: &[&dyn Diagnostic],
    format_opts: &FormattingOptions,
) {
    if diagnostics.is_empty() {
        return;
    }
    for (i, diagnostic) in diagnostics.iter().enumerate() {
        if i > 0 {
            write_str(out, &format_opts.new_line);
        }
        format_diagnostic_with_color_and_context(out, *diagnostic, format_opts);
    }
}

/// Writes a single diagnostic in pretty form: a colored
/// `path:line:col - error TSxxxx: message` header, a source-context snippet
/// (unless the file is binary), and any related-information blocks.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:FormatDiagnosticWithColorAndContext
pub fn format_diagnostic_with_color_and_context(
    out: &mut dyn fmt::Write,
    diagnostic: &dyn Diagnostic,
    format_opts: &FormattingOptions,
) {
    if let Some(file) = diagnostic.file() {
        write_location(
            out,
            file,
            diagnostic.pos(),
            format_opts,
            write_with_style_and_reset,
        );
        write_str(out, " - ");
    }

    write_with_style_and_reset(
        out,
        diagnostic.category().name(),
        get_category_format(diagnostic.category()),
    );
    let _ = write!(
        out,
        "{} TS{}: {}",
        FOREGROUND_COLOR_ESCAPE_GREY,
        diagnostic.code(),
        RESET_ESCAPE_SEQUENCE
    );
    write_flattened_diagnostic_message(out, diagnostic, &format_opts.new_line, &format_opts.locale);

    if let Some(file) = diagnostic.file() {
        if diagnostic.code() != FILE_APPEARS_TO_BE_BINARY.code() {
            write_str(out, &format_opts.new_line);
            write_code_snippet(
                out,
                file,
                diagnostic.pos(),
                diagnostic.len(),
                get_category_format(diagnostic.category()),
                "",
                format_opts,
            );
            write_str(out, &format_opts.new_line);
        }
    }

    let related_information = diagnostic.related_information();
    if !related_information.is_empty() {
        for related in related_information {
            if let Some(file) = related.file() {
                write_str(out, &format_opts.new_line);
                write_str(out, "  ");
                let pos = related.pos();
                write_location(out, file, pos, format_opts, write_with_style_and_reset);
                write_str(out, " - ");
                write_flattened_diagnostic_message(
                    out,
                    related,
                    &format_opts.new_line,
                    &format_opts.locale,
                );
                write_code_snippet(
                    out,
                    file,
                    pos,
                    related.len(),
                    FOREGROUND_COLOR_ESCAPE_CYAN,
                    "    ",
                    format_opts,
                );
            }
            write_str(out, &format_opts.new_line);
        }
    }
}

// Writes a colorized source-context snippet for the span `[start, start+length)`
// in `source_file`: a line-number gutter, the (tab-expanded, right-trimmed) line
// text, and a `~` underline. Spans over five lines are folded to the first two
// and last two lines with an `...` gutter. `squiggle_color` colors the underline
// and `indent` prefixes every row.
//
// DIVERGENCE(port): the underline length is clamped at zero (`max(0)`) where Go
// calls `strings.Repeat`, which would panic on a negative count; this only
// differs on degenerate spans (e.g. a start column past a right-trimmed line).
// Go: internal/diagnosticwriter/diagnosticwriter.go:writeCodeSnippet
fn write_code_snippet(
    out: &mut dyn fmt::Write,
    source_file: &dyn FileLike,
    start: i32,
    length: i32,
    squiggle_color: &str,
    indent: &str,
    format_opts: &FormattingOptions,
) {
    let (first_line, first_line_char) =
        get_ecma_line_and_utf16_character_of_position(source_file, start);
    let first_line_char = first_line_char.0;
    let (last_line, last_line_char) =
        get_ecma_line_and_utf16_character_of_position(source_file, start + length);
    let mut last_line_char = last_line_char.0;
    if length == 0 {
        // When length is zero, squiggle the character right after the start.
        last_line_char += 1;
    }

    let last_line_of_file = get_ecma_line_of_position(source_file, source_file.text().len() as i32);

    let has_more_than_five_lines = last_line - first_line >= 4;
    let mut gutter_width = (last_line + 1).to_string().len() as i32;
    if has_more_than_five_lines {
        gutter_width = gutter_width.max(ELLIPSIS.len() as i32);
    }
    let gw = gutter_width as usize;
    let new_line = format_opts.new_line.clone();

    let mut i = first_line;
    while i <= last_line {
        write_str(out, &new_line);

        // If the error spans over 5 lines, show only the first 2 and last 2,
        // skipping ahead to the second-to-last line.
        if has_more_than_five_lines && first_line + 1 < i && i < last_line - 1 {
            write_str(out, indent);
            write_str(out, GUTTER_STYLE_SEQUENCE);
            let _ = write!(out, "{ELLIPSIS:>gw$}");
            write_str(out, RESET_ESCAPE_SEQUENCE);
            write_str(out, GUTTER_SEPARATOR);
            write_str(out, &new_line);
            i = last_line - 1;
        }

        let line_start = get_ecma_position_of_line_and_byte_offset(source_file, i, 0);
        let line_end = if i < last_line_of_file {
            get_ecma_position_of_line_and_byte_offset(source_file, i + 1, 0)
        } else {
            source_file.text().len() as i32
        };

        let line_content = source_file.text()[line_start as usize..line_end as usize]
            .trim_end_matches(char::is_whitespace)
            .replace('\t', " ");

        // Gutter and the actual contents of the line.
        write_str(out, indent);
        write_str(out, GUTTER_STYLE_SEQUENCE);
        let line_no = i + 1;
        let _ = write!(out, "{line_no:>gw$}");
        write_str(out, RESET_ESCAPE_SEQUENCE);
        write_str(out, GUTTER_SEPARATOR);
        write_str(out, &line_content);
        write_str(out, &new_line);

        // Gutter and the error span for the line using tildes.
        write_str(out, indent);
        write_str(out, GUTTER_STYLE_SEQUENCE);
        let _ = write!(out, "{:>gw$}", "");
        write_str(out, RESET_ESCAPE_SEQUENCE);
        write_str(out, GUTTER_SEPARATOR);
        write_str(out, squiggle_color);
        if i == first_line {
            // On the last line, limit to the last character; otherwise squiggle
            // the rest of the line.
            let last_char_for_line = if i == last_line {
                last_line_char
            } else {
                utf16_len(&line_content).0
            };
            write_str(out, &" ".repeat(first_line_char.max(0) as usize));
            write_str(
                out,
                &"~".repeat((last_char_for_line - first_line_char).max(0) as usize),
            );
        } else if i == last_line {
            write_str(out, &"~".repeat(last_line_char.max(0) as usize));
        } else {
            write_str(out, &"~".repeat(utf16_len(&line_content).0.max(0) as usize));
        }
        write_str(out, RESET_ESCAPE_SEQUENCE);
        i += 1;
    }
}

/// A tally of error diagnostics for the summary line and per-file table.
///
/// DIVERGENCE(port): Go keys `ErrorsByFile` by the `FileLike` value (pointer
/// identity); this port keys by file name. Determinism is provided by
/// [`sorted_files`](ErrorSummary::sorted_files) (sorted by name), matching Go,
/// which sorts the same unordered grouping for output.
// Go: internal/diagnosticwriter/diagnosticwriter.go:ErrorSummary
pub struct ErrorSummary<'a> {
    /// The total number of error-category diagnostics counted.
    pub total_error_count: i32,
    /// Errors that are not associated with any file.
    pub global_errors: Vec<&'a dyn Diagnostic>,
    /// Errors grouped by their file name.
    pub errors_by_file: HashMap<String, Vec<&'a dyn Diagnostic>>,
    /// File names that have errors, sorted lexicographically.
    pub sorted_files: Vec<String>,
}

/// Writes a localized error summary (`Found N errors …`) followed, when more
/// than one file has errors, by a per-file count table.
///
/// Nothing is written when there are no errors.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteErrorSummaryText
pub fn write_error_summary_text(
    out: &mut dyn fmt::Write,
    all_diagnostics: &[&dyn Diagnostic],
    format_opts: &FormattingOptions,
) {
    let error_summary = get_error_summary(all_diagnostics);
    let total_error_count = error_summary.total_error_count;
    if total_error_count == 0 {
        return;
    }

    let first_file_name = match error_summary.sorted_files.first() {
        Some(name) => {
            let errs = &error_summary.errors_by_file[name];
            pretty_path_for_file_error(errs.first().and_then(|d| d.file()), errs, format_opts)
        }
        None => String::new(),
    };
    let num_erroring_files = error_summary.errors_by_file.len();
    let locale = &format_opts.locale;

    let total_str = total_error_count.to_string();
    let num_str = num_erroring_files.to_string();
    let message = if total_error_count == 1 {
        // Special-case a single error.
        if !error_summary.global_errors.is_empty() || first_file_name.is_empty() {
            FOUND_1_ERROR.localize(locale, &[])
        } else {
            FOUND_1_ERROR_IN_0.localize(locale, &[first_file_name.as_str()])
        }
    } else {
        match num_erroring_files {
            0 => FOUND_0_ERRORS.localize(locale, &[total_str.as_str()]),
            1 => FOUND_0_ERRORS_IN_THE_SAME_FILE_STARTING_AT_COLON_1
                .localize(locale, &[total_str.as_str(), first_file_name.as_str()]),
            _ => {
                FOUND_0_ERRORS_IN_1_FILES.localize(locale, &[total_str.as_str(), num_str.as_str()])
            }
        }
    };

    write_str(out, &format_opts.new_line);
    write_str(out, &message);
    write_str(out, &format_opts.new_line);
    write_str(out, &format_opts.new_line);
    if num_erroring_files > 1 {
        write_tabular_errors_display(out, &error_summary, format_opts);
        write_str(out, &format_opts.new_line);
    }
}

// Counts error-category diagnostics, partitioning them into global errors and a
// by-file-name grouping, and returns the file names sorted lexicographically.
// Go: internal/diagnosticwriter/diagnosticwriter.go:getErrorSummary
fn get_error_summary<'a>(diags: &[&'a dyn Diagnostic]) -> ErrorSummary<'a> {
    let mut total_error_count = 0;
    let mut global_errors: Vec<&'a dyn Diagnostic> = Vec::new();
    let mut errors_by_file: HashMap<String, Vec<&'a dyn Diagnostic>> = HashMap::new();

    for &diagnostic in diags {
        if diagnostic.category() != Category::Error {
            continue;
        }
        total_error_count += 1;
        match diagnostic.file() {
            None => global_errors.push(diagnostic),
            Some(file) => errors_by_file
                .entry(file.file_name().to_string())
                .or_default()
                .push(diagnostic),
        }
    }

    // Go uses an unordered map then sorts for consistency; sort by file name.
    let mut sorted_files: Vec<String> = errors_by_file.keys().cloned().collect();
    sorted_files.sort();

    ErrorSummary {
        total_error_count,
        global_errors,
        errors_by_file,
        sorted_files,
    }
}

// Writes the left-aligned `count  path:line` table of per-file error counts.
// Go: internal/diagnosticwriter/diagnosticwriter.go:writeTabularErrorsDisplay
fn write_tabular_errors_display(
    out: &mut dyn fmt::Write,
    error_summary: &ErrorSummary,
    format_opts: &FormattingOptions,
) {
    let mut max_errors = 0usize;
    for errors_for_file in error_summary.errors_by_file.values() {
        max_errors = max_errors.max(errors_for_file.len());
    }

    // TODO(port): mirrors Go's note that this header was never localized.
    let header_row = ERRORS_FILES.localize(&format_opts.locale, &[]);
    let left_column_heading_length = header_row.split(' ').next().unwrap_or("").len();
    let length_of_biggest_error_count = max_errors.to_string().len();
    let left_padding_goal = left_column_heading_length.max(length_of_biggest_error_count);
    let header_padding = length_of_biggest_error_count.saturating_sub(left_column_heading_length);

    write_str(out, &" ".repeat(header_padding));
    write_str(out, &header_row);
    write_str(out, &format_opts.new_line);

    for file_name in &error_summary.sorted_files {
        let file_errors = &error_summary.errors_by_file[file_name];
        let error_count = file_errors.len();
        let _ = write!(out, "{error_count:>left_padding_goal$}  ");
        let file = file_errors.first().and_then(|d| d.file());
        write_str(
            out,
            &pretty_path_for_file_error(file, file_errors, format_opts),
        );
        write_str(out, &format_opts.new_line);
    }
}

// Formats `name<grey>:line<reset>` for a file's first error, relativizing the
// name only when both it and the current directory are absolute. Returns the
// empty string for a missing file or an empty error list.
// Go: internal/diagnosticwriter/diagnosticwriter.go:prettyPathForFileError
fn pretty_path_for_file_error(
    file: Option<&dyn FileLike>,
    file_errors: &[&dyn Diagnostic],
    format_opts: &FormattingOptions,
) -> String {
    let file = match file {
        Some(file) if !file_errors.is_empty() => file,
        _ => return String::new(),
    };
    let line = get_ecma_line_of_position(file, file_errors[0].pos());
    let mut file_name = file.file_name().to_string();
    if path_is_absolute(&file_name)
        && path_is_absolute(&format_opts.compare_paths_options.current_directory)
    {
        file_name = convert_to_relative_path(file.file_name(), &format_opts.compare_paths_options);
    }
    format!(
        "{}{}:{}{}",
        file_name,
        FOREGROUND_COLOR_ESCAPE_GREY,
        line + 1,
        RESET_ESCAPE_SEQUENCE
    )
}

/// Converts a slice of concrete diagnostics into owned trait objects.
///
/// Mirrors Go's generic `ToDiagnostics`, which widens `[]T` to `[]Diagnostic`.
///
/// Side effects: none (pure).
// Go: internal/diagnosticwriter/diagnosticwriter.go:ToDiagnostics
pub fn to_diagnostics<T: Diagnostic + 'static>(diags: Vec<T>) -> Vec<Box<dyn Diagnostic>> {
    diags
        .into_iter()
        .map(|d| Box::new(d) as Box<dyn Diagnostic>)
        .collect()
}

/// Writes a `[time] message` status line, coloring the timestamp grey.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:FormatDiagnosticsStatusWithColorAndTime
pub fn format_diagnostics_status_with_color_and_time(
    out: &mut dyn fmt::Write,
    time: &str,
    diag: &dyn Diagnostic,
    format_opts: &FormattingOptions,
) {
    write_str(out, "[");
    write_with_style_and_reset(out, time, FOREGROUND_COLOR_ESCAPE_GREY);
    write_str(out, "] ");
    write_flattened_diagnostic_message(out, diag, &format_opts.new_line, &format_opts.locale);
}

/// Writes a `time - message` status line without color.
///
/// Side effects: appends to `out`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:FormatDiagnosticsStatusAndTime
pub fn format_diagnostics_status_and_time(
    out: &mut dyn fmt::Write,
    time: &str,
    diag: &dyn Diagnostic,
    format_opts: &FormattingOptions,
) {
    write_str(out, time);
    write_str(out, " - ");
    write_flattened_diagnostic_message(out, diag, &format_opts.new_line, &format_opts.locale);
}

/// Diagnostic codes that begin a fresh watch-mode compilation (and therefore
/// trigger a screen clear): "Starting compilation in watch mode" (6031) and
/// "File change detected. Starting incremental compilation" (6032).
// Go: internal/diagnosticwriter/diagnosticwriter.go:ScreenStartingCodes
pub static SCREEN_STARTING_CODES: LazyLock<[i32; 2]> = LazyLock::new(|| {
    [
        STARTING_COMPILATION_IN_WATCH_MODE.code(),
        FILE_CHANGE_DETECTED_STARTING_INCREMENTAL_COMPILATION.code(),
    ]
});

/// Clears the terminal when `diag` starts a watch-mode compilation, unless
/// output-preserving or diagnostics options are set.
///
/// Returns whether the clear sequence (`ESC[2J ESC[3J ESC[H`) was written.
///
/// Side effects: appends the clear sequence to `out` when it returns `true`.
// Go: internal/diagnosticwriter/diagnosticwriter.go:TryClearScreen
pub fn try_clear_screen(
    out: &mut dyn fmt::Write,
    diag: &dyn Diagnostic,
    options: &CompilerOptions,
) -> bool {
    if !options.preserve_watch_output.is_true()
        && !options.extended_diagnostics.is_true()
        && !options.diagnostics.is_true()
        && SCREEN_STARTING_CODES.contains(&diag.code())
    {
        // Clear screen and move cursor to home position.
        write_str(out, "\u{1b}[2J\u{1b}[3J\u{1b}[H");
        return true;
    }
    false
}

// Returns the ANSI foreground color escape used to style a diagnostic of the
// given category. Unlike Go (which `panic`s on an unhandled category), the Rust
// `Category` enum is closed, so the match is exhaustive and total.
// Go: internal/diagnosticwriter/diagnosticwriter.go:getCategoryFormat
fn get_category_format(category: Category) -> &'static str {
    match category {
        Category::Error => FOREGROUND_COLOR_ESCAPE_RED,
        Category::Warning => FOREGROUND_COLOR_ESCAPE_YELLOW,
        Category::Suggestion => FOREGROUND_COLOR_ESCAPE_GREY,
        Category::Message => FOREGROUND_COLOR_ESCAPE_BLUE,
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
