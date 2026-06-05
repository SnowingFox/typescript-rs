//! Stress tests for the checker: exercise type-checking of complex TypeScript
//! patterns through the StubProgram / MultiFileProgram helpers. Every test
//! must complete without panic — TS2589 for excessive depth is correct behaviour.

use std::time::Instant;

use super::program::BoundProgram;
use super::test_support::{MultiFileProgram, StubProgram};
use crate::core::Checker;

const STRESS_TIMEOUT_SECS: u64 = 30;

/// Deep generic instantiation via the checker's StubProgram path.
/// Exercises recursive type alias expansion without a full compiler pipeline.
#[test]
fn stress_checker_deep_generic_instantiation() {
    let src = r#"
type Deep<T> = { value: T; next: Deep<T[]> };
type Level0 = Deep<number>;
type Level1 = Level0["next"];
type Level2 = Level1["next"];
type Level3 = Level2["next"];
type Level4 = Level3["next"];
type Level5 = Level4["next"];
type Level6 = Level5["next"];
type Level7 = Level6["next"];
type Level8 = Level7["next"];
type Level9 = Level8["next"];
type Level10 = Level9["next"];
type Level11 = Level10["next"];
declare const d: Level11;
"#;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker deep generic instantiation took {elapsed:?}"
    );
    // TS2589 is acceptable; crash is not.
    for d in diags {
        if d.code == 2589 {
            continue;
        }
    }
}

/// Large union: 200 literal types in a single union.
#[test]
fn stress_checker_large_union() {
    let mut src = String::from("type BigUnion = ");
    for i in 0..200 {
        if i > 0 {
            src.push_str(" | ");
        }
        src.push_str(&format!("\"{i}\""));
    }
    src.push_str(";\ndeclare const x: BigUnion;\nconst y: BigUnion = x;\n");

    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker large union took {elapsed:?}"
    );
    for d in diags {
        assert_ne!(d.code, -1, "internal error: {d:?}");
    }
}

/// Circular type references through the checker path.
#[test]
fn stress_checker_circular_types() {
    let src = r#"
type A = { b: B; val: number };
type B = { a: A; val: string };
declare const root: A;
const deep = root.b.a.b.a.b.a.b.a;
"#;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker circular types took {elapsed:?}"
    );
    for d in diags {
        if d.code == 2589 {
            continue;
        }
    }
}

/// Large file with 500 declarations through StubProgram.
#[test]
fn stress_checker_many_declarations() {
    let mut src = String::new();
    for i in 0..500 {
        src.push_str(&format!(
            "function fn{i}(a: number): string {{ return String(a); }}\n"
        ));
    }
    for i in (0..500).step_by(50) {
        src.push_str(&format!("const r{i} = fn{i}({i});\n"));
    }

    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let _diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker 500 declarations took {elapsed:?}"
    );
}

/// Deeply nested parenthesised expressions through the checker.
#[test]
fn stress_checker_deeply_nested_parens() {
    let depth = 80;
    let mut src = String::from("declare const a: number;\nconst b = ");
    for _ in 0..depth {
        src.push('(');
    }
    src.push('a');
    for _ in 0..depth {
        src.push(')');
    }
    src.push_str(";\n");

    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let _diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker deeply nested parens took {elapsed:?}"
    );
}

/// Complex mapped types: recursive DeepPartial / DeepRequired patterns.
#[test]
fn stress_checker_complex_mapped_types() {
    let src = r#"
type DeepPartial<T> = {
    [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K];
};
type DeepRequired<T> = {
    [K in keyof T]-?: T[K] extends object ? DeepRequired<T[K]> : T[K];
};
interface Config {
    db: { host: string; port: number; ssl: { cert: string; key: string } };
    cache: { ttl: number; maxSize: number };
    logging: { level: string; format: string };
}
type PartialConfig = DeepPartial<Config>;
type RequiredConfig = DeepRequired<PartialConfig>;
declare const cfg: PartialConfig;
declare const full: RequiredConfig;
"#;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker complex mapped types took {elapsed:?}"
    );
    for d in diags {
        if d.code == 2589 {
            continue;
        }
    }
}

/// Discriminated union with 50 variants through the checker.
#[test]
fn stress_checker_discriminated_union() {
    let mut src = String::new();
    for i in 0..50 {
        src.push_str(&format!(
            "interface Variant{i} {{ tag: \"{i}\"; payload{i}: number; }}\n"
        ));
    }
    src.push_str("type DU = ");
    for i in 0..50 {
        if i > 0 {
            src.push_str(" | ");
        }
        src.push_str(&format!("Variant{i}"));
    }
    src.push_str(";\nfunction process(x: DU): number {\n  switch (x.tag) {\n");
    for i in 0..50 {
        src.push_str(&format!("    case \"{i}\": return x.payload{i};\n"));
    }
    src.push_str("  }\n}\n");

    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let _diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker discriminated union took {elapsed:?}"
    );
}

/// Template literal types: combinatorial explosion from several constituent unions.
#[test]
fn stress_checker_template_literal_types() {
    let src = r#"
type A = "a" | "b" | "c" | "d" | "e";
type B = "1" | "2" | "3" | "4" | "5";
type C = "x" | "y" | "z";
type Combined = `${A}-${B}-${C}`;
declare const val: Combined;
const s: string = val;
type WithCapitalize = `on${Capitalize<A>}${Capitalize<B>}`;
declare const ev: WithCapitalize;
"#;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let _diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker template literal types took {elapsed:?}"
    );
}

/// Conditional type chains: 25-branch conditional type.
#[test]
fn stress_checker_conditional_type_chains() {
    let mut src = String::from("type Classify<T> = ");
    for i in 0..25 {
        src.push_str(&format!(
            "T extends {{ tag: \"{i}\" }} ? \"match{i}\" : "
        ));
    }
    src.push_str("\"fallback\";\n");
    for i in 0..25 {
        src.push_str(&format!(
            "type Test{i} = Classify<{{ tag: \"{i}\" }}>;\n"
        ));
    }
    src.push_str("type TestFallback = Classify<{ tag: \"none\" }>;\n");

    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker conditional type chains took {elapsed:?}"
    );
    for d in diags {
        if d.code == 2589 {
            continue;
        }
    }
}

/// Cross-file stress: many modules through MultiFileProgram.
#[test]
fn stress_checker_cross_file_many_globals() {
    let mut file_specs: Vec<(String, String)> = Vec::new();
    for i in 0..100 {
        file_specs.push((
            format!("/f{i}.ts"),
            format!("var global{i}: number = {i};"),
        ));
    }
    let file_refs: Vec<(&str, &str)> = file_specs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();

    let p = std::rc::Rc::new(MultiFileProgram::build(&file_refs));
    let files = p.source_files();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    for &f in &files {
        c.check_source_file(f);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "cross-file 100 globals took {elapsed:?}"
    );
}

/// Nested binary expressions: `a + a + a + ... + a` (300 terms) to stress the
/// expression checker's recursion.
#[test]
fn stress_checker_nested_binary_expressions() {
    let mut src = String::from("declare const a: number;\nconst result = ");
    for i in 0..300 {
        if i > 0 {
            src.push_str(" + ");
        }
        src.push('a');
    }
    src.push_str(";\n");

    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let _diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker nested binary expressions took {elapsed:?}"
    );
}

/// Deeply nested object literal types.
#[test]
fn stress_checker_nested_object_literals() {
    let depth = 30;
    let mut src = String::from("declare const x: ");
    for _ in 0..depth {
        src.push_str("{ inner: ");
    }
    src.push_str("number");
    for _ in 0..depth {
        src.push_str(" }");
    }
    src.push_str(";\nconst y = x");
    for _ in 0..depth {
        src.push_str(".inner");
    }
    src.push_str(";\n");

    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let start = Instant::now();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let _diags = c.get_diagnostics(root);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < STRESS_TIMEOUT_SECS,
        "checker nested object literals took {elapsed:?}"
    );
}
