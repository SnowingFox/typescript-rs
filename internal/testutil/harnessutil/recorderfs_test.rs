use super::*;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

// A write is recorded under its (real) path with its content.
// Go: internal/testutil/harnessutil/recorderfs.go:OutputRecorderFS.WriteFile
#[test]
fn records_a_single_write() {
    let recorder = OutputRecorderFs::new(MapFs::from_map([("/a.ts", "x")], true));
    recorder.write_file("/out.js", "emitted").unwrap();
    let outputs = recorder.outputs();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].unit_name, "/out.js");
    assert_eq!(outputs[0].content, "emitted");
}

// A later write to the same path overwrites the recorded content in place,
// keeping the original ordering slot.
// Go: internal/testutil/harnessutil/recorderfs.go:OutputRecorderFS.WriteFile (outputsMap)
#[test]
fn rewriting_same_path_updates_in_place() {
    let recorder = OutputRecorderFs::new(MapFs::from_map([("/a.ts", "x")], true));
    recorder.write_file("/first.js", "v1").unwrap();
    recorder.write_file("/second.js", "w1").unwrap();
    recorder.write_file("/first.js", "v2").unwrap();

    let outputs = recorder.outputs();
    assert_eq!(outputs.len(), 2);
    // /first.js keeps slot 0 with the updated content.
    assert_eq!(outputs[0].unit_name, "/first.js");
    assert_eq!(outputs[0].content, "v2");
    assert_eq!(outputs[1].unit_name, "/second.js");
    assert_eq!(outputs[1].content, "w1");
}

// Reads (and other operations) pass through to the wrapped file system, which
// also reflects the recorded write.
// Go: internal/testutil/harnessutil/recorderfs.go:OutputRecorderFS (embedded vfs.FS)
#[test]
fn read_and_write_pass_through_to_inner() {
    let recorder = OutputRecorderFs::new(MapFs::from_map([("/a.ts", "source")], true));
    assert_eq!(recorder.read_file("/a.ts").as_deref(), Some("source"));
    recorder.write_file("/b.ts", "written").unwrap();
    assert_eq!(recorder.read_file("/b.ts").as_deref(), Some("written"));
    assert!(recorder.file_exists("/b.ts"));
}

// A fresh recorder has no outputs.
// Go: internal/testutil/harnessutil/recorderfs.go:OutputRecorderFS.Outputs
#[test]
fn fresh_recorder_has_no_outputs() {
    let recorder = OutputRecorderFs::new(MapFs::from_map([("/a.ts", "x")], true));
    assert!(recorder.outputs().is_empty());
}
