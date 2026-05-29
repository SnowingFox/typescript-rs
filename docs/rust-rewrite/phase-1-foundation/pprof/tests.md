# pprof: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例。

## 0 直接单测的情况

- Go 侧无 `*_test.go`：`internal/pprof` 直接调用 `runtime/pprof`，副作用重（写文件、改运行时状态），**无直接单测**；其行为由 **P10 parity / 手动验证**兜底（产出的 profile 能被 `go tool pprof` 打开）。
- 本轮补充的行为级 Rust 测试（聚焦**无副作用的状态机与命名逻辑**，真实剖析后端用 feature gate / 临时目录隔离）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `cpu_profiler_double_start_errors` | 重复启动报错 | 连续两次 `start_cpu_profile` → 第二次 Err("CPU profiling already in progress") | pprof.go:StartCPUProfile | ✓ |
| `cpu_profiler_stop_without_start_errors` | 未启动即停止报错 | `stop_cpu_profile()`（未 start）→ Err("CPU profiling not in progress") | pprof.go:StopCPUProfile | ✓ |
| `cpu_profiler_start_stop_returns_path` | 正常启停返回路径 | start→stop → Ok(path)，path 以 `-cpuprofile.pb.gz` 结尾 | pprof.go:StartCPUProfile/StopCPUProfile | ✓ |
| `cpu_profile_filename_contains_pid` | 文件名含 pid | path 含 `process::id()` | pprof.go:StartCPUProfile（`%d-%d-cpuprofile.pb.gz`） | ✓ |
| `begin_profiling_creates_dir` | mkdir 行为 | 传入不存在目录 → 目录被创建，session 含 cpu/mem 路径 | pprof.go:BeginProfiling | ✓ |
| `save_heap_profile_returns_path` | 堆 profile 落盘（临时目录） | `save_heap_profile(tmp)` → Ok(path) 以 `-heapprofile.pb.gz` 结尾 | pprof.go:SaveHeapProfile | ✓ |
| `save_alloc_profile_returns_path` | 分配 profile 落盘 | `save_alloc_profile(tmp)` → Ok(path) 以 `-allocprofile.pb.gz` 结尾 | pprof.go:SaveAllocProfile | ✓ |
| `run_gc_no_panic` | RunGC 不 panic | `run_gc()` 正常返回 | pprof.go:RunGC | ✓ |

> 测试使用 `tempfile::TempDir` 隔离落盘，避免污染；真实剖析后端可能受 CI 环境限制，必要时标 `#[ignore]` 转手动。

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/pprof/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）

- [x] 无 Go `func Test*` 需映射（0 直接单测，已说明）
- [x] 补充测试覆盖 CpuProfiler 状态机 + 命名 + begin/stop + save_*，均在 impl.md 有 TODO 承载
- [x] expected 取自 Go 错误消息字面量与文件名后缀
- [x] 每条补充测试标注依据

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 产出的 `.pb.gz` 能被 `go tool pprof` 打开 | 需真实剖析后端 + 工具链 | P10 / 手动 |
| 堆/分配剖析数值正确性 | Rust 无 GC，语义随所选 crate 而变 | 实现期评估 |
| `--pprofDir` CLI 端到端 | 需 execute/CLI（P9） | P9 / P10 |
