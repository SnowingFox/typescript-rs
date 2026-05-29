# api: 实现方案（impl.md）

> 写前已实读 `internal/api/` 全部非测试 `.go`（14 个顶层 + `encoder/` 5 个 = 19 个；其中 `proto.go` 38KB、`session.go` 65KB 按公开 API 表面 + 分发路径精读，`encoder.go`/`decoder.go` 按二进制布局 + 公开函数精读）。所有 TODO 带 `// Go:` 锚点。

**crate**：`tsgo_api`（+ 子 crate `tsgo_api_encoder`）
**目标**：typescript-go 的**对外编程 API 层**。在 `tsgo_project` 会话之上暴露一套 RPC（JSON-RPC 或自定义 msgpack）接口，让外部工具（如 TS 原生工具链、native 集成）查询 program/symbol/type/signature/AST，并支持把文件系统操作回调给客户端（virtual FS）。还提供把 AST 序列化为紧凑二进制格式（`encoder/`）以零拷贝传给 JS 客户端。
**依赖（crate）**：`tsgo_project` `tsgo_jsonrpc` `tsgo_json` `tsgo_lsproto` `tsgo_ast` `tsgo_checker` `tsgo_compiler` `tsgo_core` `tsgo_tspath` `tsgo_vfs`（含 `vfs/osvfs`）`tsgo_bundled` `tsgo_collections` + `tsgo_api_encoder`（+ `tsgo_parser` `tsgo_repo` 测试用）。Windows pipe：`go-winio` → Rust `interprocess`/`tokio`(不引)→用 `std::os::windows` named pipe 或 `interprocess` crate（仅 transport）。

## 这个包是什么（业务说明）

`api` 把 `project.Session` 的"工程/快照"能力，封装成一套**面向 compiler-as-a-service 的 RPC**：

1. **协议层（双协议）**：`Protocol` trait 抽象读写消息。两种实现：`JSONRPCProtocol`（JSON-RPC 2.0 + LSP `Content-Length` 帧，复用 `tsgo_jsonrpc`）和 `MessagePackProtocol`（自定义最小 msgpack：`[MessageType, method, payload]` 三元组，`fixarray3`+`bin8/16/32`+`fixint/u8`）。msgpack 用 method 名当伪 ID。
2. **连接层**：`Conn` trait（`Run`/`Call`/`Notify`）。`AsyncConn`（每请求一 goroutine 并发 + server→client `Call` 的 pending 路由 + writeMu 串行写）；`SyncConn`（单 mutex 串行处理，Call 内联读响应——因 msgpack 用 method 当 ID）。
3. **服务层**：`StdioServer`（按 `Async`/`PipePath` 选协议与 transport，建 `project.Session` + `api.Session`，可挂 `callbackFS`）。`Transport`：`PipeTransport`（Unix socket / Windows named pipe）/`StdioTransport`（stdin/stdout 单连接）。
4. **会话/分发（session.go，核心）**：`api.Session` 实现 `Handler`，`HandleRequest` 是 ~70 个方法的大 switch（initialize / updateSnapshot / release / getDefaultProjectForFile / parseConfigFile / getSourceFile / getSymbol* / getType* / getSignature* / typeToTypeNode / printNode / get*Diagnostics …）。它把请求参数里的 **Handle**（项目/符号/类型/签名/节点）解析回对象，调 checker/program，再把结果对象注册成新 Handle 返回。`snapshotData` 是每快照的 handle 注册表（含 mutex + checker 池 setup）。
5. **callbackFS（callbackfs.go）**：包装底层 vfs，把 `readFile`/`fileExists`/`directoryExists`/`getAccessibleEntries`/`realpath` 经 `Conn.Call` 反向委托给客户端（让客户端提供虚拟 FS）。
6. **二进制 AST 编解码（encoder/）**：`EncodeSourceFile`/`DecodeSourceFile` 把 AST 编成扁平 LE-uint32 节点数组 + 字符串表（`stringTable` 尽量复用源文本切片），头部含 hash/parseOptions/各段偏移。供 JS 客户端零拷贝读取 AST。

它在 P8 末位是因为它装配了本 phase 的 `project` 与协议依赖，是 LSP server（`lsp.handleInitializeAPISession`）与独立 stdio 工具的共同后端。

## 所有权 / 类型映射（本包关键决策）

### 协议序列化决策（与 lsp 一致 + 本包补充）

- **JSON-RPC**：复用 `tsgo_jsonrpc` + `serde`/`serde_json`（`Message`/`RequestMessage`/`ResponseMessage`）。
- **msgpack**：**手写最小子集**（不引 `rmp`/`rmp-serde`）。理由：Go 侧只发/收一个**特定子集**（恒 `0x93` 三元组、type 用 fixint 写但读时容忍 `0xCC`、payload 用 `bin8/16/32`）。通用 msgpack 库可能选不同编码（如 type 用别的整型 marker、payload 用 str 而非 bin），导致与 JS 客户端字节级不符。手写 `MessagePackProtocol` 逐字节对齐 Go。
- **二进制 AST 格式**：`encoder/` 自定义格式，**字节级 parity**（baseline 测试）。Rust 用 `Vec<u8>` + 显式 LE 写（`u32::to_le_bytes`），布局常量（`NodeSize`/`NodeOffset*`/`HeaderOffset*`/`NodeDataType*` 位掩码）逐一对齐。

### Handle —— 指针 → id（关键偏离）

Go 的 `Handle[T] string` 把对象编码成字符串句柄：`ProjectHandle`=项目路径；`SymbolHandle`/`TypeHandle`/`NodeHandleFrom` 把**指针/位置/kind** 编进字符串（如 node handle = `pos:end:kind:path`，symbol/type handle 含指针地址或注册 id）。

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Handle[T] string` | `struct Handle<T>(String, PhantomData<T>)` 或 `String` newtype per 类型 | 句柄是字符串（线缆友好） |
| `SymbolHandle(*ast.Symbol)` / `TypeHandle(*checker.Type)`（含指针） | 经 `snapshotData` 注册表把对象 → **id**（arena/序号），handle 编 id 而非指针 | **偏离**：Rust 无裸指针句柄；`snapshotData` 维护 `id→对象` 映射，handle 用 id。PORTING §5 |
| `SignatureHandle(id uint64)` | 已是 id，直接用 | 签名本就用注册 id |
| `NodeHandleFrom(node)` = `pos:end:kind:path` | 同构编码（pos/end/kind/path），`parseNodeHandle` 反解 | 节点用位置+kind+文件路径定位，无指针 → 可直译 |
| `parseProjectHandle` = 路径 | 直译 | |

`snapshotData{ mu; symbols/types/signatures 注册表; checker setup }` → Rust `struct SnapshotData { mu: Mutex<Registries>, ... }`，`registerSymbol/registerType/registerSignature/resolve*Handle` 用 id。

### 连接并发模型

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `AsyncConn`：`go c.handleRequest`（每请求一 goroutine）+ `pending map[ID]chan` + `writeMu` | `std::thread::spawn` 或 worker 池 + `Mutex<FxHashMap<Id, Sender<Message>>>` + `Mutex` writeMu | server→client `Call` 的回包路由 |
| `SyncConn`：单 `mu` 串行；`Call` 内联读响应 | `Mutex` 串行；`Call` 持锁写后读 | msgpack 用 method 当 ID，必须读写成对原子 |
| `seq atomic.Int64`（Call ID） | `AtomicI64` | `api%d` |
| panic recovery（handleRequest defer recover → WriteError） | `std::panic::catch_unwind` → WriteError | 含 stack |
| `context.Context` | 显式 `&Cancel` | |

### 其它

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `MessageType`(iota) + stringer | `#[repr(u8)] enum MessageType` + `Display`（名表）；`IsValid` | stringer → Display |
| `Message = jsonrpc.Message`（别名） | `type Message = jsonrpc::Message;` | |
| `DocumentIdentifier`（string 或 `{uri}` 对象，自定义 Unmarshal） | `enum DocumentIdentifier { FileName(String), Uri(DocumentUri) }` + 自定义 Deserialize（string→FileName，object 读 `uri`，否则空；非法类型→`"expected string or object, got <kind>"`） | 测试覆盖 |
| `RawBinary []byte`（msgpack 直写标记） | `struct RawBinary(Vec<u8>)` | WriteResponse 走直写分支 |
| `Protocol`/`Conn`/`Handler`/`Transport` interface | trait | |
| `go-winio` named pipe | `interprocess` crate 或 `std::os::windows::named_pipe`（仅 transport）；Unix 用 `std::os::unix::net::UnixListener` | 平台分文件（cfg） |
| `stringTable`（复用 fileText 切片 + 其它串 builder） | `struct StringTable{ file_text: &str, other: String, offsets: Vec<u32> }` | encoder 核心 |

## 文件清单 → Rust 模块

### crate `tsgo_api`（`internal/api/`，14 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `session.go` | `internal/api/session.rs`（lib.rs，crate 根） | `api.Session`（impl Handler）+ ~70 handler + snapshotData + handle 解析 + 快照 diff。basename≠目录名→lib.rs |
| `proto.go` | `internal/api/proto.rs` | 全部协议类型：`Method`/`Handle`/`DocumentIdentifier`/`*Params`/`*Response`/构造器/`literalValueToJSON` |
| `protocol.go` | `internal/api/protocol.rs` | `Protocol` trait + `Message` 别名 |
| `protocol_jsonrpc.go` | `internal/api/protocol_jsonrpc.rs` | `JSONRPCProtocol`（Read/WriteRequest/Notification/Response/Error） |
| `protocol_msgpack.go` | `internal/api/protocol_msgpack.rs` | `MessagePackProtocol`/`MessageType`/`RawBinary`/tuple+bin 读写 |
| `conn.go` | `internal/api/conn.rs` | `Conn`/`Handler` trait + `UnmarshalParams` + 错误常量 |
| `conn_async.go` | `internal/api/conn_async.rs` | `AsyncConn`（每请求 goroutine + pending 路由） |
| `conn_sync.go` | `internal/api/conn_sync.rs` | `SyncConn`（串行 + 内联读响应） |
| `server.go` | `internal/api/server.rs` | `StdioServer`/`StdioServerOptions` |
| `transport.go` | `internal/api/transport.rs` | `Transport`/`PipeTransport`/`StdioTransport`/`stdioConn` |
| `transport_unix.go` | `internal/api/transport_unix.rs`（`#[cfg(unix)]`） | `new_pipe_listener`（UnixListener）/`generate_pipe_path`（TempDir） |
| `transport_windows.go` | `internal/api/transport_windows.rs`（`#[cfg(windows)]`） | named pipe listener / `\\.\pipe\` 路径 |
| `callbackfs.go` | `internal/api/callbackfs.rs` | `callbackFS`（impl vfs.FS，委托回调） |
| `stringer_generated.go` | （并入 `protocol_msgpack.rs` 的 `Display`） | MessageType stringer |

### 子 crate `tsgo_api_encoder`（`internal/api/encoder/`，5 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `encoder.go` | `internal/api/encoder/encoder.rs`（lib.rs） | 布局常量 + `EncodeSourceFile`/`EncodeNode`/`SourceFileHash` + 节点编码 |
| `decoder.go` | `internal/api/encoder/decoder.rs` | `DecodeSourceFile`/`DecodeNodes`/`astDecoder`/`childIterator` |
| `stringtable.go` | `internal/api/encoder/stringtable.rs` | `stringTable`（add/encode/length，复用源文本切片） |
| `encoder_generated.go` | `internal/api/encoder/encoder_generated.rs`（机器生成） | 各 AST 节点的字段编码（kind→data） |
| `decoder_generated.go` | `internal/api/encoder/decoder_generated.rs`（机器生成） | 各 AST 节点的字段解码 |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| Windows named pipe / Unix socket | `interprocess`（跨平台）或 std（unix）+ `interprocess`/winapi（win） | 仅 transport；先 `// TODO(port)` 决定 |
| 序列化 | `serde`/`serde_json` | JSON-RPC + 各 Params/Response |
| 临时目录 | std `env::temp_dir` | `GeneratePipePath` |

`interprocess` 为本 phase 新增（若选用），追加到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，节选关键项）

### `tsgo_api_encoder`（先行，纯函数 + baseline 可测）

- [ ] `stringtable.rs`：`StringTable::{new,add,encode,string_length,encoded_length}`（KindSourceFile 特例；字符串字面量/模板尾去引号偏移；切片复用 vs 追加）　`// Go: encoder/stringtable.go:*`
- [ ] `encoder.rs`：布局常量（`NodeOffset*`/`NodeSize`/`NodeDataType*`/`HeaderOffset*`/位掩码）+ `EncodeSourceFile`/`EncodeNode`/`SourceFileHash`/节点遍历与 next 指针回填　`// Go: encoder/encoder.go:*`
- [ ] `encoder_generated.rs`：逐 AST kind 的字段→data 编码　`// Go: encoder/encoder_generated.go`
- [ ] `decoder.rs`：`DecodeSourceFile`/`DecodeNodes`/`astDecoder`/`childIterator`　`// Go: encoder/decoder.go:*`
- [ ] `decoder_generated.rs`：逐 AST kind 的 data→字段解码　`// Go: encoder/decoder_generated.go`

### `tsgo_api`：协议/连接/传输（中层，行为可测）

- [ ] `protocol.rs`：`Protocol` trait + `Message` 别名　`// Go: protocol.go:*`
- [ ] `protocol_jsonrpc.rs`：`JSONRPCProtocol::{new,read_message,write_request,write_notification,write_response,write_error}`（result==nil→`null`）　`// Go: protocol_jsonrpc.go:*`
- [ ] `protocol_msgpack.rs`：`MessageType`+`IsValid`+`Display`/`MessagePackProtocol::{read_message,read_tuple,read_bin,write_request,write_notification,write_response,write_error,write_tuple,write_bin}`/`RawBinary`（fixint/u8 type、bin8/16/32 size、错误文案对齐）　`// Go: protocol_msgpack.go:*`
- [ ] `conn.rs`：`Conn`/`Handler` trait + `unmarshal_params` + `ErrConnClosed`/`ErrRequestTimeout`　`// Go: conn.go:*`
- [ ] `conn_async.rs`：`AsyncConn::{new,with_protocol,run,handle_response,handle_request,handle_notification,call,notify}`（panic recover→WriteError；pending 路由；seq）　`// Go: conn_async.go:*`（**并发**）
- [ ] `conn_sync.rs`：`SyncConn::{new,run,handle_request,handle_notification,call,notify}`（单 mutex；Call 内联读；意外响应报错）　`// Go: conn_sync.go:*`（**并发**）
- [ ] `transport.rs`：`Transport` trait/`PipeTransport::{new,accept,close,path}`/`StdioTransport::{new,accept,close}`/`stdioConn`　`// Go: transport.go:*`
- [ ] `transport_unix.rs`/`transport_windows.rs`：`new_pipe_listener`/`generate_pipe_path`（cfg 分平台）　`// Go: transport_unix.go / transport_windows.go`
- [ ] `server.rs`：`StdioServer::{new,run}`/`StdioServerOptions`（按 Async/PipePath 选协议+transport，建 project.Session + api.Session，挂 callbackFS）　`// Go: server.go:*`
- [ ] `callbackfs.rs`：`callbackFS`（impl vfs.FS：ReadFile/FileExists/DirectoryExists/GetAccessibleEntries/Realpath 走回调，其余委托 base）+ `isCallbackName`/`newCallbackFS`/`SetConnection`/`call`　`// Go: callbackfs.go:*`

### `tsgo_api`：协议类型（proto.rs）

- [ ] `Method`/`Handle<T>` + `ProjectHandle`/`SymbolHandle`/`TypeHandle`/`SignatureHandle`/`NodeHandleFrom`/`parseNodeHandle`/`parseProjectHandle`/`createHandle`（指针→id 偏离）　`// Go: proto.go:*`
- [ ] `DocumentIdentifier`（自定义 Deserialize：string/object/错误文案）+ `ToFileName`/`ToURI`/`ToAbsoluteFileName`/`String`　`// Go: proto.go:DocumentIdentifier.*`
- [ ] 全部 `*Params`/`*Response` 结构（InitializeResponse/UpdateSnapshot*/SnapshotChanges/ProjectFileChanges/ConfigFileResponse/ProjectResponse/SymbolResponse/TypeResponse/SignatureResponse/IndexInfoResponse/TypePredicateResponse/DiagnosticResponse + 各 Get*Params …）　`// Go: proto.go:*`
- [ ] 构造器：`NewProjectResponse`/`NewSymbolResponse`/`newTypeData`/`typeHandles`/`literalValueToJSON`　`// Go: proto.go:*`

### `tsgo_api`：会话/分发（session.rs，最后）

- [ ] `Session`/`SessionOptions`/`NewSession`/`ID`/`ProjectSession`/`Close` + `formatSessionID`/`sessionIDCounter`　`// Go: session.go:*`
- [ ] `snapshotData::{getProgram,registerSymbol,registerType,registerSignature,resolveSymbolHandle,resolveTypeHandle,resolveSignatureHandle}` + `snapshotHandle`/`getSnapshotData`　`// Go: session.go:*`（**并发**：mu）
- [ ] `checkerSetup`/`setupChecker`/`resolveNodeHandle`/`toAbsoluteFileName`/`toPath`/`toFileChangeSummary`/`computeSnapshotChanges`　`// Go: session.go:*`
- [ ] `HandleRequest`（~70 方法大 switch）+ `HandleNotification`　`// Go: session.go:HandleRequest/HandleNotification`
- [ ] 全部 `handle*`：initialize/updateSnapshot/release/getDefaultProjectForFile/parseConfigFile/getSourceFile/getSymbol*(4)/getType*(多)/resolveName/getParent/Members/Exports/ExportSymbol/SymbolOfType/Signatures/ResolvedSignature/TypeAt*/各 type-property getter/Contextual/NonNullable/FromTypeNode/Widened/ParameterType/IsArrayLike/typeToTypeNode/signatureToSignatureDeclaration/typeToString/printNode/getIntrinsicType/isContextSensitive/各 signature getter/baseTypes/propertiesOfType/indexInfos/constraintOfTypeParameter/typeArguments/get*Diagnostics　`// Go: session.go:handle*`

### Cargo / crate 接线

- [ ] `internal/api/Cargo.toml`（`tsgo_api`）+ `internal/api/encoder/Cargo.toml`（`tsgo_api_encoder`）
- [ ] 根 workspace members 追加两目录
- [ ] lib.rs 声明子模块 + re-export `Session`/`StdioServer`/`NewAsyncConn`/`NewPipeTransport`/`GeneratePipePath` 等（被 lsp 用）

## TDD 推进顺序（tracer bullet → 增量）

1. **encoder**（叶子，纯函数 + baseline）：`stringtable` → `encoder.EncodeSourceFile`（`encoder_test.go` 2 个 baseline）→ `decoder.DecodeSourceFile`（`decoder_test.go` 21 个，逐 AST 构造 round-trip）。
2. **proto 的 `DocumentIdentifier`**（`proto_test.go` 5 子用例，纯解码）。
3. **协议帧**：`JSONRPCProtocol`（复用 jsonrpc 已测）+ `MessagePackProtocol`（行为级 round-trip，用已知三元组报文）。
4. **连接**：`AsyncConn`/`SyncConn`（行为级：请求/响应/通知/Call 路由，用内存 pipe）。
5. **transport/callbackfs/server**：集成（依赖 project，多数 P8 末尾/P10）。
6. **session 分发**：依赖 checker/program 全就绪 → P8 末尾 + P10 parity。

## 与 Go 的已知偏离（divergence）

- **Handle 指针 → id**：`SymbolHandle`/`TypeHandle` 在 Go 编码指针；Rust 用 `snapshotData` 注册表 id（无裸指针句柄）。node handle（pos:end:kind:path）可直译。
- **msgpack 手写**：不引通用 msgpack 库，逐字节对齐 Go 子集（见协议决策）。
- **二进制 AST 格式字节级 parity**：encoder/decoder 布局常量逐一对齐，baseline 对拍（含 unicode escape 用例 `\ud83d\ude03` 等）。
- **`go-winio` → `interprocess`/std**：Windows named pipe 用第三方或 std；行为对齐 `\\.\pipe\name`，Unix 用 `UnixListener` + TempDir 路径。
- **panic recover → `catch_unwind`**：handleRequest 的 panic→错误响应（含 stack）。
- **`context.Context` → 显式取消**；server→client `Call` 的 pending 路由用 `Mutex<HashMap<Id, Sender>>`。
- **`runtime/debug.Stack()`** → Rust `std::backtrace::Backtrace`（panic 响应里的 stack 文案不要求逐字一致，但需可读）。

## 转交 / 推迟（DEFER）

- `session.rs` 的 ~70 handler 依赖 `tsgo_checker`/`tsgo_compiler`/`tsgo_ls` 全就绪 → 多数 `// DEFER / blocked-by: checker+compiler green`，但骨架（dispatch + handle 解析 + snapshotData）可在 P8 内搭。
- transport/server/callbackfs 的端到端集成依赖 `project.Session` + 真实 pipe → P8 末尾/P10。
- `encoder_generated.rs`/`decoder_generated.rs` 全量节点字段 → 随 AST（P2/P3）类型就绪逐步生成；先手抄 decoder_test 覆盖的节点种类。`// DEFER(生成器)`。
