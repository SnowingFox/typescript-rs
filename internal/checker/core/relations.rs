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
use tsgo_ast::{SymbolFlags, SymbolId};
use tsgo_diagnostics::{Category, Message};

use super::check::DiagnosticMessageChain;
use super::declared_types::{
    get_applicable_index_info_for_name, get_properties_of_type, get_property_of_type,
    get_type_of_symbol,
};
use super::nodebuilder::{symbol_to_string, type_to_string};
use super::program::BoundProgram;
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
    RelationErrorReport {
        code: chain.message.code(),
        category: chain.message.category(),
        message: localize_chain_entry(chain.message, &chain.args),
        message_chain: chain
            .next
            .map(|n| error_chain_node_to_message_chain(*n))
            .into_iter()
            .collect(),
    }
}

// Materializes a non-head [`ErrorChain`] entry (and its descendants) into a
// [`DiagnosticMessageChain`].
fn error_chain_node_to_message_chain(chain: ErrorChain) -> DiagnosticMessageChain {
    DiagnosticMessageChain {
        code: chain.message.code(),
        category: chain.message.category(),
        message: localize_chain_entry(chain.message, &chain.args),
        next: chain
            .next
            .map(|n| error_chain_node_to_message_chain(*n))
            .into_iter()
            .collect(),
    }
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
        if s.intersects(TypeFlags::NUMBER_LIKE) && t.intersects(TypeFlags::NUMBER) {
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
        if (relation == RelationKind::Assignable || relation == RelationKind::Comparable)
            && s.intersects(TypeFlags::ANY)
        {
            return true;
        }
        false
    }

    // The cached structural relation check (Go's checkTypeRelatedTo core).
    //
    // DEFER(phase-4-checker-4e): the `Ternary` (Maybe) recursion machinery and
    // detailed error reporting. 4d optimistically caches `true` while recursing
    // to terminate on co-recursive structural types.
    // blocked-by: the full relater recursion model lands incrementally.
    // Go: internal/checker/relater.go:Checker.checkTypeRelatedTo
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
        // Assume related while recursing to break cycles (e.g. `interface A { a: A }`).
        self.relations.set(relation, source, target, true);
        let result = self.structured_type_related_to(program, source, target, relation);
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
            return self.properties_related_to(program, source, target, relation);
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
        true
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
            // DEFER(phase-4-checker-4bo+): union/intersection elaboration.
            self.structured_type_related_to(program, source, target, relation)
        } else if relation != RelationKind::Identity
            && sf.contains(TypeFlags::OBJECT)
            && tf.contains(TypeFlags::OBJECT)
        {
            // Same-target generic references elaborate their type arguments (Go's
            // variance arm), mirroring the bool path; everything else elaborates
            // structurally.
            match self.report_reference_type_arguments_related_to(
                program, source, target, relation, reporter,
            ) {
                Some(result) => result,
                None => {
                    self.report_properties_related_to(program, source, target, relation, reporter)
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
        true
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
            reporter.report(
                &tsgo_diagnostics::TYPES_OF_PROPERTY_0_ARE_INCOMPATIBLE,
                vec![symbol_to_string(program, target_prop)],
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
            let target_name = symbol_to_string(program, target_prop);
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
            let prop_name = symbol_to_string(program, unmatched);
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
                    .map(|&p| symbol_to_string(program, p))
                    .collect::<Vec<_>>()
                    .join(", ");
                reporter.report(
                    &tsgo_diagnostics::TYPE_0_IS_MISSING_THE_FOLLOWING_PROPERTIES_FROM_TYPE_1_COLON_2_AND_3_MORE,
                    vec![source_type, target_type, names, (props.len() - 4).to_string()],
                );
            } else {
                let names = props
                    .iter()
                    .map(|&p| symbol_to_string(program, p))
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
    // properties (Go's `Relater.tryElaborateArrayLikeErrors`). For the reachable
    // plain-object source/target this returns Go's default `true` (so the
    // multi-missing-property message is emitted).
    //
    // DEFER(phase-4-checker-4bo+): the tuple / readonly-array source arms and the
    // tuple-target `isArrayType(source)` arm. blocked-by: tuple/array types.
    // Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors
    fn try_elaborate_array_like_errors(&mut self, _source: TypeId, _target: TypeId) -> bool {
        true
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
