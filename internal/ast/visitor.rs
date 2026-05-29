//! Child visiting / tree transformation (`visit_each_child`, `get_children`).

use crate::{NodeArena, NodeData, NodeId, NodeList};
use tsgo_core::text::TextRange;

/// Controls how [`NodeArena::visit_each_child`] rebuilds list children.
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, Debug)]
pub struct VisitOptions {
    /// When set, freshly produced nodes/lists get the synthetic range `(-1, -1)`,
    /// and trailing-comma list tails get `(-2, -2)` (used by deep clone).
    pub synthetic_location: bool,
    /// When set, list children are always rebuilt (so the containing node is
    /// always treated as changed), matching deep clone's `VisitNodes` hook.
    pub clone_lists: bool,
}

/// The callback applied to each visited child; returns the replacement id.
type Visit<'a> = dyn FnMut(&mut NodeArena, NodeId) -> NodeId + 'a;

impl NodeArena {
    /// Collects the direct children of `id` via an identity [`visit_each_child`],
    /// mirroring Go's `getChildren` test helper.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// let mut arena = NodeArena::new();
    /// let a = arena.new_identifier("a");
    /// let b = arena.new_identifier("b");
    /// let qn = arena.new_qualified_name(a, b);
    /// assert_eq!(arena.get_children(qn), vec![a, b]);
    /// ```
    ///
    /// Side effects: none (the identity visit produces no new nodes).
    ///
    /// [`visit_each_child`]: NodeArena::visit_each_child
    // Go: internal/ast/deepclone_test.go:getChildren
    pub fn get_children(&mut self, id: NodeId) -> Vec<NodeId> {
        let mut children = Vec::new();
        let opts = VisitOptions {
            synthetic_location: false,
            clone_lists: false,
        };
        let mut visit = |_arena: &mut NodeArena, c: NodeId| {
            children.push(c);
            c
        };
        self.visit_each_child(id, opts, &mut visit);
        children
    }

    /// Visits each child of `id` with `visit`, returning a possibly-new node.
    ///
    /// If any child is replaced (or, under `opts.clone_lists`, a list child is
    /// present) a new node of the same kind is created with the visited children
    /// and the original node's flags/location; otherwise `id` is returned
    /// unchanged. Mirrors the per-kind `VisitEachChild` + `UpdateXxx` pair in Go.
    ///
    /// Side effects: may push new nodes; may set synthetic locations under
    /// `opts.synthetic_location`.
    // Go: internal/ast/visitor.go:NodeVisitor.VisitEachChild
    pub fn visit_each_child(
        &mut self,
        id: NodeId,
        opts: VisitOptions,
        visit: &mut Visit<'_>,
    ) -> NodeId {
        let data = self.data(id).clone();
        match data {
            NodeData::Token
            | NodeData::Identifier(_)
            | NodeData::PrivateIdentifier(_)
            | NodeData::StringLiteral(_)
            | NodeData::NumericLiteral(_)
            | NodeData::BigIntLiteral(_)
            | NodeData::KeywordExpression => id,
            NodeData::QualifiedName(d) => {
                let left = visit(self, d.left);
                let right = visit(self, d.right);
                if left != d.left || right != d.right {
                    let new = self.new_qualified_name(left, right);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::PropertyAccessExpression(d) => {
                let expression = visit(self, d.expression);
                let question = d.question_dot_token.map(|q| visit(self, q));
                let name = visit(self, d.name);
                if expression != d.expression || question != d.question_dot_token || name != d.name
                {
                    let new = self.new_property_access_expression(expression, question, name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ElementAccessExpression(d) => {
                let expression = visit(self, d.expression);
                let question = d.question_dot_token.map(|q| visit(self, q));
                let argument = visit(self, d.argument_expression);
                if expression != d.expression
                    || question != d.question_dot_token
                    || argument != d.argument_expression
                {
                    let new = self.new_element_access_expression(expression, question, argument);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::CallExpression(d) => {
                let expression = visit(self, d.expression);
                let question = d.question_dot_token.map(|q| visit(self, q));
                let (type_arguments, ta_changed) =
                    self.visit_opt_list(&d.type_arguments, opts, visit);
                let (arguments, args_changed) = self.visit_node_list(&d.arguments, opts, visit);
                let flags = self.flags(id);
                if expression != d.expression
                    || question != d.question_dot_token
                    || ta_changed
                    || args_changed
                {
                    let new = self.new_call_expression(
                        expression,
                        question,
                        type_arguments,
                        arguments,
                        flags,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NewExpression(d) => {
                let expression = visit(self, d.expression);
                let (type_arguments, ta_changed) =
                    self.visit_opt_list(&d.type_arguments, opts, visit);
                let (arguments, args_changed) = self.visit_opt_list(&d.arguments, opts, visit);
                if expression != d.expression || ta_changed || args_changed {
                    let new = self.new_new_expression(expression, type_arguments, arguments);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ParenthesizedExpression(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_parenthesized_expression(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SpreadElement(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_spread_element(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ExpressionStatement(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_expression_statement(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::PrefixUnaryExpression(d) => {
                let operand = visit(self, d.operand);
                if operand != d.operand {
                    let new = self.new_prefix_unary_expression(d.operator, operand);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::PostfixUnaryExpression(d) => {
                let operand = visit(self, d.operand);
                if operand != d.operand {
                    let new = self.new_postfix_unary_expression(operand, d.operator);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::BinaryExpression(d) => {
                let left = visit(self, d.left);
                let operator_token = visit(self, d.operator_token);
                let right = visit(self, d.right);
                if left != d.left || operator_token != d.operator_token || right != d.right {
                    let new = self.new_binary_expression(left, operator_token, right);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ArrayLiteralExpression(d) => {
                let (list, changed) = self.visit_node_list(&d.list, opts, visit);
                if changed {
                    let new = self.new_array_literal_expression(list);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::Block(d) => {
                let (list, changed) = self.visit_node_list(&d.list, opts, visit);
                if changed {
                    let new = self.new_block(list);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ReturnStatement(d) => {
                let expression = d.expression.map(|e| visit(self, e));
                if expression != d.expression {
                    let new = self.new_return_statement(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
        }
    }

    /// Visits each element of `list`, returning the new list and whether it was
    /// considered changed (any element replaced, or `opts.clone_lists`).
    ///
    /// Side effects: may push new nodes; sets synthetic list/tail locations
    /// under `opts.synthetic_location`.
    // Go: internal/ast/visitor.go:NodeVisitor.VisitNodes
    fn visit_node_list(
        &mut self,
        list: &NodeList,
        opts: VisitOptions,
        visit: &mut Visit<'_>,
    ) -> (NodeList, bool) {
        let had_trailing_comma = self.list_has_trailing_comma(list);
        let mut nodes = Vec::with_capacity(list.nodes.len());
        let mut changed = false;
        for &child in &list.nodes {
            let visited = visit(self, child);
            if visited != child {
                changed = true;
            }
            nodes.push(visited);
        }
        let mut result = NodeList {
            loc: list.loc,
            nodes,
        };
        if opts.synthetic_location {
            result.loc = TextRange::new(-1, -1);
            if had_trailing_comma {
                if let Some(&last) = result.nodes.last() {
                    self.set_loc(last, TextRange::new(-2, -2));
                }
            }
        }
        (result, changed || opts.clone_lists)
    }

    /// Like [`visit_node_list`](Self::visit_node_list) but for an optional list.
    ///
    /// Side effects: see [`visit_node_list`](Self::visit_node_list).
    fn visit_opt_list(
        &mut self,
        list: &Option<NodeList>,
        opts: VisitOptions,
        visit: &mut Visit<'_>,
    ) -> (Option<NodeList>, bool) {
        match list {
            None => (None, false),
            Some(l) => {
                let (nl, changed) = self.visit_node_list(l, opts, visit);
                (Some(nl), changed)
            }
        }
    }

    /// Copies the flags and location from `original` onto `new_id`, mirroring
    /// Go's `updateNode`.
    ///
    /// Side effects: mutates `new_id`.
    // Go: internal/ast/ast.go:updateNode
    fn copy_node_meta(&mut self, new_id: NodeId, original: NodeId) {
        let flags = self.flags(original);
        let loc = self.loc(original);
        self.set_loc(new_id, loc);
        self.set_flags(new_id, flags);
    }
}

#[cfg(test)]
#[path = "visitor_test.rs"]
mod tests;
