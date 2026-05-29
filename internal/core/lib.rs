//! `tsgo_core` — foundational core utilities for the compiler (iteration/slice
//! helpers, comparators, arena, BFS, option types, and related enums).
//!
//! 1:1 port of Go `internal/core` (`core.go` and friends).

use std::collections::HashMap;
use std::hash::Hash;

use tsgo_tspath::{
    EXTENSION_CJS, EXTENSION_CTS, EXTENSION_JS, EXTENSION_JSON, EXTENSION_JSX, EXTENSION_MJS,
    EXTENSION_MTS, EXTENSION_TS, EXTENSION_TSX,
};

use crate::scriptkind::ScriptKind;
use crate::text::TextPos;

/// Reads `TS_GO_DEBUG_STACK_LIMIT` and, if it parses to a positive integer,
/// would raise the maximum goroutine stack size in Go.
///
/// DIVERGENCE(port): stable Rust has no equivalent of Go's
/// `runtime/debug.SetMaxStack`, so the parsed limit is currently ignored; the
/// environment variable is still read to preserve intent.
///
/// Side effects: reads the process environment.
// Go: internal/core/core.go:ApplyDebugStackLimit
pub fn apply_debug_stack_limit() {
    let Ok(v) = std::env::var("TS_GO_DEBUG_STACK_LIMIT") else {
        return;
    };
    if v.is_empty() {
        return;
    }
    let _parsed = v.parse::<i64>().ok().filter(|&n| n > 0);
}

pub mod arena;
pub mod bfs;
pub mod binarysearch;
pub mod buildoptions;
pub mod compileroptions;
pub mod context;
pub mod languagevariant;
pub mod linkstore;
pub mod nodemodules;
pub mod parsedoptions;
pub mod pattern;
pub mod projectreference;
pub mod scriptkind;
pub mod semaphore;
pub mod stack;
pub mod text;
pub mod textchange;
pub mod tristate;
pub mod typeacquisition;
pub mod version;
pub mod watchoptions;
pub mod workgroup;

/// Returns its argument unchanged.
///
/// Useful as a key/projection function (e.g. in BFS or spelling suggestion).
///
/// # Examples
/// ```
/// assert_eq!(tsgo_core::identity(7), 7);
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Identity
pub fn identity<T>(t: T) -> T {
    t
}

/// Returns the elements of `slice` for which `f` returns true, in order.
///
/// # Examples
/// ```
/// assert_eq!(tsgo_core::filter(&[1, 2, 3, 4], |x| x % 2 == 0), vec![2, 4]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Filter
pub fn filter<T: Clone>(slice: &[T], f: impl Fn(&T) -> bool) -> Vec<T> {
    slice.iter().filter(|x| f(x)).cloned().collect()
}

/// Like [`filter`], but `f` also receives the element index and the full slice.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FilterIndex
pub fn filter_index<T: Clone>(slice: &[T], f: impl Fn(&T, usize, &[T]) -> bool) -> Vec<T> {
    slice
        .iter()
        .enumerate()
        .filter(|(i, x)| f(x, *i, slice))
        .map(|(_, x)| x.clone())
        .collect()
}

/// Applies `f` to each element, collecting the results.
///
/// # Examples
/// ```
/// assert_eq!(tsgo_core::map(&[1, 2], |x| x * 2), vec![2, 4]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Map
pub fn map<T, U>(slice: &[T], f: impl Fn(&T) -> U) -> Vec<U> {
    slice.iter().map(f).collect()
}

/// Applies `f` to each element, short-circuiting on the first error.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:TryMap
pub fn try_map<T, U, E>(slice: &[T], f: impl Fn(&T) -> Result<U, E>) -> Result<Vec<U>, E> {
    slice.iter().map(f).collect()
}

/// Like [`map`], but `f` also receives the element index.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:MapIndex
pub fn map_index<T, U>(slice: &[T], f: impl Fn(&T, usize) -> U) -> Vec<U> {
    slice.iter().enumerate().map(|(i, x)| f(x, i)).collect()
}

/// Maps each element and keeps only the non-default results.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:MapNonNil
pub fn map_non_nil<T, U: Default + PartialEq>(slice: &[T], f: impl Fn(&T) -> U) -> Vec<U> {
    let zero = U::default();
    slice
        .iter()
        .map(f)
        .filter(|mapped| *mapped != zero)
        .collect()
}

/// Maps each element to an optional value, keeping the `Some` results.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:MapFiltered
pub fn map_filtered<T, U>(slice: &[T], f: impl Fn(&T) -> Option<U>) -> Vec<U> {
    slice.iter().filter_map(f).collect()
}

/// Maps each element to a sub-slice and concatenates the results.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FlatMap
pub fn flat_map<T, U>(slice: &[T], f: impl Fn(&T) -> Vec<U>) -> Vec<U> {
    slice.iter().flat_map(f).collect()
}

/// Maps each element with `f`, returning a fresh slice of the same length.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:SameMap
pub fn same_map<T: Clone + PartialEq>(slice: &[T], f: impl Fn(&T) -> T) -> Vec<T> {
    slice.iter().map(f).collect()
}

/// Like [`same_map`], but `f` also receives the element index.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:SameMapIndex
pub fn same_map_index<T: Clone + PartialEq>(slice: &[T], f: impl Fn(&T, usize) -> T) -> Vec<T> {
    slice.iter().enumerate().map(|(i, x)| f(x, i)).collect()
}

/// Reports whether `s1` and `s2` refer to the same backing storage.
///
/// Mirrors Go's identity check (not element-wise equality): two slices are
/// "same" when both are empty, or when they share the same first element.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Same
pub fn same<T>(s1: &[T], s2: &[T]) -> bool {
    if s1.len() == s2.len() {
        return s1.is_empty() || std::ptr::eq(s1.as_ptr(), s2.as_ptr());
    }
    false
}

/// Reports whether any element satisfies `f`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Some
pub fn some<T>(slice: &[T], f: impl Fn(&T) -> bool) -> bool {
    slice.iter().any(f)
}

/// Reports whether every element satisfies `f`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Every
pub fn every<T>(slice: &[T], f: impl Fn(&T) -> bool) -> bool {
    slice.iter().all(f)
}

/// Combines predicates into one that is true when any of them is true.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Or
#[allow(clippy::type_complexity)]
pub fn or<T>(funcs: Vec<Box<dyn Fn(&T) -> bool>>) -> impl Fn(&T) -> bool {
    move |input| funcs.iter().any(|f| f(input))
}

/// Returns the first element satisfying `f`, if any.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Find
pub fn find<T: Clone>(slice: &[T], f: impl Fn(&T) -> bool) -> Option<T> {
    slice.iter().find(|x| f(x)).cloned()
}

/// Returns the last element satisfying `f`, if any.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FindLast
pub fn find_last<T: Clone>(slice: &[T], f: impl Fn(&T) -> bool) -> Option<T> {
    slice.iter().rev().find(|x| f(x)).cloned()
}

/// Returns the index of the first element satisfying `f`, if any.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FindIndex
pub fn find_index<T>(slice: &[T], f: impl Fn(&T) -> bool) -> Option<usize> {
    slice.iter().position(f)
}

/// Returns the index of the last element satisfying `f`, if any.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FindLastIndex
pub fn find_last_index<T>(slice: &[T], f: impl Fn(&T) -> bool) -> Option<usize> {
    slice.iter().rposition(f)
}

/// Returns the first element, if any.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FirstOrNil
pub fn first_or_nil<T: Clone>(slice: &[T]) -> Option<T> {
    slice.first().cloned()
}

/// Returns the last element, if any.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:LastOrNil
pub fn last_or_nil<T: Clone>(slice: &[T]) -> Option<T> {
    slice.last().cloned()
}

/// Returns the element at `index`, or `None` if out of range.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:ElementOrNil
pub fn element_or_nil<T: Clone>(slice: &[T], index: usize) -> Option<T> {
    slice.get(index).cloned()
}

/// Returns the first element of `seq`, if any.
///
/// Side effects: advances `seq` by at most one element.
// Go: internal/core/core.go:FirstOrNilSeq
pub fn first_or_nil_seq<T>(mut seq: impl Iterator<Item = T>) -> Option<T> {
    seq.next()
}

/// Maps each element with `f` and returns the first non-default result.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FirstNonNil
pub fn first_non_nil<T, U: Default + PartialEq>(slice: &[T], f: impl Fn(&T) -> U) -> Option<U> {
    let zero = U::default();
    slice.iter().map(f).find(|mapped| *mapped != zero)
}

/// Returns the first non-default value, or the default if all are default.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FirstNonZero
pub fn first_non_zero<T: Default + PartialEq + Clone>(values: &[T]) -> T {
    let zero = T::default();
    values.iter().find(|v| **v != zero).cloned().unwrap_or(zero)
}

/// Concatenates two slices, short-circuiting when one is empty.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Concatenate
pub fn concatenate<T: Clone>(s1: &[T], s2: &[T]) -> Vec<T> {
    if s2.is_empty() {
        return s1.to_vec();
    }
    if s1.is_empty() {
        return s2.to_vec();
    }
    [s1, s2].concat()
}

/// Removes `delete_count` elements at `start` and inserts `items` there,
/// returning the resulting slice. `start` may be negative (counted from end).
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Splice
pub fn splice<T: Clone>(s1: &[T], start: i32, delete_count: i32, items: &[T]) -> Vec<T> {
    let len = s1.len() as i32;
    let mut start = start;
    if start < 0 {
        start += len;
    }
    if start < 0 {
        start = 0;
    }
    if start > len {
        start = len;
    }
    let delete_count = delete_count.max(0);
    let end = (start + delete_count).min(len);
    let (start, end) = (start as usize, end as usize);
    let mut result = Vec::with_capacity(start + items.len() + (s1.len() - end));
    result.extend_from_slice(&s1[..start]);
    result.extend_from_slice(items);
    result.extend_from_slice(&s1[end..]);
    result
}

/// Counts the elements that satisfy `f`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:CountWhere
pub fn count_where<T>(slice: &[T], f: impl Fn(&T) -> bool) -> i32 {
    slice.iter().filter(|x| f(x)).count() as i32
}

/// Returns a copy of `slice` with the element at index `i` replaced by `t`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:ReplaceElement
pub fn replace_element<T: Clone>(slice: &[T], i: usize, t: T) -> Vec<T> {
    let mut result = slice.to_vec();
    result[i] = t;
    result
}

/// Inserts `element` into an already-sorted slice, preserving order per `cmp`.
///
/// `cmp` returns a negative, zero, or positive value as `a` orders before,
/// equal to, or after `b`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:InsertSorted
pub fn insert_sorted<T: Clone>(slice: &[T], element: T, cmp: impl Fn(&T, &T) -> i32) -> Vec<T> {
    let i = slice.partition_point(|x| cmp(x, &element) < 0);
    let mut result = Vec::with_capacity(slice.len() + 1);
    result.extend_from_slice(&slice[..i]);
    result.push(element);
    result.extend_from_slice(&slice[i..]);
    result
}

/// Returns all minimum elements of `xs` according to `cmp`, in input order.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:MinAllFunc
pub fn min_all_func<T: Clone>(xs: &[T], cmp: impl Fn(&T, &T) -> i32) -> Vec<T> {
    if xs.is_empty() {
        return Vec::new();
    }
    let mut m = xs[0].clone();
    let mut mins = vec![m.clone()];
    for x in &xs[1..] {
        let c = cmp(x, &m);
        if c < 0 {
            m = x.clone();
            mins.clear();
            mins.push(x.clone());
        } else if c == 0 {
            mins.push(x.clone());
        }
    }
    mins
}

/// Appends `element` to `slice` unless it is already present.
///
/// Side effects: none (pure); consumes and returns `slice`.
// Go: internal/core/core.go:AppendIfUnique
pub fn append_if_unique<T: PartialEq>(mut slice: Vec<T>, element: T) -> Vec<T> {
    if !slice.contains(&element) {
        slice.push(element);
    }
    slice
}

/// Returns a copy of `slice` with consecutive *and* non-consecutive duplicates
/// removed, keeping the first occurrence of each element.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Deduplicate
pub fn deduplicate<T: PartialEq + Clone>(slice: &[T]) -> Vec<T> {
    let mut result: Vec<T> = Vec::new();
    for value in slice {
        if !result.contains(value) {
            result.push(value.clone());
        }
    }
    result
}

/// Removes adjacent duplicates from an already-sorted slice using `is_equal`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:DeduplicateSorted
pub fn deduplicate_sorted<T: Clone>(slice: &[T], is_equal: impl Fn(&T, &T) -> bool) -> Vec<T> {
    if slice.is_empty() {
        return Vec::new();
    }
    let mut result = vec![slice[0].clone()];
    let mut last = &slice[0];
    for next in &slice[1..] {
        if is_equal(last, next) {
            continue;
        }
        result.push(next.clone());
        last = next;
    }
    result
}

/// Flattens a slice of slices into a single slice.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Flatten
pub fn flatten<T: Clone>(array: &[Vec<T>]) -> Vec<T> {
    array.concat()
}

/// Returns `when_true` if `b`, otherwise `when_false`.
///
/// Both branches are always evaluated, so use only with constant/precomputed
/// values (mirrors the Go contract).
///
/// Side effects: none (pure).
// Go: internal/core/core.go:IfElse
pub fn if_else<T>(b: bool, when_true: T, when_false: T) -> T {
    if b {
        when_true
    } else {
        when_false
    }
}

/// Returns `value` if it is non-default, otherwise `default_value`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:OrElse
pub fn or_else<T: Default + PartialEq>(value: T, default_value: T) -> T {
    if value != T::default() {
        value
    } else {
        default_value
    }
}

/// Returns `a` if it is `Some`, otherwise `b` (non-short-circuiting analog of
/// `??`).
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Coalesce
pub fn coalesce<T>(a: Option<T>, b: Option<T>) -> Option<T> {
    a.or(b)
}

/// Wraps `create` in a closure that computes the value at most once and caches
/// it for subsequent calls.
///
/// Side effects: the returned closure mutates its captured cache.
// Go: internal/core/core.go:Memoize
pub fn memoize<T: Clone>(create: impl FnOnce() -> T) -> impl FnMut() -> T {
    let mut create = Some(create);
    let mut value: Option<T> = None;
    move || {
        if let Some(c) = create.take() {
            value = Some(c());
        }
        value.clone().expect("memoized value initialized")
    }
}

/// Diffs two maps, invoking callbacks for entries added in `m2`, removed from
/// `m1`, and whose values changed.
///
/// Unlike Go (which accepts nil callbacks), all three callbacks are required;
/// pass no-ops to ignore a category.
///
/// Side effects: invokes the supplied callbacks.
// Go: internal/core/core.go:DiffMaps
pub fn diff_maps<K, V>(
    m1: &HashMap<K, V>,
    m2: &HashMap<K, V>,
    on_added: impl FnMut(&K, &V),
    on_removed: impl FnMut(&K, &V),
    on_changed: impl FnMut(&K, &V, &V),
) where
    K: Eq + Hash,
    V: PartialEq,
{
    diff_maps_func(m1, m2, |a, b| a == b, on_added, on_removed, on_changed);
}

/// Like [`diff_maps`], but with heterogeneous value types and a custom
/// value-equality predicate.
///
/// Side effects: invokes the supplied callbacks.
// Go: internal/core/core.go:DiffMapsFunc
pub fn diff_maps_func<K, V1, V2>(
    m1: &HashMap<K, V1>,
    m2: &HashMap<K, V2>,
    mut equal_values: impl FnMut(&V1, &V2) -> bool,
    mut on_added: impl FnMut(&K, &V2),
    mut on_removed: impl FnMut(&K, &V1),
    mut on_changed: impl FnMut(&K, &V1, &V2),
) where
    K: Eq + Hash,
{
    for (k, v2) in m2 {
        if !m1.contains_key(k) {
            on_added(k, v2);
        }
    }
    for (k, v1) in m1 {
        match m2.get(k) {
            Some(v2) => {
                if !equal_values(v1, v2) {
                    on_changed(k, v1, v2);
                }
            }
            None => on_removed(k, v1),
        }
    }
}

/// Copies `src` into `dst` when `dst` is present, otherwise clones `src`.
///
/// Side effects: none (pure); consumes and returns the destination map.
// Go: internal/core/core.go:CopyMapInto
pub fn copy_map_into<K: Eq + Hash + Clone, V: Clone>(
    dst: Option<HashMap<K, V>>,
    src: &HashMap<K, V>,
) -> HashMap<K, V> {
    match dst {
        None => src.clone(),
        Some(mut dst) => {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
            dst
        }
    }
}

/// Reports whether `s1` and `s2` contain the same elements, ignoring order.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:UnorderedEqual
pub fn unordered_equal<T: Eq + Hash>(s1: &[T], s2: &[T]) -> bool {
    if s1.len() != s2.len() {
        return false;
    }
    let mut counts: HashMap<&T, i32> = HashMap::new();
    for v in s1 {
        *counts.entry(v).or_insert(0) += 1;
    }
    for v in s2 {
        let c = counts.entry(v).or_insert(0);
        *c -= 1;
        if *c < 0 {
            return false;
        }
    }
    true
}

/// Compares two booleans, treating `true` as greater than `false`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:CompareBooleans
pub fn compare_booleans(a: bool, b: bool) -> i32 {
    if a && !b {
        1
    } else if !a && b {
        -1
    } else {
        0
    }
}

/// Returns the byte index of `pattern` in `s` at or after `start_index`, or
/// `-1` if not found.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:IndexAfter
pub fn index_after(s: &str, pattern: &str, start_index: usize) -> i32 {
    match s[start_index..].find(pattern) {
        Some(m) => (m + start_index) as i32,
        None => -1,
    }
}

/// Unwraps an `Ok` value, panicking on `Err` (mirrors Go `Must`).
///
/// Side effects: panics on `Err`.
// Go: internal/core/core.go:Must
pub fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap()
}

/// Returns a one-element vector if `element` is present, else an empty vector.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:SingleElementSlice
pub fn single_element_slice<T>(element: Option<T>) -> Vec<T> {
    element.into_iter().collect()
}

/// Validates that every element is `Some`, panicking with `msg` otherwise, and
/// returns the input slice unchanged for chaining.
///
/// Side effects: panics if any element is `None`.
// Go: internal/core/core.go:CheckEachDefined
pub fn check_each_defined<'a, T>(s: &'a [Option<T>], msg: &str) -> &'a [Option<T>] {
    for value in s {
        if value.is_none() {
            panic!("{msg}");
        }
    }
    s
}

/// Returns the [`ScriptKind`] implied by a file name's extension.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:GetScriptKindFromFileName
pub fn get_script_kind_from_file_name(file_name: &str) -> ScriptKind {
    if let Some(dot_pos) = file_name.rfind('.') {
        let ext = file_name[dot_pos..].to_ascii_lowercase();
        match ext.as_str() {
            EXTENSION_JS | EXTENSION_CJS | EXTENSION_MJS => return ScriptKind::Js,
            EXTENSION_JSX => return ScriptKind::Jsx,
            EXTENSION_TS | EXTENSION_CTS | EXTENSION_MTS => return ScriptKind::Ts,
            EXTENSION_TSX => return ScriptKind::Tsx,
            EXTENSION_JSON => return ScriptKind::Json,
            _ => {}
        }
    }
    ScriptKind::Unknown
}

/// Serializes `input` to a JSON string with the given prefix and indent.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:StringifyJson
pub fn stringify_json<T: serde::Serialize>(
    input: &T,
    prefix: &str,
    indent: &str,
) -> Result<String, tsgo_json::Error> {
    let bytes = tsgo_json::marshal_indent(input, prefix, indent)?;
    Ok(String::from_utf8(bytes).expect("json output is valid utf-8"))
}

/// Returns a lazy iterator over the elements of `slice` that satisfy `f`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:FilterSeq
pub fn filter_seq<'a, T: Clone>(
    slice: &'a [T],
    f: impl Fn(&T) -> bool + 'a,
) -> impl Iterator<Item = T> + 'a {
    slice.iter().filter(move |x| f(x)).cloned()
}

/// Chains several iterators into one.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:ConcatenateSeq
pub fn concatenate_seq<T, I: Iterator<Item = T>>(seqs: Vec<I>) -> impl Iterator<Item = T> {
    seqs.into_iter().flatten()
}

/// Pairs each element of `seq` with its 0-based index.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:Enumerate
pub fn enumerate<T>(seq: impl Iterator<Item = T>) -> impl Iterator<Item = (usize, T)> {
    seq.enumerate()
}

/// The byte offsets at which each line of source text begins.
// Go: internal/core/core.go:ECMALineStarts
pub type EcmaLineStarts = Vec<TextPos>;

/// A character offset measured in UTF-16 code units.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
// Go: internal/core/core.go:UTF16Offset
pub struct Utf16Offset(pub i32);

/// Computes the byte offsets of each line start in `text`, following the ECMA
/// line-terminator rules (`\r`, `\n`, `\r\n`, U+2028, U+2029).
///
/// The result always contains at least one entry (the start of the first line).
///
/// Side effects: none (pure).
// Go: internal/core/core.go:ComputeECMALineStarts
pub fn compute_ecma_line_starts(text: &str) -> EcmaLineStarts {
    let bytes = text.as_bytes();
    let text_len = bytes.len();
    let mut result: EcmaLineStarts = Vec::new();
    let mut pos = 0usize;
    let mut line_start = 0usize;
    while pos < text_len {
        let b = bytes[pos];
        if b < 0x80 {
            pos += 1;
            match b {
                b'\r' => {
                    if pos < text_len && bytes[pos] == b'\n' {
                        pos += 1;
                    }
                    result.push(TextPos(line_start as i32));
                    line_start = pos;
                }
                b'\n' => {
                    result.push(TextPos(line_start as i32));
                    line_start = pos;
                }
                _ => {}
            }
        } else {
            // `pos` is at a UTF-8 char boundary, so the next char decodes cleanly.
            let ch = text[pos..].chars().next().unwrap();
            pos += ch.len_utf8();
            if tsgo_stringutil::is_line_break(ch) {
                result.push(TextPos(line_start as i32));
                line_start = pos;
            }
        }
    }
    result.push(TextPos(line_start as i32));
    result
}

/// Iterator form of [`compute_ecma_line_starts`].
///
/// PERF(port): the Go `iter.Seq` is lazy; this eager wrapper computes the full
/// slice first, which is behaviorally identical for all consumers.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:ComputeECMALineStartsSeq
pub fn compute_ecma_line_starts_seq(text: &str) -> impl Iterator<Item = TextPos> {
    compute_ecma_line_starts(text).into_iter()
}

/// Maps a byte `position` to its 0-based line and byte offset within that line,
/// using the given line starts.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:PositionToLineAndByteOffset
pub fn position_to_line_and_byte_offset(position: i32, line_starts: &[TextPos]) -> (i32, i32) {
    let idx = line_starts.partition_point(|ls| ls.0 <= position);
    let line = (idx as i32 - 1).max(0);
    let offset = position - line_starts[line as usize].0;
    (line, offset)
}

/// Returns the number of UTF-16 code units needed to represent `s`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:UTF16Len
pub fn utf16_len(s: &str) -> Utf16Offset {
    Utf16Offset(s.encode_utf16().count() as i32)
}

// Reports whether `a` and `b` are equal ignoring (Unicode) case.
//
// Side effects: none (pure).
fn chars_eq_ignore_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

// Reports whether two strings are equal under simple Unicode case folding.
//
// Side effects: none (pure).
fn equal_fold(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

// Three-way string comparison returning -1, 0, or 1 (mirrors `strings.Compare`).
//
// Side effects: none (pure).
fn str_compare(a: &str, b: &str) -> i32 {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match a.cmp(b) {
        Less => -1,
        Equal => 0,
        Greater => 1,
    }
}

// Banded Levenshtein distance with an upper bound. Returns the (weighted)
// distance, or `-1.0` when it provably exceeds `max_value`.
//
// PERF(port): Go reuses pooled buffers via `sync.Pool`; here we allocate two
// row buffers per call.
//
// Side effects: none (pure).
fn levenshtein_with_max(s1: &[char], s2: &[char], max_value: f64) -> f64 {
    let s2_len = s2.len();
    let buffer_size = s2_len + 1;
    let mut previous = vec![0f64; buffer_size];
    let mut current = vec![0f64; buffer_size];
    let big = max_value + 0.01;
    for (i, p) in previous.iter_mut().enumerate() {
        *p = i as f64;
    }
    for i in 1..=s1.len() {
        let c1 = s1[i - 1];
        let min_j = (((i as f64) - max_value).ceil() as i64).max(1) as usize;
        let max_j = ((max_value + i as f64).floor() as i64)
            .min(s2_len as i64)
            .max(0) as usize;
        let mut col_min = i as f64;
        current[0] = col_min;
        current[1..min_j].fill(big);
        for j in min_j..=max_j {
            if j == 0 {
                continue;
            }
            let substitution_distance = if chars_eq_ignore_case(s1[i - 1], s2[j - 1]) {
                previous[j - 1] + 0.1
            } else {
                previous[j - 1] + 2.0
            };
            let dist = if c1 == s2[j - 1] {
                previous[j - 1]
            } else {
                (previous[j] + 1.0).min((current[j - 1] + 1.0).min(substitution_distance))
            };
            current[j] = dist;
            col_min = col_min.min(dist);
        }
        current[(max_j + 1)..(s2_len + 1)].fill(big);
        if col_min > max_value {
            // Every entry in this column exceeds the bound; no future column can
            // recover, so give up.
            return -1.0;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let res = previous[s2_len];
    if res > max_value {
        return -1.0;
    }
    res
}

/// Returns the candidate from `candidates` whose name is the closest spelling
/// match for `name`, if one is close enough.
///
/// Candidates whose length differs from `name` by more than ~34%, or whose
/// weighted Levenshtein distance exceeds ~40% of `name`'s length, are rejected.
/// Names shorter than 3 bytes only match when they differ from `name` by case.
/// Ties are broken with `compare`.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:GetSpellingSuggestion
pub fn get_spelling_suggestion<T>(
    name: &str,
    candidates: impl Iterator<Item = T>,
    get_name: impl Fn(&T) -> String,
    compare: impl Fn(&T, &T) -> i32,
) -> Option<T> {
    let maximum_length_difference = std::cmp::max(2, (name.len() as f64 * 0.34) as i64) as usize;
    let mut best_distance = (name.len() as f64 * 0.4).floor() + 0.9;
    let rune_name: Vec<char> = name.chars().collect();
    let mut best_candidate: Option<T> = None;
    for candidate in candidates {
        let candidate_name = get_name(&candidate);
        let max_len = candidate_name.len().max(name.len());
        let min_len = candidate_name.len().min(name.len());
        if candidate_name.is_empty() || max_len - min_len > maximum_length_difference {
            continue;
        }
        if candidate_name == name {
            continue;
        }
        // Only consider candidates shorter than 3 bytes when they differ by case.
        if candidate_name.len() < 3 && !equal_fold(&candidate_name, name) {
            continue;
        }
        let candidate_runes: Vec<char> = candidate_name.chars().collect();
        let distance = levenshtein_with_max(&rune_name, &candidate_runes, best_distance);
        if distance < 0.0 {
            continue;
        }
        debug_assert!(distance <= best_distance);
        if distance < best_distance {
            best_distance = distance;
            best_candidate = Some(candidate);
        } else if best_candidate.is_none()
            || compare(&candidate, best_candidate.as_ref().unwrap()) < 0
        {
            best_candidate = Some(candidate);
        }
    }
    best_candidate
}

/// Convenience wrapper over [`get_spelling_suggestion`] for string candidates.
///
/// Side effects: none (pure).
// Go: internal/core/core.go:GetSpellingSuggestionForStrings
pub fn get_spelling_suggestion_for_strings(
    name: &str,
    candidates: impl Iterator<Item = String>,
) -> Option<String> {
    get_spelling_suggestion(name, candidates, |s| s.clone(), |a, b| str_compare(a, b))
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
