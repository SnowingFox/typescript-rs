//! Port of Go `internal/transformers/moduletransforms/utilities.go`: shared
//! helpers for the module-format transforms (CommonJS, ESM, System).
//!
//! # Scope
//!
//! This round lands the dependency-light / structurally complete helpers:
//!
//! * [`is_declaration_name_of_enum_or_namespace`] — checks whether an identifier
//!   is the declared name of an enum or namespace declaration.
//! * [`create_empty_imports`] — builds `export {}` (empty named exports).
//! * [`is_file_level_reserved_generated_identifier`] — checks the auto-generate
//!   flags for a generated identifier.
//! * [`is_simple_inlineable_expression`] — pure inlineability check.
//!
//! # Deferred (DEFER(P5))
//!
//! * [`rewrite_module_specifier`] — needs `ShouldRewriteModuleSpecifier` (not
//!   yet ported to Rust in `tsgo_core`).
//! * [`get_external_module_name_literal`] / [`try_get_module_name_from_file`] /
//!   [`try_get_module_name_from_declaration`] / [`try_rename_external_module`] /
//!   [`get_external_module_name_from_path`] — Go stubs (`// !!!`) returning nil;
//!   we provide structural placeholders.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, NodeList};
use tsgo_printer::EmitContext;

/// Reports whether `name` is the declared name of an enum or namespace
/// declaration (after tracing back through `MostOriginal` to the parse tree).
///
/// Side effects: none (reads the emit context's original-node mapping and
/// the arena's parent pointers).
// Go: internal/transformers/moduletransforms/utilities.go:isDeclarationNameOfEnumOrNamespace
pub fn is_declaration_name_of_enum_or_namespace(ec: &EmitContext, name: NodeId) -> bool {
    let original = ec.most_original(name);
    let arena = ec.arena();
    if let Some(parent) = arena.parent(original) {
        match arena.kind(parent) {
            Kind::EnumDeclaration | Kind::ModuleDeclaration => {
                if let Some(parent_name) = name_of(arena, parent) {
                    return parent_name == original;
                }
            }
            _ => {}
        }
    }
    false
}

/// Returns the `Name` child of a declaration node, if it has one.
fn name_of(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::EnumDeclaration(d) => Some(d.name),
        NodeData::ModuleDeclaration(d) => Some(d.name),
        _ => None,
    }
}

/// Builds an `export {}` (empty named exports) statement. Used to ensure a file
/// is treated as a module even when there are no imports or exports.
///
/// Side effects: appends synthesized nodes to the arena.
// Go: internal/transformers/moduletransforms/utilities.go:createEmptyImports
pub fn create_empty_imports(ec: &mut EmitContext) -> NodeId {
    let named_exports = ec.arena_mut().new_named_exports(NodeList::new(Vec::new()));
    ec.arena_mut()
        .new_export_declaration(None, false, Some(named_exports), None, None)
}

/// Reports whether `name` is a file-level, optimistic, reserved-in-nested-scopes
/// generated identifier (the `exports`/`require` reserved names CommonJS and
/// SystemJS emit use).
///
/// Side effects: none (reads the auto-generate side table).
// Go: internal/transformers/moduletransforms/utilities.go:isFileLevelReservedGeneratedIdentifier
pub fn is_file_level_reserved_generated_identifier(ec: &EmitContext, name: NodeId) -> bool {
    if let Some(info) = ec.get_auto_generate_info(name) {
        info.flags.is_file_level()
            && info.flags.is_optimistic()
            && info.flags.is_reserved_in_nested_scopes()
    } else {
        false
    }
}

/// A simple inlineable expression is an expression which can be copied into
/// multiple locations without risk of repeating any side effects and whose value
/// could not possibly change between any such locations. In practice: not an
/// identifier, and also a "simple copiable" (literal, keyword, void 0, etc.).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/moduletransforms/utilities.go:isSimpleInlineableExpression
pub fn is_simple_inlineable_expression(arena: &NodeArena, expression: NodeId) -> bool {
    if arena.kind(expression) == Kind::Identifier {
        return false;
    }
    is_simple_copiable_expression(arena, expression)
}

/// Reports whether `expression` is a simple copiable expression (a literal or
/// keyword whose value cannot change and has no side effects).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/utilities.go:IsSimpleCopiableExpression
fn is_simple_copiable_expression(arena: &NodeArena, expression: NodeId) -> bool {
    matches!(
        arena.kind(expression),
        Kind::StringLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
            | Kind::UndefinedKeyword
    ) || is_void_zero(arena, expression)
}

/// Reports whether `node` is `void 0`.
fn is_void_zero(arena: &NodeArena, node: NodeId) -> bool {
    if arena.kind(node) != Kind::VoidExpression {
        return false;
    }
    let expr = match arena.data(node) {
        NodeData::VoidExpression(d) => d.expression,
        _ => return false,
    };
    arena.kind(expr) == Kind::NumericLiteral && arena.text(expr) == "0"
}

/// Placeholder: Go stubs this out (`// !!!` returning `nil`). Resolves a local
/// path to a path which is absolute to the base of the emit.
///
/// DEFER(P5) blocked-by: the `ResolveModuleNameResolutionHost` interface is not
/// yet ported.
// Go: internal/transformers/moduletransforms/utilities.go:getExternalModuleNameFromPath
#[allow(dead_code)]
pub fn get_external_module_name_from_path(
    _host: &(), // placeholder
    _file_name: &str,
    _reference_path: &str,
) -> String {
    String::new()
}

/// Placeholder: Go stubs this out (`// !!!` returning `nil`). Some bundlers
/// (SystemJS builder) sometimes want to rename dependencies.
///
/// DEFER(P5) blocked-by: the `SourceFile.renamedDependencies` field is not
/// yet ported.
// Go: internal/transformers/moduletransforms/utilities.go:tryRenameExternalModule
#[allow(dead_code)]
pub fn try_rename_external_module(
    _arena: &mut NodeArena,
    _module_name: NodeId,
    _source_file: NodeId,
) -> Option<NodeId> {
    None
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
