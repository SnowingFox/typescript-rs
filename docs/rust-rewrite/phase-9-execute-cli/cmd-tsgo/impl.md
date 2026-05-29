# cmd-tsgo: 实现方案（impl.md）

**crate**：`tsgo`（**bin crate**，workspace 成员，产出可执行文件 `tsgo`）　**目标**：真正的程序入口——解析 `os.Args` 的第一参，把 `--lsp` 路由到 LSP server、`--api` 路由到 API server，其余交给 `execute.CommandLine` 跑 tsc；并装配真实操作系统环境（osvfs + bundled stdlib + 终端能力 + 父进程看护）。
**依赖（crate）**：`tsgo_core`、`tsgo_execute`（`CommandLine`）、`tsgo_execute::tsc`（`ExitStatus`）、`tsgo_bundled`（`wrap_fs`/`lib_path`）、`tsgo_lsp`（`--lsp`）、`tsgo_api`（`--api`）、`tsgo_pprof`、`tsgo_vfs` + `tsgo_vfs::osvfs`、`tsgo_tspath`。
**Go 源**：`cmd/tsgo/`（8 个非测试文件，约 200 行；含平台条件编译文件）。**无测试文件**。

## 这个包是什么（业务说明）

`cmd/tsgo` 是 typescript-go 的命令行二进制。它极薄——几乎所有逻辑都在 `internal/*` 里——但它负责三件入口职责：

1. **子命令路由**（`main.go`）：`runMain` 取 `os.Args[1:]`，若第一参是 `--lsp` → `runLSP`，`--api` → `runAPI`，否则 `execute.CommandLine(newSystem(), args, nil)`，并把 `ExitStatus` 作为进程退出码。
2. **装配真实系统**（`sys.go`）：`osSys` 实现 `tsc.System`——用 `bundled.WrapFS(osvfs.FS())` 作 FS（这样内嵌 stdlib 可见）、`bundled.LibPath()` 作默认库路径、`os.Stdout` 作 writer、`golang.org/x/term` 探测 TTY 与终端宽度、`os.Getenv` 取环境变量、真实时钟。
3. **进程级关切**（`lsp.go`/`api.go` + 平台文件）：LSP/API 模式下用 `signal.NotifyContext` 监听 SIGINT/SIGTERM；LSP 还有**父进程看护狗**（父编辑器进程死了就自杀，防孤儿进程）——这部分按平台分实现（`isProcessAlive` 在 unix/windows/其他 各一份）。Windows 上还在 `init()` 里开启虚拟终端处理（ANSI 颜色）。

它处于 **Phase 9** 末端：依赖 execute（P9）、bundled（P9）、lsp/api（P8）、pprof/core/vfs（P1）。它是整个移植的"可运行产物"出口。

> 与 `internal/<pkg>` 不同：cmd/tsgo 不是库 crate，而是 **bin crate**。按用户约定，`.rs` 放在 `cmd/tsgo/` 同目录同名，crate 名 `tsgo`，入口 `cmd/tsgo/main.rs`。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3。本包特有（重点是**平台条件编译**与**进程/信号**）：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `package main` + `func main()` + `os.Exit(runMain())` | `fn main() { std::process::exit(run_main()) }` | bin crate 入口。 |
| `os.Args[1:]` | `std::env::args().skip(1).collect::<Vec<_>>()` | |
| `switch args[0] { "--lsp", "--api" }` | `match args.first().map(String::as_str)` | 子命令路由。 |
| `osSys` 实现 `tsc.System` | `struct OsSys { ... }` impl `tsgo_execute::tsc::System` | 真实 OS 环境。 |
| `io.Writer`(os.Stdout) | `std::io::Stdout`（`Writer()` 返回 `&dyn Write`/`Box<dyn Write>`；注意 `System::writer` 的签名与并发） | tsc 内部可能多线程写 → 需 `Mutex` 包裹或行缓冲。 |
| `golang.org/x/term`：`term.IsTerminal(fd)` / `term.GetSize(fd)` | `std::io::IsTerminal`（`std::io::stdout().is_terminal()`）+ `terminal_size` crate（宽度） | TTY 判定用 std；宽度用 `terminal_size`（或 `crossterm`）。 |
| `os.Getenv` | `std::env::var(name).unwrap_or_default()` | |
| `time.Now()` / `time.Since(start)` | `std::time::SystemTime::now()` / `start.elapsed()` | `osSys.Now`/`SinceStart`。 |
| `os.Getwd()` + `tspath.NormalizePath` | `std::env::current_dir()` + `tspath::normalize_path` | |
| `os.Exit(int(tsc.ExitStatusInvalidProject_OutputsSkipped))` on getwd 失败 | `std::process::exit(ExitStatus::InvalidProjectOutputsSkipped as i32)` | |
| **build tags**（`//go:build windows` / `unix` / `!unix && !windows`） | `#[cfg(windows)]` / `#[cfg(unix)]` / `#[cfg(not(any(unix, windows)))]` | 平台分支，见下。 |
| `golang.org/x/sys/windows`（GetStdHandle/GetConsoleMode/...） | `windows-sys` crate（`Win32::System::Console::*`）`#[cfg(windows)]` | 启用 VT 处理。 |
| `os.FindProcess` + `proc.Signal(syscall.Signal(0))`（unix 探活） | `libc::kill(pid, 0)` + `errno`(ESRCH/EPERM) 或 `nix::sys::signal::kill` | unix `is_process_alive`。 |
| `syscall.OpenProcess`+`WaitForSingleObject`（windows 探活） | `windows-sys` `OpenProcess(SYNCHRONIZE)`+`WaitForSingleObject` | windows `is_process_alive`。 |
| `context.Context` + `signal.NotifyContext(SIGINT,SIGTERM)` + `stop()` | `Arc<AtomicBool>` 取消标志 + `ctrlc` crate（或 `signal_hook`）注册 SIGINT/SIGTERM → set flag | 不引 async；用取消标志（PORTING §3 `context.Context` → `Arc<AtomicBool>`）。 |
| `go func(){ ticker ... isProcessAlive }()`（看护狗） | `std::thread::spawn` + `loop { sleep(5s); if !alive { stop(); break } }` | 5s 轮询父进程。 |
| `flag.NewFlagSet`（lsp/api 子参解析） | `std` 手写解析 或 `clap`/`lexopt`（轻量） | lsp: `-stdio`/`-pipe`/`-socket`/`-pprofDir`；api: `-cwd`/`-pipe`/`-callbacks`/`-async`。 |
| `exec.Command("npm", args...)`（lsp NpmInstall 回调） | `std::process::Command::new("npm")` | |

### 平台条件编译（本包命门）

Go 用文件级 build tag；Rust 用 `#[cfg(...)]`，文件名同名（用户要求 `.rs` 同目录同名）：

| Go 文件（build tag） | Rust 文件（cfg） | 内容 |
|---|---|---|
| `enablevtprocessing_windows.go`（`windows`，文件名后缀隐含 tag） | `enablevtprocessing_windows.rs`（`#![cfg(windows)]` 模块 + 在 `main.rs` 用 `#[cfg(windows)] mod ...`） | `init()`（Go）→ Rust **无自动 init**；改为 `main` 启动早期显式调用 `enable_vt_processing()`（`#[cfg(windows)]` 版本做事，其它平台空函数）。 |
| `isprocessalive_unix.go`（`//go:build unix`） | `isprocessalive_unix.rs`（`#[cfg(unix)]`） | `is_process_alive(pid)` via `kill(pid,0)`；`PROCESS_ALIVE_SUPPORTED=true`。 |
| `isprocessalive_windows.go`（`//go:build windows`） | `isprocessalive_windows.rs`（`#[cfg(windows)]`） | `is_process_alive(pid)` via OpenProcess+WaitForSingleObject；`PROCESS_ALIVE_SUPPORTED=true`。 |
| `isprocessalive_other.go`（`//go:build !unix && !windows`） | `isprocessalive_other.rs`（`#[cfg(not(any(unix,windows)))]`） | `is_process_alive` panic；`PROCESS_ALIVE_SUPPORTED=false`。 |

> 关键偏离：Go 的 `enablevtprocessing_windows.go` 用包级 `init()` 自动执行。Rust 没有自动 `init()`，须在 `main` 开头显式调 `enable_vt_processing()`（非 Windows 平台提供空实现的同名函数，用 `#[cfg]` 分发），保持"启动即开 VT"的语义。

## 文件清单 → Rust 模块

> crate `tsgo`（bin）。`Cargo.toml` 用 `[[bin]] name="tsgo" path="cmd/tsgo/main.rs"`。各 `.rs` 与 `.go` 同目录同名；`main.rs` 用 `mod` 声明其余文件（平台文件加 `#[cfg]`）。

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `cmd/tsgo/main.go` | `cmd/tsgo/main.rs` | `fn main` + `run_main`（子命令路由 + 退出码）；`mod sys; mod lsp; mod api;` + 平台 `#[cfg] mod`。 |
| `cmd/tsgo/sys.go` | `cmd/tsgo/sys.rs` | `OsSys`（impl `tsc::System`）+ `new_system()`。 |
| `cmd/tsgo/lsp.go` | `cmd/tsgo/lsp.rs` | `run_lsp(args)`、看护狗 `new_parent_process_watchdog`/`start_parent_process_watchdog`。 |
| `cmd/tsgo/api.go` | `cmd/tsgo/api.rs` | `run_api(args)`。 |
| `cmd/tsgo/enablevtprocessing_windows.go` | `cmd/tsgo/enablevtprocessing_windows.rs`（`#[cfg(windows)]`） | `enable_vt_processing()`（Windows 实做）；非 Windows 在 main.rs 提供同名空函数。 |
| `cmd/tsgo/isprocessalive_unix.go` | `cmd/tsgo/isprocessalive_unix.rs`（`#[cfg(unix)]`） | `is_process_alive` + `PROCESS_ALIVE_SUPPORTED=true`。 |
| `cmd/tsgo/isprocessalive_windows.go` | `cmd/tsgo/isprocessalive_windows.rs`（`#[cfg(windows)]`） | 同上（Windows）。 |
| `cmd/tsgo/isprocessalive_other.go` | `cmd/tsgo/isprocessalive_other.rs`（`#[cfg(not(any(unix,windows)))]`） | panic + `PROCESS_ALIVE_SUPPORTED=false`。 |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | cfg | 备注 |
|---|---|---|---|
| 终端宽度 | `terminal_size` | all | 对应 `term.GetSize`；TTY 判定用 `std::io::IsTerminal`（不需额外 crate）。 |
| 信号处理（SIGINT/SIGTERM） | `ctrlc`（或 `signal_hook`） | all | `signal.NotifyContext` → 注册 handler 置取消标志。 |
| Windows 系统调用 | `windows-sys`（`Win32_System_Console`/`Win32_System_Threading`/`Win32_Foundation`） | windows | VT 处理 + 进程探活。 |
| unix 系统调用 | `libc`（或 `nix`） | unix | `kill(pid, 0)` 探活。 |
| lsp/api 子参解析 | 手写 或 `lexopt` | all | 轻量；避免给 bin 引重的 `clap`，除非已在用。 |

> 执行期 `cargo add`，记录到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `main.rs`（Go: `cmd/tsgo/main.go`）

- [ ] `fn main()` — `std::process::exit(run_main())`　`// Go: main.go:main`
- [ ] `fn run_main() -> i32` — `core::apply_debug_stack_limit()`；`#[cfg(windows)] enable_vt_processing()`（替代 Go init）；取 args；`--lsp`→`run_lsp`、`--api`→`run_api`、否则 `execute::command_line(new_system(), args, None).status as i32`　`// Go: main.go:runMain`
- [ ] `mod sys; mod lsp; mod api;` + 平台 `#[cfg] mod isprocessalive_*; #[cfg(windows)] mod enablevtprocessing_windows;`
- [ ] 非 Windows 的 `enable_vt_processing()` 空实现（`#[cfg(not(windows))]`）。

### `sys.rs`（Go: `cmd/tsgo/sys.go`）

- [ ] `struct OsSys { writer, fs, default_library_path, cwd, start }`　`// Go: sys.go:osSys`
- [ ] `impl tsc::System for OsSys`：
  - [ ] `since_start` / `now`　`// Go: sys.go:SinceStart/Now`
  - [ ] `fs` → `&dyn vfs::FS`　`// Go: sys.go:FS`
  - [ ] `default_library_path` / `get_current_directory` / `writer`　`// Go: sys.go:DefaultLibraryPath/GetCurrentDirectory/Writer`
  - [ ] `write_output_is_tty` → `stdout().is_terminal()`　`// Go: sys.go:WriteOutputIsTTY`
  - [ ] `get_width_of_terminal` → `terminal_size`　`// Go: sys.go:GetWidthOfTerminal`
  - [ ] `get_environment_variable` → `env::var`　`// Go: sys.go:GetEnvironmentVariable`
- [ ] `fn new_system() -> OsSys` — `current_dir`→`normalize_path`（失败 `exit(ExitStatusInvalidProject_OutputsSkipped)`）；`fs = bundled::wrap_fs(osvfs::fs())`；`default_library_path = bundled::lib_path()`；`writer = stdout`；`start = now`　`// Go: sys.go:newSystem`

### `lsp.rs`（Go: `cmd/tsgo/lsp.go`）

- [ ] `fn run_lsp(args: &[String]) -> i32` — 解析 `-stdio`/`-pprofDir`/`-pipe`/`-socket`；非 stdio → 报错返回 1；pprof（若设）；装配 `fs=bundled::wrap_fs(osvfs)`、`default_library_path`、`typings_location=osvfs::global_typings_cache_location()`；注册 SIGINT/SIGTERM 取消标志；`lsp::Server::new(ServerOptions{...})`（含 `npm_install` 回调跑 `npm`、`progress_delay=250ms`、`set_parent_process_id=watchdog`）；`server.run(cancel)`　`// Go: lsp.go:runLSP`
- [ ] `fn new_parent_process_watchdog(cancel) -> Option<impl Fn(i32)>` — 仅当 `PROCESS_ALIVE_SUPPORTED` 返回 Some　`// Go: lsp.go:newParentProcessWatchdog`
- [ ] `fn start_parent_process_watchdog(cancel, parent_pid)` — pid≤0 直返；`thread::spawn` 每 5s `sleep` 后 `is_process_alive(parent_pid)`，死则 stderr 提示 + `stop()`　`// Go: lsp.go:startParentProcessWatchdog`

### `api.rs`（Go: `cmd/tsgo/api.go`）

- [ ] `fn run_api(args: &[String]) -> i32` — 解析 `-cwd`/`-pipe`/`-callbacks`(逗号分割)/`-async`；`default_library_path=bundled::lib_path()`；构 `api::StdioServerOptions`（pipe 或 stdin/stdout）；注册 SIGINT/SIGTERM；`api::new_stdio_server(opts).run(cancel)`，错误打印返回 1　`// Go: api.go:runAPI`

### `enablevtprocessing_windows.rs`（Go: `cmd/tsgo/enablevtprocessing_windows.go`，`#[cfg(windows)]`）

- [ ] `pub fn enable_vt_processing()` — `GetStdHandle(STD_OUTPUT_HANDLE)`；若 char 设备 `GetConsoleMode` + 置 `ENABLE_VIRTUAL_TERMINAL_PROCESSING`　`// Go: enablevtprocessing_windows.go:init`

### `isprocessalive_unix.rs`（`#[cfg(unix)]`）

- [ ] `const PROCESS_ALIVE_SUPPORTED: bool = true`　`// Go: isprocessalive_unix.go`
- [ ] `fn is_process_alive(pid: i32) -> bool` — `kill(pid,0)` → `Ok`/`EPERM` 视为存活，`ESRCH`/其它为死　`// Go: isprocessalive_unix.go:isProcessAlive`

### `isprocessalive_windows.rs`（`#[cfg(windows)]`）

- [ ] `const PROCESS_ALIVE_SUPPORTED: bool = true`　`// Go: isprocessalive_windows.go`
- [ ] `fn is_process_alive(pid: i32) -> bool` — `OpenProcess(SYNCHRONIZE)` + `WaitForSingleObject(h,0)==WAIT_TIMEOUT` 为存活；关句柄　`// Go: isprocessalive_windows.go:isProcessAlive`

### `isprocessalive_other.rs`（`#[cfg(not(any(unix,windows)))]`）

- [ ] `const PROCESS_ALIVE_SUPPORTED: bool = false`　`// Go: isprocessalive_other.go`
- [ ] `fn is_process_alive(_pid: i32) -> bool { panic!("isProcessAlive is not supported on this platform") }`　`// Go: isprocessalive_other.go:isProcessAlive`

### Cargo / crate 接线

- [ ] `cmd/tsgo/Cargo.toml`：`[package] name="tsgo"`；`[[bin]] name="tsgo" path="cmd/tsgo/main.rs"`；deps（core/execute/bundled/lsp/api/pprof/vfs/tspath、`terminal_size`、`ctrlc`）；`[target.'cfg(windows)'.dependencies] windows-sys`；`[target.'cfg(unix)'.dependencies] libc`。
- [ ] 根 `Cargo.toml` workspace members 追加 `cmd/tsgo`。

## TDD 推进顺序（tracer bullet → 增量）

1. **`main.rs` + `sys.rs`**：先让 `tsgo <files>` 跑通普通编译路径（依赖 execute/bundled/osvfs 就绪），手动 smoke：`tsgo --version`、`tsgo --help`、`tsgo app.ts`，校验退出码。
2. **`isprocessalive_*`**：实现并加单测（见 tests.md）——这是 cmd/tsgo 里唯一好做纯单测的逻辑（自探活：当前进程存活=true、不存在 pid=false）。
3. **`lsp.rs`/`api.rs`**：参数解析 + server 装配（依赖 P8 lsp/api）。看护狗线程逻辑加单测（pid 路由）。
4. **`enablevtprocessing_windows.rs`**：Windows 平台编译验证（`cargo build --target x86_64-pc-windows-msvc`）。

## 与 Go 的已知偏离（divergence）

- **`init()` → 显式调用**：Go 的 `enablevtprocessing_windows.go` 用包级 `init()` 自动跑；Rust 无等价，改为 `run_main` 开头 `#[cfg(windows)] enable_vt_processing()`，非 Windows 提供同名空函数。语义（启动即开 VT）一致。
- **`context.Context` + `signal.NotifyContext` → `Arc<AtomicBool>` + `ctrlc`**：不引 async；取消用共享标志（PORTING §3）。看护狗与信号都置该标志，server 轮询/select 该标志退出。
- **`golang.org/x/term` → `std::io::IsTerminal` + `terminal_size`**：TTY 用 std（1.70+），宽度用 `terminal_size`。
- **平台 build tag → `#[cfg]`**：四个 `isprocessalive_*` 与 `enablevtprocessing_windows` 用 `#[cfg]` 选择；`PROCESS_ALIVE_SUPPORTED` 常量按平台定义（统一名，编译期选）。
- **`os.Exit` 在 goroutine/`main` 外**：Rust `std::process::exit` 不跑析构；与 Go `os.Exit` 一致（都不跑 defer/Drop）。注意：若 server 持有需 flush 的缓冲，按需在 exit 前显式 flush。
- **bin crate 同目录布局**：`.rs` 放 `cmd/tsgo/`（非 `internal/`），与库 crate 布局不同，按用户指定。

## 转交 / 推迟（DEFER）

- `tsgo_lsp::Server`/`ServerOptions`/`ToReader`/`ToWriter`、`tsgo_api::StdioServer`/`StdioServerOptions` 来自 **P8**；`run_lsp`/`run_api` 实现 `// DEFER(phase-8) blocked-by: tsgo_lsp/tsgo_api`。
- `tsgo_execute::CommandLine`、`tsc::ExitStatus`、`tsgo_bundled::{wrap_fs,lib_path}` 来自**本 phase（P9）**；接线即可。
- `osvfs::FS()`/`GetGlobalTypingsCacheLocation`、`tsgo_pprof::BeginProfiling`、`tsgo_core::{ApplyDebugStackLimit,Must,Version}` 来自 **P1**。
- 无 Go 测试文件 → 本包单测靠新增行为级测试（见 tests.md），端到端入口行为在 **P10** 兜底。
