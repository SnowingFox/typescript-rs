use super::*;
use crate::Fs;

// Go: internal/vfs/osvfs/os_test.go:TestOS/ReadFile
#[test]
fn os_read_file() {
    let path = normalize_path(&format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")));
    let via_fs = fs().read_file(&path).expect("osvfs should read Cargo.toml");
    let via_std = std::fs::read_to_string(&path).expect("std should read Cargo.toml");
    assert_eq!(via_fs, via_std);
}

// Go: internal/vfs/osvfs/os_test.go:TestOS/Realpath
#[test]
fn os_realpath() {
    let Ok(home) = std::env::var("HOME") else {
        return; // matches Go's t.Skip when the home directory is unavailable
    };
    let home = normalize_path(&home);
    let mut expected = home.clone();
    if cfg!(windows) {
        let mut chars: Vec<char> = expected.chars().collect();
        if let Some(first) = chars.first_mut() {
            *first = first.to_ascii_uppercase();
        }
        expected = chars.into_iter().collect();
    }
    assert_eq!(fs().realpath(&home), expected);
}

// Go: internal/vfs/osvfs/os_test.go:TestOS/UseCaseSensitiveFileNames
#[test]
fn os_use_case_sensitive() {
    let _ = fs().use_case_sensitive_file_names();
    #[cfg(target_os = "linux")]
    assert!(fs().use_case_sensitive_file_names());
    #[cfg(windows)]
    assert!(!fs().use_case_sensitive_file_names());
}

// Go: internal/vfs/osvfs/realpath_test.go:TestSymlinkRealpath
#[cfg(unix)]
#[test]
fn os_symlink_realpath() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let target_file = target.join("file");
    let link = tmp.path().join("link");
    let link_file = link.join("file");

    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(&target_file, b"hello").unwrap();
    symlink(&target, &link).unwrap();

    let f = fs();
    let target_realpath = f.realpath(&normalize_path(&target_file.to_string_lossy()));
    let link_realpath = f.realpath(&normalize_path(&link_file.to_string_lossy()));
    assert_eq!(target_realpath, link_realpath);
}

// Go: internal/vfs/osvfs/realpath_test.go:TestGetAccessibleEntries
#[cfg(unix)]
#[test]
fn os_get_accessible_entries_symlinks() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let link = tmp.path().join("link");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&link).unwrap();

    let target_file1 = target.join("file1");
    let target_file2 = target.join("file2");
    std::fs::write(&target_file1, b"hello").unwrap();
    std::fs::write(&target_file2, b"world").unwrap();
    let target_dir1 = target.join("dir1");
    let target_dir2 = target.join("dir2");
    std::fs::create_dir_all(&target_dir1).unwrap();
    std::fs::create_dir_all(&target_dir2).unwrap();

    symlink(&target_file1, link.join("file1")).unwrap();
    symlink(&target_file2, link.join("file2")).unwrap();
    symlink(&target_dir1, link.join("dir1")).unwrap();
    symlink(&target_dir2, link.join("dir2")).unwrap();

    let f = fs();
    let entries = f.get_accessible_entries(&normalize_path(&link.to_string_lossy()));
    assert_eq!(entries.directories, ["dir1", "dir2"]);
    assert_eq!(entries.files, ["file1", "file2"]);
    let symlinks = entries.symlinks.as_ref().expect("symlinks set present");
    assert_eq!(symlinks.len(), 4);
    for name in ["file1", "file2", "dir1", "dir2"] {
        assert!(symlinks.contains(name), "expected {name:?} in symlinks");
    }

    // A non-symlink directory should have an empty (but present) symlink set.
    let entries = f.get_accessible_entries(&normalize_path(&target.to_string_lossy()));
    assert_eq!(entries.directories, ["dir1", "dir2"]);
    assert_eq!(entries.files, ["file1", "file2"]);
    assert_eq!(entries.symlinks.as_ref().unwrap().len(), 0);
}
