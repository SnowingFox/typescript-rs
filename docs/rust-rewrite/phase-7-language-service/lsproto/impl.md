# lsproto: 实现方案（impl.md）

> 本文件记录 `internal/lsp/lsproto` → `tsgo_lsproto` 的移植 worklog。
> 语言：规划文档用中文；`.rs` 注释一律英文（见 [PORTING.md §7](../../PORTING.md)）。
> TDD：红→绿逐行为，见 [references/tdd.md](../../references/tdd.md)。

**crate**：`tsgo_lsproto`　**目标**：LSP 协议类型模型（请求/响应/通知参数与结果、union/枚举/字符串字面量判别、`DocumentUri`、客户端/服务端能力树），用手写 `serde` 复刻 Go `go-json-experiment/json` 的 null/missing/omit/union 行为。
**依赖（crate）**：`serde` `serde_json` `tsgo_bundled` `tsgo_json` `tsgo_jsonrpc` `tsgo_tspath`（构建闭包与 checker/parser/compiler 深链**不相交**，可并行 lane）。
**Go 源**：`internal/lsp/lsproto/`（`lsp.go` `lsp_generated.go`(~3.8 万行) `baseproto.go` `jsonrpc.go` `util.go`）。

## 这个包是什么（业务说明）

`tsgo_lsproto` 是 LSP 服务器（P8 `lsp`）与语言服务层（P7 `ls`）之间共享的**协议类型层**：把 JSON-RPC 报文里的 LSP 对象/联合/枚举 1:1 映射成 Rust 类型，并提供与 Go 完全一致的 (de)serialization 行为（拒绝 null、缺字段报 `missing required properties`、忽略未知字段、omit 零值）。

之前的 lsproto 轮次已落地核心类型（`Position`/`Range`/`Location`/`Diagnostic`/`TextDocument*`/`CompletionItem`/各 union/各枚举/`InitializeParams`），但**显式 DEFER 了 `ResolvedClientCapabilities` 能力树**（`generated.rs` 顶部 module doc 与 `ClientCapabilities` 处均有说明，`ClientCapabilities`/`ServerCapabilities` 当时建成 open-object）。**本轮**补齐这棵 resolved 能力树。

## 本轮范围：`ResolvedClientCapabilities` 能力树

`ResolvedClientCapabilities` 是 `ClientCapabilities` 的**归一化视图**：所有嵌套字段都是值（非指针），便于无 nil 检查地访问深层能力。Go 经 `(*ClientCapabilities).Resolve()` 生成它；每个字段带 `json:"...,omitzero"`（零值即省略）。

本轮把整棵 resolved 值结构树（**78 个 `Resolved*` 结构** + 顶层 `ResolvedClientCapabilities`）连同其引用的**新枚举**与 **2 个 union** 一并移植为 Rust 类型，放在新建的同 crate 子模块 `resolved.rs`（`pub use resolved::*` 再导出）。`Resolve()` 转换本身 **DEFER**（见下）。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Resolved*` 值结构 + `json:",omitzero"` | `resolved_object!` 宏生成的结构 + 手写 `serde` | serde 无"非 Option 字段按零值省略"，故手写：序列化时 `field != Default::default()` 才写；反序列化缺键→零值、未知键忽略。`// Go:` 锚指 `lsp_generated.go`。 |
| `type X string`（LSP 字符串枚举） | `string_enum!` → `struct X(Cow<'static,str>)` + `const` 值 | 复刻已落地的 `LanguageKind`/`PositionEncodingKind` 风格；零值 `""`；未知值原样 round-trip。 |
| `type X uint32`（LSP 整型枚举） | `int_enum!` → `struct X(u32)` + `const` 值 | 复刻 `SymbolKind`/`DiagnosticTag` 风格；零值 `0`；序列化为 JSON 整数。 |
| `*bool` 字段（`Delta`）`json:",omitzero"` | `Option<bool>`（经 `resolved_object!`） | 缺/`None`→省略。 |
| `BooleanOrEmptyObject` / `BooleanOrClientSemanticTokensRequestFullDelta`（指针对 union） | `{ boolean: Option<bool>, <other>: Option<T> }` + 手写 serde（peek 标量/对象） | 复刻已落地的 `BooleanOrHoverOptions` union 风格；恰好一个被设置。 |
| `struct{}`（`EmptyObject`） | `struct EmptyObject;`，序列化为 `{}`、反序列化忽略成员 | union 的对象变体。 |

**模块放置（偏离）**：新枚举（`ResourceOperationKind`/`FailureHandlingKind`/`MarkupKind`/`CodeActionKind`/`TokenFormat`/`SymbolTag`/`CompletionItemTag`/`InsertTextMode`/`CodeActionTag`/`PrepareSupportDefaultBehavior`）与 2 个 union 暂随其唯一消费者放在 `resolved.rs`，而非 `generated.rs`。理由：本轮编辑边界局限于 lsproto、且这些类型当前仅被 resolved 树引用；后续生成器 pass 落地完整枚举集时再归位。已通过 `pub use resolved::*` 在 crate 根可见。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/lsp/lsproto/lsp_generated.go`（`Resolved*` 段，~2300 行） | `internal/lsp/lsproto/resolved.rs`（新增） | 整棵 resolved 能力树 + 新枚举 + 2 union + `resolved_object!`/`int_enum!`/`string_enum!` 宏 |
| 同上（测试自写，Go 无 `*_test.go` 覆盖 resolved） | `internal/lsp/lsproto/resolved_test.rs`（新增） | 行为级 serde 测试 |
| `internal/lsp/lsproto/lib.rs` | `lib.rs` | 追加 `mod resolved; pub use resolved::*;` |

## 依赖白名单（本包新增的 crate）

无新增第三方 crate（仅用现有 `serde`/`serde_json`）。

## 实现 TODO（逐组，可勾选）

> 顺序按 TDD 推进序（leaf → group → 顶层）。`// Go:` 锚均指 `lsp_generated.go`。

- [x] `resolved_object!` 宏（omitzero 序列化 + 宽松反序列化）　`// Go: lsp_generated.go:Resolved* (json ",omitzero")`
- [x] `int_enum!` / `string_enum!` 宏 + 10 个新枚举　`// Go: lsp_generated.go:ResourceOperationKind/FailureHandlingKind/MarkupKind/CodeActionKind/TokenFormat/SymbolTag/CompletionItemTag/InsertTextMode/CodeActionTag/PrepareSupportDefaultBehavior`
- [x] `EmptyObject` + `ClientSemanticTokensRequestFullDelta` + 2 个 `Boolean*` union　`// Go: lsp_generated.go:BooleanOrEmptyObject/BooleanOrClientSemanticTokensRequestFullDelta`
- [x] Workspace 叶子组（`ResolvedChangeAnnotationsSupportOptions`/`ResolvedWorkspaceEditClientCapabilities`/`ResolvedDidChange*`/`ResolvedClientSymbol*`/`ResolvedWorkspaceSymbolClientCapabilities`/`ResolvedExecuteCommandClientCapabilities`/`Resolved*WorkspaceClientCapabilities`/`ResolvedFileOperationClientCapabilities`/`ResolvedTextDocumentContentClientCapabilities`）　`// Go: lsp_generated.go:ResolvedWorkspaceClientCapabilities`
- [x] `ResolvedWorkspaceClientCapabilities`（组装）　`// Go: lsp_generated.go:ResolvedWorkspaceClientCapabilities`
- [x] TextDocument 叶子组（completion/hover/signatureHelp/definition 家族/documentSymbol/codeAction/codeLens/documentLink/color/formatting 家族/rename/foldingRange/selectionRange/publishDiagnostics/callHierarchy/semanticTokens/linkedEditing/moniker/typeHierarchy/inlineValue/inlayHint/diagnostic/inlineCompletion）　`// Go: lsp_generated.go:ResolvedTextDocumentClientCapabilities`
- [x] `ResolvedTextDocumentClientCapabilities`（组装，含 semanticTokens.requests union 字段）　`// Go: lsp_generated.go:ResolvedTextDocumentClientCapabilities`
- [x] Window 组（`ResolvedClientShowMessageActionItemOptions`/`ResolvedShowMessageRequestClientCapabilities`/`ResolvedShowDocumentClientCapabilities`/`ResolvedWindowClientCapabilities`）　`// Go: lsp_generated.go:ResolvedWindowClientCapabilities`
- [x] General 组（`ResolvedStaleRequestSupportOptions`/`ResolvedRegularExpressionsClientCapabilities`/`ResolvedMarkdownClientCapabilities`/`ResolvedGeneralClientCapabilities`）　`// Go: lsp_generated.go:ResolvedGeneralClientCapabilities`
- [x] 顶层 `ResolvedClientCapabilities`（4 组 + 5 个 `_vs_*` 标量）　`// Go: lsp_generated.go:ResolvedClientCapabilities`
- [x] `lib.rs` 接线 `mod resolved; pub use resolved::*;`

## TDD 推进顺序（tracer bullet → 增量）

1. **tracer（真红→绿）**：`ResolvedDidChangeConfigurationClientCapabilities`（单 bool）。先用普通 `#[derive(Serialize)]` → `default` 序列化出 `{"dynamicRegistration":false}`（**RED**，断言 `== "{}"` 失败）；改用 `resolved_object!`（omitzero）→ **GREEN**。
2. 嵌套结构 omit / `Vec<enum>` / `u32`/`String` 标量 omit / 新字符串枚举（直接字段 + Vec）/ 新整型枚举（直接字段）。
3. 2 个 union + `ResolvedClientSemanticTokensRequestOptions`。
4. 组装 4 个顶层组 + 顶层 `ResolvedClientCapabilities`（only-window omit 递归、`_vs_*` 标量键、深层 round-trip、客户端 JSON 反序列化）。
5. 每个 resolved 类型 `default → {}` 覆盖（`assert_default_empty::<T>()`）。

> 诚实记录：tracer 之后各行为的 serde 由宏统一生成，多为 **green-on-arrival**（宏一旦成立即过）；这些已在 tests.md 标注。

## 与 Go 的已知偏离（divergence）

1. resolved 类型的 (de)serialization 用手写 `serde`（`resolved_object!`），而非 Go 的 jsonv2 反射 + `omitzero`。行为对齐：零值省略、缺键→零值、未知键忽略。
2. resolved 类型在 Go 里几乎只被 `Resolve()` 生产、极少反序列化；本轮 `delta:null` 等显式 null 不复刻 Go 自定义 decoder 的 `errNull`（serde 对 `Option`/标量按其默认 null 行为）。这是低优先、已知的精度偏差（resolved 非线上收报类型）。
3. 新枚举的 Go `String()` stringer **未移植**（resolved 树不使用；生成器 pass 落地完整枚举时补）。

## 续轮：指针版 `ClientCapabilities` 请求树 + `Resolve()`（已落地）

> 上一轮 DEFER 的 `(*ClientCapabilities).Resolve()` 在本轮补齐。新增文件 `capabilities.rs`（+ `capabilities_test.rs`），`lib.rs` 追加 `mod capabilities; pub use capabilities::*;`。

**做了什么**：把 Go `lsp_generated.go` 里指针版 `ClientCapabilities` 请求树**整棵**移植为 Rust（4 组全部：Workspace / TextDocument / Window / General + 顶层 `_vs_*` 标量），并接上 `ClientCapabilities::resolve()` → `ResolvedClientCapabilities`（Go `Resolve()`）。每个嵌套请求子结构都带 `resolve()`（Go 私有 `resolve()`），逐层映射进对应 `Resolved*` 值结构。

**open-object 处置（retired）**：原 `ClientCapabilities` open-object（unit struct，吃任意对象、重序列化成 `{}`）**退役**，替换为指针版 typed 树（与 Go 一致）。`InitializeParams.capabilities` 现引用 typed `ClientCapabilities`（`generated.rs` 用 `use crate::ClientCapabilities`）。公共 API 影响：crate 外无任何 Rust 使用者（P8 `lsp`/`project`/`api` 未移植）；crate 内仅 1 处测试字面量 `capabilities: ClientCapabilities` → `ClientCapabilities::default()`。其余既有公共类型签名未改，`empty_client_capabilities`（`from_str("{}")`）等既有测试不变即过。`ServerCapabilities` 仍维持 open-object（Go 的 `ServerCapabilities` 不在本轮 `Resolve()` 闭包内）。

**类型/所有权映射（本子树关键决策）**：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `*T` 可选指针字段（`json:",omitzero"` + errNull 守卫） | `Option<T>`（经 `request_object!`） | 序列化 `Some` 才写、缺键→`None`、未知键忽略；`null` 经内层类型解码自然报错（不复刻 Go `errNull` 文案）。 |
| Go 个别**非指针**必填/切片字段（如 `ShowDocument.Support bool`、`SemanticTokens.TokenTypes []string`） | 同样建成 `Option<T>` | 统一化偏离；对 `resolve()`（缺→零值）完全等价，仅反序列化严格度/显式空数组 round-trip 略异（这些请求类型不依赖）。 |
| 标量/`Vec`/枚举/union 字段的 `resolve()`（Go `derefOr(v.X)` 或 `v.X`） | `self.x.clone().unwrap_or_default()`（`request_object!` 的 `val` 类） | 缺→`Default`，对齐 Go 零值。 |
| 嵌套子结构 `resolve()`（Go `v.X.resolve()`，nil→零值结构） | `self.x.as_ref().map(\|x\| x.resolve()).unwrap_or_default()`（`sub` 类） | 与 Go nil-receiver→默认结构一致。 |
| 顶层 `func (v *ClientCapabilities) Resolve()` | `ClientCapabilities::resolve()`（手写，`pub`） | 4 组 + 5 个 `_vs_*`。Go 私有 `resolve()` → Rust `pub fn resolve`（仅**新增**可见性，无害）。 |

**新增宏**：`request_object!`（`capabilities.rs`）——生成请求结构（`Option<T>` 字段）+ 手写 `serde`（skip-none / 忽略未知键）+ `resolve()`（`val`/`sub` 两类字段）。镜像 `resolved.rs` 的 `resolved_object!` 风格。

**文件清单（本轮新增）**：

| Go 源 | Rust 文件 | 说明 |
|---|---|---|
| `lsp_generated.go`（指针版 `ClientCapabilities` 族 + `resolve()`/`Resolve()`） | `internal/lsp/lsproto/capabilities.rs`（新增） | `request_object!` 宏 + 77 个请求子结构（各带 `resolve()`）+ 顶层手写 `ClientCapabilities`/`resolve()`（共 78 个请求类型，1:1 对应 78 个 `Resolved*` 值结构） |
| 同上（Go 无 resolve 对应 `*_test.go`） | `internal/lsp/lsproto/capabilities_test.rs`（新增） | 行为级 resolve/serde 测试 |
| `lib.rs` | `lib.rs` | 追加 `mod capabilities; pub use capabilities::*;` |
| `generated.rs` | `generated.rs` | 退役 `ClientCapabilities` open-object；`use crate::ClientCapabilities` |

**实现 TODO（本轮，可勾选）**：

- [x] `request_object!` 宏（`Option<T>` 字段 serde + `resolve()` 的 `val`/`sub` 映射）　`// Go: lsp_generated.go:(*T).resolve`
- [x] 顶层 typed `ClientCapabilities` + `resolve()`（4 组 + 5 个 `_vs_*` 标量；退役 open-object）　`// Go: lsp_generated.go:(*ClientCapabilities).Resolve`
- [x] Window 组（`WindowClientCapabilities`/`ShowMessageRequestClientCapabilities`/`ClientShowMessageActionItemOptions`/`ShowDocumentClientCapabilities`）
- [x] General 组（`GeneralClientCapabilities`/`StaleRequestSupportOptions`/`RegularExpressionsClientCapabilities`/`MarkdownClientCapabilities`）
- [x] Workspace 组（`WorkspaceClientCapabilities` + 17 叶子/嵌套结构）
- [x] TextDocument 组（`TextDocumentClientCapabilities` + 31 子能力，含 completion/signatureHelp/codeAction/foldingRange/semanticTokens(union 字段) 家族）

## 转交 / 推迟（DEFER）

- ✅ 上一轮的 `(*ClientCapabilities).Resolve()` DEFER **已解除**（本轮落地于 `capabilities.rs`，4 组全树）。
- `// DEFER`：新枚举 `String()` stringer（同生成器 pass）。
- `// DEFER`：请求树字段的 Go `errNull` 精确文案（与 resolved 树一致的低优先偏差；`null` 仍被拒绝，仅文案不同）。
- `// DEFER`：Go 个别非指针字段（`support`/`tokenTypes` 等）建成 `Option<T>` 的统一化偏离——若后续生成器 pass 要求精确反序列化严格度，再区分 `Option<T>` vs 直接 `T`/`Vec<T>`。
- 维持 `ServerCapabilities`/`InitializationOptions` open-object 现状（不在本轮 `Resolve()` 闭包内；公共 API 仅**新增**）。
