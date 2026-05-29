//! `tsgo_jsnum` — 1:1 Rust port of Go `internal/jsnum`.
//!
//! Provides JavaScript-exact `Number` semantics (a `Number` is a JS double):
//! bitwise operations (`ToInt32`/`ToUint32` plus shifts and the bitwise ops),
//! `Remainder`, `Exponentiate`, `Number.prototype.toString`, `StringToNumber`,
//! and `PseudoBigInt` (bigint literal parsing).

use num_bigint::BigInt;
use num_traits::Pow;

mod pseudobigint;
mod string;

pub use pseudobigint::{parse_pseudo_big_int, parse_valid_big_int, PseudoBigInt};
pub use string::from_string;

/// The largest integer that a JS `Number` can represent without losing
/// precision: `2^53 - 1` (`Number.MAX_SAFE_INTEGER`).
// Go: internal/jsnum/jsnum.go:MaxSafeInteger
pub const MAX_SAFE_INTEGER: Number = Number(9007199254740991.0);

/// The smallest safe integer: `-(2^53 - 1)` (`Number.MIN_SAFE_INTEGER`).
// Go: internal/jsnum/jsnum.go:MinSafeInteger
pub const MIN_SAFE_INTEGER: Number = Number(-9007199254740991.0);

/// A JavaScript-like number, i.e. an IEEE 754 double with JS semantics.
///
/// All operations that can be performed directly on the wrapped `f64`
/// (conversion, arithmetic, comparison) behave as they would in JavaScript;
/// any other operation goes through this type's methods rather than touching
/// the `f64` directly.
///
/// Equality matches JavaScript's `===` on doubles via the wrapped `f64`:
/// `NaN != NaN` and `+0.0 == -0.0`.
///
/// # Examples
/// ```
/// use tsgo_jsnum::Number;
/// assert_eq!(f64::from(Number::from(1.5)), 1.5);
/// assert!(Number::nan() != Number::nan());
/// ```
// Go: internal/jsnum/jsnum.go:Number
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Number(pub(crate) f64);

impl From<f64> for Number {
    fn from(value: f64) -> Self {
        Number(value)
    }
}

impl From<Number> for f64 {
    fn from(value: Number) -> Self {
        value.0
    }
}

impl Number {
    /// Returns a `NaN` value.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert!(Number::nan().is_nan());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:NaN
    pub fn nan() -> Number {
        Number(f64::NAN)
    }

    /// Reports whether this value is `NaN`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert!(Number::nan().is_nan());
    /// assert!(!Number::from(1.0).is_nan());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:(Number).IsNaN
    pub fn is_nan(self) -> bool {
        self.0.is_nan()
    }

    /// Returns positive infinity if `sign >= 0`, otherwise negative infinity.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::inf(1)), f64::INFINITY);
    /// assert_eq!(f64::from(Number::inf(-1)), f64::NEG_INFINITY);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:Inf
    pub fn inf(sign: i32) -> Number {
        Number(if sign >= 0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        })
    }

    /// Reports whether this value is positive or negative infinity.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert!(Number::inf(1).is_inf());
    /// assert!(!Number::from(1.0).is_inf());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:(Number).IsInf
    pub fn is_inf(self) -> bool {
        self.0.is_infinite()
    }

    // https://tc39.es/ecma262/2024/multipage/abstract-operations.html#sec-touint32
    // Go: internal/jsnum/jsnum.go:toUint32
    fn to_uint32(self) -> u32 {
        // The only difference between ToUint32 and ToInt32 is the
        // interpretation of the bits.
        self.to_int32() as u32
    }

    // https://tc39.es/ecma262/2024/multipage/abstract-operations.html#sec-toint32
    // Go: internal/jsnum/jsnum.go:toInt32
    #[allow(clippy::float_cmp)] // ECMAScript ToInt32 requires exact f64 equality.
    fn to_int32(self) -> i32 {
        let x = self.0;

        // Fast path: if the number is in the range (-2^31, 2^32), i.e. an SMI,
        // then we don't need to do any special mapping.
        let smi = x as i32;
        if smi as f64 == x {
            return smi;
        }

        // If number is not finite or number is either +0 or -0, return +0.
        // Zero was covered by the test above.
        if is_non_finite(x) {
            return 0;
        }

        // Let int be truncate(x), then int32bit be int modulo 2**32.
        let x = x.trunc() % 4294967296.0;
        // If int32bit >= 2**31, return int32bit - 2**32; otherwise int32bit.
        x as i64 as i32
    }

    // Go: internal/jsnum/jsnum.go:toShiftCount
    fn to_shift_count(self) -> u32 {
        self.to_uint32() & 31
    }

    /// JavaScript `>>`: arithmetic (sign-propagating) right shift, with the
    /// shift count taken modulo 32.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(-4.0).signed_right_shift(Number::from(1.0))), -2.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:SignedRightShift
    pub fn signed_right_shift(self, y: Number) -> Number {
        Number((self.to_int32() >> y.to_shift_count()) as f64)
    }

    /// JavaScript `>>>`: logical (zero-filling) right shift, with the shift
    /// count taken modulo 32.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(-4.0).unsigned_right_shift(Number::from(0.0))), 4294967292.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:UnsignedRightShift
    pub fn unsigned_right_shift(self, y: Number) -> Number {
        Number((self.to_uint32() >> y.to_shift_count()) as f64)
    }

    /// JavaScript `<<`: left shift, with the shift count taken modulo 32.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(1.0).left_shift(Number::from(31.0))), -2147483648.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:LeftShift
    pub fn left_shift(self, y: Number) -> Number {
        Number((self.to_int32() << y.to_shift_count()) as f64)
    }

    /// JavaScript `~`: bitwise NOT on the `ToInt32` value.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(0.0).bitwise_not()), -1.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:BitwiseNOT
    pub fn bitwise_not(self) -> Number {
        Number((!self.to_int32()) as f64)
    }

    /// JavaScript `|`: bitwise OR on the operands' `ToInt32` values.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(0.0).bitwise_or(Number::from(1.0))), 1.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:BitwiseOR
    pub fn bitwise_or(self, y: Number) -> Number {
        Number((self.to_int32() | y.to_int32()) as f64)
    }

    /// JavaScript `&`: bitwise AND on the operands' `ToInt32` values.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(1.0).bitwise_and(Number::from(1.0))), 1.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:BitwiseAND
    pub fn bitwise_and(self, y: Number) -> Number {
        Number((self.to_int32() & y.to_int32()) as f64)
    }

    /// JavaScript `^`: bitwise XOR on the operands' `ToInt32` values.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(1.0).bitwise_xor(Number::from(1.0))), 0.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:BitwiseXOR
    pub fn bitwise_xor(self, y: Number) -> Number {
        Number((self.to_int32() ^ y.to_int32()) as f64)
    }

    // Go: internal/jsnum/jsnum.go:trunc
    #[allow(dead_code)] // 1:1 port of Go's private trunc; consumed by evaluator in P4.
    fn trunc(self) -> Number {
        Number(self.0.trunc())
    }

    /// JavaScript `Math.floor`: the greatest integer not greater than `self`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(1.9).floor()), 1.0);
    /// assert_eq!(f64::from(Number::from(-1.1).floor()), -2.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:Floor
    pub fn floor(self) -> Number {
        Number(self.0.floor())
    }

    /// Absolute value (`Math.abs`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(-3.0).abs()), 3.0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:Abs
    pub fn abs(self) -> Number {
        Number(self.0.abs())
    }

    /// JavaScript `%`: the IEEE 754 floating-point remainder of `self / d`.
    ///
    /// Uses `fmod` rather than a hand-written formula to avoid accumulating
    /// floating-point error. `NaN` / infinity / zero edge cases follow the
    /// ECMAScript spec.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(10.0).remainder(Number::from(3.0))), 1.0);
    /// assert!(Number::from(1.0).remainder(Number::from(0.0)).is_nan());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:Remainder
    pub fn remainder(self, d: Number) -> Number {
        let n = self;
        if n.is_nan() || d.is_nan() {
            return Number::nan();
        }
        if n.is_inf() {
            return Number::nan();
        }
        if d.is_inf() {
            return n;
        }
        if d == Number(0.0) {
            return Number::nan();
        }
        if n == Number(0.0) {
            return n;
        }
        Number(n.0 % d.0)
    }

    /// JavaScript `**`: exponentiation matching ECMAScript's
    /// `Number::exponentiate`.
    ///
    /// `base == ±1` with an infinite exponent (and `1 ** NaN`) yields `NaN`.
    /// For integer `base ** integer exponent` whose result exceeds 53 bits,
    /// exact big-integer arithmetic plus IEEE 754 round-to-nearest-even is used
    /// (so the result stays within 1 ULP of other JS engines); everything else
    /// goes through `f64::powf`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_jsnum::Number;
    /// assert_eq!(f64::from(Number::from(2.0).exponentiate(Number::from(3.0))), 8.0);
    /// assert!(Number::from(1.0).exponentiate(Number::inf(1)).is_nan());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/jsnum/jsnum.go:Exponentiate
    #[allow(clippy::float_cmp)] // ECMAScript special-cases require exact f64 equality.
    pub fn exponentiate(self, exponent: Number) -> Number {
        let base = self;
        if (base == Number(1.0) || base == Number(-1.0)) && exponent.is_inf() {
            return Number::nan();
        }
        if base == Number(1.0) && exponent.is_nan() {
            return Number::nan();
        }

        let b = base.0;
        let e = exponent.0;

        // For integer base ** integer exponent where the result exceeds 53
        // bits, f64::powf can be off by multiple ULPs versus JS engines. Use
        // exact big.Int arithmetic and IEEE 754 round-to-nearest-even instead.
        // The ES spec (6.1.6.1.3) says exponentiate returns an
        // "implementation-approximated" value, so engines may differ; this is
        // always within 1 ULP of the result.
        let b_is_integral = (i64::MIN as f64..=i64::MAX as f64).contains(&b) && b == b.trunc();
        let e_is_integral = (0.0..=i64::MAX as f64).contains(&e) && e == e.trunc() && e.is_finite();
        if b_is_integral && e_is_integral {
            let magnitude = e * b.abs().log2();
            if magnitude > 53.0 && magnitude <= f64::MAX.log2() {
                let ri = BigInt::from(b as i64).pow(e as u32);
                return Number(bigint_to_f64(&ri));
            }
        }

        Number(b.powf(e))
    }
}

/// Reports whether `x` is `NaN` or infinite via a single exponent-mask test.
// Go: internal/jsnum/jsnum.go:isNonFinite
fn is_non_finite(x: f64) -> bool {
    // Equivalent to `x.is_nan() || x.is_infinite()` in one operation.
    const MASK: u64 = 0x7FF0000000000000;
    x.to_bits() & MASK == MASK
}

/// Converts a big integer to the nearest `f64` (round-to-nearest, ties to even).
///
/// Routes through the decimal string so the rounding is the IEEE 754
/// correctly-rounded result, matching Go's `big.Int.Float64`.
pub(crate) fn bigint_to_f64(bi: &BigInt) -> f64 {
    bi.to_string().parse::<f64>().unwrap_or(f64::INFINITY)
}

#[cfg(test)]
#[allow(clippy::excessive_precision)]
#[path = "lib_test.rs"]
mod tests;
