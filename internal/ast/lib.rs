//! `tsgo_ast` — 1:1 Rust port of Go `internal/ast`.
//!
//! # Ownership model (read this first)
//!
//! Go's AST is an arena of nodes wired together with raw `*Node` pointers,
//! including a back-pointer to each node's `Parent` and binder-time mutation.
//! That shape (cyclic, aliased, mutable) cannot be expressed with Rust `&`/`Box`
//! without `unsafe`. We therefore follow the rust-analyzer / swc approach:
//!
//! - A single `NodeArena` owns every `Node` in a `Vec<Node>`.
//! - Every node reference — children, `parent`, and `NodeList` elements — is a
//!   [`NodeId`] (a `u32` index), never a Rust reference.
//! - Go's `data nodeData` interface becomes the `NodeData` enum, dispatched by
//!   `match` instead of dynamic dispatch.
//! - `Kind` and `NodeData` are stored separately: Go's `Kind` is many-to-one with
//!   the data struct (one `Token` data serves ~150 kinds), so collapsing `Kind`
//!   into the enum discriminant would lose information.
//! - Flag families (`NodeFlags`/`SymbolFlags`/...) use the `bitflags` crate.
//! - The `Symbol` graph uses a parallel [`SymbolId`] index space.
//!
//! This is a deliberate, structure-preserving deviation from Go's pointer syntax
//! (`node.Parent` becomes `arena.parent(id)`); it keeps the crate 100% safe Rust
//! (zero `unsafe`).

pub mod checkflags;
pub mod deepclone;
pub mod flow;
pub mod functionflags;
pub mod ids;
pub mod kind_generated;
pub mod modifierflags;
pub mod nodeflags;
pub mod positionmap;
pub mod precedence;
pub mod subtreefacts;
pub mod symbol;
pub mod symbolflags;
pub mod tokenflags;
pub mod utilities;
pub mod visitor;

pub use checkflags::CheckFlags;
pub use flow::FlowFlags;
pub use functionflags::FunctionFlags;
pub use ids::{NodeId, SymbolId};
pub use kind_generated::Kind;
pub use modifierflags::ModifierFlags;
pub use nodeflags::NodeFlags;
pub use positionmap::{compute_position_map, PositionMap};
pub use precedence::{OperatorPrecedence, OperatorPrecedenceFlags};
pub use subtreefacts::SubtreeFacts;
pub use symbol::{Symbol, SymbolTable};
pub use symbolflags::SymbolFlags;
pub use tokenflags::TokenFlags;
pub use visitor::VisitOptions;

use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::text::TextRange;

/// An ordered list of node references, replacing Go's `NodeList{ Loc, Nodes []*Node }`.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:NodeList
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeList {
    /// Source range spanning the list.
    pub loc: TextRange,
    /// Element node ids, in source order.
    pub nodes: Vec<NodeId>,
}

impl NodeList {
    /// Creates an undefined-range list over `nodes`.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:NodeFactory.NewNodeList
    pub fn new(nodes: Vec<NodeId>) -> NodeList {
        NodeList {
            loc: TextRange::undefined(),
            nodes,
        }
    }

    /// Returns the list's start offset.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:NodeList.Pos
    pub fn pos(&self) -> i32 {
        self.loc.pos()
    }

    /// Returns the list's end offset.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:NodeList.End
    pub fn end(&self) -> i32 {
        self.loc.end()
    }
}

/// A modifier list: a [`NodeList`] paired with the union of its modifier flags.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:ModifierList
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModifierList {
    /// The underlying node list (decorators and modifier keywords).
    pub list: NodeList,
    /// Union of modifier flags across the list.
    pub modifier_flags: ModifierFlags,
}

/// Header + typed payload for one AST node, replacing Go's `ast.Node`.
///
/// The `parent` back-edge and all child references are [`NodeId`] indices into
/// the owning [`NodeArena`], so the graph (including cycles via `parent`) is
/// expressed without `unsafe`.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:Node
#[derive(Clone, Debug)]
pub struct Node {
    /// The node's syntax kind.
    pub kind: Kind,
    /// Per-node flags.
    pub flags: NodeFlags,
    /// Source range.
    pub loc: TextRange,
    /// Back-edge to the parent node, set after parsing (`None` until then).
    pub parent: Option<NodeId>,
    /// The typed payload selecting which concrete node this is.
    pub data: NodeData,
}

/// The typed payload of a [`Node`], replacing Go's `data nodeData` interface.
///
/// This phase implements a representative spread of variants (leaves, single
/// and multi-child expressions, and list-bearing nodes); the full set of ~193
/// variants is generated in a later phase.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:nodeData
#[derive(Clone, Debug)]
pub enum NodeData {
    /// A bare token (no payload); serves the ~150 punctuation/keyword kinds.
    Token,
    /// An identifier (`foo`).
    Identifier(IdentifierData),
    /// A private identifier (`#foo`).
    PrivateIdentifier(TextData),
    /// A string literal.
    StringLiteral(LiteralData),
    /// A numeric literal.
    NumericLiteral(LiteralData),
    /// A big-int literal.
    BigIntLiteral(LiteralData),
    /// A keyword expression (`this`, `super`, `null`, `true`, `false`, `import`).
    KeywordExpression,
    /// A dotted name (`a.b`).
    QualifiedName(QualifiedNameData),
    /// A property access (`a.b`, `a?.b`).
    PropertyAccessExpression(PropertyAccessData),
    /// An element access (`a[b]`, `a?.[b]`).
    ElementAccessExpression(ElementAccessData),
    /// A call (`a()`, `a?.()`).
    CallExpression(CallExpressionData),
    /// A `new` expression.
    NewExpression(NewExpressionData),
    /// A parenthesized expression.
    ParenthesizedExpression(UnaryChildData),
    /// A prefix unary expression (`!a`, `++a`).
    PrefixUnaryExpression(UnaryOperatorData),
    /// A postfix unary expression (`a++`).
    PostfixUnaryExpression(UnaryOperatorData),
    /// A binary expression (`a + b`).
    BinaryExpression(BinaryExpressionData),
    /// A spread element (`...a`).
    SpreadElement(UnaryChildData),
    /// An array literal (`[a, b]`).
    ArrayLiteralExpression(ListData),
    /// A block (`{ ... }`).
    Block(ListData),
    /// An expression statement.
    ExpressionStatement(UnaryChildData),
    /// A `return` statement (optional argument).
    ReturnStatement(OptionalChildData),
    /// An empty statement (`;`).
    EmptyStatement,
    /// A `throw` statement (`throw e;`).
    ThrowStatement(UnaryChildData),
    /// An `if`/`else` statement.
    IfStatement(IfStatementData),
    /// A `do ... while` statement.
    DoStatement(DoStatementData),
    /// A `while` statement.
    WhileStatement(WhileStatementData),
    /// A `with` statement.
    WithStatement(WithStatementData),
    /// A `switch` statement.
    SwitchStatement(SwitchStatementData),
    /// The `{ ... }` block of a `switch` statement.
    CaseBlock(CaseBlockData),
    /// A `case`/`default` clause of a `switch` statement.
    CaseOrDefaultClause(CaseOrDefaultClauseData),
    /// A `break` statement (optional label).
    BreakStatement(LabelData),
    /// A `continue` statement (optional label).
    ContinueStatement(LabelData),
    /// A labeled statement (`label: statement`).
    LabeledStatement(LabeledStatementData),
    /// A `debugger` statement.
    DebuggerStatement,
    /// A `var`/`let`/`const` statement.
    VariableStatement(VariableStatementData),
    /// A variable declaration list (the `let x = 1, y` part).
    VariableDeclarationList(VariableDeclarationListData),
    /// A single variable declaration (`x: T = init`).
    VariableDeclaration(VariableDeclarationData),
    /// An object binding pattern (`{ a, b }`).
    ObjectBindingPattern(BindingPatternData),
    /// An array binding pattern (`[ a, b ]`).
    ArrayBindingPattern(BindingPatternData),
    /// A binding element within a binding pattern.
    BindingElement(BindingElementData),
    /// An omitted array element (`[, a]`).
    OmittedExpression,
    /// A computed property name (`[expr]`).
    ComputedPropertyName(UnaryChildData),
    /// A C-style `for` statement.
    ForStatement(ForStatementData),
    /// A `for-in` or `for-of` statement.
    ForInOrOfStatement(ForInOrOfStatementData),
    /// A `try`/`catch`/`finally` statement.
    TryStatement(TryStatementData),
    /// A `catch` clause.
    CatchClause(CatchClauseData),
    /// A function declaration.
    FunctionDeclaration(Box<FunctionDeclarationData>),
    /// A function parameter.
    ParameterDeclaration(Box<ParameterDeclarationData>),
    /// A generic type parameter (`<T extends C = D>`).
    TypeParameterDeclaration(Box<TypeParameterData>),
    /// A class declaration.
    ClassDeclaration(Box<ClassLikeData>),
    /// A class expression.
    ClassExpression(Box<ClassLikeData>),
    /// A heritage clause (`extends`/`implements`).
    HeritageClause(HeritageClauseData),
    /// An expression with type arguments (heritage element `B<T>`).
    ExpressionWithTypeArguments(ExprWithTypeArgsData),
    /// A method declaration / class method.
    MethodDeclaration(Box<MethodLikeData>),
    /// A class property declaration.
    PropertyDeclaration(Box<PropertyDeclarationData>),
    /// A `get` accessor declaration.
    GetAccessorDeclaration(Box<AccessorData>),
    /// A `set` accessor declaration.
    SetAccessorDeclaration(Box<AccessorData>),
    /// A class constructor declaration.
    ConstructorDeclaration(Box<ConstructorData>),
    /// An index signature declaration (`[k: string]: T`).
    IndexSignatureDeclaration(IndexSignatureData),
    /// A class static block (`static { ... }`).
    ClassStaticBlockDeclaration(ClassStaticBlockData),
    /// A lone `;` class element.
    SemicolonClassElement,
    /// An interface declaration.
    InterfaceDeclaration(Box<ClassLikeData>),
    /// A type alias declaration (`type X = ...`).
    TypeAliasDeclaration(Box<TypeAliasData>),
    /// An enum declaration.
    EnumDeclaration(Box<EnumDeclData>),
    /// An enum member.
    EnumMember(EnumMemberData),
    /// A property signature (interface/type-literal member).
    PropertySignature(Box<PropertyDeclarationData>),
    /// A method signature (interface/type-literal member).
    MethodSignature(Box<MethodSignatureData>),
    /// A call signature member.
    CallSignature(SignatureDeclData),
    /// A construct signature member.
    ConstructSignature(SignatureDeclData),
    /// A type literal (`{ members }` type).
    TypeLiteral(TypeLiteralData),
    /// A `module`/`namespace`/`global` declaration.
    ModuleDeclaration(Box<ModuleDeclData>),
    /// A `{ ... }` module/namespace body block.
    ModuleBlock(ModuleBlockData),
    /// An `import ... from "..."` declaration.
    ImportDeclaration(Box<ImportDeclData>),
    /// The clause of an import declaration (default + named bindings).
    ImportClause(ImportClauseData),
    /// A namespace import (`* as ns`).
    NamespaceImport(NameRefData),
    /// A named-imports clause (`{ a, b as c }`).
    NamedImports(ElementsData),
    /// An import specifier (`a`, `a as b`).
    ImportSpecifier(Box<ImportExportSpecData>),
    /// An external module reference (`require("m")`).
    ExternalModuleReference(UnaryChildData),
    /// An `import x = ...` declaration.
    ImportEqualsDeclaration(Box<ImportEqualsData>),
    /// An `export ... from "..."` / `export { ... }` declaration.
    ExportDeclaration(Box<ExportDeclData>),
    /// A named-exports clause (`{ a, b as c }`).
    NamedExports(ElementsData),
    /// A namespace export (`* as ns`).
    NamespaceExport(NameRefData),
    /// An export specifier (`a`, `a as b`).
    ExportSpecifier(Box<ImportExportSpecData>),
    /// An `export =` / `export default` assignment.
    ExportAssignment(Box<ExportAssignData>),
    /// An `export as namespace X` declaration.
    NamespaceExportDeclaration(NamespaceExportDeclData),
    /// An object literal expression (`{ a, b: 1, ...c }`).
    ObjectLiteralExpression(ListData),
    /// A property assignment (`a: value`).
    PropertyAssignment(Box<PropertyDeclarationData>),
    /// A shorthand property assignment (`a` / `a = init`).
    ShorthandPropertyAssignment(Box<ShorthandPropertyAssignmentData>),
    /// A spread assignment (`...expr`).
    SpreadAssignment(UnaryChildData),
    /// A function expression.
    FunctionExpression(Box<FunctionDeclarationData>),
    /// A `delete` expression.
    DeleteExpression(UnaryChildData),
    /// A `typeof` expression.
    TypeOfExpression(UnaryChildData),
    /// A `void` expression.
    VoidExpression(UnaryChildData),
    /// An `await` expression.
    AwaitExpression(UnaryChildData),
    /// A `yield` expression.
    YieldExpression(YieldExpressionData),
    /// An arrow function (`(a) => b`).
    ArrowFunction(Box<ArrowFunctionData>),
    /// An `expr as Type` expression.
    AsExpression(ExprTypeData),
    /// An `expr satisfies Type` expression.
    SatisfiesExpression(ExprTypeData),
    /// A `<Type>expr` type assertion expression.
    TypeAssertionExpression(TypeAssertionData),
    /// A non-null assertion (`expr!`).
    NonNullExpression(UnaryChildData),
    /// A decorator (`@expr`).
    Decorator(UnaryChildData),
    /// An import attributes clause (`with { ... }` / `assert { ... }`).
    ImportAttributes(ImportAttributesData),
    /// A single import attribute (`name: value`).
    ImportAttribute(ImportAttributeData),
    /// A JSX element (`<a>...</a>`).
    JsxElement(JsxElementData),
    /// A self-closing JSX element (`<a />`).
    JsxSelfClosingElement(Box<JsxOpeningLikeData>),
    /// A JSX opening element (`<a>`).
    JsxOpeningElement(Box<JsxOpeningLikeData>),
    /// A JSX closing element (`</a>`).
    JsxClosingElement(JsxClosingElementData),
    /// A JSX fragment (`<>...</>`).
    JsxFragment(JsxElementData),
    /// A JSX opening fragment (`<>`).
    JsxOpeningFragment,
    /// A JSX closing fragment (`</>`).
    JsxClosingFragment,
    /// A JSX attributes list.
    JsxAttributes(ListData),
    /// A JSX attribute (`name={value}`).
    JsxAttribute(JsxAttributeData),
    /// A JSX spread attribute (`{...expr}`).
    JsxSpreadAttribute(UnaryChildData),
    /// A JSX namespaced name (`ns:name`).
    JsxNamespacedName(JsxNamespacedNameData),
    /// A JSX expression container (`{expr}`).
    JsxExpression(JsxExpressionData),
    /// JSX text.
    JsxText(JsxTextData),
    /// A meta-property (`new.target`, `import.meta`).
    MetaProperty(MetaPropertyData),
    /// A template expression (`` `a${b}c` ``).
    TemplateExpression(TemplateExpressionData),
    /// A template span (`${expr}literal`).
    TemplateSpan(TemplateSpanData),
    /// A tagged template expression (`` tag`...` ``).
    TaggedTemplateExpression(Box<TaggedTemplateData>),
    /// A regular-expression literal.
    RegularExpressionLiteral(LiteralData),
    /// A no-substitution template literal.
    NoSubstitutionTemplateLiteral(LiteralData),
    /// A template head (`` `...${ ``).
    TemplateHead(LiteralData),
    /// A template middle (`` }...${ ``).
    TemplateMiddle(LiteralData),
    /// A template tail (`` }...` ``).
    TemplateTail(LiteralData),
    /// A conditional/ternary expression (`a ? b : c`).
    ConditionalExpression(ConditionalExpressionData),
    /// A type reference (`A`, `A.B`, `A<T>`).
    TypeReference(TypeReferenceData),
    /// An array type (`T[]`).
    ArrayType(ArrayTypeData),
    /// An indexed-access type (`T[K]`).
    IndexedAccessType(IndexedAccessTypeData),
    /// A union type (`A | B`).
    UnionType(TypeListData),
    /// An intersection type (`A & B`).
    IntersectionType(TypeListData),
    /// A parenthesized type (`(T)`).
    ParenthesizedType(ParenTypeData),
    /// A literal type (`"a"`, `1`, `true`).
    LiteralType(LiteralTypeData),
    /// A function type (`(a: T) => U`).
    FunctionType(Box<SignatureTypeData>),
    /// A constructor type (`new (a: T) => U`).
    ConstructorType(Box<SignatureTypeData>),
    /// A conditional type (`T extends U ? X : Y`).
    ConditionalType(Box<ConditionalTypeData>),
    /// An `infer T` type.
    InferType(InferTypeData),
    /// A type operator (`keyof`/`unique`/`readonly`).
    TypeOperator(TypeOperatorData),
    /// A mapped type (`{ [K in T]: U }`).
    MappedType(Box<MappedTypeData>),
    /// A tuple type (`[A, B]`).
    TupleType(TypeListData),
    /// A named tuple member (`[name: T]`).
    NamedTupleMember(Box<NamedTupleMemberData>),
    /// A rest tuple element type (`...T`).
    RestType(ParenTypeData),
    /// An optional tuple element type (`T?`).
    OptionalType(ParenTypeData),
    /// The `this` type.
    ThisType,
    /// A type query (`typeof X`).
    TypeQuery(TypeQueryData),
    /// An import type (`import("m").X`).
    ImportType(Box<ImportTypeData>),
    /// A template literal type (`` `a${T}` ``).
    TemplateLiteralType(TemplateExpressionData),
    /// A template literal type span (`${T}literal`).
    TemplateLiteralTypeSpan(TemplateSpanData),
    /// A type predicate (`x is T`, `asserts x`).
    TypePredicate(Box<TypePredicateData>),
    /// The root of a parsed file: its statements and end-of-file token plus
    /// file-level metadata.
    SourceFile(Box<SourceFileData>),
}

/// Payload of an [`NodeData::Identifier`].
// Go: internal/ast/ast_generated.go:Identifier
#[derive(Clone, Debug)]
pub struct IdentifierData {
    /// The identifier text.
    pub text: String,
}

/// Payload of a text-only node (e.g. private identifier).
// Go: internal/ast/ast_generated.go:PrivateIdentifier
#[derive(Clone, Debug)]
pub struct TextData {
    /// The node text.
    pub text: String,
}

/// Payload of a literal-like leaf node.
// Go: internal/ast/ast_generated.go:LiteralLikeNodeBase
#[derive(Clone, Debug)]
pub struct LiteralData {
    /// The literal source text.
    pub text: String,
    /// Scanner flags for the literal.
    pub token_flags: TokenFlags,
}

/// Payload of an [`NodeData::QualifiedName`].
// Go: internal/ast/ast_generated.go:QualifiedName
#[derive(Clone, Debug)]
pub struct QualifiedNameData {
    /// Left-hand entity name.
    pub left: NodeId,
    /// Right-hand identifier.
    pub right: NodeId,
}

/// Payload of an [`NodeData::PropertyAccessExpression`].
// Go: internal/ast/ast_generated.go:PropertyAccessExpression
#[derive(Clone, Debug)]
pub struct PropertyAccessData {
    /// The object expression.
    pub expression: NodeId,
    /// Optional `?.` token.
    pub question_dot_token: Option<NodeId>,
    /// The member name.
    pub name: NodeId,
}

/// Payload of an [`NodeData::ElementAccessExpression`].
// Go: internal/ast/ast_generated.go:ElementAccessExpression
#[derive(Clone, Debug)]
pub struct ElementAccessData {
    /// The object expression.
    pub expression: NodeId,
    /// Optional `?.` token.
    pub question_dot_token: Option<NodeId>,
    /// The index/argument expression.
    pub argument_expression: NodeId,
}

/// Payload of an [`NodeData::CallExpression`].
// Go: internal/ast/ast_generated.go:CallExpression
#[derive(Clone, Debug)]
pub struct CallExpressionData {
    /// The callee expression.
    pub expression: NodeId,
    /// Optional `?.` token.
    pub question_dot_token: Option<NodeId>,
    /// Optional type-argument list.
    pub type_arguments: Option<NodeList>,
    /// The argument list.
    pub arguments: NodeList,
}

/// Payload of an [`NodeData::NewExpression`].
// Go: internal/ast/ast_generated.go:NewExpression
#[derive(Clone, Debug)]
pub struct NewExpressionData {
    /// The constructor expression.
    pub expression: NodeId,
    /// Optional type-argument list.
    pub type_arguments: Option<NodeList>,
    /// Optional argument list.
    pub arguments: Option<NodeList>,
}

/// Payload of a node with a single required child expression.
// Go: internal/ast/ast_generated.go:ParenthesizedExpression
#[derive(Clone, Debug)]
pub struct UnaryChildData {
    /// The single child expression.
    pub expression: NodeId,
}

/// Payload of a node with a single optional child expression.
// Go: internal/ast/ast_generated.go:ReturnStatement
#[derive(Clone, Debug)]
pub struct OptionalChildData {
    /// The optional child expression.
    pub expression: Option<NodeId>,
}

/// Payload of a prefix/postfix unary expression.
// Go: internal/ast/ast_generated.go:PrefixUnaryExpression
#[derive(Clone, Debug)]
pub struct UnaryOperatorData {
    /// The operator token kind.
    pub operator: Kind,
    /// The operand expression.
    pub operand: NodeId,
}

/// Payload of an [`NodeData::BinaryExpression`].
///
/// The rarely used optional `modifiers`/`type` fields from the Go struct are
/// omitted in this representative port.
// Go: internal/ast/ast_generated.go:BinaryExpression
#[derive(Clone, Debug)]
pub struct BinaryExpressionData {
    /// The left operand.
    pub left: NodeId,
    /// The operator token node.
    pub operator_token: NodeId,
    /// The right operand.
    pub right: NodeId,
}

/// Payload of a node whose only child is a [`NodeList`].
// Go: internal/ast/ast_generated.go:Block
#[derive(Clone, Debug)]
pub struct ListData {
    /// The element/statement list.
    pub list: NodeList,
}

/// Payload of an [`NodeData::IfStatement`].
// Go: internal/ast/ast_generated.go:IfStatement
#[derive(Clone, Debug)]
pub struct IfStatementData {
    /// The condition expression.
    pub expression: NodeId,
    /// The statement run when the condition is truthy.
    pub then_statement: NodeId,
    /// The optional `else` statement.
    pub else_statement: Option<NodeId>,
}

/// Payload of an [`NodeData::DoStatement`].
// Go: internal/ast/ast_generated.go:DoStatement
#[derive(Clone, Debug)]
pub struct DoStatementData {
    /// The loop body.
    pub statement: NodeId,
    /// The loop condition.
    pub expression: NodeId,
}

/// Payload of an [`NodeData::WhileStatement`].
// Go: internal/ast/ast_generated.go:WhileStatement
#[derive(Clone, Debug)]
pub struct WhileStatementData {
    /// The loop condition.
    pub expression: NodeId,
    /// The loop body.
    pub statement: NodeId,
}

/// Payload of an [`NodeData::WithStatement`].
// Go: internal/ast/ast_generated.go:WithStatement
#[derive(Clone, Debug)]
pub struct WithStatementData {
    /// The object expression.
    pub expression: NodeId,
    /// The body statement.
    pub statement: NodeId,
}

/// Payload of an [`NodeData::SwitchStatement`].
// Go: internal/ast/ast_generated.go:SwitchStatement
#[derive(Clone, Debug)]
pub struct SwitchStatementData {
    /// The switched expression.
    pub expression: NodeId,
    /// The `{ ... }` block of clauses.
    pub case_block: NodeId,
}

/// Payload of an [`NodeData::CaseBlock`].
// Go: internal/ast/ast_generated.go:CaseBlock
#[derive(Clone, Debug)]
pub struct CaseBlockData {
    /// The `case`/`default` clauses.
    pub clauses: NodeList,
}

/// Payload of an [`NodeData::CaseOrDefaultClause`].
// Go: internal/ast/ast_generated.go:CaseOrDefaultClause
#[derive(Clone, Debug)]
pub struct CaseOrDefaultClauseData {
    /// The clause expression (`None` for `default`).
    pub expression: Option<NodeId>,
    /// The clause statements.
    pub statements: NodeList,
}

/// Payload of a `break`/`continue` statement.
// Go: internal/ast/ast_generated.go:BreakStatement
#[derive(Clone, Debug)]
pub struct LabelData {
    /// The optional target label identifier.
    pub label: Option<NodeId>,
}

/// Payload of an [`NodeData::LabeledStatement`].
// Go: internal/ast/ast_generated.go:LabeledStatement
#[derive(Clone, Debug)]
pub struct LabeledStatementData {
    /// The label identifier.
    pub label: NodeId,
    /// The labeled statement.
    pub statement: NodeId,
}

/// Payload of an [`NodeData::VariableStatement`].
// Go: internal/ast/ast_generated.go:VariableStatement
#[derive(Clone, Debug)]
pub struct VariableStatementData {
    /// Optional modifiers (`export`, `declare`).
    pub modifiers: Option<ModifierList>,
    /// The declaration list (`let x = 1, y`).
    pub declaration_list: NodeId,
}

/// Payload of an [`NodeData::VariableDeclarationList`].
// Go: internal/ast/ast_generated.go:VariableDeclarationList
#[derive(Clone, Debug)]
pub struct VariableDeclarationListData {
    /// The individual declarations.
    pub declarations: NodeList,
}

/// Payload of an [`NodeData::VariableDeclaration`].
// Go: internal/ast/ast_generated.go:VariableDeclaration
#[derive(Clone, Debug)]
pub struct VariableDeclarationData {
    /// The binding name (identifier or binding pattern).
    pub name: NodeId,
    /// Optional definite-assignment `!` token.
    pub exclamation_token: Option<NodeId>,
    /// Optional type annotation.
    pub type_node: Option<NodeId>,
    /// Optional initializer expression.
    pub initializer: Option<NodeId>,
}

/// Payload of an object/array binding pattern.
// Go: internal/ast/ast_generated.go:BindingPattern
#[derive(Clone, Debug)]
pub struct BindingPatternData {
    /// The binding elements.
    pub elements: NodeList,
}

/// Payload of an [`NodeData::BindingElement`].
// Go: internal/ast/ast_generated.go:BindingElement
#[derive(Clone, Debug)]
pub struct BindingElementData {
    /// Optional `...` rest token.
    pub dot_dot_dot_token: Option<NodeId>,
    /// Optional property name (for object destructuring rename).
    pub property_name: Option<NodeId>,
    /// The binding name (`None` for an array hole).
    pub name: Option<NodeId>,
    /// Optional default initializer.
    pub initializer: Option<NodeId>,
}

/// Payload of an [`NodeData::ForStatement`].
// Go: internal/ast/ast_generated.go:ForStatement
#[derive(Clone, Debug)]
pub struct ForStatementData {
    /// Optional initializer (declaration list or expression).
    pub initializer: Option<NodeId>,
    /// Optional loop condition.
    pub condition: Option<NodeId>,
    /// Optional increment expression.
    pub incrementor: Option<NodeId>,
    /// The loop body.
    pub statement: NodeId,
}

/// Payload of an [`NodeData::ForInOrOfStatement`].
// Go: internal/ast/ast_generated.go:ForInOrOfStatement
#[derive(Clone, Debug)]
pub struct ForInOrOfStatementData {
    /// Optional `await` modifier (`for await`).
    pub await_modifier: Option<NodeId>,
    /// The loop initializer (declaration list or expression).
    pub initializer: NodeId,
    /// The iterated expression.
    pub expression: NodeId,
    /// The loop body.
    pub statement: NodeId,
}

/// Payload of an [`NodeData::TryStatement`].
// Go: internal/ast/ast_generated.go:TryStatement
#[derive(Clone, Debug)]
pub struct TryStatementData {
    /// The `try { ... }` block.
    pub try_block: NodeId,
    /// Optional `catch` clause.
    pub catch_clause: Option<NodeId>,
    /// Optional `finally { ... }` block.
    pub finally_block: Option<NodeId>,
}

/// Payload of an [`NodeData::CatchClause`].
// Go: internal/ast/ast_generated.go:CatchClause
#[derive(Clone, Debug)]
pub struct CatchClauseData {
    /// Optional caught-variable declaration.
    pub variable_declaration: Option<NodeId>,
    /// The `catch` body block.
    pub block: NodeId,
}

/// Payload of an [`NodeData::FunctionDeclaration`].
// Go: internal/ast/ast_generated.go:FunctionDeclaration
#[derive(Clone, Debug)]
pub struct FunctionDeclarationData {
    /// Optional modifiers (`export`, `async`, `declare`, ...).
    pub modifiers: Option<ModifierList>,
    /// Optional generator `*` token.
    pub asterisk_token: Option<NodeId>,
    /// Optional function name.
    pub name: Option<NodeId>,
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// Optional return type annotation.
    pub type_node: Option<NodeId>,
    /// Optional full-signature node (JSDoc overload signatures).
    pub full_signature: Option<NodeId>,
    /// Optional function body (`None` for overload signatures).
    pub body: Option<NodeId>,
}

/// Payload of an [`NodeData::ParameterDeclaration`].
// Go: internal/ast/ast_generated.go:ParameterDeclaration
#[derive(Clone, Debug)]
pub struct ParameterDeclarationData {
    /// Optional modifiers / parameter-property modifiers.
    pub modifiers: Option<ModifierList>,
    /// Optional `...` rest token.
    pub dot_dot_dot_token: Option<NodeId>,
    /// The parameter name (identifier or binding pattern).
    pub name: NodeId,
    /// Optional `?` optional-parameter token.
    pub question_token: Option<NodeId>,
    /// Optional type annotation.
    pub type_node: Option<NodeId>,
    /// Optional default initializer.
    pub initializer: Option<NodeId>,
}

/// Payload of an [`NodeData::TypeParameterDeclaration`].
// Go: internal/ast/ast_generated.go:TypeParameterDeclaration
#[derive(Clone, Debug)]
pub struct TypeParameterData {
    /// Optional modifiers (`const`, `in`, `out`).
    pub modifiers: Option<ModifierList>,
    /// The type-parameter name.
    pub name: NodeId,
    /// Optional `extends` constraint type.
    pub constraint: Option<NodeId>,
    /// Optional improper-constraint expression (error recovery).
    pub expression: Option<NodeId>,
    /// Optional `= Default` type.
    pub default_type: Option<NodeId>,
}

/// Payload of a class declaration/expression.
// Go: internal/ast/ast_generated.go:ClassLikeBase
#[derive(Clone, Debug)]
pub struct ClassLikeData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// Optional class name.
    pub name: Option<NodeId>,
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// Optional heritage clauses (`extends`/`implements`).
    pub heritage_clauses: Option<NodeList>,
    /// The class members.
    pub members: NodeList,
}

/// Payload of an [`NodeData::HeritageClause`].
// Go: internal/ast/ast_generated.go:HeritageClause
#[derive(Clone, Debug)]
pub struct HeritageClauseData {
    /// The clause token (`extends` or `implements`).
    pub token: Kind,
    /// The clause types (each an `ExpressionWithTypeArguments`).
    pub types: NodeList,
}

/// Payload of an [`NodeData::ExpressionWithTypeArguments`].
// Go: internal/ast/ast_generated.go:ExpressionWithTypeArguments
#[derive(Clone, Debug)]
pub struct ExprWithTypeArgsData {
    /// The base expression.
    pub expression: NodeId,
    /// Optional type arguments.
    pub type_arguments: Option<NodeList>,
}

/// Payload of a method declaration.
// Go: internal/ast/ast_generated.go:MethodDeclaration
#[derive(Clone, Debug)]
pub struct MethodLikeData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// Optional generator `*` token.
    pub asterisk_token: Option<NodeId>,
    /// The method name.
    pub name: NodeId,
    /// Optional `?`/`!` postfix token.
    pub postfix_token: Option<NodeId>,
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// Optional return type.
    pub type_node: Option<NodeId>,
    /// Optional full-signature node.
    pub full_signature: Option<NodeId>,
    /// Optional method body.
    pub body: Option<NodeId>,
}

/// Payload of an [`NodeData::PropertyDeclaration`].
// Go: internal/ast/ast_generated.go:PropertyDeclaration
#[derive(Clone, Debug)]
pub struct PropertyDeclarationData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The property name.
    pub name: NodeId,
    /// Optional `?`/`!` postfix token.
    pub postfix_token: Option<NodeId>,
    /// Optional type annotation.
    pub type_node: Option<NodeId>,
    /// Optional initializer.
    pub initializer: Option<NodeId>,
}

/// Payload of a `get`/`set` accessor declaration.
// Go: internal/ast/ast_generated.go:GetAccessorDeclaration
#[derive(Clone, Debug)]
pub struct AccessorData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The accessor name.
    pub name: NodeId,
    /// Optional generic type parameters (grammar error, kept for fidelity).
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// Optional return type.
    pub type_node: Option<NodeId>,
    /// Optional full-signature node.
    pub full_signature: Option<NodeId>,
    /// Optional accessor body.
    pub body: Option<NodeId>,
}

/// Payload of an [`NodeData::ConstructorDeclaration`].
// Go: internal/ast/ast_generated.go:ConstructorDeclaration
#[derive(Clone, Debug)]
pub struct ConstructorData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// Optional generic type parameters (grammar error, kept for fidelity).
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// Optional return type (grammar error, kept for fidelity).
    pub type_node: Option<NodeId>,
    /// Optional full-signature node.
    pub full_signature: Option<NodeId>,
    /// Optional constructor body.
    pub body: Option<NodeId>,
}

/// Payload of an [`NodeData::IndexSignatureDeclaration`].
// Go: internal/ast/ast_generated.go:IndexSignatureDeclaration
#[derive(Clone, Debug)]
pub struct IndexSignatureData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The index parameter list.
    pub parameters: NodeList,
    /// Optional value type.
    pub type_node: Option<NodeId>,
}

/// Payload of an [`NodeData::ClassStaticBlockDeclaration`].
// Go: internal/ast/ast_generated.go:ClassStaticBlockDeclaration
#[derive(Clone, Debug)]
pub struct ClassStaticBlockData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The static block body.
    pub body: NodeId,
}

/// Payload of an [`NodeData::TypeAliasDeclaration`].
// Go: internal/ast/ast_generated.go:TypeAliasDeclaration
#[derive(Clone, Debug)]
pub struct TypeAliasData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The alias name.
    pub name: NodeId,
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// The aliased type.
    pub type_node: NodeId,
}

/// Payload of an [`NodeData::EnumDeclaration`].
// Go: internal/ast/ast_generated.go:EnumDeclaration
#[derive(Clone, Debug)]
pub struct EnumDeclData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The enum name.
    pub name: NodeId,
    /// The enum members.
    pub members: NodeList,
}

/// Payload of an [`NodeData::EnumMember`].
// Go: internal/ast/ast_generated.go:EnumMember
#[derive(Clone, Debug)]
pub struct EnumMemberData {
    /// The member name.
    pub name: NodeId,
    /// Optional initializer.
    pub initializer: Option<NodeId>,
}

/// Payload of a call/construct signature member.
// Go: internal/ast/ast_generated.go:CallSignatureDeclaration
#[derive(Clone, Debug)]
pub struct SignatureDeclData {
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// Optional return type.
    pub type_node: Option<NodeId>,
}

/// Payload of an [`NodeData::MethodSignature`].
// Go: internal/ast/ast_generated.go:MethodSignatureDeclaration
#[derive(Clone, Debug)]
pub struct MethodSignatureData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The method name.
    pub name: NodeId,
    /// Optional `?` postfix token.
    pub postfix_token: Option<NodeId>,
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// Optional return type.
    pub type_node: Option<NodeId>,
}

/// Payload of an [`NodeData::TypeLiteral`].
// Go: internal/ast/ast_generated.go:TypeLiteralNode
#[derive(Clone, Debug)]
pub struct TypeLiteralData {
    /// The type members.
    pub members: NodeList,
}

/// Payload of an [`NodeData::ModuleDeclaration`].
// Go: internal/ast/ast_generated.go:ModuleDeclaration
#[derive(Clone, Debug)]
pub struct ModuleDeclData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The declaration keyword (`module`/`namespace`/`global`).
    pub keyword: Kind,
    /// The module name (identifier or string literal).
    pub name: NodeId,
    /// Optional body (module block or nested module declaration).
    pub body: Option<NodeId>,
}

/// Payload of an [`NodeData::ModuleBlock`].
// Go: internal/ast/ast_generated.go:ModuleBlock
#[derive(Clone, Debug)]
pub struct ModuleBlockData {
    /// The body statements.
    pub statements: NodeList,
}

/// Payload carrying a single name reference (namespace import/export).
// Go: internal/ast/ast_generated.go:NamespaceImport
#[derive(Clone, Debug)]
pub struct NameRefData {
    /// The referenced name.
    pub name: NodeId,
}

/// Payload carrying a list of elements (named imports/exports).
// Go: internal/ast/ast_generated.go:NamedImports
#[derive(Clone, Debug)]
pub struct ElementsData {
    /// The element list.
    pub elements: NodeList,
}

/// Payload of an import/export specifier.
// Go: internal/ast/ast_generated.go:ImportSpecifier
#[derive(Clone, Debug)]
pub struct ImportExportSpecData {
    /// Whether this specifier is `type`-only.
    pub is_type_only: bool,
    /// Optional original name (`a` in `a as b`).
    pub property_name: Option<NodeId>,
    /// The local/exported name.
    pub name: NodeId,
}

/// Payload of an [`NodeData::ImportDeclaration`].
// Go: internal/ast/ast_generated.go:ImportDeclaration
#[derive(Clone, Debug)]
pub struct ImportDeclData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// Optional import clause.
    pub import_clause: Option<NodeId>,
    /// The module specifier expression.
    pub module_specifier: NodeId,
    /// Optional import attributes (`with { ... }`).
    pub attributes: Option<NodeId>,
}

/// Payload of an [`NodeData::ImportAttributes`].
// Go: internal/ast/ast_generated.go:ImportAttributes
#[derive(Clone, Debug)]
pub struct ImportAttributesData {
    /// The keyword token kind (`with` or `assert`).
    pub token: Kind,
    /// The attribute elements.
    pub attributes: NodeList,
    /// Whether the clause spans multiple lines.
    pub multiline: bool,
}

/// Payload of an [`NodeData::ImportAttribute`].
// Go: internal/ast/ast_generated.go:ImportAttribute
#[derive(Clone, Debug)]
pub struct ImportAttributeData {
    /// The attribute name (identifier or string literal).
    pub name: Option<NodeId>,
    /// The attribute value expression.
    pub value: NodeId,
}

/// Payload of a [`NodeData::JsxElement`] or [`NodeData::JsxFragment`].
// Go: internal/ast/ast_generated.go:JsxElement / JsxFragment
#[derive(Clone, Debug)]
pub struct JsxElementData {
    /// The opening element/fragment.
    pub opening: NodeId,
    /// The child nodes.
    pub children: NodeList,
    /// The closing element/fragment.
    pub closing: NodeId,
}

/// Payload of a [`NodeData::JsxOpeningElement`] or [`NodeData::JsxSelfClosingElement`].
// Go: internal/ast/ast_generated.go:JsxOpeningElement / JsxSelfClosingElement
#[derive(Clone, Debug)]
pub struct JsxOpeningLikeData {
    /// The tag name expression.
    pub tag_name: NodeId,
    /// Optional type arguments.
    pub type_arguments: Option<NodeList>,
    /// The attributes node.
    pub attributes: NodeId,
}

/// Payload of a [`NodeData::JsxClosingElement`].
// Go: internal/ast/ast_generated.go:JsxClosingElement
#[derive(Clone, Debug)]
pub struct JsxClosingElementData {
    /// The tag name expression.
    pub tag_name: NodeId,
}

/// Payload of a [`NodeData::JsxAttribute`].
// Go: internal/ast/ast_generated.go:JsxAttribute
#[derive(Clone, Debug)]
pub struct JsxAttributeData {
    /// The attribute name.
    pub name: NodeId,
    /// Optional initializer (string literal or expression container).
    pub initializer: Option<NodeId>,
}

/// Payload of a [`NodeData::JsxNamespacedName`].
// Go: internal/ast/ast_generated.go:JsxNamespacedName
#[derive(Clone, Debug)]
pub struct JsxNamespacedNameData {
    /// The namespace identifier.
    pub namespace: NodeId,
    /// The local name identifier.
    pub name: NodeId,
}

/// Payload of a [`NodeData::JsxExpression`].
// Go: internal/ast/ast_generated.go:JsxExpression
#[derive(Clone, Debug)]
pub struct JsxExpressionData {
    /// Optional `...` spread token.
    pub dot_dot_dot_token: Option<NodeId>,
    /// Optional contained expression.
    pub expression: Option<NodeId>,
}

/// Payload of a [`NodeData::JsxText`].
// Go: internal/ast/ast_generated.go:JsxText
#[derive(Clone, Debug)]
pub struct JsxTextData {
    /// The raw text.
    pub text: String,
    /// Whether the text is only whitespace/trivia.
    pub contains_only_trivia_white_spaces: bool,
}

/// Payload of an [`NodeData::ImportClause`].
// Go: internal/ast/ast_generated.go:ImportClause
#[derive(Clone, Debug)]
pub struct ImportClauseData {
    /// The phase modifier (`type`/`defer`/none).
    pub phase_modifier: Kind,
    /// Optional default-import name.
    pub name: Option<NodeId>,
    /// Optional named bindings (namespace or named imports).
    pub named_bindings: Option<NodeId>,
}

/// Payload of an [`NodeData::ImportEqualsDeclaration`].
// Go: internal/ast/ast_generated.go:ImportEqualsDeclaration
#[derive(Clone, Debug)]
pub struct ImportEqualsData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// Whether this is `import type`.
    pub is_type_only: bool,
    /// The bound name.
    pub name: NodeId,
    /// The module reference (entity name or `require(...)`).
    pub module_reference: NodeId,
}

/// Payload of an [`NodeData::ExportDeclaration`].
// Go: internal/ast/ast_generated.go:ExportDeclaration
#[derive(Clone, Debug)]
pub struct ExportDeclData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// Whether this is `export type`.
    pub is_type_only: bool,
    /// Optional export clause (named exports or namespace export).
    pub export_clause: Option<NodeId>,
    /// Optional re-export module specifier.
    pub module_specifier: Option<NodeId>,
    /// Optional import attributes.
    pub attributes: Option<NodeId>,
}

/// Payload of an [`NodeData::ExportAssignment`].
// Go: internal/ast/ast_generated.go:ExportAssignment
#[derive(Clone, Debug)]
pub struct ExportAssignData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// Whether this is `export =` (vs `export default`).
    pub is_export_equals: bool,
    /// Optional type node (grammar error, kept for fidelity).
    pub type_node: Option<NodeId>,
    /// The exported expression.
    pub expression: NodeId,
}

/// Payload of an [`NodeData::NamespaceExportDeclaration`].
// Go: internal/ast/ast_generated.go:NamespaceExportDeclaration
#[derive(Clone, Debug)]
pub struct NamespaceExportDeclData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The exported namespace name.
    pub name: NodeId,
}

/// Payload of an `as`/`satisfies` expression.
// Go: internal/ast/ast_generated.go:AsExpression
#[derive(Clone, Debug)]
pub struct ExprTypeData {
    /// The operand expression.
    pub expression: NodeId,
    /// The target type.
    pub type_node: NodeId,
}

/// Payload of a `<Type>expr` type-assertion expression.
// Go: internal/ast/ast_generated.go:TypeAssertion
#[derive(Clone, Debug)]
pub struct TypeAssertionData {
    /// The asserted type.
    pub type_node: NodeId,
    /// The operand expression.
    pub expression: NodeId,
}

/// Payload of an [`NodeData::MetaProperty`].
// Go: internal/ast/ast_generated.go:MetaProperty
#[derive(Clone, Debug)]
pub struct MetaPropertyData {
    /// The keyword token kind (`new` or `import`).
    pub keyword_token: Kind,
    /// The property name identifier (`target`/`meta`).
    pub name: NodeId,
}

/// Payload of an [`NodeData::TemplateExpression`].
// Go: internal/ast/ast_generated.go:TemplateExpression
#[derive(Clone, Debug)]
pub struct TemplateExpressionData {
    /// The template head literal.
    pub head: NodeId,
    /// The template spans.
    pub template_spans: NodeList,
}

/// Payload of an [`NodeData::TemplateSpan`].
// Go: internal/ast/ast_generated.go:TemplateSpan
#[derive(Clone, Debug)]
pub struct TemplateSpanData {
    /// The interpolated expression.
    pub expression: NodeId,
    /// The trailing template literal (middle or tail).
    pub literal: NodeId,
}

/// Payload of an [`NodeData::TaggedTemplateExpression`].
// Go: internal/ast/ast_generated.go:TaggedTemplateExpression
#[derive(Clone, Debug)]
pub struct TaggedTemplateData {
    /// The tag expression.
    pub tag: NodeId,
    /// Optional `?.` token.
    pub question_dot_token: Option<NodeId>,
    /// Optional type arguments.
    pub type_arguments: Option<NodeList>,
    /// The template literal.
    pub template: NodeId,
}

/// Payload of an [`NodeData::ArrowFunction`].
// Go: internal/ast/ast_generated.go:ArrowFunction
#[derive(Clone, Debug)]
pub struct ArrowFunctionData {
    /// Optional modifiers (`async`).
    pub modifiers: Option<ModifierList>,
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// Optional return type.
    pub type_node: Option<NodeId>,
    /// Optional full-signature node.
    pub full_signature: Option<NodeId>,
    /// The `=>` token.
    pub equals_greater_than_token: NodeId,
    /// The arrow body (block or concise expression).
    pub body: NodeId,
}

/// Payload of an [`NodeData::YieldExpression`].
// Go: internal/ast/ast_generated.go:YieldExpression
#[derive(Clone, Debug)]
pub struct YieldExpressionData {
    /// Optional `*` delegate token.
    pub asterisk_token: Option<NodeId>,
    /// Optional yielded expression.
    pub expression: Option<NodeId>,
}

/// Payload of an [`NodeData::ShorthandPropertyAssignment`].
// Go: internal/ast/ast_generated.go:ShorthandPropertyAssignment
#[derive(Clone, Debug)]
pub struct ShorthandPropertyAssignmentData {
    /// Optional modifiers.
    pub modifiers: Option<ModifierList>,
    /// The property name.
    pub name: NodeId,
    /// Optional `?`/`!` postfix token.
    pub postfix_token: Option<NodeId>,
    /// Optional type annotation (grammar error, kept for fidelity).
    pub type_node: Option<NodeId>,
    /// Optional `=` token (cover-initialized-name).
    pub equals_token: Option<NodeId>,
    /// Optional object-assignment initializer.
    pub object_assignment_initializer: Option<NodeId>,
}

/// Payload of a function/constructor type. (`modifiers` is `None` for function
/// types; populated with `abstract` for some constructor types.)
// Go: internal/ast/ast_generated.go:FunctionTypeNode / ConstructorTypeNode
#[derive(Clone, Debug)]
pub struct SignatureTypeData {
    /// Optional modifiers (`abstract` for constructor types).
    pub modifiers: Option<ModifierList>,
    /// Optional generic type parameters.
    pub type_parameters: Option<NodeList>,
    /// The parameter list.
    pub parameters: NodeList,
    /// The return type.
    pub type_node: Option<NodeId>,
}

/// Payload of an [`NodeData::ConditionalType`].
// Go: internal/ast/ast_generated.go:ConditionalTypeNode
#[derive(Clone, Debug)]
pub struct ConditionalTypeData {
    /// The checked type.
    pub check_type: NodeId,
    /// The `extends` type.
    pub extends_type: NodeId,
    /// The true branch type.
    pub true_type: NodeId,
    /// The false branch type.
    pub false_type: NodeId,
}

/// Payload of an [`NodeData::InferType`].
// Go: internal/ast/ast_generated.go:InferTypeNode
#[derive(Clone, Debug)]
pub struct InferTypeData {
    /// The inferred type parameter.
    pub type_parameter: NodeId,
}

/// Payload of an [`NodeData::TypeOperator`].
// Go: internal/ast/ast_generated.go:TypeOperatorNode
#[derive(Clone, Debug)]
pub struct TypeOperatorData {
    /// The operator (`keyof`/`unique`/`readonly`).
    pub operator: Kind,
    /// The operand type.
    pub type_node: NodeId,
}

/// Payload of an [`NodeData::MappedType`].
// Go: internal/ast/ast_generated.go:MappedTypeNode
#[derive(Clone, Debug)]
pub struct MappedTypeData {
    /// Optional `readonly`/`+`/`-` token.
    pub readonly_token: Option<NodeId>,
    /// The mapped type parameter (`K in T`).
    pub type_parameter: NodeId,
    /// Optional `as` name type.
    pub name_type: Option<NodeId>,
    /// Optional `?`/`+`/`-` token.
    pub question_token: Option<NodeId>,
    /// Optional value type.
    pub type_node: Option<NodeId>,
    /// Trailing members.
    pub members: NodeList,
}

/// Payload of an [`NodeData::NamedTupleMember`].
// Go: internal/ast/ast_generated.go:NamedTupleMember
#[derive(Clone, Debug)]
pub struct NamedTupleMemberData {
    /// Optional `...` rest token.
    pub dot_dot_dot_token: Option<NodeId>,
    /// The member name.
    pub name: NodeId,
    /// Optional `?` token.
    pub question_token: Option<NodeId>,
    /// The member type.
    pub type_node: NodeId,
}

/// Payload of an [`NodeData::TypeQuery`].
// Go: internal/ast/ast_generated.go:TypeQueryNode
#[derive(Clone, Debug)]
pub struct TypeQueryData {
    /// The queried entity name.
    pub expr_name: NodeId,
    /// Optional type arguments.
    pub type_arguments: Option<NodeList>,
}

/// Payload of an [`NodeData::ImportType`].
// Go: internal/ast/ast_generated.go:ImportTypeNode
#[derive(Clone, Debug)]
pub struct ImportTypeData {
    /// Whether this is `typeof import(...)`.
    pub is_type_of: bool,
    /// The module-specifier argument type.
    pub argument: NodeId,
    /// Optional import attributes.
    pub attributes: Option<NodeId>,
    /// Optional dotted qualifier.
    pub qualifier: Option<NodeId>,
    /// Optional type arguments.
    pub type_arguments: Option<NodeList>,
}

/// Payload of an [`NodeData::TypePredicate`].
// Go: internal/ast/ast_generated.go:TypePredicateNode
#[derive(Clone, Debug)]
pub struct TypePredicateData {
    /// Optional `asserts` modifier token.
    pub asserts_modifier: Option<NodeId>,
    /// The parameter name (identifier or `this` type).
    pub parameter_name: NodeId,
    /// Optional asserted/predicate type.
    pub type_node: Option<NodeId>,
}

/// Payload of an [`NodeData::ConditionalExpression`].
// Go: internal/ast/ast_generated.go:ConditionalExpression
#[derive(Clone, Debug)]
pub struct ConditionalExpressionData {
    /// The condition expression.
    pub condition: NodeId,
    /// The `?` token.
    pub question_token: NodeId,
    /// The branch taken when the condition is truthy.
    pub when_true: NodeId,
    /// The `:` token.
    pub colon_token: NodeId,
    /// The branch taken when the condition is falsy.
    pub when_false: NodeId,
}

/// Payload of an [`NodeData::TypeReference`].
// Go: internal/ast/ast_generated.go:TypeReferenceNode
#[derive(Clone, Debug)]
pub struct TypeReferenceData {
    /// The referenced entity name (identifier or qualified name).
    pub type_name: NodeId,
    /// Optional type-argument list.
    pub type_arguments: Option<NodeList>,
}

/// Payload of an [`NodeData::ArrayType`].
// Go: internal/ast/ast_generated.go:ArrayTypeNode
#[derive(Clone, Debug)]
pub struct ArrayTypeData {
    /// The element type.
    pub element_type: NodeId,
}

/// Payload of an [`NodeData::IndexedAccessType`].
// Go: internal/ast/ast_generated.go:IndexedAccessTypeNode
#[derive(Clone, Debug)]
pub struct IndexedAccessTypeData {
    /// The object type (`T` in `T[K]`).
    pub object_type: NodeId,
    /// The index type (`K` in `T[K]`).
    pub index_type: NodeId,
}

/// Payload of a union or intersection type (a list of constituent types).
// Go: internal/ast/ast_generated.go:UnionTypeNode
#[derive(Clone, Debug)]
pub struct TypeListData {
    /// The constituent types.
    pub types: NodeList,
}

/// Payload of an [`NodeData::ParenthesizedType`].
// Go: internal/ast/ast_generated.go:ParenthesizedTypeNode
#[derive(Clone, Debug)]
pub struct ParenTypeData {
    /// The inner type.
    pub type_node: NodeId,
}

/// Payload of an [`NodeData::LiteralType`].
// Go: internal/ast/ast_generated.go:LiteralTypeNode
#[derive(Clone, Debug)]
pub struct LiteralTypeData {
    /// The literal expression (`"a"`, `1`, `true`, ...).
    pub literal: NodeId,
}

/// Payload of an [`NodeData::SourceFile`].
///
/// This representative port carries the statement list, end-of-file token, and
/// the file-level metadata the parser fills in; the full Go `SourceFile` struct
/// additionally caches binder/checker state added in later phases.
// Go: internal/ast/ast.go:SourceFile
#[derive(Clone, Debug)]
pub struct SourceFileData {
    /// The top-level statement list.
    pub statements: NodeList,
    /// The end-of-file token node.
    pub end_of_file_token: NodeId,
    /// The (normalized) file name.
    pub file_name: String,
    /// The script kind the file was parsed as.
    pub script_kind: ScriptKind,
    /// The language variant (standard vs JSX).
    pub language_variant: LanguageVariant,
    /// Whether this is a declaration (`.d.ts`) file.
    pub is_declaration_file: bool,
    /// The node that marks the file as an external module, if any.
    pub external_module_indicator: Option<NodeId>,
    /// Module-specifier string literals of top-level imports/re-exports and
    /// dynamic import/require calls (populated by the parser's reference pass).
    pub imports: Vec<NodeId>,
}

/// Owns every [`Node`] in a single `Vec`, replacing Go's `NodeFactory` arenas.
///
/// `NodeId`s are indices into `nodes`. Construction (`new_*`) appends and returns
/// the new id; traversal and cloning dispatch on [`NodeData`].
///
/// # Examples
/// ```
/// use tsgo_ast::{NodeArena, Kind};
/// let mut arena = NodeArena::new();
/// let id = arena.new_identifier("x");
/// assert_eq!(arena.kind(id), Kind::Identifier);
/// ```
///
/// Side effects: methods prefixed `new_`/`clone_`/`update_` mutate the arena.
// Go: internal/ast/ast.go:NodeFactory
#[derive(Debug, Default)]
pub struct NodeArena {
    nodes: Vec<Node>,
    node_count: usize,
    text_count: usize,
}

impl NodeArena {
    /// Creates an empty arena.
    ///
    /// Side effects: none.
    pub fn new() -> NodeArena {
        NodeArena::default()
    }

    /// Returns the number of nodes created (mirrors Go `NodeCount`).
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:NodeFactory.NodeCount
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Returns the number of text-bearing nodes created (mirrors Go `TextCount`).
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:NodeFactory.TextCount
    pub fn text_count(&self) -> usize {
        self.text_count
    }

    /// Returns the kind of node `id`.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:Node.Kind
    pub fn kind(&self, id: NodeId) -> Kind {
        self.nodes[id.index()].kind
    }

    /// Returns the flags of node `id`.
    ///
    /// Side effects: none (pure).
    pub fn flags(&self, id: NodeId) -> NodeFlags {
        self.nodes[id.index()].flags
    }

    /// Returns the source range of node `id`.
    ///
    /// Side effects: none (pure).
    pub fn loc(&self, id: NodeId) -> TextRange {
        self.nodes[id.index()].loc
    }

    /// Returns the parent of node `id`, if it has been set.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:Node.Parent
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.index()].parent
    }

    /// Returns a shared reference to the [`NodeData`] of node `id`.
    ///
    /// Side effects: none (pure).
    pub fn data(&self, id: NodeId) -> &NodeData {
        &self.nodes[id.index()].data
    }

    /// Returns a mutable reference to the [`NodeData`] of node `id`.
    ///
    /// Side effects: allows mutation of node `id`'s payload.
    pub fn data_mut(&mut self, id: NodeId) -> &mut NodeData {
        &mut self.nodes[id.index()].data
    }

    /// Sets the source range of node `id`.
    ///
    /// Side effects: mutates node `id`.
    pub fn set_loc(&mut self, id: NodeId, loc: TextRange) {
        self.nodes[id.index()].loc = loc;
    }

    /// Sets the parent of node `id`.
    ///
    /// Side effects: mutates node `id`.
    // Go: internal/ast/ast.go:Node.Parent (assignment)
    pub fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>) {
        self.nodes[id.index()].parent = parent;
    }

    /// Adds `flags` to node `id` (bitwise OR).
    ///
    /// Side effects: mutates node `id`.
    pub fn add_flags(&mut self, id: NodeId, flags: NodeFlags) {
        self.nodes[id.index()].flags |= flags;
    }

    /// Replaces the flags of node `id`.
    ///
    /// Side effects: mutates node `id`.
    pub fn set_flags(&mut self, id: NodeId, flags: NodeFlags) {
        self.nodes[id.index()].flags = flags;
    }

    /// Reports whether `list` ends with a trailing comma, i.e. its last element
    /// ends before the list's own end.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:NodeList.HasTrailingComma
    pub fn list_has_trailing_comma(&self, list: &NodeList) -> bool {
        match list.nodes.last() {
            None => false,
            Some(&last) => self.loc(last).end() < list.end(),
        }
    }

    /// Returns the source text of a text-bearing node (identifier or literal).
    ///
    /// # Panics
    /// Panics if `id` is not a text-bearing node, mirroring Go `Node.Text`.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:Node.Text
    pub fn text(&self, id: NodeId) -> &str {
        match &self.nodes[id.index()].data {
            NodeData::Identifier(d) => &d.text,
            NodeData::PrivateIdentifier(d) => &d.text,
            NodeData::StringLiteral(d)
            | NodeData::NumericLiteral(d)
            | NodeData::BigIntLiteral(d)
            | NodeData::RegularExpressionLiteral(d)
            | NodeData::NoSubstitutionTemplateLiteral(d)
            | NodeData::TemplateHead(d)
            | NodeData::TemplateMiddle(d)
            | NodeData::TemplateTail(d) => &d.text,
            NodeData::JsxText(d) => &d.text,
            other => panic!("Unhandled case in NodeArena::text: {other:?}"),
        }
    }

    /// Appends `data` of `kind` as a new node and returns its id.
    ///
    /// Side effects: pushes a node and bumps `node_count`.
    // Go: internal/ast/ast.go:NodeFactory.newNode
    fn new_node(&mut self, kind: Kind, data: NodeData) -> NodeId {
        self.node_count += 1;
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            kind,
            flags: NodeFlags::NONE,
            loc: TextRange::undefined(),
            parent: None,
            data,
        });
        id
    }

    /// Creates an identifier node with the given text.
    ///
    /// Side effects: pushes a node; bumps `node_count` and `text_count`.
    // Go: internal/ast/ast_generated.go:NewIdentifier
    pub fn new_identifier(&mut self, text: &str) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::Identifier,
            NodeData::Identifier(IdentifierData {
                text: text.to_string(),
            }),
        )
    }

    /// Creates a bare token node of the given `kind`.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewToken
    pub fn new_token(&mut self, kind: Kind) -> NodeId {
        self.new_node(kind, NodeData::Token)
    }

    /// Creates a private identifier node (`#name`).
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewPrivateIdentifier
    pub fn new_private_identifier(&mut self, text: &str) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::PrivateIdentifier,
            NodeData::PrivateIdentifier(TextData {
                text: text.to_string(),
            }),
        )
    }

    /// Creates a keyword expression of the given `kind` (`this`, `super`, ...).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewKeywordExpression
    pub fn new_keyword_expression(&mut self, kind: Kind) -> NodeId {
        self.new_node(kind, NodeData::KeywordExpression)
    }

    /// Creates a string literal node.
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewStringLiteral
    pub fn new_string_literal(&mut self, text: &str, token_flags: TokenFlags) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::StringLiteral,
            NodeData::StringLiteral(LiteralData {
                text: text.to_string(),
                token_flags,
            }),
        )
    }

    /// Creates a numeric literal node.
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewNumericLiteral
    pub fn new_numeric_literal(&mut self, text: &str, token_flags: TokenFlags) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::NumericLiteral,
            NodeData::NumericLiteral(LiteralData {
                text: text.to_string(),
                token_flags,
            }),
        )
    }

    /// Creates a big-int literal node.
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewBigIntLiteral
    pub fn new_big_int_literal(&mut self, text: &str, token_flags: TokenFlags) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::BigIntLiteral,
            NodeData::BigIntLiteral(LiteralData {
                text: text.to_string(),
                token_flags,
            }),
        )
    }

    /// Creates a qualified name (`left.right`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewQualifiedName
    pub fn new_qualified_name(&mut self, left: NodeId, right: NodeId) -> NodeId {
        self.new_node(
            Kind::QualifiedName,
            NodeData::QualifiedName(QualifiedNameData { left, right }),
        )
    }

    /// Creates a property access (`expression.name`, optionally `?.`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewPropertyAccessExpression
    pub fn new_property_access_expression(
        &mut self,
        expression: NodeId,
        question_dot_token: Option<NodeId>,
        name: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::PropertyAccessExpression,
            NodeData::PropertyAccessExpression(PropertyAccessData {
                expression,
                question_dot_token,
                name,
            }),
        )
    }

    /// Creates an element access (`expression[argument]`, optionally `?.`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewElementAccessExpression
    pub fn new_element_access_expression(
        &mut self,
        expression: NodeId,
        question_dot_token: Option<NodeId>,
        argument_expression: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ElementAccessExpression,
            NodeData::ElementAccessExpression(ElementAccessData {
                expression,
                question_dot_token,
                argument_expression,
            }),
        )
    }

    /// Creates a call expression. `flags` contributes only its optional-chain bit.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewCallExpression
    pub fn new_call_expression(
        &mut self,
        expression: NodeId,
        question_dot_token: Option<NodeId>,
        type_arguments: Option<NodeList>,
        arguments: NodeList,
        flags: NodeFlags,
    ) -> NodeId {
        let id = self.new_node(
            Kind::CallExpression,
            NodeData::CallExpression(CallExpressionData {
                expression,
                question_dot_token,
                type_arguments,
                arguments,
            }),
        );
        self.nodes[id.index()].flags |= flags & NodeFlags::OPTIONAL_CHAIN;
        id
    }

    /// Creates a `new` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNewExpression
    pub fn new_new_expression(
        &mut self,
        expression: NodeId,
        type_arguments: Option<NodeList>,
        arguments: Option<NodeList>,
    ) -> NodeId {
        self.new_node(
            Kind::NewExpression,
            NodeData::NewExpression(NewExpressionData {
                expression,
                type_arguments,
                arguments,
            }),
        )
    }

    /// Creates a parenthesized expression (`(expression)`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewParenthesizedExpression
    pub fn new_parenthesized_expression(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::ParenthesizedExpression,
            NodeData::ParenthesizedExpression(UnaryChildData { expression }),
        )
    }

    /// Creates a prefix unary expression (`operator operand`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewPrefixUnaryExpression
    pub fn new_prefix_unary_expression(&mut self, operator: Kind, operand: NodeId) -> NodeId {
        self.new_node(
            Kind::PrefixUnaryExpression,
            NodeData::PrefixUnaryExpression(UnaryOperatorData { operator, operand }),
        )
    }

    /// Creates a postfix unary expression (`operand operator`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewPostfixUnaryExpression
    pub fn new_postfix_unary_expression(&mut self, operand: NodeId, operator: Kind) -> NodeId {
        self.new_node(
            Kind::PostfixUnaryExpression,
            NodeData::PostfixUnaryExpression(UnaryOperatorData { operator, operand }),
        )
    }

    /// Creates a binary expression (`left op right`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewBinaryExpression
    pub fn new_binary_expression(
        &mut self,
        left: NodeId,
        operator_token: NodeId,
        right: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::BinaryExpression,
            NodeData::BinaryExpression(BinaryExpressionData {
                left,
                operator_token,
                right,
            }),
        )
    }

    /// Creates a spread element (`...expression`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewSpreadElement
    pub fn new_spread_element(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::SpreadElement,
            NodeData::SpreadElement(UnaryChildData { expression }),
        )
    }

    /// Creates an array literal (`[elements]`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewArrayLiteralExpression
    pub fn new_array_literal_expression(&mut self, elements: NodeList) -> NodeId {
        self.new_node(
            Kind::ArrayLiteralExpression,
            NodeData::ArrayLiteralExpression(ListData { list: elements }),
        )
    }

    /// Creates a block (`{ statements }`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewBlock
    pub fn new_block(&mut self, statements: NodeList) -> NodeId {
        self.new_node(Kind::Block, NodeData::Block(ListData { list: statements }))
    }

    /// Creates an expression statement (`expression;`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewExpressionStatement
    pub fn new_expression_statement(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::ExpressionStatement,
            NodeData::ExpressionStatement(UnaryChildData { expression }),
        )
    }

    /// Creates a `return` statement with an optional argument.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewReturnStatement
    pub fn new_return_statement(&mut self, expression: Option<NodeId>) -> NodeId {
        self.new_node(
            Kind::ReturnStatement,
            NodeData::ReturnStatement(OptionalChildData { expression }),
        )
    }

    /// Creates an empty statement (`;`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewEmptyStatement
    pub fn new_empty_statement(&mut self) -> NodeId {
        self.new_node(Kind::EmptyStatement, NodeData::EmptyStatement)
    }

    /// Creates a conditional expression (`condition ? when_true : when_false`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewConditionalExpression
    pub fn new_conditional_expression(
        &mut self,
        condition: NodeId,
        question_token: NodeId,
        when_true: NodeId,
        colon_token: NodeId,
        when_false: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ConditionalExpression,
            NodeData::ConditionalExpression(ConditionalExpressionData {
                condition,
                question_token,
                when_true,
                colon_token,
                when_false,
            }),
        )
    }

    /// Creates a `throw` statement (`throw expression;`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewThrowStatement
    pub fn new_throw_statement(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::ThrowStatement,
            NodeData::ThrowStatement(UnaryChildData { expression }),
        )
    }

    /// Creates an `if`/`else` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewIfStatement
    pub fn new_if_statement(
        &mut self,
        expression: NodeId,
        then_statement: NodeId,
        else_statement: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::IfStatement,
            NodeData::IfStatement(IfStatementData {
                expression,
                then_statement,
                else_statement,
            }),
        )
    }

    /// Creates a `do ... while` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewDoStatement
    pub fn new_do_statement(&mut self, statement: NodeId, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::DoStatement,
            NodeData::DoStatement(DoStatementData {
                statement,
                expression,
            }),
        )
    }

    /// Creates a `while` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewWhileStatement
    pub fn new_while_statement(&mut self, expression: NodeId, statement: NodeId) -> NodeId {
        self.new_node(
            Kind::WhileStatement,
            NodeData::WhileStatement(WhileStatementData {
                expression,
                statement,
            }),
        )
    }

    /// Creates a `with` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewWithStatement
    pub fn new_with_statement(&mut self, expression: NodeId, statement: NodeId) -> NodeId {
        self.new_node(
            Kind::WithStatement,
            NodeData::WithStatement(WithStatementData {
                expression,
                statement,
            }),
        )
    }

    /// Creates a `switch` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewSwitchStatement
    pub fn new_switch_statement(&mut self, expression: NodeId, case_block: NodeId) -> NodeId {
        self.new_node(
            Kind::SwitchStatement,
            NodeData::SwitchStatement(SwitchStatementData {
                expression,
                case_block,
            }),
        )
    }

    /// Creates a `switch` case block.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewCaseBlock
    pub fn new_case_block(&mut self, clauses: NodeList) -> NodeId {
        self.new_node(
            Kind::CaseBlock,
            NodeData::CaseBlock(CaseBlockData { clauses }),
        )
    }

    /// Creates a `case`/`default` clause of the given `kind`.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewCaseOrDefaultClause
    pub fn new_case_or_default_clause(
        &mut self,
        kind: Kind,
        expression: Option<NodeId>,
        statements: NodeList,
    ) -> NodeId {
        self.new_node(
            kind,
            NodeData::CaseOrDefaultClause(CaseOrDefaultClauseData {
                expression,
                statements,
            }),
        )
    }

    /// Creates a `break` statement with an optional label.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewBreakStatement
    pub fn new_break_statement(&mut self, label: Option<NodeId>) -> NodeId {
        self.new_node(
            Kind::BreakStatement,
            NodeData::BreakStatement(LabelData { label }),
        )
    }

    /// Creates a `continue` statement with an optional label.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewContinueStatement
    pub fn new_continue_statement(&mut self, label: Option<NodeId>) -> NodeId {
        self.new_node(
            Kind::ContinueStatement,
            NodeData::ContinueStatement(LabelData { label }),
        )
    }

    /// Creates a labeled statement (`label: statement`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewLabeledStatement
    pub fn new_labeled_statement(&mut self, label: NodeId, statement: NodeId) -> NodeId {
        self.new_node(
            Kind::LabeledStatement,
            NodeData::LabeledStatement(LabeledStatementData { label, statement }),
        )
    }

    /// Creates a `debugger` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewDebuggerStatement
    pub fn new_debugger_statement(&mut self) -> NodeId {
        self.new_node(Kind::DebuggerStatement, NodeData::DebuggerStatement)
    }

    /// Creates a `var`/`let`/`const` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewVariableStatement
    pub fn new_variable_statement(
        &mut self,
        modifiers: Option<ModifierList>,
        declaration_list: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::VariableStatement,
            NodeData::VariableStatement(VariableStatementData {
                modifiers,
                declaration_list,
            }),
        )
    }

    /// Creates a variable declaration list. The `let`/`const`/`using` kind is
    /// carried by the caller via [`NodeArena::add_flags`].
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewVariableDeclarationList
    pub fn new_variable_declaration_list(&mut self, declarations: NodeList) -> NodeId {
        self.new_node(
            Kind::VariableDeclarationList,
            NodeData::VariableDeclarationList(VariableDeclarationListData { declarations }),
        )
    }

    /// Creates a single variable declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewVariableDeclaration
    pub fn new_variable_declaration(
        &mut self,
        name: NodeId,
        exclamation_token: Option<NodeId>,
        type_node: Option<NodeId>,
        initializer: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::VariableDeclaration,
            NodeData::VariableDeclaration(VariableDeclarationData {
                name,
                exclamation_token,
                type_node,
                initializer,
            }),
        )
    }

    /// Creates an object/array binding pattern of the given `kind`.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewBindingPattern
    pub fn new_binding_pattern(&mut self, kind: Kind, elements: NodeList) -> NodeId {
        let data = NodeData::ObjectBindingPattern(BindingPatternData {
            elements: elements.clone(),
        });
        let data = if kind == Kind::ArrayBindingPattern {
            NodeData::ArrayBindingPattern(BindingPatternData { elements })
        } else {
            data
        };
        self.new_node(kind, data)
    }

    /// Creates a binding element within a binding pattern.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewBindingElement
    pub fn new_binding_element(
        &mut self,
        dot_dot_dot_token: Option<NodeId>,
        property_name: Option<NodeId>,
        name: Option<NodeId>,
        initializer: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::BindingElement,
            NodeData::BindingElement(BindingElementData {
                dot_dot_dot_token,
                property_name,
                name,
                initializer,
            }),
        )
    }

    /// Creates an omitted (elided) array element.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewOmittedExpression
    pub fn new_omitted_expression(&mut self) -> NodeId {
        self.new_node(Kind::OmittedExpression, NodeData::OmittedExpression)
    }

    /// Creates a computed property name (`[expression]`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewComputedPropertyName
    pub fn new_computed_property_name(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::ComputedPropertyName,
            NodeData::ComputedPropertyName(UnaryChildData { expression }),
        )
    }

    /// Creates a C-style `for` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewForStatement
    pub fn new_for_statement(
        &mut self,
        initializer: Option<NodeId>,
        condition: Option<NodeId>,
        incrementor: Option<NodeId>,
        statement: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ForStatement,
            NodeData::ForStatement(ForStatementData {
                initializer,
                condition,
                incrementor,
                statement,
            }),
        )
    }

    /// Creates a `for-in`/`for-of` statement of the given `kind`.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewForInOrOfStatement
    pub fn new_for_in_or_of_statement(
        &mut self,
        kind: Kind,
        await_modifier: Option<NodeId>,
        initializer: NodeId,
        expression: NodeId,
        statement: NodeId,
    ) -> NodeId {
        self.new_node(
            kind,
            NodeData::ForInOrOfStatement(ForInOrOfStatementData {
                await_modifier,
                initializer,
                expression,
                statement,
            }),
        )
    }

    /// Creates a `try`/`catch`/`finally` statement.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTryStatement
    pub fn new_try_statement(
        &mut self,
        try_block: NodeId,
        catch_clause: Option<NodeId>,
        finally_block: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::TryStatement,
            NodeData::TryStatement(TryStatementData {
                try_block,
                catch_clause,
                finally_block,
            }),
        )
    }

    /// Creates a `catch` clause.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewCatchClause
    pub fn new_catch_clause(
        &mut self,
        variable_declaration: Option<NodeId>,
        block: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::CatchClause,
            NodeData::CatchClause(CatchClauseData {
                variable_declaration,
                block,
            }),
        )
    }

    /// Creates a function declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewFunctionDeclaration
    #[allow(clippy::too_many_arguments)]
    pub fn new_function_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        asterisk_token: Option<NodeId>,
        name: Option<NodeId>,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
        full_signature: Option<NodeId>,
        body: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::FunctionDeclaration,
            NodeData::FunctionDeclaration(Box::new(FunctionDeclarationData {
                modifiers,
                asterisk_token,
                name,
                type_parameters,
                parameters,
                type_node,
                full_signature,
                body,
            })),
        )
    }

    /// Creates a function parameter.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewParameterDeclaration
    pub fn new_parameter_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        dot_dot_dot_token: Option<NodeId>,
        name: NodeId,
        question_token: Option<NodeId>,
        type_node: Option<NodeId>,
        initializer: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::Parameter,
            NodeData::ParameterDeclaration(Box::new(ParameterDeclarationData {
                modifiers,
                dot_dot_dot_token,
                name,
                question_token,
                type_node,
                initializer,
            })),
        )
    }

    /// Creates a generic type parameter.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeParameterDeclaration
    pub fn new_type_parameter_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        constraint: Option<NodeId>,
        expression: Option<NodeId>,
        default_type: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::TypeParameter,
            NodeData::TypeParameterDeclaration(Box::new(TypeParameterData {
                modifiers,
                name,
                constraint,
                expression,
                default_type,
            })),
        )
    }

    /// Creates a class declaration/expression of the given `kind`.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewClassDeclaration / NewClassExpression
    pub fn new_class_like(
        &mut self,
        kind: Kind,
        modifiers: Option<ModifierList>,
        name: Option<NodeId>,
        type_parameters: Option<NodeList>,
        heritage_clauses: Option<NodeList>,
        members: NodeList,
    ) -> NodeId {
        let data = Box::new(ClassLikeData {
            modifiers,
            name,
            type_parameters,
            heritage_clauses,
            members,
        });
        let data = if kind == Kind::ClassExpression {
            NodeData::ClassExpression(data)
        } else {
            NodeData::ClassDeclaration(data)
        };
        self.new_node(kind, data)
    }

    /// Creates a heritage clause (`extends`/`implements`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewHeritageClause
    pub fn new_heritage_clause(&mut self, token: Kind, types: NodeList) -> NodeId {
        self.new_node(
            Kind::HeritageClause,
            NodeData::HeritageClause(HeritageClauseData { token, types }),
        )
    }

    /// Creates an expression-with-type-arguments heritage element.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewExpressionWithTypeArguments
    pub fn new_expression_with_type_arguments(
        &mut self,
        expression: NodeId,
        type_arguments: Option<NodeList>,
    ) -> NodeId {
        self.new_node(
            Kind::ExpressionWithTypeArguments,
            NodeData::ExpressionWithTypeArguments(ExprWithTypeArgsData {
                expression,
                type_arguments,
            }),
        )
    }

    /// Creates a method declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewMethodDeclaration
    #[allow(clippy::too_many_arguments)]
    pub fn new_method_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        asterisk_token: Option<NodeId>,
        name: NodeId,
        postfix_token: Option<NodeId>,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
        full_signature: Option<NodeId>,
        body: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::MethodDeclaration,
            NodeData::MethodDeclaration(Box::new(MethodLikeData {
                modifiers,
                asterisk_token,
                name,
                postfix_token,
                type_parameters,
                parameters,
                type_node,
                full_signature,
                body,
            })),
        )
    }

    /// Creates a class property declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewPropertyDeclaration
    pub fn new_property_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        postfix_token: Option<NodeId>,
        type_node: Option<NodeId>,
        initializer: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::PropertyDeclaration,
            NodeData::PropertyDeclaration(Box::new(PropertyDeclarationData {
                modifiers,
                name,
                postfix_token,
                type_node,
                initializer,
            })),
        )
    }

    /// Creates a `get`/`set` accessor declaration of the given `kind`.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewGetAccessorDeclaration / NewSetAccessorDeclaration
    #[allow(clippy::too_many_arguments)]
    pub fn new_accessor_declaration(
        &mut self,
        kind: Kind,
        modifiers: Option<ModifierList>,
        name: NodeId,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
        full_signature: Option<NodeId>,
        body: Option<NodeId>,
    ) -> NodeId {
        let data = Box::new(AccessorData {
            modifiers,
            name,
            type_parameters,
            parameters,
            type_node,
            full_signature,
            body,
        });
        let data = if kind == Kind::SetAccessor {
            NodeData::SetAccessorDeclaration(data)
        } else {
            NodeData::GetAccessorDeclaration(data)
        };
        self.new_node(kind, data)
    }

    /// Creates a class constructor declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewConstructorDeclaration
    pub fn new_constructor_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
        full_signature: Option<NodeId>,
        body: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::Constructor,
            NodeData::ConstructorDeclaration(Box::new(ConstructorData {
                modifiers,
                type_parameters,
                parameters,
                type_node,
                full_signature,
                body,
            })),
        )
    }

    /// Creates an index signature declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewIndexSignatureDeclaration
    pub fn new_index_signature_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::IndexSignature,
            NodeData::IndexSignatureDeclaration(IndexSignatureData {
                modifiers,
                parameters,
                type_node,
            }),
        )
    }

    /// Creates a class static block declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewClassStaticBlockDeclaration
    pub fn new_class_static_block_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        body: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ClassStaticBlockDeclaration,
            NodeData::ClassStaticBlockDeclaration(ClassStaticBlockData { modifiers, body }),
        )
    }

    /// Creates a lone `;` class element.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewSemicolonClassElement
    pub fn new_semicolon_class_element(&mut self) -> NodeId {
        self.new_node(Kind::SemicolonClassElement, NodeData::SemicolonClassElement)
    }

    /// Creates an interface declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewInterfaceDeclaration
    pub fn new_interface_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        name: Option<NodeId>,
        type_parameters: Option<NodeList>,
        heritage_clauses: Option<NodeList>,
        members: NodeList,
    ) -> NodeId {
        self.new_node(
            Kind::InterfaceDeclaration,
            NodeData::InterfaceDeclaration(Box::new(ClassLikeData {
                modifiers,
                name,
                type_parameters,
                heritage_clauses,
                members,
            })),
        )
    }

    /// Creates a type alias declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeAliasDeclaration
    pub fn new_type_alias_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        type_parameters: Option<NodeList>,
        type_node: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::TypeAliasDeclaration,
            NodeData::TypeAliasDeclaration(Box::new(TypeAliasData {
                modifiers,
                name,
                type_parameters,
                type_node,
            })),
        )
    }

    /// Creates an enum declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewEnumDeclaration
    pub fn new_enum_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        members: NodeList,
    ) -> NodeId {
        self.new_node(
            Kind::EnumDeclaration,
            NodeData::EnumDeclaration(Box::new(EnumDeclData {
                modifiers,
                name,
                members,
            })),
        )
    }

    /// Creates an enum member.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewEnumMember
    pub fn new_enum_member(&mut self, name: NodeId, initializer: Option<NodeId>) -> NodeId {
        self.new_node(
            Kind::EnumMember,
            NodeData::EnumMember(EnumMemberData { name, initializer }),
        )
    }

    /// Creates a property signature (interface/type-literal member).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewPropertySignatureDeclaration
    pub fn new_property_signature(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        postfix_token: Option<NodeId>,
        type_node: Option<NodeId>,
        initializer: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::PropertySignature,
            NodeData::PropertySignature(Box::new(PropertyDeclarationData {
                modifiers,
                name,
                postfix_token,
                type_node,
                initializer,
            })),
        )
    }

    /// Creates a method signature (interface/type-literal member).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewMethodSignatureDeclaration
    pub fn new_method_signature(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        postfix_token: Option<NodeId>,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::MethodSignature,
            NodeData::MethodSignature(Box::new(MethodSignatureData {
                modifiers,
                name,
                postfix_token,
                type_parameters,
                parameters,
                type_node,
            })),
        )
    }

    /// Creates a call/construct signature member of the given `kind`.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewCallSignatureDeclaration / NewConstructSignatureDeclaration
    pub fn new_signature_declaration(
        &mut self,
        kind: Kind,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
    ) -> NodeId {
        let data = SignatureDeclData {
            type_parameters,
            parameters,
            type_node,
        };
        let data = if kind == Kind::ConstructSignature {
            NodeData::ConstructSignature(data)
        } else {
            NodeData::CallSignature(data)
        };
        self.new_node(kind, data)
    }

    /// Creates a type literal (`{ members }`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeLiteralNode
    pub fn new_type_literal_node(&mut self, members: NodeList) -> NodeId {
        self.new_node(
            Kind::TypeLiteral,
            NodeData::TypeLiteral(TypeLiteralData { members }),
        )
    }

    /// Creates a `module`/`namespace`/`global` declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewModuleDeclaration
    pub fn new_module_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        keyword: Kind,
        name: NodeId,
        body: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::ModuleDeclaration,
            NodeData::ModuleDeclaration(Box::new(ModuleDeclData {
                modifiers,
                keyword,
                name,
                body,
            })),
        )
    }

    /// Creates a module/namespace body block.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewModuleBlock
    pub fn new_module_block(&mut self, statements: NodeList) -> NodeId {
        self.new_node(
            Kind::ModuleBlock,
            NodeData::ModuleBlock(ModuleBlockData { statements }),
        )
    }

    /// Creates a template head/middle/tail literal node of the given `kind`.
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewTemplateHead/Middle/Tail
    pub fn new_template_literal_like_node(
        &mut self,
        kind: Kind,
        text: &str,
        token_flags: TokenFlags,
    ) -> NodeId {
        self.text_count += 1;
        let data = LiteralData {
            text: text.to_string(),
            token_flags,
        };
        let data = match kind {
            Kind::TemplateMiddle => NodeData::TemplateMiddle(data),
            Kind::TemplateTail => NodeData::TemplateTail(data),
            _ => NodeData::TemplateHead(data),
        };
        self.new_node(kind, data)
    }

    /// Creates an `expr as Type` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewAsExpression
    pub fn new_as_expression(&mut self, expression: NodeId, type_node: NodeId) -> NodeId {
        self.new_node(
            Kind::AsExpression,
            NodeData::AsExpression(ExprTypeData {
                expression,
                type_node,
            }),
        )
    }

    /// Creates an `expr satisfies Type` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewSatisfiesExpression
    pub fn new_satisfies_expression(&mut self, expression: NodeId, type_node: NodeId) -> NodeId {
        self.new_node(
            Kind::SatisfiesExpression,
            NodeData::SatisfiesExpression(ExprTypeData {
                expression,
                type_node,
            }),
        )
    }

    /// Creates a `<Type>expr` type-assertion expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeAssertion
    pub fn new_type_assertion(&mut self, type_node: NodeId, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::TypeAssertionExpression,
            NodeData::TypeAssertionExpression(TypeAssertionData {
                type_node,
                expression,
            }),
        )
    }

    /// Creates a non-null assertion (`expression!`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNonNullExpression
    pub fn new_non_null_expression(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::NonNullExpression,
            NodeData::NonNullExpression(UnaryChildData { expression }),
        )
    }

    /// Creates a decorator (`@expression`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewDecorator
    pub fn new_decorator(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::Decorator,
            NodeData::Decorator(UnaryChildData { expression }),
        )
    }

    /// Creates a JSX element (`<a>...</a>`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxElement
    pub fn new_jsx_element(
        &mut self,
        opening_element: NodeId,
        children: NodeList,
        closing_element: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::JsxElement,
            NodeData::JsxElement(JsxElementData {
                opening: opening_element,
                children,
                closing: closing_element,
            }),
        )
    }

    /// Creates a JSX fragment (`<>...</>`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxFragment
    pub fn new_jsx_fragment(
        &mut self,
        opening_fragment: NodeId,
        children: NodeList,
        closing_fragment: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::JsxFragment,
            NodeData::JsxFragment(JsxElementData {
                opening: opening_fragment,
                children,
                closing: closing_fragment,
            }),
        )
    }

    /// Creates a JSX opening element (`<a>`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxOpeningElement
    pub fn new_jsx_opening_element(
        &mut self,
        tag_name: NodeId,
        type_arguments: Option<NodeList>,
        attributes: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::JsxOpeningElement,
            NodeData::JsxOpeningElement(Box::new(JsxOpeningLikeData {
                tag_name,
                type_arguments,
                attributes,
            })),
        )
    }

    /// Creates a self-closing JSX element (`<a />`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxSelfClosingElement
    pub fn new_jsx_self_closing_element(
        &mut self,
        tag_name: NodeId,
        type_arguments: Option<NodeList>,
        attributes: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::JsxSelfClosingElement,
            NodeData::JsxSelfClosingElement(Box::new(JsxOpeningLikeData {
                tag_name,
                type_arguments,
                attributes,
            })),
        )
    }

    /// Creates a JSX closing element (`</a>`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxClosingElement
    pub fn new_jsx_closing_element(&mut self, tag_name: NodeId) -> NodeId {
        self.new_node(
            Kind::JsxClosingElement,
            NodeData::JsxClosingElement(JsxClosingElementData { tag_name }),
        )
    }

    /// Creates a JSX opening fragment (`<>`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxOpeningFragment
    pub fn new_jsx_opening_fragment(&mut self) -> NodeId {
        self.new_node(Kind::JsxOpeningFragment, NodeData::JsxOpeningFragment)
    }

    /// Creates a JSX closing fragment (`</>`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxClosingFragment
    pub fn new_jsx_closing_fragment(&mut self) -> NodeId {
        self.new_node(Kind::JsxClosingFragment, NodeData::JsxClosingFragment)
    }

    /// Creates a JSX attributes list.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxAttributes
    pub fn new_jsx_attributes(&mut self, properties: NodeList) -> NodeId {
        self.new_node(
            Kind::JsxAttributes,
            NodeData::JsxAttributes(ListData { list: properties }),
        )
    }

    /// Creates a JSX attribute (`name={value}`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxAttribute
    pub fn new_jsx_attribute(&mut self, name: NodeId, initializer: Option<NodeId>) -> NodeId {
        self.new_node(
            Kind::JsxAttribute,
            NodeData::JsxAttribute(JsxAttributeData { name, initializer }),
        )
    }

    /// Creates a JSX spread attribute (`{...expr}`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxSpreadAttribute
    pub fn new_jsx_spread_attribute(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::JsxSpreadAttribute,
            NodeData::JsxSpreadAttribute(UnaryChildData { expression }),
        )
    }

    /// Creates a JSX namespaced name (`ns:name`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxNamespacedName
    pub fn new_jsx_namespaced_name(&mut self, namespace: NodeId, name: NodeId) -> NodeId {
        self.new_node(
            Kind::JsxNamespacedName,
            NodeData::JsxNamespacedName(JsxNamespacedNameData { namespace, name }),
        )
    }

    /// Creates a JSX expression container (`{expr}`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewJsxExpression
    pub fn new_jsx_expression(
        &mut self,
        dot_dot_dot_token: Option<NodeId>,
        expression: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::JsxExpression,
            NodeData::JsxExpression(JsxExpressionData {
                dot_dot_dot_token,
                expression,
            }),
        )
    }

    /// Creates a JSX text node.
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewJsxText
    pub fn new_jsx_text(&mut self, text: &str, contains_only_trivia_white_spaces: bool) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::JsxText,
            NodeData::JsxText(JsxTextData {
                text: text.to_string(),
                contains_only_trivia_white_spaces,
            }),
        )
    }

    /// Creates an import attributes clause (`with { ... }`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewImportAttributes
    pub fn new_import_attributes(
        &mut self,
        token: Kind,
        attributes: NodeList,
        multiline: bool,
    ) -> NodeId {
        self.new_node(
            Kind::ImportAttributes,
            NodeData::ImportAttributes(ImportAttributesData {
                token,
                attributes,
                multiline,
            }),
        )
    }

    /// Creates a single import attribute (`name: value`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewImportAttribute
    pub fn new_import_attribute(&mut self, name: Option<NodeId>, value: NodeId) -> NodeId {
        self.new_node(
            Kind::ImportAttribute,
            NodeData::ImportAttribute(ImportAttributeData { name, value }),
        )
    }

    /// Creates a meta-property (`new.target` / `import.meta`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewMetaProperty
    pub fn new_meta_property(&mut self, keyword_token: Kind, name: NodeId) -> NodeId {
        self.new_node(
            Kind::MetaProperty,
            NodeData::MetaProperty(MetaPropertyData {
                keyword_token,
                name,
            }),
        )
    }

    /// Creates a template expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTemplateExpression
    pub fn new_template_expression(&mut self, head: NodeId, template_spans: NodeList) -> NodeId {
        self.new_node(
            Kind::TemplateExpression,
            NodeData::TemplateExpression(TemplateExpressionData {
                head,
                template_spans,
            }),
        )
    }

    /// Creates a template span.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTemplateSpan
    pub fn new_template_span(&mut self, expression: NodeId, literal: NodeId) -> NodeId {
        self.new_node(
            Kind::TemplateSpan,
            NodeData::TemplateSpan(TemplateSpanData {
                expression,
                literal,
            }),
        )
    }

    /// Creates a tagged template expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTaggedTemplateExpression
    pub fn new_tagged_template_expression(
        &mut self,
        tag: NodeId,
        question_dot_token: Option<NodeId>,
        type_arguments: Option<NodeList>,
        template: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::TaggedTemplateExpression,
            NodeData::TaggedTemplateExpression(Box::new(TaggedTemplateData {
                tag,
                question_dot_token,
                type_arguments,
                template,
            })),
        )
    }

    /// Creates a regular-expression literal.
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewRegularExpressionLiteral
    pub fn new_regular_expression_literal(
        &mut self,
        text: &str,
        token_flags: TokenFlags,
    ) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::RegularExpressionLiteral,
            NodeData::RegularExpressionLiteral(LiteralData {
                text: text.to_string(),
                token_flags,
            }),
        )
    }

    /// Creates a no-substitution template literal.
    ///
    /// Side effects: pushes a node; bumps `text_count`.
    // Go: internal/ast/ast_generated.go:NewNoSubstitutionTemplateLiteral
    pub fn new_no_substitution_template_literal(
        &mut self,
        text: &str,
        token_flags: TokenFlags,
    ) -> NodeId {
        self.text_count += 1;
        self.new_node(
            Kind::NoSubstitutionTemplateLiteral,
            NodeData::NoSubstitutionTemplateLiteral(LiteralData {
                text: text.to_string(),
                token_flags,
            }),
        )
    }

    /// Creates an arrow function.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewArrowFunction
    #[allow(clippy::too_many_arguments)]
    pub fn new_arrow_function(
        &mut self,
        modifiers: Option<ModifierList>,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
        full_signature: Option<NodeId>,
        equals_greater_than_token: NodeId,
        body: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ArrowFunction,
            NodeData::ArrowFunction(Box::new(ArrowFunctionData {
                modifiers,
                type_parameters,
                parameters,
                type_node,
                full_signature,
                equals_greater_than_token,
                body,
            })),
        )
    }

    /// Creates a `delete` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewDeleteExpression
    pub fn new_delete_expression(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::DeleteExpression,
            NodeData::DeleteExpression(UnaryChildData { expression }),
        )
    }

    /// Creates a `typeof` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeOfExpression
    pub fn new_type_of_expression(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::TypeOfExpression,
            NodeData::TypeOfExpression(UnaryChildData { expression }),
        )
    }

    /// Creates a `void` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewVoidExpression
    pub fn new_void_expression(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::VoidExpression,
            NodeData::VoidExpression(UnaryChildData { expression }),
        )
    }

    /// Creates an `await` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewAwaitExpression
    pub fn new_await_expression(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::AwaitExpression,
            NodeData::AwaitExpression(UnaryChildData { expression }),
        )
    }

    /// Creates a `yield` expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewYieldExpression
    pub fn new_yield_expression(
        &mut self,
        asterisk_token: Option<NodeId>,
        expression: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::YieldExpression,
            NodeData::YieldExpression(YieldExpressionData {
                asterisk_token,
                expression,
            }),
        )
    }

    /// Creates an object literal expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewObjectLiteralExpression
    pub fn new_object_literal_expression(&mut self, properties: NodeList) -> NodeId {
        self.new_node(
            Kind::ObjectLiteralExpression,
            NodeData::ObjectLiteralExpression(ListData { list: properties }),
        )
    }

    /// Creates a property assignment (`name: initializer`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewPropertyAssignment
    pub fn new_property_assignment(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        postfix_token: Option<NodeId>,
        type_node: Option<NodeId>,
        initializer: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::PropertyAssignment,
            NodeData::PropertyAssignment(Box::new(PropertyDeclarationData {
                modifiers,
                name,
                postfix_token,
                type_node,
                initializer,
            })),
        )
    }

    /// Creates a shorthand property assignment (`name` / `name = init`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewShorthandPropertyAssignment
    pub fn new_shorthand_property_assignment(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
        postfix_token: Option<NodeId>,
        type_node: Option<NodeId>,
        equals_token: Option<NodeId>,
        object_assignment_initializer: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::ShorthandPropertyAssignment,
            NodeData::ShorthandPropertyAssignment(Box::new(ShorthandPropertyAssignmentData {
                modifiers,
                name,
                postfix_token,
                type_node,
                equals_token,
                object_assignment_initializer,
            })),
        )
    }

    /// Creates a spread assignment (`...expression`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewSpreadAssignment
    pub fn new_spread_assignment(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::SpreadAssignment,
            NodeData::SpreadAssignment(UnaryChildData { expression }),
        )
    }

    /// Creates a function expression.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewFunctionExpression
    #[allow(clippy::too_many_arguments)]
    pub fn new_function_expression(
        &mut self,
        modifiers: Option<ModifierList>,
        asterisk_token: Option<NodeId>,
        name: Option<NodeId>,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
        full_signature: Option<NodeId>,
        body: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::FunctionExpression,
            NodeData::FunctionExpression(Box::new(FunctionDeclarationData {
                modifiers,
                asterisk_token,
                name,
                type_parameters,
                parameters,
                type_node,
                full_signature,
                body,
            })),
        )
    }

    /// Creates an import declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewImportDeclaration
    pub fn new_import_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        import_clause: Option<NodeId>,
        module_specifier: NodeId,
        attributes: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::ImportDeclaration,
            NodeData::ImportDeclaration(Box::new(ImportDeclData {
                modifiers,
                import_clause,
                module_specifier,
                attributes,
            })),
        )
    }

    /// Creates an import clause.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewImportClause
    pub fn new_import_clause(
        &mut self,
        phase_modifier: Kind,
        name: Option<NodeId>,
        named_bindings: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::ImportClause,
            NodeData::ImportClause(ImportClauseData {
                phase_modifier,
                name,
                named_bindings,
            }),
        )
    }

    /// Creates a namespace import (`* as name`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNamespaceImport
    pub fn new_namespace_import(&mut self, name: NodeId) -> NodeId {
        self.new_node(
            Kind::NamespaceImport,
            NodeData::NamespaceImport(NameRefData { name }),
        )
    }

    /// Creates a named-imports clause.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNamedImports
    pub fn new_named_imports(&mut self, elements: NodeList) -> NodeId {
        self.new_node(
            Kind::NamedImports,
            NodeData::NamedImports(ElementsData { elements }),
        )
    }

    /// Creates an import specifier.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewImportSpecifier
    pub fn new_import_specifier(
        &mut self,
        is_type_only: bool,
        property_name: Option<NodeId>,
        name: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ImportSpecifier,
            NodeData::ImportSpecifier(Box::new(ImportExportSpecData {
                is_type_only,
                property_name,
                name,
            })),
        )
    }

    /// Creates an external module reference (`require("m")`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewExternalModuleReference
    pub fn new_external_module_reference(&mut self, expression: NodeId) -> NodeId {
        self.new_node(
            Kind::ExternalModuleReference,
            NodeData::ExternalModuleReference(UnaryChildData { expression }),
        )
    }

    /// Creates an `import x = ...` declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewImportEqualsDeclaration
    pub fn new_import_equals_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        is_type_only: bool,
        name: NodeId,
        module_reference: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ImportEqualsDeclaration,
            NodeData::ImportEqualsDeclaration(Box::new(ImportEqualsData {
                modifiers,
                is_type_only,
                name,
                module_reference,
            })),
        )
    }

    /// Creates an export declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewExportDeclaration
    pub fn new_export_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        is_type_only: bool,
        export_clause: Option<NodeId>,
        module_specifier: Option<NodeId>,
        attributes: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::ExportDeclaration,
            NodeData::ExportDeclaration(Box::new(ExportDeclData {
                modifiers,
                is_type_only,
                export_clause,
                module_specifier,
                attributes,
            })),
        )
    }

    /// Creates a named-exports clause.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNamedExports
    pub fn new_named_exports(&mut self, elements: NodeList) -> NodeId {
        self.new_node(
            Kind::NamedExports,
            NodeData::NamedExports(ElementsData { elements }),
        )
    }

    /// Creates a namespace export (`* as name`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNamespaceExport
    pub fn new_namespace_export(&mut self, name: NodeId) -> NodeId {
        self.new_node(
            Kind::NamespaceExport,
            NodeData::NamespaceExport(NameRefData { name }),
        )
    }

    /// Creates an export specifier.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewExportSpecifier
    pub fn new_export_specifier(
        &mut self,
        is_type_only: bool,
        property_name: Option<NodeId>,
        name: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ExportSpecifier,
            NodeData::ExportSpecifier(Box::new(ImportExportSpecData {
                is_type_only,
                property_name,
                name,
            })),
        )
    }

    /// Creates an `export =` / `export default` assignment.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewExportAssignment
    pub fn new_export_assignment(
        &mut self,
        modifiers: Option<ModifierList>,
        is_export_equals: bool,
        type_node: Option<NodeId>,
        expression: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ExportAssignment,
            NodeData::ExportAssignment(Box::new(ExportAssignData {
                modifiers,
                is_export_equals,
                type_node,
                expression,
            })),
        )
    }

    /// Creates an `export as namespace X` declaration.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNamespaceExportDeclaration
    pub fn new_namespace_export_declaration(
        &mut self,
        modifiers: Option<ModifierList>,
        name: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::NamespaceExportDeclaration,
            NodeData::NamespaceExportDeclaration(NamespaceExportDeclData { modifiers, name }),
        )
    }

    /// Creates a type reference (`type_name<type_arguments>`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeReferenceNode
    pub fn new_type_reference_node(
        &mut self,
        type_name: NodeId,
        type_arguments: Option<NodeList>,
    ) -> NodeId {
        self.new_node(
            Kind::TypeReference,
            NodeData::TypeReference(TypeReferenceData {
                type_name,
                type_arguments,
            }),
        )
    }

    /// Creates an array type (`element_type[]`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewArrayTypeNode
    pub fn new_array_type_node(&mut self, element_type: NodeId) -> NodeId {
        self.new_node(
            Kind::ArrayType,
            NodeData::ArrayType(ArrayTypeData { element_type }),
        )
    }

    /// Creates an indexed-access type (`object_type[index_type]`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewIndexedAccessTypeNode
    pub fn new_indexed_access_type_node(
        &mut self,
        object_type: NodeId,
        index_type: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::IndexedAccessType,
            NodeData::IndexedAccessType(IndexedAccessTypeData {
                object_type,
                index_type,
            }),
        )
    }

    /// Creates a union type (`types[0] | types[1] | ...`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewUnionTypeNode
    pub fn new_union_type_node(&mut self, types: NodeList) -> NodeId {
        self.new_node(Kind::UnionType, NodeData::UnionType(TypeListData { types }))
    }

    /// Creates an intersection type (`types[0] & types[1] & ...`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewIntersectionTypeNode
    pub fn new_intersection_type_node(&mut self, types: NodeList) -> NodeId {
        self.new_node(
            Kind::IntersectionType,
            NodeData::IntersectionType(TypeListData { types }),
        )
    }

    /// Creates a function type (`(params) => type`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewFunctionTypeNode
    pub fn new_function_type_node(
        &mut self,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::FunctionType,
            NodeData::FunctionType(Box::new(SignatureTypeData {
                modifiers: None,
                type_parameters,
                parameters,
                type_node,
            })),
        )
    }

    /// Creates a constructor type (`new (params) => type`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewConstructorTypeNode
    pub fn new_constructor_type_node(
        &mut self,
        modifiers: Option<ModifierList>,
        type_parameters: Option<NodeList>,
        parameters: NodeList,
        type_node: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::ConstructorType,
            NodeData::ConstructorType(Box::new(SignatureTypeData {
                modifiers,
                type_parameters,
                parameters,
                type_node,
            })),
        )
    }

    /// Creates a conditional type.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewConditionalTypeNode
    pub fn new_conditional_type_node(
        &mut self,
        check_type: NodeId,
        extends_type: NodeId,
        true_type: NodeId,
        false_type: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::ConditionalType,
            NodeData::ConditionalType(Box::new(ConditionalTypeData {
                check_type,
                extends_type,
                true_type,
                false_type,
            })),
        )
    }

    /// Creates an `infer T` type.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewInferTypeNode
    pub fn new_infer_type_node(&mut self, type_parameter: NodeId) -> NodeId {
        self.new_node(
            Kind::InferType,
            NodeData::InferType(InferTypeData { type_parameter }),
        )
    }

    /// Creates a type operator (`keyof`/`unique`/`readonly` `type`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeOperatorNode
    pub fn new_type_operator_node(&mut self, operator: Kind, type_node: NodeId) -> NodeId {
        self.new_node(
            Kind::TypeOperator,
            NodeData::TypeOperator(TypeOperatorData {
                operator,
                type_node,
            }),
        )
    }

    /// Creates a mapped type.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewMappedTypeNode
    pub fn new_mapped_type_node(
        &mut self,
        readonly_token: Option<NodeId>,
        type_parameter: NodeId,
        name_type: Option<NodeId>,
        question_token: Option<NodeId>,
        type_node: Option<NodeId>,
        members: NodeList,
    ) -> NodeId {
        self.new_node(
            Kind::MappedType,
            NodeData::MappedType(Box::new(MappedTypeData {
                readonly_token,
                type_parameter,
                name_type,
                question_token,
                type_node,
                members,
            })),
        )
    }

    /// Creates a tuple type (`[A, B]`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTupleTypeNode
    pub fn new_tuple_type_node(&mut self, elements: NodeList) -> NodeId {
        self.new_node(
            Kind::TupleType,
            NodeData::TupleType(TypeListData { types: elements }),
        )
    }

    /// Creates a named tuple member (`name: T`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewNamedTupleMember
    pub fn new_named_tuple_member(
        &mut self,
        dot_dot_dot_token: Option<NodeId>,
        name: NodeId,
        question_token: Option<NodeId>,
        type_node: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::NamedTupleMember,
            NodeData::NamedTupleMember(Box::new(NamedTupleMemberData {
                dot_dot_dot_token,
                name,
                question_token,
                type_node,
            })),
        )
    }

    /// Creates a rest tuple element type (`...T`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewRestTypeNode
    pub fn new_rest_type_node(&mut self, type_node: NodeId) -> NodeId {
        self.new_node(
            Kind::RestType,
            NodeData::RestType(ParenTypeData { type_node }),
        )
    }

    /// Creates an optional tuple element type (`T?`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewOptionalTypeNode
    pub fn new_optional_type_node(&mut self, type_node: NodeId) -> NodeId {
        self.new_node(
            Kind::OptionalType,
            NodeData::OptionalType(ParenTypeData { type_node }),
        )
    }

    /// Creates the `this` type.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewThisTypeNode
    pub fn new_this_type_node(&mut self) -> NodeId {
        self.new_node(Kind::ThisType, NodeData::ThisType)
    }

    /// Creates a type query (`typeof X`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypeQueryNode
    pub fn new_type_query_node(
        &mut self,
        expr_name: NodeId,
        type_arguments: Option<NodeList>,
    ) -> NodeId {
        self.new_node(
            Kind::TypeQuery,
            NodeData::TypeQuery(TypeQueryData {
                expr_name,
                type_arguments,
            }),
        )
    }

    /// Creates an import type (`import("m").X`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewImportTypeNode
    pub fn new_import_type_node(
        &mut self,
        is_type_of: bool,
        argument: NodeId,
        attributes: Option<NodeId>,
        qualifier: Option<NodeId>,
        type_arguments: Option<NodeList>,
    ) -> NodeId {
        self.new_node(
            Kind::ImportType,
            NodeData::ImportType(Box::new(ImportTypeData {
                is_type_of,
                argument,
                attributes,
                qualifier,
                type_arguments,
            })),
        )
    }

    /// Creates a template literal type.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTemplateLiteralTypeNode
    pub fn new_template_literal_type_node(
        &mut self,
        head: NodeId,
        template_spans: NodeList,
    ) -> NodeId {
        self.new_node(
            Kind::TemplateLiteralType,
            NodeData::TemplateLiteralType(TemplateExpressionData {
                head,
                template_spans,
            }),
        )
    }

    /// Creates a template literal type span (`${type}literal`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTemplateLiteralTypeSpan
    pub fn new_template_literal_type_span(&mut self, type_node: NodeId, literal: NodeId) -> NodeId {
        // The `expression` field of `TemplateSpanData` holds the span's type here.
        self.new_node(
            Kind::TemplateLiteralTypeSpan,
            NodeData::TemplateLiteralTypeSpan(TemplateSpanData {
                expression: type_node,
                literal,
            }),
        )
    }

    /// Creates a type predicate (`x is T` / `asserts x`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewTypePredicateNode
    pub fn new_type_predicate_node(
        &mut self,
        asserts_modifier: Option<NodeId>,
        parameter_name: NodeId,
        type_node: Option<NodeId>,
    ) -> NodeId {
        self.new_node(
            Kind::TypePredicate,
            NodeData::TypePredicate(Box::new(TypePredicateData {
                asserts_modifier,
                parameter_name,
                type_node,
            })),
        )
    }

    /// Creates a parenthesized type (`(type_node)`).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewParenthesizedTypeNode
    pub fn new_parenthesized_type_node(&mut self, type_node: NodeId) -> NodeId {
        self.new_node(
            Kind::ParenthesizedType,
            NodeData::ParenthesizedType(ParenTypeData { type_node }),
        )
    }

    /// Creates a literal type (`"a"`, `1`, `true`, ...).
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewLiteralTypeNode
    pub fn new_literal_type_node(&mut self, literal: NodeId) -> NodeId {
        self.new_node(
            Kind::LiteralType,
            NodeData::LiteralType(LiteralTypeData { literal }),
        )
    }

    /// Creates a source file root node.
    ///
    /// Side effects: pushes a node.
    // Go: internal/ast/ast_generated.go:NewSourceFile
    pub fn new_source_file(
        &mut self,
        file_name: &str,
        script_kind: ScriptKind,
        language_variant: LanguageVariant,
        statements: NodeList,
        end_of_file_token: NodeId,
    ) -> NodeId {
        self.new_node(
            Kind::SourceFile,
            NodeData::SourceFile(Box::new(SourceFileData {
                statements,
                end_of_file_token,
                file_name: file_name.to_string(),
                script_kind,
                language_variant,
                is_declaration_file: false,
                external_module_indicator: None,
                imports: Vec::new(),
            })),
        )
    }

    /// Reports whether a [`NodeData`] is a text-bearing (identifier/literal) node,
    /// so cloning can mirror Go's `textCount` bookkeeping.
    fn is_text_data(data: &NodeData) -> bool {
        matches!(
            data,
            NodeData::Identifier(_)
                | NodeData::PrivateIdentifier(_)
                | NodeData::StringLiteral(_)
                | NodeData::NumericLiteral(_)
                | NodeData::BigIntLiteral(_)
                | NodeData::RegularExpressionLiteral(_)
                | NodeData::NoSubstitutionTemplateLiteral(_)
                | NodeData::TemplateHead(_)
                | NodeData::TemplateMiddle(_)
                | NodeData::TemplateTail(_)
                | NodeData::JsxText(_)
        )
    }

    /// Shallowly clones node `id`, returning a new id with the same kind, flags,
    /// location, and payload (child ids are shared, not deep-copied).
    ///
    /// Mirrors Go's per-node `Clone`, which reconstructs via `NewXxx` and copies
    /// the original flags/loc; the clone's `parent` is reset to `None`.
    ///
    /// Side effects: pushes a node; bumps `node_count` (and `text_count` for
    /// text-bearing nodes).
    // Go: internal/ast/ast.go:Node.Clone
    pub fn clone_node(&mut self, id: NodeId) -> NodeId {
        let src = &self.nodes[id.index()];
        let kind = src.kind;
        let flags = src.flags;
        let loc = src.loc;
        let data = src.data.clone();
        self.node_count += 1;
        if Self::is_text_data(&data) {
            self.text_count += 1;
        }
        let new_id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            kind,
            flags,
            loc,
            parent: None,
            data,
        });
        new_id
    }

    /// Invokes `f` on each direct child of `id` in source order, stopping early
    /// if `f` returns `true` (which this method then returns).
    ///
    /// Mirrors Go `Node.ForEachChild`: optional children are skipped when absent
    /// and list children are visited element by element.
    ///
    /// Side effects: none beyond invoking `f`.
    // Go: internal/ast/ast.go:Node.ForEachChild
    pub fn for_each_child(&self, id: NodeId, f: &mut dyn FnMut(NodeId) -> bool) -> bool {
        fn opt(f: &mut dyn FnMut(NodeId) -> bool, id: Option<NodeId>) -> bool {
            id.is_some_and(f)
        }
        fn list(f: &mut dyn FnMut(NodeId) -> bool, list: &NodeList) -> bool {
            list.nodes.iter().any(|&c| f(c))
        }
        fn opt_list(f: &mut dyn FnMut(NodeId) -> bool, l: &Option<NodeList>) -> bool {
            l.as_ref().is_some_and(|nl| list(f, nl))
        }
        fn mods(f: &mut dyn FnMut(NodeId) -> bool, m: &Option<ModifierList>) -> bool {
            m.as_ref()
                .is_some_and(|ml| ml.list.nodes.iter().any(|&c| f(c)))
        }
        match &self.nodes[id.index()].data {
            NodeData::Token
            | NodeData::Identifier(_)
            | NodeData::PrivateIdentifier(_)
            | NodeData::StringLiteral(_)
            | NodeData::NumericLiteral(_)
            | NodeData::BigIntLiteral(_)
            | NodeData::RegularExpressionLiteral(_)
            | NodeData::NoSubstitutionTemplateLiteral(_)
            | NodeData::TemplateHead(_)
            | NodeData::TemplateMiddle(_)
            | NodeData::TemplateTail(_)
            | NodeData::KeywordExpression => false,
            NodeData::QualifiedName(d) => f(d.left) || f(d.right),
            NodeData::PropertyAccessExpression(d) => {
                f(d.expression) || opt(f, d.question_dot_token) || f(d.name)
            }
            NodeData::ElementAccessExpression(d) => {
                f(d.expression) || opt(f, d.question_dot_token) || f(d.argument_expression)
            }
            NodeData::CallExpression(d) => {
                f(d.expression)
                    || opt(f, d.question_dot_token)
                    || opt_list(f, &d.type_arguments)
                    || list(f, &d.arguments)
            }
            NodeData::NewExpression(d) => {
                f(d.expression) || opt_list(f, &d.type_arguments) || opt_list(f, &d.arguments)
            }
            NodeData::ParenthesizedExpression(d)
            | NodeData::SpreadElement(d)
            | NodeData::ExpressionStatement(d)
            | NodeData::ThrowStatement(d)
            | NodeData::ComputedPropertyName(d)
            | NodeData::SpreadAssignment(d)
            | NodeData::DeleteExpression(d)
            | NodeData::TypeOfExpression(d)
            | NodeData::VoidExpression(d)
            | NodeData::AwaitExpression(d)
            | NodeData::NonNullExpression(d)
            | NodeData::Decorator(d) => f(d.expression),
            NodeData::AsExpression(d) | NodeData::SatisfiesExpression(d) => {
                f(d.expression) || f(d.type_node)
            }
            NodeData::TypeAssertionExpression(d) => f(d.type_node) || f(d.expression),
            NodeData::MetaProperty(d) => f(d.name),
            NodeData::TemplateExpression(d) => f(d.head) || list(f, &d.template_spans),
            NodeData::TemplateSpan(d) => f(d.expression) || f(d.literal),
            NodeData::TaggedTemplateExpression(d) => {
                f(d.tag)
                    || opt(f, d.question_dot_token)
                    || opt_list(f, &d.type_arguments)
                    || f(d.template)
            }
            NodeData::YieldExpression(d) => opt(f, d.asterisk_token) || opt(f, d.expression),
            NodeData::ImportAttributes(d) => list(f, &d.attributes),
            NodeData::ImportAttribute(d) => opt(f, d.name) || f(d.value),
            NodeData::JsxElement(d) | NodeData::JsxFragment(d) => {
                f(d.opening) || list(f, &d.children) || f(d.closing)
            }
            NodeData::JsxOpeningElement(d) | NodeData::JsxSelfClosingElement(d) => {
                f(d.tag_name) || opt_list(f, &d.type_arguments) || f(d.attributes)
            }
            NodeData::JsxClosingElement(d) => f(d.tag_name),
            NodeData::JsxOpeningFragment | NodeData::JsxClosingFragment | NodeData::JsxText(_) => {
                false
            }
            NodeData::JsxAttributes(d) => list(f, &d.list),
            NodeData::JsxAttribute(d) => f(d.name) || opt(f, d.initializer),
            NodeData::JsxSpreadAttribute(d) => f(d.expression),
            NodeData::JsxNamespacedName(d) => f(d.namespace) || f(d.name),
            NodeData::JsxExpression(d) => opt(f, d.dot_dot_dot_token) || opt(f, d.expression),
            NodeData::ArrowFunction(d) => {
                mods(f, &d.modifiers)
                    || opt_list(f, &d.type_parameters)
                    || list(f, &d.parameters)
                    || opt(f, d.type_node)
                    || opt(f, d.full_signature)
                    || f(d.equals_greater_than_token)
                    || f(d.body)
            }
            NodeData::PrefixUnaryExpression(d) | NodeData::PostfixUnaryExpression(d) => {
                f(d.operand)
            }
            NodeData::BinaryExpression(d) => f(d.left) || f(d.operator_token) || f(d.right),
            NodeData::ArrayLiteralExpression(d)
            | NodeData::Block(d)
            | NodeData::ObjectLiteralExpression(d) => list(f, &d.list),
            NodeData::ReturnStatement(d) => opt(f, d.expression),
            NodeData::EmptyStatement
            | NodeData::DebuggerStatement
            | NodeData::OmittedExpression => false,
            NodeData::VariableStatement(d) => mods(f, &d.modifiers) || f(d.declaration_list),
            NodeData::VariableDeclarationList(d) => list(f, &d.declarations),
            NodeData::VariableDeclaration(d) => {
                f(d.name)
                    || opt(f, d.exclamation_token)
                    || opt(f, d.type_node)
                    || opt(f, d.initializer)
            }
            NodeData::ObjectBindingPattern(d) | NodeData::ArrayBindingPattern(d) => {
                list(f, &d.elements)
            }
            NodeData::BindingElement(d) => {
                opt(f, d.dot_dot_dot_token)
                    || opt(f, d.property_name)
                    || opt(f, d.name)
                    || opt(f, d.initializer)
            }
            NodeData::ForStatement(d) => {
                opt(f, d.initializer)
                    || opt(f, d.condition)
                    || opt(f, d.incrementor)
                    || f(d.statement)
            }
            NodeData::ForInOrOfStatement(d) => {
                opt(f, d.await_modifier) || f(d.initializer) || f(d.expression) || f(d.statement)
            }
            NodeData::TryStatement(d) => {
                f(d.try_block) || opt(f, d.catch_clause) || opt(f, d.finally_block)
            }
            NodeData::CatchClause(d) => opt(f, d.variable_declaration) || f(d.block),
            NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => {
                mods(f, &d.modifiers)
                    || opt(f, d.asterisk_token)
                    || opt(f, d.name)
                    || opt_list(f, &d.type_parameters)
                    || list(f, &d.parameters)
                    || opt(f, d.type_node)
                    || opt(f, d.full_signature)
                    || opt(f, d.body)
            }
            NodeData::ParameterDeclaration(d) => {
                mods(f, &d.modifiers)
                    || opt(f, d.dot_dot_dot_token)
                    || f(d.name)
                    || opt(f, d.question_token)
                    || opt(f, d.type_node)
                    || opt(f, d.initializer)
            }
            NodeData::TypeParameterDeclaration(d) => {
                mods(f, &d.modifiers)
                    || f(d.name)
                    || opt(f, d.constraint)
                    || opt(f, d.expression)
                    || opt(f, d.default_type)
            }
            NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => {
                mods(f, &d.modifiers)
                    || opt(f, d.name)
                    || opt_list(f, &d.type_parameters)
                    || opt_list(f, &d.heritage_clauses)
                    || list(f, &d.members)
            }
            NodeData::HeritageClause(d) => list(f, &d.types),
            NodeData::ExpressionWithTypeArguments(d) => {
                f(d.expression) || opt_list(f, &d.type_arguments)
            }
            NodeData::MethodDeclaration(d) => {
                mods(f, &d.modifiers)
                    || opt(f, d.asterisk_token)
                    || f(d.name)
                    || opt(f, d.postfix_token)
                    || opt_list(f, &d.type_parameters)
                    || list(f, &d.parameters)
                    || opt(f, d.type_node)
                    || opt(f, d.full_signature)
                    || opt(f, d.body)
            }
            NodeData::PropertyDeclaration(d) | NodeData::PropertyAssignment(d) => {
                mods(f, &d.modifiers)
                    || f(d.name)
                    || opt(f, d.postfix_token)
                    || opt(f, d.type_node)
                    || opt(f, d.initializer)
            }
            NodeData::ShorthandPropertyAssignment(d) => {
                mods(f, &d.modifiers)
                    || f(d.name)
                    || opt(f, d.postfix_token)
                    || opt(f, d.type_node)
                    || opt(f, d.equals_token)
                    || opt(f, d.object_assignment_initializer)
            }
            NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
                mods(f, &d.modifiers)
                    || f(d.name)
                    || opt_list(f, &d.type_parameters)
                    || list(f, &d.parameters)
                    || opt(f, d.type_node)
                    || opt(f, d.full_signature)
                    || opt(f, d.body)
            }
            NodeData::ConstructorDeclaration(d) => {
                mods(f, &d.modifiers)
                    || opt_list(f, &d.type_parameters)
                    || list(f, &d.parameters)
                    || opt(f, d.type_node)
                    || opt(f, d.full_signature)
                    || opt(f, d.body)
            }
            NodeData::IndexSignatureDeclaration(d) => {
                mods(f, &d.modifiers) || list(f, &d.parameters) || opt(f, d.type_node)
            }
            NodeData::ClassStaticBlockDeclaration(d) => mods(f, &d.modifiers) || f(d.body),
            NodeData::SemicolonClassElement => false,
            NodeData::InterfaceDeclaration(d) => {
                mods(f, &d.modifiers)
                    || opt(f, d.name)
                    || opt_list(f, &d.type_parameters)
                    || opt_list(f, &d.heritage_clauses)
                    || list(f, &d.members)
            }
            NodeData::TypeAliasDeclaration(d) => {
                mods(f, &d.modifiers)
                    || f(d.name)
                    || opt_list(f, &d.type_parameters)
                    || f(d.type_node)
            }
            NodeData::EnumDeclaration(d) => {
                mods(f, &d.modifiers) || f(d.name) || list(f, &d.members)
            }
            NodeData::EnumMember(d) => f(d.name) || opt(f, d.initializer),
            NodeData::PropertySignature(d) => {
                mods(f, &d.modifiers)
                    || f(d.name)
                    || opt(f, d.postfix_token)
                    || opt(f, d.type_node)
                    || opt(f, d.initializer)
            }
            NodeData::MethodSignature(d) => {
                mods(f, &d.modifiers)
                    || f(d.name)
                    || opt(f, d.postfix_token)
                    || opt_list(f, &d.type_parameters)
                    || list(f, &d.parameters)
                    || opt(f, d.type_node)
            }
            NodeData::CallSignature(d) | NodeData::ConstructSignature(d) => {
                opt_list(f, &d.type_parameters) || list(f, &d.parameters) || opt(f, d.type_node)
            }
            NodeData::TypeLiteral(d) => list(f, &d.members),
            NodeData::ModuleDeclaration(d) => mods(f, &d.modifiers) || f(d.name) || opt(f, d.body),
            NodeData::ModuleBlock(d) => list(f, &d.statements),
            NodeData::ImportDeclaration(d) => {
                mods(f, &d.modifiers)
                    || opt(f, d.import_clause)
                    || f(d.module_specifier)
                    || opt(f, d.attributes)
            }
            NodeData::ImportClause(d) => opt(f, d.name) || opt(f, d.named_bindings),
            NodeData::NamespaceImport(d) | NodeData::NamespaceExport(d) => f(d.name),
            NodeData::NamedImports(d) | NodeData::NamedExports(d) => list(f, &d.elements),
            NodeData::ImportSpecifier(d) | NodeData::ExportSpecifier(d) => {
                opt(f, d.property_name) || f(d.name)
            }
            NodeData::ExternalModuleReference(d) => f(d.expression),
            NodeData::ImportEqualsDeclaration(d) => {
                mods(f, &d.modifiers) || f(d.name) || f(d.module_reference)
            }
            NodeData::ExportDeclaration(d) => {
                mods(f, &d.modifiers)
                    || opt(f, d.export_clause)
                    || opt(f, d.module_specifier)
                    || opt(f, d.attributes)
            }
            NodeData::ExportAssignment(d) => {
                mods(f, &d.modifiers) || opt(f, d.type_node) || f(d.expression)
            }
            NodeData::NamespaceExportDeclaration(d) => mods(f, &d.modifiers) || f(d.name),
            NodeData::IfStatement(d) => {
                f(d.expression) || f(d.then_statement) || opt(f, d.else_statement)
            }
            NodeData::DoStatement(d) => f(d.statement) || f(d.expression),
            NodeData::WhileStatement(d) => f(d.expression) || f(d.statement),
            NodeData::WithStatement(d) => f(d.expression) || f(d.statement),
            NodeData::SwitchStatement(d) => f(d.expression) || f(d.case_block),
            NodeData::CaseBlock(d) => list(f, &d.clauses),
            NodeData::CaseOrDefaultClause(d) => opt(f, d.expression) || list(f, &d.statements),
            NodeData::BreakStatement(d) | NodeData::ContinueStatement(d) => opt(f, d.label),
            NodeData::LabeledStatement(d) => f(d.label) || f(d.statement),
            NodeData::ConditionalExpression(d) => {
                f(d.condition)
                    || f(d.question_token)
                    || f(d.when_true)
                    || f(d.colon_token)
                    || f(d.when_false)
            }
            NodeData::TypeReference(d) => f(d.type_name) || opt_list(f, &d.type_arguments),
            NodeData::ArrayType(d) => f(d.element_type),
            NodeData::IndexedAccessType(d) => f(d.object_type) || f(d.index_type),
            NodeData::UnionType(d) | NodeData::IntersectionType(d) => list(f, &d.types),
            NodeData::ParenthesizedType(d) | NodeData::RestType(d) | NodeData::OptionalType(d) => {
                f(d.type_node)
            }
            NodeData::LiteralType(d) => f(d.literal),
            NodeData::FunctionType(d) | NodeData::ConstructorType(d) => {
                mods(f, &d.modifiers)
                    || opt_list(f, &d.type_parameters)
                    || list(f, &d.parameters)
                    || opt(f, d.type_node)
            }
            NodeData::ConditionalType(d) => {
                f(d.check_type) || f(d.extends_type) || f(d.true_type) || f(d.false_type)
            }
            NodeData::InferType(d) => f(d.type_parameter),
            NodeData::TypeOperator(d) => f(d.type_node),
            NodeData::MappedType(d) => {
                opt(f, d.readonly_token)
                    || f(d.type_parameter)
                    || opt(f, d.name_type)
                    || opt(f, d.question_token)
                    || opt(f, d.type_node)
                    || list(f, &d.members)
            }
            NodeData::TupleType(d) => list(f, &d.types),
            NodeData::NamedTupleMember(d) => {
                opt(f, d.dot_dot_dot_token)
                    || f(d.name)
                    || opt(f, d.question_token)
                    || f(d.type_node)
            }
            NodeData::ThisType => false,
            NodeData::TypeQuery(d) => f(d.expr_name) || opt_list(f, &d.type_arguments),
            NodeData::ImportType(d) => {
                f(d.argument)
                    || opt(f, d.attributes)
                    || opt(f, d.qualifier)
                    || opt_list(f, &d.type_arguments)
            }
            NodeData::TemplateLiteralType(d) => f(d.head) || list(f, &d.template_spans),
            NodeData::TemplateLiteralTypeSpan(d) => f(d.expression) || f(d.literal),
            NodeData::TypePredicate(d) => {
                opt(f, d.asserts_modifier) || f(d.parameter_name) || opt(f, d.type_node)
            }
            NodeData::SourceFile(d) => list(f, &d.statements) || f(d.end_of_file_token),
        }
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
