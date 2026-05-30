# diagnosticwriter: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例（**Go 侧无直接单测**）。

## 0 直接单测的情况

- Go 侧 `internal/diagnosticwriter/` **无 `*_test.go`**；该包行为由 **P10 conformance parity** 兜底（`tsc --pretty` / `--pretty false` 终端输出 baseline、watch 模式输出对拍）。
- 本轮补充**行为级 Rust 测试**：执行期建一个最小 `FakeDiagnostic`（实现 `Diagnostic`）+ `FakeFile`（实现 `FileLike`，提供固定 `Text()` 与 ECMA line map），用 `&mut String` 收集输出后断言整串。expected 取自 TS 已知格式 / 与 Go 实测输出对齐。
- 颜色测试可注入"无色 `FormattedWriter`"以稳定断言文本骨架，再单独用一条带色用例校验 ANSI 转义存在。

### 补充行为级测试（建议 `internal/diagnosticwriter/lib.rs` 内 `#[cfg(test)] mod tests`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `flatten_single_message` | 单条消息无 chain | diag("Cannot find name 'x'.") → `"Cannot find name 'x'."` | `WriteFlattenedDiagnosticMessage` | ✓ |
| `flatten_message_chain_indent` | chain 逐层缩进 2 空格 | root→child→grandchild → `"root\n  child\n    grandchild"` | `flattenDiagnosticMessageChain` level | ✓ |
| `format_compact_with_file` | 紧凑模式行列 1-based | file `a.ts`, pos→(line2,col3) → `"a.ts(2,3): error TS2304: Cannot find name 'x'.\n"` | `WriteFormatDiagnostic` | ✓ |
| `format_compact_no_file` | 无 file 时省略位置 | global diag → `"error TS2304: Cannot find name 'x'.\n"` | `WriteFormatDiagnostic` file==nil | ✓ |
| `write_location_basic` | `path:line+1:col+1` | (file,pos) → `"a.ts:2:3"`（无色 writer） | `WriteLocation` | ✓ |
| `code_snippet_single_line` | 单行片段 + 波浪线对齐 | 行 `let x = y;` 区间→ gutter `2` + `~` 落在 col8 | `writeCodeSnippet` firstLine==lastLine | ✓ |
| `code_snippet_zero_length` | length=0 squiggle 后一字符 | `lastLineChar++` 生效，恰 1 个 `~` | `writeCodeSnippet` length==0 | ✓ |
| `code_snippet_fold_over_5_lines` | 跨 ≥5 行折叠中间为 `...` | 6 行区间 → 仅首 2/末 2 行 + `...` gutter（l3/l4 不出现） | `hasMoreThanFiveLines` 分支 | ✓ |
| `code_snippet_tabs_to_spaces` | tab 转单空格 | 行含 `\t` → 输出 `" abc"` 且无 `\t` | `ReplaceAll("\t"," ")` | ✓ |
| `summary_zero_errors_empty` | 0 错误不输出 | `[]` → `""` | `WriteErrorSummaryText` total==0 | ✓ |
| `summary_single_error_in_file` | `Found 1 error in a.ts` | 1 file 1 err → `"\nFound 1 error in a.ts\x1b[90m:1\x1b[0m\n\n"` | `Found_1_error_in_0` | ✓ |
| `summary_single_global_error` | 无 file 的单错 | 1 global err → `"\nFound 1 error.\n\n"` | `Found_1_error` | ✓ |
| `summary_single_file_multi_errors` | 单文件多错（无表） | 1 file 3 errs → `"\nFound 3 errors in the same file, starting at: a.ts\x1b[90m:1\x1b[0m\n\n"` | `Found_0_errors_in_the_same_file_starting_at_Colon_1` | ✓ |
| `summary_multi_files_table` | `Found N errors in M files` + 表 | 2 files 3 errs → 计数 + 左对齐排序文件表 | `Found_0_errors_in_1_files` + `writeTabularErrorsDisplay` | ✓ |
| `summary_sorted_by_filename` | 文件按名字排序确定性 | files `b.ts`,`a.ts` → 表中 `a.ts` 在前 | `getErrorSummary` sort | ✓ |
| `category_format_colors` | 各 category 对应 ANSI | error→红/warning→黄/suggestion→灰/message→蓝 | `getCategoryFormat` | ✓ |
| `try_clear_screen_watch` | watch start code 清屏 | code 6031 且非 preserve → 返回 true + 写 `\x1B[2J\x1B[3J\x1B[H` | `TryClearScreen` | ✓ |
| `try_clear_screen_suppressed` | preserveWatchOutput 抑制 | `PreserveWatchOutput:true` → false，无输出 | `TryClearScreen` | ✓ |
| `related_information_rendered` | relatedInformation 带位置渲染 | diag+1 related → 主消息后出现缩进 related 块 | `FormatDiagnosticWithColorAndContext` related 分支 | ✓ |
| `to_diagnostics_widens_to_trait_objects` | 泛型切片 → trait 对象 | `Vec<FakeDiagnostic>` → `Vec<Box<dyn Diagnostic>>`，code/category 保持 | `ToDiagnostics` | ✓ |
| `status_with_color_and_time` | 着色状态行 | time+diag → `"[\x1b[90m12:00:00\x1b[0m] hi"` | `FormatDiagnosticsStatusWithColorAndTime` | ✓ |
| `status_and_time_plain` | 无色状态行 | time+diag → `"12:00:00 - hi"` | `FormatDiagnosticsStatusAndTime` | ✓ |

> doctest：`flatten_diagnostic_message` 带一个可运行 `# Examples`（计入 `cargo test` doctest，已绿）。

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/diagnosticwriter/diagnosticwriter.go:<Func>`，因 Go 侧无 `*_test.go`）；每个 `#[test]` 上方均有 `// Go:` 行
- [x] impl.md 中每个**已落地**的公开渲染函数都有上表至少一条用例承载；ast 适配层公开项整体 DEFER（见下）
- [x] 代码片段三分支（首行 / 末行 / 中间整行）均覆盖：`code_snippet_single_line`(首行==末行) + `code_snippet_fold_over_5_lines`（一次命中首行 / 中间默认行 / 末行 + 折叠省略）
- [x] 汇总三分支（单错 / 单文件多错 / 多文件）均覆盖：`summary_single_error_in_file`、`summary_single_file_multi_errors`、`summary_multi_files_table`
- [x] expected 取自 TS 已知格式 / Go 实测语义（非 Rust 推断）：行列 1-based、`error TSxxxx:` 形状、chain 2-空格缩进、本地化串字面量取自 `diagnostics` 生成表
- [x] 与 impl.md 双向对齐无遗漏（ast 适配项除外，已 DEFER）

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `AstDiagnostic` 适配 + `wrap/from/compare/write_flattened_ast` 的测试 | blocked-by `tsgo_ast::Diagnostic` 未移植（本轮整体 DEFER） | P5（ast 就绪后） |
| `tsc --pretty` 全量终端 baseline | 需真实编译输出对拍 | P10 |
| LSP 诊断（非 ASTDiagnostic）渲染 | `Diagnostic` 的 LSP 实现尚未移植 | P8 |
| 本地化消息（非 en）汇总串 | 依赖 locale 数据完整 | P10 |
