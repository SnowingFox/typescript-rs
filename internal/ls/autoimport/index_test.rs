use super::*;

// Go: internal/ls/autoimport/index_test.go:testEntry
#[derive(Clone)]
struct TestEntry {
    name: String,
    package_: String,
}

impl TestEntry {
    fn new(name: &str, package_: &str) -> TestEntry {
        TestEntry {
            name: name.to_string(),
            package_: package_.to_string(),
        }
    }
}

impl Named for TestEntry {
    fn name(&self) -> String {
        self.name.clone()
    }
}

// Go: internal/ls/autoimport/index_test.go:TestIndexClone/filters entries by package
#[test]
fn clone_filters_entries_by_package() {
    let mut idx: Index<TestEntry> = Index::default();
    idx.insert_as_words(TestEntry::new("fooBar", "pkg-a"));
    idx.insert_as_words(TestEntry::new("bazQux", "pkg-b"));
    idx.insert_as_words(TestEntry::new("fooQux", "pkg-a"));

    // Clone excluding pkg-b.
    let cloned = idx.clone_filtered(|e| e.package_ != "pkg-b");

    // Original should have all 3 entries.
    assert_eq!(idx.entries.len(), 3);

    // Cloned should have 2 entries (only pkg-a).
    assert_eq!(cloned.entries.len(), 2);

    // Search should work on the cloned index.
    let results = cloned.find("fooBar", true);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "fooBar");

    // bazQux should not be in the cloned index.
    let results = cloned.find("bazQux", true);
    assert_eq!(results.len(), 0);

    // Word-prefix search should work.
    let results = cloned.search_word_prefix("foo");
    assert_eq!(results.len(), 2);
}

// Go: internal/ls/autoimport/index_test.go:TestIndexClone/handles nil index
//
// NOTE(port): Go's `var idx *Index[...]; idx.Clone(...)` exercises a nil-receiver
// guard that returns nil. Rust references are non-null, so there is no callable
// nil-`&Index`; the guard has no analog. The reachable equivalent (cloning an
// empty index) is covered by `clone_handles_empty_index` below.

// Go: internal/ls/autoimport/index_test.go:TestIndexClone/handles empty index
#[test]
fn clone_handles_empty_index() {
    let idx: Index<TestEntry> = Index::default();
    let cloned = idx.clone_filtered(|_| true);
    assert_eq!(cloned.entries.len(), 0);
}

// Go: internal/ls/autoimport/index_test.go:TestIndexClone/filters all entries
#[test]
fn clone_filters_all_entries() {
    let mut idx: Index<TestEntry> = Index::default();
    idx.insert_as_words(TestEntry::new("fooBar", "pkg-a"));
    idx.insert_as_words(TestEntry::new("bazQux", "pkg-b"));

    let cloned = idx.clone_filtered(|_| false);
    assert_eq!(cloned.entries.len(), 0);
    assert_eq!(cloned.index.len(), 0);
}

// --- Additional behavior-level tests (beyond Go's single test). ---

// Go: internal/ls/autoimport/index.go:Index.Find
#[test]
fn find_case_sensitive_vs_insensitive() {
    let mut idx: Index<TestEntry> = Index::default();
    idx.insert_as_words(TestEntry::new("FooBar", "pkg-a"));

    // Case-sensitive exact match.
    assert_eq!(idx.find("FooBar", true).len(), 1);
    // Case-sensitive mismatch (different casing) misses.
    assert_eq!(idx.find("foobar", true).len(), 0);
    // Case-insensitive match folds case.
    assert_eq!(idx.find("foobar", false).len(), 1);
}

#[test]
fn find_empty_inputs_return_empty() {
    let idx: Index<TestEntry> = Index::default();
    // Empty index.
    assert_eq!(idx.find("anything", true).len(), 0);

    let mut idx: Index<TestEntry> = Index::default();
    idx.insert_as_words(TestEntry::new("foo", "p"));
    // Empty query name.
    assert_eq!(idx.find("", true).len(), 0);
    // Unindexed first letter.
    assert_eq!(idx.find("zoo", true).len(), 0);
}

// Go: internal/ls/autoimport/index.go:Index.SearchWordPrefix
#[test]
fn search_word_prefix_matches_inner_words() {
    let mut idx: Index<TestEntry> = Index::default();
    idx.insert_as_words(TestEntry::new("fooBar", "p"));
    idx.insert_as_words(TestEntry::new("bazQux", "p"));

    // "bar" matches the inner word of fooBar (lowercase word-start key).
    let results = idx.search_word_prefix("bar");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "fooBar");

    // "baz" matches bazQux's name start.
    assert_eq!(idx.search_word_prefix("baz").len(), 1);

    // An empty prefix returns every entry.
    assert_eq!(idx.search_word_prefix("").len(), 2);

    // A first letter no word starts with returns nothing.
    assert_eq!(idx.search_word_prefix("z").len(), 0);
}

// Go: internal/ls/autoimport/index.go:containsCharsInOrder
#[test]
fn contains_chars_in_order_behavior() {
    assert!(contains_chars_in_order("fooBar", "fb"));
    assert!(contains_chars_in_order("fooBar", "foobar"));
    assert!(!contains_chars_in_order("fooBar", "bf"));
    assert!(!contains_chars_in_order("foo", "fooo"));
    assert!(contains_chars_in_order("anything", ""));
}

// Go: internal/ls/autoimport/index.go:Index.insertAsWords (empty-name panic)
#[test]
#[should_panic(expected = "Cannot index entry with empty name")]
fn insert_empty_name_panics() {
    let mut idx: Index<TestEntry> = Index::default();
    idx.insert_as_words(TestEntry::new("", "p"));
}
