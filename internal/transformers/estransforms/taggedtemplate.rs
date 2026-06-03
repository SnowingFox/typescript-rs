//! Port of Go `internal/transformers/estransforms/taggedtemplate.go`: lowers
//! tagged template literals with invalid escape sequences for pre-ES2018
//! targets.
//!
//! DEFER(P5): the full implementation needs the `__makeTemplateObject` helper
//! and source-file-level cached template declarations. The structure is ported
//! but the transform body is a no-op pass-through.
//! blocked-by: `NodeFactory::NewTemplateObjectHelper`, source-file rebuild,
//! `EmitContext::ReadEmitHelpers`/`AddEmitHelper` + helper definitions.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::NodeId;
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers tagged templates with invalid escapes.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::taggedtemplate::new_tagged_template_transformer, TransformOptions};
/// let _tx = new_tagged_template_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/taggedtemplate.go:newTaggedTemplateLiftRestrictionTransformer
pub fn new_tagged_template_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|_ec: &mut EmitContext, node: NodeId| {
            // DEFER(P5): pass-through until helper infrastructure is ported.
            node
        }),
        opt.context.clone(),
    )
}

/// Escapes `*/` sequences inside a multi-line comment so the text is safe to
/// embed in a `/* ... */` block. Go's `safeMultiLineComment`.
///
/// # Examples
/// ```
/// use tsgo_transformers::estransforms::taggedtemplate::safe_multi_line_comment;
/// assert_eq!(safe_multi_line_comment("a*/b"), " a*_/b ");
/// assert_eq!(safe_multi_line_comment("hello"), " hello ");
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/estransforms/taggedtemplate.go:safeMultiLineComment (local fn in Go)
pub fn safe_multi_line_comment(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 2);
    result.push(' ');
    let mut rest = text;
    while let Some(i) = rest.find("*/") {
        result.push_str(&rest[..i]);
        result.push_str("*_/");
        rest = &rest[i + 2..];
    }
    result.push_str(rest);
    result.push(' ');
    result
}

/// Reports whether a template node (or any template span within a template
/// expression) contains an invalid escape sequence.
///
/// Side effects: none (pure).
// Go: internal/transformers/estransforms/taggedtemplate.go:hasInvalidEscape
pub fn has_invalid_escape(arena: &tsgo_ast::NodeArena, template: NodeId) -> bool {
    use tsgo_ast::{Kind, NodeData, TokenFlags};

    if arena.kind(template) == Kind::NoSubstitutionTemplateLiteral {
        if let NodeData::NoSubstitutionTemplateLiteral(d) = arena.data(template) {
            return d.token_flags.contains(TokenFlags::CONTAINS_INVALID_ESCAPE);
        }
    }

    if let NodeData::TemplateExpression(d) = arena.data(template) {
        if let NodeData::TemplateHead(head) = arena.data(d.head) {
            if head
                .token_flags
                .contains(TokenFlags::CONTAINS_INVALID_ESCAPE)
            {
                return true;
            }
        }
        for &span in &d.template_spans.nodes {
            if let NodeData::TemplateSpan(ts) = arena.data(span) {
                let literal_data = arena.data(ts.literal);
                let has_invalid = match literal_data {
                    NodeData::TemplateMiddle(m) => {
                        m.token_flags.contains(TokenFlags::CONTAINS_INVALID_ESCAPE)
                    }
                    NodeData::TemplateTail(t) => {
                        t.token_flags.contains(TokenFlags::CONTAINS_INVALID_ESCAPE)
                    }
                    _ => false,
                };
                if has_invalid {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
#[path = "taggedtemplate_test.rs"]
mod tests;
