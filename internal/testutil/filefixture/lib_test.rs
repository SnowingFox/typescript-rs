use super::*;

// Go: internal/testutil/filefixture/filefixture.go:fromString
// Behavior: a string-backed fixture exposes its name/path, returns its contents
// verbatim, and never asks to be skipped.
#[test]
fn from_string_exposes_name_path_and_contents() {
    let f = from_string("greeting", "/virtual/greeting.txt", "hello");
    assert_eq!(f.name(), "greeting");
    assert_eq!(f.path(), "/virtual/greeting.txt");
    assert_eq!(f.read_file(), "hello");
    assert!(!f.should_skip());
}

// Go: internal/testutil/filefixture/filefixture.go:fromFile (FromFile + ReadFile)
// Behavior: a file-backed fixture reads its contents from disk and caches them
// (mirroring Go's `sync.OnceValues`), so a later change on disk is not observed.
#[test]
fn from_file_reads_and_caches_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.txt");
    std::fs::write(&path, "on disk").unwrap();

    let f = from_file("fx", path.to_str().unwrap());
    assert_eq!(f.name(), "fx");
    assert!(!f.should_skip());
    assert_eq!(f.read_file(), "on disk");

    std::fs::write(&path, "changed").unwrap();
    assert_eq!(f.read_file(), "on disk", "contents should be cached");
}

// Go: internal/testutil/filefixture/filefixture.go:fromFile.SkipIfNotExist
// Behavior: skip is requested only when the backing file is absent.
#[test]
fn from_file_should_skip_tracks_existence() {
    let dir = tempfile::tempdir().unwrap();
    let present = dir.path().join("present.txt");
    std::fs::write(&present, "x").unwrap();

    let existing = from_file("present", present.to_str().unwrap());
    assert!(!existing.should_skip());

    let missing = from_file("missing", dir.path().join("missing.txt").to_str().unwrap());
    assert!(missing.should_skip());
}

// Go: internal/testutil/filefixture/filefixture.go:fromFile.ReadFile
// Behavior: reading a missing fixture aborts (Go's `tb.Fatalf` -> our panic).
#[test]
#[should_panic(expected = "Failed to read test fixture")]
fn from_file_read_file_panics_when_missing() {
    let f = from_file("missing", "/no/such/fixture/path.txt");
    let _ = f.read_file();
}
