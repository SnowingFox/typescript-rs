use super::*;

// Go: internal/ast/subtreefacts.go:SubtreeFacts (iota positions)
#[test]
fn subtree_facts_base_bits() {
    assert_eq!(SubtreeFacts::NONE.bits(), 0);
    assert_eq!(SubtreeFacts::CONTAINS_TYPESCRIPT.bits(), 1 << 0);
    assert_eq!(SubtreeFacts::CONTAINS_JSX.bits(), 1 << 1);
    assert_eq!(
        SubtreeFacts::CONTAINS_EXPONENTIATION_OPERATOR.bits(),
        1 << 13
    );
    assert_eq!(SubtreeFacts::CONTAINS_LEXICAL_THIS.bits(), 1 << 14);
    assert_eq!(
        SubtreeFacts::CONTAINS_INVALID_TEMPLATE_ESCAPE.bits(),
        1 << 24
    );
    // Computed marker must be last.
    assert_eq!(SubtreeFacts::COMPUTED.bits(), 1 << 25);
}

// Go: internal/ast/subtreefacts.go (exclusion masks — values from Go literals)
#[test]
fn subtree_facts_exclusions() {
    assert_eq!(SubtreeFacts::EXCLUSIONS_NODE, SubtreeFacts::COMPUTED);
    assert_eq!(
        SubtreeFacts::EXCLUSIONS_ERASEABLE.bits(),
        !SubtreeFacts::CONTAINS_TYPESCRIPT.bits()
    );
    assert_eq!(
        SubtreeFacts::EXCLUSIONS_ARROW_FUNCTION,
        SubtreeFacts::EXCLUSIONS_NODE
            | SubtreeFacts::CONTAINS_AWAIT
            | SubtreeFacts::CONTAINS_OBJECT_REST_OR_SPREAD
    );
    assert_eq!(
        SubtreeFacts::CONTAINS_LEXICAL_THIS_OR_SUPER,
        SubtreeFacts::CONTAINS_LEXICAL_THIS | SubtreeFacts::CONTAINS_LEXICAL_SUPER
    );
}
