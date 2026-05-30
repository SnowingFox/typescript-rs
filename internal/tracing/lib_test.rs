use super::*;

use tsgo_ast::{NodeId, Symbol};
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

// Builds the in-memory file system the Go tests use: a single `/trace`
// directory, case-sensitive file names. A `.keep` placeholder materializes the
// directory (Go seeds it with `fstest.MapFile{Mode: fs.ModeDir}`).
// Go: internal/tracing/tracing_test.go:TestConcurrentDurationEventsUseSeparateThreadIDs
fn make_trace_fs() -> MapFs {
    MapFs::from_map([("/trace/.keep", "")], true)
}

// Reads `/trace/trace.json` back and parses it into the trace event vector,
// mirroring the Go tests' `json.Unmarshal([]byte(traceText), &events)`.
fn read_events(fs: &MapFs) -> Vec<TraceEvent> {
    let text = fs
        .read_file("/trace/trace.json")
        .expect("trace.json must exist");
    tsgo_json::unmarshal(text.as_bytes()).expect("trace.json must parse")
}

// Finds the first event matching phase + name (and optional `args[key] == value`).
// Go: internal/tracing/tracing_test.go:findEvent
fn find_event<'a>(
    events: &'a [TraceEvent],
    ph: &str,
    name: &str,
    arg: Option<(&str, ArgValue)>,
) -> &'a TraceEvent {
    for e in events {
        if e.ph == ph && e.name == name {
            match &arg {
                None => return e,
                Some((k, v)) => {
                    if e.args.as_ref().and_then(|m| m.get(*k)) == Some(v) {
                        return e;
                    }
                }
            }
        }
    }
    panic!("failed to find {ph} event {name:?} with arg {arg:?}");
}

// Asserts a `thread_name` metadata event exists naming thread `tid`.
// Go: internal/tracing/tracing_test.go:assertThreadName
fn assert_thread_name(events: &[TraceEvent], tid: i32, name: &str) {
    let found = events.iter().any(|e| {
        e.ph == "M"
            && e.name == "thread_name"
            && e.tid == tid
            && e.args.as_ref().and_then(|m| m.get("name")) == Some(&ArgValue::Str(name.to_string()))
    });
    assert!(found, "missing thread_name for thread {tid} named {name:?}");
}

// Verifies that on every thread, begin/end events are strictly paired with
// matching cat/name and every stack ends empty.
// Go: internal/tracing/tracing_test.go:assertDurationEventsAreWellNestedByThread
fn assert_well_nested(events: &[TraceEvent]) {
    use std::collections::HashMap;
    let mut stacks: HashMap<i32, Vec<&TraceEvent>> = HashMap::new();
    for e in events {
        match e.ph.as_str() {
            "B" => stacks.entry(e.tid).or_default().push(e),
            "E" => {
                let stack = stacks.get_mut(&e.tid).expect("unmatched end event");
                let begin = stack.pop().expect("unmatched end event");
                assert_eq!(begin.cat, e.cat);
                assert_eq!(begin.name, e.name);
            }
            _ => {}
        }
    }
    for (tid, stack) in &stacks {
        assert!(stack.is_empty(), "thread {tid} has unterminated events");
    }
}

// Builds a `{ "path": <p> }` args map.
fn path_args(p: &str) -> Args {
    let mut m = Args::new();
    m.insert("path".to_string(), ArgValue::Str(p.to_string()));
    m
}

// Go: internal/tracing/tracing.go:Push (separateBeginAndEnd=true)
#[test]
fn push_begin_end_pair_well_nested() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    let end = tr.push(Phase::Parse, "createSourceFile", None, true);
    end();
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    let begin = find_event(&events, "B", "createSourceFile", None);
    let end_ev = find_event(&events, "E", "createSourceFile", None);
    assert_eq!(begin.tid, end_ev.tid);
    assert_eq!(begin.cat, "parse");
    assert_eq!(end_ev.cat, "parse");
    assert_well_nested(&events);
}

// Go: internal/tracing/tracing_test.go:TestConcurrentDurationEventsUseSeparateThreadIDs (file threads)
#[test]
fn distinct_files_get_distinct_thread_ids() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    let end_a = tr.push(
        Phase::Parse,
        "createSourceFile",
        Some(path_args("/a.ts")),
        true,
    );
    let end_b = tr.push(
        Phase::Parse,
        "createSourceFile",
        Some(path_args("/b.ts")),
        true,
    );
    end_a();
    end_b();
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    let a_path = ArgValue::Str("/a.ts".to_string());
    let b_path = ArgValue::Str("/b.ts".to_string());
    let a_begin = find_event(
        &events,
        "B",
        "createSourceFile",
        Some(("path", a_path.clone())),
    );
    let a_end = find_event(&events, "E", "createSourceFile", Some(("path", a_path)));
    let b_begin = find_event(
        &events,
        "B",
        "createSourceFile",
        Some(("path", b_path.clone())),
    );
    let b_end = find_event(&events, "E", "createSourceFile", Some(("path", b_path)));
    assert_eq!(a_begin.tid, a_end.tid);
    assert_eq!(b_begin.tid, b_end.tid);
    assert_ne!(a_begin.tid, b_begin.tid);
    assert_thread_name(&events, a_begin.tid, "file:/a.ts");
    assert_thread_name(&events, b_begin.tid, "file:/b.ts");
    assert_well_nested(&events);
}

// Go: internal/tracing/tracing_test.go:TestConcurrentDurationEventsUseSeparateThreadIDs (checker thread)
#[test]
fn checker_events_share_thread_id_and_json_number_arg() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();

    let mut check_args = Args::new();
    check_args.insert("checkerId".to_string(), ArgValue::Int(0));
    check_args.insert("path".to_string(), ArgValue::Str("/a.ts".to_string()));
    let mut variance_args = Args::new();
    variance_args.insert("checkerId".to_string(), ArgValue::Int(0));
    variance_args.insert("id".to_string(), ArgValue::Int(1));

    let end_check = tr.push(Phase::Check, "checkSourceFile", Some(check_args), true);
    let end_variance = tr.push(
        Phase::CheckTypes,
        "getVariancesWorker",
        Some(variance_args),
        true,
    );
    end_variance();
    end_check();
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    let check_begin = find_event(
        &events,
        "B",
        "checkSourceFile",
        Some(("path", ArgValue::Str("/a.ts".to_string()))),
    );
    // `id` survives the JSON round-trip as a number (Go asserts `float64(1)`).
    let variance_begin = find_event(
        &events,
        "B",
        "getVariancesWorker",
        Some(("id", ArgValue::Int(1))),
    );
    assert_eq!(check_begin.tid, variance_begin.tid);
    // checkerId wins over the path key, so the checker takes a synthetic id.
    assert_thread_name(&events, check_begin.tid, "checker:0");
    assert_well_nested(&events);
}

// Collects the `path -> thread id` map for a deterministic session over `paths`.
// Go: internal/tracing/tracing_test.go:traceThreadIDsForPaths
fn trace_thread_ids_for_paths(paths: &[&str]) -> std::collections::HashMap<String, i32> {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    for p in paths {
        let end = tr.push(Phase::Parse, "createSourceFile", Some(path_args(p)), true);
        end();
    }
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    let mut map = std::collections::HashMap::new();
    for p in paths {
        let begin = find_event(
            &events,
            "B",
            "createSourceFile",
            Some(("path", ArgValue::Str(p.to_string()))),
        );
        map.insert(p.to_string(), begin.tid);
    }
    map
}

// Go: internal/tracing/tracing_test.go:TestThreadIDsAreStableAcrossFirstSeenOrder
#[test]
fn thread_ids_are_stable_across_first_seen_order() {
    let first = trace_thread_ids_for_paths(&["/a.ts", "/b.ts"]);
    let second = trace_thread_ids_for_paths(&["/b.ts", "/a.ts"]);
    assert_eq!(first, second);
}

// Go: internal/tracing/tracing.go:Instant
#[test]
fn instant_event_scope_global() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    tr.instant(Phase::Program, "createProgram", None);
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    let ev = find_event(&events, "I", "createProgram", None);
    assert_eq!(ev.s, "g");
    assert_eq!(ev.cat, "program");
}

// Go: internal/tracing/tracing.go:Push (deterministic sampled branch returns a no-op)
#[test]
fn deterministic_skips_sampled_events() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    let end = tr.push(Phase::Parse, "x", None, false);
    end();
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    assert!(events.iter().all(|e| e.ph != "X"));
    // Only the three header metadata events remain.
    assert_eq!(events.len(), 3);
}

// Go: internal/tracing/tracing.go:timestamp (deterministic monotonic counter)
#[test]
fn deterministic_timestamps_monotonic() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    let e1 = tr.push(Phase::Parse, "a", None, true);
    e1();
    let e2 = tr.push(Phase::Parse, "b", None, true);
    e2();
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    for e in &events {
        assert_eq!(e.ts.fract(), 0.0, "deterministic timestamps are integers");
    }
    let durations: Vec<f64> = events
        .iter()
        .filter(|e| e.ph == "B" || e.ph == "E")
        .map(|e| e.ts)
        .collect();
    for w in durations.windows(2) {
        assert!(
            w[0] < w[1],
            "timestamps not strictly increasing: {durations:?}"
        );
    }
}

// A configurable in-test implementation of the checker-supplied trait. Every
// accessor defaults to "absent"; tests set only the fields they exercise.
// Go: internal/tracing/tracing.go:TracedType (checker's *Type implements this)
#[derive(Default)]
struct FakeType {
    id: u32,
    conditional: bool,
    recursion: Option<RecursionId>,
}

impl TracedType for FakeType {
    fn id(&self) -> u32 {
        self.id
    }
    fn format_flags(&self) -> Vec<String> {
        Vec::new()
    }
    fn is_conditional(&self) -> bool {
        self.conditional
    }
    fn symbol(&self) -> Option<&Symbol> {
        None
    }
    fn alias_symbol(&self) -> Option<&Symbol> {
        None
    }
    fn alias_type_arguments(&self) -> Vec<&dyn TracedType> {
        Vec::new()
    }
    fn intrinsic_name(&self) -> String {
        String::new()
    }
    fn union_types(&self) -> Vec<&dyn TracedType> {
        Vec::new()
    }
    fn intersection_types(&self) -> Vec<&dyn TracedType> {
        Vec::new()
    }
    fn index_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn indexed_access_object_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn indexed_access_index_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn conditional_check_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn conditional_extends_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn conditional_true_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn conditional_false_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn substitution_base_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn substitution_constraint_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn reference_target(&self) -> Option<&dyn TracedType> {
        None
    }
    fn reference_type_arguments(&self) -> Vec<&dyn TracedType> {
        Vec::new()
    }
    fn reference_node(&self) -> Option<NodeId> {
        None
    }
    fn reverse_mapped_source_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn reverse_mapped_mapped_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn reverse_mapped_constraint_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn evolving_array_element_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn evolving_array_final_type(&self) -> Option<&dyn TracedType> {
        None
    }
    fn is_tuple(&self) -> bool {
        false
    }
    fn pattern(&self) -> Option<NodeId> {
        None
    }
    fn recursion_identity(&self) -> Option<RecursionId> {
        self.recursion
    }
    fn display(&self) -> String {
        String::new()
    }
}

// Reads `types_<n>.json` back as raw JSON values.
fn read_types(fs: &MapFs, checker_index: i32) -> Vec<tsgo_json::Value> {
    let path = format!("/trace/types_{checker_index}.json");
    let text = fs.read_file(&path).expect("types file must exist");
    tsgo_json::unmarshal(text.as_bytes()).expect("types file must parse")
}

// Reads `legend.json` back.
fn read_legend(fs: &MapFs) -> Vec<TraceRecord> {
    let text = fs
        .read_file("/trace/legend.json")
        .expect("legend.json must exist");
    tsgo_json::unmarshal(text.as_bytes()).expect("legend.json must parse")
}

// Go: internal/tracing/tracing.go:StopTracing (slices.SortFunc on legend by typesPath)
#[test]
fn legend_sorted_by_types_path() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "/tsconfig.json", true).unwrap();
    let _t2 = tr.new_type_tracer(2);
    let _t0 = tr.new_type_tracer(0);
    let _t1 = tr.new_type_tracer(1);
    tr.stop_tracing().unwrap();

    let legend = read_legend(&fs);
    let paths: Vec<&str> = legend.iter().map(|r| r.types_path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
    assert_eq!(legend[0].checker_id, 0);
    assert_eq!(legend[1].checker_id, 1);
    assert_eq!(legend[2].checker_id, 2);
}

// Go: internal/tracing/tracing.go:buildTypeDescriptor (unresolved conditional branches -> -1)
#[test]
fn type_descriptor_unresolved_conditional_minus_one() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    let tracer = tr.new_type_tracer(0);
    tracer.record_type(Box::new(FakeType {
        id: 1,
        conditional: true,
        recursion: None,
    }));
    tr.stop_tracing().unwrap();

    let types = read_types(&fs, 0);
    assert_eq!(types.len(), 1);
    assert_eq!(
        types[0].get("conditionalTrueType").and_then(|v| v.as_i64()),
        Some(-1)
    );
    assert_eq!(
        types[0]
            .get("conditionalFalseType")
            .and_then(|v| v.as_i64()),
        Some(-1)
    );
}

// Go: internal/tracing/tracing.go:buildTypeDescriptor (recursion identity -> stable token)
#[test]
fn type_descriptor_recursion_token_stable() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    let tracer = tr.new_type_tracer(0);
    tracer.record_type(Box::new(FakeType {
        id: 1,
        recursion: Some(RecursionId(42)),
        ..Default::default()
    }));
    tracer.record_type(Box::new(FakeType {
        id: 2,
        recursion: Some(RecursionId(42)),
        ..Default::default()
    }));
    tr.stop_tracing().unwrap();

    let types = read_types(&fs, 0);
    assert_eq!(types.len(), 2);
    let t0 = types[0].get("recursionId").and_then(|v| v.as_i64());
    let t1 = types[1].get("recursionId").and_then(|v| v.as_i64());
    assert_eq!(t0, Some(0));
    assert_eq!(t1, Some(0));
}

// Go: internal/tracing/tracing.go:DumpTypes (open bracket has no newline so id == line number)
#[test]
fn dump_types_open_bracket_no_newline() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    let tracer = tr.new_type_tracer(0);
    tracer.record_type(Box::new(FakeType {
        id: 1,
        ..Default::default()
    }));
    tracer.record_type(Box::new(FakeType {
        id: 2,
        ..Default::default()
    }));
    tr.stop_tracing().unwrap();

    let text = fs.read_file("/trace/types_0.json").unwrap();
    assert!(text.starts_with('['));
    assert_eq!(
        text.as_bytes()[1],
        b'{',
        "no newline after '[' so id matches line"
    );
    assert!(text.contains(",\n"));
    assert!(text.ends_with("]\n"));
}

// Go: internal/tracing/tracing.go:maybeFlushLocked (buffer exceeding the threshold is appended)
#[test]
fn flush_threshold_appends() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    // Enough begin/end pairs to push the buffer well past the 256 KiB flush
    // threshold mid-session; no events may be lost or corrupted.
    let n = 3000usize;
    for _ in 0..n {
        let end = tr.push(
            Phase::Parse,
            "createSourceFile",
            Some(path_args("/a.ts")),
            true,
        );
        end();
    }
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    let begins = events.iter().filter(|e| e.ph == "B").count();
    let ends = events.iter().filter(|e| e.ph == "E").count();
    assert_eq!(begins, n);
    assert_eq!(ends, n);
    assert_well_nested(&events);
}

// Go: internal/tracing/tracing.go:StartTracing/StopTracing (empty session)
#[test]
fn empty_session_well_formed_json() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    // Header metadata events only: process_name, thread_name, TracingStartedInBrowser.
    assert_eq!(events.len(), 3);
}

// Go: internal/tracing/tracing.go:StartTracing (metadata events)
#[test]
fn metadata_events_present() {
    let fs = make_trace_fs();
    let tr = start_tracing(&fs, "/trace", "", true).unwrap();
    tr.stop_tracing().unwrap();

    let events = read_events(&fs);
    for e in &events {
        assert_eq!(e.pid, 1);
        assert_eq!(e.tid, 1);
        assert_eq!(e.ph, "M");
    }
    let names: Vec<&str> = events.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"process_name"));
    assert!(names.contains(&"thread_name"));
    assert!(names.contains(&"TracingStartedInBrowser"));
}
