use super::*;
use crate::core::declared_types::{
    get_declared_type_of_symbol, get_index_type, get_indexed_access_type,
};
use crate::core::program::BoundProgram;
use crate::core::signatures::{Signature, SignatureFlags};
use crate::core::test_support::StubProgram;
use crate::core::types::{ObjectFlags, TypeFlags};
use crate::core::Checker;
use std::rc::Rc;
use tsgo_ast::SymbolId;

fn local(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

// Go: internal/checker/mapper.go:newTypeMapper
#[test]
fn type_mapper_new_picks_simple_or_array() {
    match TypeMapper::new(&[TypeId(1)], &[TypeId(2)]) {
        TypeMapper::Simple { source, target } => {
            assert_eq!(source, TypeId(1));
            assert_eq!(target, TypeId(2));
        }
        other => panic!("expected Simple, got {other:?}"),
    }
    match TypeMapper::new(&[TypeId(1), TypeId(3)], &[TypeId(2), TypeId(4)]) {
        TypeMapper::Array { sources, targets } => {
            assert_eq!(sources, vec![TypeId(1), TypeId(3)]);
            assert_eq!(targets, vec![TypeId(2), TypeId(4)]);
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

// Go: internal/checker/mapper.go:SimpleTypeMapper.Map / ArrayTypeMapper.Map
#[test]
fn map_type_simple_and_array() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);

    let simple = TypeMapper::Simple {
        source: a,
        target: c.string_type(),
    };
    assert_eq!(c.map_type(&simple, a), c.string_type());
    assert_eq!(c.map_type(&simple, b), b); // unrelated unchanged

    let array = TypeMapper::Array {
        sources: vec![a, b],
        targets: vec![c.string_type(), c.number_type()],
    };
    assert_eq!(c.map_type(&array, a), c.string_type());
    assert_eq!(c.map_type(&array, b), c.number_type());
    assert_eq!(c.map_type(&array, c.boolean_type()), c.boolean_type());
}

// Go: internal/checker/mapper.go:MergedTypeMapper.Map (m2(m1(t)))
#[test]
fn map_type_merged_composes() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);
    let m1 = TypeMapper::Simple {
        source: a,
        target: b,
    };
    let m2 = TypeMapper::Simple {
        source: b,
        target: c.string_type(),
    };
    let merged = TypeMapper::Merged(Box::new(m1), Box::new(m2));
    assert_eq!(c.map_type(&merged, a), c.string_type());
}

// Go: internal/checker/mapper.go:CompositeTypeMapper.Map (re-instantiates)
#[test]
fn map_type_composite_reinstantiates() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);
    let m1 = TypeMapper::Simple {
        source: a,
        target: b,
    };
    let m2 = TypeMapper::Simple {
        source: b,
        target: c.number_type(),
    };
    let composite = TypeMapper::Composite(Box::new(m1), Box::new(m2));
    // a -> b (changed) -> instantiate b with m2 -> number
    assert_eq!(c.map_type(&composite, a), c.number_type());
}

fn identity(t: TypeId) -> TypeId {
    t
}

// Go: internal/checker/mapper.go:FunctionTypeMapper.Map
#[test]
fn map_type_function() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    let m = TypeMapper::Function(identity);
    assert_eq!(c.map_type(&m, a), a);
}

// Go: internal/checker/checker.go:instantiateTypeWorker (type parameter / non-variable)
#[test]
fn instantiate_type_substitutes_type_parameter() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let m = TypeMapper::Simple {
        source: tp,
        target: c.number_type(),
    };
    assert_eq!(c.instantiate_type(tp, &m), c.number_type());
    // Types without type variables are returned unchanged.
    assert_eq!(c.instantiate_type(c.string_type(), &m), c.string_type());
}

// Go: internal/checker/checker.go:instantiateTypeWorker (union recursion)
#[test]
fn instantiate_type_maps_union_members() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let union = c.get_union_type(&[tp, c.number_type()]);
    let m = TypeMapper::Simple {
        source: tp,
        target: c.string_type(),
    };
    // { tp | number } with tp -> string  ==>  string | number
    assert_eq!(c.instantiate_type(union, &m), c.string_or_number_type());
}

// Go: internal/checker/checker.go:instantiateTypeWorker (generic type reference)
#[test]
fn instantiate_type_remaps_type_reference_arguments() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let target = c.new_object_type(ObjectFlags::INTERFACE, None, Default::default());
    let reference = c.create_type_reference(target, vec![tp]);
    let m = TypeMapper::Simple {
        source: tp,
        target: c.string_type(),
    };
    let instantiated = c.instantiate_type(reference, &m);
    let obj = c.get_type(instantiated).as_object().unwrap();
    assert_eq!(obj.target, Some(target));
    assert_eq!(obj.resolved_type_arguments, vec![c.string_type()]);
}

// C-B3: instantiation caching — the same generic instantiation returns a stable,
// cached type id (Go's `getTypeReferenceType` interning / the reachable form of
// the `(type, mapper)` instantiation cache). Two `create_type_reference` calls
// with the same `(target, type arguments)` yield the same id, and instantiating
// the same generic reference with the same mapper twice yields one id; a
// different type argument yields a different id.
// Go: internal/checker/checker.go:Checker.getTypeReferenceType (interning)
#[test]
fn create_type_reference_interns_by_target_and_arguments() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let target = c.new_object_type(ObjectFlags::INTERFACE, None, Default::default());
    let num = c.number_type();
    let s = c.string_type();
    // Same (target, args) -> one stable id.
    let a = c.create_type_reference(target, vec![num]);
    let b = c.create_type_reference(target, vec![num]);
    assert_eq!(a, b, "same instantiation is cached to one type id");
    // Different args -> a different id.
    let other = c.create_type_reference(target, vec![s]);
    assert_ne!(
        a, other,
        "a distinct type argument is a distinct instantiation"
    );
    // Instantiating the same generic reference with the same mapper is cached.
    let generic = c.create_type_reference(target, vec![tp]);
    let m = TypeMapper::Simple {
        source: tp,
        target: num,
    };
    let i1 = c.instantiate_type(generic, &m);
    let i2 = c.instantiate_type(generic, &m);
    assert_eq!(i1, i2, "repeated instantiation returns the cached id");
    assert_eq!(
        i1, a,
        "the instantiation matches the directly-created reference"
    );
}

// Go: internal/checker/checker.go:instantiateTypeWorker (intersection recursion)
#[test]
fn instantiate_type_maps_intersection_members() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let a = c.new_object_type(ObjectFlags::INTERFACE, None, Default::default());
    let inter = c.get_intersection_type(&[tp, a]);
    let m = TypeMapper::Simple {
        source: tp,
        target: c.string_type(),
    };
    // { tp & A } with tp -> string  ==>  string & A
    let instantiated = c.instantiate_type(inter, &m);
    let members = c
        .get_type(instantiated)
        .intersection_types()
        .unwrap()
        .to_vec();
    assert!(members.contains(&c.string_type()));
    assert!(members.contains(&a));
    assert!(!members.contains(&tp));
}

// An intersection with no type variables in any member is returned unchanged
// (the `instantiated == members` short-circuit).
// Go: internal/checker/checker.go:instantiateTypeWorker
#[test]
fn instantiate_type_intersection_without_type_vars_is_identity() {
    let mut c = Checker::new();
    let a = c.new_object_type(ObjectFlags::INTERFACE, None, Default::default());
    let b = c.new_object_type(ObjectFlags::INTERFACE, None, Default::default());
    let inter = c.get_intersection_type(&[a, b]);
    let tp = c.new_type_parameter(None);
    let m = TypeMapper::Simple {
        source: tp,
        target: c.string_type(),
    };
    assert_eq!(c.instantiate_type(inter, &m), inter);
}

// Go: internal/checker/mapper.go:newSimpleTypeMapper / mergeTypeMappers / appendTypeMapping
#[test]
fn mapper_combinators_compose() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);
    // unary
    let unary = TypeMapper::unary(a, c.number_type());
    assert_eq!(c.map_type(&unary, a), c.number_type());
    // merge: a -> b then b -> string
    let merged = TypeMapper::merge(
        TypeMapper::unary(a, b),
        TypeMapper::unary(b, c.string_type()),
    );
    assert_eq!(c.map_type(&merged, a), c.string_type());
    // combine(None, m) == m
    let combined_none = TypeMapper::combine(None, TypeMapper::unary(a, c.number_type()));
    assert_eq!(c.map_type(&combined_none, a), c.number_type());
    // combine(Some, m) re-instantiates (composite): a -> b (changed) then b -> number
    let combined = TypeMapper::combine(
        Some(TypeMapper::unary(a, b)),
        TypeMapper::unary(b, c.number_type()),
    );
    assert_eq!(c.map_type(&combined, a), c.number_type());
    // append_mapping: None -> unary; Some -> merge after
    let appended = TypeMapper::append_mapping(None, a, c.string_type());
    let appended = TypeMapper::append_mapping(Some(appended), b, c.number_type());
    assert_eq!(c.map_type(&appended, a), c.string_type());
    assert_eq!(c.map_type(&appended, b), c.number_type());
}

// An instantiated signature composes mappers so a re-instantiation applies both,
// and the parameter symbols stay the base symbols (mapped on read).
// Go: internal/checker/checker.go:instantiateSignatureEx (mapper composition)
#[test]
fn instantiate_signature_composes_mappers_and_keeps_parameters() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.resolved_return_type = Some(a);
    let id = c.new_signature(sig);
    // First instantiation a -> b.
    let inst1 = c.instantiate_signature(id, &TypeMapper::unary(a, b));
    assert_eq!(c.signature(inst1).resolved_return_type, Some(b));
    assert!(c.signature(inst1).mapper.is_some());
    // Second instantiation b -> number; the composed mapper takes a -> number.
    let inst2 = c.instantiate_signature(inst1, &TypeMapper::unary(b, c.number_type()));
    assert_eq!(
        c.signature(inst2).resolved_return_type,
        Some(c.number_type())
    );
    // The composed mapper still resolves the original `a` to `number`.
    let mapper = c.signature(inst2).mapper.clone().unwrap();
    assert_eq!(c.map_type(&mapper, a), c.number_type());
}

// Go: internal/checker/checker.go:instantiateSignature
#[test]
fn instantiate_signature_maps_return_type() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.resolved_return_type = Some(tp);
    let id = c.new_signature(sig);
    let m = TypeMapper::Simple {
        source: tp,
        target: c.number_type(),
    };
    let inst = c.instantiate_signature(id, &m);
    assert_eq!(
        c.signature(inst).resolved_return_type,
        Some(c.number_type())
    );
    assert_eq!(c.signature(inst).target, Some(id));
}

// Go: internal/checker/checker.go:instantiateTypeWorker (TypeFlagsIndex arm)
// Instantiating `keyof T` substitutes `T` then recomputes `keyof`, so with
// `T -> { a; b }` it yields `"a" | "b"`.
#[test]
fn instantiate_type_index_recomputes_keyof() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I {\n  a: number;\n  b: string;\n}",
    ));
    let mut c = Checker::new_checker(p.clone());
    let tp = c.new_type_parameter(None);
    let key = get_index_type(&mut c, tp); // keyof T (deferred)
    assert!(c.get_type(key).flags().contains(TypeFlags::INDEX));
    let i = get_declared_type_of_symbol(&mut c, &*p, local(&p, "I"), None);
    let mapper = TypeMapper::unary(tp, i);
    let result = c.instantiate_type(key, &mapper);
    let a = c.get_string_literal_type("a");
    let b = c.get_string_literal_type("b");
    let expected = c.get_union_type(&[a, b]);
    assert_eq!(result, expected, "keyof T with T -> I is \"a\" | \"b\"");
}

// Go: internal/checker/checker.go:instantiateTypeWorker (TypeFlagsIndexedAccess arm)
// Instantiating `T[K]` substitutes both operands then re-resolves the access, so
// with `T -> { a: number }, K -> "a"` it yields `number`.
#[test]
fn instantiate_type_indexed_access_resolves_property() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I {\n  a: number;\n  b: string;\n}",
    ));
    let mut c = Checker::new_checker(p.clone());
    let t = c.new_type_parameter(None);
    let k = c.new_type_parameter(None);
    let access = get_indexed_access_type(&mut c, &*p, t, k).expect("T[K] deferred");
    assert!(c
        .get_type(access)
        .flags()
        .contains(TypeFlags::INDEXED_ACCESS));
    let i = get_declared_type_of_symbol(&mut c, &*p, local(&p, "I"), None);
    let a = c.get_string_literal_type("a");
    let mapper = TypeMapper::Array {
        sources: vec![t, k],
        targets: vec![i, a],
    };
    let result = c.instantiate_type(access, &mapper);
    assert_eq!(
        result,
        c.number_type(),
        "T[K] with T -> I, K -> \"a\" is number"
    );
}

// Go: internal/checker/checker.go:instantiateTypeWorker (TypeFlagsConditional arm)
// Instantiating a deferred conditional without a retained program (an
// intrinsic-only checker that cannot read the branch type nodes) leaves it
// deferred — the branch is re-resolved only once a program is available.
#[test]
fn instantiate_type_conditional_without_program_is_deferred() {
    use crate::core::types::ConditionalRoot;
    use tsgo_ast::NodeId;
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let root = ConditionalRoot {
        node: NodeId(0),
        check_type: tp,
        extends_type: c.string_type(),
        is_distributive: true,
        infer_type_parameters: vec![],
        outer_type_parameters: vec![tp],
    };
    let cond = c.new_conditional_type(root, None);
    assert!(c.get_type(cond).flags().contains(TypeFlags::CONDITIONAL));
    let mapper = TypeMapper::unary(tp, c.string_type());
    // No retained program: stays the same deferred conditional id.
    assert_eq!(c.instantiate_type(cond, &mapper), cond);
}
