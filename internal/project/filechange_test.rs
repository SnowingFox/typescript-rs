// Go: internal/project/filechange.go
use super::*;
use tsgo_lsproto::DocumentUri;

// -- FileChangeKind --------------------------------------------------

#[test]
fn file_change_kind_is_watch_kind_open() {
    // Go: internal/project/filechange.go:IsWatchKind
    assert!(!FileChangeKind::Open.is_watch_kind());
}

#[test]
fn file_change_kind_is_watch_kind_close() {
    assert!(!FileChangeKind::Close.is_watch_kind());
}

#[test]
fn file_change_kind_is_watch_kind_change() {
    assert!(!FileChangeKind::Change.is_watch_kind());
}

#[test]
fn file_change_kind_is_watch_kind_save() {
    assert!(!FileChangeKind::Save.is_watch_kind());
}

#[test]
fn file_change_kind_is_watch_kind_watch_create() {
    assert!(FileChangeKind::WatchCreate.is_watch_kind());
}

#[test]
fn file_change_kind_is_watch_kind_watch_change() {
    assert!(FileChangeKind::WatchChange.is_watch_kind());
}

#[test]
fn file_change_kind_is_watch_kind_watch_delete() {
    assert!(FileChangeKind::WatchDelete.is_watch_kind());
}

// -- FileChangeSummary::is_empty -------------------------------------

#[test]
fn empty_summary_is_empty() {
    // Go: internal/project/filechange.go:IsEmpty
    let s = FileChangeSummary::default();
    assert!(s.is_empty());
}

#[test]
fn summary_with_opened_is_not_empty() {
    let s = FileChangeSummary {
        opened: DocumentUri("file:///a.ts".to_string()),
        ..Default::default()
    };
    assert!(!s.is_empty());
}

#[test]
fn summary_with_reopened_is_not_empty() {
    let s = FileChangeSummary {
        reopened: DocumentUri("file:///a.ts".to_string()),
        ..Default::default()
    };
    assert!(!s.is_empty());
}

#[test]
fn summary_with_closed_is_not_empty() {
    let mut s = FileChangeSummary::default();
    s.closed.add(DocumentUri("file:///a.ts".to_string()));
    assert!(!s.is_empty());
}

#[test]
fn summary_with_changed_is_not_empty() {
    let mut s = FileChangeSummary::default();
    s.changed.add(DocumentUri("file:///a.ts".to_string()));
    assert!(!s.is_empty());
}

#[test]
fn summary_with_invalidate_all_is_not_empty() {
    let s = FileChangeSummary {
        invalidate_all: true,
        ..Default::default()
    };
    assert!(!s.is_empty());
}

// -- FileChangeSummary::has_excessive_watch_events -------------------

#[test]
fn has_excessive_watch_events_false_for_empty() {
    // Go: internal/project/filechange.go:HasExcessiveWatchEvents
    let s = FileChangeSummary::default();
    assert!(!s.has_excessive_watch_events());
}

#[test]
fn has_excessive_watch_events_true_when_invalidate_all() {
    let s = FileChangeSummary {
        invalidate_all: true,
        ..Default::default()
    };
    assert!(s.has_excessive_watch_events());
}

#[test]
fn has_excessive_watch_events_true_when_threshold_exceeded() {
    let mut s = FileChangeSummary::default();
    for i in 0..=EXCESSIVE_CHANGE_THRESHOLD {
        s.created.add(DocumentUri(format!("file:///f{}.ts", i)));
    }
    assert!(s.has_excessive_watch_events());
}

// -- FileChangeSummary::has_excessive_non_create_watch_events --------

#[test]
fn has_excessive_non_create_watch_events_false_for_empty() {
    // Go: internal/project/filechange.go:HasExcessiveNonCreateWatchEvents
    let s = FileChangeSummary::default();
    assert!(!s.has_excessive_non_create_watch_events());
}

#[test]
fn has_excessive_non_create_watch_events_ignores_created() {
    let mut s = FileChangeSummary::default();
    for i in 0..=EXCESSIVE_CHANGE_THRESHOLD {
        s.created.add(DocumentUri(format!("file:///f{}.ts", i)));
    }
    assert!(!s.has_excessive_non_create_watch_events());
}

#[test]
fn has_excessive_non_create_watch_events_counts_deleted() {
    let mut s = FileChangeSummary::default();
    for i in 0..=EXCESSIVE_CHANGE_THRESHOLD {
        s.deleted.add(DocumentUri(format!("file:///f{}.ts", i)));
    }
    assert!(s.has_excessive_non_create_watch_events());
}

// -- merge_file_change_summary ---------------------------------------

#[test]
fn merge_empty_into_empty_stays_empty() {
    // Go: internal/project/filechange.go:mergeFileChangeSummary
    let mut dst = FileChangeSummary::default();
    let src = FileChangeSummary::default();
    merge_file_change_summary(&mut dst, src);
    assert!(dst.is_empty());
}

#[test]
fn merge_propagates_invalidate_all() {
    let mut dst = FileChangeSummary::default();
    let src = FileChangeSummary {
        invalidate_all: true,
        ..Default::default()
    };
    merge_file_change_summary(&mut dst, src);
    assert!(dst.invalidate_all);
}

#[test]
fn merge_combines_changed_sets() {
    let mut dst = FileChangeSummary::default();
    dst.changed.add(DocumentUri("file:///a.ts".to_string()));

    let mut src = FileChangeSummary::default();
    src.changed.add(DocumentUri("file:///b.ts".to_string()));

    merge_file_change_summary(&mut dst, src);
    assert!(dst.changed.has(&DocumentUri("file:///a.ts".to_string())));
    assert!(dst.changed.has(&DocumentUri("file:///b.ts".to_string())));
    assert_eq!(dst.changed.len(), 2);
}

#[test]
fn merge_combines_created_and_deleted() {
    let mut dst = FileChangeSummary::default();
    let mut src = FileChangeSummary::default();
    src.created.add(DocumentUri("file:///c.ts".to_string()));
    src.deleted.add(DocumentUri("file:///d.ts".to_string()));
    merge_file_change_summary(&mut dst, src);
    assert!(dst.created.has(&DocumentUri("file:///c.ts".to_string())));
    assert!(dst.deleted.has(&DocumentUri("file:///d.ts".to_string())));
}

#[test]
fn merge_propagates_includes_watch_change_outside_node_modules() {
    let mut dst = FileChangeSummary::default();
    let mut src = FileChangeSummary {
        includes_watch_change_outside_node_modules: true,
        ..Default::default()
    };
    src.changed.add(DocumentUri("file:///x.ts".to_string()));
    merge_file_change_summary(&mut dst, src);
    assert!(dst.includes_watch_change_outside_node_modules);
}
