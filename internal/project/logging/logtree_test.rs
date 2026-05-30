use super::LogTree;
use crate::Logger;

// Go: internal/project/logging/logtree_test.go — `TestLogTree` has an empty
// body in Go (behavior covered by P10 parity); these tests exercise the public
// Fork/Embed/String surface with values derived from the Go semantics.

/// Asserts `ts` matches the `[HH:MM:SS.mmm]` header shape.
fn assert_timestamp(ts: &str) {
    let bytes = ts.as_bytes();
    assert_eq!(ts.len(), 14, "timestamp {ts:?}");
    for (i, b) in bytes.iter().enumerate() {
        match i {
            0 => assert_eq!(*b, b'['),
            13 => assert_eq!(*b, b']'),
            3 | 6 => assert_eq!(*b, b':'),
            9 => assert_eq!(*b, b'.'),
            _ => assert!(b.is_ascii_digit(), "expected digit at {i} in {ts:?}"),
        }
    }
}

// Go: internal/project/logging/logtree.go:LogTree.String
#[test]
fn logtree_log_then_string_renders_header_and_entry() {
    let root = LogTree::new("root");
    root.log("hello");

    let out = root.string();
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("======== root ========"));

    let entry = lines.next().expect("an entry line");
    let (ts, message) = entry.split_once(' ').expect("timestamp then message");
    assert_timestamp(ts);
    assert_eq!(message, "hello");

    assert_eq!(lines.next(), None, "no extra lines");
}

/// Splits an entry line into (leading tabs, timestamp, message).
fn split_entry(line: &str) -> (usize, &str, &str) {
    let indent = line.len() - line.trim_start_matches('\t').len();
    let body = &line[indent..];
    let (ts, message) = body.split_once(' ').expect("timestamp then message");
    assert_timestamp(ts);
    (indent, ts, message)
}

// Go: internal/project/logging/logtree.go:LogTree.Fork
#[test]
fn logtree_fork_nests_child_entries_under_indent() {
    let root = LogTree::new("root");
    root.log("a");
    let child = root.fork("loading");
    child.log("b");
    child.log("c");

    let out = root.string();
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("======== root ========"));

    let (indent, _, msg) = split_entry(lines.next().unwrap());
    assert_eq!((indent, msg), (0, "a"));
    let (indent, _, msg) = split_entry(lines.next().unwrap());
    assert_eq!((indent, msg), (0, "loading"));
    let (indent, _, msg) = split_entry(lines.next().unwrap());
    assert_eq!((indent, msg), (1, "b"));
    let (indent, _, msg) = split_entry(lines.next().unwrap());
    assert_eq!((indent, msg), (1, "c"));
    assert_eq!(lines.next(), None);
}

// Go: internal/project/logging/logtree.go:LogTree.Embed
#[test]
fn logtree_embed_inlines_other_tree_under_its_name() {
    let sub = LogTree::new("subtree");
    sub.log("x");

    let root = LogTree::new("root");
    root.embed(&sub);

    let out = root.string();
    let mut lines = out.lines();
    // Only the root prints a `======== name ========` header; the embedded
    // tree's name becomes a plain entry, with its logs nested one level in.
    assert_eq!(lines.next(), Some("======== root ========"));
    let (indent, _, msg) = split_entry(lines.next().unwrap());
    assert_eq!((indent, msg), (0, "subtree"));
    let (indent, _, msg) = split_entry(lines.next().unwrap());
    assert_eq!((indent, msg), (1, "x"));
    assert_eq!(lines.next(), None);
}

// Go: internal/project/logging/logtree_test.go:TestLogTreeImplementsLogger
#[test]
fn logtree_implements_logger() {
    fn assert_logger<T: Logger>() {}
    assert_logger::<LogTree>();
    // Usable as a trait object, like Go's `var _ Logger = (*LogTree)(nil)`.
    let tree = LogTree::new("x");
    let _as_dyn: &dyn Logger = &tree;
}

// Go: internal/project/logging/logtree.go:LogTree.String (panics off the root)
#[test]
#[should_panic(expected = "can only call String on root LogTree")]
fn logtree_string_on_non_root_panics() {
    let root = LogTree::new("root");
    let child = root.fork("child");
    let _ = child.string();
}

// Go: internal/project/logging/logtree.go:LogTree.Fork (verbose is a snapshot)
#[test]
fn logtree_fork_snapshots_verbose_flag() {
    let root = LogTree::new("root");
    assert!(!root.is_verbose());
    root.set_verbose(true);

    let child = root.fork("child");
    assert!(child.is_verbose(), "child inherits verbose at fork time");

    // A later change to the parent does not retroactively affect the child.
    root.set_verbose(false);
    assert!(child.is_verbose());
    assert!(child.verbose().is_some());
}
