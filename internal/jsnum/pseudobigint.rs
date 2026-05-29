//! `PseudoBigInt` (JS bigint literal parsing), ported 1:1 from Go
//! `internal/jsnum/pseudobigint.go`.

use std::fmt;

use num_bigint::BigInt;

/// A JavaScript-like bigint, stored as a sign plus the absolute value in
/// base 10. The default/zero state represents the value `0`.
// Go: internal/jsnum/pseudobigint.go:PseudoBigInt
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PseudoBigInt {
    /// `true` iff the value is a non-zero negative number.
    pub negative: bool,
    /// The absolute value in base 10 with no leading zeros. The value zero is
    /// represented as an empty string.
    pub base10_value: String,
}

impl PseudoBigInt {
    /// Builds a `PseudoBigInt` from a base-10 magnitude string and a sign,
    /// stripping leading zeros (the sign is dropped for the value zero).
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::PseudoBigInt;
    /// assert_eq!(PseudoBigInt::new("007", true).to_string(), "-7");
    /// assert_eq!(PseudoBigInt::new("0", true).to_string(), "0");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/pseudobigint.go:NewPseudoBigInt
    pub fn new(value: &str, negative: bool) -> PseudoBigInt {
        let value = value.trim_start_matches('0');
        PseudoBigInt {
            negative: negative && !value.is_empty(),
            base10_value: value.to_string(),
        }
    }

    /// Returns the sign of the value: `-1`, `0`, or `1`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::PseudoBigInt;
    /// assert_eq!(PseudoBigInt::new("5", true).sign(), -1);
    /// assert_eq!(PseudoBigInt::new("0", false).sign(), 0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/pseudobigint.go:Sign
    pub fn sign(&self) -> i32 {
        if self.base10_value.is_empty() {
            0
        } else if self.negative {
            -1
        } else {
            1
        }
    }
}

// Go: internal/jsnum/pseudobigint.go:String
impl fmt::Display for PseudoBigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.base10_value.is_empty() {
            return f.write_str("0");
        }
        if self.negative {
            f.write_str("-")?;
        }
        f.write_str(&self.base10_value)
    }
}

/// Parses a validated bigint literal `text` (an optional leading `-` followed
/// by a digit literal) into a [`PseudoBigInt`].
///
/// # Examples
/// ```
/// use tsgo_jsnum::parse_valid_big_int;
/// assert_eq!(parse_valid_big_int("0x1Fn").to_string(), "31");
/// assert_eq!(parse_valid_big_int("-123n").to_string(), "-123");
/// ```
///
/// Side effects: none (pure).
// Go: internal/jsnum/pseudobigint.go:ParseValidBigInt
pub fn parse_valid_big_int(text: &str) -> PseudoBigInt {
    let (text, negative) = match text.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (text, false),
    };
    PseudoBigInt::new(&parse_pseudo_big_int(text), negative)
}

/// Parses the magnitude of a bigint literal (decimal or `0b`/`0o`/`0x`
/// prefixed, possibly with `_` separators and a trailing `n`) into its base-10
/// string form with leading zeros stripped.
///
/// # Examples
/// ```
/// use tsgo_jsnum::parse_pseudo_big_int;
/// assert_eq!(parse_pseudo_big_int("0xFFn"), "255");
/// assert_eq!(parse_pseudo_big_int("0010n"), "10");
/// ```
///
/// # Panics
/// Panics if a non-decimal literal cannot be parsed as a big integer.
///
/// Side effects: none (pure).
// Go: internal/jsnum/pseudobigint.go:ParsePseudoBigInt
pub fn parse_pseudo_big_int(string_value: &str) -> String {
    let string_value = string_value.strip_suffix('n').unwrap_or(string_value);
    let b1 = string_value.as_bytes().get(1).copied().unwrap_or(0);
    let radix = match b1 {
        b'b' | b'B' => 2,
        b'o' | b'O' => 8,
        b'x' | b'X' => 16,
        // Decimal.
        _ => {
            let trimmed = string_value.trim_start_matches('0');
            return if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            };
        }
    };

    // Strip the `0b`/`0o`/`0x` prefix and any `_` digit separators, then parse.
    let digits: String = string_value[2..].chars().filter(|&c| c != '_').collect();
    match BigInt::parse_bytes(digits.as_bytes(), radix) {
        Some(bi) => bi.to_string(),
        None => panic!("Failed to parse big int: {string_value:?}"),
    }
}

#[cfg(test)]
#[path = "pseudobigint_test.rs"]
mod tests;
