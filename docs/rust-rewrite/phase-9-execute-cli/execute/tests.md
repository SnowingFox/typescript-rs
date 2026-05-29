# execute: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：8 文件 / **50** `func Test*` / 约 **740** 子用例（≈338 baseline 子场景 + ≈249 edit + 53 真单测/race 子用例）。

## 测试分类（关键）

execute 的 50 个 `func Test*` 分三类，处理方式不同：

| 类别 | 文件 / 函数 | 数量 | 本 phase 处理 |
|---|---|---|---|
| **A. 真单测（纯逻辑/图算法）** | `build/graph_test.go::TestBuildOrderGenerator`、`tsc/extendedconfigcache_test.go::TestExtendedConfigCacheExtendsCircularity` | 2 func / 12 子用例 | **本 phase 1:1 移植 + 实测对齐**（不依赖 baseline/testdata） |
| **B. 并发 race 测试** | `tsctests/watcher_race_test.go`（6 func） | 6 func | **本 phase 移植**为线程压测（断言无死锁/panic；`-race` → ThreadSanitizer/loom） |
| **C. baseline 对拍（依赖 testdata）** | `tsctests/tsc_test.go`(16)、`tscbuild_test.go`(22)、`tscwatch_test.go`(2)、`showconfig_test.go`(1) | 41 func / ~338 子场景 + ~249 edit | **`—` DEFER P10**：expected = `baselines/reference/{tsc,tsbuild,tscWatch,...}/...js` 基线文件，需 testdata + harnessutil/fsbaselineutil/baseline 框架 |
| **D. 测试主入口（harness）** | `tsctests/testmain_test.go::TestMain` | 1 func | 非行为测试 → 映射为 `mod.rs` 的 baseline `Track()` 初始化 |

> 合计 2+6+41+1 = 50。C 类是 execute 的大头，但其"ground truth"是 testdata 基线文件，本质属 **P10 端到端 parity**；本 phase 只保证 A/B 类真单测过，并保证 C 类涉及的实现逻辑（CLI 分发、增量、build、watch）可跑通。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层 func | 类别 |
|---|---|---|---|
| `internal/execute/build/graph_test.go` | `internal/execute/build/mod.rs`（`#[cfg(test)]`）或 `tests/graph.rs` | 1 | A |
| `internal/execute/tsc/extendedconfigcache_test.go` | `internal/execute/tsc/extendedconfigcache.rs`（`#[cfg(test)]`）或 `tests/extendedconfigcache.rs` | 1 | A |
| `internal/execute/tsctests/watcher_race_test.go` | `internal/execute/tsctests/tests/watcher_race.rs` | 6 | B |
| `internal/execute/tsctests/tsc_test.go` | `tsctests` baseline runner | 16 | C → P10 |
| `internal/execute/tsctests/tscbuild_test.go` | `tsctests` baseline runner | 22 | C → P10 |
| `internal/execute/tsctests/tscwatch_test.go` | `tsctests` baseline runner | 2 | C → P10 |
| `internal/execute/tsctests/showconfig_test.go` | `tsctests` baseline runner | 1 | C → P10 |
| `internal/execute/tsctests/testmain_test.go` | `tsctests/mod.rs` 测试初始化 | 1 | D |

---

# A 类：真单测（本 phase 必过）

## `build/graph_test.go` → `TestBuildOrderGenerator`

> Go: 表驱动 `testCases []*buildOrderTestCase{ name, projects, expected, circular }`，对每个 case 构造 A–J 十个 composite 项目（依赖图：A→{B,C}, B→{C,D}, C→{D,E}, F→{E}, H→{I}, I→{J}, J→{H,E}），用 `ParseBuildCommandLine(["--build","--dry", ...projects])` + `NewOrchestrator` + `GenerateGraph(nil)`，断言：
> 1. `Order()` 映射回 project 名 == `expected`（`assert.DeepEqual`）；
> 2. `verifyDeps`（每个项目的 `Upstream`/`Downstream` 与 deps/reverseDeps 一致，且只含已在 buildOrder 中出现的）；
> 3. 非环时校验 child 在 parent 之后（拓扑序）；
> 4. `GenerateGraphReusingOldTasks()` 重算 order 仍 == `expected`；
> 5. `--watch` 模式重建图后 `verifyDeps(..., hasDownStream=true)`（downstream 仅 watch 模式填充）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `build_order_specify_two_roots` | 两根项目的构建序 | `["A","G"]` → `["D","E","C","B","A","G"]`（circular=false） | `graph_test.go:TestBuildOrderGenerator/"specify two roots - A,G"` | |
| `build_order_multiple_parts_A` | 单根整图序 | `["A"]` → `["D","E","C","B","A"]` | `.../"multiple parts of the same graph in various orders - A"` | |
| `build_order_multiple_parts_A_C_D` | 多根去重整序 | `["A","C","D"]` → `["D","E","C","B","A"]` | `.../"...- A,C,D"` | |
| `build_order_multiple_parts_D_C_A` | 顺序无关性 | `["D","C","A"]` → `["D","E","C","B","A"]` | `.../"...- D,C,A"` | |
| `build_order_F` | 子图序 | `["F"]` → `["E","F"]` | `.../"other orderings - F"` | |
| `build_order_E` | 单叶 | `["E"]` → `["E"]` | `.../"other orderings - E"` | |
| `build_order_F_C_A` | 跨子图合并序 | `["F","C","A"]` → `["E","F","D","C","B","A"]` | `.../"other orderings - F,C,A"` | |
| `build_order_circular_H` | 环检测仍返回序 | `["H"]` → `["E","J","I","H"]`（circular=true） | `.../"returns circular order - H"` | |
| `build_order_circular_A_H` | 含环 + 正常子图 | `["A","H"]` → `["D","E","C","B","A","J","I","H"]`（circular=true） | `.../"returns circular order - A,H"` | |

每个 case 还须断言（同一 `#[test]` 内）：
- [ ] `verify_deps`：`upstream(project) ⊆ deps[project]` 且只含已在 order 前缀中的；`downstream(project)` 在非 watch 模式为空、watch 模式 ⊆ reverseDeps。
- [ ] `generate_graph_reusing_old_tasks()` 后 order 不变。
- [ ] 非环 case：`index(child) > index(parent)`（拓扑序）。
- [ ] watch 模式 `verify_deps(hasDownStream=true)`。

> 依赖：`build::Orchestrator`/`GenerateGraph`/`Order`/`Upstream`/`Downstream`、`tsoptions::ParseBuildCommandLine`、`tsctests::NewTscSystem`。**不依赖** emit/checker —— 是 build 最早可验证的真单测。

## `tsc/extendedconfigcache_test.go` → `TestExtendedConfigCacheExtendsCircularity`

> Go: 回归测试——tsconfig `extends` 形成环时应产出**循环诊断（code 18000）**而不是死锁。每个子用例建内存 FS（`vfstest.FromMap`，大小写不敏感），用 `tsc.ExtendedConfigCache{}` + `tsoptions.GetParsedCommandLineOfConfigFile`，断言 `cmd != nil` 且 `cmd.Errors` 含 code 18000（`assertHasCircularityDiagnostic`）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `extended_config_self_referencing_extends` | 自引用 extends 不死锁、报环 | `tsconfig→base.json`，`base.json→base.json` → `cmd != None` 且 errors 含 code 18000 | `extendedconfigcache_test.go:.../"self-referencing extends"` | |
| `extended_config_mutual_extends_cycle` | 互相 extends 成环 | `tsconfig→other.json`，`other→tsconfig` → 同上 | `.../"mutual extends cycle"` | |
| `extended_config_case_insensitive_self_extends` | 大小写不敏感 FS 下 `./Base.json` 与 `./base.json` 归一为同一缓存键，用 canonical path 防死锁 | `tsconfig→Base.json`，`base.json→base.json` → 同上 | `.../"case-insensitive self-referencing extends"` | |

- [ ] 每个子用例 `#[test]`（或 `rstest` 参数化），断言 `code() == 18000` 存在于 `cmd.errors`。
- [ ] **死锁回归**：测试必须在并发安全缓存（per-entry Mutex）下完成，不超时（Rust 用 `#[test]` + 合理超时；可加 `std::thread` + `join` 超时守卫）。

> 依赖：`tsc::ExtendedConfigCache`、`tsoptions::GetParsedCommandLineOfConfigFile`、`vfstest::from_map`（P1）。

---

# B 类：Watcher 并发 race 测试（本 phase 移植）

> Go: `watcher_race_test.go`，`createTestWatcher` 建最小 watch 项目（a.ts/b.ts/tsconfig.json，`--watch`），返回 `*execute.Watcher` + `TestSys`。各测试从多 goroutine 并发调 `w.DoCycle()` 同时改文件，**用 `go test -race` 检测数据竞争**；断言 = 不死锁、不 panic、`wg.Wait()` 返回。
> Rust 等价：用 `std::thread` 起多线程跑 `watcher.do_cycle()` + FS 写，断言无 panic/死锁；竞态检测用 `cargo +nightly test -Zsanitizer=thread` 或 `loom` 模型检查 `Watcher::do_cycle` 的 `Mutex`/状态访问。

| Rust 测试 | 验证内容 | 并发形态 | Go 对照 | 完成 |
|---|---|---|---|---|
| `watcher_concurrent_do_cycle` | 8 线程 ×10 次：写 a.ts + `do_cycle()` | 写+循环 | `watcher_race_test.go:TestWatcherConcurrentDoCycle` | |
| `watcher_do_cycle_with_concurrent_state_reads` | 4 写线程 + 8 纯 `do_cycle` 读线程 | 写/读混合 | `TestWatcherDoCycleWithConcurrentStateReads` | |
| `watcher_concurrent_file_changes_and_do_cycle` | 4 创建 + 1 删除 + 4 `do_cycle` | 增删改+循环 | `TestWatcherConcurrentFileChangesAndDoCycle` | |
| `watcher_rapid_config_changes` | 3 改 tsconfig + 2 改源 + 4 读，循环 | 配置抖动 | `TestWatcherRapidConfigChanges` | |
| `watcher_concurrent_do_cycle_no_changes` | 16 线程 ×50：无文件变化的 `do_cycle`（早返回路径） | 空转 | `TestWatcherConcurrentDoCycleNoChanges` | |
| `watcher_alternating_modify_and_do_cycle` | 1 写线程持续改 + 4+4 `do_cycle` | 边写边循环 | `TestWatcherAlternatingModifyAndDoCycle` | |

- [ ] 断言：所有线程 `join()` 成功，无 panic，无死锁（加超时守卫）。
- [ ] 覆盖 `Watcher.mu`(Mutex)、`configModified`/`program`/`config`/`sourceFileCache`(SyncMap)、`fileWatcher` watch state 的并发访问。

> 依赖：`execute::{CommandLine, Watcher}`、`tsctests::{newTestSys, fsFromFileMap}`。这是 watcher 并发正确性的守门测试，**不**依赖 baseline。

---

# C 类：baseline 对拍测试（`—` DEFER → P10）

这些测试的 ground truth 是 `baselines/reference/` 下的 `.js` 基线文件（`baseline.Run`/`baseline.Track`）。每个 `subScenario`（及其 `edits`）跑一遍 `execute.CommandLine`，把 FS 输入、命令、退出码、输出、emit 文件、program 状态、`.tsbuildinfo`（+ `.readable.baseline.txt`）序列化进 baseline 文本对拍；同时把"增量构建"与"全量重建"对拍（`getDiffForIncremental`，发现 unexpected diff 即失败）。

**为何 DEFER P10**：依赖 ① `testdata` 基线文件全集；② `testutil/baseline`、`testutil/harnessutil`、`testutil/fsbaselineutil`、`testutil/stringtestutil`（P10 测试设施）；③ 与 Strada 输出逐字符对齐。本 phase 保证这些场景背后的实现路径（init/version/showConfig/help、单次/增量/watch 编译、`-b` 编排）逻辑正确即可，文本对拍统一在 P10 接。

> 下面逐 func 列出 `subScenario`（即表驱动子用例）名，作为 P10 的实现/对拍清单。`scenario` 文件夹见 `getBaselineSubFolder()`（tsc / tscWatch / tsbuild / tsbuildWatch）+ `run(t, scenario)` 的第二参。完成列统一 `—`(P10)。

## `tsc_test.go`（16 func，scenario 见各 `run` 第二参）

### `TestTscCommandline`（→ commandLine，30 子场景 + edits）　`// Go: tsc_test.go:TestTscCommandline`
`—` 子场景：show help (DiagnosticsPresent_OutputsSkipped) / 同上(host 无终端宽度) / NO_COLOR / FORCE_COLOR / NO_COLOR+FORCE_COLOR / when build not first argument / Initialized TSConfig with {files,boolean,enum,list,list+enum,incorrect option,incorrect value,advanced,--help,--watch,tsconfig.json} / help / help all / Parse --lib option with file name / Project is empty string / Parse -p / Parse -p with path to tsconfig file / Parse -p with path to tsconfig folder / Parse enum type options / Parse watch interval option / 同上 without tsconfig.json / Config with references and empty file refers to noEmit / locale / bad locale。

### `TestTscComposite`（→ composite）　`// Go: tsc_test.go:TestTscComposite`
`—` 子场景：when setting composite {false,null,false but has tsbuildinfo,false+tsbuildinfo null} on command line / converting to modules / synthetic jsx import of ESM from CJS {no crash no jsx element, error on jsx element}。

### `TestTscDeclarationEmit`（→ declarationEmit）　`// Go: tsc_test.go:TestTscDeclarationEmit`
`—` 子场景：declaration file referenced through triple slash {,但 uses no references} / used inferred type from referenced project / inferred export reuse imported type alias across module boundary / reports dts generation errors {,with incremental}（两组） / when using Windows paths and uppercase letters（+ symlink 相关子场景：same version referenced through source and symlinked package {,indirect link} / pkg references sibling package through indirect symlink / resolves the symlink path）。

### `TestTscExtends`（→ extends）　`// Go: tsc_test.go:TestTscExtends`
`—` 子场景：configDir template(+suffix 变体) / building solution with projects extends config with include / building project uses reference and both extend config with include。

### `TestForceConsistentCasingInFileNames`（→ forceConsistentCasingInFileNames）　`// Go: tsc_test.go:TestForceConsistentCasingInFileNames`
`—` 子场景：with relative and non relative file resolutions / file included from multiple places with different casing / with type ref from file / with triple slash ref from file / two files exist on disk that differs only in casing。

### `TestTscIgnoreConfig`（→ ignoreConfig）　`// Go: tsc_test.go:TestTscIgnoreConfig`
`—`（`--ignoreConfig` 相关子场景，见源 1123–1175 段）。

### `TestTscIncremental`（→ incremental，子场景大户）　`// Go: tsc_test.go:TestTscIncremental`
`—` 子场景：const enums(+suffix) / serializing error chain / serializing composite project / change to modifier of class expression field {with declaration emit enabled, } / when passing filename for buildinfo on commandline / when passing rootDir from commandline / with only dts files / rootDir in tsconfig / tsbuildinfo has error / global file added signatures updated / react-jsx-emit-mode no backing types {,under --strict} / change to type used as global through export {,through indirect import} / when file is deleted / generates typerefs correctly / option changes with {composite,incremental} / when there is bind diagnostics thats ignored / Compile incremental with case insensitive file names / const enums with refCycle / internal symbolname in tsbuildInfo / js file with import in jsdoc in composite project（各含多 edit）。

### `TestTscLibraryResolution`（→ libraryResolution）　`// Go: tsc_test.go:TestTscLibraryResolution`
`—` 子场景：with config / with config with libReplacement / unknown lib / when noLib toggles。

### `TestTscListFilesOnly`（→ listFilesOnly）　`// Go: tsc_test.go:TestTscListFilesOnly`
`—` 子场景：loose file / combined with incremental。

### `TestTscModuleResolution`（→ moduleResolution）　`// Go: tsc_test.go:TestTscModuleResolution`
`—` 子场景：impliedNodeFormat differs between projects for shared file / shared resolution should not report error / when resolution is not shared / pnpm style layout / package json scope / alternateResult / handles cache when two projects use different module resolution settings / resolution from d.ts of referenced project。

### `TestTscNoCheck`（→ noCheck）　`// Go: tsc_test.go:TestTscNoCheck`
`—`（`--noCheck` 子场景，见源 3067–3149）。

### `TestTscNoEmit`（→ noEmit）　`// Go: tsc_test.go:TestTscNoEmit`
`—` 子场景：syntax errors / semantic errors / dts errors / dts errors without dts enabled / composite / incremental declaration / incremental（+ "changes …"/"changes with initial noEmit …" 派生）/ dts errors with declaration enable changes {,with incremental, with incremental as modules, with multiple files} / does not go in loop when watching when no files are emitted(+suffix) / when project has strict true。

### `TestTscNoEmitOnError`（→ noEmitOnError）　`// Go: tsc_test.go:TestTscNoEmitOnError`
`—` 子场景：noEmitOnError {,with declaration, with incremental, with declaration with incremental} / syntax errors / semantic errors / dts errors / file deleted before fixing error with noEmitOnError。

### `TestTscProjectReferences`（→ projectReferences）　`// Go: tsc_test.go:TestTscProjectReferences`
`—` 子场景：when project references composite project with noEmit / references composite / project reference is not built / project contains invalid project reference / default import interop uses referenced project settings / referenced project with esnext module disallows synthetic default imports / referencing ambient const enum with preserveConstEnums / importing const enum with preserveConstEnums and verbatimModuleSyntax / rewriteRelativeImportExtensionsProjectReferences{1,2,3}。

### `TestTypeAcquisition`（→ typeAcquisition）　`// Go: tsc_test.go:TestTypeAcquisition`
`—` 子场景：parse tsconfig with typeAcquisition。

### `TestGenerateTrace`（→ generateTrace）　`// Go: tsc_test.go:TestGenerateTrace`
`—` 子场景：generateTrace generates types file / generateTrace with multiple files and complex types。

## `tscbuild_test.go`（22 func，scenario 多为 tsbuild）

### `TestBuildCommandLine`　`// Go: tscbuild_test.go:TestBuildCommandLine`
`—`：emitDeclarationOnly on commandline(+suffix) / emitDeclarationOnly false on commandline(+suffix) / help / locale / bad locale / different options / different options with incremental。
### `TestBuildClean`　`// Go: tscbuild_test.go:TestBuildClean`
`—`：file name and output name clashing / tsx with dts emit。
### `TestBuildConfigFileErrors`　`// Go: tscbuild_test.go:TestBuildConfigFileErrors`
`—`：when tsconfig extends the missing file / reports syntax errors in config file / missing config file / reports syntax errors in config file。
### `TestBuildDemoProject`　`// Go: tscbuild_test.go:TestBuildDemoProject`
`—`：in master branch ... reports no error / in circular branch reports error by stopping build / in bad-ref branch reports files not in rootDir at import location / in circular is set in the reference / updates with circular reference / updates with bad reference。
### `TestBuildEmitDeclarationOnly`　`// Go: tscbuild_test.go:TestBuildEmitDeclarationOnly`　`—`
### `TestBuildFileDelete`　`// Go: tscbuild_test.go:TestBuildFileDelete`
`—`：detects deleted file / deleted file without composite。
### `TestBuildInferredTypeFromTransitiveModule`　`// Go: ...`
`—`：inferred type from transitive module {,with isolatedModules} / reports errors in files affected by change in signature with isolatedModules。
### `TestBuildInferredTypeFromMonorepoReference`　`—`：inferred type from referenced project that references another project in monorepo。
### `TestBuildJavascriptProjectEmit`　`—`：loads js-based projects and emits them correctly。
### `TestBuildLateBoundSymbol`　`—`：interface is merged and contains late bound member。
### `TestBuildModuleSpecifiers`　`—`（module specifier 子场景）。
### `TestBuildOutputPaths`　`—`：when rootDir {not specified, not specified and is composite, specified, specified but not all files belong to rootDir, 同前 and is composite}。
### `TestBuildProgramUpdates`（子场景大户）　`// Go: ...:TestBuildProgramUpdates`
`—`：when referenced project change introduces error in downstream then fixes it / declarationEmitErrors {when fixing error files all emitted, when file with no error changes, introduceError when fixing errors only changed file emitted, introduceError when file with no error changes} / works when noUnusedParameters changes to false / works with extended source files / works correctly when project with extended config is removed / tsbuildinfo has error / when root is source from project reference {,with composite}。
### `TestBuildProjectsBuilding`　`—`：skips builds downstream projects if upstream have errors with stopBuildOnErrors {, when test does not reference core}。
### `TestBuildProjectReferenceWithRootDirInParent`　`—`：builds correctly / reports error for same tsbuildinfo file {because no rootDir in base, , without incremental, without incremental with tsc} / reports no error when tsbuildinfo differ。
### `TestBuildReexport`　`—`：Reports errors correctly。
### `TestBuildResolveJsonModule`　`—`：include only {, without outDir, with json not in rootDir, with json without rootDir but outside configDirectory} / include of json along with other include {, and file name matches ts file} / files containing json file / include and files / sourcemap / without outDir / importing json module from project reference。
### `TestBuildRoots`　`—`：when root file is from referenced project {, and shared is first}（两组）。
### `TestBuildSample`（最大，scenario sample）　`// Go: ...:TestBuildSample`
`—`：reportErrors（+suffix 派生）/ builds correctly when {outDir specified, declarationDir specified, project not composite or no references} / does not write any files in a dry build / removes all files it built / cleaning project in not build order doesnt throw / always builds under force option / can detect when and what to rebuild / when input file text does not change but modified time changes / when declarationMap changes / indicates would skip builds during dry build / rebuilds from start if force / tsbuildinfo has error / rebuilds completely when version mismatch / rebuilds when extended config changes / building project in not build order doesnt throw / builds downstream even if upstream have errors / listFiles / listEmittedFiles / explainFiles / sample / when logic specifies tsBuildInfoFile / when {declaration,target,module,esModuleInterop} option changes / reports error if input file is missing {,with force}。
### `TestBuildTransitiveReferences`　`—`：change builds changes and reports found errors message / non local change does not start build of referencing projects / builds when new file is added and its subsequent updates / change builds … with circular references。
### `TestBuildSolutionProject`　`—`（solution project 子场景）。
### `TestBuildProjectReferenceRedirectWithMultipleSubProjects`　`—`（redirect 子场景）。

## `tscwatch_test.go`（2 func，scenario tscWatch / tsc）

### `TestWatch`（→ Watch，~29 子场景）　`// Go: tscwatch_test.go:TestWatch`
`—`：watch with no tsconfig / with tsconfig and incremental / skips build when no files change / rebuilds when file is modified / rebuilds when source file is deleted / detects new file resolving failed import / detects imported file added in new directory / detects imported directory removed / detects import path restructured / rebuilds when tsconfig include pattern adds file / rebuilds when tsconfig modified to change strict / detects file added to previously non-existent include path / detects new file in existing include directory / detects file added in new nested subdirectory / detects file added in multiple new subdirectories simultaneously / detects nested subdirectory removed and recreated / detects node modules package added / detects node modules package removed / handles tsconfig deleted / handles tsconfig with extends base modified / rebuilds when tsconfig touched but content unchanged / with tsconfig files list entry deleted / detects module going missing then coming back / detects scoped package installed / detects package json types field edited / detects at-types package installed later / detects file renamed and renamed back / detects file deleted and new file added simultaneously / handles file rapidly recreated / detects change in symlinked file。
### `TestTscNoEmitWatch`　`// Go: tscwatch_test.go:TestTscNoEmitWatch`　`—`（noEmit watch 子场景）。

## `showconfig_test.go`（1 func，scenario showConfig，17 子场景）

### `TestShowConfig`　`// Go: showconfig_test.go:TestShowConfig`
`—` 子场景：Default initialized TSConfig / Show TSConfig with files options / with boolean value compiler options / with enum value compiler options / with list compiler options / with list compiler options with enum value / with incorrect compiler option / with incorrect compiler option value / with advanced options / with compileOnSave and more / with paths and more / with include filtering files / with references / with exclude / with files and include / with transitively implied options / with exclude and outDir。

---

# D 类：测试主入口

### `testmain_test.go` → `TestMain`　`// Go: testmain_test.go:TestMain`
`core.ApplyDebugStackLimit()` + `defer baseline.Track()()` + `m.Run()`。Rust 无 `TestMain` 等价；映射为 `tsctests` 测试模块的一次性初始化（baseline 跟踪在 P10 接入）。非行为断言。

---

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（50 个）都已映射（A 类逐子用例 + 期望值；B 类逐 func；C 类逐 func + 子场景名列出，标 `—`P10；D 类说明）。
- [x] A 类表驱动子用例逐行列出（build order 9 + extends circularity 3），expected 取自 Go 字面量（构建序数组、code 18000）。
- [x] C 类子场景名取自源码 `subScenario` 字面量（ground truth 为 testdata 基线 → P10）。
- [x] 每条带 `// Go:` 锚点。
- [x] 与 impl.md 双向对齐：
  - `TestBuildOrderGenerator` ↔ `build/mod.rs` 的 `Orchestrator::generate_graph/order/upstream/downstream`、`setupBuildTask`(环检测)。
  - `TestExtendedConfigCacheExtendsCircularity` ↔ `tsc/extendedconfigcache.rs`。
  - 6 race 测试 ↔ `watcher.rs::Watcher::do_cycle`/`Mutex` 状态。
  - C 类各场景 ↔ 根 `command_line`/`tsc_compilation`、`tsc/{help,init,emit,diagnostics}`、`incremental/*`、`build/*`（实现已在 impl.md 列 TODO）。

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 41 个 baseline 对拍 func（tsc 16 / tsbuild 22 / tscWatch 2 / showConfig 1，~338 子场景 + ~249 edit） | ground truth = `baselines/reference` 基线文件；需 testdata + `testutil/{baseline,harnessutil,fsbaselineutil,stringtestutil}` 框架；须与 Strada 输出逐字符对齐 | **P10** |
| `tsctests` baseline runner（`run`/`getDiffForIncremental`/`serializeState`/`baselinePrograms`/`.readable.baseline.txt`） | 是 baseline 框架本体 | **P10**（`NewTscSystem`/`TestSys` 的基础部分本 phase 先落，供 A/B 类用） |
| `.tsbuildinfo` 内容与 Strada 完全一致（xxh3 签名、紧凑编码字节级） | 需 golden 对拍 + 全量编译管线 | **P10**（本 phase 做 round-trip 自测：snapshot↔buildInfo） |
