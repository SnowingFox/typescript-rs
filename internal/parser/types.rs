//! Parser-internal flag set (`ParseFlags`).

use bitflags::bitflags;

bitflags! {
    /// Flags controlling sub-productions of the parser (yield/await context for
    /// function bodies, type-parsing mode, JSDoc, and missing-brace recovery).
    ///
    /// Side effects: none (pure value type).
    // Go: internal/parser/types.go:ParseFlags
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ParseFlags: u32 {
        /// No flags.
        const NONE = 0;
        /// Parse the body in a `[Yield]` context.
        const YIELD = 1 << 0;
        /// Parse the body in an `[Await]` context.
        const AWAIT = 1 << 1;
        /// Parse in a type position.
        const TYPE = 1 << 2;
        /// Tolerate a missing opening brace (error recovery).
        const IGNORE_MISSING_OPEN_BRACE = 1 << 4;
        /// Parsing via the JSDoc parser.
        const JSDOC = 1 << 5;
    }
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
