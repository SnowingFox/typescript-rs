//! Port of Go `internal/transformers/destructuring.go`: the destructuring
//! flattener. This round (6j) ports the **binding-pattern** flattener
//! [`flatten_destructuring_binding`] (the down-level decomposition of
//! `var`/`let`/`const` binding patterns into individual variable declarations),
//! reachable without the checker / resolver (a pure AST transform).
//!
//! # Scope (round 6j)
//!
//! `flatten_destructuring_binding` at [`FlattenLevel::All`] decomposes array and
//! object binding patterns in a variable declaration:
//!
//! - array binding `var [a, b] = arr;` -> `var a = arr[0], b = arr[1];`
//! - object binding `var { a, b } = o;` -> `var a = o.a, b = o.b;`
//! - defaults `var [a = 1] = arr;` -> `var _a = arr[0], a = _a === void 0 ? 1 : _a;`
//! - nested patterns `var [[a]] = x;` -> `var a = x[0][0];`
//! - array rest `var [a, ...r] = arr;` -> `var a = arr[0], r = arr.slice(1);`
//! - computed keys `var { [k]: a } = o;` -> `var _a = k, a = o[_a];`
//!
//! The shared utility is exercised through [`new_destructuring_transformer`], a
//! thin driver that mirrors how TypeScript's ES2015 transformer lowers
//! variable-declaration binding patterns (the Go ES2015 transformer is not
//! ported as its own file in this repo; this driver stands in for its
//! variable-declaration lowering so the flattener can be tested behaviorally
//! through a public entry).
//!
//! Deferred (DEFER): the assignment-target flattener
//! `FlattenDestructuringAssignment` (assignment mode); object-**rest** binding
//! (`var { a, ...rest } = o;`) is already covered by the round-6g
//! `objectrestspread` `__rest` path and is left to that transform (the driver
//! skips declarations containing object rest); parameter / `for-of` / `catch`
//! destructuring positions (need the parameter / for-of wiring); the exported
//! (`hoistTempVariables = true`) variable-statement path.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, TokenFlags, VisitOptions};
use tsgo_printer::EmitContext;

/// Controls how deeply binding/assignment patterns are decomposed.
///
/// Mirrors Go's `FlattenLevel` iota (`All = 0`, `ObjectRest = 1`); the ordering
/// is significant (`level < ObjectRest` gates several branches).
///
/// # Examples
/// ```
/// use tsgo_transformers::destructuring::FlattenLevel;
/// assert!(FlattenLevel::All < FlattenLevel::ObjectRest);
/// ```
///
/// Side effects: none (a plain enum).
// Go: internal/transformers/destructuring.go:FlattenLevel
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlattenLevel {
    /// Fully decompose all patterns into individual bindings.
    All,
    /// Only decompose patterns containing object rest elements.
    ObjectRest,
}

/// Builds a [`Transformer`] that lowers variable-declaration binding patterns to
/// individual variable declarations, sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{destructuring::new_destructuring_transformer, TransformOptions};
/// let _tx = new_destructuring_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/destructuring.go:FlattenDestructuringBinding (ES2015 variable-declaration lowering)
pub fn new_destructuring_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| destructuring_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit: opens a variable environment at the source file
/// (reusing the round-6i infrastructure) and lowers variable-statement binding
/// patterns; other nodes recurse arena-only.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations / attach
/// emit helpers.
// Go: internal/transformers/estransforms (es2015 transformer dispatch)
fn destructuring_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::VariableStatement => try_lower_variable_statement(ec, node)
            .unwrap_or_else(|| arena_identity(ec.arena_mut(), node)),
        Kind::ExpressionStatement => try_lower_expression_statement(ec, node)
            .unwrap_or_else(|| arena_identity(ec.arena_mut(), node)),
        _ => {
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut()
                .visit_each_child(node, opts, &mut |a, c| arena_identity(a, c))
        }
    }
}

/// Arena-only descent placeholder used for subtrees not on the threaded path.
///
/// Side effects: may push rebuilt nodes onto the arena.
fn arena_identity(arena: &mut NodeArena, node: NodeId) -> NodeId {
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| arena_identity(a, c))
}

/// Wraps the source file's statements in a variable environment so hoisted
/// temporaries land in a leading `var ...;`, then attaches any requested emit
/// helpers so the printer emits them in the prologue.
///
/// Side effects: pushes/pops a variable environment; rebuilds the source file;
/// attaches emit helpers.
// Go: internal/printer/emitcontext.go:EmitContext.VisitVariableEnvironment (top-level statements)
fn visit_source_file(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (file_name, script_kind, language_variant, statements, end_of_file_token) =
        match ec.arena().data(node) {
            NodeData::SourceFile(d) => (
                d.file_name.clone(),
                d.script_kind,
                d.language_variant,
                d.statements.clone(),
                d.end_of_file_token,
            ),
            _ => unreachable!("kind/data mismatch"),
        };
    ec.start_variable_environment();
    let mut visited = Vec::with_capacity(statements.nodes.len());
    for &statement in &statements.nodes {
        visited.push(destructuring_visit(ec, statement));
    }
    let mut all = ec.end_variable_environment();
    all.extend(visited);
    let new_source_file = ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(all),
        end_of_file_token,
    );
    for helper in ec.read_emit_helpers() {
        ec.add_emit_helper(new_source_file, helper);
    }
    new_source_file
}

/// Rebuilds a variable statement, flattening each declaration whose name is a
/// binding pattern (array or object) into individual variable declarations.
/// Returns `None` when no declaration is a flattenable binding pattern (so the
/// caller falls back to an arena-only descent).
///
/// Object-**rest** patterns (`{ ..., ...rest }`) are left to the round-6g
/// `objectrestspread` transform (shared `__rest`), so declarations containing an
/// object rest are skipped here.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: TypeScript es2015.ts:visitVariableStatement / visitVariableDeclaration
fn try_lower_variable_statement(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let (modifiers, list) = match ec.arena().data(node) {
        NodeData::VariableStatement(d) => (d.modifiers.clone(), d.declaration_list),
        _ => return None,
    };
    let declarations = match ec.arena().data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.clone(),
        _ => return None,
    };
    let flattenable = declarations.nodes.iter().any(|&d| {
        declaration_name_is_binding_pattern(ec.arena(), d)
            && !declaration_has_object_rest(ec.arena(), d)
    });
    if !flattenable {
        return None;
    }
    let mut new_decls: Vec<NodeId> = Vec::new();
    for &decl in &declarations.nodes {
        if declaration_name_is_binding_pattern(ec.arena(), decl)
            && !declaration_has_object_rest(ec.arena(), decl)
        {
            if let Some(result) =
                flatten_destructuring_binding(ec, decl, None, FlattenLevel::All, false, false)
            {
                expand_into(ec, result, &mut new_decls);
            }
        } else {
            new_decls.push(arena_identity(ec.arena_mut(), decl));
        }
    }
    let block_scoped = ec.arena().flags(list) & NodeFlags::BLOCK_SCOPED;
    let new_list = ec
        .arena_mut()
        .new_variable_declaration_list(NodeList::new(new_decls));
    ec.arena_mut().add_flags(new_list, block_scoped);
    Some(ec.arena_mut().new_variable_statement(modifiers, new_list))
}

/// Appends the flattener's result to `out`, expanding a `SyntaxList` into its
/// child declarations (the printer does not flatten a `SyntaxList` nested inside
/// a declaration list, mirroring the round-6g adaptation).
///
/// Side effects: none beyond pushing onto `out`.
fn expand_into(ec: &EmitContext, result: NodeId, out: &mut Vec<NodeId>) {
    match ec.arena().data(result) {
        NodeData::SyntaxList(d) => out.extend(d.list.nodes.iter().copied()),
        _ => out.push(result),
    }
}

/// Lowers an expression statement whose expression is a destructuring
/// **assignment** (`[a, b] = arr;` / `({ a, b } = o);`) into a comma sequence of
/// individual assignments. Returns `None` when the statement is not a
/// destructuring assignment (so the caller falls back to an arena-only descent).
///
/// The statement context means the assignment's value is unused
/// (`needs_value = false`); the value-returning path is deferred (see module
/// docs). This mirrors how Go's ES2015 transformer lowers a destructuring
/// assignment found in a discarded expression statement.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/objectrestspread.go:visitBinaryExpression (assignment arm)
fn try_lower_expression_statement(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let expression = match ec.arena().data(node) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => return None,
    };
    let inner = skip_parentheses(ec.arena(), expression);
    if !is_destructuring_assignment(ec.arena(), inner) {
        return None;
    }
    let lowered = flatten_destructuring_assignment(ec, inner, false, FlattenLevel::All);
    Some(ec.arena_mut().new_expression_statement(lowered))
}

/// Flattens a binding pattern in a variable declaration into individual variable
/// declarations. Returns a single `VariableDeclaration`, a `SyntaxList` of
/// declarations, or `None`.
///
/// # Examples
/// ```
/// // Behavior is exercised through `new_destructuring_transformer`; see the
/// // crate's destructuring tests for `var [a, b] = arr;` -> `var a = arr[0], ...`.
/// use tsgo_transformers::destructuring::FlattenLevel;
/// assert_eq!(FlattenLevel::All, FlattenLevel::All);
/// ```
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/destructuring.go:FlattenDestructuringBinding / flattenDestructuringBinding
pub fn flatten_destructuring_binding(
    ec: &mut EmitContext,
    node: NodeId,
    rval: Option<NodeId>,
    level: FlattenLevel,
    hoist_temp_variables: bool,
    skip_initializer: bool,
) -> Option<NodeId> {
    let mut f = Flattener {
        ec,
        level,
        mode: FlattenMode::Binding,
        hoist_temp_variables,
        expressions: Vec::new(),
        declarations: Vec::new(),
    };
    f.flatten_destructuring_binding(node, rval, skip_initializer)
}

/// Flattens a destructuring **assignment** (`[a, b] = arr` / `({ a, b } = o)`)
/// into a comma sequence of individual property/element-access assignments,
/// reusing the same flattener machinery as [`flatten_destructuring_binding`].
///
/// `needs_value` reports whether the overall expression's value is consumed (the
/// value-position path is deferred — see module docs); at statement level it is
/// `false`. `level` selects how deeply patterns decompose ([`FlattenLevel::All`]
/// for the full ES5 lowering, [`FlattenLevel::ObjectRest`] for the
/// object-rest-only lowering wired through `objectrestspread`).
///
/// # Examples
/// ```
/// // Behavior is exercised through `new_destructuring_transformer`; see the
/// // crate's destructuring tests for `[a, b] = arr;` -> `a = arr[0], b = arr[1];`.
/// use tsgo_transformers::destructuring::FlattenLevel;
/// assert!(FlattenLevel::All <= FlattenLevel::ObjectRest);
/// ```
///
/// Side effects: may push rebuilt nodes; hoists `var` declarations for temps.
// Go: internal/transformers/destructuring.go:FlattenDestructuringAssignment / flattenDestructuringAssignment
pub fn flatten_destructuring_assignment(
    ec: &mut EmitContext,
    node: NodeId,
    needs_value: bool,
    level: FlattenLevel,
) -> NodeId {
    // Assignment mode always hoists temps (Go sets `hoistTempVariables = true`),
    // so captured values land in a leading `var ...;` (provided by the driver's
    // / objectrestspread's variable environment) rather than same-statement
    // declarations.
    let mut f = Flattener {
        ec,
        level,
        mode: FlattenMode::Assignment,
        hoist_temp_variables: true,
        expressions: Vec::new(),
        declarations: Vec::new(),
    };
    f.flatten_destructuring_assignment(node, needs_value)
}

/// A pending variable declaration accumulated during binding flattening.
// Go: internal/transformers/destructuring.go:pendingDecl
struct PendingDecl {
    name: NodeId,
    value: NodeId,
    pending_expressions: Vec<NodeId>,
}

/// Selects the flattener's output mode. Go switches behavior through a set of
/// function-pointer callbacks (`emitBindingOrAssignment`,
/// `createArrayBindingOrAssignmentPattern`, ...); the Rust port carries a single
/// discriminant and branches at each callback site instead.
// Go: internal/transformers/destructuring.go:FlattenDestructuringBinding / FlattenDestructuringAssignment (callback setup)
#[derive(Clone, Copy, PartialEq, Eq)]
enum FlattenMode {
    /// Emit individual `VariableDeclaration`s (`var a = ...`).
    Binding,
    /// Emit individual assignment expressions (`a = ...`) folded into a comma.
    Assignment,
}

/// Encapsulates the state and logic for flattening a binding pattern or an
/// assignment target. Mirrors the reachable subset of Go's `flattener` (the
/// `FlattenContext`).
struct Flattener<'a> {
    ec: &'a mut EmitContext,
    level: FlattenLevel,
    mode: FlattenMode,
    hoist_temp_variables: bool,
    expressions: Vec<NodeId>,
    declarations: Vec<PendingDecl>,
}

impl Flattener<'_> {
    /// Recursively applies the destructuring transform to a sub-node (the
    /// equivalent of Go's `tx.Visitor().VisitNode`).
    fn visit(&mut self, node: NodeId) -> NodeId {
        destructuring_visit(self.ec, node)
    }

    // Go: internal/transformers/destructuring.go:flattenDestructuringAssignment
    fn flatten_destructuring_assignment(&mut self, mut node: NodeId, needs_value: bool) -> NodeId {
        let mut value = None;
        if is_destructuring_assignment(self.ec.arena(), node) {
            value = Some(self.binary_right(node));
            // Peel `[] = x` / `{} = x` wrappers: an empty pattern only matters
            // for its side effects, so descend into the value when it is itself
            // a destructuring assignment, else just visit the value.
            while {
                let left = self.binary_left(node);
                is_empty_array_literal(self.ec.arena(), left)
                    || is_empty_object_literal(self.ec.arena(), left)
            } {
                let v = value.expect("destructuring assignment has a value");
                if is_destructuring_assignment(self.ec.arena(), v) {
                    node = v;
                    value = Some(self.binary_right(node));
                } else {
                    return self.visit(v);
                }
            }
        }

        if let Some(v) = value {
            let mut v = self.visit(v);
            let assigns_to_name = self.ec.arena().kind(v) == Kind::Identifier && {
                let name = self.ec.arena().text(v).to_string();
                self.binding_assigns_to_name(node, &name)
            };
            if assigns_to_name || self.contains_non_literal_computed_name(node) {
                v = self.ensure_identifier(v, false);
            } else if needs_value {
                v = self.ensure_identifier(v, true);
            }
            // Go's synthesized-node `location = value.Loc` branch only adjusts
            // source-map ranges, which the port does not track.
            value = Some(v);
        }

        let skip_initializer = is_destructuring_assignment(self.ec.arena(), node);
        self.flatten_binding_or_assignment_element(node, value, skip_initializer);

        if let Some(v) = value {
            if needs_value {
                if self.expressions.is_empty() {
                    return v;
                }
                self.expressions.push(v);
            }
        }

        self.build_assignment_result()
    }

    // Go: internal/transformers/destructuring.go:flattenDestructuringBinding
    fn flatten_destructuring_binding(
        &mut self,
        node: NodeId,
        rval: Option<NodeId>,
        skip_initializer: bool,
    ) -> Option<NodeId> {
        let mut node = node;
        if self.ec.arena().kind(node) == Kind::VariableDeclaration {
            if let Some(initializer) = self.get_initializer_of_element(node) {
                let assigns_to_name = self.ec.arena().kind(initializer) == Kind::Identifier && {
                    let name = self.ec.arena().text(initializer).to_string();
                    self.binding_assigns_to_name(node, &name)
                };
                if assigns_to_name || self.contains_non_literal_computed_name(node) {
                    // The initializer is referenced more than once (or evaluation
                    // order around a computed key must be preserved), so capture
                    // it in a temp before the pattern reads from it.
                    let visited = self.visit(initializer);
                    let new_init = self.ensure_identifier(visited, false);
                    node = self.update_variable_declaration_initializer(node, new_init);
                }
            }
        }
        self.flatten_binding_or_assignment_element(node, rval, skip_initializer);
        self.build_declarations()
    }

    /// Rebuilds a variable declaration with a new initializer (dropping its type
    /// annotation / `!` token, mirroring Go's `UpdateVariableDeclaration` call).
    // Go: internal/transformers/destructuring.go:flattenDestructuringBinding (UpdateVariableDeclaration)
    fn update_variable_declaration_initializer(
        &mut self,
        node: NodeId,
        initializer: NodeId,
    ) -> NodeId {
        let name = match self.ec.arena().data(node) {
            NodeData::VariableDeclaration(d) => d.name,
            _ => unreachable!("kind/data mismatch"),
        };
        self.ec
            .arena_mut()
            .new_variable_declaration(name, None, None, Some(initializer))
    }

    /// Reports whether any binding target in `element` assigns to `name`.
    // Go: internal/transformers/destructuring.go:BindingOrAssignmentElementAssignsToName
    fn binding_assigns_to_name(&self, element: NodeId, name: &str) -> bool {
        let target = match self.get_target_of_element(element) {
            Some(t) => t,
            None => return false,
        };
        let kind = self.ec.arena().kind(target);
        if is_binding_pattern(kind) {
            self.get_elements_of_pattern(target)
                .iter()
                .any(|&e| self.binding_assigns_to_name(e, name))
        } else if kind == Kind::Identifier {
            self.ec.arena().text(target) == name
        } else {
            false
        }
    }

    /// Reports whether any element of `element` has a non-literal computed
    /// property name.
    // Go: internal/transformers/destructuring.go:BindingOrAssignmentElementContainsNonLiteralComputedName
    fn contains_non_literal_computed_name(&self, element: NodeId) -> bool {
        if let Some(property_name) = self.try_get_property_name(element) {
            if self.ec.arena().kind(property_name) == Kind::ComputedPropertyName {
                let expr = match self.ec.arena().data(property_name) {
                    NodeData::ComputedPropertyName(d) => d.expression,
                    _ => unreachable!("kind/data mismatch"),
                };
                if !is_literal_expression(self.ec.arena().kind(expr)) {
                    return true;
                }
            }
        }
        match self.get_target_of_element(element) {
            Some(target) if is_binding_pattern(self.ec.arena().kind(target)) => self
                .get_elements_of_pattern(target)
                .iter()
                .any(|&e| self.contains_non_literal_computed_name(e)),
            _ => false,
        }
    }

    /// Builds the resulting declaration node(s) from the accumulated pending
    /// declarations: a single declaration, a `SyntaxList`, or `None`.
    fn build_declarations(&mut self) -> Option<NodeId> {
        let pending = std::mem::take(&mut self.declarations);
        let mut decls = Vec::with_capacity(pending.len());
        for p in pending {
            let expr = p.value;
            debug_assert!(
                p.pending_expressions.is_empty(),
                "pending expressions are only produced by the hoist path (DEFER)"
            );
            let decl = self
                .ec
                .arena_mut()
                .new_variable_declaration(p.name, None, None, Some(expr));
            decls.push(decl);
        }
        match decls.len() {
            0 => None,
            1 => Some(decls[0]),
            _ => Some(self.ec.arena_mut().new_syntax_list(NodeList::new(decls))),
        }
    }

    /// Folds the accumulated assignment expressions into a comma sequence,
    /// returning an `OmittedExpression` when none were produced (mirroring Go's
    /// `InlineExpressions(...)` then `NewOmittedExpression()` fallback).
    // Go: internal/printer/factory.go:NodeFactory.InlineExpressions
    fn build_assignment_result(&mut self) -> NodeId {
        let exprs = std::mem::take(&mut self.expressions);
        match self.inline_expressions(exprs) {
            Some(expr) => expr,
            None => self.ec.arena_mut().new_omitted_expression(),
        }
    }

    /// Reduces a list of expressions left-to-right with the comma operator.
    /// Returns `None` for an empty list and the single element unchanged for a
    /// length-1 list (matching Go's `InlineExpressions`). The `> 10` →
    /// `CommaListExpression` shortcut is not reachable here (DEFER).
    // Go: internal/printer/factory.go:NodeFactory.InlineExpressions
    fn inline_expressions(&mut self, exprs: Vec<NodeId>) -> Option<NodeId> {
        let mut iter = exprs.into_iter();
        let mut acc = iter.next()?;
        for next in iter {
            acc = self.new_comma(acc, next);
        }
        Some(acc)
    }

    /// Builds a comma expression `left, right`.
    // Go: internal/printer/factory.go:NodeFactory.NewCommaExpression
    fn new_comma(&mut self, left: NodeId, right: NodeId) -> NodeId {
        let comma = self.ec.arena_mut().new_token(Kind::CommaToken);
        self.ec
            .arena_mut()
            .new_binary_expression(left, comma, right)
    }

    /// Reads the left operand of a binary expression.
    fn binary_left(&self, node: NodeId) -> NodeId {
        match self.ec.arena().data(node) {
            NodeData::BinaryExpression(d) => d.left,
            _ => unreachable!("expected a binary expression"),
        }
    }

    /// Reads the right operand of a binary expression.
    fn binary_right(&self, node: NodeId) -> NodeId {
        match self.ec.arena().data(node) {
            NodeData::BinaryExpression(d) => d.right,
            _ => unreachable!("expected a binary expression"),
        }
    }

    // Go: internal/transformers/destructuring.go:flattenBindingOrAssignmentElement
    fn flatten_binding_or_assignment_element(
        &mut self,
        element: NodeId,
        value: Option<NodeId>,
        skip_initializer: bool,
    ) {
        let binding_target = match self.get_target_of_element(element) {
            Some(t) => t,
            None => return,
        };
        let mut value = value;
        if !skip_initializer {
            let initializer = self
                .get_initializer_of_element(element)
                .map(|i| self.visit(i));
            match (initializer, value) {
                (Some(init), Some(v)) => {
                    let mut new_value = self.create_default_value_check(v, init);
                    if !is_simple_copiable(self.ec.arena().kind(init))
                        && is_binding_pattern(self.ec.arena().kind(binding_target))
                    {
                        new_value = self.ensure_identifier(new_value, true);
                    }
                    value = Some(new_value);
                }
                (Some(init), None) => value = Some(init),
                (None, None) => value = Some(self.make_void_zero()),
                (None, Some(_)) => {}
            }
        }
        let bt_kind = self.ec.arena().kind(binding_target);
        let value = value.expect("binding target requires a value");
        if is_object_binding_pattern(bt_kind) {
            self.flatten_object_binding_or_assignment_pattern(element, binding_target, value);
        } else if is_array_binding_pattern(bt_kind) {
            self.flatten_array_binding_or_assignment_pattern(element, binding_target, value);
        } else {
            self.emit_binding_or_assignment(binding_target, value);
        }
    }

    /// Dispatches to the mode-appropriate leaf emitter: a pending
    /// `VariableDeclaration` (binding) or an assignment expression (assignment).
    // Go: internal/transformers/destructuring.go:emitBinding / emitAssignment (via emitBindingOrAssignment)
    fn emit_binding_or_assignment(&mut self, target: NodeId, value: NodeId) {
        match self.mode {
            FlattenMode::Binding => self.emit_binding(target, value),
            FlattenMode::Assignment => self.emit_assignment(target, value),
        }
    }

    /// Emits an assignment expression `target = value` (visiting the target so a
    /// nested kept pattern is itself lowered) onto the expression list.
    /// The `createAssignmentCallback` path (CJS export / namespace member
    /// assignment) is not wired in this round (DEFER).
    // Go: internal/transformers/destructuring.go:emitAssignment
    fn emit_assignment(&mut self, target: NodeId, value: NodeId) {
        let visited_target = self.visit(target);
        let assignment = self.new_assignment(visited_target, value);
        self.expressions.push(assignment);
    }

    // Go: internal/transformers/destructuring.go:flattenArrayBindingOrAssignmentPattern
    fn flatten_array_binding_or_assignment_pattern(
        &mut self,
        parent: NodeId,
        pattern: NodeId,
        value: NodeId,
    ) {
        let elements = self.get_elements_of_pattern(pattern);
        let num = elements.len();
        let mut value = value;
        let all_omitted = elements
            .iter()
            .all(|&e| self.ec.arena().kind(e) == Kind::OmittedExpression);
        if (num != 1 && (self.level < FlattenLevel::ObjectRest || num == 0)) || all_omitted {
            let reuse = !is_declaration_binding_element(self.ec.arena().kind(parent)) || num != 0;
            value = self.ensure_identifier(value, reuse);
        }
        for (i, &element) in elements.iter().enumerate() {
            if self.level >= FlattenLevel::ObjectRest {
                unreachable!("FlattenLevelObjectRest array path is deferred to objectrestspread");
            } else if self.ec.arena().kind(element) == Kind::OmittedExpression {
                continue;
            } else if self.get_rest_indicator(element).is_none() {
                let index = self
                    .ec
                    .arena_mut()
                    .new_numeric_literal(&i.to_string(), TokenFlags::NONE);
                let rhs = self
                    .ec
                    .arena_mut()
                    .new_element_access_expression(value, None, index);
                self.flatten_binding_or_assignment_element(element, Some(rhs), false);
            } else if i == num - 1 {
                let rhs = self.new_array_slice_call(value, i);
                self.flatten_binding_or_assignment_element(element, Some(rhs), false);
            }
        }
    }

    // Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern
    fn flatten_object_binding_or_assignment_pattern(
        &mut self,
        parent: NodeId,
        pattern: NodeId,
        value: NodeId,
    ) {
        let elements = self.get_elements_of_pattern(pattern);
        let num = elements.len();
        let mut value = value;
        if num != 1 {
            let reuse = !is_declaration_binding_element(self.ec.arena().kind(parent)) || num != 0;
            value = self.ensure_identifier(value, reuse);
        }
        // Non-rest elements kept as a sub-pattern at `FlattenLevelObjectRest`
        // (e.g. `{ a }` in `{ a, ...r }`); accumulated then emitted as one
        // `{ <kept> } = value` (assignment) / `{ <kept> } = value` (binding).
        let mut binding_elements: Vec<NodeId> = Vec::new();
        for (i, &element) in elements.iter().enumerate() {
            if self.get_rest_indicator(element).is_none() {
                let property_name = self
                    .try_get_property_name(element)
                    .expect("non-rest object element has a property name");
                if self.level >= FlattenLevel::ObjectRest && self.is_kept_object_element(element) {
                    let visited = self.visit(element);
                    binding_elements.push(visited);
                } else if self.level >= FlattenLevel::ObjectRest {
                    // Decomposing a kept element at `ObjectRest` (computed keys /
                    // nested rest) needs `computedTempVariables` plumbing; the
                    // assignment wiring gates these out (DEFER).
                    unreachable!(
                        "non-simple object-rest element decomposition is deferred (gated upstream)"
                    );
                } else {
                    let rhs = self.create_destructuring_property_access(value, property_name);
                    self.flatten_binding_or_assignment_element(element, Some(rhs), false);
                }
            } else if i == num - 1 {
                self.flush_kept_object_elements(&mut binding_elements, value);
                let rhs = self.new_rest_helper_call(value, &elements);
                self.flatten_binding_or_assignment_element(element, Some(rhs), false);
            }
        }
        self.flush_kept_object_elements(&mut binding_elements, value);
    }

    /// Emits the accumulated kept object elements as a single `{ <kept> } =
    /// value` assignment (or binding), clearing the accumulator. No-op when the
    /// accumulator is empty.
    // Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern (emit kept bindings)
    fn flush_kept_object_elements(&mut self, binding_elements: &mut Vec<NodeId>, value: NodeId) {
        if binding_elements.is_empty() {
            return;
        }
        let elements = std::mem::take(binding_elements);
        let pattern = self.create_object_pattern(elements);
        self.emit_binding_or_assignment(pattern, value);
    }

    /// Reports whether a non-rest object element is kept as a sub-pattern at
    /// `FlattenLevelObjectRest` (rather than decomposed): a simple element with a
    /// non-computed property key and no nested rest/spread. Go uses subtree facts
    /// (`SubtreeContainsRestOrSpread`); the port checks structurally.
    // Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern (keep condition)
    fn is_kept_object_element(&self, element: NodeId) -> bool {
        if let Some(property_name) = self.try_get_property_name(element) {
            if self.ec.arena().kind(property_name) == Kind::ComputedPropertyName {
                return false;
            }
        }
        !self.element_contains_rest_or_spread(element)
    }

    /// Structurally reports whether `element` (or its nested pattern) contains a
    /// rest/spread, used to decide whether an object element can be kept as a
    /// sub-pattern. Stands in for Go's `SubtreeFacts() & SubtreeContainsRestOrSpread`.
    fn element_contains_rest_or_spread(&self, element: NodeId) -> bool {
        if matches!(
            self.ec.arena().kind(element),
            Kind::SpreadElement | Kind::SpreadAssignment
        ) {
            return true;
        }
        match self.get_target_of_element(element) {
            Some(target) if is_binding_pattern(self.ec.arena().kind(target)) => self
                .get_elements_of_pattern(target)
                .iter()
                .any(|&e| self.element_contains_rest_or_spread(e)),
            _ => false,
        }
    }

    /// Builds the `__rest(value, [<keys>])` call for an object-rest element,
    /// excluding every preceding element's (literal) property key. Shares the
    /// `objectrestspread` transform's `__rest` helper builder.
    // Go: internal/printer/factory.go:NodeFactory.NewRestHelper
    fn new_rest_helper_call(&mut self, value: NodeId, elements: &[NodeId]) -> NodeId {
        let mut property_names: Vec<NodeId> = Vec::new();
        for &element in &elements[..elements.len() - 1] {
            if let Some(property_name) = self.try_get_property_name(element) {
                // Computed/symbol keys need `computedTempVariables` (DEFER, gated
                // out upstream); reachable keys are identifier / string literals,
                // emitted as the excluded string-literal key.
                let key = self.ec.arena().text(property_name).to_string();
                let literal = self
                    .ec
                    .arena_mut()
                    .new_string_literal(&key, TokenFlags::NONE);
                property_names.push(literal);
            }
        }
        crate::estransforms::objectrestspread::new_rest_helper(self.ec, value, property_names)
    }

    /// Builds the object pattern node that holds the kept elements: an object
    /// **literal** in assignment mode, an object **binding pattern** in binding
    /// mode.
    // Go: internal/transformers/destructuring.go:createObjectAssignmentPattern / createObjectBindingPattern
    fn create_object_pattern(&mut self, elements: Vec<NodeId>) -> NodeId {
        match self.mode {
            FlattenMode::Assignment => self
                .ec
                .arena_mut()
                .new_object_literal_expression(NodeList::new(elements)),
            FlattenMode::Binding => self
                .ec
                .arena_mut()
                .new_binding_pattern(Kind::ObjectBindingPattern, NodeList::new(elements)),
        }
    }

    /// Builds the property/element access used to read a binding element's
    /// value from the source object (`value.a`, `value["a"]`, or `value[<key>]`).
    // Go: internal/transformers/destructuring.go:createDestructuringPropertyAccess
    fn create_destructuring_property_access(
        &mut self,
        value: NodeId,
        property_name: NodeId,
    ) -> NodeId {
        let kind = self.ec.arena().kind(property_name);
        if kind == Kind::ComputedPropertyName {
            let expression = match self.ec.arena().data(property_name) {
                NodeData::ComputedPropertyName(d) => d.expression,
                _ => unreachable!("kind/data mismatch"),
            };
            let visited = self.visit(expression);
            let argument = self.ensure_identifier(visited, false);
            self.ec
                .arena_mut()
                .new_element_access_expression(value, None, argument)
        } else if is_string_or_numeric_literal_like(kind) {
            unimplemented!("string/numeric literal property keys (DEFER)")
        } else {
            let text = self.ec.arena().text(property_name).to_string();
            let name = self.ec.arena_mut().new_identifier(&text);
            self.ec
                .arena_mut()
                .new_property_access_expression(value, None, name)
        }
    }

    /// Returns the property name of a binding/assignment element (the key to
    /// read from the source object): a binding element's renamed key, a property
    /// assignment's key (`{ a: b }` → `a`), a shorthand identifier, or a
    /// computed key's literal expression.
    // Go: internal/ast/utilities.go:TryGetPropertyNameOfBindingOrAssignmentElement
    fn try_get_property_name(&self, element: NodeId) -> Option<NodeId> {
        // `a`/`[a]`/`"a"`/`1` in `let { a: b } = ...`.
        let explicit_key = match self.ec.arena().data(element) {
            NodeData::BindingElement(d) => d.property_name,
            NodeData::PropertyAssignment(d) => Some(d.name),
            _ => None,
        };
        if let Some(property_name) = explicit_key {
            if self.ec.arena().kind(property_name) == Kind::ComputedPropertyName {
                let expr = match self.ec.arena().data(property_name) {
                    NodeData::ComputedPropertyName(c) => c.expression,
                    _ => unreachable!("kind/data mismatch"),
                };
                if is_string_or_numeric_literal_like(self.ec.arena().kind(expr)) {
                    return Some(expr);
                }
            }
            return Some(property_name);
        }
        let target = self.get_target_of_element(element)?;
        if is_property_name(self.ec.arena().kind(target)) {
            Some(target)
        } else {
            None
        }
    }

    // Go: internal/transformers/destructuring.go:emitBinding
    fn emit_binding(&mut self, target: NodeId, value: NodeId) {
        debug_assert!(
            self.expressions.is_empty(),
            "leftover expressions only arise on the hoist path (DEFER)"
        );
        self.declarations.push(PendingDecl {
            name: target,
            value,
            pending_expressions: Vec::new(),
        });
    }

    // Go: internal/transformers/destructuring.go:ensureIdentifier
    fn ensure_identifier(&mut self, value: NodeId, reuse_identifier_expressions: bool) -> NodeId {
        if reuse_identifier_expressions && self.ec.arena().kind(value) == Kind::Identifier {
            return value;
        }
        let temp = self.ec.factory().new_temp_variable();
        if self.hoist_temp_variables {
            self.ec.add_variable_declaration(temp);
            let assign = self.new_assignment(temp, value);
            self.expressions.push(assign);
        } else {
            self.emit_binding(temp, value);
        }
        temp
    }

    /// Builds `value === void 0 ? default_value : value`, hoisting `value` into
    /// a temp first so it is evaluated only once.
    // Go: internal/transformers/destructuring.go:createDefaultValueCheck
    fn create_default_value_check(&mut self, value: NodeId, default_value: NodeId) -> NodeId {
        let value = self.ensure_identifier(value, true);
        let void_zero = self.make_void_zero();
        let strict_eq = self.ec.arena_mut().new_token(Kind::EqualsEqualsEqualsToken);
        let condition = self
            .ec
            .arena_mut()
            .new_binary_expression(value, strict_eq, void_zero);
        let question = self.ec.arena_mut().new_token(Kind::QuestionToken);
        let colon = self.ec.arena_mut().new_token(Kind::ColonToken);
        self.ec.arena_mut().new_conditional_expression(
            condition,
            question,
            default_value,
            colon,
            value,
        )
    }

    /// Builds the `void 0` expression.
    // Go: internal/printer/factory.go:NodeFactory.NewVoidZeroExpression
    fn make_void_zero(&mut self) -> NodeId {
        let zero = self
            .ec
            .arena_mut()
            .new_numeric_literal("0", TokenFlags::NONE);
        self.ec.arena_mut().new_void_expression(zero)
    }

    /// Builds an `array.slice(start)` call (`array.slice()` when `start` is 0).
    // Go: internal/printer/factory.go:NodeFactory.NewArraySliceCall
    fn new_array_slice_call(&mut self, array: NodeId, start: usize) -> NodeId {
        let mut args: Vec<NodeId> = Vec::new();
        if start != 0 {
            let literal = self
                .ec
                .arena_mut()
                .new_numeric_literal(&start.to_string(), TokenFlags::NONE);
            args.push(literal);
        }
        let slice = self.ec.arena_mut().new_identifier("slice");
        let callee = self
            .ec
            .arena_mut()
            .new_property_access_expression(array, None, slice);
        self.ec.arena_mut().new_call_expression(
            callee,
            None,
            None,
            NodeList::new(args),
            NodeFlags::NONE,
        )
    }

    /// Builds an assignment expression `left = right`.
    // Go: internal/printer/factory.go:NodeFactory.NewAssignmentExpression
    fn new_assignment(&mut self, left: NodeId, right: NodeId) -> NodeId {
        let equals = self.ec.arena_mut().new_token(Kind::EqualsToken);
        self.ec
            .arena_mut()
            .new_binary_expression(left, equals, right)
    }

    /// Returns the binding/assignment target of an element.
    ///
    /// Binding elements yield their `.name` (the variable declaration / binding
    /// element name). Assignment elements unwrap to the underlying assignment
    /// target: a property assignment `a: b` yields `b`, a shorthand `a` yields
    /// `a`, a spread `...a` / `[a = 1]` default yields `a`, and a bare target
    /// (identifier / member access / nested literal pattern) yields itself.
    // Go: internal/ast/utilities.go:GetTargetOfBindingOrAssignmentElement
    fn get_target_of_element(&self, element: NodeId) -> Option<NodeId> {
        match self.ec.arena().data(element) {
            // Declaration binding elements: `let { a } = ...`, `let [a] = ...`.
            NodeData::VariableDeclaration(d) => return Some(d.name),
            NodeData::BindingElement(d) => return d.name,
            // Object-literal assignment elements.
            NodeData::PropertyAssignment(d) => {
                // `b` in `({ a: b } = ...)` / `({ a: b = 1 } = ...)`.
                return d
                    .initializer
                    .and_then(|init| self.get_target_of_element(init));
            }
            NodeData::ShorthandPropertyAssignment(d) => return Some(d.name),
            NodeData::SpreadAssignment(d) => return self.get_target_of_element(d.expression),
            NodeData::SpreadElement(d) => return self.get_target_of_element(d.expression),
            _ => {}
        }
        if is_assignment_expression_equals(self.ec.arena(), element) {
            // `a` in `[a = 1] = ...`.
            return self.get_target_of_element(self.binary_left(element));
        }
        // Bare assignment target: `a`, `a.b`, `a[0]`, `[a]`, `{ a }`.
        Some(element)
    }

    /// Returns the default initializer of a binding/assignment element, if any.
    ///
    /// Declaration elements yield their `initializer`; a shorthand `a = 1`
    /// yields `1`; a property assignment `a: b = 1` yields `1` (only when its
    /// value is itself an assignment); an assignment-pattern element `[a = 1]`
    /// yields the assignment's right-hand side; a spread unwraps to its target.
    // Go: internal/transformers/destructuring.go:GetInitializerOfBindingOrAssignmentElement
    fn get_initializer_of_element(&self, element: NodeId) -> Option<NodeId> {
        match self.ec.arena().data(element) {
            NodeData::VariableDeclaration(d) => return d.initializer,
            NodeData::BindingElement(d) => return d.initializer,
            NodeData::PropertyAssignment(d) => {
                let initializer = d.initializer?;
                return if is_assignment_expression_equals(self.ec.arena(), initializer) {
                    Some(self.binary_right(initializer))
                } else {
                    None
                };
            }
            NodeData::ShorthandPropertyAssignment(d) => return d.object_assignment_initializer,
            NodeData::SpreadElement(d) => return self.get_initializer_of_element(d.expression),
            NodeData::SpreadAssignment(d) => return self.get_initializer_of_element(d.expression),
            _ => {}
        }
        if is_assignment_expression_equals(self.ec.arena(), element) {
            return Some(self.binary_right(element));
        }
        None
    }

    /// Returns the `...` rest indicator of an element, if present: a binding
    /// element's `...` token, or the spread element/assignment node itself.
    // Go: internal/ast/utilities.go:GetRestIndicatorOfBindingOrAssignmentElement
    fn get_rest_indicator(&self, element: NodeId) -> Option<NodeId> {
        match self.ec.arena().data(element) {
            NodeData::BindingElement(d) => d.dot_dot_dot_token,
            NodeData::SpreadElement(_) | NodeData::SpreadAssignment(_) => Some(element),
            _ => None,
        }
    }

    /// Returns the elements of an array/object binding pattern (binding
    /// elements) or assignment pattern (array literal elements / object literal
    /// properties).
    // Go: internal/ast/utilities.go:GetElementsOfBindingOrAssignmentPattern
    fn get_elements_of_pattern(&self, pattern: NodeId) -> Vec<NodeId> {
        match self.ec.arena().data(pattern) {
            NodeData::ArrayBindingPattern(d) | NodeData::ObjectBindingPattern(d) => {
                d.elements.nodes.clone()
            }
            NodeData::ArrayLiteralExpression(d) | NodeData::ObjectLiteralExpression(d) => {
                d.list.nodes.clone()
            }
            _ => Vec::new(),
        }
    }
}

/// Reports whether a variable declaration's name is an array/object binding
/// pattern.
///
/// Side effects: none (reads the arena).
fn declaration_name_is_binding_pattern(arena: &NodeArena, decl: NodeId) -> bool {
    let name = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.name,
        _ => return false,
    };
    is_array_binding_pattern(arena.kind(name)) || is_object_binding_pattern(arena.kind(name))
}

/// Reports whether a declaration's binding pattern ends in an object rest
/// element (`{ ..., ...rest }`); such declarations are left to the round-6g
/// `objectrestspread` transform.
///
/// Side effects: none (reads the arena).
fn declaration_has_object_rest(arena: &NodeArena, decl: NodeId) -> bool {
    let name = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.name,
        _ => return false,
    };
    let elements = match arena.data(name) {
        NodeData::ObjectBindingPattern(d) => &d.elements,
        _ => return false,
    };
    elements
        .nodes
        .last()
        .copied()
        .is_some_and(|last| matches!(arena.data(last), NodeData::BindingElement(d) if d.dot_dot_dot_token.is_some()))
}

/// Reports whether a node kind is an object binding pattern (`{ a }`) or an
/// object assignment pattern (`{ a }` as an assignment target, an object
/// literal).
// Go: internal/transformers/destructuring.go:isObjectBindingOrAssignmentPattern
fn is_object_binding_pattern(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::ObjectBindingPattern | Kind::ObjectLiteralExpression
    )
}

/// Reports whether a node kind is an array binding pattern (`[a]`) or an array
/// assignment pattern (`[a]` as an assignment target, an array literal).
// Go: internal/transformers/destructuring.go:isArrayBindingOrAssignmentPattern
fn is_array_binding_pattern(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::ArrayBindingPattern | Kind::ArrayLiteralExpression
    )
}

/// Skips any wrapping parentheses, returning the innermost expression.
// Go: internal/ast/utilities.go:SkipParentheses (parenthesized subset)
pub(crate) fn skip_parentheses(arena: &NodeArena, mut node: NodeId) -> NodeId {
    while arena.kind(node) == Kind::ParenthesizedExpression {
        node = match arena.data(node) {
            NodeData::ParenthesizedExpression(d) => d.expression,
            _ => break,
        };
    }
    node
}

/// Reports whether `node` is a destructuring assignment: a `=` binary
/// expression whose left operand is an array or object literal (assignment)
/// pattern. Mirrors Go's `IsDestructuringAssignment`; the `IsLeftHandSideExpression`
/// guard is subsumed by the array/object-literal kind check (both are always
/// left-hand-side expressions).
// Go: internal/ast/utilities.go:IsDestructuringAssignment
pub(crate) fn is_destructuring_assignment(arena: &NodeArena, node: NodeId) -> bool {
    if !is_assignment_expression_equals(arena, node) {
        return false;
    }
    let left = match arena.data(node) {
        NodeData::BinaryExpression(d) => d.left,
        _ => return false,
    };
    matches!(
        arena.kind(left),
        Kind::ObjectLiteralExpression | Kind::ArrayLiteralExpression
    )
}

/// Reports whether `node` is a simple-assignment (`=`) binary expression.
/// Equivalent to Go's `IsAssignmentExpression(node, excludeCompoundAssignment = true)`
/// for the `=` operator (the left operand of a parsed `=` is always a valid
/// assignment target / pattern, so the `IsLeftHandSideExpression` guard is not
/// needed for the reachable subset).
// Go: internal/ast/utilities.go:IsAssignmentExpression (excludeCompoundAssignment)
fn is_assignment_expression_equals(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::BinaryExpression(d) if arena.kind(d.operator_token) == Kind::EqualsToken
    )
}

/// Reports whether `node` is an empty array literal (`[]`).
// Go: internal/ast/utilities.go:IsEmptyArrayLiteral
fn is_empty_array_literal(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::ArrayLiteralExpression(d) if d.list.nodes.is_empty())
}

/// Reports whether `node` is an empty object literal (`{}`).
// Go: internal/ast/utilities.go:IsEmptyObjectLiteral
fn is_empty_object_literal(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::ObjectLiteralExpression(d) if d.list.nodes.is_empty())
}

/// Reports whether a node kind is a declaration binding element.
// Go: internal/ast/utilities.go:IsDeclarationBindingElement
fn is_declaration_binding_element(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::VariableDeclaration | Kind::Parameter | Kind::BindingElement
    )
}

/// Reports whether a node kind is an array or object binding pattern.
// Go: internal/ast/utilities.go:IsBindingPattern (binding subset)
fn is_binding_pattern(kind: Kind) -> bool {
    is_array_binding_pattern(kind) || is_object_binding_pattern(kind)
}

/// Reports whether a node kind is a simple, side-effect-free expression that can
/// be duplicated without a hoisted temp.
// Go: internal/transformers/utilities.go:IsSimpleCopiableExpression
fn is_simple_copiable(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Identifier
            | Kind::NumericLiteral
            | Kind::StringLiteral
            | Kind::BigIntLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::ThisKeyword
            | Kind::SuperKeyword
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
    )
}

/// Reports whether a node kind can serve as a property name.
// Go: internal/ast/utilities.go:IsPropertyName
fn is_property_name(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Identifier
            | Kind::PrivateIdentifier
            | Kind::StringLiteral
            | Kind::NumericLiteral
            | Kind::ComputedPropertyName
    )
}

/// Reports whether a node kind is a string- or numeric-literal-like node.
// Go: internal/ast/utilities.go:IsStringOrNumericLiteralLike
fn is_string_or_numeric_literal_like(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral | Kind::NumericLiteral
    )
}

/// Reports whether a node kind is a literal-token expression.
// Go: internal/ast/utilities.go:IsLiteralExpression / IsLiteralKind
fn is_literal_expression(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::StringLiteral
            | Kind::RegularExpressionLiteral
            | Kind::NoSubstitutionTemplateLiteral
    )
}

#[cfg(test)]
#[path = "destructuring_test.rs"]
mod tests;
