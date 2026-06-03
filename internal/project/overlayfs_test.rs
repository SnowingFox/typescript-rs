// Go: internal/project/overlayfs_test.go
use super::*;
use crate::filechange::{FileChange, FileChangeKind};
use crate::overlayfs::{DiskFile, FileHandle, Overlay, OverlayFS};
use std::collections::HashMap;
use tsgo_lsproto::{DocumentUri, LanguageKind};
use tsgo_tspath::Path;
use tsgo_vfs::vfstest::MapFs;

fn create_overlay_fs() -> OverlayFS {
    let fs = MapFs::from_map(
        [
            ("/test1.ts", "// existing content"),
            ("/test2.ts", "// existing content"),
        ],
        false,
    );
    OverlayFS::new(Box::new(fs), HashMap::new(), |file_name: &str| {
        Path(file_name.to_string())
    })
}

const TEST_URI1: &str = "file:///test1.ts";
const TEST_URI2: &str = "file:///test2.ts";

fn uri(s: &str) -> DocumentUri {
    DocumentUri(s.to_string())
}

fn ts_lang() -> LanguageKind {
    LanguageKind::TYPE_SCRIPT
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/multiple opens should panic
#[test]
#[should_panic(expected = "can only process one file open event at a time")]
fn test_process_changes_multiple_opens_should_panic() {
    let mut fs = create_overlay_fs();
    let changes = vec![
        FileChange {
            kind: FileChangeKind::Open,
            uri: uri(TEST_URI1),
            version: 1,
            content: "const x = 1;".to_string(),
            language_kind: ts_lang(),
            changes: Vec::new(),
        },
        FileChange {
            kind: FileChangeKind::Open,
            uri: uri(TEST_URI2),
            version: 1,
            content: "const y = 2;".to_string(),
            language_kind: ts_lang(),
            changes: Vec::new(),
        },
    ];
    fs.process_changes(&changes);
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/watch create then delete becomes nothing
#[test]
fn test_process_changes_watch_create_then_delete_becomes_nothing() {
    let mut fs = create_overlay_fs();
    let changes = vec![
        FileChange {
            kind: FileChangeKind::WatchCreate,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
        FileChange {
            kind: FileChangeKind::WatchDelete,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
    ];
    let (result, _) = fs.process_changes(&changes);
    assert!(result.is_empty());
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/watch delete then create becomes change
#[test]
fn test_process_changes_watch_delete_then_create_becomes_change() {
    let mut fs = create_overlay_fs();
    let changes = vec![
        FileChange {
            kind: FileChangeKind::WatchDelete,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
        FileChange {
            kind: FileChangeKind::WatchCreate,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
    ];
    let (result, _) = fs.process_changes(&changes);
    assert_eq!(result.created.len(), 0);
    assert_eq!(result.deleted.len(), 0);
    assert!(result.changed.has(&uri(TEST_URI1)));
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/multiple watch changes deduplicated
#[test]
fn test_process_changes_multiple_watch_changes_deduplicated() {
    let mut fs = create_overlay_fs();
    let changes = vec![
        FileChange {
            kind: FileChangeKind::WatchChange,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
        FileChange {
            kind: FileChangeKind::WatchChange,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
        FileChange {
            kind: FileChangeKind::WatchChange,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
    ];
    let (result, _) = fs.process_changes(&changes);
    assert!(result.changed.has(&uri(TEST_URI1)));
    assert_eq!(result.changed.len(), 1);
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/save marks overlay as matching disk
#[test]
fn test_process_changes_save_marks_overlay_as_matching_disk() {
    let mut fs = create_overlay_fs();

    // Open file
    fs.process_changes(&[FileChange {
        kind: FileChangeKind::Open,
        uri: uri(TEST_URI1),
        version: 1,
        content: "const x = 1;".to_string(),
        language_kind: ts_lang(),
        changes: Vec::new(),
    }]);

    // Save
    let (result, _) = fs.process_changes(&[FileChange {
        kind: FileChangeKind::Save,
        uri: uri(TEST_URI1),
        version: 0,
        content: String::new(),
        language_kind: Default::default(),
        changes: Vec::new(),
    }]);
    assert!(result.is_empty());

    // Check overlay is marked as matching disk
    let fh = fs.get_file(&DocumentUri(TEST_URI1.to_string()).file_name());
    assert!(fh.is_some());
    assert!(fh.unwrap().matches_disk_text());
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/watch change on overlay marks as not matching disk
#[test]
fn test_process_changes_watch_change_on_overlay_marks_not_matching_disk() {
    let mut fs = create_overlay_fs();

    // Open
    fs.process_changes(&[FileChange {
        kind: FileChangeKind::Open,
        uri: uri(TEST_URI1),
        version: 1,
        content: "const x = 1;".to_string(),
        language_kind: ts_lang(),
        changes: Vec::new(),
    }]);
    let fh = fs.get_file(&DocumentUri(TEST_URI1.to_string()).file_name());
    assert!(!fh.unwrap().matches_disk_text());

    // Save
    fs.process_changes(&[FileChange {
        kind: FileChangeKind::Save,
        uri: uri(TEST_URI1),
        version: 0,
        content: String::new(),
        language_kind: Default::default(),
        changes: Vec::new(),
    }]);
    let fh = fs.get_file(&DocumentUri(TEST_URI1.to_string()).file_name());
    assert!(fh.unwrap().matches_disk_text());

    // Watch change
    fs.process_changes(&[FileChange {
        kind: FileChangeKind::WatchChange,
        uri: uri(TEST_URI1),
        version: 0,
        content: String::new(),
        language_kind: Default::default(),
        changes: Vec::new(),
    }]);
    let fh = fs.get_file(&DocumentUri(TEST_URI1.to_string()).file_name());
    assert!(!fh.unwrap().matches_disk_text());
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/save without overlay should not panic
#[test]
fn test_process_changes_save_without_overlay_should_not_panic() {
    let mut fs = create_overlay_fs();
    let (result, _) = fs.process_changes(&[FileChange {
        kind: FileChangeKind::Save,
        uri: uri(TEST_URI1),
        version: 0,
        content: String::new(),
        language_kind: Default::default(),
        changes: Vec::new(),
    }]);
    assert!(result.changed.has(&uri(TEST_URI1)));
}

// Go: internal/project/overlayfs_test.go:TestProcessChanges/close then open in same batch marks as changed
#[test]
fn test_process_changes_close_then_open_marks_changed() {
    let mut fs = create_overlay_fs();

    // Open
    fs.process_changes(&[FileChange {
        kind: FileChangeKind::Open,
        uri: uri(TEST_URI1),
        version: 1,
        content: "const x = 1;".to_string(),
        language_kind: ts_lang(),
        changes: Vec::new(),
    }]);

    // Close then reopen
    let (result, _) = fs.process_changes(&[
        FileChange {
            kind: FileChangeKind::Close,
            uri: uri(TEST_URI1),
            version: 0,
            content: String::new(),
            language_kind: Default::default(),
            changes: Vec::new(),
        },
        FileChange {
            kind: FileChangeKind::Open,
            uri: uri(TEST_URI1),
            version: 0,
            content: "const x = 2;".to_string(),
            language_kind: ts_lang(),
            changes: Vec::new(),
        },
    ]);
    assert!(
        result.opened.0.is_empty(),
        "close then open should not mark as opened"
    );
    assert!(
        result.changed.has(&uri(TEST_URI1)),
        "close then open should mark as changed"
    );
    let fh = fs.get_file(&DocumentUri(TEST_URI1.to_string()).file_name());
    assert_eq!(fh.unwrap().content(), "const x = 2;");
}

// --- Additional behavior tests (Go has no direct coverage) ---

// Test that DiskFile always reports version 0 and is_overlay false
#[test]
fn test_disk_file_properties() {
    let df = DiskFile::new("/foo.ts", "hello");
    assert_eq!(df.file_name(), "/foo.ts");
    assert_eq!(df.content(), "hello");
    assert_eq!(df.version(), 0);
    assert!(!df.is_overlay());
    assert!(df.matches_disk_text());
}

// Test that Overlay tracks version and reports is_overlay true
#[test]
fn test_overlay_properties() {
    let ov = Overlay::new(
        "/bar.ts".to_string(),
        "world".to_string(),
        3,
        tsgo_core::scriptkind::ScriptKind::Ts,
    );
    assert_eq!(ov.file_name(), "/bar.ts");
    assert_eq!(ov.content(), "world");
    assert_eq!(ov.version(), 3);
    assert!(ov.is_overlay());
    assert!(!ov.matches_disk_text());
}

// Test that get_file returns disk file when no overlay
#[test]
fn test_get_file_returns_disk() {
    let fs_inner = create_overlay_fs();
    let fh = fs_inner.get_file("/test1.ts");
    assert!(fh.is_some());
    let fh = fh.unwrap();
    assert_eq!(fh.content(), "// existing content");
    assert!(!fh.is_overlay());
}

// Test that get_file returns overlay when present
#[test]
fn test_get_file_returns_overlay() {
    let mut fs = create_overlay_fs();
    fs.process_changes(&[FileChange {
        kind: FileChangeKind::Open,
        uri: uri(TEST_URI1),
        version: 1,
        content: "overlay content".to_string(),
        language_kind: ts_lang(),
        changes: Vec::new(),
    }]);
    let fh = fs.get_file(&DocumentUri(TEST_URI1.to_string()).file_name());
    assert!(fh.is_some());
    let fh = fh.unwrap();
    assert_eq!(fh.content(), "overlay content");
    assert!(fh.is_overlay());
}

// Test that get_file returns None for missing file
#[test]
fn test_get_file_returns_none_for_missing() {
    let fs = create_overlay_fs();
    let fh = fs.get_file("/nonexistent.ts");
    assert!(fh.is_none());
}
