//! Emit resolver: the query surface the declaration/JS transformers (Phase 5)
//! ask the checker, ported from `emitresolver.go`.
//!
//! Go's `EmitResolver` holds a back-reference to the checker plus per-node
//! caches and a mutex. Following this crate's ownership model (no back-pointers,
//! single-threaded), [`EmitResolver`] is a lightweight handle whose query
//! methods take the [`BoundProgram`] (and, for type-backed queries, the
//! [`Checker`]) explicitly. [`Checker::get_emit_resolver`] caches one behind a
//! `OnceCell`, mirroring Go's `GetEmitResolver`.
//!
//! 4k ports the AST-structural core (declaration visibility) plus the
//! type-backed serialization entry points that reuse the 4j node builder. The
//! alias/reference/host-dependent queries are deferred (see the per-method
//! `// blocked-by:` notes).

use tsgo_ast::{Kind, ModifierFlags, NodeData, NodeId, SymbolFlags, SymbolId};

use super::declared_types::get_type_of_symbol;
use super::nodebuilder::type_to_string;
use super::program::BoundProgram;
use super::symbols::resolve_name;
use super::symbols_query::get_symbol_of_declaration;
use super::Checker;

/// The checker's emit-time query surface (Go's `EmitResolver`).
///
/// A zero-sized handle in 4k: visibility is computed structurally and the
/// type-backed queries take the [`Checker`] explicitly, so no
/// checker back-reference or interior caches are needed yet.
///
/// # Examples
/// ```
/// use tsgo_checker::Checker;
/// let c = Checker::new();
/// let _resolver = c.get_emit_resolver();
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/emitresolver.go:EmitResolver
#[derive(Clone, Copy, Debug, Default)]
pub struct EmitResolver;

/// The serialized runtime-constructor descriptor for a type-annotation node,
/// the value the legacy-decorator transform (Phase 5) turns into the second
/// argument of `__metadata("design:type", <Ctor>)`.
///
/// Go's `serializeTypeNode` builds the AST expression directly
/// (`s.f.NewIdentifier("Number")` / `s.f.NewVoidZeroExpression()` / ...); this
/// enum *names* that result so the transform can construct it. Each variant
/// maps to exactly one of Go's emitted forms: a global constructor identifier
/// or the `void 0` expression.
///
/// 4at ports the reachable keyword-type subset (see
/// [`EmitResolver::serialize_type_node_for_metadata`]); the type-reference,
/// union/intersection, array and function arms that Go also produces are
/// deferred.
///
/// # Examples
/// ```
/// use tsgo_checker::SerializedTypeNode;
/// // `: number` serializes to the global `Number` constructor.
/// assert_ne!(SerializedTypeNode::Number, SerializedTypeNode::Object);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SerializedTypeNode {
    /// The global `Number` constructor. Go: `s.f.NewIdentifier("Number")`.
    Number,
    /// The global `String` constructor. Go: `s.f.NewIdentifier("String")`.
    String,
    /// The global `Boolean` constructor. Go: `s.f.NewIdentifier("Boolean")`.
    Boolean,
    /// The global `BigInt` constructor. Go: `s.f.NewIdentifier("BigInt")`.
    BigInt,
    /// The global `Symbol` constructor. Go: `s.f.NewIdentifier("Symbol")`.
    Symbol,
    /// The global `Object` constructor — Go's catch-all "anything else"
    /// fallback. Go: `s.f.NewIdentifier("Object")`.
    Object,
    /// The `void 0` expression (the "undefined" serialization).
    /// Go: `s.f.NewVoidZeroExpression()`.
    VoidZero,
}

impl EmitResolver {
    /// Reports whether `node`'s declaration is visible to declaration emit
    /// (Go's `IsDeclarationVisible`).
    ///
    /// 4k implements the module/declaration-emit rule: a top-level declaration
    /// is visible iff it carries the `export` modifier.
    ///
    /// DEFER(phase-4-checker-post): the global-script-file case (a non-exported
    /// declaration is visible in a non-module script), ambient modules, and
    /// member/nested visibility (`isDeclarationVisible(parent)` recursion).
    /// blocked-by: external-module detection + `compiler.Program` (P6).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_declaration_visible(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsDeclarationVisible(104)
    pub fn is_declaration_visible(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        modifier_flags(program.arena(), node).contains(ModifierFlags::EXPORT)
    }

    /// Serializes the type of declaration `node` to its printed form (Go's
    /// `SerializeTypeOfDeclaration`), reusing the 4j node builder.
    ///
    /// Used by the declaration transformer to emit explicit type annotations.
    ///
    /// DEFER(phase-4-checker-post): emit the serialized type as a *node*
    /// (`createTypeOfDeclaration`) rather than a string, plus the
    /// widening/freshening and accessibility tracking the transformer needs.
    /// blocked-by: the full node builder + `SymbolTracker` and `compiler.Program`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> String {
    /// r.serialize_type_of_declaration(c, p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may resolve and cache the declaration's type.
    // Go: internal/checker/emitresolver.go:EmitResolver.SerializeTypeOfDeclaration
    pub fn serialize_type_of_declaration(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> String {
        let ty = match get_symbol_of_declaration(program, node) {
            Some(symbol) => get_type_of_symbol(checker, program, symbol, None),
            None => checker.error_type(),
        };
        type_to_string(checker, program, ty)
    }

    /// Resolves an identifier *use* (`node`, in value position) to the
    /// declaration symbol it references, walking the scope chain from `node`
    /// outward through enclosing block/function/module scopes to the program's
    /// globals (Go's `resolveName` for an expression identifier, via
    /// `getResolvedSymbol`/`checkIdentifier`).
    ///
    /// The innermost matching declaration wins (shadowing): a use inside a
    /// function resolves to that function's local before an outer/global of the
    /// same name. Resolution keeps any symbol whose flags intersect
    /// `Value | Alias`, so an imported binding (an `Alias` symbol) is found in
    /// value position — exactly what [`is_referenced`](Self::is_referenced)
    /// needs.
    ///
    /// DEFER(phase-4-checker-post): the alias's value-ness is approximated by
    /// matching the `Alias` flag directly rather than following the alias to its
    /// target and testing the target's flags (Go's `getSymbolFlags` /
    /// `getSymbol`); full meaning-by-target plus the resolver's special-name and
    /// type-only rules need cross-file alias resolution.
    /// blocked-by: module import/export resolution (`exports.go`) + `MarkLinked`
    /// references (type-vs-value meaning).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> Option<tsgo_ast::SymbolId> {
    /// r.resolve_reference(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/checker.go:Checker.resolveName / getResolvedSymbol
    pub fn resolve_reference(&self, program: &dyn BoundProgram, node: NodeId) -> Option<SymbolId> {
        if program.arena().kind(node) != Kind::Identifier {
            return None;
        }
        let name = program.arena().text(node);
        resolve_name(
            program,
            node,
            name,
            SymbolFlags::VALUE | SymbolFlags::ALIAS,
            false,
            program.globals(),
        )
    }

    /// Returns the *export container* a value-position identifier `node`
    /// resolves to, i.e. the node a use of a top-level exported binding must be
    /// qualified against during emit (Go's `GetReferencedExportContainer`). For
    /// a use of a top-level export of the current module this is the
    /// `SourceFile` node, which the CommonJS transform rewrites into an
    /// `exports.<name>` access.
    ///
    /// 4as ports the reachable single-file subset: resolve the use (with the
    /// `ExportValue | Value | Alias` meaning Go's `getReferencedValueSymbol`
    /// uses), and if it resolves to a top-level *exported* binding of the
    /// current module, return that module's `SourceFile` node. A use of a
    /// non-exported local, or of an inner binding that shadows an export,
    /// resolves to a non-exported symbol and yields `None`. Per Go, an exported
    /// binding that owns a *local* declaration of a non-variable kind
    /// (`ExportHasLocal`: function/class/enum/namespace) is **not** prefixed
    /// (returns `None`) unless `prefix_locals` is set — only exported variables
    /// become `exports.x` use-sites.
    ///
    /// DEFER(phase-4-checker-post): the namespace/enum export containers (Go's
    /// `FindAncestor` over the matching `ModuleDeclaration`/`EnumDeclaration`),
    /// the cross-module UMD-export case (`symbolFile != referenceFile` returns
    /// `None`), the `startInDeclarationContainer` start-point shift for a
    /// module/enum declaration *name*, and merged-symbol canonicalization.
    /// blocked-by: namespace/enum container resolution + cross-module
    /// (`compiler.Program`, P6) + `getMergedSymbol`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> Option<tsgo_ast::NodeId> {
    /// r.get_referenced_export_container(p, n, false)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/binder/referenceresolver.go:referenceResolver.GetReferencedExportContainer
    pub fn get_referenced_export_container(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
        prefix_locals: bool,
    ) -> Option<NodeId> {
        let arena = program.arena();
        // Go: GetReferencedExportContainer takes an `IdentifierNode`; a
        // non-identifier never resolves to a value reference.
        if arena.kind(node) != Kind::Identifier {
            return None;
        }
        // Go: getReferencedValueSymbol resolves with meaning
        // `ExportValue | Value | Alias`. ExportValue is required because a
        // top-level *exported* binding leaves only an `ExportValue` "phantom"
        // local in the module's `locals` (the real symbol lives in the module
        // `exports`, reached via `export_symbol`).
        let name = arena.text(node);
        let mut symbol = resolve_name(
            program,
            node,
            name,
            SymbolFlags::EXPORT_VALUE | SymbolFlags::VALUE | SymbolFlags::ALIAS,
            false,
            program.globals(),
        )?;
        // Go: if symbol.Flags & ExportValue != 0 { symbol = getMergedSymbol(symbol.ExportSymbol) }
        // (getMergedSymbol is identity in the single-file subset).
        if program
            .symbol(symbol)
            .flags
            .contains(SymbolFlags::EXPORT_VALUE)
        {
            let export_symbol = program.symbol(symbol).export_symbol?;
            // Go: if !prefixLocals && exportSymbol.Flags&ExportHasLocal != 0 &&
            // exportSymbol.Flags&Variable == 0 { return nil }. `ExportHasLocal`
            // (function/class/enum/namespace) names a runtime local binding that
            // is referenced unqualified; only exported *variables* are rewritten
            // into `exports.x` use-sites.
            let export_flags = program.symbol(export_symbol).flags;
            if !prefix_locals
                && export_flags.intersects(SymbolFlags::EXPORT_HAS_LOCAL)
                && !export_flags.intersects(SymbolFlags::VARIABLE)
            {
                return None;
            }
            symbol = export_symbol;
        }
        // Go: parentSymbol := getParentOfSymbol(symbol). When the parent is the
        // module (a `ValueModule` whose `ValueDeclaration` is the `SourceFile`),
        // the export container is that source file.
        let parent_symbol = program.symbol(symbol).parent?;
        let parent = program.symbol(parent_symbol);
        if parent.flags.contains(SymbolFlags::VALUE_MODULE) {
            if let Some(value_declaration) = parent.value_declaration {
                if arena.kind(value_declaration) == Kind::SourceFile {
                    return Some(value_declaration);
                }
            }
        }
        None
    }

    /// Reports whether the declaration `node` (e.g. an import specifier / import
    /// clause) is *referenced* anywhere in the file by a value-position
    /// identifier use that resolves to it (Go's `isReferenced` primitive, the
    /// query importElision asks to elide unused imports).
    ///
    /// Scans every identifier in the file, skipping the declaration's own name
    /// node(s), and reports `true` as soon as one resolves (via
    /// [`resolve_reference`](Self::resolve_reference), scope-correct) to the
    /// declaration's symbol. This is the scope-aware replacement for a textual
    /// name-match stand-in: a use shadowed by an inner binding of the same name
    /// is correctly *not* counted as a reference to the outer declaration.
    ///
    /// DEFER(phase-4-checker-post): Go records reference kinds eagerly during
    /// checking (`markLinkedReferences` -> `symbolReferenceLinks.referenceKinds`)
    /// and `isReferenced` is then an O(1) flag read; the type-only-ness split
    /// (a *type*-only use does not keep a value import alive) needs the
    /// type-vs-value meaning of each use site.
    /// blocked-by: `markLinkedReferencesRecursively` + full type-vs-value
    /// meaning resolution.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, decl: tsgo_ast::NodeId) -> bool {
    /// r.is_referenced(p, decl)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/checker.go:Checker.isReferenced(7041)
    pub fn is_referenced(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        let Some(symbol) = get_symbol_of_declaration(program, node) else {
            return false;
        };
        // The declaration's own name node(s) resolve to `symbol` too; exclude
        // them so a binding is not counted as a reference to itself.
        let name_nodes: Vec<NodeId> = program
            .symbol(symbol)
            .declarations
            .iter()
            .filter_map(|&decl| declaration_name(arena, decl))
            .collect();
        for raw in 0..arena.node_count() as u32 {
            let id = NodeId(raw);
            if arena.kind(id) != Kind::Identifier || name_nodes.contains(&id) {
                continue;
            }
            if self.resolve_reference(program, id) == Some(symbol) {
                return true;
            }
        }
        false
    }

    /// Reports whether the alias declaration `node` (e.g. an `export { x }`
    /// specifier) aliases something that is, transitively, a *value* — the
    /// query declaration emit asks to keep value re-exports while eliding
    /// type-only ones (Go's `IsValueAliasDeclaration` /
    /// `isValueAliasDeclarationWorker` + `isAliasResolvedToValue`).
    ///
    /// 4ao ports the reachable single-file subset: for an export specifier the
    /// (property) name is resolved in the local module scope with *value*
    /// meaning, and the specifier is a value alias iff that resolves to a value
    /// symbol. So `function f() {}; export { f }` is a value alias, while
    /// `interface I {}; export { I }` is not. 4ap adds the `export =`
    /// (`ExportAssignment`) arm: an `export = <identifier>` is a value alias iff
    /// the identifier resolves to a value (so `function f() {}; export = f` is,
    /// `interface I {}; export = I` is not), and any non-identifier expression
    /// (e.g. `export = {}`) is kept (Go returns `true`).
    ///
    /// DEFER(phase-4-checker-post): the other alias forms (`import =`/import
    /// clause/namespace import/`export *`) and, crucially, transitive
    /// *cross-module* target value-ness (an export specifier or `export =` that
    /// re-exports an imported binding / entity-name target, `import { x } from
    /// "m"; export { x }`), plus const-enum/`preserveConstEnums` and
    /// type-only-ness.
    /// blocked-by: module import/export resolution (`exports.go`,
    /// `resolveExternalModuleSymbol`, `getExportSymbolOfValueSymbolIfExported`)
    /// + type-only meaning (`markLinkedReferences`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_value_alias_declaration(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/emitresolver.go:EmitResolver.isValueAliasDeclarationWorker(718)
    pub fn is_value_alias_declaration(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let arena = program.arena();
        match arena.data(node) {
            // Go: isValueAliasDeclarationWorker -> ExportSpecifier branch ->
            // isAliasResolvedToValue. Reachable subset: the alias resolves to a
            // value iff the (property) name resolves to a value symbol in the
            // local module scope.
            NodeData::ImportSpecifier(d) | NodeData::ExportSpecifier(d) => {
                let name = d.property_name.unwrap_or(d.name);
                resolve_name(
                    program,
                    name,
                    arena.text(name),
                    SymbolFlags::VALUE,
                    false,
                    program.globals(),
                )
                .is_some()
            }
            // Go: isValueAliasDeclarationWorker -> ExportAssignment branch. When
            // the exported expression is an identifier, the assignment is a
            // value alias iff that identifier resolves to a value; any other
            // expression (e.g. `export = {}`) is kept (returns true). Reachable
            // subset: resolve the expression name with *value* meaning in the
            // local module scope (same single-file stand-in as the specifier
            // arm above).
            NodeData::ExportAssignment(d) => {
                if arena.kind(d.expression) == Kind::Identifier {
                    resolve_name(
                        program,
                        d.expression,
                        arena.text(d.expression),
                        SymbolFlags::VALUE,
                        false,
                        program.globals(),
                    )
                    .is_some()
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    /// Reports whether the alias declaration `node` (e.g. an import specifier,
    /// `import =` / `export =`) is *referenced* and so must be kept by emit
    /// (Go's `IsReferencedAliasDeclaration`).
    ///
    /// 4ao ports the reachable referenced-check: `node` must be an alias symbol
    /// declaration, and it is referenced iff some value-position use resolves to
    /// it (reusing 4an's [`is_referenced`](Self::is_referenced), the scope-aware
    /// stand-in for Go's eagerly-recorded `aliasSymbolLinks.referenced`).
    ///
    /// DEFER(phase-4-checker-post): Go's "exported alias whose target is a
    /// value" branch (`target != nil && export modifier && getSymbolFlags(target)
    /// & Value`) keeps an unreferenced *exported* alias alive; plus the eager
    /// `referenced` flag and the const-enum/`preserveConstEnums` carve-out.
    /// blocked-by: alias target resolution (`resolveAlias` cross-module) +
    /// `getSymbolFlags` + `markLinkedReferences`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_referenced_alias_declaration(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsReferencedAliasDeclaration(680)
    pub fn is_referenced_alias_declaration(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> bool {
        // Go: IsReferencedAliasDeclaration only considers alias symbol
        // declarations; anything else is not.
        if !is_alias_symbol_declaration(program.arena(), node) {
            return false;
        }
        self.is_referenced(program, node)
    }

    /// Reports whether `node` is the implementation of a set of overloads
    /// (Go's `IsImplementationOfOverload`): a body-bearing function whose symbol
    /// has more than one declaration.
    ///
    /// DEFER(phase-4-checker-post): methods/constructors and the single-signature
    /// case where the lone signature comes from a different declaration; get/set
    /// accessors (never overload implementations).
    /// blocked-by: `getSignaturesOfSymbol` over all declarations.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_implementation_of_overload(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsImplementationOfOverload(458)
    pub fn is_implementation_of_overload(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let has_body = matches!(
            program.arena().data(node),
            NodeData::FunctionDeclaration(d) if d.body.is_some()
        );
        if !has_body {
            return false;
        }
        match get_symbol_of_declaration(program, node) {
            Some(symbol) => program.symbol(symbol).declarations.len() > 1,
            None => false,
        }
    }

    /// Serializes a *type-annotation node* to the runtime-constructor descriptor
    /// the legacy-decorator transform emits for `__metadata("design:type", ..)`
    /// (Go's `serializeTypeNode`).
    ///
    /// 4at ports the reachable keyword-type subset, faithful to Go's switch:
    /// `number` → [`Number`](SerializedTypeNode::Number), `string` →
    /// [`String`](SerializedTypeNode::String), `boolean` →
    /// [`Boolean`](SerializedTypeNode::Boolean), `bigint` →
    /// [`BigInt`](SerializedTypeNode::BigInt), `symbol` →
    /// [`Symbol`](SerializedTypeNode::Symbol), `void`/`undefined`/`never` and a
    /// `null` literal type → [`VoidZero`](SerializedTypeNode::VoidZero), and
    /// `any`/`unknown`/`object` (plus Go's catch-all) →
    /// [`Object`](SerializedTypeNode::Object). 4au extends the same switch with
    /// the structural arms that reuse the existing variants: Go's leading
    /// `SkipTypeParentheses` (`(T)` unwraps to its inner type), the
    /// `TemplateLiteralType` arm (→ `String`, grouped with `string`), and the
    /// non-`null` literal-type arms (`serializeLiteralOfLiteralTypeNode`):
    /// string literal → `String`, numeric literal → `Number`, bigint literal →
    /// `BigInt`, `true`/`false` → `Boolean`, and a negated numeric/bigint
    /// literal (`-1`) recurses on its operand.
    ///
    /// DEFER(phase-5): the non-keyword arms Go also handles — a `TypeReference`
    /// to a value-having entity (`Date`/a class → that entity's constructor, via
    /// `GetTypeReferenceSerializationKind` + symbol/value resolution),
    /// union/intersection/conditional recursion, and `TypePredicate`. These
    /// currently fall to the conservative [`Object`](SerializedTypeNode::Object)
    /// tail rather than their Go-specific result.
    /// blocked-by: `GetTypeReferenceSerializationKind` (entity value-ness +
    /// `printer.TypeReferenceSerializationKind`) + the `serializeTypeNode`
    /// recursion, which the P5 decorator transform drives.
    ///
    /// DEFER: `FunctionType`/`ConstructorType` → `Function` and
    /// `ArrayType`/`TupleType` → `Array` are *not* ported here because they
    /// require new [`SerializedTypeNode`] variants (`Function`/`Array`), and the
    /// P5 `tsgo_transformers` `serialized_type_to_expression` matches this enum
    /// *exhaustively with no wildcard* — adding variants breaks
    /// `cargo build -p tsgo_compiler`. They must land in a lane that may also
    /// extend that transformer match.
    /// blocked-by: a coordinated checker+transformers change adding the
    /// `Function`/`Array` variants and their transformer arms.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver, SerializedTypeNode};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, type_node: tsgo_ast::NodeId) -> SerializedTypeNode {
    /// r.serialize_type_node_for_metadata(p, type_node)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode
    pub fn serialize_type_node_for_metadata(
        &self,
        program: &dyn BoundProgram,
        type_node: NodeId,
    ) -> SerializedTypeNode {
        // Go: `node = ast.SkipTypeParentheses(node)` (run before the switch) —
        // `SkipTypeParentheses` loops `for IsParenthesizedTypeNode(node) { node =
        // node.Type() }`, so `(T)` (and `((T))`) dispatches on its inner type.
        let mut type_node = type_node;
        while let NodeData::ParenthesizedType(d) = program.arena().data(type_node) {
            type_node = d.type_node;
        }
        match program.arena().kind(type_node) {
            // Go: case KindVoidKeyword, KindUndefinedKeyword, KindNeverKeyword
            // -> NewVoidZeroExpression.
            Kind::VoidKeyword | Kind::UndefinedKeyword | Kind::NeverKeyword => {
                SerializedTypeNode::VoidZero
            }
            // Go: case KindNumberKeyword -> NewIdentifier("Number").
            Kind::NumberKeyword => SerializedTypeNode::Number,
            // Go: case KindBigIntKeyword -> NewIdentifier("BigInt").
            Kind::BigIntKeyword => SerializedTypeNode::BigInt,
            // Go: case KindSymbolKeyword -> NewIdentifier("Symbol").
            Kind::SymbolKeyword => SerializedTypeNode::Symbol,
            // Go: case KindTemplateLiteralType, KindStringKeyword -> NewIdentifier("String").
            Kind::TemplateLiteralType | Kind::StringKeyword => SerializedTypeNode::String,
            // Go: case KindBooleanKeyword -> NewIdentifier("Boolean").
            Kind::BooleanKeyword => SerializedTypeNode::Boolean,
            // Go: case KindLiteralType ->
            // serializeLiteralOfLiteralTypeNode(node.AsLiteralTypeNode().Literal).
            Kind::LiteralType => match program.arena().data(type_node) {
                NodeData::LiteralType(d) => {
                    serialize_literal_of_literal_type_node(program, d.literal)
                }
                _ => SerializedTypeNode::Object,
            },
            // Go: the `serializeTypeNode` switch tail
            // (`return s.f.NewIdentifier("Object")`) — the catch-all for
            // everything not matched by a specific arm.
            _ => SerializedTypeNode::Object,
        }
    }
}

// Returns the "name" identifier node of declaration `node`, for the declaration
// kinds whose name must be excluded when scanning for references to it.
// Go: internal/ast/utilities.go:getNameOfDeclaration (subset)
fn declaration_name(arena: &tsgo_ast::NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::ImportSpecifier(d) | NodeData::ExportSpecifier(d) => Some(d.name),
        NodeData::NamespaceImport(d) => Some(d.name),
        NodeData::ImportClause(d) => d.name,
        // Go: an `import x = require("m")` / `import x = ns` binds the identifier
        // `x`; its own name must be excluded so the import-equals is not counted
        // as a reference to itself (`node.Name()` for an import-equals).
        NodeData::ImportEqualsDeclaration(d) => Some(d.name),
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::FunctionDeclaration(d) => d.name,
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.name,
        _ => None,
    }
}

// Reports whether `node` is a declaration that introduces an alias symbol
// (import/export specifier, namespace import/export, `import =`, an import
// clause with a default-binding name, etc.).
//
// 4ao ports the reachable structural kinds; the JS-only require-initialized
// variable/binding-element and CommonJS `module.exports =` binary-expression
// forms, plus the `export =` `ExpressionIsAlias` discrimination, are deferred.
// Go: internal/ast/utilities.go:IsAliasSymbolDeclaration
fn is_alias_symbol_declaration(arena: &tsgo_ast::NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ImportEqualsDeclaration(_)
        | NodeData::NamespaceExportDeclaration(_)
        | NodeData::NamespaceImport(_)
        | NodeData::NamespaceExport(_)
        | NodeData::ImportSpecifier(_)
        | NodeData::ExportSpecifier(_) => true,
        NodeData::ImportClause(d) => d.name.is_some(),
        _ => false,
    }
}

// Returns the aggregated modifier flags of `node`, if it bears modifiers.
// Go: internal/ast/ast.go:Node.ModifierFlags
fn modifier_flags(arena: &tsgo_ast::NodeArena, node: NodeId) -> ModifierFlags {
    let modifiers = match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers
        .map(|m| m.modifier_flags)
        .unwrap_or(ModifierFlags::empty())
}

// Serializes the `literal` expression of a `LiteralType` node (the literal `"a"`
// / `1` / `true` / `null` inside `: "a"` / `: 1` / `: true` / `: null`) to the
// runtime-constructor descriptor Go's `serializeLiteralOfLiteralTypeNode`
// emits.
//
// DEFER(phase-5): the `KindNoSubstitutionTemplateLiteral -> String` literal arm
// is not reachable here — the Rust parser's `parseNonArrayType` does not yet
// route a no-substitution template (`` `abc` ``) type to `parseLiteralTypeNode`
// (it falls to a type reference), so this port omits that arm until the parser
// gap is closed. The conservative `Object` tail covers any other literal kind.
// blocked-by: parser `parseNonArrayType` `NoSubstitutionTemplateLiteral` arm.
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeLiteralOfLiteralTypeNode
fn serialize_literal_of_literal_type_node(
    program: &dyn BoundProgram,
    literal: NodeId,
) -> SerializedTypeNode {
    match program.arena().kind(literal) {
        // Go: case KindStringLiteral, KindNoSubstitutionTemplateLiteral ->
        // NewIdentifier("String").
        Kind::StringLiteral => SerializedTypeNode::String,
        // Go: case KindNumericLiteral -> NewIdentifier("Number").
        Kind::NumericLiteral => SerializedTypeNode::Number,
        // Go: case KindBigIntLiteral -> NewIdentifier("BigInt").
        Kind::BigIntLiteral => SerializedTypeNode::BigInt,
        // Go: case KindTrueKeyword, KindFalseKeyword -> NewIdentifier("Boolean").
        Kind::TrueKeyword | Kind::FalseKeyword => SerializedTypeNode::Boolean,
        // Go: case KindPrefixUnaryExpression { operand := node.Operand; switch
        // operand.Kind { case KindNumericLiteral, KindBigIntLiteral ->
        // serializeLiteralOfLiteralTypeNode(operand) } } — a negative
        // numeric/bigint literal type (`-1` / `-1n`) recurses on the operand.
        Kind::PrefixUnaryExpression => match program.arena().data(literal) {
            NodeData::PrefixUnaryExpression(d)
                if matches!(
                    program.arena().kind(d.operand),
                    Kind::NumericLiteral | Kind::BigIntLiteral
                ) =>
            {
                serialize_literal_of_literal_type_node(program, d.operand)
            }
            // Go default: debug.FailBadSyntaxKind(operand) — conservative tail.
            _ => SerializedTypeNode::Object,
        },
        // Go: case KindNullKeyword -> NewVoidZeroExpression.
        Kind::NullKeyword => SerializedTypeNode::VoidZero,
        // Go default: debug.FailBadSyntaxKind — the conservative `Object` tail.
        _ => SerializedTypeNode::Object,
    }
}

#[cfg(test)]
#[path = "emit_resolver_test.rs"]
mod tests;
