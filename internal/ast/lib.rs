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
            | NodeData::BigIntLiteral(d) => &d.text,
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
        match &self.nodes[id.index()].data {
            NodeData::Token
            | NodeData::Identifier(_)
            | NodeData::PrivateIdentifier(_)
            | NodeData::StringLiteral(_)
            | NodeData::NumericLiteral(_)
            | NodeData::BigIntLiteral(_)
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
            | NodeData::ExpressionStatement(d) => f(d.expression),
            NodeData::PrefixUnaryExpression(d) | NodeData::PostfixUnaryExpression(d) => {
                f(d.operand)
            }
            NodeData::BinaryExpression(d) => f(d.left) || f(d.operator_token) || f(d.right),
            NodeData::ArrayLiteralExpression(d) | NodeData::Block(d) => list(f, &d.list),
            NodeData::ReturnStatement(d) => opt(f, d.expression),
        }
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
