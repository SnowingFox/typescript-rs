//! Contextual typing: deriving the type that flows *into* an expression from
//! its surrounding syntax (Go's `getContextualType` family in `checker.go`).
//!
//! Round 4bj lands the foundational reachable subset: the contextual type of an
//! expression that is the initializer of an annotated variable declaration
//! (`const f: T = <expr>`) or the right-hand side of an assignment to an
//! annotated identifier target (`x = <expr>`), and — built on that — the
//! contextual *signature* used to type an arrow/function expression's
//! un-annotated parameters.
//!
//! The call-argument, return-position, yield/await, JSX,
//! object/array-literal-element, and generic-inference contextual paths are
//! deferred (see the per-function `DEFER` notes); they need the call-resolution
//! contextual pass and inference contexts that arrive in later rounds.

use tsgo_ast::{Kind, NodeData, NodeId, SymbolFlags};

use super::declared_types::{
    get_applicable_index_info_for_name, get_property_of_type, get_type_from_type_node,
    get_type_of_symbol,
};
use super::program::BoundProgram;
use super::signatures::SignatureId;
use super::symbols::resolve_name;
use super::types::{TypeFlags, TypeId};
use super::Checker;

bitflags::bitflags! {
    /// Flags controlling how a contextual type is computed (Go's `ContextFlags`).
    ///
    /// 4bj ports the bits the reachable paths thread; the cache-consulting and
    /// constraint-related behavior they gate is deferred. `SIGNATURE` is the
    /// only bit the reachable code branches on (it is passed when resolving a
    /// contextual signature).
    ///
    /// Side effects: none (pure value type).
    // Go: internal/checker/checker.go:ContextFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub(crate) struct ContextFlags: u32 {
        /// No flags.
        const NONE = 0;
        /// Obtaining the contextual type while resolving a signature (used by
        /// `get_contextual_signature`).
        const SIGNATURE = 1 << 0;
        /// Do not constrain instantiable contextual types (deferred).
        const NO_CONSTRAINTS = 1 << 1;
        /// Do not consider binding patterns when computing a contextual type
        /// (deferred).
        const SKIP_BINDING_PATTERNS = 1 << 2;
    }
}

impl Checker {
    /// Returns the contextual type of `node` — the type implied by the syntax
    /// that surrounds it — or `None` when there is none.
    ///
    /// 4bj reaches the two foundational arms: an expression that is the
    /// initializer of an annotated variable/parameter/property declaration, and
    /// the right-hand side of an assignment to an annotated identifier target.
    /// 4bk adds the inverse-direction arms — type flowing *into* a literal:
    /// an object-literal property/shorthand value (`getContextualTypeForObjectLiteralElement`)
    /// and an array-literal element (`getContextualTypeForElementExpression`).
    /// 4bl adds the call/`new` argument arm (`getContextualTypeForArgument`): a
    /// callback argument's parameters get typed from the resolved call
    /// signature's parameter at that position.
    ///
    /// DEFER(phase-4-checker-4bm+): the `NodeFlagsInWithStatement` early-out, the
    /// cached-contextual-node lookup (`findContextualNode`/`contextualInfos`),
    /// and the remaining parent arms — return/arrow-body
    /// (`getContextualTypeForReturnExpression`), yield/await operands, decorators,
    /// `as`/`satisfies`/parenthesized/non-null pass-through, the
    /// `SpreadAssignment` grandparent walk, conditional/template operands, and
    /// JSX. blocked-by: return-position inference + spread typing + JSX.
    ///
    /// Side effects: may allocate types/signatures while resolving the
    /// contextual annotation.
    // Go: internal/checker/checker.go:Checker.getContextualType
    pub(crate) fn get_contextual_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> Option<TypeId> {
        let parent = program.arena().parent(node)?;
        match program.arena().kind(parent) {
            Kind::VariableDeclaration
            | Kind::Parameter
            | Kind::PropertyDeclaration
            | Kind::PropertySignature
            | Kind::BindingElement => {
                self.get_contextual_type_for_initializer_expression(program, node, context_flags)
            }
            Kind::CallExpression | Kind::NewExpression => {
                self.get_contextual_type_for_argument(program, parent, node)
            }
            Kind::BinaryExpression => {
                self.get_contextual_type_for_binary_operand(program, node, context_flags)
            }
            Kind::PropertyAssignment | Kind::ShorthandPropertyAssignment => {
                self.get_contextual_type_for_object_literal_element(program, parent, context_flags)
            }
            Kind::ArrayLiteralExpression => self.get_contextual_type_for_array_literal_element(
                program,
                parent,
                node,
                context_flags,
            ),
            _ => None,
        }
    }

    /// In a variable/parameter/property declaration, the contextual type of an
    /// initializer expression is the declared type (its annotation, or — for a
    /// parameter — the contextual parameter type).
    ///
    /// DEFER(phase-4-checker-4bk+): the binding-pattern-implied type
    /// (`getTypeFromBindingPattern`). blocked-by: binding-pattern typing.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForInitializerExpression
    fn get_contextual_type_for_initializer_expression(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> Option<TypeId> {
        let declaration = program.arena().parent(node)?;
        if declaration_initializer(program, declaration) == Some(node) {
            return self.get_contextual_type_for_variable_like_declaration(
                program,
                declaration,
                context_flags,
            );
        }
        None
    }

    /// The contextual type implied by a variable-like declaration: its type
    /// annotation if present, else (for a parameter) the contextual parameter
    /// type.
    ///
    /// DEFER(phase-4-checker-4bk+): the binding-element
    /// (`getContextualTypeForBindingElement`) and static-property
    /// (`getContextualTypeForStaticPropertyDeclaration`) arms. blocked-by:
    /// binding-element typing + static-class-property contextual typing.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForVariableLikeDeclaration
    fn get_contextual_type_for_variable_like_declaration(
        &mut self,
        program: &dyn BoundProgram,
        declaration: NodeId,
        _context_flags: ContextFlags,
    ) -> Option<TypeId> {
        if let Some(type_node) = declaration_type_node(program, declaration) {
            let globals = program.globals();
            return Some(get_type_from_type_node(self, program, type_node, globals));
        }
        if program.arena().kind(declaration) == Kind::Parameter {
            return self.get_contextually_typed_parameter_type(program, declaration);
        }
        None
    }

    /// Returns the contextual type of a parameter — the type of the
    /// corresponding parameter of the containing arrow/function expression's
    /// contextual signature — or `None` when none is available.
    ///
    /// DEFER(phase-4-checker-4bk+): object-literal methods
    /// (`isContextSensitiveFunctionOrObjectLiteralMethod`), the
    /// immediately-invoked-function-expression argument path
    /// (`getEffectiveCallArguments`/`getSpreadArgumentType`), the `this`-parameter
    /// index offset, and the rest-parameter mapping (`getRestTypeAtPosition`).
    /// blocked-by: object-literal-method contextual typing + IIFE argument typing
    /// + `this`-typing + rest/tuple types.
    // Go: internal/checker/checker.go:Checker.getContextuallyTypedParameterType
    pub(crate) fn get_contextually_typed_parameter_type(
        &mut self,
        program: &dyn BoundProgram,
        parameter: NodeId,
    ) -> Option<TypeId> {
        let fn_node = program.arena().parent(parameter)?;
        if !is_function_expression_or_arrow(program, fn_node) {
            return None;
        }
        let contextual_signature = self.get_contextual_signature(program, fn_node)?;
        let index = parameter_index(program, fn_node, parameter)?;
        self.try_get_type_at_position(program, contextual_signature, index)
    }

    /// The contextual type of a binary expression's operand: for an assignment
    /// (`=`) the right operand is contextually typed by the left operand's type.
    ///
    /// DEFER(phase-4-checker-4bk+): the `satisfies`-annotated binary type, the
    /// compound-assignment operators (`&&=`/`||=`/`??=`), the logical/comma
    /// operators (`||`/`??`/`&&`/`,`), and the `module.exports` assignment
    /// guard. blocked-by: those operator arms + assignment-declaration symbols.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForBinaryOperand
    fn get_contextual_type_for_binary_operand(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        _context_flags: ContextFlags,
    ) -> Option<TypeId> {
        let binary = program.arena().parent(node)?;
        let (operator_token, right) = match program.arena().data(binary) {
            NodeData::BinaryExpression(d) => (d.operator_token, d.right),
            _ => return None,
        };
        if program.arena().kind(operator_token) == Kind::EqualsToken && right == node {
            return self.get_contextual_type_for_assignment_expression(program, binary);
        }
        None
    }

    /// The contextual type derived from the left operand of an assignment.
    /// Reachable subset: an identifier target's (declared) type.
    ///
    /// DEFER(phase-4-checker-4bk+): access-expression targets
    /// (`obj.x = ...`/`obj[k] = ...`/`this.x = ...`), assignment-declaration
    /// (`F.id = ...`) handling, and the `module.exports` exclusion. blocked-by:
    /// property-of-contextual-type lookup + assignment-declaration symbols.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForAssignmentExpression
    fn get_contextual_type_for_assignment_expression(
        &mut self,
        program: &dyn BoundProgram,
        binary: NodeId,
    ) -> Option<TypeId> {
        let left = match program.arena().data(binary) {
            NodeData::BinaryExpression(d) => d.left,
            _ => return None,
        };
        if program.arena().kind(left) != Kind::Identifier {
            return None;
        }
        let name = program.arena().text(left).to_string();
        let globals = program.globals();
        let symbol = resolve_name(program, left, &name, SymbolFlags::VALUE, false, globals)?;
        Some(get_type_of_symbol(self, program, symbol, globals))
    }

    /// The contextual type of `arg`, an argument of the call/`new` expression
    /// `call_target`: the type of the resolved signature's parameter at that
    /// argument's position. This is what types a callback argument's
    /// parameters from the called signature (e.g. `f((x) => ...)` gives `x` the
    /// element type the parameter of `f` expects).
    ///
    /// Returns `None` when `arg` is not one of the call's arguments (e.g. it is
    /// the callee expression, whose parent is also the call node — Go's
    /// `slices.Index(args, arg) == -1`).
    ///
    /// DEFER(phase-4-checker-4bm+): `getEffectiveCallArguments`' synthetic
    /// arguments — spread elements expanded from tuple types, the tagged-template
    /// strings-array + substitution arguments, decorator arguments, and the JSX
    /// attributes argument. blocked-by: spread/tuple typing + tagged-template
    /// typing + decorators + JSX.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForArgument
    fn get_contextual_type_for_argument(
        &mut self,
        program: &dyn BoundProgram,
        call_target: NodeId,
        arg: NodeId,
    ) -> Option<TypeId> {
        let args = call_arguments(program, call_target);
        let arg_index = args.iter().position(|&a| a == arg)?;
        self.get_contextual_type_for_argument_at_index(program, call_target, arg_index)
    }

    /// The contextual type of the argument at `arg_index` of `call_target`: the
    /// type at that position of the signature applicable to the call (Go's
    /// `getTypeAtPosition`, which falls back to `any` past the parameter list).
    ///
    /// Recursion safety: the applicable signature is resolved by typing only the
    /// callee expression (never the arguments) and selecting the single call
    /// signature, so this contextual lookup can never re-enter argument
    /// checking. See [`Checker::get_resolved_signature_for_contextual_argument`].
    ///
    /// DEFER(phase-4-checker-4bm+): the `import(...)` call argument types
    /// (`string`/`ImportCallOptions`/`any`), the JSX first-argument signature,
    /// and the rest-parameter indexed access (`getIndexedAccessTypeEx` over the
    /// rest tuple). blocked-by: import-call typing + JSX signatures + rest/tuple
    /// types.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForArgumentAtIndex
    fn get_contextual_type_for_argument_at_index(
        &mut self,
        program: &dyn BoundProgram,
        call_target: NodeId,
        arg_index: usize,
    ) -> Option<TypeId> {
        let signature =
            self.get_resolved_signature_for_contextual_argument(program, call_target)?;
        // Go's `getTypeAtPosition` returns `anyType` for an out-of-range
        // (non-rest) position; mirror that here so an extra argument still has a
        // contextual type.
        Some(
            self.try_get_type_at_position(program, signature, arg_index)
                .unwrap_or(self.any_type),
        )
    }

    /// Resolves the signature applicable to `call_target` for the purpose of
    /// contextually typing its arguments, *without* re-checking the arguments —
    /// the reachable, recursion-safe subset of Go's `getResolvedSignature`.
    ///
    /// The callee is typed (cheap and idempotent — for an identifier it is just
    /// a symbol-type lookup), then its single call signature is the resolved
    /// one. Because selecting it never consults the arguments, the contextual
    /// pass cannot recurse back into argument checking (Go guards the same
    /// cycle with the `resolvingSignature` sentinel on `signatureLinks`). An
    /// overloaded callee (more than one signature, which Go would disambiguate
    /// from the argument types) and a non-callable one (no signatures) are
    /// deferred and yield `None`.
    ///
    /// Any diagnostics produced while typing the callee here are discarded: the
    /// call-checking pass reports them once on its own, so the contextual lookup
    /// must not duplicate them (Go's `getResolvedSignature` is memoized on
    /// `signatureLinks`, so the second, contextual resolution is a cache hit
    /// that re-emits nothing).
    ///
    /// DEFER(phase-4-checker-4bm+): the overloaded-target case — Go resolves the
    /// chosen overload via `getResolvedSignature` and (during inference) unions
    /// the candidate parameter types; the `import(...)`/JSX/decorator/
    /// tagged-template call targets; and construct signatures for `new`
    /// (`get_signatures_of_type` returns only call signatures). blocked-by:
    /// overload resolution during inference + construct signatures + those call
    /// targets.
    ///
    /// Side effects: may allocate types while resolving the callee; any
    /// diagnostics it would emit are rolled back.
    // Go: internal/checker/checker.go:Checker.getResolvedSignature (reachable subset)
    fn get_resolved_signature_for_contextual_argument(
        &mut self,
        program: &dyn BoundProgram,
        call_target: NodeId,
    ) -> Option<SignatureId> {
        // A generic call whose type arguments were inferred (C-B2) memoized its
        // instantiated signature on the call node; return that so a callback
        // argument is contextually typed by the *instantiated* parameter type
        // (e.g. `map([1,2], x => ...)` types `x` as `number`). Go reaches the
        // same instantiated signature through `getResolvedSignature`'s memo.
        if let Some(&resolved) = self.resolved_signatures.get(&call_target) {
            return Some(resolved);
        }
        let callee = match program.arena().data(call_target) {
            NodeData::CallExpression(d) => d.expression,
            NodeData::NewExpression(d) => d.expression,
            _ => return None,
        };
        // Roll back any callee diagnostics emitted during this contextual-only
        // resolution (see the doc comment): snapshot the current file's
        // diagnostic count, then truncate back to it.
        let handle = program.file_handle();
        let before = self.diagnostics_by_file.get(&handle).map_or(0, Vec::len);
        let func_type = self.check_expression(program, callee);
        if let Some(diagnostics) = self.diagnostics_by_file.get_mut(&handle) {
            diagnostics.truncate(before);
        }
        match self.get_signatures_of_type(func_type).as_slice() {
            [signature] => Some(*signature),
            _ => None,
        }
    }

    /// In an object literal contextually typed by a type `T`, the contextual
    /// type of a property/shorthand value is the type of the matching property
    /// of `T` (or its applicable index signature's value type). This is the
    /// inverse-direction flow: the annotation's property type flows *into* the
    /// member value, so a literal value in a literal-typed property position is
    /// preserved rather than widened.
    ///
    /// DEFER(phase-4-checker-4bl+): the explicit-type-annotation arm (a grammar
    /// error whose Go path returns `getTypeFromTypeNode`), object-literal
    /// methods (`getContextualTypeForObjectLiteralMethod`), and the
    /// computed/dynamic-name arms — a computed name keyed by its expression type
    /// (`getPropertyNameFromType`) and the by-name index-info fallback
    /// (`mapType` over `findApplicableIndexInfo`). blocked-by: property-element
    /// annotation elaboration + object-literal methods + computed-name typing +
    /// union `mapType`.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForObjectLiteralElement
    fn get_contextual_type_for_object_literal_element(
        &mut self,
        program: &dyn BoundProgram,
        element: NodeId,
        context_flags: ContextFlags,
    ) -> Option<TypeId> {
        let object_literal = program.arena().parent(element)?;
        let t =
            self.get_apparent_type_of_contextual_type(program, object_literal, context_flags)?;
        // `hasBindableName` reachable subset: a static (non-computed) property
        // name. A computed/dynamic name has no statically-known property name to
        // look up here (DEFER).
        let name = object_literal_element_static_name(program, element)?;
        self.get_type_of_property_of_contextual_type(program, t, &name)
    }

    /// The contextual type of an array-literal element: derived from the
    /// element's position in the array literal's (apparent) contextual type.
    ///
    /// DEFER(phase-4-checker-4bl+): the spread-index offsets (`getSpreadIndices`)
    /// that shift the positional mapping. blocked-by: spread-element typing.
    // Go: internal/checker/checker.go:Checker.getContextualType (KindArrayLiteralExpression)
    fn get_contextual_type_for_array_literal_element(
        &mut self,
        program: &dyn BoundProgram,
        array_literal: NodeId,
        element: NodeId,
        context_flags: ContextFlags,
    ) -> Option<TypeId> {
        let t = self.get_apparent_type_of_contextual_type(program, array_literal, context_flags)?;
        let elements = array_literal_elements(program, array_literal);
        let element_index = elements.iter().position(|&e| e == element)?;
        self.get_contextual_type_for_element_expression(program, t, element_index, elements.len())
    }

    /// The contextual type for the element at `index` of a contextual array/
    /// tuple type `t`: a contextual property named by the index (the tuple/
    /// numeric-index path), else the iterated/element type of the contextual
    /// (array-like) type.
    ///
    /// DEFER(phase-4-checker-4bl+): the union `mapType` distribution, the tuple
    /// positional-element typing (fixed/variadic elements, spread offsets,
    /// `removeMissingType`), and the spread-index arguments. blocked-by:
    /// union contextual typing + contextual tuple positional typing + spread
    /// elements.
    // Go: internal/checker/checker.go:Checker.getContextualTypeForElementExpression
    fn get_contextual_type_for_element_expression(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        index: usize,
        _length: usize,
    ) -> Option<TypeId> {
        // If a contextual property with the element's numeric name exists, use
        // it (Go's `getTypeOfPropertyOfContextualType(t, index)`); otherwise the
        // iterated/element type of the contextual (array) type.
        if let Some(prop) =
            self.get_type_of_property_of_contextual_type(program, t, &index.to_string())
        {
            return Some(prop);
        }
        self.check_iterated_type_or_element_type(program, t, None, true)
    }

    /// The type of the property named `name` in a contextual type `t`: the
    /// matching property's type, else the value type of an applicable index
    /// signature, else `None`.
    ///
    /// DEFER(phase-4-checker-4bl+): the union `mapType` distribution, the
    /// intersection per-constituent combine, generic mapped types
    /// (`getIndexedMappedTypeSubstitutedTypeOfContextualType`), the circular
    /// mapped-property guard, and `removeMissingType` for optional members.
    /// blocked-by: union/intersection contextual typing + generic mapped types +
    /// optional-member missing-type stripping.
    // Go: internal/checker/checker.go:Checker.getTypeOfPropertyOfContextualType
    fn get_type_of_property_of_contextual_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        name: &str,
    ) -> Option<TypeId> {
        if !self.get_type(t).flags().contains(TypeFlags::OBJECT) {
            return None;
        }
        if let Some(prop) = get_property_of_type(self, t, name) {
            let globals = program.globals();
            return Some(get_type_of_symbol(self, program, prop, globals));
        }
        self.get_type_from_index_infos_of_contextual_type(program, t, name)
    }

    /// The value type of the index signature of `t` applicable to `name`, or
    /// `None` when none applies.
    ///
    /// DEFER(phase-4-checker-4bl+): the tuple numeric-rest element path
    /// (`getElementTypeOfSliceOfTupleType`). blocked-by: variadic/rest tuple
    /// element typing.
    // Go: internal/checker/checker.go:Checker.getTypeFromIndexInfosOfContextualType
    fn get_type_from_index_infos_of_contextual_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        name: &str,
    ) -> Option<TypeId> {
        let info = get_applicable_index_info_for_name(self, program, t, name)?;
        Some(self.index_info(info).value_type)
    }

    /// Returns the apparent type of `node`'s contextual type.
    ///
    /// 4bj is a thin wrapper over [`Checker::get_contextual_type`]: the reachable
    /// contextual types are object types (function types), for which
    /// `getApparentType` is the identity.
    ///
    /// DEFER(phase-4-checker-4bk+): the object-literal-method contextual type,
    /// `instantiateContextualType` (return-type/inference mappers), the
    /// `mapType` + `getApparentType` pass (primitive/type-variable apparent
    /// types), and union discrimination by object members / JSX attributes.
    /// blocked-by: inference contexts + apparent-type of primitives/type
    /// variables + union discrimination.
    // Go: internal/checker/checker.go:Checker.getApparentTypeOfContextualType
    pub(crate) fn get_apparent_type_of_contextual_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> Option<TypeId> {
        self.get_contextual_type(program, node, context_flags)
    }

    /// Returns the contextual signature of a function/arrow expression: when the
    /// contextual type is an object type with a single applicable call
    /// signature, that signature (whose parameter types contextually type the
    /// expression's parameters).
    ///
    /// DEFER(phase-4-checker-4bk+): the union-contextual-type case
    /// (`compareSignaturesIdentical` + `createUnionSignature`). blocked-by:
    /// signature-identity comparison + union signatures.
    // Go: internal/checker/checker.go:Checker.getContextualSignature
    pub(crate) fn get_contextual_signature(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Option<SignatureId> {
        let t =
            self.get_apparent_type_of_contextual_type(program, node, ContextFlags::SIGNATURE)?;
        if self.get_type(t).flags().contains(TypeFlags::UNION) {
            return None;
        }
        self.get_contextual_call_signature(program, t, node)
    }

    /// If `t` has a single call signature with at least as many parameters as
    /// the function `node`, returns it; otherwise `None`.
    ///
    /// DEFER(phase-4-checker-4bk+): `getIntersectedSignatures` (the
    /// `noImplicitAny` multi-signature combine). blocked-by: signature
    /// combination.
    // Go: internal/checker/checker.go:Checker.getContextualCallSignature
    fn get_contextual_call_signature(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        node: NodeId,
    ) -> Option<SignatureId> {
        let applicable: Vec<SignatureId> = self
            .get_signatures_of_type(t)
            .into_iter()
            .filter(|&s| !self.is_arity_smaller(program, s, node))
            .collect();
        if applicable.len() == 1 {
            return Some(applicable[0]);
        }
        None
    }

    /// Reports whether `signature` has fewer parameters than the function
    /// `target` requires (so it should not contextually type `target`).
    ///
    /// DEFER(phase-4-checker-4bk+): the `this`-parameter decrement and the
    /// effective-rest-parameter exemption (`hasEffectiveRestParameter`).
    /// blocked-by: `this`-typing + rest parameters.
    // Go: internal/checker/checker.go:Checker.isAritySmaller
    fn is_arity_smaller(
        &self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        target: NodeId,
    ) -> bool {
        let parameters = function_like_parameters(program, target);
        let mut target_parameter_count = 0usize;
        while target_parameter_count < parameters.len() {
            if parameter_is_optional_for_arity(program, parameters[target_parameter_count]) {
                break;
            }
            target_parameter_count += 1;
        }
        self.signature(signature).parameters.len() < target_parameter_count
    }

    /// Assigns each un-annotated parameter of `node` the type of the
    /// corresponding parameter of `context` (the contextual signature), caching
    /// it on the parameter symbol's links.
    ///
    /// A parameter whose links already hold a resolved type is left untouched
    /// (Go's `assignParameterType` early-return), and an annotated parameter
    /// keeps its annotation (Go never overrides an explicit type).
    ///
    /// DEFER(phase-4-checker-4bk+): the contextual type parameters / `this`
    /// parameter, the rest-parameter mapping (`getRestTypeAtPosition`), the
    /// default-initializer reconciliation (`checkDeclarationInitializer` +
    /// `widenTypeInferredFromInitializer`), `addOptionalityEx`, and the
    /// binding-pattern fallback. blocked-by: `this`/generic contextual inference
    /// + rest/tuple types + initializer widening + binding-pattern typing.
    ///
    /// Side effects: mutates the parameter symbols' value links.
    // Go: internal/checker/checker.go:Checker.assignContextualParameterTypes
    pub(crate) fn assign_contextual_parameter_types(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        context: SignatureId,
    ) {
        let parameters = function_like_parameters(program, node);
        for (i, &param) in parameters.iter().enumerate() {
            if declaration_type_node(program, param).is_some() {
                continue;
            }
            let Some(symbol) = program.symbol_of_node(param) else {
                continue;
            };
            if self
                .value_symbol_links
                .try_get(&symbol)
                .and_then(|l| l.resolved_type)
                .is_some()
            {
                continue;
            }
            if let Some(t) = self.try_get_type_at_position(program, context, i) {
                self.value_symbol_links.get(symbol).resolved_type = Some(t);
            }
        }
    }

    /// Returns the parameter type at position `pos` of `signature`, or `None`
    /// when `pos` is past the (non-rest) parameter list (Go's
    /// `tryGetTypeAtPosition`).
    ///
    /// DEFER(phase-4-checker-4bk+): the rest-parameter indexed access
    /// (`getIndexedAccessType` over the rest tuple). blocked-by: rest/tuple
    /// types.
    // Go: internal/checker/checker.go:Checker.tryGetTypeAtPosition
    pub(crate) fn try_get_type_at_position(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        pos: usize,
    ) -> Option<TypeId> {
        let symbol = self.signature(signature).parameters.get(pos).copied()?;
        let base = get_type_of_symbol(self, program, symbol, None);
        // An instantiated signature substitutes its parameter types through its
        // mapper (matching `get_type_at_position`), deep-instantiating an
        // anonymous object/function-type parameter so a callback argument is
        // contextually typed by the substituted parameter type.
        Some(match self.signature(signature).mapper.clone() {
            Some(mapper) => self.instantiate_param_type(program, base, &mapper),
            None => base,
        })
    }

    /// Widens a literal value type for a mutable location, *unless* its
    /// contextual type makes the position a "literal context", in which case the
    /// literal is preserved (only stripped of freshness). This is the engine of
    /// contextual typing into object/array literals: a `"x"` value in a `"x"`
    /// property position stays `"x"` instead of widening to `string`.
    ///
    /// DEFER(phase-4-checker-4bl+): the `getWidenedUniqueESSymbolType` step (no
    /// unique-symbol literal is typed yet). blocked-by: unique-ES-symbol typing.
    // Go: internal/checker/checker.go:Checker.getWidenedLiteralLikeTypeForContextualType
    pub(crate) fn get_widened_literal_like_type_for_contextual_type(
        &mut self,
        t: TypeId,
        contextual_type: Option<TypeId>,
    ) -> TypeId {
        let t = if self.is_literal_of_contextual_type(t, contextual_type) {
            t
        } else {
            self.get_widened_literal_type(t)
        };
        self.regular_type_of_literal_type(t)
    }

    /// Reports whether `candidate_type` (a literal) sits in a "literal context"
    /// implied by `contextual_type`: a string/number/boolean literal whose
    /// contextual type is a literal of the same primitive kind. Such a context
    /// preserves the literal instead of widening it.
    ///
    /// DEFER(phase-4-checker-4bl+): the union/intersection distribution
    /// (`Some(contextualType.Types(), ...)`), the instantiable-non-primitive
    /// constraint path (`T extends string` etc.), and the bigint /
    /// unique-ES-symbol / `Index` / template-literal / string-mapping literal
    /// kinds. blocked-by: union/intersection contextual typing + base-constraint
    /// resolution + bigint/unique-symbol/template-literal typing.
    // Go: internal/checker/checker.go:Checker.isLiteralOfContextualType
    pub(crate) fn is_literal_of_contextual_type(
        &self,
        candidate_type: TypeId,
        contextual_type: Option<TypeId>,
    ) -> bool {
        let Some(contextual_type) = contextual_type else {
            return false;
        };
        let context = self.get_type(contextual_type).flags();
        let candidate = self.get_type(candidate_type).flags();
        context.intersects(TypeFlags::STRING_LITERAL)
            && candidate.intersects(TypeFlags::STRING_LITERAL)
            || context.intersects(TypeFlags::NUMBER_LITERAL)
                && candidate.intersects(TypeFlags::NUMBER_LITERAL)
            || context.intersects(TypeFlags::BOOLEAN_LITERAL)
                && candidate.intersects(TypeFlags::BOOLEAN_LITERAL)
    }

    /// Assigns contextual parameter types to a contextually-typed function/arrow
    /// expression before its body is checked (the reachable core of Go's
    /// `contextuallyCheckFunctionExpressionOrObjectLiteralMethod`).
    ///
    /// DEFER(phase-4-checker-4bk+): the `NodeCheckFlagsContextChecked` re-entry
    /// guard, `assignNonContextualParameterTypes` (forcing resolution when there
    /// is no contextual signature), inference-context instantiation of the
    /// contextual signature, return-type-from-body inference, and
    /// `checkSignatureDeclaration`. blocked-by: node-check-flags +
    /// inference contexts + body return-type inference.
    ///
    /// Side effects: may mutate parameter symbols' value links.
    // Go: internal/checker/checker.go:Checker.contextuallyCheckFunctionExpressionOrObjectLiteralMethod
    pub(crate) fn contextually_check_function_expression(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) {
        if let Some(context) = self.get_contextual_signature(program, node) {
            self.assign_contextual_parameter_types(program, node, context);
        }
    }
}

/// Returns the initializer node of a variable-like declaration, if any.
fn declaration_initializer(program: &dyn BoundProgram, declaration: NodeId) -> Option<NodeId> {
    match program.arena().data(declaration) {
        NodeData::VariableDeclaration(d) => d.initializer,
        NodeData::ParameterDeclaration(d) => d.initializer,
        NodeData::PropertyDeclaration(d) => d.initializer,
        NodeData::BindingElement(d) => d.initializer,
        _ => None,
    }
}

/// Returns the type-annotation node of a variable-like declaration, if any.
fn declaration_type_node(program: &dyn BoundProgram, declaration: NodeId) -> Option<NodeId> {
    match program.arena().data(declaration) {
        NodeData::VariableDeclaration(d) => d.type_node,
        NodeData::ParameterDeclaration(d) => d.type_node,
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => d.type_node,
        _ => None,
    }
}

/// Reports whether `node` is a function expression or arrow function (Go's
/// `ast.IsFunctionExpressionOrArrowFunction`).
fn is_function_expression_or_arrow(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().kind(node),
        Kind::ArrowFunction | Kind::FunctionExpression
    )
}

/// Returns the parameter list of a function-like node (arrow/function
/// expression / function declaration / function type), or empty otherwise.
fn function_like_parameters(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::ArrowFunction(d) => d.parameters.nodes.clone(),
        NodeData::FunctionExpression(d) | NodeData::FunctionDeclaration(d) => {
            d.parameters.nodes.clone()
        }
        NodeData::FunctionType(d) | NodeData::ConstructorType(d) => d.parameters.nodes.clone(),
        _ => Vec::new(),
    }
}

/// Returns the index of `parameter` within `fn_node`'s parameter list.
fn parameter_index(
    program: &dyn BoundProgram,
    fn_node: NodeId,
    parameter: NodeId,
) -> Option<usize> {
    function_like_parameters(program, fn_node)
        .iter()
        .position(|&p| p == parameter)
}

/// Reports whether a parameter is optional for arity purposes (a `?` token, a
/// default initializer, or a rest `...` parameter).
fn parameter_is_optional_for_arity(program: &dyn BoundProgram, parameter: NodeId) -> bool {
    match program.arena().data(parameter) {
        NodeData::ParameterDeclaration(d) => {
            d.question_token.is_some() || d.initializer.is_some() || d.dot_dot_dot_token.is_some()
        }
        _ => false,
    }
}

/// Returns the static (non-computed) property name of an object-literal
/// property/shorthand element, or `None` for a computed/dynamic name (which has
/// no statically-known name to look up in a contextual type).
fn object_literal_element_static_name(
    program: &dyn BoundProgram,
    element: NodeId,
) -> Option<String> {
    let name_node = match program.arena().data(element) {
        NodeData::PropertyAssignment(d) => d.name,
        NodeData::ShorthandPropertyAssignment(d) => d.name,
        _ => return None,
    };
    match program.arena().kind(name_node) {
        Kind::Identifier | Kind::StringLiteral | Kind::NumericLiteral => {
            Some(program.arena().text(name_node).to_string())
        }
        _ => None,
    }
}

/// Returns the argument nodes of a call/`new` expression (the reachable subset
/// of Go's `getEffectiveCallArguments`: the raw argument list, with no spread
/// expansion or synthetic tagged-template/decorator/JSX arguments). A `new`
/// expression with no argument list yields an empty slice.
fn call_arguments(program: &dyn BoundProgram, call_target: NodeId) -> Vec<NodeId> {
    match program.arena().data(call_target) {
        NodeData::CallExpression(d) => d.arguments.nodes.clone(),
        NodeData::NewExpression(d) => d
            .arguments
            .as_ref()
            .map(|a| a.nodes.clone())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Returns the element nodes of an array-literal expression.
fn array_literal_elements(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::ArrayLiteralExpression(d) => d.list.nodes.clone(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "contextual_test.rs"]
mod tests;
