use super::*;
use crate::vfstest::MapFs;

// Go: internal/vfs/wrapvfs/wrapvfs.go (behavior-level; no direct Go unit test)
#[test]
fn wrapvfs_replacement_used() {
    let replacements = Replacements {
        file_exists: Some(Box::new(|_p| true)),
        ..Default::default()
    };
    let fs = wrap(
        MapFs::from_map(Vec::<(&str, &str)>::new(), true),
        replacements,
    );
    // Replacement forces true even though the file does not exist.
    assert!(fs.file_exists("/does/not/exist.ts"));
}

// Go: internal/vfs/wrapvfs/wrapvfs.go (behavior-level; delegation path)
#[test]
fn wrapvfs_delegates_when_no_replacement() {
    let fs = wrap(
        MapFs::from_map([("/a.ts", "hello")], true),
        Replacements::default(),
    );
    assert!(fs.file_exists("/a.ts"));
    assert!(!fs.file_exists("/missing.ts"));
    assert_eq!(fs.read_file("/a.ts").as_deref(), Some("hello"));
}
