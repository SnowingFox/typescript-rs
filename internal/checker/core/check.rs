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

use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_COMPUTED;
use tsgo_ast::{Kind, NodeData, NodeId, SymbolFlags, SymbolId, SymbolTable};
use tsgo_core::compileroptions::ScriptTarget;
use tsgo_diagnostics::{Category, Message};

use super::contextual::ContextFlags;
use super::declared_types::{
    fill_missing_type_arguments, get_apparent_type, get_constraint_of_type_parameter,
    get_declared_type_of_symbol, get_indexed_access_type, get_min_type_argument_count,
    get_property_of_type, get_type_from_type_node, get_type_of_property_of_type,
    get_type_of_symbol,
};
use super::inference::{InferenceContext, InferencePriority};
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::relations::RelationKind;
use super::signatures::{IndexInfo, IndexInfoId, Signature, SignatureFlags, SignatureId};
use super::symbols::resolve_name;
use super::type_facts::TypeFacts;
use super::types::{LiteralValue, ObjectFlags, ObjectType, TypeFlags, TypeId};
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
            Kind::TrueKeyword => self.true_type,
            Kind::FalseKeyword => self.false_type,
            Kind::NullKeyword => self.null_type,
            Kind::ThisKeyword => self.check_this_expression(program, node),
            Kind::PropertyAccessExpression => self.check_property_access(program, node),
            Kind::ElementAccessExpression => self.check_element_access(program, node),
            Kind::CallExpression => self.check_call_expression(program, node),
            Kind::NewExpression => self.check_new_expression(program, node),
            Kind::BinaryExpression => self.check_binary_expression(program, node),
            Kind::JsxSelfClosingElement => self.check_jsx_self_closing_element(program, node),
            Kind::JsxElement => self.check_jsx_element(program, node),
            Kind::JsxFragment => self.check_jsx_fragment(program, node),
            Kind::ObjectLiteralExpression => self.check_object_literal(program, node),
            Kind::ArrayLiteralExpression => self.check_array_literal(program, node),
            Kind::FunctionExpression => self.check_function_expression(program, node),
            Kind::ArrowFunction => self.check_arrow_function(program, node),
            Kind::NonNullExpression => self.check_non_null_assertion(program, node),
            Kind::AsExpression => self.check_assertion(program, node),
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
    // DEFER(phase-4-checker-4be+): the `<T>expr` type-assertion form
    // (`TypeAssertionExpression`), the `isValidConstAssertionArgument` diagnostic
    // for an invalid `as const` argument, the deferred `2352` comparability check
    // (`checkAssertionDeferred` / `checkSourceElement` on the type node), and the
    // `erasableSyntaxOnly` grammar diagnostic. blocked-by: assertion
    // comparability (`isTypeComparableTo`) + deferred-node checking +
    // erasable-syntax option.
    // Go: internal/checker/checker.go:Checker.checkAssertion(12238)
    fn check_assertion(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, type_node) = match program.arena().data(node) {
            NodeData::AsExpression(d) => (d.expression, d.type_node),
            _ => return self.error_type,
        };
        let expr_type = self.check_expression(program, expr);
        if is_const_type_reference(program.arena(), type_node) {
            return self.regular_type_of_literal_type(expr_type);
        }
        let globals = program.globals();
        super::declared_types::get_type_from_type_node(self, program, type_node, globals)
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
    // DEFER(phase-4-checker-4bi+): spread members (`{...o}`), get/set/method
    // members, contextual typing (the type flowing INTO the literal), the
    // late-bound *named* member for a string/number-literal or unique-symbol
    // computed name (`isTypeUsableAsPropertyName` -> `getPropertyNameFromType`),
    // and the destructuring-pattern member optionality. blocked-by: spread-type
    // merge (`getSpreadType`), accessor/method signature collection, contextual
    // type propagation, late binding, and destructuring-assignment typing.
    // Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076)
    fn check_object_literal(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let members_nodes = match program.arena().data(node) {
            NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
            _ => return self.error_type,
        };
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
        for member_decl in members_nodes {
            // DEFER(phase-4-checker-4bh+): spread (`{...o}`), accessor
            // (`get`/`set`), and method (`m(){}`) members are skipped; only
            // property assignments and shorthand properties are typed.
            let name_node = match program.arena().data(member_decl) {
                NodeData::PropertyAssignment(d) => d.name,
                NodeData::ShorthandPropertyAssignment(d) => d.name,
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
            // referenced identifier (Go's `checkObjectLiteral` switch).
            let member_type = match program.arena().kind(member_decl) {
                Kind::PropertyAssignment => self.check_property_assignment(program, member_decl),
                Kind::ShorthandPropertyAssignment => {
                    self.check_shorthand_property_assignment(program, member_decl)
                }
                _ => continue,
            };
            // A non-literal computed name assignable to `string | number |
            // symbol` contributes to an index signature of the matching key
            // kind, not a named property (Go's `hasComputed*Property` block).
            // DEFER(phase-4-checker-4bh+): a string/number-literal or
            // unique-symbol computed name becomes a late-bound NAMED member
            // (`isTypeUsableAsPropertyName`); the reachable subset skips it.
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
                // Literal/unique computed name -> late-bound named member: DEFER.
                continue;
            }
            let Some(name) = property_name_text(program, name_node) else {
                continue;
            };
            // Go: `newSymbolEx(SymbolFlagsProperty | member.Flags, member.Name, checkFlags)`
            // then `links.resolvedType = t`. Object-literal properties are never
            // optional in the reachable subset, so only `Property` is carried;
            // `member_check_flags` carries `Readonly` in a const context.
            let prop = self.new_object_literal_property(
                &name,
                SymbolFlags::PROPERTY,
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
        let mut index_infos: Vec<IndexInfoId> = Vec::new();
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
    fn check_computed_property_name(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
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
    // typing, and property suggestions.
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
        // Iterate the literal's own properties in declaration order (Go's
        // `getPropertiesOfType(source)`); every object-literal member is declared
        // directly in the literal, so Go's `shouldCheckAsExcessProperty` holds.
        let properties = match self.get_type(source).as_object() {
            Some(obj) => obj.properties.clone(),
            None => return false,
        };
        for prop in properties {
            let name = self.property_symbol_name(program, prop);
            if !self.is_known_property(program, target, &name) {
                let error_node = object_literal_property_name_node(program, literal_node, &name)
                    .unwrap_or(literal_node);
                let target_str = super::nodebuilder::type_to_string(self, program, target);
                self.error(
                    program,
                    error_node,
                    &tsgo_diagnostics::OBJECT_LITERAL_MAY_ONLY_SPECIFY_KNOWN_PROPERTIES_AND_0_DOES_NOT_EXIST_IN_TYPE_1,
                    &[name.as_str(), target_str.as_str()],
                );
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
    // DEFER(phase-4-checker-4bi+): spread (`[...a]`) and omitted elements, the
    // non-const tuple contexts (`forceTuple` / a tuple-like contextual type),
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
        // DEFER(phase-4-checker-4bg+): spread/omitted elements are typed through
        // the plain mutable-location path here, which yields the `error` type
        // for a `SpreadElement`/`OmittedExpression` (no clean element type),
        // degrading the array element type. blocked-by: spread/iterator typing.
        let element_types: Vec<TypeId> = elements
            .iter()
            .map(|&element| self.check_expression_for_mutable_location(program, element))
            .collect();
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

    // Checks an expression used as the object of a property/element access (Go's
    // `checkNonNullExpression`): types the expression, then runs the
    // possibly-`null`/`undefined` check on `node`.
    // Go: internal/checker/checker.go:Checker.checkNonNullExpression(7373)
    fn check_non_null_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let t = self.check_expression(program, node);
        self.check_non_null_type(program, t, node)
    }

    // Reports the possibly-`null`/`undefined` error for a property/element access
    // object and narrows to the non-null type (Go's `checkNonNullType`, which
    // defaults to the `reportObjectPossiblyNullOrUndefinedError` reporter).
    // Go: internal/checker/checker.go:Checker.checkNonNullType(7377)
    fn check_non_null_type(
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
        let name = program.arena().text(node).to_string();
        // Go's `resolveName` always consults the outermost `c.globals` scope, so
        // a bare identifier referencing a global VALUE (a lib global like
        // `Error`/`Object`/`Date`, or a cross-file global declaration) resolves.
        // Passing `None` here previously dropped the globals scope, cascading
        // every global-value reference into a spurious 2304 (and a follow-on
        // 2339 on its `error`-typed members).
        let globals = program.globals();
        match resolve_name(program, node, &name, SymbolFlags::VALUE, false, globals) {
            None => {
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
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0,
                    &[name.as_str()],
                );
                self.error_type
            }
            Some(symbol) => {
                let declared = get_type_of_symbol(self, program, symbol, globals);
                self.get_flow_type_of_reference(program, node, declared)
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
    // argument applicability, constructor accessibility (private/protected,
    // 2673/2674), `new` on a non-constructable value (2351), type-argument
    // instantiation, and the construct-signature-level `abstract` flag path.
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
        // Type the callee and arguments so nested diagnostics surface.
        let _ = self.check_expression(program, callee);
        for &arg in &args {
            self.check_expression(program, arg);
        }
        let Some(class_symbol) = self.new_expression_class_symbol(program, callee) else {
            // DEFER: `new` on a non-class value (construct signatures / 2351).
            return self.error_type;
        };
        // Constructing an abstract class is an error (2511). Go checks the
        // chosen construct signature's `abstract` flag; the reachable subset
        // reads the `abstract` modifier off the class declaration directly.
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
        let globals = program.globals();
        get_declared_type_of_symbol(self, program, class_symbol, globals)
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
        // Go's `checkIndexedAccess` types the object via `checkNonNullExpression`
        // (reports possibly-`null`/`undefined`, narrows to the non-null type).
        let object_type = self.check_non_null_expression(program, expr);
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
                    self.check_assignment_operator(program, left, left_type, result_type, None);
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
            // DEFER(phase-4-checker-later): the `checkNullishCoalesceOperands`
            // always-/never-nullish operand diagnostics (`This_expression_is_
            // always_nullish` / `Right_operand_..._never_nullish`). blocked-by:
            // the syntactic nullishness-semantics analysis. (The mixed-operator
            // `5076` grammar check is wired separately below.)
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
        expr: Option<NodeId>,
    ) {
        // A reference target is an identifier or an access expression (Go's
        // `checkReferenceExpression`); other targets are skipped here.
        if !is_reference_expression(program, left) {
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
        let (callee, args, type_argument_nodes) = match program.arena().data(node) {
            NodeData::CallExpression(d) => (
                d.expression,
                d.arguments.nodes.clone(),
                d.type_arguments.as_ref().map(|l| l.nodes.clone()),
            ),
            _ => return self.error_type,
        };
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
    fn check_type_arguments(
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
        let return_type = self.signature_return_type(signature);
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
        // Contextually type the callback's parameters from the instantiated
        // parameter type's call signature, then build its function type.
        let ctx_signature = self.get_signatures_of_type(instantiated_param);
        if let Some(&ctx_sig) = ctx_signature.first() {
            self.assign_contextual_parameter_types(program, arg, ctx_sig);
        }
        self.get_type_of_context_sensitive_arrow(program, arg)
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
                    .map(|e| {
                        let t = self.check_expression(program, e);
                        self.get_widened_literal_type(t)
                    })
                    .collect();
                let union = self.get_union_type(&types);
                self.get_widened_type(union)
            }
        } else {
            // A concise body's literal return type is widened to its base
            // (Go's `getReturnTypeFromBody` -> `getWidenedType`): `() => "s"`
            // returns `string`, not `"s"`.
            let t = self.check_expression(program, body);
            let widened = self.get_widened_literal_type(t);
            self.get_widened_type(widened)
        };
        if let Some(diagnostics) = self.diagnostics_by_file.get_mut(&handle) {
            diagnostics.truncate(before);
        }
        result
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
                if let NodeData::TypeOperator(d) = program.arena().data(node) {
                    let operand = d.type_node;
                    self.check_type_node(program, operand);
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
            let name = super::nodebuilder::symbol_to_string(program, symbol);
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
            let type_str = super::nodebuilder::symbol_to_string(program, symbol);
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
    pub(crate) fn get_type_at_position(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        pos: usize,
    ) -> TypeId {
        match self.signature(signature).parameters.get(pos).copied() {
            Some(symbol) => {
                let base = get_type_of_symbol(self, program, symbol, None);
                // For an instantiated signature, the base parameter type is
                // substituted through the signature's mapper (Go re-instantiates
                // the parameter symbols in `instantiateSignature`).
                match self.signature(signature).mapper.clone() {
                    Some(mapper) => self.instantiate_param_type(program, base, &mapper),
                    None => base,
                }
            }
            None => self.any_type,
        }
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
            // Go runs `checkClassLikeDeclaration` (heritage relations: implements
            // satisfaction + extends compatibility) BEFORE iterating the members
            // (`checkSourceElements(node.Members())`).
            self.check_class_like_declaration(program, node);
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
        // A type-alias declaration grammar-checks its type-parameter list (2706)
        // and descends into its aliased type node so a generic reference there
        // has its arity/constraints checked (Go's `checkTypeAliasDeclaration` ->
        // `checkSourceElement(node.Type())`).
        if let NodeData::TypeAliasDeclaration(d) = program.arena().data(node) {
            let (type_parameters, type_node) = (d.type_parameters.clone(), d.type_node);
            self.check_grammar_type_parameter_defaults(program, type_parameters);
            self.check_type_node(program, type_node);
        }
        // An interface declaration grammar-checks its type-parameter list (2706);
        // its member type nodes are not yet descended (DEFER below).
        if let NodeData::InterfaceDeclaration(d) = program.arena().data(node) {
            let type_parameters = d.type_parameters.clone();
            self.check_grammar_type_parameter_defaults(program, type_parameters);
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
        // Assign contextual parameter types to un-annotated parameters before the
        // body is descended into, so a body reference to such a parameter
        // resolves with its contextual type (4bj).
        self.contextually_check_function_expression(program, node);
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
        // Assign contextual parameter types to un-annotated parameters before the
        // body is descended into, so a body reference to such a parameter
        // resolves with its contextual type (4bj).
        self.contextually_check_function_expression(program, node);
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
    // DEFER(phase-4-checker-4bm+): the member-specific elaboration
    // (`issueMemberSpecificError` -> 2416 `Property_0_in_type_1_is_not_assignable_to_the_same_property_in_base_type_2`
    // and the nested 2741/2322 chain), the static-side extends check (2417), the
    // override-modifier walk (`checkKindsOfPropertyMemberOverrides` /
    // `checkMembersForOverrideModifier`), `implements` on a non-object type
    // (2422), the `implements`-a-class hint (2720), base-type accessibility
    // (private constructor, 2654), mixins / type-variable base constructors,
    // generic base classes with type arguments (the `getTypeWithThisArgument`
    // type-argument substitution beyond the monomorphic case), abstract-class
    // instantiation, `super()` requirements, and the index-constraint /
    // property-initialization checks. blocked-by: a diagnostic-producing relation
    // (`checkTypeRelatedToEx` with chains) for the member elaboration + the
    // static/constructor type of a class value symbol + override-modifier
    // resolution + generic type-argument instantiation through `this`.
    // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4266)
    fn check_class_like_declaration(&mut self, program: &dyn BoundProgram, node: NodeId) {
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
        let base_types = self
            .get_type(class_type)
            .as_object()
            .map(|o| o.base_types.clone())
            .unwrap_or_default();
        if let Some(&base_type) = base_types.first() {
            if !self.is_type_assignable_to(program, type_with_this, base_type) {
                let base_str = super::nodebuilder::type_to_string(self, program, base_type);
                self.error(
                    program,
                    error_node,
                    &tsgo_diagnostics::CLASS_0_INCORRECTLY_EXTENDS_BASE_CLASS_1,
                    &[class_str.as_str(), base_str.as_str()],
                );
            }
        }

        // `implements`-clause satisfaction (2420). Each implemented type must be
        // assignable from the class instance type.
        // Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4338)
        for type_node in self.implements_heritage_elements(program, node) {
            let Some(interface_type) = self.resolve_heritage_clause_type(program, type_node) else {
                continue;
            };
            // A type that did not resolve is the error type; skip it (Go's
            // `if !c.isErrorType(t)`).
            if interface_type == self.error_type() {
                continue;
            }
            // Monomorphic `baseWithThis` is the implemented interface type.
            if !self.is_type_assignable_to(program, type_with_this, interface_type) {
                let interface_str =
                    super::nodebuilder::type_to_string(self, program, interface_type);
                self.error(
                    program,
                    error_node,
                    &tsgo_diagnostics::CLASS_0_INCORRECTLY_IMPLEMENTS_INTERFACE_1,
                    &[class_str.as_str(), interface_str.as_str()],
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
        if program.arena().kind(expression) != Kind::Identifier {
            return None;
        }
        let name = program.arena().text(expression).to_string();
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
            // back to the flat head if it somehow holds.
            let generalized = self.generalized_source_for_error(source, target);
            let source_str = super::nodebuilder::type_to_string(self, program, generalized);
            let target_str = super::nodebuilder::type_to_string(self, program, target);
            self.error(
                program,
                node,
                &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
                &[source_str.as_str(), target_str.as_str()],
            );
            return;
        };
        let loc = program.arena().loc(node);
        let diagnostic = Diagnostic {
            code: report.code,
            category: report.category,
            message: report.message,
            start: loc.pos(),
            length: loc.end() - loc.pos(),
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
        if let Some(t) = get_type_of_property_of_type(self, program, obj, name) {
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

// Reports whether `node` was parsed in a JavaScript file (the parser sets
// `NodeFlags::JAVA_SCRIPT_FILE` on every node of a `.js`/`.jsx`/`.json` file).
// Go: internal/ast/utilities.go:IsInJSFile
fn is_in_js_file(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    arena
        .flags(node)
        .contains(tsgo_ast::NodeFlags::JAVA_SCRIPT_FILE)
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

// Returns the modifier flags of `node` (its `modifiers` list union), or empty
// when the node bears no modifier list.
fn modifier_flags_of(arena: &tsgo_ast::NodeArena, node: NodeId) -> tsgo_ast::ModifierFlags {
    let modifiers = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        NodeData::ConstructorDeclaration(d) => d.modifiers.as_ref(),
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
    match program.arena().data(node) {
        NodeData::ArrowFunction(d) => d.type_node,
        NodeData::FunctionExpression(d) => d.type_node,
        _ => None,
    }
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

// Reports whether `name` is a numeric literal name (Go's `isNumericLiteralName`):
// the name is a numeric name iff `ToString(ToNumber(name)) == name`, i.e. the
// JS-number round-trip of its text is exactly the text (so `"0"`/`"1.5"` are
// numeric but `"0xF00D"`/`"01"` are not).
// Go: internal/checker/utilities.go:isNumericLiteralName(860)
fn is_numeric_literal_name(name: &str) -> bool {
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
