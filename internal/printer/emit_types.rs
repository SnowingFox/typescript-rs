//! Type-node emit, including the type-precedence parenthesizer.
//! (`impl Printer` block; see `printer.rs`.)

use crate::printer::{ListEmit, Printer, WriteKind};
use crate::utilities::GetLiteralTextFlags;
use tsgo_ast::precedence::OperatorPrecedence;
use tsgo_ast::{Kind, NodeData, NodeId};

/// Type-node precedence, lowest (`Conditional`) to highest (`NonArray`).
///
/// Mirrors Go `TypePrecedence`.
// Go: internal/ast/precedence.go:TypePrecedence
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(i32)]
pub(crate) enum TypePrecedence {
    Conditional = 0,
    JsDoc,
    Function,
    Union,
    Intersection,
    TypeOperator,
    Postfix,
    NonArray,
}

impl TypePrecedence {
    const LOWEST: TypePrecedence = TypePrecedence::Conditional;
    const HIGHEST: TypePrecedence = TypePrecedence::NonArray;
}

/// Returns the precedence of a type node.
// Go: internal/ast/precedence.go:GetTypeNodePrecedence
fn get_type_node_precedence(arena: &tsgo_ast::NodeArena, node: NodeId) -> TypePrecedence {
    match arena.kind(node) {
        Kind::ConditionalType => TypePrecedence::Conditional,
        Kind::JSDocOptionalType | Kind::JSDocVariadicType => TypePrecedence::JsDoc,
        Kind::FunctionType | Kind::ConstructorType => TypePrecedence::Function,
        Kind::UnionType => TypePrecedence::Union,
        Kind::IntersectionType => TypePrecedence::Intersection,
        Kind::TypeOperator => TypePrecedence::TypeOperator,
        Kind::InferType => {
            // `infer X extends C` must be parenthesized like a function type.
            let has_constraint = match arena.data(node) {
                NodeData::InferType(d) => matches!(
                    arena.data(d.type_parameter),
                    NodeData::TypeParameterDeclaration(tp) if tp.constraint.is_some()
                ),
                _ => false,
            };
            if has_constraint {
                TypePrecedence::Function
            } else {
                TypePrecedence::TypeOperator
            }
        }
        Kind::IndexedAccessType | Kind::ArrayType | Kind::OptionalType => TypePrecedence::Postfix,
        Kind::TypeQuery => TypePrecedence::TypeOperator,
        _ => TypePrecedence::NonArray,
    }
}

impl Printer<'_> {
    // Go: internal/printer/printer.go:emitTypeNodeOutsideExtends
    pub(crate) fn emit_type_node_outside_extends(&mut self, node: NodeId) {
        let saved = self.in_extends;
        self.in_extends = false;
        self.emit_type_node(node, TypePrecedence::LOWEST);
        self.in_extends = saved;
    }

    // Go: internal/printer/printer.go:emitUnionTypeConstituent / emitIntersectionTypeConstituent
    pub(crate) fn emit_type_constituent(&mut self, node: NodeId) {
        self.emit_type_node(node, TypePrecedence::TypeOperator);
    }

    // Go: internal/printer/printer.go:emitTypeNodeInExtends
    fn emit_type_node_in_extends(&mut self, node: NodeId) {
        let saved = self.in_extends;
        self.in_extends = true;
        self.emit_type_node(node, TypePrecedence::LOWEST);
        self.in_extends = saved;
    }

    // Go: internal/printer/printer.go:emitTypeNode
    fn emit_type_node(&mut self, node: NodeId, precedence: TypePrecedence) {
        let mut precedence = precedence;
        if self.in_extends && precedence <= TypePrecedence::Conditional {
            precedence = TypePrecedence::Function;
        }
        let saved_in_extends = self.in_extends;
        let parens = get_type_node_precedence(self.arena(), node) < precedence;
        if parens {
            self.in_extends = false;
            self.write_punctuation("(");
        }
        self.emit_type_node_body(node);
        if parens {
            self.write_punctuation(")");
        }
        self.in_extends = saved_in_extends;
    }

    fn emit_type_node_body(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::AnyKeyword
            | Kind::UnknownKeyword
            | Kind::NumberKeyword
            | Kind::BigIntKeyword
            | Kind::ObjectKeyword
            | Kind::BooleanKeyword
            | Kind::StringKeyword
            | Kind::SymbolKeyword
            | Kind::VoidKeyword
            | Kind::UndefinedKeyword
            | Kind::NeverKeyword
            | Kind::IntrinsicKeyword => self.emit_keyword_node(Some(node)),
            Kind::TypePredicate => self.emit_type_predicate(node),
            Kind::TypeReference => self.emit_type_reference(node),
            Kind::FunctionType => self.emit_function_or_constructor_type(node, false),
            Kind::ConstructorType => self.emit_function_or_constructor_type(node, true),
            Kind::TypeQuery => self.emit_type_query(node),
            Kind::TypeLiteral => self.emit_type_literal(node),
            Kind::ArrayType => self.emit_array_type(node),
            Kind::TupleType => self.emit_tuple_type(node),
            Kind::OptionalType => self.emit_optional_type(node),
            Kind::RestType => self.emit_rest_type(node),
            Kind::UnionType => self.emit_union_or_intersection_type(node, true),
            Kind::IntersectionType => self.emit_union_or_intersection_type(node, false),
            Kind::ConditionalType => self.emit_conditional_type(node),
            Kind::InferType => self.emit_infer_type(node),
            Kind::ParenthesizedType => self.emit_parenthesized_type(node),
            Kind::ThisType => self.write_keyword("this"),
            Kind::TypeOperator => self.emit_type_operator(node),
            Kind::IndexedAccessType => self.emit_indexed_access_type(node),
            Kind::MappedType => self.emit_mapped_type(node),
            Kind::LiteralType => self.emit_literal_type(node),
            Kind::NamedTupleMember => self.emit_named_tuple_member(node),
            Kind::TemplateLiteralType => self.emit_template_type(node),
            Kind::ImportType => self.emit_import_type(node),
            other => panic!("unhandled TypeNode: {other:?}"),
        }
    }

    fn emit_type_predicate(&mut self, node: NodeId) {
        self.enter_node(node);
        let (asserts, name, type_node) = match self.arena().data(node) {
            NodeData::TypePredicate(d) => (d.asserts_modifier, d.parameter_name, d.type_node),
            other => panic!("expected TypePredicate, got {other:?}"),
        };
        if let Some(asserts) = asserts {
            self.emit_token_node(asserts);
            self.write_space();
        }
        if self.arena().kind(name) == Kind::ThisType {
            self.write_keyword("this");
        } else {
            self.emit_identifier_name(name);
        }
        if let Some(type_node) = type_node {
            self.write_space();
            self.write_keyword("is");
            self.write_space();
            self.emit_type_node_outside_extends(type_node);
        }
        self.exit_node(node);
    }

    fn emit_type_reference(&mut self, node: NodeId) {
        self.enter_node(node);
        let (type_name, type_arguments) = match self.arena().data(node) {
            NodeData::TypeReference(d) => (d.type_name, d.type_arguments.clone()),
            other => panic!("expected TypeReference, got {other:?}"),
        };
        self.emit_entity_name(type_name);
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.exit_node(node);
    }

    fn emit_function_or_constructor_type(&mut self, node: NodeId, is_constructor: bool) {
        self.enter_node(node);
        let (modifiers, type_params, params, ret) = match self.arena().data(node) {
            NodeData::FunctionType(d) | NodeData::ConstructorType(d) => (
                d.modifiers.clone(),
                d.type_parameters.clone(),
                d.parameters.clone(),
                d.type_node,
            ),
            other => panic!("expected function/constructor type, got {other:?}"),
        };
        if is_constructor {
            self.emit_modifier_list(node, modifiers.as_ref(), false);
            self.write_keyword("new");
            self.write_space();
        }
        self.push_name_generation_scope(node);
        self.emit_type_parameters(node, type_params.as_ref());
        self.emit_parameters_for_signature_type(node, &params);
        self.write_space();
        self.emit_return_type(ret);
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    fn emit_parameters_for_signature_type(&mut self, node: NodeId, params: &tsgo_ast::NodeList) {
        self.emit_list(
            ListEmit::Parameter,
            Some(node),
            Some(params),
            crate::list_format::ListFormat::PARAMETERS,
        );
    }

    // Go: internal/printer/printer.go:emitReturnType
    fn emit_return_type(&mut self, node: Option<NodeId>) {
        let Some(node) = node else { return };
        self.write_punctuation("=>");
        self.write_space();
        // `infer X extends C` in an `extends` clause must be parenthesized.
        let infer_extends = self.in_extends
            && self.arena().kind(node) == Kind::InferType
            && matches!(
                self.arena().data(node),
                NodeData::InferType(d) if matches!(
                    self.arena().data(d.type_parameter),
                    NodeData::TypeParameterDeclaration(tp) if tp.constraint.is_some()
                )
            );
        if infer_extends {
            self.emit_type_node(node, TypePrecedence::HIGHEST);
        } else {
            self.emit_type_node(node, TypePrecedence::LOWEST);
        }
    }

    fn emit_type_query(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expr_name, type_arguments) = match self.arena().data(node) {
            NodeData::TypeQuery(d) => (d.expr_name, d.type_arguments.clone()),
            other => panic!("expected TypeQuery, got {other:?}"),
        };
        self.write_keyword("typeof");
        self.write_space();
        self.emit_entity_name(expr_name);
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.exit_node(node);
    }

    fn emit_type_literal(&mut self, node: NodeId) {
        self.enter_node(node);
        let members = match self.arena().data(node) {
            NodeData::TypeLiteral(d) => d.members.clone(),
            other => panic!("expected TypeLiteral, got {other:?}"),
        };
        self.push_name_generation_scope(node);
        self.write_punctuation("{");
        let mut flags = if self.should_emit_on_single_line(node) {
            crate::list_format::ListFormat::SINGLE_LINE_TYPE_LITERAL_MEMBERS
        } else {
            crate::list_format::ListFormat::MULTI_LINE_TYPE_LITERAL_MEMBERS
        };
        flags |= crate::list_format::ListFormat::NO_SPACE_IF_EMPTY;
        self.emit_list(ListEmit::TypeElement, Some(node), Some(&members), flags);
        self.write_punctuation("}");
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    fn emit_array_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let element_type = match self.arena().data(node) {
            NodeData::ArrayType(d) => d.element_type,
            other => panic!("expected ArrayType, got {other:?}"),
        };
        self.emit_postfix_type_operand(element_type, node);
        self.write_punctuation("[");
        self.write_punctuation("]");
        self.exit_node(node);
    }

    fn emit_postfix_type_operand(&mut self, operand: NodeId, parent: NodeId) {
        // Preserve a parsed `typeof X` postfix operand without extra parens.
        if !crate::printer::node_is_synthesized(self.arena(), parent)
            && self.arena().kind(operand) == Kind::TypeQuery
        {
            self.emit_type_node(operand, TypePrecedence::TypeOperator);
            return;
        }
        self.emit_type_node(operand, TypePrecedence::Postfix);
    }

    fn emit_tuple_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let elements = match self.arena().data(node) {
            NodeData::TupleType(d) => d.types.clone(),
            other => panic!("expected TupleType, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::OpenBracketToken, pos, WriteKind::Punctuation, node);
        let mut flags = if self.should_emit_on_single_line(node) {
            crate::list_format::ListFormat::SINGLE_LINE_TUPLE_TYPE_ELEMENTS
        } else {
            crate::list_format::ListFormat::MULTI_LINE_TUPLE_TYPE_ELEMENTS
        };
        flags |= crate::list_format::ListFormat::NO_SPACE_IF_EMPTY;
        self.emit_list(ListEmit::TypeNode, Some(node), Some(&elements), flags);
        self.emit_token(
            Kind::CloseBracketToken,
            elements.end(),
            WriteKind::Punctuation,
            node,
        );
        self.exit_node(node);
    }

    fn emit_rest_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let type_node = match self.arena().data(node) {
            NodeData::RestType(d) => d.type_node,
            other => panic!("expected RestType, got {other:?}"),
        };
        self.write_punctuation("...");
        self.emit_type_node_outside_extends(type_node);
        self.exit_node(node);
    }

    fn emit_optional_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let type_node = match self.arena().data(node) {
            NodeData::OptionalType(d) => d.type_node,
            other => panic!("expected OptionalType, got {other:?}"),
        };
        self.emit_postfix_type_operand(type_node, node);
        self.write_punctuation("?");
        self.exit_node(node);
    }

    fn emit_named_tuple_member(&mut self, node: NodeId) {
        self.enter_node(node);
        let (dot_dot_dot, name, question, type_node) = match self.arena().data(node) {
            NodeData::NamedTupleMember(d) => {
                (d.dot_dot_dot_token, d.name, d.question_token, d.type_node)
            }
            other => panic!("expected NamedTupleMember, got {other:?}"),
        };
        if let Some(d) = dot_dot_dot {
            self.emit_token_node(d);
        }
        self.emit_identifier_name(name);
        if let Some(q) = question {
            self.emit_token_node(q);
        }
        let colon_pos = self.arena().loc(name).end();
        self.emit_token(Kind::ColonToken, colon_pos, WriteKind::Punctuation, node);
        self.write_space();
        self.emit_type_node_outside_extends(type_node);
        self.exit_node(node);
    }

    fn emit_union_or_intersection_type(&mut self, node: NodeId, is_union: bool) {
        self.enter_node(node);
        let types = match self.arena().data(node) {
            NodeData::UnionType(d) | NodeData::IntersectionType(d) => d.types.clone(),
            other => panic!("expected union/intersection type, got {other:?}"),
        };
        let format = if is_union {
            crate::list_format::ListFormat::UNION_TYPE_CONSTITUENTS
        } else {
            crate::list_format::ListFormat::INTERSECTION_TYPE_CONSTITUENTS
        };
        self.emit_list(ListEmit::TypeConstituent, Some(node), Some(&types), format);
        self.exit_node(node);
    }

    fn emit_conditional_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let (check_type, extends_type, true_type, false_type) = match self.arena().data(node) {
            NodeData::ConditionalType(d) => {
                (d.check_type, d.extends_type, d.true_type, d.false_type)
            }
            other => panic!("expected ConditionalType, got {other:?}"),
        };
        self.emit_type_node(check_type, TypePrecedence::Union);
        self.write_space();
        self.write_keyword("extends");
        self.write_space();
        self.emit_type_node_in_extends(extends_type);
        self.write_space();
        self.write_punctuation("?");
        self.write_space();
        self.emit_type_node_outside_extends(true_type);
        self.write_space();
        self.write_punctuation(":");
        self.write_space();
        self.emit_type_node_outside_extends(false_type);
        self.exit_node(node);
    }

    fn emit_infer_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let type_parameter = match self.arena().data(node) {
            NodeData::InferType(d) => d.type_parameter,
            other => panic!("expected InferType, got {other:?}"),
        };
        self.write_keyword("infer");
        self.write_space();
        self.emit_infer_type_parameter(type_parameter);
        self.exit_node(node);
    }

    fn emit_infer_type_parameter(&mut self, node: NodeId) {
        self.enter_node(node);
        let (name, constraint) = match self.arena().data(node) {
            NodeData::TypeParameterDeclaration(d) => (d.name, d.constraint),
            other => panic!("expected TypeParameter, got {other:?}"),
        };
        self.emit_identifier_name(name);
        if let Some(constraint) = constraint {
            self.write_space();
            self.write_keyword("extends");
            self.write_space();
            self.emit_type_node_in_extends(constraint);
        }
        self.exit_node(node);
    }

    fn emit_parenthesized_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let type_node = match self.arena().data(node) {
            NodeData::ParenthesizedType(d) => d.type_node,
            other => panic!("expected ParenthesizedType, got {other:?}"),
        };
        self.write_punctuation("(");
        self.emit_type_node_outside_extends(type_node);
        self.write_punctuation(")");
        self.exit_node(node);
    }

    fn emit_type_operator(&mut self, node: NodeId) {
        self.enter_node(node);
        let (operator, type_node) = match self.arena().data(node) {
            NodeData::TypeOperator(d) => (d.operator, d.type_node),
            other => panic!("expected TypeOperator, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(operator, pos, WriteKind::Keyword, node);
        self.write_space();
        let prec = if operator == Kind::ReadonlyKeyword {
            TypePrecedence::Postfix
        } else {
            TypePrecedence::TypeOperator
        };
        self.emit_type_node(type_node, prec);
        self.exit_node(node);
    }

    fn emit_indexed_access_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let (object_type, index_type) = match self.arena().data(node) {
            NodeData::IndexedAccessType(d) => (d.object_type, d.index_type),
            other => panic!("expected IndexedAccessType, got {other:?}"),
        };
        self.emit_postfix_type_operand(object_type, node);
        self.write_punctuation("[");
        self.emit_type_node_outside_extends(index_type);
        self.write_punctuation("]");
        self.exit_node(node);
    }

    fn emit_mapped_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let (readonly_token, type_parameter, name_type, question_token, type_node, members) =
            match self.arena().data(node) {
                NodeData::MappedType(d) => (
                    d.readonly_token,
                    d.type_parameter,
                    d.name_type,
                    d.question_token,
                    d.type_node,
                    d.members.clone(),
                ),
                other => panic!("expected MappedType, got {other:?}"),
            };
        let single_line = self.should_emit_on_single_line(node);
        self.write_punctuation("{");
        if single_line {
            self.write_space();
        } else {
            self.write_line();
            self.increase_indent();
        }
        if let Some(readonly_token) = readonly_token {
            self.emit_token_node(readonly_token);
            if self.arena().kind(readonly_token) != Kind::ReadonlyKeyword {
                self.write_keyword("readonly");
            }
            self.write_space();
        }
        self.write_punctuation("[");
        self.emit_mapped_type_parameter(type_parameter);
        if let Some(name_type) = name_type {
            self.write_space();
            self.write_keyword("as");
            self.write_space();
            self.emit_type_node_outside_extends(name_type);
        }
        self.write_punctuation("]");
        if let Some(question_token) = question_token {
            self.emit_token_node(question_token);
            if self.arena().kind(question_token) != Kind::QuestionToken {
                self.write_punctuation("?");
            }
        }
        self.write_punctuation(":");
        self.write_space();
        if let Some(type_node) = type_node {
            self.emit_type_node_outside_extends(type_node);
        }
        self.write_trailing_semicolon();
        if !members.nodes.is_empty() {
            if single_line {
                self.write_space();
            } else {
                self.write_line();
            }
            self.emit_list(
                ListEmit::TypeElement,
                Some(node),
                Some(&members),
                crate::list_format::ListFormat::PRESERVE_LINES,
            );
        }
        if single_line {
            self.write_space();
        } else {
            self.write_line();
            self.decrease_indent();
        }
        self.write_punctuation("}");
        self.exit_node(node);
    }

    fn emit_mapped_type_parameter(&mut self, node: NodeId) {
        self.enter_node(node);
        let (name, constraint) = match self.arena().data(node) {
            NodeData::TypeParameterDeclaration(d) => (d.name, d.constraint),
            other => panic!("expected mapped TypeParameter, got {other:?}"),
        };
        self.emit_identifier_name(name);
        self.write_space();
        self.write_keyword("in");
        self.write_space();
        if let Some(constraint) = constraint {
            self.emit_type_node_outside_extends(constraint);
        }
        self.exit_node(node);
    }

    fn emit_literal_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let literal = match self.arena().data(node) {
            NodeData::LiteralType(d) => d.literal,
            other => panic!("expected LiteralType, got {other:?}"),
        };
        self.emit_expression(literal, OperatorPrecedence::Comma);
        self.exit_node(node);
    }

    fn emit_template_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let (head, spans) = match self.arena().data(node) {
            NodeData::TemplateLiteralType(d) => (d.head, d.template_spans.clone()),
            other => panic!("expected TemplateLiteralType, got {other:?}"),
        };
        self.emit_template_head_literal(head);
        self.emit_list(
            ListEmit::TemplateTypeSpan,
            Some(node),
            Some(&spans),
            crate::list_format::ListFormat::TEMPLATE_EXPRESSION_SPANS,
        );
        self.exit_node(node);
    }

    pub(crate) fn emit_template_type_span(&mut self, node: NodeId) {
        self.enter_node(node);
        let (type_node, literal) = match self.arena().data(node) {
            NodeData::TemplateLiteralTypeSpan(d) => (d.expression, d.literal),
            other => panic!("expected TemplateLiteralTypeSpan, got {other:?}"),
        };
        self.emit_type_node_outside_extends(type_node);
        self.emit_template_head_literal(literal);
        self.exit_node(node);
    }

    fn emit_template_head_literal(&mut self, node: NodeId) {
        self.enter_node(node);
        self.emit_literal(node, GetLiteralTextFlags::NONE);
        self.exit_node(node);
    }

    fn emit_import_type(&mut self, node: NodeId) {
        self.enter_node(node);
        let (is_type_of, argument, attributes, qualifier, type_arguments) =
            match self.arena().data(node) {
                NodeData::ImportType(d) => (
                    d.is_type_of,
                    d.argument,
                    d.attributes,
                    d.qualifier,
                    d.type_arguments.clone(),
                ),
                other => panic!("expected ImportType, got {other:?}"),
            };
        if is_type_of {
            self.write_keyword("typeof");
            self.write_space();
        }
        self.write_keyword("import");
        self.write_punctuation("(");
        self.emit_type_node_outside_extends(argument);
        if let Some(attributes) = attributes {
            self.write_punctuation(",");
            self.write_space();
            self.emit_import_type_attributes(attributes);
        }
        self.write_punctuation(")");
        if let Some(qualifier) = qualifier {
            self.write_punctuation(".");
            self.emit_entity_name(qualifier);
        }
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.exit_node(node);
    }

    fn emit_import_type_attributes(&mut self, node: NodeId) {
        self.enter_node(node);
        let (token, attributes) = match self.arena().data(node) {
            NodeData::ImportAttributes(d) => (d.token, d.attributes.clone()),
            other => panic!("expected ImportAttributes, got {other:?}"),
        };
        self.write_punctuation("{");
        self.write_space();
        self.write_keyword(if token == Kind::AssertKeyword {
            "assert"
        } else {
            "with"
        });
        self.write_punctuation(":");
        self.write_space();
        self.emit_list(
            ListEmit::ImportAttribute,
            Some(node),
            Some(&attributes),
            crate::list_format::ListFormat::IMPORT_ATTRIBUTES,
        );
        self.write_space();
        self.write_punctuation("}");
        self.exit_node(node);
    }
}

#[cfg(test)]
#[path = "emit_types_test.rs"]
mod tests;
