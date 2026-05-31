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
//! DEFER(phase-4-checker-4ab+): the comma operator, the `with` statement
//! (reachable path is grammar-only), module declaration bodies, contextual
//! typing, unused checks, and the full node builder. The logical (`&&`/`||`/`??`)
//! and `+` operators, compound assignments, and `throw`/labeled statements landed
//! in 4p; call-expression argument checking in 4q; overload resolution, class
//! member bodies / property initializers, and function-body descent with
//! return-statement / annotated return-type checking in 4r; the `instanceof`
//! (`2358`/`2359`, driven by a synthetic global `Function`) and `in`
//! (operand assignability `2322`) operators in 4ab.

use std::rc::Rc;

use tsgo_ast::{Kind, NodeData, NodeId, SymbolFlags};
use tsgo_core::compileroptions::ScriptTarget;
use tsgo_diagnostics::{Category, Message};

use super::declared_types::{
    get_apparent_type, get_property_of_type, get_type_of_property_of_type, get_type_of_symbol,
};
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::signatures::SignatureId;
use super::symbols::resolve_name;
use super::type_facts::TypeFacts;
use super::types::{LiteralValue, TypeFlags, TypeId};
use super::Checker;

/// A type-checking diagnostic produced while checking a source file.
///
/// A minimal stand-in for Go's `ast.Diagnostic` (which also carries the file
/// and message chains); 4g records the span, code, category, and localized
/// text, and 4aj adds the related-information list (Go's `relatedInformation`).
///
/// DEFER(phase-4-checker-4j): message chains + the owning `SourceFile`.
/// blocked-by: the real `ast.Diagnostic`/`DiagnosticsCollection` (program-level,
/// P6) and the node builder (4j).
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
    /// };
    /// let mut primary = Diagnostic {
    ///     code: 2488,
    ///     category: Category::Error,
    ///     message: "Type 'T' must have a '[Symbol.iterator]()' method that returns an iterator.".to_string(),
    ///     start: 0,
    ///     length: 1,
    ///     related_information: Vec::new(),
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
        match program.arena().kind(node) {
            Kind::Identifier => self.check_identifier(program, node),
            Kind::StringLiteral => {
                let text = program.arena().text(node).to_string();
                self.new_literal_type(TypeFlags::STRING_LITERAL, LiteralValue::String(text), None)
            }
            Kind::NumericLiteral => {
                let value = tsgo_jsnum::from_string(program.arena().text(node));
                self.new_literal_type(TypeFlags::NUMBER_LITERAL, LiteralValue::Number(value), None)
            }
            Kind::TrueKeyword => self.true_type,
            Kind::FalseKeyword => self.false_type,
            Kind::NullKeyword => self.null_type,
            Kind::PropertyAccessExpression => self.check_property_access(program, node),
            Kind::ElementAccessExpression => self.check_element_access(program, node),
            Kind::CallExpression => self.check_call_expression(program, node),
            Kind::BinaryExpression => self.check_binary_expression(program, node),
            Kind::JsxSelfClosingElement => self.check_jsx_self_closing_element(program, node),
            Kind::JsxElement => self.check_jsx_element(program, node),
            Kind::JsxFragment => self.check_jsx_fragment(program, node),
            Kind::FunctionExpression => self.check_function_expression(program, node),
            Kind::ArrowFunction => self.check_arrow_function(program, node),
            Kind::NonNullExpression => self.check_non_null_assertion(program, node),
            // DEFER(phase-4-checker-4h+): remaining expression kinds are added in
            // later 4g slices / sub-phases.
            _ => self.error_type,
        }
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

    // Resolves an identifier reference to its (flow-narrowed) value type.
    // Go: internal/checker/checker.go:Checker.checkIdentifier(10999)
    fn check_identifier(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let name = program.arena().text(node).to_string();
        match resolve_name(program, node, &name, SymbolFlags::VALUE, false, None) {
            None => {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0,
                    &[name.as_str()],
                );
                self.error_type
            }
            Some(symbol) => {
                let globals = program.globals();
                let declared = get_type_of_symbol(self, program, symbol, globals);
                self.get_flow_type_of_reference(program, node, declared)
            }
        }
    }

    // Checks a property access `obj.name`, returning the property's type.
    // Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression
    fn check_property_access(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, name_node) = match program.arena().data(node) {
            NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
            _ => return self.error_type,
        };
        let object_type = self.check_expression(program, expr);
        let name = program.arena().text(name_node).to_string();
        match get_type_of_property_of_type(self, program, object_type, &name) {
            Some(t) => t,
            None => {
                let type_str = super::nodebuilder::type_to_string(self, program, object_type);
                self.error(
                    program,
                    name_node,
                    &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1,
                    &[name.as_str(), type_str.as_str()],
                );
                self.error_type
            }
        }
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
        let object_type = self.check_expression(program, expr);
        if program.arena().kind(arg) == Kind::StringLiteral {
            let name = program.arena().text(arg).to_string();
            if let Some(t) = get_type_of_property_of_type(self, program, object_type, &name) {
                return t;
            }
        }
        let index_type = self.check_expression(program, arg);
        if let Some(t) =
            super::declared_types::get_indexed_access_type(self, program, object_type, index_type)
        {
            return t;
        }
        // Go's `getPropertyTypeForIndexType` ends on `Type_0_cannot_be_used_as_an
        // _index_type` (2538) when the index is not a string/number literal name
        // and is not string/number: such a key (e.g. `boolean`) is not assignable
        // to any index signature and never enters the index-signature block. The
        // 4af subset reports 2538 for a non-string/number/symbol-like index that
        // resolved no element type; `any`/`never` indices are excluded (Go returns
        // the index/object type for them).
        // DEFER(phase-4-checker-4af+): the `7053` implicit-any element access
        // (`noImplicitAny` wiring) and the symbol-keyed string-index fallback.
        // blocked-by: `noImplicitAny` option plumbing + ES-symbol globals (P6).
        let index_flags = self.get_type(index_type).flags();
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
    // DEFER(phase-4-checker-4ab+): the comma operator and
    // destructuring-assignment targets, plus the per-operator refinements noted
    // on each arm below. blocked-by: per-operator slices land later; lib globals
    // (P6) for the ES-symbol operand / awaited types, `strictNullChecks` wiring
    // for `??`, and 4b union literal/subtype reduction for the logical results.
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
                self.check_assignment_operator(program, left, left_type, right_type);
                right_type
            }
            // Relational operators (`<`/`>`/`<=`/`>=`) yield `boolean`; the
            // operands' literal types are based for comparison, then an
            // incomparable pair reports `2365` (Go's relational arm).
            Kind::LessThanToken
            | Kind::GreaterThanToken
            | Kind::LessThanEqualsToken
            | Kind::GreaterThanEqualsToken => {
                let left_base = self.get_base_type_of_literal_type_for_comparison(left_type);
                let right_base = self.get_base_type_of_literal_type_for_comparison(right_type);
                if !self.relational_operands_comparable(program, left_base, right_base) {
                    self.report_binary_operator_error(
                        program,
                        node,
                        operator_token,
                        left_base,
                        right_base,
                    );
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
                    let left_base = self.get_base_type_of_literal_type(left_type);
                    let right_base = self.get_base_type_of_literal_type(right_type);
                    let (error_left, error_right) =
                        if self.equality_operands_comparable(program, left_base, right_base) {
                            (left_type, right_type)
                        } else {
                            (left_base, right_base)
                        };
                    self.report_binary_operator_error(
                        program,
                        node,
                        operator_token,
                        error_left,
                        error_right,
                    );
                }
                self.boolean_type
            }
            // Arithmetic operators (`-`/`*`/`/`/`%`/`**`/shifts/bitwise) require
            // number-ish operands and yield `number` (Go's arithmetic arm).
            //
            // DEFER(phase-4-checker-4o+): the `bigint` result + mixed-operand
            // (`reportOperatorError`) path, the boolean-bitwise suggestion
            // (`The_0_operator_is_not_allowed_for_boolean_types`), the shift
            // simplification suggestion, and compound assignments (`*=` etc.,
            // which also run `checkAssignmentOperator`). blocked-by: `maybeTypeOfKind`
            // bigint handling + `evaluate`-based shift constants + compound-assign
            // reference/write-type resolution.
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
                let left_ok = self.check_arithmetic_operand_type(
                    program,
                    left,
                    left_type,
                    &tsgo_diagnostics::THE_LEFT_HAND_SIDE_OF_AN_ARITHMETIC_OPERATION_MUST_BE_OF_TYPE_ANY_NUMBER_BIGINT_OR_AN_ENUM_TYPE,
                );
                let right_ok = self.check_arithmetic_operand_type(
                    program,
                    right,
                    right_type,
                    &tsgo_diagnostics::THE_RIGHT_HAND_SIDE_OF_AN_ARITHMETIC_OPERATION_MUST_BE_OF_TYPE_ANY_NUMBER_BIGINT_OR_AN_ENUM_TYPE,
                );
                let result_type = self.number_type;
                // For a compound assignment (`*=` etc.), the implied result must
                // be assignable to the (reference) left-hand side, but only once
                // both operands type-checked (Go guards on `leftOk && rightOk`).
                if left_ok && right_ok && is_compound_assignment(operator) {
                    self.check_assignment_operator(program, left, left_type, result_type);
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
                    self.check_assignment_operator(program, left, left_type, right_type);
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
                    self.check_assignment_operator(program, left, left_type, right_type);
                }
                result
            }
            // The `??`/`??=` operator (Go's `KindQuestionQuestionToken` arm): the
            // result is the left type, refined to the union of the left type's
            // non-nullable part and the right type when the left type can be
            // `undefined`/`null`. For a non-nullable left, the result is exactly
            // the left type.
            //
            // DEFER(phase-4-checker-4p+): the nullish refinement of the result
            // (`hasTypeFacts(EQUndefinedOrNull)` + `GetNonNullableType`) and the
            // `checkNullishCoalesceOperands` diagnostics. blocked-by: the
            // `EQUndefinedOrNull` type facts + `strictNullChecks` wiring.
            Kind::QuestionQuestionToken | Kind::QuestionQuestionEqualsToken => {
                if operator == Kind::QuestionQuestionEqualsToken {
                    self.check_assignment_operator(program, left, left_type, right_type);
                }
                left_type
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
                        // For `+=`, the result must be assignable to the
                        // (reference) left-hand side (Go runs `checkAssignmentOperator`
                        // only when a valid result exists).
                        if operator == Kind::PlusEqualsToken {
                            self.check_assignment_operator(program, left, left_type, rt);
                        }
                        rt
                    }
                    None => {
                        // No applicable result: the operator cannot be applied.
                        // DEFER(phase-4-checker-4p+): literal-operand generalization
                        // for the message (`getBaseTypesIfUnrelated`). blocked-by:
                        // the literal-generalization helper.
                        self.report_binary_operator_error(
                            program,
                            node,
                            operator_token,
                            left_type,
                            right_type,
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
            // DEFER(phase-4-checker-4ab+): the comma operator. The operands are
            // still checked above, so diagnostics inside them are reported.
            _ => self.error_type,
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
    // DEFER(phase-4-checker-4ab+): the private-identifier left operand
    // (`#x in obj`) and the empty-object-intersection right-operand check
    // (`2638`) are deferred. blocked-by: private-identifier expressions +
    // `hasEmptyObjectIntersection`.
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
        // The left operand must be assignable to `string | number | symbol`.
        let string_number_symbol =
            self.get_union_type(&[self.string_type, self.number_type, self.es_symbol_type]);
        self.check_type_assignable_to_or_error(program, left, left_type, string_number_symbol);
        // The right operand must be assignable to `object` (the non-primitive
        // intrinsic).
        let non_primitive = self.non_primitive_type;
        self.check_type_assignable_to_or_error(program, right, right_type, non_primitive);
        self.boolean_type
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
    // setter's type), the "left-hand side is not a reference/optional chain"
    // diagnostics, destructuring targets, and `exactOptionalPropertyTypes`
    // elaboration. blocked-by: `checkReferenceExpression`'s diagnostics +
    // write-type resolution + destructuring.
    // Go: internal/checker/checker.go:Checker.checkAssignmentOperator(12701)
    fn check_assignment_operator(
        &mut self,
        program: &dyn BoundProgram,
        left: NodeId,
        left_type: TypeId,
        right_type: TypeId,
    ) {
        // A reference target is an identifier or an access expression (Go's
        // `checkReferenceExpression`); other targets are skipped here.
        if !is_reference_expression(program, left) {
            return;
        }
        if !self.is_type_assignable_to(program, right_type, left_type) {
            let generalized = self.generalized_source_for_error(right_type, left_type);
            let source_str = super::nodebuilder::type_to_string(self, program, generalized);
            let target_str = super::nodebuilder::type_to_string(self, program, left_type);
            self.error(
                program,
                left,
                &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
                &[source_str.as_str(), target_str.as_str()],
            );
        }
    }

    // Checks that an arithmetic `operand` of type `t` is number-ish (assignable
    // to `number | bigint`), reporting `diagnostic` at the operand otherwise.
    // Returns `true` when no error was reported (Go's `checkArithmeticOperandType`).
    //
    // DEFER(phase-4-checker-4o+): the `await`-suggestion path
    // (`getAwaitedTypeOfPromise`). blocked-by: awaited-type machinery (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.checkArithmeticOperandType(12743)
    fn check_arithmetic_operand_type(
        &mut self,
        program: &dyn BoundProgram,
        operand: NodeId,
        t: TypeId,
        diagnostic: &'static Message,
    ) -> bool {
        let number_or_bigint = self.number_or_bigint_type;
        if !self.is_type_assignable_to(program, t, number_or_bigint) {
            self.error(program, operand, diagnostic, &[]);
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
    // Go: internal/checker/checker.go:Checker.isTypeAssignableToKindEx(20196)
    fn is_type_assignable_to_kind_strict(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        kind: TypeFlags,
    ) -> bool {
        let f = self.get_type(source).flags();
        if f.intersects(kind) {
            return true;
        }
        // Strict mode: the top/void/nullable types are not assignable to a
        // primitive kind (Go's `strict` guard).
        if f.intersects(TypeFlags::ANY_OR_UNKNOWN | TypeFlags::VOID | TypeFlags::NULLABLE) {
            return false;
        }
        (kind.intersects(TypeFlags::NUMBER_LIKE)
            && self.is_type_assignable_to(program, source, self.number_type))
            || (kind.intersects(TypeFlags::STRING_LIKE)
                && self.is_type_assignable_to(program, source, self.string_type))
            || (kind.intersects(TypeFlags::BIG_INT_LIKE)
                && self.is_type_assignable_to(program, source, self.bigint_type))
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
    fn are_types_comparable(&mut self, program: &dyn BoundProgram, a: TypeId, b: TypeId) -> bool {
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
    // DEFER(phase-4-checker-4o+): the `await`-suggestion variant
    // (`errorAndMaybeSuggestAwait`) and the equal-printed-name fully-qualified
    // fallback. blocked-by: awaited-type machinery (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.reportOperatorError(12662)
    fn report_binary_operator_error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        operator_token: NodeId,
        left: TypeId,
        right: TypeId,
    ) {
        let left_str = super::nodebuilder::type_to_string(self, program, left);
        let right_str = super::nodebuilder::type_to_string(self, program, right);
        let operator = program.arena().kind(operator_token);
        match operator {
            Kind::EqualsEqualsToken
            | Kind::ExclamationEqualsToken
            | Kind::EqualsEqualsEqualsToken
            | Kind::ExclamationEqualsEqualsToken => {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::THIS_COMPARISON_APPEARS_TO_BE_UNINTENTIONAL_BECAUSE_THE_TYPES_0_AND_1_HAVE_NO_OVERLAP,
                    &[left_str.as_str(), right_str.as_str()],
                );
            }
            _ => {
                let op = tsgo_scanner::token_to_string(operator);
                self.error(
                    program,
                    node,
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
        let (callee, args) = match program.arena().data(node) {
            NodeData::CallExpression(d) => (d.expression, d.arguments.nodes.clone()),
            _ => return self.error_type,
        };
        let func_type = self.check_expression(program, callee);
        let signatures = self.get_signatures_of_type(func_type);
        let Some(&signature) = signatures.first() else {
            // No call signatures (e.g. an `any`/error callee or a non-callable
            // value). Still check the argument expressions so nested diagnostics
            // surface; the invocation error itself is deferred.
            for &arg in &args {
                self.check_expression(program, arg);
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
        // `getReturnTypeOfSignature`). For the non-generic signatures 4q resolves
        // this is the declared return type; the parameter/argument-type slices
        // are unused here (generic call-site inference is deferred).
        self.get_return_type_of_call(program, signature, &[], &[])
    }

    // Reports whether the argument count matches the signature's arity (the
    // arity portion of Go's `hasCorrectArity`, 4q subset for a non-rest,
    // non-spread, complete call): the count must be at least the minimum
    // argument count.
    //
    // DEFER(phase-4-checker-4q+): rest parameters, spread arguments, incomplete
    // calls (missing close paren), and the `void`-accepting trailing-parameter
    // relaxation. blocked-by: rest/tuple types + spread detection + grammar end
    // positions.
    // Go: internal/checker/checker.go:Checker.hasCorrectArity(9070)
    fn has_correct_arity(&self, signature: SignatureId, arg_count: usize) -> bool {
        let arg_count = arg_count as i32;
        arg_count >= self.get_min_argument_count(signature)
            && arg_count <= self.get_parameter_count(signature) as i32
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
                let generalized = self.generalized_source_for_error(arg_type, param_type);
                let source_str = super::nodebuilder::type_to_string(self, program, generalized);
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                self.error(
                    program,
                    arg,
                    &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1,
                    &[source_str.as_str(), target_str.as_str()],
                );
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
                // Go wraps the last candidate's argument error in the
                // `No_overload_matches_this_call` chain. 4r reports just the
                // top-level `2769` at the call's error node (chain deferred).
                let error_node = call_error_node(program, node);
                self.error(
                    program,
                    error_node,
                    &tsgo_diagnostics::NO_OVERLOAD_MATCHES_THIS_CALL,
                    &[],
                );
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
                let generalized = self.generalized_source_for_error(arg_type, param_type);
                let source_str = super::nodebuilder::type_to_string(self, program, generalized);
                let target_str = super::nodebuilder::type_to_string(self, program, param_type);
                self.error(
                    program,
                    args[i],
                    &tsgo_diagnostics::ARGUMENT_OF_TYPE_0_IS_NOT_ASSIGNABLE_TO_PARAMETER_OF_TYPE_1,
                    &[source_str.as_str(), target_str.as_str()],
                );
                return;
            }
        }
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
    // `getTypeAtPosition` -> `getTypeOfParameter`), or `any` when out of range.
    //
    // DEFER(phase-4-checker-4q+): rest-parameter indexed access. blocked-by:
    // tuple/indexed-access types.
    // Go: internal/checker/relater.go:Checker.getTypeAtPosition(1754)
    fn get_type_at_position(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        pos: usize,
    ) -> TypeId {
        match self.signature(signature).parameters.get(pos).copied() {
            Some(symbol) => get_type_of_symbol(self, program, symbol, None),
            None => self.any_type,
        }
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
        for statement in statements {
            self.check_statement(view.as_ref(), statement);
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
        self.check_grammar_modifiers(program, node);
        // Class members carry their own modifiers (e.g. accessibility), so run
        // the grammar checks on each, then check each member (4r descends into
        // method/accessor/constructor bodies and checks property initializers so
        // nested diagnostics surface). A class expression is checked the same way
        // when reached as a statement-position expression.
        if let NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) =
            program.arena().data(node)
        {
            let members = d.members.nodes.clone();
            for member in members {
                self.check_grammar_modifiers(program, member);
                self.check_class_member(program, member);
            }
        }
        if let NodeData::ExpressionStatement(d) = program.arena().data(node) {
            let expr = d.expression;
            self.check_expression(program, expr);
        }
        if let NodeData::VariableStatement(d) = program.arena().data(node) {
            self.check_variable_declaration_list(program, d.declaration_list);
        }
        // A `{ ... }` block checks each contained statement (Go's `checkBlock` ->
        // `checkSourceElements`).
        if let NodeData::Block(d) = program.arena().data(node) {
            let statements = d.list.nodes.clone();
            for statement in statements {
                self.check_statement(program, statement);
            }
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
            }
        }
        // A `throw` statement checks its thrown expression (Go's
        // `checkThrowStatement` -> `c.checkExpression(throwExpr)`), so diagnostics
        // inside it surface.
        // DEFER(phase-4-checker-4p+): the ambient-context and empty-identifier
        // line-break grammar checks. blocked-by: `checkGrammarStatementInAmbientContext`
        // + grammar position helpers.
        if let NodeData::ThrowStatement(d) = program.arena().data(node) {
            let expression = d.expression;
            self.check_expression(program, expression);
        }
        // A labeled statement descends into its labeled statement (Go's
        // `checkLabeledStatement` -> `checkSourceElement(statement)`), so
        // diagnostics inside it surface.
        // DEFER(phase-4-checker-4p+): the duplicate-label grammar diagnostic
        // (`Duplicate_label_0`, needs a parent walk) and the unused-label
        // suggestion (`Unused_label`, needs `NodeFlagsUnreachable`/flow).
        // blocked-by: grammar parent-walk + flow reachability flags.
        if let NodeData::LabeledStatement(d) = program.arena().data(node) {
            let statement = d.statement;
            self.check_statement(program, statement);
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
        if let NodeData::FunctionDeclaration(d) = program.arena().data(node) {
            if let Some(body) = d.body {
                self.check_statement(program, body);
            }
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
        if let NodeData::ReturnStatement(d) = program.arena().data(node) {
            if let Some(expression) = d.expression {
                self.check_return_statement_expression(program, node, expression);
            }
        }
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
    // DEFER(phase-4-checker-4ar+): the inferred/contextual function type
    // (`checkExpressionWithContextualType`), parameter/`this` checking,
    // generator/async return unwrapping, and the un-annotated body return-type
    // inference. blocked-by: contextual typing + signature/`this` machinery +
    // awaited/iterable types (lib globals, P6) + body return-type inference.
    // Go: internal/checker/checker.go:Checker.checkFunctionExpressionOrObjectLiteralMethod
    fn check_function_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        if let NodeData::FunctionExpression(d) = program.arena().data(node) {
            if let Some(body) = d.body {
                self.check_statement(program, body);
            }
        }
        self.error_type
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
    // DEFER(phase-4-checker-4ar+): the inferred/contextual function type
    // (`checkExpressionWithContextualType` for an un-annotated arrow), parameter/
    // `this` checking, and generator/async unwrapping (the awaited type of an
    // async concise body against the promised return type). blocked-by:
    // contextual typing + signature/`this` machinery + awaited types (P6).
    // Go: internal/checker/checker.go:Checker.checkArrowFunction / checkFunctionExpressionOrObjectLiteralMethodDeferred
    fn check_arrow_function(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        if let NodeData::ArrowFunction(d) = program.arena().data(node) {
            let body = d.body;
            if program.arena().kind(body) == Kind::Block {
                self.check_statement(program, body);
            } else {
                self.check_return_statement_expression(program, body, body);
            }
        }
        self.error_type
    }

    // Checks a `return <expr>` (Go's `checkReturnStatement`): the returned
    // expression is always checked; when the enclosing function-like declaration
    // carries an explicit return-type annotation (reachable via 4q's signature
    // machinery), the returned expression's type must be assignable to that
    // annotated return type, else `2322`.
    //
    // DEFER(phase-4-checker-4r+): contextual return-type inference for an
    // un-annotated function, generator/async return unwrapping, and the
    // void/never special cases. blocked-by: contextual return-type inference +
    // generator/async awaited types (lib globals, P6).
    // Go: internal/checker/checker.go:Checker.checkReturnStatement
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
    // types and the constructor/accessor special cases. blocked-by: contextual
    // typing + accessor-pair resolution.
    // Go: internal/checker/checker.go:getContainingFunctionOrClassStaticBlock + getReturnTypeOfSignature
    fn enclosing_explicit_return_type(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> Option<TypeId> {
        let mut current = program.arena().parent(node);
        while let Some(id) = current {
            let return_type_node = match program.arena().data(id) {
                NodeData::FunctionDeclaration(d) => Some(d.type_node),
                NodeData::MethodDeclaration(d) => Some(d.type_node),
                NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                    Some(d.type_node)
                }
                NodeData::ConstructorDeclaration(d) => Some(d.type_node),
                NodeData::FunctionExpression(d) => Some(d.type_node),
                NodeData::ArrowFunction(d) => Some(d.type_node),
                _ => None,
            };
            if let Some(return_type_node) = return_type_node {
                return return_type_node.map(|n| {
                    super::declared_types::get_type_from_type_node(self, program, n, None)
                });
            }
            current = program.arena().parent(id);
        }
        None
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
            NodeData::MethodDeclaration(d) => {
                if let Some(body) = d.body {
                    self.check_statement(program, body);
                }
            }
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                if let Some(body) = d.body {
                    self.check_statement(program, body);
                }
            }
            NodeData::ConstructorDeclaration(d) => {
                let body = d.body;
                self.check_grammar_constructor_type_parameters(program, member);
                self.check_grammar_constructor_type_annotation(program, member);
                if let Some(body) = body {
                    self.check_statement(program, body);
                }
            }
            NodeData::ClassStaticBlockDeclaration(d) => {
                let body = d.body;
                self.check_statement(program, body);
            }
            NodeData::PropertyDeclaration(_) => {
                self.check_property_declaration(program, member);
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
        if !self.is_type_assignable_to(program, initializer_type, declared) {
            let generalized = self.generalized_source_for_error(initializer_type, declared);
            let source_str = super::nodebuilder::type_to_string(self, program, generalized);
            let target_str = super::nodebuilder::type_to_string(self, program, declared);
            self.error(
                program,
                node,
                &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
                &[source_str.as_str(), target_str.as_str()],
            );
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
        let (name, initializer) = match program.arena().data(node) {
            NodeData::VariableDeclaration(d) => (d.name, d.initializer),
            _ => return,
        };
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
        let declared = get_type_of_symbol(self, program, symbol, globals);
        let initializer_type = self.check_expression(program, initializer);
        if !self.is_type_assignable_to(program, initializer_type, declared) {
            let generalized = self.generalized_source_for_error(initializer_type, declared);
            let source_str = super::nodebuilder::type_to_string(self, program, generalized);
            let target_str = super::nodebuilder::type_to_string(self, program, declared);
            self.error(
                program,
                node,
                &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
                &[source_str.as_str(), target_str.as_str()],
            );
        }
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
    // Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType
    fn check_iterated_type_or_element_type(
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
    fn generalized_source_for_error(&self, source: TypeId, target: TypeId) -> TypeId {
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
    fn is_literal_type(&self, t: TypeId) -> bool {
        let f = self.get_type(t).flags();
        f.intersects(TypeFlags::BOOLEAN) || f.intersects(TypeFlags::UNIT)
    }

    // Reports whether `t` could contain top-level singleton (unit) types in a
    // way meaningful to error reporting (Go's `typeCouldHaveTopLevelSingletonTypes`,
    // 4m subset). `boolean` is excluded by design; unit/template-literal/
    // string-mapping types qualify.
    //
    // DEFER(phase-4-checker-4m+): union/intersection members and instantiable
    // constraints. blocked-by: constraint resolution + union iteration here.
    // Go: internal/checker/relater.go:Checker.typeCouldHaveTopLevelSingletonTypes(1302)
    fn type_could_have_top_level_singleton_types(&self, t: TypeId) -> bool {
        let f = self.get_type(t).flags();
        if f.intersects(TypeFlags::BOOLEAN) {
            return false;
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
    fn get_base_type_of_literal_type(&self, t: TypeId) -> TypeId {
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
        if f.intersects(TypeFlags::BOOLEAN_LITERAL) {
            return self.boolean_type;
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

    // Builds (without recording) a diagnostic at `node` from `message` with
    // `args` substituted; its related-information list starts empty (Go's
    // `createDiagnosticForNode`). Callers attach related entries via
    // `Diagnostic::add_related_info` before recording with `add_diagnostic`.
    // Go: internal/checker/checker.go:createDiagnosticForNode(14148)
    fn diagnostic_for_node(
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
        }
    }

    // Records an already-built diagnostic into the per-file collection, keyed by
    // the file `program` is a view of (Go's `c.diagnostics.Add`).
    fn add_diagnostic(&mut self, program: &dyn BoundProgram, diagnostic: Diagnostic) {
        self.diagnostics_by_file
            .entry(program.file_handle())
            .or_default()
            .push(diagnostic);
    }
}

// Returns the node an argument-count error should be reported on (Go's
// `getErrorNodeForCallNode`): for a call expression, the callee, narrowed to
// the member name when the callee is a property access.
// Go: internal/checker/checker.go:getErrorNodeForCallNode(9806)
fn call_error_node(program: &dyn BoundProgram, node: NodeId) -> NodeId {
    let callee = match program.arena().data(node) {
        NodeData::CallExpression(d) => d.expression,
        _ => return node,
    };
    match program.arena().data(callee) {
        NodeData::PropertyAccessExpression(d) => d.name,
        _ => callee,
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

// Reports whether `operator` is a compound assignment operator (`+=`/`*=`/`&&=`/
// ...), i.e. an assignment operator other than plain `=` (Go's
// `KindFirstCompoundAssignment ..= KindLastCompoundAssignment`).
// Go: internal/ast/ast.go:IsCompoundAssignment
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

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
