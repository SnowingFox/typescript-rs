//! Statement emit: the `emit_statement` dispatcher and per-kind statement
//! emitters. (`impl Printer` block; see `printer.rs` for the core.)

use crate::printer::{ListEmit, Printer, WriteKind};
use tsgo_ast::precedence::OperatorPrecedence;
use tsgo_ast::{Kind, NodeData, NodeFlags, NodeId};

impl Printer<'_> {
    /// Dispatches statement emit by node kind (the body of Go `emitStatement`'s
    /// switch).
    // Go: internal/printer/printer.go:emitStatement
    pub(crate) fn emit_statement(&mut self, node: NodeId) {
        match self.arena().kind(node) {
            Kind::Block => self.emit_block(node),
            Kind::EmptyStatement => self.emit_empty_statement(node, false),
            Kind::VariableStatement => self.emit_variable_statement(node),
            Kind::ExpressionStatement => self.emit_expression_statement(node),
            Kind::IfStatement => self.emit_if_statement(node),
            Kind::DoStatement => self.emit_do_statement(node),
            Kind::WhileStatement => self.emit_while_statement(node),
            Kind::ForStatement => self.emit_for_statement(node),
            Kind::ForInStatement => self.emit_for_in_statement(node),
            Kind::ForOfStatement => self.emit_for_of_statement(node),
            Kind::ContinueStatement => self.emit_continue_or_break(node, Kind::ContinueKeyword),
            Kind::BreakStatement => self.emit_continue_or_break(node, Kind::BreakKeyword),
            Kind::ReturnStatement => self.emit_return_statement(node),
            Kind::WithStatement => self.emit_with_statement(node),
            Kind::SwitchStatement => self.emit_switch_statement(node),
            Kind::LabeledStatement => self.emit_labeled_statement(node),
            Kind::ThrowStatement => self.emit_throw_statement(node),
            Kind::TryStatement => self.emit_try_statement(node),
            Kind::DebuggerStatement => self.emit_debugger_statement(node),
            Kind::FunctionDeclaration => self.emit_function_declaration(node),
            Kind::ClassDeclaration => self.emit_class_like(node),
            Kind::InterfaceDeclaration => self.emit_interface_declaration(node),
            Kind::TypeAliasDeclaration => self.emit_type_alias_declaration(node),
            Kind::EnumDeclaration => self.emit_enum_declaration(node),
            Kind::ModuleDeclaration => self.emit_module_declaration(node),
            Kind::NamespaceExportDeclaration => self.emit_namespace_export_declaration(node),
            Kind::ImportEqualsDeclaration => self.emit_import_equals_declaration(node),
            Kind::ImportDeclaration => self.emit_import_declaration(node),
            Kind::ExportAssignment => self.emit_export_assignment(node),
            Kind::ExportDeclaration => self.emit_export_declaration(node),
            Kind::NotEmittedStatement => self.emit_not_emitted_statement(node),
            Kind::SyntaxList => self.emit_syntax_list_statements(node),
            other => panic!("unhandled statement: {other:?}"),
        }
    }

    /// Emits a [`Kind::SyntaxList`](tsgo_ast::Kind::SyntaxList) in statement
    /// position: each child statement in sequence, separated by a line break (so
    /// a transform can return several sibling statements where one node was).
    // Go: internal/printer/printer.go:emitList (a `SyntaxList` is flattened into its container)
    fn emit_syntax_list_statements(&mut self, node: NodeId) {
        let children = match self.arena().data(node) {
            NodeData::SyntaxList(d) => d.list.nodes.clone(),
            other => panic!("expected SyntaxList, got {other:?}"),
        };
        for (index, child) in children.iter().enumerate() {
            if index > 0 {
                self.write_line();
            }
            self.emit_statement(*child);
        }
    }

    /// Emits a [`Kind::NotEmittedStatement`](tsgo_ast::Kind::NotEmittedStatement):
    /// no output (the elided declaration's slot is preserved only for leading
    /// comments, which run through `enter_node`/`exit_node`).
    // Go: internal/printer/printer.go:emitNotEmittedStatement
    fn emit_not_emitted_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitEmptyStatement
    pub(crate) fn emit_empty_statement(&mut self, node: NodeId, is_embedded: bool) {
        self.enter_node(node);
        if is_embedded {
            self.write_punctuation(";");
        } else {
            self.write_trailing_semicolon();
        }
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitExpressionStatement
    fn emit_expression_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::ExpressionStatement(d) => d.expression,
            other => panic!("expected ExpressionStatement, got {other:?}"),
        };
        // IIFE-callee parenthesization is deferred (no current `TestEmit` case is
        // an immediately-invoked function expression).
        let leftmost = self.get_leftmost_expression(expression);
        match self.arena().kind(leftmost) {
            Kind::FunctionExpression | Kind::ObjectLiteralExpression => {
                self.emit_expression(expression, OperatorPrecedence::Parentheses)
            }
            _ => self.emit_expression(expression, OperatorPrecedence::Comma),
        }
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitVariableStatement
    fn emit_variable_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (modifiers, declaration_list) = match self.arena().data(node) {
            NodeData::VariableStatement(d) => (d.modifiers.clone(), d.declaration_list),
            other => panic!("expected VariableStatement, got {other:?}"),
        };
        self.emit_modifier_list(node, modifiers.as_ref(), false);
        self.emit_variable_declaration_list(declaration_list);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitVariableDeclarationList
    fn emit_variable_declaration_list(&mut self, node: NodeId) {
        self.enter_node(node);
        let (declarations, flags) = match self.arena().data(node) {
            NodeData::VariableDeclarationList(d) => {
                (d.declarations.clone(), self.arena().flags(node))
            }
            other => panic!("expected VariableDeclarationList, got {other:?}"),
        };
        let block_scoped = flags & NodeFlags::BLOCK_SCOPED;
        if block_scoped == NodeFlags::LET {
            self.write_keyword("let");
        } else if block_scoped == NodeFlags::CONST {
            self.write_keyword("const");
        } else if block_scoped == NodeFlags::USING {
            self.write_keyword("using");
        } else if block_scoped == NodeFlags::AWAIT_USING {
            self.write_keyword("await");
            self.write_space();
            self.write_keyword("using");
        } else {
            self.write_keyword("var");
        }
        self.write_space();
        self.emit_list(
            ListEmit::VariableDeclaration,
            Some(node),
            Some(&declarations),
            crate::list_format::ListFormat::VARIABLE_DECLARATION_LIST,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitVariableDeclaration
    pub(crate) fn emit_variable_declaration(&mut self, node: NodeId) {
        self.enter_node(node);
        let (name, exclamation, type_node, initializer) = match self.arena().data(node) {
            NodeData::VariableDeclaration(d) => {
                (d.name, d.exclamation_token, d.type_node, d.initializer)
            }
            other => panic!("expected VariableDeclaration, got {other:?}"),
        };
        self.emit_binding_name(Some(name));
        if let Some(e) = exclamation {
            self.emit_token_node(e);
        }
        self.emit_type_annotation(type_node);
        let eq_pos = self.arena().loc(name).end();
        self.emit_initializer(initializer, eq_pos, node);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitIfStatement
    fn emit_if_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, then_statement, else_statement) = match self.arena().data(node) {
            NodeData::IfStatement(d) => (d.expression, d.then_statement, d.else_statement),
            other => panic!("expected IfStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        let pos = self.emit_token(Kind::IfKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_token(Kind::OpenParenToken, pos, WriteKind::Punctuation, node);
        self.emit_expression(expression, OperatorPrecedence::LOWEST);
        let close = self.arena().loc(expression).end();
        self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
        self.emit_embedded_statement(node, then_statement);
        if let Some(else_statement) = else_statement {
            self.write_line_or_space(node, then_statement, else_statement);
            let else_pos = self.arena().loc(then_statement).end();
            self.emit_token(Kind::ElseKeyword, else_pos, WriteKind::Keyword, node);
            if self.arena().kind(else_statement) == Kind::IfStatement {
                self.write_space();
                self.emit_if_statement(else_statement);
            } else {
                self.emit_embedded_statement(node, else_statement);
            }
        }
        self.exit_node(node);
    }

    fn emit_while_clause(&mut self, node: NodeId, expression: NodeId, start_pos: i32) {
        let pos = self.emit_token(Kind::WhileKeyword, start_pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_token(Kind::OpenParenToken, pos, WriteKind::Punctuation, node);
        self.emit_expression(expression, OperatorPrecedence::LOWEST);
        let close = self.arena().loc(expression).end();
        self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
    }

    // Go: internal/printer/printer.go:emitDoStatement
    fn emit_do_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (statement, expression) = match self.arena().data(node) {
            NodeData::DoStatement(d) => (d.statement, d.expression),
            other => panic!("expected DoStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::DoKeyword, pos, WriteKind::Keyword, node);
        self.emit_embedded_statement(node, statement);
        if self.arena().kind(statement) == Kind::Block && !self.options_preserve_source_newlines() {
            self.write_space();
        } else {
            self.write_line_or_space(node, statement, expression);
        }
        let end = self.arena().loc(statement).end();
        self.emit_while_clause(node, expression, end);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitWhileStatement
    fn emit_while_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, statement) = match self.arena().data(node) {
            NodeData::WhileStatement(d) => (d.expression, d.statement),
            other => panic!("expected WhileStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_while_clause(node, expression, pos);
        self.emit_embedded_statement(node, statement);
        self.exit_node(node);
    }

    fn emit_for_initializer(&mut self, node: NodeId) {
        if self.arena().kind(node) == Kind::VariableDeclarationList {
            self.emit_variable_declaration_list(node);
        } else {
            self.emit_expression(node, OperatorPrecedence::LOWEST);
        }
    }

    // Go: internal/printer/printer.go:emitForStatement
    fn emit_for_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (initializer, condition, incrementor, statement) = match self.arena().data(node) {
            NodeData::ForStatement(d) => (d.initializer, d.condition, d.incrementor, d.statement),
            other => panic!("expected ForStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        let pos = self.emit_token(Kind::ForKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        let mut pos = self.emit_token(Kind::OpenParenToken, pos, WriteKind::Punctuation, node);
        if let Some(initializer) = initializer {
            self.emit_for_initializer(initializer);
            pos = self.arena().loc(initializer).end();
        }
        pos = self.emit_token(Kind::SemicolonToken, pos, WriteKind::Punctuation, node);
        if let Some(condition) = condition {
            self.write_space();
            self.emit_expression(condition, OperatorPrecedence::LOWEST);
            pos = self.arena().loc(condition).end();
        }
        pos = self.emit_token(Kind::SemicolonToken, pos, WriteKind::Punctuation, node);
        if let Some(incrementor) = incrementor {
            self.write_space();
            self.emit_expression(incrementor, OperatorPrecedence::LOWEST);
            pos = self.arena().loc(incrementor).end();
        }
        self.emit_token(Kind::CloseParenToken, pos, WriteKind::Punctuation, node);
        self.emit_embedded_statement(node, statement);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitForInStatement
    fn emit_for_in_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (initializer, expression, statement) = self.for_in_or_of_parts(node);
        let pos = self.arena().loc(node).pos();
        let pos = self.emit_token(Kind::ForKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_token(Kind::OpenParenToken, pos, WriteKind::Punctuation, node);
        self.emit_for_initializer(initializer);
        self.write_space();
        let in_pos = self.arena().loc(initializer).end();
        self.emit_token(Kind::InKeyword, in_pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_expression(expression, OperatorPrecedence::LOWEST);
        let close = self.arena().loc(expression).end();
        self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
        self.emit_embedded_statement(node, statement);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitForOfStatement
    fn emit_for_of_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (await_modifier, initializer, expression, statement) = match self.arena().data(node) {
            NodeData::ForInOrOfStatement(d) => {
                (d.await_modifier, d.initializer, d.expression, d.statement)
            }
            other => panic!("expected ForOfStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        let open_paren_pos = self.emit_token(Kind::ForKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        if let Some(m) = await_modifier {
            self.emit_keyword_node(Some(m));
            self.write_space();
        }
        self.emit_token(
            Kind::OpenParenToken,
            open_paren_pos,
            WriteKind::Punctuation,
            node,
        );
        self.emit_for_initializer(initializer);
        self.write_space();
        let of_pos = self.arena().loc(initializer).end();
        self.emit_token(Kind::OfKeyword, of_pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_expression(expression, OperatorPrecedence::LOWEST);
        let close = self.arena().loc(expression).end();
        self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
        self.emit_embedded_statement(node, statement);
        self.exit_node(node);
    }

    fn for_in_or_of_parts(&self, node: NodeId) -> (NodeId, NodeId, NodeId) {
        match self.arena().data(node) {
            NodeData::ForInOrOfStatement(d) => (d.initializer, d.expression, d.statement),
            other => panic!("expected ForIn/Of, got {other:?}"),
        }
    }

    // Go: internal/printer/printer.go:emitContinueStatement / emitBreakStatement
    fn emit_continue_or_break(&mut self, node: NodeId, keyword: Kind) {
        self.enter_node(node);
        let label = match self.arena().data(node) {
            NodeData::ContinueStatement(d) | NodeData::BreakStatement(d) => d.label,
            other => panic!("expected Continue/Break, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(keyword, pos, WriteKind::Keyword, node);
        if let Some(label) = label {
            self.write_space();
            self.emit_identifier_name(label);
        }
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitReturnStatement
    fn emit_return_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::ReturnStatement(d) => d.expression,
            other => panic!("expected ReturnStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::ReturnKeyword, pos, WriteKind::Keyword, node);
        if let Some(expression) = expression {
            self.write_space();
            self.emit_expression_no_asi(expression, OperatorPrecedence::LOWEST);
        }
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitWithStatement
    fn emit_with_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, statement) = match self.arena().data(node) {
            NodeData::WithStatement(d) => (d.expression, d.statement),
            other => panic!("expected WithStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        let pos = self.emit_token(Kind::WithKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_token(Kind::OpenParenToken, pos, WriteKind::Punctuation, node);
        self.emit_expression(expression, OperatorPrecedence::LOWEST);
        let close = self.arena().loc(expression).end();
        self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
        self.emit_embedded_statement(node, statement);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitSwitchStatement
    fn emit_switch_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, case_block) = match self.arena().data(node) {
            NodeData::SwitchStatement(d) => (d.expression, d.case_block),
            other => panic!("expected SwitchStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        let pos = self.emit_token(Kind::SwitchKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_token(Kind::OpenParenToken, pos, WriteKind::Punctuation, node);
        self.emit_expression(expression, OperatorPrecedence::LOWEST);
        let close = self.arena().loc(expression).end();
        self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
        self.write_space();
        self.emit_case_block(case_block);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitCaseBlock
    fn emit_case_block(&mut self, node: NodeId) {
        self.enter_node(node);
        let clauses = match self.arena().data(node) {
            NodeData::CaseBlock(d) => d.clauses.clone(),
            other => panic!("expected CaseBlock, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::OpenBraceToken, pos, WriteKind::Punctuation, node);
        self.emit_list(
            ListEmit::CaseOrDefaultClause,
            Some(node),
            Some(&clauses),
            crate::list_format::ListFormat::CASE_BLOCK_CLAUSES,
        );
        self.emit_token(
            Kind::CloseBraceToken,
            clauses.end(),
            WriteKind::Punctuation,
            node,
        );
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitCaseClause / emitDefaultClause
    pub(crate) fn emit_case_or_default_clause(&mut self, node: NodeId) {
        self.enter_node(node);
        let (expression, statements) = match self.arena().data(node) {
            NodeData::CaseOrDefaultClause(d) => (d.expression, d.statements.clone()),
            other => panic!("expected CaseOrDefaultClause, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        match expression {
            Some(expression) => {
                self.emit_token(Kind::CaseKeyword, pos, WriteKind::Keyword, node);
                self.write_space();
                self.emit_expression(expression, OperatorPrecedence::LOWEST);
            }
            None => {
                self.emit_token(Kind::DefaultKeyword, pos, WriteKind::Keyword, node);
            }
        }
        let colon_pos = match expression {
            Some(e) => self.arena().loc(e).end(),
            None => pos,
        };

        // A single statement on the same source line as the clause is emitted
        // inline (`case b: ;`); otherwise statements are indented on new lines.
        let emit_as_single_statement = statements.nodes.len() == 1
            && (self.current_source_file().is_none()
                || crate::printer::node_is_synthesized(self.arena(), node)
                || crate::printer::node_is_synthesized(self.arena(), statements.nodes[0])
                || self.range_start_positions_same_line(node, statements.nodes[0]));

        let mut format = crate::list_format::ListFormat::CASE_OR_DEFAULT_CLAUSE_STATEMENTS;
        if emit_as_single_statement {
            self.write_token_text(Kind::ColonToken, WriteKind::Punctuation, colon_pos);
            self.write_space();
            format &= !(crate::list_format::ListFormat::MULTI_LINE
                | crate::list_format::ListFormat::INDENTED);
        } else {
            self.emit_token(Kind::ColonToken, colon_pos, WriteKind::Punctuation, node);
        }
        self.emit_list(ListEmit::Statement, Some(node), Some(&statements), format);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitLabeledStatement
    fn emit_labeled_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (label, statement) = match self.arena().data(node) {
            NodeData::LabeledStatement(d) => (d.label, d.statement),
            other => panic!("expected LabeledStatement, got {other:?}"),
        };
        self.emit_identifier_name(label);
        let colon_pos = self.arena().loc(label).end();
        self.emit_token(Kind::ColonToken, colon_pos, WriteKind::Punctuation, node);
        self.write_space();
        self.emit_statement(statement);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitThrowStatement
    fn emit_throw_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let expression = match self.arena().data(node) {
            NodeData::ThrowStatement(d) => d.expression,
            other => panic!("expected ThrowStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::ThrowKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_expression_no_asi(expression, OperatorPrecedence::LOWEST);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitTryStatement
    fn emit_try_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let (try_block, catch_clause, finally_block) = match self.arena().data(node) {
            NodeData::TryStatement(d) => (d.try_block, d.catch_clause, d.finally_block),
            other => panic!("expected TryStatement, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::TryKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        self.emit_block(try_block);
        if let Some(catch_clause) = catch_clause {
            self.write_line_or_space(node, try_block, catch_clause);
            self.emit_catch_clause(catch_clause);
        }
        if let Some(finally_block) = finally_block {
            let prev = catch_clause.unwrap_or(try_block);
            self.write_line_or_space(node, prev, finally_block);
            let finally_pos = self.arena().loc(prev).end();
            self.emit_token(Kind::FinallyKeyword, finally_pos, WriteKind::Keyword, node);
            self.write_space();
            self.emit_block(finally_block);
        }
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitCatchClause
    fn emit_catch_clause(&mut self, node: NodeId) {
        self.enter_node(node);
        let (variable_declaration, block) = match self.arena().data(node) {
            NodeData::CatchClause(d) => (d.variable_declaration, d.block),
            other => panic!("expected CatchClause, got {other:?}"),
        };
        let pos = self.arena().loc(node).pos();
        let open_paren_pos = self.emit_token(Kind::CatchKeyword, pos, WriteKind::Keyword, node);
        self.write_space();
        if let Some(variable_declaration) = variable_declaration {
            self.emit_token(
                Kind::OpenParenToken,
                open_paren_pos,
                WriteKind::Punctuation,
                node,
            );
            self.emit_variable_declaration(variable_declaration);
            let close = self.arena().loc(variable_declaration).end();
            self.emit_token(Kind::CloseParenToken, close, WriteKind::Punctuation, node);
            self.write_space();
        }
        self.emit_block(block);
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitDebuggerStatement
    fn emit_debugger_statement(&mut self, node: NodeId) {
        self.enter_node(node);
        let pos = self.arena().loc(node).pos();
        self.emit_token(Kind::DebuggerKeyword, pos, WriteKind::Keyword, node);
        self.write_trailing_semicolon();
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitEmbeddedStatement
    fn emit_embedded_statement(&mut self, parent_node: NodeId, node: NodeId) {
        if self.arena().kind(node) == Kind::Block || self.should_emit_on_single_line(parent_node) {
            self.write_space();
            self.emit_statement(node);
        } else {
            self.write_line();
            self.increase_indent();
            if self.arena().kind(node) == Kind::EmptyStatement {
                self.emit_empty_statement(node, true);
            } else {
                self.emit_statement(node);
            }
            self.decrease_indent();
        }
    }

    // Go: internal/printer/printer.go:writeLineOrSpace
    fn write_line_or_space(&mut self, parent_node: NodeId, prev_child: NodeId, next_child: NodeId) {
        if self.should_emit_on_single_line(parent_node) {
            self.write_space();
        } else if self.options_preserve_source_newlines() {
            let lines = self.get_lines_between_nodes(parent_node, prev_child, next_child);
            if lines > 0 {
                self.write_line_repeat(lines);
            } else {
                self.write_space();
            }
        } else {
            self.write_line();
        }
    }

    // Go: internal/printer/printer.go:emitObjectBindingPattern / emitArrayBindingPattern
    pub(crate) fn emit_binding_pattern(&mut self, node: NodeId) {
        self.enter_node(node);
        let (elements, is_object) = match self.arena().data(node) {
            NodeData::ObjectBindingPattern(d) => (d.elements.clone(), true),
            NodeData::ArrayBindingPattern(d) => (d.elements.clone(), false),
            other => panic!("expected BindingPattern, got {other:?}"),
        };
        if is_object {
            self.write_punctuation("{");
            self.emit_list(
                ListEmit::BindingElement,
                Some(node),
                Some(&elements),
                crate::list_format::ListFormat::OBJECT_BINDING_PATTERN_ELEMENTS,
            );
            self.write_punctuation("}");
        } else {
            self.write_punctuation("[");
            self.emit_list(
                ListEmit::BindingElement,
                Some(node),
                Some(&elements),
                crate::list_format::ListFormat::ARRAY_BINDING_PATTERN_ELEMENTS,
            );
            self.write_punctuation("]");
        }
        self.exit_node(node);
    }

    // Go: internal/printer/printer.go:emitBindingElement
    pub(crate) fn emit_binding_element(&mut self, node: NodeId) {
        self.enter_node(node);
        let (dot_dot_dot, property_name, name, initializer) = match self.arena().data(node) {
            NodeData::BindingElement(d) => {
                (d.dot_dot_dot_token, d.property_name, d.name, d.initializer)
            }
            other => panic!("expected BindingElement, got {other:?}"),
        };
        if let Some(d) = dot_dot_dot {
            self.emit_token_node(d);
        }
        if let Some(property_name) = property_name {
            self.emit_property_name(Some(property_name));
            self.write_punctuation(":");
            self.write_space();
        }
        if let Some(name) = name {
            self.emit_binding_name(Some(name));
            let eq_pos = self.arena().loc(name).end();
            self.emit_initializer(initializer, eq_pos, node);
        }
        self.exit_node(node);
    }

    /// Walks down to the leftmost expression node.
    // Go: internal/ast/utilities.go:GetLeftmostExpression
    fn get_leftmost_expression(&self, node: NodeId) -> NodeId {
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
}

#[cfg(test)]
#[path = "emit_statements_test.rs"]
mod tests;
