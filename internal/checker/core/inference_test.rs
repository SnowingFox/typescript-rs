use super::*;
use crate::core::declared_types::{get_declared_type_of_symbol, get_property_of_type};
use crate::core::mapper::TypeMapper;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::types::{LiteralValue, ObjectFlags, TypeFlags};
use crate::core::Checker;
use tsgo_ast::SymbolId;

fn empty() -> StubProgram {
    StubProgram::parse_and_bind("/a.ts", "")
}

fn sym(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

// Go: internal/checker/checker.go:InferenceContext / InferenceInfo / InferencePriority
#[test]
fn inference_context_and_info_construction() {
    let ctx = InferenceContext::new(&[TypeId(1), TypeId(2)]);
    assert_eq!(ctx.inferences.len(), 2);
    assert_eq!(ctx.inferences[0].type_parameter, TypeId(1));
    assert_eq!(ctx.type_parameters, vec![TypeId(1), TypeId(2)]);
    let info = InferenceInfo::new(TypeId(5));
    assert!(info.candidates.is_empty());
    assert!(!info.is_fixed);
    assert_eq!(info.priority, InferencePriority::NONE);
}

// Go: internal/checker/inference.go:inferFromTypes (bare type parameter)
#[test]
fn infer_bare_type_parameter() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let num = c.number_type();
    let inferred = c.infer_type_arguments(&p, &[tp], &[num], &[tp]);
    assert_eq!(inferred, vec![num]);
}

// Go: internal/checker/inference.go:inferFromTypeArguments (same generic reference)
#[test]
fn infer_from_generic_reference_arguments() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let s = c.string_type();
    let box_target = c.new_object_type(ObjectFlags::INTERFACE, None, Default::default());
    let source = c.create_type_reference(box_target, vec![s]); // Box<string>
    let target = c.create_type_reference(box_target, vec![tp]); // Box<T>
    let inferred = c.infer_type_arguments(&p, &[tp], &[source], &[target]);
    assert_eq!(inferred, vec![s]);
}

// Go: internal/checker/inference.go:inferFromTypes (target union)
#[test]
fn infer_from_union_target() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let s = c.string_type();
    let target_union = c.get_union_type(&[s, tp]); // string | T
    let inferred = c.infer_type_arguments(&p, &[tp], &[s], &[target_union]);
    assert_eq!(inferred, vec![s]);
}

// Go: internal/checker/inference.go:inferFromObjectTypes (matching members)
#[test]
fn infer_from_object_members() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface S {\n  x: number;\n}\ninterface Tgt {\n  x: number;\n}",
    );
    let mut c = Checker::new();
    let s_ty = get_declared_type_of_symbol(&mut c, &p, sym(&p, "S"), None);
    let tgt_ty = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Tgt"), None);
    let tp = c.new_type_parameter(None);
    // Make Tgt.x have type T so inference flows S.x (number) -> T.
    let x_sym = get_property_of_type(&c, tgt_ty, "x").expect("Tgt.x");
    c.value_symbol_links.get(x_sym).resolved_type = Some(tp);
    let num = c.number_type();
    let inferred = c.infer_type_arguments(&p, &[tp], &[s_ty], &[tgt_ty]);
    assert_eq!(inferred, vec![num]);
}

// Go: internal/checker/inference.go:getInferredType (no candidates -> default)
#[test]
fn infer_with_no_candidates_yields_unknown() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let inferred = c.infer_type_arguments(&p, &[tp], &[], &[]);
    assert_eq!(inferred, vec![c.unknown_type()]);
}

// Go: internal/checker/inference.go:getInferredType (best common of candidates)
#[test]
fn infer_multiple_candidates_best_common() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let num = c.number_type();
    let s = c.string_type();
    // Two identical candidates collapse to the single type.
    assert_eq!(
        c.infer_type_arguments(&p, &[tp], &[num, num], &[tp, tp]),
        vec![num]
    );
    // Disjoint candidates union.
    let expected = c.string_or_number_type();
    assert_eq!(
        c.infer_type_arguments(&p, &[tp], &[num, s], &[tp, tp]),
        vec![expected]
    );
}

// Go: internal/checker/inference.go:getMapperFromContext (eager array form)
#[test]
fn get_inference_mapper_builds_array() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let num = c.number_type();
    let mut ctx = InferenceContext::new(&[tp]);
    c.infer_types(&p, &mut ctx.inferences, num, tp);
    match c.get_inference_mapper(&p, &mut ctx) {
        TypeMapper::Array { sources, targets } => {
            assert_eq!(sources, vec![tp]);
            assert_eq!(targets, vec![num]);
        }
        other => panic!("expected Array mapper, got {other:?}"),
    }
}

// Closes the loop with 4d: infer T then instantiate the signature's return.
// Go: internal/checker/checker.go (inferTypeArguments + instantiateSignature)
#[test]
fn infer_then_instantiate_signature_return() {
    use crate::core::signatures::{Signature, SignatureFlags};
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let num = c.number_type();
    // f<T>(x: T): T  — model the return type as T.
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.type_parameters = vec![tp];
    sig.resolved_return_type = Some(tp);
    let sig_id = c.new_signature(sig);
    // Infer T from a number argument matched against parameter type T.
    let inferred = c.infer_type_arguments(&p, &[tp], &[num], &[tp]);
    let mapper = TypeMapper::new(&[tp], &inferred);
    let instantiated = c.instantiate_signature(sig_id, &mapper);
    assert_eq!(c.signature(instantiated).resolved_return_type, Some(num));
}

// Go: internal/checker/checker.go:getCommonSupertype (4e subset)
#[test]
fn best_common_type_dominator_or_union() {
    let mut c = Checker::new();
    let p = empty();
    let num = c.number_type();
    let s = c.string_type();
    assert_eq!(c.get_best_common_type(&p, &[num, num]), num);
    let str_lit = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    // string dominates the string-literal.
    assert_eq!(c.get_best_common_type(&p, &[str_lit, s]), s);
}

// Go: internal/checker/checker.go:removeSubtypes (4e subset)
#[test]
fn subtype_reduce_removes_subsumed() {
    let mut c = Checker::new();
    let p = empty();
    let num = c.number_type();
    let s = c.string_type();
    // Disjoint primitives are both kept.
    assert_eq!(c.subtype_reduce(&p, &[num, s]), vec![num, s]);
    let str_lit = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    // The literal is subsumed by `string`.
    assert_eq!(c.subtype_reduce(&p, &[str_lit, s]), vec![s]);
}

// ===========================================================================
// C-B2: call-resolution inference engine (getInferredTypes / getCovariantInference
// / getCommonSupertype reachable subset).
// ===========================================================================

fn number_literal(c: &mut Checker, n: f64) -> TypeId {
    c.get_number_literal_type(tsgo_jsnum::Number::from(n))
}

fn string_literal(c: &mut Checker, s: &str) -> TypeId {
    c.get_string_literal_type(s)
}

// Go: internal/checker/inference.go:Checker.literalTypesWithSameBaseType
#[test]
fn literal_types_with_same_base_type_classifies_candidates() {
    let mut c = Checker::new();
    let one = number_literal(&mut c, 1.0);
    let two = number_literal(&mut c, 2.0);
    let x = string_literal(&mut c, "x");
    let num = c.number_type();
    // Two number literals share the `number` base.
    assert!(c.literal_types_with_same_base_type(&[one, two]));
    // A number literal and a string literal do not.
    assert!(!c.literal_types_with_same_base_type(&[one, x]));
    // A non-literal (its base equals itself) disqualifies the set.
    assert!(!c.literal_types_with_same_base_type(&[one, num]));
    assert!(!c.literal_types_with_same_base_type(&[num, num]));
}

// Go: internal/checker/inference.go:findLeftmostType
#[test]
fn find_leftmost_supertype_keeps_leftmost_when_unrelated() {
    let mut c = Checker::new();
    let p = empty();
    let one = number_literal(&mut c, 1.0);
    let x = string_literal(&mut c, "x");
    let num = c.number_type();
    // Unrelated literals: the leftmost is kept.
    assert_eq!(c.find_leftmost_supertype(&p, &[one, x]), one);
    // A literal then its base: the base is a supertype, so it replaces the
    // running candidate.
    assert_eq!(c.find_leftmost_supertype(&p, &[one, num]), num);
}

// Go: internal/checker/inference.go:Checker.getCommonSupertype
#[test]
fn get_common_supertype_unions_same_base_and_picks_leftmost_otherwise() {
    let mut c = Checker::new();
    let p = empty();
    let one = number_literal(&mut c, 1.0);
    let two = number_literal(&mut c, 2.0);
    let x = string_literal(&mut c, "x");
    // Single candidate is returned directly.
    assert_eq!(c.get_common_supertype(&p, &[one]), one);
    // Same-base literals union.
    let union = c.get_union_type(&[one, two]);
    assert_eq!(c.get_common_supertype(&p, &[one, two]), union);
    // Cross-base literals collapse to the leftmost (no later supertype).
    assert_eq!(c.get_common_supertype(&p, &[one, x]), one);
}

// Go: internal/checker/inference.go:Checker.getCovariantInference (reachable subset)
#[test]
fn get_covariant_inference_widens_and_combines() {
    let mut c = Checker::new();
    let p = empty();
    let one = number_literal(&mut c, 1.0);
    let two = number_literal(&mut c, 2.0);
    // A single literal candidate is preserved (getWidenedType is identity here).
    assert_eq!(c.get_covariant_inference(&p, &[one]), one);
    // Same-base candidates combine into the union.
    let union = c.get_union_type(&[one, two]);
    assert_eq!(c.get_covariant_inference(&p, &[one, two]), union);
}

// Go: internal/checker/inference.go:Checker.getInferredType (no candidates -> unknown)
#[test]
fn get_inferred_type_for_call_no_candidate_is_unknown() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let mut ctx = InferenceContext::new(&[tp]);
    assert_eq!(
        c.get_inferred_type_for_call(&p, &mut ctx, 0),
        c.unknown_type()
    );
    // The result is cached.
    assert_eq!(ctx.inferences[0].inferred_type, Some(c.unknown_type()));
}

// Go: internal/checker/inference.go:Checker.getInferredType (covariant candidates)
#[test]
fn get_inferred_type_for_call_resolves_covariant_candidates() {
    let mut c = Checker::new();
    let p = empty();
    let tp = c.new_type_parameter(None);
    let one = number_literal(&mut c, 1.0);
    let two = number_literal(&mut c, 2.0);
    let mut ctx = InferenceContext::new(&[tp]);
    ctx.inferences[0].candidates = vec![one, two];
    let union = c.get_union_type(&[one, two]);
    assert_eq!(c.get_inferred_type_for_call(&p, &mut ctx, 0), union);
}

// Go: internal/checker/inference.go:Checker.getInferredTypes (one slot per parameter)
#[test]
fn get_inferred_types_for_call_resolves_each_slot() {
    let mut c = Checker::new();
    let p = empty();
    let t = c.new_type_parameter(None);
    let u = c.new_type_parameter(None);
    let one = number_literal(&mut c, 1.0);
    let mut ctx = InferenceContext::new(&[t, u]);
    ctx.inferences[0].candidates = vec![one];
    // Second slot has no candidates -> unknown fallback.
    let result = c.get_inferred_types_for_call(&p, &mut ctx);
    assert_eq!(result, vec![one, c.unknown_type()]);
}
