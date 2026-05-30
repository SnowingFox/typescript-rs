//! JSX emit. (`impl Printer` block; see `printer.rs`.)

use crate::printer::{ListEmit, Printer, WriteKind};
use crate::utilities::get_lines_between_positions;
use tsgo_ast::precedence::OperatorPrecedence;
use tsgo_ast::{Kind, NodeData, NodeId};

impl Printer<'_> {
    // Go: internal/printer/printer.go:emitJsxElement
    pub(crate) fn emit_jsx_element(&mut self, node: NodeId) {
        self.enter_node(node);
        let (opening, children, closing) = self.jsx_element_parts(node);
        self.emit_jsx_opening_element(opening);
        self.emit_list(
            ListEmit::JsxChild,
            Some(node),
            Some(&children),
            crate::list_format::ListFormat::JSX_ELEMENT_OR_FRAGMENT_CHILDREN,
        );
        self.emit_jsx_closing_element(closing);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitJsxFragment
    pub(crate) fn emit_jsx_fragment(&mut self, node: NodeId) {
        self.enter_node(node);
        let (opening, children, closing) = self.jsx_element_parts(node);
        self.emit_jsx_opening_fragment(opening);
        self.emit_list(
            ListEmit::JsxChild,
            Some(node),
            Some(&children),
            crate::list_format::ListFormat::JSX_ELEMENT_OR_FRAGMENT_CHILDREN,
        );
        self.emit_jsx_closing_fragment(closing);
        self.exit_node(node);
    }

    fn jsx_element_parts(&self, node: NodeId) -> (NodeId, tsgo_ast::NodeList, NodeId) {
        match self.arena().data(node) {
            NodeData::JsxElement(d) | NodeData::JsxFragment(d) => {
                (d.opening, d.children.clone(), d.closing)
            }
            other => panic!("expected JsxElement/Fragment, got {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitJsxSelfClosingElement
    pub(crate) fn emit_jsx_self_closing_element(&mut self, node: NodeId) {
        self.enter_node(node);
        let (tag_name, type_arguments, attributes) = self.jsx_opening_like_parts(node);
        self.write_punctuation("<");
        self.emit_jsx_tag_name(tag_name);
        self.emit_type_arguments(node, type_arguments.as_ref());
        self.write_space();
        self.emit_jsx_attributes(attributes);
        self.write_punctuation("/>");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitJsxOpeningElement
    fn emit_jsx_opening_element(&mut self, node: NodeId) {
        self.enter_node(node);
        let (tag_name, type_arguments, attributes) = self.jsx_opening_like_parts(node);
        self.write_punctuation("<");
        self.emit_jsx_tag_name(tag_name);
        self.emit_type_arguments(node, type_arguments.as_ref());
        if self.jsx_attributes_non_empty(attributes) {
            self.write_space();
        }
        self.emit_jsx_attributes(attributes);
        self.write_punctuation(">");
        self.exit_node(node);
    }

    fn jsx_opening_like_parts(&self, node: NodeId) -> (NodeId, Option<tsgo_ast::NodeList>, NodeId) {
        match self.arena().data(node) {
            NodeData::JsxOpeningElement(d) | NodeData::JsxSelfClosingElement(d) => {
                (d.tag_name, d.type_arguments.clone(), d.attributes)
            }
            other => panic!("expected JsxOpeningLike, got {other:?}"),
        }
    }

    fn jsx_attributes_non_empty(&self, attributes: NodeId) -> bool {
        match self.arena().data(attributes) {
            NodeData::JsxAttributes(d) => !d.list.nodes.is_empty(),
            _ => false,
        }
    }

    // Go: internal/printer/printer.go:emitJsxClosingElement
    fn emit_jsx_closing_element(&mut self, node: NodeId) {
        self.enter_node(node);
        let tag_name = match self.arena().data(node) {
            NodeData::JsxClosingElement(d) => d.tag_name,
            other => panic!("expected JsxClosingElement, got {other:?}"),
        };
        self.write_punctuation("</");
        self.emit_jsx_tag_name(tag_name);
        self.write_punctuation(">");
        self.exit_node(node);
    }

    fn emit_jsx_opening_fragment(&mut self, node: NodeId) {
        self.enter_node(node);
        self.write_punctuation("<");
        self.write_punctuation(">");
        self.exit_node(node);
    }

    fn emit_jsx_closing_fragment(&mut self, node: NodeId) {
        self.enter_node(node);
        self.write_punctuation("</");
        self.write_punctuation(">");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitJsxText
    fn emit_jsx_text(&mut self, node: NodeId) {
        self.enter_node(node);
        let text = match self.arena().data(node) {
            NodeData::JsxText(d) => d.text.clone(),
            other => panic!("expected JsxText, got {other:?}"),
        };
        self.write_string_literal(&text);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitJsxAttributes
    fn emit_jsx_attributes(&mut self, node: NodeId) {
        self.enter_node(node);
        let properties = match self.arena().data(node) {
            NodeData::JsxAttributes(d) => d.list.clone(),
            other => panic!("expected JsxAttributes, got {other:?}"),
        };
        self.emit_list(
            ListEmit::JsxAttributeLike,
            Some(node),
            Some(&properties),
            crate::list_format::ListFormat::JSX_ELEMENT_ATTRIBUTES,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitJsxAttributeLike
    pub(crate) fn emit_jsx_attribute_like(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::JsxAttribute => self.emit_jsx_attribute(node),
            Kind::JsxSpreadAttribute => self.emit_jsx_spread_attribute(node),
            other => panic!("unhandled JsxAttributeLike: {other:?}"),
        }
    }

    fn emit_jsx_attribute(&mut self, node: NodeId) {
        self.enter_node(node);
        let (name, initializer) = match self.arena().data(node) {
            NodeData::JsxAttribute(d) => (d.name, d.initializer),
            other => panic!("expected JsxAttribute, got {other:?}"),
        };
        self.emit_jsx_attribute_name(name);
        if let Some(initializer) = initializer {
            self.write_punctuation("=");
            self.emit_jsx_attribute_value(initializer);
        }
        self.exit_node(node);
    }

    fn emit_jsx_spread_attribute(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::JsxSpreadAttribute(d) => d.expression,
            other => panic!("expected JsxSpreadAttribute, got {other:?}"),
        };
        self.write_punctuation("{...");
        self.emit_expression(expression, OperatorPrecedence::LOWEST);
        self.write_punctuation("}");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitJsxExpression
    fn emit_jsx_expression(&mut self, node: NodeId) {
        self.enter_node(node);
        let (dot_dot_dot, expression) = match self.arena().data(node) {
            NodeData::JsxExpression(d) => (d.dot_dot_dot_token, d.expression),
            other => panic!("expected JsxExpression, got {other:?}"),
        };
        // Empty expression containers are elided unless they contain comments
        // (the comment case is part of the deferred comment slice).
        if expression.is_some() {
            let loc = self.arena().loc(node);
            let indented = self.current_source_file().is_some()
                && !crate::printer::node_is_synthesized(self.arena(), node)
                && get_lines_between_positions(self.line_starts_ref(), loc.pos(), loc.end()) != 0;
            self.increase_indent_if(indented);
            self.emit_token(
                Kind::OpenBraceToken,
                loc.pos(),
                WriteKind::Punctuation,
                node,
            );
            if let Some(d) = dot_dot_dot {
                self.emit_token_node(d);
            }
            if let Some(expression) = expression {
                self.emit_expression(expression, OperatorPrecedence::DISALLOW_COMMA);
            }
            self.emit_token(
                Kind::CloseBraceToken,
                loc.end(),
                WriteKind::Punctuation,
                node,
            );
            self.decrease_indent_if(indented);
        }
        self.exit_node(node);
    }

    fn emit_jsx_namespaced_name(&mut self, node: NodeId) {
        self.enter_node(node);
        let (namespace, name) = match self.arena().data(node) {
            NodeData::JsxNamespacedName(d) => (d.namespace, d.name),
            other => panic!("expected JsxNamespacedName, got {other:?}"),
        };
        self.emit_identifier_name(namespace);
        self.write_punctuation(":");
        self.emit_identifier_name(name);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitJsxChild
    pub(crate) fn emit_jsx_child(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::JsxText => self.emit_jsx_text(node),
            Kind::JsxExpression => self.emit_jsx_expression(node),
            Kind::JsxElement => self.emit_jsx_element(node),
            Kind::JsxSelfClosingElement => self.emit_jsx_self_closing_element(node),
            Kind::JsxFragment => self.emit_jsx_fragment(node),
            other => panic!("unhandled JsxChild: {other:?}"),
        }
    }

    fn emit_jsx_tag_name(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::Identifier => self.emit_identifier_name(node),
            Kind::ThisKeyword => self.emit_keyword_node(Some(node)),
            Kind::JsxNamespacedName => self.emit_jsx_namespaced_name(node),
            Kind::PropertyAccessExpression => self.emit_expression_node(node),
            other => panic!("unhandled JsxTagName: {other:?}"),
        }
    }

    fn emit_jsx_attribute_name(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::Identifier => self.emit_identifier_name(node),
            Kind::JsxNamespacedName => self.emit_jsx_namespaced_name(node),
            other => panic!("unhandled JsxAttributeName: {other:?}"),
        }
    }

    fn emit_jsx_attribute_value(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::StringLiteral => self.emit_string_literal_member(node),
            Kind::JsxExpression => self.emit_jsx_expression(node),
            Kind::JsxElement => self.emit_jsx_element(node),
            Kind::JsxSelfClosingElement => self.emit_jsx_self_closing_element(node),
            Kind::JsxFragment => self.emit_jsx_fragment(node),
            _ => self.emit_expression(node, OperatorPrecedence::LOWEST),
        }
    }
}

#[cfg(test)]
#[path = "emit_jsx_test.rs"]
mod tests;
