# scanner: 实现方案（impl.md）

**crate**：`tsgo_scanner`　**目标**：把 TypeScript/JavaScript 源文本（UTF-8 字符串）切成 token 流（`ast.Kind` + 值 + flags + 位置），并提供位置/行号/UTF-16 换算、trivia 跳过、注释扫描等编译器全程依赖的词法工具。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_debug` `tsgo_diagnostics` `tsgo_jsnum` `tsgo_stringutil` `tsgo_collections`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/scanner/`（4 个非测试文件：`scanner.go` 100KB / `regexp.go` 34KB / `unicodeproperties.go` 7KB / `utilities.go` 3KB）

## 这个包是什么（业务说明）

scanner 是编译器/语言服务的**词法分析层**，处于 `ast`（P2）之上、`parser`（P3）之下。parser 通过持有一个 `*Scanner`、反复调用 `Scan()` 拿到下一个 token 来构建 AST；checker、printer、语言服务（如 astnav）也会借用 scanner 做"在某位置重新扫描一个 token"之类的查询。

scanner 的核心是一台**手写状态机**（`Scan()` 里一个巨大的 `switch ch`）：从 `pos` 处读取当前字节/码点，识别出标点、运算符、关键字/标识符、数字字面量、字符串/模板、正则、JSX 文本、JSDoc token 等，把结果写进 `ScannerState`（`token`/`tokenValue`/`tokenFlags`/`tokenStart`/`pos` 等）。它还负责一组**回扫（rescan）**：同一段文本在不同语法上下文里要重新解释（如 `>` vs `>>` vs `>=`、`/` vs 正则、模板续段、JSX），parser 在已知上下文后调用 `ReScanXxx`。

scanner 另一大职责是**位置语义**。Go 侧源文本是 **UTF-8 字节串**，`pos` 是字节偏移；但 TypeScript 对外（LSP、诊断列号、sourcemap）用 **UTF-16 码元**计数。因此 scanner 提供 byte↔UTF-16↔(line,character) 三方换算（`UTF16Len`、`ECMALineMap`、`GetECMALineAndUTF16CharacterOfPosition`、`ComputePositionOfLineAndUTF16Character` 等），并用 `containsNonASCII` 标志快速判断"byte 偏移是否等于 UTF-16 偏移"以走快路径。这是本包移植**最易出错、必须 1:1 对齐**的部分。

为什么在 P3：scanner 只依赖 P1 地基（core/stringutil/jsnum/...）与 P2 的 `ast`（token 的 `Kind`、`TokenFlags`、`CommentDirective`、`SourceFile`），是 parser 的直接前置，故排在 P3 第一个。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type Scanner struct { ... ScannerState }`（内嵌） | `struct Scanner { state: ScannerState, /* 其余字段 */ }` + 委托方法 | Go 用结构体内嵌把 `ScannerState` 字段提升到 `Scanner`；Rust 无内嵌，用组合 + 在 `Scanner` 上写转发访问器（`token()`/`pos()`…）。`Mark()`/`Rewind()` 直接拷贝/赋值 `state` 字段。 |
| `ScannerState`（值语义、可拷贝） | `#[derive(Clone, Copy, Default)] struct ScannerState` | `Mark()` 返回拷贝、`Rewind(state)` 整体覆盖——是 parser 的 lookahead/speculation 命门，必须是廉价 `Copy`。`commentDirectives []ast.CommentDirective` 阻碍 `Copy`，故见下条偏离。 |
| `commentDirectives []ast.CommentDirective`（在 ScannerState 内） | 移出 `ScannerState`，放 `Scanner` 顶层 `Vec<CommentDirective>` | Go 里它在可拷贝的 `ScannerState`，但 mark/rewind 时实际共享底层数组。Rust 为保 `ScannerState: Copy`，把它提到 `Scanner`，并在 rewind 时**按长度截断**恢复（记录 `comment_directives_len` 进 `ScannerState`）。属允许偏离，须注释。 |
| `pos int`（字节偏移） | `pos: usize`（或 `i32` 对齐 Go `int`） | **全包统一**：`pos`/`tokenStart`/`fullStartPos`/`end` 都是 **UTF-8 字节偏移**，不是 UTF-16。Go 用 `int`；移植用 `i32` 对齐 Go 截断语义（位置上限远小于 2^31），用 `// PERF(port)` 标注若改 `usize`。 |
| `s.char() rune`（只解 1 字节） | `fn char(&self) -> i32`：`if pos<end { text.as_bytes()[pos] as i32 } else { -1 }` | **关键**：`char()` 故意只看单字节，返回 `-1` 表示 EOF。调用方靠 `< utf8.RuneSelf(0x80)` 判断是否需要 `char_and_size()` 完整解码。Rust 用 `as_bytes()[pos]`，不要用 `chars()`。 |
| `s.charAndSize() (rune, int)` = `utf8.DecodeRuneInString` | `fn char_and_size(&mut self) -> (char, usize)`：用 `text[pos..].chars().next()` + `len_utf8()`；解码出多字节时置 `contains_non_ascii=true` | Go 的 `utf8.DecodeRuneInString` 对非法字节返回 `(RuneError, 1)`；Rust `str` 已保证合法 UTF-8，对应行为需在"非 str 输入"边界考虑（见偏离）。 |
| `rune`（i32 码点） | `char`（标量值）/ `i32`（哨兵 -1） | 标识符范围判断（`isInUnicodeRanges`）里码点比较用 `i32`/`u32`；EOF 哨兵 `-1` 不能用 `char` 表达，故 `char()` 返回 `i32`。 |
| `containsNonASCII bool` | `contains_non_ascii: bool` | byte==UTF-16 偏移的快路径开关；任何一次 `char_and_size` 解出 size>1 即置位。位置换算据此决定走"直接相减"还是"逐码点累计 UTF-16"。 |
| `numberCache/hexNumberCache/hexDigitCache map[string]string` | `FxHashMap<String,String>` | 纯性能缓存（去分隔符后的规范化数字串）；`Reset()` 复用底层 map（`clear` 保容量）。Rust 用 `HashMap::clear()` 同义。 |
| `EscapeSequenceScanningFlags int32` / `ParseFlags` 类位枚举 | `bitflags!` | 见 PORTING §3 flags 规则。 |
| `ErrorCallback func(...)` | `Option<Box<dyn FnMut(&Message, i32, i32, &[Arg])>>` 或泛型回调 | parser 注入 `scanError`；Rust 用闭包字段。注意回调期间需可变借用 scanner 之外的 parser 状态——执行期可用 `RefCell`/索引回调，本文档标 `// TODO(port)` 由 parser 阶段定接口。 |
| `[]core.TextPos`（行起始表） | `&[TextPos]` / `Vec<TextPos>`，`TextPos=i32` | `ECMALineMap()` 来自 `ast.SourceFile`（P2 已建），scanner 只读。 |
| `unicodeESNextIdentifierStart/Part []rune`（成对区间表） | `static IDENT_START: &[i32]` / `&[i32]`（`&'static`） | 巨型常量表，直译为 `&'static [i32]`；`isInUnicodeRanges` 是成对区间二分查找，逐行 1:1。 |

### 位置/UTF-16 所有权说明（命门）

- **不可变量**：scanner 内部一切偏移都是 **UTF-8 字节**。仅在与外界（LSP、诊断列、sourcemap）交换时换算成 UTF-16。
- byte→UTF-16：`core::utf16_len(&text[a..b])`——ASCII 段直接 `len`，遇非 ASCII 逐码点 `utf16::RuneLen`（BMP=1，星形面=2）。
- (line,character)：`ECMALineMap`（`SourceFile` 持有，0 基行起始字节偏移数组）+ `compute_line_of_position`（二分）+ 段内 `utf16_len`。
- UTF-16→byte：`compute_position_of_line_and_utf16_character` 从行首逐码点累计 UTF-16 计数直到达到目标，返回字节 `pos`；`allow_edits` 控制越界 panic 还是钳制。
- 这些函数**纯函数**（输入 text+lineStarts，无副作用），优先做成自由函数，便于 doctest。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/scanner/scanner.go` | `internal/scanner/lib.rs`（crate 根，basename==目录名） | `Scanner`/`ScannerState`、`Scan()` 状态机、全部 `ReScan*`/`ScanJsx*`/`ScanJSDoc*`、标识符/数字/字符串/模板/转义扫描、`textToKeyword`/`textToToken`、标识符 unicode 区间表、trivia/注释/行号/UTF-16 位置工具。`mod regexp; mod unicodeproperties; mod utilities;` + 公开 re-export。 |
| `internal/scanner/regexp.go` | `internal/scanner/regexp.rs` | 正则字面量校验器 `regExpParser`（在 `ReScanSlashToken` 触发 `/.../flags` 校验时用），扫描 disjunction/alternative/class/escape/group name/unicode property 等并发诊断。 |
| `internal/scanner/unicodeproperties.go` | `internal/scanner/unicodeproperties.rs` | 正则 `\p{...}` 的 Unicode 属性名/值白名单（`nonBinaryUnicodeProperties`/`binaryUnicodeProperties`/`scriptValues` 等）。用 `FxHashSet<&'static str>`/`FxHashMap` 直译。 |
| `internal/scanner/utilities.go` | `internal/scanner/utilities.rs` | 代理对/CESU-8 哨兵编解码（`encodeSurrogate`/`decodeClassAtomRune`）、`IdentifierToKeywordKind`、`GetTextOfNode*`、`DeclarationNameToString`、`IsIdentifierText`、`IsIntrinsicJsxName`。 |

## 依赖白名单（本包新增的 crate）

- `rustc_hash`（`FxHashMap`/`FxHashSet`）——关键字表、unicode 属性集、数字缓存。
- 标准库 `char`/`str` 自带 UTF-8 解码；UTF-16 计数用 `char::len_utf16()`（等价 Go `utf16.RuneLen`）。
- 无需额外正则引擎：`regexp.rs` 是**自写校验器**，不依赖 `regex` crate（只做语法校验+诊断，不做匹配）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按 TDD 推进序：先建状态与位置语义，再 `Scan()` 主路径，再 rescan/jsx/jsdoc，最后 regexp 校验。

### `lib.rs` — 状态与访问器（Go: `scanner.go`）

- [ ] `struct ScannerState { pos,fullStartPos,tokenStart: i32, token: Kind, tokenValue: String, tokenFlags: TokenFlags, commentDirectivesLen: usize, skipJSDocLeadingAsterisks: i32 }`（`Clone+Copy+Default`，注意 `tokenValue` 若为 `String` 则不能 `Copy`——执行期改为存 `Range<i32>`/小串或把 value 移出 state，见偏离）　`// Go: scanner.go:ScannerState`
- [ ] `struct Scanner { text:String, end:i32, language_variant, script_target, on_error, skip_trivia:bool, state:ScannerState, contains_non_ascii:bool, comment_directives:Vec<CommentDirective>, number_cache, hex_number_cache, hex_digit_cache }`　`// Go: scanner.go:Scanner`
- [ ] `pub fn new() -> Scanner`（`skip_trivia=true`）　`// Go: scanner.go:NewScanner`
- [ ] `pub fn reset(&mut self)`（保留三个缓存 map，其余清零）　`// Go: scanner.go:Reset`
- [ ] 访问器族：`text/token/token_flags/token_full_start/token_start/token_end/token_text/token_value/token_range/comment_directives/contains_non_ascii`　`// Go: scanner.go:Token..TokenRange`
- [ ] `pub fn mark(&self) -> ScannerState` / `pub fn rewind(&mut self, s: ScannerState)`　`// Go: scanner.go:Mark/Rewind`
- [ ] `reset_pos/reset_token_state`、`set_text/set_on_error/set_language_variant/set_script_target/set_skip_trivia/set_skip_jsdoc_leading_asterisks`　`// Go: scanner.go:ResetPos..SetSkipTrivia`
- [ ] flag 查询族：`has_unicode_escape/has_extended_unicode_escape/has_preceding_line_break/has_preceding_jsdoc_comment/...`　`// Go: scanner.go:HasUnicodeEscape..HasPrecedingJSDocWithSeeOrLink`
- [ ] `fn char(&self)->i32` / `fn char_at(&self,off:i32)->i32` / `fn char_and_size(&mut self)->(char,usize)`（解多字节置 `contains_non_ascii`）　`// Go: scanner.go:char/charAt/charAndSize`

### `lib.rs` — 主扫描状态机（Go: `scanner.go`）

- [ ] `pub fn scan(&mut self) -> Kind` — 巨型 `match ch`：空白/换行 trivia、`! % & * + - . / : < = > ? ^ | ~ ( ) [ ] { } , ; @ #` 全部运算符/标点分支（逐字符前看 `char_at(1)/char_at(2)`）、字符串/模板/数字/标识符/EOF、`<<conflict marker>>`、shebang、注释指令收集。返回值写 `state.token`。**逐分支 1:1**　`// Go: scanner.go:Scan`
- [ ] `fn process_comment_directive(...)`（`@ts-ignore`/`@ts-expect-error` 收集进 `comment_directives`）　`// Go: scanner.go:processCommentDirective`
- [ ] `fn scan_identifier(&mut self, prefix_len:i32)->bool`（ASCII 快路径 + unicode 慢路径 + `\u` 转义续接）　`// Go: scanner.go:scanIdentifier`
- [ ] `fn scan_identifier_parts(&mut self)->String`　`// Go: scanner.go:scanIdentifierParts`
- [ ] `fn scan_string(&mut self, jsx_attr:bool)->String`　`// Go: scanner.go:scanString`
- [ ] `fn scan_template_and_set_token_value(...)->Kind`　`// Go: scanner.go:scanTemplateAndSetTokenValue`
- [ ] `fn scan_escape_sequence(flags)->String` / `scan_unicode_escape` / `peek_unicode_escape`　`// Go: scanner.go:scanEscapeSequence/scanUnicodeEscape/peekUnicodeEscape`
- [ ] 数字族：`scan_number/scan_number_fragment/scan_digits/scan_hex_digits/scan_binary_or_octal_digits/scan_big_int_suffix`（含 `_` 分隔符、缓存）　`// Go: scanner.go:scanNumber..scanBigIntSuffix`
- [ ] `fn scan_invalid_character(&mut self)`　`// Go: scanner.go:scanInvalidCharacter`

### `lib.rs` — rescan / JSX / JSDoc（Go: `scanner.go`）

- [ ] `re_scan_less_than_token/re_scan_greater_than_token/re_scan_asterisk_equals_token/re_scan_slash_token/re_scan_template_token/re_scan_hash_token/re_scan_question_token/re_scan_jsx_token`　`// Go: scanner.go:ReScan*`
- [ ] `scan_jsx_token/scan_jsx_token_ex/scan_jsx_identifier/scan_jsx_attribute_value/re_scan_jsx_attribute_value`　`// Go: scanner.go:ScanJsx*`
- [ ] `scan_jsdoc_token/scan_jsdoc_comment_text_token/can_follow_jsdoc_at/scan_jsdoc_comment_for_tags`　`// Go: scanner.go:ScanJSDoc*`

### `lib.rs` — 标识符判定 / token 文本 / trivia（Go: `scanner.go`）

- [ ] `pub fn get_identifier_token(s:&str)->Kind`（2..=12 长度 + 首字母小写才查关键字表）　`// Go: scanner.go:GetIdentifierToken`
- [ ] `pub fn is_valid_identifier(s:&str)->bool`　`// Go: scanner.go:IsValidIdentifier`
- [ ] `pub fn is_identifier_start(ch:i32)->bool` / `is_identifier_part(ch)` / `is_identifier_part_ex(ch,variant)`　`// Go: scanner.go:IsIdentifierStart/Part/PartEx`
- [ ] `fn is_unicode_identifier_start/part`、`fn is_in_unicode_ranges(cp,ranges)->bool`（成对区间二分，**逐行对齐**）　`// Go: scanner.go:isInUnicodeRanges`
- [ ] `pub fn token_to_string(k:Kind)->&'static str` / `string_to_token(s)->Kind` / `get_viable_keyword_suggestions()->Vec<&str>`　`// Go: scanner.go:TokenToString/StringToToken/GetViableKeywordSuggestions`
- [ ] trivia 族：`could_start_trivia/skip_trivia/skip_trivia_ex(opts)`、`is_conflict_marker_trivia/scan_conflict_marker_trivia`、`is_shebang_trivia/scan_shebang_trivia/get_shebang`　`// Go: scanner.go:couldStartTrivia..GetShebang`
- [ ] `static TEXT_TO_KEYWORD: FxHashMap<&str,Kind>` / `static TEXT_TO_TOKEN`（含全部运算符+关键字）、`static TOKEN_TO_TEXT: [&str; KindCount]`　`// Go: scanner.go:textToKeyword/textToToken/tokenToText`
- [ ] `static IDENT_START: &[i32]` / `static IDENT_PART: &[i32]`（unicode 15.1 区间表，原样搬运）　`// Go: scanner.go:unicodeESNextIdentifier*`

### `lib.rs` — 位置 / 行号 / UTF-16 换算（命门，Go: `scanner.go`）

- [ ] `pub fn get_scanner_for_source_file(sf,pos)->Scanner` / `scan_token_at_position` / `get_range_of_token_at_position`　`// Go: scanner.go:GetScannerForSourceFile/ScanTokenAtPosition/GetRangeOfTokenAtPosition`
- [ ] `pub fn get_token_pos_of_node(node,sf,include_jsdoc)->i32`　`// Go: scanner.go:GetTokenPosOfNode`
- [ ] `pub fn get_error_range_for_node(sf,node)->TextRange`（+ `get_error_range_for_arrow_function`）　`// Go: scanner.go:GetErrorRangeForNode`
- [ ] `pub fn compute_line_of_position(line_starts,pos)->i32`（二分）　`// Go: scanner.go:ComputeLineOfPosition`
- [ ] `pub fn get_ecma_line_starts(sf)->&[TextPos]`（取 `sf.ECMALineMap()`）　`// Go: scanner.go:GetECMALineStarts`
- [ ] `pub fn get_ecma_line_of_position` / `get_ecma_line_and_utf16_character_of_position` / `get_ecma_line_and_byte_offset_of_position` / `get_ecma_end_line_position`　`// Go: scanner.go:GetECMALine*`
- [ ] `pub fn get_ecma_position_of_line_and_utf16_character` / `..byte_offset` / `compute_position_of_line_and_byte_offset` / `compute_position_of_line_and_utf16_character(.., allow_edits)`（逐码点累计 UTF-16，越界 panic/钳制两路）　`// Go: scanner.go:GetECMAPositionOf*/ComputePositionOf*`
- [ ] `pub fn get_leading_comment_ranges` / `get_trailing_comment_ranges` / `iterate_comment_ranges`（返回迭代器；Rust 用 `impl Iterator` 或回调）　`// Go: scanner.go:GetLeadingCommentRanges/GetTrailingCommentRanges/iterateCommentRanges`

### `utilities.rs`（Go: `internal/scanner/utilities.go`）

- [ ] 代理对常量 `SURR1/SURR2/SURR3/SURR_SELF` + `code_point_is_high_surrogate/low_surrogate/surrogate_pair_to_codepoint`　`// Go: utilities.go:codePointIs*Surrogate/surrogatePairToCodepoint`
- [ ] `fn encode_surrogate(r)->String`（3 字节 CESU-8 哨兵）/ `fn decode_class_atom_rune(s)->(i32,usize)`　`// Go: utilities.go:encodeSurrogate/decodeClassAtomRune`
- [ ] `pub fn identifier_to_keyword_kind(id)->Kind`　`// Go: utilities.go:IdentifierToKeywordKind`
- [ ] `pub fn get_source_text_of_node_from_source_file/get_text_of_node_from_source_text/get_text_of_node`　`// Go: utilities.go:GetSourceTextOfNodeFromSourceFile/GetTextOfNodeFromSourceText/GetTextOfNode`
- [ ] `pub fn declaration_name_to_string(name)->String`　`// Go: utilities.go:DeclarationNameToString`
- [ ] `pub fn is_identifier_text(name,variant)->bool`　`// Go: utilities.go:IsIdentifierText`
- [ ] `pub fn is_intrinsic_jsx_name(name)->bool`（首字母小写或含 `-`）　`// Go: utilities.go:IsIntrinsicJsxName`

### `unicodeproperties.rs`（Go: `internal/scanner/unicodeproperties.go`）

- [ ] `static NON_BINARY_UNICODE_PROPERTIES: FxHashMap<&str,&str>`　`// Go: unicodeproperties.go:nonBinaryUnicodeProperties`
- [ ] `static BINARY_UNICODE_PROPERTIES / BINARY_UNICODE_PROPERTIES_OF_STRINGS / SCRIPT_VALUES: FxHashSet<&str>`　`// Go: unicodeproperties.go:binaryUnicodeProperties/...`
- [ ] `static VALUES_OF_NON_BINARY_UNICODE_PROPERTIES: FxHashMap<&str, FxHashSet<&str>>`（General_Category / Script / Script_Extensions）　`// Go: unicodeproperties.go:valuesOfNonBinaryUnicodeProperties`

### `regexp.rs`（Go: `internal/scanner/regexp.go`）

- [ ] `struct RegExpParser { ... }` + `pos/set_pos/inc_pos/char/char_at/text/error`　`// Go: regexp.go:regExpParser`
- [ ] `fn check_regular_expression_flag_availability(...)`（挂在 `Scanner` 上）　`// Go: regexp.go:checkRegularExpressionFlagAvailability`
- [ ] `run` → `scan_disjunction/scan_alternative/scan_pattern_modifiers`　`// Go: regexp.go:run/scanDisjunction/scanAlternative/scanPatternModifiers`
- [ ] escape 族：`scan_atom_escape/scan_decimal_escape/scan_character_escape/scan_character_class_escape`　`// Go: regexp.go:scan*Escape`
- [ ] group/class 族：`scan_group_name/named_capturing_groups_contains/scan_class_ranges/scan_class_set_expression/scan_class_set_sub_expression/scan_class_set_operand/scan_class_string_disjunction_contents/scan_class_set_character/scan_class_atom`　`// Go: regexp.go:scanGroup*/scanClass*`
- [ ] unicode 属性建议：`get_spelling_suggestion_for_unicode_property_name/value/name_or_value`　`// Go: regexp.go:getSpellingSuggestionForUnicodeProperty*`
- [ ] 杂项：`compare_decimal_strings/scan_word_characters/scan_source_character/scan_expected_char/scan_digits`　`// Go: regexp.go:compareDecimalStrings/scanWordCharacters/...`

### Cargo / crate 接线

- [ ] `internal/scanner/Cargo.toml`（`name = "tsgo_scanner"` + path deps：ast/core/debug/diagnostics/jsnum/stringutil/collections）
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/scanner`
- [ ] `lib.rs` 声明 `mod regexp; mod unicodeproperties; mod utilities;` + re-export 公开项

## TDD 推进顺序（tracer bullet → 增量）

1. **位置/UTF-16 纯函数先行**（无 scanner 状态依赖）：`utf16_len`(来自 core)→`compute_line_of_position`→`get_ecma_line_and_utf16_character_of_position`→`compute_position_of_line_and_utf16_character`。配 tests.md 的 UTF-16/行号行为级用例（ASCII 与含星形面 emoji 各一组）。
2. **最小 `scan()` tracer**：只支持空白 trivia + 单字符标点 + EOF，断言 `token/tokenStart/tokenEnd`。
3. **标识符/关键字**：`scan_identifier` + `get_identifier_token` + 关键字表；断言 `let`/`yield`/`实例标识符`/带 `$_` 标识符。
4. **数字/字符串/模板/转义**：逐类补，断言 `tokenValue` 规范化。
5. **rescan 家族**：`re_scan_greater_than_token`（`>>` → 拆 `>`）、`re_scan_slash_token`（`/` → 正则）。
6. **trivia/注释/shebang/conflict marker** 与 `iterate_comment_ranges`。
7. **regexp 校验器**最后接（依赖 unicodeproperties 表），用已知非法正则触发诊断。

## 与 Go 的已知偏离（divergence）

- **`ScannerState` 的 `Copy`**：Go 里 `ScannerState` 含 `tokenValue string` 与 `commentDirectives []`，按值拷贝即可（string/slice 是胖指针）。Rust 若要 `Copy`，`String`/`Vec` 不行。决策：`tokenValue` 用 `String` 但 `ScannerState` 改为 `Clone`（非 `Copy`）；`commentDirectives` 移出 state、用 `comment_directives_len` 记录回滚点。mark/rewind 改为 clone+截断。**须在执行期实测 lookahead 性能**，必要时把 `tokenValue` 改存 `Range` 哨兵。
- **`char()` 返回 `rune` 但只解单字节**：Rust 用 `i32` + `as_bytes()[pos]`，EOF 返回 `-1`；务必不要无意中用 `chars()` 改变语义。
- **非法 UTF-8 输入**：Go 源文本是任意 `string`（可含非法字节），`utf8.DecodeRuneInString` 对非法返回 `(RuneError=0xFFFD,1)`。Rust 的 `str` 保证合法 UTF-8。若上游（vfs/读文件）保证已是合法 UTF-8，则无差异；否则需在入口处理替换字符。标 `// TODO(port): 非法字节边界` 待 vfs 阶段确认。
- **`ErrorCallback` 借用**：回调期间 parser 要改自身诊断列表，Go 靠闭包捕获指针。Rust 借用检查更严，接口由 parser 阶段（同 P3）统一定（可能用 `&mut` 传出或回调返回诊断）。
- **`iterateCommentRanges` 返回 `iter.Seq`**：Rust 用 `impl Iterator<Item=CommentRange>` 或内部回调；语义等价但写法不同。

## 转交 / 推迟（DEFER）

- scanner 的**正确性 gate 主要靠 P10**：Go 侧本包 0 直接单测（见 tests.md），真正全覆盖在 P10 conformance/fourslash 对拍。本轮补行为级 Rust 测试覆盖关键路径。
- `regexp.rs` 的全部诊断信息对齐推迟到 P10（诊断消息文本来自 `tsgo_diagnostics`，已在 P2/P1）；本轮先保证 token 切分与 flag 校验主路径。
- `ErrorCallback` 与 parser 的精确接线 `// DEFER(phase-3-parser)`。
