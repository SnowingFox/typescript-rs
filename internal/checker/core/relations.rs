//! Type relations: identity, subtype, assignability, and comparability.
//!
//! Ports the reachable core of Go's `relater.go`: `isTypeRelatedTo` +
//! `isSimpleTypeRelatedTo` (the primitive/literal/any/unknown/never rules) and a
//! structural `checkTypeRelatedTo` (union source/target rules + object property
//! comparison) with a per-relation result cache.
//!
//! 4d covers hand-buildable shapes; the full relater (variance, intersections,
//! signatures/index-signature comparison, conditional/mapped types, detailed
//! error reporting, and the `Ternary` recursion machinery) is deferred.

use rustc_hash::{FxHashMap, FxHashSet};
use tsgo_ast::{Kind, SymbolFlags, SymbolId};
use tsgo_diagnostics::{Category, Message};

use super::check::DiagnosticMessageChain;
use super::declared_types::{
    get_apparent_type, get_applicable_index_info_for_name, get_index_type_of_type,
    get_properties_of_type, get_property_of_type, get_property_type_for_index_type,
    get_type_of_property_of_type, get_type_of_symbol,
};
use super::nodebuilder::type_to_string;
use super::program::BoundProgram;
use super::signatures::{SignatureFlags, SignatureId};
use super::types::{ObjectFlags, TypeData, TypeFlags, TypeId};
use super::Checker;

/// Which relation is being checked (Go's distinct `*Relation` singletons).
///
/// # Examples
/// ```
/// use tsgo_checker::RelationKind;
/// assert_ne!(RelationKind::Identity, RelationKind::Assignable);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.{identity,subtype,strictSubtype,assignable,comparable}Relation
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RelationKind {
    /// Mutual structural identity.
    Identity,
    /// Subtype (used for inference / best-common-type).
    Subtype,
    /// Subtype that treats `any` more strictly.
    StrictSubtype,
    /// Assignability (the everyday `=`/argument relation).
    Assignable,
    /// Comparability (used by `===`/`switch`).
    Comparable,
}

// Which kind of signature list to compare (Go's `SignatureKind`), selecting
// `call_signatures` vs `construct_signatures` on an object type.
// Go: internal/checker/types.go:SignatureKind
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SignatureKind {
    // Call signatures (`(x): T`).
    Call,
    // Construct signatures (`new (x): T`).
    Construct,
}

/// A per-relation cache of comparison results, keyed by `(kind, source, target)`.
///
/// Replaces Go's per-`*Relation` `results map[CacheHashKey]RelationComparisonResult`.
///
/// # Examples
/// ```
/// use tsgo_checker::{RelationKind, TypeId};
/// // The cache is internal to the checker; this just shows the key shape.
/// let key = (RelationKind::Assignable, TypeId(1), TypeId(2));
/// assert_eq!(key.0, RelationKind::Assignable);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/relater.go:Relation
#[derive(Clone, Debug, Default)]
pub struct RelationCache {
    results: FxHashMap<(RelationKind, TypeId, TypeId), bool>,
}

impl RelationCache {
    /// Returns the cached result for a comparison, if present.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/relater.go:Relation.get
    pub fn get(&self, kind: RelationKind, source: TypeId, target: TypeId) -> Option<bool> {
        self.results.get(&(kind, source, target)).copied()
    }

    /// Records a comparison result.
    ///
    /// Side effects: mutates the cache.
    // Go: internal/checker/relater.go:Relation.set
    pub fn set(&mut self, kind: RelationKind, source: TypeId, target: TypeId, result: bool) {
        self.results.insert((kind, source, target), result);
    }

    /// Returns the number of cached comparisons.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/relater.go:Relation.size
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Reports whether the cache is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

// One node of the relation engine's error chain, built head-to-leaf while a
// reporting relation check unwinds (Go's `ErrorChain`: a singly-linked list of
// `{message, args, next}` where the head is the outermost message and `next`
// descends toward the leaf).
// Go: internal/checker/relater.go:ErrorChain
struct ErrorChain {
    message: &'static Message,
    args: Vec<String>,
    next: Option<Box<ErrorChain>>,
}

// The reporting accumulator threaded through the reporting twin of the relation
// recursion, mirroring the `errorChain` field of Go's pooled `Relater`. The
// port has no `Relater` struct (the bool path inlines onto `Checker`), so this
// carries the transient reporting state instead.
// Go: internal/checker/relater.go:Relater (errorChain field)
#[derive(Default)]
struct ChainReporter {
    // The error chain built so far (head first), or `None` when empty.
    chain: Option<Box<ErrorChain>>,
    // Cycle guard for the reporting recursion: a (source, target) pair already
    // being elaborated is treated as related so co-recursive structural types
    // terminate. Stands in for Go's maybe-stack / relation-count machinery.
    // DEFER(phase-4-checker-4bo+): the full `maybeKeys`/recursion-depth model.
    in_progress: FxHashSet<(TypeId, TypeId)>,
}

impl ChainReporter {
    // Returns the message of the chain entry `index` steps from the head (Go's
    // `Relater.getChainMessage`), or `None` past the end.
    // Go: internal/checker/relater.go:Relater.getChainMessage
    fn chain_message(&self, index: usize) -> Option<&'static Message> {
        let mut entry = self.chain.as_deref();
        let mut index = index;
        loop {
            let e = entry?;
            if index == 0 {
                return Some(e.message);
            }
            entry = e.next.as_deref();
            index -= 1;
        }
    }

    // Returns whether the head entry's args match `args` positionally, where a
    // `None` acts as a wildcard (Go's `Relater.chainArgsMatch`).
    // Go: internal/checker/relater.go:Relater.chainArgsMatch
    fn chain_args_match(&self, args: &[Option<&str>]) -> bool {
        let Some(head) = self.chain.as_deref() else {
            return false;
        };
        for (i, a) in args.iter().enumerate() {
            if let Some(expected) = a {
                if head.args.get(i).map(String::as_str) != Some(*expected) {
                    return false;
                }
            }
        }
        true
    }

    // Prepends `message`/`args` onto the chain, collapsing a nested
    // "Types of property 'x' are incompatible." over another property-incompat
    // message into a single dotted "The types of 'x.y' are incompatible between
    // these types." (Go's `Relater.reportError`).
    //
    // DEFER(phase-4-checker-4bo+): the excess-property suppression and the
    // signature return-type collapse arms (`Call_signature_return_types_...`).
    // blocked-by: excess-property reporting + signature relation elaboration.
    // Go: internal/checker/relater.go:Relater.reportError
    fn report(&mut self, message: &'static Message, args: Vec<String>) {
        if message.code() == TYPES_OF_PROPERTY_CODE {
            // Transform a property incompatibility for 'x' followed by another
            // property incompatibility for 'y' into a single message for 'x.y'.
            if matches!(
                self.chain_message(1).map(Message::code),
                Some(TYPES_OF_PROPERTY_CODE)
                    | Some(THE_TYPES_OF_CODE)
                    | Some(THE_TYPES_RETURNED_BY_CODE)
            ) {
                let head = get_property_name_arg(&args[0]);
                // The chain has at least two entries (chain_message(1) matched).
                let next = self.chain.as_deref().expect("head").next.as_deref();
                let tail = get_property_name_arg(&next.expect("next").args[0]);
                let dotted = add_to_dotted_name(&head, &tail);
                // Drop the head (mid message) and the next (the inner property
                // message) before re-reporting the merged dotted message.
                let head_box = self.chain.take().expect("head");
                self.chain = head_box.next.and_then(|n| n.next);
                self.report(message_the_types_of_are_incompatible(), vec![dotted]);
                return;
            }
        }
        self.chain = Some(Box::new(ErrorChain {
            message,
            args,
            next: self.chain.take(),
        }));
    }
}

// The materialized head of a relation error: the head diagnostic's
// code/category/text plus its nested elaboration chain. The reporting recursion
// builds an [`ErrorChain`]; this is the consumer-facing shape the checker hangs
// on a [`Diagnostic`](super::check::Diagnostic) at the error node.
// Go: internal/checker/relater.go:createDiagnosticChainFromErrorChain (result shape)
pub(crate) struct RelationErrorReport {
    /// The head diagnostic's numeric code (`2322`, or `2741`/`2739` when the
    /// head was suppressed and a missing-property message became the head).
    pub(crate) code: i32,
    /// The head diagnostic's category.
    pub(crate) category: Category,
    /// The head diagnostic's localized, argument-substituted text.
    pub(crate) message: String,
    /// The nested elaboration hung under the head.
    pub(crate) message_chain: Vec<DiagnosticMessageChain>,
}

// The diagnostic code of `Types_of_property_0_are_incompatible` (`2326`).
const TYPES_OF_PROPERTY_CODE: i32 = 2326;
// The diagnostic code of `The_types_of_0_are_incompatible_between_these_types` (`2200`).
const THE_TYPES_OF_CODE: i32 = 2200;
// The diagnostic code of `The_types_returned_by_0_are_incompatible_between_these_types` (`2201`).
const THE_TYPES_RETURNED_BY_CODE: i32 = 2201;

// Returns the `The_types_of_0_are_incompatible_between_these_types` message.
fn message_the_types_of_are_incompatible() -> &'static Message {
    &tsgo_diagnostics::THE_TYPES_OF_0_ARE_INCOMPATIBLE_BETWEEN_THESE_TYPES
}

// Wraps a quoted property name in brackets for a dotted-name segment (Go's
// `getPropertyNameArg`): a name beginning with a quote/backtick becomes
// `["..."]`; an identifier name is returned unchanged.
// Go: internal/checker/relater.go:getPropertyNameArg
fn get_property_name_arg(arg: &str) -> String {
    let first = arg.as_bytes().first().copied();
    if matches!(first, Some(b'"') | Some(b'\'') | Some(b'`')) {
        format!("[{arg}]")
    } else {
        arg.to_string()
    }
}

// Joins a property name `head` onto a (possibly already dotted/indexed) `tail`
// into one accessor path (Go's `addToDottedName`): `a` + `b` => `a.b`,
// `a` + `[k]` => `a[k]`, with `new ...` heads parenthesized.
// Go: internal/checker/relater.go:addToDottedName
fn add_to_dotted_name(head: &str, tail: &str) -> String {
    let head = if head.starts_with("new ") {
        format!("({head})")
    } else {
        head.to_string()
    };
    let mut pos = 0;
    loop {
        if tail[pos..].starts_with('(') {
            pos += 1;
        } else if tail[pos..].starts_with("new ") {
            pos += 4;
        } else {
            break;
        }
    }
    let prefix = &tail[..pos];
    let suffix = &tail[pos..];
    if suffix.starts_with('[') {
        format!("{prefix}{head}{suffix}")
    } else {
        format!("{prefix}{head}.{suffix}")
    }
}

// Materializes an [`ErrorChain`] (head-first) into a [`RelationErrorReport`]:
// the head becomes the diagnostic head, every subsequent entry becomes a nested
// `DiagnosticMessageChain` (Go's `createDiagnosticChainFromErrorChain`, which
// builds nested `*Diagnostic`s under the head).
// Go: internal/checker/relater.go:createDiagnosticChainFromErrorChain
fn error_chain_to_report(chain: ErrorChain) -> RelationErrorReport {
    // Skip leading `elidedInCompatibilityPyramid` markers (the signature
    // return-type messages 2202..=2205), mirroring Go's
    // `createDiagnosticChainFromErrorChain`. The head is never a marker in
    // practice; the loop is defensive.
    let mut head = chain;
    while head.message.elided_in_compatibility_pyramid() {
        match head.next {
            Some(next) => head = *next,
            None => break,
        }
    }
    RelationErrorReport {
        code: head.message.code(),
        category: head.message.category(),
        message: localize_chain_entry(head.message, &head.args),
        message_chain: error_chain_node_to_message_chain(head.next)
            .into_iter()
            .collect(),
    }
}

// Materializes the remaining [`ErrorChain`] (head-first) into a
// [`DiagnosticMessageChain`], skipping `elidedInCompatibilityPyramid` marker
// entries anywhere in the list (Go's `createDiagnosticChainFromErrorChain`
// elides them and recurses on `next`).
fn error_chain_node_to_message_chain(
    mut chain: Option<Box<ErrorChain>>,
) -> Option<DiagnosticMessageChain> {
    while let Some(node) = chain {
        if node.message.elided_in_compatibility_pyramid() {
            chain = node.next;
            continue;
        }
        return Some(DiagnosticMessageChain {
            code: node.message.code(),
            category: node.message.category(),
            message: localize_chain_entry(node.message, &node.args),
            next: error_chain_node_to_message_chain(node.next)
                .into_iter()
                .collect(),
        });
    }
    None
}

// Substitutes `args` into `message`'s template text (Go's
// `diagnostics.Localize` for the default locale, as used by `Diagnostic`).
fn localize_chain_entry(message: &Message, args: &[String]) -> String {
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tsgo_diagnostics::format(&message.to_string(), &arg_refs)
}

impl Checker {
    /// Reports whether `source` is assignable to `target`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// use tsgo_checker::{BoundProgram, RelationKind};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let _ = c.is_type_assignable_to(p, c.string_type(), c.string_type());
    /// # }
    /// ```
    ///
    /// Side effects: populates the relation cache; may build property types.
    // Go: internal/checker/relater.go:Checker.isTypeAssignableTo
    pub fn is_type_assignable_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Assignable)
    }

    /// Reports whether `source` and `target` are identical.
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/relater.go:Checker.isTypeIdenticalTo
    pub fn is_type_identical_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Identity)
    }

    /// Reports whether `source` is a subtype of `target`.
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/relater.go:Checker.isTypeSubtypeOf
    pub fn is_type_subtype_of(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Subtype)
    }

    /// Reports whether `source` is comparable to `target`.
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/relater.go:Checker.isTypeComparableTo
    pub fn is_type_comparable_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Comparable)
    }

    /// The relation entry point: normalizes fresh literals, applies the simple
    /// (primitive/literal) rules, and falls back to structural comparison.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, BoundProgram, RelationKind};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let any = c.any_type();
    /// let s = c.string_type();
    /// assert!(c.is_type_related_to(p, s, any, RelationKind::Assignable));
    /// # }
    /// ```
    ///
    /// Side effects: populates the relation cache; may build property types.
    // Go: internal/checker/relater.go:Checker.isTypeRelatedTo
    pub fn is_type_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let source = self.regular_literal_type(source);
        let target = self.regular_literal_type(target);
        // Go interns literal types by value (`getStringLiteralType` /
        // `getNumberLiteralType`), so two occurrences of `"a"` are the same
        // `*Type` and identity already relates them. As of 4bc the port interns
        // string/number literals the same way (booleans were already singletons),
        // so equal-valued literals share one id and this identity check covers
        // `"a" === "a"` — the 4bb `literals_equal_by_value` value shim is retired.
        // Go: internal/checker/relater.go:Checker.isTypeRelatedTo (interned literal identity)
        if source == target {
            return true;
        }
        if relation != RelationKind::Identity {
            if self.is_simple_type_related_to(source, target, relation) {
                return true;
            }
            if relation == RelationKind::Comparable
                && !self.get_type(target).flags().contains(TypeFlags::NEVER)
                && self.is_simple_type_related_to(target, source, relation)
            {
                return true;
            }
        } else {
            let sf = self.get_type(source).flags();
            let tf = self.get_type(target).flags();
            if !(sf | tf).intersects(TypeFlags::UNION_OR_INTERSECTION) {
                if sf != tf {
                    return false;
                }
                if sf.intersects(TypeFlags::SINGLETON) {
                    return true;
                }
            }
        }
        let sf = self.get_type(source).flags();
        let tf = self.get_type(target).flags();
        if sf.intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
            || tf.intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
        {
            return self.check_type_related_to(program, source, target, relation);
        }
        false
    }

    // Normalizes a fresh literal type to its regular counterpart.
    // Go: internal/checker/relater.go:isFreshLiteralType usage
    fn regular_literal_type(&self, t: TypeId) -> TypeId {
        if let TypeData::Literal(d) = &self.get_type(t).data {
            if d.fresh_type == Some(t) {
                return d.regular_type.unwrap_or(t);
            }
        }
        t
    }

    // The primitive/literal/any/unknown/never rules (no structural recursion).
    // Go: internal/checker/relater.go:Checker.isSimpleTypeRelatedTo (4d subset)
    fn is_simple_type_related_to(
        &self,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let s = self.get_type(source).flags();
        let t = self.get_type(target).flags();
        if t.intersects(TypeFlags::ANY) || s.intersects(TypeFlags::NEVER) {
            return true;
        }
        if t.intersects(TypeFlags::UNKNOWN)
            && !(relation == RelationKind::StrictSubtype && s.intersects(TypeFlags::ANY))
        {
            return true;
        }
        if t.intersects(TypeFlags::NEVER) {
            return false;
        }
        if s.intersects(TypeFlags::STRING_LIKE) && t.intersects(TypeFlags::STRING) {
            return true;
        }
        // A string enum-literal source is assignable to a non-enum string
        // literal target with the same value (`E.A` (`"x"`) -> `"x"`).
        // Go: internal/checker/relater.go:isSimpleTypeRelatedTo (string enum literal)
        if s.contains(TypeFlags::STRING_LITERAL | TypeFlags::ENUM_LITERAL)
            && t.contains(TypeFlags::STRING_LITERAL)
            && !t.contains(TypeFlags::ENUM_LITERAL)
            && self.literal_values_equal(source, target)
        {
            return true;
        }
        if s.intersects(TypeFlags::NUMBER_LIKE) && t.intersects(TypeFlags::NUMBER) {
            return true;
        }
        // A numeric enum-literal source is assignable to a non-enum number
        // literal target with the same value (`E.B` (`2`) -> `2`).
        // Go: internal/checker/relater.go:isSimpleTypeRelatedTo (number enum literal)
        if s.contains(TypeFlags::NUMBER_LITERAL | TypeFlags::ENUM_LITERAL)
            && t.contains(TypeFlags::NUMBER_LITERAL)
            && !t.contains(TypeFlags::ENUM_LITERAL)
            && self.literal_values_equal(source, target)
        {
            return true;
        }
        if s.intersects(TypeFlags::BIG_INT_LIKE) && t.intersects(TypeFlags::BIG_INT) {
            return true;
        }
        if s.intersects(TypeFlags::BOOLEAN_LIKE) && t.intersects(TypeFlags::BOOLEAN) {
            return true;
        }
        if s.intersects(TypeFlags::ES_SYMBOL_LIKE) && t.intersects(TypeFlags::ES_SYMBOL) {
            return true;
        }
        // In non-strictNullChecks mode, `undefined` and `null` are assignable to
        // anything except `never`. Since unions and intersections may reduce to
        // `never`, they are excluded here. Under strictNullChecks, `undefined`
        // is only assignable to itself / `void`, and `null` only to itself
        // (`any`/`unknown` are handled by the top-type rules above).
        if s.intersects(TypeFlags::UNDEFINED)
            && ((!self.strict_null_checks() && !t.intersects(TypeFlags::UNION_OR_INTERSECTION))
                || t.intersects(TypeFlags::VOID_LIKE))
        {
            return true;
        }
        if s.intersects(TypeFlags::NULL)
            && ((!self.strict_null_checks() && !t.intersects(TypeFlags::UNION_OR_INTERSECTION))
                || t.intersects(TypeFlags::NULL))
        {
            return true;
        }
        if s.intersects(TypeFlags::OBJECT) && t.intersects(TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if relation == RelationKind::Assignable || relation == RelationKind::Comparable {
            if s.intersects(TypeFlags::ANY) {
                return true;
            }
            // `number` is assignable to any numeric enum type / numeric enum
            // literal, and a non-enum numeric literal is assignable to a numeric
            // enum literal with a matching value, so enums can be used as bit
            // flags (`const c: E = 0;` is OK iff a member has value `0`).
            // Go: internal/checker/relater.go:isSimpleTypeRelatedTo (number -> enum)
            if s.intersects(TypeFlags::NUMBER)
                && (t.intersects(TypeFlags::ENUM)
                    || t.contains(TypeFlags::NUMBER_LITERAL | TypeFlags::ENUM_LITERAL))
            {
                return true;
            }
            if s.contains(TypeFlags::NUMBER_LITERAL)
                && !s.contains(TypeFlags::ENUM_LITERAL)
                && (t.intersects(TypeFlags::ENUM)
                    || (t.contains(TypeFlags::NUMBER_LITERAL | TypeFlags::ENUM_LITERAL)
                        && self.literal_values_equal(source, target)))
            {
                return true;
            }
        }
        false
    }

    // Reports whether two literal types carry equal values (Go's
    // `source.AsLiteralType().value == target.AsLiteralType().value`), used by
    // the enum-literal relation rules.
    // Go: internal/checker/relater.go:isSimpleTypeRelatedTo (value equality)
    fn literal_values_equal(&self, source: TypeId, target: TypeId) -> bool {
        match (
            self.get_type(source).literal_value(),
            self.get_type(target).literal_value(),
        ) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    // The cached structural relation check (Go's checkTypeRelatedTo core).
    //
    // Includes a depth guard: when `relation_depth` reaches 100, the comparison
    // returns `false` (overflow) instead of recursing further — preventing stack
    // overflow on deeply recursive structural types.
    //
    // Go: internal/checker/relater.go:Checker.checkTypeRelatedTo + recursiveTypeRelatedTo
    // (len(r.sourceStack) == 100 || len(r.targetStack) == 100 => overflow = true)
    fn check_type_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        if let Some(cached) = self.relations.get(relation, source, target) {
            return cached;
        }
        // Go: internal/checker/relater.go:recursiveTypeRelatedTo — when either
        // stack reaches depth 100, set overflow and return false.
        if self.relation_depth >= 100 {
            return false;
        }
        // Assume related while recursing to break cycles (e.g. `interface A { a: A }`).
        self.relations.set(relation, source, target, true);
        self.relation_depth += 1;
        let result = self.structured_type_related_to(program, source, target, relation);
        self.relation_depth -= 1;
        self.relations.set(relation, source, target, result);
        result
    }

    // Go: internal/checker/relater.go:Checker.structuredTypeRelatedTo (4d subset)
    fn structured_type_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let sf = self.get_type(source).flags();
        let tf = self.get_type(target).flags();
        if relation != RelationKind::Identity {
            // Target union: source must relate to some constituent.
            if tf.contains(TypeFlags::UNION) {
                let members = self.get_type(target).union_types().unwrap_or(&[]).to_vec();
                return members
                    .iter()
                    .any(|&m| self.is_type_related_to(program, source, m, relation));
            }
            // Source union: every constituent must relate to the target.
            if sf.contains(TypeFlags::UNION) {
                let members = self.get_type(source).union_types().unwrap_or(&[]).to_vec();
                return members
                    .iter()
                    .all(|&m| self.is_type_related_to(program, m, target, relation));
            }
            // Target intersection: source must relate to EACH constituent
            // (Go's `typeRelatedToEachType`). Deconstructed after unions, which
            // are always at the top of a normalized type.
            if tf.contains(TypeFlags::INTERSECTION) {
                let members = self
                    .get_type(target)
                    .intersection_types()
                    .unwrap_or(&[])
                    .to_vec();
                return members
                    .iter()
                    .all(|&m| self.is_type_related_to(program, source, m, relation));
            }
            // Source intersection: first try whether SOME constituent relates
            // to the target on its own (Go's `someTypeRelatedToType` for
            // `IntersectionStateSource`). When none does, Go falls back to
            // checking the full intersection viewed as an object — its
            // synthesized properties — against the target. We reach that
            // fallback when the target is an object type (e.g. `A & B` ↔ `AB`).
            if sf.contains(TypeFlags::INTERSECTION) {
                let members = self
                    .get_type(source)
                    .intersection_types()
                    .unwrap_or(&[])
                    .to_vec();
                if members
                    .iter()
                    .any(|&m| self.is_type_related_to(program, m, target, relation))
                {
                    return true;
                }
                if tf.contains(TypeFlags::OBJECT) {
                    return self.properties_related_to(program, source, target, relation);
                }
                return false;
            }
        }
        if sf.contains(TypeFlags::OBJECT) && tf.contains(TypeFlags::OBJECT) {
            if relation == RelationKind::Identity {
                return self.properties_related_to(program, source, target, relation)
                    && self.properties_related_to(program, target, source, relation);
            }
            // Two type references to the same generic target (`Array<X>` vs
            // `Array<Y>`, `Foo<X>` vs `Foo<Y>`) relate by their type arguments'
            // variance rather than structurally. For the reachable subset this is
            // authoritative: on the covariant path a failed argument relation
            // means the references are not related (Go does not fall back to a
            // structural comparison for a covariant variance failure).
            if let Some(result) =
                self.reference_type_arguments_related_to(program, source, target, relation)
            {
                return result;
            }
            if let Some(related) =
                self.array_target_index_types_related_to(program, source, target, relation)
            {
                return related;
            }
            // An object type relates structurally on BOTH its properties and its
            // call/construct signatures (Go relates properties, then call sigs,
            // then construct sigs — all must hold). A bare function type has no
            // properties, so the signature comparison is what makes two function
            // types relate (or not).
            // Go: internal/checker/relater.go:structuredTypeRelatedToWorker (object arm)
            if !self.properties_related_to(program, source, target, relation) {
                return false;
            }
            if !self.signatures_related_to(
                program,
                source,
                target,
                SignatureKind::Call,
                relation,
                None,
            ) {
                return false;
            }
            if !self.signatures_related_to(
                program,
                source,
                target,
                SignatureKind::Construct,
                relation,
                None,
            ) {
                return false;
            }
            return true;
        }
        false
    }

    // If `source` and `target` are type references to the SAME generic target
    // (Go's `source.Target() == target.Target()` arm), relates them by relating
    // their type arguments according to variance and returns `Some(result)`.
    // Returns `None` when the pair is not two same-target references, so the
    // caller falls back to a structural comparison.
    //
    // DEFER(phase-4-checker-4bp+): the full `getVariances` marker-probe variance
    // computation (contravariant/bivariant/invariant/independent parameters,
    // `in`/`out` annotations, and the structural fallback `relateVariances`
    // performs for `Unmeasurable`/`Unreliable`/covariant-`void` arguments). The
    // reachable subset defaults every type parameter to covariant, which is
    // correct for `Array`/`ReadonlyArray` element types and the common user
    // generic positions and fixes the `Array<number | string>` vs `Array<number>`
    // false positive. The empty-array-literal `never[]` short-circuit and
    // marker-type exclusion are also deferred.
    // blocked-by: `getVariances` (variance markers) + signature-level relation.
    // Go: internal/checker/relater.go:Checker.structuredTypeRelatedToWorker (same-target reference arm)
    fn reference_type_arguments_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> Option<bool> {
        let (source_target, source_args) = self.type_reference_target_and_arguments(source)?;
        let (target_target, target_args) = self.type_reference_target_and_arguments(target)?;
        if source_target != target_target {
            return None;
        }
        Some(self.type_arguments_related_to(program, &source_args, &target_args, relation))
    }

    // Returns a type's generic target and its resolved type arguments when it is
    // a type reference (Go's `ObjectFlagsReference` with a `target`). A tuple
    // (`TUPLE`-flagged, no `target`) and a plain anonymous/interface object
    // (no `target`) both yield `None`, so they are compared structurally.
    // Go: internal/checker/types.go:Type.Target / Checker.getTypeArguments (reference subset)
    fn type_reference_target_and_arguments(&self, t: TypeId) -> Option<(TypeId, Vec<TypeId>)> {
        if !self
            .get_type(t)
            .object_flags()
            .contains(ObjectFlags::REFERENCE)
        {
            return None;
        }
        let obj = self.get_type(t).as_object()?;
        let target = obj.target?;
        Some((target, obj.resolved_type_arguments.clone()))
    }

    // Pairwise-relates type arguments according to each type parameter's
    // variance (Go's `Relater.typeArgumentsRelatedTo`). The reachable subset
    // treats every parameter as COVARIANT — `source[i]` must relate to
    // `target[i]` — which is correct for `Array`/`ReadonlyArray` element types
    // and the common user-generic positions.
    //
    // DEFER(phase-4-checker-4bp+): contravariant (reversed), invariant (both
    // directions), bivariant (either direction), and independent (skipped) type
    // parameters, plus the `Unmeasurable`/`Unreliable` identity / instantiation
    // paths. Those require `getVariances`. blocked-by: variance-marker
    // computation + signature relation for function-parameter positions.
    // Go: internal/checker/relater.go:Relater.typeArgumentsRelatedTo
    fn type_arguments_related_to(
        &mut self,
        program: &dyn BoundProgram,
        sources: &[TypeId],
        targets: &[TypeId],
        relation: RelationKind,
    ) -> bool {
        // Go returns false for an identity check over differently-sized lists;
        // otherwise it relates the common prefix.
        if sources.len() != targets.len() && relation == RelationKind::Identity {
            return false;
        }
        let length = sources.len().min(targets.len());
        for i in 0..length {
            // DEFER: variance default is covariant for the reachable subset.
            if !self.is_type_related_to(program, sources[i], targets[i], relation) {
                return false;
            }
        }
        true
    }

    // Returns the call or construct signatures of `t` (Go's
    // `getSignaturesOfType(t, kind)`), resolving through a type reference's
    // generic target. Mirrors the public call-only [`get_signatures_of_type`]
    // but is kind-aware for the relation engine.
    // Go: internal/checker/checker.go:Checker.getSignaturesOfType
    fn signatures_of_type(&self, t: TypeId, kind: SignatureKind) -> Vec<SignatureId> {
        let apparent = get_apparent_type(self, t);
        let Some(obj) = self.get_type(apparent).as_object() else {
            return Vec::new();
        };
        let resolved = match obj.target {
            Some(target) => self.get_type(target).as_object(),
            None => Some(obj),
        };
        let Some(obj) = resolved else {
            return Vec::new();
        };
        match kind {
            SignatureKind::Call => obj.call_signatures.clone(),
            SignatureKind::Construct => obj.construct_signatures.clone(),
        }
    }

    // The number of declared parameters of a signature (Go's
    // `getParameterCount`, reachable subset without rest-tuple expansion).
    // Go: internal/checker/relater.go:Checker.getParameterCount
    fn signature_parameter_count(&self, sig: SignatureId) -> usize {
        self.signature(sig).parameters.len()
    }

    // The minimum number of arguments a signature requires (Go's
    // `getMinArgumentCount`, the stored required-parameter count for the
    // reachable subset without rest-tuple expansion).
    // Go: internal/checker/relater.go:Checker.getMinArgumentCount
    fn signature_min_argument_count(&self, sig: SignatureId) -> i32 {
        self.signature(sig).min_argument_count
    }

    // The resolved return type of a signature, or `any` when unresolved (Go's
    // `getReturnTypeOfSignature` for an already-resolved non-generic signature).
    // Go: internal/checker/checker.go:Checker.getReturnTypeOfSignature
    pub(crate) fn signature_return_type(&self, sig: SignatureId) -> TypeId {
        self.signature(sig)
            .resolved_return_type
            .unwrap_or(self.any_type)
    }

    // Selects the return-type incompatibility marker message (Go's
    // `compareSignaturesRelated` return-type block). All four are
    // `elidedInCompatibilityPyramid`: they are elided from the materialized
    // diagnostic chain and only trigger the `x()`/`x(...)` collapse when nested
    // under a property message.
    // Go: internal/checker/relater.go:Checker.compareSignaturesRelated (return marker)
    fn return_type_marker_message(
        &self,
        source: SignatureId,
        target: SignatureId,
    ) -> &'static Message {
        let no_args = self.signature(source).parameters.is_empty()
            && self.signature(target).parameters.is_empty();
        let is_construct = self
            .signature(source)
            .flags
            .contains(SignatureFlags::CONSTRUCT);
        match (no_args, is_construct) {
            (true, false) => {
                &tsgo_diagnostics::CALL_SIGNATURES_WITH_NO_ARGUMENTS_HAVE_INCOMPATIBLE_RETURN_TYPES_0_AND_1
            }
            (true, true) => {
                &tsgo_diagnostics::CONSTRUCT_SIGNATURES_WITH_NO_ARGUMENTS_HAVE_INCOMPATIBLE_RETURN_TYPES_0_AND_1
            }
            (false, false) => {
                &tsgo_diagnostics::CALL_SIGNATURE_RETURN_TYPES_0_AND_1_ARE_INCOMPATIBLE
            }
            (false, true) => {
                &tsgo_diagnostics::CONSTRUCT_SIGNATURE_RETURN_TYPES_0_AND_1_ARE_INCOMPATIBLE
            }
        }
    }

    // Reports whether `sig` is the top function/construct signature
    // `(...args: A) => R` where `A` is `any`/`never` (or an array thereof) and
    // `R` is `any`/`unknown` (Go's `isTopSignature`).
    // Go: internal/checker/relater.go:Checker.isTopSignature
    fn is_top_signature(&mut self, program: &dyn BoundProgram, sig: SignatureId) -> bool {
        let type_parameters = self.signature(sig).type_parameters.clone();
        if !type_parameters.is_empty() {
            return false;
        }
        let this_parameter = self.signature(sig).this_parameter;
        if let Some(this_param) = this_parameter {
            let this_type = get_type_of_symbol(self, program, this_param, None);
            if this_type != self.any_type {
                return false;
            }
        }
        let rest_param = match self.signature(sig).parameters.as_slice() {
            [only] if self.signature_has_rest_parameter(sig) => *only,
            _ => return false,
        };
        let rest_type = get_type_of_symbol(self, program, rest_param, None);
        let element_type = self
            .get_element_type_of_array_type(rest_type)
            .unwrap_or(rest_type);
        if !self
            .get_type(element_type)
            .flags()
            .intersects(TypeFlags::ANY | TypeFlags::NEVER)
        {
            return false;
        }
        self.get_type(self.signature_return_type(sig))
            .flags()
            .intersects(TypeFlags::ANY_OR_UNKNOWN)
    }

    // The parameter type at position `pos`, or `None` when out of range (Go's
    // `tryGetTypeAtPosition`).
    // Go: internal/checker/relater.go:Checker.tryGetTypeAtPosition
    fn try_signature_type_at_position(
        &mut self,
        program: &dyn BoundProgram,
        sig: SignatureId,
        pos: usize,
    ) -> Option<TypeId> {
        self.try_get_type_at_position(program, sig, pos)
    }

    // The parameter name at position `pos` (Go's `getParameterNameAtPosition`,
    // reachable subset: the parameter symbol's name).
    // Go: internal/checker/relater.go:Checker.getParameterNameAtPosition
    fn signature_parameter_name(
        &self,
        program: &dyn BoundProgram,
        sig: SignatureId,
        pos: usize,
    ) -> String {
        match self.signature(sig).parameters.get(pos).copied() {
            Some(param) => program.symbol(param).name.clone(),
            None => String::new(),
        }
    }

    // Reports whether a signature is a method/constructor declaration (Go's
    // `kind == KindMethodDeclaration | KindMethodSignature | KindConstructor`
    // test in `compareSignaturesRelated`). Such parameters are ALWAYS bivariant
    // regardless of `strictFunctionTypes`.
    // Go: internal/checker/relater.go:Checker.compareSignaturesRelated (strictVariance)
    fn signature_is_method(&self, program: &dyn BoundProgram, sig: SignatureId) -> bool {
        match self.signature(sig).declaration {
            Some(decl) => matches!(
                program.arena().kind(decl),
                Kind::MethodDeclaration | Kind::MethodSignature | Kind::Constructor
            ),
            None => false,
        }
    }

    // Relates the call/construct signatures of `source` to those of `target`
    // (Go's `signaturesRelatedTo` for a non-identity relation). Each target
    // signature must be matched by SOME source signature; the common
    // single-signature case is a direct pairwise comparison. A `reporter`
    // threads the reporting twin so a failure in the single-signature case hangs
    // its elaboration chain.
    //
    // DEFER(phase-4-checker-C-A+): the identity relation (`signaturesIdenticalTo`),
    // the `anyFunctionType` wildcard, the same-symbol/same-target pairwise
    // optimization, generic type-parameter erasure/instantiation, and the
    // multi-overload `Type_0_provides_no_match_for_the_signature_1` elaboration.
    // blocked-by: identity signature compare + `anyFunctionType` + generic
    // signatures + overload reporting.
    // Go: internal/checker/relater.go:Relater.signaturesRelatedTo
    fn signatures_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        kind: SignatureKind,
        relation: RelationKind,
        reporter: Option<&mut ChainReporter>,
    ) -> bool {
        let source_sigs = self.signatures_of_type(source, kind);
        let target_sigs = self.signatures_of_type(target, kind);
        if target_sigs.is_empty() {
            return true;
        }
        if source_sigs.is_empty() {
            return false;
        }
        if source_sigs.len() == 1 && target_sigs.len() == 1 {
            return self.compare_signatures_related(
                program,
                source_sigs[0],
                target_sigs[0],
                relation,
                reporter,
            );
        }
        // Each target signature must be matched by some source signature (Go's
        // N*M matrix). Reporting for this multi-overload case is deferred.
        for &t in &target_sigs {
            let mut matched = false;
            for &s in &source_sigs {
                if self.compare_signatures_related(program, s, t, relation, None) {
                    matched = true;
                    break;
                }
            }
            if !matched {
                return false;
            }
        }
        true
    }

    // Compares two signatures for a relation (Go's `compareSignaturesRelated`,
    // reachable subset). Parameters relate CONTRAVARIANTLY (`target` param ->
    // `source` param). A `reporter` (the reporting twin) hangs the `2328`
    // parameter-incompatibility message over the contravariant leaf on failure.
    //
    // This single function serves both the bool fast path (`reporter == None`,
    // no allocation/reporting) and the reporting twin (`reporter == Some`),
    // mirroring Go's one `compareSignaturesRelated(reportErrors, errorReporter,
    // compareTypes)` parameterized by reporting.
    //
    // DEFER(phase-4-checker-C-A+): the top-signature `(...args: any[]) => any`
    // short-circuit, generic instantiation/erasure, rest parameters, the `this`
    // parameter (its symbol is not yet collected), the callback-covariance
    // optimization (`getSingleCallSignature` on a parameter), type predicates,
    // and strict-arity subtype tightening. blocked-by: top signatures + generic
    // signatures + rest/`this` parameters + type predicates + callback variance.
    // Go: internal/checker/relater.go:Checker.compareSignaturesRelated
    fn compare_signatures_related(
        &mut self,
        program: &dyn BoundProgram,
        source: SignatureId,
        target: SignatureId,
        relation: RelationKind,
        mut reporter: Option<&mut ChainReporter>,
    ) -> bool {
        if source == target {
            return true;
        }
        if self.is_top_signature(program, target) {
            return true;
        }
        let source_count = self.signature_parameter_count(source);
        let target_count = self.signature_parameter_count(target);
        let source_has_rest = self.has_effective_rest_parameter(source);
        let target_has_rest = self.has_effective_rest_parameter(target);
        // Arity: a source requiring MORE arguments than the target accepts is
        // not assignable when the target has no effective rest parameter.
        if !target_has_rest && self.signature_min_argument_count(source) > target_count as i32 {
            if let Some(r) = &mut reporter {
                r.report(
                    &tsgo_diagnostics::TARGET_SIGNATURE_PROVIDES_TOO_FEW_ARGUMENTS_EXPECTED_0_OR_MORE_BUT_GOT_1,
                    vec![
                        self.signature_min_argument_count(source).to_string(),
                        target_count.to_string(),
                    ],
                );
            }
            return false;
        }
        let param_count = if source_has_rest || target_has_rest {
            source_count.min(target_count)
        } else {
            source_count.max(target_count)
        };
        let rest_index = (source_has_rest || target_has_rest).then(|| param_count.saturating_sub(1));
        // Under `strictFunctionTypes`, parameters relate strictly contravariantly;
        // otherwise (flag off OR a method/constructor target) they relate
        // bivariantly. Methods are always bivariant regardless of the flag (Go's
        // `SignatureFlagsIsMethod` / `KindMethod*` check).
        let strict_variance =
            self.strict_function_types() && !self.signature_is_method(program, target);
        for i in 0..param_count {
            let (source_type, target_type) = if Some(i) == rest_index {
                (
                    Some(self.get_rest_or_any_type_at_position(program, source, i)),
                    Some(self.get_rest_or_any_type_at_position(program, target, i)),
                )
            } else {
                (
                    self.try_get_type_at_position(program, source, i),
                    self.try_get_type_at_position(program, target, i),
                )
            };
            let (Some(source_type), Some(target_type)) = (source_type, target_type) else {
                continue;
            };
            if source_type == target_type {
                continue;
            }
            // Parameters relate contravariantly: the target parameter type must
            // be assignable to the source parameter type. Under bivariance (flag
            // off or a method target) also accept the forward `source -> target`
            // direction (Go tries it first, without reporting).
            let related = if !strict_variance
                && self.is_type_related_to(program, source_type, target_type, relation)
            {
                true
            } else {
                match &mut reporter {
                    Some(r) => {
                        self.report_is_related_to(program, target_type, source_type, relation, r)
                    }
                    None => self.is_type_related_to(program, target_type, source_type, relation),
                }
            };
            if !related {
                if let Some(r) = &mut reporter {
                    let source_name = self.signature_parameter_name(program, source, i);
                    let target_name = self.signature_parameter_name(program, target, i);
                    r.report(
                        &tsgo_diagnostics::TYPES_OF_PARAMETERS_0_AND_1_ARE_INCOMPATIBLE,
                        vec![source_name, target_name],
                    );
                }
                return false;
            }
        }
        // Return types relate covariantly; a `void`/`any` target return accepts
        // any source return (Go's void-return special case).
        let target_return = self.signature_return_type(target);
        if target_return == self.void_type() || target_return == self.any_type {
            return true;
        }
        let source_return = self.signature_return_type(source);
        let related = match &mut reporter {
            Some(r) => {
                self.report_is_related_to(program, source_return, target_return, relation, r)
            }
            None => self.is_type_related_to(program, source_return, target_return, relation),
        };
        if !related {
            if let Some(r) = &mut reporter {
                // The return-type marker is elided from the materialized chain;
                // it triggers the `x()`/`x(...)` collapse when nested under a
                // property and is otherwise dropped, leaving the inner return
                // relation's own message as the child.
                let message = self.return_type_marker_message(source, target);
                let source_str = type_to_string(self, program, source_return);
                let target_str = type_to_string(self, program, target_return);
                r.report(message, vec![source_str, target_str]);
            }
            return false;
        }
        true
    }

    // Whether every constituent of `t` satisfies `predicate` (Go's `everyType`).
    fn every_type(&self, t: TypeId, predicate: impl Fn(&Checker, TypeId) -> bool) -> bool {
        self.distributed_types(t)
            .iter()
            .all(|&member| predicate(self, member))
    }

    // Whether `t` is a mutable (non-readonly) tuple (Go's `isMutableTupleType`).
    fn is_mutable_tuple_type(&self, t: TypeId) -> bool {
        self.is_tuple_type(t) && !self.is_readonly_tuple_type(t)
    }

    // The `number`-index element type of an array or tuple, falling back to `any`
    // when no index signature applies (Go's `getIndexTypeOfTypeEx(_, number, any)`).
    fn number_index_element_type(&mut self, program: &dyn BoundProgram, t: TypeId) -> TypeId {
        let number = self.number_type();
        if let Some(element) = get_index_type_of_type(self, t, number) {
            return element;
        }
        if let Some(element) = get_property_type_for_index_type(self, program, t, number) {
            return element;
        }
        self.any_type
    }

    // Whether the array-target index-type arm of `structuredTypeRelatedTo` applies
    // (Go's `isArrayType(target) && (...)` guard).
    fn array_target_index_types_arm_applies(
        &self,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        if relation == RelationKind::Identity || !self.is_array_type(target) {
            return false;
        }
        (self.is_readonly_array_type(target)
            && self.every_type(source, |c, m| c.is_array_or_tuple_type(m)))
            || self.every_type(source, |c, m| c.is_mutable_tuple_type(m))
    }

    // Relates a mutable tuple (or readonly array/tuple to readonly array) source to
    // an array target by comparing `number` index types (Go's
    // `structuredTypeRelatedToWorker` array-target arm).
    // Go: internal/checker/relater.go:Relater.structuredTypeRelatedToWorker (array target)
    fn array_target_index_types_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> Option<bool> {
        if !self.array_target_index_types_arm_applies(source, target, relation) {
            return None;
        }
        if self.readonly_blocks_mutable_assignability(source, target, relation) {
            return Some(false);
        }
        let source_index = self.number_index_element_type(program, source);
        let target_index = self.number_index_element_type(program, target);
        Some(self.is_type_related_to(program, source_index, target_index, relation))
    }

    // Reporting twin of [`array_target_index_types_related_to`].
    fn report_array_target_index_types_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> Option<bool> {
        if !self.array_target_index_types_arm_applies(source, target, relation) {
            return None;
        }
        if self.readonly_blocks_mutable_assignability(source, target, relation) {
            return Some(false);
        }
        let source_index = self.number_index_element_type(program, source);
        let target_index = self.number_index_element_type(program, target);
        Some(self.report_is_related_to(program, source_index, target_index, relation, reporter))
    }

    // Returns the positional element types of a fixed-arity `TUPLE`-flagged type.
    fn tuple_element_types(&self, t: TypeId) -> Vec<TypeId> {
        self.get_type(t)
            .as_object()
            .map(|o| o.resolved_type_arguments.clone())
            .unwrap_or_default()
    }

    // Fixed-arity subset of Go's tuple `propertiesRelatedTo` arm when the target
    // is a tuple and the source is an array or another tuple. Arrays are treated
    // as variable-length (`sourceRest`); fixed tuples reject them with `2620`/
    // `2621` before element comparison.
    //
    // Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm)
    fn array_or_tuple_to_tuple_types_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let source_is_tuple = self.is_tuple_type(source);
        let source_rest = !source_is_tuple;
        let target_elements = self.tuple_element_types(target);
        let target_arity = target_elements.len();
        let target_has_variable = self.tuple_has_variable_element(target);
        let target_min_length = self.tuple_min_length(target);
        let source_arity = if source_is_tuple {
            self.tuple_element_types(source).len()
        } else {
            1
        };
        let source_min_length = if source_is_tuple { source_arity } else { 0 };
        if !source_rest && source_min_length < target_min_length {
            return false;
        }
        if !target_has_variable && target_arity < source_min_length {
            return false;
        }
        if !target_has_variable && (source_rest || target_arity < source_arity) {
            return false;
        }
        if source_is_tuple {
            return self.tuple_to_tuple_types_related_to(program, source, target, relation);
        }
        let Some(array_element) = self.get_element_type_of_array_type(source) else {
            return false;
        };
        target_elements.iter().all(|&target_type| {
            self.is_type_related_to(program, array_element, target_type, relation)
        })
    }

    // Relates a tuple source to a tuple target, including rest/variadic targets.
    // Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple element loop)
    fn tuple_to_tuple_types_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        if self.tuple_has_variable_element(target) || self.tuple_has_variable_element(source) {
            if !self.tuple_positions_related_to(program, source, target, relation, None) {
                return false;
            }
        } else if !self.tuple_elements_related_to(program, source, target, relation) {
            return false;
        }
        !self.readonly_blocks_mutable_assignability(source, target, relation)
    }

    // Maps a source tuple position to its target position and compare types for
    // tuple targets with rest/variadic elements (Go's `propertiesRelatedTo` loop).
    fn tuple_positions_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        mut reporter: Option<&mut ChainReporter>,
    ) -> bool {
        let source_elements = self.tuple_element_types(source);
        let target_elements = self.tuple_element_types(target);
        let source_arity = source_elements.len();
        let target_arity = target_elements.len();
        let target_start = self.tuple_start_element_count(target);
        let target_end = self.tuple_end_element_count(target);
        let target_has_variable = self.tuple_has_variable_element(target);
        for source_position in 0..source_arity {
            let source_variadic = self.tuple_element_is_variadic(source, source_position);
            let source_position_from_end = source_arity - 1 - source_position;
            let target_position = if target_has_variable && source_position >= target_start {
                target_arity - 1 - source_position_from_end.min(target_end)
            } else {
                source_position
            };
            let target_variadic = self.tuple_element_is_variadic(target, target_position);
            let target_rest = self.tuple_element_is_rest(target, target_position)
                || (self.tuple_has_rest_element(target)
                    && target_position >= self.tuple_fixed_length(target).unwrap_or(target_arity));
            let target_variable = target_variadic || target_rest;
            if target_variadic && !source_variadic {
                if let Some(r) = reporter.as_mut() {
                    r.report(
                        &tsgo_diagnostics::SOURCE_PROVIDES_NO_MATCH_FOR_VARIADIC_ELEMENT_AT_POSITION_0_IN_TARGET,
                        vec![target_position.to_string()],
                    );
                }
                return false;
            }
            if source_variadic && !target_variable {
                if let Some(r) = reporter.as_mut() {
                    r.report(
                        &tsgo_diagnostics::VARIADIC_ELEMENT_AT_POSITION_0_IN_SOURCE_DOES_NOT_MATCH_ELEMENT_AT_POSITION_1_IN_TARGET,
                        vec![
                            source_position.to_string(),
                            target_position.to_string(),
                        ],
                    );
                }
                return false;
            }
            if self.tuple_element_is_required(target, target_position)
                && self.tuple_element_is_optional(source, source_position)
            {
                if let Some(r) = reporter.as_mut() {
                    r.report(
                        &tsgo_diagnostics::SOURCE_PROVIDES_NO_MATCH_FOR_REQUIRED_ELEMENT_AT_POSITION_0_IN_TARGET,
                        vec![target_position.to_string()],
                    );
                }
                return false;
            }
            let source_type = source_elements[source_position];
            let target_type = target_elements
                .get(target_position)
                .copied()
                .unwrap_or_else(|| self.any_type());
            let target_check_type = if source_variadic && target_rest {
                self.create_array_type(target_type)
            } else {
                target_type
            };
            let related = if let Some(r) = reporter.as_mut() {
                self.report_is_related_to(program, source_type, target_check_type, relation, r)
            } else {
                self.is_type_related_to(program, source_type, target_check_type, relation)
            };
            if !related {
                if let Some(r) = reporter.as_mut() {
                    if target_arity > 1 || source_arity > 1 {
                        if target_has_variable
                            && source_position >= target_start
                            && source_position_from_end >= target_end
                            && target_start != source_arity - target_end - 1
                        {
                            r.report(
                                &tsgo_diagnostics::TYPE_AT_POSITIONS_0_THROUGH_1_IN_SOURCE_IS_NOT_COMPATIBLE_WITH_TYPE_AT_POSITION_2_IN_TARGET,
                                vec![
                                    target_start.to_string(),
                                    (source_arity - target_end - 1).to_string(),
                                    target_position.to_string(),
                                ],
                            );
                        } else {
                            r.report(
                                &tsgo_diagnostics::TYPE_AT_POSITION_0_IN_SOURCE_IS_NOT_COMPATIBLE_WITH_TYPE_AT_POSITION_1_IN_TARGET,
                                vec![
                                    source_position.to_string(),
                                    target_position.to_string(),
                                ],
                            );
                        }
                    }
                }
                return false;
            }
        }
        true
    }

    // Reporting twin of [`array_or_tuple_to_tuple_types_related_to`].
    fn report_array_or_tuple_to_tuple_types_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> bool {
        let source_is_tuple = self.is_tuple_type(source);
        let source_rest = !source_is_tuple;
        let target_elements = self.tuple_element_types(target);
        let target_arity = target_elements.len();
        let source_arity = if source_is_tuple {
            self.tuple_element_types(source).len()
        } else {
            1
        };
        let source_min_length = if source_is_tuple { source_arity } else { 0 };
        let target_has_variable = self.tuple_has_variable_element(target);
        let target_min_length = self.tuple_min_length(target);
        if !source_rest && source_min_length < target_min_length {
            reporter.report(
                &tsgo_diagnostics::SOURCE_HAS_0_ELEMENT_S_BUT_TARGET_REQUIRES_1,
                vec![source_min_length.to_string(), target_min_length.to_string()],
            );
            return false;
        }
        if !target_has_variable && target_arity < source_min_length {
            reporter.report(
                &tsgo_diagnostics::SOURCE_HAS_0_ELEMENT_S_BUT_TARGET_ALLOWS_ONLY_1,
                vec![source_min_length.to_string(), target_arity.to_string()],
            );
            return false;
        }
        if !target_has_variable && (source_rest || target_arity < source_arity) {
            if source_min_length < target_min_length {
                reporter.report(
                    &tsgo_diagnostics::TARGET_REQUIRES_0_ELEMENT_S_BUT_SOURCE_MAY_HAVE_FEWER,
                    vec![target_min_length.to_string()],
                );
            } else {
                reporter.report(
                    &tsgo_diagnostics::TARGET_ALLOWS_ONLY_0_ELEMENT_S_BUT_SOURCE_MAY_HAVE_MORE,
                    vec![target_arity.to_string()],
                );
            }
            return false;
        }
        if source_is_tuple {
            return self.report_tuple_types_related_to(program, source, target, relation, reporter);
        }
        let Some(array_element) = self.get_element_type_of_array_type(source) else {
            return false;
        };
        for (position, &target_type) in target_elements.iter().enumerate() {
            if !self.report_is_related_to(program, array_element, target_type, relation, reporter) {
                if target_arity > 1 {
                    reporter.report(
                        &tsgo_diagnostics::TYPE_AT_POSITION_0_IN_SOURCE_IS_NOT_COMPATIBLE_WITH_TYPE_AT_POSITION_1_IN_TARGET,
                        vec![position.to_string(), position.to_string()],
                    );
                }
                return false;
            }
        }
        true
    }

    // On the first incompatible positional element, hang `2626` over the child's
    // positional element, hang `2626` over the child's head when either tuple
    // has arity greater than one (Go's tuple `reportErrors` arm).
    // Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm)
    fn report_tuple_types_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> bool {
        if self.tuple_has_variable_element(target) || self.tuple_has_variable_element(source) {
            if !self.tuple_positions_related_to(program, source, target, relation, Some(reporter)) {
                return false;
            }
            return !self.readonly_blocks_mutable_assignability(source, target, relation);
        }
        let source_elements = self.tuple_element_types(source);
        let target_elements = self.tuple_element_types(target);
        if source_elements.len() != target_elements.len() {
            if source_elements.len() < target_elements.len() {
                reporter.report(
                    &tsgo_diagnostics::SOURCE_HAS_0_ELEMENT_S_BUT_TARGET_REQUIRES_1,
                    vec![
                        source_elements.len().to_string(),
                        target_elements.len().to_string(),
                    ],
                );
            } else {
                reporter.report(
                    &tsgo_diagnostics::SOURCE_HAS_0_ELEMENT_S_BUT_TARGET_ALLOWS_ONLY_1,
                    vec![
                        source_elements.len().to_string(),
                        target_elements.len().to_string(),
                    ],
                );
            }
            return false;
        }
        for (source_position, (&source_type, &target_type)) in source_elements
            .iter()
            .zip(target_elements.iter())
            .enumerate()
        {
            if self.tuple_element_is_required(target, source_position)
                && self.tuple_element_is_optional(source, source_position)
            {
                reporter.report(
                    &tsgo_diagnostics::SOURCE_PROVIDES_NO_MATCH_FOR_REQUIRED_ELEMENT_AT_POSITION_0_IN_TARGET,
                    vec![source_position.to_string()],
                );
                return false;
            }
            if !self.report_is_related_to(program, source_type, target_type, relation, reporter) {
                if source_elements.len() > 1 || target_elements.len() > 1 {
                    reporter.report(
                        &tsgo_diagnostics::TYPE_AT_POSITION_0_IN_SOURCE_IS_NOT_COMPATIBLE_WITH_TYPE_AT_POSITION_1_IN_TARGET,
                        vec![source_position.to_string(), source_position.to_string()],
                    );
                }
                return false;
            }
        }
        !self.readonly_blocks_mutable_assignability(source, target, relation)
    }

    // For each property of `target`, `source` must have a property whose type is
    // related. A missing source property is tolerated only when the target
    // property is optional and the relation does not require optional properties
    // (Go's `getUnmatchedProperty` with `requireOptionalProperties`).
    // Go: internal/checker/relater.go:Checker.propertiesRelatedTo (4d subset)
    fn properties_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        // A readonly tuple cannot satisfy a mutable array target (Go's
        // `tryElaborateArrayLikeErrors` tuple-source arm).
        if self.is_tuple_type(source)
            && self.is_array_type(target)
            && self.readonly_blocks_mutable_assignability(source, target, relation)
        {
            return false;
        }
        // Go's tuple `propertiesRelatedTo` arm: a readonly array source cannot
        // satisfy a mutable tuple target before arity/element checks. Tuple-tuple
        // readonly is checked after elements so `2626` wins over `4104`.
        if self
            .get_type(target)
            .object_flags()
            .contains(ObjectFlags::TUPLE)
            && self.is_array_or_tuple_type(source)
        {
            if self.is_array_type(source)
                && self.readonly_blocks_mutable_assignability(source, target, relation)
            {
                return false;
            }
            return self
                .array_or_tuple_to_tuple_types_related_to(program, source, target, relation);
        }
        // Go: subtype/strictSubtype relations still require optional members to
        // be matched; assignability/comparability/identity do not.
        let require_optional_properties =
            relation == RelationKind::Subtype || relation == RelationKind::StrictSubtype;
        for (name, target_prop) in get_properties_of_type(self, target) {
            let Some(source_prop) = get_property_of_type(self, source, &name) else {
                if !require_optional_properties && self.symbol_is_optional(program, target_prop) {
                    continue;
                }
                return false;
            };
            let source_type = self.instantiated_property_type(program, source, source_prop);
            let target_type = self.instantiated_property_type(program, target, target_prop);
            if !self.is_type_related_to(program, source_type, target_type, relation) {
                return false;
            }
            // An optional source property cannot satisfy a required target
            // (class) member: the source value might lack the property entirely.
            // Comparability passes Go's `skipOptional`, tolerating the mismatch.
            if relation != RelationKind::Comparable
                && self.symbol_is_optional(program, source_prop)
                && self.symbol_is_class_member(program, target_prop)
                && !self.symbol_is_optional(program, target_prop)
            {
                return false;
            }
        }
        !self.readonly_blocks_mutable_assignability(source, target, relation)
    }

    // Returns a property symbol's type as seen on `containing`, instantiating it
    // through `containing`'s `type parameters -> type arguments` mapper when
    // `containing` is a generic type reference. Go's `getPropertiesOfType`
    // returns instantiated member symbols for a reference, so its
    // `getTypeOfSymbol(prop)` already sees `Box<number>.v` as `number`; the port
    // shares the target's member symbols (member-type instantiation is deferred
    // in `create_type_reference`), so the relation engine instantiates here.
    // For a non-reference type this is exactly `get_type_of_symbol(prop)`.
    // Go: internal/checker/checker.go:Checker.getTypeOfSymbol (instantiated reference member)
    fn instantiated_property_type(
        &mut self,
        program: &dyn BoundProgram,
        containing: TypeId,
        prop: SymbolId,
    ) -> TypeId {
        let prop_type = get_type_of_symbol(self, program, prop, None);
        let reference = self
            .get_type(containing)
            .as_object()
            .and_then(|o| o.target.map(|t| (t, o.resolved_type_arguments.clone())));
        if let Some((target, args)) = reference {
            let params = self
                .get_type(target)
                .as_object()
                .map(|o| o.type_parameters.clone())
                .unwrap_or_default();
            if !params.is_empty() && params.len() == args.len() {
                let mapper = super::mapper::TypeMapper::Array {
                    sources: params,
                    targets: args,
                };
                return self.instantiate_type(prop_type, &mapper);
            }
        }
        prop_type
    }

    // Reports whether a property symbol was declared optional (`a?: T`).
    // Routes synthesized union/intersection property symbols to the checker's
    // transient arena instead of the program (which would panic on a tagged id).
    // Go: internal/checker/relater.go usage of `ast.SymbolFlagsOptional`
    fn symbol_is_optional(&self, program: &dyn BoundProgram, symbol: SymbolId) -> bool {
        self.resolved_symbol_flags(program, symbol)
            .contains(SymbolFlags::OPTIONAL)
    }

    // Reports whether a symbol is a class member (method/accessor/property).
    // Go: internal/ast/symbolflags.go:SymbolFlagsClassMember
    fn symbol_is_class_member(&self, program: &dyn BoundProgram, symbol: SymbolId) -> bool {
        const CLASS_MEMBER: SymbolFlags = SymbolFlags::METHOD
            .union(SymbolFlags::GET_ACCESSOR)
            .union(SymbolFlags::SET_ACCESSOR)
            .union(SymbolFlags::PROPERTY);
        self.resolved_symbol_flags(program, symbol)
            .intersects(CLASS_MEMBER)
    }

    // Reports whether `t` is an object-literal type (Go's `isObjectLiteralType`):
    // an object type carrying the `ObjectLiteral` flag. This (plus the
    // `FreshLiteral` flag) gates excess-property checking.
    // Go: internal/checker/utilities.go:isObjectLiteralType(801)
    pub(crate) fn is_object_literal_type(&self, t: TypeId) -> bool {
        self.get_type(t)
            .object_flags()
            .contains(ObjectFlags::OBJECT_LITERAL)
    }

    // Reports whether `target` is a valid target for excess-property checking
    // (Go's `isExcessPropertyCheckTarget`): a non-pattern object type, the
    // non-primitive `object` type, or a union (some constituent is a target) /
    // intersection (every constituent is a target).
    //
    // DEFER(phase-4-checker-4bg+): the substitution-type arm
    // (`isExcessPropertyCheckTarget(baseType)`). blocked-by: substitution types.
    // Go: internal/checker/relater.go:isExcessPropertyCheckTarget(746)
    pub(crate) fn is_excess_property_check_target(&self, target: TypeId) -> bool {
        let flags = self.get_type(target).flags();
        if flags.intersects(TypeFlags::OBJECT)
            && !self
                .get_type(target)
                .object_flags()
                .contains(ObjectFlags::OBJECT_LITERAL_PATTERN_WITH_COMPUTED_PROPERTIES)
        {
            return true;
        }
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if flags.intersects(TypeFlags::UNION) {
            let members = self.get_type(target).union_types().unwrap_or(&[]).to_vec();
            return members
                .iter()
                .any(|&m| self.is_excess_property_check_target(m));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let members = self
                .get_type(target)
                .intersection_types()
                .unwrap_or(&[])
                .to_vec();
            return members
                .iter()
                .all(|&m| self.is_excess_property_check_target(m));
        }
        false
    }

    // Reports whether `name` is a known property of `target` (Go's
    // `isKnownProperty`): an object type knows a name when it has that property;
    // a union/intersection target knows it when some constituent does.
    //
    // DEFER(phase-4-checker-4bg+): the late-bound-name string-index exception,
    // the substitution-type arm, and the JSX hyphenated-name allowance.
    // blocked-by: late binding, substitution types, and JSX attribute typing.
    // Go: internal/checker/relater.go:Checker.isKnownProperty(716)
    pub(crate) fn is_known_property(
        &mut self,
        program: &dyn BoundProgram,
        target: TypeId,
        name: &str,
    ) -> bool {
        let flags = self.get_type(target).flags();
        if flags.intersects(TypeFlags::OBJECT)
            && (get_property_of_type(self, target, name).is_some()
                || get_applicable_index_info_for_name(self, program, target, name).is_some())
        {
            return true;
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION)
            && self.is_excess_property_check_target(target)
        {
            let members = match self.get_type(target).data {
                TypeData::Union(ref u) => u.types.clone(),
                TypeData::Intersection(ref i) => i.types.clone(),
                _ => Vec::new(),
            };
            for m in members {
                if self.is_known_property(program, m, name) {
                    return true;
                }
            }
        }
        false
    }

    // Reports whether `t` is an empty object type (Go's `isEmptyObjectType`):
    // an object type with no properties, no call/construct signatures, and no
    // index signatures, or the non-primitive `object` type. Such a target
    // accepts any property, so excess-property checking is suppressed against it.
    //
    // DEFER(phase-4-checker-4bg+): the generic-mapped-type exclusion and the
    // union (some) / intersection (every) arms. blocked-by: mapped types and the
    // union/intersection excess-check reduction.
    // Go: internal/checker/checker.go:Checker.isEmptyObjectType(26326) / isEmptyResolvedType(26322)
    pub(crate) fn is_empty_object_type(&mut self, t: TypeId) -> bool {
        let flags = self.get_type(t).flags();
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if flags.intersects(TypeFlags::OBJECT) {
            if !get_properties_of_type(self, t).is_empty() {
                return false;
            }
            if let Some(obj) = self.get_type(t).as_object() {
                if !obj.call_signatures.is_empty() || !obj.construct_signatures.is_empty() {
                    return false;
                }
            }
            return super::declared_types::get_index_infos_of_type(self, t).is_empty();
        }
        false
    }

    // Reduces a union `target` to the constituent matching a discriminated
    // object/intersection `source`, or `None` when no discriminant selects a
    // single constituent (Go's `findMatchingDiscriminantType`). The reduced
    // constituent both speeds up the structural relation and — crucially for
    // excess-property checking — pins error placement to the SELECTED
    // constituent (e.g. `{ kind: "b", subkind: 1 }` against `… | { kind: "b" }`
    // reports `subkind` against `{ kind: "b" }`, not the whole union).
    //
    // DEFER(phase-4-checker-later): the `getMatchingUnionConstituentForType`
    // key-property fast path (`getKeyPropertyName` + the ≥10-constituent
    // `constituentMap`). It returns `nil` for every reachable corpus union
    // (small unions, or unions whose key property is absent in some
    // constituent), so the `findDiscriminantProperties` path below is
    // authoritative; the fast path would only pick the SAME constituent more
    // cheaply. blocked-by: the union key-property constituent map.
    // Go: internal/checker/relater.go:Checker.findMatchingDiscriminantType
    pub(crate) fn find_matching_discriminant_type(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> Option<TypeId> {
        if !self.get_type(target).flags().contains(TypeFlags::UNION) {
            return None;
        }
        if !self
            .get_type(source)
            .flags()
            .intersects(TypeFlags::INTERSECTION | TypeFlags::OBJECT)
        {
            return None;
        }
        let discriminants = self.find_discriminant_properties(program, source, target);
        if discriminants.is_empty() {
            return None;
        }
        let discriminated = self.discriminate_type_by_discriminable_items(
            program,
            target,
            &discriminants,
            relation,
        );
        if discriminated != target {
            Some(discriminated)
        } else {
            None
        }
    }

    // Returns the union constituent that best matches `source` for error
    // elaboration (Go's `getBestMatchingType`). The reachable subset uses the
    // discriminant-selected constituent.
    //
    // DEFER(phase-4-checker-later): the type-reference / type-alias match, the
    // object-literal "best" match, the invokable match, and the most-overlappy
    // scoring fallbacks. blocked-by: type-reference identity matching + overlap
    // scoring over object members.
    // Go: internal/checker/relater.go:Checker.getBestMatchingType
    fn get_best_matching_type(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> Option<TypeId> {
        self.find_matching_discriminant_type(program, source, target, relation)
    }

    // Collects the `(name, source-property-type)` of each source property that
    // is a discriminant property of the union `target` (Go's
    // `findDiscriminantProperties` over `getPropertiesOfType(source)`).
    // Go: internal/checker/relater.go:Checker.findDiscriminantProperties
    fn find_discriminant_properties(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> Vec<(String, TypeId)> {
        let mut result: Vec<(String, TypeId)> = Vec::new();
        for (name, _) in get_properties_of_type(self, source) {
            if self.is_discriminant_property(program, target, &name) {
                if let Some(source_type) =
                    get_type_of_property_of_type(self, program, source, &name)
                {
                    result.push((name, source_type));
                }
            }
        }
        result
    }

    // Refines a union `target` to the constituents whose discriminant
    // properties match the source (Go's `discriminateTypeByDiscriminableItems`).
    // Each discriminant eliminates constituents with a non-matching value while
    // keeping at least one match; an erroneous discriminant (no match) is
    // ignored. Returns the filtered union, or the original `target` when nothing
    // is eliminated or the filter would be `never`.
    // Go: internal/checker/relater.go:Checker.discriminateTypeByDiscriminableItems
    fn discriminate_type_by_discriminable_items(
        &mut self,
        program: &dyn BoundProgram,
        target: TypeId,
        discriminants: &[(String, TypeId)],
        relation: RelationKind,
    ) -> TypeId {
        let types = self.get_type(target).union_types().unwrap_or(&[]).to_vec();
        // `0` = excluded (False), `1` = included (True), `2` = undecided (Maybe).
        let mut include: Vec<u8> = types
            .iter()
            .map(|&t| {
                let flags = self.get_type(t).flags();
                if !flags.intersects(TypeFlags::PRIMITIVE) && !flags.intersects(TypeFlags::NEVER) {
                    1
                } else {
                    0
                }
            })
            .collect();
        for (name, source_type) in discriminants {
            let mut matched = false;
            for i in 0..types.len() {
                if include[i] == 0 {
                    continue;
                }
                if let Some(target_type) =
                    self.get_type_of_property_or_index_signature_of_type(program, types[i], name)
                {
                    if self.is_type_related_to(program, *source_type, target_type, relation) {
                        matched = true;
                    } else {
                        include[i] = 2;
                    }
                }
            }
            for inc in include.iter_mut() {
                if *inc == 2 {
                    *inc = if matched { 0 } else { 1 };
                }
            }
        }
        if include.contains(&0) {
            let filtered: Vec<TypeId> = types
                .iter()
                .enumerate()
                .filter(|(i, _)| include[*i] == 1)
                .map(|(_, &t)| t)
                .collect();
            let filtered_union = self.get_union_type(&filtered);
            if !self
                .get_type(filtered_union)
                .flags()
                .contains(TypeFlags::NEVER)
            {
                return filtered_union;
            }
        }
        target
    }

    // The type of `name` on `t` as a property or, failing that, an applicable
    // index signature (Go's `getTypeOfPropertyOrIndexSignatureOfType`).
    // Go: internal/checker/checker.go:Checker.getTypeOfPropertyOrIndexSignatureOfType
    fn get_type_of_property_or_index_signature_of_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        name: &str,
    ) -> Option<TypeId> {
        if let Some(prop_type) = get_type_of_property_of_type(self, program, t, name) {
            return Some(prop_type);
        }
        let info = get_applicable_index_info_for_name(self, program, t, name)?;
        Some(self.index_info(info).value_type)
    }

    // Keeps only the constituents of a union for which excess-property checking
    // applies (Go's `filterType(reducedTarget, isExcessPropertyCheckTarget)`),
    // used to print the error target in terms of the object types we actually
    // check. A non-union type is returned unchanged.
    // Go: internal/checker/relater.go:Relater.hasExcessProperties (errorTarget)
    pub(crate) fn filter_excess_property_check_target(&mut self, t: TypeId) -> TypeId {
        if !self.get_type(t).flags().contains(TypeFlags::UNION) {
            return t;
        }
        let members = self.get_type(t).union_types().unwrap_or(&[]).to_vec();
        let filtered: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| self.is_excess_property_check_target(m))
            .collect();
        self.get_union_type(&filtered)
    }

    // If `union` contains the non-primitive `object` type, drops its primitive
    // constituents (Go's `filterPrimitivesIfContainsNonPrimitive`). For every
    // reachable corpus union this is the identity (no bare `object`
    // constituent), so the full union is checked for excess properties.
    // Go: internal/checker/relater.go:Checker.filterPrimitivesIfContainsNonPrimitive
    pub(crate) fn filter_primitives_if_contains_non_primitive(&mut self, union: TypeId) -> TypeId {
        let members = self.get_type(union).union_types().unwrap_or(&[]).to_vec();
        if members.iter().any(|&m| {
            self.get_type(m)
                .flags()
                .intersects(TypeFlags::NON_PRIMITIVE)
        }) {
            let filtered: Vec<TypeId> = members
                .into_iter()
                .filter(|&m| !self.get_type(m).flags().intersects(TypeFlags::PRIMITIVE))
                .collect();
            let result = self.get_union_type(&filtered);
            if !self.get_type(result).flags().contains(TypeFlags::NEVER) {
                return result;
            }
        }
        union
    }

    // Runs the reporting twin of the relation recursion for a known-failed
    // `source`/`target` and materializes the nested elaboration chain (Go's
    // `checkTypeRelatedToEx` with `errorNode != nil`, then
    // `createDiagnosticChainFromErrorChain`). Returns `None` if the relation
    // unexpectedly holds (the bool fast path is the source of truth; callers
    // only invoke this after it returned `false`).
    //
    // The bool [`check_type_related_to`] fast path is left untouched: this
    // re-walks the structure only on the error path to build the chain, so the
    // hot path takes no perf hit.
    //
    // DEFER(phase-4-checker-4bo+): union/intersection elaboration, signature /
    // index-signature / array-element chains, and `elaborateError` (object- and
    // array-literal element-wise machinery). blocked-by: those reporting arms.
    // Go: internal/checker/relater.go:Checker.checkTypeRelatedToEx
    pub(crate) fn build_relation_error_chain(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> Option<RelationErrorReport> {
        let mut reporter = ChainReporter::default();
        let related = self.report_is_related_to(program, source, target, relation, &mut reporter);
        if related {
            return None;
        }
        reporter.chain.map(|chain| error_chain_to_report(*chain))
    }

    // The reporting twin of [`is_type_related_to`]: returns whether `source`
    // relates to `target`, and on failure appends this level's head message to
    // `reporter` (Go's `Relater.isRelatedToEx` with `reportErrors=true`).
    // Go: internal/checker/relater.go:Relater.isRelatedToEx
    fn report_is_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> bool {
        let source = self.regular_literal_type(source);
        let target = self.regular_literal_type(target);
        if source == target {
            return true;
        }
        if relation != RelationKind::Identity
            && self.is_simple_type_related_to(source, target, relation)
        {
            return true;
        }
        let sf = self.get_type(source).flags();
        let tf = self.get_type(target).flags();
        if (sf.intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
            || tf.intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE))
            && self.report_structured_type_related_to(program, source, target, relation, reporter)
        {
            return true;
        }
        // The relation failed at this level; add its head message. Go funnels
        // through `reportErrorResults`, whose other arms (array-like, primitive
        // wrapper-object, never-intersection, JSX) are DEFER for the reachable
        // structural-object subset.
        // Go: internal/checker/relater.go:Relater.reportErrorResults
        if sf.contains(TypeFlags::OBJECT) && tf.contains(TypeFlags::OBJECT) {
            self.try_elaborate_array_like_errors_report(program, source, target, reporter);
        }
        self.report_relation_error(program, source, target, relation, reporter);
        false
    }

    // The reporting twin of [`structured_type_related_to`]: only the
    // object-to-object property comparison is elaborated; union/intersection and
    // other structured/instantiable shapes fall back to the bool path (their
    // elaboration is DEFER) so the head message still reports without a child.
    // Go: internal/checker/relater.go:Relater.structuredTypeRelatedTo
    fn report_structured_type_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> bool {
        // Break co-recursive structural types: a pair already on the reporting
        // stack is treated as related (the bool path's cache breaks the same
        // cycles in the hot path).
        if !reporter.in_progress.insert((source, target)) {
            return self.structured_type_related_to(program, source, target, relation);
        }
        let sf = self.get_type(source).flags();
        let tf = self.get_type(target).flags();
        let result = if relation != RelationKind::Identity
            && (sf.intersects(TypeFlags::UNION_OR_INTERSECTION)
                || tf.intersects(TypeFlags::UNION_OR_INTERSECTION))
        {
            let related = self.structured_type_related_to(program, source, target, relation);
            // Go's `typeRelatedToSomeType` reportErrors arm: when a non-union
            // source fails to relate to a union target, elaborate against the
            // best matching constituent (the discriminant-selected one) so the
            // chain points at the offending member instead of a flat union
            // failure. Other union/intersection elaboration is DEFER.
            // Go: internal/checker/relater.go:Relater.typeRelatedToSomeType
            if !related && tf.contains(TypeFlags::UNION) && !sf.intersects(TypeFlags::UNION) {
                if let Some(best) = self.get_best_matching_type(program, source, target, relation) {
                    self.report_is_related_to(program, source, best, relation, reporter);
                }
            }
            related
        } else if relation != RelationKind::Identity
            && sf.contains(TypeFlags::OBJECT)
            && tf.contains(TypeFlags::OBJECT)
        {
            // Same-target generic references elaborate their type arguments (Go's
            // variance arm), mirroring the bool path; everything else elaborates
            // structurally.
            if let Some(related) = self.report_array_target_index_types_related_to(
                program, source, target, relation, reporter,
            ) {
                related
            } else {
                match self.report_reference_type_arguments_related_to(
                    program, source, target, relation, reporter,
                ) {
                    Some(result) => result,
                    None => {
                        // Elaborate properties, then call/construct signatures (the
                        // reporting twin of the bool object arm), so a function-type
                        // mismatch hangs its `2328`/return-type chain under the head.
                        self.report_properties_related_to(
                            program, source, target, relation, reporter,
                        ) && self.signatures_related_to(
                            program,
                            source,
                            target,
                            SignatureKind::Call,
                            relation,
                            Some(&mut *reporter),
                        ) && self.signatures_related_to(
                            program,
                            source,
                            target,
                            SignatureKind::Construct,
                            relation,
                            Some(&mut *reporter),
                        )
                    }
                }
            }
        } else {
            // Identity and other structured/instantiable shapes: no elaboration.
            self.structured_type_related_to(program, source, target, relation)
        };
        reporter.in_progress.remove(&(source, target));
        result
    }

    // The reporting twin of [`reference_type_arguments_related_to`]: for two
    // same-target references, relates type arguments covariantly with reporting
    // so a failing argument's own head message becomes the child of this level's
    // head (Go's `typeArgumentsRelatedTo` with `reportErrors=true`). Returns
    // `None` when the pair is not two same-target references.
    //
    // DEFER(phase-4-checker-4bp+): same as the bool twin — full variance, plus
    // the deeper union/intersection elaboration of a failing type argument (so a
    // `string | number` vs `number` argument reports only its own head, not the
    // per-constituent leaf). blocked-by: `getVariances` + union elaboration.
    // Go: internal/checker/relater.go:Relater.typeArgumentsRelatedTo (reportErrors)
    fn report_reference_type_arguments_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> Option<bool> {
        let (source_target, source_args) = self.type_reference_target_and_arguments(source)?;
        let (target_target, target_args) = self.type_reference_target_and_arguments(target)?;
        if source_target != target_target {
            return None;
        }
        if relation == RelationKind::Identity && source_args.len() != target_args.len() {
            return Some(false);
        }
        let length = source_args.len().min(target_args.len());
        for i in 0..length {
            // DEFER: covariant default for the reachable subset.
            if !self.report_is_related_to(
                program,
                source_args[i],
                target_args[i],
                relation,
                reporter,
            ) {
                return Some(false);
            }
        }
        Some(true)
    }

    // The reporting twin of [`properties_related_to`] (Go's
    // `Relater.propertiesRelatedTo` for the assignable/subtype object case):
    // first report a missing required property (`2741`/`2739`), else elaborate
    // the first incompatible present property (`2326` over its child).
    // Go: internal/checker/relater.go:Relater.propertiesRelatedTo
    fn report_properties_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> bool {
        if self.is_tuple_type(source)
            && self.is_array_type(target)
            && self.readonly_blocks_mutable_assignability(source, target, relation)
        {
            return false;
        }
        if self
            .get_type(target)
            .object_flags()
            .contains(ObjectFlags::TUPLE)
            && self.is_array_or_tuple_type(source)
        {
            if self.is_array_type(source)
                && self.readonly_blocks_mutable_assignability(source, target, relation)
            {
                return false;
            }
            return self.report_array_or_tuple_to_tuple_types_related_to(
                program, source, target, relation, reporter,
            );
        }
        // Go: subtype/strictSubtype require matching optional members too.
        let require_optional_properties =
            relation == RelationKind::Subtype || relation == RelationKind::StrictSubtype;
        if let Some(unmatched) =
            self.get_unmatched_property(program, source, target, require_optional_properties)
        {
            if self.should_report_unmatched_property_error(source) {
                self.report_unmatched_property(
                    program,
                    source,
                    target,
                    unmatched,
                    require_optional_properties,
                    reporter,
                );
            }
            return false;
        }
        for (name, target_prop) in get_properties_of_type(self, target) {
            let Some(source_prop) = get_property_of_type(self, source, &name) else {
                continue;
            };
            if source_prop == target_prop {
                continue;
            }
            if !self.report_property_related_to(
                program,
                (source, target),
                source_prop,
                target_prop,
                relation,
                reporter,
            ) {
                return false;
            }
        }
        !self.readonly_blocks_mutable_assignability(source, target, relation)
    }

    // The reporting twin of [`properties_related_to`]'s per-property step (Go's
    // `Relater.propertyRelatedTo`): relate the source/target property types and,
    // on failure, hang `Types_of_property_0_are_incompatible` (`2326`) over the
    // child; also report the optional-in-source vs required-in-target case
    // (`2327`).
    //
    // DEFER(phase-4-checker-4bo+): the private/protected modifier arms and the
    // partial-union/`addOptionalityEx` target adjustment. blocked-by: access
    // modifiers in the relation engine + partial union property synthesis.
    // Go: internal/checker/relater.go:Relater.propertyRelatedTo
    fn report_property_related_to(
        &mut self,
        program: &dyn BoundProgram,
        parents: (TypeId, TypeId),
        source_prop: SymbolId,
        target_prop: SymbolId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) -> bool {
        let (source, target) = parents;
        let source_type = self.instantiated_property_type(program, source, source_prop);
        let target_type = self.instantiated_property_type(program, target, target_prop);
        let related =
            self.report_is_related_to(program, source_type, target_type, relation, reporter);
        if !related {
            let target_name = self.resolved_symbol_name(program, target_prop);
            reporter.report(
                &tsgo_diagnostics::TYPES_OF_PROPERTY_0_ARE_INCOMPATIBLE,
                vec![target_name],
            );
            return false;
        }
        // An optional source property cannot satisfy a required target class
        // member; comparability is lenient (`skipOptional`).
        if relation != RelationKind::Comparable
            && self.symbol_is_optional(program, source_prop)
            && self.symbol_is_class_member(program, target_prop)
            && !self.symbol_is_optional(program, target_prop)
        {
            let target_name = self.resolved_symbol_name(program, target_prop);
            let source_str = type_to_string(self, program, source);
            let target_str = type_to_string(self, program, target);
            reporter.report(
                &tsgo_diagnostics::PROPERTY_0_IS_OPTIONAL_IN_TYPE_1_BUT_REQUIRED_IN_TYPE_2,
                vec![target_name, source_str, target_str],
            );
            return false;
        }
        related
    }

    // The reporting twin's head-message step (Go's `Relater.reportRelationError`
    // reachable subset): pick the head message, generalize a literal source, and
    // suppress the head when the chain already leads with a missing-property
    // message whose source/target names match.
    //
    // DEFER(phase-4-checker-4bo+): the type-parameter-constraint arms, the
    // string-literal "Did you mean" suggestion, exactOptionalPropertyTypes, the
    // same-name disambiguation, and the excess-property / complexity / readonly
    // suppression cases. blocked-by: those reporting arms.
    // Go: internal/checker/relater.go:Relater.reportRelationError
    fn report_relation_error(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        reporter: &mut ChainReporter,
    ) {
        let source_type = type_to_string(self, program, source);
        let target_type = type_to_string(self, program, target);
        let generalized_source = self.generalized_source_for_error(source, target);
        let generalized_source_type = if generalized_source == source {
            source_type
        } else {
            type_to_string(self, program, generalized_source)
        };
        let message: &'static Message = if relation == RelationKind::Comparable {
            &tsgo_diagnostics::TYPE_0_IS_NOT_COMPARABLE_TO_TYPE_1
        } else {
            &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1
        };
        // Suppress the head when the next message is a missing-property message
        // for the same source/target (the missing-property message becomes the
        // diagnostic head instead).
        match reporter.chain_message(0).map(Message::code) {
            Some(2741) => {
                if !is_conversion_or_interface_implementation_message(message)
                    && reporter.chain_args_match(&[
                        None,
                        Some(&generalized_source_type),
                        Some(&target_type),
                    ])
                {
                    return;
                }
            }
            Some(2739) | Some(2740) => {
                if !is_conversion_or_interface_implementation_message(message)
                    && reporter
                        .chain_args_match(&[Some(&generalized_source_type), Some(&target_type)])
                {
                    return;
                }
            }
            Some(4104) => {
                if reporter.chain_args_match(&[Some(&generalized_source_type), Some(&target_type)])
                {
                    return;
                }
            }
            _ => {}
        }
        reporter.report(message, vec![generalized_source_type, target_type]);
    }

    // The first required target property missing from `source` (Go's
    // `getUnmatchedPropertiesWorker` returning the first). Optional/partial
    // target properties are skipped unless `require_optional_properties`.
    //
    // DEFER(phase-4-checker-4bo+): the static-private-identifier skip, the
    // `CheckFlagsPartial` test, and discriminant matching. blocked-by: those
    // symbol/check-flag features.
    // Go: internal/checker/relater.go:Checker.getUnmatchedPropertiesWorker
    fn get_unmatched_property(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        require_optional_properties: bool,
    ) -> Option<SymbolId> {
        for (name, target_prop) in get_properties_of_type(self, target) {
            if (require_optional_properties || !self.symbol_is_optional(program, target_prop))
                && get_property_of_type(self, source, &name).is_none()
            {
                return Some(target_prop);
            }
        }
        None
    }

    // All required target properties missing from `source` (Go's
    // `getUnmatchedProperties`), used to choose between the single-property
    // (`2741`) and multi-property (`2739`/`2740`) missing-property messages.
    // Go: internal/checker/relater.go:Checker.getUnmatchedProperties
    fn get_unmatched_properties(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        require_optional_properties: bool,
    ) -> Vec<SymbolId> {
        let mut out = Vec::new();
        for (name, target_prop) in get_properties_of_type(self, target) {
            if (require_optional_properties || !self.symbol_is_optional(program, target_prop))
                && get_property_of_type(self, source, &name).is_none()
            {
                out.push(target_prop);
            }
        }
        out
    }

    // Whether a missing-property error should be reported for `source` (Go's
    // `shouldReportUnmatchedPropertyError`): a callable-only source (call/
    // construct signatures and no properties) suppresses it.
    //
    // DEFER(phase-4-checker-4bo+): the refinement that still reports when the
    // target has matching signature kinds, and construct-signature collection
    // for the source. blocked-by: target signature comparison + construct
    // signatures.
    // Go: internal/checker/relater.go:Checker.shouldReportUnmatchedPropertyError
    fn should_report_unmatched_property_error(&mut self, source: TypeId) -> bool {
        if !get_properties_of_type(self, source).is_empty() {
            return true;
        }
        // A property-less source with call signatures is a callable value being
        // matched against an object type; focus stays off the missing property.
        self.get_signatures_of_type(source).is_empty()
    }

    // Reports the missing-property message(s) for `source` vs `target` (Go's
    // `Relater.reportUnmatchedProperty`): `2741` for a single missing property,
    // `2739`/`2740` for several.
    //
    // DEFER(phase-4-checker-4bo+): the private-identifier same-description arm
    // and the `'{0}' is declared here` related-info. blocked-by: private-name
    // symbol table keys + related-info on a synthesized property declaration.
    // Go: internal/checker/relater.go:Relater.reportUnmatchedProperty
    fn report_unmatched_property(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        unmatched: SymbolId,
        require_optional_properties: bool,
        reporter: &mut ChainReporter,
    ) {
        let props =
            self.get_unmatched_properties(program, source, target, require_optional_properties);
        if props.len() == 1 {
            let source_type = type_to_string(self, program, source);
            let target_type = type_to_string(self, program, target);
            let prop_name = self.resolved_symbol_name(program, unmatched);
            reporter.report(
                &tsgo_diagnostics::PROPERTY_0_IS_MISSING_IN_TYPE_1_BUT_REQUIRED_IN_TYPE_2,
                vec![prop_name, source_type, target_type],
            );
        } else if self.try_elaborate_array_like_errors(source, target) {
            let source_type = type_to_string(self, program, source);
            let target_type = type_to_string(self, program, target);
            if props.len() > 5 {
                let names = props[..4]
                    .iter()
                    .map(|&p| self.resolved_symbol_name(program, p))
                    .collect::<Vec<_>>()
                    .join(", ");
                reporter.report(
                    &tsgo_diagnostics::TYPE_0_IS_MISSING_THE_FOLLOWING_PROPERTIES_FROM_TYPE_1_COLON_2_AND_3_MORE,
                    vec![source_type, target_type, names, (props.len() - 4).to_string()],
                );
            } else {
                let names = props
                    .iter()
                    .map(|&p| self.resolved_symbol_name(program, p))
                    .collect::<Vec<_>>()
                    .join(", ");
                reporter.report(
                    &tsgo_diagnostics::TYPE_0_IS_MISSING_THE_FOLLOWING_PROPERTIES_FROM_TYPE_1_COLON_2,
                    vec![source_type, target_type, names],
                );
            }
        }
    }

    // Whether to elaborate array/tuple mutability/length errors instead of
    // properties (Go's `Relater.tryElaborateArrayLikeErrors` with
    // `reportErrors=false`). Returns `false` when a readonly source cannot be
    // assigned to a mutable target (so property elaboration is skipped).
    // Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors
    fn try_elaborate_array_like_errors(&mut self, source: TypeId, target: TypeId) -> bool {
        if self.is_tuple_type(source) {
            if self.readonly_blocks_mutable_assignability(source, target, RelationKind::Assignable)
            {
                return false;
            }
            return self.is_array_or_tuple_type(target);
        }
        if self.readonly_blocks_mutable_assignability(source, target, RelationKind::Assignable) {
            return false;
        }
        if self.is_tuple_type(target) {
            return self.is_array_type(source);
        }
        true
    }

    // Whether every positional tuple element of `source` relates to the
    // corresponding element of `target` (ignoring readonly mutability).
    fn tuple_elements_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let source_elements = self.tuple_element_types(source);
        let target_elements = self.tuple_element_types(target);
        source_elements.len() == target_elements.len()
            && source_elements
                .iter()
                .zip(target_elements.iter())
                .enumerate()
                .all(|(index, (&source_type, &target_type))| {
                    if self.tuple_element_is_required(target, index)
                        && self.tuple_element_is_optional(source, index)
                    {
                        return false;
                    }
                    self.is_type_related_to(program, source_type, target_type, relation)
                })
    }

    // Reporting twin: emits `4104` when a readonly array/tuple source cannot be
    // assigned to a mutable array/tuple target and the element types otherwise
    // match (Go skips `4104` when an element mismatch is the real failure).
    // Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors (reportErrors)
    fn try_elaborate_array_like_errors_report(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        reporter: &mut ChainReporter,
    ) {
        if !self.readonly_blocks_mutable_assignability(source, target, RelationKind::Assignable) {
            return;
        }
        if self.is_tuple_type(source)
            && self.is_tuple_type(target)
            && !self.tuple_elements_related_to(program, source, target, RelationKind::Assignable)
        {
            return;
        }
        let source_type = type_to_string(self, program, source);
        let target_type = type_to_string(self, program, target);
        reporter.report(
            &tsgo_diagnostics::THE_TYPE_0_IS_READONLY_AND_CANNOT_BE_ASSIGNED_TO_THE_MUTABLE_TYPE_1,
            vec![source_type, target_type],
        );
    }

    /// Reports whether every constituent of `source` is included in `target`
    /// when `target` is a union (simplified subset of Go's `isTypeSubsetOf`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let ab = c.get_union_type(&[c.string_type(), c.number_type()]);
    /// assert!(c.is_type_subset_of(c.string_type(), ab));
    /// assert!(!c.is_type_subset_of(ab, c.string_type()));
    /// ```
    ///
    /// Side effects: none (pure read of the type arena).
    // Go: internal/checker/relater.go:Checker.isTypeSubsetOf(2811)
    pub fn is_type_subset_of(&self, source: TypeId, target: TypeId) -> bool {
        if source == target {
            return true;
        }
        if self.get_type(source).flags().contains(TypeFlags::NEVER) {
            return true;
        }
        if self.get_type(target).flags().contains(TypeFlags::UNION) {
            return self.is_type_subset_of_union(source, target);
        }
        false
    }

    // Go: internal/checker/relater.go:Checker.isTypeSubsetOfUnion(2815)
    fn is_type_subset_of_union(&self, source: TypeId, target: TypeId) -> bool {
        if self.get_type(source).flags().contains(TypeFlags::UNION) {
            if self.union_constituents(target).contains(&source) {
                return true;
            }
            return self
                .union_constituents(source)
                .iter()
                .all(|&member| self.is_type_subset_of(member, target));
        }
        let target_types = self.flatten_union_constituents(target);
        if super::types::contains_type(&target_types, source) {
            return true;
        }
        self.union_constituents(target)
            .iter()
            .any(|&member| self.is_type_subset_of(source, member))
    }

    fn flatten_union_constituents(&self, t: TypeId) -> Vec<TypeId> {
        let mut flat = Vec::new();
        self.collect_union_constituents(t, &mut flat);
        flat.sort();
        flat.dedup();
        flat
    }

    fn collect_union_constituents(&self, t: TypeId, out: &mut Vec<TypeId>) {
        if self.get_type(t).flags().contains(TypeFlags::UNION) {
            for member in self.union_constituents(t) {
                self.collect_union_constituents(member, out);
            }
        } else {
            out.push(t);
        }
    }

    fn union_constituents(&self, t: TypeId) -> Vec<TypeId> {
        match &self.get_type(t).data {
            TypeData::Union(union) => union.types.clone(),
            _ => vec![t],
        }
    }
}

// Whether `message` is a conversion or interface-implementation head message,
// which keeps the missing-property suppression in `reportRelationError` from
// firing (Go's `isConversionOrInterfaceImplementationMessage`).
//
// DEFER(phase-4-checker-4bo+): the JSX "is not a valid JSX element" codes.
// blocked-by: JSX element relation reporting.
// Go: internal/checker/relater.go:isConversionOrInterfaceImplementationMessage
fn is_conversion_or_interface_implementation_message(message: &Message) -> bool {
    // 2420 Class_0_incorrectly_implements_interface_1,
    // 2720 Class_0_incorrectly_implements_class_1_Did_you_mean_to_extend_1...,
    // 2352 Conversion_of_type_0_to_type_1_may_be_a_mistake...
    matches!(message.code(), 2420 | 2720 | 2352)
}

#[cfg(test)]
#[path = "relations_test.rs"]
mod tests;
