use super::*;
use crate::core::declared_types::get_declared_type_of_symbol;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::types::{ObjectFlags, ObjectType};
use crate::core::Checker;
use tsgo_ast::SymbolId;

fn sym(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

// Go: internal/checker/checker.go:Checker.symbolToString (tracer)
#[test]
fn symbol_to_string_returns_declaration_name() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Foo {\n  bar: string;\n}");
    let foo = sym(&p, "Foo");
    assert_eq!(symbol_to_string(&p, foo), "Foo");
}

// Go: internal/checker/checker.go:Checker.typeToString (named interface)
#[test]
fn type_to_string_named_interface() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Foo {\n  bar: string;\n}");
    let mut c = Checker::new();
    let foo = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Foo"), None);
    // A named interface prints as its name.
    assert_eq!(type_to_string(&mut c, &p, foo), "Foo");
}

// Go: internal/checker/checker.go:Checker.typeToString (Index / IndexedAccess)
#[test]
fn type_to_string_index_and_indexed_access() {
    use crate::core::types::{AccessFlags, IndexFlags};
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    // `keyof T` prints with the `keyof` operator.
    let key = c.new_index_type(tp, IndexFlags::NONE);
    assert_eq!(type_to_string(&mut c, &p, key), "keyof T");
    // `T["a"]` prints in bracket form with the quoted literal index.
    let a = c.get_string_literal_type("a");
    let access = c.new_indexed_access_type(tp, a, AccessFlags::NONE);
    assert_eq!(type_to_string(&mut c, &p, access), "T[\"a\"]");
}

// Go: internal/checker/checker.go:Checker.typeToString (type reference)
#[test]
fn type_to_string_type_reference() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Box<T> {\n  value: T;\n}");
    let mut c = Checker::new();
    let box_t = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Box"), None);
    let s = c.string_type();
    let reference = c.create_type_reference(box_t, vec![s]);
    assert_eq!(type_to_string(&mut c, &p, reference), "Box<string>");
}

// Go: internal/checker/checker.go:Checker.typeToString (anonymous object literal)
#[test]
fn type_to_string_anonymous_object_members() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Holder {\n  value: string;\n}");
    let mut c = Checker::new();
    let holder = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Holder"), None);
    // Rebuild Holder's members as an anonymous (symbol-less) object type.
    let (members, properties) = {
        let o = c.get_type(holder).as_object().expect("object");
        (o.members.clone(), o.properties.clone())
    };
    let anon = c.new_object_type(
        ObjectFlags::empty(),
        None,
        ObjectType {
            members,
            properties,
            ..Default::default()
        },
    );
    assert_eq!(type_to_string(&mut c, &p, anon), "{ value: string; }");
}

// Go: internal/checker/checker.go:Checker.typeToString (union recursion)
#[test]
fn type_to_string_union_of_named() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface A {}\ninterface B {}");
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let union = c.get_union_type(&[a, b]);
    // Union members are id-sorted (A built before B), printed program-aware.
    assert_eq!(type_to_string(&mut c, &p, union), "A | B");
}

// Go: internal/checker/checker.go:Checker.typeToString (intersection recursion)
#[test]
fn type_to_string_intersection_of_named() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface A {}\ninterface B {}");
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let inter = c.get_intersection_type(&[a, b]);
    // Intersection members are id-sorted (A built before B), printed program-aware.
    assert_eq!(type_to_string(&mut c, &p, inter), "A & B");
}

// 4bi: a fixed-arity tuple prints as `[e0, e1]` with the positional element
// types (Go's tuple type-node serialization). A non-readonly tuple has no
// `readonly` prefix.
// Go: internal/checker/checker.go:Checker.typeToString (tuple)
#[test]
fn type_to_string_tuple_elements() {
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let tuple = c.create_tuple_type(vec![s, n]);
    assert_eq!(type_to_string(&mut c, &p, tuple), "[string, number]");
}

// 4bi: a readonly tuple (an `[...] as const` array literal) prints with a
// leading `readonly ` adornment.
// Go: internal/checker/checker.go:Checker.typeToString (readonly tuple)
#[test]
fn type_to_string_readonly_tuple_elements() {
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let tuple = c.create_tuple_type_ex(vec![s, n], true);
    assert_eq!(
        type_to_string(&mut c, &p, tuple),
        "readonly [string, number]"
    );
}

// Go: internal/checker/printer.go:typeToString (intrinsics/literals delegate)
#[test]
fn type_to_string_intrinsics_and_literals_delegate() {
    use crate::core::types::{LiteralValue, TypeFlags};
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(type_to_string(&mut c, &p, s), "string");
    let lit = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("x".into()),
        None,
    );
    assert_eq!(type_to_string(&mut c, &p, lit), "\"x\"");
}
