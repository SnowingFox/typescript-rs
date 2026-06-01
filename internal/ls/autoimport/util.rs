//! Small helpers shared by the export index.
//!
//! Ports the reachable, AST-free parts of Go `internal/ls/autoimport/util.go`.
//! The checker / module-resolver / vfs helpers (`getResolvedPackageNames`,
//! `getPackageRealpathFuncs`, `getPackageNamesInNodeModules`, the checker pool,
//! ...) are deferred until their dependencies (`tsgo_checker`, `tsgo_compiler`)
//! are available — see the crate worklog DEFER list.

/// Splits an identifier into its constituent words (camelCase + snake_case) and
/// returns the **byte** start index of each word. The first index is always 0.
///
/// Mirrors Go `wordIndices`: a new word starts after a single `_` (not a run of
/// `_`), and at an uppercase rune that either follows a lowercase rune or
/// precedes one (so `URL` in `ParseURL` and `Http` in `XMLHttpRequest` both
/// split correctly).
///
/// # Examples
/// ```
/// use tsgo_ls_autoimport::word_indices;
/// let s = "ParseURL";
/// let words: Vec<&str> = word_indices(s).into_iter().map(|i| &s[i..]).collect();
/// assert_eq!(words, vec!["ParseURL", "URL"]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/autoimport/util.go:wordIndices
pub fn word_indices(s: &str) -> Vec<usize> {
    let bytes = s.as_bytes();
    let mut indices = Vec::new();
    for (byte_index, rune) in s.char_indices() {
        if byte_index == 0 {
            indices.push(byte_index);
            continue;
        }
        if rune == '_' {
            // A single `_` starts a new word; a run of `_` does not.
            if byte_index + 1 < s.len() && bytes[byte_index + 1] != b'_' {
                indices.push(byte_index + 1);
            }
            continue;
        }
        // An uppercase rune starts a word if it follows a lowercase rune
        // (`camelCase`) or precedes one (`ParseURL` -> `URL`). Go indexes the
        // "next" rune at the raw byte `byteIndex+1`; for a multi-byte uppercase
        // rune that lands mid-rune and decodes to U+FFFD, which is not lower —
        // `decode_rune_is_lower` reproduces that.
        if rune.is_uppercase()
            && (prev_rune_is_lower(s, byte_index) || decode_rune_is_lower(bytes, byte_index + 1))
        {
            indices.push(byte_index);
        }
    }
    indices
}

/// Whether the rune immediately before `byte_index` (a char boundary) is
/// lowercase. Mirrors `unicode.IsLower(utf8.DecodeLastRuneInString(s[:byteIndex]))`.
fn prev_rune_is_lower(s: &str, byte_index: usize) -> bool {
    s[..byte_index]
        .chars()
        .next_back()
        .is_some_and(char::is_lowercase)
}

/// Whether the rune decoded starting at raw byte offset `pos` is lowercase.
/// Mirrors `unicode.IsLower(utf8.DecodeRuneInString(s[pos:]))`: a position past
/// the end or starting mid-rune decodes to U+FFFD, which is not lowercase.
fn decode_rune_is_lower(bytes: &[u8], pos: usize) -> bool {
    if pos >= bytes.len() {
        return false;
    }
    let rest = &bytes[pos..];
    let valid_prefix = match std::str::from_utf8(rest) {
        Ok(s) => s,
        Err(err) => {
            // Decoding starts mid-rune (`valid_up_to == 0`) -> U+FFFD.
            std::str::from_utf8(&rest[..err.valid_up_to()]).unwrap_or("")
        }
    };
    valid_prefix.chars().next().is_some_and(char::is_lowercase)
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
