# project: 实现方案（impl.md）

> **子 crate 拆分（依赖序）**：`project/dirty`→`tsgo_project_dirty`、`project/logging`→`tsgo_project_logging` 拆出并**前移到 P1**（叶子），以破 `ls/autoimport→project→…→ls` 环。`project` 主体（含 `ata`/`background`）留 P8。本 impl.md 同时承载 dirty/logging 的移植细节。详见 [references/crate-map.md](../../references/crate-map.md)。

> 写前已实读 `internal/project/` 全部非测试 `.go`（22 个顶层 + 4 子包 `ata`/`background`/`dirty`/`logging` 共 16 个，合计 38 个非测试文件；其中 `session.go`/`projectcollectionbuilder.go`/`configfileregistrybuilder.go`/`snapshotfs.go`/`ata/ata.go`/`ata/typesmap.go` 按公开 API 表面 + 关键路径精读）。所有 TODO 带 `// Go:` 锚点。

**crate**：`tsgo_project`（+ 子 crate `tsgo_project_dirty` `tsgo_project_background` `tsgo_project_logging` `tsgo_project_ata`）
**目标**：LSP 会话的**工程管理与不可变快照引擎**。把编辑器的 textDocument 事件 + 文件监听变更，增量地物化成不可变 `Snapshot`（含 `ProjectCollection`、`ConfigFileRegistry`、program、auto-imports），为 `tsgo_ls` 语言服务提供数据源，并据快照 diff 更新文件监听与 ATA（自动类型获取）。
**依赖（crate）**：`tsgo_ast` `tsgo_compiler` `tsgo_checker` `tsgo_core` `tsgo_collections` `tsgo_ls`（含 `ls/autoimport` `ls/lsconv` `ls/lsutil`）`tsgo_lsproto` `tsgo_tsoptions` `tsgo_tspath` `tsgo_vfs`（含 `vfs/vfsmatch`）`tsgo_diagnostics` `tsgo_locale` `tsgo_packagejson` `tsgo_semver` `tsgo_parser` `tsgo_sourcemap` + 上述 4 个子 crate；外部 `xxh3`（`zeebo/xxh3`→`xxhash-rust`/`twox-hash`）。

## 这个包是什么（业务说明）

`project` 是语言服务的"状态机心脏"。它解决一个难题：**编辑器持续推送增量变更（开/改/关/存/监听事件），而每次语言服务请求都需要一份自洽、可并发只读、可取消的工程视图**。做法是 **immutable snapshot + copy-on-write**：

1. `Session`（`session.go`，~1840 行）是唯一可变入口。`DidOpenFile`/`DidChangeFile`/`DidCloseFile`/`DidSaveFile`/`DidChangeWatchedFiles` 把事件累积进 `pendingFileChanges`，并调度（防抖）一次快照更新。请求（`GetLanguageService` 等）会先 `flushChanges` 把待处理变更并入下一个 `Snapshot`。
2. `Snapshot`（`snapshot.go`）是不可变、引用计数（`refCount`）的工程视图。`Snapshot.Clone` 是核心：用 `snapshotFSBuilder` 增量重建文件系统视图，用 `ProjectCollectionBuilder` 增量重建项目集合（仅 dirty 的项目重建 program），更新 auto-import 注册表，最后 `freeze` 掉编译 host 的可变引用。旧快照 `Deref` 到 0 时释放其 program 的 source files（`parseCache.Deref`）。
3. `ProjectCollection`（`projectcollection.go`）+ `ProjectCollectionBuilder`（`projectcollectionbuilder.go`，~1100 行）：管理 configured / inferred 项目，决定每个文件的"默认项目"（含 solution/项目引用搜索）。
4. `ConfigFileRegistry`（`configfileregistry.go`）+ `ConfigFileRegistryBuilder`（`configfileregistrybuilder.go`，~700 行）：解析/缓存 tsconfig（含 `extends` 链），引用计数地被多个项目共享。
5. `Project`（`project.go`）：单个 TS 项目，持有 `CommandLine`、`Program`、`CheckerPool`、文件监听集合。`CreateProgram` 决定增量克隆 vs 全量重建。
6. **FS 三层**：`overlayFS`（`overlayfs.go`，编辑器 overlay + 磁盘）→ `snapshotFSBuilder`/`SnapshotFS`（`snapshotfs.go`，快照内 dirty 文件/目录树 + 符号链接 realpath 别名）→ `sourceFS`（compiler host 用，可跟踪 seenFiles）。
7. **缓存族**：`parseCache`（`RefCountCache`，AST 复用）、`extendedConfigCache`（`OwnerCache`，extends 链 AST）、`programCounter`（program 引用计数）。
8. **ATA 子包**（`ata/`）：自动类型获取——从源文件名/`package.json`/`node_modules` 推断 `@types/*` 包，调 npm 安装，回填 typings 文件。
9. **支撑子包**：`dirty/`（COW dirty-tracking map/box/syncmap，快照增量的底座）、`background/`（后台任务队列）、`logging/`（树形日志 + 时间戳 logger）。

它在 P8 是因为它装配了 P4 的 `checker`/`compiler`、P6 的 `tsoptions`、P7 的 `ls`，并被 P8 的 `lsp`/`api` 消费。

## 所有权 / 类型映射（本包关键决策）

### COW dirty 模型（`dirty/` 子包）—— 快照增量的底座

Go 的 `dirty.Map`/`Box`/`SyncMap` 实现"基于 base 不可变 map 的写时复制覆盖层"：读优先看 dirty 覆盖，没有则看 base；写时若 base 项未脏则先 `Clone()` 再改并登记到 dirty；`Finalize()` 把 base+dirty 合成新 base（无改动则原样返回 base，零拷贝）。`SyncMap` 还处理"两 goroutine 竞争同一 key 的 dirty 化"——用 `proxyFor` 把败者的所有操作转发给胜者。

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Cloneable[T] interface { Clone() T }` | `trait Cloneable { fn clone_cow(&self) -> Self; }`（避免与 `std::clone::Clone` 混淆，或直接复用 `Clone`） | COW 约束 |
| `Map[K,V]{ base map[K]V; dirty map[K]*MapEntry }` | `struct Map<K,V>{ base: Arc<FxHashMap<K,V>>, dirty: FxHashMap<K, MapEntry<K,V>> }` | base 共享用 `Arc` |
| `MapEntry`（key/original/value/dirty/delete + `Change`/`Replace`/`Delete`/`ChangeIf`） | `struct MapEntry<K,V>` + 方法 | `Change` 在首次脏化时 `value = value.clone_cow()` |
| `SyncMap` + `proxyFor`（竞争路由） | `struct SyncMap<K,V>` 内含 `collections::SyncMap` + 每 entry `Mutex`；`proxy_for: Option<Arc<SyncMapEntry>>` | **关键并发点**：用 `Arc<Mutex<..>>` + `LoadOrStore` 原子化，败者设 `proxy_for` 转发 |
| `FinalizationHooks{OnDelete,OnChange,OnAdd}` | `struct FinalizationHooks<K,V>{ on_delete/on_change/on_add: Option<Box<dyn Fn>> }` | finalize 钩子 |
| `MapBuilder[K,VBase,VBuilder]`（toBuilder/build） | `struct MapBuilder<K,VBase,VBuilder>` | 双形态（base 值 vs 构建中值） |
| `CloneableMap[K,V] map[K]V`（`Clone()=maps.Clone`） | newtype + `Clone` | |
| `CloneMapIfNil` helper | `fn clone_map_if_nil(...)` | |

### Snapshot 引用计数 + 不可变性

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Snapshot{ refCount atomic.Int32; ... }` + `ref/tryRef/Deref/dispose` | `Arc<Snapshot>`（首选）或保留显式 `AtomicI32` 计数贴近 Go | **决策**：保留 Go 的显式 `ref/tryRef/Deref(session)`，因为 dispose 需要回调 session 释放 parseCache/extendedConfigCache，不是纯 `Arc` drop。用 `AtomicI32` + `dispose(&Session)`。`tryRef` 用 CAS 循环 |
| `programCounter{ refs map[*Program]int32 }` | `Mutex<FxHashMap<ProgramId, i32>>` | program 用 id（arena 索引）而非裸指针 |
| `RefCountCache[K,V,Args]`（SyncMap + 每 entry mu + refCount） | `struct RefCountCache<K,V,Args>` + `collections::SyncMap` | `Acquire/Ref/Deref/Has`；删除竞争用 `loadOrStoreNewLockedEntry` 重试 |
| `OwnerCache[K,V,Args]`（owners set 而非 count） | 同上，entry 含 `owners: FxHashSet<u64>` | extends 链 AST 跨快照共享 |

### 并发点（PORTING §6 — 必须在 Rust 落地为真并发，输出保确定性）

1. **文件监听（watch.go / session.updateWatches）**：`updateWatch[T]` 用后台 goroutine 调 `client.WatchFiles`，1s 超时（`watchRequestTimeout`）则回滚注册并标 pending 待下次重试。Rust：`std::thread` + 带超时的 channel（`crossbeam select` + `recv_timeout`），`watchRegistry` 用 `Mutex`。`WatchedFiles[T]` 的 `computeWatchersOnce` → `OnceCell`。
2. **请求处理 / checker 池（checkerpool.go）**：`CheckerPool` 用 `sync.Cond` 等待可用 checker，按 requestID/file 关联 checker。Rust：`Mutex<PoolState>` + `Condvar`（`std::sync::Condvar`）。`requestID` 从 ctx 取（Rust 显式传）。
3. **快照更新调度（session.go）**：`ScheduleSnapshotUpdate`/`ScheduleDiagnosticsRefresh`/`scheduleIdleCacheClean`/`warmAutoImportCache` 用 `context.WithCancel` + 定时器防抖。Rust：可取消 token（`Arc<AtomicBool>`）+ `crossbeam` 定时 + generation 计数。
4. **后台队列（background/queue.go）**：`sync.WaitGroup` + `wg.Go`。Rust：`std::thread::scope` 或自管 `JoinHandle` 集合 + `Mutex<bool> closed`。
5. **ATA 安装（ata/ata.go）**：`TypingsInstaller` 节流（ThrottleLimit=5）并发 npm 安装。Rust：`crossbeam` worker 池或 `rayon` + 信号量。
6. **`SyncMap.proxyFor` 竞争**（见上）。
7. **`SnapshotFS` 的 `diskFiles`/`diskDirectories`**：`dirty.SyncMap` 并发只读 + 构建期并发填充（compiler host 多 checker 并发读文件）。

> 所有并发收集后按稳定 key 排序，保证诊断/监听 glob 顺序与 Go 一致（TDD 断言前提）。

### 其它映射

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Kind`(iota) + stringer | `#[repr(i32)] enum Kind { Inferred, Configured }` + `Display`（"Inferred"/"Configured"） | `project_stringer_generated.go` → Rust `Display` impl |
| `ProgramUpdateKind`/`PendingReload`/`FileChangeKind`/`UpdateReason`/`NameValidationResult`(iota) | `#[repr(i32)] enum` | 保 iota 顺序 |
| `FileHandle` interface（diskFile/Overlay） | `trait FileHandle` + enum / `Arc<dyn FileHandle>` | Content/Hash/Version/IsOverlay/LSPLineMap/ECMALineInfo/Kind |
| `sync.Once` / `sync.OnceValue` | `OnceCell` / `OnceLock` / `Lazy` | commandLineWithTypingsFilesOnce / computeWatchersOnce / openConfiguredProjectsOnce / runtimeMetricsSamples |
| `context.Context` | 显式 `&Cancel`（`Arc<AtomicBool>`）+ requestID/locale 显式传 | PORTING §3 |
| `compiler.CompilerHost` impl（compilerhost.go） | `impl compiler::CompilerHost for CompilerHost` | `freeze`/`ensureAlive` 用 `Option<Arc<Builder>>` 置 None |
| telemetry 结构（lsproto.TelemetryEvent） | 复用 `tsgo_lsproto` 类型 | |

## 文件清单 → Rust 模块

### crate `tsgo_project`（`internal/project/` 顶层，22 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `session.go` | `internal/project/session.rs`（lib.rs，crate 根） | `Session`/`SessionOptions`/`SessionInit`/全部 Did* 事件 + 快照更新调度 + 监听更新 + telemetry + 诊断发布。basename≠目录名→lib.rs |
| `snapshot.go` | `internal/project/snapshot.rs` | `Snapshot`/`Clone`/ref 计数/`SnapshotChange`/`ResourceRequest`/`ProjectTreeRequest`/`APISnapshotRequest`/`ATAStateChange` |
| `snapshotfs.go` | `internal/project/snapshotfs.rs` | `FileHandle`/`fileBase`/`diskFile`/`Overlay`/`SnapshotFS`/`snapshotFSBuilder`/`sourceFS` + realpath 别名 |
| `overlayfs.go` | `internal/project/overlayfs.rs` | `overlayFS`/`processChanges`（事件去重与 overlay 演进） |
| `project.go` | `internal/project/project.rs` | `Project`/`Kind`/`CreateProgram`/`Clone`/ATA 触发判定 |
| `projectcollection.go` | `internal/project/projectcollection.rs` | `ProjectCollection`/默认项目搜索 |
| `projectcollectionbuilder.go` | `internal/project/projectcollectionbuilder.rs` | `ProjectCollectionBuilder`（增量重建项目集合，~1100 行） |
| `configfileregistry.go` | `internal/project/configfileregistry.rs` | `ConfigFileRegistry`/`configFileEntry`/`configFileNames` + 测试访问器 |
| `configfileregistrybuilder.go` | `internal/project/configfileregistrybuilder.rs` | `ConfigFileRegistryBuilder`（解析/缓存 tsconfig + extends，~700 行） |
| `compilerhost.go` | `internal/project/compilerhost.rs` | `compilerHost`（impl `compiler::CompilerHost`）+ freeze |
| `checkerpool.go` | `internal/project/checkerpool.rs` | `CheckerPool`（Condvar 池 + 全局诊断累积） |
| `programcounter.go` | `internal/project/programcounter.rs` | program 引用计数 |
| `parsecache.go` | `internal/project/parsecache.rs` | `ParseCacheKey`/`ParseCache`（= `RefCountCache`） |
| `refcountcache.go` | `internal/project/refcountcache.rs` | `RefCountCache`（泛型 ref-count 缓存） |
| `ownercache.go` | `internal/project/ownercache.rs` | `OwnerCache`（owners 集合缓存） |
| `extendedconfigcache.go` | `internal/project/extendedconfigcache.rs` | `ExtendedConfigCache`（= `OwnerCache` + hash 失效） |
| `filechange.go` | `internal/project/filechange.rs` | `FileChange`/`FileChangeKind`/`FileChangeSummary`/merge |
| `watch.go` | `internal/project/watch.rs` | `WatchedFiles`/`watchRegistry`/glob 计算/路径分组 |
| `autoimport.go` | `internal/project/autoimport.rs` | `autoImportBuilderFS`/`autoImportRegistryCloneHost`（impl `autoimport::RegistryCloneHost`） |
| `api.go` | `internal/project/api.rs` | `APIOpenProject`/`APIUpdateWithFileChanges`（Session 的 API 入口） |
| `client.go` | `internal/project/client.rs` | `Client` trait（被 lsp.Server 实现） |
| `project_stringer_generated.go` | （并入 `project.rs` 的 `Display`） | stringer → Display |

### 子 crate `tsgo_project_dirty`（`internal/project/dirty/`，8 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `map.go` | `internal/project/dirty/map.rs`（lib.rs，crate 根 `tsgo_project_dirty`） | `Map`/`MapEntry` |
| `syncmap.go` | `internal/project/dirty/syncmap.rs` | `SyncMap`/`SyncMapEntry`/`proxyFor`/`FinalizationHooks` |
| `box.go` | `internal/project/dirty/box.rs` | `Box`（单值 COW） |
| `entry.go` | `internal/project/dirty/entry.rs` | `mapEntry` 共享字段 |
| `interfaces.go` | `internal/project/dirty/interfaces.rs` | `Cloneable`/`Value` trait |
| `cloneablemap.go` | `internal/project/dirty/cloneablemap.rs` | `CloneableMap` |
| `mapbuilder.go` | `internal/project/dirty/mapbuilder.rs` | `MapBuilder`（双形态） |
| `util.go` | `internal/project/dirty/util.rs` | `CloneMapIfNil` |

### 子 crate `tsgo_project_background`（`internal/project/background/`，1 文件）

| `queue.go` | `internal/project/background/queue.rs`（lib.rs） | `Queue`（WaitGroup 等价 + closed 标志） |

### 子 crate `tsgo_project_logging`（`internal/project/logging/`，3 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `logger.go` | `internal/project/logging/logger.rs`（lib.rs） | `Logger` trait + 时间戳 logger + `NewLogger` |
| `logtree.go` | `internal/project/logging/logtree.rs` | `LogTree`（树形日志，Fork/Embed/String） |
| `logcollector.go` | `internal/project/logging/logcollector.rs` | `LogCollector`/`NewTestLogger`（固定时间戳） |

### 子 crate `tsgo_project_ata`（`internal/project/ata/`，4 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `ata.go` | `internal/project/ata/ata.rs`（lib.rs） | `TypingsInstaller`/`TypingsInfo`/`CachedTyping`/`NpmExecutor` trait/安装编排（~570 行） |
| `discovertypings.go` | `internal/project/ata/discovertypings.rs` | `DiscoverTypings` + 文件名/manifest/node_modules 推断 + `removeMinAndVersionNumbers` |
| `typesmap.go` | `internal/project/ata/typesmap.rs` | `safeFileNameToTypeName` 表 + types-registry 解析（~600 行，多为数据表） |
| `validatepackagename.go` | `internal/project/ata/validatepackagename.rs` | `ValidatePackageName`/`NameValidationResult`/失败渲染 |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| 内容哈希 | `xxhash-rust`（xxh3）或 `twox-hash` | 对齐 `zeebo/xxh3`（128-bit）——**需验证 128-bit 实现与 Go 字节级一致**，否则缓存 key 不匹配。先 `// TODO(port)` 标注，必要时自实现 |
| 运行时指标 | （telemetry：`logRuntimeMetrics` 用 `runtime/metrics`） | Rust 无直接等价；用 `// DEFER` 或自采集，仅 telemetry 用，不影响正确性 |
| 并发原语 | `crossbeam-channel` / std `Condvar` / `dashmap`（可选） | 见并发点 |

`xxhash-rust` 为本 phase 新增，追加到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，节选关键项；完整逐函数在执行期补全）

### `dirty/`（先行，是快照增量底座）

- [ ] `interfaces.rs`：`Cloneable` / `Value` trait　`// Go: dirty/interfaces.go`
- [ ] `entry.rs`：`mapEntry`（key/original/value/dirty/delete + getters）　`// Go: dirty/entry.go`
- [ ] `box.rs`：`Box::{new,value,original,dirty,set,change,change_if,delete,locked,finalize}`　`// Go: dirty/box.go:Box.*`
- [ ] `map.rs`：`Map::{new,get,add,change,try_delete,delete,range,clear,finalize}` + `MapEntry::{change,replace,change_if,delete,locked}`　`// Go: dirty/map.go:*`
- [ ] `syncmap.rs`：`SyncMap::{new,load,load_or_store,delete,range,finalize,finalize_with}` + `SyncMapEntry`（含 `proxy_for` 路由：`value/dirty/change/change_if/delete/delete_if/locked`）+ `FinalizationHooks`　`// Go: dirty/syncmap.go:*`（**并发**）
- [ ] `mapbuilder.rs`：`MapBuilder::{new,set,delete,clear,has,build}`　`// Go: dirty/mapbuilder.go:*`
- [ ] `cloneablemap.rs` / `util.rs`：`CloneableMap`/`CloneMapIfNil`　`// Go: dirty/cloneablemap.go / util.go`

### `logging/`

- [ ] `logger.rs`：`Logger` trait（Error/Errorf/Warn/Warnf/Info/Infof/Log/Logf/Verbose/IsVerbose/SetVerbose）+ 时间戳 logger（nil-safe）+ `new_logger`/`format_time`（`[15:04:05.000]`）　`// Go: logging/logger.go:*`
- [ ] `logtree.rs`：`LogTree::{new,log,logf,fork,embed,string,...}`（root 累积 count/stringLength，递归缩进输出，`======== name ========` 头）　`// Go: logging/logtree.go:*`
- [ ] `logcollector.rs`：`LogCollector`/`new_test_logger`（固定时间 `Unix(1349085672,0)`）　`// Go: logging/logcollector.go:*`

### `background/`

- [ ] `queue.rs`：`Queue::{new,enqueue,wait,close}`（closed 后拒绝；ctx 取消前后双检）　`// Go: background/queue.go:*`（**并发**）

### `ata/`

- [ ] `validatepackagename.rs`：`validate_package_name`（长度/`.`/`_`/scoped/URI 安全）+ `render_package_name_validation_failure`　`// Go: ata/validatepackagename.go:*`
- [ ] `discovertypings.rs`：`discover_typings`/`add_inferred_typing(s)`/`get_typing_names_from_source_file_names`/`add_typing_names_and_get_files_to_watch`/`remove_min_and_version_numbers`/`is_typing_up_to_date`　`// Go: ata/discovertypings.go:*`
- [ ] `typesmap.rs`：`safe_file_name_to_type_name` 表 + types-registry 解析（`isTypingUpToDate` 用）　`// Go: ata/typesmap.go:*`
- [ ] `ata.rs`：`TypingsInstaller`/`TypingsInfo`/`CachedTyping`/`NpmExecutor` trait/`install_typings`/节流安装/`InstallNpmPackages`（命令过长分批、部分失败继续）　`// Go: ata/ata.go:*`（**并发**）

### 顶层缓存族（依赖序较低，先行）

- [ ] `refcountcache.rs`：`RefCountCache::{new,acquire,has,ref,deref,load_or_store_new_locked_entry}`　`// Go: refcountcache.go:*`
- [ ] `ownercache.rs`：`OwnerCache::{new,load_and_acquire,acquire,add_owner,has,release,load_or_store_locked_entry}`　`// Go: ownercache.go:*`
- [ ] `programcounter.rs`：`programCounter::{ref,deref,len}`　`// Go: programcounter.go:*`
- [ ] `parsecache.rs`：`ParseCacheKey`/`new_parse_cache`（parse on miss + hash）　`// Go: parsecache.go:*`
- [ ] `extendedconfigcache.rs`：`new_extended_config_cache`/`hash`（内容 + extends 文件内容）　`// Go: extendedconfigcache.go:*`

### FS / 文件层

- [ ] `overlayfs.rs`：`FileHandle`/`diskFile`/`Overlay`/`overlayFS::{getFile,overlays,processChanges}`（事件去重状态机 + 文本变更应用）　`// Go: overlayfs.go:*`
- [ ] `snapshotfs.rs`：`fileBase`/`SnapshotFS`/`snapshotFSBuilder`（markDirtyFiles/expandAndFilterWatchEvents/convertOpenAndCloseToChanges/invalidateCache/watchChangesOverlapCache/Finalize）/`sourceFS`（Track/SeenFile/DisableTracking）/realpath 别名（expandRealpathAliases）　`// Go: snapshotfs.go:*`（**并发**：diskFiles SyncMap）
- [ ] `filechange.rs`：`FileChange`/`FileChangeKind::IsWatchKind`/`FileChangeSummary::{IsEmpty,HasExcessiveWatchEvents,...}`/`mergeFileChangeSummary`　`// Go: filechange.go:*`

### 项目 / 配置

- [ ] `project.rs`：`Project::{NewConfigured,NewInferred,Name,DisplayName,ID,CreateProgram,Clone,GetTypeAcquisition,ShouldTriggerATA,ComputeTypingsInfo,...}` + `Kind` Display　`// Go: project.go:*`
- [ ] `projectcollection.rs`：`ProjectCollection::{ConfiguredProject,GetProjectByPath,Projects,GetDefaultProject,findDefaultConfiguredProject(Worker),GetProjectsContainingFile,GetOpenConfiguredProjects,clone}` + `findDefaultConfiguredProjectFromProgramInclusion`　`// Go: projectcollection.go:*`
- [ ] `projectcollectionbuilder.rs`：`ProjectCollectionBuilder`（DidChangeFiles/DidRequestFile/DidRequestProject(Trees)/HandleAPIRequest/DidUpdateATAState/DidChangeCustomConfigFileName/Finalize）　`// Go: projectcollectionbuilder.go:*`
- [ ] `configfileregistry.rs`：`ConfigFileRegistry`/`configFileEntry`/`configFileNames` + 测试访问器（ForEachTestConfigEntry 等）　`// Go: configfileregistry.go:*`
- [ ] `configfileregistrybuilder.rs`：`ConfigFileRegistryBuilder`（acquireConfigForProject/解析 extends/失效/Finalize）　`// Go: configfileregistrybuilder.go:*`
- [ ] `compilerhost.rs`：`compilerHost`（impl compiler.CompilerHost：FS/GetSourceFile/GetResolvedProjectReference/Trace）+ `freeze`/`ensureAlive`　`// Go: compilerhost.go:*`
- [ ] `checkerpool.rs`：`CheckerPool::{GetChecker,GetGlobalDiagnostics,TakeNewGlobalDiagnostics,...}`（Condvar 等待 + 全局诊断合并去重）　`// Go: checkerpool.go:*`（**并发**）

### 监听

- [ ] `watch.rs`：`WatchedFiles`/`watchRegistry`（Acquire/Release/MarkPending/...）/`createResolutionLookupGlobMapper`/`getTypingsLocationsGlobs`/`getPathComponentsForWatching`/`perceivedOsRootLengthForWatching`/`getRecursiveGlobPattern`/`newRecursiveDirectoryWatcher`　`// Go: watch.go:*`（**并发**）

### 会话（最后，装配一切）

- [ ] `session.rs`：`Session`/`SessionOptions`/`SessionInit`/`NewSession` + `DidOpenFile`/`DidChangeFile`/`DidCloseFile`/`DidSaveFile`/`DidChangeWatchedFiles`/`DidChangeCompilerOptionsForInferredProjects`　`// Go: session.go:Did*`
- [ ] 快照获取与 LS：`Snapshot`/`getSnapshot`/`GetLanguageService`/`GetLanguageServiceAndProjectsForFile`/`GetProjectsForFile`/`GetLanguageServicesForDocuments`/`WithSnapshotLoadingProjectTree`/`WithLanguageServiceAndSnapshot`/`GetLanguageServiceWithAutoImports`　`// Go: session.go:*`（**并发**）
- [ ] 调度防抖：`ScheduleSnapshotUpdate`/`ScheduleDiagnosticsRefresh`/`scheduleIdleCacheClean`/`warmAutoImportCache`/对应 cancel　`// Go: session.go:*`（**并发**）
- [ ] 更新与监听：`updateSnapshot(Ref)`/`updateWatch`/`updateWatches`/`WaitForBackgroundTasks`/`flushChanges`/`Close`　`// Go: session.go:*`（**并发**：watchRequestTimeout）
- [ ] telemetry/诊断发布：`StartPerformanceTelemetry`/`sendPerformanceTelemetry`/`collectProjectInfoTelemetry`/`countFileStats`/`publishProgramDiagnostics`/`publishProjectDiagnostics`/`EnqueuePublishGlobalDiagnostics`/`triggerATAForUpdatedProjects`　`// Go: session.go:*`
- [ ] `Configure`/`InitializeWithUserConfig`/refresh*IfNeeded　`// Go: session.go:*`

### `api.rs` / `client.rs` / `autoimport.rs`

- [ ] `api.rs`：`Session::{APIOpenProject,APIUpdateWithFileChanges}`　`// Go: api.go:*`
- [ ] `client.rs`：`Client` trait（WatchFiles/UnwatchFiles/RefreshDiagnostics/PublishDiagnostics/RefreshInlayHints/RefreshCodeLens/ProgressStart/ProgressFinish/SendTelemetry/IsActive）　`// Go: client.go:Client`
- [ ] `autoimport.rs`：`autoImportBuilderFS`/`autoImportRegistryCloneHost`（impl autoimport.RegistryCloneHost）　`// Go: autoimport.go:*`

### Cargo / crate 接线

- [ ] 5 个 `Cargo.toml`（`tsgo_project` + 4 子 crate），path deps 镜像 Go import 边
- [ ] 根 `Cargo.toml` workspace members 追加 5 个目录
- [ ] 各 lib.rs 声明子模块 + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. **叶子先行**：`dirty/`（最纯，测试 `syncmap_test.go` 的 proxyFor）→ `background/queue`（`queue_test.go`）→ `logging/logtree`（`logtree_test.go`，目前几乎空）→ `ata/validatepackagename`（`validatepackagename_test.go` 11 子用例，纯函数最易）。
2. **缓存族**：`refcountcache`（`refcountcache_test.go` parseCache/extendedConfigCache）+ `extendedconfigcache`（`extendedconfigcache_test.go` ownership）。
3. **FS 层**：`overlayfs`（`overlayfs_test.go:TestProcessChanges` 8 子用例，纯状态机）→ `snapshotfs`（`snapshotfs_test.go` 5 函数 ~40 子用例，含 realpath 别名）。
4. **监听纯函数**：`watch`（`watch_test.go` getPathComponentsForWatching/nil clone）。
5. **ATA 发现**：`ata/discovertypings`（`discovertypings_test.go` 9 子用例）→ `ata/ata`（`ata_test.go` ~26 子用例，依赖 projecttestutil）。
6. **项目/集合/快照集成**：`project`/`projectcollection`/`snapshot`/`session`（依赖 compiler/checker/ls 全就绪，大量集成测试用 `projecttestutil` → 多数 P8 末尾收口，部分 P10）。

## 与 Go 的已知偏离（divergence）

- **裸指针 → id/Arc**：`programCounter` 的 `map[*Program]int` → `FxHashMap<ProgramId,i32>`；`fileAssociations map[*ast.SourceFile]int` → `FxHashMap<SourceFileId,usize>`（AST 用 arena id，PORTING §5）。
- **`context.Context` → 显式取消 + 参数**：requestID/locale/clientCapabilities/cancel 全显式传。
- **`testing/synctest` 虚拟时钟**（watchtimeout/部分 session 测试用）→ Rust 用可注入 `Clock` trait + 手动推进 + `crossbeam` 确定性调度（见 tests.md）。
- **`sync.Cond` → `std::sync::Condvar`**（CheckerPool）；`sync.WaitGroup` → 自管 join 集合（background.Queue 的 `wg.Go` 是 Go 1.25 新 API）。
- **`SyncMap.proxyFor`**：保留竞争路由语义；Rust 用 `Arc<Mutex<Entry>>` + 原子 `LoadOrStore`，败者持 `proxy_for`。
- **`xxh3` 128-bit hash**：必须与 Go `zeebo/xxh3` 字节级一致（缓存 key），否则增量复用失效——执行期用已知向量验证。
- **`runtime/metrics` telemetry**：Rust 无直接等价，`logRuntimeMetrics`/部分 `sendPerformanceTelemetry` 标 `// DEFER`，不影响功能正确性。

## 转交 / 推迟（DEFER）

- 大量集成测试依赖 `testutil/projecttestutil`（mock client / typings installer / session init）→ 该设施在 **P10**；故 `session_test.go`/`project_test.go`/`projectlifetime_test.go`/`projectcollectionbuilder_test.go`/`projectreferencesprogram_test.go`/`bulkcache_test.go`/`customconfigfilename_test.go`/`configfilechanges_test.go`/`projectcollectiondefaultproject_test.go`/`untitled_test.go`/`snapshot_test.go`/`snapshotfs_test.go`/`ata_test.go` 的执行 `// DEFER`，但 tests.md 仍逐子用例列出以便届时直接跑。`// DEFER(P10) / blocked-by: projecttestutil`
- 纯函数/纯数据结构测试（dirty/background/logging/validatepackagename/overlayfs processChanges/watch 路径函数/refcountcache）可在 P8 内收口（不依赖 projecttestutil）。
- `runtime/metrics` 与 npm 真实安装（installnpmpackages 用 mock executor，可在 P8 测）的系统集成 → P9/P10。
