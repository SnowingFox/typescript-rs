//! `Number.prototype.toString` and `StringToNumber`, ported 1:1 from Go
//! `internal/jsnum/string.go`.

use std::fmt;

use crate::{bigint_to_f64, Number, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER};
use num_bigint::BigInt;

/// JavaScript `StringToNumber` (the abstract operation behind `Number(str)`).
///
/// Trims ECMAScript whitespace, handles `Infinity`/`+Infinity`/`-Infinity` and
/// the empty string (`0`), recognizes the `0b`/`0o`/`0x` integer prefixes and
/// large integers, and otherwise parses a decimal float. Invalid input yields
/// `NaN`.
///
/// # Examples
/// ```
/// use tsgo_jsnum::{from_string, Number};
/// assert_eq!(from_string("0x1F"), Number::from(31.0));
/// assert_eq!(from_string("  1.5 "), Number::from(1.5));
/// assert!(from_string("whoops").is_nan());
/// ```
///
/// Side effects: none (pure).
// Go: internal/jsnum/string.go:FromString
pub fn from_string(s: &str) -> Number {
    // Implementing StringToNumber exactly as written in the spec would require
    // a parser plus AST-to-value conversion. Instead this breaks the number
    // apart and fixes it up so Rust's own parsing can handle it.
    let s = s.trim_matches(is_str_white_space);

    match s {
        "" => return Number(0.0),
        "Infinity" | "+Infinity" => return Number::inf(1),
        "-Infinity" => return Number::inf(-1),
        _ => {}
    }

    for r in s.chars() {
        if !is_number_rune(r) {
            return Number::nan();
        }
    }

    if let Some(n) = try_parse_int(s) {
        return n;
    }

    // Cut this off first so we can ensure -0 is returned as -0.
    let (s, negative) = match s.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (s.strip_prefix('+').unwrap_or(s), false),
    };

    let first = s.chars().next().unwrap_or('\0');
    if !tsgo_stringutil::is_digit(first) && first != '.' {
        return Number::nan();
    }

    let f = parse_float_string(s);
    if f.is_nan() {
        return Number::nan();
    }

    let sign = if negative { -1.0 } else { 1.0 };
    Number(f.copysign(sign))
}

// Go: internal/jsnum/string.go:isStrWhiteSpace
fn is_str_white_space(r: char) -> bool {
    // This is different than stringutil::is_white_space_like: it is exactly the
    // ECMAScript WhiteSpace + LineTerminator set, i.e. Unicode Zs plus the
    // listed control characters (notably excluding U+200B and U+0085).
    matches!(
        r,
        // LineTerminator
        '\n' | '\r' | '\u{2028}' | '\u{2029}'
        // WhiteSpace (explicit)
        | '\t' | '\u{000B}' | '\u{000C}' | '\u{FEFF}'
        // WhiteSpace (Unicode Zs / Space_Separator)
        | '\u{0020}' | '\u{00A0}' | '\u{1680}'
        | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// Tries to parse `s` as a JS integer literal.
///
/// Returns `Some` when `s` is recognized as an integer form (`Some(NaN)` for a
/// malformed prefixed literal, mirroring Go's `(NaN(), true)`), and `None` when
/// `s` should fall through to float parsing (Go's `(0, false)`).
// Go: internal/jsnum/string.go:tryParseInt
fn try_parse_int(s: &str) -> Option<Number> {
    let mut radix_rest: Option<(u32, &str)> = None;
    if s.len() > 2 {
        let (prefix, rest) = s.split_at(2);
        match prefix {
            "0b" | "0B" => {
                if !is_all_binary_digits(rest) {
                    return Some(Number::nan());
                }
                radix_rest = Some((2, rest));
            }
            "0o" | "0O" => {
                if !is_all_octal_digits(rest) {
                    return Some(Number::nan());
                }
                radix_rest = Some((8, rest));
            }
            "0x" | "0X" => {
                if !is_all_hex_digits(rest) {
                    return Some(Number::nan());
                }
                radix_rest = Some((16, rest));
            }
            _ => {}
        }
    }

    let (radix, parse_str) = match radix_rest {
        Some(rr) => rr,
        None => {
            // StringToNumber does not parse leading zeros as octal.
            let trimmed = trim_leading_zeros(s);
            if !is_all_digits(trimmed) {
                return None;
            }
            (10, trimmed)
        }
    };

    match i64::from_str_radix(parse_str, radix) {
        Ok(i) => Some(Number(i as f64)),
        // Overflow: fall back to big-integer parsing (digits already validated).
        Err(_) => match BigInt::parse_bytes(parse_str.as_bytes(), radix) {
            Some(bi) => Some(Number(bigint_to_f64(&bi))),
            None => Some(Number::nan()),
        },
    }
}

// Go: internal/jsnum/string.go:parseFloatString
fn parse_float_string(s: &str) -> f64 {
    // Shapes: <a>, <a>.<b>, <a>.<b>e<c>, <a>e<c>.
    let (a, b, c, has_dot, has_exp);
    if let Some((before, rest)) = s.split_once('.') {
        has_dot = true;
        let (bb, cc, found) = cut_any(rest, &['e', 'E']);
        a = before;
        b = bb;
        c = cc;
        has_exp = found;
    } else {
        has_dot = false;
        let (aa, cc, found) = cut_any(s, &['e', 'E']);
        a = aa;
        b = "";
        c = cc;
        has_exp = found;
    }

    let mut sb = String::with_capacity(a.len() + b.len() + c.len() + 3);

    if a.is_empty() {
        if has_dot && b.is_empty() {
            return f64::NAN;
        }
        if has_exp && c.is_empty() {
            return f64::NAN;
        }
        sb.push('0');
    } else {
        let a = trim_leading_zeros(a);
        if !is_all_digits(a) {
            return f64::NAN;
        }
        sb.push_str(a);
    }

    if has_dot {
        sb.push('.');
        if b.is_empty() {
            sb.push('0');
        } else {
            let b = trim_trailing_zeros(b);
            if !is_all_digits(b) {
                return f64::NAN;
            }
            sb.push_str(b);
        }
    }

    if has_exp {
        sb.push('e');
        let (c, negative) = match c.strip_prefix('-') {
            Some(rest) => (rest, true),
            None => (c.strip_prefix('+').unwrap_or(c), false),
        };
        if negative {
            sb.push('-');
        }
        let c = trim_leading_zeros(c);
        if !is_all_digits(c) {
            return f64::NAN;
        }
        sb.push_str(c);
    }

    string_to_float64(&sb)
}

/// Splits `s` at the first occurrence of any char in `cutset`, dropping that
/// char (mirrors Go's `cutAny`).
// Go: internal/jsnum/string.go:cutAny
fn cut_any<'a>(s: &'a str, cutset: &[char]) -> (&'a str, &'a str, bool) {
    if let Some(i) = s.find(|c| cutset.contains(&c)) {
        let before = &s[..i];
        let after_and_found = &s[i..];
        let size = after_and_found.chars().next().map_or(0, |c| c.len_utf8());
        (before, &after_and_found[size..], true)
    } else {
        (s, "", false)
    }
}

// Go: internal/jsnum/string.go:trimLeadingZeros
fn trim_leading_zeros(s: &str) -> &str {
    if s.starts_with('0') {
        let trimmed = s.trim_start_matches('0');
        if trimmed.is_empty() {
            return "0";
        }
        return trimmed;
    }
    s
}

// Go: internal/jsnum/string.go:trimTrailingZeros
fn trim_trailing_zeros(s: &str) -> &str {
    if s.ends_with('0') {
        let trimmed = s.trim_end_matches('0');
        if trimmed.is_empty() {
            return "0";
        }
        return trimmed;
    }
    s
}

// Go: internal/jsnum/string.go:stringToFloat64
fn string_to_float64(s: &str) -> f64 {
    // Rust's `parse::<f64>` already returns `±inf` (not an error) for
    // out-of-range magnitudes, matching Go's `strconv.ErrRange` handling.
    s.parse::<f64>().unwrap_or(f64::NAN)
}

// Go: internal/jsnum/string.go:isAllDigits
fn is_all_digits(s: &str) -> bool {
    s.chars().all(tsgo_stringutil::is_digit)
}

// Go: internal/jsnum/string.go:isAllBinaryDigits
fn is_all_binary_digits(s: &str) -> bool {
    s.chars().all(|r| r == '0' || r == '1')
}

// Go: internal/jsnum/string.go:isAllOctalDigits
fn is_all_octal_digits(s: &str) -> bool {
    s.chars().all(tsgo_stringutil::is_octal_digit)
}

// Go: internal/jsnum/string.go:isAllHexDigits
fn is_all_hex_digits(s: &str) -> bool {
    s.chars().all(tsgo_stringutil::is_hex_digit)
}

// Go: internal/jsnum/string.go:isNumberRune
fn is_number_rune(r: char) -> bool {
    tsgo_stringutil::is_digit(r)
        || ('a'..='f').contains(&r)
        || ('A'..='F').contains(&r)
        || matches!(r, '.' | '-' | '+' | 'x' | 'X' | 'o' | 'O')
}

/// Formats the magnitude of a positive, finite, non-zero `f64` exactly as
/// ECMAScript `Number::toString` (base 10) would.
///
/// Obtains the shortest round-tripping digit sequence from the Ryu algorithm
/// (whose round-half-to-even tie-breaking matches Go's `strconv` and the Ryu
/// corpus, unlike `f64`'s `LowerExp`), normalizes Ryu's output into a bare
/// significant-digit string plus a decimal point position, then applies the ES
/// spec's positional vs. exponential layout rules. This matches Go's
/// `json.Marshal(float64)` output byte-for-byte (`1e+308`, `1e+21`,
/// `100000000000000000000`, ...).
fn format_positive_finite(abs: f64) -> String {
    let mut buffer = ryu::Buffer::new();
    let formatted = buffer.format_finite(abs);

    // Split off an optional exponent, then the optional decimal point, and
    // reduce to canonical form: value = digits * 10^(n - k), with `digits`
    // having no leading/trailing zeros and `k = digits.len()`.
    let (mantissa, exp): (&str, i32) = match formatted.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse().expect("ryu exponent is a valid integer")),
        None => (formatted, 0),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));

    let all_digits = format!("{int_part}{frac_part}");
    let mut decimal_exp = exp - frac_part.len() as i32;
    let no_leading = all_digits.trim_start_matches('0');
    let digits = no_leading.trim_end_matches('0');
    decimal_exp += (no_leading.len() - digits.len()) as i32;

    let k = digits.len() as i32;
    let n = decimal_exp + k;

    if k <= n && n <= 21 {
        let mut out = String::with_capacity(n as usize);
        out.push_str(digits);
        out.push_str(&"0".repeat((n - k) as usize));
        out
    } else if 0 < n && n <= 21 {
        let split = n as usize;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{}", "0".repeat((-n) as usize), digits)
    } else {
        let exp_value = n - 1;
        let exp_sign = if exp_value >= 0 { '+' } else { '-' };
        let exp_abs = exp_value.abs();
        if k == 1 {
            format!("{digits}e{exp_sign}{exp_abs}")
        } else {
            format!("{}.{}e{}{}", &digits[..1], &digits[1..], exp_sign, exp_abs)
        }
    }
}

// https://tc39.es/ecma262/2024/multipage/ecmascript-data-types-and-values.html#sec-numeric-types-number-tostring
// Go: internal/jsnum/string.go:(Number).String
impl fmt::Display for Number {
    #[allow(clippy::float_cmp)] // Safe-integer fast path needs exact f64 equality.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.0;
        if n.is_nan() {
            return f.write_str("NaN");
        }
        if n.is_infinite() {
            return f.write_str(if n < 0.0 { "-Infinity" } else { "Infinity" });
        }

        // Fast path: safe integers convert directly to a base-10 integer.
        if (MIN_SAFE_INTEGER.0..=MAX_SAFE_INTEGER.0).contains(&n) {
            let i = n as i64;
            if i as f64 == n {
                return write!(f, "{i}");
            }
        }

        if n.is_sign_negative() {
            f.write_str("-")?;
        }
        f.write_str(&format_positive_finite(n.abs()))
    }
}

#[cfg(test)]
#[allow(clippy::excessive_precision)]
#[path = "string_test.rs"]
mod tests;
