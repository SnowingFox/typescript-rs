use super::*;
use tsgo_ast::NodeArena;

// Parser-backed port of Go `TestDeepCloneNodeSanityCheck`. The Go test lives in
// `internal/ast/deepclone_test.go` but depends on `parsetestutil.ParseTypeScript`
// (the parser), so its real-parse form is hosted here, where the parser is
// available. It covers the representative node kinds the current parser slice
// produces; JSX / declaration / type clusters land as those productions are
// ported.
// Go: internal/ast/deepclone_test.go:TestDeepCloneNodeSanityCheck

// Go: internal/ast/deepclone_test.go:getChildren
fn get_children(arena: &mut NodeArena, id: NodeId) -> Vec<NodeId> {
    arena.get_children(id)
}

// BFS the original/clone in lockstep: every pair must have distinct ids and the
// same number of children, recursively.
// Go: internal/ast/deepclone_test.go:TestDeepCloneNodeSanityCheck (assertion)
fn assert_deep_clone_structure(arena: &mut NodeArena, original: NodeId, clone: NodeId) {
    let mut work = vec![(original, clone)];
    while let Some((o, c)) = work.pop() {
        assert_ne!(o, c, "clone must produce a distinct node id");
        let oc = get_children(arena, o);
        let cc = get_children(arena, c);
        assert_eq!(oc.len(), cc.len(), "child counts must match");
        for (oi, ci) in oc.into_iter().zip(cc.into_iter()) {
            work.push((oi, ci));
        }
    }
}

#[test]
fn deep_clone_node_sanity_check() {
    // (title, input) — a representative subset of the Go table covering the
    // node kinds the parser currently produces.
    // Go: internal/ast/deepclone_test.go:TestDeepCloneNodeSanityCheck (data)
    let cases: &[(&str, &str)] = &[
        ("StringLiteral#1", ";\"test\""),
        ("StringLiteral#2", ";'test'"),
        ("NumericLiteral", "0"),
        ("BigIntLiteral", "0n"),
        ("BooleanLiteral#1", "true"),
        ("BooleanLiteral#2", "false"),
        ("NullLiteral", "null"),
        ("ThisExpression", "this"),
        ("EmptyStatement", ";"),
        ("PropertyAccess#1", "a.b"),
        ("PropertyAccess#2", "a.b.c"),
        ("ElementAccess#1", "a[b]"),
        ("CallExpression#1", "a()"),
        ("CallExpression#2", "a(b)"),
        ("CallExpression#3", "a(b, c)"),
        ("BinaryExpression#1", "a + b"),
        ("BinaryExpression#2", "a * b"),
        ("BinaryExpression#3", "a, b"),
        ("ConditionalExpression", "a ? b : c"),
        ("PrefixUnaryExpression#1", "-a"),
        ("PrefixUnaryExpression#2", "!a"),
        ("PostfixUnaryExpression#1", "a++"),
        ("PostfixUnaryExpression#2", "a--"),
        ("ParenthesizedExpression", "(a)"),
        ("ArrayLiteralExpression#1", "[]"),
        ("ArrayLiteralExpression#2", "[a]"),
        ("ArrayLiteralExpression#3", "[a, b]"),
        ("SpreadElement", "[...a]"),
        // Statements (round 2).
        ("Block", "{ a; b; }"),
        ("IfStatement", "if (a) b; else c;"),
        ("WhileStatement", "while (a) b;"),
        ("DoStatement", "do a; while (b);"),
        ("ForStatement", "for (let i = 0; i < 10; i++) {}"),
        ("ForInStatement", "for (const k in obj) {}"),
        ("ForOfStatement", "for (const v of arr) {}"),
        (
            "SwitchStatement",
            "switch (a) { case 1: b; break; default: c; }",
        ),
        ("TryStatement", "try { a; } catch (e) { b; } finally { c; }"),
        ("ThrowStatement", "throw e;"),
        ("ReturnLabeledBreak", "outer: { break outer; }"),
        ("WithStatement", "with (a) b;"),
        ("DebuggerStatement", "debugger;"),
        // Variables + binding patterns.
        ("VariableStatement", "let x: number = 1, y;"),
        ("ObjectBinding", "const { a, b: c } = obj;"),
        ("ArrayBinding", "const [a, , b] = arr;"),
        // Declarations.
        (
            "FunctionDeclaration",
            "function f<T>(a: number, ...b): T {}",
        ),
        (
            "ClassDeclaration",
            "class C extends B implements I { x = 1; m() {} get g() { return 1; } static {} }",
        ),
        (
            "InterfaceDeclaration",
            "interface I<T> extends B { a: number; m(): void; }",
        ),
        ("TypeAlias", "type A<T> = T | number;"),
        ("EnumDeclaration", "enum E { A, B = 1 }"),
        ("ModuleDeclaration", "namespace A.B { const x = 1; }"),
        ("ImportDeclaration", "import x, { a, b as c } from \"m\";"),
        ("ImportEquals", "import fs = require(\"fs\");"),
        ("ExportDeclaration", "export { a, b as c } from \"m\";"),
        ("ExportAssignment", "export default 42;"),
        // Types.
        ("TypeReference", "let x: Array<number>;"),
        ("ArrayType", "let x: number[];"),
        ("UnionType", "let x: A | B | C;"),
        ("TypeLiteral", "type T = { a: number; b(): void };"),
        // Expressions (round 3).
        ("ObjectLiteral", "x = { a, b: 1, [c]: 2, ...d, m() {} };"),
        ("PropertyAssignmentShorthand", "x = { a = 1 };"),
        ("FunctionExpression", "x = function* g(a): void {};"),
        ("ArrowSimple", "x = a => a + 1;"),
        ("ArrowParen", "x = (a: number): string => \"x\";"),
        ("ArrowAsync", "x = async (a) => a;"),
        ("DeleteTypeofVoid", "delete a.b; typeof a; void 0;"),
        ("AwaitExpression", "await p;"),
        ("YieldExpression", "function* g() { yield* x; }"),
        ("NewExpression", "new Foo(a, b);"),
        ("MetaProperty", "new.target; import.meta;"),
        ("OptionalChain", "a?.b?.[c]?.();"),
        ("NonNullExpression", "a!.b;"),
        ("TemplateExpression", "`a${b}c${d}e`;"),
        ("TaggedTemplate", "tag`x${y}z`;"),
        ("RegexLiteral", "/ab+c/g;"),
        ("AsExpression", "x as T;"),
        ("SatisfiesExpression", "x satisfies T;"),
        ("TypeAssertion", "<T>x;"),
        // Types (round 3).
        ("FunctionType", "type F = (a: number) => string;"),
        ("ConstructorType", "type C = abstract new () => Foo;"),
        ("ConditionalType", "type T<X> = X extends Y ? A : B;"),
        (
            "InferType",
            "type E<T> = T extends Array<infer U> ? U : never;",
        ),
        ("KeyofOperator", "type K = keyof T;"),
        ("ReadonlyArrayType", "type R = readonly number[];"),
        (
            "TupleType",
            "type Tup = [number, b?: string, ...boolean[]];",
        ),
        (
            "MappedType",
            "type M<T> = { readonly [K in keyof T]?: T[K] };",
        ),
        ("TemplateLiteralType", "type S<T> = `a${T}b`;"),
        ("ImportType", "type I = import(\"mod\").Foo<number>;"),
        ("TypeQuery", "type Q = typeof x.y;"),
        ("TypePredicate", "function f(x): x is string {}"),
        ("AssertsPredicate", "function g(x): asserts x is T {}"),
        ("ThisType", "type T = () => this;"),
        // Round 4 additions.
        ("TypeArgumentsInCall", "f<number, string>(42);"),
        ("InstantiationExpression", "const g = f<number>;"),
        ("NegativeLiteralType", "type N = -1;"),
        ("Decorators", "@sealed class C { @ro x = 1; m(@p a) {} }"),
        (
            "ImportAttributes",
            "import x from \"m\" with { type: \"json\" };",
        ),
        (
            "ExportAttributes",
            "export { a } from \"m\" with { type: \"json\" };",
        ),
        (
            "ImportTypeAttributes",
            "type T = import(\"m\", { with: { type: \"json\" } }).X;",
        ),
    ];

    for (title, input) in cases {
        let mut result =
            parse_source_file(SourceFileParseOptions::default(), input, ScriptKind::Ts);
        assert!(
            result.diagnostics.is_empty(),
            "case {title}: unexpected diagnostics for {input:?}"
        );
        let file = result.source_file;
        let clone = result.arena.deep_clone_node(file);
        assert_deep_clone_structure(&mut result.arena, file, clone);
    }

    // JSX cases (parsed as `.tsx`).
    let jsx_cases: &[(&str, &str)] = &[
        (
            "JsxElement",
            "const x = <div a=\"b\" c={d} {...e}>t {f}<br /></div>;",
        ),
        ("JsxFragment", "const x = <><span>a</span>{b}</>;"),
        ("JsxNamespacedMember", "const x = <Foo.Bar a:b=\"c\" />;"),
    ];
    for (title, input) in jsx_cases {
        let mut result =
            parse_source_file(SourceFileParseOptions::default(), input, ScriptKind::Tsx);
        assert!(
            result.diagnostics.is_empty(),
            "jsx case {title}: unexpected diagnostics for {input:?}"
        );
        let file = result.source_file;
        let clone = result.arena.deep_clone_node(file);
        assert_deep_clone_structure(&mut result.arena, file, clone);
    }

    // JSON cases.
    let json_cases: &[(&str, &str)] = &[
        ("JsonObject", "{ \"a\": 1, \"b\": [true, null, -2] }"),
        ("JsonArray", "[1, 2, 3]"),
    ];
    for (title, input) in json_cases {
        let mut result =
            parse_source_file(SourceFileParseOptions::default(), input, ScriptKind::Json);
        assert!(
            result.diagnostics.is_empty(),
            "json case {title}: unexpected diagnostics for {input:?}"
        );
        let file = result.source_file;
        let clone = result.arena.deep_clone_node(file);
        assert_deep_clone_structure(&mut result.arena, file, clone);
    }
}
