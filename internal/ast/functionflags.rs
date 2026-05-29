//! `FunctionFlags` describing generator/async/validity of a function-like node.

bitflags::bitflags! {
    /// Describes whether a function-like node is a generator, async, both, or
    /// invalid (no body).
    ///
    /// Mirrors Go `FunctionFlags`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::functionflags::FunctionFlags;
    /// assert_eq!(
    ///     FunctionFlags::ASYNC_GENERATOR,
    ///     FunctionFlags::ASYNC | FunctionFlags::GENERATOR
    /// );
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/functionflags.go:FunctionFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct FunctionFlags: u32 {
        /// A normal function.
        const NORMAL = 0;
        /// A generator function.
        const GENERATOR = 1 << 0;
        /// An async function.
        const ASYNC = 1 << 1;
        /// An invalid function (e.g. missing body).
        const INVALID = 1 << 2;
        /// An async generator.
        const ASYNC_GENERATOR = (1 << 1) | (1 << 0);
    }
}

// `get_function_flags` is intentionally not provided here yet: it reads a node's
// `BodyData` and `HasSyntacticModifier` through the arena, which is implemented
// alongside `NodeArena`. See `lib.rs` `NodeArena::get_function_flags`.

#[cfg(test)]
#[path = "functionflags_test.rs"]
mod tests;
