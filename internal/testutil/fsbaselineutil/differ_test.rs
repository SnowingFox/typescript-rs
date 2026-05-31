use super::*;

// There is no Go `_test.go` for this package; these behavior tests assert the
// helpers against the Go source in `differ.go`, with expected strings taken
// from the Go literals.

// Go: internal/testutil/fsbaselineutil/differ.go:SanitizeInternalSymbolName
#[test]
fn sanitize_returns_input_unchanged_when_no_marker() {
    assert_eq!(sanitize_internal_symbol_name(""), "");
    assert_eq!(sanitize_internal_symbol_name("hello world"), "hello world");
    // A bare replacement char without the "@" marker is left untouched.
    assert_eq!(sanitize_internal_symbol_name("\u{FFFD}"), "\u{FFFD}");
}

// Go: internal/testutil/fsbaselineutil/differ.go:SanitizeInternalSymbolName
#[test]
fn sanitize_replaces_symbol_id_with_placeholder() {
    assert_eq!(
        sanitize_internal_symbol_name("\u{FFFD}@foo@42"),
        "\u{FFFD}@foo@<symbolId>"
    );
}

// Go: internal/testutil/fsbaselineutil/differ.go:SanitizeInternalSymbolName
#[test]
fn sanitize_replaces_every_occurrence() {
    assert_eq!(
        sanitize_internal_symbol_name("a \u{FFFD}@foo@1 b \u{FFFD}@bar@200 c"),
        "a \u{FFFD}@foo@<symbolId> b \u{FFFD}@bar@<symbolId> c"
    );
}

// ---- test helpers: an in-memory `MapFsView` fake ----

struct FakeFs {
    entries: Vec<MapEntry>,
}

impl MapFsView for FakeFs {
    fn entries(&self) -> Vec<MapEntry> {
        self.entries
            .iter()
            .map(|e| MapEntry {
                path: e.path.clone(),
                mode: e.mode,
                data: e.data.clone(),
                mod_time: e.mod_time,
            })
            .collect()
    }
    fn get_target_of_symlink(&self, path: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|e| e.path == path && e.mode.is_symlink())
            .map(|e| format!("/{}", String::from_utf8_lossy(&e.data)))
    }
    fn has_entry(&self, path: &str) -> bool {
        self.entries.iter().any(|e| e.path == path)
    }
}

fn regular(path: &str, content: &str, mtime: SystemTime) -> MapEntry {
    MapEntry {
        path: path.into(),
        mode: tsgo_vfs::FileMode::REGULAR,
        data: content.as_bytes().to_vec(),
        mod_time: mtime,
    }
}

fn t(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
}

fn baseline_to_string<F: MapFsView>(differ: &mut FsDiffer<F>) -> String {
    let mut buf: Vec<u8> = Vec::new();
    differ.baseline_fs_with_diff(&mut buf);
    String::from_utf8(buf).unwrap()
}

// Go: differ.go:BaselineFSwithDiff + addFsEntryDiff (new regular file)
#[test]
fn first_run_reports_new_regular_file() {
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![regular("/a.ts", "hello", t(1))],
    });
    let out = baseline_to_string(&mut differ);
    assert_eq!(out, "//// [/a.ts] *new* \nhello\n\n");
}

// Go: differ.go:BaselineFSwithDiff + addFsEntryDiff (new symlink)
#[test]
fn first_run_reports_new_symlink() {
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![MapEntry {
            path: "/link".into(),
            mode: tsgo_vfs::FileMode::SYMLINK,
            data: b"target.ts".to_vec(),
            mod_time: t(0),
        }],
    });
    let out = baseline_to_string(&mut differ);
    assert_eq!(out, "//// [/link] -> /target.ts *new*\n\n");
}

// Go: differ.go:addFsEntryDiff (modified content)
#[test]
fn second_run_reports_modified_content() {
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![regular("/a.ts", "hello", t(1))],
    });
    let _ = baseline_to_string(&mut differ);
    differ.fs.entries = vec![regular("/a.ts", "world", t(1))];
    let out = baseline_to_string(&mut differ);
    assert_eq!(out, "//// [/a.ts] *modified* \nworld\n\n");
}

// Go: differ.go:addFsEntryDiff (deleted)
#[test]
fn second_run_reports_deleted_file() {
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![regular("/a.ts", "hello", t(1))],
    });
    let _ = baseline_to_string(&mut differ);
    differ.fs.entries = vec![];
    let out = baseline_to_string(&mut differ);
    assert_eq!(out, "//// [/a.ts] *deleted*\n\n");
}

// Go: differ.go:addFsEntryDiff (rewrite with same content)
#[test]
fn second_run_reports_rewrite_with_same_content() {
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![regular("/a.ts", "hello", t(1))],
    });
    let _ = baseline_to_string(&mut differ);
    // Same content and mtime, but the path was written this round.
    differ.written_files.add("/a.ts".to_string());
    let out = baseline_to_string(&mut differ);
    assert_eq!(out, "//// [/a.ts] *rewrite with same content*\n\n");
}

// Go: differ.go:addFsEntryDiff (mTime changed)
#[test]
fn second_run_reports_mtime_changed() {
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![regular("/a.ts", "hello", t(1))],
    });
    let _ = baseline_to_string(&mut differ);
    differ.fs.entries = vec![regular("/a.ts", "hello", t(2))];
    let out = baseline_to_string(&mut differ);
    assert_eq!(out, "//// [/a.ts] *mTime changed*\n\n");
}

// Go: differ.go:addFsEntryDiff (new default-lib file is suppressed)
#[test]
fn first_run_suppresses_new_default_lib_file() {
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![regular("/lib.d.ts", "lib", t(1))],
    });
    differ.default_libs = Some(Box::new(|| {
        let set = SyncSet::default();
        set.add("/lib.d.ts".to_string());
        Some(std::sync::Arc::new(set))
    }));
    let out = baseline_to_string(&mut differ);
    // The only entry is a current default lib, so nothing is reported.
    assert_eq!(out, "\n");
}

// Go: differ.go:addFsEntryDiff (*Lib* — previously a default lib, now read)
#[test]
fn second_run_reports_lib_when_default_lib_becomes_read() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let active = Arc::new(AtomicBool::new(true));
    let active_for_closure = active.clone();
    let mut differ = FsDiffer::new(FakeFs {
        entries: vec![regular("/lib.d.ts", "libcontent", t(1))],
    });
    differ.default_libs = Some(Box::new(move || {
        let set = SyncSet::default();
        if active_for_closure.load(Ordering::SeqCst) {
            set.add("/lib.d.ts".to_string());
        }
        Some(Arc::new(set))
    }));

    // Run 1: it is a current default lib, so it is suppressed and captured in
    // the snapshot's default-libs set.
    let out1 = baseline_to_string(&mut differ);
    assert_eq!(out1, "\n");

    // Run 2: same content/mtime, but no longer a default lib (it was read).
    active.store(false, Ordering::SeqCst);
    let out2 = baseline_to_string(&mut differ);
    assert_eq!(out2, "//// [/lib.d.ts] *Lib*\nlibcontent\n\n");
}
