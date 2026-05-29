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
| `flatten_single_message` | 单条消息无 chain | diag("Cannot find name 'x'.") → `"Cannot find name 'x'."` | `WriteFlattenedDiagnosticMessage` | |
| `flatten_message_chain_indent` | chain 逐层缩进 2 空格 | root→child→grandchild → `"root\n  child\n    grandchild"` | `flattenDiagnosticMessageChain` level | |
| `format_compact_with_file` | 紧凑模式行列 1-based | file `/a.ts`, pos→(line2,col3) → `"a.ts(2,3): error TS2304: ..."` | `WriteFormatDiagnostic` | |
| `format_compact_no_file` | 无 file 时省略位置 | global diag → `"error TS2304: ...\n"` | `WriteFormatDiagnostic` file==nil | |
| `write_location_basic` | `path:line+1:col+1` | (file,pos) → `"a.ts:2:3"`（无色 writer） | `WriteLocation` | |
| `code_snippet_single_line` | 单行片段 + 波浪线对齐 | 行 `let x = y;` 区间→ gutter `2 ` + `~~~` 落在区间 | `writeCodeSnippet` firstLine==lastLine | |
| `code_snippet_zero_length` | length=0 squiggle 后一字符 | `lastLineChar++` 生效，1 个 `~` | `writeCodeSnippet` length==0 | |
| `code_snippet_fold_over_5_lines` | 跨 ≥5 行折叠中间为 `...` | 6 行区间 → 仅首 2/末 2 行 + `...` gutter | `hasMoreThanFiveLines` 分支 | |
| `code_snippet_tabs_to_spaces` | tab 转单空格 | 行含 `\t` → 输出空格 | `ReplaceAll("\t"," ")` | |
| `summary_zero_errors_empty` | 0 错误不输出 | `[]` → `""` | `WriteErrorSummaryText` total==0 | |
| `summary_single_error_in_file` | `Found 1 error in a.ts` | 1 file 1 err → 本地化单数串 | `Found_1_error_in_0` | |
| `summary_single_global_error` | 无 file 的单错 | 1 global err → `Found 1 error` | `Found_1_error` | |
| `summary_multi_files_table` | `Found N errors in M files` + 表 | 2 files 3 errs → 计数 + 排序文件表 | `Found_0_errors_in_1_files` + `writeTabularErrorsDisplay` | |
| `summary_sorted_by_filename` | 文件按名字排序确定性 | files `b.ts`,`a.ts` → 表中 `a.ts` 在前 | `getErrorSummary` sort | |
| `category_format_colors` | 各 category 对应 ANSI | error→红/warning→黄/suggestion→灰/message→蓝 | `getCategoryFormat` | |
| `try_clear_screen_watch` | watch start code 清屏 | 匹配 `ScreenStartingCodes` 且非 preserve → 返回 true + 写清屏序列 | `TryClearScreen` | |
| `try_clear_screen_suppressed` | preserveWatchOutput 抑制 | `PreserveWatchOutput:true` → false，无输出 | `TryClearScreen` | |
| `related_information_rendered` | relatedInformation 带位置渲染 | diag+1 related → 额外缩进块 | `FormatDiagnosticWithColorAndContext` related 分支 | |

## 与 impl.md 的对齐核对

- [ ] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/diagnosticwriter/diagnosticwriter.go:<Func>`，因 Go 侧无 `*_test.go`）

- [ ] impl.md 中每个公开渲染函数都有上表至少一条用例承载（或标注 P10 兜底）
- [ ] 代码片段三分支（首行/末行/折叠）均覆盖
- [ ] 汇总三分支（单错/单文件多错/多文件）均覆盖
- [ ] expected 取自 TS 已知格式 / Go 实测（非 Rust 推断）
- [ ] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `tsc --pretty` 全量终端 baseline | 需真实编译输出对拍 | P10 |
| LSP 诊断（非 ASTDiagnostic）渲染 | `Diagnostic` 的 LSP 实现尚未移植 | P8 |
| 本地化消息（非 en）汇总串 | 依赖 locale 数据完整 | P10 |
