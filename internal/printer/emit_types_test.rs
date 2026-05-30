use crate::test_support::check_emit;

// Go: internal/printer/printer_test.go:TestEmit/AsExpression
#[test]
fn as_expression() {
    check_emit("a as T", "a as T;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/SatisfiesExpression
#[test]
fn satisfies_expression() {
    check_emit("a satisfies T", "a satisfies T;", false);
}

fn t(input_type: &str, output_type: &str) {
    check_emit(
        &format!("type T = {input_type}"),
        &format!("type T = {output_type};"),
        false,
    );
}

// Go: internal/printer/printer_test.go:TestEmit/KeywordTypeNode
#[test]
fn keyword_type_node() {
    for kw in [
        "any",
        "unknown",
        "never",
        "void",
        "undefined",
        "object",
        "string",
        "symbol",
        "number",
        "bigint",
        "boolean",
    ] {
        t(kw, kw);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/TypeReferenceNode
#[test]
fn type_reference_node() {
    t("a", "a");
    t("a.b", "a.b");
    t("a<U>", "a<U>");
    t("a.b<U>", "a.b<U>");
}

// Go: internal/printer/printer_test.go:TestEmit/FunctionTypeNode + ConstructorTypeNode
#[test]
fn function_and_constructor_type() {
    t("() => a", "() => a");
    t("<T>() => a", "<T>() => a");
    t("(a) => b", "(a) => b");
    t("new () => a", "new () => a");
    t("new <T>() => a", "new <T>() => a");
    t("new (a) => b", "new (a) => b");
    t("abstract new () => a", "abstract new () => a");
}

// Go: internal/printer/printer_test.go:TestEmit/TypeQueryNode
#[test]
fn type_query_node() {
    t("typeof a", "typeof a");
    t("typeof a.b", "typeof a.b");
    t("typeof a<U>", "typeof a<U>");
}

// Go: internal/printer/printer_test.go:TestEmit/TypeLiteralNode + ArrayType
#[test]
fn type_literal_and_array() {
    t("{}", "{}");
    t("{a}", "{\n    a;\n}");
    t("a[]", "a[]");
}

// Go: internal/printer/printer_test.go:TestEmit/TupleTypeNode + Rest/Optional/NamedTupleMember
#[test]
fn tuple_types() {
    t("[]", "[\n]");
    t("[a]", "[\n    a\n]");
    t("[a,]", "[\n    a\n]");
    t("[...a]", "[\n    ...a\n]");
    t("[a: b]", "[\n    a: b\n]");
    // DEFER: `tsgo_parser` rejects optional tuple elements (`[a?]`, `[a?: b]`),
    // reporting "',' expected". The printer's OptionalType/NamedTupleMember `?`
    // paths are implemented; re-enable when the parser supports them.
}

// Go: internal/printer/printer_test.go:TestEmit/UnionType + IntersectionType
#[test]
fn union_and_intersection() {
    t("a | b", "a | b");
    t("a | b | c", "a | b | c");
    t("| a | b", "a | b");
    t("a & b", "a & b");
    t("a & b & c", "a & b & c");
    t("& a & b", "a & b");
}

// Go: internal/printer/printer_test.go:TestEmit/ConditionalType + InferType
#[test]
fn conditional_and_infer() {
    t("a extends b ? c : d", "a extends b ? c : d");
    t("a extends infer b ? c : d", "a extends infer b ? c : d");
    t(
        "a extends infer b extends c ? d : e",
        "a extends infer b extends c ? d : e",
    );
}

// Go: internal/printer/printer_test.go:TestEmit/ParenthesizedType + ThisType
#[test]
fn parenthesized_and_this_type() {
    t("(U)", "(U)");
    t("this", "this");
}

// Go: internal/printer/printer_test.go:TestEmit/TypeOperatorNode + IndexedAccessType
#[test]
fn type_operator_and_indexed_access() {
    t("keyof U", "keyof U");
    t("readonly U[]", "readonly U[]");
    t("unique symbol", "unique symbol");
    t("a[b]", "a[b]");
}

// Go: internal/printer/printer_test.go:TestEmit/LiteralTypeNode
#[test]
fn literal_type_node() {
    t("null", "null");
    t("true", "true");
    t("false", "false");
    t("\"\"", "\"\"");
    t("0", "0");
    t("0n", "0n");
    t("-0", "-0");
}

// Go: internal/printer/printer_test.go:TestEmit/TemplateTypeNode
#[test]
fn template_type_node() {
    t("`a${b}c`", "`a${b}c`");
    t("`a${b}c${d}e`", "`a${b}c${d}e`");
}

// Go: internal/printer/printer_test.go:TestEmit/MappedType
#[test]
fn mapped_type() {
    t("{ [a in b]: c }", "{\n    [a in b]: c;\n}");
    t(
        "{ readonly [a in b]: c }",
        "{\n    readonly [a in b]: c;\n}",
    );
    t("{ [a in b]?: c }", "{\n    [a in b]?: c;\n}");
    t("{ [a in b as c]: d }", "{\n    [a in b as c]: d;\n}");
}
