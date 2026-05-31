# ls/lsconv: 实现方案（impl.md）

> 本文档在执行期补建：原仓库 `phase-7-language-service/` 下**没有** lsconv 的 impl.md/tests.md，
> 故按 Go 源码 + PORTING/tdd 直接移植，并在此记录推进与偏离。
> 语言：规划文档用中文；`.rs` 内所有注释一律英文（PORTING §7）。
> TDD：逐行为 红→绿（见 [references/tdd.md](../../references/tdd.md)），证据见本文「TDD 推进顺序」。

**crate**：`tsgo_ls_lsconv`　**目标**：语言服务的"换算层"——内部 UTF-8 字节偏移 ↔ LSP 0-based `(line, UTF-16 character)`，以及内部文件名 ↔ LSP `DocumentUri`。
**依赖（crate）**：`tsgo_core` `tsgo_lsproto` `tsgo_bundled` `tsgo_tspath`（dev-dep：`serde`/`serde_json`，仅 JS-reference 交叉验证测试用）
**Go 源**：`internal/ls/lsconv/`（2 个非测试文件：`converters.go` 357 行、`linemap.go` 72 行）

## 这个包是什么（业务说明）

LSP 协议用 0-based `(line, UTF-16 character)` 表示位置，而编译器内部统一用 UTF-8 **字节偏移**。
`lsconv` 是这两套坐标系之间的双向换算地基：每个 `Provide*` 边界都要靠 `Converters` 把
内部 `TextRange`/位置换成 `lsproto.Range`/`Position`，反之亦然。它还负责文件名 ↔ `DocumentUri`
（含盘符卷、UNC、`untitled:` 动态文件、百分号转义），以及（**本轮推迟**）把 `ast.Diagnostic` 转成 `lsproto.Diagnostic`。

它是 checker 无关的叶子：不 import checker/compiler/transformers，也不依赖 `ls/lsutil`、`ls/change`，
因此可与 checker lane 并行移植。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Script.Text() string` | `Script::text(&self) -> &[u8]` | **偏离**：Go `string` 是字节序列，换算定义在原始字节上，文本可含非法 UTF-8（见 `TestConvertersInvalidUTF8`）。Rust `&str` 无法表示非法 UTF-8，故文本建模为字节。 |
| `utf8.DecodeRuneInString` | `decode_rune(&[u8]) -> (char, usize)` | 私有；非法/空输入 → `U+FFFD`（空宽 0、非法宽 1），镜像 Go RuneError 语义。 |
| `utf16.RuneLen(r)` | `char::len_utf16()` | BMP=1、补充平面=2、`U+FFFD`=1，与 Go 一致。 |
| `getLineMap func(string) *LSPLineMap` | `Box<dyn Fn(&str) -> Rc<LSPLineMap>>`（别名 `GetLineMap`） | `Rc` 充当 Go 的共享 `*LSPLineMap` 指针。 |
| `lsproto.PositionEncodingKind` | **本地 shim** `PositionEncodingKind(pub String)` | **偏离**：lsproto 尚未移植该类型，故本地镜像（值 `"utf-8"/"utf-16"/"utf-32"`，风格仿 lsproto `FoldingRangeKind`）。lsproto 移植后应上移并改 re-export。 |
| `net/url.PathEscape` | 私有 `path_escape` + `should_escape_path_segment` | 1:1 移植 Go `escape(s, encodePathSegment)` 的 `shouldEscape`：不转义 `[A-Za-z0-9] - _ . ~ $ & + : = @`，其余（含 `/ ; , ?`、非 ASCII 字节）转义。 |
| `strings.NewReplacer(...)` (`extraEscapeReplacer`) | 私有 `extra_escape_replace` | 逐字符替换表，1:1。 |

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/ls/lsconv/linemap.go` | `internal/ls/lsconv/linemap.rs` | `LSPLineMap`/`LSPLineStarts` + `compute_lsp_line_starts` + `compute_index_of_line_start`。 |
| `internal/ls/lsconv/converters.go` | `internal/ls/lsconv/converters.rs` | `Script`/`Converters`/位置换算/`file_name_to_document_uri` + 已推迟项。 |
| —（crate 根） | `internal/ls/lsconv/lib.rs` | `mod` 声明 + re-export。 |

## 依赖白名单（本包新增的 crate）

无新增第三方 crate。`serde`/`serde_json`（dev-dep，版本对齐 `lsproto/Cargo.toml`：`1.0.228`/`1.0.150`）仅供
JS-reference 交叉验证测试解析 Node 输出。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `linemap.rs`（Go: `internal/ls/lsconv/linemap.go`）

- [x] `pub type LSPLineStarts = Vec<TextPos>` — `// Go: linemap.go:LSPLineStarts`
- [x] `pub struct LSPLineMap { line_starts, ascii_only }` — `// Go: linemap.go:LSPLineMap`
- [x] `pub fn compute_lsp_line_starts(&[u8]) -> LSPLineMap` — 仅 `\n`/`\r`/`\r\n` 断行 + ascii-only 检测　`// Go: linemap.go:ComputeLSPLineStarts`
- [x] `pub fn LSPLineMap::compute_index_of_line_start(TextPos) -> i32` — `// Go: linemap.go:ComputeIndexOfLineStart`

### `converters.rs`（Go: `internal/ls/lsconv/converters.go`）

- [x] `pub trait Script { file_name, text }` — `text -> &[u8]`（见偏离）　`// Go: converters.go:Script`
- [x] `pub struct PositionEncodingKind`（本地 shim）+ `utf8/utf16/utf32`
- [x] `pub struct Converters` + `pub fn new` — `// Go: converters.go:NewConverters`
- [x] `pub fn to_lsp_range` — `// Go: converters.go:ToLSPRange`
- [x] `pub fn from_lsp_range` — `// Go: converters.go:FromLSPRange`
- [x] `pub fn to_lsp_location` — `// Go: converters.go:ToLSPLocation`
- [x] `pub fn line_and_character_to_position` — `// Go: converters.go:LineAndCharacterToPosition`
- [x] `pub fn position_to_line_and_character` — `// Go: converters.go:PositionToLineAndCharacter`
- [x] `pub fn file_name_to_document_uri` — bundled/dynamic/卷/UNC/转义全分支　`// Go: converters.go:FileNameToDocumentURI`
- [x] 私有 `decode_rune` / `path_escape` / `should_escape_path_segment` / `extra_escape_replace`
- [ ] DEFER `from_lsp_text_change` — `// Go: converters.go:FromLSPTextChange`
- [ ] DEFER `language_kind_to_script_kind` — `// Go: converters.go:LanguageKindToScriptKind`
- [ ] DEFER `diagnostic_to_lsp_pull` / `diagnostic_to_lsp_push` / `diagnostic_to_lsp` / `message_chain_to_string` / `ptr_to_slice_if_non_empty` / `diagnosticOptions` / `styleCheckDiagnostics`

### Cargo / crate 接线

- [x] `internal/ls/lsconv/Cargo.toml`（`name = "tsgo_ls_lsconv"` + path deps）
- [x] 根 `Cargo.toml` workspace members 追加 `"internal/ls/lsconv"`
- [x] `lib.rs` 声明 `mod converters; mod linemap;` + re-export

## TDD 推进顺序（tracer bullet → 增量；红→绿证据）

1. `compute_lsp_line_starts`（ASCII 单换行）：RED（`todo!()` panic）→ GREEN（实现循环）。
2. `compute_lsp_line_starts` 其余行为（CRLF / 单 CR / 非 ASCII 清 ascii_only / 空）：随实现绿。
3. `compute_index_of_line_start`：RED（`todo!()`）→ GREEN（`binary_search` + 回退一格）。
4. `file_name_to_document_uri` tracer（`/path/to/file.ts`）：RED → GREEN（split+pathEscape+replacer+卷+动态分支）。
5. `file_name_to_document_uri` 全表（18 行，含动态 `untitled:`/卷/UNC/非 ASCII）：绿。
6. `position_to_line_and_character` + `line_and_character_to_position`（BMP 非 ASCII tracer，强制 UTF-16 分支）：RED → GREEN。
7. `TestConvertersInvalidUTF8`（非法字节 0x80 双向 + 全字节往返）：绿。
8. `TestConvertersAgainstJSReference`（14 例与 Node TextDecoder 交叉验证）：绿（node v23 可用）。
9. `to_lsp_range`/`from_lsp_range`/`to_lsp_location` 行为单测：绿。

## 与 Go 的已知偏离（divergence）

1. `Script::text() -> &[u8]`（非 `&str`）：见上「类型映射」，为 1:1 还原 Go 字节语义 + 支持非法 UTF-8 测试。
2. `PositionEncodingKind` 本地 shim：lsproto 未移植该类型，编辑边界又只允许改 `internal/ls/lsconv/**`，
   故本地镜像（标 `// TODO(port)`）。lsproto 移植后应删除 shim、改依赖 lsproto 并 re-export。
3. UTF-16 长度用 `char::len_utf16()` 替代 `utf16.RuneLen`（语义等价）；非法字节经 `decode_rune` → `U+FFFD`（宽 1）。
4. `getLineMap` 返回 `Rc<LSPLineMap>`（Go `*LSPLineMap` 共享指针）。

## 转交 / 推迟（DEFER）

| 项 | blocked-by | 目标 |
|---|---|---|
| `from_lsp_text_change`（`FromLSPTextChange`） | `tsgo_lsproto::TextDocumentContentChangePartial` 未移植 | lsproto 补该类型后回填 |
| `language_kind_to_script_kind`（`LanguageKindToScriptKind`） | `tsgo_lsproto::LanguageKind` 未移植 | lsproto 补该类型后回填 |
| `DiagnosticToLSPPull/Push` + `diagnosticToLSP` + `messageChainToString` + `diagnosticOptions` + `styleCheckDiagnostics` + `ptrToSliceIfNonEmpty` | ① `ast::Diagnostic` 未移植；② lsproto 未移植 `Diagnostic`/`DiagnosticSeverity`/`DiagnosticTag`/`DiagnosticRelatedInformation`/`ClientCapabilities`/`GetClientCapabilities`；③ `diagnosticwriter::write_flattened_ast_diagnostic_message` 本身 DEFER（同样 blocked-by `ast::Diagnostic`）；④ `tsgo_locale`/`tsgo_collections`/`tsgo_diagnostics` 依赖尚未接线 | 待 ast::Diagnostic + lsproto 诊断类型 + diagnosticwriter 落地后，整段回填（一次性带测试） |

> 这些项**无 Go 单测**（`converters_test.go` 只覆盖 URI 与位置换算），故推迟不丢测试覆盖。
