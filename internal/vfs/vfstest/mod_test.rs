use super::*;
use std::sync::Arc;

fn clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock::new())
}

fn file_seed(path: &str, data: &str) -> (String, Vec<u8>, FileMode) {
    (path.into(), data.as_bytes().to_vec(), FileMode::REGULAR)
}

fn names(infos: &[FileInfo]) -> Vec<String> {
    infos.iter().map(|i| i.name().to_string()).collect()
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestInsensitive
#[test]
fn vfstest_insensitive() {
    let fs = MapFs::convert_map_fs(
        vec![
            file_seed("foo/bar/baz", "bar"),
            file_seed("foo/bar2/baz2", "bar"),
            file_seed("foo/bar3/baz3", "bar"),
        ],
        false,
        clock(),
    );

    assert_eq!(
        fs.read_bytes_rel("foo/bar/baz").as_deref(),
        Some(&b"bar"[..])
    );
    assert!(!fs.stat_rel("foo/bar/baz").unwrap().is_dir());
    assert_eq!(fs.realpath_rel("foo/bar/baz").unwrap(), "foo/bar/baz");
    assert_eq!(
        names(&fs.read_dir_rel("foo").unwrap()),
        ["bar", "bar2", "bar3"]
    );

    assert!(fs.realpath_rel("does/not/exist").is_err());
    assert!(fs.stat_rel("does/not/exist").is_none());

    // Case-insensitive access via differently-cased path.
    assert_eq!(
        fs.read_bytes_rel("Foo/Bar/Baz").as_deref(),
        Some(&b"bar"[..])
    );
    assert_eq!(fs.realpath_rel("Foo/Bar/Baz").unwrap(), "foo/bar/baz");
    assert_eq!(
        names(&fs.read_dir_rel("Foo").unwrap()),
        ["bar", "bar2", "bar3"]
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestInsensitiveUpper
#[test]
fn vfstest_insensitive_upper() {
    let fs = MapFs::convert_map_fs(
        vec![
            file_seed("Foo/Bar/Baz", "bar"),
            file_seed("Foo/Bar2/Baz2", "bar"),
            file_seed("Foo/Bar3/Baz3", "bar"),
        ],
        false,
        clock(),
    );

    assert_eq!(
        fs.read_bytes_rel("foo/bar/baz").as_deref(),
        Some(&b"bar"[..])
    );
    assert_eq!(
        names(&fs.read_dir_rel("foo").unwrap()),
        ["Bar", "Bar2", "Bar3"]
    );

    assert_eq!(
        fs.read_bytes_rel("Foo/Bar/Baz").as_deref(),
        Some(&b"bar"[..])
    );
    assert_eq!(
        names(&fs.read_dir_rel("Foo").unwrap()),
        ["Bar", "Bar2", "Bar3"]
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestSensitive
#[test]
fn vfstest_sensitive() {
    let fs = MapFs::convert_map_fs(vec![file_seed("foo/bar/baz", "bar")], true, clock());
    assert_eq!(
        fs.read_bytes_rel("foo/bar/baz").as_deref(),
        Some(&b"bar"[..])
    );
    assert!(fs.read_bytes_rel("Foo/Bar/Baz").is_none());
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestSensitiveDuplicatePath
#[test]
#[should_panic(expected = r#"duplicate path: "Foo" and "foo" have the same canonical path"#)]
fn vfstest_sensitive_duplicate_path_panics() {
    let _ = MapFs::convert_map_fs(
        vec![file_seed("foo", "bar"), file_seed("Foo", "baz")],
        false,
        clock(),
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestInsensitiveDuplicatePath
#[test]
fn vfstest_insensitive_duplicate_path_ok() {
    let _ = MapFs::convert_map_fs(
        vec![file_seed("foo", "bar"), file_seed("Foo", "baz")],
        true,
        clock(),
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestWritableFS
#[test]
fn vfstest_writable() {
    let fs = MapFs::from_map(Vec::<(&str, &str)>::new(), false);
    fs.write_file("/foo/bar/baz", "hello, world").unwrap();
    assert_eq!(
        fs.read_file("/foo/bar/baz").as_deref(),
        Some("hello, world")
    );

    fs.write_file("/foo/bar/baz", "goodbye, world").unwrap();
    assert_eq!(
        fs.read_file("/foo/bar/baz").as_deref(),
        Some("goodbye, world")
    );

    let err = fs
        .write_file("/foo/bar/baz/oops", "goodbye, world")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains(r#"mkdir "foo/bar/baz": path exists but is not a directory"#),
        "unexpected error: {err}"
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestWritableFSDelete
#[test]
fn vfstest_writable_delete() {
    let fs = MapFs::from_map(Vec::<(&str, &str)>::new(), false);

    fs.write_file("/foo/bar/file.ts", "remove").unwrap();
    assert!(fs.file_exists("/foo/bar/file.ts"));
    fs.remove("/foo/bar/file.ts").unwrap();
    assert!(!fs.file_exists("/foo/bar/file.ts"));

    fs.write_file("/foo/bar/test/remove2.ts", "remove2")
        .unwrap();
    assert!(fs.directory_exists("/foo/bar/test"));
    fs.remove("/foo/bar/test").unwrap();
    assert!(!fs.file_exists("/foo/bar/test/remove2.ts"));
    assert!(!fs.directory_exists("/foo/bar/test"));

    // Removing nonexistent paths is not an error.
    fs.remove("/foo/bar/test").unwrap();
    fs.remove("/foo/bar/file.ts").unwrap();

    fs.write_file("/foo/barbar", "remove2").unwrap();
    fs.remove("/foo/bar").unwrap();
    assert!(fs.file_exists("/foo/barbar"));
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestParentDirFile
#[test]
#[should_panic(
    expected = r#"failed to create intermediate directories for "foo/oops": mkdir "foo": path exists but is not a directory"#
)]
fn vfstest_parent_dir_file_panics() {
    let _ = MapFs::convert_map_fs(
        vec![file_seed("foo", "bar"), file_seed("foo/oops", "baz")],
        false,
        clock(),
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestStress
#[test]
fn vfstest_stress() {
    let fs = Arc::new(MapFs::from_map(Vec::<(&str, &str)>::new(), false));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let fs = Arc::clone(&fs);
        handles.push(std::thread::spawn(move || {
            for i in 0..2000 {
                match i % 7 {
                    0 => {
                        let _ = fs.write_file("/foo/bar/baz.txt", "hello, world");
                    }
                    1 => {
                        let _ = fs.read_file("/foo/bar/baz.txt");
                    }
                    2 => {
                        let _ = fs.directory_exists("/foo/bar");
                    }
                    3 => {
                        let _ = fs.file_exists("/foo/bar/baz.txt");
                    }
                    4 => {
                        let _ = fs.get_accessible_entries("/foo/bar");
                    }
                    5 => {
                        let _ = fs.realpath("/foo/bar/baz.txt");
                    }
                    _ => {
                        let _ = fs.walk_dir("/", &mut |_p, _i| Ok(WalkControl::Continue));
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestFromMap/POSIX
#[test]
fn vfstest_from_map_posix() {
    let fs = MapFs::from_map(
        [
            ("/string", MapFile::text("hello, world")),
            ("/bytes", MapFile::bytes(b"hello, world".to_vec())),
            ("/mapfile", MapFile::bytes(b"hello, world".to_vec())),
        ],
        false,
    );
    assert_eq!(fs.read_file("/string").as_deref(), Some("hello, world"));
    assert_eq!(fs.read_file("/bytes").as_deref(), Some("hello, world"));
    assert_eq!(fs.read_file("/mapfile").as_deref(), Some("hello, world"));
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestFromMap/Windows
#[test]
fn vfstest_from_map_windows() {
    let fs = MapFs::from_map(
        [
            ("c:/string", MapFile::text("hello, world")),
            ("d:/bytes", MapFile::bytes(b"hello, world".to_vec())),
            ("e:/mapfile", MapFile::bytes(b"hello, world".to_vec())),
        ],
        false,
    );
    assert_eq!(fs.read_file("c:/string").as_deref(), Some("hello, world"));
    assert_eq!(fs.read_file("d:/bytes").as_deref(), Some("hello, world"));
    assert_eq!(fs.read_file("e:/mapfile").as_deref(), Some("hello, world"));
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestFromMap/Mixed
#[test]
#[should_panic(expected = "mixed posix and windows paths")]
fn vfstest_from_map_mixed_panics() {
    let _ = MapFs::from_map(
        [
            ("/string", MapFile::text("hello, world")),
            ("c:/bytes", MapFile::bytes(b"hello, world".to_vec())),
        ],
        false,
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestFromMap/NonRooted
#[test]
#[should_panic(expected = r#"non-rooted path "string""#)]
fn vfstest_from_map_nonrooted_panics() {
    let _ = MapFs::from_map([("string", "hello, world")], false);
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestFromMap/NonNormalized
#[test]
#[should_panic(expected = r#"non-normalized path "/string/""#)]
fn vfstest_from_map_nonnormalized_panics() {
    let _ = MapFs::from_map([("/string/", "hello, world")], false);
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestFromMap/NonNormalized2
#[test]
#[should_panic(expected = r#"non-normalized path "/string/../foo""#)]
fn vfstest_from_map_nonnormalized2_panics() {
    let _ = MapFs::from_map([("/string/../foo", "hello, world")], false);
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestVFSTestMapFS
#[test]
fn vfstest_mapfs_read_realpath_case() {
    let fs = MapFs::from_map(
        [
            ("/foo.ts", "hello, world"),
            ("/dir1/file1.ts", "export const foo = 42;"),
            ("/dir1/file2.ts", "export const foo = 42;"),
            ("/dir2/file1.ts", "export const foo = 42;"),
        ],
        false,
    );
    assert_eq!(fs.read_file("/foo.ts").as_deref(), Some("hello, world"));
    assert_eq!(fs.read_file("/does/not/exist.ts"), None);

    assert_eq!(fs.realpath("/foo.ts"), "/foo.ts");
    assert_eq!(fs.realpath("/Foo.ts"), "/foo.ts");
    assert_eq!(fs.realpath("/does/not/exist.ts"), "/does/not/exist.ts");

    assert!(!fs.use_case_sensitive_file_names());
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestVFSTestMapFSWindows
#[test]
fn vfstest_mapfs_windows() {
    let fs = MapFs::from_map(
        [
            ("c:/foo.ts", "hello, world"),
            ("c:/dir1/file1.ts", "export const foo = 42;"),
        ],
        false,
    );
    assert_eq!(fs.read_file("c:/foo.ts").as_deref(), Some("hello, world"));
    assert_eq!(fs.read_file("c:/does/not/exist.ts"), None);
    assert_eq!(fs.realpath("c:/foo.ts"), "c:/foo.ts");
    assert_eq!(fs.realpath("c:/Foo.ts"), "c:/foo.ts");
    assert_eq!(fs.realpath("c:/does/not/exist.ts"), "c:/does/not/exist.ts");
}

fn utf16_buf(bom: [u8; 2], big_endian: bool) -> Vec<u8> {
    let mut buf = bom.to_vec();
    for u in "hello, world".encode_utf16() {
        if big_endian {
            buf.extend_from_slice(&u.to_be_bytes());
        } else {
            buf.extend_from_slice(&u.to_le_bytes());
        }
    }
    buf
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestBOM/BigEndian
#[test]
fn vfstest_bom_be() {
    let fs = MapFs::from_map(
        [("/foo.ts", MapFile::bytes(utf16_buf([0xFE, 0xFF], true)))],
        true,
    );
    assert_eq!(fs.read_file("/foo.ts").as_deref(), Some("hello, world"));
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestBOM/LittleEndian
#[test]
fn vfstest_bom_le() {
    let fs = MapFs::from_map(
        [("/foo.ts", MapFile::bytes(utf16_buf([0xFF, 0xFE], false)))],
        true,
    );
    assert_eq!(fs.read_file("/foo.ts").as_deref(), Some("hello, world"));
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestBOM/UTF8
#[test]
fn vfstest_bom_utf8() {
    let mut buf = vec![0xEF, 0xBB, 0xBF];
    buf.extend_from_slice(b"hello, world");
    let fs = MapFs::from_map([("/foo.ts", MapFile::bytes(buf))], true);
    assert_eq!(fs.read_file("/foo.ts").as_deref(), Some("hello, world"));
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestSymlink
#[test]
fn vfstest_symlink() {
    let fs = MapFs::from_map(
        [
            ("/foo.ts", MapFile::text("hello, world")),
            ("/symlink.ts", MapFile::symlink("/foo.ts")),
            ("/some/dir/file.ts", MapFile::text("hello, world")),
            ("/some/dirlink", MapFile::symlink("/some/dir")),
            ("/a", MapFile::symlink("/b")),
            ("/b", MapFile::symlink("/c")),
            ("/c", MapFile::symlink("/d")),
            ("/d/existing.ts", MapFile::text("this is existing.ts")),
        ],
        false,
    );

    // ReadFile
    assert_eq!(fs.read_file("/symlink.ts").as_deref(), Some("hello, world"));
    assert_eq!(
        fs.read_file("/some/dirlink/file.ts").as_deref(),
        Some("hello, world")
    );
    assert_eq!(
        fs.read_file("/a/existing.ts").as_deref(),
        Some("this is existing.ts")
    );

    // Realpath
    assert_eq!(fs.realpath("/symlink.ts"), "/foo.ts");
    assert_eq!(fs.realpath("/some/dirlink"), "/some/dir");
    assert_eq!(fs.realpath("/some/dirlink/file.ts"), "/some/dir/file.ts");

    // FileExists
    assert!(fs.file_exists("/symlink.ts"));
    assert!(fs.file_exists("/some/dirlink/file.ts"));
    assert!(fs.file_exists("/a/existing.ts"));

    // DirectoryExists
    assert!(fs.directory_exists("/some/dirlink"));
    assert!(fs.directory_exists("/d"));
    assert!(fs.directory_exists("/c"));
    assert!(fs.directory_exists("/b"));
    assert!(fs.directory_exists("/a"));
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestWritableFSSymlink
#[test]
fn vfstest_writable_symlink() {
    let fs = MapFs::from_map(
        [
            ("/some/dir/other.ts", MapFile::text("NOTHING")),
            ("/other.ts", MapFile::symlink("/some/dir/other.ts")),
            ("/some/dirlink", MapFile::symlink("/some/dir")),
            ("/brokenlink", MapFile::symlink("/does/not/exist")),
            ("/a", MapFile::symlink("/b")),
            ("/b", MapFile::symlink("/c")),
            ("/c", MapFile::symlink("/d")),
            ("/d/existing.ts", MapFile::text("hello, world")),
        ],
        false,
    );

    fs.write_file("/some/dirlink/file.ts", "hello, world")
        .unwrap();
    assert_eq!(
        fs.read_file("/some/dirlink/file.ts").as_deref(),
        Some("hello, world")
    );
    assert_eq!(
        fs.read_file("/some/dir/file.ts").as_deref(),
        Some("hello, world")
    );

    fs.write_file("/some/dirlink/file.ts", "goodbye, world")
        .unwrap();
    assert_eq!(
        fs.read_file("/some/dirlink/file.ts").as_deref(),
        Some("goodbye, world")
    );

    fs.write_file("/other.ts", "hello, world").unwrap();
    assert_eq!(fs.read_file("/other.ts").as_deref(), Some("hello, world"));
    assert_eq!(
        fs.read_file("/some/dir/other.ts").as_deref(),
        Some("hello, world")
    );

    let err = fs.write_file("/some/dirlink", "hello, world").unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"write "some/dirlink": path exists but is not a regular file"#
    );

    // Cannot write inside a broken dir symlink.
    let err = fs
        .write_file("/brokenlink/file.ts", "hello, world")
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"broken symlink "brokenlink" -> "does/not/exist""#
    );

    let err = fs
        .write_file("/brokenlink/also/wrong/file.ts", "hello, world")
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"broken symlink "brokenlink" -> "does/not/exist""#
    );

    // But we can write to a broken file symlink.
    fs.write_file("/brokenlink", "hello, world").unwrap();
    assert_eq!(fs.read_file("/brokenlink").as_deref(), Some("hello, world"));
    assert_eq!(
        fs.read_file("/does/not/exist").as_deref(),
        Some("hello, world")
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestWritableFSSymlinkChain
#[test]
fn vfstest_writable_symlink_chain() {
    let fs = MapFs::from_map(
        [
            ("/a", MapFile::symlink("/b")),
            ("/b", MapFile::symlink("/c")),
            ("/c", MapFile::symlink("/d")),
            ("/d/existing.ts", MapFile::text("hello, world")),
        ],
        false,
    );

    fs.write_file("/a/foo/bar/new.ts", "this is new.ts")
        .unwrap();
    assert_eq!(
        fs.read_file("/a/foo/bar/new.ts").as_deref(),
        Some("this is new.ts")
    );
    assert_eq!(
        fs.read_file("/b/foo/bar/new.ts").as_deref(),
        Some("this is new.ts")
    );
    assert_eq!(
        fs.read_file("/d/foo/bar/new.ts").as_deref(),
        Some("this is new.ts")
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestWritableFSSymlinkChainNotDir
#[test]
fn vfstest_writable_symlink_chain_not_dir() {
    let fs = MapFs::from_map(
        [
            ("/a", MapFile::symlink("/b")),
            ("/b", MapFile::symlink("/c")),
            ("/c", MapFile::symlink("/d")),
            ("/d", MapFile::text("hello, world")),
        ],
        false,
    );

    let err = fs
        .write_file("/a/foo/bar/new.ts", "this is new.ts")
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"mkdir "d": path exists but is not a directory"#
    );
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestWritableFSSymlinkDelete
#[test]
fn vfstest_writable_symlink_delete() {
    let fs = MapFs::from_map(
        [
            ("/some/dir/other.ts", MapFile::text("NOTHING")),
            ("/other.ts", MapFile::symlink("/some/dir/other.ts")),
            ("/some/dirlink", MapFile::symlink("/some/dir")),
            ("/brokenlink", MapFile::symlink("/does/not/exist")),
            ("/a", MapFile::symlink("/b")),
            ("/b", MapFile::symlink("/c")),
            ("/c", MapFile::symlink("/d")),
            ("/d/existing.ts", MapFile::text("hello, world")),
        ],
        false,
    );

    fs.remove("/a").unwrap();
    assert!(!fs.directory_exists("/a"));
    assert!(fs.directory_exists("/b"));
    assert!(fs.directory_exists("/c"));
    assert!(fs.file_exists("/d/existing.ts"));

    // Symlinks still exist even if the underlying file/dir is deleted.
    fs.remove("/d").unwrap();
    assert!(!fs.directory_exists("/b"));
    assert!(!fs.directory_exists("/c"));
    assert!(!fs.directory_exists("/d"));
    assert!(!fs.file_exists("/d/again.ts"));
    fs.write_file("/d/again.ts", "d exists again").unwrap();
    assert!(fs.directory_exists("/b"));
    assert!(fs.directory_exists("/c"));
    assert_eq!(
        fs.read_file("/b/again.ts").as_deref(),
        Some("d exists again")
    );

    assert!(!fs.file_exists("/brokenlink"));
    assert!(!fs.directory_exists("/brokenlink"));
    fs.remove("/does/not/exist").unwrap();
    assert!(!fs.file_exists("/brokenlink"));
    assert!(!fs.directory_exists("/brokenlink"));
    fs.write_file("/does/not/exist", "hello, world").unwrap();
    assert!(fs.file_exists("/brokenlink"));
}
