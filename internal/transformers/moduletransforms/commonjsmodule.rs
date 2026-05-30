//! Port of Go `internal/transformers/moduletransforms/commonjsmodule.go`: lowers
//! ES module syntax to CommonJS (`require`/`exports`).
//!
//! # Scope (rounds 6e-2 + 6e-3)
//!
//! Runs only under `compilerOptions.module == CommonJs` (6e-2 track 2). Lowers,
//! in source order:
//! * **named import + use** — `import { x } from "m"; x;` →
//!   `const m_1 = require("m"); m_1.x;` (use rewritten via 6e-2 node substitution).
//! * **default import** — `import d from "m"` →
//!   `const m_1 = __importDefault(require("m"))`; uses of `d` → `m_1.default`.
//! * **namespace import** — `import * as ns from "m"` →
//!   `const m_1 = __importStar(require("m"))`; uses of `ns` → `m_1`.
//! * **`export default e`** → `exports.default = e`.
//! * **`export const y = 1`** → `exports.y = 1`.
//! * **`export { x }`** (local) → `exports.x = x` (`export { a as b }` → `exports.b = a`).
//! * **`export * from "m"`** → `__exportStar(require("m"), exports)`.
//! * The **`__esModule` marker** (`Object.defineProperty(exports, "__esModule",
//!   { value: true });`) is emitted at the top when the module has value exports.
//! * Interop helpers (`__importDefault`/`__importStar`/`__exportStar`) are
//!   requested and emitted in the module prologue (6d-2 helper infra).
//!
//! # Divergence from Go / Deferred (DEFER(P5))
//!
//! * Imported uses are matched **by name** against the collected import
//!   bindings (no scope analysis), so a shadowing local of the same name would
//!   be wrongly rewritten. Scope-correct resolution needs the real
//!   `ReferenceResolver` (checker `resolveName`/`EmitResolver`), which is a no-op
//!   placeholder. blocked-by: checker reference resolver.
//! * The require variable name is derived deterministically (`<module>_1`)
//!   rather than via the emit-context name generator. DEFER: collision-free
//!   `NewGeneratedNameForNode`.
//! * The `__esModule` marker is emitted when there are value exports (Go emits
//!   it for any external module); the `exports.y = void 0` export-name init,
//!   `"use strict"` prologue, and hoisting/ordering are not modelled.
//! * DEFER: combined default+named imports, `export { x } from "m"` re-exports,
//!   `export =`, dynamic `import()`, `import =`, exported function/class
//!   declarations (which keep a local binding), and live bindings.

use crate::moduletransforms::externalmoduleinfo::collect_external_module_info;
use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList};
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
    // Track 2: the module kind selects whether to lower to CommonJS.
    let is_commonjs = opt.compiler_options.module == ModuleKind::CommonJs;
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            if is_commonjs {
                common_js_visit(ec, node)
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
fn common_js_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    if ec.arena().kind(node) == Kind::SourceFile {
        return transform_common_js_module(ec, node);
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
}

/// Rebuilds a source file: each external named import becomes a
/// `const <var> = require("mod");` binding, and uses of the imported names are
/// rewritten to `<var>.<name>` member accesses via node substitution.
///
/// Side effects: pushes rebuilt nodes; registers node substitutions.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:transformCommonJSModule
fn transform_common_js_module(ec: &mut EmitContext, node: NodeId) -> NodeId {
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

    let mut bindings: Vec<ImportBinding> = Vec::new();
    let mut out: Vec<NodeId> = Vec::new();
    let mut kept: Vec<NodeId> = Vec::new();
    // `Object.defineProperty(exports, "__esModule", { value: true });` at the top
    // when the module has exports.
    if has_exports {
        let marker = make_es_module_marker(ec);
        out.push(marker);
    }
    // Process statements in source order so the output order matches the input.
    for &statement in &statements.nodes {
        match ec.arena().kind(statement) {
            Kind::ImportDeclaration if info.external_imports.contains(&statement) => {
                if let Some(require_stmt) = lower_import_to_require(ec, statement, &mut bindings) {
                    out.push(require_stmt);
                    continue;
                }
                kept.push(statement);
                out.push(statement);
            }
            Kind::ExportAssignment => {
                if let Some(lowered) = lower_export_default(ec, statement) {
                    out.push(lowered);
                    continue;
                }
                kept.push(statement);
                out.push(statement);
            }
            Kind::VariableStatement if statement_has_export_modifier(ec.arena(), statement) => {
                let lowered = lower_export_variable_statement(ec, statement);
                if !lowered.is_empty() {
                    out.extend(lowered);
                    continue;
                }
                kept.push(statement);
                out.push(statement);
            }
            Kind::ExportDeclaration => {
                if let Some(lowered) = lower_export_declaration(ec, statement) {
                    out.extend(lowered);
                    continue;
                }
                kept.push(statement);
                out.push(statement);
            }
            _ => {
                kept.push(statement);
                out.push(statement);
            }
        }
    }

    // Register substitutions for uses of imported names in the kept statements.
    for &statement in &kept {
        register_import_use_substitutions(ec, statement, &bindings);
    }

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

/// Lowers `export default <expr>` to `exports.default = <expr>;`. Returns `None`
/// for `export =` (deferred).
///
/// Side effects: pushes the access/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitExportAssignment
fn lower_export_default(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let (is_export_equals, expression) = match ec.arena().data(node) {
        NodeData::ExportAssignment(d) => (d.is_export_equals, d.expression),
        _ => return None,
    };
    if is_export_equals {
        // `export = x` -> deferred.
        return None;
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
/// `exports.a = a; exports.c = b;`. Returns `None` for re-exports
/// (`export { x } from "m"`) and `export *` (handled / deferred elsewhere).
///
/// Side effects: pushes the access/assignment/statement nodes.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:appendExportsOfDeclaration
fn lower_export_declaration(ec: &mut EmitContext, node: NodeId) -> Option<Vec<NodeId>> {
    let (export_clause, module_specifier) = match ec.arena().data(node) {
        NodeData::ExportDeclaration(d) => (d.export_clause, d.module_specifier),
        _ => return None,
    };
    if let Some(module_specifier) = module_specifier {
        if export_clause.is_none() && ec.arena().kind(module_specifier) == Kind::StringLiteral {
            // export * from "m" -> __exportStar(require("m"), exports);
            let module_text = ec.arena().text(module_specifier).to_string();
            return Some(vec![make_export_star(ec, &module_text)]);
        }
        // `export { x } from "m"` (re-export) -> deferred.
        return None;
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
            });
            ec.request_emit_helper(&tsgo_printer::emithelpers::IMPORT_STAR_HELPER);
            wrap_in_helper(ec, "__importStar", require_call)
        }
        Some(Kind::NamedImports) => {
            // import { x, y } from "m" -> const m_1 = require("m"); use x -> m_1.x
            // (default + named is deferred)
            if default_name.is_some() {
                return None;
            }
            let elements = match named_bindings.map(|b| ec.arena().data(b)) {
                Some(NodeData::NamedImports(d)) => d.elements.nodes.clone(),
                _ => return None,
            };
            for specifier in elements {
                if let NodeData::ImportSpecifier(d) = ec.arena().data(specifier) {
                    let name = ec.arena().text(d.name).to_string();
                    bindings.push(ImportBinding {
                        name: name.clone(),
                        require_var: require_var.clone(),
                        member: Some(name),
                    });
                }
            }
            require_call
        }
        _ => {
            // import d from "m" -> const m_1 = __importDefault(require("m")); use d -> m_1.default
            let default_name = default_name?;
            let default_text = ec.arena().text(default_name).to_string();
            bindings.push(ImportBinding {
                name: default_text,
                require_var: require_var.clone(),
                member: Some("default".to_string()),
            });
            ec.request_emit_helper(&tsgo_printer::emithelpers::IMPORT_DEFAULT_HELPER);
            wrap_in_helper(ec, "__importDefault", require_call)
        }
    };

    Some(build_const_binding(ec, &require_var, rhs))
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
    let arena = ec.arena_mut();
    let name = arena.new_identifier(var_name);
    let declaration = arena.new_variable_declaration(name, None, None, Some(rhs));
    let declaration_list = arena.new_variable_declaration_list(NodeList::new(vec![declaration]));
    arena.add_flags(declaration_list, NodeFlags::CONST);
    arena.new_variable_statement(None, declaration_list)
}

/// Registers a node substitution for each identifier *use* in `statement` whose
/// text matches an imported binding, rewriting `x` → `<require_var>.x`.
///
/// Side effects: pushes member-access nodes; registers node substitutions.
// Go: internal/transformers/moduletransforms/commonjsmodule.go:visitIdentifier (use-site rewrite)
fn register_import_use_substitutions(
    ec: &mut EmitContext,
    statement: NodeId,
    bindings: &[ImportBinding],
) {
    let mut uses: Vec<NodeId> = Vec::new();
    collect_identifiers(ec.arena(), statement, &mut uses);
    for use_node in uses {
        let text = ec.arena().text(use_node).to_string();
        if let Some(binding) = bindings.iter().find(|b| b.name == text) {
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
    }
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

#[cfg(test)]
#[path = "commonjsmodule_test.rs"]
mod tests;
