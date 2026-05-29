# jsonrpc: 测试清单（tests.md）

> Go 侧 `internal/jsonrpc/` **无 `*_test.go`**（0 直接单测）。
> 但 `internal/lsp/lsproto/baseproto_test.go` 通过 `BaseReader`/`BaseWriter`（仅是 `jsonrpc.Reader`/`Writer` 的零开销包装）**直接测试了本包的 base 协议帧逻辑** —— 这些用例就是 `jsonrpc` 行为的 ground truth，故全部复用为本包行为级测试。
> 其余（`ID`/`Message`/`ResponseError`/`JSONRPCVersion`）Go 无单测 → 用 JSON-RPC 2.0 spec 已知报文补行为级 round-trip 测试。

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：本包 0 文件 / 0 顶层函数（行为由 `lsproto/baseproto_test.go` 间接覆盖 + 本轮补行为级测试 + P10 端到端 parity 兜底）。

## 0 直接单测的情况

- Go 侧 `internal/jsonrpc` 无直接单测；端到端行为由 **P10 conformance / LSP replay parity** 兜底。
- base 协议帧（`Reader`/`Writer`）的行为由 `lsproto/baseproto_test.go` 完整覆盖（见 `../lsp/tests.md` 同名小节）；本包测试**直接对 `jsonrpc::Reader`/`Writer` 复刻这些用例**，expected 取自 Go 测试字面量。

### 行为级测试 A：base 协议 `Reader.read()`（复刻 `baseproto_test.go:TestBaseReader`）

| Rust 测试 | 验证内容 | input → expected | 依据 (Go) | 完成 |
|---|---|---|---|---|
| `read_empty_zero_length` | `Content-Length: 0` → `NoContentLength` 错误 | `"Content-Length: 0\r\n\r\n"` → err `"jsonrpc: no content length"` | `baseproto_test.go:TestBaseReader/empty` | |
| `read_early_end` | 提前 EOF | `"oops"` → err `"EOF"` | `.../early end` | |
| `read_negative_length` | 负 content-length | `"Content-Length: -1\r\n\r\n"` → err `"jsonrpc: invalid content length: negative value -1"` | `.../negative length` | |
| `read_invalid_content_one_byte` | 长度 1 读到 `{` | `"Content-Length: 1\r\n\r\n{"` → `b"{"` | `.../invalid content` | |
| `read_valid_content` | 标准 body | `"Content-Length: 2\r\n\r\n{}"` → `b"{}"` | `.../valid content` | |
| `read_extra_header_values` | 多余 header 被忽略 | `"Content-Length: 2\r\nExtra: 1\r\n\r\n{}"` → `b"{}"` | `.../extra header values` | |
| `read_too_long_content_length` | 声明长度超过实体 | `"Content-Length: 100\r\n\r\n{}"` → err `"jsonrpc: read content: unexpected EOF"` | `.../too long content length` | |
| `read_missing_content_length_value` | 空数值 | `"Content-Length: \r\n\r\n{}"` → err 含 `"jsonrpc: invalid content length: parse error: ...invalid syntax"` | `.../missing content length` | |
| `read_invalid_header` | 无冒号的 header 行 | `"Nope\r\n\r\n{}"` → err `"jsonrpc: invalid header: \"Nope\\r\\n\""` | `.../invalid header` | |

### 行为级测试 B：`Reader.read()` 连读多条（复刻 `TestBaseReaderMultipleReads`）

| Rust 测试 | 验证内容 | input → expected | 依据 (Go) | 完成 |
|---|---|---|---|---|
| `read_multiple_messages` | 一个流连续读两条后 EOF | `"Content-Length: 4\r\n\r\n1234Content-Length: 2\r\n\r\n{}"` → `b"1234"`, `b"{}"`, 再读 `EOF` | `baseproto_test.go:TestBaseReaderMultipleReads` | |

### 行为级测试 C：base 协议 `Writer.write()`（复刻 `TestBaseWriter` / `TestBaseWriterWriteError`）

| Rust 测试 | 验证内容 | input → expected | 依据 (Go) | 完成 |
|---|---|---|---|---|
| `write_empty_object` | 写 `{}` | `b"{}"` → `"Content-Length: 2\r\n\r\n{}"` | `baseproto_test.go:TestBaseWriter/empty` | |
| `write_bigger_object` | 写带键值对象 | `b"{\"key\":\"value\"}"` → `"Content-Length: 15\r\n\r\n{\"key\":\"value\"}"` | `.../bigger object` | |
| `write_propagates_io_error` | 底层 writer 报错被透传 | writer 总是 `Err("test error")` → `write` 返回该错误 | `baseproto_test.go:TestBaseWriterWriteError` | |

### 行为级测试 D：`Id` 编解码 round-trip（spec 已知值）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `id_int_marshal` | 整数 ID 序列化 | `Id::Int(7)` → `"7"` | JSON-RPC 2.0 spec（id 可为 number） | |
| `id_string_marshal` | 字符串 ID 序列化 | `Id::Str("ts1")` → `"\"ts1\""` | spec（id 可为 string）；对齐 `server.go` 用的 `"ts%d"` | |
| `id_int_unmarshal` | `7` → `Id::Int(7)` | `b"7"` → `Id::Int(7)` | spec | |
| `id_string_unmarshal` | `"abc"` → `Id::Str` | `b"\"abc\""` → `Id::Str("abc")` | spec（首字节 `"` 判别） | |
| `id_try_int_on_string_is_none` | str ID `try_int` 返回 None | `Id::Str("x").try_int()` → `None` | `jsonrpc.go:ID.TryInt` | |
| `id_must_int_on_string_panics` | str ID `must_int` panic | `Id::Str("x").must_int()` → panic `"ID is not an integer"` | `jsonrpc.go:ID.MustInt` | |
| `id_display_int` | `Display` 输出 | `Id::Int(42)` → `"42"` | `jsonrpc.go:ID.String` | |

### 行为级测试 E：`Message.kind()` 三态判别（spec 语义）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `kind_request` | 有 id + 有 method | `{id:Int(1), method:"x"}` → `Request` | `jsonrpc.go:Message.Kind` | |
| `kind_notification` | 无 id + 有 method | `{id:None, method:"x"}` → `Notification` | 同上 | |
| `kind_response` | 有 id + 空 method | `{id:Int(1), method:""}` → `Response` | 同上 | |
| `is_request_requires_id_and_method` | `is_request` 仅在 id+method 都有时 true | 各组合 → bool | `jsonrpc.go:Message.IsRequest` | |

### 行为级测试 F：`JsonRpcVersion` 校验

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `version_marshal_is_2_0` | 序列化恒为 `"2.0"` | `JsonRpcVersion` → `"\"2.0\""` | `jsonrpc.go:JSONRPCVersion.MarshalJSON` | |
| `version_unmarshal_accepts_2_0` | 接受 `"2.0"` | `b"\"2.0\""` → Ok | `jsonrpc.go:JSONRPCVersion.UnmarshalJSON` | |
| `version_unmarshal_rejects_other` | 拒绝其它版本 | `b"\"1.0\""` → `ErrInvalidJsonRpcVersion` | 同上 | |

### 行为级测试 G：`ResponseError.to_string()`

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `response_error_string_basic` | 无 data | `{code:-32601, message:"Method not found"}` → `"[-32601]: Method not found"` | `jsonrpc.go:ResponseError.String` | |
| `response_error_string_nil` | nil 指针 → 空串（Rust：`Option<&ResponseError>::None`） | `None` → `""` | 同上 | |

## 与 impl.md 的对齐核对

- [x] base 协议 Reader/Writer 的每条 `baseproto_test.go` 子用例均映射（A/B/C）
- [x] `Id` / `Message` / `JsonRpcVersion` / `ResponseError` 的 impl TODO 均有对应行为级测试（D/E/F/G）
- [x] expected 值：A/B/C 取自 Go 测试字面量；D~G 取自 spec / Go 实现逻辑
- [x] 每条带 `// Go:` 锚点（依据列）
- [x] 与 impl.md 双向对齐：impl.md 每个公开函数都有承载测试

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 真实 LSP 流式 round-trip（多消息 + 取消 + 响应交错） | 需 `lsp.Server` 装配 | P10 LSP replay parity |
| `Message.Params` 据 method 的具体解码 | 属 `tsgo_lsproto` 职责 | 见 `../lsp/tests.md` |
