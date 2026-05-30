use super::*;

// Go: internal/parser/types.go:ParseFlags
#[test]
fn parse_flags_bit_values() {
    assert_eq!(ParseFlags::NONE.bits(), 0);
    assert_eq!(ParseFlags::YIELD.bits(), 1 << 0);
    assert_eq!(ParseFlags::AWAIT.bits(), 1 << 1);
    assert_eq!(ParseFlags::TYPE.bits(), 1 << 2);
    assert_eq!(ParseFlags::IGNORE_MISSING_OPEN_BRACE.bits(), 1 << 4);
    assert_eq!(ParseFlags::JSDOC.bits(), 1 << 5);
}

// Go: internal/parser/types.go:ParseFlags
#[test]
fn parse_flags_compose() {
    let f = ParseFlags::YIELD | ParseFlags::AWAIT;
    assert!(f.contains(ParseFlags::YIELD));
    assert!(f.contains(ParseFlags::AWAIT));
    assert!(!f.contains(ParseFlags::TYPE));
}
