use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;
use tsgo_core::compileroptions::ModuleKind;

// Runs `input` through the use-strict transformer under `module` and asserts the
// emitted JS.
fn check_use_strict(input: &str, module: ModuleKind, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = module;
    let mut tx = new_use_strict_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "use_strict({input:?})");
}

// Go: usestrict.go:visitSourceFile + factory.go:EnsureUseStrict
// Tracer: a CommonJS module (emit module kind `< ES2015`, so the ESM-skip never
// applies) gains a leading `"use strict";` directive. Verified against tsc
// --module commonjs (every emitted CJS module is prefixed with `"use strict";`).
#[test]
fn commonjs_module_gains_use_strict_prologue() {
    check_use_strict(
        "export const y = 1;",
        ModuleKind::CommonJs,
        "\"use strict\";\nexport const y = 1;",
    );
}

// Go: factory.go:EnsureUseStrict (already-present prologue short-circuit)
// A source that already begins with a `"use strict"` prologue directive is not
// given a second one.
#[test]
fn existing_use_strict_prologue_is_not_duplicated() {
    check_use_strict(
        "\"use strict\"; var x = 1;",
        ModuleKind::CommonJs,
        "\"use strict\";\nvar x = 1;",
    );
}

// Go: usestrict.go:visitSourceFile (ESM-emit skip)
// An external module emitted as ESM (emit module kind `>= ES2015`) is already
// strict, so no `"use strict";` directive is added.
#[test]
fn esm_external_module_skips_use_strict() {
    check_use_strict(
        "export const y = 1;",
        ModuleKind::EsNext,
        "export const y = 1;",
    );
}
