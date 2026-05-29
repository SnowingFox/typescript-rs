# format: 测试清单（tests.md）

> 已实际通读 `internal/format/*_test.go`（5 个测试文件），逐 `func Test*`、逐表驱动子用例对齐。

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：5 文件 / 6 `func Test*` + 1 `func Benchmark` / 约 18 子用例。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/format/api_test.go` | `internal/format/api.rs`（`#[cfg(test)] mod tests`） | 1 Test + 1 Benchmark |
| `internal/format/format_test.go` | `internal/format/format_test.rs`（或 `span.rs` tests） | 1 |
| `internal/format/comment_test.go` | `internal/format/comment_test.rs` | 2 |
| `internal/format/indent_test.go` | `internal/format/indent.rs` tests | 1 |
| `internal/format/indent_getindentation_test.go` | `internal/format/indent.rs` tests | 1 |

> 公共测试工具 `applyBulkEdits(text, edits) -> String`（`api_test.go:19`）须在 Rust 测试侧实现为 helper（按 `edit.Pos` 升序拼接），所有「格式化后断言文本」的用例都依赖它。

## `api_test.go`

> Go: `func TestFormat`（`t.Run("format checker.ts", ...)`，单子用例）。重型集成：解析整份 `src/compiler/checker.ts`（来自 TS submodule），`FormatDocument` 后断言 `len(newText) > 0` 且 `text != newText`。无 submodule 时 `repo.SkipIfNoTypeScriptSubmodule` 跳过。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `format_checker_ts` | 格式化大文件不崩、产出非空且有改动 | `checker.ts` 全文 → `out.len()>0 && out != text` | `api_test.go:TestFormat/format checker.ts` | — (需 TS submodule；无则 skip，等价 Go) |
| `bench_format`（`#[bench]`/criterion，非必须） | 性能基准（format / format-no-apply / pretty-print 三组） | — | `api_test.go:BenchmarkFormat` | — (基准，非 gate；可推迟) |

> 设置：`FormatCodeSettings{ TabSize:4, IndentSize:4, BaseIndentSize:4, NewLine:"\n", ConvertTabsToSpaces:true, IndentStyle:Smart, TrimTrailingWhitespace:true, InsertSpaceBeforeTypeAnnotation:true }`。

## `format_test.go`

> Go: `func TestFormatNoTrailingSpace`（表驱动，8 子用例）。对每段源码 `FormatDocument` 后，逐行断言 `line == TrimRight(line, " \t")`（格式化不得引入行尾空白）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `no_trailing_space::simple_statement` | 无尾换行单语句 | `"1;"` → 无行尾空白 | `format_test.go:TestFormatNoTrailingSpace/simple statement without trailing newline` | |
| `no_trailing_space::function_call` | 无尾换行调用 | `"console.log('hello');"` → 无行尾空白 | `.../function call without trailing newline` | |
| `no_trailing_space::if_block_single_line` | 单行 if 块 | `"if (true) { }"` → 无行尾空白 | `.../if block on single line` | |
| `no_trailing_space::class_decl` | 类声明（带注释体） | `"class A {\n    // Class Contents Go Here\n}"` → 无行尾空白 | `.../class declaration` | |
| `no_trailing_space::class_decl_trailing_nl` | 类声明带尾换行 | `"class A {\n    // ...\n}\n"` → 无行尾空白 | `.../class declaration with trailing newline` | |
| `no_trailing_space::empty_block` | 空块 | `"if (true) {}"` → 无行尾空白 | `.../empty block` | |
| `no_trailing_space::module_decl` | module 声明 | `"module M { }"` → 无行尾空白 | `.../module declaration` | |
| `no_trailing_space::enum_decl` | enum 声明 | `"enum E { A, B }"` → 无行尾空白 | `.../enum declaration` | |

> 设置同上但 `BaseIndentSize` 缺省（0）、无 `InsertSpaceBeforeTypeAnnotation`。

## `comment_test.go`

> Go: `func TestCommentFormatting`（7 子用例）+ `func TestSliceBoundsPanic`（1 子用例）。均为**回归测试**：断言注释/JSDoc 不被破坏、不 panic（含 issue #1928 负 Repeat、#2649 块内注释、slice 越界）。断言用 `strings.Contains`/`!strings.Contains` 子串检查（非全文相等）。

| Rust 测试 | 验证内容 | input → expected（断言） | Go 对照 | 完成 |
|---|---|---|---|---|
| `comment::issue_repro_stability` | 两次格式化 `class C { /**\n*\n*/ async x(){} }` 不破坏 `*/`、不把 async 改成 sync | 见源码 → `!contains("*/\n   /")`,`contains("*/")`,`contains("async")`；二次后 `!contains(" sync x()")`,`contains("async")` | `comment_test.go:TestCommentFormatting/format comment issue reproduction` | |
| `comment::jsdoc_tab_indentation` | tab 缩进 JSDoc：tab 在 space 前 | `"class Foo {\n\t/**\n\t * @param ...\n\t */\n\texample(...) {...}"`（tabs） → `!contains(" \t*")`,`contains("\t *")`,`contains("\t\tconsole.log")` | `.../format JSDoc with tab indentation` | |
| `comment::multiline_arg_list` | 多行参数列表内注释保留 tab 缩进 | `"console.log(\n\t\"a\",\n\t// the second arg\n\t\"b\"\n);"` → `contains("\t// the second arg")`,`!contains("\n// the second arg")` | `.../format comment inside multi-line argument list` | |
| `comment::chained_calls` | 链式调用间注释保留缩进 | `"foo\n\t.bar()\n\t// A second call\n\t.baz();"` → `contains("\t// A second call"\|\|"   // ...")`,`!contains("\n// A second call")` | `.../format comment in chained method calls` | |
| `comment::issue_1928_no_panic` | 链式调用注释不再触发负 Repeat panic | 同上文本 → 不 panic + 注释保留缩进 | `.../format chained method call with comment (issue #1928)` | |
| `comment::issue_2649_multiline_js` | block 首行开 + 多行注释（.js） | `document.addEventListener('DOMContentLoaded', () => {\n  /** @type ... */\n  const ...})` → `out.len()>0` | `.../multiline comment inside block that opens on first line (issue #2649)` | |
| `comment::issue_2649_singleline_ts` | block 首行开 + 单行注释（.ts） | `... () => {\n  // a comment\n  const x = 1\n})` → `out.len()>0` | `.../single-line comment inside block that opens on first line (issue #2649)` | |
| `slice_bounds_panic::trailing_semicolon` | 含「裸分号 + 注释」不 slice 越界 panic | `"const _enableDispose... = false\n\t// || ...\n\t;\n"` → 不 panic + `out.len()>0`,`contains("_enableDisposeWithListenerWarning")` | `comment_test.go:TestSliceBoundsPanic/format code with trailing semicolon should not panic` | |

## `indent_test.go`

> Go: `func TestGetContainingList_NamedImports`（无表驱动，单断言场景）。在 `import type {\n    AAA,\n    BBB,\n} from "./bar";` 上找两个 `ImportSpecifier`，对每个调 `format.GetContainingList`，断言返回非 nil 且 `len(list.Nodes)==2`。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `get_containing_list_named_imports` | 命名导入说明符的 containing list = 2 元素列表 | 两个 ImportSpecifier → 各 `get_containing_list(..)` 非 None 且 `nodes.len()==2` | `indent_test.go:TestGetContainingList_NamedImports` | |

> 辅助 `forEachDescendantOfKind`（递归收集某 Kind 子孙）须在 Rust 测试侧实现。

## `indent_getindentation_test.go`

> Go: `func TestGetIndentationForNamedImportsPosition`（单断言）。文本 `import {\n    type SomeInterface,\n} from "./exports.js";`，取 pos=14 处的行首（`GetLineStartPositionForPosition`），`GetIndentation(lineStart, file, default, true)` 断言 `== 4`。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `get_indentation_for_named_imports_position` | 命名导入内一行的缩进为 4 | `lineStart(pos=14)` → `get_indentation(.., default_settings, assume=true) == 4` | `indent_getindentation_test.go:TestGetIndentationForNamedImportsPosition` | |

> 设置：`GetDefaultFormatCodeSettings()`（IndentSize=TabSize=printer 默认，IndentStyle=Smart）。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（6）+ Benchmark 都已映射。
- [x] 每个表驱动子用例都已逐行列出（`TestFormatNoTrailingSpace` 8、`TestCommentFormatting` 7、`TestSliceBoundsPanic` 1、`TestFormat` 1）。
- [x] expected 值均取自 Go 测试字面量（子串断言/数值）。
- [x] 每条带 `// Go:` 锚点（在表「Go 对照」列）。
- [x] 双向对齐：`get_containing_list`/`get_indentation`/`get_line_start_position_for_position`/`format_document`/`format_*` 均在 impl.md 有承载 TODO。

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| ~250 条规则在真实代码上的逐条效果（空格/换行/分号 parity） | Go 侧无规则级单测，全靠端到端 baseline | P10（fourslash + `tests/cases/fourslash/*format*`、`tests/baselines`） |
| `BenchmarkFormat`（3 组性能对比） | 基准非正确性 gate | 性能阶段（可选，criterion） |
| `TestFormat`（checker.ts 全量稳定性） | 需 TS submodule；本质是大输入 smoke + parity 兜底 | 本包写 smoke 版；完整 parity 归 P10 |
