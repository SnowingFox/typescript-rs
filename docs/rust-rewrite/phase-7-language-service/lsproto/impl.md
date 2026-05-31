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

## 转交 / 推迟（DEFER）

- `// DEFER(phase-8): (*ClientCapabilities).Resolve() / 各 resolve() 方法` — **blocked-by**：指针版 `ClientCapabilities` 树（含 `WorkspaceClientCapabilities`/`TextDocumentClientCapabilities`/… 全部指针字段）尚未移植，当前 `ClientCapabilities` 仍是 open-object。resolved **值结构树自包含**，转换函数待指针树落地（生成器 pass）后回填。
- `// DEFER`：新枚举 `String()` stringer（同生成器 pass）。
- 维持 `ClientCapabilities`/`ServerCapabilities` open-object 现状（公共 API 仅**新增**，未改既有签名）。
