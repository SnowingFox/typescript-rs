//! A minimal slice of Go's `TypeFacts` lattice.
//!
//! Go models dozens of facts (typeof results, equality with each unit type, …)
//! as a `uint32` bitset; 4g ports just the truthiness facts so the 4f
//! truthiness/equality narrowing can drop falsy literal subtypes (`""`/`0`).
//!
//! DEFER(phase-4-checker-4h+): the full fact lattice (typeof EQ/NE per name,
//! EQ/NE undefined/null, discriminant facts) and `getTypeFacts` over apparent
//! types/intersections. blocked-by: lib globals (P6) + apparent-type wrappers.

use super::types::{LiteralValue, TypeFlags, TypeId};
use super::Checker;

bitflags::bitflags! {
    /// The truthiness facts a type can carry (4g subset of Go's `TypeFacts`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::TypeFacts;
    /// assert!(TypeFacts::TRUTHY.intersects(TypeFacts::TRUTHY | TypeFacts::FALSY));
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/checker/utilities.go:TypeFacts
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct TypeFacts: u32 {
        /// The type can be truthy.
        const TRUTHY = 1 << 0;
        /// The type can be falsy.
        const FALSY = 1 << 1;
    }
}

impl Checker {
    /// Returns the truthiness facts of a single (non-union) type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeFacts};
    /// let c = Checker::new();
    /// assert_eq!(c.get_type_facts(c.undefined_type()), TypeFacts::FALSY);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/utilities.go:Checker.getTypeFacts (truthiness subset)
    pub fn get_type_facts(&self, t: TypeId) -> TypeFacts {
        let ty = self.get_type(t);
        let flags = ty.flags();
        if flags.intersects(TypeFlags::UNDEFINED | TypeFlags::NULL | TypeFlags::VOID) {
            return TypeFacts::FALSY;
        }
        if flags.intersects(TypeFlags::STRING | TypeFlags::NUMBER | TypeFlags::BIG_INT) {
            return TypeFacts::TRUTHY | TypeFacts::FALSY;
        }
        match ty.literal_value() {
            Some(LiteralValue::Boolean(false)) => TypeFacts::FALSY,
            Some(LiteralValue::Boolean(true)) => TypeFacts::TRUTHY,
            Some(LiteralValue::String(s)) => {
                if s.is_empty() {
                    TypeFacts::FALSY
                } else {
                    TypeFacts::TRUTHY
                }
            }
            Some(LiteralValue::Number(n)) => {
                if f64::from(*n) == 0.0 {
                    TypeFacts::FALSY
                } else {
                    TypeFacts::TRUTHY
                }
            }
            // Objects and anything else are truthy.
            // DEFER(phase-4-checker-4h+): `0n` bigint-literal falsiness.
            None => TypeFacts::TRUTHY,
        }
    }

    /// Reports whether any constituent of `t` can carry `facts`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeFacts};
    /// let c = Checker::new();
    /// assert!(c.has_type_facts(c.string_type(), TypeFacts::TRUTHY));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/utilities.go:Checker.hasTypeFacts
    pub fn has_type_facts(&self, t: TypeId, facts: TypeFacts) -> bool {
        self.distributed_types(t)
            .iter()
            .any(|&m| self.get_type_facts(m).intersects(facts))
    }

    /// Narrows `t` to the constituents that can carry `facts`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeFacts};
    /// let mut c = Checker::new();
    /// let n = c.number_type();
    /// assert_eq!(c.get_type_with_facts(n, TypeFacts::TRUTHY), n);
    /// ```
    ///
    /// Side effects: may allocate a union.
    // Go: internal/checker/utilities.go:Checker.getTypeWithFacts
    pub fn get_type_with_facts(&mut self, t: TypeId, facts: TypeFacts) -> TypeId {
        let kept: Vec<TypeId> = self
            .distributed_types(t)
            .into_iter()
            .filter(|&m| self.get_type_facts(m).intersects(facts))
            .collect();
        self.get_union_type(&kept)
    }

    /// Returns the non-nullable form of `t`: the union constituents that are not
    /// `null`/`undefined`/`void`.
    ///
    /// This is the reachable subset of Go's `GetNonNullableType` →
    /// `getAdjustedTypeWithFacts(t, NEUndefinedOrNull)`: its core is the
    /// constituent filter `getTypeWithFacts(t, NEUndefinedOrNull)`, which keeps
    /// the members carrying the `NEUndefinedOrNull` fact and drops the
    /// `undefined`/`null`/`void` ones (`undefined`/`void`/`null` are the only
    /// reachable kinds lacking that fact). `string | undefined` reduces to
    /// `string`, and a lone `undefined` to `never` (the empty union).
    ///
    /// DEFER(phase-4-checker-4az+): the `unknown` → `{} | null | undefined`
    /// recombination (`recombineUnknownType`/`unknownUnionType`) and the
    /// `mapType` over remaining instantiable `EQUndefinedOrNull` constituents to
    /// `getGlobalNonNullableTypeInstantiation` (intersect with `{}` / apply the
    /// `NonNullable<T>` global alias). blocked-by: `unknownUnionType` + the `{}`
    /// empty-object / `NonNullable<T>` global alias (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.GetNonNullableType / getAdjustedTypeWithFacts (NEUndefinedOrNull, reachable subset)
    pub(crate) fn get_non_null_type(&mut self, t: TypeId) -> TypeId {
        // Go: `GetNonNullableType` is the identity outside strictNullChecks (a
        // non-strict union never carries `null`/`undefined`).
        if !self.strict_null_checks() {
            return t;
        }
        let kept: Vec<TypeId> = self
            .distributed_types(t)
            .into_iter()
            .filter(|&m| {
                !self
                    .get_type(m)
                    .flags()
                    .intersects(TypeFlags::NULLABLE | TypeFlags::VOID)
            })
            .collect();
        self.get_union_type(&kept)
    }
}

#[cfg(test)]
#[path = "type_facts_test.rs"]
mod tests;
