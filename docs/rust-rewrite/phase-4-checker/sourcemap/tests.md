# sourcemap: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 文件 / **30 `func Test`** / 30 子用例（每个 `func` 一个直写场景，非表驱动 `t.Run`）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/sourcemap/generator_test.go` | `internal/sourcemap/generator.rs`（`#[cfg(test)] mod tests`） | 30 |

> 全部 30 个测试都针对 `Generator`：构造 → `AddSource`/`AddName`/`Add*Mapping` → 断言 `RawSourceMap()`（结构）或 `String()`（JSON 串）或错误消息。expected 全部取自 Go 测试字面量。`decoder.rs` / `source_mapper.rs` / `lineinfo.rs` / `util.rs` 在 Go 侧无直接单测（见末节）。

## `generator_test.go`

> 每行一个 `func Test*`。`base=NewGenerator("main.js","/","/",{})`。expected 列给关键断言（mappings/sources/names/sourcesContent 或 error 串）。

### 结构 / 序列化（空 + 基础累积）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `empty` | 空 generator 的 RawSourceMap | `base` → `{v3, file:"main.js", sourceRoot:"/", sources:[], names:[], mappings:"", sourcesContent:nil}` | `generator_test.go:TestSourceMapGenerator_Empty` | |
| `empty_serialized` | 空 generator 的 JSON 串 | `base.String()` → `{"version":3,"file":"main.js","sourceRoot":"/","sources":[],"names":[],"mappings":""}` | `TestSourceMapGenerator_Empty_Serialized` | |
| `add_source` | AddSource 相对化 + 索引 0 | `AddSource("/main.ts")` → idx 0, sources `["main.ts"]` | `TestSourceMapGenerator_AddSource` | |
| `set_source_content` | 设首个源内容 | `AddSource("/main.ts")`+`SetSourceContent(0,"foo")` → sourcesContent `[&"foo"]` | `TestSourceMapGenerator_SetSourceContent` | |
| `set_source_content_for_second_source_only` | 仅第二源有内容 → 首位 nil | 两源, `SetSourceContent(1,"foo")` → sources `["skipped.ts","main.ts"]`, content `[nil,&"foo"]` | `TestSourceMapGenerator_SetSourceContent_ForSecondSourceOnly` | |
| `set_source_content_out_of_range` | 越界返回错误 | `SetSourceContent(-1,"")`/`SetSourceContent(0,"")`（无源）→ err `"sourceIndex is out of range"` | `TestSourceMapGenerator_SetSourceContent_SourceIndexOutOfRange` | |
| `set_source_content_for_second_source_only_serialized` | null 占位序列化 | → `..."sources":["skipped.ts","main.ts"],"names":[],"mappings":"","sourcesContent":[null,"foo"]}` | `TestSourceMapGenerator_SetSourceContent_ForSecondSourceOnly_Serialized` | |
| `add_name` | AddName 索引 0 | `AddName("foo")` → idx 0, names `["foo"]` | `TestSourceMapGenerator_AddName` | |

### Mapping 编码（正例，逐字 mappings 串）

| Rust 测试 | 验证内容 | input → expected mappings | Go 对照 | 完成 |
|---|---|---|---|---|
| `add_generated_mapping` | 单生成映射 | `AddGeneratedMapping(0,0)` → `"A"` | `TestSourceMapGenerator_AddGeneratedMapping` | |
| `add_generated_mapping_on_second_line_only` | 第二行前导 `;` | `AddGeneratedMapping(1,0)` → `";A"` | `TestSourceMapGenerator_AddGeneratedMapping_OnSecondLineOnly` | |
| `add_source_mapping` | 带源的首映射 | src+`AddSourceMapping(0,0,0,0,0)` → `"AAAA"` | `TestSourceMapGenerator_AddSourceMapping` | |
| `add_source_mapping_next_generated_character` | 生成列+1（同源位置）→ comma | `(0,0,..0,0)`+`(0,1,..0,0)` → `"AAAA,CAAA"` | `TestSourceMapGenerator_AddSourceMapping_NextGeneratedCharacter` | |
| `add_source_mapping_next_generated_and_source_character` | 生成+源列各+1 | `(0,0,..0,0)`+`(0,1,..0,1)` → `"AAAA,CAAC"` | `TestSourceMapGenerator_AddSourceMapping_NextGeneratedAndSourceCharacter` | |
| `add_source_mapping_next_generated_line` | 生成换行 `;` | `(0,0,..0,0)`+`(1,0,..0,0)` → `"AAAA;AAAA"` | `TestSourceMapGenerator_AddSourceMapping_NextGeneratedLine` | |
| `add_source_mapping_previous_source_character` | 源列回退（负相对量 D） | `(0,0,..0,1)`+`(0,1,..0,0)` → `"AAAC,CAAD"` | `TestSourceMapGenerator_AddSourceMapping_PreviousSourceCharacter` | |
| `add_named_source_mapping` | 带名字第 5 段 | src+name+`AddNamedSourceMapping(0,0,0,0,0,0)` → `"AAAAA"`, names `["foo"]` | `TestSourceMapGenerator_AddNamedSourceMapping` | |
| `add_named_source_mapping_with_previous_name` | 名字索引相对量（C/D） | 两名字 bar(1)→foo(0) → `"AAAAC,CAAAD"`, names `["foo","bar"]` | `TestSourceMapGenerator_AddNamedSourceMapping_WithPreviousName` | |

### Mapping 校验（错误例，逐字错误串）

| Rust 测试 | 验证内容 | input → expected error | Go 对照 | 完成 |
|---|---|---|---|---|
| `add_generated_mapping_line_cannot_backtrack` | 生成行回退报错 | `(1,0)` 后 `(0,0)` → `"generatedLine cannot backtrack"` | `TestSourceMapGenerator_AddGeneratedMapping_GeneratedLineCannotBacktrack` | |
| `add_generated_mapping_char_cannot_be_negative` | 生成列为负报错 | `(0,0)` 后 `(0,-1)` → `"generatedCharacter cannot be negative"` | `TestSourceMapGenerator_AddGeneratedMapping_GeneratedCharacterCannotBeNegative` | |
| `add_source_mapping_line_cannot_backtrack` | 生成行回退报错 | `(1,0,..)` 后 `(0,0,..)` → `"generatedLine cannot backtrack"` | `TestSourceMapGenerator_AddSourceMapping_GeneratedLineCannotBacktrack` | |
| `add_source_mapping_char_cannot_be_negative` | 生成列为负报错 | `(0,0,..)` 后 `(0,-1,..)` → `"generatedCharacter cannot be negative"` | `TestSourceMapGenerator_AddSourceMapping_GeneratedCharacterCannotBeNegative` | |
| `add_source_mapping_source_index_out_of_range` | 源索引越界 | `(0,0,-1,0,0)`/`(0,0,0,0,0)`（无源）→ `"sourceIndex is out of range"` | `TestSourceMapGenerator_AddSourceMapping_SourceIndexIsOutOfRange` | |
| `add_source_mapping_source_line_cannot_be_negative` | 源行为负 | src+`(0,0,idx,-1,0)` → `"sourceLine cannot be negative"` | `TestSourceMapGenerator_AddSourceMapping_SourceLineCannotBeNegative` | |
| `add_source_mapping_source_char_cannot_be_negative` | 源列为负 | src+`(0,0,idx,0,-1)` → `"sourceCharacter cannot be negative"` | `TestSourceMapGenerator_AddSourceMapping_SourceCharacterCannotBeNegative` | |
| `add_named_source_mapping_line_cannot_backtrack` | 生成行回退 | name+`(1,..)` 后 `(0,..)` → `"generatedLine cannot backtrack"` | `TestSourceMapGenerator_AddNamedSourceMapping_GeneratedLineCannotBacktrack` | |
| `add_named_source_mapping_char_cannot_be_negative` | 生成列为负 | name+`(0,0,..)` 后 `(0,-1,..)` → `"generatedCharacter cannot be negative"` | `TestSourceMapGenerator_AddNamedSourceMapping_GeneratedCharacterCannotBeNegative` | |
| `add_named_source_mapping_source_index_out_of_range` | 源索引越界 | name+`(0,0,-1,0,0,name)`/`(0,0,0,0,0,name)` → `"sourceIndex is out of range"` | `TestSourceMapGenerator_AddNamedSourceMapping_SourceIndexIsOutOfRange` | |
| `add_named_source_mapping_source_line_cannot_be_negative` | 源行为负 | name+src+`(0,0,idx,-1,0,name)` → `"sourceLine cannot be negative"` | `TestSourceMapGenerator_AddNamedSourceMapping_SourceLineCannotBeNegative` | |
| `add_named_source_mapping_source_char_cannot_be_negative` | 源列为负 | name+src+`(0,0,idx,0,-1,name)` → `"sourceCharacter cannot be negative"` | `TestSourceMapGenerator_AddNamedSourceMapping_SourceCharacterCannotBeNegative` | |
| `add_named_source_mapping_name_index_out_of_range` | 名字索引越界 | src+`(0,0,idx,0,0,-1)`/`(0,0,idx,0,0,0)`（无名字）→ `"nameIndex is out of range"` | `TestSourceMapGenerator_AddNamedSourceMapping_NameIndexIsOutOfRange` | |

合计：8 + 9 + 13 = **30 个 `func Test`，全部逐一映射**。

## 0 直接单测的同包其他文件（补行为级 Rust 测试）

`decoder.rs` / `source_mapper.rs` / `lineinfo.rs` / `util.rs` Go 侧无 `*_test.go`；行为由 **P10 parity**（语言服务 go-to-definition 穿透 source map）兜底。本轮补：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `vlq_roundtrip` | VLQ 编码后解码还原 | 对 `[-1,0,1,16,1000]` encode→decode → 原值 | `appendBase64VLQ`/`base64VLQFormatDecode` 对偶 | |
| `decode_simple_mappings` | 解码 `"AAAA"` | → 1 个 `Mapping{0,0,0,0,0,Missing}` | `MappingsDecoder.Next` | |
| `decode_roundtrip_generator` | generator 产出再解码一致 | `"AAAA,CAAC"` → 2 个映射，位置匹配输入 | decoder↔generator | |
| `decode_invalid_char` | 非法字符报错 | `"!!"` → `error()=="Invalid character in VLQ"` | `base64FormatDecode` -1 分支 | |
| `try_get_source_mapping_url_found` | 尾行 `//# sourceMappingURL=` | 末行 `//# sourceMappingURL=a.js.map` → `"a.js.map"` | `util.go:TryGetSourceMappingURL` | |
| `try_get_source_mapping_url_none` | 无注释返回空 | 普通末行 → `""` | 同上 break 分支 | |
| `try_parse_base64_url_ok` | data URL 识别 | `data:application/json;base64,AAA=` → `("AAA=", true)` | `source_mapper.go:tryParseBase64Url` | |
| `ecma_line_info_line_text` | 行起点 → 行文本 | text+lineStarts → `LineText(0)`/`LineText(1)` 切片正确 | `lineinfo.go:LineText` | |

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*`（30 个）都已逐一映射
- [ ] mapping 正例的 `mappings` 串逐字对齐（`"A"`/`";A"`/`"AAAA"`/`"CAAC"`/`"CAAD"`/`"AAAAC,CAAAD"` 等）
- [ ] 错误例的错误消息逐字对齐
- [ ] `Empty_Serialized` / `ForSecondSourceOnly_Serialized` 的 JSON 整串逐字对齐
- [ ] expected 值均取自 Go 测试字面量（非 Rust 推断）
- [ ] 每条带 `// Go:` 锚点
- [ ] decoder/mapper/util 补测与 impl.md TODO 对齐

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `DocumentPositionMapper` 端到端（穿透 `.js.map` 的 go-to-def） | 需 program + ls fixtures | P7 / P10 |
| `--sourceMap` 生成的 map 与 tsc 字节级对拍 | 需真实 emit 输出 | P10 |
