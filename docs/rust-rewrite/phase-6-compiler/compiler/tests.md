# compiler: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 测试文件 / 5 `func Test*`（实为 1 个真 Test `TestProgram` + 4 个 Benchmark）/ `TestProgram` 含 3 表驱动子用例。

> **重要**：本包测试极薄——只有 `program_test.go:TestProgram`（3 子用例，断言**文件包含顺序**）是真单测，其余 4 个是性能 Benchmark（`BenchmarkNewProgram`/`BenchmarkEmitLongLines`/`BenchmarkEmitManyFiles`/`BenchmarkEmitLongLinesWithLineBreaks`）。编排层的正确性主要由 **P10 conformance/fourslash/testdata parity** 兜底。本轮补充少量行为级 Rust 单测覆盖编排骨架的关键路径（确定性、emit 基本功能、选项诊断）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/compiler/program_test.go` | `tests/program.rs`（`compiler_test`，需 bundled libs） | 1 Test + 1 Benchmark |
| `internal/compiler/emit_test.go` | `tests/emit.rs`（基准为主） | 3 Benchmark |

## `program_test.go`

### TestProgram（表 `programTestCases`，3 子用例，直接断言）

构造：`bundled.WrapFS` 内存 FS 写入 testFile，`NewProgram(Config{FileNames:[c:/dev/src/index.ts], CompilerOptions{Target}})`，断言 `program.GetSourceFiles()` 去掉 lib 前缀后的文件名列表 == `expectedFiles`（= `esnextLibs`(~95 lib) ++ 项目文件按特定顺序）。**核心是验证文件包含/排序的确定性**（深度优先、引用先于引用者、lib 在前）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `program_basic_file_ordering` | triple-slash `/// <reference>` 链的文件顺序 | index.ts 引 5.ts/10.ts，各自链式引用 → fileNames == esnextLibs ++ `[1.ts,2.ts,3.ts,4.ts,5.ts,6.ts,7.ts,8.ts,9.ts,10.ts,index.ts]`（深度优先顺序） | `program_test.go:TestProgram/BasicFileOrdering` | |
| `program_file_ordering_imports` | `import * as` 链的文件顺序 | 同结构但用 import → 与上同顺序 | `program_test.go:TestProgram/FileOrderingImports` | |
| `program_file_ordering_cycles` | 含环（3.ts/9.ts 反向 import index.ts）仍稳定 | 同结构 + 两条回边 → 与上同顺序（环不改变顺序） | `program_test.go:TestProgram/FileOrderingCycles` | |

> 这三个用例是本包**确定性铁律**的 gate：并行加载后，`getProcessedFiles`/`collectFiles` 的串行深度优先 + lib 稳定排序必须产出与线程调度无关的固定顺序。Rust 实现必须先在 `WorkGroup::Sequential` 下过这三个，再切并行重过一遍确认不变。
>
> 依赖：需 `bundled.Embedded`（嵌入 lib 文件）；无嵌入则 `Skip`。Rust 侧需 `tsgo_bundled`（P9）的 lib 嵌入；本 phase 若 bundled 未就绪，先用最小手造 lib 集替代 esnextLibs 断言（或标 `—` 待 P9/P10）。

## Benchmark（性能基准，非正确性 gate）

Go 的 4 个 Benchmark 不做正确性断言，仅测性能（emit long-line 的 O(n²) sourcemap 退化等）。Rust 侧用 `criterion` 或 `#[bench]`，**不计入 TDD 绿/红**，但其"功能等价"（能跑通 emit）作为行为级 smoke 列入补充测试。

| Go Benchmark | Rust 对应 | 说明 | 完成 |
|---|---|---|---|
| `BenchmarkNewProgram`（program_test.go） | `bench_new_program` | 反复 `NewProgram`；含 `compiler` 子基准（需 submodule） | — (perf, P10) |
| `BenchmarkEmitLongLines`（emit_test.go） | `bench_emit_long_lines` | emit 单文件超长行（1k/5k/10k props），测 sourcemap 退化 | — (perf) |
| `BenchmarkEmitManyFiles` | `bench_emit_many_files` | 200 文件并行 emit | — (perf) |
| `BenchmarkEmitLongLinesWithLineBreaks` | `bench_emit_long_lines_with_breaks` | 对照组（有换行） | — (perf) |

## 行为级补充测试（impl.md 有 TODO，覆盖编排骨架）

Go 无直接单测的编排路径，本轮补少量行为级用例（expected 取自 Go 实现的确定行为；多数 blocked-by checker/transformers，先列后随依赖补齐）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `program_includes_default_lib` | 默认引入 lib | 单文件 + Target → SourceFiles 含 `GetDefaultLibFileName` 对应 lib | fileloader.go:processAllProgramFiles（NoLib falsy 时加 lib root） | |
| `program_nolib_excludes_lib` | `noLib:true` 不引 lib | NoLib=true → SourceFiles 不含 lib | fileloader.go（NoLib 分支） | |
| `program_explicit_lib_list` | 显式 `lib:[es5]` | lib=[es5] → 仅引入对应 d.ts | fileloader.go | |
| `program_missing_file_recorded` | 引用不存在文件 | reference 指向缺失文件 → `IsMissingPath` 命中 + 诊断 | filesparser.go:getProcessedFiles（missingFiles） | |
| `program_ordering_deterministic_under_parallel` | 并行与顺序结果一致 | 同输入跑 `SingleThreaded` 与并行 → `GetSourceFiles()` 顺序相同 | filesparser.go（并行发现 + 串行收集） | |
| `program_dedup_casing_diagnostic` | 同路径不同大小写诊断 | 两 reference 仅大小写不同（caseSensitive）→ casing 诊断 | filesparser.go:addProcessingDiagnosticsForFileCasing | |
| `verify_outfile_removed_option` | `outFile` 触发 removed 诊断 | OutFile != "" → `Option_0_has_been_removed`（outFile） | program.go:verifyCompilerOptions | |
| `verify_target_es5_removed` | `target:ES5` removed | Target=ES5 → removed 诊断 | program.go:verifyCompilerOptions | |
| `verify_strict_prop_init_requires_null_checks` | strictPropertyInitialization 需 strictNullChecks | 前者 true 后者 false → `Option_0_cannot_be_specified_without_specifying_option_1` | program.go:verifyCompilerOptions | |
| `verify_paths_asterisk_rule` | paths 模式至多一个 `*` | key 含两个 `*` → `Pattern_0_can_have_at_most_one_Asterisk` | program.go:verifyCompilerOptions | |
| `emit_single_js_basic` | 单文件 emit JS | 简单 ts → 产出 .js（WriteFile 收到内容）；含 sourceMap=true 时产 .map | program.go:Emit + emitter.go | — (blocked-by transformers P5) |
| `emit_combine_results_order` | emit 结果按输入序合并 | 多文件 → EmittedFiles 顺序 == 输入顺序 | program.go:CombineEmitResults | — (blocked-by P5) |
| `sort_and_dedup_diagnostics` | 诊断排序去重 + related 合并 | 重复诊断（仅 relatedInfo 不同）→ 合并为一条，relatedInfo 排序去重 | program.go:SortAndDeduplicateDiagnostics/compactAndMergeRelatedInfos | |
| `checker_pool_count_clamp` | checker 数 clamp | files=2,checkers=4 → 池大小=2（min(4,files)）；singleThreaded → 1 | checkerpool.go:newCheckerPoolWithTracing | — (blocked-by checker P4) |
| `checker_pool_file_association` | 文件 `i%K` 分配 | K 个 checker、N 文件 → `fileAssociations[file_i]==checkers[i%K]` | checkerpool.go:createCheckers | — (blocked-by P4) |

## 与 impl.md 的对齐核对

- [x] 唯一真 Test `TestProgram` 的 3 个表驱动子用例都已逐行映射
- [x] 4 个 Benchmark 已列出（标 perf，非正确性 gate）
- [x] expected 取自 Go 测试字面量（文件顺序列表 / esnextLibs）
- [x] 每条带 `// Go:` 锚点（"Go 对照"/"依据"列）
- [x] 补充行为级测试均在 impl.md 有承载 TODO（process_all_program_files / verify_compiler_options / emit / checker pool / sort_and_dedup）
- [x] 确定性铁律（并行=顺序结果一致）有专门用例 `program_ordering_deterministic_under_parallel`

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `TestProgram` 的 esnextLibs 全量断言 | 需 `bundled` lib 嵌入 | P9（bundled）/ P10 |
| emit 字节级正确（JS/d.ts/sourcemap） | 需 transformers/printer/sourcemap（P5）+ checker（P4） | P5 / P10 parity |
| 语义/绑定/声明诊断正确性 | 需 checker（P4） | P4 / P10 |
| checker 池行为（数量/分组/non-exclusive） | 需 checker（P4）内部可变性设计 | P4 |
| 模块解析 / project reference / faking VFS | 需 module（P4）+ build 场景 | P4 / P10 |
| 性能基准（emit long-line O(n²) 等） | 性能回归，非正确性 | P10（perf 对拍） |
| 真实工程编译端到端 | conformance/fourslash/testdata | P10 parity |
