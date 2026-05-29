use super::*;

// Go: internal/ast/positionmap_test.go:TestPositionMapASCII
#[test]
fn position_map_ascii() {
    let text = "const x = 1;";
    let pm = compute_position_map(text);
    assert!(pm.is_ascii_only(), "expected ASCII-only");
    for i in 0..=text.len() as i32 {
        assert_eq!(pm.utf8_to_utf16(i), i, "UTF8ToUTF16({i})");
        assert_eq!(pm.utf16_to_utf8(i), i, "UTF16ToUTF8({i})");
    }
}

// Go: internal/ast/positionmap_test.go:TestPositionMapTwoByte
#[test]
fn position_map_two_byte() {
    // "café" — é (U+00E9) is 2 bytes UTF-8, 1 code unit UTF-16.
    let text = "const café = 1;\nconst x = 2;";
    let pm = compute_position_map(text);
    assert!(!pm.is_ascii_only(), "expected non-ASCII");

    // Everything before é (byte offset 9) is identity.
    for i in 0..10 {
        assert_eq!(pm.utf8_to_utf16(i), i, "before é: UTF8ToUTF16({i})");
    }
    // é starts at UTF-8 byte 9, UTF-16 offset 9: same.
    assert_eq!(pm.utf8_to_utf16(9), 9, "at é");
    // After é: UTF-8 byte 11 = UTF-16 offset 10 (delta 1).
    assert_eq!(pm.utf8_to_utf16(11), 10, "after é");

    let x_utf8 = text.rfind('x').unwrap() as i32;
    assert_eq!(pm.utf8_to_utf16(x_utf8), x_utf8 - 1, "at x");
    let x_utf16 = x_utf8 - 1;
    assert_eq!(pm.utf16_to_utf8(x_utf16), x_utf8, "reverse at x");
}

// Go: internal/ast/positionmap_test.go:TestPositionMapFourByte
#[test]
fn position_map_four_byte() {
    // 🎉 (U+1F389) is 4 bytes UTF-8, 2 code units UTF-16.
    let text = "const a = \"🎉\";\nconst b = 2;";
    let pm = compute_position_map(text);
    assert!(!pm.is_ascii_only(), "expected non-ASCII");

    let b_utf8 = text.rfind('b').unwrap() as i32;
    let b_utf16 = b_utf8 - 2; // delta of 2 from emoji
    assert_eq!(pm.utf8_to_utf16(b_utf8), b_utf16, "at b");
    assert_eq!(pm.utf16_to_utf8(b_utf16), b_utf8, "reverse at b");
}

// Go: internal/ast/positionmap_test.go:TestPositionMapMultipleNonASCII
#[test]
fn position_map_multiple_non_ascii() {
    // "à" (U+00E0) = 2 bytes / 1 unit; "🎉" (U+1F389) = 4 bytes / 2 units.
    let text = "à🎉x";
    let pm = compute_position_map(text);
    // à: UTF-8 [0,2), UTF-16 [0,1); 🎉: UTF-8 [2,6), UTF-16 [1,3); x: [6,7)/[3,4).
    let cases = [(0, 0), (2, 1), (6, 3), (7, 4)];
    for (utf8, utf16) in cases {
        assert_eq!(pm.utf8_to_utf16(utf8), utf16, "UTF8ToUTF16({utf8})");
        assert_eq!(pm.utf16_to_utf8(utf16), utf8, "UTF16ToUTF8({utf16})");
    }
}

// Go: internal/ast/positionmap_test.go:TestPositionMapRoundtrip
#[test]
fn position_map_roundtrip() {
    let text = "let café = \"🎉\"; // naïve";
    let pm = compute_position_map(text);
    let utf16_len = pm.utf8_to_utf16(text.len() as i32);
    for i in 0..=utf16_len {
        let utf8_pos = pm.utf16_to_utf8(i);
        let back = pm.utf8_to_utf16(utf8_pos);
        assert_eq!(
            back, i,
            "roundtrip UTF16->UTF8->UTF16: {i} -> {utf8_pos} -> {back}"
        );
    }
}
