use super::*;
use crate::vfstest::MapFs;
use crate::WalkControl;
use std::time::SystemTime;

// Go: internal/vfs/vfsmock/wrapper_test.go:TestWrap
#[test]
fn vfsmock_wrap_all_fields_set() {
    // Each method must forward to the inner FS and be recorded. Exercising every
    // method is the Rust equivalent of Go's reflect-based "no field is zero".
    let mock = FsMock::wrap(MapFs::from_map(
        [("/some/path/file.txt", "hello world")],
        true,
    ));

    assert!(mock.use_case_sensitive_file_names());
    assert!(mock.file_exists("/some/path/file.txt"));
    assert_eq!(
        mock.read_file("/some/path/file.txt").as_deref(),
        Some("hello world")
    );
    assert!(mock.directory_exists("/some/path"));
    let _ = mock.get_accessible_entries("/some/path");
    let _ = mock.stat("/some/path");
    assert_eq!(mock.realpath("/some/path"), "/some/path");
    mock.write_file("/some/path/w.txt", "w").unwrap();
    mock.append_file("/some/path/a.txt", "a").unwrap();
    mock.remove("/some/path/a.txt").unwrap();
    mock.chtimes(
        "/some/path/w.txt",
        SystemTime::UNIX_EPOCH,
        SystemTime::UNIX_EPOCH,
    )
    .unwrap();
    mock.walk_dir("/some/path", &mut |_p, _i| Ok(WalkControl::Continue))
        .unwrap();

    assert_eq!(mock.use_case_sensitive_file_names_call_count(), 1);
    assert_eq!(mock.file_exists_call_count(), 1);
    assert_eq!(mock.read_file_call_count(), 1);
    assert_eq!(mock.directory_exists_call_count(), 1);
    assert_eq!(mock.get_accessible_entries_call_count(), 1);
    assert_eq!(mock.stat_call_count(), 1);
    assert_eq!(mock.realpath_call_count(), 1);
    assert_eq!(mock.write_file_calls().len(), 1);
    assert_eq!(mock.append_file_calls().len(), 1);
    assert_eq!(mock.remove_call_count(), 1);
    assert_eq!(mock.chtimes_call_count(), 1);
    assert_eq!(mock.walk_dir_call_count(), 1);
}
