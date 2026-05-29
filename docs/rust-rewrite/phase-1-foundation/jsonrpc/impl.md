# jsonrpc: 实现方案（impl.md）

> 写前已实读 `internal/jsonrpc/*.go`（2 个非测试文件，全部读完）。所有 TODO 带 `// Go:` 锚点。

**crate**：`tsgo_jsonrpc`　**目标**：通用 JSON-RPC 2.0 类型 + LSP base 协议（`Content-Length` 帧）编解码，供 `lsproto` / `api` / 其他 JSON-RPC 协议复用。
**依赖（crate）**：`tsgo_json`（镜像 Go `import internal/json`，path 依赖）。
**Go 源**：`internal/jsonrpc/`（2 个非测试文件，约 290 行）
- `jsonrpc.go`（189 行）：JSON-RPC 2.0 消息类型（`ID` / `Message` / `ResponseError` / 版本常量 / 错误码）。
- `baseproto.go`（99 行）：base 协议的 `Reader` / `Writer`（`Content-Length` 帧解析与写出）。

## 这个包是什么（业务说明）

`jsonrpc` 是 Phase 8 最底层的叶子包，提供两类能力：

1. **JSON-RPC 2.0 数据模型**：`JSONRPCVersion`（恒为 `"2.0"` 的零大小类型）、`ID`（string 或 int32 二选一）、`ResponseError`（code/message/data）、`MessageKind`（Notification/Request/Response 三态）、以及把 params/result 保留为 raw JSON 的泛型 `Message`。`lsproto` 在其上叠加 LSP 专有的 `Method` 枚举与具体 params 解码，`api` 在其上做 msgpack/jsonrpc 双协议。
2. **LSP base 协议帧**：`Reader.Read()` 逐行读 header，识别 `Content-Length`，再读对应字节数的 body；`Writer.Write()` 写 `Content-Length: N\r\n\r\n` + body 并 flush。这是 LSP over stdio 的物理层。

它**不**做具体 LSP 方法的参数解码（那是 `lsproto` 的事），只关心"一条消息的边界 + 它是请求/通知/响应中的哪一类"。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `JSONRPCVersion struct{}` + 自定义 Marshal/Unmarshal | `struct JsonRpcVersion;` + serde：`serialize` 恒为 `"2.0"`，`deserialize` 校验等于 `"2.0"` 否则 `ErrInvalidJsonRpcVersion` | 零大小类型；Marshal 直接写字面量 `"2.0"` |
| `ID struct{ str string; int int32 }`（未导出字段，二选一） | `enum Id { Str(String), Int(i32) }` | Go 用「`str==""` 则取 int」的隐式判别；Rust 用枚举显式判别更安全。`MarshalJSON` 据此输出字符串或数字 |
| `IntegerOrString{ Integer *int32; String *string }` | `enum IntegerOrString { Integer(i32), String(String) }`（或保留两 `Option` 字段以贴近 Go 构造点） | `NewID` 的入参辅助类型 |
| `ResponseError{ Code int32; Message string; Data any }` | `struct ResponseError { code: i32, message: String, data: Option<serde_json::Value> }` | `Data` 用 `serde_json::Value`；`omitzero` → `#[serde(skip_serializing_if = "Option::is_none")]` |
| `Message{ JSONRPC; ID *ID; Method string; Params json.Value; Result json.Value; Error *ResponseError }` | `struct Message { jsonrpc, id: Option<Id>, method: String, params: Option<RawValue>, result: Option<RawValue>, error: Option<ResponseError> }` | params/result 保留原始 JSON（`Box<serde_json::value::RawValue>`），由上层据 method 解码 |
| `MessageKind` (iota) | `#[repr(i32)] enum MessageKind { Notification, Request, Response }` | 顺序与 Go iota 一致：0/1/2 |
| `error` 哨兵（`ErrInvalidHeader` 等） | `#[derive(thiserror::Error)] enum BaseProtoError` + `enum RpcError` | 见下「错误」 |
| `bufio.Reader` / `bufio.Writer` | `std::io::BufReader<R>` / `BufWriter<W>` | `Reader.Read` 用 `read_until(b'\n', ..)`；`io.ReadFull` → `Read::read_exact` |
| `*ID` 可空（`TryInt`/`MustInt`） | `Option<Id>` + 方法 `try_int(&self) -> Option<i32>` / `must_int` | `MustInt` 在 str 分支 `panic!` |

### 错误处理（本包关键）

Go 用包级哨兵 error + `fmt.Errorf("%w: ...")` 包裹。Rust 用 `thiserror`：

```rust
#[derive(thiserror::Error, Debug)]
pub enum BaseProtoError {
    #[error("jsonrpc: invalid header: {0:?}")]
    InvalidHeader(Vec<u8>),
    #[error("jsonrpc: invalid content length: {0}")]
    InvalidContentLength(String),
    #[error("jsonrpc: no content length")]
    NoContentLength,
    #[error("jsonrpc: read header: {0}")]
    ReadHeader(#[source] std::io::Error),
    #[error("jsonrpc: read content: {0}")]
    ReadContent(#[source] std::io::Error),
}
```

`Read` 返回 `Result<Vec<u8>, BaseProtoError>`，其中 EOF 单独区分（Go 直接 `return nil, io.EOF`）：用 `Option<Vec<u8>>`（`Ok(None)` 表 EOF）或专门的 `BaseProtoError::Eof`。**测试期望保留 Go 的错误文案**（见 tests.md），故 `Display` 文案需逐字对齐 Go 的 `Errorf` 串。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/jsonrpc/jsonrpc.go` | `internal/jsonrpc/jsonrpc.rs`（= `lib.rs`，crate 根 `tsgo_jsonrpc`） | 数据模型；basename == 目录名 → `lib.rs` |
| `internal/jsonrpc/baseproto.go` | `internal/jsonrpc/baseproto.rs` | `mod baseproto;`，re-export `Reader`/`Writer` |

> 命名：`jsonrpc.go` basename 与目录同名 → 作 `lib.rs`（crate 入口）。`baseproto.go` → `baseproto.rs` 子模块。

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| 序列化 | `serde` / `serde_json` | `Message.Params/Result` 用 `serde_json::value::RawValue`（保留原文，延迟解码） |
| 错误 | `thiserror` | 文案需对齐 Go |

均在 PORTING §10 白名单内，无新增。

## 实现 TODO（逐文件 / 逐函数）

### `jsonrpc.rs`（lib.rs；Go: `internal/jsonrpc/jsonrpc.go`）

- [ ] `pub struct JsonRpcVersion;` + serde：serialize→`"2.0"`，deserialize 校验，错 → `ErrInvalidJsonRpcVersion`　`// Go: jsonrpc.go:JSONRPCVersion.MarshalJSON/UnmarshalJSON`
- [ ] `pub enum Id { Str(String), Int(i32) }`　`// Go: jsonrpc.go:ID`
- [ ] `pub fn Id::new(raw: IntegerOrString) -> Id`　`// Go: jsonrpc.go:NewID`
- [ ] `pub fn Id::new_string(s: String) -> Id` / `new_int(i: i32) -> Id`　`// Go: jsonrpc.go:NewIDString/NewIDInt`
- [ ] `impl Display for Id`（str 优先，否则 itoa）　`// Go: jsonrpc.go:ID.String`
- [ ] `Id` 的 serde Serialize（str→json string，int→json number）/ Deserialize（首字节 `"` → str 否则 int）　`// Go: jsonrpc.go:ID.MarshalJSON/UnmarshalJSON`
- [ ] `pub fn Id::try_int(&self) -> Option<i32>`（nil/str→None）　`// Go: jsonrpc.go:ID.TryInt`
- [ ] `pub fn Id::must_int(&self) -> i32`（str→panic）　`// Go: jsonrpc.go:ID.MustInt`
- [ ] `pub enum IntegerOrString { Integer(i32), String(String) }`　`// Go: jsonrpc.go:IntegerOrString`
- [ ] `pub struct ResponseError { code: i32, message: String, data: Option<Value> }`　`// Go: jsonrpc.go:ResponseError`
- [ ] `impl Display/Error for ResponseError`（含 `String()`：nil→空串；data marshal 失败 → `[code]: msg\n<data>`，否则 `[code]: msg`）　`// Go: jsonrpc.go:ResponseError.String/Error`
- [ ] 错误码常量 `CODE_PARSE_ERROR=-32700 … CODE_INTERNAL_ERROR=-32603`　`// Go: jsonrpc.go:CodeParseError..CodeInternalError`
- [ ] `pub enum MessageKind { Notification, Request, Response }`（repr i32，对齐 iota）　`// Go: jsonrpc.go:MessageKind`
- [ ] `pub struct Message { jsonrpc, id, method, params, result, error }`（params/result = `Option<RawValue>`）　`// Go: jsonrpc.go:Message`
- [ ] `pub fn Message::kind(&self) -> MessageKind`（id!=None&&method==""→Response；id==None→Notification；else Request）　`// Go: jsonrpc.go:Message.Kind`
- [ ] `pub fn Message::is_request/is_notification/is_response(&self) -> bool`　`// Go: jsonrpc.go:Message.IsRequest/IsNotification/IsResponse`
- [ ] `pub struct RequestMessage { jsonrpc, id: Option<Id>, method: String, params: Option<Value> }`　`// Go: jsonrpc.go:RequestMessage`
- [ ] `pub struct ResponseMessage { jsonrpc, id: Option<Id>, result: Option<Value>, error: Option<ResponseError> }`　`// Go: jsonrpc.go:ResponseMessage`

### `baseproto.rs`（Go: `internal/jsonrpc/baseproto.go`）

- [ ] `pub struct Reader<R: BufRead> { r: R }`　`// Go: baseproto.go:Reader`
- [ ] `pub fn Reader::new(r: R) -> Reader`　`// Go: baseproto.go:NewReader`
- [ ] `pub fn Reader::read(&mut self) -> Result<Option<Vec<u8>>, BaseProtoError>`（`Ok(None)`=EOF）。循环读行：`\r\n` 单独行→header 结束；无 `:` → `InvalidHeader`；`Content-Length` 解析（含负值/空值错误文案）；`content_length<=0`→`NoContentLength`；`read_exact` 读 body，不足→`ReadContent(unexpected EOF)`　`// Go: baseproto.go:Reader.Read`
- [ ] `pub struct Writer<W: Write> { w: BufWriter<W> }`　`// Go: baseproto.go:Writer`
- [ ] `pub fn Writer::new(w: W) -> Writer`　`// Go: baseproto.go:NewWriter`
- [ ] `pub fn Writer::write(&mut self, data: &[u8]) -> io::Result<()>`（写 `Content-Length: N\r\n\r\n`+data+flush）　`// Go: baseproto.go:Writer.Write`
- [ ] `pub enum BaseProtoError`（thiserror，文案对齐 Go）

### Cargo / crate 接线

- [ ] `internal/jsonrpc/Cargo.toml`（`name = "tsgo_jsonrpc"`，deps: `tsgo_json` path, `serde`, `serde_json`, `thiserror`）
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/jsonrpc`
- [ ] `lib.rs`（= `jsonrpc.rs`）声明 `mod baseproto;` + re-export `Reader`/`Writer`

## TDD 推进顺序（tracer bullet → 增量）

1. **base 协议帧**（`baseproto.rs`）先行：它有最明确的行为级测试（`baseproto_test.go` 在 `lsproto` 里，但语义就是本包 Reader/Writer）。先实现 `Reader.read` round-trip → 用 spec 报文（见 tests.md）跑通 red→green。
2. **`Id` 编解码**：string/int 双形态 Marshal/Unmarshal round-trip。
3. **`Message.kind()`** 三态判别（纯逻辑，最易测）。
4. **`ResponseError.String()`** 文案。
5. **`JsonRpcVersion`** 校验（拒绝非 `"2.0"`）。

## 与 Go 的已知偏离（divergence）

- `ID` 用 `enum`（显式判别）替代 Go 的「`str==""` 取 int」隐式约定。语义等价，但注意：Go 里 `NewIDInt(0)` 与空 str 不可区分（`String()` 返回 `"0"`），Rust `Id::Int(0)` 明确是整数 0 —— 这其实**更正确**，但 round-trip 时需确认与 Go marshal 一致（int 0 → `0`，Go 同样）。在 impl 顶部注明。
- `Read` 的 EOF：Go 用 `(nil, io.EOF)` 双值；Rust 用 `Ok(None)` 表示干净 EOF，避免把 EOF 当错误。`// PERF(port)` 非性能问题，是惯用法差异。
- `bufio` 缓冲：Go 默认缓冲大小与 Rust `BufReader` 默认（8KB）不同，但不影响正确性（仅性能/系统调用次数）。

## 转交 / 推迟（DEFER）

- 无下游 phase 阻塞项。本包是叶子，`tsgo_json`（P1）须先就绪。
- `Message.Params/Result` 的具体类型解码不在本包（在 `tsgo_lsproto` / `tsgo_api`）。
