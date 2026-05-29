# testutil: 实现方案（impl.md）

**crate**：`tsgo_testutil`　**目标**：重建 Go 的 **baseline 对拍框架** —— "真编译 + 把实际产物与 `testdata/baselines/reference` 逐字节比对"的全套底座，供 `tsgo_testrunner` / `tsgo_fourslash` 复用。
**依赖（crate）**：`tsgo_compiler` `tsgo_checker` `tsgo_printer` `tsgo_sourcemap` `tsgo_tsoptions` `tsgo_core` `tsgo_collections` `tsgo_tspath` `tsgo_vfs`（含 `vfstest`）`tsgo_ast` `tsgo_diagnostics` `tsgo_diagnosticwriter` `tsgo_repo` `tsgo_bundled` `tsgo_outputpaths` `tsgo_locale` `tsgo_parser` `tsgo_lsp` `tsgo_project`（仅 lsptestutil/projecttestutil 子模块需要）。外部：`tempfile` `rstest`（dev）。
**Go 源**：`internal/testutil/`（27 个非测试文件 / 14 个子目录 / 仅 1 个 `*_test.go`、2 个 `func Test*`）

## 这个包是什么（业务说明）

`testutil` 是整个仓库的**测试基础设施根**。它本身几乎不含被单测的"业务逻辑"，而是给 `testrunner`（conformance）和 `fourslash`（语言服务）提供四类能力：

1. **baseline 引擎**（`baseline/`）：`Run(t, fileName, actual, opts)` —— 把测试产生的"实际字符串"写到 `testdata/baselines/local/...`，并与 `testdata/baselines/reference/...` 比较，不一致就 `t.Errorf("baseline 变了，跑 hereby baseline-accept")`。这是**逐字节对拍**的核心机制。还含 submodule diff 模式（把 corsa 输出与 TS 原始 submodule baseline 做 diff，分 `submodule` / `submoduleAccepted` / `submoduleTriaged` 三档）。
2. **编译 harness**（`harnessutil/`）：`CompileFiles(...)` —— 从测试单元（多文件 + symlink + tsconfig）建一个内存 VFS、跑真编译（program → 各类 diagnostics → emit），把输出文件录进 `OutputRecorderFS`，整理成确定性顺序的 `CompilationResult`。这是"真编译"那一半。
3. **baseline 生成器**（`tsbaseline/`）：把 `CompilationResult` 渲染成 8 种 baseline 文本格式——`.errors.txt`（诊断）、`.js` / `.d.ts`（emit）、`.js.map` + sourcemap record、`.types` / `.symbols`（类型/符号 walker）、module resolution trace。每个 `Do*Baseline` 内部调 `baseline.Run`。
4. **零散 helper**：`testutil.go`（panic 断言）、`parsetestutil` / `emittestutil`（P2-P5 单测用）、`lsptestutil`（内存 LSP client，fourslash 用）、`projecttestutil`（project session mock）、`stringtestutil.Dedent`、`filefixture` / `fixtures`（bench/lifecycle fixture）、`race`（race 检测开关）、`jstest`（调真 node 跑参照）。

> 在 Rust 移植里，`tsgo_testutil` 是 P10 第一个落地的 crate：testrunner / fourslash 都 `use tsgo_testutil::{baseline, harnessutil, tsbaseline}`。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3。本包特有：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `testing.T` / `testing.TB`（贯穿全包） | `&Harness`（自定义）或直接 `&mut TestCtx` | Go 用 `*testing.T` 做断言/skip/并行/`t.Run`。Rust 侧 P10 不能用 Go 的 testing 树，需自建轻量 harness：`fn run(name, actual, opts)` 收集失败、`t.Run` → 嵌套子 case 名。**这是 P10 最大的偏离**，单列下节。 |
| `baseline.Options{Subfolder, IsSubmodule, DiffFixupOld/New, SkipDiffWithOld}` | `struct BaselineOptions { subfolder: String, is_submodule: bool, diff_fixup_old: Option<Box<dyn Fn(&str)->String>>, ... }` | 闭包字段用 `Box<dyn Fn>` |
| `collections.Set[string]` / `SyncSet[string]` | `IndexSet<String>` / `Mutex<IndexSet<String>>` | submodule 文件名集合、`recordedBaselines` |
| `fstest.MapFile` / `vfstest.FromMap` | `tsgo_vfs::vfstest::from_map`（P1 已落地的内存 VFS） | harness 用内存 FS |
| `OutputRecorderFS`（包裹 vfs.FS，记 WriteFile 顺序） | `struct OutputRecorderFS { inner: Arc<dyn Vfs>, outputs: Mutex<...> }` | 包装器，emit 输出回收 |
| `collections.OrderedMap[string, *TestFile]` | `IndexMap<String, TestFile>` | **emit 输出顺序必须确定**（PORTING §6）：corsa 并行 emit，需按 input 顺序重排 |
| `sync.OnceValue` | `once_cell::sync::Lazy` / `std::sync::OnceLock` | submodule 集合、libfolder map 懒加载 |
| `os.Getenv("TS_TEST_*")` | `std::env::var` | race / single-thread 开关 |
| `patience.Diff` / `UnifiedDiffText`（diff 库） | `similar` crate（unified diff） | baseline diff 文本，见依赖白名单 |

### 偏离重点：Go `testing.T` → Rust harness

Go 的 baseline 体系深度耦合 `testing.T`：`t.Run(name, fn)` 建子测试、`t.Parallel()` 并行、`t.Errorf` 累积失败、`t.Skip` 跳过、`runtime.Caller` 拿调用栈算 tracking 文件名。Rust 的 `#[test]` 不提供等价 API。

**约定**：`tsgo_testutil` 自建一个最小 harness 抽象 `Harness`（或沿用 testrunner 的 `Runner`）：
- `harness.run_baseline(file_name, actual, opts)` 替代 `baseline.Run(t, ...)`；失败累积到 `harness.failures`。
- 子 case（Go `t.Run("error", ...)`）→ Rust 用"前缀 + 闭包"或 `rstest` 参数化；baseline 文件名/子目录沿用 Go 命名。
- `t.Skip`（无 submodule / 不支持的 option）→ Rust 返回 `BaselineOutcome::Skipped(reason)`。

须在本包顶部"所有权模型"小节写明：**保留 baseline 文件名/目录布局 1:1，但把 `*testing.T` 调用点改成 `&Harness` 方法**。

## 文件清单 → Rust 模块

> crate 根 `lib.rs`（对应 `testutil.go`，basename == crate 目录名）。子目录默认作子 module（PORTING §2 命名规则）。

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/testutil/testutil.go` | `internal/testutil/lib.rs` | crate 根：`assert_panics` / `recover_and_fail` / `test_program_is_single_threaded`；`pub mod baseline; pub mod harnessutil; ...` |
| `internal/testutil/baseline/baseline.go` | `internal/testutil/baseline/mod.rs` | `Options` / `run` / `run_against_submodule` / `diff_text` / `write_comparison` / submodule accepted/triaged 集合 |
| `internal/testutil/baseline/testmain.go` | `internal/testutil/baseline/tracking.rs` | `track()` 返回 cleanup 闭包；`recordedBaselines` 全局集合 + `TSGO_BASELINE_TRACKING_DIR` |
| `internal/testutil/harnessutil/harnessutil.go` | `internal/testutil/harnessutil/mod.rs` | `TestFile` / `HarnessOptions` / `CompileFiles` / `CompileFilesEx` / `CompilationResult` / `GetFileBasedTestConfigurations` / `EnumerateFiles` / option 解析 |
| `internal/testutil/harnessutil/recorderfs.go` | `internal/testutil/harnessutil/recorderfs.rs` | `OutputRecorderFS`：包装 vfs，记 emit 写入顺序 |
| `internal/testutil/harnessutil/sourcemap_recorder.go` | `internal/testutil/harnessutil/sourcemap_recorder.rs` | `sourceMapSpanWriter` / `writerAggregator`（sourcemap record 文本生成） |
| `internal/testutil/tsbaseline/error_baseline.go` | `internal/testutil/tsbaseline/error_baseline.rs` | `DoErrorBaseline` / `GetErrorBaseline`（`.errors.txt`） |
| `internal/testutil/tsbaseline/js_emit_baseline.go` | `internal/testutil/tsbaseline/js_emit_baseline.rs` | `DoJSEmitBaseline`（`.js` / `.d.ts` 拼接） |
| `internal/testutil/tsbaseline/type_symbol_baseline.go` | `internal/testutil/tsbaseline/type_symbol_baseline.rs` | `DoTypeAndSymbolBaseline` + `typeWriterWalker`（`.types` / `.symbols`） |
| `internal/testutil/tsbaseline/sourcemap_baseline.go` | `internal/testutil/tsbaseline/sourcemap_baseline.rs` | `DoSourcemapBaseline`（`.js.map`） |
| `internal/testutil/tsbaseline/sourcemap_record_baseline.go` | `internal/testutil/tsbaseline/sourcemap_record_baseline.rs` | `DoSourcemapRecordBaseline`（`.sourcemap.txt`） |
| `internal/testutil/tsbaseline/module_resolution_baseline.go` | `internal/testutil/tsbaseline/module_resolution_baseline.rs` | `DoModuleResolutionBaseline`（trace） |
| `internal/testutil/tsbaseline/util.go` | `internal/testutil/tsbaseline/util.rs` | `tsExtension` 正则、`removeTestPathPrefixes` 等共享 helper |
| `internal/testutil/parsetestutil/parsetestutil.go` | `internal/testutil/parsetestutil/mod.rs` | `ParseTypeScript` / `CheckDiagnostics` / `MarkSyntheticRecursive`（P2/P3 单测复用） |
| `internal/testutil/emittestutil/emittestutil.go` | `internal/testutil/emittestutil/mod.rs` | `CheckEmit`（P5 单测复用） |
| `internal/testutil/stringtestutil/stringtestutil.go` | `internal/testutil/stringtestutil/mod.rs` | `Dedent`（多处单测复用） |
| `internal/testutil/lsptestutil/lspclient.go` | `internal/testutil/lsptestutil/mod.rs` | 内存 `LSPClient` + `SendRequest`/`SendNotification`（**fourslash 依赖**） |
| `internal/testutil/projecttestutil/projecttestutil.go` | `internal/testutil/projecttestutil/mod.rs` | project `Session` mock setup（P8 project 单测复用） |
| `internal/testutil/projecttestutil/clientmock_generated.go` | `internal/testutil/projecttestutil/clientmock.rs` | 生成的 client mock → Rust 手写或 `mockall` |
| `internal/testutil/projecttestutil/npmexecutormock_generated.go` | `internal/testutil/projecttestutil/npmexecutormock.rs` | 同上 |
| `internal/testutil/fsbaselineutil/differ.go` | `internal/testutil/fsbaselineutil/mod.rs` | `SanitizeInternalSymbolName` + fs diff helper |
| `internal/testutil/filefixture/filefixture.go` | `internal/testutil/filefixture/mod.rs` | `Fixture` / `FromFile` / `FromString` |
| `internal/testutil/fixtures/benchfixtures.go` | `internal/testutil/fixtures/mod.rs` | bench fixture（低优先，DEFER 可） |
| `internal/testutil/autoimporttestutil/fixtures.go` | `internal/testutil/autoimporttestutil/mod.rs` | monorepo/lifecycle session fixture（P8 autoimport 用，可 DEFER） |
| `internal/testutil/jstest/node.go` | `internal/testutil/jstest/mod.rs` | `EvalNodeScript*` / `SkipIfNoNodeJS`（调真 node，可 DEFER） |
| `internal/testutil/race/race.go` + `norace.go` | `internal/testutil/race/mod.rs` | `Enabled` 常量（cfg(feature) 或 build 标志） |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| unified diff（baseline diff 文本） | `similar` | 替代 Go 的 `github.com/peter-evans/patience`；需复刻 `@@= skipped -N, +M lines =@@` 头部改写逻辑 |
| 临时目录（jstest / projecttestutil real-fs） | `tempfile` | |
| 懒加载全局 | `once_cell` | submodule 集合 / libfolder map |
| mock（projecttestutil） | `mockall`（dev，可选） | 或手写 mock |

> `similar` 的 unified diff 输出与 `patience` 不完全一致，须验证 `getBaselineDiff` 产出的 `.diff` 与 reference 中的 `.diff` 逐字节一致（见 baseline.go:getBaselineDiff 的 `@@` 头部正则改写——这是 submodule diff 模式的关键，不能省）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按 TDD 推进序：先 baseline 引擎（最底层）→ harness 编译 → tsbaseline 渲染 → 零散 helper。

### `lib.rs`（Go: `internal/testutil/testutil.go`）

- [ ] `pub fn assert_panics(f: impl FnOnce(), expected: &dyn Any)` — 捕获 panic 并断言相等　`// Go: testutil.go:AssertPanics`
- [ ] `pub fn recover_and_fail(harness, msg)` — panic 时打印 msg + backtrace（对应 `defer testutil.RecoverAndFail`）　`// Go: testutil.go:RecoverAndFail`
- [ ] `pub fn test_program_is_single_threaded() -> bool` — 读 `TS_TEST_PROGRAM_SINGLE_THREADED` env / race 模式　`// Go: testutil.go:TestProgramIsSingleThreaded`

### `baseline/mod.rs`（Go: `internal/testutil/baseline/baseline.go`）

- [ ] `pub struct Options { subfolder, is_submodule, is_submodule_accepted, is_submodule_triaged, diff_fixup_old, diff_fixup_new, skip_diff_with_old }`　`// Go: baseline.go:Options`
- [ ] `pub const NO_CONTENT: &str = "<no content>"`
- [ ] `pub fn run(harness, file_name, actual, opts)` — 写 local + 与 reference 比对；submodule 模式下三档 diff　`// Go: baseline.go:Run`
- [ ] `pub fn run_against_submodule(harness, file_name, actual, opts)`　`// Go: baseline.go:RunAgainstSubmodule`
- [ ] `pub fn diff_text(old_name, new_name, expected, actual) -> String` — unified diff（`similar`）　`// Go: baseline.go:DiffText`
- [ ] `fn get_baseline_diff(...)` — diff + `@@= skipped =@@` 头部行号抹除　`// Go: baseline.go:getBaselineDiff`
- [ ] `fn write_comparison(harness, actual, local, reference, comparing_against_submodule)` — 核心写/比/报错逻辑（含 `.delete` 占位、空内容 panic）　`// Go: baseline.go:writeComparison`
- [ ] `fn read_file_name_set(path) -> IndexSet<String>` + `submodule_accepted_file_names()` / `submodule_triaged_file_names()`（once_cell）　`// Go: baseline.go:readFileNameSet`
- [ ] 常量 `LOCAL_ROOT` / `REFERENCE_ROOT` / `SUBMODULE_REFERENCE_ROOT`（基于 `repo::test_data_path()`）

### `baseline/tracking.rs`（Go: `internal/testutil/baseline/testmain.go`）

- [ ] `pub fn track() -> impl FnOnce()` — 启用 baseline tracking，返回写 tracking 文件的 cleanup　`// Go: testmain.go:Track`
- [ ] `fn record_baseline(harness, relative_path)` — 记录写过的 baseline 路径（去重）　`// Go: testmain.go:recordBaseline`
- [ ] `fn write_recorded_baselines(tracking_path)` + `TSGO_BASELINE_TRACKING_DIR` env　`// Go: testmain.go:writeRecordedBaselines`
- [ ] 全局 `recordedBaselines: Mutex<IndexSet<String>>` + `tracking_initialized` 标志

### `harnessutil/mod.rs`（Go: `internal/testutil/harnessutil/harnessutil.go`）

- [ ] `pub struct TestFile { unit_name: String, content: String }`　`// Go: harnessutil.go:TestFile`
- [ ] `pub struct HarnessOptions { use_case_sensitive_file_names, baseline_file, lib_files, no_types_and_symbols, full_emit_paths, capture_suggestions, ... }`　`// Go: harnessutil.go:HarnessOptions`
- [ ] `pub type TestConfiguration = IndexMap<String, String>` + `NamedTestConfiguration`
- [ ] `pub fn compile_files(harness, input, other, config, tsconfig, cwd, symlinks) -> CompilationResult`　`// Go: harnessutil.go:CompileFiles`
- [ ] `pub fn compile_files_ex(...)` — 建内存 VFS（含 symlink + lib dir）→ `OutputRecorderFS` → host → program → diagnostics → emit　`// Go: harnessutil.go:CompileFilesEx`
- [ ] `fn set_options_from_test_config(...)` — 把 `@option` 解析进 CompilerOptions / HarnessOptions　`// Go: harnessutil.go:SetOptionsFromTestConfig`
- [ ] `fn get_option_value(...)` / `parse_harness_option(...)` — 各 option kind 解析　`// Go: harnessutil.go:getOptionValue`
- [ ] `pub struct CompilationResult { diagnostics, program, options, js: IndexMap, dts: IndexMap, maps: IndexMap, trace, ... }` + `new_compilation_result`（**按 input 顺序重排 emit 输出**）　`// Go: harnessutil.go:newCompilationResult`
- [ ] `CompilationResult::get_source_map_record() -> String`（sourcemap record 文本）　`// Go: harnessutil.go:GetSourceMapRecord`
- [ ] `pub fn enumerate_files(folder, regex, recursive) -> Vec<String>`　`// Go: harnessutil.go:EnumerateFiles`
- [ ] `pub fn get_file_based_test_configurations(harness, settings, vary_by) -> Vec<NamedTestConfiguration>` + `split_option_values` + variation 笛卡尔积　`// Go: harnessutil.go:GetFileBasedTestConfigurations`
- [ ] `pub fn get_config_name_from_file_name(filename) -> String`　`// Go: harnessutil.go:GetConfigNameFromFileName`
- [ ] `pub fn skip_unsupported_compiler_options(harness, options)` — AMD/UMD/System/ES5/outFile 等不支持时 skip　`// Go: harnessutil.go:SkipUnsupportedCompilerOptions`
- [ ] `struct TracerForBaselining`（module resolution trace 净化：版本号 + package.json 缓存改写）　`// Go: harnessutil.go:TracerForBaselining`
- [ ] `struct CachedCompilerHost` + `SourceFileCache`（解析缓存，跨配置变体复用）　`// Go: harnessutil.go:cachedCompilerHost`

### `harnessutil/recorderfs.rs`（Go: `internal/testutil/harnessutil/recorderfs.go`）

- [ ] `pub struct OutputRecorderFS { inner, outputs: Mutex<...> }` impl `Vfs`；`write_file` 记录顺序　`// Go: recorderfs.go:OutputRecorderFS`
- [ ] `outputs() -> Vec<TestFile>`　`// Go: recorderfs.go:Outputs`

### `harnessutil/sourcemap_recorder.rs`（Go: `internal/testutil/harnessutil/sourcemap_recorder.go`）

- [ ] `struct SourceMapSpanWriter` + `WriterAggregator` — 把 decode 后的 mapping 渲染成 record 文本　`// Go: sourcemap_recorder.go:*`

### `tsbaseline/error_baseline.rs`（Go: `internal/testutil/tsbaseline/error_baseline.go`）

- [ ] `pub fn do_error_baseline(harness, path, input_files, errors, pretty, opts)` → `.errors.txt`　`// Go: error_baseline.go:DoErrorBaseline`
- [ ] `pub fn get_error_baseline(harness, input_files, diagnostics, compare, pretty) -> String` — 排序 + 逐文件穿插源码 + 错误计数校验　`// Go: error_baseline.go:GetErrorBaseline`

### `tsbaseline/js_emit_baseline.rs`（Go: `internal/testutil/tsbaseline/js_emit_baseline.go`）

- [ ] `pub fn do_js_emit_baseline(harness, name, header, options, result, tsconfig_files, to_be_compiled, other_files, harness_options, opts)` → `.js` + `.d.ts`　`// Go: js_emit_baseline.go:DoJSEmitBaseline`

### `tsbaseline/type_symbol_baseline.rs`（Go: `internal/testutil/tsbaseline/type_symbol_baseline.go`）

- [ ] `pub fn do_type_and_symbol_baseline(...)` → `.types` + `.symbols`　`// Go: type_symbol_baseline.go:DoTypeAndSymbolBaseline`
- [ ] `struct TypeWriterWalker` — 遍历 AST 每个标识符，记其 type / symbol（full walker）　`// Go: type_symbol_baseline.go:newTypeWriterWalker`

### `tsbaseline/sourcemap_baseline.rs` / `sourcemap_record_baseline.rs` / `module_resolution_baseline.rs` / `util.rs`

- [ ] `pub fn do_sourcemap_baseline(...)` → `.js.map`　`// Go: sourcemap_baseline.go:DoSourcemapBaseline`
- [ ] `pub fn do_sourcemap_record_baseline(...)` → `.sourcemap.txt`　`// Go: sourcemap_record_baseline.go:DoSourcemapRecordBaseline`
- [ ] `pub fn do_module_resolution_baseline(harness, name, trace, opts)`　`// Go: module_resolution_baseline.go:DoModuleResolutionBaseline`
- [ ] `tsExtension` 正则 + `remove_test_path_prefixes` 等共享 util　`// Go: tsbaseline/util.go`

### 零散 helper 子模块

- [ ] `parsetestutil`：`parse_type_script` / `check_diagnostics` / `check_diagnostics_message` / `mark_synthetic_recursive`　`// Go: parsetestutil.go:*`
- [ ] `emittestutil`：`check_emit`　`// Go: emittestutil.go:CheckEmit`
- [ ] `stringtestutil`：`dedent`　`// Go: stringtestutil.go:Dedent`
- [ ] `lsptestutil`：`LSPClient`（内存双向 channel + server）+ `new_lsp_client` / `send_request` / `send_notification`　`// Go: lspclient.go:*`（**fourslash 阻塞依赖**）
- [ ] `projecttestutil`：`setup` / `setup_with_options` / typings installer mock（P8 project 复用，可 DEFER 到需要时）　`// Go: projecttestutil.go:*`
- [ ] `fsbaselineutil`：`sanitize_internal_symbol_name`　`// Go: differ.go:SanitizeInternalSymbolName`
- [ ] `filefixture`：`Fixture` / `from_file` / `from_string`　`// Go: filefixture.go:*`
- [ ] `race`：`ENABLED` 常量（`cfg!(...)` 或 feature）
- [ ] `jstest` / `fixtures` / `autoimporttestutil`：DEFER（仅特定包 bench/lifecycle 用，不阻塞 conformance）`// DEFER(phase-10): blocked-by: 对应业务包单测需要时再补`

### Cargo / crate 接线

- [ ] `internal/testutil/Cargo.toml`（`name = "tsgo_testutil"` + 上列 path deps）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] `lib.rs` 声明 `pub mod baseline; pub mod harnessutil; pub mod tsbaseline; ...` + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. **`baseline::write_comparison` + `run`**（最底层，最易测）：给定 fileName + actual + 一个 reference 临时文件，验证"相等不报错 / 不等报错 / reference 缺失报 new baseline / 空内容 panic"。这是后面一切的地基。
2. **`baseline::diff_text` + `get_baseline_diff`**：验证 unified diff + `@@= skipped =@@` 头部改写与 Go 逐字节一致（用一对 old/new 字符串 + 已知 `.diff` 期望）。
3. **`harnessutil::compile_files`** 的 tracer：单文件 `var x = 1;` → 跑出 `CompilationResult`，断言 `js` 含 `var x = 1;`、`diagnostics` 为空。
4. **`tsbaseline::do_error_baseline`**：一个有错的源 → `.errors.txt` 文本与已知期望一致。
5. 增量补 `do_js_emit_baseline` / `do_type_and_symbol_baseline` / sourcemap 三件套（这几个的 ground truth 直接取 conformance reference 里的对应文件）。
6. `lsptestutil`（fourslash 前置）→ 其余 helper 按需。

## 与 Go 的已知偏离（divergence）

1. **`*testing.T` → `&Harness`**：见上"所有权模型"。所有 `baseline.Run(t, ...)` / `t.Run` / `t.Skip` 调用点改成 harness 方法；baseline 文件名/目录布局保持 1:1。
2. **diff 库**：`patience` → `similar`。必须验证 `.diff` 字节一致（含 `@@= skipped -N, +M lines =@@` 格式），否则 submodule diff baseline 会整片漂移。
3. **emit 输出顺序**：corsa 并行 emit 顺序不确定（Go 已注释说明），Rust 同样并行，须按 `program.get_source_files()` 的 input 顺序重排 `js`/`dts`/`maps`（`new_compilation_result`），保证 baseline 确定性（PORTING §6）。
4. **生成的 mock**（`*_generated.go`）：Go 用代码生成；Rust 侧手写 trait impl 或 `mockall`，行为对齐即可，不要求生成器 1:1。
5. **`runtime.Caller` 算 tracking 文件名**：Rust 无等价；tracking 文件名改用调用方显式传入的 package 标识（或 `module_path!()`）。tracking 仅用于"未用 baseline 检测"，非对拍主路径，可简化。

## 转交 / 推迟（DEFER）

- `jstest`（调真 node.js 跑参照值）、`fixtures`（bench）、`autoimporttestutil`（monorepo lifecycle）：仅特定业务包用，不阻塞 conformance/fourslash。标 `// DEFER(phase-10): blocked-by: 对应业务单测落地时再补`。
- `projecttestutil` 完整 typings-installer mock：依赖 P8 `tsgo_project` 的 session 全量落地；fourslash 用不到它，可在 project 包单测需要时回填。
- `lsptestutil` 必须在 **fourslash 之前**落地（fourslash 的阻塞依赖），不可 DEFER。
