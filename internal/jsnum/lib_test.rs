use super::*;

/// Builds a `Number` from raw IEEE 754 bits (test helper mirroring Go's
/// `numberFromBits`).
fn number_from_bits(b: u64) -> Number {
    Number(f64::from_bits(b))
}

/// Compares two `Number`s like Go's `assertEqualNumber`: when either side is
/// `NaN`, only the `is_nan` flags are compared; otherwise plain `==`.
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

// Go: internal/jsnum/jsnum_test.go:TestToInt32 (toInt32Tests, 47 cases)
#[test]
fn to_int32_table() {
    let cases: &[(&str, Number, i32)] = &[
        ("0.0", Number(0.0), 0),
        ("-0.0", Number(-0.0), 0),
        ("NaN", Number::nan(), 0),
        ("+Inf", Number::inf(1), 0),
        ("-Inf", Number::inf(-1), 0),
        ("MaxInt32", Number(2147483647.0), 2147483647),
        ("MaxInt32+1", Number(2147483648.0), -2147483648),
        ("MinInt32", Number(-2147483648.0), -2147483648),
        ("MinInt32-1", Number(-2147483649.0), 2147483647),
        ("MIN_SAFE_INTEGER", MIN_SAFE_INTEGER, 1),
        ("MIN_SAFE_INTEGER-1", Number(MIN_SAFE_INTEGER.0 - 1.0), 0),
        ("MIN_SAFE_INTEGER+1", Number(MIN_SAFE_INTEGER.0 + 1.0), 2),
        ("MAX_SAFE_INTEGER", MAX_SAFE_INTEGER, -1),
        ("MAX_SAFE_INTEGER-1", Number(MAX_SAFE_INTEGER.0 - 1.0), -2),
        ("MAX_SAFE_INTEGER+1", Number(MAX_SAFE_INTEGER.0 + 1.0), 0),
        ("-8589934590", Number(-8589934590.0), 2),
        ("0xDEADBEEF", Number(3735928559.0), -559038737),
        ("4294967808", Number(4294967808.0), 512),
        ("-0.4", Number(-0.4), 0),
        ("SmallestNonzeroFloat64", number_from_bits(1), 0),
        ("-SmallestNonzeroFloat64", Number(-f64::from_bits(1)), 0),
        ("MaxFloat64", Number(f64::MAX), 0),
        ("-MaxFloat64", Number(-f64::MAX), 0),
        (
            "Largest subnormal number",
            number_from_bits(0x000F_FFFF_FFFF_FFFF),
            0,
        ),
        (
            "Smallest positive normal number",
            number_from_bits(0x0010_0000_0000_0000),
            0,
        ),
        ("Largest normal number", Number(f64::MAX), 0),
        ("-Largest normal number", Number(-f64::MAX), 0),
        ("1.0", Number(1.0), 1),
        ("-1.0", Number(-1.0), -1),
        ("1e308", Number(1e308), 0),
        ("-1e308", Number(-1e308), 0),
        ("math.Pi", Number(std::f64::consts::PI), 3),
        ("-math.Pi", Number(-std::f64::consts::PI), -3),
        ("math.E", Number(std::f64::consts::E), 2),
        ("-math.E", Number(-std::f64::consts::E), -2),
        ("0.5", Number(0.5), 0),
        ("-0.5", Number(-0.5), 0),
        ("0.49999999999999994", Number(0.49999999999999994), 0),
        ("-0.49999999999999994", Number(-0.49999999999999994), 0),
        ("0.5000000000000001", Number(0.5000000000000001), 0),
        ("-0.5000000000000001", Number(-0.5000000000000001), 0),
        ("2^31 + 0.5", Number(2147483648.5), -2147483648),
        ("-2^31 - 0.5", Number(-2147483648.5), -2147483648),
        ("2^40", Number(1099511627776.0), 0),
        ("-2^40", Number(-1099511627776.0), 0),
        ("TypeFlagsNarrowable", Number(536624127.0), 536624127),
    ];

    for (name, input, want) in cases {
        let got = input.to_int32();
        assert_eq!(got, *want, "to_int32({name})");
    }
}

// Go: internal/jsnum/jsnum_test.go:TestBitwiseNOT (7 cases)
#[test]
fn bitwise_not_table() {
    let cases: &[(Number, Number)] = &[
        (Number(-2147483649.0), Number(-2147483648.0)),
        (Number(2147483647.0), Number(-2147483648.0)),
        (Number(-4294967296.0), Number(-1.0)),
        (Number(0.0), Number(-1.0)),
        (Number(2147483648.0), Number(2147483647.0)),
        (Number(-2147483648.0), Number(2147483647.0)),
        (Number(4294967296.0), Number(-1.0)),
    ];
    for (x, want) in cases {
        assert_eq_number(x.bitwise_not(), *want, &format!("~{x:?}"));
    }
}

// Go: internal/jsnum/jsnum_test.go:TestBitwiseAND (4 cases)
#[test]
fn bitwise_and_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number(0.0), Number(0.0), Number(0.0)),
        (Number(0.0), Number(1.0), Number(0.0)),
        (Number(1.0), Number(0.0), Number(0.0)),
        (Number(1.0), Number(1.0), Number(1.0)),
    ];
    for (x, y, want) in cases {
        assert_eq_number(x.bitwise_and(*y), *want, &format!("{x:?} & {y:?}"));
    }
}

// Go: internal/jsnum/jsnum_test.go:TestBitwiseOR (4 cases)
#[test]
fn bitwise_or_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number(0.0), Number(0.0), Number(0.0)),
        (Number(0.0), Number(1.0), Number(1.0)),
        (Number(1.0), Number(0.0), Number(1.0)),
        (Number(1.0), Number(1.0), Number(1.0)),
    ];
    for (x, y, want) in cases {
        assert_eq_number(x.bitwise_or(*y), *want, &format!("{x:?} | {y:?}"));
    }
}

// Go: internal/jsnum/jsnum_test.go:TestBitwiseXOR (4 cases)
#[test]
fn bitwise_xor_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number(0.0), Number(0.0), Number(0.0)),
        (Number(0.0), Number(1.0), Number(1.0)),
        (Number(1.0), Number(0.0), Number(1.0)),
        (Number(1.0), Number(1.0), Number(0.0)),
    ];
    for (x, y, want) in cases {
        assert_eq_number(x.bitwise_xor(*y), *want, &format!("{x:?} ^ {y:?}"));
    }
}

// Go: internal/jsnum/jsnum_test.go:TestSignedRightShift (13 cases)
#[test]
fn signed_right_shift_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number(1.0), Number(0.0), Number(1.0)),
        (Number(1.0), Number(1.0), Number(0.0)),
        (Number(1.0), Number(2.0), Number(0.0)),
        (Number(1.0), Number(31.0), Number(0.0)),
        (Number(1.0), Number(32.0), Number(1.0)),
        (Number(-4.0), Number(0.0), Number(-4.0)),
        (Number(-4.0), Number(1.0), Number(-2.0)),
        (Number(-4.0), Number(2.0), Number(-1.0)),
        (Number(-4.0), Number(3.0), Number(-1.0)),
        (Number(-4.0), Number(4.0), Number(-1.0)),
        (Number(-4.0), Number(31.0), Number(-1.0)),
        (Number(-4.0), Number(32.0), Number(-4.0)),
        (Number(-4.0), Number(33.0), Number(-2.0)),
    ];
    for (x, y, want) in cases {
        assert_eq_number(x.signed_right_shift(*y), *want, &format!("{x:?} >> {y:?}"));
    }
}

// Go: internal/jsnum/jsnum_test.go:TestUnsignedRightShift (13 cases)
#[test]
fn unsigned_right_shift_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number(1.0), Number(0.0), Number(1.0)),
        (Number(1.0), Number(1.0), Number(0.0)),
        (Number(1.0), Number(2.0), Number(0.0)),
        (Number(1.0), Number(31.0), Number(0.0)),
        (Number(1.0), Number(32.0), Number(1.0)),
        (Number(-4.0), Number(0.0), Number(4294967292.0)),
        (Number(-4.0), Number(1.0), Number(2147483646.0)),
        (Number(-4.0), Number(2.0), Number(1073741823.0)),
        (Number(-4.0), Number(3.0), Number(536870911.0)),
        (Number(-4.0), Number(4.0), Number(268435455.0)),
        (Number(-4.0), Number(31.0), Number(1.0)),
        (Number(-4.0), Number(32.0), Number(4294967292.0)),
        (Number(-4.0), Number(33.0), Number(2147483646.0)),
    ];
    for (x, y, want) in cases {
        assert_eq_number(
            x.unsigned_right_shift(*y),
            *want,
            &format!("{x:?} >>> {y:?}"),
        );
    }
}

// Go: internal/jsnum/jsnum_test.go:TestLeftShift (11 cases)
#[test]
fn left_shift_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number(1.0), Number(0.0), Number(1.0)),
        (Number(1.0), Number(1.0), Number(2.0)),
        (Number(1.0), Number(2.0), Number(4.0)),
        (Number(1.0), Number(31.0), Number(-2147483648.0)),
        (Number(1.0), Number(32.0), Number(1.0)),
        (Number(-4.0), Number(0.0), Number(-4.0)),
        (Number(-4.0), Number(1.0), Number(-8.0)),
        (Number(-4.0), Number(2.0), Number(-16.0)),
        (Number(-4.0), Number(3.0), Number(-32.0)),
        (Number(-4.0), Number(31.0), Number(0.0)),
        (Number(-4.0), Number(32.0), Number(-4.0)),
    ];
    for (x, y, want) in cases {
        assert_eq_number(x.left_shift(*y), *want, &format!("{x:?} << {y:?}"));
    }
}

// Go: internal/jsnum/jsnum_test.go:TestRemainder (24 cases)
#[test]
fn remainder_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number::nan(), Number(1.0), Number::nan()),
        (Number(1.0), Number::nan(), Number::nan()),
        (Number::inf(1), Number(1.0), Number::nan()),
        (Number::inf(-1), Number(1.0), Number::nan()),
        (Number(123.0), Number::inf(1), Number(123.0)),
        (Number(123.0), Number::inf(-1), Number(123.0)),
        (Number(123.0), Number(0.0), Number::nan()),
        (Number(123.0), Number(-0.0), Number::nan()),
        (Number(0.0), Number(123.0), Number(0.0)),
        (Number(-0.0), Number(123.0), Number(-0.0)),
        (Number(10.0), Number(3.0), Number(1.0)),
        (Number(-10.0), Number(3.0), Number(-1.0)),
        (Number(10.0), Number(-3.0), Number(1.0)),
        (Number(-10.0), Number(-3.0), Number(-1.0)),
        (Number(5.5), Number(2.0), Number(1.5)),
        (Number(-5.5), Number(2.0), Number(-1.5)),
        (Number(1.0), Number(0.5), Number(0.0)),
        (Number(-1.0), Number(0.5), Number(-0.0)),
        (Number(1.5), Number(1.0), Number(0.5)),
        (Number(-1.5), Number(1.0), Number(-0.5)),
        (Number(7.0), Number(0.1), Number(7.0 % 0.1)),
        (Number(7.0), Number(0.2), Number(7.0 % 0.2)),
        (Number(7.0), Number(0.3), Number(7.0 % 0.3)),
        (Number(100.0), Number(0.3), Number(100.0 % 0.3)),
    ];
    for (x, y, want) in cases {
        assert_eq_number(x.remainder(*y), *want, &format!("{x:?} % {y:?}"));
    }
}

// Go: internal/jsnum/jsnum_test.go:TestExponentiate (core cases, 26)
#[test]
fn exponentiate_table() {
    let cases: &[(Number, Number, Number)] = &[
        (Number(2.0), Number(3.0), Number(8.0)),
        (Number::inf(1), Number(3.0), Number::inf(1)),
        (Number::inf(1), Number(-5.0), Number(0.0)),
        (Number::inf(-1), Number(3.0), Number::inf(-1)),
        (Number::inf(-1), Number(4.0), Number::inf(1)),
        (Number::inf(-1), Number(-3.0), Number(-0.0)),
        (Number::inf(-1), Number(-4.0), Number(0.0)),
        (Number(0.0), Number(3.0), Number(0.0)),
        (Number(0.0), Number(-10.0), Number::inf(1)),
        (Number(-0.0), Number(3.0), Number(-0.0)),
        (Number(-0.0), Number(4.0), Number(0.0)),
        (Number(-0.0), Number(-3.0), Number::inf(-1)),
        (Number(-0.0), Number(-4.0), Number::inf(1)),
        (Number(3.0), Number::inf(1), Number::inf(1)),
        (Number(-3.0), Number::inf(1), Number::inf(1)),
        (Number(3.0), Number::inf(-1), Number(0.0)),
        (Number(-3.0), Number::inf(-1), Number(0.0)),
        (Number::nan(), Number(3.0), Number::nan()),
        (Number(1.0), Number::inf(1), Number::nan()),
        (Number(1.0), Number::inf(-1), Number::nan()),
        (Number(-1.0), Number::inf(1), Number::nan()),
        (Number(-1.0), Number::inf(-1), Number::nan()),
        (Number(1.0), Number::nan(), Number::nan()),
        (
            Number(10.0),
            Number(308.0),
            number_from_bits(0x7fe1_ccf3_85eb_c8a0),
        ),
        (
            Number(5.0),
            Number(210.0),
            number_from_bits(0x5e68_557f_3132_6bbb),
        ),
        (
            Number(10.0),
            Number(200.0),
            number_from_bits(0x6974_e718_d7d7_625a),
        ),
    ];
    for (x, y, want) in cases {
        assert_eq_number(x.exponentiate(*y), *want, &format!("{x:?} ** {y:?}"));
    }
}
