# evaluator: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**0 个 `*_test.go`**。Go 侧该包无任何直接单测。

## 0 直接单测的情况

- Go 侧 `internal/evaluator/` 无 `*_test.go`。该包的正确性在 Go 仓库里完全依赖 **checker 的 conformance / enum 用例**间接覆盖（如 `tests/cases/conformance/enums/*`、模板字面量类型用例）。
- 因此本包归入 README "0 直接单测"清单，行为由 **P10 conformance/fourslash parity** 兜底。
- 本轮**补充**少量行为级 Rust 测试（基于公开接口 `new_evaluator` / `any_to_string` / `is_truthy`，expected 取自 TS 语言规范 + Go 实现里写死的语义）。所有断言用一个 stub `eval_entity`（返回 `None`，或返回固定 `Result` 以验证透传）。

## 补充行为级 Rust 测试（`lib.rs` 内 `#[cfg(test)] mod tests`）

> input 列里 `<E(...)>` 表示注入的 stub entity 求值结果；其余为 AST 字面量片段。expected 用 Rust 字面量。

### 字面量与算术（核心折叠路径）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `eval_string_literal` | 字符串字面量折叠 | `"abc"` → `Result{ Str("abc"), is_syntactically_string=true }` | evaluator.go:`KindStringLiteral` 分支 | ✓ |
| `eval_no_substitution_template` | 无插值模板串折叠 | `` `abc` `` → `Str("abc")`, `is_syntactically_string=true` | evaluator.go:`KindNoSubstitutionTemplateLiteral` | —（P4，blocked-by `tsgo_ast` 模板 NodeData） |
| `eval_numeric_literal` | 数字字面量折叠 | `123` → `Num(123)` | evaluator.go:`KindNumericLiteral` | ✓ |
| `eval_unary_plus` | `+x` 原值 | `+5` → `Num(5)` | evaluator.go:`KindPlusToken` | ✓ |
| `eval_unary_minus` | `-x` 取负 | `-5` → `Num(-5)` | evaluator.go:`KindMinusToken` | ✓ |
| `eval_unary_tilde` | `~x` 位取反 | `~0` → `Num(-1)`（`jsnum.BitwiseNOT`） | evaluator.go:`KindTildeToken` | ✓ |
| `eval_binary_or` | 位或 | `1 \| 2` → `Num(3)` | evaluator.go:`KindBarToken` | ✓ |
| `eval_binary_and` | 位与 | `6 & 3` → `Num(2)` | evaluator.go:`KindAmpersandToken` | ✓ |
| `eval_binary_xor` | 位异或 | `5 ^ 1` → `Num(4)` | evaluator.go:`KindCaretToken` | ✓ |
| `eval_shift_left` | 左移 | `1 << 4` → `Num(16)` | evaluator.go:`KindLessThanLessThanToken` | ✓ |
| `eval_shift_right` | 有符号右移 | `-8 >> 1` → `Num(-4)`（`-8` 用真实一元负号节点） | evaluator.go:`KindGreaterThanGreaterThanToken` | ✓ |
| `eval_ushift_right` | 无符号右移 | `-1 >>> 0` → `Num(4294967295)`（`-1` 用真实一元负号节点） | evaluator.go:`KindGreaterThanGreaterThanGreaterThanToken` | ✓ |
| `eval_mul_div_add_sub_mod` | 算术 4+1 则 | `2*3`→6, `6/2`→3, `2+3`→5, `5-2`→3, `5%2`→1 | evaluator.go:`Asterisk/Slash/Plus/Minus/Percent` | ✓ |
| `eval_exponent` | 幂 | `2 ** 10` → `Num(1024)` | evaluator.go:`KindAsteriskAsteriskToken` | ✓ |

### 字符串拼接 / 模板 / 短路

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `eval_string_concat` | 字符串 + 字符串 | `"a" + "b"` → `Str("ab")`, `is_syntactically_string=true` | evaluator.go 二元 `+` str 分支 | ✓ |
| `eval_string_plus_number` | 字符串 + 数字（数字侧转串） | `"a" + 1` → `Str("a1")`, `is_syntactically_string=true` | evaluator.go `leftNum.String()` 路径 | ✓ |
| `eval_number_plus_string` | 数字 + 字符串 | `1 + "a"` → `Str("1a")` | evaluator.go `rightNum.String()` 路径 | ✓ |
| `eval_template_with_number_span` | 模板插值数字 | `` `a${1}b` `` → `Str("a1b")`, `is_syntactically_string=true` | evaluator.go:`evaluateTemplateExpression` | —（P4，blocked-by `tsgo_ast` 模板 NodeData） |
| `eval_template_span_none_short_circuits` | span 求值为 None 时短路 | `` `a${x}b` ``（stub entity → None）→ `Result{ None, is_syntactically_string=true }` | evaluator.go:`if spanResult.Value == nil` | —（P4，blocked-by `tsgo_ast` 模板 NodeData） |

### entity 透传与兜底

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `eval_identifier_calls_entity` | 标识符回调 entity | `x`（stub → `Num(42)`）→ `Num(42)` | evaluator.go:`KindIdentifier` | ✓ |
| `eval_property_access_entity_name` | 属性访问且为 entity name 时回调 | `a.b`（stub → `Str("v")`）→ `Str("v")` | evaluator.go:`KindPropertyAccess` 分支 | ✓ |
| `eval_propagates_resolved_flags` | `resolved_other_files`/`has_external_references` 从 entity 冒泡 | stub 返回带两标志=true → 一元 `-x`/二元 `x+y` 结果保留该标志 | evaluator.go 各分支对两 bool 的赋值 | ✓ |
| `eval_unsupported_returns_none` | 不可折叠表达式 | `f()`（CallExpression）→ `Result{ None, .. }` | evaluator.go 末尾兜底 | ✓ |
| `eval_skips_outer_parens` | 跳过外层括号 | `(1 + 2)` → `Num(3)` | evaluator.go:`SkipOuterExpressions` | ✓ |

### `any_to_string` / `is_truthy`（纯值函数，独立可测）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `any_to_string_str` | 字符串原样 | `Str("x")` → `"x"` | evaluator.go:AnyToString | ✓ |
| `any_to_string_num` | 数字转串 | `Num(1.5)` → `"1.5"` | evaluator.go:AnyToString | ✓ |
| `any_to_string_bool` | 布尔转串 | `Bool(true)`→`"true"`, `Bool(false)`→`"false"` | evaluator.go:`core.IfElse` | ✓ |
| `any_to_string_bigint` | bigint 转串 | `BigInt(10n)` → `"10"` | evaluator.go:AnyToString | ✓ |
| `is_truthy_string` | 非空串为真 | `Str("")`→false, `Str("a")`→true | evaluator.go:IsTruthy | ✓ |
| `is_truthy_number` | 0/NaN 为假 | `Num(0)`→false, `Num(NaN)`→false, `Num(1)`→true | evaluator.go:IsTruthy | ✓ |
| `is_truthy_bool` | 布尔原值 | `Bool(false)`→false | evaluator.go:IsTruthy | ✓ |
| `is_truthy_bigint` | 0n 为假 | `BigInt(0)`→false, `BigInt(1)`→true | evaluator.go:IsTruthy | ✓ |

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（`lib_test.rs` 每个 `#[test]` 上方均有 `// Go: internal/evaluator/evaluator.go:<Func/branch>`）

- [x] 每个 Go `func Test*` 都已映射 —— **N/A**（Go 侧 0 单测）
- [x] 每个表驱动子用例都已逐行列出 —— N/A
- [x] expected 值均取自 Go 实现语义 / TS spec（非 Rust 推断）
- [x] 每条对应到 impl.md 的一个实现 TODO（折叠分支 / `any_to_string` / `is_truthy`）
- [x] 与 impl.md 双向对齐无遗漏：impl.md 的每个**已实现**折叠分支在此都有至少一条用例；模板分支（impl + 3 个测试）一并 DEFER(P4)

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `eval_no_substitution_template` / `eval_template_with_number_span` / `eval_template_span_none_short_circuits`（+ `evaluate_template_expression` 实现 + `KindTemplateExpression`/`KindNoSubstitutionTemplateLiteral` 分支） | blocked-by：`tsgo_ast` 当前为代表性子集，未移植 `TemplateExpression`/`TemplateSpan`/`NoSubstitutionTemplateLiteral` 的 `NodeData` 与构造器，无法构造测试节点；受 crate 编辑边界约束不能改 `tsgo_ast` | P4（AST 模板节点移植后） |
| 真实 enum 成员常量内联 / `isolatedModules` 诊断 | 需要 checker 的 `evaluateEntity` 实装 + 真符号解析 | P4 checker / P10 |
| 完整 enum / 模板字面量类型 conformance 对拍 | 端到端 | P10 |
