//! Tests for `estransforms/definitions.rs`.

use super::*;
use crate::test_support::{emit, parse_shared};
use crate::TransformOptions;
use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};

#[test]
fn get_es_transformer_returns_some_for_each_target() {
    let targets = [
        ScriptTarget::EsNext,
        ScriptTarget::Es2025,
        ScriptTarget::Es2024,
        ScriptTarget::Es2023,
        ScriptTarget::Es2022,
        ScriptTarget::Es2021,
        ScriptTarget::Es2020,
        ScriptTarget::Es2019,
        ScriptTarget::Es2018,
        ScriptTarget::Es2017,
        ScriptTarget::Es2016,
        ScriptTarget::Es2015,
        ScriptTarget::Es5,
    ];
    for target in &targets {
        let mut opts = TransformOptions {
            compiler_options: CompilerOptions {
                target: *target,
                ..Default::default()
            },
            ..Default::default()
        };
        let tx = get_es_transformer(&mut opts);
        assert!(
            tx.is_some(),
            "get_es_transformer should return Some for target {:?}",
            target
        );
    }
}

#[test]
fn get_es_transformer_esnext_runs_without_panic() {
    let src = "let x = 1;";
    let (ec, sf) = parse_shared(src);
    let mut opts = TransformOptions {
        context: Some(ec.clone()),
        compiler_options: CompilerOptions {
            target: ScriptTarget::EsNext,
            ..Default::default()
        },
    };
    let mut tx = get_es_transformer(&mut opts).unwrap();
    let result = tx.run_visit(&mut ec.borrow_mut(), sf);
    let text = emit(&ec, result, src);
    assert_eq!(text, "let x = 1;");
}

#[test]
fn get_es_transformer_es2016_runs_without_panic() {
    let src = "let x = 1;";
    let (ec, sf) = parse_shared(src);
    let mut opts = TransformOptions {
        context: Some(ec.clone()),
        compiler_options: CompilerOptions {
            target: ScriptTarget::Es2016,
            ..Default::default()
        },
    };
    let mut tx = get_es_transformer(&mut opts).unwrap();
    let result = tx.run_visit(&mut ec.borrow_mut(), sf);
    let text = emit(&ec, result, src);
    assert_eq!(text, "let x = 1;");
}
