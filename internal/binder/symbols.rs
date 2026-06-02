//! Symbol creation, declaration, merging, and the declaration binders.
//!
//! Ports the symbol-table machinery from Go `internal/binder/binder.go`
//! (`newSymbol`, `declareSymbol(Ex)`, the `declare*Member` family,
//! `addDeclarationToSymbol`, the per-kind `bind*Declaration` routines, and the
//! declaration-name helpers).

use tsgo_ast::symbol::{
    INTERNAL_SYMBOL_NAME_CALL, INTERNAL_SYMBOL_NAME_CLASS, INTERNAL_SYMBOL_NAME_COMPUTED,
    INTERNAL_SYMBOL_NAME_CONSTRUCTOR, INTERNAL_SYMBOL_NAME_DEFAULT,
    INTERNAL_SYMBOL_NAME_EXPORT_EQUALS, INTERNAL_SYMBOL_NAME_EXPORT_STAR,
    INTERNAL_SYMBOL_NAME_FUNCTION, INTERNAL_SYMBOL_NAME_GLOBAL, INTERNAL_SYMBOL_NAME_INDEX,
    INTERNAL_SYMBOL_NAME_MISSING, INTERNAL_SYMBOL_NAME_NEW, INTERNAL_SYMBOL_NAME_TYPE,
};
use tsgo_ast::{Kind, ModifierFlags, NodeData, NodeId, Symbol, SymbolFlags, SymbolId};
use tsgo_diagnostics as diagnostics;
use tsgo_scanner::token_to_string;

use crate::astquery as q;
use crate::{set_value_declaration, Binder, TableLoc};

impl Binder<'_> {
    // Go: internal/binder/binder.go:newSymbol
    fn new_symbol(&mut self, flags: SymbolFlags, name: String) -> SymbolId {
        self.symbol_count += 1;
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            flags,
            name,
            ..Symbol::default()
        });
        id
    }

    // Go: internal/binder/binder.go:declareSymbol
    pub(crate) fn declare_symbol(
        &mut self,
        table: TableLoc,
        parent: Option<SymbolId>,
        node: NodeId,
        includes: SymbolFlags,
        excludes: SymbolFlags,
    ) -> SymbolId {
        self.declare_symbol_ex(table, parent, node, includes, excludes, false, false)
    }

    // Go: internal/binder/binder.go:declareSymbolEx
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn declare_symbol_ex(
        &mut self,
        table: TableLoc,
        parent: Option<SymbolId>,
        node: NodeId,
        includes: SymbolFlags,
        excludes: SymbolFlags,
        is_replaceable_by_method: bool,
        is_computed_name: bool,
    ) -> SymbolId {
        let is_default_export = q::has_syntactic_modifier(self.arena, node, ModifierFlags::DEFAULT)
            || (self.arena.kind(node) == Kind::ExportSpecifier
                && self.export_specifier_name_is_default(node));
        let name = if is_computed_name {
            tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_COMPUTED.to_string()
        } else if is_default_export && parent.is_some() {
            INTERNAL_SYMBOL_NAME_DEFAULT.to_string()
        } else {
            self.get_declaration_name(node)
        };

        let symbol: SymbolId;
        if name == INTERNAL_SYMBOL_NAME_MISSING {
            symbol = self.new_symbol(SymbolFlags::NONE, INTERNAL_SYMBOL_NAME_MISSING.to_string());
        } else {
            let existing = self.table_get(table, &name);
            if includes.intersects(SymbolFlags::CLASSIFIABLE) {
                self.classifiable_names.insert(name.clone());
            }
            match existing {
                None => {
                    let s = self.new_symbol(SymbolFlags::NONE, name.clone());
                    self.table_set(table, name, s);
                    if is_replaceable_by_method {
                        self.symbols[s.index()].flags |= SymbolFlags::REPLACEABLE_BY_METHOD;
                    }
                    symbol = s;
                }
                Some(existing_sym) => {
                    let existing_flags = self.symbols[existing_sym.index()].flags;
                    if is_replaceable_by_method
                        && !existing_flags.contains(SymbolFlags::REPLACEABLE_BY_METHOD)
                    {
                        return existing_sym;
                    } else if existing_flags.intersects(excludes) {
                        if existing_flags.contains(SymbolFlags::REPLACEABLE_BY_METHOD) {
                            let s = self.new_symbol(SymbolFlags::NONE, name.clone());
                            self.table_set(table, name, s);
                            symbol = s;
                        } else if includes.intersects(SymbolFlags::VARIABLE)
                            && existing_flags.intersects(SymbolFlags::ASSIGNMENT)
                            || includes.intersects(SymbolFlags::ASSIGNMENT)
                                && existing_flags.intersects(SymbolFlags::VARIABLE)
                        {
                            // Assignment declarations merge with variables.
                            symbol = existing_sym;
                        } else {
                            symbol = self.report_merge_conflict(
                                existing_sym,
                                node,
                                includes,
                                is_default_export,
                                &name,
                            );
                        }
                    } else {
                        symbol = existing_sym;
                    }
                }
            }
        }
        self.add_declaration_to_symbol(symbol, node, includes);
        match self.symbols[symbol.index()].parent {
            None => self.symbols[symbol.index()].parent = parent,
            Some(p) => {
                if Some(p) != parent {
                    panic!("Existing symbol parent should match new one");
                }
            }
        }
        symbol
    }

    /// Reports a merge conflict and returns a fresh replacement symbol.
    // Go: internal/binder/binder.go:declareSymbolEx (conflict branch)
    fn report_merge_conflict(
        &mut self,
        existing_sym: SymbolId,
        node: NodeId,
        includes: SymbolFlags,
        is_default_export: bool,
        name: &str,
    ) -> SymbolId {
        let existing_flags = self.symbols[existing_sym.index()].flags;
        let mut message = if existing_flags.contains(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
            &diagnostics::CANNOT_REDECLARE_BLOCK_SCOPED_VARIABLE_0
        } else {
            &diagnostics::DUPLICATE_IDENTIFIER_0
        };
        let mut message_needs_name = true;
        if existing_flags.intersects(SymbolFlags::ENUM) || includes.intersects(SymbolFlags::ENUM) {
            message = &diagnostics::ENUM_DECLARATIONS_CAN_ONLY_MERGE_WITH_NAMESPACE_OR_OTHER_ENUM_DECLARATIONS;
            message_needs_name = false;
        }
        let mut multiple_default_exports = false;
        let existing_decls = self.symbols[existing_sym.index()].declarations.clone();
        // A default export (or a second `export default x;` assignment) is the
        // multiple-default-exports case; both Go branches share this body.
        let is_export_default_assignment = q::is_export_assignment(self.arena, node)
            && !matches!(self.arena.data(node), NodeData::ExportAssignment(d) if d.is_export_equals);
        if !existing_decls.is_empty() && (is_default_export || is_export_default_assignment) {
            message = &diagnostics::A_MODULE_CANNOT_HAVE_MULTIPLE_DEFAULT_EXPORTS;
            message_needs_name = false;
            multiple_default_exports = true;
        }
        let declaration_name = q::name_of_declaration(self.arena, node).unwrap_or(node);
        let args = if message_needs_name {
            vec![self.get_display_name(node)]
        } else {
            Vec::new()
        };
        let mut diag = self.create_diagnostic_for_node(declaration_name, message, args);
        for decl in &existing_decls {
            let decl_name = q::name_of_declaration(self.arena, *decl).unwrap_or(*decl);
            let d_args = if message_needs_name {
                vec![self.get_display_name(*decl)]
            } else {
                Vec::new()
            };
            let d = self.create_diagnostic_for_node(decl_name, message, d_args);
            if multiple_default_exports {
                diag.related.push(self.create_diagnostic_for_node(
                    decl_name,
                    &diagnostics::THE_FIRST_EXPORT_DEFAULT_IS_HERE,
                    Vec::new(),
                ));
            }
            self.add_diagnostic(d);
        }
        self.add_diagnostic(diag);
        // When a get/set accessor conflicts with a non-accessor (or an accessor
        // of a DIFFERENT kind), mark the surviving symbol as a FULL accessor so
        // every subsequent declaration is also considered conflicting (e.g. a
        // `get x`, then a non-accessor, then a `set x` all flagged as duplicates).
        // Go tests `symbol.Flags & Accessor != 0` (EITHER bit), so this must be
        // `intersects`, not `contains` (which would require BOTH get+set bits and
        // never fire for a lone getter/setter — the I7/I8/C3/C4/C7/C8/o7/o8 gap).
        // Go: internal/binder/binder.go:declareSymbolEx (lines 286-292)
        if existing_flags.intersects(SymbolFlags::ACCESSOR)
            && existing_flags & SymbolFlags::ACCESSOR != includes & SymbolFlags::ACCESSOR
        {
            self.symbols[existing_sym.index()].flags |= SymbolFlags::ACCESSOR;
        }
        self.new_symbol(SymbolFlags::NONE, name.to_string())
    }

    fn export_specifier_name_is_default(&self, node: NodeId) -> bool {
        if let NodeData::ExportSpecifier(d) = self.arena.data(node) {
            self.arena.kind(d.name) == Kind::Identifier && self.arena.text(d.name) == "default"
        } else {
            false
        }
    }

    // Go: internal/binder/binder.go:addDeclarationToSymbol
    fn add_declaration_to_symbol(
        &mut self,
        symbol: SymbolId,
        node: NodeId,
        symbol_flags: SymbolFlags,
    ) {
        self.symbols[symbol.index()].flags |= symbol_flags;
        self.node_symbol.insert(node, symbol);
        let decls = &mut self.symbols[symbol.index()].declarations;
        if !decls.contains(&node) {
            decls.push(node);
        }
        let flags = self.symbols[symbol.index()].flags;
        if flags.contains(SymbolFlags::CONST_ENUM_ONLY_MODULE)
            && flags
                .intersects(SymbolFlags::FUNCTION | SymbolFlags::CLASS | SymbolFlags::REGULAR_ENUM)
        {
            self.symbols[symbol.index()].flags &= !SymbolFlags::CONST_ENUM_ONLY_MODULE;
        }
        if symbol_flags.intersects(SymbolFlags::VALUE) {
            set_value_declaration(&mut self.symbols, self.arena, symbol, node);
        }
    }

    // Go: internal/binder/binder.go:getDeclarationName
    pub(crate) fn get_declaration_name(&self, node: NodeId) -> String {
        if let NodeData::ExportAssignment(d) = self.arena.data(node) {
            return if d.is_export_equals {
                INTERNAL_SYMBOL_NAME_EXPORT_EQUALS.to_string()
            } else {
                INTERNAL_SYMBOL_NAME_DEFAULT.to_string()
            };
        }
        if let Some(name) = q::name_of_declaration(self.arena, node) {
            if q::is_ambient_module(self.arena, node) {
                if q::is_global_scope_augmentation(self.arena, node) {
                    return INTERNAL_SYMBOL_NAME_GLOBAL.to_string();
                }
                return format!("\"{}\"", self.arena.text(name));
            }
            if q::is_private_identifier(self.arena, name) {
                return match q::get_containing_class(self.arena, node) {
                    None => INTERNAL_SYMBOL_NAME_MISSING.to_string(),
                    Some(class) => match self.symbol_of(class) {
                        None => INTERNAL_SYMBOL_NAME_MISSING.to_string(),
                        Some(class_sym) => crate::get_symbol_name_for_private_identifier(
                            class_sym,
                            self.arena.text(name),
                        ),
                    },
                };
            }
            if q::is_property_name_literal(self.arena, name) {
                return self.arena.text(name).to_string();
            }
            if q::is_computed_property_name(self.arena, name) {
                let name_expr = match self.arena.data(name) {
                    NodeData::ComputedPropertyName(d) => d.expression,
                    _ => unreachable!(),
                };
                if q::is_string_or_numeric_literal_like(self.arena, name_expr) {
                    return self.arena.text(name_expr).to_string();
                }
                if q::is_signed_numeric_literal(self.arena, name_expr) {
                    if let NodeData::PrefixUnaryExpression(u) = self.arena.data(name_expr) {
                        return format!(
                            "{}{}",
                            token_to_string(u.operator),
                            self.arena.text(u.operand)
                        );
                    }
                }
                panic!("Only computed properties with literal names have declaration names");
            }
            return INTERNAL_SYMBOL_NAME_MISSING.to_string();
        }
        match self.arena.kind(node) {
            Kind::Constructor => INTERNAL_SYMBOL_NAME_CONSTRUCTOR.to_string(),
            Kind::FunctionType | Kind::CallSignature => INTERNAL_SYMBOL_NAME_CALL.to_string(),
            Kind::ConstructorType | Kind::ConstructSignature => {
                INTERNAL_SYMBOL_NAME_NEW.to_string()
            }
            Kind::IndexSignature => INTERNAL_SYMBOL_NAME_INDEX.to_string(),
            Kind::ExportDeclaration => INTERNAL_SYMBOL_NAME_EXPORT_STAR.to_string(),
            Kind::SourceFile | Kind::BinaryExpression => {
                INTERNAL_SYMBOL_NAME_EXPORT_EQUALS.to_string()
            }
            _ => INTERNAL_SYMBOL_NAME_MISSING.to_string(),
        }
    }

    // Go: internal/binder/binder.go:getDisplayName
    fn get_display_name(&self, node: NodeId) -> String {
        if let Some(name) = q::declaration_name(self.arena, node) {
            return q::declaration_name_to_string(self.arena, name);
        }
        let name = self.get_declaration_name(node);
        if name != INTERNAL_SYMBOL_NAME_MISSING {
            name
        } else {
            "(Missing)".to_string()
        }
    }

    // Go: internal/binder/binder.go:getOptionalSymbolFlagForNode
    pub(crate) fn get_optional_symbol_flag_for_node(&self, node: NodeId) -> SymbolFlags {
        match q::postfix_token(self.arena, node) {
            Some(t) if self.arena.kind(t) == Kind::QuestionToken => SymbolFlags::OPTIONAL,
            _ => SymbolFlags::NONE,
        }
    }

    // ── declare* family ──────────────────────────────────────────────────────

    // Go: internal/binder/binder.go:declareModuleMember
    fn declare_module_member(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> SymbolId {
        let container = self
            .container
            .expect("declareModuleMember requires a container");
        let has_export_modifier =
            q::combined_modifier_flags(self.arena, node).contains(ModifierFlags::EXPORT);
        if symbol_flags.intersects(SymbolFlags::ALIAS) {
            let kind = self.arena.kind(node);
            if kind == Kind::ExportSpecifier
                || (kind == Kind::ImportEqualsDeclaration && has_export_modifier)
            {
                let sym = self.symbol_of(container).unwrap();
                return self.declare_symbol(
                    TableLoc::Exports(sym),
                    Some(sym),
                    node,
                    symbol_flags,
                    symbol_excludes,
                );
            }
            let locals = self.get_locals(container);
            return self.declare_symbol(locals, None, node, symbol_flags, symbol_excludes);
        }
        let in_export_context = has_export_modifier
            || self
                .arena
                .flags(container)
                .contains(tsgo_ast::NodeFlags::EXPORT_CONTEXT);
        if !q::is_ambient_module(self.arena, node) && in_export_context {
            let unnamed_default =
                q::has_syntactic_modifier(self.arena, node, ModifierFlags::DEFAULT)
                    && self.get_declaration_name(node) == INTERNAL_SYMBOL_NAME_MISSING;
            if !self.is_locals_container(container) || unnamed_default {
                let sym = self.symbol_of(container).unwrap();
                return self.declare_symbol(
                    TableLoc::Exports(sym),
                    Some(sym),
                    node,
                    symbol_flags,
                    symbol_excludes,
                );
            }
            let export_kind = if symbol_flags.intersects(SymbolFlags::VALUE) {
                SymbolFlags::EXPORT_VALUE
            } else {
                SymbolFlags::NONE
            };
            let locals = self.get_locals(container);
            let local = self.declare_symbol(locals, None, node, export_kind, symbol_excludes);
            let sym = self.symbol_of(container).unwrap();
            let export_sym = self.declare_symbol(
                TableLoc::Exports(sym),
                Some(sym),
                node,
                symbol_flags,
                symbol_excludes,
            );
            self.symbols[local.index()].export_symbol = Some(export_sym);
            self.node_local_symbol.insert(node, local);
            return local;
        }
        let locals = self.get_locals(container);
        self.declare_symbol(locals, None, node, symbol_flags, symbol_excludes)
    }

    // Go: internal/ast/utilities.go:IsLocalsContainer (proxy via container flags)
    fn is_locals_container(&self, node: NodeId) -> bool {
        crate::get_container_flags(self.arena, node).contains(crate::ContainerFlags::HAS_LOCALS)
    }

    // Go: internal/binder/binder.go:declareClassMember
    fn declare_class_member(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> SymbolId {
        let container = self.container.unwrap();
        let sym = self.symbol_of(container).unwrap();
        if q::is_static(self.arena, node) {
            self.declare_symbol(
                TableLoc::Exports(sym),
                Some(sym),
                node,
                symbol_flags,
                symbol_excludes,
            )
        } else {
            self.declare_symbol(
                TableLoc::Members(sym),
                Some(sym),
                node,
                symbol_flags,
                symbol_excludes,
            )
        }
    }

    // Go: internal/binder/binder.go:declareSourceFileMember
    fn declare_source_file_member(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> SymbolId {
        if self.is_external_or_commonjs_module() {
            return self.declare_module_member(node, symbol_flags, symbol_excludes);
        }
        let file = self.file;
        let locals = self.get_locals(file);
        self.declare_symbol(locals, None, node, symbol_flags, symbol_excludes)
    }

    // Go: internal/binder/binder.go:declareSymbolAndAddToSymbolTable
    pub(crate) fn declare_symbol_and_add_to_symbol_table(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> SymbolId {
        let container = self
            .container
            .expect("declareSymbolAndAddToSymbolTable requires a container");
        match self.arena.kind(container) {
            Kind::ModuleDeclaration => {
                self.declare_module_member(node, symbol_flags, symbol_excludes)
            }
            Kind::SourceFile => {
                self.declare_source_file_member(node, symbol_flags, symbol_excludes)
            }
            Kind::ClassExpression | Kind::ClassDeclaration => {
                self.declare_class_member(node, symbol_flags, symbol_excludes)
            }
            Kind::EnumDeclaration => {
                let sym = self.symbol_of(container).unwrap();
                self.declare_symbol(
                    TableLoc::Exports(sym),
                    Some(sym),
                    node,
                    symbol_flags,
                    symbol_excludes,
                )
            }
            Kind::TypeLiteral
            | Kind::ObjectLiteralExpression
            | Kind::InterfaceDeclaration
            | Kind::JsxAttributes => {
                let sym = self.symbol_of(container).unwrap();
                self.declare_symbol(
                    TableLoc::Members(sym),
                    Some(sym),
                    node,
                    symbol_flags,
                    symbol_excludes,
                )
            }
            Kind::FunctionType
            | Kind::ConstructorType
            | Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::MethodDeclaration
            | Kind::MethodSignature
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::ClassStaticBlockDeclaration
            | Kind::TypeAliasDeclaration
            | Kind::MappedType => {
                let locals = self.get_locals(container);
                self.declare_symbol(locals, None, node, symbol_flags, symbol_excludes)
            }
            other => panic!("Unhandled case in declareSymbolAndAddToSymbolTable: {other:?}"),
        }
    }

    // ── Anonymous / block-scoped declarations ────────────────────────────────

    // Go: internal/binder/binder.go:bindAnonymousDeclaration
    pub(crate) fn bind_anonymous_declaration(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        name: String,
    ) {
        let symbol = self.new_symbol(symbol_flags, name);
        if symbol_flags.intersects(SymbolFlags::ENUM_MEMBER | SymbolFlags::CLASS_MEMBER) {
            self.symbols[symbol.index()].parent = self.container.and_then(|c| self.symbol_of(c));
        }
        self.add_declaration_to_symbol(symbol, node, symbol_flags);
    }

    // Go: internal/binder/binder.go:declareCommonJSVariable
    pub(crate) fn declare_common_js_variable(&mut self, name: &str) {
        let file = self.file;
        // Ensure the file's locals table exists, then bail if the name is
        // already present (e.g. an explicit top-level `var module` shadow).
        if self.locals.get(&file).is_some_and(|t| t.contains_key(name)) {
            return;
        }
        let symbol = self.new_symbol(
            SymbolFlags::FUNCTION_SCOPED_VARIABLE | SymbolFlags::MODULE_EXPORTS,
            name.to_string(),
        );
        self.symbols[symbol.index()].declarations.push(file);
        self.symbols[symbol.index()].value_declaration = Some(file);
        if name == "module" {
            // `module` carries an `exports` member (`module.exports` access).
            let exports_property = self.new_symbol(
                SymbolFlags::MODULE_EXPORTS | SymbolFlags::PROPERTY,
                "exports".to_string(),
            );
            self.symbols[exports_property.index()]
                .declarations
                .push(file);
            self.symbols[exports_property.index()].value_declaration = Some(file);
            self.symbols[exports_property.index()].parent = Some(symbol);
            self.symbols[symbol.index()]
                .members
                .insert("exports".to_string(), exports_property);
        }
        self.locals
            .entry(file)
            .or_default()
            .insert(name.to_string(), symbol);
    }

    // Go: internal/binder/binder.go:bindBlockScopedDeclaration
    pub(crate) fn bind_block_scoped_declaration(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) {
        let bsc = self
            .block_scope_container
            .expect("bindBlockScopedDeclaration requires a block scope container");
        match self.arena.kind(bsc) {
            Kind::ModuleDeclaration => {
                self.declare_module_member(node, symbol_flags, symbol_excludes);
            }
            Kind::SourceFile if self.is_external_or_commonjs_module() => {
                self.declare_module_member(node, symbol_flags, symbol_excludes);
            }
            _ => {
                let locals = self.get_locals(bsc);
                self.declare_symbol(locals, None, node, symbol_flags, symbol_excludes);
            }
        }
    }

    // ── Per-kind declaration binders ─────────────────────────────────────────

    // Go: internal/binder/binder.go:bindFunctionDeclaration
    pub(crate) fn bind_function_declaration(&mut self, node: NodeId) {
        self.bind_block_scoped_declaration(
            node,
            SymbolFlags::FUNCTION,
            SymbolFlags::FUNCTION_EXCLUDES,
        );
    }

    // Go: internal/binder/binder.go:bindFunctionExpression
    pub(crate) fn bind_function_expression(&mut self, node: NodeId) {
        self.set_node_flow(node);
        let binding_name = if self.arena.kind(node) == Kind::FunctionExpression {
            match q::declaration_name(self.arena, node) {
                Some(name) => self.arena.text(name).to_string(),
                None => INTERNAL_SYMBOL_NAME_FUNCTION.to_string(),
            }
        } else {
            INTERNAL_SYMBOL_NAME_FUNCTION.to_string()
        };
        self.bind_anonymous_declaration(node, SymbolFlags::FUNCTION, binding_name);
    }

    // Go: internal/binder/binder.go:bindClassLikeDeclaration
    pub(crate) fn bind_class_like_declaration(&mut self, node: NodeId) {
        match self.arena.kind(node) {
            Kind::ClassDeclaration => {
                self.bind_block_scoped_declaration(
                    node,
                    SymbolFlags::CLASS,
                    SymbolFlags::CLASS_EXCLUDES,
                );
            }
            Kind::ClassExpression => {
                let name_text = match q::declaration_name(self.arena, node) {
                    Some(name) => {
                        let t = self.arena.text(name).to_string();
                        self.classifiable_names.insert(t.clone());
                        t
                    }
                    None => INTERNAL_SYMBOL_NAME_CLASS.to_string(),
                };
                self.bind_anonymous_declaration(node, SymbolFlags::CLASS, name_text);
            }
            _ => unreachable!(),
        }
        let symbol = self
            .symbol_of(node)
            .expect("class declaration must have a symbol");
        // Every class has an implicit static `prototype` member.
        let prototype = self.new_symbol(
            SymbolFlags::PROPERTY | SymbolFlags::PROTOTYPE,
            "prototype".to_string(),
        );
        let existing = self.symbols[symbol.index()]
            .exports
            .get("prototype")
            .copied();
        if let Some(existing) = existing {
            let decl = self.symbols[existing.index()].declarations.first().copied();
            if let Some(decl) = decl {
                self.error_on_node(
                    decl,
                    &diagnostics::DUPLICATE_IDENTIFIER_0,
                    vec!["prototype".to_string()],
                );
            }
        }
        self.symbols[symbol.index()]
            .exports
            .insert("prototype".to_string(), prototype);
        self.symbols[prototype.index()].parent = Some(symbol);
    }

    // Go: internal/binder/binder.go:bindEnumDeclaration
    pub(crate) fn bind_enum_declaration(&mut self, node: NodeId) {
        if self.arena.flags(node).contains(tsgo_ast::NodeFlags::CONST) {
            self.bind_block_scoped_declaration(
                node,
                SymbolFlags::CONST_ENUM,
                SymbolFlags::CONST_ENUM_EXCLUDES,
            );
        } else {
            self.bind_block_scoped_declaration(
                node,
                SymbolFlags::REGULAR_ENUM,
                SymbolFlags::REGULAR_ENUM_EXCLUDES,
            );
        }
    }

    // Go: internal/binder/binder.go:bindPropertyWorker
    pub(crate) fn bind_property_worker(&mut self, node: NodeId) {
        let is_auto_accessor = q::is_auto_accessor_property_declaration(self.arena, node);
        let includes = if is_auto_accessor {
            SymbolFlags::ACCESSOR
        } else {
            SymbolFlags::PROPERTY
        };
        let excludes = if is_auto_accessor {
            SymbolFlags::ACCESSOR_EXCLUDES
        } else {
            SymbolFlags::PROPERTY_EXCLUDES
        };
        self.bind_property_or_method_or_accessor(
            node,
            includes | self.get_optional_symbol_flag_for_node(node),
            excludes,
        );
    }

    // Go: internal/binder/binder.go:bindPropertyOrMethodOrAccessor
    pub(crate) fn bind_property_or_method_or_accessor(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) {
        if q::is_object_literal_or_class_expression_method_or_accessor(self.arena, node) {
            self.set_node_flow(node);
        }
        // Computed, non-literal names (`[Symbol.iterator]`, `[expr]`) are dynamic:
        // declare them anonymously under `InternalSymbolNameComputed` instead of
        // routing through `get_declaration_name` (which only handles literal
        // computed names and otherwise panics). Resolving the well-known-symbol
        // `__@iterator` form is a checker late-binding concern, not the binder's.
        if q::has_dynamic_name(self.arena, node) {
            self.bind_anonymous_declaration(
                node,
                symbol_flags,
                INTERNAL_SYMBOL_NAME_COMPUTED.to_string(),
            );
        } else {
            self.declare_symbol_and_add_to_symbol_table(node, symbol_flags, symbol_excludes);
        }
    }

    // Go: internal/binder/binder.go:bindFunctionOrConstructorType
    pub(crate) fn bind_function_or_constructor_type(&mut self, node: NodeId) {
        let name = self.get_declaration_name(node);
        let symbol = self.new_symbol(SymbolFlags::SIGNATURE, name);
        self.add_declaration_to_symbol(symbol, node, SymbolFlags::SIGNATURE);
        let type_literal_symbol = self.new_symbol(
            SymbolFlags::TYPE_LITERAL,
            INTERNAL_SYMBOL_NAME_TYPE.to_string(),
        );
        self.add_declaration_to_symbol(type_literal_symbol, node, SymbolFlags::TYPE_LITERAL);
        let sym_name = self.symbols[symbol.index()].name.clone();
        self.symbols[type_literal_symbol.index()]
            .members
            .insert(sym_name, symbol);
    }

    // Go: internal/binder/binder.go:bindVariableDeclarationOrBindingElement
    pub(crate) fn bind_variable_declaration_or_binding_element(&mut self, node: NodeId) {
        if let Some(name) = q::declaration_name(self.arena, node) {
            if !q::is_binding_pattern(self.arena, name) {
                if q::is_block_or_catch_scoped(self.arena, node) {
                    self.bind_block_scoped_declaration(
                        node,
                        SymbolFlags::BLOCK_SCOPED_VARIABLE,
                        SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES,
                    );
                } else if q::is_part_of_parameter_declaration(self.arena, node) {
                    self.declare_symbol_and_add_to_symbol_table(
                        node,
                        SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                        SymbolFlags::PARAMETER_EXCLUDES,
                    );
                } else {
                    self.declare_symbol_and_add_to_symbol_table(
                        node,
                        SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                        SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES,
                    );
                }
            }
        }
    }

    // Go: internal/binder/binder.go:bindParameter
    pub(crate) fn bind_parameter(&mut self, node: NodeId) {
        let name = match self.arena.data(node) {
            NodeData::ParameterDeclaration(d) => d.name,
            _ => return,
        };
        if q::is_binding_pattern(self.arena, name) {
            let index = self.parameter_index(node);
            self.bind_anonymous_declaration(
                node,
                SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                format!("__{index}"),
            );
        } else {
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                SymbolFlags::PARAMETER_EXCLUDES,
            );
        }
        // Parameter-property declaration into the containing class is deferred.
    }

    fn parameter_index(&self, node: NodeId) -> usize {
        let parent = match self.arena.parent(node) {
            Some(p) => p,
            None => return 0,
        };
        let params: Vec<NodeId> = match self.arena.data(parent) {
            NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => {
                d.parameters.nodes.clone()
            }
            NodeData::MethodDeclaration(d) => d.parameters.nodes.clone(),
            NodeData::ConstructorDeclaration(d) => d.parameters.nodes.clone(),
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                d.parameters.nodes.clone()
            }
            NodeData::ArrowFunction(d) => d.parameters.nodes.clone(),
            _ => Vec::new(),
        };
        params.iter().position(|&p| p == node).unwrap_or(0)
    }

    // Go: internal/binder/binder.go:bindTypeParameter
    pub(crate) fn bind_type_parameter(&mut self, node: NodeId) {
        let parent_is_infer = self
            .arena
            .parent(node)
            .is_some_and(|p| self.arena.kind(p) == Kind::InferType);
        if parent_is_infer {
            // InferType type-parameter scoping requires the conditional-type
            // container walk, which is deferred.
            self.bind_anonymous_declaration(
                node,
                SymbolFlags::TYPE_PARAMETER,
                self.get_declaration_name(node),
            );
        } else {
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::TYPE_PARAMETER,
                SymbolFlags::TYPE_PARAMETER_EXCLUDES,
            );
        }
    }
}

#[cfg(test)]
#[path = "symbols_test.rs"]
mod tests;
