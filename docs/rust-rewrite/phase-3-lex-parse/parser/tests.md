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
| `parse_empty_source` | 空源解析 | `""`(TS) → `SourceFile` 0 statements，`EndOfFile` token，无诊断 | spec | ✓ |
| `parse_single_empty_statement` | 空语句 | `";"` → 1 个 `EmptyStatement` | spec | ✓ |
| `parse_expression_statement_pos_end` | 节点 Loc（pos 含前导 trivia、end 不含尾随） | `"  x;"` → `Identifier` 节点 `Pos()==0`(含空白)、`x` 的 token start=2 | Go `finishNode`/`nodePos` 实测 | ✓ |
| `parse_binary_precedence` | 运算符优先级树形 | `"1 + 2 * 3"` → `Binary(+, 1, Binary(*, 2, 3))` | spec | ✓ |
| `parse_conditional` | 三元 | `"a ? b : c"` → `ConditionalExpression` | spec | ✓ |
| `parse_parenthesized_expression` | 括号表达式（箭头 vs 括号投机的括号半） | `"(x)"` → `ParenthesizedExpression` | spec | ✓ |
| `parse_prefix_unary_expression` / `parse_postfix_unary_expression` | 前缀/后缀一元 | `"-a"` → `PrefixUnaryExpression`；`"a++"` → `PostfixUnaryExpression` | spec | ✓ |
| `parse_arrow_speculation`（箭头半） | 箭头投机（mark/rewind） | `"(x) => x"` → `ArrowFunction` | spec | — (DEFER phase-3: arrow + param/binding + type parser) |
| `parse_isolated_entity_name_chain` | `ParseIsolatedEntityName` | `"a.b.c"` → `QualifiedName(QualifiedName(a,b),c)`，无诊断；`"a.="` → `None`（有诊断） | `ParseIsolatedEntityName` | ✓ |
| `parse_generic_type_rescan_gt` | 类型实参 `>>` 回扫 | `"let x: Array<Array<number>>;"` → 正确闭合两层 `TypeReference`（依赖 `re_scan_greater_than_token`） | spec | — (DEFER phase-3: type-node parser) |
| `parse_json_object` | JSON 模式 | `'{"a":1}'`(ScriptKindJSON) → `ObjectLiteralExpression` 1 属性，无诊断 | `parseJSONText` | — (DEFER phase-3: object literal + JSON value parser) |
| `parse_json_single_quote_diagnostic` | JSON 模式诊断 | `"{'a':1}"`(JSON) → 报 `String_literal_with_double_quotes_expected` | `validateJsonObjectLiteral` | — (DEFER phase-3) |
| `parse_missing_brace_recovery` | 错误恢复不崩 | `"function f( {"` → 产生诊断但返回非 nil `SourceFile`，含 `ThisNodeHasError` flag | Go 实测 | — (DEFER phase-3: function decl + block) |
| `collect_import_references` | 外部模块引用收集 | `'import x from "m";'` → `file.Imports()` 含 `"m"` 字面量 | `collectModuleReferences` | — (DEFER phase-3: import decl + references.rs) |
| `uses_node_prefix_core_modules` | `node:` 前缀标志 | `'import "node:fs";'` → `UsesUriStyleNodeCoreModules==TSTrue` | `collectModuleReferences` | — (DEFER phase-3) |
| `deep_clone_node_sanity_check`（在本 crate 测试） | 真实解析 + `deep_clone_node` BFS 不变量（distinct id + 子数相等），代表性 28 case；为 ast `TestDeepCloneNodeSanityCheck` 的 parser-backed 回填 | `;"test"`/`a.b.c`/`a(b,c)`/`a?b:c`/`[...a]` 等 → 克隆结构一致 | `ast/deepclone_test.go:TestDeepCloneNodeSanityCheck` | ✓ |

## Round 2 行为级测试（statements + declarations，全绿）

> 逐族 red→green 新增的 `#[test]`（`internal/parser/lib_test.rs`，每条带 `// Go:` 锚），expected 取自 TS 语法 / Go 实现。当前 `cargo test -p tsgo_parser` = **62 单测 + 3 doctests** 全绿。

| 簇 | 代表测试 | 验证 | 完成 |
|---|---|---|---|
| 控制流语句 | `parse_block_statement`/`parse_if_else_statement`/`parse_if_without_else`/`parse_while_statement`/`parse_do_statement`/`parse_switch_statement`/`parse_throw_statement`/`parse_return_statement_with_and_without_value`/`parse_break_and_continue`/`parse_with_statement`/`parse_debugger_statement`/`parse_labeled_statement` | 节点种类 + 关键子节点 + 无诊断 | ✓ |
| 变量 + 绑定 | `parse_var_let_const_statements`/`parse_var_with_type_annotation`/`parse_var_with_type_reference_args`/`parse_array_and_union_types`/`parse_object_destructuring`/`parse_array_destructuring`/`parse_for_statements`/`parse_for_of_fields`/`parse_try_catch_finally`/`parse_try_catch_no_binding` | let/const flag、类型注解、解构、for/for-in/for-of、try/catch | ✓ |
| 函数 | `parse_function_declaration_basic`/`parse_function_overload_signature`/`parse_function_parameters`/`parse_function_generics`/`parse_function_with_modifiers`/`parse_export_const` | 参数(optional/rest/default/type)、泛型、修饰符 | ✓ |
| 类 | `parse_empty_class`/`parse_class_heritage`/`parse_class_members`/`parse_class_member_modifiers`/`parse_class_expression` | heritage、成员全族、修饰符、class 表达式 | ✓ |
| interface/type/enum | `parse_interface_declaration`/`parse_interface_signature_members`/`parse_type_alias`/`parse_type_alias_object_literal`/`parse_enum_declaration` | 类型成员、type alias、enum 成员 | ✓ |
| module/import/export | `parse_namespace_declaration`/`parse_ambient_module`/`parse_import_forms`/`parse_import_equals`/`parse_export_forms`/`parse_export_named_specifiers` | 嵌套 namespace、ambient module、import/export 全形态 | ✓ |
| deepclone 回填 | `deepclone_tests::deep_clone_node_sanity_check` | ~60 真实解析 case 的克隆不变量（distinct id + 子数相等） | ✓ |

## Round 3 行为级测试（剩余表达式 + 完整类型语法，全绿）

> 逐族 red→green 新增的 `#[test]`（`internal/parser/lib_test.rs`，每条带 `// Go:` 锚）。当前 `cargo test -p tsgo_parser` = **86 单测 + 3 doctests** 全绿；`tsgo_ast` = 52 单测 + 22 doctests 全绿。

| 簇 | 代表测试 | 验证 | 完成 |
|---|---|---|---|
| 对象字面量 | `parse_object_literal`/`parse_object_literal_computed_and_cover` | property/shorthand(+cover `=`)/spread/method/get + 计算键 | ✓ |
| 一元关键字 + yield | `parse_unary_keyword_expressions`/`parse_yield_expression` | delete/typeof/void/await + `yield`/`yield*`（生成器上下文） | ✓ |
| 函数表达式 | `parse_function_expression` | `function*`/可选名/返回类型 | ✓ |
| 箭头函数 | `parse_simple_arrow`/`parse_parenthesized_arrows`/`parse_async_arrows`/`parenthesized_not_arrow_regression` | `x=>`/`(a,b)=>`/`(a):T=>`/`async`，以及 `(x)`/`(a,b)` 非箭头回滚 | ✓ |
| LHS 扩展 | `parse_new_expression`/`parse_meta_properties`/`parse_optional_chain_and_non_null`/`parse_templates`/`parse_regex_literal` | new(+无参)、new.target/import.meta、`?.`/`!`、模板/标签模板、正则 | ✓ |
| as/satisfies/断言 | `parse_as_and_satisfies`/`parse_type_assertion` | `as T`/`satisfies T`/`<T>x` | ✓ |
| 函数/构造/条件/infer 类型 | `parse_function_and_constructor_types`/`parse_conditional_and_infer_types` | `(a)=>T`/`abstract new()=>T`/`X extends Y?A:B`/`infer U` | ✓ |
| 运算符/tuple/mapped 类型 | `parse_type_operators`/`parse_tuple_types`/`parse_mapped_type` | keyof/unique/readonly、`[a, ...T[]]`/具名/可选、`{readonly [K in T]?: V}` | ✓ |
| 模板/import/query/谓词/this 类型 | `parse_template_literal_type`/`parse_import_and_query_types`/`parse_type_predicates_and_this` | `` `a${T}b` ``、`import("m").X<n>`/`typeof x`/`typeof import`、`x is T`/`asserts x is T`/`this` | ✓ |
| deepclone 回填 | `deepclone_tests::deep_clone_node_sanity_check` | ~95 真实解析 case 的克隆不变量（含全部 Round 3 节点） | ✓ |

## Round 4 行为级测试（JSX / JSON / 装饰器 / import 属性 / 模块引用 / 精修，全绿）

> 当前 `cargo test -p tsgo_parser` = **104 单测 + 3 doctests** 全绿；`tsgo_ast` = 52 单测 + 22 doctests 全绿。每条带 `// Go:` 锚。

| 簇 | 代表测试 | 验证 | 完成 |
|---|---|---|---|
| 精修 | `parse_type_arguments_in_call`/`parse_instantiation_expression`/`parse_optional_chain_reparse_through_non_null`/`parse_negative_literal_type`/`parse_parenthesized_object_literals` | `f<T>()`/`f<T>`/`a?.b!.c` 链/`-1` 类型/`({m(){}})` 非箭头 | ✓ |
| 装饰器 | `parse_class_decorators`/`parse_member_and_parameter_decorators` | 类/成员/参数装饰器 + DECORATOR flag | ✓ |
| import 属性 | `parse_import_with_attributes`/`parse_export_and_import_type_attributes` | `import ... with { type: "json" }`、export、import 类型 | ✓ |
| JSX | `parse_jsx_element`/`parse_jsx_fragment`/`parse_jsx_self_closing_and_namespaced` | 元素/属性/spread/表达式容器/文本/子元素/fragment/成员·命名空间标签/类型实参 | ✓ |
| JSON | `parse_json_object`/`parse_json_array_and_literals`/`parse_json_validation_errors` | 对象/数组/字面量 + 双引号键/合法值校验诊断 | ✓ |
| 模块引用 | `collect_imports_and_external_module_indicator`/`external_module_indicator_export_modifier` | `Imports()` 收集 + external module indicator | ✓ |
| JSDoc(interim) | `jsdoc_comments_are_skipped_as_trivia` | JSDoc 作为 trivia 跳过、不破坏解析（语义层 DEFER） | ✓ |
| deepclone 回填 | `deepclone_tests::deep_clone_node_sanity_check` | ~107 真实解析 case（TS+TSX+JSON，含全部 Round 4 节点） | ✓ |

**DEFER（理由见 impl.md）**：`TestJSDocImportTypeParentChain` —— 依赖 JSDoc+reparser 子系统（≈2000 行，JS-only），整体推迟到专门 round；`BenchmarkParse`/`FuzzParser` —— P10。

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
