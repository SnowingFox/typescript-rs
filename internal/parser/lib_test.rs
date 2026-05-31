use super::*;
use tsgo_ast::NodeData;

// Parses TypeScript source and returns the result; a `parsetestutil`-style
// helper for the behavior-level tests below.
// Go: internal/parser/parser_test.go (uses parsetestutil.ParseTypeScript)
fn parse_ts(source: &str) -> ParseResult {
    parse_source_file(SourceFileParseOptions::default(), source, ScriptKind::Ts)
}

// Parses `.tsx` source (JSX language variant).
fn parse_tsx(source: &str) -> ParseResult {
    parse_source_file(SourceFileParseOptions::default(), source, ScriptKind::Tsx)
}

// Parses JSON source.
fn parse_json(source: &str) -> ParseResult {
    parse_source_file(SourceFileParseOptions::default(), source, ScriptKind::Json)
}

// Returns the statement ids of a parsed source file.
fn statements(result: &ParseResult) -> Vec<NodeId> {
    match result.arena.data(result.source_file) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        other => panic!("expected SourceFile, got {other:?}"),
    }
}

// Go: parser.go:parseSourceFileWorker / tests.md:parse_empty_source
#[test]
fn parse_empty_source() {
    let result = parse_ts("");
    assert!(
        result.diagnostics.is_empty(),
        "no diagnostics for empty source"
    );
    assert_eq!(statements(&result).len(), 0, "no statements");
    match result.arena.data(result.source_file) {
        NodeData::SourceFile(d) => {
            assert_eq!(result.arena.kind(d.end_of_file_token), Kind::EndOfFile);
        }
        other => panic!("expected SourceFile, got {other:?}"),
    }
}

// Go: parser.go:parseEmptyStatement / tests.md:parse_single_empty_statement
#[test]
fn parse_single_empty_statement() {
    let result = parse_ts(";");
    let stmts = statements(&result);
    assert_eq!(stmts.len(), 1);
    assert_eq!(result.arena.kind(stmts[0]), Kind::EmptyStatement);
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:finishNode/nodePos / tests.md:parse_expression_statement_pos_end
#[test]
fn parse_expression_statement_pos_end() {
    let result = parse_ts("  x;");
    let stmts = statements(&result);
    assert_eq!(stmts.len(), 1);
    let stmt = stmts[0];
    assert_eq!(result.arena.kind(stmt), Kind::ExpressionStatement);
    // The statement node's pos includes the leading whitespace trivia.
    assert_eq!(result.arena.loc(stmt).pos(), 0);
    let expr = match result.arena.data(stmt) {
        NodeData::ExpressionStatement(d) => d.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    assert_eq!(result.arena.kind(expr), Kind::Identifier);
    assert_eq!(result.arena.text(expr), "x");
    // The identifier carries the leading trivia in its pos and ends after `x`.
    assert_eq!(result.arena.loc(expr).pos(), 0);
    assert_eq!(result.arena.loc(expr).end(), 3);
}

// Go: parser.go:parseBinaryExpressionRest / tests.md:parse_binary_precedence
#[test]
fn parse_binary_precedence() {
    let result = parse_ts("1 + 2 * 3;");
    let stmts = statements(&result);
    let expr = match result.arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    // Top is `1 + (2 * 3)`.
    let (left, op, right) = match result.arena.data(expr) {
        NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
        other => panic!("expected BinaryExpression, got {other:?}"),
    };
    assert_eq!(result.arena.kind(op), Kind::PlusToken);
    assert_eq!(result.arena.text(left), "1");
    // Right is `2 * 3`.
    let (rl, rop, rr) = match result.arena.data(right) {
        NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
        other => panic!("expected nested BinaryExpression, got {other:?}"),
    };
    assert_eq!(result.arena.kind(rop), Kind::AsteriskToken);
    assert_eq!(result.arena.text(rl), "2");
    assert_eq!(result.arena.text(rr), "3");
}

// Go: parser.go:parseConditionalExpressionRest / tests.md:parse_conditional
#[test]
fn parse_conditional() {
    let result = parse_ts("a ? b : c;");
    let stmts = statements(&result);
    let expr = match result.arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    match result.arena.data(expr) {
        NodeData::ConditionalExpression(d) => {
            assert_eq!(result.arena.text(d.condition), "a");
            assert_eq!(result.arena.kind(d.question_token), Kind::QuestionToken);
            assert_eq!(result.arena.text(d.when_true), "b");
            assert_eq!(result.arena.kind(d.colon_token), Kind::ColonToken);
            assert_eq!(result.arena.text(d.when_false), "c");
        }
        other => panic!("expected ConditionalExpression, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseParenthesizedExpression / tests.md:parse_arrow_speculation (paren half)
#[test]
fn parse_parenthesized_expression() {
    let result = parse_ts("(x);");
    let stmts = statements(&result);
    let expr = match result.arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    match result.arena.data(expr) {
        NodeData::ParenthesizedExpression(d) => {
            assert_eq!(result.arena.text(d.expression), "x");
        }
        other => panic!("expected ParenthesizedExpression, got {other:?}"),
    }
}

// Go: parser.go:parsePrefixUnaryExpression / tests.md:parse_paren_unary (prefix)
#[test]
fn parse_prefix_unary_expression() {
    let result = parse_ts("-a;");
    let stmts = statements(&result);
    let expr = match result.arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    match result.arena.data(expr) {
        NodeData::PrefixUnaryExpression(d) => {
            assert_eq!(d.operator, Kind::MinusToken);
            assert_eq!(result.arena.text(d.operand), "a");
        }
        other => panic!("expected PrefixUnaryExpression, got {other:?}"),
    }
}

// Go: parser.go:parseUpdateExpression / tests.md:parse_paren_unary (postfix)
#[test]
fn parse_postfix_unary_expression() {
    let result = parse_ts("a++;");
    let stmts = statements(&result);
    let expr = match result.arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    match result.arena.data(expr) {
        NodeData::PostfixUnaryExpression(d) => {
            assert_eq!(d.operator, Kind::PlusPlusToken);
            assert_eq!(result.arena.text(d.operand), "a");
        }
        other => panic!("expected PostfixUnaryExpression, got {other:?}"),
    }
}

// Go: parser.go:ParseIsolatedEntityName / tests.md:parse_isolated_entity_name_chain
#[test]
fn parse_isolated_entity_name_chain() {
    let (arena, entity) = parse_isolated_entity_name("a.b.c").expect("a.b.c parses");
    // `a.b.c` => QualifiedName(QualifiedName(a, b), c)
    let (left, right) = match arena.data(entity) {
        NodeData::QualifiedName(d) => (d.left, d.right),
        other => panic!("expected QualifiedName, got {other:?}"),
    };
    assert_eq!(arena.text(right), "c");
    let (ll, lr) = match arena.data(left) {
        NodeData::QualifiedName(d) => (d.left, d.right),
        other => panic!("expected nested QualifiedName, got {other:?}"),
    };
    assert_eq!(arena.text(ll), "a");
    assert_eq!(arena.text(lr), "b");
}

// Go: parser.go:ParseIsolatedEntityName (failure path)
#[test]
fn parse_isolated_entity_name_invalid() {
    assert!(parse_isolated_entity_name("a.=").is_none());
}

// Returns the single top-level statement kind.
fn only_stmt_kind(result: &ParseResult) -> Kind {
    let stmts = statements(result);
    assert_eq!(stmts.len(), 1, "expected exactly one statement");
    result.arena.kind(stmts[0])
}

// Go: parser.go:parseBlock
#[test]
fn parse_block_statement() {
    let result = parse_ts("{ a; b; }");
    let stmts = statements(&result);
    assert_eq!(stmts.len(), 1);
    match result.arena.data(stmts[0]) {
        NodeData::Block(d) => assert_eq!(d.list.nodes.len(), 2),
        other => panic!("expected Block, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseIfStatement
#[test]
fn parse_if_else_statement() {
    let result = parse_ts("if (a) b; else c;");
    let stmts = statements(&result);
    match result.arena.data(stmts[0]) {
        NodeData::IfStatement(d) => {
            assert_eq!(result.arena.text(d.expression), "a");
            assert_eq!(
                result.arena.kind(d.then_statement),
                Kind::ExpressionStatement
            );
            assert!(d.else_statement.is_some());
        }
        other => panic!("expected IfStatement, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseIfStatement (no else)
#[test]
fn parse_if_without_else() {
    let result = parse_ts("if (a) {}");
    let stmts = statements(&result);
    match result.arena.data(stmts[0]) {
        NodeData::IfStatement(d) => assert!(d.else_statement.is_none()),
        other => panic!("expected IfStatement, got {other:?}"),
    }
}

// Go: parser.go:parseWhileStatement
#[test]
fn parse_while_statement() {
    let result = parse_ts("while (a) b;");
    assert_eq!(only_stmt_kind(&result), Kind::WhileStatement);
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseDoStatement
#[test]
fn parse_do_statement() {
    let result = parse_ts("do a; while (b);");
    let stmts = statements(&result);
    match result.arena.data(stmts[0]) {
        NodeData::DoStatement(d) => {
            assert_eq!(result.arena.kind(d.statement), Kind::ExpressionStatement);
            assert_eq!(result.arena.text(d.expression), "b");
        }
        other => panic!("expected DoStatement, got {other:?}"),
    }
}

// Go: parser.go:parseSwitchStatement
#[test]
fn parse_switch_statement() {
    let result = parse_ts("switch (a) { case 1: b; case 2: break; default: c; }");
    let stmts = statements(&result);
    let case_block = match result.arena.data(stmts[0]) {
        NodeData::SwitchStatement(d) => {
            assert_eq!(result.arena.text(d.expression), "a");
            d.case_block
        }
        other => panic!("expected SwitchStatement, got {other:?}"),
    };
    let clauses = match result.arena.data(case_block) {
        NodeData::CaseBlock(d) => d.clauses.nodes.clone(),
        other => panic!("expected CaseBlock, got {other:?}"),
    };
    assert_eq!(clauses.len(), 3);
    assert_eq!(result.arena.kind(clauses[0]), Kind::CaseClause);
    assert_eq!(result.arena.kind(clauses[2]), Kind::DefaultClause);
    // default clause has no expression.
    match result.arena.data(clauses[2]) {
        NodeData::CaseOrDefaultClause(d) => assert!(d.expression.is_none()),
        other => panic!("expected CaseOrDefaultClause, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseThrowStatement
#[test]
fn parse_throw_statement() {
    let result = parse_ts("throw a;");
    let stmts = statements(&result);
    match result.arena.data(stmts[0]) {
        NodeData::ThrowStatement(d) => assert_eq!(result.arena.text(d.expression), "a"),
        other => panic!("expected ThrowStatement, got {other:?}"),
    }
}

// Go: parser.go:parseReturnStatement
#[test]
fn parse_return_statement_with_and_without_value() {
    let with = parse_ts("return a;");
    match with.arena.data(statements(&with)[0]) {
        NodeData::ReturnStatement(d) => assert!(d.expression.is_some()),
        other => panic!("expected ReturnStatement, got {other:?}"),
    }
    let bare = parse_ts("return;");
    match bare.arena.data(statements(&bare)[0]) {
        NodeData::ReturnStatement(d) => assert!(d.expression.is_none()),
        other => panic!("expected ReturnStatement, got {other:?}"),
    }
}

// Go: parser.go:parseBreakStatement / parseContinueStatement
#[test]
fn parse_break_and_continue() {
    let labeled = parse_ts("break outer;");
    match labeled.arena.data(statements(&labeled)[0]) {
        NodeData::BreakStatement(d) => {
            assert_eq!(labeled.arena.text(d.label.unwrap()), "outer");
        }
        other => panic!("expected BreakStatement, got {other:?}"),
    }
    let bare = parse_ts("continue;");
    match bare.arena.data(statements(&bare)[0]) {
        NodeData::ContinueStatement(d) => assert!(d.label.is_none()),
        other => panic!("expected ContinueStatement, got {other:?}"),
    }
}

// Go: parser.go:parseWithStatement
#[test]
fn parse_with_statement() {
    let result = parse_ts("with (a) b;");
    assert_eq!(only_stmt_kind(&result), Kind::WithStatement);
}

// Go: parser.go:parseDebuggerStatement
#[test]
fn parse_debugger_statement() {
    let result = parse_ts("debugger;");
    assert_eq!(only_stmt_kind(&result), Kind::DebuggerStatement);
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseVariableStatement
#[test]
fn parse_var_let_const_statements() {
    for (src, flag) in [
        ("var x = 1;", NodeFlags::NONE),
        ("let y;", NodeFlags::LET),
        ("const z = 2;", NodeFlags::CONST),
    ] {
        let result = parse_ts(src);
        let stmts = statements(&result);
        let list = match result.arena.data(stmts[0]) {
            NodeData::VariableStatement(d) => d.declaration_list,
            other => panic!("{src}: expected VariableStatement, got {other:?}"),
        };
        assert_eq!(result.arena.kind(list), Kind::VariableDeclarationList);
        assert!(
            result.arena.flags(list).contains(flag),
            "{src}: expected flag {flag:?}"
        );
        assert!(result.diagnostics.is_empty(), "{src}: no diagnostics");
    }
}

// Go: parser.go:parseVariableDeclarationWorker (type annotation + initializer)
#[test]
fn parse_var_with_type_annotation() {
    let result = parse_ts("let x: number = 1;");
    let stmts = statements(&result);
    let list = match result.arena.data(stmts[0]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    let decls = match result.arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
        other => panic!("expected VariableDeclarationList, got {other:?}"),
    };
    assert_eq!(decls.len(), 1);
    match result.arena.data(decls[0]) {
        NodeData::VariableDeclaration(d) => {
            assert_eq!(result.arena.text(d.name), "x");
            let ty = d.type_node.expect("type annotation present");
            assert_eq!(result.arena.kind(ty), Kind::NumberKeyword);
            assert!(d.initializer.is_some());
        }
        other => panic!("expected VariableDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseType (type reference with type arguments)
#[test]
fn parse_var_with_type_reference_args() {
    let result = parse_ts("let x: Array<number>;");
    let stmts = statements(&result);
    let list = match result.arena.data(stmts[0]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    let decls = match result.arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
        other => panic!("expected VariableDeclarationList, got {other:?}"),
    };
    let ty = match result.arena.data(decls[0]) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("type present"),
        other => panic!("expected VariableDeclaration, got {other:?}"),
    };
    match result.arena.data(ty) {
        NodeData::TypeReference(d) => {
            assert_eq!(result.arena.text(d.type_name), "Array");
            let args = d.type_arguments.as_ref().expect("type args present");
            assert_eq!(args.nodes.len(), 1);
            assert_eq!(result.arena.kind(args.nodes[0]), Kind::NumberKeyword);
        }
        other => panic!("expected TypeReference, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseType (array + union types)
#[test]
fn parse_array_and_union_types() {
    let arr = parse_ts("let x: number[];");
    let ty = type_of_first_decl(&arr);
    assert_eq!(arr.arena.kind(ty), Kind::ArrayType);

    let uni = parse_ts("let x: A | B;");
    let ty = type_of_first_decl(&uni);
    match uni.arena.data(ty) {
        NodeData::UnionType(d) => assert_eq!(d.types.nodes.len(), 2),
        other => panic!("expected UnionType, got {other:?}"),
    }
}

// Helper: type node of the first variable declaration.
fn type_of_first_decl(result: &ParseResult) -> NodeId {
    let list = match result.arena.data(statements(result)[0]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    let decls = match result.arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
        other => panic!("expected VariableDeclarationList, got {other:?}"),
    };
    match result.arena.data(decls[0]) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("type present"),
        other => panic!("expected VariableDeclaration, got {other:?}"),
    }
}

// Go: parser.go:parseObjectBindingPattern
#[test]
fn parse_object_destructuring() {
    let result = parse_ts("const { a, b: c } = obj;");
    let ty = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    let decls = match result.arena.data(ty) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
        other => panic!("expected list, got {other:?}"),
    };
    let name = match result.arena.data(decls[0]) {
        NodeData::VariableDeclaration(d) => d.name,
        other => panic!("expected decl, got {other:?}"),
    };
    match result.arena.data(name) {
        NodeData::ObjectBindingPattern(d) => assert_eq!(d.elements.nodes.len(), 2),
        other => panic!("expected ObjectBindingPattern, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseArrayBindingPattern
#[test]
fn parse_array_destructuring() {
    let result = parse_ts("const [a, , b] = arr;");
    let name = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => {
            let list = d.declaration_list;
            match result.arena.data(list) {
                NodeData::VariableDeclarationList(l) => {
                    match result.arena.data(l.declarations.nodes[0]) {
                        NodeData::VariableDeclaration(v) => v.name,
                        other => panic!("expected decl, got {other:?}"),
                    }
                }
                other => panic!("expected list, got {other:?}"),
            }
        }
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    match result.arena.data(name) {
        NodeData::ArrayBindingPattern(d) => assert_eq!(d.elements.nodes.len(), 3),
        other => panic!("expected ArrayBindingPattern, got {other:?}"),
    }
}

// Go: parser.go:parseForOrForInOrForOfStatement
#[test]
fn parse_for_statements() {
    assert_eq!(only_stmt_kind(&parse_ts("for (;;) {}")), Kind::ForStatement);
    assert_eq!(
        only_stmt_kind(&parse_ts("for (let i = 0; i < 10; i++) {}")),
        Kind::ForStatement
    );
    assert_eq!(
        only_stmt_kind(&parse_ts("for (const x in obj) {}")),
        Kind::ForInStatement
    );
    assert_eq!(
        only_stmt_kind(&parse_ts("for (const x of arr) {}")),
        Kind::ForOfStatement
    );
}

// Go: parser.go:parseForOrForInOrForOfStatement (for-of fields)
#[test]
fn parse_for_of_fields() {
    let result = parse_ts("for (const x of arr) body;");
    match result.arena.data(statements(&result)[0]) {
        NodeData::ForInOrOfStatement(d) => {
            assert!(d.await_modifier.is_none());
            assert_eq!(
                result.arena.kind(d.initializer),
                Kind::VariableDeclarationList
            );
            assert_eq!(result.arena.text(d.expression), "arr");
        }
        other => panic!("expected ForInOrOfStatement, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseTryStatement / parseCatchClause
#[test]
fn parse_try_catch_finally() {
    let result = parse_ts("try { a; } catch (e) { b; } finally { c; }");
    match result.arena.data(statements(&result)[0]) {
        NodeData::TryStatement(d) => {
            let catch = d.catch_clause.expect("catch present");
            assert_eq!(result.arena.kind(catch), Kind::CatchClause);
            match result.arena.data(catch) {
                NodeData::CatchClause(c) => {
                    assert!(c.variable_declaration.is_some());
                }
                other => panic!("expected CatchClause, got {other:?}"),
            }
            assert!(d.finally_block.is_some());
        }
        other => panic!("expected TryStatement, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseTryStatement (catch with no binding)
#[test]
fn parse_try_catch_no_binding() {
    let result = parse_ts("try {} catch {}");
    match result.arena.data(statements(&result)[0]) {
        NodeData::TryStatement(d) => match result.arena.data(d.catch_clause.unwrap()) {
            NodeData::CatchClause(c) => assert!(c.variable_declaration.is_none()),
            other => panic!("expected CatchClause, got {other:?}"),
        },
        other => panic!("expected TryStatement, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseFunctionDeclaration
#[test]
fn parse_function_declaration_basic() {
    let result = parse_ts("function f(a, b) { return a; }");
    let stmts = statements(&result);
    match result.arena.data(stmts[0]) {
        NodeData::FunctionDeclaration(d) => {
            assert_eq!(result.arena.text(d.name.unwrap()), "f");
            assert_eq!(d.parameters.nodes.len(), 2);
            assert!(d.body.is_some());
            assert!(d.modifiers.is_none());
            assert!(d.type_node.is_none());
        }
        other => panic!("expected FunctionDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseFunctionDeclaration (overload signature, no body)
#[test]
fn parse_function_overload_signature() {
    let result = parse_ts("function f(): void;");
    match result.arena.data(statements(&result)[0]) {
        NodeData::FunctionDeclaration(d) => {
            assert!(d.body.is_none());
            let ret = d.type_node.expect("return type present");
            assert_eq!(result.arena.kind(ret), Kind::VoidKeyword);
        }
        other => panic!("expected FunctionDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseParameterEx (typed + optional + rest + default params)
#[test]
fn parse_function_parameters() {
    let result = parse_ts("function f(a: number, b?: string, c = 1, ...rest) {}");
    let params = match result.arena.data(statements(&result)[0]) {
        NodeData::FunctionDeclaration(d) => d.parameters.nodes.clone(),
        other => panic!("expected FunctionDeclaration, got {other:?}"),
    };
    assert_eq!(params.len(), 4);
    match result.arena.data(params[0]) {
        NodeData::ParameterDeclaration(p) => {
            assert_eq!(result.arena.text(p.name), "a");
            assert_eq!(result.arena.kind(p.type_node.unwrap()), Kind::NumberKeyword);
        }
        other => panic!("expected ParameterDeclaration, got {other:?}"),
    }
    match result.arena.data(params[1]) {
        NodeData::ParameterDeclaration(p) => assert!(p.question_token.is_some()),
        other => panic!("expected ParameterDeclaration, got {other:?}"),
    }
    match result.arena.data(params[2]) {
        NodeData::ParameterDeclaration(p) => assert!(p.initializer.is_some()),
        other => panic!("expected ParameterDeclaration, got {other:?}"),
    }
    match result.arena.data(params[3]) {
        NodeData::ParameterDeclaration(p) => assert!(p.dot_dot_dot_token.is_some()),
        other => panic!("expected ParameterDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseFunctionDeclaration (generics)
#[test]
fn parse_function_generics() {
    let result = parse_ts("function f<T, U extends string>(x: T): U { return x; }");
    match result.arena.data(statements(&result)[0]) {
        NodeData::FunctionDeclaration(d) => {
            let tps = d
                .type_parameters
                .as_ref()
                .expect("type params present")
                .nodes
                .clone();
            assert_eq!(tps.len(), 2);
            match result.arena.data(tps[0]) {
                NodeData::TypeParameterDeclaration(t) => {
                    assert_eq!(result.arena.text(t.name), "T");
                    assert!(t.constraint.is_none());
                }
                other => panic!("expected TypeParameterDeclaration, got {other:?}"),
            }
            match result.arena.data(tps[1]) {
                NodeData::TypeParameterDeclaration(t) => assert!(t.constraint.is_some()),
                other => panic!("expected TypeParameterDeclaration, got {other:?}"),
            }
        }
        other => panic!("expected FunctionDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseDeclaration (modifiers: export/async/declare)
#[test]
fn parse_function_with_modifiers() {
    for (src, flag) in [
        ("export function f() {}", ModifierFlags::EXPORT),
        ("async function f() {}", ModifierFlags::ASYNC),
        ("declare function f(): void;", ModifierFlags::AMBIENT),
    ] {
        let result = parse_ts(src);
        match result.arena.data(statements(&result)[0]) {
            NodeData::FunctionDeclaration(d) => {
                let m = d
                    .modifiers
                    .as_ref()
                    .unwrap_or_else(|| panic!("{src}: modifiers"));
                assert!(m.modifier_flags.contains(flag), "{src}: expected {flag:?}");
            }
            other => panic!("{src}: expected FunctionDeclaration, got {other:?}"),
        }
        assert!(result.diagnostics.is_empty(), "{src}: no diagnostics");
    }
}

// Go: parser.go:parseDeclaration (export const)
#[test]
fn parse_export_const() {
    let result = parse_ts("export const x = 1;");
    match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => {
            let m = d.modifiers.as_ref().expect("export modifier");
            assert!(m.modifier_flags.contains(ModifierFlags::EXPORT));
        }
        other => panic!("expected VariableStatement, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Helper: members of the (single) class declaration.
fn class_members(result: &ParseResult) -> Vec<NodeId> {
    match result.arena.data(statements(result)[0]) {
        NodeData::ClassDeclaration(d) => d.members.nodes.clone(),
        other => panic!("expected ClassDeclaration, got {other:?}"),
    }
}

// Go: parser.go:parseClassDeclarationOrExpression
#[test]
fn parse_empty_class() {
    let result = parse_ts("class C {}");
    match result.arena.data(statements(&result)[0]) {
        NodeData::ClassDeclaration(d) => {
            assert_eq!(result.arena.text(d.name.unwrap()), "C");
            assert_eq!(d.members.nodes.len(), 0);
            assert!(d.heritage_clauses.is_none());
        }
        other => panic!("expected ClassDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseHeritageClauses
#[test]
fn parse_class_heritage() {
    let result = parse_ts("class C extends B implements I, J {}");
    let clauses = match result.arena.data(statements(&result)[0]) {
        NodeData::ClassDeclaration(d) => d
            .heritage_clauses
            .as_ref()
            .expect("heritage clauses")
            .nodes
            .clone(),
        other => panic!("expected ClassDeclaration, got {other:?}"),
    };
    assert_eq!(clauses.len(), 2);
    match result.arena.data(clauses[0]) {
        NodeData::HeritageClause(h) => {
            assert_eq!(h.token, Kind::ExtendsKeyword);
            assert_eq!(h.types.nodes.len(), 1);
        }
        other => panic!("expected HeritageClause, got {other:?}"),
    }
    match result.arena.data(clauses[1]) {
        NodeData::HeritageClause(h) => {
            assert_eq!(h.token, Kind::ImplementsKeyword);
            assert_eq!(h.types.nodes.len(), 2);
        }
        other => panic!("expected HeritageClause, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseClassElement (members)
#[test]
fn parse_class_members() {
    let src = "class C {
        x = 1;
        readonly y: number;
        #p = 2;
        constructor(a: number) {}
        method<T>(b: T): void {}
        get g() { return 1; }
        set s(v) {}
        static {}
        [key: string]: number;
        ;
    }";
    let result = parse_ts(src);
    let members = class_members(&result);
    let kinds: Vec<Kind> = members.iter().map(|&m| result.arena.kind(m)).collect();
    assert!(kinds.contains(&Kind::PropertyDeclaration));
    assert!(kinds.contains(&Kind::Constructor));
    assert!(kinds.contains(&Kind::MethodDeclaration));
    assert!(kinds.contains(&Kind::GetAccessor));
    assert!(kinds.contains(&Kind::SetAccessor));
    assert!(kinds.contains(&Kind::ClassStaticBlockDeclaration));
    assert!(kinds.contains(&Kind::IndexSignature));
    assert!(kinds.contains(&Kind::SemicolonClassElement));
    assert!(
        result.diagnostics.is_empty(),
        "diagnostics: {:?}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseClassElement (modifiers on members)
#[test]
fn parse_class_member_modifiers() {
    let result = parse_ts("class C { public static readonly x = 1; }");
    let members = class_members(&result);
    match result.arena.data(members[0]) {
        NodeData::PropertyDeclaration(d) => {
            let m = d.modifiers.as_ref().expect("modifiers");
            assert!(m.modifier_flags.contains(ModifierFlags::PUBLIC));
            assert!(m.modifier_flags.contains(ModifierFlags::STATIC));
            assert!(m.modifier_flags.contains(ModifierFlags::READONLY));
        }
        other => panic!("expected PropertyDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseClassExpression
#[test]
fn parse_class_expression() {
    let result = parse_ts("const C = class extends B {};");
    // `const C = class ...` => VariableStatement -> ... -> ClassExpression initializer.
    let list = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    let init = match result.arena.data(list) {
        NodeData::VariableDeclarationList(l) => match result.arena.data(l.declarations.nodes[0]) {
            NodeData::VariableDeclaration(v) => v.initializer.expect("initializer"),
            other => panic!("expected decl, got {other:?}"),
        },
        other => panic!("expected list, got {other:?}"),
    };
    assert_eq!(result.arena.kind(init), Kind::ClassExpression);
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseInterfaceDeclaration
#[test]
fn parse_interface_declaration() {
    let result = parse_ts("interface I<T> extends B { a: number; b?(x: T): void; readonly c; }");
    let members = match result.arena.data(statements(&result)[0]) {
        NodeData::InterfaceDeclaration(d) => {
            assert_eq!(result.arena.text(d.name.unwrap()), "I");
            assert!(d.type_parameters.is_some());
            assert!(d.heritage_clauses.is_some());
            d.members.nodes.clone()
        }
        other => panic!("expected InterfaceDeclaration, got {other:?}"),
    };
    let kinds: Vec<Kind> = members.iter().map(|&m| result.arena.kind(m)).collect();
    assert!(kinds.contains(&Kind::PropertySignature));
    assert!(kinds.contains(&Kind::MethodSignature));
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseSignatureMember (call/construct signatures + index)
#[test]
fn parse_interface_signature_members() {
    let result = parse_ts("interface I { (x: number): string; new (): I; [k: string]: number; }");
    let members = match result.arena.data(statements(&result)[0]) {
        NodeData::InterfaceDeclaration(d) => d.members.nodes.clone(),
        other => panic!("expected InterfaceDeclaration, got {other:?}"),
    };
    let kinds: Vec<Kind> = members.iter().map(|&m| result.arena.kind(m)).collect();
    assert!(kinds.contains(&Kind::CallSignature));
    assert!(kinds.contains(&Kind::ConstructSignature));
    assert!(kinds.contains(&Kind::IndexSignature));
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseTypeAliasDeclaration
#[test]
fn parse_type_alias() {
    let result = parse_ts("type Alias<T> = T | number;");
    match result.arena.data(statements(&result)[0]) {
        NodeData::TypeAliasDeclaration(d) => {
            assert_eq!(result.arena.text(d.name), "Alias");
            assert!(d.type_parameters.is_some());
            assert_eq!(result.arena.kind(d.type_node), Kind::UnionType);
        }
        other => panic!("expected TypeAliasDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseTypeAliasDeclaration (object type literal)
#[test]
fn parse_type_alias_object_literal() {
    let result = parse_ts("type T = { a: number; b(): void };");
    let ty = match result.arena.data(statements(&result)[0]) {
        NodeData::TypeAliasDeclaration(d) => d.type_node,
        other => panic!("expected TypeAliasDeclaration, got {other:?}"),
    };
    match result.arena.data(ty) {
        NodeData::TypeLiteral(d) => assert_eq!(d.members.nodes.len(), 2),
        other => panic!("expected TypeLiteral, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseEnumDeclaration
#[test]
fn parse_enum_declaration() {
    let result = parse_ts("enum E { A, B = 1, C }");
    let members = match result.arena.data(statements(&result)[0]) {
        NodeData::EnumDeclaration(d) => {
            assert_eq!(result.arena.text(d.name), "E");
            d.members.nodes.clone()
        }
        other => panic!("expected EnumDeclaration, got {other:?}"),
    };
    assert_eq!(members.len(), 3);
    match result.arena.data(members[0]) {
        NodeData::EnumMember(m) => assert!(m.initializer.is_none()),
        other => panic!("expected EnumMember, got {other:?}"),
    }
    match result.arena.data(members[1]) {
        NodeData::EnumMember(m) => assert!(m.initializer.is_some()),
        other => panic!("expected EnumMember, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseModuleDeclaration
#[test]
fn parse_namespace_declaration() {
    let result = parse_ts("namespace A.B { export const x = 1; }");
    match result.arena.data(statements(&result)[0]) {
        NodeData::ModuleDeclaration(d) => {
            assert_eq!(d.keyword, Kind::NamespaceKeyword);
            assert_eq!(result.arena.text(d.name), "A");
            // body is a nested ModuleDeclaration for `A.B`.
            assert_eq!(result.arena.kind(d.body.unwrap()), Kind::ModuleDeclaration);
        }
        other => panic!("expected ModuleDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseAmbientExternalModuleDeclaration
#[test]
fn parse_ambient_module() {
    let result = parse_ts("declare module \"fs\" { export const x: number; }");
    match result.arena.data(statements(&result)[0]) {
        NodeData::ModuleDeclaration(d) => {
            assert_eq!(result.arena.kind(d.name), Kind::StringLiteral);
            assert_eq!(result.arena.kind(d.body.unwrap()), Kind::ModuleBlock);
        }
        other => panic!("expected ModuleDeclaration, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseImportDeclarationOrImportEqualsDeclaration (forms)
#[test]
fn parse_import_forms() {
    let bare = parse_ts("import \"side-effect\";");
    match bare.arena.data(statements(&bare)[0]) {
        NodeData::ImportDeclaration(d) => assert!(d.import_clause.is_none()),
        other => panic!("expected ImportDeclaration, got {other:?}"),
    }
    let default_import = parse_ts("import x from \"m\";");
    match default_import.arena.data(statements(&default_import)[0]) {
        NodeData::ImportDeclaration(d) => {
            match default_import.arena.data(d.import_clause.unwrap()) {
                NodeData::ImportClause(c) => {
                    assert_eq!(default_import.arena.text(c.name.unwrap()), "x");
                }
                other => panic!("expected ImportClause, got {other:?}"),
            }
        }
        other => panic!("expected ImportDeclaration, got {other:?}"),
    }
    let ns = parse_ts("import * as ns from \"m\";");
    assert_eq!(only_stmt_kind(&ns), Kind::ImportDeclaration);
    let named = parse_ts("import { a, b as c } from \"m\";");
    let clause = match named.arena.data(statements(&named)[0]) {
        NodeData::ImportDeclaration(d) => d.import_clause.unwrap(),
        other => panic!("expected ImportDeclaration, got {other:?}"),
    };
    let bindings = match named.arena.data(clause) {
        NodeData::ImportClause(c) => c.named_bindings.unwrap(),
        other => panic!("expected ImportClause, got {other:?}"),
    };
    match named.arena.data(bindings) {
        NodeData::NamedImports(n) => assert_eq!(n.elements.nodes.len(), 2),
        other => panic!("expected NamedImports, got {other:?}"),
    }
    for r in [&bare, &default_import, &ns, &named] {
        assert!(r.diagnostics.is_empty());
    }
}

// Go: parser.go:parseImportEqualsDeclaration
#[test]
fn parse_import_equals() {
    let req = parse_ts("import fs = require(\"fs\");");
    match req.arena.data(statements(&req)[0]) {
        NodeData::ImportEqualsDeclaration(d) => {
            assert_eq!(req.arena.text(d.name), "fs");
            assert_eq!(
                req.arena.kind(d.module_reference),
                Kind::ExternalModuleReference
            );
        }
        other => panic!("expected ImportEqualsDeclaration, got {other:?}"),
    }
    let alias = parse_ts("import A = B.C;");
    match alias.arena.data(statements(&alias)[0]) {
        NodeData::ImportEqualsDeclaration(d) => {
            assert_eq!(alias.arena.kind(d.module_reference), Kind::QualifiedName);
        }
        other => panic!("expected ImportEqualsDeclaration, got {other:?}"),
    }
    assert!(req.diagnostics.is_empty() && alias.diagnostics.is_empty());
}

// Go: parser.go:parseExportDeclaration / parseExportAssignment
#[test]
fn parse_export_forms() {
    assert_eq!(
        only_stmt_kind(&parse_ts("export { a, b as c };")),
        Kind::ExportDeclaration
    );
    assert_eq!(
        only_stmt_kind(&parse_ts("export { a } from \"m\";")),
        Kind::ExportDeclaration
    );
    assert_eq!(
        only_stmt_kind(&parse_ts("export * from \"m\";")),
        Kind::ExportDeclaration
    );
    assert_eq!(
        only_stmt_kind(&parse_ts("export * as ns from \"m\";")),
        Kind::ExportDeclaration
    );
    // export = expr;
    match parse_ts("export = foo;")
        .arena
        .data(statements(&parse_ts("export = foo;"))[0])
    {
        NodeData::ExportAssignment(d) => assert!(d.is_export_equals),
        other => panic!("expected ExportAssignment, got {other:?}"),
    }
    // export default expr;
    let def = parse_ts("export default 42;");
    match def.arena.data(statements(&def)[0]) {
        NodeData::ExportAssignment(d) => assert!(!d.is_export_equals),
        other => panic!("expected ExportAssignment, got {other:?}"),
    }
    // export as namespace X;
    assert_eq!(
        only_stmt_kind(&parse_ts("export as namespace X;")),
        Kind::NamespaceExportDeclaration
    );
}

// Go: parser.go:parseExportDeclaration (re-export specifier shape)
#[test]
fn parse_export_named_specifiers() {
    let result = parse_ts("export { a, b as c } from \"m\";");
    let clause = match result.arena.data(statements(&result)[0]) {
        NodeData::ExportDeclaration(d) => {
            assert!(d.module_specifier.is_some());
            d.export_clause.unwrap()
        }
        other => panic!("expected ExportDeclaration, got {other:?}"),
    };
    let specs = match result.arena.data(clause) {
        NodeData::NamedExports(n) => n.elements.nodes.clone(),
        other => panic!("expected NamedExports, got {other:?}"),
    };
    assert_eq!(specs.len(), 2);
    match result.arena.data(specs[1]) {
        NodeData::ExportSpecifier(s) => {
            assert!(s.property_name.is_some());
            assert_eq!(result.arena.text(s.name), "c");
        }
        other => panic!("expected ExportSpecifier, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Helper: the expression of the single expression statement.
fn only_expr(result: &ParseResult) -> NodeId {
    match result.arena.data(statements(result)[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    }
}

// Go: parser.go:parseObjectLiteralExpression / parseObjectLiteralElement
#[test]
fn parse_object_literal() {
    // Assignment-RHS form avoids paren-arrow speculation over a method-bearing object.
    let result = parse_ts("x = { a, b: 1, [c]: 2, ...d, m() {}, get g() { return 1; } };");
    let obj = match result.arena.data(only_expr(&result)) {
        NodeData::BinaryExpression(d) => d.right,
        other => panic!("expected BinaryExpression, got {other:?}"),
    };
    let props = match result.arena.data(obj) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
        other => panic!("expected ObjectLiteralExpression, got {other:?}"),
    };
    let kinds: Vec<Kind> = props.iter().map(|&p| result.arena.kind(p)).collect();
    assert!(kinds.contains(&Kind::ShorthandPropertyAssignment));
    assert!(kinds.contains(&Kind::PropertyAssignment));
    assert!(kinds.contains(&Kind::SpreadAssignment));
    assert!(kinds.contains(&Kind::MethodDeclaration));
    assert!(kinds.contains(&Kind::GetAccessor));
    assert!(
        result.diagnostics.is_empty(),
        "diags: {}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseObjectLiteralElement (computed key + shorthand initializer)
#[test]
fn parse_object_literal_computed_and_cover() {
    let result = parse_ts("x = { [k]: v, a = 1 };");
    // RHS of the assignment is the object literal.
    let obj = match result.arena.data(only_expr(&result)) {
        NodeData::BinaryExpression(d) => d.right,
        other => panic!("expected BinaryExpression, got {other:?}"),
    };
    let props = match result.arena.data(obj) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
        other => panic!("expected ObjectLiteralExpression, got {other:?}"),
    };
    match result.arena.data(props[0]) {
        NodeData::PropertyAssignment(p) => {
            assert_eq!(result.arena.kind(p.name), Kind::ComputedPropertyName);
        }
        other => panic!("expected PropertyAssignment, got {other:?}"),
    }
    match result.arena.data(props[1]) {
        NodeData::ShorthandPropertyAssignment(p) => assert!(p.equals_token.is_some()),
        other => panic!("expected ShorthandPropertyAssignment, got {other:?}"),
    }
}

// Go: parser.go:parseFunctionExpression
#[test]
fn parse_function_expression() {
    let result = parse_ts("const f = function* g(a): void {};");
    let init = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => {
            let list = d.declaration_list;
            match result.arena.data(list) {
                NodeData::VariableDeclarationList(l) => {
                    match result.arena.data(l.declarations.nodes[0]) {
                        NodeData::VariableDeclaration(v) => v.initializer.unwrap(),
                        other => panic!("expected decl, got {other:?}"),
                    }
                }
                other => panic!("expected list, got {other:?}"),
            }
        }
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    match result.arena.data(init) {
        NodeData::FunctionExpression(d) => {
            assert!(d.asterisk_token.is_some());
            assert_eq!(result.arena.text(d.name.unwrap()), "g");
        }
        other => panic!("expected FunctionExpression, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseDeleteExpression / parseTypeOfExpression / parseVoidExpression / parseAwaitExpression
#[test]
fn parse_unary_keyword_expressions() {
    for (src, kind) in [
        ("delete a.b;", Kind::DeleteExpression),
        ("typeof x;", Kind::TypeOfExpression),
        ("void 0;", Kind::VoidExpression),
        ("await p;", Kind::AwaitExpression),
    ] {
        let result = parse_ts(src);
        assert_eq!(result.arena.kind(only_expr(&result)), kind, "{src}");
        assert!(result.diagnostics.is_empty(), "{src}: diags");
    }
}

// Go: parser.go:parseYieldExpression
#[test]
fn parse_yield_expression() {
    let result = parse_ts("function* g() { yield x; yield* gen(); }");
    let body = match result.arena.data(statements(&result)[0]) {
        NodeData::FunctionDeclaration(d) => d.body.unwrap(),
        other => panic!("expected FunctionDeclaration, got {other:?}"),
    };
    let stmts = match result.arena.data(body) {
        NodeData::Block(b) => b.list.nodes.clone(),
        other => panic!("expected Block, got {other:?}"),
    };
    let y0 = match result.arena.data(stmts[0]) {
        NodeData::ExpressionStatement(e) => e.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    match result.arena.data(y0) {
        NodeData::YieldExpression(d) => {
            assert!(d.asterisk_token.is_none());
            assert_eq!(result.arena.text(d.expression.unwrap()), "x");
        }
        other => panic!("expected YieldExpression, got {other:?}"),
    }
    let y1 = match result.arena.data(stmts[1]) {
        NodeData::ExpressionStatement(e) => e.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    match result.arena.data(y1) {
        NodeData::YieldExpression(d) => assert!(d.asterisk_token.is_some()),
        other => panic!("expected YieldExpression (delegate), got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseSimpleArrowFunctionExpression
#[test]
fn parse_simple_arrow() {
    let result = parse_ts("x => x + 1;");
    match result.arena.data(only_expr(&result)) {
        NodeData::ArrowFunction(d) => {
            assert_eq!(d.parameters.nodes.len(), 1);
            assert_eq!(result.arena.kind(d.body), Kind::BinaryExpression);
        }
        other => panic!("expected ArrowFunction, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseParenthesizedArrowFunctionExpression
#[test]
fn parse_parenthesized_arrows() {
    for src in [
        "(a, b) => a;",
        "() => {};",
        "(x) => x;",
        "(a: number): string => \"x\";",
    ] {
        let result = parse_ts(src);
        assert_eq!(
            result.arena.kind(only_expr(&result)),
            Kind::ArrowFunction,
            "{src}"
        );
        assert!(result.diagnostics.is_empty(), "{src}: diags");
    }
    // Block body.
    let block = parse_ts("() => {};");
    match block.arena.data(only_expr(&block)) {
        NodeData::ArrowFunction(d) => assert_eq!(block.arena.kind(d.body), Kind::Block),
        other => panic!("expected ArrowFunction, got {other:?}"),
    }
    // Typed parameter + return type.
    let typed = parse_ts("(a: number): string => \"x\";");
    match typed.arena.data(only_expr(&typed)) {
        NodeData::ArrowFunction(d) => {
            assert!(d.type_node.is_some());
            assert_eq!(d.parameters.nodes.len(), 1);
        }
        other => panic!("expected ArrowFunction, got {other:?}"),
    }
}

// Go: parser.go:parseModifiersForArrowFunction / tryParseAsyncSimpleArrowFunctionExpression
#[test]
fn parse_async_arrows() {
    let paren = parse_ts("async (a) => a;");
    match paren.arena.data(only_expr(&paren)) {
        NodeData::ArrowFunction(d) => {
            let m = d.modifiers.as_ref().expect("async modifier");
            assert!(m.modifier_flags.contains(ModifierFlags::ASYNC));
        }
        other => panic!("expected ArrowFunction, got {other:?}"),
    }
    let simple = parse_ts("async x => x;");
    match simple.arena.data(only_expr(&simple)) {
        NodeData::ArrowFunction(d) => {
            assert!(d.modifiers.is_some());
            assert_eq!(d.parameters.nodes.len(), 1);
        }
        other => panic!("expected ArrowFunction (async simple), got {other:?}"),
    }
    assert!(paren.diagnostics.is_empty() && simple.diagnostics.is_empty());
}

// Go: parser.go:parseParenthesizedArrowFunctionExpression (ambiguity → parenthesized)
#[test]
fn parenthesized_not_arrow_regression() {
    // `(x)` must stay a parenthesized expression, not an arrow.
    let result = parse_ts("(x);");
    assert_eq!(
        result.arena.kind(only_expr(&result)),
        Kind::ParenthesizedExpression
    );
    // `(a, b)` is a comma expression, not an arrow.
    let comma = parse_ts("(a, b);");
    match comma.arena.data(only_expr(&comma)) {
        NodeData::ParenthesizedExpression(d) => {
            assert_eq!(comma.arena.kind(d.expression), Kind::BinaryExpression);
        }
        other => panic!("expected ParenthesizedExpression, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty() && comma.diagnostics.is_empty());
}

// Go: parser.go:makeAsExpression / makeSatisfiesExpression
#[test]
fn parse_as_and_satisfies() {
    let as_expr = parse_ts("x as T;");
    match as_expr.arena.data(only_expr(&as_expr)) {
        NodeData::AsExpression(d) => {
            assert_eq!(as_expr.arena.text(d.expression), "x");
            assert_eq!(as_expr.arena.kind(d.type_node), Kind::TypeReference);
        }
        other => panic!("expected AsExpression, got {other:?}"),
    }
    let sat = parse_ts("x satisfies T;");
    assert_eq!(sat.arena.kind(only_expr(&sat)), Kind::SatisfiesExpression);
    assert!(as_expr.diagnostics.is_empty() && sat.diagnostics.is_empty());
}

// Go: parser.go:parseTypeAssertion
#[test]
fn parse_type_assertion() {
    let result = parse_ts("<T>x;");
    match result.arena.data(only_expr(&result)) {
        NodeData::TypeAssertionExpression(d) => {
            assert_eq!(result.arena.kind(d.type_node), Kind::TypeReference);
            assert_eq!(result.arena.text(d.expression), "x");
        }
        other => panic!("expected TypeAssertionExpression, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseNewExpressionOrNewDotTarget
#[test]
fn parse_new_expression() {
    let with_args = parse_ts("new Foo(1, 2);");
    match with_args.arena.data(only_expr(&with_args)) {
        NodeData::NewExpression(d) => {
            assert_eq!(with_args.arena.text(d.expression), "Foo");
            assert_eq!(d.arguments.as_ref().expect("args").nodes.len(), 2);
        }
        other => panic!("expected NewExpression, got {other:?}"),
    }
    let no_args = parse_ts("new Foo;");
    match no_args.arena.data(only_expr(&no_args)) {
        NodeData::NewExpression(d) => assert!(d.arguments.is_none()),
        other => panic!("expected NewExpression, got {other:?}"),
    }
    assert!(with_args.diagnostics.is_empty() && no_args.diagnostics.is_empty());
}

// Go: parser.go:parseNewExpressionOrNewDotTarget (new.target) / parseLeftHandSideExpressionOrHigher (import.meta)
#[test]
fn parse_meta_properties() {
    let nt = parse_ts("new.target;");
    match nt.arena.data(only_expr(&nt)) {
        NodeData::MetaProperty(d) => {
            assert_eq!(d.keyword_token, Kind::NewKeyword);
            assert_eq!(nt.arena.text(d.name), "target");
        }
        other => panic!("expected MetaProperty, got {other:?}"),
    }
    let im = parse_ts("import.meta;");
    match im.arena.data(only_expr(&im)) {
        NodeData::MetaProperty(d) => {
            assert_eq!(d.keyword_token, Kind::ImportKeyword);
            assert_eq!(im.arena.text(d.name), "meta");
        }
        other => panic!("expected MetaProperty, got {other:?}"),
    }
    assert!(nt.diagnostics.is_empty() && im.diagnostics.is_empty());
}

// Dynamic `import("m")` in expression position parses to a `CallExpression`
// whose callee is an `import` keyword expression (kind `ImportKeyword`), with the
// module specifier as its single argument. Before the fix the parser could not
// consume the `import` token in primary-expression position and looped forever.
// Go: parser.go:parseLeftHandSideExpressionOrHigher (import call) / tests.md:parse_dynamic_import_call
#[test]
fn parse_dynamic_import_call() {
    let result = parse_ts("import(\"m\");");
    let call = only_expr(&result);
    match result.arena.data(call) {
        NodeData::CallExpression(d) => {
            assert_eq!(result.arena.kind(d.expression), Kind::ImportKeyword);
            assert_eq!(d.arguments.nodes.len(), 1);
            assert_eq!(result.arena.kind(d.arguments.nodes[0]), Kind::StringLiteral);
        }
        other => panic!("expected CallExpression, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Guard: the `import` look-ahead added for dynamic `import(...)` must not divert
// an import *statement* (`import x from "m"`) into expression parsing. The
// statement form is dispatched as a declaration (its next token is an
// identifier, not `(`/`<`/`.`), while the call form parses as an expression.
// Go: parser.go:scanStartOfDeclaration / parseLeftHandSideExpressionOrHigher
#[test]
fn parse_import_keyword_statement_vs_call() {
    let decl = parse_ts("import x from \"m\";");
    match decl.arena.data(statements(&decl)[0]) {
        NodeData::ImportDeclaration(d) => assert!(d.import_clause.is_some()),
        other => panic!("expected ImportDeclaration, got {other:?}"),
    }
    assert!(decl.diagnostics.is_empty());

    let call = parse_ts("import(\"m\");");
    assert_eq!(call.arena.kind(only_expr(&call)), Kind::CallExpression);
    assert!(call.diagnostics.is_empty());
}

// Go: parser.go:parseMemberExpressionRest (optional chain + non-null)
#[test]
fn parse_optional_chain_and_non_null() {
    let chain = parse_ts("a?.b;");
    let pa = only_expr(&chain);
    match chain.arena.data(pa) {
        NodeData::PropertyAccessExpression(d) => {
            assert!(d.question_dot_token.is_some());
            assert_eq!(chain.arena.text(d.name), "b");
        }
        other => panic!("expected PropertyAccessExpression, got {other:?}"),
    }
    assert!(chain.arena.flags(pa).contains(NodeFlags::OPTIONAL_CHAIN));

    let non_null = parse_ts("a!.b;");
    // `a!` is a NonNullExpression, then `.b` property access on top.
    match non_null.arena.data(only_expr(&non_null)) {
        NodeData::PropertyAccessExpression(d) => {
            assert_eq!(non_null.arena.kind(d.expression), Kind::NonNullExpression);
        }
        other => panic!("expected PropertyAccessExpression, got {other:?}"),
    }

    let opt_call = parse_ts("a?.();");
    match opt_call.arena.data(only_expr(&opt_call)) {
        NodeData::CallExpression(d) => assert!(d.question_dot_token.is_some()),
        other => panic!("expected CallExpression, got {other:?}"),
    }
    assert!(chain.diagnostics.is_empty() && non_null.diagnostics.is_empty());
}

// Go: parser.go:parseTemplateExpression / parseTaggedTemplateRest
#[test]
fn parse_templates() {
    let tmpl = parse_ts("`a${b}c${d}e`;");
    match tmpl.arena.data(only_expr(&tmpl)) {
        NodeData::TemplateExpression(t) => {
            assert_eq!(tmpl.arena.kind(t.head), Kind::TemplateHead);
            assert_eq!(t.template_spans.nodes.len(), 2);
        }
        other => panic!("expected TemplateExpression, got {other:?}"),
    }
    let tagged = parse_ts("tag`x${y}z`;");
    match tagged.arena.data(only_expr(&tagged)) {
        NodeData::TaggedTemplateExpression(t) => {
            assert_eq!(tagged.arena.text(t.tag), "tag");
            assert_eq!(tagged.arena.kind(t.template), Kind::TemplateExpression);
        }
        other => panic!("expected TaggedTemplateExpression, got {other:?}"),
    }
    let no_sub = parse_ts("tag`x`;");
    match no_sub.arena.data(only_expr(&no_sub)) {
        NodeData::TaggedTemplateExpression(t) => {
            assert_eq!(
                no_sub.arena.kind(t.template),
                Kind::NoSubstitutionTemplateLiteral
            );
        }
        other => panic!("expected TaggedTemplateExpression, got {other:?}"),
    }
    assert!(tmpl.diagnostics.is_empty() && tagged.diagnostics.is_empty());
}

// Go: parser.go:parsePrimaryExpression (regex via reScanSlashToken)
#[test]
fn parse_regex_literal() {
    let result = parse_ts("/ab+c/g;");
    assert_eq!(
        result.arena.kind(only_expr(&result)),
        Kind::RegularExpressionLiteral
    );
    assert!(result.diagnostics.is_empty());
}

// Helper: the type node of a single `type X = ...;` alias declaration.
fn alias_type(result: &ParseResult) -> NodeId {
    match result.arena.data(statements(result)[0]) {
        NodeData::TypeAliasDeclaration(d) => d.type_node,
        other => panic!("expected TypeAliasDeclaration, got {other:?}"),
    }
}

// Go: parser.go:parseFunctionOrConstructorType
#[test]
fn parse_function_and_constructor_types() {
    let ft = parse_ts("type F = (a: number, b: string) => boolean;");
    match ft.arena.data(alias_type(&ft)) {
        NodeData::FunctionType(d) => {
            assert_eq!(d.parameters.nodes.len(), 2);
            assert_eq!(ft.arena.kind(d.type_node.unwrap()), Kind::BooleanKeyword);
        }
        other => panic!("expected FunctionType, got {other:?}"),
    }
    let ct = parse_ts("type C = abstract new () => Foo;");
    match ct.arena.data(alias_type(&ct)) {
        NodeData::ConstructorType(d) => assert!(d.modifiers.is_some()),
        other => panic!("expected ConstructorType, got {other:?}"),
    }
    assert!(ft.diagnostics.is_empty() && ct.diagnostics.is_empty());
}

// Go: parser.go:parseType (conditional) / parseInferType
#[test]
fn parse_conditional_and_infer_types() {
    let result = parse_ts("type T<X> = X extends Array<infer U> ? U : never;");
    match result.arena.data(alias_type(&result)) {
        NodeData::ConditionalType(d) => {
            assert_eq!(result.arena.kind(d.check_type), Kind::TypeReference);
            // extends type holds `Array<infer U>` containing an InferType arg.
            assert_eq!(result.arena.kind(d.extends_type), Kind::TypeReference);
            assert_eq!(result.arena.kind(d.false_type), Kind::NeverKeyword);
        }
        other => panic!("expected ConditionalType, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseTypeOperator
#[test]
fn parse_type_operators() {
    for (src, op) in [
        ("type K = keyof T;", Kind::KeyOfKeyword),
        ("type U = unique symbol;", Kind::UniqueKeyword),
    ] {
        let result = parse_ts(src);
        match result.arena.data(alias_type(&result)) {
            NodeData::TypeOperator(d) => assert_eq!(d.operator, op, "{src}"),
            other => panic!("{src}: expected TypeOperator, got {other:?}"),
        }
        assert!(result.diagnostics.is_empty(), "{src}");
    }
    // `readonly T[]` operator over an array type.
    let ro = parse_ts("type R = readonly number[];");
    match ro.arena.data(alias_type(&ro)) {
        NodeData::TypeOperator(d) => {
            assert_eq!(d.operator, Kind::ReadonlyKeyword);
            assert_eq!(ro.arena.kind(d.type_node), Kind::ArrayType);
        }
        other => panic!("expected TypeOperator, got {other:?}"),
    }
}

// Go: parser.go:parseTupleType / parseTupleElementType
#[test]
fn parse_tuple_types() {
    let result = parse_ts("type Tup = [number, ...string[]];");
    match result.arena.data(alias_type(&result)) {
        NodeData::TupleType(d) => {
            assert_eq!(d.types.nodes.len(), 2);
            assert_eq!(result.arena.kind(d.types.nodes[1]), Kind::RestType);
        }
        other => panic!("expected TupleType, got {other:?}"),
    }
    let named = parse_ts("type N = [a: number, b?: string];");
    match named.arena.data(alias_type(&named)) {
        NodeData::TupleType(d) => {
            assert_eq!(named.arena.kind(d.types.nodes[0]), Kind::NamedTupleMember);
            match named.arena.data(d.types.nodes[1]) {
                NodeData::NamedTupleMember(m) => assert!(m.question_token.is_some()),
                other => panic!("expected NamedTupleMember, got {other:?}"),
            }
        }
        other => panic!("expected TupleType, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty() && named.diagnostics.is_empty());
}

// Go: parser.go:parseMappedType
#[test]
fn parse_mapped_type() {
    let result = parse_ts("type M<T> = { readonly [K in keyof T]?: T[K] };");
    match result.arena.data(alias_type(&result)) {
        NodeData::MappedType(d) => {
            assert!(d.readonly_token.is_some());
            assert!(d.question_token.is_some());
            assert_eq!(result.arena.kind(d.type_parameter), Kind::TypeParameter);
        }
        other => panic!("expected MappedType, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseTemplateType
#[test]
fn parse_template_literal_type() {
    let result = parse_ts("type S<T> = `a${T}b${T}c`;");
    match result.arena.data(alias_type(&result)) {
        NodeData::TemplateLiteralType(d) => {
            assert_eq!(result.arena.kind(d.head), Kind::TemplateHead);
            assert_eq!(d.template_spans.nodes.len(), 2);
        }
        other => panic!("expected TemplateLiteralType, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseImportType / parseTypeQuery
#[test]
fn parse_import_and_query_types() {
    let it = parse_ts("type I = import(\"mod\").Foo<number>;");
    match it.arena.data(alias_type(&it)) {
        NodeData::ImportType(d) => {
            assert!(!d.is_type_of);
            assert!(d.qualifier.is_some());
            assert!(d.type_arguments.is_some());
        }
        other => panic!("expected ImportType, got {other:?}"),
    }
    let tq = parse_ts("type Q = typeof x.y;");
    assert_eq!(tq.arena.kind(alias_type(&tq)), Kind::TypeQuery);
    let toi = parse_ts("type TI = typeof import(\"m\");");
    match toi.arena.data(alias_type(&toi)) {
        NodeData::ImportType(d) => assert!(d.is_type_of),
        other => panic!("expected ImportType (typeof), got {other:?}"),
    }
    assert!(it.diagnostics.is_empty() && tq.diagnostics.is_empty());
}

// Go: parser.go:parseTypeOrTypePredicate / parseAssertsTypePredicate / parseThisTypeNode
#[test]
fn parse_type_predicates_and_this() {
    let pred = parse_ts("function f(x): x is string {}");
    let ret = match pred.arena.data(statements(&pred)[0]) {
        NodeData::FunctionDeclaration(d) => d.type_node.unwrap(),
        other => panic!("expected FunctionDeclaration, got {other:?}"),
    };
    match pred.arena.data(ret) {
        NodeData::TypePredicate(d) => {
            assert!(d.asserts_modifier.is_none());
            assert_eq!(pred.arena.text(d.parameter_name), "x");
        }
        other => panic!("expected TypePredicate, got {other:?}"),
    }
    let asserts = parse_ts("function g(x): asserts x is T {}");
    let ret2 = match asserts.arena.data(statements(&asserts)[0]) {
        NodeData::FunctionDeclaration(d) => d.type_node.unwrap(),
        other => panic!("expected FunctionDeclaration, got {other:?}"),
    };
    match asserts.arena.data(ret2) {
        NodeData::TypePredicate(d) => assert!(d.asserts_modifier.is_some()),
        other => panic!("expected TypePredicate (asserts), got {other:?}"),
    }
    // `this` type as a return type.
    let this_t = parse_ts("type T = () => this;");
    match this_t.arena.data(alias_type(&this_t)) {
        NodeData::FunctionType(d) => {
            assert_eq!(this_t.arena.kind(d.type_node.unwrap()), Kind::ThisType)
        }
        other => panic!("expected FunctionType, got {other:?}"),
    }
    assert!(pred.diagnostics.is_empty() && asserts.diagnostics.is_empty());
}

// Go: parser.go:tryParseTypeArgumentsInExpression
#[test]
fn parse_type_arguments_in_call() {
    let result = parse_ts("f<number, string>(42);");
    match result.arena.data(only_expr(&result)) {
        NodeData::CallExpression(d) => {
            assert_eq!(result.arena.text(d.expression), "f");
            assert_eq!(d.type_arguments.as_ref().expect("type args").nodes.len(), 2);
            assert_eq!(d.arguments.nodes.len(), 1);
        }
        other => panic!("expected CallExpression, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
    // `a < b` stays a relational binary expression (no type args).
    let rel = parse_ts("a < b;");
    assert_eq!(rel.arena.kind(only_expr(&rel)), Kind::BinaryExpression);
}

// Go: parser.go:tryParseTypeArgumentsInExpression (instantiation expression)
#[test]
fn parse_instantiation_expression() {
    let result = parse_ts("const g = f<number>;");
    let init = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => {
            let list = d.declaration_list;
            match result.arena.data(list) {
                NodeData::VariableDeclarationList(l) => {
                    match result.arena.data(l.declarations.nodes[0]) {
                        NodeData::VariableDeclaration(v) => v.initializer.unwrap(),
                        other => panic!("got {other:?}"),
                    }
                }
                other => panic!("got {other:?}"),
            }
        }
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    assert_eq!(result.arena.kind(init), Kind::ExpressionWithTypeArguments);
}

// Go: parser.go:tryReparseOptionalChain
#[test]
fn parse_optional_chain_reparse_through_non_null() {
    // `a?.b!.c` — the `.c` access is part of the optional chain via non-null reparse.
    let result = parse_ts("a?.b!.c;");
    let outer = only_expr(&result);
    assert_eq!(result.arena.kind(outer), Kind::PropertyAccessExpression);
    assert!(result
        .arena
        .flags(outer)
        .contains(NodeFlags::OPTIONAL_CHAIN));
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseLiteralTypeNode (negative)
#[test]
fn parse_negative_literal_type() {
    let result = parse_ts("type N = -1;");
    match result.arena.data(alias_type(&result)) {
        NodeData::LiteralType(d) => {
            assert_eq!(result.arena.kind(d.literal), Kind::PrefixUnaryExpression);
        }
        other => panic!("expected LiteralType, got {other:?}"),
    }
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:parseParenthesizedArrowFunctionExpression (paren object literal, not arrow)
#[test]
fn parse_parenthesized_object_literals() {
    for src in [
        "({ a, b: 1 });",
        "({ m() {} });",
        "({ get g() { return 1; } });",
    ] {
        let result = parse_ts(src);
        match result.arena.data(only_expr(&result)) {
            NodeData::ParenthesizedExpression(d) => {
                assert_eq!(
                    result.arena.kind(d.expression),
                    Kind::ObjectLiteralExpression,
                    "{src}"
                );
            }
            other => panic!("{src}: expected ParenthesizedExpression, got {other:?}"),
        }
        assert!(result.diagnostics.is_empty(), "{src}: diags");
    }
}

// Go: parser.go:parseDecorator / parseModifiersEx (decorators)
#[test]
fn parse_class_decorators() {
    let result = parse_ts("@sealed @log('x') class C { @readonly x = 1; m(@inject() a) {} }");
    let class = statements(&result)[0];
    assert_eq!(result.arena.kind(class), Kind::ClassDeclaration);
    let mods = match result.arena.data(class) {
        NodeData::ClassDeclaration(d) => d.modifiers.clone().expect("decorators"),
        other => panic!("expected ClassDeclaration, got {other:?}"),
    };
    let kinds: Vec<Kind> = mods
        .list
        .nodes
        .iter()
        .map(|&n| result.arena.kind(n))
        .collect();
    assert_eq!(kinds.iter().filter(|&&k| k == Kind::Decorator).count(), 2);
    assert!(mods.modifier_flags.contains(ModifierFlags::DECORATOR));
    assert!(
        result.diagnostics.is_empty(),
        "diags: {}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseDecorator (member + parameter decorators)
#[test]
fn parse_member_and_parameter_decorators() {
    let result = parse_ts("class C { @dec method(@p1 a, @p2 b) {} }");
    assert_eq!(
        result.arena.kind(statements(&result)[0]),
        Kind::ClassDeclaration
    );
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:tryParseImportAttributes / parseImportAttributes
#[test]
fn parse_import_with_attributes() {
    let result = parse_ts("import data from \"x.json\" with { type: \"json\" };");
    match result.arena.data(statements(&result)[0]) {
        NodeData::ImportDeclaration(d) => {
            let attrs = d.attributes.expect("attributes");
            match result.arena.data(attrs) {
                NodeData::ImportAttributes(a) => {
                    assert_eq!(a.token, Kind::WithKeyword);
                    assert_eq!(a.attributes.nodes.len(), 1);
                    match result.arena.data(a.attributes.nodes[0]) {
                        NodeData::ImportAttribute(at) => {
                            assert_eq!(result.arena.text(at.name.unwrap()), "type");
                        }
                        other => panic!("expected ImportAttribute, got {other:?}"),
                    }
                }
                other => panic!("expected ImportAttributes, got {other:?}"),
            }
        }
        other => panic!("expected ImportDeclaration, got {other:?}"),
    }
    assert!(
        result.diagnostics.is_empty(),
        "diags: {}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseExportDeclaration (attributes) + import type attributes
#[test]
fn parse_export_and_import_type_attributes() {
    let exp = parse_ts("export { x } from \"m\" with { type: \"json\" };");
    match exp.arena.data(statements(&exp)[0]) {
        NodeData::ExportDeclaration(d) => assert!(d.attributes.is_some()),
        other => panic!("expected ExportDeclaration, got {other:?}"),
    }
    let it = parse_ts("type T = import(\"m\", { with: { type: \"json\" } }).X;");
    match it.arena.data(alias_type(&it)) {
        NodeData::ImportType(d) => assert!(d.attributes.is_some()),
        other => panic!("expected ImportType, got {other:?}"),
    }
    assert!(
        exp.diagnostics.is_empty(),
        "export diags: {}",
        exp.diagnostics.len()
    );
}

// Go: parser.go:parseJsxElementOrSelfClosingElementOrFragment (element + children)
#[test]
fn parse_jsx_element() {
    let result =
        parse_tsx("const x = <div className=\"a\" id={y} {...rest}>hello {world}<br /></div>;");
    let init = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => match result.arena.data(d.declaration_list) {
            NodeData::VariableDeclarationList(l) => {
                match result.arena.data(l.declarations.nodes[0]) {
                    NodeData::VariableDeclaration(v) => v.initializer.unwrap(),
                    other => panic!("got {other:?}"),
                }
            }
            other => panic!("got {other:?}"),
        },
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    let (opening, children) = match result.arena.data(init) {
        NodeData::JsxElement(d) => (d.opening, d.children.nodes.clone()),
        other => panic!("expected JsxElement, got {other:?}"),
    };
    // Opening element + its attributes.
    let attrs = match result.arena.data(opening) {
        NodeData::JsxOpeningElement(o) => match result.arena.data(o.attributes) {
            NodeData::JsxAttributes(a) => a.list.nodes.clone(),
            other => panic!("expected JsxAttributes, got {other:?}"),
        },
        other => panic!("expected JsxOpeningElement, got {other:?}"),
    };
    let attr_kinds: Vec<Kind> = attrs.iter().map(|&a| result.arena.kind(a)).collect();
    assert_eq!(
        attr_kinds
            .iter()
            .filter(|&&k| k == Kind::JsxAttribute)
            .count(),
        2
    );
    assert!(attr_kinds.contains(&Kind::JsxSpreadAttribute));
    // Children: text, expression container, self-closing element.
    let child_kinds: Vec<Kind> = children.iter().map(|&c| result.arena.kind(c)).collect();
    assert!(child_kinds.contains(&Kind::JsxText));
    assert!(child_kinds.contains(&Kind::JsxExpression));
    assert!(child_kinds.contains(&Kind::JsxSelfClosingElement));
    assert!(
        result.diagnostics.is_empty(),
        "diags: {}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseJsxOpeningOrSelfClosingElementOrOpeningFragment (fragment)
#[test]
fn parse_jsx_fragment() {
    let result = parse_tsx("const x = <><span>a</span>{b}</>;");
    let init = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => match result.arena.data(d.declaration_list) {
            NodeData::VariableDeclarationList(l) => {
                match result.arena.data(l.declarations.nodes[0]) {
                    NodeData::VariableDeclaration(v) => v.initializer.unwrap(),
                    other => panic!("got {other:?}"),
                }
            }
            other => panic!("got {other:?}"),
        },
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    match result.arena.data(init) {
        NodeData::JsxFragment(d) => {
            assert_eq!(result.arena.kind(d.opening), Kind::JsxOpeningFragment);
            assert_eq!(result.arena.kind(d.closing), Kind::JsxClosingFragment);
            assert_eq!(d.children.nodes.len(), 2);
        }
        other => panic!("expected JsxFragment, got {other:?}"),
    }
    assert!(
        result.diagnostics.is_empty(),
        "diags: {}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseJsxElementName (member + namespaced) / parseJsxTagName
#[test]
fn parse_jsx_self_closing_and_namespaced() {
    let result = parse_tsx("const x = <Foo.Bar.Baz<number> a:b=\"c\" />;");
    let init = match result.arena.data(statements(&result)[0]) {
        NodeData::VariableStatement(d) => match result.arena.data(d.declaration_list) {
            NodeData::VariableDeclarationList(l) => {
                match result.arena.data(l.declarations.nodes[0]) {
                    NodeData::VariableDeclaration(v) => v.initializer.unwrap(),
                    other => panic!("got {other:?}"),
                }
            }
            other => panic!("got {other:?}"),
        },
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    match result.arena.data(init) {
        NodeData::JsxSelfClosingElement(d) => {
            assert_eq!(
                result.arena.kind(d.tag_name),
                Kind::PropertyAccessExpression
            );
            assert!(d.type_arguments.is_some());
            match result.arena.data(d.attributes) {
                NodeData::JsxAttributes(a) => {
                    assert_eq!(result.arena.kind(a.list.nodes[0]), Kind::JsxAttribute);
                    match result.arena.data(a.list.nodes[0]) {
                        NodeData::JsxAttribute(at) => {
                            assert_eq!(result.arena.kind(at.name), Kind::JsxNamespacedName);
                        }
                        other => panic!("got {other:?}"),
                    }
                }
                other => panic!("got {other:?}"),
            }
        }
        other => panic!("expected JsxSelfClosingElement, got {other:?}"),
    }
    assert!(
        result.diagnostics.is_empty(),
        "diags: {}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseJSONText / validateJsonValue
#[test]
fn parse_json_object() {
    let result = parse_json("{ \"a\": 1, \"b\": [true, null, -2], \"c\": \"x\" }");
    let expr = match result.arena.data(statements(&result)[0]) {
        NodeData::ExpressionStatement(e) => e.expression,
        other => panic!("expected ExpressionStatement, got {other:?}"),
    };
    assert_eq!(result.arena.kind(expr), Kind::ObjectLiteralExpression);
    assert!(
        result.diagnostics.is_empty(),
        "diags: {}",
        result.diagnostics.len()
    );
}

// Go: parser.go:parseJSONText (top-level array / literal)
#[test]
fn parse_json_array_and_literals() {
    let arr = parse_json("[1, 2, 3]");
    match arr.arena.data(statements(&arr)[0]) {
        NodeData::ExpressionStatement(e) => {
            assert_eq!(arr.arena.kind(e.expression), Kind::ArrayLiteralExpression)
        }
        other => panic!("got {other:?}"),
    }
    assert!(arr.diagnostics.is_empty());
    let num = parse_json("42");
    match num.arena.data(statements(&num)[0]) {
        NodeData::ExpressionStatement(e) => {
            assert_eq!(num.arena.kind(e.expression), Kind::NumericLiteral)
        }
        other => panic!("got {other:?}"),
    }
    assert!(num.diagnostics.is_empty());
}

// Go: parser.go:validateJsonObjectLiteral / validateJsonValue (errors)
#[test]
fn parse_json_validation_errors() {
    // Single-quoted key + unquoted key both flagged.
    let single = parse_json("{ 'a': 1 }");
    assert!(
        !single.diagnostics.is_empty(),
        "single-quote key should error"
    );
    let unquoted = parse_json("{ a: 1 }");
    assert!(
        !unquoted.diagnostics.is_empty(),
        "unquoted key should error"
    );
    // A function call is not a valid JSON value.
    let bad_value = parse_json("{ \"a\": f() }");
    assert!(
        !bad_value.diagnostics.is_empty(),
        "call expression value should error"
    );
}

// Helper: SourceFile metadata (external module indicator + imports).
fn source_file_data(result: &ParseResult) -> (bool, Vec<String>) {
    match result.arena.data(result.source_file) {
        NodeData::SourceFile(d) => {
            let imports = d
                .imports
                .iter()
                .map(|&i| result.arena.text(i).to_string())
                .collect();
            (d.external_module_indicator.is_some(), imports)
        }
        other => panic!("expected SourceFile, got {other:?}"),
    }
}

// Go: parser.go:finishSourceFile / references.go:collectExternalModuleReferences
#[test]
fn collect_imports_and_external_module_indicator() {
    let result = parse_ts("import x from \"m\"; export { a } from \"n\"; import \"side\";");
    let (is_external, imports) = source_file_data(&result);
    assert!(
        is_external,
        "file with imports/exports is an external module"
    );
    assert_eq!(imports, vec!["m", "n", "side"]);
    assert!(result.diagnostics.is_empty());
}

// Go: parser.go:isAnExternalModuleIndicatorNode (export modifier)
#[test]
fn external_module_indicator_export_modifier() {
    let exported = parse_ts("export const x = 1;");
    assert!(
        source_file_data(&exported).0,
        "export modifier marks external module"
    );
    // A plain script has no external module indicator and no imports.
    let script = parse_ts("const x = 1; function f() {}");
    let (is_external, imports) = source_file_data(&script);
    assert!(!is_external, "plain script is not an external module");
    assert!(imports.is_empty());
    // `import x = require("m")` is an external module indicator and import.
    let import_equals = parse_ts("import fs = require(\"fs\");");
    let (is_external2, imports2) = source_file_data(&import_equals);
    assert!(is_external2);
    assert_eq!(imports2, vec!["fs"]);
}

// JSDoc comments are scanned as trivia and skipped (the JSDoc/reparser semantic
// pass is deferred). This documents that JSDoc does not break parsing.
// Go: internal/parser/jsdoc.go (DEFER full JSDoc attach/reparse)
#[test]
fn jsdoc_comments_are_skipped_as_trivia() {
    let result = parse_source_file(
        SourceFileParseOptions::default(),
        "/** @type {number} */\nconst x = 1;\n/** @param {string} a */\nfunction f(a) {}",
        ScriptKind::Js,
    );
    assert_eq!(statements(&result).len(), 2);
    assert_eq!(
        result.arena.kind(statements(&result)[0]),
        Kind::VariableStatement
    );
    assert_eq!(
        result.arena.kind(statements(&result)[1]),
        Kind::FunctionDeclaration
    );
    assert!(
        result.diagnostics.is_empty(),
        "JSDoc trivia must not produce diagnostics"
    );
}

// Go: parser.go:parseExpressionOrLabeledStatement (labeled)
#[test]
fn parse_labeled_statement() {
    let result = parse_ts("outer: a;");
    let stmts = statements(&result);
    match result.arena.data(stmts[0]) {
        NodeData::LabeledStatement(d) => {
            assert_eq!(result.arena.text(d.label), "outer");
            assert_eq!(result.arena.kind(d.statement), Kind::ExpressionStatement);
        }
        other => panic!("expected LabeledStatement, got {other:?}"),
    }
}
