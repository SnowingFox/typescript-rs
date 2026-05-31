//! [`NameGenerator`]: produces stable auto-generated identifier text.
//!
//! Generates temp (`_a`, `_b`, ...), loop (`_i`), unique (`foo_1`), private
//! (`#foo_1`), and node-based names (e.g. `f_1` for `function f`, `class_1` for
//! a class expression). Auto/loop/unique names are cached by [`AutoGenerateId`];
//! node-based names by the resolved source [`NodeId`].

use crate::emitcontext::{AutoGenerateId, EmitContext};
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use crate::utilities::{ensure_leading_hash, format_generated_name, remove_leading_hash};
use rustc_hash::{FxHashMap, FxHashSet};
use tsgo_ast::{Kind, NodeId};

/// No preferred name.
const TEMP_FLAGS_AUTO: i32 = 0x0000_0000;
/// Mask extracting the temp-variable counter.
const TEMP_FLAGS_COUNT_MASK: i32 = 0x0FFF_FFFF;
/// Preference flag for the `_i` loop name.
const TEMP_FLAGS_I: i32 = 0x1000_0000;

/// A single name-generation scope: a temp-variable counter plus reserved names.
///
/// Side effects: none (pure value type).
// Go: internal/printer/namegenerator.go:nameGenerationScope
#[derive(Default, Debug)]
struct NameGenerationScope {
    temp_flags: i32,
    formatted_name_temp_flags: FxHashMap<String, i32>,
    reserved_names: FxHashSet<String>,
}

/// Produces auto-generated identifier text from an [`EmitContext`]'s
/// auto-generate table, maintaining per-scope temp counters and reserved-name
/// sets.
///
/// # Examples
/// ```
/// use tsgo_printer::emitcontext::EmitContext;
/// use tsgo_printer::namegenerator::NameGenerator;
/// let mut ec = EmitContext::new();
/// let a = ec.factory().new_temp_variable();
/// let b = ec.factory().new_temp_variable();
/// let mut g = NameGenerator::new(&ec);
/// assert_eq!(g.generate_name(a), "_a");
/// assert_eq!(g.generate_name(b), "_b");
/// ```
///
/// Side effects: `generate_name`/`push_scope`/`pop_scope` mutate the generator's
/// caches and scope stacks.
// Go: internal/printer/namegenerator.go:NameGenerator
pub struct NameGenerator<'a> {
    context: &'a EmitContext,
    /// Generated names for specific nodes (node-based names), keyed by the
    /// resolved source node's id.
    node_id_to_generated_name: FxHashMap<NodeId, String>,
    /// Generated private names for specific nodes, keyed by the resolved source
    /// node's id.
    node_id_to_generated_private_name: FxHashMap<NodeId, String>,
    auto_generated_id_to_generated_name: FxHashMap<AutoGenerateId, String>,
    name_generation_scope: Vec<NameGenerationScope>,
    private_name_generation_scope: Vec<NameGenerationScope>,
    generated_names: FxHashSet<String>,
}

impl<'a> NameGenerator<'a> {
    /// Creates a name generator over `context`.
    ///
    /// Side effects: none (borrows `context`).
    pub fn new(context: &'a EmitContext) -> NameGenerator<'a> {
        NameGenerator {
            context,
            node_id_to_generated_name: FxHashMap::default(),
            node_id_to_generated_private_name: FxHashMap::default(),
            auto_generated_id_to_generated_name: FxHashMap::default(),
            name_generation_scope: Vec::new(),
            private_name_generation_scope: Vec::new(),
            generated_names: FxHashSet::default(),
        }
    }

    /// Pushes a new name-generation scope.
    ///
    /// Side effects: pushes scope frames.
    // Go: internal/printer/namegenerator.go:NameGenerator.PushScope
    pub fn push_scope(&mut self, reuse_temp_variable_scope: bool) {
        self.private_name_generation_scope
            .push(NameGenerationScope::default());
        if !reuse_temp_variable_scope {
            self.name_generation_scope
                .push(NameGenerationScope::default());
        }
    }

    /// Pops the current name-generation scope.
    ///
    /// Side effects: pops scope frames.
    // Go: internal/printer/namegenerator.go:NameGenerator.PopScope
    pub fn pop_scope(&mut self, reuse_temp_variable_scope: bool) {
        self.private_name_generation_scope.pop();
        if !reuse_temp_variable_scope {
            self.name_generation_scope.pop();
        }
    }

    /// Generates the text for the auto-generated name node `name`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitcontext::EmitContext;
    /// use tsgo_printer::namegenerator::NameGenerator;
    /// let mut ec = EmitContext::new();
    /// let n = ec.factory().new_unique_name("foo");
    /// let mut g = NameGenerator::new(&ec);
    /// assert_eq!(g.generate_name(n), "foo_1");
    /// ```
    ///
    /// Side effects: updates caches and scope counters.
    // Go: internal/printer/namegenerator.go:NameGenerator.GenerateName
    pub fn generate_name(&mut self, name: NodeId) -> String {
        let ctx = self.context;
        if let Some(auto_generate) = ctx.get_auto_generate_info(name) {
            if auto_generate.flags.is_node() {
                // Node names generate unique names based on their original node
                // and are cached based on that node's id.
                let flags = auto_generate.flags;
                let prefix = auto_generate.prefix.clone();
                let suffix = auto_generate.suffix.clone();
                let is_private = ctx.arena().kind(name) == Kind::PrivateIdentifier;
                let node = ctx.get_node_for_generated_name(name);
                return self
                    .generate_name_for_node_cached(node, is_private, flags, &prefix, &suffix);
            }
            let id = auto_generate.id;
            if let Some(cached) = self.auto_generated_id_to_generated_name.get(&id) {
                return cached.clone();
            }
            let generated = self.make_name(name);
            self.auto_generated_id_to_generated_name
                .insert(id, generated.clone());
            return generated;
        }
        // Fallback mirrors Go `GetTextOfNode`: the node's own text.
        ctx.arena().text(name).to_string()
    }

    /// Generates the actual name for an auto/loop/unique generated name node.
    // Go: internal/printer/namegenerator.go:NameGenerator.makeName
    fn make_name(&mut self, name: NodeId) -> String {
        let ctx = self.context;
        if let Some(auto_generate) = ctx.get_auto_generate_info(name) {
            let flags = auto_generate.flags;
            let prefix = auto_generate.prefix.clone();
            let suffix = auto_generate.suffix.clone();
            let is_private = ctx.arena().kind(name) == Kind::PrivateIdentifier;
            match flags.kind() {
                GeneratedIdentifierFlags::AUTO => {
                    return self.make_temp_variable_name(
                        TEMP_FLAGS_AUTO,
                        flags.is_reserved_in_nested_scopes(),
                        is_private,
                        &prefix,
                        &suffix,
                    );
                }
                GeneratedIdentifierFlags::LOOP => {
                    return self.make_temp_variable_name(
                        TEMP_FLAGS_I,
                        flags.is_reserved_in_nested_scopes(),
                        false,
                        &prefix,
                        &suffix,
                    );
                }
                GeneratedIdentifierFlags::UNIQUE => {
                    let base = ctx.arena().text(name).to_string();
                    return self.make_unique_name(
                        &base,
                        flags.is_optimistic(),
                        flags.is_reserved_in_nested_scopes(),
                        is_private,
                        &prefix,
                        &suffix,
                    );
                }
                _ => {}
            }
        }
        ctx.arena().text(name).to_string()
    }

    /// Returns the cached node-based name for `node`, generating and caching it
    /// on first request. Node-based names are keyed by the resolved node's id so
    /// repeated requests for the same node are stable.
    // Go: internal/printer/namegenerator.go:NameGenerator.generateNameForNodeCached
    fn generate_name_for_node_cached(
        &mut self,
        node: NodeId,
        private_name: bool,
        flags: GeneratedIdentifierFlags,
        prefix: &str,
        suffix: &str,
    ) -> String {
        let cache = if private_name {
            &self.node_id_to_generated_private_name
        } else {
            &self.node_id_to_generated_name
        };
        if let Some(name) = cache.get(&node) {
            return name.clone();
        }
        let name = self.generate_name_for_node(node, private_name, flags, prefix, suffix);
        let cache = if private_name {
            &mut self.node_id_to_generated_private_name
        } else {
            &mut self.node_id_to_generated_name
        };
        cache.insert(node, name.clone());
        name
    }

    /// Generates a unique name derived from the kind and contents of `node`.
    // Go: internal/printer/namegenerator.go:NameGenerator.generateNameForNode
    fn generate_name_for_node(
        &mut self,
        node: NodeId,
        private_name: bool,
        flags: GeneratedIdentifierFlags,
        prefix: &str,
        suffix: &str,
    ) -> String {
        let ctx = self.context;
        match ctx.arena().kind(node) {
            Kind::Identifier | Kind::PrivateIdentifier => {
                let base = ctx.arena().text(node).to_string();
                self.make_unique_name(
                    &base,
                    flags.is_optimistic(),
                    flags.is_reserved_in_nested_scopes(),
                    private_name,
                    prefix,
                    suffix,
                )
            }
            Kind::FunctionDeclaration | Kind::ClassDeclaration => {
                assert!(
                    !private_name && prefix.is_empty() && suffix.is_empty(),
                    "Generated name for a class or function declaration cannot be private and may have neither a prefix nor suffix"
                );
                // Go short-circuits its `g.Context == nil && HasAutoGenerateInfo`
                // guard, so with a context present (always here) it simply
                // recurses into the declaration name when there is one.
                match name_of_declaration_node(ctx.arena(), node) {
                    // Recurse into the declaration name with no prefix/suffix.
                    Some(name) => self.generate_name_for_node(name, false, flags, "", ""),
                    None => self.generate_name_for_export_default(),
                }
            }
            Kind::ExportAssignment => {
                assert!(
                    !private_name && prefix.is_empty() && suffix.is_empty(),
                    "Generated name for an export assignment cannot be private and may have neither a prefix nor suffix"
                );
                self.generate_name_for_export_default()
            }
            Kind::ClassExpression => {
                assert!(
                    !private_name && prefix.is_empty() && suffix.is_empty(),
                    "Generated name for a class expression cannot be private and may have neither a prefix nor suffix"
                );
                self.generate_name_for_class_expression()
            }
            Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor => {
                self.generate_name_for_method_or_accessor(node, private_name, prefix, suffix)
            }
            Kind::ComputedPropertyName => {
                self.make_temp_variable_name(TEMP_FLAGS_AUTO, true, private_name, prefix, suffix)
            }
            _ => self.make_temp_variable_name(TEMP_FLAGS_AUTO, false, private_name, prefix, suffix),
        }
    }

    /// Generates the unique name used for an `export default` declaration.
    // Go: internal/printer/namegenerator.go:NameGenerator.generateNameForExportDefault
    fn generate_name_for_export_default(&mut self) -> String {
        self.make_unique_name("default", false, false, false, "", "")
    }

    /// Generates the unique name used for an anonymous class expression.
    // Go: internal/printer/namegenerator.go:NameGenerator.generateNameForClassExpression
    fn generate_name_for_class_expression(&mut self) -> String {
        self.make_unique_name("class", false, false, false, "", "")
    }

    /// Generates a name for a method or accessor: derived from the member name
    /// when it is an identifier, otherwise a fresh temp name.
    // Go: internal/printer/namegenerator.go:NameGenerator.generateNameForMethodOrAccessor
    fn generate_name_for_method_or_accessor(
        &mut self,
        node: NodeId,
        private_name: bool,
        prefix: &str,
        suffix: &str,
    ) -> String {
        let ctx = self.context;
        if let Some(name) = member_name_of(ctx.arena(), node) {
            if ctx.arena().kind(name) == Kind::Identifier {
                return self.generate_name_for_node_cached(
                    name,
                    private_name,
                    GeneratedIdentifierFlags::NONE,
                    prefix,
                    suffix,
                );
            }
        }
        self.make_temp_variable_name(TEMP_FLAGS_AUTO, false, private_name, prefix, suffix)
    }

    /// Returns the next available temp name (`_a`..`_z`, `_0`, `_1`, ...),
    /// preferring `_i` when `flags` requests it.
    // Go: internal/printer/namegenerator.go:NameGenerator.makeTempVariableName
    fn make_temp_variable_name(
        &mut self,
        flags: i32,
        reserved_in_nested_scopes: bool,
        private_name: bool,
        prefix: &str,
        suffix: &str,
    ) -> String {
        let simple = prefix.is_empty() && suffix.is_empty();
        let mut key = String::new();
        let mut temp_flags = if simple {
            self.get_temp_flags(private_name)
        } else {
            key = format_generated_name(private_name, prefix, "", suffix);
            if private_name {
                key = ensure_leading_hash(&key);
            }
            self.get_temp_flags_for_formatted_name(private_name, &key)
        };

        if flags != 0 && temp_flags & flags == 0 {
            let full_name = format_generated_name(private_name, prefix, "_i", suffix);
            if self.is_unique_name(&full_name, private_name) {
                temp_flags |= flags;
                self.reserve_name(&full_name, private_name, reserved_in_nested_scopes, true);
                if simple {
                    self.set_temp_flags(private_name, temp_flags);
                } else {
                    self.set_temp_flags_for_formatted_name(private_name, &key, temp_flags);
                }
                return full_name;
            }
        }

        loop {
            let count = temp_flags & TEMP_FLAGS_COUNT_MASK;
            temp_flags += 1;
            // Skip over 'i' and 'n'.
            if count != 8 && count != 13 {
                let name = if count < 26 {
                    format!("_{}", (b'a' + count as u8) as char)
                } else {
                    format!("_{}", count - 26)
                };
                let full_name = format_generated_name(private_name, prefix, &name, suffix);
                if self.is_unique_name(&full_name, private_name) {
                    self.reserve_name(&full_name, private_name, reserved_in_nested_scopes, true);
                    if simple {
                        self.set_temp_flags(private_name, temp_flags);
                    } else {
                        self.set_temp_flags_for_formatted_name(private_name, &key, temp_flags);
                    }
                    return full_name;
                }
            }
        }
    }

    /// Returns a name unique within the file by appending `_n`. With `optimistic`,
    /// the first instance uses `base_name` verbatim.
    // Go: internal/printer/namegenerator.go:NameGenerator.makeUniqueName
    fn make_unique_name(
        &mut self,
        base_name: &str,
        optimistic: bool,
        scoped: bool,
        private_name: bool,
        prefix: &str,
        suffix: &str,
    ) -> String {
        let mut base_name = remove_leading_hash(base_name).to_string();
        if optimistic {
            let full_name = format_generated_name(private_name, prefix, &base_name, suffix);
            if self.check_unique_name(&full_name, private_name) {
                self.reserve_name(&full_name, private_name, scoped, false);
                return full_name;
            }
        }

        if !base_name.is_empty() && !base_name.ends_with('_') {
            base_name.push('_');
        }

        let mut i = 1;
        loop {
            let numbered = format!("{base_name}{i}");
            let full_name = format_generated_name(private_name, prefix, &numbered, suffix);
            if self.check_unique_name(&full_name, private_name) {
                self.reserve_name(&full_name, private_name, scoped, false);
                return full_name;
            }
            i += 1;
        }
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.checkUniqueName
    fn check_unique_name(&self, name: &str, private_name: bool) -> bool {
        // The file-level uniqueness callback is wired in a later slice; until then
        // uniqueness is determined solely by reserved/generated name tracking.
        self.is_unique_name(name, private_name)
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.isUniqueName
    fn is_unique_name(&self, name: &str, private_name: bool) -> bool {
        !self.is_reserved_name(name, private_name)
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.isReservedName
    fn is_reserved_name(&self, name: &str, private_name: bool) -> bool {
        // NOTE: matches Strada (global, unscoped), but is known to be incorrect.
        if self.generated_names.contains(name) {
            return true;
        }
        let scopes = self.scope_chain(private_name);
        scopes.iter().any(|s| s.reserved_names.contains(name))
    }

    /// Returns the scope chain (`name` or `private` variant).
    fn scope_chain(&self, private_name: bool) -> &Vec<NameGenerationScope> {
        if private_name {
            &self.private_name_generation_scope
        } else {
            &self.name_generation_scope
        }
    }

    /// Returns a mutable reference to the active scope, creating a root scope if
    /// the chain is empty (mirrors Go's lazy scope creation).
    fn ensure_top_scope(&mut self, private_name: bool) -> &mut NameGenerationScope {
        let chain = if private_name {
            &mut self.private_name_generation_scope
        } else {
            &mut self.name_generation_scope
        };
        if chain.is_empty() {
            chain.push(NameGenerationScope::default());
        }
        chain.last_mut().unwrap()
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.getTempFlags
    fn get_temp_flags(&self, private_name: bool) -> i32 {
        match self.scope_chain(private_name).last() {
            Some(scope) => scope.temp_flags,
            None => TEMP_FLAGS_AUTO,
        }
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.setTempFlags
    fn set_temp_flags(&mut self, private_name: bool, flags: i32) {
        self.ensure_top_scope(private_name).temp_flags = flags;
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.getTempFlagsForFormattedName
    fn get_temp_flags_for_formatted_name(&self, private_name: bool, key: &str) -> i32 {
        match self.scope_chain(private_name).last() {
            Some(scope) => scope
                .formatted_name_temp_flags
                .get(key)
                .copied()
                .unwrap_or(TEMP_FLAGS_AUTO),
            None => TEMP_FLAGS_AUTO,
        }
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.setTempFlagsForFormattedName
    fn set_temp_flags_for_formatted_name(&mut self, private_name: bool, key: &str, flags: i32) {
        self.ensure_top_scope(private_name)
            .formatted_name_temp_flags
            .insert(key.to_string(), flags);
    }

    // Go: internal/printer/namegenerator.go:NameGenerator.reserveName
    fn reserve_name(&mut self, name: &str, private_name: bool, scoped: bool, temp: bool) {
        if private_name || scoped {
            self.ensure_top_scope(private_name)
                .reserved_names
                .insert(name.to_string());
        } else if !temp {
            // NOTE: matches Strada (global, unscoped), but is known to be incorrect.
            self.generated_names.insert(name.to_string());
        }
    }
}

/// Returns the optional name node of a function/class declaration.
///
/// Mirrors the `node.Name()` accessor Go calls in `generateNameForNode`, but is
/// scoped to the declaration kinds the binder-free port handles.
// Go: internal/ast/ast.go:Node.Name
fn name_of_declaration_node(arena: &tsgo_ast::NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        tsgo_ast::NodeData::FunctionDeclaration(d) => d.name,
        tsgo_ast::NodeData::ClassDeclaration(d) | tsgo_ast::NodeData::ClassExpression(d) => d.name,
        _ => None,
    }
}

/// Returns the name node of a method or accessor declaration.
// Go: internal/ast/ast.go:Node.Name
fn member_name_of(arena: &tsgo_ast::NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        tsgo_ast::NodeData::MethodDeclaration(d) => Some(d.name),
        tsgo_ast::NodeData::GetAccessorDeclaration(d)
        | tsgo_ast::NodeData::SetAccessorDeclaration(d) => Some(d.name),
        _ => None,
    }
}

#[cfg(test)]
#[path = "namegenerator_test.rs"]
mod tests;
