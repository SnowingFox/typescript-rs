# lsp: 实现方案（impl.md）

> 写前已实读 `internal/lsp/*.go`（logger.go / progress.go / server.go / stack_sanitizer.go）与 `internal/lsp/lsproto/*.go`（baseproto / jsonrpc / lsp / util + 抽样 `lsp_generated.go` 38691 行的关键结构）。所有 TODO 带 `// Go:` 锚点。

**crate**：`tsgo_lsp`（含子 crate `tsgo_lsproto`）　**目标**：LSP 服务器主循环 + LSP 协议类型/编解码。把 stdin/stdout 上的 LSP 消息分发到 `tsgo_ls` 语言服务，管理 `tsgo_project` 会话、进度上报、客户端能力、API 子会话。
**依赖（crate）**：`tsgo_jsonrpc` `tsgo_json` `tsgo_lsproto` `tsgo_ls`（lsconv/lsutil）`tsgo_project`（含 ata/logging）`tsgo_api` `tsgo_core` `tsgo_collections` `tsgo_diagnostics` `tsgo_locale` `tsgo_pprof` `tsgo_tspath` `tsgo_vfs` `tsgo_bundled`。并发：`rayon` 不需要；用 `crossbeam-channel` + `std::thread::scope`（替代 goroutine + channel + errgroup）。
**Go 源**：`internal/lsp/`（4 个非测试 .go + `lsproto/` 5 个非测试 .go，其中 `lsp_generated.go` 为机器生成 ~107 万字符）

## 这个包是什么（业务说明）

`lsp` 是编辑器与 typescript-go 之间的**协议边界 + 调度中枢**。它把 LSP（JSON-RPC over stdio）协议落到内部能力上：

1. **`lsproto` 子包（协议层）**：LSP 3.17 元模型生成的全部协议类型（params/result/能力/枚举/联合类型），加上手写的 `Message`/`RequestMessage`/`ResponseMessage`（含按 `Method` 分发的 params/result 解码）、`DocumentUri.FileName()` URI↔路径换算、`ResolvedClientCapabilities`（把客户端声明的能力"解析"成易查询的布尔树）、position/range 比较器。lsp_generated.go 由 `_generate/generate.mts` 从 LSP meta-model 生成，并叠加 typescript-go 的自定义扩展方法（`_/textDocument/sourceDefinition`、`textDocument/x-multiDocumentHighlight`、telemetry、API session 等）。
2. **`Server`（调度层，server.go）**：4 个并发循环（read / dispatch / write / readLoopErr 守卫）通过 channel 串起来。`readLoop` 读消息→分类；请求进 `requestQueue`；`dispatchLoop` 取请求，建可取消 ctx，调 `handlerMap` 里注册的 handler，把同步部分跑完、异步部分丢到 goroutine；`writeLoop` 把出站消息写回。`Server` 实现 `project.Client`（文件监听注册、诊断推送、进度、配置请求）和 `ata.NpmExecutor`。
3. **进度（progress.go）**：单 goroutine 状态机管理 `$/progress`（WorkDoneProgress），带"延迟显示防抖"+ 引用计数。
4. **日志/栈净化（logger.go / stack_sanitizer.go）**：`logger` 把日志变成 `window/logMessage` 通知；`stack_sanitizer` 把 panic 栈净化（去掉非本仓帧、防 VS Code "Generic Secret" 正则误伤）后随 telemetry 上报。

它在 P8 是因为它装配了 P7 的 `ls`、本 phase 的 `project` 与 `api`，是"组合根"之一（真正的 main 在 P9 `cmd/tsgo`）。

## 所有权 / 类型映射（本包关键决策）

### 协议序列化决策（lsproto）—— **核心，已定**

**结论：不采用外部 `lsp-types` crate；保留"从 LSP meta-model 代码生成 Rust 类型 + `serde`"路线。**

理由（与 PORTING §3/§10「serde 派生（或 lsp-types 视 P8 决定）」的 P8 决策回填）：

1. **行为 1:1 不可让步**：`lsp_json_test.go` 断言了大量精细行为 —— optional-non-nullable 字段拒绝 `null`、未知字段忽略、required 缺失报 `missing required properties: ...`、四类联合（discriminator-`kind` / presence / boolean-or-options / string-or-array）、字符串字面量类型、`omitzero` 省略、枚举 `String()` 名表。`lsp-types` 的 `#[serde(untagged)]` 联合**无法**精确复刻"presence 判别"和"拒绝 null"，且不会报相同错误文案。
2. **自定义扩展**：typescript-go 有 `lsp-types` 没有的方法/类型：`_/textDocument/sourceDefinition`、`textDocument/x-multiDocumentHighlight`、VS Code references、`telemetry/event`、`_typescriptgo/*` 调试命令、`InitializeAPISession`、`ProjectInfo`、`InitializationOptions.UserPreferences`。
3. **生成器复用**：移植 `_generate/generate.mts`（已是 TS）使其 emit Rust（`serde` derive + 联合/字面量的手写 `Deserialize`/`Serialize`），保持"改 meta-model→重生成"的工作流，避免手抄 38691 行。**第一阶段可手抄被测试覆盖的子集**（InlayHint/FoldingRange/Location/Initialize* 等约 30 个类型 + 全部联合/枚举模式），其余随 parity 增量生成。

具体 serde 落地：

| Go（生成）构造 | Rust 表示 | serde 策略 |
|---|---|---|
| `type X struct { A T \`json:"a"\`; B *U \`json:"b,omitzero"\` }` + 手写 `UnmarshalJSONFrom` | `struct X { a: T, b: Option<U> }` | `#[derive(Serialize, Deserialize)]`；`b` 用 `#[serde(skip_serializing_if="Option::is_none", default)]`；**但** required+拒 null+报缺失文案 需自定义 `Deserialize`（见下） |
| optional-non-nullable 字段收到 `null` → `errNull("b")` | 自定义 `Deserialize`：遇 `null` 且字段非 nullable → `Err("null value is not allowed for field \"b\"")` | 不能用裸 `Option`（serde 默认把 null→None）；用 visitor 区分 absent vs null |
| required 缺失 → `errMissing([...])` | 自定义 `Deserialize` 累积缺失键 → `"missing required properties: a, b"` | bitmask 跟踪，文案逐字对齐 |
| 未知字段 | 忽略（`SkipValue`） | `#[serde(deny_unknown_fields)]` **不要加**；默认即忽略 |
| `IntegerOrString{Integer,String}` | `enum IntegerOrString { Integer(i32), String(String) }` | 自定义：number→Integer，string→String，其它→err；marshal 反之 |
| `IntegerOrNull{Integer}` | `enum IntegerOrNull { Integer(i32), Null }` | null→Null；marshal None→`null` |
| `DocumentUriOrNull` | `enum DocumentUriOrNull { DocumentUri(DocumentUri), Null }` | 同上 |
| `BooleanOrHoverOptions{Boolean,HoverOptions}`（boolean-or-options） | `enum BooleanOrHoverOptions { Boolean(bool), Options(HoverOptions) }` | bool→Boolean，object→Options，string→err |
| `WorkDoneProgressBeginOrReportOrEnd`（discriminator `kind`） | `enum WorkDoneProgress { Begin(..), Report(..), End(..) }` | 读 `kind` 字段判别；非法 kind→err。可用 `#[serde(tag="kind")]` 但需 rename `"begin"/"report"/"end"`（注意 Go 字段名 vs JSON kind 值） |
| `TextEditOrInsertReplaceEdit`（presence 判别：有 `range`=TextEdit；有 `insert`=InsertReplaceEdit） | `enum ...` | **不能** `#[serde(untagged)]`（顺序/性能差且不报准确错误）；自定义：先 peek 存在哪个键 |
| `TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile`（无 `kind`=TextDocumentEdit；`kind:"create"/"rename"/"delete"`） | `enum ...` | 先看有无 `kind`，无则 TextDocumentEdit，有则按值分派 |
| `StringOrInlayHintLabelParts{String,InlayHintLabelParts}`（string-or-array） | `enum ...` | string→String，array→Parts |
| `StringLiteralCreate struct{}` ⇄ `"create"` | `struct StringLiteralCreate;` | serialize→`"create"`；deserialize 校验等于 `"create"` 否则 err |
| `type InlayHintKind uint32` + `String()` 名表 | `#[repr(u32)] enum InlayHintKind { Type=1, Parameter=2 }` + `Display` | 数字 (de)serialize；`String()`→名表；未知值 `String()` 含数字 |
| `type Method string` + `MethodXxx` 常量 | `pub struct Method(String)` 或 `enum Method`（带 `Custom(String)`） | 见 dispatch |
| `unmarshalParams(method, data)` / `unmarshalResult(method, data)`（大 switch） | `fn unmarshal_params(method:&Method, raw:&RawValue) -> Result<ParamsEnum>` | 由生成器 emit；返回判别联合 `enum Params { Initialize(Box<InitializeParams>), ... }`（替代 Go 的 `any`） |
| `ResolvedClientCapabilities`（`Resolve()` 把 `*ClientCapabilities` 解析成布尔树） | 同名 struct + `ClientCapabilities::resolve(&self) -> ResolvedClientCapabilities` | nil→默认（全 false）；逐字段 `resolve()` |

> **关键偏离**：Go 把 `Message.msg`、`RequestMessage.Params`、handler 入参都用 `any`（接口空类型），靠运行时类型断言 `req.Params.(*lsproto.InitializeParams)`。Rust 没有这种 `any`，改用**判别联合 `enum Params`**（生成器从 method→type 映射 emit）。handler 注册改为 `match` on `Params` 变体。这是必要且安全的偏离，在 impl 顶部「所有权模型」小节写明。

### Server 并发模型（server.go）—— Rust 落地

Go：`errgroup` + 3 loop（dispatch/write/readLoopErr）+ `go s.readLoop`。channel：`requestQueue`(100)、`outgoingQueue`(100)、`pendingServerRequests`(每请求 1)。`context.Context` 取消传播。

Rust：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `errgroup.Group` + `g.Go(...)` | `std::thread::scope(\|s\| { s.spawn(...) })` + 首错收集 | 任一线程返回错误即触发整体取消 |
| `chan *RequestMessage`(100) | `crossbeam_channel::bounded(100)` | requestQueue |
| `chan *Message`(100) | `crossbeam_channel::bounded(100)` | outgoingQueue |
| `context.Context` + `WithCancel`/`WithCancelCause` | `Arc<CancellationToken>`（自实现：`AtomicBool` + 唤醒）或 `tokio_util::sync::CancellationToken` 的同步等价 | 不引 tokio；用 `Arc<AtomicBool>` + `crossbeam` select 实现 done()。每请求一个子 token |
| `pendingClientRequests map[ID]{req,cancel}` + Mutex | `Mutex<FxHashMap<Id, PendingClientRequest>>` | cancel = 关闭/置位该请求的 token |
| `pendingServerRequests map[ID]chan resp` + Mutex | `Mutex<FxHashMap<Id, Sender<ResponseMessage>>>` | server→client 请求的回包路由 |
| `atomic.Bool/Int32/Int64/Uint32` | `AtomicBool/AtomicI32/AtomicI64/AtomicU32` | initStarted/clientSeq/lastRequestTimeMs/watcherID |
| `sync.OnceValue(func() handlerMap{...})` | `once_cell::sync::Lazy<HandlerMap>` 或 `std::sync::OnceLock` | handler 表只建一次 |
| `go func(){ doAsyncWork() }()` | `scope.spawn` 或专用 worker；输出经 outgoingQueue 保序 | 异步 handler 工作 |
| `collections.SyncSet[WatcherID]` | `dashmap::DashSet<WatcherID>` 或 `Mutex<FxHashSet>` | watchers |

**确定性**：出站消息全部经单一 `outgoingQueue` → 单 `writeLoop` 串行写，天然保序（与 Go 一致），TDD 断言可靠。

### handler 注册的泛型问题

Go 用泛型 `registerRequestHandler[Req,Resp]` / `registerLanguageServiceDocumentRequestHandler[...]` 把"取 params→调 ls→sendResult"样板收敛。Rust：保留这些泛型注册函数（trait bound `Req: HasTextDocumentUri` 等），handler 闭包签名 `Fn(&Server, &Ctx, &RequestMessage) -> Result<Option<AsyncWork>>`。`HasTextDocumentURI`/`HasTextDocumentPosition`/`HasLocations`/`HasLocation` → Rust trait（lsproto 里）。

## 文件清单 → Rust 模块

### crate `tsgo_lsproto`（`internal/lsp/lsproto/`，作独立 workspace 成员）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `lsproto/lsp.go` | `internal/lsp/lsproto/lsp.rs`（= `lib.rs`，crate 根） | `Message`/`RequestMessage`/`ResponseMessage`（按 method 解码 params/result）、`NewID`。basename≠目录名（`lsp` vs `lsproto`）→ 作 `lib.rs`（crate 入口约定） |
| `lsproto/baseproto.go` | `internal/lsp/lsproto/baseproto.rs` | `BaseReader`/`BaseWriter`：薄包装 `tsgo_jsonrpc::Reader/Writer`（可直接 re-export + 别名） |
| `lsproto/jsonrpc.go` | `internal/lsp/lsproto/jsonrpc.rs` | 注意：同名于 crate `tsgo_jsonrpc` —— 这里只是 lsproto 内的 `Message` 等，模块名用 `message.rs` 避免歧义 |
| `lsproto/util.go` | `internal/lsp/lsproto/util.rs` | `DocumentUri.FileName()`/`Path()`、URI 修正、helper trait、`PreferredMarkupKind`、CodeActionKind 常量、`WithClientCapabilities` ctx 注入、json key 扫描 helper |
| `lsproto/lsp_generated.go` | `internal/lsp/lsproto/generated.rs`（机器生成，可拆多文件） | 全部协议类型 + `unmarshal_params`/`unmarshal_result` 分发 + `Method`/`ErrorCode` 常量 + `ResolvedClientCapabilities` + 枚举名表 |
| `lsproto/_generate/generate.mts` | `internal/lsp/lsproto/_generate/`（保留 TS，改 emit Rust） | 代码生成器（DEFER：先手抄子集，生成器随后） |

### crate `tsgo_lsp`（`internal/lsp/`）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `server.go` | `internal/lsp/server.rs`（= `lib.rs`，crate 根） | `Server`/`ServerOptions`/`Reader`/`Writer` trait/loops/handlerMap/全部 `handleXxx`。basename≠目录名→`lib.rs` |
| `logger.go` | `internal/lsp/logger.rs` | `logger`（实现 `project::logging::Logger`），把日志→`window/logMessage` |
| `progress.go` | `internal/lsp/progress.rs` | `projectLoadingProgress` 进度状态机 + `progressReporter` trait + `serverProgressReporter` |
| `stack_sanitizer.go` | `internal/lsp/stack_sanitizer.rs` | panic 栈净化 + 防 VS Code Generic-Secret 正则 |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| 序列化 | `serde` / `serde_json`（含 `RawValue`） | 协议 (de)serialize；联合/字面量自定义 impl |
| channel / worker | `crossbeam-channel` | requestQueue/outgoingQueue/pendingServerRequests |
| 并发 set | `dashmap` | watchers（`SyncSet`） |
| 一次性初始化 | `once_cell` 或 std `OnceLock` | handler 表、正则 |
| 正则 | `regex` | stack_sanitizer 的两个正则 |
| 随机 | `rand` | `generateAPIPipePath`（`rand/v2` → `rand`） |
| 错误 | `thiserror` | ErrorCode 包装、userFacingRequestFailedError |

`regex` / `rand` / `once_cell` 为本 phase 新增，需追加到 `references/crate-map.md`（执行期 `cargo add`）。

## 实现 TODO（逐文件 / 逐函数）

### `lsproto`：`util.rs`（Go: `lsproto/util.go`）

- [ ] `pub struct DocumentUri(String)`　`// Go: util.go:DocumentUri`
- [ ] `pub fn DocumentUri::file_name(&self) -> String`（bundled 直返；`file://` 用 url 解析，含 host / Windows 卷修正；其它 scheme 转 `^/scheme/authority/path`）　`// Go: util.go:DocumentUri.FileName`
- [ ] `pub fn DocumentUri::path(&self, use_case_sensitive: bool) -> tspath::Path`　`// Go: util.go:DocumentUri.Path`
- [ ] `fn fix_windows_uri_path(path:&str) -> String`　`// Go: util.go:fixWindowsURIPath`
- [ ] traits `HasTextDocumentURI`/`HasTextDocumentPosition`/`HasLocations`/`HasLocation`　`// Go: util.go:HasTextDocumentURI..HasLocation`
- [ ] `pub fn preferred_markup_kind(formats:&[MarkupKind]) -> MarkupKind`　`// Go: util.go:PreferredMarkupKind`
- [ ] `Null` / `NoParams` 类型 + serde（`null` / `{}`）　`// Go: util.go:Null/NoParams`
- [ ] ctx 注入 `with_client_capabilities` / `get_client_capabilities`（Rust：显式传 `&ResolvedClientCapabilities`，替代 ctx value）　`// Go: util.go:WithClientCapabilities/GetClientCapabilities`
- [ ] json 扫描 helper `json_object_raw_field` / `json_object_has_key`（serde_json 流式）　`// Go: util.go:jsonObjectRawField/jsonObjectHasKey`
- [ ] CodeActionKind 常量（removeUnusedImports/sortImports）　`// Go: util.go`
- [ ] `RequestInfo<Params,Resp>` / `NotificationInfo<Params>`（构造请求/通知 + `UnmarshalResult`）　`// Go: util.go:RequestInfo/NotificationInfo`

### `lsproto`：`message.rs`（Go: `lsproto/jsonrpc.go`）

- [ ] `pub struct Message { kind: MessageKind, msg: MsgBody }`（`MsgBody = Request(RequestMessage)|Response(ResponseMessage)`）　`// Go: jsonrpc.go:Message`
- [ ] `Message::as_request/as_response`　`// Go: jsonrpc.go:Message.AsRequest/AsResponse`
- [ ] `Message` 自定义 `Deserialize`：有 id 无 method→Response；按 method `unmarshal_params`；错误包 `ErrorCodeInvalidRequest`/`ErrorCodeInvalidParams`　`// Go: jsonrpc.go:Message.UnmarshalJSON`
- [ ] `Message` `Serialize`（委托 msg）　`// Go: jsonrpc.go:Message.MarshalJSON`
- [ ] `RequestMessage { jsonrpc, id, method, params }` + `Message()` 构造 + 自定义 Unmarshal　`// Go: jsonrpc.go:RequestMessage`
- [ ] `ResponseMessage { jsonrpc, id, result, error }` + `Message()`　`// Go: jsonrpc.go:ResponseMessage`
- [ ] `pub fn new_id(raw: IntegerOrString) -> jsonrpc::Id`　`// Go: jsonrpc.go:NewID`

### `lsproto`：`baseproto.rs`（Go: `lsproto/baseproto.go`）

- [ ] `BaseReader`/`BaseWriter` = `tsgo_jsonrpc::Reader/Writer` 别名或薄 newtype + `NewBaseReader`/`NewBaseWriter`　`// Go: baseproto.go`

### `lsproto`：`generated.rs`（Go: `lsproto/lsp_generated.go`，机器生成）

- [ ] 结构类型（params/result/options/capabilities…）+ 自定义 `Deserialize`（拒 null / 报缺失 / 忽略未知）　`// Go: lsp_generated.go`
- [ ] 4 类联合类型（discriminator / presence / boolean-or / string-or-array）的自定义 (de)serialize　`// Go: lsp_generated.go:*Or*`
- [ ] 字符串字面量类型（`StringLiteralCreate` 等）　`// Go: lsp_generated.go`
- [ ] 整数枚举 + `Display` 名表（InlayHintKind/SymbolKind/…）　`// Go: lsp_generated.go`
- [ ] `Method` 常量 + `ErrorCode` 常量（含 -32700..-32603、ServerNotInitialized 等）　`// Go: lsp_generated.go:ErrorCode/Method`
- [ ] `unmarshal_params(method, raw)` / `unmarshal_result(method, raw)` 分发　`// Go: lsp_generated.go:unmarshalParams/unmarshalResult`
- [ ] `ResolvedClientCapabilities` + `ClientCapabilities::resolve()`（nil→默认）　`// Go: lsp_generated.go:ResolvedClientCapabilities/Resolve`
- [ ] position/range 比较器（来自 lsproto/util.go 的 `ComparePositions`/`CompareRanges`，归到 generated 或 util）　`// Go: util.go:ComparePositions/CompareRanges`

### `tsgo_lsp`：`stack_sanitizer.rs`（Go: `stack_sanitizer.go`）

- [ ] `fn sanitize_stack_trace(stack:&str) -> String`（从 `runtime/debug.Stack()` 起截；逐行：保留前导空白→`typescript-go/internal` 后净化模块/路径，否则 `(REDACTED FRAME)`）　`// Go: stack_sanitizer.go:sanitizeStackTrace`
- [ ] `fn write_sanitized_module_or_path(line, out)`（去 `+0x..`/`in goroutine`；按 `/` 分段，`|>` 连接；`)` 结尾去参数→`()`）　`// Go: stack_sanitizer.go:writeSanitizedModuleOrPath`
- [ ] `fn defeat_generic_secret_regex(s) -> String`（`(key|token|signature|sig|pwd)([(\[.|])` → `${1}X_X${2}`）　`// Go: stack_sanitizer.go:defeatGenericSecretRegex`

### `tsgo_lsp`：`logger.rs`（Go: `logger.go`）

- [ ] `struct Logger { server, mu: Mutex, verbose: bool }` impl `project::logging::Logger`　`// Go: logger.go:logger`
- [ ] `fn send_log_message(&self, ty: MessageType, msg: String)`（initStarted 前→stderr；否则 `window/logMessage` 通知，shutdown 时回退 stderr）　`// Go: logger.go:sendLogMessage`
- [ ] `Log/Logf/Error/Errorf/Warn/Warnf/Info/Infof` + `Verbose/IsVerbose/SetVerbose`　`// Go: logger.go:*`

### `tsgo_lsp`：`progress.rs`（Go: `progress.go`）

- [ ] trait `ProgressReporter`（`done`/`localize`/`create_work_done_progress`/`send_progress`）　`// Go: progress.go:progressReporter`
- [ ] `struct ServerProgressReporter`（适配 `&Server`）　`// Go: progress.go:serverProgressReporter`
- [ ] `struct ProjectLoadingProgress { reporter, ch: Sender<ProgressEvent>, delay }`　`// Go: progress.go:projectLoadingProgress`
- [ ] `new_project_loading_progress(server, delay)` / `_from_reporter(reporter, delay)`（spawn run 线程）　`// Go: progress.go:newProjectLoadingProgress*`
- [ ] `start(msg, args)` / `finish(msg, args)`（send 或 done 退出）　`// Go: progress.go:start/finish`
- [ ] `run()`：单线程状态机（OrderedMap 引用计数 + token 生成 `tsgo-loading-N` + 延迟 timer + delayFired + begun）　`// Go: progress.go:run`（**关键并发点**，见下）
- [ ] `begin_or_report(token, text, begun) -> bool`　`// Go: progress.go:beginOrReport`

### `tsgo_lsp`：`server.rs`（lib.rs；Go: `server.go`）

- [ ] `struct ServerOptions { ... }` / `fn new_server(opts) -> Server`（Cwd 必填，否则 panic）　`// Go: server.go:ServerOptions/NewServer`
- [ ] `trait Reader { fn read() -> Result<Message> }` / `trait Writer { fn write(&Message) }` + `lspReader`/`lspWriter` + `to_reader`/`to_writer`　`// Go: server.go:Reader/Writer/ToReader/ToWriter`
- [ ] `struct Server { ... }`（全字段；channel/atomic/mutex 见类型映射）　`// Go: server.go:Server`
- [ ] `project::Client` impl：`watch_files`/`unwatch_files`/`refresh_diagnostics`/`publish_diagnostics`/`send_telemetry`/`is_active`/`refresh_inlay_hints`/`refresh_code_lens`/`progress_start`/`progress_finish`/`request_configuration`　`// Go: server.go:(*Server).WatchFiles..RequestConfiguration`
- [ ] `fn run(&self, ctx) -> Result<()>`（scope spawn dispatch/write/read 守卫）　`// Go: server.go:Run`（**关键并发点**）
- [ ] `fn read_loop(ctx)`（分类消息；初始化前只接受 `initialize`；响应路由 pendingServerRequests；`$/cancelRequest`→cancel）　`// Go: server.go:readLoop`（**并发：请求处理**）
- [ ] `fn cancel_request(raw_id)`　`// Go: server.go:cancelRequest`
- [ ] `fn dispatch_loop(ctx)`（取请求→建可取消 ctx→注册 pending→同步/异步执行→错误映射 RequestCancelled/EOF）　`// Go: server.go:dispatchLoop`（**并发点**）
- [ ] `fn write_loop(ctx)`　`// Go: server.go:writeLoop`
- [ ] `fn send_client_request<Req,Resp>(...) -> Result<Resp>`（注册回包 chan→send→等 ctx/resp）　`// Go: server.go:sendClientRequest`
- [ ] `fn send_client_request_fire_and_forget(...)`　`// Go: server.go:sendClientRequestFireAndForget`
- [ ] `fn send_result/send_error/send_notification/send_response/send`（send 经 outgoingQueue，respect ctx done）　`// Go: server.go:sendResult/sendError/sendNotification/sendResponse/send`
- [ ] `userFacingRequestFailedError`（Unwrap→`ErrorCodeRequestFailed`）　`// Go: server.go:userFacingRequestFailedError`
- [ ] `fn handle_request_or_notification(ctx, req) -> Result<Option<AsyncWork>>`（查表→执行→计时日志）　`// Go: server.go:handleRequestOrNotification`
- [ ] `handlerMap` + `handlers()`（Lazy 初始化，注册全部方法）　`// Go: server.go:handlerMap/handlers`
- [ ] 注册泛型：`register_notification_handler` / `register_request_handler` / `register_language_service_document_request_handler` / `register_language_service_with_auto_imports_request_handler` / `register_multi_project_reference_request_handler`　`// Go: server.go:register*`
- [ ] `crossProjectOrchestrator`（实现 `ls::CrossProjectOrchestrator`）+ `getLanguageServiceAndCrossProjectOrchestrator`　`// Go: server.go:crossProjectOrchestrator`
- [ ] `fn recover(&self, req)`（panic→日志+错误响应+telemetry 净化栈）　`// Go: server.go:recover`
- [ ] `fn handle_initialize(ctx, params) -> InitializeResponse`（解析能力、positionEncoding、locale、trace、watchdog；返回 ServerCapabilities）　`// Go: server.go:handleInitialize`
- [ ] `fn handle_initialized(ctx, params)`（决定 cwd、建 `project::Session`、`RequestConfiguration`、注册 watch、close initComplete）　`// Go: server.go:handleInitialized`
- [ ] `fn handle_shutdown/handle_exit`（exit 返回 EOF）　`// Go: server.go:handleShutdown/handleExit`
- [ ] 文档同步：`handle_did_change_workspace_configuration`/`handle_did_open`/`handle_did_change`/`handle_did_save`/`handle_did_close`/`handle_did_change_watched_files`/`handle_set_trace`　`// Go: server.go:handleDid*/handleSetTrace`
- [ ] 语言服务请求（委托 `ls.LanguageService`）：`handle_document_diagnostic`/`handle_hover`/`handle_definition`/`handle_source_definition`/`handle_type_definition`/`handle_signature_help`/`handle_folding_range`/`handle_vs_on_auto_insert`/`handle_linked_editing_range`/`handle_document_format`/`handle_document_range_format`/`handle_document_on_type_format`/`handle_document_symbol`/`handle_document_highlight`/`handle_multi_document_highlight`/`handle_selection_range`/`handle_code_action`/`handle_inlay_hint`/`handle_code_lens`/`handle_semantic_tokens_full`/`handle_semantic_tokens_range`　`// Go: server.go:handle*`
- [ ] 补全：`handle_completion`/`handle_completion_item_resolve`（auto-imports 重试路径）　`// Go: server.go:handleCompletion/handleCompletionItemResolve`
- [ ] rename：`handle_prepare_rename`/`handle_rename`/`handle_will_rename_files`/`handle_will_rename_files_worker`（dedup edits/renames，文件改名→willRename）　`// Go: server.go:handlePrepareRename/handleRename/handleWillRenameFiles*`
- [ ] 跨项目：`handle_workspace_symbol`/`handle_call_hierarchy_incoming_calls`/`handle_call_hierarchy_outgoing_calls`/`handle_prepare_call_hierarchy`/`handle_code_lens_resolve`　`// Go: server.go:handle*`
- [ ] API session：`handle_initialize_api_session`（建 `api::Session` + pipe transport + 后台 accept goroutine）/`generate_api_pipe_path`/`remove_api_session`　`// Go: server.go:handleInitializeAPISession/generateAPIPipePath/removeAPISession`
- [ ] 调试命令：`handle_run_gc`/`handle_save_heap_profile`/`handle_save_alloc_profile`/`handle_start_cpu_profile`/`handle_stop_cpu_profile`/`handle_project_info`　`// Go: server.go:handleRunGC..handleProjectInfo`
- [ ] `SetCompilerOptionsForInferredProjects` / `NpmInstall`（ata.NpmExecutor）　`// Go: server.go:SetCompilerOptionsForInferredProjects/NpmInstall`
- [ ] `fileRenameFilters` 静态 + `Session()`/`InitComplete()` 访问器　`// Go: server.go`

### Cargo / crate 接线

- [ ] `internal/lsp/lsproto/Cargo.toml`（`name = "tsgo_lsproto"`，deps: `tsgo_jsonrpc` `tsgo_json` `tsgo_tspath` `tsgo_bundled` `serde` `serde_json`）
- [ ] `internal/lsp/Cargo.toml`（`name = "tsgo_lsp"`，deps 见上）
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/lsp/lsproto` 与 `internal/lsp`
- [ ] `tsgo_lsproto` lib.rs（= `lsp.rs`）声明子模块 + re-export
- [ ] `tsgo_lsp` lib.rs（= `server.rs`）声明 `mod logger; mod progress; mod stack_sanitizer;` + re-export `Server`/`ServerOptions`/`Reader`/`Writer`/`ToReader`/`ToWriter`

## TDD 推进顺序（tracer bullet → 增量）

1. **lsproto base 协议 + 基础联合**：先把 `baseproto`（其实是 jsonrpc）跑通，再实现 `IntegerOrString`/`IntegerOrNull`/`DocumentUriOrNull` 的 (de)serialize（`lsp_json_test.go:TestUnmarshalUnionTypes/TestMarshalUnionTypes`）。
2. **核心结构 + null/缺失/未知字段语义**：`Location`/`InlayHint`/`FoldingRange`（`TestUnmarshalRejectsNullForOptionalNonNullableFields`、`TestUnmarshalRejectsIncompleteObjects`、`TestUnmarshalIgnoresUnknownFields`、`TestMarshalOmitsZeroOptionalFields`、`TestMarshalUnmarshalRoundTrip`）。
3. **四类联合 + 字面量 + 枚举名表**（`TestUnmarshalDiscriminatorUnion`/`TestUnmarshalPresenceDiscriminatorUnion`/`TestUnmarshalDocumentEditUnion`/`TestUnmarshalBooleanUnionTypes`/`TestUnmarshalStringOrArrayUnion`/`TestLiteralTypes`/`TestEnumStringValues`）。
4. **`CompletionItem` 复杂解码**（`lsp_test.go:TestUnmarshalCompletionItem`）。
5. **stack_sanitizer**（纯函数，baseline 对拍：`stack_sanitizer_test.go` 3 个）。
6. **progress 状态机**（`progress_test.go:TestProgress` 11 子用例，用 fake reporter + 模拟时钟）。
7. **Server 端到端**（`server_*_test.go`，需 `project`/`ls` 就绪 → 大部分推迟到这些 crate green 之后；其中 shutdown/projectinfo/progress-e2e 可在 P8 末尾收口）。

## 与 Go 的已知偏离（divergence）

- **`any` → 判别联合**：`Message.msg`/`Params`/handler 入参从 Go 的 `any` + 类型断言改成 `enum Params`/`enum MsgBody`（生成器 emit method→type 映射）。语义等价、更安全。
- **协议序列化不用 `lsp-types`**：见上「协议序列化决策」。保留 meta-model 代码生成 + serde 自定义 impl。
- **ctx value → 显式参数**：Go 用 `context.WithValue` 传 `ResolvedClientCapabilities`/locale/requestID；Rust 显式传引用（PORTING §3 `context.Context` → 显式取消/参数）。
- **goroutine + errgroup → `std::thread::scope` + crossbeam**：4 循环结构保持，取消用 `Arc<AtomicBool>` token。`testing/synctest`（progress 测试用的虚拟时钟）在 Rust 用可注入的 `Clock` trait / `fake reporter` + 手动推进（见 tests.md）。
- **`net/url` 解析**：`DocumentUri.FileName` 的 `file://` 解析，Rust 用 `url` crate 或自实现（贴近 Go `url.Parse` 行为，含 host）。先标 `// TODO(port)` 决定是否引 `url`。
- **`api` 后台 accept goroutine**：保留为 `std::thread::spawn`，panic recovery 用 `std::panic::catch_unwind`。

## 转交 / 推迟（DEFER）

- `lsp_generated.rs` 全量生成 → `// DEFER(P8-后期/生成器就绪) / blocked-by: generate.mts→Rust emit`。先手抄测试覆盖子集。
- 大部分 `server_*_test.go` 端到端 → `// DEFER / blocked-by: tsgo_project + tsgo_ls + tsgo_api green`。
- `TestReplay`（replay_test.go）依赖 `testutil/lsptestutil` → **P10**（端到端 replay parity）。`// DEFER(P10)`。
- **跨 phase 依赖警告**：`tsgo_lsproto` 被 P7 的 `ls`（lsconv/lsutil）依赖，故 `lsproto` crate 实际上需在 P7 之前（或与之并行）就绪。本 phase 文档把 lsproto 归在 lsp 下，但 crate 必须独立、且其落地排期早于 ls。见 README「装配关系」与「存疑/偏离」。
