# parser: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**1 文件 / 3 顶层 func（其中仅 1 个 `Test*`）/ 1 个表驱动（无子用例，单函数内多断言）**。
`parser_test.go` 的 3 个 func：`BenchmarkParse`（基准）、`FuzzParser`（模糊）、`TestJSDocImportTypeParentChain`（唯一单测）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/parser/parser_test.go` | `internal/parser/tests/jsdoc_import_parent_chain.rs`（集成测试）或 `lib.rs` `#[cfg(test)] mod tests` | 1 个可移植 `Test*`（另 2 个为 bench/fuzz，推迟 P10） |

## `parser_test.go`

> Go: 3 个顶层 func。下表逐 func 列；`TestJSDocImportTypeParentChain` 是非表驱动的单函数多断言测试，逐断言拆行。

### `TestJSDocImportTypeParentChain`（唯一可移植单测）

输入源（`.js`，`ScriptKindJS`）：5 个 `test("", async function () { ... })`，函数体里有形如 `(/** @type {typeof import("a")} */ ({}))` 与 `(/** @type {typeof import("a")} */ a)` 的 JSDoc 类型断言（含 `;` 前缀与不含的变体）。解析后断言两件事：**ReparsedClones 不含相邻重复**、**每个 import 的 reparsed 节点 parent 链可回溯到 SourceFile**。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `jsdoc_import_no_duplicate_reparsed_clones` | `ReparsedClones` 中相邻两项不得 `pos==pos && end==end && kind==kind`（JSDoc `import("a")` 重解析去重） | 上述 5 段源 → 对 `file.ReparsedClones[i-1]` 与 `[i]` 全程无重复 | `parser_test.go:TestJSDocImportTypeParentChain`（前半 for 循环） | |
| `jsdoc_import_parent_chain_intact` | 每个 `file.Imports()` 的 reparsed 节点经 `GetReparsedNodeForNode` 后 `GetSourceFileOfNode != nil`（parent 链未断） | 同上源 → 所有 import 的 reparsed 节点都能回溯到 SourceFile | `parser_test.go:TestJSDocImportTypeParentChain`（后半 for 循环） | |

> 实现依赖：`ast.ReparsedClones`/`Imports()`/`GetReparsedNodeForNode`/`GetSourceFileOfNode`（P2 ast）+ 本包 `reparser.rs` 的 reparse 去重逻辑（`addDeepCloneReparse`/`reparsedClones`）。这条单测正是 reparser 正确性的最小回归，**优先实现**。

### `BenchmarkParse`（基准，非单测）

| Rust 对应 | 说明 | Go 对照 | 完成 |
|---|---|---|---|
| `criterion` bench（推迟） | 遍历 `fixtures.BenchFixtures` 解析大文件计时 | `parser_test.go:BenchmarkParse` | — (P10) |

### `FuzzParser`（模糊，非单测）

| Rust 对应 | 说明 | Go 对照 | 完成 |
|---|---|---|---|
| `cargo fuzz` target（推迟） | 用 TypeScript submodule 源 + testdata 作语料，断言 `parse_source_file` 不 panic | `parser_test.go:FuzzParser` | — (P10) |

## 补充行为级 Rust 测试（保证移植期 red→green，不等 P10）

> 依据：TS 语法 spec + 对当前 Go 实现的实测（expected 取 Go/spec，不靠 Rust 推断）。覆盖 impl.md 里 tracer/优先级/投机/JSON 等关键路径。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `parse_empty_source` | 空源解析 | `""`(TS) → `SourceFile` 0 statements，`EndOfFile` token，无诊断 | spec | |
| `parse_single_empty_statement` | 空语句 | `";"` → 1 个 `EmptyStatement` | spec | |
| `parse_expression_statement_pos_end` | 节点 Loc（pos 含前导 trivia、end 不含尾随） | `"  x;"` → `Identifier` 节点 `Pos()==0`(含空白)、`x` 的 token start=2 | Go `finishNode`/`nodePos` 实测 | |
| `parse_binary_precedence` | 运算符优先级树形 | `"1 + 2 * 3"` → `Binary(+, 1, Binary(*, 2, 3))` | spec | |
| `parse_conditional` | 三元 | `"a ? b : c"` → `ConditionalExpression` | spec | |
| `parse_arrow_speculation` | 箭头 vs 括号投机（mark/rewind） | `"(x) => x"` → `ArrowFunction`；`"(x)"` → `ParenthesizedExpression` | spec | |
| `parse_isolated_entity_name_chain` | `ParseIsolatedEntityName` | `"a.b.c"` → `QualifiedName(QualifiedName(a,b),c)`，无诊断；`"a.="` → `None`（有诊断） | `ParseIsolatedEntityName` | |
| `parse_generic_type_rescan_gt` | 类型实参 `>>` 回扫 | `"let x: Array<Array<number>>;"` → 正确闭合两层 `TypeReference`（依赖 `re_scan_greater_than_token`） | spec | |
| `parse_json_object` | JSON 模式 | `'{"a":1}'`(ScriptKindJSON) → `ObjectLiteralExpression` 1 属性，无诊断 | `parseJSONText` | |
| `parse_json_single_quote_diagnostic` | JSON 模式诊断 | `"{'a':1}"`(JSON) → 报 `String_literal_with_double_quotes_expected` | `validateJsonObjectLiteral` | |
| `parse_missing_brace_recovery` | 错误恢复不崩 | `"function f( {"` → 产生诊断但返回非 nil `SourceFile`，含 `ThisNodeHasError` flag | Go 实测 | |
| `collect_import_references` | 外部模块引用收集 | `'import x from "m";'` → `file.Imports()` 含 `"m"` 字面量 | `collectModuleReferences` | |
| `uses_node_prefix_core_modules` | `node:` 前缀标志 | `'import "node:fs";'` → `UsesUriStyleNodeCoreModules==TSTrue` | `collectModuleReferences` | |

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*` 都已映射：唯一 `TestJSDocImportTypeParentChain` 已拆为 2 条 Rust 测试（去重 + parent 链）
- [ ] `BenchmarkParse`/`FuzzParser` 标注 `— (P10)`
- [ ] 每条用例都有 impl.md 承载：reparse 去重→`reparser.rs`；优先级→`parseBinaryExpressionOrHigher`；投机→`look_ahead`；JSON→`parse_json_text`；references→`references.rs`
- [ ] expected 值取自 Go 测试字面量 / spec（非 Rust 推断）
- [ ] 每条带 `// Go:` 锚点（`parser_test.go:TestJSDocImportTypeParentChain` / `parser.go:<Func>`）
- [ ] 与 impl.md 双向对齐：impl 的 tracer/优先级/JSON/JSDoc-reparse 路径均在此有测试承载

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `BenchmarkParse` | 需 `fixtures.BenchFixtures` 大文件 | P10 |
| `FuzzParser` | 需 TypeScript submodule + testdata 语料 | P10 |
| 全量产生式 / 诊断 parity | 4250 fourslash + 294MB conformance 对拍 | P10 |
| 完整 JSDoc/reparse 在真实 `.js` 上的对齐 | 端到端 baseline | P10 |
