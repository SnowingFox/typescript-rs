//! [`NodeFactory`]: an emit-aware node factory that records auto-generate info.
//!
//! This file currently implements the name factories the name generator and its
//! tests need (`new_temp_variable`, `new_loop_variable`, `new_unique_name`,
//! `new_unique_private_name`). The full set of `New<NodeKind>` constructors and
//! the helper-call/name-resolution factories are added alongside the emit loop.

use crate::emitcontext::{AutoGenerateInfo, AutoGenerateOptions, EmitContext};
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use crate::utilities::format_generated_name;
use tsgo_ast::{Kind, NodeFlags, NodeId};

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
}

/// Reports whether `kind` is a member name (identifier or private identifier).
// Go: internal/ast/utilities.go:IsMemberName
fn is_member_name(kind: Kind) -> bool {
    matches!(kind, Kind::Identifier | Kind::PrivateIdentifier)
}

#[cfg(test)]
#[path = "factory_test.rs"]
mod tests;
