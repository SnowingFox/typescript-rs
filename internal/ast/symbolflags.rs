//! `SymbolFlags` bit set describing the meaning(s) of a `Symbol`.

bitflags::bitflags! {
    /// Classifies what a `Symbol` is (variable, class, type, alias, ...).
    ///
    /// Mirrors Go `SymbolFlags` (a `uint32` `iota` enum). The trailing `*EXCLUDES`
    /// constants encode merge-conflict masks via AND-NOT of the base unions.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::symbolflags::SymbolFlags;
    /// assert_eq!(SymbolFlags::ALL.bits(), (1 << 30) - 1);
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/symbolflags.go:SymbolFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct SymbolFlags: u32 {
        /// No flags set.
        const NONE = 0;
        /// Variable (`var`) or parameter.
        const FUNCTION_SCOPED_VARIABLE = 1 << 0;
        /// Block-scoped variable (`let` or `const`).
        const BLOCK_SCOPED_VARIABLE = 1 << 1;
        /// Property or enum member.
        const PROPERTY = 1 << 2;
        /// Enum member.
        const ENUM_MEMBER = 1 << 3;
        /// Function.
        const FUNCTION = 1 << 4;
        /// Class.
        const CLASS = 1 << 5;
        /// Interface.
        const INTERFACE = 1 << 6;
        /// Const enum.
        const CONST_ENUM = 1 << 7;
        /// Regular (non-const) enum.
        const REGULAR_ENUM = 1 << 8;
        /// Instantiated module.
        const VALUE_MODULE = 1 << 9;
        /// Uninstantiated module.
        const NAMESPACE_MODULE = 1 << 10;
        /// Type literal or mapped type.
        const TYPE_LITERAL = 1 << 11;
        /// Object literal.
        const OBJECT_LITERAL = 1 << 12;
        /// Method.
        const METHOD = 1 << 13;
        /// Constructor.
        const CONSTRUCTOR = 1 << 14;
        /// Get accessor.
        const GET_ACCESSOR = 1 << 15;
        /// Set accessor.
        const SET_ACCESSOR = 1 << 16;
        /// Call, construct, or index signature.
        const SIGNATURE = 1 << 17;
        /// Type parameter.
        const TYPE_PARAMETER = 1 << 18;
        /// Type alias.
        const TYPE_ALIAS = 1 << 19;
        /// Exported value marker.
        const EXPORT_VALUE = 1 << 20;
        /// An alias for another symbol.
        const ALIAS = 1 << 21;
        /// Prototype property (no source representation).
        const PROTOTYPE = 1 << 22;
        /// `export *` declaration.
        const EXPORT_STAR = 1 << 23;
        /// Optional property.
        const OPTIONAL = 1 << 24;
        /// Transient symbol (created during type checking).
        const TRANSIENT = 1 << 25;
        /// Assignment to a property on a function acting as a declaration.
        const ASSIGNMENT = 1 << 26;
        /// CommonJS `module` of `module.exports`.
        const MODULE_EXPORTS = 1 << 27;
        /// Module contains only const enums or other such modules.
        const CONST_ENUM_ONLY_MODULE = 1 << 28;
        /// Symbol can be replaced by a method.
        const REPLACEABLE_BY_METHOD = 1 << 29;
        /// Flag to signal this is a global lookup.
        const GLOBAL_LOOKUP = 1 << 30;
        /// All flags except `GLOBAL_LOOKUP`.
        const ALL = (1 << 30) - 1;

        /// Any enum kind.
        const ENUM = Self::REGULAR_ENUM.bits() | Self::CONST_ENUM.bits();
        /// Any variable kind.
        const VARIABLE = Self::FUNCTION_SCOPED_VARIABLE.bits() | Self::BLOCK_SCOPED_VARIABLE.bits();
        /// Anything that has a runtime value.
        const VALUE = Self::VARIABLE.bits()
            | Self::PROPERTY.bits()
            | Self::ENUM_MEMBER.bits()
            | Self::OBJECT_LITERAL.bits()
            | Self::FUNCTION.bits()
            | Self::CLASS.bits()
            | Self::ENUM.bits()
            | Self::VALUE_MODULE.bits()
            | Self::METHOD.bits()
            | Self::GET_ACCESSOR.bits()
            | Self::SET_ACCESSOR.bits();
        /// Anything that introduces a type.
        const TYPE = Self::CLASS.bits()
            | Self::INTERFACE.bits()
            | Self::ENUM.bits()
            | Self::ENUM_MEMBER.bits()
            | Self::TYPE_LITERAL.bits()
            | Self::TYPE_PARAMETER.bits()
            | Self::TYPE_ALIAS.bits();
        /// Anything that introduces a namespace.
        const NAMESPACE = Self::VALUE_MODULE.bits() | Self::NAMESPACE_MODULE.bits() | Self::ENUM.bits();
        /// Any module kind.
        const MODULE = Self::VALUE_MODULE.bits() | Self::NAMESPACE_MODULE.bits();
        /// Any accessor kind.
        const ACCESSOR = Self::GET_ACCESSOR.bits() | Self::SET_ACCESSOR.bits();

        /// Merge conflicts for a function-scoped variable.
        const FUNCTION_SCOPED_VARIABLE_EXCLUDES = Self::VALUE.bits() & !Self::FUNCTION_SCOPED_VARIABLE.bits();
        /// Merge conflicts for a block-scoped variable.
        const BLOCK_SCOPED_VARIABLE_EXCLUDES = Self::VALUE.bits();
        /// Merge conflicts for a parameter.
        const PARAMETER_EXCLUDES = Self::VALUE.bits();
        /// Merge conflicts for a property.
        const PROPERTY_EXCLUDES = Self::VALUE.bits() & !(Self::PROPERTY.bits() | Self::ACCESSOR.bits());
        /// Merge conflicts for an enum member.
        const ENUM_MEMBER_EXCLUDES = Self::VALUE.bits() | Self::TYPE.bits();
        /// Merge conflicts for a function.
        const FUNCTION_EXCLUDES = Self::VALUE.bits() & !(Self::FUNCTION.bits() | Self::VALUE_MODULE.bits() | Self::CLASS.bits());
        /// Merge conflicts for a class.
        const CLASS_EXCLUDES = (Self::VALUE.bits() | Self::TYPE.bits()) & !(Self::VALUE_MODULE.bits() | Self::INTERFACE.bits() | Self::FUNCTION.bits());
        /// Merge conflicts for an interface.
        const INTERFACE_EXCLUDES = Self::TYPE.bits() & !(Self::INTERFACE.bits() | Self::CLASS.bits());
        /// Merge conflicts for a regular enum.
        const REGULAR_ENUM_EXCLUDES = (Self::VALUE.bits() | Self::TYPE.bits()) & !(Self::REGULAR_ENUM.bits() | Self::VALUE_MODULE.bits());
        /// Merge conflicts for a const enum.
        const CONST_ENUM_EXCLUDES = (Self::VALUE.bits() | Self::TYPE.bits()) & !Self::CONST_ENUM.bits();
        /// Merge conflicts for a value module.
        const VALUE_MODULE_EXCLUDES = Self::VALUE.bits() & !(Self::FUNCTION.bits() | Self::CLASS.bits() | Self::REGULAR_ENUM.bits() | Self::VALUE_MODULE.bits());
        /// Merge conflicts for a namespace module (none).
        const NAMESPACE_MODULE_EXCLUDES = 0;
        /// Merge conflicts for a method.
        const METHOD_EXCLUDES = Self::VALUE.bits() & !Self::METHOD.bits();
        /// Merge conflicts for a get accessor.
        const GET_ACCESSOR_EXCLUDES = Self::VALUE.bits() & !(Self::SET_ACCESSOR.bits() | Self::PROPERTY.bits());
        /// Merge conflicts for a set accessor.
        const SET_ACCESSOR_EXCLUDES = Self::VALUE.bits() & !(Self::GET_ACCESSOR.bits() | Self::PROPERTY.bits());
        /// Merge conflicts for any accessor.
        const ACCESSOR_EXCLUDES = Self::VALUE.bits() & !Self::PROPERTY.bits();
        /// Merge conflicts for a type parameter.
        const TYPE_PARAMETER_EXCLUDES = Self::TYPE.bits() & !Self::TYPE_PARAMETER.bits();
        /// Merge conflicts for a type alias.
        const TYPE_ALIAS_EXCLUDES = Self::TYPE.bits();
        /// Merge conflicts for an alias.
        const ALIAS_EXCLUDES = Self::ALIAS.bits();

        /// Symbols that may appear as module members.
        const MODULE_MEMBER = Self::VARIABLE.bits()
            | Self::FUNCTION.bits()
            | Self::CLASS.bits()
            | Self::INTERFACE.bits()
            | Self::ENUM.bits()
            | Self::MODULE.bits()
            | Self::TYPE_ALIAS.bits()
            | Self::ALIAS.bits();
        /// Exports that have a local value declaration.
        const EXPORT_HAS_LOCAL = Self::FUNCTION.bits() | Self::CLASS.bits() | Self::ENUM.bits() | Self::VALUE_MODULE.bits();
        /// Block-scoped declarations.
        const BLOCK_SCOPED = Self::BLOCK_SCOPED_VARIABLE.bits() | Self::CLASS.bits() | Self::ENUM.bits();
        /// Property or accessor.
        const PROPERTY_OR_ACCESSOR = Self::PROPERTY.bits() | Self::ACCESSOR.bits();
        /// Class member kinds.
        const CLASS_MEMBER = Self::METHOD.bits() | Self::ACCESSOR.bits() | Self::PROPERTY.bits();
        /// Kinds that support `export default`.
        const EXPORT_SUPPORTS_DEFAULT_MODIFIER = Self::CLASS.bits() | Self::FUNCTION.bits() | Self::INTERFACE.bits();
        /// Kinds that do not support `export default`.
        const EXPORT_DOES_NOT_SUPPORT_DEFAULT_MODIFIER = !(Self::CLASS.bits() | Self::FUNCTION.bits() | Self::INTERFACE.bits());
        /// Semantically classifiable kinds (used to speed up classification).
        const CLASSIFIABLE = Self::CLASS.bits()
            | Self::ENUM.bits()
            | Self::TYPE_ALIAS.bits()
            | Self::INTERFACE.bits()
            | Self::TYPE_PARAMETER.bits()
            | Self::MODULE.bits()
            | Self::ALIAS.bits();
        /// Containers that can hold late-bound members.
        const LATE_BINDING_CONTAINER = Self::CLASS.bits()
            | Self::INTERFACE.bits()
            | Self::TYPE_LITERAL.bits()
            | Self::OBJECT_LITERAL.bits()
            | Self::FUNCTION.bits();
    }
}

#[cfg(test)]
#[path = "symbolflags_test.rs"]
mod tests;
