# compiler: 实现方案（impl.md）

**crate**：`tsgo_compiler`　**目标**：编译管线的**编排层**——把前面所有 phase（scanner/parser/binder/checker/transformers/printer/module/tsoptions/tracing…）串成一个 `Program`：根据 `ParsedCommandLine` 并行加载 + 解析全部源文件、解析模块/类型引用/lib/project references、构建文件包含图与诊断、按需绑定/类型检查、并行 emit JS/d.ts/sourcemap。
**依赖（crate）**：`tsgo_ast` `tsgo_binder` `tsgo_checker` `tsgo_collections` `tsgo_core` `tsgo_diagnostics` `tsgo_json` `tsgo_locale` `tsgo_module` `tsgo_modulespecifiers` `tsgo_outputpaths` `tsgo_packagejson` `tsgo_parser` `tsgo_printer` `tsgo_scanner` `tsgo_sourcemap` `tsgo_symlinks` `tsgo_tracing` `tsgo_tsoptions` `tsgo_tspath` `tsgo_transformers`（含 declarations/estransforms/...）`tsgo_vfs`（含 cachedvfs）；外部 `xxh3`。**几乎依赖整棵树**——这正是它作为编排层的标志。
**Go 源**：`internal/compiler/`（14 个非测试 `.go`；最大 `program.go` 2161 行 + `filesparser.go` 549 行 + `fileloader.go` 763 行 + `fileInclude.go` 320 行）

## 这个包是什么（业务说明）

`compiler` 是"把零件组装成机器"的地方。它本身**不实现**词法/语法/类型/emit 算法（那些在 P3/P4/P5），而是**编排**它们：

1. **`Program`**（`program.go`）是中枢。`NewProgram(opts)` 做三件事：`processAllProgramFiles`（并行加载全部文件）→ `initCheckerPool`（建 checker 池）→ `verifyCompilerOptions`（选项一致性诊断，~400 行）。之后 Program 提供各类诊断查询（syntactic/bind/semantic/suggestion/declaration/global/program）、`Emit`、以及大量被 LSP/checker/emit 反向调用的 host 方法（`GetSourceFile`/`GetResolvedModule`/`GetModeForUsageLocation`/...）。
2. **文件加载**（`fileloader.go` + `filesparser.go`）：从 root files + lib + automatic type directives 出发，**并行**地解析每个文件、解析它的 triple-slash 引用 / 模块导入 / type 引用 / lib 引用，递归发现新文件（worklist + 去重 + 深度控制 + 大小写/包去重）。这是本包**第一大并发点**。
3. **文件包含图**（`fileInclude.go` + `includeprocessor.go` + `processingDiagnostic.go`）：记录"每个文件为什么被包含"（root/import/reference/lib/typeRef/ATA），用于 `--explainFiles` 和"file is in program because…"诊断。
4. **project references**（`projectreference*.go`）：解析 `references` 链（也**并行**），构建 source↔output(.d.ts) 映射，支持 `--build` 的 source-of-truth 重定向，含一个"假装 .d.ts 存在"的 VFS（`projectReferenceDtsFakingHost`）。
5. **checker 池**（`checkerpool.go`）：把 N 个文件分配给 K 个 checker（默认 4，可配 `--checkers`），支持**分组并行**收集诊断。这是本包**第二大并发点**。
6. **emit**（`emitter.go` + `emitHost.go`）：对每个待 emit 文件**并行**跑 transformers + printer，产出 JS/d.ts/sourcemap；输出按输入顺序合并以保确定性。这是**第三大并发点**。

它处在 Phase 6 末尾，因为它要等 P1–P5 全部就绪。它是 P7（语言服务）/P9（CLI execute）的直接上游——大家都拿 `*Program` 干活。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5/§6。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Program`（含嵌入 `processedFiles`） | `struct Program { opts, checker_pool, processed: ProcessedFiles, ... }`（组合，非嵌入） | Go 用结构体嵌入 `processedFiles`；Rust 用组合 + 委托（`self.processed.files`）。见偏离 |
| `CompilerHost` interface | `trait CompilerHost: Send + Sync` | `FS`/`GetSourceFile`/`GetResolvedProjectReference`/`Trace`/`DefaultLibraryPath`/`GetCurrentDirectory` |
| `CheckerPool` interface + `checkerPool` struct | `trait CheckerPool` + `struct CompilerCheckerPool` | 项目系统(P8)注入自定义池；本包内置池支持分组并行/non-exclusive |
| `*ast.SourceFile`（共享、不可变） | `Arc<SourceFile>` 或 arena 索引 | 解析后只读共享；并行 emit/check 都读它。优先 arena/`Arc` |
| `func() {}`（cleanup/done 闭包，如 checker release） | `impl FnOnce()` 或 RAII guard | `GetTypeCheckerForFile` 返回 `(checker, done)`；Rust 用 guard 释放锁 |
| `sync.Once`（commonSourceDirectory/sourceFilesToEmit/hasTSFile/packagesMap…） | `OnceLock<T>` | program 的惰性缓存 |
| `lazyValue[T]`（自定义 once+atomic，支持 `tryReuse`） | `struct LazyValue<T> { cell: OnceLock<T> }` + `try_reuse` | UpdateProgram 复用旧值；Rust 用 `OnceLock` + 显式 set |
| `collections.SyncMap[K,V]`（并发 map） | `dashmap::DashMap<K,V>` 或 `Mutex<HashMap>` | declarationDiagnosticCache、filesParser.taskDataByPath、includeProcessor 的若干缓存 |
| `atomic.Int32`/`atomic.Uint32`（计数） | `AtomicI32`/`AtomicU32` | totalFileCount/libFileCount/SymbolCount 聚合 |
| `core.WorkGroup`（单线程开关的 goroutine 池） | 抽象 `WorkGroup`（`rayon` scope 或 `std::thread::scope`，可切单线程） | 见并发小节；`NewWorkGroup(singleThreaded)` → 运行时选并行/顺序 |
| goroutine + 递归 `start(...)` worklist | `std::thread::scope` + work-stealing 或 `rayon` + 递归 spawn；`SyncMap` 去重 | 文件加载/项目引用并行，见并发小节 |
| `sync.Pool`（writerPool/parseTaskDataPool） | `thread_local!` 或对象池（`crossbeam` 的 queue / 简单 `Mutex<Vec<T>>`） | emit 复用 `TextWriter`；解析复用 taskData |
| `context.Context`（取消） | `&Cancel`（`Arc<AtomicBool>`）显式传入 | 诊断/emit 接受取消标志；不引 async（PORTING §3） |
| `iter.Seq`（`GetOutputFileNames` 等） | `impl Iterator` | range-over-func |
| interface assertion `var _ checker.Program = (*Program)(nil)` | `impl checker::Program for Program` | Program 实现多个上游 trait（checker.Program / outputpaths host / SourceFileMayBeEmittedHost / ProgramLike） |
| `panic("should not be called by resolver")`（fakingVfs 多数方法） | `unreachable!("...")` | 保留 |

### 编排层的 trait 实现（被反向调用）

`Program` 同时实现多个上游定义的 trait（Go 里是隐式接口满足）：

- `checker::Program`（FileExists/GetCurrentDirectory/GetResolvedModule/GetSourceFile/...，checker 反向查 program）
- `outputpaths::OutputPathsHost` / `SourceFileMayBeEmittedHost`（emit 路径计算）
- `ProgramLike`（被 `HandleNoEmitOnError`/`GetDiagnosticsOfAnyProgram` 等通用函数用）

Rust 侧用显式 `impl Trait for Program`，trait 定义在各自上游 crate。

## 并发（本 phase 的核心，PORTING §6 落地）

本包是全仓最集中的并发点。逐站点给出 Rust 原语选择与**确定性保证**（确定性是 TDD 断言前提）。

### 并发点 1：文件加载/解析（`filesparser.go` + `fileloader.go`）

- **Go 机制**：`filesParser` 持 `core.WorkGroup`（包装 goroutine + WaitGroup，可单线程）+ `SyncMap[Path, *parseTaskData]` 去重。`start(tasks, depth)` 把每个 task `Queue` 到 wg；任务体在 `data.mu`（每文件一把锁）下 `load`（parse 文件、解析其引用），发现子任务后**递归** `start(subTasks)`（动态扩张 worklist）。`sync.Pool` 复用 `parseTaskData`。
- **数据并行 vs 生产者-消费者**：这是**动态 worklist**（任务运行时产生新任务），不是固定 batch。
- **Rust 选型**：`rayon::scope`（`s.spawn` 递归提交）或 `std::thread::scope` + `crossbeam-channel` worker 池。去重用 `dashmap::DashMap<Path, ParseTaskData>`，每文件 `Mutex` 保护其 task 状态。`parseTaskData` 池用 `thread_local` 或 `crossbeam::queue::SegQueue`。
  - 推荐 **`rayon::scope` + `DashMap`**：递归 `scope.spawn` 天然表达"任务生成任务"；`DashMap::entry` 做 LoadOrStore 去重。
  - `NewWorkGroup(singleThreaded)` 抽象成一个 `WorkGroup` enum：`Parallel(rayon scope handle)` / `Sequential`（直接调用）。单线程模式（`--singleThreaded` 或 program.SingleThreaded()）走顺序，便于调试 + 确定性。
- **确定性保证**：并行只影响**发现顺序**，不影响**最终顺序**。最终文件列表由 `getProcessedFiles` 的 `collectFiles`（**单线程后处理**，按 rootTasks → subTasks 深度优先 + `seen` 去重）确定。lib 文件再按 `sortLibs`（优先级排序）。所以 `TestProgram` 的 `expectedFiles` 顺序与线程调度无关——Rust 必须保留"并行发现、串行收集 + 稳定排序"两段式。
- **共享可变**：`fileLoader.factory`（NodeFactory，建合成 import 节点）用 `factoryMu` 保护 → Rust `Mutex<NodeFactory>`。`pathForLibFileCache`/`pathForLibFileResolutions` 用 `SyncMap` → `DashMap`。

### 并发点 2：项目引用解析（`projectreferenceparser.go`）

- **Go 机制**：另一个 `core.WorkGroup` + `SyncMap[Path, *projectReferenceParseTask]`，`start` 递归 spawn 解析每个 referenced tsconfig；`parse` 后 `initMapper`（**单线程**）按确定顺序构建 source↔output 映射。
- **Rust 选型**：同并发点 1（rayon scope + DashMap）。`initMapper`/`initMapperWorker` 保持单线程深度优先 + `seen` 去重，保证 `referencesInConfigFile`/`sourceToProjectReference` 顺序稳定（注释明确：子引用覆盖父引用，顺序敏感）。

### 并发点 3：绑定 / 诊断收集（`program.go`）

- **`BindSourceFiles`**：`NewWorkGroup` 对每个未绑定文件并行 `binder.BindSourceFile(file)`（各文件独立，无共享写）→ `rayon::par_iter` 直译。
- **`collectDiagnosticsFromFiles`**：并行 `collect(file)` 写入**预分配的 `diagnostics[i]`**（按 index 写，无竞争）→ `rayon` map 到固定大小 `Vec`。最后 `slices.Concat` + `SortAndDeduplicateDiagnostics`（稳定排序）保证确定性。
- **`collectCheckerDiagnosticsFromFiles`**：见并发点 4（分组）。

### 并发点 4：checker 池（`checkerpool.go`）

- **Go 机制**：`checkerCount = min(max(配置或4, 1), 文件数, 256)`（单线程则 1）。`createCheckers` 用 wg 并行建 N 个 checker（各配一把 `sync.Mutex`）。文件按 `i % checkerCount` 预分配到 checker（`fileAssociations`）。
  - `forEachCheckerParallel`：每 checker 一个并行任务，锁该 checker 后回调（全局诊断聚合）。
  - `forEachCheckerGroupDo`：每 checker 一个并行任务，遍历**全部文件只处理分给自己的**（保持文件原序），写 `diagnostics[fileIndex]`。这是"按 checker 分组并行"以减少锁竞争 + 提升缓存局部性。
- **Rust 选型**：`Vec<(Checker, Mutex<()>)>`（或 `Vec<Mutex<Checker>>`）。`create_checkers` 用 `rayon` 并行构造（`OnceLock` 保证一次）。`forEachCheckerGroupDo`/`forEachCheckerParallel` 用 `rayon` 对 checker 索引并行；锁用 `parking_lot::Mutex` 或 `std::Mutex`。
  - **non-exclusive 访问**（emit/全局诊断时不加锁）：Go 有 `getCheckerNonExclusive`/`getCheckerForFileNonExclusive`（注释：仅当调用方保证无并发访问同 checker 时安全，如只读取 emit resolver）。Rust 表达较难（借用检查器要求要么独占 `&mut` 要么共享 `&`）。方案：checker 内部需要的可变状态用内部可变性（`RefCell` 不 Send，故用 `Mutex`/`AtomicXxx`），non-exclusive 路径走 `&Checker`。**这是 checker(P4) 的设计约束**，本包标 `// DEFER(phase-4)`：池只持有 `Arc<Checker>`，是否能真正 non-exclusive 取决于 checker 内部可变性设计。
- **确定性保证**：诊断按 checker 收集后 `slices.Concat` + `SortAndDeduplicateDiagnostics`（`ast.CompareDiagnostics` 稳定序）。文件→checker 分配 `i % K` 是确定的。

### 并发点 5：emit（`program.go:Emit` + `emitter.go`）

- **Go 机制**：`NewWorkGroup` 对每个 `sourceFilesToEmit` 并行：取 `writerPool`（`sync.Pool`）的 writer、建 `emitHost`（内含该文件的 emit resolver，来自 non-exclusive checker）、跑 transformers + printer、写文件、归还 writer。`emitters` 按输入顺序收集，最后 `CombineEmitResults` 合并。
- **Rust 选型**：`rayon::par_iter`（或 scope）over `source_files`，每任务取线程局部 `TextWriter`（`thread_local!` 或对象池）。结果写各自 `emitter.emit_result`，**按输入顺序** `combine_emit_results`。
- **确定性保证**：`EmittedFiles`/`SourceMaps`/`Diagnostics` 按输入文件顺序合并（`core.Map(emitters, ...)` 保序）；写文件副作用本身是各文件独立路径，无序敏感。

### 并发原语小结（写进本包 `Cargo.toml` 依赖）

| 站点 | Go | Rust 首选 | 备选 |
|---|---|---|---|
| 文件加载动态 worklist | WorkGroup + SyncMap + 递归 start | `rayon::scope` 递归 spawn + `DashMap` | `std::thread::scope` + `crossbeam-channel` |
| 项目引用 | 同上 | 同上 | 同上 |
| 绑定/诊断 map | WorkGroup（固定 batch） | `rayon::par_iter` → 预分配 `Vec` | — |
| checker 池 | wg + per-checker Mutex | `rayon` over checkers + `Mutex<Checker>` | `parking_lot::Mutex` |
| emit | WorkGroup + writer sync.Pool | `rayon::par_iter` + `thread_local` writer | 对象池 |
| 计数聚合 | atomic | `AtomicI32/U32` | — |
| 并发 map 缓存 | SyncMap | `DashMap` | `Mutex<FxHashMap>` |

> **铁律**：所有并行收集后必须按稳定 key 排序/按输入顺序合并，使输出与 Go **逐字节一致**（`TestProgram` 的 fileNames 顺序、诊断顺序是断言对象）。第一遍若某并行点难直译，可先 `// PERF(port)` 写顺序版（`WorkGroup::Sequential`），保证绿后再并行化。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/compiler/program.go` | `internal/compiler/program.rs`（basename `program` ≠ crate 目录 `compiler`，但它是主文件）→ 见下 | Program 中枢：构建/诊断/emit/host 方法/verifyCompilerOptions |
| `internal/compiler/pkg.go` | `internal/compiler/lib.rs`（crate 根；Go 的 `pkg.go` 仅 package 注释） | crate 根 + `mod` 声明 + re-export；把 `program.rs` 等挂上来 |
| `internal/compiler/fileloader.go` | `internal/compiler/fileloader.rs` | fileLoader、processedFiles、解析引用/导入/lib/ATA、lib 替换、mode 计算 |
| `internal/compiler/filesparser.go` | `internal/compiler/filesparser.rs` | **并发点 1**：parseTask、filesParser、动态 worklist、collectFiles 后处理 |
| `internal/compiler/checkerpool.go` | `internal/compiler/checkerpool.rs` | **并发点 4**：CheckerPool trait + 内置池、分组并行 |
| `internal/compiler/emitter.go` | `internal/compiler/emitter.rs` | **并发点 5**：emitter、script/declaration transformers 编排、printSourceFile、sourceFileMayBeEmitted |
| `internal/compiler/emitHost.go` | `internal/compiler/emitHost.rs`（注意大小写 → `emit_host.rs`） | EmitHost trait + 实现（转发给 program + emit resolver） |
| `internal/compiler/host.go` | `internal/compiler/host.rs` | CompilerHost trait + compilerHost 默认实现（含 cachedvfs） |
| `internal/compiler/fileInclude.go` | `internal/compiler/file_include.rs` | FileIncludeReason、引用位置、各类"为什么包含"诊断/related-info |
| `internal/compiler/includeprocessor.go` | `internal/compiler/includeprocessor.rs` | includeProcessor：诊断聚合 + 各种缓存（SyncMap） |
| `internal/compiler/processingDiagnostic.go` | `internal/compiler/processing_diagnostic.rs` | processingDiagnostic、createDiagnosticExplainingFile |
| `internal/compiler/projectreferenceparser.go` | `internal/compiler/projectreferenceparser.rs` | **并发点 2**：项目引用并行解析 + initMapper |
| `internal/compiler/projectreferencefilemapper.go` | `internal/compiler/projectreferencefilemapper.rs` | source↔output 映射、redirect 解析、symlink 处理 |
| `internal/compiler/projectreferencedtsfakinghost.go` | `internal/compiler/projectreferencedtsfakinghost.rs` | "假装 .d.ts 存在"的 VFS（source-of-truth 模式） |

> 命名：Go 的 `emitHost.go`/`fileInclude.go`/`processingDiagnostic.go` 是 camelCase basename；Rust 文件名用 snake_case（`emit_host.rs`/`file_include.rs`/`processing_diagnostic.rs`），在 `lib.rs` 里 `mod emit_host;` 等。

## 依赖白名单（本包新增的 crate）

- `rayon`（已在 §10）——数据并行 + scope。
- `crossbeam-channel`（已在 §10）——若文件加载用 channel worker 池。
- `dashmap`（已在 §10）——SyncMap 等价。
- `parking_lot`（可选）——checker 池/缓冲锁，比 std 略快。
- `xxh3`——`DuplicateSourceFile.Hash`（源文件去重哈希，对齐 Go `zeebo/xxh3`）。
- 记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按"先 host/类型，后加载，再 program 编排，最后 emit"。

### `host.rs`（Go: `host.go`）

- [x] `pub trait CompilerHost: Send + Sync { fs / default_library_path / get_current_directory / get_source_file }`（P6-1：`fs` 返回 `Arc<dyn Fs+Send+Sync>`，对齐 Go `FS()` 的可共享句柄语义）　`// Go: host.go:CompilerHost`
  - [ ] `trace(msg, args)` — DEFER(P6) blocked-by: `tsgo_diagnostics` 的可变参 `Trace` 表示 + tracing 接线（P6-2）
  - [ ] `get_resolved_project_reference(...)` — DEFER(P6) blocked-by: `tsoptions::get_parsed_command_line_of_config_file_path`（未移植）
- [x] `pub struct CompilerHostImpl { current_directory, fs, default_library_path }`（`extended_config_cache`/`trace` 字段 DEFER 同上）　`// Go: host.go:compilerHost`
- [x] `pub fn new_compiler_host(...)`　`// Go: host.go:NewCompilerHost`
  - [ ] `new_cached_fs_compiler_host(...)`（cachedvfs 包裹）— DEFER(P6)，目前测试用裸 vfs，cachedvfs 接线留 P6-2
- [x] impl `get_source_file`（ReadFile→`parser::parse_source_file`，经 `parse_file` 辅助；返回 `ParsedFile`）　`// Go: host.go:compilerHost.GetSourceFile`
- [x] **新增 → P6-4 改 owned/`Rc`** `pub struct ParsedFile`（arena + 根 `NodeId` + 原文 + 诊断）= Go `*ast.SourceFile` 的本仓替身；含 `file_name/text/arena/node/diagnostics/import_specifiers` 访问器（见"已知偏离"）。P6-4：`arena: Rc<NodeArena>` + `bind: Option<Rc<BindResult>>`（让 `BoundFile` 能 owned/`'static`/`Rc`-share，喂给 `Checker::new_checker(Rc<dyn BoundProgram>)`）；新增 `pub(crate) arena_rc()` / `bind_rc()`
- [x] **P6-2 → P6-4 改** `ParsedFile::bind()` / `bind_result()` / `is_bound()`：经 `tsgo_binder::bind_source_file(&mut arena, node)` 产出并缓存 `BindResult`（幂等）；P6-4 `bind()` 用 `Rc::get_mut(&mut self.arena)`（bind 早于任何共享，arena 独占，`get_mut` 必成），缓存为 `Rc<BindResult>`　`// Go: program.go:BindSourceFiles（逐文件）`

### `fileloader.rs`（Go: `fileloader.go`）

- [x] `struct FileLoader { host, compare_paths_options, resolver }`（P6-1 可达子集；`default_library_path`/`supported_extensions`/`total/lib_file_count`/`factory`/`project_reference_file_mapper`/`dts_directories`/lib 缓存 DEFER）　`// Go: fileloader.go:fileLoader`
- [x] `struct ProcessedFiles { files, files_by_path, missing_files }`（P6-1 可达子集 + 访问器；`resolved_modules`/`source_file_meta_datas`/`include_processor`/redirect 等 DEFER）　`// Go: fileloader.go:processedFiles`
- [ ] `struct LibFile`/`libResolution`/`redirectsFile`(impl HasFileName)/`DuplicateSourceFile`/`jsxRuntimeImportSpecifier`/`sourceFileFromReferenceDiagnostic`　DEFER(P6) blocked-by: lib 解析 + 包去重（P6-2）　`// Go: fileloader.go`
- [x] `pub fn process_all_program_files(opts, single_threaded) -> ProcessedFiles`（P6-1 **编排入口**：建 loader → `NewResolver`（`LoaderResolutionHost`）→ 加 root 任务 → `FilesParser::parse` → `collect_files`；`addProjectReferenceTasks`/lib/ATA 任务 DEFER）　`// Go: fileloader.go:processAllProgramFiles`
- [x] `fn to_path`（已实现）；`add_root_task`（移至 `FilesParser`）
  - [ ] `add_automatic_type_directive_tasks` / `add_project_reference_tasks` — DEFER(P6) blocked-by: ATA/项目引用（P6-2）
- [x] `fn parse_source_file(normalized_file_path) -> Option<ParsedFile>`（经 host）；`resolve_import_file_names(file)`（逐 import specifier 调 `module::Resolver::resolve_module_name`）　`// Go: fileloader.go:resolveImportsAndModuleAugmentations`
- [x] `struct LoaderResolutionHost`（impl `module::ResolutionHost`，桥接 host 的 fs + cwd 给 resolver）
- [x] **P6-2** `pub fn import_syntax_affects_module_resolution(options) -> bool`（纯函数：node16/nodenext 或 resolvePackageJson exports/imports）+ 单测　`// Go: fileloader.go:importSyntaxAffectsModuleResolution`
- [x] **P6-2** `fn get_default_resolution_mode_for_file(options)`（可达子集：不影响时为 `None`；影响分支 DEFER）；`resolve_import_file_names` 改用它取 mode
  - [ ] `get_mode_for_usage_location(file, meta, usage, options)`（per-import 精确 mode：type-only `resolution-mode` 覆盖 / `import()` 调用语法）— DEFER(P6) **blocked-by: ast `SourceFileMetaData` + `GetImpliedNodeFormatForEmitWorker`/`GetEmitModuleFormatOfFileWorker`（未在 `tsgo_ast` 移植）**
- [ ] `fn resolve_automatic_type_directives(...)`　`// Go: fileloader.go:resolveAutomaticTypeDirectives`
- [ ] `fn sort_libs(libs)` / `get_default_lib_file_priority(file)`（lib 优先级，确定性排序）　`// Go: fileloader.go`
- [ ] `fn load_source_file_meta_data(file_name) -> SourceFileMetaData`（package.json scope/type/impliedNodeFormat）— DEFER(P6) **blocked-by: ast `SourceFileMetaData`/`GetImpliedNodeFormatForEmitWorker` 未移植（不可编辑 `internal/ast/**`）**　`// Go: fileloader.go:loadSourceFileMetaData`
- [ ] `fn parse_source_file(t) -> Option<SourceFile>`（含 tracing Push("createSourceFile")）　`// Go: fileloader.go:parseSourceFile`
- [ ] `fn is_supported_extension` / `get_source_file_from_reference(...)` / `resolve_tripleslash_path_reference(...)`　`// Go: fileloader.go`
- [ ] `fn resolve_type_reference_directives(t)`（含 tracing）　`// Go: fileloader.go:resolveTypeReferenceDirectives`
- [ ] `fn resolve_imports_and_module_augmentations(t)`（**导入解析**：importHelpers/jsx runtime 合成 import、逐 import 解析、shouldAddFile 判定、含 tracing）　`// Go: fileloader.go:resolveImportsAndModuleAugmentations`
- [ ] `fn create_synthetic_import(text, file)`（`factoryMu` 锁内建节点）　`// Go: fileloader.go:createSyntheticImport`
- [ ] `fn path_for_lib_file(name) -> LibFile`（含 libReplacement 解析，DashMap 缓存）/`resolve_library(...)`/`get_library_name_from_lib_file_name(...)`/`get_inferred_library_name_resolve_from(...)`　`// Go: fileloader.go`
- [ ] 自由函数：`get_mode_for_type_reference_directive_in_file` / `get_default_resolution_mode_for_file` / `get_mode_for_usage_location` / `import_syntax_affects_module_resolution` / `get_emit_syntax_for_usage_location_worker`　`// Go: fileloader.go`

### `filesparser.rs`（Go: `filesparser.go`）——**并发点 1**

- [x] `struct ParseTask { normalized_file_path, path, file, sub_tasks, loaded }`（P6-1 可达子集；`lib_file`/`redirected`/`is_for_ata`/`include_reason`/`package_id`/`metadata`/`resolutions*`/`processing_diagnostics`/`increase_depth`/`elide_on_depth`/`loaded_task` 等 DEFER）　`// Go: filesparser.go:parseTask`
- [ ] `fn redirect(...)` / `fn load_automatic_type_directives(...)` — DEFER(P6) blocked-by: 项目引用 + ATA（P6-2）
- [ ] `struct ResolvedRef { ... }` — DEFER(P6)（P6-1 直接用解析出的 file name + path）
- [x] `struct FilesParser { tasks_by_path: HashMap<Path, ParseTask>, root_paths }`（**P6-1 顺序版**：`// PERF(port)` Go 用 `WorkGroup` 并行 worklist；确定性来自串行 `collect_files` 后处理，故顺序版同序，并行化留后续轮）　`// Go: filesparser.go:filesParser`
- [ ] `struct ParseTaskData { ... mu, lowest_depth, started_sub_tasks, package_id }` + 对象池 — DEFER(P6) blocked-by: 并行 worklist（per-path Mutex/深度去重）（P6-2+）
- [x] `fn add_root_task(loader, normalized_file_path)` + `fn parse(&mut self, loader)`（顺序 worklist：解析每个 task → `resolve_import_file_names` 发现子任务 → 跟进直至排空）　`// Go: filesparser.go:filesParser.parse/start`
  - [ ] depth/elideOnDepth/casing/redirect 多 task 等 `start` 细节 — DEFER(P6)
- [x] `fn collect_files(self) -> ProcessedFiles`（**串行后处理**：`collect_post_order` 深度优先 + seen 去重，import 先于 referrer；missing → `missing_files`）；大小写/包去重诊断 / redirect/ATA/lib 分流 / sortLibs / lib 解析合并 DEFER　`// Go: filesparser.go:filesParser.getProcessedFiles`
- [ ] `fn add_include_reason(...)` — DEFER(P6) blocked-by: includeProcessor（P6-2）

### `projectreferenceparser.rs`（Go: `projectreferenceparser.go`）——**并发点 2**

- [ ] `struct ProjectReferenceParseTask { config_name, resolved, sub_tasks }` + `parse(...)`（含 tracing；`ParseInputOutputNames`；递归子引用）　`// Go: projectreferenceparser.go`
- [ ] `fn create_project_reference_parse_tasks(refs)`　`// Go: projectreferenceparser.go`
- [ ] `struct ProjectReferenceParser { loader, wg, tasks_by_file_name: DashMap }` + `parse`/`start`（并行）/`init_mapper`（单线程）/`init_mapper_worker`（深度优先 seen 去重，子覆盖父）　`// Go: projectreferenceparser.go`

### `projectreferencefilemapper.rs`（Go: `projectreferencefilemapper.go`）

- [ ] `struct ProjectReferenceFileMapper { opts, host, loader, configToProjectReference, referencesInConfigFile, source/outputDtsToProjectReference, realpathDtsToSource: DashMap }`　`// Go: projectreferencefilemapper.go`
- [ ] 方法：`get_parse_file_redirect` / `get_resolved_project_references` / `get_project_reference_from_source` / `..._from_output_dts` / `is_source_from_project_reference` / `get_compiler_options_for_file` / `get_redirect_*_for_resolution` / `get_resolved_reference_for` / `range_resolved_project_reference(_in_child_config)` / `range_resolved_reference_worker` / `get_source_to_dts_if_symlink`　`// Go: projectreferencefilemapper.go`

### `projectreferencedtsfakinghost.rs`（Go: `projectreferencedtsfakinghost.go`）

- [ ] `struct ProjectReferenceDtsFakingHost { host, fs: cachedvfs }` impl `module::ResolutionHost`　`// Go: projectreferencedtsfakinghost.go`
- [ ] `struct ProjectReferenceDtsFakingVfs { mapper, dts_directories, known_symlinks }` impl `vfs::FS`（FileExists/DirectoryExists/Realpath 重写；多数方法 `unreachable!`）　`// Go: projectreferencedtsfakinghost.go`
- [ ] `fn file_or_directory_exists_using_source(...)` / `file_exists_if_project_reference_dts(...)` / `directory_exists_if_project_reference_decl_dir(...)` / `handle_directory_could_be_symlink(...)`　`// Go: projectreferencedtsfakinghost.go`

### `file_include.rs`（Go: `fileInclude.go`）

- [ ] `enum FileIncludeKind { Import, ReferenceFile, TypeReferenceDirective, LibReferenceDirective, RootFile, LibFile, AutomaticTypeDirectiveFile }`（注意 `is_referenced_file = kind <= LibReferenceDirective`）　`// Go: fileInclude.go`
- [ ] `struct FileIncludeReason { kind, data, relative_diag: OnceLock, diag: OnceLock }` + `struct referencedFileData`/`automaticTypeDirectiveFileData`/`referenceFileLocation`　`// Go: fileInclude.go`
- [ ] `referenceFileLocation::text/diagnostic_at`　`// Go: fileInclude.go`
- [ ] `FileIncludeReason::get_referenced_location` / `to_diagnostic` / `compute_diagnostic` / `compute_reference_file_diagnostic` / `to_related_info` / `compute_reference_file_related_info`（大量诊断消息分支：imported via / referenced via / lib / ATA / root）　`// Go: fileInclude.go`

### `processing_diagnostic.rs`（Go: `processingDiagnostic.go`）

- [ ] `enum ProcessingDiagnosticKind { UnknownReference, ExplainingFileInclude }` + `struct ProcessingDiagnostic { kind, data }` + `struct includeExplainingDiagnostic`　`// Go: processingDiagnostic.go`
- [ ] `to_diagnostic(program)` / `create_diagnostic_explaining_file(program)`（含 related info / redirect info / preferred location）　`// Go: processingDiagnostic.go`

### `includeprocessor.rs`（Go: `includeprocessor.go`）

- [ ] `struct IncludeProcessor { fileIncludeReasons, processingDiagnostics, reasonToReferenceLocation/includeReasonToRelatedInfo/redirectAndFileFormat: DashMap, computedDiagnostics: OnceLock, compilerOptionsSyntax: OnceLock }`　`// Go: includeprocessor.go`
- [ ] `update_file_include_processor(program)` / `get_diagnostics(program)`（聚合 processing + 模块/类型解析诊断）/`add_processing_diagnostic(s)` / `add_processing_diagnostics_for_file_casing(...)` / `get_reference_location` / `get_compiler_options_object_literal_syntax` / `get_related_info` / `explain_redirect_and_implied_format`　`// Go: includeprocessor.go`

### `checkerpool.rs`（Go: `checkerpool.go`）——**并发点 4**

- [x] `pub fn checker_count(single_threaded, configured: Option<i32>, file_count) -> usize`（**纯函数**：base = single_threaded?1:configured.unwrap_or(4)，再 `clamp(1, min(files,256))`）+ 全量子用例单测　`// Go: checkerpool.go:newCheckerPoolWithTracing`
- [x] `struct CompilerCheckerPool { checker_count, checkers, file_associations }` + `new(...)` + `checker_count()`　`// Go: checkerpool.go:checkerPool`
- [x] **P6-2 → P6-4 改** `fn create_checkers(&mut self, files: &[ParsedFile])`：`i%K` 关联 + 经公共 seam `Checker::new_checker(Rc<dyn BoundProgram>)`（4l 改 retain 模型）把首个 bound 文件的 `BoundFile`（owned）`Rc::clone` 进 K 个 checker（**all sharing one program**，幂等）　`// Go: checkerpool.go:createCheckers`
- [x] **P6-2** `created_checker_count()` / `checker_index_for_file(i)` / `files_for_checker(checker_index, file_count)`（`forEachCheckerGroupDo` 的分组形状，纯）+ 单测　`// Go: checkerpool.go:forEachCheckerGroupDo`
- [x] **P6-2 新增 → P6-4 改 owned** `boundfile.rs::BoundFile` impl `tsgo_checker::BoundProgram`（arena/root/symbol/locals/flow 桥接 `BindResult`）；P6-4 去掉 `<'a>` 借用，改持 `Rc<NodeArena>`+`Rc<BindResult>`（owned/`'static`/`Rc`-shareable，可进 `Rc<dyn BoundProgram>`）+ 单测
- [x] **P6-4 新增** `fn collect_diagnostics(&mut self) -> Vec<Diagnostic>`：驱动共享程序——取 program.root()，经文件 0 关联的 checker（`0%K`）调 `Checker::get_diagnostics(root)`（自动 `checkSourceFile`），返回该文件语义诊断（端到端真出 2304/2339）+ 单测　`// Go: checkerpool.go:forEachCheckerGroupDo + program.go:getDiagnostics`
- [ ] `pub trait CheckerPool { get_checker(...) }` / `for_each_checker_parallel` / `get_global_diagnostics` / 真正的**多文件分组并行诊断收集** — DEFER(P6) **blocked-by: 多文件 `BoundProgram` view + parallel `Arc` checker（PORTING §6）+ lib globals** —— 4l 已放开 retain `new_checker(Rc)` + per-file `check_source_file`/`get_diagnostics`，pool 现已能驱动检查（见 `collect_diagnostics`）；但 `BoundProgram` 仍是**单文件**（一个 program = 一个 bound 文件），且 checker 持 `Rc`（非 `Arc`），故 (a) 真正的多文件 per-file 收集 + `GetDiagnosticsForFile(name)` 过滤、(b) 跨 checker 并行（`Arc`），仍待多文件 program view 与并行化

### `emit_host.rs`（Go: `emitHost.go`）

- [ ] `pub trait EmitHost: printer::EmitHost + declarations::DeclarationEmitHost { Options / SourceFiles / UseCaseSensitiveFileNames / GetCurrentDirectory / CommonSourceDirectory / IsEmitBlocked }`（**must be thread-safe**）　`// Go: emitHost.go:EmitHost`
- [ ] `struct EmitHostImpl { program, emit_resolver }` + `new_emit_host(cancel, program, file) -> (EmitHostImpl, done)`（从 non-exclusive checker 取 emit resolver）　`// Go: emitHost.go`
- [ ] 全部转发方法（GetModeForUsageLocation/GetResolvedModule.../GetOutputPathsFor/WriteFile/GetEmitResolver/...）　`// Go: emitHost.go`

### `emitter.rs`（Go: `emitter.go`）——**并发点 5 的单文件逻辑**

- [x] **P6-3** `pub enum EmitOnly { All, Js, Dts, ForcedDts }`　`// Go: emitter.go`
- [x] **P6-3** `emit_js_text(file_name, source_text, options) -> String`（**transform + print 核心**：重解析 → 跑可达脚本 transformer 链 → 经 `tsgo_printer::Printer::emit_source_file` 出 JS 文本）　`// Go: emitter.go:emitter.emitJSFile（核心）`
- [x] **P6-3** `run_script_transformers(ec, sf, options)`（**可达子集 = 仅 type eraser**；其余 stage DEFER 见下）　`// Go: emitter.go:emitter.runScriptTransformers / getScriptTransformers`
  - [ ] `get_module_transformer` / 其余 `getScriptTransformers` stage（metadata/importElision/runtimeSyntax/legacyDecorators/jsx/es downlevel/useStrict/module/constEnum inlining）— DEFER(P6) **blocked-by: checker `EmitResolver` + 未移植的 transformer 工厂**（Rust `tstransforms` 仅 `typeeraser` 可达，`runtimesyntax`/`legacydecorators`/`metadata`/`importelision` 均 DEFER；module/jsx/es 需 resolver）
- [x] **P6-3** `struct Emitter` 的可达字段折叠进 `Program::emit_one`/`emit_js_file`（无独立 `EmitHost`：checker 桩，emit resolver 不可达）；`emitter_diagnostics`/`writer` 池/`tr` tracing DEFER
- [ ] `fn emit(&mut self)` 的 declaration 半边 / `get_declaration_transformers` / `run_declaration_transformers` / `emit_declaration_file`　DEFER(P6) **blocked-by: declarations transformer + checker `EmitResolver`**　`// Go: emitter.go`
- [x] **P6-3** `emit_js_file`（可达子集：emit_only 守卫 + `no_emit` skip + BOM + writeText）/ `write_text`（writeFile 回调 / host fs 回退）（在 `program.rs`）　`// Go: emitter.go:emitter.emitJSFile/writeText`
  - [ ] `print_source_file` 的 sourcemap 半边（生成、URL、写 `.map`）— DEFER(P6) **blocked-by: `tsgo_printer::Printer` 不驱动 `sourcemap::Generator`（`emit_source_file` 无 generator 形参，Go `printer.Write` 有）**
- [ ] `should_emit_source_maps` / `should_emit_declaration_source_maps` / `get_source_root` / `get_source_map_directory` / `get_source_mapping_url`　DEFER(P6) blocked-by: 同上 sourcemap　`// Go: emitter.go`
- [x] **P6-3** `fn source_file_may_be_emitted(file, force_dts) -> bool`（可达子集：跳过 `.d.ts`；forceDts 直 emit）+ `get_source_files_to_emit(target, force_dts)`（按输入序，支持单文件 target）（在 `program.rs`）　`// Go: emitter.go:sourceFileMayBeEmitted/getSourceFilesToEmit`
  - [ ] `SourceFileMayBeEmittedHost` trait / `noEmitForJsFiles`·external-library·project-reference·json-without-`outDir` 分支 / `is_source_file_not_json` / `get_declaration_diagnostics`　DEFER(P6) blocked-by: checker/program 状态 + declarations transformer　`// Go: emitter.go`
- [x] **P6-3** impl `tsgo_outputpaths::SourceFileLike for ParsedFile`（file_name + script_kind）→ 经 `get_output_paths_for` 取 `.js` 路径（`host.rs`）

### `program.rs`（Go: `program.go`，最大）

类型 / 构造：

- [x] `pub struct ProgramOptions { host: Arc<dyn CompilerHost>, config: Arc<ParsedCommandLine>, single_threaded: bool }`（P6-1 可达子集；`use_source_of_project_reference`/`create_checker_pool`/`typings_location`/`project_name`/`tracing` DEFER）　`// Go: program.go:ProgramOptions`
- [ ] `struct LazyValue<T>` / `struct PackageNamesInfo` — DEFER(P6) blocked-by: ATA/auto-imports（P6-2+）
- [x] `pub struct Program { opts, processed: ProcessedFiles, checker_pool: CompilerCheckerPool, compare_paths_options }`（P6-1 骨架；`compiler_checker_pool`/`common_source_directory`/`declaration_diagnostic_cache`/`program_diagnostics`/`source_files_to_emit`/`unresolved_imports`/`known_symlinks`/`package_names`/`has_ts_file`/`packages_map` DEFER）　`// Go: program.go:Program`
- [x] `pub fn new_program(opts) -> Program`（**编排入口**：`process_all_program_files` → 内置 `CompilerCheckerPool` 计数 →（**P6-2**）`verify_compiler_options`；`tracing Push`/`init_checker_pool` 注入 DEFER）　`// Go: program.go:NewProgram`
- [x] 访问器：`options` / `command_line` / `host` / `single_threaded` / `source_files` / `get_source_file` / `get_source_file_by_path` / `to_path` / `checker_pool` / `missing_files`　`// Go: program.go`
- [x] **P6-2** `bind_source_files(&mut self)`（逐文件 `ParsedFile::bind`，顺序；`// PERF(port)` 可并行）/ `create_checkers(&mut self)`（bind 后建 pool）/ `options_diagnostics()`　`// Go: program.go:BindSourceFiles + initCheckerPool`
- [ ] `pub fn update_program(...)`（增量复用）— DEFER(P6) blocked-by: redirect 组 + LazyValue 复用（P6-2+）
- [ ] `fn init_checker_pool(&mut self)`（CreateCheckerPool 注入）— DEFER(P6)（P6-1 直接内置 `CompilerCheckerPool::new`）
- [ ] `fn can_replace_file_in_program` + `equal_*` — DEFER(P6) blocked-by: UpdateProgram（P6-2+）

实现 checker.Program 等 trait 的访问器（约 40 个）：

- [ ] FileExists/GetCurrentDirectory/GetGlobalTypingsCacheLocation/GetNearestAncestorDirectoryWithPackageJson/GetPackageJsonInfo/GetRedirectTargets/GetSourceOfProjectReferenceIfOutputIncluded/GetProjectReferenceFromSource/IsSourceFromProjectReference/GetProjectReferenceFromOutputDts/GetResolvedProjectReferenceFor/GetRedirectForResolution/GetParseFileRedirect/GetResolvedProjectReferences/RangeResolvedProjectReference(_InChildConfig)/UseCaseSensitiveFileNames/UsesUriStyleNodeCoreModules　`// Go: program.go`
- [ ] GetSourceFileFromReference/SourceFiles/DuplicateSourceFiles/Options/CommandLine/Host/Tracing/GetConfigFileParsingDiagnostics　`// Go: program.go`
- [ ] GetUnresolvedImports/extractUnresolvedImports(FromSourceFile)/GetResolvedModule(FromModuleSpecifier)/GetResolvedModules/GetPackagesMap　`// Go: program.go`
- [ ] SourceFile 元信息：GetSourceFileMetaData/GetEmitModuleFormatOfFile/GetEmitSyntaxForUsageLocation/GetImpliedNodeFormatForEmit/GetModeForUsageLocation/GetDefaultResolutionModeForFile　`// Go: program.go`
- [ ] lib/默认库：IsSourceFileDefaultLibrary/IsGlobalTypingsFile/GetDefaultLibFile/IsLibFile/GetLibFileFromReference　`// Go: program.go`
- [ ] 源文件查询：toPath/GetSourceFile(ByPath/ForResolvedModule)/FilesByPath/HasSameFileNames/GetSourceFiles/GetIncludeReasons/IsMissingPath/ExplainFiles　`// Go: program.go`
- [ ] 解析查询：GetResolvedTypeReferenceDirective*/getModeForTypeReferenceDirectiveInFile/IsSourceFileFromExternalLibrary/GetJSXRuntimeImportSpecifier/GetImportHelpersImportSpecifier/ResolveModuleName/ForEachResolvedModule/ForEachResolvedTypeReferenceDirective/forEachResolution　`// Go: program.go`
- [ ] 包名/符号：ResolvedPackageNames/UnresolvedPackageNames/DeepImportPackageNames/collectPackageNames/HasTSFile/GetSymlinkCache　`// Go: program.go`
- [ ] 计数：LineCount/IdentifierCount/SymbolCount/TypeCount/InstantiationCount/Program　`// Go: program.go`

诊断（含**并发**）：

- [x] `bind_source_files(&mut self)`（**P6-2**：顺序 bind；并行 + tracing 留后续）　`// Go: program.go:BindSourceFiles`
- [x] **P6-4** `semantic_diagnostics(&mut self) -> Vec<tsgo_checker::Diagnostic>`：`create_checkers()`（bind + 建池）→ `checker_pool.collect_diagnostics()`，端到端真出语义诊断（`y;` → 2304；interface 缺成员 → 2339）+ doctest　`// Go: program.go:GetSemanticDiagnostics`
  - [ ] per-file 过滤 / suggestion·declaration 诊断 / `@ts-expect-error`·`@ts-ignore` 指令 — DEFER(P6) blocked-by: 多文件 `BoundProgram` + checker 指令面
- [ ] `get_type_checker(cancel)` / `for_each_checker_parallel(cb)` / `get_type_checker_for_file(_exclusive)(cancel, file)`　`// Go: program.go`
- [ ] `collect_diagnostics(...)` / `collect_diagnostics_from_files(...)`（**并行** map + 稳定排序）　`// Go: program.go`
- [ ] `collect_checker_diagnostics(...)` / `collect_checker_diagnostics_from_files(...)`（**分组并行**或 per-file）　`// Go: program.go`
- [ ] `get_syntactic_diagnostics` / `get_additional_js_syntactic_diagnostics`（JS 参数装饰器）/`get_bind_diagnostics`/`get_semantic_diagnostics`/`get_semantic_diagnostics_without_no_emit_filtering`/`get_suggestion_diagnostics`/`get_program_diagnostics`/`get_include_processor_diagnostics`/`get_global_diagnostics`/`get_declaration_diagnostics`　`// Go: program.go`
- [ ] `skip_type_checking` / `can_include_bind_and_check_diagnostics`　`// Go: program.go`
- [ ] `get_semantic_diagnostics_with_checker` / `get_bind_and_check_diagnostics_with_checker`（@ts-ignore/@ts-expect-error 指令处理）/`get_diagnostics_with_preceding_directives`/`get_declaration_diagnostics_for_file`（DashMap 缓存）/`get_suggestion_diagnostics_with_checker`　`// Go: program.go`
- [ ] `is_comment_or_blank_line` / `SortAndDeduplicateDiagnostics` / `compact_and_merge_related_infos`　`// Go: program.go`
- [ ] `static PLAIN_JS_ERRORS`（plain JS 允许的诊断 code 集合，~100 项）　`// Go: program.go:plainJSErrors`

verifyCompilerOptions（~400 行，program 构建的选项一致性诊断）：

- [x] **P6-2 新增** `verify_options.rs::verify_compiler_options(&CompilerOptions) -> Vec<OptionsDiagnostic>`（**纯函数**，可达子集）：removed（outFile / target ES5 / module AMD·System·UMD / moduleResolution Classic·node10）+ 配对（strictPropertyInitialization·exactOptionalPropertyTypes 需 strictNullChecks / lib+noLib / checkJs 需 allowJs / emitDecoratorMetadata 需 experimentalDecorators）。每条 red→green 单测；经 `new_program` 暴露为 `Program::options_diagnostics()`　`// Go: program.go:verifyCompilerOptions`
  - [ ] 带源位置的诊断（指向 tsconfig 节点）/ 依赖 program 状态的规则（outDir/rootDir 布局、paths `*`、project references、common-source-directory）— DEFER(P6) blocked-by: tsconfig option-syntax AST（`tsoptions` 配置文件 AST）+ `Program` common-source-directory/emit 接线
- [ ] `block_emitting_of_file` / `is_emit_blocked` / `verify_project_references` / `has_zero_or_one_asterisk_character` / `module_resolution_supports_package_json_exports_and_imports` / `emit_module_kind_is_non_node_esm`　`// Go: program.go`
- [ ] `common_source_directory(&self)`（OnceLock）/`check_source_files_belong_to_path`　`// Go: program.go:CommonSourceDirectory`
- [ ] `get_source_files_to_emit(target, force_dts)`（OnceLock for 全量）　`// Go: program.go:getSourceFilesToEmit`

emit（**并发点 5**）：

- [x] **P6-3** `struct WriteFileData`（`source_map_url_pos`/`skipped_dts_write` 子集；`BuildInfo`/诊断 DEFER）/ `type WriteFileCallback` / `struct EmitOptions`（`target_source_file`/`emit_only`/`write_file`）/ `struct EmitResult`（`emit_skipped`/`diagnostics`/`emitted_files`/`source_maps`）/ `struct SourceMapEmitResult`（shape 占位，payload DEFER）　`// Go: program.go`
- [x] **P6-3** `pub fn emit(&self, options) -> EmitResult`（**可达子集**：getSourceFilesToEmit（按输入序）→ 逐文件 transform+print JS → writeFile/host fs 写 → 按序 `combine_emit_results`）　`// Go: program.go:Emit`
  - [ ] **并行** emit + writer 池 + tracing + `HandleNoEmitOnError` 前置 — DEFER(P6) **blocked-by: checker 语义诊断 API**（顺序版 `// PERF(port)`；确定性已由按输入序合并保证）
- [x] **P6-3** `pub fn combine_emit_results(results) -> EmitResult`（emit_skipped OR + emittedFiles/sourceMaps/diagnostics 顺序拼接）　`// Go: program.go:CombineEmitResults`
- [ ] `pub trait ProgramLike` + `fn handle_no_emit_on_error(...)` + `fn get_diagnostics_of_any_program(...)`　DEFER(P6) blocked-by: checker 语义诊断　`// Go: program.go`
- [ ] `fn filter_no_emit_semantic_diagnostics(diags, options)`　DEFER(P6) blocked-by: checker　`// Go: program.go:FilterNoEmitSemanticDiagnostics`

### `lib.rs`（Go: `pkg.go`，crate 根）

- [ ] crate 文档注释 + `mod` 声明全部模块 + re-export 公开 API（`Program`/`ProgramOptions`/`NewProgram`/`CompilerHost`/`NewCompilerHost`/`EmitOptions`/`EmitResult`/`CheckerPool`/`ProgramLike`/...）　`// Go: pkg.go`

### Cargo / crate 接线

- [ ] `internal/compiler/Cargo.toml`（`name = "tsgo_compiler"` + 全部 path deps + `rayon`/`dashmap`/`crossbeam-channel`/`xxh3`）
- [ ] 根 `Cargo.toml` workspace members 追加

## TDD 推进顺序（tracer bullet → 增量）

1. **host + 最小 program**：`new_compiler_host` + `new_program`（顺序 `WorkGroup::Sequential`）解析一个无依赖文件 → `GetSourceFiles()` 含该文件 + lib（tracer bullet）。
2. **文件排序**：triple-slash reference 链 / import 链 / 循环 → fileNames 顺序与 `TestProgram` 的 `expectedFiles` 一致（**先顺序版保证确定性**，再并行化保持一致）。
3. **并行化文件加载**：把 `WorkGroup` 切到 `rayon`，重跑 `TestProgram` 确认顺序不变（确定性铁律）。
4. **checker 池**：建池 + `i%K` 分配 + 分组并行收集诊断（需 checker P4）。
5. **emit**：单文件 emit JS（`BenchmarkEmitLongLines` 的功能等价：emit 一个 long-line 文件不崩、输出含 sourcemap），再并行多文件（`BenchmarkEmitManyFiles` 等价）。
6. **verifyCompilerOptions**：逐条选项诊断（removed/互斥/paths/module-resolution）。
7. **project references / source-of-truth / faking VFS**：留到 module(P4)/build 场景成熟。

## P6-1 实施记录（worklog + 编译器 round plan）

> 本节由 P6-1（foundation）落地后补写，记录已落地范围、red→green 推进顺序、以及编译器分轮计划。
> **本轮只做地基**：host / 文件加载-解析 / `Program` 骨架 / checker 池计数桩。**不做 emitter**（emitter 等 transformers crate 更成熟后单独成轮）。

### 编译器 round plan

| 轮次 | 范围 | 关键依赖 / blocked-by |
|---|---|---|
| **P6-1（本轮，✅）** | `host.rs`（CompilerHost + ParsedFile）、`fileloader.rs`（ProcessedFiles + process_all_program_files，单文件/多 root/可解析相对 import 子集）、`filesparser.rs`（顺序 worklist + 确定性 collect_files）、`program.rs`（Program 骨架 + 访问器）、`checkerpool.rs`（checker 数 clamp 桩） | 已绿，仅依赖 P1–P4 已绿 crate（vfs/parser/ast/core/tspath/tsoptions/module） |
| **P6-2（本轮，✅ 部分）** | 逐文件 bind（`ParsedFile::bind` / `Program::bind_source_files`）；`BoundFile` impl `tsgo_checker::BoundProgram`；`CompilerCheckerPool::create_checkers`（`i%K` 关联 + 经公共 seam 真正构造 checker + 分组形状 `files_for_checker`）；`verify_compiler_options` 可达纯规则子集（经 `new_program` 暴露）；`import_syntax_affects_module_resolution` + `get_default_resolution_mode_for_file` 子集 | 已绿。**仍 blocked**：(a) 真正驱动检查/诊断 — **checker public API**（`new_checker` 忽略实参 + 无 per-file check/diagnostics 入口）；(b) 精确 import mode / lib 加载 — **ast `SourceFileMetaData`/impliedNodeFormat 未移植**；(c) lib 集合 — `tsgo_bundled`（dev-dep，非主依赖） |
| **P6-3（本轮，✅ 部分）** | ① 重新加回 `tsgo_transformers` 依赖（已接回，build 绿）；② emitter 可达子集（`emitter.rs::emit_js_text` + `Program::emit`/`emit_one`/`emit_js_file`/`write_text` + `source_file_may_be_emitted`/`get_source_files_to_emit`）：transform（**仅 type eraser**）+ printer，按输入序 `combine_emit_results`，端到端 emit JS 文本（`const x: number = 1;` → `const x = 1;\n`）；③ `EmitResult`/`EmitOptions`/`WriteFileData`/`SourceMapEmitResult` 类型；④ `no_emit` skip / BOM / newline(CRLF) / 单文件 target / host fs 回退 | 已绿。**仍 blocked**：(a) sourcemap — **printer 不驱动 `sourcemap::Generator`**；(b) declaration（`.d.ts`）emit + `EmitHost`/emit resolver — **checker public API + declarations transformer**；(c) 其余 `getScriptTransformers` stage（importElision/runtimeSyntax/jsx/module/...）— **checker `EmitResolver` + 未移植工厂**；(d) **并行** emit + writer 池 + `HandleNoEmitOnError` — **checker 语义诊断 API** |
| **P6-4（本轮，✅ 部分）** | ① **适配 checker 4l 的 program-retaining API**（`new_checker(Rc<dyn BoundProgram>)`）：`ParsedFile` 改 `Rc<NodeArena>`+`Rc<BindResult>`，`BoundFile` 改 owned/`'static`/`Rc`-shareable（去 `<'a>`），恢复 `-p tsgo_compiler` 编译；② checker 池**真正驱动检查**：`create_checkers` 把单一 `Rc<BoundFile>` `Rc::clone` 进 K 个 checker（all sharing one program），`collect_diagnostics` 经 `get_diagnostics(root)` 端到端真出 2304/2339；③ `Program::semantic_diagnostics` 串起 bind→建池→收诊断 | 已绿（49 单测 + 9 doctest）。**仍 blocked**：见下表 P6-4 DEFER —— 多文件 per-file 收集（单文件 `BoundProgram`）、跨 checker 并行（`Rc`≠`Arc`）、emit resolver / declaration emit / sourcemap |
| **P6-5（下轮，建议）** | ① **并行化** emit + 跨 checker 诊断收集（待 checker 切 `Arc<dyn BoundProgram + Send + Sync>`）；② 多文件 `BoundProgram` view + `GetDiagnosticsForFile(name)` 过滤；③ emit resolver → declaration（`.d.ts`）emit / importElision / 完整 `getScriptTransformers`；④ project references + source-of-truth faking VFS + UpdateProgram；⑤ sourcemap（待 printer 驱动 generator） | blocked-by: checker `Arc` 程序 + emit resolver / 多文件 program；printer source-map emission；`tsoptions::get_parsed_command_line_of_config_file_path` + module redirect |

### P6-1 red→green worklog（逐行为）

1. **host tracer**（`host_returns_cwd_and_file_contents`）：RED（`todo!()` 访问器）→ GREEN（`fs`/`get_current_directory` 返回字段）。
2. **host.get_source_file**（`host_parses_source_file` + `host_missing_source_file_is_none`）：RED（缺 `ParsedFile::file_name/text`）→ GREEN（`parse_file` 辅助 + `ParsedFile` 访问器）。
3. **单 root 加载**（`loads_single_root_file`）：RED（`process_all_program_files` `todo!()`）→ GREEN（最小 root 循环）。
4. **missing / 多 root 顺序**（`records_missing_root_file` / `loads_multiple_roots_in_order`）：GREEN（同循环覆盖）。
5. **可解析相对 import**（`loads_resolved_relative_import`）：RED（只加载 root）→ GREEN（接入 `module::Resolver` + `FilesParser` worklist + `collect_files`）。
6. **import 环**（`loads_import_cycle_once`）+ **collect 确定性**（`collect_orders_imports_before_referrer` / `collect_dedups_diamond` / `collect_handles_cycle`）：GREEN（seen 去重 + 后序）。
7. **checker 数 clamp**（6 个子用例）：表驱动，expected 取 Go `max(min(...,files,256),1)` 字面逻辑。
8. **Program 骨架**（`builds_program_from_single_file`）：RED（`new_program` `todo!()`）→ GREEN（`process_all_program_files` + `CompilerCheckerPool::new`）；再补 `looks_up_source_file_by_name` / `builds_multi_file_program_and_sizes_pool` / `single_threaded_program_uses_one_checker` 覆盖访问器。

**自检门禁（仅 `-p tsgo_compiler`）**：`cargo test`（21 单测 + 3 doctest 全绿）/ `cargo clippy --all-targets -- -D warnings`（净）/ `cargo fmt --check`（净）。

### P6-2 red→green worklog（逐行为）

1. **逐文件 bind**（`binding_a_file_yields_its_symbol_table`）：RED（`ParsedFile::bind` `todo!()`）→ GREEN（`tsgo_binder::bind_source_file` + 缓存 `BindResult`）。
2. **Program bind 全量**（`bind_source_files_binds_every_file`）：RED（缺方法）→ GREEN（`ProcessedFiles::files_mut` + `Program::bind_source_files`）。
3. **BoundProgram 桥**（`bound_file_exposes_arena_root_and_symbols` / `unbound_file_has_no_bound_view`）：RED（trait 方法 `todo!()`）→ GREEN（`BoundFile` 镜像 checker 的 `StubProgram` 实现）。
4. **checker pool 真正建 checker + 关联**（`create_checkers_associates_files_round_robin`）：RED（缺方法）→ GREEN（`create_checkers` 经 `Checker::new_checker(&BoundFile)` + `i%K` + `files_for_checker`）。
5. **verify_compiler_options**（`out_file_is_removed` → `target_es5_is_removed` → `removed_module_kinds` → `strict_property_initialization_requires_strict_null_checks` → `lib_cannot_be_used_with_no_lib` → `check_js_requires_allow_js` → `emit_decorator_metadata_requires_experimental_decorators`，逐条 red→green；`default_options_are_clean` 守护）。注：`check_js` 一例先 RED 暴露了 `checkJs` 隐含 `allowJs` 的真实语义，据此修正测试 expected（ground truth 取自 `get_allow_js`）。
6. **program 集成**（`program_reports_option_diagnostics`）：RED（缺 `options_diagnostics`）→ GREEN（`new_program` 调 `verify_compiler_options` 存 `program_diagnostics`）。
7. **import 解析 mode 谓词**（`import_syntax_affects_module_resolution_predicate`）：RED（缺函数）→ GREEN（纯谓词）；`resolve_import_file_names` 改用 `get_default_resolution_mode_for_file`（回归 `loads_resolved_relative_import` 仍绿）。

**自检门禁（仅 `-p tsgo_compiler`）**：`cargo test`（37 单测 + 6 doctest 全绿，较 P6-1 +15 单测/+3 doctest）/ `cargo clippy --all-targets -- -D warnings`（净）/ `cargo fmt --check`（净）。

> **本轮临时改动（需 P6-3 复原）**：`internal/compiler/Cargo.toml` 临时注释掉 `tsgo_transformers` 依赖——它在 emitter（P6-3）前未被使用，且并行 agent 的分支里 `tsgo_transformers` 编译不过，会连带阻断 `-p tsgo_compiler` 自检。P6-3 接入 emitter 时再加回。

### P6-3 red→green worklog（逐行为，emitter 可达子集）

> 前置：`internal/compiler/Cargo.toml` **加回** `tsgo_transformers = { path = "../transformers" }`（P6-2 临时注释，本轮复原），并新增 `tsgo_stringutil`（emit BOM 用，对齐 Go `emitter.go` import `stringutil`）。接回后 `cargo build -p tsgo_compiler` 仍绿（基线）。

1. **emit tracer**（`emit_single_js_basic`）：RED（`Program::emit` `todo!()`）→ GREEN（`emit` 编排：`get_source_files_to_emit` → `emit_one` → `emit_js_file`（`emitter::emit_js_text` 重解析 + type-eraser + `Printer::emit_source_file`）→ `write_text`（writeFile 回调）→ `combine_emit_results`）。端到端：`const x: number = 1;` → 写 `/src/index.js` = `const x = 1;\n`。
2. **noEmit skip**（`emit_skipped_when_no_emit`）：RED（emit 仍写文件）→ GREEN（`emit_js_file` 加 `options.NoEmit` 守卫 → `emit_skipped=true`、不写）。
3. **EmitBOM**（`emit_prepends_bom_when_emit_bom`）：RED（无 BOM）→ GREEN（`emit_js_file` 经 `tsgo_stringutil::add_utf8_byte_order_mark` 前缀 `\uFEFF`）。
4. **多文件按序 + combine**（`emit_combines_multiple_files_in_input_order`）：两 root → `emitted_files == [a.js, index.js]`、内容各自正确（确定性铁律：按输入序合并）。
5. **跳过 `.d.ts`**（`emit_skips_declaration_files`）：`source_file_may_be_emitted` 滤掉 `a.d.ts` → 仅 `index.js`。
6. **单文件 target**（`emit_target_source_file_emits_only_that_file`）：`EmitOptions.target_source_file` → 仅该文件 emit。
7. **host fs 回退**（`emit_writes_through_host_fs_by_default`）：无 writeFile 回调 → 经 `host.fs().write_file` 写，回读 `/src/index.js` == `const x = 1;\n`。
8. **newline(CRLF)**（`emit_honors_crlf_newline_option`）：`new_line: Crlf` 经 `PrinterOptions.new_line` → 输出以 `\r\n` 结尾。

**自检门禁（仅 `-p tsgo_compiler`）**：`cargo test`（**45 单测 + 8 doctest** 全绿，较 P6-2 +8 单测/+2 doctest）/ `cargo clippy --all-targets -- -D warnings`（净）/ `cargo fmt --check`（净）。

> **接回 transformers 依赖确认**：`tsgo_transformers` 已稳定绿；加回后 `cargo build/test/clippy -p tsgo_compiler` 全绿，emitter 端到端经 `tsgo_transformers`（type eraser）+ `tsgo_printer` 出 JS 文本成功。

### P6-4 red→green worklog（逐行为，适配 checker 4l retain API + 池驱动诊断）

> 前置（RED 基线）：4l 把 `tsgo_checker` 改成 `Checker::new_checker(Rc<dyn BoundProgram>)`（retain，`+ 'static`）。`checkerpool.rs` 仍调 `Checker::new_checker(&seed)`（借用 `BoundFile<'a>`），`cargo build -p tsgo_compiler` **编译失败**（E0308：expected `Rc<dyn BoundProgram>`，found `&BoundFile<'_>`）。本轮先恢复编译再 TDD 新行为。

1. **owned/`Rc` 重构恢复编译**（spec = 既有测试这套回归网）：`ParsedFile` 改 `Rc<NodeArena>`+`Option<Rc<BindResult>>`（`bind()` 用 `Rc::get_mut`），`BoundFile` 去 `<'a>` 改持两个 `Rc`（owned/`'static`），`create_checkers` 改 `Checker::new_checker(Rc::clone(&program))`（K 个 checker 共享一个 `Rc<dyn BoundProgram>`）。→ `cargo test -p tsgo_compiler` 恢复绿（45 单测 + 8 doctest，P6-3 基线全保留）。
2. **池驱动 2304**（`checkerpool_test.rs::collects_undefined_identifier_diagnostic`）：先把 `collect_diagnostics` 写成 `todo!()`，写测试 → 跑 → **RED**（`not yet implemented` panic，见 checkerpool.rs:198）。再实现（取 program.root()，经文件 0 关联 checker 调 `get_diagnostics(root)`）→ **GREEN**（`y;` → 1 条 2304 "Cannot find name 'y'."）。
3. **端到端 Program 2304**（`program_test.rs::program_collects_semantic_diagnostics`）：`Program::semantic_diagnostics` 先 `todo!()`，写测试 → 跑 → **RED**（todo panic，program.rs:260）。再实现（`create_checkers()` → `checker_pool.collect_diagnostics()`）→ **GREEN**；+ doctest（`new_program` + MapFs，`y;` → 2304）。
4. **第二条可达语义 2339**（`checkerpool_test.rs::collects_property_does_not_exist_diagnostic`，coverage / green-on-add）：interface 缺成员 → 1 条 2339 "Property 'baz' does not exist on type 'Foo'."。证明 pool 驱动的是 checker 的**全部可达语义**（4g/4l 子集），非特例（驱动代码与 2304 同，本条为覆盖，绿即加）。
5. **owned/`Rc`-shareable 断言**（`boundfile_test.rs::bound_file_is_shareable_as_rc_program`，覆盖本轮核心适配）：`Rc<dyn BoundProgram> = Rc::new(BoundFile::for_file(..))` + `Rc::clone` → `strong_count==2`、两 handle 同 root（旧 `BoundFile<'a>` 不可能进 `Rc<.. + 'static>`，本测试守护该能力）。

**自检门禁（仅 `-p tsgo_compiler`）**：`cargo test`（**49 单测 + 9 doctest** 全绿，较 P6-3 +4 单测/+1 doctest）/ `cargo clippy --all-targets -- -D warnings`（净）/ `cargo fmt -p tsgo_compiler -- --check`（净）。**未跑 `--workspace`。**

## 与 Go 的已知偏离（divergence）

- **`*ast.SourceFile` → `ParsedFile`**：Rust `tsgo_ast` 暂无富 `SourceFile`（AST 是 `NodeArena`+`NodeId`），故 compiler 在 `host.rs` 定义 `ParsedFile`（arena + 根 NodeId + 原文 + 诊断）作为 program 的源文件表示，由 host 的 `get_source_file` 产出。`processedFiles.files []*ast.SourceFile` → `Vec<ParsedFile>` + `files_by_path: HashMap<Path, usize>`（索引而非指针）。
- **`CompilerHost.FS()` 返回值**：Go 返回可共享的接口值 `vfs.FS`；Rust `fs(&self) -> Arc<dyn Fs + Send + Sync>`（克隆 Arc，等价的可共享句柄），便于 `module::Resolver` 持有。
- **顺序 worklist（`// PERF(port)`）**：P6-1 的 `FilesParser` 是单线程 worklist；Go 用 `core.WorkGroup` 并行。确定性来自串行 `collect_files` 后处理（深度优先 + seen），与线程调度无关，故顺序版与未来并行版同序。`single_threaded` 形参已接收，真正并行化留后续轮。
- **import 解析 mode**：P6-2 落地了纯谓词 `import_syntax_affects_module_resolution` 与 `get_default_resolution_mode_for_file`（可达子集，当前对所有输入返回 `None`）；精确 per-usage mode（`getModeForUsageLocation`）与 `SourceFileMetaData`/impliedNodeFormat 推导 **blocked-by ast**（`tsgo_ast` 未移植 `SourceFileMetaData`/`GetImpliedNodeFormatForEmitWorker`，且不可编辑 `internal/ast/**`）。
- **checker 池现已驱动检查（P6-4）**：4l 把 checker 改成 program-retaining（`Checker::new_checker(Rc<dyn BoundProgram>)` 存程序 + per-file `check_source_file`/`get_diagnostics`）。`CompilerCheckerPool::create_checkers` 把首个 bound 文件的 owned `BoundFile` 放进 `Rc<dyn BoundProgram>`，`Rc::clone` 进 K 个 checker（Go: 一个 `*Program` 被池共享），`collect_diagnostics` 经 `get_diagnostics(root)` 端到端真出 2304/2339。
  - **owned/`Rc` BoundProgram 偏离**：因 checker 持 `Rc<dyn BoundProgram + 'static>`，retain 的程序必须 owned/`'static`。原 `BoundFile<'a>` 借用 `ParsedFile` 不再满足，故 `ParsedFile` 改持 `Rc<NodeArena>`+`Rc<BindResult>`（PORTING §3 共享非拥有指针→`Rc`、§5 arena 拥有节点），`BoundFile` 改持这两个 `Rc` 的 clone（owned/`'static`），`Rc<BoundFile>` coerce 成 `Rc<dyn BoundProgram>`。与 Go 偏离：Go 由 GC 拥有程序、checker 持裸指针；这里用 `Rc` 显式共享。
  - **仍单文件 + 顺序（`// blocked-by:`）**：`BoundProgram` 仍是单文件（一 program=一 bound 文件），故 pool 真正驱动的是 seed（文件 0）的诊断；多文件 per-file 收集 + `GetDiagnosticsForFile(name)` 过滤 **blocked-by 多文件 `BoundProgram` view（P6 program）**。checker 持 `Rc`（非 `Arc`），跨 checker 并行 **blocked-by parallel `Arc` checker（PORTING §6）**。
- **emit 重解析（P6-3，`// PERF/DIVERGENCE(port)`）**：Go emit 复用共享的 `*ast.SourceFile`，transformer 在 `EmitContext` 工厂里建新节点；Rust 侧 `EmitContext::with_arena` 取走 arena 的所有权，而 `ParsedFile` 持有的 `NodeArena` 不可 `Clone` 且 program 仍要保留它，故 `emit_js_text` **重解析** `ParsedFile::text()` 进一个新 `EmitContext`-owned arena 再 transform+print。这对 transform+print 子集（type erase + 打印，不需 binder/checker 状态）是 sound 的；待 emit 需要 binder/checker 状态时，改为共享文件 arena（而非重解析）。
- **emit 无 `EmitHost`（P6-3）**：Go `emitHost`/`newEmitHost` 从 non-exclusive checker 取 `EmitResolver`；checker 为桩、resolver 不可达，故本轮 emit **不建 `EmitHost`**，直接用 `Program` 的 options + `tsgo_outputpaths`（`ParsedFile: SourceFileLike`）算 `.js` 路径。`EmitHost` trait + emit resolver 转发面 DEFER(blocked-by checker public API)。
- **emit 顺序版（P6-3，`// PERF(port)`）**：Go `Emit` 用 `core.WorkGroup` 并行 + `sync.Pool` writer；本轮顺序遍历 `getSourceFilesToEmit` 并按输入序 `combine_emit_results`。确定性来自"按输入文件顺序合并"，与线程调度无关，故并行化（P6-4）后输出同序。
- **结构体嵌入 → 组合**：`Program` 嵌入 `processedFiles`；Rust 用 `program.processed.xxx` 字段访问 + 委托方法。结构 1:1，访问语法偏离。
- **`WorkGroup` 抽象**：Go `core.WorkGroup`（goroutine + 单线程开关）→ Rust 抽象成 `enum WorkGroup { Parallel(rayon), Sequential }`，`--singleThreaded` 走顺序。**并行只影响发现/调度顺序，最终输出由串行后处理 + 稳定排序固定**。
- **`sync.Pool` → thread_local/对象池**：writer/taskData 复用。
- **non-exclusive checker 访问**：Go 靠"调用方保证无并发"裸用 checker。Rust 借用检查器不允许，需 checker(P4) 用内部可变性（`Mutex`/原子）支持 `&Checker` 路径，否则只能全程 exclusive（性能 `// PERF(port)`）。本包标 `// DEFER(phase-4)`。
- **`context.Context` → `&Cancel`**：取消标志显式传，不引 async（PORTING §3）。
- **`any` 在 `data`/`BuildInfo` 字段**：`FileIncludeReason.data`/`processingDiagnostic.data`/`WriteFileData.BuildInfo` 用枚举或 `Box<dyn Any>`（优先枚举，见 PORTING §3）。
- **camelCase 文件名 → snake_case**：`emitHost.go`→`emit_host.rs` 等，`mod` 重命名。
- **`panic("should not be called")`**：fakingVfs 未用方法保留 `unreachable!`。

## 转交 / 推迟（DEFER）

- **checker 真实数据**：`get_*_diagnostics`/`GetTypeChecker*`/emit resolver 依赖 **checker（P4）**。本包先实现编排骨架，诊断/检查的端到端正确性等 checker 落地，标 `// DEFER(phase-4)`。
- **transformers/printer/sourcemap emit**：`get_script_transformers`/`print_source_file` 依赖 **P5**。emit 的字节级正确性归 P5 + P10。
  - **P6-3 落地**：transform+print 可达子集已接通（type eraser → printer，端到端出 JS）。
  - **sourcemap DEFER（blocked-by printer）**：Rust `tsgo_printer::Printer::emit_source_file(node, text)` 不接受/驱动 `sourcemap::Generator`（Go `printer.Write(node, sf, writer, generator)` 接受），且 `PrinterOptions` 无 `SourceMap`/`InlineSourceMap` 字段，故 `shouldEmitSourceMaps`/`printSourceFile` 的 sourcemap 半边、`getSourceMappingURL`、写 `.map` 全部 DEFER，待 printer 移植 source-map emission 后再做。
  - **完整 `getScriptTransformers` 链 DEFER（blocked-by checker `EmitResolver` + 未移植工厂）**：importElision（需 `MarkLinkedReferencesRecursively`）、runtimeSyntax/legacyDecorators/metadata（Rust `tstransforms` 未移植，且 metadata/decorators 需 checker 类型序列化）、jsx/module/es downlevel（需 reference/emit resolver）。本轮链只含 type eraser。
  - **declaration（`.d.ts`）emit DEFER（blocked-by declarations transformer + checker `EmitResolver`）**：`emit_declaration_file`/`getDeclarationTransformers`/`getDeclarationDiagnostics`。
- **诊断分组并行收集 DEFER（blocked-by checker public API）**：emit 路径**不依赖类型检查**即可产出（emit = transform + print，已落地）；`HandleNoEmitOnError` 前置、`collectCheckerDiagnosticsFromFiles` 分组并行收集仍 `// DEFER`（`Checker::new_checker` 忽略 `BoundProgram`、无 per-file check/diagnostics 入口）。
- **module 解析**：`resolver`/`ResolveModuleName`/`GetCompilerOptionsWithRedirect` 依赖 **module（P4）**；project reference faking VFS 的解析路径同样 blocked-by module。
- **端到端 parity**：真实工程编译（含 fourslash/testdata）归 **P10**。本 phase 的 gate 仅 `TestProgram`（文件排序，需 bundled libs）+ 行为级单测。
