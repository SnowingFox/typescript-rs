use super::*;

// Go: internal/checker/types.go:TypeFlags (bit positions)
#[test]
fn type_flags_have_go_bit_positions() {
    assert_eq!(TypeFlags::ANY.bits(), 1 << 0);
    assert_eq!(TypeFlags::UNKNOWN.bits(), 1 << 1);
    assert_eq!(TypeFlags::STRING.bits(), 1 << 5);
    assert_eq!(TypeFlags::NUMBER.bits(), 1 << 6);
    assert_eq!(TypeFlags::OBJECT.bits(), 1 << 20);
    assert_eq!(TypeFlags::UNION.bits(), 1 << 27);
    assert_eq!(TypeFlags::INTERSECTION.bits(), 1 << 28);
    assert_eq!(TypeFlags::RESERVED3.bits(), 1 << 31);
}

// Go: internal/checker/types.go:TypeFlags (composite unions)
#[test]
fn type_flags_composites_match_go() {
    assert_eq!(
        TypeFlags::LITERAL,
        TypeFlags::STRING_LITERAL
            | TypeFlags::NUMBER_LITERAL
            | TypeFlags::BIG_INT_LITERAL
            | TypeFlags::BOOLEAN_LITERAL
    );
    assert_eq!(
        TypeFlags::INTRINSIC,
        TypeFlags::ANY
            | TypeFlags::UNKNOWN
            | TypeFlags::STRING
            | TypeFlags::NUMBER
            | TypeFlags::BIG_INT
            | TypeFlags::ES_SYMBOL
            | TypeFlags::VOID
            | TypeFlags::UNDEFINED
            | TypeFlags::NULL
            | TypeFlags::NEVER
            | TypeFlags::NON_PRIMITIVE
    );
    assert_eq!(
        TypeFlags::UNION_OR_INTERSECTION,
        TypeFlags::UNION | TypeFlags::INTERSECTION
    );
    // Repurposed-bit aliases share their base bit (Go reuses the bit value).
    assert_eq!(TypeFlags::INCLUDES_ERROR, TypeFlags::RESERVED2);
}

// Go: internal/checker/types.go:ObjectFlags
#[test]
fn object_flags_have_go_bit_positions() {
    assert_eq!(ObjectFlags::CLASS.bits(), 1 << 0);
    assert_eq!(ObjectFlags::INTERFACE.bits(), 1 << 1);
    assert_eq!(ObjectFlags::ANONYMOUS.bits(), 1 << 4);
    assert_eq!(ObjectFlags::NON_INFERRABLE_TYPE.bits(), 1 << 18);
    assert_eq!(ObjectFlags::MEMBERS_RESOLVED.bits(), 1 << 21);
}

// Go: internal/checker/types.go:ObjectFlags (composite masks)
#[test]
fn object_flags_composites_match_go() {
    assert_eq!(
        ObjectFlags::CLASS_OR_INTERFACE,
        ObjectFlags::CLASS | ObjectFlags::INTERFACE
    );
    assert_eq!(
        ObjectFlags::PROPAGATING_FLAGS,
        ObjectFlags::CONTAINS_WIDENING_TYPE
            | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL
            | ObjectFlags::NON_INFERRABLE_TYPE
    );
    // The bits Go's newType strips from a freshly allocated type.
    assert_eq!(
        ObjectFlags::FRESH_ALLOCATION_CLEARED,
        ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED
            | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES
            | ObjectFlags::MEMBERS_RESOLVED
    );
}

// Go: internal/checker/types.go:FormatTypeFlags
#[test]
fn format_type_flags_single() {
    assert_eq!(format_type_flags(TypeFlags::STRING), vec!["String"]);
    assert_eq!(format_type_flags(TypeFlags::NUMBER), vec!["Number"]);
    assert_eq!(format_type_flags(TypeFlags::ANY), vec!["Any"]);
    assert_eq!(
        format_type_flags(TypeFlags::NON_PRIMITIVE),
        vec!["NonPrimitive"]
    );
}

// Go: internal/checker/types.go:FormatTypeFlags (multiple bits in table order)
#[test]
fn format_type_flags_multiple_in_table_order() {
    // Object (1<<20) is listed before Union (1<<27) in Go's typeFlagNames.
    assert_eq!(
        format_type_flags(TypeFlags::UNION | TypeFlags::OBJECT),
        vec!["Object", "Union"]
    );
    assert_eq!(
        format_type_flags(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL),
        vec!["StringLiteral", "NumberLiteral"]
    );
}

// Go: internal/checker/types.go:FormatTypeFlags (empty -> "None")
#[test]
fn format_type_flags_none() {
    assert_eq!(format_type_flags(TypeFlags::empty()), vec!["None"]);
    // The reserved bits have no name and fall through to "None".
    assert_eq!(format_type_flags(TypeFlags::RESERVED3), vec!["None"]);
}

fn intrinsic(name: &str) -> TypeData {
    TypeData::Intrinsic(IntrinsicType {
        intrinsic_name: name.to_string(),
    })
}

// Go: internal/checker/checker.go:Checker.newType (ids start at 1 and increase)
#[test]
fn arena_assigns_sequential_ids_from_one() {
    let mut arena = TypeArena::new();
    assert!(arena.is_empty());
    let a = arena.alloc(TypeFlags::ANY, ObjectFlags::empty(), None, intrinsic("any"));
    let b = arena.alloc(
        TypeFlags::STRING,
        ObjectFlags::empty(),
        None,
        intrinsic("string"),
    );
    assert_eq!(a, TypeId(1));
    assert_eq!(b, TypeId(2));
    assert_eq!(arena.len(), 2);
    assert_eq!(arena.get(a).id(), TypeId(1));
    assert_eq!(arena.get(b).id(), TypeId(2));
}

// Go: internal/checker/types.go:Type (header accessors + intrinsic name)
#[test]
fn arena_get_exposes_type_header() {
    let mut arena = TypeArena::new();
    let id = arena.alloc(
        TypeFlags::NON_PRIMITIVE,
        ObjectFlags::NON_INFERRABLE_TYPE,
        None,
        intrinsic("object"),
    );
    let t = arena.get(id);
    assert_eq!(t.flags(), TypeFlags::NON_PRIMITIVE);
    assert_eq!(t.object_flags(), ObjectFlags::NON_INFERRABLE_TYPE);
    assert_eq!(t.intrinsic_name(), Some("object"));
}

// Go: internal/checker/types.go:TypeId (arena index is id - 1)
#[test]
fn type_id_arena_index_is_id_minus_one() {
    assert_eq!(TypeId(1).arena_index(), 0);
    assert_eq!(TypeId(5).arena_index(), 4);
}

// Go: internal/checker/types.go:LiteralType (literal payload + accessor)
#[test]
fn arena_literal_value_accessor() {
    let mut arena = TypeArena::new();
    let id = arena.alloc(
        TypeFlags::BOOLEAN_LITERAL,
        ObjectFlags::empty(),
        None,
        TypeData::Literal(LiteralType {
            value: LiteralValue::Boolean(false),
            fresh_type: None,
            regular_type: None,
        }),
    );
    assert_eq!(
        arena.get(id).literal_value(),
        Some(&LiteralValue::Boolean(false))
    );
    assert_eq!(arena.get(id).intrinsic_name(), None);
    assert_eq!(arena.get(id).union_types(), None);
}

// Go: internal/checker/types.go:ObjectType (members + as_object accessor)
#[test]
fn arena_object_members_accessor() {
    use tsgo_ast::{SymbolId, SymbolTable};
    let mut arena = TypeArena::new();
    let mut members = SymbolTable::default();
    members.insert("bar".to_string(), SymbolId(3));
    let id = arena.alloc(
        TypeFlags::OBJECT,
        ObjectFlags::INTERFACE,
        None,
        TypeData::Object(ObjectType {
            members,
            properties: vec![SymbolId(3)],
            ..Default::default()
        }),
    );
    let obj = arena.get(id).as_object().expect("object type");
    assert_eq!(obj.members.get("bar"), Some(&SymbolId(3)));
    assert_eq!(arena.get(id).intrinsic_name(), None);
    assert_eq!(arena.get(id).union_types(), None);
}

// Go: internal/checker/types.go:UnionType (union payload + accessor)
#[test]
fn arena_union_types_accessor() {
    let mut arena = TypeArena::new();
    let a = arena.alloc(
        TypeFlags::STRING,
        ObjectFlags::empty(),
        None,
        intrinsic("string"),
    );
    let b = arena.alloc(
        TypeFlags::NUMBER,
        ObjectFlags::empty(),
        None,
        intrinsic("number"),
    );
    let u = arena.alloc(
        TypeFlags::UNION,
        ObjectFlags::empty(),
        None,
        TypeData::Union(UnionType { types: vec![a, b] }),
    );
    assert_eq!(arena.get(u).union_types(), Some(&[a, b][..]));
    assert_eq!(arena.get(u).literal_value(), None);
}

// Go: internal/checker/types.go:ConditionalType (payload + AsConditionalType accessor)
#[test]
fn arena_conditional_type_accessor() {
    use tsgo_ast::NodeId;
    let mut arena = TypeArena::new();
    let check = arena.alloc(
        TypeFlags::TYPE_PARAMETER,
        ObjectFlags::empty(),
        None,
        TypeData::TypeParameter(crate::core::types::TypeParameter::default()),
    );
    let extends = arena.alloc(
        TypeFlags::STRING,
        ObjectFlags::empty(),
        None,
        intrinsic("string"),
    );
    let root = ConditionalRoot {
        node: NodeId(7),
        check_type: check,
        extends_type: extends,
        is_distributive: true,
        infer_type_parameters: vec![],
        outer_type_parameters: vec![check],
    };
    let id = arena.alloc(
        TypeFlags::CONDITIONAL,
        ObjectFlags::empty(),
        None,
        TypeData::Conditional(ConditionalType {
            root: root.clone(),
            check_type: check,
            extends_type: extends,
        }),
    );
    let d = arena.get(id).as_conditional().expect("conditional type");
    assert_eq!(d.check_type, check);
    assert_eq!(d.extends_type, extends);
    assert!(d.root.is_distributive);
    assert_eq!(d.root.node, NodeId(7));
    // A non-conditional type returns `None` from the accessor.
    assert_eq!(arena.get(check).as_conditional(), None);
}

// Go: internal/checker/types.go:Type.AsTemplateLiteralType / AsStringMappingType
#[test]
fn template_literal_and_string_mapping_accessors() {
    let mut arena = TypeArena::new();
    let ph = arena.alloc(
        TypeFlags::TYPE_PARAMETER,
        ObjectFlags::empty(),
        None,
        TypeData::TypeParameter(TypeParameter::default()),
    );
    let tmpl = arena.alloc(
        TypeFlags::TEMPLATE_LITERAL,
        ObjectFlags::empty(),
        None,
        TypeData::TemplateLiteral(TemplateLiteralType {
            texts: vec!["a".to_string(), "b".to_string()],
            types: vec![ph],
        }),
    );
    let d = arena
        .get(tmpl)
        .as_template_literal()
        .expect("template literal");
    assert_eq!(d.texts.len(), d.types.len() + 1);
    assert_eq!(d.types, vec![ph]);
    assert_eq!(arena.get(ph).as_template_literal(), None);

    let sm = arena.alloc(
        TypeFlags::STRING_MAPPING,
        ObjectFlags::empty(),
        None,
        TypeData::StringMapping(StringMappingType {
            kind: StringMappingKind::Uppercase,
            target: ph,
        }),
    );
    let m = arena.get(sm).as_string_mapping().expect("string mapping");
    assert_eq!(m.kind, StringMappingKind::Uppercase);
    assert_eq!(m.target, ph);
    assert_eq!(arena.get(ph).as_string_mapping(), None);
}

// Go: internal/checker/checker.go:intrinsicTypeKinds / MappedTypeModifiers
#[test]
fn string_mapping_kind_and_mapped_type_modifiers() {
    assert_eq!(
        StringMappingKind::from_name("Uppercase"),
        Some(StringMappingKind::Uppercase)
    );
    assert_eq!(
        StringMappingKind::from_name("Lowercase"),
        Some(StringMappingKind::Lowercase)
    );
    assert_eq!(
        StringMappingKind::from_name("Capitalize"),
        Some(StringMappingKind::Capitalize)
    );
    assert_eq!(
        StringMappingKind::from_name("Uncapitalize"),
        Some(StringMappingKind::Uncapitalize)
    );
    assert_eq!(StringMappingKind::from_name("Nope"), None);
    assert_eq!(StringMappingKind::Capitalize.intrinsic_name(), "Capitalize");
    assert_eq!(MappedTypeModifiers::INCLUDE_READONLY.bits(), 1 << 0);
    assert_eq!(MappedTypeModifiers::EXCLUDE_READONLY.bits(), 1 << 1);
    assert_eq!(MappedTypeModifiers::INCLUDE_OPTIONAL.bits(), 1 << 2);
    assert_eq!(MappedTypeModifiers::EXCLUDE_OPTIONAL.bits(), 1 << 3);
}
