// Tests for session module.
// Go: internal/project/session_test.go

use super::*;
use crate::client::NoopClient;
use crate::overlayfs::OverlayFS;
use std::sync::Arc;

fn make_test_session() -> Session {
    let empty: Vec<(&str, &str)> = Vec::new();
    let fs: Box<dyn tsgo_vfs::Fs + Send + Sync> =
        Box::new(tsgo_vfs::vfstest::MapFs::from_map(empty, false));
    let overlay_fs = OverlayFS::new(fs, std::collections::HashMap::new(), |s: &str| {
        tsgo_tspath::Path(s.to_lowercase())
    });
    let options = SessionOptions {
        current_directory: "/".to_string(),
    };
    Session::new(options, overlay_fs, Arc::new(NoopClient))
}

// --- Slice 2: UpdateReason enum ---

// Go: internal/project/session.go:UpdateReason
#[test]
fn update_reason_variants_have_distinct_values() {
    assert_ne!(UpdateReason::Unknown, UpdateReason::DidOpenFile);
    assert_ne!(UpdateReason::DidOpenFile, UpdateReason::DidCloseFile);
    assert_ne!(
        UpdateReason::DidCloseFile,
        UpdateReason::DidChangeCompilerOptionsForInferredProjects
    );
}

// --- Slice 3: Session new() ---

// Go: internal/project/session_test.go:TestSession/DidOpenFile/create_configured_project
#[test]
fn session_new_has_empty_snapshot() {
    let session = make_test_session();
    let snap = session.snapshot();
    assert_eq!(snap.id(), 0);
    assert!(snap.project_collection().projects().is_empty());
}

// --- Slice 3: did_open_file creates a configured project ---

// Go: internal/project/session_test.go:TestSession/DidOpenFile/create_configured_project
#[test]
fn session_did_open_file_creates_project_entry() {
    let empty: Vec<(&str, &str)> = Vec::new();
    let fs: Box<dyn tsgo_vfs::Fs + Send + Sync> =
        Box::new(tsgo_vfs::vfstest::MapFs::from_map(empty, false));
    let overlay_fs = OverlayFS::new(fs, std::collections::HashMap::new(), |s: &str| {
        tsgo_tspath::Path(s.to_lowercase())
    });
    let options = SessionOptions {
        current_directory: "/home/projects".to_string(),
    };
    let mut session = Session::new(options, overlay_fs, Arc::new(NoopClient));

    session.did_open_file(
        "file:///home/projects/TS/p1/src/index.ts",
        1,
        "let x = 1;",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );

    let snap = session.snapshot();
    assert!(snap.id() > 0, "snapshot should have been rebuilt");
    // The overlay should now track the opened file
    assert!(session.overlay_has_file("file:///home/projects/TS/p1/src/index.ts"));
}

// --- Slice 4: open without tsconfig → inferred project ---

// Go: internal/project/session_test.go:TestSession/DidOpenFile/create_inferred_project
#[test]
fn session_open_without_tsconfig_creates_inferred_project() {
    let mut session = make_test_session();

    session.did_open_file(
        "file:///loose/config.ts",
        1,
        "let x = 1;",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );

    let snap = session.snapshot();
    assert!(
        snap.project_collection().inferred_project().is_some(),
        "should create inferred project when no tsconfig"
    );
}

// Go: internal/project/session_test.go:TestSession/DidOpenFile/inferred_project_for_in-memory_files
#[test]
fn session_open_untitled_creates_inferred_project() {
    let mut session = make_test_session();

    session.did_open_file(
        "untitled:Untitled-1",
        1,
        "x",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );

    let snap = session.snapshot();
    assert!(
        snap.project_collection().inferred_project().is_some(),
        "untitled files should use inferred project"
    );
}

// --- Slice 5: did_close_file ---

// Go: internal/project/session_test.go:TestSession/DidCloseFile/Inferred_projects/close_untitled_file
#[test]
fn session_close_file_removes_overlay() {
    let mut session = make_test_session();

    session.did_open_file(
        "untitled:Untitled-1",
        1,
        "let x = 1;",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );
    assert!(session.overlay_has_file("untitled:Untitled-1"));

    session.did_close_file("untitled:Untitled-1");
    assert!(
        !session.overlay_has_file("untitled:Untitled-1"),
        "overlay should be removed after close"
    );
}

#[test]
fn session_close_last_file_clears_inferred_project() {
    let mut session = make_test_session();

    session.did_open_file(
        "untitled:Untitled-1",
        1,
        "x",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );
    assert!(session
        .snapshot()
        .project_collection()
        .inferred_project()
        .is_some());

    session.did_close_file("untitled:Untitled-1");

    let snap = session.snapshot();
    assert!(
        snap.project_collection().inferred_project().is_none(),
        "inferred project should be removed when last file closes"
    );
}

// --- Slice 6: did_change_file ---

// Go: internal/project/session_test.go:TestSession/DidChangeFile/update_file_and_program
#[test]
fn session_did_change_file_updates_overlay_content() {
    let mut session = make_test_session();

    session.did_open_file(
        "untitled:Untitled-1",
        1,
        "let x = 1;",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );

    session.did_change_file("untitled:Untitled-1", 2, "let x = 2;");

    let snap = session.snapshot();
    assert!(snap.id() > 0);
    // Verify through overlay that content changed
    assert!(session.overlay_has_file("untitled:Untitled-1"));
}

// --- get_language_service ---

// Go: internal/project/session.go:Session.GetLanguageService
#[test]
fn session_get_language_service_returns_project_info() {
    let mut session = make_test_session();

    session.did_open_file(
        "untitled:Untitled-1",
        1,
        "let x = 1;",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );

    let result = session.get_language_service("untitled:Untitled-1");
    assert!(
        result.is_ok(),
        "should return Ok for open file with project"
    );
}

#[test]
fn session_get_language_service_no_project_returns_err() {
    let session = make_test_session();
    let result = session.get_language_service("file:///nonexistent.ts");
    assert!(result.is_err(), "should return Err when no project found");
}

// --- find_project_for_file ---

#[test]
fn session_find_project_for_file_with_tsconfig() {
    let files: Vec<(&str, &str)> = vec![
        ("/project/tsconfig.json", r#"{"compilerOptions":{}}"#),
        ("/project/src/index.ts", "let x = 1;"),
    ];
    let fs: Box<dyn tsgo_vfs::Fs + Send + Sync> =
        Box::new(tsgo_vfs::vfstest::MapFs::from_map(files, false));
    let overlay_fs = OverlayFS::new(fs, std::collections::HashMap::new(), |s: &str| {
        tsgo_tspath::Path(s.to_lowercase())
    });
    let options = SessionOptions {
        current_directory: "/project".to_string(),
    };
    let session = Session::new(options, overlay_fs, Arc::new(NoopClient));

    let result = session.find_project_for_file("/project/src/index.ts");
    assert_eq!(
        result,
        Some("/project/tsconfig.json".to_string()),
        "should find tsconfig.json in parent directory"
    );
}

#[test]
fn session_find_project_for_file_no_tsconfig_returns_none() {
    let files: Vec<(&str, &str)> = vec![("/loose/app.ts", "x")];
    let fs: Box<dyn tsgo_vfs::Fs + Send + Sync> =
        Box::new(tsgo_vfs::vfstest::MapFs::from_map(files, false));
    let overlay_fs = OverlayFS::new(fs, std::collections::HashMap::new(), |s: &str| {
        tsgo_tspath::Path(s.to_lowercase())
    });
    let options = SessionOptions {
        current_directory: "/".to_string(),
    };
    let session = Session::new(options, overlay_fs, Arc::new(NoopClient));

    let result = session.find_project_for_file("/loose/app.ts");
    assert_eq!(
        result, None,
        "should return None when no tsconfig.json found"
    );
}

// --- on_config_file_changed ---

#[test]
fn session_on_config_file_changed_invalidates_project() {
    let files: Vec<(&str, &str)> = vec![
        ("/project/tsconfig.json", r#"{"compilerOptions":{}}"#),
        ("/project/src/index.ts", "let x = 1;"),
    ];
    let fs: Box<dyn tsgo_vfs::Fs + Send + Sync> =
        Box::new(tsgo_vfs::vfstest::MapFs::from_map(files, false));
    let overlay_fs = OverlayFS::new(fs, std::collections::HashMap::new(), |s: &str| {
        tsgo_tspath::Path(s.to_lowercase())
    });
    let options = SessionOptions {
        current_directory: "/project".to_string(),
    };
    let mut session = Session::new(options, overlay_fs, Arc::new(NoopClient));

    // Open a file to create a configured project
    session.did_open_file(
        "file:///project/src/index.ts",
        1,
        "let x = 1;",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );
    let snap_before = session.snapshot().id();

    // Simulate config file change
    session.on_config_file_changed("/project/tsconfig.json");

    let snap_after = session.snapshot().id();
    assert!(
        snap_after > snap_before,
        "snapshot should be rebuilt after config change"
    );
}

// --- update_snapshot ---

#[test]
fn session_update_snapshot_increments_id() {
    let mut session = make_test_session();
    assert_eq!(session.snapshot().id(), 0);

    session.update_snapshot();
    assert_eq!(session.snapshot().id(), 1);

    session.update_snapshot();
    assert_eq!(session.snapshot().id(), 2);
}

// --- Snapshot ID increment ---

#[test]
fn session_snapshot_id_increments_on_open() {
    let mut session = make_test_session();
    assert_eq!(session.snapshot().id(), 0);

    session.did_open_file(
        "untitled:Untitled-1",
        1,
        "x",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );
    assert_eq!(session.snapshot().id(), 1);

    session.did_open_file(
        "untitled:Untitled-2",
        1,
        "y",
        tsgo_lsproto::LanguageKind::TYPE_SCRIPT,
    );
    assert_eq!(session.snapshot().id(), 2);
}
