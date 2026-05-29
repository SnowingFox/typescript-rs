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

- [ ] `pub trait CompilerHost: Send + Sync { fs / default_library_path / get_current_directory / trace / get_source_file / get_resolved_project_reference }`　`// Go: host.go:CompilerHost`
- [ ] `pub struct CompilerHostImpl { current_directory, fs, default_library_path, extended_config_cache, trace }`　`// Go: host.go:compilerHost`
- [ ] `pub fn new_compiler_host(...)` / `new_cached_fs_compiler_host(...)`（cachedvfs 包裹）　`// Go: host.go:NewCompilerHost/NewCachedFSCompilerHost`
- [ ] impl：`get_source_file`（ReadFile→`parser::parse_source_file`）/`get_resolved_project_reference`（`tsoptions::get_parsed_command_line_of_config_file_path`）　`// Go: host.go`

### `fileloader.rs`（Go: `fileloader.go`）

- [ ] `struct FileLoader { opts, resolver, default_library_path, compare_paths_options, supported_extensions(2), files_parser, root_tasks, total/lib_file_count: Atomic, factory: Mutex<NodeFactory>, project_reference_file_mapper, dts_directories, pathForLibFile*Cache: DashMap }`　`// Go: fileloader.go:fileLoader`
- [ ] `struct ProcessedFiles { ... 全部 program 共享的解析产物 ... finished_processing }`（被 Program 组合）　`// Go: fileloader.go:processedFiles`
- [ ] `struct LibFile`/`libResolution`/`redirectsFile`(impl HasFileName)/`DuplicateSourceFile`/`jsxRuntimeImportSpecifier`/`sourceFileFromReferenceDiagnostic`　`// Go: fileloader.go`
- [ ] `pub fn process_all_program_files(opts, single_threaded) -> ProcessedFiles`（**编排入口**：建 loader → addProjectReferenceTasks → NewResolver → 加 root/lib/ATA 任务 → `files_parser.parse` → getProcessedFiles）　`// Go: fileloader.go:processAllProgramFiles`
- [ ] `fn to_path` / `add_root_task` / `add_automatic_type_directive_tasks` / `add_project_reference_tasks`　`// Go: fileloader.go`
- [ ] `fn resolve_automatic_type_directives(...)`　`// Go: fileloader.go:resolveAutomaticTypeDirectives`
- [ ] `fn sort_libs(libs)` / `get_default_lib_file_priority(file)`（lib 优先级，确定性排序）　`// Go: fileloader.go`
- [ ] `fn load_source_file_meta_data(file_name) -> SourceFileMetaData`（package.json scope/type/impliedNodeFormat）　`// Go: fileloader.go:loadSourceFileMetaData`
- [ ] `fn parse_source_file(t) -> Option<SourceFile>`（含 tracing Push("createSourceFile")）　`// Go: fileloader.go:parseSourceFile`
- [ ] `fn is_supported_extension` / `get_source_file_from_reference(...)` / `resolve_tripleslash_path_reference(...)`　`// Go: fileloader.go`
- [ ] `fn resolve_type_reference_directives(t)`（含 tracing）　`// Go: fileloader.go:resolveTypeReferenceDirectives`
- [ ] `fn resolve_imports_and_module_augmentations(t)`（**导入解析**：importHelpers/jsx runtime 合成 import、逐 import 解析、shouldAddFile 判定、含 tracing）　`// Go: fileloader.go:resolveImportsAndModuleAugmentations`
- [ ] `fn create_synthetic_import(text, file)`（`factoryMu` 锁内建节点）　`// Go: fileloader.go:createSyntheticImport`
- [ ] `fn path_for_lib_file(name) -> LibFile`（含 libReplacement 解析，DashMap 缓存）/`resolve_library(...)`/`get_library_name_from_lib_file_name(...)`/`get_inferred_library_name_resolve_from(...)`　`// Go: fileloader.go`
- [ ] 自由函数：`get_mode_for_type_reference_directive_in_file` / `get_default_resolution_mode_for_file` / `get_mode_for_usage_location` / `import_syntax_affects_module_resolution` / `get_emit_syntax_for_usage_location_worker`　`// Go: fileloader.go`

### `filesparser.rs`（Go: `filesparser.go`）——**并发点 1**

- [ ] `struct ParseTask { normalized_file_path, path, file, lib_file, redirected, sub_tasks, loaded, started_sub_tasks, is_for_ata, include_reason, package_id, metadata, resolutions*, processing_diagnostics, increase_depth, elide_on_depth, loaded_task, all_include_reasons }`（impl HasFileName）　`// Go: filesparser.go:parseTask`
- [ ] `fn load(&mut self, loader)`（解析文件 + 引用/lib/导入；redirect 分支；扩展名校验）　`// Go: filesparser.go:parseTask.load`
- [ ] `fn redirect(...)` / `fn load_automatic_type_directives(...)` / `fn add_sub_task(ref, lib_file)`　`// Go: filesparser.go`
- [ ] `struct ResolvedRef { file_name, increase_depth, elide_on_depth, include_reason, package_id }`　`// Go: filesparser.go:resolvedRef`
- [ ] `struct FilesParser { wg: WorkGroup, task_data_by_path: DashMap<Path, ParseTaskData>, max_depth }`　`// Go: filesparser.go:filesParser`
- [ ] `struct ParseTaskData { tasks: HashMap<String,ParseTask>, mu: Mutex, lowest_depth, started_sub_tasks, package_id }` + 对象池（getParseTaskData/putParseTaskData）　`// Go: filesparser.go:parseTaskData`
- [ ] `fn parse(&self, loader, tasks)`（`start` + `wg.run_and_wait`）　`// Go: filesparser.go:filesParser.parse`
- [ ] `fn start(&self, loader, tasks, depth)`（**核心动态 worklist**：per-path LoadOrStore 去重、per-data Mutex、深度更新、elideOnDepth、按 casing 多 task load、递归 start 子任务）　`// Go: filesparser.go:filesParser.start`
- [ ] `fn get_processed_files(&self, loader) -> ProcessedFiles`（**单线程后处理**：`collect_files` 深度优先 + seen 去重 + 大小写/包去重诊断 + redirect/ATA/lib/file 分流 + sortLibs + lib 解析合并）　`// Go: filesparser.go:filesParser.getProcessedFiles`
- [ ] `fn add_include_reason(...)`　`// Go: filesparser.go:addIncludeReason`

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

- [ ] `pub trait CheckerPool { fn get_checker(&self, cancel, file: Option<&SourceFile>) -> (Arc<Checker>, impl FnOnce()) }`　`// Go: checkerpool.go:CheckerPool`
- [ ] `struct CompilerCheckerPool { program, tracing, create_checkers_once: OnceLock, checkers: Vec<Arc<Checker>>, locks: Vec<Mutex<()>>, file_associations: HashMap<*SourceFile,usize> }`　`// Go: checkerpool.go:checkerPool`
- [ ] `fn new_checker_pool(_with_tracing)`（checkerCount = clamp(配置/4, 1, files, 256)）　`// Go: checkerpool.go`
- [ ] `get_checker` / `get_checker_for_file_non_exclusive` / `get_checker_for_file_exclusive` / `get_checker_non_exclusive`　`// Go: checkerpool.go`
- [ ] `fn create_checkers(&self)`（**并行**建 N 个 checker + file→checker 的 `i%K` 分配）　`// Go: checkerpool.go:createCheckers`
- [ ] `fn for_each_checker_parallel(cb)` / `get_global_diagnostics()` / `for_each_checker_group_do(cancel, files, single_threaded, cb)`（**分组并行**）　`// Go: checkerpool.go`

### `emit_host.rs`（Go: `emitHost.go`）

- [ ] `pub trait EmitHost: printer::EmitHost + declarations::DeclarationEmitHost { Options / SourceFiles / UseCaseSensitiveFileNames / GetCurrentDirectory / CommonSourceDirectory / IsEmitBlocked }`（**must be thread-safe**）　`// Go: emitHost.go:EmitHost`
- [ ] `struct EmitHostImpl { program, emit_resolver }` + `new_emit_host(cancel, program, file) -> (EmitHostImpl, done)`（从 non-exclusive checker 取 emit resolver）　`// Go: emitHost.go`
- [ ] 全部转发方法（GetModeForUsageLocation/GetResolvedModule.../GetOutputPathsFor/WriteFile/GetEmitResolver/...）　`// Go: emitHost.go`

### `emitter.rs`（Go: `emitter.go`）——**并发点 5 的单文件逻辑**

- [ ] `enum EmitOnly { All, Js, Dts, ForcedDts }`　`// Go: emitter.go`
- [ ] `struct Emitter { host, emit_only, emitter_diagnostics, writer, paths, source_file, emit_result, write_file, tr }`　`// Go: emitter.go:emitter`
- [ ] `fn emit(&mut self)`（emitJSFile + emitDeclarationFile + 收集诊断；含 tracing）　`// Go: emitter.go:emit`
- [ ] `get_declaration_transformers` / `run_script_transformers` / `run_declaration_transformers`　`// Go: emitter.go`
- [ ] `get_module_transformer(opts)` / `get_script_transformers(...)`（**transformer 流水线编排**：metadata/typeEraser/importElision/runtimeSyntax/legacyDecorators/jsx/es downlevel/useStrict/module/constEnum inlining）　`// Go: emitter.go`
- [ ] `emit_js_file` / `emit_declaration_file` / `print_source_file`（sourcemap 生成、URL、写文件、BOM）/`write_text`　`// Go: emitter.go`
- [ ] `should_emit_source_maps` / `should_emit_declaration_source_maps` / `get_source_root` / `get_source_map_directory` / `get_source_mapping_url`　`// Go: emitter.go`
- [ ] `trait SourceFileMayBeEmittedHost` + `fn source_file_may_be_emitted(file, host, force_dts) -> bool` + `get_source_files_to_emit(...)` + `is_source_file_not_json` + `get_declaration_diagnostics(host, file)`　`// Go: emitter.go`

### `program.rs`（Go: `program.go`，最大）

类型 / 构造：

- [ ] `pub struct ProgramOptions { host, config, use_source_of_project_reference, single_threaded, create_checker_pool, typings_location, project_name, tracing }` + `can_use_project_reference_source`　`// Go: program.go:ProgramOptions`
- [ ] `struct LazyValue<T> { cell: OnceLock<T> }` + `get_value(compute)` + `try_reuse(from)`　`// Go: program.go:lazyValue`
- [ ] `struct PackageNamesInfo { resolved, unresolved, deep_import_packages }`　`// Go: program.go:packageNamesInfo`
- [ ] `pub struct Program { opts, checker_pool, compiler_checker_pool: Option, compare_paths_options, processed: ProcessedFiles, uses_uri_style_node_core_modules, common_source_directory: OnceLock, declaration_diagnostic_cache: DashMap, program_diagnostics, has_emit_blocking_diagnostics, source_files_to_emit: OnceLock, unresolved_imports/known_symlinks/package_names: LazyValue, has_ts_file/packages_map: OnceLock }`　`// Go: program.go:Program`
- [ ] `pub fn new_program(opts) -> Program`（**编排入口**：tracing Push("createProgram") → `process_all_program_files` → `init_checker_pool` → `verify_compiler_options`）　`// Go: program.go:NewProgram`
- [ ] `pub fn update_program(changed_path, new_host, create_checker_pool) -> (Program, bool)`（增量复用，redirect 组重建）　`// Go: program.go:UpdateProgram`
- [ ] `fn init_checker_pool(&mut self)`（CreateCheckerPool 注入或内置池）　`// Go: program.go:initCheckerPool`
- [ ] `fn can_replace_file_in_program(f1, f2)` + `equal_*`（module specifiers/augmentation/file references/checkJS directive）　`// Go: program.go`

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

- [ ] `bind_source_files(&self)`（**并行** bind，含 tracing）　`// Go: program.go:BindSourceFiles`
- [ ] `get_type_checker(cancel)` / `for_each_checker_parallel(cb)` / `get_type_checker_for_file(_exclusive)(cancel, file)`　`// Go: program.go`
- [ ] `collect_diagnostics(...)` / `collect_diagnostics_from_files(...)`（**并行** map + 稳定排序）　`// Go: program.go`
- [ ] `collect_checker_diagnostics(...)` / `collect_checker_diagnostics_from_files(...)`（**分组并行**或 per-file）　`// Go: program.go`
- [ ] `get_syntactic_diagnostics` / `get_additional_js_syntactic_diagnostics`（JS 参数装饰器）/`get_bind_diagnostics`/`get_semantic_diagnostics`/`get_semantic_diagnostics_without_no_emit_filtering`/`get_suggestion_diagnostics`/`get_program_diagnostics`/`get_include_processor_diagnostics`/`get_global_diagnostics`/`get_declaration_diagnostics`　`// Go: program.go`
- [ ] `skip_type_checking` / `can_include_bind_and_check_diagnostics`　`// Go: program.go`
- [ ] `get_semantic_diagnostics_with_checker` / `get_bind_and_check_diagnostics_with_checker`（@ts-ignore/@ts-expect-error 指令处理）/`get_diagnostics_with_preceding_directives`/`get_declaration_diagnostics_for_file`（DashMap 缓存）/`get_suggestion_diagnostics_with_checker`　`// Go: program.go`
- [ ] `is_comment_or_blank_line` / `SortAndDeduplicateDiagnostics` / `compact_and_merge_related_infos`　`// Go: program.go`
- [ ] `static PLAIN_JS_ERRORS`（plain JS 允许的诊断 code 集合，~100 项）　`// Go: program.go:plainJSErrors`

verifyCompilerOptions（~400 行，program 构建的选项一致性诊断）：

- [ ] `fn verify_compiler_options(&mut self)`（removed options / strict 互斥 / sourcemap / declaration / paths / module-resolution 配对 / emit 唯一性 / project references 校验）　`// Go: program.go:verifyCompilerOptions`
- [ ] `block_emitting_of_file` / `is_emit_blocked` / `verify_project_references` / `has_zero_or_one_asterisk_character` / `module_resolution_supports_package_json_exports_and_imports` / `emit_module_kind_is_non_node_esm`　`// Go: program.go`
- [ ] `common_source_directory(&self)`（OnceLock）/`check_source_files_belong_to_path`　`// Go: program.go:CommonSourceDirectory`
- [ ] `get_source_files_to_emit(target, force_dts)`（OnceLock for 全量）　`// Go: program.go:getSourceFilesToEmit`

emit（**并发点 5**）：

- [ ] `struct WriteFileData` / `type WriteFile` / `struct EmitOptions` / `struct EmitResult` / `struct SourceMapEmitResult`　`// Go: program.go`
- [ ] `pub fn emit(&self, cancel, options) -> EmitResult`（**并行** emit + writer 池 + 按序合并 + tracing；HandleNoEmitOnError 前置）　`// Go: program.go:Emit`
- [ ] `fn combine_emit_results(results) -> EmitResult`　`// Go: program.go:CombineEmitResults`
- [ ] `pub trait ProgramLike` + `fn handle_no_emit_on_error(...)` + `fn get_diagnostics_of_any_program(...)`　`// Go: program.go`
- [ ] `fn filter_no_emit_semantic_diagnostics(diags, options)`　`// Go: program.go:FilterNoEmitSemanticDiagnostics`

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

## 与 Go 的已知偏离（divergence）

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
- **module 解析**：`resolver`/`ResolveModuleName`/`GetCompilerOptionsWithRedirect` 依赖 **module（P4）**；project reference faking VFS 的解析路径同样 blocked-by module。
- **端到端 parity**：真实工程编译（含 fourslash/testdata）归 **P10**。本 phase 的 gate 仅 `TestProgram`（文件排序，需 bundled libs）+ 行为级单测。
