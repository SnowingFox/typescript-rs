# P9 execute · River A (first chunk) — single-program `tsc` build orchestration

> 代码轮次 worklog（配套 `impl.md`/`tests.md` 规划文档）。本轮交付 `tsgo_execute` 的
> **单工程** build/check/emit/report/exit-code 路径，1:1 对齐 Go `internal/execute` 的可达子集。
> 严格 TDD（tracer-bullet 探针先捕获真实行为，再逐切片 red→green）。**仅编辑 `internal/execute/**`**。

## 范围（本轮做了什么）

`execute(sys, args)` → `tsc_compilation` → `perform_compilation` 的单工程链路：

1. `execute(sys, args)`：解析命令行（`tsgo_tsoptions::parse_command_line`）→ 路由到单工程编译。镜像 Go `CommandLine` 的可达子集（`-b`/`--build`、`--watch` 路由 DEFER）。
2. `tsc_compilation`：命令行错误（如未知选项 TS5023）→ 报告 + 退出码 2；否则建程序、编译。镜像 Go `tscCompilation` 可达子集。
3. `perform_compilation`：建 `CompilerHost` + `Program`（`single_threaded=true`，checker 是 `Rc`）→ `emit_and_report_statistics`。镜像 Go `performCompilation`。
4. 诊断收集 + 报告（`tsgo_diagnosticwriter`）：options 诊断（global）+ 每文件语法诊断 + 全程序语义诊断 + emit 诊断，排序去重后逐条报告。镜像 Go `EmitFilesAndReportErrors`。
5. 退出码：`emit_skipped && diagnostics` → 2；`diagnostics` → 1；否则 0。镜像 Go `EmitAndReportStatistics` + `ExitStatus` iota。

## Go ground truth（实测 `cmd/tsgo`，`NO_COLOR=1` 管道=非 TTY=plain）

`go build -o /tmp/tsgo ./cmd/tsgo`，源文件 `index.ts`，cwd 在工程目录：

| 用例 | argv | exit | stdout | 产物 |
|---|---|---|---|---|
| clean | `index.ts`（`const x: number = 1;`） | **0** | (空) | `index.js` = `"use strict";\nconst x = 1;\n` |
| type error | `index.ts`（`const x: number = "s";`） | **1** | `index.ts(1,7): error TS2322: Type 'string' is not assignable to type 'number'.\n` | `index.js` = `"use strict";\nconst x = "s";\n`（仍 emit） |
| noEmit clean | `--noEmit index.ts` | **0** | (空) | 无 `index.js` |
| noEmit error | `--noEmit index.ts` | **2** | 同 TS2322 行 | 无 `index.js` |
| sourceMap | `--sourceMap index.ts` | **0** | (空) | `index.js`（尾随 `//# sourceMappingURL=index.js.map`，无尾换行）+ `index.js.map` |
| unknown option | `--badOption index.ts` | **2** | `error TS5023: Unknown compiler option '--badOption'.\n`（无文件位置） | 无产物 |
| removed option | `--target ES5 index.ts` | **1** | `error TS5108: Option 'target=ES5' has been removed. Please remove it from your configuration.\n` | `index.js`（仍 emit） |

pretty 模式（`--pretty` 或 `FORCE_COLOR=1`）额外打印 `Found 1 error in index.ts:1` 摘要 + ANSI 颜色 + 源码片段。

> **关键发现：plain（非 TTY）模式下 Go 不打印 `Found N errors` 摘要**——`CreateReportErrorSummary` 在非 pretty 下返回 `QuietDiagnosticsReporter`（no-op）。摘要只在 pretty 模式出现。本轮 1:1 复刻该门控。

## Rust 实测（探针捕获，作为断言依据）

| 用例 | Rust exit | Rust stdout | 产物 | vs Go |
|---|---|---|---|---|
| clean | Success(0) | (空) | `index.js` = `const x = 1;\n` | exit/产物✅；内容**缺 `"use strict";`**（emitter P6 分歧，下文） |
| type error | DiagnosticsPresentOutputsGenerated(1) | `index.ts(1,6): error TS2322: Type 'string' is not assignable to type 'number'.\n` | `index.js` 仍写 | exit/code/message✅；**列 6 vs Go 7**（checker span 分歧，下文） |
| noEmit clean | Success(0) | (空) | 无 `index.js` | ✅ |
| noEmit error | DiagnosticsPresentOutputsSkipped(2) | 同上 TS2322 行 | 无 `index.js` | exit✅ |
| sourceMap | Success(0) | (空) | `index.js` + `index.js.map`，含 `//# sourceMappingURL=` | ✅ |
| unknown option | DiagnosticsPresentOutputsSkipped(2) | `error TS5023: Unknown compiler option '--badOption'.\n` | 无产物 | ✅ 完全一致 |
| removed option | DiagnosticsPresentOutputsGenerated(1) | `error TS5108: Option 'target=ES5' has been removed. Please remove it from your configuration.\n` | `index.js` 仍写 | ✅ 完全一致 |
| plain summary | — | 仅 1 行诊断，无 `Found` | — | ✅（Go plain 也无摘要） |
| pretty summary | — | 含 `Found 1 error in index.ts` + ANSI + TS2322 | — | ✅ |

### 记录在案的分歧（均在不可编辑的下游 crate，本轮如实断言 Rust 现实 + 标注 Go 真值）

1. **emit 缺 `"use strict";`**：Rust emitter（`tsgo_compiler` emitter，P6-7）尚未加 CommonJS `"use strict";` 前言。**这是 emitter crate 行为，非编排层**；编排断言"`.js` 已写且含 `const x = 1`"（对分歧鲁棒），并记录 Go 产物全文。blocked-by: P6 emitter（`internal/compiler/**`，本轮不可编辑）。
2. **TS2322 列 6 vs Go 7**：探针确认 `tsgo_checker` 对该变量声明诊断给出 `start=5`（`x` 前的空格），Go 用 `6`（`x`）。列差 1 完全来自 **checker 诊断 span**（`tsgo_checker` crate，本轮不可编辑）。**code（TS2322）、message、退出码全一致**。blocked-by: checker 诊断 span（`internal/checker/**`）。
3. **语义诊断的多文件归属**：Rust `tsgo_checker::Diagnostic` 不带文件反指针，`program.semantic_diagnostics()` 返回扁平 `Vec`。可达单文件子集把语义诊断归到根文件（`config.file_names()[0]`）。多文件归属 DEFER。blocked-by: checker `Diagnostic` 无 file back-pointer。

## TDD 红→绿（垂直切片）

- **Tracer bullet**：写 `probe_observe_reality` 探针调 `execute(...)`，先看 `execute` 不存在 → 编译错误（red）；落地编排骨架 → 探针通过（green），并打印真实 exit/输出/产物/原始 checker span。据此把"现实"固化为断言。
- **slice 1 clean**：`clean_program_exits_zero_and_emits_js` → Success + `index.js` 写入。green。
- **slice 2 type error**：`type_error_reports_ts2322_and_exits_one` → exit 1 + 精确诊断行 + `.js` 仍写。
  - **显式 red→green 验证**：临时注释掉 `emit_and_report_statistics` 的退出码计算 → 该测试红：`left: Success, right: DiagnosticsPresentOutputsGenerated`；恢复 → 绿。证明退出码逻辑承重。
- **slice 3 noEmit clean**：`no_emit_clean_exits_zero_without_writing_js` → exit 0 + 无 `.js`（`Program::emit` 内部 noEmit 门控）。
- **slice 4 noEmit error**：`no_emit_errored_exits_two_without_writing_js` → exit 2 + 诊断 + 无 `.js`。
- **slice 5 sourceMap**：`source_map_clean_emits_js_and_map` → `.js` + `.js.map` + `//# sourceMappingURL=`。
- **slice 6 summary**：`plain_mode_prints_no_error_summary`（plain 无 `Found`，仅 1 行）+ `pretty_mode_prints_found_errors_summary`（`--pretty` → `Found 1 error in index.ts`）。1:1 复刻 Go 摘要门控。
- **options/config 诊断**：`unknown_option_reports_ts5023_and_exits_two`（TS5023 global，exit 2）+ `removed_option_reports_ts5108_and_exits_one`（TS5108，exit 1，仍 emit）。
- **直接入口**：`perform_compilation_entry_compiles_clean_program`。

## Go 函数映射（`// Go:` 锚点）

| Rust | Go |
|---|---|
| `lib.rs:execute` | `internal/execute/tsc.go:CommandLine` |
| `lib.rs:tsc_compilation` | `internal/execute/tsc.go:tscCompilation` |
| `lib.rs:perform_compilation` | `internal/execute/tsc.go:performCompilation` |
| `tsc/emit.rs:emit_files_and_report_errors` | `internal/execute/tsc/emit.go:EmitFilesAndReportErrors` |
| `tsc/emit.rs:emit_and_report_statistics` | `internal/execute/tsc/emit.go:EmitAndReportStatistics` |
| `tsc/compile.rs:ExitStatus` | `internal/execute/tsc/compile.go:ExitStatus` |
| `tsc/compile.rs:CommandLineResult` / `CompileAndEmitResult` | `…/compile.go:CommandLineResult` / `CompileAndEmitResult` |
| `tsc/diagnostics.rs:create_diagnostic_reporter` | `…/diagnostics.go:CreateDiagnosticReporter` |
| `tsc/diagnostics.rs:create_report_error_summary` | `…/diagnostics.go:CreateReportErrorSummary` |
| `tsc/diagnostics.rs:should_be_pretty`/`default_is_pretty`/`get_format_opts_of_sys` | `…/diagnostics.go:shouldBePretty`/`defaultIsPretty`/`getFormatOptsOfSys` |
| `tsc/diagnostics.rs:ReportedDiagnostic::from_*` | `internal/diagnosticwriter:WrapASTDiagnostic` |
| `tsc/diagnostics.rs:sort_and_deduplicate_diagnostics` | `internal/compiler/diagnostics.go:SortAndDeduplicateDiagnostics` |
| `sys.rs:System` / `VfsSystem` | `internal/execute/tsc/compile.go:System`（可达子集 + 内存实现） |

## 布局说明（mega-package split, PORTING §2）

Go 同时有 `package execute` 的 `tsc.go` 文件**和** `tsc/` 子包，Rust 不能同时存在 `tsc.rs` 模块文件与 `tsc/` 模块目录。故：`tsc.go` 的包级编排放 crate 根 `lib.rs`；`tsc/` 子包映射到 `tsc/` 子模块（`compile.rs`/`diagnostics.rs`/`emit.rs` + 合成的 `tsc/mod.rs`，因 Go 无 `tsc/tsc.go`）。`VfsSystem`（Go 的 sys 实现在 cmd/tsgo/tsctests）作为可达的内存 sys 放 `sys.rs`。

## 新增文件（仅 `internal/execute/**`）

- `Cargo.toml`：加 path deps（`tsgo_{checker,compiler,core,diagnostics,diagnosticwriter,locale,parser,tsoptions,tspath,vfs}`）。**未碰根 `Cargo.toml`**。`Cargo.lock` 由 cargo 自动更新（不可避免）。
- `lib.rs`：crate 根编排（`execute`/`tsc_compilation`/`perform_compilation` + `ParseConfigHost` 适配 + locale 助手）。`lib_test.rs`：9 个集成切片测试。
- `sys.rs` + `sys_test.rs`：`System` trait + `VfsSystem`（3 单测）。
- `tsc/mod.rs`、`tsc/compile.rs`、`tsc/diagnostics.rs`（+ `diagnostics_test.rs` 12 单测）、`tsc/emit.rs`。

## Gate 结果（crate-scoped，全绿）

- `cargo test -p tsgo_execute`：**24 单测 + 4 doctest 全过**。
- `cargo clippy -p tsgo_execute --all-targets -- -D warnings`：**clean**。
- `cargo fmt -p tsgo_execute -- --check`：**clean**。
- `cargo build -p tsgo_execute`：**Finished**。

> 全程 `-p tsgo_execute` 限定，未跑 `--workspace`，避免与并发 `internal/ls/lsutil/**` lane 冲突。

## 公开 API（仅新增，crate 内 additive）

`execute` / `tsc_compilation` / `perform_compilation`；`System` / `VfsSystem`；`ExitStatus` / `CommandLineResult` / `CompileAndEmitResult`；`ReportedDiagnostic` / `DiagFile` / `DiagnosticReporter` / `ReportErrorSummary`；`create_diagnostic_reporter` / `create_report_error_summary` / `sort_and_deduplicate_diagnostics` / `emit_files_and_report_errors` / `emit_and_report_statistics`。未删改任何既有测试或其它 crate。

## DEFER（blocked-by）

- `-b`/`--build` 编排 + 项目引用构建环 — blocked-by: P6-9a + build orchestrator chunk。
- `--watch` 监听循环 — blocked-by: p9-watcher chunk。
- 增量 skip-emit 复用接线 — blocked-by: P6-9b primitives（`tsgo_incremental`）接入。
- `cmd/tsgo` argv bin 入口 — blocked-by: p9-cmd chunk（本轮是库 `fn`）。
- `tsconfig.json` 发现 / `--project` / `--init` / `--version` / `--help` / `--showConfig` / `--locale` — blocked-by: 各自 chunk + locale 选项接线。
- 多文件语义诊断归属 — blocked-by: `tsgo_checker::Diagnostic` 无 file back-pointer（不可编辑 crate）。
- `--listFiles`/`--explainFiles`/`--diagnostics` 统计 + tracing + `CommandLineTesting` 钩子 — blocked-by: 统计/tracing chunk。
- emit 内容保真（`"use strict";` 前言）+ TS2322 列 — blocked-by: `tsgo_compiler` emitter（P6-7）/ `tsgo_checker` 诊断 span（均不可编辑）。
