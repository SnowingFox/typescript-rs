use crate::test_support::check_emit;

// Go: internal/printer/printer_test.go:TestEmit/EmptyStatement
#[test]
fn empty_statement() {
    check_emit(";", ";", false);
}

// Go: internal/printer/printer_test.go:TestEmit/Block
#[test]
fn block() {
    check_emit("{}", "{ }", false);
}

// Go: internal/printer/printer_test.go:TestEmit/VariableStatement
// NOTE: the `using` / `await using` cases (Go `using a = b` / `await using a = b`)
// are DEFERRED: the `tsgo_parser` crate does not yet parse `using` declarations
// (it reports "';' expected"). The printer's `using`/`await using` keyword paths
// are implemented (see `emit_variable_declaration_list`); re-enable when the
// parser supports them.
#[test]
fn variable_statement() {
    check_emit("var a", "var a;", false);
    check_emit("let a", "let a;", false);
    check_emit("const a = b", "const a = b;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/IfStatement
#[test]
fn if_statement() {
    let cases = [
        ("if(a);", "if (a)\n    ;"),
        ("if(a);else;", "if (a)\n    ;\nelse\n    ;"),
        ("if(a);else{}", "if (a)\n    ;\nelse { }"),
        ("if(a);else if(b);", "if (a)\n    ;\nelse if (b)\n    ;"),
        ("if(a);else if(b) {}", "if (a)\n    ;\nelse if (b) { }"),
        ("if(a) {}", "if (a) { }"),
        ("if(a) {} else;", "if (a) { }\nelse\n    ;"),
        ("if(a) {} else {}", "if (a) { }\nelse { }"),
        ("if(a) {} else if(b);", "if (a) { }\nelse if (b)\n    ;"),
        ("if(a) {} else if(b){}", "if (a) { }\nelse if (b) { }"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/DoStatement
#[test]
fn do_statement() {
    check_emit("do;while(a);", "do\n    ;\nwhile (a);", false);
    check_emit("do {} while(a);", "do { } while (a);", false);
}

// Go: internal/printer/printer_test.go:TestEmit/WhileStatement
#[test]
fn while_statement() {
    check_emit("while(a);", "while (a)\n    ;", false);
    check_emit("while(a) {}", "while (a) { }", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ForStatement
#[test]
fn for_statement() {
    let cases = [
        ("for(;;);", "for (;;)\n    ;"),
        ("for(a;;);", "for (a;;)\n    ;"),
        ("for(var a;;);", "for (var a;;)\n    ;"),
        ("for(;a;);", "for (; a;)\n    ;"),
        ("for(;;a);", "for (;; a)\n    ;"),
        ("for(;;){}", "for (;;) { }"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/ForInStatement
#[test]
fn for_in_statement() {
    check_emit("for(a in b);", "for (a in b)\n    ;", false);
    check_emit("for(var a in b);", "for (var a in b)\n    ;", false);
    check_emit("for(a in b){}", "for (a in b) { }", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ForOfStatement
#[test]
fn for_of_statement() {
    let cases = [
        ("for(a of b);", "for (a of b)\n    ;"),
        ("for(var a of b);", "for (var a of b)\n    ;"),
        ("for(a of b){}", "for (a of b) { }"),
        ("for await(a of b);", "for await (a of b)\n    ;"),
        ("for await(var a of b);", "for await (var a of b)\n    ;"),
        ("for await(a of b){}", "for await (a of b) { }"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/{Continue,Break,Return,With} statements
#[test]
fn continue_break_return_with() {
    check_emit("while(1){continue}", "while (1) {\n    continue;\n}", false);
    check_emit(
        "a:while(1){continue a}",
        "a: while (1) {\n    continue a;\n}",
        false,
    );
    check_emit("while(1){break}", "while (1) {\n    break;\n}", false);
    check_emit(
        "a:while(1){break a}",
        "a: while (1) {\n    break a;\n}",
        false,
    );
    check_emit("function f(){return}", "function f() { return; }", false);
    check_emit(
        "function f(){return a}",
        "function f() { return a; }",
        false,
    );
    check_emit("with(a);", "with (a)\n    ;", false);
    check_emit("with(a){}", "with (a) { }", false);
}

// Go: internal/printer/printer_test.go:TestEmit/SwitchStatement + Case/Default clauses
#[test]
fn switch_statement() {
    check_emit("switch (a) {}", "switch (a) {\n}", false);
    check_emit(
        "switch (a) {case b:}",
        "switch (a) {\n    case b:\n}",
        false,
    );
    check_emit(
        "switch (a) {case b:;}",
        "switch (a) {\n    case b: ;\n}",
        false,
    );
    check_emit(
        "switch (a) {default:}",
        "switch (a) {\n    default:\n}",
        false,
    );
    check_emit(
        "switch (a) {default:;}",
        "switch (a) {\n    default: ;\n}",
        false,
    );
}

// Go: internal/printer/printer_test.go:TestEmit/LabeledStatement + ThrowStatement
#[test]
fn labeled_and_throw() {
    check_emit("a:;", "a: ;", false);
    check_emit("throw a", "throw a;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/TryStatement
#[test]
fn try_statement() {
    check_emit("try {} catch {}", "try { }\ncatch { }", false);
    check_emit("try {} finally {}", "try { }\nfinally { }", false);
    check_emit(
        "try {} catch {} finally {}",
        "try { }\ncatch { }\nfinally { }",
        false,
    );
}

// Go: internal/printer/printer_test.go:TestEmit/DebuggerStatement
#[test]
fn debugger_statement() {
    check_emit("debugger", "debugger;", false);
}

// Go: internal/printer/printer.go:emitNotEmittedStatement (emits nothing)
#[test]
fn not_emitted_statement_emits_nothing() {
    use crate::test_support::check_synthetic;
    use tsgo_ast::{Kind, NodeArena, NodeList};
    use tsgo_core::languagevariant::LanguageVariant;
    use tsgo_core::scriptkind::ScriptKind;

    let mut arena = NodeArena::new();
    let elided = arena.new_not_emitted_statement();
    let eof = arena.new_token(Kind::EndOfFile);
    let sf = arena.new_source_file(
        "/file.ts",
        ScriptKind::Ts,
        LanguageVariant::Standard,
        NodeList::new(vec![elided]),
        eof,
    );
    check_synthetic(arena, sf, "");
}
