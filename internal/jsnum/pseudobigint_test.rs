use super::*;
use crate::Number;

// Go: internal/jsnum/pseudobigint_test.go:TestParsePseudoBigInt/strip base-10 strings
#[test]
fn parse_pseudo_big_int_strip_base10() {
    let mut test_numbers: Vec<i64> = (0..1000).collect();
    for bits in 0..53 {
        test_numbers.push(1i64 << bits);
        test_numbers.push((1i64 << bits) - 1);
    }

    for &i in &test_numbers {
        let num_str = Number::from(i as f64).to_string();
        for leading_zeros in 0..10 {
            let input = format!("{}{}n", "0".repeat(leading_zeros), num_str);
            assert_eq!(parse_pseudo_big_int(&input), num_str, "input {input:?}");
        }
    }
}

// Go: internal/jsnum/pseudobigint_test.go:TestParsePseudoBigInt/parse non-decimal bases (small numbers)
#[test]
fn parse_pseudo_big_int_non_decimal() {
    let cases: &[(&str, &str)] = &[
        // binary
        ("0b0n", "0"),
        ("0b1n", "1"),
        ("0b1010n", "10"),
        ("0b1010_0101n", "165"),
        ("0B1101n", "13"),
        // octal
        ("0o0n", "0"),
        ("0o7n", "7"),
        ("0o755n", "493"),
        ("0o7_5_5n", "493"),
        ("0O12n", "10"),
        // hex
        ("0x0n", "0"),
        ("0xFn", "15"),
        ("0xFFn", "255"),
        ("0xF_Fn", "255"),
        ("0X1Fn", "31"),
    ];
    for (lit, out) in cases {
        assert_eq!(parse_pseudo_big_int(lit), *out, "literal: {lit:?}");
    }
}

// Go: internal/jsnum/pseudobigint_test.go:TestParsePseudoBigInt/can parse large literals
#[test]
fn parse_pseudo_big_int_large() {
    assert_eq!(
        parse_pseudo_big_int("123456789012345678901234567890n"),
        "123456789012345678901234567890"
    );
    assert_eq!(
        parse_pseudo_big_int(
            "0b1100011101110100100001111111101101100001101110011111000001110111001001110001111110000101011010010n"
        ),
        "123456789012345678901234567890"
    );
    assert_eq!(
        parse_pseudo_big_int("0o143564417755415637016711617605322n"),
        "123456789012345678901234567890"
    );
    assert_eq!(
        parse_pseudo_big_int("0x18ee90ff6c373e0ee4e3f0ad2n"),
        "123456789012345678901234567890"
    );
}

// Behavior coverage for the public API with no direct Go unit test
// (NewPseudoBigInt / String / Sign / ParseValidBigInt), using values derived
// from Go semantics.
// Go: internal/jsnum/pseudobigint.go:NewPseudoBigInt/String/Sign/ParseValidBigInt
#[test]
fn pseudo_big_int_api() {
    // new strips leading zeros; sign is dropped for zero.
    assert_eq!(PseudoBigInt::new("0123", false).to_string(), "123");
    assert_eq!(PseudoBigInt::new("0123", true).to_string(), "-123");
    assert_eq!(PseudoBigInt::new("000", true).to_string(), "0");
    assert_eq!(PseudoBigInt::new("123", false).sign(), 1);
    assert_eq!(PseudoBigInt::new("123", true).sign(), -1);
    assert_eq!(PseudoBigInt::new("0", true).sign(), 0);

    // parse_valid_big_int strips a leading '-' then parses the magnitude.
    let pos = parse_valid_big_int("0x1Fn");
    assert_eq!(pos.to_string(), "31");
    assert_eq!(pos.sign(), 1);
    let neg = parse_valid_big_int("-0o755n");
    assert_eq!(neg.to_string(), "-493");
    assert_eq!(neg.sign(), -1);
    assert_eq!(parse_valid_big_int("-0n").to_string(), "0");
}
