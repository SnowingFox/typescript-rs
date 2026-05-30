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
| `program_includes_default_lib` | 默认引入 lib | 单文件 + Target → SourceFiles 含 `GetDefaultLibFileName` 对应 lib | fileloader.go:processAllProgramFiles（NoLib falsy 时加 lib root） | — (P6-2, blocked-by: lib 加载 / bundled) |
| `program_nolib_excludes_lib` | `noLib:true` 不引 lib | NoLib=true → SourceFiles 不含 lib | fileloader.go（NoLib 分支） | — (P6-2) |
| `program_explicit_lib_list` | 显式 `lib:[es5]` | lib=[es5] → 仅引入对应 d.ts | fileloader.go | — (P6-2, blocked-by: lib 加载) |
| `program_missing_file_recorded` | 缺失文件 | 缺失 root → `missing_files` 命中 | filesparser.go:getProcessedFiles（missingFiles） | ✓ (P6-1: `records_missing_root_file`；reference/诊断版留 P6-2) |
| `program_ordering_deterministic_under_parallel` | 并行与顺序结果一致 | 同输入跑 `SingleThreaded` 与并行 → `GetSourceFiles()` 顺序相同 | filesparser.go（并行发现 + 串行收集） | — (P6-2, blocked-by: 并行 worklist；P6-1 仅顺序版 + collect 确定性已绿) |
| `program_dedup_casing_diagnostic` | 同路径不同大小写诊断 | 两 reference 仅大小写不同（caseSensitive）→ casing 诊断 | filesparser.go:addProcessingDiagnosticsForFileCasing | — (P6-2, blocked-by: includeProcessor 诊断) |
| `verify_outfile_removed_option` | `outFile` 触发 removed 诊断 | OutFile != "" → `Option_0_has_been_removed`（outFile） | program.go:verifyCompilerOptions | — (P6-2) |
| `verify_target_es5_removed` | `target:ES5` removed | Target=ES5 → removed 诊断 | program.go:verifyCompilerOptions | — (P6-2) |
| `verify_strict_prop_init_requires_null_checks` | strictPropertyInitialization 需 strictNullChecks | 前者 true 后者 false → `Option_0_cannot_be_specified_without_specifying_option_1` | program.go:verifyCompilerOptions | — (P6-2) |
| `verify_paths_asterisk_rule` | paths 模式至多一个 `*` | key 含两个 `*` → `Pattern_0_can_have_at_most_one_Asterisk` | program.go:verifyCompilerOptions | — (P6-2) |
| `emit_single_js_basic` | 单文件 emit JS | `const x: number = 1;` → 写 `/src/index.js` = `const x = 1;\n`（WriteFile 收到内容）；sourceMap DEFER | program.go:Emit + emitter.go | ✓ (P6-3) |
| `emit_combine_results_order` | emit 结果按输入序合并 | 多文件 → `emitted_files` 顺序 == 输入顺序（`emit_combines_multiple_files_in_input_order`） | program.go:CombineEmitResults | ✓ (P6-3) |
| `sort_and_dedup_diagnostics` | 诊断排序去重 + related 合并 | 重复诊断（仅 relatedInfo 不同）→ 合并为一条，relatedInfo 排序去重 | program.go:SortAndDeduplicateDiagnostics/compactAndMergeRelatedInfos | — (P6-2) |
| `checker_pool_count_clamp` | checker 数 clamp | files=2,checkers=4 → 池大小=2（min(4,files)）；singleThreaded → 1 | checkerpool.go:newCheckerPoolWithTracing | ✓ (P6-1: `checkerpool_test.rs` 全量子用例) |
| `checker_pool_file_association` | 文件 `i%K` 分配 | K 个 checker、N 文件 → `fileAssociations[file_i]==checkers[i%K]` | checkerpool.go:createCheckers | ✓ (P6-2: `create_checkers_associates_files_round_robin`) |

## P6-1 已落地单测（21 个 `#[test]` + 3 个 doctest，全绿）

> 本轮地基范围的行为级单测，红→绿逐个写出（见 impl.md 的 worklog）。完成列 `✓`=已绿。

### `host_test.rs`（`host.rs`）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `host_returns_cwd_and_file_contents` | host over 内存 vfs 返回 cwd + 文件内容（tracer bullet） | `host.go:compilerHost.GetCurrentDirectory/FS` | ✓ |
| `host_parses_source_file` | `get_source_file` 读取+解析，暴露 file_name/text | `host.go:compilerHost.GetSourceFile` | ✓ |
| `host_missing_source_file_is_none` | 读不到的文件 → `None` | `host.go:GetSourceFile`（ReadFile miss） | ✓ |
| doctest `ParsedFile::import_specifiers` | import 说明符按源序提取 | `ast.go:SourceFile.Imports` | ✓ |

### `fileloader_test.rs`（`fileloader.rs`）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `loads_single_root_file` | 单 root 无依赖 → 仅该文件 | `fileloader.go:processAllProgramFiles` | ✓ |
| `records_missing_root_file` | 读不到的 root → `missing_files` | `filesparser.go:getProcessedFiles`（missingFiles） | ✓ |
| `loads_multiple_roots_in_order` | 多 root 按列出顺序 | `fileloader.go:processAllProgramFiles` | ✓ |
| `loads_resolved_relative_import` | `./a` 经 `module::Resolver` 解析 → a 在 index 之前（import 先于 referrer） | `fileloader.go:resolveImportsAndModuleAugmentations` | ✓ |
| `loads_import_cycle_once` | a↔index 环各加载一次、顺序确定 | `fileloader.go`（环 / seen） | ✓ |

### `filesparser_test.rs`（`filesparser.rs`，确定性铁律）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `collect_orders_imports_before_referrer` | 后序：依赖先于引用者 | `filesparser.go:collectFiles` | ✓ |
| `collect_dedups_diamond` | 菱形依赖共享文件只收一次 | `filesparser.go:collectFiles`（seen） | ✓ |
| `collect_handles_cycle` | 环终止、各一次 | `filesparser.go:collectFiles`（seen 守卫） | ✓ |

### `checkerpool_test.rs`（`checkerpool.rs`，表驱动 clamp）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `defaults_to_four_checkers` | (false,None,10)→4 | `checkerpool.go`（默认 4） | ✓ |
| `clamps_to_file_count` | (false,Some(8),2)→2;(false,None,3)→3 | `min(checkerCount,len(files))` | ✓ |
| `single_threaded_uses_one` | (true,Some(8),10)→1 | singleThreaded ⇒ 1 | ✓ |
| `clamps_to_floor_and_ceiling` | (false,None,0)→1;(false,Some(1000),1000)→256 | `max(min(...,256),1)` | ✓ |
| `honors_configured_count` | (false,Some(2),10)→2 | `options.Checkers` | ✓ |
| `pool_reports_checker_count` | `CompilerCheckerPool::new(false,Some(3),10).checker_count()`==3 | `checkerPool`（len(checkers)） | ✓ |
| doctest `checker_count` | 4 个等式 | `checkerpool.go:newCheckerPoolWithTracing` | ✓ |

### `program_test.rs`（`program.rs`）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `builds_program_from_single_file` | `new_program` 单文件 → source_files + options 往返（tracer） | `program.go:NewProgram`/`GetSourceFiles`/`Options` | ✓ |
| `looks_up_source_file_by_name` | `get_source_file`/`get_source_file_by_path` 命中/未命中 | `program.go:GetSourceFile/GetSourceFileByPath` | ✓ |
| `builds_multi_file_program_and_sizes_pool` | import 链 → [a, index] + 池大小=min(4,2)=2 | `program.go:NewProgram`+`initCheckerPool` | ✓ |
| `single_threaded_program_uses_one_checker` | single-threaded → 1 checker；host/command_line 访问器 | `program.go:SingleThreaded/Host/CommandLine` | ✓ |
| doctest `new_program` | 单文件 program 构建 + source_files | `program.go:NewProgram` | ✓ |

## P6-2 已落地单测（+15 `#[test]` / +3 doctest，全绿；累计 37 单测 + 6 doctest）

### bind（`host_test.rs` / `program_test.rs`）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `binding_a_file_yields_its_symbol_table` | `ParsedFile::bind` 经 `tsgo_binder` 产出 file-scope 符号表（x/f 是 local） | `program.go:BindSourceFiles`（逐文件） | ✓ |
| `bind_source_files_binds_every_file` | `Program::bind_source_files` 绑定全部文件 | `program.go:BindSourceFiles` | ✓ |

### checker 池（`boundfile_test.rs` / `program_test.rs`）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `bound_file_exposes_arena_root_and_symbols` | `BoundFile` impl `BoundProgram`：arena/root/locals/symbol | `program.go:Program`（bound 查询面） | ✓ |
| `unbound_file_has_no_bound_view` | 未 bind 文件 → 无 `BoundFile` | `program.go:Program`（bind 先于检查） | ✓ |
| `create_checkers_associates_files_round_robin` | `create_checkers`：3 文件 + `--checkers 2` → 2 个真实 checker、`i%K` 关联、`files_for_checker` 分组形状 | `checkerpool.go:createCheckers/forEachCheckerGroupDo` | ✓ |
| doctest `files_for_checker` | 分组形状示例 | `checkerpool.go:forEachCheckerGroupDo` | ✓ |

### verify_compiler_options（`verify_options_test.rs`，逐规则 red→green）

| Rust 测试 | input → expected（message + args） | Go 对照 | 完成 |
|---|---|---|---|
| `default_options_are_clean` | 默认 → 无诊断 | program.go:verifyCompilerOptions | ✓ |
| `out_file_is_removed` | outFile 非空 → `Option_0_has_been_removed` ["outFile"] | verifyCompilerOptions | ✓ |
| `target_es5_is_removed` | target=ES5 → `Option_0_1_has_been_removed` ["target","ES5"] | verifyCompilerOptions | ✓ |
| `removed_module_kinds` | module=AMD/System/UMD → `Option_0_1_has_been_removed` ["module",名] | verifyCompilerOptions | ✓ |
| `strict_property_initialization_requires_strict_null_checks` | spi=true & snc=false → `..._without_specifying_option_1`；`strict:true` 隐含则无 | verifyCompilerOptions | ✓ |
| `lib_cannot_be_used_with_no_lib` | lib 非空 & noLib → `..._with_option_1` ["lib","noLib"] | verifyCompilerOptions | ✓ |
| `check_js_requires_allow_js` | checkJs 单独隐含 allowJs（无诊断）；checkJs & allowJs=false → `..._without_specifying_option_1` | verifyCompilerOptions | ✓ |
| `emit_decorator_metadata_requires_experimental_decorators` | edm=true & ed 假/未知 → `..._without_specifying_option_1` | verifyCompilerOptions | ✓ |
| `program_reports_option_diagnostics` | `new_program` 暴露 `options_diagnostics()`（outFile 触发；干净程序为空） | program.go:NewProgram | ✓ |
| doctest `verify_compiler_options` | 默认 → 空 | verifyCompilerOptions | ✓ |

### import 解析 mode（`fileloader_test.rs`）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `import_syntax_affects_module_resolution_predicate` | 默认/node16 → true；Bundler+exports/imports 关 → false | fileloader.go:importSyntaxAffectsModuleResolution | ✓ |
| doctest `import_syntax_affects_module_resolution` | 默认 → true | fileloader.go | ✓ |

### P6-2 DEFER（blocked-by）

| 行为 | blocked-by | 目标 |
|---|---|---|
| checker 真正检查 / per-file 诊断 / 分组**并行**收集 | **checker public API**：`new_checker` 忽略 `BoundProgram` 实参（桩）、无 per-file check/diagnostics 入口 | checker API 放开后（建议单开一轮） |
| 精确 import mode（`get_mode_for_usage_location`）+ `SourceFileMetaData`/impliedNodeFormat | **ast**：`tsgo_ast` 未移植 `SourceFileMetaData`/`GetImpliedNodeFormatForEmitWorker`（不可编辑 `internal/ast/**`） | ast 移植该面后 |
| 带源位置的 option 诊断 + program 状态规则（outDir/rootDir、paths `*`、project refs） | tsconfig option-syntax AST + Program common-source-directory/emit | P6-3+ |
| lib 文件加载 + include/casing 诊断流水线（includeProcessor） | `tsgo_bundled` lib 集合（dev-dep，非主依赖）+ includeProcessor 子系统 | P6-3+ |

## P6-3 已落地单测（+8 `#[test]` / +2 doctest，全绿；累计 45 单测 + 8 doctest）

> emitter 可达子集（transform+print）的行为级单测，逐个 red→green（见 impl.md 的 P6-3 worklog）。测试位置：`emitter_test.rs`（挂在 `emitter.rs`），构造完整 `Program` 走 `Program::emit` 公开入口。

### emit（`emitter_test.rs`）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `emit_single_js_basic` | **tracer**：单文件 transform+print 端到端 | `const x: number = 1;` → 写 `/src/index.js` = `const x = 1;\n`；`emitted_files==[/src/index.js]` | emitter.go:emitJSFile + program.go:Emit | ✓ |
| `emit_skipped_when_no_emit` | `noEmit` skip | `no_emit:true` → `emit_skipped`、不写文件 | emitter.go:emitJSFile（`NoEmit==TSTrue`） | ✓ |
| `emit_prepends_bom_when_emit_bom` | `emitBOM` 前缀 | `emit_bom:true` → 输出 `\uFEFFconst x = 1;\n` | emitter.go:printSourceFile（`EmitBOM`） | ✓ |
| `emit_combines_multiple_files_in_input_order` | 多文件按输入序合并（确定性铁律） | 两 root `[a.ts,index.ts]` → `emitted_files==[a.js,index.js]`，内容各正确 | program.go:Emit/CombineEmitResults | ✓ |
| `emit_skips_declaration_files` | 跳过 `.d.ts` | root `[a.d.ts,index.ts]` → 仅 `[index.js]` | emitter.go:sourceFileMayBeEmitted | ✓ |
| `emit_target_source_file_emits_only_that_file` | 单文件 target | `target_source_file=/src/index.ts` → 仅 `[index.js]` | program.go:EmitOptions.TargetSourceFile | ✓ |
| `emit_writes_through_host_fs_by_default` | host fs 回退 | 无 writeFile 回调 → 经 host fs 写，回读 == `const x = 1;\n` | emitter.go:writeText（`host.WriteFile`） | ✓ |
| `emit_honors_crlf_newline_option` | newline(CRLF) | `new_line:Crlf` → 输出以 `\r\n` 结尾 | emitter.go:emitJSFile（`PrinterOptions.NewLine`） | ✓ |
| doctest `Program::emit` | 单文件 emit 往返（host fs 回读） | `const x: number = 1;` → `/src/index.js` = `const x = 1;\n` | program.go:Emit | ✓ |
| doctest `combine_emit_results` | emit_skipped OR + 顺序拼接 | 两结果 → skipped=true、files 顺序拼接 | program.go:CombineEmitResults | ✓ |

### P6-3 DEFER（blocked-by）

| 行为 | blocked-by | 目标 |
|---|---|---|
| sourcemap（`shouldEmitSourceMaps`/`printSourceFile` 的 map 半边/`getSourceMappingURL`/写 `.map`） | **printer**：`Printer::emit_source_file` 不驱动 `sourcemap::Generator`，`PrinterOptions` 无 `SourceMap` 字段（Go `printer.Write` 接受 generator） | printer 移植 source-map emission 后 |
| declaration（`.d.ts`）emit（`emit_declaration_file`/`getDeclarationTransformers`/`getDeclarationDiagnostics`） | **declarations transformer + checker `EmitResolver`** | checker public API + declarations 移植后 |
| 完整 `getScriptTransformers` 链（importElision/runtimeSyntax/legacyDecorators/metadata/jsx/module/es downlevel/constEnum inlining） | **checker `EmitResolver`** + 未移植 transformer 工厂（Rust `tstransforms` 仅 `typeeraser` 可达） | 各 transformer + checker resolver 移植后 |
| **并行** emit（writer 池）+ `HandleNoEmitOnError` 前置 + 分组并行诊断收集 | **checker 语义诊断 API**（顺序版已绿，确定性由按输入序合并保证） | P6-4（并行化）/ checker API |
| `noEmitForJsFiles`/external-library/project-reference/json-without-`outDir` 的 `sourceFileMayBeEmitted` 分支 | checker/program 状态（external library 判定、project references） | P6-4+ |

## P6-4 已落地单测（+4 `#[test]` / +1 doctest，全绿；累计 49 单测 + 9 doctest）

> 适配 checker 4l 的 program-retaining API（`new_checker(Rc<dyn BoundProgram>)`）+ checker 池**真正驱动**per-file 诊断。逐个 red→green（见 impl.md 的 P6-4 worklog）。

### owned/`Rc` BoundProgram（`boundfile_test.rs`）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `bound_file_is_shareable_as_rc_program` | `BoundFile` owned/`'static`/`Rc`-shareable（本轮核心适配；旧 `BoundFile<'a>` 不可进 `Rc<.. + 'static>`） | bound `var x;` → `Rc<dyn BoundProgram>` + `Rc::clone` ⇒ `strong_count==2`、两 handle 同 root | checker.go:NewChecker（一个 `*Program` 被池共享） | ✓ |

### checker 池驱动诊断（`checkerpool_test.rs`）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `collects_undefined_identifier_diagnostic` | **tracer**：池经 `Rc::clone` 建 K checker + `collect_diagnostics` 驱动 `get_diagnostics(root)` | `y;` → 1 条 `{code:2304, "Cannot find name 'y'."}` | checkerpool.go:createCheckers + program.go:getDiagnostics | ✓ |
| `collects_property_does_not_exist_diagnostic` | 驱动的是 checker 全部可达语义（非特例；coverage / green-on-add） | `interface Foo { bar: string } declare const foo: Foo; foo.baz;` → 1 条 `{code:2339, "Property 'baz' does not exist on type 'Foo'."}` | program.go:getSemanticDiagnostics | ✓ |

### 端到端 Program（`program_test.rs`）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `program_collects_semantic_diagnostics` | `Program::semantic_diagnostics` 串 bind→建池→收诊断 | 单文件 `y;` → 1 条 2304 "Cannot find name 'y'." | program.go:GetSemanticDiagnostics | ✓ |
| doctest `Program::semantic_diagnostics` | 端到端（`new_program` + MapFs） | `y;` → `diags[0].code==2304` | program.go:GetSemanticDiagnostics | ✓ |

### P6-4 DEFER（blocked-by）

| 行为 | blocked-by | 目标 |
|---|---|---|
| 多文件 per-file 诊断收集 + `GetDiagnosticsForFile(name)` 过滤 | **多文件 `BoundProgram` view（P6 program）** —— 现 `BoundProgram` 单文件（一 program=一 bound 文件），pool 真正驱动的是 seed（文件 0） | 多文件 program view 落地后 |
| 跨 checker **并行**诊断收集（`forEachCheckerParallel`/分组并行） | **parallel `Arc` checker（PORTING §6）** —— checker 现持 `Rc<dyn BoundProgram>`（非 `Arc + Send + Sync`），只能顺序驱动 | checker 切 `Arc` 后 |
| suggestion / declaration 诊断、`@ts-expect-error`·`@ts-ignore` 指令处理、`HandleNoEmitOnError` 前置 | checker 指令面 + declarations transformer + emit resolver | 后续 checker 轮 |
| lib globals（影响哪些诊断可达，如需要全局类型的检查） | `tsgo_bundled` lib 集合（dev-dep）+ checker lib-global 解析 | bundled + checker lib globals |

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
