# binder: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**1 文件 / 1 顶层 func / 0 子用例**——且**唯一的 func 是 `BenchmarkBind`（基准），不是 `Test*`**。即：binder **无任何直接单测**。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/binder/binder_test.go` | `internal/binder/tests/behavior.rs`（本轮补行为级） | 0 个 `Test*`（仅 `BenchmarkBind`，推迟 P10） |

## 0 直接单测的情况

- **Go 侧无直接单测**：`binder_test.go` 只有 `BenchmarkBind`（对 `fixtures.BenchFixtures` 先 parse 再 `BindSourceFile` 计时）。该包行为由 **P10 conformance/fourslash parity** 兜底——binder 产出的 Symbol 表/flow 图是 checker 的输入，任何绑定错误都会让 checker→emit 的端到端 baseline 立刻发散，所以 binder 的正确性事实上被全量类型检查 baseline 覆盖。
- 本轮按 PORTING §8.5 **补充行为级 Rust 测试**：基于公开 `bind_source_file` API + 检查 `node.Symbol`/容器符号表/诊断/flow 节点，expected 取自 **TS 语义 spec** 与 **对当前 Go 实现的实测**（不靠 Rust 推断）。

### `BenchmarkBind`（基准，非单测）

| Rust 对应 | 说明 | Go 对照 | 完成 |
|---|---|---|---|
| `criterion` bench（推迟） | parse 后 `bind_source_file` 计时 | `binder_test.go:BenchmarkBind` | — (P10) |

### 行为级用例：符号表构建（依据：TS 语义 + Go 实测）

> 设计：`parse_source_file` → `bind_source_file` → 断言符号存在性/容器归属/Flags/Declarations 数。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `bind_single_var_creates_symbol` | 顶层变量建符号并回挂 | `"var x = 1;"` → 文件作用域符号表含 `x`，`SymbolFlagsFunctionScopedVariable`，1 declaration | TS 语义 | ✓ |
| `bind_function_declaration_symbol` | 函数声明符号 | `"function f(){}"` → 含 `f`，`SymbolFlagsFunction` | TS 语义 | ✓ |
| `bind_var_merge` | 同名 `var` 合并为一个符号 | `"var x; var x;"` → 1 个符号 `x`，`Declarations.len()==2`，无诊断 | TS 语义 | ✓ |
| `bind_let_redeclare_conflict` | 块级重声明报错 | `"let x; let x;"` → 报 `Cannot_redeclare_block_scoped_variable_0`（名=x） | `declareSymbolEx` | ✓ |
| `bind_duplicate_identifier` | 不兼容同名冲突 | `"class C {} class C {}"`（DIVERGENCE：tests.md 原例 `class C{} function C(){}` 在 binder 层合并、由 checker 报错；改用双 class 这类 binder 真正会报的不可合并对）→ 报 `Duplicate_identifier_0` | `declareSymbolEx` | ✓ |
| `bind_function_locals_scope` | 函数局部不漏到文件 | `"function f(){ var y; }"` → `y` 在 `f` 的 locals，文件 locals 无 `y` | TS 语义 | ✓ |
| `bind_class_members` | 类成员进 members 表 | `"class C { m(){} p = 1; }"` → `C` 符号的 members 含 `m`(Method)、`p`(Property) | TS 语义 | ✓ |
| `bind_interface_merge` | 接口合并 | `"interface I{a:number} interface I{b:string}"` → 1 个 `I` 符号，2 declarations | TS 语义 | ✓ |
| `bind_export_const` | 模块导出 | `"export const a = 1;"` → 文件 symbol 的 exports 含 `a` | TS 语义 | ✓ |
| `bind_multiple_default_exports` | 多默认导出报错 | `"export default 1; export default 2;"` → 报 `A_module_cannot_have_multiple_default_exports` | `declareSymbolEx` | ✓ |
| `bind_enum_namespace_merge` | enum 与不可合并者冲突 | `"enum E{} var E;"` → 报 `Enum_declarations_can_only_merge_with_namespace_or_other_enum_declarations`（按 Go 实测确认触发分支） | `declareSymbolEx` | ✓ |
| `bind_private_identifier_name` | 私有标识符符号名 | `"class C { #x = 1; }"` → `#x` 符号名 = `GetSymbolNameForPrivateIdentifier(C, "#x")` 格式（`__#<id>@#x`） | `GetSymbolNameForPrivateIdentifier` | ✓ |

### 行为级用例：控制流图（依据：TS flow 语义 + Go 实测）

> 设计：bind 后检查相关 AST 节点的 `FlowNode`/`Flags`/前驱结构（最小结构断言，不要求全图对拍）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `flow_if_creates_condition_nodes` | if 产生 true/false 条件流 + 汇合 | `"let x; if (x) {} else {}"` → 生成 `FlowFlagsTrueCondition`/`FlowFlagsFalseCondition` 节点，汇合 branch label | `bindIfStatement`/`createFlowCondition` | ✓ |
| `flow_unreachable_after_return` | return 后不可达 | `"function f(){ return; let y; }"` → `let y` 处 currentFlow 为 `FlowFlagsUnreachable`（观察点：节点被打上 `NodeFlagsUnreachable`） | `bindReturnStatement` | ✓ |
| `flow_finish_label_single_folds` | 单前驱 label 折叠 | 构造仅一个前驱的 branch label → `finish_flow_label` 返回该前驱本身（非 label） | `finishFlowLabel` | ✓ |
| `flow_add_antecedent_dedup` | 前驱去重 | 对同一 label 重复 `add_antecedent(同一节点)` → Antecedents 不重复 | `addAntecedent` | ✓ |
| `flow_while_loop_label` | 循环 label | `"while (true) { break; }"` → 生成 loop label + break 汇合到 post-loop | `bindWhileStatement` | ✓ |

### 行为级用例：杂项工具（纯/半纯）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `find_use_strict_prologue_detected` | 识别 `"use strict"` | statements 以 `"use strict";` 开头 → 返回该语句节点；否则 `None` | `FindUseStrictPrologue` | ✓ |
| `get_container_flags_function` | 容器 flags 判定 | 函数声明节点 → 含 `IsContainer|IsFunctionLike|IsControlFlowContainer|HasLocals` | `GetContainerFlags` | ✓ |
| `set_value_declaration_sets` | 设置值声明 | 对符号 `set_value_declaration(sym, node)` → `sym.ValueDeclaration==node` | `SetValueDeclaration` | ✓ |

## §8.6 每函数补充单测（wave 2 实现）

除上述行为级用例外，按 PORTING §8.6 为辅助/谓词函数补了直接单测（每个 `<stem>.rs` 配兄弟 `<stem>_test.rs`，均带 `// Go:` 锚，gate C6 绿）：

| Rust 测试文件 | 覆盖 | 完成 |
|---|---|---|
| `astquery_test.rs` | `name_of_declaration` / `is_property_name_literal` / `is_block_or_catch_scoped` / `is_potentially_executable_node` / `declaration_name_to_string` | ✓ |
| `flow_test.rs` | `is_narrowing_expression`（标识符 vs `true` 关键字）+ 上述 5 条流用例 | ✓ |
| `nameresolver_test.rs` | `NameResolver::new` / `get_local_symbol_for_export_default`（无声明 → None） | ✓ |
| `referenceresolver_test.rs` | `new_reference_resolver` / 各 `GetReferenced*` 推迟查询返回 None | ✓ |

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/binder/binder.go:<Func>`，因 Go 侧无直接单测）
- [x] 每个 Go `func Test*` 都已映射 → **N/A：Go 侧 0 个 `Test*`**（唯一 `BenchmarkBind` 标 `— (P10)`，已声明 P10 兜底）
- [x] 每个表驱动子用例都已逐行列出 → N/A（同上）
- [x] 行为级 expected 取自 **TS 语义 / Go 实测**（非 Rust 推断）
- [x] 每条用例都有 impl.md 承载：建符号→`declare_symbol_ex`；冲突诊断→`declare_symbol_ex` 分支；容器→`bind_container`/`get_container_flags`；流图→`create_flow_condition`/`finish_flow_label`/`add_antecedent`/`bind_if_statement`
- [x] 与 impl.md 双向对齐：impl 强调的 Symbol arena / Flow arena / 合并冲突 / 流图汇合在此均有测试承载

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `BenchmarkBind` | 需 `fixtures.BenchFixtures` 大文件 | P10 |
| 全量符号表 / flow 图 parity | 经 checker→emit 的 conformance/fourslash baseline 端到端兜底 | P10 |
| `NameResolver.Resolve` 在真实程序上的解析对齐 | 需 checker hook（`GetSymbolOfDeclaration` 等） | P4 / P10 |
| `ReferenceResolver` 各 `GetReferenced*` | emit/转换阶段才被调用 | P5 / P10 |
