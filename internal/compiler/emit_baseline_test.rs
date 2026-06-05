//! Integration baseline harness for the full emit pipeline (parse → bind → check
//! → transform → print). Assertions describe stable output *structure* — not
//! necessarily byte-identical parity with Go/tsc yet.
//!
//! Each test exercises [`Program::emit`] end-to-end and captures written files
//! through a sink, matching the pattern in `emitter_test.rs`.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};
use tsgo_core::tristate::Tristate;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

type Captured = Rc<RefCell<Vec<(String, String)>>>;

fn build_program(files: &[(&str, &str)], roots: &[&str], options: CompilerOptions) -> Program {
    let map: Vec<(String, String)> = files
        .iter()
        .map(|(name, text)| (name.to_string(), text.to_string()))
        .collect();
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(
        map.iter().map(|(n, t)| (n.as_str(), t.as_str())),
        true,
    ));
    let host = Arc::new(new_compiler_host("/src", fs, "/lib"));
    let config = new_parsed_command_line(
        options,
        roots.iter().map(|r| r.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/src".into(),
        },
    );
    new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    })
}

fn emit_capturing(program: &Program, captured: &Captured) -> EmitResult {
    let sink = Rc::clone(captured);
    program.emit(EmitOptions {
        target_source_file: None,
        emit_only: EmitOnly::All,
        write_file: Some(Box::new(move |name, text, _data| {
            sink.borrow_mut().push((name.to_string(), text.to_string()));
            Ok(())
        })),
    })
}

fn js_text(captured: &Captured) -> String {
    captured
        .borrow()
        .iter()
        .find(|(name, _)| name.ends_with(".js"))
        .map(|(_, text)| text.clone())
        .unwrap_or_default()
}

fn dts_text(captured: &Captured, path: &str) -> Option<String> {
    captured
        .borrow()
        .iter()
        .find(|(name, _)| name == path)
        .map(|(_, text)| text.clone())
}

// 1. Simple function → JS without type annotations; body preserved after erasure.
#[test]
fn baseline_simple_function_js_strips_types_keeps_body() {
    let program = build_program(
        &[(
            "/src/index.ts",
            "function add(a: number, b: number): number { return a + b; }",
        )],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
    let text = js_text(&captured);
    assert!(
        !text.contains(": number"),
        "type annotations should be erased: {text}"
    );
    assert!(
        text.contains("function add"),
        "function declaration should survive: {text}"
    );
    assert!(
        text.contains("return a + b"),
        "function body should be preserved: {text}"
    );
}

// 2. Class with method → valid JS class syntax (no TS-only constructs).
#[test]
fn baseline_class_with_method_emits_valid_js() {
    let program = build_program(
        &[(
            "/src/index.ts",
            "export class Greeter {\n  greet(name: string): string {\n    return \"hi \" + name;\n  }\n}",
        )],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(text.contains("class Greeter"), "class keyword: {text}");
    assert!(text.contains("greet"), "method name: {text}");
    assert!(
        !text.contains(": string"),
        "parameter/return types should be erased: {text}"
    );
    assert!(
        text.contains("return"),
        "method body should be preserved: {text}"
    );
}

// 3. `export default` → ESM shape preserved under `module: EsNext`.
#[test]
fn baseline_export_default_preserves_esm_shape() {
    let options = CompilerOptions {
        module: ModuleKind::EsNext,
        ..Default::default()
    };
    let program = build_program(
        &[(
            "/src/index.ts",
            "export default function main() { return 1; }",
        )],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(
        text.contains("export default"),
        "export default should be preserved in ESM: {text}"
    );
    assert!(
        text.contains("function main"),
        "default export binding should survive: {text}"
    );
    assert!(
        !text.contains("exports."),
        "ESM output should not use CommonJS exports: {text}"
    );
}

// 4a. `const enum` omitted by default (no runtime enum declaration).
#[test]
fn baseline_const_enum_omitted_by_default() {
    let program = build_program(
        &[(
            "/src/index.ts",
            "const enum Dir { Up, Down }\nexport const x = 1;",
        )],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(
        !text.contains("enum Dir"),
        "const enum declaration should be omitted: {text}"
    );
    assert!(
        text.contains("export const x = 1") || text.contains("const x = 1"),
        "unrelated export should survive: {text}"
    );
}

// 4b. `preserveConstEnums` keeps the runtime enum form.
#[test]
fn baseline_const_enum_preserved_when_option_set() {
    let options = CompilerOptions {
        preserve_const_enums: Tristate::True,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "const enum Dir { Up, Down }")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(
        text.contains("Dir"),
        "preserved const enum should emit runtime Dir binding: {text}"
    );
    assert!(
        text.contains("Up") && text.contains("Down"),
        "enum members should appear in preserved output: {text}"
    );
}

// 5a. Instantiated namespace lowers to IIFE; type-only namespace is elided.
#[test]
fn baseline_instantiated_namespace_lowers_type_only_namespace_elided() {
    let program = build_program(
        &[(
            "/src/index.ts",
            "namespace Types { export interface I { x: number } }\nnamespace Runtime { export const v = 1; }\nexport const use = Runtime.v;",
        )],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(
        !text.contains("namespace Types"),
        "type-only namespace should be elided: {text}"
    );
    assert!(
        !text.contains("interface I"),
        "interface inside type-only namespace should not appear in JS: {text}"
    );
    assert!(
        text.contains("Runtime"),
        "instantiated namespace should lower to Runtime binding: {text}"
    );
    assert!(
        text.contains("v = 1") || text.contains("Runtime.v"),
        "namespace member assignment should appear: {text}"
    );
}

// 5b. Non-const enum produces runtime enum object (not elided).
#[test]
fn baseline_regular_enum_emits_runtime_object() {
    let program = build_program(
        &[("/src/index.ts", "enum Color { Red, Green }")],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(
        text.contains("Color"),
        "regular enum should emit Color runtime object: {text}"
    );
    assert!(
        text.contains("Red") && text.contains("Green"),
        "enum members should be present: {text}"
    );
}

// 6. `import type` is elided from JS output (type-only import removed).
#[test]
fn baseline_import_type_elided_from_js_output() {
    let program = build_program(
        &[(
            "/src/index.ts",
            "import type { Foo } from \"./types\";\nexport const x = 1;",
        )],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(
        !text.contains("import type"),
        "import type should be elided: {text}"
    );
    assert!(
        !text.contains("from \"./types\""),
        "type-only import path should not appear: {text}"
    );
    assert!(
        text.contains("x = 1"),
        "value export should survive elision: {text}"
    );
}

// 7. Declaration emit: exported function + class produce `.d.ts` with `declare`.
#[test]
fn baseline_declaration_emit_function_and_class_use_declare() {
    let options = CompilerOptions {
        declaration: Tristate::True,
        ..Default::default()
    };
    let program = build_program(
        &[(
            "/src/index.ts",
            "export function foo(x: number): string { return \"\"; }\nexport class Bar { m(): void {} }",
        )],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    assert!(
        result.emitted_files.iter().any(|f| f.ends_with(".d.ts")),
        "expected .d.ts in emitted files: {:?}",
        result.emitted_files
    );
    let dts = dts_text(&captured, "/src/index.d.ts").expect("index.d.ts");
    assert!(
        dts.contains("declare function foo"),
        ".d.ts should declare the function: {dts}"
    );
    assert!(
        dts.contains("declare class Bar"),
        ".d.ts should declare the class: {dts}"
    );
    assert!(
        !dts.contains("return"),
        ".d.ts should not contain implementation bodies: {dts}"
    );
}

// 8. CommonJS module: `exports.foo` assignment pattern for named exports.
#[test]
fn baseline_cjs_module_exports_foo_pattern() {
    let options = CompilerOptions {
        module: ModuleKind::CommonJs,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "export const foo = 42;")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let text = js_text(&captured);
    assert!(
        !text.contains("export const"),
        "ES export syntax should be lowered: {text}"
    );
    assert!(
        text.contains("exports.foo"),
        "CJS should assign via exports.foo: {text}"
    );
    assert!(
        text.contains("__esModule"),
        "CJS interop marker should be present: {text}"
    );
}
