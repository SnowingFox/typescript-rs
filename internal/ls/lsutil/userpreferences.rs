//! User-preference enums (reachable subset of Go `userpreferences.go`).
//!
//! Go's `internal/ls/lsutil/userpreferences.go` defines the full
//! `UserPreferences` struct plus reflection-based config (un)marshaling. That
//! machinery is deferred (see crate docs): it depends on Go reflection,
//! `internal/json`, `modulespecifiers`, and `vfsmatch`, none of which have a 1:1
//! Rust analog yet. Only the small value types needed by the reachable
//! language-service features are ported here: [`QuotePreference`] (the
//! syntactic quote helpers) and the [`InlayHintsPreferences`] /
//! [`IncludeInlayParameterNameHints`] gates (consumed by the `ls` inlay-hint
//! provider).

use tsgo_core::tristate::Tristate;

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

/// Whether (and which) inline parameter-name hints the editor requests at call
/// and `new` argument positions.
///
/// Go models this as a `string`-valued type; [`IncludeInlayParameterNameHints::as_str`]
/// returns the same wire values (`""`, `"all"`, `"literals"`).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::IncludeInlayParameterNameHints;
/// assert_eq!(IncludeInlayParameterNameHints::All.as_str(), "all");
/// assert_eq!(IncludeInlayParameterNameHints::None.as_str(), "");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ls/lsutil/userpreferences.go:IncludeInlayParameterNameHints
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum IncludeInlayParameterNameHints {
    /// No parameter-name hints (Go `IncludeInlayParameterNameHintsNone`, the
    /// empty string).
    #[default]
    None,
    /// Hints for every argument (Go `IncludeInlayParameterNameHintsAll`).
    All,
    /// Hints only for literal arguments (Go
    /// `IncludeInlayParameterNameHintsLiterals`).
    Literals,
}

impl IncludeInlayParameterNameHints {
    /// Returns the Go wire string for this preference.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ls_lsutil::IncludeInlayParameterNameHints;
    /// assert_eq!(IncludeInlayParameterNameHints::Literals.as_str(), "literals");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsutil/userpreferences.go:IncludeInlayParameterNameHints constants
    pub fn as_str(self) -> &'static str {
        match self {
            IncludeInlayParameterNameHints::None => "",
            IncludeInlayParameterNameHints::All => "all",
            IncludeInlayParameterNameHints::Literals => "literals",
        }
    }
}

/// The inlay-hint gates a feature consults to decide which inline hints to emit.
///
/// A faithful 1:1 port of Go's `InlayHintsPreferences`: each field gates one
/// hint kind (parameter names / parameter types / variable types / property
/// declaration types / function-like return types / enum member values) plus
/// the two suppression toggles (`…WhenArgumentMatchesName`,
/// `…WhenTypeMatchesName`). [`Default`] is Go's zero value — every gate
/// off ([`IncludeInlayParameterNameHints::None`] / [`Tristate::Unknown`]) — so
/// the provider returns no hints until the editor opts in.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{InlayHintsPreferences, IncludeInlayParameterNameHints};
/// use tsgo_core::tristate::Tristate;
/// let prefs = InlayHintsPreferences {
///     include_inlay_enum_member_value_hints: Tristate::True,
///     ..Default::default()
/// };
/// assert!(prefs.include_inlay_enum_member_value_hints.is_true());
/// assert_eq!(
///     prefs.include_inlay_parameter_name_hints,
///     IncludeInlayParameterNameHints::None
/// );
/// ```
///
/// Side effects: none (plain data).
// Go: internal/ls/lsutil/userpreferences.go:InlayHintsPreferences
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct InlayHintsPreferences {
    /// Parameter-name hints at call / `new` arguments.
    pub include_inlay_parameter_name_hints: IncludeInlayParameterNameHints,
    /// Show a parameter-name hint even when the argument's name already matches
    /// the parameter (Go inverts the VS Code `suppressWhenArgumentMatchesName`).
    pub include_inlay_parameter_name_hints_when_argument_matches_name: Tristate,
    /// Type hints on function parameters with no type annotation.
    pub include_inlay_function_parameter_type_hints: Tristate,
    /// Type hints on `const`/`let`/`var` declarations with no annotation.
    pub include_inlay_variable_type_hints: Tristate,
    /// Show a variable-type hint even when the type's name matches the variable
    /// name (Go inverts the VS Code `suppressWhenTypeMatchesName`).
    pub include_inlay_variable_type_hints_when_type_matches_name: Tristate,
    /// Type hints on class property declarations with no annotation.
    pub include_inlay_property_declaration_type_hints: Tristate,
    /// Return-type hints on function-like declarations with no annotation.
    pub include_inlay_function_like_return_type_hints: Tristate,
    /// Value hints on enum members with no initializer.
    pub include_inlay_enum_member_value_hints: Tristate,
}

#[cfg(test)]
#[path = "userpreferences_test.rs"]
mod tests;
