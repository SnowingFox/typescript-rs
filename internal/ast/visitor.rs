//! Child visiting / tree transformation (`visit_each_child`, `get_children`).

use crate::{ModifierList, NodeArena, NodeData, NodeId, NodeList};
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

/// A removal-aware visit callback: returning `None` drops the visited node from
/// its containing list (or empties an optional single-node slot), mirroring Go's
/// `NodeVisitor.Visit` returning `nil`.
type VisitRemovable<'a> = dyn FnMut(&mut NodeArena, NodeId) -> Option<NodeId> + 'a;

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
            NodeData::EmptyStatement
            | NodeData::DebuggerStatement
            | NodeData::NotEmittedStatement => id,
            NodeData::PartiallyEmittedExpression(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_partially_emitted_expression(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SyntaxList(d) => {
                let (list, changed) = self.visit_node_list(&d.list, opts, visit);
                if changed {
                    let new = self.new_syntax_list(list);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SyntheticReferenceExpression(d) => {
                let expression = visit(self, d.expression);
                let this_arg = visit(self, d.this_arg);
                if expression != d.expression || this_arg != d.this_arg {
                    let new = self.new_synthetic_reference_expression(expression, this_arg);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ThrowStatement(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_throw_statement(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::IfStatement(d) => {
                let expression = visit(self, d.expression);
                let then_statement = visit(self, d.then_statement);
                let else_statement = d.else_statement.map(|e| visit(self, e));
                if expression != d.expression
                    || then_statement != d.then_statement
                    || else_statement != d.else_statement
                {
                    let new = self.new_if_statement(expression, then_statement, else_statement);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::DoStatement(d) => {
                let statement = visit(self, d.statement);
                let expression = visit(self, d.expression);
                if statement != d.statement || expression != d.expression {
                    let new = self.new_do_statement(statement, expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::WhileStatement(d) => {
                let expression = visit(self, d.expression);
                let statement = visit(self, d.statement);
                if expression != d.expression || statement != d.statement {
                    let new = self.new_while_statement(expression, statement);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::WithStatement(d) => {
                let expression = visit(self, d.expression);
                let statement = visit(self, d.statement);
                if expression != d.expression || statement != d.statement {
                    let new = self.new_with_statement(expression, statement);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SwitchStatement(d) => {
                let expression = visit(self, d.expression);
                let case_block = visit(self, d.case_block);
                if expression != d.expression || case_block != d.case_block {
                    let new = self.new_switch_statement(expression, case_block);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::CaseBlock(d) => {
                let (clauses, changed) = self.visit_node_list(&d.clauses, opts, visit);
                if changed {
                    let new = self.new_case_block(clauses);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::CaseOrDefaultClause(d) => {
                let kind = self.kind(id);
                let expression = d.expression.map(|e| visit(self, e));
                let (statements, changed) = self.visit_node_list(&d.statements, opts, visit);
                if expression != d.expression || changed {
                    let new = self.new_case_or_default_clause(kind, expression, statements);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::BreakStatement(d) => {
                let label = d.label.map(|l| visit(self, l));
                if label != d.label {
                    let new = self.new_break_statement(label);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ContinueStatement(d) => {
                let label = d.label.map(|l| visit(self, l));
                if label != d.label {
                    let new = self.new_continue_statement(label);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::LabeledStatement(d) => {
                let label = visit(self, d.label);
                let statement = visit(self, d.statement);
                if label != d.label || statement != d.statement {
                    let new = self.new_labeled_statement(label, statement);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::OmittedExpression => id,
            NodeData::ComputedPropertyName(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_computed_property_name(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::VariableStatement(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let declaration_list = visit(self, d.declaration_list);
                if m_changed || declaration_list != d.declaration_list {
                    let new = self.new_variable_statement(modifiers, declaration_list);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::VariableDeclarationList(d) => {
                let (declarations, changed) = self.visit_node_list(&d.declarations, opts, visit);
                if changed {
                    let new = self.new_variable_declaration_list(declarations);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::VariableDeclaration(d) => {
                let name = visit(self, d.name);
                let exclamation_token = d.exclamation_token.map(|e| visit(self, e));
                let type_node = d.type_node.map(|t| visit(self, t));
                let initializer = d.initializer.map(|i| visit(self, i));
                if name != d.name
                    || exclamation_token != d.exclamation_token
                    || type_node != d.type_node
                    || initializer != d.initializer
                {
                    let new = self.new_variable_declaration(
                        name,
                        exclamation_token,
                        type_node,
                        initializer,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ObjectBindingPattern(d) | NodeData::ArrayBindingPattern(d) => {
                let kind = self.kind(id);
                let (elements, changed) = self.visit_node_list(&d.elements, opts, visit);
                if changed {
                    let new = self.new_binding_pattern(kind, elements);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::BindingElement(d) => {
                let dot_dot_dot_token = d.dot_dot_dot_token.map(|t| visit(self, t));
                let property_name = d.property_name.map(|p| visit(self, p));
                let name = d.name.map(|n| visit(self, n));
                let initializer = d.initializer.map(|i| visit(self, i));
                if dot_dot_dot_token != d.dot_dot_dot_token
                    || property_name != d.property_name
                    || name != d.name
                    || initializer != d.initializer
                {
                    let new = self.new_binding_element(
                        dot_dot_dot_token,
                        property_name,
                        name,
                        initializer,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ForStatement(d) => {
                let initializer = d.initializer.map(|i| visit(self, i));
                let condition = d.condition.map(|c| visit(self, c));
                let incrementor = d.incrementor.map(|i| visit(self, i));
                let statement = visit(self, d.statement);
                if initializer != d.initializer
                    || condition != d.condition
                    || incrementor != d.incrementor
                    || statement != d.statement
                {
                    let new =
                        self.new_for_statement(initializer, condition, incrementor, statement);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ForInOrOfStatement(d) => {
                let kind = self.kind(id);
                let await_modifier = d.await_modifier.map(|a| visit(self, a));
                let initializer = visit(self, d.initializer);
                let expression = visit(self, d.expression);
                let statement = visit(self, d.statement);
                if await_modifier != d.await_modifier
                    || initializer != d.initializer
                    || expression != d.expression
                    || statement != d.statement
                {
                    let new = self.new_for_in_or_of_statement(
                        kind,
                        await_modifier,
                        initializer,
                        expression,
                        statement,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TryStatement(d) => {
                let try_block = visit(self, d.try_block);
                let catch_clause = d.catch_clause.map(|c| visit(self, c));
                let finally_block = d.finally_block.map(|f| visit(self, f));
                if try_block != d.try_block
                    || catch_clause != d.catch_clause
                    || finally_block != d.finally_block
                {
                    let new = self.new_try_statement(try_block, catch_clause, finally_block);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::CatchClause(d) => {
                let variable_declaration = d.variable_declaration.map(|v| visit(self, v));
                let block = visit(self, d.block);
                if variable_declaration != d.variable_declaration || block != d.block {
                    let new = self.new_catch_clause(variable_declaration, block);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::FunctionDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let asterisk_token = d.asterisk_token.map(|a| visit(self, a));
                let name = d.name.map(|n| visit(self, n));
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                let full_signature = d.full_signature.map(|s| visit(self, s));
                let body = d.body.map(|b| visit(self, b));
                if m_changed
                    || asterisk_token != d.asterisk_token
                    || name != d.name
                    || tp_changed
                    || p_changed
                    || type_node != d.type_node
                    || full_signature != d.full_signature
                    || body != d.body
                {
                    let new = self.new_function_declaration(
                        modifiers,
                        asterisk_token,
                        name,
                        type_parameters,
                        parameters,
                        type_node,
                        full_signature,
                        body,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ParameterDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let dot_dot_dot_token = d.dot_dot_dot_token.map(|t| visit(self, t));
                let name = visit(self, d.name);
                let question_token = d.question_token.map(|q| visit(self, q));
                let type_node = d.type_node.map(|t| visit(self, t));
                let initializer = d.initializer.map(|i| visit(self, i));
                if m_changed
                    || dot_dot_dot_token != d.dot_dot_dot_token
                    || name != d.name
                    || question_token != d.question_token
                    || type_node != d.type_node
                    || initializer != d.initializer
                {
                    let new = self.new_parameter_declaration(
                        modifiers,
                        dot_dot_dot_token,
                        name,
                        question_token,
                        type_node,
                        initializer,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeParameterDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let constraint = d.constraint.map(|c| visit(self, c));
                let expression = d.expression.map(|e| visit(self, e));
                let default_type = d.default_type.map(|t| visit(self, t));
                if m_changed
                    || name != d.name
                    || constraint != d.constraint
                    || expression != d.expression
                    || default_type != d.default_type
                {
                    let new = self.new_type_parameter_declaration(
                        modifiers,
                        name,
                        constraint,
                        expression,
                        default_type,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => {
                let kind = self.kind(id);
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = d.name.map(|n| visit(self, n));
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (heritage_clauses, hc_changed) =
                    self.visit_opt_list(&d.heritage_clauses, opts, visit);
                let (members, mem_changed) = self.visit_node_list(&d.members, opts, visit);
                if m_changed || name != d.name || tp_changed || hc_changed || mem_changed {
                    let new = self.new_class_like(
                        kind,
                        modifiers,
                        name,
                        type_parameters,
                        heritage_clauses,
                        members,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::HeritageClause(d) => {
                let token = d.token;
                let (types, changed) = self.visit_node_list(&d.types, opts, visit);
                if changed {
                    let new = self.new_heritage_clause(token, types);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ExpressionWithTypeArguments(d) => {
                let expression = visit(self, d.expression);
                let (type_arguments, ta_changed) =
                    self.visit_opt_list(&d.type_arguments, opts, visit);
                if expression != d.expression || ta_changed {
                    let new = self.new_expression_with_type_arguments(expression, type_arguments);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::MethodDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let asterisk_token = d.asterisk_token.map(|a| visit(self, a));
                let name = visit(self, d.name);
                let postfix_token = d.postfix_token.map(|p| visit(self, p));
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                let full_signature = d.full_signature.map(|s| visit(self, s));
                let body = d.body.map(|b| visit(self, b));
                if m_changed
                    || asterisk_token != d.asterisk_token
                    || name != d.name
                    || postfix_token != d.postfix_token
                    || tp_changed
                    || p_changed
                    || type_node != d.type_node
                    || full_signature != d.full_signature
                    || body != d.body
                {
                    let new = self.new_method_declaration(
                        modifiers,
                        asterisk_token,
                        name,
                        postfix_token,
                        type_parameters,
                        parameters,
                        type_node,
                        full_signature,
                        body,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::PropertyDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let postfix_token = d.postfix_token.map(|p| visit(self, p));
                let type_node = d.type_node.map(|t| visit(self, t));
                let initializer = d.initializer.map(|i| visit(self, i));
                if m_changed
                    || name != d.name
                    || postfix_token != d.postfix_token
                    || type_node != d.type_node
                    || initializer != d.initializer
                {
                    let new = self.new_property_declaration(
                        modifiers,
                        name,
                        postfix_token,
                        type_node,
                        initializer,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                let kind = self.kind(id);
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                let full_signature = d.full_signature.map(|s| visit(self, s));
                let body = d.body.map(|b| visit(self, b));
                if m_changed
                    || name != d.name
                    || tp_changed
                    || p_changed
                    || type_node != d.type_node
                    || full_signature != d.full_signature
                    || body != d.body
                {
                    let new = self.new_accessor_declaration(
                        kind,
                        modifiers,
                        name,
                        type_parameters,
                        parameters,
                        type_node,
                        full_signature,
                        body,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ConstructorDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                let full_signature = d.full_signature.map(|s| visit(self, s));
                let body = d.body.map(|b| visit(self, b));
                if m_changed
                    || tp_changed
                    || p_changed
                    || type_node != d.type_node
                    || full_signature != d.full_signature
                    || body != d.body
                {
                    let new = self.new_constructor_declaration(
                        modifiers,
                        type_parameters,
                        parameters,
                        type_node,
                        full_signature,
                        body,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::IndexSignatureDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                if m_changed || p_changed || type_node != d.type_node {
                    let new =
                        self.new_index_signature_declaration(modifiers, parameters, type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ClassStaticBlockDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let body = visit(self, d.body);
                if m_changed || body != d.body {
                    let new = self.new_class_static_block_declaration(modifiers, body);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SemicolonClassElement => id,
            NodeData::InterfaceDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = d.name.map(|n| visit(self, n));
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (heritage_clauses, hc_changed) =
                    self.visit_opt_list(&d.heritage_clauses, opts, visit);
                let (members, mem_changed) = self.visit_node_list(&d.members, opts, visit);
                if m_changed || name != d.name || tp_changed || hc_changed || mem_changed {
                    let new = self.new_interface_declaration(
                        modifiers,
                        name,
                        type_parameters,
                        heritage_clauses,
                        members,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeAliasDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let type_node = visit(self, d.type_node);
                if m_changed || name != d.name || tp_changed || type_node != d.type_node {
                    let new = self.new_type_alias_declaration(
                        modifiers,
                        name,
                        type_parameters,
                        type_node,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::EnumDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let (members, mem_changed) = self.visit_node_list(&d.members, opts, visit);
                if m_changed || name != d.name || mem_changed {
                    let new = self.new_enum_declaration(modifiers, name, members);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::EnumMember(d) => {
                let name = visit(self, d.name);
                let initializer = d.initializer.map(|i| visit(self, i));
                if name != d.name || initializer != d.initializer {
                    let new = self.new_enum_member(name, initializer);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::PropertySignature(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let postfix_token = d.postfix_token.map(|p| visit(self, p));
                let type_node = d.type_node.map(|t| visit(self, t));
                let initializer = d.initializer.map(|i| visit(self, i));
                if m_changed
                    || name != d.name
                    || postfix_token != d.postfix_token
                    || type_node != d.type_node
                    || initializer != d.initializer
                {
                    let new = self.new_property_signature(
                        modifiers,
                        name,
                        postfix_token,
                        type_node,
                        initializer,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::MethodSignature(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let postfix_token = d.postfix_token.map(|p| visit(self, p));
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                if m_changed
                    || name != d.name
                    || postfix_token != d.postfix_token
                    || tp_changed
                    || p_changed
                    || type_node != d.type_node
                {
                    let new = self.new_method_signature(
                        modifiers,
                        name,
                        postfix_token,
                        type_parameters,
                        parameters,
                        type_node,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::CallSignature(d) | NodeData::ConstructSignature(d) => {
                let kind = self.kind(id);
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                if tp_changed || p_changed || type_node != d.type_node {
                    let new = self.new_signature_declaration(
                        kind,
                        type_parameters,
                        parameters,
                        type_node,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeLiteral(d) => {
                let (members, changed) = self.visit_node_list(&d.members, opts, visit);
                if changed {
                    let new = self.new_type_literal_node(members);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ModuleDeclaration(d) => {
                let keyword = d.keyword;
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let body = d.body.map(|b| visit(self, b));
                if m_changed || name != d.name || body != d.body {
                    let new = self.new_module_declaration(modifiers, keyword, name, body);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ModuleBlock(d) => {
                let (statements, changed) = self.visit_node_list(&d.statements, opts, visit);
                if changed {
                    let new = self.new_module_block(statements);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ImportDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let import_clause = d.import_clause.map(|c| visit(self, c));
                let module_specifier = visit(self, d.module_specifier);
                let attributes = d.attributes.map(|a| visit(self, a));
                if m_changed
                    || import_clause != d.import_clause
                    || module_specifier != d.module_specifier
                    || attributes != d.attributes
                {
                    let new = self.new_import_declaration(
                        modifiers,
                        import_clause,
                        module_specifier,
                        attributes,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ImportClause(d) => {
                let phase_modifier = d.phase_modifier;
                let name = d.name.map(|n| visit(self, n));
                let named_bindings = d.named_bindings.map(|n| visit(self, n));
                if name != d.name || named_bindings != d.named_bindings {
                    let new = self.new_import_clause(phase_modifier, name, named_bindings);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NamespaceImport(d) => {
                let name = visit(self, d.name);
                if name != d.name {
                    let new = self.new_namespace_import(name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NamespaceExport(d) => {
                let name = visit(self, d.name);
                if name != d.name {
                    let new = self.new_namespace_export(name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NamedImports(d) => {
                let (elements, changed) = self.visit_node_list(&d.elements, opts, visit);
                if changed {
                    let new = self.new_named_imports(elements);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NamedExports(d) => {
                let (elements, changed) = self.visit_node_list(&d.elements, opts, visit);
                if changed {
                    let new = self.new_named_exports(elements);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ImportSpecifier(d) => {
                let is_type_only = d.is_type_only;
                let property_name = d.property_name.map(|p| visit(self, p));
                let name = visit(self, d.name);
                if property_name != d.property_name || name != d.name {
                    let new = self.new_import_specifier(is_type_only, property_name, name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ExportSpecifier(d) => {
                let is_type_only = d.is_type_only;
                let property_name = d.property_name.map(|p| visit(self, p));
                let name = visit(self, d.name);
                if property_name != d.property_name || name != d.name {
                    let new = self.new_export_specifier(is_type_only, property_name, name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ExternalModuleReference(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_external_module_reference(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ImportEqualsDeclaration(d) => {
                let is_type_only = d.is_type_only;
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let module_reference = visit(self, d.module_reference);
                if m_changed || name != d.name || module_reference != d.module_reference {
                    let new = self.new_import_equals_declaration(
                        modifiers,
                        is_type_only,
                        name,
                        module_reference,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ExportDeclaration(d) => {
                let is_type_only = d.is_type_only;
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let export_clause = d.export_clause.map(|c| visit(self, c));
                let module_specifier = d.module_specifier.map(|m| visit(self, m));
                let attributes = d.attributes.map(|a| visit(self, a));
                if m_changed
                    || export_clause != d.export_clause
                    || module_specifier != d.module_specifier
                    || attributes != d.attributes
                {
                    let new = self.new_export_declaration(
                        modifiers,
                        is_type_only,
                        export_clause,
                        module_specifier,
                        attributes,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ExportAssignment(d) => {
                let is_export_equals = d.is_export_equals;
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                let expression = visit(self, d.expression);
                if m_changed || type_node != d.type_node || expression != d.expression {
                    let new = self.new_export_assignment(
                        modifiers,
                        is_export_equals,
                        type_node,
                        expression,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NamespaceExportDeclaration(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                if m_changed || name != d.name {
                    let new = self.new_namespace_export_declaration(modifiers, name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ObjectLiteralExpression(d) => {
                let (list, changed) = self.visit_node_list(&d.list, opts, visit);
                if changed {
                    let new = self.new_object_literal_expression(list);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::PropertyAssignment(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let postfix_token = d.postfix_token.map(|p| visit(self, p));
                let type_node = d.type_node.map(|t| visit(self, t));
                let initializer = d.initializer.map(|i| visit(self, i));
                if m_changed
                    || name != d.name
                    || postfix_token != d.postfix_token
                    || type_node != d.type_node
                    || initializer != d.initializer
                {
                    let new = self.new_property_assignment(
                        modifiers,
                        name,
                        postfix_token,
                        type_node,
                        initializer,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ShorthandPropertyAssignment(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let name = visit(self, d.name);
                let postfix_token = d.postfix_token.map(|p| visit(self, p));
                let type_node = d.type_node.map(|t| visit(self, t));
                let equals_token = d.equals_token.map(|e| visit(self, e));
                let object_assignment_initializer =
                    d.object_assignment_initializer.map(|i| visit(self, i));
                if m_changed
                    || name != d.name
                    || postfix_token != d.postfix_token
                    || type_node != d.type_node
                    || equals_token != d.equals_token
                    || object_assignment_initializer != d.object_assignment_initializer
                {
                    let new = self.new_shorthand_property_assignment(
                        modifiers,
                        name,
                        postfix_token,
                        type_node,
                        equals_token,
                        object_assignment_initializer,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SpreadAssignment(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_spread_assignment(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::DeleteExpression(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_delete_expression(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeOfExpression(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_type_of_expression(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::VoidExpression(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_void_expression(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::AwaitExpression(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_await_expression(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::YieldExpression(d) => {
                let asterisk_token = d.asterisk_token.map(|a| visit(self, a));
                let expression = d.expression.map(|e| visit(self, e));
                if asterisk_token != d.asterisk_token || expression != d.expression {
                    let new = self.new_yield_expression(asterisk_token, expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ArrowFunction(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                let full_signature = d.full_signature.map(|s| visit(self, s));
                let equals_greater_than_token = visit(self, d.equals_greater_than_token);
                let body = visit(self, d.body);
                if m_changed
                    || tp_changed
                    || p_changed
                    || type_node != d.type_node
                    || full_signature != d.full_signature
                    || equals_greater_than_token != d.equals_greater_than_token
                    || body != d.body
                {
                    let new = self.new_arrow_function(
                        modifiers,
                        type_parameters,
                        parameters,
                        type_node,
                        full_signature,
                        equals_greater_than_token,
                        body,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::RegularExpressionLiteral(_)
            | NodeData::NoSubstitutionTemplateLiteral(_)
            | NodeData::TemplateHead(_)
            | NodeData::TemplateMiddle(_)
            | NodeData::TemplateTail(_) => id,
            NodeData::AsExpression(d) => {
                let expression = visit(self, d.expression);
                let type_node = visit(self, d.type_node);
                if expression != d.expression || type_node != d.type_node {
                    let new = self.new_as_expression(expression, type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SatisfiesExpression(d) => {
                let expression = visit(self, d.expression);
                let type_node = visit(self, d.type_node);
                if expression != d.expression || type_node != d.type_node {
                    let new = self.new_satisfies_expression(expression, type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeAssertionExpression(d) => {
                let type_node = visit(self, d.type_node);
                let expression = visit(self, d.expression);
                if type_node != d.type_node || expression != d.expression {
                    let new = self.new_type_assertion(type_node, expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NonNullExpression(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_non_null_expression(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::Decorator(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_decorator(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ImportAttributes(d) => {
                let token = d.token;
                let multiline = d.multiline;
                let (attributes, changed) = self.visit_node_list(&d.attributes, opts, visit);
                if changed {
                    let new = self.new_import_attributes(token, attributes, multiline);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ImportAttribute(d) => {
                let name = d.name.map(|n| visit(self, n));
                let value = visit(self, d.value);
                if name != d.name || value != d.value {
                    let new = self.new_import_attribute(name, value);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxOpeningFragment | NodeData::JsxClosingFragment | NodeData::JsxText(_) => {
                id
            }
            NodeData::JsxElement(d) => {
                let opening = visit(self, d.opening);
                let (children, ch) = self.visit_node_list(&d.children, opts, visit);
                let closing = visit(self, d.closing);
                if opening != d.opening || ch || closing != d.closing {
                    let new = self.new_jsx_element(opening, children, closing);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxFragment(d) => {
                let opening = visit(self, d.opening);
                let (children, ch) = self.visit_node_list(&d.children, opts, visit);
                let closing = visit(self, d.closing);
                if opening != d.opening || ch || closing != d.closing {
                    let new = self.new_jsx_fragment(opening, children, closing);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxOpeningElement(d) => {
                let tag_name = visit(self, d.tag_name);
                let (type_arguments, ta) = self.visit_opt_list(&d.type_arguments, opts, visit);
                let attributes = visit(self, d.attributes);
                if tag_name != d.tag_name || ta || attributes != d.attributes {
                    let new = self.new_jsx_opening_element(tag_name, type_arguments, attributes);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxSelfClosingElement(d) => {
                let tag_name = visit(self, d.tag_name);
                let (type_arguments, ta) = self.visit_opt_list(&d.type_arguments, opts, visit);
                let attributes = visit(self, d.attributes);
                if tag_name != d.tag_name || ta || attributes != d.attributes {
                    let new =
                        self.new_jsx_self_closing_element(tag_name, type_arguments, attributes);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxClosingElement(d) => {
                let tag_name = visit(self, d.tag_name);
                if tag_name != d.tag_name {
                    let new = self.new_jsx_closing_element(tag_name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxAttributes(d) => {
                let (properties, ch) = self.visit_node_list(&d.list, opts, visit);
                if ch {
                    let new = self.new_jsx_attributes(properties);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxAttribute(d) => {
                let name = visit(self, d.name);
                let initializer = d.initializer.map(|i| visit(self, i));
                if name != d.name || initializer != d.initializer {
                    let new = self.new_jsx_attribute(name, initializer);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxSpreadAttribute(d) => {
                let expression = visit(self, d.expression);
                if expression != d.expression {
                    let new = self.new_jsx_spread_attribute(expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxNamespacedName(d) => {
                let namespace = visit(self, d.namespace);
                let name = visit(self, d.name);
                if namespace != d.namespace || name != d.name {
                    let new = self.new_jsx_namespaced_name(namespace, name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::JsxExpression(d) => {
                let dot_dot_dot_token = d.dot_dot_dot_token.map(|t| visit(self, t));
                let expression = d.expression.map(|e| visit(self, e));
                if dot_dot_dot_token != d.dot_dot_dot_token || expression != d.expression {
                    let new = self.new_jsx_expression(dot_dot_dot_token, expression);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::MetaProperty(d) => {
                let keyword_token = d.keyword_token;
                let name = visit(self, d.name);
                if name != d.name {
                    let new = self.new_meta_property(keyword_token, name);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TemplateExpression(d) => {
                let head = visit(self, d.head);
                let (template_spans, changed) =
                    self.visit_node_list(&d.template_spans, opts, visit);
                if head != d.head || changed {
                    let new = self.new_template_expression(head, template_spans);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TemplateSpan(d) => {
                let expression = visit(self, d.expression);
                let literal = visit(self, d.literal);
                if expression != d.expression || literal != d.literal {
                    let new = self.new_template_span(expression, literal);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TaggedTemplateExpression(d) => {
                let tag = visit(self, d.tag);
                let question_dot_token = d.question_dot_token.map(|q| visit(self, q));
                let (type_arguments, ta_changed) =
                    self.visit_opt_list(&d.type_arguments, opts, visit);
                let template = visit(self, d.template);
                if tag != d.tag
                    || question_dot_token != d.question_dot_token
                    || ta_changed
                    || template != d.template
                {
                    let new = self.new_tagged_template_expression(
                        tag,
                        question_dot_token,
                        type_arguments,
                        template,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::FunctionExpression(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let asterisk_token = d.asterisk_token.map(|a| visit(self, a));
                let name = d.name.map(|n| visit(self, n));
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                let full_signature = d.full_signature.map(|s| visit(self, s));
                let body = d.body.map(|b| visit(self, b));
                if m_changed
                    || asterisk_token != d.asterisk_token
                    || name != d.name
                    || tp_changed
                    || p_changed
                    || type_node != d.type_node
                    || full_signature != d.full_signature
                    || body != d.body
                {
                    let new = self.new_function_expression(
                        modifiers,
                        asterisk_token,
                        name,
                        type_parameters,
                        parameters,
                        type_node,
                        full_signature,
                        body,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ConditionalExpression(d) => {
                let condition = visit(self, d.condition);
                let question_token = visit(self, d.question_token);
                let when_true = visit(self, d.when_true);
                let colon_token = visit(self, d.colon_token);
                let when_false = visit(self, d.when_false);
                if condition != d.condition
                    || question_token != d.question_token
                    || when_true != d.when_true
                    || colon_token != d.colon_token
                    || when_false != d.when_false
                {
                    let new = self.new_conditional_expression(
                        condition,
                        question_token,
                        when_true,
                        colon_token,
                        when_false,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeReference(d) => {
                let type_name = visit(self, d.type_name);
                let (type_arguments, ta_changed) =
                    self.visit_opt_list(&d.type_arguments, opts, visit);
                if type_name != d.type_name || ta_changed {
                    let new = self.new_type_reference_node(type_name, type_arguments);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ArrayType(d) => {
                let element_type = visit(self, d.element_type);
                if element_type != d.element_type {
                    let new = self.new_array_type_node(element_type);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::IndexedAccessType(d) => {
                let object_type = visit(self, d.object_type);
                let index_type = visit(self, d.index_type);
                if object_type != d.object_type || index_type != d.index_type {
                    let new = self.new_indexed_access_type_node(object_type, index_type);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::UnionType(d) => {
                let (types, changed) = self.visit_node_list(&d.types, opts, visit);
                if changed {
                    let new = self.new_union_type_node(types);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::IntersectionType(d) => {
                let (types, changed) = self.visit_node_list(&d.types, opts, visit);
                if changed {
                    let new = self.new_intersection_type_node(types);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ParenthesizedType(d) => {
                let type_node = visit(self, d.type_node);
                if type_node != d.type_node {
                    let new = self.new_parenthesized_type_node(type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::LiteralType(d) => {
                let literal = visit(self, d.literal);
                if literal != d.literal {
                    let new = self.new_literal_type_node(literal);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ThisType => id,
            NodeData::FunctionType(d) => {
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                if tp_changed || p_changed || type_node != d.type_node {
                    let new = self.new_function_type_node(type_parameters, parameters, type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ConstructorType(d) => {
                let (modifiers, m_changed) = self.visit_modifiers(&d.modifiers, opts, visit);
                let (type_parameters, tp_changed) =
                    self.visit_opt_list(&d.type_parameters, opts, visit);
                let (parameters, p_changed) = self.visit_node_list(&d.parameters, opts, visit);
                let type_node = d.type_node.map(|t| visit(self, t));
                if m_changed || tp_changed || p_changed || type_node != d.type_node {
                    let new = self.new_constructor_type_node(
                        modifiers,
                        type_parameters,
                        parameters,
                        type_node,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ConditionalType(d) => {
                let check_type = visit(self, d.check_type);
                let extends_type = visit(self, d.extends_type);
                let true_type = visit(self, d.true_type);
                let false_type = visit(self, d.false_type);
                if check_type != d.check_type
                    || extends_type != d.extends_type
                    || true_type != d.true_type
                    || false_type != d.false_type
                {
                    let new = self.new_conditional_type_node(
                        check_type,
                        extends_type,
                        true_type,
                        false_type,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::InferType(d) => {
                let type_parameter = visit(self, d.type_parameter);
                if type_parameter != d.type_parameter {
                    let new = self.new_infer_type_node(type_parameter);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeOperator(d) => {
                let operator = d.operator;
                let type_node = visit(self, d.type_node);
                if type_node != d.type_node {
                    let new = self.new_type_operator_node(operator, type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::MappedType(d) => {
                let readonly_token = d.readonly_token.map(|t| visit(self, t));
                let type_parameter = visit(self, d.type_parameter);
                let name_type = d.name_type.map(|t| visit(self, t));
                let question_token = d.question_token.map(|t| visit(self, t));
                let type_node = d.type_node.map(|t| visit(self, t));
                let (members, mem_changed) = self.visit_node_list(&d.members, opts, visit);
                if readonly_token != d.readonly_token
                    || type_parameter != d.type_parameter
                    || name_type != d.name_type
                    || question_token != d.question_token
                    || type_node != d.type_node
                    || mem_changed
                {
                    let new = self.new_mapped_type_node(
                        readonly_token,
                        type_parameter,
                        name_type,
                        question_token,
                        type_node,
                        members,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TupleType(d) => {
                let (elements, changed) = self.visit_node_list(&d.types, opts, visit);
                if changed {
                    let new = self.new_tuple_type_node(elements);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::NamedTupleMember(d) => {
                let dot_dot_dot_token = d.dot_dot_dot_token.map(|t| visit(self, t));
                let name = visit(self, d.name);
                let question_token = d.question_token.map(|t| visit(self, t));
                let type_node = visit(self, d.type_node);
                if dot_dot_dot_token != d.dot_dot_dot_token
                    || name != d.name
                    || question_token != d.question_token
                    || type_node != d.type_node
                {
                    let new = self.new_named_tuple_member(
                        dot_dot_dot_token,
                        name,
                        question_token,
                        type_node,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::RestType(d) => {
                let type_node = visit(self, d.type_node);
                if type_node != d.type_node {
                    let new = self.new_rest_type_node(type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::OptionalType(d) => {
                let type_node = visit(self, d.type_node);
                if type_node != d.type_node {
                    let new = self.new_optional_type_node(type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypeQuery(d) => {
                let expr_name = visit(self, d.expr_name);
                let (type_arguments, ta_changed) =
                    self.visit_opt_list(&d.type_arguments, opts, visit);
                if expr_name != d.expr_name || ta_changed {
                    let new = self.new_type_query_node(expr_name, type_arguments);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::ImportType(d) => {
                let is_type_of = d.is_type_of;
                let argument = visit(self, d.argument);
                let attributes = d.attributes.map(|a| visit(self, a));
                let qualifier = d.qualifier.map(|q| visit(self, q));
                let (type_arguments, ta_changed) =
                    self.visit_opt_list(&d.type_arguments, opts, visit);
                if argument != d.argument
                    || attributes != d.attributes
                    || qualifier != d.qualifier
                    || ta_changed
                {
                    let new = self.new_import_type_node(
                        is_type_of,
                        argument,
                        attributes,
                        qualifier,
                        type_arguments,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TemplateLiteralType(d) => {
                let head = visit(self, d.head);
                let (template_spans, changed) =
                    self.visit_node_list(&d.template_spans, opts, visit);
                if head != d.head || changed {
                    let new = self.new_template_literal_type_node(head, template_spans);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TemplateLiteralTypeSpan(d) => {
                let type_node = visit(self, d.expression);
                let literal = visit(self, d.literal);
                if type_node != d.expression || literal != d.literal {
                    let new = self.new_template_literal_type_span(type_node, literal);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::TypePredicate(d) => {
                let asserts_modifier = d.asserts_modifier.map(|a| visit(self, a));
                let parameter_name = visit(self, d.parameter_name);
                let type_node = d.type_node.map(|t| visit(self, t));
                if asserts_modifier != d.asserts_modifier
                    || parameter_name != d.parameter_name
                    || type_node != d.type_node
                {
                    let new =
                        self.new_type_predicate_node(asserts_modifier, parameter_name, type_node);
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
            NodeData::SourceFile(d) => {
                let (statements, changed) = self.visit_node_list(&d.statements, opts, visit);
                let end_of_file_token = visit(self, d.end_of_file_token);
                if changed || end_of_file_token != d.end_of_file_token {
                    let new = self.new_source_file(
                        &d.file_name,
                        d.script_kind,
                        d.language_variant,
                        statements,
                        end_of_file_token,
                    );
                    self.copy_node_meta(new, id);
                    new
                } else {
                    id
                }
            }
        }
    }

    /// Visits each element of `list` with a removal-aware callback, dropping the
    /// elements whose visit returns `None`, and returns the resulting
    /// [`NodeList`] (preserving the original range).
    ///
    /// This is the removal-aware counterpart of the internal
    /// [`visit_node_list`](Self::visit_node_list) helper: it lets a transform
    /// elide nodes (type-only imports, `this` parameters, accessibility
    /// modifiers, `implements` clauses, …) the way Go's `NodeVisitor.VisitNodes`
    /// drops `nil` results. Single-node slots are handled by the caller via
    /// `slot.and_then(|n| visit(arena, n))`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::{NodeArena, NodeList};
    /// let mut arena = NodeArena::new();
    /// let a = arena.new_identifier("a");
    /// let b = arena.new_identifier("b");
    /// let list = NodeList::new(vec![a, b]);
    /// let kept = arena.visit_nodes_removable(&list, &mut |_a, c| (c == a).then_some(c));
    /// assert_eq!(kept.nodes, vec![a]);
    /// ```
    ///
    /// Side effects: invokes `visit`, which may push new nodes onto the arena.
    // Go: internal/ast/visitor.go:NodeVisitor.VisitNodes
    pub fn visit_nodes_removable(
        &mut self,
        list: &NodeList,
        visit: &mut VisitRemovable<'_>,
    ) -> NodeList {
        let mut nodes = Vec::with_capacity(list.nodes.len());
        for &child in &list.nodes {
            if let Some(replacement) = visit(self, child) {
                nodes.push(replacement);
            }
        }
        NodeList {
            loc: list.loc,
            nodes,
            missing: list.missing,
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
            missing: list.missing,
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

    /// Visits each node of an optional modifier list, preserving its flags.
    ///
    /// Side effects: may push new nodes.
    // Go: internal/ast/visitor.go:NodeVisitor.VisitModifiers
    fn visit_modifiers(
        &mut self,
        modifiers: &Option<ModifierList>,
        opts: VisitOptions,
        visit: &mut Visit<'_>,
    ) -> (Option<ModifierList>, bool) {
        match modifiers {
            None => (None, false),
            Some(m) => {
                let (list, changed) = self.visit_node_list(&m.list, opts, visit);
                (
                    Some(ModifierList {
                        list,
                        modifier_flags: m.modifier_flags,
                    }),
                    changed,
                )
            }
        }
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
