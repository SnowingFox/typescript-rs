# Go 依赖 / 标准库 → Rust crate 映射

> 全仓统一的依赖白名单。各 phase 新增依赖时**追加**到本表（执行期用 `cargo add` 取最新稳定版，不要瞎编版本号）。
> 通用类型映射见 [PORTING.md §3](../PORTING.md)。

## 基础数据结构

| 用途 | Go | Rust crate | 备注 |
|---|---|---|---|
| 有序 map/set（影响输出顺序） | `map` + 顺序切片 | `indexmap`（IndexMap/IndexSet） | 凡影响 emit/诊断顺序必须用 |
| 快速无序 hash | `map` | `rustc_hash`（FxHashMap/FxHashSet） | 默认无序 map |
| 并发 map | `sync.Map` | `dashmap` 或 `Mutex<HashMap>` | |
| bitflags 枚举 | `iota` + 位运算 | `bitflags` | NodeFlags/SymbolFlags/ModifierFlags/TypeFlags |
| arena / 索引图 | 自定义 arena（NodeFactory） | `la-arena` 或 `id-arena` | **全仓择一统一**；AST/Symbol/Type 图 |

## 并发

| 用途 | Go | Rust crate |
|---|---|---|
| 数据并行 | `go func` + WaitGroup | `rayon` |
| channel / worker 池 | `chan` + goroutine | `crossbeam-channel` + `std::thread::scope` |
| 原子 | `sync/atomic` | `std::sync::atomic` |
| 锁 | `sync.Mutex/RWMutex` | `std`（或 `parking_lot` 可选） |

## 序列化 / 协议

| 用途 | Go | Rust crate |
|---|---|---|
| JSON（tsconfig / LSP / API） | `encoding/json` + 自定义 | `serde` + `serde_json` |
| LSP 协议类型 | `internal/lsp/lsproto` | `serde` 派生（或 `lsp-types` 视 P8 决定） |

## 错误 / 工具

| 用途 | Rust crate | 备注 |
|---|---|---|
| 库内错误类型 | `thiserror` | 不把 `anyhow` 放进库公开 API |
| 临时目录（测试） | `tempfile` | 对应 Go testutil 临时 repo |
| 参数化测试 | `rstest` | 表驱动子用例 |

## 拆分子 crate（依赖序修正：1:1 映射 Go package + 破环 + 解倒置）

默认每个 `internal/<pkg>` = 一个 `tsgo_<pkg>` crate，子目录作其子 module（PORTING §2）。
但以下子包**必须拆成独立 crate**（否则 Cargo crate 环 / 跨 phase 倒置）：

| Go 子包 | crate | phase | 拆分原因 |
|---|---|---|---|
| `internal/lsp/lsproto` | `tsgo_lsproto` | P7 | LSP 协议类型，被 `ls/*`/`project`/`api`/`lsp` 共同依赖，须早于 `ls`（原 lsp 在 P8 会倒置） |
| `internal/ls/lsconv` | `tsgo_ls_lsconv` | P7 | URI/位置换算/诊断→LSP；被 `ls`/`project`/`api`/`lsp` 依赖，早于 `ls` 根 |
| `internal/ls/lsutil` | `tsgo_ls_lsutil` | P7 | 通用工具；`format` 依赖它，若作 `ls` 子 module 会成 `format→ls→format` 环 |
| `internal/ls/change` | `tsgo_ls_change` | P7 | 文本编辑追踪；`format` 依赖它，同上避免环 |
| `internal/ls/autoimport` | `tsgo_ls_autoimport` | P7 | 自动导入；`project` 依赖它、它又依赖 `project/{dirty,logging}`，拆出以破 `ls↔project` 环 |
| `internal/project/dirty` | `tsgo_project_dirty` | P1 | 脏标记（叶子）；被 `ls/autoimport` 依赖，须早于 ls/project，避免 `autoimport→project→autoimport` 环 |
| `internal/project/logging` | `tsgo_project_logging` | P1 | 日志（叶子）；同上 |

> 其余子包（`transformers/*`、`vfs/*`、`project/{ata,background}`、`api/encoder` 等）按各自 phase 文档决定（多数作父 crate 子 module 或父 phase 内的独立子 crate）。`transformers` 的 6 子包按 P5 决策各自独立 crate（见 phase-5 README）。

## 前移 / 重新归属的 crate（依赖序修正）

| crate | 原 phase | 新 phase | 原因（构建边） |
|---|---|---|---|
| `tsgo_tsoptions` | P6 | **P4** | `checker`/`modulespecifiers`/`printer` 非测试依赖它 |
| `tsgo_tracing` | P6 | **P4** | `checker` 非测试依赖它 |
| `tsgo_printer` | P5 | **P4** | `checker` 非测试依赖它 |
| `tsgo_sourcemap` | P5 | **P4** | `printer` 非测试依赖它 |
| `tsgo_outputpaths` | P5 | **P4** | `modulespecifiers`/`printer` 非测试依赖它 |
| `tsgo_jsonrpc` | P8 | **P1** | 近叶子（仅 `json`）；`lsproto`/`lsp`/`api` 依赖它 |
| `tsgo_bundled` | P9 | **P1** | 近叶子（`repo`/`tspath`/`vfs`）；`ls/lsconv`/`lsp`/`api`/`cmd/tsgo` 依赖它 |

> `tsgo_transformers`（依赖 `checker`/`printer`）保持在 **checker 之后（P5）**；`tsgo_compiler` 保持 **P6**。

## dev-dependency 层（仅测试边，不约束生产 phase 序）

某些 crate 仅在 `*_test.go` 里依赖更后 phase 的 crate → 映射为 Rust `[dev-dependencies]`，**不**构成构建倒置：

| crate | dev-dep（仅测试用） | 说明 |
|---|---|---|
| `tsgo_checker` | `tsgo_compiler`、`tsgo_bundled` | checker 的真 program 测试用 compiler 建程序 |
| `tsgo_ls_autoimport` | `tsgo_project` | autoimport 测试用 project 集成 |
| `tsgo_printer` | `tsgo_transformers` | printer 测试跑 transform 后再 emit |
| `tsgo_tsoptions` | `tsgo_diagnosticwriter` | tsoptions 测试渲染诊断 |
| （另有 `astnav`/`binder`/`format`/`parser`/`packagejson`/`vfs`/… → `parser`/`repo`/`vfs`/`bundled` 等仅测试边） | | 由 gate-docs.sh D6 自动归类为「仅测试边」 |

**测试设施层（P10）**：`tsgo_testutil`(+子包)、`tsgo_testrunner`、`tsgo_fourslash`，以及 `*tests`/`*testutil`/`*mock` 帮助子包（如 `internal/execute/tsctests`、`vfs/vfstest`、`tsoptions/tsoptionstest`）。它们的**入边按 dev-dep**，不约束生产 phase 序（已核实生产 `execute`（tsc/build/incremental）非测试代码**不**依赖 `testutil`）。

## 已决策（原"待定"回填）

| 用途 | 结论 | 决策 phase |
|---|---|---|
| sourcemap VLQ | **自实现**（Go 上游内联 ~50 行纯位运算，1:1 移植，不引第三方） | P5 决策（crate 现位 P4） |
| `Base64DataURL` 标准 base64 | 倾向 `base64` crate | P5 决策（crate 现位 P4） |
| UTF-16 / 字素处理（scanner/ls 位置换算） | **自实现**（镜像 Go `utf8.DecodeRuneInString` RuneError 语义 + `utf16.RuneLen`） | P3 / P7 |
| 文件系统抽象（vfs） | `std::fs` 封装 trait | P1 |
| `xxh3` 128-bit（buildInfo 签名） | `xxhash-rust`(`xxh3`) 或 `twox-hash`，须与 `zeebo/xxh3` 字节对齐（执行期 golden 验证） | P9 |
| 内嵌完美哈希（bundled libs） | `phf`(+`phf_macros`) | P1（bundled） |
