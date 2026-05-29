use super::*;

// Go: internal/core/core.go:Identity
#[test]
fn identity_returns_input() {
    assert_eq!(identity(5), 5);
    assert_eq!(identity("x"), "x");
}

// Go: internal/core/core.go:Map
#[test]
fn map_basic_and_empty() {
    assert_eq!(map(&[1, 2], |x| x * 2), vec![2, 4]);
    let empty: Vec<i32> = Vec::new();
    assert_eq!(map(&empty, |x| x * 2), Vec::<i32>::new());
}

// Go: internal/core/core.go:Filter
#[test]
fn filter_keeps_passing_and_drops_failing() {
    assert_eq!(filter(&[1, 2, 3], |x| *x > 0), vec![1, 2, 3]);
    assert_eq!(filter(&[1, 2, 3, 4], |x| x % 2 == 0), vec![2, 4]);
}

// Go: internal/core/core.go:TryMap
#[test]
fn try_map_ok_and_err() {
    let ok: Result<Vec<i32>, &str> = try_map(&[1, 2], |x| Ok(x * 2));
    assert_eq!(ok, Ok(vec![2, 4]));
    let err: Result<Vec<i32>, &str> =
        try_map(&[1, 2], |x| if *x == 2 { Err("bad") } else { Ok(*x) });
    assert_eq!(err, Err("bad"));
}

// Go: internal/core/core.go:Some
// Go: internal/core/core.go:Every
#[test]
fn some_and_every() {
    assert!(some(&[1, 2], |x| *x > 1));
    assert!(!some(&[1, 2], |x| *x > 5));
    assert!(every(&[1, 2], |x| *x > 0));
    assert!(!every(&[1, 2], |x| *x > 1));
}

// Go: internal/core/core.go:Find
// Go: internal/core/core.go:FindLast
// Go: internal/core/core.go:FindIndex
// Go: internal/core/core.go:FindLastIndex
#[test]
fn find_family() {
    assert_eq!(find(&[1, 2, 3], |x| *x > 1), Some(2));
    assert_eq!(find(&[1, 2, 3], |x| *x > 5), None);
    assert_eq!(find_last(&[1, 2, 3], |x| *x > 1), Some(3));
    assert_eq!(find_index(&[1, 2, 3], |x| *x > 1), Some(1));
    assert_eq!(find_last_index(&[1, 2, 3], |x| *x > 1), Some(2));
    assert_eq!(find_index(&[1, 2, 3], |x| *x > 5), None);
}

// Go: internal/core/core.go:FirstOrNil
// Go: internal/core/core.go:LastOrNil
// Go: internal/core/core.go:ElementOrNil
#[test]
fn first_last_element_or_nil() {
    let empty: Vec<i32> = Vec::new();
    assert_eq!(first_or_nil(&empty), None);
    assert_eq!(first_or_nil(&[1, 2]), Some(1));
    assert_eq!(last_or_nil(&[1, 2]), Some(2));
    assert_eq!(element_or_nil(&[1, 2, 3], 1), Some(2));
    assert_eq!(element_or_nil(&[1], 5), None);
}

// Go: internal/core/core.go:FirstNonNil
// Go: internal/core/core.go:FirstNonZero
#[test]
fn first_non_nil_and_non_zero() {
    assert_eq!(first_non_nil(&[0, 0, 3], |x| *x), Some(3));
    assert_eq!(first_non_nil(&[0, 0], |x| *x), None);
    assert_eq!(first_non_zero(&[0, 0, 5, 7]), 5);
    assert_eq!(first_non_zero(&[0, 0]), 0);
}

// Go: internal/core/core.go:Concatenate
#[test]
fn concatenate_handles_empties() {
    assert_eq!(concatenate(&[1], &[]), vec![1]);
    assert_eq!(concatenate::<i32>(&[], &[2]), vec![2]);
    assert_eq!(concatenate(&[1], &[2]), vec![1, 2]);
}

// Go: internal/core/core.go:Splice
#[test]
fn splice_inserts_and_deletes() {
    assert_eq!(splice(&[1, 2, 3], 1, 1, &[9]), vec![1, 9, 3]);
    assert_eq!(splice(&[1, 2, 3], 0, 0, &[7]), vec![7, 1, 2, 3]);
    // Negative start counts from the end.
    assert_eq!(splice(&[1, 2, 3], -1, 1, &[9]), vec![1, 2, 9]);
}

// Go: internal/core/core.go:CountWhere
#[test]
fn count_where_counts_matches() {
    assert_eq!(count_where(&[1, 2, 3, 4], |x| x % 2 == 0), 2);
}

// Go: internal/core/core.go:ReplaceElement
#[test]
fn replace_element_replaces_index() {
    assert_eq!(replace_element(&[1, 2, 3], 1, 9), vec![1, 9, 3]);
}

// Go: internal/core/core.go:InsertSorted
#[test]
fn insert_sorted_keeps_order() {
    assert_eq!(insert_sorted(&[1, 3, 5], 4, |a, b| a - b), vec![1, 3, 4, 5]);
}

// Go: internal/core/core.go:AppendIfUnique
#[test]
fn append_if_unique_skips_existing() {
    assert_eq!(append_if_unique(vec![1, 2], 2), vec![1, 2]);
    assert_eq!(append_if_unique(vec![1, 2], 3), vec![1, 2, 3]);
}

// Go: internal/core/core.go:MinAllFunc
#[test]
fn min_all_func_returns_all_minima() {
    assert_eq!(min_all_func(&[3, 1, 1, 2], |a, b| a - b), vec![1, 1]);
    assert_eq!(min_all_func::<i32>(&[], |a, b| a - b), Vec::<i32>::new());
}

// Go: internal/core/core.go:Deduplicate
#[test]
fn deduplicate_preserves_first_occurrence() {
    assert_eq!(deduplicate(&[1, 2, 1, 3]), vec![1, 2, 3]);
}

// Go: internal/core/core.go:DeduplicateSorted
#[test]
fn deduplicate_sorted_collapses_runs() {
    assert_eq!(
        deduplicate_sorted(&[1, 1, 2, 3, 3], |a, b| a == b),
        vec![1, 2, 3]
    );
}

// Go: internal/core/core.go:Flatten
#[test]
fn flatten_concatenates() {
    assert_eq!(flatten(&[vec![1, 2], vec![3]]), vec![1, 2, 3]);
}

// Go: internal/core/core.go:IfElse
// Go: internal/core/core.go:OrElse
// Go: internal/core/core.go:Coalesce
#[test]
fn if_else_or_else_coalesce() {
    assert_eq!(if_else(true, 1, 2), 1);
    assert_eq!(if_else(false, 1, 2), 2);
    assert_eq!(or_else(0, 5), 5);
    assert_eq!(or_else(3, 5), 3);
    assert_eq!(coalesce(None, Some(5)), Some(5));
    assert_eq!(coalesce(Some(1), Some(5)), Some(1));
}

// Go: internal/core/core.go:Memoize
#[test]
fn memoize_calls_create_once() {
    let mut count = 0;
    let mut m = memoize(|| {
        count += 1;
        42
    });
    assert_eq!(m(), 42);
    assert_eq!(m(), 42);
    assert_eq!(m(), 42);
    drop(m);
    assert_eq!(count, 1);
}

// Go: internal/core/core.go:DiffMaps
#[test]
fn diff_maps_reports_added_removed_changed() {
    use std::collections::HashMap;
    let m1 = HashMap::from([("a", 1), ("b", 2)]);
    let m2 = HashMap::from([("a", 1), ("c", 3)]);
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    diff_maps(
        &m1,
        &m2,
        |k, v| added.push((*k, *v)),
        |k, v| removed.push((*k, *v)),
        |k, v1, v2| changed.push((*k, *v1, *v2)),
    );
    assert_eq!(added, vec![("c", 3)]);
    assert_eq!(removed, vec![("b", 2)]);
    assert!(changed.is_empty());
}

// Go: internal/core/core.go:CopyMapInto
#[test]
fn copy_map_into_clones_or_merges() {
    use std::collections::HashMap;
    let src = HashMap::from([("a", 1)]);
    let cloned = copy_map_into(None, &src);
    assert_eq!(cloned.get("a"), Some(&1));
    let dst = HashMap::from([("b", 2)]);
    let merged = copy_map_into(Some(dst), &src);
    assert_eq!(merged.get("a"), Some(&1));
    assert_eq!(merged.get("b"), Some(&2));
}

// Go: internal/core/core.go:UnorderedEqual
#[test]
fn unordered_equal_ignores_order() {
    assert!(unordered_equal(&[1, 2, 2], &[2, 1, 2]));
    assert!(!unordered_equal(&[1, 2], &[1, 1]));
    assert!(!unordered_equal(&[1, 2, 3], &[1, 2]));
}

// Go: internal/core/core.go:CompareBooleans
#[test]
fn compare_booleans_orders_true_after_false() {
    assert_eq!(compare_booleans(true, false), 1);
    assert_eq!(compare_booleans(false, true), -1);
    assert_eq!(compare_booleans(true, true), 0);
    assert_eq!(compare_booleans(false, false), 0);
}

// Go: internal/core/core.go:IndexAfter
#[test]
fn index_after_searches_from_start() {
    assert_eq!(index_after("abcabc", "bc", 3), 4);
    assert_eq!(index_after("abcabc", "zz", 0), -1);
}

// Go: internal/core/core.go:Must
#[test]
fn must_unwraps_ok() {
    let r: Result<i32, &str> = Ok(5);
    assert_eq!(must(r), 5);
}

// Go: internal/core/core.go:SingleElementSlice
#[test]
fn single_element_slice_wraps_value() {
    assert_eq!(single_element_slice(Some(5)), vec![5]);
    assert_eq!(single_element_slice::<i32>(None), Vec::<i32>::new());
}

// Go: internal/core/core.go:CheckEachDefined
#[test]
fn check_each_defined_passes_when_all_some() {
    let s = [Some(1), Some(2)];
    assert_eq!(check_each_defined(&s, "boom").len(), 2);
}

// Go: internal/core/core.go:CheckEachDefined
#[test]
#[should_panic(expected = "boom")]
fn check_each_defined_panics_on_none() {
    let s = [Some(1), None];
    check_each_defined(&s, "boom");
}

// Go: internal/core/core.go:GetScriptKindFromFileName
#[test]
fn get_script_kind_from_file_name_maps_extensions() {
    use crate::scriptkind::ScriptKind;
    assert_eq!(get_script_kind_from_file_name("a.ts"), ScriptKind::Ts);
    assert_eq!(get_script_kind_from_file_name("a.tsx"), ScriptKind::Tsx);
    assert_eq!(get_script_kind_from_file_name("a.js"), ScriptKind::Js);
    assert_eq!(get_script_kind_from_file_name("a.jsx"), ScriptKind::Jsx);
    assert_eq!(get_script_kind_from_file_name("a.json"), ScriptKind::Json);
    assert_eq!(get_script_kind_from_file_name("a.MTS"), ScriptKind::Ts);
    assert_eq!(get_script_kind_from_file_name("a.xyz"), ScriptKind::Unknown);
    assert_eq!(get_script_kind_from_file_name("noext"), ScriptKind::Unknown);
}

// Go: internal/core/core.go:StringifyJson
#[test]
fn stringify_json_renders_value() {
    let out = stringify_json(&vec![1, 2, 3], "", "").unwrap();
    assert_eq!(out, "[1,2,3]");
}

// Go: internal/core/core.go:FilterSeq
#[test]
fn filter_seq_lazily_filters() {
    let got: Vec<i32> = filter_seq(&[1, 2, 3, 4], |x| x % 2 == 0).collect();
    assert_eq!(got, vec![2, 4]);
}

// Go: internal/core/core.go:ConcatenateSeq
#[test]
fn concatenate_seq_joins_iterators() {
    let got: Vec<i32> =
        concatenate_seq(vec![vec![1, 2].into_iter(), vec![3].into_iter()]).collect();
    assert_eq!(got, vec![1, 2, 3]);
}

// Go: internal/core/core.go:Enumerate
#[test]
fn enumerate_pairs_index_and_value() {
    let got: Vec<(usize, char)> = enumerate(vec!['a', 'b'].into_iter()).collect();
    assert_eq!(got, vec![(0, 'a'), (1, 'b')]);
}

// Go: internal/core/core.go:FirstOrNilSeq
#[test]
fn first_or_nil_seq_returns_head() {
    assert_eq!(first_or_nil_seq(vec![9, 8].into_iter()), Some(9));
    assert_eq!(first_or_nil_seq(Vec::<i32>::new().into_iter()), None);
}

// Go: internal/core/core.go:ComputeECMALineStarts
#[test]
fn compute_ecma_line_starts_handles_crlf_and_lf() {
    use crate::text::TextPos;
    let starts = compute_ecma_line_starts("a\r\nb\nc");
    assert_eq!(starts, vec![TextPos(0), TextPos(3), TextPos(5)]);
}

// Go: internal/core/core.go:ComputeECMALineStarts
#[test]
fn compute_ecma_line_starts_handles_unicode_line_separator() {
    use crate::text::TextPos;
    // U+2028 LINE SEPARATOR is 3 UTF-8 bytes; "b" therefore starts at byte 4.
    let starts = compute_ecma_line_starts("a\u{2028}b");
    assert_eq!(starts, vec![TextPos(0), TextPos(4)]);
}

// Go: internal/core/core.go:ComputeECMALineStartsSeq
#[test]
fn compute_ecma_line_starts_seq_matches_eager() {
    use crate::text::TextPos;
    let starts: Vec<TextPos> = compute_ecma_line_starts_seq("a\nb").collect();
    assert_eq!(starts, vec![TextPos(0), TextPos(2)]);
}

// Go: internal/core/core.go:PositionToLineAndByteOffset
#[test]
fn position_to_line_and_byte_offset_maps_position() {
    use crate::text::TextPos;
    let line_starts = [TextPos(0), TextPos(3), TextPos(5)];
    assert_eq!(position_to_line_and_byte_offset(0, &line_starts), (0, 0));
    assert_eq!(position_to_line_and_byte_offset(4, &line_starts), (1, 1));
    assert_eq!(position_to_line_and_byte_offset(5, &line_starts), (2, 0));
}

// Go: internal/core/core.go:UTF16Len
#[test]
fn utf16_len_counts_code_units() {
    assert_eq!(utf16_len("abc"), Utf16Offset(3));
    // An astral character occupies two UTF-16 code units.
    assert_eq!(utf16_len("a\u{1F600}b"), Utf16Offset(4));
}

// Go: internal/core/core.go:GetSpellingSuggestionForStrings
#[test]
fn spelling_suggestion_finds_close_candidate() {
    let candidates = vec!["foo".to_string(), "bar".to_string()];
    assert_eq!(
        get_spelling_suggestion_for_strings("fooo", candidates.into_iter()),
        Some("foo".to_string())
    );
}

// Go: internal/core/core.go:GetSpellingSuggestionForStrings
#[test]
fn spelling_suggestion_returns_none_when_too_far() {
    let candidates = vec!["abcd".to_string()];
    assert_eq!(
        get_spelling_suggestion_for_strings("zzzz", candidates.into_iter()),
        None
    );
}

// Go: internal/core/core.go:Or
#[test]
fn or_combines_predicates() {
    let pred = or::<i32>(vec![Box::new(|x| *x < 0), Box::new(|x| *x > 10)]);
    assert!(pred(&-5));
    assert!(pred(&20));
    assert!(!pred(&5));
}

// Go: internal/core/core.go:ApplyDebugStackLimit
#[test]
fn apply_debug_stack_limit_is_safe_to_call() {
    apply_debug_stack_limit();
}

// Go: internal/core/core.go:GetSpellingSuggestion
#[test]
fn spelling_suggestion_skips_short_names_differing_beyond_case() {
    // Candidate shorter than 3 chars only matches if it differs by case only.
    let candidates = vec!["xy".to_string()];
    assert_eq!(
        get_spelling_suggestion_for_strings("ab", candidates.into_iter()),
        None
    );
    let candidates = vec!["AB".to_string()];
    assert_eq!(
        get_spelling_suggestion_for_strings("ab", candidates.into_iter()),
        Some("AB".to_string())
    );
}
