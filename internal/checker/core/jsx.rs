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

use tsgo_ast::{Kind, NodeData, NodeId, SymbolFlags};

use super::declared_types::{
    get_declared_type_of_symbol, get_property_of_type, get_type_of_property_of_type,
};
use super::program::BoundProgram;
use super::symbols::resolve_name;
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
            // The self-closing element node IS the error span for TS7026/TS2339.
            self.check_jsx_opening_like(program, node, tag_name, attributes);
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
    // Go: internal/checker/jsx.go:Checker.checkJsxElement(71) /
    // checkJsxElementDeferred(76)
    pub fn check_jsx_element(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (opening, closing) = match program.arena().data(node) {
            NodeData::JsxElement(d) => (Some(d.opening), Some(d.closing)),
            _ => (None, None),
        };
        if let Some(op) = opening {
            let parts = match program.arena().data(op) {
                NodeData::JsxOpeningElement(d) => Some((d.tag_name, d.attributes)),
                _ => None,
            };
            if let Some((tag_name, attributes)) = parts {
                // The opening element node is the error span for TS7026/TS2339.
                self.check_jsx_opening_like(program, op, tag_name, attributes);
            }
        }
        // Resolve the closing tag too (Go's `checkJsxElementDeferred`): a paired
        // intrinsic `<div>...</div>` reports TS7026 on BOTH the opening AND the
        // closing element; a value closing tag is type-checked so rename /
        // go-to-definition still resolve it.
        if let Some(cl) = closing {
            let closing_tag = match program.arena().data(cl) {
                NodeData::JsxClosingElement(d) => Some(d.tag_name),
                _ => None,
            };
            if let Some(closing_tag) = closing_tag {
                if self.is_intrinsic_tag_name(program, closing_tag) {
                    self.get_jsx_intrinsic_attributes_type(program, cl, closing_tag);
                } else {
                    self.check_expression(program, closing_tag);
                }
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
    // `element_node` is the node a tag-resolution diagnostic (TS7026 / TS2339)
    // is emitted on (Go reports these on the element, not the tag name).
    // Go: internal/checker/jsx.go:Checker.checkJsxOpeningLikeElementOrOpeningFragment
    fn check_jsx_opening_like(
        &mut self,
        program: &dyn BoundProgram,
        element_node: NodeId,
        tag_name: NodeId,
        attributes: NodeId,
    ) {
        let attributes_type = self.resolve_jsx_tag(program, element_node, tag_name);
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
    fn resolve_jsx_tag(
        &mut self,
        program: &dyn BoundProgram,
        element_node: NodeId,
        tag_name: NodeId,
    ) -> Option<TypeId> {
        if self.is_intrinsic_tag_name(program, tag_name) {
            self.get_jsx_intrinsic_attributes_type(program, element_node, tag_name)
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

    // Resolves an intrinsic tag against `JSX.IntrinsicElements`, returning its
    // attributes type (a named member) or `None`. Mirrors Go's
    // `getIntrinsicTagSymbol`: when `JSX.IntrinsicElements` cannot be resolved
    // (its type is `errorType`), the element is implicitly `any` and reports
    // TS7026 under `noImplicitAny`; when it resolves but lacks the tag, the tag
    // reports TS2339. Both diagnostics are emitted on `element_node` (the
    // opening / self-closing / closing element), matching Go's span.
    // Go: internal/checker/jsx.go:Checker.getIntrinsicTagSymbol
    fn get_jsx_intrinsic_attributes_type(
        &mut self,
        program: &dyn BoundProgram,
        element_node: NodeId,
        tag_name: NodeId,
    ) -> Option<TypeId> {
        let name = program.arena().text(tag_name).to_string();
        // Go: `getJsxType(IntrinsicElements, node)`. A StubProgram unit test may
        // inject the resolved `JSX.IntrinsicElements` type directly; otherwise
        // resolve it Go-faithfully from a `JSX` namespace in scope.
        let intrinsic_elements = match self.jsx_intrinsic_elements {
            Some(it) => it,
            None => self.get_jsx_type(program, "IntrinsicElements", tag_name),
        };
        if intrinsic_elements != self.error_type {
            // Property case: the tag must be a declared member of
            // `JSX.IntrinsicElements`.
            if get_property_of_type(self, intrinsic_elements, &name).is_some() {
                get_type_of_property_of_type(self, program, intrinsic_elements, &name)
            } else {
                // Go reports on the element node via `c.error` (trivia-skipped
                // span); the element's `pos` includes the whitespace before `<`.
                self.error_skipping_leading_trivia(
                    program,
                    element_node,
                    &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1,
                    &[name.as_str(), "JSX.IntrinsicElements"],
                );
                None
            }
        } else {
            // No `JSX.IntrinsicElements` interface in scope: the element is
            // implicitly `any`; report it under `noImplicitAny` (the default).
            if self.no_implicit_any() {
                self.error_skipping_leading_trivia(
                    program,
                    element_node,
                    &tsgo_diagnostics::JSX_ELEMENT_IMPLICITLY_HAS_TYPE_ANY_BECAUSE_NO_INTERFACE_JSX_0_EXISTS,
                    &["IntrinsicElements"],
                );
            }
            None
        }
    }

    // Resolves a `JSX.<name>` type by finding the `JSX` namespace in scope and
    // reading its `name` type member's declared type; returns `error_type` when
    // the `JSX` namespace, or the requested member, cannot be resolved.
    //
    // DEFER(phase-4-checker-4i+): Go's `getJsxNamespaceAt` also consults the JSX
    // factory namespace (`@jsx` / `jsxFactory`) and the implicit `jsx-runtime`
    // import; those are the pragma / module-resolution subsystems that gate
    // TS2874 / TS2875 (a later round). This reachable core resolves a `JSX`
    // namespace declared in scope — the `getGlobalSymbol("JSX", ...)` fallback,
    // which is what `declare namespace JSX { ... }` provides.
    // Go: internal/checker/jsx.go:Checker.getJsxType / getJsxNamespaceAt
    fn get_jsx_type(&mut self, program: &dyn BoundProgram, name: &str, location: NodeId) -> TypeId {
        let Some(namespace) = resolve_name(
            program,
            location,
            "JSX",
            SymbolFlags::NAMESPACE,
            false,
            program.globals(),
        ) else {
            return self.error_type;
        };
        // Go: `getSymbol(getExportsOfSymbol(JSX), name, SymbolFlagsType)`.
        let Some(type_symbol) = program.symbol(namespace).exports.get(name).copied() else {
            return self.error_type;
        };
        if !program
            .symbol(type_symbol)
            .flags
            .intersects(SymbolFlags::TYPE)
        {
            return self.error_type;
        }
        get_declared_type_of_symbol(self, program, type_symbol, program.globals())
    }
}

#[cfg(test)]
#[path = "jsx_test.rs"]
mod tests;
