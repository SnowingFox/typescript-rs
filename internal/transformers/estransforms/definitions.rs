//! Port of Go `internal/transformers/estransforms/definitions.go`: the per-target
//! ECMAScript down-leveling pipeline definitions and the [`get_es_transformer`]
//! dispatch.
//!
//! Each `NewES20XXTransformer` in Go chains earlier stages plus the new stage
//! for that target level. `GetESTransformer` selects the appropriate chain
//! based on `compilerOptions.GetEmitScriptTarget()`.

use crate::estransforms::classfields::new_class_fields_transformer;
use crate::estransforms::esdecorator::new_es_decorator_transformer;
use crate::estransforms::exponentiation::new_exponentiation_transformer;
use crate::estransforms::forawait::new_for_await_transformer;
use crate::estransforms::logicalassignment::new_logical_assignment_transformer;
use crate::estransforms::nullishcoalescing::new_nullish_coalescing_transformer;
use crate::estransforms::objectrestspread::new_object_rest_spread_transformer;
use crate::estransforms::optionalcatch::new_optional_catch_transformer;
use crate::estransforms::optionalchain::new_optional_chain_transformer;
use crate::estransforms::r#async::new_async_transformer;
use crate::estransforms::taggedtemplate::new_tagged_template_transformer;
use crate::estransforms::using::new_using_declaration_transformer;
use crate::{chain, TransformOptions, TransformerFactory};
use tsgo_core::compileroptions::ScriptTarget;

/// Builds a chain of the ES decorator and class-fields transforms.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:esDecoratorAndClassFields
pub fn es_decorator_and_class_fields() -> TransformerFactory {
    chain(vec![
        Box::new(|opt: &mut TransformOptions| Some(new_es_decorator_transformer(opt))),
        Box::new(|opt: &mut TransformOptions| Some(new_class_fields_transformer(opt))),
    ])
}

/// ESNext transformer: `using` declarations + decorator/class-fields.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:NewESNextTransformer
pub fn new_es_next_transformer() -> TransformerFactory {
    chain(vec![
        Box::new(|opt: &mut TransformOptions| Some(new_using_declaration_transformer(opt))),
        es_decorator_and_class_fields(),
    ])
}

/// ES2021 transformer: ESNext chain + logical assignment.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:NewES2021Transformer
pub fn new_es2021_transformer() -> TransformerFactory {
    chain(vec![
        new_es_next_transformer(),
        Box::new(|opt: &mut TransformOptions| Some(new_logical_assignment_transformer(opt))),
    ])
}

/// ES2020 transformer: ES2021 chain + nullish coalescing + optional chaining.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:NewES2020Transformer
pub fn new_es2020_transformer() -> TransformerFactory {
    chain(vec![
        new_es2021_transformer(),
        Box::new(|opt: &mut TransformOptions| Some(new_nullish_coalescing_transformer(opt))),
        Box::new(|opt: &mut TransformOptions| Some(new_optional_chain_transformer(opt))),
    ])
}

/// ES2019 transformer: ES2020 chain + optional catch.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:NewES2019Transformer
pub fn new_es2019_transformer() -> TransformerFactory {
    chain(vec![
        new_es2020_transformer(),
        Box::new(|opt: &mut TransformOptions| Some(new_optional_catch_transformer(opt))),
    ])
}

/// ES2018 transformer: ES2019 chain + object rest/spread + for-await +
/// tagged template restriction lift.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:NewES2018Transformer
pub fn new_es2018_transformer() -> TransformerFactory {
    chain(vec![
        new_es2019_transformer(),
        Box::new(|opt: &mut TransformOptions| Some(new_object_rest_spread_transformer(opt))),
        Box::new(|opt: &mut TransformOptions| Some(new_for_await_transformer(opt))),
        Box::new(|opt: &mut TransformOptions| Some(new_tagged_template_transformer(opt))),
    ])
}

/// ES2017 transformer: ES2018 chain + async function lowering.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:NewES2017Transformer
pub fn new_es2017_transformer() -> TransformerFactory {
    chain(vec![
        new_es2018_transformer(),
        Box::new(|opt: &mut TransformOptions| Some(new_async_transformer(opt))),
    ])
}

/// ES2016 transformer: ES2017 chain + exponentiation.
///
/// Side effects: allocates transformer factories.
// Go: internal/transformers/estransforms/definitions.go:NewES2016Transformer
pub fn new_es2016_transformer() -> TransformerFactory {
    chain(vec![
        new_es2017_transformer(),
        Box::new(|opt: &mut TransformOptions| Some(new_exponentiation_transformer(opt))),
    ])
}

/// Selects the appropriate ES transform chain based on the emit target level
/// in `opts.compiler_options`. Higher targets run fewer transforms.
///
/// # Examples
/// ```
/// use tsgo_transformers::estransforms::definitions::get_es_transformer;
/// use tsgo_transformers::TransformOptions;
/// use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
///
/// let mut opts = TransformOptions {
///     compiler_options: CompilerOptions {
///         target: ScriptTarget::Es2020,
///         ..Default::default()
///     },
///     ..Default::default()
/// };
/// let tx = get_es_transformer(&mut opts);
/// assert!(tx.is_some());
/// ```
///
/// Side effects: allocates transformer factories per the selected chain.
// Go: internal/transformers/estransforms/definitions.go:GetESTransformer
pub fn get_es_transformer(opts: &mut TransformOptions) -> Option<crate::Transformer> {
    let target = opts.compiler_options.get_emit_script_target();
    let mut factory = match target {
        ScriptTarget::EsNext => es_decorator_and_class_fields(),
        ScriptTarget::Es2025
        | ScriptTarget::Es2024
        | ScriptTarget::Es2023
        | ScriptTarget::Es2022
        | ScriptTarget::Es2021 => new_es_next_transformer(),
        ScriptTarget::Es2020 => new_es2021_transformer(),
        ScriptTarget::Es2019 => new_es2020_transformer(),
        ScriptTarget::Es2018 => new_es2019_transformer(),
        ScriptTarget::Es2017 => new_es2018_transformer(),
        ScriptTarget::Es2016 => new_es2017_transformer(),
        _ => new_es2016_transformer(),
    };
    factory(opts)
}

#[cfg(test)]
#[path = "definitions_test.rs"]
mod tests;
