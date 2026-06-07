//! Name-resolution post-check hooks (`onSuccessfullyResolvedSymbol` family).

use tsgo_ast::{Kind, NodeData, NodeFlags, NodeId, SymbolFlags, SymbolId};

use super::declared_types::{get_declaration_of_alias_symbol, resolve_alias};
use super::program::BoundProgram;
use super::symbols_query::name_of_declaration;
use super::Checker;

impl Checker {
    /// Post-resolution validation: block-scoped TDZ + type-only alias value use.
    // Go: internal/checker/checker.go:Checker.onSuccessfullyResolvedSymbol
    pub(super) fn on_successfully_resolved_symbol(
        &mut self,
        program: &dyn BoundProgram,
        error_location: NodeId,
        result: SymbolId,
        meaning: SymbolFlags,
    ) {
        if meaning.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE)
            || meaning.intersects(SymbolFlags::CLASS | SymbolFlags::ENUM)
                && meaning.intersects(SymbolFlags::VALUE)
        {
            let export_or_local = get_export_symbol_of_value_symbol_if_exported(program, result);
            let flags = program.symbol(export_or_local).flags;
            if flags.intersects(
                SymbolFlags::BLOCK_SCOPED_VARIABLE | SymbolFlags::CLASS | SymbolFlags::ENUM,
            ) {
                self.check_resolved_block_scoped_variable(program, export_or_local, error_location);
            }
        }
        if meaning.intersects(SymbolFlags::VALUE) {
            let s = program.symbol(result);
            if s.flags.contains(SymbolFlags::ALIAS)
                && !s.flags.intersects(SymbolFlags::VALUE)
                && !is_valid_type_only_alias_use_site(program, error_location)
            {
                if let Some(type_only_decl) =
                    get_type_only_alias_declaration_ex(self, program, result, SymbolFlags::VALUE)
                {
                    let name = program.arena().text(error_location).to_string();
                    let is_export = matches!(
                        program.arena().kind(type_only_decl),
                        Kind::ExportSpecifier | Kind::ExportDeclaration | Kind::NamespaceExport
                    );
                    let message = if is_export {
                        &tsgo_diagnostics::X_0_CANNOT_BE_USED_AS_A_VALUE_BECAUSE_IT_WAS_EXPORTED_USING_EXPORT_TYPE
                    } else {
                        &tsgo_diagnostics::X_0_CANNOT_BE_USED_AS_A_VALUE_BECAUSE_IT_WAS_IMPORTED_USING_IMPORT_TYPE
                    };
                    self.error(program, error_location, message, &[&name]);
                }
            }
        }
    }

    // Go: internal/checker/checker.go:Checker.checkResolvedBlockScopedVariable
    fn check_resolved_block_scoped_variable(
        &mut self,
        program: &dyn BoundProgram,
        result: SymbolId,
        error_location: NodeId,
    ) {
        let sym = program.symbol(result);
        if sym.flags.intersects(
            SymbolFlags::FUNCTION | SymbolFlags::FUNCTION_SCOPED_VARIABLE | SymbolFlags::ASSIGNMENT,
        ) && sym.flags.intersects(SymbolFlags::CLASS)
        {
            return;
        }
        let arena = program.arena();
        let Some(declaration) = sym.declarations.iter().copied().find(|&d| {
            is_block_or_catch_scoped(arena, d)
                || is_class_like(arena, d)
                || arena.kind(d) == Kind::EnumDeclaration
        }) else {
            return;
        };
        if arena.flags(declaration).contains(NodeFlags::AMBIENT) {
            return;
        }
        if is_block_scoped_name_declared_before_use(program, declaration, error_location) {
            return;
        }
        let declaration_name = declaration_name_to_string(arena, declaration);
        let message = if sym.flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
            &tsgo_diagnostics::BLOCK_SCOPED_VARIABLE_0_USED_BEFORE_ITS_DECLARATION
        } else if sym.flags.intersects(SymbolFlags::CLASS) {
            &tsgo_diagnostics::CLASS_0_USED_BEFORE_ITS_DECLARATION
        } else if sym.flags.intersects(SymbolFlags::REGULAR_ENUM)
            || sym.flags.intersects(SymbolFlags::CONST_ENUM)
                && program.compiler_options().get_isolated_modules()
        {
            &tsgo_diagnostics::ENUM_0_USED_BEFORE_ITS_DECLARATION
        } else {
            return;
        };
        let mut diagnostic =
            self.diagnostic_for_node(program, error_location, message, &[&declaration_name]);
        let related = self.diagnostic_for_node(
            program,
            declaration,
            &tsgo_diagnostics::X_0_IS_DECLARED_HERE,
            &[&declaration_name],
        );
        diagnostic.add_related_info(related);
        self.add_diagnostic(program, diagnostic);
    }

    // Go: internal/checker/checker.go:Checker.getSuggestedSymbolForNonexistentModule
    pub(super) fn get_suggested_symbol_for_nonexistent_module(
        &self,
        program: &dyn BoundProgram,
        name: &str,
        target_module: SymbolId,
    ) -> Option<SymbolId> {
        let candidates: Vec<SymbolId> = program
            .symbol(target_module)
            .exports
            .values()
            .copied()
            .collect();
        get_spelling_suggestion_for_name(program, name, &candidates, SymbolFlags::MODULE_MEMBER)
    }
}

fn is_block_scoped_name_declared_before_use(
    program: &dyn BoundProgram,
    declaration: NodeId,
    usage: NodeId,
) -> bool {
    let arena = program.arena();
    if source_file_of(program, declaration) != source_file_of(program, usage) {
        return true;
    }
    if arena.flags(usage).intersects(NodeFlags::JSDOC) || is_part_of_type_query(arena, usage) {
        return true;
    }
    if super::check::is_part_of_type_node(program, usage)
        || arena.flags(usage).contains(NodeFlags::AMBIENT)
    {
        return true;
    }
    arena.loc(declaration).pos() <= arena.loc(usage).pos()
}

pub(crate) fn get_spelling_suggestion_for_name(
    program: &dyn BoundProgram,
    name: &str,
    symbols: &[SymbolId],
    meaning: SymbolFlags,
) -> Option<SymbolId> {
    use tsgo_core::get_spelling_suggestion;
    get_spelling_suggestion(
        name,
        symbols.iter().copied(),
        |&sid| {
            let candidate = program.symbol(sid);
            if candidate.name.is_empty()
                || candidate.name.starts_with('"')
                || candidate.name.starts_with('\u{fe}')
            {
                return String::new();
            }
            if candidate.flags.intersects(meaning) {
                return candidate.name.clone();
            }
            String::new()
        },
        |&a, &b| match program.symbol(a).name.cmp(&program.symbol(b).name) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        },
    )
}

fn get_type_only_alias_declaration_ex(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    mut symbol: SymbolId,
    meaning: SymbolFlags,
) -> Option<NodeId> {
    let arena = program.arena();
    loop {
        let flags = program.symbol(symbol).flags;
        if !flags.contains(SymbolFlags::ALIAS) || flags.intersects(meaning) {
            break;
        }
        if let Some(decl) = get_declaration_of_alias_symbol(program, symbol) {
            let owner = program.view_for_symbol(symbol);
            let decl_arena = owner.as_deref().map(|v| v.arena()).unwrap_or(arena);
            if is_type_only_import_or_export_declaration(decl_arena, decl) {
                return Some(decl);
            }
        }
        symbol = resolve_alias(checker, program, symbol)?;
    }
    None
}

pub(crate) fn is_valid_type_only_alias_use_site(
    program: &dyn BoundProgram,
    use_site: NodeId,
) -> bool {
    let arena = program.arena();
    if arena
        .flags(use_site)
        .intersects(NodeFlags::AMBIENT | NodeFlags::JSDOC)
    {
        return true;
    }
    if is_part_of_type_query(arena, use_site) {
        return true;
    }
    if is_identifier_in_non_emitting_heritage_clause(arena, use_site) {
        return true;
    }
    if is_part_of_possibly_valid_type_or_abstract_computed_property_name(program, use_site) {
        return true;
    }
    !super::check::is_expression_node(program, use_site)
        && !is_shorthand_property_name_use_site(arena, use_site)
}

/// Reports whether `node` was synthesized (negative source positions).
// Go: internal/ast/utilities.go:NodeIsSynthesized
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn node_is_synthesized(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    let loc = arena.loc(node);
    loc.pos() < 0 || loc.end() < 0
}

/// Marks `node` and descendants as synthesized (negative positions).
// Go: internal/checker/jsx.go:markAsSynthetic
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn mark_as_synthetic_name(arena: &mut tsgo_ast::NodeArena, node: NodeId) {
    arena.set_loc(node, tsgo_core::text::TextRange::new(-1, -1));
    let mut children = Vec::new();
    arena.for_each_child(node, &mut |c| {
        children.push(c);
        false
    });
    for child in children {
        mark_as_synthetic_name(arena, child);
    }
}

fn get_export_symbol_of_value_symbol_if_exported(
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> SymbolId {
    let s = program.symbol(symbol);
    if s.flags.contains(SymbolFlags::EXPORT_VALUE) && !s.flags.contains(SymbolFlags::ALIAS) {
        if let Some(export_symbol) = s.export_symbol {
            return export_symbol;
        }
    }
    symbol
}

fn is_type_only_import_or_export_declaration(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    is_type_only_import_declaration(arena, node) || is_type_only_export_declaration(arena, node)
}

fn import_clause_is_type_only(arena: &tsgo_ast::NodeArena, node: NodeId) -> Option<bool> {
    match arena.data(node) {
        NodeData::ImportClause(d) => Some(d.phase_modifier == Kind::TypeKeyword),
        _ => None,
    }
}

fn is_type_only_import_declaration(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ImportSpecifier(d) => {
            d.is_type_only
                || arena
                    .parent(node)
                    .and_then(|p| arena.parent(p))
                    .and_then(|gp| import_clause_is_type_only(arena, gp))
                    .unwrap_or(false)
        }
        NodeData::NamespaceImport(_) => arena
            .parent(node)
            .and_then(|p| import_clause_is_type_only(arena, p))
            .unwrap_or(false),
        NodeData::ImportClause(d) => d.phase_modifier == Kind::TypeKeyword,
        NodeData::ImportEqualsDeclaration(d) => d.is_type_only,
        _ => false,
    }
}

fn is_type_only_export_declaration(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ExportSpecifier(d) => {
            d.is_type_only
                || arena
                    .parent(node)
                    .and_then(|p| arena.parent(p))
                    .and_then(|gp| match arena.data(gp) {
                        NodeData::ExportDeclaration(ed) => Some(ed.is_type_only),
                        _ => None,
                    })
                    .unwrap_or(false)
        }
        NodeData::ExportDeclaration(d) => {
            d.is_type_only && d.module_specifier.is_some() && d.export_clause.is_none()
        }
        NodeData::NamespaceExport(_) => arena
            .parent(node)
            .and_then(|p| match arena.data(p) {
                NodeData::ExportDeclaration(ed) => Some(ed.is_type_only),
                _ => None,
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn is_part_of_type_query(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        match arena.kind(n) {
            Kind::QualifiedName | Kind::Identifier => cur = arena.parent(n),
            Kind::TypeQuery => return true,
            _ => return false,
        }
    }
    false
}

fn is_identifier_in_non_emitting_heritage_clause(
    arena: &tsgo_ast::NodeArena,
    node: NodeId,
) -> bool {
    if arena.kind(node) != Kind::Identifier {
        return false;
    }
    let mut parent = arena.parent(node);
    while let Some(p) = parent {
        match arena.kind(p) {
            Kind::PropertyAccessExpression | Kind::ExpressionWithTypeArguments => {
                parent = arena.parent(p);
            }
            Kind::HeritageClause => {
                let token = match arena.data(p) {
                    NodeData::HeritageClause(d) => d.token,
                    _ => return false,
                };
                if token == Kind::ImplementsKeyword {
                    return true;
                }
                return arena
                    .parent(p)
                    .is_some_and(|gp| arena.kind(gp) == Kind::InterfaceDeclaration);
            }
            _ => return false,
        }
    }
    false
}

fn is_part_of_possibly_valid_type_or_abstract_computed_property_name(
    program: &dyn BoundProgram,
    node: NodeId,
) -> bool {
    let arena = program.arena();
    let mut cur = Some(node);
    while let Some(n) = cur {
        match arena.kind(n) {
            Kind::Identifier | Kind::PropertyAccessExpression => cur = arena.parent(n),
            Kind::ComputedPropertyName => {
                if let Some(parent) = arena.parent(n) {
                    if super::check::has_syntactic_modifier(
                        program,
                        parent,
                        tsgo_ast::ModifierFlags::ABSTRACT,
                    ) {
                        return true;
                    }
                    if let Some(gp) = arena.parent(parent) {
                        return matches!(
                            arena.kind(gp),
                            Kind::InterfaceDeclaration | Kind::TypeLiteral
                        );
                    }
                }
                return false;
            }
            _ => return false,
        }
    }
    false
}

fn is_shorthand_property_name_use_site(arena: &tsgo_ast::NodeArena, use_site: NodeId) -> bool {
    if arena.kind(use_site) != Kind::Identifier {
        return false;
    }
    let Some(parent) = arena.parent(use_site) else {
        return false;
    };
    match arena.data(parent) {
        NodeData::ShorthandPropertyAssignment(d) => d.name == use_site,
        _ => false,
    }
}

fn is_block_or_catch_scoped(arena: &tsgo_ast::NodeArena, id: NodeId) -> bool {
    combined_node_flags(arena, id).intersects(NodeFlags::BLOCK_SCOPED)
        || is_catch_clause_variable_declaration(arena, id)
}

fn combined_node_flags(arena: &tsgo_ast::NodeArena, id: NodeId) -> NodeFlags {
    let mut node = id;
    let mut flags = arena.flags(node);
    if arena.kind(node) == Kind::VariableDeclaration {
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableDeclarationList {
        flags |= arena.flags(node);
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableStatement {
        flags |= arena.flags(node);
    }
    flags
}

fn is_catch_clause_variable_declaration(arena: &tsgo_ast::NodeArena, id: NodeId) -> bool {
    let mut cur = id;
    while arena.kind(cur) == Kind::BindingElement {
        if let Some(parent) = arena.parent(cur) {
            if matches!(
                arena.kind(parent),
                Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
            ) {
                if let Some(grandparent) = arena.parent(parent) {
                    cur = grandparent;
                    continue;
                }
            }
        }
        break;
    }
    arena.kind(cur) == Kind::VariableDeclaration
        && arena
            .parent(cur)
            .is_some_and(|p| arena.kind(p) == Kind::CatchClause)
}

fn is_class_like(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::ClassDeclaration | Kind::ClassExpression
    )
}

fn declaration_name_to_string(arena: &tsgo_ast::NodeArena, declaration: NodeId) -> String {
    name_of_declaration(arena, declaration)
        .map(|n| arena.text(n).to_string())
        .unwrap_or_default()
}

fn source_file_of(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let arena = program.arena();
    let mut cur = Some(node);
    while let Some(n) = cur {
        if arena.kind(n) == Kind::SourceFile {
            return Some(n);
        }
        cur = arena.parent(n);
    }
    None
}

#[cfg(test)]
#[path = "name_resolution_test.rs"]
mod tests;
