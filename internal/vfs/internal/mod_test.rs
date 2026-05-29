use super::*;

// Go: internal/vfs/internal/internal.go:SplitPath/RootLength
#[test]
fn internal_split_path() {
    assert_eq!(
        split_path("/c:/foo"),
        ("/".to_string(), "c:/foo".to_string())
    );
    assert_eq!(
        split_path("c:/foo/bar"),
        ("c:/".to_string(), "foo/bar".to_string())
    );
    assert_eq!(
        split_path("/foo/bar/"),
        ("/".to_string(), "foo/bar".to_string())
    );
}

// Go: internal/vfs/internal/internal.go:RootLength
#[test]
#[should_panic(expected = "vfs: path \"bar\" is not absolute")]
fn internal_root_length_panics_unrooted() {
    let _ = root_length("bar");
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestBOM/UTF8
#[test]
fn decode_bytes_utf8_bom_stripped() {
    assert_eq!(decode_bytes(b"\xEF\xBB\xBFhello, world"), "hello, world");
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestBOM/LittleEndian
#[test]
fn decode_bytes_utf16_le() {
    let mut bytes = vec![0xFF, 0xFE];
    for u in "hello, world".encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    assert_eq!(decode_bytes(&bytes), "hello, world");
}

// Go: internal/vfs/vfstest/vfstest_test.go:TestBOM/BigEndian
#[test]
fn decode_bytes_utf16_be() {
    let mut bytes = vec![0xFE, 0xFF];
    for u in "hello, world".encode_utf16() {
        bytes.extend_from_slice(&u.to_be_bytes());
    }
    assert_eq!(decode_bytes(&bytes), "hello, world");
}
