//! `tsgo_tsoptions` — 1:1 Rust port of Go `internal/tsoptions`.
//!
//! Parses and validates compiler options from the command line (`tsc` /
//! `tsc -b`) and from `tsconfig.json`/`jsconfig.json` (including `extends`
//! inheritance, `${configDir}` substitution, `files`/`include`/`exclude`
//! wildcard expansion, and project references). It also exposes the option
//! declaration tables and the various name/enum maps that drive parsing.
//!
//! ## Mega-file split (PORTING.md §2)
//!
//! Go `internal/tsoptions` is ~6k LOC across several large files
//! (`tsconfigparsing.go` 1815, `declscompiler.go` 1265, ...). Per PORTING.md
//! §2 the port is split into cohesive sibling modules rather than one giant
//! `lib.rs`. The "Go file -> Rust file" mapping lives in the package `impl.md`.

mod commandlineoption;
mod commandlineparser;
mod declsbuild;
mod declscompiler;
mod declstypeacquisition;
mod declswatch;
mod diagnostics;
mod enummaps;
mod errors;
mod namemap;
mod optionsfields;
mod parsedbuildcommandline;
mod parsedcommandline;
mod parsinghelpers;
mod tsconfigparsing;
pub mod tsoptionstest;
mod wildcarddirectories;

pub use commandlineoption::*;
pub use commandlineparser::*;
pub use declsbuild::*;
pub use declscompiler::*;
pub use declstypeacquisition::*;
pub use declswatch::*;
pub use diagnostics::*;
pub use enummaps::*;
pub use errors::*;
pub use namemap::*;
pub use optionsfields::*;
pub use parsedbuildcommandline::*;
pub use parsedcommandline::*;
pub use parsinghelpers::*;
pub use tsconfigparsing::*;
pub use wildcarddirectories::*;

/// A diagnostic produced while parsing options.
///
/// Re-exports the minimal [`tsgo_parser::Diagnostic`] used across the port
/// (the full `ast.Diagnostic` lands in a later phase).
pub use tsgo_parser::Diagnostic;

use tsgo_collections::OrderedMap;

/// A dynamically-typed option value used as the intermediate representation
/// while parsing JSON / command-line options.
///
/// This is the port's replacement for Go's pervasive `any`: where Go stores
/// `nil` / `bool` / `float64` / `string` / `[]any` / `*OrderedMap[string, any]`,
/// this discriminated union makes the type explicit. Enum option values
/// (`target`, `module`, ...) are stored as their `i32` discriminant in
/// [`OptionValue::Int`]; `lib` file names and other strings use
/// [`OptionValue::String`].
///
/// # Examples
/// ```
/// use tsgo_tsoptions::OptionValue;
/// let v = OptionValue::Array(vec![OptionValue::String("a".into())]);
/// assert!(matches!(v, OptionValue::Array(_)));
/// ```
///
/// Side effects: none (pure value type).
#[derive(Clone, Debug, Default)]
pub enum OptionValue {
    /// JSON `null` / Go `nil`.
    #[default]
    Null,
    /// A boolean.
    Bool(bool),
    /// A JSON number (always `f64`, as produced by JSON parsing).
    Number(f64),
    /// An integer (produced by `parseNumber` and enum-discriminant conversion).
    Int(i32),
    /// A string.
    String(String),
    /// An array of values.
    Array(Vec<OptionValue>),
    /// An insertion-ordered object.
    Map(OrderedMap<String, OptionValue>),
}

// `OrderedMap` does not implement `PartialEq`, so `OptionValue` equality is
// written by hand. Maps compare order-insensitively (same size, equal value per
// key), matching Go's `reflect.DeepEqual` for maps.
impl PartialEq for OptionValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (OptionValue::Null, OptionValue::Null) => true,
            (OptionValue::Bool(a), OptionValue::Bool(b)) => a == b,
            (OptionValue::Number(a), OptionValue::Number(b)) => a == b,
            (OptionValue::Int(a), OptionValue::Int(b)) => a == b,
            (OptionValue::String(a), OptionValue::String(b)) => a == b,
            (OptionValue::Array(a), OptionValue::Array(b)) => a == b,
            (OptionValue::Map(a), OptionValue::Map(b)) => {
                a.size() == b.size() && a.entries().all(|(k, v)| b.get(k).is_some_and(|bv| bv == v))
            }
            _ => false,
        }
    }
}

impl OptionValue {
    /// Reports whether this is [`OptionValue::Null`].
    ///
    /// Side effects: none (pure).
    pub fn is_null(&self) -> bool {
        matches!(self, OptionValue::Null)
    }

    /// Returns the string slice if this is [`OptionValue::String`].
    ///
    /// Side effects: none (pure).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            OptionValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the array slice if this is [`OptionValue::Array`].
    ///
    /// Side effects: none (pure).
    pub fn as_array(&self) -> Option<&[OptionValue]> {
        match self {
            OptionValue::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Returns the map if this is [`OptionValue::Map`].
    ///
    /// Side effects: none (pure).
    pub fn as_map(&self) -> Option<&OrderedMap<String, OptionValue>> {
        match self {
            OptionValue::Map(m) => Some(m),
            _ => None,
        }
    }
}
