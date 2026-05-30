//! `EmitFlags` bit set controlling per-node emit behavior.

bitflags::bitflags! {
    /// Per-node flags read by the emitter to suppress source maps, comments,
    /// adjust indentation, and steer name substitution.
    ///
    /// Mirrors Go `EmitFlags`; the single-bit values follow Go's `1 << iota`
    /// ordering so that values stored on nodes round-trip identically.
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitflags::EmitFlags;
    /// assert_eq!(
    ///     EmitFlags::NO_COMMENTS,
    ///     EmitFlags::NO_LEADING_COMMENTS | EmitFlags::NO_TRAILING_COMMENTS
    /// );
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/printer/emitflags.go:EmitFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct EmitFlags: u32 {
        /// The contents of this node should be emitted on a single line.
        const SINGLE_LINE = 1 << 0;
        /// The contents of this node should be emitted on multiple lines.
        const MULTI_LINE = 1 << 1;
        /// Do not emit a leading source map location for this node.
        const NO_LEADING_SOURCE_MAP = 1 << 2;
        /// Do not emit a trailing source map location for this node.
        const NO_TRAILING_SOURCE_MAP = 1 << 3;
        /// Do not emit source map locations for children of this node.
        const NO_NESTED_SOURCE_MAPS = 1 << 4;
        /// Do not emit leading source map location for token nodes.
        const NO_TOKEN_LEADING_SOURCE_MAPS = 1 << 5;
        /// Do not emit trailing source map location for token nodes.
        const NO_TOKEN_TRAILING_SOURCE_MAPS = 1 << 6;
        /// Do not emit leading comments for this node.
        const NO_LEADING_COMMENTS = 1 << 7;
        /// Do not emit trailing comments for this node.
        const NO_TRAILING_COMMENTS = 1 << 8;
        /// Do not emit nested comments for children of this node.
        const NO_NESTED_COMMENTS = 1 << 9;
        /// The identifier refers to an unscoped emit helper (emitted at the top of the file).
        const HELPER_NAME = 1 << 10;
        /// Ensure an export prefix is added for an identifier pointing to an exported declaration with a local name.
        const EXPORT_NAME = 1 << 11;
        /// Ensure an export prefix is not added for an identifier pointing to an exported declaration.
        const LOCAL_NAME = 1 << 12;
        /// Adds an explicit extra indentation level for class and function bodies when printing.
        const INDENTED = 1 << 13;
        /// Do not indent the node.
        const NO_INDENTATION = 1 << 14;
        /// Reuse the existing temp variable scope during emit.
        const REUSE_TEMP_VARIABLE_SCOPE = 1 << 15;
        /// Treat the statement as if it were a prologue directive.
        const CUSTOM_PROLOGUE = 1 << 16;
        /// Write the text on the node with ASCII escaping substitutions.
        const NO_ASCII_ESCAPING = 1 << 17;
        /// This source file has external helpers.
        const EXTERNAL_HELPERS = 1 << 18;
        /// Start this node on a new line.
        const START_ON_NEW_LINE = 1 << 19;
        /// Emit a `CallExpression` as an indirect call: `(0, f)()`.
        const INDIRECT_CALL = 1 << 20;
        /// The node was originally an async function body.
        const ASYNC_FUNCTION_BODY = 1 << 21;
        /// Do not capture `arguments` for this arrow function.
        const NO_LEXICAL_ARGUMENTS = 1 << 22;
        /// Static private elements in a file or class should be transformed regardless of `--target`.
        const TRANSFORM_PRIVATE_STATIC_ELEMENTS = 1 << 23;

        /// No flags.
        const NONE = 0;
        /// Do not emit a source map location for this node.
        const NO_SOURCE_MAP = (1 << 2) | (1 << 3);
        /// Do not emit source map locations for tokens of this node.
        const NO_TOKEN_SOURCE_MAPS = (1 << 5) | (1 << 6);
        /// Do not emit comments for this node.
        const NO_COMMENTS = (1 << 7) | (1 << 8);
    }
}

#[cfg(test)]
#[path = "emitflags_test.rs"]
mod tests;
