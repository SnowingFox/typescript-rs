use super::*;
use tsgo_core::Utf16Offset;

// Reference VLQ encoder mirroring `generator.go:appendBase64VLQ`, used only to
// produce inputs for the decoder round-trip below.
fn encode_vlq(mut in_value: i32) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    if in_value < 0 {
        in_value = ((-in_value) << 1) + 1;
    } else {
        in_value <<= 1;
    }
    loop {
        let mut digit = in_value & 31;
        in_value >>= 5;
        if in_value > 0 {
            digit |= 32;
        }
        out.push(ALPHABET[digit as usize] as char);
        if in_value <= 0 {
            break;
        }
    }
    out
}

// Go: decoder.go:base64VLQFormatDecode (dual of generator.go:appendBase64VLQ)
#[test]
fn vlq_roundtrip() {
    for value in [-1, 0, 1, 16, 1000] {
        let encoded = encode_vlq(value);
        let mut decoder = decode_mappings(&encoded);
        let decoded = decoder.base64_vlq_format_decode();
        assert!(
            decoder.error().is_none(),
            "value {value} encoded as {encoded:?}"
        );
        assert_eq!(decoded, value, "value {value} encoded as {encoded:?}");
    }
}

// Go: decoder.go:MappingsDecoder.Next (decode of generator output "AAAA")
#[test]
fn decode_simple_mappings() {
    let mut decoder = decode_mappings("AAAA");
    let mappings: Vec<Mapping> = decoder.values().collect();
    assert!(decoder.error().is_none());
    assert_eq!(
        mappings,
        vec![Mapping {
            generated_line: 0,
            generated_character: Utf16Offset(0),
            source_index: SourceIndex(0),
            source_line: 0,
            source_character: Utf16Offset(0),
            name_index: MISSING_NAME,
        }]
    );
}

// Go: decoder.go:MappingsDecoder.Next (decode of generator output "AAAA,CAAC")
#[test]
fn decode_roundtrip_generator() {
    let mut decoder = decode_mappings("AAAA,CAAC");
    let mappings: Vec<Mapping> = decoder.values().collect();
    assert!(decoder.error().is_none());
    assert_eq!(mappings.len(), 2);
    assert_eq!(
        mappings[0],
        Mapping {
            generated_line: 0,
            generated_character: Utf16Offset(0),
            source_index: SourceIndex(0),
            source_line: 0,
            source_character: Utf16Offset(0),
            name_index: MISSING_NAME,
        }
    );
    assert_eq!(
        mappings[1],
        Mapping {
            generated_line: 0,
            generated_character: Utf16Offset(1),
            source_index: SourceIndex(0),
            source_line: 0,
            source_character: Utf16Offset(1),
            name_index: MISSING_NAME,
        }
    );
}

// Go: decoder.go:base64FormatDecode (-1 branch) / base64VLQFormatDecode "Invalid character in VLQ"
#[test]
fn decode_invalid_char() {
    let mut decoder = decode_mappings("!!");
    let mappings: Vec<Mapping> = decoder.by_ref().collect();
    assert!(mappings.is_empty());
    assert_eq!(
        decoder.error().unwrap().to_string(),
        "Invalid character in VLQ"
    );
}
