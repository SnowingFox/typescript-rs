//! The checker's `Type` representation: a [`TypeId`]-indexed arena, the
//! `TypeFlags`/`ObjectFlags` bit sets, and the type-data variants.
//!
//! Ownership: every `Type` lives in a [`TypeArena`] and is referred to by a
//! [`TypeId`] handle; Go's `*Type` pointers and the `Type.checker` back-pointer
//! are dropped in favor of arena indexing (PORTING, section 5).

use tsgo_ast::SymbolTable;

use super::signatures::{IndexInfoId, SignatureId};

bitflags::bitflags! {
    /// Classifies a `Type` (the union of "kinds" a type may be).
    ///
    /// Mirrors Go `TypeFlags` (a `uint32` bit set). Each single-bit flag has a
    /// fixed position matching the Go source so serialized flag names and any
    /// bit arithmetic stay identical across the port. The composite unions at
    /// the bottom mirror Go's derived constants.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::TypeFlags;
    /// assert_eq!(TypeFlags::ANY.bits(), 1);
    /// assert!(TypeFlags::STRING_LITERAL.intersects(TypeFlags::LITERAL));
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/checker/types.go:TypeFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct TypeFlags: u32 {
        /// The `any` type.
        const ANY = 1 << 0;
        /// The `unknown` type.
        const UNKNOWN = 1 << 1;
        /// The `undefined` type.
        const UNDEFINED = 1 << 2;
        /// The `null` type.
        const NULL = 1 << 3;
        /// The `void` type.
        const VOID = 1 << 4;
        /// The `string` primitive type.
        const STRING = 1 << 5;
        /// The `number` primitive type.
        const NUMBER = 1 << 6;
        /// The `bigint` primitive type.
        const BIG_INT = 1 << 7;
        /// The `boolean` primitive type.
        const BOOLEAN = 1 << 8;
        /// The ES `symbol` primitive type.
        const ES_SYMBOL = 1 << 9;
        /// A string literal type.
        const STRING_LITERAL = 1 << 10;
        /// A number literal type.
        const NUMBER_LITERAL = 1 << 11;
        /// A bigint literal type.
        const BIG_INT_LITERAL = 1 << 12;
        /// A boolean literal type (`true`/`false`).
        const BOOLEAN_LITERAL = 1 << 13;
        /// A `unique symbol` type.
        const UNIQUE_ES_SYMBOL = 1 << 14;
        /// An enum literal (always combined with a string/number literal or union).
        const ENUM_LITERAL = 1 << 15;
        /// A numeric computed enum member value.
        const ENUM = 1 << 16;
        /// The intrinsic non-primitive `object` type.
        const NON_PRIMITIVE = 1 << 17;
        /// The `never` type.
        const NEVER = 1 << 18;
        /// A type parameter.
        const TYPE_PARAMETER = 1 << 19;
        /// An object type.
        const OBJECT = 1 << 20;
        /// A `keyof T` index type.
        const INDEX = 1 << 21;
        /// A template literal type.
        const TEMPLATE_LITERAL = 1 << 22;
        /// An `Uppercase`/`Lowercase` string-mapping type.
        const STRING_MAPPING = 1 << 23;
        /// A type-parameter substitution type.
        const SUBSTITUTION = 1 << 24;
        /// An indexed access type `T[K]`.
        const INDEXED_ACCESS = 1 << 25;
        /// A conditional type `T extends U ? X : Y`.
        const CONDITIONAL = 1 << 26;
        /// A union type (`T | U`).
        const UNION = 1 << 27;
        /// An intersection type (`T & U`).
        const INTERSECTION = 1 << 28;
        /// Reserved bit used during union/intersection construction.
        const RESERVED1 = 1 << 29;
        /// Reserved bit used during union/intersection construction.
        const RESERVED2 = 1 << 30;
        /// Reserved bit.
        const RESERVED3 = 1 << 31;

        /// `any` or `unknown`.
        const ANY_OR_UNKNOWN = Self::ANY.bits() | Self::UNKNOWN.bits();
        /// `undefined` or `null`.
        const NULLABLE = Self::UNDEFINED.bits() | Self::NULL.bits();
        /// Any literal type.
        const LITERAL = Self::STRING_LITERAL.bits() | Self::NUMBER_LITERAL.bits() | Self::BIG_INT_LITERAL.bits() | Self::BOOLEAN_LITERAL.bits();
        /// Types that compare by unit value.
        const UNIT = Self::ENUM.bits() | Self::LITERAL.bits() | Self::UNIQUE_ES_SYMBOL.bits() | Self::NULLABLE.bits();
        /// Types that have a fresh/regular pair.
        const FRESHABLE = Self::ENUM.bits() | Self::LITERAL.bits();
        /// String or number literal.
        const STRING_OR_NUMBER_LITERAL = Self::STRING_LITERAL.bits() | Self::NUMBER_LITERAL.bits();
        /// String/number literal or `unique symbol`.
        const STRING_OR_NUMBER_LITERAL_OR_UNIQUE = Self::STRING_LITERAL.bits() | Self::NUMBER_LITERAL.bits() | Self::UNIQUE_ES_SYMBOL.bits();
        /// Always-falsy types.
        const DEFINITELY_FALSY = Self::STRING_LITERAL.bits() | Self::NUMBER_LITERAL.bits() | Self::BIG_INT_LITERAL.bits() | Self::BOOLEAN_LITERAL.bits() | Self::VOID.bits() | Self::UNDEFINED.bits() | Self::NULL.bits();
        /// Possibly-falsy types.
        const POSSIBLY_FALSY = Self::DEFINITELY_FALSY.bits() | Self::STRING.bits() | Self::NUMBER.bits() | Self::BIG_INT.bits() | Self::BOOLEAN.bits();
        /// The intrinsic types (those backed by an `IntrinsicType`).
        const INTRINSIC = Self::ANY.bits() | Self::UNKNOWN.bits() | Self::STRING.bits() | Self::NUMBER.bits() | Self::BIG_INT.bits() | Self::ES_SYMBOL.bits() | Self::VOID.bits() | Self::UNDEFINED.bits() | Self::NULL.bits() | Self::NEVER.bits() | Self::NON_PRIMITIVE.bits();
        /// String-like types.
        const STRING_LIKE = Self::STRING.bits() | Self::STRING_LITERAL.bits() | Self::TEMPLATE_LITERAL.bits() | Self::STRING_MAPPING.bits();
        /// Number-like types.
        const NUMBER_LIKE = Self::NUMBER.bits() | Self::NUMBER_LITERAL.bits() | Self::ENUM.bits();
        /// Bigint-like types.
        const BIG_INT_LIKE = Self::BIG_INT.bits() | Self::BIG_INT_LITERAL.bits();
        /// Boolean-like types.
        const BOOLEAN_LIKE = Self::BOOLEAN.bits() | Self::BOOLEAN_LITERAL.bits();
        /// Enum-like types.
        const ENUM_LIKE = Self::ENUM.bits() | Self::ENUM_LITERAL.bits();
        /// ES-symbol-like types.
        const ES_SYMBOL_LIKE = Self::ES_SYMBOL.bits() | Self::UNIQUE_ES_SYMBOL.bits();
        /// Void-like types.
        const VOID_LIKE = Self::VOID.bits() | Self::UNDEFINED.bits();
        /// Primitive types.
        const PRIMITIVE = Self::STRING_LIKE.bits() | Self::NUMBER_LIKE.bits() | Self::BIG_INT_LIKE.bits() | Self::BOOLEAN_LIKE.bits() | Self::ENUM_LIKE.bits() | Self::ES_SYMBOL_LIKE.bits() | Self::VOID_LIKE.bits() | Self::NULL.bits();
        /// Types that are definitely non-nullable.
        const DEFINITELY_NON_NULLABLE = Self::STRING_LIKE.bits() | Self::NUMBER_LIKE.bits() | Self::BIG_INT_LIKE.bits() | Self::BOOLEAN_LIKE.bits() | Self::ENUM_LIKE.bits() | Self::ES_SYMBOL_LIKE.bits() | Self::OBJECT.bits() | Self::NON_PRIMITIVE.bits();
        /// Types belonging to disjoint domains.
        const DISJOINT_DOMAINS = Self::NON_PRIMITIVE.bits() | Self::STRING_LIKE.bits() | Self::NUMBER_LIKE.bits() | Self::BIG_INT_LIKE.bits() | Self::BOOLEAN_LIKE.bits() | Self::ES_SYMBOL_LIKE.bits() | Self::VOID_LIKE.bits() | Self::NULL.bits();
        /// Union or intersection.
        const UNION_OR_INTERSECTION = Self::UNION.bits() | Self::INTERSECTION.bits();
        /// Structured types (have members).
        const STRUCTURED_TYPE = Self::OBJECT.bits() | Self::UNION.bits() | Self::INTERSECTION.bits();
        /// Type variables.
        const TYPE_VARIABLE = Self::TYPE_PARAMETER.bits() | Self::INDEXED_ACCESS.bits();
        /// Instantiable non-primitive types.
        const INSTANTIABLE_NON_PRIMITIVE = Self::TYPE_VARIABLE.bits() | Self::CONDITIONAL.bits() | Self::SUBSTITUTION.bits();
        /// Instantiable primitive types.
        const INSTANTIABLE_PRIMITIVE = Self::INDEX.bits() | Self::TEMPLATE_LITERAL.bits() | Self::STRING_MAPPING.bits();
        /// Instantiable types.
        const INSTANTIABLE = Self::INSTANTIABLE_NON_PRIMITIVE.bits() | Self::INSTANTIABLE_PRIMITIVE.bits();
        /// Structured or instantiable types.
        const STRUCTURED_OR_INSTANTIABLE = Self::STRUCTURED_TYPE.bits() | Self::INSTANTIABLE.bits();
        /// Types that carry object flags.
        const OBJECT_FLAGS_TYPE = Self::ANY.bits() | Self::NULLABLE.bits() | Self::NEVER.bits() | Self::OBJECT.bits() | Self::UNION.bits() | Self::INTERSECTION.bits();
        /// Types that may be simplified.
        const SIMPLIFIABLE = Self::INDEXED_ACCESS.bits() | Self::CONDITIONAL.bits() | Self::INDEX.bits();
        /// Singleton intrinsic types.
        const SINGLETON = Self::ANY.bits() | Self::UNKNOWN.bits() | Self::STRING.bits() | Self::NUMBER.bits() | Self::BOOLEAN.bits() | Self::BIG_INT.bits() | Self::ES_SYMBOL.bits() | Self::VOID.bits() | Self::UNDEFINED.bits() | Self::NULL.bits() | Self::NEVER.bits() | Self::NON_PRIMITIVE.bits();
        /// Types where narrowing actually narrows.
        const NARROWABLE = Self::ANY.bits() | Self::UNKNOWN.bits() | Self::STRUCTURED_OR_INSTANTIABLE.bits() | Self::STRING_LIKE.bits() | Self::NUMBER_LIKE.bits() | Self::BIG_INT_LIKE.bits() | Self::BOOLEAN_LIKE.bits() | Self::ES_SYMBOL.bits() | Self::UNIQUE_ES_SYMBOL.bits() | Self::NON_PRIMITIVE.bits();
        /// Flags aggregated during union/intersection construction.
        const INCLUDES_MASK = Self::ANY.bits() | Self::UNKNOWN.bits() | Self::PRIMITIVE.bits() | Self::NEVER.bits() | Self::OBJECT.bits() | Self::UNION.bits() | Self::INTERSECTION.bits() | Self::NON_PRIMITIVE.bits() | Self::TEMPLATE_LITERAL.bits() | Self::STRING_MAPPING.bits();
        /// Repurposed during construction: includes a missing type.
        const INCLUDES_MISSING_TYPE = Self::TYPE_PARAMETER.bits();
        /// Repurposed during construction: includes a non-widening type.
        const INCLUDES_NON_WIDENING_TYPE = Self::INDEX.bits();
        /// Repurposed during construction: includes a wildcard.
        const INCLUDES_WILDCARD = Self::INDEXED_ACCESS.bits();
        /// Repurposed during construction: includes an empty object.
        const INCLUDES_EMPTY_OBJECT = Self::CONDITIONAL.bits();
        /// Repurposed during construction: includes an instantiable type.
        const INCLUDES_INSTANTIABLE = Self::SUBSTITUTION.bits();
        /// Repurposed during construction: includes a constrained type variable.
        const INCLUDES_CONSTRAINED_TYPE_VARIABLE = Self::RESERVED1.bits();
        /// Repurposed during construction: includes an error type.
        const INCLUDES_ERROR = Self::RESERVED2.bits();
        /// Types that prevent forming a primitive union.
        const NOT_PRIMITIVE_UNION = Self::ANY.bits() | Self::UNKNOWN.bits() | Self::VOID.bits() | Self::NEVER.bits() | Self::OBJECT.bits() | Self::INTERSECTION.bits() | Self::INCLUDES_INSTANTIABLE.bits();
    }
}

bitflags::bitflags! {
    /// Flags on object/structured types (and a few reused on unions, etc.).
    ///
    /// Mirrors Go `ObjectFlags`. Round 4a ports the lower, position-stable
    /// "kind" bits (`1<<0..=1<<21`) plus the masks needed by type construction.
    ///
    /// DEFER(phase-4-checker-4b): the high context-dependent bits (`1<<22..`),
    /// which Go reuses for different meanings depending on the owning
    /// `TypeFlags` (object vs union vs intersection vs substitution), are not
    /// ported yet; they are introduced alongside the object/union/intersection
    /// type builders that consume them.
    /// blocked-by: object/union/intersection type construction lands in 4b/4d,
    /// so the overlapping high-bit ObjectFlags have no consumer in 4a.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::ObjectFlags;
    /// assert_eq!(ObjectFlags::CLASS.bits(), 1);
    /// assert_eq!(
    ///     ObjectFlags::CLASS_OR_INTERFACE,
    ///     ObjectFlags::CLASS | ObjectFlags::INTERFACE
    /// );
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/checker/types.go:ObjectFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct ObjectFlags: u32 {
        /// A class type.
        const CLASS = 1 << 0;
        /// An interface type.
        const INTERFACE = 1 << 1;
        /// A generic type reference.
        const REFERENCE = 1 << 2;
        /// A synthesized generic tuple type.
        const TUPLE = 1 << 3;
        /// An anonymous type.
        const ANONYMOUS = 1 << 4;
        /// A mapped type.
        const MAPPED = 1 << 5;
        /// An instantiated anonymous or mapped type.
        const INSTANTIATED = 1 << 6;
        /// Originates in an object literal.
        const OBJECT_LITERAL = 1 << 7;
        /// An evolving array type.
        const EVOLVING_ARRAY = 1 << 8;
        /// An object-literal binding pattern with computed properties.
        const OBJECT_LITERAL_PATTERN_WITH_COMPUTED_PROPERTIES = 1 << 9;
        /// Contains a property from a reverse-mapped type.
        const REVERSE_MAPPED = 1 << 10;
        /// A JSX attributes type.
        const JSX_ATTRIBUTES = 1 << 11;
        /// An object type declared in JS (relaxed member checks).
        const JS_LITERAL = 1 << 12;
        /// A fresh object literal.
        const FRESH_LITERAL = 1 << 13;
        /// Originates in an array literal.
        const ARRAY_LITERAL = 1 << 14;
        /// A union of only primitive types.
        const PRIMITIVE_UNION = 1 << 15;
        /// Is or contains an `undefined`/`null` widening type.
        const CONTAINS_WIDENING_TYPE = 1 << 16;
        /// Is or contains an object/array literal type.
        const CONTAINS_OBJECT_OR_ARRAY_LITERAL = 1 << 17;
        /// Is or contains a non-inferrable (any-function/silent-never) type.
        const NON_INFERRABLE_TYPE = 1 << 18;
        /// The `CouldContainTypeVariables` flag has been computed.
        const COULD_CONTAIN_TYPE_VARIABLES_COMPUTED = 1 << 19;
        /// The type could contain a type variable.
        const COULD_CONTAIN_TYPE_VARIABLES = 1 << 20;
        /// Members have been resolved.
        const MEMBERS_RESOLVED = 1 << 21;

        /// Class or interface.
        const CLASS_OR_INTERFACE = Self::CLASS.bits() | Self::INTERFACE.bits();
        /// Flags that force widening.
        const REQUIRES_WIDENING = Self::CONTAINS_WIDENING_TYPE.bits() | Self::CONTAINS_OBJECT_OR_ARRAY_LITERAL.bits();
        /// Flags propagated when composing types.
        const PROPAGATING_FLAGS = Self::CONTAINS_WIDENING_TYPE.bits() | Self::CONTAINS_OBJECT_OR_ARRAY_LITERAL.bits() | Self::NON_INFERRABLE_TYPE.bits();
        /// An instantiated mapped type.
        const INSTANTIATED_MAPPED = Self::MAPPED.bits() | Self::INSTANTIATED.bits();
        /// Cache flags cleared when a type is first allocated (see [`super::Checker::new_type`]).
        const FRESH_ALLOCATION_CLEARED = Self::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED.bits() | Self::COULD_CONTAIN_TYPE_VARIABLES.bits() | Self::MEMBERS_RESOLVED.bits();
    }
}

/// One entry of the flag-name table used by [`format_type_flags`].
struct TypeFlagName {
    flag: TypeFlags,
    name: &'static str,
}

/// The single-bit flag names, in the exact order Go iterates them so the output
/// of [`format_type_flags`] matches `FormatTypeFlags` byte for byte.
// Go: internal/checker/types.go:typeFlagNames
const TYPE_FLAG_NAMES: &[TypeFlagName] = &[
    TypeFlagName {
        flag: TypeFlags::ANY,
        name: "Any",
    },
    TypeFlagName {
        flag: TypeFlags::UNKNOWN,
        name: "Unknown",
    },
    TypeFlagName {
        flag: TypeFlags::UNDEFINED,
        name: "Undefined",
    },
    TypeFlagName {
        flag: TypeFlags::NULL,
        name: "Null",
    },
    TypeFlagName {
        flag: TypeFlags::VOID,
        name: "Void",
    },
    TypeFlagName {
        flag: TypeFlags::STRING,
        name: "String",
    },
    TypeFlagName {
        flag: TypeFlags::NUMBER,
        name: "Number",
    },
    TypeFlagName {
        flag: TypeFlags::BIG_INT,
        name: "BigInt",
    },
    TypeFlagName {
        flag: TypeFlags::BOOLEAN,
        name: "Boolean",
    },
    TypeFlagName {
        flag: TypeFlags::ES_SYMBOL,
        name: "ESSymbol",
    },
    TypeFlagName {
        flag: TypeFlags::STRING_LITERAL,
        name: "StringLiteral",
    },
    TypeFlagName {
        flag: TypeFlags::NUMBER_LITERAL,
        name: "NumberLiteral",
    },
    TypeFlagName {
        flag: TypeFlags::BIG_INT_LITERAL,
        name: "BigIntLiteral",
    },
    TypeFlagName {
        flag: TypeFlags::BOOLEAN_LITERAL,
        name: "BooleanLiteral",
    },
    TypeFlagName {
        flag: TypeFlags::UNIQUE_ES_SYMBOL,
        name: "UniqueESSymbol",
    },
    TypeFlagName {
        flag: TypeFlags::ENUM_LITERAL,
        name: "EnumLiteral",
    },
    TypeFlagName {
        flag: TypeFlags::ENUM,
        name: "Enum",
    },
    TypeFlagName {
        flag: TypeFlags::NON_PRIMITIVE,
        name: "NonPrimitive",
    },
    TypeFlagName {
        flag: TypeFlags::NEVER,
        name: "Never",
    },
    TypeFlagName {
        flag: TypeFlags::TYPE_PARAMETER,
        name: "TypeParameter",
    },
    TypeFlagName {
        flag: TypeFlags::OBJECT,
        name: "Object",
    },
    TypeFlagName {
        flag: TypeFlags::INDEX,
        name: "Index",
    },
    TypeFlagName {
        flag: TypeFlags::TEMPLATE_LITERAL,
        name: "TemplateLiteral",
    },
    TypeFlagName {
        flag: TypeFlags::STRING_MAPPING,
        name: "StringMapping",
    },
    TypeFlagName {
        flag: TypeFlags::SUBSTITUTION,
        name: "Substitution",
    },
    TypeFlagName {
        flag: TypeFlags::INDEXED_ACCESS,
        name: "IndexedAccess",
    },
    TypeFlagName {
        flag: TypeFlags::CONDITIONAL,
        name: "Conditional",
    },
    TypeFlagName {
        flag: TypeFlags::UNION,
        name: "Union",
    },
    TypeFlagName {
        flag: TypeFlags::INTERSECTION,
        name: "Intersection",
    },
];

/// Returns the individual set flag names, in canonical order.
///
/// When no named flag is set, returns the single entry `"None"` (matching Go's
/// `FormatTypeFlags`). The reserved bits have no name and are skipped, exactly
/// as in the Go table.
///
/// # Examples
/// ```
/// use tsgo_checker::{format_type_flags, TypeFlags};
/// assert_eq!(format_type_flags(TypeFlags::STRING), vec!["String".to_string()]);
/// assert_eq!(
///     format_type_flags(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL),
///     vec!["StringLiteral".to_string(), "NumberLiteral".to_string()],
/// );
/// assert_eq!(format_type_flags(TypeFlags::empty()), vec!["None".to_string()]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/checker/types.go:FormatTypeFlags
pub fn format_type_flags(flags: TypeFlags) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for fname in TYPE_FLAG_NAMES {
        if flags.intersects(fname.flag) {
            result.push(fname.name.to_string());
        }
    }
    if result.is_empty() {
        result.push("None".to_string());
    }
    result
}

/// A handle into a [`TypeArena`], replacing Go's `*Type`.
///
/// Ids are assigned sequentially starting at `1` (matching Go's `TypeCount`,
/// which is pre-incremented before each assignment), so `TypeId(0)` never names
/// a real type.
///
/// # Examples
/// ```
/// use tsgo_checker::TypeId;
/// assert_eq!(TypeId(1).arena_index(), 0);
/// assert_ne!(TypeId(1), TypeId(2));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:TypeId
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct TypeId(pub u32);

impl TypeId {
    /// Returns the zero-based `Vec` subscript for this id (`id - 1`).
    ///
    /// Side effects: none (pure).
    pub fn arena_index(self) -> usize {
        (self.0 - 1) as usize
    }
}

/// The payload of an intrinsic type such as `any`, `string`, or `never`.
///
/// # Examples
/// ```
/// use tsgo_checker::IntrinsicType;
/// let d = IntrinsicType { intrinsic_name: "string".to_string() };
/// assert_eq!(d.intrinsic_name, "string");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:IntrinsicType
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntrinsicType {
    /// The printable name of the intrinsic (e.g. `"string"`, `"any"`).
    pub intrinsic_name: String,
}

/// The value of a literal type, the closed form of Go's `LiteralType.value any`.
///
/// # Examples
/// ```
/// use tsgo_checker::LiteralValue;
/// assert_eq!(LiteralValue::Boolean(true), LiteralValue::Boolean(true));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:LiteralType.value (any)
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralValue {
    /// A string literal value.
    String(String),
    /// A numeric literal value (JS `number` semantics, via `tsgo_jsnum`).
    Number(tsgo_jsnum::Number),
    /// A boolean literal value (`true` / `false`).
    Boolean(bool),
}

/// The payload of a literal type (`"a"`, `1`, `true`, `false`).
///
/// `fresh_type` / `regular_type` mirror Go's fresh/regular literal pairing: a
/// fresh literal type (from a literal expression) widens to its regular
/// counterpart used in declared positions. They reference type ids and may be
/// self-referential, so they are filled in after allocation.
///
/// # Examples
/// ```
/// use tsgo_checker::{LiteralType, LiteralValue};
/// let d = LiteralType { value: LiteralValue::Boolean(false), fresh_type: None, regular_type: None };
/// assert_eq!(d.value, LiteralValue::Boolean(false));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:LiteralType
#[derive(Clone, Debug, PartialEq)]
pub struct LiteralType {
    /// The literal's value.
    pub value: LiteralValue,
    /// The fresh version of this literal type, if linked.
    pub fresh_type: Option<TypeId>,
    /// The regular version of this literal type, if linked.
    pub regular_type: Option<TypeId>,
}

/// The payload of an object/interface/class type: its resolved members plus
/// call/construct signatures and index signatures.
///
/// This flattens Go's `ObjectType`/`StructuredType`/`InterfaceType`/
/// `TypeReference` layering into one struct: the resolved members and
/// signatures, the generic type parameters / `this` type of an interface
/// target, and the target + type arguments of a generic instantiation
/// (`Foo<string>`).
///
/// DEFER(phase-4-checker-4e+): mapped/reverse-mapped object kinds, the variance
/// cache, and base-constructor types are not modeled.
/// blocked-by: those object kinds land with inference/flow (4e/4f).
///
/// # Examples
/// ```
/// use tsgo_checker::ObjectType;
/// let o = ObjectType::default();
/// assert!(o.members.is_empty());
/// assert!(o.type_parameters.is_empty());
/// assert!(o.target.is_none());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:ObjectType/StructuredType/InterfaceType/TypeReference
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectType {
    /// Resolved own members, by name.
    pub members: SymbolTable,
    /// Resolved property symbols (the value members), in arbitrary order.
    pub properties: Vec<tsgo_ast::SymbolId>,
    /// Call signatures.
    pub call_signatures: Vec<SignatureId>,
    /// Construct signatures.
    pub construct_signatures: Vec<SignatureId>,
    /// Index signatures.
    pub index_infos: Vec<IndexInfoId>,
    /// Local type parameters of a generic interface/class target.
    pub type_parameters: Vec<TypeId>,
    /// The synthesized `this` type parameter, if any.
    pub this_type: Option<TypeId>,
    /// For a type reference (`Foo<...>`), the generic target type.
    pub target: Option<TypeId>,
    /// For a type reference, the type arguments applied to `target`.
    pub resolved_type_arguments: Vec<TypeId>,
    /// Base (extends) types whose members are inherited, by type id.
    pub base_types: Vec<TypeId>,
}

/// The payload of a union type (`A | B`), holding its constituents by id.
///
/// Members are stored deduplicated and sorted by [`TypeId`] (Go orders union
/// constituents by type id), so the printed form is deterministic.
///
/// # Examples
/// ```
/// use tsgo_checker::{TypeId, UnionType};
/// let d = UnionType { types: vec![TypeId(1), TypeId(2)] };
/// assert_eq!(d.types.len(), 2);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:UnionType
#[derive(Clone, Debug, PartialEq)]
pub struct UnionType {
    /// The union's constituent type ids, deduplicated and id-sorted.
    pub types: Vec<TypeId>,
}

/// The payload of a type parameter (`T` in `interface Foo<T>` or a generic
/// function). Includes the synthesized `this` type parameter of an interface.
///
/// # Examples
/// ```
/// use tsgo_checker::TypeParameter;
/// let tp = TypeParameter::default();
/// assert!(tp.symbol.is_none());
/// assert!(!tp.is_this_type);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:TypeParameter
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TypeParameter {
    /// The declaring symbol, if any.
    pub symbol: Option<tsgo_ast::SymbolId>,
    /// The resolved constraint (`extends ...`), if any.
    pub constraint: Option<TypeId>,
    /// Whether this is the synthesized `this` type parameter.
    pub is_this_type: bool,
}

/// Type-specific data, the discriminated-union form of Go's `TypeData`
/// interface (PORTING, section 3).
///
/// Round 4a modeled only the [`IntrinsicType`] variant; 4b adds [`LiteralType`]
/// and [`UnionType`]; 4c adds [`ObjectType`]; 4d adds [`TypeParameter`].
/// Intersection and the remaining variants are added in later sub-phases.
///
/// # Examples
/// ```
/// use tsgo_checker::{IntrinsicType, TypeData};
/// let d = TypeData::Intrinsic(IntrinsicType { intrinsic_name: "any".to_string() });
/// assert!(matches!(d, TypeData::Intrinsic(_)));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:TypeData
#[derive(Clone, Debug, PartialEq)]
pub enum TypeData {
    /// An intrinsic type (`any`/`unknown`/`string`/.../`never`/`object`).
    Intrinsic(IntrinsicType),
    /// A literal type (`"a"`, `1`, `true`, `false`).
    Literal(LiteralType),
    /// A union type (`A | B`).
    Union(UnionType),
    /// An object/interface/class/enum type (members + signatures).
    Object(ObjectType),
    /// A type parameter (`T`), including the interface `this` type.
    TypeParameter(TypeParameter),
}

/// A checker type: the common header (Go's `Type` struct fields) plus its
/// type-specific [`TypeData`].
///
/// Go's `checker *Checker` back-pointer is intentionally absent; the owning
/// [`Checker`](super::Checker) holds the arena and performs all type
/// operations (PORTING, section 5).
///
/// # Examples
/// ```
/// use tsgo_checker::{IntrinsicType, Type, TypeData, TypeFlags, TypeId, ObjectFlags};
/// let t = Type {
///     flags: TypeFlags::STRING,
///     object_flags: ObjectFlags::empty(),
///     id: TypeId(1),
///     symbol: None,
///     data: TypeData::Intrinsic(IntrinsicType { intrinsic_name: "string".to_string() }),
/// };
/// assert_eq!(t.intrinsic_name(), Some("string"));
/// assert!(t.flags().intersects(TypeFlags::STRING));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:Type
#[derive(Clone, Debug)]
pub struct Type {
    /// The type's kind flags.
    pub flags: TypeFlags,
    /// Object/structured-type flags.
    pub object_flags: ObjectFlags,
    /// This type's own id (equal to its arena slot index plus one).
    pub id: TypeId,
    /// The associated symbol, if any.
    pub symbol: Option<tsgo_ast::SymbolId>,
    /// Type-specific data.
    pub data: TypeData,
}

impl Type {
    /// Returns the type's kind flags.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:Type.Flags
    pub fn flags(&self) -> TypeFlags {
        self.flags
    }

    /// Returns the type's object flags.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:Type.ObjectFlags
    pub fn object_flags(&self) -> ObjectFlags {
        self.object_flags
    }

    /// Returns the type's id.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:Type.Id
    pub fn id(&self) -> TypeId {
        self.id
    }

    /// Returns the intrinsic name if this is an intrinsic type, else `None`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{IntrinsicType, Type, TypeData, TypeFlags, TypeId, ObjectFlags};
    /// let t = Type {
    ///     flags: TypeFlags::NEVER,
    ///     object_flags: ObjectFlags::empty(),
    ///     id: TypeId(1),
    ///     symbol: None,
    ///     data: TypeData::Intrinsic(IntrinsicType { intrinsic_name: "never".to_string() }),
    /// };
    /// assert_eq!(t.intrinsic_name(), Some("never"));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:IntrinsicType.IntrinsicName
    pub fn intrinsic_name(&self) -> Option<&str> {
        match &self.data {
            TypeData::Intrinsic(d) => Some(&d.intrinsic_name),
            _ => None,
        }
    }

    /// Returns the literal value if this is a literal type, else `None`.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:LiteralType.value
    pub fn literal_value(&self) -> Option<&LiteralValue> {
        match &self.data {
            TypeData::Literal(d) => Some(&d.value),
            _ => None,
        }
    }

    /// Returns the constituent type ids if this is a union type, else `None`.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:UnionType.types
    pub fn union_types(&self) -> Option<&[TypeId]> {
        match &self.data {
            TypeData::Union(d) => Some(&d.types),
            _ => None,
        }
    }

    /// Returns the object payload if this is an object/interface type, else
    /// `None`.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:Type.AsObjectType
    pub fn as_object(&self) -> Option<&ObjectType> {
        match &self.data {
            TypeData::Object(d) => Some(d),
            _ => None,
        }
    }

    /// Returns a mutable reference to the object payload, if any.
    ///
    /// Side effects: allows mutation of the object payload.
    // Go: internal/checker/types.go:Type.AsObjectType
    pub fn as_object_mut(&mut self) -> Option<&mut ObjectType> {
        match &mut self.data {
            TypeData::Object(d) => Some(d),
            _ => None,
        }
    }

    /// Returns the type-parameter payload, if this is a type parameter.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:Type.AsTypeParameter
    pub fn as_type_parameter(&self) -> Option<&TypeParameter> {
        match &self.data {
            TypeData::TypeParameter(d) => Some(d),
            _ => None,
        }
    }
}

/// An arena owning every checker [`Type`], addressed by [`TypeId`].
///
/// Replaces Go's per-type heap allocation plus `Type.id`/`Checker.TypeCount`
/// counter: a type's id is its slot index plus one.
///
/// # Examples
/// ```
/// use tsgo_checker::{IntrinsicType, ObjectFlags, TypeArena, TypeData, TypeFlags, TypeId};
/// let mut arena = TypeArena::new();
/// let id = arena.alloc(
///     TypeFlags::STRING,
///     ObjectFlags::empty(),
///     None,
///     TypeData::Intrinsic(IntrinsicType { intrinsic_name: "string".to_string() }),
/// );
/// assert_eq!(id, TypeId(1));
/// assert_eq!(arena.len(), 1);
/// assert_eq!(arena.get(id).intrinsic_name(), Some("string"));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.typeArena / Checker.TypeCount
#[derive(Clone, Debug, Default)]
pub struct TypeArena {
    types: Vec<Type>,
}

impl TypeArena {
    /// Creates an empty type arena.
    ///
    /// Side effects: none (pure).
    pub fn new() -> Self {
        TypeArena::default()
    }

    /// Returns the number of allocated types.
    ///
    /// Side effects: none (pure).
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Reports whether no types have been allocated.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Allocates a new type, assigning it the next sequential [`TypeId`], and
    /// returns that id.
    ///
    /// Side effects: mutates `self` by appending one type.
    // Go: internal/checker/checker.go:Checker.newType (id assignment)
    pub fn alloc(
        &mut self,
        flags: TypeFlags,
        object_flags: ObjectFlags,
        symbol: Option<tsgo_ast::SymbolId>,
        data: TypeData,
    ) -> TypeId {
        let id = TypeId(self.types.len() as u32 + 1);
        self.types.push(Type {
            flags,
            object_flags,
            id,
            symbol,
            data,
        });
        id
    }

    /// Returns the type for `id`.
    ///
    /// # Panics
    /// Panics if `id` does not name an allocated type (Go would index out of
    /// range identically).
    ///
    /// Side effects: none (pure).
    pub fn get(&self, id: TypeId) -> &Type {
        &self.types[id.arena_index()]
    }

    /// Returns a mutable reference to the type for `id`.
    ///
    /// # Panics
    /// Panics if `id` does not name an allocated type.
    ///
    /// Side effects: mutates the referenced type via the returned reference.
    pub fn get_mut(&mut self, id: TypeId) -> &mut Type {
        &mut self.types[id.arena_index()]
    }
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
