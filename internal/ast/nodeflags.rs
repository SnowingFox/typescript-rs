//! `NodeFlags` bit set carried by every AST node.

bitflags::bitflags! {
    /// Per-node bit flags set by the parser and binder.
    ///
    /// Mirrors Go `NodeFlags` (a `uint32` `iota` enum). Base flags occupy single
    /// bits; the trailing constants are unions used as masks. The identifier-only
    /// `IDENTIFIER_HAS_EXTENDED_UNICODE_ESCAPE` deliberately reuses the
    /// `CONTAINS_THIS` bit, matching Go.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::nodeflags::NodeFlags;
    /// assert_eq!(
    ///     NodeFlags::BLOCK_SCOPED,
    ///     NodeFlags::LET | NodeFlags::CONST | NodeFlags::USING
    /// );
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/nodeflags.go:NodeFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct NodeFlags: u32 {
        /// No flags set.
        const NONE = 0;
        /// `let` variable declaration.
        const LET = 1 << 0;
        /// `const` variable declaration.
        const CONST = 1 << 1;
        /// `using` variable declaration.
        const USING = 1 << 2;
        /// Node was synthesized during parsing (reparse).
        const REPARSED = 1 << 3;
        /// Node was synthesized during transformation.
        const SYNTHESIZED = 1 << 4;
        /// Chained member expression rooted to a pseudo-`OptionalExpression`.
        const OPTIONAL_CHAIN = 1 << 5;
        /// Export context (initialized by binding).
        const EXPORT_CONTEXT = 1 << 6;
        /// Interface contains references to `this`.
        const CONTAINS_THIS = 1 << 7;
        /// Function implicitly returns on one of its code paths.
        const HAS_IMPLICIT_RETURN = 1 << 8;
        /// Function has an explicit reachable return on one of its code paths.
        const HAS_EXPLICIT_RETURN = 1 << 9;
        /// Parsed where `in`-expressions are not allowed.
        const DISALLOW_IN_CONTEXT = 1 << 10;
        /// Parsed in the `yield` context of a generator.
        const YIELD_CONTEXT = 1 << 11;
        /// Parsed as part of a decorator.
        const DECORATOR_CONTEXT = 1 << 12;
        /// Parsed in the `await` context of an async function.
        const AWAIT_CONTEXT = 1 << 13;
        /// Parsed where conditional types are not allowed.
        const DISALLOW_CONDITIONAL_TYPES_CONTEXT = 1 << 14;
        /// The parser encountered an error in the code that created this node.
        const THIS_NODE_HAS_ERROR = 1 << 15;
        /// Parsed in a JavaScript file.
        const JAVA_SCRIPT_FILE = 1 << 16;
        /// This node or one of its children had an error.
        const THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR = 1 << 17;
        /// The file has async functions (initialized by binding).
        const HAS_ASYNC_FUNCTIONS = 1 << 18;
        /// The parser saw a dynamic `import(...)` somewhere (approximate).
        const POSSIBLY_CONTAINS_DYNAMIC_IMPORT = 1 << 19;
        /// The parser saw `import.meta` somewhere (approximate).
        const POSSIBLY_CONTAINS_IMPORT_META = 1 << 20;
        /// Node has preceding JSDoc comment(s).
        const HAS_JSDOC = 1 << 21;
        /// Node was parsed inside JSDoc.
        const JSDOC = 1 << 22;
        /// Node was inside an ambient context (declaration file or `declare`).
        const AMBIENT = 1 << 23;
        /// Some ancestor was the `statement` of a `with` statement.
        const IN_WITH_STATEMENT = 1 << 24;
        /// Node was parsed in a JSON file.
        const JSON_FILE = 1 << 25;
        /// Comment text may contain `@deprecated` (confirm via JSDoc lookup).
        const POSSIBLY_CONTAINS_DEPRECATED_TAG = 1 << 26;
        /// Node is unreachable according to the binder.
        const UNREACHABLE = 1 << 27;

        /// Any block-scoped declaration kind.
        const BLOCK_SCOPED = (1 << 0) | (1 << 1) | (1 << 2);
        /// `const`-like (immutable binding) declaration kind.
        const CONSTANT = (1 << 1) | (1 << 2);
        /// `await using` declaration (shares bits with `const`/`using`).
        const AWAIT_USING = (1 << 1) | (1 << 2);

        /// Reachability flags computed by the binder.
        const REACHABILITY_CHECK_FLAGS = (1 << 8) | (1 << 9);
        /// Reachability flags plus the async-functions marker.
        const REACHABILITY_AND_EMIT_FLAGS = (1 << 8) | (1 << 9) | (1 << 18);

        /// Parsing context flags that propagate to child nodes.
        const CONTEXT_FLAGS = (1 << 10)
            | (1 << 14)
            | (1 << 11)
            | (1 << 12)
            | (1 << 13)
            | (1 << 16)
            | (1 << 24)
            | (1 << 23);

        /// Context flags excluded when parsing a type.
        const TYPE_EXCLUDES_FLAGS = (1 << 11) | (1 << 13);

        /// Flags that, once set on a reused `SourceFile`, are never cleared.
        const PERMANENTLY_SET_INCREMENTAL_FLAGS = (1 << 19) | (1 << 20);

        /// On identifiers, reuses the `CONTAINS_THIS` bit to mark an extended
        /// unicode escape.
        const IDENTIFIER_HAS_EXTENDED_UNICODE_ESCAPE = 1 << 7;
    }
}

#[cfg(test)]
#[path = "nodeflags_test.rs"]
mod tests;
