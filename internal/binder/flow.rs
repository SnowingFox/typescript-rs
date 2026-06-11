//! Control-flow graph construction.
//!
//! Ports the flow half of Go `internal/binder/binder.go`: the flow-node/flow-list
//! allocators, label helpers (`createBranchLabel`, `finishFlowLabel`,
//! `addAntecedent`), the per-statement/expression flow binders, and the
//! narrowing predicates used by `createFlowCondition`.

use tsgo_ast::flow::{
    FlowFlags, FlowListId, FlowNodeId, FlowReduceLabelData, FlowSwitchClauseData,
};
use tsgo_ast::utilities::{is_assignment_operator, is_left_hand_side_expression_kind};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId};

use crate::astquery as q;
use crate::{ActiveLabel, Binder};

impl Binder<'_> {
    // ── Flow node / list allocation and labels ───────────────────────────────

    // Go: internal/binder/binder.go:createLoopLabel
    fn create_loop_label(&mut self) -> FlowNodeId {
        self.new_flow_node(FlowFlags::LOOP_LABEL)
    }

    // Go: internal/binder/binder.go:createBranchLabel
    fn create_branch_label(&mut self) -> FlowNodeId {
        self.new_flow_node(FlowFlags::BRANCH_LABEL)
    }

    // Go: internal/binder/binder.go:createReduceLabel
    fn create_reduce_label(
        &mut self,
        target: FlowNodeId,
        antecedents: Option<FlowListId>,
        antecedent: FlowNodeId,
    ) -> FlowNodeId {
        let id = self.new_flow_node_ex(FlowFlags::REDUCE_LABEL, None, Some(antecedent));
        self.flow_reduce_data.insert(
            id,
            FlowReduceLabelData {
                target: Some(target),
                antecedents,
            },
        );
        id
    }

    // Go: internal/binder/binder.go:newFlowList
    fn new_flow_list(&mut self, head: FlowNodeId, tail: Option<FlowListId>) -> FlowListId {
        let id = FlowListId(self.flow_lists.len() as u32);
        self.flow_lists.push(tsgo_ast::flow::FlowList {
            flow: Some(head),
            next: tail,
        });
        id
    }

    // Go: internal/binder/binder.go:combineFlowLists
    fn combine_flow_lists(
        &mut self,
        head: Option<FlowListId>,
        tail: Option<FlowListId>,
    ) -> Option<FlowListId> {
        match head {
            None => tail,
            Some(h) => {
                let cell = self.flow_lists[h.0 as usize];
                let flow = cell.flow.expect("flow list cell must hold a flow node");
                let next = self.combine_flow_lists(cell.next, tail);
                Some(self.new_flow_list(flow, next))
            }
        }
    }

    // Go: internal/binder/binder.go:setFlowNodeReferenced
    fn set_flow_node_referenced(&mut self, flow: FlowNodeId) {
        let flags = self.flow_nodes[flow.0 as usize].flags;
        if !flags.contains(FlowFlags::REFERENCED) {
            self.flow_nodes[flow.0 as usize].flags |= FlowFlags::REFERENCED;
        } else {
            self.flow_nodes[flow.0 as usize].flags |= FlowFlags::SHARED;
        }
    }

    // Go: internal/binder/binder.go:addAntecedent
    pub(crate) fn add_antecedent(&mut self, label: FlowNodeId, antecedent: FlowNodeId) {
        if self.flow_nodes[antecedent.0 as usize]
            .flags
            .contains(FlowFlags::UNREACHABLE)
        {
            return;
        }
        let mut last: Option<FlowListId> = None;
        let mut cur = self.flow_nodes[label.0 as usize].antecedents;
        while let Some(listid) = cur {
            let cell = self.flow_lists[listid.0 as usize];
            if cell.flow == Some(antecedent) {
                return;
            }
            last = Some(listid);
            cur = cell.next;
        }
        let new_cell = self.new_flow_list(antecedent, None);
        match last {
            None => self.flow_nodes[label.0 as usize].antecedents = Some(new_cell),
            Some(l) => self.flow_lists[l.0 as usize].next = Some(new_cell),
        }
        self.set_flow_node_referenced(antecedent);
    }

    // Go: internal/binder/binder.go:finishFlowLabel
    pub(crate) fn finish_flow_label(&mut self, label: FlowNodeId) -> FlowNodeId {
        match self.flow_nodes[label.0 as usize].antecedents {
            None => self.unreachable_flow,
            Some(listid) => {
                let cell = self.flow_lists[listid.0 as usize];
                if cell.next.is_none() {
                    cell.flow.expect("flow list cell must hold a flow node")
                } else {
                    label
                }
            }
        }
    }

    // Go: internal/binder/binder.go:createFlowCondition
    fn create_flow_condition(
        &mut self,
        flags: FlowFlags,
        antecedent: FlowNodeId,
        expression: Option<NodeId>,
    ) -> FlowNodeId {
        if self.flow_nodes[antecedent.0 as usize]
            .flags
            .contains(FlowFlags::UNREACHABLE)
        {
            return antecedent;
        }
        let expr = match expression {
            None => {
                return if flags.contains(FlowFlags::TRUE_CONDITION) {
                    antecedent
                } else {
                    self.unreachable_flow
                };
            }
            Some(e) => e,
        };
        let kind = self.arena.kind(expr);
        let constant_false = (kind == Kind::TrueKeyword
            && flags.contains(FlowFlags::FALSE_CONDITION))
            || (kind == Kind::FalseKeyword && flags.contains(FlowFlags::TRUE_CONDITION));
        if constant_false
            && !is_expression_of_optional_chain_root(self.arena, expr)
            && !self
                .arena
                .parent(expr)
                .is_some_and(|p| is_nullish_coalesce(self.arena, p))
        {
            return self.unreachable_flow;
        }
        if !is_narrowing_expression(self.arena, expr) {
            return antecedent;
        }
        self.set_flow_node_referenced(antecedent);
        self.new_flow_node_ex(flags, Some(expr), Some(antecedent))
    }

    // Go: internal/binder/binder.go:createFlowMutation
    fn create_flow_mutation(
        &mut self,
        flags: FlowFlags,
        antecedent: FlowNodeId,
        node: NodeId,
    ) -> FlowNodeId {
        self.set_flow_node_referenced(antecedent);
        self.has_flow_effects = true;
        let result = self.new_flow_node_ex(flags, Some(node), Some(antecedent));
        if let Some(ex) = self.current_exception_target {
            self.add_antecedent(ex, result);
        }
        result
    }

    // Go: internal/binder/binder.go:createFlowSwitchClause
    fn create_flow_switch_clause(
        &mut self,
        antecedent: FlowNodeId,
        switch_statement: NodeId,
        clause_start: i32,
        clause_end: i32,
    ) -> FlowNodeId {
        self.set_flow_node_referenced(antecedent);
        let id = self.new_flow_node_ex(FlowFlags::SWITCH_CLAUSE, None, Some(antecedent));
        self.flow_switch_data.insert(
            id,
            FlowSwitchClauseData {
                switch_statement: Some(switch_statement),
                clause_start,
                clause_end,
            },
        );
        id
    }

    // Go: internal/binder/binder.go:createFlowCall
    fn create_flow_call(&mut self, antecedent: FlowNodeId, node: NodeId) -> FlowNodeId {
        self.set_flow_node_referenced(antecedent);
        self.has_flow_effects = true;
        self.new_flow_node_ex(FlowFlags::CALL, Some(node), Some(antecedent))
    }

    // ── Conditional binding ──────────────────────────────────────────────────

    // Go: internal/binder/binder.go:doWithConditionalBranches((*Binder).bind, ...)
    fn do_with_conditional_branches_bind(
        &mut self,
        node: Option<NodeId>,
        true_target: FlowNodeId,
        false_target: FlowNodeId,
    ) {
        let saved_true = self.current_true_target;
        let saved_false = self.current_false_target;
        self.current_true_target = Some(true_target);
        self.current_false_target = Some(false_target);
        if let Some(n) = node {
            self.bind(n);
        }
        self.current_true_target = saved_true;
        self.current_false_target = saved_false;
    }

    // Go: internal/binder/binder.go:bindCondition
    fn bind_condition(
        &mut self,
        node: Option<NodeId>,
        true_target: FlowNodeId,
        false_target: FlowNodeId,
    ) {
        self.do_with_conditional_branches_bind(node, true_target, false_target);
        let skip = node.is_some_and(|n| {
            is_logical_assignment_expression(self.arena, n)
                || is_logical_expression(self.arena, n)
                || (is_optional_chain(self.arena, n) && is_outermost_optional_chain(self.arena, n))
        });
        if !skip {
            let cf = self.current_flow;
            let t = self.create_flow_condition(FlowFlags::TRUE_CONDITION, cf, node);
            self.add_antecedent(true_target, t);
            let f = self.create_flow_condition(FlowFlags::FALSE_CONDITION, cf, node);
            self.add_antecedent(false_target, f);
        }
    }

    // Go: internal/binder/binder.go:bindIterativeStatement
    fn bind_iterative_statement(
        &mut self,
        node: NodeId,
        break_target: FlowNodeId,
        continue_target: FlowNodeId,
    ) {
        let save_break = self.current_break_target;
        let save_continue = self.current_continue_target;
        self.current_break_target = Some(break_target);
        self.current_continue_target = Some(continue_target);
        self.bind(node);
        self.current_break_target = save_break;
        self.current_continue_target = save_continue;
    }

    // Go: internal/binder/binder.go:setContinueTarget
    fn set_continue_target(&mut self, mut node: NodeId, target: FlowNodeId) -> FlowNodeId {
        let mut i = self.active_label_list.len();
        while i > 0
            && self
                .arena
                .parent(node)
                .is_some_and(|p| self.arena.kind(p) == Kind::LabeledStatement)
        {
            i -= 1;
            self.active_label_list[i].continue_target = Some(target);
            node = self.arena.parent(node).unwrap();
        }
        target
    }

    // ── Statement flow binders ───────────────────────────────────────────────

    // Go: internal/binder/binder.go:bindWhileStatement
    pub(crate) fn bind_while_statement(&mut self, node: NodeId) {
        let (cond, body) = match self.arena.data(node) {
            NodeData::WhileStatement(d) => (d.expression, d.statement),
            _ => unreachable!(),
        };
        let loop_label = self.create_loop_label();
        let pre_while = self.set_continue_target(node, loop_label);
        let pre_body = self.create_branch_label();
        let post_while = self.create_branch_label();
        let cf = self.current_flow;
        self.add_antecedent(pre_while, cf);
        self.current_flow = pre_while;
        self.bind_condition(Some(cond), pre_body, post_while);
        self.current_flow = self.finish_flow_label(pre_body);
        self.bind_iterative_statement(body, post_while, pre_while);
        let cf = self.current_flow;
        self.add_antecedent(pre_while, cf);
        self.current_flow = self.finish_flow_label(post_while);
    }

    // Go: internal/binder/binder.go:bindDoStatement
    pub(crate) fn bind_do_statement(&mut self, node: NodeId) {
        let (body, cond) = match self.arena.data(node) {
            NodeData::DoStatement(d) => (d.statement, d.expression),
            _ => unreachable!(),
        };
        let pre_do = self.create_loop_label();
        let branch = self.create_branch_label();
        let pre_condition = self.set_continue_target(node, branch);
        let post_do = self.create_branch_label();
        let cf = self.current_flow;
        self.add_antecedent(pre_do, cf);
        self.current_flow = pre_do;
        self.bind_iterative_statement(body, post_do, pre_condition);
        let cf = self.current_flow;
        self.add_antecedent(pre_condition, cf);
        self.current_flow = self.finish_flow_label(pre_condition);
        self.bind_condition(Some(cond), pre_do, post_do);
        self.current_flow = self.finish_flow_label(post_do);
    }

    // Go: internal/binder/binder.go:bindForStatement
    pub(crate) fn bind_for_statement(&mut self, node: NodeId) {
        let (init, cond, incr, body) = match self.arena.data(node) {
            NodeData::ForStatement(d) => (d.initializer, d.condition, d.incrementor, d.statement),
            _ => unreachable!(),
        };
        let loop_label = self.create_loop_label();
        let pre_loop = self.set_continue_target(node, loop_label);
        let pre_body = self.create_branch_label();
        let pre_incr = self.create_branch_label();
        let post_loop = self.create_branch_label();
        if let Some(i) = init {
            self.bind(i);
        }
        let cf = self.current_flow;
        self.add_antecedent(pre_loop, cf);
        self.current_flow = pre_loop;
        self.bind_condition(cond, pre_body, post_loop);
        self.current_flow = self.finish_flow_label(pre_body);
        self.bind_iterative_statement(body, post_loop, pre_incr);
        let cf = self.current_flow;
        self.add_antecedent(pre_incr, cf);
        self.current_flow = self.finish_flow_label(pre_incr);
        if let Some(i) = incr {
            self.bind(i);
        }
        let cf = self.current_flow;
        self.add_antecedent(pre_loop, cf);
        self.current_flow = self.finish_flow_label(post_loop);
    }

    // Go: internal/binder/binder.go:bindForInOrForOfStatement
    pub(crate) fn bind_for_in_or_for_of_statement(&mut self, node: NodeId) {
        let (await_modifier, init, expr, body) = match self.arena.data(node) {
            NodeData::ForInOrOfStatement(d) => {
                (d.await_modifier, d.initializer, d.expression, d.statement)
            }
            _ => unreachable!(),
        };
        let loop_label = self.create_loop_label();
        let pre_loop = self.set_continue_target(node, loop_label);
        let post_loop = self.create_branch_label();
        self.bind(expr);
        let cf = self.current_flow;
        self.add_antecedent(pre_loop, cf);
        self.current_flow = pre_loop;
        if self.arena.kind(node) == Kind::ForOfStatement {
            if let Some(a) = await_modifier {
                self.bind(a);
            }
        }
        let cf = self.current_flow;
        self.add_antecedent(post_loop, cf);
        self.bind(init);
        if self.arena.kind(init) != Kind::VariableDeclarationList {
            self.bind_assignment_target_flow(init);
        }
        self.bind_iterative_statement(body, post_loop, pre_loop);
        let cf = self.current_flow;
        self.add_antecedent(pre_loop, cf);
        self.current_flow = self.finish_flow_label(post_loop);
    }

    // Go: internal/binder/binder.go:bindIfStatement
    pub(crate) fn bind_if_statement(&mut self, node: NodeId) {
        let (cond, then_stmt, else_stmt) = match self.arena.data(node) {
            NodeData::IfStatement(d) => (d.expression, d.then_statement, d.else_statement),
            _ => unreachable!(),
        };
        let then_label = self.create_branch_label();
        let else_label = self.create_branch_label();
        let post_if = self.create_branch_label();
        self.bind_condition(Some(cond), then_label, else_label);
        self.current_flow = self.finish_flow_label(then_label);
        self.bind(then_stmt);
        let cf = self.current_flow;
        self.add_antecedent(post_if, cf);
        self.current_flow = self.finish_flow_label(else_label);
        if let Some(e) = else_stmt {
            self.bind(e);
        }
        let cf = self.current_flow;
        self.add_antecedent(post_if, cf);
        self.current_flow = self.finish_flow_label(post_if);
    }

    // Go: internal/binder/binder.go:bindReturnStatement
    pub(crate) fn bind_return_statement(&mut self, node: NodeId) {
        let expr = match self.arena.data(node) {
            NodeData::ReturnStatement(d) => d.expression,
            _ => unreachable!(),
        };
        if let Some(e) = expr {
            self.bind(e);
        }
        if let Some(rt) = self.current_return_target {
            let cf = self.current_flow;
            self.add_antecedent(rt, cf);
        }
        self.current_flow = self.unreachable_flow;
        self.has_explicit_return = true;
        self.has_flow_effects = true;
    }

    // Go: internal/binder/binder.go:bindThrowStatement
    pub(crate) fn bind_throw_statement(&mut self, node: NodeId) {
        let expr = match self.arena.data(node) {
            NodeData::ThrowStatement(d) => d.expression,
            _ => unreachable!(),
        };
        self.bind(expr);
        self.current_flow = self.unreachable_flow;
        self.has_flow_effects = true;
    }

    // Go: internal/binder/binder.go:bindBreakStatement
    pub(crate) fn bind_break_statement(&mut self, node: NodeId) {
        let label = match self.arena.data(node) {
            NodeData::BreakStatement(d) => d.label,
            _ => unreachable!(),
        };
        let target = self.current_break_target;
        self.bind_break_or_continue_statement(label, target, true);
    }

    // Go: internal/binder/binder.go:bindContinueStatement
    pub(crate) fn bind_continue_statement(&mut self, node: NodeId) {
        let label = match self.arena.data(node) {
            NodeData::ContinueStatement(d) => d.label,
            _ => unreachable!(),
        };
        let target = self.current_continue_target;
        self.bind_break_or_continue_statement(label, target, false);
    }

    // Go: internal/binder/binder.go:bindBreakOrContinueStatement
    fn bind_break_or_continue_statement(
        &mut self,
        label: Option<NodeId>,
        current_target: Option<FlowNodeId>,
        is_break: bool,
    ) {
        if let Some(l) = label {
            self.bind(l);
            let name = self.arena.text(l).to_string();
            if let Some(idx) = self.find_active_label(&name) {
                self.active_label_list[idx].referenced = true;
                let target = if is_break {
                    Some(self.active_label_list[idx].break_target)
                } else {
                    self.active_label_list[idx].continue_target
                };
                self.bind_break_or_continue_flow(target);
            }
        } else {
            self.bind_break_or_continue_flow(current_target);
        }
    }

    // Go: internal/binder/binder.go:findActiveLabel
    fn find_active_label(&self, name: &str) -> Option<usize> {
        self.active_label_list.iter().rposition(|l| l.name == name)
    }

    // Go: internal/binder/binder.go:bindBreakOrContinueFlow
    fn bind_break_or_continue_flow(&mut self, flow_label: Option<FlowNodeId>) {
        if let Some(fl) = flow_label {
            let cf = self.current_flow;
            self.add_antecedent(fl, cf);
            self.current_flow = self.unreachable_flow;
            self.has_flow_effects = true;
        }
    }

    // Go: internal/binder/binder.go:bindTryStatement
    pub(crate) fn bind_try_statement(&mut self, node: NodeId) {
        let (try_block, catch_clause, finally_block) = match self.arena.data(node) {
            NodeData::TryStatement(d) => (d.try_block, d.catch_clause, d.finally_block),
            _ => unreachable!(),
        };
        let save_return_target = self.current_return_target;
        let save_exception_target = self.current_exception_target;
        let normal_exit_label = self.create_branch_label();
        let return_label = self.create_branch_label();
        let mut exception_label = self.create_branch_label();
        if finally_block.is_some() {
            self.current_return_target = Some(return_label);
        }
        let cf = self.current_flow;
        self.add_antecedent(exception_label, cf);
        self.current_exception_target = Some(exception_label);
        self.bind(try_block);
        let cf = self.current_flow;
        self.add_antecedent(normal_exit_label, cf);
        if let Some(catch) = catch_clause {
            self.current_flow = self.finish_flow_label(exception_label);
            exception_label = self.create_branch_label();
            let cf = self.current_flow;
            self.add_antecedent(exception_label, cf);
            self.current_exception_target = Some(exception_label);
            self.bind(catch);
            let cf = self.current_flow;
            self.add_antecedent(normal_exit_label, cf);
        }
        self.current_return_target = save_return_target;
        self.current_exception_target = save_exception_target;
        if let Some(finally) = finally_block {
            let finally_label = self.create_branch_label();
            let exc_ante = self.flow_nodes[exception_label.0 as usize].antecedents;
            let ret_ante = self.flow_nodes[return_label.0 as usize].antecedents;
            let normal_ante = self.flow_nodes[normal_exit_label.0 as usize].antecedents;
            let combined_exc_ret = self.combine_flow_lists(exc_ante, ret_ante);
            let combined = self.combine_flow_lists(normal_ante, combined_exc_ret);
            self.flow_nodes[finally_label.0 as usize].antecedents = combined;
            self.current_flow = finally_label;
            self.bind(finally);
            if self
                .flow_flags(self.current_flow)
                .contains(FlowFlags::UNREACHABLE)
            {
                self.current_flow = self.unreachable_flow;
            } else {
                if let Some(rt) = self.current_return_target {
                    if ret_ante.is_some() {
                        let cf = self.current_flow;
                        let reduce = self.create_reduce_label(finally_label, ret_ante, cf);
                        self.add_antecedent(rt, reduce);
                    }
                }
                if let Some(et) = self.current_exception_target {
                    if exc_ante.is_some() {
                        let cf = self.current_flow;
                        let reduce = self.create_reduce_label(finally_label, exc_ante, cf);
                        self.add_antecedent(et, reduce);
                    }
                }
                if normal_ante.is_some() {
                    let cf = self.current_flow;
                    self.current_flow = self.create_reduce_label(finally_label, normal_ante, cf);
                } else {
                    self.current_flow = self.unreachable_flow;
                }
            }
        } else {
            self.current_flow = self.finish_flow_label(normal_exit_label);
        }
    }

    // Go: internal/binder/binder.go:bindSwitchStatement
    pub(crate) fn bind_switch_statement(&mut self, node: NodeId) {
        let (expression, case_block) = match self.arena.data(node) {
            NodeData::SwitchStatement(d) => (d.expression, d.case_block),
            _ => unreachable!(),
        };
        let post_switch_label = self.create_branch_label();
        self.bind(expression);
        let save_break_target = self.current_break_target;
        let save_pre_switch_case_flow = self.pre_switch_case_flow;
        self.current_break_target = Some(post_switch_label);
        self.pre_switch_case_flow = Some(self.current_flow);
        self.bind(case_block);
        let cf = self.current_flow;
        self.add_antecedent(post_switch_label, cf);
        let clauses = match self.arena.data(case_block) {
            NodeData::CaseBlock(d) => d.clauses.nodes.clone(),
            _ => Vec::new(),
        };
        let has_default = clauses
            .iter()
            .any(|&c| self.arena.kind(c) == Kind::DefaultClause);
        if !has_default {
            let pre = self.pre_switch_case_flow.unwrap();
            let clause = self.create_flow_switch_clause(pre, node, 0, 0);
            self.add_antecedent(post_switch_label, clause);
        }
        self.current_break_target = save_break_target;
        self.pre_switch_case_flow = save_pre_switch_case_flow;
        self.current_flow = self.finish_flow_label(post_switch_label);
    }

    // Go: internal/binder/binder.go:bindCaseBlock
    pub(crate) fn bind_case_block(&mut self, node: NodeId) {
        let switch_statement = self.arena.parent(node).unwrap();
        let clauses = match self.arena.data(node) {
            NodeData::CaseBlock(d) => d.clauses.nodes.clone(),
            _ => Vec::new(),
        };
        let switch_expr = match self.arena.data(switch_statement) {
            NodeData::SwitchStatement(d) => d.expression,
            _ => unreachable!(),
        };
        let is_narrowing_switch = self.arena.kind(switch_expr) == Kind::TrueKeyword
            || is_narrowing_expression(self.arena, switch_expr);
        let mut fallthrough_flow = self.unreachable_flow;
        let mut i = 0usize;
        while i < clauses.len() {
            let clause_start = i;
            while self.clause_statement_count(clauses[i]) == 0 && i + 1 < clauses.len() {
                if fallthrough_flow == self.unreachable_flow {
                    self.current_flow = self.pre_switch_case_flow.unwrap();
                }
                self.bind(clauses[i]);
                i += 1;
            }
            let pre_case_label = self.create_branch_label();
            let pre_switch = self.pre_switch_case_flow.unwrap();
            let pre_case_flow = if is_narrowing_switch {
                self.create_flow_switch_clause(
                    pre_switch,
                    switch_statement,
                    clause_start as i32,
                    (i + 1) as i32,
                )
            } else {
                pre_switch
            };
            self.add_antecedent(pre_case_label, pre_case_flow);
            self.add_antecedent(pre_case_label, fallthrough_flow);
            self.current_flow = self.finish_flow_label(pre_case_label);
            let clause = clauses[i];
            self.bind(clause);
            fallthrough_flow = self.current_flow;
            i += 1;
        }
    }

    fn clause_statement_count(&self, clause: NodeId) -> usize {
        match self.arena.data(clause) {
            NodeData::CaseOrDefaultClause(d) => d.statements.nodes.len(),
            _ => 0,
        }
    }

    // Go: internal/binder/binder.go:bindCaseOrDefaultClause
    pub(crate) fn bind_case_or_default_clause(&mut self, node: NodeId) {
        let (expression, statements) = match self.arena.data(node) {
            NodeData::CaseOrDefaultClause(d) => (d.expression, d.statements.nodes.clone()),
            _ => unreachable!(),
        };
        if let Some(e) = expression {
            let save_current_flow = self.current_flow;
            self.current_flow = self.pre_switch_case_flow.unwrap();
            self.bind(e);
            self.current_flow = save_current_flow;
        }
        self.bind_each(&statements);
    }

    // Go: internal/binder/binder.go:bindExpressionStatement
    pub(crate) fn bind_expression_statement(&mut self, node: NodeId) {
        let expr = match self.arena.data(node) {
            NodeData::ExpressionStatement(d) => d.expression,
            _ => unreachable!(),
        };
        self.bind(expr);
        self.maybe_bind_expression_flow_if_call(expr);
    }

    // Go: internal/binder/binder.go:maybeBindExpressionFlowIfCall
    fn maybe_bind_expression_flow_if_call(&mut self, node: NodeId) {
        if let NodeData::CallExpression(d) = self.arena.data(node) {
            let callee = d.expression;
            if self.arena.kind(callee) != Kind::SuperKeyword && is_dotted_name(self.arena, callee) {
                let cf = self.current_flow;
                self.current_flow = self.create_flow_call(cf, node);
            }
        }
    }

    // Go: internal/binder/binder.go:bindLabeledStatement
    pub(crate) fn bind_labeled_statement(&mut self, node: NodeId) {
        let (label, statement) = match self.arena.data(node) {
            NodeData::LabeledStatement(d) => (d.label, d.statement),
            _ => unreachable!(),
        };
        let post_statement_label = self.create_branch_label();
        let name = self.arena.text(label).to_string();
        self.active_label_list.push(ActiveLabel {
            name,
            break_target: post_statement_label,
            continue_target: None,
            referenced: false,
        });
        self.bind(label);
        self.bind(statement);
        let al = self
            .active_label_list
            .pop()
            .expect("labeled statement pushed an active label");
        if !al.referenced {
            self.arena.add_flags(label, NodeFlags::UNREACHABLE);
        }
        let cf = self.current_flow;
        self.add_antecedent(post_statement_label, cf);
        self.current_flow = self.finish_flow_label(post_statement_label);
    }

    // Go: internal/binder/binder.go:bindPrefixUnaryExpressionFlow
    pub(crate) fn bind_prefix_unary_expression_flow(&mut self, node: NodeId) {
        let (operator, operand) = match self.arena.data(node) {
            NodeData::PrefixUnaryExpression(d) => (d.operator, d.operand),
            _ => unreachable!(),
        };
        if operator == Kind::ExclamationToken {
            let save_true_target = self.current_true_target;
            std::mem::swap(
                &mut self.current_true_target,
                &mut self.current_false_target,
            );
            self.bind_each_child(node);
            self.current_false_target = self.current_true_target;
            self.current_true_target = save_true_target;
        } else {
            self.bind_each_child(node);
            if operator == Kind::PlusPlusToken || operator == Kind::MinusMinusToken {
                self.bind_assignment_target_flow(operand);
            }
        }
    }

    // Go: internal/binder/binder.go:bindPostfixUnaryExpressionFlow
    pub(crate) fn bind_postfix_unary_expression_flow(&mut self, node: NodeId) {
        let (operator, operand) = match self.arena.data(node) {
            NodeData::PostfixUnaryExpression(d) => (d.operator, d.operand),
            _ => unreachable!(),
        };
        self.bind_each_child(node);
        if operator == Kind::PlusPlusToken || operator == Kind::MinusMinusToken {
            self.bind_assignment_target_flow(operand);
        }
    }

    // Go: internal/binder/binder.go:bindBinaryExpressionFlow
    pub(crate) fn bind_binary_expression_flow(&mut self, node: NodeId) {
        let (left, operator_token, right) = match self.arena.data(node) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            _ => unreachable!(),
        };
        let operator = self.arena.kind(operator_token);
        if is_logical_or_coalescing_binary_operator(operator)
            || is_logical_or_coalescing_assignment_operator(operator)
        {
            if is_top_level_logical_expression(self.arena, node) {
                let post_expression_label = self.create_branch_label();
                let save_current_flow = self.current_flow;
                let save_has_flow_effects = self.has_flow_effects;
                self.has_flow_effects = false;
                self.bind_logical_like_expression(
                    node,
                    post_expression_label,
                    post_expression_label,
                );
                if self.has_flow_effects {
                    self.current_flow = self.finish_flow_label(post_expression_label);
                } else {
                    self.current_flow = save_current_flow;
                }
                self.has_flow_effects = self.has_flow_effects || save_has_flow_effects;
            } else {
                let t = self.current_true_target.unwrap_or(self.unreachable_flow);
                let f = self.current_false_target.unwrap_or(self.unreachable_flow);
                self.bind_logical_like_expression(node, t, f);
            }
        } else {
            self.bind(left);
            if operator == Kind::CommaToken {
                self.maybe_bind_expression_flow_if_call(left);
            }
            self.bind(operator_token);
            self.bind(right);
            if operator == Kind::CommaToken {
                self.maybe_bind_expression_flow_if_call(right);
            }
            if is_assignment_operator(operator) {
                self.bind_assignment_target_flow(left);
                // ---- T1-E batch 135: array index assignment flow ----
                // Go: internal/binder/binder.go:bindBinaryExpressionFlow(2259)
                if operator == Kind::EqualsToken
                    && !tsgo_ast::utilities::is_assignment_target(self.arena, node)
                    && self.arena.kind(left) == Kind::ElementAccessExpression
                {
                    let elem_expr = match self.arena.data(left) {
                        NodeData::ElementAccessExpression(d) => d.expression,
                        _ => left,
                    };
                    if is_narrowable_operand(self.arena, elem_expr) {
                        let cf = self.current_flow;
                        self.current_flow = self.create_flow_mutation(
                            FlowFlags::ARRAY_MUTATION,
                            cf,
                            node,
                        );
                    }
                }
            }
        }
    }

    // Records push/unshift array-mutation flow after children are bound.
    // Go: internal/binder/binder.go:bindCallExpressionFlow(2444) (mutation tail)
    pub(crate) fn bind_call_expression_array_mutation(&mut self, node: NodeId) {
        let NodeData::CallExpression(d) = self.arena.data(node) else {
            return;
        };
        let expression = d.expression;
        if self.arena.kind(expression) == Kind::SuperKeyword {
            let cf = self.current_flow;
            self.current_flow = self.create_flow_call(cf, node);
        }
        // ---- T1-E batch 135: push/unshift array mutation flow ----
        if self.arena.kind(expression) == Kind::PropertyAccessExpression {
            let NodeData::PropertyAccessExpression(access) = self.arena.data(expression) else {
                return;
            };
            if self.arena.kind(access.name) == Kind::Identifier
                && is_narrowable_operand(self.arena, access.expression)
                && is_push_or_unshift_name(self.arena.text(access.name))
            {
                let cf = self.current_flow;
                self.current_flow =
                    self.create_flow_mutation(FlowFlags::ARRAY_MUTATION, cf, node);
            }
        }
    }

    // Go: internal/binder/binder.go:bindLogicalLikeExpression
    fn bind_logical_like_expression(
        &mut self,
        node: NodeId,
        true_target: FlowNodeId,
        false_target: FlowNodeId,
    ) {
        let (left, operator_token, right) = match self.arena.data(node) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            _ => unreachable!(),
        };
        let op = self.arena.kind(operator_token);
        let pre_right_label = self.create_branch_label();
        if op == Kind::AmpersandAmpersandToken || op == Kind::AmpersandAmpersandEqualsToken {
            self.bind_condition(Some(left), pre_right_label, false_target);
        } else {
            self.bind_condition(Some(left), true_target, pre_right_label);
        }
        self.current_flow = self.finish_flow_label(pre_right_label);
        self.bind(operator_token);
        if is_logical_or_coalescing_assignment_operator(op) {
            self.do_with_conditional_branches_bind(Some(right), true_target, false_target);
            self.bind_assignment_target_flow(left);
            let cf = self.current_flow;
            let t = self.create_flow_condition(FlowFlags::TRUE_CONDITION, cf, Some(node));
            self.add_antecedent(true_target, t);
            let f = self.create_flow_condition(FlowFlags::FALSE_CONDITION, cf, Some(node));
            self.add_antecedent(false_target, f);
        } else {
            self.bind_condition(Some(right), true_target, false_target);
        }
    }

    // Go: internal/binder/binder.go:bindConditionalExpressionFlow
    pub(crate) fn bind_conditional_expression_flow(&mut self, node: NodeId) {
        let (condition, question, when_true, colon, when_false) = match self.arena.data(node) {
            NodeData::ConditionalExpression(d) => (
                d.condition,
                d.question_token,
                d.when_true,
                d.colon_token,
                d.when_false,
            ),
            _ => unreachable!(),
        };
        let true_label = self.create_branch_label();
        let false_label = self.create_branch_label();
        let post_expression_label = self.create_branch_label();
        let save_current_flow = self.current_flow;
        let save_has_flow_effects = self.has_flow_effects;
        self.has_flow_effects = false;
        self.bind_condition(Some(condition), true_label, false_label);
        self.current_flow = self.finish_flow_label(true_label);
        self.bind(question);
        self.bind(when_true);
        let cf = self.current_flow;
        self.add_antecedent(post_expression_label, cf);
        self.current_flow = self.finish_flow_label(false_label);
        self.bind(colon);
        self.bind(when_false);
        let cf = self.current_flow;
        self.add_antecedent(post_expression_label, cf);
        if self.has_flow_effects {
            self.current_flow = self.finish_flow_label(post_expression_label);
        } else {
            self.current_flow = save_current_flow;
        }
        self.has_flow_effects = self.has_flow_effects || save_has_flow_effects;
    }

    // Go: internal/binder/binder.go:bindDestructuringAssignmentFlow
    pub(crate) fn bind_destructuring_assignment_flow(&mut self, node: NodeId) {
        let (left, operator_token, right) = match self.arena.data(node) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            _ => unreachable!(),
        };
        if self.in_assignment_pattern {
            self.in_assignment_pattern = false;
            self.bind(operator_token);
            self.bind(right);
            self.in_assignment_pattern = true;
            self.bind(left);
        } else {
            self.in_assignment_pattern = true;
            self.bind(left);
            self.in_assignment_pattern = false;
            self.bind(operator_token);
            self.bind(right);
        }
        self.bind_assignment_target_flow(left);
    }

    // Go: internal/binder/binder.go:bindAssignmentTargetFlow
    fn bind_assignment_target_flow(&mut self, node: NodeId) {
        match self.arena.kind(node) {
            Kind::ArrayLiteralExpression => {
                let elements = match self.arena.data(node) {
                    NodeData::ArrayLiteralExpression(d) => d.list.nodes.clone(),
                    _ => Vec::new(),
                };
                for e in elements {
                    if self.arena.kind(e) == Kind::SpreadElement {
                        let inner = match self.arena.data(e) {
                            NodeData::SpreadElement(d) => d.expression,
                            _ => continue,
                        };
                        self.bind_assignment_target_flow(inner);
                    } else {
                        self.bind_destructuring_target_flow(e);
                    }
                }
            }
            Kind::ObjectLiteralExpression => {
                let properties = match self.arena.data(node) {
                    NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
                    _ => Vec::new(),
                };
                for p in properties {
                    match self.arena.data(p) {
                        NodeData::PropertyAssignment(d) => {
                            if let Some(init) = d.initializer {
                                self.bind_destructuring_target_flow(init);
                            }
                        }
                        NodeData::ShorthandPropertyAssignment(d) => {
                            let name = d.name;
                            self.bind_assignment_target_flow(name);
                        }
                        NodeData::SpreadAssignment(d) => {
                            let e = d.expression;
                            self.bind_assignment_target_flow(e);
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                if is_narrowable_reference(self.arena, node) {
                    let cf = self.current_flow;
                    self.current_flow = self.create_flow_mutation(FlowFlags::ASSIGNMENT, cf, node);
                }
            }
        }
    }

    // Go: internal/binder/binder.go:bindDestructuringTargetFlow
    fn bind_destructuring_target_flow(&mut self, node: NodeId) {
        if let NodeData::BinaryExpression(d) = self.arena.data(node) {
            if self.arena.kind(d.operator_token) == Kind::EqualsToken {
                let left = d.left;
                self.bind_assignment_target_flow(left);
                return;
            }
        }
        self.bind_assignment_target_flow(node);
    }

    // Go: internal/binder/binder.go:bindVariableDeclarationFlow
    pub(crate) fn bind_variable_declaration_flow(&mut self, node: NodeId) {
        self.bind_each_child(node);
        let has_init = matches!(
            self.arena.data(node),
            NodeData::VariableDeclaration(d) if d.initializer.is_some()
        );
        let grandparent_for_in_of = self
            .arena
            .parent(node)
            .and_then(|p| self.arena.parent(p))
            .is_some_and(|gp| {
                matches!(
                    self.arena.kind(gp),
                    Kind::ForInStatement | Kind::ForOfStatement
                )
            });
        if has_init || grandparent_for_in_of {
            self.bind_initialized_variable_flow(node);
        }
    }

    // Go: internal/binder/binder.go:bindInitializedVariableFlow
    fn bind_initialized_variable_flow(&mut self, node: NodeId) {
        let name = match self.arena.data(node) {
            NodeData::VariableDeclaration(d) => Some(d.name),
            NodeData::BindingElement(d) => d.name,
            _ => None,
        };
        if let Some(name) = name {
            if q::is_binding_pattern(self.arena, name) {
                let elements = match self.arena.data(name) {
                    NodeData::ObjectBindingPattern(d) | NodeData::ArrayBindingPattern(d) => {
                        d.elements.nodes.clone()
                    }
                    _ => Vec::new(),
                };
                for child in elements {
                    self.bind_initialized_variable_flow(child);
                }
                return;
            }
        }
        let cf = self.current_flow;
        self.current_flow = self.create_flow_mutation(FlowFlags::ASSIGNMENT, cf, node);
    }

    // Go: internal/binder/binder.go:bindBindingElementFlow
    pub(crate) fn bind_binding_element_flow(&mut self, node: NodeId) {
        let (dot_dot_dot, property_name, name, initializer) = match self.arena.data(node) {
            NodeData::BindingElement(d) => {
                (d.dot_dot_dot_token, d.property_name, d.name, d.initializer)
            }
            _ => unreachable!(),
        };
        if let Some(t) = dot_dot_dot {
            self.bind(t);
        }
        if let Some(p) = property_name {
            self.bind(p);
        }
        self.bind_initializer(initializer);
        if let Some(n) = name {
            self.bind(n);
        }
    }

    // Go: internal/binder/binder.go:bindParameterFlow
    pub(crate) fn bind_parameter_flow(&mut self, node: NodeId) {
        let (modifiers, dot_dot_dot, question, type_node, initializer, name) =
            match self.arena.data(node) {
                NodeData::ParameterDeclaration(d) => (
                    d.modifiers.as_ref().map(|m| m.list.nodes.clone()),
                    d.dot_dot_dot_token,
                    d.question_token,
                    d.type_node,
                    d.initializer,
                    d.name,
                ),
                _ => unreachable!(),
            };
        if let Some(mods) = modifiers {
            self.bind_each(&mods);
        }
        if let Some(t) = dot_dot_dot {
            self.bind(t);
        }
        if let Some(t) = question {
            self.bind(t);
        }
        if let Some(t) = type_node {
            self.bind(t);
        }
        self.bind_initializer(initializer);
        self.bind(name);
    }

    // Go: internal/binder/binder.go:bindInitializer
    fn bind_initializer(&mut self, node: Option<NodeId>) {
        let node = match node {
            Some(n) => n,
            None => return,
        };
        let entry_flow = self.current_flow;
        self.bind(node);
        if entry_flow == self.unreachable_flow || entry_flow == self.current_flow {
            return;
        }
        let exit_flow = self.create_branch_label();
        self.add_antecedent(exit_flow, entry_flow);
        let cf = self.current_flow;
        self.add_antecedent(exit_flow, cf);
        self.current_flow = self.finish_flow_label(exit_flow);
    }
}

// ── Narrowing predicates (free functions over the arena) ─────────────────────

/// Returns the `.expression` child of an expression node, if it has one.
fn expression_of(arena: &NodeArena, id: NodeId) -> Option<NodeId> {
    match arena.data(id) {
        NodeData::PropertyAccessExpression(d) => Some(d.expression),
        NodeData::ParenthesizedExpression(d)
        | NodeData::NonNullExpression(d)
        | NodeData::TypeOfExpression(d)
        | NodeData::DeleteExpression(d)
        | NodeData::SpreadElement(d) => Some(d.expression),
        NodeData::ElementAccessExpression(d) => Some(d.expression),
        NodeData::CallExpression(d) => Some(d.expression),
        _ => None,
    }
}

// Go: internal/binder/binder.go:isNarrowingExpression
pub(crate) fn is_narrowing_expression(arena: &NodeArena, expr: NodeId) -> bool {
    match arena.kind(expr) {
        Kind::Identifier | Kind::ThisKeyword => true,
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression => {
            contains_narrowable_reference(arena, expr)
        }
        Kind::CallExpression => has_narrowable_argument(arena, expr),
        Kind::ParenthesizedExpression | Kind::NonNullExpression | Kind::TypeOfExpression => {
            expression_of(arena, expr).is_some_and(|e| is_narrowing_expression(arena, e))
        }
        Kind::BinaryExpression => is_narrowing_binary_expression(arena, expr),
        Kind::PrefixUnaryExpression => match arena.data(expr) {
            NodeData::PrefixUnaryExpression(d) => {
                d.operator == Kind::ExclamationToken && is_narrowing_expression(arena, d.operand)
            }
            _ => false,
        },
        _ => false,
    }
}

// Go: internal/binder/binder.go:containsNarrowableReference
pub(crate) fn contains_narrowable_reference(arena: &NodeArena, expr: NodeId) -> bool {
    if is_narrowable_reference(arena, expr) {
        return true;
    }
    if arena.flags(expr).contains(NodeFlags::OPTIONAL_CHAIN)
        && matches!(
            arena.kind(expr),
            Kind::PropertyAccessExpression
                | Kind::ElementAccessExpression
                | Kind::CallExpression
                | Kind::NonNullExpression
        )
    {
        return expression_of(arena, expr).is_some_and(|e| contains_narrowable_reference(arena, e));
    }
    false
}

// Go: internal/binder/binder.go:isNarrowableReference
pub(crate) fn is_narrowable_reference(arena: &NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::Identifier | Kind::ThisKeyword | Kind::SuperKeyword | Kind::MetaProperty => true,
        Kind::PropertyAccessExpression
        | Kind::ParenthesizedExpression
        | Kind::NonNullExpression => {
            expression_of(arena, node).is_some_and(|e| is_narrowable_reference(arena, e))
        }
        Kind::ElementAccessExpression => match arena.data(node) {
            NodeData::ElementAccessExpression(d) => {
                q::is_string_or_numeric_literal_like(arena, d.argument_expression)
                    || (q::is_entity_name_expression(arena, d.argument_expression)
                        && is_narrowable_reference(arena, d.expression))
            }
            _ => false,
        },
        Kind::BinaryExpression => match arena.data(node) {
            NodeData::BinaryExpression(d) => {
                let op = arena.kind(d.operator_token);
                (op == Kind::CommaToken && is_narrowable_reference(arena, d.right))
                    || (is_assignment_operator(op)
                        && is_left_hand_side_expression_kind(arena.kind(d.left)))
            }
            _ => false,
        },
        _ => false,
    }
}

// Go: internal/binder/binder.go:hasNarrowableArgument
fn has_narrowable_argument(arena: &NodeArena, expr: NodeId) -> bool {
    let (callee, arguments) = match arena.data(expr) {
        NodeData::CallExpression(d) => (d.expression, d.arguments.nodes.clone()),
        _ => return false,
    };
    for arg in arguments {
        if contains_narrowable_reference(arena, arg) {
            return true;
        }
    }
    if let NodeData::PropertyAccessExpression(d) = arena.data(callee) {
        if contains_narrowable_reference(arena, d.expression) {
            return true;
        }
    }
    false
}

// Go: internal/binder/binder.go:isNarrowingBinaryExpression
fn is_narrowing_binary_expression(arena: &NodeArena, expr: NodeId) -> bool {
    let (left, operator_token, right) = match arena.data(expr) {
        NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
        _ => return false,
    };
    match arena.kind(operator_token) {
        Kind::EqualsToken
        | Kind::BarBarEqualsToken
        | Kind::AmpersandAmpersandEqualsToken
        | Kind::QuestionQuestionEqualsToken => contains_narrowable_reference(arena, left),
        Kind::EqualsEqualsToken
        | Kind::ExclamationEqualsToken
        | Kind::EqualsEqualsEqualsToken
        | Kind::ExclamationEqualsEqualsToken => {
            let left = skip_parentheses(arena, left);
            let right = skip_parentheses(arena, right);
            is_narrowable_operand(arena, left)
                || is_narrowable_operand(arena, right)
                || is_narrowing_type_of_operands(arena, right, left)
                || is_narrowing_type_of_operands(arena, left, right)
                || (is_boolean_literal(arena, right) && is_narrowing_expression(arena, left))
                || (is_boolean_literal(arena, left) && is_narrowing_expression(arena, right))
        }
        Kind::InstanceOfKeyword => is_narrowable_operand(arena, left),
        Kind::InKeyword => is_narrowing_expression(arena, right),
        Kind::CommaToken => is_narrowing_expression(arena, right),
        _ => false,
    }
}

// Go: internal/binder/binder.go:isNarrowableOperand
fn is_narrowable_operand(arena: &NodeArena, expr: NodeId) -> bool {
    match arena.data(expr) {
        NodeData::ParenthesizedExpression(d) => is_narrowable_operand(arena, d.expression),
        NodeData::BinaryExpression(d) => match arena.kind(d.operator_token) {
            Kind::EqualsToken => is_narrowable_operand(arena, d.left),
            Kind::CommaToken => is_narrowable_operand(arena, d.right),
            _ => contains_narrowable_reference(arena, expr),
        },
        _ => contains_narrowable_reference(arena, expr),
    }
}

// Go: internal/binder/binder.go:isNarrowingTypeOfOperands
fn is_narrowing_type_of_operands(arena: &NodeArena, expr1: NodeId, expr2: NodeId) -> bool {
    matches!(arena.data(expr1), NodeData::TypeOfExpression(_))
        && expression_of(arena, expr1).is_some_and(|e| is_narrowable_operand(arena, e))
        && is_string_literal_like(arena, expr2)
}

fn skip_parentheses(arena: &NodeArena, mut id: NodeId) -> NodeId {
    while let NodeData::ParenthesizedExpression(d) = arena.data(id) {
        id = d.expression;
    }
    id
}

fn is_boolean_literal(arena: &NodeArena, id: NodeId) -> bool {
    matches!(arena.kind(id), Kind::TrueKeyword | Kind::FalseKeyword)
}

fn is_string_literal_like(arena: &NodeArena, id: NodeId) -> bool {
    matches!(
        arena.kind(id),
        Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral
    )
}

// Go: internal/binder/binder.go:isLogicalAssignmentExpression
fn is_logical_assignment_expression(arena: &NodeArena, node: NodeId) -> bool {
    let n = skip_parentheses(arena, node);
    matches!(arena.data(n), NodeData::BinaryExpression(d)
        if is_logical_or_coalescing_assignment_operator(arena.kind(d.operator_token)))
}

// Go: internal/ast/utilities.go:IsLogicalExpression
fn is_logical_expression(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::BinaryExpression(d)
        if matches!(arena.kind(d.operator_token), Kind::AmpersandAmpersandToken | Kind::BarBarToken))
}

// Go: internal/ast/utilities.go:IsLogicalOrCoalescingBinaryOperator
fn is_logical_or_coalescing_binary_operator(op: Kind) -> bool {
    matches!(
        op,
        Kind::AmpersandAmpersandToken | Kind::BarBarToken | Kind::QuestionQuestionToken
    )
}

// Go: internal/ast/utilities.go:IsLogicalOrCoalescingAssignmentOperator
fn is_logical_or_coalescing_assignment_operator(op: Kind) -> bool {
    matches!(
        op,
        Kind::AmpersandAmpersandEqualsToken
            | Kind::BarBarEqualsToken
            | Kind::QuestionQuestionEqualsToken
    )
}

// Go: internal/ast/utilities.go:IsOptionalChain
fn is_optional_chain(arena: &NodeArena, node: NodeId) -> bool {
    arena.flags(node).contains(NodeFlags::OPTIONAL_CHAIN)
        && matches!(
            arena.kind(node),
            Kind::PropertyAccessExpression
                | Kind::ElementAccessExpression
                | Kind::CallExpression
                | Kind::NonNullExpression
        )
}

// Go: internal/ast/utilities.go:IsOutermostOptionalChain
fn is_outermost_optional_chain(arena: &NodeArena, node: NodeId) -> bool {
    if !is_optional_chain(arena, node) {
        return false;
    }
    match arena.parent(node) {
        Some(p) => !is_optional_chain(arena, p) || expression_of(arena, p) != Some(node),
        None => true,
    }
}

// Go: internal/ast/utilities.go:IsNullishCoalesce
fn is_nullish_coalesce(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::BinaryExpression(d)
        if arena.kind(d.operator_token) == Kind::QuestionQuestionToken)
}

// Go: internal/ast/utilities.go:IsExpressionOfOptionalChainRoot (approximate)
fn is_expression_of_optional_chain_root(arena: &NodeArena, node: NodeId) -> bool {
    arena.parent(node).is_some_and(|p| {
        is_optional_chain(arena, p)
            && arena.flags(p).contains(NodeFlags::OPTIONAL_CHAIN)
            && expression_of(arena, p) == Some(node)
    })
}

// Go: internal/binder/binder.go:isTopLevelLogicalExpression
fn is_top_level_logical_expression(arena: &NodeArena, mut node: NodeId) -> bool {
    while let Some(parent) = arena.parent(node) {
        let is_paren = arena.kind(parent) == Kind::ParenthesizedExpression;
        let is_not = matches!(arena.data(parent), NodeData::PrefixUnaryExpression(d)
            if d.operator == Kind::ExclamationToken);
        if is_paren || is_not {
            node = parent;
        } else {
            break;
        }
    }
    let parent = match arena.parent(node) {
        Some(p) => p,
        None => return true,
    };
    !(is_statement_condition(arena, node)
        || is_logical_expression(arena, parent)
        || (is_optional_chain(arena, parent) && expression_of(arena, parent) == Some(node)))
}

// Go: internal/binder/binder.go:isStatementCondition
fn is_statement_condition(arena: &NodeArena, node: NodeId) -> bool {
    let parent = match arena.parent(node) {
        Some(p) => p,
        None => return false,
    };
    match arena.data(parent) {
        NodeData::IfStatement(d) => d.expression == node,
        NodeData::WhileStatement(d) => d.expression == node,
        NodeData::DoStatement(d) => d.expression == node,
        NodeData::ForStatement(d) => d.condition == Some(node),
        NodeData::ConditionalExpression(d) => d.condition == node,
        _ => false,
    }
}

// Go: internal/ast/utilities.go:IsDottedName
fn is_dotted_name(arena: &NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::Identifier | Kind::ThisKeyword | Kind::SuperKeyword | Kind::MetaProperty => true,
        Kind::PropertyAccessExpression | Kind::ParenthesizedExpression => {
            expression_of(arena, node).is_some_and(|e| is_dotted_name(arena, e))
        }
        _ => false,
    }
}

// Go: internal/ast/utilities.go:IsPushOrUnshiftIdentifier
fn is_push_or_unshift_name(name: &str) -> bool {
    name == "push" || name == "unshift"
}

// Go: internal/binder/binder.go:bindChildren (KindBinaryExpression destructuring guard)
pub(crate) fn is_destructuring_assignment(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::BinaryExpression(d)
    if arena.kind(d.operator_token) == Kind::EqualsToken
        && matches!(
            arena.kind(d.left),
            Kind::ArrayLiteralExpression | Kind::ObjectLiteralExpression
        ))
}

#[cfg(test)]
#[path = "flow_test.rs"]
mod tests;
