//! The auto-import export index.
//!
//! 1:1 port of Go `internal/ls/autoimport/index.go`. [`Index<T>`] stores entries
//! together with a `char -> Vec<entry index>` map: a name start is keyed by its
//! uppercased first letter, and every subsequent word start is keyed by its
//! lowercased first letter. This lets completions look up candidates by the
//! first character the user typed (case-insensitively across word boundaries)
//! without scanning every export.

use std::collections::HashMap;

use crate::util::word_indices;

/// Anything that can provide its indexable name.
///
/// Mirrors Go's `Named` constraint (`Name() string`).
// Go: internal/ls/autoimport/index.go:Named
pub trait Named {
    /// The entry's name, used to key the index.
    fn name(&self) -> String;
}

/// An index over `T` keyed by the first letter of each word in `T`'s name.
///
/// Uppercase keys map to entries whose **name** starts with that letter;
/// lowercase keys map to entries that *contain* a word starting with that
/// letter. Entries are stored in insertion order in `entries`; the `index` map
/// holds positions into `entries`.
///
/// # Examples
/// ```
/// use tsgo_ls_autoimport::{Index, Named};
/// struct E(&'static str);
/// impl Named for E {
///     fn name(&self) -> String {
///         self.0.to_string()
///     }
/// }
/// let mut idx = Index::default();
/// idx.insert_as_words(E("fooBar"));
/// assert_eq!(idx.find("fooBar", true).len(), 1);
/// assert_eq!(idx.search_word_prefix("bar").len(), 1);
/// ```
///
/// Side effects: `insert_as_words` mutates the index in place.
// Go: internal/ls/autoimport/index.go:Index
#[derive(Debug)]
pub struct Index<T: Named> {
    entries: Vec<T>,
    index: HashMap<char, Vec<usize>>,
}

impl<T: Named> Default for Index<T> {
    fn default() -> Self {
        Index {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<T: Named> Index<T> {
    /// Returns the entries whose name equals `name`. When `case_sensitive` is
    /// false the comparison folds case (ASCII-fold, matching Go's
    /// `strings.EqualFold` for the identifiers we index).
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/autoimport/index.go:Index.Find
    pub fn find(&self, name: &str, case_sensitive: bool) -> Vec<&T> {
        if self.entries.is_empty() || name.is_empty() {
            return Vec::new();
        }
        let Some(first_rune) = name.chars().next() else {
            return Vec::new();
        };
        let first_rune_upper = to_upper(first_rune);
        let Some(candidates) = self.index.get(&first_rune_upper) else {
            return Vec::new();
        };

        let mut results = Vec::new();
        for &entry_index in candidates {
            let entry = &self.entries[entry_index];
            let entry_name = entry.name();
            if (case_sensitive && entry_name == name)
                || (!case_sensitive && equal_fold(&entry_name, name))
            {
                results.push(entry);
            }
        }
        results
    }

    /// Returns each entry whose name contains a word beginning with the first
    /// character of `prefix` and that contains all characters of `prefix` in
    /// order (case-insensitive). An empty prefix returns every entry.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/autoimport/index.go:Index.SearchWordPrefix
    pub fn search_word_prefix(&self, prefix: &str) -> Vec<&T> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        if prefix.is_empty() {
            return self.entries.iter().collect();
        }

        let prefix = prefix.to_lowercase();
        let Some(first_rune) = prefix.chars().next() else {
            return Vec::new();
        };

        let first_rune_upper = to_upper(first_rune);
        let first_rune_lower = to_lower(first_rune);

        let name_starts: &[usize] = self
            .index
            .get(&first_rune_upper)
            .map_or(&[][..], Vec::as_slice);
        let word_starts: &[usize] = if first_rune_upper != first_rune_lower {
            self.index
                .get(&first_rune_lower)
                .map_or(&[][..], Vec::as_slice)
        } else {
            &[]
        };
        if name_starts.len() + word_starts.len() == 0 {
            return Vec::new();
        }

        let mut results = Vec::with_capacity(name_starts.len() + word_starts.len());
        for starts in [name_starts, word_starts] {
            for &i in starts {
                let entry = &self.entries[i];
                if contains_chars_in_order(&entry.name(), &prefix) {
                    results.push(entry);
                }
            }
        }
        results
    }

    /// Adds `value`, keying the index by the first letter of each word in its
    /// name.
    ///
    /// # Panics
    /// Panics on an empty name, mirroring Go's `insertAsWords`.
    ///
    /// Side effects: appends to `entries` and updates `index`.
    // Go: internal/ls/autoimport/index.go:Index.insertAsWords
    pub fn insert_as_words(&mut self, value: T) {
        let name = value.name();
        assert!(!name.is_empty(), "Cannot index entry with empty name");

        let entry_index = self.entries.len();
        self.entries.push(value);

        let indices = word_indices(&name);
        let mut seen_runes: HashMap<char, bool> = HashMap::new();

        for (i, &start) in indices.iter().enumerate() {
            let substr = &name[start..];
            let Some(mut first_rune) = substr.chars().next() else {
                continue;
            };
            if i == 0 {
                // Name start keyed by uppercase.
                first_rune = to_upper(first_rune);
                self.index.entry(first_rune).or_default().push(entry_index);
                // (Still mark seen in case the first character is non-alphabetic.)
                seen_runes.insert(first_rune, true);
            } else {
                // Subsequent word starts keyed by lowercase.
                first_rune = to_lower(first_rune);
                if !seen_runes.get(&first_rune).copied().unwrap_or(false) {
                    self.index.entry(first_rune).or_default().push(entry_index);
                    seen_runes.insert(first_rune, true);
                }
            }
        }
    }
}

impl<T: Named + Clone> Index<T> {
    /// Creates a new index containing only the entries for which `filter`
    /// returns true, remapping the word index to the surviving positions.
    ///
    /// Side effects: none (allocates a fresh index).
    // Go: internal/ls/autoimport/index.go:Index.Clone
    pub fn clone_filtered(&self, filter: impl Fn(&T) -> bool) -> Index<T> {
        let mut new_idx = Index {
            entries: Vec::with_capacity(self.entries.len()),
            index: HashMap::with_capacity(self.index.len()),
        };

        // Build a mapping from old entry position to new entry position for the
        // surviving entries.
        let mut old_to_new: HashMap<usize, usize> = HashMap::with_capacity(self.entries.len());
        for (old_index, entry) in self.entries.iter().enumerate() {
            if filter(entry) {
                let new_index = new_idx.entries.len();
                new_idx.entries.push(entry.clone());
                old_to_new.insert(old_index, new_index);
            }
        }

        // Rebuild the word index with remapped positions.
        for (&r, old_indices) in &self.index {
            let mut new_indices = Vec::with_capacity(old_indices.len());
            for old_index in old_indices {
                if let Some(&new_index) = old_to_new.get(old_index) {
                    new_indices.push(new_index);
                }
            }
            if !new_indices.is_empty() {
                new_idx.index.insert(r, new_indices);
            }
        }

        new_idx
    }
}

/// Reports whether `s` contains all characters of `pattern` in order
/// (case-insensitive). Both inputs are lowercased before scanning.
///
/// # Examples
/// ```
/// use tsgo_ls_autoimport::index::contains_chars_in_order;
/// assert!(contains_chars_in_order("fooBar", "fb"));
/// assert!(!contains_chars_in_order("fooBar", "bf"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/autoimport/index.go:containsCharsInOrder
pub fn contains_chars_in_order(s: &str, pattern: &str) -> bool {
    let s = s.to_lowercase();
    let pattern = pattern.to_lowercase();

    let mut pattern_chars = pattern.chars().peekable();
    for ch in s.chars() {
        match pattern_chars.peek() {
            Some(&pat) if pat == ch => {
                pattern_chars.next();
            }
            _ => {}
        }
    }
    pattern_chars.peek().is_none()
}

/// Single-rune uppercase, mirroring Go `unicode.ToUpper` (one rune -> one rune).
fn to_upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// Single-rune lowercase, mirroring Go `unicode.ToLower`.
fn to_lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Case-insensitive equality matching Go `strings.EqualFold` for the ASCII /
/// BMP identifiers the index holds.
fn equal_fold(a: &str, b: &str) -> bool {
    let mut bc = b.chars();
    for ca in a.chars() {
        match bc.next() {
            Some(cb) if to_lower(ca) == to_lower(cb) => {}
            _ => return false,
        }
    }
    bc.next().is_none()
}

#[cfg(test)]
#[path = "index_test.rs"]
mod tests;
