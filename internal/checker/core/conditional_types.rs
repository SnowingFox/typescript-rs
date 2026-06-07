//! Conditional, template-literal, and string-mapping type construction.
//!
//! Ports Go checker routines for distributive conditionals, template literal
//! types, intrinsic string mappings, and pattern-literal classification.

use tsgo_ast::{NodeData, NodeId};

use super::inference::InferenceContext;
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::types::{ConditionalRoot, LiteralValue, StringMappingKind, TypeFlags, TypeId};
use super::Checker;

/// Reports whether a conditional type with check type `check_type` distributes
/// over a union (Go: `checkType.flags&TypeFlagsTypeParameter != 0` at
/// conditional construction).
///
/// # Examples
/// ```
/// use tsgo_checker::{is_distributive_conditional_type, Checker};
/// let mut c = Checker::new();
/// let tp = c.new_type_parameter(None);
/// assert!(is_distributive_conditional_type(&c, tp));
/// assert!(!is_distributive_conditional_type(&c, c.string_type()));
/// ```
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (isDistributive)
pub fn is_distributive_conditional_type(checker: &Checker, check_type: TypeId) -> bool {
    checker
        .get_type(check_type)
        .flags()
        .contains(TypeFlags::TYPE_PARAMETER)
}

/// Returns the true branch type of deferred conditional `t` (Go's
/// `getTrueTypeFromConditionalType`).
///
/// Side effects: may allocate types; caches the resolved branch on `t`.
// Go: internal/checker/checker.go:Checker.getTrueTypeFromConditionalType
pub fn get_true_type_from_conditional_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
) -> TypeId {
    if let Some(cached) = checker.conditional_resolved_true_type(t) {
        return cached;
    }
    let root_node = checker
        .get_type(t)
        .as_conditional()
        .expect("conditional type")
        .root
        .node;
    let (true_node, _) = conditional_branch_nodes(program, root_node);
    let branch = checker.resolve_type_node(program, true_node);
    let resolved = match checker.conditional_mapper(t) {
        Some(mapper) => checker.instantiate_type(branch, &mapper),
        None => branch,
    };
    checker.set_conditional_resolved_true_type(t, resolved);
    resolved
}

/// Returns the false branch type of deferred conditional `t` (Go's
/// `getFalseTypeFromConditionalType`).
///
/// Side effects: may allocate types; caches the resolved branch on `t`.
// Go: internal/checker/checker.go:Checker.getFalseTypeFromConditionalType
pub fn get_false_type_from_conditional_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
) -> TypeId {
    if let Some(cached) = checker.conditional_resolved_false_type(t) {
        return cached;
    }
    let root_node = checker
        .get_type(t)
        .as_conditional()
        .expect("conditional type")
        .root
        .node;
    let (_, false_node) = conditional_branch_nodes(program, root_node);
    let branch = checker.resolve_type_node(program, false_node);
    let resolved = match checker.conditional_mapper(t) {
        Some(mapper) => checker.instantiate_type(branch, &mapper),
        None => branch,
    };
    checker.set_conditional_resolved_false_type(t, resolved);
    resolved
}

/// Builds a template literal type from interleaved `texts` and placeholder
/// `types` (`texts.len() == types.len() + 1`).
///
/// Side effects: may allocate string-literal/union/template-literal types.
// Go: internal/checker/checker.go:Checker.getTemplateLiteralType
pub fn get_template_literal_type(
    checker: &mut Checker,
    texts: &[String],
    types: &[TypeId],
) -> TypeId {
    let union_index = types.iter().position(|&t| {
        checker
            .get_type(t)
            .flags()
            .intersects(TypeFlags::NEVER | TypeFlags::UNION)
    });
    if let Some(idx) = union_index {
        let member_type = types[idx];
        let members = checker
            .get_type(member_type)
            .union_types()
            .map(|m| m.to_vec())
            .unwrap_or_default();
        let mut results: Vec<TypeId> = Vec::with_capacity(members.len());
        for m in members {
            let mut new_types = types.to_vec();
            new_types[idx] = m;
            results.push(get_template_literal_type(checker, texts, &new_types));
        }
        return checker.get_union_type(&results);
    }
    let mut sb = String::new();
    sb.push_str(&texts[0]);
    let mut new_texts: Vec<String> = Vec::new();
    let mut new_types: Vec<TypeId> = Vec::new();
    if !add_template_spans(
        checker,
        &mut sb,
        &mut new_texts,
        &mut new_types,
        texts,
        types,
    ) {
        return checker.string_type();
    }
    if new_types.is_empty() {
        return checker.get_string_literal_type(&sb);
    }
    new_texts.push(sb);
    if new_texts.iter().all(|t| t.is_empty())
        && new_types
            .iter()
            .all(|&t| checker.get_type(t).flags().contains(TypeFlags::STRING))
    {
        return checker.string_type();
    }
    checker.new_template_literal_type(new_texts, new_types)
}

/// Applies an intrinsic string-mapping type `kind<t>` (`Uppercase<S>` etc.).
///
/// Side effects: may allocate string-literal/union/string-mapping types.
// Go: internal/checker/checker.go:Checker.getStringMappingType
pub fn get_string_mapping_type(
    checker: &mut Checker,
    kind: StringMappingKind,
    t: TypeId,
) -> TypeId {
    let flags = checker.get_type(t).flags();
    if flags.intersects(TypeFlags::UNION | TypeFlags::NEVER) {
        let members = checker
            .get_type(t)
            .union_types()
            .map(|m| m.to_vec())
            .unwrap_or_default();
        let mapped: Vec<TypeId> = members
            .iter()
            .map(|&m| get_string_mapping_type(checker, kind, m))
            .collect();
        return checker.get_union_type(&mapped);
    }
    if flags.contains(TypeFlags::STRING_LITERAL) {
        if let Some(LiteralValue::String(s)) = checker.get_type(t).literal_value().cloned() {
            return checker.get_string_literal_type(&apply_string_mapping(kind, &s));
        }
    }
    if flags.contains(TypeFlags::STRING_MAPPING)
        && checker.get_type(t).as_string_mapping().map(|m| m.kind) == Some(kind)
    {
        return t;
    }
    if flags.intersects(TypeFlags::ANY | TypeFlags::STRING | TypeFlags::STRING_MAPPING)
        || is_generic_index_type(checker, t)
        || is_pattern_literal_placeholder_type(checker, t)
    {
        let target = if is_pattern_literal_placeholder_type(checker, t)
            && !flags.intersects(TypeFlags::ANY | TypeFlags::STRING | TypeFlags::STRING_MAPPING)
        {
            get_template_literal_type(checker, &[String::new(), String::new()], &[t])
        } else {
            t
        };
        return checker.new_string_mapping_type(kind, target);
    }
    t
}

/// Reports whether `t` is a pattern literal type (Go's `isPatternLiteralType`).
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.isPatternLiteralType
pub fn is_pattern_literal_type(checker: &Checker, t: TypeId) -> bool {
    let ty = checker.get_type(t);
    let flags = ty.flags();
    if flags.contains(TypeFlags::TEMPLATE_LITERAL) {
        let d = ty.as_template_literal().expect("template literal");
        return d
            .types
            .iter()
            .all(|&ty| is_pattern_literal_placeholder_type(checker, ty));
    }
    if flags.contains(TypeFlags::STRING_MAPPING) {
        let target = ty.as_string_mapping().expect("string mapping").target;
        return is_pattern_literal_placeholder_type(checker, target);
    }
    false
}

/// Re-resolves a deferred conditional type `t` under `mapper` (Go's
/// `getConditionalTypeInstantiation`).
///
/// Side effects: may allocate branch/union/conditional types; caches the
/// instantiation per `(node, type arguments)`.
// Go: internal/checker/checker.go:Checker.getConditionalTypeInstantiation
pub(crate) fn get_conditional_type_instantiation(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    t: TypeId,
    mapper: &TypeMapper,
) -> TypeId {
    let root = checker
        .get_type(t)
        .as_conditional()
        .expect("conditional type")
        .root
        .clone();
    if root.outer_type_parameters.is_empty() {
        return t;
    }
    let type_arguments: Vec<TypeId> = root
        .outer_type_parameters
        .iter()
        .map(|&tp| checker.map_type(mapper, tp))
        .collect();
    if let Some(cached) = checker.conditional_instantiation(root.node, &type_arguments) {
        return cached;
    }
    let new_mapper = TypeMapper::new(&root.outer_type_parameters, &type_arguments);
    let mut result: Option<TypeId> = None;
    if root.is_distributive {
        let distribution_type = checker.map_type(&new_mapper, root.check_type);
        if distribution_type != root.check_type {
            let dflags = checker.get_type(distribution_type).flags();
            if dflags.contains(TypeFlags::UNION) {
                let members = checker
                    .get_type(distribution_type)
                    .union_types()
                    .unwrap_or(&[])
                    .to_vec();
                let mut mapped = Vec::with_capacity(members.len());
                for m in members {
                    let inst_mapper = TypeMapper::merge(
                        TypeMapper::unary(root.check_type, m),
                        new_mapper.clone(),
                    );
                    let r = get_conditional_type(checker, program, &root, Some(&inst_mapper));
                    mapped.push(r);
                }
                result = Some(checker.get_union_type(&mapped));
            } else if dflags.contains(TypeFlags::NEVER) {
                result = Some(checker.never_type());
            }
        }
    }
    let result =
        result.unwrap_or_else(|| get_conditional_type(checker, program, &root, Some(&new_mapper)));
    checker.set_conditional_instantiation(root.node, type_arguments, result);
    result
}

/// Resolves a conditional type `T extends U ? X : Y` for `root` under an
/// optional `mapper` (Go's `getConditionalType`).
///
/// Side effects: may allocate branch/union/conditional types and run inference.
// Go: internal/checker/checker.go:Checker.getConditionalType
pub fn get_conditional_type(
    checker: &mut Checker,
    program: &dyn BoundProgram,
    root: &ConditionalRoot,
    mapper: Option<&TypeMapper>,
) -> TypeId {
    let check_type = match mapper {
        Some(m) => {
            let m = m.clone();
            checker.instantiate_param_type(program, root.check_type, &m)
        }
        None => root.check_type,
    };
    let extends_type = match mapper {
        Some(m) => {
            let m = m.clone();
            checker.instantiate_param_type(program, root.extends_type, &m)
        }
        None => root.extends_type,
    };
    let error = checker.error_type();
    if check_type == error || extends_type == error {
        return error;
    }
    let check_type_deferred = is_generic_type(checker, check_type);
    let mut combined_mapper: Option<TypeMapper> = None;
    if !root.infer_type_parameters.is_empty() {
        let mut context = InferenceContext::new(&root.infer_type_parameters);
        if !check_type_deferred {
            checker.infer_types(program, &mut context.inferences, check_type, extends_type);
        }
        let inference_mapper = checker.get_inference_mapper(program, &mut context);
        combined_mapper = Some(match mapper {
            Some(m) => TypeMapper::merge(inference_mapper, m.clone()),
            None => inference_mapper,
        });
    }
    let inferred_extends_type = match &combined_mapper {
        Some(cm) => {
            let cm = cm.clone();
            checker.instantiate_param_type(program, root.extends_type, &cm)
        }
        None => extends_type,
    };
    if !check_type_deferred && !is_generic_type(checker, inferred_extends_type) {
        let extends_any_or_unknown = checker
            .get_type(inferred_extends_type)
            .flags()
            .intersects(TypeFlags::ANY_OR_UNKNOWN);
        let (true_node, false_node) = conditional_branch_nodes(program, root.node);
        if !extends_any_or_unknown
            && !checker.is_type_assignable_to(program, check_type, inferred_extends_type)
        {
            let false_type = checker.resolve_type_node(program, false_node);
            return match mapper {
                Some(m) => {
                    let m = m.clone();
                    checker.instantiate_param_type(program, false_type, &m)
                }
                None => false_type,
            };
        }
        if extends_any_or_unknown
            || checker.is_type_assignable_to(program, check_type, inferred_extends_type)
        {
            let true_type = checker.resolve_type_node(program, true_node);
            return match combined_mapper.as_ref().or(mapper) {
                Some(m) => {
                    let m = m.clone();
                    checker.instantiate_param_type(program, true_type, &m)
                }
                None => true_type,
            };
        }
    }
    checker.new_conditional_type(root.clone(), mapper.cloned())
}

fn conditional_branch_nodes(program: &dyn BoundProgram, node: NodeId) -> (NodeId, NodeId) {
    match program.arena().data(node) {
        NodeData::ConditionalType(d) => (d.true_type, d.false_type),
        _ => (node, node),
    }
}

fn add_template_spans(
    checker: &mut Checker,
    sb: &mut String,
    new_texts: &mut Vec<String>,
    new_types: &mut Vec<TypeId>,
    texts: &[String],
    types: &[TypeId],
) -> bool {
    for (i, &t) in types.iter().enumerate() {
        let flags = checker.get_type(t).flags();
        if flags.intersects(TypeFlags::LITERAL | TypeFlags::NULL | TypeFlags::UNDEFINED) {
            sb.push_str(&get_template_string_for_type(checker, t));
            sb.push_str(&texts[i + 1]);
        } else if flags.contains(TypeFlags::TEMPLATE_LITERAL) {
            let (inner_texts, inner_types) = {
                let d = checker
                    .get_type(t)
                    .as_template_literal()
                    .expect("template literal");
                (d.texts.clone(), d.types.clone())
            };
            sb.push_str(&inner_texts[0]);
            if !add_template_spans(
                checker,
                sb,
                new_texts,
                new_types,
                &inner_texts,
                &inner_types,
            ) {
                return false;
            }
            sb.push_str(&texts[i + 1]);
        } else if is_generic_index_type(checker, t)
            || is_pattern_literal_placeholder_type(checker, t)
        {
            new_types.push(t);
            new_texts.push(std::mem::take(sb));
            sb.push_str(&texts[i + 1]);
        } else {
            return false;
        }
    }
    true
}

fn get_template_string_for_type(checker: &Checker, t: TypeId) -> String {
    let ty = checker.get_type(t);
    match ty.literal_value() {
        Some(LiteralValue::String(s)) => s.clone(),
        Some(LiteralValue::Number(n)) => n.to_string(),
        Some(LiteralValue::BigInt(bi)) => format!("{bi}n"),
        Some(LiteralValue::Boolean(b)) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        None if ty.flags().intersects(TypeFlags::NULLABLE) => {
            ty.intrinsic_name().unwrap_or_default().to_string()
        }
        None => String::new(),
    }
}

fn is_pattern_literal_placeholder_type(checker: &Checker, t: TypeId) -> bool {
    let flags = checker.get_type(t).flags();
    if flags.contains(TypeFlags::INTERSECTION) {
        let members = union_or_intersection_members(checker, t);
        let mut seen_placeholder = false;
        for m in members {
            let mflags = checker.get_type(m).flags();
            if mflags.intersects(TypeFlags::LITERAL | TypeFlags::NULL | TypeFlags::UNDEFINED)
                || is_pattern_literal_placeholder_type(checker, m)
            {
                seen_placeholder = true;
            } else if !mflags.contains(TypeFlags::OBJECT) {
                return false;
            }
        }
        return seen_placeholder;
    }
    flags.intersects(TypeFlags::ANY | TypeFlags::STRING | TypeFlags::NUMBER | TypeFlags::BIG_INT)
        || is_pattern_literal_type(checker, t)
}

fn is_generic_index_type(checker: &Checker, t: TypeId) -> bool {
    let flags = checker.get_type(t).flags();
    if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
        let members = union_or_intersection_members(checker, t);
        return members.iter().any(|&m| is_generic_index_type(checker, m));
    }
    flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE | TypeFlags::INDEX)
}

fn is_generic_object_type(checker: &Checker, t: TypeId) -> bool {
    let flags = checker.get_type(t).flags();
    if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
        let members = union_or_intersection_members(checker, t);
        return members.iter().any(|&m| is_generic_object_type(checker, m));
    }
    flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
}

fn is_generic_type(checker: &Checker, t: TypeId) -> bool {
    is_generic_object_type(checker, t) || is_generic_index_type(checker, t)
}

fn union_or_intersection_members(checker: &Checker, t: TypeId) -> Vec<TypeId> {
    let ty = checker.get_type(t);
    if let Some(members) = ty.union_types() {
        return members.to_vec();
    }
    if let Some(members) = ty.intersection_types() {
        return members.to_vec();
    }
    vec![t]
}

fn apply_string_mapping(kind: StringMappingKind, s: &str) -> String {
    match kind {
        StringMappingKind::Uppercase => s.to_uppercase(),
        StringMappingKind::Lowercase => s.to_lowercase(),
        StringMappingKind::Capitalize => map_first_char(s, true),
        StringMappingKind::Uncapitalize => map_first_char(s, false),
    }
}

fn map_first_char(s: &str, upper: bool) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let head: String = if upper {
                first.to_uppercase().collect()
            } else {
                first.to_lowercase().collect()
            };
            head + chars.as_str()
        }
        None => String::new(),
    }
}

#[cfg(test)]
#[path = "conditional_types_test.rs"]
mod tests;
