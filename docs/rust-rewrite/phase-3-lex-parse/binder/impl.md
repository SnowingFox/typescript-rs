# binder: 实现方案（impl.md）

**crate**：`tsgo_binder`　**目标**：遍历一棵 `ast.SourceFile`，为其中的声明建立 **Symbol 表**（locals/exports/members）、把声明回挂到节点（`node.Symbol`），并构建**控制流图（flow graph，FlowNode/FlowLabel）**——这是 checker（P4）做名字解析、类型推断与流敏感窄化的前置数据结构。
**依赖（crate）**：`tsgo_ast` `tsgo_scanner` `tsgo_core` `tsgo_collections` `tsgo_debug` `tsgo_diagnostics` `tsgo_tspath`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/binder/`（3 个非测试文件：`binder.go` 119KB / `nameresolver.go` 23KB / `referenceresolver.go` 10KB）

## 这个包是什么（业务说明）

parser 给出语法树后，binder 做**语义预处理**。一次 `BindSourceFile(file)` 完成两件大事：

1. **建符号表（Symbol table）**：自顶向下遍历 AST，识别每个"声明"（变量/函数/类/接口/枚举/模块/参数/属性/import/export…），用 `declareSymbol` 把它登记进所在**容器**（container）的符号表。容器有三种表：`locals`（块/函数作用域局部）、`exports`（模块/命名空间导出）、`members`（类/接口/对象类型成员）。同名声明按规则**合并**（merge，如多个 `var`、`interface` + `interface`、`namespace` + `function`）或**报冲突**（`Duplicate identifier`、`Cannot redeclare block-scoped variable`、`A module cannot have multiple default exports` 等）。每个 `Symbol` 记 `Flags`（种类）、`Declarations`、`ValueDeclaration`、`Parent`、`Members`/`Exports`。`node.Symbol` 反向指回。

2. **建控制流图（flow graph）**：在遍历语句/表达式时维护"当前流节点"`currentFlow`，为分支（if/三元/`&&`/`||`/`??`）、循环（for/while/do/for-in/for-of）、跳转（break/continue/return/throw）、switch、try/catch/finally、赋值与可能的断言调用等创建 `FlowNode`（带 `FlowFlags` 与前驱 `Antecedent`/`Antecedents`）与汇合点 `FlowLabel`。这些 flow 节点挂到相关 AST 节点（`node.FlowNode`），供 checker 做"此处 `x` 的类型被窄化成什么"。

binder 还顺带：识别 `"use strict"` prologue、容器/this-容器/block-scope 容器切换（`bindContainer` + `GetContainerFlags`）、JS 文件的 CommonJS/expando 赋值模式（`module.exports =`、`this.x =`、`exports.foo =`、`Object.defineProperty`）、收集 `classifiableNames`、延迟绑定 expando 赋值。

`nameresolver.go` 提供 `NameResolver.Resolve(location, name, meaning, ...)`——沿作用域链向上查名字解析成符号（checker 复用，binder 内也用），含一大套作用域可见性规则。`referenceresolver.go` 提供 `ReferenceResolver` 接口（emit/转换阶段查"某标识符引用的导出容器/导入声明/值声明"）。

为什么在 P3：binder 依赖 ast（P2）、parser+scanner（P3），是 checker（P4）的直接前置。**binder 产出的 Symbol 表 + flow 图正是 P4 checker 的输入地基**。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。Symbol/Flow 图是与 AST 并列的**第二张图**，沿用 arena 思路。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `*ast.Symbol`（共享、长生命周期、可变） | `SymbolId`（arena 索引，P2/本包定义）/ `Option<SymbolId>` | **Symbol 图用 arena + `SymbolId`**（PORTING §5 同 AST）。`symbol.Parent`/`Members`/`Exports`/`Declarations` 全用 `SymbolId`/`Vec<NodeId>`，零 `unsafe`。binder 持 `symbolArena: Arena<Symbol>`。 |
| `ast.SymbolTable = map[string]*Symbol` | `SymbolTable = FxHashMap<String, SymbolId>` 或 `IndexMap`（若影响 emit/诊断顺序） | **顺序敏感**：成员/导出顺序影响诊断与 emit。Go 的 `map` 无序但 TS 语义靠 `Declarations` 顺序定。决策：若某表的迭代顺序进入输出，用 `IndexMap`（见 PORTING §3）；纯查找用 `FxHashMap`。执行期逐表核对。 |
| `*ast.FlowNode`（图，含环、共享前驱） | `FlowNodeId`（arena 索引）/ `Option<FlowNodeId>` | **Flow 图用 arena + `FlowNodeId`**。`FlowNode{Flags, Node: Option<NodeId>, Antecedent: Option<FlowNodeId>, Antecedents: Option<FlowListId>}`。`FlowLabel = FlowNode`（type alias，Go 同）。`binder` 持 `flowNodeArena`/`flowListArena`。 |
| `ast.FlowList{Flow *FlowNode; Next *FlowList}`（链表） | `FlowList{flow: FlowNodeId, next: Option<FlowListId>}` + `flowListArena: Arena<FlowList>` | label 的前驱集是单链表；用 arena 索引避免 `Box` 环问题。`combineFlowLists` 递归直译。 |
| `type Binder struct { ...大量 current* 状态... }` | `struct Binder { ... }`（短命可变状态机，`&mut self`） | 不是图节点；遍历期的"当前容器/流/break-continue 目标/异常目标/true-false 目标/活动标签链"等都是可变字段。 |
| `currentFlow/currentBreakTarget/...`（遍历游标） | `current_flow: FlowNodeId`、`current_break_target: Option<FlowNodeId>`… | 进入/退出结构时 save/restore（Go 用局部变量暂存再恢复）。Rust 同：函数内 `let saved = self.current_x; ...; self.current_x = saved;`。 |
| `ActiveLabel{ next *ActiveLabel; ... }`（标签链） | `ActiveLabel{ ... }` + `Vec<ActiveLabel>` 栈 或 arena 链 | labeled statement 的 break/continue 目标链。用 `Vec` 栈更地道（Go 用侵入式链表）。 |
| `sync.Pool`（binderPool） | `thread_local!` 池 或每次 `Binder::new()` | 同 parser，`// PERF(port): binder pool`。 |
| `ContainerFlags int32` / `SymbolFlags` / `FlowFlags` / `NodeFlags`（iota/位） | `bitflags!`（各一个） | `ContainerFlags`（本包定义）、`SymbolFlags`/`FlowFlags`/`NodeFlags`（P2 ast 定义）。 |
| `file.BindOnce(func(){...})`（一次性绑定锁） | `OnceCell`/`Once` 或 SourceFile 的 `bind_once(|| ...)`（P2 提供） | 保证一个 SourceFile 只绑定一次（并发安全）。`IsBound()` 快路径。 |
| `collections.Set[*ast.Symbol]`（notConstEnumOnlyModules） | `FxHashSet<SymbolId>` | 用 `SymbolId` 作键（不是裸指针）。 |
| `core.Arena[ast.Symbol]`/`[ast.FlowNode]`/`[ast.FlowList]`/`[*ast.Node]` | `Arena<Symbol>`/`Arena<FlowNode>`/`Arena<FlowList>`/`Arena<NodeId>`（P1 core::Arena） | 批量分配，省 GC/分配开销。`newSingleDeclaration` = `NewSlice1`。 |
| 闭包 `bindFunc = b.bind`（复用） | 方法 `Self::bind`（无需常驻闭包字段） | Go 把方法存成闭包以省分配；Rust 直接调方法。 |
| panic（`"Existing symbol parent should match new one"` 等） | `panic!`/`debug_assert!` | 1:1 保留不变量断言。 |
| `NameResolver{ 一堆回调字段 }` | `struct NameResolver { ... }` + 回调字段 `Box<dyn Fn...>`/泛型 | checker 注入大量 hook（`GetSymbolOfDeclaration`/`Error`/`Lookup`/…）。Rust 用闭包字段或 trait；`// TODO(port)`：与 checker 同阶段定接口形态（可能用 trait 对象）。 |
| `ReferenceResolver interface` + `referenceResolver struct` | `trait ReferenceResolver` + `struct ReferenceResolverImpl` | Go 接口 → Rust trait；`NewReferenceResolver` 返回 `Box<dyn ReferenceResolver>` 或具体类型。 |

### Symbol / Flow 所有权图（命门）

- **两张 arena 图**：AST（NodeId，P2）与 Symbol/Flow（SymbolId/FlowNodeId，本包+P2）。跨图引用都用索引：`node.symbol: Option<SymbolId>`、`symbol.declarations: Vec<NodeId>`、`symbol.value_declaration: Option<NodeId>`、`flow_node.node: Option<NodeId>`。**绝不用 Rust `&` 跨图**——这正是零 `unsafe` 表达环/反向指针/绑定期可变的关键。
- **绑定期可变**：binder 会改 `symbol.Flags`/追加 `Declarations`/设 `Parent`/建 `Members`。在 arena 模型里就是 `arena[symbol_id].flags |= ...`，借用范围清晰。
- **flow 节点共享与去重**：`addAntecedent` 检查重复、`setFlowNodeReferenced` 设 `Referenced`/`Shared`、`finishFlowLabel` 把单前驱 label 折叠成其前驱、`createFlowCondition` 对常量条件/不可达做短路。逐函数 1:1。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/binder/binder.go` | `internal/binder/lib.rs`（crate 根） | `Binder`/`ContainerFlags`/`ActiveLabel`、`BindSourceFile`、`declareSymbol(Ex)`/`declareSymbolAndAddToSymbolTable`/`declareModuleMember`、符号名/显示名、各 `bindXxx`（声明/语句/表达式/流）、`bindContainer`/`bindChildren`/`GetContainerFlags`、flow 创建族（`newFlowNode`/`createBranchLabel`/`createFlowCondition`/`addAntecedent`/`finishFlowLabel`…）、`FindUseStrictPrologue`/`SetValueDeclaration`/`GetSymbolNameForPrivateIdentifier`。`mod nameresolver; mod referenceresolver;`。 |
| `internal/binder/nameresolver.go` | `internal/binder/nameresolver.rs` | `NameResolver` 结构 + `Resolve(...)`（作用域链查名）+ `GetLocalSymbolForExportDefault`。 |
| `internal/binder/referenceresolver.go` | `internal/binder/referenceresolver.rs` | `ReferenceResolver` trait + `ReferenceResolverHooks` + `referenceResolver` 实现 + `NewReferenceResolver`，`GetReferenced*`（导出容器/导入声明/值声明/成员值声明/元素访问名）。 |

## 依赖白名单（本包新增的 crate）

- `rustc_hash`（`FxHashMap`/`FxHashSet`）——符号表、`classifiableNames`、`notConstEnumOnlyModules`。
- `indexmap`（`IndexMap`/`IndexSet`）——**凡迭代顺序进入诊断/emit 的符号表**（执行期逐表判定）。
- `bitflags`——`ContainerFlags`（本包）+ 复用 ast 的 `SymbolFlags`/`FlowFlags`/`NodeFlags`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按 TDD 推进序：符号表机制 → 容器遍历 → 声明绑定 → 流图 → name/reference resolver。

### `lib.rs` — 入口与符号机制（Go: `binder.go`）

- [ ] `bitflags! ContainerFlags`（None/IsContainer/IsBlockScopedContainer/IsControlFlowContainer/IsFunctionLike/IsFunctionExpression/HasLocals/IsInterface/IsObjectLiteralOrClassExpressionMethodOrAccessor/IsThisContainer/PropagatesThisKeyword）　`// Go: binder.go:ContainerFlags`
- [ ] `struct Binder`（全部字段，见类型映射）+ `ActiveLabel`（+ `BreakTarget()`/`ContinueTarget()`）+ `ExpandoAssignmentInfo`　`// Go: binder.go:Binder/ActiveLabel/ExpandoAssignmentInfo`
- [ ] `pub fn bind_source_file(file)`（`IsBound()` 快路径 → `bind_source_file_inner`）　`// Go: binder.go:BindSourceFile/bindSourceFile`
- [ ] `fn bind_source_file_inner(file)`（`BindOnce`：取 binder→`unreachableFlow`→`bind(file)`→`bindDeferredExpandoAssignments`→回写 `SymbolCount`/`ClassifiableNames`/`NestedCJSExports`）　`// Go: binder.go:bindSourceFile`
- [ ] `fn new_symbol(flags, name)->SymbolId`（`symbolCount++` + arena 分配）　`// Go: binder.go:newSymbol`
- [ ] `fn declare_symbol(table, parent, node, includes, excludes)->SymbolId`　`// Go: binder.go:declareSymbol`
- [ ] `fn declare_symbol_ex(table, parent, node, includes, excludes, is_replaceable_by_method, is_computed_name)->SymbolId`（**核心**：默认导出名、missing 名、查表、`isReplaceableByMethod` 规则、`excludes` 冲突 → `Duplicate_identifier`/`Cannot_redeclare_block_scoped_variable`/`A_module_cannot_have_multiple_default_exports`/enum-merge 诊断 + related info、accessor 升级、`addDeclarationToSymbol`、设 `Parent`）　`// Go: binder.go:declareSymbolEx`
- [ ] `fn get_declaration_name(node)->Option<String>`（export=/default、ambient module 引号名/global、private id、属性名字面量、computed 字面量/signed numeric、构造/call/new/index/export*/exportEquals 内部名）　`// Go: binder.go:getDeclarationName`
- [ ] `fn get_display_name(node)->String`　`// Go: binder.go:getDisplayName`
- [ ] `pub fn get_symbol_name_for_private_identifier(class_symbol, desc)->String`　`// Go: binder.go:GetSymbolNameForPrivateIdentifier`
- [ ] `fn declare_module_member(node, flags, excludes)->SymbolId` / `declare_symbol_and_add_to_symbol_table(node, flags, excludes)->SymbolId` / `addDeclarationToSymbol`（在 ast 或本包）　`// Go: binder.go:declareModuleMember/declareSymbolAndAddToSymbolTable`
- [ ] `pub fn set_value_declaration(symbol, node)`　`// Go: binder.go:SetValueDeclaration`

### `lib.rs` — 容器与遍历（Go: `binder.go`）

- [ ] `fn bind(&mut self, node)->bool`（总分发：按 Kind 调对应 `bindXxx`，设 `node.Symbol`/`node.FlowNode`，递归 `bindContainer`/`bindChildren`）　`// Go: binder.go:bind`
- [ ] `fn bind_container(node, container_flags)`（save/restore container/thisContainer/blockScopeContainer/currentFlow 等，按 flags 初始化新流）　`// Go: binder.go:bindContainer`
- [ ] `fn bind_children(node)` / `bind_each_child` / `bind_each(nodes)` / `bind_node_list` / `bind_modifiers` / `bind_each_statement_functions_first`　`// Go: binder.go:bindChildren/bindEach*/bindModifiers`
- [ ] `pub fn get_container_flags(node)->ContainerFlags`（按 Kind 大 match）　`// Go: binder.go:GetContainerFlags`
- [ ] `pub fn find_use_strict_prologue(sf, statements)->Option<NodeId>`　`// Go: binder.go:FindUseStrictPrologue`

### `lib.rs` — 声明绑定族（Go: `binder.go`）

- [ ] `bind_module_declaration`/`bind_namespace_export_declaration`/`bind_import_clause`/`bind_export_declaration`/`bind_export_assignment`　`// Go: binder.go:bind*Declaration/bind*Export*`
- [ ] `bind_class_like_declaration`/`bind_property_or_method_or_accessor`/`bind_property_worker`/`bind_function_or_constructor_type`　`// Go: binder.go:bindClassLikeDeclaration/...`
- [ ] `bind_function_expression`/`bind_function_declaration`/`bind_call_expression`/`bind_parameter`/`bind_type_parameter`　`// Go: binder.go:bindFunction*/bindCallExpression/bindParameter/bindTypeParameter`
- [ ] `bind_variable_declaration_or_binding_element`/`bind_block_scoped_declaration`/`bind_anonymous_declaration`/`bind_enum_declaration`/`bind_jsx_attributes`/`bind_jsx_attribute`　`// Go: binder.go:bind*`
- [ ] JS/CommonJS：`bind_module_exports_assignment`/`bind_expando_property_assignment`/`bind_deferred_expando_assignment(s)`/`bind_common_js_type_exports`/`bind_exports_or_object_define_property`/`bind_this_property_assignment`　`// Go: binder.go:bind*Expando*/bindModuleExportsAssignment/bindExportsOrObjectDefineProperty/bindThisPropertyAssignment`

### `lib.rs` — flow 图创建（Go: `binder.go`）

- [ ] `fn new_flow_node(flags)->FlowNodeId` / `new_flow_node_ex(flags, node, antecedent)->FlowNodeId`　`// Go: binder.go:newFlowNode/newFlowNodeEx`
- [ ] `create_loop_label`/`create_branch_label`/`create_reduce_label`　`// Go: binder.go:createLoopLabel/createBranchLabel/createReduceLabel`
- [ ] `create_flow_condition(flags, antecedent, expr)->FlowNodeId`（不可达短路、常量条件、`isNarrowingExpression`）　`// Go: binder.go:createFlowCondition`
- [ ] `create_flow_mutation`/`create_flow_switch_clause`/`create_flow_call`（设 `hasFlowEffects`、异常目标 antecedent）　`// Go: binder.go:createFlow*`
- [ ] `new_flow_list(head, tail)->FlowListId` / `combine_flow_lists` / `new_single_declaration(decl)->Vec<NodeId>`　`// Go: binder.go:newFlowList/combineFlowLists/newSingleDeclaration`
- [ ] `fn set_flow_node_referenced(flow)`（Referenced→Shared）/ `add_antecedent(label, antecedent)`（去重追加）/ `finish_flow_label(label)->FlowNodeId`（空→unreachable、单→折叠）　`// Go: binder.go:setFlowNodeReferenced/addAntecedent/finishFlowLabel`

### `lib.rs` — 语句/表达式流绑定族（Go: `binder.go`）

- [ ] 条件/循环：`bind_condition`/`bind_iterative_statement`/`bind_while_statement`/`bind_do_statement`/`bind_for_statement`/`bind_for_in_or_for_of_statement`/`bind_if_statement`　`// Go: binder.go:bind*Statement`
- [ ] 跳转：`bind_return_statement`/`bind_throw_statement`/`bind_break_statement`/`bind_continue_statement`/`bind_break_or_continue_statement`/`bind_break_or_continue_flow`　`// Go: binder.go:bind*`
- [ ] try/switch/labeled：`bind_try_statement`/`bind_switch_statement`/`bind_case_block`/`bind_case_or_default_clause`/`bind_labeled_statement`/`bind_expression_statement`　`// Go: binder.go:bind*`
- [ ] 表达式流：`bind_prefix/postfix_unary_expression_flow`/`bind_binary_expression_flow`/`bind_logical_like_expression`/`bind_destructuring_assignment_flow`/`bind_destructuring_target_flow`/`bind_assignment_target_flow`/`bind_delete_expression_flow`/`bind_conditional_expression_flow`/`bind_variable_declaration_flow`/`bind_initialized_variable_flow`　`// Go: binder.go:bind*Flow`
- [ ] 辅助谓词：`is_narrowing_expression`/`isNarrowableReference` 等（在 binder.go 后段）　`// Go: binder.go:isNarrowing*`

### `nameresolver.rs`（Go: `internal/binder/nameresolver.go`）

- [ ] `struct NameResolver { CompilerOptions, GetSymbolOfDeclaration, Error, Globals, ArgumentsSymbol, RequireSymbol, Lookup, SymbolReferenced, ...hooks }`　`// Go: nameresolver.go:NameResolver`
- [ ] `fn resolve(&mut self, location, name, meaning, name_not_found_message, is_use, exclude_globals)->Option<SymbolId>`（作用域链向上：locals/`isGlobalSourceFile` 跳过、函数 like 的类型参数/参数可见性、各容器 exports/members、`const` as-const 特判、did-you-mean 收集、失败/成功回调）　`// Go: nameresolver.go:Resolve`
- [ ] `pub fn get_local_symbol_for_export_default(symbol)->Option<SymbolId>`　`// Go: nameresolver.go:GetLocalSymbolForExportDefault`

### `referenceresolver.rs`（Go: `internal/binder/referenceresolver.go`）

- [ ] `trait ReferenceResolver`（6 方法）+ `struct ReferenceResolverHooks`（8 回调）　`// Go: referenceresolver.go:ReferenceResolver/ReferenceResolverHooks`
- [ ] `struct ReferenceResolverImpl { resolver, options, hooks }` + `pub fn new_reference_resolver(options, hooks)->impl ReferenceResolver`　`// Go: referenceresolver.go:referenceResolver/NewReferenceResolver`
- [ ] `get_referenced_export_container`/`get_referenced_import_declaration`/`get_referenced_value_declaration(s)`/`get_element_access_expression_name`/`get_referenced_member_value_declaration`（+ 内部 `getResolvedSymbol`/`getMergedSymbol` 等 hook 包装）　`// Go: referenceresolver.go:GetReferenced*/getResolvedSymbol/...`

### Cargo / crate 接线

- [ ] `internal/binder/Cargo.toml`（`name = "tsgo_binder"` + path deps：ast/scanner/core/collections/debug/diagnostics/tspath）
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/binder`
- [ ] `lib.rs` 声明 `mod nameresolver; mod referenceresolver;` + re-export（`bind_source_file`/`NameResolver`/`ReferenceResolver`/`NewReferenceResolver`/`GetContainerFlags`/`SetValueDeclaration`/`FindUseStrictPrologue`/`GetSymbolNameForPrivateIdentifier`/`GetLocalSymbolForExportDefault`）

## TDD 推进顺序（tracer bullet → 增量）

1. **符号机制 tracer**：`new_symbol` + `declare_symbol` + 最小 `bind`（只处理 SourceFile + VariableStatement + 标识符声明）→ 断言 `file` 顶层符号表含 `x`，`node.Symbol` 回挂。配 tests.md 行为级用例 `bind_single_var_creates_symbol`。
2. **合并/冲突**：两个 `var x` 合并成一个 Symbol（2 declarations）；`let x; let x;` → `Cannot redeclare block-scoped variable` 诊断。
3. **容器**：`function f(){ var y; }` → `y` 在 f 的 locals，不在文件 locals；`class C { m(){} }` → `m` 在 C 的 members。
4. **导出**：`export const a = 1` → 模块 exports 含 `a`；多个 `export default` → 报 multiple default exports。
5. **flow 图最小**：`if (c) {} else {}` → 生成 branch label + true/false condition flow，汇合 `finishFlowLabel`。逐步加 while/for/switch/try。
6. **nameresolver**：`Resolve` 在简单作用域链上查到符号（无 checker hook 时用桩）。
7. 全量正确性 gate → P10（见 tests.md）。

## 与 Go 的已知偏离（divergence）

- **Symbol/Flow 用 arena+Id（非裸指针）**：见 PORTING §5。`symbol.Parent`/`Members`/`Exports`、`flowNode.Antecedent(s)`、`node.Symbol`/`node.FlowNode` 全用 `SymbolId`/`FlowNodeId`/`FlowListId`。这是零 `unsafe` 表达 symbol-merge（绑定期可变）与 flow 图（环/共享前驱）的关键，**属必要偏离**。
- **`SymbolTable` 顺序**：Go `map[string]*Symbol` 无序；凡迭代顺序进入诊断/emit 的表改 `IndexMap`（执行期逐表核对，宁可保守用 `IndexMap`）。
- **`ActiveLabel` 链 → `Vec` 栈**：Go 侵入式单链表，Rust 用 `Vec<ActiveLabel>` 栈更地道（语义等价）。
- **`NameResolver`/`ReferenceResolver` 的回调字段**：Go 用函数字段注入 checker 行为。Rust 用 `Box<dyn Fn>`/trait；具体形态与 checker（P4）协同定。本轮文档先用闭包字段表达。
- **`sync.Pool` → 暂不池化/`thread_local`**：`// PERF(port): binder pool`。
- **`BindOnce`/`IsBound`**：靠 P2 SourceFile 的一次性绑定原语（`OnceCell`/`Once`）。

## 转交 / 推迟（DEFER）

- **正确性 gate 几乎全靠 P10**：Go 侧本包**无任何 `Test*`**（`binder_test.go` 仅 `BenchmarkBind`，见 tests.md），符号表/流图的正确性由 checker→emit 的 conformance/fourslash baseline 端到端兜底。本轮补行为级 Rust 测试覆盖关键路径（建符号、合并、冲突诊断、最小流图）。
- **`BenchmarkBind`**：`// DEFER(phase-10)`（需 `fixtures.BenchFixtures`）。
- **`NameResolver`/`ReferenceResolver` 与 checker 的精确接口**：`// DEFER(phase-4-checker)`——回调签名、`GetSymbolOfDeclaration` 等 hook 在 checker 阶段最终确定；本包先提供结构与 `Resolve` 主体。
