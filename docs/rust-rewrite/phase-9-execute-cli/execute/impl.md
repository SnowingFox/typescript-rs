# execute: 实现方案（impl.md）

**crate**：`tsgo_execute`（含子模块 `tsc` / `incremental` / `build` / `tsctests`）　**目标**：实现 **tsc 命令行的全部行为层**——参数分发、单次编译、增量编译（`--incremental` / `.tsbuildinfo`）、watch 模式（`--watch`）、项目引用编排构建（`-b/--build`）、`--init`/`--showConfig`/`--help`/`--version`、诊断报告与统计。它是把"已解析的命令行 + program + emit"粘合成 `tsc` 用户体验的那一层。
**依赖（crate）**：`tsgo_compiler`（Program/Emit/CompilerHost）、`tsgo_tsoptions`（命令行/配置解析）、`tsgo_core`、`tsgo_ast`、`tsgo_diagnostics`、`tsgo_diagnosticwriter`、`tsgo_collections`、`tsgo_tspath`、`tsgo_locale`、`tsgo_json`、`tsgo_tracing`、`tsgo_pprof`、`tsgo_format`+`tsgo_ls`（fmtMain）、`tsgo_parser`、`tsgo_outputpaths`、`tsgo_checker`、`tsgo_binder`、`tsgo_vfs`（含 `cachedvfs`/`trackingvfs`/`vfswatch`）、`tsgo_bundled`（间接）。
**Go 源**：`internal/execute/`（30 个非测试文件，约 5400 行；根 2 文件 + `tsc/` 7 + `build/` 6 + `incremental/` 11 + `tsctests/` 4）。

## 这个包是什么（业务说明）

`internal/execute` 是 `tsc` 可执行文件的"大脑"。`cmd/tsgo`（入口 bin）把 `os.Args` 直接喂给 `execute.CommandLine(sys, args, testing)`，本包据此决定：是构建模式（`-b`）还是普通编译；是否 `--init`/`--version`/`--showConfig`/`--help`；要不要 watch；要不要走增量；最终调 `tsgo_compiler` 跑 program、emit、报诊断与统计。

四个子目录各司其职，存在清晰的内部依赖序（叶子在前）：

1. **`tsc/`**（最底层抽象）：定义 `System` 接口（FS/Writer/cwd/时间/环境变量/终端）、`ExitStatus`、`CommandLineResult`、`CommandLineTesting`（测试钩子）、`EmitInput`/`EmitFilesAndReportErrors`/`EmitAndReportStatistics`（编译→诊断→emit→报告的核心循环）、诊断报告器（pretty/color、watch 状态、构建状态）、`--help` 文本排版、`--init` 生成 `tsconfig.json`、`Statistics` 统计表、`ExtendedConfigCache`（并发安全的 extends 缓存）。**不依赖** build/incremental/根。
2. **`incremental/`**（增量引擎，依赖 compiler，**不**依赖 tsc）：`Program`（实现 `compiler.ProgramLike`）、快照（`snapshot`：fileInfos/版本/签名/引用图/每文件语义诊断缓存/待 emit 集合）、`.tsbuildinfo` 的读写与序列化（`BuildInfo` ↔ `snapshot` 双向转换）、受影响文件传播（`affectedFilesHandler`）、增量 emit（`emitFilesHandler`）。这是"只重编改动文件"的全部逻辑。
3. **`build/`**（项目引用编排，依赖 tsc + incremental）：`Orchestrator` 解析项目引用图、拓扑排序、并行构建任务（`BuildTask`）、up-to-date 判定（`upToDateStatus` 18 种）、伪构建（只更新时间戳）、`--clean`、`--dry`、`--verbose`、`-b --watch` 的增量重建。
4. **根**（`tsc.go`/`watcher.go`，依赖以上全部）：`CommandLine` 总分发；`tscCompilation`（找 config、init/version/showConfig/help、watch/incremental/普通分支）；`performCompilation`/`performIncrementalCompilation`；`Watcher`（轮询文件变化 → 增量重建）。
5. **`tsctests/`**（测试基座，依赖根+tsc+incremental）：`TestSys`（内存 FS + 假时钟 + 输出捕获 + baseline 序列化）、`NewTscSystem`、baseline 对拍框架、`.readable.baseline.txt`（人类可读 buildInfo）。**大部分仅服务于 baseline 测试（→ P10）**，但 `NewTscSystem`/`TestSys` 也支撑 `build` 的图测试与 watcher race 测试（非 baseline）。

> 为何在 Phase 9：它依赖几乎整条编译管线（compiler/checker/printer/tsoptions/...），是真正的 CLI 行为层；下游只有 cmd/tsgo（入口）和 P10（端到端对拍）。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5/§6。本包特有：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `tsc.System` interface | `trait System: Send + Sync`（FS/Writer/cwd/Now/SinceStart/env/TTY/width） | cmd/tsgo 与 tsctests 各有实现。`Writer() io.Writer` → `fn writer(&self) -> &Mutex<dyn Write>` 或 `&dyn Write`（注意并发写需同步）。 |
| `ExitStatus int` + 6 常量 | `#[repr(i32)] enum ExitStatus { Success=0, DiagnosticsPresent_OutputsGenerated=1, ... }` | 数值必须与 Go 一致（cmd/tsgo `os.Exit(int(status))`，baseline 也打印名）。 |
| `CommandLineResult{ Status, Watcher }` | `struct CommandLineResult { status: ExitStatus, watcher: Option<Box<dyn Watcher>> }` | `Watcher` interface（仅 `DoCycle()`）→ `trait Watcher { fn do_cycle(&self); }`。 |
| `CommandLineTesting` interface（11 钩子） | `trait CommandLineTesting`（OnProgram/OnEmittedFiles/OnListFiles.../GetTrace） | 生产为 `None`（`Option<&dyn CommandLineTesting>`）；测试注入 `TestSys`。 |
| `DiagnosticReporter = func(*ast.Diagnostic)` | `type DiagnosticReporter = Box<dyn Fn(&Diagnostic) + Send + Sync>` | 闭包捕获 writer/formatOpts。 |
| `collections.SyncMap[K,V]` | `dashmap::DashMap<K,V>`（或 `Mutex<FxHashMap>`） | 见 watcher/orchestrator/snapshot 并发缓存。 |
| `collections.SyncSet[T]` / `Set[T]` | `DashSet` / `FxHashSet`（按是否并发） | |
| `sync.Mutex` / `sync.Once` | `std::sync::Mutex` / `std::sync::Once`（或 `OnceLock`） | `Watcher.mu`、`buildInfoEntryMu`、`allFilesExcludingDefaultLibraryFileOnce`。 |
| `atomic.Bool` / `atomic.Int64` | `AtomicBool` / `AtomicI64` | `BuildTask.pending`、`rangeTask` 的 `currentTaskIndex`、`buildInfoEmitPending`。 |
| goroutine + `chan struct{}`（task.done / reportDone） | `crossbeam_channel`（或每任务 `Arc<(Mutex<bool>,Condvar)>` / oneshot） | 见下"并发模型"。 |
| `core.WorkGroup`（`NewWorkGroup(singleThreaded)` + Queue/RunAndWait） | P1 `tsgo_core::WorkGroup`（rayon scope 或顺序） | 并行解析配置/受影响文件/任务。 |
| `core.Tristate`（TSTrue/TSFalse/TSUnknown） | P1 `tsgo_core::Tristate` | 选项与 `hasErrors` 三态。 |
| `time.Time` / `time.Duration` | `std::time::{SystemTime, Duration}`（或 `Instant`） | mtime 比较、计时。注意 baseline 用假时钟。 |
| `reflect.DeepEqual`（watcher 比 ParsedConfig；help 反射 slice） | 显式 `PartialEq` derive；help 的 `reflect.ValueOf(...).Kind()==Slice` → `enum`/泛型分发 | `init.go` 的 `formatValueOrArray` 用反射判 slice → Rust 用具体类型 match。 |
| `any` / 类型断言（`upToDateStatus.data any`，`Raw.(*OrderedMap)`） | `enum UpToDateStatusData { ... }`；`Raw` 用具体类型/`enum` | 见 build `upToDateStatus`。 |
| `strings.Builder`（per-task 输出缓冲，保证并行报告确定性） | `String` 缓冲，最后按 `order` 顺序 `flush` 到真实 writer | 顺序确定性是 baseline 前提。 |

### 并发模型（本包命门，PORTING §6）

execute 的并行点（必须保确定性输出）：

1. **build Orchestrator 的任务调度**（`buildtask.go`/`orchestrator.go`）：
   - `rangeTask`：N 个 worker（`builders` 或默认 4，单线程则 1）从 `atomic.Int64` 索引拉 `order[]` 中的任务 → Rust `std::thread::scope` + `AtomicI64` work-stealing，或 `rayon`。
   - 任务间依赖用 channel：`waitOnUpstream` = `for up { <-up.task.done }`；`unblockDownstream` = `close(t.done)`。**报告顺序**用 `prevReporter.reportDone` 链保证按 `order` 串行 flush。Rust：每任务持 `done`/`report_done` 信号（`crossbeam::channel::bounded(0)` 关闭语义，或 `Arc<Barrier>`/`(Mutex<bool>,Condvar)`）。`close(ch)` → 广播；`<-ch` → 阻塞 recv。
   - 各任务把输出写进自己的 `strings.Builder`，`report` 时等 `prevReporter` 完成再 `fmt.Fprint(realWriter, builder)`，从而并行算、串行打。Rust 用同模式（`String` buffer + 顺序 flush）。
2. **配置图生成**（`createBuildTasks` + `core.WorkGroup`）：并行解析所有 tsconfig。
3. **incremental 受影响文件**（`collectAllAffectedFiles`）：两轮 `WorkGroup`——先并行 `getFilesAffectedBy`，再并行 `handleDtsMayChangeOfAffectedFile`；中间用 `SyncSet`/`SyncMap` 汇聚。Rust：rayon `par_iter` + 并发 map，最后 `updateSnapshot` 按 `GetSourceFiles()` 顺序提交（确定性）。
4. **incremental emit**（`emitFilesIncremental`）：并行 emit 各 affected file，`updateSnapshot` 再按源文件顺序收集 `results`（保证 emit 结果顺序确定）。
5. **Watcher**（`watcher.go`）：单 `sync.Mutex` 串行化 `DoCycle`；`vfswatch.FileWatcher.Run` 轮询线程。race 测试专门压这把锁。Rust：`Mutex<WatcherState>` + 轮询 `std::thread`。
6. **tsctests `run`**：每个 edit 用 `WorkGroup` 并行跑"增量 sys" 与"全量非增量 sys"再对拍——这是测试基座，主要服务 P10。

> 不引入 tokio（CPU-bound）。watcher 轮询用 `std::thread` + `sleep(interval)`。所有并行收集点（受影响文件、emit 结果、任务报告）**必须按稳定 key/源文件顺序排序输出**，否则 baseline 不可复现。

## 文件清单 → Rust 模块

> crate `tsgo_execute`，根 `lib.rs`（= `tsc.go` basename≠目录名？目录名是 `execute`，无同名文件 → crate 根用 `lib.rs`，声明各 `mod`）。子目录默认作**子模块**（`pub mod tsc;` 等），内部依赖序：`tsc` → `incremental` → `build` → 根 → `tsctests`。

### 根模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/execute/tsc.go` | `internal/execute/tsc_root.rs`（或并入 `lib.rs`） | `CommandLine` 分发、`tscCompilation`/`tscBuildCompilation`、`performCompilation`/`performIncrementalCompilation`、`findConfigFile`、`showConfig`、`fmtMain`、tracing 起停。**注意命名冲突**：根有 `tsc.go` 而子目录也叫 `tsc`；Rust 把根逻辑放 `lib.rs`/`commandline.rs`，子模块 `pub mod tsc;`。 |
| `internal/execute/watcher.go` | `internal/execute/watcher.rs` | `Watcher`（pub）、`watchCompilerHost`、`createWatcher`、`start`/`DoCycle`/`doBuild`/`compileAndEmit`/`recheckTsConfig`。 |

### `tsc/` 子模块（Go: `internal/execute/tsc/`）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `tsc/compile.go` | `internal/execute/tsc/mod.rs`（basename==目录名→`mod.rs`） | `System` trait、`ExitStatus`、`Watcher` trait、`CommandLineResult`、`CommandLineTesting`、`CompileTimes`、`CompileAndEmitResult`。 |
| `tsc/diagnostics.go` | `internal/execute/tsc/diagnostics.rs` | 诊断报告器：`CreateDiagnosticReporter`、`shouldBePretty`/`defaultIsPretty`、`colors`（ANSI）、`CreateReportErrorSummary`、`CreateBuilderStatusReporter`、`CreateWatchStatusReporter`。 |
| `tsc/emit.go` | `internal/execute/tsc/emit.rs` | `EmitInput`、`EmitAndReportStatistics`、`EmitFilesAndReportErrors`（核心循环：bind→check→emit→排序去重诊断→报告）、`listFiles`、`GetTraceWithWriterFromSys`。 |
| `tsc/extendedconfigcache.go` | `internal/execute/tsc/extendedconfigcache.rs` | `ExtendedConfigCache`（并发 extends 缓存，per-entry Mutex 防死锁/循环）。 |
| `tsc/help.go` | `internal/execute/tsc/help.rs` | `PrintHelp`/`printEasyHelp`/`printAllHelp`/`PrintBuildHelp`、`getHeader`（TS 图标排版）、`generateOptionOutput`（选项对齐换行）、`getValueCandidate`/`getPossibleValues`/`formatDefaultValue`。 |
| `tsc/init.go` | `internal/execute/tsc/init.rs` | `WriteConfigFile`/`generateTSConfig`（生成带注释的 `tsconfig.json`）。 |
| `tsc/statistics.go` | `internal/execute/tsc/statistics.rs` | `Statistics`、`table`（对齐打印）、`statisticsFromProgram`、`Report`/`Aggregate`/`SetTotalTime`、`formatDuration`。 |

### `incremental/` 子模块（Go: `internal/execute/incremental/`）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `incremental/incremental.go` | `internal/execute/incremental/mod.rs` | `BuildInfoReader` trait、`buildInfoReader`、`ReadBuildInfoProgram`、`NewBuildInfoReader`。 |
| `incremental/program.go` | `incremental/program.rs` | `Program`（impl `compiler::ProgramLike`）、`NewProgram`、`TestingData`、各 `Get*Diagnostics`、`Emit`、`collectSemanticDiagnosticsOfAffectedFiles`、`emitBuildInfo`、`ensureHasErrorsForState`、`SignatureUpdateKind`。 |
| `incremental/snapshot.go` | `incremental/snapshot.rs` | `snapshot`（核心状态）、`FileInfo`、`FileEmitKind`（bitflags）、`ComputeHash`(xxh3)、`emitSignature`、`buildInfoDiagnosticWithFileName`、`DiagnosticsOrBuildInfoDiagnosticsWithFileName`、`computeSignatureWithDiagnostics`、`canUseIncrementalState`、repopulate 链。 |
| `incremental/affectedfileshandler.go` | `incremental/affectedfileshandler.rs` | `affectedFilesHandler`、`collectAllAffectedFiles`、`getFilesAffectedBy`、`updateShapeSignature`、`forEachFileReferencedBy`、`handleDtsMayChangeOf*`、`updateSnapshot`。**并发热点**。 |
| `incremental/emitfileshandler.go` | `incremental/emitfileshandler.rs` | `emitFilesHandler`、`emitFiles`、`emitAllAffectedFiles`、`emitFilesIncremental`、`getEmitOptions`、`skipDtsOutputOfComposite`、`updateSnapshot`。**并发热点**。 |
| `incremental/programtosnapshot.go` | `incremental/programtosnapshot.rs` | `programToSnapshot`、`toProgramSnapshot`（reuse/computeChanges/handleDelete/PendingEmit/PendingCheck）、`fileAffectsGlobalScope`、`getReferencedFiles`、repopulate diagnostics。 |
| `incremental/snapshottobuildinfo.go` | `incremental/snapshottobuildinfo.rs` | `snapshotToBuildInfo`、`toBuildInfo`（fileId 分配、root 折叠、options 相对化、诊断序列化）。 |
| `incremental/buildinfotosnapshot.go` | `incremental/buildinfotosnapshot.rs` | `buildInfoToSnapshot`、`toSnapshot`（反序列化回 snapshot）。 |
| `incremental/buildInfo.go` | `incremental/build_info.rs`（注意大小写：Go 文件名 `buildInfo.go`，Rust 用 snake `build_info.rs`） | `.tsbuildinfo` JSON 模型：`BuildInfo`、`BuildInfoFileInfo`/`BuildInfoRoot`/`BuildInfoDiagnostic`/`BuildInfoEmitSignature`/... 全套自定义 `MarshalJSON`/`UnmarshalJSON`（紧凑变体编码）；`GetCompilerOptions`、`IsEmitPending`、`GetBuildInfoRootInfoReader`。 |
| `incremental/host.go` | `incremental/host.rs` | `Host` trait（GetMTime/SetMTime）、`CreateHost`、`GetMTime`。 |
| `incremental/referencemap.go` | `incremental/referencemap.rs` | `referenceMap`（references + 反向 referencedBy 懒构建，`sync.Once`）。 |

### `build/` 子模块（Go: `internal/execute/build/`）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `build/orchestrator.go` | `internal/execute/build/mod.rs` | `Orchestrator`（pub）、`Options`、`NewOrchestrator`、`GenerateGraph`/`GenerateGraphReusingOldTasks`、`createBuildTasks`/`setupBuildTask`（拓扑+环检测）、`Start`/`Watch`/`DoCycle`、`rangeTask`（并行池）、`buildOrClean`、`Order`/`Upstream`/`Downstream`、报告器工厂。 |
| `build/buildtask.go` | `build/buildtask.rs` | `BuildTask`、`upToDateStatus` 流程（`getUpToDateStatus`/`reportUpToDateStatus`）、`buildProject`/`compileAndEmit`/`cleanProject`、`handleStatusThatDoesntRequireBuild`、`updateTimeStamps`/伪构建、`updateDownstream`、watch 增量（`hasUpdate`/`updateWatch`/`resetStatus`/`resetConfig`）、buildInfo 缓存（`loadOrStoreBuildInfo`/`onBuildInfoEmit`/`getLatestChangedDtsMTime`）、`writeFile`。 |
| `build/host.go` | `build/host.rs` | `host`（impl `compiler.CompilerHost`+`incremental.BuildInfoReader`+`incremental.Host`）、`GetResolvedProjectReference`、mtime 缓存。 |
| `build/compilerHost.go` | `build/compiler_host.rs` | `compilerHost`（带 trace 的 per-task host 包装）。 |
| `build/parseCache.go` | `build/parse_cache.rs` | 泛型 `parseCache[K,V]`（per-entry Mutex 的并发 loadOrStore）。 |
| `build/uptodatestatus.go` | `build/uptodatestatus.rs` | `upToDateStatusType`（18 个 iota）、`upToDateStatus` 及其 `data` 变体（`inputOutputName`/`fileAndTime`/`inputOutputFileAndTime`/`upstreamErrors`）、`isError`/`isPseudoBuild`/`oldestOutputFileName`。 |

### `tsctests/` 子模块（Go: `internal/execute/tsctests/`，测试基座）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `tsctests/runner.go` | `internal/execute/tsctests/mod.rs`（`#[cfg(test)]` 或独立 dev crate） | `tscInput`/`tscEdit`、`run`（baseline 对拍驱动）、`executeCommand`、`getDiffForIncremental`、`getBaselineSubFolder`。 |
| `tsctests/sys.go` | `tsctests/sys.rs` | `TestSys`（impl `System`+`CommandLineTesting`）、`NewTscSystem`（pub，被 build 图测试用）、`newTestSys`、`TestClock`、输出捕获/sanitize、`baselinePrograms`/`serializeState`、FS 助手。 |
| `tsctests/fs.go` | `tsctests/fs.rs` | `testFs`（拦截 `.tsbuildinfo` 读写改版本号 + 写 `.readable.baseline.txt`）。 |
| `tsctests/readablebuildinfo.go` | `tsctests/readablebuildinfo.rs` | `readableBuildInfo`（把紧凑 buildInfo 转人类可读 JSON，含 fileId→路径展开）。 |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| xxh3 128 位哈希（文件签名/版本） | `xxhash-rust`（`xxh3` 特性）或 `twox-hash` | 对应 Go `github.com/zeebo/xxh3`，**必须同算法同输出**（`ComputeHash` 影响 buildInfo 内容，与 baseline 对拍）。需确认与 zeebo/xxh3 的 128bit 字节序一致。 |
| channel（任务依赖/worker 池） | `crossbeam-channel` | `task.done`/`reportDone`、`rangeTask`。 |
| 并发 map/set | `dashmap` | `SyncMap`/`SyncSet`。 |
| 数据并行 | `rayon`（经 `tsgo_core::WorkGroup`） | 受影响文件/emit/配置图。 |
| bitflags | `bitflags` | `FileEmitKind`。 |
| 有序 map | `indexmap` | buildInfo `Options`（`OrderedMap` 影响序列化顺序）。 |

> `xxh3` 选型要在执行期实测对齐 Go 输出（写个 golden 对照）；选定后记入 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序 = 内部依赖序：`tsc` → `incremental` → `build` → 根 → `tsctests`。

### `tsc/mod.rs`（Go: `tsc/compile.go`）

- [x] `pub trait System` — 7 方法（fs/default_library_path/get_current_directory/now/write/write_output_is_tty/get_environment_variable）　`// Go: compile.go:System`（done in sys.rs, committed; `width`/`since_start` DEFER）
- [x] `#[repr(i32)] pub enum ExitStatus`（Success/DiagnosticsPresent_OutputsSkipped/DiagnosticsPresent_OutputsGenerated/InvalidProject_OutputsSkipped/ProjectReferenceCycle_OutputsSkipped/NotImplemented）　`// Go: compile.go:ExitStatus`（done in lib.rs, committed）
- [ ] `pub trait Watcher { fn do_cycle(&self); }`　`// Go: compile.go:Watcher`
- [ ] `pub struct CommandLineResult { status, watcher: Option<...> }`　`// Go: compile.go:CommandLineResult`
- [ ] `pub trait CommandLineTesting`（11 钩子）　`// Go: compile.go:CommandLineTesting`
- [ ] `pub struct CompileTimes { config/parse/bind/check/total/emit/build_info_read/changes_compute }`　`// Go: compile.go:CompileTimes`
- [ ] `pub struct CompileAndEmitResult { diagnostics, emit_result, status, times }`　`// Go: compile.go:CompileAndEmitResult`

### `tsc/emit.rs`（Go: `tsc/emit.go`）

- [ ] `pub struct EmitInput { ... }`　`// Go: emit.go:EmitInput`
- [ ] `pub fn emit_and_report_statistics(input) -> (CompileAndEmitResult, Option<Statistics>)`　`// Go: emit.go:EmitAndReportStatistics`
- [ ] `pub fn emit_files_and_report_errors(input) -> CompileAndEmitResult` — bind 计时→check 计时→emit→`SortAndDeduplicateDiagnostics`→report→listFiles→summary　`// Go: emit.go:EmitFilesAndReportErrors`
- [ ] `fn list_files(input, emit_result)` — ListEmittedFiles/ExplainFiles/ListFiles(Only)　`// Go: emit.go:listFiles`
- [ ] `pub fn get_trace_with_writer_from_sys(...)`　`// Go: emit.go:GetTraceWithWriterFromSys`

### `tsc/diagnostics.rs`（Go: `tsc/diagnostics.go`）

- [ ] `pub type DiagnosticReporter` / `pub type DiagnosticsReporter`
- [ ] `pub fn quiet_diagnostic_reporter` / `quiet_diagnostics_reporter`　`// Go: diagnostics.go:QuietDiagnosticReporter`
- [ ] `pub fn create_diagnostic_reporter(sys, w, locale, options)` — quiet/pretty+color/plain　`// Go: diagnostics.go:CreateDiagnosticReporter`
- [ ] `fn default_is_pretty` / `fn should_be_pretty`（NO_COLOR/FORCE_COLOR/TTY）　`// Go: diagnostics.go:defaultIsPretty/shouldBePretty`
- [ ] `struct Colors`（is_windows/is_windows_terminal/is_vscode/richer）+ `bold/blue/blue_background/bright_white`（ANSI 码逐字节对齐）　`// Go: diagnostics.go:colors/createColors`
- [ ] `pub fn create_report_error_summary`　`// Go: diagnostics.go:CreateReportErrorSummary`
- [ ] `pub fn create_builder_status_reporter`（带 testing 钩子）　`// Go: diagnostics.go:CreateBuilderStatusReporter`
- [ ] `pub fn create_watch_status_reporter`（清屏 + 时间戳 `03:04:05 PM`）　`// Go: diagnostics.go:CreateWatchStatusReporter`

### `tsc/help.rs`（Go: `tsc/help.go`）

- [ ] `pub fn print_version` / `pub fn print_help` / `pub fn print_build_help`　`// Go: help.go:PrintVersion/PrintHelp/PrintBuildHelp`
- [ ] `fn get_options_for_help`（排序/过滤 simplified）　`// Go: help.go:getOptionsForHelp`
- [ ] `fn get_header`（TS 图标 + 终端宽度自适应，右对齐 ≤120）　`// Go: help.go:getHeader`
- [ ] `fn print_easy_help` / `fn print_all_help`（CLI/COMPILER/WATCH/BUILD 分节）　`// Go: help.go:printEasyHelp/printAllHelp`
- [ ] `fn generate_section_options_output` / `generate_group_option_output` / `generate_option_output`（对齐换行排版）　`// Go: help.go:generate*`
- [ ] `fn format_default_value` / `get_value_candidate` / `get_possible_values` / `show_additional_info_output` / `get_pretty_output` / `get_display_name_text_of_option`　`// Go: help.go:*`

### `tsc/init.rs`（Go: `tsc/init.go`）

- [ ] `pub fn write_config_file(sys, locale, report, options)` — 已存在则报错，否则写 + 提示　`// Go: init.go:WriteConfigFile`
- [ ] `fn generate_ts_config(options, locale) -> String` — 带注释模板（File Layout/Environment/.../Recommended），commented 三态，反射 slice 处理 → 具体类型　`// Go: init.go:generateTSConfig`

### `tsc/statistics.rs`（Go: `tsc/statistics.go`）

- [ ] `struct Table` + `add`/`print`（name/value 列对齐）　`// Go: statistics.go:table`
- [ ] `pub struct Statistics`（files/lines/identifiers/symbols/types/instantiations/memory/times，含 aggregate 字段）　`// Go: statistics.go:Statistics`
- [ ] `fn statistics_from_program(input, mem_stats)`（内存来自 runtime.MemStats → Rust 等价或占位）　`// Go: statistics.go:statisticsFromProgram`
- [ ] `Statistics::report` / `aggregate` / `set_total_time`、`format_duration`　`// Go: statistics.go:Report/Aggregate/SetTotalTime`

### `tsc/extendedconfigcache.rs`（Go: `tsc/extendedconfigcache.go`）

- [ ] `pub struct ExtendedConfigCache { m: DashMap<Path, Arc<ExtendedConfigCacheEntry>> }`　`// Go: extendedconfigcache.go:ExtendedConfigCache`
- [ ] `struct ExtendedConfigCacheEntry { inner, mu: Mutex<...> }`
- [ ] `impl tsoptions::ExtendedConfigCache::get_extended_config(...)` — per-entry 锁，防 extends 循环死锁　`// Go: extendedconfigcache.go:GetExtendedConfig`
- [ ] `fn load_or_store_new_locked_entry(path)` — 返回已锁条目（死锁/循环回归点）　`// Go: extendedconfigcache.go:loadOrStoreNewLockedEntry`

### `incremental/` 实现 TODO

- [ ] `mod.rs`：`trait BuildInfoReader`、`ReadBuildInfoProgram`、`NewBuildInfoReader`　`// Go: incremental.go:*`
- [ ] `host.rs`：`trait Host`(GetMTime/SetMTime)、`CreateHost`、`GetMTime`　`// Go: host.go:*`
- [ ] `snapshot.rs`：
  - [ ] `FileInfo`（version/signature/affects_global_scope/implied_node_format）　`// Go: snapshot.go:FileInfo`
  - [ ] `pub fn compute_hash(text, hash_with_text)`（xxh3-128 → hex，testing 追加文本）　`// Go: snapshot.go:ComputeHash`
  - [ ] `bitflags! FileEmitKind`（Js/JsMap/JsInlineMap/DtsErrors/DtsEmit/DtsMap + 组合）　`// Go: snapshot.go:FileEmitKind`
  - [ ] `GetFileEmitKind` / `getPendingEmitKind(WithOptions)`　`// Go: snapshot.go:GetFileEmitKind/getPendingEmitKind`
  - [ ] `emitSignature` + `getNewEmitSignature`　`// Go: snapshot.go:emitSignature`
  - [ ] `buildInfoDiagnosticWithFileName` + `toDiagnostic`/repopulate 链（ModeMismatch/ModuleNotFound）　`// Go: snapshot.go:toDiagnostic/repopulate*`
  - [ ] `DiagnosticsOrBuildInfoDiagnosticsWithFileName` + `getDiagnostics`
  - [ ] `struct snapshot {...}`（全字段，含 atomic/once/缓存）　`// Go: snapshot.go:snapshot`
  - [ ] `addFileToChangeSet`/`addFileToAffectedFilesPendingEmit`/`getAllFilesExcludingDefaultLibraryFile`/`computeSignatureWithDiagnostics`/`computeHash`/`canUseIncrementalState`　`// Go: snapshot.go:*`
- [ ] `referencemap.rs`：`referenceMap`（store/get/getPathsWithReferences/getReferencedBy 懒构建反向图）　`// Go: referencemap.go:*`
- [ ] `program.rs`：`Program`（impl `ProgramLike` 全部方法）、`NewProgram`、`TestingData`、`GetSemanticDiagnostics`(增量)、`Emit`(noEmit/HandleNoEmitOnError/emitBuildInfo)、`collectSemanticDiagnosticsOfAffectedFiles`、`emitBuildInfo`、`ensureHasErrorsForState`、`SignatureUpdateKind`　`// Go: program.go:*`
- [ ] `programtosnapshot.rs`：`programToSnapshot`、`toProgramSnapshot.{reuseFromOldProgram,computeProgramFileChanges,handleFileDelete,handlePendingEmit,handlePendingCheck}`、`fileAffectsGlobalScope`、`getReferencedFiles`(+symbol/import/triple-slash/typeRef/augmentation/ambient)、repopulate*　`// Go: programtosnapshot.go:*`
- [ ] `affectedfileshandler.rs`：`affectedFilesHandler`、`collectAllAffectedFiles`(两轮 WorkGroup)、`getFilesAffectedBy`、`updateShapeSignature`/`computeDtsSignature`、`forEachFileReferencedBy`、`handleDtsMayChangeOfAffectedFile`/`...OfFileAndReferences`/`...OfGlobalScope`/`...Of`、`updateSnapshot`、`isChangedSignature`、`removeDiagnosticsOfLibraryFiles`　`// Go: affectedfileshandler.go:*`
- [ ] `emitfileshandler.rs`：`emitFilesHandler`、`emitFiles`、`emitAllAffectedFiles`、`emitFilesIncremental`(并行 emit)、`getPendingEmitKindForEmitOptions`、`getEmitOptions`(dts 签名/composite skip)、`skipDtsOutputOfComposite`、`emitBuildInfo`、`updateSnapshot`(按源序收集)　`// Go: emitfileshandler.go:*`
- [ ] `build_info.rs`：`BuildInfo` 全模型 + 每类型的 `MarshalJSON`/`UnmarshalJSON`（紧凑变体编码 → serde 自定义 `Serialize`/`Deserialize`）、`IsValidVersion`/`IsIncremental`/`GetCompilerOptions`/`IsEmitPending`/`GetBuildInfoRootInfoReader`/`BuildInfoRootInfoReader`　`// Go: buildInfo.go:*`
- [ ] `snapshottobuildinfo.rs`：`snapshotToBuildInfo`、`toBuildInfo.{toFileId,toFileIdListId,collectRootFiles,setFileInfoAndEmitSignatures,setRootOf*,setCompilerOptions(相对化),setReferencedMap,setChangeFileSet,setSemanticDiagnostics,setEmitDiagnostics,setAffectedFilesPendingEmit}`、`toBuildInfoDiagnostics*`、`toBuildInfoRepopulateInfo`　`// Go: snapshottobuildinfo.go:*`
- [ ] `buildinfotosnapshot.rs`：`buildInfoToSnapshot`、`toSnapshot.{toAbsolutePath,toFilePath,toFilePathSet,setCompilerOptions,setFileInfoAndEmitSignatures,setReferencedMap,setChangeFileSet,setSemanticDiagnostics,setEmitDiagnostics,setAffectedFilesPendingEmit}`、`toBuildInfoDiagnosticsWithFileName`、`fromBuildInfoRepopulateInfo`　`// Go: buildinfotosnapshot.go:*`

### `build/` 实现 TODO

- [ ] `uptodatestatus.rs`：`enum UpToDateStatusType`（18 值，顺序对齐 iota）、`UpToDateStatus{ kind, data: UpToDateStatusData }`、`isError`/`isPseudoBuild`/`oldestOutputFileName`/`upstreamErrors`、data 变体结构　`// Go: uptodatestatus.go:*`
- [ ] `parse_cache.rs`：泛型 `ParseCache<K,V>`（per-entry Mutex `loadOrStore`/store/delete/reset）　`// Go: parseCache.go:*`
- [ ] `host.rs`：`host`（impl CompilerHost+BuildInfoReader(incremental)+Host(incremental)）、`GetResolvedProjectReference`、`ReadBuildInfo`、`GetMTime`/`SetMTime`/`loadOrStoreMTime`/`storeMTime`/`storeMTimeFromOldCache`、`GetSourceFile`(缓存 dts/json)　`// Go: host.go:*`
- [ ] `compiler_host.rs`：`compilerHost`（per-task trace 包装，转发 host）　`// Go: compilerHost.go:*`
- [ ] `buildtask.rs`：
  - [ ] `BuildTask` 结构 + 通道字段 + `taskResult`/`upstreamTask`/`buildInfoEntry`　`// Go: buildtask.go:BuildTask`
  - [ ] `waitOnUpstream`/`unblockDownstream`/`reportDiagnostic`/`report`（按 prevReporter 串行 flush）　`// Go: buildtask.go:*`
  - [ ] `buildProject`/`compileAndEmit`/`updateDownstream`　`// Go: buildtask.go:buildProject/compileAndEmit/updateDownstream`
  - [ ] `getUpToDateStatus`（全套判定：缺文件/版本/选项/输入新于输出/上游/伪构建）　`// Go: buildtask.go:getUpToDateStatus`
  - [ ] `handleStatusThatDoesntRequireBuild`/`reportUpToDateStatus`(verbose 18 分支)　`// Go: buildtask.go:*`
  - [ ] `updateTimeStamps`/`canUpdateJsDtsOutputTimestamps`/`cleanProject`/`cleanProjectOutput`　`// Go: buildtask.go:*`
  - [ ] watch：`updateWatch`/`hasUpdate`/`resetStatus`/`resetConfig`　`// Go: buildtask.go:*`
  - [ ] buildInfo 缓存：`loadOrStoreBuildInfo`/`onBuildInfoEmit`/`hasConflictingBuildInfo`/`getLatestChangedDtsMTime`/`storeOutputTimeStamp`/`writeFile`　`// Go: buildtask.go:*`
  - [ ] `enum UpdateKind`/`enum BuildKind`　`// Go: buildtask.go:updateKind/buildKind`
- [ ] `mod.rs`（orchestrator）：`Orchestrator`/`Options`/`NewOrchestrator`、`GenerateGraph(ReusingOldTasks)`、`createBuildTasks`/`setupBuildTask`(环检测+order)、`Start`/`Watch`/`updateWatch`/`resetCaches`/`DoCycle`、`buildOrClean`/`rangeTask`(并行池)/`buildOrCleanProject`、`Order`/`Upstream`/`Downstream`/`getTask`/`relativeFileName`/`toPath`、`orchestratorResult.report`、报告器工厂　`// Go: orchestrator.go:*`

### 根模块 实现 TODO

- [x] `pub fn execute(sys, args) -> CommandLineResult` — version/help/build dispatch + `tsc_compilation`　`// Go: tsc.go:CommandLine`（done in lib.rs, committed; `--build` DEFER）
- [ ] `fn tsc_build_compilation(...)`（build 错误/pprof/help/Orchestrator.Start）　`// Go: tsc.go:tscBuildCompilation`
- [x] `pub fn tsc_compilation(sys, parsed) -> CommandLineResult` — error reporting + `perform_compilation`　`// Go: tsc.go:tscCompilation`（done in lib.rs, committed; version/help/config DEFER to cmd/tsgo）
- [x] `pub fn perform_compilation(sys, parsed) -> CommandLineResult` — host→NewProgram→check→emit→diagnostics　`// Go: tsc.go:performCompilation`（done in lib.rs, committed; incremental DEFER）
- [ ] `fn find_config_file`（向上找 tsconfig.json）　`// Go: tsc.go:findConfigFile`
- [ ] `fn show_config`（ConvertToTSConfig + JSON 缩进）　`// Go: tsc.go:showConfig`
- [ ] `fn fmt_main`（`-f` 格式化，目前 NotImplemented 路径）　`// Go: tsc.go:fmtMain`
- [ ] `fn start_tracing_if_needed` / `fn stop_tracing` / `fn get_trace_from_sys`　`// Go: tsc.go:*`
- [ ] `watcher.rs`：`pub struct Watcher`（impl `tsc::Watcher`）、`watchCompilerHost`(modtime 缓存的 GetSourceFile)、`createWatcher`、`start`(初读 buildInfo + 首次 build + 启轮询)、`DoCycle`(锁 + recheckTsConfig + 变化判定 + doBuild)、`doBuild`(tracking FS + 增量 program + emit + 更新 watch state)、`compileAndEmit`、`recheckTsConfig`(配置变化重解析)　`// Go: watcher.go:*`

### `tsctests/` 实现 TODO（测试基座；多数服务 P10 baseline）

- [ ] `sys.rs`：`pub fn new_tsc_system`（pub，被 build 图测试用）、`TestSys`(impl System+CommandLineTesting)、`newTestSys`、`TestClock`(单调假时钟)、`OnProgram`/`baselinePrograms`/`serializeState`/输出 sanitize、FS 助手（write/remove/rename/replace/append/prepend）　`// Go: sys.go:*`
- [ ] `fs.rs`：`testFs`（`.tsbuildinfo` 读写改 FakeTSVersion + 写 `.readable.baseline.txt`）　`// Go: fs.go:*`
- [ ] `readablebuildinfo.rs`：`readableBuildInfo` + `toReadableBuildInfo`（fileId→路径、emitKind→名）　`// Go: readablebuildinfo.go:*`
- [ ] `mod.rs`（runner）：`tscInput`/`tscEdit`/`run`/`executeCommand`/`getDiffForIncremental`/`getBaselineSubFolder`　`// Go: runner.go:*`（**baseline 对拍主体 → P10**）

### Cargo / crate 接线

- [ ] `internal/execute/Cargo.toml`：`name = "tsgo_execute"`；path deps（compiler/tsoptions/core/ast/diagnostics/diagnosticwriter/collections/tspath/locale/json/tracing/pprof/format/ls/parser/outputpaths/checker/binder/vfs）；新增 `xxhash-rust`/`crossbeam-channel`/`dashmap`/`rayon`/`bitflags`/`indexmap`；dev-deps `rstest`/`tempfile`/baseline 工具。
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/execute`。
- [ ] `lib.rs`：`pub mod tsc; pub mod incremental; pub mod build;` + 根 `command_line`/`Watcher` 重导出；`#[cfg(test)] mod tsctests;`（或独立 dev crate）。

## TDD 推进顺序（tracer bullet → 增量）

1. **`tsc/mod.rs` + `tsc/extendedconfigcache.rs`**：先把 `System`/`ExitStatus`/`ExtendedConfigCache` 立起来，过 `TestExtendedConfigCacheExtendsCircularity`（3 子用例，纯逻辑、依赖 tsoptions 已就绪）。这是 execute 里最早能闭环的真单测。
2. **`tsc/emit.rs` + `tsc/diagnostics.rs`**：跑通"单文件 program → 诊断 → 报告"（普通编译 `performCompilation`），用最小内存 FS 验证退出码与诊断文本。
3. **`incremental/`**：先 `snapshot`+`programToSnapshot`（首次构建，全量 pending emit），再 `buildInfo` 序列化往返（`snapshotToBuildInfo`↔`buildInfoToSnapshot` round-trip 自测），再 `affectedFilesHandler`/`emitFilesHandler`（改一个文件只重编它）。
4. **根 `tsc.go` 分发 + `performIncrementalCompilation`**：跑通 `--incremental`。
5. **`build/`**：`uptodatestatus`/`parse_cache`/`host` → `Orchestrator.GenerateGraph`（拓扑+环检测），过 `TestBuildOrderGenerator`（9 子用例，纯图算法，**不依赖 emit**——最先能验证 build 的真单测）。再补 `buildProject`/up-to-date/clean/dry/verbose。
6. **`watcher.rs`**：单线程 DoCycle 跑通后，加轮询线程，过 6 个 `watcher_race_test`（线程压测 + ThreadSanitizer/loom，断言无死锁/panic）。
7. **`tsctests/`**：实现 `NewTscSystem`/`TestSys` 支撑上面 5/6 的测试；baseline 对拍主体 `run` 留到 **P10**。

## 与 Go 的已知偏离（divergence）

- **子目录 = 子模块而非独立 crate**：遵循用户指定的单一 `tsgo_execute` crate 名。内部依赖序（tsc→incremental→build→根）靠 `mod` 顺序与可见性维持。若编译期发现需要独立外部依赖再拆 crate（备选 `tsgo_execute_tsc` 等），届时在此更新。
- **goroutine + `close(chan)` 信号 → crossbeam channel / Condvar**：`task.done`/`reportDone` 的"关闭即广播"语义用 `crossbeam::channel`（drop sender 唤醒所有 recv）或 `(Mutex<bool>, Condvar)`。结构保持"等上游 done、串行 flush 报告"。
- **`reflect.DeepEqual`（watcher 比 ParsedConfig）→ `PartialEq`**：需为 `ParsedConfig` 派生/手写 `PartialEq`（P6 tsoptions 提供）。
- **`reflect` slice 判定（init.go `formatValueOrArray`）→ 具体类型 match**：用 `any`/`enum` 表达选项值，按 `is_array` 分支。
- **`runtime.MemStats`（statistics 内存）**：Rust 无等价标准 API；用 `jemalloc`/`tikv-jemalloc-ctl` 统计或先占位 0 + `// PERF(port)`，因为内存数仅在 `--diagnostics` 打印且 baseline 已 sanitize 时间/内存。
- **`time.Now().Format("03:04:05 PM")`**：watch/build 状态时间戳格式必须逐字符对齐（baseline sanitize 会替换，但格式串需一致）→ 用 `chrono`/`time` 格式或手写。
- **xxh3 哈希**：必须与 `zeebo/xxh3` 的 128bit 输出逐字节一致（影响 `.tsbuildinfo` 内容与对拍），执行期写 golden 校验。
- **`.tsbuildinfo` 紧凑变体编码（自定义 Marshal/Unmarshal）**：Go 用手写 `MarshalJSON` 编码"标量 or 元组 or 对象"三态（如 `BuildInfoFileInfo`/`BuildInfoEmitSignature`/`BuildInfoRoot`）。Rust 用 serde 的 `#[serde(untagged)]`/自定义 `Serialize`/`Deserialize` 复刻**完全相同的 JSON 形状**（顺序、`omitzero` 语义 → `skip_serializing_if`）。

## 转交 / 推迟（DEFER）

- **baseline 对拍**（`tsctests/runner.go` 的 `run`/`getDiffForIncremental`，及 41 个 baseline 测试函数 / ~338 子场景 / ~249 edit）→ `// DEFER(phase-10) blocked-by: testdata baselines + harnessutil/fsbaselineutil/baseline`。本 phase 只产出能跑通逻辑的代码 + 非 baseline 真单测；baseline 文本对拍在 P10 统一接入。
- **`tsgo_compiler`/`tsgo_checker`/`tsgo_tsoptions`/`tsgo_printer`/`tsgo_tracing`/`tsgo_format`/`tsgo_ls`/`tsgo_outputpaths` 等上游 crate**来自 P1–P7；execute 实现各函数体时 `// DEFER(phase-N) blocked-by:` 对应上游就绪即可。
- **`vfs/cachedvfs`/`trackingvfs`/`vfswatch`** 来自 P1 `tsgo_vfs`；watcher 依赖它们的 watch state / tracking 行为。
- **`pprof` profiling**（`pprof.BeginProfiling`）来自 P1 `tsgo_pprof`；CLI 仅在 `--pprofDir`/`PprofDir` 时触发，可先空实现。
- **`runtime.MemStats` 等价内存统计**：`// TODO(port)` 留待选型（仅 `--diagnostics` 用）。
