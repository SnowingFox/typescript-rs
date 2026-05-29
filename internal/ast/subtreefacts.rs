//! `SubtreeFacts` bit set summarizing transform-relevant syntax in a subtree.

bitflags::bitflags! {
    /// Summarizes which down-leveling-relevant syntax appears in a node and its
    /// subtree, used by the transformers to decide what to lower.
    ///
    /// Mirrors Go `SubtreeFacts` (a `uint32` `iota` enum). `COMPUTED` is the
    /// highest single bit and marks that the facts have been computed; the
    /// `EXCLUSIONS_*` masks remove flags that must not propagate out of a scope.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::subtreefacts::SubtreeFacts;
    /// assert_eq!(SubtreeFacts::EXCLUSIONS_NODE, SubtreeFacts::COMPUTED);
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/subtreefacts.go:SubtreeFacts
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct SubtreeFacts: u32 {
        /// No facts.
        const NONE = 0;

        // Facts: subtree contains syntax relevant to a specific transform.
        /// Subtree contains TypeScript-only syntax.
        const CONTAINS_TYPESCRIPT = 1 << 0;
        /// Subtree contains JSX.
        const CONTAINS_JSX = 1 << 1;
        /// Subtree contains ECMAScript decorators.
        const CONTAINS_ES_DECORATORS = 1 << 2;
        /// Subtree contains `using` declarations.
        const CONTAINS_USING = 1 << 3;
        /// Subtree contains class static blocks.
        const CONTAINS_CLASS_STATIC_BLOCKS = 1 << 4;
        /// Subtree contains ECMAScript class fields.
        const CONTAINS_ES_CLASS_FIELDS = 1 << 5;
        /// Subtree contains logical assignment operators.
        const CONTAINS_LOGICAL_ASSIGNMENTS = 1 << 6;
        /// Subtree contains nullish coalescing.
        const CONTAINS_NULLISH_COALESCING = 1 << 7;
        /// Subtree contains optional chaining.
        const CONTAINS_OPTIONAL_CHAINING = 1 << 8;
        /// Subtree contains a missing catch-clause variable.
        const CONTAINS_MISSING_CATCH_CLAUSE_VARIABLE = 1 << 9;
        /// Subtree contains object rest/spread (never cleared).
        const CONTAINS_ES_OBJECT_REST_OR_SPREAD = 1 << 10;
        /// Subtree contains `for await` or an async generator.
        const CONTAINS_FOR_AWAIT_OR_ASYNC_GENERATOR = 1 << 11;
        /// Subtree contains any `await`.
        const CONTAINS_ANY_AWAIT = 1 << 12;
        /// Subtree contains the exponentiation operator.
        const CONTAINS_EXPONENTIATION_OPERATOR = 1 << 13;

        // Markers: subtree contains a particular kind of syntax.
        /// Subtree contains lexical `this`.
        const CONTAINS_LEXICAL_THIS = 1 << 14;
        /// Subtree contains lexical `super`.
        const CONTAINS_LEXICAL_SUPER = 1 << 15;
        /// Marker on any `...` (cleared on binding-pattern exit).
        const CONTAINS_REST_OR_SPREAD = 1 << 16;
        /// Marker on any `{...x}` (cleared on most scope exits).
        const CONTAINS_OBJECT_REST_OR_SPREAD = 1 << 17;
        /// Marker for `await`.
        const CONTAINS_AWAIT = 1 << 18;
        /// Marker for dynamic `import()`.
        const CONTAINS_DYNAMIC_IMPORT = 1 << 19;
        /// Marker for class fields.
        const CONTAINS_CLASS_FIELDS = 1 << 20;
        /// Marker for decorators.
        const CONTAINS_DECORATORS = 1 << 21;
        /// Marker for identifiers.
        const CONTAINS_IDENTIFIER = 1 << 22;
        /// Marker for a private identifier used in an expression.
        const CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION = 1 << 23;
        /// Marker for an invalid template escape.
        const CONTAINS_INVALID_TEMPLATE_ESCAPE = 1 << 24;

        /// The facts have been computed (must be the last single bit).
        const COMPUTED = 1 << 25;

        // Scope exclusions: masks removing flags from propagating out of a scope.
        /// Exclusions for a generic node.
        const EXCLUSIONS_NODE = Self::COMPUTED.bits();
        /// Exclusions for eraseable (TypeScript-only) syntax.
        const EXCLUSIONS_ERASEABLE = !Self::CONTAINS_TYPESCRIPT.bits();
        /// Exclusions for an outer expression.
        const EXCLUSIONS_OUTER_EXPRESSION = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for property access.
        const EXCLUSIONS_PROPERTY_ACCESS = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for element access.
        const EXCLUSIONS_ELEMENT_ACCESS = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for an arrow function.
        const EXCLUSIONS_ARROW_FUNCTION = Self::EXCLUSIONS_NODE.bits()
            | Self::CONTAINS_AWAIT.bits()
            | Self::CONTAINS_OBJECT_REST_OR_SPREAD.bits();
        /// Exclusions for a function.
        const EXCLUSIONS_FUNCTION = Self::EXCLUSIONS_NODE.bits()
            | Self::CONTAINS_LEXICAL_THIS.bits()
            | Self::CONTAINS_LEXICAL_SUPER.bits()
            | Self::CONTAINS_AWAIT.bits()
            | Self::CONTAINS_OBJECT_REST_OR_SPREAD.bits();
        /// Exclusions for a constructor.
        const EXCLUSIONS_CONSTRUCTOR = Self::EXCLUSIONS_FUNCTION.bits();
        /// Exclusions for a method.
        const EXCLUSIONS_METHOD = Self::EXCLUSIONS_FUNCTION.bits();
        /// Exclusions for an accessor.
        const EXCLUSIONS_ACCESSOR = Self::EXCLUSIONS_FUNCTION.bits();
        /// Exclusions for a property.
        const EXCLUSIONS_PROPERTY = Self::EXCLUSIONS_NODE.bits()
            | Self::CONTAINS_LEXICAL_THIS.bits()
            | Self::CONTAINS_LEXICAL_SUPER.bits();
        /// Exclusions for a class.
        const EXCLUSIONS_CLASS = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for a module.
        const EXCLUSIONS_MODULE = Self::EXCLUSIONS_NODE.bits()
            | Self::CONTAINS_LEXICAL_THIS.bits()
            | Self::CONTAINS_LEXICAL_SUPER.bits();
        /// Exclusions for an object literal.
        const EXCLUSIONS_OBJECT_LITERAL = Self::EXCLUSIONS_NODE.bits() | Self::CONTAINS_OBJECT_REST_OR_SPREAD.bits();
        /// Exclusions for an array literal.
        const EXCLUSIONS_ARRAY_LITERAL = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for a call.
        const EXCLUSIONS_CALL = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for `new`.
        const EXCLUSIONS_NEW = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for a variable declaration list.
        const EXCLUSIONS_VARIABLE_DECLARATION_LIST = Self::EXCLUSIONS_NODE.bits() | Self::CONTAINS_OBJECT_REST_OR_SPREAD.bits();
        /// Exclusions for a parameter.
        const EXCLUSIONS_PARAMETER = Self::EXCLUSIONS_NODE.bits();
        /// Exclusions for a catch clause.
        const EXCLUSIONS_CATCH_CLAUSE = Self::EXCLUSIONS_NODE.bits() | Self::CONTAINS_OBJECT_REST_OR_SPREAD.bits();
        /// Exclusions for a binding pattern.
        const EXCLUSIONS_BINDING_PATTERN = Self::EXCLUSIONS_NODE.bits() | Self::CONTAINS_REST_OR_SPREAD.bits();

        /// Mask for either lexical `this` or `super`.
        const CONTAINS_LEXICAL_THIS_OR_SUPER = Self::CONTAINS_LEXICAL_THIS.bits() | Self::CONTAINS_LEXICAL_SUPER.bits();
    }
}

#[cfg(test)]
#[path = "subtreefacts_test.rs"]
mod tests;
