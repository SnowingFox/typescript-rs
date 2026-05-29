use super::*;

// Go: internal/vfs/vfs.go:ErrNotExist
#[test]
fn fs_error_not_exist_message() {
    assert_eq!(FsError::NotExist.to_string(), "file does not exist");
    assert!(FsError::NotExist.is_not_exist());
    assert!(!FsError::Other("boom".into()).is_not_exist());
}

// Go: internal/vfs/vfstest/vfstest.go:brokenSymlinkError.Error
#[test]
fn fs_error_broken_symlink_message() {
    let e = FsError::BrokenSymlink {
        from: "brokenlink".into(),
        to: "does/not/exist".into(),
    };
    assert_eq!(
        e.to_string(),
        r#"broken symlink "brokenlink" -> "does/not/exist""#
    );
}

// Go: internal/vfs/vfs.go:FileInfo (fs.FileMode subset)
#[test]
fn file_mode_classification() {
    assert!(FileMode::DIR.is_dir());
    assert!(!FileMode::DIR.is_regular());
    assert!(FileMode::REGULAR.is_regular());
    assert!(!FileMode::REGULAR.is_dir());
    assert!(FileMode::SYMLINK.is_symlink());
    assert!(!FileMode::SYMLINK.is_regular());
}

// Go: internal/vfs/vfs.go:Entries
#[test]
fn entries_default_is_empty() {
    let e = Entries::default();
    assert!(e.files.is_empty());
    assert!(e.directories.is_empty());
    assert!(e.symlinks.is_none());
}
