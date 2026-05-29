//! `ModifierFlags` bit set for syntactic and JSDoc modifiers.

bitflags::bitflags! {
    /// Modifiers attached to a declaration (`public`, `static`, `export`, ...),
    /// plus a cache-only segment for JSDoc-derived modifiers.
    ///
    /// Mirrors Go `ModifierFlags`. The JSDoc cache-only bits start at bit 23 and
    /// intentionally mirror the order of the syntactic/JSDoc modifiers above.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::modifierflags::ModifierFlags;
    /// assert_eq!(
    ///     ModifierFlags::MODIFIER,
    ///     ModifierFlags::ALL & !ModifierFlags::DECORATOR
    /// );
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/modifierflags.go:ModifierFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct ModifierFlags: u32 {
        /// No modifiers.
        const NONE = 0;
        /// `public` modifier.
        const PUBLIC = 1 << 0;
        /// `private` modifier.
        const PRIVATE = 1 << 1;
        /// `protected` modifier.
        const PROTECTED = 1 << 2;
        /// `readonly` modifier.
        const READONLY = 1 << 3;
        /// `override` modifier.
        const OVERRIDE = 1 << 4;
        /// `export` modifier.
        const EXPORT = 1 << 5;
        /// `abstract` modifier.
        const ABSTRACT = 1 << 6;
        /// `declare` (ambient) modifier.
        const AMBIENT = 1 << 7;
        /// `static` modifier.
        const STATIC = 1 << 8;
        /// `accessor` modifier.
        const ACCESSOR = 1 << 9;
        /// `async` modifier.
        const ASYNC = 1 << 10;
        /// `default` modifier (in `export default`).
        const DEFAULT = 1 << 11;
        /// `const` modifier (const enum).
        const CONST = 1 << 12;
        /// `in` (contravariance) modifier.
        const IN = 1 << 13;
        /// `out` (covariance) modifier.
        const OUT = 1 << 14;
        /// Contains a decorator.
        const DECORATOR = 1 << 15;
        /// JSDoc `@deprecated` tag.
        const DEPRECATED = 1 << 16;
        /// Cache-only JSDoc `public`.
        const JSDOC_PUBLIC = 1 << 23;
        /// Cache-only JSDoc `private`.
        const JSDOC_PRIVATE = 1 << 24;
        /// Cache-only JSDoc `protected`.
        const JSDOC_PROTECTED = 1 << 25;
        /// Cache-only JSDoc `readonly`.
        const JSDOC_READONLY = 1 << 26;
        /// Cache-only JSDoc `override`.
        const JSDOC_OVERRIDE = 1 << 27;
        /// Computed flags include modifiers derived from JSDoc.
        const HAS_COMPUTED_JSDOC_MODIFIERS = 1 << 28;
        /// Modifier flags have been computed.
        const HAS_COMPUTED_FLAGS = 1 << 29;

        /// Modifiers that may also come from JSDoc.
        const SYNTACTIC_OR_JSDOC_MODIFIERS = Self::PUBLIC.bits()
            | Self::PRIVATE.bits()
            | Self::PROTECTED.bits()
            | Self::READONLY.bits()
            | Self::OVERRIDE.bits();
        /// Modifiers that are syntactic only.
        const SYNTACTIC_ONLY_MODIFIERS = Self::EXPORT.bits()
            | Self::AMBIENT.bits()
            | Self::ABSTRACT.bits()
            | Self::STATIC.bits()
            | Self::ACCESSOR.bits()
            | Self::ASYNC.bits()
            | Self::DEFAULT.bits()
            | Self::CONST.bits()
            | Self::IN.bits()
            | Self::OUT.bits()
            | Self::DECORATOR.bits();
        /// All syntactic modifiers.
        const SYNTACTIC_MODIFIERS = Self::SYNTACTIC_OR_JSDOC_MODIFIERS.bits() | Self::SYNTACTIC_ONLY_MODIFIERS.bits();
        /// JSDoc cache-only modifiers.
        const JSDOC_CACHE_ONLY_MODIFIERS = Self::JSDOC_PUBLIC.bits()
            | Self::JSDOC_PRIVATE.bits()
            | Self::JSDOC_PROTECTED.bits()
            | Self::JSDOC_READONLY.bits()
            | Self::JSDOC_OVERRIDE.bits();
        /// Modifiers that come only from JSDoc.
        const JSDOC_ONLY_MODIFIERS = Self::DEPRECATED.bits();
        /// All non-cache-only modifiers.
        const NON_CACHE_ONLY_MODIFIERS = Self::SYNTACTIC_OR_JSDOC_MODIFIERS.bits()
            | Self::SYNTACTIC_ONLY_MODIFIERS.bits()
            | Self::JSDOC_ONLY_MODIFIERS.bits();

        /// Accessibility modifiers (`public`/`private`/`protected`).
        const ACCESSIBILITY_MODIFIER = Self::PUBLIC.bits() | Self::PRIVATE.bits() | Self::PROTECTED.bits();
        /// Modifiers allowed on a constructor parameter property.
        const PARAMETER_PROPERTY_MODIFIER = Self::ACCESSIBILITY_MODIFIER.bits() | Self::READONLY.bits() | Self::OVERRIDE.bits();
        /// Non-`public` accessibility modifiers.
        const NON_PUBLIC_ACCESSIBILITY_MODIFIER = Self::PRIVATE.bits() | Self::PROTECTED.bits();

        /// Modifiers meaningful only in TypeScript.
        const TYPE_SCRIPT_MODIFIER = Self::AMBIENT.bits()
            | Self::PUBLIC.bits()
            | Self::PRIVATE.bits()
            | Self::PROTECTED.bits()
            | Self::READONLY.bits()
            | Self::ABSTRACT.bits()
            | Self::CONST.bits()
            | Self::OVERRIDE.bits()
            | Self::IN.bits()
            | Self::OUT.bits();
        /// `export default`.
        const EXPORT_DEFAULT = Self::EXPORT.bits() | Self::DEFAULT.bits();
        /// All modifiers (including decorators).
        const ALL = Self::EXPORT.bits()
            | Self::AMBIENT.bits()
            | Self::PUBLIC.bits()
            | Self::PRIVATE.bits()
            | Self::PROTECTED.bits()
            | Self::STATIC.bits()
            | Self::READONLY.bits()
            | Self::ABSTRACT.bits()
            | Self::ACCESSOR.bits()
            | Self::ASYNC.bits()
            | Self::DEFAULT.bits()
            | Self::CONST.bits()
            | Self::DEPRECATED.bits()
            | Self::OVERRIDE.bits()
            | Self::IN.bits()
            | Self::OUT.bits()
            | Self::DECORATOR.bits();
        /// All modifiers except decorators.
        const MODIFIER = Self::ALL.bits() & !Self::DECORATOR.bits();
        /// Modifiers meaningful in JavaScript.
        const JAVA_SCRIPT = Self::EXPORT.bits()
            | Self::STATIC.bits()
            | Self::ACCESSOR.bits()
            | Self::ASYNC.bits()
            | Self::DEFAULT.bits();
    }
}

impl Default for ModifierFlags {
    fn default() -> Self {
        ModifierFlags::empty()
    }
}

#[cfg(test)]
#[path = "modifierflags_test.rs"]
mod tests;
