# cmd-tsgo: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**0 文件 / 0 `func Test*` / 0 子用例**。

## 0 直接单测的情况

- `cmd/tsgo/` **无任何 `*_test.go`**（grep 确认）。它是极薄的入口胶水：路由 `--lsp`/`--api`、装配 `osSys`、起信号与看护狗。其行为 ground truth 由：
  - **execute 的 baseline 对拍（P10）** 间接覆盖（tsctests 直接调 `execute.CommandLine`，复刻 cmd/tsgo 的 tsc 路径）；
  - **lsp/api 各自的测试（P8）** 覆盖 server 行为；
  - **端到端 CLI parity（P10）** 覆盖真实二进制入口。
- 因此本包测试策略：**逐条说明 P10/P8 兜底关系 + 本 phase 补少量可独立验证的行为级 Rust 测试**（聚焦平台无关、可纯逻辑断言的部分：进程探活、看护狗 pid 路由、子命令路由、参数解析）。

## 本轮补充的行为级 Rust 测试（基于公开/内部接口，expected 取自 Go 源逻辑）

| Rust 测试 | 验证内容 | input → expected | 依据（Go） | 完成 |
|---|---|---|---|---|
| `is_process_alive_self_true`（`#[cfg(unix)]`/`#[cfg(windows)]`） | 当前进程 pid 视为存活 | `is_process_alive(std::process::id())` → `true` | `isprocessalive_unix.go`/`isprocessalive_windows.go:isProcessAlive` | |
| `is_process_alive_dead_pid_false` | 几乎不可能存在的大 pid 视为死 | `is_process_alive(0x7FFF_FFFE)` → `false` | 同上（ESRCH/打开失败分支） | |
| `process_alive_supported_flag` | 平台支持常量正确 | unix/windows → `true`；其它 → `false` | `isprocessalive_*.go:processAliveSupported` | |
| `is_process_alive_other_panics`（`#[cfg(not(any(unix,windows)))]`） | 非 unix/windows 平台 panic | 调用 → `#[should_panic]` | `isprocessalive_other.go:isProcessAlive` | |
| `watchdog_returns_none_when_unsupported` | 平台不支持探活时看护狗工厂返回 None | `PROCESS_ALIVE_SUPPORTED=false` → `new_parent_process_watchdog()==None` | `lsp.go:newParentProcessWatchdog` | |
| `watchdog_ignores_non_positive_pid` | pid≤0 时看护狗不起线程（直接返回） | `start_parent_process_watchdog(_, 0)` → 立即返回，不 spawn | `lsp.go:startParentProcessWatchdog`（`if parentPID <= 0 { return }`） | |
| `route_lsp_subcommand` | 第一参 `--lsp` 路由到 LSP | args=`["--lsp", ...]` → 走 `run_lsp` 分支 | `main.go:runMain`（`case "--lsp"`） | |
| `route_api_subcommand` | 第一参 `--api` 路由到 API | args=`["--api", ...]` → 走 `run_api` 分支 | `main.go:runMain`（`case "--api"`） | |
| `route_default_to_command_line` | 其余路由到 `execute::command_line` | args=`["app.ts"]` / `[]` → 走 tsc 分支 | `main.go:runMain`（default） | |
| `lsp_non_stdio_returns_1` | LSP 仅支持 stdio，缺 `-stdio` 返回 1 | `run_lsp([])`（无 `-stdio`）→ stderr "only stdio is supported" + 退出码 1 | `lsp.go:runLSP`（`if !*stdio { ... return 1 }`） | |
| `lsp_flag_parse_error_returns_2` | LSP 参数解析失败返回 2 | `run_lsp(["--bad"])` → 2 | `lsp.go:runLSP`（`flag.Parse` 失败 `return 2`） | |
| `api_flag_parse_error_returns_2` | API 参数解析失败返回 2 | `run_api(["--bad"])` → 2 | `api.go:runAPI`（`flag.Parse` 失败 `return 2`） | |
| `api_callbacks_split` | `-callbacks` 逗号分割 | `-callbacks "readFile,fileExists"` → `["readFile","fileExists"]`；空 → 空 | `api.go:runAPI`（`strings.Split`） | |

> 路由测试为了可测，建议把 `run_main` 的"分派决策"抽成纯函数 `fn dispatch(args: &[String]) -> Route`（`Lsp/Api/Tsc`），对其断言；真正起 server/编译则在集成层。这样路由逻辑可纯单测，不触发真实 I/O。

## 平台条件编译测试说明

- 各 `is_process_alive` 测试用 `#[cfg(...)]` 与实现同条件门控（unix/windows 测真实探活；other 测 panic）。
- `enable_vt_processing`（Windows）难以纯单测（改控制台状态）；仅做 `#[cfg(windows)]` 编译验证 + 手动 smoke（终端显示 ANSI 颜色）。标 `—`(P10/手动)。

## 与 impl.md 的对齐核对

- [x] Go 无 `func Test*`，已声明"0 直接单测，由 P8(lsp/api) + P10(端到端 + execute baseline) 兜底"。
- [x] 补充的行为级测试均对应 impl.md 的实现 TODO（`is_process_alive`/`PROCESS_ALIVE_SUPPORTED`/`run_main` 路由/`run_lsp`/`run_api`/看护狗）。
- [x] 每条带 `// Go:` 锚点（指向源函数）。
- [x] expected 取自 Go 源逻辑（退出码 1/2、pid≤0 早返、`-callbacks` 分割、stdio 限制）。

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `tsgo --version`/`--help`/`app.ts`/`-b` 真实二进制端到端 | 需完整 execute + bundled + 真实 FS；与 Strada CLI 输出对齐 | **P10**（端到端 CLI parity） |
| `tsgo --lsp -stdio` 真实 LSP 会话 | 依赖 P8 lsp server 行为测试 | **P8 / P10** |
| `tsgo --api` 真实 API 会话（MessagePack/JSON-RPC） | 依赖 P8 api server 测试 | **P8 / P10** |
| 看护狗实际杀进程（父进程死→子退出） | 需真实子进程编排（spawn 父+子，杀父观察子退出） | **P10**（集成测试） |
| `enable_vt_processing` 实际开 VT | 改控制台状态，难自动断言 | 手动 / **P10** |
