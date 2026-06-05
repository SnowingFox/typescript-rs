//! Stress tests: exercise the Rust tsgo compiler on complex TypeScript patterns
//! to verify robustness (no panics/crashes) and reasonable performance under
//! pressure. Every test must complete without panic — if the checker emits
//! TS2589 for excessive depth, that is correct behaviour.

use std::sync::Arc;
use std::time::Instant;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::host::new_compiler_host;
use crate::program::{new_program, ProgramOptions};

const STRESS_TIMEOUT_SECS: u64 = 30;

fn stress_program(files: &[(&str, &str)], roots: &[&str]) -> crate::program::Program {
    stress_program_with(files, roots, CompilerOptions::default())
}

fn stress_program_with(
    files: &[(&str, &str)],
    roots: &[&str],
    options: CompilerOptions,
) -> crate::program::Program {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    let host = Arc::new(new_compiler_host("/", fs, "/lib"));
    let config = new_parsed_command_line(
        options,
        roots.iter().map(|s| s.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/".to_string(),
        },
    );
    new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    })
}

/// Deep generic instantiation: `type Deep<T> = { value: T; next: Deep<T[]> }`
/// with 10+ levels of instantiation. Must not panic/stack-overflow.
#[test]
fn stress_deep_generic_instantiation() {
    let src = r#"
type Deep<T> = { value: T; next: Deep<T[]> };
type D0 = Deep<number>;
type D1 = D0["next"];
type D2 = D1["next"];
type D3 = D2["next"];
type D4 = D3["next"];
type D5 = D4["next"];
type D6 = D5["next"];
type D7 = D6["next"];
type D8 = D7["next"];
type D9 = D8["next"];
type D10 = D9["next"];
declare const d: D10;
const x = d.value;
"#;
    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "deep generic instantiation took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(
            d.code, -1,
            "internal error diagnostic (should not appear): {d:?}"
        );
    }
}

/// Large union type: union of 200 literal types. Must not stack-overflow.
#[test]
fn stress_large_union_type() {
    let mut src = String::from("type Big = ");
    for i in 0..200 {
        if i > 0 {
            src.push_str(" | ");
        }
        src.push_str(&format!("\"v{i}\""));
    }
    src.push_str(";\ndeclare const x: Big;\nconst y: Big = x;\n");

    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "large union took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Circular type references: `type A = { b: B }; type B = { a: A }`.
/// Must not stack-overflow; TS2589 is acceptable.
#[test]
fn stress_circular_type_references() {
    let src = r#"
type A = { b: B };
type B = { a: A };
declare const a: A;
const nested = a.b.a.b.a.b.a;
"#;
    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "circular type refs took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        if d.code == 2589 {
            continue; // "Type instantiation is excessively deep" is fine
        }
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Large file: 1000 exported function declarations. Must parse and check
/// without panic in reasonable time.
#[test]
fn stress_large_file_1000_declarations() {
    let mut src = String::new();
    for i in 0..1000 {
        src.push_str(&format!(
            "export function fn{i}(a: number, b: string): boolean {{ return a > 0; }}\n"
        ));
    }
    // Add some cross-references to exercise the checker.
    src.push_str("const r0 = fn0(1, \"a\");\n");
    src.push_str("const r500 = fn500(2, \"b\");\n");
    src.push_str("const r999 = fn999(3, \"c\");\n");

    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "1000 declarations took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Many imports: 100+ modules each exporting one symbol, a main file importing
/// all of them. Verifies module resolution doesn't hang.
#[test]
fn stress_many_imports_100_modules() {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut main_src = String::new();
    for i in 0..100 {
        let mod_path = format!("/mod{i}.ts");
        let mod_src = format!("export const val{i} = {i};\n");
        files.push((mod_path.clone(), mod_src));
        main_src.push_str(&format!("import {{ val{i} }} from \"./mod{i}\";\n"));
    }
    main_src.push_str("const sum = val0 + val50 + val99;\n");
    files.push(("/index.ts".to_string(), main_src));

    let file_refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let start = Instant::now();
    let mut program = stress_program(&file_refs, &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "100 module imports took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Deeply nested expressions: `((((((a))))))` 60 levels. Parser + checker must
/// not stack-overflow.
#[test]
fn stress_deeply_nested_expressions() {
    let depth = 60;
    let mut src = String::from("declare const a: number;\nconst b = ");
    for _ in 0..depth {
        src.push('(');
    }
    src.push('a');
    for _ in 0..depth {
        src.push(')');
    }
    src.push_str(";\n");

    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "deeply nested expressions took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Complex mapped types: recursive mapped type applied to a nested object.
/// Must not crash.
#[test]
fn stress_complex_mapped_types() {
    let src = r#"
type DeepReadonly<T> = {
    readonly [K in keyof T]: T[K] extends object ? DeepReadonly<T[K]> : T[K];
};
interface Nested {
    a: { b: { c: { d: { e: number } } } };
    f: string;
    g: { h: { i: boolean } };
}
type R = DeepReadonly<Nested>;
declare const r: R;
const x = r.a.b.c.d.e;
"#;
    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "complex mapped types took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        if d.code == 2589 {
            continue;
        }
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Discriminated union with 50+ variants: an exhaustiveness-exercising union.
#[test]
fn stress_discriminated_union_50_variants() {
    let mut src = String::new();
    for i in 0..50 {
        src.push_str(&format!(
            "interface V{i} {{ kind: \"v{i}\"; data{i}: number; }}\n"
        ));
    }
    src.push_str("type Union = ");
    for i in 0..50 {
        if i > 0 {
            src.push_str(" | ");
        }
        src.push_str(&format!("V{i}"));
    }
    src.push_str(";\n");
    src.push_str("function handle(u: Union) {\n  switch (u.kind) {\n");
    for i in 0..50 {
        src.push_str(&format!("    case \"v{i}\": return u.data{i};\n"));
    }
    src.push_str("  }\n}\n");
    src.push_str("declare const u: Union;\nconst result = handle(u);\n");

    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "discriminated union 50 variants took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Template literal types: complex pattern matching with multiple components.
#[test]
fn stress_template_literal_types() {
    let src = r#"
type Color = "red" | "green" | "blue" | "yellow" | "cyan" | "magenta";
type Size = "small" | "medium" | "large" | "xl" | "xxl";
type Style = "bold" | "italic" | "underline" | "normal";
type ClassName = `${Color}-${Size}-${Style}`;
type PrefixedClass = `class-${ClassName}`;
declare const c: PrefixedClass;
const x: string = c;
type EventName<T extends string> = `on${Capitalize<T>}`;
type MouseEvents = EventName<"click" | "move" | "down" | "up" | "enter" | "leave">;
declare const e: MouseEvents;
"#;
    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "template literal types took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

/// Conditional type chains: `T extends A ? B : T extends C ? D : ...` with
/// 20+ branches. Must not crash.
#[test]
fn stress_conditional_type_chains() {
    let mut src = String::from("type Chain<T> = ");
    for i in 0..25 {
        src.push_str(&format!(
            "T extends {{ kind: \"v{i}\" }} ? \"result{i}\" : "
        ));
    }
    src.push_str("never;\n");

    // Exercise each branch.
    for i in 0..25 {
        src.push_str(&format!("type R{i} = Chain<{{ kind: \"v{i}\" }}>;\n"));
    }
    src.push_str("type RFallback = Chain<{ kind: \"unknown\" }>;\n");

    let start = Instant::now();
    let mut program = stress_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "conditional type chains took {elapsed:?} (>{STRESS_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        if d.code == 2589 {
            continue;
        }
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}
