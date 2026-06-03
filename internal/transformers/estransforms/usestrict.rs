//! Port of Go `internal/transformers/estransforms/usestrict.go`: prepends a
//! leading `"use strict";` prologue directive to emitted modules/scripts.
//!
//! # Scope (round 6x — reachable CommonJS subset)
//!
//! This is a **separate** transform from the module transforms (Go appends it to
//! the emit pipeline right after the ES down-leveler and before the module
//! transformer), correcting the assumption that the CommonJS transform inserts
//! `"use strict"` itself — it does not. The transform prepends a `"use strict";`
//! directive (via the `EnsureUseStrict` dedup) to every non-JSON source file,
//! **except** an external module that is emitted as ESM (ESM is always strict).
//!
//! Reachable this round: the CommonJS path. With emit module kind `CommonJs`
//! (`< ES2015`) the ESM-skip never applies, so every non-JSON file gains a
//! leading `"use strict";`. An existing leading `"use strict"` is not
//! duplicated.
//!
//! # Deferred (DEFER(P5))
//!
//! * Precise per-file ESM gating: Go skips when
//!   `isExternalModule && moduleKind >= ES2015 && (moduleKind == Preserve ||
//!   format >= ES2015)`, where `format` is `GetEmitModuleFormatOfFile` — a
//!   per-file, `package.json`-`type`/resolver-dependent value not yet threaded
//!   into `TransformOptions`. The reachable subset approximates `format >=
//!   ES2015` by the emit module kind, which is exact for non-Node ESM kinds
//!   (`ES2015..=ESNext`, `Preserve`) but not for `Node16+` (where a CommonJS
//!   `format` should still get `"use strict"`). blocked-by: threading
//!   `getEmitModuleFormatOfFile` (needs the resolver / module format analysis).

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeId, NodeList};
use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that ensures a leading `"use strict";` directive,
/// sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::usestrict::new_use_strict_transformer, TransformOptions};
/// let _tx = new_use_strict_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/usestrict.go:NewUseStrictTransformer
pub fn new_use_strict_transformer(opt: &TransformOptions) -> Transformer {
    let compiler_options = opt.compiler_options.clone();
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            use_strict_visit(ec, &compiler_options, node)
        }),
        opt.context.clone(),
    )
}

/// Visits a node, dispatching the source file to the use-strict transform.
///
/// Side effects: see [`new_use_strict_transformer`].
// Go: internal/transformers/estransforms/usestrict.go:useStrictTransformer.visit
fn use_strict_visit(ec: &mut EmitContext, options: &CompilerOptions, node: NodeId) -> NodeId {
    if ec.arena().kind(node) == Kind::SourceFile {
        return transform_use_strict(ec, options, node);
    }
    node
}

/// Prepends a `"use strict";` directive to the source file unless it is JSON or
/// an external module emitted as ESM.
///
/// Side effects: may rebuild the source file with a leading directive.
// Go: internal/transformers/estransforms/usestrict.go:useStrictTransformer.visitSourceFile
fn transform_use_strict(ec: &mut EmitContext, options: &CompilerOptions, node: NodeId) -> NodeId {
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

    if script_kind == ScriptKind::Json {
        return node;
    }

    // Detect external-module-ness from statements: `external_module_indicator`
    // may be lost when earlier transforms rebuild the source file (the arena
    // `new_source_file` initializes it to `None`). Scanning for import/export
    // statements mirrors Go's `isAnExternalModuleIndicatorNode`.
    // Go: internal/parser/parser.go:isAnExternalModuleIndicatorNode
    let is_external = has_module_indicator(ec.arena(), &statements.nodes);

    // ESM is always strict. If the file is an external module emitted as ESM,
    // skip adding `"use strict"`. The exact per-file `format` gating is deferred
    // (see module docs); for the reachable `module < ES2015` (CommonJS) subset
    // the skip never applies.
    let module_kind = options.get_emit_module_kind();
    if is_external && (module_kind as i32) >= (ModuleKind::Es2015 as i32) {
        return node;
    }

    let new_statements = ensure_use_strict(ec.arena_mut(), &statements.nodes);
    ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(new_statements),
        end_of_file_token,
    )
}

/// Reports whether any statement in the source file is an external module
/// indicator (import/export declaration, export modifier), mirroring Go's
/// `isAnExternalModuleIndicatorNode`. This replaces reading
/// `external_module_indicator` which may be lost when a prior transform
/// rebuilds the source-file node.
///
/// Side effects: none (reads the arena).
// Go: internal/parser/parser.go:isAnExternalModuleIndicatorNode (subset)
fn has_module_indicator(arena: &NodeArena, statements: &[NodeId]) -> bool {
    for &stmt in statements {
        match arena.kind(stmt) {
            Kind::ImportDeclaration | Kind::ExportDeclaration | Kind::ExportAssignment => {
                return true;
            }
            Kind::ImportEqualsDeclaration => return true,
            _ => {
                if has_export_modifier(arena, stmt) {
                    return true;
                }
            }
        }
    }
    false
}

/// Reports whether `node` has an `export` modifier.
///
/// Side effects: none (reads the arena).
fn has_export_modifier(arena: &NodeArena, node: NodeId) -> bool {
    let modifiers = match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers
        .map(|m| m.modifier_flags.contains(ModifierFlags::EXPORT))
        .unwrap_or(false)
}

/// Ensures `"use strict"` is the first statement: returns the statements
/// unchanged if they already begin with a `"use strict"` prologue directive,
/// otherwise prepends one.
///
/// Side effects: may push the directive's literal/statement nodes.
// Go: internal/printer/factory.go:NodeFactory.EnsureUseStrict
fn ensure_use_strict(arena: &mut NodeArena, statements: &[NodeId]) -> Vec<NodeId> {
    if let Some(&first) = statements.first() {
        if is_use_strict_prologue(arena, first) {
            return statements.to_vec();
        }
    }
    let literal = arena.new_string_literal("use strict", tsgo_ast::TokenFlags::NONE);
    let directive = arena.new_expression_statement(literal);
    let mut out = Vec::with_capacity(statements.len() + 1);
    out.push(directive);
    out.extend_from_slice(statements);
    out
}

/// Reports whether `statement` is the `"use strict"` prologue directive (an
/// expression statement whose expression is the string literal `use strict`).
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsPrologueDirective + Expression().Text() == "use strict"
fn is_use_strict_prologue(arena: &NodeArena, statement: NodeId) -> bool {
    let expression = match arena.data(statement) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => return false,
    };
    arena.kind(expression) == Kind::StringLiteral && arena.text(expression) == "use strict"
}

#[cfg(test)]
#[path = "usestrict_test.rs"]
mod tests;
