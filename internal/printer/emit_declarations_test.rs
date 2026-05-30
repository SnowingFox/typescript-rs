use crate::test_support::check_emit;

// Go: internal/printer/printer_test.go:TestEmit/ClassExpression#1
#[test]
fn class_expression_empty() {
    check_emit("(class {})", "(class {\n});", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ClassExpression#2
#[test]
fn class_expression_named() {
    check_emit("(class a {})", "(class a {\n});", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ClassExpression#5
#[test]
fn class_expression_extends() {
    check_emit("(class extends b {})", "(class extends b {\n});", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ClassExpression (remaining)
#[test]
fn class_expression_rest() {
    let cases = [
        ("(class<T>{})", "(class<T> {\n});"),
        ("(class a<T>{})", "(class a<T> {\n});"),
        ("(class a extends b {})", "(class a extends b {\n});"),
        ("(class implements b {})", "(class implements b {\n});"),
        ("(class a implements b {})", "(class a implements b {\n});"),
        (
            "(class implements b, c {})",
            "(class implements b, c {\n});",
        ),
        (
            "(class a implements b, c {})",
            "(class a implements b, c {\n});",
        ),
        (
            "(class extends b implements c, d {})",
            "(class extends b implements c, d {\n});",
        ),
        (
            "(class a extends b implements c, d {})",
            "(class a extends b implements c, d {\n});",
        ),
        ("(@a class {})", "(\n@a\nclass {\n});"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/FunctionDeclaration
#[test]
fn function_declaration() {
    let cases = [
        (
            "export default function(){}",
            "export default function () { }",
        ),
        ("function f(){}", "function f() { }"),
        ("function*f(){}", "function* f() { }"),
        ("async function f(){}", "async function f() { }"),
        ("async function*f(){}", "async function* f() { }"),
        ("function f<T>(){}", "function f<T>() { }"),
        ("function f(a){}", "function f(a) { }"),
        ("function f():T{}", "function f(): T { }"),
        ("function f();", "function f();"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/ClassDeclaration
#[test]
fn class_declaration() {
    let cases = [
        ("class a {}", "class a {\n}"),
        ("class a<T>{}", "class a<T> {\n}"),
        ("class a extends b {}", "class a extends b {\n}"),
        ("class a implements b {}", "class a implements b {\n}"),
        ("class a implements b, c {}", "class a implements b, c {\n}"),
        (
            "class a extends b implements c, d {}",
            "class a extends b implements c, d {\n}",
        ),
        ("export default class {}", "export default class {\n}"),
        ("export default class<T>{}", "export default class<T> {\n}"),
        (
            "export default class extends b {}",
            "export default class extends b {\n}",
        ),
        ("@a class b {}", "@a\nclass b {\n}"),
        ("@a export class b {}", "@a\nexport class b {\n}"),
        ("export @a class b {}", "export \n@a\nclass b {\n}"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/InterfaceDeclaration
#[test]
fn interface_declaration() {
    let cases = [
        ("interface a {}", "interface a {\n}"),
        ("interface a<T>{}", "interface a<T> {\n}"),
        ("interface a extends b {}", "interface a extends b {\n}"),
        (
            "interface a extends b, c {}",
            "interface a extends b, c {\n}",
        ),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/TypeAliasDeclaration
#[test]
fn type_alias_declaration() {
    check_emit("type a = b", "type a = b;", false);
    check_emit("type a<T> = b", "type a<T> = b;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/EnumDeclaration
#[test]
fn enum_declaration() {
    check_emit("enum a{}", "enum a {\n}", false);
    check_emit("enum a{b}", "enum a {\n    b\n}", false);
    check_emit("enum a{b=c}", "enum a {\n    b = c\n}", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ModuleDeclaration
#[test]
fn module_declaration() {
    let cases = [
        ("module a{}", "module a { }"),
        ("module a.b{}", "module a.b { }"),
        ("module \"a\";", "module \"a\";"),
        ("module \"a\"{}", "module \"a\" { }"),
        ("namespace a{}", "namespace a { }"),
        ("namespace a.b{}", "namespace a.b { }"),
        ("global;", "global;"),
        // DEFER: `tsgo_parser` rejects `global{}` (a `global` augmentation block);
        // the printer's global-keyword path is covered by `global;`.
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/ImportEqualsDeclaration
#[test]
fn import_equals_declaration() {
    let cases = [
        ("import a = b", "import a = b;"),
        ("import a = b.c", "import a = b.c;"),
        ("import a = require(\"b\")", "import a = require(\"b\");"),
        ("export import a = b", "export import a = b;"),
        (
            "export import a = require(\"b\")",
            "export import a = require(\"b\");",
        ),
        ("import type a = b", "import type a = b;"),
        ("import type a = b.c", "import type a = b.c;"),
        (
            "import type a = require(\"b\")",
            "import type a = require(\"b\");",
        ),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/ImportDeclaration
#[test]
fn import_declaration() {
    let cases = [
        ("import \"a\"", "import \"a\";"),
        ("import a from \"b\"", "import a from \"b\";"),
        ("import type a from \"b\"", "import type a from \"b\";"),
        ("import * as a from \"b\"", "import * as a from \"b\";"),
        (
            "import type * as a from \"b\"",
            "import type * as a from \"b\";",
        ),
        ("import {} from \"b\"", "import {} from \"b\";"),
        ("import type {} from \"b\"", "import type {} from \"b\";"),
        ("import { a } from \"b\"", "import { a } from \"b\";"),
        (
            "import type { a } from \"b\"",
            "import type { a } from \"b\";",
        ),
        (
            "import { a as b } from \"c\"",
            "import { a as b } from \"c\";",
        ),
        (
            "import { \"a\" as b } from \"c\"",
            "import { \"a\" as b } from \"c\";",
        ),
        ("import a, {} from \"b\"", "import a, {} from \"b\";"),
        (
            "import a, * as b from \"c\"",
            "import a, * as b from \"c\";",
        ),
        (
            "import {} from \"a\" with {}",
            "import {} from \"a\" with {};",
        ),
        (
            "import {} from \"a\" with { b: \"c\" }",
            "import {} from \"a\" with { b: \"c\" };",
        ),
        (
            "import {} from \"a\" with { \"b\": \"c\" }",
            "import {} from \"a\" with { \"b\": \"c\" };",
        ),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/{ExportAssignment,NamespaceExportDeclaration}
#[test]
fn export_assignment_and_namespace() {
    check_emit("export = a", "export = a;", false);
    check_emit("export default a", "export default a;", false);
    check_emit("export as namespace a", "export as namespace a;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ExportDeclaration
#[test]
fn export_declaration() {
    let cases = [
        ("export * from \"a\"", "export * from \"a\";"),
        ("export type * from \"a\"", "export type * from \"a\";"),
        ("export * as a from \"b\"", "export * as a from \"b\";"),
        (
            "export type * as a from \"b\"",
            "export type * as a from \"b\";",
        ),
        ("export { } from \"a\"", "export {} from \"a\";"),
        ("export type { } from \"a\"", "export type {} from \"a\";"),
        ("export { a } from \"b\"", "export { a } from \"b\";"),
        (
            "export { type a } from \"b\"",
            "export { type a } from \"b\";",
        ),
        (
            "export { a as b } from \"c\"",
            "export { a as b } from \"c\";",
        ),
        (
            "export { a as \"b\" } from \"c\"",
            "export { a as \"b\" } from \"c\";",
        ),
        (
            "export { \"a\" } from \"b\"",
            "export { \"a\" } from \"b\";",
        ),
        (
            "export { \"a\" as b } from \"c\"",
            "export { \"a\" as b } from \"c\";",
        ),
        ("export { }", "export {};"),
        ("export { a }", "export { a };"),
        ("export { type a }", "export { type a };"),
        ("export { a as b }", "export { a as b };"),
        (
            "export {} from \"a\" with {}",
            "export {} from \"a\" with {};",
        ),
        (
            "export {} from \"a\" with { b: \"c\" }",
            "export {} from \"a\" with { b: \"c\" };",
        ),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/PropertyDeclaration (class members)
#[test]
fn class_members() {
    let cases = [
        ("class C {a}", "class C {\n    a;\n}"),
        ("class C {readonly a}", "class C {\n    readonly a;\n}"),
        ("class C {static a}", "class C {\n    static a;\n}"),
        ("class C {\"a\"}", "class C {\n    \"a\";\n}"),
        ("class C {0}", "class C {\n    0;\n}"),
        ("class C {[a]}", "class C {\n    [a];\n}"),
        ("class C {#a}", "class C {\n    #a;\n}"),
        ("class C {a?}", "class C {\n    a?;\n}"),
        ("class C {a!}", "class C {\n    a!;\n}"),
        ("class C {a: b}", "class C {\n    a: b;\n}"),
        ("class C {a = b}", "class C {\n    a = b;\n}"),
        ("class C {a() {} }", "class C {\n    a() { }\n}"),
        (
            "class C {static a() {} }",
            "class C {\n    static a() { }\n}",
        ),
        ("class C {async a() {} }", "class C {\n    async a() { }\n}"),
        ("class C {get a() {} }", "class C {\n    get a() { }\n}"),
        ("class C {set a(v) {} }", "class C {\n    set a(v) { }\n}"),
        (
            "class C {constructor() {} }",
            "class C {\n    constructor() { }\n}",
        ),
        ("class C {static { }}", "class C {\n    static { }\n}"),
        ("class C {;}", "class C {\n    ;\n}"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/{PropertySignature,MethodSignature,...} (interface members)
#[test]
fn interface_members() {
    let cases = [
        ("interface I {a}", "interface I {\n    a;\n}"),
        (
            "interface I {readonly a}",
            "interface I {\n    readonly a;\n}",
        ),
        ("interface I {a?}", "interface I {\n    a?;\n}"),
        ("interface I {a: b}", "interface I {\n    a: b;\n}"),
        ("interface I {a()}", "interface I {\n    a();\n}"),
        ("interface I {a(): b}", "interface I {\n    a(): b;\n}"),
        ("interface I {()}", "interface I {\n    ();\n}"),
        ("interface I {new ()}", "interface I {\n    new ();\n}"),
        (
            "interface I {[a: b]: c}",
            "interface I {\n    [a: b]: c;\n}",
        ),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/ParameterDeclaration
#[test]
fn parameters_and_type_parameters() {
    let cases = [
        ("function f(a){}", "function f(a) { }"),
        ("function f(a: b){}", "function f(a: b) { }"),
        ("function f(a = b){}", "function f(a = b) { }"),
        ("function f(a?){}", "function f(a?) { }"),
        ("function f(...a){}", "function f(...a) { }"),
        ("function f({a}){}", "function f({ a }) { }"),
        ("function f([a]){}", "function f([a]) { }"),
        (
            "function f<T extends U>(){}",
            "function f<T extends U>() { }",
        ),
        ("function f<T = U>(){}", "function f<T = U>() { }"),
        ("function f<T, U>(){}", "function f<T, U>() { }"),
    ];
    for (i, o) in cases {
        check_emit(i, o, false);
    }
}
