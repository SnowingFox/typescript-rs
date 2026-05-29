# api: 测试清单（tests.md）

> 已实读全部 4 个 `*_test.go`（`proto_test.go` 1、`encoder/encoder_test.go` 2+1bench、`encoder/decoder_test.go` 21+1bench、`encoder/testmain_test.go` 1）。
> **完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
> **Go 测试规模**：4 文件 / 25 顶层 `func Test*`（含 1 `TestMain`）+ 2 `func Benchmark*`（= 27，与速查表一致）/ 约 30 子用例。
> crate 拆分：`tsgo_api`（协议/连接/会话）+ `tsgo_api_encoder`（二进制 AST 编解码）。
> 注意：`api.Session` 的 ~70 handler **无直接单测**（行为由 P10 conformance/集成兜底）；本轮补行为级测试设计（见末节）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | crate | 顶层函数 |
|---|---|---|---|
| `proto_test.go` | `proto.rs` tests | `tsgo_api` | 1 |
| `encoder/encoder_test.go` | `encoder.rs` tests（baseline 对拍） | `tsgo_api_encoder` | 2 (+1 bench) |
| `encoder/decoder_test.go` | `decoder.rs` tests | `tsgo_api_encoder` | 21 (+1 bench) |
| `encoder/testmain_test.go` | harness | — | 1 (TestMain) |

---

## `proto_test.go` → `TestDocumentIdentifierUnmarshalJSON`（表驱动，5 子用例）

> `DocumentIdentifier` 自定义解码：plain string → FileName；`{uri:...}` 对象 → URI（忽略未知字段）；空对象 → 都空；非法类型报错。

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `doc_id_plain_string` | `"foo.ts"` → FileName="foo.ts", URI="" | `.../plain string` | |
| `doc_id_uri_object` | `{"uri":"file:///foo.ts"}` → URI="file:///foo.ts" | `.../uri object` | |
| `doc_id_uri_object_unknown_fields` | `{"uri":"file:///foo.ts","extra":true}` → URI="file:///foo.ts"（忽略 extra） | `.../uri object with unknown fields` | |
| `doc_id_empty_object` | `{}` → 都空 | `.../empty object` | |
| `doc_id_invalid_type` | `42` → err `"expected string or object, got number"` | `.../invalid type` | |

---

## `encoder/encoder_test.go`（baseline 对拍，2 函数 + 1 bench）

> 把源文件解析成 AST → `EncodeSourceFile` → 用 `formatEncodedSourceFile`（按 NodeSize/各 offset 解析二进制）格式化 → baseline 文件比对。Rust 用快照测试（baseline `.txt`），expected = Go baseline 内容，**字节级 parity**。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `encode_source_file_baseline` | 基本 TS 文件编码（import/export/function/泛型/调用） | `import { bar } from "bar";\nexport function foo<T, U>(a,b):any{}\nfoo();` → baseline `api/encodeSourceFile.txt` | `TestEncodeSourceFile/baseline` | |
| `encode_source_file_unicode_escapes` | 含 emoji/代理对转义的字符串字面量编码 | `let a="😃"; let b="\ud83d\ude03"; ...` → baseline `api/encodeSourceFileWithUnicodeEscapes.txt` | `TestEncodeSourceFileWithUnicodeEscapes/baseline` | |
| （bench）`encode_source_file_bench` | checker.ts 编码性能 | — | `BenchmarkEncodeSourceFile`（`#[bench]`/criterion，可选） | —(perf) |

---

## `encoder/decoder_test.go`（21 函数 + 1 bench）

> 模式：源码 → `parser.ParseSourceFile` → `EncodeSourceFile` → `DecodeSourceFile` → 断言解码 AST 的结构/kind/位置/flags/字符串与原始一致（round-trip parity）。`DecodeNodes` 测子树。

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `decode_basic` | 基本源文件 round-trip | `TestDecodeSourceFile_Basic` | |
| `decode_statements` | 语句列表 | `TestDecodeSourceFile_Statements` | |
| `decode_variable_declaration` | 变量声明 | `TestDecodeSourceFile_VariableDeclaration` | |
| `decode_variable_declaration_list_flags` | 声明列表 flags（const/let/var） | `TestDecodeSourceFile_VariableDeclarationListFlags` | |
| `decode_function_declaration` | 函数声明 | `TestDecodeSourceFile_FunctionDeclaration` | |
| `decode_import_declaration` | import 声明 | `TestDecodeSourceFile_ImportDeclaration` | |
| `decode_if_statement` | if 语句 | `TestDecodeSourceFile_IfStatement` | |
| `decode_template_expression` | 模板表达式 | `TestDecodeSourceFile_TemplateExpression` | |
| `decode_export_modifier` | export 修饰符 | `TestDecodeSourceFile_ExportModifier` | |
| `decode_positions` | 节点 pos/end 位置 | `TestDecodeSourceFile_Positions` | |
| `decode_class_declaration` | 类声明 | `TestDecodeSourceFile_ClassDeclaration` | |
| `decode_nodes_subtree_round_trip` | `DecodeNodes` 子树 round-trip | `TestDecodeNodes_SubtreeRoundTrip` | |
| `decode_binary_expression` | 二元表达式 | `TestDecodeSourceFile_BinaryExpression` | |
| `decode_keyword_expressions` | 关键字表达式（true/false/null/this 等） | `TestDecodeSourceFile_KeywordExpressions` | |
| `decode_empty_module_block` | 空 module block | `TestDecodeSourceFile_EmptyModuleBlock` | |
| `decode_empty_block_and_params` | 空 block / 空参数 | `TestDecodeSourceFile_EmptyBlockAndParams` | |
| `decode_arrow_function_empty_params` | 箭头函数空参 | `TestDecodeSourceFile_ArrowFunctionEmptyParams` | |
| `decode_function_expression_empty_params` | 函数表达式空参 | `TestDecodeSourceFile_FunctionExpressionEmptyParams` | |
| `decode_postfix_unary_operator` | 后缀一元运算符 | `TestDecodeSourceFile_PostfixUnaryOperator` | |
| `decode_prefix_unary_operator` | 前缀一元运算符 | `TestDecodeSourceFile_PrefixUnaryOperator` | |
| `decode_postfix_decrement` | 后缀 `--` | `TestDecodeSourceFile_PostfixDecrement` | |
| （bench）`decode_source_file_bench` | checker.ts 解码性能 | `BenchmarkDecodeSourceFile`（可选） | —(perf) |

### `encoder/testmain_test.go`

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| （harness：`ApplyDebugStackLimit` + baseline `Track`） | 无业务断言 | `TestMain` | —(harness) |

---

## 0 直接单测的部分（补行为级测试设计）

`api.Session` 的 ~70 个 `handle*` 与协议/连接/transport/callbackFS **无直接 Go 单测**；端到端行为由 **P10 conformance/集成** 兜底。本轮补少量行为级 Rust 测试（基于公开接口，expected 取自 spec/Go 实现逻辑）：

### 协议 round-trip（`MessagePackProtocol` / `JSONRPCProtocol`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `msgpack_write_read_request_round_trip` | Call 写出三元组再读回 | `WriteRequest(method,params)` → `ReadMessage` 得等价 | `protocol_msgpack.go` 语义 | |
| `msgpack_fixarray3_marker` | 写出首字节为 `0x93` | tuple → 字节[0]==0x93 | `writeTuple` | |
| `msgpack_bin8_size_framing` | <256 payload 用 bin8（`0xC4`+len） | small payload → `0xC4` marker | `writeBin` | |
| `msgpack_invalid_array_marker_errors` | 非 `0x93` 首字节报 `ErrInvalidRequest` | `0x94...` → err 含 "expected fixed 3-element array (0x93)" | `readTuple` | |
| `msgpack_type_fixint_and_u8` | type 既接受 fixint 也接受 `0xCC`+value | 两种编码读出同 MessageType | `readTuple` | |
| `jsonrpc_write_response_nil_is_null` | result==nil → 写 `null` | `WriteResponse(id,nil)` → 含 `"result":null` | `protocol_jsonrpc.go:WriteResponse` | |

### 连接路由（`AsyncConn` / `SyncConn`，内存 pipe）

| Rust 测试 | 验证内容 | 依据 | 完成 |
|---|---|---|---|
| `async_conn_request_response` | 请求经 handler 处理并写回响应 | `conn_async.go:handleRequest` | |
| `async_conn_call_routes_response` | server→client `Call` 经 pending 路由拿到响应 | `conn_async.go:Call/handleResponse` | |
| `async_conn_handler_panic_writes_error` | handler panic → 写 InternalError 响应 | `conn_async.go:handleRequest` recover | |
| `sync_conn_call_inline_read` | `SyncConn.Call` 内联读响应（method 当 ID 匹配） | `conn_sync.go:Call` | |
| `sync_conn_unexpected_response_errors` | 主循环遇响应消息报错 | `conn_sync.go:Run` | |

### `MessageType` 枚举

| Rust 测试 | 验证内容 | 依据 | 完成 |
|---|---|---|---|
| `message_type_is_valid` | Request..Call 为 valid，Unknown 不是 | `protocol_msgpack.go:IsValid` | |
| `message_type_string` | `String()` 名表（"MessageTypeRequest" 等） | `stringer_generated.go` | |

### `callbackFS`（mock Conn）

| Rust 测试 | 验证内容 | 依据 | 完成 |
|---|---|---|---|
| `callbackfs_readfile_wrapper_states` | 三态：undefined→回退 base；`{content:null}`→not found；`{content:"x"}`→内容 | `callbackfs.go:ReadFile` | |
| `callbackfs_disabled_delegates_base` | 未启用回调时委托 base FS | `callbackfs.go:isEnabled` | |
| `callbackfs_unknown_callback_panics` | 构造时未知回调名 panic | `callbackfs.go:newCallbackFS` | |

### `Handle` 编解码（node handle 可直测）

| Rust 测试 | 验证内容 | 依据 | 完成 |
|---|---|---|---|
| `node_handle_round_trip` | `NodeHandleFrom` → `parseNodeHandle` 还原 pos/end/kind/path | `proto.go:NodeHandleFrom/parseNodeHandle` | |
| `project_handle_round_trip` | `ProjectHandle`/`parseProjectHandle` 路径还原 | `proto.go` | |

### Session 分发（依赖 checker/program → `—(P10)`）

| Rust 测试 | 验证内容 | 依据 | 完成 |
|---|---|---|---|
| `session_initialize` | initialize 返回版本/能力 | `session.go:handleInitialize` | —(P10) |
| `session_update_snapshot_diff` | updateSnapshot 返回快照 diff | `session.go:handleUpdateSnapshot/computeSnapshotChanges` | —(P10) |
| `session_get_symbol_at_position` | 取位置符号 → SymbolResponse + handle | `session.go:handleGetSymbolAtPosition` | —(P10) |
| `session_get_type_of_symbol` | 取符号类型 → TypeResponse | `session.go:handleGetTypeOfSymbol` | —(P10) |
| `session_release_handle` | release 释放快照/对象 handle | `session.go:handleRelease` | —(P10) |

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（25，含 TestMain）+ `Benchmark*`（2）都已映射
- [x] `proto_test.go` 5 子用例逐行；encoder/decoder 各函数逐行
- [x] expected 值取自 Go 测试字面量（DocumentIdentifier 的 fileName/uri/err、encoder baseline、decoder round-trip 结构）
- [x] 每条带 `// Go:` 锚点
- [x] 0 单测的 Session/协议/连接/callbackFS 已补行为级测试设计（msgpack/jsonrpc round-trip、conn 路由、callbackFS 三态、handle round-trip）
- [x] 与 impl.md 双向对齐

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `api.Session` 全部 handler 端到端 | 依赖 checker/compiler/ls + project.Session | P10 conformance |
| transport/server 真实 pipe 集成 | 依赖 OS pipe + project | P10 |
| encoder/decoder 全 AST 节点 parity | 依赖完整 AST（P2/P3）+ 生成器 | P10 |
| Benchmark（encode/decode checker.ts） | 性能基准 | perf 阶段 |
