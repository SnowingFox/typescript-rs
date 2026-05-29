//! Source-file script kind (`ScriptKind`) with `Display`.
//!
//! 1:1 port of Go `internal/core/scriptkind.go` and
//! `scriptkind_stringer_generated.go`.

/// The kind of a source file, derived from its extension or content.
///
/// Discriminant values mirror the Go `iota` order and are relied upon by
/// range comparisons elsewhere in the compiler.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum ScriptKind {
    /// Unknown / unset.
    #[default]
    Unknown = 0,
    /// JavaScript (`.js`).
    Js = 1,
    /// JavaScript with JSX (`.jsx`).
    Jsx = 2,
    /// TypeScript (`.ts`).
    Ts = 3,
    /// TypeScript with JSX (`.tsx`).
    Tsx = 4,
    /// External (host-provided) file.
    External = 5,
    /// JSON (`.json`).
    Json = 6,
    /// Deferred: extension does not define the kind; content does.
    Deferred = 7,
}

// Go: internal/core/scriptkind_stringer_generated.go:String
impl std::fmt::Display for ScriptKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ScriptKind::Unknown => "ScriptKindUnknown",
            ScriptKind::Js => "ScriptKindJS",
            ScriptKind::Jsx => "ScriptKindJSX",
            ScriptKind::Ts => "ScriptKindTS",
            ScriptKind::Tsx => "ScriptKindTSX",
            ScriptKind::External => "ScriptKindExternal",
            ScriptKind::Json => "ScriptKindJSON",
            ScriptKind::Deferred => "ScriptKindDeferred",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
#[path = "scriptkind_test.rs"]
mod tests;
