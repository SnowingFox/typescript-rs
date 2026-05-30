//! `get_literal_text`: produces the emitted text of a literal-like node, using
//! the original source text when possible and a canonical/escaped form otherwise.

use crate::printer::node_is_synthesized;
use crate::utilities::{escape_string_worker, GetLiteralTextFlags, QuoteChar};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, TokenFlags};

/// Returns the token flags of a literal-like node (empty for non-literals).
fn literal_token_flags(arena: &NodeArena, node: NodeId) -> TokenFlags {
    match arena.data(node) {
        NodeData::StringLiteral(d)
        | NodeData::NumericLiteral(d)
        | NodeData::BigIntLiteral(d)
        | NodeData::RegularExpressionLiteral(d)
        | NodeData::NoSubstitutionTemplateLiteral(d)
        | NodeData::TemplateHead(d)
        | NodeData::TemplateMiddle(d)
        | NodeData::TemplateTail(d) => d.token_flags,
        _ => TokenFlags::empty(),
    }
}

/// Reports whether a literal node is unterminated.
// Go: internal/ast/utilities.go:IsUnterminatedLiteral
fn is_unterminated_literal(arena: &NodeArena, node: NodeId) -> bool {
    literal_token_flags(arena, node).contains(TokenFlags::UNTERMINATED)
}

/// Reports whether the literal's original source text may be used verbatim.
// Go: internal/printer/utilities.go:canUseOriginalText
fn can_use_original_text(arena: &NodeArena, node: NodeId, flags: GetLiteralTextFlags) -> bool {
    if node_is_synthesized(arena, node)
        || arena.parent(node).is_none()
        || (flags.contains(GetLiteralTextFlags::TERMINATE_UNTERMINATED_LITERALS)
            && is_unterminated_literal(arena, node))
    {
        return false;
    }

    if arena.kind(node) == Kind::NumericLiteral {
        let token_flags = literal_token_flags(arena, node);
        if token_flags.contains(TokenFlags::IS_INVALID) {
            return false;
        }
        if token_flags.contains(TokenFlags::CONTAINS_SEPARATOR) {
            return flags.contains(GetLiteralTextFlags::ALLOW_NUMERIC_SEPARATOR);
        }
    }

    arena.kind(node) != Kind::BigIntLiteral
}

/// Returns the emitted text of a literal-like node.
///
/// When the node's original source text is reachable and usable, it is returned
/// verbatim; otherwise a canonical (numeric/bigint) or escaped/quoted form is
/// produced. The raw-text branch for synthetic template literals approximates Go
/// (which threads a `RawText` field the Rust AST does not yet carry); parsed
/// template literals always take the original-text path.
// Go: internal/printer/utilities.go:getLiteralText
pub(crate) fn get_literal_text(
    arena: &NodeArena,
    text: &str,
    node: NodeId,
    flags: GetLiteralTextFlags,
) -> String {
    if !text.is_empty() && can_use_original_text(arena, node, flags) {
        let loc = arena.loc(node);
        let pos = tsgo_scanner::skip_trivia(text, loc.pos());
        return text[pos as usize..loc.end() as usize].to_string();
    }

    let kind = arena.kind(node);
    match kind {
        Kind::StringLiteral => {
            let quote = if literal_token_flags(arena, node).contains(TokenFlags::SINGLE_QUOTE) {
                QuoteChar::SingleQuote
            } else {
                QuoteChar::DoubleQuote
            };
            let value = arena.text(node);
            let mut b = String::with_capacity(value.len() + 2);
            b.push(quote.ch());
            escape_string_worker(value, quote, flags, &mut b);
            b.push(quote.ch());
            b
        }
        Kind::NoSubstitutionTemplateLiteral
        | Kind::TemplateHead
        | Kind::TemplateMiddle
        | Kind::TemplateTail => {
            let value = arena.text(node);
            let mut b = String::new();
            match kind {
                Kind::NoSubstitutionTemplateLiteral | Kind::TemplateHead => b.push('`'),
                _ => b.push('}'),
            }
            escape_string_worker(value, QuoteChar::Backtick, flags, &mut b);
            match kind {
                Kind::NoSubstitutionTemplateLiteral | Kind::TemplateTail => b.push('`'),
                _ => b.push_str("${"),
            }
            b
        }
        Kind::NumericLiteral | Kind::BigIntLiteral => arena.text(node).to_string(),
        Kind::RegularExpressionLiteral => arena.text(node).to_string(),
        other => panic!("Unsupported LiteralLikeNode: {other:?}"),
    }
}

#[cfg(test)]
#[path = "literal_text_test.rs"]
mod tests;
