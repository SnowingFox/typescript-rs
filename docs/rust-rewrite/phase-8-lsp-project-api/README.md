# Phase 8 — LSP / 工程 / API

> typescript-go → Rust 移植的第 8 阶段。把语言服务的**协议边界 + 工程状态机 + 对外 API** 移植到安全 Rust。
> 方法论与共享契约见根目录 [PORTING.md](../PORTING.md)（必读）。本 README 讲本 phase 的包构成、装配关系、协议序列化决策、并发点与进度。

## 包清单（依赖序）

按本 phase 内真实 import 边排序（叶子先行）。**本 phase 现含 3 个包：`project` / `api` / `lsp`**：

| 顺序 | 包 | crate | 实现文件 | 测试文件 / 函数 | 文档 |
|---|---|---|---|---|---|
| 1 | `project`（含 `ata`/`background`） | `tsgo_project`（+ `ata`/`background` 子 crate；`dirty`/`logging` 已拆到 P1） | 22 + 16 = 38 | 26 / 39 | [project/impl.md](./project/impl.md) · [tests.md](./project/tests.md) |
| 2 | `api`（含 `encoder`） | `tsgo_api` + `tsgo_api_encoder` | 14 + 5 = 19 | 4 / 27（25 Test + 2 Bench） | [api/impl.md](./api/impl.md) · [tests.md](./api/tests.md) |
| 3 | `lsp` | `tsgo_lsp` | 4（`lsproto` 已拆到 P7） | 13 / 42 | [lsp/impl.md](./lsp/impl.md) · [tests.md](./lsp/tests.md) |

> **依赖序修正（本轮）**：`jsonrpc` 前移到 **P1**（近叶子，仅依赖 `json`）；`lsproto` 拆 `tsgo_lsproto` 前移到 **P7**（被 `ls/*` 依赖）；`project/dirty`、`project/logging` 拆出前移到 **P1**（叶子，破 `ls↔project` 环）。本 phase 仅保留 `project`（根 + `ata`/`background`）、`api`、`lsp` 主体——它们依赖 P7 的 `ls`/`ls/*`/`lsproto`。
>
> crate 命名一律 `tsgo_<pkg>`；保留的子包（`project/ata`/`project/background`/`api/encoder`）作父 crate 的子 crate（`tsgo_<pkg>_<sub>`），镜像 Go package 边界（PORTING §2）。

## 装配关系（数据流与依赖图）

```mermaid
flowchart TB
  editor["编辑器 / 客户端\n(stdin·stdout JSON-RPC)"]
  subgraph P8[Phase 8]
    jsonrpc["tsgo_jsonrpc\nJSON-RPC 2.0 + base 帧"]
    lsproto["tsgo_lsproto\nLSP 3.17 协议类型 (serde)"]
    lsp["tsgo_lsp::Server\n读/派发/写 4 循环 + handlerMap"]
    project["tsgo_project::Session\n不可变快照引擎 (COW + refcount)"]
    api["tsgo_api::Session\nRPC over JSON-RPC / msgpack"]
  end
  subgraph lower[已移植下层 (P1–P7)]
    ls["tsgo_ls\n语言服务 (P7)"]
    compiler["tsgo_compiler / tsgo_checker (P4/P6)"]
  end

  editor <--> lsp
  lsp --> lsproto
  lsproto --> jsonrpc
  lsp --> project
  lsp --> ls
  lsp -. handleInitializeAPISession .-> api
  api --> project
  api --> jsonrpc
  project --> ls
  project --> compiler
  ls --> compiler
```

**关键装配链（`lsp ← project ← ls/compiler`）**：

1. **`lsp.Server` 是组合根之一**：`Run` 起 4 个并发循环（read/dispatch/write + readLoop 守卫）。`readLoop` 把 stdin 的字节流经 `lsproto.BaseReader`（= `jsonrpc.Reader`，Content-Length 帧）→ `json.Unmarshal` 成 `lsproto.Message`（按 `Method` 解码 params）。
2. **`handleInitialized` 建 `project.Session`**：把 `vfs.FS`、`Client`（= Server 自身）、`NpmExecutor`、`ParseCache` 注入。此后所有 textDocument 事件 → `Session.Did*`。
3. **每个语言服务请求**：`handlerMap` 的注册器（`registerLanguageServiceDocumentRequestHandler` 等）调 `session.GetLanguageService(uri)` → `Session` flush 待处理变更、增量 `Snapshot.Clone`、取出该文件默认项目的 `ls.LanguageService`，再委托给 `tsgo_ls`（P7）的 `Provide*`。`ls` 内部用 `compiler.Program` / `checker.Checker`（P4/P6）。
4. **`project.Session` 是状态机心脏**：把可变事件物化成不可变 `Snapshot`（COW + 引用计数），并据快照 diff 更新文件监听（回调 `Client.WatchFiles`）与 ATA。
5. **`api.Session` 是旁路**：`lsp.handleInitializeAPISession` 在同一 `project.Session` 上建一个 `api.Session`，经独立 pipe（`api.Transport`）用 JSON-RPC 或 msgpack 对外暴露 program/symbol/type 查询。`api` 也可作独立 `StdioServer`（P9 `cmd/tsgo` 入口）。

**为什么 `lsproto` 必须早于消费它的包**：`lsproto`（LSP 协议类型）被 `tsgo_ls`（P7）、`tsgo_project`、`tsgo_api`、`tsgo_lsp` 共同依赖。见下「存疑 / 偏离」中的跨 phase 排期说明。

## 协议序列化决策（jsonrpc / lsp 的 serde 用法）—— 已定

> 回填 PORTING §10 / crate-map 的「LSP 协议类型 | serde 派生（或 lsp-types 视 P8 决定）」待定项。

**结论：全部用 `serde` + `serde_json`；不采用外部 `lsp-types` crate；保留"从 LSP meta-model 代码生成 Rust 类型 + serde 自定义 (de)serialize"路线。**

1. **`jsonrpc`（base 层）**：`serde` 派生 `Message`/`RequestMessage`/`ResponseMessage`；params/result 用 `serde_json::value::RawValue` 延迟解码（对应 Go `json.Value`）。base 协议帧（Content-Length）是手写 `Reader`/`Writer`，不经 serde。
2. **`lsproto`（LSP 协议类型）**：**不用 `lsp-types`**，理由：
   - **行为 1:1 不可让步**：`lsp_json_test.go`（20 函数 ~70 子用例）断言了 optional-non-nullable 拒 `null`、未知字段忽略、required 缺失报 `missing required properties: ...`、四类联合（discriminator-`kind` / presence / boolean-or-options / string-or-array）、字符串字面量类型、`omitzero` 省略、枚举名表。`lsp-types` 的 `#[serde(untagged)]` 联合无法精确复刻 presence 判别与"拒 null"，错误文案也不一致。
   - **自定义扩展**：typescript-go 有 `lsp-types` 没有的方法/类型（`_/textDocument/sourceDefinition`、`x-multiDocumentHighlight`、VS Code references、`telemetry/event`、`_typescriptgo/*` 调试命令、`InitializeAPISession`、`ProjectInfo`、`InitializationOptions.UserPreferences`）。
   - **做法**：移植 `lsproto/_generate/generate.mts`（已是 TS）使其 emit Rust（serde derive + 联合/字面量手写 impl），保留"改 meta-model→重生成"工作流。第一阶段手抄被测试覆盖的子集（~30 类型 + 全部联合/枚举模式），其余随 P10 parity 增量生成。
3. **`api` 双协议**：JSON-RPC 走 serde；**msgpack 手写最小子集**（不引 `rmp`），逐字节对齐 Go 的 `[MessageType, method, payload]` 三元组（`0x93` + fixint/`0xCC` + `bin8/16/32`），否则与 JS 客户端不符。
4. **`api/encoder` 二进制 AST 格式**：自定义扁平 LE-uint32 布局，**字节级 parity**（baseline 测试），用 `Vec<u8>` + `to_le_bytes` 手写，不经 serde。

> Go 用 `any` + 运行时类型断言（`req.Params.(*lsproto.InitializeParams)`）；Rust 无此能力，改用**判别联合 `enum Params`**（生成器从 method→type 映射 emit），handler 注册改 `match` 变体。这是必要且安全的偏离。

## 并发点（PORTING §6 — 真并发 + 输出确定性）

| 位置 | Go 原语 | Rust 落地 |
|---|---|---|
| `lsp.Server` 4 循环 | `errgroup` + goroutine + chan(100) | `std::thread::scope` + `crossbeam-channel`(bounded) + `Arc<AtomicBool>` 取消 token |
| LSP 请求取消 | `context.WithCancel` + pending map | 每请求子 token + `Mutex<FxHashMap<Id, Pending>>` |
| `lsp` 进度状态机 | 单 goroutine + timer | 专用线程 + 可注入 `Clock`（测试虚拟时钟） |
| `project` 文件监听 | goroutine + 1s 超时回滚 | thread + `recv_timeout` + `watchRegistry` Mutex（保 watcher ID 稳定以重试） |
| `project` checker 池 | `sync.Cond` | `std::sync::Condvar` |
| `project` 快照调度/防抖 | `context` + timer + generation | 取消 token + 定时 + generation |
| `project.dirty.SyncMap` 竞争 | `proxyFor` 路由 | `Arc<Mutex<Entry>>` + 原子 LoadOrStore，败者持 `proxy_for` |
| `project` 后台队列 / ATA 安装 | `WaitGroup` / 节流 | 自管 join 集合 / `crossbeam` worker + 信号量 |
| `api.AsyncConn` | 每请求 goroutine + pending | thread + `Mutex<HashMap<Id, Sender>>` + writeMu |

**确定性铁律**：所有并发收集后按稳定 key 排序（诊断顺序、监听 glob、emit 顺序），保证与 Go 输出一致——这是 TDD 断言的前提。

## 0 直接单测的包 / 行为级补充

- **`jsonrpc`**：0 单测。base 协议帧由 `lsproto/baseproto_test.go` 间接全覆盖（复用为本包 ground truth），其余（ID/Message/版本/错误）补 spec round-trip。
- **`api.Session`（~70 handler）/ 协议 / 连接 / callbackFS**：无直接单测。补行为级（msgpack/jsonrpc round-trip、conn 路由、callbackFS 三态、handle round-trip），handler 端到端由 P10 兜底。

## 测试规模速查（采自当前仓库）

| 包 | 实现文件 | 测试文件 | 测试函数 | 子用例（约） |
|---|---|---|---|---|
| jsonrpc | 2 | 0 | 0 | — |
| lsp（+lsproto） | 9 | 13 | 42 | ~95 |
| project（+子包） | 38 | 26 | 39 | ~190 |
| api（+encoder） | 19 | 4 | 27（25T+2B） | ~30 |

## 实施纪律（每个包收口前）

1. 读 `impl.md` + `tests.md` + 对应 Go 源 + `*_test.go`。
2. 先写 Rust 测试（red）→ 再写实现（green），逐文件、逐用例。
3. 验证：`cargo test -p <crate>` 全绿 + `cargo clippy` 干净 + rustdoc 规范自检（PORTING §7）。
4. tests.md 与 Go 测试逐用例对齐审查（PORTING §8），impl.md 与 tests.md 互对齐。
5. 勾选文档，更新根 README 的 P8 进度。

## 推进顺序（phase 内）

> 前置（已在更早 phase）：`tsgo_jsonrpc`(P1)、`tsgo_lsproto`(P7)、`tsgo_project_dirty`/`tsgo_project_logging`(P1)、`tsgo_ls`/`ls/*`(P7)。本 phase 在它们之上推进：

1. **`project`**：phase 内叶子先行（`background`/`ata.validatepackagename`）→ 缓存族 → FS 层（`overlayfs`/`snapshotfs`）→ 项目/集合/会话（集成测试多依赖 `projecttestutil`，多数 DEFER 到 P10）。
2. **`api`**：`encoder`（baseline）→ `proto.DocumentIdentifier` → 协议/连接行为级 → `Session` 分发（依赖 checker/compiler，多数 DEFER）。
3. **`lsp.Server`**：4 循环 + handlerMap（多数 server 集成测试 DEFER 到 project/ls/api 就绪）。

## 存疑 / 偏离（提请评审）

1. ~~**跨 phase 依赖倒置（`lsproto` ← P7 `ls`）**~~ **已解决**：`tsgo_lsproto` 已抽成独立 crate 并**前移到 P7**（早于 `ls`/`format`）。`jsonrpc` 前移到 P1；`project/dirty`、`project/logging` 前移到 P1（破 `ls↔project` 环）。本 phase 现仅含 `project`/`api`/`lsp` 主体，无跨 phase 构建倒置（gate-docs.sh D6 GREEN）。本 README 下文涉及 `jsonrpc`/`lsproto`/`dirty`/`logging` 的装配/序列化描述仍有效，只是这些 crate 已落在更早 phase。
2. **`xxh3` 128-bit 一致性**：`project` 的缓存 key 用 `zeebo/xxh3` 128-bit；Rust 候选 `xxhash-rust`/`twox-hash` 须与 Go 字节级一致，否则增量复用失效。执行期用已知向量验证；不一致则自实现。已记 `// TODO(port)`。
3. **`api` Handle 指针 → id**：Go 的 `SymbolHandle`/`TypeHandle` 编码裸指针；Rust 改用 `snapshotData` 注册表 id（PORTING §5）。线缆格式因此与 Go 不完全一致——**若外部 JS 客户端依赖具体 handle 字符串格式，需确认兼容策略**（建议：handle 对客户端不透明，仅要求 round-trip 一致）。
4. **`api` msgpack 与 `lsproto` 序列化均"手写不引库"**：换取字节级 parity，代价是维护成本。请评审是否接受。
5. **`runtime/metrics` telemetry**（project 性能上报）：Rust 无直接等价，标 `// DEFER`，不影响功能正确性。
6. **Windows named pipe**：Go 用 `go-winio`；Rust 候选 `interprocess` 或 std named pipe，未最终选定（`// TODO(port)`）。
7. **大量 server/project/api 集成测试依赖 `testutil/lsptestutil`/`projecttestutil`/`baseline`**（P10 设施）：本 phase 文档逐用例列出但执行多标 `—(P10)`；纯函数/数据结构单测可在 P8 内收口。
