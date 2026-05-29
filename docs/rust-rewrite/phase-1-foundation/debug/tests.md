# debug: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 文件 / 11 顶层 `func Test` / 11 用例（非表驱动，每个 `func Test*` 一条断言）。

> Go 测试在 `package debug_test`（外部测试包），用 `testutil.AssertPanics(t, fn, expectedMsg)` 断言 panic 消息逐字相等。Rust 侧用 `#[should_panic(expected = "...")]` 或 `catch_unwind` 比对 panic payload。
> `testutil.AssertPanics` 属于 P10 的 testutil 包；本包测试需要它，故 Rust 侧先实现一个等价的本地辅助（catch_unwind + downcast 到 `&str`/`String`），或直接用 `#[should_panic]`。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/debug/debug_test.go` | `internal/debug/lib.rs`（`#[cfg(test)] mod tests`） | 11 |

## `debug_test.go`

> 每个 `func Test*` 一条断言（非表驱动）。逐函数列。mock 类型：`mockNode{kind}` 实现 `KindString()`；`mockStringer{s}` 实现 `String()`。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `fail_empty_reason` | 空 reason 的默认消息 | `fail("")` panic → `"Debug failure."` | `debug_test.go:TestFailEmptyReason` | |
| `fail_with_reason` | 带 reason | `fail("something went wrong")` → `"Debug failure. something went wrong"` | `debug_test.go:TestFailWithReason` | |
| `fail_bad_syntax_kind_no_message` | 默认节点消息 | `fail_bad_syntax_kind(mock("FooNode"), None)` → `"Debug failure. Unexpected node.\nNode FooNode was unexpected."` | `debug_test.go:TestFailBadSyntaxKindNoMessage` | |
| `fail_bad_syntax_kind_with_message` | 自定义消息 | `fail_bad_syntax_kind(mock("BarNode"), Some("custom message"))` → `"Debug failure. custom message\nNode BarNode was unexpected."` | `debug_test.go:TestFailBadSyntaxKindWithMessage` | |
| `assert_never_default_message_kind_string` | 默认消息 + KindString 优先 | `assert_never(mock("TestNode"), None)` → `"Debug failure. Illegal value: TestNode"` | `debug_test.go:TestAssertNeverDefaultMessageKindString` | |
| `assert_never_custom_message_kind_string` | 自定义消息 + KindString | `assert_never(mock("TestNode"), Some("bad value:"))` → `"Debug failure. bad value: TestNode"` | `debug_test.go:TestAssertNeverCustomMessageKindString` | |
| `assert_never_stringer` | 退到 Display | `assert_never(mock_stringer("hello"), None)` → `"Debug failure. Illegal value: hello"` | `debug_test.go:TestAssertNeverStringer` | |
| `assert_never_fallback` | 退到 Debug/`%v` | `assert_never(42, None)` → `"Debug failure. Illegal value: 42"` | `debug_test.go:TestAssertNeverFallback` | |
| `assert_true` | true 不 panic | `assert(true, None)` 正常返回 | `debug_test.go:TestAssertTrue` | |
| `assert_true_with_message` | true + 消息不 panic | `assert(true, Some("this should not trigger"))` 正常返回 | `debug_test.go:TestAssertTrueWithMessage` | |
| `assert_false_no_message` | false 默认消息 | `assert(false, None)` → `"Debug failure. False expression."` | `debug_test.go:TestAssertFalseNoMessage` | |
| `assert_false_with_message` | false + 消息 | `assert(false, Some("expected x > 0"))` → `"Debug failure. False expression: expected x > 0"` | `debug_test.go:TestAssertFalseWithMessage` | |

> 注：Go 文件里 `TestAssertTrue` 与 `TestAssertTrueWithMessage` 是两个独立函数（共 11 个 `func Test`，上表 12 行因 assert_true 拆两行）。逐 `func Test` 计为 11。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（11 个全覆盖）
- [x] 无表驱动子用例（每个 `func Test` 独立断言）
- [x] expected 值均取自 Go 测试字面量（panic 消息逐字抄）
- [x] 每条带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐无遗漏（fail/fail_bad_syntax_kind/assert_never/assert 全部有测试）

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `assert_never` 对真实 AST 节点（实现 `KindString`）的行为 | AST 节点在 P2/P3 落地 | P2/P3 |
| `message ...any` 多参拼接语义与 Go `fmt.Sprint` 完全一致 | 当前测试仅单参；多参规则需更多用例 | 实现期补 |
