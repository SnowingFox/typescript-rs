//! Port of Go `internal/transformers/moduletransforms/systemmodule.go`: lowers a
//! module to the SystemJS `System.register([deps], function (exports, context) {
//! ... })` wrapper for `--module system` output.
//!
//! # Scope (round 6ae — reachable register-wrapper core)
//!
//! Runs only under `compilerOptions.module == System`; otherwise the source file
//! is returned unchanged. The reachable structural core landed this round wraps
//! the whole module body in the `System.register` call:
//!
//! ```text
//! System.register([<deps>], function (exports_1, context_1) {
//!     "use strict";
//!     return { setters: [<setters>], execute: function () { <body> } };
//! });
//! ```
//!
//! * **register wrapper** — every transformed source file becomes a single
//!   top-level `System.register(...)` expression statement. The two generated
//!   parameter names (`exports_1`/`context_1`) come from the emit-context name
//!   generator (`new_unique_name`), matching tsc. The outer module function body
//!   is emitted multi-line (Go builds it with `multiLine: true`); the inner
//!   `return { setters, execute }` object and the empty `execute` body stay
//!   single-line because the Rust printer does not yet carry the per-node
//!   `MultiLine` flag for list-bearing literals (see `emit_expressions.rs`).
//! * **dependency list** — each external import contributes its module specifier
//!   to the `System.register` dependency array, paired with a (currently empty)
//!   setter function in the `setters` array (one setter per dependency).
//! * **execute body** — top-level value statements (e.g. `f();`) are moved into
//!   the `execute` function body, in source order.
//!
//! # Deferred (DEFER(P5))
//!
//! The full System emit is large; this round lands only the wrapper + dependency
//! list + execute-body core. Deferred:
//!
//! * **export-setter wiring** — named exports → `exports({...})` calls inside the
//!   register body, and the per-dependency setter bodies that forward imported
//!   bindings. blocked-by: the real `ReferenceResolver` (checker
//!   `resolveName`/`EmitResolver`), the same gap as the CommonJS transform.
//! * **import binding rewriting / hoisting / live bindings** — rewriting uses of
//!   imported names to the dependency-local bindings, variable/function
//!   hoisting into the register body, and live-binding `exports(...)` updates.
//!   blocked-by: `ReferenceResolver`.
//! * `var __moduleName = context_1 && context_1.id;`, the module-name first
//!   argument (`System.register("name", [deps], ...)`), `export *` star helpers,
//!   and the `setters`/`execute` interplay with `export =`.

use crate::moduletransforms::externalmoduleinfo::collect_external_module_info;
use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeData, NodeFlags, NodeId, NodeList};
use tsgo_core::compileroptions::ModuleKind;
use tsgo_printer::{EmitContext, EmitFlags};

/// Builds a [`Transformer`] that lowers a module to the SystemJS
/// `System.register` form. Only active when `compilerOptions.module == System`;
/// otherwise the source file is returned unchanged.
///
/// # Examples
/// ```
/// use tsgo_transformers::{moduletransforms::systemmodule::new_system_module_transformer, TransformOptions};
/// let _tx = new_system_module_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/moduletransforms/systemmodule.go:NewSystemModuleTransformer
pub fn new_system_module_transformer(opt: &TransformOptions) -> Transformer {
    let is_system = opt.compiler_options.module == ModuleKind::System;
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            if is_system {
                system_visit(ec, node)
            } else {
                node
            }
        }),
        opt.context.clone(),
    )
}

/// Lowers a source file to the SystemJS register form; non-source-file nodes
/// pass through.
///
/// Side effects: may push rebuilt nodes.
// Go: internal/transformers/moduletransforms/systemmodule.go:SystemModuleTransformer.visit
fn system_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    if ec.arena().kind(node) == Kind::SourceFile {
        return transform_system_module(ec, node);
    }
    node
}

/// Wraps a source file's body in a `System.register([deps], function (exports,
/// context) { ... })` call.
///
/// Side effects: pushes rebuilt nodes.
// Go: internal/transformers/moduletransforms/systemmodule.go:transformSourceFile
fn transform_system_module(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (file_name, script_kind, language_variant, statements, end_of_file_token) =
        match ec.arena().data(node) {
            NodeData::SourceFile(d) => (
                d.file_name.clone(),
                d.script_kind,
                d.language_variant,
                d.statements.clone(),
                d.end_of_file_token,
            ),
            _ => unreachable!("kind checked by caller"),
        };

    // Each external import contributes one dependency + setter (no grouping/dedup
    // yet — that, and the setter bodies, are deferred; see the module docs).
    let info = collect_external_module_info(ec.arena(), node);
    let dependency_module_texts: Vec<String> = info
        .external_imports
        .iter()
        .filter_map(|&import| external_import_module_specifier_text(ec, import))
        .collect();

    // Top-level value statements (everything that is not module syntax) move into
    // the `execute` body, in source order. Module-syntax statements
    // (import/export declarations, `import =`, `export =`) are consumed as
    // dependencies above or deferred (export wiring) — see the module docs.
    let execute_statements: Vec<NodeId> = statements
        .nodes
        .iter()
        .copied()
        .filter(|&s| !is_module_syntax_statement(ec, s))
        .collect();

    // The two generated parameter names (`exports_1`/`context_1`), via the
    // emit-context name generator (matching tsc's `createUniqueName`).
    let exports_param_name = ec.factory().new_unique_name("exports");
    let context_param_name = ec.factory().new_unique_name("context");

    let register_call = build_system_register_call(
        ec,
        exports_param_name,
        context_param_name,
        &dependency_module_texts,
        execute_statements,
    );
    let register_stmt = ec.arena_mut().new_expression_statement(register_call);

    ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(vec![register_stmt]),
        end_of_file_token,
    )
}

/// Reports whether `statement` is top-level module syntax (an import/export
/// declaration, `import =`, or `export =`) rather than a value statement. Module
/// syntax is consumed as `System.register` dependencies or deferred (export
/// wiring); everything else moves into the `execute` body.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/moduletransforms/systemmodule.go:createSystemModuleBody (statement partition)
fn is_module_syntax_statement(ec: &EmitContext, statement: NodeId) -> bool {
    matches!(
        ec.arena().kind(statement),
        Kind::ImportDeclaration
            | Kind::ImportEqualsDeclaration
            | Kind::ExportDeclaration
            | Kind::ExportAssignment
    )
}

/// Reads the module-specifier text of an external import (`import "m"` /
/// `import ... from "m"` / `export ... from "m"`), if it is a string literal.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/moduletransforms/systemmodule.go:transformSourceFile (dependency collection)
fn external_import_module_specifier_text(ec: &EmitContext, import: NodeId) -> Option<String> {
    let module_specifier = match ec.arena().data(import) {
        NodeData::ImportDeclaration(d) => Some(d.module_specifier),
        NodeData::ExportDeclaration(d) => d.module_specifier,
        _ => None,
    }?;
    if ec.arena().kind(module_specifier) != Kind::StringLiteral {
        return None;
    }
    Some(ec.arena().text(module_specifier).to_string())
}

/// Builds the `System.register([<deps>], function (<exports>, <context>) { "use
/// strict"; return { setters: [<setters>], execute: function () { } }; })` call.
///
/// Side effects: pushes the identifier/array/function/object/call nodes; sets
/// the `MULTI_LINE` emit flag on the outer module function body.
// Go: internal/transformers/moduletransforms/systemmodule.go:transformSourceFile (System.register call)
fn build_system_register_call(
    ec: &mut EmitContext,
    exports_param_name: NodeId,
    context_param_name: NodeId,
    dependency_module_texts: &[String],
    execute_statements: Vec<NodeId>,
) -> NodeId {
    let module_body_function = build_system_module_body_function(
        ec,
        exports_param_name,
        context_param_name,
        dependency_module_texts,
        execute_statements,
    );

    // The dependency array: one string-literal entry per external import.
    let dependency_literals: Vec<NodeId> = dependency_module_texts
        .iter()
        .map(|text| {
            ec.arena_mut()
                .new_string_literal(text, tsgo_ast::TokenFlags::NONE)
        })
        .collect();
    let dependencies = ec
        .arena_mut()
        .new_array_literal_expression(NodeList::new(dependency_literals));

    // `System.register`
    let system = ec.arena_mut().new_identifier("System");
    let register = ec.arena_mut().new_identifier("register");
    let callee = ec
        .arena_mut()
        .new_property_access_expression(system, None, register);

    ec.arena_mut().new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![dependencies, module_body_function]),
        NodeFlags::NONE,
    )
}

/// Builds the outer `function (<exports>, <context>) { "use strict"; return {
/// setters: [<setters>], execute: function () { } }; }` module body function.
///
/// Side effects: pushes the parameter/statement/block/function nodes; sets the
/// `MULTI_LINE` emit flag on the body block.
// Go: internal/transformers/moduletransforms/systemmodule.go:createSystemModuleBody
fn build_system_module_body_function(
    ec: &mut EmitContext,
    exports_param_name: NodeId,
    context_param_name: NodeId,
    dependency_module_texts: &[String],
    execute_statements: Vec<NodeId>,
) -> NodeId {
    let exports_param =
        ec.arena_mut()
            .new_parameter_declaration(None, None, exports_param_name, None, None, None);
    let context_param =
        ec.arena_mut()
            .new_parameter_declaration(None, None, context_param_name, None, None, None);

    // `"use strict";`
    let use_strict_literal = ec
        .arena_mut()
        .new_string_literal("use strict", tsgo_ast::TokenFlags::NONE);
    let use_strict_stmt = ec.arena_mut().new_expression_statement(use_strict_literal);

    // `return { setters: [<setters>], execute: function () { <body> } };`
    let module_object = build_system_module_object(ec, dependency_module_texts, execute_statements);
    let return_stmt = ec.arena_mut().new_return_statement(Some(module_object));

    let body = ec
        .arena_mut()
        .new_block(NodeList::new(vec![use_strict_stmt, return_stmt]));
    // Go builds the module body block with `multiLine: true`.
    ec.set_emit_flags(body, EmitFlags::MULTI_LINE);

    ec.arena_mut().new_function_expression(
        None,
        None,
        None,
        None,
        NodeList::new(vec![exports_param, context_param]),
        None,
        None,
        Some(body),
    )
}

/// Builds the `{ setters: [<setters>], execute: function () { <body> } }` object
/// returned by the module body function. One empty setter function is emitted
/// per dependency (the setter *bodies*, which forward imported bindings, are
/// deferred — see the module docs). The `execute` body holds the module's
/// top-level value statements, in source order.
///
/// Side effects: pushes the array/function/property/object nodes.
// Go: internal/transformers/moduletransforms/systemmodule.go:createSystemModuleBody (return object)
fn build_system_module_object(
    ec: &mut EmitContext,
    dependency_module_texts: &[String],
    execute_statements: Vec<NodeId>,
) -> NodeId {
    // `setters: [function (_1) { }, ...]` — one setter per dependency.
    let setter_fns: Vec<NodeId> = dependency_module_texts
        .iter()
        .map(|_| build_empty_setter(ec))
        .collect();
    let setters_array = ec
        .arena_mut()
        .new_array_literal_expression(NodeList::new(setter_fns));
    let setters_name = ec.arena_mut().new_identifier("setters");
    let setters_prop =
        ec.arena_mut()
            .new_property_assignment(None, setters_name, None, None, Some(setters_array));

    // `execute: function () { <body> }`
    let execute_body = ec.arena_mut().new_block(NodeList::new(execute_statements));
    let execute_fn = ec.arena_mut().new_function_expression(
        None,
        None,
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        Some(execute_body),
    );
    let execute_name = ec.arena_mut().new_identifier("execute");
    let execute_prop =
        ec.arena_mut()
            .new_property_assignment(None, execute_name, None, None, Some(execute_fn));

    ec.arena_mut()
        .new_object_literal_expression(NodeList::new(vec![setters_prop, execute_prop]))
}

/// Builds an empty dependency setter `function (_1) { }`.
///
/// The setter *parameter* is the import's local binding name. For a binding-less
/// dependency (e.g. `import "m"`) Go uses `factory.createUniqueName("")`, which
/// tsc renders as `_1`. The Rust name generator's empty-base path diverges (it
/// drops the leading `_`, yielding `1`), so `"_"` is passed to reproduce tsc's
/// `_1`. The setter *body* (which forwards the imported bindings via
/// `exports({...})`) is deferred — see the module docs.
///
/// Side effects: pushes the name/parameter/block/function nodes.
// Go: internal/transformers/moduletransforms/systemmodule.go:createSettersArray
// TODO(port): align the name generator's empty-base path with Go (`"" -> "_1"`).
fn build_empty_setter(ec: &mut EmitContext) -> NodeId {
    let param_name = ec.factory().new_unique_name("_");
    let param = ec
        .arena_mut()
        .new_parameter_declaration(None, None, param_name, None, None, None);
    let body = ec.arena_mut().new_block(NodeList::new(vec![]));
    ec.arena_mut().new_function_expression(
        None,
        None,
        None,
        None,
        NodeList::new(vec![param]),
        None,
        None,
        Some(body),
    )
}

#[cfg(test)]
#[path = "systemmodule_test.rs"]
mod tests;
