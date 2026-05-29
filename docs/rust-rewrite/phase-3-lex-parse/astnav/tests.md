# astnav: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**2 文件 / 6 顶层 func（5 个 `Test*` + 1 个 `TestMain`）/ 约 13 个 `t.Run` 子用例（其中可移植的内联确定性用例 5 个，其余为需 TS submodule+Node.js 的 baseline 对拍）**。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/astnav/tokens_test.go` | `internal/astnav/tests/tokens.rs`（集成测试） | 5（`TestGetTokenAtPosition`/`TestGetTouchingPropertyName`/`TestFindPrecedingToken`/`TestFindNextToken`/`TestUnitFindPrecedingToken`） |
| `internal/astnav/testmain_test.go` | （随 P10 baseline 框架） | 1（`TestMain`） |

## `tokens_test.go`

> 多数顶层测试是 **baseline 对拍**：对 `src/services/mapCode.ts` 的每个字节位置，分别用 Go astnav 与 Node.js 跑真实 `typescript` 的 `getTokenAtPosition` 等，比较差异写 baseline 文件。这类需 `repo.TypeScriptSubmodulePath()`+`jstest`(Node.js)+`testutil/baseline`，整体 `— (P10)`。下表逐 `t.Run` 列；可移植的内联确定性用例标依据。

### `TestGetTokenAtPosition`（5 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `get_token_at_position_baseline` | 与 TS 逐位置对拍 `getTokenAtPosition` | mapCode.ts 全位置 → 与 TS 一致 | `tokens_test.go:TestGetTokenAtPosition/baseline` | — (P10) |
| `get_token_at_position_go_json` | Go-only token run JSON baseline | mapCode.ts → token 区段 JSON | `tokens_test.go:TestGetTokenAtPosition/go baseline json` | — (P10) |
| `get_token_at_position_jsdoc_type_assertion` | JSDoc 类型断言处不 panic | `.js`：`function foo(x){ const s = /**@type {string}*/(x) }`，pos=52 → `GetTouchingPropertyName` 返回非 nil，Kind ∈ {Identifier, ParenthesizedExpression} | `tokens_test.go:TestGetTokenAtPosition/JSDoc type assertion` | |
| `get_token_at_position_jsdoc_type_assertion_with_comment` | 同上但带行尾注释，不 panic | `.js`：同上 + `// ...` 注释，xPos=52 → 返回非 nil | `tokens_test.go:TestGetTokenAtPosition/JSDoc type assertion with comment` | |
| `get_token_at_position_pointer_equality` | 同位置返回**同一对象**（token 缓存） | `.ts`：`function foo(){ return 0; }`，`GetTokenAtPosition(f,0)==GetTokenAtPosition(f,0)`（NodeId 相等） | `tokens_test.go:TestGetTokenAtPosition/pointer equality` | |

### `TestGetTouchingPropertyName`（2 子用例，均 baseline）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `get_touching_property_name_baseline` | 与 TS 对拍 `getTouchingPropertyName` | mapCode.ts 全位置 → 一致 | `tokens_test.go:TestGetTouchingPropertyName`（baseline） | — (P10) |
| `get_touching_property_name_go_json` | Go-only JSON baseline | mapCode.ts → JSON | `tokens_test.go:TestGetTouchingPropertyName/go baseline json` | — (P10) |

### `TestFindPrecedingToken`（2 子用例，均 baseline）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `find_preceding_token_baseline` | 与 TS 对拍 `findPrecedingToken`（含 EOF 位置） | mapCode.ts 全位置 → 一致 | `tokens_test.go:TestFindPrecedingToken/baseline` | — (P10) |
| `find_preceding_token_go_json` | Go-only JSON baseline | mapCode.ts → JSON | `tokens_test.go:TestFindPrecedingToken/go baseline json` | — (P10) |

### `TestFindNextToken`（1 子用例，baseline）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `find_next_token_go_json` | Go-only JSON baseline（trivia 间隙处 panic 被 recover→该位置缺省） | mapCode.ts → JSON | `tokens_test.go:TestFindNextToken/go baseline json` | — (P10) |

### `TestUnitFindPrecedingToken`（2 子用例，**可移植，表驱动**）

> 内联源 + 明确 position + 期望 Kind，纯确定性，无 Node.js 依赖。优先实现。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `find_preceding_token_after_dot_in_jsdoc` | 标识符后 `.` 的前导 token | `.ts`（含 import 块 + `backslashRegExp.` 一行），position=839 → `FindPrecedingToken().Kind == KindDotToken` | `tokens_test.go:TestUnitFindPrecedingToken/after dot in jsdoc` | |
| `find_preceding_token_after_comma_in_param_list` | 参数列表逗号后的前导 token | `.ts`：`takesCb((n, s, ))`，position=15 → `Kind == KindCommaToken` | `tokens_test.go:TestUnitFindPrecedingToken/after comma in parameter list` | |

## `testmain_test.go`

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| （无独立 Rust 对应） | 设 `ApplyDebugStackLimit` + `baseline.Track`，仅测试框架 | `testmain_test.go:TestMain` | — (P10) |

## 补充行为级 Rust 测试（可移植确定性，补 scanner 合成路径覆盖）

> 依据：TS spec + Go 实测。补 impl.md 里"两 token 间隙走 scanner 合成"路径的覆盖（baseline 对拍前先有确定性回归）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `token_at_position_on_identifier` | 命中真实 AST 节点 | `.ts`：`const x = 1;`，pos 落在 `x` 上 → Kind `Identifier`，pos/end 覆盖 `x` | Go 实测 | |
| `token_at_position_on_punctuation_synthesized` | 落在标点（AST 无独立节点）→ scanner 合成 | `.ts`：`a + b`，pos 落在 `+` 上 → Kind `PlusToken`（合成 token） | Go 实测 | |
| `find_next_token_basic` | 下一个 token | `.ts`：`a.b`，对 `a` 调 `FindNextToken` → `DotToken` | Go 实测 | |
| `find_child_of_kind_brace` | 容器内找指定 Kind | `.ts`：`function f(){}`，在函数体容器找 `OpenBraceToken` → 命中 | Go 实测 | |

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*` 都已映射：5 个 `Test*` 全列；baseline 子用例标 `— (P10)`，内联确定性用例（5 个）有 expected
- [ ] 每个表驱动子用例都逐行列出：`TestUnitFindPrecedingToken` 2 例、`TestGetTokenAtPosition` 5 例
- [ ] expected 值取自 Go 测试字面量（position 839/15、Kind=Dot/Comma、pos=52、`GetTokenAtPosition(f,0)` 相等）
- [ ] 每条带 `// Go:` 锚点（`tokens_test.go:<TestFunc>/<case>`）
- [ ] 与 impl.md 双向对齐：`pointer equality`→`GetOrCreateToken`；`JSDoc type assertion`→JSDoc 分支；`TestUnitFindPrecedingToken`→`find_preceding_token_ex`/`find_rightmost_valid_token`

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 全部 baseline / go-json 对拍（GetTokenAtPosition/GetTouchingPropertyName/FindPrecedingToken/FindNextToken） | 需 TypeScript submodule + Node.js(`jstest`) + `testutil/baseline` | P10 |
| `TestMain`（baseline.Track） | 测试框架设施 | P10 |
