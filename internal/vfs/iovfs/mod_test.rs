use super::*;
use crate::{Fs, WalkControl};

fn make_fs() -> MapFs {
    from(
        [
            ("foo.ts", "hello, world"),
            ("dir1/file1.ts", "export const foo = 42;"),
            ("dir1/file2.ts", "export const foo = 42;"),
            ("dir2/file1.ts", "export const foo = 42;"),
        ],
        true,
    )
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/ReadFile
#[test]
fn iofs_read_file() {
    let fs = make_fs();
    assert_eq!(fs.read_file("/foo.ts").as_deref(), Some("hello, world"));
    assert_eq!(fs.read_file("/does/not/exist.ts"), None);
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/ReadFileUnrooted
#[test]
#[should_panic(expected = "vfs: path \"bar\" is not absolute")]
fn iofs_read_file_unrooted_panics() {
    let fs = make_fs();
    let _ = fs.read_file("bar");
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/FileExists
#[test]
fn iofs_file_exists() {
    let fs = make_fs();
    assert!(fs.file_exists("/foo.ts"));
    assert!(!fs.file_exists("/bar"));
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/DirectoryExists
#[test]
fn iofs_directory_exists() {
    let fs = make_fs();
    assert!(fs.directory_exists("/"));
    assert!(fs.directory_exists("/dir1"));
    assert!(fs.directory_exists("/dir1/"));
    assert!(fs.directory_exists("/dir1/./"));
    assert!(!fs.directory_exists("/bar"));
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/GetAccessibleEntries
#[test]
fn iofs_get_accessible_entries() {
    let fs = make_fs();
    let entries = fs.get_accessible_entries("/");
    assert_eq!(entries.directories, ["dir1", "dir2"]);
    assert_eq!(entries.files, ["foo.ts"]);
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/WalkDir
#[test]
fn iofs_walk_dir() {
    let fs = make_fs();
    let mut files = Vec::new();
    fs.walk_dir("/", &mut |path, info| {
        if !info.is_dir() {
            files.push(path.to_string());
        }
        Ok(WalkControl::Continue)
    })
    .unwrap();
    files.sort();
    assert_eq!(
        files,
        [
            "/dir1/file1.ts",
            "/dir1/file2.ts",
            "/dir2/file1.ts",
            "/foo.ts"
        ]
    );
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/WalkDirSkip
#[test]
fn iofs_walk_dir_skip() {
    let fs = make_fs();
    let mut files = Vec::new();
    fs.walk_dir("/", &mut |path, info| {
        if !info.is_dir() {
            files.push(path.to_string());
        }
        if path == "/" {
            Ok(WalkControl::Continue)
        } else {
            Ok(WalkControl::SkipDir)
        }
    })
    .unwrap();
    files.sort();
    assert_eq!(files, ["/foo.ts"]);
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/Realpath
#[test]
fn iofs_realpath() {
    let fs = make_fs();
    assert_eq!(fs.realpath("/foo.ts"), "/foo.ts");
}

// Go: internal/vfs/iovfs/iofs_test.go:TestIOFS/UseCaseSensitiveFileNames
#[test]
fn iofs_use_case_sensitive() {
    let fs = make_fs();
    assert!(fs.use_case_sensitive_file_names());
}
