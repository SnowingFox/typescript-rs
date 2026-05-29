# diagnosticwriter: 实现方案（impl.md）

**crate**：`tsgo_diagnosticwriter`　**目标**：把诊断（`ast.Diagnostic` / LSP 诊断）格式化成人类可读文本——带颜色/代码片段的 pretty 输出、`file(line,col): error TSxxxx: msg` 紧凑输出、错误汇总表。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_diagnostics` `tsgo_locale` `tsgo_scanner` `tsgo_tspath`
**Go 源**：`internal/diagnosticwriter/`（1 个非测试文件，约 506 行）

## 这个包是什么（业务说明）

编译/检查产生 `Diagnostic` 之后，需要落到终端或 LSP 输出。本包是"诊断 → 字符串"的渲染层，对齐 TypeScript 的 `formatDiagnosticsWithColorAndContext` / `formatDiagnostic` / `getErrorSummaryText`：

- **pretty 模式**（`FormatDiagnosticWithColorAndContext`）：`path:line:col - error TS2304: ...`，下面跟带行号 gutter 的源码片段，错误区间用 `~~~~` 波浪线标注，跨 5 行以上折叠中间行为 `...`，ANSI 颜色转义。
- **紧凑模式**（`WriteFormatDiagnostic`）：`path(line,col): error TS2304: message`，给非 TTY / `--pretty false`。
- **错误汇总**（`WriteErrorSummaryText`）：`Found N errors in M files` + 按文件分组的表格。
- **message chain 扁平化**：嵌套的 `messageChain` 按层级缩进展开。

它放在 Phase 5 是因为依赖 `scanner` 的行列换算（P3）与 `diagnostics`/`locale` 的消息本地化（P1/P2），且与 emit 同属"输出"环节。位置/列号一律走 **ECMA line map + UTF-16** 口径（与编辑器一致）。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `FileLike interface`（`FileName()/Text()/ECMALineMap()`） | `pub trait FileLike` | 抽象 source file，供位置换算 |
| `Diagnostic interface`（11 方法，抽象 ast 与 LSP 诊断） | `pub trait Diagnostic` | 含 `message_chain()/related_information()` 返回 `Vec<Box<dyn Diagnostic>>` 或 `Vec<DiagnosticRef>` |
| `ASTDiagnostic struct{ *ast.Diagnostic }`（嵌入） | `pub struct AstDiagnostic(/* ref/idx to */ ast::Diagnostic)` + `impl Diagnostic` | Go 用嵌入转发；Rust 组合 + 手写 `impl Diagnostic`（无继承，见 PORTING §4） |
| `ToDiagnostics[T Diagnostic](diags []T) []Diagnostic` 泛型 | `pub fn to_diagnostics<T: Diagnostic>(...) -> Vec<Box<dyn Diagnostic>>` | trait 对象装箱 |
| `FormattedWriter func(io.Writer, text, style)` | `type FormattedWriter = fn(&mut dyn Write, &str, &str)` 或 `impl Fn` | 注入着色策略（pretty 用带色，测试可用无色） |
| `io.Writer` 输出目标 | `&mut dyn std::fmt::Write` / `&mut String` | 全部 `fmt.Fprint(output, ...)` → `write!(out, ...)` |
| `map[FileLike][]Diagnostic`（ErrorSummary 按文件分组） | `IndexMap<FileKey, Vec<...>>` + 显式 `sorted_files` | **影响输出顺序**，必须有序（Go 已注释 "Need an ordered map here, but sorting for consistency"），用 IndexMap 并按 FileName 排序 |
| ANSI 颜色常量（`\u001b[91m` 等） | `const FOREGROUND_COLOR_ESCAPE_RED: &str = "\x1b[91m"` 等 | 逐常量 1:1 |
| `unicode.IsSpace` / `core.UTF16Len` | `char::is_whitespace` 调整 + `tsgo_core::utf16_len` | trim 与波浪线长度均按 UTF-16 计宽 |

> `FileLike` 作 map key：Go 用接口值（指针）作 key。Rust 侧用稳定标识（如 `FileName` 字符串或文件 `NodeId`/指针等价的 key newtype）做分组 key，再用 `sorted_files: Vec<FileKey>` 保证输出确定性。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/diagnosticwriter/diagnosticwriter.go` | `internal/diagnosticwriter/lib.rs` | 全部内容（单文件包，basename == 目录名 → `lib.rs`） |

## 依赖白名单（本包新增的 crate）

无 §10 之外新增。ANSI 转义手写常量；不引入 `colored`/`termcolor`（保持与 Go 字节级一致，便于测试断言）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/diagnosticwriter/diagnosticwriter.go`）

类型 / 适配：
- [ ] `pub trait FileLike`　`// Go: diagnosticwriter.go:FileLike`
- [ ] `pub trait Diagnostic`（`file/pos/end/len/code/category/localize/message_chain/related_information`）　`// Go: diagnosticwriter.go:Diagnostic`
- [ ] `pub struct AstDiagnostic` + `impl Diagnostic`（含 `message_chain`/`related_information` 包裹子诊断）　`// Go: diagnosticwriter.go:ASTDiagnostic`
- [ ] `pub fn wrap_ast_diagnostic(d) -> AstDiagnostic`　`// Go: diagnosticwriter.go:WrapASTDiagnostic`
- [ ] `pub fn wrap_ast_diagnostics(diags) -> Vec<AstDiagnostic>`　`// Go: diagnosticwriter.go:WrapASTDiagnostics`
- [ ] `pub fn from_ast_diagnostics(diags) -> Vec<Box<dyn Diagnostic>>`　`// Go: diagnosticwriter.go:FromASTDiagnostics`
- [ ] `pub fn to_diagnostics<T: Diagnostic>(diags) -> Vec<Box<dyn Diagnostic>>`　`// Go: diagnosticwriter.go:ToDiagnostics`
- [ ] `pub fn compare_ast_diagnostics(a, b) -> Ordering`　`// Go: diagnosticwriter.go:CompareASTDiagnostics`
- [ ] `pub struct FormattingOptions { locale, compare_paths_options, new_line }`　`// Go: diagnosticwriter.go:FormattingOptions`
- [ ] `pub type FormattedWriter` + `fn write_with_style_and_reset`（私有默认实现）　`// Go: diagnosticwriter.go:FormattedWriter / writeWithStyleAndReset`

pretty 输出：
- [ ] `pub fn format_diagnostics_with_color_and_context(out, diags, opts)` — 逐条间插 newLine　`// Go: diagnosticwriter.go:FormatDiagnosticsWithColorAndContext`
- [ ] `pub fn format_diagnostic_with_color_and_context(out, diag, opts)` — location + category + `TSxxxx` + 扁平消息 + 代码片段 + relatedInformation　`// Go: diagnosticwriter.go:FormatDiagnosticWithColorAndContext`
- [ ] `fn write_code_snippet(out, file, start, length, squiggle_color, indent, opts)`（私有）— gutter 行号、5 行折叠省略、`~` 波浪线（首行/末行/中间三分支）、tab→空格、UTF-16 计宽　`// Go: diagnosticwriter.go:writeCodeSnippet`

消息扁平化：
- [ ] `pub fn flatten_diagnostic_message(d, new_line, locale) -> String`　`// Go: diagnosticwriter.go:FlattenDiagnosticMessage`
- [ ] `pub fn write_flattened_ast_diagnostic_message(out, d, newline, locale)`　`// Go: diagnosticwriter.go:WriteFlattenedASTDiagnosticMessage`
- [ ] `pub fn write_flattened_diagnostic_message(out, d, newline, locale)`　`// Go: diagnosticwriter.go:WriteFlattenedDiagnosticMessage`
- [ ] `fn flatten_diagnostic_message_chain(out, chain, new_line, locale, level)`（私有，递归缩进）　`// Go: diagnosticwriter.go:flattenDiagnosticMessageChain`
- [ ] `fn get_category_format(category) -> &'static str`（私有；未知 category → panic）　`// Go: diagnosticwriter.go:getCategoryFormat`

位置 / 紧凑输出：
- [ ] `pub fn write_location(out, file, pos, opts, writer)` — `relativePath:line+1:col+1` 着色　`// Go: diagnosticwriter.go:WriteLocation`
- [ ] `pub fn write_format_diagnostics(out, diags, opts)`　`// Go: diagnosticwriter.go:WriteFormatDiagnostics`
- [ ] `pub fn write_format_diagnostic(out, diag, opts)` — `path(line,col): category TSxxxx: msg`　`// Go: diagnosticwriter.go:WriteFormatDiagnostic`

错误汇总：
- [ ] `pub struct ErrorSummary { total_error_count, global_errors, errors_by_file, sorted_files }`　`// Go: diagnosticwriter.go:ErrorSummary`
- [ ] `pub fn write_error_summary_text(out, all_diags, opts)` — `Found 1 error` / `Found N errors in M files` 等本地化分支　`// Go: diagnosticwriter.go:WriteErrorSummaryText`
- [ ] `fn get_error_summary(diags) -> ErrorSummary`（私有；只数 `CategoryError`，按 FileName 排序）　`// Go: diagnosticwriter.go:getErrorSummary`
- [ ] `fn write_tabular_errors_display(out, summary, opts)`（私有；左对齐宽度计算）　`// Go: diagnosticwriter.go:writeTabularErrorsDisplay`
- [ ] `fn pretty_path_for_file_error(file, file_errors, opts) -> String`（私有）　`// Go: diagnosticwriter.go:prettyPathForFileError`

状态行 / 屏幕：
- [ ] `pub fn format_diagnostics_status_with_color_and_time(out, time, diag, opts)`　`// Go: diagnosticwriter.go:FormatDiagnosticsStatusWithColorAndTime`
- [ ] `pub fn format_diagnostics_status_and_time(out, time, diag, opts)`　`// Go: diagnosticwriter.go:FormatDiagnosticsStatusAndTime`
- [ ] `pub static SCREEN_STARTING_CODES: &[i32]`　`// Go: diagnosticwriter.go:ScreenStartingCodes`
- [ ] `pub fn try_clear_screen(out, diag, options) -> bool` — watch 模式清屏 `\x1B[2J\x1B[3J\x1B[H`　`// Go: diagnosticwriter.go:TryClearScreen`

### Cargo / crate 接线

- [ ] `internal/diagnosticwriter/Cargo.toml`（`name = "tsgo_diagnosticwriter"` + path deps：`tsgo_ast` `tsgo_core` `tsgo_diagnostics` `tsgo_locale` `tsgo_scanner` `tsgo_tspath`）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] `lib.rs` 公开 re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `FormattingOptions` + `write_with_style_and_reset` + `get_category_format`（颜色常量打底）。
2. `flatten_diagnostic_message` / `write_flattened_diagnostic_message`（无位置依赖，先用一个最小 `Diagnostic` 假实现 + message chain，断言缩进）。
3. `write_location` + `write_format_diagnostic`（紧凑模式，依赖 scanner 行列换算）。
4. `write_code_snippet`（最难：gutter/折叠/波浪线，需 ECMA line map fixture）。
5. `get_error_summary` + `write_error_summary_text` + `write_tabular_errors_display`。
6. `try_clear_screen` / 状态行函数（收尾）。

## 与 Go 的已知偏离（divergence）

- `ASTDiagnostic` 用 Go 结构体嵌入（`*ast.Diagnostic`）做方法转发；Rust 改组合 + 手写 `impl Diagnostic`，行为 1:1。
- `errorsByFile` map 在 Go 里是无序 map 再 sort；Rust 直接用 `IndexMap` + 显式 sort，保证输出确定性（断言前提）。
- 波浪线/缩进宽度：Go 用 `core.UTF16Len` 计宽，Rust 必须同口径（不能按 byte 或 char），否则非 ASCII 行的 `~` 数量会偏。

## 转交 / 推迟（DEFER）

- Go 侧 0 直接单测 → 行为由 **P10**（`tsc --pretty` baseline、watch 输出对拍）兜底；本轮补行为级 Rust 测试（见 tests.md）。
- LSP 诊断侧的 `Diagnostic` 实现（非 `ASTDiagnostic`）在 P8 `lsp` 落地；本包只定义 trait，LSP 适配 `// DEFER(phase-8)`。
