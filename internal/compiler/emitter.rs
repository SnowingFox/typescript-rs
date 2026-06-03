//! Port of Go `internal/compiler/emitter.go` (transform+print pipeline).
//!
//! The emitter turns one loaded source file into emitted text by running the
//! script-transformer pipeline (`tsgo_transformers`) and then the printer
//! (`tsgo_printer`). `Program::emit` (in `program.rs`) drives this per file,
//! preserving input order, and combines the per-file results.
//!
//! # Transform chain (W3 T2-5)
//!
//! [`get_script_transformers`] assembles the full chain of already-ported
//! transforms, matching Go's `getScriptTransformers` order:
//!
//! 1. type eraser (always)
//! 2. runtime syntax — enum/namespace (always)
//! 3. legacy decorators (when `experimentalDecorators`)
//! 4. JSX (when `jsx` is `react`/`reactJsx`/`reactJsxDev` and file is JSX)
//! 5. ES downlevel ([`get_es_transformer`], gated by `--target`)
//! 6. `"use strict"` directive
//! 7. module transform ([`get_module_transformer`], gated by `--module`)
//!
//! Deferred (see DEFER notes in [`get_script_transformers`]):
//!
//! - Metadata transform (`emitDecoratorMetadata`). DEFER: not yet ported.
//! - Import elision. DEFER blocked-by: checker `EmitResolver`.
//! - Const-enum inlining. DEFER: `inliners` package not yet ported.
//! - ES stages not yet ported: `using`, `esDecorator`, `logicalAssignment`,
//!   `nullishCoalescing`, `optionalCatch`, `taggedTemplate`.
//! - Declaration (`.d.ts`) emit. DEFER blocked-by: checker public API.
//!
//! # Source maps (P6-7)
//!
//! `--sourceMap` / `--inlineSourceMap` are wired: when source maps are enabled
//! [`emit_js_text_with_source_map`] drives a `tsgo_sourcemap::Generator` while
//! printing, and [`crate::Program`] assembles the `.js.map` (file mode) or the
//! inlined `data:` URL, plus the trailing `//# sourceMappingURL=` comment.
//! `--mapRoot`/`--sourceRoot` beyond the reachable subset and token-level brace
//! mappings are deferred (see `impl.md`).

use std::cell::RefCell;
use std::rc::Rc;

use tsgo_ast::{NodeData, NodeId};
use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ScriptTarget};
use tsgo_core::get_script_kind_from_file_name;
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};
use tsgo_printer::printer::{PrintHandlers, Printer, PrinterOptions};
use tsgo_printer::EmitContext;
use tsgo_sourcemap::Generator;
use tsgo_transformers::estransforms::classfields::new_class_fields_transformer;
use tsgo_transformers::estransforms::exponentiation::new_exponentiation_transformer;
use tsgo_transformers::estransforms::forawait::new_for_await_transformer;
use tsgo_transformers::estransforms::objectrestspread::new_object_rest_spread_transformer;
use tsgo_transformers::estransforms::optionalchain::new_optional_chain_transformer;
use tsgo_transformers::estransforms::r#async::new_async_transformer;
use tsgo_transformers::estransforms::usestrict::new_use_strict_transformer;
use tsgo_transformers::jsxtransforms::jsx::new_jsx_transformer;
use tsgo_transformers::moduletransforms::commonjsmodule::new_common_js_module_transformer;
use tsgo_transformers::moduletransforms::esmodule::new_es_module_transformer;
use tsgo_transformers::moduletransforms::impliedmodule::new_implied_module_transformer;
use tsgo_transformers::tstransforms::legacydecorators::new_legacy_decorators_transformer;
use tsgo_transformers::tstransforms::runtimesyntax::new_runtime_syntax_transformer;
use tsgo_transformers::tstransforms::typeeraser::new_type_eraser_transformer;
use tsgo_transformers::{SharedEmitContext, TransformOptions, Transformer};

/// Which output artifacts an emit produces.
///
/// Side effects: none (plain value type).
// Go: internal/compiler/emitter.go:EmitOnly
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EmitOnly {
    /// Emit JavaScript and declarations.
    #[default]
    All,
    /// Emit JavaScript only.
    Js,
    /// Emit declarations only.
    Dts,
    /// Force declaration emit even when declarations are otherwise off.
    ForcedDts,
}

/// Runs the reachable script-transformer pipeline over `source_text` and prints
/// the result as JavaScript text.
///
/// The parsed file's arena is owned (and not clonable), so this re-parses the
/// file's text into a fresh [`EmitContext`]-owned arena, runs the transforms,
/// and prints. That is sound for the transform+print subset, which needs no
/// binder/checker state; once emit needs that state, share the file's arena
/// instead of re-parsing.
///
/// When `generator` is `Some` this also drives source map generation (recording
/// generated→source mappings as the file prints) and returns the populated
/// generator alongside the JS text; with `None` it just returns the JS text.
/// The generator must already be configured with the generated file name,
/// source root, and sources directory (the caller, [`crate::Program`], does this
/// from the compiler options and output paths, mirroring Go's `printSourceFile`
/// which constructs the `sourcemap.Generator` before calling `printer.Write`).
///
/// Side effects: allocates a parse arena and emit context; mutates `generator`.
// Go: internal/compiler/emitter.go:emitter.emitJSFile + printSourceFile
pub(crate) fn emit_js_text_with_source_map(
    file_name: &str,
    source_text: &str,
    options: &CompilerOptions,
    generator: Option<Generator>,
) -> (String, Option<Generator>) {
    let parse = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        source_text,
        parse_script_kind(file_name),
    );
    let ec: SharedEmitContext = Rc::new(RefCell::new(EmitContext::with_arena(parse.arena)));
    let source_file = run_script_transformers(&ec, parse.source_file, options);

    let ec_ref = ec.borrow();
    let mut printer = Printer::new(
        PrinterOptions {
            remove_comments: options.remove_comments.is_true(),
            new_line: options.new_line,
            inline_sources: options.inline_sources.is_true(),
            ..Default::default()
        },
        PrintHandlers::default(),
        &ec_ref,
    );
    match generator {
        Some(generator) => {
            let (text, generator) = printer.emit_source_file_with_source_map(
                source_file,
                source_text,
                file_name,
                generator,
            );
            (text, Some(generator))
        }
        None => (printer.emit_source_file(source_file, source_text), None),
    }
}

/// Runs the script-transformer pipeline over `source_file`, returning the
/// (possibly new) source-file id after all stages have run in order.
///
/// Side effects: mutates the shared context's arena.
// Go: internal/compiler/emitter.go:emitter.runScriptTransformers
fn run_script_transformers(
    ec: &SharedEmitContext,
    source_file: NodeId,
    options: &CompilerOptions,
) -> NodeId {
    let mut sf = source_file;
    for mut tx in get_script_transformers(ec, options, source_file) {
        sf = tx.transform_source_file(sf);
    }
    sf
}

/// Assembles the full chain of script transformers matching Go's
/// `getScriptTransformers`, using only the already-ported transform factories.
///
/// Side effects: allocates transformers over the shared context.
// Go: internal/compiler/emitter.go:getScriptTransformers
fn get_script_transformers(
    ec: &SharedEmitContext,
    options: &CompilerOptions,
    source_file: NodeId,
) -> Vec<Transformer> {
    let mut tx: Vec<Transformer> = Vec::new();

    let ec_ref = ec.borrow();
    let language_variant = match ec_ref.arena().data(source_file) {
        NodeData::SourceFile(d) => d.language_variant,
        _ => LanguageVariant::Standard,
    };
    let script_kind = match ec_ref.arena().data(source_file) {
        NodeData::SourceFile(d) => d.script_kind,
        _ => ScriptKind::Ts,
    };
    drop(ec_ref);

    let is_js_file = matches!(script_kind, ScriptKind::Js | ScriptKind::Jsx);
    let jsx_transform_enabled =
        options.get_jsx_transform_enabled() && language_variant == LanguageVariant::Jsx;

    // Go: `importElisionEnabled := !options.VerbatimModuleSyntax.IsTrue() && !ast.IsInJSFile(sourceFile.AsNode())`
    let _import_elision_enabled = !options.verbatim_module_syntax.is_true() && !is_js_file;

    let topt = TransformOptions {
        context: Some(Rc::clone(ec)),
        compiler_options: options.clone(),
    };

    // -- TypeScript syntax transforms --

    // DEFER: metadata transform (emitDecoratorMetadata). Not yet ported to Rust.
    // Go: `if options.EmitDecoratorMetadata.IsTrue() { tx = append(tx, tstransforms.NewMetadataTransformer(&opts)) }`

    // Go: erase types (always)
    tx.push(new_type_eraser_transformer(&topt));

    // DEFER: import elision. blocked-by: checker EmitResolver (MarkLinkedReferencesRecursively).
    // Go: `if importElisionEnabled { tx = append(tx, tstransforms.NewImportElisionTransformer(&opts)) }`

    // Go: transform `enum`, `namespace`, and parameter properties (always)
    tx.push(new_runtime_syntax_transformer(&topt));

    // Go: `if options.ExperimentalDecorators.IsTrue()`
    if options.experimental_decorators.is_true() {
        tx.push(new_legacy_decorators_transformer(&topt));
    }

    // -- JSX --
    // Go: `if jsxTransformEnabled`
    if jsx_transform_enabled {
        tx.push(new_jsx_transformer(&topt));
    }

    // -- ES downlevel --
    // Go: `downleveler := estransforms.GetESTransformer(&opts); if downleveler != nil { tx = append(tx, downleveler) }`
    push_es_transformers(&mut tx, &topt);

    // Go: `tx = append(tx, estransforms.NewUseStrictTransformer(&opts))`
    tx.push(new_use_strict_transformer(&topt));

    // -- Module transform --
    // Go: `tx = append(tx, getModuleTransformer(&opts))`
    tx.push(get_module_transformer(&topt));

    // DEFER: const-enum inlining. `inliners` package not yet ported to Rust.
    // Go: `if !options.GetIsolatedModules() { tx = append(tx, inliners.NewConstEnumInliningTransformer(&opts)) }`

    tx
}

/// Selects the module-format transformer based on `CompilerOptions.module`.
///
/// Side effects: allocates one transformer.
// Go: internal/compiler/emitter.go:getModuleTransformer
fn get_module_transformer(opts: &TransformOptions) -> Transformer {
    match opts.compiler_options.get_emit_module_kind() {
        // Go: `case core.ModuleKindPreserve:`
        ModuleKind::Preserve => new_es_module_transformer(opts),
        // Go: ESM / Node / CommonJS → implied module
        ModuleKind::EsNext
        | ModuleKind::Es2022
        | ModuleKind::Es2020
        | ModuleKind::Es2015
        | ModuleKind::Node20
        | ModuleKind::Node18
        | ModuleKind::Node16
        | ModuleKind::NodeNext
        | ModuleKind::CommonJs => new_implied_module_transformer(opts),
        // Go: `default:` → classic CommonJS
        _ => new_common_js_module_transformer(opts),
    }
}

/// Collects the ES-downlevel transformer stages based on `--target`, mirroring
/// Go's `estransforms.GetESTransformer`. Returns `None` when the target is
/// `ESNext` and only class-fields applies (no-op for most code).
///
/// Not-yet-ported stages (`using`, `esDecorator`, `logicalAssignment`,
/// `nullishCoalescing`, `optionalCatch`, `taggedTemplate`) are skipped with
/// DEFER notes; available stages are returned in the same cumulative order Go
/// uses (each lower target adds more stages).
///
/// Side effects: allocates transformers.
// Go: internal/transformers/estransforms/definitions.go:GetESTransformer
fn get_es_transformer(opts: &TransformOptions) -> Option<Vec<Transformer>> {
    let target = opts.compiler_options.get_emit_script_target();

    // Collect the stages that apply at this target level. Go composes static
    // chains (`NewES2016Transformer = Chain(NewES2017Transformer, newExponentiationTransformer)`);
    // we build the equivalent vec of individual stages, skipping un-ported ones.
    let mut stages: Vec<Transformer> = Vec::new();

    // --- ESNext: esDecorator + classFields ---
    // DEFER: esDecorator not yet ported.
    stages.push(new_class_fields_transformer(opts));

    if target as i32 <= ScriptTarget::Es2021 as i32 {
        // --- ESNext chain: using + esDecorator + classFields ---
        // DEFER: `using` not yet ported (parser does not support `using` declarations).
    }

    if target as i32 <= ScriptTarget::Es2020 as i32 {
        // --- ES2021: + logicalAssignment ---
        // DEFER: logicalAssignment not yet ported.
    }

    if target as i32 <= ScriptTarget::Es2019 as i32 {
        // --- ES2020: + nullishCoalescing + optionalChain ---
        // DEFER: nullishCoalescing not yet ported.
        stages.push(new_optional_chain_transformer(opts));
    }

    if target as i32 <= ScriptTarget::Es2018 as i32 {
        // --- ES2019: + optionalCatch ---
        // DEFER: optionalCatch not yet ported.
    }

    if target as i32 <= ScriptTarget::Es2017 as i32 {
        // --- ES2018: + objectRestSpread + forAwait + taggedTemplate ---
        stages.push(new_object_rest_spread_transformer(opts));
        stages.push(new_for_await_transformer(opts));
        // DEFER: taggedTemplate not yet ported.
    }

    if target as i32 <= ScriptTarget::Es2016 as i32 {
        // --- ES2017: + async ---
        stages.push(new_async_transformer(opts));
    }

    if (target as i32) < ScriptTarget::Es2016 as i32 {
        // --- ES2016: + exponentiation ---
        stages.push(new_exponentiation_transformer(opts));
    }

    if stages.is_empty() {
        return None;
    }
    Some(stages)
}

/// Assembles the ES-downlevel stages into the transformer vec (inlined into
/// the caller's chain rather than wrapped in a single composite transformer,
/// because [`Transformer::transform_source_file`] manages its own borrow and
/// cannot be called from within another borrow).
fn push_es_transformers(tx: &mut Vec<Transformer>, opts: &TransformOptions) {
    if let Some(mut stages) = get_es_transformer(opts) {
        tx.append(&mut stages);
    }
}

/// The script kind used to re-parse a file for emit, falling back to `Ts` for
/// extensions the scanner does not recognize (so emit never panics on the
/// reachable inputs, matching [`crate::host`]'s parse helper).
///
/// Side effects: none (pure).
// Go: internal/core/core.go:GetScriptKindFromFileName
fn parse_script_kind(file_name: &str) -> ScriptKind {
    match get_script_kind_from_file_name(file_name) {
        ScriptKind::Unknown => ScriptKind::Ts,
        kind => kind,
    }
}

#[cfg(test)]
#[path = "emitter_test.rs"]
mod tests;
