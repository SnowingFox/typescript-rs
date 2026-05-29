# diagnostics: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 文件 / 2 顶层 `func Test` / 15 子用例（`TestLocalize` 13 + `TestLocalize_ByKey` 2，均表驱动）。

> Go: `internal/diagnostics/diagnostics_test.go`。两个表驱动测试，子用例经 `t.Run(tt.name, ...)`。expected 全部抄自 Go 字面量（含各语言 UTF-8 译文）。这些译文 ground truth 取自嵌入的 `loc/*.json.gz`，是验证"locale 匹配 + 解压 + 占位符格式化"链路的硬断言。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/diagnostics/diagnostics_test.go` | `internal/diagnostics/diagnostics.rs`（`#[cfg(test)] mod tests`） | 2 |

## `diagnostics_test.go` → `TestLocalize`

> Go: `func TestLocalize`（表驱动，12 子用例）。调用 `tt.message.Localize(tt.locale, tt.args...)`，断言等于 `expected`。
> Rust：`message.localize(locale, &args)`。建议 `rstest` 参数化，或逐 case `#[test]`。

| Rust 测试 | 验证内容 | input（message / locale / args） → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `localize_english_default` | 英语直接取 `text` | `IDENTIFIER_EXPECTED` / `English` / — → `"Identifier expected."` | `TestLocalize/english default` | ✓ |
| `localize_undefined_locale_uses_english` | `Und` 回落英语 | `IDENTIFIER_EXPECTED` / `Und` / — → `"Identifier expected."` | `TestLocalize/undefined locale uses english` | ✓ |
| `localize_with_single_argument` | 单参 `{0}` 替换 | `X_0_EXPECTED` / `English` / `[")"]` → `"')' expected."` | `TestLocalize/with single argument` | ✓ |
| `localize_with_multiple_arguments` | 多参 `{0}`/`{1}` 替换 | `THE_PARSER_EXPECTED_TO_FIND_A_1_TO_MATCH_THE_0_TOKEN_HERE` / `English` / `["{","}"]` → `"The parser expected to find a '}' to match the '{' token here."` | `TestLocalize/with multiple arguments` | ✓ |
| `localize_fallback_to_english_for_unknown_locale` | 未支持 locale 回落英语 | `IDENTIFIER_EXPECTED` / `af-ZA` / — → `"Identifier expected."` | `TestLocalize/fallback to english for unknown locale` | ✓ |
| `localize_german` | 德语译文命中 | `IDENTIFIER_EXPECTED` / `de-DE` / — → `"Es wurde ein Bezeichner erwartet."` | `TestLocalize/german` | ✓ |
| `localize_french` | 法语译文命中 | `IDENTIFIER_EXPECTED` / `fr-FR` / — → `"Identificateur attendu."` | `TestLocalize/french` | ✓ |
| `localize_spanish` | 西语译文命中 | `IDENTIFIER_EXPECTED` / `es-ES` / — → `"Se esperaba un identificador."` | `TestLocalize/spanish` | ✓ |
| `localize_japanese` | 日语译文命中 | `IDENTIFIER_EXPECTED` / `ja-JP` / — → `"識別子が必要です。"` | `TestLocalize/japanese` | ✓ |
| `localize_chinese_simplified` | 简中译文命中 | `IDENTIFIER_EXPECTED` / `zh-CN` / — → `"应为标识符。"` | `TestLocalize/chinese simplified` | ✓ |
| `localize_korean` | 韩语译文命中 | `IDENTIFIER_EXPECTED` / `ko-KR` / — → `"식별자가 필요합니다."` | `TestLocalize/korean` | ✓ |
| `localize_russian` | 俄语译文命中 | `IDENTIFIER_EXPECTED` / `ru-RU` / — → `"Ожидался идентификатор."` | `TestLocalize/russian` | ✓ |
| `localize_german_with_args` | 德语译文 + 占位符替换 | `X_0_EXPECTED` / `de-DE` / `[")"]` → `"\")\" wurde erwartet."` | `TestLocalize/german with args` | ✓ |

> 注：Go 表里共 13 行（上表 13 条）。`german with args` 的 expected 含转义双引号 `"\")\" wurde erwartet."`，即德语模板用 `"{0}"`（双引号）而非英语的 `'{0}'`（单引号），是 locale 模板差异的关键断言，须逐字节核对。

## `diagnostics_test.go` → `TestLocalize_ByKey`

> Go: `func TestLocalize_ByKey`（表驱动，2 子用例）。调用 `Localize(tt.locale, nil, tt.key, tt.args...)`（message 传 nil，强制走 `keyToMessage(key)`）。
> Rust：`localize(locale, None, key, &args)`。

| Rust 测试 | 验证内容 | input（key / locale / args） → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `localize_by_key_without_args` | 仅凭 key 查表取英语 | `"Identifier_expected_1003"` / `English` / — → `"Identifier expected."` | `TestLocalize_ByKey/by key without args` | ✓ |
| `localize_by_key_with_args` | 仅凭 key 查表 + 单参替换 | `"_0_expected_1005"` / `English` / `[")"]` → `"')' expected."` | `TestLocalize_ByKey/by key with args` | ✓ |

> 注：`key_to_message` 用的 key（`Identifier_expected_1003` / `_0_expected_1005`）是 Go 生成器 `convertPropertyName` 的产物，须与生成的 `Key` 字面量逐字节一致（不随 Rust static 变量名改写）。

## 0 直接单测的情况（补充行为级 Rust 测试）

Go 单测只覆盖 `Localize` 链路；以下函数无直接单测，按 PORTING §8 补行为级测试（expected 取自 Go 实现的确定逻辑）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `format_no_args_short_circuit` | 无参直接返回原串 | `format("a{0}b", &[])` → `"a{0}b"` | diagnostics.go:Format | ✓ |
| `format_invalid_placeholder_panics` | 占位符越界 panic | `format("{1}", &["x"])` → panic `"Invalid formatting placeholder"` | diagnostics.go:Format | ✓ |
| `format_invalid_utf8_args_sanitized` | 参数非法 UTF-8 → U+FFFD | 含非法字节的 arg → 替换为 `\u{FFFD}` | diagnostics.go:Format（`ToValidUTF8`） | ✓ |
| `category_name_mapping` | 4 个分类名 | `Warning→"warning"`,`Error→"error"`,`Suggestion→"suggestion"`,`Message→"message"` | diagnostics.go:Category.Name | ✓ |
| `category_repr_values` | 数值与 Go iota 一致 | `Warning=0,Error=1,Suggestion=2,Message=3` | diagnostics.go:Category | ✓ |
| `stringify_args_mixed` | 字符串原样 / 其他 `Display` | `["x", 42]` → `["x","42"]`；`[]`→`[]` | diagnostics.go:StringifyArgs | ✓ |
| `key_to_message_unknown_returns_none` | 未知 key → None | `key_to_message("nope")` → None | diagnostics_generated.go:keyToMessage | ✓ |
| `localize_unknown_key_panics` | message=None 且 key 未知 → panic | `localize(English, None, "nope", &[])` → panic `"Unknown diagnostic message: nope"` | diagnostics.go:Localize | ✓ |

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（`TestLocalize` 13 行 + `TestLocalize_ByKey` 2 行）
- [x] 每个表驱动子用例都已逐行列出（含各语言译文 expected）
- [x] expected 值均取自 Go 测试字面量（逐字节抄录，含 CJK/西里尔/转义引号）
- [x] 每条带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐：`localize`/`format`/`key_to_message`/`Category`/`stringify_args` 在 impl.md 均有实现 TODO 承载

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 任意 BCP-47 locale 的 CLDR 最近匹配与 Go `x/text/language` 完全一致 | P2 用精简匹配，完整匹配待选 locale crate | P10 parity |
| 全 2153 条消息的 code/category/text 与 TS 子模块逐条一致 | 需生成器全量产出后对拍 | P10 parity（生成器自校验） |
| 真实编译路径上诊断文案与 `tsc` 输出一致 | 需 parser/checker 落地 | P10 conformance |
