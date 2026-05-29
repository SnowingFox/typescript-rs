//! Language variant (`LanguageVariant`) with `Display`.
//!
//! 1:1 port of Go `internal/core/languagevariant.go` and
//! `languagevariant_stringer_generated.go`.

/// Whether a source file is parsed as standard TS/JS or with JSX enabled.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum LanguageVariant {
    /// Standard TypeScript / JavaScript.
    #[default]
    Standard = 0,
    /// JSX-enabled variant.
    Jsx = 1,
}

// Go: internal/core/languagevariant_stringer_generated.go:String
impl std::fmt::Display for LanguageVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            LanguageVariant::Standard => "LanguageVariantStandard",
            LanguageVariant::Jsx => "LanguageVariantJSX",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
#[path = "languagevariant_test.rs"]
mod tests;
