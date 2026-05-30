//! Port of Go `internal/pseudochecker/type.go`.
//!
//! `PseudoType`s are skeletons of types: partially interpreted expressions and
//! type nodes composed to describe how a type *should* be constructed from
//! them. A real `Checker` can trivially map them into actual types, and a
//! `NodeBuilder` can map them straight into a tree of `Node`s without ever
//! materializing an intermediate type. Unlike checker `Type`s, these are never
//! normalized, and several pseudo-types may refer to the same underlying type.

use tsgo_ast::NodeId;

/// Discriminant tag for a [`PseudoType`], mirroring Go's `PseudoTypeKind` iota.
///
/// The Rust `PseudoType` enum is itself a discriminated union, so this tag is
/// redundant for matching; it exists to preserve the upstream ordering and to
/// give consumers a lightweight, `Copy` classifier.
///
/// # Examples
/// ```
/// use tsgo_pseudochecker::PseudoTypeKind;
/// assert_eq!(PseudoTypeKind::Direct as i16, 0);
/// assert_eq!(PseudoTypeKind::BigIntLiteral as i16, 19);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/pseudochecker/type.go:PseudoTypeKind
#[repr(i16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoTypeKind {
    /// Directly references a type node.
    Direct = 0,
    /// References an expression too complex for the pseudochecker.
    Inferred,
    /// References a declaration/return position too complex to infer.
    NoResult,
    /// Carries both the const and regular interpretation of a location.
    MaybeConstLocation,
    /// A union of pseudo-types.
    Union,
    /// The `undefined` type.
    Undefined,
    /// The `null` type.
    Null,
    /// The `any` type.
    Any,
    /// The `string` type.
    String,
    /// The `number` type.
    Number,
    /// The `bigint` type.
    BigInt,
    /// The `boolean` type.
    Boolean,
    /// The `false` literal type.
    False,
    /// The `true` literal type.
    True,
    /// An object type with a single call signature (arrow/function expression).
    SingleCallSignature,
    /// A tuple type from an `as const` array literal.
    Tuple,
    /// An object type from an object literal.
    ObjectLiteral,
    /// A string literal type.
    StringLiteral,
    /// A numeric literal type.
    NumericLiteral,
    /// A big-int literal type.
    BigIntLiteral,
}

/// A skeleton type produced by the [`PseudoChecker`](crate::PseudoChecker).
///
/// This is the Rust form of Go's `PseudoType{ Kind; data pseudoTypeData }`: the
/// `Kind`/`data` pair collapses into a single discriminated `enum`. No-payload
/// variants replace Go's global singletons (`PseudoTypeUndefined`, ...), and
/// child pseudo-types are owned via `Box`/`Vec` because the skeleton is a
/// short-lived, acyclic tree (it stores only [`NodeId`]s, never AST nodes).
///
/// # Examples
/// ```
/// use tsgo_pseudochecker::{PseudoType, PseudoTypeKind};
/// assert_eq!(PseudoType::Undefined.kind(), PseudoTypeKind::Undefined);
/// assert_eq!(PseudoType::union(vec![]).kind(), PseudoTypeKind::Union);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/pseudochecker/type.go:PseudoType
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PseudoType {
    /// The `undefined` type (Go `PseudoTypeUndefined`).
    Undefined,
    /// The `null` type (Go `PseudoTypeNull`).
    Null,
    /// The `any` type (Go `PseudoTypeAny`).
    Any,
    /// The `string` type (Go `PseudoTypeString`).
    String,
    /// The `number` type (Go `PseudoTypeNumber`).
    Number,
    /// The `bigint` type (Go `PseudoTypeBigInt`).
    BigInt,
    /// The `boolean` type (Go `PseudoTypeBoolean`).
    Boolean,
    /// The `false` literal type (Go `PseudoTypeFalse`).
    False,
    /// The `true` literal type (Go `PseudoTypeTrue`).
    True,
    /// Directly encodes the type referred to by a type node.
    Direct {
        /// The referenced type node.
        type_node: NodeId,
    },
    /// References an expression too complex for the pseudochecker. Most such
    /// locations error under ID; specific blocking nodes are recorded in
    /// `error_nodes`.
    Inferred {
        /// The expression that could not be reduced to a skeleton.
        expression: NodeId,
        /// Specific error nodes (shorthand/spread/etc.) collected during typing.
        error_nodes: Vec<NodeId>,
    },
    /// References a declaration/return position too complex to infer (the
    /// signature/declaration analogue of [`PseudoType::Inferred`]).
    NoResult {
        /// The declaration whose type could not be inferred.
        declaration: NodeId,
    },
    /// Carries both the const and regular interpretation of a location so the
    /// builder can pick the right one once it knows the location's context.
    MaybeConstLocation {
        /// The location node this ambiguity applies to.
        node: NodeId,
        /// The interpretation used in a const context.
        const_type: Box<PseudoType>,
        /// The interpretation used in a regular context.
        regular_type: Box<PseudoType>,
    },
    /// A union of pseudo-types.
    Union(Vec<PseudoType>),
    /// An object type with a single call signature (arrow/function expression).
    SingleCallSignature {
        /// The signature-bearing node.
        signature: NodeId,
        /// The (cloned) parameters of the signature.
        parameters: Vec<PseudoParameter>,
        /// The (cloned) type parameters of the signature.
        type_parameters: Vec<NodeId>,
        /// The signature's return type.
        return_type: Box<PseudoType>,
    },
    /// A tuple type originating from an `as const` array literal.
    Tuple {
        /// The tuple element types, in order.
        elements: Vec<PseudoType>,
    },
    /// An object type originating from an object literal.
    ObjectLiteral {
        /// The object members, in source order.
        elements: Vec<PseudoObjectElement>,
    },
    /// A string literal type referring to the literal node.
    StringLiteral(NodeId),
    /// A numeric literal type referring to the literal node.
    NumericLiteral(NodeId),
    /// A big-int literal type referring to the literal node.
    BigIntLiteral(NodeId),
}

impl PseudoType {
    /// Builds a [`PseudoType::Direct`] referencing `type_node`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// use tsgo_pseudochecker::{PseudoType, PseudoTypeKind};
    /// let mut arena = NodeArena::new();
    /// let t = arena.new_identifier("T");
    /// assert_eq!(PseudoType::direct(t).kind(), PseudoTypeKind::Direct);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeDirect
    pub fn direct(type_node: NodeId) -> PseudoType {
        PseudoType::Direct { type_node }
    }

    /// Builds a [`PseudoType::Inferred`] for `expression` with no error nodes.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// use tsgo_pseudochecker::{PseudoType, PseudoTypeKind};
    /// let mut arena = NodeArena::new();
    /// let e = arena.new_identifier("e");
    /// assert_eq!(PseudoType::inferred(e).kind(), PseudoTypeKind::Inferred);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeInferred
    pub fn inferred(expression: NodeId) -> PseudoType {
        PseudoType::Inferred {
            expression,
            error_nodes: Vec::new(),
        }
    }

    /// Builds a [`PseudoType::Inferred`] for `expression` carrying the blocking
    /// `error_nodes` (shorthand/spread properties, non-literal computed names,
    /// non-const arrays, ...).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// use tsgo_pseudochecker::PseudoType;
    /// let mut arena = NodeArena::new();
    /// let e = arena.new_identifier("e");
    /// let bad = arena.new_identifier("bad");
    /// assert!(matches!(
    ///     PseudoType::inferred_with_errors(e, vec![bad]),
    ///     PseudoType::Inferred { error_nodes, .. } if error_nodes.len() == 1
    /// ));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeInferredWithErrors
    pub fn inferred_with_errors(expression: NodeId, error_nodes: Vec<NodeId>) -> PseudoType {
        PseudoType::Inferred {
            expression,
            error_nodes,
        }
    }

    /// Builds a [`PseudoType::NoResult`] for `declaration`.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeNoResult
    pub fn no_result(declaration: NodeId) -> PseudoType {
        PseudoType::NoResult { declaration }
    }

    /// Builds a [`PseudoType::MaybeConstLocation`] over `node`, boxing the
    /// const/regular interpretations.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeMaybeConstLocation
    pub fn maybe_const_location(
        node: NodeId,
        const_type: PseudoType,
        regular_type: PseudoType,
    ) -> PseudoType {
        PseudoType::MaybeConstLocation {
            node,
            const_type: Box::new(const_type),
            regular_type: Box::new(regular_type),
        }
    }

    /// Builds a [`PseudoType::Union`] from `types`.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeUnion
    pub fn union(types: Vec<PseudoType>) -> PseudoType {
        PseudoType::Union(types)
    }

    /// Builds a [`PseudoType::StringLiteral`] referencing the literal `node`.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeStringLiteral
    pub fn string_literal(node: NodeId) -> PseudoType {
        PseudoType::StringLiteral(node)
    }

    /// Builds a [`PseudoType::NumericLiteral`] referencing the literal `node`.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeNumericLiteral
    pub fn numeric_literal(node: NodeId) -> PseudoType {
        PseudoType::NumericLiteral(node)
    }

    /// Builds a [`PseudoType::BigIntLiteral`] referencing the literal `node`.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeBigIntLiteral
    pub fn bigint_literal(node: NodeId) -> PseudoType {
        PseudoType::BigIntLiteral(node)
    }

    /// Builds a [`PseudoType::SingleCallSignature`].
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeSingleCallSignature
    pub fn single_call_signature(
        signature: NodeId,
        parameters: Vec<PseudoParameter>,
        type_parameters: Vec<NodeId>,
        return_type: PseudoType,
    ) -> PseudoType {
        PseudoType::SingleCallSignature {
            signature,
            parameters,
            type_parameters,
            return_type: Box::new(return_type),
        }
    }

    /// Builds a [`PseudoType::Tuple`] from `elements`.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeTuple
    pub fn tuple(elements: Vec<PseudoType>) -> PseudoType {
        PseudoType::Tuple { elements }
    }

    /// Builds a [`PseudoType::ObjectLiteral`] from `elements`.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoTypeObjectLiteral
    pub fn object_literal(elements: Vec<PseudoObjectElement>) -> PseudoType {
        PseudoType::ObjectLiteral { elements }
    }

    /// Returns the [`PseudoTypeKind`] discriminant of this pseudo-type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_pseudochecker::{PseudoType, PseudoTypeKind};
    /// assert_eq!(PseudoType::True.kind(), PseudoTypeKind::True);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:PseudoType.Kind (field access)
    pub fn kind(&self) -> PseudoTypeKind {
        match self {
            PseudoType::Undefined => PseudoTypeKind::Undefined,
            PseudoType::Null => PseudoTypeKind::Null,
            PseudoType::Any => PseudoTypeKind::Any,
            PseudoType::String => PseudoTypeKind::String,
            PseudoType::Number => PseudoTypeKind::Number,
            PseudoType::BigInt => PseudoTypeKind::BigInt,
            PseudoType::Boolean => PseudoTypeKind::Boolean,
            PseudoType::False => PseudoTypeKind::False,
            PseudoType::True => PseudoTypeKind::True,
            PseudoType::Direct { .. } => PseudoTypeKind::Direct,
            PseudoType::Inferred { .. } => PseudoTypeKind::Inferred,
            PseudoType::NoResult { .. } => PseudoTypeKind::NoResult,
            PseudoType::MaybeConstLocation { .. } => PseudoTypeKind::MaybeConstLocation,
            PseudoType::Union(_) => PseudoTypeKind::Union,
            PseudoType::SingleCallSignature { .. } => PseudoTypeKind::SingleCallSignature,
            PseudoType::Tuple { .. } => PseudoTypeKind::Tuple,
            PseudoType::ObjectLiteral { .. } => PseudoTypeKind::ObjectLiteral,
            PseudoType::StringLiteral(_) => PseudoTypeKind::StringLiteral,
            PseudoType::NumericLiteral(_) => PseudoTypeKind::NumericLiteral,
            PseudoType::BigIntLiteral(_) => PseudoTypeKind::BigIntLiteral,
        }
    }
}

/// One parameter of a pseudo call signature.
///
/// Side effects: none (pure value type).
// Go: internal/pseudochecker/type.go:PseudoParameter
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PseudoParameter {
    /// Whether this is a rest parameter (`...p`).
    pub rest: bool,
    /// The parameter's name node.
    pub name: NodeId,
    /// Whether the parameter is optional.
    pub optional: bool,
    /// The parameter's type.
    pub type_: Box<PseudoType>,
}

impl PseudoParameter {
    /// Builds a [`PseudoParameter`], boxing its type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// use tsgo_pseudochecker::{PseudoParameter, PseudoType};
    /// let mut arena = NodeArena::new();
    /// let p = arena.new_identifier("p");
    /// let param = PseudoParameter::new(false, p, true, PseudoType::Any);
    /// assert!(param.optional);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoParameter
    pub fn new(rest: bool, name: NodeId, optional: bool, type_: PseudoType) -> PseudoParameter {
        PseudoParameter {
            rest,
            name,
            optional,
            type_: Box::new(type_),
        }
    }
}

/// Discriminant tag for a [`PseudoObjectElement`], mirroring Go's
/// `PseudoObjectElementKind` iota.
///
/// Side effects: none (pure value type).
// Go: internal/pseudochecker/type.go:PseudoObjectElementKind
#[repr(i8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoObjectElementKind {
    /// A method member.
    Method = 0,
    /// A property assignment member.
    PropertyAssignment,
    /// A `set` accessor member.
    SetAccessor,
    /// A `get` accessor member.
    GetAccessor,
}

/// A member of a pseudo object-literal type.
///
/// Go models this as a `Kind`/`data` pair embedding a shared
/// `PseudoObjectElement` base (`Name`/`Optional`); the Rust form is a
/// discriminated `enum` whose variants each carry the shared `name`/`optional`.
///
/// Side effects: none (pure value type).
// Go: internal/pseudochecker/type.go:PseudoObjectElement
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PseudoObjectElement {
    /// A method member (`m() {}`).
    Method {
        /// The member name node.
        name: NodeId,
        /// Whether the member is optional (`m?()`).
        optional: bool,
        /// The signature-bearing node.
        signature: NodeId,
        /// The (cloned) type parameters of the method.
        type_parameters: Vec<NodeId>,
        /// The (cloned) parameters of the method.
        parameters: Vec<PseudoParameter>,
        /// The method's return type.
        return_type: Box<PseudoType>,
    },
    /// A property assignment member (`p: v`).
    PropertyAssignment {
        /// The member name node.
        name: NodeId,
        /// Whether the member is optional (`p?: v`).
        optional: bool,
        /// Whether the property is `readonly`.
        readonly: bool,
        /// The property's type.
        type_: Box<PseudoType>,
    },
    /// A `set` accessor member.
    SetAccessor {
        /// The member name node.
        name: NodeId,
        /// Whether the member is optional.
        optional: bool,
        /// The signature-bearing node.
        signature: NodeId,
        /// The accessor's single parameter.
        parameter: Box<PseudoParameter>,
    },
    /// A `get` accessor member.
    GetAccessor {
        /// The member name node.
        name: NodeId,
        /// Whether the member is optional.
        optional: bool,
        /// The signature-bearing node.
        signature: NodeId,
        /// The accessor's type.
        type_: Box<PseudoType>,
    },
}

impl PseudoObjectElement {
    /// Builds a [`PseudoObjectElement::Method`].
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoObjectMethod
    pub fn method(
        signature: NodeId,
        name: NodeId,
        optional: bool,
        type_parameters: Vec<NodeId>,
        parameters: Vec<PseudoParameter>,
        return_type: PseudoType,
    ) -> PseudoObjectElement {
        PseudoObjectElement::Method {
            name,
            optional,
            signature,
            type_parameters,
            parameters,
            return_type: Box::new(return_type),
        }
    }

    /// Builds a [`PseudoObjectElement::PropertyAssignment`].
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoPropertyAssignment
    pub fn property_assignment(
        readonly: bool,
        name: NodeId,
        optional: bool,
        type_: PseudoType,
    ) -> PseudoObjectElement {
        PseudoObjectElement::PropertyAssignment {
            name,
            optional,
            readonly,
            type_: Box::new(type_),
        }
    }

    /// Builds a [`PseudoObjectElement::SetAccessor`].
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoSetAccessor
    pub fn set_accessor(
        signature: NodeId,
        name: NodeId,
        optional: bool,
        parameter: PseudoParameter,
    ) -> PseudoObjectElement {
        PseudoObjectElement::SetAccessor {
            name,
            optional,
            signature,
            parameter: Box::new(parameter),
        }
    }

    /// Builds a [`PseudoObjectElement::GetAccessor`].
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:NewPseudoGetAccessor
    pub fn get_accessor(
        signature: NodeId,
        name: NodeId,
        optional: bool,
        type_: PseudoType,
    ) -> PseudoObjectElement {
        PseudoObjectElement::GetAccessor {
            name,
            optional,
            signature,
            type_: Box::new(type_),
        }
    }

    /// Returns the [`PseudoObjectElementKind`] discriminant of this member.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:PseudoObjectElement.Kind (field access)
    pub fn kind(&self) -> PseudoObjectElementKind {
        match self {
            PseudoObjectElement::Method { .. } => PseudoObjectElementKind::Method,
            PseudoObjectElement::PropertyAssignment { .. } => {
                PseudoObjectElementKind::PropertyAssignment
            }
            PseudoObjectElement::SetAccessor { .. } => PseudoObjectElementKind::SetAccessor,
            PseudoObjectElement::GetAccessor { .. } => PseudoObjectElementKind::GetAccessor,
        }
    }

    /// Returns the member's name node (the shared `Name` field).
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:PseudoObjectElement.Name (field access)
    pub fn name(&self) -> NodeId {
        match self {
            PseudoObjectElement::Method { name, .. }
            | PseudoObjectElement::PropertyAssignment { name, .. }
            | PseudoObjectElement::SetAccessor { name, .. }
            | PseudoObjectElement::GetAccessor { name, .. } => *name,
        }
    }

    /// Returns whether the member is optional (the shared `Optional` field).
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:PseudoObjectElement.Optional (field access)
    pub fn optional(&self) -> bool {
        match self {
            PseudoObjectElement::Method { optional, .. }
            | PseudoObjectElement::PropertyAssignment { optional, .. }
            | PseudoObjectElement::SetAccessor { optional, .. }
            | PseudoObjectElement::GetAccessor { optional, .. } => *optional,
        }
    }

    /// Returns the signature node for method/accessor members, or `None` for a
    /// property assignment (mirrors Go's `Signature()` returning `nil`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// use tsgo_pseudochecker::{PseudoObjectElement, PseudoType};
    /// let mut arena = NodeArena::new();
    /// let name = arena.new_identifier("p");
    /// let prop = PseudoObjectElement::property_assignment(false, name, false, PseudoType::Any);
    /// assert_eq!(prop.signature(), None);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/type.go:PseudoObjectElement.Signature
    pub fn signature(&self) -> Option<NodeId> {
        match self {
            PseudoObjectElement::Method { signature, .. }
            | PseudoObjectElement::SetAccessor { signature, .. }
            | PseudoObjectElement::GetAccessor { signature, .. } => Some(*signature),
            PseudoObjectElement::PropertyAssignment { .. } => None,
        }
    }
}

#[cfg(test)]
#[path = "ptype_test.rs"]
mod tests;
