use super::*;
use crate::vfsmock::FsMock;
use crate::vfstest::MapFs;
use crate::WalkControl;
use std::sync::Arc;

fn create_mock_fs() -> Arc<FsMock> {
    Arc::new(FsMock::wrap(MapFs::from_map(
        [("/some/path/file.txt", "hello world")],
        true,
    )))
}

fn cached(underlying: &Arc<FsMock>) -> CachedFs {
    CachedFs::from(underlying.clone() as Arc<dyn Fs + Send + Sync>)
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestDirectoryExists
#[test]
fn cached_directory_exists() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    cached.directory_exists("/some/path");
    assert_eq!(underlying.directory_exists_call_count(), 1);
    cached.directory_exists("/some/path");
    assert_eq!(underlying.directory_exists_call_count(), 1);
    cached.clear_cache();
    cached.directory_exists("/some/path");
    assert_eq!(underlying.directory_exists_call_count(), 2);
    cached.directory_exists("/other/path");
    assert_eq!(underlying.directory_exists_call_count(), 3);
    cached.disable_and_clear_cache();
    cached.directory_exists("/some/path");
    assert_eq!(underlying.directory_exists_call_count(), 4);
    cached.directory_exists("/some/path");
    assert_eq!(underlying.directory_exists_call_count(), 5);
    cached.enable();
    cached.directory_exists("/some/path");
    assert_eq!(underlying.directory_exists_call_count(), 6);
    cached.directory_exists("/some/path");
    assert_eq!(underlying.directory_exists_call_count(), 6);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestFileExists
#[test]
fn cached_file_exists() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    cached.file_exists("/some/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 1);
    cached.file_exists("/some/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 1);
    cached.clear_cache();
    cached.file_exists("/some/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 2);
    cached.file_exists("/other/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 3);
    cached.disable_and_clear_cache();
    cached.file_exists("/some/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 4);
    cached.file_exists("/some/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 5);
    cached.enable();
    cached.file_exists("/some/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 6);
    cached.file_exists("/some/path/file.txt");
    assert_eq!(underlying.file_exists_call_count(), 6);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestGetAccessibleEntries
#[test]
fn cached_get_accessible_entries() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    cached.get_accessible_entries("/some/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 1);
    cached.get_accessible_entries("/some/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 1);
    cached.clear_cache();
    cached.get_accessible_entries("/some/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 2);
    cached.get_accessible_entries("/other/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 3);
    cached.disable_and_clear_cache();
    cached.get_accessible_entries("/some/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 4);
    cached.get_accessible_entries("/some/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 5);
    cached.enable();
    cached.get_accessible_entries("/some/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 6);
    cached.get_accessible_entries("/some/path");
    assert_eq!(underlying.get_accessible_entries_call_count(), 6);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestRealpath
#[test]
fn cached_realpath() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    cached.realpath("/some/path");
    assert_eq!(underlying.realpath_call_count(), 1);
    cached.realpath("/some/path");
    assert_eq!(underlying.realpath_call_count(), 1);
    cached.clear_cache();
    cached.realpath("/some/path");
    assert_eq!(underlying.realpath_call_count(), 2);
    cached.realpath("/other/path");
    assert_eq!(underlying.realpath_call_count(), 3);
    cached.disable_and_clear_cache();
    cached.realpath("/some/path");
    assert_eq!(underlying.realpath_call_count(), 4);
    cached.realpath("/some/path");
    assert_eq!(underlying.realpath_call_count(), 5);
    cached.enable();
    cached.realpath("/some/path");
    assert_eq!(underlying.realpath_call_count(), 6);
    cached.realpath("/some/path");
    assert_eq!(underlying.realpath_call_count(), 6);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestStat
#[test]
fn cached_stat() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    cached.stat("/some/path");
    assert_eq!(underlying.stat_call_count(), 1);
    cached.stat("/some/path");
    assert_eq!(underlying.stat_call_count(), 1);
    cached.clear_cache();
    cached.stat("/some/path");
    assert_eq!(underlying.stat_call_count(), 2);
    cached.stat("/other/path");
    assert_eq!(underlying.stat_call_count(), 3);
    cached.disable_and_clear_cache();
    cached.stat("/some/path");
    assert_eq!(underlying.stat_call_count(), 4);
    cached.stat("/some/path");
    assert_eq!(underlying.stat_call_count(), 5);
    cached.enable();
    cached.stat("/some/path");
    assert_eq!(underlying.stat_call_count(), 6);
    cached.stat("/some/path");
    assert_eq!(underlying.stat_call_count(), 6);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestReadFile
#[test]
fn cached_read_file_not_cached() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    cached.read_file("/some/path/file.txt");
    assert_eq!(underlying.read_file_call_count(), 1);
    cached.read_file("/some/path/file.txt");
    assert_eq!(underlying.read_file_call_count(), 2);
    cached.clear_cache();
    cached.read_file("/some/path/file.txt");
    assert_eq!(underlying.read_file_call_count(), 3);
    cached.disable_and_clear_cache();
    cached.read_file("/some/path/file.txt");
    assert_eq!(underlying.read_file_call_count(), 4);
    cached.read_file("/some/path/file.txt");
    assert_eq!(underlying.read_file_call_count(), 5);
    cached.enable();
    cached.read_file("/some/path/file.txt");
    assert_eq!(underlying.read_file_call_count(), 6);
    cached.read_file("/some/path/file.txt");
    assert_eq!(underlying.read_file_call_count(), 7);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestUseCaseSensitiveFileNames
#[test]
fn cached_use_case_sensitive_not_cached() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    cached.use_case_sensitive_file_names();
    assert_eq!(underlying.use_case_sensitive_file_names_call_count(), 1);
    cached.use_case_sensitive_file_names();
    assert_eq!(underlying.use_case_sensitive_file_names_call_count(), 2);
    cached.clear_cache();
    cached.use_case_sensitive_file_names();
    assert_eq!(underlying.use_case_sensitive_file_names_call_count(), 3);
    cached.disable_and_clear_cache();
    cached.use_case_sensitive_file_names();
    assert_eq!(underlying.use_case_sensitive_file_names_call_count(), 4);
    cached.use_case_sensitive_file_names();
    assert_eq!(underlying.use_case_sensitive_file_names_call_count(), 5);
    cached.enable();
    cached.use_case_sensitive_file_names();
    assert_eq!(underlying.use_case_sensitive_file_names_call_count(), 6);
    cached.use_case_sensitive_file_names();
    assert_eq!(underlying.use_case_sensitive_file_names_call_count(), 7);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestWalkDir
#[test]
fn cached_walk_dir_not_cached() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);
    let walk = |c: &CachedFs| {
        let _ = c.walk_dir("/some/path", &mut |_p, _i| Ok(WalkControl::Continue));
    };

    walk(&cached);
    assert_eq!(underlying.walk_dir_call_count(), 1);
    walk(&cached);
    assert_eq!(underlying.walk_dir_call_count(), 2);
    cached.clear_cache();
    walk(&cached);
    assert_eq!(underlying.walk_dir_call_count(), 3);
    cached.disable_and_clear_cache();
    walk(&cached);
    assert_eq!(underlying.walk_dir_call_count(), 4);
    walk(&cached);
    assert_eq!(underlying.walk_dir_call_count(), 5);
    cached.enable();
    walk(&cached);
    assert_eq!(underlying.walk_dir_call_count(), 6);
    walk(&cached);
    assert_eq!(underlying.walk_dir_call_count(), 7);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestRemove
#[test]
fn cached_remove_not_cached() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    let _ = cached.remove("/some/path/file.txt");
    assert_eq!(underlying.remove_call_count(), 1);
    let _ = cached.remove("/some/path/file.txt");
    assert_eq!(underlying.remove_call_count(), 2);
    cached.clear_cache();
    let _ = cached.remove("/some/path/file.txt");
    assert_eq!(underlying.remove_call_count(), 3);
    cached.disable_and_clear_cache();
    let _ = cached.remove("/some/path/file.txt");
    assert_eq!(underlying.remove_call_count(), 4);
    let _ = cached.remove("/some/path/file.txt");
    assert_eq!(underlying.remove_call_count(), 5);
    cached.enable();
    let _ = cached.remove("/some/path/file.txt");
    assert_eq!(underlying.remove_call_count(), 6);
    let _ = cached.remove("/some/path/file.txt");
    assert_eq!(underlying.remove_call_count(), 7);
}

// Go: internal/vfs/cachedvfs/cachedvfs_test.go:TestWriteFile
#[test]
fn cached_write_file_not_cached() {
    let underlying = create_mock_fs();
    let cached = cached(&underlying);

    let _ = cached.write_file("/some/path/file.txt", "new content");
    assert_eq!(underlying.write_file_calls().len(), 1);
    let _ = cached.write_file("/some/path/file.txt", "another content");
    assert_eq!(underlying.write_file_calls().len(), 2);
    cached.clear_cache();
    let _ = cached.write_file("/some/path/file.txt", "third content");
    assert_eq!(underlying.write_file_calls().len(), 3);

    let call = underlying.write_file_calls()[2].clone();
    assert_eq!(call.path, "/some/path/file.txt");
    assert_eq!(call.data, "third content");

    cached.disable_and_clear_cache();
    let _ = cached.write_file("/some/path/file.txt", "fourth content");
    assert_eq!(underlying.write_file_calls().len(), 4);
    let _ = cached.write_file("/some/path/file.txt", "fifth content");
    assert_eq!(underlying.write_file_calls().len(), 5);
    cached.enable();
    let _ = cached.write_file("/some/path/file.txt", "sixth content");
    assert_eq!(underlying.write_file_calls().len(), 6);
    let _ = cached.write_file("/some/path/file.txt", "seventh content");
    assert_eq!(underlying.write_file_calls().len(), 7);
}
