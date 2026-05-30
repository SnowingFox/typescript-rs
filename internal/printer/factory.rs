//! [`NodeFactory`]: an emit-aware node factory that records auto-generate info.
//!
//! This file currently implements the name factories the name generator and its
//! tests need (`new_temp_variable`, `new_loop_variable`, `new_unique_name`,
//! `new_unique_private_name`). The full set of `New<NodeKind>` constructors and
//! the helper-call/name-resolution factories are added alongside the emit loop.

use crate::emitcontext::{AutoGenerateInfo, AutoGenerateOptions, EmitContext};
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use crate::utilities::format_generated_name;
use tsgo_ast::{Kind, NodeFlags, NodeId, TokenFlags};

/// Combines a kind selector with the non-kind flag bits of `options`.
fn combine_flags(
    kind: GeneratedIdentifierFlags,
    options_flags: GeneratedIdentifierFlags,
) -> GeneratedIdentifierFlags {
    let non_kind = options_flags.bits() & !GeneratedIdentifierFlags::KIND_MASK.bits();
    GeneratedIdentifierFlags::from_bits(kind.bits() | non_kind)
}

/// An emit-aware node factory borrowing an [`EmitContext`].
///
/// Mirrors Go's `printer.NodeFactory`, which embeds `ast.NodeFactory` and writes
/// auto-generate side-table entries back into the emit context.
///
/// # Examples
/// ```
/// use tsgo_printer::emitcontext::EmitContext;
/// let mut ec = EmitContext::new();
/// let t = ec.factory().new_temp_variable();
/// assert!(ec.has_auto_generate_info(t));
/// ```
///
/// Side effects: every `new_*` method appends to the context's arena and records
/// an auto-generate entry.
// Go: internal/printer/factory.go:NodeFactory
pub struct NodeFactory<'a> {
    ctx: &'a mut EmitContext,
}

impl<'a> NodeFactory<'a> {
    /// Wraps a mutable emit context.
    ///
    /// Side effects: none (borrows `ctx`).
    // Go: internal/printer/factory.go:NewNodeFactory
    pub(crate) fn new(ctx: &'a mut EmitContext) -> NodeFactory<'a> {
        NodeFactory { ctx }
    }

    /// Creates an auto-generated identifier and records its auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.newGeneratedIdentifier
    fn new_generated_identifier(
        &mut self,
        kind: GeneratedIdentifierFlags,
        text: &str,
        node: Option<NodeId>,
        options: AutoGenerateOptions,
    ) -> NodeId {
        let id = self.ctx.alloc_auto_generate_id();

        let final_text = if text.is_empty() {
            let base = match node {
                None => format!("(auto@{})", id.0),
                Some(n) if is_member_name(self.ctx.arena().kind(n)) => {
                    self.ctx.arena().text(n).to_string()
                }
                Some(n) => format!("(generated@{})", n.index()),
            };
            format_generated_name(false, &options.prefix, &base, &options.suffix)
        } else {
            text.to_string()
        };

        let name = self.ctx.arena_mut().new_identifier(&final_text);
        self.ctx.arena_mut().add_flags(name, NodeFlags::SYNTHESIZED);
        let info = AutoGenerateInfo {
            id,
            flags: combine_flags(kind, options.flags),
            prefix: options.prefix,
            suffix: options.suffix,
            node,
        };
        self.ctx.set_auto_generate(name, info);
        name
    }

    /// Creates an auto-generated private identifier and records its entry.
    // Go: internal/printer/factory.go:NodeFactory.newGeneratedPrivateIdentifier
    fn new_generated_private_identifier(
        &mut self,
        kind: GeneratedIdentifierFlags,
        text: &str,
        node: Option<NodeId>,
        options: AutoGenerateOptions,
    ) -> NodeId {
        let id = self.ctx.alloc_auto_generate_id();

        let final_text = if text.is_empty() {
            let base = match node {
                None => format!("(auto@{})", id.0),
                Some(n) if is_member_name(self.ctx.arena().kind(n)) => {
                    self.ctx.arena().text(n).to_string()
                }
                Some(n) => format!("(generated@{})", n.index()),
            };
            format_generated_name(true, &options.prefix, &base, &options.suffix)
        } else {
            assert!(
                text.starts_with('#'),
                "First character of private identifier must be #: {text}"
            );
            text.to_string()
        };

        let name = self.ctx.arena_mut().new_private_identifier(&final_text);
        self.ctx.arena_mut().add_flags(name, NodeFlags::SYNTHESIZED);
        let info = AutoGenerateInfo {
            id,
            flags: combine_flags(kind, options.flags),
            prefix: options.prefix,
            suffix: options.suffix,
            node,
        };
        self.ctx.set_auto_generate(name, info);
        name
    }

    /// Allocates a new temp variable name.
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewTempVariable
    pub fn new_temp_variable(&mut self) -> NodeId {
        self.new_temp_variable_ex(AutoGenerateOptions::default())
    }

    /// Allocates a new temp variable name with options.
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewTempVariableEx
    pub fn new_temp_variable_ex(&mut self, options: AutoGenerateOptions) -> NodeId {
        self.new_generated_identifier(GeneratedIdentifierFlags::AUTO, "", None, options)
    }

    /// Allocates a new loop variable name (preferring `_i`).
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewLoopVariable
    pub fn new_loop_variable(&mut self) -> NodeId {
        self.new_loop_variable_ex(AutoGenerateOptions::default())
    }

    /// Allocates a new loop variable name with options.
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewLoopVariableEx
    pub fn new_loop_variable_ex(&mut self, options: AutoGenerateOptions) -> NodeId {
        self.new_generated_identifier(GeneratedIdentifierFlags::LOOP, "", None, options)
    }

    /// Allocates a new unique name based on `text`.
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewUniqueName
    pub fn new_unique_name(&mut self, text: &str) -> NodeId {
        self.new_unique_name_ex(text, AutoGenerateOptions::default())
    }

    /// Allocates a new unique name based on `text` with options.
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewUniqueNameEx
    pub fn new_unique_name_ex(&mut self, text: &str, options: AutoGenerateOptions) -> NodeId {
        self.new_generated_identifier(GeneratedIdentifierFlags::UNIQUE, text, None, options)
    }

    /// Allocates a new unique private name based on `text`.
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewUniquePrivateName
    pub fn new_unique_private_name(&mut self, text: &str) -> NodeId {
        self.new_unique_private_name_ex(text, AutoGenerateOptions::default())
    }

    /// Allocates a new unique private name based on `text` with options.
    ///
    /// Side effects: appends a node and records an auto-generate entry.
    // Go: internal/printer/factory.go:NodeFactory.NewUniquePrivateNameEx
    pub fn new_unique_private_name_ex(
        &mut self,
        text: &str,
        options: AutoGenerateOptions,
    ) -> NodeId {
        self.new_generated_private_identifier(GeneratedIdentifierFlags::UNIQUE, text, None, options)
    }

    /// Marks a freshly created node as synthesized, mirroring Go's
    /// `EmitContext.onCreate` hook fired by the embedded `ast.NodeFactory`.
    ///
    /// Side effects: sets [`NodeFlags::SYNTHESIZED`] on `id`.
    // Go: internal/printer/emitcontext.go:EmitContext.onCreate
    fn on_create(&mut self, id: NodeId) {
        self.ctx.arena_mut().add_flags(id, NodeFlags::SYNTHESIZED);
    }

    /// Creates a synthesized identifier with the given `text`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitcontext::EmitContext;
    /// let mut ec = EmitContext::new();
    /// let id = ec.factory().new_identifier("Infinity");
    /// assert_eq!(ec.arena().text(id), "Infinity");
    /// ```
    ///
    /// Side effects: appends a node and marks it synthesized.
    // Go: internal/ast/ast.go:NodeFactory.NewIdentifier
    pub fn new_identifier(&mut self, text: &str) -> NodeId {
        let id = self.ctx.arena_mut().new_identifier(text);
        self.on_create(id);
        id
    }

    /// Creates an identifier that references an emit helper by name (e.g.
    /// `__setFunctionName`), marking it with the `EmitFlags::HELPER_NAME` emit
    /// flag so module emit can rewrite it to the imported-helpers form when
    /// needed.
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitcontext::EmitContext;
    /// use tsgo_printer::EmitFlags;
    /// let mut ec = EmitContext::new();
    /// let h = ec.factory().new_unscoped_helper_name("__setFunctionName");
    /// assert!(ec.emit_flags(h).contains(EmitFlags::HELPER_NAME));
    /// ```
    ///
    /// Side effects: appends a node, marks it synthesized, and sets its emit flags.
    // Go: internal/printer/factory.go:NodeFactory.NewUnscopedHelperName
    pub fn new_unscoped_helper_name(&mut self, name: &str) -> NodeId {
        let id = self.new_identifier(name);
        self.ctx
            .set_emit_flags(id, crate::emitflags::EmitFlags::HELPER_NAME);
        id
    }

    /// Creates a synthesized string literal with the given `text` and flags.
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitcontext::EmitContext;
    /// use tsgo_ast::TokenFlags;
    /// let mut ec = EmitContext::new();
    /// let s = ec.factory().new_string_literal("hi", TokenFlags::NONE);
    /// assert_eq!(ec.arena().text(s), "hi");
    /// ```
    ///
    /// Side effects: appends a node and marks it synthesized.
    // Go: internal/ast/ast.go:NodeFactory.NewStringLiteral
    pub fn new_string_literal(&mut self, text: &str, token_flags: TokenFlags) -> NodeId {
        let id = self.ctx.arena_mut().new_string_literal(text, token_flags);
        self.on_create(id);
        id
    }

    /// Creates a synthesized numeric literal with the given `text` and flags.
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitcontext::EmitContext;
    /// use tsgo_ast::TokenFlags;
    /// let mut ec = EmitContext::new();
    /// let n = ec.factory().new_numeric_literal("1", TokenFlags::NONE);
    /// assert_eq!(ec.arena().text(n), "1");
    /// ```
    ///
    /// Side effects: appends a node and marks it synthesized.
    // Go: internal/ast/ast.go:NodeFactory.NewNumericLiteral
    pub fn new_numeric_literal(&mut self, text: &str, token_flags: TokenFlags) -> NodeId {
        let id = self.ctx.arena_mut().new_numeric_literal(text, token_flags);
        self.on_create(id);
        id
    }

    /// Creates a synthesized prefix unary expression (`<operator><operand>`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitcontext::EmitContext;
    /// use tsgo_ast::Kind;
    /// let mut ec = EmitContext::new();
    /// let operand = ec.factory().new_identifier("Infinity");
    /// let neg = ec.factory().new_prefix_unary_expression(Kind::MinusToken, operand);
    /// assert_eq!(ec.arena().kind(neg), Kind::PrefixUnaryExpression);
    /// ```
    ///
    /// Side effects: appends a node and marks it synthesized.
    // Go: internal/ast/ast.go:NodeFactory.NewPrefixUnaryExpression
    pub fn new_prefix_unary_expression(&mut self, operator: Kind, operand: NodeId) -> NodeId {
        let id = self
            .ctx
            .arena_mut()
            .new_prefix_unary_expression(operator, operand);
        self.on_create(id);
        id
    }
}

/// Reports whether `kind` is a member name (identifier or private identifier).
// Go: internal/ast/utilities.go:IsMemberName
fn is_member_name(kind: Kind) -> bool {
    matches!(kind, Kind::Identifier | Kind::PrivateIdentifier)
}

#[cfg(test)]
#[path = "factory_test.rs"]
mod tests;
