//! Expression/statement checking and diagnostics.
//!
//! Ports the reachable core of Go's `checker.go` `checkSourceFile` →
//! `checkSourceElement` → `checkExpression` recursion plus `getDiagnostics`.
//! Covers literals, identifiers, property/element access, a minimal call
//! resolution, simple-assignment assignability, the non-assignment relational /
//! equality / arithmetic binary-operator arms, variable-declaration
//! assignability, and the statement-container kinds that recurse (block / if /
//! while / do / for / for-in / for-of / try / switch); the type of each
//! expression feeds the 4f flow engine.
//!
//! DEFER(phase-4-checker-4ab+): the `with` statement
//! (reachable path is grammar-only), module declaration bodies, contextual
//! typing, unused checks, and the full node builder. The logical (`&&`/`||`/`??`)
//! and `+` operators, compound assignments, and `throw`/labeled statements landed
//! in 4p; call-expression argument checking in 4q; overload resolution, class
//! member bodies / property initializers, and function-body descent with
//! return-statement / annotated return-type checking in 4r; the `instanceof`
//! (`2358`/`2359`, driven by a synthetic global `Function`) and `in`
//! (operand assignability `2322`) operators in 4ab; the comma operator in 4ab
//! slice 2 (result type + `2695` unused-left diagnostic).

use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};
use tsgo_ast::symbol::{INTERNAL_SYMBOL_NAME_COMPUTED, INTERNAL_SYMBOL_NAME_EXPORT_EQUALS};
use tsgo_ast::{
    utilities::is_assignment_operator, CheckFlags, Kind, ModifierFlags, NodeData, NodeFlags,
    NodeId, SymbolFlags, SymbolId, SymbolTable,
};
use tsgo_core::compileroptions::ScriptTarget;
use tsgo_diagnostics::{Category, Message};

use super::contextual::ContextFlags;
use super::declared_types::{
    combined_node_flags, fill_missing_type_arguments, get_apparent_type,
    get_applicable_index_info, get_applicable_index_info_for_name, get_applicable_index_infos,
    get_class_construct_signatures, get_constraint_of_type_parameter,
    get_declaration_of_alias_symbol, get_declared_type_of_symbol, get_default_from_type_parameter,
    has_base_type, get_index_info_of_type, get_index_infos_of_type,
    get_index_type_of_type, get_indexed_access_type, get_min_type_argument_count,
    get_property_of_type,     get_property_of_union_or_intersection_type, get_properties_of_type,
    get_explicit_accessor_return_type, get_property_type_for_index_type, get_type_from_type_node,
    get_type_of_property_of_type, get_type_of_symbol, get_type_without_signatures,
    is_generic_object_type, resolve_alias,
};
use super::inference::{InferenceContext, InferencePriority};
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::late_binding::get_property_name_from_type;
use super::reachability::{is_instantiated_module, module_is_value_module};
use super::relations::RelationKind;
use super::signatures::{IndexInfo, IndexInfoId, Signature, SignatureFlags, SignatureId};
use super::symbols::resolve_name;
use super::type_facts::TypeFacts;
use super::types::{
    IntrinsicType, LiteralValue, ObjectFlags, ObjectType, TypeData, TypeFlags, TypeId,
};
use super::Checker;

/// A type-checking diagnostic produced while checking a source file.
///
/// A minimal stand-in for Go's `ast.Diagnostic` (which also carries the file);
/// 4g records the span, code, category, and localized text, 4aj adds the
/// related-information list (Go's `relatedInformation`), and 4bn adds the
/// nested [`message_chain`](Diagnostic::message_chain) (Go's `messageChain`)
/// that carries relation-error elaboration ("Types of property 'x' are
/// incompatible." over a leaf "Type 'string' is not assignable to type
/// 'number'.").
///
/// DEFER(phase-4-checker-4j): the owning `SourceFile`. blocked-by: the real
/// `ast.Diagnostic`/`DiagnosticsCollection` (program-level, P6) and the node
/// builder (4j).
// Go: internal/ast/diagnostic.go:Diagnostic (subset)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The numeric diagnostic code (e.g. `2304`).
    pub code: i32,
    /// The diagnostic category (error/warning/...).
    pub category: Category,
    /// The localized, argument-substituted message text.
    pub message: String,
    /// Start position in the source text.
    pub start: i32,
    /// Length of the flagged span.
    pub length: i32,
    /// Sub-diagnostics attached to this one as related information (Go's
    /// `Diagnostic.relatedInformation`), e.g. a `2489` "iterator must have a
    /// `next()` method" hung under a primary `2488` "not iterable". Empty for
    /// most diagnostics; populated via [`Diagnostic::add_related_info`].
    pub related_information: Vec<Diagnostic>,
    /// The nested elaboration chain hung under this diagnostic (Go's
    /// `Diagnostic.messageChain`). Empty for most diagnostics; a failed
    /// assignability check fills it with the "Types of property 'x' are
    /// incompatible." / "Property 'x' is missing ..." elaboration produced by
    /// the relation engine's reporting path. Mirrors how Go nests
    /// `[]*Diagnostic` under a head diagnostic.
    pub message_chain: Vec<DiagnosticMessageChain>,
}

/// One node of a diagnostic's nested elaboration chain (Go's `messageChain`
/// entries, which are themselves `*Diagnostic`s).
///
/// The relation engine builds these head-to-leaf when an assignability check
/// fails: a head [`Diagnostic`] (e.g. `2322` "Type 'A' is not assignable to
/// type 'B'.") carries a [`message_chain`](Diagnostic::message_chain) of these,
/// each of which may carry its own [`next`](DiagnosticMessageChain::next) child,
/// bottoming out at a leaf (e.g. `2322` "Type 'string' is not assignable to
/// type 'number'.").
///
/// # Examples
/// ```
/// use tsgo_checker::{Category, DiagnosticMessageChain};
/// let leaf = DiagnosticMessageChain {
///     code: 2322,
///     category: Category::Error,
///     message: "Type 'string' is not assignable to type 'number'.".to_string(),
///     next: Vec::new(),
/// };
/// let parent = DiagnosticMessageChain {
///     code: 2326,
///     category: Category::Error,
///     message: "Types of property 'a' are incompatible.".to_string(),
///     next: vec![leaf],
/// };
/// assert_eq!(parent.next[0].code, 2322);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ast/diagnostic.go:Diagnostic.messageChain (a chain entry is a *Diagnostic)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticMessageChain {
    /// The numeric diagnostic code (e.g. `2326`).
    pub code: i32,
    /// The diagnostic category (error/warning/...).
    pub category: Category,
    /// The localized, argument-substituted message text.
    pub message: String,
    /// Nested child elaboration(s) hung under this entry (Go's per-entry
    /// `messageChain`). For the structural-object relation chain this holds 0
    /// or 1 child; modeled as a `Vec` to mirror Go's `[]*Diagnostic`.
    pub next: Vec<DiagnosticMessageChain>,
}

impl Diagnostic {
    /// Attaches `related` as a related-information sub-diagnostic of `self` and
    /// returns `&mut self` for chaining (Go's `Diagnostic.AddRelatedInfo`).
    ///
    /// This is additive: a freshly built [`Diagnostic`] starts with an empty
    /// [`related_information`](Diagnostic::related_information) list, so callers
    /// that never attach related info are unaffected.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Category, Diagnostic};
    /// let related = Diagnostic {
    ///     code: 2489,
    ///     category: Category::Error,
    ///     message: "An iterator must have a 'next()' method.".to_string(),
    ///     start: 0,
    ///     length: 1,
    ///     related_information: Vec::new(),
    ///     message_chain: Vec::new(),
    /// };
    /// let mut primary = Diagnostic {
    ///     code: 2488,
    ///     category: Category::Error,
    ///     message: "Type 'T' must have a '[Symbol.iterator]()' method that returns an iterator.".to_string(),
    ///     start: 0,
    ///     length: 1,
    ///     related_information: Vec::new(),
    ///     message_chain: Vec::new(),
    /// };
    /// primary.add_related_info(related);
    /// assert_eq!(primary.related_information.len(), 1);
    /// assert_eq!(primary.related_information[0].code, 2489);
    /// ```
    ///
    /// Side effects: pushes `related` onto `self.related_information`.
    // Go: internal/ast/diagnostic.go:Diagnostic.AddRelatedInfo
    pub fn add_related_info(&mut self, related: Diagnostic) -> &mut Self {
        self.related_information.push(related);
        self
    }
}

// Selects the diagnostic family for a possibly-`null`/`undefined` operand, the
// port of Go's `reportError` function pointer threaded through
// `checkNonNullTypeWithReporter`.
// Go: internal/checker/checker.go:Checker.checkNonNullTypeWithReporter(7381)
#[derive(Clone, Copy, PartialEq, Eq)]
enum NonNullReporter {
    /// A property/element access object (`reportObjectPossiblyNullOrUndefinedError`).
    Access,
    /// A call callee (`reportCannotInvokePossiblyNullOrUndefinedError`).
    Invocation,
}

/// Syntactic truthiness classification for a condition expression (Go's
/// `PredicateSemantics`).
// Go: internal/checker/checker.go:PredicateSemantics
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PredicateSemantics {
    /// No fixed truthiness.
    None = 0,
    /// Always truthy.
    Always = 1,
    /// Always falsy.
    Never = 2,
    /// May be either (Go's `PredicateSemanticsSometimes`).
    Sometimes = 3,
}

impl std::ops::BitOr for PredicateSemantics {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::None, other) | (other, Self::None) => other,
            (Self::Always, Self::Always) => Self::Always,
            (Self::Never, Self::Never) => Self::Never,
            _ => Self::Sometimes,
        }
    }
}

// A typed object-literal member, recorded in declaration order to compute index
// signatures (Go's `propertiesArray` entries). The port's synthesized property
// symbols carry no declarations, so the computed-name expression's type is kept
// alongside the symbol (Go reads it from `prop.Declarations[0].Name()`).
struct ObjectLiteralMember {
    // The synthesized property symbol carrying the member's value type.
    symbol: tsgo_ast::SymbolId,
    // The computed property name's expression type, or `None` for a
    // statically-named member.
    computed_name_type: Option<TypeId>,
}

// Which signature list an invocation diagnostic consults (Go's `SignatureKind`).
// Go: internal/checker/types.go:SignatureKind
#[derive(Copy, Clone, Eq, PartialEq)]
enum InvocationKind {
    Call,
    Construct,
}

impl Checker {
    /// Computes the type of an expression `node` (Go's `checkExpression`).
    ///
    /// 4g handles literals, identifiers (resolved + flow-narrowed), property and
    /// element access, and calls; unhandled kinds yield the error type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) {
    /// let _ = c.check_expression(p, n);
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics and allocate types.
    // Go: internal/checker/checker.go:Checker.checkExpression(7521)/checkExpressionWorker(7699)
    pub fn check_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        // Go: checkExpression saves/restores currentNode and resets
        // instantiationCount for each expression check.
        let saved_current_node = self.current_node;
        self.current_node = Some(node);
        self.instantiation_count = 0;
        let result = self.check_expression_worker(program, node);
        self.current_node = saved_current_node;
        result
    }

    /// Checks `node` with an explicit `contextual_type`, optionally participating
    /// in generic inference via `inference_context` (Go's
    /// `checkExpressionWithContextualType`).
    ///
    /// Pushes the contextual type onto the checker stack so
    /// [`Checker::get_contextual_type`] sees it, checks the expression, then
    /// strips literal freshness when the result is a literal in a matching
    /// literal context. The `inference_context` arm (inferential check mode and
    /// intra-expression inference sites) is deferred.
    ///
    /// Side effects: may record diagnostics, allocate types, and mutate the
    /// contextual-type stack.
    // Go: internal/checker/checker.go:Checker.checkExpressionWithContextualType
    pub(crate) fn check_expression_with_contextual_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        contextual_type: TypeId,
        _inference_context: Option<&mut InferenceContext>,
    ) -> TypeId {
        let context_node = get_context_node(program, node);
        self.push_contextual_type(context_node, contextual_type, false);
        let t = if super::contextual::is_function_expression_or_arrow(program, node) {
            // Context-sensitive callback arguments: assign parameter types from
            // the contextual signature, check the body, and build the function
            // type for return-type inference (Go's inferential
            // `checkExpressionWithContextualType` on a function-like node).
            if let Some(&ctx_sig) = self.get_signatures_of_type(contextual_type).first() {
                self.assign_contextual_parameter_types(program, node, ctx_sig);
            }
            self.get_type_of_context_sensitive_arrow(program, node)
        } else {
            self.check_expression(program, node)
        };
        let t = if self.is_literal_type(t)
            && self.is_literal_of_contextual_type(t, Some(contextual_type))
        {
            self.regular_type_of_literal_type(t)
        } else {
            t
        };
        self.pop_contextual_type();
        t
    }

    /// Returns the semantic type of `node` for tooling queries (Go's
    /// `getTypeOfNode` / `GetTypeAtLocation`).
    ///
    /// Reachable subset: whole non-module source files and nodes inside `with`
    /// blocks yield the error type; type-syntax nodes use
    /// [`Checker::get_type_from_type_node`]; expressions use
    /// [`Checker::get_regular_type_of_expression`]; declarations and declaration
    /// names resolve through their symbols.
    ///
    /// Side effects: may check expressions and allocate types.
    // Go: internal/checker/checker.go:Checker.getTypeOfNode
    pub(crate) fn get_type_of_node(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        globals: Option<&SymbolTable>,
    ) -> TypeId {
        let arena = program.arena();
        if arena.kind(node) == Kind::SourceFile
            || arena.flags(node).contains(NodeFlags::IN_WITH_STATEMENT)
        {
            return self.error_type();
        }
        if is_part_of_type_node(program, node) {
            return self.get_type_from_type_node(program, node, globals);
        }
        if is_expression_node(program, node) {
            return self.get_regular_type_of_expression(program, node);
        }
        if super::symbols_query::is_declaration(arena, node) {
            return match super::symbols_query::get_symbol_of_declaration(program, node) {
                Some(symbol) => get_type_of_symbol(self, program, symbol, globals),
                None => self.error_type(),
            };
        }
        if super::symbols_query::is_declaration_name(arena, node) {
            return match super::symbols_query::get_symbol_at_location(self, program, node, globals)
            {
                Some(symbol) => get_type_of_symbol(self, program, symbol, globals),
                None => self.error_type(),
            };
        }
        self.error_type()
    }

    /// The regular (non-fresh) type of an expression `expr` (Go's
    /// `getRegularTypeOfExpression`).
    ///
    /// Side effects: may check `expr` and allocate types.
    // Go: internal/checker/checker.go:Checker.getRegularTypeOfExpression
    pub(crate) fn get_regular_type_of_expression(
        &mut self,
        program: &dyn BoundProgram,
        expr: NodeId,
    ) -> TypeId {
        let expr = if is_right_side_of_qualified_name_or_property_access(program, expr) {
            program.arena().parent(expr).unwrap_or(expr)
        } else {
            expr
        };
        let t = self.check_expression(program, expr);
        self.regular_type_of_literal_type(t)
    }

    /// Resolves a type-syntax node to its type (Go's `getTypeFromTypeNode`).
    ///
    /// Side effects: may allocate types.
    // Go: internal/checker/checker.go:Checker.getTypeFromTypeNode
    pub(crate) fn get_type_from_type_node(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        globals: Option<&SymbolTable>,
    ) -> TypeId {
        get_type_from_type_node(self, program, node, globals)
    }

    /// Returns the type of the property named `name` on object type `t` (Go's
    /// `getTypeOfPropertyOfType`).
    ///
    /// Side effects: may allocate synthesized properties.
    // Go: internal/checker/checker.go:Checker.getTypeOfPropertyOfType
    pub(crate) fn get_type_of_property_of_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        name: &str,
    ) -> Option<TypeId> {
        get_type_of_property_of_type(self, program, t, name)
    }

    /// Resolves `object_type[index_type]` to the selected property/element type
    /// (Go's `getPropertyTypeForIndexType`).
    ///
    /// Side effects: may allocate types.
    // Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType
    pub(crate) fn get_property_type_for_index_type(
        &mut self,
        program: &dyn BoundProgram,
        object_type: TypeId,
        index_type: TypeId,
    ) -> Option<TypeId> {
        get_property_type_for_index_type(self, program, object_type, index_type)
    }

    /// Returns the resolved return type of `signature` (Go's
    /// `getReturnTypeOfSignature` for an already-resolved signature).
    ///
    /// Side effects: none for signatures whose return type is already resolved.
    // Go: internal/checker/checker.go:Checker.getReturnTypeOfSignature
    pub(crate) fn get_return_type_of_signature(&self, signature: SignatureId) -> TypeId {
        self.signature_return_type(signature)
    }

    // Returns the `this` type declared by a signature's leading `this` parameter,
    // or `None` when the signature has no explicit `this` type.
    // Go: internal/checker/relater.go:Checker.getThisTypeOfSignature(1911)
    fn get_this_type_of_signature(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
    ) -> Option<TypeId> {
        let this_parameter = self.signature(signature).this_parameter?;
        Some(super::declared_types::get_type_of_symbol(
            self, program, this_parameter, None,
        ))
    }

    fn check_expression_worker(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        match program.arena().kind(node) {
            Kind::Identifier => self.check_identifier(program, node),
            Kind::StringLiteral => {
                // Go: getFreshTypeOfLiteralType(getStringLiteralType(text)). The
                // value-keyed intern gives every `"a"` one regular TypeId; the
                // fresh wrapping makes the *expression* carry the fresh form,
                // which `getWidenedLiteralType` widens in a mutable binding.
                let text = program.arena().text(node).to_string();
                let regular = self.get_string_literal_type(&text);
                self.get_fresh_type_of_literal_type(regular)
            }
            Kind::NumericLiteral => {
                // Go: getFreshTypeOfLiteralType(getNumberLiteralType(value)).
                let value = tsgo_jsnum::from_string(program.arena().text(node));
                let regular = self.get_number_literal_type(value);
                self.get_fresh_type_of_literal_type(regular)
            }
            Kind::BigIntLiteral => {
                // Go: getFreshTypeOfLiteralType(getBigIntLiteralType(...)).
                let text = program.arena().text(node);
                let regular = self.get_bigint_literal_type(text);
                self.get_fresh_type_of_literal_type(regular)
            }
            Kind::TrueKeyword => self.true_type,
            Kind::FalseKeyword => self.false_type,
            Kind::NullKeyword => self.null_type,
            Kind::ThisKeyword => self.check_this_expression(program, node),
            Kind::PropertyAccessExpression => self.check_property_access(program, node),
            Kind::ElementAccessExpression => self.check_element_access(program, node),
            Kind::CallExpression => {
                if is_import_call(program, node) {
                    self.check_import_call_expression(program, node)
                } else {
                    self.check_call_expression(program, node)
                }
            }
            Kind::NewExpression => self.check_new_expression(program, node),
            Kind::TaggedTemplateExpression => {
                self.check_tagged_template_expression(program, node)
            }
            Kind::PrefixUnaryExpression => self.check_prefix_unary_expression(program, node),
            Kind::PostfixUnaryExpression => self.check_postfix_unary_expression(program, node),
            Kind::BinaryExpression => self.check_binary_expression(program, node),
            Kind::JsxSelfClosingElement => self.check_jsx_self_closing_element(program, node),
            Kind::JsxElement => self.check_jsx_element(program, node),
            Kind::JsxFragment => self.check_jsx_fragment(program, node),
            Kind::ObjectLiteralExpression => self.check_object_literal(program, node),
            Kind::ArrayLiteralExpression => self.check_array_literal(program, node),
            Kind::FunctionExpression => self.check_function_expression(program, node),
            Kind::ClassExpression => self.check_class_expression(program, node),
            Kind::ArrowFunction => self.check_arrow_function(program, node),
            Kind::NonNullExpression => self.check_non_null_assertion(program, node),
            Kind::AsExpression | Kind::TypeAssertionExpression => {
                self.check_assertion(program, node)
            }
            Kind::SatisfiesExpression => self.check_satisfies_expression(program, node),
            Kind::MetaProperty => self.check_meta_property(program, node),
            Kind::SuperKeyword => self.check_super_expression(program, node),
            // A parenthesized expression `(expr)` has the type of its inner
            // expression (Go's `checkParenthesizedExpression` ->
            // `checkExpressionEx(node.Expression())`).
            Kind::ParenthesizedExpression => {
                let inner = match program.arena().data(node) {
                    NodeData::ParenthesizedExpression(d) => d.expression,
                    _ => return self.error_type,
                };
                self.check_expression(program, inner)
            }
            Kind::TypeOfExpression => self.check_typeof_expression(program, node),
            Kind::VoidExpression => self.check_void_expression(program, node),
            Kind::DeleteExpression => self.check_delete_expression(program, node),
            // DEFER(phase-4-checker-4h+): remaining expression kinds are added in
            // later 4g slices / sub-phases.
            _ => self.error_type,
        }
    }

    // Checks an `expr as T` assertion (Go's `checkAssertion`). For a `const`
    // type reference (`as const`) the result is `getRegularTypeOfLiteralType` of
    // the operand's type: stripping freshness yields a regular literal, which
    // `getWidenedLiteralType` then leaves unchanged in a mutable binding, so the
    // literal value is preserved (e.g. `"a" as const` stays `"a"` instead of
    // widening to `string`).
    //
    // A non-const assertion takes the asserted type as its result.
    //
    // DEFER(phase-4-checker-4be+): the `erasableSyntaxOnly` grammar diagnostic
    // for `<T>expr` in erasable-syntax-only mode. blocked-by: erasable-syntax
    // option wiring.
    // Go: internal/checker/checker.go:Checker.checkAssertion(12238)
    fn check_assertion(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, type_node) = match program.arena().data(node) {
            NodeData::AsExpression(d) => (d.expression, d.type_node),
            NodeData::TypeAssertionExpression(d) => (d.expression, d.type_node),
            _ => return self.error_type,
        };
        let expr_type = self.check_expression(program, expr);
        if is_const_type_reference(program.arena(), type_node) {
            if !is_valid_const_assertion_argument(self, program, expr) {
                self.error(
                    program,
                    expr,
                    &tsgo_diagnostics::A_CONST_ASSERTION_CAN_ONLY_BE_APPLIED_TO_REFERENCES_TO_ENUM_MEMBERS_OR_STRING_NUMBER_BOOLEAN_ARRAY_OR_OBJECT_LITERALS,
                    &[],
                );
            }
            return self.regular_type_of_literal_type(expr_type);
        }
        let globals = program.globals();
        let target_type =
            super::declared_types::get_type_from_type_node(self, program, type_node, globals);
        self.check_assertion_comparability(program, node, expr_type, target_type);
        target_type
    }

    // Reports `2352` when a non-const assertion's operand type is not comparable
    // to the asserted type (Go's `checkAssertionDeferred`).
    // Go: internal/checker/checker.go:Checker.checkAssertionDeferred(12259)
    fn check_assertion_comparability(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        expr_type: TypeId,
        target_type: TypeId,
    ) {
        if target_type == self.error_type {
            return;
        }
        let base = self.get_base_type_of_literal_type(expr_type);
        let widened = self.get_widened_type(base);
        if self.is_type_comparable_to(program, target_type, widened) {
            return;
        }
        let source_str = super::nodebuilder::type_to_string(self, program, expr_type);
        let target_str = super::nodebuilder::type_to_string(self, program, target_type);
        self.error(
            program,
            node,
            &tsgo_diagnostics::CONVERSION_OF_TYPE_0_TO_TYPE_1_MAY_BE_A_MISTAKE_BECAUSE_NEITHER_TYPE_SUFFICIENTLY_OVERLAPS_WITH_THE_OTHER_IF_THIS_WAS_INTENTIONAL_CONVERT_THE_EXPRESSION_TO_UNKNOWN_FIRST,
            &[source_str.as_str(), target_str.as_str()],
        );
    }

    /// Checks a dynamic `import()` call expression.
    ///
    /// Runs grammar checks, type-checks any arguments, and returns
    /// `Promise<any>` (the full module-symbol resolution is deferred).
    ///
    /// DEFER(phase-4-checker-later): `resolveExternalModuleName`,
    /// `createPromiseReturnType`, `getTypeWithSyntheticDefaultImportType`.
    // Go: internal/checker/checker.go:Checker.checkImportCallExpression(8235)
    fn check_import_call_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        self.check_grammar_import_call_expression(program, node);
        let args = match program.arena().data(node) {
            NodeData::CallExpression(d) => d.arguments.nodes.clone(),
            _ => return self.error_type,
        };
        for &arg in &args {
            self.check_expression(program, arg);
        }
        // Full module resolution + promise wrapping deferred.
        self.any_type
    }

    /// Checks a `satisfies` expression (`expr satisfies T`).
    ///
    /// Unlike `as`, `satisfies` returns the **expression's** type — the
    /// assertion only validates that the expression type is assignable to the
    /// target type, reporting TS1360 on failure.
    // Go: internal/checker/checker.go:Checker.checkSatisfiesExpression(10699)
    fn check_satisfies_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, type_node) = match program.arena().data(node) {
            NodeData::SatisfiesExpression(d) => (d.expression, d.type_node),
            _ => return self.error_type,
        };
        let expr_type = self.check_expression(program, expr);
        let globals = program.globals();
        let target_type =
            super::declared_types::get_type_from_type_node(self, program, type_node, globals);
        if target_type == self.error_type {
            return target_type;
        }
        if !self.is_type_assignable_to(program, expr_type, target_type) {
            let source_str = super::nodebuilder::type_to_string(self, program, expr_type);
            let target_str = super::nodebuilder::type_to_string(self, program, target_type);
            self.error(
                program,
                node,
                &tsgo_diagnostics::TYPE_0_DOES_NOT_SATISFY_THE_EXPECTED_TYPE_1,
                &[source_str.as_str(), target_str.as_str()],
            );
        }
        expr_type
    }

    /// Checks a meta-property expression (`new.target` or `import.meta`).
    ///
    /// Dispatches grammar check, then `checkNewTargetMetaProperty` (TS17013)
    /// or `checkImportMetaProperty` (TS1343) based on keyword.
    ///
    /// DEFER(phase-4-checker-later): real return types (the `new.target` type
    /// is `getTypeOfSymbol` on the containing constructor, `import.meta` is
    /// `ImportMeta`).
    // Go: internal/checker/checker.go:Checker.checkMetaProperty(10712)
    fn check_meta_property(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        self.check_grammar_meta_property(program, node);
        let keyword = match program.arena().data(node) {
            NodeData::MetaProperty(d) => d.keyword_token,
            _ => return self.error_type,
        };
        match keyword {
            Kind::NewKeyword => self.check_new_target_meta_property(program, node),
            Kind::ImportKeyword => {
                // `import.meta` / `import.defer` — type is deferred.
                self.error_type
            }
            _ => self.error_type,
        }
    }

    /// Checks `new.target`. Reports TS17013 if `new.target` is used outside
    /// a function declaration, function expression, or constructor.
    // Go: internal/checker/checker.go:Checker.checkNewTargetMetaProperty(10727)
    fn check_new_target_meta_property(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        let container = get_new_target_container(program, node);
        if container.is_none() {
            self.error(
                program,
                node,
                &tsgo_diagnostics::META_PROPERTY_0_IS_ONLY_ALLOWED_IN_THE_BODY_OF_A_FUNCTION_DECLARATION_FUNCTION_EXPRESSION_OR_CONSTRUCTOR,
                &["new.target"],
            );
            return self.error_type;
        }
        // Full type resolution (`getTypeOfSymbol(symbol)`) deferred.
        self.error_type
    }

    // Types an object literal `{ a: 1, b: "x" }` as a fresh anonymous object
    // type whose properties are synthesized (transient) symbols carrying each
    // member initializer's (widened) type (Go's `checkObjectLiteral` ->
    // `createObjectLiteralType` over `newAnonymousType`).
    //
    // A member whose name is a non-literal computed property name (`[k]: v`
    // where `k: string`) does NOT become a named property; instead it
    // contributes its value type to a string/number/symbol index signature on
    // the object type (Go's `hasComputed*Property` flags feeding
    // `getObjectLiteralIndexInfo`).
    //
    // In a const context (`{ a: 1 } as const`) every property symbol carries the
    // `Readonly` check flag and its value type is kept as a literal (Go's
    // `checkFlags = CheckFlagsReadonly` + the const-context
    // `checkExpressionForMutableLocation` path); the index signatures are
    // readonly too.
    //
    // DEFER(phase-4-checker-4bi+): contextual typing (the type flowing INTO the
    // literal) for destructuring patterns, generic object-literal method
    // instantiation, and accessor body return-type inference without annotations.
    // blocked-by: destructuring-assignment typing, generic call-site inference,
    // and full accessor typing (`getTypeOfAccessors`).
    // Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076)
    fn check_object_literal(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        if !self.object_literals_resolving.insert(node) {
            return self.error_type;
        }
        let result = self.check_object_literal_worker(program, node);
        self.object_literals_resolving.remove(&node);
        result
    }

    fn check_object_literal_worker(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let members_nodes = match program.arena().data(node) {
            NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
            _ => return self.error_type,
        };
        // DEFER(phase-4-checker-C5+): `ast.IsAssignmentTarget(node)` to detect
        // destructuring patterns. blocked-by: Rust-side `is_assignment_target`.
        let in_destructuring = false;
        self.check_grammar_object_literal_expression(program, node, in_destructuring);
        // In a const context (`{ a: 1 } as const`) every property is readonly:
        // Go sets `checkFlags = CheckFlagsReadonly` when `isConstContext(node)`
        // and passes it to `newSymbolEx` for each member, and makes the index
        // signatures readonly too. The property *value* types are kept as
        // literals through the same const-context path in
        // `checkExpressionForMutableLocation`.
        let in_const_context = is_const_context(program, node);
        let member_check_flags = if in_const_context {
            tsgo_ast::CheckFlags::READONLY
        } else {
            tsgo_ast::CheckFlags::empty()
        };
        let mut members = SymbolTable::default();
        let mut properties: Vec<tsgo_ast::SymbolId> = Vec::new();
        // Every typed member (named AND computed) in declaration order, used to
        // compute index-signature value types (Go's `propertiesArray`).
        let mut all_members: Vec<ObjectLiteralMember> = Vec::new();
        let mut has_computed_string_property = false;
        let mut has_computed_number_property = false;
        let mut has_computed_symbol_property = false;
        let mut index_infos: Vec<IndexInfoId> = Vec::new();
        let mut spread_acc: Option<TypeId> = None;
        let spread_only = members_nodes.len() == 1;
        for member_decl in members_nodes {
            if let NodeData::SpreadAssignment(d) = program.arena().data(member_decl) {
                let raw_spread_type = self.check_expression(program, d.expression);
                let spread_type =
                    self.normalize_type_for_spread_validation(program, raw_spread_type);
                if !self.is_valid_spread_type(program, spread_type) {
                    self.error(
                        program,
                        member_decl,
                        &tsgo_diagnostics::SPREAD_TYPES_MAY_ONLY_BE_CREATED_FROM_OBJECT_TYPES,
                        &[],
                    );
                    continue;
                }
                if let Some(union_members) = self
                    .get_type(spread_type)
                    .union_types()
                    .map(|m| m.to_vec())
                {
                    // A spread-only object literal defers to `getSpreadType` so
                    // union-with-empty-object rewriting and distribution both
                    // match Go (`getSpreadType(empty, A | B)` / `tryMerge…`).
                    if members.is_empty() && all_members.is_empty() && spread_only {
                        return self.get_spread_type(
                            program,
                            self.empty_object_type(),
                            spread_type,
                            in_const_context,
                        );
                    }
                    // Spreading a union after named members distributes
                    // `getSpreadType(left, eachMember)` (Go's right-union arm).
                    if !members.is_empty() || !all_members.is_empty() {
                        let left = self.materialize_object_literal_type(
                            program,
                            node,
                            &members,
                            &properties,
                            &index_infos,
                        );
                        let spread_results: Vec<TypeId> = union_members
                            .iter()
                            .map(|&right| {
                                self.get_spread_type(program, left, right, in_const_context)
                            })
                            .collect();
                        return self.get_union_type(&spread_results);
                    }
                }
                if members.is_empty() && all_members.is_empty() {
                    spread_acc = Some(match spread_acc {
                        None => self.get_spread_type(
                            program,
                            self.empty_object_type(),
                            spread_type,
                            in_const_context,
                        ),
                        Some(left) => {
                            self.get_spread_type(program, left, spread_type, in_const_context)
                        }
                    });
                } else {
                    self.merge_spread_into_object_literal(
                        program,
                        spread_type,
                        member_check_flags,
                        &mut members,
                        &mut properties,
                        &mut all_members,
                    );
                    spread_acc = None;
                }
                continue;
            }
            let member_kind = program.arena().kind(member_decl);
            if matches!(member_kind, Kind::GetAccessor | Kind::SetAccessor) {
                let name_node = match program.arena().data(member_decl) {
                    NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                        d.name
                    }
                    _ => continue,
                };
                let computed_name_type =
                    if program.arena().kind(name_node) == Kind::ComputedPropertyName {
                        Some(self.check_computed_property_name(program, name_node))
                    } else {
                        None
                    };
                self.check_accessor_declaration(program, member_decl);
                let Some(symbol) = program.symbol_of_node(member_decl) else {
                    continue;
                };
                if let Some(name_type) = computed_name_type {
                    if !self
                        .get_type(name_type)
                        .flags()
                        .intersects(TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE)
                    {
                        let string_number_symbol = self.get_union_type(&[
                            self.string_type,
                            self.number_type,
                            self.es_symbol_type,
                        ]);
                        if self.is_type_assignable_to(program, name_type, string_number_symbol) {
                            if self.is_type_assignable_to(program, name_type, self.number_type) {
                                has_computed_number_property = true;
                            } else if self
                                .is_type_assignable_to(program, name_type, self.es_symbol_type)
                            {
                                has_computed_symbol_property = true;
                            } else {
                                has_computed_string_property = true;
                            }
                        }
                    } else {
                        let name = get_property_name_from_type(self, name_type);
                        members.insert(name.clone(), symbol);
                        properties.push(symbol);
                        all_members.push(ObjectLiteralMember {
                            symbol,
                            computed_name_type: Some(name_type),
                        });
                    }
                    continue;
                }
                let name = program.symbol(symbol).name.clone();
                members.insert(name.clone(), symbol);
                properties.push(symbol);
                all_members.push(ObjectLiteralMember {
                    symbol,
                    computed_name_type: None,
                });
                continue;
            }
            let name_node = match program.arena().data(member_decl) {
                NodeData::PropertyAssignment(d) => d.name,
                NodeData::ShorthandPropertyAssignment(d) => d.name,
                NodeData::MethodDeclaration(d) => d.name,
                _ => continue,
            };
            // A computed property name (`[expr]: v`) types its bracket
            // expression first (Go: `computedNameType =
            // checkComputedPropertyName(memberDecl.Name())`).
            let computed_name_type =
                if program.arena().kind(name_node) == Kind::ComputedPropertyName {
                    Some(self.check_computed_property_name(program, name_node))
                } else {
                    None
                };
            // Go's member loop dispatches on the member kind: a property
            // assignment types its initializer, a shorthand property types the
            // referenced identifier, and a method is checked like a function
            // expression (Go's `checkObjectLiteral` switch).
            let member_type = match member_kind {
                Kind::PropertyAssignment => self.check_property_assignment(program, member_decl),
                Kind::ShorthandPropertyAssignment => {
                    self.check_shorthand_property_assignment(program, member_decl)
                }
                Kind::MethodDeclaration => self.check_object_literal_method(program, member_decl),
                _ => continue,
            };
            // A non-literal computed name assignable to `string | number |
            // symbol` contributes to an index signature of the matching key
            // kind, not a named property (Go's `hasComputed*Property` block).
            if let Some(name_type) = computed_name_type {
                if !self
                    .get_type(name_type)
                    .flags()
                    .intersects(TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE)
                {
                    let string = self.string_type;
                    let number = self.number_type;
                    let es_symbol = self.es_symbol_type;
                    let string_number_symbol = self.get_union_type(&[string, number, es_symbol]);
                    if self.is_type_assignable_to(program, name_type, string_number_symbol) {
                        if self.is_type_assignable_to(program, name_type, number) {
                            has_computed_number_property = true;
                        } else if self.is_type_assignable_to(program, name_type, es_symbol) {
                            has_computed_symbol_property = true;
                        } else {
                            has_computed_string_property = true;
                        }
                        // Go: `newSymbolEx(SymbolFlagsProperty | member.Flags,
                        // member.Name, ...)` with the binder's `__computed` name;
                        // the symbol carries the member value type for the index
                        // signature's value-type union.
                        let prop = self.new_object_literal_property(
                            INTERNAL_SYMBOL_NAME_COMPUTED,
                            SymbolFlags::PROPERTY,
                            member_check_flags,
                            member_type,
                        );
                        all_members.push(ObjectLiteralMember {
                            symbol: prop,
                            computed_name_type: Some(name_type),
                        });
                    }
                    continue;
                }
                // A string/number-literal or unique-symbol computed name becomes a
                // late-bound named member (`isTypeUsableAsPropertyName`).
                let name = get_property_name_from_type(self, name_type);
                let prop = self.new_object_literal_property(
                    &name,
                    SymbolFlags::PROPERTY,
                    member_check_flags | CheckFlags::LATE,
                    member_type,
                );
                members.insert(name.clone(), prop);
                properties.push(prop);
                all_members.push(ObjectLiteralMember {
                    symbol: prop,
                    computed_name_type: Some(name_type),
                });
                continue;
            }
            let Some(name) = property_name_text(program, name_node) else {
                continue;
            };
            // Go: `newSymbolEx(SymbolFlagsProperty | member.Flags, member.Name, checkFlags)`
            // then `links.resolvedType = t`. Object-literal properties are never
            // optional in the reachable subset; methods carry the binder's method
            // flags in addition to `Property`.
            let member_flags = program
                .symbol_of_node(member_decl)
                .map(|s| program.symbol(s).flags)
                .unwrap_or(SymbolFlags::empty());
            let prop = self.new_object_literal_property(
                &name,
                SymbolFlags::PROPERTY | member_flags,
                member_check_flags,
                member_type,
            );
            members.insert(name, prop);
            properties.push(prop);
            all_members.push(ObjectLiteralMember {
                symbol: prop,
                computed_name_type: None,
            });
        }
        // Go's `createObjectLiteralType` synthesizes one index signature per
        // present computed-name kind, unioning the value types of all members
        // whose names match that key kind (`getObjectLiteralIndexInfo`). The
        // index signatures are readonly in a const context (Go's `isReadonly :=
        // c.isConstContext(node)`).
        if has_computed_string_property {
            let string = self.string_type;
            let info =
                self.get_object_literal_index_info(program, &all_members, string, in_const_context);
            index_infos.push(info);
        }
        if has_computed_number_property {
            let number = self.number_type;
            let info =
                self.get_object_literal_index_info(program, &all_members, number, in_const_context);
            index_infos.push(info);
        }
        if has_computed_symbol_property {
            let es_symbol = self.es_symbol_type;
            let info = self.get_object_literal_index_info(
                program,
                &all_members,
                es_symbol,
                in_const_context,
            );
            index_infos.push(info);
        }
        if let Some(spread) = spread_acc {
            if members.is_empty() && all_members.is_empty() {
                return spread;
            }
            // Go folds trailing named members into the accumulated spread at the
            // end of `checkObjectLiteral` (`getSpreadType(spread,
            // createObjectLiteralType())`).
            let trailing = self.materialize_object_literal_type(
                program,
                node,
                &members,
                &properties,
                &index_infos,
            );
            return self.get_spread_type(program, spread, trailing, in_const_context);
        }
        let object = ObjectType {
            members,
            properties,
            index_infos,
            ..Default::default()
        };
        // Go's `createObjectLiteralType` sets `ObjectFlagsFreshLiteral |
        // ObjectFlagsObjectLiteral | ObjectFlagsContainsObjectOrArrayLiteral`
        // on top of the `ObjectFlagsAnonymous` from `newAnonymousType`.
        let symbol = program.symbol_of_node(node);
        self.types.alloc(
            TypeFlags::OBJECT,
            ObjectFlags::ANONYMOUS
                | ObjectFlags::OBJECT_LITERAL
                | ObjectFlags::FRESH_LITERAL
                | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL,
            symbol,
            super::types::TypeData::Object(object),
        )
    }

    // Reports whether `t` may be spread into an object literal (Go's
    // `isValidSpreadType` reachable subset: `any`, `object`, `nonPrimitive`).
    //
    // DEFER(phase-4-checker-4bh+): instantiable non-primitive and the
    // `removeDefinitelyFalsyTypes` normalization. blocked-by: full spread-type merge.
    // Go: internal/checker/checker.go:Checker.isValidSpreadType(13418)
    fn is_valid_spread_type(&mut self, program: &dyn BoundProgram, t: TypeId) -> bool {
        let t = self.normalize_type_for_spread_validation(program, t);
        let flags = self.get_type(t).flags();
        if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
            let members: Vec<TypeId> = self
                .get_type(t)
                .union_types()
                .or_else(|| self.get_type(t).intersection_types())
                .unwrap_or(&[])
                .to_vec();
            return members
                .iter()
                .all(|&m| self.is_valid_spread_type(program, m));
        }
        flags.intersects(
            TypeFlags::ANY
                | TypeFlags::NON_PRIMITIVE
                | TypeFlags::OBJECT
                | TypeFlags::INSTANTIABLE_NON_PRIMITIVE,
        )
    }

    // Normalizes a spread operand the way Go's `isValidSpreadType` does:
    // `removeDefinitelyFalsyTypes(mapType(t, getBaseConstraintOrType))`.
    // Go: internal/checker/checker.go:Checker.isValidSpreadType(13418)
    fn normalize_type_for_spread_validation(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> TypeId {
        let mapped = if self.get_type(t).flags().intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = self
                .distributed_types(t)
                .into_iter()
                .map(|m| self.get_base_constraint_or_type(program, m))
                .collect();
            self.get_union_type(&members)
        } else {
            self.get_base_constraint_or_type(program, t)
        };
        self.remove_definitely_falsy_types(mapped)
    }

    // Merges the own properties of `spread_type` into an object literal under
    // construction; spread properties override same-named properties already
    // present (Go's `getSpreadType` for non-generic object types).
    //
    // DEFER(phase-4-checker-4bh+): private/protected skipping, accessors/methods,
    // index-signature merge, and readonly propagation. blocked-by: full
    // `getSpreadType`.
    // Go: internal/checker/checker.go:Checker.getSpreadType(13301)
    fn merge_spread_into_object_literal(
        &mut self,
        program: &dyn BoundProgram,
        spread_type: TypeId,
        member_check_flags: tsgo_ast::CheckFlags,
        members: &mut SymbolTable,
        properties: &mut Vec<tsgo_ast::SymbolId>,
        all_members: &mut Vec<ObjectLiteralMember>,
    ) {
        use super::declared_types::{get_apparent_type, get_properties_of_type, get_type_of_symbol};
        let apparent = get_apparent_type(self, spread_type);
        let globals = program.globals();
        let mut skipped_private_members = FxHashSet::default();
        for (name, prop_sym) in get_properties_of_type(self, apparent) {
            if is_private_or_protected_member(self, program, prop_sym) {
                skipped_private_members.insert(name.clone());
                continue;
            }
            if !is_spreadable_property(program, prop_sym) {
                continue;
            }
            let prop = self.get_spread_property_symbol(
                program,
                &name,
                prop_sym,
                globals,
                member_check_flags,
            );
            if let Some(existing) = members.get(&name).copied() {
                if self.is_optional_spread_symbol(program, prop_sym) {
                    let left_type = get_type_of_symbol(self, program, existing, globals);
                    let right_type = get_type_of_symbol(self, program, prop, globals);
                    let merged = self.merge_optional_spread_property_types(left_type, right_type);
                    let flags = SymbolFlags::PROPERTY
                        | (self.spread_source_symbol_flags(program, existing)
                            & SymbolFlags::OPTIONAL);
                    let merged_prop = self.new_object_literal_property(
                        &name,
                        flags,
                        member_check_flags,
                        merged,
                    );
                    if let Some(pos) = properties.iter().position(|&s| {
                        self.spread_property_name(program, s) == name
                    }) {
                        properties[pos] = merged_prop;
                    }
                    if let Some(pos) = all_members.iter().position(|m| {
                        self.spread_property_name(program, m.symbol) == name
                    }) {
                        all_members[pos].symbol = merged_prop;
                    }
                    members.insert(name, merged_prop);
                    continue;
                }
            }
            if let Some(pos) = properties.iter().position(|&s| {
                self.spread_property_name(program, s) == name
            }) {
                properties[pos] = prop;
            } else {
                properties.push(prop);
            }
            members.insert(name.clone(), prop);
            if !all_members.iter().any(|m| self.spread_property_name(program, m.symbol) == name)
            {
                all_members.push(ObjectLiteralMember {
                    symbol: prop,
                    computed_name_type: None,
                });
            }
        }
        for skipped in skipped_private_members {
            members.remove(&skipped);
            properties.retain(|&s| self.spread_property_name(program, s) != skipped);
            all_members
                .retain(|m| self.spread_property_name(program, m.symbol) != skipped);
        }
    }

    // Materializes the object-literal type accumulated so far (Go's
    // `createObjectLiteralType` on a partial member table).
    // Go: internal/checker/checker.go:Checker.createObjectLiteralType
    fn materialize_object_literal_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        members: &SymbolTable,
        properties: &[tsgo_ast::SymbolId],
        index_infos: &[IndexInfoId],
    ) -> TypeId {
        let object = ObjectType {
            members: members.clone(),
            properties: properties.to_vec(),
            index_infos: index_infos.to_vec(),
            ..Default::default()
        };
        let symbol = program.symbol_of_node(node);
        self.types.alloc(
            TypeFlags::OBJECT,
            ObjectFlags::ANONYMOUS
                | ObjectFlags::OBJECT_LITERAL
                | ObjectFlags::FRESH_LITERAL
                | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL,
            symbol,
            super::types::TypeData::Object(object),
        )
    }

    // Folds two object types for spread (`{...right}` into `left`); spread
    // properties override same-named properties on the left (Go's `getSpreadType`
    // reachable subset: union distribution + non-generic object merge).
    // Go: internal/checker/checker.go:Checker.getSpreadType(13301)
    fn get_spread_type(
        &mut self,
        program: &dyn BoundProgram,
        left: TypeId,
        right: TypeId,
        readonly: bool,
    ) -> TypeId {
        let left_flags = self.get_type(left).flags();
        let right_flags = self.get_type(right).flags();
        if left_flags.intersects(TypeFlags::ANY) || right_flags.intersects(TypeFlags::ANY) {
            return self.any_type;
        }
        if left_flags.intersects(TypeFlags::UNKNOWN) || right_flags.intersects(TypeFlags::UNKNOWN) {
            return self.unknown_type;
        }
        if left_flags.intersects(TypeFlags::NEVER) {
            return right;
        }
        if right_flags.intersects(TypeFlags::NEVER) {
            return left;
        }
        let left = self.try_merge_union_of_object_type_and_empty_object(program, left, readonly);
        if let Some(members) = self.get_type(left).union_types().map(|m| m.to_vec()) {
            let mapped: Vec<TypeId> = members
                .iter()
                .map(|&member| self.get_spread_type(program, member, right, readonly))
                .collect();
            return self.get_union_type(&mapped);
        }
        let right = self.try_merge_union_of_object_type_and_empty_object(program, right, readonly);
        let right_flags = self.get_type(right).flags();
        if let Some(members) = self.get_type(right).union_types().map(|m| m.to_vec()) {
            let mapped: Vec<TypeId> = members
                .iter()
                .map(|&member| self.get_spread_type(program, left, member, readonly))
                .collect();
            return self.get_union_type(&mapped);
        }
        if right_flags.intersects(
            TypeFlags::BOOLEAN_LIKE
                | TypeFlags::NUMBER_LIKE
                | TypeFlags::BIG_INT_LIKE
                | TypeFlags::STRING_LIKE
                | TypeFlags::ENUM_LIKE
                | TypeFlags::NON_PRIMITIVE
                | TypeFlags::INDEX,
        ) {
            return left;
        }
        if is_generic_object_type(self, left) || is_generic_object_type(self, right) {
            if self.is_empty_object_type(left) {
                return right;
            }
            if self.get_type(left).flags().intersects(TypeFlags::INTERSECTION) {
                if let Some(mut types) = self.get_type(left).intersection_types().map(|m| m.to_vec())
                {
                    if let Some(last_left) = types.last().copied() {
                        if self.is_non_generic_object_type(last_left)
                            && self.is_non_generic_object_type(right)
                        {
                            let last = types.len() - 1;
                            types[last] = self.get_spread_type(program, types[last], right, readonly);
                            return self.get_intersection_type(&types);
                        }
                    }
                }
            }
            return self.get_intersection_type(&[left, right]);
        }
        self.merge_object_types_for_spread(program, left, right, readonly)
    }

    // Reports whether `t` is a concrete (non-generic) object type (Go's
    // `isNonGenericObjectType`).
    // Go: internal/checker/checker.go:Checker.isNonGenericObjectType(13440)
    fn is_non_generic_object_type(&mut self, t: TypeId) -> bool {
        self.get_type(t).flags().intersects(TypeFlags::OBJECT)
            && !is_generic_object_type(self, t)
    }

    // When a union is exactly one object type plus members that spread into an
    // empty object (`{}`, `null`, primitives, etc.), rewrites the object arm so
    // every property is optional (Go's `tryMergeUnionOfObjectTypeAndEmptyObject`).
    // Go: internal/checker/checker.go:Checker.tryMergeUnionOfObjectTypeAndEmptyObject(13444)
    fn try_merge_union_of_object_type_and_empty_object(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        readonly: bool,
    ) -> TypeId {
        if !self.get_type(t).flags().intersects(TypeFlags::UNION) {
            return t;
        }
        let members = self.get_type(t).union_types().unwrap().to_vec();
        if members
            .iter()
            .all(|&m| self.is_empty_object_type_or_spreads_into_empty_object(m))
        {
            if let Some(&empty) = members
                .iter()
                .find(|&&m| self.is_empty_object_type(m))
            {
                return empty;
            }
            return self.empty_object_type();
        }
        let first_type = members.iter().find(|&&m| {
            !self.is_empty_object_type_or_spreads_into_empty_object(m)
        });
        let Some(&first_type) = first_type else {
            return t;
        };
        if members.iter().any(|&m| {
            m != first_type && !self.is_empty_object_type_or_spreads_into_empty_object(m)
        }) {
            return t;
        }
        self.make_all_properties_optional_for_spread(program, first_type, readonly)
    }

    // Reports whether `t` is an empty object or a primitive/nullish type that
    // contributes no properties when spread (Go's
    // `isEmptyObjectTypeOrSpreadsIntoEmptyObject`).
    // Go: internal/checker/checker.go:Checker.isEmptyObjectTypeOrSpreadsIntoEmptyObject(13518)
    fn is_empty_object_type_or_spreads_into_empty_object(&mut self, t: TypeId) -> bool {
        if self.is_empty_object_type(t) {
            return true;
        }
        self.get_type(t).flags().intersects(
            TypeFlags::NULL
                | TypeFlags::UNDEFINED
                | TypeFlags::BOOLEAN_LIKE
                | TypeFlags::NUMBER_LIKE
                | TypeFlags::BIG_INT_LIKE
                | TypeFlags::STRING_LIKE
                | TypeFlags::ENUM_LIKE
                | TypeFlags::NON_PRIMITIVE
                | TypeFlags::INDEX,
        )
    }

    // Materializes `t` with every spreadable property made optional, matching
    // the union-with-empty-object rewrite in Go's `tryMergeUnionOfObjectTypeAndEmptyObject`.
    fn make_all_properties_optional_for_spread(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        readonly: bool,
    ) -> TypeId {
        use super::declared_types::get_properties_of_type;
        let globals = program.globals();
        let member_check_flags = if readonly {
            tsgo_ast::CheckFlags::READONLY
        } else {
            tsgo_ast::CheckFlags::empty()
        };
        let mut members = SymbolTable::default();
        let mut properties: Vec<tsgo_ast::SymbolId> = Vec::new();
        for (name, prop_sym) in get_properties_of_type(self, t) {
            if is_private_or_protected_member(self, program, prop_sym) {
                continue;
            }
            if !is_spreadable_property(program, prop_sym) {
                continue;
            }
            let prop_type = self.get_spread_property_type(program, prop_sym, globals);
            let optional_type = self.add_optionality_ex(prop_type, true, true);
            let prop = self.new_object_literal_property(
                &name,
                SymbolFlags::PROPERTY | SymbolFlags::OPTIONAL,
                member_check_flags,
                optional_type,
            );
            members.insert(name, prop);
            properties.push(prop);
        }
        let index_infos = get_index_infos_of_type(self, t);
        let object = ObjectType {
            members,
            properties,
            index_infos,
            ..Default::default()
        };
        self.new_object_type(
            ObjectFlags::ANONYMOUS | ObjectFlags::OBJECT_LITERAL | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL,
            self.get_type(t).symbol,
            object,
        )
    }

    // Merges the own properties of `right` into `left` for spread; right wins on
    // name clashes unless the right property is optional (union merge) or
    // private/protected (skip). Go's `getSpreadType` non-generic object arm.
    fn merge_object_types_for_spread(
        &mut self,
        program: &dyn BoundProgram,
        left: TypeId,
        right: TypeId,
        readonly: bool,
    ) -> TypeId {
        use super::declared_types::{get_properties_of_type, get_type_of_symbol};
        let globals = program.globals();
        let spread_check_flags = if readonly {
            tsgo_ast::CheckFlags::READONLY
        } else {
            tsgo_ast::CheckFlags::empty()
        };
        let mut members = SymbolTable::default();
        let mut properties: Vec<tsgo_ast::SymbolId> = Vec::new();
        let mut skipped_private_members = FxHashSet::default();
        let raw_index_infos = if left == self.empty_object_type() {
            get_index_infos_of_type(self, right)
        } else {
            self.get_union_index_infos(&[left, right])
        };
        let index_infos = self.apply_readonly_to_index_infos(raw_index_infos, readonly);
        for (name, prop_sym) in get_properties_of_type(self, right) {
            if is_private_or_protected_member(self, program, prop_sym) {
                skipped_private_members.insert(name);
                continue;
            }
            if !is_spreadable_property(program, prop_sym) {
                continue;
            }
            let prop = self.get_spread_property_symbol(
                program,
                &name,
                prop_sym,
                globals,
                spread_check_flags,
            );
            members.insert(name.clone(), prop);
        }
        for (name, prop_sym) in get_properties_of_type(self, left) {
            if skipped_private_members.contains(&name) || !is_spreadable_property(program, prop_sym)
            {
                continue;
            }
            if let Some(&right_prop) = members.get(&name) {
                if self.is_optional_spread_symbol(program, right_prop) {
                    let left_type = get_type_of_symbol(self, program, prop_sym, globals);
                    let right_type = get_type_of_symbol(self, program, right_prop, globals);
                    let merged = self.merge_optional_spread_property_types(left_type, right_type);
                    let flags = SymbolFlags::PROPERTY
                        | (self.spread_source_symbol_flags(program, prop_sym) & SymbolFlags::OPTIONAL);
                    let prop = self.new_object_literal_property(
                        &name,
                        flags,
                        spread_check_flags,
                        merged,
                    );
                    members.insert(name, prop);
                }
                continue;
            }
            let prop = self.get_spread_property_symbol(
                program,
                &name,
                prop_sym,
                globals,
                spread_check_flags,
            );
            members.insert(name.clone(), prop);
        }
        let mut seen = FxHashSet::default();
        for (name, prop_sym) in get_properties_of_type(self, left) {
            if skipped_private_members.contains(&name) || !is_spreadable_property(program, prop_sym) {
                continue;
            }
            if let Some(&prop) = members.get(&name) {
                properties.push(prop);
                seen.insert(name);
            }
        }
        for (name, prop_sym) in get_properties_of_type(self, right) {
            if skipped_private_members.contains(&name) || !is_spreadable_property(program, prop_sym) {
                continue;
            }
            if seen.contains(&name) {
                continue;
            }
            if let Some(&prop) = members.get(&name) {
                properties.push(prop);
            }
        }
        let object = ObjectType {
            members,
            properties,
            index_infos,
            ..Default::default()
        };
        // Go's `getSpreadType` non-generic arm sets `ObjectFlagsObjectLiteral |
        // ObjectFlagsContainsObjectOrArrayLiteral` on the merged spread type.
        self.new_object_type(
            ObjectFlags::ANONYMOUS
                | ObjectFlags::OBJECT_LITERAL
                | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL,
            None,
            object,
        )
    }

    fn get_spread_property_symbol(
        &mut self,
        program: &dyn BoundProgram,
        name: &str,
        prop_sym: SymbolId,
        globals: Option<&SymbolTable>,
        check_flags: tsgo_ast::CheckFlags,
    ) -> SymbolId {
        let prop_type = self.get_spread_property_type(program, prop_sym, globals);
        let flags = SymbolFlags::PROPERTY
            | (self.spread_source_symbol_flags(program, prop_sym) & SymbolFlags::OPTIONAL);
        self.new_object_literal_property(name, flags, check_flags, prop_type)
    }

    // Resolves the type carried by a spread property symbol (Go's
    // `getSpreadSymbol` set-only accessor arm returns `undefined`).
    // Go: internal/checker/checker.go:Checker.getSpreadSymbol(13499)
    fn get_spread_property_type(
        &mut self,
        program: &dyn BoundProgram,
        prop_sym: SymbolId,
        globals: Option<&SymbolTable>,
    ) -> TypeId {
        if is_setonly_accessor_symbol(self, program, prop_sym) {
            return self.undefined_type();
        }
        get_type_of_symbol(self, program, prop_sym, globals)
    }

    fn spread_source_symbol_flags(
        &self,
        program: &dyn BoundProgram,
        symbol: SymbolId,
    ) -> SymbolFlags {
        if super::is_synthesized_symbol(symbol) {
            self.synthesized_symbol_flags(symbol)
        } else {
            program.symbol(symbol).flags
        }
    }

    fn spread_property_name(&self, program: &dyn BoundProgram, symbol: SymbolId) -> String {
        if super::is_synthesized_symbol(symbol) {
            self.synthesized_symbol_name(symbol).to_string()
        } else {
            program.symbol(symbol).name.clone()
        }
    }

    fn is_optional_spread_symbol(&self, program: &dyn BoundProgram, symbol: SymbolId) -> bool {
        self.spread_source_symbol_flags(program, symbol)
            .contains(SymbolFlags::OPTIONAL)
    }

    fn merge_optional_spread_property_types(
        &mut self,
        left_type: TypeId,
        right_type: TypeId,
    ) -> TypeId {
        let left_without_undefined =
            self.get_type_with_facts(left_type, TypeFacts::NE_UNDEFINED);
        let right_without_undefined =
            self.get_type_with_facts(right_type, TypeFacts::NE_UNDEFINED);
        if left_without_undefined == right_without_undefined {
            left_type
        } else {
            self.get_union_type(&[left_type, right_without_undefined])
        }
    }

    // Applies the spread `readonly` flag to each index signature (Go's
    // `getIndexInfoWithReadonly`).
    // Go: internal/checker/checker.go:Checker.getIndexInfoWithReadonly(13411)
    fn apply_readonly_to_index_infos(
        &mut self,
        infos: Vec<IndexInfoId>,
        readonly: bool,
    ) -> Vec<IndexInfoId> {
        if !readonly {
            return infos;
        }
        infos
            .into_iter()
            .map(|id| {
                let info = self.index_info(id);
                if info.is_readonly {
                    id
                } else {
                    self.new_index_info(IndexInfo::new(
                        info.key_type,
                        info.value_type,
                        true,
                    ))
                }
            })
            .collect()
    }

    // Unions the index signatures shared by every type in `types` (Go's
    // `getUnionIndexInfos`).
    // Go: internal/checker/checker.go:Checker.getUnionIndexInfos(13424)
    fn get_union_index_infos(&mut self, types: &[TypeId]) -> Vec<IndexInfoId> {
        if types.is_empty() {
            return Vec::new();
        }
        let source_infos = get_index_infos_of_type(self, types[0]);
        let mut result = Vec::new();
        for id in source_infos {
            let index_type = self.index_info(id).key_type;
            if types
                .iter()
                .all(|&t| get_index_info_of_type(self, t, index_type).is_some())
            {
                let value_types: Vec<TypeId> = types
                    .iter()
                    .map(|&t| get_index_type_of_type(self, t, index_type).unwrap())
                    .collect();
                let value_type = self.get_union_type(&value_types);
                let is_readonly = types.iter().any(|&t| {
                    get_index_info_of_type(self, t, index_type)
                        .map(|info_id| self.index_info(info_id).is_readonly)
                        .unwrap_or(false)
                });
                result.push(self.new_index_info(IndexInfo::new(
                    index_type,
                    value_type,
                    is_readonly,
                )));
            }
        }
        result
    }

    // Builds the index signature of kind `key_type` (`string`/`number`/`symbol`)
    // for an object literal, unioning the value types of every member whose name
    // matches that key kind (Go's `getObjectLiteralIndexInfo`): a string index
    // unions all non-symbol-named members; a number index unions numeric-named
    // members; a symbol index unions symbol-named members. An empty union is
    // `undefined`.
    //
    // The reachable subset reads each member's computed-name kind from the
    // `ObjectLiteralMember` record (the port's synthesized symbols carry no
    // declarations, unlike Go's `prop.Declarations[0].Name()`), and uses
    // `getUnionType` (Go's `UnionReductionSubtype` is observably equivalent for
    // the widened primitive value types built here).
    //
    // The synthesized index signature is `readonly` when `is_readonly` is set
    // (an `as const` object literal, Go's `isReadonly := c.isConstContext(node)`).
    //
    // DEFER(phase-4-checker-4bh+): the `components` slice (conflicting
    // computed-name declarations) and known-symbol membership. blocked-by:
    // declaration-carrying synthesized symbols + well-known symbols.
    // Go: internal/checker/checker.go:Checker.getObjectLiteralIndexInfo(19576)
    fn get_object_literal_index_info(
        &mut self,
        program: &dyn BoundProgram,
        members: &[ObjectLiteralMember],
        key_type: TypeId,
        is_readonly: bool,
    ) -> IndexInfoId {
        let string = self.string_type;
        let number = self.number_type;
        let mut prop_types: Vec<TypeId> = Vec::new();
        for member in members {
            let matches = if key_type == string {
                !self.is_object_literal_member_with_symbol_name(member)
            } else if key_type == number {
                self.is_object_literal_member_with_numeric_name(program, member)
            } else {
                self.is_object_literal_member_with_symbol_name(member)
            };
            if matches {
                let globals = program.globals();
                prop_types.push(get_type_of_symbol(self, program, member.symbol, globals));
            }
        }
        let value_type = if prop_types.is_empty() {
            self.undefined_type()
        } else {
            self.get_union_type(&prop_types)
        };
        self.new_index_info(IndexInfo::new(key_type, value_type, is_readonly))
    }

    // Reports whether an object-literal member's name is symbol-typed (Go's
    // `isSymbolWithSymbolName` reachable subset): a computed name whose
    // expression is assignable to the ES-symbol kind. A statically-named member
    // is never symbol-named here.
    // DEFER(phase-4-checker-4bh+): `IsKnownSymbol` (well-known-symbol props).
    // blocked-by: well-known symbols (P6).
    // Go: internal/checker/checker.go:Checker.isSymbolWithSymbolName(19596)
    fn is_object_literal_member_with_symbol_name(&self, member: &ObjectLiteralMember) -> bool {
        match member.computed_name_type {
            Some(t) => self
                .get_type(t)
                .flags()
                .intersects(TypeFlags::ES_SYMBOL_LIKE),
            None => false,
        }
    }

    // Reports whether an object-literal member's name is numeric (Go's
    // `isSymbolWithNumericName`): a statically-named member with a numeric-literal
    // name, or a computed name whose expression is assignable to the number kind
    // (Go's `isNumericComputedName`).
    // Go: internal/checker/checker.go:Checker.isSymbolWithNumericName(19607)
    fn is_object_literal_member_with_numeric_name(
        &self,
        program: &dyn BoundProgram,
        member: &ObjectLiteralMember,
    ) -> bool {
        match member.computed_name_type {
            Some(t) => self.get_type(t).flags().intersects(TypeFlags::NUMBER_LIKE),
            None => is_numeric_literal_name(&self.property_symbol_name(program, member.symbol)),
        }
    }

    // Types a computed property name `[expr]` (Go's `checkComputedPropertyName`):
    // its bracket expression is type-checked and that type is returned (used by
    // `checkObjectLiteral` to decide whether the member is late-bound named or
    // contributes to an index signature). When the expression's type is neither a
    // `string`/`number`/`symbol`-like type nor assignable to `string | number |
    // symbol` (or is nullable), `2464` is reported.
    //
    // DEFER(phase-4-checker-4bh+): the `n in obj`-name special case for
    // type-literal/class/interface parents, and the `typeNodeLinks` caching (the
    // port has no expression-type cache; the spread pre-pass that would re-check
    // computed names is deferred, so each name is checked once). blocked-by:
    // in-operator computed names + expression-type memoization.
    // Go: internal/checker/checker.go:Checker.checkComputedPropertyName(26619)
    pub(crate) fn check_computed_property_name(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        self.check_grammar_computed_property_name(program, node);
        let expression = match program.arena().data(node) {
            NodeData::ComputedPropertyName(d) => d.expression,
            _ => return self.error_type,
        };
        let t = self.check_expression(program, expression);
        let flags = self.get_type(t).flags();
        // Go: `isTypeAssignableToKind(t, StringLike|NumberLike|ESSymbolLike)`. The
        // `any`/error type is permitted (it behaves as `any`).
        let kind_ok = flags.intersects(
            TypeFlags::STRING_LIKE
                | TypeFlags::NUMBER_LIKE
                | TypeFlags::ES_SYMBOL_LIKE
                | TypeFlags::ANY,
        );
        let usable_as_index_key = kind_ok || {
            let string = self.string_type;
            let number = self.number_type;
            let es_symbol = self.es_symbol_type;
            let string_number_symbol = self.get_union_type(&[string, number, es_symbol]);
            self.is_type_assignable_to(program, t, string_number_symbol)
        };
        if flags.intersects(TypeFlags::NULLABLE) || !usable_as_index_key {
            self.error(
                program,
                node,
                &tsgo_diagnostics::A_COMPUTED_PROPERTY_NAME_MUST_BE_OF_TYPE_STRING_NUMBER_SYMBOL_OR_ANY,
                &[],
            );
        }
        t
    }

    // Types an object-literal `name: value` member (Go's
    // `checkPropertyAssignment`): the member's type is its initializer typed
    // through `checkExpressionForMutableLocation` (so a fresh literal widens to
    // its primitive in this non-const, non-contextual position).
    //
    // DEFER(phase-4-checker-4bg+): the (grammar-error) explicit type annotation
    // on a property assignment (`{ a: number }` as a value), whose Go path runs
    // `checkTypeAssignableToAndOptionallyElaborate`, and computed property
    // names. blocked-by: assignment elaboration + computed-name typing.
    // Go: internal/checker/checker.go:Checker.checkPropertyAssignment(13587)
    fn check_property_assignment(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let initializer = match program.arena().data(node) {
            NodeData::PropertyAssignment(d) => d.initializer,
            _ => return self.error_type,
        };
        let Some(initializer) = initializer else {
            return self.error_type;
        };
        self.check_expression_for_mutable_location(program, initializer)
    }

    // Types an object-literal shorthand property `{ a }` (Go's
    // `checkShorthandPropertyAssignment`): `{ a }` is equivalent to `{ a: a }`,
    // so the member's type is the type of the referenced identifier `a`, typed
    // through `checkExpressionForMutableLocation` (a fresh literal widens to its
    // primitive, exactly as a normal property value would).
    //
    // Outside a destructuring pattern, Go uses the cover-initialized-name
    // expression (`{ a = 1 }`'s `ObjectAssignmentInitializer`) when present and
    // otherwise the name identifier; we mirror that reachable path.
    //
    // DEFER(phase-4-checker-4bh+): the destructuring-assignment-pattern path
    // (`inDestructuringPattern`), where `{ a = 1 }` makes the property optional
    // and the default value is checked, and the (grammar-error) explicit type
    // annotation on a shorthand (`{ a }: T`), whose Go path runs
    // `checkTypeAssignableToAndOptionallyElaborate` and returns the annotated
    // type. blocked-by: destructuring-assignment typing + shorthand annotation
    // elaboration.
    // Go: internal/checker/checker.go:Checker.checkShorthandPropertyAssignment(13603)
    fn check_shorthand_property_assignment(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        let (name, initializer) = match program.arena().data(node) {
            NodeData::ShorthandPropertyAssignment(d) => (d.name, d.object_assignment_initializer),
            _ => return self.error_type,
        };
        let expr = initializer.unwrap_or(name);
        self.check_expression_for_mutable_location(program, expr)
    }

    // Types an expression occupying a mutable location (an object-literal
    // property value or an array-literal element): its fresh literal type is
    // widened to the base primitive (Go's `checkExpressionForMutableLocation`,
    // whose default branch is `getWidenedLiteralLikeTypeForContextualType` with
    // no contextual type, i.e. `getWidenedLiteralType`).
    //
    // In a const context (an `as const` member/element, recursively) the literal
    // is kept via `getRegularTypeOfLiteralType` (freshness stripped, value
    // preserved) instead of being widened, so `{ a: 1 } as const`'s `a` stays the
    // literal `1`.
    //
    // Otherwise (4bk) the literal is widened *unless* its contextual type makes
    // the position a literal context, in which case it is preserved (Go's
    // default branch -> `getWidenedLiteralLikeTypeForContextualType(t,
    // getContextualType(node))`). This is the inverse-direction flow: an
    // annotation's property/element type flows into the literal so that, e.g.,
    // `{ a: "x" }` typed by `{ a: "x" }` keeps `a` at `"x"` rather than widening
    // to `string`. With no contextual type the call degrades to the prior plain
    // `getWidenedLiteralType` behavior.
    //
    // DEFER(phase-4-checker-4bl+): the `isTypeAssertion(node)` branch (a non-const
    // `x as T` member/element value returns the asserted type unchanged) and
    // `instantiateContextualType` (inference-context instantiation of the
    // contextual type). blocked-by: assertion-value passthrough + inference
    // contexts.
    // Go: internal/checker/checker.go:Checker.checkExpressionForMutableLocation(13784)
    fn check_expression_for_mutable_location(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        let t = self.check_expression(program, node);
        if is_const_context(program, node) {
            return self.regular_type_of_literal_type(t);
        }
        let contextual_type = self.get_contextual_type(program, node, ContextFlags::NONE);
        self.get_widened_literal_like_type_for_contextual_type(t, contextual_type)
    }

    // Runs the fresh-object-literal excess-property check before assignability
    // is reported, mirroring Go's `recursiveTypeRelatedToWorker`: excess-property
    // checking is performed only when the source is a fresh object literal (and
    // not in an `IntersectionStateTarget` context, which is unreachable here).
    // When `has_excess_properties` reports `2353`, the caller suppresses the
    // `2322` head message — Go's `reportRelationError` returns early when the
    // chain head is an excess-property message.
    //
    // `literal_node` is the object-literal initializer whose member name nodes
    // locate the precise error span (Go uses each property's `ValueDeclaration`).
    //
    // Returns `true` when an excess-property error was reported.
    // Go: internal/checker/relater.go:Relater.recursiveTypeRelatedToWorker (isPerformingExcessPropertyChecks, 2647)
    fn check_object_literal_excess_properties(
        &mut self,
        program: &dyn BoundProgram,
        literal_node: NodeId,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        // isPerformingExcessPropertyChecks := isObjectLiteralType(source) &&
        //   source.objectFlags&ObjectFlagsFreshLiteral != 0
        if !self.is_object_literal_type(source)
            || !self
                .get_type(source)
                .object_flags()
                .contains(ObjectFlags::FRESH_LITERAL)
        {
            return false;
        }
        self.has_excess_properties(program, literal_node, source, target)
    }

    // Reports the first source property absent from `target` as `2353`, returning
    // whether such a property was found (Go's `hasExcessProperties`). Iterates the
    // literal's own properties in declaration order; the error is reported on the
    // property's name node within `literal_node`.
    //
    // DEFER(phase-4-checker-4bg+): the JS-literal index-signature simulation, the
    // `globalObjectType` subset suppression (lib globals, P6), the union
    // `reducedTarget`/`checkTypes` reduction and its `Types_of_property_0_are_incompatible`
    // arm, the JSX-attribute message variant, and the `Did you mean to write`
    // suggestion variant (`2561`).
    // blocked-by: JS literals, lib globals, union discriminant reduction, JSX
    // typing.
    // Go: internal/checker/relater.go:Relater.hasExcessProperties(2695)
    fn has_excess_properties(
        &mut self,
        program: &dyn BoundProgram,
        literal_node: NodeId,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        if !self.is_excess_property_check_target(target) {
            return false;
        }
        // The assignable relation suppresses excess checks against an empty
        // object target (any property is accepted). Go also suppresses when the
        // target is a superset of the global `Object` type (DEFER: lib globals,
        // P6).
        if self.is_empty_object_type(target) {
            return false;
        }
        // A union target is reduced to its discriminant-matched constituent
        // (Go's `reducedTarget = findMatchingDiscriminantType(...)`, else
        // `filterPrimitivesIfContainsNonPrimitive`), so excess is checked — and
        // the error reported — against the SELECTED constituent. For
        // `{ kind: "b", subkind: 1 }` against `… | { kind: "b" }` this reports
        // `subkind` against `{ kind: "b" }` (matching tsc) instead of treating
        // `subkind` as known because some OTHER constituent declares it.
        let reduced_target = if self.get_type(target).flags().contains(TypeFlags::UNION) {
            self.find_matching_discriminant_type(program, source, target, RelationKind::Assignable)
                .unwrap_or_else(|| self.filter_primitives_if_contains_non_primitive(target))
        } else {
            target
        };
        // Iterate the literal's own properties in declaration order (Go's
        // `getPropertiesOfType(source)`); every object-literal member is declared
        // directly in the literal, so Go's `shouldCheckAsExcessProperty` holds.
        let properties = match self.get_type(source).as_object() {
            Some(obj) => obj.properties.clone(),
            None => return false,
        };
        for prop in properties {
            let name = self.property_symbol_name(program, prop);
            if !self.is_known_property(program, reduced_target, &name) {
                let error_node = object_literal_property_name_node(program, literal_node, &name)
                    .unwrap_or(literal_node);
                // Report in terms of the object types we actually check (Go's
                // `errorTarget = filterType(reducedTarget, isExcessPropertyCheckTarget)`).
                let error_target = self.filter_excess_property_check_target(reduced_target);
                let target_str = super::nodebuilder::type_to_string(self, program, error_target);
                // Go's `c.error(errorNode, …)` uses `GetErrorRangeForNode` =
                // `skipTrivia(pos)..end`, so the span starts at the property name,
                // not the leading whitespace before it.
                if let Some(suggested) = self.get_suggestion_for_nonexistent_property_name(
                    program,
                    &name,
                    error_target,
                ) {
                    self.error_skipping_leading_trivia(
                        program,
                        error_node,
                        &tsgo_diagnostics::OBJECT_LITERAL_MAY_ONLY_SPECIFY_KNOWN_PROPERTIES_BUT_0_DOES_NOT_EXIST_IN_TYPE_1_DID_YOU_MEAN_TO_WRITE_2,
                        &[name.as_str(), target_str.as_str(), suggested.as_str()],
                    );
                } else {
                    self.error_skipping_leading_trivia(
                        program,
                        error_node,
                        &tsgo_diagnostics::OBJECT_LITERAL_MAY_ONLY_SPECIFY_KNOWN_PROPERTIES_AND_0_DOES_NOT_EXIST_IN_TYPE_1,
                        &[name.as_str(), target_str.as_str()],
                    );
                }
                return true;
            }
        }
        false
    }

    // Returns a property symbol's name, routing checker-synthesized (transient)
    // object-literal property symbols to the transient arena and program symbols
    // to the bound program (which would panic on a tagged id).
    fn property_symbol_name(
        &self,
        program: &dyn BoundProgram,
        symbol: tsgo_ast::SymbolId,
    ) -> String {
        if super::is_synthesized_symbol(symbol) {
            self.synthesized_symbol_name(symbol)
        } else {
            program.symbol(symbol).name.clone()
        }
    }

    // Returns a close-spelling property name on `containing_type`, or `None`
    // when no candidate is near enough (Go's `getSuggestionForNonexistentProperty`).
    // Go: internal/checker/checker.go:Checker.getSuggestionForNonexistentProperty(27041)
    fn get_suggestion_for_nonexistent_property_name(
        &mut self,
        program: &dyn BoundProgram,
        prop_name: &str,
        containing_type: TypeId,
    ) -> Option<String> {
        let apparent = get_apparent_type(self, containing_type);
        let candidates: Vec<SymbolId> = get_properties_of_type(self, apparent)
            .into_iter()
            .map(|(_, sym)| sym)
            .collect();
        super::name_resolution::get_spelling_suggestion_for_name(
            program,
            prop_name,
            &candidates,
            SymbolFlags::VALUE,
        )
        .map(|sym| program.symbol(sym).name.clone())
    }

    // Widens an inferred declaration type (the reachable subset of Go's
    // `getWidenedType`): a fresh object-literal type is rebuilt as a regular
    // anonymous object type, dropping the `FreshLiteral` / `ObjectLiteral` /
    // `ContainsObjectOrArrayLiteral` flags. This is the freshness-stripping step
    // that makes an object literal assigned to a variable stop participating in
    // excess-property checking when read back through the variable. Types that do
    // not require widening pass through unchanged.
    //
    // DEFER(phase-4-checker-later): the `any`/nullable arm, union / intersection /
    // array widening, the widening context (sibling and `undefined`-padded
    // properties), and recursive per-property widening of nested object literals.
    // blocked-by: union/array widening, widening contexts, and nested-literal
    // member re-widening.
    // Go: internal/checker/checker.go:Checker.getWidenedType(18214) / getWidenedTypeWithContext(18218)
    pub(crate) fn get_widened_type(&mut self, t: TypeId) -> TypeId {
        if !self
            .get_type(t)
            .object_flags()
            .contains(ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL)
        {
            return t;
        }
        if self.is_object_literal_type(t) {
            return self.get_widened_type_of_object_literal(t);
        }
        t
    }

    // Rebuilds a fresh object-literal type as a regular anonymous object type
    // (Go's `getWidenedTypeOfObjectLiteral`). The literal's member symbols are
    // already widened in the reachable subset, so they are reused directly; the
    // result keeps no object-literal flags (Go retains only `JSLiteral` /
    // `NonInferrableType`, neither of which is modeled).
    //
    // DEFER(phase-4-checker-later): per-property `getWidenedType` recursion (for
    // nested object/array literals) and the widening-context `undefined` padding.
    // blocked-by: nested-literal recursion + widening contexts.
    // Go: internal/checker/checker.go:Checker.getWidenedTypeOfObjectLiteral(18259)
    fn get_widened_type_of_object_literal(&mut self, t: TypeId) -> TypeId {
        let Some(obj) = self.get_type(t).as_object() else {
            return t;
        };
        let widened = ObjectType {
            members: obj.members.clone(),
            properties: obj.properties.clone(),
            index_infos: obj.index_infos.clone(),
            ..Default::default()
        };
        let symbol = self.get_type(t).symbol;
        self.new_object_type(ObjectFlags::ANONYMOUS, symbol, widened)
    }

    // Types an array literal `[1, 2]` as the global `Array<T>` reference whose
    // element type `T` is the widened union of the element expression types
    // (Go's `checkArrayLiteral` non-tuple, non-destructuring, non-const path ->
    // `createArrayLiteralType(createArrayType(elementType))`). An empty literal
    // takes `never` under strictNullChecks, else `undefined` (Go's
    // `implicitNeverType` / `undefinedWideningType`).
    //
    // In a const context (`[1, 2] as const`) the literal is instead a readonly
    // fixed-arity tuple whose element types are the preserved literals (Go's
    // `inConstContext` -> `createTupleTypeEx(elementTypes, _, readonly=true)`).
    //
    // DEFER(phase-4-checker-4bi+): omitted elements, the non-const tuple
    // contexts (`forceTuple` / a tuple-like contextual type),
    // contextual typing, and the `ObjectFlagsArrayLiteral` clone of
    // `createArrayLiteralType` (the reachable subset returns the plain `Array<T>`
    // reference / fixed-arity tuple, which is sufficient for element access +
    // assignability + printing). blocked-by: spread/iterator typing, `forceTuple`
    // check mode, contextual type propagation, and the array-literal widening
    // flag's consumers.
    // Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989)
    fn check_array_literal(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let elements = match program.arena().data(node) {
            NodeData::ArrayLiteralExpression(d) => d.list.nodes.clone(),
            _ => return self.error_type,
        };
        // DEFER(phase-4-checker-4bi+): omitted elements, destructuring-pattern
        // spread-as-rest, and array-like spread (`isArrayLikeType`) fast paths.
        // blocked-by: omitted-expression typing + `isArrayLikeType`.
        let iterable_exists = self.iterables_resolvable_via_protocol();
        let mut element_types = Vec::with_capacity(elements.len());
        for &element in &elements {
            if let NodeData::SpreadElement(d) = program.arena().data(element) {
                let spread_type = self.check_expression(program, d.expression);
                let rest_element_type = self.check_iterated_type_or_element_type(
                    program,
                    spread_type,
                    Some(d.expression),
                    iterable_exists,
                );
                element_types.push(rest_element_type.unwrap_or(self.error_type));
            } else {
                element_types.push(
                    self.check_expression_for_mutable_location(program, element),
                );
            }
        }
        // In a const context (`[1, 2] as const`) the literal is a readonly tuple
        // whose element types are the preserved literals (the elements above were
        // typed in the same const context, so they are already regular literals,
        // not widened). Go: `createArrayLiteralType(createTupleTypeEx(elementTypes,
        // elementInfos, inConstContext && !mutableArrayLikeContext))`; the
        // reachable subset always has `readonly = inConstContext` (no contextual
        // mutable-array-like type to clear it).
        //
        // DEFER(phase-4-checker-4bi+): the non-const tuple contexts (`forceTuple`
        // / a tuple-like contextual type), the `createArrayLiteralType`
        // `ObjectFlagsArrayLiteral`/`ContainsObjectOrArrayLiteral` clone, and a
        // mutable-array-like contextual type clearing the readonly flag.
        // blocked-by: contextual typing + `forceTuple` check mode + array-literal
        // widening flags.
        if is_const_context(program, node) {
            return self.create_tuple_type_ex(element_types, true);
        }
        let element_type = if element_types.is_empty() {
            // Go: `core.IfElse(c.strictNullChecks, c.implicitNeverType,
            // c.undefinedWideningType)`. The widening distinction is not modeled.
            if self.strict_null_checks() {
                self.never_type()
            } else {
                self.undefined_type()
            }
        } else {
            self.get_union_type(&element_types)
        };
        self.create_array_literal_type(program, node, element_type)
    }

    // Builds the `Array<element_type>` reference for an array literal at `node`,
    // resolving the global `Array` interface by name through the node's scope
    // (Go's `createArrayType` -> `createTypeReference(globalArrayType,
    // [elementType])`). Mirrors `get_type_from_array_type_node` (the `T[]` type
    // node path), which likewise resolves `Array` by name as the lib stand-in.
    //
    // DEFER(phase-4-checker-P6): no global `Array` in scope (lib.d.ts not
    // loaded) yields the error type. blocked-by: library globals (P6).
    // Go: internal/checker/checker.go:Checker.createArrayType / createArrayTypeEx
    fn create_array_literal_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        element_type: TypeId,
    ) -> TypeId {
        let globals = program.globals();
        let array_symbol =
            match resolve_name(program, node, "Array", SymbolFlags::TYPE, false, globals) {
                Some(symbol) => symbol,
                None => return self.error_type,
            };
        let target = get_declared_type_of_symbol(self, program, array_symbol, globals);
        self.create_type_reference(target, vec![element_type])
    }

    // Checks a non-null assertion `expr!`: the operand's type with `null`/
    // `undefined`/`void` removed (Go's `checkNonNullAssertion` non-optional-chain
    // path -> `GetNonNullableType(checkExpression(node.Expression()))`).
    //
    // DEFER(phase-4-checker-4az+): the optional-chain form (`a?.b!`, when
    // `node.Flags & NodeFlagsOptionalChain`), which Go routes to
    // `checkNonNullChain` (strip the optional marker, non-null, re-propagate).
    // blocked-by: optional-chain expression typing + optional-type markers.
    // Go: internal/checker/checker.go:Checker.checkNonNullAssertion(10582)
    fn check_non_null_assertion(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let expr = match program.arena().data(node) {
            NodeData::NonNullExpression(d) => d.expression,
            _ => return self.error_type,
        };
        let operand_type = self.check_expression(program, expr);
        self.get_non_null_type(operand_type)
    }

    /// Checks an expression used as the object of a property/element access (Go's
    /// `checkNonNullExpression`): types the expression, then runs the
    /// possibly-`null`/`undefined` check on `node`.
    // Go: internal/checker/checker.go:Checker.checkNonNullExpression(7373)
    pub fn check_non_null_expression(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        let t = self.check_expression(program, node);
        self.check_non_null_type(program, t, node)
    }

    /// Reports the possibly-`null`/`undefined` error for a property/element access
    /// object and narrows to the non-null type (Go's `checkNonNullType`).
    // Go: internal/checker/checker.go:Checker.checkNonNullType(7377)
    pub fn check_non_null_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        node: NodeId,
    ) -> TypeId {
        self.check_non_null_type_with_reporter(program, t, node, NonNullReporter::Access)
    }

    // Reports the possibly-`null`/`undefined` error when `t` can be
    // `null`/`undefined` under strictNullChecks, then narrows to the non-null
    // type. The `reporter` selects the diagnostic family: property/element
    // access (`2531`/`2532`/`2533`, or the entity-name `18047`/`18048`/`18049`)
    // vs invocation (`2721`/`2722`/`2723`). Returns `error_type` if nothing
    // non-nullable survives (Go's `checkNonNullTypeWithReporter` reachable
    // subset).
    //
    // DEFER(phase-4-checker-4ba+): the `unknown`-operand branch
    // (`Object_is_of_type_unknown` / `_0_is_of_type_unknown`, 2571/18046) and
    // the `checkNonNullNonVoidType` void path. blocked-by: `unknown` entity-name
    // reporting + void-access diagnostics. Gated on strictNullChecks (Go relies
    // on non-strict union simplification to suppress the facts; gating gives the
    // same observable: no report in non-strict).
    // Go: internal/checker/checker.go:Checker.checkNonNullTypeWithReporter(7381)
    fn check_non_null_type_with_reporter(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        node: NodeId,
        reporter: NonNullReporter,
    ) -> TypeId {
        if !self.strict_null_checks() {
            return t;
        }
        let facts = self.get_type_facts(t) & TypeFacts::IS_UNDEFINED_OR_NULL;
        if !facts.intersects(TypeFacts::IS_UNDEFINED_OR_NULL) {
            return t;
        }
        match reporter {
            NonNullReporter::Access => {
                self.report_object_possibly_null_or_undefined_error(program, node, facts)
            }
            NonNullReporter::Invocation => {
                self.report_cannot_invoke_possibly_null_or_undefined_error(program, node, facts)
            }
        }
        let non_nullable = self.get_non_null_type(t);
        if self
            .get_type(non_nullable)
            .flags()
            .intersects(TypeFlags::NULLABLE | TypeFlags::NEVER)
        {
            return self.error_type;
        }
        non_nullable
    }

    // Emits the possibly-`null`/`undefined` diagnostic for `node` given its
    // `IsUndefined`/`IsNull` facts. An entity-name object (`x`, `a.b`) shorter
    // than 100 chars uses the `'{0}' is possibly ...` form (`18047`/`18048`/
    // `18049`); otherwise the `Object is possibly ...` form (`2531`/`2532`/
    // `2533`). A bare `null`/`undefined` value reports `The_value_0_cannot_be
    // _used_here`.
    // Go: internal/checker/checker.go:Checker.reportObjectPossiblyNullOrUndefinedError(7424)
    fn report_object_possibly_null_or_undefined_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        facts: TypeFacts,
    ) {
        let arena = program.arena();
        let kind = arena.kind(node);
        let node_text = if is_entity_name_expression(arena, node) {
            Some(entity_name_to_string(arena, node))
        } else {
            None
        };
        if kind == Kind::NullKeyword {
            self.error(
                program,
                node,
                &tsgo_diagnostics::THE_VALUE_0_CANNOT_BE_USED_HERE,
                &["null"],
            );
            return;
        }
        let has_undefined = facts.intersects(TypeFacts::IS_UNDEFINED);
        let has_null = facts.intersects(TypeFacts::IS_NULL);
        match node_text {
            Some(text) if text.len() < 100 => {
                if kind == Kind::Identifier && text == "undefined" {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::THE_VALUE_0_CANNOT_BE_USED_HERE,
                        &["undefined"],
                    );
                    return;
                }
                let message: &'static Message = if has_undefined {
                    if has_null {
                        &tsgo_diagnostics::X_0_IS_POSSIBLY_NULL_OR_UNDEFINED
                    } else {
                        &tsgo_diagnostics::X_0_IS_POSSIBLY_UNDEFINED
                    }
                } else {
                    &tsgo_diagnostics::X_0_IS_POSSIBLY_NULL
                };
                self.error(program, node, message, &[text.as_str()]);
            }
            _ => {
                let message: &'static Message = if has_undefined {
                    if has_null {
                        &tsgo_diagnostics::OBJECT_IS_POSSIBLY_NULL_OR_UNDEFINED
                    } else {
                        &tsgo_diagnostics::OBJECT_IS_POSSIBLY_UNDEFINED
                    }
                } else {
                    &tsgo_diagnostics::OBJECT_IS_POSSIBLY_NULL
                };
                self.error(program, node, message, &[]);
            }
        }
    }

    // Emits the cannot-invoke possibly-`null`/`undefined` diagnostic for a call
    // callee given its `IsUndefined`/`IsNull` facts (`2722` for possibly-
    // `undefined`, `2721` for possibly-`null`, `2723` for both). Unlike the
    // property-access reporter, this family has no entity-name vs `Object`
    // split: the message is the same regardless of the callee shape (Go's
    // `reportCannotInvokePossiblyNullOrUndefinedError`).
    // Go: internal/checker/checker.go:Checker.reportCannotInvokePossiblyNullOrUndefinedError(9854)
    fn report_cannot_invoke_possibly_null_or_undefined_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        facts: TypeFacts,
    ) {
        let has_undefined = facts.intersects(TypeFacts::IS_UNDEFINED);
        let has_null = facts.intersects(TypeFacts::IS_NULL);
        let message: &'static Message = if has_undefined {
            if has_null {
                &tsgo_diagnostics::CANNOT_INVOKE_AN_OBJECT_WHICH_IS_POSSIBLY_NULL_OR_UNDEFINED
            } else {
                &tsgo_diagnostics::CANNOT_INVOKE_AN_OBJECT_WHICH_IS_POSSIBLY_UNDEFINED
            }
        } else {
            &tsgo_diagnostics::CANNOT_INVOKE_AN_OBJECT_WHICH_IS_POSSIBLY_NULL
        };
        self.error(program, node, message, &[]);
    }

    // Resolves an identifier reference to its (flow-narrowed) value type.
    // Go: internal/checker/checker.go:Checker.checkIdentifier(10999)
    fn check_identifier(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        // Go's `getResolvedSymbol` only calls `resolveName` (which emits the
        // cannot-find-name diagnostic on failure) when `!ast.NodeIsMissing(node)`.
        // A parser-recovered MISSING identifier (zero-width, empty text — e.g.
        // the body/condition of an incomplete `do` statement, or an empty
        // declaration list in `for (let in)`) resolves to `unknownSymbol` with no
        // error, so `checkIdentifier` returns the error type. Without this guard
        // the empty-name identifier cascaded into a spurious `TS2304: Cannot find
        // name ''.` that `tsc` never emits.
        // Go: internal/checker/checker.go:Checker.getResolvedSymbol (NodeIsMissing guard)
        if tsgo_ast::utilities::node_is_missing(program.arena(), node) {
            return self.error_type;
        }
        let name = program.arena().text(node).to_string();
        // Go's `resolveName` always consults the outermost `c.globals` scope, so
        // a bare identifier referencing a global VALUE (a lib global like
        // `Error`/`Object`/`Date`, or a cross-file global declaration) resolves.
        // Passing `None` here previously dropped the globals scope, cascading
        // every global-value reference into a spurious 2304 (and a follow-on
        // 2339 on its `error`-typed members).
        let globals = program.globals();
        // Go's `getResolvedSymbol` resolves a value identifier with meaning
        // `Value | ExportValue`. `ExportValue` is required because the binder
        // gives a top-level EXPORTED value declaration TWO symbols: a phantom
        // `ExportValue` local in the module's `locals` (so locals/exports of the
        // same name stay mutually exclusive) and the real symbol in `exports`,
        // linked from the phantom via `export_symbol`. A `Value`-only lookup
        // misses the phantom (its sole flag is `ExportValue`, which does not
        // intersect `Value`), cascading every same-module reference to an
        // exported enum/class/function/const into a spurious TS2304.
        // Go: internal/checker/checker.go:Checker.getResolvedSymbol
        match resolve_name(
            program,
            node,
            &name,
            SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
            false,
            globals,
        ) {
            None => {
                // Alias fallback (Go's `getSymbol` alias branch): a name imported
                // with `import { x } from "m"` / `import d from "m"` / `import *
                // as ns from "m"` is bound as an `Alias` symbol whose flags do
                // not intersect `Value`, so the direct `Value` lookup misses it.
                // Resolve the alias's target; when it denotes a value (or its
                // resolution failed — Go's `unknownSymbol`, already reported via
                // TS2305, returned to suppress a cascading TS2304) the alias
                // resolves, and `get_type_of_symbol` follows it to the imported
                // declaration's type.
                // Go: internal/binder/nameresolver.go:NameResolver.getSymbol (alias branch)
                if let Some(alias) =
                    resolve_name(program, node, &name, SymbolFlags::ALIAS, false, globals)
                {
                    // Go: internal/checker/checker.go:Checker.onSuccessfullyResolvedSymbol
                    self.on_successfully_resolved_symbol(
                        program,
                        node,
                        alias,
                        SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
                    );
                    let assignment_kind =
                        tsgo_ast::utilities::get_assignment_target_kind(program.arena(), node);
                    if assignment_kind != tsgo_ast::utilities::AssignmentKind::None
                        && self
                            .check_assignment_target_symbol(program, node, alias, alias)
                            .is_err()
                    {
                        return self.error_type;
                    }
                    let resolves_to_value = match resolve_alias(self, program, alias) {
                        Some(target) => self
                            .resolved_symbol_flags(program, target)
                            .intersects(SymbolFlags::VALUE),
                        None => true,
                    };
                    if resolves_to_value {
                        let declared = get_type_of_symbol(self, program, alias, globals);
                        return self.get_flow_type_of_reference(program, node, declared);
                    }
                    return self.error_type;
                }
                // Go registers a global `undefinedSymbol` (type
                // `undefinedWideningType`) in `NewChecker`, so the `undefined`
                // value identifier always resolves; the stub program has no lib,
                // so resolve it to the `undefined` type here (the widening
                // distinction is not modeled).
                // Go: internal/checker/checker.go:NewChecker (undefinedSymbol)
                if name == "undefined" {
                    return self.undefined_type();
                }
                // Go's `resolveName` returns the synthetic `require` symbol when
                // an unresolved name is the callee of a `require(...)` call in a
                // JS file; that symbol's type is `any`. This is what lets
                // CommonJS `const a = require("./x")` type-check without a
                // spurious 2304 on the `require` identifier. The reachable subset
                // returns `any` directly (equivalent to typing the require
                // symbol), since flow-narrowing a freshly-`any` callee is a no-op.
                // Go: internal/binder/nameresolver.go:Resolve (RequireSymbol branch)
                let arena = program.arena();
                if is_in_js_file(arena, node) {
                    if let Some(parent) = arena.parent(node) {
                        if is_require_call(arena, parent) {
                            return self.any_type();
                        }
                    }
                }
                let message = self.get_cannot_find_name_diagnostic_for_name(program, node);
                // Go's `onFailedToResolveSymbol` reports the missing-lib variant
                // FIRST: when `getSuggestedLibForNonExistentName(name)` is
                // non-empty it passes the suggested lib as the `{1}` argument
                // (the TS2583 "...change the 'lib' option to '{1}' or later."
                // message), otherwise just the name `{0}`. Every name routed to
                // TS2583 has a feature-map entry, so the `{1}` is always present
                // for that message (and is harmlessly ignored by the `{0}`-only
                // messages).
                // Go: internal/checker/checker.go:Checker.onFailedToResolveSymbol
                let suggested_lib = get_suggested_lib_for_non_existent_name(&name);
                if suggested_lib.is_empty() {
                    self.error(program, node, message, &[name.as_str()]);
                } else {
                    self.error(program, node, message, &[name.as_str(), suggested_lib]);
                }
                self.error_type
            }
            Some(symbol) => {
                // Go: internal/checker/checker.go:Checker.onSuccessfullyResolvedSymbol
                self.on_successfully_resolved_symbol(
                    program,
                    node,
                    symbol,
                    SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
                );
                // Go's `checkIdentifier` maps the resolved symbol through
                // `getExportSymbolOfValueSymbolIfExported` before typing it: a
                // phantom `ExportValue` local carries no real declaration flags,
                // so its type/declarations must be read from the linked
                // `export_symbol` (the real exported enum/class/function/const).
                let original_symbol = symbol;
                let symbol = get_export_symbol_of_value_symbol_if_exported(program, symbol);
                let assignment_kind =
                    tsgo_ast::utilities::get_assignment_target_kind(program.arena(), node);
                if assignment_kind != tsgo_ast::utilities::AssignmentKind::None
                    && self
                        .check_assignment_target_symbol(
                            program,
                            node,
                            original_symbol,
                            symbol,
                        )
                        .is_err()
                {
                    return self.error_type;
                }
                // Go: `markLinkedReferences(node, ReferenceHintIdentifier)` ->
                // accumulates `referenceKinds` for unused checking. The reachable
                // subset marks every successfully-resolved identifier as a
                // `VARIABLE` reference.
                // Go: internal/checker/checker.go:Checker.checkIdentifier -> markLinkedReferences
                self.mark_symbol_referenced(symbol, SymbolFlags::VARIABLE);
                let declared = get_type_of_symbol(self, program, symbol, globals);
                self.get_flow_type_of_reference(program, node, declared)
            }
        }
    }

    // Validates that `symbol` may be written to when `node` is an assignment
    // target (definite, compound, or `for`-`in`/`of` initializer).
    // Go: internal/checker/checker.go:Checker.checkIdentifier (assignment guards)
    fn check_assignment_target_symbol(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        display_symbol: SymbolId,
        symbol: SymbolId,
    ) -> Result<(), ()> {
        let flags = self.resolved_symbol_flags(program, symbol);
        let js_value_module_ok = is_in_js_file(program.arena(), node)
            && flags.intersects(SymbolFlags::VALUE_MODULE);
        let name =
            super::nodebuilder::symbol_to_string(self, program, display_symbol);
        if !flags.intersects(SymbolFlags::VARIABLE) && !js_value_module_ok {
            let message = if flags.intersects(SymbolFlags::ENUM) {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_AN_ENUM
            } else if flags.intersects(SymbolFlags::CLASS) {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_A_CLASS
            } else if flags.intersects(SymbolFlags::MODULE) {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_A_NAMESPACE
            } else if flags.intersects(SymbolFlags::FUNCTION) {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_A_FUNCTION
            } else if flags.intersects(SymbolFlags::ALIAS) {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_AN_IMPORT
            } else {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_NOT_A_VARIABLE
            };
            self.error(program, node, message, &[name.as_str()]);
            return Err(());
        }
        if is_readonly_symbol(self, program, symbol) {
            let message = if flags.intersects(SymbolFlags::VARIABLE) {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_A_CONSTANT
            } else {
                &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_A_READ_ONLY_PROPERTY
            };
            self.error(program, node, message, &[name.as_str()]);
            return Err(());
        }
        Ok(())
    }

    // Returns the specialized "cannot find name" diagnostic for an unresolved
    // identifier `node`, dispatched on the identifier text, its parent kind,
    // and the `types: ["*"]` wildcard option — mirroring tsc, which points the
    // user at the missing `@types/node` / dom / target-lib definitions instead
    // of the bare TS2304. Go's `getResolvedSymbol` passes this message to
    // `resolveName`, which emits it on resolution failure; in the port the
    // emission lives in `check_identifier` (Rust's `resolve_name` is a pure
    // lookup), so the dispatch is reproduced here.
    // Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName
    fn get_cannot_find_name_diagnostic_for_name(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> &'static Message {
        // `types: ["*"]` selects the shorter "install @types/X" variant (no
        // "...and then add 'X' to the types field..." tail), since a wildcard
        // `types` list already opts every `@types` package in.
        let uses_wildcard = program.compiler_options().uses_wildcard_types();
        match program.arena().text(node) {
            // The dom globals point at the `dom` lib (not gated on wildcard).
            "document" | "console" => {
                &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_CHANGE_YOUR_TARGET_LIBRARY_TRY_CHANGING_THE_LIB_COMPILER_OPTION_TO_INCLUDE_DOM
            }
            // `$` points at `@types/jquery`.
            "$" => {
                if uses_wildcard {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_JQUERY_TRY_NPM_I_SAVE_DEV_TYPES_SLASHJQUERY
                } else {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_JQUERY_TRY_NPM_I_SAVE_DEV_TYPES_SLASHJQUERY_AND_THEN_ADD_JQUERY_TO_THE_TYPES_FIELD_IN_YOUR_TSCONFIG
                }
            }
            // The test-runner globals point at `@types/jest`/`@types/mocha`.
            "beforeEach" | "describe" | "suite" | "it" | "test" => {
                if uses_wildcard {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_A_TEST_RUNNER_TRY_NPM_I_SAVE_DEV_TYPES_SLASHJEST_OR_NPM_I_SAVE_DEV_TYPES_SLASHMOCHA
                } else {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_A_TEST_RUNNER_TRY_NPM_I_SAVE_DEV_TYPES_SLASHJEST_OR_NPM_I_SAVE_DEV_TYPES_SLASHMOCHA_AND_THEN_ADD_JEST_OR_MOCHA_TO_THE_TYPES_FIELD_IN_YOUR_TSCONFIG
                }
            }
            // The CommonJS / Node ambient globals point at `@types/node`.
            "process" | "require" | "Buffer" | "module" | "NodeJS" => {
                if uses_wildcard {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_NODE_TRY_NPM_I_SAVE_DEV_TYPES_SLASHNODE
                } else {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_NODE_TRY_NPM_I_SAVE_DEV_TYPES_SLASHNODE_AND_THEN_ADD_NODE_TO_THE_TYPES_FIELD_IN_YOUR_TSCONFIG
                }
            }
            // `Bun` points at `@types/bun`.
            "Bun" => {
                if uses_wildcard {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_BUN_TRY_NPM_I_SAVE_DEV_TYPES_SLASHBUN
                } else {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_INSTALL_TYPE_DEFINITIONS_FOR_BUN_TRY_NPM_I_SAVE_DEV_TYPES_SLASHBUN_AND_THEN_ADD_BUN_TO_THE_TYPES_FIELD_IN_YOUR_TSCONFIG
                }
            }
            // Target-library globals point at a higher `lib` target. Go's
            // switch literal reads `"ast.Symbol"` here, which is a
            // search-and-replace artifact from the original `Symbol` case — a
            // bare identifier's `Text()` is never the qualified `ast.Symbol`, so
            // that arm is dead in Go. The port uses the real `"Symbol"` so the
            // behavior matches `tsc` (and `getSuggestedLibForNonExistentName`,
            // which keys on `"Symbol"`). The `{1}` lib is filled by the caller
            // from `get_suggested_lib_for_non_existent_name`.
            "Map" | "Set" | "Promise" | "Symbol" | "WeakMap" | "WeakSet" | "Iterator"
            | "AsyncIterator" | "SharedArrayBuffer" | "Atomics" | "AsyncIterable"
            | "AsyncIterableIterator" | "AsyncGenerator" | "AsyncGeneratorFunction" | "BigInt"
            | "Reflect" | "BigInt64Array" | "BigUint64Array" => {
                &tsgo_diagnostics::CANNOT_FIND_NAME_0_DO_YOU_NEED_TO_CHANGE_YOUR_TARGET_LIBRARY_TRY_CHANGING_THE_LIB_COMPILER_OPTION_TO_1_OR_LATER
            }
            // `await` used as the callee of a call (`await(x)` in a non-async
            // context, where it parses as an identifier) suggests the enclosing
            // function should be `async`; otherwise it FALLS THROUGH to the
            // default arm (Go's `fallthrough`).
            "await"
                if program
                    .arena()
                    .parent(node)
                    .is_some_and(|p| program.arena().kind(p) == Kind::CallExpression) =>
            {
                &tsgo_diagnostics::CANNOT_FIND_NAME_0_DID_YOU_MEAN_TO_WRITE_THIS_IN_AN_ASYNC_FUNCTION
            }
            _ => {
                // Default arm: an undefined shorthand property (`{ x }`) reports
                // TS18004 ("No value exists in scope for the shorthand
                // property..."); everything else is the generic TS2304.
                if program
                    .arena()
                    .parent(node)
                    .is_some_and(|p| program.arena().kind(p) == Kind::ShorthandPropertyAssignment)
                {
                    &tsgo_diagnostics::NO_VALUE_EXISTS_IN_SCOPE_FOR_THE_SHORTHAND_PROPERTY_0_EITHER_DECLARE_ONE_OR_PROVIDE_AN_INITIALIZER
                } else {
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0
                }
            }
        }
    }

    // Checks a `this` expression (Go's `checkThisExpression`), returning the
    // type of `this` at this location. The reachable subset resolves `this`
    // inside a non-static class member to the class instance type (so `this.x`
    // reads an instance property); a `this` with no class container yields
    // `any`.
    //
    // DEFER(phase-4-checker-C-D2+): the polymorphic `this` *type parameter*
    // (`getDeclaredTypeOfClassOrInterface(...).thisType`), the static-side
    // typing flow narrowing, the `noImplicitThis` diagnostics, the
    // arrow-capture / computed-property / module / enum container errors, and
    // the global-`this` fallback. blocked-by: polymorphic `this` type
    // parameter + `this`-parameter signatures + `noImplicitThis` option +
    // global `this` symbol.
    // Go: internal/checker/checker.go:Checker.checkThisExpression / tryGetThisTypeAtEx
    fn check_this_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        match self.try_get_this_type_at(program, node) {
            Some(t) => t,
            None => self.any_type(),
        }
    }

    // Resolves a `this` *type node* (`m(): this`) to the enclosing class's
    // instance type (Go's `getThisType`, reachable subset). Returns the error
    // type when `this` is not inside a non-static class/interface member.
    //
    // DEFER(phase-4-checker-C-D2+): the polymorphic `this` type parameter and
    // the 2526 "A 'this' type is available only in a non-static member of a
    // class or interface" diagnostic. blocked-by: polymorphic `this` type
    // parameter + grammar diagnostic wiring.
    // Go: internal/checker/checker.go:Checker.getThisType
    pub(crate) fn get_this_type_from_node(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        if let Some(container) = get_this_container(program, node) {
            if let Some(parent) = program.arena().parent(container) {
                if is_class_like(program.arena(), parent)
                    && !has_static_modifier(program.arena(), container)
                {
                    if let Some(symbol) = program.symbol_of_node(parent) {
                        let globals = program.globals();
                        return get_declared_type_of_symbol(self, program, symbol, globals);
                    }
                }
            }
        }
        self.error_type
    }

    // Resolves the type of `this` at `node` by walking to the enclosing
    // (non-arrow) function-like container: if its parent is a class, `this` is
    // the class instance type for a non-static member, or the class value
    // (static side) type for a static member (Go's `tryGetThisTypeAtEx`, class
    // branch). Returns `None` when there is no class container.
    // Go: internal/checker/checker.go:Checker.tryGetThisTypeAtEx
    fn try_get_this_type_at(&mut self, program: &dyn BoundProgram, node: NodeId) -> Option<TypeId> {
        let container = get_this_container(program, node)?;
        let parent = program.arena().parent(container)?;
        if !is_class_like(program.arena(), parent) {
            return None;
        }
        let symbol = program.symbol_of_node(parent)?;
        let globals = program.globals();
        if has_static_modifier(program.arena(), container) {
            // The static side: the class value type (its constructor/static
            // members object), mirrored by the namespace/enum value path.
            Some(super::declared_types::get_type_of_symbol(
                self, program, symbol, globals,
            ))
        } else {
            // The instance type (`this` in a non-static member).
            Some(super::declared_types::get_declared_type_of_symbol(
                self, program, symbol, globals,
            ))
        }
    }

    // Checks a `new C(...)` expression (Go's `checkNewExpression` ->
    // `resolveNewExpression`), returning the constructed instance type.
    //
    // Reachable subset: a class-identifier callee, whose constructed type is the
    // class's declared (instance) type. Constructing an `abstract` class reports
    // 2511 "Cannot create an instance of an abstract class.".
    //
    // DEFER(phase-4-checker-C-D2+): construct-signature resolution + overloads +
    // argument applicability, type-argument instantiation, and the
    // construct-signature-level `abstract` flag path.
    // blocked-by: construct signatures on the class value type + `new`-signature
    // applicability.
    // Go: internal/checker/checker.go:Checker.checkNewExpression / resolveNewExpression
    fn check_new_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (callee, args) = match program.arena().data(node) {
            NodeData::NewExpression(d) => (
                d.expression,
                d.arguments
                    .as_ref()
                    .map(|l| l.nodes.clone())
                    .unwrap_or_default(),
            ),
            _ => return self.error_type,
        };
        let callee_type = self.check_expression(program, callee);
        if self
            .get_type(callee_type)
            .flags()
            .intersects(TypeFlags::ANY)
        {
            for &arg in &args {
                self.check_expression(program, arg);
            }
            return self.any_type;
        }
        let construct_sigs = self.collect_construct_signatures_of_type(program, callee_type);
        if construct_sigs.is_empty() {
            let call_sigs = self.get_signatures_of_type(callee_type);
            if !call_sigs.is_empty() {
                let resolved_signature = if call_sigs.len() > 1 {
                    self.resolve_overloaded_call_signature(program, node, &call_sigs, &args)
                } else {
                    let signature = call_sigs[0];
                    if self.has_correct_arity(signature, args.len()) {
                        self.check_applicable_signature_for_call(program, signature, &args);
                    } else {
                        for &arg in &args {
                            self.check_expression(program, arg);
                        }
                        self.report_argument_arity_error(program, node, signature, &args);
                    }
                    signature
                };
                if !self.no_implicit_any() {
                    if self.signature(resolved_signature).declaration.is_some()
                        && self.get_return_type_of_signature(resolved_signature) != self.void_type
                    {
                        self.error(
                            program,
                            node,
                            &tsgo_diagnostics::ONLY_A_VOID_FUNCTION_CAN_BE_CALLED_WITH_THE_NEW_KEYWORD,
                            &[],
                        );
                    }
                    if self.get_this_type_of_signature(program, resolved_signature)
                        == Some(self.void_type)
                    {
                        self.error(
                            program,
                            node,
                            &tsgo_diagnostics::A_FUNCTION_THAT_IS_CALLED_WITH_THE_NEW_KEYWORD_CANNOT_HAVE_A_THIS_TYPE_THAT_IS_VOID,
                            &[],
                        );
                    }
                } else {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::X_NEW_EXPRESSION_WHOSE_TARGET_LACKS_A_CONSTRUCT_SIGNATURE_IMPLICITLY_HAS_AN_ANY_TYPE,
                        &[],
                    );
                    return self.any_type;
                }
                return self.get_return_type_of_call(program, resolved_signature, &[], &[]);
            }
            for &arg in &args {
                self.check_expression(program, arg);
            }
            self.invocation_error(
                program,
                callee,
                callee_type,
                InvocationKind::Construct,
            );
            return self.error_type;
        }
        if let Some(&sig) = construct_sigs.first() {
            if let Some(decl) = self.signature(sig).declaration {
                if program.arena().kind(decl) == Kind::Constructor {
                    if let Some(class_sym) = program.symbol_of_node(decl).and_then(|ctor_sym| {
                        program.symbol(ctor_sym).parent
                    }) {
                        if !self.is_constructor_accessible(program, node, class_sym, decl) {
                            for &arg in &args {
                                self.check_expression(program, arg);
                            }
                            return self.error_type;
                        }
                    }
                }
            }
        }
        if construct_sigs
            .iter()
            .any(|&sig| self.signature(sig).flags.contains(SignatureFlags::ABSTRACT))
        {
            self.error(
                program,
                node,
                &tsgo_diagnostics::CANNOT_CREATE_AN_INSTANCE_OF_AN_ABSTRACT_CLASS,
                &[],
            );
        } else if let Some(class_symbol) = self.new_expression_class_symbol(program, callee) {
            if let Some(decl) = program.symbol(class_symbol).value_declaration {
                if has_abstract_modifier(program.arena(), decl) {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::CANNOT_CREATE_AN_INSTANCE_OF_AN_ABSTRACT_CLASS,
                        &[],
                    );
                }
            }
        }
        self.resolve_construct_signatures(program, node, &construct_sigs, &args)
    }

    // Resolves a `new` expression against its construct-signature candidates
    // (Go's `resolveNewExpression` -> `resolveCall`).
    // Go: internal/checker/checker.go:Checker.resolveNewExpression(8592)
    fn resolve_construct_signatures(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signatures: &[SignatureId],
        args: &[NodeId],
    ) -> TypeId {
        if signatures.len() > 1 {
            return self.resolve_overloaded_call(program, node, signatures, args);
        }
        let signature = signatures[0];
        if self.has_correct_arity(signature, args.len()) {
            self.check_applicable_signature_for_call(program, signature, args);
        } else {
            for &arg in args {
                self.check_expression(program, arg);
            }
            self.report_argument_arity_error(program, node, signature, args);
        }
        self.get_return_type_of_call(program, signature, &[], &[])
    }

    // Resolves the class symbol a `new C(...)` callee refers to (reachable
    // subset: a plain identifier referencing a class value), or `None` when the
    // callee is not a class identifier.
    fn new_expression_class_symbol(
        &self,
        program: &dyn BoundProgram,
        callee: NodeId,
    ) -> Option<tsgo_ast::SymbolId> {
        if program.arena().kind(callee) != Kind::Identifier {
            return None;
        }
        let name = program.arena().text(callee).to_string();
        let globals = program.globals();
        let symbol = resolve_name(program, callee, &name, SymbolFlags::VALUE, false, globals)?;
        if program.symbol(symbol).flags.intersects(SymbolFlags::CLASS) {
            Some(symbol)
        } else {
            None
        }
    }

    // Reports whether a `new` expression may invoke `constructor` of
    // `declaring_class_sym`, emitting 2673/2674 when it may not.
    // Go: internal/checker/checker.go:Checker.isConstructorAccessible(8615)
    fn is_constructor_accessible(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        declaring_class_sym: SymbolId,
        constructor: NodeId,
    ) -> bool {
        let modifiers = modifier_flags_of(program.arena(), constructor);
        if !modifiers.intersects(ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER) {
            return true;
        }
        if program.arena().kind(constructor) != Kind::Constructor {
            return true;
        }
        let Some(class_decl) = get_class_like_declaration_of_symbol(program, declaring_class_sym)
        else {
            return true;
        };
        if is_node_within_class(program, node, class_decl) {
            return true;
        }
        if modifiers.contains(ModifierFlags::PROTECTED) {
            if let Some(containing_class) = get_containing_class(program, node) {
                if let Some(containing_sym) = program.symbol_of_node(containing_class) {
                    let globals = program.globals();
                    let containing_type =
                        get_declared_type_of_symbol(self, program, containing_sym, globals);
                    let target_type =
                        get_declared_type_of_symbol(self, program, declaring_class_sym, globals);
                    if has_base_type(self, containing_type, target_type) {
                        return true;
                    }
                }
            }
        }
        let class_name = super::nodebuilder::symbol_to_string(self, program, declaring_class_sym);
        if modifiers.contains(ModifierFlags::PRIVATE) {
            self.error(
                program,
                node,
                &tsgo_diagnostics::CONSTRUCTOR_OF_CLASS_0_IS_PRIVATE_AND_ONLY_ACCESSIBLE_WITHIN_THE_CLASS_DECLARATION,
                &[class_name.as_str()],
            );
        } else if modifiers.contains(ModifierFlags::PROTECTED) {
            self.error(
                program,
                node,
                &tsgo_diagnostics::CONSTRUCTOR_OF_CLASS_0_IS_PROTECTED_AND_ONLY_ACCESSIBLE_WITHIN_THE_CLASS_DECLARATION,
                &[class_name.as_str()],
            );
        }
        false
    }

    // Resolves a private-identifier property access (`obj.#name`) using lexical
    // class scope and the binder's mangled member name (Go's
    // `checkPropertyAccessExpressionOrQualifiedName` private-identifier arm).
    // Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName(11225)
    fn check_private_identifier_property_access(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        name_node: NodeId,
        object_type: TypeId,
        apparent: TypeId,
        is_super: bool,
    ) -> TypeId {
        let prop_name = program.arena().text(name_node);
        let lex_sym = lookup_symbol_for_private_identifier_declaration(program, prop_name, name_node);
        let prop = lex_sym.and_then(|sym| {
            let mangled = program.symbol(sym).name.clone();
            get_property_of_type(self, apparent, &mangled)
                .or_else(|| get_property_of_union_or_intersection_type(self, apparent, &mangled))
        });
        if let Some(prop) = prop {
            if is_property_access_write_only(program, node)
                && program
                    .symbol(prop)
                    .flags
                    .intersects(SymbolFlags::ACCESSOR)
            {
                if !self.check_property_accessibility(
                    program,
                    node,
                    is_super,
                    apparent,
                    prop,
                    Some(name_node),
                ) {
                    return self.error_type;
                }
                return super::declared_types::get_write_type_of_accessors(
                    self, program, prop, None,
                );
            }
            if let Some(t) = get_type_of_property_of_type(
                self,
                program,
                apparent,
                &program.symbol(prop).name,
            ) {
                if !self.check_property_accessibility(
                    program,
                    node,
                    is_super,
                    apparent,
                    prop,
                    Some(name_node),
                ) {
                    return self.error_type;
                }
                return t;
            }
        }
        let type_str = super::nodebuilder::type_to_string(self, program, object_type);
        self.error(
            program,
            name_node,
            &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1,
            &[prop_name, type_str.as_str()],
        );
        self.error_type
    }

    // Checks a property access `obj.name`, returning the property's type.
    // Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression
    fn check_property_access(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, name_node) = match program.arena().data(node) {
            NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
            _ => return self.error_type,
        };
        // Go's `checkPropertyAccessExpression` types the object via
        // `checkNonNullExpression`, reporting possibly-`null`/`undefined` and
        // narrowing the object to its non-null type before the member lookup.
        let object_type = self.check_non_null_expression(program, expr);
        // Go's `checkPropertyAccessExpressionOrQualifiedName` short-circuits an
        // any-like receiver (`isTypeAny(apparentType)`): accessing any member of
        // `any` — or of the `error` type (which also carries the `Any` flag) —
        // yields that same type with NO 2339. This stops the false-positive
        // cascade where an unresolved name (typed `error`) would otherwise add a
        // spurious "Property does not exist on type 'error'" on top of its 2304.
        if self
            .get_type(object_type)
            .flags()
            .intersects(TypeFlags::ANY)
        {
            return object_type;
        }
        let is_super = program.arena().kind(expr) == Kind::SuperKeyword;
        let apparent = get_apparent_type(self, object_type);
        if is_private_identifier_name_node(program.arena(), name_node) {
            return self.check_private_identifier_property_access(
                program,
                node,
                name_node,
                object_type,
                apparent,
                is_super,
            );
        }
        let name = program.arena().text(name_node).to_string();
        let assignment_kind =
            tsgo_ast::utilities::get_assignment_target_kind(program.arena(), node);
        if is_property_access_write_only(program, node) {
            if let Some(prop) = get_property_of_type(self, apparent, &name)
                .or_else(|| get_property_of_union_or_intersection_type(self, apparent, &name))
            {
                if program
                    .symbol(prop)
                    .flags
                    .intersects(SymbolFlags::ACCESSOR)
                {
                    if !self.check_property_accessibility(
                        program,
                        node,
                        is_super,
                        apparent,
                        prop,
                        Some(name_node),
                    ) {
                        return self.error_type;
                    }
                    let mut prop_type = super::declared_types::get_write_type_of_accessors(
                        self, program, prop, None,
                    );
                    if let Some((target, args)) =
                        self.get_type(apparent).as_object().and_then(|o| {
                            o.target
                                .map(|target| (target, o.resolved_type_arguments.clone()))
                        })
                    {
                        let params = self
                            .get_type(target)
                            .as_object()
                            .map(|o| o.type_parameters.clone())
                            .unwrap_or_default();
                        if !params.is_empty() && params.len() == args.len() {
                            let mapper = super::mapper::TypeMapper::Array {
                                sources: params,
                                targets: args,
                            };
                            prop_type = self.instantiate_type(prop_type, &mapper);
                        }
                    }
                    return prop_type;
                }
            }
        }
        if let Some(t) = self.get_type_of_property_of_type(program, apparent, &name) {
            if let Some(prop) = get_property_of_type(self, apparent, &name)
                .or_else(|| get_property_of_union_or_intersection_type(self, apparent, &name))
            {
                if !self.check_property_accessibility(
                    program,
                    node,
                    is_super,
                    apparent,
                    prop,
                    Some(name_node),
                ) {
                    return self.error_type;
                }
                if is_assignment_to_readonly_entity(
                    self,
                    program,
                    node,
                    prop,
                    assignment_kind,
                ) {
                    self.error(
                        program,
                        name_node,
                        &tsgo_diagnostics::CANNOT_ASSIGN_TO_0_BECAUSE_IT_IS_A_READ_ONLY_PROPERTY,
                        &[name.as_str()],
                    );
                    return self.error_type;
                }
            }
            return t;
        }
        if let Some(info) = get_applicable_index_info_for_name(self, program, apparent, &name) {
            self.error_if_writing_to_readonly_index(program, node, apparent, info);
            return self.index_info(info).value_type;
        }
        self.report_nonexistent_property(program, name_node, object_type, node, is_super);
        self.error_type
    }

    // Reports TS2542 when `access_node` writes through a readonly index signature.
    // Go: internal/checker/checker.go:Checker.errorIfWritingToReadonlyIndex
    fn error_if_writing_to_readonly_index(
        &mut self,
        program: &dyn BoundProgram,
        access_node: NodeId,
        object_type: TypeId,
        index_info: IndexInfoId,
    ) {
        if !self.index_info(index_info).is_readonly {
            return;
        }
        let arena = program.arena();
        let assignment_kind = tsgo_ast::utilities::get_assignment_target_kind(arena, access_node);
        if assignment_kind == tsgo_ast::utilities::AssignmentKind::None
            && !tsgo_ast::utilities::is_delete_target(arena, access_node)
        {
            return;
        }
        let type_str = super::nodebuilder::type_to_string(self, program, object_type);
        self.error(
            program,
            access_node,
            &tsgo_diagnostics::INDEX_SIGNATURE_IN_TYPE_0_ONLY_PERMITS_READING,
            &[type_str.as_str()],
        );
    }

    // Reports a missing property on `containing_type`, suggesting a close
    // spelling when one exists (Go's `reportNonexistentProperty`).
    // Go: internal/checker/checker.go:Checker.reportNonexistentProperty(11481)
    fn report_nonexistent_property(
        &mut self,
        program: &dyn BoundProgram,
        name_node: NodeId,
        containing_type: TypeId,
        access_node: NodeId,
        is_super: bool,
    ) {
        let name = program.arena().text(name_node).to_string();
        let type_str = super::nodebuilder::type_to_string(self, program, containing_type);
        let message_chain = self.union_missing_property_chain(
            program,
            name_node,
            containing_type,
            &name,
        );
        if let Some(promised) = self.get_promised_type_of_promise(program, containing_type) {
            let promised_apparent = get_apparent_type(self, promised);
            if get_property_of_type(self, promised_apparent, &name).is_some() {
                let mut diagnostic = self.diagnostic_for_node(
                    program,
                    name_node,
                    &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1,
                    &[name.as_str(), type_str.as_str()],
                );
                let related = self.diagnostic_for_node(
                    program,
                    name_node,
                    &tsgo_diagnostics::DID_YOU_FORGET_TO_USE_AWAIT,
                    &[],
                );
                diagnostic.add_related_info(related);
                if !message_chain.is_empty() {
                    diagnostic.message_chain = message_chain;
                }
                self.add_diagnostic(program, diagnostic);
                return;
            }
        }
        let mut diagnostic = if self.type_has_static_property(program, &name, containing_type) {
            let static_access = self.static_member_access_suggestion(
                program,
                access_node,
                &type_str,
                &name,
            );
            self.diagnostic_for_node(
                program,
                name_node,
                &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1_DID_YOU_MEAN_TO_ACCESS_THE_STATIC_MEMBER_2_INSTEAD,
                &[name.as_str(), type_str.as_str(), static_access.as_str()],
            )
        } else if let Some(suggestion_sym) = self.get_suggested_symbol_for_nonexistent_property(
            program,
            &name,
            containing_type,
            access_node,
            is_super,
        ) {
            let suggested_name = program.symbol(suggestion_sym).name.clone();
            let mut diagnostic = self.diagnostic_for_node(
                program,
                name_node,
                &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1_DID_YOU_MEAN_2,
                &[name.as_str(), type_str.as_str(), suggested_name.as_str()],
            );
            if let Some(decl) = program.symbol(suggestion_sym).value_declaration {
                let related = self.diagnostic_for_node(
                    program,
                    decl,
                    &tsgo_diagnostics::X_0_IS_DECLARED_HERE,
                    &[suggested_name.as_str()],
                );
                diagnostic.add_related_info(related);
            }
            diagnostic
        } else {
            self.diagnostic_for_node(
                program,
                name_node,
                &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1,
                &[name.as_str(), type_str.as_str()],
            )
        };
        if !message_chain.is_empty() {
            diagnostic.message_chain = message_chain;
        }
        self.add_diagnostic(program, diagnostic);
    }

    // When `containing_type` is a non-primitive union, returns a chain node for
    // the first constituent missing `prop_name` (Go's `reportNonexistentProperty`
    // union preamble).
    // Go: internal/checker/checker.go:Checker.reportNonexistentProperty(11496)
    fn union_missing_property_chain(
        &mut self,
        program: &dyn BoundProgram,
        name_node: NodeId,
        containing_type: TypeId,
        prop_name: &str,
    ) -> Vec<DiagnosticMessageChain> {
        if is_private_identifier_name_node(program.arena(), name_node) {
            return Vec::new();
        }
        let containing_flags = self.get_type(containing_type).flags();
        if !containing_flags.intersects(TypeFlags::UNION)
            || containing_flags.intersects(TypeFlags::PRIMITIVE)
        {
            return Vec::new();
        }
        let members = self
            .get_type(containing_type)
            .union_types()
            .map(|m| m.to_vec())
            .unwrap_or_default();
        for subtype in members {
            let sub_apparent = get_apparent_type(self, subtype);
            let missing = get_property_of_type(self, sub_apparent, prop_name).is_none()
                && get_applicable_index_info_for_name(self, program, sub_apparent, prop_name)
                    .is_none();
            if missing {
                let subtype_str = super::nodebuilder::type_to_string(self, program, subtype);
                let message = &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1;
                return vec![DiagnosticMessageChain {
                    code: message.code(),
                    category: message.category(),
                    message: tsgo_diagnostics::format(
                        &message.to_string(),
                        &[prop_name, subtype_str.as_str()],
                    ),
                    next: Vec::new(),
                }];
            }
        }
        Vec::new()
    }

    // Reports whether `prop_name` exists as a static member on the class value
    // type associated with `containing_type` (Go's `typeHasStaticProperty`).
    // Go: internal/checker/checker.go:Checker.typeHasStaticProperty(27033)
    fn type_has_static_property(
        &mut self,
        program: &dyn BoundProgram,
        prop_name: &str,
        containing_type: TypeId,
    ) -> bool {
        let Some(class_sym) = self.get_type(containing_type).symbol else {
            return false;
        };
        let static_type =
            get_type_of_symbol(self, program, class_sym, program.globals());
        let apparent = get_apparent_type(self, static_type);
        let Some(prop) = get_property_of_type(self, apparent, prop_name) else {
            return false;
        };
        program
            .symbol(prop)
            .value_declaration
            .is_some_and(|decl| has_static_modifier(program.arena(), decl))
    }

    // Formats the static-member access suggestion for TS2576 (`Type.prop` or
    // `Type["prop"]` for element access).
    // Go: internal/checker/checker.go:Checker.reportNonexistentProperty(11507)
    fn static_member_access_suggestion(
        &self,
        program: &dyn BoundProgram,
        access_node: NodeId,
        type_name: &str,
        prop_name: &str,
    ) -> String {
        if program.arena().kind(access_node) == Kind::ElementAccessExpression {
            let arg = match program.arena().data(access_node) {
                NodeData::ElementAccessExpression(d) => d.argument_expression,
                _ => return format!("{type_name}.{prop_name}"),
            };
            let key = node_source_text(program, arg);
            format!("{type_name}[{key}]")
        } else {
            format!("{type_name}.{prop_name}")
        }
    }

    // Returns a close-spelling property symbol on `containing_type`, or `None`
    // when no candidate is near enough (Go's
    // `getSuggestedSymbolForNonexistentProperty`).
    // Go: internal/checker/checker.go:Checker.getSuggestedSymbolForNonexistentProperty(11559)
    fn get_suggested_symbol_for_nonexistent_property(
        &mut self,
        program: &dyn BoundProgram,
        prop_name: &str,
        containing_type: TypeId,
        access_node: NodeId,
        is_super: bool,
    ) -> Option<SymbolId> {
        let apparent = get_apparent_type(self, containing_type);
        let candidates: Vec<SymbolId> = get_properties_of_type(self, apparent)
            .into_iter()
            .filter(|(_, sym)| {
                self.is_property_accessible(program, access_node, is_super, false, apparent, *sym)
            })
            .map(|(_, sym)| sym)
            .collect();
        super::name_resolution::get_spelling_suggestion_for_name(
            program,
            prop_name,
            &candidates,
            SymbolFlags::VALUE,
        )
    }

    /// Reports whether `node` is a valid property access of `property_name`.
    ///
    /// Side effects: may type-check the receiver expression.
    // Go: internal/checker/services.go:Checker.isValidPropertyAccess
    pub fn is_valid_property_access(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        property_name: &str,
    ) -> bool {
        let data = match program.arena().data(node) {
            NodeData::PropertyAccessExpression(d) => d,
            _ => return false,
        };
        let is_super = program.arena().kind(data.expression) == Kind::SuperKeyword;
        let expr_type = self.check_expression(program, data.expression);
        let receiver = self.get_widened_type(expr_type);
        self.is_valid_property_access_with_type(program, node, is_super, property_name, receiver)
    }

    fn is_valid_property_access_with_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        is_super: bool,
        property_name: &str,
        t: TypeId,
    ) -> bool {
        if self.get_type(t).flags().intersects(TypeFlags::ANY) {
            return true;
        }
        let apparent = get_apparent_type(self, t);
        let prop = get_property_of_type(self, apparent, property_name)
            .or_else(|| get_property_of_union_or_intersection_type(self, apparent, property_name));
        prop.is_some_and(|p| {
            self.is_property_accessible(program, node, is_super, false, apparent, p)
        })
    }

    fn is_property_accessible(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        is_super: bool,
        is_write: bool,
        containing_type: TypeId,
        property: SymbolId,
    ) -> bool {
        if self
            .get_type(containing_type)
            .flags()
            .intersects(TypeFlags::ANY)
        {
            return true;
        }
        self.check_property_accessibility_at_location(
            program,
            node,
            is_super,
            is_write,
            containing_type,
            property,
            None,
        )
    }

    fn check_property_accessibility(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        is_super: bool,
        containing_type: TypeId,
        prop: SymbolId,
        error_node: Option<NodeId>,
    ) -> bool {
        self.check_property_accessibility_at_location(
            program,
            node,
            is_super,
            false,
            containing_type,
            prop,
            error_node,
        )
    }

    #[allow(clippy::too_many_arguments)] // Go: checkPropertyAccessibilityAtLocation arity
    fn check_property_accessibility_at_location(
        &mut self,
        program: &dyn BoundProgram,
        location: NodeId,
        is_super: bool,
        _writing: bool,
        _containing_type: TypeId,
        prop: SymbolId,
        error_node: Option<NodeId>,
    ) -> bool {
        let flags = declaration_modifier_flags_from_symbol(self, program, prop, false);
        if is_super && flags.contains(ModifierFlags::ABSTRACT) {
            return false;
        }
        if !flags.intersects(ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER) {
            return true;
        }
        if flags.contains(ModifierFlags::PRIVATE) {
            let Some(class_decl) = get_declaring_class_declaration(program, prop) else {
                return true;
            };
            if !is_node_within_class(program, location, class_decl) {
                if let Some(err) = error_node {
                    let prop_name = self.resolved_symbol_name(program, prop);
                    let class_name = declaring_class_name(program, prop);
                    self.error(
                        program,
                        err,
                        &tsgo_diagnostics::PROPERTY_0_IS_PRIVATE_AND_ONLY_ACCESSIBLE_WITHIN_CLASS_1,
                        &[prop_name.as_str(), class_name.as_str()],
                    );
                }
                return false;
            }
            return true;
        }
        if is_super {
            return true;
        }
        let Some(class_decl) = get_declaring_class_declaration(program, prop) else {
            return true;
        };
        if is_node_within_class(program, location, class_decl) {
            return true;
        }
        if let Some(err) = error_node {
            let prop_name = self.resolved_symbol_name(program, prop);
            let class_name = declaring_class_name(program, prop);
            self.error(
                program,
                err,
                &tsgo_diagnostics::PROPERTY_0_IS_PROTECTED_AND_ONLY_ACCESSIBLE_WITHIN_CLASS_1_AND_ITS_SUBCLASSES,
                &[prop_name.as_str(), class_name.as_str()],
            );
        }
        false
    }

    // Checks an element access `obj[index]`. String-literal indices first try a
    // named property; otherwise (and for all other index kinds) an applicable
    // index signature yields the indexed value type.
    // Go: internal/checker/checker.go:Checker.checkIndexedAccess
    fn check_element_access(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, arg) = match program.arena().data(node) {
            NodeData::ElementAccessExpression(d) => (d.expression, d.argument_expression),
            _ => return self.error_type,
        };
        // Go's `checkIndexedAccess` types the object via `checkNonNullExpression`
        // (reports possibly-`null`/`undefined`, narrows to the non-null type).
        let object_type = self.check_non_null_expression(program, expr);
        if self
            .get_type(object_type)
            .flags()
            .intersects(TypeFlags::ANY)
        {
            return object_type;
        }
        if program.arena().kind(arg) == Kind::StringLiteral {
            let name = program.arena().text(arg).to_string();
            if let Some(t) = self.get_type_of_property_of_type(program, object_type, &name) {
                return t;
            }
        }
        let index_type = self.check_expression(program, arg);
        if let Some(t) =
            super::declared_types::get_indexed_access_type(self, program, object_type, index_type)
        {
            let apparent = get_apparent_type(self, object_type);
            if let Some(info) =
                super::declared_types::get_applicable_index_info(self, program, apparent, index_type)
            {
                self.error_if_writing_to_readonly_index(program, node, apparent, info);
            }
            return t;
        }
        // Go's `getPropertyTypeForIndexType` ends on `Type_0_cannot_be_used_as_an
        // _index_type` (2538) when the index is not a string/number literal name
        // and is not string/number: such a key (e.g. `boolean`) is not assignable
        // to any index signature and never enters the index-signature block. The
        // 4af subset reports 2538 for a non-string/number/symbol-like index that
        // resolved no element type; `any`/`never` indices are excluded (Go returns
        // the index/object type for them).
        // DEFER(phase-4-checker-4af+): the symbol-keyed string-index fallback and
        // property-does-not-exist suggestions. blocked-by: ES-symbol globals (P6).
        let index_flags = self.get_type(index_type).flags();
        if index_flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL) {
            let is_super = program.arena().kind(expr) == Kind::SuperKeyword;
            self.report_nonexistent_property(program, arg, object_type, node, is_super);
            return self.error_type;
        }
        if !index_flags.intersects(
            TypeFlags::STRING_LIKE
                | TypeFlags::NUMBER_LIKE
                | TypeFlags::ES_SYMBOL_LIKE
                | TypeFlags::ANY
                | TypeFlags::NEVER,
        ) {
            let type_str = super::nodebuilder::type_to_string(self, program, index_type);
            self.error(
                program,
                arg,
                &tsgo_diagnostics::TYPE_0_CANNOT_BE_USED_AS_AN_INDEX_TYPE,
                &[type_str.as_str()],
            );
            return self.error_type;
        }
        if self.no_implicit_any()
            && index_flags.intersects(TypeFlags::STRING_LIKE | TypeFlags::NUMBER_LIKE)
            && !index_flags.intersects(TypeFlags::ANY | TypeFlags::NEVER)
            && index_flags.intersects(TypeFlags::STRING | TypeFlags::NUMBER)
        {
            self.report_element_access_implicit_any_index_error(
                program,
                node,
                arg,
                object_type,
                index_type,
            );
        }
        self.error_type
    }

    // Checks a binary expression `left <op> right` (Go's `checkBinaryExpression`
    // -> `checkBinaryLikeExpression`). Both operands are always checked so that
    // diagnostics inside them surface. 4n handles the assignment operator (`=`);
    // 4o adds the relational/equality arms (result `boolean` + comparability
    // diagnostics 2365/2367) and the non-`+` arithmetic arms (number-ish operand
    // checks 2362/2363, result `number`); 4p adds the logical (`&&`/`||`/`??`)
    // result-type arms, the `+`/`+=` arm (string/number/bigint/any result + the
    // not-applicable `2365`), and wires compound assignments (`+=`/`*=`/.../`&&=`/
    // `||=`/`??=`) through `check_assignment_operator`.
    //
    // 4ab adds the `instanceof` (result `boolean`; left-operand `2358`,
    // right-operand `2359` via the synthetic global `Function`) and `in` (result
    // `boolean`; operand assignability `2322`) arms.
    //
    // Checks a prefix unary expression (`-x`, `!x`, `++x`, ...).
    //
    // DEFER(phase-4-checker-4o+): the
    // `maybeTypeOfKindConsideringBaseConstraint` ES-symbol guard and the
    // `await`-suggestion path on arithmetic operands. blocked-by: base-constraint
    // type facts + awaited-type machinery (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression(10814)
    fn check_prefix_unary_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (operator, operand) = match program.arena().data(node) {
            NodeData::PrefixUnaryExpression(d) => (d.operator, d.operand),
            _ => return self.error_type,
        };
        let operand_type = self.check_expression(program, operand);
        if operand_type == self.silent_never_type {
            return self.silent_never_type;
        }
        let operand_kind = program.arena().kind(operand);
        if operand_kind == Kind::NumericLiteral {
            match operator {
                Kind::MinusToken => {
                    let value = tsgo_jsnum::from_string(program.arena().text(operand));
                    let negated = tsgo_jsnum::Number::from(-f64::from(value));
                    let regular = self.get_number_literal_type(negated);
                    return self.get_fresh_type_of_literal_type(regular);
                }
                Kind::PlusToken => {
                    let value = tsgo_jsnum::from_string(program.arena().text(operand));
                    let regular = self.get_number_literal_type(value);
                    return self.get_fresh_type_of_literal_type(regular);
                }
                _ => {}
            }
        }
        if operand_kind == Kind::BigIntLiteral && operator == Kind::MinusToken {
            let magnitude = tsgo_jsnum::parse_pseudo_big_int(program.arena().text(operand));
            let negated = tsgo_jsnum::PseudoBigInt::new(&magnitude, true);
            let negated_text = format!("{negated}n");
            let regular = self.get_bigint_literal_type(&negated_text);
            return self.get_fresh_type_of_literal_type(regular);
        }
        match operator {
            Kind::PlusToken | Kind::MinusToken | Kind::TildeToken => {
                let operand_type =
                    self.check_non_null_type(program, operand_type, operand);
                let op = tsgo_scanner::token_to_string(operator);
                if self.maybe_type_of_kind_considering_base_constraint(
                    program,
                    operand_type,
                    TypeFlags::ES_SYMBOL_LIKE,
                ) {
                    self.error(
                        program,
                        operand,
                        &tsgo_diagnostics::THE_0_OPERATOR_CANNOT_BE_APPLIED_TO_TYPE_SYMBOL,
                        &[&op],
                    );
                }
                if operator == Kind::PlusToken {
                    if self
                        .get_type(operand_type)
                        .flags()
                        .intersects(TypeFlags::BIG_INT_LIKE)
                    {
                        let ty_str = super::nodebuilder::type_to_string(
                            self,
                            program,
                            self.get_base_type_of_literal_type(operand_type),
                        );
                        self.error(
                            program,
                            operand,
                            &tsgo_diagnostics::OPERATOR_0_CANNOT_BE_APPLIED_TO_TYPE_1,
                            &[&op, ty_str.as_str()],
                        );
                    }
                    return self.number_type;
                }
                return self.get_unary_result_type(operand_type);
            }
            Kind::ExclamationToken => {
                self.check_truthiness_of_type(program, operand_type, operand);
                let facts =
                    self.get_type_facts(operand_type) & (TypeFacts::TRUTHY | TypeFacts::FALSY);
                return if facts == TypeFacts::TRUTHY {
                    self.false_type
                } else if facts == TypeFacts::FALSY {
                    self.true_type
                } else {
                    self.boolean_type
                };
            }
            Kind::PlusPlusToken | Kind::MinusMinusToken => {
                let non_null =
                    self.check_non_null_type(program, operand_type, operand);
                let ok = self.check_arithmetic_operand_type(
                    program,
                    operand,
                    non_null,
                    &tsgo_diagnostics::AN_ARITHMETIC_OPERAND_MUST_BE_OF_TYPE_ANY_NUMBER_BIGINT_OR_AN_ENUM_TYPE,
                    true,
                );
                if ok {
                    self.check_reference_expression(
                        program,
                        operand,
                        &tsgo_diagnostics::THE_OPERAND_OF_AN_INCREMENT_OR_DECREMENT_OPERATOR_MUST_BE_A_VARIABLE_OR_A_PROPERTY_ACCESS,
                        &tsgo_diagnostics::THE_OPERAND_OF_AN_INCREMENT_OR_DECREMENT_OPERATOR_MAY_NOT_BE_AN_OPTIONAL_PROPERTY_ACCESS,
                    );
                }
                return self.get_unary_result_type(operand_type);
            }
            _ => self.error_type,
        }
    }

    // Checks a postfix unary expression (`x++`, `x--`).
    //
    // DEFER(phase-4-checker-4o+): the `await`-suggestion path on arithmetic
    // operands. blocked-by: awaited-type machinery (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.checkPostfixUnaryExpression(10868)
    fn check_postfix_unary_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let operand = match program.arena().data(node) {
            NodeData::PostfixUnaryExpression(d) => d.operand,
            _ => return self.error_type,
        };
        let operand_type = self.check_expression(program, operand);
        if operand_type == self.silent_never_type {
            return self.silent_never_type;
        }
        let non_null = self.check_non_null_type(program, operand_type, operand);
        let ok = self.check_arithmetic_operand_type(
            program,
            operand,
            non_null,
            &tsgo_diagnostics::AN_ARITHMETIC_OPERAND_MUST_BE_OF_TYPE_ANY_NUMBER_BIGINT_OR_AN_ENUM_TYPE,
            true,
        );
        if ok {
            self.check_reference_expression(
                program,
                operand,
                &tsgo_diagnostics::THE_OPERAND_OF_AN_INCREMENT_OR_DECREMENT_OPERATOR_MUST_BE_A_VARIABLE_OR_A_PROPERTY_ACCESS,
                &tsgo_diagnostics::THE_OPERAND_OF_AN_INCREMENT_OR_DECREMENT_OPERATOR_MAY_NOT_BE_AN_OPTIONAL_PROPERTY_ACCESS,
            );
        }
        self.get_unary_result_type(operand_type)
    }

    // Returns the result type of a unary `-`/`~`/`++`/`--` on `operand_type`
    // (Go's `getUnaryResultType`).
    //
    // DEFER(phase-4-checker-4o+): the `isTypeAssignableToKind` any/unknown arm
    // that widens bigint results to `number | bigint`. blocked-by: per-kind
    // assignability slices.
    // Go: internal/checker/checker.go:Checker.getUnaryResultType(10882)
    pub(crate) fn get_unary_result_type(&self, operand_type: TypeId) -> TypeId {
        if self
            .get_type(operand_type)
            .flags()
            .intersects(TypeFlags::BIG_INT_LIKE)
        {
            if self.get_type(operand_type).flags().intersects(
                TypeFlags::ANY | TypeFlags::UNKNOWN | TypeFlags::NUMBER_LIKE,
            ) {
                return self.number_or_bigint_type;
            }
            return self.bigint_type;
        }
        self.number_type
    }

    // Validates that `expr` is a reference suitable for `++`/`--` (identifier or
    // access expression, not an optional chain).
    //
    // DEFER(phase-4-checker-4n+): destructuring targets and private identifiers.
    // blocked-by: destructuring reference forms.
    // Go: internal/checker/checker.go:Checker.checkReferenceExpression(13062)
    fn check_reference_expression(
        &mut self,
        program: &dyn BoundProgram,
        expr: NodeId,
        invalid_reference_message: &'static Message,
        invalid_optional_chain_message: &'static Message,
    ) -> bool {
        let node = skip_outer_expressions(program, expr);
        let kind = program.arena().kind(node);
        if kind != Kind::Identifier && !is_access_expression(kind) {
            self.error(program, expr, invalid_reference_message, &[]);
            return false;
        }
        if program
            .arena()
            .flags(node)
            .contains(NodeFlags::OPTIONAL_CHAIN)
        {
            self.error(program, expr, invalid_optional_chain_message, &[]);
            return false;
        }
        true
    }

    // DEFER(phase-4-checker-4ab+): destructuring-assignment targets, plus the
    // per-operator refinements noted on each arm below. blocked-by: per-operator
    // slices land later; lib globals (P6) for the ES-symbol operand / awaited
    // types, `strictNullChecks` wiring for `??`, and 4b union literal/subtype
    // reduction for the logical results.
    // Go: internal/checker/checker.go:Checker.checkBinaryExpression(12275)/checkBinaryLikeExpression(12280)
    fn check_binary_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (left, operator_token, right) = match program.arena().data(node) {
            NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
            _ => return self.error_type,
        };
        let left_type = self.check_expression(program, left);
        let right_type = self.check_expression(program, right);
        let operator = program.arena().kind(operator_token);
        match operator {
            Kind::EqualsToken => {
                self.check_assignment_operator(program, left, left_type, right_type, Some(right));
                right_type
            }
            // Relational operators (`<`/`>`/`<=`/`>=`) yield `boolean`; the
            // operands' literal types are based for comparison, then an
            // incomparable pair reports `2365` (Go's relational arm).
            Kind::LessThanToken
            | Kind::GreaterThanToken
            | Kind::LessThanEqualsToken
            | Kind::GreaterThanEqualsToken => {
                if self.check_for_disallowed_es_symbol_operand(
                    program,
                    left,
                    right,
                    left_type,
                    right_type,
                    operator,
                ) {
                    let left_base =
                        self.get_base_type_of_literal_type_for_comparison(left_type);
                    let right_base =
                        self.get_base_type_of_literal_type_for_comparison(right_type);
                    if !self.relational_operands_comparable(program, left_base, right_base) {
                        let awaited_left =
                            self.get_awaited_type_no_alias(program, left_base);
                        let awaited_right =
                            self.get_awaited_type_no_alias(program, right_base);
                        let would_work_with_await = (awaited_left != left_base
                            || awaited_right != right_base)
                            && self.relational_operands_comparable(
                                program,
                                awaited_left,
                                awaited_right,
                            );
                        let (error_left, error_right) = if would_work_with_await {
                            (left_base, right_base)
                        } else {
                            self.get_base_types_if_unrelated(
                                program,
                                left_base,
                                right_base,
                                |c, p, l, r| c.relational_operands_comparable(p, l, r),
                            )
                        };
                        self.report_binary_operator_error(
                            program,
                            node,
                            operator_token,
                            error_left,
                            error_right,
                            would_work_with_await,
                        );
                    }
                }
                self.boolean_type
            }
            // Equality operators (`==`/`!=`/`===`/`!==`) yield `boolean`; an
            // operand pair that is not equality-comparable in either direction
            // reports `2367`, generalizing literal operands to their base types
            // for the message when those are also incomparable (Go's equality arm
            // + `getBaseTypesIfUnrelated`).
            Kind::EqualsEqualsToken
            | Kind::ExclamationEqualsToken
            | Kind::EqualsEqualsEqualsToken
            | Kind::ExclamationEqualsEqualsToken => {
                if !self.equality_operands_comparable(program, left_type, right_type) {
                    let awaited_left =
                        self.get_awaited_type_no_alias(program, left_type);
                    let awaited_right =
                        self.get_awaited_type_no_alias(program, right_type);
                    let would_work_with_await = (awaited_left != left_type
                        || awaited_right != right_type)
                        && self.equality_operands_comparable(
                            program,
                            awaited_left,
                            awaited_right,
                        );
                    let (error_left, error_right) = if would_work_with_await {
                        (left_type, right_type)
                    } else {
                        self.get_base_types_if_unrelated(
                            program,
                            left_type,
                            right_type,
                            |c, p, l, r| c.equality_operands_comparable(p, l, r),
                        )
                    };
                    self.report_binary_operator_error(
                        program,
                        node,
                        operator_token,
                        error_left,
                        error_right,
                        would_work_with_await,
                    );
                }
                self.boolean_type
            }
            // Arithmetic operators (`-`/`*`/`/`/`%`/`**`/shifts/bitwise) require
            // number-ish operands and yield `number` (Go's arithmetic arm).
            //
            // DEFER(phase-4-checker-4o+): the `await`-suggestion path on
            // arithmetic operands. blocked-by: awaited-type machinery (lib globals, P6).
            Kind::MinusToken
            | Kind::AsteriskToken
            | Kind::AsteriskAsteriskToken
            | Kind::SlashToken
            | Kind::PercentToken
            | Kind::LessThanLessThanToken
            | Kind::GreaterThanGreaterThanToken
            | Kind::GreaterThanGreaterThanGreaterThanToken
            | Kind::AmpersandToken
            | Kind::BarToken
            | Kind::CaretToken
            | Kind::MinusEqualsToken
            | Kind::AsteriskEqualsToken
            | Kind::AsteriskAsteriskEqualsToken
            | Kind::SlashEqualsToken
            | Kind::PercentEqualsToken
            | Kind::LessThanLessThanEqualsToken
            | Kind::GreaterThanGreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
            | Kind::AmpersandEqualsToken
            | Kind::BarEqualsToken
            | Kind::CaretEqualsToken => {
                if left_type == self.silent_never_type || right_type == self.silent_never_type {
                    return self.silent_never_type;
                }
                let left_type = self.check_non_null_type(program, left_type, left);
                let right_type = self.check_non_null_type(program, right_type, right);
                let left_boolean = self.maybe_type_of_kind(left_type, TypeFlags::BOOLEAN_LIKE);
                let right_boolean = self.maybe_type_of_kind(right_type, TypeFlags::BOOLEAN_LIKE);
                if left_boolean && right_boolean {
                    if let Some(suggested) = get_suggested_boolean_operator(operator) {
                        let op = tsgo_scanner::token_to_string(operator);
                        let suggested_op = tsgo_scanner::token_to_string(suggested);
                        self.error(
                            program,
                            operator_token,
                            &tsgo_diagnostics::THE_0_OPERATOR_IS_NOT_ALLOWED_FOR_BOOLEAN_TYPES_CONSIDER_USING_1_INSTEAD,
                            &[op, suggested_op],
                        );
                        return self.number_type;
                    }
                }
                let left_ok = self.check_arithmetic_operand_type(
                    program,
                    left,
                    left_type,
                    &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_AN_ARITHMETIC_OPERATION_MUST_BE_OF_TYPE_ANY_NUMBER_BIGINT_OR_AN_ENUM_TYPE,
                    true,
                );
                let right_ok = self.check_arithmetic_operand_type(
                    program,
                    right,
                    right_type,
                    &tsgo_diagnostics::THE_RIGHT_HAND_SIDE_OF_AN_ARITHMETIC_OPERATION_MUST_BE_OF_TYPE_ANY_NUMBER_BIGINT_OR_AN_ENUM_TYPE,
                    true,
                );
                let result_type = if self.is_type_assignable_to_kind(
                    program,
                    left_type,
                    TypeFlags::ANY_OR_UNKNOWN,
                ) && self.is_type_assignable_to_kind(
                    program,
                    right_type,
                    TypeFlags::ANY_OR_UNKNOWN,
                ) || !self.maybe_type_of_kind(left_type, TypeFlags::BIG_INT_LIKE)
                    && !self.maybe_type_of_kind(right_type, TypeFlags::BIG_INT_LIKE)
                {
                    self.number_type
                } else if self.both_are_big_int_like(program, left_type, right_type) {
                    if matches!(
                        operator,
                        Kind::GreaterThanGreaterThanGreaterThanToken
                            | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
                    ) {
                        self.report_binary_operator_error(
                            program,
                            node,
                            operator_token,
                            left_type,
                            right_type,
                            false,
                        );
                    }
                    if matches!(
                        operator,
                        Kind::AsteriskAsteriskToken | Kind::AsteriskAsteriskEqualsToken
                    ) && (self.compiler_options().target as i32)
                        < (ScriptTarget::Es2016 as i32)
                    {
                        self.error(
                            program,
                            node,
                            &tsgo_diagnostics::EXPONENTIATION_CANNOT_BE_PERFORMED_ON_BIGINT_VALUES_UNLESS_THE_TARGET_OPTION_IS_SET_TO_ES2016_OR_LATER,
                            &[],
                        );
                    }
                    self.bigint_type
                } else {
                    self.report_binary_operator_error(
                        program,
                        node,
                        operator_token,
                        left_type,
                        right_type,
                        false,
                    );
                    self.error_type
                };
                if left_ok && right_ok {
                    if is_compound_assignment(operator) {
                        self.check_assignment_operator(
                            program,
                            left,
                            left_type,
                            result_type,
                            None,
                        );
                    }
                    self.check_shift_simplification(
                        program,
                        node,
                        operator,
                        operator_token,
                        left,
                        right,
                    );
                }
                result_type
            }
            // The `||`/`||=` operator (Go's `KindBarBarToken` arm): the result is
            // the left type, refined to the union of the left type's non-falsy
            // (truthy) part and the right type when the left type can be falsy.
            //
            // DEFER(phase-4-checker-4p+): `GetNonNullableType` of the truthy part
            // (identity here because `strictNullChecks` is unwired), union subtype
            // reduction, and flattening a union left operand. blocked-by:
            // `strictNullChecks` wiring + 4b union subtype/flatten reduction.
            Kind::BarBarToken | Kind::BarBarEqualsToken => {
                let mut result = left_type;
                if self.has_type_facts(left_type, TypeFacts::FALSY) {
                    let truthy = self.remove_definitely_falsy_types(left_type);
                    result = self.get_union_dropping_never(&[truthy, right_type]);
                }
                if operator == Kind::BarBarEqualsToken {
                    self.check_assignment_operator(program, left, left_type, right_type, None);
                }
                result
            }
            // The `&&`/`&&=` operator (Go's `KindAmpersandAmpersandToken` arm):
            // the result is the left type, refined to the union of the left
            // type's definitely-falsy part and the right type when the left type
            // can be truthy.
            //
            // DEFER(phase-4-checker-4p+): the precise falsy literal extraction for
            // string/number/bigint primitives (`emptyString`/`zero`/`zeroBigInt`
            // intrinsics) and union subtype reduction. blocked-by: the falsy
            // literal intrinsics + 4b union reduction.
            Kind::AmpersandAmpersandToken | Kind::AmpersandAmpersandEqualsToken => {
                let mut result = left_type;
                if self.has_type_facts(left_type, TypeFacts::TRUTHY) {
                    // `strictNullChecks` is unwired (off), so Go takes the falsy
                    // part from the base type of the right operand.
                    let t = self.get_base_type_of_literal_type(right_type);
                    let falsy = self.extract_definitely_falsy_types(t);
                    result = self.get_union_dropping_never(&[falsy, right_type]);
                }
                if operator == Kind::AmpersandAmpersandEqualsToken {
                    self.check_assignment_operator(program, left, left_type, right_type, None);
                }
                result
            }
            // The `??`/`??=` operator (Go's `KindQuestionQuestionToken` arm): the
            // result is the left type, refined to the union of the left type's
            // non-nullable part and the right type when the left type can be
            // `undefined`/`null` (`hasTypeFacts(left, EQUndefinedOrNull)`). For a
            // non-nullable left, the result is exactly the left type.
            //
            // The result union is subtype-reduced (`UnionReductionSubtype`): a
            // member that is a subtype of another (e.g. the literal `"a"`
            // subsumed by `string`) is dropped, so `("a" | undefined) ?? string`
            // is `string`, not `"a" | string`.
            //
            Kind::QuestionQuestionToken | Kind::QuestionQuestionEqualsToken => {
                // Go runs the mixed-operator grammar check (`5076`) only for the
                // binary `??` form, not the `??=` compound assignment.
                if operator == Kind::QuestionQuestionToken {
                    self.check_nullish_coalesce_operands(program, node, left, right);
                }
                let mut result = left_type;
                if self.has_type_facts(left_type, TypeFacts::EQ_UNDEFINED_OR_NULL) {
                    let non_null = self.get_non_null_type(left_type);
                    let reduced = self.subtype_reduce(program, &[non_null, right_type]);
                    result = self.get_union_type(&reduced);
                }
                if operator == Kind::QuestionQuestionEqualsToken {
                    self.check_assignment_operator(program, left, left_type, right_type, None);
                }
                result
            }
            // The `+`/`+=` operator (Go's `KindPlusToken`/`KindPlusEqualsToken`
            // arm): the result is `number` when both operands are number-like,
            // `bigint` when both are bigint-like, `string` when either is
            // string-like, and `any`/`error` when either is `any`; otherwise the
            // operator cannot be applied (`2365`).
            //
            // DEFER(phase-4-checker-4p+): the ES-symbol operand diagnostic (`2469`,
            // `checkForDisallowedESSymbolOperand`), the `await`-suggestion path,
            // and literal-operand generalization for the `2365` message
            // (`getBaseTypesIfUnrelated`). blocked-by: the `Symbol` lib global (P6)
            // + awaited-type machinery + the literal-generalization helper.
            Kind::PlusToken | Kind::PlusEqualsToken => {
                let left_num = self.is_type_assignable_to_kind_strict(
                    program,
                    left_type,
                    TypeFlags::NUMBER_LIKE,
                );
                let right_num = self.is_type_assignable_to_kind_strict(
                    program,
                    right_type,
                    TypeFlags::NUMBER_LIKE,
                );
                let result = if left_num && right_num {
                    Some(self.number_type)
                } else if self.is_type_assignable_to_kind_strict(
                    program,
                    left_type,
                    TypeFlags::BIG_INT_LIKE,
                ) && self.is_type_assignable_to_kind_strict(
                    program,
                    right_type,
                    TypeFlags::BIG_INT_LIKE,
                ) {
                    Some(self.bigint_type)
                } else if self.is_type_assignable_to_kind_strict(
                    program,
                    left_type,
                    TypeFlags::STRING_LIKE,
                ) || self.is_type_assignable_to_kind_strict(
                    program,
                    right_type,
                    TypeFlags::STRING_LIKE,
                ) {
                    Some(self.string_type)
                } else if self.get_type(left_type).flags().intersects(TypeFlags::ANY)
                    || self.get_type(right_type).flags().intersects(TypeFlags::ANY)
                {
                    // Either operand is `any` (or the error type): assume the
                    // operation resolves, propagating `error` to avoid cascading.
                    if left_type == self.error_type || right_type == self.error_type {
                        Some(self.error_type)
                    } else {
                        Some(self.any_type)
                    }
                } else {
                    None
                };
                match result {
                    Some(rt) => {
                        if !self.check_for_disallowed_es_symbol_operand(
                            program,
                            left,
                            right,
                            left_type,
                            right_type,
                            operator,
                        ) {
                            return rt;
                        }
                        // For `+=`, the result must be assignable to the
                        // (reference) left-hand side (Go runs `checkAssignmentOperator`
                        // only when a valid result exists).
                        if operator == Kind::PlusEqualsToken {
                            self.check_assignment_operator(program, left, left_type, rt, None);
                        }
                        rt
                    }
                    None => {
                        // No applicable result: the operator cannot be applied.
                        // DEFER(phase-4-checker-4p+): literal-operand generalization
                        // for the message (`getBaseTypesIfUnrelated`). blocked-by:
                        // the literal-generalization helper.
                        let close_enough = TypeFlags::NUMBER_LIKE
                            | TypeFlags::BIG_INT_LIKE
                            | TypeFlags::STRING_LIKE
                            | TypeFlags::ANY_OR_UNKNOWN;
                        let awaited_left = self.get_awaited_type_no_alias(program, left_type);
                        let awaited_right = self.get_awaited_type_no_alias(program, right_type);
                        let would_work_with_await = (awaited_left != left_type
                            || awaited_right != right_type)
                            && self.is_type_assignable_to_kind(program, awaited_left, close_enough)
                            && self
                                .is_type_assignable_to_kind(program, awaited_right, close_enough);
                        let (error_left, error_right) = if would_work_with_await {
                            (left_type, right_type)
                        } else {
                            self.get_base_types_if_unrelated(
                                program,
                                left_type,
                                right_type,
                                |c, p, l, r| {
                                    c.is_type_assignable_to_kind(p, l, close_enough)
                                        && c.is_type_assignable_to_kind(p, r, close_enough)
                                },
                            )
                        };
                        self.report_binary_operator_error(
                            program,
                            node,
                            operator_token,
                            error_left,
                            error_right,
                            would_work_with_await,
                        );
                        self.any_type
                    }
                }
            }
            // The `instanceof` operator (Go's `KindInstanceOfKeyword` arm ->
            // `checkInstanceOfExpression`): the result is always `boolean`; the
            // left operand must be object-ish (else `2358`) and the right operand
            // must be callable or assignable to the global `Function` interface
            // (else `2359`).
            Kind::InstanceOfKeyword => {
                self.check_instanceof_expression(program, left, right, left_type, right_type)
            }
            // The `in` operator (Go's `KindInKeyword` arm -> `checkInExpression`):
            // the result is always `boolean`; the left operand must be assignable
            // to `string | number | symbol` and the right operand to `object`.
            Kind::InKeyword => {
                self.check_in_expression(program, left, right, left_type, right_type)
            }
            // The comma operator (Go's `KindCommaToken` arm): the result is the
            // right operand's type. When `allowUnreachableCode` is not true and
            // the left operand has no side effects (and the comma is not an
            // indirect call like `(0, eval)(...)`), reports `2695`.
            //
            // DEFER(phase-4-checker-later): the JSX `2657` suppression that skips
            // `2695` when the comma sits inside a JSX expression with multiple
            // roots. blocked-by: JSX checking.
            Kind::CommaToken => {
                if !self.compiler_options().allow_unreachable_code.is_true()
                    && self.is_side_effect_free(program, left)
                    && !is_indirect_call(program, node)
                {
                    self.error(
                        program,
                        left,
                        &tsgo_diagnostics::LEFT_SIDE_OF_COMMA_OPERATOR_IS_UNUSED_AND_HAS_NO_SIDE_EFFECTS,
                        &[],
                    );
                }
                right_type
            }
            _ => self.error_type,
        }
    }

    // Shallow side-effect test for discarded-value positions (comma operator).
    //
    // DEFER(phase-4-checker-later): JSX element nodes. blocked-by: JSX checking.
    // Go: internal/checker/checker.go:Checker.isSideEffectFree(12943)
    fn is_side_effect_free(&self, program: &dyn BoundProgram, expr: NodeId) -> bool {
        let node = skip_parentheses(program, expr);
        let kind = program.arena().kind(node);
        match kind {
            Kind::Identifier
            | Kind::StringLiteral
            | Kind::RegularExpressionLiteral
            | Kind::TaggedTemplateExpression
            | Kind::TemplateExpression
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
            | Kind::UndefinedKeyword
            | Kind::FunctionExpression
            | Kind::ClassExpression
            | Kind::ArrowFunction
            | Kind::ArrayLiteralExpression
            | Kind::ObjectLiteralExpression
            | Kind::TypeOfExpression
            | Kind::NonNullExpression => true,
            Kind::ConditionalExpression => {
                let NodeData::ConditionalExpression(d) = program.arena().data(node) else {
                    return false;
                };
                self.is_side_effect_free(program, d.when_true)
                    && self.is_side_effect_free(program, d.when_false)
            }
            Kind::BinaryExpression => {
                let NodeData::BinaryExpression(d) = program.arena().data(node) else {
                    return false;
                };
                if is_assignment_operator(program.arena().kind(d.operator_token)) {
                    return false;
                }
                self.is_side_effect_free(program, d.left)
                    && self.is_side_effect_free(program, d.right)
            }
            Kind::PrefixUnaryExpression => {
                let NodeData::PrefixUnaryExpression(d) = program.arena().data(node) else {
                    return false;
                };
                matches!(
                    d.operator,
                    Kind::ExclamationToken | Kind::PlusToken | Kind::MinusToken | Kind::TildeToken
                )
            }
            _ => false,
        }
    }

    // Checks an `instanceof` expression (Go's `checkInstanceOfExpression`). The
    // result is always `boolean`.
    //
    // DEFER(phase-4-checker-4ab+): the `Symbol.hasInstance` method path (when the
    // right operand is a plain object with a `[Symbol.hasInstance]` method) and
    // construct-signature detection are deferred. blocked-by: `getResolvedSignature`
    // for the `[Symbol.hasInstance]` call + the global `Symbol` type (lib globals,
    // P6) + construct-signature collection.
    // Go: internal/checker/checker.go:Checker.checkInstanceOfExpression(12979) /
    //     Checker.resolveInstanceofExpression(8763)
    fn check_instanceof_expression(
        &mut self,
        program: &dyn BoundProgram,
        left: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
    ) -> TypeId {
        // The left operand must be `any`, an object type, or a type parameter.
        // A purely primitive left operand reports `2358` (Go skips `any` since a
        // related error was already reported).
        if !self.get_type(left_type).flags().intersects(TypeFlags::ANY)
            && self.all_types_assignable_to_kind(left_type, TypeFlags::PRIMITIVE)
        {
            self.error(
                program,
                left,
                &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_AN_INSTANCEOF_EXPRESSION_MUST_BE_OF_TYPE_ANY_AN_OBJECT_TYPE_OR_A_TYPE_PARAMETER,
                &[],
            );
        }
        // The right operand must be `any`, have a call/construct signature, or be
        // a subtype of the global `Function` interface (Go's
        // `resolveInstanceofExpression` else-branch). The synthetic global
        // `interface Function {}` supplies `globalFunctionType` here.
        if !self.get_type(right_type).flags().intersects(TypeFlags::ANY)
            && !self.type_has_call_or_construct_signatures(right_type)
            && !self.is_type_subtype_of_global_function(program, right_type)
        {
            self.error(
                program,
                right,
                &tsgo_diagnostics::THE_RIGHT_HAND_SIDE_OF_AN_INSTANCEOF_EXPRESSION_MUST_BE_EITHER_OF_TYPE_ANY_A_CLASS_FUNCTION_OR_OTHER_TYPE_ASSIGNABLE_TO_THE_FUNCTION_INTERFACE_TYPE_OR_AN_OBJECT_TYPE_WITH_A_SYMBOL_HASINSTANCE_METHOD,
                &[],
            );
        }
        self.boolean_type
    }

    // Reports whether `t` has at least one call signature (Go's
    // `typeHasCallOrConstructSignatures`, 4ab subset: call signatures only).
    //
    // DEFER(phase-4-checker-later): construct signatures. blocked-by:
    // construct-signature collection on object types.
    // Go: internal/checker/checker.go:Checker.typeHasCallOrConstructSignatures
    fn type_has_call_or_construct_signatures(&self, t: TypeId) -> bool {
        !self.get_signatures_of_type(t).is_empty()
    }

    // Reports whether `t` is a subtype of the (synthetic) global `Function`
    // interface, when one is resolvable from the program globals. Returns `false`
    // when there is no global `Function` (no lib / no synthetic declaration).
    // Go: the `c.isTypeSubtypeOf(rightType, c.globalFunctionType)` clause of
    //     Checker.resolveInstanceofExpression(8784)
    fn is_type_subtype_of_global_function(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> bool {
        match self.get_global_type("Function") {
            Some(global_function) => self.is_type_subtype_of(program, t, global_function),
            None => false,
        }
    }

    // Reports whether every constituent of `source` is assignable to the
    // primitive `kind` (Go's `allTypesAssignableToKind`, 4ab subset): a union
    // requires every member, otherwise a direct flag match decides. This subset
    // (used by the `instanceof` left-operand `2358` guard) checks flag
    // membership, which is exact for the reachable primitive/object types.
    //
    // DEFER(phase-4-checker-later): the full `isTypeAssignableToKind`
    // value-level assignability (e.g. enum-literal / fresh-literal widening).
    // blocked-by: per-kind assignability slices.
    // Go: internal/checker/checker.go:Checker.allTypesAssignableToKind(27440)
    fn all_types_assignable_to_kind(&self, source: TypeId, kind: TypeFlags) -> bool {
        if let Some(members) = self.get_type(source).union_types() {
            return members
                .iter()
                .all(|&member| self.all_types_assignable_to_kind(member, kind));
        }
        self.get_type(source).flags().intersects(kind)
    }

    // Checks an `in` expression (Go's `checkInExpression`). The result is always
    // `boolean`.
    //
    // DEFER(phase-4-checker-4ab+): the private-identifier left operand's
    // `reportNonexistentProperty` / emit-helper path when the field symbol is
    // unresolved. blocked-by: `symbolNodeLinks.resolvedSymbol` wiring.
    //
    // Note: Go checks the operands with `checkTypeAssignableTo(..., nil)`, so a
    // bad operand surfaces as the generic assignability error `2322` (TS-go has
    // no dedicated `in`-operand codes — the legacy `2360`/`2361` are not emitted).
    // Go: internal/checker/checker.go:Checker.checkInExpression(13009)
    fn check_in_expression(
        &mut self,
        program: &dyn BoundProgram,
        left: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
    ) -> TypeId {
        if left_type == self.silent_never_type || right_type == self.silent_never_type {
            return self.silent_never_type;
        }
        if program.arena().kind(left) != Kind::PrivateIdentifier {
            // The left operand must be assignable to `string | number | symbol`.
            let string_number_symbol =
                self.get_union_type(&[self.string_type, self.number_type, self.es_symbol_type]);
            self.check_type_assignable_to_or_error(program, left, left_type, string_number_symbol);
        }
        // The right operand must be assignable to `object` (the non-primitive
        // intrinsic).
        let non_primitive = self.non_primitive_type;
        if self.is_type_assignable_to(program, right_type, non_primitive)
            && self.has_empty_object_intersection(program, right_type)
        {
            let type_str = super::nodebuilder::type_to_string(self, program, right_type);
            self.error(
                program,
                right,
                &tsgo_diagnostics::TYPE_0_MAY_REPRESENT_A_PRIMITIVE_VALUE_WHICH_IS_NOT_PERMITTED_AS_THE_RIGHT_OPERAND_OF_THE_IN_OPERATOR,
                &[type_str.as_str()],
            );
        } else {
            self.check_type_assignable_to_or_error(program, right, right_type, non_primitive);
        }
        self.boolean_type
    }

    // Reports whether any constituent of `t` is the unknown-narrowed `{}` type or
    // an intersection whose base constraint is an empty anonymous object (Go's
    // `hasEmptyObjectIntersection`).
    // Go: internal/checker/checker.go:Checker.hasEmptyObjectIntersection(13043)
    fn has_empty_object_intersection(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> bool {
        self.distributed_types(t).iter().any(|&member| {
            if member == self.unknown_empty_object_type {
                return true;
            }
            if !self.get_type(member).flags().intersects(TypeFlags::INTERSECTION) {
                return false;
            }
            let base = self.get_base_constraint_or_type(program, member);
            self.is_empty_object_type(base)
        })
    }

    // Reports `2322` at `node` when `source` is not assignable to `target`,
    // generalizing a literal source to its base type for the message (the
    // reachable subset of Go's `checkTypeAssignableTo(source, target, node, nil)`
    // with the default head message).
    // Go: internal/checker/checker.go:Checker.checkTypeAssignableTo + reportRelationError
    fn check_type_assignable_to_or_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        source: TypeId,
        target: TypeId,
    ) {
        if self.is_type_assignable_to(program, source, target) {
            return;
        }
        let generalized = self.generalized_source_for_error(source, target);
        let source_str = super::nodebuilder::type_to_string(self, program, generalized);
        let target_str = super::nodebuilder::type_to_string(self, program, target);
        self.error(
            program,
            node,
            &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
            &[source_str.as_str(), target_str.as_str()],
        );
    }

    // Checks a simple assignment `left = right` for assignability (the
    // `KindEqualsToken` arm of Go's `checkAssignmentOperator`): when the
    // left-hand side is a reference, the right-hand type must be assignable to
    // the left-hand type, else `2322`. The error is reported at the LHS, and a
    // literal source is generalized to its base type for the message (Go's
    // `checkTypeAssignableToAndOptionallyElaborate(rightType, leftType, left, ...)`).
    //
    // DEFER(phase-4-checker-4n+): compound assignment operators (using the
    // setter's type), destructuring targets, and `exactOptionalPropertyTypes`
    // elaboration. blocked-by: write-type resolution + destructuring.
    // Go: internal/checker/checker.go:Checker.checkAssignmentOperator(12701)
    fn check_assignment_operator(
        &mut self,
        program: &dyn BoundProgram,
        left: NodeId,
        left_type: TypeId,
        right_type: TypeId,
        expr: Option<NodeId>,
    ) {
        if !self.check_reference_expression(
            program,
            left,
            &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_AN_ASSIGNMENT_EXPRESSION_MUST_BE_A_VARIABLE_OR_A_PROPERTY_ACCESS,
            &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_AN_ASSIGNMENT_EXPRESSION_MAY_NOT_BE_AN_OPTIONAL_PROPERTY_ACCESS,
        ) {
            return;
        }
        if left_type == self.error_type {
            return;
        }
        // Go's `checkTypeAssignableToAndOptionallyElaborate(rightType, leftType,
        // left, right, ...)`: a fresh object/array-literal RHS elaborates onto
        // its offending element first; otherwise the generic chain reports at
        // the LHS. `expr` is `None` for compound assignments, whose result type
        // is not a literal RHS (elaboration DEFER).
        if !self.is_type_assignable_to(program, right_type, left_type) {
            if let Some(expr) = expr {
                if self.elaborate_error(
                    program,
                    expr,
                    right_type,
                    left_type,
                    RelationKind::Assignable,
                ) {
                    return;
                }
            }
            self.report_type_not_assignable(program, left, right_type, left_type);
        }
    }

    // Reports whether `t` is, or contains, a type with flag bits in `kind` (Go's
    // `maybeTypeOfKind`).
    // Go: internal/checker/checker.go:Checker.maybeTypeOfKind(27418)
    fn maybe_type_of_kind(&self, t: TypeId, kind: TypeFlags) -> bool {
        let f = self.get_type(t).flags();
        if f.intersects(kind) {
            return true;
        }
        if f.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let members = self
                .get_type(t)
                .union_types()
                .or_else(|| self.get_type(t).intersection_types())
                .unwrap_or(&[]);
            return members
                .iter()
                .any(|&m| self.maybe_type_of_kind(m, kind));
        }
        false
    }

    // Returns the base constraint of a type parameter, or `t` itself (Go's
    // `getBaseConstraintOrType` reachable subset).
    // Go: internal/checker/checker.go:Checker.getBaseConstraintOrType(27246)
    fn get_base_constraint_or_type(&mut self, program: &dyn BoundProgram, t: TypeId) -> TypeId {
        super::declared_types::get_constraint_of_type_parameter(self, program, t).unwrap_or(t)
    }

    // Reports whether `t` is, or its base constraint is, a type with flag bits
    // in `kind` (Go's `maybeTypeOfKindConsideringBaseConstraint`).
    // Go: internal/checker/checker.go:Checker.maybeTypeOfKindConsideringBaseConstraint(27432)
    fn maybe_type_of_kind_considering_base_constraint(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        kind: TypeFlags,
    ) -> bool {
        if self.maybe_type_of_kind(t, kind) {
            return true;
        }
        let base = self.get_base_constraint_or_type(program, t);
        base != t && self.maybe_type_of_kind(base, kind)
    }

    // Reports whether `t` is a reference to the global `Promise` type.
    // Go: internal/checker/checker.go:Checker.isReferenceToType (Promise arm)
    fn is_reference_to_type(&self, t: TypeId, target: TypeId) -> bool {
        self.get_type(t)
            .as_object()
            .and_then(|o| o.target)
            .is_some_and(|tgt| tgt == target)
    }

    // Returns the "promised" type of a `Promise<T>` reference or thenable (Go's
    // `getPromisedTypeOfPromiseEx` reachable subset: global `Promise` + `.then`
    // callback value parameter).
    // Go: internal/checker/checker.go:Checker.getPromisedTypeOfPromiseEx(28602)
    fn get_promised_type_of_promise(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> Option<TypeId> {
        if self.get_type(t).flags().intersects(TypeFlags::ANY) {
            return None;
        }
        if let Some(promise_type) = self.get_global_promise_type() {
            if self.is_reference_to_type(t, promise_type) {
                return self
                    .get_type(t)
                    .as_object()
                    .and_then(|o| o.resolved_type_arguments.first().copied());
            }
        }
        let base = self.get_base_constraint_or_type(program, t);
        if self.all_types_assignable_to_kind(base, TypeFlags::PRIMITIVE | TypeFlags::NEVER) {
            return None;
        }
        let then_type = self.get_type_of_property_of_type(program, t, "then")?;
        if self.get_type(then_type).flags().intersects(TypeFlags::ANY) {
            return None;
        }
        let then_signatures = self.get_signatures_of_type(then_type);
        if then_signatures.is_empty() {
            return None;
        }
        let onfulfilled_params: Vec<TypeId> = then_signatures
            .iter()
            .map(|&sig| self.get_type_at_position(program, sig, 0))
            .collect();
        let onfulfilled_union = self.get_union_type(&onfulfilled_params);
        let onfulfilled_param =
            self.get_type_with_facts(onfulfilled_union, TypeFacts::NE_UNDEFINED_OR_NULL);
        if self
            .get_type(onfulfilled_param)
            .flags()
            .intersects(TypeFlags::ANY)
        {
            return None;
        }
        let callback_signatures = self.get_signatures_of_type(onfulfilled_param);
        if callback_signatures.is_empty() {
            return None;
        }
        let value_types: Vec<TypeId> = callback_signatures
            .iter()
            .map(|&sig| {
                let value = self.get_type_at_position(program, sig, 0);
                self.instantiate_through_type_reference(t, value)
            })
            .collect();
        Some(self.get_union_type(&value_types))
    }

    // Instantiates `promised` through `source`'s type-argument mapper when
    // `source` is a generic reference (so a callback `(value: T) => void` on
    // `Thenable<number>` yields `number` for `T`).
    fn instantiate_through_type_reference(&mut self, source: TypeId, promised: TypeId) -> TypeId {
        let Some(obj) = self.get_type(source).as_object() else {
            return promised;
        };
        let Some(target) = obj.target else {
            return promised;
        };
        let params = self
            .get_type(target)
            .as_object()
            .map(|o| o.type_parameters.clone())
            .unwrap_or_default();
        let args = obj.resolved_type_arguments.clone();
        if params.is_empty() || params.len() != args.len() {
            return promised;
        }
        let mapper = TypeMapper::Array {
            sources: params,
            targets: args,
        };
        self.instantiate_type(promised, &mapper)
    }

    // Returns the awaited type of `t` without alias expansion (Go's
    // `getAwaitedTypeNoAlias` reachable subset).
    // Go: internal/checker/checker.go:Checker.getAwaitedTypeNoAlias
    fn get_awaited_type_no_alias(&mut self, program: &dyn BoundProgram, t: TypeId) -> TypeId {
        self.get_awaited_type_no_alias_impl(program, t, &mut Vec::new())
    }

    fn get_awaited_type_no_alias_impl(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        stack: &mut Vec<TypeId>,
    ) -> TypeId {
        if self.get_type(t).flags().intersects(TypeFlags::ANY) {
            return t;
        }
        if self.get_type(t).flags().intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = self
                .distributed_types(t)
                .into_iter()
                .map(|m| self.get_awaited_type_no_alias_impl(program, m, stack))
                .collect();
            return self.get_union_type(&members);
        }
        if stack.contains(&t) {
            return t;
        }
        stack.push(t);
        let result = if let Some(promised) = self.get_promised_type_of_promise(program, t) {
            if promised == t {
                t
            } else {
                self.get_awaited_type_no_alias_impl(program, promised, stack)
            }
        } else {
            t
        };
        stack.pop();
        result
    }

    // Generalizes literal operands for operator-error display when their base
    // types are also unrelated (Go's `getBaseTypesIfUnrelated`).
    // Go: internal/checker/checker.go:Checker.getBaseTypesIfUnrelated(12689)
    fn get_base_types_if_unrelated(
        &mut self,
        program: &dyn BoundProgram,
        left: TypeId,
        right: TypeId,
        is_related: impl FnOnce(&mut Self, &dyn BoundProgram, TypeId, TypeId) -> bool,
    ) -> (TypeId, TypeId) {
        let left_base = self.get_base_type_of_literal_type(left);
        let right_base = self.get_base_type_of_literal_type(right);
        if is_related(self, program, left_base, right_base) {
            (left, right)
        } else {
            (left_base, right_base)
        }
    }

    // Returns `true` when no ES-symbol operand error was reported (Go's
    // `checkForDisallowedESSymbolOperand`).
    // Go: internal/checker/checker.go:Checker.checkForDisallowedESSymbolOperand(12756)
    fn check_for_disallowed_es_symbol_operand(
        &mut self,
        program: &dyn BoundProgram,
        left: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
        operator: Kind,
    ) -> bool {
        let offending = if self.maybe_type_of_kind_considering_base_constraint(
            program,
            left_type,
            TypeFlags::ES_SYMBOL_LIKE,
        ) {
            Some(left)
        } else if self.maybe_type_of_kind_considering_base_constraint(
            program,
            right_type,
            TypeFlags::ES_SYMBOL_LIKE,
        ) {
            Some(right)
        } else {
            None
        };
        if let Some(operand) = offending {
            let op = tsgo_scanner::token_to_string(operator);
            self.error(
                program,
                operand,
                &tsgo_diagnostics::THE_0_OPERATOR_CANNOT_BE_APPLIED_TO_TYPE_SYMBOL,
                &[&op],
            );
            return false;
        }
        true
    }

    // Checks that an arithmetic `operand` of type `t` is number-ish (assignable
    // to `number | bigint`), reporting `diagnostic` at the operand otherwise.
    // Returns `true` when no error was reported (Go's `checkArithmeticOperandType`).
    // Go: internal/checker/checker.go:Checker.checkArithmeticOperandType(12743)
    fn check_arithmetic_operand_type(
        &mut self,
        program: &dyn BoundProgram,
        operand: NodeId,
        t: TypeId,
        diagnostic: &'static Message,
        is_await_valid: bool,
    ) -> bool {
        let number_or_bigint = self.number_or_bigint_type;
        if !self.is_type_assignable_to(program, t, number_or_bigint) {
            let awaited = self.get_awaited_type_no_alias(program, t);
            let maybe_missing_await = is_await_valid
                && awaited != t
                && self.is_type_assignable_to(program, awaited, number_or_bigint);
            self.error_and_maybe_suggest_await(
                program,
                operand,
                maybe_missing_await,
                diagnostic,
                &[],
            );
            return false;
        }
        true
    }

    // Reports whether `source` is assignable to a primitive type `kind` in the
    // strict sense Go's `+` arm uses (`isTypeAssignableToKindEx(_, _, true)`,
    // 4p subset covering `STRING_LIKE`/`NUMBER_LIKE`/`BIG_INT_LIKE`): a direct
    // flag match passes; `any`/`unknown`/`void`/`null`/`undefined` never pass in
    // strict mode; otherwise the value-level assignability decides.
    //
    // DEFER(phase-4-checker-4p+): the other kinds (`ESSymbolLike`, `VoidLike`,
    // `BooleanLike`) and the non-strict variant. blocked-by: per-kind slices land
    // with the operators that need them.
    // Go: internal/checker/checker.go:Checker.isTypeAssignableToKind(27453)
    fn is_type_assignable_to_kind(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        kind: TypeFlags,
    ) -> bool {
        self.is_type_assignable_to_kind_ex(program, source, kind, false)
    }

    // Go: internal/checker/checker.go:Checker.isTypeAssignableToKindEx(20196)
    fn is_type_assignable_to_kind_strict(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        kind: TypeFlags,
    ) -> bool {
        self.is_type_assignable_to_kind_ex(program, source, kind, true)
    }

    fn is_type_assignable_to_kind_ex(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        kind: TypeFlags,
        strict: bool,
    ) -> bool {
        let f = self.get_type(source).flags();
        if f.intersects(kind) {
            return true;
        }
        // Strict mode: the top/void/nullable types are not assignable to a
        // primitive kind (Go's `strict` guard).
        if strict && f.intersects(TypeFlags::ANY_OR_UNKNOWN | TypeFlags::VOID | TypeFlags::NULLABLE)
        {
            return false;
        }
        (kind.intersects(TypeFlags::NUMBER_LIKE)
            && self.is_type_assignable_to(program, source, self.number_type))
            || (kind.intersects(TypeFlags::STRING_LIKE)
                && self.is_type_assignable_to(program, source, self.string_type))
            || (kind.intersects(TypeFlags::BIG_INT_LIKE)
                && self.is_type_assignable_to(program, source, self.bigint_type))
            || (kind.intersects(TypeFlags::BOOLEAN_LIKE)
                && self.is_type_assignable_to(program, source, self.boolean_type))
    }

    // Go: internal/checker/checker.go:Checker.bothAreBigIntLike(12727)
    fn both_are_big_int_like(
        &mut self,
        program: &dyn BoundProgram,
        left: TypeId,
        right: TypeId,
    ) -> bool {
        self.is_type_assignable_to_kind(program, left, TypeFlags::BIG_INT_LIKE)
            && self.is_type_assignable_to_kind(program, right, TypeFlags::BIG_INT_LIKE)
    }

    // When the right-hand side of a shift is a constant >= 32, reports that the
    // shift is equivalent to shifting by the remainder mod 32. In enum members
    // this is an error; elsewhere it is a suggestion.
    // Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12347)
    fn check_shift_simplification(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        operator: Kind,
        _operator_token: NodeId,
        left: NodeId,
        right: NodeId,
    ) {
        if !matches!(
            operator,
            Kind::LessThanLessThanToken
                | Kind::LessThanLessThanEqualsToken
                | Kind::GreaterThanGreaterThanToken
                | Kind::GreaterThanGreaterThanEqualsToken
                | Kind::GreaterThanGreaterThanGreaterThanToken
                | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
        ) {
            return;
        }
        use tsgo_evaluator::{new_evaluator, new_result, EvalValue, OuterExpressionKinds};
        let evaluator = new_evaluator(
            |_arena, _expr, _loc| new_result(EvalValue::None, false, false, false),
            OuterExpressionKinds::NONE,
        );
        let rhs_eval = evaluator.evaluate(program.arena(), right, Some(right));
        let EvalValue::Num(num_value) = rhs_eval.value else {
            return;
        };
        if num_value.abs() < tsgo_jsnum::Number::from(32.0) {
            return;
        }
        let simplified = num_value.remainder(tsgo_jsnum::Number::from(32.0));
        let simplified_text = shift_amount_display(simplified);
        let is_enum_member = program
            .arena()
            .parent(right)
            .and_then(|parent| program.arena().parent(parent))
            .is_some_and(|grandparent| program.arena().kind(grandparent) == Kind::EnumMember);
        let left_text = program.arena().text(left);
        let op = tsgo_scanner::token_to_string(operator);
        self.add_error_or_suggestion(
            program,
            is_enum_member,
            node,
            &tsgo_diagnostics::THIS_OPERATION_CAN_BE_SIMPLIFIED_THIS_SHIFT_IS_IDENTICAL_TO_0_1_2,
            &[left_text, op, simplified_text.as_str()],
        );
    }

    // Reports whether the (already base-typed) operands of a relational
    // comparison are comparable (the `isRelated` predicate of Go's relational
    // arm): `any` on either side passes; otherwise both must be number-ish, or
    // neither number-ish and the two types comparable.
    //
    // DEFER(phase-4-checker-4o+): the disallowed-ES-symbol-operand guard
    // (`checkForDisallowedESSymbolOperand`) and `await`-suggestion path.
    // blocked-by: ES-symbol-operand diagnostics + awaited-type (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (relational isRelated)
    fn relational_operands_comparable(
        &mut self,
        program: &dyn BoundProgram,
        left: TypeId,
        right: TypeId,
    ) -> bool {
        if self.get_type(left).flags().intersects(TypeFlags::ANY)
            || self.get_type(right).flags().intersects(TypeFlags::ANY)
        {
            return true;
        }
        let number_or_bigint = self.number_or_bigint_type;
        let left_numeric = self.is_type_assignable_to(program, left, number_or_bigint);
        let right_numeric = self.is_type_assignable_to(program, right, number_or_bigint);
        (left_numeric && right_numeric)
            || (!left_numeric && !right_numeric && self.are_types_comparable(program, left, right))
    }

    // Reports whether `a` and `b` are comparable in either direction (Go's
    // `areTypesComparable`).
    // Go: internal/checker/relater.go:Checker.areTypesComparable(166)
    pub(crate) fn are_types_comparable(
        &mut self,
        program: &dyn BoundProgram,
        a: TypeId,
        b: TypeId,
    ) -> bool {
        self.is_type_comparable_to(program, a, b) || self.is_type_comparable_to(program, b, a)
    }

    // Reports whether an equality comparison's operands are comparable in either
    // direction (the `isRelated` predicate of Go's equality arm).
    // Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (equality isRelated)
    fn equality_operands_comparable(
        &mut self,
        program: &dyn BoundProgram,
        left: TypeId,
        right: TypeId,
    ) -> bool {
        self.is_type_equality_comparable_to(program, left, right)
            || self.is_type_equality_comparable_to(program, right, left)
    }

    // Reports whether `source` is equality-comparable to `target`: a nullable
    // target always passes, else the comparable relation decides (Go's
    // `isTypeEqualityComparableTo`).
    // Go: internal/checker/checker.go:Checker.isTypeEqualityComparableTo(12805)
    fn is_type_equality_comparable_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.get_type(target)
            .flags()
            .intersects(TypeFlags::NULLABLE)
            || self.is_type_comparable_to(program, source, target)
    }

    // Returns the base type of a literal type for comparison contexts (Go's
    // `getBaseTypeOfLiteralTypeForComparison`, 4o subset): string-likes widen to
    // `string`, numeric literals/enums to `number`, bigint literals to `bigint`,
    // boolean literals to `boolean`, and unions map member-wise.
    // Go: internal/checker/checker.go:Checker.getBaseTypeOfLiteralTypeForComparison(25313)
    fn get_base_type_of_literal_type_for_comparison(&mut self, t: TypeId) -> TypeId {
        let f = self.get_type(t).flags();
        if f.intersects(
            TypeFlags::STRING_LITERAL | TypeFlags::TEMPLATE_LITERAL | TypeFlags::STRING_MAPPING,
        ) {
            return self.string_type;
        }
        if f.intersects(TypeFlags::NUMBER_LITERAL | TypeFlags::ENUM) {
            return self.number_type;
        }
        if f.intersects(TypeFlags::BIG_INT_LITERAL) {
            return self.bigint_type;
        }
        if f.intersects(TypeFlags::BOOLEAN_LITERAL) {
            return self.boolean_type;
        }
        if f.contains(TypeFlags::UNION) {
            let members = self
                .get_type(t)
                .union_types()
                .map(<[TypeId]>::to_vec)
                .unwrap_or_default();
            let mut mapped = Vec::with_capacity(members.len());
            for member in members {
                mapped.push(self.get_base_type_of_literal_type_for_comparison(member));
            }
            return self.get_union_type(&mapped);
        }
        t
    }

    // Reports an incompatible-binary-operator error at `node` (Go's
    // `reportOperatorError`, 4o subset): equality operators use the "no overlap"
    // message (`2367`); the rest use "Operator '{0}' cannot be applied" (`2365`).
    //
    // DEFER(phase-4-checker-4o+): the equal-printed-name fully-qualified fallback.
    // Go: internal/checker/checker.go:Checker.reportOperatorError(12662)
    fn report_binary_operator_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        operator_token: NodeId,
        left: TypeId,
        right: TypeId,
        would_work_with_await: bool,
    ) {
        let left_str = super::nodebuilder::type_to_string(self, program, left);
        let right_str = super::nodebuilder::type_to_string(self, program, right);
        let operator = program.arena().kind(operator_token);
        match operator {
            Kind::EqualsEqualsToken
            | Kind::ExclamationEqualsToken
            | Kind::EqualsEqualsEqualsToken
            | Kind::ExclamationEqualsEqualsToken => {
                self.error_and_maybe_suggest_await(
                    program,
                    node,
                    would_work_with_await,
                    &tsgo_diagnostics::THIS_COMPARISON_APPEARS_TO_BE_UNINTENTIONAL_BECAUSE_THE_TYPES_0_AND_1_HAVE_NO_OVERLAP,
                    &[left_str.as_str(), right_str.as_str()],
                );
            }
            _ => {
                let op = tsgo_scanner::token_to_string(operator);
                self.error_and_maybe_suggest_await(
                    program,
                    node,
                    would_work_with_await,
                    &tsgo_diagnostics::OPERATOR_0_CANNOT_BE_APPLIED_TO_TYPES_1_AND_2,
                    &[op, left_str.as_str(), right_str.as_str()],
                );
            }
        }
    }

    // Checks a call expression `f(args)` (Go's `checkCallExpression` ->
    // `resolveCallExpression` -> `resolveCall`): resolves the callee type's call
    // signatures, then (for the single non-generic candidate 4q handles) checks
    // each argument against its parameter, reporting `2345` for a non-assignable
    // argument.
    //
    // 4r adds overload resolution (the multi-call-signature path via
    // `resolve_overloaded_call`).
    //
    // DEFER(phase-4-checker-4r+): the overload best-match selection
    // (`getCandidateForOverloadFailure`) and per-overload elaboration chain, the
    // two-pass subtype/assignable relations, generic call-site inference (full),
    // rest/spread arguments, `this`-argument checking, contextual typing of
    // callback arguments, `new` expressions, and the not-callable/untyped-call
    // invocation diagnostics (an `any`/error callee or one with no call
    // signatures). blocked-by: diagnostic message chains, inference contexts,
    // tuple/spread types, `this`-type resolution, contextual typing, construct
    // signatures, and `getApparentType`/lib globals (P6).
    // Go: internal/checker/checker.go:Checker.checkCallExpression(8289)
    fn check_call_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (callee, args, type_argument_nodes, type_arguments_ref) =
            match program.arena().data(node) {
                NodeData::CallExpression(d) => (
                    d.expression,
                    d.arguments.nodes.clone(),
                    d.type_arguments.as_ref().map(|l| l.nodes.clone()),
                    d.type_arguments.as_ref(),
                ),
                _ => return self.error_type,
            };
        self.check_grammar_type_arguments(program, node, type_arguments_ref);
        let func_type = self.check_expression(program, callee);
        // Go's `resolveCallExpression` types the callee through
        // `checkNonNullTypeWithReporter` with the cannot-invoke reporter: a
        // possibly-`null`/`undefined` callee reports `2721`/`2722`/`2723`, then
        // the call resolves on the narrowed non-null type.
        let func_type = self.check_non_null_type_with_reporter(
            program,
            func_type,
            callee,
            NonNullReporter::Invocation,
        );
        let signatures = self.get_signatures_of_type(func_type);
        let Some(&signature) = signatures.first() else {
            // No call signatures (e.g. an `any`/error callee or a non-callable
            // value). Still check the argument expressions so nested diagnostics
            // surface, then report the invocation error (Go's
            // `resolveCallExpression` -> `invocationError` / the construct-only
            // `2348` hint).
            for &arg in &args {
                self.check_expression(program, arg);
            }
            if self.is_constructor_type(program, func_type) {
                let type_str = self
                    .get_type(func_type)
                    .symbol
                    .filter(|&sym| program.symbol(sym).flags.contains(SymbolFlags::CLASS))
                    .map(|sym| {
                        format!(
                            "typeof {}",
                            super::nodebuilder::symbol_to_string(self, program, sym)
                        )
                    })
                    .unwrap_or_else(|| {
                        super::nodebuilder::type_to_string(self, program, func_type)
                    });
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::VALUE_OF_TYPE_0_IS_NOT_CALLABLE_DID_YOU_MEAN_TO_INCLUDE_NEW,
                    &[type_str.as_str()],
                );
            } else if func_type != self.error_type {
                self.invocation_error(program, callee, func_type, InvocationKind::Call);
            }
            return self.error_type;
        };
        // With more than one call signature the callee is overloaded; 4r mirrors
        // `resolveCall` -> `chooseOverload`: pick the first applicable signature,
        // else report the overload-resolution error (`No_overload_matches_this_call`
        // 2769, or the overload arity error 2575).
        if signatures.len() > 1 {
            return self.resolve_overloaded_call(program, node, &signatures, &args);
        }
        // Explicit type arguments (`f<number>(x)`) instantiate the generic
        // signature directly (Go's `resolveCall` -> `chooseOverload`, the
        // user-supplied-type-arguments path), so the parameter and return types
        // are the substituted types. A wrong type-argument count reports `2558`
        // (`getTypeArgumentArityError`) and aborts to the error type, while still
        // checking the argument expressions so nested diagnostics surface.
        let signature =
            if let Some(type_arg_nodes) = type_argument_nodes.as_ref().filter(|n| !n.is_empty()) {
                match self.resolve_explicit_type_argument_signature(
                    program,
                    node,
                    signature,
                    type_arg_nodes,
                ) {
                    Some(instantiated) => instantiated,
                    None => {
                        for &arg in &args {
                            self.check_expression(program, arg);
                        }
                        return self.error_type;
                    }
                }
            } else if !self.signature(signature).type_parameters.is_empty() {
                // A generic call WITHOUT explicit type arguments: infer the type
                // arguments from the call arguments, then instantiate the
                // signature so the parameter and return types are the substituted
                // types (Go's `resolveCall` -> `chooseOverload` inference branch).
                self.resolve_inferred_type_argument_signature(program, node, signature, &args)
            } else {
                signature
            };
        // 4q resolves the single candidate: a correct-arity call has each
        // argument checked for assignability (`2345`); an incorrect-arity call
        // reports `2554` after still checking the argument expressions so nested
        // diagnostics surface.
        if self.has_correct_arity(signature, args.len()) {
            self.check_applicable_signature_for_call(program, signature, &args);
        } else {
            for &arg in &args {
                self.check_expression(program, arg);
            }
            self.report_argument_arity_error(program, node, signature, &args);
        }
        // The call's result type is the resolved signature's return type (Go's
        // `getReturnTypeOfSignature`). An explicitly-instantiated signature has
        // its type parameters erased and its return type already substituted, so
        // this returns that type directly; for a bare (non-generic) signature it
        // is the declared return type.
        self.get_return_type_of_call(program, signature, &[], &[])
    }

    // Checks a tagged template expression `` tag`...` `` (Go's
    // `checkTaggedTemplateExpression` -> `resolveTaggedTemplateExpression` ->
    // `resolveCall`): resolves the tag's call signatures against the effective
    // arguments (a synthetic `TemplateStringsArray` plus each template-span
    // expression).
    //
    // DEFER(phase-4-checker-4r+): the real `TemplateStringsArray` global type,
    // generic call-site inference, explicit type arguments, untyped-function-call
    // handling, and the missing-comma array-literal hint. blocked-by: lib globals
    // (P6) + inference contexts + untyped-call detection.
    // Go: internal/checker/checker.go:Checker.checkTaggedTemplateExpression(9994)
    fn check_tagged_template_expression(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        let (tag, template, type_arguments_ref) = match program.arena().data(node) {
            NodeData::TaggedTemplateExpression(d) => (
                d.tag,
                d.template,
                d.type_arguments.as_ref(),
            ),
            _ => return self.error_type,
        };
        self.check_grammar_type_arguments(program, node, type_arguments_ref);
        let tag_type = self.check_expression(program, tag);
        let tag_type = self.check_non_null_type_with_reporter(
            program,
            tag_type,
            tag,
            NonNullReporter::Invocation,
        );
        self.check_tagged_template_literal(program, template);
        let substitutions = self.tagged_template_substitution_expressions(program, template);
        let signatures = self.get_signatures_of_type(tag_type);
        if signatures.is_empty() {
            self.invocation_error(program, tag, tag_type, InvocationKind::Call);
            return self.error_type;
        }
        if signatures.len() > 1 {
            return self.resolve_overloaded_tagged_template(
                program,
                node,
                tag,
                &signatures,
                template,
                &substitutions,
            );
        }
        let signature = signatures[0];
        let effective_arg_count = 1 + substitutions.len();
        if self.has_correct_arity(signature, effective_arg_count) {
            self.check_applicable_signature_for_tagged_template(
                program,
                signature,
                template,
                &substitutions,
            );
        } else {
            let effective_args = self.tagged_template_effective_argument_nodes(
                template,
                &substitutions,
            );
            self.report_argument_arity_error(program, node, signature, &effective_args);
        }
        self.get_return_type_of_call(program, signature, &[], &[])
    }

    // Type-checks the template literal of a tagged template (the span expressions
    // and any nested template parts).
    fn check_tagged_template_literal(&mut self, program: &dyn BoundProgram, template: NodeId) {
        match program.arena().data(template) {
            NodeData::TemplateExpression(d) => {
                for &span in &d.template_spans.nodes {
                    let NodeData::TemplateSpan(span_data) = program.arena().data(span) else {
                        continue;
                    };
                    self.check_expression(program, span_data.expression);
                }
            }
            NodeData::NoSubstitutionTemplateLiteral(_) => {}
            _ => {
                self.check_expression(program, template);
            }
        }
    }

    // Returns the template-span expression nodes of a tagged template's
    // template literal (Go's `getEffectiveCallArguments` tail).
    fn tagged_template_substitution_expressions(
        &self,
        program: &dyn BoundProgram,
        template: NodeId,
    ) -> Vec<NodeId> {
        let NodeData::TemplateExpression(d) = program.arena().data(template) else {
            return Vec::new();
        };
        d.template_spans
            .nodes
            .iter()
            .filter_map(|&span| {
                let NodeData::TemplateSpan(span_data) = program.arena().data(span) else {
                    return None;
                };
                Some(span_data.expression)
            })
            .collect()
    }

    // Builds the effective argument node list for arity-error reporting: the
    // template literal stands in for the synthetic `TemplateStringsArray`
    // argument, followed by each substitution expression.
    fn tagged_template_effective_argument_nodes(
        &self,
        template: NodeId,
        substitutions: &[NodeId],
    ) -> Vec<NodeId> {
        let mut nodes = Vec::with_capacity(1 + substitutions.len());
        nodes.push(template);
        nodes.extend_from_slice(substitutions);
        nodes
    }

    // Checks that each substitution expression is assignable to its parameter,
    // after the synthetic `TemplateStringsArray` first argument (Go's
    // `isSignatureApplicable` for tagged templates). The first parameter, when
    // present, is checked against `any` as a stand-in for
    // `TemplateStringsArray` until lib globals are wired (P6).
    fn check_applicable_signature_for_tagged_template(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        template: NodeId,
        substitutions: &[NodeId],
    ) {
        if self.get_parameter_count(signature) > 0 {
            let param_type = self.get_type_at_position(program, signature, 0);
            if !self.is_type_assignable_to(program, self.any_type, param_type) {
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                self.error(
                    program,
                    template,
                    &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1,
                    &["any", target_str.as_str()],
                );
                return;
            }
        }
        for (i, &expr) in substitutions.iter().enumerate() {
            let param_idx = i + 1;
            if param_idx >= self.get_parameter_count(signature) {
                break;
            }
            let arg_type = self.check_expression(program, expr);
            let param_type = self.get_type_at_position(program, signature, param_idx);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                let generalized = self.generalized_source_for_error(arg_type, param_type);
                let source_str = super::nodebuilder::type_to_string(self, program, generalized);
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                self.error(
                    program,
                    expr,
                    &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1,
                    &[source_str.as_str(), target_str.as_str()],
                );
                return;
            }
        }
    }

    // Resolves an overloaded tagged template (Go's `resolveCall` overload path
    // with `getEffectiveCallArguments`).
    // Go: internal/checker/checker.go:Checker.resolveTaggedTemplateExpression(8686)
    fn resolve_overloaded_tagged_template(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        tag: NodeId,
        signatures: &[SignatureId],
        template: NodeId,
        substitutions: &[NodeId],
    ) -> TypeId {
        let effective_arg_count = 1 + substitutions.len();
        let arg_types: Vec<TypeId> = substitutions
            .iter()
            .map(|&arg| self.check_expression(program, arg))
            .collect();
        let mut arity_matched: Vec<SignatureId> = Vec::new();
        for &signature in signatures {
            if !self.has_correct_arity(signature, effective_arg_count) {
                continue;
            }
            if self.signature_applicable_for_tagged_template(
                program,
                signature,
                &arg_types,
            ) {
                return self.get_return_type_of_call(program, signature, &[], &[]);
            }
            arity_matched.push(signature);
        }
        let effective_args =
            self.tagged_template_effective_argument_nodes(template, substitutions);
        match arity_matched.len() {
            0 => self.report_overload_arity_error(program, node, signatures, &effective_args),
            1 => self.report_inapplicable_tagged_template_argument(
                program,
                arity_matched[0],
                template,
                substitutions,
                &arg_types,
            ),
            _ => {
                let last = *arity_matched.last().unwrap();
                if let Some((arg_node, source_str, target_str)) =
                    self.first_failing_tagged_template_argument(
                        program,
                        last,
                        template,
                        substitutions,
                        &arg_types,
                    )
                {
                    self.report_no_overload_matches(program, arg_node, &source_str, &target_str);
                } else {
                    let error_node = tagged_template_error_node(program, node, tag);
                    self.error(
                        program,
                        error_node,
                        &tsgo_diagnostics::NO_OVERLOAD_MATCHES_THIS_CALL,
                        &[],
                    );
                }
            }
        }
        match signatures.last() {
            Some(&last) => self.get_return_type_of_call(program, last, &[], &[]),
            None => self.error_type,
        }
    }

    // Silent applicability check for tagged-template overload resolution.
    fn signature_applicable_for_tagged_template(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        substitution_types: &[TypeId],
    ) -> bool {
        if self.get_parameter_count(signature) > 0 {
            let param_type = self.get_type_at_position(program, signature, 0);
            if !self.is_type_assignable_to(program, self.any_type, param_type) {
                return false;
            }
        }
        for (i, &arg_type) in substitution_types.iter().enumerate() {
            let param_idx = i + 1;
            if param_idx >= self.get_parameter_count(signature) {
                break;
            }
            let param_type = self.get_type_at_position(program, signature, param_idx);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                return false;
            }
        }
        true
    }

    fn report_inapplicable_tagged_template_argument(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        template: NodeId,
        substitutions: &[NodeId],
        substitution_types: &[TypeId],
    ) {
        if self.get_parameter_count(signature) > 0 {
            let param_type = self.get_type_at_position(program, signature, 0);
            if !self.is_type_assignable_to(program, self.any_type, param_type) {
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                self.error(
                    program,
                    template,
                    &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1,
                    &["any", target_str.as_str()],
                );
                return;
            }
        }
        for (i, &arg_type) in substitution_types.iter().enumerate() {
            let param_idx = i + 1;
            if param_idx >= self.get_parameter_count(signature) {
                break;
            }
            let param_type = self.get_type_at_position(program, signature, param_idx);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                let generalized = self.generalized_source_for_error(arg_type, param_type);
                let source_str = super::nodebuilder::type_to_string(self, program, generalized);
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                self.error(
                    program,
                    substitutions[i],
                    &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1,
                    &[source_str.as_str(), target_str.as_str()],
                );
                return;
            }
        }
    }

    fn first_failing_tagged_template_argument(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        template: NodeId,
        substitutions: &[NodeId],
        substitution_types: &[TypeId],
    ) -> Option<(NodeId, String, String)> {
        if self.get_parameter_count(signature) > 0 {
            let param_type = self.get_type_at_position(program, signature, 0);
            if !self.is_type_assignable_to(program, self.any_type, param_type) {
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                return Some((template, "any".to_string(), target_str));
            }
        }
        for (i, &arg_type) in substitution_types.iter().enumerate() {
            let param_idx = i + 1;
            if param_idx >= self.get_parameter_count(signature) {
                break;
            }
            let param_type = self.get_type_at_position(program, signature, param_idx);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                let generalized = self.generalized_source_for_error(arg_type, param_type);
                let source_str = super::nodebuilder::type_to_string(self, program, generalized);
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                return Some((substitutions[i], source_str, target_str));
            }
        }
        None
    }

    // Instantiates a generic signature with the call's explicit type arguments
    // (Go's `resolveCall` user-supplied-type-arguments path:
    // `hasCorrectTypeArgumentArity` -> `checkTypeArguments` ->
    // `getSignatureInstantiation`). Returns `None` (after reporting `2558`) when
    // the type-argument count does not match the signature's arity.
    //
    // DEFER(phase-4-checker-C-B2+): the overloaded-call type-argument path
    // (multiple candidates) and the inferred-type-parameter return-signature
    // re-instantiation. blocked-by: overload resolution + nested generic return
    // signatures.
    // Go: internal/checker/checker.go:Checker.resolveCall (typeArguments branch)
    fn resolve_explicit_type_argument_signature(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signature: SignatureId,
        type_argument_nodes: &[NodeId],
    ) -> Option<SignatureId> {
        if !self.has_correct_type_argument_arity(program, signature, type_argument_nodes.len()) {
            self.report_type_argument_arity_error(program, node, signature, type_argument_nodes);
            return None;
        }
        let type_arguments: Vec<TypeId> = type_argument_nodes
            .iter()
            .map(|&n| get_type_from_type_node(self, program, n, None))
            .collect();
        // Check each explicit type argument against its (instantiated) constraint
        // (`2344`), mirroring Go's `checkTypeArguments`. A failing constraint
        // makes the signature inapplicable (Go returns `nil`), so the call
        // aborts to the error type without a follow-on `2345`.
        if !self.check_type_arguments(program, signature, type_argument_nodes, &type_arguments) {
            return None;
        }
        Some(self.get_signature_instantiation(program, signature, &type_arguments))
    }

    // Reports whether the supplied type-argument count matches the signature's
    // type-parameter arity (Go's `hasCorrectTypeArgumentArity`): zero arguments
    // is always allowed (inference), otherwise the count must be within
    // `[minTypeArgumentCount, len(typeParameters)]`.
    // Go: internal/checker/checker.go:Checker.hasCorrectTypeArgumentArity
    fn has_correct_type_argument_arity(
        &self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        type_argument_count: usize,
    ) -> bool {
        let type_parameters = self.signature(signature).type_parameters.clone();
        if type_argument_count == 0 {
            return true;
        }
        let min = get_min_type_argument_count(self, program, &type_parameters);
        type_argument_count >= min && type_argument_count <= type_parameters.len()
    }

    // Reports a wrong type-argument-count error (`2558` for a single signature),
    // spanning the type-argument list (Go's `getTypeArgumentArityError` ->
    // `node.TypeArgumentList().Loc`). `expected` is the minimum count, or
    // `"min-max"` when defaults make the count a range.
    //
    // DEFER(phase-4-checker-C-B2): the overloaded-call variant (`2769`-style
    // "No overload expects N type arguments"). blocked-by: overload resolution.
    // Go: internal/checker/checker.go:Checker.getTypeArgumentArityError (len==1)
    fn report_type_argument_arity_error(
        &mut self,
        program: &dyn BoundProgram,
        _node: NodeId,
        signature: SignatureId,
        type_argument_nodes: &[NodeId],
    ) {
        let type_parameters = self.signature(signature).type_parameters.clone();
        let min = get_min_type_argument_count(self, program, &type_parameters);
        let max = type_parameters.len();
        let expected = if min < max {
            format!("{min}-{max}")
        } else {
            min.to_string()
        };
        let arg_count = type_argument_nodes.len().to_string();
        // Span the type-argument list (Go: node.TypeArgumentList().Loc).
        let first = type_argument_nodes[0];
        let last = *type_argument_nodes.last().unwrap();
        let start = program.arena().loc(first).pos();
        let end = program.arena().loc(last).end();
        let message = &tsgo_diagnostics::EXPECTED_0_TYPE_ARGUMENTS_BUT_GOT_1;
        let diagnostic = Diagnostic {
            code: message.code(),
            category: message.category(),
            message: tsgo_diagnostics::format(&message.to_string(), &[&expected, &arg_count]),
            start,
            length: end - start,
            related_information: Vec::new(),
            message_chain: Vec::new(),
        };
        self.add_diagnostic(program, diagnostic);
    }

    // Checks each explicit type argument against its (instantiated) constraint,
    // reporting `2344` "Type 'X' does not satisfy the constraint 'Y'." on the
    // offending argument node (Go's `checkTypeArguments`). The constraint is
    // instantiated through the `type parameters -> filled type arguments` mapper
    // so a constraint that references an earlier parameter (`<T, U extends T>`)
    // resolves.
    //
    // DEFER(phase-4-checker-C-C): the `getTypeWithThisArgument` constraint
    // adjustment and the head-message chaining. blocked-by: `this`-type
    // instantiation + diagnostic-chain head messages.
    // Go: internal/checker/checker.go:Checker.checkTypeArguments
    pub(crate) fn check_type_arguments(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        type_argument_nodes: &[NodeId],
        type_arguments: &[TypeId],
    ) -> bool {
        let type_parameters = self.signature(signature).type_parameters.clone();
        if type_parameters.is_empty() {
            return true;
        }
        let filled = fill_missing_type_arguments(self, program, type_arguments, &type_parameters);
        let mapper = TypeMapper::new(&type_parameters, &filled);
        for (i, &arg_node) in type_argument_nodes.iter().enumerate() {
            if i >= type_parameters.len() {
                break;
            }
            let Some(constraint) =
                get_constraint_of_type_parameter(self, program, type_parameters[i])
            else {
                continue;
            };
            let instantiated_constraint = self.instantiate_type(constraint, &mapper);
            let argument = filled[i];
            if !self.is_type_assignable_to(program, argument, instantiated_constraint) {
                let arg_str = super::nodebuilder::type_to_string(self, program, argument);
                let constraint_str =
                    super::nodebuilder::type_to_string(self, program, instantiated_constraint);
                self.error(
                    program,
                    arg_node,
                    &tsgo_diagnostics::TYPE_0_DOES_NOT_SATISFY_THE_CONSTRAINT_1,
                    &[arg_str.as_str(), constraint_str.as_str()],
                );
                // Go's `checkTypeArguments` returns `nil` at the first failing
                // constraint, so the signature is inapplicable and no follow-on
                // argument error is reported.
                return false;
            }
        }
        true
    }

    // Instantiates a generic signature with explicit (or defaulted) type
    // arguments, erasing its type parameters (Go's `getSignatureInstantiation`
    // -> `createSignatureInstantiation` with `eraseTypeParameters`). Missing
    // trailing arguments are filled from defaults.
    // Go: internal/checker/checker.go:Checker.getSignatureInstantiation
    fn get_signature_instantiation(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        type_arguments: &[TypeId],
    ) -> SignatureId {
        let type_parameters = self.signature(signature).type_parameters.clone();
        let filled = fill_missing_type_arguments(self, program, type_arguments, &type_parameters);
        let mapper = TypeMapper::new(&type_parameters, &filled);
        let instantiated = self.instantiate_signature(signature, &mapper);
        // `createSignatureInstantiation` erases the instantiation's own type
        // parameters, so the result is a concrete (non-generic) signature.
        self.signatures
            .get_mut(instantiated)
            .type_parameters
            .clear();
        instantiated
    }

    // Infers a generic signature's type arguments from the call arguments and
    // returns the instantiated (concrete) signature, memoizing it on the call
    // node so a context-sensitive argument's contextual typing sees the
    // instantiated parameter types (Go's `resolveCall` -> `chooseOverload`
    // inference branch: `inferTypeArguments` -> `getSignatureInstantiation`, with
    // the chosen signature stored on `signatureLinks[node].resolvedSignature`).
    //
    // Go: internal/checker/checker.go:Checker.resolveCall (inference branch)
    fn resolve_inferred_type_argument_signature(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signature: SignatureId,
        args: &[NodeId],
    ) -> SignatureId {
        let inferred = self.infer_type_arguments_for_call(program, node, signature, args);
        let instantiated = self.get_signature_instantiation(program, signature, &inferred);
        self.resolved_signatures.insert(node, instantiated);
        instantiated
    }

    // Infers the type arguments of a generic `signature` from the call `args`
    // (Go's `inferTypeArguments`, reachable subset). Three phases mirror Go:
    //
    // 1. Contextual-return inference: when the call has a contextual type (e.g.
    //    `const xs: number[] = make()`), infer from that contextual type to the
    //    signature's (generic) return type, at the lower `RETURN_TYPE` priority
    //    so argument inferences override it (Go's leading `getContextualType` /
    //    `inferTypes(..., InferencePriorityReturnType)` block).
    // 2. Non-context-sensitive arguments: each is typed and inferred against its
    //    (generic) parameter type, fixing the type variables it mentions.
    // 3. Context-sensitive arguments (arrows/functions): each is contextually
    //    typed by its parameter type *instantiated with the inferences made so
    //    far* (Go's lazy inference `TypeMapper`), so its un-annotated parameters
    //    take the fixed type (e.g. `x: number`); its checked function type is
    //    then matched against the (generic) parameter type to infer the
    //    callback's result (the `U` in `map<T,U>(a: T[], f: (x:T)=>U)`).
    //
    // The accumulated candidates are then resolved (`getInferredTypes`).
    //
    // DEFER(phase-4-checker-C-C): the `this`-argument inference, rest/spread
    // argument aggregation, the precise `isContextSensitive` test (a
    // fully-annotated function is not context-sensitive), the
    // `outerMapper`/`returnMapper` machinery for nested generic contextual
    // signatures, and intra-expression inference sites. blocked-by: `this`/
    // rest/tuple types + outer-inference threading + literal-element inference.
    // Go: internal/checker/checker.go:Checker.inferTypeArguments
    fn infer_type_arguments_for_call(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signature: SignatureId,
        args: &[NodeId],
    ) -> Vec<TypeId> {
        let type_parameters = self.signature(signature).type_parameters.clone();
        let mut context = InferenceContext::new(&type_parameters);
        // Phase 1: contextual-return inference (lower priority than arguments).
        self.infer_from_contextual_return_type(program, node, signature, &mut context);
        let count = self.get_parameter_count(signature).min(args.len());
        // Phase 2: non-context-sensitive arguments.
        for (i, &arg) in args.iter().enumerate().take(count) {
            if is_context_sensitive_argument(program, arg) {
                continue;
            }
            let param_type = self.get_type_at_position(program, signature, i);
            let arg_type = self.check_expression_for_inference(program, arg);
            self.infer_types(program, &mut context.inferences, arg_type, param_type);
        }
        // Phase 3: context-sensitive arguments (callbacks), now that the other
        // arguments have fixed the type variables their parameters depend on.
        for (i, &arg) in args.iter().enumerate().take(count) {
            if !is_context_sensitive_argument(program, arg) {
                continue;
            }
            let param_type = self.get_type_at_position(program, signature, i);
            let arg_type =
                self.infer_from_context_sensitive_argument(program, arg, param_type, &mut context);
            self.infer_types(program, &mut context.inferences, arg_type, param_type);
        }
        self.get_inferred_types_for_call(program, &mut context)
    }

    // Phase 1 of `inferTypeArguments`: when the call expression has a contextual
    // type, infer from it to the signature's (generic) return type at the
    // `RETURN_TYPE` priority. This lets `const xs: number[] = make()` (where
    // `make<T>(): T[]`) infer `T = number` from the annotation, with no
    // arguments to infer from. Argument inferences (priority `NONE`) override
    // these, so `id(1)` still infers `T = 1` even under `const s: string = id(1)`.
    //
    // DEFER(phase-4-checker-C-C): the binding-pattern contextual type, the
    // `outerMapper`/`returnMapper` instantiation of a generic contextual
    // signature, and the `couldContainTypeVariables` object-flag cache.
    // blocked-by: binding-pattern typing + outer-inference threading.
    // Go: internal/checker/checker.go:Checker.inferTypeArguments (contextual return block)
    fn infer_from_contextual_return_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signature: SignatureId,
        context: &mut InferenceContext,
    ) {
        let return_type = self.get_return_type_of_signature(signature);
        if !self.could_contain_type_variables(return_type) {
            return;
        }
        let Some(contextual_type) = self.get_contextual_type(program, node, ContextFlags::NONE)
        else {
            return;
        };
        self.infer_types_with_priority(
            program,
            &mut context.inferences,
            contextual_type,
            return_type,
            InferencePriority::RETURN_TYPE,
        );
    }

    // Phase 3 helper: contextually types a context-sensitive argument (an
    // arrow/function expression) with `param_type` instantiated through the
    // inferences made so far, then returns its checked function type so the
    // caller can infer the callback's result. Mirrors the body of Go's
    // `inferTypeArguments` argument loop for a context-sensitive argument:
    // `checkExpressionWithContextualType(arg, paramType, context, inferential)`
    // assigns the callback's parameter types (from the contextual signature
    // instantiated by the fixing mapper) and infers its return type from the
    // body, yielding the function type matched against `paramType`.
    // Go: internal/checker/checker.go:Checker.inferTypeArguments (arg loop) +
    // contextuallyCheckFunctionExpressionOrObjectLiteralMethod
    fn infer_from_context_sensitive_argument(
        &mut self,
        program: &dyn BoundProgram,
        arg: NodeId,
        param_type: TypeId,
        context: &mut InferenceContext,
    ) -> TypeId {
        // The lazy inference mapper fixes the type variables inferred so far
        // (e.g. `T -> number`), leaving still-uninferred ones (e.g. `U`) as
        // themselves, so the instantiated parameter type is `(x: number) => U`.
        let mapper = self.get_fixing_inference_mapper(program, context);
        let instantiated_param = self.instantiate_param_type(program, param_type, &mapper);
        self.check_expression_with_contextual_type(program, arg, instantiated_param, Some(context))
    }

    // Builds the function type of a context-sensitive arrow/function expression
    // argument after its parameters have been contextually typed: a fresh
    // anonymous object type carrying a call signature whose parameter symbols are
    // the expression's parameters (now resolved to their contextual types) and
    // whose return type is the expression's annotated return type, else its
    // body-inferred return type. Mirrors `getTypeOfSymbol` of a function
    // expression (an anonymous type with one call signature) for the reachable
    // subset.
    //
    // DEFER(phase-4-checker-C-C): generic arrows (their own type parameters),
    // the `this`-parameter, rest parameters, and async/generator return
    // unwrapping. blocked-by: those signature features.
    // Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModule (function expr)
    fn get_type_of_context_sensitive_arrow(
        &mut self,
        program: &dyn BoundProgram,
        arg: NodeId,
    ) -> TypeId {
        let params = function_like_parameters(program, arg);
        let mut parameters = Vec::with_capacity(params.len());
        let mut min_argument_count = 0i32;
        for &param in &params {
            if let Some(sym) = program.symbol_of_node(param) {
                parameters.push(sym);
            }
            if !is_optional_parameter(program, param) {
                min_argument_count = parameters.len() as i32;
            }
        }
        let return_type = match arrow_return_type_node(program, arg) {
            Some(node) => get_type_from_type_node(self, program, node, None),
            None => self.get_return_type_from_body(program, arg),
        };
        let mut signature = Signature::new(SignatureFlags::NONE);
        signature.declaration = Some(arg);
        signature.parameters = parameters;
        signature.min_argument_count = min_argument_count;
        signature.resolved_return_type = Some(return_type);
        let sig = self.new_signature(signature);
        let object = ObjectType {
            call_signatures: vec![sig],
            ..Default::default()
        };
        self.new_object_type(ObjectFlags::ANONYMOUS, None, object)
    }

    // Infers the return type of a context-sensitive arrow/function expression
    // from its body (Go's `getReturnTypeFromBody`, reachable subset): for a
    // concise body, the (widened) type of the body expression; for a block body,
    // the (widened) union of its `return` expression types, else `void`. The body
    // is checked with diagnostics rolled back, since the applicability pass
    // re-checks and reports it once.
    //
    // DEFER(phase-4-checker-C-C): async/generator unwrapping, the
    // never-returning / contextual-`undefined` arms, and the contextual-signature
    // literal-preservation step. blocked-by: awaited/iterable types + return
    // control-flow analysis.
    // Go: internal/checker/checker.go:Checker.getReturnTypeFromBody
    pub(crate) fn get_return_type_from_body(
        &mut self,
        program: &dyn BoundProgram,
        arg: NodeId,
    ) -> TypeId {
        let body = function_like_body(program, arg);
        let Some(body) = body else {
            return self.error_type;
        };
        let handle = program.file_handle();
        let before = self.diagnostics_by_file.get(&handle).map_or(0, Vec::len);
        let result = if program.arena().kind(body) == Kind::Block {
            let return_exprs = collect_return_expressions(program, body);
            if return_exprs.is_empty() {
                self.void_type
            } else {
                let types: Vec<TypeId> = return_exprs
                    .into_iter()
                    .map(|e| self.get_widened_type_for_return_expression(program, e))
                    .collect();
                let union = self.get_union_type(&types);
                self.get_widened_type(union)
            }
        } else {
            self.get_widened_type_for_return_expression(program, body)
        };
        if let Some(diagnostics) = self.diagnostics_by_file.get_mut(&handle) {
            diagnostics.truncate(before);
        }
        result
    }

    /// Widens the type of a return-position expression after checking it (Go's
    /// `checkAndAggregateReturnExpressionTypes` per-expression widening followed
    /// by `getWidenedType`).
    ///
    /// # Examples
    ///
    /// A fresh string literal `"s"` widens to `string`.
    ///
    /// # Side effects
    ///
    /// May record diagnostics from `check_expression` (callers that infer return
    /// types roll these back).
    // Go: internal/checker/checker.go:Checker.checkAndAggregateReturnExpressionTypes (per expr)
    pub(crate) fn get_widened_type_for_return_expression(
        &mut self,
        program: &dyn BoundProgram,
        expression: NodeId,
    ) -> TypeId {
        let t = self.check_expression(program, expression);
        let widened = self.get_widened_literal_type(t);
        self.get_widened_type(widened)
    }

    // Builds the lazy inference mapper that fixes the type variables inferred so
    // far (Go's fixing `InferenceTypeMapper`, reachable realization): each slot
    // with candidates maps its type parameter to its current inferred type
    // (resolved and cached, i.e. "fixed"); slots without candidates are omitted,
    // so they instantiate to themselves rather than being prematurely resolved
    // to `unknown` (which would block a later inference such as a callback's
    // result). This is what types `(x: T) => U` as `(x: number) => U` after `T`
    // is inferred while leaving `U` open.
    //
    // DEFER(phase-4-checker-C-C): the genuinely on-demand variant that fixes a
    // type parameter only when first accessed, the non-fixing mapper, and the
    // default/constraint instantiation of un-inferred slots. blocked-by: a
    // context-capturing mapper variant + the full `getInferredType` default path.
    // Go: internal/checker/mapper.go:Checker.newInferenceTypeMapper (fixing)
    fn get_fixing_inference_mapper(
        &mut self,
        program: &dyn BoundProgram,
        context: &mut InferenceContext,
    ) -> TypeMapper {
        let mut sources = Vec::new();
        let mut targets = Vec::new();
        for i in 0..context.inferences.len() {
            if context.inferences[i].candidates.is_empty() {
                continue;
            }
            let tp = context.inferences[i].type_parameter;
            let inferred = self.get_inferred_type_for_call(program, context, i);
            sources.push(tp);
            targets.push(inferred);
        }
        TypeMapper::Array { sources, targets }
    }

    // Types an argument expression to obtain its type for inference, rolling back
    // any diagnostics it emits: the applicability pass re-checks the argument and
    // reports them once (Go reuses `checkExpressionCached`, whose memoized type is
    // reused without re-reporting).
    fn check_expression_for_inference(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        let handle = program.file_handle();
        let before = self.diagnostics_by_file.get(&handle).map_or(0, Vec::len);
        let t = self.check_expression(program, node);
        if let Some(diagnostics) = self.diagnostics_by_file.get_mut(&handle) {
            diagnostics.truncate(before);
        }
        t
    }

    // Descends into a type node, validating each contained type-reference node
    // (Go's `checkSourceElement` over a type node). Recurses through the
    // composite type-node kinds reachable in C-B1.
    //
    // DEFER(phase-4-checker-C-C): type-literal member type nodes, mapped /
    // conditional / indexed-access / function type-node bodies, and `import()`
    // types. blocked-by: those type constructors + their member walks.
    // Validates labeled tuple element modifier placement (Go's `checkNamedTupleMember`).
    // Go: internal/checker/checker.go:Checker.checkNamedTupleMember
    fn check_named_tuple_member(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::NamedTupleMember(d) = program.arena().data(node) else {
            return;
        };
        if d.dot_dot_dot_token.is_some() && d.question_token.is_some() {
            self.grammar_error_on_node(
                program,
                node,
                &tsgo_diagnostics::A_TUPLE_MEMBER_CANNOT_BE_BOTH_OPTIONAL_AND_REST,
                &[],
            );
        }
        if program.arena().kind(d.type_node) == Kind::OptionalType {
            self.grammar_error_on_node(
                program,
                d.type_node,
                &tsgo_diagnostics::A_LABELED_TUPLE_ELEMENT_IS_DECLARED_AS_OPTIONAL_WITH_A_QUESTION_MARK_AFTER_THE_NAME_AND_BEFORE_THE_COLON_RATHER_THAN_AFTER_THE_TYPE,
                &[],
            );
        }
        if program.arena().kind(d.type_node) == Kind::RestType {
            self.grammar_error_on_node(
                program,
                d.type_node,
                &tsgo_diagnostics::A_LABELED_TUPLE_ELEMENT_IS_DECLARED_AS_REST_WITH_A_BEFORE_THE_NAME_RATHER_THAN_BEFORE_THE_TYPE,
                &[],
            );
        }
        self.check_type_node(program, d.type_node);
    }

    // Go: internal/checker/checker.go:Checker.checkSourceElement (type-node arms)
    fn check_type_node(&mut self, program: &dyn BoundProgram, node: NodeId) {
        match program.arena().kind(node) {
            Kind::TypeReference => {
                self.check_type_reference_node(program, node);
                if let NodeData::TypeReference(d) = program.arena().data(node) {
                    if let Some(list) = d.type_arguments.clone() {
                        for arg in list.nodes {
                            self.check_type_node(program, arg);
                        }
                    }
                }
            }
            Kind::ArrayType => {
                if let NodeData::ArrayType(d) = program.arena().data(node) {
                    let element = d.element_type;
                    self.check_type_node(program, element);
                }
            }
            Kind::TupleType => {
                if let NodeData::TupleType(d) = program.arena().data(node) {
                    for element in d.types.nodes.clone() {
                        self.check_type_node(program, element);
                    }
                }
            }
            Kind::UnionType => {
                if let NodeData::UnionType(d) = program.arena().data(node) {
                    for member in d.types.nodes.clone() {
                        self.check_type_node(program, member);
                    }
                }
            }
            Kind::IntersectionType => {
                if let NodeData::IntersectionType(d) = program.arena().data(node) {
                    for member in d.types.nodes.clone() {
                        self.check_type_node(program, member);
                    }
                }
            }
            Kind::ParenthesizedType => {
                if let NodeData::ParenthesizedType(d) = program.arena().data(node) {
                    let inner = d.type_node;
                    self.check_type_node(program, inner);
                }
            }
            Kind::TypeOperator => {
                self.check_grammar_type_operator_node(program, node);
                if let NodeData::TypeOperator(d) = program.arena().data(node) {
                    let operand = d.type_node;
                    self.check_type_node(program, operand);
                }
            }
            Kind::NamedTupleMember => {
                self.check_named_tuple_member(program, node);
            }
            Kind::OptionalType => {
                if let NodeData::OptionalType(d) = program.arena().data(node) {
                    self.check_type_node(program, d.type_node);
                }
            }
            Kind::RestType => {
                if let NodeData::RestType(d) = program.arena().data(node) {
                    self.check_type_node(program, d.type_node);
                }
            }
            _ => {}
        }
    }

    // Validates a type-reference node's type arguments against the referenced
    // declaration's type parameters: arity (`2314`/`2707`) and constraints
    // (`2344`). Mirrors Go's `checkTypeReferenceOrImport` (the constraint half)
    // plus the arity diagnostics `getTypeFromClassOrInterfaceReference`/
    // `getTypeFromTypeAliasReference` emit during type formation — the port emits
    // them here so the diagnostic surfaces once per node.
    //
    // DEFER(phase-4-checker-C-C): qualified-name references, the
    // `Type_0_is_not_generic` (2315) arm for arguments on a non-generic type,
    // and the JS-implicit-any relaxation. blocked-by: namespace resolution +
    // JS-file gating.
    // Go: internal/checker/checker.go:Checker.checkTypeReferenceNode / checkTypeReferenceOrImport
    fn check_type_reference_node(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let (type_name, type_arg_nodes) = match program.arena().data(node) {
            NodeData::TypeReference(d) => (
                d.type_name,
                d.type_arguments
                    .as_ref()
                    .map(|l| l.nodes.clone())
                    .unwrap_or_default(),
            ),
            _ => return,
        };
        if program.arena().kind(type_name) != Kind::Identifier {
            return;
        }
        let name = program.arena().text(type_name).to_string();
        let Some(symbol) = resolve_name(
            program,
            node,
            &name,
            SymbolFlags::TYPE,
            false,
            program.globals(),
        ) else {
            return;
        };
        let flags = program.symbol(symbol).flags;
        if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
            self.check_class_or_interface_type_reference(program, node, symbol, &type_arg_nodes);
        } else if flags.contains(SymbolFlags::TYPE_ALIAS) {
            self.check_type_alias_type_reference(program, node, symbol, &type_arg_nodes);
        }
        // DEFER(phase-4-checker-C-C): enum/type-parameter references with
        // arguments report `Type_0_is_not_generic` (2315). blocked-by:
        // checkNoTypeArguments for the non-generic-symbol path.
    }

    // Arity + constraint checking for a class/interface type reference (Go's
    // `getTypeFromClassOrInterfaceReference` arity arm + `checkTypeArgumentConstraints`).
    // Go: internal/checker/checker.go:Checker.getTypeFromClassOrInterfaceReference
    fn check_class_or_interface_type_reference(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        symbol: SymbolId,
        type_arg_nodes: &[NodeId],
    ) {
        let declared = get_declared_type_of_symbol(self, program, symbol, program.globals());
        if declared == self.error_type {
            return;
        }
        let type_parameters = self
            .get_type(declared)
            .as_object()
            .map(|o| o.type_parameters.clone())
            .unwrap_or_default();
        if type_parameters.is_empty() {
            return;
        }
        let num_args = type_arg_nodes.len();
        let min = get_min_type_argument_count(self, program, &type_parameters);
        let max = type_parameters.len();
        if num_args < min || num_args > max {
            // A class/interface prints with its type parameters: `Box<T>` (Go's
            // `TypeToStringEx(t, ..., WriteArrayAsGenericType)`).
            let name = super::nodebuilder::symbol_to_string(self, program, symbol);
            let type_str = self.format_generic_type_name(program, &name, &type_parameters);
            self.report_generic_arity_error(program, node, &type_str, min, max);
            return;
        }
        if !type_arg_nodes.is_empty() {
            self.check_type_argument_constraints_for_reference(
                program,
                &type_parameters,
                type_arg_nodes,
            );
        }
    }

    // Arity + constraint checking for a type-alias reference (Go's
    // `getTypeFromTypeAliasReference` arity arm + `checkTypeArgumentConstraints`).
    // Go: internal/checker/checker.go:Checker.getTypeFromTypeAliasReference
    fn check_type_alias_type_reference(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        symbol: SymbolId,
        type_arg_nodes: &[NodeId],
    ) {
        // Populate the alias's local type parameters (set in its declared type).
        let _ = get_declared_type_of_symbol(self, program, symbol, program.globals());
        let type_parameters = self
            .type_alias_links
            .try_get(&symbol)
            .map(|l| l.type_parameters.clone())
            .unwrap_or_default();
        if type_parameters.is_empty() {
            return;
        }
        let num_args = type_arg_nodes.len();
        let min = get_min_type_argument_count(self, program, &type_parameters);
        let max = type_parameters.len();
        if num_args < min || num_args > max {
            // A type alias prints as just its name `G` (Go's `c.symbolToString`).
            let type_str = super::nodebuilder::symbol_to_string(self, program, symbol);
            self.report_generic_arity_error(program, node, &type_str, min, max);
            return;
        }
        if !type_arg_nodes.is_empty() {
            self.check_type_argument_constraints_for_reference(
                program,
                &type_parameters,
                type_arg_nodes,
            );
        }
    }

    // Renders a generic class/interface name with its type parameters, e.g.
    // `Box<T>` / `Pair<A, B>` (each parameter printed by its declaration name).
    // Mirrors `TypeToStringEx`'s generic-target form for the arity message.
    // Go: internal/checker/nodebuilderimpl.go (type reference with type parameters)
    fn format_generic_type_name(
        &self,
        program: &dyn BoundProgram,
        name: &str,
        type_parameters: &[TypeId],
    ) -> String {
        let params: Vec<String> = type_parameters
            .iter()
            .map(|&tp| {
                self.get_type(tp)
                    .as_type_parameter()
                    .and_then(|d| d.symbol)
                    .map(|s| program.symbol(s).name.clone())
                    .unwrap_or_else(|| "T".to_string())
            })
            .collect();
        format!("{name}<{}>", params.join(", "))
    }

    // Emits `2314` (single count) or `2707` (a `min..max` range, when defaults
    // make the count a range) for a wrong type-argument count on a generic type
    // reference, spanning the whole reference node.
    // Go: internal/checker/checker.go (Generic_type_0_requires_* emission)
    fn report_generic_arity_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        type_str: &str,
        min: usize,
        max: usize,
    ) {
        let message = if min == max {
            &tsgo_diagnostics::GENERIC_TYPE_0_REQUIRES_1_TYPE_ARGUMENT_S
        } else {
            &tsgo_diagnostics::GENERIC_TYPE_0_REQUIRES_BETWEEN_1_AND_2_TYPE_ARGUMENTS
        };
        self.error(
            program,
            node,
            message,
            &[type_str, &min.to_string(), &max.to_string()],
        );
    }

    // Checks each provided type argument of a type reference against its
    // (instantiated) constraint, reporting `2344` on the offending argument node
    // (Go's `checkTypeArgumentConstraints`). All type parameters are visited so
    // the constraint mapper covers defaults, but only explicitly-provided
    // arguments carry an error node (Go's `core.ElementOrNil`).
    // Go: internal/checker/checker.go:Checker.checkTypeArgumentConstraints
    fn check_type_argument_constraints_for_reference(
        &mut self,
        program: &dyn BoundProgram,
        type_parameters: &[TypeId],
        type_arg_nodes: &[NodeId],
    ) {
        let provided: Vec<TypeId> = type_arg_nodes
            .iter()
            .map(|&n| get_type_from_type_node(self, program, n, None))
            .collect();
        let effective = fill_missing_type_arguments(self, program, &provided, type_parameters);
        let mapper = TypeMapper::new(type_parameters, &effective);
        for (i, &tp) in type_parameters.iter().enumerate() {
            let Some(constraint) = get_constraint_of_type_parameter(self, program, tp) else {
                continue;
            };
            let instantiated_constraint = self.instantiate_type(constraint, &mapper);
            if !self.is_type_assignable_to(program, effective[i], instantiated_constraint) {
                let Some(&arg_node) = type_arg_nodes.get(i) else {
                    continue;
                };
                let arg_str = super::nodebuilder::type_to_string(self, program, effective[i]);
                let constraint_str =
                    super::nodebuilder::type_to_string(self, program, instantiated_constraint);
                self.error(
                    program,
                    arg_node,
                    &tsgo_diagnostics::TYPE_0_DOES_NOT_SATISFY_THE_CONSTRAINT_1,
                    &[arg_str.as_str(), constraint_str.as_str()],
                );
            }
        }
    }

    // Grammar-checks a type-parameter list for `2706` "Required type parameters
    // may not follow optional type parameters." (a required parameter after one
    // with a `= Default`), mirroring Go's `checkTypeParameters` (the
    // `seenDefault` arm).
    //
    // DEFER(phase-4-checker-C-C): `checkTypeParametersNotReferenced` (a default
    // referencing a later parameter) and the duplicate-identifier check.
    // blocked-by: forward-reference detection + per-list duplicate tracking.
    // Go: internal/checker/checker.go:Checker.checkTypeParameters
    fn check_grammar_type_parameter_defaults(
        &mut self,
        program: &dyn BoundProgram,
        type_parameters: Option<tsgo_ast::NodeList>,
    ) {
        let Some(list) = type_parameters else {
            return;
        };
        let mut seen_default = false;
        for node in list.nodes {
            let has_default = matches!(
                program.arena().data(node),
                NodeData::TypeParameterDeclaration(d) if d.default_type.is_some()
            );
            if has_default {
                seen_default = true;
            } else if seen_default {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::REQUIRED_TYPE_PARAMETERS_MAY_NOT_FOLLOW_OPTIONAL_TYPE_PARAMETERS,
                    &[],
                );
            }
        }
    }

    // Reports whether the argument count matches the signature's arity (the
    // arity portion of Go's `hasCorrectArity`, 4q subset for a non-spread,
    // complete call): the count must be at least the minimum argument count, and
    // — UNLESS the signature has an effective rest parameter (`...args`), which
    // accepts unboundedly many trailing arguments — at most the parameter count.
    //
    // DEFER(phase-4-checker-4q+): spread arguments, incomplete calls (missing
    // close paren), and the `void`-accepting trailing-parameter relaxation.
    // blocked-by: spread detection + grammar end positions + `void` filtering.
    // Go: internal/checker/checker.go:Checker.hasCorrectArity(9070)
    fn has_correct_arity(&self, signature: SignatureId, arg_count: usize) -> bool {
        let arg_count = arg_count as i32;
        if arg_count < self.get_min_argument_count(signature) {
            return false;
        }
        // Go: `!hasEffectiveRestParameter && argCount > effectiveParameterCount`
        // is the only "too many" rejection — a rest parameter lifts the cap.
        self.has_effective_rest_parameter(signature)
            || arg_count <= self.get_parameter_count(signature) as i32
    }

    // Reports a wrong-argument-count error (`2554`) for the call (the relevant
    // branches of Go's `getArgumentArityError`). For too few arguments the error
    // is placed on the call target (Go's `getErrorNodeForCallNode`).
    //
    // DEFER(phase-4-checker-4q+): the overload `No_overload_expects_0_arguments...`
    // (2575) message, the rest (`Expected_at_least`) variant, the spread-argument
    // and decorator messages, the multi-extra-argument synthetic span (4q reports
    // on the first extra argument), and the related-info ("An argument for '0'
    // was not provided"). blocked-by: overload resolution + rest/spread types +
    // decorators + synthetic-span construction.
    // Go: internal/checker/checker.go:Checker.getArgumentArityError(9668)
    fn report_argument_arity_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signature: SignatureId,
        args: &[NodeId],
    ) {
        let parameter_range = self.parameter_range_string(signature);
        let arg_count = args.len().to_string();
        let min = self.get_min_argument_count(signature);
        let message = &tsgo_diagnostics::EXPECTED_0_ARGUMENTS_BUT_GOT_1;
        if (args.len() as i32) < min {
            // Too few arguments: the span is the call target, not any argument
            // (Go's `len(args) < minCount` branch).
            let error_node = call_error_node(program, node);
            self.error(
                program,
                error_node,
                message,
                &[&parameter_range, &arg_count],
            );
        } else {
            // Too many arguments: the span covers the extra arguments. 4q reports
            // on the first extra argument (Go spans `args[maxCount]..last`).
            let max = self.get_parameter_count(signature);
            let error_node = args.get(max).copied().unwrap_or(node);
            self.error(
                program,
                error_node,
                message,
                &[&parameter_range, &arg_count],
            );
        }
    }

    // Returns the printed parameter-count range for an arity error message
    // (Go's `parameterRange`): `"min"` when the minimum and maximum counts match,
    // else `"min-max"`.
    //
    // DEFER(phase-4-checker-4q+): the rest-parameter form (just `"min"` with a
    // trailing `+` semantics handled by the `Expected_at_least` message).
    // blocked-by: rest parameters.
    // Go: internal/checker/checker.go:Checker.getArgumentArityError (parameterRange)
    fn parameter_range_string(&self, signature: SignatureId) -> String {
        let min = self.get_min_argument_count(signature);
        let max = self.get_parameter_count(signature) as i32;
        if min < max {
            format!("{min}-{max}")
        } else {
            min.to_string()
        }
    }

    // Returns the minimum number of arguments a signature requires (Go's
    // `getMinArgumentCount`, 4q subset: the stored required-parameter count).
    //
    // DEFER(phase-4-checker-4q+): the rest-tuple required count and the
    // trailing-`void` relaxation. blocked-by: tuple types + `void` filtering.
    // Go: internal/checker/relater.go:Checker.getMinArgumentCount(1701)
    fn get_min_argument_count(&self, signature: SignatureId) -> i32 {
        self.signature(signature).min_argument_count
    }

    // Checks that each argument of a call is assignable to its parameter (the
    // argument loop of Go's `isSignatureApplicable`): reports `2345` at the first
    // non-assignable argument (Go stops at the first failure). A literal source
    // is generalized to its base type for the message, reusing 4m's
    // `generalized_source_for_error`.
    //
    // DEFER(phase-4-checker-4q+): the `this`-argument check, contextual typing,
    // rest/spread argument aggregation, and the missing-`await` suggestion.
    // blocked-by: `this`-typing, contextual typing, tuple/spread types, awaited
    // types (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.isSignatureApplicable(9219)
    fn check_applicable_signature_for_call(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        args: &[NodeId],
    ) {
        let count = self.get_parameter_count(signature).min(args.len());
        for (i, &arg) in args.iter().enumerate().take(count) {
            let arg_type = self.check_expression(program, arg);
            let param_type = self.get_type_at_position(program, signature, i);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                self.report_argument_not_assignable(program, arg, arg_type, param_type);
                // Go's `isSignatureApplicable` returns at the first failure, so
                // only one `2345` is reported per call.
                return;
            }
        }
    }

    // Resolves an overloaded call (more than one call signature), mirroring Go's
    // `resolveCall` -> `chooseOverload` -> `reportCallResolutionErrors` for the
    // reachable subset: each argument expression is checked once (its type is
    // cached locally, mirroring Go's `checkExpressionCached`), then candidates
    // are tried in declaration order. The first signature whose arity matches and
    // whose arguments are all assignable is the resolved overload (no
    // diagnostic). When none applies:
    // - more than one correct-arity candidate failed on argument types -> `2769`
    //   `No overload matches this call.`;
    // - exactly one correct-arity candidate failed -> that candidate's own `2345`
    //   argument error;
    // - no candidate had correct arity -> the overload arity error (`2575` /
    //   `2554`).
    //
    // DEFER(phase-4-checker-4r+): the per-overload elaboration chain
    // (`The last overload gave the following error.` + `Overload N of M`
    // related-info), the `getCandidateForOverloadFailure` best-match selection,
    // the two-pass subtype/assignable relations, generic call-site inference, and
    // construct/`this`/spread handling. blocked-by: diagnostic message chains +
    // related information + inference contexts + tuple/spread types.
    // Go: internal/checker/checker.go:Checker.resolveCall(8806)/chooseOverload(8988)/reportCallResolutionErrors(9612)
    fn resolve_overloaded_call(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signatures: &[SignatureId],
        args: &[NodeId],
    ) -> TypeId {
        // Check each argument once and cache its type, so applicability passes
        // over multiple candidates do not re-report nested diagnostics.
        let arg_types: Vec<TypeId> = args
            .iter()
            .map(|&arg| self.check_expression(program, arg))
            .collect();
        let mut arity_matched: Vec<SignatureId> = Vec::new();
        for &signature in signatures {
            if !self.has_correct_arity(signature, args.len()) {
                continue;
            }
            if self.signature_applicable_with_types(program, signature, &arg_types) {
                // The first applicable overload resolves the call (Go's
                // `chooseOverload` returns it); its return type is the result.
                return self.get_return_type_of_call(program, signature, &[], &[]);
            }
            arity_matched.push(signature);
        }
        match arity_matched.len() {
            0 => self.report_overload_arity_error(program, node, signatures, args),
            1 => self.report_inapplicable_argument(program, arity_matched[0], args, &arg_types),
            _ => {
                // More than one correct-arity candidate failed on argument types:
                // Go wraps the LAST candidate's argument error in a chain under
                // the `No_overload_matches_this_call` (2769) head:
                //   2769 No overload matches this call.
                //     2770 The last overload gave the following error.
                //       2345 Argument of type 'X' is not assignable to ...
                // located at the failing argument (Go's
                // `reportCallResolutionErrors`, `candidatesForArgumentError`
                // branch with `len > 1`).
                let last = *arity_matched.last().unwrap();
                if let Some((arg_node, source_str, target_str)) =
                    self.first_failing_argument(program, last, args, &arg_types)
                {
                    self.report_no_overload_matches(program, arg_node, &source_str, &target_str);
                } else {
                    // Defensive: the multi-candidate branch is reached only when
                    // every arity-matched candidate failed, so a failing
                    // argument is expected; fall back to a bare 2769.
                    let error_node = call_error_node(program, node);
                    self.error(
                        program,
                        error_node,
                        &tsgo_diagnostics::NO_OVERLOAD_MATCHES_THIS_CALL,
                        &[],
                    );
                }
            }
        }
        // The overload-failure result type is the last candidate's return type
        // (Go's `getCandidateForOverloadFailure` falls back to the last
        // signature), avoiding a cascading error type at the use site.
        match signatures.last() {
            Some(&last) => self.get_return_type_of_call(program, last, &[], &[]),
            None => self.error_type,
        }
    }

    // Chooses the overload signature a call/new resolves to (Go's `chooseOverload`
    // / `resolveCall` selection), reporting resolution errors when no candidate
    // applies.
    // Go: internal/checker/checker.go:Checker.resolveCall(8806)/chooseOverload(8988)
    fn resolve_overloaded_call_signature(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signatures: &[SignatureId],
        args: &[NodeId],
    ) -> SignatureId {
        let arg_types: Vec<TypeId> = args
            .iter()
            .map(|&arg| self.check_expression(program, arg))
            .collect();
        let mut arity_matched: Vec<SignatureId> = Vec::new();
        for &signature in signatures {
            if !self.has_correct_arity(signature, args.len()) {
                continue;
            }
            if self.signature_applicable_with_types(program, signature, &arg_types) {
                return signature;
            }
            arity_matched.push(signature);
        }
        match arity_matched.len() {
            0 => self.report_overload_arity_error(program, node, signatures, args),
            1 => self.report_inapplicable_argument(program, arity_matched[0], args, &arg_types),
            _ => {
                let last = *arity_matched.last().unwrap();
                if let Some((arg_node, source_str, target_str)) =
                    self.first_failing_argument(program, last, args, &arg_types)
                {
                    self.report_no_overload_matches(program, arg_node, &source_str, &target_str);
                } else {
                    let error_node = call_error_node(program, node);
                    self.error(
                        program,
                        error_node,
                        &tsgo_diagnostics::NO_OVERLOAD_MATCHES_THIS_CALL,
                        &[],
                    );
                }
            }
        }
        signatures
            .last()
            .copied()
            .unwrap_or_else(|| signatures[0])
    }

    // Reports whether every overlapping argument of a call is assignable to its
    // parameter for `signature`, using already-computed `arg_types` (the silent,
    // non-reporting form of `check_applicable_signature_for_call`, used by
    // overload resolution).
    // Go: internal/checker/checker.go:Checker.isSignatureApplicable(9219) (no reportErrors)
    fn signature_applicable_with_types(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        arg_types: &[TypeId],
    ) -> bool {
        let count = self.get_parameter_count(signature).min(arg_types.len());
        for (i, &arg_type) in arg_types.iter().enumerate().take(count) {
            let param_type = self.get_type_at_position(program, signature, i);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                return false;
            }
        }
        true
    }

    // Reports the first non-assignable argument of `signature` as `2345`, using
    // already-computed `arg_types` (the reporting form used when exactly one
    // overload had correct arity, mirroring Go emitting the single candidate's
    // argument error without the overload chain).
    // Go: internal/checker/checker.go:Checker.isSignatureApplicable(9219) (reportErrors)
    fn report_inapplicable_argument(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        args: &[NodeId],
        arg_types: &[TypeId],
    ) {
        let count = self.get_parameter_count(signature).min(args.len());
        for i in 0..count {
            let arg_type = arg_types[i];
            let param_type = self.get_type_at_position(program, signature, i);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                self.report_argument_not_assignable(program, args[i], arg_type, param_type);
                return;
            }
        }
    }

    // Returns the first argument of `signature` whose type is not assignable to
    // its parameter, as `(arg_node, generalized_source_str, target_str)`, or
    // `None` when every overlapping argument is assignable (the silent form of
    // `report_inapplicable_argument` returning the offending pair instead of
    // recording a `2345`). Used to build the overload-failure elaboration.
    // Go: internal/checker/checker.go:Checker.isSignatureApplicable (first failure)
    fn first_failing_argument(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        args: &[NodeId],
        arg_types: &[TypeId],
    ) -> Option<(NodeId, String, String)> {
        let count = self.get_parameter_count(signature).min(args.len());
        for i in 0..count {
            let arg_type = arg_types[i];
            let param_type = self.get_type_at_position(program, signature, i);
            if !self.is_type_assignable_to(program, arg_type, param_type) {
                let generalized = self.generalized_source_for_error(arg_type, param_type);
                let source_str = super::nodebuilder::type_to_string(self, program, generalized);
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                return Some((args[i], source_str, target_str));
            }
        }
        None
    }

    // Records the overload-failure diagnostic `2769` "No overload matches this
    // call." at `arg_node`, with the nested elaboration chain `2770` "The last
    // overload gave the following error." wrapping the last overload's `2345`
    // argument error (Go's `reportCallResolutionErrors` ->
    // `NewDiagnosticChain(NewDiagnosticChain(argDiag, 2770), 2769)`).
    //
    // DEFER(phase-4-checker-C-D2+): the per-overload "Overload N of M, '(sig)',
    // gave the following error." variant (used when the best candidate is not
    // the last), the `The_last_overload_is_declared_here` related info, and the
    // implementation-success elaboration. blocked-by: per-overload signature
    // printing + `getCandidateForOverloadFailure` best-match selection.
    // Go: internal/checker/checker.go:Checker.reportCallResolutionErrors
    fn report_no_overload_matches(
        &mut self,
        program: &dyn BoundProgram,
        arg_node: NodeId,
        source_str: &str,
        target_str: &str,
    ) {
        let arg_message =
            &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1;
        let last_message = &tsgo_diagnostics::THE_LAST_OVERLOAD_GAVE_THE_FOLLOWING_ERROR;
        let head_message = &tsgo_diagnostics::NO_OVERLOAD_MATCHES_THIS_CALL;
        let leaf = DiagnosticMessageChain {
            code: arg_message.code(),
            category: arg_message.category(),
            message: tsgo_diagnostics::format(&arg_message.to_string(), &[source_str, target_str]),
            next: Vec::new(),
        };
        let mid = DiagnosticMessageChain {
            code: last_message.code(),
            category: last_message.category(),
            message: tsgo_diagnostics::format(&last_message.to_string(), &[]),
            next: vec![leaf],
        };
        let loc = program.arena().loc(arg_node);
        let diagnostic = Diagnostic {
            code: head_message.code(),
            category: head_message.category(),
            message: tsgo_diagnostics::format(&head_message.to_string(), &[]),
            start: loc.pos(),
            length: loc.end() - loc.pos(),
            related_information: Vec::new(),
            message_chain: vec![mid],
        };
        self.add_diagnostic(program, diagnostic);
    }

    // Records the not-callable / not-constructable diagnostic (Go's
    // `invocationError` -> `invocationErrorDetails`), including union-type
    // elaboration chains.
    // Go: internal/checker/checker.go:Checker.invocationError(9956)
    fn invocation_error(
        &mut self,
        program: &dyn BoundProgram,
        error_target: NodeId,
        apparent_type: TypeId,
        kind: InvocationKind,
    ) {
        let mut diagnostic =
            self.invocation_error_details(program, error_target, apparent_type, kind);
        self.invocation_error_recovery(program, apparent_type, kind, &mut diagnostic);
        self.add_diagnostic(program, diagnostic);
    }

    fn signatures_for_invocation(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        kind: InvocationKind,
    ) -> Vec<SignatureId> {
        match kind {
            InvocationKind::Call => self.get_signatures_of_type(t),
            InvocationKind::Construct => {
                self.collect_construct_signatures_of_type(program, t)
            }
        }
    }

    // Attaches namespace-import recovery related information when a wrapped
    // module export was called or constructed (Go's `invocationErrorRecovery`).
    // Go: internal/checker/checker.go:Checker.invocationErrorRecovery(9965)
    fn invocation_error_recovery(
        &mut self,
        program: &dyn BoundProgram,
        apparent_type: TypeId,
        kind: InvocationKind,
        diagnostic: &mut Diagnostic,
    ) {
        let Some(symbol) = self.get_type(apparent_type).symbol else {
            return;
        };
        let Some(links) = self.export_type_links.try_get(&symbol) else {
            return;
        };
        let Some(import_node) = links.originating_import else {
            return;
        };
        let Some(target_sym) = links.target else {
            return;
        };
        let target_type = super::declared_types::get_type_of_symbol(
            self,
            program,
            target_sym,
            program.globals(),
        );
        if self
            .signatures_for_invocation(program, target_type, kind)
            .is_empty()
        {
            return;
        }
        let related = self.diagnostic_for_node(
            program,
            import_node,
            &tsgo_diagnostics::TYPE_ORIGINATES_AT_THIS_IMPORT_A_NAMESPACE_STYLE_IMPORT_CANNOT_BE_CALLED_OR_CONSTRUCTED_AND_WILL_CAUSE_A_FAILURE_AT_RUNTIME_CONSIDER_USING_A_DEFAULT_IMPORT_OR_IMPORT_REQUIRE_HERE_INSTEAD,
            &[],
        );
        diagnostic.add_related_info(related);
    }

    // When a property-access expression is the callee of a call, Go reports the
    // invocation error on the member name, not the whole `obj.prop` span.
    // Go: internal/checker/checker.go:Checker.invocationErrorDetails(9905)
    fn invocation_error_diagnostic_target(
        program: &dyn BoundProgram,
        error_target: NodeId,
    ) -> NodeId {
        if let Some(parent) = program.arena().parent(error_target) {
            if program.arena().kind(parent) == Kind::CallExpression {
                if let NodeData::PropertyAccessExpression(d) = program.arena().data(error_target)
                {
                    return d.name;
                }
            }
        }
        error_target
    }

    // Builds the not-callable / not-constructable diagnostic chain for
    // `error_target` (Go's `invocationErrorDetails`).
    // Go: internal/checker/checker.go:Checker.invocationErrorDetails(9900)
    fn invocation_error_details(
        &mut self,
        program: &dyn BoundProgram,
        error_target: NodeId,
        apparent_type: TypeId,
        kind: InvocationKind,
    ) -> Diagnostic {
        let diagnostic_target = Self::invocation_error_diagnostic_target(program, error_target);
        let apparent = get_apparent_type(self, apparent_type);
        let awaited = self.get_awaited_type_no_alias(program, apparent);
        let maybe_missing_await = matches!(kind, InvocationKind::Call)
            && awaited != apparent
            && !self
                .signatures_for_invocation(program, awaited, kind)
                .is_empty();
        let mut head_message = match kind {
            InvocationKind::Call => &tsgo_diagnostics::THIS_EXPRESSION_IS_NOT_CALLABLE,
            InvocationKind::Construct => &tsgo_diagnostics::THIS_EXPRESSION_IS_NOT_CONSTRUCTABLE,
        };
        if matches!(kind, InvocationKind::Call) {
            if let Some(parent) = program.arena().parent(error_target) {
                if program.arena().kind(parent) == Kind::CallExpression {
                    let zero_arg_call = matches!(
                        program.arena().data(parent),
                        NodeData::CallExpression(d) if d.arguments.nodes.is_empty()
                    );
                    if zero_arg_call {
                        if let Some(sym) = super::symbols_query::get_symbol_at_location(
                            self,
                            program,
                            error_target,
                            program.globals(),
                        ) {
                            if self
                                .resolved_symbol_flags(program, sym)
                                .contains(SymbolFlags::GET_ACCESSOR)
                            {
                                head_message = &tsgo_diagnostics::THIS_EXPRESSION_IS_NOT_CALLABLE_BECAUSE_IT_IS_A_GET_ACCESSOR_DID_YOU_MEAN_TO_USE_IT_WITHOUT;
                            }
                        }
                    }
                }
            }
        }
        let mut chain: Option<DiagnosticMessageChain> = None;
        if self.get_type(apparent).flags().contains(TypeFlags::UNION) {
            let constituents = self
                .get_type(apparent)
                .union_types()
                .unwrap_or(&[])
                .to_vec();
            let apparent_str = super::nodebuilder::type_to_string(self, program, apparent);
            let mut has_signatures = false;
            for constituent in constituents {
                if !self
                    .signatures_for_invocation(program, constituent, kind)
                    .is_empty()
                {
                    has_signatures = true;
                    if chain.is_some() {
                        break;
                    }
                } else if chain.is_none() {
                    let constituent_str =
                        super::nodebuilder::type_to_string(self, program, constituent);
                    let leaf_message = match kind {
                        InvocationKind::Call => &tsgo_diagnostics::TYPE_0_HAS_NO_CALL_SIGNATURES,
                        InvocationKind::Construct => {
                            &tsgo_diagnostics::TYPE_0_HAS_NO_CONSTRUCT_SIGNATURES
                        }
                    };
                    let leaf = DiagnosticMessageChain {
                        code: leaf_message.code(),
                        category: leaf_message.category(),
                        message: tsgo_diagnostics::format(
                            &leaf_message.to_string(),
                            &[constituent_str.as_str()],
                        ),
                        next: Vec::new(),
                    };
                    let mid_message = match kind {
                        InvocationKind::Call => {
                            &tsgo_diagnostics::NOT_ALL_CONSTITUENTS_OF_TYPE_0_ARE_CALLABLE
                        }
                        InvocationKind::Construct => {
                            &tsgo_diagnostics::NOT_ALL_CONSTITUENTS_OF_TYPE_0_ARE_CONSTRUCTABLE
                        }
                    };
                    chain = Some(DiagnosticMessageChain {
                        code: mid_message.code(),
                        category: mid_message.category(),
                        message: tsgo_diagnostics::format(
                            &mid_message.to_string(),
                            &[apparent_str.as_str()],
                        ),
                        next: vec![leaf],
                    });
                    if has_signatures {
                        break;
                    }
                }
            }
            if !has_signatures {
                let message = match kind {
                    InvocationKind::Call => &tsgo_diagnostics::NO_CONSTITUENT_OF_TYPE_0_IS_CALLABLE,
                    InvocationKind::Construct => {
                        &tsgo_diagnostics::NO_CONSTITUENT_OF_TYPE_0_IS_CONSTRUCTABLE
                    }
                };
                chain = Some(DiagnosticMessageChain {
                    code: message.code(),
                    category: message.category(),
                    message: tsgo_diagnostics::format(&message.to_string(), &[apparent_str.as_str()]),
                    next: Vec::new(),
                });
            } else if chain.is_none() {
                let message = match kind {
                    InvocationKind::Call => {
                        &tsgo_diagnostics::EACH_MEMBER_OF_THE_UNION_TYPE_0_HAS_SIGNATURES_BUT_NONE_OF_THOSE_SIGNATURES_ARE_COMPATIBLE_WITH_EACH_OTHER
                    }
                    InvocationKind::Construct => {
                        &tsgo_diagnostics::EACH_MEMBER_OF_THE_UNION_TYPE_0_HAS_CONSTRUCT_SIGNATURES_BUT_NONE_OF_THOSE_SIGNATURES_ARE_COMPATIBLE_WITH_EACH_OTHER
                    }
                };
                chain = Some(DiagnosticMessageChain {
                    code: message.code(),
                    category: message.category(),
                    message: tsgo_diagnostics::format(&message.to_string(), &[apparent_str.as_str()]),
                    next: Vec::new(),
                });
            }
        } else {
            let type_str = super::nodebuilder::type_to_string(self, program, apparent);
            let message = match kind {
                InvocationKind::Call => &tsgo_diagnostics::TYPE_0_HAS_NO_CALL_SIGNATURES,
                InvocationKind::Construct => &tsgo_diagnostics::TYPE_0_HAS_NO_CONSTRUCT_SIGNATURES,
            };
            chain = Some(DiagnosticMessageChain {
                code: message.code(),
                category: message.category(),
                message: tsgo_diagnostics::format(&message.to_string(), &[type_str.as_str()]),
                next: Vec::new(),
            });
        }
        let loc = program.arena().loc(diagnostic_target);
        let mut diagnostic = Diagnostic {
            code: head_message.code(),
            category: head_message.category(),
            message: tsgo_diagnostics::format(&head_message.to_string(), &[]),
            start: loc.pos(),
            length: loc.end() - loc.pos(),
            related_information: Vec::new(),
            message_chain: chain.into_iter().collect(),
        };
        if maybe_missing_await {
            let related = self.diagnostic_for_node(
                program,
                error_target,
                &tsgo_diagnostics::DID_YOU_FORGET_TO_USE_AWAIT,
                &[],
            );
            diagnostic.add_related_info(related);
        }
        diagnostic
    }

    // Reports the `7053` implicit-any element-access error when a `string`/`number`
    // index cannot be applied to `object_type` (Go's
    // `getPropertyTypeForIndexType` `noImplicitAny` branch with the `7054`
    // inner diagnostic for primitive `string`/`number` indices).
    // Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType(26965)
    fn report_element_access_implicit_any_index_error(
        &mut self,
        program: &dyn BoundProgram,
        access_node: NodeId,
        _index_node: NodeId,
        object_type: TypeId,
        index_type: TypeId,
    ) {
        let object_str = super::nodebuilder::type_to_string(self, program, object_type);
        let index_str = super::nodebuilder::type_to_string(self, program, index_type);
        let inner_message =
            &tsgo_diagnostics::NO_INDEX_SIGNATURE_WITH_A_PARAMETER_OF_TYPE_0_WAS_FOUND_ON_TYPE_1;
        let inner = DiagnosticMessageChain {
            code: inner_message.code(),
            category: inner_message.category(),
            message: tsgo_diagnostics::format(
                &inner_message.to_string(),
                &[index_str.as_str(), object_str.as_str()],
            ),
            next: Vec::new(),
        };
        let head_message =
            &tsgo_diagnostics::ELEMENT_IMPLICITLY_HAS_AN_ANY_TYPE_BECAUSE_EXPRESSION_OF_TYPE_0_CAN_T_BE_USED_TO_INDEX_TYPE_1;
        let loc = program.arena().loc(access_node);
        let diagnostic = Diagnostic {
            code: head_message.code(),
            category: head_message.category(),
            message: tsgo_diagnostics::format(
                &head_message.to_string(),
                &[index_str.as_str(), object_str.as_str()],
            ),
            start: loc.pos(),
            length: loc.end() - loc.pos(),
            related_information: Vec::new(),
            message_chain: vec![inner],
        };
        self.add_diagnostic(program, diagnostic);
    }

    // Reports a wrong-argument-count error for an overloaded call where no
    // signature had correct arity (the multi-signature branch of Go's
    // `getArgumentArityError`): an argument count strictly between the smallest
    // minimum and largest maximum that matches no overload reports `2575`;
    // otherwise the count is outside every overload's range and reports `2554`
    // with the `min`/`min-max` range.
    //
    // DEFER(phase-4-checker-4r+): rest parameters (`Expected_at_least`), the
    // `void`-promise hint, the too-few related-info, and the multi-extra-argument
    // synthetic span. blocked-by: rest/tuple types + related information +
    // synthetic-span construction.
    // Go: internal/checker/checker.go:Checker.getArgumentArityError(9668)
    fn report_overload_arity_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        signatures: &[SignatureId],
        args: &[NodeId],
    ) {
        let arg_count = args.len() as i32;
        let mut min_count = i32::MAX;
        let mut max_count = i32::MIN;
        let mut max_below = i32::MIN;
        let mut min_above = i32::MAX;
        for &signature in signatures {
            let min_parameter = self.get_min_argument_count(signature);
            let max_parameter = self.get_parameter_count(signature) as i32;
            min_count = min_count.min(min_parameter);
            max_count = max_count.max(max_parameter);
            if min_parameter < arg_count && min_parameter > max_below {
                max_below = min_parameter;
            }
            if arg_count < max_parameter && max_parameter < min_above {
                min_above = max_parameter;
            }
        }
        let error_node = call_error_node(program, node);
        if min_count < arg_count && arg_count < max_count {
            // Between the smallest minimum and largest maximum, matching no
            // overload exactly (Go's `No_overload_expects_0_arguments...`).
            self.error(
                program,
                error_node,
                &tsgo_diagnostics::NO_OVERLOAD_EXPECTS_0_ARGUMENTS_BUT_OVERLOADS_DO_EXIST_THAT_EXPECT_EITHER_1_OR_2_ARGUMENTS,
                &[
                    &arg_count.to_string(),
                    &max_below.to_string(),
                    &min_above.to_string(),
                ],
            );
        } else {
            let parameter_range = if min_count < max_count {
                format!("{min_count}-{max_count}")
            } else {
                min_count.to_string()
            };
            self.error(
                program,
                error_node,
                &tsgo_diagnostics::EXPECTED_0_ARGUMENTS_BUT_GOT_1,
                &[&parameter_range, &arg_count.to_string()],
            );
        }
    }

    // Returns the number of parameters of a signature (Go's `getParameterCount`,
    // 4q subset: the plain parameter count).
    //
    // DEFER(phase-4-checker-4q+): rest-parameter tuple expansion. blocked-by:
    // tuple types.
    // Go: internal/checker/relater.go:Checker.getParameterCount(1690)
    fn get_parameter_count(&self, signature: SignatureId) -> usize {
        self.signature(signature).parameters.len()
    }

    // Returns the parameter type at position `pos` of a signature (Go's
    // `getTypeAtPosition` -> `tryGetTypeAtPosition`), or `any` when no type
    // applies (an out-of-range position on a non-rest signature).
    // Go: internal/checker/relater.go:Checker.getTypeAtPosition(1754)
    pub(crate) fn get_type_at_position(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        pos: usize,
    ) -> TypeId {
        self.try_get_type_at_position(program, signature, pos)
            .unwrap_or(self.any_type)
    }

    // Reports whether a signature's last parameter is a rest parameter
    // (`...args`), keyed off the `HAS_REST_PARAMETER` flag set when the
    // signature was built (Go's `signatureHasRestParameter`).
    // Go: internal/checker/checker.go:signatureHasRestParameter(16897)
    pub(crate) fn signature_has_rest_parameter(&self, signature: SignatureId) -> bool {
        self.signature(signature)
            .flags
            .contains(SignatureFlags::HAS_REST_PARAMETER)
    }

    // Reports whether a signature has an EFFECTIVE rest parameter — one that
    // accepts any number of trailing arguments (Go's
    // `hasEffectiveRestParameter`). For the reachable subset a rest type is
    // always a non-tuple array, so this coincides with
    // `signatureHasRestParameter`.
    //
    // DEFER(phase-4-checker-later): a tuple rest with no variadic element is NOT
    // an effective rest (its arity is fixed). blocked-by: tuple types.
    // Go: internal/checker/relater.go:Checker.hasEffectiveRestParameter(1746)
    pub(crate) fn has_effective_rest_parameter(&self, signature: SignatureId) -> bool {
        self.signature_has_rest_parameter(signature)
    }

    // Instantiates a parameter type through `mapper`, deep-instantiating an
    // anonymous object/function-type parameter using `program` to resolve its
    // member types (which the program-less [`Checker::instantiate_type`] cannot
    // do, hence its anonymous-object DEFER). Everything else delegates to
    // `instantiate_type`. This is the program-aware hook the call/contextual
    // parameter-read path needs so an instantiated signature's object/callback
    // parameter (`{ v: T }` -> `{ v: number }`, `(x: T) => U` -> `(x: number)
    // => U`) has its type variables substituted.
    //
    // DEFER(phase-4-checker-C-C): nested anonymous objects inside a member type
    // (the recursive member instantiation still goes through the program-less
    // `instantiate_type`, which leaves a nested anonymous object unchanged), and
    // member optionality beyond the meaning flags. blocked-by: a fully
    // program-aware recursive `instantiateType`.
    // Go: internal/checker/checker.go:Checker.instantiateType (anonymous object arm)
    pub(crate) fn instantiate_param_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        mapper: &TypeMapper,
    ) -> TypeId {
        let Some(obj) = self.get_type(t).as_object() else {
            return self.instantiate_type(t, mapper);
        };
        // A generic type reference instantiates its type arguments through the
        // program-less path already.
        if obj.target.is_some() {
            return self.instantiate_type(t, mapper);
        }
        // An anonymous object with no members/signatures has nothing to
        // instantiate.
        if obj.properties.is_empty()
            && obj.call_signatures.is_empty()
            && obj.construct_signatures.is_empty()
            && obj.index_infos.is_empty()
        {
            return self.instantiate_type(t, mapper);
        }
        self.instantiate_anonymous_object_type(program, t, mapper)
    }

    // Deep-instantiates an anonymous object/function type: re-create each
    // property with its (program-resolved) type mapped through `mapper`, and
    // instantiate the call/construct signatures and index infos. Returns the
    // original type unchanged when nothing depends on the mapper.
    // Go: internal/checker/checker.go:Checker.instantiateAnonymousType
    fn instantiate_anonymous_object_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        mapper: &TypeMapper,
    ) -> TypeId {
        let (properties, call_signatures, construct_signatures, index_infos) = {
            let obj = self.get_type(t).as_object().expect("anonymous object");
            (
                obj.properties.clone(),
                obj.call_signatures.clone(),
                obj.construct_signatures.clone(),
                obj.index_infos.clone(),
            )
        };
        let globals = program.globals();
        let mut changed = false;
        let mut new_members = SymbolTable::default();
        let mut new_properties = Vec::with_capacity(properties.len());
        for symbol in properties {
            let name = if super::is_synthesized_symbol(symbol) {
                self.synthesized_symbol_name(symbol)
            } else {
                program.symbol(symbol).name.clone()
            };
            let member_type = get_type_of_symbol(self, program, symbol, globals);
            let instantiated = self.instantiate_type(member_type, mapper);
            if instantiated != member_type {
                changed = true;
            }
            let flags = self.resolved_symbol_flags(program, symbol);
            let new_symbol = self.new_object_literal_property(
                &name,
                flags,
                tsgo_ast::CheckFlags::empty(),
                instantiated,
            );
            new_members.insert(name, new_symbol);
            new_properties.push(new_symbol);
        }
        let new_call = self.instantiate_signature_list(&call_signatures, mapper, &mut changed);
        let new_construct =
            self.instantiate_signature_list(&construct_signatures, mapper, &mut changed);
        if !changed {
            return t;
        }
        let object = ObjectType {
            members: new_members,
            properties: new_properties,
            call_signatures: new_call,
            construct_signatures: new_construct,
            index_infos,
            ..Default::default()
        };
        self.new_object_type(ObjectFlags::ANONYMOUS, None, object)
    }

    // Instantiates each signature in `list` through `mapper`, setting `changed`
    // when any signature carries type variables to substitute.
    fn instantiate_signature_list(
        &mut self,
        list: &[SignatureId],
        mapper: &TypeMapper,
        changed: &mut bool,
    ) -> Vec<SignatureId> {
        list.iter()
            .map(|&s| {
                let instantiated = self.instantiate_signature(s, mapper);
                if instantiated != s {
                    *changed = true;
                }
                instantiated
            })
            .collect()
    }

    /// Returns the call signatures of `t` (Go's `getSignaturesOfType` for the
    /// call kind), resolving through a type reference's target.
    ///
    /// DEFER(phase-4-checker-4h+): construct signatures, union/intersection
    /// signature merging, and apparent-type signatures from primitives.
    /// blocked-by: lib globals (P6) + interface call-signature collection.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let c = Checker::new();
    /// assert!(c.get_signatures_of_type(c.string_type()).is_empty());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.getSignaturesOfType
    pub fn get_signatures_of_type(&self, t: TypeId) -> Vec<SignatureId> {
        let apparent = get_apparent_type(self, t);
        let Some(obj) = self.get_type(apparent).as_object() else {
            return Vec::new();
        };
        match obj.target {
            Some(target) => self
                .get_type(target)
                .as_object()
                .map(|o| o.call_signatures.clone())
                .unwrap_or_default(),
            None => obj.call_signatures.clone(),
        }
    }

    /// Resolves the return type of calling `signature` with `argument_types`,
    /// where `parameter_types` are the signature's parameter types.
    ///
    /// For a non-generic signature this is its declared return type; for a
    /// generic one the type parameters are inferred from the arguments (4e
    /// `infer_type_arguments`) and the return type is instantiated.
    ///
    /// DEFER(phase-4-checker-4h+): overload resolution, arg-count/arg-type
    /// diagnostics, contextual typing, and wiring a `CallExpression` through a
    /// bound program. blocked-by: a callable type built from a function/method
    /// declaration (interface call-signature collection in declared types).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, Signature, SignatureFlags};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let num = c.number_type();
    /// let mut sig = Signature::new(SignatureFlags::NONE);
    /// sig.resolved_return_type = Some(num);
    /// let sid = c.new_signature(sig);
    /// assert_eq!(c.get_return_type_of_call(p, sid, &[], &[]), num);
    /// # }
    /// ```
    ///
    /// Side effects: may infer types and allocate an instantiated signature.
    // Go: internal/checker/checker.go:Checker.getReturnTypeOfSignature + inference.go
    pub fn get_return_type_of_call(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        parameter_types: &[TypeId],
        argument_types: &[TypeId],
    ) -> TypeId {
        let type_parameters = self.signature(signature).type_parameters.clone();
        if type_parameters.is_empty() {
            return self
                .signature(signature)
                .resolved_return_type
                .unwrap_or(self.error_type);
        }
        // `infer_types(source, target)` collects candidates into the target's
        // type-parameter slots, so arguments are the sources, parameters the
        // targets.
        let inferred =
            self.infer_type_arguments(program, &type_parameters, argument_types, parameter_types);
        let mapper = TypeMapper::Array {
            sources: type_parameters,
            targets: inferred,
        };
        let instantiated = self.instantiate_signature(signature, &mapper);
        self.signature(instantiated)
            .resolved_return_type
            .unwrap_or(self.error_type)
    }

    /// Type-checks a whole source file, recording diagnostics on the checker
    /// (Go's `checkSourceFile(file)`).
    ///
    /// Works off the program retained by [`Checker::new_checker`]; an
    /// intrinsic-only checker (built via [`Checker::new`], with no retained
    /// program) is a no-op. Checking is idempotent per file (Go's
    /// `sourceFileLinks.typeChecked`), so repeated calls do not re-report.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// # fn demo(c: &mut Checker, file: tsgo_ast::NodeId) {
    /// c.check_source_file(file);
    /// # }
    /// ```
    ///
    /// Side effects: records diagnostics and allocates types.
    // Go: internal/checker/checker.go:Checker.checkSourceFile(2176)
    pub fn check_source_file(&mut self, file: NodeId) {
        // Idempotent per file (Go's `sourceFileLinks.typeChecked`).
        if !self.mark_file_checked(file) {
            return;
        }
        // The retained program is shared (`Rc`); clone the handle so the
        // statement walk can borrow it while `&mut self` accumulates diagnostics.
        let Some(program) = self.retained_program() else {
            return;
        };
        // A multi-file program hands back a single-file view for `file` (its own
        // arena + the program-wide merged symbols/globals); a single-file program
        // is its own view. The view's raw root indexes its own arena, while
        // `file` is the (possibly encoded) program file handle.
        let view = program
            .file_view(file)
            .unwrap_or_else(|| Rc::clone(&program));
        let root = view.root();
        let statements = match view.arena().data(root) {
            NodeData::SourceFile(d) => d.statements.nodes.clone(),
            _ => return,
        };
        // Reset the per-file unreachable-code state (Go clears
        // `reportedUnreachableNodes` per source file; `withinUnreachableCode` is
        // saved/restored per statement and starts `false`). The node ids are
        // file-view-local, so the set must not leak across files.
        self.within_unreachable_code = false;
        self.reported_unreachable_nodes.clear();
        self.check_grammar_source_file(view.as_ref(), root);
        for statement in statements {
            self.check_statement(view.as_ref(), statement);
        }
        // Go: `if ast.IsExternalOrCommonJSModule(sourceFile) {
        // c.checkExternalModuleExports(sourceFile.AsNode()) }` at the tail of
        // `checkSourceFile`. The binder assigns the source file a module symbol
        // (`VALUE_MODULE`) iff it is an external/CommonJS module, so the presence
        // of that file symbol is the gate; a script file has no file symbol and
        // is a no-op. (Go's other call site — `checkExportAssignment` for an
        // `export =` nested in an ambient `declare module "…"` — is DEFERRED; see
        // `check_external_module_exports`.)
        if let Some(module_symbol) = view.symbol_of_node(root) {
            self.check_external_module_exports(view.as_ref(), module_symbol);
            // Go: if IsExternalOrCommonJSModule(sourceFile) { registerForUnusedIdentifiersCheck(sourceFile) }
            self.register_for_unused_identifiers_check(root);
        }
        // Run the post-check unused-identifiers analysis (Go runs this after
        // checkSourceFileWorker completes, gated on `checkUnused`; the Rust port
        // always runs it since unused errors depend only on the compiler options).
        // Go: internal/checker/checker.go:checkSourceFile -> checkUnusedIdentifiers
        self.check_unused_identifiers(view.as_ref());
    }

    // -- resolveEntityName -------------------------------------------------------

    /// Resolves an entity name (identifier, qualified name, or property access)
    /// to its symbol through the scope chain. Reports a diagnostic when
    /// `!ignore_errors` and the name cannot be found.
    ///
    /// This is the checker-level entry point that wraps the declared-types
    /// `resolve_entity_name` (silent) with error reporting and alias resolution.
    ///
    /// DEFER(phase-4-checker): the full alias-resolution loop
    /// (`!dont_resolve_alias && symbol.flags & Alias != 0` chain), the
    /// `markSymbolOfAliasDeclarationIfTypeOnly` call, and the
    /// `PropertyAccessExpression` entity path. blocked-by:
    /// `resolveAlias` / `getTargetOfAliasDeclaration`.
    // Go: internal/checker/checker.go:Checker.resolveEntityName(15646)
    pub(crate) fn resolve_entity_name(
        &mut self,
        program: &dyn BoundProgram,
        name: NodeId,
        meaning: SymbolFlags,
        ignore_errors: bool,
        _dont_resolve_alias: bool,
        location: Option<NodeId>,
    ) -> Option<SymbolId> {
        let arena = program.arena();
        if arena.kind(name) == Kind::Identifier
            && arena.text(name).is_empty()
            && arena.loc(name).end() == arena.loc(name).pos()
        {
            return None;
        }
        match arena.kind(name) {
            Kind::Identifier => {
                let resolve_location = location.unwrap_or(name);
                let text = arena.text(name).to_string();
                let result = resolve_name(
                    program,
                    resolve_location,
                    &text,
                    meaning,
                    false,
                    program.globals().as_ref().copied(),
                );
                if result.is_none() && !ignore_errors {
                    let msg = if meaning == SymbolFlags::NAMESPACE {
                        &tsgo_diagnostics::CANNOT_FIND_NAMESPACE_0
                    } else {
                        self.get_cannot_find_name_diagnostic_for_name(program, name)
                    };
                    let suggested_lib = get_suggested_lib_for_non_existent_name(&text);
                    if suggested_lib.is_empty() {
                        self.error(program, name, msg, &[&text]);
                    } else {
                        self.error(program, name, msg, &[&text, suggested_lib]);
                    }
                } else if let Some(sym) = result {
                    if !ignore_errors {
                        let hook_location = location.unwrap_or(name);
                        self.on_successfully_resolved_symbol(program, hook_location, sym, meaning);
                    }
                }
                result
            }
            Kind::QualifiedName => {
                let (left, right) = match arena.data(name) {
                    NodeData::QualifiedName(d) => (d.left, d.right),
                    _ => return None,
                };
                let namespace = self.resolve_entity_name(
                    program,
                    left,
                    SymbolFlags::NAMESPACE,
                    ignore_errors,
                    false,
                    location,
                )?;
                let right_text = arena.text(right).to_string();
                let exports = &program.symbol(namespace).exports;
                let symbol = exports
                    .get(&right_text)
                    .copied()
                    .filter(|&sid| program.symbol(sid).flags.intersects(meaning));
                if symbol.is_none() && !ignore_errors {
                    self.report_missing_namespace_member(program, namespace, right, &right_text);
                }
                symbol
            }
            Kind::PropertyAccessExpression => {
                let (expression, prop_name) = match arena.data(name) {
                    NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
                    _ => return None,
                };
                let namespace = self.resolve_entity_name(
                    program,
                    expression,
                    SymbolFlags::NAMESPACE,
                    ignore_errors,
                    false,
                    location,
                )?;
                let prop_text = arena.text(prop_name).to_string();
                let exports = &program.symbol(namespace).exports;
                let symbol = exports
                    .get(&prop_text)
                    .copied()
                    .filter(|&sid| program.symbol(sid).flags.intersects(meaning));
                if symbol.is_none() && !ignore_errors {
                    self.report_missing_namespace_member(program, namespace, prop_name, &prop_text);
                }
                symbol
            }
            _ => None,
        }
    }

    // Reports TS2724 spelling suggestion or TS2694 for a missing namespace export.
    // Go: internal/checker/checker.go:Checker.resolveQualifiedName
    fn report_missing_namespace_member(
        &mut self,
        program: &dyn BoundProgram,
        namespace: SymbolId,
        member_name: NodeId,
        member_text: &str,
    ) {
        let ns_name = program.symbol(namespace).name.clone();
        if let Some(suggestion) =
            self.get_suggested_symbol_for_nonexistent_module(program, member_text, namespace)
        {
            let suggestion_name = program.symbol(suggestion).name.clone();
            self.error(
                program,
                member_name,
                &tsgo_diagnostics::X_0_HAS_NO_EXPORTED_MEMBER_NAMED_1_DID_YOU_MEAN_2,
                &[&ns_name, member_text, &suggestion_name],
            );
        } else {
            self.error(
                program,
                member_name,
                &tsgo_diagnostics::NAMESPACE_0_HAS_NO_EXPORTED_MEMBER_1,
                &[&ns_name, member_text],
            );
        }
    }

    // -- checkExportSpecifier ----------------------------------------------------

    /// Validates an export specifier (`{ x }` / `{ x as y }`). When no module
    /// specifier is present (`export { x }` — not `export { x } from "m"`),
    /// ensures the exported name resolves in the local scope; reports TS2304
    /// when it cannot be found.
    ///
    /// DEFER(phase-4-checker): `checkAliasSymbol`, `checkModuleExportName`,
    /// `markLinkedReferences`, `checkExternalEmitHelpers`, and the
    /// `Cannot_export_0_Only_local_declarations_can_be_exported` path.
    /// blocked-by: alias-target resolution + emit helpers + global source file
    /// detection.
    // Go: internal/checker/checker.go:Checker.checkExportSpecifier(5525)
    pub(crate) fn check_export_specifier(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        has_module_specifier: bool,
    ) {
        let (property_name, name) = match program.arena().data(node) {
            NodeData::ExportSpecifier(d) => (d.property_name, d.name),
            _ => return,
        };

        if has_module_specifier {
            return;
        }

        let exported_name = property_name.unwrap_or(name);
        if program.arena().kind(exported_name) == Kind::StringLiteral {
            return;
        }

        let text = program.arena().text(exported_name).to_string();
        let meaning =
            SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::ALIAS;
        let result = resolve_name(
            program,
            exported_name,
            &text,
            meaning,
            false,
            program.globals().as_ref().copied(),
        );
        if result.is_none() {
            self.error(
                program,
                exported_name,
                &tsgo_diagnostics::CANNOT_FIND_NAME_0,
                &[&text],
            );
        }
    }

    // -- checkExportAssignment --------------------------------------------------

    /// Validates an `export = X` / `export default X` assignment: context
    /// checks (namespace restriction) and expression checking.
    ///
    /// DEFER(phase-4-checker): `checkGrammarModuleElementContext`,
    /// `shouldCheckErasableSyntax`, `verbatimModuleSyntax` checks,
    /// `isolatedModules` re-export type-only diagnostics, and the
    /// `isIllegalExportDefaultInCJS` path. blocked-by: module format
    /// detection + type-only alias declarations.
    // Go: internal/checker/checker.go:Checker.checkExportAssignment(5549)
    fn check_export_assignment(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        is_export_equals: bool,
        expression: NodeId,
    ) {
        let arena = program.arena();
        let container = arena.parent(node);
        let container = container.and_then(|c| {
            if arena.kind(c) == Kind::SourceFile {
                Some(c)
            } else {
                arena.parent(c)
            }
        });

        if let Some(container) = container {
            if arena.kind(container) == Kind::ModuleDeclaration
                && !arena
                    .flags(container)
                    .contains(tsgo_ast::NodeFlags::AMBIENT)
            {
                if is_export_equals {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::AN_EXPORT_ASSIGNMENT_CANNOT_BE_USED_IN_A_NAMESPACE,
                        &[],
                    );
                } else {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::A_DEFAULT_EXPORT_CAN_ONLY_BE_USED_IN_AN_ECMASCRIPT_STYLE_MODULE,
                        &[],
                    );
                }
                return;
            }
        }

        // Go: `if isIdentifier(node.Expression()) { ... } else { checkExpressionCached }`
        if program.arena().kind(expression) == Kind::Identifier {
            let _sym = self.resolve_entity_name(
                program,
                expression,
                SymbolFlags::ALL,
                true, // ignoreErrors
                true, // dontResolveAlias
                Some(node),
            );
            // DEFER(phase-4-checker): getExportSymbolOfValueSymbolIfExported,
            // verbatimModuleSyntax checks, isolatedModules re-export checks.
        }
        self.check_expression(program, expression);
    }

    // Reports `TS2309` when a module that has an `export =` (export-equals) ALSO
    // exports other VALUE members — the reachable core of Go's
    // `checkExternalModuleExports`. The error is reported on the export-assignment
    // declaration (`export = X;`), whose `GetErrorRangeForNode` span is the whole
    // statement (its `end` includes the trailing `;`), trivia-skipped.
    //
    // DEFER(phase-4-checker): (1) the `hasShadowedNamespace(exportEqualsSymbol)`
    // arm — an `export = <namespace-alias>` whose aliased namespace exports
    // type/namespace members (Go's `exportAssignmentMerging3`); it needs the
    // export-equals symbol's `NamespaceModule|Alias` shape + `resolveAlias` over a
    // local namespace. (2) Go's `links.exportsChecked` guard is subsumed here by
    // the single (per-file-idempotent) source-file call site — Go needs the guard
    // because it ALSO calls this from `checkExportAssignment` (for an `export =`
    // nested in an ambient `declare module "…"`, the `exportAssignmentMerging9`
    // shape), which this round does not wire. blocked-by: ExportSpecifier/
    // namespace-alias resolution + module-declaration-body descent.
    // Go: internal/checker/checker.go:Checker.checkExternalModuleExports(5663)
    fn check_external_module_exports(
        &mut self,
        program: &dyn BoundProgram,
        module_symbol: SymbolId,
    ) {
        // exportEqualsSymbol := moduleSymbol.Exports[InternalSymbolNameExportEquals]
        let Some(export_equals_symbol) = program
            .symbol(module_symbol)
            .exports
            .get(INTERNAL_SYMBOL_NAME_EXPORT_EQUALS)
            .copied()
        else {
            return;
        };
        // An export assignment is in error if the module exports value members
        // (Go's condition (a); condition (b), `hasShadowedNamespace`, DEFERRED).
        if !self.has_exported_members_of_kind(program, module_symbol, SymbolFlags::VALUE) {
            return;
        }
        // declaration := OrElse(getDeclarationOfAliasSymbol(export=),
        //                       exportEqualsSymbol.ValueDeclaration)
        let Some(declaration) = get_declaration_of_alias_symbol(program, export_equals_symbol)
            .or_else(|| program.symbol(export_equals_symbol).value_declaration)
        else {
            return;
        };
        // Go guards with `!isTopLevelInExternalModuleAugmentation(declaration)`
        // (the export= must not be a top-level statement of an external module
        // augmentation). For THIS source-file call site that guard is provably a
        // no-op: the file symbol's `export=` entry always comes from a top-level
        // `export = X` whose parent is the `SourceFile` (a ModuleBlock-nested
        // export= belongs to a namespace/`declare module` symbol, not the file
        // symbol), and `isTopLevelInExternalModuleAugmentation` requires a
        // ModuleBlock parent. It will be wired with the DEFERRED
        // `checkExportAssignment` ambient-module call site (`declare module "…"`).
        //
        // The export-assignment node is NOT in `GetErrorRangeForNode`'s
        // declaration-narrowing group, so its span is `skipTrivia(pos)..end` over
        // the whole `export = X;` statement — trivia-skipped to the `export`
        // keyword so it byte-matches `tsc`'s `a.ts(6,1)` baseline.
        self.error_skipping_leading_trivia(
            program,
            declaration,
            &tsgo_diagnostics::AN_EXPORT_ASSIGNMENT_CANNOT_BE_USED_IN_A_MODULE_WITH_OTHER_EXPORTED_ELEMENTS,
            &[],
        );
    }

    // Reports whether `module_symbol` has any exported member of `kind` OTHER than
    // the export-equals symbol itself (Go's `hasExportedMembersOfKind`). For the
    // TS2309 conflict check `kind` is `SymbolFlags::VALUE`, so a type-only export
    // (`export type T` / `export interface I`) does NOT count — matching `tsc`
    // (the corpus `exportAssignmentMerging1`/`8` are NOT flagged).
    //
    // The Rust binder over-assigns `VALUE_MODULE` to EVERY namespace (the
    // `ValueModule`-vs-`NamespaceModule` instance-state split is DEFERRED there),
    // so a namespace whose ONLY `kind`-contributing flag is the module bit is
    // counted only when its declaration is actually instantiated — re-deriving
    // Go's binder classification (`declareModuleSymbol`: ValueModule iff
    // `GetModuleInstanceState != NonInstantiated`). This keeps a type-only
    // namespace (`export namespace Bar { export interface Baz {} }`, the
    // `exportAssignmentMerging1` trap) from over-firing while an instantiated
    // namespace still counts.
    // Go: internal/checker/checker.go:Checker.hasExportedMembersOfKind(5708)
    fn has_exported_members_of_kind(
        &self,
        program: &dyn BoundProgram,
        module_symbol: SymbolId,
        kind: SymbolFlags,
    ) -> bool {
        for (name, &symbol) in program.symbol(module_symbol).exports.iter() {
            if name.as_str() == INTERNAL_SYMBOL_NAME_EXPORT_EQUALS {
                continue;
            }
            let flags = self.get_symbol_flags(program, symbol);
            if !flags.intersects(kind) {
                continue;
            }
            // Undo the binder's over-broad `VALUE_MODULE`: when the only
            // `kind`-contributing flag is a module bit, require the namespace to
            // be a ValueModule (instantiated).
            let non_module_kind = flags & kind & !SymbolFlags::MODULE;
            if non_module_kind.is_empty()
                && flags.intersects(SymbolFlags::MODULE)
                && !self.symbol_has_value_module_declaration(program, symbol)
            {
                continue;
            }
            return true;
        }
        false
    }

    // The effective `SymbolFlags` of `symbol` (Go's `getSymbolFlags` /
    // `getSymbolFlagsEx(symbol, false, false)`).
    //
    // Reachable subset: returns the binder-assigned flags directly. Go also
    // FOLLOWS alias targets (`for symbol.Flags&Alias { flags |= resolveAlias(…)
    // .Flags }`) so an exported alias that resolves to a value counts; the only
    // alias EXPORTS in the reachable set are `export { x }` specifiers whose
    // target resolution is itself DEFERRED (`getTargetOfAliasDeclaration` returns
    // `None` for `ExportSpecifier`), so following them adds nothing — and doing so
    // here would risk emitting alias-resolution diagnostics (TS2305/TS2307) out of
    // order during the export-conflict scan. So the port reads the declared flags.
    // (Consequence: the `export { Bar }` value re-export of `exportAssignmentMerging7`
    // is not yet counted; that flip is DEFERRED with the ExportSpecifier target
    // resolution, but the symmetric `export { SomeTypeAlias }` trap correctly does
    // NOT over-fire.) blocked-by: ExportSpecifier alias-target resolution.
    // Go: internal/checker/checker.go:Checker.getSymbolFlags(16222)
    fn get_symbol_flags(&self, program: &dyn BoundProgram, symbol: SymbolId) -> SymbolFlags {
        program.symbol(symbol).flags
    }

    // Reports whether `symbol` has any `ModuleDeclaration` declaration the binder
    // would classify as a `ValueModule` (an instantiated, runtime namespace) —
    // used to undo the Rust binder's over-broad `VALUE_MODULE` assignment in the
    // TS2309 membership predicate.
    // Go: internal/binder/binder.go:Binder.declareModuleSymbol (instantiated boolean)
    fn symbol_has_value_module_declaration(
        &self,
        program: &dyn BoundProgram,
        symbol: SymbolId,
    ) -> bool {
        let arena = program.arena();
        program
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&d| arena.kind(d) == Kind::ModuleDeclaration && module_is_value_module(arena, d))
    }

    /// Checks a block statement and its nested statements (Go's `checkBlock`).
    ///
    /// Side effects: may record diagnostics and recurse into child statements.
    // Go: internal/checker/checker.go:Checker.checkBlock(3761)
    pub fn check_block(&mut self, program: &dyn BoundProgram, node: NodeId) {
        if program.arena().kind(node) == Kind::Block {
            self.check_grammar_statement_in_ambient_context(program, node);
        }
        // DEFER(phase-4-checker-4g): save/restore `flowAnalysisDisabled` around
        // function/module blocks (`IsFunctionOrModuleBlock`). blocked-by:
        // `flowAnalysisDisabled` wiring.
        self.check_source_elements(program, node);
        if program.locals(node).is_some() {
            self.register_for_unused_identifiers_check(node);
        }
    }

    /// Checks an expression statement (Go's `checkExpressionStatement`).
    ///
    /// Side effects: may record diagnostics and check the expression.
    // Go: internal/checker/checker.go:Checker.checkExpressionStatement(7297)
    pub fn check_expression_statement(&mut self, program: &dyn BoundProgram, node: NodeId) {
        self.check_grammar_statement_in_ambient_context(program, node);
        if let NodeData::ExpressionStatement(d) = program.arena().data(node) {
            self.check_expression(program, d.expression);
        }
    }

    /// Checks a labeled statement, including duplicate-label and unused-label
    /// diagnostics (Go's `checkLabeledStatement`).
    ///
    /// Side effects: may record diagnostics and recurse into the body.
    // Go: internal/checker/checker.go:Checker.checkLabeledStatement(4180)
    pub fn check_labeled_statement(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::LabeledStatement(d) = program.arena().data(node) else {
            return;
        };
        let (label, statement) = (d.label, d.statement);
        if !self.check_grammar_statement_in_ambient_context(program, node) {
            let label_text = program.arena().text(label).to_string();
            let mut current = program.arena().parent(node);
            while let Some(cur) = current {
                if is_function_like_kind(program.arena().kind(cur)) {
                    break;
                }
                if let NodeData::LabeledStatement(ld) = program.arena().data(cur) {
                    if program.arena().text(ld.label) == label_text {
                        self.grammar_error_on_node(
                            program,
                            label,
                            &tsgo_diagnostics::DUPLICATE_LABEL_0,
                            &[&label_text],
                        );
                        break;
                    }
                }
                current = program.arena().parent(cur);
            }
        }
        if program
            .arena()
            .flags(label)
            .contains(NodeFlags::UNREACHABLE)
            && self.compiler_options().allow_unused_labels != tsgo_core::tristate::Tristate::True
        {
            self.error(program, label, &tsgo_diagnostics::UNUSED_LABEL, &[]);
        }
        self.check_statement(program, statement);
    }

    /// Validates that `t` may be tested for truthiness at `node` (partial port
    /// of Go's `checkTruthinessOfType`).
    ///
    /// Reports `1345` for `void`, and `2872`/`2873` when syntactic semantics
    /// prove the expression is always truthy/falsy.
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/checker.go:Checker.checkTruthinessOfType(12809)
    pub fn check_truthiness_of_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        node: NodeId,
    ) -> TypeId {
        if self.get_type(t).flags().contains(TypeFlags::VOID) {
            self.error(
                program,
                node,
                &tsgo_diagnostics::AN_EXPRESSION_OF_TYPE_VOID_CANNOT_BE_TESTED_FOR_TRUTHINESS,
                &[],
            );
            return t;
        }
        let semantics = self.get_syntactic_truthy_semantics(program, node);
        if semantics != PredicateSemantics::Sometimes {
            let message = if semantics == PredicateSemantics::Always {
                &tsgo_diagnostics::THIS_KIND_OF_EXPRESSION_IS_ALWAYS_TRUTHY
            } else {
                &tsgo_diagnostics::THIS_KIND_OF_EXPRESSION_IS_ALWAYS_FALSY
            };
            self.error(program, node, message, &[]);
        }
        t
    }

    /// Returns syntactic truthiness semantics for `node` (partial port of Go's
    /// `getSyntacticTruthySemantics`).
    ///
    /// Side effects: none (pure read of the AST).
    // Go: internal/checker/checker.go:Checker.getSyntacticTruthySemantics(12830)
    pub fn get_syntactic_truthy_semantics(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> PredicateSemantics {
        let node = skip_outer_expressions(program, node);
        match program.arena().kind(node) {
            Kind::NumericLiteral => {
                let text = program.arena().text(node);
                if text == "0" || text == "1" {
                    PredicateSemantics::Sometimes
                } else {
                    PredicateSemantics::Always
                }
            }
            Kind::ArrayLiteralExpression
            | Kind::ArrowFunction
            | Kind::BigIntLiteral
            | Kind::ClassExpression
            | Kind::FunctionExpression
            | Kind::JsxElement
            | Kind::JsxSelfClosingElement
            | Kind::ObjectLiteralExpression
            | Kind::RegularExpressionLiteral => PredicateSemantics::Always,
            Kind::VoidExpression | Kind::NullKeyword => PredicateSemantics::Never,
            Kind::NoSubstitutionTemplateLiteral | Kind::StringLiteral => {
                if program.arena().text(node).is_empty() {
                    PredicateSemantics::Never
                } else {
                    PredicateSemantics::Always
                }
            }
            Kind::ConditionalExpression => {
                let NodeData::ConditionalExpression(d) = program.arena().data(node) else {
                    return PredicateSemantics::Sometimes;
                };
                self.get_syntactic_truthy_semantics(program, d.when_true)
                    | self.get_syntactic_truthy_semantics(program, d.when_false)
            }
            _ => PredicateSemantics::Sometimes,
        }
    }

    /// Returns syntactic nullishness semantics for `node` (port of Go's
    /// `getSyntacticNullishnessSemantics`).
    ///
    /// Side effects: none (pure read of the AST).
    // Go: internal/checker/checker.go:Checker.getSyntacticNullishnessSemantics(12892)
    pub(crate) fn get_syntactic_nullishness_semantics(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> PredicateSemantics {
        let node = skip_outer_expressions(program, node);
        match program.arena().kind(node) {
            Kind::AwaitExpression
            | Kind::CallExpression
            | Kind::TaggedTemplateExpression
            | Kind::ElementAccessExpression
            | Kind::MetaProperty
            | Kind::NewExpression
            | Kind::PropertyAccessExpression
            | Kind::YieldExpression
            | Kind::ThisKeyword => PredicateSemantics::Sometimes,
            Kind::BinaryExpression => {
                let NodeData::BinaryExpression(d) = program.arena().data(node) else {
                    return PredicateSemantics::Sometimes;
                };
                match program.arena().kind(d.operator_token) {
                    Kind::BarBarToken
                    | Kind::BarBarEqualsToken
                    | Kind::AmpersandAmpersandToken
                    | Kind::AmpersandAmpersandEqualsToken => PredicateSemantics::Sometimes,
                    Kind::CommaToken
                    | Kind::EqualsToken
                    | Kind::QuestionQuestionToken
                    | Kind::QuestionQuestionEqualsToken => {
                        self.get_syntactic_nullishness_semantics(program, d.right)
                    }
                    _ => PredicateSemantics::Never,
                }
            }
            Kind::ConditionalExpression => {
                let NodeData::ConditionalExpression(d) = program.arena().data(node) else {
                    return PredicateSemantics::Sometimes;
                };
                self.get_syntactic_nullishness_semantics(program, d.when_true)
                    | self.get_syntactic_nullishness_semantics(program, d.when_false)
            }
            Kind::NullKeyword => PredicateSemantics::Always,
            Kind::Identifier => {
                if program.arena().text(node) == "undefined" {
                    PredicateSemantics::Always
                } else {
                    PredicateSemantics::Sometimes
                }
            }
            _ => PredicateSemantics::Never,
        }
    }

    /// Reports `2871`/`2869` when syntactic nullishness semantics prove the `??`
    /// left operand is always or never nullish (Go's `checkNullishCoalesceOperandLeft`).
    ///
    /// Side effects: may record a diagnostic.
    // Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperandLeft(12880)
    pub(crate) fn check_nullish_coalesce_operand_left(
        &mut self,
        program: &dyn BoundProgram,
        left: NodeId,
    ) {
        let left_target = skip_outer_expressions(program, left);
        let semantics = self.get_syntactic_nullishness_semantics(program, left_target);
        if semantics != PredicateSemantics::Sometimes {
            let message = if semantics == PredicateSemantics::Always {
                &tsgo_diagnostics::THIS_EXPRESSION_IS_ALWAYS_NULLISH
            } else {
                &tsgo_diagnostics::RIGHT_OPERAND_OF_IS_UNREACHABLE_BECAUSE_THE_LEFT_OPERAND_IS_NEVER_NULLISH
            };
            self.error(program, left_target, message, &[]);
        }
    }

    /// Checks each statement contained in `node` (Go's `checkSourceElements`).
    ///
    /// Side effects: may record diagnostics while checking nested statements.
    // Go: internal/checker/checker.go:Checker.checkSourceElements
    fn check_source_elements(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let statements = match program.arena().data(node) {
            NodeData::Block(d) => d.list.nodes.clone(),
            NodeData::SourceFile(d) => d.statements.nodes.clone(),
            NodeData::ModuleBlock(d) => d.statements.nodes.clone(),
            _ => Vec::new(),
        };
        for statement in statements {
            self.check_statement(program, statement);
        }
    }

    // Checks a single statement (Go's `checkSourceElement` dispatch). Covers
    // grammar modifiers, expression statements, variable statements, and the
    // statement-container kinds that recurse (block / if / while / do / for /
    // for-in / for-of / try / switch), the `throw` statement, and labeled
    // statements (4p), so diagnostics nested inside them surface.
    //
    // DEFER(phase-4-checker-4r+): the `with` statement (its reachable path is
    // grammar-only — `with` always reports `1101`, which needs grammar position
    // diagnostics), module declaration bodies, the function/arrow *expression*
    // bodies (reached only through expression positions not yet descended), and
    // the rest of the statement surface. 4r descends `FunctionDeclaration` bodies
    // and `ClassDeclaration` member bodies, and checks `return <expr>` (plus the
    // annotated return-type assignability). blocked-by: grammar infrastructure
    // (`checkGrammarStatementInAmbientContext` + position diagnostics) + module
    // body checking + expression-body descent.
    // Go: internal/checker/checker.go:Checker.checkSourceElement(2223)
    fn check_statement(&mut self, program: &dyn BoundProgram, node: NodeId) {
        // Go: checkSourceElement saves/restores currentNode and resets
        // instantiationCount per statement so deeply recursive types in a single
        // statement are caught by the count limit.
        let saved_current_node = self.current_node;
        self.current_node = Some(node);
        self.instantiation_count = 0;
        // Go's `checkSourceElement` saves/restores `withinUnreachableCode` around
        // the per-node worker, and `checkSourceElementWorker` reports `TS7027`
        // (gated on `allowUnreachableCode != true`) on the first unreachable
        // statement of a subtree, then sets the flag so its descendants do not
        // re-report. Restoring on the way out keeps siblings independent.
        let saved_within_unreachable = self.within_unreachable_code;
        if !self.within_unreachable_code
            && self.compiler_options().allow_unreachable_code != tsgo_core::tristate::Tristate::True
            && self.check_source_element_unreachable(program, node)
        {
            self.within_unreachable_code = true;
        }

        // Grammar-check modifiers for statement nodes that don't have a
        // dedicated function-like handler (which calls check_grammar_modifiers
        // internally via check_grammar_function_like_declaration).
        if !matches!(
            program.arena().kind(node),
            Kind::FunctionDeclaration
                | Kind::ArrowFunction
                | Kind::FunctionExpression
                | Kind::MethodDeclaration
                | Kind::Constructor
                | Kind::GetAccessor
                | Kind::SetAccessor
        ) {
            self.check_grammar_modifiers(program, node);
        }
        // Class members carry their own modifiers (e.g. accessibility), so run
        // the grammar checks on each, then check each member (4r descends into
        // method/accessor/constructor bodies and checks property initializers so
        // nested diagnostics surface). A class expression is checked the same way
        // when reached as a statement-position expression.
        if let NodeData::ClassDeclaration(_) = program.arena().data(node) {
            self.check_class_declaration(program, node);
        }
        if matches!(program.arena().kind(node), Kind::ClassExpression) {
            let _ = self.check_class_expression(program, node);
        }
        if let NodeData::ExpressionStatement(_) = program.arena().data(node) {
            self.check_expression_statement(program, node);
        }
        if let NodeData::VariableStatement(d) = program.arena().data(node) {
            self.check_grammar_variable_declaration_list(program, d.declaration_list);
            self.check_variable_declaration_list(program, d.declaration_list);
        }
        if let NodeData::TypeAliasDeclaration(_) = program.arena().data(node) {
            self.check_type_alias_declaration(program, node);
        }
        if let NodeData::InterfaceDeclaration(_) = program.arena().data(node) {
            self.check_interface_declaration(program, node);
        }
        if let NodeData::EnumDeclaration(_) = program.arena().data(node) {
            self.check_enum_declaration(program, node);
        }
        if matches!(
            program.arena().kind(node),
            Kind::ImportDeclaration | Kind::JSImportDeclaration
        ) {
            self.check_import_declaration(program, node);
        }
        if let NodeData::Block(_) = program.arena().data(node) {
            self.check_block(program, node);
        }
        // An `if` statement checks its condition then descends into the then/else
        // branches (Go's `checkIfStatement` -> `checkSourceElement`), so nested
        // diagnostics surface.
        // DEFER(phase-4-checker-4n+): `checkTestingKnownTruthy...` and the
        // empty-then-statement diagnostic. blocked-by: strict-null-checks wiring
        // + truthiness elaboration.
        if let NodeData::IfStatement(d) = program.arena().data(node) {
            let (expression, then_statement, else_statement) =
                (d.expression, d.then_statement, d.else_statement);
            self.check_expression(program, expression);
            self.check_statement(program, then_statement);
            if let Some(else_statement) = else_statement {
                self.check_statement(program, else_statement);
            }
        }
        // A `while` loop checks its condition then descends into its body (Go's
        // `checkWhileStatement`).
        if let NodeData::WhileStatement(d) = program.arena().data(node) {
            let (expression, statement) = (d.expression, d.statement);
            self.check_expression(program, expression);
            self.check_statement(program, statement);
        }
        // A `do ... while` loop descends into its body then checks its condition
        // (Go's `checkDoStatement`).
        if let NodeData::DoStatement(d) = program.arena().data(node) {
            let (statement, expression) = (d.statement, d.expression);
            self.check_statement(program, statement);
            self.check_expression(program, expression);
        }
        // A `for` loop checks its initializer (a declaration list or an
        // expression), optional condition, optional incrementor, then descends
        // into its body (Go's `checkForStatement`).
        // DEFER(phase-4-checker-4n+): `for-in`/`for-of` statements + unused-local
        // registration. blocked-by: iterable/iterator typing + unused checks.
        if let NodeData::ForStatement(d) = program.arena().data(node) {
            let (initializer, condition, incrementor, statement) =
                (d.initializer, d.condition, d.incrementor, d.statement);
            if let Some(initializer) = initializer {
                if program.arena().kind(initializer) == Kind::VariableDeclarationList {
                    self.check_variable_declaration_list(program, initializer);
                } else {
                    self.check_expression(program, initializer);
                }
            }
            if let Some(condition) = condition {
                self.check_expression(program, condition);
            }
            if let Some(incrementor) = incrementor {
                self.check_expression(program, incrementor);
            }
            self.check_statement(program, statement);
            // Go: checkForStatement -> if node.Locals() != nil { registerForUnusedIdentifiersCheck }
            if program.locals(node).is_some() {
                self.register_for_unused_identifiers_check(node);
            }
        }
        // A `try` statement descends into its `try` block, the `catch` clause's
        // block, and the `finally` block (Go's `checkTryStatement` ->
        // `checkBlock`/`checkCatchClause`).
        // DEFER(phase-4-checker-4n+): the catch-variable declaration check and
        // catch-clause grammar. blocked-by: `checkVariableLikeDeclaration` for
        // catch variables + catch-clause grammar diagnostics.
        if let NodeData::TryStatement(d) = program.arena().data(node) {
            let (try_block, catch_clause, finally_block) =
                (d.try_block, d.catch_clause, d.finally_block);
            self.check_statement(program, try_block);
            if let Some(catch_clause) = catch_clause {
                let catch_block = match program.arena().data(catch_clause) {
                    NodeData::CatchClause(c) => Some(c.block),
                    _ => None,
                };
                if let Some(catch_block) = catch_block {
                    self.check_statement(program, catch_block);
                }
            }
            if let Some(finally_block) = finally_block {
                self.check_statement(program, finally_block);
            }
        }
        // A `switch` statement descends into each `case`/`default` clause's
        // statements (Go's `checkSwitchStatement` -> `checkSourceElements`), so
        // nested diagnostics surface.
        // DEFER(phase-4-checker-4o+): the switch-expression/case-expression typing
        // and the case-vs-switch comparability diagnostic
        // (`checkTypeComparableTo` -> 2678), duplicate-`default` grammar, and
        // fallthrough/unused checks. blocked-by: comparability error elaboration
        // + flow fallthrough + grammar.
        if let NodeData::SwitchStatement(d) = program.arena().data(node) {
            let (expression, case_block) = (d.expression, d.case_block);
            self.check_expression(program, expression);
            let clauses = match program.arena().data(case_block) {
                NodeData::CaseBlock(c) => c.clauses.nodes.clone(),
                _ => Vec::new(),
            };
            for clause in clauses {
                let (clause_expression, statements) = match program.arena().data(clause) {
                    NodeData::CaseOrDefaultClause(c) => (c.expression, c.statements.nodes.clone()),
                    _ => (None, Vec::new()),
                };
                // A `case` clause carries an expression (`default` does not).
                if let Some(clause_expression) = clause_expression {
                    self.check_expression(program, clause_expression);
                }
                for statement in statements {
                    self.check_statement(program, statement);
                }
            }
        }
        // A `for-in`/`for-of` statement checks its initializer (a declaration
        // list or an expression) and its iterated expression, then descends into
        // its body (Go's `checkForInStatement`/`checkForOfStatement`), so nested
        // diagnostics surface.
        // DEFER(phase-4-checker-4o+): the for-in LHS/RHS type diagnostics
        // (`The_left_hand_side_of_a_for_in_statement_must_be_of_type_string_or_any`
        // 2405, `The_right_hand_side_of_a_for_in_statement_must_be...` 2407) and
        // for-of iterated-element typing (`checkRightHandSideOfForOf` ->
        // assignability of the element type to the target). blocked-by:
        // `getIndexTypeOrString` + iterable/iterator typing (`Symbol.iterator`)
        // need lib globals (P6) + destructuring assignment.
        if let NodeData::ForInOrOfStatement(d) = program.arena().data(node) {
            let kind = program.arena().kind(node);
            if matches!(kind, Kind::ForInStatement | Kind::ForOfStatement) {
                self.check_grammar_for_in_or_for_of_statement(program, node);
                let (initializer, expression, statement) =
                    (d.initializer, d.expression, d.statement);
                let expression_type = self.check_expression(program, expression);
                // A for-of resolves the iterated element type from its right-hand
                // side and reports the not-iterable diagnostics (`2488`/`2489`) on
                // the iterated expression (Go's `checkForOfStatement` ->
                // `checkRightHandSideOfForOf` -> `checkIteratedTypeOrElementType`),
                // independent of whether the loop variable is annotated. The
                // resolved element type then types each (un-annotated, identifier)
                // loop variable before the body is descended into, so a body
                // reference resolves with the element type rather than `any`.
                if kind == Kind::ForOfStatement {
                    let iterable_exists = self.iterables_resolvable_via_protocol();
                    let element_type = self.check_iterated_type_or_element_type(
                        program,
                        expression_type,
                        Some(expression),
                        iterable_exists,
                    );
                    if let Some(element_type) = element_type {
                        if program.arena().kind(initializer) == Kind::VariableDeclarationList {
                            self.assign_for_of_element_types(program, initializer, element_type);
                        }
                    }
                }
                // A for-in declaration list types each (un-annotated, identifier)
                // loop variable as `string` (Go's
                // `getTypeForVariableLikeDeclaration` returns `c.stringType` for a
                // for-in `VariableDeclaration`), so a body reference resolves with
                // `string` rather than the un-annotated `any`.
                // DEFER(phase-4-checker-4af+): the `keyof T` loop-variable type
                // when the iterated expression is a type-parameter/index type
                // (Go's `getExtractStringType(getIndexType(...))`). blocked-by:
                // `getIndexType` (`keyof`) typing.
                if kind == Kind::ForInStatement
                    && program.arena().kind(initializer) == Kind::VariableDeclarationList
                {
                    self.assign_for_in_variable_types(program, initializer);
                }
                if program.arena().kind(initializer) == Kind::VariableDeclarationList {
                    self.check_variable_declaration_list(program, initializer);
                } else {
                    self.check_expression(program, initializer);
                }
                self.check_statement(program, statement);
                // Go: checkForInStatement / checkForOfStatement ->
                // if node.Locals() != nil { registerForUnusedIdentifiersCheck }
                if program.locals(node).is_some() {
                    self.register_for_unused_identifiers_check(node);
                }
            }
        }
        // A `throw` statement checks its thrown expression (Go's
        // `checkThrowStatement` -> `c.checkExpression(throwExpr)`), and validates
        // grammar (ambient context, line-break before expression).
        // Go: internal/checker/checker.go:Checker.checkThrowStatement(4198)
        if let NodeData::ThrowStatement(d) = program.arena().data(node) {
            let expression = d.expression;
            if !self.check_grammar_statement_in_ambient_context(program, node)
                && program.arena().kind(expression) == Kind::Identifier
                && program.arena().text(expression).is_empty()
            {
                self.grammar_error_on_first_token(
                    program,
                    node,
                    &tsgo_diagnostics::LINE_BREAK_NOT_PERMITTED_HERE,
                    &[],
                );
            }
            self.check_expression(program, expression);
        }
        if let NodeData::LabeledStatement(_) = program.arena().data(node) {
            self.check_labeled_statement(program, node);
        }
        // A `with` statement reports TS2410 (the with statement is not supported)
        // and checks its expression.
        // Go: internal/checker/checker.go:Checker.checkWithStatement(4129)
        if let NodeData::WithStatement(d) = program.arena().data(node) {
            let (expression, statement) = (d.expression, d.statement);
            if !self.check_grammar_statement_in_ambient_context(program, node) {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::THE_WITH_STATEMENT_IS_NOT_SUPPORTED_ALL_SYMBOLS_IN_A_WITH_BLOCK_WILL_HAVE_TYPE_ANY,
                    &[],
                );
            }
            self.check_expression(program, expression);
            self.check_statement(program, statement);
        }
        // A `break` or `continue` statement validates its grammar (ambient
        // context check). Full label-target validation (1107/1104/1115) is deferred
        // pending flow analysis.
        // Go: internal/checker/checker.go:Checker.checkBreakOrContinueStatement(4056)
        if let NodeData::BreakStatement(_) | NodeData::ContinueStatement(_) =
            program.arena().data(node)
        {
            self.check_grammar_statement_in_ambient_context(program, node);
        }
        // A function declaration's body is descended into so nested diagnostics
        // surface (Go's `checkFunctionDeclaration` -> `checkFunctionOrMethod
        // Declaration` -> `checkSourceElement(body)`). An overload-signature /
        // ambient declaration has no body and is skipped.
        // DEFER(phase-4-checker-4r+): the signature/parameter checks, unused-local
        // and implicit-return analysis, and the function/arrow expression bodies
        // (reached only through expression positions not yet descended).
        // blocked-by: signature checking + flow implicit-return + expression-body
        // descent.
        if let NodeData::FunctionDeclaration(_) = program.arena().data(node) {
            self.check_function_declaration(program, node);
        }
        // A `return <expr>` statement checks the returned expression so nested
        // diagnostics surface, and (where the enclosing function has an explicit
        // return-type annotation) checks the returned type against it (`2322`).
        // Go's `checkReturnStatement` -> `checkExpression` + `checkTypeAssignable
        // ToAndOptionallyElaborate`.
        // DEFER(phase-4-checker-4r+): contextual return-type inference for an
        // un-annotated enclosing function (Go infers from the body), the
        // generator/async unwrapping, the container-less `1108` grammar error, and
        // the implicit-return / missing-return analysis. blocked-by: contextual
        // return-type inference + generator/async awaited types (lib globals, P6)
        // + grammar infrastructure + flow reachability.
        if program.arena().kind(node) == Kind::ReturnStatement {
            self.check_return_statement(program, node);
        }
        // A `namespace N { ... }` / `module "m" { ... }` declaration descends
        // into its body (Go's `checkModuleDeclaration` -> `checkSourceElement(body)`),
        // so nested export-assignment / grammar diagnostics surface.
        // DEFER(phase-4-checker): the full `checkModuleDeclaration` (ambient checks,
        // non-identifier name checks, augmentation merge checks).
        // Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5365)
        if let NodeData::ModuleDeclaration(_) = program.arena().data(node) {
            self.check_module_declaration(program, node);
        }
        // A `ModuleBlock` (the `{ ... }` body of a namespace/module) descends
        // into its child statements.
        // Go: internal/checker/checker.go:Checker.checkModuleDeclaration -> body
        if let NodeData::ModuleBlock(d) = program.arena().data(node) {
            let statements = d.statements.nodes.clone();
            for statement in statements {
                self.check_statement(program, statement);
            }
        }
        // Go: internal/checker/checker.go:Checker.checkExportDeclaration(5460)
        if let NodeData::ExportDeclaration(_) = program.arena().data(node) {
            self.check_export_declaration(program, node);
        }
        // An `export = X` / `export default X` assignment checks the exported
        // expression and validates the context (Go's `checkExportAssignment`).
        // Go: internal/checker/checker.go:Checker.checkExportAssignment(5549)
        if let NodeData::ExportAssignment(d) = program.arena().data(node) {
            let is_export_equals = d.is_export_equals;
            let expression = d.expression;
            self.check_export_assignment(program, node, is_export_equals, expression);
        }

        // Restore the enclosing reachability state (Go's `checkSourceElement`
        // restores `c.withinUnreachableCode` after the worker returns).
        self.within_unreachable_code = saved_within_unreachable;
        self.current_node = saved_current_node;
    }

    // Checks a function expression (`function (): T { ... }`) appearing in an
    // expression position by descending into its block body so nested
    // diagnostics surface and any `return <expr>` is checked against the
    // expression's explicit return-type annotation (`2322`, reached through
    // `enclosing_explicit_return_type`'s parent walk).
    //
    // The expression's own (function) type is not yet computed; `error_type` is
    // returned as a placeholder, matching the other un-typed expression kinds.
    //
    // An un-annotated parameter is contextually typed first (4bj's
    // `contextually_check_function_expression` -> `assign_contextual_parameter_types`),
    // so a body reference to it resolves with the contextual type.
    //
    // DEFER(phase-4-checker-4bk+): the function expression's own (anonymous
    // function) type (`checkExpressionWithContextualType`), `this`-parameter
    // checking, generator/async return unwrapping, and un-annotated body
    // return-type inference. blocked-by: anonymous function typing +
    // signature/`this` machinery + awaited/iterable types (lib globals, P6) +
    // body return-type inference.
    // Go: internal/checker/checker.go:Checker.checkFunctionExpressionOrObjectLiteralMethod
    fn check_function_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        self.check_function_expression_or_object_literal_method(program, node)
    }

    // Checks an arrow function (`(): T => { ... }`) appearing in an expression
    // position by descending into its *block* body so nested diagnostics
    // surface and any `return <expr>` is checked against the arrow's explicit
    // return-type annotation (`2322`, reached through
    // `enclosing_explicit_return_type`'s parent walk).
    //
    // The arrow's own (function) type is not yet computed; `error_type` is
    // returned as a placeholder.
    //
    // A concise (non-block) expression body `(): T => expr` has no `return`
    // statement; instead the body expression itself is checked against the
    // arrow's explicit return-type annotation (`2322`), reusing the same
    // assignability/`enclosing_explicit_return_type` path as a `return <expr>`
    // (the body's parent is the arrow, so its annotation is found by the walk).
    //
    // An un-annotated parameter is contextually typed first (4bj's
    // `contextually_check_function_expression` -> `assign_contextual_parameter_types`),
    // so a body reference to it resolves with the contextual type.
    //
    // DEFER(phase-4-checker-4bk+): the arrow's own (anonymous function) type
    // (`checkExpressionWithContextualType` for an un-annotated arrow),
    // `this`-parameter checking, and generator/async unwrapping (the awaited type
    // of an async concise body against the promised return type). blocked-by:
    // anonymous function typing + signature/`this` machinery + awaited types (P6).
    // Go: internal/checker/checker.go:Checker.checkArrowFunction / checkFunctionExpressionOrObjectLiteralMethodDeferred
    fn check_arrow_function(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        self.check_function_expression_or_object_literal_method(program, node)
    }

    /// Checks a function expression, arrow function, or object-literal method:
    /// grammar, contextual parameter types, per-parameter checks, body descent,
    /// and (when un-annotated) widened return-type inference from the body.
    ///
    /// # Side effects
    ///
    /// May record diagnostics; may infer a widened return type via
    /// [`Checker::get_return_type_from_body`].
    // Go: internal/checker/checker.go:Checker.checkFunctionExpressionOrObjectLiteralMethod(10074)
    fn check_function_expression_or_object_literal_method(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        self.check_grammar_function_like_declaration(program, node);
        self.contextually_check_function_expression(program, node);
        let params = function_like_parameters(program, node);
        for (index, &param) in params.iter().enumerate() {
            self.check_parameter_declaration(program, node, index, param);
        }
        match program.arena().data(node) {
            NodeData::FunctionExpression(d) => {
                if let Some(body) = d.body {
                    if program.arena().kind(body) == Kind::Block {
                        self.check_statement(program, body);
                    }
                }
            }
            NodeData::ArrowFunction(d) => {
                let body = d.body;
                if program.arena().kind(body) == Kind::Block {
                    self.check_statement(program, body);
                } else {
                    self.check_return_statement_expression(program, body, body);
                }
            }
            NodeData::MethodDeclaration(d) => {
                if let Some(body) = d.body {
                    self.check_statement(program, body);
                }
            }
            _ => {}
        }
        if get_effective_return_type_node(program, node).is_none() {
            let _ = self.get_return_type_from_body(program, node);
        }
        if program.arena().kind(node) == Kind::MethodDeclaration {
            if let Some(symbol) = program.symbol_of_node(node) {
                return get_type_of_symbol(self, program, symbol, program.globals());
            }
        }
        self.error_type
    }

    // Types an object-literal method member: grammar, computed-name check (when
    // the caller has not already done so), function-body checking, and the
    // method's declared/call signature type (Go's `checkObjectLiteralMethod`).
    //
    // DEFER(phase-4-checker-later): `instantiateTypeWithSingleGenericCallSignature`
    // for generic object-literal methods. blocked-by: generic call-site inference.
    // Go: internal/checker/checker.go:Checker.checkObjectLiteralMethod(13771)
    fn check_object_literal_method(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> TypeId {
        self.check_grammar_function_like_declaration(program, node);
        self.check_function_expression_or_object_literal_method(program, node)
    }

    // Checks a function-like declaration's return-type annotation when it is a
    // type predicate (`x is T` / `asserts x`): the reachable slice of Go's
    // `checkSignatureDeclaration` -> `checkSourceElement(node.Type())` ->
    // `checkTypePredicate`. The predicate is reached constructively from the
    // declaration whose return type it is (the declaration IS Go's
    // `getTypePredicateParent(node)`), so this does not depend on the node's
    // parent pointer.
    //
    // Go: internal/checker/checker.go:Checker.checkSignatureDeclaration (Type())
    fn check_return_type_predicate(
        &mut self,
        program: &dyn BoundProgram,
        params: &[NodeId],
        return_type: Option<NodeId>,
    ) {
        let Some(return_type) = return_type else {
            return;
        };
        if program.arena().kind(return_type) != Kind::TypePredicate {
            return;
        }
        self.check_type_predicate(program, params, return_type);
    }

    // Reports `TS1225: Cannot find parameter '{0}'.` when a (non-`this`) type
    // predicate names a parameter the enclosing signature does not have (Go's
    // `checkTypePredicate`, the `parameterIndex < 0` arm).
    //
    // The predicate parameter index is the position of the first parameter whose
    // top-level identifier name equals the predicate name (Go computes the same
    // index over `signature.parameters` in `createTypePredicateFromTypePredicate
    // Node`; a `this` parameter and binding-pattern parameters carry names that
    // never equal a written predicate name, so a name-text scan over the
    // declaration's parameter nodes yields the same found/not-found verdict).
    // When the index is found (`>= 0`) nothing is reported here. When it is not
    // found, the binding-pattern guard (`checkIfTypePredicateVariableIsDeclared
    // InBindingPattern`, which reports `TS1230` instead) runs first; only if it
    // reports nothing does `TS1225` fire.
    //
    // DEFER(phase-4-checker): the `parameterIndex >= 0` arm — the rest-parameter
    // reference (`TS1229`) and the predicate-type-assignable-to-its-parameter-
    // type chain (`TS1226`) — needs resolved signature parameter types; and the
    // `getTypePredicateParent == nil` arm (`TS1228`, a predicate outside return-
    // type position) is unreachable here because the predicate is always the
    // return type of the declaration it is checked from. blocked-by: signature
    // parameter-type resolution + a generic type-node `checkSourceElement`
    // dispatch.
    // Go: internal/checker/checker.go:Checker.checkTypePredicate(3037)
    fn check_type_predicate(
        &mut self,
        program: &dyn BoundProgram,
        params: &[NodeId],
        node: NodeId,
    ) {
        let NodeData::TypePredicate(d) = program.arena().data(node) else {
            return;
        };
        let parameter_name = d.parameter_name;
        // A `this`-typed predicate (`this is T` / `asserts this`) has no
        // parameter-name identifier to resolve (Go skips both `TypePredicateKind
        // This` and `TypePredicateKindAssertsThis`).
        if program.arena().kind(parameter_name) == Kind::ThisType {
            return;
        }
        let predicate_name = program.arena().text(parameter_name).to_string();
        // The predicate parameter index: found when some parameter's top-level
        // identifier name equals the predicate name (Go's `FindIndex(signature
        // .parameters, p.Name == name) >= 0`).
        let found = params.iter().any(|&param| {
            matches!(program.arena().data(param), NodeData::ParameterDeclaration(p)
                if program.arena().kind(p.name) == Kind::Identifier
                    && program.arena().text(p.name) == predicate_name)
        });
        if found {
            return;
        }
        // The name does not match a plain parameter; before reporting it as
        // missing, Go checks whether it names an element of a binding-pattern
        // parameter (reporting `TS1230` instead).
        let mut has_reported_error = false;
        for &param in params {
            let pattern = match program.arena().data(param) {
                NodeData::ParameterDeclaration(p) => p.name,
                _ => continue,
            };
            if matches!(
                program.arena().kind(pattern),
                Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
            ) && self.check_if_type_predicate_variable_is_declared_in_binding_pattern(
                program,
                pattern,
                parameter_name,
                &predicate_name,
            ) {
                has_reported_error = true;
                break;
            }
        }
        if !has_reported_error {
            // Go's `c.error(parameterName, ...)` spans the name via
            // `GetErrorRangeForNode` (skips the leading trivia between the
            // preceding `:`/`asserts` and the name identifier).
            self.error_skipping_leading_trivia(
                program,
                parameter_name,
                &tsgo_diagnostics::CANNOT_FIND_PARAMETER_0,
                &[&predicate_name],
            );
        }
    }

    // Reports `TS1230: A type predicate cannot reference element '{0}' in a
    // binding pattern.` when the predicate name is declared as an element of the
    // binding pattern `pattern` (recursing into nested patterns). Returns whether
    // an error was reported, so the caller suppresses `TS1225`.
    // Go: internal/checker/checker.go:Checker.checkIfTypePredicateVariableIsDeclaredInBindingPattern(3091)
    fn check_if_type_predicate_variable_is_declared_in_binding_pattern(
        &mut self,
        program: &dyn BoundProgram,
        pattern: NodeId,
        predicate_variable_node: NodeId,
        predicate_variable_name: &str,
    ) -> bool {
        let elements = match program.arena().data(pattern) {
            NodeData::ObjectBindingPattern(d) | NodeData::ArrayBindingPattern(d) => {
                d.elements.nodes.clone()
            }
            _ => return false,
        };
        for element in elements {
            let name = match program.arena().data(element) {
                NodeData::BindingElement(d) => d.name,
                _ => None,
            };
            let Some(name) = name else {
                continue;
            };
            if program.arena().kind(name) == Kind::Identifier
                && program.arena().text(name) == predicate_variable_name
            {
                self.error_skipping_leading_trivia(
                    program,
                    predicate_variable_node,
                    &tsgo_diagnostics::A_TYPE_PREDICATE_CANNOT_REFERENCE_ELEMENT_0_IN_A_BINDING_PATTERN,
                    &[predicate_variable_name],
                );
                return true;
            }
            if matches!(
                program.arena().kind(name),
                Kind::ArrayBindingPattern | Kind::ObjectBindingPattern
            ) && self.check_if_type_predicate_variable_is_declared_in_binding_pattern(
                program,
                name,
                predicate_variable_node,
                predicate_variable_name,
            ) {
                return true;
            }
        }
        false
    }

    /// Checks a `return` statement (Go's `checkReturnStatement` reachable subset):
    /// grammar in ambient context, then assignability of the returned expression
    /// to the enclosing function's explicit return-type annotation (`2322`).
    ///
    /// # Side effects
    ///
    /// May record diagnostics.
    // Go: internal/checker/checker.go:Checker.checkReturnStatement(4062)
    fn check_return_statement(&mut self, program: &dyn BoundProgram, node: NodeId) {
        if self.check_grammar_statement_in_ambient_context(program, node) {
            return;
        }
        let NodeData::ReturnStatement(d) = program.arena().data(node) else {
            return;
        };
        if let Some(expression) = d.expression {
            self.check_return_statement_expression(program, node, expression);
        }
    }

    // Checks a `return <expr>` expression assignability (Go's
    // `checkReturnExpression` / the expression half of `checkReturnStatement`):
    // the returned expression is always checked; when the enclosing function-like
    // declaration carries an explicit return-type annotation (reachable via 4q's
    // signature machinery), the returned expression's type must be assignable to
    // that annotated return type, else `2322`.
    //
    // DEFER(phase-4-checker-4r+): contextual return-type inference for an
    // un-annotated function, generator/async return unwrapping, and the
    // void/never special cases. blocked-by: contextual return-type inference +
    // generator/async awaited types (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.checkReturnExpression
    fn check_return_statement_expression(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        expression: NodeId,
    ) {
        let expression_type = self.check_expression(program, expression);
        // Only check assignability when the enclosing function has an explicit
        // return-type annotation; otherwise the return type would need
        // body-based inference, which is deferred.
        let Some(return_type) = self.enclosing_explicit_return_type(program, node) else {
            return;
        };
        if !self.is_type_assignable_to(program, expression_type, return_type) {
            let generalized = self.generalized_source_for_error(expression_type, return_type);
            let source_str = super::nodebuilder::type_to_string(self, program, generalized);
            let target_str = super::nodebuilder::type_to_string(self, program, return_type);
            self.error(
                program,
                node,
                &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
                &[source_str.as_str(), target_str.as_str()],
            );
        }
    }

    // Returns the explicit annotated return type of the nearest enclosing
    // function-like declaration of `node`, or `None` when there is none (the
    // function is un-annotated, or no function-like ancestor exists). Mirrors the
    // return-type half of Go's `getSignatureFromDeclaration` reachable in 4q (the
    // annotation's type via `get_type_from_type_node`).
    //
    // DEFER(phase-4-checker-4r+): function/arrow expressions' contextual return
    // types. blocked-by: contextual typing.
    // Go: internal/checker/checker.go:getContainingFunctionOrClassStaticBlock + getReturnTypeOfSignature
    fn enclosing_explicit_return_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Option<TypeId> {
        let mut current = program.arena().parent(node);
        while let Some(id) = current {
            if program.arena().kind(id) == Kind::GetAccessor {
                if let Some(symbol) = program.symbol_of_node(id) {
                    return get_explicit_accessor_return_type(
                        self,
                        program,
                        symbol,
                        program.globals(),
                    );
                }
                return None;
            }
            if let Some(return_type_node) = get_effective_return_type_node(program, id) {
                return Some(get_type_from_type_node(
                    self,
                    program,
                    return_type_node,
                    None,
                ));
            }
            if is_function_like_kind(program.arena().kind(id)) {
                return None;
            }
            current = program.arena().parent(id);
        }
        None
    }

    /// Reports when a function-like declaration with an explicit non-void return
    /// type can fall through without returning (`TS2366` under `strictNullChecks`).
    ///
    /// # Side effects
    ///
    /// May push diagnostics.
    // Go: internal/checker/checker.go:Checker.checkAllCodePathsInNonVoidFunctionReturnOrThrow(3704)
    fn check_all_code_paths_in_non_void_function_return_or_throw(
        &mut self,
        program: &dyn BoundProgram,
        fn_node: NodeId,
        return_type: TypeId,
    ) {
        if self.maybe_type_of_kind(return_type, TypeFlags::VOID)
            || self
                .get_type(return_type)
                .flags()
                .intersects(TypeFlags::ANY | TypeFlags::UNDEFINED)
        {
            return;
        }
        let Some(body) = function_like_body(program, fn_node) else {
            return;
        };
        if program.arena().kind(body) != Kind::Block {
            return;
        }
        let flags = program.arena().flags(fn_node);
        if !flags.contains(NodeFlags::HAS_IMPLICIT_RETURN) {
            return;
        }
        if !self.strict_null_checks() {
            return;
        }
        if self.is_type_assignable_to(program, self.undefined_type(), return_type) {
            return;
        }
        let error_node = get_effective_return_type_node(program, fn_node).unwrap_or(fn_node);
        self.error(
            program,
            error_node,
            &tsgo_diagnostics::FUNCTION_LACKS_ENDING_RETURN_STATEMENT_AND_RETURN_TYPE_DOES_NOT_INCLUDE_UNDEFINED,
            &[],
        );
    }

    // Checks a `typeof expr` expression. The result type is `string` (Go's
    // `typeofType` is the union of typeof literal strings, but for checking
    // purposes it is always a subtype of `string`; the simplified port returns
    // `string`).
    // Go: internal/checker/checker.go:Checker.checkTypeOfExpression(10577)
    fn check_typeof_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let expression = match program.arena().data(node) {
            NodeData::TypeOfExpression(d) => d.expression,
            _ => return self.error_type,
        };
        self.check_expression(program, expression);
        self.string_type
    }

    // Checks a `void expr` expression. The expression is checked for side-effect
    // diagnostics, and the result is always `undefined`.
    // Go: internal/checker/checker.go:Checker.checkVoidExpression(10799)
    fn check_void_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let expression = match program.arena().data(node) {
            NodeData::VoidExpression(d) => d.expression,
            _ => return self.error_type,
        };
        self.check_expression(program, expression);
        self.undefined_type()
    }

    // Checks a `delete expr` expression. Validates that the operand is a
    // property access or element access, then returns `boolean`.
    // Go: internal/checker/checker.go:Checker.checkDeleteExpression(10763)
    fn check_delete_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let expression = match program.arena().data(node) {
            NodeData::DeleteExpression(d) => d.expression,
            _ => return self.error_type,
        };
        self.check_expression(program, expression);
        let expr = skip_parentheses(program, expression);
        if !is_access_expression(program.arena().kind(expr)) {
            self.error(
                program,
                expr,
                &tsgo_diagnostics::THE_OPERAND_OF_A_DELETE_OPERATOR_MUST_BE_A_PROPERTY_REFERENCE,
                &[],
            );
            return self.boolean_type;
        }
        if let NodeData::PropertyAccessExpression(d) = program.arena().data(expr) {
            if is_private_identifier_name_node(program.arena(), d.name) {
                self.error(
                    program,
                    expr,
                    &tsgo_diagnostics::THE_OPERAND_OF_A_DELETE_OPERATOR_CANNOT_BE_A_PRIVATE_IDENTIFIER,
                    &[],
                );
            }
        }
        if let Some(symbol) =
            super::symbols_query::get_symbol_at_location(self, program, expr, program.globals())
        {
            let symbol = get_export_symbol_of_value_symbol_if_exported(program, symbol);
            if is_readonly_symbol(self, program, symbol) {
                self.error(
                    program,
                    expr,
                    &tsgo_diagnostics::THE_OPERAND_OF_A_DELETE_OPERATOR_CANNOT_BE_A_READ_ONLY_PROPERTY,
                    &[],
                );
            } else {
                self.check_delete_expression_must_be_optional(program, expr, symbol);
            }
        }
        self.boolean_type
    }

    // Go: internal/checker/checker.go:Checker.checkDeleteExpressionMustBeOptional(10784)
    fn check_delete_expression_must_be_optional(
        &mut self,
        program: &dyn BoundProgram,
        expr: NodeId,
        symbol: SymbolId,
    ) {
        if !self.strict_null_checks() {
            return;
        }
        let t = get_type_of_symbol(self, program, symbol, program.globals());
        let flags = self.get_type(t).flags();
        if flags.intersects(TypeFlags::ANY_OR_UNKNOWN | TypeFlags::NEVER) {
            return;
        }
        // DEFER(phase-4-checker): `exactOptionalPropertyTypes` uses
        // `SymbolFlags::OPTIONAL` instead of `has_type_facts(IS_UNDEFINED)`.
        let is_optional = self.has_type_facts(t, TypeFacts::IS_UNDEFINED);
        if !is_optional {
            self.error(
                program,
                expr,
                &tsgo_diagnostics::THE_OPERAND_OF_A_DELETE_OPERATOR_MUST_BE_OPTIONAL,
                &[],
            );
        }
    }

    // Checks a class-like declaration's heritage relations (the reachable
    // monomorphic subset of Go's `checkClassLikeDeclaration`):
    //
    // - `implements` satisfaction (2420): the class instance type must be
    //   assignable to each implemented interface; else
    //   `Class_0_incorrectly_implements_interface_1`.
    // - `extends` compatibility (2415): the (derived) class instance type must be
    //   assignable to its base class instance type; else
    //   `Class_0_incorrectly_extends_base_class_1` (this surfaces an incompatible
    //   property/method override structurally).
    //
    // The class instance (declared) type already merges inherited base members
    // (`getDeclaredTypeOfClassOrInterface`), so the structural relation engine
    // catches missing/incompatible members directly. The diagnostic prints the
    // class and base/interface via `type_to_string` (a named class/interface
    // instance type renders as its symbol name), matching Go's
    // `c.TypeToString(typeWithThis)` / `c.TypeToString(baseWithThis)` arguments
    // and the `core.OrElse(node.Name(), node)` error node.
    //
    // For the monomorphic case `getTypeWithThisArgument(t, nil, false)` returns
    // `t` (the type is neither a generic reference nor an intersection, and no
    // apparent type is requested), so `typeWithThis == classType` and
    // `baseWithThis == baseType`.
    //
    // DEFER(phase-4-checker-4bm+): the nested 2741/2322 diagnostic chain on
    // member-specific extends errors, the override-modifier walk (`checkKindsOfPropertyMemberOverrides` /
    // `checkMembersForOverrideModifier`), `implements` on a non-object type
    // (2422), base-type accessibility
    // (private constructor, 2654), mixins / type-variable base constructors,
    // generic base classes with type arguments (the `getTypeWithThisArgument`
    // type-argument substitution beyond the monomorphic case), abstract-class
    // instantiation, `super()` requirements, and the index-constraint /
    // property-initialization checks. blocked-by: a diagnostic-producing relation
    // (`checkTypeRelatedToEx` with chains) for the member elaboration + the
    // static/constructor type of a class value symbol + override-modifier
    // resolution + generic type-argument instantiation through `this`.
    /// Checks that all declarations of an interface/class symbol share
    /// identical type parameter lists (TS2428).
    ///
    /// Called once per symbol when the declared type is first resolved. If the
    /// parameter lists differ in count, name, constraint, or default, every
    /// conflicting declaration gets an error.
    // Go: internal/checker/checker.go:Checker.checkTypeParameterListsIdentical(4389)
    pub fn check_type_parameter_lists_identical(
        &mut self,
        program: &dyn BoundProgram,
        symbol: SymbolId,
    ) {
        let decls = &program.symbol(symbol).declarations;
        if decls.len() <= 1 {
            return;
        }
        if self.type_parameter_lists_checked.contains(&symbol) {
            return;
        }
        self.type_parameter_lists_checked.insert(symbol);

        let class_or_interface_decls: Vec<NodeId> = decls
            .iter()
            .copied()
            .filter(|&d| {
                matches!(
                    program.arena().kind(d),
                    Kind::ClassDeclaration | Kind::InterfaceDeclaration
                )
            })
            .collect();
        if class_or_interface_decls.len() <= 1 {
            return;
        }

        let globals = program.globals();
        let t = get_declared_type_of_symbol(self, program, symbol, globals);
        let Some(obj) = self.get_type(t).as_object() else {
            return;
        };
        let mut target_params = obj.type_parameters.clone();
        target_params.dedup();

        if !self.are_type_parameters_identical(program, &class_or_interface_decls, &target_params) {
            let name = super::nodebuilder::symbol_to_string(self, program, symbol);
            for &decl in &class_or_interface_decls {
                let name_node = match program.arena().data(decl) {
                    NodeData::ClassDeclaration(d)
                    | NodeData::ClassExpression(d)
                    | NodeData::InterfaceDeclaration(d) => d.name.unwrap_or(decl),
                    _ => decl,
                };
                self.error(
                    program,
                    name_node,
                    &tsgo_diagnostics::ALL_DECLARATIONS_OF_0_MUST_HAVE_IDENTICAL_TYPE_PARAMETERS,
                    &[&name],
                );
            }
        }
    }

    /// Checks whether all declarations share the same type parameters in
    /// name, count, constraint, and default.
    // Go: internal/checker/checker.go:Checker.areTypeParametersIdentical(4417)
    fn are_type_parameters_identical(
        &mut self,
        program: &dyn BoundProgram,
        declarations: &[NodeId],
        target_parameters: &[TypeId],
    ) -> bool {
        let max_count = target_parameters.len();
        let min_count = get_min_type_argument_count(self, program, target_parameters);

        for &decl in declarations {
            let source_params = type_parameter_nodes(program.arena(), decl);
            if source_params.len() < min_count || source_params.len() > max_count {
                return false;
            }
            for (i, &source) in source_params.iter().enumerate() {
                let target = target_parameters[i];
                let source_data = match program.arena().data(source) {
                    NodeData::TypeParameterDeclaration(d) => d,
                    _ => return false,
                };
                let target_name = self
                    .get_type(target)
                    .as_type_parameter()
                    .and_then(|tp| tp.symbol)
                    .map(|s| program.symbol(s).name.as_str())
                    .unwrap_or("");
                if program.arena().text(source_data.name) != target_name {
                    return false;
                }
                if let Some(constraint_node) = source_data.constraint {
                    if let Some(target_constraint) =
                        get_constraint_of_type_parameter(self, program, target)
                    {
                        let globals = program.globals();
                        let source_constraint =
                            get_type_from_type_node(self, program, constraint_node, globals);
                        if !self.is_type_identical_to(program, source_constraint, target_constraint)
                        {
                            return false;
                        }
                    }
                }
                if let Some(default_node) = source_data.default_type {
                    if let Some(target_default) =
                        get_default_from_type_parameter(self, program, target)
                    {
                        let globals = program.globals();
                        let source_default =
                            get_type_from_type_node(self, program, default_node, globals);
                        if !self.is_type_identical_to(program, source_default, target_default) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// Runs the grammar decorator check on each decorator modifier of `node`.
    ///
    /// In Go, `checkDecorators` iterates `node.ModifierNodes()` and calls
    /// `checkDecorator` → `checkGrammarDecorator` on each decorator. The full
    /// type-level decorator checking (return-type assignability) is deferred;
    /// here we only run the grammar validation.
    // Go: internal/checker/checker.go:Checker.checkDecorators(5992)
    fn check_decorators_on_node(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let modifiers = super::grammar::modifier_nodes_pub(program.arena(), node);
        for m in modifiers {
            if program.arena().kind(m) == Kind::Decorator {
                self.check_grammar_decorator(program, m);
            }
        }
    }

    fn check_class_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::ClassDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let name = d.name;
        let members = d.members.nodes.clone();
        if name.is_none()
            && !modifier_flags_of(program.arena(), node).contains(tsgo_ast::ModifierFlags::DEFAULT)
        {
            self.grammar_error_on_first_token(program, node, &tsgo_diagnostics::A_CLASS_DECLARATION_WITHOUT_THE_DEFAULT_MODIFIER_MUST_HAVE_A_NAME, &[]);
        }
        self.check_decorators_on_node(program, node);
        self.check_grammar_class_like_declaration(program, node);
        self.check_class_like_declaration(program, node);
        for member in members {
            if !matches!(
                program.arena().kind(member),
                Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor
            ) {
                self.check_grammar_modifiers(program, member);
            }
            self.check_decorators_on_node(program, member);
            self.check_class_member(program, member);
        }
        self.check_class_method_overload_static_consistency(program, node);
        self.register_for_unused_identifiers_check(node);
    }

    // Checks a class expression in an expression position (Go's
    // `checkClassExpression`): heritage/duplicate-member checks, then member
    // bodies. External-helper checks are deferred.
    // Go: internal/checker/checker.go:Checker.checkClassExpression(10007)
    fn check_class_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let NodeData::ClassExpression(d) = program.arena().data(node) else {
            return self.error_type;
        };
        let members = d.members.nodes.clone();
        self.check_decorators_on_node(program, node);
        self.check_grammar_class_like_declaration(program, node);
        self.check_class_like_declaration(program, node);
        for member in members {
            if !matches!(
                program.arena().kind(member),
                Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor
            ) {
                self.check_grammar_modifiers(program, member);
            }
            self.check_decorators_on_node(program, member);
            self.check_class_member(program, member);
        }
        self.check_class_method_overload_static_consistency(program, node);
        self.register_for_unused_identifiers_check(node);
        // DEFER(phase-4-checker-later): `checkClassExpressionExternalHelpers`.
        program
            .symbol_of_node(node)
            .map(|sym| get_type_of_symbol(self, program, sym, program.globals()))
            .unwrap_or(self.error_type)
    }

    fn check_interface_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::InterfaceDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let type_parameters = d.type_parameters.clone();
        let members = d.members.nodes.clone();
        let name = d.name;
        self.check_grammar_modifiers(program, node);
        self.check_grammar_type_parameter_defaults(program, type_parameters);
        if let Some(name) = name {
            self.check_type_name_is_reserved(
                program,
                name,
                &tsgo_diagnostics::INTERFACE_NAME_CANNOT_BE_0,
            );
        }
        self.check_object_type_for_duplicate_declarations(program, node);
        for member in &members {
            let mk = program.arena().kind(*member);
            if mk == Kind::IndexSignature {
                self.check_grammar_index_signature(program, *member);
            } else if mk == Kind::PropertySignature {
                self.check_grammar_property(program, *member);
            }
        }
        if let Some(sym) = program.symbol_of_node(node) {
            self.check_type_parameter_lists_identical(program, sym);
            if !self.declared_type_links.get(sym).index_signatures_checked {
                self.declared_type_links.get(sym).index_signatures_checked = true;
                self.check_type_for_duplicate_index_signatures(program, node);
            }
            // Go: internal/checker/checker.go:Checker.checkInterfaceDeclaration(4978)
            if !self.declared_type_links.get(sym).interface_checked {
                self.declared_type_links.get(sym).interface_checked = true;
                let globals = program.globals();
                let t = get_declared_type_of_symbol(self, program, sym, globals);
                let type_with_this = t;
                let error_node = name.unwrap_or(node);
                if self.check_inherited_properties_are_identical(program, t, sym, error_node) {
                    let base_types = self
                        .get_type(t)
                        .as_object()
                        .map(|o| o.base_types.clone())
                        .unwrap_or_default();
                    for base_type in base_types {
                        if !self.is_type_assignable_to(program, type_with_this, base_type) {
                            let t_str = super::nodebuilder::type_to_string(self, program, type_with_this);
                            let base_str =
                                super::nodebuilder::type_to_string(self, program, base_type);
                            self.error(
                                program,
                                error_node,
                                &tsgo_diagnostics::INTERFACE_0_INCORRECTLY_EXTENDS_INTERFACE_1,
                                &[t_str.as_str(), base_str.as_str()],
                            );
                        }
                    }
                    self.check_index_constraints(program, t, sym, false);
                }
            }
        }
        // Go: internal/checker/checker.go:Checker.checkInterfaceDeclaration(4991)
        for heritage_element in self.extends_heritage_elements(program, node) {
            let expression = match program.arena().data(heritage_element) {
                NodeData::ExpressionWithTypeArguments(e) => e.expression,
                _ => continue,
            };
            if !is_entity_name_expression(program.arena(), expression)
                || program
                    .arena()
                    .flags(expression)
                    .contains(NodeFlags::OPTIONAL_CHAIN)
            {
                self.error(
                    program,
                    expression,
                    &tsgo_diagnostics::AN_INTERFACE_CAN_ONLY_EXTEND_AN_IDENTIFIER_SLASHQUALIFIED_NAME_WITH_OPTIONAL_TYPE_ARGUMENTS,
                    &[],
                );
            }
            self.check_heritage_type_reference_node(program, heritage_element);
            if let Some(base_type) =
                self.resolve_heritage_clause_type(program, heritage_element)
            {
                if base_type != self.error_type() && !self.is_valid_base_type(base_type) {
                    self.error(
                        program,
                        heritage_element,
                        &tsgo_diagnostics::AN_INTERFACE_CAN_ONLY_EXTEND_AN_OBJECT_TYPE_OR_INTERSECTION_OF_OBJECT_TYPES_WITH_STATICALLY_KNOWN_MEMBERS,
                        &[],
                    );
                }
            }
        }
        self.register_for_unused_identifiers_check(node);
    }
    fn check_type_alias_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::TypeAliasDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let (type_parameters, type_node, name) = (d.type_parameters.clone(), d.type_node, d.name);
        self.check_grammar_modifiers(program, node);
        self.check_type_name_is_reserved(
            program,
            name,
            &tsgo_diagnostics::TYPE_ALIAS_NAME_CANNOT_BE_0,
        );
        self.check_grammar_type_parameter_defaults(program, type_parameters.clone());
        if let Some(ref tps) = type_parameters {
            for tp in &tps.nodes {
                self.check_type_parameter_declaration(program, *tp);
            }
        }
        self.check_type_node(program, type_node);
        self.register_for_unused_identifiers_check(node);
    }
    fn check_enum_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::EnumDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let members = d.members.nodes.clone();
        let in_ambient = program
            .arena()
            .flags(node)
            .contains(NodeFlags::AMBIENT);
        self.check_grammar_modifiers(program, node);
        if should_check_erasable_syntax(program, node) && !in_ambient {
            self.error(
                program,
                node,
                &tsgo_diagnostics::THIS_SYNTAX_IS_NOT_ALLOWED_WHEN_ERASABLESYNTAXONLY_IS_ENABLED,
                &[],
            );
        }
        self.compute_enum_member_values(program, node);
        for member in &members {
            self.check_enum_member(program, *member);
        }
        let Some(enum_symbol) = program.symbol_of_node(node) else {
            return;
        };
        if !self.declared_type_links.get(enum_symbol).enum_checked {
            self.declared_type_links.get(enum_symbol).enum_checked = true;
            let decls = program.symbol(enum_symbol).declarations.clone();
            if decls.len() > 1 {
                let this_is_const = is_enum_const(program, node);
                for decl in &decls {
                    if program.arena().kind(*decl) == Kind::EnumDeclaration
                        && is_enum_const(program, *decl) != this_is_const
                    {
                        if let Some(dn) = enum_decl_name(program, *decl) {
                            self.error(
                                program,
                                dn,
                                &tsgo_diagnostics::ENUM_DECLARATIONS_MUST_ALL_BE_CONST_OR_NON_CONST,
                                &[],
                            );
                        }
                    }
                }
            }
            let mut seen_missing = false;
            for decl in &decls {
                if program.arena().kind(*decl) != Kind::EnumDeclaration {
                    continue;
                }
                let dm = match program.arena().data(*decl) {
                    NodeData::EnumDeclaration(ed) => ed.members.nodes.clone(),
                    _ => continue,
                };
                if dm.is_empty() {
                    continue;
                }
                let first = dm[0];
                let has_init = matches!(program.arena().data(first), NodeData::EnumMember(em) if em.initializer.is_some());
                if !has_init {
                    if seen_missing {
                        let nn = match program.arena().data(first) {
                            NodeData::EnumMember(em) => em.name,
                            _ => first,
                        };
                        self.error(program, nn, &tsgo_diagnostics::IN_AN_ENUM_WITH_MULTIPLE_DECLARATIONS_ONLY_ONE_DECLARATION_CAN_OMIT_AN_INITIALIZER_FOR_ITS_FIRST_ENUM_ELEMENT, &[]);
                    } else {
                        seen_missing = true;
                    }
                }
            }
        }
    }
    fn check_enum_member(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::EnumMember(d) = program.arena().data(node) else {
            return;
        };
        if program.arena().kind(d.name) == Kind::PrivateIdentifier {
            self.error(
                program,
                node,
                &tsgo_diagnostics::AN_ENUM_MEMBER_CANNOT_BE_NAMED_WITH_A_PRIVATE_IDENTIFIER,
                &[],
            );
        }
        if let Some(init) = d.initializer {
            self.check_enum_member_initializer_shifts(program, init);
        }
    }

    fn check_enum_member_initializer_shifts(
        &mut self,
        program: &dyn BoundProgram,
        expr: NodeId,
    ) {
        let node = skip_parentheses(program, expr);
        if let NodeData::BinaryExpression(d) = program.arena().data(node) {
            let operator = program.arena().kind(d.operator_token);
            self.check_shift_simplification(
                program,
                node,
                operator,
                d.operator_token,
                d.left,
                d.right,
            );
            self.check_enum_member_initializer_shifts(program, d.left);
            self.check_enum_member_initializer_shifts(program, d.right);
        }
    }
    fn check_module_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::ModuleDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let (body, name, keyword) = (d.body, d.name, d.keyword);
        let is_global_augmentation = is_global_scope_augmentation(program, node);
        if let Some(body) = body {
            self.check_statement(program, body);
            if !is_global_augmentation {
                self.register_for_unused_identifiers_check(node);
            }
        }
        let in_ambient = program
            .arena()
            .flags(node)
            .contains(tsgo_ast::NodeFlags::AMBIENT);
        if is_global_augmentation && !in_ambient {
            self.error(
                program,
                name,
                &tsgo_diagnostics::AUGMENTATIONS_FOR_THE_GLOBAL_SCOPE_SHOULD_HAVE_DECLARE_MODIFIER_UNLESS_THEY_APPEAR_IN_ALREADY_AMBIENT_CONTEXT,
                &[],
            );
        }
        let is_ambient_external = is_ambient_module(program, node);
        let context_error = if is_ambient_external {
            &tsgo_diagnostics::AN_AMBIENT_MODULE_DECLARATION_IS_ONLY_ALLOWED_AT_THE_TOP_LEVEL_IN_A_FILE
        } else {
            &tsgo_diagnostics::A_NAMESPACE_DECLARATION_IS_ONLY_ALLOWED_AT_THE_TOP_LEVEL_OF_A_NAMESPACE_OR_MODULE
        };
        if self.check_grammar_module_element_context(program, node, context_error) {
            return;
        }
        self.check_grammar_modifiers(program, node);
        if program.arena().kind(name) == Kind::Identifier && keyword == Kind::ModuleKeyword {
            self.error(program, name, &tsgo_diagnostics::A_NAMESPACE_DECLARATION_SHOULD_NOT_BE_DECLARED_USING_THE_MODULE_KEYWORD_PLEASE_USE_THE_NAMESPACE_KEYWORD_INSTEAD, &[]);
        }
        if !in_ambient && program.arena().kind(name) == Kind::StringLiteral {
            self.grammar_error_on_node(
                program,
                name,
                &tsgo_diagnostics::ONLY_AMBIENT_MODULES_CAN_USE_QUOTED_NAMES,
                &[],
            );
        }
        let Some(module_symbol) = program.symbol_of_node(node) else {
            return;
        };
        let module_is_instantiated = is_instantiated_module(
            program.arena(),
            node,
            program.compiler_options().should_preserve_const_enums(),
        );
        if program.symbol(module_symbol)
            .flags
            .intersects(SymbolFlags::VALUE_MODULE)
            && !in_ambient
            && module_is_instantiated
        {
            if should_check_erasable_syntax(program, node) {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::THIS_SYNTAX_IS_NOT_ALLOWED_WHEN_ERASABLESYNTAXONLY_IS_ENABLED,
                    &[],
                );
            }
            if program.compiler_options().get_isolated_modules()
                && source_file_of(program, node).is_some_and(|sf| {
                    matches!(
                        program.arena().data(sf),
                        NodeData::SourceFile(d) if d.external_module_indicator.is_none()
                    )
                })
            {
                let flag_name = get_isolated_modules_like_flag_name(program);
                self.error(
                    program,
                    name,
                    &tsgo_diagnostics::NAMESPACES_ARE_NOT_ALLOWED_IN_GLOBAL_SCRIPT_FILES_WHEN_0_IS_ENABLED_IF_THIS_FILE_IS_NOT_INTENDED_TO_BE_A_GLOBAL_SCRIPT_SET_MODULEDETECTION_TO_FORCE_OR_ADD_AN_EMPTY_EXPORT_STATEMENT,
                    &[flag_name.as_str()],
                );
            }
            if program.symbol(module_symbol).declarations.len() > 1 {
                if let Some(first_class_or_func) =
                    get_first_non_ambient_class_or_function_declaration(program, module_symbol)
                {
                    let node_file = source_file_handle_of_node(program, node);
                    let class_file = source_file_handle_of_node(program, first_class_or_func);
                    if node_file != class_file {
                        self.error(
                            program,
                            name,
                            &tsgo_diagnostics::A_NAMESPACE_DECLARATION_CANNOT_BE_IN_A_DIFFERENT_FILE_FROM_A_CLASS_OR_FUNCTION_WITH_WHICH_IT_IS_MERGED,
                            &[],
                        );
                    } else if node_file == class_file
                        && program.arena().loc(node).pos()
                            < program.arena().loc(first_class_or_func).pos()
                    {
                        self.error(
                            program,
                            name,
                            &tsgo_diagnostics::A_NAMESPACE_DECLARATION_CANNOT_BE_LOCATED_PRIOR_TO_A_CLASS_OR_FUNCTION_WITH_WHICH_IT_IS_MERGED,
                            &[],
                        );
                    }
                }
            }
        }
        if is_ambient_external && !is_external_module_augmentation(program, node) {
            let parent = program.arena().parent(node);
            if parent.is_some_and(|p| is_global_source_file(program, p)) {
                if is_global_augmentation {
                    self.error(
                        program,
                        name,
                        &tsgo_diagnostics::AUGMENTATIONS_FOR_THE_GLOBAL_SCOPE_CAN_ONLY_BE_DIRECTLY_NESTED_IN_EXTERNAL_MODULES_OR_AMBIENT_MODULE_DECLARATIONS,
                        &[],
                    );
                } else if program.arena().kind(name) == Kind::StringLiteral
                    && tsgo_tspath::is_external_module_name_relative(program.arena().text(name))
                {
                    self.error(
                        program,
                        name,
                        &tsgo_diagnostics::AMBIENT_MODULE_DECLARATION_CANNOT_SPECIFY_RELATIVE_MODULE_NAME,
                        &[],
                    );
                }
            } else if is_global_augmentation {
                self.error(
                    program,
                    name,
                    &tsgo_diagnostics::AUGMENTATIONS_FOR_THE_GLOBAL_SCOPE_CAN_ONLY_BE_DIRECTLY_NESTED_IN_EXTERNAL_MODULES_OR_AMBIENT_MODULE_DECLARATIONS,
                    &[],
                );
            } else {
                self.error(
                    program,
                    name,
                    &tsgo_diagnostics::AMBIENT_MODULES_CANNOT_BE_NESTED_IN_OTHER_MODULES_OR_NAMESPACES,
                    &[],
                );
            }
        }
    }
    fn check_function_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::FunctionDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let (params, return_type, body) = (d.parameters.nodes.clone(), d.type_node, d.body);
        self.check_grammar_function_like_declaration(program, node);
        self.check_function_or_method_declaration(program, node);
        self.check_return_type_predicate(program, &params, return_type);
        for (index, &param) in params.iter().enumerate() {
            self.check_parameter_declaration(program, node, index, param);
        }
        if let Some(body) = body {
            self.check_statement(program, body);
        }
        self.register_for_unused_identifiers_check(node);
    }
    fn check_method_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::MethodDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let (name, params, body) = (d.name, d.parameters.nodes.clone(), d.body);
        self.check_grammar_function_like_declaration(program, node);
        self.check_function_or_method_declaration(program, node);
        if d.asterisk_token.is_some()
            && program.arena().kind(name) == Kind::Identifier
            && program.arena().text(name) == "constructor"
        {
            self.error(
                program,
                name,
                &tsgo_diagnostics::CLASS_CONSTRUCTOR_MAY_NOT_BE_A_GENERATOR,
                &[],
            );
        }
        for param in &params {
            self.check_parameter(program, *param);
        }
        if let Some(body) = body {
            self.check_statement(program, body);
        }
        if has_abstract_modifier(program.arena(), node) && body.is_some() {
            let ns = program.arena().text(name).to_string();
            self.error(program, node, &tsgo_diagnostics::METHOD_0_CANNOT_HAVE_AN_IMPLEMENTATION_BECAUSE_IT_IS_MARKED_ABSTRACT, &[&ns]);
        }
        if program.arena().kind(name) == Kind::PrivateIdentifier
            && get_containing_class(program, node).is_none()
        {
            self.error(
                program,
                node,
                &tsgo_diagnostics::PRIVATE_IDENTIFIERS_ARE_NOT_ALLOWED_OUTSIDE_CLASS_BODIES,
                &[],
            );
        }
    }
    fn check_constructor_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::ConstructorDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let (params, body) = (d.parameters.nodes.clone(), d.body);
        self.check_grammar_constructor_type_parameters(program, node);
        self.check_grammar_constructor_type_annotation(program, node);
        for param in &params {
            self.check_parameter(program, *param);
        }
        if let Some(body) = body {
            self.check_statement(program, body);
        }
        if let Some(sym) = program.symbol_of_node(node) {
            self.check_function_or_constructor_symbol(program, sym);
        }
        let Some(body) = body else {
            return;
        };
        let Some(class_decl) = program.arena().parent(node) else {
            return;
        };
        if get_extends_heritage_clause_element(program, class_decl).is_none() {
            return;
        }
        let class_extends_null = self.class_declaration_extends_null(program, class_decl);
        let super_call = find_first_super_call(program, body);
        if let Some(super_call) = super_call {
            if class_extends_null {
                self.error(
                    program,
                    super_call,
                    &tsgo_diagnostics::A_CONSTRUCTOR_CANNOT_CONTAIN_A_SUPER_CALL_WHEN_ITS_CLASS_EXTENDS_NULL,
                    &[],
                );
            }
            let super_call_should_be_root_level = !self.compiler_options().get_emit_standard_class_fields()
                && (class_has_initialized_property_or_private_identifier(program, class_decl)
                    || constructor_has_parameter_property(program, node));
            if super_call_should_be_root_level {
                if !super_call_is_root_level_in_constructor(program, super_call, body) {
                    self.error(
                        program,
                        super_call,
                        &tsgo_diagnostics::A_SUPER_CALL_MUST_BE_A_ROOT_LEVEL_STATEMENT_WITHIN_A_CONSTRUCTOR_OF_A_DERIVED_CLASS_THAT_CONTAINS_INITIALIZED_PROPERTIES_PARAMETER_PROPERTIES_OR_PRIVATE_IDENTIFIERS,
                        &[],
                    );
                } else if find_super_call_statement_after_super_or_this_reference(
                    program, node, body,
                )
                .is_none()
                {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::A_SUPER_CALL_MUST_BE_THE_FIRST_STATEMENT_IN_THE_CONSTRUCTOR_TO_REFER_TO_SUPER_OR_THIS_WHEN_A_DERIVED_CLASS_CONTAINS_INITIALIZED_PROPERTIES_PARAMETER_PROPERTIES_OR_PRIVATE_IDENTIFIERS,
                        &[],
                    );
                }
            }
        } else if !class_extends_null {
            self.error(
                program,
                node,
                &tsgo_diagnostics::CONSTRUCTORS_FOR_DERIVED_CLASSES_MUST_CONTAIN_A_SUPER_CALL,
                &[],
            );
        }
    }
    fn check_accessor_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let (name, params, body) = match program.arena().data(node) {
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                (d.name, d.parameters.nodes.clone(), d.body)
            }
            _ => return,
        };
        self.check_grammar_function_like_declaration(program, node);
        self.check_grammar_accessor(program, node);
        self.check_accessor_pair_consistency(program, node);
        if program.arena().kind(node) == Kind::GetAccessor {
            let flags = program.arena().flags(node);
            if !flags.contains(NodeFlags::AMBIENT)
                && body.is_some()
                && flags.contains(NodeFlags::HAS_IMPLICIT_RETURN)
                && !flags.contains(NodeFlags::HAS_EXPLICIT_RETURN)
            {
                self.error(
                    program,
                    name,
                    &tsgo_diagnostics::A_GET_ACCESSOR_MUST_RETURN_A_VALUE,
                    &[],
                );
            }
        }
        if program.arena().kind(name) == Kind::ComputedPropertyName {
            self.check_computed_property_name(program, name);
        }
        if program.arena().kind(name) == Kind::Identifier
            && program.arena().text(name) == "constructor"
            && get_containing_class(program, node).is_some()
        {
            self.error(
                program,
                name,
                &tsgo_diagnostics::CLASS_CONSTRUCTOR_MAY_NOT_BE_AN_ACCESSOR,
                &[],
            );
        }
        for param in &params {
            self.check_parameter(program, *param);
        }
        if let Some(symbol) = program.symbol_of_node(node) {
            let globals = program.globals();
            let return_type =
                super::declared_types::get_type_of_accessors(self, program, symbol, globals);
            if program.arena().kind(node) == Kind::GetAccessor {
                self.check_all_code_paths_in_non_void_function_return_or_throw(
                    program, node, return_type,
                );
            }
        }
        if let Some(body) = body {
            self.check_statement(program, body);
        }
    }
    fn check_property_declaration_full(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::PropertyDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let (name, initializer) = (d.name, d.initializer);
        self.check_grammar_property(program, node);
        self.check_property_declaration(program, node);
        if has_abstract_modifier(program.arena(), node) && initializer.is_some() {
            let ns = program.arena().text(name).to_string();
            self.error(program, node, &tsgo_diagnostics::PROPERTY_0_CANNOT_HAVE_AN_INITIALIZER_BECAUSE_IT_IS_MARKED_ABSTRACT, &[&ns]);
        }
        if has_accessor_modifier(program.arena(), node) {
            if let Some(symbol) = program.symbol_of_node(node) {
                let globals = program.globals();
                super::declared_types::get_type_of_accessors(self, program, symbol, globals);
            }
        }
    }
    fn check_type_parameter_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let NodeData::TypeParameterDeclaration(d) = program.arena().data(node) else {
            return;
        };
        let name = d.name;
        if let Some(expr) = d.expression {
            self.grammar_error_on_first_token(program, expr, &tsgo_diagnostics::TYPE_EXPECTED, &[]);
        }
        if let Some(c) = d.constraint {
            self.check_type_node(program, c);
        }
        if let Some(dt) = d.default_type {
            self.check_type_node(program, dt);
        }
        self.check_type_name_is_reserved(
            program,
            name,
            &tsgo_diagnostics::TYPE_PARAMETER_NAME_CANNOT_BE_0,
        );
    }
    fn check_type_name_is_reserved(
        &mut self,
        program: &dyn BoundProgram,
        name: NodeId,
        message: &'static Message,
    ) {
        match program.arena().text(name) {
            "any" | "unknown" | "never" | "number" | "bigint" | "boolean" | "string" | "symbol"
            | "void" | "object" | "undefined" => {
                self.error(program, name, message, &[program.arena().text(name)]);
            }
            _ => {}
        }
    }
    #[allow(dead_code)]
    fn container_allows_block_scoped_variable(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let Some(parent) = program.arena().parent(node) else {
            return true;
        };
        matches!(
            program.arena().kind(parent),
            Kind::SourceFile
                | Kind::ModuleBlock
                | Kind::Block
                | Kind::CaseClause
                | Kind::DefaultClause
        )
    }

    // Go: internal/checker/checker.go:Checker.getBaseConstructorTypeOfClass(16816)
    pub(crate) fn get_base_constructor_type_of_class(
        &mut self,
        program: &dyn BoundProgram,
        class_type: TypeId,
    ) -> TypeId {
        let Some(obj) = self.get_type(class_type).as_object() else {
            return self.error_type();
        };
        if let Some(cached) = obj.resolved_base_constructor_type {
            return cached;
        }
        let Some(sym) = self.get_type(class_type).symbol else {
            return self.error_type();
        };
        let base_type_node = program
            .symbol(sym)
            .declarations
            .iter()
            .find_map(|&decl| get_extends_heritage_clause_element(program, decl));
        let Some(base_type_node) = base_type_node else {
            self.set_resolved_base_constructor_type(class_type, self.undefined_type());
            return self.undefined_type();
        };
        let expression = match program.arena().data(base_type_node) {
            NodeData::ExpressionWithTypeArguments(e) => e.expression,
            _ => {
                self.set_resolved_base_constructor_type(class_type, self.error_type());
                return self.error_type();
            }
        };
        let base_constructor_type = self.check_expression(program, expression);
        if !self.get_type(base_constructor_type).flags().contains(TypeFlags::ANY)
            && base_constructor_type != self.null_type()
            && !self.is_constructor_type(program, base_constructor_type)
        {
            let type_str =
                super::nodebuilder::type_to_string(self, program, base_constructor_type);
            self.error(
                program,
                expression,
                &tsgo_diagnostics::TYPE_0_IS_NOT_A_CONSTRUCTOR_FUNCTION_TYPE,
                &[type_str.as_str()],
            );
            self.set_resolved_base_constructor_type(class_type, self.error_type());
            return self.error_type();
        }
        self.set_resolved_base_constructor_type(class_type, base_constructor_type);
        base_constructor_type
    }

    /// Reports whether `class_decl` extends `null` (base constructor type is null).
    // Go: internal/checker/checker.go:Checker.classDeclarationExtendsNull(12231)
    fn class_declaration_extends_null(
        &mut self,
        program: &dyn BoundProgram,
        class_decl: NodeId,
    ) -> bool {
        let Some(sym) = program.symbol_of_node(class_decl) else {
            return false;
        };
        let class_type = get_declared_type_of_symbol(self, program, sym, program.globals());
        let base_constructor_type =
            self.get_base_constructor_type_of_class(program, class_type);
        base_constructor_type == self.null_type()
    }

    fn set_resolved_base_constructor_type(&mut self, class_type: TypeId, resolved: TypeId) {
        if let Some(obj) = self.types.get_mut(class_type).as_object_mut() {
            obj.resolved_base_constructor_type = Some(resolved);
        }
    }

    // Go: internal/checker/checker.go:Checker.isConstructorType(16872)
    fn is_constructor_type(&mut self, program: &dyn BoundProgram, t: TypeId) -> bool {
        if !self
            .collect_construct_signatures_of_type(program, t)
            .is_empty()
        {
            return true;
        }
        self.get_type(t).symbol.is_some_and(|sym| {
            self.resolved_symbol_flags(program, sym)
                .contains(SymbolFlags::CLASS)
        })
    }

    // Go: internal/checker/checker.go:Checker.getSignaturesOfType (construct, intersection)
    fn collect_construct_signatures_of_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> Vec<SignatureId> {
        let apparent = get_apparent_type(self, t);
        if let Some(members) = self
            .get_type(apparent)
            .intersection_types()
            .map(|types| types.to_vec())
        {
            let mut sigs = Vec::new();
            for member in members {
                sigs.extend(self.collect_construct_signatures_of_type(program, member));
            }
            return sigs;
        }
        let mut sigs = self.get_construct_signatures_of_type(apparent);
        if sigs.is_empty() {
            if let Some(sym) = self.get_type(apparent).symbol {
                if !super::is_es_module_symbol(sym)
                    && self
                        .resolved_symbol_flags(program, sym)
                        .contains(SymbolFlags::CLASS)
                {
                    sigs = self.construct_signatures_of_class_symbol(program, sym);
                }
            }
        }
        sigs
    }

    fn construct_signatures_of_class_symbol(
        &mut self,
        program: &dyn BoundProgram,
        class_sym: SymbolId,
    ) -> Vec<SignatureId> {
        get_class_construct_signatures(self, program, class_sym)
    }

    // Go: internal/checker/checker.go:Checker.getConstructorsForTypeArguments(19141)
    fn get_constructors_for_type_arguments(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        type_arg_nodes: &[NodeId],
    ) -> Vec<SignatureId> {
        let type_arg_count = type_arg_nodes.len();
        self.collect_construct_signatures_of_type(program, t)
            .into_iter()
            .filter(|&sig| {
                let tps = self.signature(sig).type_parameters.clone();
                type_arg_count >= get_min_type_argument_count(self, program, &tps)
                    && type_arg_count <= tps.len()
            })
            .collect()
    }

    // Go: internal/checker/checker.go:Checker.getInstantiatedConstructorsForTypeArguments(19130)
    pub(crate) fn get_instantiated_constructors_for_type_arguments(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        base_type_node: NodeId,
    ) -> Vec<SignatureId> {
        let type_arg_nodes = type_arguments_of_heritage_node(program, base_type_node);
        let constructors =
            self.get_constructors_for_type_arguments(program, t, &type_arg_nodes);
        let type_arguments: Vec<TypeId> = type_arg_nodes
            .iter()
            .map(|&n| get_type_from_type_node(self, program, n, None))
            .collect();
        constructors
            .into_iter()
            .map(|sig| {
                if self.signature(sig).type_parameters.is_empty() {
                    sig
                } else {
                    self.get_signature_instantiation(program, sig, &type_arguments)
                }
            })
            .collect()
    }

    // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4266)
    fn check_class_like_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        self.check_class_heritage(program, node);
        self.check_object_type_for_duplicate_declarations(program, node);
        let in_ambient = program
            .arena()
            .flags(node)
            .contains(tsgo_ast::NodeFlags::AMBIENT);
        if !in_ambient {
            self.check_class_for_static_property_name_conflicts(program, node);
        }
        if let Some(sym) = program.symbol_of_node(node) {
            if !self.declared_type_links.get(sym).index_signatures_checked {
                self.declared_type_links.get(sym).index_signatures_checked = true;
                self.check_type_for_duplicate_index_signatures(program, node);
            }
        }
        let Some(symbol) = program.symbol_of_node(node) else {
            return;
        };
        let globals = program.globals();
        let class_type = get_declared_type_of_symbol(self, program, symbol, globals);
        // Monomorphic `getTypeWithThisArgument(classType, nil, false)`.
        let type_with_this = class_type;
        let class_str = super::nodebuilder::type_to_string(self, program, type_with_this);
        // Go reports on `core.OrElse(node.Name(), node)` (the class name, else the
        // class node).
        let error_node = match program.arena().data(node) {
            NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.name.unwrap_or(node),
            _ => node,
        };

        // `extends`-clause compatibility (2415). For a class the declared type's
        // `base_types` come only from its `extends` heritage (the implements
        // clause does not contribute), so a non-empty list means the class
        // extends a base class. The monomorphic `baseWithThis` is the base type.
        // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4287)
        // Mixin constructor checks when the `extends` expression is a type
        // variable (Go's `baseConstructorType.flags&TypeFlagsTypeVariable`).
        // Runs even when `base_types` is empty (monomorphic heritage resolution
        // does not yet resolve constructor-typed `extends` parameters).
        // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4312)
        if let Some(base_type_node) = get_extends_heritage_clause_element(program, node) {
            let expression = match program.arena().data(base_type_node) {
                NodeData::ExpressionWithTypeArguments(e) => Some(e.expression),
                _ => None,
            };
            if let Some(expression) = expression {
                if let Some(base_constructor_type) =
                    self.extends_expression_type_variable(program, expression)
                {
                    if !is_mixin_constructor_class(self, program, node) {
                        self.error(
                            program,
                            error_node,
                            &tsgo_diagnostics::A_MIXIN_CLASS_MUST_HAVE_A_CONSTRUCTOR_WITH_A_SINGLE_REST_PARAMETER_OF_TYPE_ANY,
                            &[],
                        );
                    } else if !has_abstract_modifier(program.arena(), node)
                        && self.has_abstract_construct_signature(program, base_constructor_type)
                    {
                        self.error(
                            program,
                            error_node,
                            &tsgo_diagnostics::A_MIXIN_CLASS_THAT_EXTENDS_FROM_A_TYPE_VARIABLE_CONTAINING_AN_ABSTRACT_CONSTRUCT_SIGNATURE_MUST_ALSO_BE_DECLARED_ABSTRACT,
                            &[],
                        );
                    }
                }
            }
        }

        let base_types = self
            .get_type(class_type)
            .as_object()
            .map(|o| o.base_types.clone())
            .unwrap_or_default();
        let base_constructor_type = self.get_base_constructor_type_of_class(program, class_type);
        let static_base_type = get_apparent_type(self, base_constructor_type);
        if let Some(base_type_node) = get_extends_heritage_clause_element(program, node) {
            let type_arg_nodes = type_arguments_of_heritage_node(program, base_type_node);
            // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4299)
            if !type_arg_nodes.is_empty() {
                for constructor in self.get_constructors_for_type_arguments(
                    program,
                    static_base_type,
                    &type_arg_nodes,
                ) {
                    self.check_type_argument_constraints_for_reference(
                        program,
                        &self.signature(constructor).type_parameters.clone(),
                        &type_arg_nodes,
                    );
                    break;
                }
            }
        }
        if let Some(&base_type) = base_types.first() {
            // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4295)
            if let Some(base_type_node) = get_extends_heritage_clause_element(program, node) {
                if let Some(base_sym) = self.get_type(base_type).symbol {
                    self.check_base_type_accessibility(program, base_sym, base_type_node);
                }
            }
            if self.is_type_assignable_to(program, type_with_this, base_type) {
                // Static-side extends (2417) runs only when the instance side is
                // assignable (Go's `else` arm after the instance check).
                // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4310)
                let static_type = get_type_of_symbol(self, program, symbol, globals);
                if let Some(base_sym) = self.get_type(base_type).symbol {
                    let static_base = get_type_of_symbol(self, program, base_sym, globals);
                    let static_base = get_apparent_type(self, static_base);
                    let static_base = get_type_without_signatures(self, static_base);
                    if !self.is_type_assignable_to(program, static_type, static_base) {
                        let static_str = format!("typeof {class_str}");
                        let base_name = super::nodebuilder::symbol_to_string(self, program, base_sym);
                        let static_base_str = format!("typeof {base_name}");
                        self.error(
                            program,
                            error_node,
                            &tsgo_diagnostics::CLASS_STATIC_SIDE_0_INCORRECTLY_EXTENDS_BASE_CLASS_STATIC_SIDE_1,
                            &[static_str.as_str(), static_base_str.as_str()],
                        );
                    }
                }
            } else {
                let base_str = super::nodebuilder::type_to_string(self, program, base_type);
                if !self.issue_member_specific_error(
                    program,
                    node,
                    type_with_this,
                    base_type,
                ) {
                    self.error(
                        program,
                        error_node,
                        &tsgo_diagnostics::CLASS_0_INCORRECTLY_EXTENDS_BASE_CLASS_1,
                        &[class_str.as_str(), base_str.as_str()],
                    );
                }
            }
            // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4334)
            self.check_kinds_of_property_member_overrides(program, node, class_type, base_type);
            // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4324)
            let static_is_class = self.get_type(static_base_type).symbol.is_some_and(|sym| {
                program.symbol(sym).flags.contains(SymbolFlags::CLASS)
            });
            let base_is_type_variable = self
                .get_type(base_constructor_type)
                .flags()
                .intersects(TypeFlags::TYPE_PARAMETER);
            if !static_is_class && !base_is_type_variable {
                if let Some(base_type_node) = get_extends_heritage_clause_element(program, node)
                {
                    let constructors = self.get_instantiated_constructors_for_type_arguments(
                        program,
                        static_base_type,
                        base_type_node,
                    );
                    let all_same = !constructors.is_empty()
                        && constructors.iter().all(|&sig| {
                            self.is_type_identical_to(
                                program,
                                self.get_return_type_of_signature(sig),
                                base_type,
                            )
                        });
                    if !all_same {
                        let expression = match program.arena().data(base_type_node) {
                            NodeData::ExpressionWithTypeArguments(e) => e.expression,
                            _ => error_node,
                        };
                        self.error(
                            program,
                            expression,
                            &tsgo_diagnostics::BASE_CONSTRUCTORS_MUST_ALL_HAVE_THE_SAME_RETURN_TYPE,
                            &[],
                        );
                    }
                }
            }
        }

        // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4337)
        self.check_members_for_override_modifier(
            program,
            node,
            class_type,
            type_with_this,
            symbol,
        );

        // `implements`-clause satisfaction (2420). Each implemented type must be
        // assignable from the class instance type.
        // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4338)
        for type_node in self.implements_heritage_elements(program, node) {
            let expression = match program.arena().data(type_node) {
                NodeData::ExpressionWithTypeArguments(e) => e.expression,
                _ => continue,
            };
            // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4340)
            if !is_entity_name_expression(program.arena(), expression)
                || program
                    .arena()
                    .flags(expression)
                    .contains(NodeFlags::OPTIONAL_CHAIN)
            {
                self.error(
                    program,
                    expression,
                    &tsgo_diagnostics::A_CLASS_CAN_ONLY_IMPLEMENT_AN_IDENTIFIER_SLASHQUALIFIED_NAME_WITH_OPTIONAL_TYPE_ARGUMENTS,
                    &[],
                );
            }
            let Some(interface_type) = self.resolve_heritage_clause_type(program, type_node) else {
                continue;
            };
            // A type that did not resolve is the error type; skip it (Go's
            // `if !c.isErrorType(t)`).
            if interface_type == self.error_type() {
                continue;
            }
            if !self.is_valid_base_type(interface_type) {
                self.error(
                    program,
                    type_node,
                    &tsgo_diagnostics::A_CLASS_CAN_ONLY_IMPLEMENT_AN_OBJECT_TYPE_OR_INTERSECTION_OF_OBJECT_TYPES_WITH_STATICALLY_KNOWN_MEMBERS,
                    &[],
                );
                continue;
            }
            // Monomorphic `baseWithThis` is the implemented interface type.
            if !self.is_type_assignable_to(program, type_with_this, interface_type) {
                let interface_str =
                    super::nodebuilder::type_to_string(self, program, interface_type);
                // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4348)
                let implements_class = self
                    .get_type(interface_type)
                    .symbol
                    .is_some_and(|sym| {
                        program.symbol(sym).flags.intersects(SymbolFlags::CLASS)
                    });
                if implements_class {
                    self.error(
                        program,
                        error_node,
                        &tsgo_diagnostics::CLASS_0_INCORRECTLY_IMPLEMENTS_CLASS_1_DID_YOU_MEAN_TO_EXTEND_1_AND_INHERIT_ITS_MEMBERS_AS_A_SUBCLASS,
                        &[class_str.as_str(), interface_str.as_str()],
                    );
                } else {
                    self.error(
                        program,
                        error_node,
                        &tsgo_diagnostics::CLASS_0_INCORRECTLY_IMPLEMENTS_INTERFACE_1,
                        &[class_str.as_str(), interface_str.as_str()],
                    );
                }
            }
        }

        // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4360)
        self.check_index_constraints(program, class_type, symbol, false);
        let static_type = get_type_of_symbol(self, program, symbol, globals);
        self.check_index_constraints(program, static_type, symbol, true);

        // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4363)
        self.check_property_initialization(program, node);
    }

    // Go: internal/checker/checker.go:Checker.checkBaseTypeAccessibility(4454)
    fn check_base_type_accessibility(
        &mut self,
        program: &dyn BoundProgram,
        base_class_sym: SymbolId,
        extends_node: NodeId,
    ) {
        let Some(class_decl) = get_class_like_declaration_of_symbol(program, base_class_sym) else {
            return;
        };
        for member in object_type_member_nodes(program, class_decl) {
            if program.arena().kind(member) != Kind::Constructor {
                continue;
            }
            if modifier_flags_of(program.arena(), member).contains(ModifierFlags::PRIVATE)
                && !is_node_within_class(program, extends_node, class_decl)
            {
                let base_name = super::nodebuilder::symbol_to_string(self, program, base_class_sym);
                self.error(
                    program,
                    extends_node,
                    &tsgo_diagnostics::CANNOT_EXTEND_A_CLASS_0_CLASS_CONSTRUCTOR_IS_MARKED_AS_PRIVATE,
                    &[base_name.as_str()],
                );
            }
            return;
        }
    }

    // Go: internal/checker/checker.go:Checker.checkKindsOfPropertyMemberOverrides(4510)
    fn check_kinds_of_property_member_overrides(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        class_type: TypeId,
        base_type: TypeId,
    ) {
        let mut missed: Vec<String> = Vec::new();
        for (name, base_prop) in get_properties_of_type(self, base_type) {
            if program
                .symbol(base_prop)
                .flags
                .intersects(SymbolFlags::PROTOTYPE)
            {
                continue;
            }
            let Some(derived_prop) = get_property_of_type(self, class_type, &name) else {
                continue;
            };
            let base_target = get_target_symbol(self, program, base_prop);
            let derived_target = get_target_symbol(self, program, derived_prop);
            if derived_target == base_target {
                let base_flags =
                    declaration_modifier_flags_from_symbol(self, program, base_prop, false);
                if !base_flags.contains(ModifierFlags::ABSTRACT) {
                    continue;
                }
                if has_abstract_modifier(program.arena(), node) {
                    continue;
                }
                missed.push(super::nodebuilder::symbol_to_string(self, program, base_prop));
                continue;
            }
            let base_flags =
                declaration_modifier_flags_from_symbol(self, program, base_prop, false);
            let derived_flags =
                declaration_modifier_flags_from_symbol(self, program, derived_prop, false);
            if base_flags.contains(ModifierFlags::PRIVATE)
                || derived_flags.contains(ModifierFlags::PRIVATE)
            {
                continue;
            }
            let base_sym = program.symbol(base_target);
            let derived_sym = program.symbol(derived_target);
            let base_property_flags = base_sym.flags & SymbolFlags::PROPERTY_OR_ACCESSOR;
            let derived_property_flags = derived_sym.flags & SymbolFlags::PROPERTY_OR_ACCESSOR;
            if !base_property_flags.is_empty() && !derived_property_flags.is_empty() {
                let overridden_instance_property = !base_property_flags
                    .intersects(SymbolFlags::PROPERTY)
                    && derived_property_flags.intersects(SymbolFlags::PROPERTY);
                let overridden_instance_accessor = base_property_flags
                    .intersects(SymbolFlags::PROPERTY)
                    && !derived_property_flags.intersects(SymbolFlags::PROPERTY);
                if overridden_instance_property || overridden_instance_accessor {
                    let error_node = override_member_error_node(program, derived_target);
                    let prop_name = super::nodebuilder::symbol_to_string(self, program, base_prop);
                    let base_name = super::nodebuilder::type_to_string(self, program, base_type);
                    let class_name = super::nodebuilder::type_to_string(self, program, class_type);
                    let message = if overridden_instance_property {
                        &tsgo_diagnostics::X_0_IS_DEFINED_AS_AN_ACCESSOR_IN_CLASS_1_BUT_IS_OVERRIDDEN_HERE_IN_2_AS_AN_INSTANCE_PROPERTY
                    } else {
                        &tsgo_diagnostics::X_0_IS_DEFINED_AS_A_PROPERTY_IN_CLASS_1_BUT_IS_OVERRIDDEN_HERE_IN_2_AS_AN_ACCESSOR
                    };
                    self.error(
                        program,
                        error_node,
                        message,
                        &[prop_name.as_str(), base_name.as_str(), class_name.as_str()],
                    );
                }
                continue;
            }
            let error_node = override_member_error_node(program, derived_target);
            let base_name = super::nodebuilder::type_to_string(self, program, base_type);
            let prop_name = super::nodebuilder::symbol_to_string(self, program, base_prop);
            let class_name = super::nodebuilder::type_to_string(self, program, class_type);
            let message = if is_prototype_property(self, program, base_target) {
                if is_prototype_property(self, program, derived_target)
                    || derived_sym.flags.intersects(SymbolFlags::PROPERTY)
                {
                    continue;
                }
                &tsgo_diagnostics::CLASS_0_DEFINES_INSTANCE_MEMBER_FUNCTION_1_BUT_EXTENDED_CLASS_2_DEFINES_IT_AS_INSTANCE_MEMBER_ACCESSOR
            } else if base_sym.flags.intersects(SymbolFlags::ACCESSOR) {
                &tsgo_diagnostics::CLASS_0_DEFINES_INSTANCE_MEMBER_ACCESSOR_1_BUT_EXTENDED_CLASS_2_DEFINES_IT_AS_INSTANCE_MEMBER_FUNCTION
            } else {
                &tsgo_diagnostics::CLASS_0_DEFINES_INSTANCE_MEMBER_PROPERTY_1_BUT_EXTENDED_CLASS_2_DEFINES_IT_AS_INSTANCE_MEMBER_FUNCTION
            };
            self.error(
                program,
                error_node,
                message,
                &[base_name.as_str(), prop_name.as_str(), class_name.as_str()],
            );
        }
        if missed.is_empty() {
            return;
        }
        let base_name = super::nodebuilder::type_to_string(self, program, base_type);
        let class_name = class_like_display_name(program, node);
        if missed.len() == 1 {
            self.error(
                program,
                node,
                &tsgo_diagnostics::NON_ABSTRACT_CLASS_0_DOES_NOT_IMPLEMENT_INHERITED_ABSTRACT_MEMBER_1_FROM_CLASS_2,
                &[class_name.as_str(), missed[0].as_str(), base_name.as_str()],
            );
        } else {
            let props = missed
                .iter()
                .map(|p| format!("'{p}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.error(
                program,
                node,
                &tsgo_diagnostics::NON_ABSTRACT_CLASS_0_IS_MISSING_IMPLEMENTATIONS_FOR_THE_FOLLOWING_MEMBERS_OF_1_COLON_2,
                &[class_name.as_str(), base_name.as_str(), props.as_str()],
            );
        }
    }

    // Resolves the type-variable (type parameter) referenced by a class `extends`
    // expression. `checkExpression` on a parameter typed with a type parameter
    // may not yield a `TYPE_PARAMETER` type id, so fall back to the parameter's
    // annotation when needed.
    fn extends_expression_type_variable(
        &mut self,
        program: &dyn BoundProgram,
        expression: NodeId,
    ) -> Option<TypeId> {
        let t = self.check_expression(program, expression);
        if self
            .get_type(t)
            .flags()
            .intersects(TypeFlags::TYPE_PARAMETER)
        {
            return Some(t);
        }
        if program.arena().kind(expression) != Kind::Identifier {
            return None;
        }
        let name = program.arena().text(expression).to_string();
        let globals = program.globals();
        if let Some(tp_sym) = resolve_name(
            program,
            expression,
            &name,
            SymbolFlags::TYPE_PARAMETER,
            false,
            globals,
        ) {
            let tp_type = get_declared_type_of_symbol(self, program, tp_sym, globals);
            if self
                .get_type(tp_type)
                .flags()
                .intersects(TypeFlags::TYPE_PARAMETER)
            {
                return Some(tp_type);
            }
        }
        let sym = resolve_name(
            program,
            expression,
            &name,
            SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
            false,
            globals,
        )?;
        let decl = program.symbol(sym).value_declaration?;
        let NodeData::ParameterDeclaration(d) = program.arena().data(decl) else {
            return None;
        };
        let type_node = d.type_node?;
        let ann = get_type_from_type_node(self, program, type_node, None);
        if self
            .get_type(ann)
            .flags()
            .intersects(TypeFlags::TYPE_PARAMETER)
        {
            Some(ann)
        } else {
            None
        }
    }

    // Go: internal/checker/checker.go:Checker.isMixinConstructorType(16885)
    fn has_abstract_construct_signature(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> bool {
        if self.type_declares_abstract_constructor(program, t) {
            return true;
        }
        if self
            .get_type(t)
            .flags()
            .intersects(TypeFlags::TYPE_PARAMETER)
        {
            if self.type_parameter_constraint_declares_abstract_constructor(program, t) {
                return true;
            }
            if let Some(constraint) = get_constraint_of_type_parameter(self, program, t) {
                return self.has_abstract_construct_signature(program, constraint);
            }
        }
        false
    }

    fn type_parameter_constraint_declares_abstract_constructor(
        &self,
        program: &dyn BoundProgram,
        type_parameter: TypeId,
    ) -> bool {
        let Some(symbol) = self
            .get_type(type_parameter)
            .as_type_parameter()
            .and_then(|d| d.symbol)
        else {
            return false;
        };
        let owner = program.view_for_symbol(symbol);
        let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
        let Some(constraint_node) = prog.symbol(symbol).declarations.iter().find_map(|&decl| {
            match prog.arena().data(decl) {
                NodeData::TypeParameterDeclaration(d) => d.constraint,
                _ => None,
            }
        }) else {
            return false;
        };
        let mut type_node = constraint_node;
        if let NodeData::TypeReference(d) = prog.arena().data(constraint_node) {
            if prog.arena().kind(d.type_name) == Kind::Identifier {
                let name = prog.arena().text(d.type_name).to_string();
                if let Some(alias_sym) = resolve_name(
                    prog,
                    d.type_name,
                    &name,
                    SymbolFlags::TYPE,
                    false,
                    prog.globals(),
                ) {
                    for &decl in &prog.symbol(alias_sym).declarations {
                        if let NodeData::TypeAliasDeclaration(ad) = prog.arena().data(decl) {
                            type_node = ad.type_node;
                            break;
                        }
                    }
                }
            }
        }
        matches!(
            prog.arena().kind(type_node),
            Kind::ConstructorType | Kind::ConstructSignature
        ) && has_abstract_modifier(prog.arena(), type_node)
    }

    fn type_declares_abstract_constructor(
        &self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> bool {
        for sig in self.get_construct_signatures_of_type(t) {
            if self
                .signature(sig)
                .flags
                .contains(SignatureFlags::ABSTRACT)
            {
                return true;
            }
            if let Some(decl) = self.signature(sig).declaration {
                if matches!(
                    program.arena().kind(decl),
                    Kind::ConstructorType | Kind::ConstructSignature
                ) && has_abstract_modifier(program.arena(), decl)
                {
                    return true;
                }
            }
        }
        let apparent = get_apparent_type(self, t);
        if let Some(obj) = self.get_type(apparent).as_object() {
            if let Some(target) = obj.target {
                return self.type_declares_abstract_constructor(program, target);
            }
        }
        let Some(sym) = self.get_type(apparent).symbol else {
            return false;
        };
        let owner = program.view_for_symbol(sym);
        let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
        for &decl in &prog.symbol(sym).declarations {
            if matches!(
                prog.arena().kind(decl),
                Kind::ConstructorType | Kind::ConstructSignature
            ) && has_abstract_modifier(prog.arena(), decl)
            {
                return true;
            }
            if let NodeData::TypeAliasDeclaration(d) = prog.arena().data(decl) {
                let type_node = d.type_node;
                if matches!(prog.arena().kind(type_node), Kind::ConstructorType | Kind::ConstructSignature)
                    && has_abstract_modifier(prog.arena(), type_node)
                {
                    return true;
                }
            }
        }
        false
    }

    // Go: internal/checker/checker.go:Checker.getSuggestedSymbolForNonexistentClassMember(4756)
    fn get_suggested_symbol_for_nonexistent_class_member(
        &mut self,
        program: &dyn BoundProgram,
        name: &str,
        base_type: TypeId,
    ) -> Option<SymbolId> {
        let candidates: Vec<SymbolId> = get_properties_of_type(self, base_type)
            .into_iter()
            .map(|(_, sym)| sym)
            .collect();
        super::name_resolution::get_spelling_suggestion_for_name(
            program,
            name,
            &candidates,
            SymbolFlags::CLASS_MEMBER,
        )
    }

    // Go: internal/checker/checker.go:Checker.getSignaturesOfType (construct kind)
    fn get_construct_signatures_of_type(&self, t: TypeId) -> Vec<SignatureId> {
        let apparent = get_apparent_type(self, t);
        let Some(obj) = self.get_type(apparent).as_object() else {
            return Vec::new();
        };
        let resolved = match obj.target {
            Some(target) => self.get_type(target).as_object(),
            None => Some(obj),
        };
        let Some(obj) = resolved else {
            return Vec::new();
        };
        obj.construct_signatures.clone()
    }

    // Go: internal/checker/checker.go:Checker.checkMembersForOverrideModifier(4678)
    fn check_members_for_override_modifier(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        class_type: TypeId,
        type_with_this: TypeId,
        class_symbol: SymbolId,
    ) {
        let globals = program.globals();
        let base_with_this = self
            .get_type(class_type)
            .as_object()
            .and_then(|o| o.base_types.first().copied());
        let base_static_type = base_with_this.and_then(|base| {
            self.get_type(base)
                .symbol
                .map(|sym| get_type_of_symbol(self, program, sym, globals))
        });
        for member in object_type_member_nodes(program, node) {
            if has_ambient_modifier(program, member) {
                continue;
            }
            if program.arena().kind(member) == Kind::Constructor {
                if let NodeData::ConstructorDeclaration(d) = program.arena().data(member) {
                    for &param in &d.parameters.nodes {
                        if is_parameter_property_declaration(program, param) {
                            self.check_member_for_override_modifier(
                                program,
                                node,
                                class_type,
                                type_with_this,
                                base_with_this,
                                base_static_type,
                                class_symbol,
                                param,
                            );
                        }
                    }
                }
            } else {
                self.check_member_for_override_modifier(
                    program,
                    node,
                    class_type,
                    type_with_this,
                    base_with_this,
                    base_static_type,
                    class_symbol,
                    member,
                );
            }
        }
    }

    // Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier(4703)
    fn check_member_for_override_modifier(
        &mut self,
        program: &dyn BoundProgram,
        class_node: NodeId,
        class_type: TypeId,
        type_with_this: TypeId,
        base_with_this: Option<TypeId>,
        base_static_type: Option<TypeId>,
        class_symbol: SymbolId,
        member: NodeId,
    ) {
        let member_has_override = has_override_modifier(program.arena(), member);
        let class_name = super::nodebuilder::type_to_string(self, program, class_type);
        if base_with_this.is_none() {
            if member_has_override {
                self.error(
                    program,
                    member,
                    &tsgo_diagnostics::THIS_MEMBER_CANNOT_HAVE_AN_OVERRIDE_MODIFIER_BECAUSE_ITS_CONTAINING_CLASS_0_DOES_NOT_EXTEND_ANOTHER_CLASS,
                    &[class_name.as_str()],
                );
            }
            return;
        }
        if !member_has_override
            && !self
                .compiler_options()
                .no_implicit_override
                .is_true()
        {
            return;
        }
        let Some(member_sym) = program.symbol_of_node(member) else {
            return;
        };
        let member_name = program.symbol(member_sym).name.clone();
        let member_is_static = has_static_modifier(program.arena(), member);
        let this_type = if member_is_static {
            get_type_of_symbol(self, program, class_symbol, program.globals())
        } else {
            type_with_this
        };
        if get_property_of_type(self, this_type, &member_name).is_none() {
            return;
        }
        let base_type = if member_is_static {
            base_static_type
        } else {
            base_with_this
        };
        let Some(base_type) = base_type else {
            return;
        };
        let base_prop = get_property_of_type(self, base_type, &member_name);
        if base_prop.is_none() && member_has_override {
            let base_name = super::nodebuilder::type_to_string(self, program, base_type);
            if let Some(suggestion) =
                self.get_suggested_symbol_for_nonexistent_class_member(program, &member_name, base_type)
            {
                let suggestion_name =
                    super::nodebuilder::symbol_to_string(self, program, suggestion);
                self.error(
                    program,
                    member,
                    &tsgo_diagnostics::THIS_MEMBER_CANNOT_HAVE_AN_OVERRIDE_MODIFIER_BECAUSE_IT_IS_NOT_DECLARED_IN_THE_BASE_CLASS_0_DID_YOU_MEAN_1,
                    &[base_name.as_str(), suggestion_name.as_str()],
                );
            } else {
                self.error(
                    program,
                    member,
                    &tsgo_diagnostics::THIS_MEMBER_CANNOT_HAVE_AN_OVERRIDE_MODIFIER_BECAUSE_IT_IS_NOT_DECLARED_IN_THE_BASE_CLASS_0,
                    &[base_name.as_str()],
                );
            }
            return;
        }
        if base_prop.is_some()
            && !member_has_override
            && self.compiler_options().no_implicit_override.is_true()
            && !program
                .arena()
                .flags(class_node)
                .contains(NodeFlags::AMBIENT)
        {
            let base_has_abstract = base_prop.is_some_and(|prop| {
                declaration_modifier_flags_from_symbol(self, program, prop, false)
                    .contains(ModifierFlags::ABSTRACT)
            });
            if !base_has_abstract {
                let base_name = super::nodebuilder::type_to_string(
                    self,
                    program,
                    base_with_this.unwrap(),
                );
                let message = if program.arena().kind(member) == Kind::Parameter {
                    &tsgo_diagnostics::THIS_PARAMETER_PROPERTY_MUST_HAVE_AN_OVERRIDE_MODIFIER_BECAUSE_IT_OVERRIDES_A_MEMBER_IN_BASE_CLASS_0
                } else {
                    &tsgo_diagnostics::THIS_MEMBER_MUST_HAVE_AN_OVERRIDE_MODIFIER_BECAUSE_IT_OVERRIDES_A_MEMBER_IN_THE_BASE_CLASS_0
                };
                self.error(program, member, message, &[base_name.as_str()]);
            } else if has_abstract_modifier(program.arena(), member) && base_has_abstract {
                let base_name = super::nodebuilder::type_to_string(
                    self,
                    program,
                    base_with_this.unwrap(),
                );
                self.error(
                    program,
                    member,
                    &tsgo_diagnostics::THIS_MEMBER_MUST_HAVE_AN_OVERRIDE_MODIFIER_BECAUSE_IT_OVERRIDES_AN_ABSTRACT_METHOD_THAT_IS_DECLARED_IN_THE_BASE_CLASS_0,
                    &[base_name.as_str()],
                );
            }
        }
    }

    // Go: internal/checker/checker.go:Checker.checkPropertyInitialization(4907)
    fn check_property_initialization(&mut self, program: &dyn BoundProgram, node: NodeId) {
        if !self.strict_null_checks()
            || !self
                .compiler_options()
                .strict_property_initialization
                .is_true()
            || program.arena().flags(node).contains(NodeFlags::AMBIENT)
        {
            return;
        }
        let constructor = find_constructor_declaration(program, node);
        let globals = program.globals();
        for member in object_type_member_nodes(program, node) {
            if has_ambient_modifier(program, member) || has_static_modifier(program.arena(), member)
            {
                continue;
            }
            if !is_property_without_initializer(program, member) {
                continue;
            }
            let Some(prop_name) = property_declaration_name_node(program, member) else {
                continue;
            };
            let Some(prop_sym) = program.symbol_of_node(member) else {
                continue;
            };
            let prop_type = get_type_of_symbol(self, program, prop_sym, globals);
            if self.is_any_or_unknown_type(prop_type) || self.contains_undefined_type(prop_type) {
                continue;
            }
            if constructor.is_none() {
                let name = declaration_name_to_string(program, prop_name);
                self.error(
                    program,
                    prop_name,
                    &tsgo_diagnostics::PROPERTY_0_HAS_NO_INITIALIZER_AND_IS_NOT_DEFINITELY_ASSIGNED_IN_THE_CONSTRUCTOR,
                    &[name.as_str()],
                );
            }
        }
    }

    fn is_any_or_unknown_type(&self, t: TypeId) -> bool {
        self.get_type(t)
            .flags()
            .intersects(TypeFlags::ANY | TypeFlags::UNKNOWN)
    }

    fn contains_undefined_type(&self, t: TypeId) -> bool {
        if t == self.undefined_type() {
            return true;
        }
        if let Some(union) = self.get_type(t).union_types() {
            return union
                .iter()
                .any(|&u| self.contains_undefined_type(u));
        }
        false
    }

    // Verifies that declared properties are assignable to applicable index
    // signatures on a class or static type (TS2411).
    //
    // DEFER(phase-4-checker-4bm+): computed property names without bindable
    // names, and inherited interface error-node selection.
    // Go: internal/checker/checker.go:Checker.checkIndexConstraints(4760)
    fn check_index_constraints(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        type_symbol: SymbolId,
        is_static_index: bool,
    ) {
        let index_infos = get_index_infos_of_type(self, t);
        if index_infos.is_empty() {
            return;
        }
        let globals = program.globals();
        for (name, prop_sym) in get_properties_of_type(self, t) {
            if is_static_index
                && program
                    .symbol(prop_sym)
                    .flags
                    .intersects(SymbolFlags::PROTOTYPE)
            {
                continue;
            }
            let prop_name_type = self.get_string_literal_type(&name);
            let prop_type = get_type_of_symbol(self, program, prop_sym, globals);
            self.check_index_constraint_for_property(
                program,
                t,
                type_symbol,
                prop_sym,
                prop_name_type,
                prop_type,
            );
        }
        if index_infos.len() > 1 {
            for &check_info_id in &index_infos {
                self.check_index_constraint_for_index_signature(
                    program,
                    t,
                    type_symbol,
                    check_info_id,
                );
            }
        }
    }

    // Go: internal/checker/checker.go:Checker.checkIndexConstraintForProperty(4787)
    fn check_index_constraint_for_property(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        type_symbol: SymbolId,
        prop_sym: SymbolId,
        prop_name_type: TypeId,
        prop_type: TypeId,
    ) {
        let declaration = program.symbol(prop_sym).value_declaration;
        if let Some(decl) = declaration {
            let name = match program.arena().data(decl) {
                NodeData::PropertyDeclaration(p) | NodeData::PropertySignature(p) => Some(p.name),
                _ => None,
            };
            if let Some(name) = name {
                if program.arena().kind(name) == Kind::PrivateIdentifier {
                    return;
                }
            }
        }
        let Some(info_id) = get_applicable_index_info(self, program, t, prop_name_type) else {
            return;
        };
        let (key_type, value_type, index_declaration) = {
            let info = self.index_info(info_id);
            (info.key_type, info.value_type, info.declaration)
        };
        let local_prop_declaration =
            if program.symbol(prop_sym).parent == Some(type_symbol) {
                declaration
            } else {
                None
            };
        let local_index_declaration = index_declaration.filter(|&decl| {
            program
                .symbol_of_node(decl)
                .is_some_and(|index_sym| program.symbol(index_sym).parent == Some(type_symbol))
        });
        let Some(error_node) = local_prop_declaration.or(local_index_declaration) else {
            return;
        };
        if self.is_type_assignable_to(program, prop_type, value_type) {
            return;
        }
        let prop_str = super::nodebuilder::symbol_to_string(self, program, prop_sym);
        let prop_type_str = super::nodebuilder::type_to_string(self, program, prop_type);
        let key_type_str = super::nodebuilder::type_to_string(self, program, key_type);
        let value_type_str = super::nodebuilder::type_to_string(self, program, value_type);
        self.error(
            program,
            error_node,
            &tsgo_diagnostics::PROPERTY_0_OF_TYPE_1_IS_NOT_ASSIGNABLE_TO_2_INDEX_TYPE_3,
            &[
                prop_str.as_str(),
                prop_type_str.as_str(),
                key_type_str.as_str(),
                value_type_str.as_str(),
            ],
        );
    }

    // Go: internal/checker/checker.go:Checker.checkIndexConstraintForIndexSignature(4833)
    fn check_index_constraint_for_index_signature(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        type_symbol: SymbolId,
        check_info_id: IndexInfoId,
    ) {
        let (check_key_type, check_value_type, check_declaration) = {
            let info = self.index_info(check_info_id);
            (info.key_type, info.value_type, info.declaration)
        };
        let applicable = get_applicable_index_infos(self, program, t, check_key_type);
        if applicable.is_empty() {
            return;
        }
        let local_check_declaration = check_declaration.filter(|&decl| {
            program
                .symbol_of_node(decl)
                .is_some_and(|sym| program.symbol(sym).parent == Some(type_symbol))
        });
        for info_id in applicable {
            if info_id == check_info_id {
                continue;
            }
            let (key_type, value_type, index_declaration) = {
                let info = self.index_info(info_id);
                (info.key_type, info.value_type, info.declaration)
            };
            let local_index_declaration = index_declaration.filter(|&decl| {
                program
                    .symbol_of_node(decl)
                    .is_some_and(|index_sym| program.symbol(index_sym).parent == Some(type_symbol))
            });
            let Some(error_node) = local_check_declaration.or(local_index_declaration) else {
                continue;
            };
            if self.is_type_assignable_to(program, check_value_type, value_type) {
                continue;
            }
            let check_key_type_str =
                super::nodebuilder::type_to_string(self, program, check_key_type);
            let check_value_type_str =
                super::nodebuilder::type_to_string(self, program, check_value_type);
            let key_type_str = super::nodebuilder::type_to_string(self, program, key_type);
            let value_type_str = super::nodebuilder::type_to_string(self, program, value_type);
            self.error(
                program,
                error_node,
                &tsgo_diagnostics::X_0_INDEX_TYPE_1_IS_NOT_ASSIGNABLE_TO_2_INDEX_TYPE_3,
                &[
                    check_key_type_str.as_str(),
                    check_value_type_str.as_str(),
                    key_type_str.as_str(),
                    value_type_str.as_str(),
                ],
            );
        }
    }

    // Checks that an object-type declaration (interface / class) does not declare
    // the same instance (or static) member name twice, or mix a property and an
    // accessor with the same name. Mirrors Go's per-name state machine over the
    // merged member symbols: a member is only a candidate once it has MERGED into
    // a symbol with more than one declaration (`len(Declarations) > 1`). State `0`
    // (unseen) records the member kind (`1` = property, `2` = accessor); a second
    // property, or a property after an accessor / an accessor after a property,
    /// Reports TS2374 ("Duplicate index signature for type '{0}'.") when a type
    /// contains more than one index signature whose parameter resolves to the
    /// same key type.
    ///
    /// TypeScript 1.0 spec (April 2014):
    /// - 3.7.4: An object type can contain at most one string index signature
    ///   and one numeric index signature.
    /// - 8.5: A class declaration can have at most one string index member
    ///   declaration and one numeric index member declaration.
    ///
    /// Late-bound index signatures (computed property names that resolve to index
    /// signatures) are intentionally excluded — Go's comment: "allow these to
    /// duplicate one another and explicit indexes".
    ///
    /// # Side effects
    ///
    /// Pushes zero or more TS2374 diagnostics.
    // Go: internal/checker/checker.go:Checker.checkTypeForDuplicateIndexSignatures(4878)
    fn check_type_for_duplicate_index_signatures(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) {
        use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_INDEX;

        let Some(node_symbol) = program.symbol_of_node(node) else {
            return;
        };
        let members = &program.symbol(node_symbol).members;
        let Some(&index_symbol) = members.get(INTERNAL_SYMBOL_NAME_INDEX) else {
            return;
        };
        let decls = &program.symbol(index_symbol).declarations;
        if decls.len() <= 1 {
            return;
        }

        let globals = program.globals();
        let mut index_signature_map: std::collections::HashMap<TypeId, Vec<NodeId>> =
            std::collections::HashMap::new();
        for &declaration in decls {
            if let NodeData::IndexSignatureDeclaration(d) = program.arena().data(declaration) {
                let parameters = &d.parameters.nodes;
                if parameters.len() == 1 {
                    let param_type_node = match program.arena().data(parameters[0]) {
                        NodeData::ParameterDeclaration(p) => p.type_node,
                        _ => None,
                    };
                    if let Some(type_node) = param_type_node {
                        let key_type = get_type_from_type_node(self, program, type_node, globals);
                        for t in self.distributed_types(key_type) {
                            index_signature_map.entry(t).or_default().push(declaration);
                        }
                    }
                }
            }
        }

        for (t, declarations) in &index_signature_map {
            if declarations.len() > 1 {
                let type_str = super::nodebuilder::type_to_string(self, program, *t);
                for &decl in declarations {
                    self.error(
                        program,
                        decl,
                        &tsgo_diagnostics::DUPLICATE_INDEX_SIGNATURE_FOR_TYPE_0,
                        &[&type_str],
                    );
                }
            }
        }
    }

    // reports `Duplicate identifier` (TS2300) on EVERY same-named member and then
    // records state `3` (reported). Two SAME-kind accessors / a method colliding
    // with anything are already flagged by the binder's `declareSymbol` excludes
    // (which do NOT let those merge), and a LEGAL get/set pair stays state `2`
    // (no error), so this checker half only reports the property-vs-property and
    // property-vs-accessor combinations the binder intentionally lets merge.
    //
    // DEFER(phase-4-checker): computed/late-bound names (the `__computed` members
    // are bound anonymously and only merge after checker late-binding), and
    // type-literal members. blocked-by: checker late-binding + type-literal
    // traversal in `check_type_node`.
    // Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3122)
    // When a class incorrectly extends a base class, try to report a
    // member-specific property incompatibility (TS2416) before falling back to
    // the broad extends error (TS2415). Go's `issueMemberSpecificError`.
    // Go: internal/checker/checker.go:Checker.issueMemberSpecificError(4467)
    fn issue_member_specific_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        type_with_this: TypeId,
        base_with_this: TypeId,
    ) -> bool {
        let globals = program.globals();
        let mut issued_member_error = false;
        for member in object_type_member_nodes(program, node) {
            if has_static_modifier(program.arena(), member) {
                continue;
            }
            let Some(declared_prop) = program.symbol_of_node(member) else {
                continue;
            };
            let sym_name = &program.symbol(declared_prop).name;
            if sym_name == INTERNAL_SYMBOL_NAME_COMPUTED {
                continue;
            }
            let Some(prop) = get_property_of_type(self, type_with_this, sym_name) else {
                continue;
            };
            let Some(base_prop) = get_property_of_type(self, base_with_this, sym_name) else {
                continue;
            };
            let prop_type = get_type_of_symbol(self, program, prop, globals);
            let base_prop_type = get_type_of_symbol(self, program, base_prop, globals);
            if !self.is_type_assignable_to(program, prop_type, base_prop_type) {
                let type_str =
                    super::nodebuilder::type_to_string(self, program, type_with_this);
                let base_str =
                    super::nodebuilder::type_to_string(self, program, base_with_this);
                let prop_str = super::nodebuilder::symbol_to_string(self, program, declared_prop);
                let report_node = member_name_node_for_duplicate(program, member)
                    .unwrap_or(node);
                self.error(
                    program,
                    report_node,
                    &tsgo_diagnostics::PROPERTY_0_IN_TYPE_1_IS_NOT_ASSIGNABLE_TO_THE_SAME_PROPERTY_IN_BASE_TYPE_2,
                    &[prop_str.as_str(), type_str.as_str(), base_str.as_str()],
                );
                issued_member_error = true;
            }
        }
        issued_member_error
    }

    fn check_object_type_for_duplicate_declarations(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) {
        self.check_auto_accessor_conflicts_with_accessors(program, node);
        let node_in_ambient = program
            .arena()
            .flags(node)
            .contains(tsgo_ast::NodeFlags::AMBIENT);
        let check_private_names = matches!(
            program.arena().kind(node),
            Kind::ClassDeclaration | Kind::ClassExpression
        );
        let mut instance_names: std::collections::HashMap<String, i32> =
            std::collections::HashMap::new();
        let mut static_names: std::collections::HashMap<String, i32> =
            std::collections::HashMap::new();
        let mut private_names: std::collections::HashMap<String, i32> =
            std::collections::HashMap::new();
        for member in object_type_member_nodes(program, node) {
            if program.arena().kind(member) == Kind::Constructor {
                self.register_constructor_parameter_property_names(
                    program,
                    node,
                    member,
                    &mut instance_names,
                );
                continue;
            }
            let symbol = program.symbol_of_node(member);
            let is_static = has_static_modifier(program.arena(), member);

            // Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3165)
            if !node_in_ambient && is_static {
                if let Some(sym) = symbol {
                    if program.symbol(sym).name == "prototype" {
                        if let Some(name_node) =
                            member_name_node_for_duplicate(program, member)
                        {
                            let class_display = class_like_display_name(program, node);
                            self.error(
                                program,
                                name_node,
                                &tsgo_diagnostics::STATIC_PROPERTY_0_CONFLICTS_WITH_BUILT_IN_PROPERTY_FUNCTION_0_OF_CONSTRUCTOR_FUNCTION_1,
                                &["prototype", "prototype", class_display.as_str()],
                            );
                        }
                    }
                }
            }

            if let Some(sym) = symbol {
                if let Some(kind) = classify_property_or_accessor(program, member) {
                    let name = program.symbol(sym).name.clone();
                    let names = if is_static {
                        &mut static_names
                    } else {
                        &mut instance_names
                    };
                    let skip_merged_duplicate_pass = !is_static
                        && self.maybe_report_parameter_property_member_conflict(
                            program, node, names, &name, kind, is_static,
                        );
                    // Only members that MERGED into one symbol can be duplicates.
                    if !skip_merged_duplicate_pass
                        && program.symbol(sym).declarations.len() > 1
                    {
                        let state = names.get(&name).copied().unwrap_or(0);
                        if state == 0 {
                            // On first occurrence just record the kind.
                            names.insert(name, kind);
                        } else if state == 1 || (state == 2 && kind != 2) {
                            // Error on a second property, or a property/accessor combination.
                            names.insert(name.clone(), 3);
                            self.report_duplicate_member_errors(program, node, &name, is_static);
                        }
                    }
                }
            }

            // Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3177)
            if check_private_names {
                if let Some(sym) = symbol {
                    if let Some(name_node) = member_name_node_for_duplicate(program, member) {
                        if program.arena().kind(name_node) == Kind::PrivateIdentifier {
                            let sym_name = program.symbol(sym).name.clone();
                            let flags = private_names.get(&sym_name).copied().unwrap_or(0);
                            if flags != 3 {
                                let new_flags = flags | if is_static { 2 } else { 1 };
                                private_names.insert(sym_name.clone(), new_flags);
                                if new_flags == 3 {
                                    self.report_duplicate_private_name_member_errors(
                                        program, node, &sym_name,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Records constructor parameter-property names for duplicate detection against
    // class members. Parameter properties do not yet merge into class-member symbols
    // at bind time, so this pass seeds the instance-name state machine before the
    // merged-symbol walk (Go `checkObjectTypeForDuplicateDeclarations` 3157-3159).
    // Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3157)
    fn register_constructor_parameter_property_names(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        constructor: NodeId,
        instance_names: &mut std::collections::HashMap<String, i32>,
    ) {
        let NodeData::ConstructorDeclaration(d) = program.arena().data(constructor) else {
            return;
        };
        for &param in &d.parameters.nodes {
            if !is_parameter_property_declaration(program, param)
                || is_parameter_property_binding_pattern(program, param)
            {
                continue;
            }
            let NodeData::ParameterDeclaration(pd) = program.arena().data(param) else {
                continue;
            };
            let Some(name) = property_name_text(program, pd.name) else {
                continue;
            };
            let state = instance_names.get(&name).copied().unwrap_or(0);
            if state == 0 {
                instance_names.insert(name, 1);
            } else if state == 1 {
                instance_names.insert(name.clone(), 3);
                self.report_duplicate_member_errors(program, node, &name, false);
            }
        }
    }

    // When a parameter property already registered `name`, report a class member
    // with an incompatible kind (property vs accessor). Returns `true` when the
    // conflict was reported and the merged-symbol pass should be skipped.
    fn maybe_report_parameter_property_member_conflict(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        names: &mut std::collections::HashMap<String, i32>,
        name: &str,
        kind: i32,
        is_static: bool,
    ) -> bool {
        let state = names.get(name).copied().unwrap_or(0);
        if state == 0 || state == 3 {
            return false;
        }
        if state == 1 || (state == 2 && kind != 2) {
            names.insert(name.to_string(), 3);
            self.report_duplicate_member_errors(program, node, name, is_static);
            return true;
        }
        false
    }

    // Auto-accessors bind as accessor symbols and conflict with standalone get/set
    // accessors at bind time (Go's `SymbolFlagsAccessorExcludes`), so they never
    // merge into one multi-declaration symbol. The merged-symbol duplicate pass
    // below therefore misses `get x` + `accessor x` combinations; flag every
    // same-named get/set/auto-accessor member when an auto-accessor coexists with
    // a standalone accessor (Go `duplicateIdentifierChecks.ts` C3/C4).
    // Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3122)
    fn check_auto_accessor_conflicts_with_accessors(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) {
        let mut groups: std::collections::HashMap<(bool, String), Vec<NodeId>> =
            std::collections::HashMap::new();
        for member in object_type_member_nodes(program, node) {
            if !is_class_accessor_member(program, member) {
                continue;
            }
            let Some(name_node) = member_name_node_for_duplicate(program, member) else {
                continue;
            };
            let Some(name) = property_name_text(program, name_node) else {
                continue;
            };
            let is_static = has_static_modifier(program.arena(), member);
            groups.entry((is_static, name)).or_default().push(member);
        }
        for ((is_static, name), members) in groups {
            if members.len() < 2 {
                continue;
            }
            let has_auto_accessor = members
                .iter()
                .any(|&member| is_auto_accessor_member(program, member));
            let has_get = members
                .iter()
                .any(|&member| program.arena().kind(member) == Kind::GetAccessor);
            let has_set = members
                .iter()
                .any(|&member| program.arena().kind(member) == Kind::SetAccessor);
            if has_auto_accessor && (has_get || has_set) {
                self.report_duplicate_member_errors(program, node, &name, is_static);
            }
        }
    }

    // Reports `Duplicate identifier` (TS2300) on every member of `node` whose
    // symbol name is `name` and whose static-ness matches `is_static` (Go's
    // `reportDuplicateMemberErrors` with `checkStatic` always true here). The
    // span is the member NAME node with leading trivia skipped, matching Go's
    // `c.error(member.Name(), ...)` -> `GetErrorRangeForNode`.
    // Go: internal/checker/checker.go:Checker.reportDuplicateMemberErrors(3193)
    fn report_duplicate_member_errors(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        name: &str,
        is_static: bool,
    ) {
        for member in object_type_member_nodes(program, node) {
            if program.arena().kind(member) == Kind::Constructor {
                if let NodeData::ConstructorDeclaration(d) = program.arena().data(member) {
                    for &param in &d.parameters.nodes {
                        if !is_parameter_property_declaration(program, param)
                            || is_parameter_property_binding_pattern(program, param)
                        {
                            continue;
                        }
                        let NodeData::ParameterDeclaration(pd) = program.arena().data(param)
                        else {
                            continue;
                        };
                        if property_name_text(program, pd.name).as_deref() != Some(name) {
                            continue;
                        }
                        let Some(symbol) = program.symbol_of_node(param) else {
                            continue;
                        };
                        let display = super::nodebuilder::symbol_to_string(self, program, symbol);
                        self.error_skipping_leading_trivia(
                            program,
                            pd.name,
                            &tsgo_diagnostics::DUPLICATE_IDENTIFIER_0,
                            &[&display],
                        );
                    }
                }
                continue;
            }
            let Some(symbol) = program.symbol_of_node(member) else {
                continue;
            };
            if program.symbol(symbol).name == name
                && is_static == has_static_modifier(program.arena(), member)
            {
                if let Some(name_node) = member_name_node_for_duplicate(program, member) {
                    let display = super::nodebuilder::symbol_to_string(self, program, symbol);
                    self.error_skipping_leading_trivia(
                        program,
                        name_node,
                        &tsgo_diagnostics::DUPLICATE_IDENTIFIER_0,
                        &[&display],
                    );
                }
            }
        }
    }

    // Reports TS2804 on every class member whose symbol shares the same mangled
    // private-identifier name (Go's `reportDuplicateMemberErrors` with
    // `checkStatic == false`).
    // Go: internal/checker/checker.go:Checker.reportDuplicateMemberErrors(3193)
    fn report_duplicate_private_name_member_errors(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        name: &str,
    ) {
        for member in object_type_member_nodes(program, node) {
            let Some(symbol) = program.symbol_of_node(member) else {
                continue;
            };
            if program.symbol(symbol).name != name {
                continue;
            }
            if let Some(name_node) = member_name_node_for_duplicate(program, member) {
                let display = super::nodebuilder::symbol_to_string(self, program, symbol);
                self.error_skipping_leading_trivia(
                    program,
                    name_node,
                    &tsgo_diagnostics::DUPLICATE_IDENTIFIER_0_STATIC_AND_INSTANCE_ELEMENTS_CANNOT_SHARE_THE_SAME_PRIVATE_NAME,
                    &[&display],
                );
            }
        }
    }

    // Returns the `ExpressionWithTypeArguments` elements of a class-like node's
    // `implements` heritage clause (Go's `ast.GetImplementsHeritageClauseElements`).
    // Go: internal/ast/utilities.go:GetImplementsHeritageClauseElements
    fn implements_heritage_elements(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Vec<NodeId> {
        let heritage = match program.arena().data(node) {
            NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => {
                d.heritage_clauses.clone()
            }
            _ => None,
        };
        let Some(clauses) = heritage else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for clause in clauses.nodes {
            if let NodeData::HeritageClause(h) = program.arena().data(clause) {
                if h.token == Kind::ImplementsKeyword {
                    result.extend(h.types.nodes.iter().copied());
                }
            }
        }
        result
    }

    // Returns the `ExpressionWithTypeArguments` elements of an interface's
    // `extends` heritage clause (Go's `ast.GetExtendsHeritageClauseElements`).
    // Go: internal/ast/utilities.go:GetExtendsHeritageClauseElements
    fn extends_heritage_elements(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Vec<NodeId> {
        let heritage = match program.arena().data(node) {
            NodeData::InterfaceDeclaration(d) => d.heritage_clauses.clone(),
            _ => None,
        };
        let Some(clauses) = heritage else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for clause in clauses.nodes {
            if let NodeData::HeritageClause(h) = program.arena().data(clause) {
                if h.token == Kind::ExtendsKeyword {
                    result.extend(h.types.nodes.iter().copied());
                }
            }
        }
        result
    }

    // Arity + constraint checking for a heritage `ExpressionWithTypeArguments`
    // (Go's `checkTypeReferenceNode` over an extends/implements element).
    // Go: internal/checker/checker.go:Checker.checkTypeReferenceOrImport
    fn check_heritage_type_reference_node(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let (expression, type_arg_nodes) = match program.arena().data(node) {
            NodeData::ExpressionWithTypeArguments(e) => (
                e.expression,
                e.type_arguments
                    .as_ref()
                    .map(|list| list.nodes.clone())
                    .unwrap_or_default(),
            ),
            _ => return,
        };
        let name = match program.arena().kind(expression) {
            Kind::Identifier => program.arena().text(expression).to_string(),
            _ => return,
        };
        let Some(symbol) = resolve_name(
            program,
            node,
            &name,
            SymbolFlags::TYPE,
            false,
            program.globals(),
        ) else {
            return;
        };
        let flags = program.symbol(symbol).flags;
        if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
            self.check_class_or_interface_type_reference(program, node, symbol, &type_arg_nodes);
        } else if flags.contains(SymbolFlags::TYPE_ALIAS) {
            self.check_type_alias_type_reference(program, node, symbol, &type_arg_nodes);
        }
    }

    // Go: internal/checker/checker.go:Checker.checkInheritedPropertiesAreIdentical(5008)
    fn check_inherited_properties_are_identical(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
        type_symbol: SymbolId,
        error_node: NodeId,
    ) -> bool {
        let base_types = self
            .get_type(t)
            .as_object()
            .map(|o| o.base_types.clone())
            .unwrap_or_default();
        if base_types.len() < 2 {
            return true;
        }
        let mut seen: FxHashMap<String, (SymbolId, TypeId)> = FxHashMap::default();
        for (name, prop) in get_properties_of_type(self, t) {
            if program.symbol(prop).parent == Some(type_symbol) {
                seen.insert(name, (prop, t));
            }
        }
        let mut identical = true;
        let interface_str = super::nodebuilder::type_to_string(self, program, t);
        for base in base_types {
            for (name, prop) in get_properties_of_type(self, base) {
                if let Some(&(existing_prop, existing_type)) = seen.get(&name) {
                    if existing_type != t
                        && !self.is_property_identical_to(program, existing_prop, prop)
                    {
                        identical = false;
                        let type_name1 =
                            super::nodebuilder::type_to_string(self, program, existing_type);
                        let type_name2 = super::nodebuilder::type_to_string(self, program, base);
                        let prop_str = super::nodebuilder::symbol_to_string(self, program, prop);
                        let related = self.diagnostic_for_node(
                            program,
                            error_node,
                            &tsgo_diagnostics::NAMED_PROPERTY_0_OF_TYPES_1_AND_2_ARE_NOT_IDENTICAL,
                            &[prop_str.as_str(), type_name1.as_str(), type_name2.as_str()],
                        );
                        let mut chain = self.diagnostic_for_node(
                            program,
                            error_node,
                            &tsgo_diagnostics::INTERFACE_0_CANNOT_SIMULTANEOUSLY_EXTEND_TYPES_1_AND_2,
                            &[interface_str.as_str(), type_name1.as_str(), type_name2.as_str()],
                        );
                        chain.add_related_info(related);
                        self.add_diagnostic(program, chain);
                    }
                } else {
                    seen.insert(name, (prop, base));
                }
            }
        }
        identical
    }

    // Go: internal/checker/checker.go:Checker.isPropertyIdenticalTo(5040)
    fn is_property_identical_to(
        &mut self,
        program: &dyn BoundProgram,
        source_prop: SymbolId,
        target_prop: SymbolId,
    ) -> bool {
        self.compare_properties(program, source_prop, target_prop)
    }

    // Go: internal/checker/checker.go:Checker.compareProperties(27484)
    fn compare_properties(
        &mut self,
        program: &dyn BoundProgram,
        source_prop: SymbolId,
        target_prop: SymbolId,
    ) -> bool {
        if source_prop == target_prop {
            return true;
        }
        let source_accessibility = declaration_modifier_flags_from_symbol(
            self,
            program,
            source_prop,
            false,
        ) & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER;
        let target_accessibility = declaration_modifier_flags_from_symbol(
            self,
            program,
            target_prop,
            false,
        ) & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER;
        if source_accessibility != target_accessibility {
            return false;
        }
        if source_accessibility != ModifierFlags::empty() {
            if get_target_symbol(self, program, source_prop)
                != get_target_symbol(self, program, target_prop)
            {
                return false;
            }
        } else {
            let source_optional = program
                .symbol(source_prop)
                .flags
                .contains(SymbolFlags::OPTIONAL);
            let target_optional = program
                .symbol(target_prop)
                .flags
                .contains(SymbolFlags::OPTIONAL);
            if source_optional != target_optional {
                return false;
            }
        }
        if is_readonly_property_symbol(self, program, source_prop)
            != is_readonly_property_symbol(self, program, target_prop)
        {
            return false;
        }
        let globals = program.globals();
        let source_type = get_type_of_symbol(self, program, source_prop, globals);
        let target_type = get_type_of_symbol(self, program, target_prop, globals);
        self.is_type_identical_to(program, source_type, target_type)
    }

    // Resolves an `ExpressionWithTypeArguments` heritage element to a type id
    // (the reachable subset of Go's `getTypeFromTypeNode` over an implements
    // element): an identifier expression resolves by name in the type meaning to
    // its declared type. Returns `None` when the element is not a bare identifier.
    //
    // DEFER(phase-4-checker-4bm+): qualified-name implements targets and the
    // type-argument-bearing form (`implements I<T>`); the `getReducedType`
    // normalization. blocked-by: qualified-name resolution + generic reference
    // instantiation through `this`.
    // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (getTypeFromTypeNode(typeRefNode))
    fn resolve_heritage_clause_type(
        &mut self,
        program: &dyn BoundProgram,
        type_node: NodeId,
    ) -> Option<TypeId> {
        let expression = match program.arena().data(type_node) {
            NodeData::ExpressionWithTypeArguments(e) => e.expression,
            _ => return None,
        };
        let name = match program.arena().kind(expression) {
            Kind::Identifier => program.arena().text(expression).to_string(),
            Kind::NumberKeyword => return Some(self.number_type()),
            Kind::StringKeyword => return Some(self.string_type()),
            Kind::BooleanKeyword => return Some(self.boolean_type()),
            Kind::BigIntKeyword => return Some(self.bigint_type()),
            _ => return None,
        };
        if let Some(intrinsic) = heritage_intrinsic_type(self, &name) {
            return Some(intrinsic);
        }
        let globals = program.globals();
        let symbol = resolve_name(
            program,
            expression,
            &name,
            SymbolFlags::TYPE,
            false,
            globals,
        )?;
        Some(get_declared_type_of_symbol(self, program, symbol, globals))
    }

    // Checks a single class member (Go's `checkClassMember` dispatch over
    // `checkSourceElement` per member kind): a method/accessor/constructor body
    // is descended into so nested diagnostics surface; a property declaration's
    // initializer is checked for assignability to its annotation.
    //
    // DEFER(phase-4-checker-4r+): the full member checking (signature/override/
    // accessor-pair consistency, parameter-property assignment, decorators,
    // static blocks, computed names, and `this`-typing inside bodies). blocked-by:
    // those member-level checks + function-signature/`this`-type machinery.
    // Go: internal/checker/checker.go:Checker.checkClassMember / checkSourceElement
    fn check_class_member(&mut self, program: &dyn BoundProgram, member: NodeId) {
        match program.arena().data(member) {
            NodeData::MethodDeclaration(_) => {
                self.check_method_declaration(program, member);
            }
            NodeData::GetAccessorDeclaration(_) | NodeData::SetAccessorDeclaration(_) => {
                self.check_accessor_declaration(program, member);
            }
            NodeData::ConstructorDeclaration(_) => {
                self.check_constructor_declaration(program, member);
            }
            NodeData::ClassStaticBlockDeclaration(d) => {
                let body = d.body;
                self.check_statement(program, body);
            }
            NodeData::PropertyDeclaration(_) => {
                self.check_property_declaration_full(program, member);
            }
            _ => {}
        }
    }

    // Checks a class property declaration's initializer against its annotation
    // (the assignability arm of Go's `checkPropertyDeclaration` ->
    // `checkVariableLikeDeclaration`): an annotated property with an initializer
    // requires the initializer's type to be assignable to the annotation, else
    // `2322`. Mirrors `check_variable_declaration` for the property case.
    //
    // DEFER(phase-4-checker-4r+): un-annotated initializer widening/inference,
    // `declare`/ambient property rules, definite-assignment, and accessor-backed
    // properties. blocked-by: initializer widening + ambient/definite-assignment
    // rules.
    // Go: internal/checker/checker.go:Checker.checkPropertyDeclaration / checkVariableLikeDeclaration(5760)
    fn check_property_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let initializer = match program.arena().data(node) {
            NodeData::PropertyDeclaration(d) => d.initializer,
            _ => return,
        };
        let Some(initializer) = initializer else {
            return;
        };
        let Some(symbol) = program.symbol_of_node(node) else {
            // Without a symbol the initializer is still checked so its own
            // nested diagnostics surface.
            self.check_expression(program, initializer);
            return;
        };
        let globals = program.globals();
        let declared = get_type_of_symbol(self, program, symbol, globals);
        let initializer_type = self.check_expression(program, initializer);
        // Go's `checkTypeAssignableToAndOptionallyElaborate(initializerType, t,
        // node, initializer, ...)`: elaborate the initializer literal first.
        if !self.is_type_assignable_to(program, initializer_type, declared)
            && !self.elaborate_error(
                program,
                initializer,
                initializer_type,
                declared,
                RelationKind::Assignable,
            )
        {
            self.report_type_not_assignable(program, node, initializer_type, declared);
        }
    }

    /// Checks whether a `super` expression is used in a legal context and returns
    /// its type (currently `errorType` until super typing is wired).
    ///
    /// # Diagnostics
    ///
    /// - TS2335: `super` referenced in a class with no `extends` clause.
    /// - TS2337: `super()` outside a constructor (or nested in one).
    /// - TS2338: `super` property access outside an allowed member container.
    ///
    /// # Side effects
    ///
    /// May push diagnostics.
    // Go: internal/checker/checker.go:Checker.checkSuperExpression(7822)
    fn check_super_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let parent = program.arena().parent(node);
        let is_call = parent.is_some_and(|p| {
            matches!(program.arena().data(p), NodeData::CallExpression(d) if d.expression == node)
        });
        let mut container = get_super_container(program, node, true);
        if !is_call {
            while let Some(c) = container {
                if program.arena().kind(c) == Kind::ArrowFunction {
                    container = get_super_container(program, c, true);
                } else {
                    break;
                }
            }
        }
        let is_legal = || {
            if is_call {
                return container.is_some_and(|c| program.arena().kind(c) == Kind::Constructor);
            }
            let Some(c) = container else {
                return false;
            };
            let Some(parent) = program.arena().parent(c) else {
                return false;
            };
            if !matches!(
                program.arena().kind(parent),
                Kind::ClassDeclaration | Kind::ClassExpression | Kind::ObjectLiteralExpression
            ) {
                return false;
            }
            if has_static_modifier(program.arena(), c) {
                matches!(
                    program.arena().kind(c),
                    Kind::MethodDeclaration
                        | Kind::MethodSignature
                        | Kind::GetAccessor
                        | Kind::SetAccessor
                        | Kind::PropertyDeclaration
                        | Kind::ClassStaticBlockDeclaration
                )
            } else {
                matches!(
                    program.arena().kind(c),
                    Kind::MethodDeclaration
                        | Kind::MethodSignature
                        | Kind::GetAccessor
                        | Kind::SetAccessor
                        | Kind::PropertyDeclaration
                        | Kind::PropertySignature
                        | Kind::Constructor
                )
            }
        };
        if container.is_none() || !is_legal() {
            if is_call {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::SUPER_CALLS_ARE_NOT_PERMITTED_OUTSIDE_CONSTRUCTORS_OR_IN_NESTED_FUNCTIONS_INSIDE_CONSTRUCTORS,
                    &[],
                );
            } else {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::X_SUPER_PROPERTY_ACCESS_IS_PERMITTED_ONLY_IN_A_CONSTRUCTOR_MEMBER_FUNCTION_OR_MEMBER_ACCESSOR_OF_A_DERIVED_CLASS,
                    &[],
                );
            }
            return self.error_type;
        }
        let Some(class_like) = get_containing_class(program, node) else {
            self.error(
                program,
                node,
                &tsgo_diagnostics::X_SUPER_CAN_ONLY_BE_REFERENCED_IN_A_DERIVED_CLASS,
                &[],
            );
            return self.error_type;
        };
        if get_extends_heritage_clause_element(program, class_like).is_none() {
            self.error(
                program,
                node,
                &tsgo_diagnostics::X_SUPER_CAN_ONLY_BE_REFERENCED_IN_A_DERIVED_CLASS,
                &[],
            );
            return self.error_type;
        }
        self.error_type
    }

    /// Heritage-clause checking for a class-like declaration: walks `extends` /
    /// `implements` elements for grammar-level heritage diagnostics.
    ///
    /// # Side effects
    ///
    /// May push diagnostics via per-element heritage checks.
    // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4287)
    fn check_class_heritage(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let heritage = match program.arena().data(node) {
            NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => {
                d.heritage_clauses.clone()
            }
            _ => None,
        };
        let Some(clauses) = heritage else {
            return;
        };
        for clause in clauses.nodes {
            if let NodeData::HeritageClause(h) = program.arena().data(clause) {
                for &type_node in &h.types.nodes {
                    self.check_heritage_clause_element(program, type_node);
                }
            }
        }
    }

    /// Checks a single `ExpressionWithTypeArguments` heritage element.
    ///
    /// # Diagnostics
    ///
    /// - TS2690: a type-only name used as a heritage value.
    ///
    /// # Side effects
    ///
    /// May push diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarHeritageClause(876)
    fn check_heritage_clause_element(&mut self, program: &dyn BoundProgram, type_node: NodeId) {
        let expression = match program.arena().data(type_node) {
            NodeData::ExpressionWithTypeArguments(d) => d.expression,
            _ => return,
        };
        if program.arena().kind(expression) != Kind::Identifier {
            return;
        }
        let name = program.arena().text(expression).to_string();
        let globals = program.globals();
        let type_sym = resolve_name(
            program,
            expression,
            &name,
            SymbolFlags::TYPE,
            false,
            globals,
        );
        if let Some(sym) = type_sym {
            if resolve_name(
                program,
                expression,
                &name,
                SymbolFlags::VALUE,
                false,
                globals,
            )
            .is_none()
            {
                let sym_flags = program.symbol(sym).flags;
                if !sym_flags.intersects(SymbolFlags::INTERFACE | SymbolFlags::CLASS) {
                    self.error(
                        program,
                        expression,
                        &tsgo_diagnostics::X_0_ONLY_REFERS_TO_A_TYPE_BUT_IS_BEING_USED_AS_A_VALUE_HERE,
                        &[&name],
                    );
                }
            }
        }
    }

    /// Reports when a static property name collides with built-in `Function`
    /// properties (`name`, `length`, `caller`, `arguments`).
    ///
    /// # Diagnostics
    ///
    /// - TS2699: static property conflicts with `Function.{name}`.
    ///
    /// # Side effects
    ///
    /// May push diagnostics.
    // Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4366)
    fn check_class_for_static_property_name_conflicts(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) {
        // Go skips this check when class fields are emitted via `Object.defineProperty`
        // (`useDefineForClassFields`), since static builtins are not assigned on the
        // constructor function in that mode.
        // Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4367)
        if self.compiler_options().get_use_define_for_class_fields() {
            return;
        }
        for member in object_type_member_nodes(program, node) {
            if !has_static_modifier(program.arena(), member) {
                continue;
            }
            let Some(name_node) = member_name_node_for_duplicate(program, member) else {
                continue;
            };
            let Some(name) =
                effective_property_name_for_property_name_node(self, program, name_node)
            else {
                continue;
            };
            if !matches!(name.as_str(), "name" | "length" | "caller" | "arguments") {
                continue;
            }
            let class_name = match program.arena().data(node) {
                NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d
                    .name
                    .map(|n| program.arena().text(n).to_string())
                    .unwrap_or_else(|| "<anonymous>".to_string()),
                _ => "<anonymous>".to_string(),
            };
            self.error(
                program,
                name_node,
                &tsgo_diagnostics::STATIC_PROPERTY_0_CONFLICTS_WITH_BUILT_IN_PROPERTY_FUNCTION_0_OF_CONSTRUCTOR_FUNCTION_1,
                &[name.as_str(), name.as_str(), class_name.as_str()],
            );
        }
    }

    /// Checks get/set accessor pair modifier agreement (abstract flag, accessibility).
    ///
    /// # Diagnostics
    ///
    /// - TS2808: get accessor less accessible than setter.
    ///
    /// # Side effects
    ///
    /// May push diagnostics once per accessor symbol.
    // Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2936)
    fn check_accessor_pair_consistency(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let Some(symbol) = program.symbol_of_node(node) else {
            return;
        };
        let getter = get_declaration_of_kind(program, symbol, Kind::GetAccessor);
        let setter = get_declaration_of_kind(program, symbol, Kind::SetAccessor);
        let (Some(getter), Some(setter)) = (getter, setter) else {
            return;
        };
        if self.accessor_pairs_checked.contains(&getter) {
            return;
        }
        self.accessor_pairs_checked.insert(getter);
        let getter_flags = modifier_flags_of(program.arena(), getter);
        let setter_flags = modifier_flags_of(program.arena(), setter);
        let getter_name = match program.arena().data(getter) {
            NodeData::GetAccessorDeclaration(d) => d.name,
            _ => return,
        };
        let setter_name = match program.arena().data(setter) {
            NodeData::SetAccessorDeclaration(d) => d.name,
            _ => return,
        };
        if getter_flags.contains(tsgo_ast::ModifierFlags::ABSTRACT)
            != setter_flags.contains(tsgo_ast::ModifierFlags::ABSTRACT)
        {
            self.error(
                program,
                getter_name,
                &tsgo_diagnostics::ACCESSORS_MUST_BOTH_BE_ABSTRACT_OR_NON_ABSTRACT,
                &[],
            );
            self.error(
                program,
                setter_name,
                &tsgo_diagnostics::ACCESSORS_MUST_BOTH_BE_ABSTRACT_OR_NON_ABSTRACT,
                &[],
            );
        }
        if (getter_flags.contains(tsgo_ast::ModifierFlags::PROTECTED)
            && !setter_flags
                .intersects(tsgo_ast::ModifierFlags::PROTECTED | tsgo_ast::ModifierFlags::PRIVATE))
            || (getter_flags.contains(tsgo_ast::ModifierFlags::PRIVATE)
                && !setter_flags.contains(tsgo_ast::ModifierFlags::PRIVATE))
        {
            self.error(
                program,
                getter_name,
                &tsgo_diagnostics::A_GET_ACCESSOR_MUST_BE_AT_LEAST_AS_ACCESSIBLE_AS_THE_SETTER,
                &[],
            );
            self.error(
                program,
                setter_name,
                &tsgo_diagnostics::A_GET_ACCESSOR_MUST_BE_AT_LEAST_AS_ACCESSIBLE_AS_THE_SETTER,
                &[],
            );
        }
    }

    /// Walks a class body's method declarations and reports static/instance overload
    /// mismatches between consecutive overloads of the same name.
    // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (overload pass)
    fn check_class_method_overload_static_consistency(
        &mut self,
        program: &dyn BoundProgram,
        class: NodeId,
    ) {
        for member in object_type_member_nodes(program, class) {
            if program.arena().kind(member) == Kind::MethodDeclaration {
                self.check_consecutive_method_overload_static_mismatch(program, member);
            }
        }
    }

    /// Reports when consecutive overload signatures of the same name disagree on
    /// `static` (Go's `reportImplementationExpectedError` static/instance arm).
    ///
    /// # Diagnostics
    ///
    /// - TS2387 / TS2388: static/instance overload mismatch.
    ///
    /// # Side effects
    ///
    /// May push diagnostics.
    // Go: internal/checker/checker.go:Checker.checkFunctionOrConstructorSymbolWorker (reportImplementationExpectedError)
    fn check_consecutive_method_overload_static_mismatch(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) {
        if program.arena().kind(node) != Kind::MethodDeclaration {
            return;
        }
        let Some(class) = get_containing_class(program, node) else {
            return;
        };
        let members = object_type_member_nodes(program, class);
        let Some(idx) = members.iter().position(|&m| m == node) else {
            return;
        };
        if idx + 1 >= members.len() {
            return;
        }
        let next = members[idx + 1];
        if program.arena().kind(next) != Kind::MethodDeclaration {
            return;
        }
        let name_a = match program.arena().data(node) {
            NodeData::MethodDeclaration(d) => d.name,
            _ => return,
        };
        let name_b = match program.arena().data(next) {
            NodeData::MethodDeclaration(d) => d.name,
            _ => return,
        };
        if program.arena().text(name_a) != program.arena().text(name_b) {
            return;
        }
        if has_static_modifier(program.arena(), node) == has_static_modifier(program.arena(), next)
        {
            return;
        }
        let msg = if has_static_modifier(program.arena(), node) {
            &tsgo_diagnostics::FUNCTION_OVERLOAD_MUST_NOT_BE_STATIC
        } else {
            &tsgo_diagnostics::FUNCTION_OVERLOAD_MUST_BE_STATIC
        };
        self.error(program, name_b, msg, &[]);
    }

    /// Method/function overload consistency: static/instance overload agreement and
    /// missing implementation diagnostics.
    ///
    /// # Diagnostics
    ///
    /// - TS2387 / TS2388: static/instance overload mismatch.
    /// - TS2390: constructor implementation missing.
    ///
    /// # Side effects
    ///
    /// May push diagnostics.
    // Go: internal/checker/checker.go:Checker.checkFunctionOrMethodDeclaration(3390)
    fn check_function_or_method_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let Some(symbol) = program.symbol_of_node(node) else {
            return;
        };
        self.check_function_or_constructor_symbol(program, symbol);
    }

    /// Checks a function/constructor symbol once for overload-list consistency.
    ///
    /// # Side effects
    ///
    /// May push diagnostics; marks the symbol as checked.
    // Go: internal/checker/checker.go:Checker.checkFunctionOrConstructorSymbol(3437)
    fn check_function_or_constructor_symbol(
        &mut self,
        program: &dyn BoundProgram,
        symbol: SymbolId,
    ) {
        if self
            .value_symbol_links
            .get(symbol)
            .function_or_constructor_checked
        {
            return;
        }
        self.value_symbol_links
            .get(symbol)
            .function_or_constructor_checked = true;
        let decls = program.symbol(symbol).declarations.clone();
        let is_constructor = program
            .symbol(symbol)
            .flags
            .contains(SymbolFlags::CONSTRUCTOR);
        let mut function_decls: Vec<NodeId> = Vec::new();
        let mut overloads: Vec<NodeId> = Vec::new();
        let mut implementation: Option<NodeId> = None;
        let mut duplicate_function_implementation = false;
        let mut body_declaration_count = 0usize;
        for decl in &decls {
            let kind = program.arena().kind(*decl);
            if !matches!(
                kind,
                Kind::FunctionDeclaration
                    | Kind::MethodDeclaration
                    | Kind::MethodSignature
                    | Kind::Constructor
            ) {
                continue;
            }
            function_decls.push(*decl);
            let has_body = node_has_present_body(program, *decl);
            if has_body {
                body_declaration_count += 1;
                if body_declaration_count > 1 && !is_constructor {
                    duplicate_function_implementation = true;
                }
                if implementation.is_none() {
                    implementation = Some(*decl);
                }
            } else {
                overloads.push(*decl);
            }
        }
        if duplicate_function_implementation {
            for decl in &function_decls {
                let error_node = declaration_name_node(program, *decl).unwrap_or(*decl);
                self.error(
                    program,
                    error_node,
                    &tsgo_diagnostics::DUPLICATE_FUNCTION_IMPLEMENTATION,
                    &[],
                );
            }
        }
        if !overloads.is_empty() {
            let mut some_ambient = false;
            let mut all_ambient = true;
            for decl in &function_decls {
                let ambient = is_effective_ambient_declaration(program, *decl);
                some_ambient |= ambient;
                all_ambient &= ambient;
            }
            if some_ambient && !all_ambient {
                for decl in &function_decls {
                    let error_node = declaration_name_node(program, *decl).unwrap_or(*decl);
                    self.error(
                        program,
                        error_node,
                        &tsgo_diagnostics::OVERLOAD_SIGNATURES_MUST_ALL_BE_AMBIENT_OR_NON_AMBIENT,
                        &[],
                    );
                }
            }
        }
        if overloads.is_empty() {
            return;
        }
        if implementation.is_none() {
            let error_node = overloads.last().copied().unwrap_or(overloads[0]);
            if is_constructor {
                self.error(
                    program,
                    error_node,
                    &tsgo_diagnostics::CONSTRUCTOR_IMPLEMENTATION_IS_MISSING,
                    &[],
                );
            }
            return;
        }
        let impl_node = implementation.expect("implementation");
        for &overload in &overloads {
            if program.arena().kind(overload) != program.arena().kind(impl_node) {
                continue;
            }
            if has_static_modifier(program.arena(), overload)
                != has_static_modifier(program.arena(), impl_node)
            {
                let msg = if has_static_modifier(program.arena(), overload) {
                    &tsgo_diagnostics::FUNCTION_OVERLOAD_MUST_NOT_BE_STATIC
                } else {
                    &tsgo_diagnostics::FUNCTION_OVERLOAD_MUST_BE_STATIC
                };
                let name_node = declaration_name_node(program, overload).unwrap_or(overload);
                self.error(program, name_node, msg, &[]);
            }
        }
    }

    /// Reports whether `t` is a valid `implements` / `extends` object type.
    ///
    /// Reachable subset of Go's `isValidBaseType`: object-like, `any`, or an
    /// intersection of valid bases.
    ///
    /// # Side effects
    ///
    /// None (pure).
    // Go: internal/checker/checker.go:Checker.isValidBaseType(19392)
    fn is_valid_base_type(&self, type_id: TypeId) -> bool {
        let ty = self.get_type(type_id);
        let flags = ty.flags();
        if flags.intersects(TypeFlags::OBJECT | TypeFlags::ANY | TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            return ty
                .intersection_types()
                .map(|types| types.iter().all(|&t| self.is_valid_base_type(t)))
                .unwrap_or(false);
        }
        false
    }

    /// Checks a parameter declaration for constructor parameter-property
    /// diagnostics and `this`/`new` parameter placement.
    ///
    /// # Diagnostics
    ///
    /// - TS2369: parameter property outside a constructor implementation.
    /// - TS2398: parameter property named `constructor`.
    /// - TS2680: `this`/`new` parameter not in first position.
    /// - TS2681: `this` parameter in a constructor.
    /// - TS2730: `this` parameter in an arrow function.
    /// - TS2784: `this` parameter on an accessor.
    ///
    /// # Side effects
    ///
    /// May push diagnostics.
    /// Grammar-checks a rest parameter's position and modifiers (TS1014/1047/1048).
    ///
    /// # Side effects
    ///
    /// May record diagnostics.
    // Go: internal/checker/grammarchecks.go:Checker.checkGrammarParameterList (rest arm)
    fn check_rest_parameter(
        &mut self,
        program: &dyn BoundProgram,
        parameters: &[NodeId],
        index: usize,
        param: NodeId,
    ) {
        let NodeData::ParameterDeclaration(d) = program.arena().data(param) else {
            return;
        };
        let Some(dot_dot_dot) = d.dot_dot_dot_token else {
            return;
        };
        if index != parameters.len().saturating_sub(1) {
            self.grammar_error_on_node(
                program,
                dot_dot_dot,
                &tsgo_diagnostics::A_REST_PARAMETER_MUST_BE_LAST_IN_A_PARAMETER_LIST,
                &[],
            );
        }
        if d.question_token.is_some() {
            self.grammar_error_on_node(
                program,
                d.question_token.unwrap(),
                &tsgo_diagnostics::A_REST_PARAMETER_CANNOT_BE_OPTIONAL,
                &[],
            );
        }
        if d.initializer.is_some() {
            self.grammar_error_on_node(
                program,
                d.name,
                &tsgo_diagnostics::A_REST_PARAMETER_CANNOT_HAVE_AN_INITIALIZER,
                &[],
            );
        }
    }

    /// Checks one parameter declaration: semantic rules plus list-position grammar
    /// (required-after-optional TS1016 and rest-parameter rules).
    ///
    /// # Side effects
    ///
    /// May record diagnostics.
    // Go: internal/checker/checker.go:Checker.checkParameter(2636) +
    // internal/checker/grammarchecks.go:Checker.checkGrammarParameterList
    fn check_parameter_declaration(
        &mut self,
        program: &dyn BoundProgram,
        fn_node: NodeId,
        index: usize,
        param: NodeId,
    ) {
        self.check_parameter(program, param);
        let params = function_like_parameters(program, fn_node);
        self.check_rest_parameter(program, &params, index, param);
        let mut seen_optional = false;
        for (i, &p) in params.iter().enumerate() {
            if i == index {
                let NodeData::ParameterDeclaration(d) = program.arena().data(p) else {
                    break;
                };
                if d.dot_dot_dot_token.is_none()
                    && !is_optional_parameter(program, p)
                    && d.initializer.is_none()
                    && seen_optional
                {
                    self.grammar_error_on_node(
                        program,
                        d.name,
                        &tsgo_diagnostics::A_REQUIRED_PARAMETER_CANNOT_FOLLOW_AN_OPTIONAL_PARAMETER,
                        &[],
                    );
                }
                break;
            }
            if is_optional_parameter(program, p) {
                seen_optional = true;
            }
        }
    }

    // Go: internal/checker/checker.go:Checker.checkParameter(2636)
    fn check_parameter(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let param_data = match program.arena().data(node) {
            NodeData::ParameterDeclaration(d) => d.clone(),
            _ => return,
        };

        let containing_fn = get_containing_function(program, node);

        let param_name = {
            let name_node = param_data.name;
            if program.arena().kind(name_node) == Kind::Identifier {
                program.arena().text(name_node).to_string()
            } else {
                String::new()
            }
        };

        let mods = modifier_flags_of(program.arena(), node);
        if mods.intersects(tsgo_ast::ModifierFlags::PARAMETER_PROPERTY_MODIFIER) {
            if let Some(fn_node) = containing_fn {
                let is_constructor = program.arena().kind(fn_node) == Kind::Constructor;
                let has_body = match program.arena().data(fn_node) {
                    NodeData::ConstructorDeclaration(d) => d.body.is_some(),
                    _ => false,
                };
                if !(is_constructor && has_body) {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::A_PARAMETER_PROPERTY_IS_ONLY_ALLOWED_IN_A_CONSTRUCTOR_IMPLEMENTATION,
                        &[],
                    );
                }
                if is_constructor && param_name == "constructor" {
                    self.error(
                        program,
                        param_data.name,
                        &tsgo_diagnostics::X_CONSTRUCTOR_CANNOT_BE_USED_AS_A_PARAMETER_PROPERTY_NAME,
                        &[],
                    );
                }
            }
        }

        if param_name == "this" || param_name == "new" {
            if let Some(fn_node) = containing_fn {
                let params = function_like_all_parameters(program, fn_node);
                if params.first().copied() != Some(node) {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::A_0_PARAMETER_MUST_BE_THE_FIRST_PARAMETER,
                        &[&param_name],
                    );
                }
                let fn_kind = program.arena().kind(fn_node);
                if fn_kind == Kind::Constructor
                    || fn_kind == Kind::ConstructSignature
                    || fn_kind == Kind::ConstructorType
                {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::A_CONSTRUCTOR_CANNOT_HAVE_A_THIS_PARAMETER,
                        &[],
                    );
                }
                if fn_kind == Kind::ArrowFunction {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::AN_ARROW_FUNCTION_CANNOT_HAVE_A_THIS_PARAMETER,
                        &[],
                    );
                }
                if fn_kind == Kind::GetAccessor || fn_kind == Kind::SetAccessor {
                    self.error(
                        program,
                        node,
                        &tsgo_diagnostics::X_GET_AND_SET_ACCESSORS_CANNOT_DECLARE_THIS_PARAMETERS,
                        &[],
                    );
                }
            }
        }
    }

    // Checks each declaration in a `VariableDeclarationList` (Go's
    // `checkVariableDeclarationList` -> `checkSourceElement` per declaration),
    // shared by variable statements and `for` initializers.
    // Go: internal/checker/checker.go:Checker.checkVariableDeclarationList
    fn check_variable_declaration_list(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let declarations = match program.arena().data(node) {
            NodeData::VariableDeclarationList(l) => l.declarations.nodes.clone(),
            _ => Vec::new(),
        };
        for declaration in declarations {
            self.check_variable_declaration(program, declaration);
        }
    }

    // Checks a variable declaration's initializer against its declared type
    // (the assignability arm of Go's `checkVariableLikeDeclaration`): when the
    // declaration has a type annotation and an initializer, the initializer's
    // type must be assignable to the annotated type, else `2322`.
    //
    // DEFER(phase-4-checker-4m+): binding patterns, parameter initializers,
    // for-in/of initializers, `using`/`await using` disposability, definite
    // assignment, decorators, and initializer-based inference of un-annotated
    // declarations (which would let mismatches against an inferred widened type
    // surface). blocked-by: destructuring + parameter/function bodies +
    // initializer widening/inference + lib globals (P6).
    // Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration(5760)
    fn check_variable_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
        self.check_grammar_variable_declaration(program, node);
        let (name, initializer, type_node) = match program.arena().data(node) {
            NodeData::VariableDeclaration(d) => (d.name, d.initializer, d.type_node),
            _ => return,
        };
        // Check the type annotation's type nodes (Go's `checkSourceElement`
        // descent into `node.Type()`), so a generic type reference in the
        // annotation has its type-argument arity and constraints validated.
        if let Some(type_node) = type_node {
            self.check_type_node(program, type_node);
        }
        // DEFER(phase-4-checker-4m+): binding patterns (destructuring).
        // blocked-by: binding-element checking.
        if program.arena().kind(name) != Kind::Identifier {
            return;
        }
        let Some(initializer) = initializer else {
            return;
        };
        let Some(symbol) = program.symbol_of_node(node) else {
            return;
        };
        // Only validate at the symbol's primary declaration (Go's
        // `node == symbol.ValueDeclaration`), so a redeclaration is not
        // re-checked.
        if program.symbol(symbol).value_declaration != Some(node) {
            return;
        }
        let globals = program.globals();
        // `getTypeOfSymbol` resolves the declared type; for an un-annotated
        // declaration it infers (and type-checks) the initializer, so the
        // initializer's own diagnostics are emitted there. Go then re-checks the
        // initializer via the memoized `checkExpressionCached` (a cache hit, no
        // re-report) against that widened type, which trivially holds. The port
        // has no expression-type cache, so a second `check_expression` here
        // would duplicate the initializer's inner diagnostics; only re-check the
        // initializer when there is an explicit annotation to validate against.
        // Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration(5863)
        let declared = get_type_of_symbol(self, program, symbol, globals);
        if type_node.is_none() {
            return;
        }
        let initializer_type = self.check_expression(program, initializer);
        // Go's `checkTypeAssignableToAndOptionallyElaborate(initializerType, t,
        // node, initializer, ...)`. Go folds excess-property checking into the
        // relation (`hasExcessProperties` inside `recursiveTypeRelatedToWorker`);
        // the port models it as a separate call here, so the ordering mirrors
        // Go's three steps:
        //   (A) relation holds: the only remaining failure Go's relation would
        //       have raised is excess properties, so run that check.
        //   (B) relation failed: `elaborateError` first. A reported element
        //       suppresses BOTH the excess message and the generic chain (Go
        //       never reaches `checkTypeRelatedToEx` once `elaborateError`
        //       reports), e.g. `{ a: "x", b: 1 }` reports only the `a` mismatch.
        //   (C) no element reported: the excess message, then the generic chain.
        if self.is_type_assignable_to(program, initializer_type, declared) {
            self.check_object_literal_excess_properties(
                program,
                initializer,
                initializer_type,
                declared,
            );
            return;
        }
        if self.elaborate_error(
            program,
            initializer,
            initializer_type,
            declared,
            RelationKind::Assignable,
        ) {
            return;
        }
        if self.check_object_literal_excess_properties(
            program,
            initializer,
            initializer_type,
            declared,
        ) {
            return;
        }
        self.report_type_not_assignable(program, node, initializer_type, declared);
    }

    // Types each un-annotated identifier loop variable of a for-of declaration
    // list as the iterated element type (Go's `checkForOfStatement` assigning
    // `checkRightHandSideOfForOf`'s result to the declarations). An annotated or
    // binding-pattern variable is left to its annotation / deferred path.
    // Go: internal/checker/checker.go:Checker.checkForOfStatement
    fn assign_for_of_element_types(
        &mut self,
        program: &dyn BoundProgram,
        list: NodeId,
        element_type: TypeId,
    ) {
        let declarations = match program.arena().data(list) {
            NodeData::VariableDeclarationList(l) => l.declarations.nodes.clone(),
            _ => return,
        };
        for decl in declarations {
            let (name, type_node) = match program.arena().data(decl) {
                NodeData::VariableDeclaration(d) => (d.name, d.type_node),
                _ => continue,
            };
            // DEFER(phase-4-checker-4ad+): binding-pattern (destructuring) loop
            // variables. blocked-by: binding-element typing.
            if type_node.is_some() || program.arena().kind(name) != Kind::Identifier {
                continue;
            }
            if let Some(symbol) = program.symbol_of_node(decl) {
                self.value_symbol_links.get(symbol).resolved_type = Some(element_type);
            }
        }
    }

    // Types each un-annotated identifier loop variable of a for-in declaration
    // list as `string` (Go's `getTypeForVariableLikeDeclaration` returns
    // `c.stringType` for a for-in `VariableDeclaration` in the reachable subset).
    // An annotated or binding-pattern variable is left to its annotation /
    // deferred path.
    // Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration (ForInStatement)
    fn assign_for_in_variable_types(&mut self, program: &dyn BoundProgram, list: NodeId) {
        let string_type = self.string_type;
        let declarations = match program.arena().data(list) {
            NodeData::VariableDeclarationList(l) => l.declarations.nodes.clone(),
            _ => return,
        };
        for decl in declarations {
            let (name, type_node) = match program.arena().data(decl) {
                NodeData::VariableDeclaration(d) => (d.name, d.type_node),
                _ => continue,
            };
            // DEFER(phase-4-checker-4af+): binding-pattern (destructuring) loop
            // variables. blocked-by: binding-element typing.
            if type_node.is_some() || program.arena().kind(name) != Kind::Identifier {
                continue;
            }
            if let Some(symbol) = program.symbol_of_node(decl) {
                self.value_symbol_links.get(symbol).resolved_type = Some(string_type);
            }
        }
    }

    // Resolves the element type produced by iterating a for-of right-hand side,
    // reporting the not-iterable diagnostics (`2488`/`2489`) on `error_node`
    // when the type cannot be iterated (Go's `getIteratedTypeOrElementType` with
    // a non-nil `errorNode`). The array fast path (4ad) tries the `[n: number]`
    // element type first (anything with a number index signature, e.g. the
    // global `Array<T>` reference for `T[]`); a string-like input iterates as
    // `string` (4ai, Go's `getElementTypeOfStringType` reachable subset); the
    // general iterator-protocol path (4ah/4ai) resolves the element type via the
    // `[Symbol.iterator]()` member, reporting `2488`/`2489` on failure.
    //
    // An `any`/error input short-circuits to itself (Go's
    // `checkIteratedTypeOrElementType` returns the input when `IsTypeAny`), so a
    // for-of over an unresolved expression does not additionally report 2488.
    //
    // A union right-hand side distributes (Go's
    // `getIterationTypesOfIterableWorker` union arm + `combineIterationTypes`):
    // each constituent's iterated element type is resolved independently and the
    // results are combined into a union. A constituent that is not iterable
    // fails the whole union with a single `2488` on the union type; the
    // per-constituent resolution is run with `error_node = None` so it does not
    // report its own diagnostic.
    //
    // DEFER(phase-4-checker-4aj+): the `string | string[]` mixed path (a
    // string-like constituent removed from the array type then folded back as a
    // string constituent) and async iterables. blocked-by:
    // `getIteratedTypeOrElementType`'s string-constituent split + async
    // iteration types (lib.d.ts, P6).
    // `pub(crate)` so the contextual-typing pass can reuse it as the port of
    // Go's `getIteratedTypeOrElementType` call inside
    // `getContextualTypeForElementExpression` (an array literal's element gets
    // its contextual type from the iterated element type of the contextual
    // array). It is called there with `error_node = None`, so every reporting
    // branch early-returns and the query stays side-effect-light.
    // Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType
    pub(crate) fn check_iterated_type_or_element_type(
        &mut self,
        program: &dyn BoundProgram,
        input_type: TypeId,
        error_node: Option<NodeId>,
        iterable_exists: bool,
    ) -> Option<TypeId> {
        if self.get_type(input_type).flags().intersects(TypeFlags::ANY) {
            return Some(input_type);
        }
        if self
            .get_type(input_type)
            .flags()
            .intersects(TypeFlags::UNION)
        {
            return self.iterate_union(program, input_type, error_node, iterable_exists);
        }
        let number = self.number_type;
        if let Some(element) =
            super::declared_types::get_indexed_access_type(self, program, input_type, number)
        {
            return Some(element);
        }
        // A string-like right-hand side iterates as `string` (Go's
        // `getIteratedTypeOrElementType` removes string-like constituents from
        // the array type and, when the whole input was a string, yields
        // `c.stringType`). The reachable subset returns `string` for a plain
        // `string`/string-literal input, so no `2488` fires for a string.
        if self
            .get_type(input_type)
            .flags()
            .intersects(TypeFlags::STRING_LIKE)
        {
            return Some(self.string_type);
        }
        if !iterable_exists {
            // No iterator-protocol world (`--target` < `es2015` and no
            // `--downlevelIteration`): Go skips the iterator-protocol resolution
            // and falls to the array-like/string routing, where
            // `getIterationDiagnosticDetails` re-probes the yield type with a nil
            // errorNode. A type that IS iterable via `[Symbol.iterator]`
            // (yield type resolves) reports `2802` ("can only be iterated through
            // when using the '--downlevelIteration' flag or with a '--target' of
            // 'es2015' or higher."); a truly non-iterable type falls to the
            // not-an-array-or-string routing (`2495`, via `report_type_not_iterable`).
            if self
                .get_iterated_type_of_iterable(program, input_type, None, true)
                .is_some()
            {
                self.report_iteration_requires_downlevel(program, input_type, error_node);
            } else {
                self.report_type_not_iterable(program, input_type, error_node, false);
            }
            return None;
        }
        self.get_iterated_type_of_iterable(program, input_type, error_node, true)
    }

    // Reports whether for-of iteration resolves es2015 iterables through the
    // iterator protocol, i.e. whether downlevelling is supported: the `--target`
    // is `es2015` or higher, or `--downlevelIteration` is set. The negative case
    // (a `[Symbol.iterator]`-bearing iterable iterated below this bar) reports
    // `2802`. This replaces the 4ak `getGlobalIterableType` lib-presence proxy
    // with the real compiler-option read now that options are threaded into the
    // checker (4al).
    //
    // DIVERGENCE(port): Go's `iterableExists` is `getGlobalIterableType() !=
    // c.emptyGenericType`, driven by which lib files the effective target loads.
    // Without real lib.d.ts loading the checker reads the raw `--target` /
    // `--downlevelIteration` options directly; the effective-target lib
    // resolution lands with P6 default-lib assembly.
    // Go: internal/checker/checker.go:getIteratedTypeOrElementType (iterableExists)
    fn iterables_resolvable_via_protocol(&self) -> bool {
        let options = self.compiler_options();
        options.downlevel_iteration.is_true()
            || (options.target as i32) >= (ScriptTarget::Es2015 as i32)
    }

    // Resolves the iterated element type of a union right-hand side (Go's
    // `getIterationTypesOfIterableWorker` union arm + `combineIterationTypes`,
    // plus the `getIteratedTypeOrElementType` string-constituent split). For-of
    // permits string input, so string-like constituents iterate as `string`;
    // each remaining constituent's element type is resolved independently
    // (`error_node = None` so it does not report on its own) and the results are
    // combined into a union.
    //
    // When some non-string constituent is not iterable, the failure routing
    // depends on whether a global `Iterable` exists:
    //   - with `Iterable` (iterator-protocol world): report `2488` on the whole
    //     union and yield no element type (Go's union arm reports
    //     `reportTypeNotIterableError` on `t`);
    //   - without `Iterable` and with a string constituent: report `2461`
    //     "is not an array type" on the non-string remainder and still yield
    //     `string` (Go's string-constituent split: a string was present, so the
    //     element type is `string`, but the non-string remainder is not an
    //     array);
    //   - without `Iterable` and without a string constituent: report `2495`
    //     "is not an array type or a string type" on the whole union.
    // Go: internal/checker/checker.go:Checker.getIterationTypesOfIterableWorker (union)
    fn iterate_union(
        &mut self,
        program: &dyn BoundProgram,
        input_type: TypeId,
        error_node: Option<NodeId>,
        iterable_exists: bool,
    ) -> Option<TypeId> {
        let constituents = self
            .get_type(input_type)
            .union_types()
            .map(<[TypeId]>::to_vec)
            .unwrap_or_default();
        let mut non_string = Vec::with_capacity(constituents.len());
        let mut has_string_constituent = false;
        for &constituent in &constituents {
            if self
                .get_type(constituent)
                .flags()
                .intersects(TypeFlags::STRING_LIKE)
            {
                has_string_constituent = true;
            } else {
                non_string.push(constituent);
            }
        }
        let mut element_types: Vec<TypeId> = Vec::with_capacity(constituents.len());
        if has_string_constituent {
            element_types.push(self.string_type);
        }
        let mut any_failed = false;
        for &constituent in &non_string {
            match self.check_iterated_type_or_element_type(
                program,
                constituent,
                None,
                iterable_exists,
            ) {
                Some(t) => element_types.push(t),
                None => any_failed = true,
            }
        }
        if !any_failed {
            return Some(self.get_union_type(&element_types));
        }
        if iterable_exists {
            self.report_type_not_iterable(program, input_type, error_node, iterable_exists);
            return None;
        }
        if has_string_constituent {
            // A string was present, so the element type is `string`; the
            // non-string remainder is not an array, hence `2461` on it.
            let remainder = self.get_union_type(&non_string);
            self.report_not_array_type(program, remainder, error_node);
            return Some(self.string_type);
        }
        self.report_type_not_iterable(program, input_type, error_node, iterable_exists);
        None
    }

    // Resolves the element type of a `[Symbol.iterator]`-bearing iterable via the
    // iterator protocol (Go's `getIteratedTypeOfIterable` ->
    // `getIterationTypesOfIterable` -> `getIterationTypesOfIterator`, reachable
    // subset): the `__@iterator` member's call-signature return type is the
    // iterator; that iterator's `next()` call-signature return type is the
    // iteration result; the result's `value` property is the element type.
    //
    // When the type has no `[Symbol.iterator]()` method (or its method yields no
    // iterator type), `2488` is reported (Go's `reportTypeNotIterableError`).
    //
    // DIVERGENCE(port): rather than instantiating the iterator's anonymous
    // `next` method type (anonymous-object deep instantiation is deferred), the
    // element type is read as the `value` property type of the (uninstantiated)
    // `next()` result and then instantiated through the iterator reference's own
    // `type parameters -> type arguments` mapper, so
    // `Iterator<string>.next(): { value: T }` yields `string`. The element type
    // is identical to Go's for the reachable subset.
    //
    // DEFER(phase-4-checker-4ai+): `getIterationTypesOfIterable`'s full
    // union/async-iterable (`__@asyncIterator`) handling, the iteration-type
    // cache, and the `2489` "iterator must have a `next()`" diagnostic.
    // blocked-by: `IterationTypes` + async iteration + diagnostic plumbing.
    // Go: internal/checker/checker.go:Checker.getIteratedTypeOfIterable
    fn get_iterated_type_of_iterable(
        &mut self,
        program: &dyn BoundProgram,
        input_type: TypeId,
        error_node: Option<NodeId>,
        iterable_exists: bool,
    ) -> Option<TypeId> {
        let iterator_name = self.get_property_name_for_known_symbol_name("iterator");
        let iterator_method = match get_property_of_type(self, input_type, &iterator_name) {
            Some(method) => method,
            None => {
                self.report_type_not_iterable(program, input_type, error_node, iterable_exists);
                return None;
            }
        };
        let globals = program.globals();
        let method_type = get_type_of_symbol(self, program, iterator_method, globals);
        let iterator_type = match self.first_signature_return_type(program, method_type) {
            Some(t) => t,
            None => {
                self.report_type_not_iterable(program, input_type, error_node, iterable_exists);
                return None;
            }
        };
        // The iterator reference's type-argument mapper (`Iterator<string>` ->
        // `{ T: string }`), used to instantiate the element type below.
        let mapper = self.type_reference_mapper(iterator_type);
        // Both sync and async iterators must have a `next()` method whose call
        // signature yields the iteration result; a missing `next()` (no member,
        // or a member with no call signatures) reports `2489` (Go's
        // `getIterationTypesOfMethod` for `"next"`).
        let next_sym = get_property_of_type(self, iterator_type, "next");
        let next_type = match next_sym {
            Some(next_sym) => {
                let globals = program.globals();
                get_type_of_symbol(self, program, next_sym, globals)
            }
            None => {
                self.report_iterator_missing_next(program, input_type, error_node, iterable_exists);
                return None;
            }
        };
        let result_type = match self.first_signature_return_type(program, next_type) {
            Some(t) => t,
            None => {
                self.report_iterator_missing_next(program, input_type, error_node, iterable_exists);
                return None;
            }
        };
        let value_sym = get_property_of_type(self, result_type, "value")?;
        let globals = program.globals();
        let value_type = get_type_of_symbol(self, program, value_sym, globals);
        match mapper {
            Some(m) => Some(self.instantiate_type(value_type, &m)),
            None => Some(value_type),
        }
    }

    // Reports `2488` ("Type '...' must have a '[Symbol.iterator]()' method that
    // returns an iterator.") on `error_node`, printing the offending type via
    // `type_to_string` (Go's `reportTypeNotIterableError`, sync subset).
    //
    // DEFER(phase-4-checker-4ai+): the async-iterable message variant (`2504`)
    // and the "did you forget `await`?" suggestion. blocked-by:
    // `allowAsyncIterables` plumbing + `getAwaitedTypeOfPromise`.
    // Go: internal/checker/checker.go:Checker.reportTypeNotIterableError
    fn report_type_not_iterable(
        &mut self,
        program: &dyn BoundProgram,
        input_type: TypeId,
        error_node: Option<NodeId>,
        iterable_exists: bool,
    ) {
        let Some(error_node) = error_node else {
            return;
        };
        // Without a global `Iterable`, the for-of falls back to the array-like /
        // string routing: a non-array/non-string input (no string constituent)
        // is "not an array type or a string type" (`2495`), since for-of allows
        // string input (Go's `getIterationDiagnosticDetails`, `allowsStrings`).
        if !iterable_exists {
            let type_str = super::nodebuilder::type_to_string(self, program, input_type);
            self.error(
                program,
                error_node,
                &tsgo_diagnostics::TYPE_0_IS_NOT_AN_ARRAY_TYPE_OR_A_STRING_TYPE,
                &[type_str.as_str()],
            );
            return;
        }
        let type_str = super::nodebuilder::type_to_string(self, program, input_type);
        self.error(
            program,
            error_node,
            &tsgo_diagnostics::TYPE_0_MUST_HAVE_A_SYMBOL_ITERATOR_METHOD_THAT_RETURNS_AN_ITERATOR,
            &[type_str.as_str()],
        );
    }

    // Reports `2461` ("Type '...' is not an array type.") on `error_node`,
    // printing `input_type` via `type_to_string`. This is Go's
    // `getIterationDiagnosticDetails` `allowsStrings == false` branch, reached
    // when a string constituent was already split off (so strings are known to
    // be fine) but the non-string remainder is not an array.
    // Go: internal/checker/checker.go:Checker.getIterationDiagnosticDetails (not allowsStrings)
    fn report_not_array_type(
        &mut self,
        program: &dyn BoundProgram,
        input_type: TypeId,
        error_node: Option<NodeId>,
    ) {
        let Some(error_node) = error_node else {
            return;
        };
        let type_str = super::nodebuilder::type_to_string(self, program, input_type);
        self.error(
            program,
            error_node,
            &tsgo_diagnostics::TYPE_0_IS_NOT_AN_ARRAY_TYPE,
            &[type_str.as_str()],
        );
    }

    // Reports `2802` ("Type '...' can only be iterated through when using the
    // '--downlevelIteration' flag or with a '--target' of 'es2015' or higher.")
    // on `error_node`, printing `input_type` via `type_to_string`. This is Go's
    // `getIterationDiagnosticDetails` `yieldType != nil` branch: the type IS
    // iterable via `[Symbol.iterator]`, but the effective `--target` is below
    // `es2015` and `--downlevelIteration` is not set.
    //
    // DEFER(phase-4-checker-4am+): the `isES2015OrLaterIterable` symbol-name
    // table (`Float32Array`/`NodeList`/...) that also yields `2802` for a
    // not-yet-iterable named type, and `2802` for a union member. blocked-by:
    // those lib global types (P6) + union `getIterationDiagnosticDetails`.
    // Go: internal/checker/checker.go:Checker.getIterationDiagnosticDetails (yieldType != nil)
    fn report_iteration_requires_downlevel(
        &mut self,
        program: &dyn BoundProgram,
        input_type: TypeId,
        error_node: Option<NodeId>,
    ) {
        let Some(error_node) = error_node else {
            return;
        };
        let type_str = super::nodebuilder::type_to_string(self, program, input_type);
        self.error(
            program,
            error_node,
            &tsgo_diagnostics::TYPE_0_CAN_ONLY_BE_ITERATED_THROUGH_WHEN_USING_THE_DOWNLEVELITERATION_FLAG_OR_WITH_A_TARGET_OF_ES2015_OR_HIGHER,
            &[type_str.as_str()],
        );
    }

    // Reports a for-of iterator whose returned iterator type lacks a `next()`
    // method: the primary diagnostic is `2488` ("Type '...' must have a
    // '[Symbol.iterator]()' method that returns an iterator.") on `error_node`,
    // carrying `2489` ("An iterator must have a 'next()' method.") as *related
    // information* (Go's `getIterationTypesOfIterableWorker`:
    // `getIterationTypesOfMethod` for `"next"` pushes `2489` into
    // `diagnosticOutput`, then the worker creates the `2488` via
    // `reportTypeNotIterableError` and `AddRelatedInfo`s the `2489` onto it).
    //
    // This restores the Go-faithful nesting and fixes the 4ai divergence (which,
    // lacking related-info plumbing, surfaced `2489` as a top-level diagnostic).
    //
    // DEFER(phase-4-checker-4aj+): the `return`/`throw` method checks
    // (`mustBeAMethodDiagnostic`) and the async iterator (`2504`) variant.
    // blocked-by: full `IterationTypes` + async iteration (lib.d.ts, P6).
    // Go: internal/checker/checker.go:Checker.getIterationTypesOfMethod / getIterationTypesOfIterableWorker
    fn report_iterator_missing_next(
        &mut self,
        program: &dyn BoundProgram,
        input_type: TypeId,
        error_node: Option<NodeId>,
        iterable_exists: bool,
    ) {
        // Without a global `Iterable`, the iterator protocol is never consulted
        // and the type falls through to the array-like/string routing, so a
        // missing `next()` is not the relevant failure; report the same
        // not-an-array-or-string diagnostic the fallback would (Go reaches
        // `getIterationDiagnosticDetails` here, not the `2489` path).
        if !iterable_exists {
            self.report_type_not_iterable(program, input_type, error_node, iterable_exists);
            return;
        }
        let Some(error_node) = error_node else {
            return;
        };
        let related = self.diagnostic_for_node(
            program,
            error_node,
            &tsgo_diagnostics::AN_ITERATOR_MUST_HAVE_A_NEXT_METHOD,
            &[],
        );
        let type_str = super::nodebuilder::type_to_string(self, program, input_type);
        let mut primary = self.diagnostic_for_node(
            program,
            error_node,
            &tsgo_diagnostics::TYPE_0_MUST_HAVE_A_SYMBOL_ITERATOR_METHOD_THAT_RETURNS_AN_ITERATOR,
            &[type_str.as_str()],
        );
        primary.add_related_info(related);
        self.add_diagnostic(program, primary);
    }

    // Returns the return type of the first call signature of `t`, if any (the
    // reachable single-signature subset of Go's `getSignaturesOfType` +
    // `getReturnTypeOfSignature`).
    fn first_signature_return_type(
        &mut self,
        program: &dyn BoundProgram,
        t: TypeId,
    ) -> Option<TypeId> {
        let signature = *self.get_signatures_of_type(t).first()?;
        Some(self.get_return_type_of_call(program, signature, &[], &[]))
    }

    // Builds the `type parameters -> type arguments` mapper of a generic type
    // reference (`Foo<string>` -> `{ T: string }`), or `None` when `t` is not a
    // reference whose target's type-parameter arity matches its arguments. Used
    // to instantiate a member type read through the reference (Go folds this
    // into `getTypeOfPropertyOfType`; 4ah threads it for the iterator value).
    fn type_reference_mapper(&self, t: TypeId) -> Option<TypeMapper> {
        let obj = self.get_type(t).as_object()?;
        let target = obj.target?;
        let args = obj.resolved_type_arguments.clone();
        let params = self
            .get_type(target)
            .as_object()
            .map(|o| o.type_parameters.clone())
            .unwrap_or_default();
        if params.is_empty() || params.len() != args.len() {
            return None;
        }
        Some(TypeMapper::Array {
            sources: params,
            targets: args,
        })
    }

    // Generalizes a literal `source` to its base type for an assignability error
    // message, mirroring Go's `reportRelationError`: a literal source is widened
    // (e.g. `"s"` -> `string`) when the `target` cannot hold top-level singleton
    // types, so the message reads `Type 'string' ...` rather than `Type '"s"' ...`.
    // Go: internal/checker/relater.go:errorReporter.reportRelationError
    pub(crate) fn generalized_source_for_error(&self, source: TypeId, target: TypeId) -> TypeId {
        if !self.get_type(target).flags().contains(TypeFlags::NEVER)
            && self.is_literal_type(source)
            && !self.type_could_have_top_level_singleton_types(target)
        {
            return self.get_base_type_of_literal_type(source);
        }
        source
    }

    // Reports whether `t` is a literal type (Go's `isLiteralType`, 4m subset:
    // `boolean` and unit types).
    //
    // DEFER(phase-4-checker-4m+): unions whose members are all unit types.
    // blocked-by: union literal-type construction.
    // Go: internal/checker/checker.go:isLiteralType(25252)
    pub(crate) fn is_literal_type(&self, t: TypeId) -> bool {
        let f = self.get_type(t).flags();
        f.intersects(TypeFlags::BOOLEAN) || f.intersects(TypeFlags::UNIT)
    }

    // Reports whether `t` could contain top-level singleton (unit) types in a
    // way meaningful to error reporting (Go's `typeCouldHaveTopLevelSingletonTypes`).
    // `boolean` (a `true | false` union) is excluded by design; a union /
    // intersection qualifies when any constituent does, so a literal-union target
    // (e.g. `"a" | "b"`, the result of `keyof`) keeps a non-member source literal
    // un-generalized in the assignability error (`"c"`, not `string`).
    //
    // DEFER(phase-4-checker-C-C2): the instantiable-constraint arm
    // (`getConstraintOfType`) and `isPatternLiteralType`. blocked-by: constraint
    // resolution over instantiable types + pattern (template) literal types.
    // Go: internal/checker/relater.go:Checker.typeCouldHaveTopLevelSingletonTypes(1302)
    fn type_could_have_top_level_singleton_types(&self, t: TypeId) -> bool {
        let f = self.get_type(t).flags();
        // `boolean` is `true | false` but is not a useful singleton for errors.
        // (This port represents `boolean` as a plain union with no `BOOLEAN`
        // flag bit, so compare against the interned boolean type directly.)
        if f.intersects(TypeFlags::BOOLEAN) || t == self.boolean_type {
            return false;
        }
        if f.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let members = if let Some(m) = self.get_type(t).union_types() {
                m.to_vec()
            } else {
                self.get_type(t)
                    .intersection_types()
                    .unwrap_or(&[])
                    .to_vec()
            };
            return members
                .iter()
                .any(|&m| self.type_could_have_top_level_singleton_types(m));
        }
        f.intersects(TypeFlags::UNIT | TypeFlags::TEMPLATE_LITERAL | TypeFlags::STRING_MAPPING)
    }

    // Returns the union of `members` with the `never` type dropped (Go's
    // `getUnionType`, which discards `never` constituents), so a logical
    // operator's union result does not carry a spurious `never`.
    //
    // DEFER(phase-4-checker-4p+): flattening union members + subtype/literal
    // reduction. blocked-by: 4b union reduction (`getUnionType` reduction modes).
    // Go: internal/checker/checker.go:Checker.getUnionType (never removal)
    fn get_union_dropping_never(&mut self, members: &[TypeId]) -> TypeId {
        let kept: Vec<TypeId> = members
            .iter()
            .copied()
            .filter(|&m| m != self.never_type)
            .collect();
        self.get_union_type(&kept)
    }

    // Removes the definitely-falsy constituents of `t`, keeping the truthy part
    // (Go's `removeDefinitelyFalsyTypes` = `filterType(t, hasTypeFacts(Truthy))`).
    // Go: internal/checker/checker.go:Checker.removeDefinitelyFalsyTypes(28782)
    fn remove_definitely_falsy_types(&mut self, t: TypeId) -> TypeId {
        self.get_type_with_facts(t, TypeFacts::TRUTHY)
    }

    // Maps each constituent of `t` to its definitely-falsy part and unions them
    // (Go's `extractDefinitelyFalsyTypes` = `mapType(t, getDefinitelyFalsyPartOfType)`).
    // Go: internal/checker/checker.go:Checker.extractDefinitelyFalsyTypes(28786)
    fn extract_definitely_falsy_types(&mut self, t: TypeId) -> TypeId {
        let members = self.distributed_types(t);
        let mut mapped = Vec::with_capacity(members.len());
        for member in members {
            mapped.push(self.get_definitely_falsy_part_of_type(member));
        }
        self.get_union_type(&mapped)
    }

    // Returns the definitely-falsy part of a single (non-union) type (Go's
    // `getDefinitelyFalsyPartOfType`, 4p subset): already-falsy types (`false`,
    // `void`/`undefined`/`null`, `any`/`unknown`, the empty-string / zero-number
    // literals) are their own falsy part; everything else has no falsy part
    // (`never`).
    //
    // DEFER(phase-4-checker-4p+): the falsy literal for the `string`/`number`/
    // `bigint` primitives (Go maps them to the `emptyString`/`zero`/`zeroBigInt`
    // literal intrinsics). Returning `never` here coincides with Go's *reduced*
    // union result whenever the other operand already carries that primitive.
    // blocked-by: the falsy literal intrinsics + 4b union literal reduction.
    // Go: internal/checker/checker.go:Checker.getDefinitelyFalsyPartOfType(28790)
    fn get_definitely_falsy_part_of_type(&self, t: TypeId) -> TypeId {
        let ty = self.get_type(t);
        let f = ty.flags();
        if t == self.regular_false_type
            || t == self.false_type
            || f.intersects(TypeFlags::VOID | TypeFlags::NULLABLE | TypeFlags::ANY_OR_UNKNOWN)
        {
            return t;
        }
        match ty.literal_value() {
            Some(LiteralValue::String(s)) if s.is_empty() => return t,
            Some(LiteralValue::Number(n)) if f64::from(*n) == 0.0 => return t,
            _ => {}
        }
        self.never_type
    }

    // Returns the base type of a literal type (Go's `getBaseTypeOfLiteralType`,
    // 4m subset: the primitive backing string/number/bigint/boolean literals).
    //
    // DEFER(phase-4-checker-4m+): enum-like base types and union mapping.
    // blocked-by: enum base-type resolution + union mapping.
    // Go: internal/checker/checker.go:Checker.getBaseTypeOfLiteralType(25293)
    pub(crate) fn get_base_type_of_literal_type(&self, t: TypeId) -> TypeId {
        let f = self.get_type(t).flags();
        if f.intersects(TypeFlags::STRING_LITERAL) {
            return self.string_type;
        }
        if f.intersects(TypeFlags::NUMBER_LITERAL) {
            return self.number_type;
        }
        if f.intersects(TypeFlags::BIG_INT_LITERAL) {
            return self.bigint_type;
        }
        if f.intersects(TypeFlags::BOOLEAN_LITERAL | TypeFlags::BOOLEAN) {
            return self.boolean_type;
        }
        if f.contains(TypeFlags::UNION) {
            if let Some(members) = self.get_type(t).union_types() {
                if members
                    .iter()
                    .all(|&m| self.get_type(m).flags().intersects(TypeFlags::BOOLEAN_LIKE))
                {
                    return self.boolean_type;
                }
                if members
                    .iter()
                    .all(|&m| self.get_type(m).flags().intersects(TypeFlags::STRING_LIKE))
                {
                    return self.string_type;
                }
                if members
                    .iter()
                    .all(|&m| self.get_type(m).flags().intersects(TypeFlags::NUMBER_LIKE))
                {
                    return self.number_type;
                }
            }
        }
        t
    }

    /// Type-checks `file` if needed, then returns its diagnostics (Go's
    /// `getDiagnostics`, which itself runs `checkSourceFile`).
    ///
    /// This is the surface a checker pool drives: after
    /// [`Checker::new_checker(program)`](Checker::new_checker) it calls
    /// `get_diagnostics(file)` per assigned file, with no per-call program. The
    /// underlying [`Checker::check_source_file`] is idempotent, so the file is
    /// checked at most once.
    ///
    /// `file` is a source-file handle from
    /// [`BoundProgram::source_files`](crate::BoundProgram::source_files). For a
    /// multi-file program the result is filtered to `file` (Go's
    /// `collection.GetDiagnosticsForFile(name)`): diagnostics produced while
    /// checking other files are not returned. A single-file program has exactly
    /// one such handle (its [`root`](crate::BoundProgram::root)).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// # fn demo(c: &mut Checker, file: tsgo_ast::NodeId) {
    /// let _ = c.get_diagnostics(file);
    /// # }
    /// ```
    ///
    /// Side effects: type-checks `file` on first request (records diagnostics,
    /// allocates types).
    // Go: internal/checker/checker.go:Checker.getDiagnostics(13865)
    pub fn get_diagnostics(&mut self, file: NodeId) -> &[Diagnostic] {
        self.check_source_file(file);
        self.diagnostics_by_file
            .get(&file)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns suggestion diagnostics for `file`, type-checking it first if needed
    /// (Go's `getSuggestionDiagnostics`).
    // Go: internal/checker/checker.go:Checker.getSuggestionDiagnostics(13861)
    pub fn get_suggestion_diagnostics(&mut self, file: NodeId) -> &[Diagnostic] {
        self.check_source_file(file);
        self.suggestion_diagnostics_by_file
            .get(&file)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns diagnostics already recorded for `file` without type-checking.
    #[cfg(test)]
    pub(crate) fn peek_recorded_diagnostics(&self, file: NodeId) -> &[Diagnostic] {
        self.diagnostics_by_file
            .get(&file)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    // Records a diagnostic at `node` from `message` with `args` substituted,
    // partitioned by the file `program` is a view of (Go records into a per-file
    // `DiagnosticsCollection`). The partition key is `program.file_handle()`, so
    // `get_diagnostics(file)` returns only that file's diagnostics regardless of
    // whether checking was driven via `check_source_file` or a direct check.
    // Go: internal/checker/checker.go:Checker.error(13893)
    pub(crate) fn error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static Message,
        args: &[&str],
    ) {
        let diagnostic = self.diagnostic_for_node(program, node, message, args);
        self.add_diagnostic(program, diagnostic);
    }

    // Reports an error and, when `maybe_missing_await` is set, attaches the
    // "Did you forget to use 'await'?" related diagnostic (Go's
    // `errorAndMaybeSuggestAwait`).
    // Go: internal/checker/checker.go:Checker.errorAndMaybeSuggestAwait(13909)
    fn error_and_maybe_suggest_await(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        maybe_missing_await: bool,
        message: &'static Message,
        args: &[&str],
    ) {
        let mut diagnostic = self.diagnostic_for_node(program, node, message, args);
        if maybe_missing_await {
            let related = self.diagnostic_for_node(
                program,
                node,
                &tsgo_diagnostics::DID_YOU_FORGET_TO_USE_AWAIT,
                &[],
            );
            diagnostic.add_related_info(related);
        }
        self.add_diagnostic(program, diagnostic);
    }

    // Records a diagnostic on `node` whose span starts at the node's first
    // non-trivia character — Go's `c.error(node)` span via
    // `scanner.GetErrorRangeForNode` (default case: `SkipTrivia(text,
    // node.Pos())..node.End()`). A node's `pos` is its FULL-start (leading
    // trivia included), so a span byte-compared against `tsc`'s baseline must
    // skip that trivia. Falls back to the raw `pos` when the file text is not
    // available (correct when the node has no leading trivia).
    //
    // Used by the JSX intrinsic-tag path (TS7026 / TS2339), whose element node
    // is preceded by whitespace (e.g. `const a = <div/>`); the generic
    // `self.error` keeps the raw range to match the existing relation-error
    // emission sites that already byte-match `tsc`.
    // Go: internal/scanner/scanner.go:GetErrorRangeForNode (default case)
    pub(crate) fn error_skipping_leading_trivia(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static Message,
        args: &[&str],
    ) {
        let loc = program.arena().loc(node);
        let start = match program.source_text() {
            Some(text) => tsgo_scanner::skip_trivia(text, loc.pos()),
            None => loc.pos(),
        };
        let diagnostic = Diagnostic {
            code: message.code(),
            category: message.category(),
            message: tsgo_diagnostics::format(&message.to_string(), args),
            start,
            length: loc.end() - start,
            related_information: Vec::new(),
            message_chain: Vec::new(),
        };
        self.add_diagnostic(program, diagnostic);
    }

    // Computes the reported source range `[start, end)` for an error on `node`,
    // faithfully porting Go's `scanner.GetErrorRangeForNode` for the cases the
    // relation-error path reaches. Two rules combine:
    //   * a DECLARATION whose error range narrows to its NAME — Go's switch maps
    //     `KindVariableDeclaration` (and the rest of the declaration group, plus
    //     `KindClassExpression`) to `errorNode = GetNameOfDeclaration(node)`, so
    //     `const x: number = "";` underlines just `x`, not `x: number = ""`;
    //   * the generic tail — `pos = errorNode.Pos()`, then `skipTrivia(pos)`
    //     unless the node is missing or JSX text, and `end = errorNode.End()`.
    // An expression error node (e.g. an assignment LHS identifier) is not in the
    // narrowing group, so it keeps `skipTrivia(pos)..end` over the node itself.
    //
    // DEFER(phase-4-checker): the special non-narrowing arms of Go's switch —
    // `KindSourceFile`, `KindArrowFunction`, `KindCaseClause`/`KindDefaultClause`,
    // `KindReturnStatement`/`KindYieldExpression`, `KindSatisfiesExpression`,
    // `KindConstructor`, and the `KindFunctionDeclaration`/`KindMethodDeclaration`
    // reparsed nuance — need `GetRangeOfTokenAtPosition` / a re-scan / the line
    // map; none are reached by the relation-error emitters routed through here,
    // so they fall through to the generic `skipTrivia(pos)..end` tail.
    // blocked-by: `GetRangeOfTokenAtPosition` + reparsed-JSDoc declarations.
    // Go: internal/scanner/scanner.go:GetErrorRangeForNode(2517)
    fn get_error_range_for_node(&self, program: &dyn BoundProgram, node: NodeId) -> (i32, i32) {
        let arena = program.arena();
        // The declaration kinds whose error range narrows to the declaration
        // NAME (Go's `GetErrorRangeForNode` declaration group + the
        // `KindClassExpression` arm). `name_of_declaration` returns the name for
        // the kinds it covers; an absent name falls back to the node itself
        // (Go's `errorNode == nil` tail, approximated without the token re-scan).
        let error_node = match arena.kind(node) {
            Kind::VariableDeclaration
            | Kind::BindingElement
            | Kind::ClassDeclaration
            | Kind::InterfaceDeclaration
            | Kind::ModuleDeclaration
            | Kind::EnumDeclaration
            | Kind::EnumMember
            | Kind::FunctionExpression
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::TypeAliasDeclaration
            | Kind::PropertyDeclaration
            | Kind::PropertySignature
            | Kind::NamespaceImport
            | Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::ClassExpression => {
                super::symbols_query::name_of_declaration(arena, node).unwrap_or(node)
            }
            _ => node,
        };
        let loc = arena.loc(error_node);
        let pos = loc.pos();
        // Generic tail: skip leading trivia unless the node is missing or JSX
        // text (a node's `pos` is its full-start, leading trivia included).
        let start = if !tsgo_ast::utilities::node_is_missing(arena, error_node)
            && arena.kind(error_node) != Kind::JsxText
        {
            match program.source_text() {
                Some(text) => tsgo_scanner::skip_trivia(text, pos),
                None => pos,
            }
        } else {
            pos
        };
        (start, loc.end())
    }

    // Builds (without recording) a diagnostic at `node` from `message` with
    // `args` substituted; its related-information list starts empty (Go's
    // `createDiagnosticForNode`). Callers attach related entries via
    // `Diagnostic::add_related_info` before recording with `add_diagnostic`.
    // Go: internal/checker/checker.go:createDiagnosticForNode(14148)
    pub(super) fn diagnostic_for_node(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static Message,
        args: &[&str],
    ) -> Diagnostic {
        let loc = program.arena().loc(node);
        Diagnostic {
            code: message.code(),
            category: message.category(),
            message: tsgo_diagnostics::format(&message.to_string(), args),
            start: loc.pos(),
            length: loc.end() - loc.pos(),
            related_information: Vec::new(),
            message_chain: Vec::new(),
        }
    }

    // Reports that `source` is not assignable to `target` at `node`, building
    // the nested elaboration chain via the relation engine's reporting path
    // (4bn). The head is normally `2322` "Type 'X' is not assignable to type
    // 'Y'." carrying a chain (`2326` "Types of property 'x' are incompatible." /
    // the dotted `2200` "The types of 'x.y' are incompatible between these
    // types." over a leaf `2322`); a single missing required property collapses
    // to a `2741` head (Go suppresses the `2322` head in that case).
    //
    // This replaces the old flat `type_to_string`-only `2322` emission at the
    // var-decl / assignment / property-decl sites. The head text is identical to
    // the old flat path for a non-structural mismatch (e.g. `number` vs
    // `string`), so those cases keep a single chain-less `2322`.
    // Go: internal/checker/checker.go:Checker.checkTypeAssignableTo* + relater.go
    // Reports the first non-assignable call argument, preferring the relation
    // error chain (e.g. `4104` readonly-to-mutable) when one is available.
    // Go: internal/checker/checker.go:Checker.checkTypeAssignableTo (call args)
    fn report_argument_not_assignable(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        source: TypeId,
        target: TypeId,
    ) {
        if self.readonly_blocks_mutable_assignability(source, target, RelationKind::Assignable)
        {
            if let Some(report) =
                self.build_relation_error_chain(program, source, target, RelationKind::Assignable)
            {
                if report.code == 4104 {
                    let (start, end) = self.get_error_range_for_node(program, node);
                    let diagnostic = Diagnostic {
                        code: report.code,
                        category: report.category,
                        message: report.message,
                        start,
                        length: end - start,
                        related_information: Vec::new(),
                        message_chain: report.message_chain,
                    };
                    self.add_diagnostic(program, diagnostic);
                    return;
                }
            }
        }
        let generalized = self.generalized_source_for_error(source, target);
        let source_str = super::nodebuilder::type_to_string(self, program, generalized);
        let target_str = super::nodebuilder::type_to_string(self, program, target);
        self.error(
            program,
            node,
            &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1,
            &[source_str.as_str(), target_str.as_str()],
        );
    }

    pub(crate) fn report_type_not_assignable(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        source: TypeId,
        target: TypeId,
    ) {
        let Some(report) =
            self.build_relation_error_chain(program, source, target, RelationKind::Assignable)
        else {
            // Defensive: the caller only reaches here after the bool fast path
            // reported the relation as failing, so a chain is expected. Fall
            // back to the flat head if it somehow holds — still through
            // `get_error_range_for_node` so the span matches the chain path.
            let generalized = self.generalized_source_for_error(source, target);
            let source_str = super::nodebuilder::type_to_string(self, program, generalized);
            let target_str = super::nodebuilder::type_to_string(self, program, target);
            let message = &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1;
            let (start, end) = self.get_error_range_for_node(program, node);
            let diagnostic = Diagnostic {
                code: message.code(),
                category: message.category(),
                message: tsgo_diagnostics::format(
                    &message.to_string(),
                    &[source_str.as_str(), target_str.as_str()],
                ),
                start,
                length: end - start,
                related_information: Vec::new(),
                message_chain: Vec::new(),
            };
            self.add_diagnostic(program, diagnostic);
            return;
        };
        // Go reports every relation error through `createDiagnosticForNode` ->
        // `GetErrorRangeForNode`, so a `VariableDeclaration` error node narrows
        // to its name (`const x: number = ""` underlines `x`) and an expression
        // error node skips leading trivia — both byte-matching `tsc`.
        let (start, end) = self.get_error_range_for_node(program, node);
        let diagnostic = Diagnostic {
            code: report.code,
            category: report.category,
            message: report.message,
            start,
            length: end - start,
            related_information: Vec::new(),
            message_chain: report.message_chain,
        };
        self.add_diagnostic(program, diagnostic);
    }

    // Tries to elaborate an assignability failure element-wise onto the
    // offending member of a fresh object/array-literal `node`, reporting a
    // precise leaf diagnostic on that element instead of a chain hung on the
    // whole assignment (Go's `elaborateError`). Returns whether it reported.
    //
    // This is Go's "try `elaborateError` first" step of
    // `checkTypeRelatedToAndOptionallyElaborate`: when `node` is a fresh
    // object/array literal and an element is the source of the mismatch, the
    // error points at that element's node (with a `6500` related-info), which is
    // more precise than the 4bn generic relation chain. The caller falls back to
    // the generic chain ([`report_type_not_assignable`]) only when this returns
    // `false` (non-literal RHS, or no element-level mismatch found).
    //
    // DEFER(phase-4-checker-4bp+): the `isOrHasGenericConditional` early-out, the
    // `elaborateDidYouMeanToCallOrConstruct` call/construct suggestion, the
    // binary (`=`/`,`) and `as const` / JSX-expression unwrap arms, and the
    // arrow-function (`elaborateArrowFunction`) and JSX-attributes
    // (`elaborateJsxComponents`) dispatch. blocked-by: conditional types,
    // signature-return suggestion reporting, and arrow/JSX elaboration.
    // Go: internal/checker/relater.go:Checker.elaborateError
    pub(crate) fn elaborate_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        match program.arena().kind(node) {
            // Unwrap a parenthesized expression and elaborate its inner
            // expression (Go's `KindParenthesizedExpression` arm).
            Kind::ParenthesizedExpression => {
                let inner = match program.arena().data(node) {
                    NodeData::ParenthesizedExpression(d) => d.expression,
                    _ => return false,
                };
                self.elaborate_error(program, inner, source, target, relation)
            }
            Kind::ObjectLiteralExpression => {
                self.elaborate_object_literal(program, node, source, target, relation)
            }
            Kind::ArrayLiteralExpression => {
                self.elaborate_array_literal(program, node, source, target, relation)
            }
            _ => false,
        }
    }

    // Elaborates an object-literal `node` against `target` element-by-element
    // (Go's `elaborateObjectLiteral`): each `name: value` property is checked via
    // [`elaborate_element`], reporting on the offending property when its value
    // type does not relate to the contextual target property type. Returns
    // whether any element reported.
    //
    // DEFER(phase-4-checker-4bp+): spread assignments, shorthand-property /
    // method / accessor members, and computed (non-literal) property names (the
    // `Type_of_computed_property_s_value_is_0...` message). blocked-by: spread
    // typing, accessor/method member typing, and computed-name literal types.
    // Go: internal/checker/relater.go:Checker.elaborateObjectLiteral
    fn elaborate_object_literal(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        // Go: a primitive/never target has no member structure to elaborate.
        if self
            .get_type(target)
            .flags()
            .intersects(TypeFlags::PRIMITIVE | TypeFlags::NEVER)
        {
            return false;
        }
        let members = match program.arena().data(node) {
            NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
            _ => return false,
        };
        let mut reported = false;
        for member in members {
            let (name_node, value_node) = match program.arena().data(member) {
                NodeData::PropertyAssignment(d) => (d.name, d.initializer),
                // DEFER: spread / shorthand / method / accessor members.
                _ => continue,
            };
            // A `PropertyAssignment` always has an initializer; skip defensively.
            let Some(value_node) = value_node else {
                continue;
            };
            // DEFER: computed (non-literal) property names.
            let Some(name) = property_name_text(program, name_node) else {
                continue;
            };
            let name_type = self.get_string_literal_type(&name);
            reported = self.elaborate_element(
                program,
                source,
                target,
                relation,
                name_node,
                Some(value_node),
                &name,
                name_type,
            ) || reported;
        }
        reported
    }

    // Elaborates an array-literal `node` against `target` element-by-element
    // (Go's `elaborateArrayLiteral`): the literal is re-typed as a fixed-arity
    // tuple (Go's `checkArrayLiteral(node, CheckModeForceTuple)`), then each
    // element is checked via [`elaborate_element`] against the target's element
    // type at that index. Returns whether any element reported.
    //
    // DEFER(phase-4-checker-4bp+): spread / omitted elements, the contextual
    // push during the force-tuple re-check, and the tuple-target optional/rest
    // element skipping beyond the present-property check. blocked-by: spread
    // typing, contextual-type propagation, and variadic/optional tuple targets.
    // Go: internal/checker/relater.go:Checker.elaborateArrayLiteral
    fn elaborate_array_literal(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        if self
            .get_type(target)
            .flags()
            .intersects(TypeFlags::PRIMITIVE | TypeFlags::NEVER)
        {
            return false;
        }
        let elements = match program.arena().data(node) {
            NodeData::ArrayLiteralExpression(d) => d.list.nodes.clone(),
            _ => return false,
        };
        // Go re-checks with `CheckModeForceTuple` when the source is not already
        // tuple-like; the reachable subset builds the fixed-arity tuple directly
        // from each element's mutable-location (widened) type. (No contextual
        // push: the reachable target element types do not refine the source.)
        let source = if self.is_tuple_like_type(source) {
            source
        } else {
            let element_types: Vec<TypeId> = elements
                .iter()
                .map(|&element| self.check_expression_for_mutable_location(program, element))
                .collect();
            let tuple = self.create_tuple_type_ex(element_types, false);
            if !self.is_tuple_like_type(tuple) {
                return false;
            }
            tuple
        };
        let target_is_tuple_like = self.is_tuple_like_type(target);
        let mut reported = false;
        for (i, element) in elements.iter().enumerate() {
            let element = *element;
            // Go skips omitted elements and tuple-target positions with no
            // corresponding property.
            if program.arena().kind(element) == Kind::OmittedExpression
                || (target_is_tuple_like
                    && get_property_of_type(self, target, &i.to_string()).is_none())
            {
                continue;
            }
            let name_type = self.get_number_literal_type(tsgo_jsnum::Number::from(i as f64));
            let index_name = i.to_string();
            reported = self.elaborate_element(
                program,
                source,
                target,
                relation,
                element,
                Some(element),
                &index_name,
                name_type,
            ) || reported;
        }
        reported
    }

    // Checks one literal member's source type against the contextual target type
    // at `name` and, on failure, reports the leaf diagnostic on `prop_node`
    // (Go's `elaborateElement`). When the member value is itself an
    // object/array literal, the error recurses into it via [`elaborate_error`]
    // so the innermost offending element is flagged; otherwise the diagnostic is
    // anchored at `prop_node` with the relation engine's chain plus a `6500`
    // "The expected type comes from property ..." related-info. Returns whether
    // it reported.
    //
    // DEFER(phase-4-checker-4bp+): the union-target `getBestMatchingType` arm of
    // `getBestMatchIndexedAccessTypeOrUndefined`, the `exactOptionalPropertyTypes`
    // message, the custom JSX diagnostic factory, the `removeMissingType`
    // optional adjustment, and the `6501` (index-signature) / `target.symbol`
    // fallback related-info arms (which need default-library source detection).
    // blocked-by: union best-match, exact-optional types, JSX, and
    // default-library file detection.
    // Go: internal/checker/relater.go:Checker.elaborateElement
    #[allow(clippy::too_many_arguments)]
    fn elaborate_element(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        prop_node: NodeId,
        next: Option<NodeId>,
        name: &str,
        name_type: TypeId,
    ) -> bool {
        // Go: `getBestMatchIndexedAccessTypeOrUndefined` reduced to a non-union
        // target's `getIndexedAccessTypeOrUndefined`. A missing target member (no
        // index either) yields no elaboration.
        let Some(target_prop_type) = self.elaboration_member_type(program, target, name, name_type)
        else {
            return false;
        };
        let Some(source_prop_type) = self.elaboration_member_type(program, source, name, name_type)
        else {
            return false;
        };
        if self.is_type_related_to(program, source_prop_type, target_prop_type, relation) {
            return false;
        }
        // A nested object/array literal value elaborates its own offending
        // element instead of reporting on this one.
        if let Some(next) = next {
            if self.elaborate_error(program, next, source_prop_type, target_prop_type, relation) {
                return true;
            }
        }
        // Use the expression's (widened) type for the leaf message, mirroring
        // Go's `checkExpressionForMutableLocationWithContextualType` (the
        // contextual push is unmodeled in the reachable subset).
        let specific_source = match next {
            Some(next) => self.check_expression_for_mutable_location(program, next),
            None => source_prop_type,
        };
        let Some(report) =
            self.build_relation_error_chain(program, specific_source, target_prop_type, relation)
        else {
            return false;
        };
        let loc = program.arena().loc(prop_node);
        let mut diagnostic = Diagnostic {
            code: report.code,
            category: report.category,
            message: report.message,
            start: loc.pos(),
            length: loc.end() - loc.pos(),
            related_information: Vec::new(),
            message_chain: report.message_chain,
        };
        // The `6500` "The expected type comes from property '0' ..." related-info
        // points at the target property's declaration. Go also has the `6501`
        // index-signature arm and a `target.symbol` fallback, both gated on the
        // declaration not being in a default library — DEFER (the reachable
        // object-literal target always resolves a user-declared property here).
        // A synthesized (object-literal) target property carries no program
        // declaration node; only a real program symbol has one to point at.
        if let Some(target_prop) =
            get_property_of_type(self, target, name).filter(|&p| !super::is_synthesized_symbol(p))
        {
            if let Some(decl) = program.symbol(target_prop).declarations.first().copied() {
                let decl_name = declaration_name_node(program, decl).unwrap_or(decl);
                let target_str = super::nodebuilder::type_to_string(self, program, target);
                let related = self.diagnostic_for_node(
                    program,
                    decl_name,
                    &tsgo_diagnostics::THE_EXPECTED_TYPE_COMES_FROM_PROPERTY_0_WHICH_IS_DECLARED_HERE_ON_TYPE_1,
                    &[name, target_str.as_str()],
                );
                diagnostic.add_related_info(related);
            }
        }
        self.add_diagnostic(program, diagnostic);
        true
    }

    // Resolves the member type of `obj` at `name`/`name_type` for elaboration,
    // mirroring Go's `getIndexedAccessTypeOrUndefined`: a named property by its
    // literal name, else an index-signature / tuple-element access. Returns
    // `None` when the member is absent (Go's "don't elaborate" sentinel).
    // Go: internal/checker/checker.go:Checker.getIndexedAccessTypeOrUndefined
    fn elaboration_member_type(
        &mut self,
        program: &dyn BoundProgram,
        obj: TypeId,
        name: &str,
        name_type: TypeId,
    ) -> Option<TypeId> {
        if let Some(t) = self.get_type_of_property_of_type(program, obj, name) {
            return Some(t);
        }
        get_indexed_access_type(self, program, obj, name_type)
    }

    // Reports whether `t` is tuple-like (Go's `isTupleLikeType` reachable
    // subset): a fixed-arity tuple object, or an object with a `"0"` property.
    //
    // DEFER(phase-4-checker-4bp+): the array-like + numeric-literal-`length`
    // arm. blocked-by: `isArrayLikeType` (`ReadonlyArray<any>` assignability,
    // which needs lib globals).
    // Go: internal/checker/checker.go:Checker.isTupleLikeType(23405)
    fn is_tuple_like_type(&mut self, t: TypeId) -> bool {
        self.get_type(t).object_flags().contains(ObjectFlags::TUPLE)
            || get_property_of_type(self, t, "0").is_some()
    }

    // Records an already-built diagnostic into the per-file collection, keyed by
    // the file `program` is a view of (Go's `c.diagnostics.Add`).
    pub(crate) fn add_diagnostic(&mut self, program: &dyn BoundProgram, diagnostic: Diagnostic) {
        self.diagnostics_by_file
            .entry(program.file_handle())
            .or_default()
            .push(diagnostic);
    }

    // Records a suggestion diagnostic at `node` (Go's `addErrorOrSuggestion` when
    // `isError` is false).
    // Go: internal/checker/checker.go:Checker.addErrorOrSuggestion(13917)
    pub(crate) fn add_suggestion(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static Message,
        args: &[&str],
    ) {
        let mut diagnostic = self.diagnostic_for_node(program, node, message, args);
        diagnostic.category = Category::Suggestion;
        self.suggestion_diagnostics_by_file
            .entry(program.file_handle())
            .or_default()
            .push(diagnostic);
    }

    // Routes a diagnostic to the error or suggestion collection (Go's
    // `addErrorOrSuggestion`).
    // Go: internal/checker/checker.go:Checker.addErrorOrSuggestion(13917)
    pub(crate) fn add_error_or_suggestion(
        &mut self,
        program: &dyn BoundProgram,
        is_error: bool,
        node: NodeId,
        message: &'static Message,
        args: &[&str],
    ) {
        if is_error {
            self.error(program, node, message, args);
        } else {
            self.add_suggestion(program, node, message, args);
        }
    }

    // -- Unused-identifiers subsystem (T1-C5) ---------------------------------

    /// Registers `node` for post-check unused-identifiers analysis (Go's
    /// `registerForUnusedIdentifiersCheck`).
    ///
    /// Side effects: appends `node` to the checker's deferred-check list.
    // Go: internal/checker/checker.go:Checker.registerForUnusedIdentifiersCheck(7008)
    pub(crate) fn register_for_unused_identifiers_check(&mut self, node: NodeId) {
        self.unused_identifier_nodes.push(node);
    }

    /// Runs the unused-identifiers check over all nodes registered during the
    /// statement walk (Go's `checkUnusedIdentifiers`).
    ///
    /// DEFER(phase-4-checker-C5+): class-member, type-parameter, and infer-type
    /// unused checking. blocked-by: class-member unused detection, type
    /// parameter reference tracking.
    ///
    /// Side effects: may record TS6133/TS6196/TS6199 diagnostics.
    // Go: internal/checker/checker.go:Checker.checkUnusedIdentifiers(7014)
    fn check_unused_identifiers(&mut self, program: &dyn BoundProgram) {
        let nodes: Vec<NodeId> = std::mem::take(&mut self.unused_identifier_nodes);
        for node in nodes {
            let kind = program.arena().kind(node);
            match kind {
                Kind::SourceFile
                | Kind::Block
                | Kind::CaseBlock
                | Kind::ForStatement
                | Kind::ForInStatement
                | Kind::ForOfStatement => {
                    self.check_unused_locals_and_parameters(program, node);
                }
                Kind::Constructor
                | Kind::FunctionExpression
                | Kind::FunctionDeclaration
                | Kind::ArrowFunction
                | Kind::MethodDeclaration
                | Kind::GetAccessor
                | Kind::SetAccessor => {
                    if function_like_body(program, node).is_some() {
                        self.check_unused_locals_and_parameters(program, node);
                    }
                    // DEFER: checkUnusedTypeParameters
                }
                // DEFER: ClassDeclaration, ClassExpression -> checkUnusedClassMembers +
                // checkUnusedTypeParameters; MethodSignature, CallSignature, etc. ->
                // checkUnusedTypeParameters; InferType -> checkUnusedInferTypeParameter.
                _ => {}
            }
        }
    }

    /// Iterates the `Locals` table of `node` and reports unused local variables
    /// and parameters (Go's `checkUnusedLocalsAndParameters`).
    ///
    /// DEFER(phase-4-checker-C5+): import-clause aggregation, type-parameter
    /// union-flag fast-path. blocked-by: import tracking, full symbol merging.
    ///
    /// Side effects: may record diagnostics.
    // Go: internal/checker/checker.go:Checker.checkUnusedLocalsAndParameters(7108)
    fn check_unused_locals_and_parameters(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let locals = match program.locals(node) {
            Some(locals) => locals.clone(),
            None => return,
        };
        let mut variable_parents: FxHashSet<NodeId> = FxHashSet::default();

        for &symbol_id in locals.values() {
            let sym = program.symbol(symbol_id);
            let reference_kinds = self
                .symbol_reference_links
                .try_get(&symbol_id)
                .map(|l| l.reference_kinds)
                .unwrap_or_else(SymbolFlags::empty);

            // Skip type parameters that have been referenced as types (the
            // value-side may still be unreferenced). Also skip symbols that are
            // exported or are namespace-exports.
            if sym.flags.intersects(SymbolFlags::TYPE_PARAMETER)
                && (!sym.flags.intersects(SymbolFlags::VARIABLE)
                    || reference_kinds.intersects(SymbolFlags::VARIABLE))
            {
                continue;
            }
            if !sym.flags.intersects(SymbolFlags::TYPE_PARAMETER)
                && (!reference_kinds.is_empty()
                    || sym.export_symbol.is_some()
                    || sym.flags.intersects(SymbolFlags::MODULE_EXPORTS))
            {
                continue;
            }

            for decl in &sym.declarations {
                let decl = *decl;
                let decl_kind = program.arena().kind(decl);
                if matches!(
                    decl_kind,
                    Kind::VariableDeclaration | Kind::Parameter | Kind::BindingElement
                ) {
                    let root = get_root_declaration(program, decl);
                    if let Some(parent) = program.arena().parent(root) {
                        variable_parents.insert(parent);
                    }
                } else if !matches!(
                    decl_kind,
                    Kind::TypeParameter
                        | Kind::ImportClause
                        | Kind::ImportSpecifier
                        | Kind::NamespaceImport
                ) && !is_ambient_module(program, decl)
                {
                    self.report_unused_local(program, decl, &sym.name);
                }
            }
        }

        for declaration in variable_parents {
            let kind = program.arena().kind(declaration);
            if kind == Kind::VariableDeclarationList {
                self.report_unused_variables(program, declaration);
            } else {
                self.report_unused_parameters(program, declaration);
            }
        }
        // DEFER: import-clause unused aggregation.
    }

    /// Reports a single unused local (non-variable, non-parameter).
    // Go: internal/checker/checker.go:Checker.reportUnusedLocal(7149)
    fn report_unused_local(&mut self, program: &dyn BoundProgram, node: NodeId, name: &str) {
        let is_type_decl = is_type_declaration(program, node);
        let message = if is_type_decl {
            &tsgo_diagnostics::X_0_IS_DECLARED_BUT_NEVER_USED
        } else {
            &tsgo_diagnostics::X_0_IS_DECLARED_BUT_ITS_VALUE_IS_NEVER_READ
        };
        let name_node = name_of_node(program, node).unwrap_or(node);
        let diag = self.diagnostic_for_node(program, name_node, message, &[name]);
        self.report_unused(program, UnusedKind::Local, diag);
    }

    /// Reports a variable-list or binding-pattern where ALL declarations are
    /// unused (TS6199 "All variables are unused.").
    // Go: internal/checker/checker.go:Checker.reportUnusedVariables(7154)
    fn report_unused_variables(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let declarations = match program.arena().data(node) {
            NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
            _ => return,
        };
        if declarations.len() > 1
            && declarations
                .iter()
                .all(|&d| self.is_unreferenced_variable_declaration(program, d))
        {
            let diag = self.diagnostic_for_node(
                program,
                node,
                &tsgo_diagnostics::ALL_VARIABLES_ARE_UNUSED,
                &[],
            );
            self.report_unused_variable(program, node, diag);
        } else {
            self.report_unused_variable_declarations(program, &declarations);
        }
    }

    /// Reports unused parameters of a function.
    // Go: internal/checker/checker.go:Checker.reportUnusedParameters(7163)
    fn report_unused_parameters(&mut self, program: &dyn BoundProgram, node: NodeId) {
        let params = function_like_all_parameters(program, node);
        self.report_unused_variable_declarations(program, &params);
    }

    /// Reports individual unused variable/parameter declarations.
    // Go: internal/checker/checker.go:Checker.reportUnusedVariableDeclarations(7176)
    fn report_unused_variable_declarations(
        &mut self,
        program: &dyn BoundProgram,
        declarations: &[NodeId],
    ) {
        for &declaration in declarations {
            let Some(name) = name_of_node(program, declaration) else {
                continue;
            };
            if is_parameter_property_declaration(program, declaration) {
                continue;
            }
            if is_this_parameter(program, declaration) {
                continue;
            }
            // DEFER: binding-pattern recursion (reportUnusedBindingElements).
            if self.is_unreferenced_variable_declaration(program, declaration) {
                let name_text = program.arena().text(name);
                let diag = self.diagnostic_for_node(
                    program,
                    name,
                    &tsgo_diagnostics::X_0_IS_DECLARED_BUT_ITS_VALUE_IS_NEVER_READ,
                    &[name_text],
                );
                self.report_unused_variable(program, declaration, diag);
            }
        }
    }

    /// Walks up from a binding element to the enclosing variable statement or
    /// parameter and routes to the correct unused-kind.
    // Go: internal/checker/checker.go:Checker.reportUnusedVariable(7052)
    fn report_unused_variable(
        &mut self,
        program: &dyn BoundProgram,
        mut location: NodeId,
        diagnostic: Diagnostic,
    ) {
        while matches!(
            program.arena().kind(location),
            Kind::BindingElement | Kind::ArrayBindingPattern | Kind::ObjectBindingPattern
        ) {
            if let Some(parent) = program.arena().parent(location) {
                location = parent;
            } else {
                break;
            }
        }
        let kind = if is_parameter_declaration(program, location) {
            UnusedKind::Parameter
        } else {
            UnusedKind::Local
        };
        self.report_unused(program, kind, diagnostic);
    }

    /// Routes an unused diagnostic as error or suggestion depending on
    /// compiler options.
    // Go: internal/checker/checker.go:Checker.reportUnused(7059)
    fn report_unused(
        &mut self,
        program: &dyn BoundProgram,
        kind: UnusedKind,
        diagnostic: Diagnostic,
    ) {
        // Go: `if location.Flags&(NodeFlagsAmbient|NodeFlagsThisNodeOrAnySubNodesHasError) == 0`
        // DEFER: the ambient/error flag check (needs NodeFlags wiring for
        // declarations). For now, always emit.
        let is_error = self.unused_is_error(kind);
        if is_error {
            self.add_diagnostic(program, diagnostic);
        }
        // DEFER: suggestion diagnostics collection (non-error unused).
    }

    /// Whether an unused-`kind` diagnostic should be an error (vs. suggestion).
    // Go: internal/checker/checker.go:Checker.unusedIsError(7072)
    fn unused_is_error(&self, kind: UnusedKind) -> bool {
        match kind {
            UnusedKind::Local => self.compiler_options().no_unused_locals.is_true(),
            UnusedKind::Parameter => self.compiler_options().no_unused_parameters.is_true(),
        }
    }

    /// Reports whether a variable/parameter/binding-element declaration is
    /// unreferenced (Go's `isUnreferencedVariableDeclaration`).
    // Go: internal/checker/checker.go:Checker.isUnreferencedVariableDeclaration(7189)
    fn is_unreferenced_variable_declaration(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        let Some(name) = name_of_node(program, node) else {
            return true;
        };
        let name_kind = program.arena().kind(name);
        // DEFER: binding-pattern recursion.
        if matches!(
            name_kind,
            Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
        ) {
            return false;
        }
        let sym = program.symbol_of_node(node);
        let referenced = sym.is_some_and(|s| {
            self.symbol_reference_links
                .try_get(&s)
                .map(|l| l.reference_kinds.intersects(SymbolFlags::VARIABLE))
                .unwrap_or(false)
        });
        if referenced {
            return false;
        }
        // Parameters or variables starting with `_` suppress unused reporting.
        let is_param = is_parameter_declaration(program, node);
        if is_param && is_identifier_that_starts_with_underscore(program, name) {
            return false;
        }
        true
    }

    /// Reports whether `t` is a tuple type (Go's `isTupleType`): an object type
    /// carrying the `TUPLE` flag.
    // Go: internal/checker/checker.go:isTupleType(23350)
    pub fn is_tuple_type(&self, t: TypeId) -> bool {
        self.get_type(t).object_flags().contains(ObjectFlags::TUPLE)
    }

    // Whether a type-reference target is the global `Array` or `ReadonlyArray`
    // interface (intrinsic stub or a synthetic `interface Array<T>` in scope).
    fn array_reference_target_name(&self, target: TypeId) -> Option<&'static str> {
        match self.get_type(target).intrinsic_name() {
            Some("Array") => Some("Array"),
            Some("ReadonlyArray") => Some("ReadonlyArray"),
            _ => {
                if self.global_types.get("Array") == Some(&target) {
                    Some("Array")
                } else if self.global_types.get("ReadonlyArray") == Some(&target) {
                    Some("ReadonlyArray")
                } else {
                    None
                }
            }
        }
    }

    /// Reports whether `t` is an array type (Go's `isArrayType`).
    // Go: internal/checker/checker.go:Checker.isArrayType(23342)
    pub fn is_array_type(&self, t: TypeId) -> bool {
        let ty = self.get_type(t);
        if !ty.object_flags().contains(ObjectFlags::REFERENCE) {
            return false;
        }
        if let Some(obj) = ty.as_object() {
            if let Some(target) = obj.target {
                return self.array_reference_target_name(target).is_some();
            }
        }
        false
    }

    /// Reports whether `t` is a readonly array type (Go's `isReadonlyArrayType`).
    // Go: internal/checker/checker.go:Checker.isReadonlyArrayType(23346)
    pub fn is_readonly_array_type(&self, t: TypeId) -> bool {
        let ty = self.get_type(t);
        if !ty.object_flags().contains(ObjectFlags::REFERENCE) {
            return false;
        }
        if let Some(obj) = ty.as_object() {
            if let Some(target) = obj.target {
                return self.array_reference_target_name(target) == Some("ReadonlyArray");
            }
        }
        false
    }

    /// Reports whether `t` is an array or tuple type (Go's `isArrayOrTupleType`).
    // Go: internal/checker/checker.go:Checker.isArrayOrTupleType(23366)
    pub fn is_array_or_tuple_type(&self, t: TypeId) -> bool {
        self.is_array_type(t) || self.is_tuple_type(t)
    }

    /// Reports whether `t` is a readonly tuple (`as const` / `readonly [...]`).
    // Go: internal/checker/checker.go:isTupleType + TargetTupleType().readonly
    pub(crate) fn is_readonly_tuple_type(&self, t: TypeId) -> bool {
        self.is_tuple_type(t)
            && self
                .get_type(t)
                .as_object()
                .is_some_and(|object| object.readonly)
    }

    /// Reports whether `t` is a mutable array or tuple (not `readonly`).
    // Go: internal/checker/checker.go:Checker.isMutableArrayOrTuple(23370)
    pub(crate) fn is_mutable_array_or_tuple(&self, t: TypeId) -> bool {
        (self.is_array_type(t) && !self.is_readonly_array_type(t))
            || (self.is_tuple_type(t) && !self.is_readonly_tuple_type(t))
    }

    /// Whether assignability from `source` to `target` is blocked because a
    /// readonly array/tuple cannot be assigned to a mutable one.
    // Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm)
    pub(crate) fn readonly_blocks_mutable_assignability(
        &self,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        relation == RelationKind::Assignable
            && self.is_mutable_array_or_tuple(target)
            && (self.is_readonly_array_type(source) || self.is_readonly_tuple_type(source))
    }

    /// Returns the element type of an array type reference (`Array<T>` /
    /// `ReadonlyArray<T>`), or `None` when `t` is not an array type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let n = c.number_type();
    /// let arr = c.create_array_type(n);
    /// assert_eq!(c.get_element_type_of_array_type(arr), Some(n));
    /// assert_eq!(c.get_element_type_of_array_type(n), None);
    /// ```
    ///
    /// Side effects: none (pure read over the type arena).
    // Go: internal/checker/checker.go:Checker.getElementTypeOfArrayType(23374)
    pub fn get_element_type_of_array_type(&self, t: TypeId) -> Option<TypeId> {
        if !self.is_array_type(t) {
            return None;
        }
        self.get_type(t)
            .as_object()
            .and_then(|obj| obj.resolved_type_arguments.first().copied())
    }

    /// Returns the element type at `index` in a tuple or tuple-like type (Go's
    /// `getTupleElementType`): in-range indices read the positional element;
    /// out-of-range indices on a tuple resolve through the rest element (when
    /// present) or `undefined`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let s = c.string_type();
    /// let n = c.number_type();
    /// let tuple = c.create_tuple_type(vec![s, n]);
    /// assert_eq!(c.get_tuple_element_type(tuple, 0), Some(s));
    /// assert_eq!(c.get_tuple_element_type(tuple, 1), Some(n));
    /// ```
    ///
    /// Side effects: may allocate union types for out-of-range access.
    // Go: internal/checker/checker.go:Checker.getTupleElementType(23425)
    pub fn get_tuple_element_type(&mut self, t: TypeId, index: usize) -> Option<TypeId> {
        let name = index.to_string();
        if let Some(program) = self.retained_program() {
            if let Some(prop_type) = self.get_type_of_property_of_type(program.as_ref(), t, &name) {
                return Some(prop_type);
            }
        }
        if self.is_tuple_type(t) {
            if let Some(obj) = self.get_type(t).as_object() {
                if let Some(&element) = obj.resolved_type_arguments.get(index) {
                    return Some(element);
                }
            }
            return Some(self.get_tuple_element_type_out_of_start_count(t, index));
        }
        if self.every_type_is_tuple(t) {
            return Some(self.get_tuple_element_type_out_of_start_count(t, index));
        }
        None
    }

    /// Returns the rest-element type of a tuple (the union of element types
    /// from `fixedLength` onward), or `None` when the tuple has no rest element.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let s = c.string_type();
    /// let n = c.number_type();
    /// let tuple = c.create_tuple_type(vec![s, n]);
    /// assert_eq!(c.get_rest_type_of_tuple_type(tuple), None);
    /// ```
    ///
    /// Side effects: may allocate union types.
    // Go: internal/checker/checker.go:Checker.getRestTypeOfTupleType(24712)
    pub fn get_rest_type_of_tuple_type(&mut self, t: TypeId) -> Option<TypeId> {
        let fixed_length = self.tuple_fixed_length(t)?;
        self.get_element_type_of_slice_of_tuple_type(t, fixed_length, 0, false)
    }

    /// Slices a tuple type from `index`, optionally skipping `end_skip_count`
    /// elements at the end (Go's `sliceTupleType`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let s = c.string_type();
    /// let n = c.number_type();
    /// let tuple = c.create_tuple_type(vec![s, n]);
    /// let tail = c.slice_tuple_type(tuple, 1, 0);
    /// assert_eq!(c.get_tuple_element_type(tail, 0), Some(n));
    /// ```
    ///
    /// Side effects: mutates the checker's type arena.
    // Go: internal/checker/relater.go:Checker.sliceTupleType(1879)
    pub fn slice_tuple_type(&mut self, t: TypeId, index: usize, end_skip_count: usize) -> TypeId {
        let fixed_length = self.tuple_fixed_length(t).unwrap_or(0);
        let end_index = self
            .tuple_reference_arity(t)
            .unwrap_or(0)
            .saturating_sub(end_skip_count);
        if index > fixed_length {
            if let Some(rest_array) = self.get_rest_array_type_of_tuple_type(t) {
                return rest_array;
            }
            return self.create_tuple_type(vec![]);
        }
        if index >= end_index {
            return self.create_tuple_type(vec![]);
        }
        let elements = self
            .get_type(t)
            .as_object()
            .map(|o| o.resolved_type_arguments.clone())
            .unwrap_or_default();
        let readonly = self
            .get_type(t)
            .as_object()
            .map(|o| o.readonly)
            .unwrap_or(false);
        self.create_tuple_type_ex(elements[index..end_index].to_vec(), readonly)
    }

    /// Creates an array type by wrapping `element_type` in a generic type
    /// reference to a named array target.
    // Go: internal/checker/checker.go:Checker.createArrayType(24562)
    pub fn create_array_type(&mut self, element_type: TypeId) -> TypeId {
        self.create_array_type_ex(element_type, false)
    }

    /// Creates an array type reference, with an optional `readonly` flag.
    // Go: internal/checker/checker.go:Checker.createArrayTypeEx(24566)
    pub fn create_array_type_ex(&mut self, element_type: TypeId, readonly: bool) -> TypeId {
        let name = if readonly { "ReadonlyArray" } else { "Array" };
        let target = self.get_or_create_array_target(name);
        self.create_type_reference(target, vec![element_type])
    }

    fn get_or_create_array_target(&mut self, name: &str) -> TypeId {
        if let Some(&id) = self.global_types.get(name) {
            return id;
        }
        let tp = self.new_type_parameter(None);
        let target = self.new_object_type(
            ObjectFlags::INTERFACE,
            None,
            ObjectType {
                type_parameters: vec![tp],
                ..Default::default()
            },
        );
        self.types.get_mut(target).data = TypeData::Intrinsic(IntrinsicType {
            intrinsic_name: name.to_string(),
        });
        self.global_types.insert(name.to_string(), target);
        target
    }

    // Returns the union of element types from `index` through the end of a tuple
    // (minus `end_skip_count` trailing elements), or `None` when `index` is
    // out of range. The `writing` flag selects intersection vs union (Go uses
    // union for read contexts and intersection for write contexts).
    //
    // DEFER(phase-4-checker-4af+): variadic tuple elements (`ElementFlagsVariadic`
    // -> indexed access over the variadic type). blocked-by: tuple element flags.
    // Go: internal/checker/checker.go:Checker.getElementTypeOfSliceOfTupleType(24691)
    fn get_element_type_of_slice_of_tuple_type(
        &mut self,
        t: TypeId,
        index: usize,
        end_skip_count: usize,
        writing: bool,
    ) -> Option<TypeId> {
        let length = self
            .tuple_reference_arity(t)?
            .saturating_sub(end_skip_count);
        if index >= length {
            return None;
        }
        let elements = self
            .get_type(t)
            .as_object()?
            .resolved_type_arguments
            .clone();
        let slice = &elements[index..length];
        if writing {
            Some(self.get_intersection_type(slice))
        } else {
            Some(self.get_union_type(slice))
        }
    }

    // Returns `Array<restElementType>` when the tuple has a rest element, or
    // `None` when it does not.
    // Go: internal/checker/relater.go:Checker.getRestArrayTypeOfTupleType(1904)
    fn get_rest_array_type_of_tuple_type(&mut self, t: TypeId) -> Option<TypeId> {
        let rest = self.get_rest_type_of_tuple_type(t)?;
        Some(self.create_array_type(rest))
    }

    // Resolves an out-of-range tuple index to the rest element type (when
    // present) or `undefined` (Go's `getTupleElementTypeOutOfStartCount`).
    //
    // DEFER(phase-4-checker-4af+): variadic/rest tuples and
    // `noUncheckedIndexedAccess` (union with `undefined` when enabled).
    // blocked-by: tuple element flags + compiler-option wiring.
    // Go: internal/checker/checker.go:Checker.getTupleElementTypeOutOfStartCount(24716)
    fn get_tuple_element_type_out_of_start_count(&mut self, t: TypeId, index: usize) -> TypeId {
        let undefined_like = if self
            .compiler_options()
            .no_unchecked_indexed_access
            .is_true()
        {
            Some(self.undefined_type)
        } else {
            None
        };
        let members = self.distributed_types(t);
        let mut mapped = Vec::with_capacity(members.len());
        for member in members {
            if let Some(rest) = self.get_rest_type_of_tuple_type(member) {
                if let Some(undefined) = undefined_like {
                    if index >= self.tuple_fixed_length(member).unwrap_or(0) {
                        mapped.push(self.get_union_type(&[rest, undefined]));
                    } else {
                        mapped.push(rest);
                    }
                } else {
                    mapped.push(rest);
                }
            } else {
                mapped.push(self.undefined_type);
            }
        }
        self.get_union_type(&mapped)
    }

    // Reports whether every constituent of `t` is a tuple type (Go's
    // `everyType(t, isTupleType)`).
    fn every_type_is_tuple(&self, t: TypeId) -> bool {
        self.distributed_types(t)
            .iter()
            .all(|&member| self.is_tuple_type(member))
    }

    // Returns the number of type arguments on a fixed-arity tuple (Go's
    // `getTypeReferenceArity` for tuple references).
    fn tuple_reference_arity(&self, t: TypeId) -> Option<usize> {
        if !self.is_tuple_type(t) {
            return None;
        }
        self.get_type(t)
            .as_object()
            .map(|obj| obj.resolved_type_arguments.len())
    }

    // Returns the fixed (non-rest) length of a tuple (Go's
    // `TargetTupleType().fixedLength`).
    pub(crate) fn tuple_fixed_length(&self, t: TypeId) -> Option<usize> {
        let obj = self.get_type(t).as_object()?;
        let arity = obj.resolved_type_arguments.len();
        Some(obj.tuple_fixed_length.unwrap_or(arity))
    }

    // Reports whether `t` is a tuple with a rest element (`[A, ...B]`).
    pub(crate) fn tuple_has_rest_element(&self, t: TypeId) -> bool {
        let Some(obj) = self.get_type(t).as_object() else {
            return false;
        };
        let arity = obj.resolved_type_arguments.len();
        obj.tuple_fixed_length.is_some_and(|fixed| fixed < arity)
    }

    // Minimum required element count for tuple assignability (Go's `minLength`).
    pub(crate) fn tuple_min_length(&self, t: TypeId) -> usize {
        let Some(obj) = self.get_type(t).as_object() else {
            return 0;
        };
        obj.tuple_min_length.unwrap_or_else(|| {
            self.tuple_fixed_length(t).unwrap_or(0)
        })
    }

    // Whether tuple position `index` is optional (Go's `ElementFlagsOptional`).
    pub(crate) fn tuple_element_is_optional(&self, t: TypeId, index: usize) -> bool {
        let Some(obj) = self.get_type(t).as_object() else {
            return false;
        };
        obj.tuple_element_optional
            .as_ref()
            .and_then(|flags| flags.get(index))
            .copied()
            .unwrap_or(false)
    }

    // Whether tuple position `index` is required (Go's `ElementFlagsRequired`).
    pub(crate) fn tuple_element_is_required(&self, t: TypeId, index: usize) -> bool {
        if self.tuple_has_rest_element(t) {
            let fixed = self.tuple_fixed_length(t).unwrap_or(0);
            if index >= fixed {
                return false;
            }
        }
        !self.tuple_element_is_optional(t, index)
    }

    // Whether tuple position `index` is a rest element (`...T[]`).
    pub(crate) fn tuple_element_is_rest(&self, t: TypeId, index: usize) -> bool {
        let Some(obj) = self.get_type(t).as_object() else {
            return false;
        };
        obj.tuple_element_rest
            .as_ref()
            .and_then(|flags| flags.get(index))
            .copied()
            .unwrap_or(false)
    }

    // Whether tuple position `index` is a variadic element (`...T`).
    pub(crate) fn tuple_element_is_variadic(&self, t: TypeId, index: usize) -> bool {
        let Some(obj) = self.get_type(t).as_object() else {
            return false;
        };
        obj.tuple_element_variadic
            .as_ref()
            .and_then(|flags| flags.get(index))
            .copied()
            .unwrap_or(false)
    }

    // Whether `t` has a rest or variadic element (Go's `ElementFlagsVariable`).
    pub(crate) fn tuple_has_variable_element(&self, t: TypeId) -> bool {
        if self.tuple_has_rest_element(t) {
            return true;
        }
        let Some(obj) = self.get_type(t).as_object() else {
            return false;
        };
        obj.tuple_element_variadic
            .as_ref()
            .is_some_and(|flags| flags.iter().any(|&v| v))
    }

    // Count of leading fixed (non-rest) elements before a variable tail.
    pub(crate) fn tuple_start_element_count(&self, t: TypeId) -> usize {
        let Some(obj) = self.get_type(t).as_object() else {
            return 0;
        };
        if let Some(rest) = &obj.tuple_element_rest {
            if let Some(pos) = rest.iter().position(|&r| r) {
                return pos;
            }
        }
        if let Some(variadic) = &obj.tuple_element_variadic {
            if let Some(pos) = variadic.iter().position(|&v| v) {
                return pos;
            }
        }
        obj.resolved_type_arguments.len()
    }

    // Count of trailing fixed elements after a variable tail (0 for `[A, ...B]`).
    pub(crate) fn tuple_end_element_count(&self, t: TypeId) -> usize {
        let Some(obj) = self.get_type(t).as_object() else {
            return 0;
        };
        let arity = obj.resolved_type_arguments.len();
        if let Some(rest) = &obj.tuple_element_rest {
            if let Some(pos) = rest.iter().rposition(|&r| r) {
                return arity - 1 - pos;
            }
        }
        if let Some(variadic) = &obj.tuple_element_variadic {
            if let Some(pos) = variadic.iter().rposition(|&v| v) {
                return arity - 1 - pos;
            }
        }
        0
    }

    /// Performs assignability checking with optional elaboration (Go's
    /// `checkTypeAssignableToAndOptionallyElaborate`).
    // Go: internal/checker/checker.go:Checker.checkTypeAssignableToAndOptionallyElaborate(12568)
    #[allow(dead_code)]
    pub(crate) fn check_type_assignable_to_and_optionally_elaborate(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        error_node: NodeId,
        expr: Option<NodeId>,
    ) {
        if self.is_type_assignable_to(program, source, target) {
            return;
        }
        if let Some(expr_node) = expr {
            if self.elaborate_error(program, expr_node, source, target, RelationKind::Assignable) {
                return;
            }
        }
        self.report_type_not_assignable(program, error_node, source, target);
    }
}

/// The kind of unused diagnostic: local variable or parameter (Go's
/// `UnusedKind` iota).
// Go: internal/checker/checker.go:UnusedKind
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnusedKind {
    Local,
    Parameter,
}

// Walks up from a declaration to the root (non-binding-element) declaration.
// Go: internal/ast/utilities.go:GetRootDeclaration
fn get_root_declaration(program: &dyn BoundProgram, mut node: NodeId) -> NodeId {
    while program.arena().kind(node) == Kind::BindingElement {
        if let Some(parent) = program.arena().parent(node) {
            if matches!(
                program.arena().kind(parent),
                Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
            ) {
                if let Some(grandparent) = program.arena().parent(parent) {
                    node = grandparent;
                    continue;
                }
            }
        }
        break;
    }
    node
}

// Reports whether `node` is a parameter declaration.
// Go: internal/ast/utilities.go:IsParameterDeclaration
fn is_parameter_declaration(program: &dyn BoundProgram, node: NodeId) -> bool {
    let root = get_root_declaration(program, node);
    program.arena().kind(root) == Kind::Parameter
}

// Reports whether `node` is a `this: T` parameter.
// Go: internal/ast/ast.go:IsThisParameter
fn is_this_parameter(program: &dyn BoundProgram, node: NodeId) -> bool {
    if program.arena().kind(node) != Kind::Parameter {
        return false;
    }
    if let Some(name) = name_of_node(program, node) {
        program.arena().kind(name) == Kind::Identifier && program.arena().text(name) == "this"
    } else {
        false
    }
}

// Reports whether `node` is a parameter property (has accessibility modifier).
// Go: internal/ast/utilities.go:IsParameterPropertyDeclaration
fn is_parameter_property_declaration(program: &dyn BoundProgram, node: NodeId) -> bool {
    if program.arena().kind(node) != Kind::Parameter {
        return false;
    }
    let Some(parent) = program.arena().parent(node) else {
        return false;
    };
    program.arena().kind(parent) == Kind::Constructor
        && has_syntactic_modifier(
            program,
            node,
            tsgo_ast::ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
        )
}

fn is_parameter_property_binding_pattern(program: &dyn BoundProgram, param: NodeId) -> bool {
    let NodeData::ParameterDeclaration(d) = program.arena().data(param) else {
        return false;
    };
    matches!(
        program.arena().kind(d.name),
        Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
    )
}

// Reports whether `node` has the given syntactic modifier.
// Go: internal/ast/utilities.go:HasSyntacticModifier
pub(crate) fn has_syntactic_modifier(
    program: &dyn BoundProgram,
    node: NodeId,
    flag: tsgo_ast::ModifierFlags,
) -> bool {
    get_syntactic_modifier_flags(program, node).intersects(flag)
}

// Collects the modifier flags from a node's modifier list.
// Go: internal/ast/utilities.go:GetSyntacticModifierFlags
fn get_syntactic_modifier_flags(
    program: &dyn BoundProgram,
    node: NodeId,
) -> tsgo_ast::ModifierFlags {
    let modifiers = match program.arena().data(node) {
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref().map(|m| &m.list.nodes),
        _ => None,
    };
    let Some(modifiers) = modifiers else {
        return tsgo_ast::ModifierFlags::empty();
    };
    let mut flags = tsgo_ast::ModifierFlags::empty();
    for &m in modifiers {
        flags |= tsgo_ast::utilities::modifier_to_flag(program.arena().kind(m));
    }
    flags
}

// Reports whether `node` is a type declaration (type alias, interface, enum).
// Go: internal/ast/utilities.go:IsTypeDeclaration
fn is_type_declaration(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().kind(node),
        Kind::TypeAliasDeclaration | Kind::InterfaceDeclaration | Kind::EnumDeclaration
    )
}

// Reports whether `node` is an ambient module (`declare module "foo" { ... }`).
// Go: internal/ast/utilities.go:IsAmbientModule
fn is_ambient_module(program: &dyn BoundProgram, node: NodeId) -> bool {
    if program.arena().kind(node) != Kind::ModuleDeclaration {
        return false;
    }
    if let NodeData::ModuleDeclaration(d) = program.arena().data(node) {
        program.arena().kind(d.name) == Kind::StringLiteral
            || is_global_scope_augmentation(program, node)
    } else {
        false
    }
}

// Go: internal/ast/utilities.go:IsGlobalScopeAugmentation
fn is_global_scope_augmentation(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().data(node),
        NodeData::ModuleDeclaration(d) if d.keyword == Kind::GlobalKeyword
    )
}

// Go: internal/ast/utilities.go:IsExternalModuleAugmentation(3533)
fn is_external_module_augmentation(program: &dyn BoundProgram, node: NodeId) -> bool {
    is_ambient_module(program, node) && is_module_augmentation_external(program, node)
}

// Go: internal/ast/utilities.go:IsModuleAugmentationExternal(1660)
fn is_module_augmentation_external(program: &dyn BoundProgram, node: NodeId) -> bool {
    let Some(parent) = program.arena().parent(node) else {
        return false;
    };
    match program.arena().kind(parent) {
        Kind::SourceFile => !is_global_source_file(program, parent),
        Kind::ModuleBlock => {
            let Some(grand_parent) = program.arena().parent(parent) else {
                return false;
            };
            is_ambient_module(program, grand_parent)
                && program
                    .arena()
                    .parent(grand_parent)
                    .is_some_and(|sf| is_global_source_file(program, sf))
        }
        _ => false,
    }
}

// Go: internal/ast/utilities.go:IsGlobalSourceFile(2443)
fn is_global_source_file(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().data(node),
        NodeData::SourceFile(d) if d.external_module_indicator.is_none()
    )
}

// Go: internal/checker/checker.go:Checker.shouldCheckErasableSyntax(2632)
fn should_check_erasable_syntax(program: &dyn BoundProgram, node: NodeId) -> bool {
    use tsgo_core::tristate::Tristate;
    program.compiler_options().erasable_syntax_only == Tristate::True
        && !is_in_js_file(program.arena(), node)
}

// Go: internal/checker/checker.go:Checker.getIsolatedModulesLikeFlagName(5208)
fn get_isolated_modules_like_flag_name(program: &dyn BoundProgram) -> String {
    use tsgo_core::tristate::Tristate;
    if program.compiler_options().verbatim_module_syntax == Tristate::True {
        "verbatimModuleSyntax".to_string()
    } else {
        "isolatedModules".to_string()
    }
}

// Go: internal/checker/checker.go:getFirstNonAmbientClassOrFunctionDeclaration(5199)
fn get_first_non_ambient_class_or_function_declaration(
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> Option<NodeId> {
    for &decl in &program.symbol(symbol).declarations {
        let is_match = with_program_view_for_node(program, decl, |view| {
            if view.arena().flags(decl).contains(NodeFlags::AMBIENT) {
                return false;
            }
            match view.arena().kind(decl) {
                Kind::ClassDeclaration => true,
                Kind::FunctionDeclaration => matches!(
                    view.arena().data(decl),
                    NodeData::FunctionDeclaration(d) if d.body.is_some()
                ),
                _ => false,
            }
        });
        if is_match == Some(true) {
            return Some(decl);
        }
    }
    None
}

// Go: internal/ast/utilities.go:GetSourceFileOfNode
fn source_file_of(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    with_program_view_for_node(program, node, |view| {
        source_file_in_arena(view.arena(), node)
    })
    .flatten()
}

fn source_file_in_arena(arena: &tsgo_ast::NodeArena, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if arena.kind(n) == Kind::SourceFile {
            return Some(n);
        }
        cur = arena.parent(n);
    }
    None
}

/// Returns whether `node` is owned by `view`'s arena (reaches that file's root).
fn node_belongs_to_view(view: &dyn BoundProgram, node: NodeId) -> bool {
    let arena = view.arena();
    if node.index() >= arena.node_count() {
        return false;
    }
    let root = view.root();
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n == root {
            return true;
        }
        cur = arena.parent(n);
    }
    false
}

/// Returns the program file handle owning `node` (the diagnostics partition key).
fn source_file_handle_of_node(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    for file in program.source_files() {
        if let Some(view) = program.file_view(file) {
            if node_belongs_to_view(view.as_ref(), node) {
                return Some(file);
            }
        } else if file == program.file_handle() && node_belongs_to_view(program, node) {
            return Some(file);
        }
    }
    None
}

/// Invokes `f` with the [`BoundProgram`] view whose arena owns `node`.
fn with_program_view_for_node<R>(
    program: &dyn BoundProgram,
    node: NodeId,
    f: impl FnOnce(&dyn BoundProgram) -> R,
) -> Option<R> {
    for file in program.source_files() {
        if let Some(view) = program.file_view(file) {
            if node_belongs_to_view(view.as_ref(), node) {
                return Some(f(view.as_ref()));
            }
        } else if file == program.file_handle() && node_belongs_to_view(program, node) {
            return Some(f(program));
        }
    }
    None
}

// Reports whether an identifier node starts with `_` (used to suppress unused
// reports for intentionally-discarded bindings).
// Go: internal/checker/checker.go:isIdentifierThatStartsWithUnderscore(7235)
fn is_identifier_that_starts_with_underscore(program: &dyn BoundProgram, node: NodeId) -> bool {
    if program.arena().kind(node) != Kind::Identifier {
        return false;
    }
    let text = program.arena().text(node);
    !text.is_empty() && text.starts_with('_')
}

// Extracts the name node of a declaration, if it has one.
// Go: internal/ast/ast.go:Node.Name()
fn name_of_node(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::ParameterDeclaration(d) => Some(d.name),
        NodeData::FunctionDeclaration(d) => d.name,
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.name,
        NodeData::TypeAliasDeclaration(d) => Some(d.name),
        NodeData::EnumDeclaration(d) => Some(d.name),
        NodeData::ModuleDeclaration(d) => Some(d.name),
        NodeData::BindingElement(d) => d.name,
        _ => None,
    }
}

// Returns the node an argument-count error should be reported on (Go's
// `getErrorNodeForCallNode`): for a call expression, the callee, narrowed to
// the member name when the callee is a property access.
// Go: internal/checker/checker.go:getErrorNodeForCallNode(9806)
fn call_error_node(program: &dyn BoundProgram, node: NodeId) -> NodeId {
    let callee = match program.arena().data(node) {
        NodeData::CallExpression(d) => d.expression,
        NodeData::TaggedTemplateExpression(d) => d.tag,
        _ => return node,
    };
    match program.arena().data(callee) {
        NodeData::PropertyAccessExpression(d) => d.name,
        _ => callee,
    }
}

fn tagged_template_error_node(program: &dyn BoundProgram, _node: NodeId, tag: NodeId) -> NodeId {
    match program.arena().data(tag) {
        NodeData::PropertyAccessExpression(d) => d.name,
        _ => tag,
    }
}

// Reports whether `kind` is a function-like declaration (the reachable subset
// of Go's `ast.IsFunctionLikeKind`), used to find the `this` container.
fn is_function_like_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::MethodDeclaration
            | Kind::MethodSignature
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
    )
}

// Reports whether `node` is a class declaration or class expression (Go's
// `ast.IsClassLike`).
fn is_class_like(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::ClassDeclaration | Kind::ClassExpression
    )
}

// Reports whether `decl` is a class element whose name is a private identifier
// (Go's `ast.IsPrivateIdentifierClassElementDeclaration`).
// Reports whether `node` is a private identifier name in a property access
// (Go's `ast.IsPrivateIdentifier`). The parser's `parseRightSideOfDot` subset
// may represent `#x` as an `Identifier` with a `#` prefix rather than a
// `PrivateIdentifier` node; accept both shapes.
// Go: internal/ast/utilities.go:IsPrivateIdentifier
fn is_private_identifier_name_node(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::PrivateIdentifier => true,
        Kind::Identifier => arena.text(node).starts_with('#'),
        _ => false,
    }
}

fn is_private_identifier_class_element_declaration(
    program: &dyn BoundProgram,
    decl: NodeId,
) -> bool {
    let name = match program.arena().data(decl) {
        NodeData::PropertyDeclaration(d) => d.name,
        NodeData::MethodDeclaration(d) => d.name,
        NodeData::GetAccessorDeclaration(d) => d.name,
        NodeData::SetAccessorDeclaration(d) => d.name,
        _ => return false,
    };
    program.arena().kind(name) == Kind::PrivateIdentifier
}

// Reports whether `prop` may be copied by object spread (Go's
// `isSpreadableProperty`).
fn is_spreadable_property(program: &dyn BoundProgram, prop: SymbolId) -> bool {
    if super::is_synthesized_symbol(prop) {
        return true;
    }
    let sym = program.symbol(prop);
    let is_private_id = sym
        .declarations
        .iter()
        .copied()
        .any(|d| is_private_identifier_class_element_declaration(program, d));
    let is_method_or_accessor = sym.flags.intersects(
        SymbolFlags::METHOD | SymbolFlags::GET_ACCESSOR | SymbolFlags::SET_ACCESSOR,
    );
    let in_class = sym.declarations.iter().copied().any(|d| {
        program
            .arena()
            .parent(d)
            .is_some_and(|p| is_class_like(program.arena(), p))
    });
    (!is_private_id && !is_method_or_accessor) || !in_class
}

// Reports whether `prop` is a set-only accessor (Go's `getSpreadSymbol`
// `isSetonlyAccessor` check).
fn is_setonly_accessor_symbol(
    checker: &Checker,
    program: &dyn BoundProgram,
    prop: SymbolId,
) -> bool {
    let flags = if super::is_synthesized_symbol(prop) {
        checker.synthesized_symbol_flags(prop)
    } else {
        program.symbol(prop).flags
    };
    flags.contains(SymbolFlags::SET_ACCESSOR) && !flags.contains(SymbolFlags::GET_ACCESSOR)
}

// Reports whether `prop` is declared private or protected (Go's
// `getDeclarationModifierFlagsFromSymbol` check in `getSpreadType`).
fn is_private_or_protected_member(
    checker: &Checker,
    program: &dyn BoundProgram,
    prop: SymbolId,
) -> bool {
    declaration_modifier_flags_from_symbol(checker, program, prop, false)
        .intersects(ModifierFlags::PRIVATE | ModifierFlags::PROTECTED)
}

// Reports whether `node` was parsed in a JavaScript file (the parser sets
// `NodeFlags::JAVA_SCRIPT_FILE` on every node of a `.js`/`.jsx`/`.json` file).
// Go: internal/ast/utilities.go:IsInJSFile
pub(crate) fn is_in_js_file(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    arena
        .flags(node)
        .contains(tsgo_ast::NodeFlags::JAVA_SCRIPT_FILE)
}

/// Maps a phantom `ExportValue` local symbol to its real exported counterpart.
///
/// The binder gives a top-level exported value declaration two symbols: a local
/// in the module's `locals` flagged only `ExportValue`, and the real symbol in
/// `exports` (with the declaration's actual flags), linked from the local via
/// `export_symbol`. When `resolveName` finds the phantom local, the type and
/// declarations must be read from the export symbol. A non-phantom symbol (no
/// `ExportValue` flag, or no link) is returned unchanged. `getMergedSymbol` is
/// identity in the reachable single-/multi-file subset.
///
/// A re-export symbol that ALSO carries `Alias` (`export *` / re-export
/// specifier whose `export_symbol` link points straight at the re-exported
/// declaration) is left UNMAPPED here, so it flows through the alias-resolution
/// path in `get_type_of_symbol` instead. Go maps it and then types the target
/// on its static/constructor side; that static-side class value type is DEFERRED
/// in this port (only the instance type is built), so mapping an alias-bearing
/// re-export straight to a class would surface a premature TS2339 on a static
/// member. Routing it through `resolve_alias` (which itself defers `export *`)
/// preserves the pre-existing behavior until both land.
// DEFER(phase-4-checker): the static/constructor-side type of a class value
// symbol + `export *` re-export resolution. blocked-by: class static-member type
// construction + `getExportsOfModuleWorker` star visit.
// Go: internal/checker/checker.go:Checker.getExportSymbolOfValueSymbolIfExported
fn get_export_symbol_of_value_symbol_if_exported(
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> SymbolId {
    let s = program.symbol(symbol);
    if s.flags.contains(SymbolFlags::EXPORT_VALUE) && !s.flags.contains(SymbolFlags::ALIAS) {
        if let Some(export_symbol) = s.export_symbol {
            return export_symbol;
        }
    }
    symbol
}

// Reports whether `node` is a `require(...)` call: a call expression whose
// callee is the identifier `require` and that has exactly one argument. Mirrors
// Go's `ast.IsRequireCall(node, false /*requireStringLiteralLikeArgument*/)`,
// so the argument is not required to be a string literal.
// Go: internal/ast/utilities.go:IsRequireCall
fn is_require_call(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::CallExpression(d) => {
            arena.kind(d.expression) == Kind::Identifier
                && arena.text(d.expression) == "require"
                && d.arguments.nodes.len() == 1
        }
        _ => false,
    }
}

// Returns the source text of `node` (including quotes on string literals), or
// falls back to `NodeArena::text` when no file text is available.
fn node_source_text(program: &dyn BoundProgram, node: NodeId) -> String {
    if let Some(source) = program.source_text() {
        let loc = program.arena().loc(node);
        let start = loc.pos() as usize;
        let end = loc.end() as usize;
        if end <= source.len() {
            return source[start..end].to_string();
        }
    }
    let arena = program.arena();
    let text = arena.text(node);
    if arena.kind(node) == Kind::StringLiteral {
        format!("\"{text}\"")
    } else {
        text.to_string()
    }
}

// Returns the `lib` (e.g. `es2015`) whose feature map first introduced `name`,
// or `""` when `name` is not a known target-library global. This is the `{1}`
// argument of the TS2583 "...change the 'lib' compiler option to '{1}'..."
// diagnostic.
//
// Ports Go's `getSuggestedLibForNonExistentName`, which returns
// `getFeatureMap()[name][0].lib`. The Go feature map (`internal/checker/
// utilities.go:getFeatureMap`) carries the full per-name `(lib, props)` history;
// this consumer reads only the FIRST entry's lib, so the table is reduced to a
// `name -> first-lib` map (a complete, faithful port of THIS function's
// behavior for every input). The per-property `props` arrays — needed only by
// the OTHER consumer, `getSuggestedLibForNonExistentProperty` (TS2550) — are
// not modeled here.
//
// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.getSuggestedLibForNonExistentName
//     + internal/checker/utilities.go:getFeatureMap
fn get_suggested_lib_for_non_existent_name(name: &str) -> &'static str {
    match name {
        "Array" | "Iterator" | "AsyncIterator" | "RegExp" | "Reflect" | "ArrayConstructor"
        | "ObjectConstructor" | "NumberConstructor" | "Math" | "Map" | "Set"
        | "PromiseConstructor" | "Symbol" | "WeakMap" | "WeakSet" | "String"
        | "StringConstructor" | "Promise" => "es2015",
        "Atomics" | "SharedArrayBuffer" | "DateTimeFormat" => "es2017",
        "AsyncIterable"
        | "AsyncIterableIterator"
        | "AsyncGenerator"
        | "AsyncGeneratorFunction"
        | "RegExpMatchArray"
        | "RegExpExecArray"
        | "Intl"
        | "NumberFormat" => "es2018",
        "SymbolConstructor" | "DataView" | "BigInt" | "RelativeTimeFormat" | "BigInt64Array"
        | "BigUint64Array" => "es2020",
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array" | "Error" => "es2022",
        "MapConstructor" | "ArrayBuffer" => "es2024",
        "RegExpConstructor" | "Float16Array" => "es2025",
        "ErrorConstructor"
        | "Uint8ArrayConstructor"
        | "DisposableStack"
        | "AsyncDisposableStack"
        | "Date" => "esnext",
        _ => "",
    }
}

// Returns the nearest enclosing function-like ancestor of `node` (the
// reachable subset of Go's `ast.GetContainingFunction`). For a parameter node,
// the parent is the function-like declaration.
// Go: internal/ast/utilities.go:GetContainingFunction
fn get_containing_function(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let arena = program.arena();
    let mut current = arena.parent(node);
    while let Some(n) = current {
        if is_function_like_kind(arena.kind(n)) {
            return Some(n);
        }
        current = arena.parent(n);
    }
    None
}

// Returns the parameter nodes of any function-like declaration (broader than
// `function_like_parameters` which only covers arrow/function expressions).
// Go: internal/ast: FunctionLikeData parameters
fn function_like_all_parameters(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::ArrowFunction(d) => d.parameters.nodes.clone(),
        NodeData::FunctionExpression(d) | NodeData::FunctionDeclaration(d) => {
            d.parameters.nodes.clone()
        }
        NodeData::MethodDeclaration(d) => d.parameters.nodes.clone(),
        NodeData::ConstructorDeclaration(d) => d.parameters.nodes.clone(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.parameters.nodes.clone()
        }
        NodeData::IndexSignatureDeclaration(d) => d.parameters.nodes.clone(),
        _ => Vec::new(),
    }
}

// Returns the modifier flags of `node` (its `modifiers` list union), or empty
// when the node bears no modifier list.
fn modifier_flags_of(arena: &tsgo_ast::NodeArena, node: NodeId) -> tsgo_ast::ModifierFlags {
    let modifiers = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => d.modifiers.as_ref(),
        NodeData::MethodSignature(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        NodeData::ConstructorDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ConstructorType(d) => d.modifiers.as_ref(),
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref(),
        NodeData::IndexSignatureDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers
        .map(|m| m.modifier_flags)
        .unwrap_or(tsgo_ast::ModifierFlags::empty())
}

// Reports whether `node` carries the `static` modifier (Go's `ast.IsStatic`,
// class-element subset).
fn has_static_modifier(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    modifier_flags_of(arena, node).contains(tsgo_ast::ModifierFlags::STATIC)
}

// Reports whether `node` carries the `abstract` modifier.
fn has_abstract_modifier(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    modifier_flags_of(arena, node).contains(tsgo_ast::ModifierFlags::ABSTRACT)
}

// Reports whether `expr` assigns to a readonly entity (`symbol` or a namespace
// import receiver). Constructor-local `this.x` writes to readonly properties of
// the enclosing class are allowed.
// Go: internal/checker/checker.go:Checker.isAssignmentToReadonlyEntity
fn is_assignment_to_readonly_entity(
    checker: &Checker,
    program: &dyn BoundProgram,
    expr: NodeId,
    symbol: SymbolId,
    assignment_kind: tsgo_ast::utilities::AssignmentKind,
) -> bool {
    if assignment_kind == tsgo_ast::utilities::AssignmentKind::None {
        return false;
    }
    if is_access_expression(program.arena().kind(expr)) {
        let object = skip_parentheses(program, {
            match program.arena().data(expr) {
                NodeData::PropertyAccessExpression(d) => d.expression,
                NodeData::ElementAccessExpression(d) => d.expression,
                _ => return is_readonly_symbol(checker, program, symbol),
            }
        });
        if program.arena().kind(object) == Kind::Identifier {
            if let Some(expression_symbol) = resolve_name(
                program,
                object,
                program.arena().text(object),
                SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
                true,
                program.globals(),
            ) {
                if checker
                    .resolved_symbol_flags(program, expression_symbol)
                    .intersects(SymbolFlags::MODULE_EXPORTS)
                {
                    return false;
                }
            }
        }
    }
    if is_readonly_symbol(checker, program, symbol) {
        if checker
            .resolved_symbol_flags(program, symbol)
            .intersects(SymbolFlags::PROPERTY)
            && is_access_expression(program.arena().kind(expr))
        {
            let object = skip_parentheses(program, {
                match program.arena().data(expr) {
                    NodeData::PropertyAccessExpression(d) => d.expression,
                    NodeData::ElementAccessExpression(d) => d.expression,
                    _ => return true,
                }
            });
            if program.arena().kind(object) == Kind::ThisKeyword {
                let Some(ctor) = get_control_flow_container(program, expr) else {
                    return true;
                };
                if program.arena().kind(ctor) != Kind::Constructor {
                    return true;
                }
                let sym = program.symbol(symbol);
                if let Some(value_decl) = sym.value_declaration {
                    let arena = program.arena();
                    let is_assignment_declaration =
                        arena.kind(value_decl) == Kind::BinaryExpression;
                    let is_local_property_declaration =
                        arena.parent(ctor) == arena.parent(value_decl);
                    let is_local_parameter_property = Some(ctor) == arena.parent(value_decl);
                    let parent_value_decl = sym
                        .parent
                        .and_then(|p| program.symbol(p).value_declaration);
                    let is_local_this_property_assignment = is_assignment_declaration
                        && parent_value_decl
                            .is_some_and(|vd| arena.parent(ctor) == arena.parent(vd));
                    let is_local_this_property_assignment_constructor_function =
                        is_assignment_declaration
                            && parent_value_decl
                                .is_some_and(|vd| Some(ctor) == arena.parent(vd));
                    let is_writable_symbol = is_local_property_declaration
                        || is_local_parameter_property
                        || is_local_this_property_assignment
                        || is_local_this_property_assignment_constructor_function;
                    return !is_writable_symbol;
                }
            }
        }
        return true;
    }
    if is_access_expression(program.arena().kind(expr)) {
        let object = skip_parentheses(program, {
            match program.arena().data(expr) {
                NodeData::PropertyAccessExpression(d) => d.expression,
                NodeData::ElementAccessExpression(d) => d.expression,
                _ => return false,
            }
        });
        if program.arena().kind(object) == Kind::Identifier {
            if let Some(expression_symbol) = resolve_name(
                program,
                object,
                program.arena().text(object),
                SymbolFlags::ALIAS,
                true,
                program.globals(),
            ) {
                return get_declaration_of_alias_symbol(program, expression_symbol)
                    .is_some_and(|decl| program.arena().kind(decl) == Kind::NamespaceImport);
            }
        }
    }
    false
}

// Go: internal/checker/checker.go:Checker.isReadonlySymbol
fn is_readonly_symbol(
    checker: &Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> bool {
    if super::is_synthesized_symbol(symbol) {
        return checker
            .synthesized_symbol_check_flags(symbol)
            .contains(CheckFlags::READONLY);
    }
    let flags = checker.resolved_symbol_flags(program, symbol);
    if flags.intersects(SymbolFlags::PROPERTY)
        && is_readonly_property_symbol(checker, program, symbol)
    {
        return true;
    }
    if flags.intersects(SymbolFlags::VARIABLE) {
        let sym = program.symbol(symbol);
        if sym.value_declaration.is_some_and(|decl| {
            combined_node_flags(program, decl).intersects(NodeFlags::CONSTANT)
        }) {
            return true;
        }
    }
    if flags.intersects(SymbolFlags::ACCESSOR) && !flags.intersects(SymbolFlags::SET_ACCESSOR) {
        return true;
    }
    flags.intersects(SymbolFlags::ENUM_MEMBER)
}

// Go: internal/checker/checker.go:Checker.isReadonlySymbol (property arm)
fn is_readonly_property_symbol(
    checker: &Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> bool {
    if super::is_synthesized_symbol(symbol) {
        return checker
            .synthesized_symbol_check_flags(symbol)
            .contains(CheckFlags::READONLY);
    }
    let sym = program.symbol(symbol);
    sym.value_declaration.is_some_and(|decl| {
        combined_modifier_flags(program, decl).contains(ModifierFlags::READONLY)
    })
}

// Go: internal/checker/checker.go:Checker.getTargetSymbol(21519)
fn get_target_symbol(
    checker: &Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> SymbolId {
    if checker
        .resolved_symbol_check_flags(program, symbol)
        .contains(CheckFlags::INSTANTIATED)
    {
        checker
            .value_symbol_links
            .try_get(&symbol)
            .and_then(|l| l.target)
            .unwrap_or(symbol)
    } else {
        symbol
    }
}

// Go: internal/checker/checker.go:isPrototypeProperty(21533)
fn is_prototype_property(
    checker: &Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> bool {
    program.symbol(symbol).flags.intersects(SymbolFlags::METHOD)
        || checker
            .resolved_symbol_check_flags(program, symbol)
            .contains(CheckFlags::SYNTHETIC_METHOD)
}

// Go: internal/checker/checker.go:Checker.isMixinConstructorType(16885)
fn is_mixin_constructor_class(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    class_node: NodeId,
) -> bool {
    let Some(constructor) = find_constructor_declaration(program, class_node) else {
        return false;
    };
    let NodeData::ConstructorDeclaration(d) = program.arena().data(constructor) else {
        return false;
    };
    if d.parameters.nodes.len() != 1 {
        return false;
    }
    let param = d.parameters.nodes[0];
    let NodeData::ParameterDeclaration(pd) = program.arena().data(param) else {
        return false;
    };
    if pd.dot_dot_dot_token.is_none() {
        return false;
    }
    let Some(param_sym) = program.symbol_of_node(param) else {
        return false;
    };
    let param_type = get_type_of_symbol(checker, program, param_sym, program.globals());
    if checker.get_type(param_type).flags().intersects(TypeFlags::ANY) {
        return true;
    }
    checker
        .get_element_type_of_array_type(param_type)
        .is_some_and(|elem| checker.get_type(elem).flags().intersects(TypeFlags::ANY))
}

fn override_member_error_node(program: &dyn BoundProgram, symbol: SymbolId) -> NodeId {
    let sym = program.symbol(symbol);
    let Some(decl) = sym.value_declaration else {
        return program.root();
    };
    super::symbols_query::name_of_declaration(program.arena(), decl).unwrap_or(decl)
}

// Returns the nearest control-flow container of `node` (function-like, module
// block, source file, or property declaration). Used for constructor-local
// readonly-write exceptions.
// Go: internal/checker/checker.go:Checker.getControlFlowContainer
fn get_control_flow_container(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let arena = program.arena();
    let mut current = arena.parent(node);
    while let Some(n) = current {
        let kind = arena.kind(n);
        if kind == Kind::PropertyDeclaration {
            return Some(n);
        }
        if matches!(kind, Kind::SourceFile | Kind::ModuleBlock) {
            return Some(n);
        }
        if is_function_like_kind(kind) {
            return Some(n);
        }
        current = arena.parent(n);
    }
    None
}

// Returns the nearest enclosing (non-arrow) function-like container of `node`
// (the reachable subset of Go's `ast.GetThisContainer`): walks the parent
// chain, skipping arrow functions so a lexical `this` resolves to its real
// owner. Returns `None` when no function-like ancestor exists.
//
// DEFER(phase-4-checker-C-D2+): computed-property-name / decorator / module
// containers and the `includeClassComputedPropertyName` handling. blocked-by:
// the full `getThisContainer` walk.
// Go: internal/ast/utilities.go:GetThisContainer
fn get_this_container(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let arena = program.arena();
    let mut current = arena.parent(node);
    while let Some(n) = current {
        let kind = arena.kind(n);
        if kind == Kind::ArrowFunction {
            current = arena.parent(n);
            continue;
        }
        if is_function_like_kind(kind) {
            return Some(n);
        }
        current = arena.parent(n);
    }
    None
}

// Reports whether a call argument is context-sensitive for inference purposes
// (the reachable subset of Go's `isContextSensitive`): a function or arrow
// expression argument, whose parameter type variables are fixed by the other
// arguments before it is contextually typed.
//
// DEFER(phase-4-checker-C-B3): the precise test — a fully type-annotated
// function is NOT context-sensitive, and object/array literals containing
// context-sensitive elements are. blocked-by: per-parameter annotation analysis
// + literal element recursion.
// Go: internal/checker/checker.go:Checker.isContextSensitive
fn is_context_sensitive_argument(program: &dyn BoundProgram, arg: NodeId) -> bool {
    matches!(
        program.arena().kind(arg),
        Kind::ArrowFunction | Kind::FunctionExpression
    )
}

// Returns the parameter nodes of a function/arrow expression (the reachable
// subset used by C-B3's callback-return inference).
// Go: internal/ast: FunctionLikeData parameters
fn function_like_parameters(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::ArrowFunction(d) => d.parameters.nodes.clone(),
        NodeData::FunctionExpression(d) | NodeData::FunctionDeclaration(d) => {
            d.parameters.nodes.clone()
        }
        _ => Vec::new(),
    }
}

// Returns the body node of a function-like declaration, if any.
//
// Covers arrow/function expressions (the contextual-inference path) plus the
// function/method/accessor declaration kinds (declaration-emit return-type
// inference, `create_return_type_of_signature_declaration`).
// Go: internal/ast: FunctionLikeData body
fn function_like_body(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::ArrowFunction(d) => Some(d.body),
        NodeData::FunctionExpression(d) => d.body,
        NodeData::FunctionDeclaration(d) => d.body,
        NodeData::MethodDeclaration(d) => d.body,
        NodeData::GetAccessorDeclaration(d) => d.body,
        _ => None,
    }
}

// Returns the (return-type) annotation node of an arrow/function expression.
// Go: internal/ast: FunctionLikeData type
fn arrow_return_type_node(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    get_effective_return_type_node(program, node)
}

/// Returns the explicit return-type annotation node of a function-like
/// declaration, if any (Go's `node.Type()` / `getReturnTypeFromAnnotation` input).
///
/// # Examples
///
/// `function f(): number` → `Some(type_node_for_number)`.
///
/// # Side effects
///
/// None (pure).
// Go: internal/checker/checker.go:Checker.getReturnTypeFromAnnotation (declaration.Type())
pub(crate) fn get_effective_return_type_node(
    program: &dyn BoundProgram,
    node: NodeId,
) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::FunctionDeclaration(d) => d.type_node,
        NodeData::MethodDeclaration(d) => d.type_node,
        NodeData::FunctionExpression(d) => d.type_node,
        NodeData::ArrowFunction(d) => d.type_node,
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.type_node,
        NodeData::ConstructorDeclaration(d) => d.type_node,
        _ => None,
    }
}

/// Reports whether a declaration is effectively ambient for overload-flag agreement
/// (Go's `getEffectiveDeclarationFlags` ambient bit).
fn is_effective_ambient_declaration(program: &dyn BoundProgram, node: NodeId) -> bool {
    if program
        .arena()
        .flags(node)
        .contains(tsgo_ast::NodeFlags::AMBIENT)
    {
        return true;
    }
    combined_modifier_flags(program, node).contains(ModifierFlags::AMBIENT)
}

// Reports whether a parameter declaration is optional for arity (a `?`,
// initializer, or rest `...`), mirroring the contextual/declared-types helper.
fn is_optional_parameter(program: &dyn BoundProgram, param: NodeId) -> bool {
    match program.arena().data(param) {
        NodeData::ParameterDeclaration(d) => {
            d.question_token.is_some() || d.initializer.is_some() || d.dot_dot_dot_token.is_some()
        }
        _ => false,
    }
}

// Collects the `return <expr>` expressions reachable in a function body block,
// without descending into nested function-like declarations (whose returns
// belong to that inner function). The reachable subset: top-level statements and
// the immediate bodies of control-flow containers.
// Go: internal/checker/checker.go:Checker.checkAndAggregateReturnExpressionTypes (subset)
fn collect_return_expressions(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_return_expressions_into(program, node, &mut out);
    out
}

fn collect_return_expressions_into(
    program: &dyn BoundProgram,
    node: NodeId,
    out: &mut Vec<NodeId>,
) {
    match program.arena().data(node) {
        NodeData::ReturnStatement(d) => {
            if let Some(expr) = d.expression {
                out.push(expr);
            }
        }
        NodeData::Block(d) => {
            for &stmt in &d.list.nodes {
                collect_return_expressions_into(program, stmt, out);
            }
        }
        NodeData::IfStatement(d) => {
            collect_return_expressions_into(program, d.then_statement, out);
            if let Some(else_statement) = d.else_statement {
                collect_return_expressions_into(program, else_statement, out);
            }
        }
        // DEFER(phase-4-checker-C-C): loops/try/switch and nested-block return
        // aggregation. blocked-by: full control-flow return analysis.
        _ => {}
    }
}

// Reports whether `node` can be an assignment target reference (Go's
// `checkReferenceExpression`, 4n subset): an identifier or an access
// expression. The full version also skips assertions/parentheses and rejects
// optional chains with dedicated diagnostics.
//
// DEFER(phase-4-checker-4n+): skipping outer expressions + the
// invalid-reference/optional-chain diagnostics. blocked-by: those diagnostics
// + `SkipOuterExpressions`.
// Go: internal/checker/checker.go:Checker.checkReferenceExpression(13062)
fn is_reference_expression(program: &dyn BoundProgram, node: NodeId) -> bool {
    matches!(
        program.arena().kind(node),
        Kind::Identifier | Kind::PropertyAccessExpression | Kind::ElementAccessExpression
    )
}

// Reports whether `node` is an entity-name expression (Go's
// `IsEntityNameExpression`, `allowJS=false` reachable subset): an identifier, or
// a property access `<entity>.name` whose name is an identifier and whose object
// is itself an entity name. Drives the `'{0}' is possibly ...` vs the
// `Object is possibly ...` diagnostic choice.
// Go: internal/ast/utilities.go:IsEntityNameExpression(1580)/IsPropertyAccessEntityNameExpression
fn is_entity_name_expression(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::Identifier => true,
        Kind::PropertyAccessExpression => match arena.data(node) {
            NodeData::PropertyAccessExpression(d) => {
                arena.kind(d.name) == Kind::Identifier
                    && is_entity_name_expression(arena, d.expression)
            }
            _ => false,
        },
        _ => false,
    }
    // DEFER(phase-4-checker-4az+): the `allowJS` forms (`this`, element-access
    // entity names). blocked-by: JS-file entity-name parity.
}

// Reports whether `node` is a valid `as const` / `<const>` operand (Go's
// `isValidConstAssertionArgument`).
// Go: internal/checker/checker.go:Checker.isValidConstAssertionArgument(13537)
fn is_valid_const_assertion_argument(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    node: NodeId,
) -> bool {
    let arena = program.arena();
    match arena.kind(node) {
        Kind::StringLiteral
        | Kind::NoSubstitutionTemplateLiteral
        | Kind::NumericLiteral
        | Kind::BigIntLiteral
        | Kind::TrueKeyword
        | Kind::FalseKeyword
        | Kind::ArrayLiteralExpression
        | Kind::ObjectLiteralExpression
        | Kind::TemplateExpression => true,
        Kind::ParenthesizedExpression => {
            let inner = match arena.data(node) {
                NodeData::ParenthesizedExpression(d) => d.expression,
                _ => return false,
            };
            is_valid_const_assertion_argument(checker, program, inner)
        }
        Kind::PrefixUnaryExpression => {
            let (operator, operand) = match arena.data(node) {
                NodeData::PrefixUnaryExpression(d) => (d.operator, d.operand),
                _ => return false,
            };
            if operator == Kind::MinusToken {
                matches!(
                    arena.kind(operand),
                    Kind::NumericLiteral | Kind::BigIntLiteral
                )
            } else if operator == Kind::PlusToken {
                arena.kind(operand) == Kind::NumericLiteral
            } else {
                false
            }
        }
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression => {
            let expr = match arena.data(node) {
                NodeData::PropertyAccessExpression(d) => d.expression,
                NodeData::ElementAccessExpression(d) => d.expression,
                _ => return false,
            };
            let expr = skip_outer_expressions(program, expr);
            if !is_entity_name_expression(arena, expr) {
                return false;
            }
            let symbol = checker.resolve_entity_name(
                program,
                expr,
                SymbolFlags::VALUE,
                true,
                false,
                None,
            );
            symbol.is_some_and(|sym| {
                checker
                    .resolved_symbol_flags(program, sym)
                    .intersects(SymbolFlags::ENUM)
            })
        }
        _ => false,
    }
}

// Reports whether `node` is a `const` assertion (`expr as const` or
// `<const>expr`): an `AsExpression`/`TypeAssertionExpression` whose type node is
// the `const` type reference (Go's `ast.IsConstAssertion`).
// Go: internal/ast/utilities.go:IsConstAssertion(2431)
fn is_const_assertion(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::AsExpression(d) => is_const_type_reference(arena, d.type_node),
        NodeData::TypeAssertionExpression(d) => is_const_type_reference(arena, d.type_node),
        _ => false,
    }
}

// Reports whether `node` occurs in a const context (Go's `isConstContext`): the
// operand of an `as const` / `<const>` assertion, or — recursively — an
// element/property nested within one. The reachable subset implements the
// syntactic propagation: a parenthesized expression, array-literal element, or
// spread element inherits its parent's const context; a property assignment,
// shorthand property, or template span inherits its grandparent's (the
// containing object literal / template). This is what makes
// `{ a: [1] } as const` mark both the inner array element and the outer object
// property as const.
//
// DEFER(phase-4-checker-4bi+): the contextual-type branch
// (`isValidConstAssertionArgument(node) && isConstTypeVariable(getContextualType(node))`),
// which marks a literal const when it is contextually typed by a `const` type
// parameter. blocked-by: contextual type propagation + `isConstTypeVariable`
// (const type parameters).
// Go: internal/checker/checker.go:Checker.isConstContext(13529)
fn is_const_context(program: &dyn BoundProgram, node: NodeId) -> bool {
    let arena = program.arena();
    let Some(parent) = arena.parent(node) else {
        return false;
    };
    if is_const_assertion(arena, parent) {
        return true;
    }
    match arena.kind(parent) {
        Kind::ParenthesizedExpression | Kind::ArrayLiteralExpression | Kind::SpreadElement => {
            is_const_context(program, parent)
        }
        Kind::PropertyAssignment | Kind::ShorthandPropertyAssignment | Kind::TemplateSpan => {
            match arena.parent(parent) {
                Some(grandparent) => is_const_context(program, grandparent),
                None => false,
            }
        }
        _ => false,
    }
}

// Reports whether `type_node` is the `const` type reference of an `as const`
// assertion (Go's `ast.IsConstTypeReference` / `isConstTypeReference`): a type
// reference with no type arguments whose name is the identifier `const`.
// Go: internal/ast/utilities.go:IsConstTypeReference(2439) / internal/checker/utilities.go:isConstTypeReference(128)
fn is_const_type_reference(arena: &tsgo_ast::NodeArena, type_node: NodeId) -> bool {
    match arena.data(type_node) {
        NodeData::TypeReference(d) => {
            d.type_arguments
                .as_ref()
                .is_none_or(|list| list.nodes.is_empty())
                && arena.kind(d.type_name) == Kind::Identifier
                && arena.text(d.type_name) == "const"
        }
        _ => false,
    }
}

// Returns the property name text of a non-computed object-literal member name
// node (an identifier, string literal, or numeric literal). A computed property
// name (`[expr]: v`) yields `None` (handled separately as an index signature or
// late-bound member). Mirrors reading `member.Name` off the binder's property
// symbol, where a numeric name is its decimal text.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral (member.Name)
fn property_name_text(program: &dyn BoundProgram, name_node: NodeId) -> Option<String> {
    match program.arena().kind(name_node) {
        Kind::Identifier | Kind::StringLiteral | Kind::NumericLiteral => {
            Some(program.arena().text(name_node).to_string())
        }
        _ => None,
    }
}

// Resolves a property name node to its effective name text (Go's
// `getEffectivePropertyNameForPropertyNameNode`).
//
// # Side effects
//
// May type-check computed name expressions.
// Go: internal/ast/utilities.go:GetPropertyNameForPropertyNameNode(3126) +
//     internal/checker/checker.go:getEffectivePropertyNameForPropertyNameNode(18566)
fn effective_property_name_for_property_name_node(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    name_node: NodeId,
) -> Option<String> {
    if let Some(name) = property_name_for_property_name_node(program, name_node) {
        return Some(name);
    }
    let NodeData::ComputedPropertyName(d) = program.arena().data(name_node) else {
        return None;
    };
    let expr_type = checker.check_expression(program, d.expression);
    try_get_name_from_type(checker, expr_type)
}

// Go: internal/ast/utilities.go:GetPropertyNameForPropertyNameNode(3126)
fn property_name_for_property_name_node(
    program: &dyn BoundProgram,
    name_node: NodeId,
) -> Option<String> {
    if let Some(name) = property_name_text(program, name_node) {
        return Some(name);
    }
    let NodeData::ComputedPropertyName(d) = program.arena().data(name_node) else {
        return None;
    };
    let expr = d.expression;
    // Go only reads literal text from computed names; identifier expressions are
    // resolved later via `getTypeOfExpression` / `tryGetNameFromType`.
    match program.arena().kind(expr) {
        Kind::StringLiteral | Kind::NumericLiteral | Kind::NoSubstitutionTemplateLiteral
        | Kind::BigIntLiteral => property_name_text(program, expr),
        Kind::PrefixUnaryExpression => {
            let NodeData::PrefixUnaryExpression(ud) = program.arena().data(expr) else {
                return None;
            };
            if ud.operator == Kind::MinusToken {
                property_name_text(program, ud.operand).map(|operand| format!("-{operand}"))
            } else {
                None
            }
        }
        _ => None,
    }
}

// Go: internal/checker/checker.go:tryGetNameFromType(18577)
fn try_get_name_from_type(checker: &Checker, t: TypeId) -> Option<String> {
    let ty = checker.get_type(t);
    let flags = ty.flags();
    if let Some(val) = ty.literal_value() {
        match val {
            super::types::LiteralValue::String(s)
                if flags.intersects(TypeFlags::STRING_LITERAL) =>
            {
                Some(s.clone())
            }
            super::types::LiteralValue::Number(n)
                if flags.intersects(TypeFlags::NUMBER_LITERAL) =>
            {
                Some(n.to_string())
            }
            _ => None,
        }
    } else if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
        ty.unique_es_symbol_name().map(|s| s.to_string())
    } else {
        None
    }
}

// Reports whether `name` is a numeric literal name (Go's `isNumericLiteralName`):
// the name is a numeric name iff `ToString(ToNumber(name)) == name`, i.e. the
// JS-number round-trip of its text is exactly the text (so `"0"`/`"1.5"` are
// numeric but `"0xF00D"`/`"01"` are not).
// Go: internal/checker/utilities.go:isNumericLiteralName(860)
pub(crate) fn is_numeric_literal_name(name: &str) -> bool {
    tsgo_jsnum::from_string(name).to_string() == name
}

// Locates the name node of the object-literal property assignment named `name`
// within `literal_node`, so an excess-property error reports on the property
// itself (Go narrows `r.errorNode` to `prop.ValueDeclaration.Name()`).
//
// DEFER(phase-4-checker-4bg+): shorthand/spread/accessor/method members and
// computed names; only `name: value` assignments are matched.
// Go: internal/checker/relater.go:Relater.hasExcessProperties (errorNode = name)
// Returns the name node of a member declaration so a related-info diagnostic
// points at the property's name (Go's `GetErrorRangeForNode` narrows a
// `PropertySignature`/`PropertyDeclaration` error span to its name via
// `GetNameOfDeclaration`). The reachable subset covers the type-literal /
// interface / class property kinds an elaboration target resolves.
//
// DEFER(phase-4-checker-4bp+): the remaining declaration kinds. blocked-by:
// those declarations appearing as elaboration targets.
// Go: internal/ast/utilities.go:GetNameOfDeclaration / scanner.GetErrorRangeForNode
fn declaration_name_node(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::PropertySignature(d) | NodeData::PropertyDeclaration(d) => Some(d.name),
        _ => None,
    }
}

// Returns the member declaration nodes of an object-type declaration (interface
// or class) for the duplicate-declaration check (Go's `node.Members()`).
// Go: internal/ast/utilities.go:Node.Members
fn object_type_member_nodes(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::InterfaceDeclaration(d)
        | NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d) => d.members.nodes.clone(),
        _ => Vec::new(),
    }
}

// Display name for a class-like declaration (Go's `symbolToString` on the class
// symbol from `getSymbolOfDeclaration(node)`).
fn class_like_display_name(program: &dyn BoundProgram, node: NodeId) -> String {
    if let Some(sym) = program.symbol_of_node(node) {
        program.symbol(sym).name.clone()
    } else {
        match program.arena().data(node) {
            NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d
                .name
                .map(|n| program.arena().text(n).to_string())
                .unwrap_or_else(|| "<anonymous>".to_string()),
            _ => "<anonymous>".to_string(),
        }
    }
}

// Classifies a member for `checkObjectTypeForDuplicateDeclarations`: `1` for a
// property/property-signature (not an auto-accessor), `2` for a get/set accessor
// or an `accessor`-modified property; `None` for any other member kind (methods,
// index signatures, ... are not subject to this check).
// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations
//     (the property/accessor classification, 3170-3173)
fn classify_property_or_accessor(program: &dyn BoundProgram, member: NodeId) -> Option<i32> {
    match program.arena().kind(member) {
        Kind::PropertySignature => Some(1),
        Kind::PropertyDeclaration => {
            if has_accessor_modifier(program.arena(), member) {
                Some(2)
            } else {
                Some(1)
            }
        }
        Kind::GetAccessor | Kind::SetAccessor => Some(2),
        _ => None,
    }
}

fn is_class_accessor_member(program: &dyn BoundProgram, member: NodeId) -> bool {
    matches!(
        program.arena().kind(member),
        Kind::GetAccessor | Kind::SetAccessor
    ) || is_auto_accessor_member(program, member)
}

fn is_auto_accessor_member(program: &dyn BoundProgram, member: NodeId) -> bool {
    program.arena().kind(member) == Kind::PropertyDeclaration
        && has_accessor_modifier(program.arena(), member)
}

// Reports whether `node` carries the `accessor` modifier (an auto-accessor
// property, Go's `ast.HasAccessorModifier`).
fn has_accessor_modifier(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    modifier_flags_of(arena, node).contains(tsgo_ast::ModifierFlags::ACCESSOR)
}

// Returns the NAME node of a class/interface member declaration (Go's
// `member.Name()`), for the property/accessor/method member kinds the
// duplicate-declaration check reports on.
fn member_name_node_for_duplicate(program: &dyn BoundProgram, member: NodeId) -> Option<NodeId> {
    match program.arena().data(member) {
        NodeData::PropertySignature(d) | NodeData::PropertyDeclaration(d) => Some(d.name),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => Some(d.name),
        NodeData::MethodSignature(d) => Some(d.name),
        NodeData::MethodDeclaration(d) => Some(d.name),
        _ => None,
    }
}

fn object_literal_property_name_node(
    program: &dyn BoundProgram,
    literal_node: NodeId,
    name: &str,
) -> Option<NodeId> {
    let members = match program.arena().data(literal_node) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
        _ => return None,
    };
    for member in members {
        let name_node = match program.arena().data(member) {
            NodeData::PropertyAssignment(d) => d.name,
            _ => continue,
        };
        if property_name_text(program, name_node).as_deref() == Some(name) {
            return Some(name_node);
        }
    }
    None
}

// Renders an entity-name expression to its source text (Go's
// `entityNameToString` reachable subset): an identifier yields its text, a
// property access yields `<object>.<name>`.
// Go: internal/checker/utilities.go:entityNameToString(195) / ast.EntityNameToString
fn entity_name_to_string(arena: &tsgo_ast::NodeArena, node: NodeId) -> String {
    match arena.data(node) {
        NodeData::PropertyAccessExpression(d) => {
            format!(
                "{}.{}",
                entity_name_to_string(arena, d.expression),
                arena.text(d.name)
            )
        }
        _ => arena.text(node).to_string(),
    }
}

// Reports whether `operator` is a compound assignment operator (`+=`/`*=`/`&&=`/
// ...), i.e. an assignment operator other than plain `=` (Go's
// `KindFirstCompoundAssignment ..= KindLastCompoundAssignment`).
// Go: internal/ast/ast.go:IsCompoundAssignment
// Returns true for indirect calls like `(0, x.f)(...)` or `(0, eval)(...)`,
// where the comma operator's left operand is intentionally discarded.
//
// Go: internal/checker/checker.go:Checker.isIndirectCall(12971)
fn is_indirect_call(program: &dyn BoundProgram, comma_node: NodeId) -> bool {
    let parent = match program.arena().parent(comma_node) {
        Some(p) => p,
        None => return false,
    };
    if program.arena().kind(parent) != Kind::ParenthesizedExpression {
        return false;
    }
    let (left, right) = match program.arena().data(comma_node) {
        NodeData::BinaryExpression(d) => (d.left, d.right),
        _ => return false,
    };
    if program.arena().kind(left) != Kind::NumericLiteral
        || program.arena().text(left) != "0"
    {
        return false;
    }
    let grandparent = match program.arena().parent(parent) {
        Some(gp) => gp,
        None => return false,
    };
    let is_call_or_tagged = match program.arena().data(grandparent) {
        NodeData::CallExpression(d) => d.expression == parent,
        NodeData::TaggedTemplateExpression(d) => d.tag == parent,
        _ => false,
    };
    if !is_call_or_tagged {
        return false;
    }
    let right_kind = program.arena().kind(right);
    is_access_expression(right_kind)
        || (right_kind == Kind::Identifier && program.arena().text(right) == "eval")
}

fn shift_amount_display(amount: tsgo_jsnum::Number) -> String {
    let n = f64::from(amount);
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn is_compound_assignment(operator: Kind) -> bool {
    matches!(
        operator,
        Kind::PlusEqualsToken
            | Kind::MinusEqualsToken
            | Kind::AsteriskEqualsToken
            | Kind::AsteriskAsteriskEqualsToken
            | Kind::SlashEqualsToken
            | Kind::PercentEqualsToken
            | Kind::LessThanLessThanEqualsToken
            | Kind::GreaterThanGreaterThanEqualsToken
            | Kind::GreaterThanGreaterThanGreaterThanEqualsToken
            | Kind::AmpersandEqualsToken
            | Kind::BarEqualsToken
            | Kind::CaretEqualsToken
            | Kind::AmpersandAmpersandEqualsToken
            | Kind::BarBarEqualsToken
            | Kind::QuestionQuestionEqualsToken
    )
}

// Maps a bitwise operator applied to boolean operands to the logical operator
// Go suggests in diagnostic `2447`.
// Go: internal/checker/checker.go:Checker.getSuggestedBooleanOperator(12731)
fn get_suggested_boolean_operator(operator: Kind) -> Option<Kind> {
    match operator {
        Kind::BarToken | Kind::BarEqualsToken => Some(Kind::BarBarToken),
        Kind::CaretToken | Kind::CaretEqualsToken => Some(Kind::ExclamationEqualsEqualsToken),
        Kind::AmpersandToken | Kind::AmpersandEqualsToken => Some(Kind::AmpersandAmpersandToken),
        _ => None,
    }
}

/// Returns the enclosing function-like container where `new.target` is valid
/// (constructor, function declaration, or function expression), or `None` if
/// `node` is at the top level.
// Go: internal/ast/utilities.go:GetNewTargetContainer(2126)
fn get_new_target_container(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let arena = program.arena();
    let mut current = arena.parent(node)?;
    loop {
        match arena.kind(current) {
            Kind::Constructor | Kind::FunctionDeclaration | Kind::FunctionExpression => {
                return Some(current)
            }
            Kind::ArrowFunction => {
                // Arrow functions don't have their own `new.target`, keep
                // walking up.
            }
            Kind::SourceFile => return None,
            _ => {}
        }
        current = arena.parent(current)?;
    }
}

/// Returns the type-parameter declaration nodes of a class or interface declaration.
// Go: internal/ast/ast.go:Node.TypeParameters
fn type_parameter_nodes(arena: &tsgo_ast::NodeArena, node: NodeId) -> Vec<NodeId> {
    let list = match arena.data(node) {
        NodeData::InterfaceDeclaration(d) => d.type_parameters.as_ref(),
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.type_parameters.as_ref(),
        _ => None,
    };
    list.map(|l| l.nodes.clone()).unwrap_or_default()
}

/// Reports whether `node` is a property access used as an assignment write
/// target (`=` or compound assignment LHS).
// Go: internal/ast/ast.go:IsWriteOnlyAccess (assignment-target subset)
fn is_property_access_write_only(program: &dyn BoundProgram, node: NodeId) -> bool {
    let arena = program.arena();
    let Some(parent) = arena.parent(node) else {
        return false;
    };
    match arena.data(parent) {
        NodeData::BinaryExpression(d) if d.left == node => {
            is_assignment_operator(arena.kind(d.operator_token))
        }
        _ => false,
    }
}

/// Returns `true` if `kind` is an access expression (`PropertyAccessExpression`
/// or `ElementAccessExpression`).
// Go: internal/ast/utilities.go:IsAccessExpression
fn is_access_expression(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression
    )
}

/// Skips parenthesized expressions, returning the innermost non-parenthesized
/// child.
// Go: internal/ast/utilities.go:SkipParentheses
fn skip_parentheses(program: &dyn BoundProgram, mut node: NodeId) -> NodeId {
    loop {
        match program.arena().data(node) {
            NodeData::ParenthesizedExpression(d) => node = d.expression,
            _ => return node,
        }
    }
}

/// Returns `true` if `node` is a dynamic `import()` call (the callee is the
/// `import` keyword token, or an `import.defer` meta-property).
// Go: internal/ast/utilities.go:IsImportCall(2059)
fn is_import_call(program: &dyn BoundProgram, node: NodeId) -> bool {
    let callee = match program.arena().data(node) {
        NodeData::CallExpression(d) => d.expression,
        _ => return false,
    };
    let callee_kind = program.arena().kind(callee);
    if callee_kind == Kind::ImportKeyword {
        return true;
    }
    if callee_kind == Kind::MetaProperty {
        if let NodeData::MetaProperty(d) = program.arena().data(callee) {
            return d.keyword_token == Kind::ImportKeyword
                && program.arena().text(d.name) == "defer";
        }
    }
    false
}

// Go: internal/checker/utilities.go:isTypeUsableAsPropertyName
#[allow(dead_code)]
pub(crate) fn is_type_usable_as_property_name(flags: TypeFlags) -> bool {
    flags.intersects(TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE)
}

/// Returns the declaration of `kind` on `symbol`, if any.
// Go: internal/ast/utilities.go:GetDeclarationOfKind
/// Maps lowercase intrinsic type names in heritage clauses to checker intrinsics
/// (so `implements number` resolves to the primitive, not the `Number` interface).
fn heritage_intrinsic_type(checker: &Checker, name: &str) -> Option<TypeId> {
    match name {
        "number" => Some(checker.number_type()),
        "string" => Some(checker.string_type()),
        "boolean" => Some(checker.boolean_type()),
        "bigint" => Some(checker.bigint_type()),
        "void" => Some(checker.void_type()),
        "undefined" => Some(checker.undefined_type()),
        "null" => Some(checker.null_type()),
        "never" => Some(checker.never_type()),
        "unknown" => Some(checker.unknown_type()),
        "any" => Some(checker.any_type()),
        _ => None,
    }
}

fn get_declaration_of_kind(
    program: &dyn BoundProgram,
    symbol: SymbolId,
    kind: Kind,
) -> Option<NodeId> {
    program
        .symbol(symbol)
        .declarations
        .iter()
        .find(|&&d| program.arena().kind(d) == kind)
        .copied()
}

/// Reports whether `node` is a function-like declaration with a present body.
fn node_has_present_body(program: &dyn BoundProgram, node: NodeId) -> bool {
    match program.arena().data(node) {
        NodeData::FunctionDeclaration(d) => d.body.is_some(),
        NodeData::MethodDeclaration(d) => d.body.is_some(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.body.is_some()
        }
        NodeData::ConstructorDeclaration(d) => d.body.is_some(),
        _ => false,
    }
}

fn has_override_modifier(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    modifier_flags_of(arena, node).contains(ModifierFlags::OVERRIDE)
}

fn has_ambient_modifier(program: &dyn BoundProgram, node: NodeId) -> bool {
    modifier_flags_of(program.arena(), node).contains(ModifierFlags::AMBIENT)
}

// Go: internal/checker/checker.go:Checker.isPropertyWithoutInitializer(4930)
fn is_property_without_initializer(program: &dyn BoundProgram, node: NodeId) -> bool {
    if program.arena().kind(node) != Kind::PropertyDeclaration {
        return false;
    }
    let NodeData::PropertyDeclaration(d) = program.arena().data(node) else {
        return false;
    };
    !has_abstract_modifier(program.arena(), node)
        && !d
            .postfix_token
            .is_some_and(|tok| program.arena().kind(tok) == Kind::ExclamationToken)
        && d.initializer.is_none()
}

fn property_declaration_name_node(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::PropertyDeclaration(d) => {
            let name = d.name;
            match program.arena().kind(name) {
                Kind::Identifier | Kind::PrivateIdentifier | Kind::ComputedPropertyName => {
                    Some(name)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn declaration_name_to_string(program: &dyn BoundProgram, name: NodeId) -> String {
    program.arena().text(name).to_string()
}

// Go: internal/ast/utilities.go:FindConstructorDeclaration
fn find_constructor_declaration(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    object_type_member_nodes(program, node)
        .into_iter()
        .find(|&m| program.arena().kind(m) == Kind::Constructor)
}

fn type_arguments_of_heritage_node(program: &dyn BoundProgram, node: NodeId) -> Vec<NodeId> {
    match program.arena().data(node) {
        NodeData::ExpressionWithTypeArguments(e) => e
            .type_arguments
            .as_ref()
            .map(|list| list.nodes.clone())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Returns the `extends` heritage element of a class-like node, if any.
// Go: internal/ast/utilities.go:GetExtendsHeritageClauseElement
fn get_extends_heritage_clause_element(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let heritage = match program.arena().data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.heritage_clauses.clone(),
        _ => None,
    }?;
    for clause in heritage.nodes {
        if let NodeData::HeritageClause(h) = program.arena().data(clause) {
            if h.token == Kind::ExtendsKeyword {
                return h.types.nodes.first().copied();
            }
        }
    }
    None
}

/// Reports whether `member` is a non-static instance property with an initializer
/// or a private-identifier class element.
// Go: internal/checker/checker.go:isInstancePropertyWithInitializerOrPrivateIdentifierProperty(2888)
fn is_instance_property_with_initializer_or_private_identifier(
    program: &dyn BoundProgram,
    member: NodeId,
) -> bool {
    if is_private_identifier_class_element_declaration(program, member) {
        return true;
    }
    let NodeData::PropertyDeclaration(d) = program.arena().data(member) else {
        return false;
    };
    !has_static_modifier(program.arena(), member) && d.initializer.is_some()
}

/// Reports whether `class_decl` has an initialized instance property or private identifier.
fn class_has_initialized_property_or_private_identifier(
    program: &dyn BoundProgram,
    class_decl: NodeId,
) -> bool {
    object_type_member_nodes(program, class_decl)
        .into_iter()
        .any(|member| is_instance_property_with_initializer_or_private_identifier(program, member))
}

/// Reports whether `constructor` declares a parameter property.
fn constructor_has_parameter_property(program: &dyn BoundProgram, constructor: NodeId) -> bool {
    let NodeData::ConstructorDeclaration(d) = program.arena().data(constructor) else {
        return false;
    };
    d.parameters.nodes.iter().any(|&param| {
        is_parameter_property_declaration(program, param)
    })
}

/// Reports whether `super_call` is a direct child expression statement of `body`.
// Go: internal/checker/checker.go:superCallIsRootLevelInConstructor(2892)
fn super_call_is_root_level_in_constructor(
    program: &dyn BoundProgram,
    super_call: NodeId,
    body: NodeId,
) -> bool {
    let Some(mut current) = program.arena().parent(super_call) else {
        return false;
    };
    while program.arena().kind(current) == Kind::ParenthesizedExpression {
        current = match program.arena().parent(current) {
            Some(parent) => parent,
            None => return false,
        };
    }
    program.arena().kind(current) == Kind::ExpressionStatement
        && program.arena().parent(current) == Some(body)
}

/// Returns the first expression-statement `super()` in `body`, unless a prior
/// statement immediately references `super` or `this`.
// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2849)
fn find_super_call_statement_after_super_or_this_reference(
    program: &dyn BoundProgram,
    constructor: NodeId,
    body: NodeId,
) -> Option<NodeId> {
    let NodeData::Block(d) = program.arena().data(body) else {
        return None;
    };
    for &statement in &d.list.nodes {
        if let NodeData::ExpressionStatement(expr_stmt) = program.arena().data(statement) {
            let expr = skip_outer_expressions(program, expr_stmt.expression);
            if is_super_call(program, expr) {
                return Some(statement);
            }
        }
        if node_immediately_references_super_or_this(program, statement) {
            break;
        }
    }
    let _ = constructor;
    None
}

/// Reports whether `node` or a direct child references `super` or `this`.
// Go: internal/checker/checker.go:nodeImmediatelyReferencesSuperOrThis(2897)
fn node_immediately_references_super_or_this(program: &dyn BoundProgram, node: NodeId) -> bool {
    match program.arena().kind(node) {
        Kind::SuperKeyword | Kind::ThisKeyword => true,
        Kind::ArrowFunction
        | Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::PropertyDeclaration => false,
        Kind::Block => {
            if let Some(parent) = program.arena().parent(node) {
                matches!(
                    program.arena().kind(parent),
                    Kind::Constructor
                        | Kind::MethodDeclaration
                        | Kind::GetAccessor
                        | Kind::SetAccessor
                )
            } else {
                false
            }
        }
        _ => program
            .arena()
            .for_each_child(node, &mut |child| {
                node_immediately_references_super_or_this(program, child)
            }),
    }
}

/// Locates the first `super()` call in `node`'s subtree (skipping nested functions).
// Go: internal/checker/checker.go:Checker.findFirstSuperCall(2871)
fn find_first_super_call(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    fn visit(program: &dyn BoundProgram, node: NodeId, found: &mut Option<NodeId>) -> bool {
        if is_super_call(program, node) {
            *found = Some(node);
            return true;
        }
        if matches!(
            program.arena().kind(node),
            Kind::FunctionDeclaration
                | Kind::FunctionExpression
                | Kind::ArrowFunction
                | Kind::MethodDeclaration
                | Kind::GetAccessor
                | Kind::SetAccessor
        ) {
            return false;
        }
        program
            .arena()
            .for_each_child(node, &mut |child| visit(program, child, found))
    }
    let mut found = None;
    visit(program, node, &mut found);
    found
}

/// Reports whether `node` is a `super` call (`super` or `super(...)`).
fn is_super_call(program: &dyn BoundProgram, node: NodeId) -> bool {
    let inner = skip_outer_expressions(program, node);
    if program.arena().kind(inner) == Kind::SuperKeyword {
        return true;
    }
    matches!(
        program.arena().data(inner),
        NodeData::CallExpression(d) if program.arena().kind(d.expression) == Kind::SuperKeyword
    )
}

/// Returns the innermost class member container for a `super` reference.
// Go: internal/checker/utilities.go:getSuperContainer(1150)
fn get_super_container(
    program: &dyn BoundProgram,
    node: NodeId,
    stop_on_functions: bool,
) -> Option<NodeId> {
    let arena = program.arena();
    let mut current = arena.parent(node)?;
    loop {
        match arena.kind(current) {
            Kind::ComputedPropertyName => {
                current = arena.parent(current)?;
            }
            Kind::FunctionDeclaration | Kind::FunctionExpression | Kind::ArrowFunction => {
                if !stop_on_functions {
                    current = arena.parent(current)?;
                    continue;
                }
                return None;
            }
            Kind::PropertyDeclaration
            | Kind::PropertySignature
            | Kind::MethodDeclaration
            | Kind::MethodSignature
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::ClassStaticBlockDeclaration => return Some(current),
            Kind::SourceFile => return None,
            _ => current = arena.parent(current)?,
        }
    }
}

fn get_containing_class(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    let arena = program.arena();
    let mut current = arena.parent(node);
    while let Some(n) = current {
        if matches!(
            arena.kind(n),
            Kind::ClassDeclaration | Kind::ClassExpression
        ) {
            return Some(n);
        }
        current = arena.parent(n);
    }
    None
}

// Mangles a private identifier description into the class-scoped symbol name
// (Go's `binder.GetSymbolNameForPrivateIdentifier`).
// Go: internal/binder/binder.go:GetSymbolNameForPrivateIdentifier
fn get_symbol_name_for_private_identifier(class_symbol: SymbolId, description: &str) -> String {
    format!(
        "{}#{}@{}",
        tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_PREFIX,
        class_symbol.0,
        description
    )
}

// Walks enclosing classes from `location` to resolve a private identifier
// declaration symbol (Go's `lookupSymbolForPrivateIdentifierDeclaration`).
// Go: internal/checker/checker.go:Checker.lookupSymbolForPrivateIdentifierDeclaration(11425)
fn lookup_symbol_for_private_identifier_declaration(
    program: &dyn BoundProgram,
    prop_name: &str,
    location: NodeId,
) -> Option<SymbolId> {
    let mut containing_class = get_containing_class(program, location);
    while let Some(class_node) = containing_class {
        let class_symbol = program.symbol_of_node(class_node)?;
        let name = get_symbol_name_for_private_identifier(class_symbol, prop_name);
        let class_sym = program.symbol(class_symbol);
        if let Some(&prop) = class_sym.members.get(&name) {
            return Some(prop);
        }
        if let Some(&prop) = class_sym.exports.get(&name) {
            return Some(prop);
        }
        containing_class = get_containing_class(program, class_node);
    }
    None
}

pub(crate) fn is_enum_const(program: &dyn BoundProgram, node: NodeId) -> bool {
    modifier_flags_of(program.arena(), node).contains(tsgo_ast::ModifierFlags::CONST)
}

fn enum_decl_name(program: &dyn BoundProgram, node: NodeId) -> Option<NodeId> {
    match program.arena().data(node) {
        NodeData::EnumDeclaration(d) => Some(d.name),
        _ => None,
    }
}

/// Skips parenthesized and assertion wrappers (partial port of Go's
/// `SkipOuterExpressions` with `OEKAll`).
// Go: internal/ast/utilities.go:SkipOuterExpressions
fn skip_outer_expressions(program: &dyn BoundProgram, mut node: NodeId) -> NodeId {
    loop {
        match program.arena().kind(node) {
            Kind::ParenthesizedExpression => {
                node = match program.arena().data(node) {
                    NodeData::ParenthesizedExpression(d) => d.expression,
                    _ => break,
                };
            }
            Kind::AsExpression | Kind::TypeAssertionExpression | Kind::SatisfiesExpression => {
                node = match program.arena().data(node) {
                    NodeData::AsExpression(d) => d.expression,
                    NodeData::TypeAssertionExpression(d) => d.expression,
                    NodeData::SatisfiesExpression(d) => d.expression,
                    _ => break,
                };
            }
            _ => break,
        }
    }
    node
}

/// The node that owns a pushed contextual type (Go's `getContextNode`).
// Go: internal/checker/checker.go:Checker.getContextNode
fn get_context_node(_program: &dyn BoundProgram, node: NodeId) -> NodeId {
    node
}

/// Reports whether `node` sits in a type-syntax position (Go's
/// `IsPartOfTypeNode`, reachable subset).
// Go: internal/ast/utilities.go:IsPartOfTypeNode
pub(crate) fn is_part_of_type_node(program: &dyn BoundProgram, node: NodeId) -> bool {
    let arena = program.arena();
    let kind = arena.kind(node);
    if kind >= Kind::FIRST_TYPE_NODE && kind <= Kind::LAST_TYPE_NODE {
        return true;
    }
    matches!(
        kind,
        Kind::AnyKeyword
            | Kind::UnknownKeyword
            | Kind::NumberKeyword
            | Kind::BigIntKeyword
            | Kind::StringKeyword
            | Kind::BooleanKeyword
            | Kind::SymbolKeyword
            | Kind::ObjectKeyword
            | Kind::UndefinedKeyword
            | Kind::NullKeyword
            | Kind::NeverKeyword
    ) || (kind == Kind::VoidKeyword
        && program
            .arena()
            .parent(node)
            .is_none_or(|p| arena.kind(p) != Kind::VoidExpression))
}

/// Reports whether `node` is an expression node (Go's `IsExpressionNode`,
/// reachable subset used by `getTypeOfNode`).
// Go: internal/ast/utilities.go:IsExpressionNode
pub(crate) fn is_expression_node(program: &dyn BoundProgram, node: NodeId) -> bool {
    if is_part_of_type_node(program, node) {
        return false;
    }
    let arena = program.arena();
    if super::symbols_query::is_declaration(arena, node)
        || super::symbols_query::is_declaration_name(arena, node)
    {
        return false;
    }
    matches!(
        arena.kind(node),
        Kind::Identifier
            | Kind::StringLiteral
            | Kind::NumericLiteral
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
            | Kind::ThisKeyword
            | Kind::PropertyAccessExpression
            | Kind::ElementAccessExpression
            | Kind::CallExpression
            | Kind::NewExpression
            | Kind::BinaryExpression
            | Kind::ObjectLiteralExpression
            | Kind::ArrayLiteralExpression
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::ParenthesizedExpression
            | Kind::NonNullExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeOfExpression
            | Kind::VoidExpression
            | Kind::DeleteExpression
            | Kind::JsxElement
            | Kind::JsxSelfClosingElement
            | Kind::JsxFragment
            | Kind::MetaProperty
    )
}

fn declaration_modifier_flags_from_symbol(
    checker: &Checker,
    program: &dyn BoundProgram,
    symbol: SymbolId,
    is_write: bool,
) -> ModifierFlags {
    if super::is_synthesized_symbol(symbol) {
        let check_flags = checker.synthesized_symbol_check_flags(symbol);
        let access = if check_flags.contains(CheckFlags::CONTAINS_PRIVATE) {
            ModifierFlags::PRIVATE
        } else if check_flags.contains(CheckFlags::CONTAINS_PROTECTED) {
            ModifierFlags::PROTECTED
        } else {
            ModifierFlags::PUBLIC
        };
        let static_mod = if check_flags.contains(CheckFlags::CONTAINS_STATIC) {
            ModifierFlags::STATIC
        } else {
            ModifierFlags::empty()
        };
        return access | static_mod;
    }
    // Declaration nodes may live in another file's arena (lib globals, merged
    // cross-file symbols). Read them through the view that owns `symbol`.
    let owner = program.view_for_symbol(symbol);
    let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
    let sym = prog.symbol(symbol);
    if let Some(mut decl) = sym.value_declaration {
        if is_write {
            decl = sym
                .declarations
                .iter()
                .copied()
                .find(|&d| prog.arena().kind(d) == Kind::SetAccessor)
                .unwrap_or(decl);
        } else if sym.flags.intersects(SymbolFlags::GET_ACCESSOR) {
            if let Some(getter) = sym
                .declarations
                .iter()
                .copied()
                .find(|&d| prog.arena().kind(d) == Kind::GetAccessor)
            {
                decl = getter;
            }
        }
        let flags = combined_modifier_flags(prog, decl);
        if sym
            .parent
            .is_some_and(|p| prog.symbol(p).flags.intersects(SymbolFlags::CLASS))
        {
            return flags;
        }
        return flags & !ModifierFlags::ACCESSIBILITY_MODIFIER;
    }
    if sym.flags.intersects(SymbolFlags::PROTOTYPE) {
        return ModifierFlags::PUBLIC | ModifierFlags::STATIC;
    }
    ModifierFlags::empty()
}

fn combined_modifier_flags(program: &dyn BoundProgram, node: NodeId) -> ModifierFlags {
    let node = get_root_declaration(program, node);
    let mut flags = modifier_flags_of(program.arena(), node);
    let mut current = node;
    if program.arena().kind(current) == Kind::VariableDeclaration {
        if let Some(parent) = program.arena().parent(current) {
            current = parent;
        }
    }
    if program.arena().kind(current) == Kind::VariableDeclarationList {
        flags |= modifier_flags_of(program.arena(), current);
        if let Some(parent) = program.arena().parent(current) {
            current = parent;
        }
    }
    if program.arena().kind(current) == Kind::VariableStatement {
        flags |= modifier_flags_of(program.arena(), current);
    }
    flags
}

fn get_declaring_class_declaration(program: &dyn BoundProgram, prop: SymbolId) -> Option<NodeId> {
    let parent = program.symbol(prop).parent?;
    get_class_like_declaration_of_symbol(program, parent)
}

fn get_class_like_declaration_of_symbol(
    program: &dyn BoundProgram,
    symbol: SymbolId,
) -> Option<NodeId> {
    let owner = program.view_for_symbol(symbol);
    let prog: &dyn BoundProgram = owner.as_deref().unwrap_or(program);
    let sym = prog.symbol(symbol);
    sym.value_declaration
        .or_else(|| sym.declarations.first().copied())
}

fn declaring_class_name(program: &dyn BoundProgram, prop: SymbolId) -> String {
    if super::is_synthesized_symbol(prop) {
        return String::new();
    }
    program
        .symbol(prop)
        .parent
        .map(|p| program.symbol(p).name.clone())
        .unwrap_or_default()
}

fn is_node_within_class(
    program: &dyn BoundProgram,
    node: NodeId,
    class_declaration: NodeId,
) -> bool {
    let mut container = get_containing_class(program, node);
    while let Some(class) = container {
        if class == class_declaration {
            return true;
        }
        container = get_containing_class(program, class);
    }
    false
}

/// Reports whether `node` is the right-hand name of `a.b` / `a?.b` (Go's
/// `IsRightSideOfQualifiedNameOrPropertyAccess`).
// Go: internal/ast/utilities.go:IsRightSideOfQualifiedNameOrPropertyAccess
fn is_right_side_of_qualified_name_or_property_access(
    program: &dyn BoundProgram,
    node: NodeId,
) -> bool {
    let Some(parent) = program.arena().parent(node) else {
        return false;
    };
    match program.arena().data(parent) {
        NodeData::PropertyAccessExpression(d) => d.name == node,
        NodeData::QualifiedName(d) => d.right == node,
        _ => false,
    }
}

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
