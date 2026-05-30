//! Expression emit: the `emit_expression_node` dispatcher and the per-kind
//! expression emitters. (`impl Printer` block; see `printer.rs` for the core.)

use crate::printer::{
    is_optional_chain, skip_partially_emitted_expressions, ListEmit, Printer, WriteKind,
};
use crate::utilities::GetLiteralTextFlags;
use tsgo_ast::precedence::{get_expression_precedence, OperatorPrecedence};
use tsgo_ast::{Kind, NodeData, NodeId};
use tsgo_core::text::TextRange;

impl Printer<'_> {
    /// Dispatches expression emit by node kind (the body of Go `emitExpression`'s
    /// switch; the outer parenthesization lives in `emit_expression`).
    // Go: internal/printer/printer.go:emitExpression
    pub(crate) fn emit_expression_node(&mut self, node: NodeId) {
        // Just-in-time substitution (Go `onSubstituteNode`): if a transform
        // registered a replacement for this node, emit the replacement instead.
        // Applied once — the replacement's own sub-parts are not re-substituted
        // because they are distinct nodes not present in the table.
        let node = self.get_node_substitution(node).unwrap_or(node);
        match self.arena().kind(node) {
            Kind::TrueKeyword | Kind::FalseKeyword | Kind::NullKeyword => {
                self.emit_token_node(node)
            }
            Kind::ThisKeyword | Kind::SuperKeyword | Kind::ImportKeyword => {
                self.emit_keyword_node(Some(node))
            }
            Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::StringLiteral
            | Kind::RegularExpressionLiteral
            | Kind::NoSubstitutionTemplateLiteral => self.emit_literal_expression(node),
            Kind::Identifier | Kind::PrivateIdentifier => self.emit_identifier_like(node),
            Kind::ArrayLiteralExpression => self.emit_array_literal_expression(node),
            Kind::ObjectLiteralExpression => self.emit_object_literal_expression(node),
            Kind::PropertyAccessExpression => self.emit_property_access_expression(node),
            Kind::ElementAccessExpression => self.emit_element_access_expression(node),
            Kind::CallExpression => self.emit_call_expression(node),
            Kind::NewExpression => self.emit_new_expression(node),
            Kind::TaggedTemplateExpression => self.emit_tagged_template_expression(node),
            Kind::TypeAssertionExpression => self.emit_type_assertion_expression(node),
            Kind::ParenthesizedExpression => self.emit_parenthesized_expression(node),
            Kind::FunctionExpression => self.emit_function_expression(node),
            Kind::ArrowFunction => self.emit_arrow_function(node),
            Kind::DeleteExpression => self.emit_unary_keyword(node, Kind::DeleteKeyword),
            Kind::TypeOfExpression => self.emit_unary_keyword(node, Kind::TypeOfKeyword),
            Kind::VoidExpression => self.emit_unary_keyword(node, Kind::VoidKeyword),
            Kind::AwaitExpression => self.emit_unary_keyword(node, Kind::AwaitKeyword),
            Kind::PrefixUnaryExpression => self.emit_prefix_unary_expression(node),
            Kind::PostfixUnaryExpression => self.emit_postfix_unary_expression(node),
            Kind::BinaryExpression => self.emit_binary_expression(node),
            Kind::ConditionalExpression => self.emit_conditional_expression(node),
            Kind::TemplateExpression => self.emit_template_expression(node),
            Kind::YieldExpression => self.emit_yield_expression(node),
            Kind::SpreadElement => self.emit_spread_element(node),
            Kind::ClassExpression => self.emit_class_like(node),
            Kind::OmittedExpression => {}
            Kind::AsExpression => self.emit_as_expression(node, "as"),
            Kind::SatisfiesExpression => self.emit_as_expression(node, "satisfies"),
            Kind::NonNullExpression => self.emit_non_null_expression(node),
            Kind::ExpressionWithTypeArguments => self.emit_expression_with_type_arguments(node),
            Kind::PartiallyEmittedExpression => self.emit_partially_emitted_expression(node),
            Kind::MetaProperty => self.emit_meta_property(node),
            Kind::JsxElement => self.emit_jsx_element(node),
            Kind::JsxSelfClosingElement => self.emit_jsx_self_closing_element(node),
            Kind::JsxFragment => self.emit_jsx_fragment(node),
            other => panic!("emit_expression_node: unhandled kind {other:?}"),
        }
    }

    fn emit_literal_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        self.emit_literal(node, GetLiteralTextFlags::NONE);
        self.exit_node(node);
    }

    fn emit_identifier_like(&mut self, node: NodeId) {
        self.enter_node(node);
        let text = self.get_text_of_node(node, false);
        self.write(&text);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitArrayLiteralExpression
    fn emit_array_literal_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        // TODO(port): the Rust AST does not carry Go's per-node `MultiLine` flag
        // for list-bearing literals; freshly-parsed single-line literals are false.
        let (elements, multi_line) = match self.arena().data(node) {
            NodeData::ArrayLiteralExpression(d) => (d.list.clone(), false),
            other => panic!("expected ArrayLiteralExpression, got {other:?}"),
        };
        let mut format = crate::list_format::ListFormat::ARRAY_LITERAL_EXPRESSION_ELEMENTS;
        if multi_line {
            format |= crate::list_format::ListFormat::PREFER_NEW_LINE;
        }
        self.emit_list(
            ListEmit::ArrayLiteralElement,
            Some(node),
            Some(&elements),
            format,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitObjectLiteralExpression
    fn emit_object_literal_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (properties, multi_line) = match self.arena().data(node) {
            NodeData::ObjectLiteralExpression(d) => (d.list.clone(), false),
            other => panic!("expected ObjectLiteralExpression, got {other:?}"),
        };
        self.push_name_generation_scope(node);
        let mut format = crate::list_format::ListFormat::OBJECT_LITERAL_EXPRESSION_PROPERTIES;
        if multi_line {
            format |= crate::list_format::ListFormat::PREFER_NEW_LINE;
        }
        if self.should_allow_trailing_comma(node) {
            format |= crate::list_format::ListFormat::ALLOW_TRAILING_COMMA;
        }
        self.emit_list(
            ListEmit::ObjectLiteralElement,
            Some(node),
            Some(&properties),
            format,
        );
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    fn should_allow_trailing_comma(&self, node: NodeId) -> bool {
        // Object literals always permit a trailing comma (outside JSON).
        matches!(self.arena().kind(node), Kind::ObjectLiteralExpression)
    }

    // Go: internal/printer/printer.go:emitObjectLiteralElement
    pub(crate) fn emit_object_literal_element(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::PropertyAssignment => self.emit_property_assignment(node),
            Kind::ShorthandPropertyAssignment => self.emit_shorthand_property_assignment(node),
            Kind::SpreadAssignment => self.emit_spread_assignment(node),
            Kind::MethodDeclaration => self.emit_method_declaration(node),
            Kind::GetAccessor => self.emit_accessor_declaration(node, Kind::GetKeyword),
            Kind::SetAccessor => self.emit_accessor_declaration(node, Kind::SetKeyword),
            other => panic!("unhandled ObjectLiteralElement: {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitPropertyAssignment
    fn emit_property_assignment(&mut self, node: NodeId) {
        self.enter_node(node);
        let (name, initializer) = match self.arena().data(node) {
            NodeData::PropertyAssignment(d) => (d.name, d.initializer),
            other => panic!("expected PropertyAssignment, got {other:?}"),
        };
        self.emit_property_name(Some(name));
        self.write_punctuation(":");
        self.write_space();
        self.emit_expression(
            initializer.expect("property assignment initializer"),
            OperatorPrecedence::DISALLOW_COMMA,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitShorthandPropertyAssignment
    fn emit_shorthand_property_assignment(&mut self, node: NodeId) {
        self.enter_node(node);
        let (name, initializer) = match self.arena().data(node) {
            NodeData::ShorthandPropertyAssignment(d) => (d.name, d.object_assignment_initializer),
            other => panic!("expected ShorthandPropertyAssignment, got {other:?}"),
        };
        self.emit_property_name(Some(name));
        if let Some(initializer) = initializer {
            self.write_space();
            self.write_punctuation("=");
            self.write_space();
            self.emit_expression(initializer, OperatorPrecedence::DISALLOW_COMMA);
        }
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitSpreadAssignment
    fn emit_spread_assignment(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::SpreadAssignment(d) => d.expression,
            other => panic!("expected SpreadAssignment, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::DotDotDotToken, pos, WriteKind::Punctuation, node);
        self.emit_expression(expression, OperatorPrecedence::DISALLOW_COMMA);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitPropertyAccessExpression
    fn emit_property_access_expression(&mut self, node: NodeId) {
        let arena = self.arena();
        self.enter_node(node);
        let (expression, question_dot, name) = match arena.data(node) {
            NodeData::PropertyAccessExpression(d) => (d.expression, d.question_dot_token, d.name),
            other => panic!("expected PropertyAccessExpression, got {other:?}"),
        };
        let opt = is_optional_chain(arena, node);
        self.emit_expression(
            expression,
            if opt {
                OperatorPrecedence::OptionalChain
            } else {
                OperatorPrecedence::Member
            },
        );
        let expr_loc = arena.loc(expression);
        let name_loc = arena.loc(name);
        let token_loc = match question_dot {
            Some(q) => arena.loc(q),
            None => TextRange::new(expr_loc.end(), name_loc.pos()),
        };
        let lines_before_dot = self.get_lines_between_ranges(node, expr_loc, token_loc);
        self.write_line_repeat(lines_before_dot);
        self.increase_indent_if(lines_before_dot > 0);

        let should_emit_dot_dot = question_dot.is_none()
            && self.may_need_dot_dot_for_property_access(expression)
            && !self.has_trailing_comment()
            && !self.has_trailing_whitespace();
        if should_emit_dot_dot {
            self.write_punctuation(".");
        }
        match question_dot {
            Some(q) => self.emit_token_node(q),
            None => {
                self.emit_token(Kind::DotToken, expr_loc.end(), WriteKind::Punctuation, node);
            }
        }
        let lines_after_dot = self.get_lines_between_ranges(node, token_loc, name_loc);
        self.write_line_repeat(lines_after_dot);
        self.increase_indent_if(lines_after_dot > 0);
        self.emit_member_name(Some(name));
        self.decrease_indent_if(lines_after_dot > 0);
        self.decrease_indent_if(lines_before_dot > 0);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitElementAccessExpression
    fn emit_element_access_expression(&mut self, node: NodeId) {
        let arena = self.arena();
        self.enter_node(node);
        let (expression, question_dot, argument) = match arena.data(node) {
            NodeData::ElementAccessExpression(d) => {
                (d.expression, d.question_dot_token, d.argument_expression)
            }
            other => panic!("expected ElementAccessExpression, got {other:?}"),
        };
        let opt = is_optional_chain(arena, node);
        self.emit_expression(
            expression,
            if opt {
                OperatorPrecedence::OptionalChain
            } else {
                OperatorPrecedence::Member
            },
        );
        if let Some(q) = question_dot {
            self.emit_token_node(q);
        }
        let bracket_pos = self.arena().loc(question_dot.unwrap_or(expression)).end();
        self.emit_token(
            Kind::OpenBracketToken,
            bracket_pos,
            WriteKind::Punctuation,
            node,
        );
        self.emit_expression(argument, OperatorPrecedence::Comma);
        let close_pos = self.arena().loc(argument).end();
        self.emit_token(
            Kind::CloseBracketToken,
            close_pos,
            WriteKind::Punctuation,
            node,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitCallExpression
    fn emit_call_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, question_dot, type_arguments, arguments) = match self.arena().data(node) {
            NodeData::CallExpression(d) => (
                d.expression,
                d.question_dot_token,
                d.type_arguments.clone(),
                d.arguments.clone(),
            ),
            other => panic!("expected CallExpression, got {other:?}"),
        };
        self.emit_callee(expression, node);
        if let Some(q) = question_dot {
            self.emit_token_node(q);
        }
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.emit_list(
            ListEmit::Argument,
            Some(node),
            Some(&arguments),
            crate::list_format::ListFormat::CALL_EXPRESSION_ARGUMENTS,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitCallee
    fn emit_callee(&mut self, callee: NodeId, parent: NodeId) {
        let arena = self.arena();
        if parent_is_call(arena, parent)
            && is_new_expression_without_arguments(
                arena,
                skip_partially_emitted_expressions(arena, callee),
            )
        {
            self.emit_expression(callee, OperatorPrecedence::Parentheses);
        } else {
            let opt = is_optional_chain(arena, parent);
            self.emit_expression(
                callee,
                if opt {
                    OperatorPrecedence::OptionalChain
                } else {
                    OperatorPrecedence::Member
                },
            );
        }
    }

    // Go: internal/printer/printer.go:emitNewExpression
    fn emit_new_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, type_arguments, arguments) = match self.arena().data(node) {
            NodeData::NewExpression(d) => {
                (d.expression, d.type_arguments.clone(), d.arguments.clone())
            }
            other => panic!("expected NewExpression, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::NewKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        let inner = skip_partially_emitted_expressions(self.arena(), expression);
        if self.arena().kind(inner) == Kind::CallExpression {
            self.emit_expression(expression, OperatorPrecedence::Parentheses);
        } else {
            self.emit_expression(expression, OperatorPrecedence::Member);
        }
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.emit_list(
            ListEmit::Argument,
            Some(node),
            arguments.as_ref(),
            crate::list_format::ListFormat::NEW_EXPRESSION_ARGUMENTS,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitTaggedTemplateExpression
    fn emit_tagged_template_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (tag, type_arguments, template) = match self.arena().data(node) {
            NodeData::TaggedTemplateExpression(d) => (d.tag, d.type_arguments.clone(), d.template),
            other => panic!("expected TaggedTemplateExpression, got {other:?}"),
        };
        self.emit_callee(tag, node);
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.write_space();
        self.emit_template_literal(template);
        self.exit_node(node);
    }

    fn emit_template_literal(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::NoSubstitutionTemplateLiteral => self.emit_literal_expression(node),
            Kind::TemplateExpression => self.emit_template_expression(node),
            other => panic!("unhandled TemplateLiteral: {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitTemplateExpression
    fn emit_template_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (head, spans) = match self.arena().data(node) {
            NodeData::TemplateExpression(d) => (d.head, d.template_spans.clone()),
            other => panic!("expected TemplateExpression, got {other:?}"),
        };
        self.emit_template_head(head);
        self.emit_list(
            ListEmit::TemplateSpan,
            Some(node),
            Some(&spans),
            crate::list_format::ListFormat::TEMPLATE_EXPRESSION_SPANS,
        );
        self.exit_node(node);
    }

    fn emit_template_head(&mut self, node: NodeId) {
        self.enter_node(node);
        self.emit_literal(node, GetLiteralTextFlags::NONE);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitTypeAssertionExpression
    fn emit_type_assertion_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (type_node, expression) = match self.arena().data(node) {
            NodeData::TypeAssertionExpression(d) => (d.type_node, d.expression),
            other => panic!("expected TypeAssertionExpression, got {other:?}"),
        };
        self.write_punctuation("<");
        self.emit_type_node_outside_extends(type_node);
        self.write_punctuation(">");
        self.emit_expression(expression, OperatorPrecedence::Unary);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitParenthesizedExpression
    fn emit_parenthesized_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::ParenthesizedExpression(d) => d.expression,
            other => panic!("expected ParenthesizedExpression, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::OpenParenToken, pos, WriteKind::Punctuation, node);
        self.emit_expression(expression, OperatorPrecedence::Comma);
        let close = self.arena().loc(expression).end();
        self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitFunctionExpression
    fn emit_function_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, asterisk, name, type_parameters, parameters, return_type, body) =
            match self.arena().data(node) {
                NodeData::FunctionExpression(d) => (
                    d.modifiers.clone(),
                    d.asterisk_token,
                    d.name,
                    d.type_parameters.clone(),
                    d.parameters.clone(),
                    d.type_node,
                    d.body,
                ),
                other => panic!("expected FunctionExpression, got {other:?}"),
            };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.write_keyword("function");
        if let Some(a) = asterisk {
            self.emit_token_node(a);
        }
        self.write_space();
        self.emit_member_name(name);
        self.push_name_generation_scope(node);
        self.emit_signature_of(node, type_parameters.as_ref(), &parameters, return_type);
        self.emit_function_body_node(body);
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitArrowFunction
    fn emit_arrow_function(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, type_parameters, parameters, return_type, arrow_token, body) =
            match self.arena().data(node) {
                NodeData::ArrowFunction(d) => (
                    d.modifiers.clone(),
                    d.type_parameters.clone(),
                    d.parameters.clone(),
                    d.type_node,
                    d.equals_greater_than_token,
                    d.body,
                ),
                other => panic!("expected ArrowFunction, got {other:?}"),
            };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.push_name_generation_scope(node);
        self.emit_type_parameters(node, type_parameters.as_ref());
        self.emit_parameters_for_arrow(node, &parameters);
        self.emit_type_annotation(return_type);
        self.write_space();
        self.emit_token_node(arrow_token);
        self.write_space();
        self.emit_concise_body(body);
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    fn emit_unary_keyword(&mut self, node: NodeId, keyword: Kind) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::DeleteExpression(d)
            | NodeData::TypeOfExpression(d)
            | NodeData::VoidExpression(d)
            | NodeData::AwaitExpression(d) => d.expression,
            other => panic!("expected unary keyword expression, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(keyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_expression(expression, OperatorPrecedence::Unary);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitPrefixUnaryExpression
    fn emit_prefix_unary_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (operator, operand) = match self.arena().data(node) {
            NodeData::PrefixUnaryExpression(d) => (d.operator, d.operand),
            other => panic!("expected PrefixUnaryExpression, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(operator, pos, WriteKind::Operator, node);
        if self.arena().kind(operand) == Kind::PrefixUnaryExpression {
            let inner = match self.arena().data(operand) {
                NodeData::PrefixUnaryExpression(d) => d.operator,
                _ => unreachable!(),
            };
            if (operator == Kind::PlusToken
                && (inner == Kind::PlusToken || inner == Kind::PlusPlusToken))
                || (operator == Kind::MinusToken
                    && (inner == Kind::MinusToken || inner == Kind::MinusMinusToken))
            {
                self.write_space();
            }
        }
        self.emit_expression(operand, OperatorPrecedence::Unary);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitPostfixUnaryExpression
    fn emit_postfix_unary_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (operator, operand) = match self.arena().data(node) {
            NodeData::PostfixUnaryExpression(d) => (d.operator, d.operand),
            other => panic!("expected PostfixUnaryExpression, got {other:?}"),
        };
        self.emit_expression(operand, OperatorPrecedence::LeftHandSide);
        let pos = self.arena().loc(operand).end();
        self.emit_token(operator, pos, WriteKind::Operator, node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitBinaryExpression
    fn emit_binary_expression(&mut self, node: NodeId) {
        let arena = self.arena();
        let (left, operator_token, right) = match arena.data(node) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            other => panic!("expected BinaryExpression, got {other:?}"),
        };
        let (left_prec, right_prec) = self.get_binary_expression_precedence(node);
        self.enter_node(node);
        self.emit_expression(left, left_prec);
        let lines_before = self.get_lines_between_nodes(node, left, operator_token);
        let lines_after = self.get_lines_between_nodes(node, operator_token, right);
        let op_is_comma = self.arena().kind(operator_token) == Kind::CommaToken;
        self.write_lines_and_indent(lines_before, !op_is_comma);
        self.emit_token_node(operator_token);
        self.write_lines_and_indent(lines_after, true);
        self.emit_expression(right, right_prec);
        self.decrease_indent_if(lines_after > 0);
        self.decrease_indent_if(lines_before > 0);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:getBinaryExpressionPrecedence
    fn get_binary_expression_precedence(
        &self,
        node: NodeId,
    ) -> (OperatorPrecedence, OperatorPrecedence) {
        let arena = self.arena();
        let precedence = get_expression_precedence(arena, node);
        let mut left = precedence;
        let mut right = precedence;
        let (operator_token, right_operand) = match arena.data(node) {
            NodeData::BinaryExpression(d) => (d.operator_token, d.right),
            _ => unreachable!(),
        };
        let op = arena.kind(operator_token);
        match precedence {
            OperatorPrecedence::Comma => {}
            OperatorPrecedence::Assignment => {
                left = OperatorPrecedence::Conditional;
                right = OperatorPrecedence::Yield;
            }
            OperatorPrecedence::LogicalOR => right = OperatorPrecedence::LogicalAND,
            OperatorPrecedence::LogicalAND => right = OperatorPrecedence::BitwiseOR,
            OperatorPrecedence::BitwiseOR
            | OperatorPrecedence::BitwiseXOR
            | OperatorPrecedence::BitwiseAND => {}
            OperatorPrecedence::Equality => right = OperatorPrecedence::Relational,
            OperatorPrecedence::Relational => right = OperatorPrecedence::Shift,
            OperatorPrecedence::Shift => right = OperatorPrecedence::Additive,
            OperatorPrecedence::Additive => {
                if op == Kind::PlusToken
                    && is_binary_operation(arena, right_operand, Kind::PlusToken)
                {
                    let left_kind =
                        self.get_literal_kind_of_binary_plus_operand(self.binary_left(node));
                    if is_literal_kind(left_kind)
                        && left_kind == self.get_literal_kind_of_binary_plus_operand(right_operand)
                    {
                        return (left, right);
                    }
                }
                right = OperatorPrecedence::Multiplicative;
            }
            OperatorPrecedence::Multiplicative => {
                if op == Kind::AsteriskToken
                    && is_binary_operation(arena, right_operand, Kind::AsteriskToken)
                {
                    return (left, right);
                }
                right = OperatorPrecedence::Exponentiation;
            }
            OperatorPrecedence::Exponentiation => left = OperatorPrecedence::Update,
            other => panic!("unhandled precedence: {other:?}"),
        }
        (left, right)
    }

    fn binary_left(&self, node: NodeId) -> NodeId {
        match self.arena().data(node) {
            NodeData::BinaryExpression(d) => d.left,
            _ => unreachable!(),
        }
    }

    // Go: internal/printer/printer.go:getLiteralKindOfBinaryPlusOperand
    fn get_literal_kind_of_binary_plus_operand(&self, node: NodeId) -> Kind {
        let arena = self.arena();
        let node = skip_partially_emitted_expressions(arena, node);
        let kind = arena.kind(node);
        if is_literal_kind(kind) {
            return kind;
        }
        if kind == Kind::BinaryExpression {
            let (left, op, right) = match arena.data(node) {
                NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
                _ => unreachable!(),
            };
            if arena.kind(op) == Kind::PlusToken {
                let left_kind = self.get_literal_kind_of_binary_plus_operand(left);
                if is_literal_kind(left_kind)
                    && left_kind == self.get_literal_kind_of_binary_plus_operand(right)
                {
                    return left_kind;
                }
            }
        }
        Kind::Unknown
    }

    // Go: internal/printer/printer.go:emitShortCircuitExpression
    fn emit_short_circuit_expression(&mut self, node: NodeId) {
        if is_binary_operation(
            self.arena(),
            skip_partially_emitted_expressions(self.arena(), node),
            Kind::QuestionQuestionToken,
        ) {
            self.emit_expression(node, OperatorPrecedence::COALESCE);
        } else {
            self.emit_expression(node, OperatorPrecedence::LogicalOR);
        }
    }

    // Go: internal/printer/printer.go:emitConditionalExpression
    fn emit_conditional_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (condition, question, when_true, colon, when_false) = match self.arena().data(node) {
            NodeData::ConditionalExpression(d) => (
                d.condition,
                d.question_token,
                d.when_true,
                d.colon_token,
                d.when_false,
            ),
            other => panic!("expected ConditionalExpression, got {other:?}"),
        };
        let lines_before_question = self.get_lines_between_nodes(node, condition, question);
        let lines_after_question = self.get_lines_between_nodes(node, question, when_true);
        let lines_before_colon = self.get_lines_between_nodes(node, when_true, colon);
        let lines_after_colon = self.get_lines_between_nodes(node, colon, when_false);
        self.emit_short_circuit_expression(condition);
        self.write_lines_and_indent(lines_before_question, true);
        self.emit_token_node(question);
        self.write_lines_and_indent(lines_after_question, true);
        self.emit_expression(when_true, OperatorPrecedence::Yield);
        self.decrease_indent_if(lines_after_question > 0);
        self.decrease_indent_if(lines_before_question > 0);
        self.write_lines_and_indent(lines_before_colon, true);
        self.emit_token_node(colon);
        self.write_lines_and_indent(lines_after_colon, true);
        self.emit_expression(when_false, OperatorPrecedence::Yield);
        self.decrease_indent_if(lines_after_colon > 0);
        self.decrease_indent_if(lines_before_colon > 0);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitYieldExpression
    fn emit_yield_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (asterisk, expression) = match self.arena().data(node) {
            NodeData::YieldExpression(d) => (d.asterisk_token, d.expression),
            other => panic!("expected YieldExpression, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::YieldKeyword, pos, WriteKind::Keyword, node);
        if let Some(a) = asterisk {
            self.emit_token_node(a);
        }
        if let Some(expression) = expression {
            self.write_space();
            self.emit_expression_no_asi(expression, OperatorPrecedence::DISALLOW_COMMA);
        }
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitSpreadElement
    fn emit_spread_element(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::SpreadElement(d) => d.expression,
            other => panic!("expected SpreadElement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::DotDotDotToken, pos, WriteKind::Punctuation, node);
        self.emit_expression(expression, OperatorPrecedence::DISALLOW_COMMA);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitExpressionWithTypeArguments
    pub(crate) fn emit_expression_with_type_arguments(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, type_arguments) = match self.arena().data(node) {
            NodeData::ExpressionWithTypeArguments(d) => (d.expression, d.type_arguments.clone()),
            other => panic!("expected ExpressionWithTypeArguments, got {other:?}"),
        };
        self.emit_expression(expression, OperatorPrecedence::Member);
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitAsExpression / emitSatisfiesExpression
    fn emit_as_expression(&mut self, node: NodeId, keyword: &str) {
        self.enter_node(node);
        let (expression, type_node) = match self.arena().data(node) {
            NodeData::AsExpression(d) | NodeData::SatisfiesExpression(d) => {
                (d.expression, d.type_node)
            }
            other => panic!("expected As/SatisfiesExpression, got {other:?}"),
        };
        self.emit_expression(expression, OperatorPrecedence::Relational);
        self.write_space();
        self.write_keyword(keyword);
        self.write_space();
        self.emit_type_node_outside_extends(type_node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitNonNullExpression
    fn emit_non_null_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::NonNullExpression(d) => d.expression,
            other => panic!("expected NonNullExpression, got {other:?}"),
        };
        self.emit_expression(expression, OperatorPrecedence::Member);
        self.write_operator("!");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitPartiallyEmittedExpression
    fn emit_partially_emitted_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::PartiallyEmittedExpression(d) => d.expression,
            other => panic!("expected PartiallyEmittedExpression, got {other:?}"),
        };
        // The wrapper is transparent: outer `emit_expression` already chose
        // parenthesization from the skipped-through inner expression, so emit the
        // inner directly without re-applying precedence.
        self.emit_expression_node(expression);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitMetaProperty
    fn emit_meta_property(&mut self, node: NodeId) {
        self.enter_node(node);
        let (keyword_token, name) = match self.arena().data(node) {
            NodeData::MetaProperty(d) => (d.keyword_token, d.name),
            other => panic!("expected MetaProperty, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(keyword_token, pos, WriteKind::Punctuation, node);
        self.write_punctuation(".");
        self.emit_identifier_name(name);
        self.exit_node(node);
    }
}

/// Reports whether `kind` is a literal kind (Go `IsLiteralKind`).
// Go: internal/ast/utilities.go:IsLiteralKind
fn is_literal_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::StringLiteral
            | Kind::RegularExpressionLiteral
            | Kind::NoSubstitutionTemplateLiteral
    )
}

/// Reports whether `node` is a binary expression with the given operator (after
/// skipping partially-emitted wrappers).
// Go: internal/printer/utilities.go:isBinaryOperation
fn is_binary_operation(arena: &tsgo_ast::NodeArena, node: NodeId, token: Kind) -> bool {
    let node = skip_partially_emitted_expressions(arena, node);
    matches!(arena.data(node), NodeData::BinaryExpression(d) if arena.kind(d.operator_token) == token)
}

/// Reports whether `node` is a `new` expression without an argument list.
// Go: internal/printer/utilities.go:isNewExpressionWithoutArguments
fn is_new_expression_without_arguments(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::NewExpression(d) if d.arguments.is_none())
}

/// Reports whether `parent` is a call expression.
fn parent_is_call(arena: &tsgo_ast::NodeArena, parent: NodeId) -> bool {
    arena.kind(parent) == Kind::CallExpression
}

#[cfg(test)]
#[path = "emit_expressions_test.rs"]
mod tests;
