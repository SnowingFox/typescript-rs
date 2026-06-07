//! Emit resolver: the query surface the declaration/JS transformers (Phase 5)
//! ask the checker, ported from `emitresolver.go`.
//!
//! Go's `EmitResolver` holds a back-reference to the checker plus per-node
//! caches and a mutex. Following this crate's ownership model (no back-pointers,
//! single-threaded), [`EmitResolver`] is a lightweight handle whose query
//! methods take the [`BoundProgram`] (and, for type-backed queries, the
//! [`Checker`]) explicitly. [`Checker::get_emit_resolver`] caches one behind a
//! `OnceCell`, mirroring Go's `GetEmitResolver`.
//!
//! 4k ports the AST-structural core (declaration visibility) plus the
//! type-backed serialization entry points that reuse the 4j node builder. The
//! alias/reference/host-dependent queries are deferred (see the per-method
//! `// blocked-by:` notes).

use tsgo_ast::{Kind, ModifierFlags, NodeData, NodeFlags, NodeId, SymbolFlags, SymbolId};
use tsgo_evaluator::EvalValue;

use super::declared_types::{
    combined_node_flags, get_declared_type_of_symbol,
    get_enum_member_value as declared_enum_member_value, get_type_of_symbol,
};
use super::nodebuilder::{type_to_string, type_to_type_node, SynthesizedTypeNode};
use super::program::BoundProgram;
use super::symbols::resolve_name;
use super::symbols_query::get_symbol_of_declaration;
use super::types::{LiteralValue, TypeFlags};
use super::Checker;

/// The checker's emit-time query surface (Go's `EmitResolver`).
///
/// A zero-sized handle in 4k: visibility is computed structurally and the
/// type-backed queries take the [`Checker`] explicitly, so no
/// checker back-reference or interior caches are needed yet.
///
/// # Examples
/// ```
/// use tsgo_checker::Checker;
/// let c = Checker::new();
/// let _resolver = c.get_emit_resolver();
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/emitresolver.go:EmitResolver
#[derive(Clone, Copy, Debug, Default)]
pub struct EmitResolver;

/// The serialized runtime-constructor descriptor for a type-annotation node,
/// the value the legacy-decorator transform (Phase 5) turns into the second
/// argument of `__metadata("design:type", <Ctor>)`.
///
/// Go's `serializeTypeNode` builds the AST expression directly
/// (`s.f.NewIdentifier("Number")` / `s.f.NewVoidZeroExpression()` / ...); this
/// enum *names* that result so the transform can construct it. Each variant
/// maps to exactly one of Go's emitted forms: a global constructor identifier
/// or the `void 0` expression.
///
/// 4at ports the reachable keyword-type subset (see
/// [`EmitResolver::serialize_type_node_for_metadata`]); 4av adds the
/// [`Array`](Self::Array) (array/tuple types) and [`Function`](Self::Function)
/// (function/constructor types) variants. The type-reference and
/// union/intersection arms that Go also produces are still deferred.
///
/// # Examples
/// ```
/// use tsgo_checker::SerializedTypeNode;
/// // `: number` serializes to the global `Number` constructor.
/// assert_ne!(SerializedTypeNode::Number, SerializedTypeNode::Object);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SerializedTypeNode {
    /// The global `Number` constructor. Go: `s.f.NewIdentifier("Number")`.
    Number,
    /// The global `String` constructor. Go: `s.f.NewIdentifier("String")`.
    String,
    /// The global `Boolean` constructor. Go: `s.f.NewIdentifier("Boolean")`.
    Boolean,
    /// The global `BigInt` constructor. Go: `s.f.NewIdentifier("BigInt")`.
    BigInt,
    /// The global `Symbol` constructor. Go: `s.f.NewIdentifier("Symbol")`.
    Symbol,
    /// The global `Object` constructor — Go's catch-all "anything else"
    /// fallback. Go: `s.f.NewIdentifier("Object")`.
    Object,
    /// The global `Array` constructor, for an array or tuple type
    /// (`number[]` / `[number, string]`). Go: `s.f.NewIdentifier("Array")`
    /// (`case KindArrayType, KindTupleType`).
    Array,
    /// The global `Function` constructor, for a function or constructor type
    /// (`() => void` / `new () => C`). Go: `s.f.NewIdentifier("Function")`
    /// (`case KindFunctionType, KindConstructorType`).
    Function,
    /// The `void 0` expression (the "undefined" serialization).
    /// Go: `s.f.NewVoidZeroExpression()`.
    VoidZero,
}

/// How a `TypeReference` type node classifies for legacy-decorator
/// `design:type` emit (Go's `printer.TypeReferenceSerializationKind`).
///
/// The legacy-decorator transform (Phase 5) emits the constructor of a
/// referenced *value* (`: SomeClass` → the `SomeClass` identifier) when the
/// reference resolves to something reachable at runtime, and a safe fallback
/// otherwise. This enum names the classification the checker hands the
/// transform so it can pick the right emitted expression; the variants and
/// their order mirror Go's `iota` enum exactly.
///
/// 4aw ports the reachable single-file subset
/// (see [`EmitResolver::get_type_reference_serialization_kind`]): a local class
/// reference → [`TypeWithConstructSignatureAndValue`](Self::TypeWithConstructSignatureAndValue),
/// an interface/type-alias reference → [`ObjectType`](Self::ObjectType), and an
/// unresolved name → [`Unknown`](Self::Unknown). The lib-globals-dependent
/// kinds (`Promise`/`NumberLikeType`/`ArrayLikeType`/...) are still deferred.
///
/// # Examples
/// ```
/// use tsgo_checker::TypeReferenceSerializationKind;
/// // A reference whose entity cannot be resolved gets the safe fallback.
/// assert_ne!(
///     TypeReferenceSerializationKind::Unknown,
///     TypeReferenceSerializationKind::ObjectType,
/// );
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/printer/emitresolver.go:TypeReferenceSerializationKind
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeReferenceSerializationKind {
    /// The reference could not be resolved; the type name should be emitted
    /// using a safe fallback. Go: `TypeReferenceSerializationKindUnknown`.
    Unknown,
    /// The reference resolves to a type with a constructor function reachable at
    /// runtime (a `class` declaration, or a `var` for the static side of a type
    /// such as the global `Promise`). Go:
    /// `TypeReferenceSerializationKindTypeWithConstructSignatureAndValue`.
    TypeWithConstructSignatureAndValue,
    /// The reference resolves to a void-like, nullable, or never type. Go:
    /// `TypeReferenceSerializationKindVoidNullableOrNeverType`.
    VoidNullableOrNeverType,
    /// The reference resolves to a number-like type. Go:
    /// `TypeReferenceSerializationKindNumberLikeType`.
    NumberLikeType,
    /// The reference resolves to a bigint-like type. Go:
    /// `TypeReferenceSerializationKindBigIntLikeType`.
    BigIntLikeType,
    /// The reference resolves to a string-like type. Go:
    /// `TypeReferenceSerializationKindStringLikeType`.
    StringLikeType,
    /// The reference resolves to a boolean-like type. Go:
    /// `TypeReferenceSerializationKindBooleanType`.
    BooleanType,
    /// The reference resolves to an array-like type. Go:
    /// `TypeReferenceSerializationKindArrayLikeType`.
    ArrayLikeType,
    /// The reference resolves to the ESSymbol type. Go:
    /// `TypeReferenceSerializationKindESSymbolType`.
    ESSymbolType,
    /// The reference resolves to the global `Promise` constructor symbol. Go:
    /// `TypeReferenceSerializationKindPromise`.
    Promise,
    /// The reference resolves to a function type or a type with call signatures.
    /// Go: `TypeReferenceSerializationKindTypeWithCallSignature`.
    TypeWithCallSignature,
    /// The reference resolves to any other type. Go:
    /// `TypeReferenceSerializationKindObjectType`.
    ObjectType,
}

/// The constant *value expression* declaration emit keeps in place of a type for
/// a literal `const` (`declare const x = 1;`), the closed descriptor the
/// declaration transformer reconstructs into AST.
///
/// Go's `CreateLiteralConstValue` builds the expression directly in the emit
/// context's factory (`NewNumericLiteral` / `NewStringLiteral` /
/// `NewKeywordExpression`); like [`SynthesizedTypeNode`] this *names* that result
/// for the transformer to rebuild across the two-arena split.
///
/// 4 (D-F2) ports the reachable primitive-literal subset (number/string/
/// boolean). The enum-member (`SymbolToExpression`) and bigint arms are
/// deferred.
///
/// # Examples
/// ```
/// use tsgo_checker::LiteralConstValue;
/// assert_eq!(LiteralConstValue::Boolean(true), LiteralConstValue::Boolean(true));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/emitresolver.go:EmitResolver.CreateLiteralConstValue
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralConstValue {
    /// A numeric literal value (`1` / `-1`); `text` is the unsigned literal
    /// text, `negative` a leading unary minus.
    Number {
        /// The unsigned literal text.
        text: String,
        /// Whether the value is negative (a leading unary minus).
        negative: bool,
    },
    /// A string literal value (`"a"`); the unescaped text.
    String(String),
    /// A boolean literal value (`true`/`false`).
    Boolean(bool),
}

impl EmitResolver {
    /// Reports whether `node`'s declaration is visible to declaration emit
    /// (Go's `IsDeclarationVisible`).
    ///
    /// 4k/C-E port the reachable subset of Go's
    /// `determineIfDeclarationIsVisible` switch: a declaration kind (variable /
    /// module / class / interface / type-alias / function / enum / `import =`)
    /// is visible iff it is exported (by its **combined** modifier flags, which
    /// fold a variable declaration's wrapping `VariableStatement`'s `export` in)
    /// and its declaration container is visible (recursion). `SourceFile`,
    /// `NamespaceExportDeclaration`, and `TypeParameter` are always visible;
    /// import clauses/specifiers/namespace imports and `export =` are not.
    ///
    /// The global-script-file case is modelled (D-F3): a non-exported top-level
    /// declaration is visible in a non-module script (no external-module
    /// indicator) and not visible in a module, via [`is_global_source_file`].
    ///
    /// DEFER(phase-4-checker-post): ambient-module augmentation, the
    /// empty-binding-pattern variable carve-out, JS typedef tags, CommonJS
    /// module detection, and the property/method/accessor arms (which need
    /// `GetEffectiveDeclarationFlags`) are deferred.
    /// blocked-by: `compiler.Program` (P6) + CommonJS indicator +
    /// `GetEffectiveDeclarationFlags` (private/protected member visibility).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_declaration_visible(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsDeclarationVisible(104)
    pub fn is_declaration_visible(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        determine_if_declaration_is_visible(program.arena(), node)
    }

    /// Serializes the type of declaration `node` to its printed form (Go's
    /// `SerializeTypeOfDeclaration`), reusing the 4j node builder.
    ///
    /// Used by the declaration transformer to emit explicit type annotations.
    ///
    /// DEFER(phase-4-checker-post): emit the serialized type as a *node*
    /// (`createTypeOfDeclaration`) rather than a string, plus the
    /// widening/freshening and accessibility tracking the transformer needs.
    /// blocked-by: the full node builder + `SymbolTracker` and `compiler.Program`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> String {
    /// r.serialize_type_of_declaration(c, p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may resolve and cache the declaration's type.
    // Go: internal/checker/emitresolver.go:EmitResolver.SerializeTypeOfDeclaration
    pub fn serialize_type_of_declaration(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> String {
        let ty = match get_symbol_of_declaration(program, node) {
            Some(symbol) => get_type_of_symbol(checker, program, symbol, None),
            None => checker.error_type(),
        };
        type_to_string(checker, program, ty)
    }

    /// Synthesizes the *type node* declaration emit annotates `declaration` with
    /// when it has no explicit annotation (Go's `CreateTypeOfDeclaration`).
    ///
    /// Mirrors Go's `CreateTypeOfDeclaration` -> `SerializeTypeForDeclaration`:
    /// the declaration's symbol type is taken and *widened* to its base
    /// primitive (`getWidenedLiteralType(getTypeOfSymbol(symbol))`), then run
    /// through the node builder's [`type_to_type_node`]. So `let n = 1` yields a
    /// `number` keyword node, `const xs = [1, 2]` (with a global `Array`) a
    /// `number[]` array node, and `const o = { a: 1 }` a `{ a: number; }`
    /// type-literal. The result is a [`SynthesizedTypeNode`] descriptor the
    /// declaration transformer reconstructs into AST in its own arena.
    ///
    /// A node with no symbol, or whose type the reachable node builder cannot
    /// serialize, falls back to the `any` keyword (Go's
    /// `serializeTypeForDeclaration` `result == nil` tail).
    ///
    /// DEFER(phase-5-emit-D-F3): the accessor write-type path
    /// (`getWriteTypeOfSymbol`), the `enclosingSymbolTypes` reuse, the
    /// `requiresAddingImplicitUndefined` optional-parameter widening, the
    /// pseudo-type annotation reuse (`tryReuse`), the `instantiateType` mapper,
    /// and the `SymbolTracker` accessibility reporting. blocked-by: accessor
    /// write types + pseudochecker reuse + the symbol tracker (D-F3).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver, SynthesizedTypeNode};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> Option<SynthesizedTypeNode> {
    /// r.create_type_of_declaration(c, p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may resolve and cache the declaration's type.
    // Go: internal/checker/emitresolver.go:EmitResolver.CreateTypeOfDeclaration
    pub fn create_type_of_declaration(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Option<SynthesizedTypeNode> {
        // Go: symbol = getSymbolOfDeclaration(declaration); then
        // serializeTypeForDeclaration computes
        // `t = getWidenedLiteralType(getTypeOfSymbol(symbol))`.
        let symbol = get_symbol_of_declaration(program, node)?;
        let ty = get_type_of_symbol(checker, program, symbol, None);
        let widened = checker.get_widened_literal_type(ty);
        Some(
            type_to_type_node(checker, program, widened)
                .unwrap_or(SynthesizedTypeNode::Keyword(Kind::AnyKeyword)),
        )
    }

    /// Synthesizes the *return type node* declaration emit annotates a
    /// function-like declaration with when it has no return annotation (Go's
    /// `CreateReturnTypeOfSignatureDeclaration`).
    ///
    /// Mirrors Go's `CreateReturnTypeOfSignatureDeclaration` ->
    /// `SerializeReturnTypeForSignature` -> `getReturnTypeOfSignature`: the
    /// reachable subset infers the return type from the body
    /// (`getReturnTypeFromBody`), so `function f() { return 1; }` yields a
    /// `number` keyword node. A bodyless declaration degrades to the `any`
    /// keyword (its return type is `any`).
    ///
    /// DEFER(phase-5-emit-D-F3): the annotated-signature reuse, the
    /// `enclosingSymbolTypes` reuse, the pseudo-type annotation reuse, the
    /// inferred type-predicate path, `SuppressAnyReturnType`, and the
    /// `SymbolTracker`. blocked-by: signature-scope node building + pseudochecker
    /// reuse + the symbol tracker (D-F3).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver, SynthesizedTypeNode};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> Option<SynthesizedTypeNode> {
    /// r.create_return_type_of_signature_declaration(c, p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may check the function body and cache its return type.
    // Go: internal/checker/emitresolver.go:EmitResolver.CreateReturnTypeOfSignatureDeclaration
    pub fn create_return_type_of_signature_declaration(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Option<SynthesizedTypeNode> {
        // Go: getSignatureFromDeclaration(node) -> getReturnTypeOfSignature.
        // The reachable subset has no body-based inference in
        // `getSignatureFromDeclaration` (it yields `any`), so the return type is
        // inferred from the body here (Go's `getReturnTypeFromBody`, which
        // `getReturnTypeOfSignature` calls for an un-annotated function).
        let return_type = checker.get_return_type_from_body(program, node);
        Some(
            type_to_type_node(checker, program, return_type)
                .unwrap_or(SynthesizedTypeNode::Keyword(Kind::AnyKeyword)),
        )
    }

    /// Reports whether `node` is a *literal const* declaration whose initializer
    /// declaration emit keeps verbatim instead of a synthesized type (so
    /// `const x = 1` emits `declare const x = 1;`, Go's
    /// `IsLiteralConstDeclaration`).
    ///
    /// 4 (D-F2) ports the reachable `var const` arm: a `const` variable
    /// declaration whose symbol type is a *fresh* literal (`1` / `"a"` /
    /// `true`). The `isDeclarationReadonly` arm (a `readonly` property /
    /// parameter property) is deferred.
    ///
    /// DEFER(phase-5-emit-later): `isDeclarationReadonly` (readonly fields). An
    /// array/object/enum const is not a literal const (its type is not a fresh
    /// primitive literal), so it correctly takes a synthesized type instead.
    /// blocked-by: readonly-modifier resolution on properties/parameters.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_literal_const_declaration(c, p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may resolve and cache the declaration's type.
    // Go: internal/checker/emitresolver.go:EmitResolver.IsLiteralConstDeclaration
    pub fn is_literal_const_declaration(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        // Go: isDeclarationReadonly(node) || (IsVariableDeclaration(node) &&
        // IsVarConst(node)). DEFER: the readonly arm.
        let is_var_const = program.arena().kind(node) == Kind::VariableDeclaration
            && combined_node_flags(program, node).intersects(NodeFlags::CONSTANT);
        if !is_var_const {
            return false;
        }
        // Go: isFreshLiteralType(getTypeOfSymbol(getSymbolOfDeclaration(node))).
        let Some(symbol) = get_symbol_of_declaration(program, node) else {
            return false;
        };
        let ty = get_type_of_symbol(checker, program, symbol, None);
        checker.is_fresh_literal_type(ty)
    }

    /// Returns the constant *value expression* declaration emit keeps for a
    /// literal `const` declaration (Go's `CreateLiteralConstValue`).
    ///
    /// 4 (D-F2) ports the reachable primitive-literal subset: the declaration's
    /// symbol type's literal value is returned as a [`LiteralConstValue`]
    /// (number/string/boolean), which the declaration transformer rebuilds into
    /// the kept initializer. A non-literal type yields `None`.
    ///
    /// DEFER(phase-5-emit-later): the enum-like (`SymbolToExpression`) and
    /// bigint / `Infinity` / `NaN` arms, and the `regularTrue`/`regularFalse`
    /// special-casing. blocked-by: enum symbol-to-expression + bigint literal
    /// values.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver, LiteralConstValue};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> Option<LiteralConstValue> {
    /// r.create_literal_const_value(c, p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may resolve and cache the declaration's type.
    // Go: internal/checker/emitresolver.go:EmitResolver.CreateLiteralConstValue
    pub fn create_literal_const_value(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Option<LiteralConstValue> {
        let symbol = get_symbol_of_declaration(program, node)?;
        let ty = get_type_of_symbol(checker, program, symbol, None);
        // Go: enum-like -> SymbolToExpression (DEFER); trueType/falseType ->
        // keyword; then `if t.flags & Literal == 0 { return nil }` and a switch
        // on the literal value. The reachable subset is the primitive-literal
        // switch (boolean handled via its literal value).
        if !checker.get_type(ty).flags().intersects(TypeFlags::LITERAL) {
            return None;
        }
        match checker.get_type(ty).literal_value()? {
            LiteralValue::String(s) => Some(LiteralConstValue::String(s.clone())),
            LiteralValue::Boolean(b) => Some(LiteralConstValue::Boolean(*b)),
            LiteralValue::Number(n) => {
                let value = f64::from(*n);
                let negative = value < 0.0;
                let text = if negative {
                    (-value).to_string()
                } else {
                    value.to_string()
                };
                Some(LiteralConstValue::Number { text, negative })
            }
            LiteralValue::BigInt(_) => None,
        }
    }

    /// Resolves an identifier *use* (`node`, in value position) to the
    /// declaration symbol it references, walking the scope chain from `node`
    /// outward through enclosing block/function/module scopes to the program's
    /// globals (Go's `resolveName` for an expression identifier, via
    /// `getResolvedSymbol`/`checkIdentifier`).
    ///
    /// The innermost matching declaration wins (shadowing): a use inside a
    /// function resolves to that function's local before an outer/global of the
    /// same name. Resolution keeps any symbol whose flags intersect
    /// `Value | Alias`, so an imported binding (an `Alias` symbol) is found in
    /// value position — exactly what [`is_referenced`](Self::is_referenced)
    /// needs.
    ///
    /// DEFER(phase-4-checker-post): the alias's value-ness is approximated by
    /// matching the `Alias` flag directly rather than following the alias to its
    /// target and testing the target's flags (Go's `getSymbolFlags` /
    /// `getSymbol`); full meaning-by-target plus the resolver's special-name and
    /// type-only rules need cross-file alias resolution.
    /// blocked-by: module import/export resolution (`exports.go`) + `MarkLinked`
    /// references (type-vs-value meaning).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> Option<tsgo_ast::SymbolId> {
    /// r.resolve_reference(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/checker.go:Checker.resolveName / getResolvedSymbol
    pub fn resolve_reference(&self, program: &dyn BoundProgram, node: NodeId) -> Option<SymbolId> {
        if program.arena().kind(node) != Kind::Identifier {
            return None;
        }
        let name = program.arena().text(node);
        resolve_name(
            program,
            node,
            name,
            SymbolFlags::VALUE | SymbolFlags::ALIAS,
            false,
            program.globals(),
        )
    }

    /// Returns the *export container* a value-position identifier `node`
    /// resolves to, i.e. the node a use of a top-level exported binding must be
    /// qualified against during emit (Go's `GetReferencedExportContainer`). For
    /// a use of a top-level export of the current module this is the
    /// `SourceFile` node, which the CommonJS transform rewrites into an
    /// `exports.<name>` access.
    ///
    /// 4as ports the reachable single-file subset: resolve the use (with the
    /// `ExportValue | Value | Alias` meaning Go's `getReferencedValueSymbol`
    /// uses), and if it resolves to a top-level *exported* binding of the
    /// current module, return that module's `SourceFile` node. A use of a
    /// non-exported local, or of an inner binding that shadows an export,
    /// resolves to a non-exported symbol and yields `None`. Per Go, an exported
    /// binding that owns a *local* declaration of a non-variable kind
    /// (`ExportHasLocal`: function/class/enum/namespace) is **not** prefixed
    /// (returns `None`) unless `prefix_locals` is set — only exported variables
    /// become `exports.x` use-sites.
    ///
    /// DEFER(phase-4-checker-post): the namespace/enum export containers (Go's
    /// `FindAncestor` over the matching `ModuleDeclaration`/`EnumDeclaration`),
    /// the cross-module UMD-export case (`symbolFile != referenceFile` returns
    /// `None`), the `startInDeclarationContainer` start-point shift for a
    /// module/enum declaration *name*, and merged-symbol canonicalization.
    /// blocked-by: namespace/enum container resolution + cross-module
    /// (`compiler.Program`, P6) + `getMergedSymbol`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> Option<tsgo_ast::NodeId> {
    /// r.get_referenced_export_container(p, n, false)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/binder/referenceresolver.go:referenceResolver.GetReferencedExportContainer
    pub fn get_referenced_export_container(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
        prefix_locals: bool,
    ) -> Option<NodeId> {
        let arena = program.arena();
        // Go: GetReferencedExportContainer takes an `IdentifierNode`; a
        // non-identifier never resolves to a value reference.
        if arena.kind(node) != Kind::Identifier {
            return None;
        }
        // Go: getReferencedValueSymbol resolves with meaning
        // `ExportValue | Value | Alias`. ExportValue is required because a
        // top-level *exported* binding leaves only an `ExportValue` "phantom"
        // local in the module's `locals` (the real symbol lives in the module
        // `exports`, reached via `export_symbol`).
        let name = arena.text(node);
        let mut symbol = resolve_name(
            program,
            node,
            name,
            SymbolFlags::EXPORT_VALUE | SymbolFlags::VALUE | SymbolFlags::ALIAS,
            false,
            program.globals(),
        )?;
        // Go: if symbol.Flags & ExportValue != 0 { symbol = getMergedSymbol(symbol.ExportSymbol) }
        // (getMergedSymbol is identity in the single-file subset).
        if program
            .symbol(symbol)
            .flags
            .contains(SymbolFlags::EXPORT_VALUE)
        {
            let export_symbol = program.symbol(symbol).export_symbol?;
            // Go: if !prefixLocals && exportSymbol.Flags&ExportHasLocal != 0 &&
            // exportSymbol.Flags&Variable == 0 { return nil }. `ExportHasLocal`
            // (function/class/enum/namespace) names a runtime local binding that
            // is referenced unqualified; only exported *variables* are rewritten
            // into `exports.x` use-sites.
            let export_flags = program.symbol(export_symbol).flags;
            if !prefix_locals
                && export_flags.intersects(SymbolFlags::EXPORT_HAS_LOCAL)
                && !export_flags.intersects(SymbolFlags::VARIABLE)
            {
                return None;
            }
            symbol = export_symbol;
        }
        // Go: parentSymbol := getParentOfSymbol(symbol). When the parent is the
        // module (a `ValueModule` whose `ValueDeclaration` is the `SourceFile`),
        // the export container is that source file.
        let parent_symbol = program.symbol(symbol).parent?;
        let parent = program.symbol(parent_symbol);
        if parent.flags.contains(SymbolFlags::VALUE_MODULE) {
            if let Some(value_declaration) = parent.value_declaration {
                if arena.kind(value_declaration) == Kind::SourceFile {
                    return Some(value_declaration);
                }
            }
        }
        None
    }

    /// Reports whether the declaration `node` (e.g. an import specifier / import
    /// clause) is *referenced* anywhere in the file by a value-position
    /// identifier use that resolves to it (Go's `isReferenced` primitive, the
    /// query importElision asks to elide unused imports).
    ///
    /// Scans every identifier in the file, skipping the declaration's own name
    /// node(s), and reports `true` as soon as one resolves (via
    /// [`resolve_reference`](Self::resolve_reference), scope-correct) to the
    /// declaration's symbol. This is the scope-aware replacement for a textual
    /// name-match stand-in: a use shadowed by an inner binding of the same name
    /// is correctly *not* counted as a reference to the outer declaration.
    ///
    /// DEFER(phase-4-checker-post): Go records reference kinds eagerly during
    /// checking (`markLinkedReferences` -> `symbolReferenceLinks.referenceKinds`)
    /// and `isReferenced` is then an O(1) flag read; the type-only-ness split
    /// (a *type*-only use does not keep a value import alive) needs the
    /// type-vs-value meaning of each use site.
    /// blocked-by: `markLinkedReferencesRecursively` + full type-vs-value
    /// meaning resolution.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, decl: tsgo_ast::NodeId) -> bool {
    /// r.is_referenced(p, decl)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/checker.go:Checker.isReferenced(7041)
    pub fn is_referenced(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        let Some(symbol) = get_symbol_of_declaration(program, node) else {
            return false;
        };
        // The declaration's own name node(s) resolve to `symbol` too; exclude
        // them so a binding is not counted as a reference to itself.
        let name_nodes: Vec<NodeId> = program
            .symbol(symbol)
            .declarations
            .iter()
            .filter_map(|&decl| declaration_name(arena, decl))
            .collect();
        for raw in 0..arena.node_count() as u32 {
            let id = NodeId(raw);
            if arena.kind(id) != Kind::Identifier || name_nodes.contains(&id) {
                continue;
            }
            if self.resolve_reference(program, id) == Some(symbol) {
                return true;
            }
        }
        false
    }

    /// Reports whether the alias declaration `node` (e.g. an `export { x }`
    /// specifier) aliases something that is, transitively, a *value* — the
    /// query declaration emit asks to keep value re-exports while eliding
    /// type-only ones (Go's `IsValueAliasDeclaration` /
    /// `isValueAliasDeclarationWorker` + `isAliasResolvedToValue`).
    ///
    /// 4ao ports the reachable single-file subset: for an export specifier the
    /// (property) name is resolved in the local module scope with *value*
    /// meaning, and the specifier is a value alias iff that resolves to a value
    /// symbol. So `function f() {}; export { f }` is a value alias, while
    /// `interface I {}; export { I }` is not. 4ap adds the `export =`
    /// (`ExportAssignment`) arm: an `export = <identifier>` is a value alias iff
    /// the identifier resolves to a value (so `function f() {}; export = f` is,
    /// `interface I {}; export = I` is not), and any non-identifier expression
    /// (e.g. `export = {}`) is kept (Go returns `true`).
    ///
    /// DEFER(phase-4-checker-post): the other alias forms (`import =`/import
    /// clause/namespace import/`export *`) and, crucially, transitive
    /// *cross-module* target value-ness (an export specifier or `export =` that
    /// re-exports an imported binding / entity-name target, `import { x } from
    /// "m"; export { x }`), plus const-enum/`preserveConstEnums` and
    /// type-only-ness.
    /// blocked-by: module import/export resolution (`exports.go`,
    /// `resolveExternalModuleSymbol`, `getExportSymbolOfValueSymbolIfExported`)
    /// + type-only meaning (`markLinkedReferences`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_value_alias_declaration(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/emitresolver.go:EmitResolver.isValueAliasDeclarationWorker(718)
    pub fn is_value_alias_declaration(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        match arena.data(node) {
            // Go: isValueAliasDeclarationWorker -> ExportSpecifier branch ->
            // isAliasResolvedToValue. Reachable subset: the alias resolves to a
            // value iff the (property) name resolves to a value symbol in the
            // local module scope.
            NodeData::ImportSpecifier(d) | NodeData::ExportSpecifier(d) => {
                let name = d.property_name.unwrap_or(d.name);
                resolve_name(
                    program,
                    name,
                    arena.text(name),
                    SymbolFlags::VALUE,
                    false,
                    program.globals(),
                )
                .is_some()
            }
            // Go: isValueAliasDeclarationWorker -> ExportAssignment branch. When
            // the exported expression is an identifier, the assignment is a
            // value alias iff that identifier resolves to a value; any other
            // expression (e.g. `export = {}`) is kept (returns true). Reachable
            // subset: resolve the expression name with *value* meaning in the
            // local module scope (same single-file stand-in as the specifier
            // arm above).
            NodeData::ExportAssignment(d) => {
                if arena.kind(d.expression) == Kind::Identifier {
                    resolve_name(
                        program,
                        d.expression,
                        arena.text(d.expression),
                        SymbolFlags::VALUE,
                        false,
                        program.globals(),
                    )
                    .is_some()
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    /// Reports whether the alias declaration `node` (e.g. an import specifier,
    /// `import =` / `export =`) is *referenced* and so must be kept by emit
    /// (Go's `IsReferencedAliasDeclaration`).
    ///
    /// 4ao ports the reachable referenced-check: `node` must be an alias symbol
    /// declaration, and it is referenced iff some value-position use resolves to
    /// it (reusing 4an's [`is_referenced`](Self::is_referenced), the scope-aware
    /// stand-in for Go's eagerly-recorded `aliasSymbolLinks.referenced`).
    ///
    /// DEFER(phase-4-checker-post): Go's "exported alias whose target is a
    /// value" branch (`target != nil && export modifier && getSymbolFlags(target)
    /// & Value`) keeps an unreferenced *exported* alias alive; plus the eager
    /// `referenced` flag and the const-enum/`preserveConstEnums` carve-out.
    /// blocked-by: alias target resolution (`resolveAlias` cross-module) +
    /// `getSymbolFlags` + `markLinkedReferences`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_referenced_alias_declaration(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsReferencedAliasDeclaration(680)
    pub fn is_referenced_alias_declaration(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        // Go: IsReferencedAliasDeclaration only considers alias symbol
        // declarations; anything else is not.
        if !is_alias_symbol_declaration(program.arena(), node) {
            return false;
        }
        self.is_referenced(program, node)
    }

    /// Reports whether `node` is the implementation of a set of overloads
    /// (Go's `IsImplementationOfOverload`): a body-bearing function whose symbol
    /// has more than one declaration.
    ///
    /// DEFER(phase-4-checker-post): methods/constructors and the single-signature
    /// case where the lone signature comes from a different declaration; get/set
    /// accessors (never overload implementations).
    /// blocked-by: `getSignaturesOfSymbol` over all declarations.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_implementation_of_overload(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsImplementationOfOverload(458)
    pub fn is_implementation_of_overload(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let has_body = matches!(
            program.arena().data(node),
            NodeData::FunctionDeclaration(d) if d.body.is_some()
        );
        if !has_body {
            return false;
        }
        match get_symbol_of_declaration(program, node) {
            Some(symbol) => program.symbol(symbol).declarations.len() > 1,
            None => false,
        }
    }

    /// Returns the evaluated constant value of an enum member `node` (Go's
    /// `GetEnumMemberValue` → `getEnumMemberValue`): the auto-incremented or
    /// constant-folded value the parent enum assigns this member.
    ///
    /// A node that is not an enum member (or whose value is not foldable in the
    /// reachable subset) yields [`EvalValue::None`] (Go's nil `Result.Value`).
    ///
    /// DEFER(phase-4-checker-post): Go's `GetEnumMemberValue` returns the full
    /// `evaluator.Result` (value plus the `isSyntacticallyString`/
    /// `resolvedOtherFiles` bookkeeping the `isolatedModules` checks read); the
    /// reachable port returns only the value, and the per-member diagnostics and
    /// cross-file resolution are deferred with [`EvalValue`]'s producer.
    /// blocked-by: eager `enumMemberLinks` caching + enum diagnostics + cross-file.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver, EvalValue};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> EvalValue {
    /// r.get_enum_member_value(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (computes the parent enum's member values on demand).
    // Go: internal/checker/emitresolver.go:EmitResolver.GetEnumMemberValue(89)
    pub fn get_enum_member_value(&self, program: &dyn BoundProgram, node: NodeId) -> EvalValue {
        declared_enum_member_value(program, node)
    }

    /// Returns the constant value a node folds to for emit (Go's
    /// `GetConstantValue`), or [`EvalValue::None`] when the node is not a
    /// compile-time constant.
    ///
    /// An enum member node yields its value directly (Go's first
    /// `node.Kind == KindEnumMember` branch). A reference whose target is a
    /// **const** enum member — a property access `E.A` (or a bare identifier) —
    /// is inlined to that member's value, which is exactly what lets the emitter
    /// rewrite `E.A` to its literal (`var x = 10`); a reference to a *non-const*
    /// enum member is not inlined (Go returns nil), so the runtime `E.A` access
    /// is preserved.
    ///
    /// DEFER(phase-4-checker-post): Go resolves the reference via
    /// `checkExpressionCached` + `symbolNodeLinks.resolvedSymbol` (falling back
    /// to `resolveEntityName`); the reachable single-file port resolves the
    /// entity name directly and handles the property-access and bare-identifier
    /// forms only — element-access (`E["A"]`) and qualified/cross-module
    /// references are deferred.
    /// blocked-by: `checkExpression` symbol caching + `resolveEntityName`
    /// (element access / qualified name) + cross-module (`compiler.Program`, P6).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver, EvalValue};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> EvalValue {
    /// r.get_constant_value(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (a read-only fold over the bound program).
    // Go: internal/checker/services.go:Checker.GetConstantValue(819)
    pub fn get_constant_value(&self, program: &dyn BoundProgram, node: NodeId) -> EvalValue {
        let arena = program.arena();
        // Go: if node.Kind == KindEnumMember { return getEnumMemberValue(node).Value }.
        if arena.kind(node) == Kind::EnumMember {
            return self.get_enum_member_value(program, node);
        }
        // Go: the resolved symbol is an enum member -> inline only for const
        // enums (`ast.IsEnumConst(member.Parent)`).
        let Some(member) = self.resolve_enum_member_reference(program, node) else {
            return EvalValue::None;
        };
        let Some(value_declaration) = program.symbol(member).value_declaration else {
            return EvalValue::None;
        };
        let Some(enum_decl) = arena.parent(value_declaration) else {
            return EvalValue::None;
        };
        if !is_enum_const(arena, enum_decl) {
            return EvalValue::None;
        }
        self.get_enum_member_value(program, value_declaration)
    }

    /// Resolves a value-position reference (`E.A` / a bare identifier) to the
    /// enum-member symbol it names, the reachable single-file stand-in for the
    /// symbol Go's `GetConstantValue` reads off `symbolNodeLinks.resolvedSymbol`.
    /// Returns `None` for any node that does not resolve to an enum member.
    // Go: internal/checker/services.go:Checker.GetConstantValue (resolved symbol)
    fn resolve_enum_member_reference(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Option<SymbolId> {
        let arena = program.arena();
        match arena.data(node) {
            // `E.A`: resolve the object `E` as a value, then read member `A` off
            // its exports.
            NodeData::PropertyAccessExpression(d) => {
                if arena.kind(d.expression) != Kind::Identifier {
                    return None;
                }
                let enum_symbol = resolve_name(
                    program,
                    d.expression,
                    arena.text(d.expression),
                    SymbolFlags::VALUE,
                    false,
                    program.globals(),
                )?;
                let member_name = arena.text(d.name);
                let &member = program.symbol(enum_symbol).exports.get(member_name)?;
                program
                    .symbol(member)
                    .flags
                    .contains(SymbolFlags::ENUM_MEMBER)
                    .then_some(member)
            }
            // A bare identifier whose value resolution is itself an enum member.
            NodeData::Identifier(_) => {
                let symbol = resolve_name(
                    program,
                    node,
                    arena.text(node),
                    SymbolFlags::VALUE,
                    false,
                    program.globals(),
                )?;
                program
                    .symbol(symbol)
                    .flags
                    .contains(SymbolFlags::ENUM_MEMBER)
                    .then_some(symbol)
            }
            _ => None,
        }
    }

    /// Serializes a *type-annotation node* to the runtime-constructor descriptor
    /// the legacy-decorator transform emits for `__metadata("design:type", ..)`
    /// (Go's `serializeTypeNode`).
    ///
    /// 4at ports the reachable keyword-type subset, faithful to Go's switch:
    /// `number` → [`Number`](SerializedTypeNode::Number), `string` →
    /// [`String`](SerializedTypeNode::String), `boolean` →
    /// [`Boolean`](SerializedTypeNode::Boolean), `bigint` →
    /// [`BigInt`](SerializedTypeNode::BigInt), `symbol` →
    /// [`Symbol`](SerializedTypeNode::Symbol), `void`/`undefined`/`never` and a
    /// `null` literal type → [`VoidZero`](SerializedTypeNode::VoidZero), and
    /// `any`/`unknown`/`object` (plus Go's catch-all) →
    /// [`Object`](SerializedTypeNode::Object). 4au extends the same switch with
    /// the structural arms that reuse the existing variants: Go's leading
    /// `SkipTypeParentheses` (`(T)` unwraps to its inner type), the
    /// `TemplateLiteralType` arm (→ `String`, grouped with `string`), and the
    /// non-`null` literal-type arms (`serializeLiteralOfLiteralTypeNode`):
    /// string literal → `String`, numeric literal → `Number`, bigint literal →
    /// `BigInt`, `true`/`false` → `Boolean`, and a negated numeric/bigint
    /// literal (`-1`) recurses on its operand. 4av adds the structural arms
    /// that need new variants: `ArrayType`/`TupleType` →
    /// [`Array`](SerializedTypeNode::Array) and `FunctionType`/`ConstructorType`
    /// → [`Function`](SerializedTypeNode::Function) (landed in a coordinated
    /// checker + `tsgo_transformers` lane that also extended the transformer's
    /// exhaustive `serialized_type_to_expression` match).
    ///
    /// DEFER(phase-5): the non-keyword arms Go also handles — a `TypeReference`
    /// to a value-having entity (`Date`/a class → that entity's constructor, via
    /// `GetTypeReferenceSerializationKind` + symbol/value resolution),
    /// union/intersection/conditional recursion, and `TypePredicate`. These
    /// currently fall to the conservative [`Object`](SerializedTypeNode::Object)
    /// tail rather than their Go-specific result.
    /// blocked-by: `GetTypeReferenceSerializationKind` (entity value-ness +
    /// `printer.TypeReferenceSerializationKind`) + the `serializeTypeNode`
    /// recursion, which the P5 decorator transform drives.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver, SerializedTypeNode};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, type_node: tsgo_ast::NodeId) -> SerializedTypeNode {
    /// r.serialize_type_node_for_metadata(p, type_node)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode
    pub fn serialize_type_node_for_metadata(
        &self,
        program: &dyn BoundProgram,
        type_node: NodeId,
    ) -> SerializedTypeNode {
        // Go: `node = ast.SkipTypeParentheses(node)` (run before the switch) —
        // `SkipTypeParentheses` loops `for IsParenthesizedTypeNode(node) { node =
        // node.Type() }`, so `(T)` (and `((T))`) dispatches on its inner type.
        let mut type_node = type_node;
        while let NodeData::ParenthesizedType(d) = program.arena().data(type_node) {
            type_node = d.type_node;
        }
        match program.arena().kind(type_node) {
            // Go: case KindVoidKeyword, KindUndefinedKeyword, KindNeverKeyword
            // -> NewVoidZeroExpression.
            Kind::VoidKeyword | Kind::UndefinedKeyword | Kind::NeverKeyword => {
                SerializedTypeNode::VoidZero
            }
            // Go: case KindNumberKeyword -> NewIdentifier("Number").
            Kind::NumberKeyword => SerializedTypeNode::Number,
            // Go: case KindBigIntKeyword -> NewIdentifier("BigInt").
            Kind::BigIntKeyword => SerializedTypeNode::BigInt,
            // Go: case KindSymbolKeyword -> NewIdentifier("Symbol").
            Kind::SymbolKeyword => SerializedTypeNode::Symbol,
            // Go: case KindTemplateLiteralType, KindStringKeyword -> NewIdentifier("String").
            Kind::TemplateLiteralType | Kind::StringKeyword => SerializedTypeNode::String,
            // Go: case KindBooleanKeyword -> NewIdentifier("Boolean").
            Kind::BooleanKeyword => SerializedTypeNode::Boolean,
            // Go: case KindLiteralType ->
            // serializeLiteralOfLiteralTypeNode(node.AsLiteralTypeNode().Literal).
            Kind::LiteralType => match program.arena().data(type_node) {
                NodeData::LiteralType(d) => {
                    serialize_literal_of_literal_type_node(program, d.literal)
                }
                _ => SerializedTypeNode::Object,
            },
            // Go: case KindArrayType, KindTupleType -> NewIdentifier("Array").
            Kind::ArrayType | Kind::TupleType => SerializedTypeNode::Array,
            // Go: case KindFunctionType, KindConstructorType ->
            // NewIdentifier("Function").
            Kind::FunctionType | Kind::ConstructorType => SerializedTypeNode::Function,
            // Go: the `serializeTypeNode` switch tail
            // (`return s.f.NewIdentifier("Object")`) — the catch-all for
            // everything not matched by a specific arm.
            _ => SerializedTypeNode::Object,
        }
    }

    /// Classifies a `TypeReference` type node for legacy-decorator `design:type`
    /// emit (Go's `GetTypeReferenceSerializationKind`), reporting whether the
    /// referenced entity is reachable at runtime as a value.
    ///
    /// The legacy-decorator transform (Phase 5) emits the referenced entity's
    /// constructor (`: SomeClass` → the `SomeClass` identifier) when this
    /// returns [`TypeWithConstructSignatureAndValue`](TypeReferenceSerializationKind::TypeWithConstructSignatureAndValue),
    /// and a safe fallback otherwise.
    ///
    /// 4aw ports the reachable single-file subset, faithful to Go's structure:
    /// the reference's entity name is resolved both as a *value* and as a *type*
    /// (Go's two `resolveEntityName` calls); when both resolve to the same
    /// class symbol — a runtime constructor reachable without lib globals — the
    /// result is `TypeWithConstructSignatureAndValue`. Otherwise, if the name
    /// resolves as a type to a non-error declared type (an interface or
    /// type-alias), the result is [`ObjectType`](TypeReferenceSerializationKind::ObjectType);
    /// an unresolved name (no value and no type symbol, or an error declared
    /// type) is [`Unknown`](TypeReferenceSerializationKind::Unknown).
    ///
    /// DEFER(phase-6): the lib-globals-dependent kinds — the global `Promise`
    /// constructor arm (`getGlobalPromiseConstructorSymbol`), and the
    /// `isTypeAssignableToKind` chain that maps a resolved type to
    /// `VoidNullableOrNeverType`/`NumberLikeType`/`BigIntLikeType`/
    /// `StringLikeType`/`BooleanType`/`ESSymbolType`, plus the
    /// tuple/function/array (`isTupleType`/`isFunctionType`/`isArrayType`,
    /// `TypeWithCallSignature`/`ArrayLikeType`) arms — all need resolved global
    /// lib types and full construct/call-signature collection. The general
    /// `isConstructorType(getTypeOfSymbol(valueSymbol))` is approximated here by
    /// the resolved value symbol being a `class` (the only single-file source of
    /// a runtime construct signature); Rust's `get_type_of_symbol` gives a
    /// class's *instance* type, and construct signatures are not yet collected.
    /// The cross-module alias resolution (`resolveAlias`), the
    /// type-only-import split (`isTypeOnly` → `TypeWithCallSignature`/
    /// `ObjectType`), the separate `serialScope` resolution location, and
    /// qualified-name (`A.B`) entities are also deferred.
    /// blocked-by: lib global types plus class construct-signature collection
    /// (P6), cross-module alias / type-only meaning (`exports.go`,
    /// `markLinkedReferences`), and qualified-name `resolveEntityName`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver, TypeReferenceSerializationKind};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, type_node: tsgo_ast::NodeId) -> TypeReferenceSerializationKind {
    /// r.get_type_reference_serialization_kind(c, p, type_node)
    /// # }
    /// ```
    ///
    /// Side effects: may build and cache declared types for the referenced
    /// symbol.
    // Go: internal/checker/emitresolver.go:EmitResolver.GetTypeReferenceSerializationKind(1139)
    pub fn get_type_reference_serialization_kind(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        type_node: NodeId,
    ) -> TypeReferenceSerializationKind {
        // Go: `serializeTypeReferenceNode` hands `node.TypeName` (the entity
        // name) to `GetTypeReferenceSerializationKind`; recover it from the
        // `TypeReference` node. A node that is not a type reference has no
        // entity name (Go's `typeName == nil` → `Unknown`).
        let type_name = match program.arena().data(type_node) {
            NodeData::TypeReference(d) => d.type_name,
            _ => return TypeReferenceSerializationKind::Unknown,
        };
        // DEFER(phase-6): a qualified-name entity (`A.B`) needs
        // `resolveEntityName`'s namespace-member walk + `GetFirstIdentifier`
        // root value-symbol type-only check; the reachable subset is a bare
        // identifier reference.
        // blocked-by: qualified-name `resolveEntityName` + namespace resolution.
        if program.arena().kind(type_name) != Kind::Identifier {
            return TypeReferenceSerializationKind::Unknown;
        }
        let name = program.arena().text(type_name).to_string();
        // Go: valueSymbol = resolveEntityName(typeName, Value, ...) — resolve as
        // a value so the type can be reached at runtime during emit.
        let value_symbol = resolve_name(
            program,
            type_name,
            &name,
            SymbolFlags::VALUE,
            false,
            program.globals(),
        );
        // Go: typeSymbol = resolveEntityName(typeName, Type, ...) — resolve as a
        // type for a more useful serializer hint.
        let type_symbol = resolve_name(
            program,
            type_name,
            &name,
            SymbolFlags::TYPE,
            false,
            program.globals(),
        );
        // Go: if resolvedValueSymbol != nil && resolvedValueSymbol ==
        // resolvedTypeSymbol { ... isConstructorType(getTypeOfSymbol(value)) ...
        // return TypeWithConstructSignatureAndValue }. Reachable stand-in for
        // `isConstructorType`: a `class` value symbol is the only single-file
        // source of a runtime construct signature (see the method's DEFER note).
        if let (Some(value), Some(ty)) = (value_symbol, type_symbol) {
            if value == ty && program.symbol(value).flags.contains(SymbolFlags::CLASS) {
                return TypeReferenceSerializationKind::TypeWithConstructSignatureAndValue;
            }
        }
        // Go: if resolvedTypeSymbol == nil { ... return Unknown }. With no type
        // symbol the reference is unresolved (DEFER: the `isTypeOnly` carve-out
        // that returns `ObjectType` here).
        let Some(type_symbol) = type_symbol else {
            return TypeReferenceSerializationKind::Unknown;
        };
        // Go: type_ = getDeclaredTypeOfSymbol(resolvedTypeSymbol); if
        // isErrorType(type_) { ... return Unknown }.
        let declared = get_declared_type_of_symbol(checker, program, type_symbol, None);
        if declared == checker.error_type() {
            return TypeReferenceSerializationKind::Unknown;
        }
        // Go: the `isTypeAssignableToKind` kind chain
        // (Void/Nullable/Never → Boolean → Number → BigInt → String, then
        // `isTupleType` → ESSymbol → `isFunctionType` → `isArrayType`) all need
        // resolved global lib types and full call/construct-signature
        // collection (DEFER, P6); a resolved interface/type-alias type falls to
        // Go's final `else { return ObjectType }`.
        TypeReferenceSerializationKind::ObjectType
    }
}

// Returns the "name" identifier node of declaration `node`, for the declaration
// kinds whose name must be excluded when scanning for references to it.
// Go: internal/ast/utilities.go:getNameOfDeclaration (subset)
fn declaration_name(arena: &tsgo_ast::NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::ImportSpecifier(d) | NodeData::ExportSpecifier(d) => Some(d.name),
        NodeData::NamespaceImport(d) => Some(d.name),
        NodeData::ImportClause(d) => d.name,
        // Go: an `import x = require("m")` / `import x = ns` binds the identifier
        // `x`; its own name must be excluded so the import-equals is not counted
        // as a reference to itself (`node.Name()` for an import-equals).
        NodeData::ImportEqualsDeclaration(d) => Some(d.name),
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::FunctionDeclaration(d) => d.name,
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.name,
        _ => None,
    }
}

// Reports whether `node` is a declaration that introduces an alias symbol
// (import/export specifier, namespace import/export, `import =`, an import
// clause with a default-binding name, etc.).
//
// 4ao ports the reachable structural kinds; the JS-only require-initialized
// variable/binding-element and CommonJS `module.exports =` binary-expression
// forms, plus the `export =` `ExpressionIsAlias` discrimination, are deferred.
// Go: internal/ast/utilities.go:IsAliasSymbolDeclaration
fn is_alias_symbol_declaration(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ImportEqualsDeclaration(_)
        | NodeData::NamespaceExportDeclaration(_)
        | NodeData::NamespaceImport(_)
        | NodeData::NamespaceExport(_)
        | NodeData::ImportSpecifier(_)
        | NodeData::ExportSpecifier(_) => true,
        NodeData::ImportClause(d) => d.name.is_some(),
        _ => false,
    }
}

// Reachable subset of Go's `EmitResolver.determineIfDeclarationIsVisible`: a
// declaration is visible iff it is exported (by combined modifier flags) and its
// container is visible. See [`EmitResolver::is_declaration_visible`] for the
// DEFER list.
// Go: internal/checker/emitresolver.go:EmitResolver.determineIfDeclarationIsVisible(131)
fn determine_if_declaration_is_visible(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        // Go: the variable/module/class/interface/type-alias/function/enum/
        // import-equals group. DEFER: the empty-binding-pattern variable
        // carve-out and the ambient-module-augmentation/JS-typedef exceptions.
        Kind::VariableDeclaration
        | Kind::ModuleDeclaration
        | Kind::ClassDeclaration
        | Kind::InterfaceDeclaration
        | Kind::TypeAliasDeclaration
        | Kind::FunctionDeclaration
        | Kind::EnumDeclaration
        | Kind::ImportEqualsDeclaration => {
            // Go: if combinedModifierFlags & Export == 0 && !(ambient module
            // element exception) { return IsGlobalSourceFile(parent) }. The
            // single-file reachable subset has no ambient modules, so a
            // non-exported declaration falls to `IsGlobalSourceFile(parent)`:
            // visible in a global script (no external-module indicator),
            // not visible in a module.
            if !combined_modifier_flags(arena, node).contains(ModifierFlags::EXPORT) {
                return is_global_source_file(arena, get_declaration_container(arena, node));
            }
            // Go: return isDeclarationVisible(GetDeclarationContainer(node)).
            let container = get_declaration_container(arena, node);
            determine_if_declaration_is_visible(arena, container)
        }
        // Go: source file + namespace-export declaration are always visible.
        Kind::SourceFile | Kind::NamespaceExportDeclaration => true,
        // Go: type parameters are always visible.
        Kind::TypeParameter => true,
        // Go: default-binding/import-specifier/namespace-import are visible only
        // on demand (false by default); export assignment creates no outside
        // binding.
        Kind::ImportClause
        | Kind::NamespaceImport
        | Kind::ImportSpecifier
        | Kind::ExportAssignment => false,
        // Go's `default: return false` (plus the DEFERred property/method/
        // accessor and structural-type arms).
        _ => false,
    }
}

// Reports whether `node` is a *global* source file — a `SourceFile` that is not
// an external (or CommonJS) module, i.e. a script whose top-level declarations
// are globals. The reachable subset detects module-ness via the parser's
// external-module indicator (CommonJS detection is deferred with the JS file
// support).
// Go: internal/ast/utilities.go:IsGlobalSourceFile(2443) / IsExternalOrCommonJSModule(1630)
fn is_global_source_file(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::SourceFile(d) => d.external_module_indicator.is_none(),
        _ => false,
    }
}

// Returns the root declaration of `node`, walking up through binding elements
// (Go's `GetRootDeclaration`: `for node.Kind == KindBindingElement { node =
// node.Parent.Parent }`).
// Go: internal/ast/utilities.go:GetRootDeclaration(1139)
fn get_root_declaration(arena: &tsgo_ast::NodeArena, node: NodeId) -> NodeId {
    let mut current = node;
    while arena.kind(current) == Kind::BindingElement {
        // `node.Parent.Parent`: a binding element's parent is the pattern, whose
        // parent is the variable declaration / parameter.
        let Some(pattern) = arena.parent(current) else {
            break;
        };
        let Some(decl) = arena.parent(pattern) else {
            break;
        };
        current = decl;
    }
    current
}

// Returns the declaration container of `node` (Go's `GetDeclarationContainer`):
// the parent of the nearest ancestor that is not part of a variable-declaration
// list / named-import grouping.
// Go: internal/ast/utilities.go:GetDeclarationContainer(2556)
fn get_declaration_container(arena: &tsgo_ast::NodeArena, node: NodeId) -> NodeId {
    let mut current = get_root_declaration(arena, node);
    loop {
        match arena.kind(current) {
            // Go's predicate returns `false` for these (keep walking up).
            Kind::VariableDeclaration
            | Kind::VariableDeclarationList
            | Kind::ImportSpecifier
            | Kind::NamedImports
            | Kind::NamespaceImport
            | Kind::ImportClause => match arena.parent(current) {
                Some(parent) => current = parent,
                None => return current,
            },
            // Go's predicate returns `true`: the container is this node's parent.
            _ => return arena.parent(current).unwrap_or(current),
        }
    }
}

// Returns the combined modifier flags of `node` (Go's
// `GetCombinedModifierFlags`): the node's own flags unioned with its wrapping
// `VariableDeclarationList`/`VariableStatement` flags, so a variable
// declaration inherits the `export` from its statement.
// Go: internal/ast/utilities.go:GetCombinedModifierFlags(1162) / getCombinedFlags(1146)
fn combined_modifier_flags(arena: &tsgo_ast::NodeArena, node: NodeId) -> ModifierFlags {
    let node = get_root_declaration(arena, node);
    let mut flags = modifier_flags(arena, node);
    let mut current = node;
    if arena.kind(current) == Kind::VariableDeclaration {
        if let Some(parent) = arena.parent(current) {
            current = parent;
        }
    }
    if arena.kind(current) == Kind::VariableDeclarationList {
        flags |= modifier_flags(arena, current);
        if let Some(parent) = arena.parent(current) {
            current = parent;
        }
    }
    if arena.kind(current) == Kind::VariableStatement {
        flags |= modifier_flags(arena, current);
    }
    flags
}

// Reports whether `node` is a `const enum` declaration (Go's `IsEnumConst`:
// `GetCombinedModifierFlags(node) & ModifierFlagsConst != 0`). For an enum
// declaration the combined modifier flags equal its own (no variable-list/
// statement wrapping), so the own-modifier read suffices.
// Go: internal/ast/utilities.go:IsEnumConst(1834)
fn is_enum_const(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    arena.kind(node) == Kind::EnumDeclaration
        && modifier_flags(arena, node).contains(ModifierFlags::CONST)
}

// Returns the aggregated modifier flags of `node`, if it bears modifiers.
// Go: internal/ast/ast.go:Node.ModifierFlags
fn modifier_flags(arena: &tsgo_ast::NodeArena, node: NodeId) -> ModifierFlags {
    let modifiers = match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers
        .map(|m| m.modifier_flags)
        .unwrap_or(ModifierFlags::empty())
}

// Serializes the `literal` expression of a `LiteralType` node (the literal `"a"`
// / `1` / `true` / `null` inside `: "a"` / `: 1` / `: true` / `: null`) to the
// runtime-constructor descriptor Go's `serializeLiteralOfLiteralTypeNode`
// emits.
//
// DEFER(phase-5): the `KindNoSubstitutionTemplateLiteral -> String` literal arm
// is not reachable here — the Rust parser's `parseNonArrayType` does not yet
// route a no-substitution template (`` `abc` ``) type to `parseLiteralTypeNode`
// (it falls to a type reference), so this port omits that arm until the parser
// gap is closed. The conservative `Object` tail covers any other literal kind.
// blocked-by: parser `parseNonArrayType` `NoSubstitutionTemplateLiteral` arm.
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeLiteralOfLiteralTypeNode
fn serialize_literal_of_literal_type_node(
    program: &dyn BoundProgram,
    literal: NodeId,
) -> SerializedTypeNode {
    match program.arena().kind(literal) {
        // Go: case KindStringLiteral, KindNoSubstitutionTemplateLiteral ->
        // NewIdentifier("String").
        Kind::StringLiteral => SerializedTypeNode::String,
        // Go: case KindNumericLiteral -> NewIdentifier("Number").
        Kind::NumericLiteral => SerializedTypeNode::Number,
        // Go: case KindBigIntLiteral -> NewIdentifier("BigInt").
        Kind::BigIntLiteral => SerializedTypeNode::BigInt,
        // Go: case KindTrueKeyword, KindFalseKeyword -> NewIdentifier("Boolean").
        Kind::TrueKeyword | Kind::FalseKeyword => SerializedTypeNode::Boolean,
        // Go: case KindPrefixUnaryExpression { operand := node.Operand; switch
        // operand.Kind { case KindNumericLiteral, KindBigIntLiteral ->
        // serializeLiteralOfLiteralTypeNode(operand) } } — a negative
        // numeric/bigint literal type (`-1` / `-1n`) recurses on the operand.
        Kind::PrefixUnaryExpression => match program.arena().data(literal) {
            NodeData::PrefixUnaryExpression(d)
                if matches!(
                    program.arena().kind(d.operand),
                    Kind::NumericLiteral | Kind::BigIntLiteral
                ) =>
            {
                serialize_literal_of_literal_type_node(program, d.operand)
            }
            // Go default: debug.FailBadSyntaxKind(operand) — conservative tail.
            _ => SerializedTypeNode::Object,
        },
        // Go: case KindNullKeyword -> NewVoidZeroExpression.
        Kind::NullKeyword => SerializedTypeNode::VoidZero,
        // Go default: debug.FailBadSyntaxKind — the conservative `Object` tail.
        _ => SerializedTypeNode::Object,
    }
}

#[cfg(test)]
#[path = "emit_resolver_test.rs"]
mod tests;
