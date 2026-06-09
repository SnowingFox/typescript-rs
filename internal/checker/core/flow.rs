//! Control-flow analysis and type narrowing.
//!
//! Ports the reachable core of Go's `flow.go`: the narrowing primitives
//! (`narrowTypeByTypeof`/`narrowTypeByTruthiness`/`narrowTypeByEquality`/`in`),
//! the `get_flow_type_of_reference` flow-node walk, and `is_reachable_flow_node`.
//! The flow graph itself is built by `tsgo_binder` and exposed through
//! [`BoundProgram`].
//!
//! 4f covers the common linear cases (a reference narrowed inside an `if` by
//! `typeof`/truthiness/equality, branch-label unions, reachability). Go's full
//! `TypeFacts` lattice, loop fixpoints, assignment/array-mutation flow, and
//! optional-chain handling are deferred to later sub-phases.

use rustc_hash::{FxHashMap, FxHashSet};
use tsgo_ast::flow::{FlowFlags, FlowNodeId, FlowSwitchClauseData};
use tsgo_ast::{Kind, NodeData, NodeFlags, NodeId, SymbolFlags, SymbolId};

use super::program::BoundProgram;
use super::symbols::resolve_name;
use super::type_facts::TypeFacts;
use super::types::{LiteralValue, TypeFlags, TypeId};
use super::Checker;

/// Metadata for a user-defined type predicate used during flow narrowing (Go's
/// `TypePredicate` subset).
///
/// # Examples
/// ```
/// use tsgo_checker::core::flow::TypePredicateInfo;
/// let p = TypePredicateInfo {
///     parameter_index: 0,
///     predicate_type: None,
/// };
/// assert_eq!(p.parameter_index, 0);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:TypePredicate
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypePredicateInfo {
    /// Index of the guarded parameter in the call arguments (`-1` for `this`/method).
    pub parameter_index: i32,
    /// The asserted/predicate type, once resolved.
    pub predicate_type: Option<TypeId>,
}

impl Checker {
    /// Narrows `t` by a `typeof x === "<name>"` (or `!==`) guard.
    ///
    /// With `assume_true`, keeps the union constituents whose runtime `typeof`
    /// can be `<name>`; otherwise keeps the rest. Mirrors Go's
    /// `narrowTypeByTypeName` for the primitive type names.
    ///
    /// DEFER(phase-4-checker-4g): the `"function"`/host-object names and the full
    /// `TypeFacts`-based refinement (e.g. literal subtypes of `string`).
    /// blocked-by: `TypeFacts` + the global `Function` type (lib globals, P6).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, t: tsgo_checker::TypeId) {
    /// let _ = c.narrow_type_by_typeof(p, t, "string", true);
    /// # }
    /// ```
    ///
    /// Side effects: may allocate a union; populates the relation cache.
    // Go: internal/checker/flow.go:Checker.narrowTypeByTypeof / narrowTypeByTypeName
    pub fn narrow_type_by_typeof(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        type_name: &str,
        assume_true: bool,
    ) -> TypeId {
        let implied = match type_name {
            "string" => self.string_type(),
            "number" => self.number_type(),
            "boolean" => self.boolean_type(),
            "bigint" => self.bigint_type(),
            "symbol" => self.es_symbol_type(),
            "undefined" => self.undefined_type(),
            "object" => self.non_primitive_type(),
            // DEFER(phase-4-checker-4g): "function"/host-object typeof names.
            // blocked-by: the global `Function` type (lib globals, P6).
            _ => return t,
        };
        let members = self.distributed_types(t);
        let kept: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| {
                let related = self.is_type_subtype_of(program, m, implied)
                    || self.is_type_subtype_of(program, implied, m);
                related == assume_true
            })
            .collect();
        self.get_union_type(&kept)
    }

    /// Narrows `t` by the truthiness of a reference (`if (x) ...`).
    ///
    /// With `assume_true`, drops constituents that are definitely falsy
    /// (`undefined`/`null`/`void`/the `false` literal); with `assume_false`,
    /// keeps only those.
    ///
    /// DEFER(phase-4-checker-4g): the falsy literal subtypes of `string`/
    /// `number`/`bigint` (`""`/`0`/`0n`) and the `TypeFacts` refinement that
    /// narrows e.g. `string` to its empty-string subtype in the false branch.
    /// blocked-by: `TypeFacts` lattice.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let b = c.boolean_type();
    /// let t = c.regular_true_type();
    /// assert_eq!(c.narrow_type_by_truthiness(b, true), t);
    /// ```
    ///
    /// Side effects: may allocate a union.
    // Go: internal/checker/flow.go:Checker.narrowTypeByTruthiness
    pub fn narrow_type_by_truthiness(&mut self, t: TypeId, assume_true: bool) -> TypeId {
        let facts = if assume_true {
            super::type_facts::TypeFacts::TRUTHY
        } else {
            super::type_facts::TypeFacts::FALSY
        };
        let narrowed = self.get_type_with_facts(t, facts);
        if !assume_true {
            return narrowed;
        }
        // Truthy-narrowed `unknown` becomes the `{}` representative used for
        // `in`-operand checking (Go's `getAdjustedTypeWithFacts` / `unknownUnionType`
        // recombination subset).
        let members: Vec<TypeId> = self
            .distributed_types(narrowed)
            .into_iter()
            .map(|m| {
                if self.get_type(m).flags().contains(TypeFlags::UNKNOWN) {
                    self.unknown_empty_object_type()
                } else {
                    m
                }
            })
            .collect();
        self.get_union_type(&members)
    }

    /// Narrows `t` by an equality guard (`x === value` / `x !== value`).
    ///
    /// Keeps constituents that overlap `value_type`: literal constituents match
    /// by value, others by subtyping. With `assume_true` keeps the overlapping
    /// ones; with `assume_false` keeps the rest.
    ///
    /// DEFER(phase-4-checker-4g): full discriminated-union narrowing on a key
    /// property, `==`/`!=` (loose) nullish folding, and unit-type reduction.
    /// blocked-by: discriminant-property access + `TypeFacts`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, t: tsgo_checker::TypeId, v: tsgo_checker::TypeId) {
    /// let _ = c.narrow_type_by_equality(p, t, v, true);
    /// # }
    /// ```
    ///
    /// Side effects: may allocate a union; populates the relation cache.
    // Go: internal/checker/flow.go:Checker.narrowTypeByEquality
    pub fn narrow_type_by_equality(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        value_type: TypeId,
        assume_true: bool,
    ) -> TypeId {
        let members = self.distributed_types(t);
        let filtered: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| self.equality_overlap(program, m, value_type) == assume_true)
            .collect();
        let kept: Vec<TypeId> = filtered
            .into_iter()
            .map(|m| self.refine_equality_narrowed_member(m, value_type, assume_true))
            .collect();
        self.get_union_type(&kept)
    }

    // After filtering by comparability, replace a widened primitive with the
    // compared literal (Go's `replacePrimitivesWithLiterals` + boolean literals,
    // which are stored as a `false | true` union rather than a `BOOLEAN` flag).
    // Go: internal/checker/flow.go:Checker.replacePrimitivesWithLiterals(1884)
    fn refine_equality_narrowed_member(
        &self,
        member: TypeId,
        value_type: TypeId,
        assume_true: bool,
    ) -> TypeId {
        if !assume_true {
            return member;
        }
        let value = self.get_type(value_type);
        if value.flags().contains(TypeFlags::BOOLEAN_LITERAL) {
            if let Some(LiteralValue::Boolean(v)) = value.literal_value() {
                if member == self.boolean_type() {
                    return if *v {
                        self.regular_true_type()
                    } else {
                        self.regular_false_type()
                    };
                }
            }
        }
        member
    }

    // The nullable-value branch of Go's `narrowTypeByEquality` (4az): when the
    // compared `value_type` is `null`/`undefined`, narrow `t` by the matching
    // `EQ`/`NE` `undefined`/`null` fact (via `getTypeWithFacts`) instead of the
    // literal/subtype overlap. `assume_true` is the already-negation-adjusted
    // truth value; `double_equals` selects the loose (`EQUndefinedOrNull`) vs
    // strict (`EQUndefined`/`EQNull`) facts. Returns `None` for a non-nullable
    // value so the caller falls back to `narrow_type_by_equality`.
    //
    // DEFER(phase-4-checker-4az+): the `getAdjustedTypeWithFacts` adjustments
    // (non-null-assertion-only references, the `unknown`/empty-object
    // recombination). blocked-by: `getAdjustedTypeWithFacts` extras (lib globals,
    // P6).
    // Go: internal/checker/flow.go:Checker.narrowTypeByEquality(549) (nullable branch)
    fn narrow_type_by_equality_to_value(
        &mut self,
        t: TypeId,
        value_type: TypeId,
        double_equals: bool,
        assume_true: bool,
    ) -> Option<TypeId> {
        if !self
            .get_type(value_type)
            .flags()
            .intersects(TypeFlags::NULLABLE)
        {
            return None;
        }
        // Go: outside strictNullChecks a union never carries `null`/`undefined`,
        // so the equality refinement is the identity.
        if !self.strict_null_checks() {
            return Some(t);
        }
        let value_is_null = self.get_type(value_type).flags().contains(TypeFlags::NULL);
        let facts = if double_equals {
            if assume_true {
                TypeFacts::EQ_UNDEFINED_OR_NULL
            } else {
                TypeFacts::NE_UNDEFINED_OR_NULL
            }
        } else if value_is_null {
            if assume_true {
                TypeFacts::EQ_NULL
            } else {
                TypeFacts::NE_NULL
            }
        } else if assume_true {
            TypeFacts::EQ_UNDEFINED
        } else {
            TypeFacts::NE_UNDEFINED
        };
        Some(self.get_type_with_facts(t, facts))
    }

    /// Narrows a union `t` by an `in` guard (`"prop" in x`).
    ///
    /// With `assume_true`, keeps constituents that have the property; with
    /// `assume_false`, keeps those that lack it.
    ///
    /// DEFER(phase-4-checker-4g): `instanceof` narrowing (needs the constructor's
    /// prototype/instance type) and index-signature-aware `in`.
    /// blocked-by: constructor/instance types + the global `Function` type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// // A primitive has no own properties, so `in` keeps nothing on the true side.
    /// let s = c.string_type();
    /// assert_eq!(c.narrow_type_by_in(s, "x", true), c.never_type());
    /// ```
    ///
    /// Side effects: may allocate a union.
    // Go: internal/checker/flow.go:Checker.narrowTypeByInKeyword
    pub fn narrow_type_by_in(
        &mut self,
        t: TypeId,
        property_name: &str,
        assume_true: bool,
    ) -> TypeId {
        let members = self.distributed_types(t);
        let kept: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| {
                let has = crate::core::declared_types::get_property_of_type(self, m, property_name)
                    .is_some();
                has == assume_true
            })
            .collect();
        self.get_union_type(&kept)
    }

    // Reports whether two types can be `===`-equal (4f subset): literals compare
    // by value, everything else by subtyping in either direction.
    // Go: internal/checker/flow.go:Checker.narrowTypeByEquality (overlap test)
    fn equality_overlap(&mut self, program: &dyn BoundProgram, a: TypeId, b: TypeId) -> bool {
        let va = self.get_type(a).literal_value().cloned();
        let vb = self.get_type(b).literal_value().cloned();
        if let (Some(va), Some(vb)) = (va, vb) {
            return va == vb;
        }
        self.is_type_subtype_of(program, a, b) || self.is_type_subtype_of(program, b, a)
    }

    /// Computes the narrowed type of `reference` at its control-flow position by
    /// walking the flow-node graph from the reference back to the start.
    ///
    /// Conditions (`if`) narrow the antecedent type via the 4f narrowing
    /// primitives; branch labels union their antecedents. `declared_type` is the
    /// reference's statically declared type (computed by the caller, e.g. via
    /// `get_type_of_symbol`), used at the start and as the loop/recursion guard.
    ///
    /// DEFER(phase-4-checker-4g): assignment/array-mutation/call flow,
    /// switch-clause narrowing, loop fixpoints, `&&`/`||` flow, and `x === <expr>`
    /// discriminants (the last needs the expression checker for the value type).
    /// blocked-by: expression checking + `TypeFacts`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, r: tsgo_ast::NodeId, d: tsgo_checker::TypeId) {
    /// let _ = c.get_flow_type_of_reference(p, r, d);
    /// # }
    /// ```
    ///
    /// Side effects: may allocate unions; populates the relation cache.
    // Go: internal/checker/flow.go:Checker.getFlowTypeOfReference
    pub fn get_flow_type_of_reference(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        declared_type: TypeId,
    ) -> TypeId {
        match program.flow_node_of(reference) {
            None => declared_type,
            Some(flow) => {
                let mut cache: FxHashMap<FlowNodeId, TypeId> = FxHashMap::default();
                self.get_type_at_flow_node(program, reference, declared_type, flow, &mut cache)
            }
        }
    }

    /// Walks one flow node for `reference` and returns the narrowed type (partial
    /// port of Go's `getTypeAtFlowNode`).
    ///
    /// Side effects: may allocate unions and populate `cache`.
    // Go: internal/checker/flow.go:Checker.getTypeAtFlowNode(117)
    pub(crate) fn get_type_at_flow_node(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        declared: TypeId,
        flow: FlowNodeId,
        cache: &mut FxHashMap<FlowNodeId, TypeId>,
    ) -> TypeId {
        if let Some(&cached) = cache.get(&flow) {
            return cached;
        }
        // Seed with the declared type to break flow loops; overwritten below.
        cache.insert(flow, declared);
        let fnode = program.flow_node(flow);
        let result = if fnode.flags.contains(FlowFlags::UNREACHABLE) {
            // DEFER(phase-4-checker-4g): unreachable code yields `never`.
            declared
        } else if fnode.flags.contains(FlowFlags::START) {
            declared
        } else if fnode.flags.intersects(FlowFlags::CONDITION) {
            let assume_true = fnode.flags.contains(FlowFlags::TRUE_CONDITION);
            let ante_type = match fnode.antecedent {
                Some(a) => self.get_type_at_flow_node(program, reference, declared, a, cache),
                None => declared,
            };
            match fnode.node {
                Some(expr) => {
                    self.narrow_type_at_condition(program, reference, ante_type, expr, assume_true)
                }
                None => ante_type,
            }
        } else if fnode.flags.contains(FlowFlags::ASSIGNMENT) {
            match self.get_type_at_flow_assignment(program, reference, declared, fnode.node) {
                Some(t) => t,
                None => match fnode.antecedent {
                    Some(a) => self.get_type_at_flow_node(program, reference, declared, a, cache),
                    None => declared,
                },
            }
        } else if fnode.flags.contains(FlowFlags::SWITCH_CLAUSE) {
            let ante_type = match fnode.antecedent {
                Some(a) => self.get_type_at_flow_node(program, reference, declared, a, cache),
                None => declared,
            };
            match program.flow_switch_clause_data(flow) {
                Some(data) => {
                    self.narrow_type_at_switch_clause(program, reference, ante_type, data)
                }
                None => ante_type,
            }
        } else if fnode.flags.intersects(FlowFlags::LABEL) {
            let mut types: Vec<TypeId> = Vec::new();
            let mut list = fnode.antecedents;
            while let Some(lid) = list {
                let cell = program.flow_list(lid);
                if let Some(a) = cell.flow {
                    types.push(self.get_type_at_flow_node(program, reference, declared, a, cache));
                }
                list = cell.next;
            }
            if types.is_empty() {
                declared
            } else {
                self.get_union_type(&types)
            }
        } else {
            // DEFER(phase-4-checker-4g): assignment/call/array-mutation/switch flow.
            match fnode.antecedent {
                Some(a) => self.get_type_at_flow_node(program, reference, declared, a, cache),
                None => declared,
            }
        };
        cache.insert(flow, result);
        result
    }

    // Narrows `t` by the truth value of a condition `expr` for `reference`.
    // Go: internal/checker/flow.go:Checker.narrowType (4f subset)
    fn narrow_type_at_condition(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        t: TypeId,
        expr: NodeId,
        assume_true: bool,
    ) -> TypeId {
        let expr = skip_parentheses(program, expr);
        match program.arena().kind(expr) {
            Kind::Identifier => {
                if !self.is_matching_reference(program, reference, expr)
                    && self.flow_inline_level < 5
                {
                    if let Some(sym) =
                        resolve_name(program, expr, program.arena().text(expr), SymbolFlags::VALUE, false, None)
                    {
                        if self.is_constant_variable(program, sym) {
                            let sym_rec = program.symbol(sym);
                            if let Some(decl) = sym_rec.value_declaration {
                                if program.arena().kind(decl) == Kind::VariableDeclaration {
                                    let NodeData::VariableDeclaration(vd) =
                                        program.arena().data(decl)
                                    else {
                                        return t;
                                    };
                                    if vd.type_node.is_none() {
                                        if let Some(init) = vd.initializer {
                                            if self.is_constant_reference(program, reference) {
                                                self.flow_inline_level += 1;
                                                let result = self.narrow_type_at_condition(
                                                    program, reference, t, init, assume_true,
                                                );
                                                self.flow_inline_level -= 1;
                                                return result;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if self.is_matching_reference(program, reference, expr) {
                    self.narrow_type_by_truthiness(t, assume_true)
                } else {
                    t
                }
            }
            Kind::BinaryExpression => {
                self.narrow_type_by_binary(program, reference, t, expr, assume_true)
            }
            Kind::PrefixUnaryExpression => {
                let NodeData::PrefixUnaryExpression(d) = program.arena().data(expr) else {
                    return t;
                };
                if d.operator == Kind::ExclamationToken {
                    return self.narrow_type_at_condition(
                        program,
                        reference,
                        t,
                        d.operand,
                        !assume_true,
                    );
                }
                t
            }
            Kind::CallExpression => {
                self.narrow_type_by_call_expression(program, reference, t, expr, assume_true)
            }
            // DEFER(phase-4-checker-4g): `&&`/`||` flow.
            _ => t,
        }
    }

    // Narrows by a type-predicate call used as a condition (`if (isT(x))`).
    // Go: internal/checker/flow.go:Checker.narrowTypeByCallExpression(437)
    fn narrow_type_by_call_expression(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        t: TypeId,
        call_expression: NodeId,
        assume_true: bool,
    ) -> TypeId {
        if !self.has_matching_argument(program, reference, call_expression) {
            return t;
        }
        let Some(signature) = self.get_effects_signature(program, call_expression) else {
            return t;
        };
        let Some(predicate) = self
            .get_type_predicate_of_signature(signature)
            .cloned()
        else {
            return t;
        };
        self.narrow_type_by_type_predicate(
            program,
            reference,
            t,
            &predicate,
            call_expression,
            assume_true,
        )
    }

    // Reports whether `reference` matches any argument of `call_expression`.
    // Go: internal/checker/flow.go:Checker.hasMatchingArgument
    fn has_matching_argument(
        &self,
        program: &dyn BoundProgram,
        reference: NodeId,
        call_expression: NodeId,
    ) -> bool {
        let NodeData::CallExpression(d) = program.arena().data(call_expression) else {
            return false;
        };
        d.arguments
            .nodes
            .iter()
            .any(|&arg| self.is_matching_reference(program, reference, arg))
    }

    // Returns the call signature whose type-predicate effects apply to `call`.
    // Go: internal/checker/flow.go:Checker.getEffectsSignature(2024)
    fn get_effects_signature(
        &mut self,
        program: &dyn BoundProgram,
        call_expression: NodeId,
    ) -> Option<super::signatures::SignatureId> {
        let NodeData::CallExpression(d) = program.arena().data(call_expression) else {
            return None;
        };
        let func_type = self.check_non_null_expression(program, d.expression);
        let signatures = self.get_signatures_of_type(func_type);
        let signature = *signatures.first()?;
        if self.get_type_predicate_of_signature(signature).is_some() {
            Some(signature)
        } else {
            None
        }
    }

    // Returns the resolved type predicate of `signature`, if any.
    // Go: internal/checker/relater.go:Checker.getTypePredicateOfSignature(2013)
    pub(crate) fn get_type_predicate_of_signature(
        &self,
        signature: super::signatures::SignatureId,
    ) -> Option<&TypePredicateInfo> {
        self.signature(signature)
            .resolved_type_predicate
            .as_ref()
    }

    // Narrows by a binary condition; 4f handles `typeof x === "<name>"`.
    // Go: internal/checker/flow.go:Checker.narrowTypeByBinaryExpression (subset)
    fn narrow_type_by_binary(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        t: TypeId,
        expr: NodeId,
        assume_true: bool,
    ) -> TypeId {
        let arena = program.arena();
        let (left, op_token, right) = match arena.data(expr) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            _ => return t,
        };
        let op = arena.kind(op_token);
        if op == Kind::InKeyword {
            if let NodeData::BinaryExpression(d) = arena.data(expr) {
                let target = d.right;
                if self.is_matching_reference(program, reference, target) {
                    let value_type = self.check_expression(program, d.left);
                    if self
                        .get_type(value_type)
                        .flags()
                        .contains(TypeFlags::STRING_LITERAL)
                    {
                        let name = super::late_binding::get_property_name_from_type(self, value_type);
                        return self.narrow_type_by_in(t, &name, assume_true);
                    }
                    if arena.kind(d.left) == Kind::StringLiteral {
                        let name = arena.text(d.left).to_string();
                        return self.narrow_type_by_in(t, &name, assume_true);
                    }
                }
            }
            return t;
        }
        if op == Kind::CommaToken {
            return self.narrow_type_at_condition(program, reference, t, right, assume_true);
        }
        if op == Kind::AmpersandAmpersandToken {
            if assume_true {
                let t_left =
                    self.narrow_type_at_condition(program, reference, t, left, true);
                return self.narrow_type_at_condition(program, reference, t_left, right, true);
            }
            let t_left =
                self.narrow_type_at_condition(program, reference, t, left, false);
            let t_right =
                self.narrow_type_at_condition(program, reference, t, right, false);
            return self.get_union_type(&[t_left, t_right]);
        }
        if op == Kind::BarBarToken {
            if assume_true {
                let t_left =
                    self.narrow_type_at_condition(program, reference, t, left, true);
                let t_right =
                    self.narrow_type_at_condition(program, reference, t, right, true);
                return self.get_union_type(&[t_left, t_right]);
            }
            let t_left =
                self.narrow_type_at_condition(program, reference, t, left, false);
            return self.narrow_type_at_condition(program, reference, t_left, right, false);
        }
        let is_equality = matches!(
            op,
            Kind::EqualsEqualsEqualsToken
                | Kind::EqualsEqualsToken
                | Kind::ExclamationEqualsEqualsToken
                | Kind::ExclamationEqualsToken
        );
        if !is_equality {
            // DEFER(phase-4-checker-4g): `instanceof` binary flow.
            return t;
        }
        let negated = matches!(
            op,
            Kind::ExclamationEqualsToken | Kind::ExclamationEqualsEqualsToken
        );
        let effective = assume_true != negated;
        // Loose (`==`/`!=`) operators fold `null` and `undefined` together; strict
        // (`===`/`!==`) keep them distinct (Go's `doubleEquals`).
        let double_equals = matches!(op, Kind::EqualsEqualsToken | Kind::ExclamationEqualsToken);
        // `typeof x === "name"`
        if arena.kind(left) == Kind::TypeOfExpression && arena.kind(right) == Kind::StringLiteral {
            if let NodeData::TypeOfExpression(d) = arena.data(left) {
                let operand = d.expression;
                if self.is_matching_reference(program, reference, operand) {
                    let name = arena.text(right).to_string();
                    return self.narrow_type_by_typeof(program, t, &name, effective);
                }
            }
        }
        // `x === value` / `value === x`: narrow by the value's type (4g wires
        // the expression checker to type the value operand). A `null`/`undefined`
        // value takes the fact-based nullable branch (4az).
        if self.is_matching_reference(program, reference, left) {
            let value_type = self.check_expression(program, right);
            if let Some(narrowed) =
                self.narrow_type_by_equality_to_value(t, value_type, double_equals, effective)
            {
                return narrowed;
            }
            return self.narrow_type_by_equality(program, t, value_type, effective);
        }
        if self.is_matching_reference(program, reference, right) {
            let value_type = self.check_expression(program, left);
            if let Some(narrowed) =
                self.narrow_type_by_equality_to_value(t, value_type, double_equals, effective)
            {
                return narrowed;
            }
            return self.narrow_type_by_equality(program, t, value_type, effective);
        }
        // Discriminated-union narrowing (`obj.kind === "a"`): neither side
        // matches the reference directly, but one side is a property access
        // `ref.kind` on a literal discriminant property of the union `t`. Go
        // routes through `getDiscriminantPropertyAccess` ->
        // `narrowTypeByDiscriminantProperty`.
        // DEFER(phase-4-checker-later): `getDiscriminantPropertyAccess`'s
        // const-alias / binding-pattern candidate forms and the optional-chain
        // containment branch. blocked-by: alias-reference matching + optional
        // chains.
        if let Some(access) = self.get_discriminant_property_access(program, reference, left, t) {
            let value_type = self.check_expression(program, right);
            return self
                .narrow_type_by_discriminant_property(program, t, access, value_type, effective);
        }
        if let Some(access) = self.get_discriminant_property_access(program, reference, right, t) {
            let value_type = self.check_expression(program, left);
            return self
                .narrow_type_by_discriminant_property(program, t, access, value_type, effective);
        }
        if is_boolean_literal(program, right) && !is_access_expression(program, left) {
            return self.narrow_type_by_boolean_comparison(
                program, reference, t, left, right, op, assume_true,
            );
        }
        if is_boolean_literal(program, left) && !is_access_expression(program, right) {
            return self.narrow_type_by_boolean_comparison(
                program, reference, t, right, left, op, assume_true,
            );
        }
        t
    }

    // Narrows by `expr === true` / `expr !== false` style comparisons (Go routes
    // these through truthiness rather than literal equality when the boolean
    // literal is not paired with a direct reference match).
    // Go: internal/checker/flow.go:Checker.narrowTypeByBooleanComparison(786)
    fn narrow_type_by_boolean_comparison(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        t: TypeId,
        expr: NodeId,
        bool_value: NodeId,
        operator: Kind,
        assume_true: bool,
    ) -> TypeId {
        let bool_is_true = program.arena().kind(bool_value) == Kind::TrueKeyword;
        let is_inequality = matches!(
            operator,
            Kind::ExclamationEqualsEqualsToken | Kind::ExclamationEqualsToken
        );
        let effective = (assume_true != bool_is_true) != !is_inequality;
        if self.is_matching_reference(program, reference, expr) {
            self.narrow_type_by_truthiness(t, effective)
        } else {
            t
        }
    }

    // Returns the property-access node `ref.prop` (`left`/`right` of an equality)
    // when it is a candidate discriminant-property access for `reference` on the
    // union `t`: the access's object must match the reference and `prop` must be
    // a literal discriminant property of `t` (Go's `getDiscriminantPropertyAccess`
    // + `getCandidateDiscriminantPropertyAccess`).
    //
    // DEFER(phase-4-checker-later): the const-alias (`const k = obj.kind`) and
    // destructuring (`const { kind } = obj`) candidate forms, and using the
    // declared union type when the computed type isn't a union subset.
    // blocked-by: alias/binding-element reference matching.
    // Go: internal/checker/flow.go:Checker.getDiscriminantPropertyAccess(1408)
    fn get_discriminant_property_access(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        expr: NodeId,
        t: TypeId,
    ) -> Option<NodeId> {
        if !self.get_type(t).flags().contains(TypeFlags::UNION) {
            return None;
        }
        // The candidate access: a property/element access whose object matches
        // the reference (Go's `getCandidateDiscriminantPropertyAccess`).
        let object = match program.arena().data(expr) {
            NodeData::PropertyAccessExpression(d) => d.expression,
            NodeData::ElementAccessExpression(d) => d.expression,
            _ => return None,
        };
        if !self.is_matching_reference(program, reference, object) {
            return None;
        }
        let name = self.get_accessed_property_name(program, expr)?;
        if self.is_discriminant_property(program, t, &name) {
            Some(expr)
        } else {
            None
        }
    }

    // Returns the accessed property name of an access expression (Go's
    // `getAccessedPropertyName`, reachable subset: property access only).
    //
    // DEFER(phase-4-checker-later): element-access (`obj["k"]`), binding-element,
    // and parameter forms. blocked-by: element-access name extraction + binding
    // destructuring names.
    // Go: internal/checker/flow.go:Checker.getAccessedPropertyName(1699)
    fn get_accessed_property_name(
        &self,
        program: &dyn BoundProgram,
        access: NodeId,
    ) -> Option<String> {
        match program.arena().data(access) {
            NodeData::PropertyAccessExpression(d) => Some(program.arena().text(d.name).to_string()),
            NodeData::ElementAccessExpression(d) => {
                if program.arena().kind(d.argument_expression) == Kind::StringLiteral {
                    Some(program.arena().text(d.argument_expression).to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // Reports whether `symbol` is a `const` variable (Go's `isConstantVariable`).
    // Go: internal/checker/utilities.go:isConstantVariable
    fn is_constant_variable(&self, program: &dyn BoundProgram, symbol: SymbolId) -> bool {
        let sym = program.symbol(symbol);
        sym.flags.intersects(SymbolFlags::VARIABLE)
            && sym.value_declaration.is_some_and(|decl| {
                super::declared_types::combined_node_flags(program, decl)
                    .intersects(NodeFlags::CONSTANT)
            })
    }

    // Reports whether `node` is a reference that flow narrowing may refine via
    // const-alias inlining (Go's `isConstantReference` subset: identifiers only).
    // Go: internal/checker/flow.go:Checker.isConstantReference
    fn is_constant_reference(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        program.arena().kind(node) == Kind::Identifier
    }

    // Reports whether `name` is a literal discriminant property of the union
    // `t`: a synthesized union property whose per-constituent types are
    // non-uniform and at least one is a literal (Go's `isDiscriminantProperty`
    // gating on `CheckFlagsNonUniformAndLiteral`).
    //
    // Non-uniformity is compared by literal value (not type id), because the
    // port does not intern literal types — two `"a"` occurrences are distinct
    // ids but the same discriminant value.
    //
    // DEFER(phase-4-checker-later): the `!isGenericType(prop type)` exclusion
    // and the `HasNeverType` interaction. blocked-by: generic-type detection.
    // Go: internal/checker/relater.go:Checker.isDiscriminantProperty(1084)
    pub(crate) fn is_discriminant_property(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        name: &str,
    ) -> bool {
        if !self.get_type(t).flags().contains(TypeFlags::UNION) {
            return false;
        }
        // Only a synthesized (synthetic) union property is a candidate; a
        // property contributed by a single symbol across all constituents is
        // not (it cannot be non-uniform).
        let prop = match crate::core::declared_types::get_property_of_type(self, t, name) {
            Some(p) => p,
            None => return false,
        };
        if !crate::core::is_synthesized_symbol(prop) {
            return false;
        }
        let members = self.distributed_types(t);
        let mut first: Option<TypeId> = None;
        let mut non_uniform = false;
        let mut has_literal = false;
        for m in members {
            let pt = match crate::core::declared_types::get_type_of_property_of_type(
                self, program, m, name,
            ) {
                Some(pt) => pt,
                None => continue,
            };
            match first {
                None => first = Some(pt),
                Some(f) => {
                    if pt != f && !self.types_same_literal_value(f, pt) {
                        non_uniform = true;
                    }
                }
            }
            if self.is_literal_type(pt) {
                has_literal = true;
            }
        }
        non_uniform && has_literal
    }

    // Reports whether `a` and `b` are literal types carrying the same value,
    // standing in for Go's literal interning when checking discriminant
    // uniformity.
    // Go: internal/checker/relater.go:createUnionOrIntersectionProperty (uniform check)
    fn types_same_literal_value(&self, a: TypeId, b: TypeId) -> bool {
        match (
            self.get_type(a).literal_value(),
            self.get_type(b).literal_value(),
        ) {
            (Some(va), Some(vb)) => va == vb,
            _ => false,
        }
    }

    // Narrows a union `t` by a discriminant-property equality (`obj.kind === v`).
    // The 4az equality dispatch narrows the property type, then the union is
    // filtered to the constituents whose own discriminant property is comparable
    // to the narrowed property type (Go's `narrowTypeByDiscriminantProperty` ->
    // `narrowTypeByDiscriminant` fallback, the equality closure).
    //
    // DEFER(phase-4-checker-later): the `getKeyPropertyName` fast path (only
    // taken for unions with >= 10 constituents), the optional-chain / non-null
    // `removeNullable` adjustment, and the `getTypeOfPropertyOrIndexSignatureOf
    // Type` index-signature fallback. blocked-by: key-property maps + optional
    // chains + index-signature property typing.
    // Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty(683) /
    //     narrowTypeByDiscriminant(706)
    fn narrow_type_by_discriminant_property(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        access: NodeId,
        value_type: TypeId,
        assume_true: bool,
    ) -> TypeId {
        let prop_name = match self.get_accessed_property_name(program, access) {
            Some(n) => n,
            None => return t,
        };
        let prop_type = match crate::core::declared_types::get_type_of_property_of_type(
            self, program, t, &prop_name,
        ) {
            Some(pt) => pt,
            None => return t,
        };
        let narrowed_prop_type =
            self.narrow_type_by_equality(program, prop_type, value_type, assume_true);
        let narrowed_is_never = self
            .get_type(narrowed_prop_type)
            .flags()
            .contains(TypeFlags::NEVER);
        let unknown = self.unknown_type();
        let members = self.distributed_types(t);
        let mut kept: Vec<TypeId> = Vec::new();
        for m in members {
            let discriminant = crate::core::declared_types::get_type_of_property_of_type(
                self, program, m, &prop_name,
            )
            .unwrap_or(unknown);
            let discriminant_never = self
                .get_type(discriminant)
                .flags()
                .contains(TypeFlags::NEVER);
            if !discriminant_never
                && !narrowed_is_never
                && self.are_types_comparable(program, narrowed_prop_type, discriminant)
            {
                kept.push(m);
            }
        }
        self.get_union_type(&kept)
    }

    // Narrows `t` at a `switch`-clause flow node for `reference`. 4t subset: a
    // discriminant `switch (x)` where the switch expression matches the
    // reference directly.
    // Go: internal/checker/flow.go:Checker.getTypeAtSwitchClause
    fn narrow_type_at_switch_clause(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        t: TypeId,
        data: FlowSwitchClauseData,
    ) -> TypeId {
        let switch_stmt = match data.switch_statement {
            Some(s) => s,
            None => return t,
        };
        let expr = match program.arena().data(switch_stmt) {
            NodeData::SwitchStatement(d) => d.expression,
            _ => return t,
        };
        if self.is_matching_reference(program, reference, expr) {
            return self.narrow_type_by_switch_on_discriminant(program, t, &data);
        }
        if program.arena().kind(expr) == Kind::TypeOfExpression {
            let NodeData::TypeOfExpression(d) = program.arena().data(expr) else {
                return t;
            };
            if self.is_matching_reference(program, reference, d.expression) {
                return self.narrow_type_by_switch_on_typeof(program, t, switch_stmt, &data);
            }
        }
        if let Some(access) = self.get_discriminant_property_access(program, reference, expr, t) {
            return self.narrow_type_by_switch_on_discriminant_property(program, t, access, &data);
        }
        // DEFER(phase-4-checker-4u): `switch (true)` and optional-chain
        // containment. blocked-by: optional-chain reference matching.
        t
    }

    // Narrows a union `t` at a `switch (ref.prop)` clause by narrowing the
    // discriminant property with the case-clause types, then filtering union
    // constituents (Go's `narrowTypeBySwitchOnDiscriminantProperty` fallback).
    // Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminantProperty(1203)
    fn narrow_type_by_switch_on_discriminant_property(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        access: NodeId,
        data: &FlowSwitchClauseData,
    ) -> TypeId {
        let prop_name = match self.get_accessed_property_name(program, access) {
            Some(n) => n,
            None => return t,
        };
        let prop_type = match crate::core::declared_types::get_type_of_property_of_type(
            self, program, t, &prop_name,
        ) {
            Some(pt) => pt,
            None => return t,
        };
        let narrowed_prop_type =
            self.narrow_type_by_switch_on_discriminant(program, prop_type, data);
        let narrowed_is_never = self
            .get_type(narrowed_prop_type)
            .flags()
            .contains(TypeFlags::NEVER);
        let unknown = self.unknown_type();
        let members = self.distributed_types(t);
        let mut kept: Vec<TypeId> = Vec::new();
        for m in members {
            let discriminant = crate::core::declared_types::get_type_of_property_of_type(
                self, program, m, &prop_name,
            )
            .unwrap_or(unknown);
            let discriminant_never = self
                .get_type(discriminant)
                .flags()
                .contains(TypeFlags::NEVER);
            if !discriminant_never
                && !narrowed_is_never
                && self.are_types_comparable(program, narrowed_prop_type, discriminant)
            {
                kept.push(m);
            }
        }
        self.get_union_type(&kept)
    }

    // Narrows `t` at a `switch (typeof ref)` clause. Non-default clauses union
    // the `narrow_type_by_typeof` result for each witness in the range; the
    // default clause keeps constituents not narrowed by any other clause's
    // witness (Go's `narrowTypeBySwitchOnTypeOf`).
    // Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTypeOf
    fn narrow_type_by_switch_on_typeof(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        switch_stmt: NodeId,
        data: &FlowSwitchClauseData,
    ) -> TypeId {
        let witnesses = match self.get_switch_clause_typeof_witnesses(program, switch_stmt) {
            Some(w) => w,
            None => return t,
        };
        let clauses = self.switch_case_clauses(program, switch_stmt);
        let default_index = clauses.iter().position(|&c| {
            matches!(
                program.arena().data(c),
                NodeData::CaseOrDefaultClause(d) if d.expression.is_none()
            )
        });
        let start = data.clause_start.max(0) as usize;
        let end = (data.clause_end.max(0) as usize).min(witnesses.len());
        let has_default = start == end
            || default_index.is_some_and(|i| i >= start && i < end);
        if has_default {
            return self.narrow_type_by_switch_on_typeof_default(program, t, start, end, &witnesses);
        }
        let mut narrowed = Vec::new();
        for w in &witnesses[start..end] {
            if w.is_empty() {
                narrowed.push(self.never_type());
            } else {
                narrowed.push(self.narrow_type_by_typeof(program, t, w, true));
            }
        }
        self.get_union_type(&narrowed)
    }

    // Default-clause arm of `switch (typeof x)`: keep constituents not captured
    // by any witness outside the current clause range.
    // Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTypeOf (default)
    fn narrow_type_by_switch_on_typeof_default(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        start: usize,
        end: usize,
        witnesses: &[String],
    ) -> TypeId {
        let outside: Vec<&str> = witnesses
            .iter()
            .enumerate()
            .filter(|(i, w)| (*i < start || *i >= end) && !w.is_empty())
            .map(|(_, w)| w.as_str())
            .collect();
        if outside.is_empty() {
            return t;
        }
        let members = self.distributed_types(t);
        let kept: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| {
                outside.iter().all(|&w| {
                    let narrowed = self.narrow_type_by_typeof(program, m, w, true);
                    let is_empty = self
                        .get_type(narrowed)
                        .flags()
                        .contains(TypeFlags::NEVER)
                        || self.distributed_types(narrowed).is_empty();
                    is_empty || narrowed != m
                })
            })
            .collect();
        self.get_union_type(&kept)
    }

    // Returns the `typeof` witness string for each `switch` clause (`""` for
    // `default` or duplicate witnesses; `None` when a case is not a string
    // literal). Mirrors Go's `getSwitchClauseTypeOfWitnesses`.
    // Go: internal/checker/flow.go:Checker.getSwitchClauseTypeOfWitnesses
    fn get_switch_clause_typeof_witnesses(
        &self,
        program: &dyn BoundProgram,
        switch_stmt: NodeId,
    ) -> Option<Vec<String>> {
        let mut witnesses = Vec::new();
        for clause in self.switch_case_clauses(program, switch_stmt) {
            let NodeData::CaseOrDefaultClause(d) = program.arena().data(clause) else {
                return None;
            };
            let Some(expr) = d.expression else {
                witnesses.push(String::new());
                continue;
            };
            if !is_string_literal_like(program, expr) {
                return None;
            }
            let text = program.arena().text(expr).to_string();
            if witnesses.contains(&text) {
                witnesses.push(String::new());
            } else {
                witnesses.push(text);
            }
        }
        Some(witnesses)
    }

    // Returns the case/default clause nodes of `switch_stmt`.
    fn switch_case_clauses(&self, program: &dyn BoundProgram, switch_stmt: NodeId) -> Vec<NodeId> {
        let case_block = match program.arena().data(switch_stmt) {
            NodeData::SwitchStatement(d) => d.case_block,
            _ => return Vec::new(),
        };
        match program.arena().data(case_block) {
            NodeData::CaseBlock(d) => d.clauses.nodes.clone(),
            _ => Vec::new(),
        }
    }

    // Narrows a union `t` by the clause range `[clause_start, clause_end)` of a
    // discriminant `switch`. 4t subset: a union of literals, comparing each
    // constituent against the case-clause types by value (mirroring the
    // equality path, since literal types are not yet interned). The `default`
    // clause receives the complement of all handled cases.
    // Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminant
    fn narrow_type_by_switch_on_discriminant(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        data: &FlowSwitchClauseData,
    ) -> TypeId {
        let switch_types = self.get_switch_clause_types(program, data.switch_statement);
        if switch_types.is_empty() {
            return t;
        }
        let start = data.clause_start.max(0) as usize;
        let end = (data.clause_end.max(0) as usize).min(switch_types.len());
        if start > end {
            return t;
        }
        let never = self.never_type();
        let clause_types: Vec<TypeId> = switch_types[start..end].to_vec();
        let has_default = start == end || clause_types.contains(&never);
        // The case type: constituents comparable to one of this range's clauses.
        let members = self.distributed_types(t);
        let mut case_kept: Vec<TypeId> = Vec::new();
        for &m in &members {
            let mut overlaps = false;
            for &ct in &clause_types {
                if ct != never && self.equality_overlap(program, m, ct) {
                    overlaps = true;
                    break;
                }
            }
            if overlaps {
                case_kept.push(m);
            }
        }
        let case_type = self.get_union_type(&case_kept);
        if !has_default {
            return case_type;
        }
        // The default type: constituents not comparable to ANY switch clause.
        let mut default_kept: Vec<TypeId> = Vec::new();
        for &m in &members {
            let mut handled = false;
            for &ct in &switch_types {
                if ct != never && self.equality_overlap(program, m, ct) {
                    handled = true;
                    break;
                }
            }
            if !handled {
                default_kept.push(m);
            }
        }
        let default_type = self.get_union_type(&default_kept);
        if case_type == never {
            default_type
        } else {
            self.get_union_type(&[case_type, default_type])
        }
    }

    // Returns the type of each `switch` clause expression (`never` for the
    // `default` clause). 4t subset: no per-switch caching.
    // Go: internal/checker/flow.go:Checker.getSwitchClauseTypes / getTypeOfSwitchClause
    fn get_switch_clause_types(
        &mut self,
        program: &dyn BoundProgram,
        switch_statement: Option<NodeId>,
    ) -> Vec<TypeId> {
        let switch_statement = match switch_statement {
            Some(s) => s,
            None => return Vec::new(),
        };
        let case_block = match program.arena().data(switch_statement) {
            NodeData::SwitchStatement(d) => d.case_block,
            _ => return Vec::new(),
        };
        let clauses = match program.arena().data(case_block) {
            NodeData::CaseBlock(d) => d.clauses.nodes.clone(),
            _ => return Vec::new(),
        };
        clauses
            .into_iter()
            .map(|clause| match program.arena().data(clause) {
                NodeData::CaseOrDefaultClause(d) => match d.expression {
                    Some(expr) => self.check_expression(program, expr),
                    None => self.never_type(),
                },
                _ => self.never_type(),
            })
            .collect()
    }

    // Computes the type of `reference` at an `ASSIGNMENT` flow node. Returns
    // `None` when the assignment does not affect the reference (the caller then
    // continues to the antecedent). 4t subset: only a direct identifier match
    // on a union declared type.
    // Go: internal/checker/flow.go:Checker.getTypeAtFlowAssignment
    fn get_type_at_flow_assignment(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        declared: TypeId,
        node: Option<NodeId>,
    ) -> Option<TypeId> {
        let node = node?;
        if self.is_matching_reference(program, reference, node) {
            // DEFER(phase-4-checker-4u): compound assignment base widening,
            // auto/auto-array evolving types, and the unreachable `never` result.
            // blocked-by: evolving-array flow + `unreachableNeverType`.
            let t = declared;
            if self.get_type(t).flags().contains(TypeFlags::UNION) {
                let assigned = self.get_assigned_type(program, node);
                return Some(self.get_assignment_reduced_type(program, t, assigned));
            }
            return Some(t);
        }
        // DEFER(phase-4-checker-4u): `containsMatchingReference` for a dotted-name
        // left-hand part (`x.y` assignment seen while narrowing `x.y.z`) and the
        // `for (const _ in ref)` non-null effect. blocked-by: property/element
        // access reference matching.
        None
    }

    // Returns the type assigned to `node` (an assignment target). 4t subset:
    // the right-hand side of a binary `=` assignment.
    // Go: internal/checker/flow.go:Checker.getAssignedType
    fn get_assigned_type(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let arena = program.arena();
        if let Some(parent) = arena.parent(node) {
            if arena.kind(parent) == Kind::BinaryExpression {
                if let NodeData::BinaryExpression(d) = arena.data(parent) {
                    let right = d.right;
                    return self.check_expression(program, right);
                }
            }
        }
        // DEFER(phase-4-checker-4u): for-in/for-of, destructuring (array/object
        // literal), spread, and delete assignment targets.
        self.error_type
    }

    // Removes constituents of `declared` to which no constituent of `assigned`
    // is assignable (Go's `getAssignmentReducedType`). Gives up (returns
    // `declared`) when the heuristic produces a non-assignable result.
    // Go: internal/checker/flow.go:Checker.getAssignmentReducedTypeWorker
    fn get_assignment_reduced_type(
        &mut self,
        program: &dyn BoundProgram,
        declared: TypeId,
        assigned: TypeId,
    ) -> TypeId {
        if declared == assigned {
            return declared;
        }
        if self.get_type(assigned).flags().contains(TypeFlags::NEVER) {
            return assigned;
        }
        let members = self.distributed_types(declared);
        let filtered: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| self.type_maybe_assignable_to(program, assigned, m))
            .collect();
        let reduced = self.get_union_type(&filtered);
        // DEFER(phase-4-checker-4u): freshen fresh boolean-literal assignments.
        // blocked-by: fresh/regular boolean-literal type pairing.
        if self.is_type_assignable_to(program, assigned, reduced) {
            reduced
        } else {
            declared
        }
    }

    // Reports whether some constituent of `source` is assignable to `target`.
    // Go: internal/checker/flow.go:Checker.typeMaybeAssignableTo
    fn type_maybe_assignable_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        if !self.get_type(source).flags().contains(TypeFlags::UNION) {
            return self.is_type_assignable_to(program, source, target);
        }
        self.distributed_types(source)
            .into_iter()
            .any(|t| self.is_type_assignable_to(program, t, target))
    }

    /// Reports whether `flow` can be reached from the start of the control-flow
    /// graph (Go's `isReachableFlowNode`).
    ///
    /// A node is unreachable when flagged `UNREACHABLE`; a label is reachable if
    /// any antecedent is; otherwise reachability follows the single antecedent.
    /// Back-edges (loops) are guarded by a visited set.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &Checker, p: &P, f: tsgo_ast::flow::FlowNodeId) -> bool {
    /// c.is_reachable_flow_node(p, f)
    /// # }
    /// ```
    ///
    /// Side effects: none.
    // Go: internal/checker/flow.go:Checker.isReachableFlowNode
    pub fn is_reachable_flow_node(&self, program: &dyn BoundProgram, flow: FlowNodeId) -> bool {
        let mut visited = FxHashSet::default();
        self.is_reachable_flow_node_worker(program, flow, &mut visited)
    }

    // Go: internal/checker/flow.go:Checker.isReachableFlowNodeWorker (4f subset)
    fn is_reachable_flow_node_worker(
        &self,
        program: &dyn BoundProgram,
        flow: FlowNodeId,
        visited: &mut FxHashSet<FlowNodeId>,
    ) -> bool {
        if !visited.insert(flow) {
            // A back-edge: this path does not independently reach the start.
            return false;
        }
        let fnode = program.flow_node(flow);
        if fnode.flags.contains(FlowFlags::UNREACHABLE) {
            return false;
        }
        if fnode.flags.contains(FlowFlags::START) {
            return true;
        }
        if fnode.flags.intersects(FlowFlags::LABEL) {
            let mut list = fnode.antecedents;
            while let Some(lid) = list {
                let cell = program.flow_list(lid);
                if let Some(a) = cell.flow {
                    if self.is_reachable_flow_node_worker(program, a, visited) {
                        return true;
                    }
                }
                list = cell.next;
            }
            return false;
        }
        match fnode.antecedent {
            Some(a) => self.is_reachable_flow_node_worker(program, a, visited),
            None => true,
        }
    }

    // Reports whether two reference nodes denote the same declaration (4f: only
    // identifiers, compared by resolved value symbol).
    // Go: internal/checker/flow.go:Checker.isMatchingReference (subset)
    fn is_matching_reference(&self, program: &dyn BoundProgram, a: NodeId, b: NodeId) -> bool {
        let arena = program.arena();
        if arena.kind(a) != Kind::Identifier || arena.kind(b) != Kind::Identifier {
            return false;
        }
        if arena.text(a) != arena.text(b) {
            return false;
        }
        let sa = resolve_name(program, a, arena.text(a), SymbolFlags::VALUE, false, None);
        let sb = resolve_name(program, b, arena.text(b), SymbolFlags::VALUE, false, None);
        sa.is_some() && sa == sb
    }

    /// Returns the internal member name a well-known `Symbol.<name>` computed
    /// property late-binds to (Go's `getPropertyNameForKnownSymbolName`).
    ///
    /// With no global `Symbol` constructor exposing a `unique symbol` property,
    /// this is the fallback form `InternalSymbolNamePrefix + "@" + name`, which
    /// escapes to Go's literal `"__@<name>"` (e.g. `[Symbol.iterator]` binds as
    /// `__@iterator`). The port's prefix is `U+00FE` rather than `__` (see
    /// `INTERNAL_SYMBOL_NAME_PREFIX`), so the raw key is `"\u{FE}@<name>"`.
    ///
    /// DEFER(phase-4-checker-4ah+): the `unique symbol` path — look the property
    /// up on the global `SymbolConstructor` type and use `getPropertyNameFromType`,
    /// which yields the id-suffixed `__@<name>@<id>` form.
    /// blocked-by: unique-ES-symbol type construction (`getESSymbolLikeTypeForNode`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let c = Checker::new();
    /// let name = c.get_property_name_for_known_symbol_name("iterator");
    /// assert!(name.ends_with("@iterator"));
    /// ```
    ///
    /// Side effects: none (pure; the deferred global path would read the program).
    // Go: internal/checker/flow.go:Checker.getPropertyNameForKnownSymbolName
    pub fn get_property_name_for_known_symbol_name(&self, symbol_name: &str) -> String {
        // DEFER(phase-4-checker-4ah+): first try the global `Symbol`
        // constructor's `unique symbol` property (`getTypeOfPropertyOfType` +
        // `getPropertyNameFromType`); fall back to the prefixed name below.
        // blocked-by: unique-ES-symbol type construction.
        let _ = self;
        format!(
            "{}@{}",
            tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_PREFIX,
            symbol_name
        )
    }

    /// Returns the call argument guarded by a type predicate (Go's
    /// `getTypePredicateArgument`).
    ///
    /// Side effects: none (pure AST read).
    // Go: internal/checker/flow.go:Checker.getTypePredicateArgument(2419)
    pub fn get_type_predicate_argument(
        &self,
        program: &dyn BoundProgram,
        parameter_index: i32,
        call_expression: NodeId,
    ) -> Option<NodeId> {
        let NodeData::CallExpression(d) = program.arena().data(call_expression) else {
            return None;
        };
        if parameter_index >= 0 {
            return d.arguments.nodes.get(parameter_index as usize).copied();
        }
        match program.arena().data(d.expression) {
            NodeData::PropertyAccessExpression(access) => Some(access.expression),
            NodeData::ElementAccessExpression(access) => Some(access.expression),
            _ => None,
        }
    }

    /// Narrows `t` when a type-predicate call matches `reference` (partial port
    /// of Go's `narrowTypeByTypePredicate`).
    ///
    /// DEFER(phase-4-checker-4g): optional-chain containment, discriminant
    /// property narrowing, and the full `getNarrowedType` worker.
    /// blocked-by: optional-chain reference matching + derived-type filtering.
    ///
    /// Side effects: may allocate narrowed types.
    // Go: internal/checker/flow.go:Checker.narrowTypeByTypePredicate(309)
    pub fn narrow_type_by_type_predicate(
        &mut self,
        program: &dyn BoundProgram,
        reference: NodeId,
        t: TypeId,
        predicate: &TypePredicateInfo,
        call_expression: NodeId,
        assume_true: bool,
    ) -> TypeId {
        let Some(predicate_type) = predicate.predicate_type else {
            return t;
        };
        let Some(argument) =
            self.get_type_predicate_argument(program, predicate.parameter_index, call_expression)
        else {
            return t;
        };
        if self.is_matching_reference(program, reference, argument) {
            return self.get_narrowed_type_simple(t, predicate_type, assume_true);
        }
        t
    }

    // Simplified `getNarrowedType` for type-predicate matching (true/false arms).
    // Go: internal/checker/flow.go:Checker.getNarrowedType(826)
    fn get_narrowed_type_simple(
        &mut self,
        t: TypeId,
        candidate: TypeId,
        assume_true: bool,
    ) -> TypeId {
        if assume_true {
            if self
                .get_type(t)
                .flags()
                .intersects(TypeFlags::ANY | TypeFlags::UNKNOWN)
            {
                return candidate;
            }
            if t == candidate {
                return candidate;
            }
            return self.get_intersection_type(&[t, candidate]);
        }
        if t == candidate {
            return self.never_type();
        }
        if self.get_type(t).flags().contains(TypeFlags::UNION) {
            let members: Vec<TypeId> = self
                .distributed_types(t)
                .into_iter()
                .filter(|member| *member != candidate)
                .collect();
            return self.get_union_type(&members);
        }
        t
    }

    // Returns the union constituents of `t`, or `[t]` for a non-union.
    // Go: internal/checker/checker.go:Type.Distributed
    pub(crate) fn distributed_types(&self, t: TypeId) -> Vec<TypeId> {
        if self.get_type(t).flags().contains(TypeFlags::UNION) {
            self.get_type(t).union_types().unwrap_or(&[]).to_vec()
        } else {
            vec![t]
        }
    }
}

// Strips outer parenthesized wrappers from `node` (Go's `SkipParentheses` subset).
fn skip_parentheses(program: &dyn BoundProgram, mut node: NodeId) -> NodeId {
    loop {
        if program.arena().kind(node) != Kind::ParenthesizedExpression {
            break;
        }
        node = match program.arena().data(node) {
            NodeData::ParenthesizedExpression(d) => d.expression,
            _ => break,
        };
    }
    node
}

// Reports whether `node` is a string literal usable as a `typeof` switch witness.
fn is_string_literal_like(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().kind(node),
        Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral
    )
}

fn is_boolean_literal(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().kind(node),
        Kind::TrueKeyword | Kind::FalseKeyword
    )
}

fn is_access_expression(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().kind(node),
        Kind::PropertyAccessExpression
            | Kind::ElementAccessExpression
            | Kind::NonNullExpression
    )
}

#[cfg(test)]
#[path = "flow_test.rs"]
mod tests;
