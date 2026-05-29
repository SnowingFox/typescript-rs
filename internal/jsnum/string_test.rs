use super::*;

const MAX_MANTISSA: u64 = (1 << 53) - 1;

/// Builds a `Number` from raw IEEE 754 bits (mirrors Go's `numberFromBits`).
fn number_from_bits(b: u64) -> Number {
    Number(f64::from_bits(b))
}

/// Builds a `Number` from IEEE 754 sign/exponent/mantissa fields (mirrors Go's
/// `ieeeParts2Double`).
fn ieee_parts_2_double(sign: bool, ieee_exponent: u32, ieee_mantissa: u64) -> Number {
    assert!(ieee_exponent <= 2047, "ieeeExponent > 2047");
    assert!(ieee_mantissa <= MAX_MANTISSA, "ieeeMantissa > maxMantissa");
    let sign_bit = u64::from(sign);
    number_from_bits((sign_bit << 63) | ((ieee_exponent as u64) << 52) | ieee_mantissa)
}

/// Compares two `Number`s like Go's `assertEqualNumber`.
fn assert_eq_number(got: Number, want: Number, msg: &str) {
    if got.is_nan() || want.is_nan() {
        assert_eq!(
            got.is_nan(),
            want.is_nan(),
            "{msg}: got {got:?}, want {want:?}"
        );
    } else {
        assert_eq!(got, want, "{msg}");
    }
}

/// The explicit `stringTests` entries (Go: string_test.go:stringTests, head).
fn explicit_string_tests() -> Vec<(Number, &'static str)> {
    vec![
        (Number::nan(), "NaN"),
        (Number::inf(1), "Infinity"),
        (Number::inf(-1), "-Infinity"),
        (Number(0.0), "0"),
        (Number(-0.0), "0"),
        (Number(1.0), "1"),
        (Number(-1.0), "-1"),
        (Number(0.3), "0.3"),
        (Number(-0.3), "-0.3"),
        (Number(1.5), "1.5"),
        (Number(-1.5), "-1.5"),
        (Number(1e308), "1e+308"),
        (Number(-1e308), "-1e+308"),
        (Number(std::f64::consts::PI), "3.141592653589793"),
        (Number(-std::f64::consts::PI), "-3.141592653589793"),
        (MAX_SAFE_INTEGER, "9007199254740991"),
        (MIN_SAFE_INTEGER, "-9007199254740991"),
        (
            number_from_bits(0x000F_FFFF_FFFF_FFFF),
            "2.225073858507201e-308",
        ),
        (
            number_from_bits(0x0010_0000_0000_0000),
            "2.2250738585072014e-308",
        ),
        (Number(1234567.8), "1234567.8"),
        (Number(19686109595169230000.0), "19686109595169230000"),
        (Number(123.456), "123.456"),
        (Number(-123.456), "-123.456"),
        (Number(444123.0), "444123"),
        (Number(-444123.0), "-444123"),
        (Number(444123.789123456789875436), "444123.7891234568"),
        (Number(-444123.78963636363636363636), "-444123.7896363636"),
        (Number(1e21), "1e+21"),
        (Number(1e20), "100000000000000000000"),
    ]
}

/// The Ryu corpus (Go: ryu_test.go:ryuTests). Every literal is exercised
/// through `Number::to_string` (via `TestString`) and `FromString`.
fn ryu_tests() -> Vec<(Number, &'static str)> {
    vec![
        (Number(2.2250738585072014e-308), "2.2250738585072014e-308"),
        (
            number_from_bits(0x7fefffffffffffff),
            "1.7976931348623157e+308",
        ),
        (number_from_bits(1), "5e-324"),
        (Number(2.98023223876953125e-8), "2.9802322387695312e-8"),
        (Number(-2.109808898695963e16), "-21098088986959630"),
        (Number(4.940656e-318), "4.940656e-318"),
        (Number(1.18575755e-316), "1.18575755e-316"),
        (Number(2.989102097996e-312), "2.989102097996e-312"),
        (Number(9.0608011534336e15), "9060801153433600"),
        (Number(4.708356024711512e18), "4708356024711512000"),
        (Number(9.409340012568248e18), "9409340012568248000"),
        (Number(1.2345678), "1.2345678"),
        (
            number_from_bits(0x4830F0CF064DD592),
            "5.764607523034235e+39",
        ),
        (
            number_from_bits(0x4840F0CF064DD592),
            "1.152921504606847e+40",
        ),
        (
            number_from_bits(0x4850F0CF064DD592),
            "2.305843009213694e+40",
        ),
        (Number(1.2), "1.2"),
        (Number(1.23), "1.23"),
        (Number(1.234), "1.234"),
        (Number(1.2345), "1.2345"),
        (Number(1.23456), "1.23456"),
        (Number(1.234567), "1.234567"),
        (Number(1.2345678), "1.2345678"),
        (Number(1.23456789), "1.23456789"),
        (Number(1.234567895), "1.234567895"),
        (Number(1.2345678901), "1.2345678901"),
        (Number(1.23456789012), "1.23456789012"),
        (Number(1.234567890123), "1.234567890123"),
        (Number(1.2345678901234), "1.2345678901234"),
        (Number(1.23456789012345), "1.23456789012345"),
        (Number(1.234567890123456), "1.234567890123456"),
        (Number(1.2345678901234567), "1.2345678901234567"),
        (Number(4.294967294), "4.294967294"),
        (Number(4.294967295), "4.294967295"),
        (Number(4.294967296), "4.294967296"),
        (Number(4.294967297), "4.294967297"),
        (Number(4.294967298), "4.294967298"),
        (ieee_parts_2_double(false, 4, 0), "1.7800590868057611e-307"),
        (
            ieee_parts_2_double(false, 6, MAX_MANTISSA),
            "2.8480945388892175e-306",
        ),
        (ieee_parts_2_double(false, 41, 0), "2.446494580089078e-296"),
        (
            ieee_parts_2_double(false, 40, MAX_MANTISSA),
            "4.8929891601781557e-296",
        ),
        (ieee_parts_2_double(false, 1077, 0), "18014398509481984"),
        (
            ieee_parts_2_double(false, 1076, MAX_MANTISSA),
            "36028797018963964",
        ),
        (ieee_parts_2_double(false, 307, 0), "2.900835519859558e-216"),
        (
            ieee_parts_2_double(false, 306, MAX_MANTISSA),
            "5.801671039719115e-216",
        ),
        (
            ieee_parts_2_double(false, 934, 0x000FA7161A4D6E0C),
            "3.196104012172126e-27",
        ),
        (Number(9007199254740991.0), "9007199254740991"),
        (Number(9007199254740992.0), "9007199254740992"),
        (Number(1.0e+0), "1"),
        (Number(1.2e+1), "12"),
        (Number(1.23e+2), "123"),
        (Number(1.234e+3), "1234"),
        (Number(1.2345e+4), "12345"),
        (Number(1.23456e+5), "123456"),
        (Number(1.234567e+6), "1234567"),
        (Number(1.2345678e+7), "12345678"),
        (Number(1.23456789e+8), "123456789"),
        (Number(1.23456789e+9), "1234567890"),
        (Number(1.234567895e+9), "1234567895"),
        (Number(1.2345678901e+10), "12345678901"),
        (Number(1.23456789012e+11), "123456789012"),
        (Number(1.234567890123e+12), "1234567890123"),
        (Number(1.2345678901234e+13), "12345678901234"),
        (Number(1.23456789012345e+14), "123456789012345"),
        (Number(1.234567890123456e+15), "1234567890123456"),
        (Number(1.0e+0), "1"),
        (Number(1.0e+1), "10"),
        (Number(1.0e+2), "100"),
        (Number(1.0e+3), "1000"),
        (Number(1.0e+4), "10000"),
        (Number(1.0e+5), "100000"),
        (Number(1.0e+6), "1000000"),
        (Number(1.0e+7), "10000000"),
        (Number(1.0e+8), "100000000"),
        (Number(1.0e+9), "1000000000"),
        (Number(1.0e+10), "10000000000"),
        (Number(1.0e+11), "100000000000"),
        (Number(1.0e+12), "1000000000000"),
        (Number(1.0e+13), "10000000000000"),
        (Number(1.0e+14), "100000000000000"),
        (Number(1.0e+15), "1000000000000000"),
        (Number(1000000000000001.0), "1000000000000001"),
        (Number(1000000000000010.0), "1000000000000010"),
        (Number(1000000000000100.0), "1000000000000100"),
        (Number(1000000000001000.0), "1000000000001000"),
        (Number(1000000000010000.0), "1000000000010000"),
        (Number(1000000000100000.0), "1000000000100000"),
        (Number(1000000001000000.0), "1000000001000000"),
        (Number(1000000010000000.0), "1000000010000000"),
        (Number(1000000100000000.0), "1000000100000000"),
        (Number(1000001000000000.0), "1000001000000000"),
        (Number(1000010000000000.0), "1000010000000000"),
        (Number(1000100000000000.0), "1000100000000000"),
        (Number(1001000000000000.0), "1001000000000000"),
        (Number(1010000000000000.0), "1010000000000000"),
        (Number(1100000000000000.0), "1100000000000000"),
        (Number(8.0), "8"),
        (Number(64.0), "64"),
        (Number(512.0), "512"),
        (Number(8192.0), "8192"),
        (Number(65536.0), "65536"),
        (Number(524288.0), "524288"),
        (Number(8388608.0), "8388608"),
        (Number(67108864.0), "67108864"),
        (Number(536870912.0), "536870912"),
        (Number(8589934592.0), "8589934592"),
        (Number(68719476736.0), "68719476736"),
        (Number(549755813888.0), "549755813888"),
        (Number(8796093022208.0), "8796093022208"),
        (Number(70368744177664.0), "70368744177664"),
        (Number(562949953421312.0), "562949953421312"),
        (Number(9007199254740992.0), "9007199254740992"),
        (Number(8.0e+3), "8000"),
        (Number(64.0e+3), "64000"),
        (Number(512.0e+3), "512000"),
        (Number(8192.0e+3), "8192000"),
        (Number(65536.0e+3), "65536000"),
        (Number(524288.0e+3), "524288000"),
        (Number(8388608.0e+3), "8388608000"),
        (Number(67108864.0e+3), "67108864000"),
        (Number(536870912.0e+3), "536870912000"),
        (Number(8589934592.0e+3), "8589934592000"),
        (Number(68719476736.0e+3), "68719476736000"),
        (Number(549755813888.0e+3), "549755813888000"),
        (Number(8796093022208.0e+3), "8796093022208000"),
    ]
}

/// The full `stringTests` slice: explicit cases concatenated with the Ryu
/// corpus (Go: string_test.go:stringTests = slices.Concat(..., ryuTests)).
fn string_tests() -> Vec<(Number, &'static str)> {
    let mut v = explicit_string_tests();
    v.extend(ryu_tests());
    v
}

// Go: internal/jsnum/string_test.go:TestString (stringTests, explicit + ryuTests)
#[test]
fn string_table() {
    for (number, want) in string_tests() {
        assert_eq!(number.to_string(), want, "String({:?})", f64::from(number));
    }
}

/// The `fromStringTests` slice (Go: string_test.go:fromStringTests, ~97 cases,
/// including the deliberate duplicates `0X0`, `0xABCDEF`, `0b2`).
fn from_string_tests() -> Vec<(Number, &'static str)> {
    vec![
        (Number::nan(), "    NaN"),
        (Number::inf(1), "Infinity    "),
        (Number::inf(-1), "    -Infinity"),
        (Number(1.0), "1."),
        (Number(1.0), "1.0   "),
        (Number(1.0), "+1"),
        (Number(1.0), "+1."),
        (Number(1.0), "+1.0"),
        (Number::nan(), "whoops"),
        (Number(0.0), ""),
        (Number(0.0), "0"),
        (Number(0.0), "0."),
        (Number(0.0), "0.0"),
        (Number(0.0), "0.0000"),
        (Number(0.0), ".0000"),
        (Number(-0.0), "-0"),
        (Number(-0.0), "-0."),
        (Number(-0.0), "-0.0"),
        (Number(-0.0), "-.0"),
        (Number::nan(), "."),
        (Number::nan(), "e"),
        (Number::nan(), ".e"),
        (Number::nan(), "+"),
        (Number(0.0), "0X0"),
        (Number::nan(), "e0"),
        (Number::nan(), "E0"),
        (Number::nan(), "1e"),
        (Number::nan(), "1e+"),
        (Number::nan(), "1e-"),
        (Number(1.0), "1e+0"),
        (Number::nan(), "++0"),
        (Number::nan(), "0_0"),
        (Number::inf(1), "1e1000"),
        (Number::inf(-1), "-1e1000"),
        (Number(0.0), ".0e0"),
        (Number::nan(), "0e++0"),
        (Number(10.0), "0XA"),
        (Number(0b1010 as f64), "0b1010"),
        (Number(0b1010 as f64), "0B1010"),
        (Number(0o12 as f64), "0o12"),
        (Number(0o12 as f64), "0O12"),
        (Number(0x123456789abcdef0u64 as f64), "0x123456789abcdef0"),
        (Number(0x123456789abcdef0u64 as f64), "0X123456789ABCDEF0"),
        (Number(18446744073709552000.0), "0X10000000000000000"),
        (Number(18446744073709597000.0), "0X1000000000000A801"),
        (Number::nan(), "0B0.0"),
        (
            Number(1.231235345083403e+91),
            "12312353450834030486384068034683603046834603806830644850340602384608368034634603680348603864",
        ),
        (
            Number::nan(),
            "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX8OOOOOOOOOOOOOOOOOOO",
        ),
        (Number::inf(1), "+Infinity"),
        (Number(1234.56), "  \t1234.56  "),
        (Number::nan(), "\u{200b}"),
        (Number(0.0), " "),
        (Number(0.0), "\n"),
        (Number(0.0), "\r"),
        (Number(0.0), "\r\n"),
        (Number(0.0), "\u{2028}"),
        (Number(0.0), "\u{2029}"),
        (Number(0.0), "\t"),
        (Number(0.0), "\u{000B}"),
        (Number(0.0), "\u{000C}"),
        (Number(0.0), "\u{FEFF}"),
        (Number(0.0), "\u{00A0}"),
        (Number(10000000000000000000.0), "010000000000000000000"),
        (Number::nan(), "0x1.fffffffffffffp1023"),
        (Number::nan(), "0X_1FFFP-16"),
        (Number::nan(), "1_000"),
        (Number(0.0), "0x0"),
        (Number(0.0), "0X0"),
        (Number::nan(), "0xOOPS"),
        (Number(0xABCDEFu32 as f64), "0xABCDEF"),
        (Number(0xABCDEFu32 as f64), "0xABCDEF"),
        (Number(0.0), "0o0"),
        (Number(0.0), "0O0"),
        (Number::nan(), "0o8"),
        (Number::nan(), "0O8"),
        (Number(0o12345 as f64), "0o12345"),
        (Number(0o12345 as f64), "0O12345"),
        (Number(0.0), "0b0"),
        (Number(0.0), "0B0"),
        (Number::nan(), "0b2"),
        (Number::nan(), "0b2"),
        (Number(0b10101 as f64), "0b10101"),
        (Number(0b10101 as f64), "0B10101"),
        (Number::nan(), "1.f"),
        (Number::nan(), "1.e"),
        (Number::nan(), "1.0ef"),
        (Number::nan(), "1.0e"),
        (Number::nan(), ".f"),
        (Number::nan(), ".e"),
        (Number::nan(), ".0ef"),
        (Number::nan(), ".0e"),
        (Number::nan(), "a.f"),
        (Number::nan(), "a.e"),
        (Number::nan(), "a.0ef"),
        (Number::nan(), "a.0e"),
    ]
}

// Go: internal/jsnum/string_test.go:TestFromString/stringTests
#[test]
fn from_string_string_tests() {
    for (number, str) in string_tests() {
        assert_eq_number(from_string(str), number, &format!("FromString({str:?})"));
        assert_eq_number(
            from_string(&format!("{str} ")),
            number,
            &format!("FromString({str:?}+\" \")"),
        );
        assert_eq_number(
            from_string(&format!(" {str}")),
            number,
            &format!("FromString(\" \"+{str:?})"),
        );
    }
}

// Go: internal/jsnum/string_test.go:TestFromString/fromStringTests
#[test]
fn from_string_table() {
    for (number, str) in from_string_tests() {
        assert_eq_number(from_string(str), number, &format!("FromString({str:?})"));
    }
}

// Go: internal/jsnum/string_test.go:TestStringRoundtrip
#[test]
fn string_roundtrip() {
    for (_number, str) in string_tests() {
        assert_eq!(from_string(str).to_string(), str, "roundtrip({str:?})");
    }
}
