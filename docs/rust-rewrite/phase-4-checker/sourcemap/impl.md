# sourcemap: 实现方案（impl.md）

> **phase 归属（依赖序修正）**：本包**前移到 P4**（原列 P5）。原因：`printer`（现 P4）非测试依赖 `tsgo_sourcemap`。VLQ **自实现**（见 [references/crate-map.md](../../references/crate-map.md)）。

**crate**：`tsgo_sourcemap`　**目标**：生成与解析 Source Map v3——`Generator` 增量累积 mappings 并输出 `RawSourceMap`/JSON/base64 data URL；`MappingsDecoder` 解码 VLQ mappings；`DocumentPositionMapper` 做生成位置↔源位置双向映射。
**依赖（crate）**：`tsgo_core` `tsgo_json` `tsgo_tspath` `tsgo_scanner` `tsgo_stringutil` `tsgo_debug`
**Go 源**：`internal/sourcemap/`（6 个非测试文件，约 740 行）

## 这个包是什么（业务说明）

Source map 把生成的 `.js` 里每个位置映射回原始 `.ts` 位置。printer（本 phase 的核心）emit 每个 token 时调用 `Generator.AddSourceMapping` 记录"生成行列 ↔ 源索引/行列/名字索引"；emit 结束 `RawSourceMap()` / `String()` 产出最终 `{version:3, sources, names, mappings, ...}`。mappings 字段是 **Base64 VLQ** 增量编码的核心难点。

反向链路（`DocumentPositionMapper` + `MappingsDecoder`）服务语言服务（go-to-definition 穿透 `.d.ts`/`.js.map`），把生成文件位置解回源文件位置，或反向。`util.go` 负责从文件尾部找 `//# sourceMappingURL=` 注释。

放在 Phase 5：printer 直接依赖 `Generator`；解码/mapper 侧供 P6/P7 复用。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type SourceIndex int` / `type NameIndex int` | `#[derive(Copy,Clone,PartialEq,Eq,PartialOrd,Ord)] struct SourceIndex(i32)` / `NameIndex(i32)` | newtype，保留 `-1 = notSet` 哨兵语义 |
| `core.UTF16Offset`（列偏移） | `tsgo_core::Utf16Offset`（i32 newtype） | 列一律 UTF-16，与 scanner 一致 |
| `Generator`（大量 `last*`/`pending*` 可变状态 + `strings.Builder`） | `pub struct Generator { mappings: String, ... }` + `&mut self` 方法 | 增量编码状态机；`strings.Builder` → `String`（`push`/`push_str`） |
| `sourceToSourceIndexMap map[string]SourceIndex` | `FxHashMap<String, SourceIndex>` | 去重用，不影响输出顺序（顺序由 `sources` Vec 决定） |
| `RawSourceMap` + `json` tag（`omitzero` 的 SourcesContent） | `#[derive(Serialize,Deserialize)] struct RawSourceMap` + `#[serde(skip_serializing_if=...)]` | **字段序与 JSON key 必须与 Go 完全一致**（断言 `String()` 整串）：version,file,sourceRoot,sources,names,mappings,sourcesContent |
| `[]*string`（sourcesContent，元素可空） | `Vec<Option<String>>` | `null` ↔ `None`，序列化保留 null 占位（见测试 `[null,"foo"]`） |
| `(T, error)` 返回 | `Result<T, SourceMapError>`（`thiserror`）；错误消息字符串与 Go 完全一致 | 测试用 `assert.Error(..., "sourceIndex is out of range")` 逐字断言 |
| `panic("generatedLine cannot backtrack")`（不变量违反） | `panic!`/`unreachable!`（`commitPendingMapping` 内部） | 与 `AddGeneratedMapping` 的 `Result` 区分：公开 API 返回错误，内部不变量 panic |
| `iter.Seq[*Mapping]`（`Values()`） | `impl Iterator<Item = Mapping>` / `Iterator` impl on decoder | Go 1.23 range-over-func → Rust Iterator |
| `core.Arena[Mapping]`（decoder 的 mappingArena） | 直接返回 `Mapping`（Copy 值）或 `Vec<Mapping>` | decoder 每步 `captureMapping` 新建；Rust 用值语义即可，无需 arena（`Mapping` 是小 POD） |
| `DocumentPositionMapper`（两套排序去重的 mapping 列表 + 二分） | `pub struct DocumentPositionMapper` + `Vec<MappedPosition>` + `binary_search_by` | `slices.BinarySearchFunc` → `slice::binary_search_by`；`SortFunc`+`DeduplicateSorted` → `sort_by`+`dedup_by` |
| `Host interface`（`UseCaseSensitiveFileNames/GetECMALineInfo/ReadFile`） | `pub trait Host` | mapper 的文件/行信息来源 |

### VLQ 实现决策（crate-map 待定项 → **本 phase 敲定：自实现**）

`references/crate-map.md` 的待定表里 "sourcemap VLQ：自实现 / `vlq` crate（决策 phase P5）"。**结论：自实现**，理由：

1. Go 上游就是内联手写（`appendBase64VLQ` / `base64VLQFormatDecode` / `base64FormatEncode` / `base64FormatDecode`，合计 ~50 行纯位运算），无外部依赖。
2. 1:1 移植这 4 个函数即可，逻辑直白（5 位一组、msb 续位标志、符号位放最低位）。引入 `vlq` crate 反而要适配其 API、增加供应链面、且难保证字节级与 TS 一致。
3. 这是确定性 emit 输出的命门，自己掌控编码细节最稳。

> 落地动作（执行期）：把 `references/crate-map.md` 待定表里 VLQ 行从"自实现 / vlq"更新为"**自实现（P5 敲定）**"。本文档因边界限制不直接改该文件，列为 README 的转交项。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/sourcemap/generator.go` | `internal/sourcemap/generator.rs` | `Generator`/`RawSourceMap` + VLQ 编码 + base64 编码表 |
| `internal/sourcemap/decoder.go` | `internal/sourcemap/decoder.rs` | `Mapping` + `MappingsDecoder` + VLQ 解码 |
| `internal/sourcemap/lineinfo.go` | `internal/sourcemap/lineinfo.rs` | `ECMALineInfo`（行起点 → 行文本） |
| `internal/sourcemap/source.go` | `internal/sourcemap/source.rs` | `Source` trait |
| `internal/sourcemap/util.go` | `internal/sourcemap/util.rs` | `TryGetSourceMappingURL` |
| `internal/sourcemap/source_mapper.go` | `internal/sourcemap/source_mapper.rs` | `Host`/`DocumentPositionMapper`/`GetDocumentPositionMapper` 等 |
| —（无 `sourcemap.go`，crate 根用 `lib.rs` 汇总 `mod` + re-export） | `internal/sourcemap/lib.rs` | `mod generator; mod decoder; ...` + `pub use` |

## 依赖白名单（本包新增的 crate）

- base64：用 std 自实现编码表（`base64FormatEncode/Decode`）+ data URL 用 `base64` crate 或自实现 `StdEncoding`。**倾向 `base64` crate**（`Base64DataURL` 用标准 base64，引入成熟实现更稳）；记入 crate-map「序列化/编码」。VLQ 部分仍自实现（见上）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `generator.rs`（Go: `internal/sourcemap/generator.go`）

- [ ] `pub struct SourceIndex(i32)` / `pub struct NameIndex(i32)` + 哨兵常量　`// Go: generator.go`
- [ ] `pub struct RawSourceMap`（serde，字段序 1:1）　`// Go: generator.go:RawSourceMap`
- [ ] `pub struct Generator` + 全部状态字段　`// Go: generator.go:Generator`
- [ ] `pub fn new(file, source_root, sources_directory_path, options) -> Generator`　`// Go: generator.go:NewGenerator`
- [ ] `pub fn sources(&self) -> &[String]`（返回 rawSources）　`// Go: generator.go:Sources`
- [ ] `pub fn add_source(&mut self, file_name) -> SourceIndex` — 相对化 + 去重　`// Go: generator.go:AddSource`
- [ ] `pub fn set_source_content(&mut self, idx, content) -> Result<()>` — 越界错误　`// Go: generator.go:SetSourceContent`
- [ ] `pub fn add_name(&mut self, name) -> NameIndex` — 去重　`// Go: generator.go:AddName`
- [ ] `fn is_new_generated_position(...)` / `fn is_backtracking_source_position(...)` / `fn should_commit_mapping(...)`（私有）　`// Go: generator.go:isNewGeneratedPosition / isBacktrackingSourcePosition / shouldCommitMapping`
- [ ] `fn append_mapping_char_code` / `fn append_base64_vlq`（私有，VLQ 自实现）　`// Go: generator.go:appendMappingCharCode / appendBase64VLQ`
- [ ] `fn commit_pending_mapping`（私有；line `;` / comma `,` 分隔 + 5 段相对量；backtrack panic）　`// Go: generator.go:commitPendingMapping`
- [ ] `fn add_mapping`（私有，pending/last 状态推进）　`// Go: generator.go:addMapping`
- [ ] `pub fn add_generated_mapping(&mut self, line, char) -> Result<()>`　`// Go: generator.go:AddGeneratedMapping`
- [ ] `pub fn add_source_mapping(&mut self, line, char, src_idx, src_line, src_char) -> Result<()>`　`// Go: generator.go:AddSourceMapping`
- [ ] `pub fn add_named_source_mapping(&mut self, ..., name_idx) -> Result<()>`　`// Go: generator.go:AddNamedSourceMapping`
- [ ] `pub fn raw_source_map(&mut self) -> RawSourceMap`（先 commit pending；空 sources/names → `[]` 而非 null）　`// Go: generator.go:RawSourceMap`
- [ ] `fn bytes(&mut self) -> Vec<u8>`（私有，json marshal）　`// Go: generator.go:bytes`
- [ ] `pub fn to_string(&mut self) -> String`（impl Display 或具名）　`// Go: generator.go:String`
- [ ] `pub fn base64_data_url(&mut self) -> String` — `data:application/json;base64,` 前缀　`// Go: generator.go:Base64DataURL`
- [ ] `fn base64_format_encode(value) -> char`（私有，6 位 → A-Za-z0-9+/）　`// Go: generator.go:base64FormatEncode`

### `decoder.rs`（Go: `internal/sourcemap/decoder.go`）

- [ ] `pub struct Mapping` + `fn equals` + `fn is_source_mapping`　`// Go: decoder.go:Mapping`
- [ ] `Missing*` 哨兵常量　`// Go: decoder.go`
- [ ] `pub struct MappingsDecoder` + `pub fn decode_mappings(s) -> MappingsDecoder`　`// Go: decoder.go:DecodeMappings`
- [ ] `pub fn mappings_string/pos/error/state`　`// Go: decoder.go:MappingsString/Pos/Error/State`
- [ ] `pub fn values(&mut self) -> impl Iterator<Item=Mapping>` + `fn next`（状态机：`;`/`,`/段解析、错误分支）　`// Go: decoder.go:Values/Next`
- [ ] `fn capture_mapping` / `fn stop_iterating` / `fn set_error` / `fn set_error_and_stop` / `fn has_reported_error` / `fn is_source_mapping_segment_end`（私有）　`// Go: decoder.go:*`
- [ ] `fn base64_vlq_format_decode(&mut self) -> i32`（私有，VLQ 自实现解码）　`// Go: decoder.go:base64VLQFormatDecode`
- [ ] `fn base64_format_decode(ch) -> i32`（私有，字符 → 6 位；非法 → -1）　`// Go: decoder.go:base64FormatDecode`

### `lineinfo.rs` / `source.rs` / `util.rs`

- [ ] `pub struct ECMALineInfo` + `pub fn create_ecma_line_info` + `line_count` + `line_text`　`// Go: lineinfo.go:*`
- [ ] `pub trait Source`（`text/file_name/ecma_line_map`）　`// Go: source.go:Source`
- [ ] `pub fn try_get_source_mapping_url(line_info: Option<&ECMALineInfo>) -> String` — 从尾部行找 `//# sourceMappingURL=`　`// Go: util.go:TryGetSourceMappingURL`

### `source_mapper.rs`（Go: `internal/sourcemap/source_mapper.go`）

- [ ] `pub trait Host`（`use_case_sensitive_file_names/get_ecma_line_info/read_file`）　`// Go: source_mapper.go:Host`
- [ ] `pub struct MappedPosition` + `fn is_source_mapped_position`；`type SourceMappedPosition = MappedPosition`　`// Go: source_mapper.go:MappedPosition`
- [ ] `pub struct DocumentPositionMapper` + `fn create_document_position_mapper`（私有：解码、processMapping、按源/生成位置排序去重）　`// Go: source_mapper.go:createDocumentPositionMapper`
- [ ] `pub struct DocumentPosition { file_name, pos }`　`// Go: source_mapper.go:DocumentPosition`
- [ ] `pub fn get_source_position(&self, loc) -> Option<DocumentPosition>`（二分 generatedMappings）　`// Go: source_mapper.go:GetSourcePosition`
- [ ] `pub fn get_generated_position(&self, loc) -> Option<DocumentPosition>`（二分 sourceMappings）　`// Go: source_mapper.go:GetGeneratedPosition`
- [ ] `pub fn get_document_position_mapper(host, generated_file_name) -> Option<DocumentPositionMapper>`（url/base64/`.map` 文件查找）　`// Go: source_mapper.go:GetDocumentPositionMapper`
- [ ] `fn convert_document_to_source_mapper` / `fn try_parse_raw_source_map` / `fn try_get_source_mapping_url` / `fn try_parse_base64_url`（私有）　`// Go: source_mapper.go:*`

### Cargo / crate 接线

- [ ] `internal/sourcemap/Cargo.toml`（`name = "tsgo_sourcemap"` + path deps + `serde`/`serde_json`/`base64`/`rustc_hash`）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] `lib.rs`：`mod generator; mod decoder; mod lineinfo; mod source; mod util; mod source_mapper;` + `pub use`

## TDD 推进顺序（tracer bullet → 增量）

1. VLQ 编解码（`base64_format_encode/decode` + `append_base64_vlq` + `base64_vlq_format_decode`）单独建私有单测（round-trip：encode 后 decode 还原），这是命门。
2. `Generator::new` + `RawSourceMap` + `to_string`（先过 `Empty` / `Empty_Serialized` 两个测试）。
3. `add_source` / `set_source_content` / `add_name`（过对应单测，含越界错误串）。
4. `add_generated_mapping` / `add_source_mapping` / `add_named_source_mapping` + `commit_pending_mapping`（过 14 个 mapping 正例 + 错误例，逐条对齐 mappings 串如 `"AAAA,CAAC"`）。
5. `MappingsDecoder`（decode 后与 generator 输入对拍）。
6. `ECMALineInfo` / `TryGetSourceMappingURL` / `DocumentPositionMapper`（mapper 链，行为级覆盖）。

## 与 Go 的已知偏离（divergence）

- `core.Arena[Mapping]` 在 decoder 里用于复用 `*Mapping`；Rust 用 `Mapping`（Copy 值）直接返回，无需 arena（这是允许的简化，结构不变）。
- `iter.Seq` → `Iterator`：`Values()` 返回的迭代器在出错时停止（`Next` 返回 done），Rust 用 `Iterator::next` 返回 `None` 表示终止，错误经 `decoder.error()` 取出。
- VLQ 自实现（不引入 vlq crate，见上决策）。
- `RawSourceMap` 的 `omitzero`/null 占位语义：序列化必须保证空 sources/names 输出 `[]`、sourcesContent 元素 null 保留——逐字节对齐 `String()` 断言。

## 转交 / 推迟（DEFER）

- 更新 `references/crate-map.md` 的 VLQ 待定行为"自实现（P5 敲定）"——边界限制，转交（见 README）。
- `DocumentPositionMapper` 的实际 `Host` 实现来自 P6/P7（program/ls）；本包仅定义 trait + 算法，行为级测试用 fake host。
