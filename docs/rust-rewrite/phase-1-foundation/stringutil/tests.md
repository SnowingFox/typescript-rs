# stringutil: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 文件 / 1 顶层 `func Test` / 3 子用例（表驱动）。

> ⚠️ 本包 Go 侧只有 `TestEncodeURI` 一个表驱动测试（3 子用例），其余 30+ 个公开函数**无直接单测**，行为由 P10 conformance/fourslash parity 兜底。本轮按 PORTING §8 补充行为级 Rust 测试覆盖关键路径，expected 取自 Go 实现可推导的确定值 / TS spec 已知值。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/stringutil/util_test.go` | `internal/stringutil/util.rs`（`#[cfg(test)] mod tests`） | 1 |

## `util_test.go`

> Go: `func TestEncodeURI`（表驱动，子用例 `t.Run(tt.name, ...)`）。逐子用例列。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `encode_uri_encodes_spaces_as_percent20` | 空格编码为 `%20` | `"a b"` → `"a%20b"` | `util_test.go:TestEncodeURI/encodes spaces as percent20` | |
| `encode_uri_preserves_reserved_uri_characters` | 保留字符不转义 | `";/?:@&=+$,#"` → `";/?:@&=+$,#"` | `util_test.go:TestEncodeURI/preserves reserved uri characters` | |
| `encode_uri_encodes_brackets_and_unicode_using_utf8_bytes` | 非 ASCII 按 UTF-8 字节逐个 `%XX`，`[` `]` 转义 | `"①Ⅻㄨㄩ U1[abc]"` → `"%E2%91%A0%E2%85%AB%E3%84%A8%E3%84%A9%20U1%5Babc%5D"` | `util_test.go:TestEncodeURI/encodes brackets and unicode using utf8 bytes` | |

## 0 直接单测的情况（补充行为级 Rust 测试）

Go 侧除 `EncodeURI` 外无直接单测；以下函数行为由 **P10 conformance/fourslash parity** 兜底。本轮补充行为级 Rust 测试（expected 取自 Go 实现逻辑 / ECMAScript 定义的确定值）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `is_white_space_like_basics` | 常见空白/换行判定 | `' '`→true、`'\t'`→true、`'\n'`→true、`'a'`→false、`'\u{FEFF}'`→true | util.go:IsWhiteSpaceLike | |
| `is_line_break_set` | 仅 4 个换行码点为真 | `'\n'`/`'\r'`/`'\u{2028}'`/`'\u{2029}'`→true、`' '`→false | util.go:IsLineBreak | |
| `is_digit_octal_hex_ascii` | 数字/八进制/十六进制/字母判定 | `'7'`octal→true、`'8'`octal→false、`'f'`hex→true、`'g'`hex→false、`'Z'`ascii→true | util.go:IsDigit/IsOctalDigit/IsHexDigit/IsASCIILetter | |
| `split_lines_crlf_lf_cr` | 三种换行混合切分 | `"a\r\nb\nc\rd"` → `["a","b","c","d"]` | util.go:SplitLines | |
| `split_lines_trailing` | 末尾非空段保留、末尾换行不产空段尾随逻辑 | `"a\n"` → `["a"]`；`"a\nb"` → `["a","b"]` | util.go:SplitLines | |
| `guess_indentation_min` | 取非空行最小缩进 | `["  a","    b","   c"]` → `2`；含空行被跳过 | util.go:GuessIndentation | |
| `guess_indentation_zero_and_empty` | 出现 0 缩进 / 全空 → 0 | `["a"]`→0；`[]`→0；`["",""]`→0 | util.go:GuessIndentation | |
| `remove_byte_order_mark_utf8` | 去 UTF-8 BOM | `"\u{FEFF}abc"`（UTF-8 BOM 字节）→ `"abc"`；无 BOM 原样 | util.go:RemoveByteOrderMark | |
| `add_utf8_byte_order_mark` | 无 BOM 时前置 | `"abc"` → `"\xEF\xBB\xBFabc"`；已有 BOM 原样 | util.go:AddUTF8ByteOrderMark | |
| `strip_quotes_pairs` | 配对引号剥离 | `"\"x\""`→`"x"`、`"'x'"`→`"x"`、`` "`x`" ``→`"x"`、`"x"`(无引号)→`"x"`、`"\""`(len<2)→`"\""` | util.go:StripQuotes | |
| `unquote_string_backslash` | 剥引号 + `\\.`→第二字符 | `"\"a\\nb\""` → `"anb"`（`\n` 还原成 `n`，照搬 Go 行为） | util.go:UnquoteString | |
| `lower_first_char` | 首字母小写 | `"Foo"`→`"foo"`、`""`→`""`、`"Ünder"`→`"ünder"` | util.go:LowerFirstChar | |
| `truncate_by_runes_basic` | 按 rune 截断 | `("hello",3)`→`"hel"`、`("hi",5)`→`"hi"`、`("x",0)`→`""` | util.go:TruncateByRunes | |
| `compare_case_sensitive_order` | 字节序三路比较 | `("a","b")`→Less、`("b","a")`→Greater、`("a","a")`→Equal | compare.go:CompareStringsCaseSensitive | |
| `compare_case_insensitive_order` | 大小写不敏感 | `("ABC","abc")`→Equal、`("a","B")`→Less | compare.go:CompareStringsCaseInsensitive | |
| `has_prefix_suffix_casing` | 前/后缀大小写敏感与否 | `has_prefix("Foo","fo",false)`→true、`(...,true)`→false；`has_suffix` 同理 | compare.go:HasPrefix/HasSuffix | |
| `eslint_compatible_lowercase_order` | ESLint 兼容用小写折叠 | `("__String","Foo")` 的相对序与 `to_lower` 后 `cmp` 一致 | compare.go:CompareStringsCaseInsensitiveEslintCompatible | |

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（仅 `TestEncodeURI`，3 子用例全列）
- [x] 每个表驱动子用例都已逐行列出
- [x] expected 值均取自 Go 测试字面量（`TestEncodeURI` 三行直接抄）
- [x] 每条带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐无遗漏（补充测试覆盖的函数在 impl.md 均有 TODO 承载）

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `unquote_string` / `encode_uri` 与真实 TS 源在 scanner/printer 路径的端到端一致 | 需 scanner/printer 落地 | P10 parity |
| Unicode 大小写折叠边界（土耳其语 I 等）与 Go `EqualFold` 完全一致 | 需大规模 conformance 语料 | P10 parity |
