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
| `submodule_accepted_files_exist` | `submoduleAccepted.txt` 每个条目在 `reference/submoduleAccepted/<name>` 存在 | 读 `submoduleAccepted.txt`（84KB 列表）逐行 stat → 全部存在 | `baseline_test.go:TestSubmoduleAcceptedFilesExist` | |
| `submodule_triaged_files_exist` | `submoduleTriaged.txt` 每个条目在 `reference/submoduleTriaged/<name>` 存在 | 读 `submoduleTriaged.txt`（8KB 列表）逐行 stat → 全部存在 | `baseline_test.go:TestSubmoduleTriagedFilesExist` | |

> 注：这两个测试依赖 TS submodule checkout（Go 里 `RunAgainstSubmodule` 路径）。Rust 侧无 submodule 时 `—`（skip），有 submodule 时 `✓`。

## B. 移植补充的行为级 Rust 测试（基础设施正确性兜底）

> 依据 PORTING §8.5。expected 值取自 Go 实测 / `testdata/baselines/reference` 里的真实文件，不用 Rust 推断。这些是 baseline 引擎/harness 自身的回归防线——它们一旦错，几万个 conformance baseline 会整片误判。

### B.1 `baseline::write_comparison` / `run`（核心对拍逻辑）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `run_matches_reference_no_error` | actual == reference 内容 → 无失败，仅写 local | actual=`"foo\n"`, reference 文件=`"foo\n"` → harness.failures 空 | Go `writeComparison`（expected==actual 不报错） | |
| `run_mismatch_reports_error` | actual != reference → 报 "baseline has changed" | actual=`"foo\n"`, reference=`"bar\n"` → 1 个失败，含 "has changed" | Go `writeComparison` line ~241 | |
| `run_missing_reference_reports_new` | reference 不存在 → 报 "new baseline created" | actual=`"x"`, 无 reference → 失败含 "new baseline created at" | Go `writeComparison` line ~236 | |
| `run_empty_actual_panics` | actual=="" → panic（要求用 NoContent） | actual=`""` → panic "the generated content was" | Go `writeComparison` line ~197 | |
| `run_nocontent_writes_delete_marker` | actual==NoContent 且 reference 存在 → 写 `<local>.delete` | actual=NoContent, reference 存在 → local.delete 文件被建 | Go `writeComparison` line ~220 | |

### B.2 `baseline::diff_text` / `get_baseline_diff`（diff 字节一致性）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `diff_text_basic_unified` | 两段文本的 unified diff 头/体格式 | `expected="a\nb\nc"`, `actual="a\nB\nc"` → 含 `--- old.x` / `+++ new.x` / `-b` / `+B` | Go `DiffText`（`similar` 须对齐 `patience` 输出） | |
| `diff_identical_returns_nocontent` | 内容相同 → NoContent | expected==actual → `<no content>` | Go `getBaselineDiff` line ~151 | |
| `diff_header_line_numbers_skipped` | `@@ -N,M +K,L @@` 改写成 `@@= skipped -d, +d lines =@@` | 多 hunk diff → 头部全部按"相对上一 hunk 起点的增量"改写 | Go `getBaselineDiff` line ~166（fixUnifiedDiff 正则） | |

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

### B.7 `stringtestutil::dedent`

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `dedent_strips_common_indent` | 去公共缩进 | 缩进文本 → 去掉公共前导空白 | Go `Dedent`（多包单测复用，需先对） | |

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
| `projecttestutil` / `jstest` / `fixtures` / `autoimporttestutil` | 仅特定业务包用，对应包单测落地时再补 | 各业务包 / DEFER |
