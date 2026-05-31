//! Port of Go `internal/transformers/moduletransforms/impliedmodule.go`: the
//! implied-module-format transformer, which dispatches each source file to the
//! CommonJS or ES module transform based on the file's emit module format.

use crate::moduletransforms::commonjsmodule::new_common_js_module_transformer;
use crate::moduletransforms::esmodule::new_es_module_transformer;
use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeData, NodeId};
use tsgo_core::compileroptions::ModuleKind;
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that dispatches per-file between the CommonJS and ES
/// module transforms based on the file's emit module format.
///
/// # Examples
/// ```
/// use tsgo_transformers::{moduletransforms::impliedmodule::new_implied_module_transformer, TransformOptions};
/// let _tx = new_implied_module_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/moduletransforms/impliedmodule.go:NewImpliedModuleTransformer
pub fn new_implied_module_transformer(opt: &TransformOptions) -> Transformer {
    let mut cjs = new_common_js_module_transformer(opt);
    let mut esm = new_es_module_transformer(opt);
    let format = opt.compiler_options.module;
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            // Only source files dispatch; a declaration file is returned
            // unchanged (Go's `visitSourceFile` short-circuits on
            // `node.IsDeclarationFile`).
            if ec.arena().kind(node) != Kind::SourceFile || is_declaration_file(ec, node) {
                return node;
            }
            if is_es_module_format(format) {
                esm.run_visit(ec, node)
            } else {
                cjs.run_visit(ec, node)
            }
        }),
        opt.context.clone(),
    )
}

/// Reports whether source-file `node` is a declaration (`.d.ts`) file.
///
/// Side effects: none (reads the arena).
fn is_declaration_file(ec: &EmitContext, node: NodeId) -> bool {
    matches!(ec.arena().data(node), NodeData::SourceFile(d) if d.is_declaration_file)
}

/// Reports whether a file whose emit module format is `format` should be lowered
/// by the ES module transform (`format >= ES2015`) rather than the CommonJS
/// transform.
///
/// # Examples
/// ```
/// use tsgo_transformers::moduletransforms::impliedmodule::is_es_module_format;
/// use tsgo_core::compileroptions::ModuleKind;
/// assert!(!is_es_module_format(ModuleKind::CommonJs));
/// assert!(is_es_module_format(ModuleKind::Es2015));
/// assert!(is_es_module_format(ModuleKind::EsNext));
/// ```
///
/// Side effects: none (pure).
// Go: internal/transformers/moduletransforms/impliedmodule.go:visitSourceFile
// (`format >= core.ModuleKindES2015`)
pub fn is_es_module_format(format: ModuleKind) -> bool {
    (format as i32) >= (ModuleKind::Es2015 as i32)
}

#[cfg(test)]
#[path = "impliedmodule_test.rs"]
mod tests;
