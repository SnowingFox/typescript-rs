//! Declaration emit: classes, class/type members, heritage clauses, and the
//! declaration statements. (`impl Printer` block; see `printer.rs`.)

use crate::printer::{ListEmit, Printer, WriteKind};
use crate::utilities::GetLiteralTextFlags;
use tsgo_ast::precedence::OperatorPrecedence;
use tsgo_ast::{Kind, ModifierList, NodeData, NodeId, NodeList};

/// Extracted parts of a method-like declaration (avoids a very large tuple).
struct MethodParts {
    modifiers: Option<ModifierList>,
    asterisk: Option<NodeId>,
    name: NodeId,
    postfix: Option<NodeId>,
    type_params: Option<NodeList>,
    params: NodeList,
    ret: Option<NodeId>,
    body: Option<NodeId>,
}

impl Printer<'_> {
    // Go: internal/printer/printer.go:emitClassExpression / emitClassDeclaration
    pub(crate) fn emit_class_like(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, name, type_parameters, heritage_clauses, members) =
            match self.arena().data(node) {
                NodeData::ClassExpression(d) | NodeData::ClassDeclaration(d) => (
                    d.modifiers.clone(),
                    d.name,
                    d.type_parameters.clone(),
                    d.heritage_clauses.clone(),
                    d.members.clone(),
                ),
                other => panic!("expected class-like, got {other:?}"),
            };
        self.generate_name_if_needed(name);
        let pos = self.emit_modifier_list(node, modifiers.as_ref(), true);
        self.emit_token(Kind::ClassKeyword, pos, WriteKind::Keyword, node);
        if let Some(name) = name {
            self.write_space();
            self.emit_identifier_name(name);
        }
        let indented = self.should_emit_indented(node);
        self.increase_indent_if(indented);
        self.emit_type_parameters(node, type_parameters.as_ref());
        self.emit_list(
            ListEmit::HeritageClause,
            Some(node),
            heritage_clauses.as_ref(),
            crate::list_format::ListFormat::CLASS_HERITAGE_CLAUSES,
        );
        self.write_space();
        self.write_punctuation("{");
        self.push_name_generation_scope(node);
        self.emit_list(
            ListEmit::ClassElement,
            Some(node),
            Some(&members),
            crate::list_format::ListFormat::CLASS_MEMBERS,
        );
        self.pop_name_generation_scope(node);
        self.write_punctuation("}");
        self.decrease_indent_if(indented);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitHeritageClause
    pub(crate) fn emit_heritage_clause(&mut self, node: NodeId) {
        self.enter_node(node);
        let (token, types) = match self.arena().data(node) {
            NodeData::HeritageClause(d) => (d.token, d.types.clone()),
            other => panic!("expected HeritageClause, got {other:?}"),
        };
        self.write_space();
        let pos = self.arena().loc(node).pos();
        self.emit_token(token, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_list(
            ListEmit::HeritageType,
            Some(node),
            Some(&types),
            crate::list_format::ListFormat::HERITAGE_CLAUSE_TYPES,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitClassElement
    pub(crate) fn emit_class_element(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::PropertyDeclaration => self.emit_property_declaration(node),
            Kind::MethodDeclaration => self.emit_method_declaration(node),
            Kind::ClassStaticBlockDeclaration => self.emit_class_static_block(node),
            Kind::Constructor => self.emit_constructor(node),
            Kind::GetAccessor => self.emit_accessor_declaration(node, Kind::GetKeyword),
            Kind::SetAccessor => self.emit_accessor_declaration(node, Kind::SetKeyword),
            Kind::IndexSignature => self.emit_index_signature(node),
            Kind::SemicolonClassElement => self.emit_semicolon_class_element(node),
            other => panic!("unexpected ClassElement: {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitTypeElement
    pub(crate) fn emit_type_element(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::PropertySignature => self.emit_property_signature(node),
            Kind::MethodSignature => self.emit_method_signature(node),
            Kind::CallSignature => self.emit_call_or_construct_signature(node, false),
            Kind::ConstructSignature => self.emit_call_or_construct_signature(node, true),
            Kind::GetAccessor => self.emit_accessor_declaration(node, Kind::GetKeyword),
            Kind::SetAccessor => self.emit_accessor_declaration(node, Kind::SetKeyword),
            Kind::IndexSignature => self.emit_index_signature(node),
            other => panic!("unexpected TypeElement: {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitPropertyDeclaration
    fn emit_property_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, name, postfix, type_node, initializer) = match self.arena().data(node) {
            NodeData::PropertyDeclaration(d) => (
                d.modifiers.clone(),
                d.name,
                d.postfix_token,
                d.type_node,
                d.initializer,
            ),
            other => panic!("expected PropertyDeclaration, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), true);
        self.emit_property_name(Some(name));
        if let Some(p) = postfix {
            self.emit_token_node(p);
        }
        self.emit_type_annotation(type_node);
        let eq_pos = self.arena().loc(name).end();
        self.emit_initializer(initializer, eq_pos, node);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitPropertySignature
    fn emit_property_signature(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, name, postfix, type_node) = match self.arena().data(node) {
            NodeData::PropertySignature(d) => {
                (d.modifiers.clone(), d.name, d.postfix_token, d.type_node)
            }
            other => panic!("expected PropertySignature, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.emit_property_name(Some(name));
        if let Some(p) = postfix {
            self.emit_token_node(p);
        }
        self.emit_type_annotation(type_node);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitMethodDeclaration
    pub(crate) fn emit_method_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let parts = match self.arena().data(node) {
            NodeData::MethodDeclaration(d) => MethodParts {
                modifiers: d.modifiers.clone(),
                asterisk: d.asterisk_token,
                name: d.name,
                postfix: d.postfix_token,
                type_params: d.type_parameters.clone(),
                params: d.parameters.clone(),
                ret: d.type_node,
                body: d.body,
            },
            other => panic!("expected MethodDeclaration, got {other:?}"),
        };
        let MethodParts {
            modifiers,
            asterisk,
            name,
            postfix,
            type_params,
            params,
            ret,
            body,
        } = parts;
        self.emit_modifier_list(node, modifiers.as_ref(), true);
        if let Some(a) = asterisk {
            self.emit_token_node(a);
        }
        self.emit_property_name(Some(name));
        if let Some(p) = postfix {
            self.emit_token_node(p);
        }
        self.push_name_generation_scope(node);
        self.emit_signature_of(node, type_params.as_ref(), &params, ret);
        self.emit_function_body_node(body);
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitMethodSignature
    fn emit_method_signature(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, name, postfix, type_params, params, ret) = match self.arena().data(node) {
            NodeData::MethodSignature(d) => (
                d.modifiers.clone(),
                d.name,
                d.postfix_token,
                d.type_parameters.clone(),
                d.parameters.clone(),
                d.type_node,
            ),
            other => panic!("expected MethodSignature, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.emit_property_name(Some(name));
        if let Some(p) = postfix {
            self.emit_token_node(p);
        }
        self.push_name_generation_scope(node);
        self.emit_signature_of(node, type_params.as_ref(), &params, ret);
        self.write_trailing_semicolon();
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitAccessorDeclaration
    pub(crate) fn emit_accessor_declaration(&mut self, node: NodeId, keyword: Kind) {
        self.enter_node(node);
        let (modifiers, name, type_params, params, ret, body) = match self.arena().data(node) {
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => (
                d.modifiers.clone(),
                d.name,
                d.type_parameters.clone(),
                d.parameters.clone(),
                d.type_node,
                d.body,
            ),
            other => panic!("expected accessor, got {other:?}"),
        };
        let pos = self.emit_modifier_list(node, modifiers.as_ref(), true);
        self.emit_token(keyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_property_name(Some(name));
        self.push_name_generation_scope(node);
        self.emit_signature_of(node, type_params.as_ref(), &params, ret);
        self.emit_function_body_node(body);
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitConstructor
    fn emit_constructor(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, type_params, params, ret, body) = match self.arena().data(node) {
            NodeData::ConstructorDeclaration(d) => (
                d.modifiers.clone(),
                d.type_parameters.clone(),
                d.parameters.clone(),
                d.type_node,
                d.body,
            ),
            other => panic!("expected Constructor, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.write_keyword("constructor");
        self.push_name_generation_scope(node);
        self.emit_signature_of(node, type_params.as_ref(), &params, ret);
        self.emit_function_body_node(body);
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitClassStaticBlockDeclaration
    fn emit_class_static_block(&mut self, node: NodeId) {
        self.enter_node(node);
        let body = match self.arena().data(node) {
            NodeData::ClassStaticBlockDeclaration(d) => d.body,
            other => panic!("expected ClassStaticBlock, got {other:?}"),
        };
        self.write_keyword("static");
        self.push_name_generation_scope(node);
        self.emit_function_body_node(Some(body));
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitIndexSignature
    fn emit_index_signature(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, parameters, type_node) = match self.arena().data(node) {
            NodeData::IndexSignatureDeclaration(d) => {
                (d.modifiers.clone(), d.parameters.clone(), d.type_node)
            }
            other => panic!("expected IndexSignature, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.emit_list(
            ListEmit::Parameter,
            Some(node),
            Some(&parameters),
            crate::list_format::ListFormat::INDEX_SIGNATURE_PARAMETERS,
        );
        self.emit_type_annotation(type_node);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    fn emit_call_or_construct_signature(&mut self, node: NodeId, is_construct: bool) {
        self.enter_node(node);
        let (type_params, params, ret) = match self.arena().data(node) {
            NodeData::CallSignature(d) | NodeData::ConstructSignature(d) => {
                (d.type_parameters.clone(), d.parameters.clone(), d.type_node)
            }
            other => panic!("expected call/construct signature, got {other:?}"),
        };
        if is_construct {
            self.write_keyword("new");
            self.write_space();
        }
        self.push_name_generation_scope(node);
        self.emit_signature_of(node, type_params.as_ref(), &params, ret);
        self.write_trailing_semicolon();
        self.pop_name_generation_scope(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitSemicolonClassElement
    fn emit_semicolon_class_element(&mut self, node: NodeId) {
        self.enter_node(node);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitEnumMember
    pub(crate) fn emit_enum_member(&mut self, node: NodeId) {
        self.enter_node(node);
        let (name, initializer) = match self.arena().data(node) {
            NodeData::EnumMember(d) => (d.name, d.initializer),
            other => panic!("expected EnumMember, got {other:?}"),
        };
        self.emit_property_name(Some(name));
        let eq_pos = self.arena().loc(name).end();
        self.emit_initializer(initializer, eq_pos, node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitFunctionDeclaration
    pub(crate) fn emit_function_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, asterisk, name, type_params, params, ret, body) =
            match self.arena().data(node) {
                NodeData::FunctionDeclaration(d) => (
                    d.modifiers.clone(),
                    d.asterisk_token,
                    d.name,
                    d.type_parameters.clone(),
                    d.parameters.clone(),
                    d.type_node,
                    d.body,
                ),
                other => panic!("expected FunctionDeclaration, got {other:?}"),
            };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.write_keyword("function");
        if let Some(a) = asterisk {
            self.emit_token_node(a);
        }
        self.write_space();
        if let Some(name) = name {
            self.emit_identifier_name(name);
        }
        let indented = self.should_emit_indented(node);
        self.increase_indent_if(indented);
        self.push_name_generation_scope(node);
        self.emit_signature_of(node, type_params.as_ref(), &params, ret);
        self.emit_function_body_node(body);
        self.pop_name_generation_scope(node);
        self.decrease_indent_if(indented);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitInterfaceDeclaration
    pub(crate) fn emit_interface_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, name, type_parameters, heritage_clauses, members) =
            match self.arena().data(node) {
                NodeData::InterfaceDeclaration(d) => (
                    d.modifiers.clone(),
                    d.name,
                    d.type_parameters.clone(),
                    d.heritage_clauses.clone(),
                    d.members.clone(),
                ),
                other => panic!("expected InterfaceDeclaration, got {other:?}"),
            };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.write_keyword("interface");
        self.write_space();
        self.emit_identifier_name(name.expect("interface name"));
        self.emit_type_parameters(node, type_parameters.as_ref());
        self.emit_list(
            ListEmit::HeritageClause,
            Some(node),
            heritage_clauses.as_ref(),
            crate::list_format::ListFormat::HERITAGE_CLAUSES,
        );
        self.write_space();
        self.write_punctuation("{");
        self.push_name_generation_scope(node);
        self.emit_list(
            ListEmit::TypeElement,
            Some(node),
            Some(&members),
            crate::list_format::ListFormat::INTERFACE_MEMBERS,
        );
        self.pop_name_generation_scope(node);
        self.write_punctuation("}");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitTypeAliasDeclaration
    pub(crate) fn emit_type_alias_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, name, type_parameters, type_node) = match self.arena().data(node) {
            NodeData::TypeAliasDeclaration(d) => (
                d.modifiers.clone(),
                d.name,
                d.type_parameters.clone(),
                d.type_node,
            ),
            other => panic!("expected TypeAliasDeclaration, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.write_keyword("type");
        self.write_space();
        self.emit_identifier_name(name);
        self.emit_type_parameters(node, type_parameters.as_ref());
        self.write_space();
        self.write_punctuation("=");
        self.write_space();
        self.emit_type_node_outside_extends(type_node);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitEnumDeclaration
    pub(crate) fn emit_enum_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, name, members) = match self.arena().data(node) {
            NodeData::EnumDeclaration(d) => (d.modifiers.clone(), d.name, d.members.clone()),
            other => panic!("expected EnumDeclaration, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.write_keyword("enum");
        self.write_space();
        self.emit_identifier_name(name);
        self.write_space();
        self.write_punctuation("{");
        self.emit_list(
            ListEmit::EnumMember,
            Some(node),
            Some(&members),
            crate::list_format::ListFormat::ENUM_MEMBERS,
        );
        self.write_punctuation("}");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitModuleDeclaration
    pub(crate) fn emit_module_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, keyword, name, mut body) = match self.arena().data(node) {
            NodeData::ModuleDeclaration(d) => (d.modifiers.clone(), d.keyword, d.name, d.body),
            other => panic!("expected ModuleDeclaration, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        if keyword != Kind::GlobalKeyword {
            self.write_keyword(if keyword == Kind::NamespaceKeyword {
                "namespace"
            } else {
                "module"
            });
            self.write_space();
        }
        self.emit_module_name(name);
        while let Some(b) = body {
            if self.arena().kind(b) != Kind::ModuleDeclaration {
                break;
            }
            let (inner_name, inner_body) = match self.arena().data(b) {
                NodeData::ModuleDeclaration(d) => (d.name, d.body),
                _ => unreachable!(),
            };
            self.write_punctuation(".");
            self.emit_module_name(inner_name);
            body = inner_body;
        }
        match body {
            None => self.write_trailing_semicolon(),
            Some(b) => {
                self.write_space();
                self.emit_module_block(b);
            }
        }
        self.exit_node(node);
    }

    fn emit_module_name(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::Identifier => self.emit_identifier_name(node),
            Kind::StringLiteral => self.emit_string_literal_member(node),
            other => panic!("unexpected ModuleName: {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitModuleBlock
    fn emit_module_block(&mut self, node: NodeId) {
        self.enter_node(node);
        let statements = match self.arena().data(node) {
            NodeData::ModuleBlock(d) => d.statements.clone(),
            other => panic!("expected ModuleBlock, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::OpenBraceToken, pos, WriteKind::Punctuation, node);
        let single =
            self.is_empty_block(node, &statements) || self.should_emit_on_single_line(node);
        let format = if single {
            crate::list_format::ListFormat::SINGLE_LINE_BLOCK_STATEMENTS
        } else {
            crate::list_format::ListFormat::MULTI_LINE_BLOCK_STATEMENTS
        };
        self.emit_list(ListEmit::Statement, Some(node), Some(&statements), format);
        self.emit_token(
            Kind::CloseBraceToken,
            statements.end(),
            WriteKind::Punctuation,
            node,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitNamespaceExportDeclaration
    pub(crate) fn emit_namespace_export_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let name = match self.arena().data(node) {
            NodeData::NamespaceExportDeclaration(d) => d.name,
            other => panic!("expected NamespaceExportDeclaration, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::ExportKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_token(Kind::AsKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_token(Kind::NamespaceKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_identifier_name(name);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitImportEqualsDeclaration
    pub(crate) fn emit_import_equals_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, is_type_only, name, module_reference) = match self.arena().data(node) {
            NodeData::ImportEqualsDeclaration(d) => (
                d.modifiers.clone(),
                d.is_type_only,
                d.name,
                d.module_reference,
            ),
            other => panic!("expected ImportEqualsDeclaration, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::ImportKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        if is_type_only {
            self.emit_token(Kind::TypeKeyword, pos, WriteKind::Keyword, node);
            self.write_space();
        }
        self.emit_identifier_name(name);
        self.write_space();
        self.emit_token(Kind::EqualsToken, pos, WriteKind::Punctuation, node);
        self.write_space();
        self.emit_module_reference(module_reference);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    fn emit_module_reference(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::Identifier => self.emit_identifier_name(node),
            Kind::QualifiedName => self.emit_entity_name(node),
            Kind::ExternalModuleReference => self.emit_external_module_reference(node),
            other => panic!("unhandled ModuleReference: {other:?}"),
        }
    }

    fn emit_external_module_reference(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::ExternalModuleReference(d) => d.expression,
            other => panic!("expected ExternalModuleReference, got {other:?}"),
        };
        self.write_keyword("require");
        self.write_punctuation("(");
        self.emit_expression(expression, OperatorPrecedence::DISALLOW_COMMA);
        self.write_punctuation(")");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitImportDeclaration
    pub(crate) fn emit_import_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, import_clause, module_specifier, attributes) = match self.arena().data(node)
        {
            NodeData::ImportDeclaration(d) => (
                d.modifiers.clone(),
                d.import_clause,
                d.module_specifier,
                d.attributes,
            ),
            other => panic!("expected ImportDeclaration, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::ImportKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        if let Some(import_clause) = import_clause {
            self.emit_import_clause(import_clause);
            self.write_space();
            self.emit_token(Kind::FromKeyword, pos, WriteKind::Keyword, node);
            self.write_space();
        }
        self.emit_expression(module_specifier, OperatorPrecedence::LOWEST);
        if let Some(attributes) = attributes {
            self.write_space();
            self.emit_import_attributes(attributes);
        }
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    fn emit_import_clause(&mut self, node: NodeId) {
        self.enter_node(node);
        let (phase_modifier, name, named_bindings) = match self.arena().data(node) {
            NodeData::ImportClause(d) => (d.phase_modifier, d.name, d.named_bindings),
            other => panic!("expected ImportClause, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        if phase_modifier != Kind::Unknown {
            self.emit_token(phase_modifier, pos, WriteKind::Keyword, node);
            self.write_space();
        }
        if let Some(name) = name {
            self.emit_identifier_name(name);
            if named_bindings.is_some() {
                self.emit_token(Kind::CommaToken, pos, WriteKind::Punctuation, node);
                self.write_space();
            }
        }
        if let Some(named_bindings) = named_bindings {
            self.emit_named_import_bindings(named_bindings);
        }
        self.exit_node(node);
    }

    fn emit_named_import_bindings(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::NamespaceImport => self.emit_namespace_import_or_export(node, false),
            Kind::NamedImports => self.emit_named_imports_or_exports(node),
            other => panic!("unhandled NamedImportBindings: {other:?}"),
        }
    }

    fn emit_namespace_import_or_export(&mut self, node: NodeId, is_export: bool) {
        self.enter_node(node);
        let name = match self.arena().data(node) {
            NodeData::NamespaceImport(d) | NodeData::NamespaceExport(d) => d.name,
            other => panic!("expected Namespace import/export, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::AsteriskToken, pos, WriteKind::Punctuation, node);
        self.write_space();
        self.emit_token(Kind::AsKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        if is_export {
            self.emit_module_export_name(name);
        } else {
            self.emit_identifier_name(name);
        }
        self.exit_node(node);
    }

    fn emit_named_imports_or_exports(&mut self, node: NodeId) {
        self.enter_node(node);
        let elements = match self.arena().data(node) {
            NodeData::NamedImports(d) | NodeData::NamedExports(d) => d.elements.clone(),
            other => panic!("expected Named imports/exports, got {other:?}"),
        };
        self.write_punctuation("{");
        self.emit_list(
            ListEmit::ImportOrExportSpecifier,
            Some(node),
            Some(&elements),
            crate::list_format::ListFormat::NAMED_IMPORTS_OR_EXPORTS_ELEMENTS,
        );
        self.write_punctuation("}");
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitExportAssignment
    pub(crate) fn emit_export_assignment(&mut self, node: NodeId) {
        self.enter_node(node);
        let (is_export_equals, expression) = match self.arena().data(node) {
            NodeData::ExportAssignment(d) => (d.is_export_equals, d.expression),
            other => panic!("expected ExportAssignment, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::ExportKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        if is_export_equals {
            self.emit_token(Kind::EqualsToken, pos, WriteKind::Operator, node);
        } else {
            self.emit_token(Kind::DefaultKeyword, pos, WriteKind::Keyword, node);
        }
        self.write_space();
        if is_export_equals {
            self.emit_expression(expression, OperatorPrecedence::Assignment);
        } else {
            let leftmost = self.leftmost_for_export(expression);
            if matches!(
                self.arena().kind(leftmost),
                Kind::ClassExpression | Kind::FunctionExpression
            ) {
                self.emit_expression(expression, OperatorPrecedence::Parentheses);
            } else {
                self.emit_expression(expression, OperatorPrecedence::Assignment);
            }
        }
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    fn leftmost_for_export(&self, node: NodeId) -> NodeId {
        let arena = self.arena();
        let mut node = node;
        loop {
            match arena.data(node) {
                NodeData::PostfixUnaryExpression(d) => node = d.operand,
                NodeData::BinaryExpression(d) => node = d.left,
                NodeData::ConditionalExpression(d) => node = d.condition,
                NodeData::PropertyAccessExpression(d) => node = d.expression,
                NodeData::ElementAccessExpression(d) => node = d.expression,
                NodeData::CallExpression(d) => node = d.expression,
                NodeData::NonNullExpression(d) => node = d.expression,
                NodeData::AsExpression(d) => node = d.expression,
                NodeData::SatisfiesExpression(d) => node = d.expression,
                NodeData::TaggedTemplateExpression(d) => node = d.tag,
                _ => return node,
            }
        }
    }

    // Go: internal/printer/printer.go:emitExportDeclaration
    pub(crate) fn emit_export_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, is_type_only, export_clause, module_specifier, attributes) =
            match self.arena().data(node) {
                NodeData::ExportDeclaration(d) => (
                    d.modifiers.clone(),
                    d.is_type_only,
                    d.export_clause,
                    d.module_specifier,
                    d.attributes,
                ),
                other => panic!("expected ExportDeclaration, got {other:?}"),
            };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::ExportKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        if is_type_only {
            self.emit_token(Kind::TypeKeyword, pos, WriteKind::Keyword, node);
            self.write_space();
        }
        if let Some(export_clause) = export_clause {
            self.emit_named_export_bindings(export_clause);
        } else {
            self.emit_token(Kind::AsteriskToken, pos, WriteKind::Punctuation, node);
        }
        if let Some(module_specifier) = module_specifier {
            self.write_space();
            self.emit_token(Kind::FromKeyword, pos, WriteKind::Keyword, node);
            self.write_space();
            self.emit_expression(module_specifier, OperatorPrecedence::LOWEST);
        }
        if let Some(attributes) = attributes {
            self.write_space();
            self.emit_import_attributes(attributes);
        }
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    fn emit_named_export_bindings(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::NamespaceExport => self.emit_namespace_import_or_export(node, true),
            Kind::NamedExports => self.emit_named_imports_or_exports(node),
            other => panic!("unhandled NamedExportBindings: {other:?}"),
        }
    }

    fn emit_module_export_name(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::Identifier => self.emit_identifier_name(node),
            Kind::StringLiteral => self.emit_string_literal_member(node),
            other => panic!("unexpected ModuleExportName: {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitImportAttributes
    fn emit_import_attributes(&mut self, node: NodeId) {
        self.enter_node(node);
        let (token, attributes) = match self.arena().data(node) {
            NodeData::ImportAttributes(d) => (d.token, d.attributes.clone()),
            other => panic!("expected ImportAttributes, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(token, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_list(
            ListEmit::ImportAttribute,
            Some(node),
            Some(&attributes),
            crate::list_format::ListFormat::IMPORT_ATTRIBUTES,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitImportSpecifier / emitExportSpecifier
    pub(crate) fn emit_import_or_export_specifier(&mut self, node: NodeId) {
        self.enter_node(node);
        let (is_type_only, property_name, name) = match self.arena().data(node) {
            NodeData::ImportSpecifier(d) | NodeData::ExportSpecifier(d) => {
                (d.is_type_only, d.property_name, d.name)
            }
            other => panic!("expected import/export specifier, got {other:?}"),
        };
        let is_export = self.arena().kind(node) == Kind::ExportSpecifier;
        if is_type_only {
            self.write_keyword("type");
            self.write_space();
        }
        if let Some(property_name) = property_name {
            self.emit_module_export_name(property_name);
            self.write_space();
            let pos = self.arena().loc(property_name).end();
            self.emit_token(Kind::AsKeyword, pos, WriteKind::Keyword, node);
            self.write_space();
        }
        if is_export {
            self.emit_module_export_name(name);
        } else {
            self.emit_identifier_name(name);
        }
        self.exit_node(node);
    }

    pub(crate) fn emit_import_attribute(&mut self, node: NodeId) {
        let (name, value) = match self.arena().data(node) {
            NodeData::ImportAttribute(d) => (d.name, d.value),
            other => panic!("expected ImportAttribute, got {other:?}"),
        };
        if let Some(name) = name {
            self.emit_import_attribute_name(name);
        }
        self.write_punctuation(":");
        self.write_space();
        self.emit_expression(value, OperatorPrecedence::DISALLOW_COMMA);
    }

    fn emit_import_attribute_name(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::Identifier => self.emit_identifier_name(node),
            Kind::StringLiteral => self.emit_string_literal_member(node),
            other => panic!("unexpected ImportAttributeName: {other:?}"),
        }
    }

    pub(crate) fn emit_string_literal_member(&mut self, node: NodeId) {
        self.enter_node(node);
        self.emit_literal(node, GetLiteralTextFlags::NONE);
        self.exit_node(node);
    }
}

#[cfg(test)]
#[path = "emit_declarations_test.rs"]
mod tests;
