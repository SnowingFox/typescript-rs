# scanner: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**0 文件 / 0 `func Test*` / 0 子用例**（`internal/scanner/` 下无任何 `*_test.go`）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| （无） | `internal/scanner/lib.rs`（`#[cfg(test)] mod tests`）/ `internal/scanner/tests/*.rs` | 0（Go 侧）→ 本轮补行为级 |

## 0 直接单测的情况

- **Go 侧无直接单测**：`internal/scanner/` 不含 `*_test.go`。该包行为由 **P10 conformance/fourslash parity** 兜底——scanner 的正确性事实上被 parser→checker→emit 的端到端 baseline 全面覆盖（任何 token 切分错误都会让上层 baseline 发散）。
- 本轮按 PORTING §8.5 **补充行为级 Rust 测试**：基于公开 `scan` API + 位置换算公开函数，expected 取自 **TS/ECMAScript spec 已知值** 与 **对当前 Go 实现的实测**（执行期可用 `go test` 写个临时 harness 取 ground truth，但 expected 字面量来自 Go/spec，不靠 Rust 推断）。

### 行为级用例：`scan()` token 序列（依据：ECMAScript/TS spec + Go 实测）

> 设计：构造 `Scanner`，`set_text` 后反复 `scan()` 收集 `(token, token_text, token_value)` 序列直到 `KindEndOfFile`，与期望序列比对。`skip_trivia=true`（默认）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `scan_punctuation_singletons` | 单字符标点 Kind | `"( ) { } [ ] ; , ."` → `[OpenParen,CloseParen,OpenBrace,CloseBrace,OpenBracket,CloseBracket,Semicolon,Comma,Dot,EOF]` | spec | |
| `scan_compound_operators` | 多字符运算符最长匹配 | `"=> === !== >>> ?. ?? ??= **="` → `[EqualsGreaterThan,EqualsEqualsEquals,ExclamationEqualsEquals,GreaterThanGreaterThanGreaterThan,QuestionDot,QuestionQuestion,QuestionQuestionEquals,AsteriskAsteriskEquals,EOF]` | spec | |
| `scan_keyword_vs_identifier` | 关键字表命中 / 不命中 | `"let x const yield foo"` → `[LetKeyword,Identifier(x),ConstKeyword,YieldKeyword,Identifier(foo),EOF]` | `GetIdentifierToken` | |
| `scan_identifier_dollar_underscore` | `$`/`_` 合法标识符 | `"$_a _ $123"` → `[Identifier($_a),Identifier(_),Identifier($123),EOF]`（`$123` 起始 `$` 合法） | spec | |
| `scan_numeric_literals` | 数字 token + `tokenValue` 去分隔符 | `"0 0x1F 0b1010 0o17 1_000 3.14 1e3 5n"` → kinds 均 `NumericLiteral`/末位 `BigIntLiteral`，value 规范化（`1_000`→`1000`，`0x1F`→…） | spec + Go 实测 | |
| `scan_string_literal_value` | 字符串值与引号 flag | 源含双引号串 `"a\n"` → `StringLiteral` value=`a`+换行(已解转义)；源含单引号串 `'b'` → 置 `TokenFlagsSingleQuote` | spec | |
| `scan_template_head_tail` | 模板拆段 | `` "`a${" `` → `TemplateHead`；`` "}b`" `` 经 `re_scan_template_token` → `TemplateTail` | spec | |
| `scan_trivia_skipped_default` | 默认跳 trivia | `"  \n\t a"` → `[Identifier(a),EOF]`，且 `has_preceding_line_break()==true` | Go 实测 | |
| `scan_trivia_emitted_when_disabled` | `set_skip_trivia(false)` 产 trivia token | `" \n//c\n a"` → `[WhitespaceTrivia,NewLineTrivia,SingleLineCommentTrivia,...,Identifier(a),EOF]` | Go 实测 | |

### 行为级用例：rescan 家族（依据：TS parser 上下文语义）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `rescan_greater_than_splits` | `>>` 在类型参数上下文回扫为单 `>` | scan `">>"` 得 `GreaterThanGreaterThan`；`re_scan_greater_than_token()` → `GreaterThan`，`token_end` 后退 1 | `ReScanGreaterThanToken` | |
| `rescan_slash_to_regex` | `/` 回扫为正则字面量 | text `"/ab/g"` 起点 scan 得 `Slash`；`re_scan_slash_token()` → `RegularExpressionLiteral`，value=`/ab/g` | `ReScanSlashToken` | |
| `rescan_less_than_jsx` | JSX 上下文 `re_scan_jsx_token` | （variant=JSX）`"<div>"` 序列起始正确切分 | `ReScanJsxToken` | |

### 行为级用例：标识符判定 / token 文本（纯函数）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `is_identifier_start_ascii` | ASCII 起始判定 | `'a'`/`'_'`/`'$'`→true，`'1'`/`' '`→false | `IsIdentifierStart` | |
| `is_identifier_part_jsx_dash` | JSX variant 允许 `-`/`:` | `is_identifier_part_ex('-', JSX)`→true，`(.., Standard)`→false | `IsIdentifierPartEx` | |
| `is_in_unicode_ranges_binary_search` | unicode 区间二分 | 码点 `0x00DF`(ß,start)→true、`0x0040`(@)→false、星形面标识符码点→按表 | `isInUnicodeRanges` | |
| `is_valid_identifier_full` | 整串校验 | `"foo"`→true，`"1foo"`→false，`""`→false，`"a-b"`(Standard)→false | `IsValidIdentifier` | |
| `get_identifier_token_bounds` | 仅 2..=12 长度首字母小写查表 | `"let"`→LetKeyword，`"Let"`→Identifier，`"abstractx"`(非词)→Identifier | `GetIdentifierToken` | |
| `token_to_string_roundtrip` | `string_to_token`/`token_to_string` 对偶 | `token_to_string(PlusToken)=="+"`；`string_to_token("+")==PlusToken`；未知 → `KindUnknown` | `TokenToString/StringToToken` | |

### 行为级用例：UTF-16 / 行列换算（命门，依据：spec + Go 实测）

> 这些是移植最易错处，必须独立、密集测试。emoji `😀`(U+1F600) 是星形面：UTF-8 4 字节、UTF-16 2 码元。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `utf16_len_ascii_fastpath` | ASCII 段 byte==UTF-16 | `utf16_len("abc")==3` | `core::UTF16Len` | |
| `utf16_len_bmp_and_astral` | BMP=1、星形面=2 码元 | `utf16_len("é")==1`，`utf16_len("😀")==2`，`utf16_len("a😀b")==4` | spec | |
| `compute_line_of_position_binary` | 行号二分 | lineStarts=`[0,5,9]`,pos=7 → line 1 | `ComputeLineOfPosition` | |
| `line_and_utf16_char_with_astral` | 含星形面的列号（UTF-16 计） | text=`"a😀b"`(单行)，pos=byte 5(b 前) → (line 0, character 3)（a=1+😀=2） | `GetECMALineAndUTF16CharacterOfPosition` | |
| `line_and_byte_offset_with_astral` | 同位置的**字节**列号 | text=`"a😀b"`,pos=5 → (line 0, byteOffset 5) | `GetECMALineAndByteOffsetOfPosition` | |
| `position_of_line_and_utf16_char_roundtrip` | UTF-16 列→byte 反查 | text=`"a😀b"`,(line0,char3) → byte 5；(line0,char1)→byte1 | `ComputePositionOfLineAndUTF16Character` | |
| `position_of_line_and_utf16_char_clamp` | `allow_edits` 越界钳制 vs panic | char 越界：`allow_edits=true`→`len(text)`；`false`→panic | `ComputePositionOfLineAndUTF16Character` | |
| `contains_non_ascii_flag_set` | 扫到多字节置位 | scan `"a😀"` 后 `contains_non_ascii()==true`；scan `"ab"` 后 false | `ContainsNonASCII` | |

### 行为级用例：trivia / 注释 / shebang

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `skip_trivia_basic` | 跳过空白/注释到首 token | `skip_trivia("  /*c*/ x", 0)` → x 的字节偏移 | `SkipTrivia` | |
| `skip_trivia_stop_at_comments` | `StopAtComments` 选项 | `skip_trivia_ex(" //c\n x", opts{stop_at_comments})` 停在注释前 | `SkipTriviaEx` | |
| `shebang_detected_at_zero` | 仅位置 0 识别 `#!` | `get_shebang("#!/usr/bin/env node\n")` → `"#!/usr/bin/env node"` | `GetShebang` | |
| `leading_comment_ranges` | 收集前导注释区间 | `"/*a*//*b*/x"` → 2 个 MultiLineComment 区间，kind/pos/end 正确 | `GetLeadingCommentRanges` | |
| `conflict_marker_trivia` | `<<<<<<<` 冲突标记识别 | `is_conflict_marker_trivia("<<<<<<< HEAD",0)`→true | `isConflictMarkerTrivia` | |

## 与 impl.md 的对齐核对

- [ ] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/scanner/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）

- [ ] 每个 Go `func Test*` 都已映射 → **N/A：Go 侧 0 个 `Test*`**（已声明 P10 兜底）
- [ ] 每个表驱动子用例都已逐行列出 → N/A（同上）
- [ ] 行为级用例的 expected 取自 **spec / Go 实测**（非 Rust 推断）
- [ ] 每条用例对应 impl.md 的实现 TODO（`scan`/`re_scan_*`/`is_identifier_*`/UTF-16 换算族/trivia 族）均存在承载
- [ ] 与 impl.md 双向对齐：UTF-16/行列换算、rescan、标识符表是两文档共同强调的命门

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 全量 token 切分一致性（所有 conformance 源） | 需 testdata 294MB + 端到端对拍 | P10 |
| 正则字面量诊断文本逐条对齐 | 诊断消息与 fourslash 关联 | P10 |
| JSDoc token 在真实 `.js` 上的切分 | 依赖 parser 的 JSDoc 流程 | P10（经 parser baseline） |
| `ErrorCallback` 与 parser 协同的诊断位置 | 接口在 parser 阶段定 | P3-parser / P10 |
