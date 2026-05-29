use super::*;

use tsgo_tspath::get_base_file_name;
use tsgo_vfs::{FileInfo, Fs, FsResult, WalkControl};

// Go: internal/bundled/bundled_test.go:TestEmbeddedLibs
#[test]
fn test_embedded_libs() {
    let fs = wrap_fs(tsgo_vfs::osvfs::fs());

    let mut files: Vec<String> = Vec::new();
    {
        let mut collect = |path: &str, entry: &FileInfo| -> FsResult<WalkControl> {
            if !entry.is_dir() {
                files.push(get_base_file_name(path));
            }
            Ok(WalkControl::Continue)
        };
        fs.walk_dir(&lib_path(), &mut collect).unwrap();
    }

    let expected: Vec<String> = LIB_NAMES.iter().map(|name| name.to_string()).collect();
    assert_eq!(files, expected);
}
