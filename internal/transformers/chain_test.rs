use super::*;
use crate::test_support::{emit, parse_shared, rename_ident};
use std::rc::Rc;

// Builds a factory whose transformer renames identifier `from` -> `to`, sharing
// the pipeline's emit context so every stage appends to one arena.
fn rename_factory(from: &'static str, to: &'static str) -> TransformerFactory {
    Box::new(move |opt: &mut TransformOptions| {
        Some(new_transformer(
            Box::new(move |ec, node| rename_ident(ec.arena_mut(), node, from, to)),
            opt.context.clone(),
        ))
    })
}

// Go: internal/transformers/chain.go:Chain
// Two stages chained left-to-right run in order over the shared SourceFile:
// `a -> b` then `b -> c` yields `c`.
#[test]
fn chain_runs_stages_left_to_right() {
    let input = "a;";
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
    };
    let mut factory = chain(vec![rename_factory("a", "b"), rename_factory("b", "c")]);
    let mut tx = factory(&mut opts).expect("chain produced a transformer");
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "c;");
}

// Go: internal/transformers/chain.go:Chain (len == 1 branch)
// A single-element chain returns that stage's factory unchanged.
#[test]
fn chain_single_element_passthrough() {
    let input = "a;";
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
    };
    let mut factory = chain(vec![rename_factory("a", "b")]);
    let mut tx = factory(&mut opts).expect("single chain produced a transformer");
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "b;");
}

// Go: internal/transformers/chain.go:Chain (skips None stages)
// Stages that decline (return None) are dropped; one survivor is returned as-is.
#[test]
fn chain_skips_none_stages() {
    let input = "a;";
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
    };
    let skip: TransformerFactory = Box::new(|_opt| None);
    let mut factory = chain(vec![skip, rename_factory("a", "b")]);
    let mut tx = factory(&mut opts).expect("chain produced surviving transformer");
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "b;");
}

// Go: internal/transformers/chain.go:Chain (all-None -> nil)
// When every stage declines, the combined factory yields no transformer.
#[test]
fn chain_all_none_yields_none() {
    let mut opts = TransformOptions::default();
    let a: TransformerFactory = Box::new(|_opt| None);
    let b: TransformerFactory = Box::new(|_opt| None);
    let mut factory = chain(vec![a, b]);
    assert!(factory(&mut opts).is_none());
}

// Go: internal/transformers/chain.go:Chain (len == 0 panics)
#[test]
#[should_panic(expected = "Expected some number of transforms to chain")]
fn chain_empty_panics() {
    let _ = chain(Vec::new());
}
