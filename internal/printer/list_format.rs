//! `ListFormat`: bit flags steering how the emitter prints node lists.

bitflags::bitflags! {
    /// Controls delimiters, brackets, indentation, and line breaking when
    /// emitting a node list. Mirrors Go `ListFormat` (an `int` bit set), including
    /// the precomputed combinations used at call sites.
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::ListFormat;
    /// assert!(ListFormat::SOURCE_FILE_STATEMENTS.contains(ListFormat::MULTI_LINE));
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/printer/printer.go:ListFormat
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct ListFormat: i32 {
        /// No flags / single line / not delimited (default).
        const NONE = 0;
        /// Prints the list on multiple lines.
        const MULTI_LINE = 1 << 0;
        /// Prints the list using line preservation if possible.
        const PRESERVE_LINES = 1 << 1;
        /// Mask over the line-style bits.
        const LINES_MASK = (1 << 0) | (1 << 1);

        /// Each list item is space-and-bar (` |`) delimited.
        const BAR_DELIMITED = 1 << 2;
        /// Each list item is space-and-ampersand (` &`) delimited.
        const AMPERSAND_DELIMITED = 1 << 3;
        /// Each list item is comma (`,`) delimited.
        const COMMA_DELIMITED = 1 << 4;
        /// Each list item is asterisk (`\n *`) delimited (JSDoc).
        const ASTERISK_DELIMITED = 1 << 5;
        /// Mask over the delimiter bits.
        const DELIMITERS_MASK = (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5);

        /// Write a trailing comma if present.
        const ALLOW_TRAILING_COMMA = 1 << 6;

        /// The list should be indented.
        const INDENTED = 1 << 7;
        /// Insert a space after the opening brace and before the closing brace.
        const SPACE_BETWEEN_BRACES = 1 << 8;
        /// Insert a space between each sibling node.
        const SPACE_BETWEEN_SIBLINGS = 1 << 9;

        /// The list is surrounded by `{` and `}`.
        const BRACES = 1 << 10;
        /// The list is surrounded by `(` and `)`.
        const PARENTHESIS = 1 << 11;
        /// The list is surrounded by `<` and `>`.
        const ANGLE_BRACKETS = 1 << 12;
        /// The list is surrounded by `[` and `]`.
        const SQUARE_BRACKETS = 1 << 13;
        /// Mask over the bracket bits.
        const BRACKETS_MASK = (1 << 10) | (1 << 11) | (1 << 12) | (1 << 13);

        /// Do not emit brackets if the list is nil.
        const OPTIONAL_IF_NIL = 1 << 14;
        /// Do not emit brackets if the list is empty.
        const OPTIONAL_IF_EMPTY = 1 << 15;
        /// Do not emit brackets if the list is nil or empty.
        const OPTIONAL = (1 << 14) | (1 << 15);

        /// Prefer adding a line terminator between synthesized nodes.
        const PREFER_NEW_LINE = 1 << 16;
        /// Do not emit a trailing newline for a multi-line list.
        const NO_TRAILING_NEW_LINE = 1 << 17;
        /// Do not emit comments between each node.
        const NO_INTERVENING_COMMENTS = 1 << 18;
        /// If the literal is empty, do not add spaces between braces.
        const NO_SPACE_IF_EMPTY = 1 << 19;
        /// The list has a single element.
        const SINGLE_ELEMENT = 1 << 20;
        /// Add a space after the list.
        const SPACE_AFTER_LIST = 1 << 21;

        /// Single line (explicit alias of `NONE`).
        const SINGLE_LINE = 0;

        // Precomputed formats.
        /// Modifier list.
        const MODIFIERS = (1 << 9) | (1 << 18) | (1 << 21);
        /// Heritage clauses.
        const HERITAGE_CLAUSES = 1 << 9;
        /// Single-line type-literal members.
        const SINGLE_LINE_TYPE_LITERAL_MEMBERS = (1 << 8) | (1 << 9);
        /// Multi-line type-literal members.
        const MULTI_LINE_TYPE_LITERAL_MEMBERS = (1 << 0) | (1 << 7) | (1 << 15);
        /// Single-line tuple-type elements.
        const SINGLE_LINE_TUPLE_TYPE_ELEMENTS = (1 << 4) | (1 << 9);
        /// Multi-line tuple-type elements.
        const MULTI_LINE_TUPLE_TYPE_ELEMENTS = (1 << 4) | (1 << 7) | (1 << 9) | (1 << 0);
        /// Union-type constituents.
        const UNION_TYPE_CONSTITUENTS = (1 << 2) | (1 << 9);
        /// Intersection-type constituents.
        const INTERSECTION_TYPE_CONSTITUENTS = (1 << 3) | (1 << 9);
        /// Object binding-pattern elements.
        const OBJECT_BINDING_PATTERN_ELEMENTS =
            (1 << 6) | (1 << 8) | (1 << 4) | (1 << 9) | (1 << 19);
        /// Array binding-pattern elements.
        const ARRAY_BINDING_PATTERN_ELEMENTS = (1 << 6) | (1 << 4) | (1 << 9) | (1 << 19);
        /// Object-literal expression properties.
        const OBJECT_LITERAL_EXPRESSION_PROPERTIES =
            (1 << 1) | (1 << 4) | (1 << 9) | (1 << 8) | (1 << 7) | (1 << 10) | (1 << 19);
        /// Import attributes.
        const IMPORT_ATTRIBUTES =
            (1 << 1) | (1 << 4) | (1 << 9) | (1 << 8) | (1 << 7) | (1 << 10) | (1 << 19);
        /// Array-literal expression elements.
        const ARRAY_LITERAL_EXPRESSION_ELEMENTS =
            (1 << 1) | (1 << 4) | (1 << 9) | (1 << 6) | (1 << 7) | (1 << 13);
        /// Comma-list elements.
        const COMMA_LIST_ELEMENTS = (1 << 4) | (1 << 9);
        /// Call-expression arguments.
        const CALL_EXPRESSION_ARGUMENTS = (1 << 4) | (1 << 9) | (1 << 11);
        /// New-expression arguments.
        const NEW_EXPRESSION_ARGUMENTS = (1 << 4) | (1 << 9) | (1 << 11) | (1 << 14);
        /// Template-expression spans.
        const TEMPLATE_EXPRESSION_SPANS = 1 << 18;
        /// Single-line block statements.
        const SINGLE_LINE_BLOCK_STATEMENTS = (1 << 8) | (1 << 9);
        /// Multi-line block statements.
        const MULTI_LINE_BLOCK_STATEMENTS = (1 << 7) | (1 << 0);
        /// Variable-declaration list.
        const VARIABLE_DECLARATION_LIST = (1 << 4) | (1 << 9);
        /// Single-line function-body statements.
        const SINGLE_LINE_FUNCTION_BODY_STATEMENTS = (1 << 9) | (1 << 8);
        /// Multi-line function-body statements.
        const MULTI_LINE_FUNCTION_BODY_STATEMENTS = 1 << 0;
        /// Class heritage clauses.
        const CLASS_HERITAGE_CLAUSES = 0;
        /// Class members.
        const CLASS_MEMBERS = (1 << 7) | (1 << 0);
        /// Interface members.
        const INTERFACE_MEMBERS = (1 << 7) | (1 << 0);
        /// Enum members.
        const ENUM_MEMBERS = (1 << 4) | (1 << 7) | (1 << 0);
        /// Case-block clauses.
        const CASE_BLOCK_CLAUSES = (1 << 7) | (1 << 0);
        /// Named imports/exports elements.
        const NAMED_IMPORTS_OR_EXPORTS_ELEMENTS =
            (1 << 4) | (1 << 9) | (1 << 6) | (1 << 8) | (1 << 19);
        /// JSX element/fragment children.
        const JSX_ELEMENT_OR_FRAGMENT_CHILDREN = 1 << 18;
        /// JSX element attributes.
        const JSX_ELEMENT_ATTRIBUTES = (1 << 9) | (1 << 18);
        /// Case/default-clause statements.
        const CASE_OR_DEFAULT_CLAUSE_STATEMENTS = (1 << 7) | (1 << 0) | (1 << 17) | (1 << 15);
        /// Heritage-clause types.
        const HERITAGE_CLAUSE_TYPES = (1 << 4) | (1 << 9);
        /// Source-file statements.
        const SOURCE_FILE_STATEMENTS = (1 << 0) | (1 << 17);
        /// Decorators.
        const DECORATORS = (1 << 0) | (1 << 14) | (1 << 15) | (1 << 21);
        /// Type arguments.
        const TYPE_ARGUMENTS = (1 << 4) | (1 << 9) | (1 << 12) | (1 << 14) | (1 << 15);
        /// Type parameters.
        const TYPE_PARAMETERS = (1 << 4) | (1 << 9) | (1 << 12) | (1 << 14) | (1 << 15);
        /// Parameters.
        const PARAMETERS = (1 << 4) | (1 << 9) | (1 << 11);
        /// Single arrow parameter.
        const SINGLE_ARROW_PARAMETER = (1 << 4) | (1 << 9);
        /// Index-signature parameters.
        const INDEX_SIGNATURE_PARAMETERS = (1 << 4) | (1 << 9) | (1 << 7) | (1 << 13);
    }
}

#[cfg(test)]
#[path = "list_format_test.rs"]
mod tests;
