# testutil: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 个 `*_test.go` 文件（`baseline/baseline_test.go`）/ 2 个 `func Test*` / 0 个表驱动子用例。

> `testutil` 是测试**基础设施**包，自身几乎无直接单测：它的正确性由"被 testrunner/fourslash 调用后跑出与 reference 一致的 baseline"间接证明（PORTING §8.5：0 直接单测的包行为由 P10 conformance/fourslash parity 兜底）。
> 因此本 tests.md 分两部分：(A) Go 仅有的 2 个 func 逐条对齐；(B) 移植时**必须补**的行为级 Rust 测试（覆盖 baseline 引擎 + harness 关键路径，expected 取自 Go 实测 / reference 文件）。

## A. 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/testutil/baseline/baseline_test.go` | `internal/testutil/baseline/mod.rs`（`#[cfg(test)] mod tests`） | 2 |

### `baseline_test.go`

> 这两个测试校验"`submoduleAccepted.txt` / `submoduleTriaged.txt` 里列的每个 diff 文件名，在 `baselines/reference/submoduleAccepted|Triaged/` 下都真实存在"。属于"清单与磁盘一致性"自检。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `submodule_accepted_files_exist` | `submoduleAccepted.txt` 每个条目在 `reference/submoduleAccepted/<name>` 存在 | 读 `submoduleAccepted.txt`（82KB 列表）逐行 stat → 全部存在 | `baseline_test.go:TestSubmoduleAcceptedFilesExist` | ✓ |
| `submodule_triaged_files_exist` | `submoduleTriaged.txt` 每个条目在 `reference/submoduleTriaged/<name>` 存在 | 读 `submoduleTriaged.txt`（4.5KB 列表）逐行 stat → 全部存在 | `baseline_test.go:TestSubmoduleTriagedFilesExist` | ✓ |

> 注：这两个测试用 `referenceRoot`（本仓 `testdata/baselines/reference`，列表 txt 与 reference 目录均存在）→ 全绿。它们**不**依赖 TS submodule checkout（该路径只影响 `RunAgainstSubmodule`/submodule diff）。实测：`reference/submoduleAccepted` 与 `reference/submoduleTriaged` 下所列文件均存在。
> Rust 测试位置：`internal/testutil/baseline/lib_test.rs`（`use super::*;`）。

## B. 移植补充的行为级 Rust 测试（基础设施正确性兜底）

> 依据 PORTING §8.5。expected 值取自 Go 实测 / `testdata/baselines/reference` 里的真实文件，不用 Rust 推断。这些是 baseline 引擎/harness 自身的回归防线——它们一旦错，几万个 conformance baseline 会整片误判。

### B.1 `baseline::write_comparison` / `run`（核心对拍逻辑）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `write_comparison_equal_content_no_failure` | actual == reference 内容 → 无失败，且 local 不写 | actual=`"foo\n"`, reference 文件=`"foo\n"` → harness.failures 空 | Go `writeComparison`（expected==actual 不报错） | ✓ |
| `write_comparison_mismatch_reports_changed` | actual != reference → 报 "has changed" + 写 local | actual=`"foo\n"`, reference=`"bar\n"` → 1 个失败，含 "has changed" | Go `writeComparison` line ~241 | ✓ |
| `write_comparison_missing_reference_reports_new` | reference 不存在 → 报 "new baseline created" | actual=`"x"`, 无 reference → 失败含 "new baseline created at" | Go `writeComparison` line ~236 | ✓ |
| `write_comparison_empty_actual_panics` | actual=="" → panic（要求用 NoContent） | actual=`""` → `#[should_panic]` "the generated content was" | Go `writeComparison` line ~197 | ✓ |
| `write_comparison_nocontent_writes_delete_marker` | actual==NoContent 且 reference 存在 → 写 `<local>.delete` | actual=NoContent, reference 存在 → local.delete 文件被建，local 不写 | Go `writeComparison` line ~220 | ✓ |
| `write_comparison_submodule_messages` | submodule 模式两档报错 | ref 缺失→"does not exist in the TypeScript submodule"；ref 不同→"does not match the reference..." | Go `writeComparison` line ~234/239 | ✓ |
| `run_creates_new_local_baseline_for_unknown_name` | `run`（非 submodule）写 local + 报 new baseline | 唯一子目录+新文件名 → 失败含 "new baseline created" + local 内容正确（用后清理） | Go `Run` | ✓ |
| `run_submodule_writes_categorized_diff` | `run`（submodule）三档 diff 写入 | `is_submodule=true` + 非空 actual → `local/submodule/<sub>/<f>.diff` 被建（用后清理） | Go `Run`（submodule 分支） | ✓ |
| `run_against_submodule_reports_missing_in_submodule` | `RunAgainstSubmodule` 报 submodule 缺失 | submodule 未 checkout → 失败含 "does not exist in the TypeScript submodule" + local 写入 | Go `RunAgainstSubmodule` | ✓ |

### B.2 `baseline::diff_text` / `get_baseline_diff`（diff 字节一致性）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `diff_text_basic_unified` | 两段文本的 unified diff 头/体格式 | `expected="a\nb\nc\n"`, `actual="a\nB\nc\n"` → 含 `--- old.x` / `+++ new.x` / `@@` / `-b` / `+B` | Go `DiffText`（`similar`；与 `patience` 字节对齐留 P10 conformance 端到端验证） | ✓ |
| `get_baseline_diff_identical_returns_nocontent` | 内容相同 → NoContent | expected==actual → `<no content>` | Go `getBaselineDiff` line ~151 | ✓ |
| `get_baseline_diff_fixups_applied` | fixup 闭包先归一化再比较 | `fixup_new` 把 actual 归一成 expected → `<no content>` | Go `getBaselineDiff` line ~145-150（fixupOld/New） | ✓ |
| `get_baseline_diff_header_line_numbers_skipped` | `@@ -N,M +K,L @@` 改写成 `@@= skipped -d, +d lines =@@` | 12 行文件两处分离改动→2 个 hunk → 头部全部按"相对上一 hunk 起点的增量"改写，无残留 `@@ -` | Go `getBaselineDiff` line ~166（fixUnifiedDiff 正则） | ✓ |

> **本轮额外补测（baseline 子包）**：`read_file_name_set_skips_blank_and_comments`（`readFileNameSet`：跳过空行/`#` 注释 + 去重，✓）；`testmain` 子模块 `record_action_*`（三分支，✓）/ `do_write_recorded_baselines_writes_one_per_line`（✓）/ `fnv64a_known_vectors`（FNV-1a 已知向量，✓）/ `track_returns_runnable_cleanup`（disabled→no-op，✓）/ `record_baseline_noop_when_disabled`（✓）。本轮 `tsgo_testutil_baseline` 共 **23 单测 + 6 doctest** 全绿。

> **B.3–B.6 仍 DEFER（`—`）**：以下属于 `harnessutil` / `tsbaseline` 子包，依赖尚未移植：
> - B.3 `compile_files` / B.4 `get_file_based_test_configurations` / B.5 `skip_unsupported_compiler_options` → `—` **blocked-by: P6 `tsgo_compiler` + P4 `tsgo_checker`/`tsgo_printer`/`tsgo_tsoptions`**（真编译 harness）。
> - B.6 `do_error_baseline` → `—` **blocked-by: `harnessutil` + checker/diagnosticwriter**。
> - B.7 `dedent`（`stringtestutil`）+ B.8 `race::enabled` → **本轮已落地 ✅**（独立零依赖叶子 crate，详见 B.7 / B.8）。

### B.3 `harnessutil::compile_files`（真编译路径）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `compile_single_file_no_errors` | 单文件无错编译 | `var x = 1;` → diagnostics 空，`js` 含 `var x = 1;` | Go `CompileFiles` 行为 | |
| `compile_emits_default_crlf_newline` | 默认 NewLine = CRLF | 无 `@newLine` → emit 用 `\r\n` | Go `CompileFiles` line ~96 | |
| `compile_multi_file_order_deterministic` | 多文件 emit 输出按 input 顺序 | 3 文件 → `js` IndexMap 顺序 == input 顺序 | Go `newCompilationResult`（确定性重排） | |
| `compile_reports_type_error` | 类型错被收集 | `const x: number = "s";` → diagnostics 含 TS2322 | Go `compileFilesWithHost`（GetSemanticDiagnostics） | |

### B.4 `harnessutil::get_file_based_test_configurations` / `split_option_values`（配置变体）

> Go 注释里直接给了 `splitOptionValues` 的 input→output 例子，逐条转测试：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `split_simple_list` | 逗号分隔去重 | `("esnext, es2015, es6", "target")` → `["esnext", "es2015"]`（es6==es2015 去重） | Go `splitOptionValues` doc 例 | |
| `split_star_expands_all` | `*` 展开全部布尔值 | `("*", "strict")` → `["true", "false"]` | Go `splitOptionValues` doc 例 | |
| `split_star_with_exclude` | `*, -true` 排除 | `("*, -true", "strict")` → `["false"]` | Go `splitOptionValues` doc 例 | |
| `config_variations_cartesian` | 多变体笛卡尔积 | `@target: es5,es6` + `@strict: *` → 4 个 NamedConfiguration | Go `computeFileBasedTestConfigurationVariations` | |
| `config_variations_cap_25` | 超过 25 变体 fatal | 变体数 > 25 → 报错 | Go `GetFileBasedTestConfigurations` line ~1001 | |

### B.5 `harnessutil::skip_unsupported_compiler_options` / `get_config_name_from_file_name`

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `skip_amd_module` | AMD/UMD/System module → skip | `module=amd` → Skipped | Go `SkipUnsupportedCompilerOptions` | |
| `skip_es5_target` | ES5 target → skip | `target=es5` → Skipped | Go `SkipUnsupportedCompilerOptions` | |
| `config_name_tsconfig` | `tsconfig.json` 识别 | `"a/tsconfig.json"` → `"tsconfig.json"` | Go `GetConfigNameFromFileName` | |
| `config_name_non_config` | 普通文件 → 空 | `"a.ts"` → `""` | Go `GetConfigNameFromFileName` | |

### B.6 `tsbaseline::do_error_baseline`（诊断 baseline 文本）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `error_baseline_format` | 错误 baseline 文本结构 | 1 个 TS2322 → `.errors.txt` 含 `==== file.ts (1 errors) ====` + 源码行 + `!!! error TS2322` | 取自 `reference/compiler/*.errors.txt` 真实样本 | |
| `error_baseline_no_errors_nocontent` | 无错 → NoContent | diagnostics 空 → `<no content>` | Go `DoErrorBaseline` line ~41 | |

### B.7 `stringtestutil::dedent`（已落地 ✅，crate `tsgo_testutil_stringtestutil`）

> Go 包无 `_test.go`，按 PORTING §8.6 写行为级单测；每个 expected 取自**真实 Go `Dedent` 实测输出**（用 in-repo 临时 main 调真包采集，已删除）。`// Go: internal/testutil/stringtestutil/stringtestutil.go:Dedent`。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `single_line_without_indentation_is_unchanged` | slice1 identity | `"hello"` → `"hello"` | Go `Dedent` 实测 | ✓ |
| `strips_leading_and_trailing_blank_lines` | slice2 去首尾空行 | `"\nhello\n"` → `"hello"` | Go `Dedent` 实测 | ✓ |
| `removes_common_leading_indentation` | slice3 去公共缩进 | `"\n    function f() {\n        return 1;\n    }\n"` → `"function f() {\n    return 1;\n}"` | Go `Dedent` 实测 | ✓ |
| `expands_leading_tabs_to_four_spaces` | tab→4空格 | `"\n\tfoo\n\t\tbar\n"` → `"foo\n    bar"` | Go `Dedent` 实测 | ✓ |
| `preserves_interior_blank_lines` | 内部空行保留 | `"\n  a\n\n  b\n"` → `"a\n\nb"` | Go `Dedent` 实测 | ✓ |
| `multi_line_without_indentation_is_unchanged` | 多行零缩进 | `"a\nb\nc"` → `"a\nb\nc"` | Go `Dedent` 实测 | ✓ |
| `handles_mixed_tab_and_space_indentation` | tab+space 混合 | `"\n\t  x\n\t\t  y\n"` → `"x\n    y"` | Go `Dedent` 实测 | ✓ |

### B.8 `race::enabled`（已落地 ✅，crate `tsgo_testutil_race`）

> Go 包用 build-tag 选 `Enabled`（无 `_test.go`）。Rust 偏离：`#[cfg(feature="race")]` 双常量；测试断言 cfg 选定值。`// Go: internal/testutil/race/race.go:Enabled (+ norace.go)`。

| Rust 测试 | 验证内容 | 期望 | 依据 | 完成 |
|---|---|---|---|---|
| `enabled_matches_cfg_selected_value` | cfg 选定值 | 默认（无 feature）`enabled()==false`（对齐 `norace.go`）；`--features race` 时 `==true`（对齐 `race.go`） | Go build-tag 二值语义 | ✓ |
| `const_and_accessor_agree` | const/accessor 一致 | `enabled() == ENABLED` | 不漂移自检 | ✓ |

## 与 impl.md 的对齐核对

- [ ] Go 的 2 个 `func Test*`（submodule accepted/triaged 存在性）已映射到 A 节
- [ ] 表驱动子用例：Go 本包无表驱动；`splitOptionValues` 的 doc 例子已逐条转 B.4
- [ ] B 节每条行为级测试都有 impl.md 的实现 TODO 承载（baseline.run / diff_text / compile_files / get_file_based_test_configurations / do_error_baseline / dedent）
- [ ] expected 值取自 Go doc 字面量 / `reference/**` 真实 baseline 样本（非 Rust 推断）
- [ ] 每条带 `// Go:` 锚点
- [ ] impl.md 每个有对应行为的公开函数（run / diff_text / compile_files / get_file_based_test_configurations / skip_unsupported / do_error_baseline）在此有测试行

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `do_type_and_symbol_baseline` 的 walker 细节 | `.types`/`.symbols` 正确性由 conformance 端到端逐字节兜底，单测意义有限 | conformance-parity |
| `do_sourcemap_*` baseline | 同上，由 sourcemap conformance baseline 兜底 | conformance-parity |
| `lsptestutil` LSPClient 往返 | 由 fourslash 框架冒烟测试覆盖（fourslash 跑通即证明 client 对） | fourslash |
| `projecttestutil` / `autoimporttestutil` | 仅特定业务包用，对应包单测落地时再补 | 各业务包 / DEFER |

## 已落地的外围叶子 testutil（本轮 PERIPHERAL lane，无 Go `_test.go`，行为级 red→green）

| crate | 测试（单测 / doctest） | 备注 |
|---|---|---|
| `tsgo_testutil_fsbaselineutil` | 11 / 5 | `sanitize_internal_symbol_name`（3）+ `baseline_fs_with_diff` 全分类（new/symlink/modified/deleted/rewrite/mtime/lib，8）；diff 经 in-crate `MapFsView` fake 端到端验证。expected 取 Go `differ.go` 字面量（`*new* \n`/`-> .. *new*`/`*deleted*`/`*modified* \n`/`*rewrite with same content*`/`*mTime changed*`/`*Lib*\n`） |
| `tsgo_testutil_jstest` | 7 / 5 | `node_exe`/`should_skip_no_nodejs`/`LOADER_SCRIPT`/`build_ts_loader_script`/`typescript_module_url` + `eval_node_script`（求和 / 反序列化对象 / run-error，node 真跑、缺 node 自跳过） |
| `tsgo_testutil_fixtures` | 3 / 1 | `bench_fixtures` 名单顺序 + string-backed `empty.ts` + file-backed 路径锚定 submodule（expected 取 `benchfixtures.go` 字面量） |
