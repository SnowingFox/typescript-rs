//! Port of Go `internal/compiler/emitter.go` (the reachable transform+print
//! subset).
//!
//! The emitter turns one loaded source file into emitted text by running the
//! script-transformer pipeline (`tsgo_transformers`) and then the printer
//! (`tsgo_printer`). `Program::emit` (in `program.rs`) drives this per file,
//! preserving input order, and combines the per-file results.
//!
//! # Reachable subset (P6-3)
//!
//! This round wires the end-to-end path that does **not** depend on the
//! checker: parse → script transformers → print → write `.js`. The only script
//! transformer reachable without a checker `EmitResolver` is the type eraser, so
//! the pipeline is just that stage for now (see [`run_script_transformers`]).
//!
//! Deferred (see `impl.md`/`tests.md` DEFER tables):
//!
//! - The rest of Go's `getScriptTransformers` chain (metadata, import elision,
//!   runtime syntax, legacy decorators, JSX, ES downlevel, `use strict`, module,
//!   const-enum inlining). DEFER blocked-by: checker `EmitResolver` and the
//!   not-yet-ported transformer factories.
//! - Declaration (`.d.ts`) emit and `EmitHost`/emit-resolver wiring. DEFER
//!   blocked-by: checker public API + the declarations transformer.
//! - Source maps. DEFER blocked-by: `tsgo_printer::Printer` does not yet drive a
//!   `sourcemap::Generator` (its `emit_source_file` takes no generator, unlike
//!   Go's `printer.Write`).

use std::cell::RefCell;
use std::rc::Rc;

use tsgo_ast::NodeId;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::get_script_kind_from_file_name;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};
use tsgo_printer::printer::{PrintHandlers, Printer, PrinterOptions};
use tsgo_printer::EmitContext;
use tsgo_transformers::tstransforms::typeeraser::new_type_eraser_transformer;
use tsgo_transformers::{SharedEmitContext, TransformOptions};

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
/// Side effects: allocates a parse arena and emit context.
// Go: internal/compiler/emitter.go:emitter.emitJSFile (transform + print core)
pub(crate) fn emit_js_text(
    file_name: &str,
    source_text: &str,
    options: &CompilerOptions,
) -> String {
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
            ..Default::default()
        },
        PrintHandlers::default(),
        &ec_ref,
    );
    printer.emit_source_file(source_file, source_text)
}

/// Runs the reachable script transformers over `source_file`, returning the
/// (possibly new) source-file id.
///
/// Side effects: mutates the shared context's arena.
// Go: internal/compiler/emitter.go:emitter.runScriptTransformers / getScriptTransformers
fn run_script_transformers(
    ec: &SharedEmitContext,
    source_file: NodeId,
    options: &CompilerOptions,
) -> NodeId {
    let topt = TransformOptions {
        context: Some(Rc::clone(ec)),
        compiler_options: options.clone(),
    };
    // Reachable subset of `getScriptTransformers`: erase TypeScript-only syntax.
    // DEFER (blocked-by checker EmitResolver / not-yet-ported factories): the
    // metadata, import-elision, runtime-syntax, legacy-decorator, JSX, ES
    // downlevel, use-strict, module, and const-enum-inlining stages.
    let mut type_eraser = new_type_eraser_transformer(&topt);
    type_eraser.transform_source_file(source_file)
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
