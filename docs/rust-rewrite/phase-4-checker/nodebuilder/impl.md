# nodebuilder: 实现方案（impl.md）

**crate**：`tsgo_nodebuilder`　**目标**：定义 node builder（"把 `Type`/`Symbol` 序列化回 AST 类型节点"）的**接口与 flag 类型**。具体实现在 checker 之上，但这些类型/接口被 printer 里的 emit resolver 共用，故单独成包以打破 checker↔printer 的循环依赖。
**依赖（crate）**：`tsgo_ast`
**Go 源**：`internal/nodebuilder/`（1 个非测试文件 `types.go`，79 行）

## 这个包是什么（业务说明）

`d.ts` 声明文件生成 / hover 类型显示，都需要把内部 `Type`/`Symbol` 反向构造成 AST 类型节点（`TypeNode`）。这套构造逻辑（"node builder"）真正的实现挂在 checker 上（`internal/checker/nodebuilder*.go`），但 **emit 阶段的 `EmitResolver`（在 printer/declaration transformer 里调用）也要引用 node builder 的标志位与 `SymbolTracker` 回调接口**。

为了避免 `printer` ←→ `checker` 直接互相 import，TypeScript-Go 把这些"共享契约"（`SymbolTracker` trait、`Flags` / `InternalFlags` 位标志）抽到一个极小的叶子包 `nodebuilder`。它**只有类型定义，没有逻辑**。

它在 Phase 4 的位置：被 checker（实现 node builder）与后续 P5 printer（emit resolver）共用，所以放在 checker 之前移植。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包关键决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type SymbolTracker interface { ... }`（14 个方法） | `pub trait SymbolTracker { ... }` | 行为接口 → trait（PORTING §3）。方法接收的 `*ast.Symbol` / `*ast.Node` 在 Rust 侧是 `SymbolId` / `NodeId`（arena 索引，PORTING §5） |
| `type Flags uint32` + 一组 `1 << n` 常量 | `bitflags! { pub struct Flags: u32 { ... } }` | flags 类用 `bitflags`（PORTING §3/§10）。**注意有刻意的位空洞**（如 25/26/27/28/29/30 与低位混用）——必须逐位 1:1 复制 bit 值，不能重排 |
| `FlagsIgnoreErrors`（组合常量） | `Flags::IGNORE_ERRORS`（用 `\|` 组合的 `const`） | 组合保持与 Go 同一组成员 |
| `type InternalFlags int32` + 常量 | `bitflags! { pub struct InternalFlags: i32 { ... } }` | 同上 |
| 注释 "If modifying this enum, must modify `TypeFormatFlags` too!" | 在 rustdoc 保留该警告 | `Flags` 的位布局必须与 checker `TypeFormatFlags`、nodebuilder 互转对齐 |

**无所有权图**：纯类型/trait 定义包，无 arena、无状态。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/nodebuilder/types.go` | `internal/nodebuilder/lib.rs` | crate 根。`SymbolTracker` trait、`Flags`、`InternalFlags` |

## 依赖白名单（本包新增的 crate）

- `bitflags`——PORTING §10 白名单。无其它新增。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `types.go`）

- [x] `pub trait SymbolTracker`，逐方法（参数里 `*ast.Symbol`→`SymbolId`，`*ast.Node`→`NodeId`，`*ast.SourceFile`→`NodeId`，见 divergence）：　`// Go: types.go:SymbolTracker`
  - [x] `fn track_symbol(&mut self, symbol: SymbolId, enclosing_declaration: Option<NodeId>, meaning: SymbolFlags) -> bool`
  - [x] `fn report_inaccessible_this_error(&mut self)`
  - [x] `fn report_private_in_base_of_class_expression(&mut self, property_name: &str)`
  - [x] `fn report_inaccessible_unique_symbol_error(&mut self)`
  - [x] `fn report_cyclic_structure_error(&mut self)`
  - [x] `fn report_likely_unsafe_import_required_error(&mut self, specifier: &str, symbol_name: &str)`
  - [x] `fn report_truncation_error(&mut self)`
  - [x] `fn report_nonlocal_augmentation(&mut self, containing_file: NodeId, parent_symbol: SymbolId, augmenting_symbol: SymbolId)`
  - [x] `fn report_non_serializable_property(&mut self, property_name: &str)`
  - [x] `fn report_inference_fallback(&mut self, node: NodeId)`
  - [x] `fn push_error_fallback_node(&mut self, node: NodeId)`
  - [x] `fn pop_error_fallback_node(&mut self)`
- [x] `bitflags! pub struct Flags: u32`——逐位 1:1（含 25/27/28/29/30 高位与低位空洞）：`NoTruncation`(1<<0) … `OmitThisParameter`(1<<25) `AllowNodeModulesRelativePaths`(1<<26) `WriteCallStyleSignature`(1<<27) `UseSingleQuotesForStringLiteralType`(1<<28) `NoTypeReduction`(1<<29) `UseInstantiationExpressions`(1<<30)；状态位 `InObjectTypeLiteral`(1<<22)/`InTypeAlias`(1<<23)/`InInitialEntityName`(1<<24)　`// Go: types.go:Flags`
- [x] `const IGNORE_ERRORS`（= `AllowThisInObjectLiteral \| AllowQualifiedNameInPlaceOfIdentifier \| AllowAnonymousIdentifier \| AllowEmptyUnionOrIntersection \| AllowEmptyTuple \| AllowEmptyIndexInfoType \| AllowNodeModulesRelativePaths`）　`// Go: types.go:FlagsIgnoreErrors`
- [x] `bitflags! pub struct InternalFlags: i32`：`WriteComputedProps`(1<<0) `NoSyntacticPrinter`(1<<1) `DoNotIncludeSymbolChain`(1<<2) `AllowUnresolvedNames`(1<<3)　`// Go: types.go:InternalFlags`
- [x] rustdoc 保留 "must modify `TypeFormatFlags` too" 警告

### Cargo / crate 接线

- [x] `internal/nodebuilder/Cargo.toml`（`name = "tsgo_nodebuilder"` + path dep `tsgo_ast` + `bitflags`）
- [x] 根 `Cargo.toml` workspace members 追加（脚手架已置入 `internal/nodebuilder`）
- [x] `lib.rs` re-export `SymbolTracker` / `Flags` / `InternalFlags`（直接定义于 crate 根，无需额外 re-export）

## TDD 推进顺序（tracer bullet → 增量）

1. `Flags` / `InternalFlags` bitflags 定义 + 一个"位值快照"测试（断言每个常量的整型值与 Go 一致，见 tests.md）。
2. `SymbolTracker` trait（无逻辑，仅签名编译通过）+ 一个 mock 实现验证可对象安全（`dyn SymbolTracker`）。

## 与 Go 的已知偏离（divergence）

- `interface SymbolTracker` → `trait SymbolTracker`；接口方法的指针参数 → arena 索引（`SymbolId`/`NodeId`）。Go 里方法是"非可选"（注释提到曾经可选），Rust trait 全部为必需方法。
- **`*ast.SourceFile` → `NodeId`**（而非计划里的 `SourceFileId`）：现已落地的 `tsgo_ast` crate 只导出 `NodeId`/`SymbolId`，无 `SourceFileId`；而 SourceFile 本身就是一个 AST node，故 `report_nonlocal_augmentation` 的 `containing_file` 用 `NodeId` 表示。若后续 `tsgo_ast` 引入 `SourceFileId`，可窄化此参数。
- **可空指针 `enclosing_declaration *ast.Node` → `Option<NodeId>`**：这是 TrackSymbol 中惯例可空的参数（PORTING §3 可空指针 → `Option<Idx>`）。其余指针参数（`node`/`symbol`/`containing_file`）在调用点均为具体值，保留裸索引类型。
- `Flags`/`InternalFlags` 用 `bitflags`（`2.11.1`，与 `tsgo_ast` 同版本）而非裸整型，但**位值严格 1:1**（含 25/27/28/29/30 的乱序高位）。
- `InternalFlags` 底层是 `int32`（有符号），用 `bitflags ... : i32` 对齐。
- bitflags 内常量声明顺序按 TDD 逐行为推进而成组排列（options → 高位 → state → error → 复合 `IGNORE_ERRORS`），与 Go 源声明顺序不同，但每个常量的位值与 Go 字面量逐一对齐。

## 转交 / 推迟（DEFER）

- `SymbolTracker` 的具体实现（`SymbolTrackerImpl`）在 checker 的 `symboltracker.go`，归 checker 包；本包只定义接口。
- 该接口的"使用方" emit resolver 在 P5 printer，届时直接依赖本 crate。
