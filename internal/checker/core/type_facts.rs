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
    /// The reachable subset of Go's `TypeFacts` lattice (4az): the
    /// truthiness facts plus the `EQ`/`NE`/`Is` `undefined`/`null` facts that
    /// drive nullable equality narrowing and the possibly-`null`/`undefined`
    /// diagnostics. The bit positions mirror Go's `TypeFacts` constants exactly
    /// (`checker.go`), so they can grow toward the full lattice without
    /// renumbering; the `typeof`-result bits (`1 << 0 ..= 1 << 15`) are not yet
    /// modeled (typeof narrowing uses the relation engine, not facts).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::TypeFacts;
    /// assert!(TypeFacts::TRUTHY.intersects(TypeFacts::TRUTHY | TypeFacts::FALSY));
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/checker/checker.go:TypeFacts
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct TypeFacts: u32 {
        /// The type can be `=== undefined` (`x === undefined` can hold).
        const EQ_UNDEFINED = 1 << 16;
        /// The type can be `=== null`.
        const EQ_NULL = 1 << 17;
        /// The type can be `== undefined`/`== null` (loose nullish).
        const EQ_UNDEFINED_OR_NULL = 1 << 18;
        /// The type can be `!== undefined`.
        const NE_UNDEFINED = 1 << 19;
        /// The type can be `!== null`.
        const NE_NULL = 1 << 20;
        /// The type can be `!= undefined`/`!= null` (loose non-nullish).
        const NE_UNDEFINED_OR_NULL = 1 << 21;
        /// The type can be truthy.
        const TRUTHY = 1 << 22;
        /// The type can be falsy.
        const FALSY = 1 << 23;
        /// The type *is* `undefined` (drives the possibly-undefined diagnostic).
        const IS_UNDEFINED = 1 << 24;
        /// The type *is* `null` (drives the possibly-null diagnostic).
        const IS_NULL = 1 << 25;
        /// The type is `undefined` or `null`.
        const IS_UNDEFINED_OR_NULL = Self::IS_UNDEFINED.bits() | Self::IS_NULL.bits();
    }
}

// The reachable per-kind fact groups (Go's `TypeFacts*Facts` constants, reduced
// to the EQ/NE/Is/Truthy/Falsy subset this port models). The `Base*` nullable
// part of every non-nullable kind is `NE_UNDEFINED | NE_NULL |
// NE_UNDEFINED_OR_NULL` under strictNullChecks; the non-strict form additionally
// carries the `EQ_*` (everything is potentially `undefined`/`null`) and `FALSY`.
// Go: internal/checker/checker.go (the `TypeFacts*Facts` group constants)
const BASE_STRICT: TypeFacts = TypeFacts::NE_UNDEFINED
    .union(TypeFacts::NE_NULL)
    .union(TypeFacts::NE_UNDEFINED_OR_NULL);
const BASE_NONSTRICT: TypeFacts = BASE_STRICT
    .union(TypeFacts::EQ_UNDEFINED)
    .union(TypeFacts::EQ_NULL)
    .union(TypeFacts::EQ_UNDEFINED_OR_NULL)
    .union(TypeFacts::FALSY);
// `undefined`/`null`/`void` carry mode-independent facts (Go's `*Facts` for them
// have no strict variant).
const UNDEFINED_FACTS: TypeFacts = TypeFacts::EQ_UNDEFINED
    .union(TypeFacts::EQ_UNDEFINED_OR_NULL)
    .union(TypeFacts::NE_NULL)
    .union(TypeFacts::FALSY)
    .union(TypeFacts::IS_UNDEFINED);
const NULL_FACTS: TypeFacts = TypeFacts::EQ_NULL
    .union(TypeFacts::EQ_UNDEFINED_OR_NULL)
    .union(TypeFacts::NE_UNDEFINED)
    .union(TypeFacts::FALSY)
    .union(TypeFacts::IS_NULL);
const VOID_FACTS: TypeFacts = TypeFacts::EQ_UNDEFINED
    .union(TypeFacts::EQ_UNDEFINED_OR_NULL)
    .union(TypeFacts::NE_NULL)
    .union(TypeFacts::FALSY);
// The fallback for kinds this port does not model precisely (`any`/`unknown`/
// `error`): everything except the `Is*` bits, mirroring Go's `UnknownFacts`
// (`All & ^IsUndefinedOrNull`) so an `any` object never trips the
// possibly-`null`/`undefined` diagnostic while still reading as truthy/falsy.
const UNKNOWN_FACTS: TypeFacts = BASE_NONSTRICT.union(TypeFacts::TRUTHY);

// Returns the strict/non-strict fact group for a non-nullable kind with the
// given truthiness facts (`truthy`/`falsy`).
fn primitive_facts(strict: bool, truthy: bool, falsy: bool) -> TypeFacts {
    let mut facts = if strict { BASE_STRICT } else { BASE_NONSTRICT };
    if truthy {
        facts |= TypeFacts::TRUTHY;
    }
    if falsy {
        facts |= TypeFacts::FALSY;
    }
    facts
}

impl Checker {
    /// Returns the facts of `t` (the reachable subset of Go's
    /// `getTypeFactsWorker`): the truthiness facts plus the `EQ`/`NE`/`Is`
    /// `undefined`/`null` facts. A union OR-folds its members' facts (Go's
    /// `case flags&Union`); the `EQ_*`/`FALSY` facts of a non-nullable kind are
    /// mode-dependent (strictNullChecks), while `undefined`/`null`/`void` carry
    /// mode-independent facts.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeFacts};
    /// let c = Checker::new();
    /// // `undefined` is falsy and *is* undefined (no `NE_UNDEFINED`).
    /// let f = c.get_type_facts(c.undefined_type());
    /// assert!(f.contains(TypeFacts::FALSY | TypeFacts::IS_UNDEFINED));
    /// assert!(!f.intersects(TypeFacts::NE_UNDEFINED));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.getTypeFactsWorker (EQ/NE/Is/Truthy/Falsy subset)
    pub fn get_type_facts(&self, t: TypeId) -> TypeFacts {
        let ty = self.get_type(t);
        let flags = ty.flags();
        let strict = self.strict_null_checks();
        // A union is the OR of its members' facts (Go's union case).
        if flags.contains(TypeFlags::UNION) {
            return ty
                .union_types()
                .unwrap_or(&[])
                .iter()
                .fold(TypeFacts::empty(), |acc, &m| acc | self.get_type_facts(m));
        }
        if flags.intersects(TypeFlags::STRING | TypeFlags::STRING_MAPPING) {
            return primitive_facts(strict, true, true);
        }
        if flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::TEMPLATE_LITERAL) {
            let is_empty =
                matches!(ty.literal_value(), Some(LiteralValue::String(s)) if s.is_empty());
            return primitive_facts(strict, !is_empty, is_empty);
        }
        if flags.intersects(TypeFlags::NUMBER) {
            return primitive_facts(strict, true, true);
        }
        if flags.contains(TypeFlags::NUMBER_LITERAL) {
            let is_zero =
                matches!(ty.literal_value(), Some(LiteralValue::Number(n)) if f64::from(*n) == 0.0);
            return primitive_facts(strict, !is_zero, is_zero);
        }
        if flags.intersects(TypeFlags::BIG_INT | TypeFlags::BIG_INT_LITERAL) {
            // DEFER(phase-4-checker-4az+): `0n` bigint-literal falsiness; treat
            // bigint as both truthy/falsy (the `Base*Facts` form).
            return primitive_facts(strict, true, true);
        }
        if flags.contains(TypeFlags::BOOLEAN) {
            return primitive_facts(strict, true, true);
        }
        if flags.contains(TypeFlags::BOOLEAN_LITERAL) {
            let is_false = matches!(ty.literal_value(), Some(LiteralValue::Boolean(false)));
            return primitive_facts(strict, !is_false, is_false);
        }
        if flags.contains(TypeFlags::OBJECT) {
            // Objects are truthy; the precise empty-object/function refinement
            // (Go's `getTypeFactsWorker` object branch) is not needed for the
            // reachable EQ/NE/Is/Truthy subset.
            return primitive_facts(strict, true, false);
        }
        // A `symbol`/`unique symbol` value is always truthy and (strict)
        // non-nullable (Go's `SymbolStrictFacts`/`SymbolFacts`). Same shape as
        // an object: truthy, never falsy, no `Is*`.
        if flags.intersects(TypeFlags::ES_SYMBOL_LIKE) {
            return primitive_facts(strict, true, false);
        }
        // The non-primitive `object` type behaves like an object value (Go's
        // object branch maps `NonPrimitive` to `ObjectStrictFacts`/`ObjectFacts`).
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return primitive_facts(strict, true, false);
        }
        if flags.intersects(TypeFlags::VOID) {
            return VOID_FACTS;
        }
        if flags.intersects(TypeFlags::UNDEFINED) {
            return UNDEFINED_FACTS;
        }
        if flags.intersects(TypeFlags::NULL) {
            return NULL_FACTS;
        }
        if flags.contains(TypeFlags::NEVER) {
            return TypeFacts::empty();
        }
        // `any`/`unknown`/`error` and any unmodeled kind: Go's `UnknownFacts`.
        UNKNOWN_FACTS
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
    /// Adds `undefined` to `t` when `is_optional` and strictNullChecks is on
    /// (Go's `addOptionalityEx`); otherwise returns `t` unchanged.
    ///
    /// This is what injects `| undefined` into the read type of an optional
    /// property (`{ a?: number }` → `o.a` is `number | undefined`) and an
    /// optional parameter (`function f(x?: number)` → `x` is `number |
    /// undefined`). Outside strictNullChecks (or for a non-optional
    /// declaration) it is the identity.
    ///
    /// Side effects: may allocate a union.
    // Go: internal/checker/checker.go:Checker.addOptionalityEx
    pub(crate) fn add_optionality_ex(
        &mut self,
        t: TypeId,
        is_property: bool,
        is_optional: bool,
    ) -> TypeId {
        if self.strict_null_checks() && is_optional {
            return self.get_optional_type(t, is_property);
        }
        t
    }

    /// Returns `t | undefined`, the optional form of `t` (Go's `getOptionalType`).
    ///
    /// `is_property` selects `undefinedOrMissingType` vs `undefinedType`; with
    /// `exactOptionalPropertyTypes` deferred, both are the plain `undefined`
    /// type, so the result is `t | undefined` (already-optional `t` is returned
    /// unchanged). Callers gate this on strictNullChecks via
    /// [`add_optionality_ex`](Self::add_optionality_ex).
    ///
    /// Side effects: may allocate a union.
    // Go: internal/checker/checker.go:Checker.getOptionalType
    pub(crate) fn get_optional_type(&mut self, t: TypeId, _is_property: bool) -> TypeId {
        // DEFER(phase-4-checker-C-D2): `exactOptionalPropertyTypes` makes the
        // property form use `missingType` rather than `undefinedType`; the base
        // behavior uses `undefinedType` for both. blocked-by: `missingType` +
        // `exactOptionalPropertyTypes` option consumer.
        let missing_or_undefined = self.undefined_type();
        if t == missing_or_undefined {
            return t;
        }
        // A union already led by `undefined`/missing is already optional.
        if self
            .get_type(t)
            .union_types()
            .and_then(|m| m.first().copied())
            == Some(missing_or_undefined)
        {
            return t;
        }
        self.get_union_type(&[t, missing_or_undefined])
    }

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
