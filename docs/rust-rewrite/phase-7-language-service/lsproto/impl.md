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

## 续轮：服务端 `ServerCapabilities` typed 树（本轮落地）

> 上一轮维持的 `ServerCapabilities` open-object **本轮退役**，替换为 typed 结构树（与 Go 一致；`ServerCapabilities` 是服务端**产出**值，建成 skip-none 序列化的 typed struct）。全部编辑在 `generated.rs`（+ `generated_test.rs`），无新模块/无新第三方 crate。

**open-object 处置（retired）**：原 `ServerCapabilities` open-object（`lsp_open_object!` 生成的 unit struct，吃任意对象、重序列化成 `{}`）**退役**，替换为 `lsp_object!` 生成的 typed struct（38 个字段，1:1 对齐 Go 声明序）。公共 API 影响：crate 外无任何 Rust 使用者（P8 `lsp`/`project`/`api` 未移植，已 grep 确认）；crate 内仅 `InitializeResult.capabilities`（`reqnn`，仍拒 null）与既有 `empty_server_capabilities`（`from_str("{}")`）测试引用——typed struct 全字段可选，`{}` 仍解码为默认、`InitializeResult capabilities null` 仍报错，既有测试不变即过。其余既有公共类型签名未改，**纯新增**。`InitializationOptions` 仍维持 open-object（用户自定义初始化项，非本轮范围）。

**为什么用 `lsp_object!`（`opt` 字段）而非 `request_object!`**：Go `ServerCapabilities` 每个字段都是 `*T json:",omitzero"` + `UnmarshalJSONFrom` 里逐字段 `if PeekKind()=='n' { errNull }`。这正是 `lsp_object!` 的 `opt` 语义（`Option<T>`、`Some` 才序列化、缺键→`None`、显式 `null` 拒绝、未知键忽略）。`request_object!` 多带一个 `resolve()`（ServerCapabilities 不需要），故复用 `generated.rs` 既有的 `lsp_object!` + 既有 `BooleanOrHoverOptions` 风格最贴合。

**类型/所有权映射（本子树关键决策）**：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `ServerCapabilities` 各 `*T json:",omitzero"` 字段 | `lsp_object!` 的 `opt T`（`Option<T>`） | skip-none 序列化、缺→`None`、`null` 拒绝、未知键忽略；逐字段 `// Go:` 不另标（整 struct 一个锚）。 |
| `BooleanOr<XxxOptions>`（bool 对 options 指针 union） | `boolean_or_options!` 宏生成 `{ boolean: Option<bool>, <field>: Option<T> }` + 手写 serde（`visit_bool`/`visit_map` peek dispatch） | 复刻既有 `BooleanOrHoverOptions` 风格；恰一个被设置。 |
| `TextDocumentSyncOptionsOrKind`（options 对 number kind） | 手写 union（`visit_map`→options、`visit_u64/i64`→kind） | 标量/对象 peek dispatch。 |
| `SemanticTokensOptionsOrRegistrationOptions`（按 `documentSelector` 键判别） | union `{ options: Option<SemanticTokensOptions>, registration_options: Option<serde_json::Value> }` | 保留 Go union 形状；**仅 registration 变体 DEFER 为 raw JSON**（`SemanticTokensRegistrationOptions` 深类型）。 |
| `type TextDocumentSyncKind uint32`（iota 0/1/2 + `String()`） | 手写 `struct TextDocumentSyncKind(pub u32)` + `const NONE/FULL/INCREMENTAL` + `Display`（复刻 Go stringer） | 复刻 `generated.rs` 既有 int-enum 风格（`SymbolKind`/`DiagnosticSeverity`）。 |
| 必填 `legend *SemanticTokensLegend json:"legend"`（无 omitzero，decode 必填+拒 null） | `lsp_object!` 的 `reqnn`（始终序列化、缺→`missing required properties`、拒 null） | 同 `tokenTypes`/`tokenModifiers`（必填 `[]string`）。 |
| 深/稀有 provider（declaration/typeDefinition/implementation/documentHighlight/codeLens/documentLink/color/documentRangeFormatting/documentOnTypeFormatting/foldingRange/selectionRange/executeCommand/callHierarchy/linkedEditing/moniker/typeHierarchy/inlineValue/inlayHint/diagnostic/inlineCompletion/workspace/_vs_onAutoInsert） | `opt serde_json::Value`（raw JSON，带 `// DEFER … blocked-by:` 注释） | **保留 Go 字段名 + optionality**，仅延后精确嵌套类型；缺→`None`、`null` 拒绝、round-trip 原值。 |
| `*bool` 标量 provider（`customSourceDefinitionProvider`/`_vs_referencesProvider`/`customMultiDocumentHighlightProvider`） | `opt bool`（`Option<bool>`） | omitzero。 |
| `positionEncoding *PositionEncodingKind` | `opt PositionEncodingKind`（复用既有字符串枚举） | 直接 typed。 |

**新增宏**：`boolean_or_options!`（`generated.rs`）——生成 `boolean | <Options>` union（serde：`visit_bool`/`visit_map`），DRY 掉 7 个同形 union（Definition/Reference/DocumentSymbol/CodeAction/DocumentFormatting/Rename/WorkspaceSymbol）。既有手写 `BooleanOrHoverOptions`/新手写 `BooleanOrSaveOptions`/`BooleanOrSemanticTokensFullDelta`（变体字段名不同）保持手写。

**落地的 provider option 组（全 typed + 各带行为测试）**：
- `textDocumentSync`：`TextDocumentSyncOptions`/`TextDocumentSyncKind`/`SaveOptions`/`BooleanOrSaveOptions`/`TextDocumentSyncOptionsOrKind`
- `completionProvider`：`CompletionOptions` + `ServerCompletionItemOptions`
- `hoverProvider`：复用既有 `HoverOptions`/`BooleanOrHoverOptions`
- `signatureHelpProvider`：`SignatureHelpOptions`
- `definitionProvider`/`referencesProvider`：`DefinitionOptions`/`ReferenceOptions` + `BooleanOr*`
- `documentSymbolProvider`/`codeActionProvider`：`DocumentSymbolOptions`/`CodeActionOptions`（含 `Vec<CodeActionKind>`；`documentation` DEFER raw JSON）+ `BooleanOr*`
- `documentFormattingProvider`/`renameProvider`/`workspaceSymbolProvider`：对应 options + `BooleanOr*`
- `semanticTokensProvider`：`SemanticTokensOptions`/`SemanticTokensLegend`/`SemanticTokensFullDelta`/`BooleanOrSemanticTokensFullDelta`/`SemanticTokensOptionsOrRegistrationOptions`（复用既有 `BooleanOrEmptyObject`）
- `positionEncoding`：复用既有 `PositionEncodingKind`

**实现 TODO（本轮，可勾选）**：

- [x] 退役 open-object `ServerCapabilities` → typed `lsp_object!` struct（38 字段，Go 声明序）　`// Go: lsp_generated.go:ServerCapabilities`
- [x] `boolean_or_options!` 宏（`boolean | <Options>` union serde）　`// Go: lsp_generated.go:BooleanOr<...>Options.UnmarshalJSONFrom`
- [x] textDocumentSync 组（含 `TextDocumentSyncKind` int-enum + `OrKind`/`BooleanOrSaveOptions` union）
- [x] completion 组（`CompletionOptions`/`ServerCompletionItemOptions`）
- [x] signatureHelp/definition/references 组
- [x] documentSymbol/codeAction 组（`CodeActionKind` 复用 resolved.rs）
- [x] documentFormatting/rename/workspaceSymbol 组
- [x] semanticTokens 组（options/legend/fullDelta + `OrRegistrationOptions` union，registration 变体 DEFER）
- [x] 其余 22 个深/稀有 provider 字段建成 `opt serde_json::Value`（DEFER）+ 3 个 `*bool` provider + `positionEncoding`

## 续轮：服务端 provider 注册选项树（registration-options）（本轮落地）

> 上一轮把 `ServerCapabilities` 落成 typed struct，但有 **22 个深/稀有 provider 字段**仍是 `serde_json::Value` raw-JSON DEFER 占位。**本轮**把其中 **21 个**替换为真实 typed option/registration 树（保留 Go 字段名 + optionality），仅 `workspace`（`WorkspaceOptions`）因深依赖未移植而保留 DEFER。全部编辑在 `generated.rs`(+`generated_test.rs`)，无新模块、无新第三方 crate。

**新增宏 `boolean_or_options_or_registration!`（`generated.rs`）**：生成 Go 的 triple-union `boolean | <Options> | <RegistrationOptions>`（结构 `{ boolean: Option<bool>, <field>: Option<T>, registration_options: Option<serde_json::Value> }`）。serde 复刻 Go `PeekKind`/`jsonObjectHasKey` 派发：JSON `bool` → `boolean`；带 `documentSelector` 键的对象 → registration 变体（**保持 raw JSON**，因 `*RegistrationOptions` 嵌入尚未移植的 `DocumentSelectorOrNull`）；其余对象 → typed options。DRY 掉 12 个同形 triple-union。`registration_options` raw-JSON 与既有 `SemanticTokensOptionsOrRegistrationOptions` 先例一致。

**类型/所有权映射（本子树关键决策）**：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `*XxxOptions json:",omitzero"` 简单 provider（只有 `workDoneProgress *bool`） | `lsp_object!` 的全 `opt` struct | omitzero；`default → {}`（§8.6 覆盖）。 |
| `Commands []string`（`ExecuteCommandOptions`，非指针必填、有 null 守卫） | `reqnn Vec<String>` | 缺→`missing required properties: commands`、拒 null、始终序列化（对齐 `SemanticTokensLegend.tokenTypes` 先例）。 |
| `FirstTriggerCharacter string`（`DocumentOnTypeFormatting`，非指针必填值类型、无 null 守卫） | `req String` | 缺→`errMissing`、始终序列化、不拒 null（值类型）。 |
| `InterFileDependencies/WorkspaceDiagnostics bool`（`DiagnosticOptions`，非指针必填、无 omitzero） | `req bool` | 始终序列化（即使 `false`），缺→`errMissing`。 |
| `Boolean\|Options\|RegistrationOptions` triple-union | `boolean_or_options_or_registration!` 宏 | 见上；registration 变体 raw JSON DEFER。 |
| `Boolean\|Options`（无 registration：documentHighlight/documentRangeFormatting/inlineCompletion） | 复用既有 `boolean_or_options!` 宏 | 与既有 definition/reference 等同形。 |
| `Options\|RegistrationOptions`（无 boolean：`DiagnosticOptionsOrRegistrationOptions`） | 手写 union（复刻 `SemanticTokensOptionsOrRegistrationOptions`） | 按 `documentSelector` 键派发；registration 变体 raw JSON DEFER。 |

**落地的 provider option（21/22，全 typed + 各带行为测试）**：
- 直接 typed option：`executeCommandProvider`(`ExecuteCommandOptions`，**tracer**)、`documentOnTypeFormattingProvider`(`DocumentOnTypeFormattingOptions`)、`codeLensProvider`(`CodeLensOptions`)、`documentLinkProvider`(`DocumentLinkOptions`)、`_vs_onAutoInsertProvider`(`VsOnAutoInsertOptions`)
- `boolean_or_options!` 复用：`documentHighlightProvider`(`BooleanOrDocumentHighlightOptions`)、`documentRangeFormattingProvider`(`BooleanOrDocumentRangeFormattingOptions`)、`inlineCompletionProvider`(`BooleanOrInlineCompletionOptions`)
- options-or-registration：`diagnosticProvider`(`DiagnosticOptionsOrRegistrationOptions` + `DiagnosticOptions`)
- triple-union（新宏，12 个）：`declarationProvider`(**首个 RED→GREEN**)、`typeDefinitionProvider`、`implementationProvider`、`colorProvider`、`foldingRangeProvider`、`selectionRangeProvider`、`callHierarchyProvider`、`linkedEditingRangeProvider`、`monikerProvider`、`typeHierarchyProvider`、`inlineValueProvider`、`inlayHintProvider`(`InlayHintOptions` 含额外 `resolveProvider`)

**实现 TODO（本轮，可勾选）**：

- [x] 新宏 `boolean_or_options_or_registration!`（triple-union serde：bool/typed-options/raw-registration 派发）　`// Go: lsp_generated.go:BooleanOr*Or*RegistrationOptions.UnmarshalJSONFrom`
- [x] tracer：`ExecuteCommandOptions`（`reqnn commands`）+ 替换 `executeCommandProvider` 字段（退役 raw JSON；改既有测试字面量）　`// Go: lsp_generated.go:ExecuteCommandOptions`
- [x] `DocumentOnTypeFormattingOptions`（`req firstTriggerCharacter` + `opt moreTriggerCharacter`）
- [x] 直接 typed：`CodeLensOptions`/`DocumentLinkOptions`/`VsOnAutoInsertOptions`
- [x] `boolean_or_options!` 复用：documentHighlight/documentRangeFormatting/inlineCompletion 三组（含 `DocumentRangeFormattingOptions.rangesSupport`）
- [x] `DiagnosticOptions` + `DiagnosticOptionsOrRegistrationOptions`（按 `documentSelector` 派发；`interFileDependencies`/`workspaceDiagnostics` 为 `req bool`）
- [x] 12 个 triple-union（declaration 首个真 RED→GREEN，其余宏生成 green-on-arrival；各自 `*Options` 全 typed）
- [x] `workspace`(`WorkspaceOptions`) 维持 raw JSON DEFER（深依赖未移植，见下）

## 续轮：registration-options base tree（本轮落地）

> 上一轮把 12 个 triple-union + `DiagnosticOptionsOrRegistrationOptions` + `SemanticTokensOptionsOrRegistrationOptions` 的 **registration 变体**留作 `registration_options: serde_json::Value` raw-JSON 占位（其嵌入 `DocumentSelectorOrNull` 未移植）。**本轮**移植 registration-options 的**基底类型树**，并把全部 **14 个 raw-JSON registration 槽**升级为真实 typed `*RegistrationOptions`。全部编辑在 `generated.rs`(+`generated_test.rs`)，无新模块、无新第三方 crate。

**落地的基底类型（slice 1–3）**：
- `StaticRegistrationOptions { id?: String }`（`lsp_object!` 的 `opt id`；`default → {}`）。
- `PatternOrRelativePattern`（`Pattern | RelativePattern` union）：string 变体 typed（`pattern: Option<String>`），`RelativePattern` 对象变体 **DEFER raw JSON**（其 `baseUri: WorkspaceFolderOrURI/WorkspaceFolder` 树未移植）。
- `DocumentFilter`（= Go `TextDocumentFilterLanguageOrSchemeOrPattern`）：三变体 union（`TextDocumentFilterLanguage`/`Scheme`/`Pattern`，各 `language`/`scheme`/`pattern` 字段，必填判别字段不同），手写 serde 复刻 Go「按声明序逐个 try-decode，第一个成功者胜」。
- `DocumentSelectorOrNull`（`[]DocumentFilter | null`）：`document_selector: Option<Vec<DocumentFilter>>`，手写 serde（`null`→`None`、数组→`Some`、其余 err）；`default → null`（对齐 Go nil pointer 序列化为 `null`）。
- `TextDocumentRegistrationOptions { documentSelector: DocumentSelectorOrNull }`（`req documentSelector`，缺→`missing required properties`、始终序列化、`null` 由 union 自身接收）。

**升级的 14 个 registration 槽（slice 4–5）**：
- **新宏签名**：`boolean_or_options_or_registration!` 增加 `registration: $reg:ty` 参数，`registration_options` 字段由 `Option<serde_json::Value>` 改为 `Option<$reg>`；反序列化的 registration 臂由「`Some(value)` 原值」改为 `serde_json::from_value::<$reg>(value)`，序列化臂不变（typed `r.serialize()`）。12 个 triple-union 调用点各传入其 `*RegistrationOptions`。
- **2 个手写 union**（`DiagnosticOptionsOrRegistrationOptions` / `SemanticTokensOptionsOrRegistrationOptions`）：`registration_options` 字段同样升级为 typed（`DiagnosticRegistrationOptions` / `SemanticTokensRegistrationOptions`），派发逻辑（按 `documentSelector` 键）不变。
- 共 14 个 `*RegistrationOptions` 用 `lsp_object!` 落地（Go 把 embedding 展平，故 Rust 也是扁平 struct）。**字段声明序逐 struct 1:1 对齐 Go**（关键：Go 各 struct 的字段序不同——`Declaration`/`SelectionRange`/`InlineValue`/`InlayHint` 把 `workDoneProgress` 放在 `documentSelector` 前，其余把 `documentSelector` 放最前；`Moniker` **无 `id` 字段**；`Diagnostic` 含 `req` 非指针 bool `interFileDependencies`/`workspaceDiagnostics`；`SemanticTokens` 含 `reqnn legend`），保证序列化 byte-for-byte 与 Go 一致。

**类型/所有权映射（本子树关键决策）**：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `*RegistrationOptions`（展平的 embedding：`<Feature>Options` + `TextDocumentRegistrationOptions` + `StaticRegistrationOptions`） | `lsp_object!` 扁平 struct，字段序 1:1 对齐 Go 声明 | 与 Go 生成器一致（Go 也把 embedding 展平进每个 struct）；非 `req`/`reqnn`/`opt` 行为同既有约定。 |
| `documentSelector DocumentSelectorOrNull json:"documentSelector"`（必填、无 omitzero、无 null 守卫） | `req document_selector: DocumentSelectorOrNull` | 缺→`missing required properties: documentSelector`、始终序列化、`null` 交由 union 自身接收（不拒）；`default → {"documentSelector":null}`（故**不**纳入 `default → {}` 覆盖集）。 |
| `DocumentSelector = []TextDocumentFilterLanguageOrSchemeOrPattern` | `Vec<TextDocumentFilterLanguageOrSchemeOrPattern>` | — |
| `DocumentSelectorOrNull{ *[]... }`（nil→null） | `{ document_selector: Option<Vec<...>> }` + 手写 serde | `None`↔`null`；`default → null`。 |
| `TextDocumentFilterLanguageOrSchemeOrPattern`（按 try-decode 顺序判别的 union） | 三 `Option<变体>` + 手写 serde（依序 `from_value` try） | 复刻 Go「Language→Scheme→Pattern 依序尝试、首个成功者胜」。 |
| `PatternOrRelativePattern`（`string | RelativePattern`） | `{ pattern: Option<String>, relative_pattern: Option<serde_json::Value> }` | string typed；`RelativePattern` 对象 **DEFER raw JSON**（深依赖 `WorkspaceFolderOrURI`）。与既有 raw-JSON DEFER 变体先例一致。 |
| `Boolean|Options|RegistrationOptions` triple-union | `boolean_or_options_or_registration!`（新增 `registration: $reg` 参数） | registration 臂现为 typed。 |

**红→绿推进序（slice）**：
1. `StaticRegistrationOptions`（tracer，真 RED→GREEN：类型不存在→`lsp_object!`）。
2. `PatternOrRelativePattern` → 3 个 filter 变体 struct → `TextDocumentFilterLanguageOrSchemeOrPattern` union → `DocumentSelectorOrNull`（各真 RED→GREEN）。
3. `TextDocumentRegistrationOptions`（真 RED→GREEN）。
4. 升级 triple-union 宏 + `DeclarationRegistrationOptions`（**tracer，真 RED→GREEN**：测试访问 `registration_options.unwrap().id`/`.document_selector` 在 raw-JSON 态编译失败 → 加 14 个 struct + 改宏 + 改 12 调用点 → GREEN）。同宏的其余 11 个 triple-union registration 为 **green-on-arrival**（诚实标注）。
5. 升级 2 个手写 union（`Diagnostic` / `SemanticTokens` registration 变体，各真 RED→GREEN）+ 补 11 个 green-on-arrival 的覆盖测试（综合 round-trip + 每类型 default + inlayHint/moniker 边角）。

**实现 TODO（本轮，可勾选）**：

- [x] `StaticRegistrationOptions`（`opt id`）　`// Go: lsp_generated.go:StaticRegistrationOptions`
- [x] `PatternOrRelativePattern`（string typed + RelativePattern raw-JSON DEFER）　`// Go: lsp_generated.go:PatternOrRelativePattern`
- [x] 3 个 filter 变体 + `TextDocumentFilterLanguageOrSchemeOrPattern` union（DocumentFilter）　`// Go: lsp_generated.go:TextDocumentFilterLanguage/Scheme/Pattern`
- [x] `DocumentSelectorOrNull`（`[]DocumentFilter | null`，default→null）　`// Go: lsp_generated.go:DocumentSelectorOrNull`
- [x] `TextDocumentRegistrationOptions`（`req documentSelector`）　`// Go: lsp_generated.go:TextDocumentRegistrationOptions`
- [x] 升级 `boolean_or_options_or_registration!` 宏（`registration: $reg` typed）+ 12 个 triple-union `*RegistrationOptions`（declaration tracer）　`// Go: lsp_generated.go:*RegistrationOptions`
- [x] 升级 `DiagnosticOptionsOrRegistrationOptions` → typed `DiagnosticRegistrationOptions`（含 `req` 非指针 bool）
- [x] 升级 `SemanticTokensOptionsOrRegistrationOptions` → typed `SemanticTokensRegistrationOptions`（含 `reqnn legend`）

## 续轮：`WorkspaceOptions` 子树 + `RelativePattern` 对象变体（本轮落地）

> 上一轮把 registration-options base tree 落地后，lsproto 仅剩 **2 个 raw-JSON DEFER 槽**：`ServerCapabilities.workspace`（`WorkspaceOptions`）与 `PatternOrRelativePattern` 的 `RelativePattern` 对象变体。**本轮**把这两处全部升级为真实 typed 树，**清空 `ServerCapabilities` 最后一个 raw-JSON DEFER 槽**。全部编辑在 `generated.rs`(+`generated_test.rs`)，无新模块、无新第三方 crate。

**落地的类型（与 Go 字段名 / optionality / embedding 1:1）**：
- `StringOrBoolean`（`string | boolean` union）：复刻 Go `PeekKind` 派发（`"`→string、`t`/`f`→boolean，其余 err）。手写 serde，恰一变体置位。
- `WorkspaceFoldersServerCapabilities { supported?: bool, changeNotifications?: StringOrBoolean }`（`lsp_object!`，全 `opt`）。
- `FileOperationPatternKind`（string enum：`file`/`folder`，手写 `Cow<'static,str>` newtype，风格同 `PositionEncodingKind`）。
- `FileOperationPatternOptions { ignoreCase?: bool }`、`FileOperationPattern { glob: req String, matches?: FileOperationPatternKind, options?: FileOperationPatternOptions }`、`FileOperationFilter { scheme?: String, pattern: reqnn FileOperationPattern }`、`FileOperationRegistrationOptions { filters: reqnn Vec<FileOperationFilter> }`、`FileOperationOptions { didCreate?/willCreate?/didRename?/willRename?/didDelete?/willDelete?: FileOperationRegistrationOptions }`（自底向上）。
- `TextDocumentContentOptions { schemes: reqnn Vec<String> }`、`TextDocumentContentRegistrationOptions { schemes: reqnn, id?: String }`、`TextDocumentContentOptionsOrRegistrationOptions`（try-decode union：先 options 后 registration，复刻 Go fall-through——options 吃任意带 `schemes` 的对象、忽略多余 `id`，故 options 总胜，与 Go 一致）。
- `WorkspaceOptions { workspaceFolders?, fileOperations?, textDocumentContent? }`（`lsp_object!`，全 `opt`），并把 `ServerCapabilities.workspace` 从 `serde_json::Value` 换成 `Option<WorkspaceOptions>`。
- `WorkspaceFolder { uri: req URI, name: req String }`（`URI` 复用 `lsp.rs` 既有 newtype）、`WorkspaceFolderOrURI`（`WorkspaceFolder | URI` union：`{`→folder、`"`→URI string）、`RelativePattern { baseUri: req WorkspaceFolderOrURI, pattern: req String }`；把 `PatternOrRelativePattern.relative_pattern` 从 `Option<serde_json::Value>` 换成 `Option<RelativePattern>`（serialize 臂不变 `r.serialize()`；deserialize 对象臂改为 `serde_json::from_value::<RelativePattern>`）。

**类型/所有权映射（本子树关键决策）**：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `StringOrBoolean{ String *string; Boolean *bool }` | `{ string: Option<String>, boolean: Option<bool> }` + 手写 serde | `"`→string、`t`/`f`→boolean、其余 err；恰一变体置位。 |
| `*T json:",omitzero"`（workspace 子树各可选字段） | `lsp_object!` 的 `opt T` | skip-none、缺→`None`、`null` 拒绝、未知键忽略。 |
| `Glob string json:"glob"`（值类型必填、无 null 守卫） | `req String` | 缺→`errMissing`、始终序列化、不拒 null（值类型）。 |
| `Pattern *FileOperationPattern json:"pattern"`（指针必填、有 `errNull` 守卫） | `reqnn FileOperationPattern` | 缺→`errMissing`、拒 null、始终序列化。 |
| `Filters []*FileOperationFilter json:"filters"`（必填、有 null 守卫） | `reqnn Vec<FileOperationFilter>` | 同上。 |
| `Schemes []string json:"schemes"`（必填、有 null 守卫） | `reqnn Vec<String>` | 同上。 |
| `type FileOperationPatternKind string`（`file`/`folder`） | 手写 `Cow<'static,str>` newtype + `const FILE/FOLDER` | 复刻 `PositionEncodingKind`；未知值原样 round-trip。 |
| `TextDocumentContentOptionsOrRegistrationOptions`（try-decode union） | 2 `Option<变体>` + 手写 serde（依序 `from_value`） | 复刻 Go「options→registration 依序、首个成功者胜」；options 总胜（与 Go 同）。 |
| `BaseUri WorkspaceFolderOrURI json:"baseUri"`（值类型必填、无 null 守卫） | `req base_uri: WorkspaceFolderOrURI` | union 自身处理 `{`/`"` 派发与拒绝其他 kind。 |
| `WorkspaceFolderOrURI{ WorkspaceFolder *; URI * }` | `{ workspace_folder: Option<WorkspaceFolder>, uri: Option<URI> }` + 手写 serde | `{`→folder、`"`→URI、其余 err。 |
| `Uri URI json:"uri"` | `req uri: crate::URI` | 复用 `lsp.rs` 的 `URI(pub String)` newtype（Go `type URI string`）。 |

**红→绿推进序（slice）**：
1. **tracer（真 RED→GREEN）**：`WorkspaceFoldersServerCapabilities` + `StringOrBoolean`（测试引用不存在类型→编译失败 RED→加 union+`lsp_object!`→GREEN）。
2. fileOperations 链自底向上，逐个真 RED→GREEN：`FileOperationPattern`(+`FileOperationPatternKind`/`FileOperationPatternOptions`，tracer)→`FileOperationFilter`→`FileOperationRegistrationOptions`→`FileOperationOptions`。
3. `TextDocumentContentOptions`(tracer)→`TextDocumentContentRegistrationOptions`→union→`WorkspaceOptions`→**swap `ServerCapabilities.workspace`**（headline 真 RED→GREEN：测试访问 `caps.workspace.unwrap().file_operations` 在 raw-`Value` 态编译失败 → 换 typed → GREEN）。
4. `WorkspaceFolder`(tracer)→`WorkspaceFolderOrURI`→`RelativePattern`→**upgrade `PatternOrRelativePattern.relative_pattern`**（headline 真 RED→GREEN：测试访问 `.relative_pattern.unwrap().base_uri` 在 raw-`Value` 态编译失败 → 换 typed → GREEN）。
5. 各 union 派发臂 / `default → {}`（全可选结构）/ 必填缺失文案为 **green-on-arrival**（诚实标注）。

> 诚实记录：每个新类型的首个引用测试均为真 RED（类型/字段不存在→编译失败），实现后 GREEN；同类型的「另一 union 臂」「round-trip」「default→{}」「errMissing 文案」多为 green-on-arrival（手写 union/宏一旦成立即过）。既有 `pattern_or_relative_pattern_relative_variant_raw`（仅断言 `.is_some()` + round-trip）在升级为 typed 后仍绿（green-on-arrival）。

**附加偏离（divergence）**：
- `TextDocumentContentOptionsOrRegistrationOptions` 的 registration 变体经 deserialize 实际不可达（options 总先匹配带 `schemes` 的对象），与 Go try-order 完全一致；显式构造时仍可序列化（已测）。

**实现 TODO（本轮，可勾选）**：

- [x] `StringOrBoolean`（`string | boolean` union）+ `WorkspaceFoldersServerCapabilities`（tracer）　`// Go: lsp_generated.go:StringOrBoolean / WorkspaceFoldersServerCapabilities`
- [x] fileOperations 链：`FileOperationPatternKind`/`FileOperationPatternOptions`/`FileOperationPattern`/`FileOperationFilter`/`FileOperationRegistrationOptions`/`FileOperationOptions`　`// Go: lsp_generated.go:FileOperation*`
- [x] `TextDocumentContentOptions`/`TextDocumentContentRegistrationOptions`/`TextDocumentContentOptionsOrRegistrationOptions`（try-decode union）　`// Go: lsp_generated.go:TextDocumentContent*`
- [x] `WorkspaceOptions` 组装 + 把 `ServerCapabilities.workspace` 从 raw JSON 换成 `Option<WorkspaceOptions>`（清空 ServerCapabilities 最后一个 raw 槽）　`// Go: lsp_generated.go:WorkspaceOptions / ServerCapabilities`
- [x] `WorkspaceFolder`/`WorkspaceFolderOrURI`/`RelativePattern` + 把 `PatternOrRelativePattern.relative_pattern` 升级为 typed `RelativePattern`　`// Go: lsp_generated.go:WorkspaceFolder/WorkspaceFolderOrURI/RelativePattern/PatternOrRelativePattern`

## 转交 / 推迟（DEFER）

- ✅ 上一轮的 `(*ClientCapabilities).Resolve()` DEFER **已解除**（落地于 `capabilities.rs`，4 组全树）。
- ✅ 上一轮维持的 `ServerCapabilities` open-object **已退役**，落地 typed 树（高价值 11 组 provider 全 typed）。
- ✅ 上一轮把 `ServerCapabilities` 22 个 raw-JSON DEFER provider 中的 **21 个**落成 typed option/registration 树。
- ✅ 上一轮移植 registration-options base tree（`StaticRegistrationOptions`/`DocumentSelectorOrNull`/`DocumentFilter` union/`TextDocumentRegistrationOptions`），并把全部 **14 个** raw-JSON registration 槽（12 triple-union + diagnostic + semanticTokens）升级为 typed `*RegistrationOptions`。triple-union / `*OrRegistrationOptions` 的 registration 变体 **DEFER 已全部解除**。
- ✅ **本轮**移植 `WorkspaceOptions` 子树（`WorkspaceFoldersServerCapabilities`/`StringOrBoolean`/`FileOperation*` 链/`TextDocumentContent*` + union），把 `ServerCapabilities.workspace` 从 `serde_json::Value` 升级为 `Option<WorkspaceOptions>`——**`ServerCapabilities` 的最后一个 raw-JSON DEFER 槽已清空**。
- ✅ **本轮**移植 `RelativePattern` 对象变体（`WorkspaceFolder`/`WorkspaceFolderOrURI`/`RelativePattern`），把 `PatternOrRelativePattern.relative_pattern` 从 `serde_json::Value` 升级为 `Option<RelativePattern>`——**`PatternOrRelativePattern` 两个变体现已全 typed**。
- `// DEFER`：`CodeActionKindDocumentation`（`CodeActionOptions.documentation` 的 `[]*` 元素，proposed/稀有）保持 raw JSON。`blocked-by:` 生成器 pass。这是 `ServerCapabilities` provider 树里**唯一**剩余的 raw-JSON DEFER（不在本轮范围）。
- `// DEFER`：新枚举 `String()` stringer（同生成器 pass；本轮 `TextDocumentSyncKind` 已含 `Display` 复刻 stringer）。
- `// DEFER`：请求/resolved 树字段的 Go `errNull` 精确文案（低优先偏差；`null` 仍被拒绝，仅文案不同）。
- `// DEFER`：Go 个别非指针字段（`support`/`tokenTypes` 等）建成 `Option<T>` 的统一化偏离。
- 维持 `InitializationOptions` open-object 现状（用户自定义初始化项，非本轮范围；公共 API 仅**新增**）。
