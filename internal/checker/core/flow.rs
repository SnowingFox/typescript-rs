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
use tsgo_ast::flow::{FlowFlags, FlowNodeId};
use tsgo_ast::{Kind, NodeData, NodeId, SymbolFlags};

use super::program::BoundProgram;
use super::symbols::resolve_name;
use super::types::{TypeFlags, TypeId};
use super::Checker;

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
        self.get_type_with_facts(t, facts)
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
        let kept: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| self.equality_overlap(program, m, value_type) == assume_true)
            .collect();
        self.get_union_type(&kept)
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

    // Go: internal/checker/flow.go:FlowState.getTypeAtFlowNode (4f subset)
    fn get_type_at_flow_node(
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
        match program.arena().kind(expr) {
            Kind::Identifier => {
                if self.is_matching_reference(program, reference, expr) {
                    self.narrow_type_by_truthiness(t, assume_true)
                } else {
                    t
                }
            }
            Kind::BinaryExpression => {
                self.narrow_type_by_binary(program, reference, t, expr, assume_true)
            }
            // DEFER(phase-4-checker-4g): parenthesized, prefix `!`, `&&`/`||` flow.
            _ => t,
        }
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
        let is_equality = matches!(
            op,
            Kind::EqualsEqualsEqualsToken
                | Kind::EqualsEqualsToken
                | Kind::ExclamationEqualsEqualsToken
                | Kind::ExclamationEqualsToken
        );
        if !is_equality {
            // DEFER(phase-4-checker-4g): `&&`/`||`/`instanceof`/`in` binary flow.
            return t;
        }
        let negated = matches!(
            op,
            Kind::ExclamationEqualsToken | Kind::ExclamationEqualsEqualsToken
        );
        let effective = assume_true != negated;
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
        // the expression checker to type the value operand).
        if self.is_matching_reference(program, reference, left) {
            let value_type = self.check_expression(program, right);
            return self.narrow_type_by_equality(program, t, value_type, effective);
        }
        if self.is_matching_reference(program, reference, right) {
            let value_type = self.check_expression(program, left);
            return self.narrow_type_by_equality(program, t, value_type, effective);
        }
        // DEFER(phase-4-checker-4h+): discriminant-property narrowing on a key
        // property of a union (`obj.kind === "a"`).
        t
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

#[cfg(test)]
#[path = "flow_test.rs"]
mod tests;
