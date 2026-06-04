//! Module-reference collection for source files.
//!
//! 1:1 port of Go `internal/parser/references.go`.
//!
//! After parsing, this pass walks top-level statements to collect:
//! - `imports`: module-specifier string-literal ids of imports/re-exports.
//! - `module_augmentations`: names of ambient modules that augment external modules.
//! - `ambient_module_names`: names declared (but not augmenting) in the global scope.
//!
//! The functions operate purely on a read-only [`NodeArena`] (no parser state
//! needed).

use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeId};
use tsgo_tspath;

/// The result of collecting external module references from a source file.
#[derive(Debug, Default)]
pub struct ModuleReferences {
    /// Module-specifier string-literal node ids.
    pub imports: Vec<NodeId>,
    /// Module-augmentation name nodes.
    pub module_augmentations: Vec<NodeId>,
    /// Top-level ambient module name strings.
    pub ambient_module_names: Vec<String>,
}

/// Reports whether `id` is an `import` declaration, `import = ...`, or
/// `export { ... } from ...` / `export * from ...`.
// Go: internal/ast/utilities.go:IsAnyImportOrReExport
pub fn is_any_import_or_reexport(arena: &NodeArena, id: NodeId) -> bool {
    matches!(
        arena.kind(id),
        Kind::ImportDeclaration | Kind::ImportEqualsDeclaration | Kind::ExportDeclaration
    )
}

/// Returns the module-specifier expression of an import/re-export node, or
/// `None` if the node has no specifier.
// Go: internal/ast/utilities.go:GetExternalModuleName
pub fn get_external_module_name(arena: &NodeArena, id: NodeId) -> Option<NodeId> {
    match arena.data(id) {
        NodeData::ImportDeclaration(d) => Some(d.module_specifier),
        NodeData::ImportEqualsDeclaration(d) => {
            if arena.kind(d.module_reference) == Kind::ExternalModuleReference {
                if let NodeData::ExternalModuleReference(emr) = arena.data(d.module_reference) {
                    Some(emr.expression)
                } else {
                    None
                }
            } else {
                None
            }
        }
        NodeData::ExportDeclaration(d) => d.module_specifier,
        _ => None,
    }
}

/// Reports whether `id` is a `ModuleDeclaration` node.
// Go: internal/ast/ast_generated.go:IsModuleDeclaration
pub fn is_module_declaration(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::ModuleDeclaration
}

/// Reports whether `id` is an ambient module (module declaration with a
/// string-literal name).
// Go: internal/ast/utilities.go:IsAmbientModule
pub fn is_ambient_module(arena: &NodeArena, id: NodeId) -> bool {
    if !is_module_declaration(arena, id) {
        return false;
    }
    if let NodeData::ModuleDeclaration(d) = arena.data(id) {
        arena.kind(d.name) == Kind::StringLiteral
    } else {
        false
    }
}

/// Reports whether the source file is an external module (has an
/// `external_module_indicator`).
// Go: internal/ast/utilities.go:IsExternalModule
pub fn is_external_module(arena: &NodeArena, source_file: NodeId) -> bool {
    if let NodeData::SourceFile(d) = arena.data(source_file) {
        d.external_module_indicator.is_some()
    } else {
        false
    }
}

/// Reports whether `id` has the `Ambient` (`declare`) syntactic modifier.
// Go: internal/ast/utilities.go:HasSyntacticModifier
pub fn has_ambient_modifier(arena: &NodeArena, id: NodeId) -> bool {
    get_syntactic_modifier_flags(arena, id).contains(ModifierFlags::AMBIENT)
}

/// Computes the syntactic modifier flags from a node's modifier list.
// Go: internal/ast/utilities.go:GetSyntacticModifierFlags
fn get_syntactic_modifier_flags(arena: &NodeArena, id: NodeId) -> ModifierFlags {
    let modifiers = match arena.data(id) {
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportEqualsDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers.map_or(ModifierFlags::NONE, |m| m.modifier_flags)
}

/// Returns the text of a string-literal or identifier node.
fn node_text(arena: &NodeArena, id: NodeId) -> &str {
    arena.text(id)
}

/// Collects external module references from a source file's top-level
/// statements.
///
/// This is the primary entry point, mirroring Go's
/// `collectExternalModuleReferences`. It iterates the source file's statements
/// and calls [`collect_module_references`] for each.
///
/// # DEFER
///
/// The `ForEachDynamicImportOrRequireCall` pass (for JS files) is not ported
/// yet; it requires AST traversal utilities (`forEachChild`) that operate at
/// a more granular level. The static import/export collection is complete.
// Go: internal/parser/references.go:collectExternalModuleReferences
pub fn collect_external_module_references(
    arena: &NodeArena,
    source_file: NodeId,
) -> ModuleReferences {
    let statements = match arena.data(source_file) {
        NodeData::SourceFile(d) => &d.statements,
        _ => return ModuleReferences::default(),
    };
    let is_decl_file = match arena.data(source_file) {
        NodeData::SourceFile(d) => d.is_declaration_file,
        _ => false,
    };

    let mut refs = ModuleReferences::default();
    for &stmt in &statements.nodes {
        collect_module_references(arena, source_file, stmt, false, is_decl_file, &mut refs);
    }
    refs
}

/// Collects import/re-export module specifiers and ambient module declarations
/// from a single statement, recursing into ambient module bodies.
// Go: internal/parser/references.go:collectModuleReferences
pub fn collect_module_references(
    arena: &NodeArena,
    source_file: NodeId,
    node: NodeId,
    in_ambient_module: bool,
    is_declaration_file: bool,
    refs: &mut ModuleReferences,
) {
    if is_any_import_or_reexport(arena, node) {
        if let Some(module_name_expr) = get_external_module_name(arena, node) {
            if arena.kind(module_name_expr) == Kind::StringLiteral {
                let module_name = node_text(arena, module_name_expr);
                if !module_name.is_empty()
                    && (!in_ambient_module
                        || !tsgo_tspath::is_external_module_name_relative(module_name))
                {
                    refs.imports.push(module_name_expr);
                }
            }
        }
        return;
    }

    if is_module_declaration(arena, node) && is_ambient_module(arena, node) {
        let (name_id, body, has_ambient_kw) = match arena.data(node) {
            NodeData::ModuleDeclaration(d) => (d.name, d.body, has_ambient_modifier(arena, node)),
            _ => return,
        };
        if !(in_ambient_module || has_ambient_kw || is_declaration_file) {
            return;
        }
        let name_text = node_text(arena, name_id).to_string();

        if is_external_module(arena, source_file)
            || (in_ambient_module && !tsgo_tspath::is_external_module_name_relative(&name_text))
        {
            refs.module_augmentations.push(name_id);
        } else if !in_ambient_module {
            refs.ambient_module_names.push(name_text);
            if let Some(body_id) = body {
                if let NodeData::ModuleBlock(block) = arena.data(body_id) {
                    for &statement in &block.statements.nodes {
                        collect_module_references(
                            arena,
                            source_file,
                            statement,
                            true,
                            is_declaration_file,
                            refs,
                        );
                    }
                }
            }
        }
    }
}
