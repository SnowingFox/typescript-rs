# testrunner: 实现方案（impl.md）

**crate**：`tsgo_testrunner`　**目标**：**conformance 测试驱动器** —— 枚举 `testdata/tests/cases/{compiler,conformance}` 下每个 `.ts`/`.tsx` fixture，切成多文件单元、按 `@option` 展开配置变体、跑真编译，再调 `tsgo_testutil::tsbaseline` 产出 8 类 baseline 与 reference 对拍。
**依赖（crate）**：`tsgo_testutil`（baseline / harnessutil / tsbaseline）`tsgo_compiler` `tsgo_checker` `tsgo_ast` `tsgo_parser` `tsgo_scanner` `tsgo_core` `tsgo_tsoptions`（含 `tsoptionstest`）`tsgo_tspath` `tsgo_vfs`（`osvfs`）`tsgo_repo` `tsgo_bundled` `tsgo_collections`。
**Go 源**：`internal/testrunner/`（3 个非测试文件 / 3 个 `*_test.go` / 4 个 `func Test*`）

## 这个包是什么（业务说明）

`testrunner` 是把"一个 `.ts` 测试用例 → 一组 baseline 文件"的**流水线**。它和 `testutil` 的分工是：

- `testutil/harnessutil` 提供**编译能力**（`CompileFiles`）和**配置展开**（`GetFileBasedTestConfigurations`）；
- `testutil/tsbaseline` 提供**baseline 渲染**（`Do*Baseline`）；
- `testrunner` 把它们**编排**起来：枚举文件 → 解析多文件结构（`makeUnitsFromTest`）→ 对每个配置变体跑 `CompileFiles` → 依次调 7 个 `verify*`（error / output / sourcemap / sourcemap record / types & symbols / module resolution）+ 2 个结构自检（union ordering / parent pointers）。

入口是两个顶层测试：`TestLocal`（跑 corsa 自带的 `testdata/tests/cases`，产 local baseline 并与 reference 比）和 `TestSubmodule`（跑 TS 原始 submodule 的 cases，产 diff baseline）。`CompilerBaselineRunner` 实现 `Runner` 接口（`EnumerateTestFiles` + `RunTests`）。

> 在 P10，`tsgo_testrunner` 是 conformance-parity 的**执行引擎**：conformance-parity 子目录讲"对拍策略 + 分批"，testrunner 讲"驱动器代码本身"。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3。本包特有：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `*testing.T` + `t.Run(name, ...)` + `t.Parallel()` | `&Harness` + 子 case 列表（见 testutil/impl.md 偏离说明） | 每个 fixture 是一个顶层 case，每个配置变体 + 每个 `verify*` 是子 case（`t.Run("error", ...)` 等）。Rust 用 harness 的嵌套 case 名复刻 |
| `Runner` interface（`EnumerateTestFiles` / `RunTests`） | `trait Runner { fn enumerate_test_files(&mut self) -> Vec<String>; fn run_tests(&mut self, h: &mut Harness); }` | 直译 |
| `CompilerTestType`（iota：Conformance / Regression） | `#[repr(i32)] enum CompilerTestType { Conformance, Regression }` + `as_str()` | `String()` 映射：Regression→"compiler"，Conformance→"conformance" |
| `rawCompilerSettings map[string]string` | `IndexMap<String, String>` | `@option` 抽取结果 |
| `testUnit{content, name}` / `testCaseContent{...}` | `struct TestUnit { content, name }` / `struct TestCaseContent { ... }` | 多文件切分结果（含 tsConfig + symlinks） |
| `regexp.MustCompile`（optionRegex / linkRegex / lineDelimiter） | `once_cell::Lazy<Regex>` | 见依赖 |
| `slices.Delete` / `core.Map` / `core.Some` | `Vec::remove` / `iter().map()` / `iter().any()` | |
| `ParseTestFilesAndSymlinks[T]`（泛型 parseFile 回调） | `fn parse_test_files_and_symlinks<T>(code, file_name, parse_file: impl Fn(...) -> Result<T>)` | **fourslash 复用此泛型**（fourslash 传自己的 `parseFileContent`）—— 跨 crate 复用点 |

> `ParseTestFilesAndSymlinksWithOptions` 是 testrunner 暴露给 fourslash 的关键泛型函数（fourslash `ParseTestData` 调它，传 `AllowImplicitFirstFile: true`）。Rust 侧须 `pub`，签名保持泛型 + options。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/testrunner/runner.go` | `internal/testrunner/lib.rs` | crate 根：`Runner` trait + `run_tests(runners)`；`pub mod compiler_runner; pub mod test_case_parser;` |
| `internal/testrunner/compiler_runner.go` | `internal/testrunner/compiler_runner.rs` | `CompilerBaselineRunner` + `compilerTest` + 7 个 `verify*` + skip 列表 + varyBy map |
| `internal/testrunner/test_case_parser.go` | `internal/testrunner/test_case_parser.rs` | `makeUnitsFromTest` / `ParseTestFilesAndSymlinks[WithOptions]` / `extractCompilerSettings` / `parseSymlinkFromTest` |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| 正则 | `regex` | optionRegex `^//\s*@(\w+)\s*:\s*(...)`、linkRegex、lineDelimiter `\r?\n`、compilerBaselineRegex `\.tsx?$`、referencesRegex |
| 确定性随机（union ordering 自检） | `rand`（`rand::rngs` + PCG） | Go 用 `math/rand/v2` 的 `NewPCG(1234, 5678)`；Rust 用 `rand_pcg` 同种子复刻 shuffle 自检 |

> 其余（IndexMap / once_cell）已在 testutil/crate-map 白名单内。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/testrunner/runner.go`）

- [ ] `pub trait Runner { fn enumerate_test_files(&mut self) -> Vec<String>; fn run_tests(&mut self, h: &mut Harness); }`　`// Go: runner.go:Runner`
- [ ] `fn run_tests(h: &mut Harness, runners: &mut [Box<dyn Runner>])`　`// Go: runner.go:runTests`

### `test_case_parser.rs`（Go: `internal/testrunner/test_case_parser.go`）

- [ ] `struct TestUnit { content: String, name: String }`　`// Go: test_case_parser.go:testUnit`
- [ ] `struct TestCaseContent { test_unit_data: Vec<TestUnit>, ts_config: Option<ParsedCommandLine>, ts_config_file_unit_data: Option<TestUnit>, symlinks: IndexMap<String,String> }`　`// Go: test_case_parser.go:testCaseContent`
- [ ] `fn make_units_from_test(code, file_name) -> TestCaseContent` — 切多文件 + 找 tsconfig + 解析 config　`// Go: test_case_parser.go:makeUnitsFromTest`
- [ ] `struct ParseTestFilesOptions { allow_implicit_first_file: bool }`　`// Go: test_case_parser.go:ParseTestFilesOptions`
- [ ] `pub fn parse_test_files_and_symlinks<T>(code, file_name, parse_file) -> (Vec<T>, symlinks, current_dir, global_options, err)`（默认 options）　`// Go: test_case_parser.go:ParseTestFilesAndSymlinks`
- [ ] `pub fn parse_test_files_and_symlinks_with_options<T>(code, file_name, parse_file, options)` — `// @Filename` / `// @symlink` / 全局 vs 文件 option / implicit first file（fourslash 路径）逐行状态机　`// Go: test_case_parser.go:ParseTestFilesAndSymlinksWithOptions`
- [ ] `fn extract_compiler_settings(content) -> IndexMap<String,String>`（`@option` 全抽，小写 key、去尾分号）　`// Go: test_case_parser.go:extractCompilerSettings`
- [ ] `fn parse_symlink_from_test(line, symlinks) -> bool`（`@link: A -> B`）　`// Go: test_case_parser.go:parseSymlinkFromTest`
- [ ] 常量正则：`optionRegex` / `linkRegex` / `lineDelimiter` / `fourslashDirectives = ["emitthisfile"]`

### `compiler_runner.rs`（Go: `internal/testrunner/compiler_runner.go`）

- [ ] `enum CompilerTestType { Conformance, Regression }` + `as_str()`（Regression→"compiler", Conformance→"conformance"）　`// Go: compiler_runner.go:CompilerTestType.String`
- [ ] `struct CompilerBaselineRunner { is_submodule, test_files, base_path, test_suit_name }` + `new_compiler_baseline_runner(test_type, is_submodule)`　`// Go: compiler_runner.go:NewCompilerBaselineRunner`
- [ ] `impl Runner for CompilerBaselineRunner`：`enumerate_test_files`（`EnumerateFiles(basePath, \.tsx?$, recursive)`）　`// Go: compiler_runner.go:EnumerateTestFiles`
- [ ] `run_tests`：`clean_up_local` → 枚举 → 跳过 `skippedTests` → `run_test`　`// Go: compiler_runner.go:RunTests`
- [ ] `static SKIPPED_TESTS: &[&str]`（API*.ts + 已移除 option 的 ~35 个文件，逐条照搬）　`// Go: compiler_runner.go:skippedTests`
- [ ] `fn clean_up_local(h)` — 删 `baselines/local/<suite>`　`// Go: compiler_runner.go:cleanUpLocal`
- [ ] `static COMPILER_VARY_BY: IndexSet<String>`（从 `OptionsDeclarations` 筛 affects* 的 bool/enum option + noEmit + isolatedModules，小写）　`// Go: compiler_runner.go:getCompilerVaryByMap`
- [ ] `fn run_test(h, filename)` — 读文件 → `getCompilerFileBasedTest` → 对每个 configuration `t.Run(testName, runSingleConfigTest)`　`// Go: compiler_runner.go:runTest`
- [ ] `struct CompilerFileBasedTest { filename, content, configurations }` + `get_compiler_file_based_test`　`// Go: compiler_runner.go:getCompilerFileBasedTest`
- [ ] `fn run_single_config_test(h, test_name, test, config)` — `makeUnitsFromTest` → `newCompilerTest` → `SkipUnsupportedCompilerOptions` → 依次跑 8 个 verify　`// Go: compiler_runner.go:runSingleConfigTest`
- [ ] `struct CompilerTest { options, harness_options, result, ts_config_files, to_be_compiled, other_files, has_non_dts_files, ... }` + `new_compiler_test`（分 toBeCompiled / otherFiles：含 tsconfig fileNames 判定 + require/reference 启发式）　`// Go: compiler_runner.go:newCompilerTest`
- [ ] `fn create_harness_test_file(unit, cwd) -> TestFile`　`// Go: compiler_runner.go:createHarnessTestFile`
- [ ] **8 个 verify**（每个对应一类 baseline 子 case）：
  - [ ] `verify_diagnostics` → `tsbaseline::do_error_baseline`（含 `DiffFixupOld` 去 `./` 前缀）　`// Go: compiler_runner.go:verifyDiagnostics`
  - [ ] `verify_javascript_output` → `do_js_emit_baseline`（仅 hasNonDtsFiles；含 `skippedEmitTests` map）　`// Go: compiler_runner.go:verifyJavaScriptOutput`
  - [ ] `verify_source_map_output` → `do_sourcemap_baseline`　`// Go: compiler_runner.go:verifySourceMapOutput`
  - [ ] `verify_source_map_record` → `do_sourcemap_record_baseline`　`// Go: compiler_runner.go:verifySourceMapRecord`
  - [ ] `verify_types_and_symbols` → `do_type_and_symbol_baseline`（仅非 NoTypesAndSymbols）　`// Go: compiler_runner.go:verifyTypesAndSymbols`
  - [ ] `verify_module_resolution` → `do_module_resolution_baseline`（仅 TraceResolution）　`// Go: compiler_runner.go:verifyModuleResolution`
  - [ ] `verify_union_ordering` — `CompareTypes` 对每个 union 反转 + 10 次随机 shuffle 后排序，断言稳定　`// Go: compiler_runner.go:verifyUnionOrdering`
  - [ ] `verify_parent_pointers` — 遍历每个非 lib 源文件，断言每个 node 的 `parent` 与遍历父一致　`// Go: compiler_runner.go:verifyParentPointers`
- [ ] `static SKIPPED_EMIT_TESTS: IndexMap<&str, &str>`（8 个并行 emit 非确定性文件，逐条照搬）　`// Go: compiler_runner.go:skippedEmitTests`

### Cargo / crate 接线

- [ ] `internal/testrunner/Cargo.toml`（`name = "tsgo_testrunner"` + path deps）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs`：`pub mod compiler_runner; pub mod test_case_parser;` + re-export `Runner` / `parse_test_files_and_symlinks*`（fourslash 复用）

## TDD 推进顺序（tracer bullet → 增量）

1. **`test_case_parser::make_units_from_test`**：这是唯一有直接单测的函数（`TestMakeUnitsFromTest`），先把它对齐（多文件切分 + 注释归属）。它不依赖编译，最易 red→green。
2. **`parse_test_files_and_symlinks_with_options`** 的 `allow_implicit_first_file` 分支（fourslash 路径）：补行为测试，因为 fourslash 阻塞依赖它。
3. **`compiler_runner` 的枚举 + 单 fixture 单配置**：选一个极简 conformance fixture（无 vary-by），跑通 `verify_diagnostics` 一类 baseline 与 reference 比对成功（tracer bullet 打穿 testutil + testrunner）。
4. 逐个开 `verify_*`（output → types/symbols → sourcemap），每开一个就用 conformance 里已有的对应 reference 文件验。
5. 最后接 `TestLocal` 全量驱动（这会跑 conformance-parity 的分批 checklist，见该子目录）。

## 与 Go 的已知偏离（divergence）

1. **`*testing.T` → `&Harness`**：同 testutil。`t.Run("error"/"output"/...)` 的子 case 名保留，用于 baseline 文件子目录/失败定位。
2. **`t.Parallel()`**：Go 每个 `runSingleConfigTest` 并行。Rust 用 `rayon` 并行跑 fixtures（PORTING §6），但 **baseline 写入/比较须线程安全 + 输出确定**（harness.failures 用 Mutex；emit 输出已在 testutil 重排）。
3. **`math/rand/v2` PCG**：`verify_union_ordering` 用固定种子 `(1234, 5678)` shuffle。Rust 用 `rand_pcg::Pcg64`（或等价）同种子；只要 shuffle 序列等价即可（此测试只验"排序稳定"，不依赖具体序列字节）。
4. **`go-cmp` 的 `cmp.AllowUnexported`**（`TestMakeUnitsFromTest` 用）：Rust 直接 `assert_eq!`（TestUnit/TestCaseContent derive PartialEq）。
5. **submodule 路径**（`TestSubmodule` / `../_submodules/TypeScript`）：依赖 TS submodule checkout，无 checkout 时 skip（同 Go `SkipIfNoTypeScriptSubmodule`）。

## 转交 / 推迟（DEFER）

- `TestSubmodule`（diff 模式，对 TS 原始 cases 跑）：依赖 submodule checkout + testutil 的三档 diff 完整落地；可在 `TestLocal` 全绿后再开。标 `// DEFER(phase-10): blocked-by: testutil submodule diff 三档 + TS submodule checkout`。
- 全量 `verify_module_resolution`：依赖 `TracerForBaselining` 的 package.json 缓存净化完整移植（testutil），仅 `traceResolution` 用例触发，可后置。
