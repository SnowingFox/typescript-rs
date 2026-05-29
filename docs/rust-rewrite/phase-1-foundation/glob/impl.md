# glob: 实现方案（impl.md）

**crate**：`tsgo_glob`　**目标**：实现 LSP 规范定义的 glob 模式（`*` `?` `**` `{}` `[]` `[!...]`）的解析与匹配。
**依赖（crate）**：无（仅标准库 + UTF-8 处理）。叶子包。
**Go 源**：`internal/glob/`（1 个非测试文件：`glob.go` 350 行）

## 这个包是什么（业务说明）

`glob` 是 LSP `documentFilter` 的 glob 匹配器，主要用于 file watcher / `files`-`include`-`exclude` 之类的路径匹配场景。Go 源头部注释明确：当前实现"仅用于测试目的"，要生产化还需对照 VS Code 实现、补测试、微基准、以及厘清"字符"是否指 UTF-16 code unit。

模型是一个**元素序列 + 回溯匹配器**：
- `Parse` 把模式串解析成 `[]element`（`slash`/`literal`/`star`/`anyChar`/`starStar`/`group`/`charRange`）。
- `Match` 用递归 + 回溯逐元素消费输入串。
- 语义要点：`/` 匹配一个或多个连续斜杠；`*` 匹配段内任意字符（不跨 `/`）；`**` 只能与 `/` 相邻，可跨段；`{a,b}` 是 OR 分组；`[x-y]`/`[!x-y]` 是字符范围/取反。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type element fmt.Stringer`（接口） | `enum Element { Slash, Literal(String), Star, AnyChar, StarStar, Group(Vec<Glob>), CharRange{negate, low, high} }` | Go 用接口实现的判别联合 → Rust `enum`（PORTING §3：`interface{}` → enum 优先） |
| `group []*Glob` | `Group(Vec<Glob>)` | 分组是若干子 Glob |
| `charRange{negate bool; low,high rune}` | `CharRange{negate: bool, low: char, high: char}` | rune→char |
| `(g *Glob) String()`（各元素 `String()`） | `impl Display for Element` / `Glob` | 用于回显/调试 |
| `parse` 返回 `(*Glob, string, error)` | `fn parse(pattern: &str, nested: bool) -> Result<(Glob, &str), GlobError>` | 多返回值 → 元组；剩余串用 `&str` 切片 |
| `error`（`errBadRange` 等包级哨兵） | `enum GlobError`（`thiserror`） | `errBadRange` / `errInvalidUTF8` → enum 变体 |
| `match([]element, string) bool` 递归回溯 | `fn match_elems(elems: &[Element], input: &str) -> bool` | 递归 + slice 窗口；保持回溯算法结构 1:1 |
| `utf8.DecodeRuneInString` | `str::chars().next()` / `char_indices()` | charRange 与 readRangeRune 解码 |

> **结构 1:1 注意**：`match` 的回溯逻辑（`starStar` 推进段、`star` 段内回溯、`group` 把剩余元素拼到每个分支后再递归）必须逐分支照搬，否则边界用例（如 `**/a` 匹配 `"a"`、尾随 `**` 匹配一切）会偏。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/glob/glob.go` | `internal/glob/glob.rs`（basename `glob` == crate 目录名 → 实为 `lib.rs`） | 解析 + 匹配 + 元素枚举 |

## 依赖白名单（本包新增的 crate）

- `thiserror`（`GlobError`，库内错误）。其余仅标准库。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/glob/glob.go`）

- [ ] `pub struct Glob { elems: Vec<Element> }`　`// Go: glob.go:Glob`
- [ ] `enum Element { Slash, Literal(String), Star, AnyChar, StarStar, Group(Vec<Glob>), CharRange{negate,low,high} }`　`// Go: glob.go`（element 类型族）
- [ ] `pub fn parse(pattern: &str) -> Result<Glob, GlobError>` — 调内部 `parse_inner(pattern,false)`　`// Go: glob.go:Parse`
- [ ] `fn parse_inner(pattern: &str, nested: bool) -> Result<(Glob, &str), GlobError>` — 主解析循环：`/`→Slash、`**`（仅邻 `/`，否则 `errBadRange`/"** may only be adjacent to '/'"）、`*`→Star、`?`→AnyChar、`{`→递归分组（未闭合报 "unmatched '{'"）、`}`/`,`（nested 时返回）、`[`→范围、default→literal　`// Go: glob.go:parse`
- [ ] `fn read_range_rune(input: &str) -> Result<(char, usize), GlobError>` — 解码范围端点 rune；空→`errBadRange`，非法→`errInvalidUTF8`　`// Go: glob.go:readRangeRune`
- [ ] `fn parse_literal(&mut self, pattern, nested) -> remaining` — 截取到下一个特殊字符（nested 时特殊集含 `},`）　`// Go: glob.go:(*Glob).parseLiteral`
- [ ] `impl Display for Glob` / `Element` — 回显模式串　`// Go: glob.go:(各 String 方法)`
- [ ] `pub fn match_input(&self, input: &str) -> bool`（对应 `Match`） — 调 `match_elems`　`// Go: glob.go:(*Glob).Match`
- [ ] `fn match_elems(elems: &[Element], input: &str) -> bool` — 回溯匹配器：Slash（≥1 斜杠）、StarStar（推进段+尾随匹配一切+回溯）、Literal（前缀）、Star（段内回溯）、AnyChar（非 `/` 单字符）、Group（分支拼接递归）、CharRange（范围/取反）　`// Go: glob.go:match`
- [ ] `fn split(input: &str) -> (&str, &str)` — 首个斜杠（含连续斜杠）前后切分　`// Go: glob.go:split`
- [ ] `enum GlobError { BadRange, InvalidUtf8, DoubleStarAdjacency, UnmatchedBrace }` + `Display`　`// Go: glob.go:errBadRange/errInvalidUTF8 + inline errors`

### Cargo / crate 接线

- [ ] `internal/glob/Cargo.toml`（`name = "tsgo_glob"`，dep `thiserror`）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` re-export `Glob` / `GlobError`

## TDD 推进顺序（tracer bullet → 增量）

1. `parse` + `Display` round-trip（解析后回显等于规范化模式）—— tracer bullet。
2. 字面量 + `?` + `[x-y]` 段内匹配（最简单 `match_elems` 分支）。
3. `*` 段内回溯（`*.ts` 匹配 `foo.ts`，不跨 `/`）。
4. `/` 多斜杠 + `**` 跨段（`**/*.ts`、`**/a` 匹配 `a`、尾随 `**`）。
5. `{a,b}` 分组 + `[!x-y]` 取反 + 错误路径（未闭合 `{`、坏范围）。

## 与 Go 的已知偏离（divergence）

- **接口判别联合 → enum**：Go 的 `element fmt.Stringer` + 类型断言 `switch elem.(type)` → Rust `enum Element` + `match`。结构等价的必要偏离。
- **"字符"定义存疑**：Go 注释指出未厘清 character 是否为 UTF-16 code unit。本包按 Go 现状（`utf8.DecodeRuneInString`，即 Unicode code point）实现，存疑处加 `// TODO(port)`，留待 P10 与 VS Code 行为对拍。
- **剩余串返回**：Go `parse` 返回剩余 `string`，Rust 用 `&str` 切片（生命周期绑定输入），等价。
- **错误消息文本**：Go 内联 `errors.New("** may only be adjacent to '/'")` 等消息，Rust `GlobError` 的 `Display` 须保留同文本以备上层断言。

## 转交 / 推迟（DEFER）

- 与 VS Code glob 行为的严格对齐、UTF-16 character 语义厘清：标 `// DEFER(phase-10)`，由 P10 parity 决定是否需要调整。
