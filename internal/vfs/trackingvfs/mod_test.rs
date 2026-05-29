use super::*;
use crate::vfstest::MapFs;
use std::time::SystemTime;

// Go: internal/vfs/trackingvfs/trackingvfs.go (behavior-level; no direct Go unit test)
#[test]
fn trackingvfs_records_reads() {
    let fs = TrackingFs::new(MapFs::from_map([("/some/file.ts", "x")], true));

    fs.read_file("/some/file.ts");
    fs.file_exists("/some/other.ts");
    fs.stat("/some/dir");
    assert!(fs.seen("/some/file.ts"));
    assert!(fs.seen("/some/other.ts"));
    assert!(fs.seen("/some/dir"));
}

// Go: internal/vfs/trackingvfs/trackingvfs.go (behavior-level; write ops not tracked)
#[test]
fn trackingvfs_does_not_record_writes() {
    let fs = TrackingFs::new(MapFs::from_map(Vec::<(&str, &str)>::new(), true));

    fs.write_file("/out/written.ts", "x").unwrap();
    let _ = fs.remove("/out/written.ts");
    fs.chtimes(
        "/out/written.ts",
        SystemTime::UNIX_EPOCH,
        SystemTime::UNIX_EPOCH,
    )
    .ok();
    assert!(!fs.seen("/out/written.ts"));
}
