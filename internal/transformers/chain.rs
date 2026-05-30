//! Port of Go `internal/transformers/chain.go`: composing a pipeline of
//! transforms that each run over a whole `SourceFile`, one after another.

use crate::transformer::{new_transformer, Transformer, VisitFn};
use crate::SharedEmitContext;
use tsgo_ast::{Kind, NodeId};
use tsgo_printer::EmitContext;

/// Shared configuration handed to every [`TransformerFactory`] in a pipeline.
///
/// Only the shared [`EmitContext`] handle is modelled in round 6a; the
/// checker/resolver-derived fields are deferred until those crates'
/// transform-facing APIs land.
///
/// Side effects: none (a configuration holder).
// Go: internal/transformers/chain.go:TransformOptions
#[derive(Clone, Default)]
pub struct TransformOptions {
    /// The shared emit context reused across every stage of the pipeline.
    pub context: Option<SharedEmitContext>,
    /// The compiler options that drive transform decisions (e.g. `module`
    /// kind selects CommonJS vs ESM emit; `jsx` selects the classic vs
    /// automatic JSX runtime).
    pub compiler_options: tsgo_core::compileroptions::CompilerOptions,
    // DEFER(P5): resolver / emit_resolver. blocked-by: a real
    // `tsgo_binder::ReferenceResolver` needs checker `resolveName`/`EmitResolver`
    // (use-site -> declaration resolution) not yet ported for the transform
    // pipeline; the current resolver is a no-op placeholder.
}

/// A factory that builds (or skips, via `None`) a [`Transformer`] for a pipeline
/// stage, given the shared [`TransformOptions`].
///
/// Side effects: invoking the factory may allocate a transformer and append to
/// the shared context.
// Go: internal/transformers/chain.go:TransformerFactory
pub type TransformerFactory = Box<dyn FnMut(&mut TransformOptions) -> Option<Transformer>>;

/// Chains transforms in left-to-right order, running them one at a time over the
/// whole `SourceFile` (as opposed to interleaving at each node). The combined
/// transform only operates on `SourceFile` nodes.
///
/// Panics if `transforms` is empty (mirrors Go's `Chain`).
///
/// # Examples
/// ```
/// use tsgo_transformers::{chain, new_transformer, TransformOptions, TransformerFactory};
/// let only: TransformerFactory =
///     Box::new(|opt: &mut TransformOptions| Some(new_transformer(Box::new(|_e, n| n), opt.context.clone())));
/// // A single-element chain returns that stage's factory unchanged.
/// let mut combined = chain(vec![only]);
/// assert!(combined(&mut TransformOptions::default()).is_some());
/// ```
///
/// Side effects: the returned factory allocates transformers when invoked.
// Go: internal/transformers/chain.go:Chain
pub fn chain(mut transforms: Vec<TransformerFactory>) -> TransformerFactory {
    if transforms.len() < 2 {
        if transforms.is_empty() {
            panic!("Expected some number of transforms to chain, but got none");
        }
        return transforms.pop().expect("len == 1");
    }
    Box::new(move |opt: &mut TransformOptions| {
        let mut constructed: Vec<Transformer> = Vec::with_capacity(transforms.len());
        for t in &mut transforms {
            // TODO(port): flatten nested chains? (mirrors Go's open TODO)
            if let Some(tr) = t(opt) {
                constructed.push(tr);
            }
        }
        match constructed.len() {
            0 => None,
            1 => Some(constructed.pop().expect("len == 1")),
            _ => {
                let context = opt.context.clone();
                let mut components = constructed;
                let visit: VisitFn = Box::new(move |ec: &mut EmitContext, node: NodeId| {
                    assert_eq!(
                        ec.arena().kind(node),
                        Kind::SourceFile,
                        "Chained transform passed non-sourcefile initial node"
                    );
                    let mut result = node;
                    for comp in &mut components {
                        result = comp.run_visit(ec, result);
                    }
                    result
                });
                Some(new_transformer(visit, context))
            }
        }
    })
}

#[cfg(test)]
#[path = "chain_test.rs"]
mod tests;
