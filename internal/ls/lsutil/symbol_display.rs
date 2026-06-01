//! Script-element kind/modifier enums (reachable subset of `symbol_display.go`).
//!
//! Ports the `ScriptElementKind` and `ScriptElementKindModifier` value types
//! (plus the modifier name table) from `internal/ls/lsutil/symbol_display.go`.
//!
//! The functions that compute these from a symbol — `GetSymbolKind`,
//! `GetSymbolModifiers`, and their helpers — are deferred (see crate docs): they
//! take a `*checker.Checker`, which is not ported. They belong with the rest of
//! the checker-dependent language-service surface in the `ls` root.

use bitflags::bitflags;

/// The high-level kind of a program element, as surfaced to the language
/// service (completions, quick info, ...).
///
/// Mirrors Go's `iota`-numbered `ScriptElementKind`; the discriminants match
/// 1:1, so `ScriptElementKind::Unknown as i32 == 0`.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::ScriptElementKind;
/// assert_eq!(ScriptElementKind::Unknown as i32, 0);
/// assert_eq!(ScriptElementKind::ClassElement as i32, 5);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKind
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(i32)]
pub enum ScriptElementKind {
    /// Unknown / unclassified.
    #[default]
    Unknown = 0,
    Warning,
    /// Predefined type (`void`) or keyword (`class`).
    Keyword,
    /// Top-level script node.
    ScriptElement,
    /// `module foo {}`.
    ModuleElement,
    /// `class X {}`.
    ClassElement,
    /// `var x = class X {}`.
    LocalClassElement,
    /// `interface Y {}`.
    InterfaceElement,
    /// `type T = ...`.
    TypeElement,
    /// `enum E {}`.
    EnumElement,
    EnumMemberElement,
    /// `const v = ...` (module/script scope).
    VariableElement,
    /// Local variable (function scope).
    LocalVariableElement,
    /// `using foo = ...`.
    VariableUsingElement,
    /// `await using foo = ...`.
    VariableAwaitUsingElement,
    /// `function f() {}` (module/script scope).
    FunctionElement,
    /// Local function (function scope).
    LocalFunctionElement,
    /// `class X { foo() {} }`.
    MemberFunctionElement,
    /// `class X { get foo() {} }`.
    MemberGetAccessorElement,
    /// `class X { set foo(v) {} }`.
    MemberSetAccessorElement,
    /// `class X { foo: number }` / `interface Y { foo: number }`.
    MemberVariableElement,
    /// `class X { accessor foo: number }`.
    MemberAccessorVariableElement,
    /// `class X { constructor() {} }` / `class X { static {} }`.
    ConstructorImplementationElement,
    /// `interface Y { (): number }`.
    CallSignatureElement,
    /// `interface Y { []: number }`.
    IndexSignatureElement,
    /// `interface Y { new (): Y }`.
    ConstructSignatureElement,
    /// A parameter (`function foo(y: string)`).
    ParameterElement,
    TypeParameterElement,
    PrimitiveType,
    Label,
    Alias,
    ConstElement,
    LetElement,
    Directory,
    ExternalModuleName,
    /// String literal.
    String,
    /// JSDoc `@link`: the `{@link ` / `}` framing text.
    Link,
    /// JSDoc `@link`: the entity name (`C` in `{@link C link text}`).
    LinkName,
    /// JSDoc `@link`: the link text.
    LinkText,
}

bitflags! {
    /// Modifiers attached to a program element (accessibility, `static`,
    /// `abstract`, file-extension hints, ...).
    ///
    /// Mirrors Go's `ScriptElementKindModifier`. Per Go's `iota` start, the
    /// first declared flag (`Public`) is bit 1 (value `2`); bit 0 (value `1`)
    /// is intentionally unused.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ls_lsutil::ScriptElementKindModifier as M;
    /// assert_eq!(M::PUBLIC.bits(), 1 << 1);
    /// assert_eq!(M::CJS.bits(), 1 << 21);
    /// assert!(M::empty().is_empty());
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ls/lsutil/symbol_display.go:ScriptElementKindModifier
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct ScriptElementKindModifier: u32 {
        /// `public`.
        const PUBLIC = 1 << 1;
        /// `private`.
        const PRIVATE = 1 << 2;
        /// `protected`.
        const PROTECTED = 1 << 3;
        /// `export`.
        const EXPORTED = 1 << 4;
        /// `declare`.
        const AMBIENT = 1 << 5;
        /// `static`.
        const STATIC = 1 << 6;
        /// `abstract`.
        const ABSTRACT = 1 << 7;
        /// `optional`.
        const OPTIONAL = 1 << 8;
        /// `deprecated`.
        const DEPRECATED = 1 << 9;
        /// `.d.ts`.
        const DTS = 1 << 10;
        /// `.ts`.
        const TS = 1 << 11;
        /// `.tsx`.
        const TSX = 1 << 12;
        /// `.js`.
        const JS = 1 << 13;
        /// `.jsx`.
        const JSX = 1 << 14;
        /// `.json`.
        const JSON = 1 << 15;
        /// `.d.mts`.
        const DMTS = 1 << 16;
        /// `.mts`.
        const MTS = 1 << 17;
        /// `.mjs`.
        const MJS = 1 << 18;
        /// `.d.cts`.
        const DCTS = 1 << 19;
        /// `.cts`.
        const CTS = 1 << 20;
        /// `.cjs`.
        const CJS = 1 << 21;
    }
}

/// The modifier-flag/name pairs, in the fixed order Go iterates them.
///
/// Side effects: none (static table).
// Go: internal/ls/lsutil/symbol_display.go:scriptElementKindModifierNames
const SCRIPT_ELEMENT_KIND_MODIFIER_NAMES: &[(ScriptElementKindModifier, &str)] = &[
    (ScriptElementKindModifier::PUBLIC, "public"),
    (ScriptElementKindModifier::PRIVATE, "private"),
    (ScriptElementKindModifier::PROTECTED, "protected"),
    (ScriptElementKindModifier::EXPORTED, "export"),
    (ScriptElementKindModifier::AMBIENT, "declare"),
    (ScriptElementKindModifier::STATIC, "static"),
    (ScriptElementKindModifier::ABSTRACT, "abstract"),
    (ScriptElementKindModifier::OPTIONAL, "optional"),
    (ScriptElementKindModifier::DEPRECATED, "deprecated"),
    (ScriptElementKindModifier::DTS, ".d.ts"),
    (ScriptElementKindModifier::TS, ".ts"),
    (ScriptElementKindModifier::TSX, ".tsx"),
    (ScriptElementKindModifier::JS, ".js"),
    (ScriptElementKindModifier::JSX, ".jsx"),
    (ScriptElementKindModifier::JSON, ".json"),
    (ScriptElementKindModifier::DMTS, ".d.mts"),
    (ScriptElementKindModifier::MTS, ".mts"),
    (ScriptElementKindModifier::MJS, ".mjs"),
    (ScriptElementKindModifier::DCTS, ".d.cts"),
    (ScriptElementKindModifier::CTS, ".cts"),
    (ScriptElementKindModifier::CJS, ".cjs"),
];

/// The subset of [`ScriptElementKindModifier`] flags that denote file
/// extensions.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{ScriptElementKindModifier as M, FILE_EXTENSION_KIND_MODIFIERS};
/// assert!(FILE_EXTENSION_KIND_MODIFIERS.contains(M::DTS));
/// assert!(!FILE_EXTENSION_KIND_MODIFIERS.contains(M::PUBLIC));
/// ```
///
/// Side effects: none (pure value).
// Go: internal/ls/lsutil/symbol_display.go:FileExtensionKindModifiers
pub const FILE_EXTENSION_KIND_MODIFIERS: ScriptElementKindModifier = ScriptElementKindModifier::DTS
    .union(ScriptElementKindModifier::TS)
    .union(ScriptElementKindModifier::TSX)
    .union(ScriptElementKindModifier::JS)
    .union(ScriptElementKindModifier::JSX)
    .union(ScriptElementKindModifier::JSON)
    .union(ScriptElementKindModifier::DMTS)
    .union(ScriptElementKindModifier::MTS)
    .union(ScriptElementKindModifier::MJS)
    .union(ScriptElementKindModifier::DCTS)
    .union(ScriptElementKindModifier::CTS)
    .union(ScriptElementKindModifier::CJS);

impl ScriptElementKindModifier {
    /// Returns the modifier names present in `self`, in the fixed table order.
    ///
    /// Go returns a `collections.Set[string]`; this port returns the names in
    /// the deterministic `scriptElementKindModifierNames` order (which the LS
    /// joins for display), avoiding a dependency on the set type while keeping
    /// the observable contents identical.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ls_lsutil::ScriptElementKindModifier as M;
    /// let m = M::PUBLIC | M::STATIC;
    /// assert_eq!(m.strings(), vec!["public", "static"]);
    /// assert!(M::empty().strings().is_empty());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsutil/symbol_display.go:ScriptElementKindModifier.Strings
    pub fn strings(self) -> Vec<&'static str> {
        SCRIPT_ELEMENT_KIND_MODIFIER_NAMES
            .iter()
            .filter(|(flag, _)| self.contains(*flag))
            .map(|(_, name)| *name)
            .collect()
    }
}

#[cfg(test)]
#[path = "symbol_display_test.rs"]
mod tests;
