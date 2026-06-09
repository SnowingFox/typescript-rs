//! Module, import/export, and enum-member checking (Go `checker.go` module surface).
//!
//! Ports the reachable subset of `checkEnumDeclaration`, `checkEnumMember`,
//! `computeEnumMemberValues`, `checkModuleDeclaration`, `checkModuleAugmentationElement`,
//! `checkImportDeclaration` / `checkImportClause`, `checkExportDeclaration`, and
//! `getExportsOfModule`.

use rustc_hash::FxHashMap;
use tsgo_ast::utilities::node_is_missing;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, SymbolFlags, SymbolId, SymbolTable};
use tsgo_diagnostics::Message;
use tsgo_evaluator::EvalValue;
use tsgo_tspath;

use super::check::{is_enum_const, is_in_js_file, is_numeric_literal_name};
use super::declared_types::{
    compute_enum_member_values as compute_enum_member_values_impl,
    evaluate_enum_member_initializer, resolve_alias, resolve_external_module_name,
    resolve_external_module_symbol,
};
use super::program::BoundProgram;
use super::symbols::resolve_name;
use super::Checker;

impl Checker {
    /// Cached exports of a module symbol (`getExportsOfModule`).
    ///
    /// The reachable subset clones the binder `exports` table (export-star
    /// merging is DEFER).
    ///
    /// Side effects: may populate [`ModuleSymbolLinks::resolved_exports`].
    // Go: internal/checker/checker.go:Checker.getExportsOfModule
    pub(crate) fn get_exports_of_module(
        &mut self,
        program: &dyn BoundProgram,
        module_symbol: SymbolId,
    ) -> &SymbolTable {
        if !self.module_exports_cached.contains(&module_symbol) {
            let exports = program.symbol(module_symbol).exports.clone();
            self.module_symbol_links.get(module_symbol).resolved_exports = exports;
            self.module_exports_cached.insert(module_symbol);
        }
        &self.module_symbol_links.get(module_symbol).resolved_exports
    }

    /// Computes enum member values and reports enum-member diagnostics.
    ///
    /// Side effects: may record diagnostics; marks the enum as computed.
    // Go: internal/checker/checker.go:Checker.computeEnumMemberValues
    pub(crate) fn compute_enum_member_values(
        &mut self,
        program: &dyn BoundProgram,
        enum_declaration: NodeId,
    ) {
        if self.enum_values_computed.contains(&enum_declaration) {
            return;
        }
        self.enum_values_computed.insert(enum_declaration);

        let members = match program.arena().data(enum_declaration) {
            NodeData::EnumDeclaration(d) => d.members.nodes.clone(),
            _ => return,
        };

        let in_ambient = program
            .arena()
            .flags(enum_declaration)
            .contains(NodeFlags::AMBIENT);
        let is_const = is_enum_const(program, enum_declaration);

        let mut previous: Option<NodeId> = None;
        let mut has_string_valued_member = false;
        let mut by_name: FxHashMap<String, EvalValue> = FxHashMap::default();
        let computed = compute_enum_member_values_impl(program, enum_declaration);

        for member in &members {
            let (name_node, initializer) = match program.arena().data(*member) {
                NodeData::EnumMember(d) => (d.name, d.initializer),
                _ => continue,
            };

            if is_computed_non_literal_name(program, name_node) {
                self.error(
                    program,
                    name_node,
                    &tsgo_diagnostics::COMPUTED_PROPERTY_NAMES_ARE_NOT_ALLOWED_IN_ENUMS,
                    &[],
                );
            } else if program.arena().kind(name_node) == Kind::BigIntLiteral {
                self.error(
                    program,
                    name_node,
                    &tsgo_diagnostics::AN_ENUM_MEMBER_CANNOT_HAVE_A_NUMERIC_NAME,
                    &[],
                );
            } else {
                let text = match program.arena().kind(name_node) {
                    Kind::Identifier | Kind::StringLiteral | Kind::NumericLiteral => {
                        program.arena().text(name_node).to_string()
                    }
                    _ => String::new(),
                };
                if !text.is_empty()
                    && is_numeric_literal_name(&text)
                    && text != "Infinity"
                    && text != "NaN"
                {
                    self.error(
                        program,
                        name_node,
                        &tsgo_diagnostics::AN_ENUM_MEMBER_CANNOT_HAVE_A_NUMERIC_NAME,
                        &[],
                    );
                }
            }

            let member_value = computed
                .iter()
                .find(|(m, _)| *m == *member)
                .map(|(_, v)| v.clone());
            let eval_result = initializer.map(|_| {
                evaluate_enum_member_initializer(program, *member, &by_name)
            });

            if let Some(init) = initializer {
                if has_string_valued_member
                    && !is_string_or_numeric_literal_like(program, init)
                    && !eval_result.as_ref().is_some_and(|r| {
                        matches!(r.value, EvalValue::Str(_))
                    })
                {
                    self.error(
                        program,
                        init,
                        &tsgo_diagnostics::COMPUTED_VALUES_ARE_NOT_PERMITTED_IN_AN_ENUM_WITH_STRING_VALUED_MEMBERS,
                        &[],
                    );
                } else if let Some(result) = eval_result {
                    if !matches!(result.value, EvalValue::None) {
                        if is_const {
                            if let EvalValue::Num(n) = result.value {
                                if n.is_nan() {
                                    self.error(
                                        program,
                                        init,
                                        &tsgo_diagnostics::X_CONST_ENUM_MEMBER_INITIALIZER_WAS_EVALUATED_TO_DISALLOWED_VALUE_NAN,
                                        &[],
                                    );
                                } else if n.is_inf() {
                                    self.error(
                                        program,
                                        init,
                                        &tsgo_diagnostics::X_CONST_ENUM_MEMBER_INITIALIZER_WAS_EVALUATED_TO_A_NON_FINITE_VALUE,
                                        &[],
                                    );
                                }
                            }
                        }
                        if program.compiler_options().get_isolated_modules()
                            && matches!(result.value, EvalValue::Str(_))
                            && !result.is_syntactically_string
                        {
                            let qualified = enum_member_qualified_name(
                                program,
                                enum_declaration,
                                name_node,
                            );
                            self.error(
                                program,
                                init,
                                &tsgo_diagnostics::X_0_HAS_A_STRING_TYPE_BUT_MUST_HAVE_SYNTACTICALLY_RECOGNIZABLE_STRING_SYNTAX_WHEN_ISOLATEDMODULES_IS_ENABLED,
                                &[qualified.as_str()],
                            );
                        }
                    } else if matches!(member_value, Some(EvalValue::None)) {
                        if is_const {
                            self.error(
                                program,
                                init,
                                &tsgo_diagnostics::X_CONST_ENUM_MEMBER_INITIALIZERS_MUST_BE_CONSTANT_EXPRESSIONS,
                                &[],
                            );
                        } else if in_ambient {
                            self.error(
                                program,
                                init,
                                &tsgo_diagnostics::IN_AMBIENT_ENUM_DECLARATIONS_MEMBER_INITIALIZER_MUST_BE_CONSTANT_EXPRESSION,
                                &[],
                            );
                        } else {
                            let expr_type = self.check_expression(program, init);
                            if !self.is_type_assignable_to(program, expr_type, self.number_type) {
                                let source_for_msg =
                                    self.get_base_type_of_literal_type(expr_type);
                                let source_str = if source_for_msg == self.boolean_type {
                                    "boolean".to_string()
                                } else {
                                    super::nodebuilder::type_to_string(
                                        self, program, source_for_msg,
                                    )
                                };
                                let target_str = super::nodebuilder::type_to_string(
                                    self,
                                    program,
                                    self.number_type,
                                );
                                self.error(
                                    program,
                                    init,
                                    &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1_AS_REQUIRED_FOR_COMPUTED_ENUM_MEMBER_VALUES,
                                    &[source_str.as_str(), target_str.as_str()],
                                );
                            }
                        }
                    }
                }
            }

            if initializer.is_none() {
                if in_ambient && !is_const {
                    // ambient non-const: computed member, no auto value.
                } else if matches!(member_value, Some(tsgo_evaluator::EvalValue::None)) {
                    self.error(
                        program,
                        name_node,
                        &tsgo_diagnostics::ENUM_MEMBER_MUST_HAVE_INITIALIZER,
                        &[],
                    );
                } else if program.compiler_options().get_isolated_modules() {
                    if let Some(prev) = previous {
                        if let NodeData::EnumMember(pd) = program.arena().data(prev) {
                            if let Some(prev_init) = pd.initializer {
                                if !is_isolated_modules_literal_numeric_enum_initializer(
                                    program, prev_init,
                                ) {
                                    self.error(
                                        program,
                                        name_node,
                                        &tsgo_diagnostics::ENUM_MEMBER_FOLLOWING_A_NON_LITERAL_NUMERIC_MEMBER_MUST_HAVE_AN_INITIALIZER_WHEN_ISOLATEDMODULES_IS_ENABLED,
                                        &[],
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if matches!(member_value, Some(EvalValue::Str(_))) {
                has_string_valued_member = true;
            }

            if let Some(v) = member_value {
                let text = match program.arena().kind(name_node) {
                    Kind::Identifier | Kind::StringLiteral | Kind::NumericLiteral => {
                        program.arena().text(name_node).to_string()
                    }
                    _ => String::new(),
                };
                if !text.is_empty() {
                    by_name.insert(text, v);
                }
            }

            previous = Some(*member);
        }
    }

    /// Validates an `import { ... } from "m"` declaration.
    // Go: internal/checker/checker.go:Checker.checkImportDeclaration
    pub(crate) fn check_import_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        if self.check_grammar_module_element_context(
            program,
            node,
            if is_in_js_file(program.arena(), node) {
                &tsgo_diagnostics::AN_IMPORT_DECLARATION_CAN_ONLY_BE_USED_AT_THE_TOP_LEVEL_OF_A_MODULE
            } else {
                &tsgo_diagnostics::AN_IMPORT_DECLARATION_CAN_ONLY_BE_USED_AT_THE_TOP_LEVEL_OF_A_NAMESPACE_OR_MODULE
            },
        ) {
            return;
        }
        let NodeData::ImportDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let (import_clause, _module_specifier) = (d.import_clause, d.module_specifier);
        if !self.check_grammar_modifiers(program, node) && d.modifiers.is_some() {
            self.grammar_error_on_first_token(
                program,
                node,
                &tsgo_diagnostics::AN_IMPORT_DECLARATION_CANNOT_HAVE_MODIFIERS,
                &[],
            );
        }
        if self.check_external_import_or_export_declaration(program, node) {
            if let Some(clause) = import_clause {
                if !self.check_grammar_import_clause(program, clause) {
                    self.check_import_clause(program, node, clause);
                }
            }
        }
    }

    /// Grammar-checks an import clause; returns `true` when an error was reported.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarImportClause
    pub(crate) fn check_grammar_import_clause(
        &mut self,
        program: &dyn BoundProgram,
        import_clause: NodeId,
    ) -> bool {
        let NodeData::ImportClause(d) = program.arena().data(import_clause) else {
            return false;
        };
        if d.phase_modifier == Kind::TypeKeyword
            && !program
                .arena()
                .flags(import_clause)
                .contains(NodeFlags::JSDOC)
            && d.name.is_some()
            && d.named_bindings.is_some()
        {
            return self.grammar_error_on_node(
                program,
                import_clause,
                &tsgo_diagnostics::A_TYPE_ONLY_IMPORT_CAN_SPECIFY_A_DEFAULT_IMPORT_OR_NAMED_BINDINGS_BUT_NOT_BOTH,
                &[],
            );
        }
        false
    }

    /// Resolves import bindings (default import `TS1192`, named `TS2305` via alias resolve).
    // Go: internal/checker/checker.go:Checker.checkImportDeclaration (clause body)
    fn check_import_clause(
        &mut self,
        program: &dyn BoundProgram,
        import_decl: NodeId,
        import_clause: NodeId,
    ) {
        let Some(specifier) = module_specifier_text(program, import_decl) else {
            return;
        };
        let Some(module_symbol) = resolve_external_module_name(program, &specifier) else {
            return;
        };
        let target = resolve_external_module_symbol(self, program, module_symbol);
        let exports = self.get_exports_of_module(program, target).clone();

        let NodeData::ImportClause(d) = program.arena().data(import_clause) else {
            return;
        };
        if let Some(default_binding) = d.name {
            if !exports.contains_key("default") {
                self.error(
                    program,
                    default_binding,
                    &tsgo_diagnostics::MODULE_0_HAS_NO_DEFAULT_EXPORT,
                    &[&format!("\"{specifier}\"")],
                );
            }
            if let Some(sym) = program.symbol_of_node(default_binding) {
                let _ = resolve_alias(self, program, sym);
            }
        }
        if let Some(named_bindings) = d.named_bindings {
            if let NodeData::NamedImports(ni) = program.arena().data(named_bindings) {
                for &spec in &ni.elements.nodes {
                    if let Some(sym) = program.symbol_of_node(spec) {
                        let _ = resolve_alias(self, program, sym);
                    }
                }
            }
        }
    }

    /// Validates an `export { ... }` / `export * from "m"` declaration.
    // Go: internal/checker/checker.go:Checker.checkExportDeclaration
    pub(crate) fn check_export_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        if self.check_grammar_module_element_context(
            program,
            node,
            if is_in_js_file(program.arena(), node) {
                &tsgo_diagnostics::AN_EXPORT_DECLARATION_CAN_ONLY_BE_USED_AT_THE_TOP_LEVEL_OF_A_MODULE
            } else {
                &tsgo_diagnostics::AN_EXPORT_DECLARATION_CAN_ONLY_BE_USED_AT_THE_TOP_LEVEL_OF_A_NAMESPACE_OR_MODULE
            },
        ) {
            return;
        }
        let NodeData::ExportDeclaration(d) = program.arena().data(node) else {
            return;
        };
        if !self.check_grammar_modifiers(program, node) && d.modifiers.is_some() {
            self.grammar_error_on_first_token(
                program,
                node,
                &tsgo_diagnostics::AN_EXPORT_DECLARATION_CANNOT_HAVE_MODIFIERS,
                &[],
            );
        }
        if d.module_specifier.is_none()
            || self.check_external_import_or_export_declaration(program, node)
        {
            if let Some(export_clause) = d.export_clause {
                if program.arena().kind(export_clause) == Kind::NamedExports {
                    let specifiers = match program.arena().data(export_clause) {
                        NodeData::NamedExports(ne) => ne.elements.nodes.clone(),
                        _ => Vec::new(),
                    };
                    let has_module_specifier = d.module_specifier.is_some();
                    for specifier in specifiers {
                        self.check_export_specifier_extended(
                            program,
                            specifier,
                            has_module_specifier,
                        );
                    }
                    let in_ambient_external = is_ambient_external_module_context(program, node);
                    let in_ambient_namespace = !in_ambient_external
                        && program
                            .arena()
                            .parent(node)
                            .is_some_and(|p| program.arena().kind(p) == Kind::ModuleBlock)
                        && d.module_specifier.is_none()
                        && program.arena().flags(node).contains(NodeFlags::AMBIENT);
                    if !is_source_file_parent(program, node)
                        && !in_ambient_external
                        && !in_ambient_namespace
                    {
                        self.error(
                            program,
                            node,
                            &tsgo_diagnostics::EXPORT_DECLARATIONS_ARE_NOT_PERMITTED_IN_A_NAMESPACE,
                            &[],
                        );
                    }
                }
            }
        }
    }

    /// Validates import/export module specifiers and placement.
    ///
    /// Returns `false` when checking should not continue.
    // Go: internal/checker/checker.go:Checker.checkExternalImportOrExportDeclaration
    pub(crate) fn check_external_import_or_export_declaration(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let Some(specifier_node) = external_module_name_node(program, node) else {
            return false;
        };
        if node_is_missing(program.arena(), specifier_node) {
            return false;
        }
        if program.arena().kind(specifier_node) != Kind::StringLiteral {
            self.error(
                program,
                specifier_node,
                &tsgo_diagnostics::STRING_LITERAL_EXPECTED,
                &[],
            );
            return false;
        }
        let in_ambient_external = is_ambient_external_module_context(program, node);
        if !is_source_file_parent(program, node) && !in_ambient_external {
            self.error(
                program,
                specifier_node,
                if program.arena().kind(node) == Kind::ExportDeclaration {
                    &tsgo_diagnostics::EXPORT_DECLARATIONS_ARE_NOT_PERMITTED_IN_A_NAMESPACE
                } else {
                    &tsgo_diagnostics::IMPORT_DECLARATIONS_IN_A_NAMESPACE_CANNOT_REFERENCE_A_MODULE
                },
                &[],
            );
            return false;
        }
        if in_ambient_external {
            let text = program.arena().text(specifier_node);
            if tsgo_tspath::is_external_module_name_relative(text) {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::IMPORT_OR_EXPORT_DECLARATION_IN_AN_AMBIENT_MODULE_DECLARATION_CANNOT_REFERENCE_MODULE_THROUGH_RELATIVE_MODULE_NAME,
                    &[],
                );
                return false;
            }
        }
        let specifier = program.arena().text(specifier_node).to_string();
        if resolve_external_module_name(program, &specifier).is_none() {
            self.error(
                program,
                specifier_node,
                &tsgo_diagnostics::FILE_0_IS_NOT_A_MODULE,
                &[&format!("\"{specifier}\"")],
            );
            return false;
        }
        true
    }

    /// Returns `true` when the node is in an illegal module-element context.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarModuleElementContext
    pub(crate) fn check_grammar_module_element_context(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        error_message: &'static Message,
    ) -> bool {
        let Some(parent) = program.arena().parent(node) else {
            return false;
        };
        let ok = matches!(
            program.arena().kind(parent),
            Kind::SourceFile | Kind::ModuleBlock | Kind::ModuleDeclaration
        );
        if !ok {
            self.grammar_error_on_first_token(program, node, error_message, &[]);
        }
        !ok
    }

    /// Validates statements inside a module-augmentation body.
    // Go: internal/checker/checker.go:Checker.checkModuleAugmentationElement
    // Go: internal/checker/checker.go:Checker.checkModuleAugmentationElement
    pub(crate) fn check_module_augmentation_element(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) {
        match program.arena().kind(node) {
            Kind::VariableStatement => {
                if let NodeData::VariableStatement(d) = program.arena().data(node) {
                    let decls = match program.arena().data(d.declaration_list) {
                        NodeData::VariableDeclarationList(vdl) => vdl.declarations.nodes.clone(),
                        _ => Vec::new(),
                    };
                    for decl in decls {
                        self.check_module_augmentation_element(program, decl);
                    }
                }
            }
            Kind::ExportAssignment | Kind::ExportDeclaration => {
                self.grammar_error_on_first_token(
                    program,
                    node,
                    &tsgo_diagnostics::EXPORTS_AND_EXPORT_ASSIGNMENTS_ARE_NOT_PERMITTED_IN_MODULE_AUGMENTATIONS,
                    &[],
                );
            }
            Kind::ImportDeclaration | Kind::JSImportDeclaration => {
                self.grammar_error_on_first_token(
                    program,
                    node,
                    &tsgo_diagnostics::IMPORTS_ARE_NOT_PERMITTED_IN_MODULE_AUGMENTATIONS_CONSIDER_MOVING_THEM_TO_THE_ENCLOSING_EXTERNAL_MODULE,
                    &[],
                );
            }
            Kind::BindingElement | Kind::VariableDeclaration => {
                if let Some(name) = name_of_declaration_node(program, node) {
                    if matches!(
                        program.arena().kind(name),
                        Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
                    ) {
                        let elements = match program.arena().data(name) {
                            NodeData::ObjectBindingPattern(ob) => ob.elements.nodes.clone(),
                            NodeData::ArrayBindingPattern(ab) => ab.elements.nodes.clone(),
                            _ => Vec::new(),
                        };
                        for el in elements {
                            self.check_module_augmentation_element(program, el);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Validates an export specifier, including `TS2661` for non-local exports.
    // Go: internal/checker/checker.go:Checker.checkExportSpecifier
    pub(crate) fn check_export_specifier_extended(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        has_module_specifier: bool,
    ) {
        self.check_export_specifier(program, node, has_module_specifier);
        if has_module_specifier {
            return;
        }
        let (property_name, name) = match program.arena().data(node) {
            NodeData::ExportSpecifier(d) => (d.property_name, d.name),
            _ => return,
        };
        let exported_name = property_name.unwrap_or(name);
        if program.arena().kind(exported_name) == Kind::StringLiteral {
            return;
        }
        let text = program.arena().text(exported_name).to_string();
        let meaning =
            SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::ALIAS;
        let symbol = resolve_name(
            program,
            exported_name,
            &text,
            meaning,
            false,
            program.globals().as_ref().copied(),
        );
        if let Some(sym) = symbol {
            let s = program.symbol(sym);
            if s.name == "undefined" || s.name == "globalThis" {
                self.error(
                    program,
                    exported_name,
                    &tsgo_diagnostics::CANNOT_EXPORT_0_ONLY_LOCAL_DECLARATIONS_CAN_BE_EXPORTED_FROM_A_MODULE,
                    &[&text],
                );
            }
        }
    }
}

fn enum_member_qualified_name(
    program: &dyn BoundProgram,
    enum_declaration: NodeId,
    member_name: NodeId,
) -> String {
    let enum_name = match program.arena().data(enum_declaration) {
        NodeData::EnumDeclaration(d) => program.arena().text(d.name).to_string(),
        _ => "?".to_string(),
    };
    let member = program.arena().text(member_name).to_string();
    format!("{enum_name}.{member}")
}

/// Reports whether `name` is a computed property name that is not a string/number literal.
// Go: internal/ast/utilities.go:IsComputedNonLiteralName
fn is_computed_non_literal_name(program: &dyn BoundProgram, name: NodeId) -> bool {
    if !is_computed_property_name(program.arena(), name) {
        return false;
    }
    let NodeData::ComputedPropertyName(d) = program.arena().data(name) else {
        return false;
    };
    !is_string_or_numeric_literal_like(program, d.expression)
}

fn is_string_or_numeric_literal_like(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().kind(node),
        Kind::StringLiteral | Kind::NumericLiteral | Kind::NoSubstitutionTemplateLiteral
    )
}

/// Reports whether `expr` is a syntactically literal numeric enum initializer
/// that `isolatedModules` allows to be followed by an auto-incremented member
/// (Go `computeEnumMemberValue` / `enumNoInitializerFollowsNonLiteralInitializer`).
fn is_isolated_modules_literal_numeric_enum_initializer(
    program: &dyn BoundProgram,
    expr: NodeId,
) -> bool {
    let mut node = expr;
    loop {
        node = match program.arena().data(node) {
            NodeData::ParenthesizedExpression(d) => d.expression,
            _ => break,
        };
    }
    match program.arena().data(node) {
        NodeData::NumericLiteral(_) => true,
        NodeData::PrefixUnaryExpression(d) => {
            (d.operator == Kind::PlusToken || d.operator == Kind::MinusToken)
                && is_isolated_modules_literal_numeric_enum_initializer(program, d.operand)
        }
        NodeData::BinaryExpression(d) => {
            let op = program.arena().kind(d.operator_token);
            matches!(
                op,
                Kind::PlusToken
                    | Kind::MinusToken
                    | Kind::AsteriskToken
                    | Kind::SlashToken
                    | Kind::PercentToken
                    | Kind::BarToken
                    | Kind::AmpersandToken
                    | Kind::CaretToken
                    | Kind::LessThanLessThanToken
                    | Kind::GreaterThanGreaterThanToken
                    | Kind::GreaterThanGreaterThanGreaterThanToken
            ) && is_isolated_modules_literal_numeric_enum_initializer(program, d.left)
                && is_isolated_modules_literal_numeric_enum_initializer(program, d.right)
        }
        _ => false,
    }
}

fn external_module_name_node(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::ImportDeclaration(d) => Some(d.module_specifier),
        NodeData::ExportDeclaration(d) => d.module_specifier,
        NodeData::ImportEqualsDeclaration(d) => match program.arena().data(d.module_reference) {
            NodeData::ExternalModuleReference(em) => Some(em.expression),
            _ => None,
        },
        _ => None,
    }
}

fn module_specifier_text(program: &dyn BoundProgram, import_decl: NodeId) -> Option<String> {
    match program.arena().data(import_decl) {
        NodeData::ImportDeclaration(d) => {
            Some(program.arena().text(d.module_specifier).to_string())
        }
        NodeData::ExportDeclaration(d) => d
            .module_specifier
            .map(|m| program.arena().text(m).to_string()),
        _ => None,
    }
}

fn is_source_file_parent(program: &dyn BoundProgram, node: NodeId) -> bool {
    program
        .arena()
        .parent(node)
        .is_some_and(|p| program.arena().kind(p) == Kind::SourceFile)
}

fn is_ambient_external_module_context(program: &dyn BoundProgram, node: NodeId) -> bool {
    let Some(parent) = program.arena().parent(node) else {
        return false;
    };
    if program.arena().kind(parent) != Kind::ModuleBlock {
        return false;
    }
    program
        .arena()
        .parent(parent)
        .is_some_and(|md| is_ambient_module(program.arena(), md))
}

fn is_computed_property_name(arena: &NodeArena, id: NodeId) -> bool {
    matches!(arena.data(id), NodeData::ComputedPropertyName(_))
}

fn is_ambient_module(arena: &NodeArena, id: NodeId) -> bool {
    if let NodeData::ModuleDeclaration(d) = arena.data(id) {
        arena.kind(d.name) == Kind::StringLiteral || d.keyword == Kind::GlobalKeyword
    } else {
        false
    }
}

#[allow(dead_code)]
fn name_of_declaration_node(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::BindingElement(d) => d.name,
        _ => None,
    }
}

#[cfg(test)]
#[path = "modules_test.rs"]
mod tests;
