//! Port of Go `internal/transformers/jsxtransforms/jsx.go`: the JSX transform.
//!
//! # Scope (round 6f)
//!
//! Lowers the **classic runtime** (`React.createElement`): self-closing and
//! container elements and fragments, with intrinsic (string) and component
//! (identifier) tags, string/expression attributes → a props object (or `null`),
//! and expression/text/nested-element children → trailing arguments.
//!
//! # Divergence from Go / Deferred (DEFER(P5))
//!
//! The Go transform selects the runtime and factory from `compilerOptions`
//! (`--jsx react`/`react-jsx`/`react-jsxdev`, `--jsxFactory`,
//! `--jsxImportSource`) and uses the emit resolver. The Rust `TransformOptions`
//! carries only the emit context (no `compiler_options`/`emit_resolver`), so this
//! port **hardcodes the classic `React.createElement`/`React.Fragment` factory**.
//! DEFER: the **automatic runtime** (`jsx`/`jsxs`/`jsxDEV` + implicit
//! `react/jsx-runtime` import injection — needs `compilerOptions` + the emit
//! resolver's `SetReferencedImportDeclaration`); custom `@jsxFactory`/
//! `@jsxImportSource`/`@jsxRuntime` pragmas; `React` namespace substitution
//! inside `namespace` blocks (needs the resolver); spread attributes (`{...p}`);
//! `key`-after-spread `createElement` fallback; JSX entity decoding and the exact
//! first/last-line whitespace preservation in `fixupWhitespaceAndDecodeEntities`
//! (the current port collapses internal whitespace and drops all-whitespace
//! runs); and `_jsxFileName`/dev-mode source positions.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, TokenFlags, VisitOptions};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers JSX, sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{jsxtransforms::jsx::new_jsx_transformer, TransformOptions};
/// let _tx = new_jsx_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/jsxtransforms/jsx.go:NewJSXTransformer
pub fn new_jsx_transformer(opt: &TransformOptions) -> Transformer {
    // Track 2: `compiler_options.jsx` selects the classic vs automatic runtime.
    let automatic = matches!(
        opt.compiler_options.jsx,
        tsgo_core::compileroptions::JsxEmit::ReactJsx
            | tsgo_core::compileroptions::JsxEmit::ReactJsxDev
    );
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            jsx_visit(ec.arena_mut(), node, automatic)
        }),
        opt.context.clone(),
    )
}

/// Lowers JSX in the subtree rooted at `node`. `automatic` selects the automatic
/// (`jsx`/`jsxs`) runtime over the classic (`React.createElement`) one.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:JSXTransformer.visit
fn jsx_visit(arena: &mut NodeArena, node: NodeId, automatic: bool) -> NodeId {
    match arena.kind(node) {
        Kind::JsxSelfClosingElement => {
            if let Some(lowered) = lower_create_element(arena, node, &[], automatic) {
                return lowered;
            }
        }
        Kind::JsxElement => {
            let (opening, children) = match arena.data(node) {
                NodeData::JsxElement(d) => (d.opening, d.children.nodes.clone()),
                _ => unreachable!("kind checked above"),
            };
            if let Some(lowered) = lower_create_element(arena, opening, &children, automatic) {
                return lowered;
            }
        }
        Kind::JsxFragment if !automatic => {
            let children = match arena.data(node) {
                NodeData::JsxFragment(d) => d.children.nodes.clone(),
                _ => unreachable!("kind checked above"),
            };
            return lower_fragment_create_element(arena, &children);
        }
        _ => {}
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| jsx_visit(a, c, automatic))
}

/// Lowers an opening-like element (`<tag ...>`) plus its `children` to a
/// classic-runtime `React.createElement(tag, props, ...children)` call.
///
/// Returns `None` for shapes outside the reachable subset (non-identifier tag
/// names such as `A.B` or namespaced names; attributes; children) so the caller
/// leaves them for the deferred fuller port.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxOpeningLikeElementCreateElement
fn lower_create_element(
    arena: &mut NodeArena,
    opening_like: NodeId,
    children: &[NodeId],
    automatic: bool,
) -> Option<NodeId> {
    let attributes = match arena.data(opening_like) {
        NodeData::JsxSelfClosingElement(d) | NodeData::JsxOpeningElement(d) => d.attributes,
        _ => return None,
    };
    let tag_name = get_tag_name(arena, opening_like)?;
    if automatic {
        // Automatic runtime: `jsx(tag, props)` with children folded into
        // `props.children`. Only the no-children case is handled here; children
        // (and `jsxs`) are deferred.
        if !children.is_empty() {
            return None;
        }
        // Empty attributes become `{}` (not `null`) in the automatic runtime.
        let props = match transform_jsx_attributes_to_object_props(arena, attributes, automatic)? {
            null_props if arena.kind(null_props) == Kind::NullKeyword => {
                arena.new_object_literal_expression(NodeList::new(vec![]))
            }
            object_props => object_props,
        };
        let callee = arena.new_identifier("jsx");
        let args = vec![tag_name, props];
        return Some(arena.new_call_expression(
            callee,
            None,
            None,
            NodeList::new(args),
            NodeFlags::NONE,
        ));
    }
    let props = transform_jsx_attributes_to_object_props(arena, attributes, automatic)?;
    let callee = make_react_create_element(arena);
    let mut args = vec![tag_name, props];
    for &child in children {
        if let Some(expression) = transform_jsx_child_to_expression(arena, child, automatic) {
            args.push(expression);
        }
    }
    Some(arena.new_call_expression(callee, None, None, NodeList::new(args), NodeFlags::NONE))
}

/// Lowers a JSX fragment `<>...</>` to
/// `React.createElement(React.Fragment, null, ...children)`.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxOpeningFragmentCreateElement
fn lower_fragment_create_element(arena: &mut NodeArena, children: &[NodeId]) -> NodeId {
    let tag_name = make_react_fragment(arena);
    let props = arena.new_keyword_expression(Kind::NullKeyword);
    let callee = make_react_create_element(arena);
    let mut args = vec![tag_name, props];
    for &child in children {
        // Fragments are classic-runtime only (the automatic arm is gated off).
        if let Some(expression) = transform_jsx_child_to_expression(arena, child, false) {
            args.push(expression);
        }
    }
    arena.new_call_expression(callee, None, None, NodeList::new(args), NodeFlags::NONE)
}

/// Lowers one JSX child to a `createElement` argument expression, or `None` to
/// drop it (whitespace-only text, an empty `{}` expression).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxChildToExpression
fn transform_jsx_child_to_expression(
    arena: &mut NodeArena,
    child: NodeId,
    automatic: bool,
) -> Option<NodeId> {
    match arena.kind(child) {
        Kind::JsxText => {
            let text = match arena.data(child) {
                NodeData::JsxText(d) => d.text.clone(),
                _ => return None,
            };
            let fixed = fixup_whitespace_and_decode_entities(&text);
            if fixed.is_empty() {
                None
            } else {
                Some(arena.new_string_literal(&fixed, TokenFlags::NONE))
            }
        }
        Kind::JsxExpression => {
            let (dot_dot_dot, expression) = match arena.data(child) {
                NodeData::JsxExpression(d) => (d.dot_dot_dot_token, d.expression),
                _ => return None,
            };
            let expression = expression?;
            let expression = jsx_visit(arena, expression, automatic);
            if dot_dot_dot.is_some() {
                Some(arena.new_spread_element(expression))
            } else {
                Some(expression)
            }
        }
        // Nested elements/fragments are lowered recursively.
        _ => Some(jsx_visit(arena, child, automatic)),
    }
}

/// Trims JSX whitespace, collapsing internal line breaks to single spaces and
/// dropping all-whitespace runs (entity decoding is deferred). Returns the empty
/// string for whitespace-only text, which the caller drops.
///
/// Side effects: none.
// Go: internal/transformers/jsxtransforms/jsx.go:fixupWhitespaceAndDecodeEntities
fn fixup_whitespace_and_decode_entities(text: &str) -> String {
    let mut acc = String::new();
    let mut initial = true;
    for line in text.split('\n') {
        let trimmed = line.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\r');
        if trimmed.is_empty() {
            continue;
        }
        if !initial {
            acc.push(' ');
        }
        acc.push_str(trimmed);
        initial = false;
    }
    acc
}

/// Transforms a `JsxAttributes` node to the `createElement` props argument: a
/// `null` keyword when there are no attributes, otherwise an object literal of
/// the attributes. Returns `None` for shapes outside the reachable subset
/// (spread attributes) so the caller defers the whole element.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxAttributesToObjectProps (ES2018+ path)
fn transform_jsx_attributes_to_object_props(
    arena: &mut NodeArena,
    attributes: NodeId,
    automatic: bool,
) -> Option<NodeId> {
    let properties = match arena.data(attributes) {
        NodeData::JsxAttributes(d) => d.list.nodes.clone(),
        _ => return None,
    };
    if properties.is_empty() {
        // No attributes: React wants `null`.
        return Some(arena.new_keyword_expression(Kind::NullKeyword));
    }
    let mut object_props = Vec::with_capacity(properties.len());
    for property in properties {
        // Spread attributes need `Object.assign`/object-spread handling -> deferred.
        if arena.kind(property) != Kind::JsxAttribute {
            return None;
        }
        object_props.push(transform_jsx_attribute_to_object_literal_element(
            arena, property, automatic,
        )?);
    }
    Some(arena.new_object_literal_expression(NodeList::new(object_props)))
}

/// Transforms one `JsxAttribute` into a `PropertyAssignment` object-literal
/// element (`name: value`).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxAttributeToObjectLiteralElement
fn transform_jsx_attribute_to_object_literal_element(
    arena: &mut NodeArena,
    attribute: NodeId,
    automatic: bool,
) -> Option<NodeId> {
    let (name, initializer) = match arena.data(attribute) {
        NodeData::JsxAttribute(d) => (d.name, d.initializer),
        _ => return None,
    };
    // Namespaced attribute names (`a:b`) -> deferred.
    if arena.kind(name) != Kind::Identifier {
        return None;
    }
    let value = transform_jsx_attribute_initializer(arena, initializer, automatic);
    Some(arena.new_property_assignment(None, name, None, None, Some(value)))
}

/// Computes a JSX attribute's value expression: a missing initializer is
/// `true`, a string literal is recreated, and a `{expr}` initializer uses the
/// inner expression.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxAttributeInitializer
fn transform_jsx_attribute_initializer(
    arena: &mut NodeArena,
    initializer: Option<NodeId>,
    automatic: bool,
) -> NodeId {
    let Some(initializer) = initializer else {
        // `<div hidden/>` -> `{ hidden: true }`
        return arena.new_keyword_expression(Kind::TrueKeyword);
    };
    match arena.kind(initializer) {
        Kind::StringLiteral => {
            // Recreate the literal (entity decoding is deferred), preserving text.
            let text = arena.text(initializer).to_string();
            arena.new_string_literal(&text, TokenFlags::NONE)
        }
        Kind::JsxExpression => {
            let expression = match arena.data(initializer) {
                NodeData::JsxExpression(d) => d.expression,
                _ => None,
            };
            match expression {
                Some(expression) => jsx_visit(arena, expression, automatic),
                None => arena.new_keyword_expression(Kind::TrueKeyword),
            }
        }
        // JSX element initializers (`<div x={<y/>}/>`) -> visited recursively.
        _ => jsx_visit(arena, initializer, automatic),
    }
}

/// Computes the `createElement` first argument from a JSX tag name: an
/// intrinsic (lowercase / hyphenated) name becomes a string literal, a
/// component identifier (`Foo`) is used directly. Qualified / namespaced tag
/// names are deferred (`None`).
///
/// Side effects: may push a string-literal node onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:getTagName
fn get_tag_name(arena: &mut NodeArena, opening_like: NodeId) -> Option<NodeId> {
    let tag_name = match arena.data(opening_like) {
        NodeData::JsxSelfClosingElement(d) | NodeData::JsxOpeningElement(d) => d.tag_name,
        _ => return None,
    };
    if arena.kind(tag_name) != Kind::Identifier {
        // Qualified (`A.B`) / namespaced (`a:b`) tag names -> deferred.
        return None;
    }
    let text = arena.text(tag_name).to_string();
    if tsgo_scanner::is_intrinsic_jsx_name(&text) {
        Some(arena.new_string_literal(&text, TokenFlags::NONE))
    } else {
        // Component reference: use the tag identifier directly.
        Some(tag_name)
    }
}

/// Builds the `React.createElement` callee expression.
///
/// Side effects: pushes the identifier/access nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:createJsxFactoryExpression (default React factory)
fn make_react_create_element(arena: &mut NodeArena) -> NodeId {
    make_react_member(arena, "createElement")
}

/// Builds the `React.Fragment` fragment-factory expression.
///
/// Side effects: pushes the identifier/access nodes onto the arena.
// Go: internal/transformers/jsxtransforms/jsx.go:createJsxFragmentFactoryExpression (default React factory)
fn make_react_fragment(arena: &mut NodeArena) -> NodeId {
    make_react_member(arena, "Fragment")
}

/// Builds a `React.<member>` property-access expression.
///
/// Side effects: pushes the identifier/access nodes onto the arena.
fn make_react_member(arena: &mut NodeArena, member: &str) -> NodeId {
    let react = arena.new_identifier("React");
    let member = arena.new_identifier(member);
    arena.new_property_access_expression(react, None, member)
}

#[cfg(test)]
#[path = "jsx_test.rs"]
mod tests;
