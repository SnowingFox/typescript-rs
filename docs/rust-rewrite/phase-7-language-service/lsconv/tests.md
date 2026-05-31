# ls/lsconv: 测试清单（tests.md）

> 执行期补建（原仓库无此文档）。逐 `func Test*`、逐表驱动子用例对齐 `internal/ls/lsconv/converters_test.go`。
> **完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟（带 blocked-by）。
> **Go 测试规模**：1 文件（`converters_test.go`）/ 4 个 `func Test` / 共约 50 子用例（URI 18 + 非法 UTF-8 7 映射 + 全字节往返 + JS-ref 14 例）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试文件 | 顶层测试函数数 |
|---|---|---|
| `internal/ls/lsconv/converters_test.go` | `internal/ls/lsconv/converters_test.rs`（`use super::*;`，挂在 `converters.rs` 末尾） | URI/位置换算相关全部 |
| —（Go 无 linemap_test.go） | `internal/ls/lsconv/linemap_test.rs` | linemap 每函数补测 |

## `converters_test.go`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `file_name_to_document_uri_simple_posix_path` | tracer：普通 POSIX 路径 | `/path/to/file.ts` → `file:///path/to/file.ts` | `TestFileNameToDocumentURI` | ✓ |
| `file_name_to_document_uri_table` | 全表 18 行（卷盘符 `c%3A`、UNC、非 ASCII `%C3%BC…`、`(test)`→`%28%29`、`c#`→`%23`、`$`→`%24`、`untitled:` 动态 3 例等） | 见 Go 表字面量 | `TestFileNameToDocumentURI`（全子用例） | ✓ |
| `position_to_line_and_character_bmp` | tracer：字节→(line,UTF-16 char)，强制 UTF-16 分支 | `"α\nβ"`：0→(0,0)、2→(0,1)、3→(1,0) | （`PositionToLineAndCharacter`） | ✓ |
| `line_and_character_to_position_bmp` | tracer：(line,char)→字节，逆向 | 同上反向 | （`LineAndCharacterToPosition`） | ✓ |
| `converters_invalid_utf8` | 非法字节 0x80：每行 7 个 (line,char)↔byte 双向 + 全字节往返 | `"a\x80b\ncd"`，见 Go `mappings` 字面量 | `TestConvertersInvalidUTF8`（全子用例） | ✓ |
| `converters_against_js_reference` | 与 Node `TextDecoder` 真 UTF-16 语义交叉验证 14 例（empty/ascii/crlf/cr/trailing/bmp/补充平面 emoji/ZWJ/混合空白/纯换行…），每例对每个码点边界双向校验 | 14 例文本 → Node 计算的权威映射 | `TestConvertersAgainstJSReference`（全 14 子用例；node 不可用则跳过） | ✓ |
| `to_and_from_lsp_range_roundtrip` | `ToLSPRange`/`FromLSPRange` 组合往返 | `"ab\ncd"` `TextRange(0,5)` ↔ `Range{(0,0),(1,2)}` | （`ToLSPRange`/`FromLSPRange`，Go 无直接单测） | ✓ |
| `to_lsp_location_builds_uri_and_range` | `ToLSPLocation` 拼 URI + range | `/a/b.ts`,`(0,5)` → `Location{file:///a/b.ts,(0,0)..(1,2)}` | （`ToLSPLocation`，Go 无直接单测） | ✓ |
| （`DocumentUri.FileName()`） | URI→文件名 | 见 Go 表 | `TestDocumentURIToFileName` | — blocked-by `lsproto::DocumentUri::file_name`（属 lsproto，非 lsconv；且 lsproto 未移植该方法） |

## linemap（Go 无 `*_test.go` → 按 §8.6 每函数补行为单测）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `compute_lsp_line_starts_ascii_single_newline` | 单换行两行 + ascii_only | `b"hello\nworld"` → `[0,6]`, true | Go `ComputeLSPLineStarts` 语义 | ✓ |
| `compute_lsp_line_starts_crlf` | `\r\n` 算一个断行 | `b"a\r\nb"` → `[0,3]` | 同上 | ✓ |
| `compute_lsp_line_starts_cr_only` | 单 `\r` 也断行 | `b"a\rb"` → `[0,2]` | 同上 | ✓ |
| `compute_lsp_line_starts_non_ascii_clears_flag` | 非 ASCII 清 ascii_only | `"α\nβ"` → `[0,3]`, false | 同上 | ✓ |
| `compute_lsp_line_starts_empty` | 空文本一行 | `b""` → `[0]`, true | 同上 | ✓ |
| `compute_index_of_line_start_basic` | 精确命中/行内/边界 | `[0,5,10]`：5→1,7→1,0→0,3→0,12→2 | Go `ComputeIndexOfLineStart`（`slices.BinarySearch` 语义） | ✓ |

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（`TestFileNameToDocumentURI`/`TestConvertersInvalidUTF8`/`TestConvertersAgainstJSReference` ✓；`TestDocumentURIToFileName` `—` 属 lsproto）
- [x] 每个表驱动子用例都已逐行列出（URI 18 行、非法 UTF-8 映射、JS-ref 14 例）
- [x] expected 值均取自 Go 测试字面量
- [x] 每个测试文件/用例带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐：impl.md 每个已实现 `pub fn` 在此都有覆盖

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因（blocked-by） | 目标 phase |
|---|---|---|
| `TestDocumentURIToFileName` | `lsproto::DocumentUri::file_name` 未移植（且属 lsproto crate，非 lsconv 编辑边界） | lsproto 补 `DocumentUri.FileName()` 后在 lsproto 收口 |
| `from_lsp_text_change` 行为 | `lsproto::TextDocumentContentChangePartial` 未移植 | lsproto 补类型后 |
| `language_kind_to_script_kind` 行为 | `lsproto::LanguageKind` 未移植 | lsproto 补类型后 |
| `DiagnosticToLSP*` / `diagnosticToLSP` 行为 | `ast::Diagnostic` + lsproto 诊断类型 + `diagnosticwriter::write_flattened_ast_diagnostic_message`（本身 DEFER）均未就绪 | ast::Diagnostic + 相关类型落地后（Go 侧本就无这些函数的单测） |
