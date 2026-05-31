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

use super::declared_types::{get_type_from_type_node, get_type_of_symbol};
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
    ///
    /// DEFER(phase-4-checker-4bk+): the `NodeFlagsInWithStatement` early-out, the
    /// cached-contextual-node lookup (`findContextualNode`/`contextualInfos`),
    /// and every other parent arm — call/`new` arguments
    /// (`getContextualTypeForArgument`, blocked-by: call resolution's contextual
    /// pass), return/arrow-body (`getContextualTypeForReturnExpression`),
    /// yield/await operands, decorators, `as`/`satisfies`/parenthesized/non-null
    /// pass-through, object/array-literal elements (the inverse direction:
    /// type flowing into a literal), conditional/template operands, and JSX.
    /// blocked-by: call-resolution contextual pass + return-position inference +
    /// literal contextual typing + JSX.
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
            Kind::BinaryExpression => {
                self.get_contextual_type_for_binary_operand(program, node, context_flags)
            }
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
        Some(get_type_of_symbol(self, program, symbol, None))
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

#[cfg(test)]
#[path = "contextual_test.rs"]
mod tests;
