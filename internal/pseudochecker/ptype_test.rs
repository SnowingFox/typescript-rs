use super::*;
use tsgo_ast::NodeArena;

// Go: internal/pseudochecker/type.go:PseudoTypeKind
// The discriminant order mirrors Go's `iota` block exactly. Ground truth is the
// Go source: `BigIntLiteral` is the 20th constant, i.e. index 19 (the package
// planning docs say "==18", which is an off-by-one in the docs).
#[test]
fn pseudotype_kind_values() {
    assert_eq!(PseudoTypeKind::Direct as i16, 0);
    assert_eq!(PseudoTypeKind::Inferred as i16, 1);
    assert_eq!(PseudoTypeKind::NoResult as i16, 2);
    assert_eq!(PseudoTypeKind::MaybeConstLocation as i16, 3);
    assert_eq!(PseudoTypeKind::Union as i16, 4);
    assert_eq!(PseudoTypeKind::Undefined as i16, 5);
    assert_eq!(PseudoTypeKind::Null as i16, 6);
    assert_eq!(PseudoTypeKind::Any as i16, 7);
    assert_eq!(PseudoTypeKind::String as i16, 8);
    assert_eq!(PseudoTypeKind::Number as i16, 9);
    assert_eq!(PseudoTypeKind::BigInt as i16, 10);
    assert_eq!(PseudoTypeKind::Boolean as i16, 11);
    assert_eq!(PseudoTypeKind::False as i16, 12);
    assert_eq!(PseudoTypeKind::True as i16, 13);
    assert_eq!(PseudoTypeKind::SingleCallSignature as i16, 14);
    assert_eq!(PseudoTypeKind::Tuple as i16, 15);
    assert_eq!(PseudoTypeKind::ObjectLiteral as i16, 16);
    assert_eq!(PseudoTypeKind::StringLiteral as i16, 17);
    assert_eq!(PseudoTypeKind::NumericLiteral as i16, 18);
    assert_eq!(PseudoTypeKind::BigIntLiteral as i16, 19);
}

// Go: internal/pseudochecker/type.go:PseudoType (the no-payload singletons +
// `Kind`). Each no-payload variant reports its matching `PseudoTypeKind`.
#[test]
fn no_payload_variants_report_their_kind() {
    assert_eq!(PseudoType::Undefined.kind(), PseudoTypeKind::Undefined);
    assert_eq!(PseudoType::Null.kind(), PseudoTypeKind::Null);
    assert_eq!(PseudoType::Any.kind(), PseudoTypeKind::Any);
    assert_eq!(PseudoType::String.kind(), PseudoTypeKind::String);
    assert_eq!(PseudoType::Number.kind(), PseudoTypeKind::Number);
    assert_eq!(PseudoType::BigInt.kind(), PseudoTypeKind::BigInt);
    assert_eq!(PseudoType::Boolean.kind(), PseudoTypeKind::Boolean);
    assert_eq!(PseudoType::False.kind(), PseudoTypeKind::False);
    assert_eq!(PseudoType::True.kind(), PseudoTypeKind::True);
}

// Go: internal/pseudochecker/type.go:NewPseudoTypeDirect / NewPseudoTypeStringLiteral
// / NewPseudoTypeNumericLiteral / NewPseudoTypeBigIntLiteral
// Single-node constructors record the node and report the matching kind.
#[test]
fn single_node_constructors() {
    let mut arena = NodeArena::new();
    let n = arena.new_identifier("x");

    assert_eq!(PseudoType::direct(n), PseudoType::Direct { type_node: n });
    assert_eq!(PseudoType::string_literal(n), PseudoType::StringLiteral(n));
    assert_eq!(
        PseudoType::numeric_literal(n),
        PseudoType::NumericLiteral(n)
    );
    assert_eq!(PseudoType::bigint_literal(n), PseudoType::BigIntLiteral(n));

    assert_eq!(PseudoType::direct(n).kind(), PseudoTypeKind::Direct);
    assert_eq!(
        PseudoType::string_literal(n).kind(),
        PseudoTypeKind::StringLiteral
    );
    assert_eq!(
        PseudoType::numeric_literal(n).kind(),
        PseudoTypeKind::NumericLiteral
    );
    assert_eq!(
        PseudoType::bigint_literal(n).kind(),
        PseudoTypeKind::BigIntLiteral
    );
}

// Go: internal/pseudochecker/type.go:NewPseudoTypeInferred / NewPseudoTypeInferredWithErrors
// The bare constructor leaves `error_nodes` empty; the `_with_errors` form
// carries the collected blocking nodes.
#[test]
fn inferred_constructors_carry_error_nodes() {
    let mut arena = NodeArena::new();
    let expr = arena.new_identifier("e");
    let err1 = arena.new_identifier("a");
    let err2 = arena.new_identifier("b");

    assert_eq!(
        PseudoType::inferred(expr),
        PseudoType::Inferred {
            expression: expr,
            error_nodes: vec![]
        }
    );
    assert_eq!(
        PseudoType::inferred_with_errors(expr, vec![err1, err2]),
        PseudoType::Inferred {
            expression: expr,
            error_nodes: vec![err1, err2]
        }
    );
}

// Go: internal/pseudochecker/type.go:NewPseudoTypeNoResult / NewPseudoTypeMaybeConstLocation
// / NewPseudoTypeUnion
// Composite constructors box their children and report the matching kind.
#[test]
fn composite_constructors() {
    let mut arena = NodeArena::new();
    let n = arena.new_identifier("n");

    assert_eq!(
        PseudoType::no_result(n),
        PseudoType::NoResult { declaration: n }
    );

    let mc = PseudoType::maybe_const_location(n, PseudoType::True, PseudoType::Boolean);
    assert_eq!(
        mc,
        PseudoType::MaybeConstLocation {
            node: n,
            const_type: Box::new(PseudoType::True),
            regular_type: Box::new(PseudoType::Boolean),
        }
    );
    assert_eq!(mc.kind(), PseudoTypeKind::MaybeConstLocation);

    let u = PseudoType::union(vec![PseudoType::String, PseudoType::Undefined]);
    assert_eq!(
        u,
        PseudoType::Union(vec![PseudoType::String, PseudoType::Undefined])
    );
    assert_eq!(u.kind(), PseudoTypeKind::Union);
}

// Go: internal/pseudochecker/type.go:NewPseudoParameter
// A pseudo parameter records rest/optional flags, its name node, and a boxed type.
#[test]
fn pseudo_parameter_new() {
    let mut arena = NodeArena::new();
    let name = arena.new_identifier("p");

    let p = PseudoParameter::new(true, name, false, PseudoType::Any);
    assert_eq!(
        p,
        PseudoParameter {
            rest: true,
            name,
            optional: false,
            type_: Box::new(PseudoType::Any),
        }
    );
}

// Go: internal/pseudochecker/type.go:NewPseudoTypeSingleCallSignature / NewPseudoTypeTuple
// / NewPseudoTypeObjectLiteral
// Aggregate constructors report the matching kind and retain their children.
#[test]
fn aggregate_constructors() {
    let mut arena = NodeArena::new();
    let sig = arena.new_identifier("sig");
    let tp = arena.new_identifier("T");
    let pname = arena.new_identifier("p");

    let params = vec![PseudoParameter::new(
        false,
        pname,
        false,
        PseudoType::Number,
    )];
    let scs = PseudoType::single_call_signature(sig, params.clone(), vec![tp], PseudoType::Number);
    assert_eq!(
        scs,
        PseudoType::SingleCallSignature {
            signature: sig,
            parameters: params,
            type_parameters: vec![tp],
            return_type: Box::new(PseudoType::Number),
        }
    );
    assert_eq!(scs.kind(), PseudoTypeKind::SingleCallSignature);

    let tup = PseudoType::tuple(vec![PseudoType::Number, PseudoType::String]);
    assert_eq!(
        tup,
        PseudoType::Tuple {
            elements: vec![PseudoType::Number, PseudoType::String]
        }
    );
    assert_eq!(tup.kind(), PseudoTypeKind::Tuple);

    let obj = PseudoType::object_literal(vec![]);
    assert_eq!(obj, PseudoType::ObjectLiteral { elements: vec![] });
    assert_eq!(obj.kind(), PseudoTypeKind::ObjectLiteral);
}

// Go: internal/pseudochecker/type.go:PseudoObjectElement.Signature
// Method/SetAccessor/GetAccessor expose their signature node; a property
// assignment has none. The shared `name`/`optional`/`kind` are also exposed.
#[test]
fn object_element_signature_accessor() {
    let mut arena = NodeArena::new();
    let sig = arena.new_identifier("sig");
    let name = arena.new_identifier("m");
    let pname = arena.new_identifier("v");

    let method = PseudoObjectElement::method(sig, name, true, vec![], vec![], PseudoType::Number);
    assert_eq!(method.kind(), PseudoObjectElementKind::Method);
    assert_eq!(method.name(), name);
    assert!(method.optional());
    assert_eq!(method.signature(), Some(sig));

    let prop = PseudoObjectElement::property_assignment(true, name, false, PseudoType::String);
    assert_eq!(prop.kind(), PseudoObjectElementKind::PropertyAssignment);
    assert!(!prop.optional());
    assert_eq!(prop.signature(), None);

    let setter = PseudoObjectElement::set_accessor(
        sig,
        name,
        false,
        PseudoParameter::new(false, pname, false, PseudoType::Any),
    );
    assert_eq!(setter.kind(), PseudoObjectElementKind::SetAccessor);
    assert_eq!(setter.signature(), Some(sig));

    let getter = PseudoObjectElement::get_accessor(sig, name, false, PseudoType::Any);
    assert_eq!(getter.kind(), PseudoObjectElementKind::GetAccessor);
    assert_eq!(getter.signature(), Some(sig));
}
