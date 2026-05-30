use super::*;
use crate::core::signatures::{Signature, SignatureFlags};
use crate::core::types::ObjectFlags;
use crate::core::Checker;

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
