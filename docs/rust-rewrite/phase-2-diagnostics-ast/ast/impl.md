# ast: 实现方案（impl.md）

**crate**：`tsgo_ast`　**目标**：整个编译器的**地基 AST**——节点定义（~362 种 `Kind` / ~193 种节点数据结构）、节点工厂（arena 分配）、访问器（`ForEachChild`/`VisitEachChild`）、深克隆、各类位标志（Node/Symbol/Modifier/Token/Flow/Subtree/Check/Function）、`Symbol` 雏形、`Diagnostic` 载体、UTF-8↔UTF-16 位置映射、运算符优先级与海量节点查询工具。
**依赖（crate）**：`tsgo_core`（`TextRange`/`Arena`/`CompilerOptions`/`ScriptKind`/`Tristate` 等）、`tsgo_diagnostics`（`Message`/`Category`/`Key`，供 `Diagnostic`）、`tsgo_tspath`（`Path`/扩展名判定）、`tsgo_collections`（`Set`/`OrderedMap`）。镜像 Go import。
**Go 源**：`internal/ast/`（21 个非测试 `.go` 文件，合计 ~8400 行手写 + 两个大生成文件：`ast_generated.go` ~9560 行、`kind_generated.go`）

## 这个包是什么（业务说明）

`ast` 定义 TypeScript 语法树的**全部**节点类型与其上的基础操作，是 scanner 之上、parser/binder/checker/printer/ls 之下的承重墙。它**不做解析**（parser 在 P3 调用本包的 `NodeFactory` 建树），只提供：

1. **统一节点表示**：单一 `Node{ Kind, Flags, Loc, Parent, data }`，`data` 是 ~193 种具体节点结构之一（`Identifier` / `CallExpression` / `FunctionDeclaration` …），通过接口 `nodeData` 做派发与类型断言。`Kind`（~362 个枚举）与 `data` 类型**多对一**（如一个 `Token` 结构服务 ~150 种 punctuation/keyword `Kind`）。
2. **节点工厂 `NodeFactory`**：按类型 arena 批量分配节点，`NewXxx`（193 个构造器）/`UpdateXxx`（193 个不可变更新器）/`Clone`。
3. **遍历与变换**：`ForEachChild`（只读早退遍历）、`IterChildren`（Go 1.23 迭代器）、`VisitEachChild` + `NodeVisitor`（产生新树的变换骨架）、`DeepCloneNode`（跨文件复制子树）。
4. **位标志族**：`NodeFlags`/`SymbolFlags`/`ModifierFlags`/`TokenFlags`/`FunctionFlags`/`CheckFlags`/`FlowFlags`/`SubtreeFacts`——全是 `iota` 位枚举。
5. **附属类型**：`Symbol`/`SymbolTable`（绑定期填充的符号雏形）、`FlowNode`（控制流图节点）、`Diagnostic`/`DiagnosticsCollection`（AST 层诊断载体）、`SourceFile`（根节点，承载文件级元数据/缓存/锁）、`PositionMap`（UTF-8↔UTF-16）、`OperatorPrecedence`。
6. **查询工具 `utilities.go`**（417 个函数）：`Is*` 谓词、`Get*` 取值、modifier/名字/声明判定等，全编译器复用。

它在 Phase 2 落地（紧随 diagnostics），因为 parser（P3）一切建树调用都打在本包接口上。**这是 arena + NodeId 所有权模型的核心落地点**——能否零 `unsafe` 表达 Go 的"裸指针图 + 反向 Parent 指针 + 绑定期可变"，成败在此。

---

## 所有权 / 类型映射（本包关键决策）—— ★ AST 所有权模型 ★

> 这是全仓最关键的一节。先讲 Go 的真实内存模型，再给 Rust 的安全等价物，最后逐构造列映射。通用规则见 PORTING.md §5。

### 1. Go 的内存模型（ground truth）

```go
type Node struct {
    Kind   Kind            // int16，节点种类（362 种）
    Flags  NodeFlags       // uint32 位标志
    Loc    core.TextRange  // {pos, end} int32
    id     atomic.Uint64   // 懒分配的 NodeId（绑定期用作符号表键）
    Parent *Node           // 反向父指针（SetParentInChildren 后才有效）
    data   nodeData        // 接口：指向具体节点结构（≈判别联合）
}
```

- `data` 是 ~193 个具体结构之一，**每个具体结构内嵌一个 `Node`**（经 `NodeBase→NodeDefault→Node`），且 `Node.data` 反指回该结构——即 `Node` 与其 `data` **同一块分配，互相引用**（Go 的自嵌入惯用法）。
- 子节点、`Parent`、`NodeList.Nodes []*Node` 全是**裸 `*Node` 指针**，构成一张可有环（`Parent` 反向边）的有向图。
- `NodeFactory` 用 `core.Arena[T]`（带 size-class 的 bump 分配器）按类型批量分配热点节点；冷门节点 `&Struct{}` 直接堆分配。
- 绑定期**可变**：`binder` 会写 `node.Parent`、`symbol.Members`、`flowNode` 等。
- `data nodeData` 接口提供动态派发（`ForEachChild`/`VisitEachChild`/`Clone`/`computeSubtreeFacts`/`Name`/`Modifiers`/各 `XxxData()` 取内嵌基类）与类型断言（`n.AsIdentifier()` = `n.data.(*Identifier)`）。

裸指针图 + 反向边 + 可变，在安全 Rust 里**不能**用 `&`/`Box` 直接表达（借用检查器拒绝环与别名可变）。故采用 arena + 索引。

### 2. Rust 的安全等价物（统一约定）

**核心：所有节点放进一个 arena，所有"指针"换成 `NodeId(u32)` 索引。** 这是 rust-analyzer / swc 的主流做法。

```rust
/// 类型化节点索引，替代 Go 的 *Node。Copy，廉价传递。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(u32);

/// 全部节点的所有者。替代 Go 的 NodeFactory 各 arena 之和。
pub struct NodeArena {
    nodes: Vec<Node>,                 // index == NodeId.0
    node_lists: Vec<Vec<NodeId>>,     // NodeList 的后备存储（NodeListId 索引）
    modifier_lists: Vec<ModifierList>,
    hooks: NodeFactoryHooks,
    node_count: usize,
    text_count: usize,
}

/// 节点头 + 类型化负载。对应 Go 的 Node 结构。
pub struct Node {
    pub kind: Kind,            // 保留！与 data 多对一（Token 等共享）
    pub flags: NodeFlags,
    pub loc: TextRange,
    pub parent: Option<NodeId>,// 替代 *Node 反向指针；None = 未父化
    pub data: NodeData,        // 判别联合，替代 data nodeData 接口
}

/// ~193 个 variant，对应 Go 的每个具体节点结构（nodeData 的实现）。
pub enum NodeData {
    Token(TokenData),                       // 服务 ~150 种 Kind
    Identifier(IdentifierData),
    QualifiedName(QualifiedNameData),
    CallExpression(CallExpressionData),
    FunctionDeclaration(FunctionDeclarationData),
    SourceFile(Box<SourceFileData>),        // 大结构装箱，避免 enum 过胖
    // … 其余 ~187 个 variant
}
```

要点逐条：

- **`NodeId` 替代一切 `*Node`**：子节点、`Parent`、`NodeList`、`Symbol.ValueDeclaration` 等全用 `NodeId` / `Option<NodeId>`。环与反向边在 arena 里是普通 `u32`，借用检查器无意见 → **零 unsafe**。
- **`Kind` 与 `NodeData` 都保留**：Go 里 `Kind` ≠ `data` 类型（多对一）。强行用 enum discriminant 当 Kind 会破坏 1:1（Token 的 150 种 Kind 会塌成一个）。故 `Node` 同时存 `kind: Kind` 和 `data: NodeData`，与 Go 完全对齐。
- **`NodeList` → `Vec<NodeId>`**：Go `NodeList{ Loc, Nodes []*Node }` → Rust `NodeList{ loc: TextRange, nodes: Vec<NodeId> }`，或存进 `node_lists` arena 用 `NodeListId` 引用（热点路径省重复分配，对齐 Go 的 `nodeListArena`）。默认内联 `Vec<NodeId>`，列表本身需共享时才走 arena。
- **`ModifierList`** = `NodeList` + `ModifierFlags`：组合（Go 是嵌入 `NodeList`）。
- **`Parent` 反向边**：`node.parent: Option<NodeId>`，由 `set_parent_in_children(arena, id)` 在解析后/reparse 时写入（对应 Go `SetParentInChildren`）。读取 `n.Parent` → `arena[id].parent`，调用点改 `parent(arena, id)` 访问器（**允许且必要的语法偏离**，结构 1:1）。
- **`nodeData` 接口方法 → arena 上的 `match`**：`ForEachChild`/`VisitEachChild`/`Clone`/`computeSubtreeFacts`/`Name`/`Modifiers` 等不再是 dyn 派发，而是
  ```rust
  impl NodeArena {
      pub fn for_each_child(&self, id: NodeId, f: &mut dyn FnMut(NodeId) -> bool) -> bool {
          match &self[id].data {
              NodeData::QualifiedName(d) => visit(f, d.left) || visit(f, d.right),
              NodeData::CallExpression(d) => { /* … */ }
              _ => false, // NodeDefault：无子节点
          }
      }
  }
  ```
  PORTING §3 明确"enum + match 优先于 dyn"。
- **内嵌基类（embedding）→ 组合 + match 访问器**：Go 用接口方法 `DeclarationData() *DeclarationBase` 返回内嵌基类。Rust 让每个 variant 的 data 结构**含**对应基类字段（`decl: DeclarationBase`、`locals: LocalsContainerBase`、`body: BodyBase`…），访问器 `fn declaration_data(&self, id) -> Option<&DeclarationBase>` 用 `match` 命中拥有该基类的 variant。基类本身（`DeclarationBase{ symbol: Option<SymbolId> }` 等）是小结构体，按值组合。
- **`id atomic.Uint64`（懒分配 NodeId）**：Go 的 `Node.id` 是绑定期才赋值的稀疏 id（用作 symbol 表键）。Rust 里 arena 下标 `NodeId` 已是稳定唯一 id，可直接复用 → 省去 `atomic.Uint64`。若需与 Go 的 id 语义完全对齐（懒分配序），保留一个 `node_id: Cell<u32>`/单独计数器。**决策：用 arena 下标当 id**，记为偏离。
- **`Symbol` / `Type` 图同构**：`Symbol` 用 `SymbolId(u32)` + `SymbolArena`（`Symbol.Declarations []*Node` → `Vec<NodeId>`，`Symbol.Parent *Symbol` → `Option<SymbolId>`）。本包先落 `Symbol` 结构 + `SymbolTable`；`Type` 图在 P4 checker。`SymbolId` 本包先定义（Go `ids.go`）。
- **arena 选型**：Go 的 `core.Arena[T]` 是 size-class bump 分配器。Rust 候选 `la-arena::Arena<T>` + `Idx<T>`（rust-analyzer 同款）或手写 `Vec<Node>` + `NodeId` newtype。**决策（全仓统一）**：节点用**手写 `Vec<Node>` + `NodeId(u32)`**（最贴近 Go 的单一 arena 语义、可控、零依赖）；如团队偏好类型安全 `Idx<T>`，则统一切 `la-arena`。本文档按手写方案写 TODO，二者可一键替换。记入 `references/crate-map.md`。

### 3. `NodeFactory` → `NodeArena` 的方法形态

Go：`f.NewIdentifier(text)` 在 `identifierArena` 上 `New()` 一个 `Identifier`，回填 `Node` 头，跑 `OnCreate` 钩子，返回 `*Node`。
Rust：`arena.new_identifier(text) -> NodeId`：`push` 一个 `Node{ kind: KindIdentifier, data: NodeData::Identifier(..), .. }`，跑钩子，返回下标。所有 `NewXxx` 同形。`UpdateXxx`（不可变更新：字段没变就返回原 `NodeId`，变了才新建 + 拷 `Flags`/`Loc` + `OnUpdate` 钩子）同形。

### 4. 映射速查表

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type NodeId uint64` / `type SymbolId uint64` | `struct NodeId(u32)` / `struct SymbolId(u32)`（`ids.rs`） | arena 下标够用 `u32`（Go 用 `uint64` 是图省事）；存疑处 `// PERF(port)` |
| `*Node`（子/父/列表元素） | `NodeId` / `Option<NodeId>` | 一切节点引用走索引 |
| `data nodeData`（接口/判别联合） | `enum NodeData { … ~193 variant }` | 见上 |
| 具体节点结构（`Identifier` 等，内嵌 `Node`） | `struct IdentifierData { .. }`（**不**含 Node 头） | 头部 `Node{kind,flags,loc,parent}` 抽到外层，data 只存类型字段 |
| `Node.Kind Kind`（int16） | `kind: Kind`（`#[repr(i16)] enum Kind`） | 与 data 多对一，**保留** |
| `NodeList{ Loc, Nodes []*Node }` | `NodeList{ loc: TextRange, nodes: Vec<NodeId> }` | 或 `NodeListId` 进 arena |
| `ModifierList{ NodeList; ModifierFlags }` | `ModifierList{ list: NodeList, modifier_flags: ModifierFlags }` | 组合替代嵌入 |
| 内嵌基类 `DeclarationBase{ Symbol *Symbol }` | `DeclarationBase{ symbol: Option<SymbolId> }` 作 variant 字段 | match 访问器返回 `Option<&DeclarationBase>` |
| `FlowNodeBase{ FlowNode *FlowNode }` | `flow_node: Option<FlowNodeId>`（FlowNode 进 flow arena） | 控制流图同构于节点图 |
| `core.Arena[T]` | `Vec<T>` + newtype 索引（或 `la-arena`） | 全仓统一 |
| `NodeFactoryHooks{ OnCreate/OnUpdate/OnClone func(*Node) }` | `struct NodeFactoryHooks { on_create: Option<Box<dyn FnMut(...)>>, .. }` | 闭包钩子；签名带 `&mut NodeArena, NodeId` |
| `atomic.Uint64`（Node.id/Symbol.id 懒分配） | 复用 `NodeId`/`SymbolId` arena 下标 | 见偏离 |
| `Visitor func(*Node) bool` | `FnMut(NodeId) -> bool`（`true`=早退） | `ForEachChild` 回调 |
| `iter.Seq[*Node]`（IterChildren） | `impl Iterator<Item=NodeId>` 或回调适配 | Go 1.23 range-over-func → Rust 迭代器 |
| `NodeVisitor{ Visit, Factory, Hooks }` | `struct NodeVisitor<'a> { visit: ..., arena: &'a mut NodeArena, hooks: NodeVisitorHooks }` | 变换骨架；产生新节点要 `&mut arena` |
| `iota` 位枚举（8 个 Flags 族） | `bitflags! { struct XxxFlags: u32 { … } }` | 见下"标志族" |
| `Kind` `iota` 枚举（362 个） | `#[repr(i16)] enum Kind { … }` + 区间常量 | `kind_generated.rs` |
| `SymbolTable = map[string]*Symbol` | `FxHashMap<String, SymbolId>` 或 `IndexMap`（若影响 emit 序） | 绑定期填充 |
| `xxh3.Uint128`（SourceFile.Hash） | `u128` / `[u8;16]`（`xxhash-rust` 的 xxh3_128） | 文件内容哈希 |
| `sync.Once`/`sync.RWMutex`/`atomic.Bool`（SourceFile 缓存） | `OnceLock`/`RwLock`/`AtomicBool` | 懒缓存 jsdoc/lineMap/positionMap/tokenCache |

### 5. 标志族（全部 `bitflags`）

| Go 文件 / 类型 | 位宽 | Rust |
|---|---|---|
| `nodeflags.go` `NodeFlags` | u32 | `bitflags! NodeFlags: u32`（含 `BlockScoped`/`ContextFlags` 等组合常量；`IdentifierHasExtendedUnicodeEscape` 复用 `ContainsThis` 位——照搬） |
| `symbolflags.go` `SymbolFlags` | u32 | `bitflags! SymbolFlags: u32`（含 `All=1<<30-1`、大量 `*Excludes` 组合） |
| `modifierflags.go` `ModifierFlags` | u32 | `bitflags! ModifierFlags: u32`（JSDoc cache-only 位段、`HasComputedFlags`） |
| `tokenflags.go` `TokenFlags` | i32 | `bitflags! TokenFlags: i32`（数值字面量/字符串/模板 flag 组合） |
| `functionflags.go` `FunctionFlags` | u32 | `bitflags! FunctionFlags: u32` + `get_function_flags(arena, id)` |
| `checkflags.go` `CheckFlags` | u32 | `bitflags! CheckFlags: u32`（transient symbol 用，binder/checker 期） |
| `flow.go` `FlowFlags` | u32 | `bitflags! FlowFlags: u32` + `FlowNode`/`FlowList`/`FlowSwitchClauseData`/`FlowReduceLabelData` |
| `subtreefacts.go` `SubtreeFacts` | u32 | `bitflags! SubtreeFacts: u32`（transform 相关；`SubtreeFactsComputed` 必须最后一位；`propagate*`/`SubtreeExclusions*` 一族） |

---

## 文件清单 → Rust 模块

> crate 根 = `internal/ast/`，basename==目录名的 `ast.go` → `lib.rs`。

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/ast/ast.go`（94KB） | `internal/ast/lib.rs` | crate 根。`Node`/`NodeArena`(原 NodeFactory) /`NodeList`/`ModifierList`/`nodeData`→`NodeData` 接口形态、`Node` 访问器（`Text`/`Expression`/`Name`/各 `*Data`）、`Contains`、`SourceFile` 结构与方法、`SourceFileMetaData`/`CommentDirective` 等。**最大手写文件，拆多个 `mod`** |
| `internal/ast/ast_generated.go`（9560 行，生成） | `internal/ast/ast_generated.rs`（生成） | ~193 个 `NodeData` variant 结构 + `NewXxx`/`UpdateXxx`/`Clone`/`ForEachChild`/`VisitEachChild`/`computeSubtreeFacts`/`IsXxx`（232 个谓词）+ 基类结构定义。**由移植版 `_scripts/generate-go-ast.ts` 等价生成器产出** |
| `internal/ast/kind_generated.go`（生成） | `internal/ast/kind_generated.rs`（生成） | `#[repr(i16)] enum Kind`（362 个 variant + `KindCount`）+ 区间常量（`KindFirstStatement` 等）+ `*SyntaxKind` 类型别名 |
| `internal/ast/kind_stringer_generated.go`（生成） | （并入 `kind_generated.rs`） | `impl Display/Debug for Kind`，可用 `strum`/手写表 |
| `internal/ast/ids.go` | `internal/ast/ids.rs` | `NodeId`/`SymbolId` newtype（见所有权模型，u32） |
| `internal/ast/nodeflags.go` | `internal/ast/nodeflags.rs` | `bitflags! NodeFlags` |
| `internal/ast/symbolflags.go` | `internal/ast/symbolflags.rs` | `bitflags! SymbolFlags` |
| `internal/ast/modifierflags.go` | `internal/ast/modifierflags.rs` | `bitflags! ModifierFlags` |
| `internal/ast/tokenflags.go` | `internal/ast/tokenflags.rs` | `bitflags! TokenFlags` |
| `internal/ast/functionflags.go` | `internal/ast/functionflags.rs` | `bitflags! FunctionFlags` + `get_function_flags` |
| `internal/ast/checkflags.go` | `internal/ast/checkflags.rs` | `bitflags! CheckFlags` |
| `internal/ast/flow.go` | `internal/ast/flow.rs` | `FlowFlags`/`FlowNode`/`FlowList`/`FlowSwitchClauseData`/`FlowReduceLabelData` |
| `internal/ast/subtreefacts.go` | `internal/ast/subtreefacts.rs` | `SubtreeFacts` + `propagate*`/`SubtreeExclusions*` |
| `internal/ast/symbol.go` | `internal/ast/symbol.rs` | `Symbol`/`SymbolTable`/内部符号名常量/`SymbolName`/`EscapeAllInternalSymbolNames` |
| `internal/ast/diagnostic.go` | `internal/ast/diagnostic.rs` | `Diagnostic`/`DiagnosticsCollection`/`Compare*`/`Equal*`（引用 `tsgo_diagnostics`） |
| `internal/ast/positionmap.go` | `internal/ast/positionmap.rs` | `PositionMap`（UTF-8↔UTF-16，二分） |
| `internal/ast/visitor.go` | `internal/ast/visitor.rs` | `NodeVisitor`/`NodeVisitorHooks`/`VisitNode(s)`/`VisitSlice`/`liftToBlock` |
| `internal/ast/deepclone.go` | `internal/ast/deepclone.rs` | `DeepCloneNode`/`DeepCloneReparse`/`DeepCloneReparseModifiers`（基于 NodeVisitor） |
| `internal/ast/precedence.go` | `internal/ast/precedence.rs` | `OperatorPrecedence` + 6 个函数（运算符→优先级、`getExpressionPrecedence` 等） |
| `internal/ast/parseoptions.go` | `internal/ast/parseoptions.rs` | `SourceFileParseOptions`/`ExternalModuleIndicatorOptions`/外部模块指示符判定（依赖 `tspath`/`core`） |
| `internal/ast/utilities.go`（146KB，417 函数） | `internal/ast/utilities.rs` | 海量 `Is*`/`Get*`/谓词/工具。**最大手写文件，按主题拆子模块**（见 TODO 与偏离） |

## 依赖白名单（本包新增的 crate）

- `bitflags`（8 个标志族）。
- `xxhash-rust`（feature `xxh3`；对应 `github.com/zeebo/xxh3`，`SourceFile.Hash`）。
- `rustc-hash`（`FxHashMap`，SymbolTable / Identifiers / 各缓存 map；影响 emit 序的改用 `indexmap`）。
- `la-arena`（**可选**；若不手写 `Vec`+newtype 索引则用它，全仓二选一）。
- 其余复用 Phase 1：`tsgo_core`/`tsgo_collections`/`tsgo_tspath`/`tsgo_diagnostics`。
- 记入 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按"叶子标志 → 索引/工厂 → 遍历 → 工具"的 TDD 推进序。生成文件用移植生成器产出，不逐 variant 手写。

### `ids.rs`（Go: `ids.go`）

- [ ] `pub struct NodeId(u32)` / `pub struct SymbolId(u32)`，`#[derive(Copy,Clone,PartialEq,Eq,Hash,Debug)]`　`// Go: ids.go`

### 标志族（Go: `*flags.go` / `flow.go` / `subtreefacts.go`）

- [ ] `bitflags! NodeFlags: u32`（28 个基位 + `BlockScoped`/`Constant`/`AwaitUsing`/`ReachabilityCheckFlags`/`ReachabilityAndEmitFlags`/`ContextFlags`/`TypeExcludesFlags`/`PermanentlySetIncrementalFlags`/`IdentifierHasExtendedUnicodeEscape`=`ContainsThis`）　`// Go: nodeflags.go`
- [ ] `bitflags! SymbolFlags: u32`（31 基位 + `Enum`/`Variable`/`Value`/`Type`/`Namespace`/`Module`/`Accessor` + 全部 `*Excludes`/`ModuleMember`/`BlockScoped`/`Classifiable`/`LateBindingContainer`/`All`）　`// Go: symbolflags.go`
- [ ] `bitflags! ModifierFlags: u32`（基位 + JSDoc cache-only 段 1<<23.. + `Syntactic*`/`JSDoc*`/`Accessibility*`/`ParameterPropertyModifier`/`TypeScriptModifier`/`All`/`Modifier`/`JavaScript`）　`// Go: modifierflags.go`
- [ ] `bitflags! TokenFlags: i32`（19 基位 + `BinaryOrOctalSpecifier`/`WithSpecifier`/`StringLiteralFlags`/`NumericLiteralFlags`/`TemplateLiteralLikeFlags`/`RegularExpressionLiteralFlags`/`IsInvalid`）　`// Go: tokenflags.go`
- [ ] `bitflags! FunctionFlags: u32`（Normal/Generator/Async/Invalid/AsyncGenerator）+ `pub fn get_function_flags(arena: &NodeArena, id: Option<NodeId>) -> FunctionFlags`　`// Go: functionflags.go:GetFunctionFlags`
- [ ] `bitflags! CheckFlags: u32`　`// Go: checkflags.go`
- [ ] `bitflags! FlowFlags: u32` + `struct FlowNode{ flags, node: Option<NodeId>, antecedent: Option<FlowNodeId>, antecedents: Option<FlowListId> }` + `FlowList` + `FlowSwitchClauseData`/`FlowReduceLabelData`（+ `new_flow_switch_clause_data`/`new_flow_reduce_label_data`/`IsEmpty`）　`// Go: flow.go`
- [ ] `bitflags! SubtreeFacts: u32`（14 facts + 11 markers + `SubtreeFactsComputed` 末位；`SubtreeContainsES*` 别名；`SubtreeExclusions*`；`LexicalThisOrSuper`）+ `propagate_subtree_facts`/`propagate_node_list_subtree_facts`/`propagate_modifier_list_subtree_facts`/`propagate_eraseable_*`/`propagate_*binding_element_*`　`// Go: subtreefacts.go`

### `kind_generated.rs`（Go: `kind_generated.go`，生成）

- [ ] `#[repr(i16)] pub enum Kind { Unknown=0, EndOfFile, … Count }`（362 个 + `KindCount`）　`// Go: kind_generated.go`
- [ ] 区间常量：`KindFirstAssignment`/`KindLastToken`/`KindFirstStatement`/`KindFirstTypeNode`/`KindFirstJSDocNode` 等（约 30 个）　`// Go: kind_generated.go`
- [ ] `*SyntaxKind` 类型别名（`TokenSyntaxKind`/`KeywordSyntaxKind`/`ModifierSyntaxKind`…）→ Rust `type X = Kind;`　`// Go: kind_generated.go`
- [ ] `impl Display for Kind`（stringer 表）　`// Go: kind_stringer_generated.go`

### `ast_generated.rs`（Go: `ast_generated.go`，生成）

> 由移植版 `generate-go-ast` 产出。不逐 variant 手写；代表性 variant 示例如下，供生成器模板与人工抽查对照：

- [ ] 基类结构：`DeclarationBase{ symbol: Option<SymbolId> }`、`ExportableBase{ local_symbol: Option<SymbolId> }`、`ModifiersBase{ modifiers: Option<ModifierList> }`、`LocalsContainerBase{ locals: SymbolTable, next_container: Option<NodeId> }`、`FlowNodeBase{ flow_node: Option<FlowNodeId> }`、`FunctionLikeBase`、`BodyBase`、`ClassLikeBase`、`LiteralLikeNodeBase`、`TemplateLiteralLikeNodeBase`、`NamedMemberBase`、`CompositeBase{ facts: Cell<u32> }` 等（~25 个）　`// Go: ast_generated.go:*Base`
- [ ] ~193 个 `XxxData` variant 结构（仅类型字段，头部在外层 `Node`）。代表：
  - [ ] `TokenData`（空；服务 ~150 Kind）　`// Go: ast_generated.go:Token`
  - [ ] `IdentifierData{ text: String, /*+ flow*/ }`　`// Go: ast_generated.go:Identifier`
  - [ ] `QualifiedNameData{ left: NodeId, right: NodeId }` + `for_each_child`=visit(left)||visit(right)　`// Go: ast_generated.go:QualifiedName`
  - [ ] `CallExpressionData{ expression, question_dot_token: Option<NodeId>, type_arguments: Option<NodeList>, arguments: NodeList }`　`// Go: ast_generated.go:CallExpression`
  - [ ] `FunctionDeclarationData{ /* FunctionLikeWithBodyBase 展开 */ name, type_parameters, parameters, type, body, … }`　`// Go: ast_generated.go:FunctionDeclaration`
  - [ ] `SourceFileData`（大，`Box` 进 enum）——见 `lib.rs`
- [ ] 每 variant 的 `new_xxx`（193）/`update_xxx`（193）/`clone`/`for_each_child`/`visit_each_child`/`compute_subtree_facts`　`// Go: ast_generated.go:NewXxx/UpdateXxx/...`
- [ ] 232 个 `pub fn is_xxx(arena, id) -> bool`（多数 `arena[id].kind == KindXxx`）　`// Go: ast_generated.go:IsXxx`

### `lib.rs`（Go: `ast.go`）

- [ ] `struct NodeId`-索引化的 `Node{ kind, flags, loc, parent, data: NodeData }`　`// Go: ast.go:Node`
- [ ] `enum NodeData`（汇集 ast_generated 的 variant；`mod` 组织）　`// Go: ast.go:nodeData`
- [ ] `NodeArena`（原 `NodeFactory`）：`new_node`/`node_count`/`text_count`/钩子；`NewNodeList`/`NewModifierList`　`// Go: ast.go:NodeFactory/newNode`
- [ ] `NodeList{ loc, nodes: Vec<NodeId> }` + `pos/end/has_trailing_comma/clone`　`// Go: ast.go:NodeList`
- [ ] `ModifierList{ list, modifier_flags }` + `clone`　`// Go: ast.go:ModifierList`
- [ ] Node 访问器（arena 方法）：`text`/`expression`/`name`/`modifiers`/`symbol`/`local_symbol`/`locals`/`body`/各 `*_data`/`decorators`/`subtree_facts`　`// Go: ast.go:Node.Text/Expression/...`
- [ ] `set_expression`（MutableNode 等价：`&mut arena`）　`// Go: ast.go:MutableNode.SetExpression`
- [ ] `contains(arena, n, descendant) -> bool`（沿 parent 上溯；未父化非 SourceFile → panic）　`// Go: ast.go:Node.Contains`
- [ ] `get_declaration_from_name`/`is_write_access*`/`access_kind`/`is_array_or_object_literal_destructuring_pattern` 等（ast.go 内手写谓词）　`// Go: ast.go:*`
- [ ] `SourceFileData`（root 节点）：`file_name`/`parse_options`/`text`/`statements: NodeList`/`end_of_file_token`/解析期字段（diagnostics/语言变体/imports/pragmas/…）/绑定期字段（symbol_count/classifiable_names/…）/语言服务缓存（hash/token_cache/name_table/position_map，懒 `OnceLock`）　`// Go: ast.go:SourceFile`
- [ ] `new_source_file`（FileName 须规范化绝对路径，否则 panic）/`update_source_file`/`copy_from`/`clone`/`for_each_child`/`visit_each_child`/`compute_subtree_facts`　`// Go: ast.go:SourceFile.*`
- [ ] `source_file` 懒缓存：`ecma_line_map`/`get_name_table`/`position_map`/`resolve_jsdoc`（`OnceLock`/`RwLock`）　`// Go: ast.go:SourceFile.ECMALineMap/GetNameTable/...`
- [ ] `SourceFileMetaData`/`CommentDirective`/`CommentDirectiveKind`/`CheckJsDirective`/`HasFileName` trait　`// Go: ast.go`
- [ ] `set_parse_jsdoc_for_node`（parser 在 P3 注入的懒 JSDoc 解析回调）→ Rust 用函数指针/`OnceLock<fn>`　`// Go: ast.go:SetParseJSDocForNode`

### `symbol.rs`（Go: `symbol.go`）

- [ ] `struct Symbol{ flags: SymbolFlags, check_flags: CheckFlags, name: String, declarations: Vec<NodeId>, value_declaration: Option<NodeId>, members: SymbolTable, exports: SymbolTable, parent: Option<SymbolId>, export_symbol: Option<SymbolId> }`（`id` 用 SymbolId 下标）　`// Go: symbol.go:Symbol`
- [ ] `is_external_module`/`is_static`/`combined_local_and_export_symbol_flags`　`// Go: symbol.go:Symbol.*`
- [ ] `type SymbolTable = FxHashMap<String, SymbolId>`　`// Go: symbol.go:SymbolTable`
- [ ] 内部符号名常量（`INTERNAL_SYMBOL_NAME_PREFIX="\u{FE}"` + Call/Constructor/… + ExportEquals/Default/This/ModuleExports）　`// Go: symbol.go`
- [ ] `symbol_name`/`escape_all_internal_symbol_names`　`// Go: symbol.go:SymbolName/EscapeAllInternalSymbolNames`

### `diagnostic.rs`（Go: `diagnostic.go`）

- [ ] `struct Diagnostic{ file: Option<NodeId/SourceFileId>, loc: TextRange, code: i32, category: Category, message: Option<&'static Message>, message_key: Key, message_args: Vec<String>, message_chain: Vec<Diagnostic>, related_information: Vec<Diagnostic>, reports_unnecessary/deprecated/skipped_on_no_emit: bool, repopulate_info: Option<RepopulateDiagnosticInfo> }`　`// Go: diagnostic.go:Diagnostic`
- [ ] getter/setter + `add_message_chain`/`add_related_info`/`clone`/`localize`（委托 `tsgo_diagnostics::localize`）　`// Go: diagnostic.go:Diagnostic.*`
- [ ] `RepopulateDiagnosticInfo`/`RepopulateDiagnosticKind`　`// Go: diagnostic.go`
- [ ] 构造器 `new_diagnostic`/`new_diagnostic_from_serialized`/`new_diagnostic_chain`/`new_compiler_diagnostic`　`// Go: diagnostic.go:NewDiagnostic*`
- [ ] `DiagnosticsCollection`（`Mutex`，按文件/全局分桶 + 懒排序 + 二分 Lookup）　`// Go: diagnostic.go:DiagnosticsCollection`
- [ ] `compare_diagnostics`/`equal_diagnostics*`/`compare_message_chain*`/`compare_related_info`　`// Go: diagnostic.go:CompareDiagnostics/...`

### `positionmap.rs`（Go: `positionmap.go`）

- [ ] `struct PositionMap{ ascii_only: bool, entries: Vec<PositionMapEntry{ utf8_pos: i32, delta: i32 }> }`　`// Go: positionmap.go:PositionMap`
- [ ] `compute_position_map(text: &str) -> PositionMap`（逐 byte/rune，累计 delta=utf8-utf16，4 字节 rune→2 code unit）　`// Go: positionmap.go:ComputePositionMap`
- [ ] `is_ascii_only`/`utf8_to_utf16`/`utf16_to_utf8`（二分）　`// Go: positionmap.go:IsAsciiOnly/UTF8ToUTF16/UTF16ToUTF8`

### `visitor.rs`（Go: `visitor.go`）

- [ ] `struct NodeVisitor`/`NodeVisitorHooks`（10 个 hook 字段）　`// Go: visitor.go`
- [ ] `new_node_visitor`/`visit_source_file`/`visit_node`/`visit_embedded_statement`/`visit_nodes`/`visit_modifiers`/`visit_slice`/`visit_each_child`/`lift_to_block` + 内部 `visit_*`（带 hook 分发）　`// Go: visitor.go:*`
- [ ] SyntaxList 提升语义（visit 返回 SyntaxList → 摊平/单子节点；多于 1 panic）　`// Go: visitor.go:VisitNode/VisitSlice`

### `deepclone.rs`（Go: `deepclone.go`）

- [ ] `get_deep_clone_visitor(arena, synthetic_location) -> NodeVisitor`（叶子强制 `clone`，列表/修饰符列表克隆 + 合成 Loc(-1,-1)/尾逗号 (-2,-2)）　`// Go: deepclone.go:getDeepCloneVisitor`
- [ ] `deep_clone_node`（synthetic=true）/`deep_clone_reparse`（synthetic=false + `set_parent_in_children` + 置 `NodeFlagsReparsed`）/`deep_clone_reparse_modifiers`　`// Go: deepclone.go:DeepClone*`

### `precedence.rs`（Go: `precedence.go`）

- [ ] `#[repr(i32)] enum OperatorPrecedence`（Comma..Primary + `Invalid=-1`/`Lowest`/`Highest`）　`// Go: precedence.go:OperatorPrecedence`
- [ ] 6 个函数：运算符→优先级映射、`get_binary_operator_precedence`、`get_expression_precedence`、`get_operator` 等（按 Go 实测）　`// Go: precedence.go:*`

### `parseoptions.rs`（Go: `parseoptions.go`）

- [ ] `SourceFileParseOptions`/`ExternalModuleIndicatorOptions`　`// Go: parseoptions.go`
- [ ] `get_external_module_indicator_options`/`set_external_module_indicator`/`is_file_probably_external_module`/`is_an_external_module_indicator_node`/`get_import_meta_if_necessary`/`is_file_module_from_using_jsx_tag`/`walk_tree_for_jsx_tags`/`find_child_node`　`// Go: parseoptions.go:*`

### `utilities.rs`（Go: `utilities.go`，417 函数）

> 体量巨大，按主题拆 `utilities/` 子模块（`predicates.rs`/`getters.rs`/`modifiers.rs`/`names.rs`/`declarations.rs`…），但**保持原文件名锚点**。逐函数 TODO 在执行期细列；本轮先登记主题与代表函数：

- [ ] modifier 工具：`has_syntactic_modifier`/`get_combined_modifier_flags`/`modifiers_to_flags`/`get_selected_effective_modifier_flags`　`// Go: utilities.go:*`
- [ ] 名字/声明：`get_name_of_declaration`/`is_declaration`/`is_declaration_name`/`get_declaration_modifier_flags_from_symbol`　`// Go: utilities.go:*`
- [ ] 大批 `Is*` 谓词与 `Get*` 取值（与 `ast_generated` 的 `IsXxx` 互补，覆盖组合判定）　`// Go: utilities.go:*`
- [ ] `set_parent_in_children`（写 `node.parent`；本包关键，deepclone/parser 用）　`// Go: utilities.go:SetParentInChildren`
- [ ] 节点定位/区间工具（`get_token_pos_of_node`/`skip_trivia` 协作项可能 `DEFER(phase-3)` 到 scanner）

### Cargo / crate 接线

- [ ] `internal/ast/Cargo.toml`（`name = "tsgo_ast"`，deps：`tsgo_core` `tsgo_collections` `tsgo_tspath` `tsgo_diagnostics` `bitflags` `rustc-hash` `xxhash-rust` [`la-arena`]）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明全部 `mod` + `pub use`；接入移植版 AST 生成器（xtask）

## TDD 推进顺序（tracer bullet → 增量）

1. **标志族**（`ids` + 8 个 `bitflags`）——纯位运算、无依赖、组合常量值可对 Go 逐一校验，补行为级 Rust 测试（值相等）。最先 green。
2. **`positionmap.rs`**——自包含、有 5 个 Go 单测（tracer bullet：直接把 `TestPositionMap*` 译过来 red→green）。
3. **`Kind` 枚举 + 最小 `NodeData`**（`Token`/`Identifier`/`QualifiedName`/`CallExpression`）+ `NodeArena.new_*` + `for_each_child` + `clone`——能 parse 出的最小子集（parser 未到，先手工建树）。
4. **`visitor.rs` + `deepclone.rs`**——`DeepCloneNode` 依赖 visitor 与 clone；用手工建树覆盖 `TestDeepCloneNodeSanityCheck` 的一小撮 case（如 Identifier/PropertyAccess/CallExpression）做 tracer bullet。
5. **生成器接管 `ast_generated.rs` / `kind_generated.rs`**——全量 193 variant，跑通 deepclone 全 ~270 case（需 parser？见偏离——deepclone 测试依赖 `parsetestutil.ParseTypeScript`，故该测试整体 `DEFER(phase-3)`，见 tests.md）。
6. **`symbol.rs` / `diagnostic.rs` / `precedence.rs` / `parseoptions.rs` / `utilities.rs`**——按上层（binder/checker）需要增量补齐；`utilities.rs` 随调用方拉动，不必一次写满 417 个。

## 与 Go 的已知偏离（divergence）

- **`node.Parent` → `parent(arena, id)`**：反向边改 arena 索引访问器。结构 1:1，语法偏离。PORTING §5 已授权。所有读 `Parent` 的调用点改造。
- **`data nodeData` 接口 → `enum NodeData` + match**：去掉 dyn 派发，换判别联合。`n.AsIdentifier()`（类型断言）→ `if let NodeData::Identifier(d) = &arena[id].data`。PORTING §3 授权。
- **`Node.id atomic.Uint64` → 复用 `NodeId` arena 下标**：省去懒分配原子计数。若 binder 对 id 的"懒分配顺序/稀疏性"有隐式依赖，回退为单独计数器并标 `// PERF(port)`。待 P3 binder 验证。
- **`Kind` 与 `NodeData` 双存**：不把 Kind 塌进 enum discriminant（否则 Token 的 150 种 Kind 丢失）。多占一个 `i16`，是 1:1 必需。
- **大 variant 装箱**：`SourceFileData`（含锁/缓存/数十字段）等大结构 `Box` 进 `NodeData`，控制 enum 体积。Go 无此问题（接口本就是指针）。
- **`SymbolTable` map 序**：Go `map[string]*Symbol` 无序；凡参与 emit/诊断输出顺序的符号迭代须改 `IndexMap`（PORTING §3）。binder/checker 落地时逐点判定，存疑 `// PERF(port)`。
- **生成器**：`ast_generated`/`kind_generated` 由移植版 `_scripts/generate-go-ast.ts` 产出（执行期作 xtask）。variant 字段名/`Kind` 名/区间常量须与 Go 逐一对齐（这些是跨包引用键）。
- **`utilities.go` 拆分**：417 函数拆子模块便于维护，但**保留 `// Go: utilities.go:Fn` 锚点**，不改函数语义。
- **`atomic`/`sync` 缓存**：`SourceFile` 的懒缓存（jsdoc/lineMap/positionMap/tokenCache）用 `OnceLock`/`RwLock`/`AtomicBool` 一一对映，语义等价。

## 转交 / 推迟（DEFER）

- **`deepclone_test.go` 整体依赖 parser**：测试用 `parsetestutil.ParseTypeScript(input)` 建树再克隆。parser 在 P3。故该测试的 red→green **完整收口推迟到 P3**（`// DEFER(phase-3): blocked-by tsgo_parser`）；P2 内用**手工建树**覆盖代表性 case 验证 `DeepCloneNode`/`VisitEachChild`/`for_each_child` 正确性。详见 tests.md。
- **`positionmap_test.go` 的 `BenchmarkComputePositionMap_CheckerTS`**：读 `_submodules/TypeScript/src/compiler/checker.ts`，缺文件时 `b.Skip`。Rust 用 `criterion`，缺文件跳过。benchmark 非 gate，记为可选。
- **`set_parse_jsdoc_for_node`**：JSDoc 懒解析回调由 parser（P3）注入；本包仅留注册点。`// DEFER(phase-3)`。
- **`utilities.rs` 中依赖 scanner 的项**（`skip_trivia`/`get_token_pos_of_node` 等）：`// DEFER(phase-3): blocked-by tsgo_scanner`。
- **`Type` 图**：`TypeId`/`TypeArena` 在 P4 checker；本包只落 `Symbol`/`SymbolId`。
- **`FlowNode` 完整构建**：本包定义结构与 flag；实际控制流图由 binder（P3）填充。
