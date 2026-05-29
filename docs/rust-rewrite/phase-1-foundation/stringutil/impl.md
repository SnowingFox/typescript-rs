# stringutil: 实现方案（impl.md）

**crate**：`tsgo_stringutil`　**目标**：提供解析/生成 JavaScript 时所需的字符/字符串底层工具（空白判定、行切分、URI 编码、BOM 处理、引号剥离、大小写比较器等）。
**依赖（crate）**：无（纯标准库 + Unicode）。这是整个移植的最叶子包之一。
**Go 源**：`internal/stringutil/`（2 个非测试文件：`util.go` 271 行、`compare.go` 129 行）

## 这个包是什么（业务说明）

`stringutil` 是 typescript-go 的"字符/字符串原语"库，被 scanner、parser、printer、checker、ls 等几乎所有上层包依赖。它分两类能力：

1. **rune/字符判定与文本工具**（`util.go`）：ECMAScript 空白与换行判定（`IsWhiteSpaceLike` / `IsLineBreak`）、数字/十六进制/八进制/ASCII 字母判定、行切分 `SplitLines`、缩进推断 `GuessIndentation`、`EncodeURI`、BOM 增删、引号剥离 `StripQuotes` / `UnquoteString`、首字母小写、按 rune 截断。
2. **字符串比较器**（`compare.go`）：大小写敏感/不敏感的相等与三路比较，外加给定 `ignoreCase` 返回相应比较函数的工厂，以及 ESLint 兼容的比较器（用于 auto-import 排序时与 eslint 的 `sort-imports` 保持一致）。

它在 Phase 1 最先落地，因为没有任何内部依赖，且是 scanner 等的前置条件。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `func(ch rune) bool` | `fn(ch: char) -> bool` | Go `rune` = Unicode code point，对应 Rust `char`；ASCII 字面量 `' '`/`'\t'` 直译 |
| `[]string`（`SplitLines` 返回） | `Vec<&str>`（借用源串）或 `Vec<String>` | Go 切片是源串的子切片（零拷贝）。Rust 优先返回 `Vec<&str>` 借用 `text`，对齐零拷贝语义；若调用方需要拥有可改 `Vec<String>` |
| `utf8.DecodeRuneInString` | `str::chars()` / `char_indices()` | 逐 rune 解码；注意 Go 对非法 UTF-8 返回 `RuneError`，Rust `&str` 已保证有效 UTF-8（见偏离） |
| `Comparison = int`（-1/0/1） | `std::cmp::Ordering` | Go 用 `int` 三态，Rust 用 `Ordering::{Less,Equal,Greater}`。导出常量 `ComparisonLessThan` 等映射到 `Ordering` 变体 |
| `func GetStringComparer(ignoreCase bool) func(a,b string) Comparison` | `fn get_string_comparer(ignore_case: bool) -> fn(&str,&str)->Ordering` | 返回函数指针；Rust 用 `fn` 指针（无捕获）即可 |
| `EncodeURI` 按字节处理 | 按 `s.as_bytes()` 逐字节 | Go 实现是**逐字节**（非逐 rune）转义，多字节 UTF-8 字符会被逐字节 `%XX`，必须照搬字节级语义 |
| `regexp.MustCompile(`\\.`)` | `regex::Regex`（`lazy_static`/`OnceLock`） | `UnquoteString` 用到；见依赖白名单 |

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/stringutil/util.go` | `internal/stringutil/util.rs`（在 `lib.rs` 里 `mod util; pub use util::*;`） | 字符判定 + 文本工具 |
| `internal/stringutil/compare.go` | `internal/stringutil/compare.rs`（`mod compare; pub use compare::*;`） | 字符串比较器族 |
| （crate 根） | `internal/stringutil/lib.rs` | `tsgo_stringutil` 入口，声明子模块并 re-export |

## 依赖白名单（本包新增的 crate）

- `regex`（`UnquoteString` 需要 `\\.` 替换；用 `OnceLock<Regex>` 懒初始化，对齐 Go 的 `regexp.MustCompile` 包级变量）。
- 其余仅标准库；Unicode 大小写转换用 `char::to_lowercase` / `str::to_lowercase`（对齐 Go `unicode.ToLower` / `strings.ToLower`）。
- 记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `util.rs`（Go: `internal/stringutil/util.go`）

- [ ] `pub fn is_white_space_like(ch: char) -> bool` — `IsWhiteSpaceSingleLine || IsLineBreak`　`// Go: util.go:IsWhiteSpaceLike`
- [ ] `pub fn is_white_space_single_line(ch: char) -> bool` — 21 个空白码点的 match（含 `0x0085` nextLine、`0xFEFF` BOM）　`// Go: util.go:IsWhiteSpaceSingleLine`
- [ ] `pub fn is_line_break(ch: char) -> bool` — `\n` `\r` `\u{2028}` `\u{2029}`　`// Go: util.go:IsLineBreak`
- [ ] `pub fn is_digit(ch: char) -> bool` — `'0'..='9'`　`// Go: util.go:IsDigit`
- [ ] `pub fn is_octal_digit(ch: char) -> bool` — `'0'..='7'`　`// Go: util.go:IsOctalDigit`
- [ ] `pub fn is_hex_digit(ch: char) -> bool` — `0-9A-Fa-f`　`// Go: util.go:IsHexDigit`
- [ ] `pub fn is_ascii_letter(ch: char) -> bool` — `A-Za-z`　`// Go: util.go:IsASCIILetter`
- [ ] `pub fn split_lines(text: &str) -> Vec<&str>` — 按 `\r\n`/`\r`/`\n` 切分；保留尾部非空段；预分配 `count('\n')+1`　`// Go: util.go:SplitLines`
- [ ] `pub fn guess_indentation(lines: &[&str]) -> usize` — 取所有非空行的最小前导空白宽度（按 rune 解码、`IsWhiteSpaceLike`）；空集或全空返回 0；命中 0 立即返回　`// Go: util.go:GuessIndentation`
- [ ] `pub fn encode_uri(s: &str) -> String` — **逐字节**百分号编码，保留未保留字符集 `;/?:@&=+$,#-_.!~*'()` 与字母数字　`// Go: util.go:EncodeURI`
- [ ] `fn should_escape_for_encode_uri(b: u8) -> bool` — 私有，配合 `encode_uri`　`// Go: util.go:shouldEscapeForEncodeURI`
- [ ] `fn get_byte_order_mark_length(text: &str) -> usize` — 私有；识别 UTF16BE(0xFEFF)/UTF16LE(0xFFFE)/UTF8(0xEFBBBF) BOM 字节，返回 0/2/3　`// Go: util.go:getByteOrderMarkLength`
- [ ] `pub fn remove_byte_order_mark(text: &str) -> &str` — 去掉 BOM 前缀　`// Go: util.go:RemoveByteOrderMark`
- [ ] `pub fn add_utf8_byte_order_mark(text: &str) -> String` — 无 BOM 时前置 `\xEF\xBB\xBF`　`// Go: util.go:AddUTF8ByteOrderMark`
- [ ] `pub fn strip_quotes(name: &str) -> &str` — 首尾同为 `'`/`"`/`` ` `` 时剥离　`// Go: util.go:StripQuotes`
- [ ] `pub fn unquote_string(s: &str) -> String` — `strip_quotes` 后用正则把 `\\.` 替换为其第二字符（注意 Go 注释指出此行为"看似有误但照搬"）　`// Go: util.go:UnquoteString`
- [ ] `pub fn lower_first_char(s: &str) -> String` — 首 rune 转小写 + 余下原样　`// Go: util.go:LowerFirstChar`
- [ ] `pub fn truncate_by_runes(s: &str, max_length: usize) -> &str` — 字节长 < max 直接返回；max<=0 返回空；否则在第 max+1 个 rune 处截断　`// Go: util.go:TruncateByRunes`

### `compare.rs`（Go: `internal/stringutil/compare.go`）

- [ ] `pub fn equate_string_case_insensitive(a: &str, b: &str) -> bool` — `str::eq_ignore_ascii_case`?**不**，Go 用 `strings.EqualFold`（Unicode 折叠），需用 Unicode 折叠等价物　`// Go: compare.go:EquateStringCaseInsensitive`
- [ ] `pub fn equate_string_case_sensitive(a: &str, b: &str) -> bool` — `a == b`　`// Go: compare.go:EquateStringCaseSensitive`
- [ ] `pub fn get_string_equality_comparer(ignore_case: bool) -> fn(&str,&str)->bool`　`// Go: compare.go:GetStringEqualityComparer`
- [ ] `pub type Comparison = Ordering`（或导出 `ComparisonLessThan/Equal/GreaterThan` 常量）　`// Go: compare.go:Comparison`
- [ ] `pub fn compare_strings_case_insensitive(a: &str, b: &str) -> Ordering` — 逐 rune 解码、`to_lower` 比较，等价时按长度收尾　`// Go: compare.go:CompareStringsCaseInsensitive`
- [ ] `pub fn compare_strings_case_sensitive(a: &str, b: &str) -> Ordering` — `a.cmp(b)`（字节序）　`// Go: compare.go:CompareStringsCaseSensitive`
- [ ] `pub fn get_string_comparer(ignore_case: bool) -> fn(&str,&str)->Ordering`　`// Go: compare.go:GetStringComparer`
- [ ] `pub fn has_prefix(s: &str, prefix: &str, case_sensitive: bool) -> bool`　`// Go: compare.go:HasPrefix`
- [ ] `pub fn has_suffix(s: &str, suffix: &str, case_sensitive: bool) -> bool`　`// Go: compare.go:HasSuffix`
- [ ] `pub fn has_prefix_and_suffix_without_overlap(s, prefix, suffix, case_sensitive) -> bool`　`// Go: compare.go:HasPrefixAndSuffixWithoutOverlap`
- [ ] `pub fn compare_strings_case_insensitive_then_sensitive(a, b) -> Ordering`　`// Go: compare.go:CompareStringsCaseInsensitiveThenSensitive`
- [ ] `pub fn compare_strings_case_insensitive_eslint_compatible(a, b) -> Ordering` — 用 `to_lowercase()` 而非大写，保持与 eslint `sort-imports` 一致　`// Go: compare.go:CompareStringsCaseInsensitiveEslintCompatible`

### Cargo / crate 接线

- [ ] `internal/stringutil/Cargo.toml`（`name = "tsgo_stringutil"`，`[dependencies] regex`）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] `lib.rs` 声明 `mod util; mod compare;` + `pub use`

## TDD 推进顺序（tracer bullet → 增量）

1. `encode_uri` + `should_escape_for_encode_uri`（唯一有 Go 单测的函数，3 个子用例直接做 red→green tracer bullet）。
2. 字符判定族（`is_*`）——纯查表，最简单，补 Rust 行为级测试。
3. `split_lines` / `guess_indentation`（行处理，被 printer 使用）。
4. BOM 族 + `strip_quotes` / `unquote_string` / `lower_first_char` / `truncate_by_runes`。
5. `compare.rs` 全族（比较器，被 collections 排序、auto-import 使用）。

## 与 Go 的已知偏离（divergence）

- **非法 UTF-8**：Go 在多个函数里用 `utf8.DecodeRuneInString` 并可能遇到 `RuneError`；Rust 的 `&str` 已保证有效 UTF-8，故这些分支在 Rust 不可达。读取阶段（vfs/scanner）若引入非法字节需在更上层处理。本包按"输入为有效 UTF-8"实现。
- **`Comparison` 类型**：Go 用 `int`(-1/0/1)，Rust 改用 `std::cmp::Ordering`。所有调用点须改成模式匹配 `Ordering`，是结构等价的允许偏离。
- **`EquateStringCaseInsensitive`**：Go 注释里保留了一行被注掉的 `ToUpper` 实现，当前用 `strings.EqualFold`。Rust 须用 Unicode 大小写折叠等价（非仅 ASCII 折叠），存疑处加 `// PERF(port)`。
- **`SplitLines` 返回切片**：Go 返回源串子切片（零拷贝）。Rust 默认返回 `Vec<&str>` 保持零拷贝；若生命周期掣肘，回退 `Vec<String>` 并标注。

## 转交 / 推迟（DEFER）

- 无跨 phase 依赖；本包可在 Phase 1 起步即完整落地。
