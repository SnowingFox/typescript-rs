# pprof: 实现方案（impl.md）

**crate**：`tsgo_pprof`　**目标**：封装 CPU / 堆 / 分配 性能剖析的启停与落盘（`--pprofDir` 与 LSP 按需剖析）。
**依赖（crate）**：Rust 侧的剖析方案（如 `pprof` crate / `dhat` / 平台原生）。叶子包。
**Go 源**：`internal/pprof/`（1 个非测试文件：`pprof.go` 170 行）

## 这个包是什么（业务说明）

`pprof` 让编译器/语言服务能产出 Go pprof 格式的性能数据。两种用法：

1. **进程级一次性**：`BeginProfiling(dir, logWriter)` 开始 CPU 剖析并返回 `ProfileSession`，`Stop()` 停止 CPU 剖析、写堆（allocs）profile、打印路径。文件名 `<pid>-cpuprofile.pb.gz` / `<pid>-memprofile.pb.gz`。
2. **按需（LSP）**：`CPUProfiler`（带 `Mutex`）支持 `StartCPUProfile(dir)` / `StopCPUProfile()`，文件名带毫秒时间戳，避免重复启动（已在进行则报错）。
3. **独立快照**：`SaveHeapProfile(dir)`（先 `runtime.GC()`）、`SaveAllocProfile(dir)`、`RunGC()`。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `runtime/pprof`（标准库剖析） | `pprof` crate（pprof-rs，输出 protobuf `.pb.gz`） 或平台方案 | **关键偏离**：Rust 标准库无内建 pprof；CPU 剖析用 `pprof` crate（基于信号采样），堆/分配剖析用 `dhat`/`jemalloc` 等。输出格式尽量保持 pprof protobuf 以兼容 `go tool pprof` |
| `*os.File` / `io.Writer` | `std::fs::File` / `Box<dyn Write>` | 文件与日志写入 |
| `sync.Mutex` | `std::sync::Mutex` | `CPUProfiler` 的并发保护 |
| `panic(err)`（BeginProfiling/Stop） | `panic!` 或 `Result`（按调用点） | Go 这里直接 panic；Rust 可保留 panic 或上抛 `Result`，与调用点契约一致 |
| `(string, error)` 返回 | `Result<String, Error>` | `StartCPUProfile`/`StopCPUProfile`/`Save*` 返回路径或错误 |
| `os.Getpid()` / `time.Now().UnixMilli()` | `std::process::id()` / `SystemTime` | 文件名生成 |
| `runtime.GC()` | （Rust 无 GC）→ 空操作 / `drop` 触发 | **核心偏离**：Rust 无 GC，`RunGC`/堆剖析前的 `GC()` 无直接对应。堆剖析语义随所选 crate 而变 |

> **核心偏离**：Rust 无 GC、无标准库 pprof。本包是 P1 里与 Go 运行时绑定最深的包。移植策略：CPU 剖析用 `pprof` crate 产出兼容文件；堆/分配剖析改用 `dhat`/分配器钩子，**语义不完全等价**，整体标 `// TODO(port)` 并在 tests.md 记为 P10/工具链评估项。优先保证 API 形状与文件命名一致。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/pprof/pprof.go` | `internal/pprof/lib.rs`（basename == crate 目录名 → `lib.rs`） | 全部剖析 API |

## 依赖白名单（本包新增的 crate）

- `pprof`（pprof-rs，CPU 剖析 + protobuf 输出，`flamegraph`/`protobuf` feature 按需）。
- 堆/分配剖析候选：`dhat` 或自定义全局分配器钩子（实现期定夺）。记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/pprof/pprof.go`）

- [x] `pub struct ProfileSession { cpu_file_path, mem_file_path, cpu_file, log_writer }`　`// Go: pprof.go:ProfileSession`
- [x] `pub fn begin_profiling(profile_dir: &Path, log_writer: Box<dyn Write>) -> ProfileSession` — mkdir、按 pid 命名、启动 CPU 剖析　`// Go: pprof.go:BeginProfiling`
- [x] `impl ProfileSession { pub fn stop(self) }` — 停 CPU、写 mem profile（allocs）、打印路径　`// Go: pprof.go:(*ProfileSession).Stop`
- [x] `pub struct CpuProfiler { mu: Mutex<Option<ProfileSession>> }`　`// Go: pprof.go:CPUProfiler`
- [x] `impl CpuProfiler { pub fn start_cpu_profile(&self, dir: &Path) -> Result<(), Error> }` — 已在进行则报错 "CPU profiling already in progress"；文件名带毫秒时间戳　`// Go: pprof.go:(*CPUProfiler).StartCPUProfile`
- [x] `impl CpuProfiler { pub fn stop_cpu_profile(&self) -> Result<String, Error> }` — 未进行则报错 "CPU profiling not in progress"；返回文件路径　`// Go: pprof.go:(*CPUProfiler).StopCPUProfile`
- [x] `pub fn save_heap_profile(dir: &Path) -> Result<String, Error>` — 先 GC（Rust 无 GC，按 crate 语义）→ 写 heap profile　`// Go: pprof.go:SaveHeapProfile`
- [x] `pub fn save_alloc_profile(dir: &Path) -> Result<String, Error>` — 写 allocs profile　`// Go: pprof.go:SaveAllocProfile`
- [x] `pub fn run_gc()` — Rust 无 GC，空操作 / 触发分配器整理（按 crate）　`// Go: pprof.go:RunGC`

### Cargo / crate 接线

- [x] `internal/pprof/Cargo.toml`（`name = "tsgo_pprof"`；dev-dep `tempfile`。**偏离**：real `pprof` crate 后端推迟到 P10，本轮不引入运行期依赖，profile 文件按 Go 命名落盘但内容为占位）
- [x] 根 `Cargo.toml` workspace members 追加
- [x] `lib.rs` re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `CpuProfiler::start_cpu_profile` + `stop_cpu_profile` 的状态机（重复启动/未启动停止的错误消息）——**纯状态逻辑**，可不依赖真实剖析后端先测（用 mock/feature gate）。
2. 文件命名规则（pid / 毫秒时间戳 / 后缀 `.pb.gz`）。
3. 真实 CPU 剖析产出可被 `go tool pprof` 读取（集成测试，可能需手动/CI）。
4. 堆/分配剖析（依所选 crate，语义评估）。

## 与 Go 的已知偏离（divergence）

- **无标准库 pprof / 无 GC**：见核心偏离。CPU 剖析用 `pprof` crate，堆/分配剖析改用替代方案，`run_gc` 在 Rust 近乎空操作。整体 `// TODO(port)`。
- **错误处理**：Go `BeginProfiling`/`Stop` 用 panic，`CPUProfiler`/`Save*` 用 `error`。Rust 保留这一分裂（构造期 panic，运行期 `Result`）以贴合调用点，或统一为 `Result`（实现期定）。
- **文件格式兼容性**：目标是产出能被 `go tool pprof` 打开的 `.pb.gz`；`pprof` crate 支持 protobuf 输出，需验证兼容。

## 转交 / 推迟（DEFER）

- 堆/分配剖析的具体后端（dhat vs 分配器钩子）与文件格式兼容性验证：`// DEFER(phase-10)`，按工具链评估在实现期决定，不阻塞 P1 的 API 形状落地。
