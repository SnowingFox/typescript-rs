//! Port of Go `internal/transformers/utilities.go`: shared transform helpers.
//!
//! Round 6a ports the reachable, dependency-light subset: the emit-flag /
//! generated-name predicates that only need [`EmitContext`] side tables, and the
//! pure compound-assignment operator mapping. The remaining helpers are deferred
//! because they need un-ported `ast`/`printer` surface:
//!
//! - `convert_binding_*` / `convert_variable_declaration_to_assignment_expression`
//!   / `single_or_many` build replacement nodes via factory constructors
//!   (`new_omitted_expression`, `new_spread_element`, `new_assignment_expression`,
//!   `new_property_assignment`, `new_object_literal_expression`, `new_syntax_list`,
//!   …) plus `EmitContext::assign_comment_and_source_map_ranges`.
//!   DEFER(P5/6b) blocked-by: those `tsgo_printer::NodeFactory` constructors and
//!   the comment/source-map side tables are not yet ported.
//! - `is_simple_copiable_expression` / `is_simple_inlineable_expression`.
//!   DEFER(P5/6b) blocked-by: `tsgo_ast::is_string_literal_like` /
//!   `tsgo_ast::is_numeric_literal` predicates are not yet ported (and
//!   `internal/ast/utilities.rs` is out of this round's edit scope).
//! - `is_identifier_reference`. DEFER(P5/6b) blocked-by: the `Expression()` /
//!   `Initializer()` / `Arguments()` node accessors and per-kind matching it
//!   needs are not yet ported.
//! - `find_super_statement_index_path` / `get_super_call_from_statement`.
//!   DEFER(P5/6b) blocked-by: `tsgo_ast::skip_parentheses` / `is_super_call` /
//!   `is_expression_statement` / `is_try_statement` are not yet ported.
//! - `move_range_past_modifiers` / `move_range_past_decorators`.
//!   DEFER(P5/6b) blocked-by: `tsgo_ast::can_have_modifiers` / `modifier_nodes`
//!   / `position_is_synthesized` and the `is_property_declaration` /
//!   `is_method_declaration` predicates are not yet ported.
//! - `is_original_node_single_line`. DEFER(P5/6b) blocked-by:
//!   `tsgo_ast::get_source_file_of_node` and
//!   `tsgo_scanner::get_ecma_line_of_position` are not yet ported.

use tsgo_ast::{Kind, NodeId};
use tsgo_printer::{EmitContext, EmitFlags};

/// Reports whether identifier `name` is an auto-generated name (i.e. the emit
/// context recorded auto-generate info for it).
///
/// # Examples
/// ```
/// use tsgo_transformers::utilities::is_generated_identifier;
/// use tsgo_printer::EmitContext;
/// let mut ec = EmitContext::new();
/// let temp = ec.factory().new_temp_variable();
/// assert!(is_generated_identifier(&ec, temp));
/// let plain = ec.arena_mut().new_identifier("x");
/// assert!(!is_generated_identifier(&ec, plain));
/// ```
///
/// Side effects: none (reads the auto-generate side table).
// Go: internal/transformers/utilities.go:IsGeneratedIdentifier
pub fn is_generated_identifier(emit_context: &EmitContext, name: NodeId) -> bool {
    emit_context.has_auto_generate_info(name)
}

/// Reports whether identifier `name` carries the helper-name emit flag (it
/// refers to an unscoped emit helper).
///
/// # Examples
/// ```
/// use tsgo_transformers::utilities::is_helper_name;
/// use tsgo_printer::{EmitContext, EmitFlags};
/// let mut ec = EmitContext::new();
/// let name = ec.arena_mut().new_identifier("__assign");
/// ec.set_emit_flags(name, EmitFlags::HELPER_NAME);
/// assert!(is_helper_name(&ec, name));
/// ```
///
/// Side effects: none (reads the emit-flags side table).
// Go: internal/transformers/utilities.go:IsHelperName
pub fn is_helper_name(emit_context: &EmitContext, name: NodeId) -> bool {
    emit_context
        .emit_flags(name)
        .contains(EmitFlags::HELPER_NAME)
}

/// Reports whether identifier `name` carries the local-name emit flag (an export
/// prefix must not be added even though it points at an exported declaration).
///
/// # Examples
/// ```
/// use tsgo_transformers::utilities::is_local_name;
/// use tsgo_printer::{EmitContext, EmitFlags};
/// let mut ec = EmitContext::new();
/// let name = ec.arena_mut().new_identifier("C");
/// ec.set_emit_flags(name, EmitFlags::LOCAL_NAME);
/// assert!(is_local_name(&ec, name));
/// ```
///
/// Side effects: none (reads the emit-flags side table).
// Go: internal/transformers/utilities.go:IsLocalName
pub fn is_local_name(emit_context: &EmitContext, name: NodeId) -> bool {
    emit_context
        .emit_flags(name)
        .contains(EmitFlags::LOCAL_NAME)
}

/// Reports whether identifier `name` carries the export-name emit flag (an
/// export prefix must be added for the local name of an exported declaration).
///
/// # Examples
/// ```
/// use tsgo_transformers::utilities::is_export_name;
/// use tsgo_printer::{EmitContext, EmitFlags};
/// let mut ec = EmitContext::new();
/// let name = ec.arena_mut().new_identifier("C");
/// ec.set_emit_flags(name, EmitFlags::EXPORT_NAME);
/// assert!(is_export_name(&ec, name));
/// ```
///
/// Side effects: none (reads the emit-flags side table).
// Go: internal/transformers/utilities.go:IsExportName
pub fn is_export_name(emit_context: &EmitContext, name: NodeId) -> bool {
    emit_context
        .emit_flags(name)
        .contains(EmitFlags::EXPORT_NAME)
}

/// Maps a compound-assignment operator token to its non-assignment counterpart
/// (`+=` → `+`, `**=` → `**`, `&&=` → `&&`, …); any other token is returned
/// unchanged.
///
/// # Examples
/// ```
/// use tsgo_transformers::utilities::get_non_assignment_operator_for_compound_assignment;
/// use tsgo_ast::Kind;
/// assert_eq!(
///     get_non_assignment_operator_for_compound_assignment(Kind::PlusEqualsToken),
///     Kind::PlusToken
/// );
/// assert_eq!(
///     get_non_assignment_operator_for_compound_assignment(Kind::PlusToken),
///     Kind::PlusToken
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/utilities.go:GetNonAssignmentOperatorForCompoundAssignment
pub fn get_non_assignment_operator_for_compound_assignment(kind: Kind) -> Kind {
    match kind {
        Kind::PlusEqualsToken => Kind::PlusToken,
        Kind::MinusEqualsToken => Kind::MinusToken,
        Kind::AsteriskEqualsToken => Kind::AsteriskToken,
        Kind::AsteriskAsteriskEqualsToken => Kind::AsteriskAsteriskToken,
        Kind::SlashEqualsToken => Kind::SlashToken,
        Kind::PercentEqualsToken => Kind::PercentToken,
        Kind::LessThanLessThanEqualsToken => Kind::LessThanLessThanToken,
        Kind::GreaterThanGreaterThanEqualsToken => Kind::GreaterThanGreaterThanToken,
        Kind::GreaterThanGreaterThanGreaterThanEqualsToken => {
            Kind::GreaterThanGreaterThanGreaterThanToken
        }
        Kind::AmpersandEqualsToken => Kind::AmpersandToken,
        Kind::BarEqualsToken => Kind::BarToken,
        Kind::CaretEqualsToken => Kind::CaretToken,
        Kind::BarBarEqualsToken => Kind::BarBarToken,
        Kind::AmpersandAmpersandEqualsToken => Kind::AmpersandAmpersandToken,
        Kind::QuestionQuestionEqualsToken => Kind::QuestionQuestionToken,
        _ => kind,
    }
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
