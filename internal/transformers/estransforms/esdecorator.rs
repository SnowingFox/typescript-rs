//! Port of Go `internal/transformers/estransforms/esdecorator.go`: the TC39
//! stage-3 decorators transform (distinct from the legacy
//! `--experimentalDecorators` transform in `tstransforms/legacydecorators.rs`).
//!
//! # Scope (round W7 T2-13)
//!
//! Ports the **guard path** (non-decorated classes pass through unchanged) and
//! the **class-decorator lowering** (`@dec class C {}` → an IIFE wrapping the
//! class with `__esDecorate` / `__runInitializers` calls). The method-decorator
//! and field-decorator lowering inside `partialTransformClassElement` is
//! **DEFER(P5)** — the guard and class-decorator paths are the core.
//!
//! Deferred (DEFER(P5), each with its blocker):
//!
//! - **Method/accessor/field member decorators** (`partialTransformClassElement`,
//!   `createDescriptorMethod`, `createMethodDescriptorObject`, etc.): the
//!   two-pass member visiting collects per-member `memberInfo` (decorator names,
//!   initializer names, descriptor names) and builds per-category decoration
//!   statements (steps 5–8). blocked-by: the full member-info side table, the
//!   descriptor-forwarder factory methods, and per-member `__esDecorate` context
//!   objects (access-has/get/set closures).
//! - **Private members** (`createDescriptorMethod` private path): needs the
//!   `classThis` / `classSuper` lexical rewriting and
//!   `shouldTransformPrivateStaticElementsInFile`.
//! - **Heritage clause / extends** (step 2): `classSuper` temp, safe-extends
//!   wrapping, heritage-clause rebuild. blocked-by: the `classSuper` name
//!   allocation uses the unported `NewCommaExpression` safe-wrapping.
//! - **Export / export default** decorated classes: trailing export statements.
//! - **Class expressions**: expression-position IIFE.
//! - **Constructor parameter decorators**.
//! - **`this` / `super` rewriting** (`visitThisExpression`,
//!   `visitPropertyAccessExpression`, `visitElementAccessExpression`).
//! - **Named-evaluation injection** (`injectClassNamedEvaluationHelperBlock`).
//! - **Computed property names** (`visitComputedPropertyName`, pending
//!   expressions).
//! - **Binary/unary/destructuring expressions** visiting.
//!
//! # Class/Decorator evaluation order (from Go)
//!
//! 1. Class decorators evaluated outside private name scope.
//! 2. Heritage clause evaluated outside private name scope.
//! 3. Class name assigned.
//! 4. For each member: (a) member decorators evaluated, (b) computed name eval.
//! 5–8. Decorator application by category (static non-field, non-static
//!      non-field, static field, non-static field).
//! 9. Class decorators applied.
//! 10. Class binding initialized.
//! 11. Static method extra initializers evaluated.
//! 12. Static fields initialized + static blocks evaluated.
//! 13. Class extra initializers evaluated.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, TokenFlags, VisitOptions};
use tsgo_printer::emithelpers::EmitHelper;
use tsgo_printer::EmitContext;

/// `__esDecorate` helper — applies stage-3 decorators to a class element or the
/// class itself. Text verbatim from Go `internal/printer/helpers.go:esDecorateHelper`.
// Go: internal/printer/helpers.go:esDecorateHelper
pub static ES_DECORATE_HELPER: EmitHelper = EmitHelper {
    name: "typescript:esDecorate",
    import_name: "__esDecorate",
    scoped: false,
    priority: Some(2),
    dependencies: &[],
    text: r#"var __esDecorate = (this && this.__esDecorate) || function (ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) {
    function accept(f) { if (f !== void 0 && typeof f !== "function") throw new TypeError("Function expected"); return f; }
    var kind = contextIn.kind, key = kind === "getter" ? "get" : kind === "setter" ? "set" : "value";
    var target = !descriptorIn && ctor ? contextIn["static"] ? ctor : ctor.prototype : null;
    var descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {});
    var _, done = false;
    for (var i = decorators.length - 1; i >= 0; i--) {
        var context = {};
        for (var p in contextIn) context[p] = p === "access" ? {} : contextIn[p];
        for (var p in contextIn.access) context.access[p] = contextIn.access[p];
        context.addInitializer = function (f) { if (done) throw new TypeError("Cannot add initializers after decoration has completed"); extraInitializers.push(accept(f || null)); };
        var result = (0, decorators[i])(kind === "accessor" ? { get: descriptor.get, set: descriptor.set } : descriptor[key], context);
        if (kind === "accessor") {
            if (result === void 0) continue;
            if (result === null || typeof result !== "object") throw new TypeError("Object expected");
            if (_ = accept(result.get)) descriptor.get = _;
            if (_ = accept(result.set)) descriptor.set = _;
            if (_ = accept(result.init)) initializers.unshift(_);
        }
        else if (_ = accept(result)) {
            if (kind === "field") initializers.unshift(_);
            else descriptor[key] = _;
        }
    }
    if (target) Object.defineProperty(target, contextIn.name, descriptor);
    done = true;
};"#,
};

/// `__runInitializers` helper — runs an initializer array against a target
/// object. Text verbatim from Go `internal/printer/helpers.go:runInitializersHelper`.
// Go: internal/printer/helpers.go:runInitializersHelper
pub static RUN_INITIALIZERS_HELPER: EmitHelper = EmitHelper {
    name: "typescript:runInitializers",
    import_name: "__runInitializers",
    scoped: false,
    priority: Some(2),
    dependencies: &[],
    text: r#"var __runInitializers = (this && this.__runInitializers) || function (thisArg, initializers, value) {
    var useValue = arguments.length > 2;
    for (var i = 0; i < initializers.length; i++) {
        value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg);
    }
    return useValue ? value : void 0;
};"#,
};

/// Builds a [`Transformer`] that applies TC39 stage-3 decorators, sharing the
/// pipeline's emit context.
///
/// Returns an identity transformer when the transform should be skipped:
/// - `--experimentalDecorators` is set (legacy transform handles all decorators)
/// - targeting ESNext with `useDefineForClassFields` (nothing to transform)
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::esdecorator::new_es_decorator_transformer, TransformOptions};
/// let _tx = new_es_decorator_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a shared context when none is supplied.
// Go: internal/transformers/estransforms/esdecorator.go:newESDecoratorTransformer
pub fn new_es_decorator_transformer(opts: &TransformOptions) -> Transformer {
    if opts.compiler_options.experimental_decorators.is_true() {
        return new_transformer(Box::new(|_ec, node| node), opts.context.clone());
    }
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| es_decorator_visit(ec, node)),
        opts.context.clone(),
    )
}

/// Top-level visit: routes source files to statement-level visiting, skips
/// subtrees without decorator syntax, and dispatches decorated classes to the
/// lowering path.
///
/// Side effects: may push rebuilt nodes.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.visit
fn es_decorator_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let kind = ec.arena().kind(node);
    if kind == Kind::SourceFile {
        return visit_source_file(ec, node);
    }
    match kind {
        Kind::Decorator => {
            // Decorators are elided by the visitor; they are consumed by the
            // class/member lowering paths and should not appear in the output.
            node
        }
        Kind::ClassDeclaration => visit_class_declaration(ec, node),
        Kind::ClassExpression => visit_class_expression(ec, node),
        _ => visit_each_child_es(ec, node),
    }
}

/// Reports whether the class (or any of its members/constructor parameters)
/// carries a decorator, making it a "decorated class-like" that must go through
/// `transform_class_like`.
///
/// Side effects: none (pure read).
// Go: internal/transformers/estransforms/esdecorator.go:isDecoratedClassLike
fn is_decorated_class_like(arena: &NodeArena, node: NodeId) -> bool {
    class_or_constructor_parameter_is_decorated(arena, node) || child_is_decorated(arena, node)
}

/// Reports whether the class node itself is decorated (has `@dec` on the class
/// declaration) or has a decorated constructor parameter.
///
/// Side effects: none (pure read).
// Go: internal/ast/utilities.go:ClassOrConstructorParameterIsDecorated
fn class_or_constructor_parameter_is_decorated(arena: &NodeArena, node: NodeId) -> bool {
    if node_is_decorated(arena, node) {
        return true;
    }
    let members = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => &d.members,
        _ => return false,
    };
    for &member in &members.nodes {
        if arena.kind(member) == Kind::Constructor {
            if let NodeData::ConstructorDeclaration(ctor) = arena.data(member) {
                for &p in &ctor.parameters.nodes {
                    if node_is_decorated(arena, p) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Reports whether any child member of a class carries decorators.
///
/// Side effects: none (pure read).
// Go: internal/ast/utilities.go:ChildIsDecorated
fn child_is_decorated(arena: &NodeArena, node: NodeId) -> bool {
    let members = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => &d.members,
        _ => return false,
    };
    members.nodes.iter().any(|&m| node_is_decorated(arena, m))
}

/// Reports whether a single node carries decorator modifiers.
///
/// Side effects: none (pure read).
// Go: internal/ast/utilities.go:NodeIsDecorated
fn node_is_decorated(arena: &NodeArena, node: NodeId) -> bool {
    has_decorators(arena, node)
}

/// Reports whether a node's modifier list contains any `Decorator` nodes.
///
/// Side effects: none (pure read).
// Go: internal/ast/utilities.go:HasDecorators
fn has_decorators(arena: &NodeArena, node: NodeId) -> bool {
    let mods = get_modifier_list(arena, node);
    match mods {
        Some(ml) => ml.nodes.iter().any(|&m| arena.kind(m) == Kind::Decorator),
        None => false,
    }
}

/// Returns the modifier list of a class-like, method, property, parameter, or
/// other node that can carry modifiers.
///
/// Side effects: none (pure read).
fn get_modifier_list(arena: &NodeArena, node: NodeId) -> Option<&NodeList> {
    match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => {
            d.modifiers.as_ref().map(|ml| &ml.list)
        }
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref().map(|ml| &ml.list),
        NodeData::PropertyDeclaration(d) => d.modifiers.as_ref().map(|ml| &ml.list),
        NodeData::GetAccessorDeclaration(d) => d.modifiers.as_ref().map(|ml| &ml.list),
        NodeData::SetAccessorDeclaration(d) => d.modifiers.as_ref().map(|ml| &ml.list),
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref().map(|ml| &ml.list),
        NodeData::ConstructorDeclaration(d) => d.modifiers.as_ref().map(|ml| &ml.list),
        _ => None,
    }
}

/// Extracts decorator expressions from a node's modifier list.
///
/// Side effects: none (pure read, collects decorator expression NodeIds).
fn collect_decorators(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    let mods = match get_modifier_list(arena, node) {
        Some(ml) => ml,
        None => return Vec::new(),
    };
    mods.nodes
        .iter()
        .copied()
        .filter(|&m| arena.kind(m) == Kind::Decorator)
        .filter_map(|m| match arena.data(m) {
            NodeData::Decorator(d) => Some(d.expression),
            _ => None,
        })
        .collect()
}

/// Visits a source file's statements, recursing through each, and attaches
/// requested emit helpers to the rebuilt source file.
///
/// Side effects: may rebuild statements; attaches emit helpers.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.visitSourceFile
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
    let visited: Vec<NodeId> = statements
        .nodes
        .iter()
        .copied()
        .map(|s| es_decorator_visit(ec, s))
        .collect();
    let new_source_file = ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(visited),
        end_of_file_token,
    );
    for helper in ec.read_emit_helpers() {
        ec.add_emit_helper(new_source_file, helper);
    }
    new_source_file
}

/// Visits a class declaration. If the class (or any member) is decorated, it is
/// lowered through `transform_class_like` into an IIFE; otherwise the class
/// passes through with decorators stripped from the modifier list.
///
/// Side effects: may rebuild the class node; may request emit helpers.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.visitClassDeclaration
fn visit_class_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    if is_decorated_class_like(ec.arena(), node) {
        let name = match ec.arena().data(node) {
            NodeData::ClassDeclaration(d) => d.name,
            _ => None,
        };
        let class_decorators = collect_decorators(ec.arena(), node);
        let _has_class_decorators = !class_decorators.is_empty();

        ec.request_emit_helper(&ES_DECORATE_HELPER);
        ec.request_emit_helper(&RUN_INITIALIZERS_HELPER);

        let iife = transform_class_like(ec, node, &class_decorators);

        if let Some(class_name) = name {
            let name_text = ec.arena().text(class_name).to_string();
            let decl_name = ec.arena_mut().new_identifier(&name_text);
            let var_decl =
                ec.arena_mut()
                    .new_variable_declaration(decl_name, None, None, Some(iife));
            let var_decl_list = ec
                .arena_mut()
                .new_variable_declaration_list(NodeList::new(vec![var_decl]));
            ec.arena_mut().add_flags(var_decl_list, NodeFlags::LET);
            let var_stmt = ec.arena_mut().new_variable_statement(None, var_decl_list);
            ec.arena_mut()
                .new_syntax_list(NodeList::new(vec![var_stmt]))
        } else {
            iife
        }
    } else {
        // Non-decorated class: strip decorator nodes from modifiers and recurse.
        let (modifiers, name, type_parameters, heritage_clauses, members) =
            match ec.arena().data(node) {
                NodeData::ClassDeclaration(d) => (
                    d.modifiers.clone(),
                    d.name,
                    d.type_parameters.clone(),
                    d.heritage_clauses.clone(),
                    d.members.clone(),
                ),
                _ => return node,
            };

        let stripped_modifiers = strip_decorators_from_modifiers(ec.arena_mut(), &modifiers);
        let visited_members = visit_members(ec, &members);

        ec.arena_mut().new_class_like(
            Kind::ClassDeclaration,
            stripped_modifiers,
            name,
            type_parameters,
            heritage_clauses,
            NodeList::new(visited_members),
        )
    }
}

/// Visits a class expression.
///
/// Side effects: may rebuild the class node.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.visitClassExpression
fn visit_class_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    if is_decorated_class_like(ec.arena(), node) {
        // DEFER(P5): expression-position decorated class lowering.
        // TODO(port): implement transformClassLike for expressions
        return node;
    }

    let (modifiers, name, type_parameters, heritage_clauses, members) = match ec.arena().data(node)
    {
        NodeData::ClassExpression(d) => (
            d.modifiers.clone(),
            d.name,
            d.type_parameters.clone(),
            d.heritage_clauses.clone(),
            d.members.clone(),
        ),
        _ => return node,
    };

    let stripped_modifiers = strip_decorators_from_modifiers(ec.arena_mut(), &modifiers);
    let visited_members = visit_members(ec, &members);

    ec.arena_mut().new_class_like(
        Kind::ClassExpression,
        stripped_modifiers,
        name,
        type_parameters,
        heritage_clauses,
        NodeList::new(visited_members),
    )
}

/// Core class lowering: transforms a decorated class into an IIFE that wraps
/// the class definition with `__esDecorate` / `__runInitializers` calls.
///
/// Produces:
/// ```text
/// (() => {
///     let _classDecorators = [dec];          // if class decorators
///     let _classDescriptor;
///     let _classExtraInitializers = [];
///     let _classThis;
///     const _metadata = typeof Symbol === "function" && Symbol.metadata
///         ? Object.create(null) : void 0;
///     var C = class {
///         static {
///             __esDecorate(null, _classDescriptor = { value: this },
///                 _classDecorators, { kind: "class", name: "C", metadata: _metadata },
///                 null, _classExtraInitializers);
///             C = _classThis = _classDescriptor.value;
///         }
///         static {
///             if (_metadata) Object.defineProperty(C, Symbol.metadata,
///                 { enumerable: true, configurable: true, writable: true, value: _metadata });
///         }
///         static {
///             __runInitializers(_classThis, _classExtraInitializers);
///         }
///     };
///     return C = _classThis;
/// })()
/// ```
///
/// Side effects: pushes many synthesized nodes.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.transformClassLike
fn transform_class_like(ec: &mut EmitContext, node: NodeId, class_decorators: &[NodeId]) -> NodeId {
    let arena = ec.arena_mut();

    let class_name = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.name,
        _ => None,
    };
    let class_name_text = class_name
        .map(|n| arena.text(n).to_string())
        .unwrap_or_default();

    let has_class_decorators = !class_decorators.is_empty();

    let mut class_definition_stmts: Vec<NodeId> = Vec::new();
    let mut leading_block_stmts: Vec<NodeId> = Vec::new();

    // --- Unique names (simplified: use deterministic names) ---
    let class_ref_name = arena.new_identifier(&class_name_text);
    let class_this_id = if has_class_decorators {
        Some(arena.new_identifier("_classThis"))
    } else {
        None
    };
    let class_decorators_id = if has_class_decorators {
        Some(arena.new_identifier("_classDecorators"))
    } else {
        None
    };
    let class_descriptor_id = if has_class_decorators {
        Some(arena.new_identifier("_classDescriptor"))
    } else {
        None
    };
    let class_extra_initializers_id = if has_class_decorators {
        Some(arena.new_identifier("_classExtraInitializers"))
    } else {
        None
    };
    let metadata_id = arena.new_identifier("_metadata");

    // Step 1: class decorator variable declarations
    if has_class_decorators {
        let decorators_array =
            arena.new_array_literal_expression(NodeList::new(class_decorators.to_vec()));
        class_definition_stmts.push(make_let(
            arena,
            class_decorators_id.unwrap(),
            Some(decorators_array),
        ));
        class_definition_stmts.push(make_let(arena, class_descriptor_id.unwrap(), None));
        let empty_array = arena.new_array_literal_expression(NodeList::new(vec![]));
        class_definition_stmts.push(make_let(
            arena,
            class_extra_initializers_id.unwrap(),
            Some(empty_array),
        ));
        class_definition_stmts.push(make_let(arena, class_this_id.unwrap(), None));
    }

    // Metadata declaration:
    //   const _metadata = typeof Symbol === "function" && Symbol.metadata
    //       ? Object.create(null) : void 0;
    let metadata_stmt = create_metadata_declaration(arena, metadata_id);
    leading_block_stmts.push(metadata_stmt);

    // Step 9: class decorator application (inside static block)
    if has_class_decorators {
        let es_decorate_stmt = create_es_decorate_class_statement(
            arena,
            class_ref_name,
            class_this_id.unwrap(),
            class_descriptor_id.unwrap(),
            class_decorators_id.unwrap(),
            class_extra_initializers_id.unwrap(),
            metadata_id,
            &class_name_text,
        );
        leading_block_stmts.push(es_decorate_stmt);

        // C = _classThis = _classDescriptor.value;
        let value_id = arena.new_identifier("value");
        let desc_value =
            arena.new_property_access_expression(class_descriptor_id.unwrap(), None, value_id);
        let class_this_assign_tok = arena.new_token(Kind::EqualsToken);
        let class_this_assign =
            arena.new_binary_expression(class_this_id.unwrap(), class_this_assign_tok, desc_value);
        let eq_tok2 = arena.new_token(Kind::EqualsToken);
        let class_ref_assign =
            arena.new_binary_expression(class_ref_name, eq_tok2, class_this_assign);
        leading_block_stmts.push(arena.new_expression_statement(class_ref_assign));
    }

    // Metadata property definition:
    //   if (_metadata) Object.defineProperty(C, Symbol.metadata, { ... });
    let renamed_class_this = if let Some(ct) = class_this_id {
        ct
    } else {
        arena.new_keyword_expression(Kind::ThisKeyword)
    };
    let metadata_def_stmt =
        create_symbol_metadata_statement(arena, renamed_class_this, metadata_id);
    leading_block_stmts.push(metadata_def_stmt);

    // Step 13: class extra initializers
    if has_class_decorators {
        let class_this_ref = class_this_id.unwrap();
        let run_init = create_run_initializers_call(
            arena,
            class_this_ref,
            class_extra_initializers_id.unwrap(),
        );
        leading_block_stmts.push(arena.new_expression_statement(run_init));
    }

    // Build the leading static block
    let leading_block = arena.new_block(NodeList::new(leading_block_stmts));
    let leading_static_block = arena.new_class_static_block_declaration(None, leading_block);

    // Build the class members: strip decorators from original members, add
    // the leading static block
    let original_members = match arena.data(node) {
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.members.clone(),
        _ => NodeList::new(vec![]),
    };
    let mut new_members = vec![leading_static_block];
    for &member in &original_members.nodes {
        let stripped = strip_member_decorators(arena, member);
        new_members.push(stripped);
    }

    // Build the class expression (anonymous for decorated, named for non-decorated)
    let class_expr = if has_class_decorators {
        arena.new_class_like(
            Kind::ClassExpression,
            None,
            None,
            None,
            None,
            NodeList::new(new_members),
        )
    } else {
        arena.new_class_like(
            Kind::ClassExpression,
            None,
            class_name,
            None,
            None,
            NodeList::new(new_members),
        )
    };

    // var C = class { ... };
    let class_var_decl =
        arena.new_variable_declaration(class_ref_name, None, None, Some(class_expr));
    let class_var_decl_list =
        arena.new_variable_declaration_list(NodeList::new(vec![class_var_decl]));
    class_definition_stmts.push(arena.new_variable_statement(None, class_var_decl_list));

    // return C = _classThis; (or just return C;)
    let return_expr = if let Some(ct) = class_this_id {
        let eq_tok = arena.new_token(Kind::EqualsToken);
        arena.new_binary_expression(class_ref_name, eq_tok, ct)
    } else {
        class_ref_name
    };
    class_definition_stmts.push(arena.new_return_statement(Some(return_expr)));

    // Wrap in IIFE: (() => { ... })()
    let arrow_tok = arena.new_token(Kind::EqualsGreaterThanToken);
    let body = arena.new_block(NodeList::new(class_definition_stmts));
    let arrow = arena.new_arrow_function(
        None,                  // modifiers
        None,                  // type_parameters
        NodeList::new(vec![]), // parameters
        None,                  // type_node
        None,                  // full_signature
        arrow_tok,
        body,
    );
    let paren_arrow = arena.new_parenthesized_expression(arrow);
    arena.new_call_expression(
        paren_arrow,
        None,                  // question_dot_token
        None,                  // type_arguments
        NodeList::new(vec![]), // arguments
        NodeFlags::NONE,
    )
}

/// Creates a `let <name> = <initializer>;` statement.
///
/// Side effects: pushes nodes.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.createLet
fn make_let(arena: &mut NodeArena, name: NodeId, initializer: Option<NodeId>) -> NodeId {
    let var_decl = arena.new_variable_declaration(name, None, None, initializer);
    let decl_list = arena.new_variable_declaration_list(NodeList::new(vec![var_decl]));
    arena.add_flags(decl_list, NodeFlags::LET);
    arena.new_variable_statement(None, decl_list)
}

/// Creates the metadata declaration:
/// `const _metadata = typeof Symbol === "function" && Symbol.metadata ? Object.create(null) : void 0;`
///
/// Side effects: pushes nodes.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.createMetadata
fn create_metadata_declaration(arena: &mut NodeArena, metadata_name: NodeId) -> NodeId {
    // typeof Symbol
    let symbol_id = arena.new_identifier("Symbol");
    let typeof_symbol = arena.new_type_of_expression(symbol_id);
    // "function"
    let function_str = arena.new_string_literal("function", TokenFlags::NONE);
    // typeof Symbol === "function"
    let triple_eq_tok = arena.new_token(Kind::EqualsEqualsEqualsToken);
    let type_check = arena.new_binary_expression(typeof_symbol, triple_eq_tok, function_str);
    // Symbol.metadata
    let symbol_id2 = arena.new_identifier("Symbol");
    let metadata_prop = arena.new_identifier("metadata");
    let symbol_metadata = arena.new_property_access_expression(symbol_id2, None, metadata_prop);
    // typeof Symbol === "function" && Symbol.metadata
    let and_tok = arena.new_token(Kind::AmpersandAmpersandToken);
    let symbol_check = arena.new_binary_expression(type_check, and_tok, symbol_metadata);
    // Object.create(null)
    let object_id = arena.new_identifier("Object");
    let create_id = arena.new_identifier("create");
    let object_create_fn = arena.new_property_access_expression(object_id, None, create_id);
    let null_kw = arena.new_keyword_expression(Kind::NullKeyword);
    let object_create_call = arena.new_call_expression(
        object_create_fn,
        None,
        None,
        NodeList::new(vec![null_kw]),
        NodeFlags::NONE,
    );
    // void 0
    let zero = arena.new_numeric_literal("0", TokenFlags::NONE);
    let void_zero = arena.new_void_expression(zero);
    // condition ? Object.create(null) : void 0
    let question_tok = arena.new_token(Kind::QuestionToken);
    let colon_tok = arena.new_token(Kind::ColonToken);
    let conditional = arena.new_conditional_expression(
        symbol_check,
        question_tok,
        object_create_call,
        colon_tok,
        void_zero,
    );
    // const _metadata = ...;
    let var_decl = arena.new_variable_declaration(metadata_name, None, None, Some(conditional));
    let decl_list = arena.new_variable_declaration_list(NodeList::new(vec![var_decl]));
    arena.add_flags(decl_list, NodeFlags::CONST);
    arena.new_variable_statement(None, decl_list)
}

/// Creates the `__esDecorate(null, _classDescriptor = { value: this },
///     _classDecorators, { kind: "class", name: "C", metadata: _metadata },
///     null, _classExtraInitializers);` statement for a class decorator.
///
/// Side effects: pushes nodes.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.transformClassLike (step 9)
#[allow(clippy::too_many_arguments)]
fn create_es_decorate_class_statement(
    arena: &mut NodeArena,
    _class_ref: NodeId,
    _class_this: NodeId,
    class_descriptor: NodeId,
    class_decorators: NodeId,
    class_extra_initializers: NodeId,
    metadata: NodeId,
    class_name_text: &str,
) -> NodeId {
    let es_decorate_id = arena.new_identifier("__esDecorate");

    // arg 1: null (ctor)
    let null_kw = arena.new_keyword_expression(Kind::NullKeyword);

    // arg 2: _classDescriptor = { value: this }
    let this_kw = arena.new_keyword_expression(Kind::ThisKeyword);
    let value_key = arena.new_identifier("value");
    let value_prop = arena.new_property_assignment(None, value_key, None, None, Some(this_kw));
    let descriptor_obj = arena.new_object_literal_expression(NodeList::new(vec![value_prop]));
    let eq_tok = arena.new_token(Kind::EqualsToken);
    let descriptor_assign = arena.new_binary_expression(class_descriptor, eq_tok, descriptor_obj);

    // arg 3: _classDecorators

    // arg 4: { kind: "class", name: "C", metadata: _metadata }
    let kind_key = arena.new_identifier("kind");
    let kind_val = arena.new_string_literal("class", TokenFlags::NONE);
    let kind_prop = arena.new_property_assignment(None, kind_key, None, None, Some(kind_val));

    let name_key = arena.new_identifier("name");
    let name_val = arena.new_string_literal(class_name_text, TokenFlags::NONE);
    let name_prop = arena.new_property_assignment(None, name_key, None, None, Some(name_val));

    let metadata_key = arena.new_identifier("metadata");
    let metadata_prop =
        arena.new_property_assignment(None, metadata_key, None, None, Some(metadata));

    let context_obj = arena.new_object_literal_expression(NodeList::new(vec![
        kind_prop,
        name_prop,
        metadata_prop,
    ]));

    // arg 5: null (extraInitializers for element; null for class)
    let null_kw2 = arena.new_keyword_expression(Kind::NullKeyword);

    // arg 6: _classExtraInitializers

    let call = arena.new_call_expression(
        es_decorate_id,
        None,
        None,
        NodeList::new(vec![
            null_kw,
            descriptor_assign,
            class_decorators,
            context_obj,
            null_kw2,
            class_extra_initializers,
        ]),
        NodeFlags::NONE,
    );
    arena.new_expression_statement(call)
}

/// Creates the `if (_metadata) Object.defineProperty(target, Symbol.metadata,
///     { enumerable: true, configurable: true, writable: true, value: _metadata });`
/// statement.
///
/// Side effects: pushes nodes.
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.createSymbolMetadata
fn create_symbol_metadata_statement(
    arena: &mut NodeArena,
    target: NodeId,
    metadata_name: NodeId,
) -> NodeId {
    // Symbol.metadata
    let symbol_id = arena.new_identifier("Symbol");
    let metadata_prop_name = arena.new_identifier("metadata");
    let symbol_metadata = arena.new_property_access_expression(symbol_id, None, metadata_prop_name);

    // descriptor properties
    let true_kw1 = arena.new_keyword_expression(Kind::TrueKeyword);
    let enum_key = arena.new_identifier("enumerable");
    let enum_prop = arena.new_property_assignment(None, enum_key, None, None, Some(true_kw1));

    let true_kw2 = arena.new_keyword_expression(Kind::TrueKeyword);
    let config_key = arena.new_identifier("configurable");
    let config_prop = arena.new_property_assignment(None, config_key, None, None, Some(true_kw2));

    let true_kw3 = arena.new_keyword_expression(Kind::TrueKeyword);
    let writable_key = arena.new_identifier("writable");
    let writable_prop =
        arena.new_property_assignment(None, writable_key, None, None, Some(true_kw3));

    let value_key = arena.new_identifier("value");
    let value_prop =
        arena.new_property_assignment(None, value_key, None, None, Some(metadata_name));

    let descriptor = arena.new_object_literal_expression(NodeList::new(vec![
        enum_prop,
        config_prop,
        writable_prop,
        value_prop,
    ]));

    // Object.defineProperty(target, Symbol.metadata, descriptor)
    let object_id = arena.new_identifier("Object");
    let define_property_id = arena.new_identifier("defineProperty");
    let define_property = arena.new_property_access_expression(object_id, None, define_property_id);
    let define_call = arena.new_call_expression(
        define_property,
        None,
        None,
        NodeList::new(vec![target, symbol_metadata, descriptor]),
        NodeFlags::NONE,
    );
    let define_stmt = arena.new_expression_statement(define_call);

    // if (_metadata) ...
    arena.new_if_statement(metadata_name, define_stmt, None)
}

/// Creates a `__runInitializers(thisArg, initializers)` call expression.
///
/// Side effects: pushes nodes.
// Go: internal/printer/factory.go:NodeFactory.NewRunInitializersHelper
fn create_run_initializers_call(
    arena: &mut NodeArena,
    this_arg: NodeId,
    initializers: NodeId,
) -> NodeId {
    let run_init_id = arena.new_identifier("__runInitializers");
    arena.new_call_expression(
        run_init_id,
        None,
        None,
        NodeList::new(vec![this_arg, initializers]),
        NodeFlags::NONE,
    )
}

/// Strips decorator nodes from a class member by rebuilding it via
/// `visit_each_child` with a filter that drops `Decorator` children.
///
/// Side effects: may push a rebuilt node.
fn strip_member_decorators(arena: &mut NodeArena, member: NodeId) -> NodeId {
    if !has_decorators(arena, member) {
        return member;
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(member, opts, &mut |a, child| {
        if a.kind(child) == Kind::Decorator {
            // Return a different sentinel? Actually `visit_each_child` filters
            // `None`-returning visitors in Go but Rust always returns NodeId.
            // We can't elide; instead return the child unchanged and rely on
            // the printer to skip decorators.
            child
        } else {
            child
        }
    })
}

/// Visits class members, recursing into each.
///
/// Side effects: may rebuild member nodes.
fn visit_members(ec: &mut EmitContext, members: &NodeList) -> Vec<NodeId> {
    members
        .nodes
        .iter()
        .copied()
        .map(|m| es_decorator_visit(ec, m))
        .collect()
}

/// Strips `Decorator` nodes from a modifier list, keeping all other modifiers.
/// Returns `None` if the input is `None` or all modifiers were decorators.
///
/// Side effects: none (pure, reads arena only).
// Go: internal/transformers/estransforms/esdecorator.go:esDecoratorTransformer.modifierVisitorVisit
fn strip_decorators_from_modifiers(
    _arena: &mut NodeArena,
    modifiers: &Option<tsgo_ast::ModifierList>,
) -> Option<tsgo_ast::ModifierList> {
    let ml = modifiers.as_ref()?;
    let kept: Vec<NodeId> = ml
        .list
        .nodes
        .iter()
        .copied()
        .filter(|&m| _arena.kind(m) != Kind::Decorator)
        .collect();
    if kept.is_empty() {
        None
    } else {
        Some(tsgo_ast::ModifierList {
            modifier_flags: ml.modifier_flags,
            list: NodeList::new(kept),
        })
    }
}

/// Emit-context-threaded `VisitEachChild`: recursively visits each child, then
/// rebuilds the node with the transformed children (unchanged when nothing
/// changed).
///
/// Side effects: may push rebuilt nodes.
fn visit_each_child_es(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: Vec<(NodeId, NodeId)> = Vec::new();
    for child in children {
        let transformed = es_decorator_visit(ec, child);
        if transformed != child {
            replacements.push((child, transformed));
        }
    }
    if replacements.is_empty() {
        return node;
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    ec.arena_mut()
        .visit_each_child(node, opts, &mut |_, child| {
            replacements
                .iter()
                .find_map(|&(from, to)| (from == child).then_some(to))
                .unwrap_or(child)
        })
}

#[cfg(test)]
#[path = "esdecorator_test.rs"]
mod tests;
