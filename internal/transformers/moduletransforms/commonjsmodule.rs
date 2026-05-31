//! Port of Go `internal/transformers/moduletransforms/commonjsmodule.go`: lowers
//! ES module syntax to CommonJS (`require`/`exports`).
//!
//! # Scope (rounds 6e-2 + 6e-3 + 6v + 6w + 6x + 6ad + 6ai)
//!
//! Runs only under `compilerOptions.module == CommonJs` (6e-2 track 2). Lowers,
//! in source order:
//! * **named import + use** — `import { x } from "m"; x;` →
//!   `const m_1 = require("m"); m_1.x;` (use rewritten via 6e-2 node substitution).
//! * **default import** — `import d from "m"` →
//!   `const m_1 = __importDefault(require("m"))`; uses of `d` → `m_1.default`.
//! * **namespace import** — `import * as ns from "m"` →
//!   `const m_1 = __importStar(require("m"))`; uses of `ns` → `m_1`.
//! * **combined default + named import** (6v) — `import d, { x } from "m"` →
//!   `const m_1 = __importStar(require("m"))` (Go `getImportNeedsImportStarHelper`:
//!   default import mixed with non-default named refs); `d` → `m_1.default`,
//!   `x` → `m_1.x`.
//! * **re-export** (6v) — `export { x } from "m"` → `var m_1 = require("m");`
//!   plus a live-binding getter
//!   `Object.defineProperty(exports, "x", { enumerable: true, get: function () { return m_1.x; } });`
//!   (`export { a as b } from "m"` reads the renamed source member `m_1.a`).
//! * **`export default e`** → `exports.default = e`.
//! * **`export = e`** (6w) → `module.exports = e;` (the CommonJS whole-module
//!   form; its `__esModule` marker is suppressed).
//! * **`import x = require("m")`** (6x) → `const x = require("m");` (the
//!   external-module `import =` form, for emit module kind below Node16); the
//!   bound `x` is a real local `const`, so its uses are left unchanged.
//! * **`exports.<name> = void 0;` export-name init** (6x) — every exported name
//!   is zero-initialized right after the `__esModule` marker. Multiple names
//!   share one chained statement (chunked at 50, like Go), folded so source
//!   order `a, b` emits `exports.b = exports.a = void 0;` (last name outermost).
//!   The exported-name set is the reachable subset (`export const`, local /
//!   re-export `export { … }`, and non-default `export class`); exported
//!   functions, `export default`, `export =`, and `export *` are excluded.
//! * **exported function declaration** (6w) — `export function f() {}` → the
//!   kept local `function f() {}` (export modifier stripped) plus a hoisted
//!   `exports.f = f;` (function declarations hoist, so the assignment precedes
//!   the declaration). `export default function f() {}` → `exports.default = f;`.
//! * **exported class declaration** (6w) — `export class C {}` → the kept local
//!   `class C {}` (export modifier stripped) followed in place by `exports.C = C;`
//!   (classes do not hoist). `export default class C {}` → `exports.default = C;`.
//! * **`export const y = 1`** → `exports.y = 1`.
//! * **`export { x }`** (local) → `exports.x = x` (`export { a as b }` → `exports.b = a`).
//! * **`export * from "m"`** → `__exportStar(require("m"), exports)`.
//! * **dynamic `import()`** (6ad) — `import("m")` →
//!   `Promise.resolve().then(() => __importStar(require("m")))`. Go (and
//!   upstream typescript-go) wraps `require(...)` in `__importStar`
//!   *unconditionally* here (independent of `esModuleInterop`), via an arrow
//!   callback. A simple inlineable argument (string literal) is inlined into
//!   `require(...)`; the no-argument `import()` form emits `require()`.
//! * The **`__esModule` marker** (`Object.defineProperty(exports, "__esModule",
//!   { value: true });`) is emitted at the top when the module has value exports.
//! * Interop helpers (`__importDefault`/`__importStar`/`__exportStar`) are
//!   requested and emitted in the module prologue (6d-2 helper infra).
//!
//! # Use-site rewriting (6ai, scope-correct)
//!
//! When the transformer is built via
//! [`new_common_js_module_transformer_with_resolver`] it consults the checker's
//! scope-correct [`EmitReferenceResolver`] (4an's `EmitResolver::resolve_reference`)
//! to rewrite a *use* of an imported binding to its qualified member access:
//! each use identifier is resolved to its declaration symbol (innermost binding
//! wins) and rewritten only when that symbol is one of the collected import
//! bindings'. A use shadowed by a local of the same name resolves to the local,
//! so it is left unchanged (Go's `visitExpressionIdentifier` over
//! `GetReferencedImportDeclaration`). The require-alias (`m_1`) and member name
//! reuse the same scheme the import lowering already emits.
//!
//! Without a resolver (the bare [`new_common_js_module_transformer`]) the
//! transform falls back to a textual name match (the pre-6ai behavior, which a
//! shadowing local would fool).
//!
//! # Divergence from Go / Deferred (DEFER(P5))
//!
//! * Scope-correct use rewriting (6ai) covers named-import uses; default
//!   (`d` → `m_1.default`) and namespace (`ns` → `m_1`) import reference forms
//!   route through the same `decl`-symbol match and are exercised against the
//!   resolver — including the shadowing scope guard — as of 6aj.
//!   DEFER: export reference rewriting (`exports.x` for local exports),
//!   shorthand-property-assignment expansion (`{ x }` → `{ x: m_1.x }`), and the
//!   string-literal-named element-access form. blocked-by: full
//!   `GetReferencedExportContainer` / `markLinkedReferences`.
//! * The require variable name is derived deterministically (`<module>_1`)
//!   rather than via the emit-context name generator. DEFER: collision-free
//!   `NewGeneratedNameForNode`.
//! * The `__esModule` marker is emitted when there are value exports (Go emits
//!   it for any external module, including import-only ones); the full
//!   external-module gating and hoisting/ordering details are not modelled.
//! * The `"use strict"` prologue is NOT inserted here: Go emits it from a
//!   *separate* transformer ([`crate::estransforms::usestrict`]), not the
//!   CommonJS transform. (Earlier briefings placed it here; corrected in 6x.)
//! * `export =`/exported function/class declarations land in 6w, but the
//!   `export =` interplay with `import =`, the function-export hoisting *across
//!   other top-level statements* (Go hoists to a custom prologue ahead of the
//!   whole body; here the assignment is hoisted only ahead of the rebuilt body,
//!   which matches single-declaration modules), and the unnamed
//!   `export default function () {}` / `export default class {}` forms (which
//!   need a generated name) remain deferred.
//! * DEFER: the dynamic-`import()` `needSyncEval` template form (a
//!   non-inlineable argument, e.g. `import(someVar)`, which Go lowers to
//!   `Promise.resolve(`${x}`).then((s) => __importStar(require(s)))`), dynamic
//!   `import()` spread arguments, and the end-to-end parse path (the parser
//!   still defers the `import(...)` call head — blocked-by `internal/parser`),
//!   the `export import x = require("m")` form (Go
//!   lowers it to `exports.x = require(...)`) and Node16+ `import =` (synchronous
//!   `require` helper), live bindings for *local* exports (re-exports now use
//!   the `Object.defineProperty` getter form, but local `export { x }` still
//!   uses the simple `exports.x = x` assignment), `export * as ns from "m"`
//!   namespace re-exports, the `default as`-only named import edge (which Go
//!   routes to `__importDefault` rather than `__importStar`), and string-literal
//!   export names (which Go zero-initializes via element access).

use crate::moduletransforms::externalmoduleinfo::collect_external_module_info;
use crate::{new_transformer, EmitReferenceResolver, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, SymbolId};
use tsgo_core::compileroptions::ModuleKind;
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers ES modules to CommonJS. Only active when
/// `compilerOptions.module == CommonJs`; otherwise the source file is returned
/// unchanged.
///
/// # Examples
/// ```
/// use tsgo_transformers::{moduletransforms::commonjsmodule::new_common_js_module_transformer, TransformOptions};
/// let _tx = new_common_js_module_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:NewCommonJSModuleTransformer
pub fn new_common_js_module_transformer(opt: &TransformOptions) -> Transformer {
    build_common_js_module_transformer(opt, None)
}

/// Like [`new_common_js_module_transformer`], but threads a scope-correct
/// [`EmitReferenceResolver`] so uses of an imported binding are rewritten to a
/// qualified member access on the require-alias only when they *resolve* to the
/// import (Go's `visitExpressionIdentifier` over
/// `GetReferencedImportDeclaration`). A use shadowed by a local of the same name
/// resolves to the local and is left unchanged.
///
/// The resolver is an *additive* parameter (a new constructor variant) rather
/// than a [`TransformOptions`] field or a changed signature, so the
/// emitter-constructed [`new_common_js_module_transformer`] is unaffected; see
/// [`EmitReferenceResolver`] for the rationale. Without a resolver the transform
/// falls back to a textual name match (the pre-6ai behavior).
///
/// # Examples
/// ```
/// use tsgo_transformers::{
///     moduletransforms::commonjsmodule::new_common_js_module_transformer_with_resolver,
///     EmitReferenceResolver, TransformOptions,
/// };
/// # fn demo(resolver: EmitReferenceResolver) {
/// let _tx = new_common_js_module_transformer_with_resolver(&TransformOptions::default(), resolver);
/// # }
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:NewCommonJSModuleTransformer
pub fn new_common_js_module_transformer_with_resolver(
    opt: &TransformOptions,
    resolver: EmitReferenceResolver,
) -> Transformer {
    build_common_js_module_transformer(opt, Some(resolver))
}

/// Shared constructor for the CommonJS transformer, with an optional
/// scope-correct reference resolver.
///
/// Side effects: allocates a transformer over the shared context.
fn build_common_js_module_transformer(
    opt: &TransformOptions,
    resolver: Option<EmitReferenceResolver>,
) -> Transformer {
    // Track 2: the module kind selects whether to lower to CommonJS.
    let is_commonjs = opt.compiler_options.module == ModuleKind::CommonJs;
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            if is_commonjs {
                common_js_visit(ec, node, resolver.as_ref())
            } else {
                node
            }
        }),
        opt.context.clone(),
    )
}

/// Lowers a source file to CommonJS; non-source-file nodes pass through.
///
/// Side effects: may push rebuilt nodes; may register node substitutions.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:CommonJSModuleTransformer.visit
fn common_js_visit(
    ec: &mut EmitContext,
    node: NodeId,
    resolver: Option<&EmitReferenceResolver>,
) -> NodeId {
    if ec.arena().kind(node) == Kind::SourceFile {
        return transform_common_js_module(ec, node, resolver);
    }
    node
}

/// One import binding: the local name and how a use of it is rewritten against
/// the `require` result variable.
struct ImportBinding {
    /// The local imported name (`x`/`d`/`ns`).
    name: String,
    /// The `require` result variable the name now reads from (`m_1`).
    require_var: String,
    /// The member to access on `require_var`: `Some("x")`/`Some("default")` →
    /// `require_var.member`; `None` (namespace import) → `require_var` itself.
    member: Option<String>,
    /// The declaration node that binds this name (the import specifier / import
    /// clause / namespace import). Used for scope-correct use-site matching: a
    /// use's resolved symbol is compared against this declaration's symbol. Only
    /// consulted when a reference resolver is threaded in.
    decl: Option<NodeId>,
}

/// Rebuilds a source file: each external named import becomes a
/// `const <var> = require("mod");` binding, and uses of the imported names are
/// rewritten to `<var>.<name>` member accesses via node substitution.
///
/// Side effects: pushes rebuilt nodes; registers node substitutions.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:transformCommonJSModule
fn transform_common_js_module(
    ec: &mut EmitContext,
    node: NodeId,
    resolver: Option<&EmitReferenceResolver>,
) -> NodeId {
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

    // Use the shared analysis to identify external imports.
    let info = collect_external_module_info(ec.arena(), node);
    let has_exports = module_has_exports(ec.arena(), &statements);
    // Snapshot the exported name texts up front (they drive the `exports.x =
    // void 0;` zero-initializer); collecting now avoids holding an `info` borrow
    // across the later `ec.arena_mut()` node construction.
    let exported_name_texts: Vec<String> = info
        .exported_names
        .iter()
        .map(|&n| ec.arena().text(n).to_string())
        .collect();

    let mut bindings: Vec<ImportBinding> = Vec::new();
    let mut body: Vec<NodeId> = Vec::new();
    // `exports.f = f;` assignments for exported function declarations. Because
    // function declarations are hoisted, Go emits these ahead of the body (right
    // after the `__esModule` marker), so we collect them separately.
    let mut hoisted_function_exports: Vec<NodeId> = Vec::new();
    let mut kept: Vec<NodeId> = Vec::new();
    // Process statements in source order so the output order matches the input.
    for &statement in &statements.nodes {
        match ec.arena().kind(statement) {
            Kind::ImportDeclaration if info.external_imports.contains(&statement) => {
                if let Some(require_stmt) = lower_import_to_require(ec, statement, &mut bindings) {
                    body.push(require_stmt);
                    continue;
                }
                kept.push(statement);
                body.push(statement);
            }
            Kind::ImportEqualsDeclaration => {
                // `import x = require("m")` -> `const x = require("m");`. The
                // bound `x` is a real local `const`, so its uses are left
                // unchanged (no entry pushed to `bindings`).
                if let Some(require_stmt) = lower_import_equals_to_require(ec, statement) {
                    body.push(require_stmt);
                    continue;
                }
                kept.push(statement);
                body.push(statement);
            }
            Kind::ExportAssignment => {
                if let Some(lowered) = lower_export_default(ec, statement) {
                    body.push(lowered);
                    continue;
                }
                kept.push(statement);
                body.push(statement);
            }
            Kind::VariableStatement if statement_has_export_modifier(ec.arena(), statement) => {
                let lowered = lower_export_variable_statement(ec, statement);
                if !lowered.is_empty() {
                    body.extend(lowered);
                    continue;
                }
                kept.push(statement);
                body.push(statement);
            }
            Kind::ExportDeclaration => {
                if let Some(lowered) = lower_export_declaration(ec, statement) {
                    body.extend(lowered);
                    continue;
                }
                kept.push(statement);
                body.push(statement);
            }
            Kind::FunctionDeclaration if declaration_has_export_modifier(ec.arena(), statement) => {
                // Keep the local `function f() {}` (export modifier stripped) and
                // hoist `exports.f = f;` ahead of the body.
                if let Some((decl, export_stmt)) =
                    lower_exported_function_declaration(ec, statement)
                {
                    hoisted_function_exports.push(export_stmt);
                    body.push(decl);
                    continue;
                }
                kept.push(statement);
                body.push(statement);
            }
            Kind::ClassDeclaration if declaration_has_export_modifier(ec.arena(), statement) => {
                // Keep the local `class C {}` (export modifier stripped) followed
                // in place by `exports.C = C;` (classes are not hoisted).
                if let Some((decl, export_stmt)) = lower_exported_class_declaration(ec, statement) {
                    body.push(decl);
                    body.push(export_stmt);
                    continue;
                }
                kept.push(statement);
                body.push(statement);
            }
            _ => {
                kept.push(statement);
                body.push(statement);
            }
        }
    }

    // Register substitutions for uses of imported names in the kept statements,
    // and lower any dynamic `import(...)` calls they contain.
    for &statement in &kept {
        register_import_use_substitutions(ec, statement, &bindings, resolver);
        register_dynamic_import_substitutions(ec, statement);
    }

    let mut out: Vec<NodeId> = Vec::new();
    // `Object.defineProperty(exports, "__esModule", { value: true });` at the top
    // when the module has exports.
    if has_exports {
        out.push(make_es_module_marker(ec));
    }
    // `exports.<name> = void 0;` zero-initializer for the exported names (Go
    // emits this right after the `__esModule` marker).
    out.extend(make_exports_void_zero_inits(ec, &exported_name_texts));
    out.extend(hoisted_function_exports);
    out.extend(body);

    let new_source_file = ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(out),
        end_of_file_token,
    );
    // Attach any helpers requested during lowering (interop helpers) so the
    // printer emits their definitions in the module prologue.
    for helper in ec.read_emit_helpers() {
        ec.add_emit_helper(new_source_file, helper);
    }
    new_source_file
}

/// Reports whether the module has any value export (which triggers the
/// `__esModule` marker). An `export =` does **not** count (and is deferred).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/moduletransforms/commonjsmodule.go:shouldEmitUnderscoreUnderscoreESModule
fn module_has_exports(arena: &NodeArena, statements: &NodeList) -> bool {
    statements.nodes.iter().any(|&s| match arena.data(s) {
        NodeData::ExportDeclaration(_) => true,
        NodeData::ExportAssignment(d) => !d.is_export_equals,
        NodeData::VariableStatement(d) => d
            .modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::EXPORT)),
        // `export function f() {}` / `export class C {}` are value exports.
        NodeData::FunctionDeclaration(d) => d
            .modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::EXPORT)),
        NodeData::ClassDeclaration(d) => d
            .modifiers
            .as_ref()
            .is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::EXPORT)),
        _ => false,
    })
}

/// Builds `Object.defineProperty(exports, "__esModule", { value: true });`.
///
/// Side effects: pushes the access/literal/object/call/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:createUnderscoreUnderscoreESModule
fn make_es_module_marker(ec: &mut EmitContext) -> NodeId {
    let arena = ec.arena_mut();
    let object = arena.new_identifier("Object");
    let define_property = arena.new_identifier("defineProperty");
    let callee = arena.new_property_access_expression(object, None, define_property);
    let exports = arena.new_identifier("exports");
    let key = arena.new_string_literal("__esModule", tsgo_ast::TokenFlags::NONE);
    let value_name = arena.new_identifier("value");
    let value_true = arena.new_keyword_expression(Kind::TrueKeyword);
    let value_prop = arena.new_property_assignment(None, value_name, None, None, Some(value_true));
    let descriptor = arena.new_object_literal_expression(NodeList::new(vec![value_prop]));
    let call = arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![exports, key, descriptor]),
        NodeFlags::NONE,
    );
    arena.new_expression_statement(call)
}

/// Lowers an `ExportAssignment`: `export default <expr>` →
/// `exports.default = <expr>;`, and `export = <expr>` → `module.exports =
/// <expr>;` (the `export =` whole-module form, whose `__esModule` marker is
/// suppressed by [`module_has_exports`]).
///
/// Side effects: pushes the access/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitTopLevelExportAssignment + appendExportEqualsIfNeeded
fn lower_export_default(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let (is_export_equals, expression) = match ec.arena().data(node) {
        NodeData::ExportAssignment(d) => (d.is_export_equals, d.expression),
        _ => return None,
    };
    if is_export_equals {
        // `export = x` -> `module.exports = x;`.
        return Some(make_module_exports_assignment(ec, expression));
    }
    Some(make_exports_assignment(ec, "default", expression))
}

/// Reports whether a statement carries the `export` modifier.
///
/// Side effects: none (reads the arena).
fn statement_has_export_modifier(arena: &NodeArena, statement: NodeId) -> bool {
    let modifiers = match arena.data(statement) {
        NodeData::VariableStatement(d) => d.modifiers.clone(),
        _ => return false,
    };
    modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::EXPORT))
}

/// Reports whether a function/class declaration carries the `export` modifier.
///
/// Side effects: none (reads the arena).
fn declaration_has_export_modifier(arena: &NodeArena, declaration: NodeId) -> bool {
    let modifiers = match arena.data(declaration) {
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        _ => return false,
    };
    modifiers.is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::EXPORT))
}

/// Lowers `export function f() {}` to the kept local `function f() {}` (with the
/// `export`/`default` modifiers stripped) plus an `exports.<name> = <name>;`
/// assignment. For `export default function f() {}` the export name is
/// `default`. Returns `None` for an unnamed declaration (deferred — needs a
/// generated name).
///
/// Side effects: rebuilds the function declaration; pushes the export
/// assignment nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitTopLevelFunctionDeclaration + appendExportsOfClassOrFunctionDeclaration
fn lower_exported_function_declaration(
    ec: &mut EmitContext,
    statement: NodeId,
) -> Option<(NodeId, NodeId)> {
    let (modifiers, asterisk_token, name, parameters, type_node, full_signature, body) =
        match ec.arena().data(statement) {
            NodeData::FunctionDeclaration(d) => (
                d.modifiers.clone(),
                d.asterisk_token,
                d.name,
                d.parameters.clone(),
                d.type_node,
                d.full_signature,
                d.body,
            ),
            _ => return None,
        };
    let name = name?;
    let is_default = modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::DEFAULT));
    let name_text = ec.arena().text(name).to_string();
    // Strip the `export`/`default` modifiers; keep any others (e.g. `async`).
    let kept_modifiers = crate::extract_modifiers(
        ec,
        modifiers.as_ref(),
        !tsgo_ast::ModifierFlags::EXPORT_DEFAULT,
    );
    let decl = ec.arena_mut().new_function_declaration(
        kept_modifiers,
        asterisk_token,
        Some(name),
        None,
        parameters,
        type_node,
        full_signature,
        body,
    );
    // `export default function f(){}` -> `exports.default = f;`; otherwise
    // `exports.f = f;`.
    let export_name = if is_default { "default" } else { &name_text };
    let value = ec.arena_mut().new_identifier(&name_text);
    let export_stmt = make_exports_assignment(ec, export_name, value);
    Some((decl, export_stmt))
}

/// Lowers `export class C {}` to the kept local `class C {}` (with the
/// `export`/`default` modifiers stripped) plus an `exports.<name> = <name>;`
/// assignment. For `export default class C {}` the export name is `default`.
/// Returns `None` for an unnamed declaration (deferred — needs a generated name).
///
/// Side effects: rebuilds the class declaration; pushes the export assignment
/// nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitTopLevelClassDeclaration + appendExportsOfClassOrFunctionDeclaration
fn lower_exported_class_declaration(
    ec: &mut EmitContext,
    statement: NodeId,
) -> Option<(NodeId, NodeId)> {
    let (modifiers, name, type_parameters, heritage_clauses, members) =
        match ec.arena().data(statement) {
            NodeData::ClassDeclaration(d) => (
                d.modifiers.clone(),
                d.name,
                d.type_parameters.clone(),
                d.heritage_clauses.clone(),
                d.members.clone(),
            ),
            _ => return None,
        };
    let name = name?;
    let is_default = modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::DEFAULT));
    let name_text = ec.arena().text(name).to_string();
    let kept_modifiers = crate::extract_modifiers(
        ec,
        modifiers.as_ref(),
        !tsgo_ast::ModifierFlags::EXPORT_DEFAULT,
    );
    let decl = ec.arena_mut().new_class_like(
        Kind::ClassDeclaration,
        kept_modifiers,
        Some(name),
        type_parameters,
        heritage_clauses,
        members,
    );
    let export_name = if is_default { "default" } else { &name_text };
    let value = ec.arena_mut().new_identifier(&name_text);
    let export_stmt = make_exports_assignment(ec, export_name, value);
    Some((decl, export_stmt))
}

/// Lowers `export const a = 1, b = 2;` to `exports.a = 1; exports.b = 2;`.
/// Only identifier-named declarations with initializers are handled; other
/// declarations are dropped from the result (returning an empty vec defers the
/// whole statement). Function/class-valued initializers (which keep a local
/// binding in Go) are deferred.
///
/// Side effects: pushes the access/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitTopLevelVariableStatement
fn lower_export_variable_statement(ec: &mut EmitContext, statement: NodeId) -> Vec<NodeId> {
    let declaration_list = match ec.arena().data(statement) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => return Vec::new(),
    };
    let declarations = match ec.arena().data(declaration_list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
        _ => return Vec::new(),
    };
    let mut out = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let (name, initializer) = match ec.arena().data(declaration) {
            NodeData::VariableDeclaration(d) => (d.name, d.initializer),
            _ => return Vec::new(),
        };
        // Only `export const x = <init>` with an identifier name is handled;
        // binding patterns / no-initializer / function-class initializers defer.
        if ec.arena().kind(name) != Kind::Identifier {
            return Vec::new();
        }
        let Some(initializer) = initializer else {
            return Vec::new();
        };
        let name_text = ec.arena().text(name).to_string();
        out.push(make_exports_assignment(ec, &name_text, initializer));
    }
    out
}

/// Lowers an `export { a, b as c };` (no module specifier) to
/// `exports.a = a; exports.c = b;`. For declarations *with* a module specifier:
/// `export * from "m"` → `__exportStar(...)`, and `export { x } from "m"` /
/// `export { a as b } from "m"` → a `require` binding plus live-binding getters
/// (`export * as ns from "m"` namespace re-exports are deferred).
///
/// Side effects: pushes the access/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:appendExportsOfDeclaration
fn lower_export_declaration(ec: &mut EmitContext, node: NodeId) -> Option<Vec<NodeId>> {
    let (export_clause, module_specifier) = match ec.arena().data(node) {
        NodeData::ExportDeclaration(d) => (d.export_clause, d.module_specifier),
        _ => return None,
    };
    if let Some(module_specifier) = module_specifier {
        if ec.arena().kind(module_specifier) != Kind::StringLiteral {
            return None;
        }
        let module_text = ec.arena().text(module_specifier).to_string();
        let Some(clause) = export_clause else {
            // export * from "m" -> __exportStar(require("m"), exports);
            return Some(vec![make_export_star(ec, &module_text)]);
        };
        // export { x } from "m" / export { a as b } from "m" (re-export):
        //   var m_1 = require("m");
        //   Object.defineProperty(exports, "x", { enumerable: true, get: function () { return m_1.x; } });
        // (`export * as ns from "m"` NamespaceExport is deferred.)
        let elements = match ec.arena().data(clause) {
            NodeData::NamedExports(d) => d.elements.nodes.clone(),
            _ => return None,
        };
        // Deterministic require var name (`m_1`); a generated unique name is deferred.
        let require_var = format!("{module_text}_1");
        let require_call = build_require_call(ec, &module_text);
        let mut out = vec![build_var_binding(ec, &require_var, require_call)];
        for specifier in elements {
            let (property_name, name) = match ec.arena().data(specifier) {
                NodeData::ExportSpecifier(d) => (d.property_name, d.name),
                _ => return None,
            };
            // `export { a as b }` -> exports.b getter reads `m_1.a`;
            // `export { x }` -> exports.x getter reads `m_1.x`.
            let export_name = ec.arena().text(name).to_string();
            let member_text = ec.arena().text(property_name.unwrap_or(name)).to_string();
            let object = ec.arena_mut().new_identifier(&require_var);
            let member = ec.arena_mut().new_identifier(&member_text);
            let value = ec
                .arena_mut()
                .new_property_access_expression(object, None, member);
            out.push(make_live_binding_export(ec, &export_name, value));
        }
        return Some(out);
    }
    let clause = export_clause?;
    let elements = match ec.arena().data(clause) {
        NodeData::NamedExports(d) => d.elements.nodes.clone(),
        _ => return None,
    };
    let mut out = Vec::with_capacity(elements.len());
    for specifier in elements {
        let (property_name, name) = match ec.arena().data(specifier) {
            NodeData::ExportSpecifier(d) => (d.property_name, d.name),
            _ => return None,
        };
        // `export { a as b }` -> `exports.b = a`; `export { x }` -> `exports.x = x`.
        let export_name = ec.arena().text(name).to_string();
        let local_text = ec.arena().text(property_name.unwrap_or(name)).to_string();
        let value = ec.arena_mut().new_identifier(&local_text);
        out.push(make_exports_assignment(ec, &export_name, value));
    }
    Some(out)
}

/// Builds `__exportStar(require("<module>"), exports);` (requesting the helper).
///
/// Side effects: requests the `__exportStar` helper; pushes the call/statement.
// Go: internal/printer/factory.go:NodeFactory.NewExportStarHelper
fn make_export_star(ec: &mut EmitContext, module_text: &str) -> NodeId {
    ec.request_emit_helper(&tsgo_printer::emithelpers::EXPORT_STAR_HELPER);
    let require_call = build_require_call(ec, module_text);
    let exports = ec.arena_mut().new_identifier("exports");
    let callee = ec.factory().new_unscoped_helper_name("__exportStar");
    let call = ec.arena_mut().new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![require_call, exports]),
        NodeFlags::NONE,
    );
    ec.arena_mut().new_expression_statement(call)
}

/// Builds `module.exports = <value>;` (the `export =` whole-module form).
///
/// Side effects: pushes the access/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:appendExportEqualsIfNeeded
fn make_module_exports_assignment(ec: &mut EmitContext, value: NodeId) -> NodeId {
    let arena = ec.arena_mut();
    let module = arena.new_identifier("module");
    let exports = arena.new_identifier("exports");
    let target = arena.new_property_access_expression(module, None, exports);
    let equals = arena.new_token(Kind::EqualsToken);
    let assignment = arena.new_binary_expression(target, equals, value);
    arena.new_expression_statement(assignment)
}

/// Builds `exports.<name> = <value>;`.
///
/// Side effects: pushes the access/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:createExportExpression (non-live-binding)
fn make_exports_assignment(ec: &mut EmitContext, name: &str, value: NodeId) -> NodeId {
    let arena = ec.arena_mut();
    let exports = arena.new_identifier("exports");
    let name_id = arena.new_identifier(name);
    let target = arena.new_property_access_expression(exports, None, name_id);
    let equals = arena.new_token(Kind::EqualsToken);
    let assignment = arena.new_binary_expression(target, equals, value);
    arena.new_expression_statement(assignment)
}

/// Builds the export-name zero-initializer statements
/// (`exports.<name> = void 0;`). Names are folded into a single chained
/// assignment per chunk of 50 (matching Go), with each name applied as the
/// outer assignment target, so source order `a, b` yields
/// `exports.b = exports.a = void 0;` (the last name is outermost). Returns an
/// empty vec when there are no exported names.
///
/// Side effects: pushes the access/void/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:transformCommonJSModule (exportedNames init)
fn make_exports_void_zero_inits(ec: &mut EmitContext, names: &[String]) -> Vec<NodeId> {
    const CHUNK_SIZE: usize = 50;
    let mut out = Vec::new();
    for chunk in names.chunks(CHUNK_SIZE) {
        let arena = ec.arena_mut();
        let zero = arena.new_numeric_literal("0", tsgo_ast::TokenFlags::NONE);
        let mut right = arena.new_void_expression(zero);
        for name in chunk {
            let exports = arena.new_identifier("exports");
            let name_id = arena.new_identifier(name);
            let left = arena.new_property_access_expression(exports, None, name_id);
            let equals = arena.new_token(Kind::EqualsToken);
            right = arena.new_binary_expression(left, equals, right);
        }
        out.push(arena.new_expression_statement(right));
    }
    out
}

/// Lowers `import { a, b } from "mod"` to `const mod_1 = require("mod");`,
/// recording each named binding. Returns `None` (leaving the import for the
/// deferred fuller port) for default/namespace imports or non-string-literal /
/// non-identifier-derivable module specifiers.
///
/// Side effects: pushes rebuilt nodes; appends to `bindings`.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitTopLevelImportDeclaration
fn lower_import_to_require(
    ec: &mut EmitContext,
    import_decl: NodeId,
    bindings: &mut Vec<ImportBinding>,
) -> Option<NodeId> {
    let (import_clause, module_specifier) = match ec.arena().data(import_decl) {
        NodeData::ImportDeclaration(d) => (d.import_clause, d.module_specifier),
        _ => return None,
    };
    if ec.arena().kind(module_specifier) != Kind::StringLiteral {
        return None;
    }
    let module_text = ec.arena().text(module_specifier).to_string();
    // Deterministic require var name (`m_1`); a generated unique name is deferred.
    let require_var = format!("{module_text}_1");

    let clause = import_clause?;
    let (default_name, named_bindings) = match ec.arena().data(clause) {
        NodeData::ImportClause(d) => (d.name, d.named_bindings),
        _ => return None,
    };

    // `require("m")`, then wrap with an interop helper as needed.
    let require_call = build_require_call(ec, &module_text);

    let rhs = match named_bindings.map(|b| ec.arena().kind(b)) {
        Some(Kind::NamespaceImport) => {
            // import * as ns from "m" -> const m_1 = __importStar(require("m"));
            let ns_name = match named_bindings.map(|b| ec.arena().data(b)) {
                Some(NodeData::NamespaceImport(d)) => d.name,
                _ => return None,
            };
            let ns_text = ec.arena().text(ns_name).to_string();
            bindings.push(ImportBinding {
                name: ns_text,
                require_var: require_var.clone(),
                member: None,
                decl: named_bindings,
            });
            ec.request_emit_helper(&tsgo_printer::emithelpers::IMPORT_STAR_HELPER);
            wrap_in_helper(ec, "__importStar", require_call)
        }
        Some(Kind::NamedImports) => {
            // import { x, y } from "m" -> const m_1 = require("m"); use x -> m_1.x
            let elements = match named_bindings.map(|b| ec.arena().data(b)) {
                Some(NodeData::NamedImports(d)) => d.elements.nodes.clone(),
                _ => return None,
            };
            // Combined `import d, { x } from "m"`: a default import mixed with
            // non-default named refs requires the `__importStar` interop helper
            // (Go `getImportNeedsImportStarHelper`). `d` -> `m_1.default`.
            let has_default = default_name.is_some();
            if let Some(default_name) = default_name {
                let default_text = ec.arena().text(default_name).to_string();
                bindings.push(ImportBinding {
                    name: default_text,
                    require_var: require_var.clone(),
                    member: Some("default".to_string()),
                    decl: Some(clause),
                });
            }
            for specifier in elements {
                if let NodeData::ImportSpecifier(d) = ec.arena().data(specifier) {
                    let name = ec.arena().text(d.name).to_string();
                    bindings.push(ImportBinding {
                        name: name.clone(),
                        require_var: require_var.clone(),
                        member: Some(name),
                        decl: Some(specifier),
                    });
                }
            }
            if has_default {
                ec.request_emit_helper(&tsgo_printer::emithelpers::IMPORT_STAR_HELPER);
                wrap_in_helper(ec, "__importStar", require_call)
            } else {
                require_call
            }
        }
        _ => {
            // import d from "m" -> const m_1 = __importDefault(require("m")); use d -> m_1.default
            let default_name = default_name?;
            let default_text = ec.arena().text(default_name).to_string();
            bindings.push(ImportBinding {
                name: default_text,
                require_var: require_var.clone(),
                member: Some("default".to_string()),
                decl: Some(clause),
            });
            ec.request_emit_helper(&tsgo_printer::emithelpers::IMPORT_DEFAULT_HELPER);
            wrap_in_helper(ec, "__importDefault", require_call)
        }
    };

    Some(build_const_binding(ec, &require_var, rhs))
}

/// Lowers `import x = require("m")` to `const x = require("m");` (for emit
/// module kind below Node16). Returns `None` for an internal module reference
/// (`import x = a.b`, handled in an earlier transformer in Go) or an exported
/// `export import x = require("m")` (which Go lowers to `exports.x =
/// require(...)`; deferred), leaving the statement for the caller to keep.
///
/// Side effects: pushes the identifier/declaration/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitTopLevelImportEqualsDeclaration
fn lower_import_equals_to_require(ec: &mut EmitContext, statement: NodeId) -> Option<NodeId> {
    let (modifiers, name, module_reference) = match ec.arena().data(statement) {
        NodeData::ImportEqualsDeclaration(d) => (d.modifiers.clone(), d.name, d.module_reference),
        _ => return None,
    };
    // `export import x = require("m")` -> `exports.x = require(...)`: deferred.
    if modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(tsgo_ast::ModifierFlags::EXPORT))
    {
        return None;
    }
    // Only the external-module form `= require("m")` is lowered here.
    let module_specifier = match ec.arena().data(module_reference) {
        NodeData::ExternalModuleReference(d) => d.expression,
        _ => return None,
    };
    if ec.arena().kind(module_specifier) != Kind::StringLiteral {
        return None;
    }
    let module_text = ec.arena().text(module_specifier).to_string();
    let name_text = ec.arena().text(name).to_string();
    let require_call = build_require_call(ec, &module_text);
    Some(build_const_binding(ec, &name_text, require_call))
}

/// Wraps `argument` in a call to the named interop helper:
/// `<helper>(<argument>)` (e.g. `__importDefault(require("m"))`).
///
/// Side effects: pushes the helper-name/call nodes onto the arena.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:getHelperExpressionForImport
fn wrap_in_helper(ec: &mut EmitContext, helper_name: &str, argument: NodeId) -> NodeId {
    let callee = ec.factory().new_unscoped_helper_name(helper_name);
    ec.arena_mut().new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![argument]),
        NodeFlags::NONE,
    )
}

/// Builds the `require("<module>")` call expression.
///
/// Side effects: pushes the identifier/literal/call nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:createRequireCall
fn build_require_call(ec: &mut EmitContext, module_text: &str) -> NodeId {
    let arena = ec.arena_mut();
    let require_fn = arena.new_identifier("require");
    let module_literal = arena.new_string_literal(module_text, tsgo_ast::TokenFlags::NONE);
    arena.new_call_expression(
        require_fn,
        None,
        None,
        NodeList::new(vec![module_literal]),
        NodeFlags::NONE,
    )
}

/// Builds `const <var> = <rhs>;`.
///
/// Side effects: pushes the identifier/declaration/statement nodes.
fn build_const_binding(ec: &mut EmitContext, var_name: &str, rhs: NodeId) -> NodeId {
    build_binding(ec, var_name, rhs, true)
}

/// Builds `var <var> = <rhs>;` (no `const` flag). Used for re-export `require`
/// bindings, which Go emits with `NodeFlagsNone`.
///
/// Side effects: pushes the identifier/declaration/statement nodes.
fn build_var_binding(ec: &mut EmitContext, var_name: &str, rhs: NodeId) -> NodeId {
    build_binding(ec, var_name, rhs, false)
}

/// Builds `<const|var> <var> = <rhs>;` depending on `is_const`.
///
/// Side effects: pushes the identifier/declaration/statement nodes.
fn build_binding(ec: &mut EmitContext, var_name: &str, rhs: NodeId, is_const: bool) -> NodeId {
    let arena = ec.arena_mut();
    let name = arena.new_identifier(var_name);
    let declaration = arena.new_variable_declaration(name, None, None, Some(rhs));
    let declaration_list = arena.new_variable_declaration_list(NodeList::new(vec![declaration]));
    if is_const {
        arena.add_flags(declaration_list, NodeFlags::CONST);
    }
    arena.new_variable_statement(None, declaration_list)
}

/// Builds the live-binding export getter for a re-export:
/// `Object.defineProperty(exports, "<name>", { enumerable: true, get: function () { return <value>; } });`.
///
/// Side effects: pushes the access/literal/object/function/call/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:createExportExpression (liveBinding)
fn make_live_binding_export(ec: &mut EmitContext, name: &str, value: NodeId) -> NodeId {
    let arena = ec.arena_mut();
    // get: function () { return <value>; }
    let return_stmt = arena.new_return_statement(Some(value));
    let body = arena.new_block(NodeList::new(vec![return_stmt]));
    let getter = arena.new_function_expression(
        None,
        None,
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        Some(body),
    );
    let enumerable_name = arena.new_identifier("enumerable");
    let true_value = arena.new_keyword_expression(Kind::TrueKeyword);
    let enumerable_prop =
        arena.new_property_assignment(None, enumerable_name, None, None, Some(true_value));
    let get_name = arena.new_identifier("get");
    let get_prop = arena.new_property_assignment(None, get_name, None, None, Some(getter));
    let descriptor =
        arena.new_object_literal_expression(NodeList::new(vec![enumerable_prop, get_prop]));
    // Object.defineProperty(exports, "<name>", <descriptor>)
    let object = arena.new_identifier("Object");
    let define_property = arena.new_identifier("defineProperty");
    let callee = arena.new_property_access_expression(object, None, define_property);
    let exports = arena.new_identifier("exports");
    let key = arena.new_string_literal(name, tsgo_ast::TokenFlags::NONE);
    let call = arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![exports, key, descriptor]),
        NodeFlags::NONE,
    );
    arena.new_expression_statement(call)
}

/// Registers a node substitution for each identifier *use* in `statement` that
/// refers to an imported binding, rewriting `x` → `<require_var>.x`.
///
/// With a `resolver` the match is scope-correct: each use is resolved to its
/// declaration symbol (innermost binding wins) and compared against the import
/// bindings' declaration symbols, so a use shadowed by a local of the same name
/// is left unchanged (Go's `visitExpressionIdentifier` over
/// `GetReferencedImportDeclaration`). Without a resolver the match falls back to
/// a textual name comparison (the pre-6ai behavior, which a shadowing local
/// would fool).
///
/// Side effects: pushes member-access nodes; registers node substitutions.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitIdentifier (use-site rewrite)
fn register_import_use_substitutions(
    ec: &mut EmitContext,
    statement: NodeId,
    bindings: &[ImportBinding],
    resolver: Option<&EmitReferenceResolver>,
) {
    let mut uses: Vec<NodeId> = Vec::new();
    collect_identifiers(ec.arena(), statement, &mut uses);
    match resolver {
        Some(resolver) => {
            // Scope-correct: precompute each binding's declaration symbol, then
            // match a use's resolved symbol against them. A use shadowed by a
            // local of the same name resolves to the local (a different symbol),
            // so it is left unchanged.
            let binding_syms: Vec<Option<SymbolId>> = bindings
                .iter()
                .map(|b| b.decl.and_then(|d| resolver.symbol_of_declaration(d)))
                .collect();
            for use_node in uses {
                // Go: visitExpressionIdentifier checks GetReferencedExportContainer
                // first. A use that resolves to a top-level *exported variable* of
                // the current module has the source file as its export container,
                // so it is qualified into an `exports.<name>` access. CommonJS
                // passes `prefixLocals = false` (a plain use is not an export
                // name), so exported functions/classes are referenced unqualified
                // and yield `None`.
                if let Some(container) = resolver.get_referenced_export_container(use_node, false) {
                    if ec.arena().kind(container) == Kind::SourceFile {
                        substitute_exported_name_use(ec, use_node);
                        continue;
                    }
                }
                let Some(symbol) = resolver.resolve_reference(use_node) else {
                    continue;
                };
                if let Some(index) = binding_syms.iter().position(|&s| s == Some(symbol)) {
                    substitute_import_use(ec, use_node, &bindings[index]);
                }
            }
        }
        None => {
            for use_node in uses {
                let text = ec.arena().text(use_node).to_string();
                if let Some(binding) = bindings.iter().find(|b| b.name == text) {
                    substitute_import_use(ec, use_node, binding);
                }
            }
        }
    }
}

/// Registers the use-site substitution for one resolved import binding:
/// `x` → `<require_var>.x` (named/default) or `ns` → `<require_var>` (namespace).
///
/// Side effects: pushes member-access nodes; registers a node substitution.
fn substitute_import_use(ec: &mut EmitContext, use_node: NodeId, binding: &ImportBinding) {
    let require_var = binding.require_var.clone();
    let substitute = match &binding.member {
        // Named/default import: `x` -> `m_1.x` / `d` -> `m_1.default`.
        Some(member) => {
            let member = member.clone();
            let object = ec.arena_mut().new_identifier(&require_var);
            let name = ec.arena_mut().new_identifier(&member);
            ec.arena_mut()
                .new_property_access_expression(object, None, name)
        }
        // Namespace import: `ns` -> `m_1`.
        None => ec.arena_mut().new_identifier(&require_var),
    };
    ec.set_node_substitution(use_node, substitute);
}

/// Registers the use-site substitution for a use of a top-level exported
/// variable: `x` → `exports.x` (a property access on the `exports` object,
/// reusing the same `exports` identifier the export assignments reference).
///
/// Side effects: pushes member-access nodes; registers a node substitution.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitExpressionIdentifier (export-container rewrite)
fn substitute_exported_name_use(ec: &mut EmitContext, use_node: NodeId) {
    let name = ec.arena().text(use_node).to_string();
    let exports = ec.arena_mut().new_identifier("exports");
    let name_id = ec.arena_mut().new_identifier(&name);
    let substitute = ec
        .arena_mut()
        .new_property_access_expression(exports, None, name_id);
    ec.set_node_substitution(use_node, substitute);
}

/// Collects all `Identifier` node ids in the subtree rooted at `node`.
///
/// Side effects: appends to `out`.
fn collect_identifiers(arena: &NodeArena, node: NodeId, out: &mut Vec<NodeId>) {
    if arena.kind(node) == Kind::Identifier {
        out.push(node);
        return;
    }
    arena.for_each_child(node, &mut |child| {
        collect_identifiers(arena, child, out);
        false
    });
}

/// Registers a node substitution lowering each dynamic `import(...)` call within
/// `statement` to its CommonJS form (see [`lower_dynamic_import_call`]). Calls
/// whose lowering is deferred (e.g. a non-inlineable argument) are left
/// unchanged.
///
/// Side effects: pushes the lowered nodes; requests the `__importStar` helper;
/// registers node substitutions.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitCallExpression (import-call branch)
fn register_dynamic_import_substitutions(ec: &mut EmitContext, statement: NodeId) {
    let mut calls: Vec<NodeId> = Vec::new();
    collect_import_calls(ec.arena(), statement, &mut calls);
    for call in calls {
        if let Some(lowered) = lower_dynamic_import_call(ec, call) {
            ec.set_node_substitution(call, lowered);
        }
    }
}

/// Collects all dynamic `import(...)` call node ids in the subtree rooted at
/// `node`.
///
/// Side effects: appends to `out`.
fn collect_import_calls(arena: &NodeArena, node: NodeId, out: &mut Vec<NodeId>) {
    if is_import_call(arena, node) {
        out.push(node);
    }
    arena.for_each_child(node, &mut |child| {
        collect_import_calls(arena, child, out);
        false
    });
}

/// Reports whether `node` is a dynamic `import(...)` call: a `CallExpression`
/// whose callee is the `import` keyword expression.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsImportCall
fn is_import_call(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::CallExpression(d) if arena.kind(d.expression) == Kind::ImportKeyword)
}

/// Reports whether `expr` is a "simple inlineable" expression that can be
/// inlined directly into the generated `require(...)` call without a
/// `Promise.resolve(`${x}`)` evaluation wrapper. Mirrors Go
/// `isSimpleInlineableExpression` (`!IsIdentifier && IsSimpleCopiableExpression`):
/// a string/numeric/keyword literal, but never a bare identifier. The keyword
/// set here is the literal-like subset reachable in this round (the full Go
/// predicate accepts any keyword via `IsKeywordKind`).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/moduletransforms/utilities.go:isSimpleInlineableExpression
fn is_simple_inlineable_expression(arena: &NodeArena, expr: NodeId) -> bool {
    matches!(
        arena.kind(expr),
        Kind::StringLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
    )
}

/// Lowers a dynamic `import(<arg>)` call to its CommonJS form
/// `Promise.resolve().then(() => __importStar(require(<arg>)))`.
///
/// Mirrors Go `createImportCallExpressionCommonJS`, which ALWAYS wraps the
/// `require(...)` call in the `__importStar` helper — independent of
/// `esModuleInterop` (upstream `NewImportStarHelper` is called unconditionally).
/// This round handles the no-argument `import()` form (`require()` with no
/// args) and the simple-inlineable-argument case (a string literal, inlined into
/// `require(...)`) with no `Promise.resolve(`${x}`)` template / `(s) =>`
/// evaluation wrapper. Returns `None` (deferring, leaving the call unchanged)
/// for a non-inlineable argument (the `needSyncEval` template form).
///
/// Side effects: requests the `__importStar` helper; pushes the
/// identifier/access/call/arrow nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:createImportCallExpressionCommonJS
fn lower_dynamic_import_call(ec: &mut EmitContext, call: NodeId) -> Option<NodeId> {
    let arg = match ec.arena().data(call) {
        NodeData::CallExpression(d) => d.arguments.nodes.first().copied(),
        _ => return None,
    };
    let require_args = match arg {
        // The no-argument `import()` form: `require()` with an empty argument list.
        None => Vec::new(),
        Some(arg) => {
            // DEFER: the `needSyncEval` path for non-inlineable arguments (which
            // Go evaluates via a `Promise.resolve(`${x}`).then((s) => ...
            // require(s))` template) is not handled this round.
            if !is_simple_inlineable_expression(ec.arena(), arg) {
                return None;
            }
            vec![arg]
        }
    };
    Some(build_downleveled_import(ec, require_args))
}

/// Builds `Promise.resolve().then(() => __importStar(require(<args>)))` for a
/// dynamic import whose argument has already been resolved (or is empty).
///
/// Side effects: requests the `__importStar` helper; pushes the
/// identifier/access/call/arrow nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:createImportCallExpressionCommonJS
fn build_downleveled_import(ec: &mut EmitContext, require_args: Vec<NodeId>) -> NodeId {
    // require(<args>)
    let require_fn = ec.arena_mut().new_identifier("require");
    let require_call_inner = ec.arena_mut().new_call_expression(
        require_fn,
        None,
        None,
        NodeList::new(require_args),
        NodeFlags::NONE,
    );
    // __importStar(require(<args>)) — Go wraps unconditionally.
    ec.request_emit_helper(&tsgo_printer::emithelpers::IMPORT_STAR_HELPER);
    let import_star = wrap_in_helper(ec, "__importStar", require_call_inner);
    // () => __importStar(require(<args>))
    let arrow_token = ec.arena_mut().new_token(Kind::EqualsGreaterThanToken);
    let arrow = ec.arena_mut().new_arrow_function(
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        arrow_token,
        import_star,
    );
    // Promise.resolve()
    let promise = ec.arena_mut().new_identifier("Promise");
    let resolve = ec.arena_mut().new_identifier("resolve");
    let resolve_access = ec
        .arena_mut()
        .new_property_access_expression(promise, None, resolve);
    let promise_resolve_call = ec.arena_mut().new_call_expression(
        resolve_access,
        None,
        None,
        NodeList::new(vec![]),
        NodeFlags::NONE,
    );
    // Promise.resolve().then(() => ...)
    let then = ec.arena_mut().new_identifier("then");
    let then_access =
        ec.arena_mut()
            .new_property_access_expression(promise_resolve_call, None, then);
    ec.arena_mut().new_call_expression(
        then_access,
        None,
        None,
        NodeList::new(vec![arrow]),
        NodeFlags::NONE,
    )
}

#[cfg(test)]
#[path = "commonjsmodule_test.rs"]
mod tests;
