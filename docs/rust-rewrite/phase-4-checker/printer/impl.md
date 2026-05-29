# printer: 实现方案（impl.md）

**crate**：`tsgo_printer`　**目标**：把（经 transformers 降级后的）AST 打印成 TypeScript/JavaScript 源文本——含 emit 上下文（`EmitContext`）、emit 感知的节点工厂（`NodeFactory`）、名字生成器、parenthesizer（按优先级插括号）、注释/源映射发射、各类 `EmitTextWriter`。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_collections` `tsgo_debug` `tsgo_scanner` `tsgo_sourcemap` `tsgo_diagnostics` `tsgo_binder`(测试) `tsgo_stringutil`
**Go 源**：`internal/printer/`（15 个非测试文件，约 14789 行；`printer.go` 单文件 ~220KB 是 emit 核心）

## 这个包是什么（业务说明）

printer 是 **emit 的核心**。编译管线在 checker 之后、transformers 把 TS 专有语法 / 高版本语法降级之后，调用 printer 把最终 AST 序列化成文本。它做的事情远不止"拼字符串"：

1. **EmitContext**（`emitcontext.go`，35KB）：emit 期的"副状态"中心——把合成节点关联回原始节点（`Original`/`MostOriginal`）、记录 `EmitFlags`、注释范围、源映射范围、自动生成名字信息、emit helpers、词法/变量环境（hoist 声明）。transformers 与 printer 共享同一个 `EmitContext`。
2. **NodeFactory**（`factory.go`，48KB）：在 `ast.NodeFactory` 之上叠加 emit 语义的节点构造器（`NewTempVariable`/`NewUniqueName`/`NewGeneratedNameForNode`/各种 helper 调用表达式/名字解析 `GetLocalName` 等）。transformers 大量用它造新节点。
3. **Printer + parenthesizer**（`printer.go`）：递归 emit 每个 `ast.Node`；按运算符优先级/结合性决定何时插入 `( )`（parenthesizer，对应 105 个测试里近半数）；处理缩进、换行（ListFormat）、注释、source map 发射、JSX。
4. **NameGenerator**（`namegenerator.go`）：把 `_a/_b`、`_i`、`foo_1`、`#foo_1` 等自动名字按作用域生成且稳定。
5. **EmitTextWriter** 家族（`textwriter.go`/`singlelinestringwriter.go`/`changetrackerwriter.go`）：底层文本累积器，追踪行/列/缩进/尾随注释。

这是整个移植里最依赖 ast/checker/transformers 的包之一，也是测试最密集的包（105 个 `func Test`）。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包关键：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `EmitContext`（大量 `map[*ast.Node]X` 旁路表） | `pub struct EmitContext` + `FxHashMap<NodeId, X>`（按 `NodeId` 键） | **核心偏离**：Go 用节点指针作 map key；Rust 用 `NodeId`（PORTING §5），所有 `emitContext.X(node)` → 按 `node.id` 查表 |
| `EmitContext.Factory *NodeFactory`（自引用） | `Factory` 与 `EmitContext` 通过 `&mut`/索引解耦，或 `Factory` 持 `&mut EmitContext` | 注意 Go 里 factory 写回 emitContext 的旁路表；Rust 需借用规划（factory 方法取 `&mut self.context`） |
| `EmitFlags uint32`（iota 位标志） | `bitflags! { pub struct EmitFlags: u32 }` | `EFSingleLine`..`EFTransformPrivateStaticElements` + 组合 `EFNoSourceMap` 等 |
| `GeneratedIdentifierFlags int`（位标志 + Kind 掩码） | `bitflags!` 或 `#[repr(i32)]` + 掩码方法 | `IsAuto/IsLoop/IsUnique/IsNode/...` |
| `EmitTextWriter interface`（30 方法） | `pub trait EmitTextWriter` | 多实现：`TextWriter`/`SingleLineStringWriter`/`ChangeTrackerWriter` |
| `EmitHost`/`EmitResolver`/`SourceFileMetaDataProvider` interface | `pub trait EmitHost` / `EmitResolver` / `SourceFileMetaDataProvider` | 上层（compiler/checker）实现；注释标"thread-safe" → Rust `Send+Sync` 约束 |
| `Printer`（递归方法 + 大量 emit* 私有方法） | `pub struct Printer` + `impl Printer { fn emit_*(&mut self, ...) }` | ~数百私有 emit 方法 1:1（按 Kind 分派 `match`） |
| `strings.Builder`（writer 内部） | `String` | |
| `core.UTF16Offset`（列号） | `tsgo_core::Utf16Offset` | writer 的 GetColumn 等 |
| `NameGenerator`（scope 链表 + tempFlags） | `pub struct NameGenerator` + `Option<Box<NameGenerationScope>>` 链 | scope 用 `Box` 链表或 `Vec` 栈；`generatedNames: Set<String>` 用 `FxHashSet` |
| `*ast.NodeFactory` 嵌入 | 组合（`NodeFactory` 内含 `ast::NodeFactory`） + 委托 | emit 工厂在 ast 工厂上叠加旁路记录 |
| `sourcemap.Generator`（emit 时累积） | `tsgo_sourcemap::Generator`（path dep） | `Write(node, file, writer, sourceMapGenerator)` 注入 |

### 所有权图要点

- emit 期所有节点引用走 `NodeId` + arena（来自 `tsgo_ast`）。`EmitContext` 的所有旁路表（original/emitFlags/commentRange/sourceMapRange/autoGenerate/assignedName/classThis/typeNode/helpers/synthetic comments）键统一为 `NodeId`。
- `Printer` 持 `&EmitContext`（emit 期主要读，名字生成处写 NameGenerator 自身的 cache），写出到 `&mut dyn EmitTextWriter`，可选写 `&mut sourcemap::Generator`。
- `NameGenerator` 的作用域栈用链表/Vec；`GenerateName` 按节点/auto-id 缓存，命中走 object identity（这里 → `NodeId`/`AutoGenerateId`）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/printer/printer.go` | `internal/printer/lib.rs`（或 `printer.rs`，crate 根聚合） | `Printer`/`PrinterOptions`/`PrintHandlers`/`WriteKind`/`ListFormat` + emit 递归 + parenthesizer。**emit 核心** |
| `internal/printer/emitcontext.go` | `internal/printer/emitcontext.rs` | `EmitContext` + 全部旁路表 + 环境（变量/词法）管理 + helpers + 注释/源映射范围 + `AutoGenerateInfo`/`SynthesizedComment` |
| `internal/printer/factory.go` | `internal/printer/factory.rs` | `NodeFactory`：emit 感知节点构造器 + 名字解析（GetLocalName 等）+ helper 调用工厂 |
| `internal/printer/namegenerator.go` | `internal/printer/namegenerator.rs` | `NameGenerator` + scope/tempFlags |
| `internal/printer/utilities.go` | `internal/printer/utilities.rs` | `EscapeString`/范围-同行判定/`FormatGeneratedName`/`IsRecognizedTripleSlashComment`/`IsPinnedComment` 等 |
| `internal/printer/emitflags.go` | `internal/printer/emitflags.rs` | `EmitFlags` bitflags |
| `internal/printer/generatedidentifierflags.go` | `internal/printer/generatedidentifierflags.rs` | `GeneratedIdentifierFlags` |
| `internal/printer/emittextwriter.go` | `internal/printer/emittextwriter.rs` | `EmitTextWriter` trait |
| `internal/printer/textwriter.go` | `internal/printer/textwriter.rs` | `TextWriter` + `NewTextWriter` + `GetDefaultIndentSize` |
| `internal/printer/singlelinestringwriter.go` | `internal/printer/singlelinestringwriter.rs` | `SingleLineStringWriter` + `GetSingleLineStringWriter`（pool） |
| `internal/printer/changetrackerwriter.go` | `internal/printer/changetrackerwriter.rs` | `ChangeTrackerWriter`（供 P7 format/ls 复用） |
| `internal/printer/helpers.go` | `internal/printer/helpers.rs` | `EmitHelper`/`Priority`（emit helper 描述） |
| `internal/printer/emithost.go` | `internal/printer/emithost.rs` | `EmitHost` trait |
| `internal/printer/emitresolver.go` | `internal/printer/emitresolver.rs` | `EmitResolver` trait |
| `internal/printer/sourcefilemetadataprovider.go` | `internal/printer/sourcefilemetadataprovider.rs` | `SourceFileMetaDataProvider` trait |

## 依赖白名单（本包新增的 crate）

- `bitflags`（EmitFlags / GeneratedIdentifierFlags，已在 §10）。
- 复用 `tsgo_sourcemap`（VLQ/Generator）、`tsgo_scanner`（行列换算/token 文本）、`tsgo_stringutil`（转义辅助）。无额外第三方。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> printer.go/factory.go/emitcontext.go 体量巨大，下面列**公开 API 与测试直接触达的核心项**；其余私有 emit*/Newxxx 构造器按"逐 Kind / 逐节点类型"在执行期补全（与 ast NodeKind 一一对应）。

### `emitflags.rs` / `generatedidentifierflags.rs`（先行，无依赖）

- [ ] `bitflags EmitFlags`（全部 24 个 flag + `EFNone/EFNoSourceMap/EFNoTokenSourceMaps/EFNoComments` 组合）　`// Go: emitflags.go`
- [ ] `GeneratedIdentifierFlags` + `Kind/IsAuto/IsLoop/IsUnique/IsNode/IsReservedInNestedScopes/IsOptimistic/IsFileLevel/HasAllowNameSubstitution`　`// Go: generatedidentifierflags.go`

### `emittextwriter.rs` / `textwriter.rs` / `singlelinestringwriter.rs` / `changetrackerwriter.rs`

- [ ] `pub trait EmitTextWriter`（30 方法）　`// Go: emittextwriter.go:EmitTextWriter`
- [ ] `pub fn get_default_indent_size() -> usize`　`// Go: textwriter.go:GetDefaultIndentSize`
- [ ] `pub fn new_text_writer(new_line, indent_size) -> impl EmitTextWriter` + `TextWriter` 全部方法（行/列/缩进/尾随注释追踪）　`// Go: textwriter.go:NewTextWriter`
- [ ] `pub fn get_single_line_string_writer() -> (impl EmitTextWriter, impl FnOnce())`（pool + 归还）　`// Go: singlelinestringwriter.go:GetSingleLineStringWriter`
- [ ] `pub fn new_change_tracker_writer(newline, indent_size) -> ChangeTrackerWriter`　`// Go: changetrackerwriter.go:NewChangeTrackerWriter`

### `utilities.rs`（Go: `internal/printer/utilities.go`）—— 被 `utilities_test.go` 直接覆盖

- [ ] `pub fn escape_string(s, quote_char) -> String`　`// Go: utilities.go:EscapeString`
- [ ] `fn escape_non_ascii_string(s, quote_char) -> String`（私有，被测试经包内调用）　`// Go: utilities.go:escapeNonAsciiString`
- [ ] `fn escape_jsx_attribute_string(s, quote_char) -> String`（私有，被测试经包内调用）　`// Go: utilities.go:escapeJsxAttributeString`
- [ ] `pub enum QuoteChar`（DoubleQuote/SingleQuote/Backtick）　`// Go: utilities.go`
- [ ] `pub fn is_recognized_triple_slash_comment(text, comment_range) -> bool`　`// Go: utilities.go:IsRecognizedTripleSlashComment`
- [ ] `pub fn is_pinned_comment(text, comment) -> bool`　`// Go: utilities.go:IsPinnedComment`
- [ ] `pub fn range_is_on_single_line / range_start_positions_are_on_same_line / positions_are_on_same_line / get_lines_between_positions`　`// Go: utilities.go:*`
- [ ] `pub fn is_file_level_unique_name(source_file, name, has_global_name) -> bool`　`// Go: utilities.go:IsFileLevelUniqueName`
- [ ] `pub fn format_generated_name(private_name, prefix, base, suffix) -> String`　`// Go: utilities.go:FormatGeneratedName`

### `namegenerator.rs`（Go: `internal/printer/namegenerator.go`）—— 被 `namegenerator_test.go`（36 测试）覆盖

- [ ] `pub struct NameGenerator { context, is_file_level_unique_name_in_current_file, get_text_of_node, ...caches }`　`// Go: namegenerator.go:NameGenerator`
- [ ] `pub fn push_scope(&mut self, reuse_temp_variable_scope)` / `pub fn pop_scope(...)`　`// Go: namegenerator.go:PushScope/PopScope`
- [ ] `pub fn generate_name(&mut self, name: &ast::Node) -> String`（按 GeneratedIdentifierFlags 分派：auto/loop/unique/node）　`// Go: namegenerator.go:GenerateName`
- [ ] temp/loop 名字递增（`_a/_b`、`_i`）、formatted name（prefix/suffix）、reserved-in-nested-scopes、unique（`foo_1`）、private（`#foo_1`）、node-based（reuse vs `_1` 后缀）逻辑　`// Go: namegenerator.go:generateName*/makeName/...`

### `emitcontext.rs`（Go: `internal/printer/emitcontext.go`）

- [ ] `pub struct EmitContext` + `pub fn new() -> EmitContext` / `get_emit_context() -> (..., reset)`（pool）　`// Go: emitcontext.go:NewEmitContext/GetEmitContext`
- [ ] `Reset` / `NewNodeVisitor`　`// Go: emitcontext.go:Reset/NewNodeVisitor`
- [ ] 变量环境：`StartVariableEnvironment/EndVariableEnvironment/EndAndMergeVariableEnvironment(List)/AddVariableDeclaration/AddHoistedFunctionDeclaration`　`// Go: emitcontext.go:*VariableEnvironment*`
- [ ] 词法环境：`StartLexicalEnvironment/EndLexicalEnvironment/EndAndMergeLexicalEnvironment(List)/AddLexicalDeclaration/MergeEnvironment(List)`　`// Go: emitcontext.go:*LexicalEnvironment* / MergeEnvironment*`
- [ ] 自动名字：`HasAutoGenerateInfo/GetAutoGenerateInfo/GetNodeForGeneratedName` + `AutoGenerateOptions/AutoGenerateId/AutoGenerateInfo`　`// Go: emitcontext.go:*AutoGenerate*`
- [ ] original 链：`SetOriginal/UnsetOriginal/SetOriginalEx/Original/MostOriginal/ParseNode`　`// Go: emitcontext.go:*Original*/ParseNode`
- [ ] emit flags：`EmitFlags/SetEmitFlags/AddEmitFlags`　`// Go: emitcontext.go:*EmitFlags`
- [ ] 注释/源映射范围：`CommentRange/SetCommentRange/AssignCommentRange/SourceMapRange/SetSourceMapRange/AssignSourceMapRange/AssignCommentAndSourceMapRanges/TokenSourceMapRange/SetTokenSourceMapRange`　`// Go: emitcontext.go:*Range*`
- [ ] 合成注释：`SetSyntheticLeadingComments/AddSyntheticLeadingComment/GetSyntheticLeadingComments`（trailing 同）+ `SynthesizedComment`　`// Go: emitcontext.go:*SyntheticComment*`
- [ ] assigned name / classThis / typeNode：`AssignedName/SetAssignedName/TextSource/ClassThis/SetClassThis/SetTypeNode/GetTypeNode`　`// Go: emitcontext.go:*`
- [ ] emit helpers：`RequestEmitHelper/ReadEmitHelpers/AddEmitHelper/MoveEmitHelpers/GetEmitHelpers/GetExternalHelpersModuleName/SetExternalHelpersModuleName/HasRecordedExternalHelpers/IsCallToHelper`　`// Go: emitcontext.go:*EmitHelper*/*ExternalHelpers*`
- [ ] visitor 辅助：`VisitVariableEnvironment/VisitParameters/VisitFunctionBody/VisitIterationBody/VisitEmbeddedStatement/AddInitializationStatement`　`// Go: emitcontext.go:Visit*`
- [ ] `NewNotEmittedStatement`　`// Go: emitcontext.go:NewNotEmittedStatement`

### `factory.rs`（Go: `internal/printer/factory.go`）

- [ ] `pub struct NodeFactory` + `pub fn new_node_factory(context) -> NodeFactory`　`// Go: factory.go:NewNodeFactory`
- [ ] 名字工厂（测试直接用）：`NewTempVariable(Ex)/NewLoopVariable(Ex)/NewUniqueName(Ex)/NewGeneratedNameForNode(Ex)/NewUniquePrivateName(Ex)/NewGeneratedPrivateNameForNode(Ex)`　`// Go: factory.go:New*Variable/New*Name*`
- [ ] 名字解析：`GetLocalName(Ex)/GetExportName(Ex)/GetDeclarationName(Ex)/GetNamespaceMemberName/GetExternalModuleOrNamespaceExportName` + `NameOptions/AssignedNameOptions`　`// Go: factory.go:Get*Name*`
- [ ] 表达式便捷构造：`NewCommaExpression/NewAssignmentExpression/NewLogicalOR/AND/StrictEquality/Inequality/VoidZero/ThisExpression/True/False/InlineExpressions/CreateExpressionFromEntityName`　`// Go: factory.go:New*/Inline*/Create*`
- [ ] helper 调用工厂：`NewUnscopedHelperName/NewDecorateHelper/NewMetadataHelper/NewParamHelper/NewAddDisposableResourceHelper/NewDisposeResourcesHelper/NewMethodCall/NewGlobalMethodCall/NewFunctionCallCall/NewArraySliceCall/NewTypeCheck`　`// Go: factory.go:New*Helper/New*Call*`
- [ ] 语句工具：`RestoreEnclosingLabel/CreateForOfBindingStatement/RestoreOuterExpressions/EnsureUseStrict/SplitStandardPrologue/SplitCustomPrologue`　`// Go: factory.go:*`
- [ ] 其余 ~数百 `New<NodeKind>` 构造器（与 `ast::NodeFactory` 对应，叠加 emit 记录）——逐 NodeKind 补全　`// Go: factory.go:New*`

### `helpers.rs` / `emithost.rs` / `emitresolver.rs` / `sourcefilemetadataprovider.rs`

- [ ] `pub struct EmitHelper { ... }` + `pub struct Priority`　`// Go: helpers.go:EmitHelper/Priority`
- [ ] `pub trait EmitHost`（`Send+Sync`）　`// Go: emithost.go:EmitHost`
- [ ] `pub trait EmitResolver`　`// Go: emitresolver.go:EmitResolver`
- [ ] `pub trait SourceFileMetaDataProvider`　`// Go: sourcefilemetadataprovider.go`

### `lib.rs` / `printer.rs`（Go: `internal/printer/printer.go`）—— emit 核心，被 `printer_test.go`（65 测试）覆盖

- [ ] `pub struct PrinterOptions` / `pub struct PrintHandlers` / `pub struct Printer` / `pub enum WriteKind` / `pub struct ListFormat`(bitflags-like)　`// Go: printer.go:PrinterOptions/PrintHandlers/Printer/WriteKind/ListFormat`
- [ ] `pub fn new_printer(options, handlers, emit_context) -> Printer`　`// Go: printer.go:NewPrinter`
- [ ] `pub fn emit(&mut self, node, source_file) -> String`　`// Go: printer.go:Emit`
- [ ] `pub fn emit_source_file(&mut self, source_file) -> String`　`// Go: printer.go:EmitSourceFile`
- [ ] `pub fn write(&mut self, node, source_file, writer, source_map_generator)`　`// Go: printer.go:Write`
- [ ] `fn emit_<Kind>(...)` 全节点族递归 emit（表达式/语句/声明/类型节点/JSX/绑定模式/签名/修饰符/token）——逐 Kind 1:1（对应 `TestEmit` 的 ~290 子用例所覆盖的全部 NodeKind）　`// Go: printer.go:emit*`
- [ ] **parenthesizer**：`fn parenthesize_*`（decorator/computed-property/array-literal/property-access/element-access/call/new/tagged-template/type-assertion/arrow/delete/void/typeof/await/binary/conditional/yield/spread/expr-with-type-args/as/satisfies/non-null/expression-statement/default-expression + 类型侧 array/optional/union/intersection/readonly/keyof/indexed-access/conditional-type）——对应 `printer_test.go` 的 Parenthesize* 测试　`// Go: printer.go:parenthesize*`
- [ ] 缩进/换行/列表（`ListFormat`）、注释发射、source map 发射、name substitution、JSX 文本处理　`// Go: printer.go:*`

### Cargo / crate 接线

- [ ] `internal/printer/Cargo.toml`（`name = "tsgo_printer"` + path deps + `bitflags`）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] crate 根（`lib.rs`）声明全部 `mod` + `pub use`

## TDD 推进顺序（tracer bullet → 增量）

1. `emitflags.rs` + `generatedidentifierflags.rs`（纯枚举/位标志，无依赖）。
2. `utilities.rs` 的 `EscapeString`/`escapeNonAsciiString`/`escapeJsxAttributeString`/`IsRecognizedTripleSlashComment`（**这 4 个是 `utilities_test.go` 的全部，且不依赖 emit 主循环，最佳 tracer bullet**）。
3. `EmitTextWriter` + `TextWriter`（最小可写出文本）。
4. `EmitContext`（最小：original/emitFlags/auto-generate 表）+ `NameGenerator`（过 `namegenerator_test.go` 36 个：temp/loop/unique/private/generated-for-node）。
5. `Printer` 主循环：先过 `TestEmit` 的字面量/标识符/成员访问 → 表达式 → 语句 → 声明 → 类型节点 → JSX（按节点族增量），每加一族 NodeKind 过对应子用例。
6. parenthesizer：过 `printer_test.go` 的全部 `TestParenthesize*`（按表达式优先级、类型优先级），含 nullish-coalescing 混用、partially-emitted-expression。
7. `factory.rs` 的名字工厂/helper 工厂（多数由 transformers 在 P5 同期驱动；本包先满足 printer_test 用到的 `NewTempVariable` 等）。

## 与 Go 的已知偏离（divergence）

- **节点指针 → NodeId**：`EmitContext` 全部旁路表 key、`NameGenerator` 的节点缓存 key、parenthesizer 的节点比较，由 Go 指针/`x == y` 改为 `NodeId` 相等（PORTING §5）。`TestUniqueName2`/`TestGeneratedNameForNodeCached` 依赖 "object identity" 缓存——Rust 用 `NodeId` 等价实现。
- **借用规划**：Go 里 `EmitContext.Factory` 自引用、factory 写回 context 旁路表，是裸指针图；Rust 需让 `NodeFactory` 方法接收 `&mut EmitContext`（或 context 持 `RefCell` 旁路表），避免别名冲突。这是结构保真的必要偏离，落地时在文件头"所有权模型"小节细化。
- `GetSingleLineStringWriter`/`GetEmitContext` 用对象池（sync.Pool 风格）→ Rust 用线程局部池或简单 `thread_local!`；不影响行为。
- `EmitHost` 注释要求 thread-safe → trait 加 `Send + Sync`。
- `ListFormat` 在 Go 是 `int` 位枚举 → `bitflags`。

## 转交 / 推迟（DEFER）

- `EmitResolver`/`EmitHost`/`SourceFileMetaDataProvider` 的实际实现来自 checker（P4）/compiler（P6）；本包定义 trait + 在测试用 fake/nil。
- declaration emit（`.d.ts`）由 `transformers/declarations`（本 phase transformers 包）+ printer 协作；printer 侧只负责打印，声明生成逻辑在 transformers。
- 完整 emit parity（fourslash/conformance baseline）`// DEFER(phase-10)`；本轮单测 gate 为 `printer_test.go` 的 105 个 func。
