# glob: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例。

## 0 直接单测的情况

- Go 侧无 `*_test.go`：`internal/glob` 头注释自述"仅用于测试目的、尚未生产化"，**无直接单测**；其行为由 **P10 conformance/fourslash parity** 兜底（file watcher / include-exclude 路径匹配端到端对拍）。
- 本轮补充的行为级 Rust 测试（基于 Go 头注释里的语义说明 + LSP glob spec 已知值）：

### parse / Display（解析正确性）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `parse_roundtrip_literal` | 纯字面量回显 | `parse("foo.ts").to_string()` → `"foo.ts"` | glob.go:Parse/String | |
| `parse_roundtrip_star_globstar` | `*`/`**`/`?` 回显 | `parse("**/*.ts")` 回显 `"**/*.ts"` | glob.go:Parse/String | |
| `parse_group_roundtrip` | 分组回显 | `parse("**/*.{ts,js}")` 回显含 `{ts,js}` | glob.go:Parse/group.String | |
| `parse_char_range_roundtrip` | 范围回显 | `parse("example.[0-9]")` 回显 `"example.[0-9]"` | glob.go:Parse/charRange.String | |
| `parse_err_double_star_adjacency` | `**` 非邻 `/` 报错 | `parse("a**b")` → Err("** may only be adjacent to '/'") | glob.go:parse | |
| `parse_err_unmatched_brace` | 未闭合 `{` 报错 | `parse("{a")` → Err("unmatched '{'") | glob.go:parse | |
| `parse_err_bad_range` | 坏范围报错 | `parse("[a]")`（缺 `-`）→ Err(bad range) | glob.go:parse/errBadRange | |

### Match（匹配语义，取自头注释示例与 LSP spec）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `match_literal_exact` | 字面量精确匹配 | `"foo.ts".match("foo.ts")` → true；`match("foo.js")` → false | glob.go:match/literal | |
| `match_star_within_segment` | `*` 匹配段内、不跨 `/` | `"*.ts"` 匹配 `"foo.ts"`→true、`"a/foo.ts"`→false | glob.go:match/star | |
| `match_anychar` | `?` 匹配单个非 `/` 字符 | `"a?c"` 匹配 `"abc"`→true、`"a/c"`→false | glob.go:match/anyChar | |
| `match_slash_multiple` | `/` 匹配一个或多个斜杠 | `"a/b"` 匹配 `"a//b"`→true | glob.go:match/slash+split | |
| `match_globstar_cross_segments` | `**/` 跨任意段 | `"**/*.ts"` 匹配 `"a/b/c.ts"`→true | glob.go:match/starStar | |
| `match_globstar_matches_none` | `**/a` 匹配 `"a"`（零段） | `"**/a"` 匹配 `"a"`→true | glob.go:match（注释特例） | |
| `match_globstar_trailing` | 尾随 `**` 匹配一切 | `"a/**"` 匹配 `"a/b/c"`→true | glob.go:match（注释特例） | |
| `match_group_or` | `{ts,js}` OR | `"*.{ts,js}"` 匹配 `"x.ts"`/`"x.js"`→true、`"x.go"`→false | glob.go:match/group | |
| `match_char_range` | `[0-9]` 范围 | `"example.[0-9]"` 匹配 `"example.0"`→true、`"example.a"`→false | glob.go:match/charRange | |
| `match_char_range_negate` | `[!0-9]` 取反 | `"example.[!0-9]"` 匹配 `"example.a"`→true、`"example.0"`→false | glob.go:match/charRange.negate | |

## 与 impl.md 的对齐核对

- [ ] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/glob/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）

- [x] 无 Go `func Test*` 需映射（0 直接单测，已说明）
- [x] 补充测试覆盖 parse/Display/match 全部元素类型与错误路径，均在 impl.md 有 TODO 承载
- [x] expected 取自 Go 头注释示例 + LSP glob spec 的确定值
- [x] 每条补充测试标注依据

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 与 VS Code glob 实现逐用例对齐 | Go 自述尚未对照 VS Code | P10 parity |
| UTF-16 "character" 语义（多字节边界） | Go 注释存疑未决 | P10 parity |
| file watcher / include-exclude 端到端路径匹配 | 需 project/lsp（P8）落地 | P10 parity |
