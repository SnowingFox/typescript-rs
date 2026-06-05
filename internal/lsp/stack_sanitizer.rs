//! Port of Go `internal/lsp/stack_sanitizer.go`.
//!
//! Stack-trace sanitization for LSP telemetry: strips paths down to the
//! `typescript-go/internal/...` portion, elides function arguments, and defeats
//! VS Code's generic-secret regex so that innocuous frames like
//! `getSignatureHelp(` are not redacted.

use once_cell::sync::Lazy;
use regex::Regex;

/// Regex matching keywords that VS Code's telemetry pipeline would redact.
// Go: internal/lsp/stack_sanitizer.go:genericSecretKeywordRegex
static GENERIC_SECRET_KEYWORD_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(key|token|signature|sig|pwd)([(\[.|])").unwrap());

/// Inserts `X_X` after trigger keywords followed by punctuation we emit,
/// preventing VS Code telemetry from redacting them.
// Go: internal/lsp/stack_sanitizer.go:defeatGenericSecretRegex
pub fn defeat_generic_secret_regex(s: &str) -> String {
    GENERIC_SECRET_KEYWORD_REGEX
        .replace_all(s, "${1}X_X${2}")
        .into_owned()
}

/// Sanitizes a Go-style stack trace for telemetry transport.
///
/// Strips everything before `runtime/debug.Stack()`, then for each frame:
/// - Preserves only the `typescript-go/internal/...` portion
/// - Replaces path separators with `|>`
/// - Strips function arguments (leaving empty `()`)
/// - Marks non-internal frames as `(REDACTED FRAME)`
/// - Applies secret-keyword defeat at the end
// Go: internal/lsp/stack_sanitizer.go:sanitizeStackTrace
pub fn sanitize_stack_trace(stack: &str) -> String {
    let start_marker = "runtime/debug.Stack()";
    let start_index = match stack.find(start_marker) {
        Some(idx) => idx,
        None => return String::new(),
    };
    let stack = &stack[start_index..];

    let mut result = String::new();
    for (line_num, line) in stack.lines().enumerate() {
        if line_num > 0 {
            result.push('\n');
        }

        let trimmed_start = line.len() - line.trim_start().len();
        result.push_str(&line[..trimmed_start]);

        let line = &line[trimmed_start..];

        if let Some(our_module_index) = line.find("typescript-go/internal") {
            let line = &line[our_module_index..];
            write_sanitized_module_or_path(line, &mut result);
        } else {
            result.push_str("(REDACTED FRAME)");
        }
    }

    defeat_generic_secret_regex(&result)
}

/// Processes a single line known to contain our module path.
// Go: internal/lsp/stack_sanitizer.go:writeSanitizedModuleOrPath
fn write_sanitized_module_or_path(line: &str, result: &mut String) {
    let line = line.trim();

    let line = if let Some(idx) = line.find(" +0x") {
        &line[..idx]
    } else if let Some(idx) = line.rfind(" in goroutine ") {
        &line[..idx]
    } else {
        line
    };

    for (segment_index, segment) in line.split('/').enumerate() {
        if segment_index > 0 {
            result.push_str("|>");
        }

        if segment.ends_with(')') {
            if let Some(open_paren_index) = segment.rfind('(') {
                result.push_str(&segment[..open_paren_index]);
                result.push_str("()");
            } else {
                result.push_str("???");
            }
            continue;
        }

        result.push_str(segment);
    }
}

#[cfg(test)]
#[path = "stack_sanitizer_test.rs"]
mod tests;
