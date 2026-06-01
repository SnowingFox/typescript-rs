//! User-preference enums (reachable subset of Go `userpreferences.go`).
//!
//! Go's `internal/ls/lsutil/userpreferences.go` defines the full
//! `UserPreferences` struct plus reflection-based config (un)marshaling. That
//! machinery is deferred (see crate docs): it depends on Go reflection,
//! `internal/json`, `modulespecifiers`, and `vfsmatch`, none of which have a 1:1
//! Rust analog yet. Only the small value enum needed by the reachable syntactic
//! helpers — [`QuotePreference`] — is ported here.

/// The quote style the language service prefers when emitting string literals.
///
/// Go models this as a `string`-valued type; [`QuotePreference::as_str`] returns
/// the same wire values (`""`, `"auto"`, `"double"`, `"single"`).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::QuotePreference;
/// assert_eq!(QuotePreference::Single.as_str(), "single");
/// assert_eq!(QuotePreference::Unknown.as_str(), "");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ls/lsutil/userpreferences.go:QuotePreference
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum QuotePreference {
    /// Unset (Go `QuotePreferenceUnknown`, the empty string).
    #[default]
    Unknown,
    /// Detect from existing imports (Go `QuotePreferenceAuto`).
    Auto,
    /// Prefer double quotes (Go `QuotePreferenceDouble`).
    Double,
    /// Prefer single quotes (Go `QuotePreferenceSingle`).
    Single,
}

impl QuotePreference {
    /// Returns the Go wire string for this preference.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ls_lsutil::QuotePreference;
    /// assert_eq!(QuotePreference::Double.as_str(), "double");
    /// assert_eq!(QuotePreference::Auto.as_str(), "auto");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsutil/userpreferences.go:QuotePreference constants
    pub fn as_str(self) -> &'static str {
        match self {
            QuotePreference::Unknown => "",
            QuotePreference::Auto => "auto",
            QuotePreference::Double => "double",
            QuotePreference::Single => "single",
        }
    }
}

#[cfg(test)]
#[path = "userpreferences_test.rs"]
mod tests;
