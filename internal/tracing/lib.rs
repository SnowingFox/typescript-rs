//! `tsgo_tracing` — 1:1 Rust port of Go `internal/tracing`.
//!
//! The compiler's observability layer: when `--generateTrace <dir>` is set, the
//! pipeline records Chrome Trace Event entries (`trace.json`), per-checker type
//! snapshots (`types_<n>.json`), and an index (`legend.json`). This crate owns
//! the trace-event writer (begin/end scopes, instant events, well-nested
//! duration events, stable per-thread id assignment) plus the type-descriptor
//! machinery.
//!
//! # Divergences from Go
//!
//! - **nil receiver**: Go calls `tr.Push(...)` on a possibly-nil `*Tracing`.
//!   Here a [`Tracing`] always exists; callers gate on `Option<&Tracing>`
//!   themselves, and every method re-checks `trace_started` so a stopped session
//!   is a no-op.
//! - **`map[string]any` args**: modeled as an ordered [`Args`] map of the
//!   concrete [`ArgValue`] union. Trace events serialize with sorted keys
//!   (a `BTreeMap`), matching Go's `json.Deterministic(true)` output.
//! - **`RecursionIdentity() any`**: Go uses an `any` map key (interface
//!   equality). Rust needs a concrete `Hash + Eq` key, so [`TracedType`] yields a
//!   [`RecursionId`] stand-in. The checker (P4) maps its recursion-identity
//!   object to a stable id; the invariant "same identity → same token" holds.
//! - **`get_location`** and the location-bearing descriptor fields are
//!   DEFERRED until `ast::get_source_file_of_node` and the scanner's
//!   `GetECMALine*` family are ported (see [`TypeDescriptor`]).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant as StdInstant;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use tsgo_ast::{NodeId, Symbol};
use tsgo_vfs::{Fs, FsError};

/// The size at which buffered trace content is flushed to disk via `append_file`.
///
/// Keeps peak memory bounded for long compilations while avoiding a syscall per
/// event.
// Go: internal/tracing/tracing.go:flushThreshold
const FLUSH_THRESHOLD: usize = 256 * 1024;

/// Name of the trace event file written into the trace directory.
// Go: internal/tracing/tracing.go:traceFileName
const TRACE_FILE_NAME: &str = "trace.json";

/// TypeScript's 10ms sampling interval, in microseconds. Sampled events are only
/// recorded if their duration crosses a multiple of this interval.
// Go: internal/tracing/tracing.go:sampleInterval
const SAMPLE_INTERVAL_MICROS: f64 = 10_000.0;

/// The thread id used for the metadata/header events.
// Go: internal/tracing/tracing.go:mainThreadID
const MAIN_THREAD_ID: i32 = 1;

/// First thread id handed out to checkers (checker `i` -> `FIRST_SYNTHETIC_THREAD_ID + i`).
// Go: internal/tracing/tracing.go:firstSyntheticThreadID
const FIRST_SYNTHETIC_THREAD_ID: i32 = 2;

/// Base of the thread-id range reserved for file threads.
// Go: internal/tracing/tracing.go:firstFileThreadID
const FIRST_FILE_THREAD_ID: i32 = 1_000_000;

/// Size of the hash range used to spread file threads above [`FIRST_FILE_THREAD_ID`].
// Go: internal/tracing/tracing.go:fileThreadIDHashRange
const FILE_THREAD_ID_HASH_RANGE: u64 = 1_000_000_000;

/// Args keys, in priority order, whose string value identifies a file thread.
// Go: internal/tracing/tracing.go:traceThreadArgKeys
const TRACE_THREAD_ARG_KEYS: [&str; 5] = [
    "path",
    "fileName",
    "containingFileName",
    "jsFilePath",
    "declarationFilePath",
];

/// A single value carried in a trace event's `args` map.
///
/// DIVERGENCE(port): replaces Go's `any` map value. The compiler only ever
/// stores strings (paths/names), integers (ids/counts), booleans, and — after a
/// JSON round-trip — floating-point numbers, so the union is closed.
///
/// # Examples
/// ```
/// use tsgo_tracing::ArgValue;
/// assert_eq!(ArgValue::Int(1), ArgValue::Int(1));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go:traceEvent.Args (map[string]any value)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ArgValue {
    /// A boolean argument (e.g. `hasResolved`).
    Bool(bool),
    /// An integer argument (e.g. `checkerId`, `id`, `count`).
    Int(i64),
    /// A floating-point argument (JSON numbers decoded from a trace file).
    Float(f64),
    /// A string argument (e.g. `path`, `fileName`, `name`).
    Str(String),
}

/// The `args` payload of a trace event: an ordered map of named [`ArgValue`]s.
///
/// A `BTreeMap` keeps keys sorted so serialized output is deterministic,
/// matching Go's `json.Deterministic(true)`.
// Go: internal/tracing/tracing.go:traceEvent.Args (map[string]any)
pub type Args = BTreeMap<String, ArgValue>;

/// A compilation phase, used as the `cat` (category) of a trace event.
///
/// # Examples
/// ```
/// use tsgo_tracing::Phase;
/// assert_eq!(Phase::CheckTypes.as_str(), "checkTypes");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go:Phase
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Parsing / source-file creation.
    Parse,
    /// Program construction.
    Program,
    /// Binding.
    Bind,
    /// Type checking.
    Check,
    /// Type-level work within checking (variance, relations, ...).
    CheckTypes,
    /// Emit.
    Emit,
    /// Language-service session work.
    Session,
}

impl Phase {
    /// Returns the category string written into trace events.
    ///
    /// # Examples
    /// ```
    /// use tsgo_tracing::Phase;
    /// assert_eq!(Phase::Parse.as_str(), "parse");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/tracing/tracing.go:Phase constants
    pub fn as_str(&self) -> &'static str {
        match self {
            Phase::Parse => "parse",
            Phase::Program => "program",
            Phase::Bind => "bind",
            Phase::Check => "check",
            Phase::CheckTypes => "checkTypes",
            Phase::Emit => "emit",
            Phase::Session => "session",
        }
    }
}

/// A stable, hashable stand-in for a type's recursion identity.
///
/// DIVERGENCE(port): Go uses `RecursionIdentity() any` as a `map[any]int` key,
/// relying on interface equality. Rust needs a concrete `Hash + Eq` key, so the
/// checker (P4) maps its recursion-identity object to a stable `RecursionId`.
/// The only invariant `tracing` relies on is "same identity → same id", which
/// yields "same identity → same recursion token" in the type dump.
///
/// # Examples
/// ```
/// use tsgo_tracing::RecursionId;
/// assert_eq!(RecursionId(7), RecursionId(7));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go:TracedType.RecursionIdentity (any)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecursionId(pub u64);

/// Records types created during checking so they can be dumped to disk.
///
/// Each checker owns its own tracer to keep type ids from colliding across
/// checkers. Implemented here by the per-checker type tracer; the checker calls
/// [`Tracer::record_type`] as it creates types.
///
/// Side effects: implementations buffer types in memory and write a
/// `types_<n>.json` file on [`Tracer::dump_types`].
// Go: internal/tracing/tracing.go:Tracer
pub trait Tracer {
    /// Records a type for later dumping.
    ///
    /// Side effects: appends the type to the tracer's in-memory buffer.
    // Go: internal/tracing/tracing.go:Tracer.RecordType
    fn record_type(&self, t: Box<dyn TracedType + Send + Sync>);

    /// Writes all recorded types to disk as `types_<n>.json`.
    ///
    /// Side effects: writes the per-checker types file (nothing if no types were
    /// recorded).
    // Go: internal/tracing/tracing.go:Tracer.DumpTypes
    fn dump_types(&self) -> Result<(), TraceError>;
}

/// A type the checker exposes to `tracing` without a circular dependency.
///
/// The checker's `Type` implements this; `tracing` only reads through it. Sub-type
/// accessors return borrows so [`build_type_descriptor`](self) can collect ids
/// without owning the type graph.
///
/// DIVERGENCE(port): node-valued accessors ([`TracedType::reference_node`],
/// [`TracedType::pattern`]) return [`tsgo_ast::NodeId`] (the arena handle that
/// replaces Go's `*ast.Node`). Their locations are not yet resolvable (see
/// [`TypeDescriptor`]).
///
/// Side effects: none (pure accessors).
// Go: internal/tracing/tracing.go:TracedType
pub trait TracedType {
    /// The type's unique id.
    // Go: internal/tracing/tracing.go:TracedType.Id
    fn id(&self) -> u32;
    /// Human-readable type flag names.
    // Go: internal/tracing/tracing.go:TracedType.FormatFlags
    fn format_flags(&self) -> Vec<String>;
    /// Whether this is a conditional type.
    // Go: internal/tracing/tracing.go:TracedType.IsConditional
    fn is_conditional(&self) -> bool;
    /// The type's symbol, if any.
    // Go: internal/tracing/tracing.go:TracedType.Symbol
    fn symbol(&self) -> Option<&Symbol>;
    /// The type's alias symbol, if any.
    // Go: internal/tracing/tracing.go:TracedType.AliasSymbol
    fn alias_symbol(&self) -> Option<&Symbol>;
    /// The alias type arguments.
    // Go: internal/tracing/tracing.go:TracedType.AliasTypeArguments
    fn alias_type_arguments(&self) -> Vec<&dyn TracedType>;
    /// The intrinsic name (e.g. `string`, `number`), or empty.
    // Go: internal/tracing/tracing.go:TracedType.IntrinsicName
    fn intrinsic_name(&self) -> String;
    /// The constituents of a union type.
    // Go: internal/tracing/tracing.go:TracedType.UnionTypes
    fn union_types(&self) -> Vec<&dyn TracedType>;
    /// The constituents of an intersection type.
    // Go: internal/tracing/tracing.go:TracedType.IntersectionTypes
    fn intersection_types(&self) -> Vec<&dyn TracedType>;
    /// The operand of a `keyof` index type.
    // Go: internal/tracing/tracing.go:TracedType.IndexType
    fn index_type(&self) -> Option<&dyn TracedType>;
    /// The object type of an indexed access type.
    // Go: internal/tracing/tracing.go:TracedType.IndexedAccessObjectType
    fn indexed_access_object_type(&self) -> Option<&dyn TracedType>;
    /// The index type of an indexed access type.
    // Go: internal/tracing/tracing.go:TracedType.IndexedAccessIndexType
    fn indexed_access_index_type(&self) -> Option<&dyn TracedType>;
    /// The check type of a conditional type.
    // Go: internal/tracing/tracing.go:TracedType.ConditionalCheckType
    fn conditional_check_type(&self) -> Option<&dyn TracedType>;
    /// The extends type of a conditional type.
    // Go: internal/tracing/tracing.go:TracedType.ConditionalExtendsType
    fn conditional_extends_type(&self) -> Option<&dyn TracedType>;
    /// The resolved true branch of a conditional type, if resolved.
    // Go: internal/tracing/tracing.go:TracedType.ConditionalTrueType
    fn conditional_true_type(&self) -> Option<&dyn TracedType>;
    /// The resolved false branch of a conditional type, if resolved.
    // Go: internal/tracing/tracing.go:TracedType.ConditionalFalseType
    fn conditional_false_type(&self) -> Option<&dyn TracedType>;
    /// The base type of a substitution type.
    // Go: internal/tracing/tracing.go:TracedType.SubstitutionBaseType
    fn substitution_base_type(&self) -> Option<&dyn TracedType>;
    /// The constraint of a substitution type.
    // Go: internal/tracing/tracing.go:TracedType.SubstitutionConstraintType
    fn substitution_constraint_type(&self) -> Option<&dyn TracedType>;
    /// The target of a type reference.
    // Go: internal/tracing/tracing.go:TracedType.ReferenceTarget
    fn reference_target(&self) -> Option<&dyn TracedType>;
    /// The type arguments of a type reference.
    // Go: internal/tracing/tracing.go:TracedType.ReferenceTypeArguments
    fn reference_type_arguments(&self) -> Vec<&dyn TracedType>;
    /// The node a type reference originates from, if any.
    // Go: internal/tracing/tracing.go:TracedType.ReferenceNode
    fn reference_node(&self) -> Option<NodeId>;
    /// The source type of a reverse-mapped type.
    // Go: internal/tracing/tracing.go:TracedType.ReverseMappedSourceType
    fn reverse_mapped_source_type(&self) -> Option<&dyn TracedType>;
    /// The mapped type of a reverse-mapped type.
    // Go: internal/tracing/tracing.go:TracedType.ReverseMappedMappedType
    fn reverse_mapped_mapped_type(&self) -> Option<&dyn TracedType>;
    /// The constraint type of a reverse-mapped type.
    // Go: internal/tracing/tracing.go:TracedType.ReverseMappedConstraintType
    fn reverse_mapped_constraint_type(&self) -> Option<&dyn TracedType>;
    /// The element type of an evolving array type.
    // Go: internal/tracing/tracing.go:TracedType.EvolvingArrayElementType
    fn evolving_array_element_type(&self) -> Option<&dyn TracedType>;
    /// The final type of an evolving array type.
    // Go: internal/tracing/tracing.go:TracedType.EvolvingArrayFinalType
    fn evolving_array_final_type(&self) -> Option<&dyn TracedType>;
    /// Whether this is a tuple type.
    // Go: internal/tracing/tracing.go:TracedType.IsTuple
    fn is_tuple(&self) -> bool;
    /// The destructuring pattern node, if any.
    // Go: internal/tracing/tracing.go:TracedType.Pattern
    fn pattern(&self) -> Option<NodeId>;
    /// A stable id for the type's recursion identity, if any.
    // Go: internal/tracing/tracing.go:TracedType.RecursionIdentity
    fn recursion_identity(&self) -> Option<RecursionId>;
    /// An optional string representation of the type.
    // Go: internal/tracing/tracing.go:TracedType.Display
    fn display(&self) -> String;
}

/// Metadata about one trace file, written into `legend.json`.
///
/// # Examples
/// ```
/// use tsgo_tracing::TraceRecord;
/// let r = TraceRecord::default();
/// assert_eq!(r.checker_id, 0);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go:TraceRecord
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct TraceRecord {
    /// Path of the tsconfig that produced this trace, if any.
    #[serde(
        rename = "configFilePath",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub config_file_path: String,
    /// Path of the `trace.json` file.
    #[serde(
        rename = "tracePath",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub trace_path: String,
    /// Path of the `types_<n>.json` file.
    #[serde(
        rename = "typesPath",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub types_path: String,
    /// The checker index that owns the types file.
    #[serde(rename = "checkerId")]
    pub checker_id: i32,
}

/// A single Chrome Trace Event entry.
///
/// Crate-private, mirroring Go's unexported `traceEvent`.
// Go: internal/tracing/tracing.go:traceEvent
#[derive(Serialize, Deserialize, Clone, Debug)]
struct TraceEvent {
    pid: i32,
    tid: i32,
    ph: String,
    cat: String,
    ts: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    // Scope; only set for instant events ("g" = global).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    s: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dur: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    args: Option<Args>,
}

/// An error produced while starting, writing, or finalizing a trace session.
///
/// # Examples
/// ```
/// use tsgo_tracing::TraceError;
/// let e = TraceError::from(tsgo_vfs::FsError::NotExist);
/// assert!(e.to_string().contains("does not exist"));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go (wrapped error returns)
#[derive(Debug)]
pub enum TraceError {
    /// A file-system operation failed.
    Fs(FsError),
    /// A trace event or descriptor could not be serialized to JSON.
    Marshal(String),
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceError::Fs(e) => write!(f, "trace file system error: {e}"),
            TraceError::Marshal(msg) => write!(f, "failed to marshal trace data: {msg}"),
        }
    }
}

impl std::error::Error for TraceError {}

impl From<FsError> for TraceError {
    fn from(e: FsError) -> Self {
        TraceError::Fs(e)
    }
}

/// A 1-indexed line/character position in a source file.
///
/// # Examples
/// ```
/// use tsgo_tracing::LineAndChar;
/// let lc = LineAndChar { line: 1, character: 1 };
/// assert_eq!(lc.line, 1);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go:LineAndChar
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LineAndChar {
    /// 1-indexed line number.
    pub line: i32,
    /// 1-indexed UTF-16 character offset within the line.
    pub character: i32,
}

/// A source-code location: a file path plus optional start/end positions.
///
/// # Examples
/// ```
/// use tsgo_tracing::Location;
/// let loc = Location::default();
/// assert!(loc.path.is_empty());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go:Location
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Location {
    /// The (tspath-normalized) file path.
    pub path: String,
    /// The start position, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<LineAndChar>,
    /// The end position, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<LineAndChar>,
}

/// One type's entry in a `types_<n>.json` file.
///
/// The field order matches Go so the output is comparable. Cross-type
/// references are stored as type ids.
///
/// DEFER(phase-10): the location-bearing fields ([`TypeDescriptor::reference_location`],
/// [`TypeDescriptor::destructuring_pattern`], [`TypeDescriptor::first_declaration`])
/// are always `None` until source locations can be resolved
/// (see [`build_type_descriptor`](self)).
///
/// # Examples
/// ```
/// use tsgo_tracing::TypeDescriptor;
/// let d = TypeDescriptor { id: 1, ..Default::default() };
/// assert_eq!(d.id, 1);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/tracing/tracing.go:TypeDescriptor
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TypeDescriptor {
    /// The type id.
    pub id: u32,
    /// The intrinsic name (e.g. `string`), or empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub intrinsic_name: String,
    /// The (escaped) symbol name, or empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub symbol_name: String,
    /// A stable token shared by types with the same recursion identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recursion_id: Option<i64>,
    /// Whether the type is a tuple.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_tuple: bool,
    /// Constituent ids of a union type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub union_types: Option<Vec<u32>>,
    /// Constituent ids of an intersection type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intersection_types: Option<Vec<u32>>,
    /// Alias type-argument ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_type_arguments: Option<Vec<u32>>,
    /// The id of a `keyof` operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyof_type: Option<u32>,
    /// The object-type id of an indexed access type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_access_object_type: Option<u32>,
    /// The index-type id of an indexed access type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_access_index_type: Option<u32>,
    /// The check-type id of a conditional type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditional_check_type: Option<u32>,
    /// The extends-type id of a conditional type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditional_extends_type: Option<u32>,
    /// The true-branch id of a conditional type; `-1` if unresolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditional_true_type: Option<i32>,
    /// The false-branch id of a conditional type; `-1` if unresolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditional_false_type: Option<i32>,
    /// The base-type id of a substitution type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substitution_base_type: Option<u32>,
    /// The constraint-type id of a substitution type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint_type: Option<u32>,
    /// The target id of a type reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instantiated_type: Option<u32>,
    /// The type-argument ids of a type reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_arguments: Option<Vec<u32>>,
    /// The source location of a type reference (DEFERRED; always `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_location: Option<Location>,
    /// The source-type id of a reverse-mapped type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_mapped_source_type: Option<u32>,
    /// The mapped-type id of a reverse-mapped type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_mapped_mapped_type: Option<u32>,
    /// The constraint-type id of a reverse-mapped type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_mapped_constraint_type: Option<u32>,
    /// The element-type id of an evolving array type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evolving_array_element_type: Option<u32>,
    /// The final-type id of an evolving array type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evolving_array_final_type: Option<u32>,
    /// The destructuring-pattern location (DEFERRED; always `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructuring_pattern: Option<Location>,
    /// The first declaration's location (DEFERRED; always `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_declaration: Option<Location>,
    /// The type's flag names (always serialized, possibly empty).
    pub flags: Vec<String>,
    /// An optional textual representation of the type.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display: String,
}

// serde `skip_serializing_if` predicate for `bool` fields with Go `omitzero`.
fn is_false(b: &bool) -> bool {
    !*b
}

// Collects type ids from a slice of traced types.
// Go: internal/tracing/tracing.go:mapTypeIds
fn map_type_ids(types: &[&dyn TracedType]) -> Vec<u32> {
    types.iter().map(|t| t.id()).collect()
}

// Resolves a node's source location for a type descriptor.
//
// DEFER(phase-10): byte-accurate locations need `ast::get_source_file_of_node`
// plus the scanner's `GetTokenPosOfNode` / `GetECMALineAndUTF16CharacterOfPosition`,
// none of which is ported yet, so this returns `None` (Go also returns nil when
// the node has no source file). The descriptor wiring is in place so only this
// body changes once the dependencies land.
// blocked-by: tsgo_ast has no get_source_file_of_node; tsgo_scanner defers the
// GetECMALine* / GetTokenPosOfNode helpers (see scanner lib.rs module docs).
// Go: internal/tracing/tracing.go:getLocation
fn get_location(_node: NodeId) -> Option<Location> {
    None
}

/// Builds a [`TypeDescriptor`] for a traced type, assigning recursion tokens.
///
/// Each distinct recursion identity gets a unique integer token (its first-seen
/// index in `recursion_identity_map`), matching TypeScript so trace tools can
/// group types that share a recursion identity.
///
/// # Examples
/// ```
/// use rustc_hash::FxHashMap;
/// use tsgo_tracing::{build_type_descriptor, RecursionId, TracedType};
/// # struct T;
/// # impl TracedType for T {
/// #   fn id(&self) -> u32 { 1 }
/// #   fn format_flags(&self) -> Vec<String> { Vec::new() }
/// #   fn is_conditional(&self) -> bool { false }
/// #   fn symbol(&self) -> Option<&tsgo_ast::Symbol> { None }
/// #   fn alias_symbol(&self) -> Option<&tsgo_ast::Symbol> { None }
/// #   fn alias_type_arguments(&self) -> Vec<&dyn TracedType> { Vec::new() }
/// #   fn intrinsic_name(&self) -> String { "number".into() }
/// #   fn union_types(&self) -> Vec<&dyn TracedType> { Vec::new() }
/// #   fn intersection_types(&self) -> Vec<&dyn TracedType> { Vec::new() }
/// #   fn index_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn indexed_access_object_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn indexed_access_index_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn conditional_check_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn conditional_extends_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn conditional_true_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn conditional_false_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn substitution_base_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn substitution_constraint_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn reference_target(&self) -> Option<&dyn TracedType> { None }
/// #   fn reference_type_arguments(&self) -> Vec<&dyn TracedType> { Vec::new() }
/// #   fn reference_node(&self) -> Option<tsgo_ast::NodeId> { None }
/// #   fn reverse_mapped_source_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn reverse_mapped_mapped_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn reverse_mapped_constraint_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn evolving_array_element_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn evolving_array_final_type(&self) -> Option<&dyn TracedType> { None }
/// #   fn is_tuple(&self) -> bool { false }
/// #   fn pattern(&self) -> Option<tsgo_ast::NodeId> { None }
/// #   fn recursion_identity(&self) -> Option<RecursionId> { None }
/// #   fn display(&self) -> String { String::new() }
/// # }
/// let mut map = FxHashMap::default();
/// let d = build_type_descriptor(&T, &mut map);
/// assert_eq!(d.intrinsic_name, "number");
/// ```
///
/// Side effects: inserts newly seen recursion identities into
/// `recursion_identity_map`.
// Go: internal/tracing/tracing.go:buildTypeDescriptor
pub fn build_type_descriptor(
    typ: &dyn TracedType,
    recursion_identity_map: &mut FxHashMap<RecursionId, usize>,
) -> TypeDescriptor {
    let symbol = typ.symbol();
    let alias_symbol = typ.alias_symbol();

    let mut desc = TypeDescriptor {
        id: typ.id(),
        flags: typ.format_flags(),
        ..Default::default()
    };

    // Assign a unique token per recursion identity (first-seen index).
    if let Some(identity) = typ.recursion_identity() {
        let next = recursion_identity_map.len();
        let token = *recursion_identity_map.entry(identity).or_insert(next);
        desc.recursion_id = Some(token as i64);
    }

    let intrinsic = typ.intrinsic_name();
    if !intrinsic.is_empty() {
        desc.intrinsic_name = intrinsic;
    }

    // Symbol name: prefer the alias symbol, escaping the internal-name prefix.
    if let Some(sym) = alias_symbol.or(symbol) {
        desc.symbol_name = tsgo_ast::symbol::escape_all_internal_symbol_names(&sym.name);
    }

    if typ.is_tuple() {
        desc.is_tuple = true;
    }

    let unions = typ.union_types();
    if !unions.is_empty() {
        desc.union_types = Some(map_type_ids(&unions));
    }
    let intersections = typ.intersection_types();
    if !intersections.is_empty() {
        desc.intersection_types = Some(map_type_ids(&intersections));
    }
    let alias_args = typ.alias_type_arguments();
    if !alias_args.is_empty() {
        desc.alias_type_arguments = Some(map_type_ids(&alias_args));
    }

    if let Some(index) = typ.index_type() {
        desc.keyof_type = Some(index.id());
    }
    if let Some(obj) = typ.indexed_access_object_type() {
        desc.indexed_access_object_type = Some(obj.id());
    }
    if let Some(idx) = typ.indexed_access_index_type() {
        desc.indexed_access_index_type = Some(idx.id());
    }

    if typ.is_conditional() {
        if let Some(check) = typ.conditional_check_type() {
            desc.conditional_check_type = Some(check.id());
        }
        if let Some(extends) = typ.conditional_extends_type() {
            desc.conditional_extends_type = Some(extends.id());
        }
        // Unresolved branches serialize as -1, matching TypeScript.
        desc.conditional_true_type =
            Some(typ.conditional_true_type().map_or(-1, |t| t.id() as i32));
        desc.conditional_false_type =
            Some(typ.conditional_false_type().map_or(-1, |t| t.id() as i32));
    }

    if let Some(base) = typ.substitution_base_type() {
        desc.substitution_base_type = Some(base.id());
    }
    if let Some(constraint) = typ.substitution_constraint_type() {
        desc.constraint_type = Some(constraint.id());
    }

    if let Some(target) = typ.reference_target() {
        desc.instantiated_type = Some(target.id());
    }
    let ref_args = typ.reference_type_arguments();
    if !ref_args.is_empty() {
        desc.type_arguments = Some(map_type_ids(&ref_args));
    }
    desc.reference_location = typ.reference_node().and_then(get_location);

    if let Some(source) = typ.reverse_mapped_source_type() {
        desc.reverse_mapped_source_type = Some(source.id());
    }
    if let Some(mapped) = typ.reverse_mapped_mapped_type() {
        desc.reverse_mapped_mapped_type = Some(mapped.id());
    }
    if let Some(constraint) = typ.reverse_mapped_constraint_type() {
        desc.reverse_mapped_constraint_type = Some(constraint.id());
    }

    if let Some(elem) = typ.evolving_array_element_type() {
        desc.evolving_array_element_type = Some(elem.id());
    }
    if let Some(final_type) = typ.evolving_array_final_type() {
        desc.evolving_array_final_type = Some(final_type.id());
    }

    desc.destructuring_pattern = typ.pattern().and_then(get_location);

    // First declaration: prefer the alias symbol (matching `aliasSymbol ?? symbol`).
    if let Some(sym) = alias_symbol.or(symbol) {
        if let Some(&decl) = sym.declarations.first() {
            desc.first_declaration = get_location(decl);
        }
    }

    let display = typ.display();
    if !display.is_empty() {
        desc.display = display;
    }

    desc
}

/// Mutable session state guarded by [`Tracing`]'s single mutex.
// Go: internal/tracing/tracing.go:Tracing (mutex-guarded fields)
struct TraceState<'fs> {
    // Buffered, not-yet-flushed trace content.
    content: String,
    // Monotonic counter used for timestamps in deterministic mode.
    timestamp_counter: u64,
    // First flush error; once set, further flushes are no-ops and the error is
    // surfaced from `stop_tracing`.
    flush_err: Option<FsError>,
    // Index entries, one per type tracer.
    legend: Vec<TraceRecord>,
    // Per-checker type tracers, in creation order.
    tracers: Vec<Arc<TypeTracer<'fs>>>,
    // Thread-key -> assigned thread id.
    thread_ids: HashMap<TraceThreadKey, i32>,
    // Assigned thread id -> thread-key (for linear-probe collision resolution).
    thread_keys: HashMap<i32, TraceThreadKey>,
}

impl TraceState<'_> {
    // Returns the next timestamp in microseconds: a monotonic counter in
    // deterministic mode, otherwise real elapsed wall-clock since `start_time`.
    // Go: internal/tracing/tracing.go:timestamp
    fn timestamp(&mut self, deterministic: bool, start_time: StdInstant) -> f64 {
        if deterministic {
            self.timestamp_counter += 1;
            self.timestamp_counter as f64
        } else {
            start_time.elapsed().as_nanos() as f64 / 1000.0
        }
    }

    // Appends the buffered content to disk once it grows past the flush
    // threshold. If a previous flush failed, drops the buffer and stays a no-op;
    // a new failure is recorded and surfaced from `stop_tracing`.
    // Go: internal/tracing/tracing.go:maybeFlushLocked
    fn maybe_flush_locked(&mut self, fs: &dyn Fs, trace_path: &str) {
        if self.flush_err.is_some() {
            self.content.clear();
            return;
        }
        if self.content.len() < FLUSH_THRESHOLD {
            return;
        }
        if let Err(e) = fs.append_file(trace_path, &self.content) {
            self.flush_err = Some(e);
        }
        self.content.clear();
    }

    // Resolves the thread id for an event's args, assigning a new one (and
    // emitting a `thread_name` metadata event) on first sight. Linear-probes on
    // collision so distinct keys never share an id.
    // Go: internal/tracing/tracing.go:threadIDLocked
    fn thread_id_locked(&mut self, args: Option<&Args>, metadata_ts: f64) -> i32 {
        let Some(key) = args.and_then(trace_thread_key_from_args) else {
            return MAIN_THREAD_ID;
        };
        if let Some(&tid) = self.thread_ids.get(&key) {
            return tid;
        }
        let mut tid = key.default_thread_id();
        loop {
            match self.thread_keys.get(&tid) {
                Some(existing) if *existing != key => tid += 1,
                _ => break,
            }
        }
        self.thread_ids.insert(key.clone(), tid);
        self.thread_keys.insert(tid, key.clone());
        self.write_thread_name_event_locked(tid, &key.display_name(), metadata_ts);
        tid
    }

    // Writes a `thread_name` metadata event naming a newly assigned thread.
    // Go: internal/tracing/tracing.go:writeThreadNameEventLocked
    fn write_thread_name_event_locked(&mut self, tid: i32, name: &str, metadata_ts: f64) {
        self.content.push_str(",\n");
        self.write_event(&TraceEvent {
            pid: 1,
            tid,
            ph: "M".to_string(),
            cat: "__metadata".to_string(),
            ts: metadata_ts,
            name: "thread_name".to_string(),
            s: String::new(),
            dur: None,
            args: Some(name_args(name)),
        });
    }

    // Appends one event to the content buffer.
    // Go: internal/tracing/tracing.go:writeEvent/writeEventTo
    fn write_event(&mut self, event: &TraceEvent) {
        let bytes = match tsgo_json::marshal(event) {
            Ok(b) => b,
            Err(e) => panic!("failed to marshal trace event: {e}"),
        };
        // Serialized JSON is always valid UTF-8.
        self.content
            .push_str(std::str::from_utf8(&bytes).expect("serialized JSON is valid UTF-8"));
    }
}

/// Manages a tracing session: the shared trace buffer, per-thread id table, and
/// the per-checker type tracers.
///
/// Methods take `&self` and use interior mutability so a single session can be
/// shared across the compiler's worker threads.
///
/// # Examples
/// ```
/// use tsgo_tracing::start_tracing;
/// use tsgo_vfs::vfstest::MapFs;
/// let fs = MapFs::from_map([("/t/.keep", "")], true);
/// let tr = start_tracing(&fs, "/t", "", true).unwrap();
/// tr.stop_tracing().unwrap();
/// ```
///
/// Side effects: writes `trace.json` and `legend.json` under the trace
/// directory; reads nothing.
// Go: internal/tracing/tracing.go:Tracing
pub struct Tracing<'fs> {
    fs: &'fs (dyn Fs + Send + Sync),
    trace_dir: String,
    trace_path: String,
    config_file_path: String,
    metadata_ts: f64,
    deterministic: bool,
    start_time: StdInstant,
    trace_started: AtomicBool,
    state: Mutex<TraceState<'fs>>,
}

/// Creates a new tracing session and writes the `trace.json` header.
///
/// When `deterministic` is true, timestamps use a monotonic counter instead of
/// wall-clock time, producing stable output for test baselines.
///
/// # Examples
/// ```
/// use tsgo_tracing::start_tracing;
/// use tsgo_vfs::vfstest::MapFs;
/// let fs = MapFs::from_map([("/t/.keep", "")], true);
/// let tr = start_tracing(&fs, "/t", "", true).unwrap();
/// tr.stop_tracing().unwrap();
/// ```
///
/// Side effects: truncates and writes `<trace_dir>/trace.json` with the JSON
/// array opener and three metadata events.
// Go: internal/tracing/tracing.go:StartTracing
pub fn start_tracing<'fs>(
    fs: &'fs (dyn Fs + Send + Sync),
    trace_dir: &str,
    config_file_path: &str,
    deterministic: bool,
) -> Result<Tracing<'fs>, TraceError> {
    let trace_path = tsgo_tspath::combine_paths(trace_dir, &[TRACE_FILE_NAME]);
    let mut state = TraceState {
        content: String::new(),
        timestamp_counter: 0,
        flush_err: None,
        legend: Vec::new(),
        tracers: Vec::new(),
        thread_ids: HashMap::new(),
        thread_keys: HashMap::new(),
    };
    let start_time = StdInstant::now();

    // Trace file header: open the JSON array, then the metadata events.
    state.content.push_str("[\n");
    let meta_ts = state.timestamp(deterministic, start_time);
    state.write_event(&TraceEvent {
        pid: 1,
        tid: MAIN_THREAD_ID,
        ph: "M".to_string(),
        cat: "__metadata".to_string(),
        ts: meta_ts,
        name: "process_name".to_string(),
        s: String::new(),
        dur: None,
        args: Some(name_args("tsgo")),
    });
    state.content.push_str(",\n");
    state.write_event(&TraceEvent {
        pid: 1,
        tid: MAIN_THREAD_ID,
        ph: "M".to_string(),
        cat: "__metadata".to_string(),
        ts: meta_ts,
        name: "thread_name".to_string(),
        s: String::new(),
        dur: None,
        args: Some(name_args("Main")),
    });
    state.content.push_str(",\n");
    state.write_event(&TraceEvent {
        pid: 1,
        tid: MAIN_THREAD_ID,
        ph: "M".to_string(),
        cat: "disabled-by-default-devtools.timeline".to_string(),
        ts: meta_ts,
        name: "TracingStartedInBrowser".to_string(),
        s: String::new(),
        dur: None,
        args: None,
    });

    // Truncate any existing trace file with the header so later appends extend
    // a clean file.
    fs.write_file(&trace_path, &state.content)?;
    state.content.clear();

    Ok(Tracing {
        fs,
        trace_dir: trace_dir.to_string(),
        trace_path,
        config_file_path: config_file_path.to_string(),
        metadata_ts: meta_ts,
        deterministic,
        start_time,
        trace_started: AtomicBool::new(true),
        state: Mutex::new(state),
    })
}

// Builds a one-entry `{ "name": <value> }` args map.
fn name_args(value: &str) -> Args {
    let mut m = Args::new();
    m.insert("name".to_string(), ArgValue::Str(value.to_string()));
    m
}

/// The kind of thread a trace event belongs to.
// Go: internal/tracing/tracing.go:traceThreadKind
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TraceThreadKind {
    Checker,
    File,
}

impl TraceThreadKind {
    fn as_str(self) -> &'static str {
        match self {
            TraceThreadKind::Checker => "checker",
            TraceThreadKind::File => "file",
        }
    }
}

/// A stable identity for a trace thread, derived from an event's args.
// Go: internal/tracing/tracing.go:traceThreadKey
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TraceThreadKey {
    kind: TraceThreadKind,
    text: String,
    index: i32,
    has_index: bool,
}

impl TraceThreadKey {
    // The id this key prefers before collision probing: synthetic ids for
    // checkers, a hashed file-range id otherwise.
    // Go: internal/tracing/tracing.go:traceThreadKey.defaultThreadID
    fn default_thread_id(&self) -> i32 {
        if self.kind == TraceThreadKind::Checker && self.has_index && self.index >= 0 {
            FIRST_SYNTHETIC_THREAD_ID + self.index
        } else {
            stable_trace_thread_id(self)
        }
    }

    // The human-readable thread name, e.g. `checker:0` or `file:/a.ts`.
    // Go: internal/tracing/tracing.go:traceThreadKey.displayName
    fn display_name(&self) -> String {
        if self.has_index {
            format!("{}:{}", self.kind.as_str(), self.index)
        } else {
            format!("{}:{}", self.kind.as_str(), self.text)
        }
    }
}

// Extracts a thread key from an event's args: a checker id wins, otherwise the
// first non-empty path-like string in priority order.
// Go: internal/tracing/tracing.go:traceThreadKeyFromArgs
fn trace_thread_key_from_args(args: &Args) -> Option<TraceThreadKey> {
    if args.is_empty() {
        return None;
    }
    if let Some(ArgValue::Int(checker_id)) = args.get("checkerId") {
        return Some(TraceThreadKey {
            kind: TraceThreadKind::Checker,
            text: String::new(),
            index: *checker_id as i32,
            has_index: true,
        });
    }
    for key in TRACE_THREAD_ARG_KEYS {
        if let Some(ArgValue::Str(path)) = args.get(key) {
            if !path.is_empty() {
                return Some(TraceThreadKey {
                    kind: TraceThreadKind::File,
                    text: path.clone(),
                    index: 0,
                    has_index: false,
                });
            }
        }
    }
    None
}

// Maps a thread key into the file-thread range via an XXH3 hash of
// `<kind>:<index|text>`, matching Go's `xxh3.New().Sum64()` (canonical XXH3,
// seed 0). The absolute value is byte-for-byte parity work deferred to P10; the
// gates here only rely on it being stable and collision-free for distinct keys.
// Go: internal/tracing/tracing.go:stableTraceThreadID
fn stable_trace_thread_id(key: &TraceThreadKey) -> i32 {
    let mut buf = Vec::with_capacity(key.text.len() + 16);
    buf.extend_from_slice(key.kind.as_str().as_bytes());
    buf.push(b':');
    if key.has_index {
        buf.extend_from_slice(key.index.to_string().as_bytes());
    } else {
        buf.extend_from_slice(key.text.as_bytes());
    }
    let sum = xxh3::hash64_with_seed(&buf, 0);
    FIRST_FILE_THREAD_ID + (sum % FILE_THREAD_ID_HASH_RANGE) as i32
}

impl Tracing<'_> {
    /// Starts a trace event block on the shared trace buffer and returns a
    /// closure that ends it.
    ///
    /// When `separate_begin_and_end` is true a `B` (begin) event is written
    /// immediately and the returned closure writes the matching `E` (end) event
    /// on the same thread; each closure is self-contained, so begin/end may run
    /// on different threads. When false the event is sampled (only recorded if
    /// its duration crosses a 10ms boundary), and in deterministic mode it is
    /// skipped entirely to avoid flaky baselines.
    ///
    /// # Examples
    /// ```
    /// use tsgo_tracing::{start_tracing, Phase};
    /// use tsgo_vfs::vfstest::MapFs;
    /// let fs = MapFs::from_map([("/t/.keep", "")], true);
    /// let tr = start_tracing(&fs, "/t", "", true).unwrap();
    /// let end = tr.push(Phase::Parse, "createSourceFile", None, true);
    /// end();
    /// tr.stop_tracing().unwrap();
    /// ```
    ///
    /// Side effects: appends a begin event (and, via the returned closure, an
    /// end event) to the trace buffer, possibly flushing to disk.
    // Go: internal/tracing/tracing.go:Push
    pub fn push(
        &self,
        phase: Phase,
        name: &str,
        args: Option<Args>,
        separate_begin_and_end: bool,
    ) -> Box<dyn FnOnce() + '_> {
        if !self.trace_started.load(Ordering::SeqCst) {
            return Box::new(|| {});
        }

        if separate_begin_and_end {
            let cat = phase.as_str().to_string();
            let tid;
            {
                let mut state = self.state.lock().expect("trace state poisoned");
                if !self.trace_started.load(Ordering::SeqCst) {
                    return Box::new(|| {});
                }
                let ts = state.timestamp(self.deterministic, self.start_time);
                tid = state.thread_id_locked(args.as_ref(), self.metadata_ts);
                state.content.push_str(",\n");
                state.write_event(&TraceEvent {
                    pid: 1,
                    tid,
                    ph: "B".to_string(),
                    cat: cat.clone(),
                    ts,
                    name: name.to_string(),
                    s: String::new(),
                    dur: None,
                    args: args.clone(),
                });
                state.maybe_flush_locked(self.fs, &self.trace_path);
            }
            let name = name.to_string();
            return Box::new(move || {
                let mut state = self.state.lock().expect("trace state poisoned");
                if !self.trace_started.load(Ordering::SeqCst) {
                    return;
                }
                let end_ts = state.timestamp(self.deterministic, self.start_time);
                state.content.push_str(",\n");
                state.write_event(&TraceEvent {
                    pid: 1,
                    tid,
                    ph: "E".to_string(),
                    cat,
                    ts: end_ts,
                    name,
                    s: String::new(),
                    dur: None,
                    args,
                });
                state.maybe_flush_locked(self.fs, &self.trace_path);
            });
        }

        // Sampled event (separate_begin_and_end=false). In deterministic mode
        // these are skipped entirely so baselines stay stable; otherwise the
        // event is only recorded if its duration crosses a sampling boundary.
        //
        // NOTE(port): the non-deterministic timing path cannot be unit-tested
        // deterministically; its byte-level behavior is covered by P10 parity.
        if self.deterministic {
            return Box::new(|| {});
        }
        let cat = phase.as_str().to_string();
        let name = name.to_string();
        let sample_start = StdInstant::now();
        Box::new(move || {
            let dur = sample_start.elapsed().as_nanos() as f64 / 1000.0;
            let start_micros = sample_start
                .saturating_duration_since(self.start_time)
                .as_nanos() as f64
                / 1000.0;
            if SAMPLE_INTERVAL_MICROS - (start_micros % SAMPLE_INTERVAL_MICROS) > dur {
                return;
            }
            let mut state = self.state.lock().expect("trace state poisoned");
            if !self.trace_started.load(Ordering::SeqCst) {
                return;
            }
            let tid = state.thread_id_locked(args.as_ref(), self.metadata_ts);
            state.content.push_str(",\n");
            state.write_event(&TraceEvent {
                pid: 1,
                tid,
                ph: "X".to_string(),
                cat,
                ts: start_micros,
                name,
                s: String::new(),
                dur: Some(dur),
                args,
            });
            state.maybe_flush_locked(self.fs, &self.trace_path);
        })
    }

    /// Records an instant ("I") event with global scope.
    ///
    /// # Examples
    /// ```
    /// use tsgo_tracing::{start_tracing, Phase};
    /// use tsgo_vfs::vfstest::MapFs;
    /// let fs = MapFs::from_map([("/t/.keep", "")], true);
    /// let tr = start_tracing(&fs, "/t", "", true).unwrap();
    /// tr.instant(Phase::Program, "createProgram", None);
    /// tr.stop_tracing().unwrap();
    /// ```
    ///
    /// Side effects: appends one instant event to the trace buffer, possibly
    /// flushing to disk.
    // Go: internal/tracing/tracing.go:Instant
    pub fn instant(&self, phase: Phase, name: &str, args: Option<Args>) {
        if !self.trace_started.load(Ordering::SeqCst) {
            return;
        }
        let mut state = self.state.lock().expect("trace state poisoned");
        // Re-check under the lock: `stop_tracing` may have run in between.
        if !self.trace_started.load(Ordering::SeqCst) {
            return;
        }
        let ts = state.timestamp(self.deterministic, self.start_time);
        let tid = state.thread_id_locked(args.as_ref(), self.metadata_ts);
        state.content.push_str(",\n");
        state.write_event(&TraceEvent {
            pid: 1,
            tid,
            ph: "I".to_string(),
            cat: phase.as_str().to_string(),
            ts,
            name: name.to_string(),
            s: "g".to_string(),
            dur: None,
            args,
        });
        state.maybe_flush_locked(self.fs, &self.trace_path);
    }

    /// Creates a per-checker type tracer and registers it in the legend.
    ///
    /// `checker_index` is used to name the tracer's output file
    /// (`types_<checker_index>.json`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_tracing::start_tracing;
    /// use tsgo_vfs::vfstest::MapFs;
    /// let fs = MapFs::from_map([("/t/.keep", "")], true);
    /// let tr = start_tracing(&fs, "/t", "", true).unwrap();
    /// let _tracer = tr.new_type_tracer(0);
    /// tr.stop_tracing().unwrap();
    /// ```
    ///
    /// Side effects: appends a legend entry; the returned tracer is shared with
    /// this session so it is dumped on [`Tracing::stop_tracing`].
    // Go: internal/tracing/tracing.go:NewTypeTracer
    pub fn new_type_tracer(&self, checker_index: i32) -> Arc<dyn Tracer + Send + Sync + '_> {
        let mut state = self.state.lock().expect("trace state poisoned");
        let types_path =
            tsgo_tspath::combine_paths(&self.trace_dir, &[&format!("types_{checker_index}.json")]);
        let tracer = Arc::new(TypeTracer {
            fs: self.fs,
            types_path: types_path.clone(),
            types: Mutex::new(Vec::new()),
        });
        state.tracers.push(Arc::clone(&tracer));
        state.legend.push(TraceRecord {
            config_file_path: self.config_file_path.clone(),
            trace_path: self.trace_path.clone(),
            types_path,
            checker_id: checker_index,
        });
        tracer
    }

    /// Finalizes the session: closes `trace.json` and writes a sorted
    /// `legend.json`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_tracing::start_tracing;
    /// use tsgo_vfs::vfstest::MapFs;
    /// let fs = MapFs::from_map([("/t/.keep", "")], true);
    /// let tr = start_tracing(&fs, "/t", "", true).unwrap();
    /// assert!(tr.stop_tracing().is_ok());
    /// ```
    ///
    /// Side effects: appends the JSON array closer to `trace.json` and writes
    /// `legend.json`; both under the trace directory.
    // Go: internal/tracing/tracing.go:StopTracing
    pub fn stop_tracing(&self) -> Result<(), TraceError> {
        // Dump types from all tracers BEFORE taking the session lock, because in
        // the full compiler a type's `display()` can re-enter the checker, which
        // calls `push` (needing this lock). Each tracer has its own lock.
        let tracers: Vec<Arc<TypeTracer<'_>>> = {
            let state = self.state.lock().expect("trace state poisoned");
            state.tracers.clone()
        };
        for tracer in &tracers {
            tracer.dump_types()?;
        }

        let mut state = self.state.lock().expect("trace state poisoned");

        if self.trace_started.load(Ordering::SeqCst) {
            if let Some(err) = state.flush_err.take() {
                state.content.clear();
                self.trace_started.store(false, Ordering::SeqCst);
                return Err(TraceError::Fs(err));
            }
            let tail = format!("{}\n]\n", state.content);
            self.fs.append_file(&self.trace_path, &tail)?;
            state.content.clear();
            self.trace_started.store(false, Ordering::SeqCst);
        }

        // Sort legend entries by types path for deterministic output.
        state.legend.sort_by(|a, b| a.types_path.cmp(&b.types_path));

        let legend_path = tsgo_tspath::combine_paths(&self.trace_dir, &["legend.json"]);
        let legend_data = tsgo_json::marshal_indent(&state.legend, "", "  ")
            .map_err(|e| TraceError::Marshal(e.to_string()))?;
        let legend_text = String::from_utf8(legend_data).expect("serialized JSON is valid UTF-8");
        self.fs.write_file(&legend_path, &legend_text)?;

        Ok(())
    }
}

/// A per-checker type tracer: buffers recorded types and dumps them as
/// `types_<n>.json`. Crate-private; constructed via [`Tracing::new_type_tracer`].
// Go: internal/tracing/tracing.go:typeTracer
struct TypeTracer<'fs> {
    fs: &'fs (dyn Fs + Send + Sync),
    types_path: String,
    types: Mutex<Vec<Box<dyn TracedType + Send + Sync>>>,
}

impl Tracer for TypeTracer<'_> {
    // Go: internal/tracing/tracing.go:typeTracer.RecordType
    fn record_type(&self, t: Box<dyn TracedType + Send + Sync>) {
        self.types.lock().expect("type tracer poisoned").push(t);
    }

    // Go: internal/tracing/tracing.go:typeTracer.DumpTypes
    fn dump_types(&self) -> Result<(), TraceError> {
        // Take the buffer out under the lock and release it before building
        // descriptors (Go clones for the same reason).
        let types = std::mem::take(&mut *self.types.lock().expect("type tracer poisoned"));
        if types.is_empty() {
            return Ok(());
        }

        let mut sb = String::new();
        // No newline after `[` so each type's id matches its 1-based line number.
        sb.push('[');

        let mut recursion_identity_map: FxHashMap<RecursionId, usize> = FxHashMap::default();
        let last = types.len() - 1;
        for (i, typ) in types.iter().enumerate() {
            let descriptor = build_type_descriptor(typ.as_ref(), &mut recursion_identity_map);
            let bytes =
                tsgo_json::marshal(&descriptor).map_err(|e| TraceError::Marshal(e.to_string()))?;
            sb.push_str(std::str::from_utf8(&bytes).expect("serialized JSON is valid UTF-8"));
            if i < last {
                sb.push_str(",\n");
            }
        }
        sb.push_str("]\n");

        self.fs.write_file(&self.types_path, &sb)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
