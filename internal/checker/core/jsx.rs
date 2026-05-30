//! JSX element checking.
//!
//! Ports the reachable core of Go's `jsx.go`: `check_jsx_element` /
//! `check_jsx_self_closing_element` / `check_jsx_fragment`, intrinsic-vs-value
//! tag resolution, attribute (props) assignability checking, and children
//! typing.
//!
//! The result type of a JSX element is Go's `JSX.Element` (a lib global); until
//! those land (P6) the checks return `any` and resolve intrinsic tags against an
//! injected `JSX.IntrinsicElements` table (see
//! [`Checker::set_jsx_intrinsic_elements`]).
//!
//! DEFER(phase-4-checker-4i+): grammar checks, the JSX factory/pragma machinery,
//! spread attributes, namespaced names, stateless/class component signature
//! resolution, and the real `JSX.Element`/`JSX.ElementType` constraints.

use tsgo_ast::{Kind, NodeData, NodeId};

use super::declared_types::{get_property_of_type, get_type_of_property_of_type};
use super::program::BoundProgram;
use super::types::TypeId;
use super::Checker;

impl Checker {
    /// Checks a self-closing JSX element (`<a ... />`), returning its type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) {
    /// let _ = c.check_jsx_self_closing_element(p, n);
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics and allocate types.
    // Go: internal/checker/jsx.go:Checker.checkJsxSelfClosingElement(100)
    pub fn check_jsx_self_closing_element(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        let opening = match program.arena().data(node) {
            NodeData::JsxSelfClosingElement(d) => Some((d.tag_name, d.attributes)),
            _ => None,
        };
        if let Some((tag_name, attributes)) = opening {
            self.check_jsx_opening_like(program, tag_name, attributes);
        }
        // DEFER(phase-4-checker-4i+): the element type is `JSX.Element`.
        // blocked-by: lib globals (P6).
        self.any_type
    }

    /// Checks a paired JSX element (`<a>...</a>`), returning its type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) {
    /// let _ = c.check_jsx_element(p, n);
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics and allocate types.
    // Go: internal/checker/jsx.go:Checker.checkJsxElement(71)
    pub fn check_jsx_element(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let opening = match program.arena().data(node) {
            NodeData::JsxElement(d) => Some(d.opening),
            _ => None,
        };
        if let Some(op) = opening {
            let parts = match program.arena().data(op) {
                NodeData::JsxOpeningElement(d) => Some((d.tag_name, d.attributes)),
                _ => None,
            };
            if let Some((tag_name, attributes)) = parts {
                self.check_jsx_opening_like(program, tag_name, attributes);
            }
        }
        self.check_jsx_children(program, node);
        // DEFER(phase-4-checker-4i+): the element type is `JSX.Element`.
        // blocked-by: lib globals (P6).
        self.any_type
    }

    /// Checks a JSX fragment (`<>...</>`), returning its type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) {
    /// let _ = c.check_jsx_fragment(p, n);
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics and allocate types.
    // Go: internal/checker/jsx.go:Checker.checkJsxFragment(109)
    pub fn check_jsx_fragment(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        // DEFER(phase-4-checker-4i+): JSX factory/pragma grammar checks for
        // fragments. blocked-by: compiler options + pragmas (P6).
        self.check_jsx_children(program, node);
        self.any_type
    }

    // Types each `{expr}` child of a JSX element/fragment.
    // Go: internal/checker/jsx.go:Checker.checkJsxChildren
    fn check_jsx_children(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let children: Vec<NodeId> = match program.arena().data(node) {
            NodeData::JsxElement(d) | NodeData::JsxFragment(d) => d.children.nodes.clone(),
            _ => return,
        };
        for child in children {
            // DEFER(phase-4-checker-4i+): JsxText and nested-element children
            // typing against the element-children type.
            if program.arena().kind(child) == Kind::JsxExpression {
                let inner = match program.arena().data(child) {
                    NodeData::JsxExpression(d) => d.expression,
                    _ => None,
                };
                if let Some(expr) = inner {
                    self.check_expression(program, expr);
                }
            }
        }
    }

    // Resolves the tag and checks the attributes of an opening-like element.
    // Go: internal/checker/jsx.go:Checker.checkJsxOpeningLikeElementOrOpeningFragment
    fn check_jsx_opening_like(
        &mut self,
        program: &dyn BoundProgram,
        tag_name: NodeId,
        attributes: NodeId,
    ) {
        let attributes_type = self.resolve_jsx_tag(program, tag_name);
        self.check_jsx_attributes(program, attributes, attributes_type);
    }

    // Checks each attribute value against its declared property on the element's
    // attributes type, emitting an assignability diagnostic on mismatch.
    // Go: internal/checker/jsx.go:Checker.checkJsxAttributes / checkJsxAttribute
    fn check_jsx_attributes(
        &mut self,
        program: &dyn BoundProgram,
        attributes: NodeId,
        attributes_type: Option<TypeId>,
    ) {
        let entries: Vec<(NodeId, Option<NodeId>)> = match program.arena().data(attributes) {
            NodeData::JsxAttributes(d) => d
                .list
                .nodes
                .iter()
                .filter_map(|&n| match program.arena().data(n) {
                    NodeData::JsxAttribute(a) => Some((a.name, a.initializer)),
                    // DEFER(phase-4-checker-4i+): spread attributes.
                    _ => None,
                })
                .collect(),
            _ => return,
        };
        for (name_node, initializer) in entries {
            // DEFER(phase-4-checker-4i+): boolean-shorthand attributes (`<a b/>`).
            let Some(init) = initializer else { continue };
            let value_type = self.check_jsx_attribute_value(program, init);
            let Some(attrs_type) = attributes_type else {
                continue;
            };
            let attr_name = program.arena().text(name_node).to_string();
            // DEFER(phase-4-checker-4i+): excess-property check when the attribute
            // is not declared on the attributes type.
            if let Some(prop_type) =
                get_type_of_property_of_type(self, program, attrs_type, &attr_name)
            {
                if !self.is_type_assignable_to(program, value_type, prop_type) {
                    let source = super::nodebuilder::type_to_string(self, program, value_type);
                    let target = super::nodebuilder::type_to_string(self, program, prop_type);
                    self.error(
                        program,
                        init,
                        &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
                        &[source.as_str(), target.as_str()],
                    );
                }
            }
        }
    }

    // Types a JSX attribute value: a `{expr}` container's inner expression, or a
    // bare string-literal initializer.
    // Go: internal/checker/jsx.go:Checker.checkJsxAttribute (initializer typing)
    fn check_jsx_attribute_value(&mut self, program: &dyn BoundProgram, init: NodeId) -> TypeId {
        if program.arena().kind(init) == Kind::JsxExpression {
            let inner = match program.arena().data(init) {
                NodeData::JsxExpression(d) => d.expression,
                _ => None,
            };
            return match inner {
                Some(expr) => self.check_expression(program, expr),
                None => self.error_type,
            };
        }
        self.check_expression(program, init)
    }

    // Resolves a JSX tag to its attributes type (intrinsic) or `None`.
    // Go: internal/checker/jsx.go:Checker.getIntrinsicTagSymbol / checkExpression(tagName)
    fn resolve_jsx_tag(&mut self, program: &dyn BoundProgram, tag_name: NodeId) -> Option<TypeId> {
        if self.is_intrinsic_tag_name(program, tag_name) {
            self.get_jsx_intrinsic_attributes_type(program, tag_name)
        } else {
            // Value element (component): typing the tag name reports "Cannot find
            // name" for an unresolved component (via 4g `check_expression`).
            self.check_expression(program, tag_name);
            // DEFER(phase-4-checker-4i+): derive the component's props (attributes)
            // type from its (call/construct/function) signature.
            // blocked-by: callable types from declarations + `JSX.ElementType`.
            None
        }
    }

    // An identifier whose first character is lowercase is an intrinsic tag.
    // Go: internal/checker/jsx.go:isJsxIntrinsicTagName
    fn is_intrinsic_tag_name(&self, program: &dyn BoundProgram, tag_name: NodeId) -> bool {
        if program.arena().kind(tag_name) != Kind::Identifier {
            // DEFER(phase-4-checker-4i+): namespaced names (`ns:tag`) are intrinsic.
            return false;
        }
        program
            .arena()
            .text(tag_name)
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase())
    }

    // Looks the tag up as a property of the injected `JSX.IntrinsicElements`.
    // Go: internal/checker/jsx.go:Checker.getIntrinsicTagSymbol
    fn get_jsx_intrinsic_attributes_type(
        &mut self,
        program: &dyn BoundProgram,
        tag_name: NodeId,
    ) -> Option<TypeId> {
        let name = program.arena().text(tag_name).to_string();
        // blocked-by: lib globals (P6) — without a `JSX.IntrinsicElements` table
        // the tag is treated as implicitly `any` (no error in non-strict mode).
        let intrinsic_elements = self.jsx_intrinsic_elements?;
        if get_property_of_type(self, intrinsic_elements, &name).is_some() {
            get_type_of_property_of_type(self, program, intrinsic_elements, &name)
        } else {
            self.error(
                program,
                tag_name,
                &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1,
                &[name.as_str(), "JSX.IntrinsicElements"],
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "jsx_test.rs"]
mod tests;
