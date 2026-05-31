# printer: 实现方案（impl.md）

> **phase 归属（依赖序修正）**：本包**前移到 P4**（原列 P5）。原因：`checker` 非测试依赖 `tsgo_printer`（如 `nodebuilder.go`），须早于 checker；`transformers`（依赖 printer/checker）相应留在 checker 之后（P5）。详见根 README「依赖序口径」与 [references/crate-map.md](../../references/crate-map.md)。

**crate**：`tsgo_printer`　**目标**：把（经 transformers 降级后的）AST 打印成 TypeScript/JavaScript 源文本——含 emit 上下文（`EmitContext`）、emit 感知的节点工厂（`NodeFactory`）、名字生成器、parenthesizer（按优先级插括号）、注释/源映射发射、各类 `EmitTextWriter`。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_collections` `tsgo_debug` `tsgo_scanner` `tsgo_sourcemap` `tsgo_diagnostics` `tsgo_binder`(测试) `tsgo_stringutil`
**Go 源**：`internal/printer/`（15 个非测试文件，约 14789 行；`printer.go` 单文件 ~220KB 是 emit 核心）

## 实现进度（执行期 / 本轮 subagent）

已落地并全绿（`cargo test -p tsgo_printer` + doctests + `cargo clippy -p tsgo_printer --all-targets -D warnings`）：

| 切片 | Rust 文件 | 状态 | 覆盖测试 |
|---|---|---|---|
| EmitFlags | `emitflags.rs` (+`emitflags_test.rs`) | ✓ | bit/组合值 |
| GeneratedIdentifierFlags | `generatedidentifierflags.rs` (+test) | ✓ | Kind/Is*/Has* |
| escape/triple-slash/format-name | `utilities.rs` (+test) | ✓ | `utilities_test.go` 全 4 函数 |
| EmitTextWriter trait | `emittextwriter.rs` (+test) | ✓ | trait-object 行为 |
| TextWriter | `textwriter.rs` (+test) | ✓ | 行/列/缩进/trailing |
| EmitContext（最小：autoGenerate 表 + arena 所有权） | `emitcontext.rs` (+test) | ✓ | has/get auto-generate |
| NodeFactory（名字工厂：temp/loop/unique/private） | `factory.rs` (+test) | ✓ | 4 种名字 + flags 组合 |
| NameGenerator（temp/loop/unique/private + scope + node-based） | `namegenerator.rs` (+test) | ✓ | `namegenerator_test.go` 36 中的 31 条（temp/loop/unique/private 16 + node-based 15；namespace/import/export 5 DEFER）|

**Go → Rust 文件对照（已落地）**：`emitflags.go→emitflags.rs`、`generatedidentifierflags.go→generatedidentifierflags.rs`、`utilities.go→utilities.rs`(部分)、`emittextwriter.go→emittextwriter.rs`、`textwriter.go→textwriter.rs`、`emitcontext.go→emitcontext.rs`(部分)、`factory.go→factory.rs`(部分)、`namegenerator.go→namegenerator.rs`。

### Round 2 进度（emit 核心已落地）

emit* 巨型 switch 已逐 NodeKind 移植并全绿，按 §2 拆分：

| Rust 文件 | 覆盖 | 状态 |
|---|---|---|
| `printer.rs` | Printer/PrinterOptions/PrintHandlers/WriteKind + emit 入口 + emitList/emitListItems + 行终止符计数 + token/name/literal-text 助手 + enter/exit（注释/源映射 no-op 路径） | ✓ |
| `list_format.rs` | `ListFormat` bitflags（全部单 bit + 预计算组合） | ✓ |
| `literal_text.rs` | `get_literal_text` + `can_use_original_text` | ✓ |
| `emit_expressions.rs` | 字面量/标识符/成员&元素访问/调用/new/一元/二元/条件/模板/spread/数组&对象字面量/箭头&函数&类表达式/as/satisfies/nonnull/meta/tagged-template/type-assertion | ✓ |
| `emit_statements.rs` | block/empty/var/expr/if/do/while/for/for-in/for-of/continue/break/return/with/switch/case/default/labeled/throw/try/debugger + binding pattern/element | ✓ |
| `emit_declarations.rs` | function/class/interface/type-alias/enum/module/import(-equals)/export(-assignment/declaration) + class&type 成员（method/accessor/constructor/property/index/call/construct sig/static block）+ heritage + 名字工厂调用 | ✓ |
| `emit_types.rs` | `TypePrecedence` + `get_type_node_precedence` + 全部类型节点（keyword/reference/function/constructor/query/literal/array/tuple/rest/optional/named-member/union/intersection/conditional/infer/paren/this/operator/indexed-access/mapped/literal-type/template-type/import-type）含精度括号 | ✓ |
| `emit_jsx.rs` | element/self-closing/fragment/opening&closing/attributes/spread/expression/text/namespaced-name/tag-name | ✓ |
| `parenthesizer.rs` | 精度驱动括号（在各 emit 函数内）+ 合成 AST 测试 | ✓（测试部分见 tests.md） |

**位置/换行助手**（来自 `utilities.go`）：`get_lines_between_positions/positions_are_on_same_line/range_*_on_same_line/range_is_on_single_line` 已移植到 `utilities.rs`；`get_literal_text` 移到 `literal_text.rs`。

**关键偏离（round 2）**：
- **源文本注入**：Rust `SourceFile` 不携带源文本，emit 入口显式接收 `text`（Go 从 `sourceFile.Text()` 取）。
- **注释/源映射 emit = no-op 路径**：`TestEmit` 关闭 source map 且（几乎）无注释；`enter_node`/`exit_node` 为结构占位。真正的注释 emit（`emitLeadingComments` 等，依赖 scanner 注释扫描）与 source-map VLQ 发射列为后续切片（对应 2 个含注释的 `TestEmit` 子用例 ArrayLiteral#6/JsxElement12 推迟）。
- **`MultiLine` 标志**：Go 的 `ArrayLiteral/ObjectLiteral/Block.MultiLine` 字段未被 Rust AST 携带；解析出的单行字面量按 `false` 处理（对全部 `TestEmit` 子用例正确）。

**仍 DEFER（后续/round 3）**：

- **node-based 生成名字**（`generateNameForNode`）：**[6p] 已落地可达 kind**（Identifier/Function-Class-decl/ExportAssignment/ClassExpression/Method-Accessor/ComputedPropertyName/default + 节点缓存，15 条）。仅 ModuleDeclaration/EnumDeclaration（`isUniqueLocalName` 需 binder `Locals`）与 Import/Export（`GetExternalModuleName`）的 5 条仍 DEFER。
- **注释 + source-map emit**：`enterNode`/`exitNode` 的真实注释发射、`getLeadingCommentRanges` 经 `tsgo_scanner` 本地接线、VLQ source map。
- **parenthesizer 剩余合成用例**：本轮移植了 `TestParenthesizeBinary`(15) + conditional1/2 + spread1 + call4 + new2 + as（共 ~21/58），其余 ~37 条合成 AST 用例 round 3 续。
- **解析器限制导致推迟的 `TestEmit` 子用例**（非 printer 责任，`tsgo_parser` 不支持）：动态 `import()`（解析器死循环）、`using`/`await using` 声明、`global{}` 增强块、可选元组元素 `[a?]`/`[a?: b]`。
- **NameGenerator 的 node-based 名字 namespace/import-export 子集**（`generateNameForModuleOrEnum`/`isUniqueLocalName` + `generateNameForImportOrExportDeclaration`）：仅这 5 条仍 `// DEFER(phase-4)` + `// blocked-by:`。阻塞：`isUniqueLocalName` 需 binder 的 `Locals`/`NextContainer` + `IsNodeDescendantOf`；import/export 需 `GetExternalModuleName`/`makeIdentifierFromModuleName`。其余 node-based 路径已在 [6p] 落地（见下「Round 6p worklog」）。
- **EmitContext 其余旁路表 / 环境 / helpers**、**factory 的表达式便捷构造 + helper 调用工厂 + 名字解析**、**`helpers.rs`/`emithost.rs`/`emitresolver.rs`/`sourcefilemetadataprovider.rs`/`singlelinestringwriter.rs`/`changetrackerwriter.rs`**：随 emit 核心同期推进，本轮未做。
- **`utilities.go` 的 range/line/`getLiteralText`/`getContainingNodeArray` 等**（依赖 arena/scanner/EmitContext）：随 emit 核心推进。

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

- [x] `bitflags EmitFlags`（全部 24 个 flag + `EFNone/EFNoSourceMap/EFNoTokenSourceMaps/EFNoComments` 组合）　`// Go: emitflags.go`
- [x] `GeneratedIdentifierFlags` + `Kind/IsAuto/IsLoop/IsUnique/IsNode/IsReservedInNestedScopes/IsOptimistic/IsFileLevel/HasAllowNameSubstitution`　`// Go: generatedidentifierflags.go`

### `emittextwriter.rs` / `textwriter.rs` / `singlelinestringwriter.rs` / `changetrackerwriter.rs`

- [x] `pub trait EmitTextWriter`（30 方法）　`// Go: emittextwriter.go:EmitTextWriter`
- [x] `pub fn get_default_indent_size() -> i32`　`// Go: textwriter.go:GetDefaultIndentSize`（偏离：返回 i32 对齐 indent 算术）
- [x] `pub fn new_text_writer(new_line, indent_size) -> TextWriter` + `TextWriter` 全部方法（行/列/缩进/尾随注释追踪）　`// Go: textwriter.go:NewTextWriter`
- [ ] `pub fn get_single_line_string_writer() -> (impl EmitTextWriter, impl FnOnce())`（pool + 归还）　`// Go: singlelinestringwriter.go:GetSingleLineStringWriter`　— DEFER
- [ ] `pub fn new_change_tracker_writer(newline, indent_size) -> ChangeTrackerWriter`　`// Go: changetrackerwriter.go:NewChangeTrackerWriter`　— DEFER(phase-7)

### `utilities.rs`（Go: `internal/printer/utilities.go`）—— 被 `utilities_test.go` 直接覆盖

- [x] `pub fn escape_string(s, quote_char) -> String`　`// Go: utilities.go:EscapeString`
- [x] `pub fn escape_non_ascii_string(s, quote_char) -> String`（设为 pub 以避免未接线时的 dead_code）　`// Go: utilities.go:escapeNonAsciiString`
- [x] `pub fn escape_jsx_attribute_string(s, quote_char) -> String`（同上）　`// Go: utilities.go:escapeJsxAttributeString`
- [x] `pub enum QuoteChar`（DoubleQuote/SingleQuote/Backtick）　`// Go: utilities.go`
- [x] `pub fn is_recognized_triple_slash_comment(text, comment_range) -> bool`（含本地 `CommentRange`，ast 尚未移植该类型）　`// Go: utilities.go:IsRecognizedTripleSlashComment`
- [x] `pub fn is_pinned_comment(text, comment) -> bool`　`// Go: utilities.go:IsPinnedComment`
- [ ] `pub fn range_is_on_single_line / range_start_positions_are_on_same_line / positions_are_on_same_line / get_lines_between_positions`　`// Go: utilities.go:*`　— DEFER（随 emit 核心）
- [ ] `pub fn is_file_level_unique_name(source_file, name, has_global_name) -> bool`　`// Go: utilities.go:IsFileLevelUniqueName`　— DEFER
- [x] `pub fn format_generated_name(private_name, prefix, base, suffix) -> String`　`// Go: utilities.go:FormatGeneratedName`

### `namegenerator.rs`（Go: `internal/printer/namegenerator.go`）—— 被 `namegenerator_test.go`（36 测试）覆盖

- [x] `pub struct NameGenerator { context, ...caches }`（file-level/get-text 回调随 node-based 路径一起补）　`// Go: namegenerator.go:NameGenerator`
- [x] `pub fn push_scope(&mut self, reuse_temp_variable_scope)` / `pub fn pop_scope(...)`　`// Go: namegenerator.go:PushScope/PopScope`
- [x] `pub fn generate_name(&mut self, name: NodeId) -> String`（auto/loop/unique **+ node 分派已实现**：node 经 `get_node_for_generated_name` 解析 + `generate_name_for_node_cached` 缓存）　`// Go: namegenerator.go:GenerateName`　**[6p]**
- [x] temp/loop（`_a/_b`、`_i`）、formatted name（prefix/suffix）、reserved-in-nested-scopes、unique（`foo_1`）、private（`#foo_1`）已实现；**node-based（`generateNameForNode`）[6p] 落地可达 kind**：Identifier/PrivateIdentifier（`makeUniqueName`，含 optimistic prefix/suffix）、Function/Class declaration（递归 name 或 export-default）、ExportAssignment（`default_n`）、ClassExpression（`class_n`）、Method/Get/SetAccessor（`generateNameForMethodOrAccessor`）、ComputedPropertyName（temp，reserved-in-nested-scopes）、default（temp）+ 节点缓存（node-id keyed，含 `generateNameForNodeCached` 稳定性）。**仍 DEFER**：ModuleDeclaration/EnumDeclaration（`isUniqueLocalName` 需 binder `Locals`/`NextContainer`）、Import/Export declaration（`GetExternalModuleName`/`makeIdentifierFromModuleName`）　`// Go: namegenerator.go:generateName*/makeName/...`　**[6p]**

### `emitcontext.rs`（Go: `internal/printer/emitcontext.go`）

- [x] `pub struct EmitContext` + `pub fn new() -> EmitContext`（+ `with_arena`，拥有单一 `NodeArena`；pool `get_emit_context` DEFER）　`// Go: emitcontext.go:NewEmitContext`
- [ ] `Reset` / `NewNodeVisitor`　`// Go: emitcontext.go:Reset/NewNodeVisitor`
- [ ] 变量环境：`StartVariableEnvironment/EndVariableEnvironment/EndAndMergeVariableEnvironment(List)/AddVariableDeclaration/AddHoistedFunctionDeclaration`　`// Go: emitcontext.go:*VariableEnvironment*`
- [ ] 词法环境：`StartLexicalEnvironment/EndLexicalEnvironment/EndAndMergeLexicalEnvironment(List)/AddLexicalDeclaration/MergeEnvironment(List)`　`// Go: emitcontext.go:*LexicalEnvironment* / MergeEnvironment*`
- [x] 自动名字：`has_auto_generate_info/get_auto_generate_info` + `AutoGenerateOptions/AutoGenerateId/AutoGenerateInfo`（`GetNodeForGeneratedName` DEFER 随 node-based 名字）　`// Go: emitcontext.go:*AutoGenerate*`
- [ ] original 链：`SetOriginal/UnsetOriginal/SetOriginalEx/Original/MostOriginal/ParseNode`　`// Go: emitcontext.go:*Original*/ParseNode`
- [ ] emit flags：`EmitFlags/SetEmitFlags/AddEmitFlags`　`// Go: emitcontext.go:*EmitFlags`
- [ ] 注释/源映射范围：`CommentRange/SetCommentRange/AssignCommentRange/SourceMapRange/SetSourceMapRange/AssignSourceMapRange/AssignCommentAndSourceMapRanges/TokenSourceMapRange/SetTokenSourceMapRange`　`// Go: emitcontext.go:*Range*`
- [ ] 合成注释：`SetSyntheticLeadingComments/AddSyntheticLeadingComment/GetSyntheticLeadingComments`（trailing 同）+ `SynthesizedComment`　`// Go: emitcontext.go:*SyntheticComment*`
- [ ] assigned name / classThis / typeNode：`AssignedName/SetAssignedName/TextSource/ClassThis/SetClassThis/SetTypeNode/GetTypeNode`　`// Go: emitcontext.go:*`
- [ ] emit helpers：`RequestEmitHelper/ReadEmitHelpers/AddEmitHelper/MoveEmitHelpers/GetEmitHelpers/GetExternalHelpersModuleName/SetExternalHelpersModuleName/HasRecordedExternalHelpers/IsCallToHelper`　`// Go: emitcontext.go:*EmitHelper*/*ExternalHelpers*`
- [ ] visitor 辅助：`VisitVariableEnvironment/VisitParameters/VisitFunctionBody/VisitIterationBody/VisitEmbeddedStatement/AddInitializationStatement`　`// Go: emitcontext.go:Visit*`
- [ ] `NewNotEmittedStatement`　`// Go: emitcontext.go:NewNotEmittedStatement`

### `factory.rs`（Go: `internal/printer/factory.go`）

- [x] `pub struct NodeFactory<'a>` + `EmitContext::factory()`（借 `&mut EmitContext`）　`// Go: factory.go:NewNodeFactory`
- [x] 名字工厂：`new_temp_variable(_ex)/new_loop_variable(_ex)/new_unique_name(_ex)/new_unique_private_name(_ex)`　已实现；**`new_generated_name_for_node(_ex)/new_generated_private_name_for_node(_ex)` [6p] 落地**（node-based；prefix/suffix → `OPTIMISTIC`）　`// Go: factory.go:New*Variable/New*Name*/NewGeneratedNameForNode*`　**[6p]**
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

- [x] `PrinterOptions` / `PrintHandlers` / `Printer` / `WriteKind` / `ListFormat`（`list_format.rs` bitflags）　`// Go: printer.go:PrinterOptions/PrintHandlers/Printer/WriteKind/ListFormat`
- [x] `Printer::new(options, handlers, &EmitContext)`　`// Go: printer.go:NewPrinter`
- [x] `emit_source_file(&mut self, source_file, text) -> String`（源文本注入偏离）　`// Go: printer.go:Emit/EmitSourceFile`
- [ ] `write(node, source_file, writer, source_map_generator)`（外部 writer + source map）— DEFER（随注释/源映射切片）　`// Go: printer.go:Write`
- [x] `fn emit_<Kind>(...)` 全节点族递归 emit（表达式/语句/声明/类型/JSX/绑定模式/签名/修饰符/token）—— 已逐 Kind 移植（见上 Round 2 表）　`// Go: printer.go:emit*`
- [x] **parenthesizer**：精度驱动括号已在 `emit_expression`/`emit_types`/`emit_callee`/`emit_new_expression`/`emit_expression_statement`/`emit_concise_body` 实现；合成 AST 测试见 `parenthesizer_test.rs`　`// Go: printer.go:parenthesize*`
- [x] 缩进/换行/列表（`ListFormat`）、name substitution、JSX；**注释发射 / source map 发射 = no-op 路径（DEFER）**　`// Go: printer.go:*`

### Cargo / crate 接线

- [x] `internal/printer/Cargo.toml`（`name = "tsgo_printer"` + path deps + `bitflags` + `rustc-hash` + dev-dep `tsgo_parser`）
- [x] 根 `Cargo.toml` workspace members 已含本 crate（scaffold 既有）
- [x] crate 根（`lib.rs`）声明已落地 `mod` + `pub use`

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

## Round 6p worklog（NameGenerator node-based `GenerateNameForNode`，red→green 推进记录）

> 本轮完成 emit-context name generator 的 **node-based 生成名字**路径（此前 `namegenerator.rs` 的 `generate_name` 在 `is_node()` 分支为 `todo!()`），解锁 transformers classfields DEFER 列表里依赖确定性生成名的项（accessor backing 私有名、私有方法/accessor brand 名、class-expr hoist temp）。这是 transformers 链上的**上游基建**，纯 additive 增长 `tsgo_printer`（`EmitContext`/`NodeFactory`/`NameGenerator`），不改既有签名。

先确认基线绿（`cargo test -p tsgo_printer` = 175 unit + 24 doctest）。逐行为红→绿（驱动选型：Go `namegenerator_test.go` 用 `parsetestutil`+`binder` 取 `.Name()`；binder-free 子集改用 arena factory **合成等价源节点**，再 `new_generated_name_for_node` 请求 + 断言 materialized 文本——与 Go `TestGeneratedNameForOther` 的合成风格一致）：

**Slice A — Identifier family（plumbing + Identifier 臂）**
1. **tracer（`generated_name_for_identifier_1`）**：合成 `Identifier("f")` → `new_generated_name_for_node(f)` → `generate_name` 命中 `todo!()` panic（红）→ 实现 `EmitContext.get_node_for_generated_name`(+worker)（解析 node + original 链）、`NameGenerator` node-id 缓存（`node_id_to_generated_name`/private 双表）、`generate_name_for_node_cached` + `generate_name_for_node` 的 Identifier/PrivateIdentifier 臂（`make_unique_name(text, optimistic, …)`）→ `"f_1"` → 绿。
2. `generated_name_for_identifier_2`：prefix `a`/suffix `b` → `new_generated_name_for_node_ex`（prefix/suffix 置 `OPTIMISTIC`）→ optimistic `make_unique_name` → `"afb"`（绿）。
3. `generated_name_for_identifier_3`：`new_generated_name_for_node(name1)`（生成名的生成名）→ worker 解析回 `name1`（无 original，是 member-name）→ Identifier 臂取 `arena.text(name1)="afb"` + 非 optimistic → `"afb_1"`（绿，验证 worker 链）。
4. `generated_name_for_node_cached`：同一源节点两次请求 → 第二次 node-id 缓存命中 → 均 `"foo_1"`（绿，验证 `generateNameForNodeCached`；divergence：Go 用 namespace 节点，binder-free 端口改用 identifier，缓存语义同）。

**Slice B — export-default / declaration / class-expression 臂**
5. **RED（`generated_name_for_export_assignment`）**：合成 `ExportAssignment` → `generate_name_for_node` default 臂 `todo!()`（红）→ 实现 ExportAssignment 臂 + `generate_name_for_export_default`（`make_unique_name("default")`）→ `"default_1"` → 绿。
6. `generated_name_for_class_expression`：合成 `ClassExpression` → ClassExpression 臂 + `generate_name_for_class_expression`（`"class"`）→ `"class_1"`（绿）。
7. `generated_name_for_function_declaration_1/2`：`function f` 有名 → 递归 name → `"f_1"`；`export default function ()` 无名 → export-default → `"default_1"`（FunctionDeclaration/ClassDeclaration 臂 + `name_of_declaration_node` 助手 + Go 短路 `g.Context==nil&&…` 注记）（绿）。
8. `generated_name_for_class_declaration_1/2`：`class C` → `"C_1"`；`export default class` → `"default_1"`（同臂泛化，绿）。

**Slice C — method/accessor、computed、default 臂**
9. **RED（`generated_name_for_method_1`）**：合成 `MethodDeclaration name=Identifier("m")` → default 臂 `todo!()`（红）→ 实现 Method/Get/SetAccessor 臂 + `generate_name_for_method_or_accessor`（name 是 identifier → `generate_name_for_node_cached`）+ `member_name_of` 助手 → `"m_1"` → 绿。
10. `generated_name_for_method_2`：method name=NumericLiteral(`0`) → 非 identifier → `make_temp_variable_name` → `"_a"`（绿）。
11. `generated_private_name_for_method`：`new_generated_private_name_for_node(method)` → private path → `"#m_1"`（绿）。
12. `generated_name_for_computed_property_name`：`ComputedPropertyName([x])` → ComputedPropertyName 臂（temp，reserved-in-nested-scopes=true）→ `"_a"`（绿）。
13. `generated_name_for_other`：`ObjectLiteralExpression{}` → default 臂（temp，reserved-in-nested-scopes=false）→ `"_a"`（绿）。

**DEFER（仍 blocked-by）**：`TestGeneratedNameForNamespace1-4`（`generateNameForModuleOrEnum`→`isUniqueLocalName` 需 binder `Locals`/`NextContainer`/`IsNodeDescendantOf`）+ `TestGeneratedNameForImport`/`Export`（`generateNameForImportOrExportDeclaration`→`GetExternalModuleName`/`makeIdentifierFromModuleName`）。共 5 条。

**测试计数（6p 新增）**：`tsgo_printer` +15 `#[test]`（node-based GenerateNameForNode：identifier 3 + cached 1 + export-assignment 1 + class-expr 1 + function-decl 2 + class-decl 2 + method 2 + private-method 1 + computed 1 + other 1）。crate 合计 **190 unit + 24 doctest**（6e-2 基线 175 + 24，无回归）。

### upstream（printer）增长（6p）

- **printer `EmitContext`**（additive）：`get_node_for_generated_name`（pub）+ `get_node_for_generated_name_worker`（私有，walk node + `original` 链 + member-name autoGenerate 判别）。未改既有签名。
- **printer `NodeFactory`**（additive）：`new_generated_name_for_node(_ex)` + `new_generated_private_name_for_node(_ex)`（`GeneratedIdentifierFlags::NODE`；prefix/suffix → `OPTIMISTIC`）。复用既有 `new_generated_identifier`/`new_generated_private_identifier`。
- **printer `NameGenerator`**（additive）：node-id 缓存双表（`node_id_to_generated_name`/`_private_name`）+ `generate_name` 的 `is_node()` 分派（替换 `todo!()`）+ `generate_name_for_node_cached`/`generate_name_for_node`（kind switch）+ `generate_name_for_export_default`/`_class_expression`/`_method_or_accessor` + 模块级 `name_of_declaration_node`/`member_name_of` 读取助手（读 `arena.data`，未碰 `internal/ast/*`）。
- **printer 既有接线复用**：`printer.rs` 的 `get_text_of_node` 早已在 emit member-name 时调用 `name_generator.generate_name(node)`——故 node-based 名字现在也能经 printer 表面 materialize（"请求名字 → emit 携带它的 identifier → 断言文本"），无需额外 printer 改动。
- **未触碰** `internal/ast/*`/`internal/binder/*`/`internal/checker/*`：node-based 路径全走 `arena.data`/`arena.kind`/`arena.text` 读取 + 既有名字算法。namespace/import-export 子集（需 binder Locals / external module）仍 DEFER。
