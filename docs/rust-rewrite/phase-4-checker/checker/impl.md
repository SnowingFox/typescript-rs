# checker: 实现方案（impl.md）

**crate**：`tsgo_checker`　**目标**：TypeScript 的**类型检查器**——全仓最大、最硬的模块。负责符号解析、类型构造与实例化、子类型/赋值关系判定、类型推断、控制流分析与类型收窄、表达式/语句/JSX 的类型检查与诊断、`.d.ts` 序列化（node builder）、以及给 emit/语言服务用的查询 API。
**依赖（crate）**：`tsgo_ast` `tsgo_binder` `tsgo_collections` `tsgo_core` `tsgo_debug` `tsgo_diagnostics` `tsgo_evaluator` `tsgo_jsnum` `tsgo_module` `tsgo_modulespecifiers` `tsgo_scanner` `tsgo_tracing` `tsgo_tsoptions` `tsgo_tspath`，外加 `xxh3`（hash）
**Go 源**：`internal/checker/`（24 个非测试文件，**59,514 行**）

---

## checker 移植状态（4a–4p 收口 · 给下一 phase 看的"接缝"）

> **状态：checker 移植子阶段（4a–4k）+ 4l（program 保留 + pool 驱动面）+ 4m（变量声明赋值性 2322 + block 递归）+ 4n（赋值表达式赋值性 2322 + 语句容器递归 if/else/while/do/for/try/catch/finally）+ 4o（非赋值二元运算符：关系/相等 → boolean + 可比较性诊断 2365/2367、非 `+` 算术 → number + 操作数诊断 2362/2363；语句容器递归 switch/for-in/for-of）+ 4p（逻辑运算符 `&&`/`||`/`??` 结果类型可达子集、`+`/`+=` 运算符结果类型 string/number/bigint/any + 不可应用 2365、复合赋值 `+=`/`*=`/…/`&&=`/`||=`/`??=` 经 `check_assignment_operator` 校验、语句 `throw`/labeled 递归）已收口、gate 全绿（183 单测 + 110 doctest，clippy/fmt 干净）。** 这是一个**可达核心**的纵向切片，不是全量 checker；剩余深化 + 端到端正确性由 **P10 conformance** 兜底，跨 phase 依赖标注于下。
>
> ⚠️ **4l 破坏 compiler 调用点（需下一轮 compiler 适配）**：`new_checker` 签名由 `&dyn BoundProgram` 改为 `Rc<dyn BoundProgram>`（真正保留 program）。`internal/compiler/checkerpool.rs` 的 `Checker::new_checker(&seed)`（`seed: BoundFile<'a>`）将不再编译——compiler 需提供一个 **owned/`Rc`-shared 且 `'static`** 的 `BoundProgram`（当前借用式 `BoundFile<'a>` 无法塞进 `Rc<dyn BoundProgram + 'static>`），随后 pool 改为 `new_checker(Rc::clone(&program))` + 每文件 `checker.get_diagnostics(file)`。详见 4l worklog。

**已移植（各子系统可达核心，均严格 TDD + §8.6 每 `pub fn` 有测）**
- **4a 地基**：`Type`/`Symbol` arena（`TypeId` 句柄，无 `Type.checker` 反向指针）、`TypeFlags`/`ObjectFlags`、intrinsic 类型、`Checker` 骨架、`Tracer`（checkerId 注入）。`core/types.rs`/`core/mod.rs`/`tracer.rs`
- **4b 符号**：`resolve_name`/`get_symbol_at_location`/`skip_alias` + link stores + `Literal`/`Union` + 桩 `BoundProgram`/`StubProgram`。`core/symbols*.rs`/`core/program.rs`
- **4c 声明类型**：`get_declared_type_of_symbol`（interface/class/type-alias/enum）/`get_type_of_symbol`/`get_property_of_type`/`get_global_type` + `Signature`/`IndexInfo` arena。`core/declared_types.rs`/`core/signatures.rs`
- **4d 实例化+关系**：`TypeMapper`/`instantiate_type`/`instantiate_signature`、类型参数/泛型 interface/`TypeReference`、`is_type_related_to`（identity/subtype/assignable/comparable + 结构化）。`core/mapper.rs`/`core/relations.rs`
- **4e 推断**：`InferenceContext`/`infer_types`/`get_inferred_type(s)`/`infer_type_arguments` + best-common + subtype-reduce + 成员实例化。`core/inference.rs`
- **4f 控制流/收窄**：`get_flow_type_of_reference`（flow 遍历 + 缓存）+ typeof/truthiness/equality/`in` 收窄 + 可达性。`core/flow.rs`
- **4g 表达式/诊断**：`check_source_file`→`check_expression`（字面量/标识符/属性/元素访问）+ `Diagnostic`/`get_diagnostics`（2304/2339）+ 调用解析起步 + `TypeFacts`（truthiness 子集）。`core/check.rs`/`core/type_facts.rs`
- **4h JSX**：`check_jsx_element`/自闭/片段 + 内在/值解析（注入 `JSX.IntrinsicElements`）+ props 可赋值（2322）+ children。`core/jsx.rs`
- **4i 语法检查**：`check_grammar_modifiers`（重复 1030 / ambient 1040 / 可访问性 1028）。`core/grammar.rs`
- **4j node builder / type→string**：`type_to_string`（命名/引用/匿名成员/union）+ `symbol_to_string`，诊断显示真名。`core/nodebuilder.rs`
- **4k emit resolver + 收口**：`EmitResolver`（`is_declaration_visible`/`serialize_type_of_declaration`/`is_implementation_of_overload`）+ `get_emit_resolver`(OnceCell) + `new_checker(program)` 前向入口。`core/emit_resolver.rs`
- **4l program 保留 + pool 驱动面**：`new_checker(Rc<dyn BoundProgram>)` 真正保留 program（`Checker.program` 字段，Go `c.program`）+ `program()` 访问器；`check_source_file(file)` / `get_diagnostics(file)` 改为基于**保留的** program 工作（Go-faithful，去掉 per-call program 参数）+ 幂等守卫（`checked_files`，Go `sourceFileLinks.typeChecked`）。这是多-checker pool 的干净驱动面：`new_checker(Rc::clone(&program))` → 每文件 `get_diagnostics(file)`。`core/mod.rs`/`core/check.rs`
- **4m 变量声明赋值性 + block 递归**：`check_statement` 新增 `VariableStatement` 臂——`check_variable_declaration` 对带注解+初始化器的声明经 4d 关系引擎校验初始化器类型可赋值于注解类型，不符→`Type_0_is_not_assignable_to_type_1`(2322)；错误消息里 literal 源经 `generalized_source_for_error`/`get_base_type_of_literal_type`（Go `reportRelationError` 的 `generalizedSource`）广义化（`"s"`→`string`）。并新增 `Block` 臂（Go `checkBlock`→`checkSourceElements`）使嵌套语句也被检查。全部**私有 fn**（公开 API 不变，compiler 保持绿）。`core/check.rs`
- **4n 赋值表达式赋值性 + 语句容器递归**：`check_expression` 新增 `BinaryExpression` 臂（Go `checkBinaryExpression`→`checkBinaryLikeExpression`）——`check_binary_expression` 求左右操作数类型（始终求值，故操作数内诊断可见），`EqualsToken` 经 `check_assignment_operator`（Go `checkAssignmentOperator` 的 `KindEqualsToken` 臂）对引用式 LHS 校验 RHS 可赋值，不符→`2322`（错误节点=LHS，复用 4m `generalized_source_for_error`）；`is_reference_expression`（Go `checkReferenceExpression` 子集）守卫 LHS 为 identifier/属性访问/元素访问。`check_statement` 新增 `IfStatement`/`WhileStatement`/`DoStatement`/`ForStatement`/`TryStatement` 臂（Go `checkSourceElement` 的对应臂）递归检查 then/else/loop body/初始化器·条件·增量/try·catch·finally 块，使嵌套诊断浮现；抽出 `check_variable_declaration_list`（Go `checkVariableDeclarationList`）供变量语句与 for 初始化器复用。全部**私有 fn/私有臂**（公开 API 不变，compiler 保持绿）。`core/check.rs`
- **4o 非赋值二元运算符 + 语句容器递归（switch/for-in/for-of）**：`check_binary_expression` 新增非赋值运算符臂（Go `checkBinaryLikeExpression`）——关系 `<`/`>`/`<=`/`>=`（结果 `boolean`，操作数经 `get_base_type_of_literal_type_for_comparison` 广义化后经 `relational_operands_comparable`（any||双 number-ish||双非 number-ish 且 `are_types_comparable`）判定，不可比报 `2365` "Operator '{0}' cannot be applied to types '{1}' and '{2}'."）；相等 `==`/`!=`/`===`/`!==`（结果 `boolean`，经 `equality_operands_comparable`（双向 `is_type_equality_comparable_to`=target nullable||`is_type_comparable_to`）判定，不可比报 `2367` "This comparison appears to be unintentional..."，literal 源经 `get_base_type_of_literal_type` 广义化，Go `getBaseTypesIfUnrelated`）；非 `+` 算术 `-`/`*`/`/`/`%`/`**`/`<<`/`>>`/`>>>`/`&`/`|`/`^`（结果 `number`，每操作数经 `check_arithmetic_operand_type`（`is_type_assignable_to(_, number|bigint)`）校验，不符报 `2362`/`2363`，错误节点=操作数）；错误经 `report_binary_operator_error`（按运算符族选 2365/2367 消息 + `tsgo_scanner::token_to_string` 运算符文本）。`check_statement` 新增 `SwitchStatement` 臂（检查 switch 表达式 + 每个 case/default clause 的表达式与语句递归，Go `checkSwitchStatement`）与 `ForInOrOfStatement` 臂（检查初始化器 decl-list/expr + 迭代表达式 + body 递归，Go `checkForInStatement`/`checkForOfStatement`，覆盖 `for-in`+`for-of`）。全部**私有 fn/私有臂**（公开 API 不变，compiler 保持绿）。`core/check.rs`

- **4p 逻辑/`+`/复合赋值运算符 + `throw`/labeled 语句**：`check_binary_expression` 新增 `&&`/`&&=`（结果=左型，左可真 `has_type_facts(TRUTHY)`→`union(extractDefinitelyFalsyTypes(getBaseTypeOfLiteralType(右)), 右)`，strictNullChecks 未接线故走 Go 非 strict 分支）、`||`/`||=`（结果=左型，左可假 `has_type_facts(FALSY)`→`union(removeDefinitelyFalsyTypes(左)=getTypeWithFacts(TRUTHY), 右)`）、`??`/`??=`（结果=左型，nullish 精化 DEFER）、`+`/`+=`（双 number-like→number、双 bigint-like→bigint、任一 string-like→string、任一 any→any/error、否则 `report_binary_operator_error`→`2365` 返回 any）臂；helper `is_type_assignable_to_kind_strict`（Go `isTypeAssignableToKindEx(_,_,strict)` 子集，覆盖 STRING/NUMBER/BIG_INT_LIKE，top/void/nullable 在 strict 下不可达）、`get_union_dropping_never`（Go `getUnionType` 去 never）、`remove_definitely_falsy_types`/`extract_definitely_falsy_types`/`get_definitely_falsy_part_of_type`（可达子集，String/Number/BigInt 原始 falsy 字面量 DEFER）。复合赋值经 `check_assignment_operator`：算术族扩 `*=`/`/=`/`%=`/`**=`/`-=`/`<<=`/`>>=`/`>>>=`/`&=`/`|=`/`^=` token（操作数 2362/2363 + `leftOk&&rightOk` 时 `check_assignment_operator(左, 左型, number)`）、`+=`（仅有有效结果时校验，Go-faithful 跳过 2365 分支）、`&&=`/`||=`/`??=`（`check_assignment_operator(左, 左型, 右型)`）；free fn `is_compound_assignment`（Go `IsCompoundAssignment`）。`check_statement` 新增 `ThrowStatement`（检查抛出表达式，Go `checkThrowStatement`）与 `LabeledStatement`（递归被标注语句，Go `checkLabeledStatement`→`checkSourceElement`）臂。全部**私有 fn/私有臂/私有 free fn**（公开 API 不变，compiler 保持绿）。`core/check.rs`

**接缝 / 延迟（下一 phase 必读）**
- **`// blocked-by: lib globals (P6)`**：JSX `JSX.Element`/`JSX.IntrinsicElements` 真值（4h 用 `set_jsx_intrinsic_elements` 注入 + 返回 `any`）；`get_apparent_type` 的 primitive→global wrapper；`typeof "function"`/host-object；完整 `TypeFacts`；`get_global_type` 的 lib 类型。
- **`// blocked-by: compiler.Program (P6)`**：`new_checker` 的全局/lib 绑定 + 保留 program；多文件符号查询；跨模块别名解析（`is_value_aliased`/`skip_alias` 全量）；`is_declaration_visible` 的 module-vs-script 判定；`TestGetSymbolAtLocation` 真 program 版 / `BenchmarkNewChecker`。
- **P5 消费者驱动**：完整 `NodeBuilder` 门面（类型→节点、序列化为声明、scope 追踪、`SymbolTracker`）、emit resolver 其余方法（别名/可达性/常量值/装饰器元数据/`get_type_reference_serialization_kind`）。
- **后续子阶段深化（同 crate）**：表达式/语句检查的其余面（赋值/控制流语句/类/重载选择/上下文类型/未用检查）、关系引擎（variance/intersection/可选属性/`Ternary`）、推断（contravariant/mapped/conditional）、flow（assignment/switch/loop fixpoint/`instanceof`）、grammar 全量族、签名/array/tuple/intersection 的 `type_to_string`。
- **正确性兜底 = P10**：包内单测覆盖**每个 `pub fn` 的行为级**判定（Go 派生 expected）；TS 全量 conformance/`.d.ts`/fourslash 对拍在 P10。

---

> ⚠️ 本 impl.md **不逐函数枚举**（`checker.go` 一个文件就 ~3.2 万行、上千个方法）。改为：① 文件职责总览（逐文件一句话）；② 按**子系统**拆 TODO 并锚到对应文件；③ 给出**执行期再拆子阶段（4a..4k）**的强烈建议。函数级 TODO 在每个子阶段真正动工时，由该子阶段的 worklog 细化。

> 🔴 **超大文件拆分（PORTING §2 + 用户指定）**：`checker.go`（~3.2 万行）**不做成单个 `.rs`**。拆到子目录 **`internal/checker/core/`**，按子系统分多个内聚 `.rs`（如 `core/mod.rs` 持 `Checker` 结构+入口、`core/relations.rs`、`core/inference.rs`、`core/instantiation.rs`、`core/flow.rs`、`core/types.rs`、`core/symbols.rs` …），**每个 `.rs` 配兄弟 `<stem>_test.rs`**。其余大文件（`relater.go`/`nodebuilder.go` 等 > ~1500 行）同样按职责拆。每个 Rust 函数仍带 `// Go: internal/checker/<file>.go:<Func>` 锚（锚原 Go 文件）。在本文件底部维护"Go 源 → Rust 子文件"对照表。
>
> 🔴 **每函数单测（PORTING §8.6）**：尽管 checker 正确性主要靠 P10 conformance，**每个 `pub fn` / 行为不平凡的方法仍要有包内单测**（用确定性的小输入 / Go 已知语义做 expected），不能只靠 P10。Go 现有的 3 个测试 func 全量移植。

## 这个包是什么（业务说明）

checker 是编译器的"大脑"：binder 建好符号表后，checker 把符号映射成类型、在节点间传播类型、判断类型间关系、做推断与收窄、报类型错误，最后还要把内部类型反序列化成 AST 给声明 emit（`.d.ts`）和 hover 用。它的输入是绑定后的 `Program`（多个 `SourceFile` + 符号表 + 解析结果），输出是诊断集合 + 一组供 emit / 语言服务调用的查询方法（`GetSymbolAtLocation`、`GetTypeOfSymbol`、`GetContextualType`、`GetDiagnostics`…）。

它处于 Phase 4 末尾，依赖本 phase 前序的 `evaluator`/`module`/`modulespecifiers`/`nodebuilder`，以及 P1–P3 的全部地基（`ast`/`binder`/`scanner`/`core`/`diagnostics`…）。**它的正确性几乎无法靠包内单测保证**——Go 侧只有 2 个测试文件 3 个 func（`TestGetSymbolAtLocation` + `BenchmarkNewChecker` + `TestTracerPushPreservesEndArgMutations`）。绝大多数行为正确性由 **P10 的 conformance（`tests/cases/conformance/**`）/ fourslash / `.d.ts` baseline 端到端对拍**兜底。这是 TypeScript 团队自己的现实：检查器的"测试"就是数万个 conformance 用例。

## 所有权 / 类型映射（命门：Type / Symbol 图）

通用规则见 PORTING.md §3/§5。checker 是 §5 arena 模型最重的落地点。

### Type 图

Go 的 `Type` 是判别联合（同 ast.Node 模式）：

```
type Type struct { flags TypeFlags; objectFlags ObjectFlags; id TypeId; symbol *ast.Symbol; alias *TypeAlias; checker *Checker; data TypeData }
```

`data TypeData` 是接口，由 `IntrinsicType` / `LiteralType` / `UnionType` / `IntersectionType` / `ObjectType`(+embedded `StructuredType`/`TypeReference`/`InterfaceType`) / `TypeParameter` / `MappedType` / `ConditionalType` / `IndexType` / `IndexedAccessType` / `TemplateLiteralType` / `SubstitutionType` / `TupleType` / … 实现。Go 里 `Type` 是**堆指针 `*Type`**，靠 checker 里几十张 `map[...]*Type` 做 interning（`stringLiteralTypes`/`unionTypes`/`intersectionTypes`/`indexedAccessTypes`/…）。

**Rust 表示（PORTING §5 强制）**：
- 用 **arena 持有所有 `Type`**：`types: Arena<TypeData>`，`TypeId(u32)` newtype 做句柄。`Type` 的公共头（flags/objectFlags/id/symbol/alias）+ `enum TypeData { Intrinsic(...), Literal(...), Union(...), Object(Box<ObjectType>), ... }`。
- **所有类型引用一律用 `TypeId`**（union 的成员、type reference 的 target/typeArguments、mapper 的映射对等），不用 `&Type` / `Rc<Type>`。这样环、缓存、`checker` 反向指针都能零 `unsafe` 表达。
- interning 的 `map[...]*Type` → `FxHashMap<Key, TypeId>`（或 `IndexMap` 当顺序影响诊断时）。
- Go `Type.checker *Checker`（每个类型回指 checker）→ **删除该反向指针**；Rust 里类型操作都是 `checker.method(type_id, ...)`，checker 持 arena，无需类型回指。这是允许且必要的偏离（PORTING §5），impl 顶部注明。

### Symbol 图

Go 用 `core.Arena[ast.Symbol]`（`symbolArena`）+ 大量 `core.LinkStore[*ast.Symbol, XxxLinks]`（`valueSymbolLinks`/`aliasSymbolLinks`/`moduleSymbolLinks`/`declaredTypeLinks`/…，约 20 张）做"按符号挂载的惰性计算缓存"。`Signature` / `IndexInfo` 同样走 `core.Arena`。

**Rust 表示**：
- `Symbol` 用 arena + `SymbolId`（ast 包在 P2/P3 已确立的 `SymbolId` 句柄，checker 复用）。
- `LinkStore[K, V]` → `FxHashMap<K, V>`（K 为 `SymbolId`/`NodeId`/`SourceFileId`/`TypeId`）。Go 的 `LinkStore.Get` 返回 `*V`（惰性插入），Rust 用 `entry().or_default()` 或显式 `get_or_insert`。约 30 张 link store 全部如此。
- `Signature` → `Arena<Signature>` + `SignatureId`；`IndexInfo` → `Arena<IndexInfo>` + `IndexInfoId`。
- `TypeMapper`（`mapper.go`）是类型替换函数的具体化（`enum TypeMapper { Simple{source, target}, Array{...}, Composite{...}, Merged{...}, Function(...) }`）——映射的 source/target 都用 `TypeId`。

### Checker 状态本体

Go 的 `type Checker struct` 有 **~300 个字段**（intrinsic 类型单例、几十张 interning map、~30 张 link store、3 个 arena、关系对象、全局类型惰性 getter、flow/inference 复用栈…）。Rust 直译为一个大 `struct Checker`，字段一一对应；惰性 getter（`getGlobalXxxType func() *Type`）→ `OnceCell<TypeId>` + 方法。`sync.Once`/`sync.Mutex` → `OnceCell`/`Mutex`。

### 并发

`Program.GetTypeChecker` 走 checker 池（`CheckerPool`，多 checker 实例并行检查不同文件）。每个 `Checker` 内部基本单线程（有 `mu sync.Mutex` 护少量共享）。Rust 侧：checker 实例不跨线程共享可变状态；多文件并行用 rayon 在**多个 checker 实例**层面（PORTING §6），输出按文件稳定排序保证诊断顺序确定。`emitResolverOnce`/`ambientModulesOnce` → `OnceCell`。

## 文件清单 → Rust 模块（逐文件一句话职责）

> 命名：`checker.go`→`lib.rs`（crate 根）；其余同名 `.rs`，在 `lib.rs` 里 `mod xxx;`。按行数降序。

| Go 文件 | 行数 | Rust 文件 | 一句话职责 |
|---|---|---|---|
| `checker.go` | 31842 | `lib.rs` | **检查器主体**：`Checker` struct（~300 字段）、`NewChecker`、intrinsic/global 类型初始化、`resolveName`/符号解析、`getTypeOfSymbol`/`getDeclaredTypeOfSymbol`、`instantiateType`、表达式/语句检查 `checkExpression`/`checkSourceFile`、`getDiagnostics`、`GetEmitResolver`。**必拆**（见子阶段建议） |
| `relater.go` | 4985 | `relater.rs` | **类型关系**：`Relation` 缓存、`isTypeRelatedTo`/`checkTypeRelatedTo`（子类型/赋值/可比较/恒等）、结构化比较、方差（variance）计算 |
| `nodebuilderimpl.go` | 3556 | `nodebuilderimpl.rs` | **node builder 实现**：把 `Type`/`Symbol` 反序列化成 AST `TypeNode`（`typeToTypeNodeHelper` 等），`.d.ts` 生成与 hover 的核心 |
| `flow.go` | 2732 | `flow.rs` | **控制流分析**：`getFlowTypeOfReference`、各类收窄（narrowing）、可达性、`FlowState` 复用 |
| `grammarchecks.go` | 2213 | `grammarchecks.rs` | **语法检查**：非类型的"语法层"错误（修饰符冲突、重复、非法位置等），独立于类型推断 |
| `utilities.go` | 1829 | `utilities.rs` | **工具函数**：类型/符号谓词、flag 判定、各种 helper（无单一主题，配合主体） |
| `inference.go` | 1627 | `inference.rs` | **类型推断**：`inferTypes`、`InferenceInfo`/`InferenceState`、签名参数推断、推断优先级 |
| `jsx.go` | 1479 | `jsx.rs` | **JSX 检查**：内在/值元素解析、props 类型、JSX runtime、`checkJsxElement` 等 |
| `types.go` | 1435 | `types.rs` | **类型表示**：`Type`/`Signature`/`IndexInfo`/各 `TypeData` 变体、`TypeFlags`/`ObjectFlags`/`TypeFormatFlags`/`SymbolFormatFlags`、`TypeAlias`、`TypePredicate` |
| `emitresolver.go` | 1253 | `emitresolver.rs` | **emit 解析器**：`EmitResolver`，给 P5 printer/declaration transformer 用的查询接口实现 |
| `services.go` | 1099 | `services.rs` | **语言服务 API**：`GetSymbolsInScope`/`GetContextualType`/`GetConstantValue`/`GetApparentProperties`… 供 LS（P7）调用 |
| `nodecopy.go` | 895 | `nodecopy.rs` | **节点深拷贝**：node builder 生成 `.d.ts` 时复制/改写 AST 子树 |
| `symbolaccessibility.go` | 840 | `symbolaccessibility.rs` | **符号可达性**：判断符号在某位置是否可命名/可访问（`.d.ts` 生成时的可见性分析） |
| `pseudotypenodebuilder.go` | 711 | `pseudotypenodebuilder.rs` | **PseudoType→AST**：把 `pseudochecker.PseudoType` 回放成 AST 节点（isolatedDeclarations 路径） |
| `nodebuilder_hover.go` | 597 | `nodebuilder_hover.rs` | **hover 扩展**：`ExpandSymbolForHover` 等悬浮提示专用的符号展开 |
| `printer.go` | 471 | `printer.rs` | **类型转字符串**：`typeToString`/`symbolToString` 的打印逻辑（基于 node builder + 一个内部 printer） |
| `tracer.go` | 366 | `tracer.rs` | **trace**：`Tracer`、`--generateTrace` 的事件/类型记录（`Push`/`Pop`） |
| `exports.go` | 331 | `exports.rs` | **模块导出**：`getExportsOfModule` 等导出符号表解析 |
| `nodebuilder.go` | 299 | `nodebuilder.rs` | **node builder 门面**：`NodeBuilder` struct + 公开 `TypeToTypeNode`/`SymbolToEntityName`/`SerializeTypeForDeclaration`… 入口（薄封装 → `nodebuilderimpl`） |
| `mapper.go` | 296 | `mapper.rs` | **类型映射器**：`TypeMapper` 各变体 + 应用映射（类型实例化的基础设施） |
| `nodebuilderscopes.go` | 253 | `nodebuilderscopes.rs` | **node builder 作用域**：生成 `.d.ts` 时的作用域/名称追踪辅助 |
| `symboltracker.go` | 129 | `symboltracker.rs` | **SymbolTracker 实现**：实现 `nodebuilder::SymbolTracker`（收集可见性/可达性报告） |
| `jsdoc.go` | 98 | `jsdoc.rs` | **JSDoc**：JSDoc 类型相关的小工具 |
| `stringer_generated.go` | 25 | `stringer_generated.rs` | `SignatureKind` 的 `Display`（Go `//go:generate stringer`）→ 手写 `Display`/derive |

## 强烈建议：把 checker 拆成执行期子阶段 4a..4k

`checker` 不可能一次 red→green。建议按下面**子阶段**推进（每个子阶段一个 worklog / 一组 PR），每个子阶段编译通过 + 能跑该子阶段能覆盖的 conformance 子集再进下一个。每个子阶段在 `phase-4-checker/checker/` 下建 `4x-*/worklog.md` 细化函数级 TODO。

> 依赖关系：4a 是地基；4b/4c 互相耦合（符号↔类型）需穿插；4d–4f 依赖 4a–4c；4g 较独立；4h–4j 依赖 4a–4c（读类型）但产出 AST；4k 在最后桥接 emit。

| 子阶段 | 主题 | 主要文件 | 收口判据（建议） |
|---|---|---|---|
| **4a** | **类型/符号所有权地基** | `types.rs` `mapper.rs` `tracer.rs` `stringer_generated.rs` + `lib.rs` 的 `Checker` 骨架/intrinsic 初始化 | `NewChecker` 能构造、intrinsic 类型（any/unknown/string/number/boolean/null/undefined…）与全局符号占位就绪；`TypeId`/`SymbolId`/`SignatureId` arena + 各 interning map 落地；`TestTracerPushPreservesEndArgMutations` 绿 |
| **4b** | **符号解析与模块导出** | `lib.rs`(resolveName 区) `exports.rs` `symboltracker.rs` | `resolve_name`/`getSymbolAtLocation` 路径打通；**`TestGetSymbolAtLocation` 绿**（这是包内唯一的真功能单测）；导出符号表可解析 |
| **4c** | **声明类型 / 类型获取** | `lib.rs`(getTypeOfSymbol/getDeclaredTypeOfSymbol 区) `utilities.rs` | `getTypeOfSymbol`/`getDeclaredTypeOfSymbol`/`getApparentType` 可用；接口/类/类型别名/枚举的 declared type 构造正确（小型 conformance 子集） |
| **4d** | **类型实例化 + 关系** | `mapper.rs`(应用) `lib.rs`(instantiateType/Signature) `relater.rs` | `instantiate_type`、`is_type_related_to`/`check_type_related_to`（4 个 relation）+ 方差计算；泛型实例化与赋值性 conformance 子集 |
| **4e** | **类型推断** | `inference.rs` | `infer_types`、签名推断、推断优先级；泛型函数调用 conformance 子集 |
| **4f** | **控制流分析 / 收窄** | `flow.rs` | `get_flow_type_of_reference` + 各 narrowing；`controlFlow*` conformance 子集 |
| **4g** | **表达式/语句检查 + JSDoc** | `lib.rs`(check* 区) `jsdoc.rs` | `check_expression`/`check_source_file`/`getDiagnostics` 跑通；可对一批 conformance 产出诊断 |
| **4h** | **JSX 检查** | `jsx.rs` | JSX conformance 子集（`tests/cases/conformance/jsx/*`） |
| **4i** | **语法检查** | `grammarchecks.rs` | grammar 错误对拍（相对独立，可与 4g 并行） |
| **4j** | **node builder / 序列化 / hover / services** | `nodebuilder.rs` `nodebuilderimpl.rs` `nodebuilderscopes.rs` `nodebuilder_hover.rs` `nodecopy.rs` `symbolaccessibility.rs` `pseudotypenodebuilder.rs` `printer.rs` `services.rs` | `typeToString`/`TypeToTypeNode`/`SerializeTypeForDeclaration` 可用；为 `.d.ts` emit（P5）与 LS（P7）供能 |
| **4k** | **emit resolver** | `emitresolver.rs` | `GetEmitResolver` 完整；交付给 P5 printer/declaration transformer |

> 进度勾选：README 的 P4 主条目下，建议补 4a..4k 子勾选；每个子阶段 worklog 维护其函数级 TODO + 覆盖的 conformance 目录。

## 按子系统的实现 TODO（锚到文件，不逐函数）

> 每条是一个子系统级别的"史诗"，落到子阶段后再细化。`// Go:` 锚到文件 + 关键入口函数。

### 类型表示与 arena（4a）

- [x] `Type` 头 + `TypeId` + `TypeArena`（`Vec<Type>`）+ `TypeData::Intrinsic` + `IntrinsicType`（4a 落地，`core/types.rs`）　`// Go: types.go:Type/TypeData`
- [x] `TypeData::Literal` + `LiteralType`/`LiteralValue` + `TypeData::Union` + `UnionType`（4b 落地，`core/types.rs`）　`// Go: types.go:LiteralType/UnionType`
- [x] `TypeData::Object` + `ObjectType`（members/properties/signatures/index + 4d: type_parameters/this_type/target/resolved_type_arguments/base_types）（4c/4d，`core/types.rs`）　`// Go: types.go:ObjectType/StructuredType/InterfaceType/TypeReference`
- [x] `TypeData::TypeParameter` + `TypeParameter`（symbol/constraint/is_this_type）（4d 落地，`core/types.rs`）　`// Go: types.go:TypeParameter`
- [ ] `enum TypeData` 其余变体（Intersection/Mapped/Conditional/IndexedAccess/IndexType/...）　`// Go: types.go:TypeData`（4e+）
- [x] `Signature` + `SignatureArena` + `SignatureId`；`IndexInfo` + `IndexInfoArena` + `IndexInfoId`；`SignatureFlags`（4c 落地，`core/signatures.rs`；接入 `Checker.new_signature`/`new_index_info`）　`// Go: types.go:Signature/IndexInfo/SignatureFlags`
- [x] `TypeFlags`（全量单 bit + 复合 union，位值 1:1）+ `ObjectFlags`（kind bits `1<<0..=1<<21` + 掩码）（4a 落地，`core/types.rs`）　`// Go: types.go`
- [ ] `ObjectFlags` 高位 context bits（`1<<22..`，按 owning TypeFlags 复用）+ `TypeFormatFlags`/`SymbolFormatFlags`/`SignatureFlags`　`// Go: types.go`（4c/4j）
- [x] union interning map（`FxHashMap<Vec<TypeId>, TypeId>`，键=排序成员 id）（4b 落地，`core/mod.rs`）　`// Go: checker.go:Checker.unionTypes`
- [ ] 其余 interning map（string/number/bigint/enum/intersection/indexedAccess/template/...）　`// Go: checker.go:Checker 字段`（4c+）
- [x] `LinkStore` 地基：`SymbolLinks<V> = LinkStore<SymbolId, V>` + links（`SymbolReferenceLinks`(4a) + `Value`/`Alias`/`ModuleSymbolLinks`(4b) + `DeclaredTypeLinks`/`TypeAliasLinks`(4c，接入 `Checker`)）　`// Go: checker.go:Checker 字段`
- [ ] 其余 ~24 张 `LinkStore`（mapped/deferred/lateBound/spread/...）　`// Go: checker.go:Checker 字段`（4d+）
- [x] 删除 `Type.checker` 反向指针，改 checker 持 arena（PORTING §5 偏离，已落地）
- [x] intrinsic 类型构造（any/unknown/undefined/null/void/string/number/bigint/symbol/never/object/error/autoType/silentNever）+ `Checker::new`（4a 落地，`core/mod.rs`）　`// Go: checker.go:NewChecker/newIntrinsicTypeEx`
- [x] intrinsic `type_to_string`（intrinsic name + literal 值 + union `" | "` 连接；全量 node-builder 路径与 `false|true`=>`boolean` 折叠 DEFER 4j）（4a/4b）　`// Go: printer.go:TypeToString`
- [x] boolean/union intrinsics：`new_literal_type` + `get_union_type`（dedup+id 排序+intern）+ `boolean`/`false`/`true`/`string|number`/`number|bigint`（4b 落地，`core/mod.rs`）　`// Go: checker.go:newLiteralType/getUnionType/NewChecker`
- [ ] union 的 subtype/literal reduction + `ObjectFlags` 聚合 + union-of-union 快路径（relations 引擎已就绪，待接入 `get_union_type`）　`// Go: checker.go:getUnionType`（4e）
- [x] `new_object_type`（object/interface/class/enum 类型分配）+ `type_to_string(Object)` 占位（4c 落地，`core/mod.rs`）　`// Go: checker.go:newObjectType`
- [x] `get_global_type` 真解析+构建（在 `globals` 表查名 → 建声明类型 → 缓存；4a 占位作废）（4c 落地，`core/declared_types.rs`）　`// Go: checker.go:getGlobalType`

### 类型映射 / 实例化（4d）

- [x] `enum TypeMapper`（Simple/Array/Composite/Merged/Function）+ `map_type`（Go `TypeMapper.Map`）（4d 落地，`core/mapper.rs`）　`// Go: mapper.go:TypeMapper`
- [x] `instantiate_type`（类型参数替换 + union 递归 + 泛型 type-reference 实参重映射）+ `instantiate_signature`（返回类型）+ 深度/计数限制（depth==100 / count>=5M → errorType）（4d 落地，`core/mapper.rs`）　`// Go: checker.go:instantiateType(21958)/instantiateSignature(20470)`
- [ ] 匿名/mapped object 深度实例化（重建成员类型）、index/indexed-access/conditional/template/substitution 实例化、实例化缓存　`// Go: checker.go:getObjectTypeInstantiation/instantiateTypeWorker`（4e+）

### 符号解析 / 声明类型（4b/4c）

- [x] `BoundProgram` 瘦 trait（arena/root/symbol_of_node/symbol/locals + 4aa 多文件 `source_files`/`file_view`/`view_for_symbol`/`file_handle` + 4al `compiler_options`（带默认，加法））+ 测试桩 `StubProgram`（parse+bind dev-dep；4al 加 `parse_and_bind_with_options`）（4b/4aa/4al 落地，`core/program.rs` + `core/test_support.rs`）　`// Go: compiler/program.go:Program（子集）/Program.Options`
- [x] `resolve_name`（meaning/excludeGlobals + 作用域链上行 locals 查找；TDZ/参数/this/use-before-def/错误报告 DEFER 4c+）（4b 落地，`core/symbols.rs`）　`// Go: checker.go:resolveName / binder/nameresolver.go:Resolve`
- [x] `get_symbol_at_location` + `get_symbol_of_declaration`（声明名路径 + `ident.member` 属性访问；4c 起属性走真类型路径 `get_type_of_symbol`→`get_property_of_type`；完整 `checkPropertyAccessExpression` DEFER 4g）（4b/4c，`core/symbols_query.rs`）　`// Go: checker.go:getSymbolAtLocation/getSymbolOfDeclaration/getSymbolOfNameOrPropertyAccessExpression`
- [x] 符号合并 `MergedSymbols`（`get`/`record`）+ alias 跳转 `skip_alias`（已知 target 跟随；模块 import/export alias 解析 DEFER）（4b 落地，`core/symbols.rs`）　`// Go: checker.go:getMergedSymbol/recordMergedSymbol/resolveSymbol`
- [x] `get_declared_type_of_symbol`（interface/class→object type from `members`；type-alias→RHS type node；enum→object type from `exports`）+ `get_type_of_symbol`（变量/属性注解→类型）+ `get_type_from_type_node`（keyword/TypeReference）+ `get_apparent_type`（4c 恒等）+ `get_property_of_type`（4c 落地，`core/declared_types.rs`）　`// Go: checker.go:getTypeOfSymbol(16352)/getDeclaredTypeOfSymbol(23531)/getApparentType(21587)/getPropertyOfType(18742)/getTypeFromTypeNode(22666)`
- [x] 泛型 interface/class：`TypeData::TypeParameter` + `new_type_parameter` + 局部类型参数收集 + `this` 类型；`extends` base 成员合并（`resolve_structured_type_members`，构造期 eager merge）+ `get_property_of_type` 经引擎解析继承成员；`get_type_from_type_node` 为 `Foo<Args>` 建 `TypeReference`（target + resolved_type_arguments）（4d 落地，`core/declared_types.rs`）　`// Go: checker.go:getDeclaredTypeOfClassOrInterface/resolveStructuredTypeMembers/getBaseTypes/getTypeFromTypeReference`
- [x] 成员类型经引用 mapper 实例化 `get_type_of_property_of_type`（4e，见"类型推断"段）　`// Go: checker.go:getTypeOfPropertyOfType`
- [ ] 深化：variance、index 签名继承、enum literal-union、apparent 的 primitive→global wrapper　`// Go: checker.go/relater.go`（4f/4g/P6）
- [ ] 导出符号表：`get_exports_of_module` / `getExportsAndProperties`　`// Go: exports.go`（4e+）

### 关系判定（4d）

- [x] `RelationKind`（Identity/Subtype/StrictSubtype/Assignable/Comparable）+ `RelationCache` + `is_type_related_to`/`check_type_related_to` + 便捷包装（assignable/identical/subtype/comparable）（4d 落地，`core/relations.rs`）　`// Go: relater.go:isTypeRelatedTo(170)/checkTypeRelatedTo(352)`
- [x] `is_simple_type_related_to`（any/unknown/never、string/number/bigint/boolean/esSymbol-like→primitive、undefined→void-like、null→null、object→non-primitive、any 可赋值）+ 结构化比较（union source/target + object `properties_related_to`）+ 缓存式递归守卫（pending=true 破环）（4d 落地，`core/relations.rs`）　`// Go: relater.go:isSimpleTypeRelatedTo/structuredTypeRelatedTo/propertiesRelatedTo`
- [x] **4u 可选属性赋值性**：`properties_related_to` 接入 `require_optional_properties`（Subtype/StrictSubtype 为 true，Assignable/Comparable/Identity 为 false）——缺失的可选 target 属性在非 subtype 关系下放过（Go `getUnmatchedProperty`）；可选 source 属性对必需 target class-member 不可赋值（`symbol_is_optional`/`symbol_is_class_member`，Go `propertyRelatedTo` 的 optional-in-source/required-in-target 分支），但 Comparable 经 `skipOptional` 放宽　`// Go: relater.go:propertiesRelatedTo/propertyRelatedTo/getUnmatchedPropertiesWorker`
- [ ] 完整结构化：signatures/index 比较、variance、intersection、`Ternary`(Maybe) 递归模型、`exactOptionalPropertyTypes`/strictNullChecks 的可选属性 `undefined` 注入、错误报告　`// Go: relater.go`（4f+）

### 类型推断（4e）

- [x] `InferenceInfo`/`InferenceContext`/`InferencePriority` + `infer_types`（type-param 槽 + union + 泛型引用 type-args + object 成员，visited 破环）（4e 落地，`core/inference.rs`）　`// Go: inference.go:inferTypes(53) + checker.go:InferenceInfo/InferenceContext`
- [x] `get_inferred_type`/`get_inferred_types`（候选→best-common；无候选→unknown 默认）+ `get_inference_mapper`（eager Array `typeParams→inferred`）（4e 落地，`core/inference.rs`）　`// Go: inference.go:getInferredType(1260)/getInferredTypes(1349)/getMapperFromContext`
- [x] 泛型签名调用推断 `infer_type_arguments` + 与 `instantiate_signature` 闭环（推断 T → 实例化返回类型）（4e 落地，`core/inference.rs`）　`// Go: inference.go + checker.go:instantiateSignature`
- [x] 成员类型实例化 `get_type_of_property_of_type`（`Foo<string>.value`→`string`，经引用 `typeParams→args` mapper）+ `get_declared_type_of_type_parameter`（4e 落地，`core/declared_types.rs`）　`// Go: checker.go:getTypeOfPropertyOfType(18806)/getDeclaredTypeOfTypeParameter`
- [x] best-common-type `get_best_common_type`（支配者或 union）+ `subtype_reduce`（经 `is_type_subtype_of` 去包含成员）（4e 落地，`core/inference.rs`）　`// Go: checker.go:getCommonSupertype/removeSubtypes`
- [ ] 推断深化：contravariant 候选 + 优先级 lattice、mapped/conditional/template 推断、约束/默认、上下文类型、lazy `InferenceTypeMapper`、`get_union_type`/`get_apparent_type` 接入 reduction　`// Go: inference.go`（4f+）

### 控制流分析（4f）

- [x] `BoundProgram` 暴露 flow 图：`flow_node_of(node)`/`flow_node(id)`/`flow_list(id)`（读 binder `BindResult.node_flow`/`flow_nodes`/`flow_lists`）（4f 落地，`core/program.rs`+`core/test_support.rs`）　`// Go: compiler/program.go`
- [x] `get_flow_type_of_reference` + `get_type_at_flow_node` 前驱遍历 + flow 循环缓存（FxHashMap 种子破环）：START/CONDITION/LABEL(union) 路径（4f 落地，`core/flow.rs`）　`// Go: flow.go:getFlowTypeOfReference(77)/getTypeAtFlowNode`
- [x] narrowing 原语：`narrow_type_by_typeof`（string/number/boolean/bigint/symbol/undefined/object）、`narrow_type_by_truthiness`（去 falsy：undefined/null/void/`false`）、`narrow_type_by_equality`（字面量按值 + 子类型重叠）、`narrow_type_by_in`（属性存在过滤）——经 union 过滤 + 4d 关系引擎（4f 落地，`core/flow.rs`）　`// Go: flow.go:narrowTypeByTypeof/narrowTypeByTruthiness/narrowTypeByEquality/narrowTypeByInKeyword`
- [x] 条件分发 `narrow_type_at_condition`/`narrow_type_by_binary`：`if (typeof x === "s")` + bare `if (x)` truthiness；`is_matching_reference`（标识符按 resolved value symbol 比较）（4f 落地，`core/flow.rs`）　`// Go: flow.go:narrowType/narrowTypeByBinaryExpression/isMatchingReference`
- [x] 可达性 `is_reachable_flow_node`（+ worker，visited 破 back-edge；UNREACHABLE→false / START→true / LABEL→any antecedent / 单前驱）（4f 落地，`core/flow.rs`）　`// Go: flow.go:isReachableFlowNode(2481)`
- [ ] DEFER(4g+)：`instanceof` 收窄（需构造器/实例类型 + global `Function`）、`x === <expr>` 判别式接表达式检查、`&&`/`||`/括号/前缀 `!` flow、assignment/array-mutation/call/switch-clause flow、loop fixpoint、完整 `TypeFacts` lattice（字符串/数字 falsy 字面量子类型）、unreachable→`never`　`// Go: flow.go`　blocked-by: 4g 表达式检查 + `TypeFacts` + lib globals(P6)

### 表达式 / 语句检查（4g）

- [x] `core/check.rs`：`check_source_file`→`check_statement`→`check_expression` 递归 + checker 上累积 `Diagnostic` + `get_diagnostics(file)`（4g 落地）　`// Go: checker.go:checkSourceFile(2176)/checkExpression(7521)/getDiagnostics(13865)`
- [x] `check_expression` 族：字面量（string/number/bool/null）、identifier（`resolve_name`→`get_type_of_symbol`→4f flow 收窄）、property access（`get_type_of_property_of_type`）、element access（字符串字面量索引）（4g 落地，`core/check.rs`）　`// Go: checker.go:checkIdentifier(10999)/checkPropertyAccessExpression`
- [x] 诊断：未定义名 `Cannot_find_name_0`(2304)、缺属性 `Property_0_does_not_exist_on_type_1`(2339)（经 `Message`+`format` 本地化，记录 code/category/span）（4g 落地）　`// Go: checker.go:error(13893)`
- [x] 调用解析（起步）：`get_signatures_of_type`（call 签名，经 apparent + reference target）+ `get_return_type_of_call`（非泛型→返回类型；泛型→复用 4e `infer_type_arguments` 推断 + `instantiate_signature`）（4g 落地，`core/check.rs`）　`// Go: checker.go:getSignaturesOfType/getReturnTypeOfSignature + inference.go`
- [x] `TypeFacts` lattice（起步）：`core/type_facts.rs` 的 `TypeFacts`(TRUTHY/FALSY) + `get_type_facts`（`""`/`0` falsy 字面量子类型）+ `get_type_with_facts` + `has_type_facts`；`narrow_type_by_truthiness` 改走 `get_type_with_facts`（4g 落地）　`// Go: utilities.go:getTypeFacts/getTypeWithFacts`
- [x] 回填 4f：`x === <expr>` 判别式在 flow walk 中经 `check_expression` 求 value 类型 → `narrow_type_by_equality`（4g 落地，`core/flow.rs`）　`// Go: flow.go:narrowTypeByEquality`
- [x] 变量声明赋值性（4m）：`check_statement` 的 `VariableStatement` 臂 → `check_variable_declaration`（带注解+初始化器→`is_type_assignable_to(初始化器类型, 注解类型)`，不符→`Type_0_is_not_assignable_to_type_1`(2322)）+ 错误消息 literal 源广义化 `generalized_source_for_error`/`get_base_type_of_literal_type`（Go `reportRelationError`）+ `Block` 臂递归检查嵌套语句（`checkBlock`）　`// Go: checker.go:checkVariableLikeDeclaration(5760)/checkBlock + relater.go:reportRelationError`
- [x] 赋值表达式赋值性（4n）：`check_expression` 的 `BinaryExpression` 臂 → `check_binary_expression`（求左右操作数类型）+ `check_assignment_operator`（`EqualsToken`：引用式 LHS 经 `is_reference_expression` 守卫 → `is_type_assignable_to(右值, 左值)`，不符→`2322`，错误节点=LHS，复用 4m 广义化）　`// Go: checker.go:checkBinaryExpression(12275)/checkBinaryLikeExpression(12280)/checkAssignmentOperator(12701)/checkReferenceExpression(13062)`
- [x] 语句容器递归（4n）：`check_statement` 的 `IfStatement`（cond+then+else）/`WhileStatement`/`DoStatement`（body+cond）/`ForStatement`（init·cond·incr+body，init 为 decl-list 走 `check_variable_declaration_list`）/`TryStatement`（try·catch·finally 块）臂，使嵌套诊断浮现；抽 `check_variable_declaration_list`（Go `checkVariableDeclarationList`）供变量语句与 for 初始化器复用　`// Go: checker.go:checkSourceElement(2223)/checkIfStatement(3778)/checkWhileStatement(3929)/checkForStatement(3935)/checkTryStatement(4208)/checkCatchClause(4220)`
- [x] 非赋值二元运算符（4o）：`check_binary_expression` 的关系 `<`/`>`/`<=`/`>=`（→`boolean`+`2365`）、相等 `==`/`!=`/`===`/`!==`（→`boolean`+`2367`）、非 `+` 算术 `-`/`*`/`/`/`%`/`**`/`<<`/`>>`/`>>>`/`&`/`|`/`^`（→`number`+`2362`/`2363`）臂；helper `relational_operands_comparable`/`equality_operands_comparable`/`is_type_equality_comparable_to`/`are_types_comparable`/`check_arithmetic_operand_type`/`get_base_type_of_literal_type_for_comparison`/`report_binary_operator_error`（经 4d 关系引擎 + 4j node builder + `tsgo_scanner::token_to_string`）　`// Go: checker.go:checkBinaryLikeExpression(12280)/checkArithmeticOperandType(12743)/reportOperatorError(12662)/reportOperatorErrorUnless(12683)/getBaseTypeOfLiteralTypeForComparison(25313)/isTypeEqualityComparableTo(12805) + relater.go:areTypesComparable(166)`
- [x] 语句容器递归（4o）：`check_statement` 的 `SwitchStatement`（switch 表达式 + case/default clause 表达式·语句递归）/`ForInOrOfStatement`（初始化器 decl-list·expr + 迭代表达式 + body 递归，覆盖 `for-in`+`for-of`）臂　`// Go: checker.go:checkSwitchStatement(4144)/checkForInStatement(3961)/checkForOfStatement(4008)`
- [x] 逻辑/`+`/复合赋值运算符（4p）：`check_binary_expression` 的 `&&`/`&&=`（结果=左型，左可真→`union(extractDefinitelyFalsyTypes(getBaseTypeOfLiteralType(右)), 右)`）、`||`/`||=`（结果=左型，左可假→`union(removeDefinitelyFalsyTypes(左), 右)`）、`??`/`??=`（结果=左型，nullish 精化 DEFER）、`+`/`+=`（双 number-like→number、双 bigint-like→bigint、任一 string-like→string、任一 any→any/error、否则 `2365`+any）臂；复合赋值族 `*=`/`/=`/`%=`/`**=`/`-=`/`<<=`/`>>=`/`>>>=`/`&=`/`|=`/`^=`/`+=`/`&&=`/`||=`/`??=` 经 `check_assignment_operator` 校验；helper `is_type_assignable_to_kind_strict`/`get_union_dropping_never`/`remove_definitely_falsy_types`/`extract_definitely_falsy_types`/`get_definitely_falsy_part_of_type` + free fn `is_compound_assignment`　`// Go: checker.go:checkBinaryLikeExpression(12280)/checkAssignmentOperator(12701)/isTypeAssignableToKindEx(20196)/removeDefinitelyFalsyTypes(28782)/extractDefinitelyFalsyTypes(28786)/getDefinitelyFalsyPartOfType(28790) + ast.go:IsCompoundAssignment`
- [x] 语句容器递归（4p）：`check_statement` 的 `ThrowStatement`（检查抛出表达式）/`LabeledStatement`（递归被标注语句）臂　`// Go: checker.go:checkThrowStatement(4198)/checkLabeledStatement(4180)`
- [x] 调用表达式实参检查（4q）：`check_expression` 的 `CallExpression` 臂 → `check_call_expression`（callee 类型 → `get_signatures_of_type` → 单非泛型候选）；实参数 `has_correct_arity`（min/max，可选参 `?`/初始化器/rest 降低 min）不符 → `report_argument_arity_error`→`Expected_0_arguments_but_got_1`(2554)（过少报在 callee `getErrorNodeForCallNode`，过多报在多余实参）；实参类型 `check_applicable_signature_for_call`（逐实参经关系引擎，首个不可赋值 → `Argument_of_type_0_is_not_assignable_to_parameter_of_type_1`(2345)，literal 源广义化）；结果 = `get_return_type_of_call`（签名返回类型）。函数符号类型经 `declared_types.rs` 的 `get_type_of_func_class_enum_module`（匿名 object + call 签名）/`get_signatures_of_symbol`/`get_signature_from_declaration`（参数符号 + min 实参数 + 注解返回类型）+ 参数类型经 `get_type_of_variable_or_property` 的 `ParameterDeclaration` 臂　`// Go: checker.go:checkCallExpression(8289)/resolveCallExpression(8438)/resolveCall(8806)/isSignatureApplicable(9219)/getArgumentArityError(9668)/getSignatureFromDeclaration(19691)/getSignaturesOfSymbol(19661)/getTypeOfFuncClassEnumModule(16763) + relater.go:getParameterCount(1690)/getMinArgumentCount(1701)/getTypeAtPosition(1754)`
- [ ] DEFER(4q+)：调用——重载选择 + best-match（`chooseOverload`/`reorderCandidates`/`reportCallResolutionErrors`）、泛型 call-site 推断（完整，含上下文回传）、rest/spread 实参（tuple 类型）、`this`-实参（`getThisArgumentOfCall`）、回调实参上下文类型、`new` 表达式（构造签名）、不可调用/未类型化调用诊断（`2349`/`isUntypedFunctionCall`，需 `getApparentType`/lib globals）、方法/访问器/箭头/函数表达式签名、重载实现节点 de-dup、函数体返回类型推断（`getReturnTypeFromBody`，未注解→`any`）、过多实参的多 token 合成 span　`// Go: checker.go`　blocked-by: 重载 `chooseOverload` + 推断上下文 + tuple/spread 类型 + `this`-typing + 上下文类型 + 构造签名 + `getApparentType`/lib globals(P6) + 函数体下传
- [ ] DEFER(4p+)：`+` 的 ES-symbol 操作数诊断（`2469`，需 `Symbol` lib global）+ await 建议 + 2365 literal-operand 广义化（`getBaseTypesIfUnrelated`）；`&&` 真值分支 String/Number/BigInt 原始 falsy 字面量（`emptyString`/`zero`/`zeroBigInt` 内建，需 4b union literal reduction）；`??` 的 nullish 结果精化（`hasTypeFacts(EQUndefinedOrNull)`+`GetNonNullableType`，需 `strictNullChecks` 接线）+`checkNullishCoalesceOperands`；逻辑运算符 union 成员 flatten/subtype reduction；`instanceof`（需构造器/实例类型+global `Function`）、`in`（`checkInExpression`）、comma（unused-left+`AllowUnreachableCode`）；复合赋值 setter writeOnly 类型 + `exactOptionalPropertyTypes`；`return`/`with` 语句（需 `getContainingFunctionOrClassStaticBlock`+`getSignatureFromDeclaration` 的返回类型校验、grammar `checkGrammarStatementInAmbientContext`/`grammarErrorOnFirstToken`/`grammarErrorAtPos`：1108/1101——reachable 路径均 grammar-only，故整体 DEFER）；labeled 的 `Duplicate_label_0` grammar + `Unused_label`；解构赋值目标；类·函数·模块声明体检查 / 未用检查 / 重载选择 / 上下文敏感函数（`CallExpression` 实参检查起步已在 4q 落地，见上）　`// Go: checker.go`　blocked-by: lib globals(P6, `Symbol`/`Function`/iterable) + `strictNullChecks` 接线 + 4b union literal/subtype reduction + grammar 基建（ambient-context/位置型诊断）+ 解构

### JSX（4h）

- [x] `core/jsx.rs`：`check_jsx_element` / `check_jsx_self_closing_element` / `check_jsx_fragment`（接入 `check_expression` 分发；返回 `any` 占位至 lib globals）（4h 落地）　`// Go: jsx.go:checkJsxElement(71)/checkJsxSelfClosingElement(100)/checkJsxFragment(109)`
- [x] 内在 vs 值元素：首字母小写 identifier→内在（查注入的 `JSX.IntrinsicElements`；缺→`Property_0_does_not_exist_on_type_1`(2339)）；大写→值元素经 `check_expression`（未定义→`Cannot_find_name_0`(2304)）（4h 落地，`core/jsx.rs`）　`// Go: jsx.go:isJsxIntrinsicTagName/getIntrinsicTagSymbol`　blocked-by: lib globals (P6) — 真 `JSX.IntrinsicElements`
- [x] 属性（props）检查：每个属性值经 4g `check_expression` 求类型，对元素属性类型上同名 prop 经 4d 关系引擎查可赋值性，不符→`Type_0_is_not_assignable_to_type_1`(2322)（4h 落地，`core/jsx.rs`）　`// Go: jsx.go:checkJsxAttributes/checkJsxAttribute`
- [x] 子节点类型化：`{expr}` 容器子节点经 `check_expression`（4h 落地，`core/jsx.rs`）　`// Go: jsx.go:checkJsxChildren`
- [x] `Checker::set_jsx_intrinsic_elements` 注入点（替代 lib-global 解析至 P6）（4h 落地，`core/mod.rs`）
- [ ] DEFER(4i+)：JSX factory/pragma + grammar、spread 属性、namespaced 名、布尔简写属性、组件 props（call/construct 签名→属性类型）、`JSX.Element`/`JSX.ElementType` 约束、excess-property 检查、JsxText/嵌套子节点对 children 类型校验　`// Go: jsx.go`　blocked-by: lib globals (P6) + 可调用类型构造 + node builder(4j)

### 语法检查（4i）

- [x] `core/grammar.rs`：`check_grammar_modifiers` 修饰符族（接入 `check_source_file`→`check_statement`，含类成员遍历）（4i 落地）　`// Go: grammarchecks.go:checkGrammarModifiers(213)`
- [x] 重复修饰符→`'{0}' modifier already seen`(1030)；冲突 `declare async`→`'{0}' modifier cannot be used in an ambient context`(1040)；重复可访问性 `public private`→`Accessibility modifier already seen`(1028)（4i 落地，`core/grammar.rs`）　`// Go: grammarchecks.go`
- [x] `modifier_nodes`（按 NodeData 取修饰符 token：Function/Class/Property/Method 声明）+ `modifier_text`（修饰符关键字文本）（4i 落地）　`// Go: ast.go:Node.ModifierNodes + scanner.go:TokenToString`
- [x] **4s 扩展修饰符族**（`core/grammar.rs`，全部经类成员/属性可达）：可访问性须前于 `static`（`static public`→`'public' modifier must precede 'static' modifier`(1029)）；`accessor`+`readonly` 冲突（`readonly accessor`→`'accessor' modifier cannot be used with 'readonly' modifier`(1243)）；`readonly` 仅限属性/索引签名（`readonly m(){}`→1024）；`accessor` 仅限属性声明（`accessor m(){}`→1275）　`// Go: grammarchecks.go:checkGrammarModifiers(213)`
- [x] **4s 变量声明 grammar**：`check_grammar_variable_declaration`（接入 `check_variable_declaration`）——`const x;`（无初始化器、非 ambient）→`'const' declarations must be initialized`(1155)；ambient（`declare const`）走 `checkAmbientInitializer` 故跳过（DEFER）　`// Go: grammarchecks.go:checkGrammarVariableDeclaration(1567)`
- [x] **4s 构造器 grammar**：`check_grammar_constructor_type_parameters`（`constructor<T>(){}`→`Type parameters cannot appear on a constructor declaration`(1092)）+ `check_grammar_constructor_type_annotation`（`constructor(): void {}`→`Type annotation cannot appear on a constructor declaration`(1093)），接入 `check_class_member` 的 Constructor 臂　`// Go: grammarchecks.go:checkGrammarConstructorTypeParameters(1869)/checkGrammarConstructorTypeAnnotation(1884)`
- [ ] DEFER(4j+)：装饰器 grammar、完整修饰符顺序/位置矩阵剩余项（顶层 `static`/`public`/`readonly`/`abstract`/`accessor` 等被解析为标识符而非修饰符故不可达——`1044` 模块/命名空间元素、`1242` abstract 位置、`override` 顺序等需 parser 将其解析为修饰符）、参数属性、严格模式保留字（需 strict-mode 上下文检测）、其余 grammar 族（语句/类型/heritage/索引签名/参数列表 1014/1047/1048/trailing-comma…）　`// Go: grammarchecks.go`　blocked-by: parser 顶层/函数修饰符解析 + strict-mode 上下文 + `compilerOptions`(tsoptions/program wiring) + node builder(4j) + 位置型诊断基建（`grammarErrorAtPos`）

### node builder / 序列化 / 可达性 / printer / hover / services（4j）

- [x] `core/nodebuilder.rs`：`type_to_string`（程序感知，`&mut Checker`：命名 interface/class→名；type reference→`Box<string>`；匿名 object→`{ k: T; }` 成员字面量；union→`A | B` 递归；intrinsic/literal 委托程序无关 printer）+ `symbol_to_string`（声明名）（4j 落地）　`// Go: checker.go:typeToString/symbolToString + nodebuilderimpl.go`
- [x] 4g/4h 诊断改用真 `type_to_string`：缺属性→"Property 'x' does not exist on type 'Foo'"（不再 `{ ... }`）；JSX 属性不符同理（4j 落地，`core/check.rs`/`core/jsx.rs`）　`// Go: checker.go`
- [ ] `NodeBuilder` 完整门面（`type_to_type_node`/`serialize_type_for_declaration`/`symbol_to_entity_name`…）+ 作用域追踪 / 节点深拷贝 / 符号可达性 / pseudo-type 回放 / hover 展开　`// Go: nodebuilder.go/nodebuilderimpl.go/nodebuilderscopes.go/nodecopy.go/symbolaccessibility.go/pseudotypenodebuilder.go/nodebuilder_hover.go`　DEFER(4k+)
- [ ] DEFER(4k)：函数/构造签名 `(x: T) => U`、array/tuple、intersection（`A & B`，类型种类未建）、mapped/conditional、alias 名、可选/只读成员装饰、`SymbolTracker`、声明顺序成员（当前按 `properties` 顺序）　`// Go: nodebuilderimpl.go`　blocked-by: 这些类型种类尚未构造 + node-builder scope 机制
- [ ] 语言服务查询 API（`GetSymbolsInScope`/`GetContextualType`/`GetConstantValue`/…）　`// Go: services.go`
- [ ] `SymbolTracker` 实现　`// Go: symboltracker.go`

### emit resolver（4k）

- [x] `core/emit_resolver.rs`：`EmitResolver`（轻量值句柄）+ `Checker::get_emit_resolver`（`OnceCell` 缓存）（4k 落地）　`// Go: emitresolver.go:EmitResolver + checker.go:GetEmitResolver(31832)`
- [x] `is_declaration_visible`（模块/decl-emit 规则：顶层声明带 `export` 即可见）（4k 落地，`core/emit_resolver.rs`）　`// Go: emitresolver.go:IsDeclarationVisible(104)`
- [x] `serialize_type_of_declaration`（取声明类型 → 4j `type_to_string`）（4k 落地）　`// Go: emitresolver.go:SerializeTypeOfDeclaration`
- [x] `is_implementation_of_overload`（带 body 的 function + 符号多声明）（4k 落地）　`// Go: emitresolver.go:IsImplementationOfOverload(458)`
- [x] `resolve_reference`（标识符 USE → 声明符号：经 `resolve_name` 作用域链上行，meaning=`VALUE|ALIAS`，innermost 遮蔽优先；alias 直接按 `ALIAS` 标志匹配而非跟随 target，DEFER 跨文件 target meaning）（4an 落地，`core/emit_resolver.rs`）　`// Go: checker.go:resolveName/getResolvedSymbol`
- [x] `is_referenced`（importElision 原语：扫全文件值位标识符 USE，排除声明自身名节点，任一经 `resolve_reference` 解析到该声明符号即 referenced；作用域正确，替代 6e-2 的 name-match 替身。**4ap 扩 `declaration_name` 加 `ImportEqualsDeclaration` 臂**——未用 `import x = require("m")` 的自身名 `x` 被排除 → `is_referenced`=false 可观察）（4an 落地 + 4ap 扩，`core/emit_resolver.rs`）　`// Go: checker.go:isReferenced(7041)`
- [x] `Checker::new_checker(program)` 入口：4k 前向兼容（忽略 program）→ **4l 真正保留**（`Rc<dyn BoundProgram>` 存入 `Checker.program`，Go `c.program = program`）+ `program()` 访问器；`check_source_file(file)`/`get_diagnostics(file)` 基于保留 program 工作 + 幂等（`checked_files`）（`core/mod.rs`/`core/check.rs`）　`// Go: checker.go:NewChecker/checkSourceFile/getDiagnostics`　blocked-by: 全局/lib 绑定 + `getGlobalType` 全量（P6）
- [x] `is_value_alias_declaration`（可达子集：export/import specifier 的 (property)name 在本文件作用域按 *VALUE* meaning 解析成功即为 value alias；`function f(){};export{f}`→true，`interface I{};export{I}`→false。**4ap 加 `ExportAssignment` 臂**：`export = <value ident>`→true、`export = <type ident>`→false、`export = <非 ident>`→true（Go fallback）。DEFER 跨模块 / entity-name target value-ness / 其余 alias 形态 / const-enum）（4ao 落地 + 4ap 扩，`core/emit_resolver.rs`）　`// Go: emitresolver.go:isValueAliasDeclarationWorker(718)`
- [x] `is_referenced_alias_declaration`（可达子集：仅当 `is_alias_symbol_declaration(node)` 且经 4an `is_referenced` 有值位 USE 解析到它；DEFER 导出-alias-target-是-value 分支 + `referenced` 提前累积）（4ao 落地，`core/emit_resolver.rs`）　`// Go: emitresolver.go:IsReferencedAliasDeclaration(680)`
- [x] `get_referenced_export_container`（可达子集：值位标识符 USE 经 `resolve_name`（meaning=`EXPORT_VALUE|VALUE|ALIAS`）解析；若为本模块顶层*导出*绑定则返回该模块 `SourceFile` 节点（CJS 改写 `exports.x` 的容器）；`!prefix_locals && ExportHasLocal && !Variable` → None（仅导出 *variable* 走 `exports.x`）；非导出/被遮蔽 USE → None。DEFER namespace/enum 容器（`FindAncestor`）+ 跨模块 UMD-export + `startInDeclarationContainer`）（4as 落地，`core/emit_resolver.rs`）　`// Go: referenceresolver.go:GetReferencedExportContainer`
- [x] `serialize_type_node_for_metadata`（legacy-decorator `__metadata("design:type", ..)` 地基；加法式 pub 枚举 `SerializedTypeNode` + pub 方法）——可达 keyword-type 子集，逐 Go switch 臂：`number`→`Number`、`string`→`String`、`boolean`→`Boolean`、`bigint`→`BigInt`、`symbol`→`Symbol`、`void`/`undefined`/`never` 及 `null` literal-type→`VoidZero`（`void 0`）、`any`/`unknown`/`object`（及 catch-all）→`Object`。**4au 扩**（仅扩 match、复用既有变体、未加枚举变体）：Go 顶层 `SkipTypeParentheses`（`(T)` unwrap）、`TemplateLiteralType`→`String`、`LiteralType`→`serializeLiteralOfLiteralTypeNode` 的非-`null` 臂（string→`String`/numeric→`Number`/bigint→`BigInt`/`true`,`false`→`Boolean`/负号前缀递归）。**4av 扩**（协调跨-crate lane，checker 4av + transformers 6am 同 lane）：**新增 `Array`/`Function` 加法式枚举变体** + match 臂 `ArrayType`/`TupleType`→`Array`、`FunctionType`/`ConstructorType`→`Function`（Go `case KindArrayType, KindTupleType -> "Array"`、`case KindFunctionType, KindConstructorType -> "Function"`）——这正是 4au 实测会破坏下游 `tsgo_transformers` 无-wildcard 穷尽 match 的两组臂，故与 transformers `serialized_type_to_expression` 对应臂在同 lane 落地（先加 checker 变体→立即加 transformer 臂保持可构建）。DEFER 剩余非-keyword 臂：`TypeReference`→entity ctor、union/intersection/conditional 递归、`TypePredicate`；另 `NoSubstitutionTemplateLiteral` literal 臂当前因 parser gap 不可达（4at+4au+4av 落地，`core/emit_resolver.rs`）　`// Go: tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode`
- [x] `get_type_reference_serialization_kind`（`serialize_type_node_for_metadata` 的 `TypeReference` 臂消费的分类原语；加法式 pub 枚举 `TypeReferenceSerializationKind`（12 变体 1:1 镜像 Go `printer.TypeReferenceSerializationKind` iota）+ pub 方法 `get_type_reference_serialization_kind(checker, program, type_node)`）——可达单文件子集，忠实端口 Go 结构：取 `TypeReference.type_name`（实体名）→ 分别以 Value/Type meaning `resolve_name`（Go 两次 `resolveEntityName`）→ value+type 同解析到 `class` 符号则 `TypeWithConstructSignatureAndValue`（可达 `isConstructorType` stand-in：class 是单文件唯一运行时构造器源）；否则 type 符号解析到非-error 声明类型（interface/type-alias）则 `ObjectType`；未解析则 `Unknown`。DEFER lib-globals 类（`Promise` + `isTypeAssignableToKind` 链 `Void/Number/BigInt/String/Boolean/ESSymbol` + tuple/function/array），及 alias/type-only split/`serialScope`/qualified-name（4aw 落地，`core/emit_resolver.rs`）　`// Go: emitresolver.go:GetTypeReferenceSerializationKind(1139)`
- [ ] DEFER(post-4k / P6)：`is_value_aliased`/`is_referenced_alias` 的**跨模块别名/导入解析分支**、`get_type_reference_serialization_kind` 的 **lib-globals 类**（`Promise`/`NumberLikeType`/`ArrayLikeType`/`VoidNullableOrNeverType`/… 需 resolved global types + 构造/调用签名收集）、`create_type_of_declaration`（输出类型*节点*而非字符串，需完整 node builder + `SymbolTracker`）、其余 `EmitResolver` 方法（参数/可达性/常量值/装饰器元数据）　`// Go: emitresolver.go`　blocked-by: `compiler.Program` + lib globals（P6）+ 完整 node builder

### tracer（4a）

- [x] `Tracer` + `checkerId` 注入（`copy_with_checker_index`，不污染调用方 args）（4a 落地，`tracer.rs`）　`// Go: tracer.go:Tracer/copyWithCheckerIndex`
- [ ] `Push`/`Pop`（separateBeginAndEnd 的 end-arg 变更保留）+ `RecordType`/`wrapType`/`tracedTypeAdapter`　`// Go: tracer.go`（DEFER：见下方"4a 落地记录"——依赖完整 Type 图，且 end-arg 别名语义需 `tsgo_tracing` 暴露独立 begin/end 写入或共享可变 args）

### Cargo / crate 接线

- [x] `internal/checker/Cargo.toml`（`name = "tsgo_checker"` + 全部 path deps + `bitflags`/`rustc-hash`）；`xxh3` DEFER 到需要类型 hash interning（4b+）
- [x] 根 `Cargo.toml` workspace members 追加（checker 已在 members）
- [x] `lib.rs` 声明 `mod`（`core`/`tracer`；`core` 含 `declared_types`/`inference`/`mapper`/`program`/`relations`/`signatures`/`symbols`/`symbols_query`/`types` + cfg(test) `test_support`）+ re-export 4a–4e 公开 API（含 `InferenceContext`/`InferenceInfo`/`InferencePriority`/`get_type_of_property_of_type`）
- [x] checker `Cargo.toml`：`tsgo_parser`/`tsgo_binder` 移到 `[dev-dependencies]`（仅 cfg(test) 桩 program 用）
- [ ] `lib.rs` 声明其余 `mod`（flow/jsx/grammarchecks/...）+ re-export `NewChecker`/`EmitResolver`/`NodeBuilder` 等（4f+）

## TDD 推进顺序（tracer bullet → 增量）

1. 4a：arena + `Type`/`Signature` 表示 + `NewChecker` 能构造 + intrinsic 类型 → `TestTracerPushPreservesEndArgMutations`（只需 tracer + 构造）先绿。
2. 4b：`resolveName` + `getSymbolAtLocation` 最小路径 → **`TestGetSymbolAtLocation` 绿**（接口/变量/属性访问三种节点取到非空符号）。这是包内唯一真功能单测，作为第一个端到端 tracer bullet。
3. 4c–4k：每个子阶段挑一小撮对应 conformance 目录，先把"能产出诊断/类型字符串"打通，再逐步对齐 baseline（详见 tests.md 的 P10 策略）。

## Go 源 → Rust 子文件对照（4a–4k 实际拆分）

> 维护"Go 源文件 → Rust 子文件"映射（PORTING §2）。每个 Rust 函数仍带 `// Go: internal/checker/<file>.go:<Func>` 锚到**原** Go 文件。

| Rust 文件 | 承载内容 | 锚定的 Go 源 |
|---|---|---|
| `internal/checker/lib.rs` | crate 根：模块声明 + 公开 API re-export + 所有权模型说明 | `checker.go`（crate 根） |
| `internal/checker/core/mod.rs` | `Checker` 骨架 + `Checker::new` + `new_checker(Rc<dyn BoundProgram>)`（保留 program）+ `program()`/`retained_program()`/`mark_file_checked()` + 4al `compiler_options()`/`get_strict_option_value()`/`strict_null_checks()` + `RetainedProgram` + `new_type`/`new_literal_type`/`get_union_type`/`new_object_type`/`new_type_parameter`/`create_type_reference`/`new_signature`/`new_index_info`/`type_to_string` + link/arena/relation/instantiation/`program`/`checked_files` 字段 | `checker.go:Checker/NewChecker/newType/newLiteralType/getUnionType/newObjectType/newTypeParameter/createTypeReference/newSignature/newIndexInfo` + `printer.go:TypeToString` |
| `internal/checker/core/types.rs` | `TypeId`/`TypeFlags`/`ObjectFlags`/`format_type_flags`/`Type`/`TypeArena` + `TypeData::{Intrinsic,Literal,Union,Object,TypeParameter}` + `*Type` payloads | `types.go:TypeFlags/ObjectFlags/Type/TypeData/IntrinsicType/LiteralType/UnionType/ObjectType/TypeParameter/FormatTypeFlags` |
| `internal/checker/core/mapper.rs` | `TypeMapper`（Simple/Array/Merged/Composite/Function）+ `map_type`/`instantiate_type`/`instantiate_signature` | `mapper.go:TypeMapper` + `checker.go:instantiateType/instantiateTypeWorker/instantiateSignature` |
| `internal/checker/core/relations.rs` | `RelationKind`/`RelationCache` + `is_type_related_to`(4bb：`literals_equal_by_value` 等值字面量关联，复刻 Go interning)/`check_type_related_to`/`is_simple_type_related_to`/`structured_type_related_to`/`properties_related_to` + 便捷包装 | `relater.go:Relation/isTypeRelatedTo/isSimpleTypeRelatedTo/checkTypeRelatedTo/structuredTypeRelatedTo/propertiesRelatedTo` + `checker.go:getLiteralType(interning)` |
| `internal/checker/core/inference.rs` | `InferenceInfo`/`InferenceContext`/`InferencePriority` + `infer_types`/`get_inferred_type(s)`/`infer_type_arguments`/`get_inference_mapper`/`get_best_common_type`/`subtype_reduce` | `inference.go:inferTypes/inferFromTypes/getInferredType/getInferredTypes` + `checker.go:InferenceInfo/InferenceContext/getCommonSupertype/removeSubtypes` |
| `internal/checker/core/flow.rs` | `get_flow_type_of_reference`/`get_type_at_flow_node`（flow 遍历 + 循环缓存）+ `narrow_type_by_typeof`/`narrow_type_by_truthiness`(经 `TypeFacts`)/`narrow_type_by_equality`/`narrow_type_by_in` + `narrow_type_at_condition`/`narrow_type_by_binary`(typeof + `x === <expr>` + 4az nullable 相等 `narrow_type_by_equality_to_value`：`EQ/NE undefined/null` facts + **4bb 判别属性** `obj.kind === "a"`)/`is_matching_reference` + 4bb `get_discriminant_property_access`/`get_accessed_property_name`/`is_discriminant_property`/`types_same_literal_value`/`narrow_type_by_discriminant_property` + `is_reachable_flow_node` | `flow.go:getFlowTypeOfReference/getTypeAtFlowNode/narrowTypeByTypeof/narrowTypeByTruthiness/narrowTypeByEquality/narrowTypeByInKeyword/narrowType/isMatchingReference/getDiscriminantPropertyAccess/getAccessedPropertyName/narrowTypeByDiscriminantProperty/narrowTypeByDiscriminant/isReachableFlowNode` + `relater.go:isDiscriminantProperty` |
| `internal/checker/core/check.rs` | `check_source_file(file)`（基于保留 program + 幂等）/`check_statement`（expr-stmt/class/`VariableStatement`/`Block`/`IfStatement`/`WhileStatement`/`DoStatement`/`ForStatement`/`TryStatement`/`SwitchStatement`/`ForInOrOfStatement`/`ThrowStatement`/`LabeledStatement` 递归）/`check_variable_declaration_list`/`check_expression`（literal/identifier/property/element/`BinaryExpression`/`NonNullExpression`/`AsExpression`）+ `check_assertion`(4be：`as const`→`getRegularTypeOfLiteralType`（抑制 widening+保字面量）/非-const `as T`→`getTypeFromTypeNode`)/`is_const_type_reference`(4be) + `check_identifier`(4az：`undefined` 值→`undefined_type`)/`check_property_access`/`check_element_access`(4az：经 `check_non_null_expression`)/`check_non_null_assertion`(4ay：`x!`→`get_non_null_type(check_expression(operand))`) + `check_non_null_type`/`check_non_null_type_with_reporter`(4ba：`NonNullReporter{Access,Invocation}` 选 reporter)/`check_non_null_expression`/`report_object_possibly_null_or_undefined_error`(4az：2531/2532/2533 实达 18047/18048/18049)/`report_cannot_invoke_possibly_null_or_undefined_error`(4ba：调用接收者 2721/2722/2723)/`is_entity_name_expression`/`entity_name_to_string` + `check_binary_expression`（赋值 2322 + 关系/相等→boolean 2365/2367 + 非 `+` 算术→number 2362/2363 + `&&`/`||` 结果类型 + `??`/`??=` 4ba nullish 精化 `getNonNullableType(left)\|right`（4bb：结果 `subtype_reduce` 化简 `UnionReductionSubtype` + 非 `??=` 跑 `check_nullish_coalesce_operands` 混用 `5076`）+ `+`/`+=`→string/number/bigint/any 2365 + 复合赋值经 `check_assignment_operator`）/`check_call_expression`(4ba：callee 经 invocation reporter non-null 检查)/`check_assignment_operator`/`is_reference_expression`/`is_compound_assignment` + `check_arithmetic_operand_type`/`relational_operands_comparable`/`are_types_comparable`/`equality_operands_comparable`/`is_type_equality_comparable_to`/`get_base_type_of_literal_type_for_comparison`/`report_binary_operator_error`/`is_type_assignable_to_kind_strict`/`get_union_dropping_never`/`remove_definitely_falsy_types`/`extract_definitely_falsy_types`/`get_definitely_falsy_part_of_type` + `check_variable_declaration`（赋值性 2322）+ `generalized_source_for_error`/`is_literal_type`/`type_could_have_top_level_singleton_types`/`get_base_type_of_literal_type`（错误源广义化）+ 4al for-of 选项门控：`iterables_resolvable_via_protocol`（读真 `--target`/`--downlevelIteration`，替 4ak `global_iterable_type_exists` 代理）/`report_iteration_requires_downlevel`（2802）+ `Diagnostic` + `get_diagnostics(file)`（触发 check）/`error` + `get_signatures_of_type`/`get_return_type_of_call` | `checker.go:checkSourceFile/checkSourceElement/checkExpression/checkAssertion/checkBinaryExpression/checkBinaryLikeExpression/checkArithmeticOperandType/reportOperatorError/reportOperatorErrorUnless/isTypeEqualityComparableTo/getBaseTypeOfLiteralTypeForComparison/checkAssignmentOperator/checkReferenceExpression/checkNonNullType/checkNonNullTypeWithReporter/reportCannotInvokePossiblyNullOrUndefinedError/resolveCallExpression/checkIfStatement/checkWhileStatement/checkForStatement/checkSwitchStatement/checkForInStatement/checkForOfStatement/checkTryStatement/checkIdentifier/checkPropertyAccessExpression/checkVariableLikeDeclaration/checkVariableDeclarationList/checkBlock/getDiagnostics/error/getSignaturesOfType/getReturnTypeOfSignature + relater.go:areTypesComparable/reportRelationError/getBaseTypeOfLiteralType` |
| `internal/checker/core/type_facts.rs` | `TypeFacts`(4az：EQ/NE/Is undefined/null + TRUTHY/FALSY，Go 位号) + `get_type_facts`（4az 忠实 per-kind，strict-aware，union OR-fold）/`has_type_facts`/`get_type_with_facts` + `get_non_null_type`（4ay：`NEUndefinedOrNull` 抽 null/undefined/void 的可达子集，strict-gated） | `checker.go:TypeFacts/getTypeFactsWorker/hasTypeFacts` + `utilities.go:getTypeWithFacts` + `checker.go:GetNonNullableType/getAdjustedTypeWithFacts` |
| `internal/checker/core/jsx.rs` | `check_jsx_element`/`check_jsx_self_closing_element`/`check_jsx_fragment` + `check_jsx_opening_like`/`resolve_jsx_tag`/`is_intrinsic_tag_name`/`get_jsx_intrinsic_attributes_type`/`check_jsx_attributes`/`check_jsx_attribute_value`/`check_jsx_children` | `jsx.go:checkJsxElement/checkJsxSelfClosingElement/checkJsxFragment/checkJsxOpeningLikeElementOrOpeningFragment/getIntrinsicTagSymbol/isJsxIntrinsicTagName/checkJsxAttributes/checkJsxAttribute/checkJsxChildren` |
| `internal/checker/core/grammar.rs` | `check_grammar_modifiers`（重复/冲突/可访问性）+ `check_grammar_variable_declaration`/`check_grammar_constructor_type_annotation`/`check_grammar_constructor_type_parameters` + 4bb `check_nullish_coalesce_operands`（`??` 混用 `\|\|`/`&&` 无括号 → `5076`）+ `modifier_nodes`/`modifier_text` | `grammarchecks.go:checkGrammarModifiers` + `checker.go:checkNullishCoalesceOperands` + `ast.go:Node.ModifierNodes` |
| `internal/checker/core/nodebuilder.rs` | `type_to_string`（命名/引用/匿名成员/union；intrinsic/literal 委托）+ `symbol_to_string` + `serialize_members` | `checker.go:typeToString/symbolToString` + `nodebuilderimpl.go` + `printer.go` |
| `internal/checker/core/emit_resolver.rs` | `EmitResolver` + `is_declaration_visible`/`serialize_type_of_declaration`/`is_implementation_of_overload` + `Checker::get_emit_resolver`(OnceCell)/`new_checker` | `emitresolver.go:EmitResolver/IsDeclarationVisible/SerializeTypeOfDeclaration/IsImplementationOfOverload` + `checker.go:GetEmitResolver/NewChecker` |
| `internal/checker/core/signatures.rs` | `Signature`/`SignatureFlags`/`SignatureArena`/`SignatureId` + `IndexInfo`/`IndexInfoArena`/`IndexInfoId` | `types.go:Signature/SignatureFlags/IndexInfo` |
| `internal/checker/core/declared_types.rs` | `get_declared_type_of_symbol`(+generic/extends/type-param)/`get_type_of_symbol`/`get_type_from_type_node`(+TypeReference +4az `LiteralType`/`null`)/`get_type_from_literal_type_node`(4az：`NullKeyword`→`null`；4bb：`"a"`/`1`/`true` → `regular_type_of_literal_type(check_expression(literal))`)/`get_apparent_type`/`get_property_of_type`/`get_type_of_property_of_type`/`get_properties_of_type`/`resolve_structured_type_members`/`get_global_type` | `checker.go:getDeclaredType*/getDeclaredTypeOfTypeParameter/getTypeOfSymbol/getTypeFromTypeNode/getTypeFromLiteralTypeNode/getApparentType/getPropertyOfType/getTypeOfPropertyOfType/resolveStructuredTypeMembers/getBaseTypes/getGlobalType` |
| `internal/checker/core/symbols.rs` | `SymbolLinks<V>` + `*SymbolLinks`/`DeclaredTypeLinks`/`TypeAliasLinks` + `MergedSymbols` + `skip_alias` + `resolve_name` | `types.go:*SymbolLinks` + `checker.go:resolveName/getMergedSymbol/resolveSymbol` + `core/linkstore.go:LinkStore` |
| `internal/checker/core/program.rs` | `BoundProgram` 瘦 trait（arena/root/symbol_of_node/symbol/locals + 4f flow：`flow_node_of`/`flow_node`/`flow_list` + 4aa 多文件 `source_files`/`file_view`/`view_for_symbol`/`file_handle` + 4al `compiler_options`（带默认）） + `default_compiler_options`（`OnceLock`） | `compiler/program.go:Program`（checker 查询子集）/`Program.Options` |
| `internal/checker/core/symbols_query.rs` | `get_symbol_at_location`/`get_symbol_of_declaration` + 属性访问（真类型路径）+ `is_declaration_name`/`name_of_declaration` | `checker.go:getSymbolAtLocation/getSymbolOfDeclaration/getSymbolOfNameOrPropertyAccessExpression` + `ast/utilities.go:IsDeclarationName` |
| `internal/checker/core/test_support.rs`（cfg(test)） | `StubProgram`（`tsgo_parser` 解析 + `tsgo_binder` 绑定）实现 `BoundProgram`（4al 加 `parse_and_bind_with_options` 驱动选项门控）+ `MultiFileProgram`/`FileView`（4aa 多文件） | 测试桩（替代 P6 `compiler.Program`） |
| `internal/checker/tracer.rs` | `Tracer` + `copy_with_checker_index`（checkerId 注入） | `tracer.go:Tracer/NewTracer/copyWithCheckerIndex` |

> 4a–4k 已落地各子系统的**可达核心**（见上表 + 顶部"checker 移植状态"）。尚未落地：`nodebuilderimpl.go` 完整门面 / `exports.go` / `services.go` / `symboltracker.go` / `nodebuilderscopes.go` 等（→ P5/P6 消费者驱动）。各大文件（`checker.go` ~3.2 万行 / `relater.go` ~5k / `inference.go` ~2.7k / `flow.go` ~2.7k / `emitresolver.go` / `utilities.go`）均按子系统在 `core/*.rs` 增量落地了其 4a–4k 范围内的可达子集；剩余深化 + 全量正确性由 P10 conformance 兜底。

## 4a 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：本轮按行为逐个 red→green，留下两处显式红证据——
`format_type_flags`（先把实现打桩成空 `Vec` → 看到 3 个断言红 → 补全 → 绿）与
`Checker::type_to_string`（先返回空串 → `type_to_string_of_intrinsics` 红 → 补全 → 绿）。
其余结构性/数据切片（arena、bitflags 位值、symbol link、tracer 注入）以"测试取 Go 字面量 → 实现 → 绿"成对推进。

**测试计数（全绿）**：23 个 `#[test]` 单测（types 10 / symbols 3 / mod 6 / tracer 4）+ 20 个 doctest。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；
`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**每函数单测（§8.6）**：本轮所有 `pub fn`（`format_type_flags`、`TypeArena::{new,len,is_empty,alloc,get,get_mut}`、`TypeId::arena_index`、`Type::{flags,object_flags,id,intrinsic_name}`、`Checker::{new,new_type,type_count,get_type,type_to_string,get_global_type,mark_symbol_referenced,symbol_reference_kinds,*_type 访问器}`、`Tracer::{new,checker_index,copy_with_checker_index}`）均有单测或 doctest 覆盖。

**本轮 DEFER（带 blocked-by）**：

- `TestTracerPushPreservesEndArgMutations`（端到端 trace round-trip）——`tsgo_tracing::Tracing::push` 以**值**传 `args` 并快照，无法复现 Go `map[string]any` 的**共享可变**别名语义（Push 后、pop 前对 args 的修改要反映到 end 事件）；忠实复现需改 `tsgo_tracing`（越界）或在 checker 侧用共享可变 args + 独立 begin/end 写入。本轮只移植可移植的不变量（checkerId 注入不污染调用方 args，已测）。→ 后续 checker 轮次。
- `TestGetSymbolAtLocation`——需可运行 `Program`（`compiler`/`tsoptions`/`bundled`，P6/P9）+ `resolveName`/`getSymbolAtLocation`（4b）。→ 4b（先用桩 program）/P6。
- `BenchmarkNewChecker`——需真 program + TS 子模块源；仅性能。→ P10。
- `ObjectFlags` 高位 context bits、`TypeFormatFlags`/`SymbolFormatFlags`/`SignatureFlags`、`TypeData` 其余变体、`Signature`/`IndexInfo` arena、interning maps、其余 ~30 link store、`TypeMapper`/实例化、关系/推断/flow/jsx/grammar/nodebuilder/emitResolver——按 4b..4k 推进。

## 4b 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：延续逐行为 red→green，留下两处显式红证据——
`format_type_flags`（4a 复用的 stub 红）与本轮新增的端到端 tracer bullet：`get_symbol_at_location`（先在 `symbols_query` 返回 `None`/桩 → `TestGetSymbolAtLocation` 端口的三节点断言红 → 实现声明名路径 + 结构化属性访问 → 绿）。数据/结构切片（Literal/Union、link store、merged/skip_alias）以"测试取 Go 字面量 → 实现 → 绿"成对推进。

**本轮交付**：
- 类型：`TypeData::{Literal,Union}` + `LiteralType`/`LiteralValue`/`UnionType`；`Checker` 的 `new_literal_type`/`get_union_type`（dedup+id 排序+intern，空=never，单=自身）；boolean(`false|true`)/`false`/`true`/`string|number`/`number|bigint` intrinsic 落地（构造序 ids 1..=21）；`type_to_string` 扩展到 literal/union。
- 符号：`MergedSymbols`(get/record) + `skip_alias`(alias 跟随) + `ValueSymbolLinks`/`AliasSymbolLinks`/`ModuleSymbolLinks` 数据；`resolve_name`（meaning + 作用域链上行 + globals 兜底）。
- 查询：`BoundProgram` 瘦 trait + cfg(test) `StubProgram`（parse+bind）；`get_symbol_at_location`（声明名 → `get_symbol_of_declaration`；`ident.member` → 结构化属性解析）；**`TestGetSymbolAtLocation` 端口三节点（接口名 `Foo`/变量名 `foo`/属性访问 `bar`）全绿**。

**测试计数（全绿）**：38 个 `#[test]` 单测 + 34 个 doctest。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**本轮 DEFER（带 blocked-by）**：
- 真 `Program` 版 `TestGetSymbolAtLocation`（多文件 `compiler.Program` + host + tsconfig）——`compiler`/`tsoptions`/`bundled` 在 P6/P9。本轮用 cfg(test) `StubProgram`（单文件 parse+bind）跑通核心路径。→ P6。
- 属性访问的**完整**类型化解析（`checkPropertyAccessExpression`：apparent type、union 分配、继承/索引成员、可选链、element access）——4b 仅结构化解析 `const x: TypeRef` 形态。→ 4d（apparent type）/4g（表达式检查）。
- `get_type_of_symbol`/`get_declared_type_of_symbol`/`get_apparent_type`、导出符号表、union reduction/`ObjectFlags` 聚合、`false|true`=>`boolean` 打印折叠、JS 数字字面量规范打印 → 4c/4d/4j。
- alias 的模块 import/export 解析（`resolveAlias`）；`resolve_name` 的 TDZ/参数/this 作用域/use-before-def 错误 → 4c+。

## 4c 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：延续逐行为推进。本轮端到端验证点是"属性访问改走真类型路径"：把 `symbols_query` 的属性解析从 4b 的结构化注解-walk 换成 `get_type_of_symbol`→`get_apparent_type`→`get_property_of_type`，`TestGetSymbolAtLocation` 端口（含 `foo.bar`→`bar`）仍全绿，且新增 `declared_types` 行为单测（interface/type-alias/enum/keyword/global）逐个红→绿。

**本轮交付**：
- 签名/索引：`core/signatures.rs` 落地 `Signature`/`SignatureFlags`/`SignatureArena`/`SignatureId` + `IndexInfo`/`IndexInfoArena`/`IndexInfoId`，并接入 `Checker.new_signature`/`signature`/`new_index_info`/`index_info`。
- 类型：`TypeData::Object` + `ObjectType`（members/properties/signatures/index）+ `Type::as_object`；`Checker.new_object_type`；`type_to_string(Object)` 占位（命名/结构化打印 DEFER 4j）。
- 声明类型：`core/declared_types.rs` 落地 `get_declared_type_of_symbol`（interface/class← `members`、type-alias← RHS type node、enum← `exports`）、`get_type_of_symbol`（变量/属性注解）、`get_type_from_type_node`（keyword + identifier TypeReference）、`get_apparent_type`（4c 恒等）、`get_property_of_type`（经 apparent → object members）、`get_global_type`（在 globals 表解析+构建+缓存，作废 4a 占位）。
- link store：`DeclaredTypeLinks`/`TypeAliasLinks` 数据 + 接入 `Checker`（`declared_type_links`/`type_alias_links`/`value_symbol_links`）做惰性缓存。
- 查询：`get_symbol_at_location`/属性访问签名改带 `&mut Checker`，属性走真类型路径。

**测试计数（全绿）**：54 个 `#[test]` 单测 + 54 个 doctest。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：跟踪 `tests/cases/conformance/types/members/`（属性访问/接口成员）与 `tests/cases/conformance/interfaces/interfaceDeclarations/` 的最小子集——4c 的"接口声明 → 成员属性查找"路径对应这些目录的最基础用例（无继承/泛型/索引签名）。详见 tests.md。

**本轮 DEFER（带 blocked-by）**：
- 声明类型深化：泛型类型参数 / heritage & base types / `this` 类型 / 成员惰性解析（`resolveStructuredTypeMembers`）/ `TypeReference` 类型实参 → 4d（实例化/关系）。
- enum 的忠实声明类型（成员 literal 的 union，需 evaluator 算常量值）；4c 用 object-type-from-`exports` 简化。→ 4g。
- `get_apparent_type` 的 primitive→global wrapper 映射（`globalStringType` 等）、`get_property_of_type` 的 union/intersection 合成属性 + 继承/索引成员 + `Object`/`Function` 增强 → 需 lib globals（P6）+ 关系（4d）。
- 未注解变量的初始化器推断、函数/方法/accessor/alias 的 `get_type_of_symbol`；qualified-name TypeReference → 4d/4g。
- object 类型的命名/结构化 `type_to_string`（node builder）→ 4j。

## 4d 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：mapper/relations 以"小手搭类型 + Go 派生 expected"逐行为推进（type-param 替换、union 实例化、type-reference 实参重映射；any/unknown/never、literal→primitive、union source/target、结构化 object 赋值/继承/子集）。

**本轮交付**：
- mapper/instantiation：`core/mapper.rs` 落地 `TypeMapper`（Simple/Array/Merged/Composite/Function）+ `map_type` + `instantiate_type`（类型参数、union、泛型 type-reference 实参）+ `instantiate_signature`（返回类型）+ 深度/计数守卫（100 / 5M → errorType）。
- 类型参数/泛型：`TypeData::TypeParameter` + `Checker::new_type_parameter` + `create_type_reference`；`ObjectType` 扩展 `type_parameters`/`this_type`/`target`/`resolved_type_arguments`/`base_types`。
- relations：`core/relations.rs` 落地 `RelationKind`/`RelationCache` + `is_type_related_to`/`is_simple_type_related_to`/`check_type_related_to`/`structured_type_related_to`/`properties_related_to` + 便捷包装；缓存式破环递归。
- 声明类型深化：`get_declared_type_of_class_or_interface` 收集局部类型参数 + 合并 `extends` base 成员（构造期 eager merge）；`resolve_structured_type_members`/`get_properties_of_type`；`get_property_of_type` 经引用 delegate 到 target；`get_type_from_type_node` 为 `Foo<Args>` 建 `TypeReference`。

**测试计数（全绿）**：74 个 `#[test]` 单测 + 68 个 doctest。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeRelationships/`（赋值/可比较/恒等）与 `generics/`（泛型接口、type 实参）最小子集——4d 的赋值/结构化关系 + 泛型 type-reference 路径对应这些目录的最基础用例（无 variance/约束/索引签名）。

**本轮 DEFER（带 blocked-by）**：
- 实例化深化：匿名/mapped object 深度实例化（重建成员类型）、index/indexed-access/conditional/template/substitution；实例化缓存。→ 4e+。
- relations 深化：signatures/index-signature 比较、variance、intersection、`Ternary`(Maybe) 递归模型、可选属性、错误报告；wire 进 `get_union_type`（reduction）与 `get_apparent_type`（primitive→global wrapper，需 lib globals P6）。→ 4e/4g/P6。
- 泛型成员类型经引用 mapper 实例化（`getTypeOfPropertyOfSymbol`）；约束/默认类型；this 类型在子类型中的处理。→ 4e。

## 4e 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：inference 以"小手搭泛型类型 + Go 派生 expected 推断结果"逐行为推进（bare type-param、泛型引用 type-args、union target、object 成员、多候选 best-common、无候选→unknown、签名闭环、成员实例化、subtype-reduce）。

**本轮交付**：
- `core/inference.rs`：`InferenceInfo`/`InferenceContext`/`InferencePriority`；`infer_types`/`infer_from_types`（type-param 槽收集；union/源 union/泛型引用 type-args/object 成员；`visited` 破环）；`get_inferred_type`/`get_inferred_types`（候选→best-common，无候选→unknown）；`infer_type_arguments`（创建 context→逐对 infer→inferred）；`get_inference_mapper`（eager Array `typeParams→inferred`）；`get_best_common_type`（支配者或 union）；`subtype_reduce`（经 `is_type_subtype_of` 去包含成员）。
- 泛型调用闭环：`infer_type_arguments` → `TypeMapper::new(typeParams, inferred)` → `instantiate_signature`（返回类型实例化）。
- 成员实例化：`declared_types.rs` `get_type_of_property_of_type`（引用经 `typeParams→args` mapper 实例化属性类型，`Box<string>.value`→`string`）+ `get_declared_type_of_type_parameter`（类型参数符号→type-param 类型，接入 `get_declared_type_of_symbol` 与 `collect_local_type_parameters`）。

**测试计数（全绿）**：87 个 `#[test]` 单测 + 81 个 doctest。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeInference/` 与 `generics/`（调用推断）最小子集——4e 的"从实参推断类型参数 + 实例化签名/成员"路径对应这些目录的最基础用例（无 contextual typing / 约束推断 / mapped 推断）。

**本轮 DEFER（带 blocked-by）**：
- contravariant 候选 + 优先级 lattice（return-type/keyof/mapped 等）；mapped/conditional/template/indexed-access 推断；约束/默认类型推断；上下文类型推断（`getContextualType`）。→ 4f+（部分需表达式检查/flow）。
- lazy `InferenceTypeMapper`（访问时触发进一步推断）；4e 用 eager Array mapper。→ 后续。
- `get_union_type` 接 subtype reduction、`get_apparent_type` 接 primitive→global wrapper（需 lib globals P6）。→ 4f/P6。
- `value: T` 注解经作用域解析 T（类型参数在 interface 成员表而非 locals，`resolve_name` 暂不查成员表）——4e 测试用注入/直接构造规避；真注解路径 DEFER。→ 4f（成员作用域解析）。

## 4f 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：先 tracer bullet —— `narrow_type_by_typeof` 在手搭 `string | number` union 上：先以恒等桩看到红（断言 `string` 得到 union），再实现转绿。随后逐行为推进 truthiness（`boolean`→`true`/`false`、`string|undefined`→`string`）、equality（字面量按值收窄）、`in`（属性存在）、flow 访问器、可达性、最后 `get_flow_type_of_reference` 端到端（真 binder flow 图上 `if (typeof x === "string") { x; }` → `x` 收窄为 `string`）。每步单独 `cargo test` 看红/绿。

**本轮交付**：
- `core/program.rs`+`core/test_support.rs`：`BoundProgram` 新增 `flow_node_of`/`flow_node`/`flow_list`，`StubProgram` 读 binder `BindResult.node_flow`/`flow_nodes`/`flow_lists`。
- `core/flow.rs`：
  - narrowing 原语 `narrow_type_by_typeof`/`narrow_type_by_truthiness`/`narrow_type_by_equality`/`narrow_type_by_in`（均经 `distributed_types` 分配 union 成员 + 4d 关系引擎/字面量值比较过滤，空集→`never`）。
  - `get_flow_type_of_reference` + `get_type_at_flow_node`：从引用的 flow 节点沿前驱回溯，START/不可达→declared，CONDITION→对前驱类型按条件收窄，LABEL→前驱类型 union；`FxHashMap` 缓存（先以 declared 种子破 flow 环）。
  - 条件分发 `narrow_type_at_condition`/`narrow_type_by_binary`：`typeof x === "name"` + bare `if (x)` truthiness；`is_matching_reference` 以解析后的 value symbol 比较标识符。
  - `is_reachable_flow_node`（+ worker，visited 破 back-edge）。

**测试计数（全绿）**：95 个 `#[test]` 单测（+8：7 flow + 1 program flow 访问器）+ 87 个 doctest（+6：四个 narrowing + flow + reachable）。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/controlFlow/` 与 `narrowing/` 最小子集——4f 的 `typeof`/truthiness/`in`/字面量 equality 收窄 + 单层 `if` 控制流对应这些目录最基础用例（无 loop / switch / 赋值 flow / 判别联合 / `instanceof`）。

**本轮 DEFER（带 blocked-by）**：
- `instanceof` 收窄（需构造器/实例类型 + global `Function`）；`x === <expr>` 判别式在 flow walk 中（需表达式检查求 value 类型）。→ 4g（+ lib globals P6）。
- `&&`/`||`/括号/前缀 `!` 条件 flow；assignment/array-mutation/call/switch-clause flow；loop fixpoint；unreachable→`never`。→ 4g+。
- 完整 `TypeFacts` lattice（`""`/`0`/`0n` falsy 字面量子类型、`getTypeWithFacts`/`hasTypeFacts`）；`narrowTypeByDiscriminant` 判别属性。→ 4g+（blocked-by: `TypeFacts`）。

## 4g 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：tracer bullet 先行 —— `check_expression` 以 error-type 桩看红（identifier 期望 `string` 得到 error），再实现 identifier 分发转绿。随后逐行为推进：未定义名诊断（`check_source_file`/`get_diagnostics`/`Diagnostic` 编译红→实现绿）、字面量、属性访问（成功 + 缺属性诊断）、元素访问、调用解析（`get_signatures_of_type`/`get_return_type_of_call`）、`TypeFacts`（`get_type_with_facts` 桩红→实现绿）、回填 4f `x === <expr>`（flow walk 红→绿）、truthiness 改走 facts（`"" | "a"` 红→绿）。每步单独 `cargo test` 看红/绿。

**本轮交付**：
- `core/check.rs`：`check_source_file`→`check_statement`→`check_expression`；表达式族（string/number/bool/null 字面量、identifier 经 `resolve_name`+`get_type_of_symbol`+4f flow 收窄、property access、string-literal element access）；`Diagnostic` + `error`（经 `Message`/`format` 本地化，记 code/category/span）+ `get_diagnostics`；诊断 `Cannot_find_name_0`(2304)/`Property_0_does_not_exist_on_type_1`(2339)；调用解析起步 `get_signatures_of_type` + `get_return_type_of_call`（泛型复用 4e 推断 + 实例化）。
- `core/type_facts.rs`：`TypeFacts`(TRUTHY/FALSY) + `get_type_facts`/`has_type_facts`/`get_type_with_facts`（识别 `""`/`0` falsy 字面量子类型）。
- `core/flow.rs`：`narrow_type_by_binary` 增 `x === <expr>` / `<expr> === x`（经 `check_expression` 求 value 类型 → `narrow_type_by_equality`）；`narrow_type_by_truthiness` 改走 `get_type_with_facts`（删除旧 `is_definitely_falsy`）；`distributed_types` 提升为 `pub(crate)` 供 `type_facts` 复用。
- `core/mod.rs`：`Checker` 增 `diagnostics: Vec<Diagnostic>` 字段。

**测试计数（全绿）**：108 个 `#[test]` 单测（+13：9 check + 2 type_facts + 2 flow）+ 96 个 doctest（+9）。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt --check` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/`(字面量・标识符・属性/元素访问) + `salsa`/`diagnostics`(Cannot_find_name / Property does not exist) 最小子集——4g 的表达式类型 + 两类诊断对应这些目录最基础用例。

**本轮 DEFER（带 blocked-by）**：
- `CallExpression` 经 bound program 端到端 + not-callable / arg-count 诊断 + 重载选择 + 上下文敏感函数。→ 4h+（blocked-by: 声明→可调用类型构造 / interface call-signature 收集）。
- 赋值・控制流语句・类・未用检查；数字/计算索引 + 索引签名访问；`==`/`!=` 宽松判别式 + discriminant-property 收窄。→ 4h+。
- 完整 `TypeFacts` lattice（typeof EQ/NE、EQ/NE undefined/null、`0n`）。→ 4h+（blocked-by: lib globals P6 + apparent wrappers）。
- 类型字符串化经 node builder（缺属性诊断里 object 类型暂打印 `{ ... }`）。→ 4j。

## 4h 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：tracer bullet 先行 —— `check_jsx_self_closing_element` 以 error-type 桩看红（`<div/>` 期望 `any` 得到 error），再实现内在标签解析转绿。随后逐行为：未知内在标签诊断（2339）、属性类型不符诊断（2322，flow 注入 `IntrinsicElements.div=Attrs{id:string}`）、值元素未定义（2304，经 `check_source_file` 验证 `check_expression` 分发）、`check_jsx_element` + children（`<div>{y}</div>` y 未定义）、`check_jsx_fragment` + children（`<>{z}</>`）。每步单独 `cargo test` 看红/绿。

**本轮交付**：
- `core/jsx.rs`：`check_jsx_self_closing_element`/`check_jsx_element`/`check_jsx_fragment`（均接入 `check_expression` 分发，返回 `any` 占位）；`check_jsx_opening_like`（解析标签 + 检查属性）；`resolve_jsx_tag`（内在→`get_jsx_intrinsic_attributes_type`；值元素→`check_expression(tagName)`）；`is_intrinsic_tag_name`（首字母小写）；`get_jsx_intrinsic_attributes_type`（查注入的 `JSX.IntrinsicElements`，缺→2339）；`check_jsx_attributes`/`check_jsx_attribute_value`（值经 `check_expression`，对 prop 经 `is_type_assignable_to` 查可赋值，不符→2322）；`check_jsx_children`（`{expr}` 子节点经 `check_expression`）。
- `core/mod.rs`：`Checker` 增 `jsx_intrinsic_elements: Option<TypeId>` + `set_jsx_intrinsic_elements`（lib-global 注入点至 P6）。
- `core/check.rs`：`check_expression` 增 Jsx{SelfClosingElement,Element,Fragment} 分发；`error` 提升 `pub(crate)` 供 `jsx` 复用。
- `core/test_support.rs`：`parse_and_bind_tsx`（ScriptKind::Tsx）驱动 JSX 测试。

**测试计数（全绿）**：114 个 `#[test]` 单测（+6 JSX）+ 100 个 doctest（+4）。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt --check` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/jsx/` 最小子集——内在元素解析、未知内在标签、属性可赋值性、值元素名解析、children 类型化（无 props 推断 / 组件签名 / JSX runtime / 工厂）。

**本轮 DEFER（带 blocked-by）**：
- 真 `JSX.IntrinsicElements`/`JSX.Element`/`JSX.ElementType` 解析（本轮用 `set_jsx_intrinsic_elements` 注入 + 返回 `any`）。→ P6（lib globals）。
- 组件 props 类型（call/construct/函数签名→属性类型）+ 值元素属性检查；spread 属性；namespaced 名；布尔简写属性；excess-property 检查；JsxText/嵌套子节点对 element-children 类型校验。→ 4i+（blocked-by: 可调用类型构造）。
- JSX factory/pragma + grammar 检查。→ 4i。

## 4i 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：tracer bullet 先行 —— `check_grammar_modifiers` 以 no-op 桩看红（`export export function` 期望 1 诊断得到 0），再实现通用重复检测转绿。随后逐行为：`declare async`（1040 ambient）、`public private`（1028 accessibility，含类成员遍历）。`static function`（模块作用域）一项发现 `static` 在顶层被解析为标识符而非修饰符（不可达），遂撤回该 slice 改为可访问性 slice。每步单独 `cargo test` 看红/绿。

**本轮交付**：
- `core/grammar.rs`：`check_grammar_modifiers`（遍历修饰符 token，跳过装饰器；可访问性重复→1028；`async` 于 ambient→1040；通用重复→1030）+ `modifier_nodes`（按 NodeData 取 Function/Class/Property/Method 声明的修饰符）+ `modifier_text`（关键字文本）。
- `core/check.rs`：`check_statement` 顶部调用 `check_grammar_modifiers`，并遍历 `ClassDeclaration` 成员逐个检查修饰符。

**测试计数（全绿）**：117 个 `#[test]` 单测（+3 grammar）+ 101 个 doctest（+1）。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt --check` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/grammar*` / parser-error 最小子集——重复/冲突/可访问性修饰符诊断对应这些目录最基础用例。

**本轮 DEFER（带 blocked-by）**：
- 装饰器 grammar；完整修饰符顺序/位置矩阵（`must precede`/`cannot appear on a type member/index signature/type parameter` 等；多数位置规则需该 token 真被解析为修饰符才可达，顶层 `static`/`public` 会被解析为标识符）。→ 4j+。
- 严格模式保留字检查（`implements`/`interface`/… 作标识符）。→ 4j+（blocked-by: strict-mode 上下文检测 from `NodeFlags`/源）。
- 其余 grammar 族（语句/类型/heritage/索引签名/解构/for-in-of/标签…）+ 依赖 `compilerOptions` 的路径。→ 4j+（blocked-by: tsoptions/program wiring）。

## 4j 落地记录（worklog 摘要）

**严格 TDD（红→绿）**：tracer bullet 先行 —— `symbol_to_string` 以空串桩看红（`Foo`→""），实现转绿。随后逐行为：命名 interface→"Foo"、type reference→"Box<string>"、匿名 object→`{ value: string; }`、union→"A | B"（程序感知递归）、intrinsic/literal 委托确认；最后回填诊断：缺属性消息收紧为 "Property 'baz' does not exist on type 'Foo'"（先看红 `{ ... }`→接线 `nodebuilder::type_to_string` 转绿）。每步单独 `cargo test` 看红/绿。

**本轮交付**：
- `core/nodebuilder.rs`：`type_to_string(checker, program, ty)`（程序感知 + `&mut` 触发惰性成员解析）：union→` | ` 递归；type reference→`target<args>`（target 符号名 + 4e 实参打印）；命名 object→符号名；匿名 object→`serialize_members` 成员字面量 `{ k: T; }`；intrinsic/literal/其余委托程序无关 `Checker::type_to_string`。`symbol_to_string(program, sym)`→声明名。
- 诊断接线：`core/check.rs` 缺属性 + `core/jsx.rs` 属性不符改用 `nodebuilder::type_to_string`，命名类型显示真名而非 `{ ... }`。
- `lib.rs` re-export `type_to_string`/`symbol_to_string`。
- 设计取舍：程序无关 `Checker::type_to_string`（4a 占位）保留供 doctest/intrinsic 快速打印（doctest 无法构造 `StubProgram`，dev-deps 不可见）；程序感知的真实序列化器为自由函数（`&mut Checker` + `program`），诊断走它。

**测试计数（全绿）**：123 个 `#[test]` 单测（+6 nodebuilder）+ 103 个 doctest（+2）。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt --check` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/` 类型打印相关（`*.types` baseline 的命名/引用/匿名/union 文本）最小子集——4j 的 `type_to_string` 命名/引用/成员/union 对应这些 baseline 的最基础形态。

**本轮 DEFER（带 blocked-by）**：
- 函数/构造签名 `(x: T) => U`、array/tuple、intersection（`A & B`，类型种类未建）、mapped/conditional/template、alias 名、可选/只读成员装饰、声明顺序成员（当前按 `properties` 顺序）。→ 4k+（blocked-by: 这些类型种类尚未构造）。
- 完整 `NodeBuilder` 门面（`type_to_type_node`/序列化为声明/scope 追踪/符号可达性/`SymbolTracker`）+ hover 展开。→ 4k+（node builder + services）。

## 4k 落地记录（worklog 摘要）—— checker 移植子阶段收口

**严格 TDD（红→绿）**：tracer bullet 先行 —— `EmitResolver::is_declaration_visible` 以 `false` 桩看红（`export function f`→应 true），实现 `export` 修饰符规则转绿。随后逐行为：`serialize_type_of_declaration`（取声明类型 → 4j `type_to_string` → "Foo"）、`is_implementation_of_overload`（带 body + 符号多声明）、`Checker::new_checker(program)` 前向入口。每步单独 `cargo test` 看红/绿。

**本轮交付**：
- `core/emit_resolver.rs`：`EmitResolver`（ZST 值句柄，遵循无 back-pointer 模型）；`is_declaration_visible`（顶层带 `export` 即可见）；`serialize_type_of_declaration`（复用 4j node builder）；`is_implementation_of_overload`（function body + `symbol.declarations > 1`）。
- `core/mod.rs`：`Checker` 增 `emit_resolver: OnceCell<EmitResolver>` + `get_emit_resolver`（缓存）+ `new_checker(program)` 前向兼容入口（4k 仅初始化 intrinsic，全局/lib 绑定 blocked-by P6）。
- `lib.rs` re-export `EmitResolver`。

**测试计数（全绿，最终）**：127 个 `#[test]` 单测（+4：3 emit_resolver + 1 new_checker）+ 109 个 doctest（+6）。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt --check` 干净；`bash docs/rust-rewrite/scripts/gate-code.sh` 全绿（C1–C8，整 workspace 保持绿）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`emitResolver`/`declarationEmit` 最小子集——可见性 + 类型序列化 + overload 实现判定对应 `.d.ts` baseline 的最基础判定。

**本轮 DEFER（带 blocked-by）**：
- `is_value_aliased`/`is_referenced_alias`（跨模块别名/导入解析）、`get_type_reference_serialization_kind`（需 `printer.TypeReferenceSerializationKind` + 类型名解析）、`create_type_of_declaration`（输出类型节点，需完整 node builder + `SymbolTracker`）、其余 `EmitResolver` 方法。→ P6 / P5 消费者驱动。
- `is_declaration_visible` 的 global-script-file 分支（非 module 脚本里非 export 也可见）+ ambient module + 成员/嵌套可见性。→ blocked-by: external-module 检测 + `compiler.Program`（P6）。

## 4l 落地记录（worklog 摘要）—— program 保留 + pool 驱动面

**目标**：让 checker 对齐 Go 的「`NewChecker(program)` 保留 program，之后 `checkSourceFile(file)` / `getDiagnostics(file)` 基于保留的 program 工作」模型，从而能被多-checker pool 干净驱动（解锁 compiler P6-2/P6-3）。

**严格 TDD（逐行为 red→green，一次一个）**：
1. **S1 保留 program**（tracer bullet）：`new_checker` 改 `Rc<dyn BoundProgram>` + 加 `program` 字段 + `program()` 访问器；stub 仍忽略 program → `new_checker_retains_program` 断言 `program().root()` 红（`None` vs `Some(NodeId(7))`）→ 存入 `Some(RetainedProgram(program))` 绿。
2. **S2a `check_source_file(file)` 驱动**：去掉 per-call program 参数，空 stub → 迁移后 `undefined_identifier_reports_cannot_find_name` 红（0 vs 1 诊断）→ 从 `retained_program()` 克隆 `Rc` 句柄 + 遍历语句 `check_statement` 绿。
3. **S2b 幂等**：`check_source_file_is_idempotent`（两次 check）红（2 vs 1）→ 加 `checked_files: FxHashSet<NodeId>` 守卫（Go `sourceFileLinks.typeChecked`）+ `mark_file_checked` 绿。
4. **S3 `get_diagnostics(file)` 触发检查**：`get_diagnostics_triggers_checking`（只调 get，不显式 check）红（0 vs 1）→ `get_diagnostics(&mut self, file)` 内部调 `check_source_file(file)` 再返回（Go `getDiagnostics` 自跑 `checkSourceFile`）绿。
5. **S4 端到端 2339**：`get_diagnostics_drives_property_does_not_exist` 经纯驱动面（`new_checker(rc)` → `get_diagnostics(root)`）断言 `Property 'baz' does not exist on type 'Foo'.`(2339) 绿——验证 pool 驱动面端到端组合（2304/2339 两类 Go 已知语义诊断）。

**program 保留的所有权方案 + divergence**：见 `lib.rs` 顶部"Retained program (sub-phase 4l)"。要点：
- Go `c.program = program`（共享非拥有指针）→ `Rc<dyn BoundProgram>`（PORTING §3：共享非空指针 → `Rc`）。用 `RetainedProgram(Rc<dyn BoundProgram>)` newtype 包一层手写 `Debug`，使 `Checker` 保留 `#[derive(Debug)]`。
- **关键设计取舍**：`Checker` **不引入生命周期参数**（避免 ripple 到 ~200 处 `&mut Checker` 调用点与遍布 doctest 的程序无关 `Checker::new()`）。借用式 `Checker<'a>` 或 `Rc<dyn ... + 'a>` 均会把生命周期传染到全 crate，故选 `Rc<dyn BoundProgram>`（`'static`）。
- divergence：(a) `Rc` 非 GC/裸指针；(b) `'static` 要求 program 拥有其数据（测试桩 `StubProgram` 满足；compiler 借用式 `BoundFile<'a>` 需先 owned/`Rc`-share）；(c) `Rc` 非 `Arc`——pool 当前顺序驱动（parallel-over-checkers 是 PORTING §6 的 DEFER），并行落地时换 `Arc<dyn BoundProgram + Send + Sync>`。

**对外 public API 形状（pool 怎么驱动）**：
```rust
let checker = Checker::new_checker(Rc::clone(&program)); // 每个 pool slot 一个，共享同一 program
let diags = checker.get_diagnostics(file);               // 每文件：检查（幂等）+ 收诊断
```
- `new_checker(Rc<dyn BoundProgram>) -> Checker`：保留 program。
- `program(&self) -> Option<&dyn BoundProgram>`：取回保留的 program。
- `check_source_file(&mut self, file)`：基于保留 program 检查（幂等；无 program 的 intrinsic-only checker 为 no-op）。
- `get_diagnostics(&mut self, file) -> &[Diagnostic]`：触发 `check_source_file(file)` 再返回（Go-faithful）。

**测试增量**：131 单测（+4：`new_checker_retains_program`/`check_source_file_is_idempotent`/`get_diagnostics_triggers_checking`/`get_diagnostics_drives_property_does_not_exist`）+ 110 doctest（+1：`program()`）。相对 4a–4k 基线 127+109。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**破坏 compiler 调用点（需下一轮 compiler 适配，明确 flag）**：
- `internal/compiler/checkerpool.rs`：`Checker::new_checker(&seed)`（`seed: BoundFile<'a>`）→ 需改 `Checker::new_checker(program_rc)`，且 `program_rc: Rc<dyn BoundProgram>` 要求 `'static`——compiler 的借用式 `BoundFile<'a>` 需重构为 **owned/`Rc`-shared** 的 `BoundProgram`（如 `Rc` 持有/共享 arena + bind 结果）。届时 pool 可：`new_checker(Rc::clone(&program))` 建 K 个 checker（共享一份 program）；每文件 `checker.get_diagnostics(file)` 检查 + 收诊断（替代当前 DEFER 的"无法驱动"占位）。
- 这正是 compiler P6-2/P6-3 的解锁点：pool 现在有了"构造（保留 program）+ per-file 检查/收诊断"的完整 public 面。

**本轮 DEFER（带 blocked-by）**：
- 多文件诊断按文件名过滤（`getDiagnostics` 的 `collection.GetDiagnosticsForFile(name)`）——保留 program 当前是单文件桩，所有诊断即该文件。blocked-by: 多文件 `compiler.Program`（P6）。
- `new_checker` 的全局/lib 绑定 + `getGlobalType` 全量 + 多文件符号查询 + 跨模块别名。blocked-by: `compiler.Program` + lib globals（P6）。
- `checkSourceFile` 的完整面（deferred diagnostics / unused 检查 / 外部模块导出 / cancellation / 语句声明全量）。blocked-by: 4g+ 后续 checker 轮 + program wiring。
- 并行：pool 顺序驱动；rayon-over-checkers 需把 `Rc` 换 `Arc` + program `Send+Sync`。blocked-by: PORTING §6 并行化（P6）。

## 4m 落地记录（worklog 摘要）—— 变量声明赋值性诊断 + block 递归

**目标**：扩展**可达**的类型检查诊断覆盖面。4l 已让 checker pool 端到端驱动 `get_diagnostics(file)`，本轮把 Go `checkVariableLikeDeclaration` 的**赋值性校验**那一臂移植上线——`var x: number = "s"` 这类初始化器与注解类型不匹配立即经真实管线产出 `2322`。这是 impl.md「后续子阶段深化」清单里"赋值/控制流语句"的第一片，**全程仅用单文件 bound program + 已移植的 4d 关系引擎 + 4j node builder**，无 lib globals / 多文件 program 依赖。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test` 看红/绿）**：
1. **S1 不可赋值对象类型 → 2322（tracer bullet）**：`interface A{x:number} interface B{x:string} declare const b:B; var a:A=b;`。`check_statement` 此前忽略 `VariableStatement` → `variable_initializer_not_assignable_reports_diagnostic` 红（0 vs 1 诊断）→ 加 `VariableStatement` 臂 + `check_variable_declaration`（取符号注解类型 `get_type_of_symbol` → `check_expression(initializer)` → `is_type_assignable_to` → 不符则 `error(2322)`，源/目标经 4j `type_to_string`）→ 绿（"Type 'B' is not assignable to type 'A'."）。
2. **S2 literal 源广义化**：`var x: number = "s";`。当前实现直接打印初始化器类型 → 消息为 `Type '"s"' ...` 红（vs `Type 'string' ...`）→ 加 `generalized_source_for_error`（Go `reportRelationError` 的 `generalizedSource`）+ `is_literal_type`/`type_could_have_top_level_singleton_types`/`get_base_type_of_literal_type`（均 Go 同名函数的 4m 子集）→ 绿（"Type 'string' is not assignable to type 'number'."）。
3. **S3 可赋值初始化器 → 零诊断（守卫）**：`var s: string = "ok"; var n: number = 1;` → `[]`（literal→primitive 可赋值，防假阳）。
4. **S4 未注解变量 → 零诊断（守卫）**：`var z = "s";` → `[]`（无注解时 `get_type_of_symbol`=`any`，初始化器检查不假阳；初始化器推断本身 DEFER）。
5. **S5 block 递归**：`{ var x: number = "s"; }`。`check_statement` 此前不进 `Block` → `variable_declaration_in_block_is_checked` 红（0 vs 1）→ 加 `Block` 臂递归 `check_statement`（Go `checkBlock`→`checkSourceElements`）→ 绿（嵌套声明仍报 2322）。

**本轮交付（全部 `core/check.rs`，均私有 fn / 私有臂）**：
- `check_statement`：新增 `VariableStatement` 臂（遍历 `VariableDeclarationList.declarations`）与 `Block` 臂（递归嵌套语句）。
- `check_variable_declaration`：identifier 名 + 有初始化器 + `node == symbol.value_declaration` 时，`is_type_assignable_to(初始化器类型, get_type_of_symbol(注解))`，不符报 `Type_0_is_not_assignable_to_type_1`(2322)，错误节点=声明（Go `errorNode=node`）。
- `generalized_source_for_error` + `is_literal_type` + `type_could_have_top_level_singleton_types` + `get_base_type_of_literal_type`：错误消息里 literal 源广义化为基础类型（`"s"`→`string`，`1`→`number` 等）。

**测试增量**：136 单测（+5：`variable_initializer_not_assignable_reports_diagnostic`/`variable_initializer_literal_generalizes_to_base_type`/`variable_initializer_assignable_reports_no_diagnostic`/`unannotated_variable_initializer_reports_no_diagnostic`/`variable_declaration_in_block_is_checked`）+ 110 doctest（+0，无新增 `pub fn`）。相对 4l 基线 131+110。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：本轮**未新增/未改任何 `pub fn`**——`new_checker(Rc<dyn BoundProgram>)`/`program()`/`check_source_file(&mut self, file)`/`get_diagnostics(&mut self, file)` 签名原样保留，新增物全为私有方法与私有 `check_statement` 臂，故 `tsgo_compiler` 调用面不受影响（4l 已记录的 compiler 适配仍是下一轮 compiler 的活，与本轮无关）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeRelationships/assignmentCompatibility/` + `variableDeclarations/` 最小子集——"注解 vs 初始化器赋值性 2322 + literal 源广义化文本"对应这些目录最基础用例（无 widening 推断 / 上下文类型 / 联合判别）。

**本轮 DEFER（带 blocked-by）**：
- 赋值表达式 `x = y` 的赋值性、其余语句容器（if/for/while/try/switch 体的递归检查）、类成员体检查、未用检查。→ 4n+（blocked-by: 各语句臂逐片移植）。
- 变量声明深化：binding-pattern（解构）、参数初始化器、for-in/of 初始化器、`using`/`await using` disposability、definite-assignment `!`、装饰器；**未注解变量经初始化器 widening 推断**后再做赋值性（当前未注解→`any`，不报错）。blocked-by: 解构 + 参数/函数体 + 初始化器 widening/推断 + lib globals(P6)。
- `generalized_source` 的 union（全 unit 成员）/ enum-like 基础类型 / instantiable 约束分支。blocked-by: union literal 构造 + enum 基础类型 + 约束解析。
- literal **类型注解**（`var x: 1 = 2`）：`get_type_from_type_node` 尚未构造 literal 类型节点（→ 注解解析为 error-type，不报错）。blocked-by: literal 类型节点构造（4d+）。

## 4n 落地记录（worklog 摘要）—— 赋值表达式赋值性 + 语句容器递归

**目标**：继续扩展**可达**的诊断覆盖面（沿 4m「后续子阶段深化」清单的"赋值/控制流语句"片）。两片均**只用单文件 bound program + 已移植的 4d 关系引擎 + 4j node builder**，无 lib globals / 多文件 program 依赖；全程经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test` 看红/绿）**：

*Slice 1 — 赋值表达式赋值性（`x = y`）*
1. **S1 不可赋值对象类型赋值 → 2322（tracer bullet）**：`interface A{x:number} interface B{x:string} declare const a:A; declare const b:B; a = b;`。`check_expression` 此前忽略 `BinaryExpression` → `assignment_expression_not_assignable_reports_diagnostic` 红（0 vs 1）→ 加 `BinaryExpression` 臂 + `check_binary_expression`（求左右操作数类型）+ `check_assignment_operator`（`EqualsToken`：`is_reference_expression(LHS)` 守卫 → `is_type_assignable_to(右值,左值)`，不符 `error(2322)`，错误节点=LHS，源经 4m `generalized_source_for_error` 广义化）+ `is_reference_expression`（Go `checkReferenceExpression` 子集）→ 绿（"Type 'B' is not assignable to type 'A'."）。
2. **S2 literal 源广义化（守卫，绿即过）**：`declare const n:number; n = "s";` → "Type 'string' is not assignable to type 'number'."（复用 4m 广义化路径）。
3. **S3 可赋值赋值 → 零诊断（守卫）**：`declare const a:A; declare const a2:A; a = a2;` → `[]`（防假阳）。

*Slice 2 — 语句容器递归（Go `checkSourceElement` 各臂）*
4. **S4 if then-body（tracer bullet）**：`if (true) { y; }`。`check_statement` 此前不进 `IfStatement` → `statement_in_if_then_body_is_checked` 红（0 vs 1）→ 加 `IfStatement` 臂（`check_expression(cond)` + 递归 then/else）→ 绿（2304 "Cannot find name 'y'."）。
5. **S5 if else-body（守卫，绿即过）**：`if (false) {} else { y; }` → 2304。
6. **S6 while-body**：`while (true) { y; }` 红（0 vs 1）→ 加 `WhileStatement` 臂 → 绿（2304）。
7. **S7 for-body**：`for (;;) { y; }` 红 → 加 `ForStatement` 臂（init·cond·incr+body，init 为 decl-list 走新抽出的 `check_variable_declaration_list`，并回收 `VariableStatement` 臂复用之）→ 绿（2304）。
8. **S8 for-initializer 声明（守卫，绿即过）**：`for (var x:number="s"; ;) {}` → 2322（init 的声明列表被检查）。
9. **S9 try-block**：`try { y; } catch (e) {}` 红 → 加 `TryStatement` 臂（try·catch·finally 块递归）→ 绿（2304）。
10. **S10/S11 catch-block / finally-block（守卫，绿即过）**：`try {} catch (e) { y; }` / `try {} finally { y; }` → 2304。
11. **S12 do-while body**：`do { y; } while (true);` 红 → 加 `DoStatement` 臂 → 绿（2304）。

**本轮交付（全部 `core/check.rs`，均私有 fn / 私有臂）**：
- `check_expression`：新增 `BinaryExpression` 分发。
- `check_binary_expression`：求左右操作数类型（始终求值，故操作数内诊断浮现）；`EqualsToken` 经 `check_assignment_operator` 后返回右值类型；其余运算符 DEFER（返回 error-type，操作数仍被检查）。
- `check_assignment_operator`：`is_reference_expression` 守卫的 LHS 上 `is_type_assignable_to(右值,左值)`，不符报 `Type_0_is_not_assignable_to_type_1`(2322)（错误节点=LHS，复用 4m 广义化）。
- `is_reference_expression`（自由 fn）：identifier / 属性访问 / 元素访问（Go `checkReferenceExpression` 子集；invalid-reference/optional-chain 诊断 DEFER）。
- `check_statement`：新增 `IfStatement`/`WhileStatement`/`DoStatement`/`ForStatement`/`TryStatement` 臂；`check_variable_declaration_list` 抽出供 `VariableStatement` 臂与 for 初始化器复用。

**最小输入 → 诊断示例（每类新诊断）**：
- 赋值表达式 2322：`declare const a:A; declare const b:B; a = b;`（A/B 同名属性类型不符）→ `Type 'B' is not assignable to type 'A'.`（错误节点=LHS `a`）。
- 语句容器递归（复用既有 2304/2322）：`if (true) { y; }` → `Cannot find name 'y'.`(2304)；`for (var x:number="s"; ;) {}` → `Type 'string' is not assignable to type 'number'.`(2322)。

**测试增量**：148 单测（+12：3 赋值表达式 + 9 语句容器递归——if-then/if-else/while/for-body/for-init/try/catch/finally/do-while）+ 110 doctest（+0，**未新增 `pub fn`**）。相对 4m 基线 136+110。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：本轮**未新增/未改任何 `pub fn`**——`new_checker(Rc<dyn BoundProgram>)`/`program()`/`check_source_file(&mut self, file)`/`get_diagnostics(&mut self, file)`/`check_expression`/`get_signatures_of_type`/`get_return_type_of_call` 签名原样保留，新增物全为私有方法、私有 `check_expression`/`check_statement` 臂与一个私有自由 fn。`cargo build -p tsgo_compiler` 绿（`checkerpool.rs` 经 `new_checker(Rc::clone)`+`get_diagnostics` 驱动面不受影响）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeRelationships/assignmentCompatibility/`（赋值表达式 2322）+ `controlFlow/`、`statements/`（if/while/do/for/try 体内诊断浮现）最小子集——对应这些目录最基础用例（无解构/复合赋值/`for-in`·`for-of`/switch/loop fixpoint）。

**本轮 DEFER（带 blocked-by）**：
- 非赋值二元运算符臂（算术 `* / % - << >> &|^`、`+`、关系 `< > <= >=`、相等 `== != === !==`、逻辑 `&& || ??`、`instanceof`、`in`、comma：操作数种类检查·结果类型·复合赋值如 `+=`/`&&=`）；解构赋值目标（`[a]=…`/`{a}=…`）。blocked-by: 各运算符臂逐片移植 + `checkArithmeticOperandType`/`reportOperatorError` + 解构。
- `checkAssignmentOperator` 深化：`checkReferenceExpression` 的 invalid-reference(2364)/optional-chain 诊断、复合赋值用 setter（writeOnly）类型、`exactOptionalPropertyTypes` 头消息、JS exports 特例。blocked-by: 这些诊断 + 写类型解析。
- 其余语句容器：`for-in`/`for-of`（需可迭代/迭代器类型）、`switch`（clause 类型 + 判别）、`with`、labeled、`return`/`throw`（需函数返回类型）；catch 变量声明检查（`checkVariableLikeDeclaration` for catch var + catch-clause grammar）；`if` 的 `checkTestingKnownTruthy…` + empty-then 诊断（需 strict-null-checks + truthiness 阐释）。blocked-by: 迭代器/可迭代类型 + switch 类型 + 函数签名/返回类型 + strict-mode 上下文。
- 类/函数/模块声明体的检查（成员体、签名、accessor、未用检查）。blocked-by: 函数签名（声明→签名）+ 类成员检查 + 未用分析。

## 4o 落地记录（worklog 摘要）—— 非赋值二元运算符 + 语句容器递归（switch/for-in/for-of）

**目标**：继续扩展**可达**的诊断覆盖面（沿 4n「后续子阶段深化」清单的"非赋值二元运算符臂"与"其余语句容器"两片）。所有片均**只用单文件 bound program + 已移植的 4d 关系引擎（assignable/comparable）+ 4j node builder + `tsgo_scanner::token_to_string`**，无 lib globals / 多文件 program 依赖；运算符结果类型测试经 `check_expression` 直接断言，诊断测试经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test` 看红/绿）**：

*Slice 组 A — 非赋值二元运算符（Go `checkBinaryLikeExpression` 各臂）*
1. **S1 关系 → boolean（tracer bullet）**：`a < b`（a/b:number）→ `check_expression` 期望 `boolean` 得到 error-type 红 → 加关系臂返回 `boolean` 绿。
2. **S2 关系不可比 → 2365**：`declare const s:string; declare const n:number; s < n;` 红（0 vs 1）→ 加 `get_base_type_of_literal_type_for_comparison` 广义化 + `relational_operands_comparable`（any||双 number-ish||双非 number-ish 且 `are_types_comparable`）+ `report_binary_operator_error` → 绿（"Operator '<' cannot be applied to types 'string' and 'number'."）。
3. **S3 关系可比 → 零诊断（守卫）**：`number < number` → `[]`。
4. **S4 相等 → boolean**：`a === b`（number）→ `check_expression` 期望 `boolean` 红 → 加相等臂返回 `boolean` 绿。
5. **S5 相等无重叠 → 2367**：`s === n`（string/number）红 → 加 `equality_operands_comparable`（双向 `is_type_equality_comparable_to`）+ literal 源广义化（`getBaseTypesIfUnrelated`）→ 绿（"This comparison appears to be unintentional because the types 'string' and 'number' have no overlap."）。
6. **S6 相等可比 → 零诊断（守卫）**：`number === number` → `[]`。
7. **S7 算术 → number**：`a - b`（number）→ `check_expression` 期望 `number` 红 → 加非 `+` 算术臂返回 `number` 绿。
8. **S8 算术 LHS 非数值 → 2362**：`declare const s:string; s - 1;` 红（0 vs 1）→ 加 `check_arithmetic_operand_type`（左操作数）→ 绿（"The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."，错误节点=左操作数）。
9. **S9 算术 RHS 非数值 → 2363**：`declare const s:string; 1 - s;` 红（S8 只查左 → 0 诊断）→ 加右操作数检查 → 绿（"The right-hand side..."）。
10. **S10 算术双数值 → 零诊断（守卫）**：`number * number` → `[]`。

*Slice 组 B — 语句容器递归（Go `checkSourceElement` 各臂）*
11. **S11 switch case-clause 语句递归（tracer）**：`switch (1) { case 2: y; }` 红（0 vs 1）→ 加 `SwitchStatement` 臂（最小：遍历 clauses 检查其 statements）→ 绿（2304）。
12. **S12 switch default-clause（守卫）**：`switch (1) { default: y; }` → 2304。
13. **S13 switch 表达式被检查**：`switch (y) {}` 红（S11 不查 switch 表达式）→ 加 `check_expression(switch expr)` → 绿（2304）。
14. **S14 case-clause 表达式被检查**：`switch (1) { case y: ; }` 红（S11/S13 不查 clause 表达式）→ 加 clause 表达式检查 → 绿（2304）。
15. **S15 for-in body 递归（tracer）**：`for (var k in {}) { y; }` 红 → 加 `ForInOrOfStatement` 臂（gate `ForInStatement`：初始化器 decl-list/expr + 迭代表达式 + body 递归）→ 绿（2304）。
16. **S16 for-in 迭代表达式被检查（守卫）**：`for (var k in y) {}` → 2304。
17. **S17 for-of body 递归**：`for (var x of []) { y; }` 红（S15 仅 gate `ForInStatement`）→ 扩 gate 含 `ForOfStatement` → 绿（2304）。
18. **S18 for-of 迭代表达式被检查（守卫）**：`for (var x of y) {}` → 2304。

**本轮交付（全部 `core/check.rs`，均私有 fn / 私有臂）**：
- `check_binary_expression`：新增关系/相等/非 `+` 算术运算符臂（结果类型 + 诊断），其余运算符仍 DEFER（返回 error-type，操作数仍被检查）。
- helper：`check_arithmetic_operand_type`（2362/2363）、`relational_operands_comparable`、`are_types_comparable`、`equality_operands_comparable`、`is_type_equality_comparable_to`、`get_base_type_of_literal_type_for_comparison`、`report_binary_operator_error`（2365/2367 选择 + `token_to_string`）。
- `check_statement`：新增 `SwitchStatement`（switch 表达式 + clause 表达式·语句递归）与 `ForInOrOfStatement`（初始化器 + 迭代表达式 + body 递归，覆盖 for-in/for-of）臂。

**最小输入 → 诊断示例（每类新诊断）**：
- 关系不可比 2365：`declare const s:string; declare const n:number; s < n;` → `Operator '<' cannot be applied to types 'string' and 'number'.`（错误节点=整个二元表达式）。
- 相等无重叠 2367：`s === n` → `This comparison appears to be unintentional because the types 'string' and 'number' have no overlap.`。
- 算术 LHS/RHS 非数值 2362/2363：`declare const s:string; s - 1;` → `The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type.`（错误节点=操作数）；`1 - s;` → 右侧版。
- 语句容器递归（复用既有 2304）：`switch (1) { case 2: y; }` / `for (var k in {}) { y; }` / `for (var x of []) { y; }` → `Cannot find name 'y'.`(2304)。

**测试增量**：166 单测（+18：S1–S18）+ 110 doctest（+0，**未新增 `pub fn`**）。相对 4n 基线 148+110。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：本轮**未新增/未改任何 `pub fn`**——`new_checker(Rc<dyn BoundProgram>)`/`program()`/`check_source_file`/`get_diagnostics`/`check_expression`/`get_signatures_of_type`/`get_return_type_of_call` 签名原样保留，新增物全为私有方法、私有 `check_expression`/`check_statement` 臂。`tsgo_compiler` 调用面（`checkerpool.rs` 经 `new_checker(Rc::clone)`+`get_diagnostics`）不受影响。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/binaryOperators/`（关系/相等/算术运算符 2362/2363/2365/2367）+ `controlFlow/`、`statements/`（switch/for-in/for-of 体内诊断浮现）最小子集——对应这些目录最基础用例（无 `+`/逻辑/复合赋值/解构/bigint/迭代元素类型化/case 可比较性）。

**本轮 DEFER（带 blocked-by）**：
- 剩余二元运算符：逻辑 `&&`/`||`/`??`（结果类型需 `TypeFacts` truthiness + `extractDefinitelyFalsyTypes`/`GetNonNullableType` + union reduction）、`+`（string 连接结果 + ESSymbol 操作数检查 + await 建议）、`instanceof`（需构造器/实例类型 + global `Function`）、`in`（`checkInExpression`）、comma（unused 左侧 + `AllowUnreachableCode`）、复合赋值 `+=`/`*=`/`&&=`/…（`checkAssignmentOperator` + setter writeOnly 类型）、bigint 算术结果 + 混合操作数 `reportOperatorError`、boolean-bitwise 建议（`The_0_operator_is_not_allowed_for_boolean_types`）、shift 简化建议（`evaluate` 常量值）。blocked-by: `TypeFacts` 全量 + lib globals(P6) + 写类型解析 + `evaluate`/`checkInstanceOfExpression`/`checkInExpression` 逐片移植 + 解构。
- `reportOperatorError` 深化：await 建议（`getAwaitedTypeNoAlias`/`errorAndMaybeSuggestAwait`）+ 同名打印时的 fully-qualified 回退（`getTypeNameForErrorDisplay`）。blocked-by: awaited-type 机制（lib globals P6）+ `TypeFormatFlags::UseFullyQualifiedType`。
- 相等运算符的 `CheckMode.TypeOnly` 抑制（flow 分析期临时收窄使操作数暂不可比时不报诊断）——本轮 `check_expression` 无 checkMode，但可达路径（if/while 条件、表达式语句）经 `check_statement` 单次检查，flow narrowing 仅对操作数（非整个二元）调 `check_expression`，故无双报；忠实复现需引入 `CheckMode`。blocked-by: `CheckMode` 参数化 `check_expression`。
- switch 深化：case-vs-switch 可比较性诊断（`checkTypeComparableTo`→2678，需关系引擎错误阐释路径）、重复 `default` grammar（A_default_clause_cannot_appear_more_than_once）、`NoFallthroughCasesInSwitch` fallthrough（需 flow `FallthroughFlowNode`）、unused-locals 注册。blocked-by: 可比较性错误阐释 + grammar + flow fallthrough + 未用分析。
- for-in/for-of 深化：for-in LHS 必须 string/any（2405）+ RHS 必须 object/any/type-param（2407，需 `getIndexTypeOrString`/`isTypeAssignableToKind` 对 NonPrimitive）、for-in 析构 LHS 诊断；for-of 迭代元素类型化（`checkRightHandSideOfForOf` → 元素类型可赋值于 LHS，需 `Symbol.iterator`/iterable 协议）、`for await` 语义、解构赋值 LHS、unused-locals 注册。blocked-by: 索引类型/迭代器·可迭代类型（lib globals P6）+ 解构赋值。

## 4p 落地记录（worklog 摘要）—— 逻辑/`+`/复合赋值运算符 + `throw`/labeled 语句

**目标**：继续扩展**可达**的诊断/结果类型覆盖面（沿 4o「剩余二元运算符 + 其余语句」清单）。所有片仅用单文件 bound program + 已移植的 4d 关系引擎 + 4g `TypeFacts`(TRUTHY/FALSY) 子集 + 4j node builder + `tsgo_scanner::token_to_string`，无 lib globals / 多文件 program 依赖；运算符结果类型测试经 `check_expression` 直接断言（或与右操作数类型比对），诊断测试经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test --lib <name>` 看红/绿）**：

*Slice 组 P — `+`/`+=` 运算符*
1. **P1 双 number-like → number（tracer）**：`a + b`（number）→ `check_expression` 期望 `number` 得 error-type 红 → 加 `+` 臂 + `is_type_assignable_to_kind_strict` → 绿。
2. **P2 任一 string-like → string**：`s + n`（string/number）红（P1 仅 number）→ 加 bigint(双)/string(任一) 分支 → 绿。
3. **P3 不可应用 → 2365**：`interface O{x:number} declare const a:O; declare const b:O; a + b;` 红（0 vs 1）→ 加 else `report_binary_operator_error`→2365 返回 any → 绿（"Operator '+' cannot be applied to types 'O' and 'O'."）。
4. **P4 any/error 不级联**：`y + 1;`（y 未定义→error 类型）红（2304 + 误报 2365 = 2）→ 在 else 前加 any/error 分支（任一 any→error/any）→ 绿（仅 2304）。

*Slice 组 L/M/Q — 逻辑运算符*
5. **L1 `||` 非假左 → 左型（tracer）**：`true || 1` → `true` 型，红（default error-type）→ 加 `||` 臂最小 `=> left_type` → 绿。
6. **L2 `||` 可假左 → union**：`s || n`（string/number）→ `type_to_string` 期望 "string | number" 红（"string"）→ 加 falsy 分支 `union(remove_definitely_falsy_types(左)=get_type_with_facts(TRUTHY), 右)` + `get_union_dropping_never` → 绿。
7. **M1 `&&` 非真左 → 左型（tracer）**：`false && 1` → `false` 型，红 → 加 `&&` 臂最小 `=> left_type` → 绿。
8. **M2 `&&` 可真左 → union（对象右 falsy 部分为空）**：`interface O{x:number} ... a && o`（a:number, o:O）→ 结果 == 右型，红（"number"≠右）→ 加真值分支 `union(extract_definitely_falsy_types(get_base_type_of_literal_type(右)), 右)`（对象 falsy 部分=never→union 折叠为右型）+ `extract_definitely_falsy_types`/`get_definitely_falsy_part_of_type` → 绿。
9. **Q1 `??` 非 nullable 左 → 左型（tracer）**：`s ?? n`（string）→ string，红 → 加 `??` 臂 `=> left_type`（nullish 精化 DEFER）→ 绿。

*Slice 组 C — 复合赋值*
10. **C1 复合算术操作数检查（tracer）**：`declare const s:string; s *= 1;` 红（0 vs 1，`*=` 原落入 default 不查操作数）→ 把复合算术 token 加入算术臂（操作数经 `check_arithmetic_operand_type`）+ `leftOk&&rightOk&&is_compound_assignment` 时 `check_assignment_operator(左,左型,number)` → 绿（2362）。
11. **C2 `+=` 赋值性 → 2322**：`declare const n:number; n += "s";` 红（0 vs 1）→ `+` 臂改返回 `Option`，有有效结果且 `+=` 时 `check_assignment_operator(左,左型,result=string)` → 绿（"Type 'string' is not assignable to type 'number'."）。
12. **C3 `&&=` 赋值性 → 2322**：`declare const n:number; n &&= "s";` 红（0 vs 1）→ `&&`/`||`/`??` 臂在 compound token 时 `check_assignment_operator(左,左型,右型=string)` → 绿（2322）。
13. **C4 `+=` 可赋值 → 零诊断（守卫）**：`declare const n:number; n += 1;` → `[]`。
14. **C5/C6 `||=`/`??=` 赋值性 → 2322（覆盖 C3 同族 token）**：`n ||= "s"` / `n ??= "s"` → 2322（验证三个逻辑复合 token 均经 `check_assignment_operator`）。

*Slice 组 S — 语句*
15. **S1 throw 表达式被检查（tracer）**：`throw y;` 红（0 vs 1，无 `ThrowStatement` 臂）→ 加臂 `check_expression(throwExpr)` → 绿（2304）。
16. **S2 labeled 递归**：`lbl: y;` 红（无 `LabeledStatement` 臂）→ 加臂递归 `check_statement(被标注语句)` → 绿（2304）。

**本轮交付（全部 `core/check.rs`，均私有 fn / 私有臂 / 私有 free fn）**：
- `check_binary_expression`：新增 `&&`/`&&=`、`||`/`||=`、`??`/`??=`、`+`/`+=` 臂；算术臂扩复合 token + 复合赋值 `check_assignment_operator` 接线。
- helper：`is_type_assignable_to_kind_strict`、`get_union_dropping_never`、`remove_definitely_falsy_types`、`extract_definitely_falsy_types`、`get_definitely_falsy_part_of_type`；free fn `is_compound_assignment`。
- `check_statement`：新增 `ThrowStatement`、`LabeledStatement` 臂。

**最小输入 → 诊断/结果类型示例**：
- `+` 结果：`a+b`(number)→number；`s+n`→string；`y+1`→error（不级联 2365）；`O+O`→`2365` "Operator '+' cannot be applied to types 'O' and 'O'."。
- 逻辑结果：`true||1`→`true`；`s||n`→`string | number`；`false&&1`→`false`；`a&&o`(o:O)→`O`；`s??n`→`string`。
- 复合赋值：`s *= 1`→`2362`；`n += "s"`→`2322`；`n &&= "s"`/`n ||= "s"`/`n ??= "s"`→`2322`；`n += 1`→`[]`。
- 语句递归（复用 2304）：`throw y;` / `lbl: y;` → "Cannot find name 'y'."。

**测试增量**：183 单测（+17：P1–P4、L1–L2、M1–M2、Q1、C1–C6、S1–S2）+ 110 doctest（+0，**未新增 `pub fn`**）。相对 4o 基线 166+110。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：本轮**未新增/未改任何 `pub fn`**——`new_checker(Rc<dyn BoundProgram>)`/`program()`/`check_source_file`/`get_diagnostics`/`check_expression`/`get_signatures_of_type`/`get_return_type_of_call` 签名原样保留，新增物全为私有方法、私有 `check_expression`/`check_statement`/`check_binary_expression` 臂、私有 free fn `is_compound_assignment`。`tsgo_compiler` 调用面不受影响。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/binaryOperators/`（`+`/逻辑/复合赋值最基础用例）+ `statements/throwStatements/`、`statements/...`（labeled）最小子集——对应这些目录无 ES-symbol/bigint 字面量/解构/strictNullChecks/迭代器的基础用例。

**本轮 DEFER（带 blocked-by）**：
- `+` 的 ES-symbol 操作数诊断（`2469` `The_0_operator_cannot_be_applied_to_type_symbol`，`checkForDisallowedESSymbolOperand`）+ await 建议（`getAwaitedTypeNoAlias`/`errorAndMaybeSuggestAwait`）+ `2365` 的 literal-operand 广义化（`getBaseTypesIfUnrelated`）。blocked-by: `Symbol` lib global(P6) + awaited-type 机制 + literal 广义化 helper（仅非 literal 操作数路径忠实）。
- `&&` 真值分支对 `string`/`number`/`bigint` **原始**类型的精确 falsy 字面量（Go 映射到 `emptyStringType`/`zeroType`/`zeroBigIntType`）。本轮 `get_definitely_falsy_part_of_type` 对这些原始返回 `never`——当另一操作数已含该原始时，与 Go 的**约简后** union 结果一致（`0|number`→`number` 等）；右操作数为字面量等少数情形会偏离。blocked-by: falsy 字面量内建 + 4b `getUnionType` 的 literal/subtype 约简。
- `??` 的 nullish 结果精化（`hasTypeFacts(EQUndefinedOrNull)` + `GetNonNullableType`）与 `checkNullishCoalesceOperands` 诊断；非 nullable 左已忠实（结果=左型）。blocked-by: `EQUndefinedOrNull` 全量 `TypeFacts` + `strictNullChecks` 接线。
- 逻辑运算符 union 成员的 flatten（union 左操作数）与 subtype 约简。blocked-by: 4b `getUnionType` flatten/约简。
- 复合赋值的 setter writeOnly 类型（`checkPropertyAccessExpression(_, writeOnly=true)`）+ `exactOptionalPropertyTypes` 阐释 + 解构赋值目标。blocked-by: 写类型解析 + `exactOptionalPropertyTypes` + 解构。
- `return`/`with` 语句**整体 DEFER**：可达（单文件、无函数体下传）路径均为 grammar-only——`return` 容器为空报 `1108`（`A_return_statement_can_only_be_used_within_a_function_body`，需 `getContainingFunctionOrClassStaticBlock` + `grammarErrorOnFirstToken`，且**不**检查表达式）；`with` 恒报 `1101`（`The_with_statement_is_not_supported...`，需 `grammarErrorAtPos`/`hasParseDiagnostics`，且**不**检查 body）。返回类型校验另需 `getSignatureFromDeclaration`/`getReturnTypeOfSignature`。忠实复现这两者须先移植 grammar 基建 + 函数签名构造 + 函数体下传，故本轮不做（仅记录确切 Go 行为）。blocked-by: grammar 基建（ambient-context/位置型诊断）+ 函数签名构造（声明→签名）+ 函数体下传。
- labeled 的 `Duplicate_label_0`（需父链 walk）+ `Unused_label`（需 `NodeFlagsUnreachable`/flow）；本轮仅做递归（与 Go 在可达 reachable 路径一致）。blocked-by: grammar 父链 walk + flow 可达性。
- `instanceof`/`in`/comma 运算符、bigint 算术混合操作数、boolean-bitwise 建议、shift 简化建议。blocked-by: lib globals(P6) + `evaluate` 常量 + `checkInstanceOfExpression`/`checkInExpression` 逐片移植。

## 4q 落地记录（worklog 摘要）—— 调用表达式实参检查（实参数 2554 + 实参类型 2345）

**目标**：让单文件 bound program 上「顶层 `function f(...)` 声明 + `f(...)` 调用」端到端可达：解析 callee 类型的 call 签名、按签名做实参数（arity）与逐实参类型（applicability）检查、返回签名返回类型。复用 4d 关系引擎（`is_type_assignable_to`）+ 4m 字面量广义化（`generalized_source_for_error`）+ 4j node builder（`type_to_string`）+ 既有 `get_signatures_of_type`/`get_return_type_of_call`（4g 起步）。无 lib globals / 多文件 program 依赖。

**前置基建（`declared_types.rs`，私有 fn / 既有 pub fn 体扩展）**：`get_type_of_symbol` 新增 `FUNCTION` 分支 → `get_type_of_func_class_enum_module`（匿名 object 类型，`call_signatures` = `get_signatures_of_symbol`，按 `value_symbol_links.resolved_type` 缓存）；`get_signatures_of_symbol`（遍历 `FunctionDeclaration` 声明）；`get_signature_from_declaration`（收集参数符号 + min 实参数 + 注解返回类型，未注解→`any` DEFER 函数体推断）；`is_optional_parameter`（`?`/初始化器/`...`）；`get_type_of_variable_or_property` 新增 `ParameterDeclaration` 注解臂（使参数类型解析）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

1. **S1 实参类型不可赋值 → 2345（tracer bullet）**：`function f(a: number){} f("s");` 红（无 `CallExpression` 臂 → 0 诊断 vs 1）→ 加 `CallExpression` 臂 + `check_call_expression`（取 `signatures[0]` + `check_applicable_signature_for_call` 逐实参 `is_type_assignable_to`，首个不符报 2345 + 广义化）+ 全套前置基建（函数符号类型/签名构造/参数类型）→ 绿（"Argument of type 'string' is not assignable to parameter of type 'number'."）。此片 min 实参数取 `parameters.len()`（全必填，最简）。
2. **S2 实参过少 → 2554**：`function f(a: number){} f();` 红（S1 后 `f()` 走 applicability，0 实参 → 0 诊断 vs 1）→ 加 `has_correct_arity`（下界 `argCount >= minArgCount`）+ `report_argument_arity_error`（过少报在 callee `call_error_node`）+ `get_min_argument_count`/`parameter_range_string`/`get_parameter_count` → 绿（"Expected 1 arguments, but got 0."）。
3. **S3 实参过多 → 2554**：`function f(a: number){} f(1, 2);` 红（S2 仅下界 → `2>=1` 通过 → applicability 不报）→ `has_correct_arity` 加上界 `argCount <= paramCount` + `report_argument_arity_error` 加过多分支（报在多余实参 `args[max]`）→ 绿（"Expected 1 arguments, but got 2."）。
4. **S4 可选参 min/max → 守卫 + 范围**：先把 S1 的 min 退回最简（`parameters.len()`）以保红；`function f(a: number, b?: number){} f(1);` 红（min=2 → `1>=2` 误报 2554）+ `f(1,2,3)` 红（范围显示 "2" 而非 "1-2"）→ `get_signature_from_declaration` 改 Go 逐参追踪（`if !is_optional_parameter { min = len(parameters) }`）→ 绿（`f(1)`→`[]`；`f(1,2,3)`→"Expected 1-2 arguments, but got 3."）。
5. **S5 结果 = 签名返回类型**：`function f(a: number): string { return ""; } f(1);` 红（`check_call_expression` 末尾返回 `error_type`）→ 改返回 `get_return_type_of_call(sig, &[], &[])`（非泛型→注解返回类型）→ 绿（`check_expression(call)`=`string`）。
- **守卫**：`function f(a: number){} f(1);` → `[]`（正确调用零诊断）。

**本轮交付（`core/check.rs` 私有方法/臂/free fn + `declared_types.rs` 私有 fn / 既有 pub fn 体扩展）**：
- `check.rs`：`check_expression` 新增 `CallExpression` 臂；`check_call_expression`/`check_applicable_signature_for_call`/`has_correct_arity`/`report_argument_arity_error`/`parameter_range_string`/`get_min_argument_count`/`get_parameter_count`/`get_type_at_position`；free fn `call_error_node`。
- `declared_types.rs`：`get_type_of_func_class_enum_module`/`get_signatures_of_symbol`/`get_signature_from_declaration`/`is_optional_parameter`；`get_type_of_symbol`（`FUNCTION` 分支）+ `get_type_of_variable_or_property`（`ParameterDeclaration` 注解）体扩展。

**最小输入 → 诊断/结果（每新增诊断一例）**：
- 2345（实参类型）：`function f(a: number){} f("s");` → "Argument of type 'string' is not assignable to parameter of type 'number'."
- 2554（实参数过少）：`function f(a: number){} f();` → "Expected 1 arguments, but got 0."
- 2554（实参数过多）：`function f(a: number){} f(1, 2);` → "Expected 1 arguments, but got 2."
- 2554（可选参范围）：`function f(a: number, b?: number){} f(1, 2, 3);` → "Expected 1-2 arguments, but got 3."
- 结果类型：`function f(a: number): string {...} f(1);` → `string`。

**测试增量**：190 单测（+7：S1–S5 + 可选参守卫 + 正确调用守卫）+ 110 doctest（+0，**未新增 `pub fn`**）。相对 4p 基线 183+110。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：本轮**未新增/未改任何 `pub fn`**——`new_checker`/`program`/`check_source_file`/`get_diagnostics`/`check_expression`/`get_signatures_of_type`/`get_return_type_of_call` 签名原样保留，新增物全为私有方法/私有臂/私有 free fn，及既有 pub `get_type_of_symbol` 的体扩展（签名不变）。`cargo build -p tsgo_compiler` 绿（`checkerpool.rs` 驱动面不受影响）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/functionCalls/`、`expressions/callExpression/`、`functions/`（实参数/可选参/实参类型不匹配最基础用例）——对应这些目录无重载/泛型/rest·spread/`this`/上下文回调/`new` 的基础用例。

**本轮 DEFER（带 blocked-by）**：
- 重载选择 + best-match（`chooseOverload`/`reorderCandidates`/`getCandidateForOverloadFailure`/`reportCallResolutionErrors`：多候选 subtype/assignable 两遍、`No_overload_matches_this_call`/`The_last_overload_gave_the_following_error` 链、`2575` `No_overload_expects_0_arguments...`）。blocked-by: 多签名收集（重载声明）+ 关系两遍 + 诊断链。
- 泛型 call-site 推断（完整）：上下文类型回传（`getContextualType`/`returnMapper`）、`inferTypeArguments` 的 `InferenceContext`、`hasCorrectTypeArgumentArity`/`checkTypeArguments`（`2558`/`2344`）。4q 仅对非泛型签名忠实（`get_return_type_of_call` 的泛型分支以空 arg/param 调用，不做推断）。blocked-by: 推断上下文 + 上下文类型。
- rest/spread 实参与 tuple 参数：`getNonArrayRestType`/`getSpreadArgumentType`/`getSpreadArgumentIndex`、`hasEffectiveRestParameter`、`getParameterCount`/`getMinArgumentCount` 的 tuple 展开、`A_spread_argument_must_either_have_a_tuple_type...`、`Expected_at_least_0_arguments_but_got_1`(2555)。blocked-by: tuple/数组类型 + spread 检测。
- `this`-实参检查（`getThisArgumentOfCall`/`getThisTypeOfSignature`/`The_this_context_of_type_0...`）、回调实参上下文类型（`checkExpressionWithContextualType` 的 `CheckModeSkipContextSensitive`）、缺 `await` 建议（`maybeAddMissingAwaitInfo`）。blocked-by: `this`-typing + 上下文类型 + awaited 类型（lib globals P6）。
- `new` 表达式（`resolveNewExpression`/构造签名/`Cannot_create_an_instance_of_an_abstract_class`/`Only_a_void_function...`）。blocked-by: 构造签名收集 + 类静态侧类型。
- 不可调用 / 未类型化调用诊断（`callSignatures` 为空 → `invocationError`/`2349`、`isUntypedFunctionCall`→`anyType`、`Untyped_function_calls_may_not_accept_type_arguments`）。4q 在无 call 签名时仅检查实参表达式后返回 `error_type`（不报 invocation 错误）。blocked-by: `getApparentType` 的原始→包装映射 + lib globals(P6)。
- 方法/访问器/箭头/函数表达式/构造器声明的签名构造、重载实现节点 de-dup、`getSignatureOfFullSignatureType`（JSDoc）、`this`-参数、类型参数（泛型签名）。blocked-by: 那些声明种类 + 泛型 + JSDoc full-signature。
- 函数体返回类型推断（`getReturnTypeFromBody`）：4q 未注解函数返回 `any`（Go 空体→`void`、有 `return`→推断）。blocked-by: 函数体下传 + 控制流返回类型推断。
- 实参数过多的多 token 合成 span（Go `args[maxCount].Pos()..args[last].End()` 经 `skipTrivia`）：4q 报在首个多余实参节点（单多余实参时 span 一致）。blocked-by: 合成 `TextRange` 诊断构造。

## 4r 落地记录（worklog 摘要）—— 重载解析（2769/2575）+ 类成员体/属性初始化器 + 函数体下传与 return 检查

**目标**：在单文件 bound program 上继续扩展**可达**诊断覆盖面，按批次推进三个子区域：(A) 调用表达式的**重载解析**（多 call 签名）、(B) **类成员体/属性初始化器**检查、(C) **函数体下传 + return 语句/带注解返回类型**检查。全部仅用单文件 bound program + 4d 关系引擎（`is_type_assignable_to`）+ 4m 字面量广义化 + 4j node builder + 4q 的签名/参数机制，无 lib globals / 多文件 program 依赖。诊断测试经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。

> **诊断码勘误**：交办单写作 "2575 'No overload matches this call.'"，但按 Go ground truth（`internal/diagnostics/diagnostics_generated.go`）该文本对应 **2769**（`No_overload_matches_this_call`）；**2575** 是 `No_overload_expects_0_arguments_but_overloads_do_exist_that_expect_either_1_or_2_arguments`（重载实参数错误）。本轮按 Go 实测码落地：重载实参类型不符 → 2769，重载实参数不符且落在最小最大之间 → 2575。

**严格 TDD（逐行为 red→green）**：

*Item A — 重载解析（`core/check.rs`）*
1. **A1（tracer，红→绿实证）**：`declare function f(a: number): void; declare function f(a: string): void; f(true);` → 期望 2769 得 2345（旧单候选路径取 `signatures[0]` 报实参类型不符）**红** → 新增 `signatures.len() > 1` 分支 `resolve_overloaded_call`（实参类型缓存一次，按声明序找首个 arity+applicable 的签名；无匹配时：>1 个 arity 命中→2769、=1 个→该候选 2345、0 个→arity 错误）**绿**（"No overload matches this call."）。
2. **A2（守卫）**：`... f("s");` → 第二重载 `string` 接受 → `[]`。
3. **A3（单 arity 命中分支）**：`declare function f(a: number): void; declare function f(a: number, b: number): void; f("s");` → 仅重载 1 arity 命中且实参不符 → 该候选 2345（无重载链）。
4. **A4（无 arity 命中 → 2575）**：`declare function f(a:number):void; declare function f(a:number,b:number,c:number):void; declare const n:number; f(n, n);` → 实参数 2 落在最小 1、最大 3 之间且匹配不到任何重载 → 2575（"No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments."）。
   > A2–A4 的分支在 A1 的 `resolve_overloaded_call` 中一并落地（同一内聚函数的不同臂），故为分支覆盖/守卫；A1 是该 item 的严格 red→green tracer。

*Item B — 类成员体/属性初始化器（`core/check.rs`）*
5. **B1（tracer，红→绿实证）**：`class C { m() { y; } }` → 期望 2304 得 0（旧 ClassDeclaration 臂仅跑 grammar）**红** → ClassDeclaration/ClassExpression 臂改为对每个成员调 `check_class_member`（method/accessor/constructor/static-block 体 → `check_statement(body)` 下传；property → `check_property_declaration`）**绿**（2304）。
6. **B2（属性初始化器不可赋值 → 2322）**：`class C { x: number = "s"; }` → 2322（字面量广义化 → "Type 'string' is not assignable to type 'number'."）。
7. **B3/B4（守卫）**：`x: number = 1` → `[]`；`x = "s"`（未注解→`any`）→ `[]`。
   > B2 的属性检查随 B1 的 `check_class_member` 一并落地；B1 是该 item 的严格 tracer。

*Item C — 函数体下传 + return 检查（`core/check.rs`）*
8. **C1（tracer，红→绿实证）**：`function f() { return y; }` → 期望 2304 得 0（函数体未下传）**红** → 新增 `FunctionDeclaration` 体下传臂 + `ReturnStatement` 臂（`check_return_statement_expression`：先检查 return 表达式，再——若有显式返回类型注解——经 `enclosing_explicit_return_type`（父链 walk 到最近 function-like 取注解 `get_type_from_type_node`）做可赋值检查）**绿**（2304）。
9. **C2（带注解返回类型不符 → 2322）**：`function f(): number { return "s"; }` → 2322（广义化 → "Type 'string' is not assignable to type 'number'."）。
10. **C3/C4（守卫）**：`function f(): string { return "s"; }` → `[]`；`function f() { return "s"; }`（未注解→DEFER 上下文推断）→ `[]`。
11. **C5（与 Item B 组合）**：`class C { m(): number { return "s"; } }` → 2322（方法体下传 + 方法显式返回类型检查）。
    > C2/C5 的返回类型检查随 C1 的 `check_return_statement_expression`/`enclosing_explicit_return_type` 一并落地；C1 是该 item 的严格 tracer。

**本轮交付（全部 `core/check.rs`，均私有方法/私有臂）**：
- `check_call_expression`：`signatures.len() > 1` → `resolve_overloaded_call`；helper `signature_applicable_with_types`（静默 applicability，用缓存实参类型）、`report_inapplicable_argument`（单 arity 命中 → 2345）、`report_overload_arity_error`（多签名 `getArgumentArityError` 子集：minCount/maxCount/maxBelow/minAbove + 2575/2554 选择）。
- `check_statement`：ClassDeclaration 臂扩 ClassExpression 并对每成员调 `check_class_member`；新增 `FunctionDeclaration` 体下传臂、`ReturnStatement` 臂。
- helper：`check_class_member`、`check_property_declaration`、`check_return_statement_expression`、`enclosing_explicit_return_type`。

**最小输入 → 诊断（每新增码一例）**：
- 2769（重载实参类型均不符）：`declare function f(a:number):void; declare function f(a:string):void; f(true);` → "No overload matches this call."
- 2575（重载实参数落区间内不匹配）：`...f(a:number)...; ...f(a:number,b:number,c:number)...; f(n,n);` → "No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments."
- 2304（类方法体 / 函数体 return 内未定义名）：`class C { m() { y; } }` / `function f() { return y; }` → "Cannot find name 'y'."
- 2322（属性初始化器 / 带注解返回类型不符）：`class C { x: number = "s"; }` / `function f(): number { return "s"; }` → "Type 'string' is not assignable to type 'number'."

**测试增量**：203 单测（+13：A1–A4、B1–B4、C1–C5）+ 110 doctest（+0，**未新增 `pub fn`**）。相对 4q 基线 190+110。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：本轮**未新增/未改任何 `pub fn`**——`new_checker`/`program`/`check_source_file`/`get_diagnostics`/`check_expression`/`get_signatures_of_type`/`get_return_type_of_call` 签名原样保留，新增物全为私有方法/私有臂。`tsgo_compiler` 驱动面（`checkerpool.rs`）不受影响。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/functionCalls/`（重载解析 2769/2575 最基础用例）+ `classes/members/`、`classes/propertyMemberDeclarations/`（成员体/属性初始化器诊断浮现）+ `functions/`、`statements/returnStatements/`（return 表达式 + 带注解返回类型）最小子集——对应这些目录无重载链阐释/泛型/上下文返回推断/heritage 的基础用例。

**本轮 DEFER（带 blocked-by）**：
- **重载链阐释**：`The_last_overload_gave_the_following_error`(2770) + `No_overload_matches_this_call`(2769) 的 message chain、`Overload_N_of_M` related-info、`getCandidateForOverloadFailure` best-match、两遍 subtype/assignable 关系、`addImplementationSuccessElaboration`。本轮仅报顶层 2769（chain 子消息推迟）。blocked-by: 诊断 message chain + related information + 关系两遍。
- **重载 + 泛型**：重载签名的类型参数推断 / `hasCorrectTypeArgumentArity` / `2743`。blocked-by: 推断上下文 + 类型实参 arity。
- **重载实现去重**：`function f(){...}`（有 body）+ 前置重载时跳过实现签名（`decl.Body()!=nil && 紧邻同 kind 前驱`）；本轮 tracer 用 `declare function`（无 body）规避。blocked-by: `getSignaturesOfSymbol` 的实现节点去重（已在 4q DEFER 登记）。
- **Heritage 检查**（Item B 的 implements/extends 可赋值性）：`class C implements I {}` 需构造类实例类型（成员符号类型）并与接口做关系阐释；`extends` 需基类型解析。本轮**整体 DEFER**——可达性受限于类实例类型构造 + 关系错误阐释 + lib globals。blocked-by: 类实例类型（`getDeclaredTypeOfSymbol` for class value/instance side）+ 关系错误阐释 + lib globals(P6)。
- **类成员深化**：签名/override/accessor-pair 一致性、parameter-property 赋值、decorators、计算名、成员体内 `this`-typing、未用检查。blocked-by: 成员级检查 + 函数签名/`this`-type 机制 + 未用分析。
- **return 深化**：未注解函数的上下文返回类型推断（Go 从 body 推断）、generator/async 返回解包（awaited/iterable，需 lib globals）、容器外 `return` 的 `1108` grammar、隐式/缺失 return 分析（需 flow 可达性）；~~函数/箭头**表达式**体下传~~ **4aq 已落地块体下传**（见 §4aq worklog）；~~concise 箭头体~~ **4ar 已落地 concise 箭头体返回检查**（`(): T => expr` 对显式注解，见 §4ar worklog；未注解推断 + async 解包仍 DEFER）。blocked-by: 上下文返回类型推断 + awaited/iterable 类型(lib globals P6) + grammar 基建 + flow 可达性。

## 4s 落地记录（grammar 检查族深化 — worklog 摘要）

**严格 TDD（逐行为 红→绿，每条都看到 0 vs 1 的真红）**：grammar 检查彼此独立，故每条都是独立 RED。每步单独 `cargo test -p tsgo_checker <name>` 看红→最小实现转绿→下一条。

落地 7 条（按 slice 顺序，每条「最小输入 → 诊断码 + 文案」）：
1. **1029**（可访问性须前于 static）：`class C { static public x = 1; }` → `'public' modifier must precede 'static' modifier.`（`check_grammar_modifiers` 可访问性臂新增 `flags.contains(STATIC)` 分支）
2. **1243**（`accessor`+`readonly` 冲突）：`class C { readonly accessor x = 1; }` → `'accessor' modifier cannot be used with 'readonly' modifier.`
3. **1024**（`readonly` 仅限属性/索引签名）：`class C { readonly m() {} }` → `'readonly' modifier can only appear on a property declaration or index signature.`
4. **1275**（`accessor` 仅限属性声明）：`class C { accessor m() {} }` → `'accessor' modifier can only appear on a property declaration.`
5. **1155**（`const` 须初始化）：`const x;` → `'const' declarations must be initialized.`（新 `check_grammar_variable_declaration`，接入 `check_variable_declaration`）
6. **1093**（构造器返回类型注解）：`class C { constructor(): void {} }` → `Type annotation cannot appear on a constructor declaration.`（新 `check_grammar_constructor_type_annotation`，接入 `check_class_member` Constructor 臂）
7. **1092**（构造器类型参数）：`class C { constructor<T>() {} }` → `Type parameters cannot appear on a constructor declaration.`（新 `check_grammar_constructor_type_parameters`，报在首个类型参数节点上）

**关键修正（红的过程中发现）**：`check_grammar_variable_declaration` 初版未守卫 ambient，`declare const x: string;`（合法，Go 走 `checkAmbientInitializer`）被误报 1155，导致 `check_test` 大面积红。改为内联 `getCombinedNodeFlags`（decl|list|statement）并在 `AMBIENT` 时跳过——转绿。`static public`/`readonly`/`accessor`/`abstract function` 等**顶层/函数位**修饰符经实测被 parser 解析为标识符（`public function f(){}` 得 2304 而非 1044），故 4s 全部用**类成员/属性/构造器**上下文（修饰符可靠 attach）。

**本轮交付（`core/grammar.rs` + `core/check.rs` 接线）**：
- `check_grammar_modifiers`：可访问性臂（1028 dup + 1029 precede-static）、`accessor`+`readonly`(1243)、`accessor` 非属性(1275)、`readonly` 非属性/索引签名(1024)。
- 新 `pub fn check_grammar_variable_declaration`（1155，含 ambient 守卫）、`check_grammar_constructor_type_annotation`(1093)、`check_grammar_constructor_type_parameters`(1092)。
- `check.rs`：`check_variable_declaration` 顶部调 grammar 检查；`check_class_member` 的 Constructor 臂调两个构造器 grammar 检查。

**测试增量**：210 单测（+7：上列 7 条 grammar slice）+ 113 doctest（+3：三个新 `pub fn` 的 §8.6 doctest）。相对 4r 基线 203+110。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：未改任何既有 `pub fn` 签名（`new_checker`/`program`/`check_source_file`/`get_diagnostics`/`check_expression`/`check_grammar_modifiers` 原样）。新增 3 个 `pub fn` 为**纯增量**；其余为私有臂/接线。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/parser/ecmascript5/...`（修饰符顺序/位置）+ `tests/cases/compiler/`（const 未初始化、构造器类型参数/返回注解）最小子集。

**本轮 DEFER（带 blocked-by）**：
- **1044 模块/命名空间元素修饰符** / **1242 abstract 位置** / **`override` 顺序矩阵** / 顶层 `static`/`public`：均需 parser 将顶层/函数位关键字解析为修饰符（实测被解析为标识符，不可达）。blocked-by: parser 修饰符解析（顶层/函数声明）。
- **abstract-in-non-abstract-class（1253/1244）**：需读父类 `abstract` 修饰符 + 抽象方法/属性无 body 规则。blocked-by: 父类修饰符判定 + abstract 成员校验。
- **构造器 `static`/`override`/`async`（1089）** / 参数属性绑定模式/rest（1187/...）：构造器后置块 + 参数属性机制未移植。blocked-by: 参数属性 + 构造器后置 grammar。
- **`checkGrammarParameterList`（1014 rest 须最后 / 1047 rest 不可选 / 1048 rest 不可有初始化器）** / **trailing-comma（1013/1025/...）**：需参数列表 grammar 接线 + 位置型诊断（`grammarErrorAtPos`）。blocked-by: 参数列表 grammar 接线 + 位置型诊断基建。
- **变量声明其余分支**：binding-pattern（解构）、`using`/`await using` 须初始化、definite-assignment(`!`)、ambient initializer（1039 等）。blocked-by: binding-element 检查 + `using` disposability + 定值断言 + ambient initializer 检查。
- 1092 报点：Go 用 `grammarErrorAtPos` 覆盖整个 `<...>` 列（去 trivia）；本轮报在首个类型参数节点（码/文案一致，span 略偏）。blocked-by: 位置型诊断基建（`grammarErrorAtPos`）。

## 4u 落地记录（worklog 摘要）—— 关系引擎可选属性深化（variance/intersection 调研 + optional-property 三切片）

**目标**：在单文件 bound program + 4d 关系引擎上深化**类型关系**的结构化比较，聚焦 prompt 列的"可选属性赋值性"（reachable 无 lib globals）。先对 Go `relater.go` 核对 variance / intersection / optional-property / `Ternary` 的可达性，确认 intersection 与 signature/variance 比较因缺类型构造器/签名比较而**整体 blocked**，只推进**可选属性**这一可达面，逐行为红→绿。

**red→green 切片（逐一观察）**：
- **切片 1（缺失可选 target 属性 OK）**：`interface S { x: number }` / `interface T { x: number; a?: string }` + `declare const s: S; var t: T = s;`。RED：`var t: T = s` 报 `2322`（`properties_related_to` 见 target 缺 `a` 即 false）。GREEN：`properties_related_to` 引入 `require_optional_properties = relation∈{Subtype,StrictSubtype}`，缺失 source 属性时若 target 属性可选且非 subtype 关系则放过（Go `getUnmatchedPropertiesWorker` 的 `requireOptionalProperties || !optional` 守卫）。观察：诊断 0。`core/check_test.rs:missing_optional_target_property_is_assignable`
- **切片 2（可选 source → 必需 target 不可赋值）**：`interface S { a?: string }` / `interface T { a: string }` + `var t: T = s;`。RED：无诊断（属性同名同型→true）。GREEN：属性类型 related 后追加 Go `propertyRelatedTo` 的 optional-in-source/required-in-target 分支（`symbol_is_optional(source) && symbol_is_class_member(target) && !symbol_is_optional(target)` → false）。观察：诊断 1（`2322`）。`core/check_test.rs:optional_source_property_not_assignable_to_required_target`
- **切片 3（comparable 对 optional 宽松）**：`interface S { a?: string }` / `interface T { a: string }`。RED：`is_type_comparable_to(s,t)` 为 false（切片 2 的检查未排除 comparable）。GREEN：该检查加 `relation != Comparable` 门（Go `propertyRelatedTo` 的 `skipOptional = relation == comparableRelation`）。观察：`!is_type_assignable_to` 且 `is_type_comparable_to`。`core/relations_test.rs:comparable_is_lenient_about_optional_vs_required`

**新增私有 helper**：`symbol_is_optional`（`SymbolFlagsOptional`）、`symbol_is_class_member`（`Method|Accessor|Property`，镜像 `ast.SymbolFlagsClassMember`）。

**测试增量**：216 单测（+3：切片 1–3）+ 113 doctest（+0，**未新增 `pub fn`**）。相对 4t 基线 213+113。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 不变（compiler 保持绿）**：未改任何既有 `pub fn` 签名。新增全为 `properties_related_to` 内的私有精化 + 两个私有 helper。

**本轮 DEFER（带 blocked-by）**：
- **Intersection 处理（target `A&B` 需 source related to 每个成员 / source `A&B` 任一成员 related）**：`TypeData` 无 `Intersection` 变体、`get_type_from_type_node` 对 `IntersectionType` 节点返回 `error_type`、无 intersection interning/构造器，故无法构造 `A & B` 驱动诊断。blocked-by: `TypeData::Intersection` + `get_type_from_type_node` 的 intersection 臂 + intersection interning（4e+）。
- **Variance / 方法-属性 bivariance / signatures·index 比较**：`structuredTypeRelatedTo` 的签名/索引签名比较与方差计算（`relateVariances`/`signaturesRelatedTo`/`indexInfosRelatedTo`）未移植；无法用方法成员/索引签名/泛型方差驱动。blocked-by: 签名关系比较 + variance 计算（`getVariances`）+ array/`ReadonlyArray` 经 lib globals。
- **`Ternary`(True/False/Maybe) 递归模型**：当前用乐观 `true` 缓存破环（4d）；完整 `Maybe` 阈值/`maybeKeys` 栈未移植。blocked-by: 完整 relater 递归模型（4f+）。
- **`exactOptionalPropertyTypes` / strictNullChecks 的可选属性语义**（`isPropertySymbolTypeRelated` 对 partial-union/可选 target 注入 `undefined`、`getNonMissingTypeOfSymbol`）：需 compiler options 接线 + `missingType`/`getNonMissingTypeOfSymbol`。blocked-by: compiler options 接线（strictNullChecks/exactOptionalPropertyTypes）。
- **readonly / strictSubtype 成员序**（`{readonly a}` 非 `{a}` 的 strict-subtype）：需 `isReadonlySymbol` + strictSubtype 的诊断可达路径（仅用于 subtype reduction）。blocked-by: `isReadonlySymbol` + union subtype reduction 接入。
- **private/protected 成员可达性 + 错误细化**（`Property_0_is_private/protected...`、`Types_of_property_0_are_incompatible` 等 elaboration）：当前仅产出顶层 `2322`。blocked-by: 关系错误链 reporting（4f+）。

## 4v 落地记录（worklog 摘要）—— 交叉类型 `TypeData::Intersection`（解 4u 的 intersection DEFER）

**目标**：补齐类型系统的 intersection 面——4u 明确 DEFER 的"wire `TypeData::Intersection` + `get_type_from_type_node` intersection 臂 + intersection interning"。逐行为红→绿，全程 reachable（无 lib globals / 无带约束泛型）。

**red→green 切片（逐一观察）**：
- **切片 1（构造器 + interning）**：新增 `TypeData::Intersection(IntersectionType { types: Vec<TypeId> })` 变体 + `intersection_types` interning map（`FxHashMap<Vec<TypeId>, TypeId>`，按 sorted member-id 做键，镜像 union sibling）+ `Checker::get_intersection_type`（先以 `todo!()` 占位）。RED：`get_intersection_type([A,B])` panic（`todo!`）。GREEN：实现 Go `getIntersectionType` 的 reachable 核——`add_type_to_intersection` 扁平化嵌套 intersection、`getRegularTypeOfLiteralType` 归一 fresh 字面量、去重、累积 `includes`；再 trivial reduction：`never` 短路、`any` 短路（保留 `error`）、`unknown` 丢弃（identity）、空集 → `unknown`、单成员塌缩、否则 intern。观察：两次 `[A,B]`（含 `[B,A]`）→ 同 `TypeId`，flags `INTERSECTION`，成员 `[A,B]`。`core/mod_test.rs:get_intersection_type_interns_by_members` / `_trivial_reductions` / `_flattens_and_dedups`
- **切片 2（type-node 臂）**：`get_type_from_type_node` 对 `Kind::IntersectionType` 原返回 `error_type`。RED：`var i: A & B` 的注解节点解析得 `error_type`（flags ANY），期望 `INTERSECTION`。GREEN：新增 `get_type_from_intersection_type_node`——逐成员节点 `get_type_from_type_node` 后 `get_intersection_type`（镜像 Go `getTypeFromIntersectionTypeNode`，`X & {}` no-supertype-reduction 特例与 alias 归因 DEFER）。观察：节点解析为 `A & B` 的 intersection。`core/declared_types_test.rs:type_from_type_node_resolves_intersection`
- **切片 3（关系规则）**：`structured_type_related_to` 增 target/source intersection 臂。RED：`AB`（含 `x,y`）对 `A & B` 不可赋值、`A & B` 对 `A` 不可赋值（无 intersection 处理）。GREEN：**target intersection** = source related to 每个成员（Go `typeRelatedToEachType`）；**source intersection** = 任一成员 related to target（Go `someTypeRelatedToType`/`IntersectionStateSource`）；置于 union 臂之后（union 始终在顶层先解构）。观察：`AB→A&B` ok / `A→A&B` 否；`A&B→A` ok / `A&B→B` ok。`core/relations_test.rs:assignable_to_target_intersection_requires_each_constituent` / `assignable_from_source_intersection_needs_some_constituent`
- **切片 4（type_to_string + 端到端 2322）**：端到端 `declare const a: A; var v: A & B = a;` 经 `get_diagnostics`。RED：报 `2322` 但 target 印为 `{ ... } & { ... }`（`nodebuilder::type_to_string` 无 intersection 臂，落到 program-less printer 的对象占位）。GREEN：`nodebuilder::type_to_string` 增 intersection 臂（成员 program-aware，`" & "` join）+ program-less `Checker::type_to_string` 的 `TypeData::Intersection` 臂同样 `" & "` join。观察：诊断 1 条 `2322`，消息 `Type 'A' is not assignable to type 'A & B'.`；可赋值情形（`var v: A & B = ab` where `ab: A & B`）诊断 0。`core/check_test.rs:variable_initializer_not_assignable_to_intersection_reports_diagnostic` / `_assignable_to_intersection_reports_no_diagnostic`；`core/nodebuilder_test.rs:type_to_string_intersection_of_named`

**新增公开项**：`TypeData::Intersection` 变体、`IntersectionType` struct（+ re-export）、`Type::intersection_types()`、`Checker::get_intersection_type`。新增私有 helper：`add_type_to_intersection`、`regular_type_of_literal_type`、`intern_intersection`、`get_type_from_intersection_type_node`。

**测试增量**：225 单测（+9：切片 1 三测 + 切片 2 一测 + 切片 3 两测 + 切片 4 三测）+ 115 doctest（+2：`IntersectionType` struct 与 `Checker::get_intersection_type` 各一条 `# Examples`）。相对 4u 基线 216+113。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净。

**公开 API 仅做加法（compiler 保持绿）**：新增 `TypeData::Intersection` 为 enum 变体的加法；唯一对 `TypeData` 的穷尽 `match`（`mod.rs:type_to_string`）已补臂。未改任何既有 `pub fn` 签名。

**本轮 DEFER（带 blocked-by）**：
- **disjoint-domain / unit reduction**（`string & number` → `never`、两个互异 unit 类型 → `never`、`A & {}` 的 no-supertype-reduction）：需 `DISJOINT_DOMAINS` 完整守卫 + `emptyTypeLiteralType`/`missingType` 构造。blocked-by: 这些类型/选项尚未建。本轮成员用 type-parameter / object 接口（不触发 disjoint reduction）驱动。
- **type-variable 约束 reduction**（`T & P` 当 `T extends ...` → `T` 或 `never` 或 `IsConstrainedTypeVariable`）：需 `getBaseConstraintOfType` + `isTypeStrictSubtypeOf`。blocked-by: 带约束泛型 + base-constraint 解析。
- **union 分配律**（`X & (A | B)` → `X & A | X & B`、divide-and-conquer、cross-product、origin 附着）：需 `getCrossProductIntersections`/`checkCrossProductUnion`。blocked-by: union 叉积构造（4e+）。
- **source 成员顺序保留**：Go 保留 intersection 成员的源序；本轮按 id 排序（镜像 union sibling，interning 键稳定即可满足可达赋值行为）。blocked-by: 全顺序保留需独立 intern 键策略，非本轮可达诊断所需。
- **source intersection 的"整体作对象"结构化回退**（`A & B` 经合成 intersection 属性结构性 related to `AB`）：需 `getPropertiesOfType` 的 intersection 成员合成。blocked-by: union/intersection 合成属性（4e+）。
- **propagating ObjectFlags / alias 归因 / `IsConstrainedTypeVariable` objectFlags**：本轮 `get_intersection_type` 以 `ObjectFlags::empty()` 构造。blocked-by: `getPropagatingFlagsOfTypes` + alias 接线。

## 4w 落地记录（worklog 摘要）—— 合成交叉属性 + union 分配律 + source-intersection 结构化回退

**目标**：续 4v 的 intersection DEFER 清单中两项可达面——「合成 intersection 属性（`getPropertiesOfType` over intersections）」与「union 分配律（`X & (A|B)` → `(X&A)|(X&B)`）」，并以前者解锁 4v 明确 DEFER 的「source-intersection synthesized-property structural fallback」（`A & B` ↔ `AB`）。逐行为红→绿，全程 reachable（无 lib globals / 无带约束泛型）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

1. **切片 1（合成 intersection 属性，tracer red→green）**：`get_property_of_type`/`get_properties_of_type` 对 `TypeData::Intersection` 原走 `as_object()` → `None`。RED：`interface A{a:number} interface B{b:string}` 经 `get_intersection_type([A,B])`，`get_property_of_type("a")` 返回 `None`（断言 `.expect("a property")` panic）。GREEN：两函数新增 intersection 臂——`get_property_of_type` 对成员逐一 `find_map`（首个含该名的成员属性符号；Go `getPropertyOfUnionOrIntersectionType` intersection 分支），`get_properties_of_type` 并所有成员属性（首个声明该名者胜，Go `getPropertiesOfUnionOrIntersectionType`）。观察：`"a"`/`"b"` 解析且类型 `number`/`string`，`"nope"`→`None`，名集 `["a","b"]`。`declared_types_test.rs::intersection_synthesizes_properties_of_each_constituent`
2. **切片 2（source-intersection 结构化回退，tracer red→green）**：`structured_type_related_to` 的 source-intersection 臂原 `return some(成员 related to target)`。RED：`A{x} & B{y}` → `AB{x,y}` 不可赋值（`some` 为 false——`A`/`B` 各缺一成员——直接 `return false`）。GREEN：镜像 Go `structuredTypeRelatedTo`——`some` 为 true 时返回 true；否则当 target 为 object 时回退 `properties_related_to(source=A&B, target)`（经切片1 的合成属性把整个 intersection 视作对象）。观察：`A&B → AB` true；守卫 `A → AB` false。`relations_test.rs::source_intersection_relates_structurally_to_object`
3. **切片 3（union 分配律，tracer red→green）**：`get_intersection_type` 对含 union 成员的集合原直接 intern 成 intersection。RED：`X & (A|B)` 得 `INTERSECTION`（≠ 分配后 union，`assert_eq!` 失败）。GREEN：trivial reduction 之后、intern 之前加 `includes.contains(UNION)` 分支 → `get_cross_product_intersections`（枚举每个 union 成员组合、逐组合递归 `get_intersection_type`、丢 `never`）+ `get_cross_product_union_size`（积/never 短路），再 `get_union_type`（Go `getIntersectionTypeEx` 的 `default` cross-product 臂）。观察：`X&(A|B)` == `(X&A)|(X&B)`，flags `UNION`。`mod_test.rs::get_intersection_type_distributes_over_union`
4. **切片 4（端到端守卫）**：`declare const ab: A & B; var v: AB = ab;` 经 `get_diagnostics`。slices 1+2 落地前该路径报 2322（source-intersection `some` 失败）；落地后 `A&B → AB` 经合成属性可赋值 → 零诊断。观察：`[]`。`check_test.rs::intersection_source_assignable_to_object_reports_no_diagnostic`

**本轮交付（均私有体扩展 / 私有 helper，公开签名不变）**：
- `core/declared_types.rs`：`get_property_of_type`/`get_properties_of_type` 新增 intersection 臂。
- `core/relations.rs`：`structured_type_related_to` 的 source-intersection 臂改「some → 否则 object 回退 `properties_related_to`」。
- `core/mod.rs`：`get_intersection_type` 新增 union-distribution 分支 + 私有 `get_cross_product_intersections`/`get_cross_product_union_size`。

**最小输入 → 可观察**：
- 合成属性：`A{a:number} & B{b:string}` → `get_property_of_type("a")`=Some(类型 `number`)、`("b")`=Some(类型 `string`)。
- 结构化回退：`A{x} & B{y}` 可赋值于 `AB{x,y}`（`is_type_assignable_to` true）。
- union 分配：`X & (A|B)` == `(X&A)|(X&B)`（同 `TypeId`，flags `UNION`）。
- 端到端：`declare const ab: A & B; var v: AB = ab;` → 零诊断（2322 不触发）。

**测试增量**：229 单测（+4：切片 1–4）+ 115 doctest（+0，**未新增 `pub fn`**）。相对 4v 基线 225+115。
`cargo test -p tsgo_checker` 绿（229 + 115）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**公开 API 仅做加法/体扩展（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`get_property_of_type`/`get_properties_of_type`/`get_intersection_type`/`is_type_assignable_to` 原样保留，新增物全为私有体扩展 + 两个私有 helper（`get_cross_product_intersections`/`get_cross_product_union_size`）。`tsgo_compiler` 驱动面不受影响。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/intersection/`（合成交叉属性 + `A & B` ↔ `AB` 结构关系）+ `types/union/`（`X & (A|B)` 规范化）最小子集——对应这些目录无 disjoint-domain reduction/带约束泛型/lib globals 的基础用例。

**本轮 DEFER（带 blocked-by）**：
- **多成员合成属性的真·交叉类型**（一个名出现在 2+ 成员时，合成属性类型应为各成员版本的 `X & Y`；Go `getUnionOrIntersectionProperty` 铸造携带交叉类型的 transient 符号）：本轮单成员名走该成员符号（类型与 Go 合成-of-one 一致），多成员名取首个成员符号（类型偏离）。blocked-by: Checker 侧 transient/合成符号 arena——`get_property_of_type` 为 `&Checker`（不可变）入口，无法铸造符号；且 `program.symbol(id)` 对非 program id 越界 panic。
- **union 属性合成**（`get_property_of_type`/`get_properties_of_type` over union：仅存在于所有成员且类型并集，Go 同名函数 union 分支）：本轮仅 intersection 分支。blocked-by: 同上 transient 符号 + union 属性"全成员都有"判定 + 属性类型并集。
- **disjoint-domain / unit reduction**（`string & number` → `never`、互异 unit → `never`）：仍需 `DISJOINT_DOMAINS` 完整守卫 + `emptyTypeLiteralType`/`missingType`。blocked-by: 这些类型/选项尚未建（4v 同条）。
- **union 分配的 strictNullChecks 快路径**（`isUnionWithUndefined`/`isUnionWithNull` 抽 `undefined`/`null` 再并）+ **divide-and-conquer**（3+ 成员、>2 输入时 `(A&B)&(C&D)`）+ **origin 附着**（denormalized intersection origin）+ `checkCrossProductUnion` 100k 上限诊断：本轮仅 `default` cross-product。blocked-by: strictNullChecks 接线 + union origin 字段 + 复杂度上限诊断基建。
- **type-variable 约束 reduction**（`T & P` 当 `T extends …`）：需 `getBaseConstraintOfType` + `isTypeStrictSubtypeOf`。blocked-by: 带约束泛型 + base-constraint 解析（4v 同条）。

**推荐下一轮（4x）**：union 属性合成（`getPropertyOfUnionOrIntersectionType` union 分支：仅全成员都有的属性浮现 + 类型并集）以解锁 `(A|B)` 上的属性访问/结构关系；同时调研 Checker 侧 transient/合成符号 arena（铸造携带 `X & Y`/`X | Y` 类型的合成符号），它是多成员交叉属性真类型与 union 属性类型并集的共同 blocker。次选：disjoint-domain reduction（需先建 `emptyTypeLiteralType`/`missingType` + `DISJOINT_DOMAINS` 守卫）。

## 4x 落地记录（worklog 摘要）—— 瞬态/合成符号 arena + union 属性合成 + 多成员交叉属性真类型

**目标**：建 4w DEFER 清单点名的共享基建——**Checker 侧瞬态/合成符号 arena**（`get_property_of_type` 是 `&Checker` 入口，不能铸造符号；且 `program.symbol(id)` 对非 program id 越界 panic），并以它解锁两项 4w 明确 DEFER 的可达面：**union 属性合成**（`getPropertyOfUnionOrIntersectionType` union 分支）与**多成员交叉属性真类型**（`{a:A}&{a:B}` → `a:A&B`）。逐行为红→绿，全程 reachable（无 lib globals / 无带约束泛型）。

**所有权 / 内部可变性方案（与 Go 的偏离）**：
- Go 从 `c.symbolArena` 用 `newSymbol`/`newSymbolEx` 铸造瞬态 `*ast.Symbol`（带 `SymbolFlagsTransient` + `CheckFlagsSyntheticProperty`），并在 `createUnionOrIntersectionProperty` 内**就地**（`*Checker` 可变）算好 `valueSymbolLinks.{containingType,resolvedType}`。
- Rust 侧：Checker 新增 `synthesized_symbols: RefCell<Vec<SynthesizedSymbol>>` + `synthesized_property_cache: RefCell<FxHashMap<(TypeId,String), Option<SymbolId>>>`。**内部可变性（`RefCell`）**让 `&Checker` 的 `get_property_of_type` 仍能铸造符号——避免把 `get_property_of_type`/relations 一大片 `&Checker` 入口改成 `&mut`（编译器依赖的公开 `&self` 面也不动）。
- **合成符号 id 空间**：高位 tag `1<<31` 区分合成 id 与 binder 程序 id（后者索引一个 `Vec`，恒为小数）。`is_synthesized_symbol(id)` 判定。所有"`program.symbol(id)` 风格"查询在读 program 前先路由合成 id：`get_type_of_symbol` 顶部短路到合成 arena；relations 的 `symbol_is_optional`/`symbol_is_class_member` 改走 `Checker::resolved_symbol_flags`（合成→arena，否则→program），不再 panic。
- **divergence：类型惰性求值**。Go 在 `createUnionOrIntersectionProperty` 内就地算 `resolvedType`；本轮铸造时只记 `containing_type`+`name`（`&Checker` 不能分配 union/intersection 类型），首次 `get_type_of_symbol`（`&mut Checker`+program）再惰性算 `getUnionType`/`getIntersectionType(propTypes)` 并缓存。结果按符号 id 等价。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

1. **切片 1（union 类型节点，tracer red→green）**：`get_type_from_type_node` 对 `Kind::UnionType` 原返回 `error_type`（4v 只补了 intersection 臂）。RED：`var u: A | B` 注解节点解析得 flags `ANY`（期望 `UNION`）。GREEN：新增 `get_type_from_union_type_node`——逐成员节点 `get_type_from_type_node` 后 `get_union_type`（镜像 Go `getTypeFromUnionTypeNode`，`UnionReductionLiteral` 模式与 alias 归因 DEFER）。观察：节点 interns 同 `get_union_type([A,B])`。`declared_types_test.rs::type_from_type_node_resolves_union`
2. **切片 2（合成符号 arena + 多成员交叉属性真类型，tracer red→green）**：`get_property_of_type` 的 intersection 臂原 `find_map` 取首个含该名的成员符号（多成员名→类型偏离首成员）。RED：`interface X{p} Y{q} A{a:X} B{a:Y}` + `A&B`，`get_property_of_type("a")` 的类型得 `X`（≠ `X&Y`）。GREEN：建合成符号 arena 基建（`SynthesizedSymbol`/`new_synthesized_property`/cache/`resolved_symbol_flags`/id-tag 路由）+ intersection 臂改 `get_intersection_property`——收集去重各成员属性符号，0→None、恰 1→该成员符号（Go `singleProp` 回退）、≥2→铸造合成符号（`SymbolFlagsProperty|Transient`+`CheckFlagsSyntheticProperty`，记 containing+name）；`get_type_of_symbol` 顶部路由合成 id 到 `get_type_of_synthesized_symbol`（惰性按 containing flags 选 union/intersection，求各成员属性类型并集/交集）。观察：`a` 类型 == `get_intersection_type([X,Y])`，flags `INTERSECTION`。`declared_types_test.rs::intersection_multi_constituent_property_has_intersected_type`
3. **切片 3（union 属性合成，端到端 tracer red→green）**：`get_property_of_type` 无 union 臂（union 的 `as_object()`=None → 返回 None → 2339）。RED：`interface A{a:number} B{a:string} type U=A|B; declare const u:U; u.a;`，`check_expression(u.a)` 得 `error_type`（期望 `number|string`）。GREEN：新增 union 分发 + `get_union_property`——收集各成员属性，distinct 多→铸造合成符号；`get_type_of_synthesized_symbol` 对 union containing 求类型并集。观察：`check_expression(u.a)` == `string | number`（== `string_or_number_type`）。`check_test.rs::check_property_access_on_union_yields_union_of_member_types`
4. **切片 4（union partial 过滤，端到端 red→green）**：切片3 的最小 `get_union_property` 未做"全成员都有"判定（缺成员的名仍按 distinct=1 返回 Some）。RED：`interface A{a:number} C{b:string} type U2=A|C; declare const u2:U2; u2.a;` → 0 诊断（误浮现，期望 2339）。GREEN：加 `present_on_all` 守卫——任一成员缺该名 → partial → None（Go `CheckFlagsReadPartial` → `getPropertyOfUnionOrIntersectionType` 返回 nil）。观察：1 诊断 2339。`check_test.rs::union_property_absent_from_one_constituent_reports_2339`

**本轮交付（公开签名不变；新增物为 `pub(crate)` arena API + 私有 helper + 既有体扩展）**：
- `core/mod.rs`：`SynthesizedSymbol` struct + `synthesized_symbols`/`synthesized_property_cache` 字段 + `SYNTHESIZED_SYMBOL_TAG`/`is_synthesized_symbol` + `Checker::{new_synthesized_property, cached_synthesized_property, cache_synthesized_property, synthesized_symbol_flags/name/containing_type/resolved_type, set_synthesized_symbol_resolved_type, resolved_symbol_flags}`（均 `pub(crate)`）。
- `core/declared_types.rs`：`get_type_from_type_node` 新增 `UnionType` 臂 + `get_type_from_union_type_node`；`get_property_of_type` 改 intersection→`get_intersection_property`、新增 union→`get_union_property`；`get_type_of_symbol` 顶部合成 id 路由 + `get_type_of_synthesized_symbol`。
- `core/relations.rs`：`symbol_is_optional`/`symbol_is_class_member` 改走 `resolved_symbol_flags`（合成符号不再 panic）。

**最小输入 → 可观察**：
- union 类型节点：`var u: A | B` → flags `UNION`（interns 同 `get_union_type([A,B])`）。
- 多成员交叉属性：`A{a:X} & B{a:Y}` → `a` 类型 == `X & Y`（合成符号，flags `INTERSECTION`）。
- union 属性合成：`type U=A|B; u.a`（A.a:number, B.a:string）→ `number | string`。
- union partial：`type U2=A|C`（a 仅在 A）→ `u2.a` → 2339。

**测试增量**：233 单测（+4：切片 1–4）+ 115 doctest（+0，**未新增 `pub fn`**）。相对 4w 基线 229+115。
`cargo test -p tsgo_checker` 绿（233 + 115）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**公开 API 仅做加法/体扩展（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`get_property_of_type`/`get_type_of_symbol`/`get_type_from_type_node`/`is_type_assignable_to` 原样保留；合成符号 arena 全为 `pub(crate)` + 私有 helper。`tsgo_compiler` 驱动面（`checkerpool.rs`）不受影响。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/union/`（union 属性访问 + 仅全成员属性浮现）+ `types/intersection/`（多成员交叉属性真类型 `A&B`）最小子集——对应这些目录无 index-signature/private-protected discriminant/lib globals 的基础用例。

**本轮 DEFER（带 blocked-by）**：
- **union partial 的非"全成员都有"机制**（index-info / object-literal `undefined` widening 的 `CheckFlagsWritePartial`、private/protected discriminant 过滤、late-bound name）：本轮只实现"全成员都有 → 浮现，否则 None"。blocked-by: 索引签名 + 修饰符可见性 plumbing + late-bound 名。
- **accessor/optional/readonly 标志传播到合成符号**（Go 的 `propFlags`/`optionalFlag`/`checkFlags` 累积：union 取并、intersection 取交）：本轮合成符号恒 `Property`（非 optional/accessor）。blocked-by: 在合成符号上接 accessor/modifier/optional 标志。
- **`getTypeOfPropertyOfType` 经引用 mapper 实例化合成属性**（泛型 union/intersection 成员）：本轮合成符号类型直接惰性求值，无 mapper（union/intersection containing 非 reference）。blocked-by: 合成符号的 `mapper` 字段 + 实例化接线。
- **`propTypes>2` 的 `CheckFlagsDeferredType` 延迟归一**（Go 用 `deferredSymbolLinks` 避免 union 爆炸）：本轮恒即时 `getUnionType`/`getIntersectionType`。blocked-by: deferred-type 符号 links。
- **`type_to_string` 命名打印**：合成 union/intersection 属性类型走既有 union/intersection 打印臂；命名对象成员仍 4j placeholder（未触本轮诊断）。blocked-by: node builder 命名打印（4j）。
- **`get_properties_of_type` 的 union 分支**（迭代 union 全成员属性）：本轮只补 `get_property_of_type` union 单名查询；relations 在顶层先解构 union 故未触。blocked-by: 需要时补 union 成员属性合集迭代。

**推荐下一轮（4y）**：在合成符号 arena 上接 **accessor/optional/readonly 标志传播**（union 取并、intersection 取交）+ **union partial 的 index-info / object-literal 分支**，使 union/intersection 属性的 modifier 语义忠实；次选 **disjoint-domain / unit reduction**（需先建 `emptyTypeLiteralType`/`missingType` + `DISJOINT_DOMAINS` 守卫，4v/4w 同条），让 `string & number` → `never`。

## 4y 落地记录（worklog 摘要）—— 合成属性 optional 标志传播 + disjoint-domain 交叉归约

**目标**：续 4x DEFER 清单两项可达面——**合成 union/intersection 属性的 optional 标志传播**（Go `createUnionOrIntersectionProperty` 的 `optionalFlag`：union 取并、intersection 取交）与 4v/4w/4x 反复 DEFER 的 **disjoint-domain 交叉归约**（`string & number` → `never`）。逐行为红→绿，全程 reachable（无 lib globals / 无 `exactOptionalPropertyTypes` / 无 unit-type/`missingType` 机制）。

**关键发现 / 方案**：
- 4x 铸造合成属性符号时恒 `SymbolFlags::PROPERTY`（无 optional）。Go 在 `createUnionOrIntersectionProperty` 内**就地**用 `*Checker` 算 `optionalFlag`（union `optionalFlag |= prop.Flags & SymbolFlagsOptional`；intersection 起始 `SymbolFlagsOptional` 后 `optionalFlag &= prop.Flags`）。本轮在铸造点读各成员属性符号的 meaning flags 来算 optional。
- **读 flags 需 program**：`get_property_of_type` 是 `&Checker`（无 per-call program 参数）。利用 4l 保留的 program——`checker.program()` 取回 `Option<&dyn BoundProgram>`，再走 `Checker::resolved_symbol_flags`（合成→arena，否则→program）读取，无程序时降级为非 optional（真实驱动路径恒有 program）。**未改 `get_property_of_type` 公开签名**。
- `DISJOINT_DOMAINS` 守卫位（types.rs:135，含 `BOOLEAN_LIKE`，与 Go types.go:472 一致）此前已建但无消费者；本轮在 `get_intersection_type` 接上 Go `getIntersectionTypeEx` 的非 strict 子集守卫。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

1. **切片 A（union optional 传播 OR，type-probe red→green）**：`get_union_property` 铸造合成符号恒 `PROPERTY`。RED：`interface A{a:number} B{a?:string}` → `A | B` 的 `a` 合成符号经 `resolved_symbol_flags` 不含 `OPTIONAL`（期望含）。GREEN：新增 `union_optional_flag`（读各 distinct 成员 flags，OR `& OPTIONAL`），铸造时 `PROPERTY | optional`。观察：`A|B` 的 `a` optional；`A|C`（两者皆 required）的 `a` 非 optional。`declared_types_test.rs::union_property_is_optional_when_optional_in_any_constituent`
2. **切片 B（intersection optional 传播 AND，type-probe red→green）**：`get_intersection_property` 同样恒 `PROPERTY`。RED：`interface A{a?:X} B{a?:Y}` → `A & B` 的 `a` 合成符号不含 `OPTIONAL`（期望含，两成员皆 optional）。GREEN：新增 `intersection_optional_flag`（起始 `OPTIONAL`，AND 各成员 flags），铸造时 `PROPERTY | optional`。观察：`A&B`（皆 optional）的 `a` optional；`B&D`（D 的 `a` required）的 `a` 非 optional。`declared_types_test.rs::intersection_property_is_optional_only_when_optional_in_all_constituents`
3. **切片 D（disjoint-domain 归约，type-probe red→green）**：`get_intersection_type` 对 `string & number` 直接 intern 成 2 成员 intersection。RED：`get_intersection_type([string, number])` ≠ `never`。GREEN：never 短路后、`any` 短路前插入 `is_disjoint_domain_intersection(includes)` 守卫（镜像 Go 25924–25929：NonPrimitive/StringLike/NumberLike/BigIntLike/ESSymbolLike/VoidLike 各域，若另一 disjoint 域同现 → `never`）。观察：`string & number`、`number & bigint`、`string & boolean`、`symbol & number`、`object & string` 皆 `never`；`string & T`（类型变量）仍 `INTERSECTION`（守卫不过度触发）。`mod_test.rs::get_intersection_type_disjoint_domains_reduce_to_never`

**切片 C（readonly 标志传播）—— DEFER（确认 Go 语义后判定不可达）**：Go `isReadonlySymbol` 需 `getDeclarationModifierFlagsFromSymbol`（声明 `readonly`/`const` 修饰符）+ `CheckFlagsReadonly`；checker 侧均未建，且 relations 尚未移植任何 readonly 消费者（Go relater.go:27505 `isReadonlySymbol(source) != isReadonlySymbol(target)` 未移植，无 strictNullChecks）。故无可观察消费者且缺修饰符基建 → 本轮 DEFER（见下 blocked-by）。

**本轮交付（公开签名不变；新增物为私有 helper + 既有体扩展）**：
- `core/declared_types.rs`：`get_union_property`/`get_intersection_property` 铸造时带 optional 标志；新增私有 `union_optional_flag`/`intersection_optional_flag`（经 `checker.program()` + `resolved_symbol_flags` 读成员 flags）。
- `core/mod.rs`：`get_intersection_type` 接 disjoint-domain 守卫；新增私有自由函数 `is_disjoint_domain_intersection`。
- rustdoc：`get_property_of_type`（optional 已传播；accessor/readonly 仍 DEFER）与 `get_intersection_type`（disjoint 已归约；unit/supertype/constraint/strictNullChecks 仍 DEFER）的 DEFER 段更新。

**最小输入 → 可观察**：
- union optional：`A{a:number} | B{a?:string}` → `a` 合成符号 optional（`A | C` 皆 required → 非 optional）。
- intersection optional：`A{a?:X} & B{a?:Y}` → `a` optional（`B & D{a:X}` → 非 optional）。
- disjoint-domain：`string & number` / `number & bigint` / `string & boolean` / `symbol & number` / `object & string` → `never`。

**测试增量**：236 单测（+3：切片 A/B/D）+ 115 doctest（+0，**未新增 `pub fn`**）。相对 4x 基线 233+115。
`cargo test -p tsgo_checker` 绿（236 + 115）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**公开 API 仅做加法/体扩展（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`get_property_of_type`/`get_intersection_type` 原样保留；新增物全为私有 helper / 自由函数 + 既有体扩展。`tsgo_compiler` 构建绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/intersection/`（`string & number` 等 disjoint-domain 归约 + 合成属性 optional 语义）+ `types/union/`（union 属性 optional 传播）最小子集——无 `exactOptionalPropertyTypes`/unit-type/lib globals 的基础用例。

**本轮 DEFER（带 blocked-by）**：
- **readonly 标志传播**（Go union 取并 / intersection 取交，经 `isReadonlySymbol`）：需 `getDeclarationModifierFlagsFromSymbol`（声明 `readonly`/`const` 修饰符解析）+ `CheckFlagsReadonly` plumbing，**且**需一个 readonly 消费者（relater.go 的 readonly 属性比较未移植）。blocked-by: 修饰符-自声明解析 + `CheckFlags` readonly + readonly 关系/赋值检查移植。
- **unit-type / `missingType` 归约**（两个互异 unit 类型 → `never`，经 `addTypeToIntersection` 的 `includes |= NonPrimitive`；`A & {}` no-supertype-reduction）：需 literal/unit 类型构造 + `emptyTypeLiteralType`/`missingType`。blocked-by: 这些类型尚未建（4v/4w/4x 同条）。
- **strictNullChecks 的 `Nullable & (Object|NonPrimitive)` → `never` 子句**与 distribution 的 undefined/null 快路：本轮只实现非 strict 子集。blocked-by: strictNullChecks 接线。
- **accessor 标志传播**（Go `propFlags` 的 `SymbolFlagsAccessor`，全成员一致才保留）：本轮合成符号恒 `Property`。blocked-by: 合成符号 accessor 标志接线。
- **`exactOptionalPropertyTypes` 下 optional 影响属性类型**（`undefined` 加入）：本轮只传播符号 optional 标志，不改属性类型。blocked-by: strictNullChecks + `exactOptionalPropertyTypes` 选项。

**推荐下一轮（4z）**：unit-type / `missingType` 归约（先建 `emptyTypeLiteralType`/`missingType` + literal/unit 类型构造，让 `"a" & "b"` → `never`、`A & {}` no-supertype-reduction），同时调研 strictNullChecks 接线以解锁 `Nullable & Object` 子句与 optional 属性 `undefined` 加入；次选 readonly 标志传播（需先建 `getDeclarationModifierFlagsFromSymbol` + readonly 关系消费者）。

## 4z 落地记录（worklog 摘要）—— 全局符号/类型解析（"lib globals" 的 checker 侧半张）

**目标**：建 checker 从 bound program 的**全局作用域**解析 GLOBAL 类型/值的能力——许多 4a–4k 的 DEFER 标注 "blocked-by: lib globals (P6)"。本轮**不**做真 lib.d.ts 文件加载（仍 P6-8 DEFER），而是让 checker 在 bound program **暴露**全局符号表时能解析它，全程用**合成全局声明**经测试 harness 驱动（script 源文件顶层声明 = 该程序的 globals，是 Go `c.globals`（合并各 global 文件 `Locals`）的单文件 stand-in）。逐行为红→绿，一次一个。

**关键发现 / 方案**：
- `resolve_name`（4b）与自由函数 `get_global_type`（4c）早已接受**显式传入**的 `globals: &SymbolTable`；缺口是 **bound program 不暴露 globals**、**Checker 无 globals 入口**，调用方（如 `check_identifier`）传 `None`。Go 的 `getGlobalSymbol(name, meaning)` = `resolveName(nil, name, meaning, …)`：`location==nil` 时跳过作用域链上行、只查 `c.globals`。
- 方案：`BoundProgram` 新增 `globals(&self) -> Option<&SymbolTable>`（默认 `None`，trait 默认方法 → **加法**，不破坏既有实现）；测试 `StubProgram` override 返回 script 根文件 `locals`（合成 globals）。Checker 新增只读入口 `get_global_symbol`/`get_global_type(name)` 读保留 program 的 globals。
- `get_apparent_type`（`&Checker` 纯函数）的 primitive→wrapper：从 `checker.global_types` **缓存**读全局 `String`（由 `get_global_type` 填充）。这样保持 `&Checker` 签名不变（仅多一次缓存读），无需把 `get_apparent_type`/`get_property_of_type` 一大片入口改 `&mut`+program。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

1. **切片 1（全局符号解析，tracer red→green）**：`Checker::get_global_symbol(name, meaning)` 读 `program.globals()`。RED：`declare var g: number;` 经 `new_checker`，`get_global_symbol("g", VALUE)` 返回 `None`（`StubProgram` 尚未 override `globals()` → 默认 `None`）→ `.expect("global g")` panic。GREEN：`StubProgram::globals()` 返回根文件 `locals`。观察：`g` 解析为 `FUNCTION_SCOPED_VARIABLE`；`get_global_symbol("nope", VALUE)`=`None`（未定义→2304 语义）；meaning 过滤 `get_global_symbol("g", TYPE)`=`None`。`core/mod_test.rs:get_global_symbol_resolves_global_value_by_meaning`
2. **切片 2（全局类型解析，tracer red→green）**：`Checker::get_global_type(name)` 读 program globals + delegately 建声明类型。RED：`interface Foo { bar: string }` + `declare const foo: Foo;` 经 `new_checker`，`get_global_type("Foo")` 临时桩返回 `None` → `.expect("global type Foo")` panic（观察真红后恢复 delegation）。GREEN：`get_global_type` = `retained_program().globals()` → `declared_types::get_global_type(self, program, name, globals)`。观察：`Foo`→object 类型且二次查同 id（缓存）；value-only 名 `foo`→`None`；未定义 `Missing`→`None`。`core/mod_test.rs:get_global_type_resolves_global_interface_off_program`
3. **切片 3（apparent-type primitive→global wrapper，tracer red→green）**：`get_apparent_type` 对 string-like 映射到已建的全局 `String`。RED：`interface String { length: number }` 注入 globals 后，`get_apparent_type(string_type)` 仍返回 `string_type`（≠ wrapper）。GREEN：`get_apparent_type` 对 `STRING_LIKE` 读 `checker.global_types["String"]`，命中即返回。观察：建 `String` 前 apparent(`string`)=`string`、`get_property_of_type(string,"length")`=`None`；建后 apparent(`string`)与 apparent(string-literal)均=wrapper，且 string-literal 上 `get_property_of_type("length")` 解析、其类型=`number`（Go `getApparentType` 的 `"abc".length` 路径）。`core/declared_types_test.rs:apparent_type_of_string_maps_to_global_string_wrapper`

**本轮交付（公开 API 仅做加法，compiler 保持绿）**：
- `core/program.rs`：`BoundProgram::globals()` 默认方法（加法）。
- `core/test_support.rs`：`StubProgram::globals()` override（返回 script 根文件 locals = 合成 globals）。
- `core/mod.rs`：`Checker::get_global_symbol`（pub，全局作用域 only 解析）+ `Checker::get_global_type(name)`（pub，读保留 program globals + delegate 自由函数 `get_global_type`）。
- `core/declared_types.rs`：`get_apparent_type` 的 `STRING_LIKE`→全局 `String` 映射（读 `global_types` 缓存；签名不变）+ DEFER 段更新。

**最小输入 → 可观察**：
- 全局符号：`declare var g: number;` → `get_global_symbol("g", VALUE)`=Some(`FUNCTION_SCOPED_VARIABLE`)；`("nope", VALUE)`/`("g", TYPE)`=None。
- 全局类型：`interface Foo {…}` → `get_global_type("Foo")`=Some(object，缓存)；`("foo")`/`("Missing")`=None。
- apparent wrapper：注入 `interface String { length: number }` 并 `get_global_type("String")` 后，`get_apparent_type(string)`=wrapper、`"abc"`.`length` 解析为 `number`。

**测试增量**：239 单测（+3：切片 1–3）+ 117 doctest（+2：`Checker::get_global_symbol`/`get_global_type` 各一条 `# Examples`）。相对 4y 基线 236+115。
`cargo test -p tsgo_checker` 绿（239 + 117）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**公开 API 仅做加法（compiler 保持绿）**：未改任何既有 `pub fn` 签名——`resolve_name`/`get_global_type`(自由函数)/`get_apparent_type`/`get_property_of_type`/`new_checker`/`program`/`check_source_file`/`get_diagnostics` 原样保留。新增 `BoundProgram::globals()` 为 trait 默认方法（既有实现不受影响）；`Checker::get_global_symbol`/`get_global_type` 为新 pub 方法；`get_apparent_type` 仅扩展函数体（签名不变）。`tsgo_compiler` 构建绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/` 中依赖全局符号/类型解析的最小子集——全局 `interface`/`declare var` 的名字解析、primitive 上的 wrapper 成员访问（`"x".length`）对应这些 baseline 的最基础形态。

**本轮 DEFER（带 blocked-by）**：
- **真 lib.d.ts 文件加载**（`String`/`Array`/`Object`/`Function`/`Symbol`/`Promise`/`JSX` 等 lib 全局的真实来源）：本轮全程用合成 globals 经 harness 驱动。blocked-by: P6-8 编译管线的 lib 文件加载（`compiler.Program` 装配 default-lib）。
- **跨文件 global merge + `globalThis` 符号**（Go `NewChecker` 合并各 global 文件 `Locals`、建 `globalThisSymbol`、`mergeGlobalSymbol`、ambient module/global augmentation）：本轮单文件 `StubProgram` 的根 locals 即 globals。blocked-by: 多文件 `BoundProgram` 暴露合并后的 globals（P6）。
- **`getGlobalType` 的 arity 检查 + 错误回退**（`Global_type_0_must_have_1_type_parameter_s`/`must_be_a_class_or_interface_type`/`Cannot_find_global_type_0`、`emptyObjectType`/`emptyGenericType` 回退）：需 empty-object/generic 类型构造 + 诊断接线。blocked-by: `emptyObjectType`/`emptyGenericType` 构造 + 诊断（reportErrors 路径）。
- **`new_checker` 时自动建全局 wrapper（`globalStringType` 等）+ `check_identifier`/property-access 端到端经全局 wrapper**：Go 在 `NewChecker` 调 `getGlobalType("String"/"Array"/…)` 预建；本轮 wrapper 须调用方先 `get_global_type` 填缓存，apparent-type 才命中。blocked-by: lib globals（P6）——真 `String` 来自 lib.d.ts，且 `NewChecker` 的全局类型初始化块依赖它。
- **其余 primitive wrapper**（`Number`/`Boolean`/`BigInt`/`Symbol` → apparent type）+ index/instantiable apparent 形 + base-constraint：本轮只做 `string`→`String`。blocked-by: 同上 lib globals + 各 wrapper 全局类型 + base-constraint 解析（4d+）。
- **`get_global_value_symbol`/`getGlobalTypeSymbol`/`resolveName` 的 nil-location 完整语义**（merged-symbol 规范化、suggestion 解析、`globalThis` 成员）：本轮 `get_global_symbol` 直接查 globals 表 + meaning 过滤。blocked-by: 完整 name resolver hook + merged-symbol/`globalThis` 基建。

**推荐下一轮（4aa）**：在多文件 `BoundProgram`（P6 前置）或 harness 扩展上接 **跨文件 global merge + `globalThis` 符号**，并把 `new_checker` 的**全局 wrapper 预建**（`globalStringType`/`globalArrayType`/…）+ `check_identifier`/property-access 端到端经全局 wrapper 打通（解锁 `"x".length` 经 `get_diagnostics` 的真实路径）；次选 `getGlobalType` 的 arity/错误回退（需先建 `emptyObjectType`/`emptyGenericType`）。这些均最终 blocked-by P6-8 的真 lib.d.ts 加载。

## 4aa 落地记录（worklog 摘要）—— 多文件 `BoundProgram` view（最大解锁项）

**目标**：把 checker 的 `BoundProgram` 抽象从**单文件**扩到**多文件**，使一个 SOURCE 文件能对 LIB 文件 + 兄弟文件装配出的 GLOBAL 表解析引用（`var s: string; s.length;` 在一个文件里、`String` 声明在 lib 文件里 → `length` 经合并 globals + apparent type 解析 → 无 2339）。这是会波及 compiler 的 API 变更（P6-6 适配），与 4l 改 API、P6-4 适配同构。本轮**不**做真 lib.d.ts 加载（仍 P6-8），多文件 program 经测试 harness 装配。

**所有权模型（命门）**：parser 每文件铸一个独立 arena（NodeId 从 0 起）+ 独立 binder 符号空间，故多文件**保留每文件独立 arena**，但**合并符号空间**：文件 `i` 的符号 id 偏移其前面文件的符号总数，所有带 SymbolId 的字段（`members`/`exports` 值、`parent`、`export_symbol`、`locals`/`node_symbol` 映射值、合并 `globals`）重写为合并 id；NodeId 保持文件本地，只经**所属文件的 view 的 arena** 访问。每文件一个 `FileView`（own `Rc` 句柄：自己的 arena + 程序级合并符号 vec + 合并 globals + 本文件的 encoded 映射 + 本文件 flow），实现 `BoundProgram`：`arena()`=本文件 arena、`symbol(id)`=合并 vec、`globals()`=合并表。checker 逐文件经 `source_files()` + `file_view(file)` 驱动；跨文件全局类型构建经 `view_for_symbol(symbol)` 用**声明该符号的文件的 view**（其 arena 拥有声明节点）。

**严格 TDD（逐行为 red→green，一次一个；每片 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

1. **切片 1（多文件 view + 合并 globals，tracer red→green）**：2 文件程序（A=`interface String { length: number }`，B=`declare const s: string;`）。RED：`MultiFileProgram` 未 override `source_files()` → 默认返回 1 个 → `assert_eq!(files.len(), 2)` 失败（观察 left=1 right=2）。GREEN：`source_files()` override 返回每文件 encoded 句柄。观察：两句柄互不相等；合并 `globals()` 同时含 `String`（lib）与 `s`（source）。`core/program_test.rs:multi_file_program_exposes_files_and_merged_globals`
2. **切片 2（跨文件 global 解析，tracer red→green）**：A=`interface String { length: number }`（lib），B=`declare const s: string;\ns.length;`（source）。RED：`check_source_file` 仍用保留的 `MultiFileProgram` 自身（`arena()`=文件0=lib），用 encoded 句柄索引 lib arena → `index out of bounds`（观察真红 panic）。GREEN：`check_source_file` 用 `file_view(file)` 取该文件 single-file view 走语句；`Checker::get_global_type` 用 `view_for_symbol` 在**声明文件**的 view 上建声明类型并**预热成员类型**（让跨文件属性访问命中缓存而非读错 arena）；`get_type_of_property_of_type` 入口 `ensure_primitive_apparent_wrapper` 对 string-like 惰性建全局 `String`。观察：经 `get_diagnostics(fileB)` 检 B，`s.length` 无 2339（无诊断）。负向控制：无 lib 的单文件 `s.length` → 2339。`core/check_test.rs:cross_file_global_resolves_string_property_via_lib` / `string_property_without_lib_reports_2339`
3. **切片 3（per-file 诊断过滤，tracer red→green）**：A=`var a: number = "x";`（2322），B=`var b: number = 1;`（无）。RED：诊断为单一平表，`get_diagnostics(fileB)` 含 A 的 2322（观察）。GREEN：诊断按 `program.file_handle()` 在**记录时**分桶（`error` 入 `diagnostics_by_file`），`get_diagnostics(file)` 只返回该文件桶。观察：`get_diagnostics(fileA)`=[2322]、`get_diagnostics(fileB)`=[]，互不含。`core/check_test.rs:get_diagnostics_is_filtered_per_file`

**新 `BoundProgram` trait 形（供 compiler P6-6 适配）—— 全部为带默认实现的加法方法，既有单文件实现（`StubProgram`/`compiler::BoundFile`）无需改即满足**：

```rust
// 既有方法不变：arena/root/symbol_of_node/symbol/locals/globals/flow_node_of/
//                 flow_node/flow_list/flow_switch_clause_data
fn file_handle(&self) -> NodeId { self.root() }                          // 诊断分桶键（默认 = root）
fn source_files(&self) -> Vec<NodeId> { vec![self.root()] }             // 每文件句柄（Go program.SourceFiles）
fn file_view(&self, file: NodeId) -> Option<Rc<dyn BoundProgram>> { None } // 单文件 view（None = 自身）
fn view_for_symbol(&self, symbol: SymbolId) -> Option<Rc<dyn BoundProgram>> { None } // 声明该符号的文件 view（None = 自身）
```

`globals()` 语义从「单文件根 locals」推广为「跨文件合并表」（多文件 program override 返回合并表；单文件实现不变）。`Checker::check_source_file(file)`/`get_diagnostics(file)`/`new_checker`/`program` 的**名字与签名不变**（churn 最小化）；内部：`check_source_file` 现经 `file_view` 取 view 检查，`get_diagnostics` 现按 `file` 句柄过滤桶；`get_global_type` 现经 `view_for_symbol` 在声明文件 view 上构建并预热成员类型。`Diagnostic` 形与 `get_diagnostics(file) -> &[Diagnostic]` 签名不变。

**tsgo_compiler 适配（P6-6，下一轮）**：本轮 API **仅做加法**，故 `internal/compiler/boundfile.rs::BoundFile`（单文件 `BoundProgram` 实现）**仍满足 trait、应仍可编译**——本轮按约束**未运行 `cargo build -p tsgo_compiler`**，需 P6-6 验证。P6-6 的实质是让 compiler **提供**一个真多文件 program view（而非修复破坏）：
- 新增一个 compiler 侧 `BoundProgram` 实现（类比 harness 的 `MultiFileProgram`）：装配多个已绑定 `ParsedFile`，合并符号空间（按文件偏移重映射 SymbolId）+ 合并 globals（各 global 文件根 `locals` 的并集，含 lib.d.ts），并实现 `source_files()`/`file_view()`/`view_for_symbol()`/`file_handle()`/`globals()`；
- `checkerpool` 改为用该多文件 program 的 `source_files()` 句柄派发、`get_diagnostics(handle)` 收集（句柄是 encoded、唯一），不再以单个 `BoundFile` seed 充当整个 program；
- 真 lib.d.ts 来源（`String`/`Array`/…）在 P6-8 装配到合并 globals 后，`ensure_primitive_apparent_wrapper` 的 `string`→`String` 即走真实路径。

**本轮交付**：
- `core/program.rs`：`BoundProgram` 新增 `file_handle`/`source_files`/`file_view`/`view_for_symbol`（均默认实现，加法）。
- `core/test_support.rs`：`MultiFileProgram::build(&[(name, text)])` + `FileView`（合并符号 vec / 合并 globals / per-file encoded 映射 / per-file arena+flow / encoded 文件句柄 `FILE_INDEX_SHIFT`）。
- `core/mod.rs`：`Checker::get_global_type` 经 `view_for_symbol` 在声明文件 view 构建 + 预热成员类型；`diagnostics` 平表换为 `diagnostics_by_file: FxHashMap<NodeId, Vec<Diagnostic>>`。
- `core/check.rs`：`check_source_file` 经 `file_view` 取 view；`error` 按 `file_handle` 分桶；`get_diagnostics` 按 `file` 过滤。
- `core/declared_types.rs`：`get_type_of_property_of_type` 入口 `ensure_primitive_apparent_wrapper`（string-like 惰性建全局 `String`，跨文件经声明文件 view）。

**本轮 DEFER（带 blocked-by）**：
- **跨文件同名声明的符号 MERGE（declaration merging）**：合并 globals 现「首文件优先」，不合并同名声明。blocked-by: binder 的 `mergeSymbol`/`mergeSymbolTable` 跨文件合并。
- **`globalThis` 符号**：未建。blocked-by: `globalThisSymbol` + 全局成员解析基建。
- **并行（`Arc`）**：保留 program 仍 `Rc<dyn>`，pool 顺序驱动；多 checker 真并行换 `Arc`。blocked-by: 并行 checker pool 落地。
- **真 lib.d.ts 加载**：多文件 view 现经 harness 装配合成 lib。blocked-by: P6-8 default-lib 装配。
- **其余 primitive wrapper（`Number`/`Boolean`/`BigInt`/`Symbol`）的 apparent 惰性建**：本轮只 `string`→`String`。blocked-by: 各 wrapper 全局类型 + lib globals（P6）。
- **`new_checker` 全局 wrapper 预建**：本轮改为属性访问路径**惰性**建 `String`（更贴 Go `getApparentType`），未在 `new_checker` 预建全套。blocked-by: lib globals（P6）。

**推荐下一轮（P6-6 compiler 适配）**：按上「tsgo_compiler 适配」让 compiler 提供真多文件 program view（`MultiFileBoundProgram` + `checkerpool` 改派发），并 `cargo build -p tsgo_compiler` / `cargo test --workspace` 收口；其后 P6-8 接真 lib.d.ts，跨文件 wrapper 走真实路径。

## 4ab 落地记录（worklog 摘要）—— `instanceof` / `in` 表达式检查（lib-global 解锁的表达式臂）

**目标**：现在 4z/4aa 给了 checker 全局解析 + 多文件合并 globals（P6-6 证明真 `lib.es5.d.ts` 可端到端解析），4o/4p 标 "blocked-by lib globals" 的 `instanceof`/`in` 二元运算符臂变得**可达**。本轮用**合成全局**（在程序顶层声明 `interface Function {…}`，是 Go `c.globalFunctionType` 的单文件 stand-in）驱动 `instanceof` 右操作数检查；`in` 的操作数检查只用 intrinsics（`stringNumberSymbolType` = `string|number|symbol`、`nonPrimitiveType` = `object`），不需真 lib。逐行为红→绿，一次一个。

**关键 Go ground-truth 确认**：
- `instanceof`（`checkInstanceOfExpression`/`resolveInstanceofExpression`）：结果恒 `boolean`；左操作数非 `any` 且 `allTypesAssignableToKind(left, Primitive)` → **2358**；右操作数非 `any` 且无 `[Symbol.hasInstance]` 方法 且 `!(typeHasCallOrConstructSignatures(right) || isTypeSubtypeOf(right, globalFunctionType))` → **2359**。
- `in`（`checkInExpression`）：结果恒 `boolean`；左 `checkTypeAssignableTo(left, stringNumberSymbolType, left, nil)`、右 `checkTypeAssignableTo(right, nonPrimitiveType, right, nil)`。**头消息为 `nil` → 走默认关系错误 = `2322`**（`Type_0_is_not_assignable_to_type_1`）。**TS-go 并不发 `2360`/`2361`**（交办单所述 2360/2361 是历史/旧 TS 码——实测 `internal/diagnostics` 中无此二码用于 `in`；故本轮 expected 取 Go 真实的 `2322`）。

**严格 TDD（逐行为 red→green，一次一个；每片 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

*Slice 组 I — `instanceof`（`core/check.rs`）*
1. **I1 结果 boolean（tracer）**：`declare const o: object; declare function f(): void; o instanceof f;` → `check_expression` 期望 `boolean` 得 `error_type` 红（落入 `_` 臂）→ 加 `InstanceOfKeyword` 臂 + `check_instanceof_expression` 最小 `=> boolean` → 绿。`check_test.rs:instanceof_expression_yields_boolean`
2. **I2 左 primitive → 2358**：`declare function f(): void; declare const s: string; s instanceof f;` 红（0 vs 1）→ 加左检查（`!is_any && all_types_assignable_to_kind(left, PRIMITIVE)` → 2358）+ `all_types_assignable_to_kind`（union 全员 / flag 命中）→ 绿（"The left-hand side of an 'instanceof' expression must be of type 'any', an object type or a type parameter."）。`instanceof_primitive_left_reports_diagnostic`
3. **I3 右非 Function/不可调用 → 2359（合成全局 Function）**：`interface Function { bind: number } interface O { x: number } declare const a: O; declare const b: O; a instanceof b;` 红（0 vs 1；左 O 对象不报 2358、右未检）→ 加右检查（`!is_any && !type_has_call_or_construct_signatures(right) && !is_type_subtype_of_global_function(right)` → 2359）+ helper `type_has_call_or_construct_signatures`（call 签名，construct DEFER）+ `is_type_subtype_of_global_function`（`get_global_type("Function")` → `is_type_subtype_of`）→ 绿（2359 全文）。**合成 `interface Function { bind: number }` 驱动**：`b: O` 缺 `bind` → 非 Function 子类型 → 2359。`instanceof_non_callable_right_reports_diagnostic`
4. **I4 守卫（无 2359）**：(a) 右为 Function 子类型：`interface Function{bind:number} declare const a:Function; declare const b:Function; a instanceof b;` → `b` 是 Function 子类型（恒等）→ 0 诊断（验证合成全局子类型路径）；(b) 右可调用：`interface O{x:number} declare const o:O; declare function f():void; o instanceof f;` → `f` 有 call 签名 → 0 诊断（验证 call-signature 分支，无需全局 Function）。`instanceof_function_subtype_right_reports_no_diagnostic` / `instanceof_callable_right_reports_no_diagnostic`

*Slice 组 N — `in`（`core/check.rs`）*
5. **N1 结果 boolean（tracer）**：`declare const k: string; declare const o: object; k in o;` → `check_expression` 期望 `boolean` 得 `error_type` 红 → 加 `InKeyword` 臂 + `check_in_expression` 最小 `=> boolean` → 绿。`in_expression_yields_boolean`
6. **N2 左非 string|number|symbol → 2322**：`interface O{x:number} declare const o:O; declare const r:object; o in r;` 红（0 vs 1）→ 加左检查（`get_union_type([string,number,esSymbol])` + `check_type_assignable_to_or_error`）→ 绿（"Type 'O' is not assignable to type 'string | number | symbol'."）。`in_expression_non_string_number_symbol_left_reports_diagnostic`
7. **N3 右非 object → 2322**：`declare const k: string; declare const s: string; k in s;` 红（N2 仅查左 → 0 vs 1）→ 加右检查（`is_type_assignable_to(right, non_primitive_type)` 否则 2322）→ 绿（"Type 'string' is not assignable to type 'object'."）。`in_expression_non_object_right_reports_diagnostic`
8. **N4 守卫（合法 → 无诊断）**：`declare const k: string; declare const o: object; k in o;` → 0 诊断。`in_expression_valid_operands_report_no_diagnostic`

**本轮交付（全部 `core/check.rs`，均私有 fn / 私有臂）**：
- `check_binary_expression`：新增 `InstanceOfKeyword`、`InKeyword` 臂（comma 仍 DEFER）。
- `check_instanceof_expression`（左 2358 + 右 2359 + 结果 boolean）、`all_types_assignable_to_kind`、`type_has_call_or_construct_signatures`、`is_type_subtype_of_global_function`（经 `get_global_type("Function")`）。
- `check_in_expression`（左→`string|number|symbol`、右→`object`、结果 boolean）、`check_type_assignable_to_or_error`（2322 + 字面量源广义化，复用 `generalized_source_for_error`）。

**最小输入 → 诊断/结果类型示例**：
- `instanceof`：`o instanceof f`→`boolean`；`s instanceof f`(s:string)→`2358`；`a instanceof b`(a/b:O, 合成 Function 缺 `bind`)→`2359`；右 Function 子类型 / 右可调用 → `[]`。
- `in`：`k in o`(string/object)→`boolean` 且 `[]`；`o in r`(o:O)→`2322` "Type 'O' is not assignable to type 'string | number | symbol'."；`k in s`(s:string)→`2322` "Type 'string' is not assignable to type 'object'."。

**测试增量**：252 单测（+9：I1–I4 共 5 个、N1–N4 共 4 个）+ 117 doctest（+0，**未新增 `pub fn`**）。相对 4aa 基线 243+117。
`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**公开 API 不变（compiler 保持绿）**：本轮**未新增/未改任何 `pub fn`**——新增物全为私有方法、私有 `check_binary_expression` 臂。`get_global_type`(4z pub)、`is_type_subtype_of`/`is_type_assignable_to`/`get_signatures_of_type`/`get_union_type`(既有 pub) 原样复用。`tsgo_compiler` 调用面不受影响。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/instanceofOperator/`、`expressions/inOperator/`（操作数种类检查 + 结果 boolean 最基础用例）——对应这些目录无 `Symbol.hasInstance`/私有标识符/索引推断/迭代器的基础用例。

**本轮 DEFER（带 blocked-by）**：
- `instanceof` 的 `[Symbol.hasInstance]` 方法路径（右操作数为带 `[Symbol.hasInstance]` 的对象 → 经 `getResolvedSignature` 当 `right[Symbol.hasInstance](left)` 调用检查 + 返回类型须可赋值于 `boolean`，否则 `2860`/相关诊断）+ 右操作数 construct 签名检测。blocked-by: `getResolvedSignature`/`getSymbolHasInstanceMethodOfObjectType` + 全局 `Symbol`(P6) + construct 签名收集。
- `instanceof` 的 `silentNeverType` 短路 + `CheckMode.SkipGenericFunctions`（返回 `silentNever`）+ 右操作数为 instantiation-expression 的 `2848`。blocked-by: `CheckMode` 参数化 + 实例化表达式 grammar。
- `in` 的私有标识符左操作数（`#x in obj` → `checkExternalEmitHelpers`/`reportNonexistentProperty`）+ 右操作数空对象交叉的 `2638`（`hasEmptyObjectIntersection`）+ `checkNonNullType` 的 strictNullChecks 语义（本轮无 strictNullChecks，恒等）。blocked-by: 私有标识符表达式 + `unknownEmptyObjectType`/`IsEmptyAnonymousObjectType` + `strictNullChecks` 接线。
- ~~**数组元素访问**~~ **4ac 已落地**（见下 §4ac worklog）。
- **for-of 元素类型化（`for (const x of arr)` 经迭代元素类型化 `x`）**：`check_statement` 的 for-of 臂现仅递归检查 body + 迭代表达式（4o），未对 LHS 做迭代元素类型化（`getIteratedTypeOrElementType` 需 `Symbol.iterator`/`IterableIterator`/`IteratorResult` 全局 + 迭代器协议）。blocked-by: 迭代器/可迭代协议全局（lib globals, P6）+ `getIteratedTypeOrElementType`。

**推荐下一轮（4ab，已被 4ac 部分取代）**：见 §4ac worklog。

## 4ac 落地记录（worklog 摘要）

**目标**：元素访问 + 索引签名解析（`obj[0]` / `obj[key]` 经 `[n:number]:T` / `[k:string]:T`；合成 `interface Array<T> { [n: number]: T }` 驱动数组 tracer）。

**red→green 垂直切片**：

| # | 测试 | 最小 input → observable | 实现触点 |
|---|---|---|---|
| 1 | `check_element_access_number_index_signature` | `Box { [n: number]: string }` + `b[0]` → `string` | `collect_index_infos_of_members` + `get_indexed_access_type` + `check_element_access` |
| 2 | `check_element_access_string_index_signature` | `Dict { [k: string]: number }` + `d["x"]` → `number` | `get_applicable_index_info` 字符串索引回落 |
| 3 | `check_element_access_string_index_with_variable_key` | `d[key]`（`key: string`）→ `number` | `is_applicable_index_type` + 计算索引类型 |
| 4 | `check_element_access_array_element_type` + `array_type_reference_index_signature_instantiates_element` | 合成 `Array<T>` + `Array<number>` + `a[0]` → `number` | `build_type_parameter_name_map` + `instantiate_index_infos` |

**测试增量**：257 unit + 119 doctest（相对 4ab 基线 252+117：**+5 unit / +2 doctest**）。

**additive pub**：`get_index_infos_of_type`、`get_indexed_access_type`（`lib.rs` re-export）；其余为私有 helpers / `check_element_access` 臂。

**gate（实测）**：`cargo test -p tsgo_checker` 绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- 元素访问失败诊断（`7053`/`2538`/`Element_implicitly_has_an_any_type`）：索引 miss 现静默 `error_type`。blocked-by: `getPropertyTypeForIndexType` 错误报告 + `noImplicitAny`/`noUncheckedIndexedAccess`。
- **`number[]` / `ArrayType` 语法**：数组 tracer 用 `Array<number>` 类型引用。blocked-by: `ArrayType` 节点 + lib `Array`（P6）。
- **for-of 循环变量元素类型**：blocked-by: `getIteratedTypeOrElementType` + iterator 协议（P6）。
- 多 applicable 索引签名 intersection、union 索引、tuple 数字索引、`noUncheckedIndexedAccess`（`undefined` 并入结果）。blocked-by: 4ad+ 实例化/关系深化。

**推荐下一轮（4ac）**：(a) 元素访问失败诊断 + `number[]`；(b) for-of 数组元素捷径（无完整 iterator）；(c) tuple 元素 / `noUncheckedIndexedAccess` 子集。

## 4ad 落地记录（worklog 摘要）—— `T[]` ArrayType 类型节点 + for-of 数组元素类型化

**目标**：续 4ac 的元素访问/索引签名工作，把数组语法 `T[]`（`ArrayTypeNode`）解析为全局 `Array<T>` 引用，并让 for-of 循环变量按数组元素类型化。全程用**合成全局** `interface Array<T> { [n: number]: T; length: number }`（程序顶层声明，Go `c.globalArrayType` 的单文件 stand-in）驱动，逐行为红→绿，一次一个。

**关键 Go ground-truth 确认**：
- `getTypeFromArrayOrTupleTypeNode`：一个 `ArrayTypeNode(elem)` → `getArrayType(elem)` → `createArrayType` → `createTypeReference(globalArrayType, [elem])`。本轮经 scope-chain 名字解析取全局 `Array` 符号（合成顶层 `interface Array<T>` stand-in），再 `create_type_reference(arrayTarget, [elem])`；无 `Array` 在 scope 时降级 `error_type`（lib 未加载）。
- `checkForOfStatement` 的元素类型化：for-of 循环变量类型 = `checkRightHandSideOfForOf` 的结果（对数组即元素类型）。本轮复用 4ac 的 `get_indexed_access_type(arrayType, number)` 取 `[n: number]` 元素（`Array<number>` → `number`）。
- `checkGrammarVariableDeclaration`：整个 `initializer == nil` 块（含「const 必须初始化」`1155`）在声明的 `parent.parent` **是** for-in/for-of 时**跳过**——4o 的 for-of 递归此前漏了这个门控，故 `for (const x of …)` 误报 1155。本轮补门控。

**严格 TDD（逐行为 red→green，一次一个；每片 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红） | 实现触点 |
|---|---|---|---|
| 1 | `check_element_access_number_array_element_type`（tracer，行为级经 `check_expression`） | 合成 `Array<T>` + `declare const a: number[]; a[0];` → `number`（红：ArrayType 落 `_` 臂得 `error_type`，`a[0]` → `error_type` ≠ `number`） | `get_type_from_type_node` 加 `ArrayType` 臂 + `get_type_from_array_type_node`（解析 elem → 经 `resolve_name("Array", TYPE)` 取全局 `Array` target → `create_type_reference(target, [elem])`） |
| 2a | `for_of_const_loop_variable_without_initializer_reports_no_grammar_error`（grammar 门控，经 `get_diagnostics`） | `for (const x of []) {}` → 无诊断（红：误报 `1155` "'const' declarations must be initialized."） | `check_grammar_variable_declaration` 在 `parent.parent` 为 `ForInStatement`/`ForOfStatement` 时 early-return（门控 `initializer==nil` 块） |
| 2b | `for_of_loop_variable_is_typed_as_array_element`（元素类型化，经 `get_diagnostics`） | 合成 `Array<T>` + `declare const a: number[]; for (const x of a) { const y: string = x; }` → 1 诊断 `2322` "Type 'number' is not assignable to type 'string'."（红：`x` 未注解 = `any` → 无诊断） | for-of 臂先 `check_expression(rhs)` 取类型，`assign_for_of_element_types`（对 VariableDeclarationList 的 un-annotated identifier 变量缓存 `value_symbol_links.resolved_type` = 元素类型）+ `get_iterated_type_or_element_type`（数组捷径 = `get_indexed_access_type(input, number)`） |
| 2b 守卫 | `for_of_loop_variable_element_type_assignable_to_matching_target` | 同上但 body `const y: number = x;` → 无诊断（证明 `x` 真为 `number`，非 blanket 报错） | 同 2b（无新触点） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅做加法/体扩展）**：
- `core/declared_types.rs`：`get_type_from_type_node` 新增 `Kind::ArrayType` 臂 + 私有 `get_type_from_array_type_node`。
- `core/grammar.rs`：`check_grammar_variable_declaration` 加 for-in/of `parent.parent` 门控（体扩展，签名不变）。
- `core/check.rs`：for-of 臂先算 rhs 类型并对 for-of 类型化循环变量；新增私有 `assign_for_of_element_types` + `get_iterated_type_or_element_type`（数组捷径）。

**最小输入 → 可观察**：
- ArrayType：`number[]` → `Array<number>` 引用；`a[0]` → `number`。
- for-of grammar：`for (const x of []) {}` → 无 `1155`。
- for-of 元素类型化：`for (const x of a)`（`a: number[]`）→ `x: number`；body `const y: string = x` → `2322`；`const y: number = x` → 无诊断。

**测试增量**：261 单测（+4：切片 1 / 2a / 2b / 2b 守卫）+ 119 doctest（+0，**未新增 `pub fn`**）。相对 4ac 基线 257+119。

**公开 API 仅做加法/体扩展（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`get_type_from_type_node`/`get_indexed_access_type`/`check_grammar_variable_declaration`/`check_expression` 原样保留；新增物全为私有 fn / 新 match 臂 / 既有体扩展。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（261 单测 + 119 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeRelationships/` 与 `es6/for-ofStatements/`（`T[]` 数组语法解析 + for-of 数组元素类型化的最基础形态）——无 tuple/`ReadonlyArray`/字符串迭代/生成器/异步迭代器的子集。

**本轮 DEFER（带 blocked-by）**：
- **tuple 类型节点 `[A, B]`** 与 **`ReadonlyArray` / `readonly T[]`**：本轮只解析 `T[]` → `Array<T>`。blocked-by: `globalReadonlyArrayType` + tuple 类型构造。
- **完整 `Symbol.iterator` / iterator-protocol**（非数组可迭代物 `IterableIterator`/`IteratorResult`、字符串迭代、生成器、异步可迭代物）：本轮 for-of 仅走数组捷径（number 索引元素），非数组 rhs → 元素类型 None → 变量保持 `any`。blocked-by: 迭代器/可迭代协议全局（lib.d.ts, P6）。
- **`getIteratedTypeOrElementType` 的完整 union 处理**（rhs 为 union 时按成员分配元素类型再并）：本轮单一数组类型。blocked-by: union 元素类型分配。
- **for-in 变量类型化**（`string`/index 类型）+ for-of 解构（binding-pattern）循环变量：本轮只 for-of 的 identifier 变量。blocked-by: `getIndexTypeOrString` + binding-element 类型化。
- **元素访问/索引 miss 失败诊断**（`7053`/`2538`/`Element_implicitly_has_an_any_type`）+ `noUncheckedIndexedAccess`（`undefined` 并入）：续 4ac DEFER。blocked-by: `getPropertyTypeForIndexType` 错误报告 + `noImplicitAny`/`noUncheckedIndexedAccess`。

**推荐下一轮（4ae）**：(a) tuple 类型节点 `[A, B]` + `ReadonlyArray`/`readonly T[]`（需 `globalReadonlyArrayType` + tuple 构造）；(b) `getIteratedTypeOrElementType` 的 union 分配 + for-in 变量 `string` 类型化；(c) 元素访问失败诊断（`7053`/`2538`），最终 blocked-by P6 真 lib.d.ts。

## 4ae 落地记录（worklog 摘要）—— 元组类型节点 `[A, B]`（定长子集）+ `ReadonlyArray` / `readonly T[]`

**目标**：续 4ad 的 `T[]` ArrayType 工作，把 (a) 元组类型节点 `[A, B]` 解析为定长元组类型（元素类型按位置，`t[0]` → 首元素类型），以及 (b) `readonly T[]`（`readonly` 类型操作符覆盖数组）与 `ReadonlyArray<T>` 引用解析为全局 `ReadonlyArray<T>` 引用。全程用**合成全局** `interface Array<T>{...}` / `interface ReadonlyArray<T>{ readonly [n:number]:T; readonly length:number }`（程序顶层声明，Go `c.globalArrayType`/`c.globalReadonlyArrayType` 的单文件 stand-in）驱动，逐行为红→绿，一次一个。

**关键 Go ground-truth 确认**：
- `getTypeFromArrayOrTupleTypeNode`：`node.Kind == TupleType` 时元素类型 = `core.Map(node.Elements(), getTypeFromTypeNode)`，再据 `target.objectFlags&Tuple` 走 `createNormalizedTupleTypeEx`。元组本质是「以元素类型为类型实参、指向生成/全局元组 target 的类型引用」。**定长子集**直接把映射后的元素类型存进一个 `ObjectFlags::TUPLE` 对象类型的 `resolved_type_arguments`（按位置），足以支撑字面量索引的元素访问；完整生成 target（`TupleElementInfo`/`length`/`[number]` 成员）DEFER。
- `getArrayOrTupleTargetType`：`readonly := isReadonlyTypeOperator(node.Parent)`；数组形（`getArrayElementTypeNode != nil`）下 readonly→`globalReadonlyArrayType`，否则 `globalArrayType`。即**数组节点据其父节点是否为 `readonly` 操作符**来选全局 target。
- `getTypeFromTypeOperatorNode`：`KindReadonlyKeyword` → `getTypeFromTypeNode(argType)`（对操作数透传；readonly 的语义落在子数组节点选 target 上，而非在操作符这层包一层）。
- `ReadonlyArray<T>` 引用形：纯泛型类型引用，走既有 `getTypeFromTypeReference`（4v），与 `Array<T>` 同路径——**无需新构造代码**。

**严格 TDD（逐行为 red→green，一次一个；每片 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红） | 实现触点 |
|---|---|---|---|
| 1 | `check_element_access_tuple_first_element_type`（tracer，行为级经 `check_expression`） | `declare const t: [string, number]; t[0];` → `string`（红：`TupleType` 落 `_` 臂 → `t` 类型=`error_type`，`t[0]`=`error_type` TypeId(3) ≠ string） | `get_type_from_type_node` 加 `TupleType` 臂 + 私有 `get_type_from_tuple_type_node`（映射元素 → `Checker::create_tuple_type`）；`get_indexed_access_type` 加私有 `get_tuple_element_by_literal_index`（`TUPLE` 标志 + 非负整数字面量索引 → 按位置取 `resolved_type_arguments`） |
| 1 守卫 | `check_element_access_tuple_second_element_type` | `t[1]` → `number`（证明按位置取，非 blanket 首元素） | 同切片1（无新触点） |
| 2 | `check_element_access_readonly_array_element_type`（tracer，行为级经 `check_expression`） | 合成 `ReadonlyArray<T>` + `declare const r: readonly string[]; r[0];` → `string`（红：`TypeOperator` 落 `_` 臂 → `r` 类型=`error_type`，`r[0]`=`error_type` TypeId(3) ≠ string） | `get_type_from_type_node` 加 `TypeOperator` 臂 + 私有 `get_type_from_type_operator_node`（`readonly` → 透传操作数）；`get_type_from_array_type_node` 据父节点是否 `readonly` 操作符（私有 `is_readonly_type_operator_parent`）选 `ReadonlyArray` vs `Array` 全局名 |
| 3 | `check_element_access_readonly_array_type_reference_element_type`（确认；既有机制即绿，非新 RED） | 合成 `ReadonlyArray<T>` + `declare const r: ReadonlyArray<string>; r[0];` → `string` | 无（复用 4v `get_type_from_type_reference`） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅做加法）**：
- `core/declared_types.rs`：`get_type_from_type_node` 新增 `Kind::TupleType` / `Kind::TypeOperator` 两个 match 臂；新增私有 fn `get_type_from_tuple_type_node` / `get_type_from_type_operator_node` / `is_readonly_type_operator_parent` / `get_tuple_element_by_literal_index`；`get_type_from_array_type_node` 体扩展（据父 `readonly` 操作符选全局 target 名）。
- `core/mod.rs`：新增 pub fn `Checker::create_tuple_type`（Go `createTupleType` 的定长子集；加法新增，含 §8.6 doctest——故 doctest +1）。

**最小输入 → 可观察**：
- 元组定长：`[string, number]` → `TUPLE` 对象（`resolved_type_arguments=[string, number]`）；`t[0]`→`string`、`t[1]`→`number`。
- `readonly T[]`：`readonly string[]` → `ReadonlyArray<string>` 引用；`r[0]`→`string`。
- `ReadonlyArray<T>` 引用形：`ReadonlyArray<string>` → 同引用；`r[0]`→`string`（既有 TypeReference 机制）。

**测试增量**：265 单测（+4：切片1 / 切片1守卫 / 切片2 / 切片3 确认）+ 120 doctest（+1：`create_tuple_type` 的 §8.6 doctest）。相对 4ad 基线 261+119。

**公开 API 仅做加法（compiler 保持绿）**：未改任何既有 `pub fn` 签名——`get_type_from_type_node`/`get_indexed_access_type`/`create_type_reference` 等原样保留；新增物为私有 fn / 新 match 臂 / 既有体扩展，外加一个**新**的 pub fn `Checker::create_tuple_type`（纯加法，不破坏既有签名）。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（265 单测 + 120 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/tuple/`（定长元组语法 + 字面量索引元素访问的最基础形态）与 `types/typeRelationships/`（`readonly T[]`/`ReadonlyArray<T>` 解析的最基础形态）——无变长/可选/具名元组、tuple→数组可赋值性、`as const` 的子集。

**本轮 DEFER（带 blocked-by）**：
- **变长元组 `[...T[]]` / 可选 `[a?, b]` / 具名 `[x: string]` / rest 元组元素**：本轮仅定长（fixed-arity）。blocked-by: `createNormalizedTupleType` + `getTupleTargetType` + 元组元素 flags（`getTupleElementInfo`/`ElementFlags`）+ 迭代器/spread 机制。
- **完整生成元组 target**（`TupleElementInfo`、`length` 成员、`[number]` 索引签名、元组 → 数组/`ReadonlyArray` 可赋值性）：本轮元组无 `length`/数字索引签名，仅支持字面量索引按位置取元素。blocked-by: 生成 target 构造 + 关系引擎对元组的处理。
- **非字面量 `number` 索引的元组元素访问**（`t[i]`，`i: number` → 全元素并集）+ 越界/负索引诊断：本轮仅非负整数字面量、在界内。blocked-by: 元组元素并集类型 + 元素访问失败诊断（`7053`/越界）。
- **`as const`（const 断言）元组、`keyof`/`unique symbol` 类型操作符**：`get_type_from_type_operator_node` 仅实现 `readonly` 透传臂。blocked-by: const-assertion 元组冻结 + `getIndexType`（`keyof`）+ unique-ES-symbol 类型化。

**推荐下一轮（4af）**：(a) 元组 `length`（`2`）成员 + 非字面量 `number` 索引（元素并集）；(b) `getIteratedTypeOrElementType` 的 union 分配（4ad DEFER 续）+ for-in 变量 `string` 类型化；(c) 元素访问失败诊断（`7053`/`2538`），最终 blocked-by P6 真 lib.d.ts。

## 4af 落地记录（worklog 摘要）—— 元素访问失败诊断 `2538` + for-in 变量 `string` 类型化 + 元组 `length`/非字面量数字索引

**目标**：续 4ac/4ad/4ae 的元素访问 + 数组/元组工作，落地三个互相独立的 red→green 切片：(1) 用非 string/number/symbol-like 键索引（且无适用索引签名）→ `2538`；(2) for-in 循环变量类型化为 `string`（4ad for-in carryover）；(3) 定长元组 `.length`（数字字面量 arity）+ 非字面量 `number` 索引（元素并集）。全程逐行为 red→green，一次一个，每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿。

**关键 Go ground-truth 确认**：
- `getPropertyTypeForIndexType` 尾部 `2538` 臂：当索引既非 string/number 字面量名、又非 string/number（且非 bigint 字面量）时，落到末尾 `c.error(indexNode, diagnostics.Type_0_cannot_be_used_as_an_index_type, typeString)`。`boolean` 键不满足 `isTypeAssignableToKind(indexType, StringLike|NumberLike|ESSymbolLike)`，故从不进入索引签名块，直接落到该 `2538`。本轮 reachable 子集：`get_indexed_access_type` 返回 `None` 且索引类型不交 `STRING_LIKE|NUMBER_LIKE|ES_SYMBOL_LIKE|ANY|NEVER` → `2538`（报在 argument 节点）。
- `getTypeForVariableLikeDeclaration`（`grandParent.Kind == KindForInStatement`）：for-in 变量声明类型 = `c.stringType`（reachable 子集；当迭代表达式为类型参数/index 类型时为 `keyof T`，DEFER）。
- `createTupleTargetType` 的 `length` 成员：定长元组（无 variable 元素）的 `length` = `getUnionType(minLength..=arity 的数字字面量)`，对定长 `[A, B]` 即单一数字字面量 `2`。
- 元组 `[number]` 索引：非字面量 `number` 索引分布到全部元素 → 元素类型并集（`[string, number][i]`，`i: number` → `string | number`）。

**严格 TDD（逐行为 red→green，一次一个；每片 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红） | 实现触点 |
|---|---|---|---|
| 1 | `check_element_access_boolean_index_reports_2538`（tracer，经 `get_diagnostics`） | `interface O{a:number} declare const o:O; declare const k:boolean; o[k];` → 1× `2538`（红：miss 静默 `error_type` → 0 诊断） | `check_element_access`：`get_indexed_access_type`=None 且索引不交 `STRING_LIKE\|NUMBER_LIKE\|ES_SYMBOL_LIKE\|ANY\|NEVER` 时报 `2538`（报在 arg 节点） |
| 2 | `for_in_loop_variable_is_typed_as_string`（经 `get_diagnostics`） | `for (const k in {}) { const n: number = k; }` → 1× `2322`（红：`k` 未类型化=`any` → 无诊断） | for-in 臂新增 `assign_for_in_variable_types`（对 VariableDeclarationList 的 un-annotated identifier 变量缓存 `value_symbol_links.resolved_type` = `string`） |
| 2 守卫 | `for_in_loop_variable_string_assignable_to_matching_target` | 同上但 body `const s: string = k;` → 无诊断（证明 `k` 真为 `string`） | 同切片2（无新触点） |
| 3a | `tuple_length_resolves_to_numeric_literal_arity`（经 `check_expression`） | `declare const t: [string, number]; t.length;` → `type_to_string`="2"（红：元组无 `length` 成员 → 属性访问 miss → `error_type`="error"） | `get_type_of_property_of_type` 入口：`name=="length"` 且 `TUPLE` 标志 → 私有 `get_tuple_length_type`（`new_literal_type(NUMBER_LITERAL, arity)`） |
| 3b | `check_element_access_tuple_non_literal_number_index_yields_element_union`（经 `check_expression`） | `declare const t: [string, number]; declare const i: number; t[i];` → `string\|number`（红：`get_applicable_index_info(tuple, number)`=None → `error_type` TypeId(3)） | `get_indexed_access_type`：字面量索引臂之后新增私有 `get_tuple_number_index_type`（`TUPLE` 标志 + 索引含 `NUMBER` 标志 → `get_union_type(resolved_type_arguments)`） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅做加法/体扩展）**：
- `core/check.rs`：`check_element_access` 体扩展（miss 时按 Go-faithful 条件报 `2538`）；for-in 臂新增 `assign_for_in_variable_types` 调用 + 私有 fn `assign_for_in_variable_types`。
- `core/declared_types.rs`：`get_type_of_property_of_type` 入口 `length`/`TUPLE` 特例（私有 `get_tuple_length_type`）；`get_indexed_access_type` 新增 tuple 非字面量数字索引臂（私有 `get_tuple_number_index_type`）。

**最小输入 → 可观察**：
- 2538：`o[k]`（k:boolean）→ `2538` "Type 'false | true' cannot be used as an index type."（boolean 打印为 `false | true`，其→`boolean` 折叠 DEFER 至 4j；2538 码为受测行为）。
- for-in string：`for (const k in {})` → `k: string`；body `const n: number = k` → `2322`；`const s: string = k` → 无诊断。
- 元组 length：`[string, number].length` → 数字字面量 `2`（打印 "2"）。
- 元组数字索引：`t[i]`（i:number）→ `string | number`；`t[0]`/`t[1]` 字面量索引仍按位置取 `string`/`number`（4ae 路径不变）。

**测试增量**：270 单测（+5：切片1 / 2 / 2 守卫 / 3a / 3b）+ 120 doctest（+0，**未新增 `pub fn`**）。相对 4ae 基线 265+120。

**公开 API 仅做加法/体扩展（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`check_element_access`(私有)、`get_type_of_property_of_type`/`get_indexed_access_type`(既有 pub) 原样保留签名；新增物全为私有 fn / 新 match 臂 / 既有体扩展。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（270 单测 + 120 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeRelationships/`（非法索引键 `2538`）、`es6/forStatements/`（for-in 变量 `string`）、`types/tuple/`（元组 `length` + 数字索引元素并集）的最基础形态。

**本轮 DEFER（带 blocked-by）**：
- **`7053`（隐式 any 元素访问）/ `noImplicitAny`**：本轮只报 `2538`（非 string/number/symbol-like 键），object-literal 上的隐式 any 元素访问仍静默。blocked-by: `noImplicitAny` 选项接线 + `getSuggestionForNonexistent*` 路径。
- **symbol-keyed 元素访问**（`o[sym]`，sym: symbol/unique symbol → string 索引回落）：本轮 `2538` 条件排除 `ES_SYMBOL_LIKE`，故 symbol 键不报 `2538` 但也未走 string 索引回落。blocked-by: 全局 `Symbol`（P6）+ string-index 回落 + `getPropertyNameFromIndex` 的 unique-symbol 名。
- **`noUncheckedIndexedAccess`**（元组数字索引/索引签名结果并入 `undefined`）：本轮不并 `undefined`。blocked-by: 选项接线 + `missingType`。
- **变长/可选/rest 元组 `length`**（`number` 或多字面量长度并集）+ 完整生成元组 target（`length` 真成员、`[number]` 索引签名、越界/负索引诊断）：本轮 `length` 仅定长单字面量、数字索引仅经合成并集。blocked-by: `createNormalizedTupleType` + 元组元素 flags + 元素访问越界诊断。
- **for-in `keyof T` 变量类型**（迭代表达式为类型参数/index 类型时）+ for-in 解构（binding-pattern）循环变量 + for-in LHS/RHS 诊断（`2405`/`2407`）：本轮 for-in 变量恒 `string`、仅 identifier 变量。blocked-by: `getIndexType`（`keyof`）+ binding-element 类型化 + for-in 操作数诊断。

**推荐下一轮（4ag）**：(a) `getIteratedTypeOrElementType` 的 union 分配（4ad DEFER 续，rhs 为 union 时按成员分配元素类型再并）；(b) for-in/for-of LHS/RHS 操作数诊断（`2405`/`2407`/`2461`）；(c) symbol-keyed 元素访问的 string-index 回落 + `noImplicitAny` 的 `7053`，最终 blocked-by P6 真 lib.d.ts。

## 4ag 落地记录（worklog 摘要）—— well-known symbol late-binding（`[Symbol.iterator]` → `__@iterator`）

**目标**：binder 修复轮刚落地（`[Symbol.x]` 计算名现按 Go `bindPropertyOrMethodOrAccessor` 的 `HasDynamicName` 守卫匿名绑为 `__computed`，不再 panic、不进 `I.members`）。本轮是其 **checker 侧补全**：把这类 `__computed` 成员 **late-bind** 到其内部晚绑名 `__@<name>`（如 `__@iterator`/`__@asyncIterator`），使之按 well-known symbol 名可达。逐行为 red→green，一次一个，每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。

**关键 Go ground-truth 确认**：
- `getPropertyNameForKnownSymbolName(symbolName)`（`flow.go`）：先查全局 `Symbol` 构造器的 `unique symbol` 属性（`getTypeOfPropertyOfType`+`getPropertyNameFromType`），命中则返回该 unique 名（形如 `__@iterator@<id>`）；否则 fallback `ast.InternalSymbolNamePrefix + "@" + symbolName`（即 `"__@" + name`）。
- 真正的 late-binding 在 `getResolvedMembersOrExportsOfSymbol`：遍历符号 `Declarations` 的 `getMembersOfDeclaration` 成员节点，对 `hasLateBindableName` 的成员调 `lateBindMember`；后者用 `getPropertyNameFromType(checkComputedPropertyName(declName))` 作晚绑名，新建 `CheckFlagsLate` 符号，最后 `combineSymbolTables(early, late)`。
- `isLateBindableAST`/`isSymbolOrSymbolForCall`：计算名表达式须是 entity-name；`Symbol` 须是**全局** `Symbol` 值符号（`getGlobalESSymbolConstructorSymbolOrNil() == resolveName(left,"Symbol",Value)`）。

**reachable 子集 / 与 Go 的偏离（已据 ground-truth 校正）**：本仓库 Rust 端**尚无 unique-ES-symbol 类型构造**（`getESSymbolLikeTypeForNode`，DEFER），故 `checkComputedPropertyName([Symbol.iterator])` 不会产出 unique-symbol 名。本轮走 Go 的 **fallback 路径**：晚绑名统一用 `getPropertyNameForKnownSymbolName` 的 fallback 形态 `INTERNAL_SYMBOL_NAME_PREFIX + "@" + name`（escape 后即 Go 字面量 `"__@iterator"`；注意 Rust 前缀是 `U+00FE` 而非 `"__"`，见 ast `INTERNAL_SYMBOL_NAME_PREFIX` 的 DIVERGENCE）。又因 binder 把 `__computed` 成员匿名绑（不进父 `members`、仅挂在成员节点上），本轮 late-binding **扫描接口/类声明的 AST 成员节点**，对计算名 `Symbol.<name>` 者用 `program.symbol_of_node(member)` 取回那枚匿名 `__computed` 符号，按晚绑名加入类型成员表（复用该符号，未新建 `CheckFlagsLate` 符号）。

**严格 TDD（逐行为 red→green，一次一个）**：

| # | 切片 | 最小 input → observable（实测红） | 实现触点 |
|---|---|---|---|
| 1 | `property_name_for_known_symbol_name_uses_at_prefixed_internal_name`（`flow_test.rs`，纯 helper） | `c.get_property_name_for_known_symbol_name("iterator")` == `"\u{FE}@iterator"`、escape 后 == `"__@iterator"`（红：方法不存在=编译错） | `flow.rs` 新增 pub fn `Checker::get_property_name_for_known_symbol_name`（fallback `PREFIX + "@" + name`） |
| 2 | `late_binds_well_known_symbol_iterator_member`（`declared_types_test.rs`，经 `get_property_of_type`） | `interface SymbolConstructor{readonly iterator: unique symbol} declare var Symbol: SymbolConstructor; interface I{ [Symbol.iterator](): void }` → `get_property_of_type(I, "__@iterator")`=Some（method 声明）；`"iterator"` 字面名仍=None（红：`__@iterator` 名 miss=None） | `declared_types.rs`：`get_declared_type_of_class_or_interface` 新增 `collect_late_bound_well_known_members`（扫成员 AST、`well_known_symbol_name`、`member_name_node`），加入 `obj.members` |
| 3 | `computed_symbol_member_without_global_symbol_is_not_late_bound`（同上） | `interface I{ [Symbol.iterator](): void }`（**无** `declare var Symbol`）→ `get_property_of_type(I,"__@iterator")`=None（红：切片2 纯语法实现会误绑=Some(SymbolId(1))） | `well_known_symbol_name` 加全局 `Symbol` 身份守卫（`globals["Symbol"]` 为 VALUE 且 `resolve_name(...)==` 之，Go `isSymbolOrSymbolForCall`） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅做加法）**：
- `core/flow.rs`：新增 **pub fn** `Checker::get_property_name_for_known_symbol_name`（纯加法，Go 锚 `flow.go:getPropertyNameForKnownSymbolName`，含 unique-symbol 全局路径 DEFER）。
- `core/declared_types.rs`：`get_declared_type_of_class_or_interface` 体扩展（base-merge 后、index-sig 前插入 late-bind）；新增私有 fn `collect_late_bound_well_known_members` / `member_name_node` / `well_known_symbol_name`。

**最小输入 → 可观察**：`interface I { [Symbol.iterator](): void }` + 合成全局 `Symbol` → `get_property_of_type(I, get_property_name_for_known_symbol_name("iterator"))` = Some（其声明为 `MethodSignature`）；去掉全局 `Symbol` → None。合成全局 `interface SymbolConstructor { readonly iterator: unique symbol }` + `declare var Symbol: SymbolConstructor` 即 well-known-symbol 来源（驱动全局身份守卫）。

**测试增量**：273 单测（+3：切片1 helper / 切片2 late-bind / 切片3 全局守卫）+ 121 doctest（+1：`get_property_name_for_known_symbol_name` 的 §8.6 doctest）。相对 4af 基线 270+120。

**公开 API 仅做加法（compiler 保持绿）**：未改任何既有 `pub fn` 签名——`get_declared_type_of_symbol`/`get_property_of_type` 等原样保留；新增物为私有 fn / 既有体扩展，外加一个**新** pub fn `Checker::get_property_name_for_known_symbol_name`（纯加法）。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（273 单测 + 121 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/es6/Symbols/`、`es6/computedProperties/`、`externalModules/`（well-known symbol 计算成员的最基础 late-binding 形态）；fourslash `codeFixClassImplementInterfaceComputedPropertyNameWellKnownSymbols` 是其端到端目标之一。

**本轮 DEFER（带 blocked-by）**：
- **完整迭代器协议**（`for-of` over `[Symbol.iterator]`-bearing 对象、`getIterationTypesOfIterable`/`getIteratedTypeOfIterable`）：本轮只落地 late-bind 成员可达，未接迭代器协议消费端。blocked-by: `getIterationTypesOfIterable` + 调用签名解析 + `IterationTypes`。
- **unique-ES-symbol 类型构造**（`getESSymbolLikeTypeForNode`/`newUniqueESSymbolType`）：本轮 `get_property_name_for_known_symbol_name` 只走 fallback `"__@"+name`，未走全局 `Symbol` 的 unique-symbol 路径（其晚绑名带 `@<id>` 后缀）。blocked-by: unique-ES-symbol 类型化 + `getTypeOfPropertyOfType` 的 unique 命中。
- **`Symbol.x`-keyed 元素访问**（`obj[Symbol.iterator]`）+ `ElementAccessExpression` 计算名的 late-binding（`isLateBindableAST` 的 element-access 臂）：本轮只处理 `ComputedPropertyName` 成员名。blocked-by: 元素访问 late-bind 路径 + unique-symbol 名。
- **新建 `CheckFlagsLate` 晚绑符号 + 冲突诊断 + accessor 合并 + static/instance 拆分 + 索引签名 late-binding**：本轮复用 binder 的 `__computed` 符号、单表插入，未做 Go `lateBindMember` 的完整语义（`Duplicate_identifier_0`、`getExportsOfSymbol` 静态侧、`lateBindIndexSignature`）。blocked-by: 晚绑符号 arena + 诊断接线。

**推荐下一轮（4ah）**：(a) `getIteratedTypeOfIterable`：`for-of` 经 `__@iterator` 成员的调用签名解析出元素类型（接 4ad/4af 的数组 for-of）；(b) unique-ES-symbol 类型构造（`getESSymbolLikeTypeForNode`），使晚绑名走 Go 的 `@<id>` 形态并与 `Symbol.x` 类型一致；(c) `obj[Symbol.iterator]` 元素访问 late-binding，最终 blocked-by P6 真 lib.d.ts。

## 4ah 落地记录（worklog 摘要）—— for-of over a `[Symbol.iterator]`-bearing object（iterator-protocol 元素类型化）

**目标**：续 4ag 的 well-known symbol late-binding（`[Symbol.iterator]` → `__@iterator`），把 for-of 循环变量按**迭代器协议**类型化（接 4ad/4af 的数组 for-of：数组捷径保留，可迭代物路径为一般情形）。全程用**合成全局** `interface Iterator<T> { next(): { value: T } }` + `interface It { [Symbol.iterator](): Iterator<string> }` + 4ag 的 `SymbolConstructor`/`declare var Symbol`（程序顶层声明，Go lib 迭代器协议类型的单文件 stand-in）驱动，逐行为红→绿，一次一个，每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿。

**关键 Go ground-truth 确认**：
- `getIteratedTypeOfIterable` → `getIterationTypesOfIterable` → `getIterationTypesOfIterableWorker`：取 `[Symbol.iterator]()` 方法 → 其调用签名返回类型（iterator）→ 该 iterator 的 `next()` 调用签名返回类型（`{ value, done }`）→ `value` 属性类型即元素类型。
- `getTypeOfSymbol`（METHOD）→ `getTypeOfFuncClassEnumModule`：method 符号的类型是携带其调用签名的匿名对象类型；`getSignaturesOfSymbol`/`getSignatureFromDeclaration` 对 `MethodSignature`/`MethodDeclaration` 收集参数 + 返回类型。
- `getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode`：`{ value: T }` 类型字面量 → 携带其成员的匿名对象类型（成员类型懒解析）。
- `resolveName`（type-parameter meaning）：泛型接口/类/方法的类型参数在 Go 里绑进声明的 `locals`，故 `value: T` 的 `T` 经 `resolveName` 命中类型参数。**本仓库 binder 不把类型参数放进 `locals`**（既有索引签名路径用 `build_type_parameter_name_map` 绕过），故本轮在 checker 侧补一个 `resolve_type_parameter_in_scope` fallback（沿父链扫泛型容器的 `<...>` 列表按名匹配）。

**reachable 子集 / 与 Go 的偏离（DIVERGENCE）**：不实现匿名对象深实例化（4e DEFER）。`getTypeOfPropertyOfType(Iterator<string>, "next")` 会实例化整个匿名 `next` 函数类型（被 DEFER → 原样返回），故无法靠它把 `{value:T}` 实例化成 `{value:string}`。本轮改为：取（未实例化的）`next()` 结果的 `value` 属性类型（= 类型参数 `T`），再经**迭代器引用自身**的 `type parameters -> type arguments` mapper 实例化（`Iterator<string>` → `{T:string}`，对裸类型参数 `instantiate_type` 直接命中）→ 元素类型 `string`。元素类型与 Go 完全一致。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红） | 实现触点 |
|---|---|---|---|
| 1 | `check_property_access_type_literal_member`（tracer，经 `check_expression`） | `declare const o: { value: string }; o.value;` → `string`（红：`TypeLiteral` 落 `_` 臂 → `o`=`error_type`，`o.value`=TypeId(3)≠string TypeId(7)） | `get_type_from_type_node` 加 `Kind::TypeLiteral` 臂 + 私有 `get_type_from_type_literal_node`（取类型字面量 `__type` 符号的 members → 匿名对象类型） |
| 2 | `method_member_call_signature_return_type`（§8.6，经 `get_signatures_of_type`/`get_return_type_of_call`） | `interface I { m(): string }` → `typeof m` 的 call signatures.len()=1、返回类型=`string`（红：METHOD 落 `get_type_of_symbol` 未处理臂 → `error_type`，0 签名） | `get_type_of_symbol`：`SymbolFlags::FUNCTION` 臂改含 `\| METHOD`；`get_signatures_of_symbol`/`get_signature_from_declaration` 加 `MethodSignature`/`MethodDeclaration` 臂（参数 + 返回类型节点） |
| 2.5 | `bare_type_parameter_reference_resolves_to_enclosing_type_parameter`（经 `get_type_of_symbol`） | `interface Iterator<T> { next(): { value: T } }` → `next()` 结果 `value` 成员类型 == `Iterator` 的类型参数（红：`resolve_name("T")` miss → `error_type` ≠ 类型参数；接口 `locals` 不含 `T`） | `get_type_from_type_reference`：`resolve_name` miss 时 fallback `resolve_type_parameter_in_scope`（沿父链扫 `type_parameter_list_of` 的 `<...>` 列表按名匹配 → 类型参数符号） |
| 3 | `for_of_iterable_loop_variable_is_typed_as_iterator_value`（tracer，经 `get_diagnostics`） | 合成 `Iterator<T>`/`It` + `declare const it: It; for (const x of it) { const n: number = x; }` → 1 诊断 `2322` "Type 'string' is not assignable to type 'number'."（红：`x` 保持 `any` → 0 诊断） | `get_iterated_type_or_element_type`：数组捷径后接私有 `get_iterated_type_of_iterable`（`__@iterator` 成员 → 调用签名返回类型=iterator → `next()` 返回类型=结果 → `value` 类型经迭代器引用 mapper 实例化）+ 私有 `first_signature_return_type`/`type_reference_mapper`；`check.rs` 类型解析点（`check_identifier`/`check_variable_declaration`/`check_property_declaration`）改穿 `program.globals()`，触发 `It` 的 `__@iterator` late-binding |
| 3 守卫 | `for_of_iterable_loop_variable_value_assignable_to_matching_target` | 同上但 body `const s: string = x;` → 无诊断（证明 `x` 真为 `string`，非 blanket 报错） | 同切片3（无新触点） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅做加法/体扩展）**：
- `core/declared_types.rs`：`get_type_from_type_node` 加 `Kind::TypeLiteral` 臂 + 私有 `get_type_from_type_literal_node`；`get_type_of_symbol` 的 FUNCTION 臂改含 `METHOD`；`get_signatures_of_symbol`/`get_signature_from_declaration` 加 `MethodSignature`/`MethodDeclaration` 臂；`get_type_from_type_reference` 加类型参数 fallback + 私有 `resolve_type_parameter_in_scope`/`type_parameter_list_of`。
- `core/check.rs`：`get_iterated_type_or_element_type` 体扩展（数组捷径 → 可迭代物路径）；新增私有 `get_iterated_type_of_iterable`/`first_signature_return_type`/`type_reference_mapper`；`check_identifier`/`check_variable_declaration`/`check_property_declaration` 的类型解析改穿 `program.globals()`（触发 late-binding）；`use` 增 `get_property_of_type`。

**最小输入 → 可观察**：
- 类型字面量：`{ value: string }` → 匿名对象；`o.value` → `string`。
- method 调用签名：`interface I { m(): string }` → `typeof m` 含 1 call sig，返回 `string`。
- 类型参数 fallback：`Iterator<T>` 的 `next()`-`value` → `Iterator` 的类型参数。
- iterator-protocol for-of：`for (const x of it)`（`it: It`，`It` 有 `[Symbol.iterator](): Iterator<string>`）→ `x: string`；body `const n: number = x` → `2322`；`const s: string = x` → 无诊断。

**测试增量**：278 单测（+5：切片1 / 切片2 / 切片2.5 / 切片3 tracer / 切片3 守卫）+ 121 doctest（+0，**未新增 `pub fn`**）。相对 4ag 基线 273+121。

**公开 API 仅做加法/体扩展（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`get_type_from_type_node`/`get_type_of_symbol`/`get_signatures_of_type`/`get_return_type_of_call`/`get_property_of_type` 原样保留；新增物全为私有 fn / 新 match 臂 / 既有体扩展。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（278 单测 + 121 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/es6/for-ofStatements/`（非数组可迭代物 for-of 元素类型化的最基础形态）与 `es6/Symbols/`、`externalModules/`（well-known-symbol 迭代器消费端）——无 union/异步迭代/字符串迭代/生成器/`downlevelIteration`/解构循环变量的子集。

**本轮 DEFER（带 blocked-by）**：
- **`getIterationTypesOfIterable` 完整形态**（rhs 为 union 时按成员分配并并集元素类型）+ **异步可迭代物 / `Symbol.asyncIterator`（`__@asyncIterator`）/ `for await`**：本轮单一可迭代物 + 同步 `__@iterator`。blocked-by: union 元素分配 + 异步迭代类型（lib.d.ts, P6）。
- **字符串迭代**（`for (const c of "abc")` → `string`）/ **生成器** / **`downlevelIteration`**：本轮只走 `[Symbol.iterator]` 成员协议。blocked-by: `getElementTypeOfStringType` + 生成器返回类型 unwrap + `downlevelIteration` 选项接线。
- **`2488`/`2489` 诊断**（"Type must have a `[Symbol.iterator]()` method" / "An iterator must have a `next()` method"）：reachable（本轮缺 `__@iterator`/`next` 时静默返回 None，未报）。blocked-by: 诊断节点定位 + `getIteratedTypeOfIterable` 的 `errorNode`/`allowAsyncIterables` 参数线。
- **匿名对象深实例化**（`getTypeOfPropertyOfType` 经引用 mapper 实例化匿名 `next` 函数类型 / `{value:T}` 字面量）：本轮用「取 `value` 类型再经迭代器引用 mapper 实例化」绕过。blocked-by: `instantiateAnonymousType`（重建成员/签名/索引签名的实例化类型，4e）。
- **类型参数绑进 `locals`**（binder 侧）/ **`unique symbol` 类型构造**：本轮 checker 侧补 `resolve_type_parameter_in_scope` fallback；`Symbol.iterator` 的 unique-symbol 名仍走 4ag fallback `"__@"+name`。blocked-by: binder 类型参数 locals（边界外）+ unique-ES-symbol 类型化。

**推荐下一轮（4ai）**：(a) `getIterationTypesOfIterable` 的 union 分配（rhs 为 union 时按成员分配元素类型再并）+ `2488`/`2489` 诊断（缺 `[Symbol.iterator]`/`next` 时报）；(b) 字符串迭代（`for...of "abc"` → `string`，需 `getElementTypeOfStringType`）；(c) 匿名对象深实例化（`instantiateAnonymousType`，使 `getTypeOfPropertyOfType` 经引用 mapper 正确实例化匿名 `next`/`{value:T}`，回归 Go-faithful 路径），最终 blocked-by P6 真 lib.d.ts。

## 4ai 落地记录（worklog 摘要）—— for-of 迭代诊断（2488/2489）+ 字符串迭代

**目标**：续 4ah 的 iterator-protocol for-of 元素类型化，落地三个互相独立的 red→green 切片：(1) 迭代物缺 `[Symbol.iterator]()` → `2488`；(2) `[Symbol.iterator]()` 存在但其返回的迭代器无 `next()` → `2489`；(3) 字符串迭代（`for...of` 一个 `string`）把循环变量类型化为 `string`（不报 `2488`）。全程逐行为红→绿，一次一个，每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿。

**关键 Go ground-truth 确认**：
- 码/文本（`diagnostics_generated`）：`2488` = "Type '{0}' must have a '[Symbol.iterator]()' method that returns an iterator."；`2489` = "An iterator must have a 'next()' method."。
- `checkRightHandSideOfForOf` → `checkIteratedTypeOrElementType(use, rhsType, undefined, rhsExpr)`：**errorNode = for-of 的 rhs 表达式**；`IsTypeAny(input)` 直接返回 input（不报 2488）；元素类型 None 时返回 `c.anyType`。
- `getIteratedTypeOrElementType`：可迭代物路径（`getIterationTypesOfIterable`）；失败后 `IterationUseAllowsStringInputFlag`（for-of 含此位）下，string-like 输入 → `arrayType=never`、`hasStringConstituent=true` → 返回 `c.stringType`。
- `reportTypeNotIterableError`（sync）→ `2488`，类型经 `TypeToString`。`getIterationTypesOfMethod`（methodName=="next"，无 call signatures）→ `resolver.mustHaveANextMethodDiagnostic` = `An_iterator_must_have_a_next_method`（`2489`）。
- **2488 vs 2489 的 Go 结构（baseline `for-of16.errors.txt` 实测）**：`[Symbol.iterator](): {}`（迭代器无 `next`）→ 顶层 **2488** + **2489 作为 related info**；`for-of14`（有 `next` 但无 `[Symbol.iterator]`）→ 仅 `2488`。即 Go 顶层只报 1 个诊断（2488），2489 是其 related info。

**reachable 子集 / 与 Go 的偏离（DIVERGENCE，已据 baseline 校正）**：本仓库 checker 侧诊断为平表（无 related-info 接线）。本轮 reachable 子集做如下区分：迭代器方法**缺席**（或其返回类型无法解析为迭代器）→ 顶层 `2488`；迭代器方法**存在**但其迭代器**无 `next()`** → 顶层 `2489`（Go 把 2489 作为 2488 的 related info；本轮把更具体的 2489 直接提为顶层诊断）。完整的 `2488`-primary + `2489`-related 嵌套结构 DEFER（blocked-by: related-info 接线）。字符串迭代不实现 `getElementTypeOfStringType` 的 wrapper-类型路径，只对 string-like 输入直接返回 `string`（元素类型与 Go 一致）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红） | 实现触点 |
|---|---|---|---|
| 1 | `for_of_non_iterable_object_reports_2488`（tracer，经 `get_diagnostics`） | `declare const v: { a: number }; for (const x of v) {}` → 1× `2488` "Type '{ a: number; }' must have a '[Symbol.iterator]()' method that returns an iterator."（红：缺迭代器静默返回 None → 0 诊断） | for-of 臂改调新私有 `check_iterated_type_or_element_type`（数组捷径 + iterator 协议 + errorNode）；`get_iterated_type_of_iterable` 取 errorNode，`__@iterator` 缺席 / 迭代器类型未解析 → 私有 `report_type_not_iterable`（`2488`，经 `type_to_string`）。**额外修复**：`nodebuilder::type_to_string` 对带内部 `\u{FE}`-前缀名（`__type` 等）的匿名对象符号改 serialize 成员字面量（此前误印符号名 `þtype`） |
| 2 | `for_of_iterator_without_next_method_reports_2489`（tracer，经 `get_diagnostics`） | 合成 `Symbol`/`SymbolConstructor` + `interface Bad { [Symbol.iterator](): {}; } declare const b: Bad; for (const x of b) {}` → 1× `2489` "An iterator must have a 'next()' method."（红：切片1 实现下 next 缺席静默返回 None → 0 诊断） | `get_iterated_type_of_iterable`：`next` 成员缺席 / 其类型无 call signature → 私有 `report_iterator_missing_next`（`2489`） |
| 3 | `for_of_over_string_types_element_as_string`（tracer，经 `get_diagnostics`） | `declare const s: string; for (const c of s) { const n: number = c; }` → 1× `2322` "Type 'string' is not assignable to type 'number'."（红：string 落 iterator 协议 → 误报 `2488`，`c` 保持 `any`） | `check_iterated_type_or_element_type`：数组捷径后、iterator 协议前插 string-like 分支（`STRING_LIKE` → `string_type`） |
| 3 守卫 | `for_of_over_string_element_assignable_to_string_target` | 同上但 body `const t: string = c;` → 无诊断（证明 `c` 真为 `string`，且不误报 `2488`） | 同切片3（无新触点） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅做加法/体扩展）**：
- `core/check.rs`：for-of 臂改调 `check_iterated_type_or_element_type`（无条件解析 rhs 元素类型 + 报诊断，再据 VariableDeclarationList 类型化循环变量）；`assign_for_of_element_types` 改为直接收 `element_type: TypeId`（私有签名变更）；新增私有 `check_iterated_type_or_element_type`（any 短路 + 数组捷径 + string-like 分支 + iterator 协议）、私有 `report_type_not_iterable`（2488）、私有 `report_iterator_missing_next`（2489）；`get_iterated_type_of_iterable` 改取 `error_node` 并区分 2488/2489 失败臂。
- `core/nodebuilder.rs`：`type_to_string` 对象命名分支加内部符号名守卫（名以 `INTERNAL_SYMBOL_NAME_PREFIX` 起头 → serialize 成员，而非印 `__type`）。

**最小输入 → 可观察**：
- 2488：`for (const x of v)`（`v: { a: number }`）→ `2488`（类型印 `{ a: number; }`）。
- 2489：`for (const x of b)`（`b: Bad`，`Bad` 有 `[Symbol.iterator](): {}`）→ `2489`。
- 字符串迭代：`for (const c of s)`（`s: string`）→ `c: string`；body `const n: number = c` → `2322`；`const t: string = c` → 无诊断；无 `2488`。

**测试增量**：282 单测（+4：切片1 / 切片2 / 切片3 / 切片3 守卫）+ 121 doctest（+0，**未新增 `pub fn`**）。相对 4ah 基线 278+121。

**公开 API 仅做加法/体扩展（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`check_source_file`/`get_diagnostics`/`type_to_string`/`symbol_to_string` 等原样保留；新增物全为私有 fn / 既有私有 fn 签名变更 / 既有体扩展。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（282 单测 + 121 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/es6/for-ofStatements/`（`for-of14`/`for-of16` 的 2488/2489 形态、字符串 for-of 元素 `string`）的最基础子集——无 union 分配 / 异步迭代 / `downlevelIteration` / related-info 嵌套。

**本轮 DEFER（带 blocked-by）**：
- **`2488`-primary + `2489`-related 的嵌套结构**（Go 把 2489 经 `diagnosticOutput` 挂为 2488 的 related info）+ **errorNode-precise span 的进一步对齐**：本轮平表诊断，按 reachable 子集把 2489 提为顶层。blocked-by: related-info / `addRelatedInfo` 接线。
- **异步可迭代物 `2504`**（`for await` / `Symbol.asyncIterator` / `__@asyncIterator`）+ `reportTypeNotIterableError` 的 async 消息变体 + "forgot await?" 建议：本轮仅同步 `__@iterator`。blocked-by: `allowAsyncIterables` plumbing + `getAwaitedTypeOfPromise` + 异步迭代全局（lib.d.ts, P6）。
- **`downlevelIteration` / `target` 门控诊断**（`1232`/`2569`-style "can only be iterated through when using the --downlevelIteration flag..."）+ `isArrayLikeType` 的非数组诊断分流（`2461`/`2549`/`Type_0_is_not_an_array_type`）：本轮只报 2488/2489。blocked-by: `compilerOptions.downlevelIteration`/`target` 接线 + `getIterationDiagnosticDetails`。
- **union-of-iterables 元素类型分配**（rhs 为 union 时按成员分配并并集 + `string | string[]` 混合路径）：本轮单一类型。blocked-by: `getIterationTypesOfIterableWorker` 的 union 臂 + `combineIterationTypes`。
- **`return`/`throw` 方法 + `IteratorResult` 完整解析**（`getIterationTypesOfMethod` 的 return/throw 臂、`mustHaveAValueDiagnostic`、`IteratorYieldResult`/`IteratorReturnResult` 优化）：本轮仅 `next` + `value`。blocked-by: 完整 `IterationTypes` + `getIterationTypesOfIteratorResult`。
- **真 `getElementTypeOfStringType` wrapper 路径**（经全局 `String` 的 `[Symbol.iterator]`/`StringIterator<string>`）：本轮 string-like 输入直接返回 `string`。blocked-by: 真 lib.d.ts（P6）。

**推荐下一轮（4aj）**：(a) `getIterationTypesOfIterableWorker` 的 union 分配（rhs union 按成员分配元素类型再并 + `string | string[]` 混合）；(b) related-info 接线，把 2489 还原为 2488 的 related（回归 Go-faithful 顶层结构）；(c) `downlevelIteration`/`target` 门控诊断 + 非数组分流（`2461`/`Type_0_is_not_an_array_type`），最终 blocked-by P6 真 lib.d.ts。

## 4aj 落地记录（worklog 摘要）—— union-of-iterables 元素分配 + 诊断 related-information 基建（修复 4ai 偏离）

**目标**：续 4ah/4ai 的 for-of 工作，落地两个互相独立的 red→green 切片：(1) **union-of-iterables 元素分配**——`for (const x of u)`（`u: A[] | B[]`）把 `x` 类型化为各成员元素类型的并集（`A | B`）；(2) **诊断 related-information 基建** + 修复 4ai 偏离——缺-`next` 迭代器从「顶层 2489」回归 Go-faithful 的「顶层 2488 + 2489 作为 related info」。全程逐行为红→绿，一次一个，每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿。

**关键 Go ground-truth 确认**：
- `getIterationTypesOfIterableWorker`（`checker.go`）union 臂：`t.flags & TypeFlagsUnion != 0` 时对每个 `constituent` 递归 `getIterationTypesOfIterableWorker(constituent, use, nil, ...)`（**errorNode 传 nil**，成员级不单独报错），任一成员 `!hasTypes()` → 对整体 `t` 调 `reportTypeNotIterableError` 并 `return IterationTypes{}`；全部成功 → `combineIterationTypes` → `getIterationTypeUnion` → `getUnionType`（yield/return/next 各自并集）。
- related-info：`ast.Diagnostic` 有 `relatedInformation []*Diagnostic` 字段 + `AddRelatedInfo(d)` 追加。缺-`next` 时 `getIterationTypesOfMethod`(`"next"`) 把 2489（`mustHaveANextMethodDiagnostic`）推入 `diagnosticOutput`（`diags`）；worker 末尾 `errorNode != nil` 时创建顶层 2488（`reportTypeNotIterableError`）再对每个 `d` in `diags` 调 `diagnostic.AddRelatedInfo(d)` —— 即 2488 为 primary、2489 为其 related。

**reachable 子集 / 与 Go 的偏离（DIVERGENCE）**：union 分配只覆盖 yield（元素）类型的并集（return/next 类型 DEFER，本仓库 for-of 只消费元素类型）。`string | string[]` 混合（string-like 成员从 array 型剥离再折回 string 成员）DEFER。本轮把 4ai 的「2489 提为顶层」偏离**修复**为 Go-faithful 的 2488-primary + 2489-related。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红） | 实现触点 |
|---|---|---|---|
| 1 | `for_of_union_of_iterables_distributes_element_type`（tracer，经 `get_diagnostics`） | `interface Array<T>{[n:number]:T;length:number} declare const u: string[] \| number[]; for (const x of u) { const s: string = x; }` → 1× `2322` "Type 'string \| number' is not assignable to type 'string'."（红：union 整体落非可迭代 → 误报 `2488` "Type 'Array<string> \| Array<number>' must have a '[Symbol.iterator]()' method..."） | `check_iterated_type_or_element_type`：any 短路后插 union 臂（逐成员递归 `error_node=None` + `get_union_type` 并集；任一成员 None → 对整体报 `2488` 返回 None）；`error_node` 参数 `NodeId → Option<NodeId>`（成员级抑制报错），随之 `get_iterated_type_of_iterable`/`report_type_not_iterable`/`report_iterator_missing_next` 同步改 `Option<NodeId>` + None 守卫 |
| 1 守卫 | `for_of_union_of_iterables_element_assignable_to_union_target` | 同上但 body `const v: string \| number = x;` → 无诊断（证明 `x` 真为 `string \| number`，无 `2488`） | 同切片1（无新触点） |
| 2 | `for_of_iterator_without_next_method_reports_2488_with_related_2489`（red→green；4ai 偏离修复，经 `get_diagnostics`） | 合成 `Symbol` + `interface Bad { [Symbol.iterator](): {}; } declare const b: Bad; for (const x of b) {}` → 1 顶层 `2488` "Type 'Bad' must have a..."，其 `related_information` 含 1 条 `2489`（红：4ai 实现顶层报 `2489`，`diags[0].code == 2489 ≠ 2488`） | `Diagnostic` 加 `related_information: Vec<Diagnostic>`（默认空）+ `pub fn add_related_info`；`error` 拆出私有 `diagnostic_for_node`（构造不入库）/`add_diagnostic`（入库）；`report_iterator_missing_next` 改收 `input_type`，构造 2489-related → 构造 2488-primary → `add_related_info` → `add_diagnostic`；`get_iterated_type_of_iterable` 两处缺-`next` 臂改传 `input_type` |

**本轮交付（全部 `internal/checker/**`；公开 API 仅做加法）**：
- `core/check.rs`：`Diagnostic` 加公开字段 `related_information`（默认空）+ `pub fn Diagnostic::add_related_info`；`check_iterated_type_or_element_type` 加 union 分配臂 + `error_node: Option<NodeId>`；`get_iterated_type_of_iterable`/`report_type_not_iterable`/`report_iterator_missing_next` 改 `error_node: Option<NodeId>`（+ None 守卫）；`report_iterator_missing_next` 改报 2488-primary + 2489-related；`error` 拆出私有 `diagnostic_for_node`/`add_diagnostic`；for-of 臂调用点传 `Some(expression)`。
- `lib.rs`：重导出 `tsgo_diagnostics::Category`（`Diagnostic.category` 字段类型，doctest 可用；加法式）。

**最小输入 → 可观察**：
- union 分配：`for (const x of u)`（`u: string[] | number[]`）→ `x: string | number`；body `const s: string = x` → `2322`；`const v: string | number = x` → 无诊断。
- related-info：`for (const x of b)`（`b: Bad`，`Bad` 有 `[Symbol.iterator](): {}`）→ 顶层 `2488`（类型印 `Bad`）+ 其 `related_information[0]` 为 `2489`。

**测试增量**：284 单测（+2：切片1 / 切片1 守卫；切片2 为既有 4ai 测试重写，净 +0）+ 122 doctest（+1：`Diagnostic::add_related_info` 的 `# Examples`）。相对 4ai 基线 282+121。

**公开 API 加法式（compiler 保持绿）**：`Diagnostic` 仅**新增**字段 `related_information`（默认空），既有字段 `code`/`category`/`message`/`start`/`length` 与读取面（compiler 的 `program.rs`/`checkerpool.rs` 只读 `.code`/`.message`）原样保留；新增 `pub fn add_related_info` + 重导出 `Category`。唯一 `Diagnostic { ... }` 构造点在 checker 内（`diagnostic_for_node`），已补默认空字段。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（284 单测 + 122 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/es6/for-ofStatements/`（`for-of16` 的 2488-primary + 2489-related 形态、union-of-arrays for-of 元素并集）的最基础子集——无 async-iterable union / `string | string[]` 混合 / `downlevelIteration` / 完整 2769→2770→2345 overload-elaboration related 链。

**本轮 DEFER（带 blocked-by）**：
- **async-iterable union / `for await` union 分配**（`__@asyncIterator`、`2504` 的 union 形态）：本轮仅同步 `__@iterator` union。blocked-by: `allowAsyncIterables` plumbing + `getAwaitedTypeOfPromise` + 异步迭代全局（lib.d.ts, P6）。
- **`string | string[]` 混合 union**（string-like 成员从 array 型剥离、`hasStringConstituent` 折回 string 成员）：本轮 union 分配假定各成员都是「正常可迭代物」（数组/iterator 协议）。blocked-by: `getIteratedTypeOrElementType` 的 string-constituent split（`arrayType`/`hasStringConstituent`）。
- **完整 overload-elaboration related 链（2769→2770→2345）** + message-chain（`Diagnostic.messageChain`）：本轮只接了「单条 related diagnostic」基建（2489 挂 2488）。blocked-by: 重载解析 elaboration + `createDiagnosticChainFromErrorChain` / message-chain 基建。
- **union 分配的 return/next 类型并集**（`combineIterationTypes` 的 return/next 臂）：本轮只并 yield（元素）类型。blocked-by: 完整 `IterationTypes` 三元组 + 生成器/`yield*` 消费端。
- **`downlevelIteration`/`target` 门控 + 非数组分流（`2461`/`Type_0_is_not_an_array_type`）**：本轮仅 2488/2489。blocked-by: `compilerOptions.downlevelIteration`/`target` 接线 + `getIterationDiagnosticDetails`。

**推荐下一轮（4ak）**：(a) `string | string[]` 混合 union（string-constituent split → 元素并 `string`）；(b) `downlevelIteration`/`target` 门控诊断 + `isArrayLikeType` 非数组分流（`2461`/`Type_0_is_not_an_array_type`）；(c) 把 related-info 基建复用到 overload-elaboration 的 2769→2770 related 链起点，最终 blocked-by P6 真 lib.d.ts。

## 4ak 落地记录（worklog 摘要）—— `string | string[]` 混合 union + 非数组分流（2461/2495）+ `iterableExists` 门控

**目标**：续 4ah/4ai/4aj 的 for-of 工作，落地三个 red→green 切片（按可达性重排）：(1) `string | string[]` 混合 union 元素类型化为 `string`；(2) 无全局 `Iterable` 时（Go `getGlobalIterableType() == emptyGenericType`，即默认 `--target` < `es2015` / 无 `--downlevelIteration` 的世界）一个普通非可迭代物 for-of → `2495`；(3) 无 `Iterable` 时 `string | <非数组>` 混合 union → 在非 string 余部报 `2461` 且元素类型仍为 `string`。全程逐行为红→绿，一次一个，每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿。

**关键 Go ground-truth 确认（码/文本经 `diagnostics_generated`）**：
- `2461` = `Type_0_is_not_an_array_type` = "Type '{0}' is not an array type."（**注**：交办单标题写的就是 2461；它是 `getIterationDiagnosticDetails` 的 `allowsStrings == false` 臂——已剥离 string 余部仍非数组）。
- `2495` = `Type_0_is_not_an_array_type_or_a_string_type` = "Type '{0}' is not an array type or a string type."（`allowsStrings == true` 臂——无 string constituent 且 for-of 允许 string 输入）。
- `2802`（**非交办单猜的 2569**）= `Type_0_can_only_be_iterated_through_when_using_the_downlevelIteration_flag_or_with_a_target_of_es2015_or_higher`：`getIterationDiagnosticDetails` 在「类型本身可迭代（`yieldType != nil`）但 flag/target 太低」或「`isES2015OrLaterIterable(symbol.Name)`」时报。
- `getIteratedTypeOrElementType`（`checker.go:6076`）结构：`if iterableExists || allowAsyncIterables` 块先走 `getIterationTypesOfIterable`（含 union worker 分配 + 2488）；该块在 `iterableExists` 为真且 `yieldType == nil` 时直接 `return nil`（2488 已在内部报）。**仅当 `iterableExists` 为假**（无全局 `Iterable`）才落到 string-constituent split（`6116-6181`）→ `isArrayLikeType` → `getIterationDiagnosticDetails`（2461/2495/2802）。string split：`use & AllowsStringInputFlag` 下从 union 滤掉 `StringLike` 成员；若有 string constituent 且余部为 `never` → 返回 `c.stringType`；`string | string[]` 优化（`6166-6176`）：余部数组元素 string-like → 返回 `c.stringType`。

**reachable 子集 / 与 Go 的偏离（DIVERGENCE）**：
- 本仓库无真 lib.d.ts，无 `compilerOptions` 接线（checker 核心**完全没有** `compiler_options`/`target`/`downlevelIteration` 字段）。故 `iterableExists` 用**全局 `Iterable` 类型符号是否在 scope** 作 Go-faithful 代理（Go 的 `getGlobalIterableType() != emptyGenericType` 正是「lib 是否定义了 `Iterable`」，对应 `target >= es2015`）。
- **门控仅作用于失败上报**：元素**解析**路径（数组捷径 / string / union 分配 / `__@iterator` 协议）**不门控**——故 4ah 的 `It`（带 `[Symbol.iterator]`，未声明 `Iterable`）仍能解析到 `string` 元素（Go 在无 lib 世界会对 `It` 报 2495，因 `It` 非数组；本仓库续 4ah 简化继续解析 iterator 协议）。这是受认可的可达性偏离，与 4ah「总走 iterator 协议」一脉相承。
- 既有 4ai/4aj 失败上报测试（`{a:number}`→2488、`Bad`→2488+2489）**改为声明合成全局** `interface Iterable<T> {}` 使其落在 `iterableExists` 真臂（保持 2488 语义，更贴 Go ES2015 世界）；这是把它们从 4ah 的「总 2488」简化迁移到门控模型。
- `2802` 门控 + 真 `--downlevelIteration`/`--target` 选项 + `isES2015OrLaterIterable` 名表 DEFER（见下）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 | `for_of_string_or_string_array_union_types_element_as_string`（经 `get_diagnostics`） | 合成 `Array<T>` + `declare const u: string \| string[]; for (const x of u) { const n: number = x }` → 1× `2322`（`x` 为 `string`）。**实测：直接绿**——4aj 的 union 分配已对 `string`→`string`、`string[]`→`string` 逐成员解析并并集为 `string`，故本切片为**行为守卫**（非新红），确认混合 union 元素与 Go `string \| string[]` 优化结果一致 | 无（既有 union 分配覆盖） |
| 2 | `for_of_non_iterable_object_without_global_iterable_reports_2495`（tracer，经 `get_diagnostics`） | `declare const v: { a: number }; for (const x of v) {}`（**无** `Iterable`）→ 1× `2495`（红：现实现总报 `2488`） | 新私有 `global_iterable_type_exists`（查全局 `Iterable` TYPE 符号）；`check_iterated_type_or_element_type`/`get_iterated_type_of_iterable`/`report_type_not_iterable`/`report_iterator_missing_next` 加 `iterable_exists: bool` 线；`report_type_not_iterable` 在 `!iterable_exists` 时改报 `2495`（`Type_0_is_not_an_array_type_or_a_string_type`） |
| 3 | `for_of_string_or_non_array_union_reports_2461_on_remainder`（经 `get_diagnostics`） | `declare const u: string \| { a: number }; for (const x of u) {}`（**无** `Iterable`）→ 1× `2461` "Type '{ a: number; }' is not an array type."（红：临时屏蔽 2461 臂实测报 `2495`-on-whole-union ≠ `2461`-on-remainder） | union 臂抽出私有 `iterate_union`：string-constituent split（滤 `StringLike` 成员、`string` 成员贡献 `string` 元素）；某非 string 成员不可迭代时——`iterable_exists`→整体 `2488`；`!iterable_exists` 且有 string→对非 string 余部报 `2461`（新私有 `report_not_array_type`）返回 `string`；`!iterable_exists` 无 string→整体 `2495` | 
| 3 守卫 | `for_of_string_or_non_array_union_element_is_string` | 同上但 body `const n: number = x` → 2× 诊断 `{2322, 2461}`（证明 `x` 真为 `string`） | 同切片3（无新触点） |

**本轮交付（全部 `internal/checker/**`；公开 API 不变）**：
- `core/check.rs`：新增私有 `global_iterable_type_exists`（`get_global_symbol("Iterable", TYPE)` 存在性，Go `getGlobalIterableType` 代理）；union 臂抽出私有 `iterate_union`（string-constituent split + 2461/2495/2488 分流）；新增私有 `report_not_array_type`（2461）；`check_iterated_type_or_element_type`/`get_iterated_type_of_iterable`/`report_type_not_iterable`/`report_iterator_missing_next` 加 `iterable_exists: bool` 形参（私有签名变更）；`report_type_not_iterable` 与 `report_iterator_missing_next` 在 `!iterable_exists` 时走 2495 余/array 回退。for-of 臂调用点计算 `iterable_exists` 并下传。
- `core/check_test.rs`：4ak 4 个新测（切片1守卫/切片2/切片3/切片3守卫）；4ai `for_of_non_iterable_object_reports_2488` 与 4aj `for_of_iterator_without_next_method_reports_2488_with_related_2489` 改声明合成全局 `interface Iterable<T> {}`（迁移到门控模型，保持 2488 语义）。

**最小输入 → 可观察**：
- `string | string[]` → `x: string`（守卫，body `const n: number = x` → `2322`）。
- 无 `Iterable` 普通非可迭代物：`for (const x of {a:number})` → `2495`。
- 无 `Iterable` `string | {非数组}`：`for (const x of u)` → `2461`（印余部 `{ a: number; }`），元素 `string`（守卫 body `const n: number = x` → `{2322, 2461}`）。

**测试增量**：288 单测（+4：切片1守卫 / 切片2 tracer / 切片3 / 切片3守卫；4ai/4aj 两测改全局声明，净 +0）+ 122 doctest（+0，**未新增 `pub fn`**）。相对 4aj 基线 284+122。

**公开 API 不变（compiler 保持绿）**：未新增/未改任何 `pub fn` 签名——`check_source_file`/`get_diagnostics`/`get_global_symbol` 等原样保留；新增物全为私有 fn（`global_iterable_type_exists`/`iterate_union`/`report_not_array_type`）/ 既有私有 fn 签名变更（加 `iterable_exists`）/ 既有体扩展。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（288 单测 + 122 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/es6/for-ofStatements/`（无 `--downlevelIteration` 的 `string | string[]` 元素 `string`、非数组 for-of 的 `2461`/`2495` 分流）的最基础子集——无真 `--downlevelIteration`/`--target` 选项 / `2802` / async-iterable 分流。

**本轮 DEFER（带 blocked-by）**：
- **真 `--downlevelIteration` / `--target` 选项 + `2802` 诊断**（`Type_0_can_only_be_iterated_through_when_using_the_downlevelIteration_flag_or_with_a_target_of_es2015_or_higher`）+ `isES2015OrLaterIterable` 名表（`Float32Array` 等）：本轮 `iterableExists` 用全局 `Iterable` 存在性代理 target>=es2015；真选项 + 2802 需把 `compilerOptions.downlevelIteration`/`target` 接线进 checker（**当前 checker 核心完全无 options 字段**）。blocked-by: `compilerOptions` threading（P6 `tsoptions`）+ `getIterationTypeOfIterable`（判「可迭代但 flag 太低」）。
- **`isArrayLikeType` 的完整结构判定**（Go 对 union 检查每个成员都 array-like + 真 `getIndexTypeOfType(union, number)`）+ `noUncheckedIndexedAccess` 的 `includeUndefinedInIndexSignature` + `string | string[]` 在 `possibleOutOfBounds` 下的 `string | undefined`：本轮用「逐成员经 `check_iterated_type_or_element_type` 解析」近似 isArrayLikeType。blocked-by: `getIndexTypeOfType` union 分配 + `noUncheckedIndexedAccess` 选项。
- **元素解析路径的 `iterableExists` 门控**（无 `Iterable` 时 `It` 等 `[Symbol.iterator]`-only 物应 2495 而非解析）：本轮仅门控失败上报，解析续 4ah 不门控。blocked-by: 把 `getIterationTypesOfIterable` 真正纳入 `iterableExists` 块（需全局 `Iterable`/`Iterator` lib 类型，P6）。
- **async-iterable / `for await` 的非数组分流**（`2504` + async 消息变体）+ **overload-elaboration 2769→2770→2345 related 链**：未触及。blocked-by: `allowAsyncIterables` plumbing（P6）/ 重载 elaboration + message-chain。

**推荐下一轮（4al）**：(a) 把 `compilerOptions`（`downlevelIteration`/`target`）接线进 checker 核心（经 P6 `tsoptions`），落地真 `2802` 门控 + `isES2015OrLaterIterable` 名表；(b) `getIndexTypeOfType` 的 union 分配 + `isArrayLikeType` 完整结构判定；(c) 把 related-info 基建复用到 overload-elaboration 的 2769→2770 related 链起点，最终 blocked-by P6 真 lib.d.ts。

## 4al 落地记录（worklog 摘要）—— `compilerOptions` 接入 checker 核心 + strict 取值族 getters + 选项门控 2802

**目标**：把 `compiler_options` 接进 checker 核心（镜像 transformers 在 6e-2 经 `TransformOptions.compiler_options` 拿到选项的方式），解锁多轮（4y strictNullChecks、4ak downlevelIteration/target、exactOptionalPropertyTypes 等）反复 DEFER 的 "blocked-by: 无 compilerOptions 接线"。本轮**加法式**接线 + 落地 **ONE 选项门控行为**作证（4ak 的 `2802` 门控真选项化），其余（strictNullChecks 语义、exactOptionalPropertyTypes、完整选项矩阵）DEFER。

**关键设计（加法式 trait 方法，compiler 保持绿）**：`BoundProgram` 新增 `fn compiler_options(&self) -> &CompilerOptions`，**带默认**（返回进程级 `OnceLock` 全默认 `CompilerOptions`），故既有单文件实现（compiler 的 `BoundFile`/多文件 `MultiFileBoundProgram`、测试 `StubProgram`/`FileView`/`MultiFileProgram`）**无需改即满足**——与 4aa（`source_files`/`file_view`/...）同构的加法模式。checker 经 4l 保留的 program 读取（`Checker::compiler_options()` → `self.program().compiler_options()`，无 program 时降级全默认）。Go: `c.compilerOptions = program.Options()`。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 | `compiler_options_reflects_program_options`（tracer） | `StubProgram` opts `target: Es2015` → `c.compiler_options().target == Es2015`；无选项 program → `None`（红：`BoundProgram::compiler_options`/harness setter/`Checker::compiler_options` 均不存在 → 不编译） | `program.rs`：`BoundProgram::compiler_options()`（默认 = `default_compiler_options()` `OnceLock`）；`test_support.rs`：`StubProgram` 加 `options` 字段 + `parse_and_bind_with_options` + override；`mod.rs`：`Checker::compiler_options()` |
| S2 | `get_strict_option_value_follows_strict_and_explicit` / `strict_null_checks_reads_option` | `GetStrictOptionValue`：显式 per-option 胜，否则 `strict != TSFalse`；`strictNullChecks` 同。**实测真红**：初版按"默认→false"断言 → 跑出 Go 真语义 `GetStrictOptionValue(Unknown)=strict!=TSFalse`（默认 strict 未设 → enabled）→ 据此把 expected 改 Go 字面语义 | `mod.rs`：`Checker::get_strict_option_value`（委托 `CompilerOptions::get_strict_option_value`）/`strict_null_checks`（= `get_strict_option_value(opts.strict_null_checks)`） |
| S3 | `for_of_symbol_iterator_iterable_without_downlevel_iteration_reports_2802`（tracer，red→green） | `[Symbol.iterator]`-only `It` 在 `--target es5`（<es2015）+ 无 `--downlevelIteration` → 1× `2802`（红：现实现静默解析 iterator 协议 → 0 诊断） | `check.rs`：删 `global_iterable_type_exists`（4ak 代理）→ 新私有 `iterables_resolvable_via_protocol`（`opts.downlevel_iteration.is_true() \|\| opts.target as i32 >= Es2015 as i32`）；`check_iterated_type_or_element_type` 在 `!iterable_exists` 时**静默探测** yield 类型（Go `getIterationDiagnosticDetails` 用 nil errorNode 重算 yieldType）：可迭代（探测 Some）→ 新私有 `report_iteration_requires_downlevel`（`2802`）；否则 `report_type_not_iterable`（`2495`，既有路径） |
| S3 companion | `..._with_downlevel_iteration_resolves_element` / `..._with_es2015_target_resolves_element` | 同 `It`，开 `--downlevelIteration` 或 `--target es2015` → **无 2802**，元素解析为 `string`（body `const n: number = x` → 2322 证 `x: string`） | 同 S3（门控放行 → 走 iterator 协议解析） |

**4ah/4ai/4aj 的 4 个 iterator-protocol 测试迁移（同 GREEN 步）**：4ak 用"声明合成全局 `interface Iterable<T> {}`"作 iterableExists 代理；本轮代理被真选项替换，故 `for_of_iterable_loop_variable_is_typed_as_iterator_value`（及守卫）、`for_of_non_iterable_object_reports_2488`、`for_of_iterator_without_next_method_reports_2488_with_related_2489` 改为 `--target es2015` 选项进入 iterator-protocol world（同语义；后两者顺手删去现已多余的 `interface Iterable<T>` 声明）。无 `Iterable` 声明的 `2495`/`2461` 测试（`for_of_non_iterable_object_without_global_iterable_reports_2495` 等）保留默认选项（target None < es2015 → 非 iterator-world），行为不变。

**本轮交付（全部 `internal/checker/**`；公开 API 仅加法）**：
- `core/program.rs`：`BoundProgram::compiler_options()`（带默认，加法）+ 私有 `default_compiler_options()`（`pub(crate)` `OnceLock`）。
- `core/test_support.rs`：`StubProgram` 加 `options` 字段 + `parse_and_bind_with_options`（+ override `compiler_options`）。
- `core/mod.rs`：`Checker::compiler_options`/`get_strict_option_value`/`strict_null_checks`（3 个新 pub fn）。
- `core/check.rs`：删私有 `global_iterable_type_exists`；新私有 `iterables_resolvable_via_protocol`（读真选项）+ `report_iteration_requires_downlevel`（2802）；`check_iterated_type_or_element_type` 在 `!iterable_exists` 时改"静默探测 yield → 2802 / 2495"。

**最小输入 → 可观察**：
- 选项接入：`StubProgram(target:Es2015)` → `c.compiler_options().target == Es2015`；`strict:true` → `c.strict_null_checks()` true；`strict:true,strictNullChecks:false` → false。
- 选项门控 2802：`--target es5` + `for (const x of it)`（`it: It`，`It` 有 `[Symbol.iterator](): Iterator<string>`）→ `2802`；`--downlevelIteration` 或 `--target es2015` → 无 `2802`，`x: string`。

**测试增量**：294 单测（+6：S1 / S2×2 / S3 tracer / S3 companion×2；4 个 iterator-protocol 测试迁移，净 +0）+ 125 doctest（+3：`compiler_options`/`get_strict_option_value`/`strict_null_checks` 各一条 `# Examples`）。相对 4ak 基线 288+122。

**公开 API 仅加法（compiler 保持绿）**：新增 `BoundProgram::compiler_options()` 为**带默认的 trait 方法**（既有单文件实现 `compiler::BoundFile`/`MultiFileBoundProgram`、`StubProgram` 等无需改即满足——与 4aa 同构）；`Checker::compiler_options`/`get_strict_option_value`/`strict_null_checks` 为新 pub 方法；`new_checker`/`get_diagnostics`/`check_source_file` 等签名原样保留。**compiler 后续可 override `compiler_options()`** 提供真选项（届时 `2802`/strict 门控走真实路径，blocked-by P6 program 装配 + 真 lib.d.ts）；本轮 compiler 不 override → 走全默认（无行为变化）。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（294 单测 + 125 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/es6/for-ofStatements/`（`--target es5`/`--downlevelIteration` 门控的 `2802` 最基础形态）的最基础子集——无 `isES2015OrLaterIterable` 名表 / union 成员 2802 / async-iterable。

**本轮 DEFER（带 blocked-by）**：
- **strictNullChecks 语义**（接线已落地 `c.strict_null_checks()`，但尚无消费者：nullable 抽取/`getNonNullableType`/可选属性 `undefined` 注入/`&&`·`||`·`??` 的真值精化走 strict 分支）：blocked-by: `TypeFacts` 全量 + `getNonNullableType` + 关系层 strictNullChecks 消费端（4y/4p 标注处）。
- **`exactOptionalPropertyTypes`**（可选属性注入 `undefined`、`getNonMissingTypeOfSymbol`）：选项已可读，未接消费端。blocked-by: `missingType`/`getNonMissingTypeOfSymbol` + 关系层（4y 标注处）。
- **`isES2015OrLaterIterable` 名表**（`Float32Array`/`NodeList`/... 在无 lib 时也报 2802）+ **union 成员 2802**（`getIterationDiagnosticDetails` 经 `iterate_union`）：本轮仅单一 `[Symbol.iterator]` 物。blocked-by: 这些 lib 全局类型（P6）+ union 门控分流。
- **真 lib.d.ts 驱动的 `iterableExists`**（Go `getGlobalIterableType()` 由 effective target 加载的 lib 决定；本轮用 raw `--target`/`--downlevelIteration` 直读代替）：blocked-by: P6 default-lib 装配。
- **compiler 侧 override `compiler_options()` 提供真选项 + checkerpool 传递**：blocked-by: P6 compiler program 选项装配。
- **完整选项矩阵**（`noUncheckedIndexedAccess`/`strictFunctionTypes`/`useUnknownInCatchVariables`/... 的 checker 消费端）：本轮仅 `target`/`downlevelIteration`/strict 取值族 getter。blocked-by: 各自消费路径可达性。

**推荐下一轮（4am）**：(a) 用已接入的 `strict_null_checks()` 接 **strictNullChecks 的首个可观察消费者**（如 `??`/`&&`/`||` 的 nullish/真值精化走 strict 分支，或可选属性 `undefined` 注入的赋值性差异）；(b) `isES2015OrLaterIterable` 名表 + union 成员 2802 分流；(c) `getIndexTypeOfType` union 分配 + `isArrayLikeType` 完整结构判定，最终 blocked-by P6 真 lib.d.ts。

## 4am 落地记录（worklog 摘要）—— strictNullChecks 赋值性门控（首个可观察 strictNullChecks 消费者）

**目标**：用 4al 接入的 `strict_null_checks()` getter，落地 **首个可观察的 strictNullChecks 语义消费者** —— 关系层 `is_simple_type_related_to` 里 `undefined`/`null` 的 "非 strict 下可赋给任意类型" 规则（之前因 "无 compilerOptions 接线" 被 DEFER，恒走非 strict 路径之外的保守子集）。本轮**加法式**（仅私有方法内部精化，无公开签名改动），逐行为 red→green。

**Go 真值语义（ground truth，`relater.go:isSimpleTypeRelatedTo`）**：
```go
// In non-strictNullChecks mode, `undefined` and `null` are assignable to anything except `never`.
// Since unions and intersections may reduce to `never`, we exclude them here.
if s&Undefined != 0 && (!c.strictNullChecks && t&UnionOrIntersection == 0 || t&(Undefined|Void) != 0) { return true }
if s&Null      != 0 && (!c.strictNullChecks && t&UnionOrIntersection == 0 || t&Null != 0)            { return true }
```
即 `(!strict && t 非 union/intersection) || t 是 void-like(undefined) / null(null)`。strict 下只剩后半（`undefined`→`void`/`undefined`，`null`→`null`；`any`/`unknown` 由上方 top 规则吸收）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 tracer | `null_initializer_to_non_nullable_ok_when_not_strict` | `--strictNullChecks false` + `var x: string = null;`（`null` 关键字表达式 → `null` 型）→ **无 2322**（红：现实现恒报 `2322 "Type 'null' is not assignable to type 'string'."`，与 flag 无关） | `relations.rs`：`is_simple_type_related_to` 的 NULL 臂加 `(!strict_null_checks() && t 非 UNION_OR_INTERSECTION)` 选项 |
| S1 companion | `null_initializer_to_non_nullable_reports_2322_under_strict` | 同输入但 `--strictNullChecks true` → `2322`（绿：strict 下仍报，证 flag 差异） | 同上（strict 走原保守路径） |
| S2 | `undefined_initializer_to_non_nullable_ok_when_not_strict` | `--strictNullChecks false` + `declare const u: undefined;\nvar x: string = u;`（`undefined` 经类型注解的 const → `undefined` 型；`undefined` 标识符本身无 lib global 不可解析，见 DEFER）→ **无 2322**（红：临时回退 UNDEFINED 臂实测恒报 `2322 "Type 'undefined' ..."`） | `relations.rs`：UNDEFINED 臂加同样的非 strict 选项 |
| S2 companion | `undefined_initializer_to_non_nullable_reports_2322_under_strict` | 同输入 `--strictNullChecks true` → `2322`（绿） | 同上 |
| S3 guard | `undefined_initializer_to_nullable_union_ok_under_strict` | `--strictNullChecks true` + `var x: string \| undefined = u;`（`u: undefined`）→ **无诊断**（绿：经结构化 target-union 规则匹配 `undefined` 成员，证 strict 门控未过度收紧） | 无（既有 union 规则） |
| S4 relation 级 | `assignable_null_undefined_gated_on_strict_null_checks` | 直接 `is_type_assignable_to`：strict-off program → `null`/`undefined`→`string` 真；strict-on program → 假，但 `undefined`→`void`、各自→自身仍真 | 同 S1/S2（pub fn `is_type_assignable_to` 单元覆盖） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅加法）**：
- `core/relations.rs`：`is_simple_type_related_to`（私有）的 `undefined`/`null` 两臂从 "strictNullChecks-independent 子集" 精化为 Go 的完整门控（读 `self.strict_null_checks()`，`&self`，无签名改动）。注释更新（删 DEFER 段，换 Go 锚语义说明）。
- `core/check_test.rs`：S1/S1c/S2/S2c/S3 共 5 个行为级测试（经 `get_diagnostics` + `parse_and_bind_with_options` 驱动 flag）。
- `core/relations_test.rs`：S4 关系级测试（直接 `is_type_assignable_to`，双 flag 状态）。

**测试增量**：300 单测（+6：S1/S1c/S2/S2c/S3/S4）+ 125 doctest（+0；无新 pub fn）。相对 4al 基线 294+125。

**公开 API 仅加法（compiler 保持绿）**：唯一改动在私有 `is_simple_type_related_to` 内部（读已存在的 `strict_null_checks()` getter）；`is_type_assignable_to`/`is_type_related_to`/`get_diagnostics`/`check_source_file` 等签名原样保留。compiler 不 override `compiler_options()` → 走全默认（默认 `strict != false` → strictNullChecks ON），原有行为不变。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（300 单测 + 125 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeRelationships/typeAndMemberIdentity/` 与 `tests/cases/compiler/strictNullChecks*` 里 `undefined`/`null` 赋给非 nullable 的最基础形态（单变量声明），无 narrowing/`getNonNullableType`/`??`。

**本轮 DEFER（带 blocked-by）**：
- **`undefined` 标识符表达式 → `undefined` 型**（`var x = undefined;` / `var x: string = undefined;`）：本轮用类型注解的 const（`declare const u: undefined`）拿 `undefined` 型；`undefined` 作为标识符在无 lib global 时报 `2304 Cannot find name 'undefined'`。blocked-by: `undefinedSymbol` 全局注册（lib.d.ts / P6 default-lib，或 checker 侧 `undefinedSymbol` 内建）。
- **`getNonNullableType` / 真值-narrowing 的 strict 语义**（`if (x) ...` 在 strict 下抽 `undefined`/`null`、非空断言 `!`、`checkNonNullType`）：blocked-by: `TypeFacts` 全量 strict 分支 + `getNonNullableType`（4p/flow 标注处）。
- **`??` 的 nullish 结果精化走 strict 分支**（`hasTypeFacts(EQUndefinedOrNull)` + `GetNonNullableType`）+ `checkNullishCoalesceOperands`：blocked-by: `EQUndefinedOrNull` 全量 `TypeFacts` + `getNonNullableType`。
- **`exactOptionalPropertyTypes` / strict 下可选属性 `undefined` 注入**（`isPropertySymbolTypeRelated` / `getNonMissingTypeOfSymbol`）：blocked-by: `missingType`/`getNonMissingTypeOfSymbol`（4y 标注处）。
- **union 分配的 strictNullChecks 快路径**（`isUnionWithUndefined`/`isUnionWithNull` 抽 nullable 再并）+ **`Nullable & (Object|NonPrimitive)` → `never` 子句**：blocked-by: union origin 字段 + intersection nullable 归约。
- **函数/返回类型 strictNullChecks 效应、可选参数 `undefined` 注入**：blocked-by: 签名关系完整移植（4f+）。

**推荐下一轮（4an）**：(a) `getNonNullableType`（先建，供非空断言 `!` / 真值 narrowing strict 分支消费）+ `if (x)` 在 strict 下抽 `undefined`/`null` 的可观察差异；(b) union 分配的 strictNullChecks 快路径（`isUnionWithUndefined`/`isUnionWithNull`）；(c) `??` nullish 结果精化走 strict（需 `getNonNullableType` + `EQUndefinedOrNull`）。最终强 strict 语义仍 blocked-by P6 真 lib.d.ts。

## 4an 落地记录（worklog 摘要）—— EmitResolver 引用解析核心（scope-aware resolveName + isReferenced）

**目标**：落地 **P5 反复出现的 P5 megablocker 的钥匙**——"需要真 ReferenceResolver / checker EmitResolver"。这道阻塞门控 importElision、legacy decorators、declarations emit、以及作用域正确的 CJS/ESM/System 模块改写。本轮交付 EmitResolver 上两个**加法式** pub 方法：把标识符 USE 解析到其声明符号（作用域链上行），以及 importElision 需要的 `is_referenced` 查询（"这个 import/alias 是否被某个值位引用真正用到？"）。逐行为红→绿，一次一个。

**Go 真值语义（ground truth）**：
- `checker.go:resolveName` / `binder/nameresolver.go:Resolve`：从 location 起，沿父链向外走每个容器的 `locals`（block → function → module → globals），按 meaning 过滤，取**第一个**匹配（innermost 遮蔽优先）。
- `checker.go:isReferenced(7041)`：`return c.symbolReferenceLinks.Get(symbol).referenceKinds != 0`（Go 在 check 期由 `markLinkedReferences` 提前累积 `referenceKinds`，故 isReferenced 是 O(1) 标志读）。本轮**不**做 `markLinkedReferences` 提前累积（DEFER），而是按需扫描全文件标识符 USE + `resolve_reference` 直接计算同一答案——这正是 importElision 所需的作用域正确判定。

**现状定位**：自由函数 `resolve_name`（4b）已实现作用域链上行 + meaning 过滤 + globals 兜底；4z 加了全局作用域解析。缺口是：① 没有把"标识符 USE 节点 → 声明符号"暴露成 P5 可消费的 pub 方法（且通过 bound program 演示嵌套作用域 + 遮蔽）；② 没有 `is_referenced` 查询。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 tracer | `resolve_reference_picks_innermost_shadowing_declaration` | `var a = 1;\nfunction f() { var a = 2; a; }`：USE `a`（函数体内）→ **inner `var a` 符号**（红：`resolve_reference` 桩 `todo!()` panic） | `EmitResolver::resolve_reference`：`Identifier` → `resolve_name(node, text, VALUE\|ALIAS, globals())`（作用域链上行；innermost 命中 function locals） |
| S2 | `is_referenced_true_for_used_import_binding` | `import { x } from "m";\nx;`：import specifier `x` → **referenced=true**（红：`is_referenced` 桩 `todo!()`） | `EmitResolver::is_referenced`：取声明符号 → 扫全 arena 标识符 → 排除声明自身名节点 → 任一 `resolve_reference == 符号` 即 true |
| S2 companion | `is_referenced_false_for_unused_import_binding` | `import { y } from "m";`（无使用）→ **referenced=false**（红：同 `todo!()`；同时验证 specifier 自身名节点被 `declaration_name` 排除，否则误判 true） | 同上（`name_nodes` 排除声明名） |
| S3 guard | `is_referenced_is_scope_correct_not_name_match` | `import { x } from "m";\nfunction f() { var x = 1; x; }`：唯一 `x` USE 被 inner `var x` 遮蔽 → **referenced=false**（绿：证作用域正确，name-match 替身会误判 true） | 无（S1+S2 实现已满足；锁定本轮 headline 性质） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅加法）**：
- `core/emit_resolver.rs`：新增 pub 方法 `EmitResolver::resolve_reference(&self, program, node) -> Option<SymbolId>`（标识符 USE → 声明符号，作用域链上行，meaning=`VALUE|ALIAS`）+ `EmitResolver::is_referenced(&self, program, node) -> bool`（importElision 原语）；私有 helper `declaration_name`（取声明名节点：import/export specifier、namespace import、import clause、variable/function/class/interface decl）。
- `core/emit_resolver_test.rs`：S1/S2/S2c/S3 共 4 个行为级测试（经 `StubProgram` parse+bind 驱动）。

**新增 additive pub API 形状（供 P5 importElision 消费）**：
```rust
impl EmitResolver {
    // 标识符 USE 节点 → 它引用的声明符号（作用域正确，innermost 遮蔽优先）
    pub fn resolve_reference(&self, program: &dyn BoundProgram, node: NodeId) -> Option<SymbolId>;
    // 给定声明节点（如 import specifier）：文件内是否有值位 USE 解析到它？
    pub fn is_referenced(&self, program: &dyn BoundProgram, node: NodeId) -> bool;
}
```

**测试增量**：304 单测（+4：S1/S2/S2c/S3）+ 127 doctest（+2：`resolve_reference`/`is_referenced` 的 `# Examples`）。相对 4am 基线 300+125。

**公开 API 仅加法（compiler 保持绿）**：未改任何既有 `pub fn` 签名——新增物全为 `EmitResolver` 上的 pub 方法（`EmitResolver` 已在 `lib.rs` re-export，无需新增导出）+ 私有 helper。`resolve_name`(4b 自由函数)/`is_declaration_visible`/`serialize_type_of_declaration`/`is_implementation_of_overload`/`get_symbol_of_declaration` 原样复用。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（304 单测 + 127 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **type-only-ness（类型位 USE 不应保活值 import）**：需每个 USE 站点的 type-vs-value meaning + `markLinkedReferencesRecursively`。本轮 `resolve_reference` 用 `VALUE|ALIAS` meaning 近似，不区分 USE 站点是值位还是类型位。blocked-by: `markLinkedReferencesRecursively` + 完整 type-vs-value meaning 解析。
- **alias 按 target meaning 匹配**：本轮 alias 直接按 `ALIAS` 标志命中（而非跟随 alias 到 target、按 target 的 `getSymbolFlags` 测 meaning，Go `getSymbol`）。blocked-by: 跨文件模块 import/export 解析（`exports.go`/`resolveExternalModuleSymbol`）。
- **`markLinkedReferences` 提前累积 + `referenceKinds` O(1) 读**：本轮 `is_referenced` 按需扫描计算，不预累积。blocked-by: check 期 `markLinkedReferences` 接线。
- **完整 `getReferencedXxx` emit-resolver 面**（`GetReferencedValueDeclaration`/`IsReferencedAliasDeclaration`/`IsValueAliasDeclaration` 等）+ `resolveName` 的 TDZ/参数/this/special-name 规则、use-before-def 错误报告。blocked-by: `compiler.Program`（P6）+ 类型解析 + 别名解析。

**conformance 切片（登记，端到端对拍仍在 P10）**：importElision 相关 `tests/cases/compiler/importElision*` / 模块改写最基础形态（单文件命名导入用/不用），无 type-only / 跨文件 alias。

**推荐下一轮（4ao）**：(a) **P5 importElision 消费本轮 `is_referenced`**（替换 6e-2 的 name-match 替身，落地作用域正确的未用 import 省略）；或 (b) 扩到 type-only-ness（先建 USE 站点 type-vs-value meaning + `markLinkedReferences` 起步），让类型位 USE 不保活值 import；次选把 `resolve_reference` 的 alias 解析接到跨文件 target（需 `exports.go`）。

## 4ao 落地记录（worklog 摘要）—— EmitResolver value-alias 查询（IsValueAliasDeclaration / IsReferencedAliasDeclaration 可达子集）

**目标**：在 4an 的 `resolve_reference`/`is_referenced` 之上，落地 EXPORT 侧 importElision + `export =`/`import =` elision 的钥匙——EmitResolver 上两个**加法式** pub 方法：`is_value_alias_declaration`（一个 alias 声明的 target 是否（可达地）是个 *VALUE*）与 `is_referenced_alias_declaration`（一个 alias 声明是否被引用）。逐行为红→绿，一次一个。

**Go ground truth（emitresolver.go）**：
- `isValueAliasDeclarationWorker(718)`：对 `ImportClause`/`NamespaceImport`/`ImportSpecifier`/`ExportSpecifier` → `symbol != nil && isAliasResolvedToValue(symbol, true)`；`ImportEqualsDeclaration` → `isAliasResolvedToValue(.., false)`；`ExportDeclaration` → `exportClause != nil && (IsNamespaceExport || Some(elements, isValueAliasDeclaration))`；`ExportAssignment` → 表达式为 identifier 则 `isAliasResolvedToValue`，否则 true。
- `isAliasResolvedToValue`：`target := getExportSymbolOfValueSymbolIfExported(resolveAlias(symbol))`；`getSymbolFlagsEx(symbol, excludeTypeOnly, excludeLocalMeanings)&Value != 0 && (preserveConstEnums || !isConstEnum(target))`——即"alias 的最终 target 是否有 VALUE 含义"。
- `IsReferencedAliasDeclaration(680)`：`if !IsAliasSymbolDeclaration(node) → false`；否则 `aliasLinks.referenced`（check 期 `markLinkedReferences` 提前累积）为 true，或（导出 alias 且 target 是 value）为 true。

**可达裁剪（本轮做什么 / 不做什么）**：跨模块 target value-ness（`import{x}from"m";export{x}` 这种再导出 import binding，需 `resolveExternalModuleSymbol`/`getExportSymbolOfValueSymbolIfExported`）**不可达**——故 `is_value_alias_declaration` 只做**本文件**子集：export/import specifier 的 (property)name 在本作用域按 **VALUE** meaning 解析成功 ⇔ value alias。`is_referenced_alias_declaration` 复用 4an `is_referenced` 作为 `referenced` 标志的作用域正确替身，并加 `is_alias_symbol_declaration` 守卫。

| 步骤 | 测试 | 输入 → 可观察（红怎么看到的） | 实现（最小转绿） |
| --- | --- | --- | --- |
| S1 tracer | `is_value_alias_declaration_true_for_exported_value` | `function f(){}\nexport { f };`：export specifier → **true**（红：`is_value_alias_declaration` 桩 `todo!()` panic） | `ExportSpecifier`/`ImportSpecifier` → `resolve_name((property)name, VALUE, globals())`.is_some() |
| S2 | `is_value_alias_declaration_false_for_exported_type_only` | `interface I{}\nexport { I };`：export specifier → **false**（红：先降级 impl 为 specifier 硬编 true，看断言红） | 恢复真实"按 VALUE 解析"逻辑（interface 仅 TYPE，VALUE 解析失败 → false） |
| S3 tracer | `is_referenced_alias_declaration_true_for_used_import` | `import { x } from "m";\nx;`：import specifier → **true**（红：`is_referenced_alias_declaration` 桩 `todo!()`） | `is_alias_symbol_declaration(node) && self.is_referenced(node)` |
| S4 guard | `is_referenced_alias_declaration_false_for_non_alias` | `function f(){}\nf();`：被引用的 function（`is_referenced`=true）→ **false**（红：先去掉 `is_alias_symbol_declaration` 守卫，看断言红） | 恢复守卫：非 alias 声明直接 false |

**本轮交付（全部 `internal/checker/**`；公开 API 仅加法）**：
- `core/emit_resolver.rs`：新增 pub 方法 `is_value_alias_declaration` + `is_referenced_alias_declaration`；私有 helper `is_alias_symbol_declaration`（可达结构种类：import/export specifier、namespace import/export、`import =`、namespace export decl、带 name 的 import clause）。
- `core/emit_resolver_test.rs`：S1/S2/S3/S4 共 4 个行为级测试（经 `StubProgram` parse+bind 驱动）。

**新增 additive pub API 形状（供 P5 importElision EXPORT 侧消费）**：
```rust
impl EmitResolver {
    // 一个 alias 声明（如 export specifier）是否（可达地）别名了一个 *VALUE*
    pub fn is_value_alias_declaration(&self, program: &dyn BoundProgram, node: NodeId) -> bool;
    // 一个 alias 声明（如 import specifier / import=）是否被某值位 USE 引用
    pub fn is_referenced_alias_declaration(&self, program: &dyn BoundProgram, node: NodeId) -> bool;
}
```

**测试增量**：308 单测（+4：S1/S2/S3/S4）+ 129 doctest（+2：两个新方法的 `# Examples`）。相对 4an 基线 304+127。

**公开 API 仅加法（compiler 保持绿）**：未改任何既有 `pub fn` 签名——新增物全为 `EmitResolver` 上的 pub 方法（已 re-export）+ 私有 helper。4an 的 `resolve_reference`/`is_referenced`/`declaration_name`、4b `resolve_name`、`get_symbol_of_declaration` 原样复用。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（308 单测 + 129 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **跨模块 target value-ness**（`import{x}from"m";export{x}` 再导出 import binding，及 `import =`/`export =` 真正的 target 是否 value）：需 `resolveAlias` 跟随 alias 到目标 + `getExportSymbolOfValueSymbolIfExported` + `getSymbolFlagsEx`。本轮 `is_value_alias_declaration` 只覆盖本文件直接 alias 到本地 value/type 的形态。blocked-by: 模块 import/export 解析（`exports.go`/`resolveExternalModuleSymbol`）。
- **type-only-ness**（`excludeTypeOnlyValues` / `getTypeOnlyAliasDeclaration`）：类型位 USE 不保活值 alias。blocked-by: `markLinkedReferences` + type-vs-value meaning。
- **其余 alias 形态**：`ImportClause`/`NamespaceImport`（默认/命名空间导入）、`ExportDeclaration`（`export *` / `Some(elements,...)`）、`ExportAssignment`/`import =` 的 value-ness、CJS `module.exports =`。blocked-by: 同上跨模块解析 + `ExpressionIsAlias`。
- **`IsReferencedAliasDeclaration` 的导出-alias-target-是-value 分支**（`target != nil && export modifier && getSymbolFlags(target)&Value`）+ `referenced` 提前累积 + const-enum/`preserveConstEnums` carve-out。blocked-by: alias target 解析 + `getSymbolFlags` + `markLinkedReferences`。
- **`canCollectSymbolAliasAccessibilityData` 短路**（Go：未开启时 `IsValueAliasDeclaration`/`IsReferencedAliasDeclaration` 直接返回 true）：本轮假定已开启。blocked-by: checker 该标志接线。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/compiler/importElision*` 的 EXPORT 侧最基础形态（本地 value 再导出 vs type-only 再导出），无 type-only / 跨文件 alias / const enum。

**推荐下一轮（4ap）**：**P5 importElision EXPORT 侧消费本轮 `is_value_alias_declaration`/`is_referenced_alias_declaration`**（落地 `export { ... }` 中 type-only 成员的省略 + `export =`/`import =` elision）；或扩 `is_value_alias_declaration` 到跨模块 target value-ness（先建 `resolveExternalModuleSymbol` 起步，让再导出 import binding 可判定）。

## 4ap 落地记录（worklog 摘要）—— EmitResolver alias completion（`import =` 名排除 + `export =` value-alias 分支）

**目标**：补齐 6ag（P5 importElision EXPORT 侧）实测 BLOCKED 并显式点名的两处 Go-faithful 扩展，均**加法式**扩既有私有 helper / 既有 pub 方法体（无签名变更）：
1. 私有 helper `declaration_name` 加 `ImportEqualsDeclaration` 臂（返回其绑定名 `x`）——使单文件 `import x = require("m")` 的 elision 可观察：未用的 `import =` 的自身名 `x` 被排除出 use-scan 后，`is_referenced` / `is_referenced_alias_declaration` 报 false（此前因 `x` 自解析恒为 true，6ag 实测确认）。
2. `is_value_alias_declaration` 加 `ExportAssignment` 臂（Go-faithful 分类 `export = expr`）。

**Go ground truth（emitresolver.go / ast）**：
- `isValueAliasDeclarationWorker(718)` 的 `KindExportAssignment` 臂：`if node.Expression() != nil && node.Expression().Kind == KindIdentifier { return isAliasResolvedToValue(getSymbolOfDeclaration(node), true) } return true`——表达式为 identifier 走 alias-to-value（可达裁剪：本文件按 VALUE meaning 解析该 identifier），非 identifier（如 `export = {}`）一律 `true`。
- `getNameOfDeclaration` / `node.Name()`：`ImportEqualsDeclaration` 的名是其 identifier（`AsImportEqualsDeclaration().Name`）。
- `IsTopLevelValueImportEqualsWithEntityName`：entity-name 形 `import x = a.b` 的 target value-ness——**DEFER**（需 `resolveAlias` 跟随 entity name 跨命名空间/模块 + `getSymbolFlagsEx`，本轮不可达）。

| 步骤 | 测试 | 输入 → 可观察（红怎么看到的） | 实现（最小转绿） |
| --- | --- | --- | --- |
| S1 tracer（genuine RED）| `is_referenced_false_for_unused_import_equals` | `import x = require("m");`（无使用）→ **is_referenced=false**（红：`declaration_name` 未列 `ImportEqualsDeclaration`，名节点 `x` 未排除，`x` 自解析回自身 symbol → `is_referenced` 恒 true，断言 `!is_referenced` 红——正是 6ag 实测的 blocker） | `declaration_name` 加 `NodeData::ImportEqualsDeclaration(d) => Some(d.name)` |
| S1 guard | `is_referenced_true_for_used_import_equals` | `import x = require("m");\nx;` → **true**（值位 `x;` 解析到 import-equals symbol；green-on-arrival，证明修复不过度省略） | 同上臂 |
| S2 tracer（genuine RED）| `is_value_alias_declaration_true_for_export_assignment_value` | `function f(){}\nexport = f;` → **true**（红：`match` 仅 `Import/ExportSpecifier`，`ExportAssignment` 落 `_ => false`） | `match` 加 `NodeData::ExportAssignment(_) => true`（硬编最小转绿） |
| S3（genuine RED）| `is_value_alias_declaration_false_for_export_assignment_type_only` | `interface I{}\nexport = I;` → **false**（红：S2 的硬编 `true` 对 type-only 也返回 true，断言 `!...` 红） | 恢复真实逻辑：表达式为 identifier 时按 **VALUE** meaning `resolve_name`（interface 仅 TYPE → 失败 → false），非 identifier → `true`（Go fallback） |

**本轮交付（全部 `internal/checker/**`；公开 API 仅加法）**：
- `core/emit_resolver.rs`：私有 helper `declaration_name` 加 `ImportEqualsDeclaration` 臂；既有 pub 方法 `is_value_alias_declaration` 的 `match` 加 `ExportAssignment` 臂（identifier→按 VALUE 解析 / 非 identifier→true）。**无新增 pub 方法、无签名变更**。
- `core/emit_resolver_test.rs`：S1/S1-guard/S2/S3 共 4 个行为级测试（经 `StubProgram` parse+bind 驱动）。

**测试增量**：312 单测（+4：S1/S1-guard/S2/S3）+ 129 doctest（**+0**：未新增 pub 项，仅扩既有方法体；既有方法 `# Examples` doctest 不变）。相对 4ao 基线 308+129。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名——本轮全是扩既有私有 helper（`declaration_name`）+ 既有 pub 方法体（`is_value_alias_declaration`）。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（312 单测 + 129 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **entity-name 形 `import =`**（`import x = a.b`）的 target value-ness（`IsTopLevelValueImportEqualsWithEntityName`）：需 `resolveAlias` 跟随 entity name + `getSymbolFlagsEx`。本轮 `declaration_name` 仅排除其名节点（使 require-形单文件 elision 可观察），未判定 entity-name target 是否 value。blocked-by: `resolveAlias`（entity name 跨命名空间/模块）+ `getSymbolFlagsEx`。
- **`export =` / export specifier 的跨模块 target value-ness**（再导出 imported binding：`import{x}from"m";export = x`）：本轮 `ExportAssignment` 臂只按本文件 VALUE meaning 解析表达式 identifier。blocked-by: `exports.go`/`resolveExternalModuleSymbol`/`getExportSymbolOfValueSymbolIfExported`。
- **`export default`（`is_export_equals=false`）的 value-alias**：与 `export =` 同臂（Go `KindExportAssignment` 同分支），逻辑已覆盖，但 P5 消费侧（importElision `export default e` 保留）仍待 6ah 接线。
- **type-only-ness / const-enum / `preserveConstEnums`**：同 4ao DEFER。blocked-by: `markLinkedReferences` + `getSymbolFlagsEx`。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/compiler/importElision*` 的 `import x = require("m")`（used/unused）+ `export = <value>` vs `export = <type>` 最基础单文件形态。

**推荐下一轮（4ah / P5 6ah）**：**P5 importElision 接 `import =`/`export =` 臂**——消费本轮 `declaration_name`（经 `is_referenced_alias_declaration` 省略未用 `import x = require("m")`）+ `is_value_alias_declaration` 的 `ExportAssignment` 分类（保留 `export = <value>` / 省略 type-only `export =`）。次选扩 checker 到跨模块 / entity-name target value-ness（先建 `resolveExternalModuleSymbol` 起步）。

## 4aq 落地记录（worklog 摘要）—— 函数/箭头**表达式**体下传（return 检查覆盖到表达式位函数）

**目标**：补上 4r「return 深化」DEFER 里点名的**函数/箭头表达式体下传**——4r 已让 `FunctionDeclaration` 体 + 类方法体经 `check_return_statement_expression`/`enclosing_explicit_return_type` 检查带注解返回类型（2322），但**表达式位**的 `function (): T {…}` / `(): T => {…}` 因 `check_expression` 落 `_ => error_type` 臂、体从未下传，其 `return` 永不被检查。本轮把这两类表达式的**块体**下传，使其 `return <expr>` 经既有 `enclosing_explicit_return_type`（父链已含 `FunctionExpression`/`ArrowFunction` 臂）对显式返回注解做可赋值检查。全程单文件 bound program + 既有 4d 关系引擎 + 4m 字面量广义化 + 4r 的 return 机制，无新依赖。

**Go 真值（ground truth）**：`checkExpression` 对 `KindFunctionExpression`/`KindArrowFunction` → `checkFunctionExpressionOrObjectLiteralMethod`/`checkArrowFunction` → 最终 `checkSourceElement(body)` 下传函数体；体内 `return` 经 `checkReturnStatement` 取容器签名返回类型（`getReturnTypeFromAnnotation`）做 `checkTypeAssignableToAndOptionallyElaborate` → 2322。本轮仅做**块体 + 带注解**这一可达子集（与 4r 同形）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker --lib <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 tracer（genuine RED）| `return_type_mismatch_in_function_expression_body_reports_diagnostic` | `const f = function (): number { return "s"; };` → **2322**（红：`check_expression` 无 `FunctionExpression` 臂，体未下传 → 0 vs 1） | `check_expression` 加 `FunctionExpression` 臂 → `check_function_expression`（块体 `check_statement(body)` 下传，返回 `error_type` 占位） |
| S2 tracer（genuine RED）| `return_type_mismatch_in_arrow_function_body_reports_diagnostic` | `const f = (): number => { return "s"; };` → **2322**（红：无 `ArrowFunction` 臂 → 0 vs 1） | `check_expression` 加 `ArrowFunction` 臂 → `check_arrow_function`（仅当 body 为 `Block` 时 `check_statement(body)` 下传；concise 表达式体 DEFER） |
| S3 guard | `return_type_assignable_in_function_expression_body_reports_no_diagnostic` | `const f = function (): string { return "s"; };` → `[]`（可赋值不误报） | 同 S1 臂 |
| S4 guard | `return_type_assignable_in_arrow_function_body_reports_no_diagnostic` | `const f = (): string => { return "s"; };` → `[]` | 同 S2 臂 |
| S5 guard | `arrow_function_body_descends_into_nested_expression` | `const f = () => { return y; };`（无返回注解）→ **2304**（证明体确实下传到一般表达式检查，而非仅 return-type 路径；未注解时 return-type 检查按 4r 已 DEFER 不触发） | 同 S2 臂 |

**本轮交付（全部 `core/check.rs`，均私有方法/私有臂）**：
- `check_expression`：新增 `FunctionExpression`、`ArrowFunction` 臂。
- helper：`check_function_expression`（块体下传）、`check_arrow_function`（仅 `Block` body 下传）。两者均返回 `error_type` 占位（函数类型推断/上下文类型 DEFER）。

**最小输入 → 诊断**：
- 2322（表达式位函数带注解返回类型不符）：`const f = function (): number { return "s"; };` / `const f = (): number => { return "s"; };` → "Type 'string' is not assignable to type 'number'."
- 2304（表达式位函数体下传到嵌套未定义名）：`const f = () => { return y; };` → "Cannot find name 'y'."

**测试增量**：317 单测（+5：S1–S5）+ 129 doctest（**+0**，未新增 `pub fn`）。相对 4ap 基线 312+129。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名——`check_expression`（pub）体扩展（新增两个 match 臂，签名不变），新增物全为私有方法。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（317 单测 + 129 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **箭头的 concise 表达式体**（`(): T => expr`，无 `return` 语句，须经 `checkExpressionWithContextualType` 把体表达式当返回值对注解做可赋值检查）。本轮仅下传 `Block` 体。blocked-by: concise-body 返回类型检查 + 上下文类型。
- **未注解函数/箭头的上下文/推断返回类型**（Go 从 body 推断 `getReturnTypeFromBody`）：本轮未注解仍按 4r DEFER 不做返回类型检查（S5 守卫确认无误报）。blocked-by: 上下文返回类型推断 + 函数体返回类型推断。
- **表达式位函数的自身（函数）类型**（`checkFunctionExpressionOrObjectLiteralMethod` 的签名/参数/`this` 类型构造、上下文类型回传）：本轮返回 `error_type` 占位。blocked-by: 函数签名构造 + 上下文类型 + `this`-typing。
- **generator/async 返回解包**（`return` 经 awaited/iterable 解包对 `Promise<T>`/`Generator` 注解检查）、`2366`「缺失 return」（须全路径 flow 可达性）、`2355`（返回类型排除 undefined）：同 4r DEFER。blocked-by: awaited/iterable 类型(lib globals P6) + flow 可达性。
- **嵌套对象字面量方法体 / getter-setter 表达式位**：本轮只做裸 `FunctionExpression`/`ArrowFunction`。blocked-by: 对象字面量方法符号/签名构造。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/functions/`、`expressions/functionExpressions/`、`expressions/arrowFunction/`、`statements/returnStatements/`（表达式位函数/箭头带注解返回类型 + 块体 return 最基础用例）——对应这些目录无 concise 体/上下文返回推断/泛型/async 的基础形态。

**推荐下一轮（4ar）**：箭头 **concise 表达式体** 的返回类型检查（`(): number => "s"` → 2322，须先建「体表达式当返回值」的可赋值检查，是 contextual-type 的最小起步）；次选未注解函数/箭头的**函数体返回类型推断**（`getReturnTypeFromBody`，解锁未注解 return 的覆盖 + 表达式位函数自身类型）。

## 4ar 落地记录（worklog 摘要）—— 箭头 **concise 表达式体** 的返回类型检查（2322）

**目标**：补上 4aq 明确 DEFER 的**箭头 concise（非块）表达式体**——4aq 已让 `(): T => { return … }` 的**块体** `return` 经 `enclosing_explicit_return_type` 对显式返回注解做可赋值检查（2322），但 concise 形态 `(): T => expr` 无 `return` 语句、4aq 的 `check_arrow_function` 仅在 `body == Block` 时下传，故体表达式从未对注解检查。本轮把 concise 体表达式**当作返回值**对箭头的显式返回注解做可赋值检查，复用 4aq 的 `check_return_statement_expression`/`enclosing_explicit_return_type`/2322 路径（无新依赖、无新公开 API）。这完成了可达的**带返回注解**检查面（块体 + concise 体）。

**Go 真值（ground truth）**：`checkFunctionExpressionOrObjectLiteralMethodDeferred`（checker.go ~10180）：`if ast.IsBlock(body) { checkSourceElement(body) } else { exprType := c.checkExpression(body); if returnType != nil { … checkReturnExpression(node, returnOrPromisedType, body, body, exprType, false) } }`——concise 体经 `checkExpression` 求值后由 `checkReturnExpression` → `checkTypeAssignableToAndOptionallyElaborate(exprType, returnType, body, body, …)` → 2322（errorNode = body 表达式自身）。本轮仅做**带注解 + 非 async**这一可达子集（async 的 awaited 解包 DEFER）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 tracer（genuine RED）| `return_type_mismatch_in_arrow_concise_body_reports_diagnostic` | `const f = (): number => "s";` → **2322**（红：4aq 仅下传 `Block` 体，concise 体未检查 → 0 vs 1） | `check_arrow_function` 加 `else` 臂：非块体调 `check_return_statement_expression(program, body, body)`，复用既有可赋值/2322 路径（body 父链 = 箭头 → 经 `enclosing_explicit_return_type` 找到 `type_node` 注解） |
| S2 guard | `return_type_assignable_in_arrow_concise_body_reports_no_diagnostic` | `const f = (): number => 1;` → `[]`（数字字面量可赋值不误报） | 同 S1 臂 |
| S3 guard | `return_type_matching_string_in_arrow_concise_body_reports_no_diagnostic` | `const f = (): string => "s";` → `[]`（匹配注解不误报） | 同 S1 臂；并确认 4aq 块体 `const f = (): number => { return "s"; }` → 仍 **2322** 不回归 |

**本轮交付（全部 `core/check.rs`，均私有方法/私有臂）**：
- `check_arrow_function`：原仅 `if body == Block { check_statement }`，新增 `else { check_return_statement_expression(program, body, body) }`——concise 体表达式当返回值对注解做可赋值检查。返回 `error_type` 占位不变（函数类型推断/上下文类型仍 DEFER）。
- 复用既有 `check_return_statement_expression`（4r/4aq）：以 body 作 errorNode 与 expression，`enclosing_explicit_return_type` 从 body 父链找到箭头 `type_node` 注解（与 Go `errorNode = body` 一致）。

**最小输入 → 诊断**：
- 2322（concise 体不符注解）：`const f = (): number => "s";` → "Type 'string' is not assignable to type 'number'."
- 无诊断（concise 体可赋值）：`const f = (): number => 1;` / `const f = (): string => "s";` → `[]`

**测试增量**：320 单测（+3：S1–S3）+ 129 doctest（**+0**，未新增 `pub fn`）。相对 4aq 基线 317+129。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名——仅在私有 `check_arrow_function` 体内加一个 `else` 臂；无新增公开物。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（320 单测 + 129 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **未注解箭头/函数的上下文/推断返回类型**（`getReturnTypeFromBody`，Go 从 body 推断；含 concise 体的 `getReturnTypeFromExpressionBody`）：本轮仅在**有显式返回注解**时检查 concise 体（与 4aq 块体同口径）；未注解时按 4r DEFER 不触发返回类型检查。blocked-by: 上下文返回类型推断 + 函数体返回类型推断。
- **async concise 体的 Promise 解包**（Go `unwrapReturnType` + `checkAwaitedType`：async 箭头体的 awaited 类型对 `Promise<T>` 注解检查）：本轮非 async 直比。blocked-by: awaited 类型(lib globals P6) + `unwrapReturnType`。
- **parenthesized / object-literal concise 体等边缘**（`(): T => ({...})`、带括号体）超出可达最小集；当前仅按一般表达式求值后比对注解，未做对象字面量上下文类型回传。blocked-by: 上下文类型 + 对象字面量 fresh/widening。
- **表达式位函数/箭头自身（函数）类型**：仍返回 `error_type` 占位（同 4aq）。blocked-by: 函数签名构造 + 上下文类型 + `this`-typing。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/arrowFunction/`、`functions/`（concise 体带注解返回类型最基础用例）——对应无上下文返回推断/泛型/async 的基础形态。

**推荐下一轮（4as）**：未注解函数/箭头的**函数体返回类型推断**（`getReturnTypeFromBody` / `getReturnTypeFromExpressionBody`，解锁未注解 return + concise 体的覆盖，并支撑表达式位函数自身类型）；次选 async 体的 `unwrapReturnType` + awaited 解包（须 lib globals，P6）。

## 4as 落地记录（worklog 摘要）—— EmitResolver `get_referenced_export_container`（CJS local-export use 改写原语，可达子集）

**目标**：在 4an 的 `resolve_reference` 之上，落地 P5 CommonJS 模块变换把**顶层导出绑定的 USE** 改写成 `exports.<name>` 所需的 checker 原语——`EmitResolver` 上一个**加法式** pub 方法 `get_referenced_export_container(program, use_node, prefix_locals) -> Option<NodeId>`：值位标识符 USE 若解析到本模块顶层*导出*绑定，返回该模块 `SourceFile` 节点（变换据此 qualify 为 `exports.x`）；否则 None。6aj（CJS default/namespace use 改写）点名本原语为 "local-export reference rewriting（`export { x }` / 导出声明 → `exports.x` use-site）" 的下一个 checker unblock。

**Go 真值（ground truth）**：`internal/checker/emitresolver.go:GetReferencedExportContainer` 委托 `internal/binder/referenceresolver.go:referenceResolver.GetReferencedExportContainer(node, prefixLocals)`：`getReferencedValueSymbol`（meaning=`ExportValue|Value|Alias`）解析 USE → 若 `symbol.Flags&ExportValue != 0` 取 `getMergedSymbol(symbol.ExportSymbol)`，且 `!prefixLocals && exportSymbol.Flags&ExportHasLocal != 0 && exportSymbol.Flags&Variable == 0` → 返回 nil（仅导出 *variable* 改写）；`parentSymbol := getParentOfSymbol(symbol)`，当 parent 是 `ValueModule` 且其 `ValueDeclaration.Kind == SourceFile` 时，单文件下 `symbolFile == referenceFile`（非 UMD-export）→ 返回该 `SourceFile`。namespace/enum 容器走 `FindAncestor`（DEFER）。

**绑定侧数据流（为何 work）**：模块文件里 `export const x = 1` 经 binder `declareModuleMember` 在文件 `locals` 留下一个仅带 `EXPORT_VALUE` 标志的 "phantom" local（`export_symbol` 指向 `exports` 表里的真实 `x`），故解析 USE 必须带 `EXPORT_VALUE` meaning 才命中该 local；真实导出符号 `x` 的 `parent` = 文件符号（`VALUE_MODULE`，`value_declaration` = `SourceFile`）。脚本文件里 `const y = 1` 经 `declare_source_file_member`（非模块）直接进 `locals`（无 `EXPORT_VALUE`、`parent` = None）→ None。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 tracer（genuine RED）| `get_referenced_export_container_source_file_for_exported_value_use` | `export const x = 1;\nx;`：USE `x` → **Some(source file)**（红：方法桩 `None`，断言 `Some(NodeId(9))` 红） | 实现 resolve（`EXPORT_VALUE\|VALUE\|ALIAS`）→ `EXPORT_VALUE` 则取 `export_symbol` → parent `VALUE_MODULE` + `value_declaration` 是 `SourceFile` → 返回该节点（此片**不**含 `ExportHasLocal` 守卫） |
| S2（genuine RED）| `get_referenced_export_container_none_for_exported_function_use` | `export function f() {}\nf;`：USE `f` → **None**（红：S1 impl 对 function 也返回 `Some(NodeId(7))`） | 加 Go 的 `!prefix_locals && ExportHasLocal && !Variable` 守卫（function ∈ `ExportHasLocal` 且非 variable → None） |
| S3 guard | `get_referenced_export_container_none_for_non_exported_local` | `const y = 1;\ny;`（脚本文件）→ **None**（S1 后即绿：local 无 `EXPORT_VALUE`、`parent` None） | 同 S1 路径（确认非导出无容器） |
| S4 guard | `get_referenced_export_container_none_for_shadowing_local` | `export const x = 1;\nfunction f() { const x = 2; x; }`：inner USE `x` → **None**（S1 后即绿：作用域正确，解析到非导出 inner 绑定） | 同 S1 路径（确认遮蔽不误返回外层导出容器） |

> 红→绿证据：S1（桩 None vs `Some(NodeId(9))`）、S2（S1-impl `Some(NodeId(7))` vs None）均为 genuine RED（实测断言失败）→ 最小实现转绿。S3/S4 是 S1 实现的自然结果（绿-on-arrival 的覆盖守卫，非伪造红），如实记录。

**本轮交付（全部 `core/emit_resolver.rs`）**：
- 新增 pub 方法 `EmitResolver::get_referenced_export_container`（加法式；`EmitResolver` 已 re-export，无需改 `lib.rs`）。复用 4b `resolve_name` + `BoundProgram` 符号视图，无新增私有 helper、无新依赖。
- `core/emit_resolver_test.rs`：S1–S4 共 4 个行为级测试（经 `StubProgram` parse+bind 驱动）。

**新公开 API 形状（供 P5 CJS 消费）**：
```rust
impl EmitResolver {
    pub fn get_referenced_export_container(
        &self,
        program: &dyn BoundProgram,
        node: NodeId,        // a value-position identifier USE
        prefix_locals: bool, // CJS passes false; only exported variables → exports.x
    ) -> Option<NodeId>;     // the SourceFile node to qualify against, else None
}
```

**测试增量**：324 单测（+4：S1–S4）+ 130 doctest（+1：新方法的 `# Examples`）。相对 4ar 基线 320+129。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名——新增物只有 `EmitResolver` 上一个 pub 方法。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（324 单测 + 130 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **namespace/enum 导出容器**（Go 末尾 `FindAncestor` 匹配 `ModuleDeclaration`/`EnumDeclaration` 且 `getSymbolOfDeclaration(n) == parentSymbol`）：本轮仅返回模块 `SourceFile` 容器。blocked-by: namespace/enum 成员符号 parent 链 + `FindAncestor` 容器匹配。
- **跨模块 UMD-export**（Go `symbolFile != referenceFile` → nil）：单文件下恒等（symbolFile == referenceFile），跨文件须 `compiler.Program`。blocked-by: 多文件 program（P6）。
- **`prefix_locals == true`**：守卫 `!prefix_locals` 已 Go-faithful 实现（true 时不返回 nil），但本轮未单测（CJS 变换恒传 false）。blocked-by: —（仅缺测试覆盖；实现已在）。
- **`startInDeclarationContainer`**（Go 对 module/enum *声明名* 从其容器起解析以避免自引用）：本轮 USE 均非 module/enum decl-name，恒 false。blocked-by: namespace/enum 容器解析（同上）。
- **`getMergedSymbol` 规范化 / `getResolvedSymbol` 缓存**：单文件下 `getMergedSymbol` 恒等；无 check 期解析缓存，按 `resolve_name` 现算（与 4an `resolve_reference` 同口径）。blocked-by: 跨文件符号 merge + check 期引用累积。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/externalModules/`、`compiler/`（CommonJS `export const`/`export {}` use-site → `exports.x` 改写的最基础形态）——对应无 namespace/enum 容器、无跨模块再导出的基础形态。

**推荐下一轮（P5 6ak）**：**P5 CJS 模块变换消费本轮 `get_referenced_export_container`**——把顶层导出 *variable* 绑定的 use-site 改写成 `exports.<name>`（`export const x = 1; x;` → `exports.x = 1; exports.x;`），落地 6aj 点名的 local-export reference rewriting。次选扩 checker 到 namespace/enum 导出容器（先建成员符号 parent 链 + `FindAncestor`），让 `namespace N { export const x }` 内 `x` 的 USE 返回 `N`。

## 4at 落地记录（worklog 摘要）—— EmitResolver `serialize_type_node_for_metadata`（legacy-decorator `design:type` 元数据地基，keyword-type 子集）

**目标**：为 P5 legacy-decorator `emitDecoratorMetadata`（`@dec x: number;` → `__metadata("design:type", Number)`）落地 checker 侧的**类型注解节点 → 运行时构造器描述符**映射地基——`EmitResolver` 上一个**加法式** pub 方法 `serialize_type_node_for_metadata(program, type_node) -> SerializedTypeNode` + 一个**加法式** pub 枚举 `SerializedTypeNode`（命名 Go 发射的运行时构造器 / `void 0`，供 P5 变换据此构造 metadata 表达式）。逐行为红→绿，一次一个。

**Go 真值（ground truth）**：`internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode` 的 `switch node.Kind` —— keyword 臂直接产 `s.f.NewIdentifier("<Ctor>")` 或 `s.f.NewVoidZeroExpression()`：`KindNumberKeyword`→`Number`、`KindStringKeyword`(+`KindTemplateLiteralType`)→`String`、`KindBooleanKeyword`→`Boolean`、`KindBigIntKeyword`→`BigInt`、`KindSymbolKeyword`→`Symbol`、`KindVoidKeyword,KindUndefinedKeyword,KindNeverKeyword`→`void 0`、`KindLiteralType`→`serializeLiteralOfLiteralTypeNode`（`KindNullKeyword`→`void 0`）、`KindObjectKeyword`→`Object`、`KindAnyKeyword,KindUnknownKeyword`(break 组)→switch tail `Object`、其余 → switch tail `Object`。`KindTypeReference` 走 `serializeTypeReferenceNode`→`resolver.GetTypeReferenceSerializationKind`（DEFER，需类型/value 解析 + `printer.TypeReferenceSerializationKind`）。

**为何放 `EmitResolver`（与 Go 的偏离说明）**：Go 里 keyword 臂由**变换器** `serializeTypeNode` 直接处理（不经 resolver），仅 `TypeReference` 臂才回调 `resolver.GetTypeReferenceSerializationKind`。本轮按 lane 约定把这套"类型注解→构造器"地基建在 `core/emit_resolver.rs`（同 4an–4as 的加法式 EmitResolver 风格），供 P5 装饰器变换统一消费；P5 落地时 keyword 走本方法、`TypeReference` 走后续 `get_type_reference_serialization_kind`。描述符用**新加法式枚举** `SerializedTypeNode`（变体名即 Go 发射的构造器：`Number`/`String`/`Boolean`/`BigInt`/`Symbol`/`Object` + `VoidZero`），而非 `printer.TypeReferenceSerializationKind`（后者是 `TypeReference` 臂的解析提示，DEFER）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 tracer（genuine RED）| `serialize_type_node_number_keyword_is_number` | `declare const x: number;`：`: number` 类型节点 → **`Number`**（红：方法桩返回默认 `Object`，`Object != Number` 实测断言失败） | 建枚举 `SerializedTypeNode` + 方法桩（返回 Go switch tail `Object`）→ 加 `Kind::NumberKeyword => Number` 臂 |
| S2（genuine RED）| `serialize_type_node_string_keyword_is_string` | `declare const x: string;` → **`String`**（红：默认 `Object`） | 加 `Kind::StringKeyword => String` 臂 |
| S3（genuine RED）| `serialize_type_node_boolean_keyword_is_boolean` | `declare const x: boolean;` → **`Boolean`**（红：默认 `Object`） | 加 `Kind::BooleanKeyword => Boolean` 臂 |
| S4（genuine RED）| `serialize_type_node_void_undefined_never_are_void_zero` | `void`/`undefined`/`never` 三个声明 → 各 **`VoidZero`**（红：默认 `Object`） | 加 `Kind::VoidKeyword \| UndefinedKeyword \| NeverKeyword => VoidZero` 臂 |
| S5（genuine RED）| `serialize_type_node_null_literal_is_void_zero` | `declare const x: null;`：`LiteralType`(literal=`NullKeyword`) → **`VoidZero`**（红：默认 `Object`） | 加 `Kind::LiteralType if literal==NullKeyword => VoidZero` 守卫臂（DEFER 非-null literal 臂） |
| S6（genuine RED）| `serialize_type_node_bigint_keyword_is_bigint` | `declare const x: bigint;` → **`BigInt`**（红：默认 `Object`） | 加 `Kind::BigIntKeyword => BigInt` 臂 |
| S7（genuine RED）| `serialize_type_node_symbol_keyword_is_symbol` | `declare const x: symbol;` → **`Symbol`**（红：默认 `Object`） | 加 `Kind::SymbolKeyword => Symbol` 臂 |
| S8 守卫（green-on-arrival）| `serialize_type_node_any_unknown_object_are_object` | `any`/`unknown`/`object` 三个声明 → 各 **`Object`**（**无新臂**：Go `object` 显式臂 + `any`/`unknown` break 组皆汇于 `Object` switch tail；本港两路汇于 `_ => Object` 默认） | 无（S1 桩默认即 `Object`，覆盖守卫） |

> 红→绿证据：S1–S7 均为 genuine RED（方法桩/前序臂返回默认 `Object`，实测断言 `Object != <期望>` 失败）→ 最小臂转绿。S8 是 `Object` 默认的自然结果（绿-on-arrival 覆盖守卫，**非伪造红**），如实记录（同 4as S3/S4 口径）。

**本轮交付（全部 `core/emit_resolver.rs` + `lib.rs` re-export）**：
- 新增 pub 枚举 `SerializedTypeNode`（7 变体 + rustdoc `# Examples`）+ pub 方法 `EmitResolver::serialize_type_node_for_metadata`（加法式）。`lib.rs` re-export 加 `SerializedTypeNode`（`EmitResolver` 已 re-export）。无新增私有 helper、无新依赖。
- `core/emit_resolver_test.rs`：新增 helper `var_type_annotation` + S1–S8 共 8 个行为级测试（经 `StubProgram` parse+bind 驱动）。

**新公开 API 形状（供 P5 legacy-decorator 消费）**：
```rust
pub enum SerializedTypeNode { Number, String, Boolean, BigInt, Symbol, Object, VoidZero }

impl EmitResolver {
    pub fn serialize_type_node_for_metadata(
        &self,
        program: &dyn BoundProgram,
        type_node: NodeId,     // a `: T` type-annotation node
    ) -> SerializedTypeNode;   // the runtime ctor (NewIdentifier) / `void 0` Go emits
}
```

**测试增量**：332 单测（+8：S1–S8）+ 132 doctest（+2：新枚举 + 新方法各一个 `# Examples`）。相对 4as 基线 324+130。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名——新增物只有 `EmitResolver` 上一个 pub 方法 + 一个 pub 枚举（均已 re-export）。`cargo build -p tsgo_compiler` 绿。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（332 单测 + 132 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **`TypeReference` → entity 构造器**（`Date`/class → 该 entity 的 identifier，Go `serializeTypeReferenceNode`→`GetTypeReferenceSerializationKind`）：本轮落到保守 `Object` tail。blocked-by: `get_type_reference_serialization_kind`（`resolveEntityName` value/type 解析 + `getDeclaredTypeOfSymbol` + `isTypeAssignableToKind` + `printer.TypeReferenceSerializationKind`）+ lib globals（`getGlobalPromiseConstructorSymbol`，P6）。
- **union/intersection/conditional 递归**（Go `serializeUnionOrIntersectionConstituents`：`never`/`unknown`/`any`/null/undefined 的 reduce/elide + `equateSerializedTypeNodes` 公共类型）：本轮落到 `Object` tail。blocked-by: 全 `serializeTypeNode` 递归 + `strictNullChecks` 标志（P5 变换驱动）。
- **`FunctionType`/`ConstructorType`→`Function`、`ArrayType`/`TupleType`→`Array`、`TypePredicate`、`TemplateLiteralType`→`String`**：keyword 之外的结构臂，本轮落到 `Object` tail。blocked-by: 各自 Go switch 臂的端口（P5）。
- **非-`null` literal-type 臂**（Go `serializeLiteralOfLiteralTypeNode`：string/numeric/bigint/`true`/`false` literal → `String`/`Number`/`BigInt`/`Boolean`，含负号前缀）：本轮仅 `null`→`VoidZero`，其余落到 `Object` tail。blocked-by: 全 `serializeLiteralOfLiteralTypeNode` 端口（P5）。
- **`SkipTypeParentheses`**（Go `serializeTypeNode` 开头）+ **`node == nil`→`Object`**（无注解属性）：本轮 `type_node` 恒为已存在的非括号类型节点。blocked-by: —（仅缺覆盖；实现按需补）。
- **`serializeTypeOfNode`/`serializeParameterTypesOfNode`/`serializeReturnTypeOfNode` 外层 + `__metadata(...)` 表达式构造**：是 P5 装饰器变换的活，消费本方法。blocked-by: P5 transformers（legacy decorators）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/decorators/`（`emitDecoratorMetadata` 的 `design:type` 基础形态，keyword 注解 `: number`/`: string`/`: boolean`/`: void` 等 → `Number`/`String`/`Boolean`/`void 0`）——对应无 `TypeReference`、无 union/array 的基础形态。

**推荐下一轮（P5 legacy decorators 消费本轮）**：**P5 装饰器变换 `serializeTypeOfNode`→`serializeTypeNode` keyword 臂消费本轮 `serialize_type_node_for_metadata`**，落地 `@dec x: number;` → `__metadata("design:type", Number)`（property/parameter 注解 → `design:type` 元数据）。次选扩 checker 到 `get_type_reference_serialization_kind`（先建 `resolveEntityName` value-ness + `printer.TypeReferenceSerializationKind`），解锁 `TypeReference`（`Date`/class）→ entity 构造器。

## 4au 落地记录（worklog 摘要）—— EmitResolver `serialize_type_node_for_metadata` 扩展（结构臂：`SkipTypeParentheses` + `TemplateLiteralType` + 非-`null` literal-type 臂）

**目标**：继续扩 4at 落地的 `serialize_type_node_for_metadata` 的 `match`，覆盖更多 Go `serializeTypeNode` 臂——本轮聚焦**复用既有 `SerializedTypeNode` 变体**的结构臂：Go 顶层 `ast.SkipTypeParentheses`（`(T)` unwrap）、`KindTemplateLiteralType`→`String`、`KindLiteralType`→`serializeLiteralOfLiteralTypeNode` 的非-`null` 臂（string/numeric/bigint/`true`/`false`/负号前缀）。逐行为红→绿，一次一个，仅扩 match（**未加枚举变体**）。

**Go 真值（ground truth）**：`internal/transformers/tstransforms/typeserializer.go:serializeTypeNode`：开头 `node = ast.SkipTypeParentheses(node)`（`for IsParenthesizedTypeNode(node){ node = node.Type() }`）；`case KindTemplateLiteralType, KindStringKeyword -> NewIdentifier("String")`；`case KindLiteralType -> serializeLiteralOfLiteralTypeNode(node.AsLiteralTypeNode().Literal)`。`serializeLiteralOfLiteralTypeNode`：`KindStringLiteral,KindNoSubstitutionTemplateLiteral->String`、`KindPrefixUnaryExpression`{operand∈{NumericLiteral,BigIntLiteral}→递归 operand}、`KindNumericLiteral->Number`、`KindBigIntLiteral->BigInt`、`KindTrueKeyword,KindFalseKeyword->Boolean`、`KindNullKeyword->void 0`。

**🔴 关键发现 / 头号 DEFER（Function/Array 变体破坏下游构建）**：scope 列的 slice 1–4（`FunctionType`/`ConstructorType`→`Function`、`ArrayType`/`TupleType`→`Array`）需要**新加 `SerializedTypeNode` 变体**（`Function`/`Array`）。**实测**：在枚举上加这两个变体后 `cargo build -p tsgo_compiler` 报 **E0004**——P5 `tsgo_transformers` 的 `legacydecorators.rs:serialized_type_to_expression` 对 `SerializedTypeNode` 做**无 wildcard 的穷尽 match**（`patterns SerializedTypeNode::Function and SerializedTypeNode::Array not covered`），而 `tsgo_compiler` 依赖 `tsgo_transformers`（其 `Cargo.toml`）→ 编译该依赖即失败。按本 lane 边界（**只改 `internal/checker/**` + 本 docs；不改 transformers；若加变体破坏 compiler 构建则 STOP+report**），**本轮不落地 Function/Array 变体**（已 revert 探针，compiler 恢复绿）。blocked-by: 一个可同时改 checker（加 `Function`/`Array` 变体）+ transformers（补 `serialized_type_to_expression` 对应臂或 wildcard）的协调 lane（P5/P6 跨 crate）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| A（genuine RED）| `serialize_type_node_parenthesized_unwraps_to_inner` | `declare const x: (number);`（`ParenthesizedType`）→ **`Number`**（红：无 paren 处理，落 `_ => Object` tail；实测 `Object != Number`） | 方法顶部加 `while let ParenthesizedType(d) { type_node = d.type_node }`（Go `SkipTypeParentheses`），随后落既有 `NumberKeyword => Number` 臂 |
| B（genuine RED）| `serialize_type_node_template_literal_type_is_string` | `` declare const x: `a${string}b`; ``（`TemplateLiteralType`）→ **`String`**（红：默认 `Object`） | 把 `Kind::StringKeyword => String` 改为 `Kind::TemplateLiteralType \| Kind::StringKeyword => String`（Go 同组臂） |
| C（genuine RED）| `serialize_type_node_string_literal_type_is_string` | `declare const x: "a";`（`LiteralType`/`StringLiteral`）→ **`String`**（红：旧 LiteralType 臂仅守 `NullKeyword`，余落 `Object`） | 把守卫式 `LiteralType` 臂重构为 `Kind::LiteralType => serialize_literal_of_literal_type_node(program, d.literal)`（新私有 helper）+ helper 加 `StringLiteral => String`（保留 `NullKeyword => VoidZero`） |
| D（genuine RED）| `serialize_type_node_numeric_literal_type_is_number` | `declare const x: 1;`（`NumericLiteral`）→ **`Number`**（红：默认 `Object`） | helper 加 `NumericLiteral => Number` |
| E（genuine RED）| `serialize_type_node_boolean_literal_types_are_boolean` | `declare const a: true; declare const b: false;` → 各 **`Boolean`**（红：默认 `Object`） | helper 加 `TrueKeyword \| FalseKeyword => Boolean` |
| F1（genuine RED）| `serialize_type_node_bigint_literal_type_is_bigint` | `declare const x: 1n;`（`BigIntLiteral`）→ **`BigInt`**（红：默认 `Object`） | helper 加 `BigIntLiteral => BigInt` |
| F2（genuine RED）| `serialize_type_node_negative_numeric_literal_type_is_number` | `declare const x: -1;`（`LiteralType`/`PrefixUnaryExpression`(NumericLiteral)）→ **`Number`**（红：默认 `Object`） | helper 加 `PrefixUnaryExpression` 臂：operand∈{Numeric,BigInt} 时递归 `serialize_literal_of_literal_type_node(operand)` |

> 红→绿证据：A–F2 **全部 genuine RED**（前序仅 `_ => Object` tail，或 LiteralType 仅守 `NullKeyword`；实测断言 `Object != <期望>` 失败）→ 最小臂转绿。**无伪造红、无 green-on-arrival**。

**本轮交付（全部 `core/emit_resolver.rs`）**：
- 方法 `serialize_type_node_for_metadata`：顶部加 `SkipTypeParentheses` unwrap 循环；`TemplateLiteralType` 并入 `String` 臂；`LiteralType` 臂改为派发到新私有 helper。
- 新增**私有** helper `serialize_literal_of_literal_type_node(program, literal) -> SerializedTypeNode`（Go `serializeLiteralOfLiteralTypeNode` 的可达子集：string/numeric/bigint/`true`/`false`/负号前缀 + `null`；其余落保守 `Object` tail）。
- `core/emit_resolver_test.rs`：+7 行为级测试（A–F2）。
- **未加 `SerializedTypeNode` 变体、未改任何既有 pub 签名、无 `lib.rs` 改动、无新依赖**。

**测试增量**：339 单测（+7：A–F2）+ 132 doctest（**±0**：未加 pub 项，新 helper 私有不需 doctest）。相对 4at 基线 332+132。

**公开 API 仅加法（compiler + transformers 保持绿）**：本轮**未新增任何 pub 项**（仅扩既有方法 match + 一个私有 helper），公开 API 形状与 4at 完全一致。`cargo build -p tsgo_compiler` 绿（已实测）。

**gate（实测）**：`cargo test -p tsgo_checker` 绿（339 单测 + 132 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。

**本轮 DEFER（带 blocked-by）**：
- **`FunctionType`/`ConstructorType`→`Function`、`ArrayType`/`TupleType`→`Array`**（scope slice 1–4）：需新 `SerializedTypeNode` 变体，**实测破坏 `tsgo_transformers` 无-wildcard 穷尽 match → `cargo build -p tsgo_compiler` E0004**；按边界本轮不落地。blocked-by: 可同时改 checker（加 `Function`/`Array` 变体）+ transformers（补 `serialized_type_to_expression` 对应臂/wildcard）的协调跨-crate lane。
- **`TypeReference`→entity 构造器**（`Date`/class）：落保守 `Object` tail。blocked-by: `get_type_reference_serialization_kind`（`resolveEntityName` value/type 解析 + `getDeclaredTypeOfSymbol` + `isTypeAssignableToKind` + `printer.TypeReferenceSerializationKind`）+ lib globals（P6）。
- **union/intersection/conditional 递归**（Go `serializeUnionOrIntersectionConstituents`：`never`/`unknown`/`any`/null/undefined 的 reduce/elide + `equateSerializedTypeNodes`）：落 `Object` tail。blocked-by: 全 `serializeTypeNode` 递归 + `strictNullChecks` 标志（P5 变换驱动）。
- **`TypePredicate`**（`x is T`→`Boolean`、`asserts`→`void 0`）：落 `Object` tail。blocked-by: `TypePredicateNode.AssertsModifier` 臂端口（P5）。
- **`serializeLiteralOfLiteralTypeNode` 的 `KindNoSubstitutionTemplateLiteral`→`String` 臂**：当前**不可达**——Rust parser `parseNonArrayType` 未把无替换模板（`` `abc` ``）类型路由到 `parseLiteralTypeNode`（落 type-reference）。blocked-by: parser `parseNonArrayType` 的 `NoSubstitutionTemplateLiteral` 臂。
- **`serializeTypeOfNode`/`serializeParameterTypesOfNode`/`serializeReturnTypeOfNode` 外层 + `__metadata(...)` 表达式构造**：P5 装饰器变换的活，消费本方法。blocked-by: P5 transformers（legacy decorators）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/decorators/`（`emitDecoratorMetadata` 的 `design:type`：literal-type 注解 `: "a"`/`: 1`/`: true`、括号类型 `: (number)`、模板字面量类型 → `String`/`Number`/`Boolean`）。

**推荐下一轮**：**协调跨-crate lane（P5/P6）落地 `Function`/`Array` 变体 + transformers `serialized_type_to_expression` 对应臂**，解锁方法/数组/函数-typed 成员的 `design:type=Function`/`Array`（本轮头号 DEFER）。次选扩 checker `get_type_reference_serialization_kind`，解锁 `TypeReference`（`Date`/class）→ entity 构造器。

## 4av 落地记录（worklog 摘要）—— EmitResolver `serialize_type_node_for_metadata` `Array`/`Function` 变体（协调跨-crate lane：checker 4av + transformers 6am）

**目标**：解锁 4au 的**头号 DEFER**——`ArrayType`/`TupleType`→`Array`、`FunctionType`/`ConstructorType`→`Function`。这两组臂需要**新加 `SerializedTypeNode` 枚举变体**（`Array`/`Function`），而 4au 实测加变体会破坏下游 `tsgo_transformers` 的**无-wildcard 穷尽 match** `serialized_type_to_expression`（`cargo build -p tsgo_compiler` E0004）。故本轮是一个**协调跨-crate lane**：同时拥有 `internal/checker/**` 与 `internal/transformers/**`，checker 加变体 + transformer 加对应臂在同一 lane 串行落地，workspace 在每个行为之间始终可构建（**先加 checker 变体 → 立即加 transformer match 臂保持穷尽 → 再观察行为红→绿**）。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。

**Go 真值（ground truth）**：`internal/transformers/tstransforms/typeserializer.go:serializeTypeNode`：`case ast.KindFunctionType, ast.KindConstructorType: return s.f.NewIdentifier("Function")`；`case ast.KindArrayType, ast.KindTupleType: return s.f.NewIdentifier("Array")`。发射标识符 `Array`/`Function` 已对 Go/`tsc --experimentalDecorators --emitDecoratorMetadata` 核对。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` + `cargo test -p tsgo_transformers <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1（Array，end-to-end，genuine RED×2）| 加 `Array` 变体 + transformer arm（保持构建）→ checker `serialize_type_node_array_type_is_array`（`: number[]` → **`Array`**；红：落 `_ => Object` tail，`Object != Array`）+ transformer `array_typed_property_serializes_to_array_constructor`（`class C { @dec x: number[]; }`+meta → `__metadata("design:type", Array)`；红：checker 未映射 → 发射 `Object`）| checker 加 `Kind::ArrayType => SerializedTypeNode::Array`（一处最小臂同时翻绿 checker+transformer 两个观察点）|
| 2（Tuple→Array，end-to-end，genuine RED×2）| checker `serialize_type_node_tuple_type_is_array`（`: [number, string]` → **`Array`**；红 `Object != Array`）+ transformer `tuple_typed_property_serializes_to_array_constructor`（`@dec x: [number, string]` → `Array`；红：发射 `Object`）| checker 扩既有臂 group → `Kind::ArrayType \| Kind::TupleType => SerializedTypeNode::Array` |
| 3（Function，end-to-end，genuine RED×2）| 加 `Function` 变体 + transformer arm → checker `serialize_type_node_function_type_is_function`（`: () => void` → **`Function`**；红 `Object != Function`）+ transformer `function_typed_property_serializes_to_function_constructor`（`@dec x: () => void` → `Function`；红：发射 `Object`）| checker 加 `Kind::FunctionType => SerializedTypeNode::Function`（翻绿 checker FunctionType + transformer）|
| 4（ConstructorType→Function，checker，genuine RED）| checker `serialize_type_node_constructor_type_is_function`（`: new () => C` → **`Function`**；红：FunctionType 臂未含 ctor，落 `_ => Object`，`Object != Function`）| checker 扩臂 group → `Kind::FunctionType \| Kind::ConstructorType => SerializedTypeNode::Function` |

> 红→绿证据：每个切片**先观察 RED 再转绿**——Array/Function 端到端切片在加完 transformer arm（构建保持）但**未加 checker 臂**的窗口里，checker 返回 `Object` → transformer 发射 `Object`，两个观察点同时实测红（`Object != Array`/`Object != Function`），随后**单处 checker 臂**把两端翻绿。transformer arm 仅是穷尽-match 的构建占位（在 checker 发射新变体前不可达），非"提前实现"——翻绿的最小代码是 checker 臂。**无伪造红、无 green-on-arrival**。

**本轮交付**：
- `core/emit_resolver.rs`：`SerializedTypeNode` 加 `Array`/`Function` 两个变体（带 Go 锚 rustdoc）；`serialize_type_node_for_metadata` 加 `Kind::ArrayType | Kind::TupleType => Array`、`Kind::FunctionType | Kind::ConstructorType => Function` 两臂（Go 锚注释）；更新枚举/方法 rustdoc（移除 Function/Array DEFER，记 4av 落地）。
- `core/emit_resolver_test.rs`：+4 行为级测试（S1–S4）。
- `internal/transformers/tstransforms/legacydecorators.rs`：`serialized_type_to_expression` 加 `SerializedTypeNode::Array => new_identifier("Array")`、`SerializedTypeNode::Function => new_identifier("Function")` 两臂（同 lane，见 transformers 6am）。
- `internal/transformers/tstransforms/legacydecorators_test.rs`：+3 端到端测试（array/tuple/function 属性 → `design:type`）。

**测试增量**：checker 343 单测（+4：S1–S4，相对 4au 基线 339）+ 132 doctest（**±0**：枚举变体非 pub fn）；transformers 229 单测（+3，相对 6al 基线 226）+ 44 doctest（**±0**）。

**协调枚举变更保持 compiler 绿（实测）**：新增 `Array`/`Function` 变体在精神上是 additive，但**破坏穷尽 match**（这正是本 lane 同时拥有两 crate 的原因）。`cargo build -p tsgo_compiler` 绿——确认协调枚举变更跨 workspace 一致（transformers 的穷尽 match 现覆盖 `Array`/`Function`）。未改任何其它 pub fn 签名。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker` 绿（343 单测 + 132 doctest）；`cargo test -p tsgo_transformers` 绿（229 单测 + 44 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **`TypeReference`→entity 构造器**（`Date`/class）：落保守 `Object` tail。blocked-by: `get_type_reference_serialization_kind`（`resolveEntityName` value/type 解析 + `getDeclaredTypeOfSymbol` + `isTypeAssignableToKind` + `printer.TypeReferenceSerializationKind`）+ lib globals（P6）。
- **union/intersection/conditional 递归**（Go `serializeUnionOrIntersectionConstituents`）：落 `Object` tail。blocked-by: 全 `serializeTypeNode` 递归 + `strictNullChecks` 标志。
- **`TypePredicate`**（`x is T`→`Boolean`、`asserts`→`void 0`）：落 `Object` tail。blocked-by: `TypePredicateNode.AssertsModifier` 臂端口。
- **方法装饰器 `design:type=Function`（硬编码，无 checker）/ `design:paramtypes` / `design:returntype`**：本轮只覆盖**属性**装饰器的 `design:type`。blocked-by: method/accessor 装饰形态 + `serializeParameterTypesOfNode`/`serializeReturnTypeOfNode`（transformers 维度，见 6am DEFER）。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/decorators/`（`emitDecoratorMetadata` 的 `design:type`：数组 `: number[]`/元组 `: [A,B]` → `Array`、函数 `: () => void`/构造 `: new () => C` → `Function`）。

**推荐下一轮**：扩 checker `get_type_reference_serialization_kind`，解锁 `TypeReference`（`Date`/class）→ entity 构造器（当前两 lane 的共同头号 DEFER）；次选方法/参数装饰器维度的 `design:paramtypes`/`design:returntype`（transformers 6am DEFER）。

## 4aw 落地记录（worklog 摘要）—— EmitResolver `get_type_reference_serialization_kind`（`TypeReference` 实体 value-ness 分类原语，可达单文件子集）

**目标**：落地 4at/4au/4av 共同的头号 DEFER——`serialize_type_node_for_metadata` 的 `TypeReference` 臂所消费的分类原语 `get_type_reference_serialization_kind`，按是否引用一个运行时 value（装饰器 `design:type` 为 `: SomeClass` 发射类标识符）对一个 `TypeReference` 类型节点分类。在 `EmitResolver` 上落地一个**加法式** pub 方法 + 一个**加法式** pub 枚举 `TypeReferenceSerializationKind`（12 变体 1:1 镜像 Go `printer.TypeReferenceSerializationKind` 的 iota：`Unknown`/`TypeWithConstructSignatureAndValue`/`VoidNullableOrNeverType`/`NumberLikeType`/`BigIntLikeType`/`StringLikeType`/`BooleanType`/`ArrayLikeType`/`ESSymbolType`/`Promise`/`TypeWithCallSignature`/`ObjectType`）。逐行为红→绿，一次一个，可达单文件子集（无 lib globals）。

**Go 真值（ground truth）**：`internal/checker/emitresolver.go:EmitResolver.GetTypeReferenceSerializationKind(1139)`（枚举：`internal/printer/emitresolver.go:TypeReferenceSerializationKind`）。结构：① `typeName == nil` → `Unknown`；② 以 **Value** meaning `resolveEntityName(typeName, Value, ...)` 取 `valueSymbol`、以 **Type** meaning 取 `typeSymbol`（各经 `resolveAlias` 解 alias）；③ `if resolvedValueSymbol != nil && resolvedValueSymbol == resolvedTypeSymbol`：先查 `getGlobalPromiseConstructorSymbol()`（命中 → `Promise`），再 `constructorType = getTypeOfSymbol(resolvedValueSymbol)`，`isConstructorType(constructorType)` → `isTypeOnly ? TypeWithCallSignature : TypeWithConstructSignatureAndValue`；④ `if resolvedTypeSymbol == nil` → `isTypeOnly ? ObjectType : Unknown`；⑤ `type_ = getDeclaredTypeOfSymbol(resolvedTypeSymbol)`，`isErrorType` → `isTypeOnly ? ObjectType : Unknown`；⑥ `isTypeAssignableToKind` 链（`AnyOrUnknown`→`ObjectType`、`Void|Nullable|Never`→`VoidNullableOrNeverType`、`BooleanLike`→`BooleanType`、`NumberLike`→`NumberLikeType`、`BigIntLike`→`BigIntLikeType`、`StringLike`→`StringLikeType`、`isTupleType`→`ArrayLikeType`、`ESSymbolLike`→`ESSymbolType`、`isFunctionType`→`TypeWithCallSignature`、`isArrayType`→`ArrayLikeType`）→ `else` tail `ObjectType`。

**为何放 `EmitResolver` + 取 `&mut Checker`（与 Go 的偏离说明）**：同 4an–4at 的加法式 EmitResolver 风格。方法签名 `get_type_reference_serialization_kind(&self, checker: &mut Checker, program: &dyn BoundProgram, type_node: NodeId)`——需 `&mut Checker` 以 `get_declared_type_of_symbol`（同 `serialize_type_of_declaration`）。Go 取 `(typeName, location)`，本港取 `type_node`（`TypeReference` 节点）并从中抽 `type_name`（Go `serializeTypeReferenceNode` 传 `node.TypeName`）；scope 起点用 `type_name` 自身（顶层引用经 `resolve_name` 走父链到 source file + globals 等价于 Go 的 serialScope，DEFER 类内 name-scope 漂移）。

**可达性裁剪（faithful-but-reachable）**：`isConstructorType(getTypeOfSymbol(valueSymbol))` 当前不可达——Rust `get_type_of_symbol` 对 class 返回*实例*类型（非静态/构造器侧，与 Go 偏离），且构造签名尚未收集（见 `declared_types.rs` DEFER）。故用**可达 stand-in**：解析到的 value 符号带 `SymbolFlags::CLASS`（单文件中唯一的运行时构造器源，无需 lib globals）。⑥ 的 `isTypeAssignableToKind` 链与 tuple/function/array 全 DEFER（需 resolved global lib types），可达解析-type 落 Go `else` tail `ObjectType`。alias 解析、`isTypeOnly` split、`serialScope`、qualified-name 实体亦 DEFER。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| S1 tracer（genuine RED）| `type_reference_to_local_class_is_construct_signature_and_value` | `class C {}` + `declare const x: C;`：`: C` → **`TypeWithConstructSignatureAndValue`**（红：方法桩返回 `Unknown`，`Unknown != TypeWithConstructSignatureAndValue` 实测断言失败）| 建枚举 `TypeReferenceSerializationKind` + 方法桩（返 `Unknown`）→ 抽 `type_name` + identifier 守卫 + Value/Type `resolve_name` + `value==type && CLASS => TypeWithConstructSignatureAndValue` 臂；tail 仍 `Unknown` |
| S2（genuine RED）| `type_reference_to_interface_is_object_type` | `interface I {}` + `declare const x: I;`：仅-type 引用 → **`ObjectType`**（红：S1 后落 `Unknown` tail，`Unknown != ObjectType`）| 加 `let Some(type_symbol) = type_symbol else { Unknown }` + `get_declared_type_of_symbol` + `isErrorType → Unknown` + `else ObjectType` tail（Go ⑥ kind 链全 DEFER 落 ObjectType）|
| S3 守卫（green-on-arrival）| `type_reference_to_type_alias_is_object_type` | `type T = {};` + `declare const x: T;` → **`ObjectType`**（**无新臂**：Go 对 interface 与 type-alias 经 `getDeclaredTypeOfSymbol`→`else` tail 同样分类；S2 实现已覆盖）| 无 |
| S4 守卫（green-on-arrival）| `type_reference_to_unresolved_name_is_unknown` | `declare const x: Missing;` → **`Unknown`**（**无新臂**：无声明的名 value+type 解析皆失败 → `resolvedTypeSymbol == nil` tail；S2 的 `let Some(..) else { Unknown }` 已覆盖）| 无 |

> 红→绿证据：S1/S2 均为 genuine RED（S1 方法桩返 `Unknown`，S2 前序仅 class 臂使 interface 落 `Unknown` tail；实测断言 `Unknown != <期望>` 失败）→ 最小臂转绿。S3/S4 是 S2 结构的自然结果（绿-on-arrival 覆盖守卫，**非伪造红**，同 4at S8 口径）——如实记录。

**本轮交付（全部 `core/emit_resolver.rs` + `lib.rs` re-export）**：
- 新增 pub 枚举 `TypeReferenceSerializationKind`（12 变体 + rustdoc `# Examples`）+ pub 方法 `EmitResolver::get_type_reference_serialization_kind`（加法式）。`lib.rs` re-export 加 `TypeReferenceSerializationKind`。新增 `use` 引入 `get_declared_type_of_symbol`。无新依赖。
- `core/emit_resolver_test.rs`：+4 行为级测试（S1–S4，经 `StubProgram` parse+bind 驱动）。

**新公开 API 形状（供后续 P5/P6 transformer 消费）**：
```rust
pub enum TypeReferenceSerializationKind {
    Unknown, TypeWithConstructSignatureAndValue, VoidNullableOrNeverType,
    NumberLikeType, BigIntLikeType, StringLikeType, BooleanType, ArrayLikeType,
    ESSymbolType, Promise, TypeWithCallSignature, ObjectType,
}

impl EmitResolver {
    pub fn get_type_reference_serialization_kind(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        type_node: NodeId,   // a `TypeReference` type node (`: SomeName`)
    ) -> TypeReferenceSerializationKind;
}
```
> 后续协调 transformer 轮：`serializeTypeReferenceNode` 据本分类，对 `TypeWithConstructSignatureAndValue` 发射 `serializeEntityNameAsExpression(node.TypeName)`（即类标识符本身）作 `design:type`；其余 kind 发射 `Object`/`void 0`/`Number`/`String`/… 标识符。

**测试增量**：347 单测（+4：S1–S4，相对 4av 基线 343）+ 134 doctest（+2：新 pub 枚举 + 新 pub 方法各一个 `# Examples`）。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名——新增物只有 `EmitResolver` 上一个 pub 方法 + 一个 pub 枚举（均已 re-export）。新加枚举无下游穷尽 match（不同于 4av 的 `SerializedTypeNode`），故纯加法、不破坏构建。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker` 绿（347 单测 + 134 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **lib-globals 类**（`Promise` via `getGlobalPromiseConstructorSymbol`；`isTypeAssignableToKind` 链 → `VoidNullableOrNeverType`/`NumberLikeType`/`BigIntLikeType`/`StringLikeType`/`BooleanType`/`ESSymbolType`；tuple `isTupleType`→`ArrayLikeType`、`isFunctionType`→`TypeWithCallSignature`、`isArrayType`→`ArrayLikeType`）：可达解析-type 落保守 `ObjectType` tail。blocked-by: resolved global lib types + 完整构造/调用签名收集（P6）。
- **`isConstructorType` 真身**（构造签名收集 + type-variable/mixin 分支）：本轮以 value 符号 `CLASS` flag 作可达 stand-in。blocked-by: class 静态侧/构造签名收集（`getTypeOfFuncClassEnumModule` class 分支）。
- **alias 解析 + type-only split**（`resolveAlias`、`getTypeOnlyAliasDeclarationEx`、`isTypeOnly ? TypeWithCallSignature/ObjectType`）：本轮直接比较解析符号、不解 alias、不分 type-only。blocked-by: 跨模块 alias 解析（`exports.go`）+ type-only meaning（`markLinkedReferences`）。
- **qualified-name 实体**（`A.B`：`IsQualifiedName`/`GetFirstIdentifier` 根 value-symbol type-only 检查 + namespace 成员解析）+ **`serialScope` 独立起点**（类内 name-scope 漂移）：本轮仅 identifier 实体、scope 起点用 `type_name` 自身。blocked-by: qualified-name `resolveEntityName` + namespace 成员解析。
- **transformer 消费**（`serializeTypeReferenceNode` 对 `TypeWithConstructSignatureAndValue` 发射 `serializeEntityNameAsExpression`（类标识符）作 `design:type`）：~~是 P5/P6 协调 transformer 轮的活~~ **✅ 已由 round 6an/4ax 落地**（见 transformers `impl.md` 6an worklog）：transformer `serialize_type_reference_node` 消费本方法，对 `TypeWithConstructSignatureAndValue` 发射类标识符、`ObjectType`/`Unknown` 发 `Object`。`&mut Checker` 线程经 `EmitReferenceResolver` 内持 `Rc<RefCell<Checker>>` 解（4aw 签名/checker 代码本轮**零改动**——纯被消费）。`class C {}\nclass D { @dec x: C; }`+emitDecoratorMetadata → `__metadata("design:type", C)`。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/decorators/`（`emitDecoratorMetadata` 的 `design:type`：类-typed 成员 `: SomeClass` → 类标识符本身；interface/type-alias-typed `: I`/`: T` → `Object`）。

**推荐下一轮（协调 checker+transformers）**：**P5/P6 协调跨-crate lane**——transformer `serialize_type_node_for_metadata`（或其消费方）的 `TypeReference` 臂改为调本轮 `get_type_reference_serialization_kind`，对 `TypeWithConstructSignatureAndValue` 发射 entity 标识符（`@dec x: SomeClass;` → `__metadata("design:type", SomeClass)`），其余 kind 发射对应构造器/`Object`/`void 0`。次选 checker 扩 lib-globals 类（待 P6 lib globals 就绪后解锁 `Promise`/`NumberLikeType`/`ArrayLikeType` 等）。

## 4ay 落地记录（worklog 摘要）—— `getNonNullableType` union 抽 null/undefined + 非空断言 `x!` + 真值 narrowing 抽 nullable（消费 4al/4am `strict_null_checks()`）

**目标**：消费 4al/4am 落地的 `c.strict_null_checks()` getter，落地三件互相串联的 strictNullChecks 语义：(1) `getNonNullableType` 在 union 上抽 `null`/`undefined`（strict 下），(2) 非空断言表达式 `x!` 的类型 = `getNonNullableType(typeof x)`，(3) 真值 narrowing（`if (x)`）在 truthy 分支抽 nullable。逐行为红→绿，一次一个，可达子集。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。

**Go 真值（ground truth）**：
- `internal/checker/checker.go:GetNonNullableType(18526)`：`if c.strictNullChecks { return c.getAdjustedTypeWithFacts(t, TypeFactsNEUndefinedOrNull) }; return t`。`getAdjustedTypeWithFacts(t, NEUndefinedOrNull)`(30834) 核心是 `getTypeWithFacts(t, NEUndefinedOrNull)` = `filterType` 保留带 `NEUndefinedOrNull` fact 的成员（`undefined`/`null`/`void` 三种 reachable kind 缺该 fact → 被抽掉），随后 `mapType` 把残留的 `EQUndefinedOrNull`（instantiable）成员映射成 `getGlobalNonNullableTypeInstantiation`（DEFER）。
- `internal/checker/checker.go:checkNonNullAssertion(10582)`：非 optional-chain 路径 = `GetNonNullableType(checkExpression(node.Expression()))`；`checkExpressionWorker` 的 `KindNonNullExpression` 臂(7759) 派发到它。
- `internal/checker/flow.go:narrowTypeByTruthiness(421)`：匹配引用时 `getAdjustedTypeWithFacts(t, IfElse(assumeTrue, Truthy, Falsy))`——truthy 抽掉只-falsy 的 `undefined`/`null`。

**可达裁剪（faithful-but-reachable）**：`get_non_null_type` 落 `getTypeWithFacts(t, NEUndefinedOrNull)` 的可达核心——按 `TypeFlags::NULLABLE | VOID`（=`UNDEFINED|NULL|VOID`，对应 Go 三种缺 `NEUndefinedOrNull` fact 的 reachable kind）过滤 union 成员，strict 下 reduce、非 strict 恒等。`unknown`→`{} | null | undefined` 的 `recombineUnknownType`/`unknownUnionType` 重组、残留 instantiable 成员的 `mapType`→`getGlobalNonNullableTypeInstantiation`（`NonNullable<T>` 全局 alias / 与 `{}` 相交）全 DEFER。`x!` 只做非-chain 路径（`a?.b!` 的 `checkNonNullChain` DEFER）。`get_non_null_type` 取 `pub(crate)`（内部）——`x!` 臂与行为单测均在 crate 内消费，无需扩 pub API。**未改 `get_type_facts`**（仍 TRUTHY/FALSY 子集，避免破坏既有 doctest）——`get_non_null_type` 自带按 flag 的 nullable 判定，不依赖全 `TypeFacts` lattice（全量 lattice 仍 DEFER）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1a tracer（genuine RED）| `get_non_null_type_strict_removes_undefined` | 建 union `string \| undefined`，`get_non_null_type` → **`type_to_string=="string"`**（红：identity 桩返回 union，实测 `"undefined \| string" != "string"`）| `type_facts.rs` 加 `get_non_null_type`：从 identity 桩改为按 `NULLABLE\|VOID` 过滤 + `get_union_type` |
| 1b（green-on-arrival 守卫）| `get_non_null_type_strict_removes_null` | `string \| null` → `"string"`（**无新臂**：同一过滤已抽 `null`）| 无 |
| 1c（green-on-arrival 守卫）| `get_non_null_type_strict_removes_null_and_undefined` | `string \| null \| undefined` → `"string"`（**无新臂**）| 无 |
| 1d（genuine RED）| `get_non_null_type_non_strict_is_identity` | `--strictNullChecks false` 下 `string \| undefined` → **恒等（同一 TypeId）**（红：无 gate 的过滤把它 reduce 成 `string`，`TypeId(7) != TypeId(22)`）| `get_non_null_type` 顶部加 `if !self.strict_null_checks() { return t; }` |
| 2（genuine RED）| `non_null_assertion_strips_undefined_then_reports_2322_against_number` | `declare const x: string \| undefined;\nvar n: number = x!;` → **1 个 2322「Type 'string' is not assignable to type 'number'.」**（红：无 `NonNullExpression` 臂，`x!` 落 `_ => error_type` → 0 诊断）| `check.rs` `check_expression` 加 `Kind::NonNullExpression => self.check_non_null_assertion(...)` + 新私有 `check_non_null_assertion`（`get_non_null_type(check_expression(operand))`）|
| 2-contrast（baseline）| `plain_nullable_reference_reports_2322_with_union_source` | 同句去掉 `!`：`var n: number = x;` → 1 个 2322「Type 'undefined \| string' is not assignable to type 'number'.」（源是整 union，与 2 的 `string` 对照出 `!` 的效果）| 无（既有 4am union-source 路径）|
| 2-guard | `non_null_assertion_assignable_to_string_target` | `var s: string = x!;` → **0 诊断**（`x!` 已 reduce 成 `string`，可赋给 `string`）| 无 |
| 3（真值 narrowing 抽 nullable，green-on-arrival 守卫）| `truthy_branch_narrows_out_nullable` | `if (x) {\n  var n: number = x;\n}`（`x: string \| undefined`）→ 1 个 2322「Type 'string' is not assignable to type 'number'.」（truthy 分支把 `x` narrow 成 `string`）| 无（既有 4t flow 遍历 + 4g `narrow_type_by_truthiness`→`get_type_with_facts(Truthy)` 已抽 `undefined`）|

> 红→绿证据：1a/1d/2 **均为 genuine RED**（1a identity 桩 → `"undefined \| string" != "string"`；1d 无-gate 过滤把非 strict 也 reduce → `TypeId` 不等；2 无 `NonNullExpression` 臂 → 0 诊断 ≠ 1）→ 最小触点转绿。1b/1c/3 **green-on-arrival 守卫**（同一过滤已抽 null；真值 narrowing 早在 4t/4g 落地，本轮仅确认 nullable 形态可观察）——**如实记录非伪造红**（同 4aw S3/S4 口径）。

**本轮交付**：
- `core/type_facts.rs`：新增 `pub(crate)` 方法 `get_non_null_type`（strict-gated；按 `NULLABLE|VOID` 过滤 union；带 Go 锚 rustdoc + DEFER 注）。
- `core/check.rs`：`check_expression` 加 `Kind::NonNullExpression` 臂 + 新私有 `check_non_null_assertion`（非-chain 路径 = `get_non_null_type(check_expression(operand))`）。
- `core/type_facts_test.rs`：+4 行为单测（1a/1b/1c/1d）+ 引入 `StubProgram`/`type_to_string` use。
- `core/check_test.rs`：+4 行为单测（2 / 2-contrast / 2-guard / 3）。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。**

**新公开 API 形状**：本轮**无新 pub 项**——`get_non_null_type` 取 `pub(crate)`（内部），`x!` 臂走既有 `check_expression`。公开 API 形状与 4aw 完全一致（仅加法式*内部*方法 + `check_expression` 多覆盖一个 Kind）。

**测试增量**：355 单测（+8：1a/1b/1c/1d + 2/2-contrast/2-guard/3，相对 4aw 基线 347）+ 134 doctest（**±0**：`get_non_null_type` 为 `pub(crate)`、无 `# Examples` 代码块，不计 doctest）。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名，未加任何 pub 项；`get_non_null_type` 内部、`NonNullExpression` 臂内部。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker` 绿（355 单测 + 134 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **`??` nullish 精化走 strict 分支**（`hasTypeFacts(EQUndefinedOrNull)` + `GetNonNullableType` 的 `checkNullishCoalesceOperands`）：blocked-by: `EQUndefinedOrNull` 全量 `TypeFacts` fact 位（本轮 `get_type_facts` 仍 TRUTHY/FALSY 子集）。
- **全 `TypeFacts` narrowing 矩阵**（typeof EQ/NE 每名、EQ/NE undefined/null、各 strict/非-strict primitive fact 组）：本轮 `get_non_null_type` 用按-flag 的 nullable 判定绕过，不扩 `get_type_facts`。blocked-by: 全 fact lattice 端口（会改 `get_type_facts` 返回，须同步既有 doctest/单测期望）。
- **`getAdjustedTypeWithFacts` 的 `unknown` 重组 + instantiable `mapType`**（`recombineUnknownType`/`unknownUnionType`；残留 `EQUndefinedOrNull` 成员→`getGlobalNonNullableTypeInstantiation`=`NonNullable<T>` 全局 alias / 与 `{}` 相交）：本轮落纯成员过滤。blocked-by: `unknownUnionType` + `{}` 空对象 / `NonNullable<T>` 全局 alias（lib globals，P6）。
- **`exactOptionalPropertyTypes`**：未接线。blocked-by: 选项消费端 + 可选属性 `undefined` 注入。
- **`x!` 的 optional-chain 形态 `a?.b!`**（Go `checkNonNullChain`：剥 optional marker→non-null→重挂 marker）：本轮只非-chain 路径。blocked-by: optional-chain 表达式类型化 + optional-type marker。
- **非-union `getNonNullableType`**（apparent-type / generic 约束的 nullable 抽取）：超出可达最小，未做。blocked-by: `getBaseConstraintOfType` + apparent-type 包装。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/compiler/strictNullChecks*` / `tests/cases/conformance/expressions/optionalChaining/` 里非空断言 `x!`（`declare const x: string | undefined; const s: string = x!;`）+ `tests/cases/conformance/controlFlow/` 的 `if (x)` 真值 narrow 抽 nullable 的最基础单变量形态（无 `??`、无 optional chain、无判别属性）。

**推荐下一轮（4az）**：(a) `??`/`??=` 的 nullish 结果精化走 strict 分支（需 `EQUndefinedOrNull` fact + 本轮 `get_non_null_type`）——先扩 `get_type_facts` 加 EQ/NE undefined/null fact 位（同步既有期望）；(b) `x === null`/`x !== undefined` 的 flow 相等 narrowing 走 `NEUndefined`/`EQUndefined` facts（`narrowTypeByEquality` 的 nullable 分支）；(c) `checkNonNullType`/非空断言上的 `2533`「Object is possibly null/undefined」诊断（消费端报错形态）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4az 落地记录（worklog 摘要）—— EQ/NE-nullable `TypeFacts` 位 + 相等 flow narrowing（`narrowTypeByEquality` nullable 分支）+ 属性/元素访问的 `2531`/`2532`/`2533`（实达 `18047`/`18048`/`18049`）possibly-null/undefined 诊断 + `undefined` 值标识符解析 + 类型位 `null` 字面量

**目标**：在 4ay（`get_non_null_type` + `x!`）+ 4al/4am（`strict_null_checks()` getter）之上，落地三件串联的 strictNullChecks 语义，逐行为红→绿：(1) 扩 `get_type_facts` 加 Go 的 EQ/NE/Is `undefined`/`null` fact 位（从旧 TRUTHY/FALSY 子集，同步既有期望）；(2) 相等 flow narrowing 的 nullable 分支（`x !== undefined`→`string`、`x === undefined`→`undefined`、`null` 镜像、loose `== null`→`null | undefined`）；(3) 属性/元素访问上的 possibly-null/undefined 诊断（`checkNonNullType`/`reportObjectPossiblyNullOrUndefinedError`）。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:TypeFacts(398)` 位定义：`EQUndefined=1<<16`、`EQNull=1<<17`、`EQUndefinedOrNull=1<<18`、`NEUndefined=1<<19`、`NENull=1<<20`、`NEUndefinedOrNull=1<<21`、`Truthy=1<<22`、`Falsy=1<<23`、`IsUndefined=1<<24`、`IsNull=1<<25`。本港**精确镜像位号**（typeof 位 `1<<0..1<<15` 仍不建模——typeof narrowing 走关系引擎而非 facts）。fact 组：`string`(StrictFacts)=`NE*|Truthy|Falsy`（非 strict 加 `EQ*|Falsy`）；`UndefinedFacts`=`EQUndefined|EQUndefinedOrNull|NENull|Falsy|IsUndefined`；`NullFacts`=`EQNull|EQUndefinedOrNull|NEUndefined|Falsy|IsNull`；`VoidFacts`=`EQUndefined|EQUndefinedOrNull|NENull|Falsy`。
- `internal/checker/flow.go:narrowTypeByEquality(549)`：`!=`/`!==` 先翻 `assumeTrue`；`valueType.flags&Nullable!=0` 时（strict 下）按 `doubleEquals`→`EQUndefinedOrNull`/`NEUndefinedOrNull`、否则按 `valueType` 是否 `null`→`EQNull`/`NENull` 或 `EQUndefined`/`NEUndefined`，调 `getAdjustedTypeWithFacts`（可达核心 = `getTypeWithFacts`）。
- `internal/checker/checker.go:checkNonNullType(7377)/checkNonNullTypeWithReporter(7381)`：`facts := getTypeFacts(t, IsUndefinedOrNull)`；非 0 则 `reportObjectPossiblyNullOrUndefinedError` 后 `GetNonNullableType(t)`（残留 nullable/never→errorType）。`checkPropertyAccessExpression(11201)`/`checkIndexedAccess` 经 `checkNonNullExpression` 类型化对象。
- `internal/checker/checker.go:reportObjectPossiblyNullOrUndefinedError(7424)`：entity-name 对象（`IsEntityNameExpression`，`len<100`）用 `_0_is_possibly_*`（**18047/18048/18049**）；否则 `Object_is_possibly_*`（2531/2532/2533）。**确认 Go 实达码**：本轮的对象都是标识符 `x`（entity name），故实测 Go 发 **18047/18048/18049**（任务文案的 2531/2532/2533 仅在对象为**非** entity-name 时触发——同一函数的 `else` 臂，本港已忠实端口两路）。`null` 关键字 / `undefined` 标识符值另发 `The_value_0_cannot_be_used_here`。
- `internal/checker/checker.go:NewChecker`（`undefinedSymbol` 949/1339/1456）：全局 `undefined` 值符号，类型 `undefinedWideningType`。
- `internal/checker/checker.go:getTypeFromLiteralTypeNode(22781)`：`LiteralType` 的 `NullKeyword`→`nullType`。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| A tracer（genuine RED）| `property_access_on_possibly_undefined_reports_18048` | `declare const x: { a: number } \| undefined;\nx.a;` → **1×18048「'x' is possibly 'undefined'.」**（红：无 non-null 检查，union 缺共有 `a`→落 `2339`）| 扩 `TypeFacts`（EQ/NE/Is/Truthy/Falsy，Go 位号）+ 重写 `get_type_facts`（忠实 per-kind，strict-aware，union OR-fold）；`check.rs` 加 `check_non_null_expression`/`check_non_null_type`/`report_object_possibly_null_or_undefined_error` + `is_entity_name_expression`/`entity_name_to_string`；`check_property_access`/`check_element_access` 改走 `check_non_null_expression` |
| A-facts-sync | `type_facts_of_primitives_and_literals`（既有）+ type_facts.rs doctest | `string`/`undefined`/`null` 的 facts 从 TRUTHY/FALSY 子集 → 全 EQ/NE/Is 子集（同步期望，保持绿）| 同上 |
| A-guard（genuine RED：类型位 `null`）| `property_access_on_possibly_null_reports_18047` | `{ a: number } \| null` → **1×18047「'x' is possibly 'null'.」**（红：类型位 `null` 落 `error_type`→`{a}\|error`，无 IS_NULL fact→落 `2339`）| `declared_types.rs` 加 `Kind::LiteralType`→`get_type_from_literal_type_node`（`NullKeyword`→`null_type`）|
| A-guard（genuine RED）| `property_access_on_possibly_null_or_undefined_reports_18049` | `{ a: number } \| null \| undefined` → **1×18049「'x' is possibly 'null' or 'undefined'.」** | 无（A + 类型位 null 落地后两 fact 齐备）|
| A-guard（green-on-arrival）| `element_access_on_possibly_undefined_reports_18048` | `x["a"]`（`{a}\|undefined`）→ 1×18048 | 无（`check_element_access` 同走 `check_non_null_expression`）|
| A-guard | `property_access_on_non_nullable_object_reports_nothing` | `{ a: number }` → 0 诊断（无 Is fact，`check_non_null_type` 恒等）| 无 |
| B（genuine RED）| `undefined_value_resolves_without_cannot_find_name` / `undefined_value_checks_as_undefined_type` | `undefined;` → **0 诊断** / `check_expression`→`undefined` 类型（红：`undefined` 不解析→`2304`「Cannot find name 'undefined'」/ `error_type`(TypeId 3)≠`undefined`(TypeId 5)）| `check_identifier` 的 `None` 臂加 `if name=="undefined" { return self.undefined_type(); }`（Go `undefinedSymbol`）|
| C tracer（genuine RED）| `flow_equality_loose_null_keeps_both_nullables` | `string \| null \| undefined` 在 `if (x == null)` 真分支 → **`null \| undefined`**（红：旧 `equality_overlap` 只留精确 `null`，TypeId(6)≠TypeId(23)）| `flow.rs` 加 `narrow_type_by_equality_to_value`（nullable 分支：按 `double_equals`/`value_is_null`/`assume_true` 选 `EQ/NE*` fact → `get_type_with_facts`）+ `narrow_type_by_binary` 路由（nullable 值→新分支，否则旧 `narrow_type_by_equality`）|
| C-guard（green-on-arrival）| `flow_equality_ne_undefined_narrows_to_string` | `string \| undefined` + `x !== undefined` → `string`（`NEUndefined`）| 无（C 落地 + B 使 `undefined` 解析）|
| C-guard（green-on-arrival）| `flow_equality_eq_undefined_narrows_to_undefined` | `x === undefined` 真分支 → `undefined`（`EQUndefined`）| 无 |
| C-guard（green-on-arrival）| `flow_equality_ne_null_narrows_to_string` | `string \| null` + `x !== null` → `string`（`NENull`）| 无 |
| C-guard（green-on-arrival）| `flow_equality_eq_null_narrows_to_null` | `x === null` 真分支 → `null`（`EQNull`）| 无 |
| C 端到端（genuine RED 经 B+C）| `ne_undefined_branch_narrows_to_string_no_diagnostics` | `if (x !== undefined) {\n  var s: string = x;\n}` → **0 诊断**（红：B 前两次 `2304`；narrow 成 `string` 可赋 `string`）| 无（B+C）|
| C 端到端对照（baseline）| `plain_nullable_assigned_to_string_reports_2322` | 去 guard：`var s: string = x;` → 1×2322「Type 'undefined \| string' is not assignable to type 'string'.」（与上 0/1 对照出 narrowing 效果）| 无 |

> 红→绿证据：A / A-guard(18047) / B / C **均 genuine RED**（A：`2339`≠`18048`；18047：类型位 null→`error` 落 `2339`；B：`2304` / `error_type`≠`undefined`；C：`TypeId(6)`≠`TypeId(23)`）→ 最小触点转绿。18049 也 genuine RED（A + 类型位 null 落地后才齐备）。element-access / non-nullable / strict 镜像 / 端到端-0诊断 为 **green-on-arrival 守卫**（**如实记录非伪造红**，同 4ay 口径）。**踩坑修正**：`narrow_type_by_equality_to_value` 初版用 `flags.contains(TypeFlags::NULLABLE)`（要求 `NULL`+`UNDEFINED` 全位）误判 `null`/`undefined` 单一位类型不 nullable → 实测仍 `TypeId(6)`；改 `intersects` 后转绿（Go 是 `& Nullable != 0` = intersects）。

**本轮交付**：
- `core/type_facts.rs`：`TypeFacts` 扩 EQ/NE/Is/Truthy/Falsy 位（Go 位号，TRUTHY/FALSY 从 `1<<0/1<<1` 迁到 `1<<22/1<<23`）+ fact 组常量（`BASE_STRICT`/`BASE_NONSTRICT`/`UNDEFINED_FACTS`/`NULL_FACTS`/`VOID_FACTS`/`UNKNOWN_FACTS`）+ `primitive_facts` 辅助；重写 `get_type_facts`（忠实 per-kind，strict-aware，union OR-fold）。
- `core/check.rs`：新私有 `check_non_null_expression`/`check_non_null_type`/`report_object_possibly_null_or_undefined_error` + 自由私有 `is_entity_name_expression`/`entity_name_to_string`；`check_property_access`/`check_element_access` 改走 `check_non_null_expression`；`check_identifier` 的 `undefined` 解析。
- `core/flow.rs`：新私有 `narrow_type_by_equality_to_value`（nullable 相等 narrowing）+ `narrow_type_by_binary` 路由 + `use super::type_facts::TypeFacts`。
- `core/declared_types.rs`：`get_type_from_type_node` 加 `Kind::LiteralType` 臂 + 新私有 `get_type_from_literal_type_node`（`NullKeyword`→`null_type`，其余 DEFER）。
- 测试：`check_test.rs` +9（A×5 + B×2 + C 端到端×2）、`flow_test.rs` +5（C×5）、`type_facts_test.rs` 同步既有 1 条期望。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。**

**新公开 API 形状**：本轮**无新 pub 项**——所有新方法/函数均 `pub(crate)`/私有；`TypeFacts` 仅**加常量**（既有 `TRUTHY`/`FALSY` 仍存在，位号变更但符号不变，无数值断言依赖）。公开 API 形状与 4ay 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**369 单测**（+14，相对 4ay 基线 355）+ **134 doctest**（**±0**：新方法/函数均非 pub fn，`get_type_facts` doctest 改写但不新增）。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名，未加任何 pub 项。`get_type_facts` 返回值扩展（更多 fact 位）**只影响 crate 内既有期望**（`type_facts_test.rs` 1 条 + type_facts.rs doctest 已同步），下游消费者（`||`/`&&` 的 `has_type_facts(TRUTHY/FALSY)`、`get_type_with_facts(TRUTHY)`、`narrow_type_by_truthiness`）只用 TRUTHY/FALSY 掩码，行为不变。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker` 绿（369 单测 + 134 doctest）；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（lsproto lane 并发）。

**本轮 DEFER（带 blocked-by）**：
- **`??`/`??=` 的 nullish 结果精化走 strict 分支**（`hasTypeFacts(EQUndefinedOrNull)` + `GetNonNullableType` 的 `checkNullishCoalesceOperands`）：本轮已备 `EQUndefinedOrNull` fact 位，但 `??` 结果臂的 nullish 精化未接。blocked-by: `??` 臂消费 `EQUndefinedOrNull` + `checkNullishCoalesceOperands` 诊断端口。
- **`typeof`-guard 走 facts**（`narrowTypeByLiteralExpression`/`narrowTypeByTypeName` 的全 typeof EQ/NE 位）：本轮只建 EQ/NE-nullable，typeof narrowing 仍走关系引擎子集。blocked-by: typeof 位（`1<<0..1<<15`）建模 + lib `Function` 全局。
- **判别联合 narrowing**（`narrowTypeByDiscriminant`：`obj.kind === "a"`）：未做。blocked-by: 判别属性访问匹配 + `getDiscriminantPropertyAccess`。
- **全 `TypeFacts` 矩阵**（typeof EQ/NE 每名、intersection `getIntersectionTypeFacts`、instantiable `getBaseConstraintOfType`、object 的 empty/function 精化 `getTypeFactsWorker` object 臂）：本轮 object 用简化 `Truthy + NE*`、fallback 用 `UnknownFacts`。blocked-by: intersection/约束 facts + 空对象/函数对象判定。
- **call-expression nullable 接收者**（`o.m()` 中 `o` 可空的 `Cannot_invoke_an_object_which_is_possibly_*` 2721/2722/2723 与 `checkNonNullNonVoidType` 的 void 访问 `Object_is_possibly_undefined`）：未做。blocked-by: 调用接收者 non-null 检查 + void 路径诊断。
- **`reportObjectPossiblyNullOrUndefinedError` 的 `Object_is_possibly_*`（2531/2532/2533）臂**：已忠实端口但**本轮测试不可达**（对象均为 entity-name 标识符 `x`→走 18047/18048/18049）。需非-entity-name 对象（如 `(expr).a`、`arr[0].a`）触发，依赖括号/元素访问对象表达式类型化。blocked-by: 非-entity-name 对象表达式（`ParenthesizedExpression`/复合接收者）类型化。
- **`checkNonNullType` 的 `unknown`-操作数臂**（`Object_is_of_type_unknown`/`_0_is_of_type_unknown` 2571/18046）：DEFER。blocked-by: `unknown` entity-name 报错路径。
- **非-`null` 类型位字面量**（`"a"`/`1`/`true` 在类型位）：`get_type_from_literal_type_node` 只落 `NullKeyword`。blocked-by: 类型位 fresh/regular 字面量配对（`getRegularTypeOfLiteralType`）。
- **非 strict 的 possibly-null 抑制**：`check_non_null_type` gate 在 `strict_null_checks()`（任务要求）——Go 不 gate facts 部分而靠非 strict union 化简抑制；本港 gate 给同样可观察（非 strict 不报）。blocked-by: 非 strict union 化简忠实建模。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/controlFlow/`（`controlFlowDeleteOperator`/`narrowingByDiscriminantInUnionType` 等里 `x === null`/`x !== undefined` 单变量相等 narrowing 的最基础形态）+ `tests/cases/compiler/strictNullChecks*`（属性访问 possibly-undefined）。

**推荐下一轮（4ba）**：(a) `??`/`??=` 的 nullish 结果精化走 strict 分支（消费本轮 `EQUndefinedOrNull` fact + 4ay `get_non_null_type`）；(b) call-expression nullable 接收者 `o.m()` 的 2721/2722/2723 + `checkNonNullNonVoidType` void 访问；(c) `typeof` narrowing 走全 typeof facts 位（需 typeof `1<<0..1<<15` 建模）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4ba 落地记录（worklog 摘要）—— `??`/`??=` nullish 结果精化（`getNonNullableType(left) | right`）+ 调用接收者 possibly-null/undefined 诊断（`2721`/`2722`/`2723`）+ typeof narrowing 端到端见证

**目标**：在 4az（EQ/NE/Is undefined/null `TypeFacts` 位 + `get_type_facts`）+ 4ay（`get_non_null_type`）之上，落地三件可达的 strictNullChecks 语义，逐行为红→绿：(1) `??`/`??=` 结果精化为 `getNonNullableType(left) | right`（当 `hasTypeFacts(left, EQUndefinedOrNull)`）；(2) 调用接收者 `f()` 的 possibly-null/undefined 诊断（`reportCannotInvokePossiblyNullOrUndefinedError` → **2721/2722/2723**）；(3) `typeof` narrowing 端到端诊断见证（flow 层早在 4f/4az 落地）。单 lane（lsproto track 已完成）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:checkBinaryLikeExpression(12462)` 的 `KindQuestionQuestionToken`/`KindQuestionQuestionEqualsToken` 臂：`resultType := leftType`；`if hasTypeFacts(leftType, EQUndefinedOrNull) { resultType = getUnionTypeEx([GetNonNullableType(leftType), rightType], UnionReductionSubtype) }`；`??=` 额外 `checkAssignmentOperator(left, op, right, leftType, rightType)`；`??` 额外 `checkNullishCoalesceOperands`（grammar 混用 `||`/`&&` 检查，DEFER）。
- `internal/checker/checker.go:resolveCallExpression(8478)`：`funcType = checkNonNullTypeWithReporter(funcType, node.Expression(), reportCannotInvokePossiblyNullOrUndefinedError)`——callee 经 non-null 检查，**与属性访问用不同 reporter**。
- `internal/checker/checker.go:reportCannotInvokePossiblyNullOrUndefinedError(9854)`：`IsUndefined&&IsNull`→`Cannot_invoke_an_object_which_is_possibly_null_or_undefined`(**2723**)；`IsUndefined`→`..._undefined`(**2722**)；else→`..._null`(**2721**)。**确认**：本族**无** entity-name vs Object 分支（与 4az 的 18047/18048/18049 vs 2531/2532/2533 不同），消息恒定无 `'{0}'` 占位。
- `internal/checker/flow.go:narrowTypeByTypeof`/`narrowTypeByTypeName`：`typeof x === "string"` narrow（早在 4f 落地，`narrow_type_by_binary` 已路由 `typeof x === "name"` → `narrow_type_by_typeof`）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 tracer（genuine RED）| `nullish_coalesce_removes_undefined_assignable_to_string` | `declare const x: string \| undefined;\nvar s: string = x ?? "d";` → **0 诊断**（红：旧臂返回 raw `string \| undefined` → 1×2322「undefined \| string」不可赋 `string`）| `check.rs` `??` 臂：`has_type_facts(left, EQUndefinedOrNull)` 时 `get_union_dropping_never([get_non_null_type(left), right])` |
| 1 类型见证（green-on-arrival）| `nullish_coalesce_result_drops_nullable_facts` | `check_expression(x ?? "d")` → `!has_type_facts(_, IS_UNDEFINED_OR_NULL)` | 无（同臂）|
| 2 `??=` 共享精化（green-on-arrival）| `nullish_coalesce_assign_removes_undefined_assignable_to_string` | `declare let x: string \| undefined;\nvar s: string = x ??= "d";` → **0 诊断** | 无（`??`/`??=` 共享臂）|
| 3 tracer（genuine RED）| `call_on_possibly_undefined_callee_reports_2722` | `declare const f: (() => void) \| undefined;\nf();` → **1×2722**「Cannot invoke an object which is possibly 'undefined'.」（红：union callee 无调用签名 → 静默 `error`，0 诊断）| `check.rs` 加私有 enum `NonNullReporter{Access,Invocation}` + 把 `check_non_null_type` 抽成 `check_non_null_type_with_reporter(_,_,_,reporter)` 薄封装 + 新私有 `report_cannot_invoke_possibly_null_or_undefined_error`（2721/2722/2723）；`check_call_expression` callee 经 `check_non_null_type_with_reporter(_, Invocation)` |
| 3-guard（green-on-arrival）| `call_on_possibly_null_callee_reports_2721` | `(() => void) \| null` → 1×2721 | 无 |
| 3-guard（green-on-arrival）| `call_on_possibly_null_or_undefined_callee_reports_2723` | `(() => void) \| null \| undefined` → 1×2723 | 无 |
| 3-guard（属性访问族对照，green-on-arrival）| `call_on_property_access_possibly_undefined_reports_18048` | `declare const o: { m(): void } \| undefined;\no.m();` → **1×18048**「'o' is possibly 'undefined'.」（NOT 2722——接收者 `o` 在属性访问的 `checkNonNullExpression` 早于 callee 检查触发，确认 4az 路径已覆盖 `o.m`）| 无 |
| 3-guard（green-on-arrival）| `call_on_non_nullable_callee_reports_nothing` | `declare const f: () => void;\nf();` → 0 诊断 | 无 |
| 4 端到端见证（green-on-arrival）| `typeof_string_guard_narrows_var_assignment_no_diagnostics` | `declare const x: string \| number;\nif (typeof x === "string") {\n  var s: string = x;\n}` → **0 诊断**（flow 层 `flow_typeof_narrows_in_then_branch` 早已覆盖；本轮加诊断层见证）| 无（riding 既有 4f/4az flow 机器）|
| 4 对照（baseline）| `plain_string_or_number_assigned_to_string_reports_2322` | 去 guard：`var s: string = x;` → 1×2322「string \| number」（0/1 对照）| 无 |

> 红→绿证据：slice 1 / slice 3 **genuine RED**（1：1×2322≠0；3：callee 无签名 0≠1）→ 最小触点转绿。`??=`/2721/2723/`o.m()`/非空 callee/typeof 端到端 为 **green-on-arrival 守卫**（**如实记录非伪造红**，同 4ay/4az 口径）：`??=` 与 `??` 共享精化臂，故 slice-1 impl 落地即覆盖；typeof 端到端 riding 既有 flow narrowing（flow 层单测早绿）。

**本轮交付**：
- `core/check.rs`：`??`/`??=` 臂改为 nullish 结果精化（`has_type_facts(EQUndefinedOrNull)` → `get_union_dropping_never([get_non_null_type(left), right])`）；新私有 enum `NonNullReporter`；`check_non_null_type` → 薄封装到新私有 `check_non_null_type_with_reporter(_,_,_,reporter)`；新私有 `report_cannot_invoke_possibly_null_or_undefined_error`（2721/2722/2723）；`check_call_expression` callee 经 invocation reporter 的 non-null 检查。
- 测试：`check_test.rs` +10（slice1×2 + slice2×1 + slice3×5 + slice4×2）。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。**

**新公开 API 形状**：本轮**无新 pub 项**——`NonNullReporter` 私有 enum、`check_non_null_type_with_reporter`/`report_cannot_invoke_possibly_null_or_undefined_error` 私有方法、`??` 臂内部。公开 API 形状与 4az 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**379 单测**（+10，相对 4az 基线 369）+ **134 doctest**（**±0**：新方法/enum 均非 pub fn）。

**本轮 DEFER（带 blocked-by）**：
- **`??` 的 `UnionReductionSubtype` 结果化简**（Go `getUnionTypeEx(_, UnionReductionSubtype)`：`string | "d"` → `string`）：本港 `get_union_dropping_never` 不做 subtype 化简，结果留 `string | "d"`（可观察赋性不变）。blocked-by: 4b union subtype reduction。
- **`checkNullishCoalesceOperands` grammar 诊断**（`??` 与 `||`/`&&` 不加括号混用 → `5076`/`X_0_and_1_operations_cannot_be_mixed`）：未做。blocked-by: grammar mixed-operator 检查端口。
- **`??=` 的 LHS 写类型 + 赋值后 flow narrowing**：本轮 `??=` 复用 `check_assignment_operator(left, leftType, rightType)`（与 4p 一致），赋值后 `x` 的 flow 收窄未建。blocked-by: 复合赋值 setter 写类型 + assignment flow node narrowing。
- **`checkNonNullNonVoidType` 的 void 访问诊断**（`Object_is_possibly_undefined` for `void`）：未做。blocked-by: void-访问路径诊断。
- **call 接收者的 `silentNeverType`/optional-chain 形态**（`a?.b()`）：本轮只非-chain。blocked-by: optional-chain 调用类型化 + `silentNeverType`。
- **typeof narrowing 走全 typeof facts 位**（`1<<0..1<<15` 的 typeof EQ/NE 位 + lib `Function` 全局）：本轮 typeof 仍走关系引擎子集（string/number/boolean/bigint/symbol/undefined/object，4f 落地）。blocked-by: typeof facts 位建模 + lib globals（P6）。
- **判别联合 narrowing / instanceof 深化 / exactOptionalPropertyTypes**：未做（任务 DEFER 列表）。blocked-by: 判别属性访问匹配 / 构造签名 + 原型链 / 选项消费端。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/expressions/nullishCoalescingOperator/`（`??` 结果类型）+ `tests/cases/conformance/expressions/optionalChaining/`（possibly-undefined 调用 2722）+ `tests/cases/conformance/controlFlow/`（typeof narrowing 单变量）。

**推荐下一轮（4bb）**：(a) `??` 的 `UnionReductionSubtype` 结果化简（需 4b union subtype reduction）；(b) 判别联合 narrowing（`obj.kind === "a"` 的 `narrowTypeByDiscriminant` + `getDiscriminantPropertyAccess`）；(c) `checkNullishCoalesceOperands` grammar 混用诊断。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4bb 落地记录（worklog 摘要）—— 判别联合 narrowing（`obj.kind === "a"`）+ `??` 结果 `UnionReductionSubtype` 化简 + 混用运算符 grammar `5076`（外加前置：字面量类型节点解析 + 字面量关系按值相等）

**目标**：在 4ba（`??` 精化 + nullable facts）之上，落地 4bb 三件可达语义，逐行为红→绿。落地过程中发现两个**前置 blocker**（slice 1 拆为 1a/1b/1c），同样逐行为红→绿推进。单 lane（无其它 lane）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:getTypeFromLiteralTypeNode(22781)`：`NullKeyword` → `nullType`；否则 `getRegularTypeOfLiteralType(checkExpression(literal))`（字面量在类型位是**regular**而非 fresh）。
- `internal/checker/relater.go:isTypeRelatedTo`：`source == target` 经字面量 interning 已覆盖等值字面量；本港不 intern 字面量，故等值字面量是不同 id，需按 `(LITERAL flags, value)` 相等关联——复刻 Go 的 interning 可观察语义（确认码：`5076`、`2367`、`2339`、`2322` 均逐一核对）。
- `internal/checker/flow.go:narrowTypeByBinaryExpression(462)`：相等臂里 `left`/`right` 都不匹配 reference 时，`getDiscriminantPropertyAccess(f, left, t)`/`(f, right, t)` → 命中则 `narrowTypeByDiscriminantProperty(t, access, op, value, assumeTrue)`。
- `internal/checker/flow.go:getDiscriminantPropertyAccess(1408)`：`declaredType`/`computedType` 是 union 时，`getCandidateDiscriminantPropertyAccess`（access case：`isMatchingReference(reference, expr.Expression())`）+ `isDiscriminantProperty(t, name)` → 返回 access。
- `internal/checker/flow.go:narrowTypeByDiscriminantProperty(683)`：`getKeyPropertyName` 快路径仅 ≥10 成员触发（小 union 跳过）；否则 `narrowTypeByDiscriminant(t, access, t' => narrowTypeByEquality(t', op, value, assumeTrue))`。
- `internal/checker/flow.go:narrowTypeByDiscriminant(706)`：`propName = getAccessedPropertyName`；`propType = getTypeOfPropertyOfType(t, propName)`；`narrowedPropType = narrow(propType)`；`filterType(t, c => discType=getTypeOfPropertyOrIndexSignatureOfType(c,propName); discType≠never && narrowed≠never && areTypesComparable(narrowed, disc))`。
- `internal/checker/relater.go:isDiscriminantProperty(1084)`：union 上的 synthetic 属性，`CheckFlags & NonUniformAndLiteral == NonUniformAndLiteral`（`HasNonUniformType`=某成员属性类型≠首个；`HasLiteralType`=某成员属性是 `isLiteralType`）且 `!isGenericType`。
- `internal/checker/checker.go:checkBinaryLikeExpression(12468)` 的 `??` 臂：`resultType = getUnionTypeEx([GetNonNullableType(left), right], UnionReductionSubtype)`（当 `hasTypeFacts(left, EQUndefinedOrNull)`）；`??`（非 `??=`）额外先跑 `checkNullishCoalesceOperands(left, right)`。
- `internal/checker/checker.go:checkNullishCoalesceOperands(12859)`：`if IsBinaryExpression(left.Parent.Parent)`（=`??` 节点的 parent）→ grandparentLeft 是 binary 且 grandparentOp==`||` → `grammarErrorOnNode(grandparentLeft, 5076, "??","||")`；`else if IsBinaryExpression(left)` 且 op∈{`||`,`&&`} → `5076(left, op, "??")`；`else if IsBinaryExpression(right)` 且 op==`&&` → `5076(right, "??","&&")`。`a ?? b || c` 经 `COALESCE==LogicalOR` 同优先级左结合解析为 `(a ?? b) || c`（取分支 1）。`5076` 文案 `'{0}' and '{1}' operations cannot be mixed without parentheses.`（确认 code=5076）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1a tracer（genuine RED）| `string_literal_type_node_not_assignable_to_other_literal` | `declare const x: "a";\nconst n: "b" = x;` → **1×2322**「Type '"a"' is not assignable to type '"b"'.」（红：旧 `get_type_from_literal_type_node` 对非-`null` 字面量返回 `error_type`→赋值无诊断，0≠1）| `declared_types.rs`：`get_type_from_literal_type_node` 非-`null` 分支 → `regular_type_of_literal_type(check_expression(literal))`；`mod.rs`：`regular_type_of_literal_type` 提 `pub(crate)` |
| 1a 对照（green-on-arrival）| `string_literal_type_node_assignable_to_string` | `declare const x: "a";\nconst s: string = x;` → 0 诊断（`"a"` 是 `string` 子类型）| 无 |
| 1b tracer（genuine RED）| `equality_literal_in_its_union_reports_no_overlap_diagnostic` | `declare const k: "a" \| "b";\nk === "a";` → **0 诊断**（红：两个 `"a"` 是不同 id，关系按 id 比较 → 判 no-overlap → 误报 1×2367）| `relations.rs`：`is_type_related_to` 在 `source==target` 后加 `literals_equal_by_value`（同 `LITERAL` flags + 等值）|
| 1b 对照（green-on-arrival）| `equality_literal_outside_union_reports_no_overlap` | `k: "a" \| "b"` vs `"c"` → 1×2367「…'"a" \| "b"' and '"c"' have no overlap.」（真 no-overlap 不被抑制）| 无 |
| 1c tracer（genuine RED）| `discriminant_property_eq_narrows_union_in_then_branch` | `type A={kind:"a";x:number}; type B={kind:"b";y:string}; declare const v:A\|B; if(v.kind==="a"){const n:number=v.x;}` → **0 诊断**（红：未收窄 → `v.x` 在 `A\|B` 上不存在 → 1×2339）| `flow.rs`：`narrow_type_by_binary` 加判别分支 + `get_discriminant_property_access`/`get_accessed_property_name`/`is_discriminant_property`/`narrow_type_by_discriminant_property`/`types_same_literal_value`；`check.rs`：`are_types_comparable`/`is_literal_type` 提 `pub(crate)` |
| 1c 收窄见证（green-on-arrival）| `discriminant_narrowed_branch_rejects_other_constituent_property` | 真分支内 `v.y` → 1×2339「…does not exist on type '{ kind: "a"; x: number; }'.」（窄到 `A` 的消息，区别于全 union）| 无 |
| 1c 取反见证（green-on-arrival）| `discriminant_not_equal_narrows_to_complement_constituent` | `if(v.kind!=="a"){const s:string=v.y;}` → 0 诊断（窄到补集 `B`，`v.y` 存在）| 无 |
| 2 tracer（genuine RED）| `nullish_coalesce_result_is_subtype_reduced` | `declare const x:"a"\|undefined; declare const y:string; const n:number = x ?? y;` → **1×2322**「Type 'string' is not assignable to type 'number'.」（红：旧 `get_union_dropping_never` 不做 subtype 化简 → 结果 `string \| "a"`，消息源含字面量）| `check.rs` `??` 臂：`get_union_dropping_never([..])` → `get_union_type(subtype_reduce([non_null, right]))` |
| 3 tracer（genuine RED）| `nullish_coalesce_mixed_with_logical_or_reports_5076` | `a ?? b \|\| c;`（a/b/c:number）→ **1×5076**「'??' and '\|\|' operations cannot be mixed without parentheses.」（红：未实现 grammar 检查，0≠1）| `check.rs` `??` 臂（非 `??=`）调 `check_nullish_coalesce_operands(node,left,right)`；`grammar.rs` 新 `pub(crate)` 方法（3 分支 if/else-if 镜像 Go）|
| 3 分支2（green-on-arrival）| `logical_or_then_nullish_coalesce_reports_5076` | `a \|\| b ?? c;` → 1×5076「'\|\|' and '??' …」（`(a\|\|b) ?? c`，`??` 左是 `\|\|`）| 无 |
| 3 分支3（green-on-arrival）| `nullish_coalesce_with_logical_and_reports_5076` | `a ?? b && c;` → 1×5076「'??' and '&&' …」（`a ?? (b&&c)`，`??` 右是 `&&`）| 无 |
| 3 对照（green-on-arrival）| `parenthesized_nullish_coalesce_with_logical_or_reports_nothing` | `(a ?? b) \|\| c;` → 0 诊断（括号消歧，`??` grandparent 是 paren 非 binary）| 无 |

> 红→绿证据：1a/1b/1c/2/3 全部 **genuine RED**（各自实测红：1a 0≠1、1b 误报 2367、1c 1×2339、2 消息源差、3 0≠1）→ 最小触点转绿。其余为 **green-on-arrival 守卫**（如实记录非伪造红）：同臂/同函数其它分支落地即覆盖。

**本轮交付**：
- `core/declared_types.rs`：`get_type_from_literal_type_node` 解析 `"a"`/`1`/`true` 字面量类型节点（经 `check_expression` + `regular_type_of_literal_type`）。
- `core/relations.rs`：`is_type_related_to` 加等值字面量关联（`literals_equal_by_value`，复刻 Go interning）。
- `core/flow.rs`：`narrow_type_by_binary` 加判别属性分支；新私有 `get_discriminant_property_access`/`get_accessed_property_name`/`is_discriminant_property`/`types_same_literal_value`/`narrow_type_by_discriminant_property`。
- `core/check.rs`：`??` 臂 subtype 化简（`subtype_reduce`）+ 非 `??=` 调 grammar 检查；`are_types_comparable`/`is_literal_type` 提 `pub(crate)`。
- `core/mod.rs`：`regular_type_of_literal_type` 提 `pub(crate)`。
- `core/grammar.rs`：新 `pub(crate)` `check_nullish_coalesce_operands`（`5076`）。
- 测试：`check_test.rs` +12（1a×2 + 1b×2 + 1c×3 + 2×1 + 3×4）。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。** 仅把既有私有方法提 `pub(crate)`（additive 可见性，不入公开 API）。

**新公开 API 形状**：本轮**无新 pub 项**——全部新方法为私有/`pub(crate)`，`??` 臂内部。公开 API 形状与 4ba 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**391 单测**（+12，相对 4ba 基线 379）+ **134 doctest**（**±0**：新方法均非 pub fn / 为 `pub(crate)` 用 `//` 注释不挂 doctest）。

**本轮 DEFER（带 blocked-by）**：
- **字面量类型节点的负数 / bigint 形态**（`-1`/`1n` 在类型位，`PrefixUnaryExpression`/`BigIntLiteral` 操作数）+ `links.resolvedType` 记忆化：未做。blocked-by: 前缀一元/bigint 表达式类型化。
- **字面量 interning（按值唯一）**：本港 `new_literal_type` 不 intern，靠 `literals_equal_by_value`（关系层）+ `types_same_literal_value`（判别 non-uniform 层）+ `equality_overlap`（flow 层，4az 早落地）按值比较绕过。blocked-by: 全局 `getLiteralType` interning（影响 union dedup/`getRegularTypeOfLiteralType` 配对，跨多片，单独立项）。
- **`getDiscriminantPropertyAccess` 的 const-alias / 解构候选形态**（`const k = obj.kind` / `const { kind } = obj`）+ optional-chain containment 分支 + 用 `declaredType` 兜底（computedType 非 union 子集时）：未做。blocked-by: alias/binding-element reference 匹配。
- **`isDiscriminantProperty` 的 `!isGenericType` 排除 + `HasNeverType` 交互**：未做（可达子集字面量恒非 generic）。blocked-by: generic-type 检测。
- **`narrowTypeByDiscriminantProperty` 的 `getKeyPropertyName` 快路径**（≥10 成员 union 的 constituentMap）+ optional-chain/非空 `removeNullable` 调整 + `getTypeOfPropertyOrIndexSignatureOfType` 索引签名兜底：未做。blocked-by: key-property maps + optional chains + 索引签名属性类型。
- **`checkNullishCoalesceOperandLeft`（always-/never-nullish 操作数诊断 `This_expression_is_always_nullish`/`Right_operand_..._never_nullish`）**：未做。blocked-by: 语法 nullishness-semantics 分析。
- **instanceof narrowing 深化 / `in`-运算符判别 narrowing / exactOptionalPropertyTypes / 非字面量判别**：未做（任务 DEFER 列表）。blocked-by: 构造签名+原型链 / `in` 判别属性 / 选项消费端 / 非字面量判别属性类型。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/typeRelationships/typeGuards/`（`typeGuardsWithInstanceOfBySuperType`/`discriminantPropertyCheck` 等里 `obj.kind === "lit"` 判别 narrowing 的基础形态）+ `tests/cases/conformance/expressions/nullishCoalescingOperator/nullishCoalescingOperator_grammarErrors`（`5076` 混用）+ `types/literal/`（字面量类型节点）。

**推荐下一轮（4bc）**：(a) 字面量 interning（`getLiteralType` 按值唯一）——为 union dedup/discriminant non-uniform 提供 Go 一致的 id 语义，消除本轮三处按值绕过；(b) `getDiscriminantPropertyAccess` 的 const-alias / 解构候选（`const k = obj.kind` 别名）；(c) `checkNullishCoalesceOperandLeft` 的 always-/never-nullish 操作数诊断。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4bc 落地记录（worklog 摘要）—— 字面量类型按值 interning（`getStringLiteralType`/`getNumberLiteralType` 值键缓存）+ union dedup by id + 退役 relations 按值绕过（`literals_equal_by_value`）

**目标**：在 4bb（字面量类型节点 + 三处按值比较绕过）之上，落地 Go 的 `getLiteralType` 值唯一语义：等值字面量（处处的 `"a"`、`1`、`true`）intern 到**同一个 `TypeId`**，给 union dedup / discriminant 一致的 Go id 语义，并退役 4bb 在 relations 层的按值绕过 `literals_equal_by_value`（识别 `"a" === "a"` 改走 `source==target` 身份）。逐行为红→绿，一次一个。单 lane（无其它 lane）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:Checker.getStringLiteralType(25164)`：`t := c.stringLiteralTypes[value]; if t==nil { t = c.newLiteralType(StringLiteral, value, nil); c.stringLiteralTypes[value]=t }; return t`——per-checker `map[string]*Type` 值键缓存。
- `internal/checker/checker.go:Checker.getNumberLiteralType(25173)`：同构 `map[jsnum.Number]*Type`，但 **`NaN` 单独缓存到 `c.nanType`**（因 `NaN != NaN` 使 float map-key 永不命中）。
- `internal/checker/checker.go:checkExpressionWorker(7711/7717)`：`KindStringLiteral`→`getFreshTypeOfLiteralType(getStringLiteralType(text))`；`KindNumericLiteral`→`getFreshTypeOfLiteralType(getNumberLiteralType(jsnum.FromString(text)))`——**fresh** 套在 interned **regular** 之外（fresh/regular widening 本轮 DEFER）。
- `internal/checker/checker.go:NewChecker(615/926)`：`stringLiteralTypes`/`numberLiteralTypes` 在构造期 `make`；`trueType`/`falseType` 是构造期单例（布尔字面量天然 interned）。
- `internal/checker/checker.go:createTupleTargetType`：tuple `length` 成员用 `getNumberLiteralType`（与其它 `N` 同缓存）。
- `internal/checker/relater.go:Checker.isTypeRelatedTo`：`source==target` 经字面量 interning 已覆盖等值字面量身份——本港 4bc 后 string/number 也 intern（布尔早是单例），故身份检查直接覆盖 `"a" === "a"`，4bb 的 `literals_equal_by_value` 值 shim 退役。

**可达裁剪（faithful-but-reachable）**：`check_expression` 的 `StringLiteral`/`NumericLiteral` 臂从 `new_literal_type(..., None)`（每次新 id）改走新 `pub(crate)` `get_string_literal_type`/`get_number_literal_type`（值键缓存，返回 interned regular literal）。**fresh-vs-regular 配对（可变上下文的字面量 widening）DEFER**：本港 `check_expression` 返回 interned regular（其自身即 regular，`fresh_type=None`），不套 `getFreshTypeOfLiteralType` 的 fresh 包装——对本轮全部可观察行为（id 身份 / union dedup / 关系 / 判别）一致。number 键用 `f64::to_bits()`，但 **`NaN` 规约到单一键、`+0/-0` 规约到 `0`**（镜像 Go 的 `nanType` 单例 + float map-key 的 `0==-0`）。布尔已是构造期单例（`true_type`/`false_type`），无新缓存。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 tracer（genuine RED）| `string_literal_expressions_intern_to_one_type_id` | `"a";\n"a";` 两个 `check_expression` → **同一 `TypeId`**（红：`new_literal_type` 每次新 id，实测 `TypeId(22) != TypeId(23)`）| `mod.rs`：加 `string_literal_types` 缓存 + `pub(crate) get_string_literal_type`；`check.rs` `StringLiteral` 臂改走它 |
| 1 distinct（green-on-arrival）| `distinct_string_literal_values_get_distinct_type_ids` | `"a";\n"b";` → 两个不同 id（值键缓存不串）| 无 |
| 2 tracer（genuine RED）| `number_literal_expressions_intern_to_one_type_id` | `1;\n1;` → **同一 `TypeId`**（红：实测 `TypeId(22) != TypeId(23)`，临时回退 numeric 臂观察）| `mod.rs`：加 `number_literal_types` 缓存 + `number_literal_key`（NaN/±0 规约）+ `pub(crate) get_number_literal_type`；`check.rs` `NumericLiteral` 臂改走它 |
| 2 distinct（green-on-arrival）| `distinct_number_literal_values_get_distinct_type_ids` | `1;\n2;` → 两个不同 id | 无 |
| 3（green-on-arrival 守卫）| `boolean_literal_expressions_intern_to_one_type_id` | `true;\ntrue;\nfalse;` → 两 `true` 同 id、`true`≠`false`（布尔早是构造期单例 `true_type`/`false_type`）| 无 |
| 4（genuine RED）| `union_of_equal_string_literals_collapses_to_single_literal` | `declare const x: "a" \| "a";` → `type_to_string=="\"a\""` 且 `union_types().is_none()`（红：临时回退 string 臂观察，两 `"a"` 不同 id → union 留 `"a" \| "a"`）| 无（slice 1 interning 落地后 `get_union_type` 的 id-dedup 自动塌缩）|
| 5（faithfulness）| `get_number_literal_type_interns_by_value_with_nan_and_zero_canonicalization`（单测）| tuple `length` 字面量 + 一般 `N` 共缓存；NaN/±0 规约 | `declared_types.rs` `get_tuple_length_type` 改走 `get_number_literal_type`（Go `createTupleTargetType`）|
| 6（退役 shim，refactor-green）| `equality_literal_in_its_union_reports_no_overlap_diagnostic`（4bb 既有）| `declare const k: "a" \| "b";\nk === "a";` → 0 诊断，**经身份**（interning 后 union 成员 `"a"` 与操作数 `"a"` 同 id，`source==target` 命中）| `relations.rs`：删 `literals_equal_by_value` 调用 + 定义；`is_type_related_to` 保留 `regular_literal_type` 归一 + `source==target` 身份 |

> 红→绿证据：slice 1 / slice 2 / slice 4 **genuine RED**（1：`TypeId(22)!=TypeId(23)`；2：临时回退 numeric 臂见 `TypeId(22)!=TypeId(23)`；4：临时回退 string 臂见 `"a" \| "a" != "a"`）→ 最小触点转绿。slice 3 / distinct 守卫为 **green-on-arrival**（布尔早是单例；值键缓存天然分离不同值）——**如实记录非伪造红**（同 4ay/4az/4bb 口径）。slice 6 是 **refactor-绿**（interning 落地后 shim 变冗余；删后既有 2367-FP 测试经身份保持绿，relations 模块 13 测全绿）。

**本轮交付**：
- `core/mod.rs`：`Checker` 加 `string_literal_types: FxHashMap<String, TypeId>` / `number_literal_types: FxHashMap<u64, TypeId>` 字段 + 构造期 `default()` 初始化；新 `pub(crate)` `get_string_literal_type`/`get_number_literal_type`（值键 intern，`//` 行注释 + Go 锚）；新自由 fn `number_literal_key`（NaN/±0 规约）。
- `core/check.rs`：`check_expression` 的 `StringLiteral`/`NumericLiteral` 臂改走 interning（`get_string_literal_type`/`get_number_literal_type`）。
- `core/declared_types.rs`：`get_tuple_length_type` 改走 `get_number_literal_type`（faithful + 使全部程序驱动字面量 interned，令 shim 退役安全）。
- `core/relations.rs`：删 `literals_equal_by_value`（调用 + 定义）；`is_type_related_to` 头部改为 `regular_literal_type` 归一 + `source==target` 身份（注释更新说明 4bc interning 后 shim 退役）。
- 测试：`check_test.rs` +5（slice1×2 + slice2×2 + slice3×1）、`declared_types_test.rs` +1（slice4）、`mod_test.rs` +2（`get_string_literal_type`/`get_number_literal_type` 单测，含 NaN/±0 边界）。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。** 新缓存方法为 `pub(crate)`（additive 内部，不入公开 API）；删除的 `literals_equal_by_value` 是 4bb 新增的私有 fn（删私有不破 API）。

**新公开 API 形状**：本轮**无新 pub 项**——`get_string_literal_type`/`get_number_literal_type` 为 `pub(crate)`（内部），`check_expression` 臂内部改写。公开 API 形状与 4bb 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**399 单测**（+8，相对 4bb 基线 391）+ **134 doctest**（**±0**：新缓存方法为 `pub(crate)` 用 `//` 行注释、不挂 doctest；删除的私有 fn 无 doctest）。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名，未加任何 pub 项。interning 改变的是 `check_expression` 对字面量返回的 id（同值→同 id），下游消费者（关系/union/判别）行为只更 Go-faithful（身份替代按值）。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker`（`--lib` 399 单测 + `--doc` 134 doctest）绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（无其它 lane）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **fresh-vs-regular 字面量配对（可变上下文 widening）**：`check_expression` 返回 interned regular，不套 `getFreshTypeOfLiteralType` 的 fresh 包装；`const x = "a"`（widen 到 `string`）vs `const x: "a"`（保 literal）的 fresh/regular 区分未建。blocked-by: `getFreshTypeOfLiteralType` 的按-regular 缓存 fresh 单例 + widening 上下文（`getWidenedLiteralType`/可变 binding 推断）。
- **flow 层两处按值绕过仍保留**（`types_same_literal_value`@判别 non-uniform、`equality_overlap`@4az flow 相等）：程序驱动路径现已主要走 interned 身份（等值字面量同 id），但单测里经 `new_literal_type` 手工建的字面量仍是不同 id，故保留这两处作 safety net（任务 item 2 仅要求退役 **relations** 的 `literals_equal_by_value`，已完成）。blocked-by: 全面以 interning 取代——需把所有手工 `new_literal_type` 单测改走 `get_*_literal_type` 后才能安全删（跨多测试文件，单独立项）。
- **bigint 字面量 interning**（`getBigIntLiteralType` + `bigintLiteralTypes`）：`LiteralValue` 无 BigInt 变体，bigint 字面量表达式未类型化。blocked-by: `PseudoBigInt` 值类型 + bigint 字面量表达式类型化。
- **负数 / 一元字面量**（`-1` 在类型/值位经 `PrefixUnaryExpression`）：`check_expression` 未类型化前缀一元。blocked-by: 前缀一元表达式类型化（Go `getFreshTypeOfLiteralType(getNumberLiteralType(-jsnum.FromString(...)))`）。
- **`getDiscriminantPropertyAccess` 的 const-alias / 解构候选**（`const k = obj.kind; if (k==="a")`）：探查后**未落地**（需 alias/binding-element reference 匹配 + `getReferenceCandidate`），保持 DEFER。blocked-by: const-alias reference 匹配（`getCandidateDiscriminantPropertyAccess` 的 const-variable 分支）。
- **`getRegularTypeOfLiteralType` 的 union 分支记忆化**（`u.regularType = mapType(t, getRegularTypeOfLiteralType)`）：本港 `regular_type_of_literal_type` 只处理 freshable 单体，union 分支未建。blocked-by: union `regularType` 链 + `mapType`。
- **always-/never-nullish 操作数诊断 / 泛型判别排除**：未做（任务 DEFER 列表）。blocked-by: 语法 nullishness 分析 / generic-type 检测。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/literal/`（字面量类型 identity / union dedup）+ `tests/cases/conformance/types/typeRelationships/typeGuards/`（判别 narrowing 经 interned 字面量身份）+ `tests/cases/compiler/literalTypes*`（等值字面量 union 塌缩）。

**推荐下一轮（4bd）**：(a) fresh-vs-regular 字面量配对（`getFreshTypeOfLiteralType` 按-regular 缓存 fresh + `getWidenedLiteralType` 在 `const`/可变 binding 推断的 widening）——解锁 `let x = "a"` widen 到 `string` 的可观察行为；(b) `getDiscriminantPropertyAccess` 的 const-alias 候选（`const k = obj.kind`）；(c) bigint 字面量 interning（需 `LiteralValue::BigInt` + bigint 表达式类型化）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4bd 落地记录（worklog 摘要）—— fresh-vs-regular 字面量配对（`getFreshTypeOfLiteralType`）+ `getWidenedLiteralType` + `let`/`var` 未注解初始化器 widening（const-gated）

**目标**：在 4bc（值键 interning 的 regular 字面量）之上，落地 Go 的 fresh/regular 字面量配对 + widening：字面量**表达式**产出 FRESH 字面量（配对到 4bc 的 interned regular）；未注解 `let x = "a"` 的声明类型 widen 到 `string`（可变 binding），而 `const x = "a"` 保留字面量 `"a"`。逐行为红→绿，一次一个。单 lane（无其它 lane）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:Checker.getFreshTypeOfLiteralType(25146)`：freshable（`TypeFlagsFreshable`=`ENUM|LITERAL`）类型若 `d.freshType==nil` 则 `f := newLiteralType(t.flags, d.value, t)`（fresh 的 `regularType=t`）、`f.symbol=t.symbol`、`f.freshType=f`（自指）、`d.freshType=f`；返回 `d.freshType`。非 freshable 恒等。
- `internal/checker/checker.go:Checker.getRegularTypeOfLiteralType(25132)`：freshable → 返回 `regularType`（regular 自身的 `regularType` 即自己；fresh 的指回 regular）；union → `mapType`（DEFER）。
- `internal/checker/checker.go:isFreshLiteralType(25160)`：freshable 且 `freshType==t`（自指）。
- `internal/checker/checker.go:checkExpressionWorker(7711/7717)`：`KindStringLiteral`→`getFreshTypeOfLiteralType(getStringLiteralType(text))`；`KindNumericLiteral` 同构；布尔 `true`/`false` 直接返回构造期 `trueType`/`falseType`（本就是 fresh 单例）。
- `internal/checker/checker.go:Checker.getWidenedLiteralType(25346)`：`StringLiteral&fresh`→`stringType`；`NumberLiteral&fresh`→`numberType`；`BigIntLiteral&fresh`→`bigintType`；`BooleanLiteral&fresh`→`booleanType`；`EnumLike&fresh`→`getBaseTypeOfEnumLikeType`（DEFER）；`Union`→`mapType`（DEFER）；否则恒等。
- `internal/checker/checker.go:getTypeForVariableLikeDeclaration(16607)`：无注解且有初始化器 → `widenTypeInferredFromInitializer(decl, checkDeclarationInitializer(decl,...))`；`checkDeclarationInitializer(16656)` 核心 = `checkExpressionCached(initializer)`。
- `internal/checker/checker.go:widenTypeInferredFromInitializer(16741)`→`getWidenedLiteralTypeForInitializer(16756)`：`getCombinedNodeFlagsCached(decl)&NodeFlagsConstant!=0 || isDeclarationReadonly(decl)` → 返回 `t`（保留字面量）；否则 `getWidenedLiteralType(t)`。`NodeFlagsConstant`=`Const|Using`。
- `internal/checker/checker.go:getWidenedType(18214)`：仅对 `ObjectFlagsRequiresWidening` 的类型生效（object-literal/widening-type），对字面量是恒等——故本港只跑 `getWidenedLiteralType`、跳过 object-literal widening（DEFER）。
- `internal/checker/checker.go:checkVariableLikeDeclaration(5863)`：`t = getTypeOfSymbol(symbol)`；主声明且有初始化器 → `initializerType = checkExpressionCached(initializer)` + `checkTypeAssignableToAndOptionallyElaborate(initializerType, t, ...)`。**关键**：`checkExpressionCached` 记忆化使初始化器只检查一次、诊断只报一次。

**可达裁剪（faithful-but-reachable）**：`LiteralType` 已有 `fresh_type`/`regular_type` 字段（4bb 起），布尔早在构造期建好 fresh/regular 对。本轮新增 `get_fresh_type_of_literal_type`/`is_fresh_literal_type`/`get_widened_literal_type`（均 `pub(crate)`），`check_expression` 的 `StringLiteral`/`NumericLiteral` 臂包 fresh。未注解变量推断：`get_type_of_variable_or_property` 的 `None`（无注解）分支对 **VariableDeclaration + 有初始化器** 走 `check_expression(init)` → `getWidenedLiteralTypeForInitializer`（const-gated）；property/parameter 初始化器推断、binding-pattern、循环初始化器、circular-initializer 解析栈 DEFER。`getWidenedType` 的 object-literal widening 跳过（对字面量恒等）。`getWidenedLiteralType` 的 bigint/enum/union 臂 DEFER（无可达驱动）。

**记忆化偏离（port-divergence，必要）**：本港无 `checkExpressionCached`（表达式类型不缓存、`check_expression` 重跑会重报诊断）。Go 靠缓存使初始化器只检查/报一次。本港在 `check_variable_declaration` 改为：未注解声明的初始化器**已在 `get_type_of_symbol` 推断时检查并报诊断**，故不再二次 `check_expression`（仅在**有显式注解**时二次检查初始化器做赋值性校验）——可观察等价于 Go（初始化器内层诊断只报一次；无注解时无赋值性误报，因 `t` 是初始化器自身的 widened 类型，恒可赋）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 tracer（genuine RED）| `string_literal_expression_is_fresh_paired_to_interned_regular` | `"a";` 的 `check_expression` → **fresh（≠ interned regular）且 `regular_type_of_literal_type(fresh)==get_string_literal_type("a")`**（红：旧返回 interned regular 自身，`TypeId(22)==TypeId(22)`）| `mod.rs` 新 `pub(crate) get_fresh_type_of_literal_type`/`is_fresh_literal_type`；`check.rs` `StringLiteral`/`NumericLiteral` 臂包 `get_fresh_type_of_literal_type(get_*_literal_type(...))` |
| 1 guard（green-on-arrival）| `fresh_string_literal_expressions_still_intern_to_one_type_id` | `"a";\n"a";` → 同 id（fresh 缓存在 regular 上）| 无 |
| 2 tracer（genuine RED）| `let_binding_widens_string_literal_initializer_to_string` | `let x = "a";\nconst y: "a" = x;` → **1×2322「Type 'string' is not assignable to type '"a"'.」**（红：未注解→`any`，0≠1）| `mod.rs` 新 `pub(crate) get_widened_literal_type`（仅 string 臂）；`declared_types.rs` `get_type_of_variable_or_property` 无注解分支推断初始化器 + 新私有 `variable_declaration_initializer`/`get_widened_literal_type_for_initializer`/`combined_node_flags`；`check.rs` `check_variable_declaration` 改为「无注解不二次检查」（避免重报）|
| 2 guard（green-on-arrival）| `let_binding_widened_string_is_assignable_to_string` | `let x = "a";\nvar s: string = x;` → 0 诊断 | 无 |
| 2 guard（green-on-arrival）| `const_binding_keeps_string_literal_assignable_to_literal_target` | `const x = "a";\nconst y: "a" = x;` → 0 诊断（const 保留字面量）| 无 |
| 3a tracer（genuine RED）| `let_binding_widens_number_literal_initializer_to_number` | `let n = 1;\nconst m: 1 = n;` → **1×2322「Type 'number' is not assignable to type '1'.」**（红：slice 2 仅 string 臂，`1` 未 widen → 0≠1）| `mod.rs` `get_widened_literal_type` 加 number 臂 |
| 3a guard（green-on-arrival）| `let_binding_widened_number_is_assignable_to_number` | `let n = 1;\nvar x: number = n;` → 0 诊断 | 无 |
| 3b tracer（genuine RED）| `let_binding_widens_boolean_literal_initializer_to_boolean` | `let b = true;\nconst c: true = b;` → **1×2322「Type 'false \| true' is not assignable to type 'true'.」**（红：无 boolean 臂，`true` 未 widen → 0≠1）| `mod.rs` `get_widened_literal_type` 加 boolean 臂 |
| 3b guard（green-on-arrival）| `let_binding_widened_boolean_is_assignable_to_boolean` | `let b = true;\nvar x: boolean = b;` → 0 诊断 | 无 |
| 3b guard（green-on-arrival）| `const_binding_keeps_boolean_literal_assignable_to_literal_target` | `const b = true;\nconst c: true = b;` → 0 诊断 | 无 |

> 红→绿证据：slice 1 / 2 / 3a / 3b **均 genuine RED**（1：`TypeId(22)==TypeId(22)`；2：未注解→`any` 0≠1；3a：仅 string 臂 number 未 widen 0≠1；3b：仅 string+number 臂 boolean 未 widen 0≠1）→ 最小触点转绿。guard 为 **green-on-arrival**（fresh 缓存天然分享 id；widened 可赋基元；const 保留字面量经身份）——**如实记录非伪造红**（同 4bc 口径）。**踩坑修正**：slice 2 初版在 `get_type_of_variable_or_property` 加初始化器检查后，函数/箭头表达式初始化器被检查两次（推断 1 次 + `check_variable_declaration` 1 次）→ 4 个既有 return-type 测试报双诊断（2≠1）；改为「无注解不二次检查」（镜像 Go `checkExpressionCached` 记忆化）后转绿。**踩坑修正 2**：boolean widening 源类型 `boolean`（=`false \| true` union）印为 `'false \| true'`——`false\|true`⇒`boolean` 的 `typeToString` 塌缩（Go `formatUnionTypes`）DEFER 到 4j 印刷器（与既有 `check_element_access_boolean_index_reports_2538` 同口径），故 expected 取 `'false \| true'`。

**本轮交付**：
- `core/mod.rs`：新 `pub(crate)` `get_fresh_type_of_literal_type`（创建并链接 fresh/regular 对，`fresh.symbol=t.symbol`）、`is_fresh_literal_type`、`get_widened_literal_type`（string/number/boolean fresh 臂）。
- `core/check.rs`：`check_expression` 的 `StringLiteral`/`NumericLiteral` 臂包 `get_fresh_type_of_literal_type`；`check_variable_declaration` 改为「无注解声明不二次 `check_expression`」（避免重报，镜像 Go 记忆化）。
- `core/declared_types.rs`：`get_type_of_variable_or_property` 无注解 + VariableDeclaration + 有初始化器 → 推断 widened 类型；新私有 `variable_declaration_initializer`/`get_widened_literal_type_for_initializer`（const-gated）/`combined_node_flags`（`getCombinedNodeFlags`：node→VariableDeclarationList→VariableStatement OR-fold）；`use tsgo_ast::NodeFlags`。
- 测试：`check_test.rs` +10（slice1×2 + slice2×3 + slice3a×2 + slice3b×3）。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。** 新方法均 `pub(crate)`/私有（additive 内部，不入公开 API）。

**新公开 API 形状**：本轮**无新 pub 项**——`get_fresh_type_of_literal_type`/`is_fresh_literal_type`/`get_widened_literal_type` 为 `pub(crate)`，`check_expression` 臂内部改写，var-decl 推断为私有自由 fn。公开 API 形状与 4bc 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**409 单测**（+10，相对 4bc 基线 399）+ **134 doctest**（**±0**：新方法均 `pub(crate)` 用 `//` 行注释、不挂 doctest）。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名，未加任何 pub 项。`check_expression` 对字面量改返回 fresh（下游关系/union/flow 均经 `regular_*` 归一或按值比较，行为只更 Go-faithful）；未注解变量类型从 `any` 变为推断 widened 类型（下游赋值性更精确，无既有测试回归）。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker`（`--lib` 409 单测 + `--doc` 134 doctest）绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（无其它 lane）。未 `git commit`。

**flow 按值绕过 shim：保留（非 clean 退役）**：4bc 已退役 relations 层 `literals_equal_by_value`。flow 层两处按值 shim（`equality_overlap`@4az 相等 narrowing、`types_same_literal_value`@判别 non-uniform）**本轮保留**。原因：本轮引入 fresh 后，**值位**字面量是 fresh、**类型/属性位**字面量是 regular（不同 id），`equality_overlap` 靠比较 `literal_value()`（fresh/regular 同值）正确关联，是必需的；更关键的是 `flow_test.rs` 多处用 `new_literal_type` 手工建字面量（不经 interning，等值仍异 id），直接依赖按值比较 —— 退役需先把这些手工单测迁到 `get_*_literal_type` interning（跨多测试文件，单独立项）。故按任务「retire if clean, else keep + note」判定为**非 clean → 保留 + 记录**。blocked-by: 手工 `new_literal_type` flow 单测迁移到 interning。

**本轮 DEFER（带 blocked-by）**：
- **`getWidenedLiteralType` 的 bigint / enum-like / union 臂**：bigint 无可达字面量表达式（item 4 见下）；enum-like 需 `getBaseTypeOfEnumLikeType`；union 需 `mapType(getWidenedLiteralType)`。blocked-by: bigint 字面量类型化 + enum 基类型 + union `mapType`。
- **bigint 字面量 interning（item 4）**：探查后**未落地**——`LiteralValue` 无 `BigInt` 变体，bigint 字面量表达式（`1n`）未被 `check_expression` 类型化（`BigIntLiteral` 臂缺失），且 `getBigIntLiteralType` 用 `jsnum.PseudoBigInt` 值键。落地需先建 `PseudoBigInt` 值类型 + `LiteralValue::BigInt` + bigint 字面量表达式臂，跨多触点，干净裁不出最小可达 RED。保持 DEFER。blocked-by: `PseudoBigInt` 值类型 + bigint 字面量表达式类型化。
- **contextual/return 位 widening**（`getWidenedLiteralLikeTypeForContextualType`/`isLiteralOfContextualType`）：未做。blocked-by: 上下文类型传递 + `isLiteralOfContextualType`。
- **`as const`（`getRegularTypeOfLiteralType` 的 const-assertion 抑制 widening）**：未做。blocked-by: `TypeAssertion`/`as const` 表达式类型化 + freshness 保留。
- **property/parameter 初始化器推断 + binding-pattern + 循环初始化器 + circular-initializer 解析栈（`pushTypeResolution`）**：本轮仅 VariableDeclaration identifier。blocked-by: 类成员/参数推断 + binding-element 类型化 + 循环解析栈。
- **`getWidenedType` 的 object-literal/widening-type 通道**（`getWidenedTypeOfObjectLiteral` + `RequiresWidening`/`CONTAINS_WIDENING_TYPE`）：本轮仅 `getWidenedLiteralType`（对字面量已足）。blocked-by: object-literal 类型构造 + widening-type 标记。
- **`isDeclarationReadonly`（readonly property / parameter property 保留字面量）**：本轮 const-gate 仅读 `NodeFlags::CONSTANT`（const/using）。blocked-by: readonly 修饰符解析。
- **`getRegularTypeOfLiteralType` 的 union 分支记忆化**（`u.regularType = mapType`）：本港仍只处理 freshable 单体（4bc 既有 DEFER）。blocked-by: union `regularType` 链 + `mapType`。
- **const-alias 判别候选 / 负数一元字面量 / typeof 全位 / always-nullish 操作数诊断**：未做（任务 DEFER 列表 + 4bc 既有 DEFER）。blocked-by: alias reference 匹配 / 前缀一元类型化 / typeof 位建模 / nullishness 分析。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/literal/`（fresh/regular 字面量 widening）+ `tests/cases/compiler/`（`let`/`var` vs `const` 字面量推断：`let x = "a"` → `string`、`const x = "a"` → `"a"`）+ `tests/cases/conformance/types/typeWidening/`（字面量 widening 基础形态）。

**推荐下一轮（4be）**：(a) `as const`（const-assertion 抑制 widening + freshness 保留，`getRegularTypeOfLiteralType` 的 const-context 分支）——解锁 `const x = "a" as const` 保 literal 与数组/对象 readonly 化的起步；(b) bigint 字面量 interning（需 `LiteralValue::BigInt` + `PseudoBigInt` + `BigIntLiteral` 表达式臂 + `getBigIntLiteralType`/`getWidenedLiteralType` bigint 臂）；(c) contextual-type 位的字面量保留（`isLiteralOfContextualType`/`getWidenedLiteralLikeTypeForContextualType`，须上下文类型传递）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4be 落地记录（worklog 摘要）—— `as const` const-assertion（抑制 widening + 保留 freshness-stripped 字面量）+ 非-const `as T` 结果类型（`AsExpression` 臂首次接线）

**目标**：在 4bd（fresh/regular 配对 + `getWidenedLiteralType` + const-gated 初始化器 widening）之上，落地 `as const`：`let x = "a" as const` 保留字面量 `"a"`（const 断言抑制 4bd 的可变-binding widening）；非-const `as T` 取被断言类型 `T` 为结果。逐行为红→绿，一次一个。单 lane（无其它 lane）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:Checker.checkAssertion(12238)`：`exprType := checkExpressionEx(node.Expression())`；若 `isConstTypeReference(typeNode)` → （`isValidConstAssertionArgument` 校验，DEFER）`return getRegularTypeOfLiteralType(exprType)`；否则 `checkSourceElement(typeNode)`+`checkNodeDeferred`+`return getTypeFromTypeNode(typeNode)`。
- `internal/checker/checker.go:Checker.getRegularTypeOfLiteralType(25132)`：freshable → `regularType`（fresh 指回 regular，regular 自身即自己）；union → `mapType`（DEFER）。
- `internal/checker/checker.go:Checker.getWidenedLiteralType(25346)`：仅对 `isFreshLiteralType(t)` 的字面量 widen——故 `as const` 归一出的 **regular** 字面量在 4bd 的 `getWidenedLiteralTypeForInitializer`(`let`/`var`) 里**恒等保留**（这正是 `as const` 抑制 widening 的机制：不是特判 const-binding，而是把值变成非-fresh）。
- `internal/checker/utilities.go:isConstTypeReference(128)` / `internal/ast/utilities.go:IsConstTypeReference(2439)`：`TypeReference` 且无 type-arguments 且 `TypeName` 是 identifier 且 text=="const"。
- 解析侧（既有，未改）：`"a" as const` 的 `as` 经 parser `make_as_expression` → `AsExpression{ expression, type_node }`；`const` 在类型位经 `parse_non_array_type` 的 `_ => parse_type_reference` → `parse_entity_name(allow_reserved_words=true)` 把 `KindConstKeyword` 建成 identifier text "const" 的 `TypeReference`（故 `is_const_type_reference` 命中）。

**可达裁剪（faithful-but-reachable）**：`check_expression` 新增 `Kind::AsExpression => check_assertion`（首次接线 `as`）。`check_assertion`：type 操作数 → 若 `is_const_type_reference(type_node)` → `regular_type_of_literal_type(expr_type)`（复用 4bb/4bd 既有 `pub(crate)`）；否则 `get_type_from_type_node(type_node, globals)`。**DEFER**：`isValidConstAssertionargument` 无效参数诊断（可达子集全是合法字面量参数，无诊断）、`checkAssertionDeferred` 的 `2352` 可比性检查 + `checkSourceElement(typeNode)` 深检、`<T>expr`（`TypeAssertionExpression`）形、`erasableSyntaxOnly` grammar。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 tracer（genuine RED）| `const_assertion_on_string_literal_keeps_literal_type` | `let x = "a" as const;\nconst y: "b" = x;` → **1×2322「Type '"a"' is not assignable to type '"b"'.」**（红：`as const` 未类型化→`error`（ANY，可赋任意），0≠1）| `check.rs` 新 `check_expression` `AsExpression` 臂 → 新私有 `check_assertion`（const 臂 `regular_type_of_literal_type`）+ 新私有 `is_const_type_reference` |
| 1 guard（green-on-arrival）| `const_assertion_on_string_literal_is_assignable_to_same_literal` | `let x = "a" as const;\nconst y: "a" = x;` → 0 诊断（经典 `as const` 保字面量；对照 4bd 同形 widen 报 2322）| 无 |
| 1 guard（green-on-arrival）| `const_assertion_on_string_literal_is_assignable_to_string` | `let x = "a" as const;\nvar s: string = x;` → 0 诊断（字面量可赋基元）| 无 |
| 2（green-on-arrival）| `const_assertion_on_number_literal_keeps_literal_type` | `let n = 1 as const;\nconst m: 2 = n;` → **1×2322「Type '1' is not assignable to type '2'.」**（const 臂 value-kind 无关，slice 1 后天然泛化）| 无 |
| 2 guard（green-on-arrival）| `const_assertion_on_number_literal_is_assignable_to_same_literal` | `let n = 1 as const;\nconst m: 1 = n;` → 0 诊断 | 无 |
| 3（green-on-arrival）| `const_assertion_on_boolean_literal_keeps_literal_type` | `let b = true as const;\nconst c: false = b;` → **1×2322「Type 'true' is not assignable to type 'false'.」**（`trueType` 构造期 fresh，const 臂归一到 regular `true`）| 无 |
| 3 guard（green-on-arrival）| `const_assertion_on_boolean_literal_is_assignable_to_same_literal` | `let b = true as const;\nconst c: true = b;` → 0 诊断 | 无 |
| 4 tracer（genuine RED）| `non_const_assertion_takes_asserted_type` | `let x = "a" as string;\nconst y: "a" = x;` → **1×2322「Type 'string' is not assignable to type '"a"'.」**（红：非-const 臂返回 `error`，0≠1；`"a"` 可比 `string` 故无 DEFER 的 2352）| `check.rs` `check_assertion` 非-const 臂 `get_type_from_type_node(type_node, program.globals())` |
| 4 guard（green-on-arrival）| `non_const_assertion_to_matching_type_is_assignable` | `let x = "a" as string;\nvar s: string = x;` → 0 诊断 | 无 |

> 红→绿证据：slice 1 / slice 4 **均 genuine RED**（1：`as const` 未类型化→`error`（ANY）可赋任意，0≠1；4：非-const 臂返回 `error`，0≠1）→ 最小触点转绿。slice 2（number）/ slice 3（boolean）为 **green-on-arrival 守卫**：const 臂经 `getRegularTypeOfLiteralType` 对任意 freshable 字面量统一归一（值-kind 无关），slice 1 落地后天然泛化（布尔 fresh/regular 对构造期已建）——**如实记录非伪造红**（同 4bc/4bd 口径）。

**4be 与 4bd widening 的交互（关键洞察）**：`as const` 抑制 widening 的机制**不是**给 var-decl widening 路径加 const-context 特判，而是 `checkAssertion` 把表达式类型从 **fresh** 字面量归一为 **regular** 字面量（`getRegularTypeOfLiteralType`）。4bd 的 `getWidenedLiteralType` 只 widen `isFreshLiteralType` 的类型，对 regular 字面量恒等——故 `let x = "a" as const` 的 regular `"a"` 流经 `getWidenedLiteralTypeForInitializer`(`let`，非 const) → `getWidenedLiteralType`(regular `"a"`) → **恒等返回 `"a"`**。这与 Go 完全一致，且**零改** 4bd 的 widening 路径（`get_type_of_variable_or_property` / `get_widened_literal_type_for_initializer` 未动）。

**本轮交付**：
- `core/check.rs`：`check_expression` 新增 `Kind::AsExpression => self.check_assertion(...)`；新私有 `check_assertion`（const 臂 `regular_type_of_literal_type`；非-const 臂 `get_type_from_type_node`）；新私有自由 fn `is_const_type_reference`（`TypeReference` + 无 type-args + identifier text "const"）。
- 测试：`check_test.rs` +9（slice1×3 + slice2×2 + slice3×2 + slice4×2）。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。** 新方法均私有；复用既有 `pub(crate) regular_type_of_literal_type`（4bb）/ `get_type_from_type_node`（4c free fn）。

**新公开 API 形状**：本轮**无新 pub 项**——`check_assertion`/`is_const_type_reference` 为私有，`check_expression` 臂内部改写。公开 API 形状与 4bd 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**418 单测**（+9，相对 4bd 基线 409）+ **134 doctest**（**±0**：新方法均私有/`pub(crate)`，不挂 doctest）。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名，未加任何 pub 项。`check_expression` 对 `AsExpression` 从 `error_type` 改为 const→regular-literal / 非-const→被断言类型（下游赋值性更精确，无既有测试回归）。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker`（`--lib` 418 单测 + `--doc` 134 doctest）绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（无其它 lane）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **`[1,2] as const` / `{x:1} as const`（readonly 元组/对象）**：探查后 DEFER——`check_expression` 无 `ArrayLiteralExpression`/`ObjectLiteralExpression` 臂（二者产 `error_type`），故 `[…] as const` 经 `regular_type_of_literal_type(error)` 恒等返回 error，裁不出干净可达的 readonly 元组/对象。blocked-by: 数组/对象字面量表达式类型化（4ae 元组类型节点已有，但**字面量表达式**未类型化）+ readonly 冻结（`getRegularTypeOfObjectLiteral` + tuple `readonly` 化 + `isValidConstAssertionArgument` 的 array/object 臂）。
- **`isValidConstAssertionArgument` 无效参数诊断**（`A const assertion can only be applied to ...`）：可达子集（string/number/boolean 字面量）全是合法参数，无诊断。blocked-by: 非字面量 `as const` 参数 + 诊断接线。
- **`checkAssertionDeferred` 的 `2352` 可比性检查**（`Conversion of type ... may be a mistake ...`）+ `checkSourceElement(typeNode)` 深检：未做（任务 DEFER 列表）。blocked-by: `isTypeComparableTo` + `getWidenedType` object-literal + deferred-node 检查队列。
- **`<T>expr` 类型断言形（`TypeAssertionExpression`）**：未做（本轮仅 `AsExpression`）。blocked-by: `TypeAssertionExpression` 臂（与 `AsExpression` 共用 `checkAssertion`，但 `shouldCheckErasableSyntax` 分支不同）。
- **`getRegularTypeOfLiteralType` 的 union 分支记忆化**（`u.regularType = mapType`）：4bc/4bd 既有 DEFER，本轮 const 断言只触及 freshable 单体字面量。blocked-by: union `regularType` 链 + `mapType`。
- **contextual-type 位 const（`isConstTypeVariable` 经 contextual type）+ 嵌套 const-context 透传**（`isConstContext` 的 paren/array/spread/property-assignment 递归）：未做（可达子集仅直接 `<literal> as const`）。blocked-by: 上下文类型传递 + 对象/数组字面量 const-context 递归。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/literal/constAssertions*` + `tests/cases/conformance/expressions/asOperator/`（`as const` 字面量保留 + 非-const `as T` 结果类型最基础形态——无 readonly 元组/对象、无 2352 可比性、无 `<T>expr`）。

**推荐下一轮（4bf）**：(a) `[1,2] as const` / `{x:1} as const` 的 readonly 元组/对象——需先给 `check_expression` 接 `ArrayLiteralExpression`/`ObjectLiteralExpression`（字面量表达式类型化）再做 const 冻结（`getRegularTypeOfObjectLiteral` + tuple readonly）；(b) `<T>expr`（`TypeAssertionExpression`）臂 + `checkAssertionDeferred` 的 `2352` 可比性（须 `isTypeComparableTo` + `getWidenedType`）；(c) bigint 字面量 interning（`LiteralValue::BigInt` + `PseudoBigInt` + `BigIntLiteral` 表达式臂）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4bf 落地记录（worklog 摘要）—— 对象字面量 / 数组字面量**表达式**类型化（`checkObjectLiteral` / `checkArrayLiteral` 可达子集）

**目标**：补齐 `check_expression` 的长期地基缺口——`{ x: 1 }`（`ObjectLiteralExpression`）与 `[1, 2]`（`ArrayLiteralExpression`）此前一律返回 `error_type`。本轮把二者类型化为 Go 一致的可达形态：对象字面量 → 带属性符号的匿名对象类型；数组字面量 → `Array<T>` 引用（元素 widen union）。逐行为红→绿，一次一个。单 lane（无其它 lane）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**建立在以下既有基建之上**：
- 4x **瞬态/合成符号 arena**（`new_synthesized_property` + `set_synthesized_symbol_resolved_type` + `is_synthesized_symbol`）：本轮新增 `new_object_literal_property`——直接置 `resolvedType` 的对象字面量属性符号（不走 union/intersection 的惰性合并；`containingType` 槽对它无意义，置成员类型自身作无害有效占位）。
- 4a/4v **类型 arena**（`new_object_type` / `ObjectType.members`+`properties` / `ObjectFlags`）：对象字面量类型直接 `types.alloc(OBJECT, Anonymous|ObjectLiteral|FreshLiteral|ContainsObjectOrArrayLiteral, symbol, Object{members,properties})`。
- 4ad/4ae **`Array<T>` 引用 + 数字索引签名实例化**：数组字面量经 `create_type_reference(globalArrayType, [elementType])`（名字解析 `Array`，与 `get_type_from_array_type_node` 同 lib stand-in），`arr[0]` 经 4ad 数字索引签名实例化到元素类型。
- 4bc/4bd **fresh/regular 配对 + `getWidenedLiteralType`**：`checkExpressionForMutableLocation` 在对象成员值 / 数组元素位 widen fresh 字面量（`1`→`number`，`"x"`→`string`），非-const/无上下文路径。
- 4u/4w **关系引擎结构化对象比较**（`properties_related_to`）：对象/数组字面量赋值性（2322）天然解锁——合成成员被 `get_properties_of_type` 迭代、`get_type_of_symbol` 经 `is_synthesized_symbol` 读 `resolvedType`。

**可达裁剪（faithful-but-reachable）**：
- `check_expression` 新增 `Kind::ObjectLiteralExpression => check_object_literal` 与 `Kind::ArrayLiteralExpression => check_array_literal`。
- `check_object_literal`：仅 `PropertyAssignment`（非计算名）成员；`name: value` → `check_property_assignment`（`checkExpressionForMutableLocation`）→ `new_object_literal_property(name, Property, t)` → 入 `members`/`properties`。对象类型旗标取 Go `createObjectLiteralType` 的 `Anonymous|ObjectLiteral|FreshLiteral|ContainsObjectOrArrayLiteral`。
- `check_array_literal`：每元素 `checkExpressionForMutableLocation` → 非空 `getUnionType` / 空 `strictNullChecks ? never : undefined` → `create_array_literal_type`（`create_type_reference(Array_target, [elementType])`）。
- node builder `serialize_members`：改为 `is_synthesized_symbol` 感知（瞬态成员名取 `synthesized_symbol_name`，否则 `program.symbol().name`）。**必要修复**：对象字面量类型成员是瞬态符号，旧版经 `program.symbol(tagged_id)` 越界 panic。

**DEFER（带 blocked-by）**：spread 成员（`{...o}`，`getSpreadType`）、计算属性名（`checkComputedPropertyName` + late binding + string/number/symbol 索引签名合成）、get/set/方法成员、shorthand 属性、上下文类型（类型**流入**字面量，`getApparentTypeOfContextualType`）、元组/const 上下文（`createTupleType` + `as const` readonly 冻结 `getRegularTypeOfObjectLiteral`）、excess-property `2353`（关系引擎无 elaboration）、`createArrayLiteralType` 的 `ObjectFlagsArrayLiteral` 克隆（可达子集返回裸 `Array<T>` 引用即足够元素访问+赋值性）、PropertyAssignment 上的（grammar-error）显式类型注解（`checkTypeAssignableToAndOptionallyElaborate`）。

**red→green 切片表（实测红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 tracer（genuine RED）| `object_literal_property_reads_member_type` | `const o = { a: 1 };\no.a;` → `check_expression(o.a)==number`（红：旧 `ObjectLiteralExpression` 产 `error`，`o.a`=`error`，`TypeId(3)≠TypeId(8)`）| `check.rs` `check_expression` 新 `ObjectLiteralExpression` 臂 + 新私有 `check_object_literal`/`check_property_assignment`/`check_expression_for_mutable_location` + 私有 fn `property_name_text` + `mod.rs` 新 `pub(crate) new_object_literal_property` + `nodebuilder.rs` `serialize_members` 合成符号感知 |
| 2（slice 1 前 genuine RED；正例 green-on-arrival）| `object_literal_property_mismatch_reports_2322` / `object_literal_assignable_to_matching_annotation` | `const o: { a: number } = { a: "x" };` → 1×2322「Type '{ a: string; }' is not assignable to type '{ a: number; }'.」；`= { a: 1 }` → 0 诊断（红：slice 1 前对象=`error` 可赋任意 0 诊断）| 无（既有 `properties_related_to` 迭代合成成员）|
| 3 tracer（genuine RED）| `array_literal_element_access_resolves_element_type` | `interface Array<T>{...}\nconst arr = [1, 2];\narr[0];` → `check_expression(arr[0])==number`（红：旧 `ArrayLiteralExpression` 产 `error`，`arr[0]`=`error`）| `check.rs` 新 `ArrayLiteralExpression` 臂 + 新私有 `check_array_literal`/`create_array_literal_type`（复用 `get_declared_type_of_symbol` + `resolve_name`）|
| 3（tracer 前 genuine RED；正例 green-on-arrival）| `array_literal_element_mismatch_reports_2322` / `array_literal_element_assignable_to_number` | `const n: string = arr[0];` → 1×2322「Type 'number' is not assignable to type 'string'.」；`const n: number = arr[0];` → 0 诊断 | 无 |
| 3 guard | `empty_array_literal_is_never_array_under_strict_null_checks` / `..._is_undefined_array_without_strict_null_checks` | `[];` → `Array<never>`（默认 strict）/ strictNullChecks=False → `Array<undefined>` | 空臂 `strict_null_checks() ? never : undefined` |

> 红→绿证据：slice 1 / slice 3 tracer **均 genuine RED**（`o.a`/`arr[0]` 旧为 `error`，断言 id 不等）→最小触点转绿；slice 2 的 2322 在 slice 1 落地前亦 genuine red（对象=`error` 0 诊断）。正例「0 诊断」与对象多成员印刷为 **green-on-arrival 守卫**（slice 1 / slice 3 tracer impl 一并解锁结构赋值/印刷）——**如实记录非伪造红**（同 4bc/4bd/4be 口径）。**踩坑修正 1**：node builder `serialize_members` 经 `program.symbol(tagged)` 对瞬态成员越界 panic（`index 2147483648 = 1<<31`）→ 改 `is_synthesized_symbol` 分流转绿（同修 2322 源类型印刷 `{ a: string; }`）。**踩坑修正 2**：空 `[]` 默认取 `never`（非 `undefined`，默认 options `strict != false` 开 strictNullChecks）；`Checker::new()` 直接 `check_expression` 取默认 options，故 strictNullChecks-off 臂须 `new_checker(Rc<program>)` 保留 program 读其 options 才能见证。

**本轮交付**：
- `core/check.rs`：`check_expression` 新增 `ObjectLiteralExpression`/`ArrayLiteralExpression` 臂；新私有 `check_object_literal`、`check_property_assignment`、`check_expression_for_mutable_location`、`check_array_literal`、`create_array_literal_type`；新私有自由 fn `property_name_text`。imports 加 `SymbolTable`、`ObjectFlags`/`ObjectType`、`get_declared_type_of_symbol`。
- `core/mod.rs`：新 `pub(crate) Checker::new_object_literal_property`（瞬态属性 + 直接置 `resolvedType`）。
- `core/nodebuilder.rs`：`serialize_members` 合成符号名感知（必要修复）。
- 测试：`check_test.rs` +10（slice1×3 + slice2×2 + slice3×3 + 空数组×2）。
- **未改任何既有 pub fn 签名、无 `lib.rs` 改动、无新依赖。** 唯一新公开项 `new_object_literal_property` 为 `pub(crate)`（不挂 doctest）。

**新公开 API 形状**：本轮新增 1 个 `pub(crate)` 方法（`new_object_literal_property`），其余均私有方法/自由 fn 与 `check_expression` 臂内部改写。无新 `pub` 项，公开 API 形状与 4be 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**428 单测**（+10，相对 4be 基线 418）+ **134 doctest**（**±0**：新增 `pub(crate)` 方法用 `//` 行注释、不挂 doctest）。

**公开 API 仅加法（compiler + transformers 保持绿）**：未改任何既有 `pub fn` 签名，未加任何 pub 项。`check_expression` 对 `ObjectLiteralExpression`/`ArrayLiteralExpression` 从 `error_type` 改为匿名对象类型 / `Array<T>` 引用（下游属性访问/元素访问/赋值性更精确，无既有测试回归）。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker`（`--lib` 428 单测 + `--doc` 134 doctest）绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（无其它 lane）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **spread 成员 `{...o}`**：未做。blocked-by: `getSpreadType`（对象类型合并 + 覆盖诊断 `checkSpreadPropOverrides`）。
- **计算属性名 `[expr]: v`**：`property_name_text` 对计算名返回 `None`（跳过该成员）。blocked-by: `checkComputedPropertyName` + late-binding（4ag 已有 well-known symbol 半张）+ string/number/symbol 索引签名合成（`getObjectLiteralIndexInfo`）。
- **get/set/方法成员 / shorthand 属性**：未做（仅 `PropertyAssignment`）。blocked-by: 访问器/方法签名收集（`checkObjectLiteralMethod`）+ shorthand 解析（`checkShorthandPropertyAssignment`）。
- **上下文类型（类型流入字面量）**：`checkExpressionForMutableLocation` 仅做无上下文 widen。blocked-by: `getApparentTypeOfContextualType` + `getContextualType` + `instantiateContextualType`。
- **元组 / `as const` readonly 冻结**：数组字面量仅非-元组非-const 路径；`[1,2] as const` / `{x:1} as const` 仍经 `regular_type_of_literal_type` 恒等（4be DEFER 现部分解锁——字面量已类型化，但 readonly 冻结未做）。blocked-by: `createTupleType`(elementInfos) + `getRegularTypeOfObjectLiteral` + tuple readonly 化 + `isValidConstAssertionArgument` 的 array/object 臂。
- **excess-property `2353`**（`{ a: 1, b: 2 }` 赋 `{ a: number }` 多余 `b`）：探查后 DEFER——关系引擎 `properties_related_to` 无 excess-property elaboration（不检查 source 多出的属性）。blocked-by: `checkTypeRelatedTo` 的 fresh-object-literal excess-property 检查 + elaboration（`getPropertyOfType` source-side 反查）。
- **`createArrayLiteralType` 的 `ObjectFlagsArrayLiteral` 克隆**：可达子集返回裸 `Array<T>` 引用即足够元素访问+赋值性。blocked-by: array-literal widening 旗标消费者。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/objectLiterals/`（简单 `{ a: 1 }` 属性类型 + 结构赋值 2322）+ `tests/cases/conformance/types/typeRelationships/typeInference/`（数组字面量元素 union → `T[]`）最基础形态——无 spread/computed/method/contextual/excess-property/tuple-inference。

**推荐下一轮（4bg）**：(a) **excess-property `2353`**（fresh object literal 多余属性，关系引擎 elaboration）——本轮已置 `FreshLiteral` 旗标，下一步给 `checkTypeRelatedTo` 接 fresh-object-literal excess-property 检查；(b) **shorthand 属性 `{ a }`**（`checkShorthandPropertyAssignment` → resolve identifier value）+ 计算属性名索引签名（`getObjectLiteralIndexInfo`）；(c) **`[1,2] as const` / `{x:1} as const`** readonly 元组/对象冻结（`getRegularTypeOfObjectLiteral` + tuple readonly，承接 4be DEFER，现字面量已类型化）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4bg 落地记录（worklog 摘要）—— fresh object literal 的 excess-property 检查 `2353`

> 承接 4bf：对象字面量已类型化并置 `FreshLiteral` 旗标。本轮在赋值性检查点对 **fresh object literal** 跑 excess-property 检查（Go `hasExcessProperties` 的可达子集），命中报 `2353` 并抑制 `2322` 头消息。

**Go 真值锚点**：
- `internal/checker/relater.go:Relater.hasExcessProperties(2695)`：核心——`isExcessPropertyCheckTarget` 门控、空对象/global Object 抑制、按 source 属性顺序逐个 `isKnownProperty`，首个未知属性报 `2353` 并 `return true`。
- `internal/checker/relater.go:recursiveTypeRelatedToWorker(2647)`：`isPerformingExcessPropertyChecks := intersectionState&IntersectionStateTarget==0 && isObjectLiteralType(source) && source.objectFlags&ObjectFlagsFreshLiteral != 0`；命中 `hasExcessProperties` 则 `reportRelationError(headMessage)`。
- `internal/checker/relater.go:reportRelationError(4773-4777)`：链头是 `2353`/`2561` excess 消息时**早返回**，故 `2322` 头消息被抑制（最终只报 `2353`）。
- `internal/checker/relater.go:Checker.isKnownProperty(716)` + `isExcessPropertyCheckTarget(746)`；`internal/checker/checker.go:isEmptyObjectType(26326)`/`isEmptyResolvedType(26322)`；`getApplicableIndexInfoForName`；`getNamedMembers(21907)`/`isReservedMemberName(1584)`。
- **freshness 剥离**：`internal/checker/checker.go:widenTypeForVariableLikeDeclaration(18101)` → `getWidenedType(18214)` → `getWidenedTypeOfObjectLiteral(18259)`：变量声明类型经 widening 丢掉 `FreshLiteral`/`ObjectLiteral` 旗标——故对象字面量赋给变量后**经变量读回不再 fresh**，不触发 excess 检查。

**新增实现（全部 additive，未改既有签名 / lib.rs 导出 / 依赖 / `.go`）**：
- `core/relations.rs`（镜像 relater.go）：`is_object_literal_type`、`is_excess_property_check_target`、`is_known_property`（属性查 + 索引签名查 `getApplicableIndexInfoForName` + union/intersection 递归）、`is_empty_object_type`（object：无属性/签名/索引；non-primitive `object`）——均 `pub(crate)`。
- `core/check.rs`：`check_object_literal_excess_properties`（门控：source 是 fresh object literal）+ `has_excess_properties`（报 `2353`，错误节点定位到字面量成员名）+ `property_symbol_name`（synthesized/program 名分流）+ `get_widened_type`/`get_widened_type_of_object_literal`（可达子集：fresh object literal → 去 `FreshLiteral|ObjectLiteral|ContainsObjectOrArrayLiteral` 的 regular anonymous object）+ 自由函数 `object_literal_property_name_node`（在字面量里按名找 `PropertyAssignment` 名节点）。
- `core/declared_types.rs`：`get_applicable_index_info_for_name`（`pub(crate)`，name → string-literal key → `get_applicable_index_info`）；`get_properties_of_type` 过滤 reserved-name 成员（`is_reserved_member_name`，镜像 `getNamedMembers`，使 `__index`/`__call`/`__new` 不再泄漏进属性表）；`get_type_of_variable_or_property` 末尾接 `get_widened_type`（镜像 `widenTypeForVariableLikeDeclaration`，对注解/非字面量恒等）。

**集成点**：`check_variable_declaration` 在 `is_type_assignable_to` 报 `2322` **之前**先跑 `check_object_literal_excess_properties`；命中即 `return`（不再报 `2322`），镜像 Go reportRelationError 的 excess 抑制。

**TDD 切片（逐个 genuine RED→GREEN）**：
1. `const o: { a: number } = { a: 1, b: 2 };` → 1×`2353`（`b`）。RED：0 诊断（关系忽略多余属性）。GREEN：excess 检查命中报 `2353`。+ 正控 `{ a: 1 }` → 0 诊断。
2. **非-fresh 源**：`const src = { a: 1, b: 2 }; const o: { a: number } = src;` → 0×`2353`。RED：`src` 保留 fresh 旗标 → 误报 `2353`。GREEN：`get_widened_type` 在变量类型计算处剥离 freshness。
3. **索引签名抑制**：`interface T { [k: string]: number } const o: T = { a: 1, b: 2 };` → 0 诊断。RED：`is_known_property` 无索引通道 → 误报 `2353`（再加 `__index` 泄漏致 `2322`）。GREEN：补 `getApplicableIndexInfoForName` 索引通道 + `get_properties_of_type` 过滤 reserved 名。
4. **空对象抑制**：`const o: {} = { a: 1 };` → 0 诊断。RED：误报 `2353`。GREEN：`has_excess_properties` 接 `is_empty_object_type` 抑制门。

**测试增量**：`cargo test -p tsgo_checker` 单测 428 → 440（+12：4 集成切片含正控 / 5 关系谓词单测 / `get_applicable_index_info_for_name` / `get_properties_of_type` reserved-名过滤），doctest 134 不变。clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 通过。

**DEFER（带 blocked-by）**：
- **"Did you mean to write X" 建议变体 `2561`**（`Object_literal_may_only_specify_known_properties_but_..._Did_you_mean_to_write`）：blocked-by `getSuggestionForNonexistentProperty`（拼写建议）。
- **spread 经过的 excess**（`shouldCheckAsExcessProperty` 的 `ValueDeclaration.Parent` 判定 + spread 属性）：blocked-by `getSpreadType` + 属性 value-declaration 回链。
- **union-target excess 归约**（`findMatchingDiscriminantType` / `filterPrimitivesIfContainsNonPrimitive` / `checkTypes` 的 `Types_of_property_0_are_incompatible` 臂）：blocked-by 判别式归约 + union 关系 elaboration。
- **`suppressExcessPropertyErrors` / `// @ts-expect-error`**：blocked-by 指令解析 + 抑制基础设施。
- **JS-literal 索引签名模拟 / `globalObjectType` subset 抑制 / JSX-attribute 消息变体**：blocked-by JS literals、lib globals（P6）、JSX 属性类型化。
- **索引签名的结构关系**（source 属性逐个 vs target 索引签名，`indexSignaturesRelatedTo`）：本轮 `properties_related_to` 仍只比对名义属性；`{a:1}` 赋 `interface T {[k:string]:number}` 的 **2353 抑制**已可达（`isKnownProperty` 索引通道 + reserved-名过滤使 target 名义属性为空 → 结构关系真空成立），但完整索引签名关系 elaboration 仍 DEFER。blocked-by `checkTypeRelatedTo` 的 `indexSignaturesRelatedTo`。
- **late-bound name / substitution-type / generic mapped** 在 `isKnownProperty`/`isExcessPropertyCheckTarget`/`isEmptyObjectType` 的臂：blocked-by late binding、substitution、mapped types。
- **`getWidenedType` 全量**（`any`/nullable→`any`、union/intersection/array widening、widening context、嵌套字面量逐属性 re-widening）：本轮仅 fresh object literal 顶层去旗标（成员已在 `checkExpressionForMutableLocation` widen）。blocked-by union/array widening + widening contexts + 嵌套递归。

**推荐下一轮（4bh）**：(a) **`2561` 建议变体**（`getSuggestionForNonexistentProperty` 拼写建议）；(b) **shorthand 属性 `{ a }`** + 计算属性名索引签名（`getObjectLiteralIndexInfo`）；(c) **`indexSignaturesRelatedTo`**（source 结构 vs target 索引签名的完整关系 elaboration）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4bh 落地记录（worklog 摘要）—— 对象字面量 shorthand 属性 `{ a }` + 非字面量计算属性名索引签名 `{ [k]: v }`

> 承接 4bf（对象字面量 `PropertyAssignment` 类型化）/ 4bg（excess-property 2353 + `getApplicableIndexInfoForName` + reserved-名过滤）。本轮把 Go `checkObjectLiteral` 的成员循环再镜像两类 `ObjectLiteralElement`：**ShorthandPropertyAssignment**（`{ a }`）+ **非字面量 ComputedPropertyName**（`{ [k]: v }` 贡献索引签名）。逐行为红→绿，一次一个。单 lane（无其它 lane）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:Checker.checkObjectLiteral(13076)`：成员 switch（13153）对 `PropertyAssignment`/`ShorthandPropertyAssignment`/`ObjectLiteralMethod` 分别 `checkPropertyAssignment`/`checkShorthandPropertyAssignment`/`checkObjectLiteralMethod`；计算名先 `computedNameType = checkComputedPropertyName(memberDecl.Name())`（13150）；`hasComputed*Property` 块（13244）：`computedNameType != nil && flags&StringOrNumberLiteralOrUnique == 0` 且 `isTypeAssignableTo(computedNameType, stringNumberSymbolType)` → 按 `number`/`esSymbol`/else 置 `hasComputed{Number,Symbol,String}Property`，否则（字面量/unique 名）入 `propertiesTable`；`createObjectLiteralType`（13122）按三 flag 各 `append(getObjectLiteralIndexInfo(isReadonly, propertiesArray[offset:], {string,number,esSymbol}Type))`。
- `internal/checker/checker.go:Checker.checkShorthandPropertyAssignment(13603)`：非解构 → `expr = ObjectAssignmentInitializer`，nil 则 `expr = node.Name()`；`expressionType = checkExpressionForMutableLocation(expr, checkMode)`；有 `node.Type()` → `getTypeFromTypeNode` + `checkTypeAssignableToAndOptionallyElaborate` 返回注解（DEFER）；否则返回 `expressionType`。
- `internal/checker/checker.go:Checker.checkComputedPropertyName(26619)`：`links.resolvedType = checkExpression(node.Expression())`；若 `flags&Nullable != 0 || !isTypeAssignableToKind(t, StringLike|NumberLike|ESSymbolLike) && !isTypeAssignableTo(t, stringNumberSymbolType)` → 报 `A_computed_property_name_must_be_of_type_string_number_symbol_or_any`（2464）。`in`-name 特例 + `typeNodeLinks` 缓存 DEFER。
- `internal/checker/checker.go:Checker.getObjectLiteralIndexInfo(19576)`：逐 prop 按 `keyType==stringType && !isSymbolWithSymbolName` / `keyType==numberType && isSymbolWithNumericName` / `keyType==esSymbolType && isSymbolWithSymbolName` 收 `getTypeOfSymbol(prop)`；`unionType = undefinedType` 或 `getUnionTypeEx(propTypes, Subtype)`；`newIndexInfo(keyType, unionType, isReadonly, nil, components)`。
- `internal/checker/checker.go:isSymbolWithSymbolName(19596)` / `isSymbolWithNumericName(19607)` / `isNumericName(19626)`+`isNumericComputedName(19636)`（`isTypeAssignableToKind(checkComputedPropertyName(name), NumberLike/ESSymbol)`）；`utilities.go:isNumericLiteralName(860)`（`jsnum.FromString(name).String()==name`）；`isTypeUsableAsPropertyName(841)`（`flags&StringOrNumberLiteralOrUnique != 0`）。

**可达裁剪（faithful-but-reachable）**：
- 成员循环新增 `ShorthandPropertyAssignment` 臂（dispatch 到新私有 `check_shorthand_property_assignment`）；新增计算名预处理（`check_computed_property_name`）。
- **shorthand**：`expr = object_assignment_initializer.unwrap_or(name)` → `check_expression_for_mutable_location`（fresh 字面量 widen；`const a = 1` 的 fresh `1` widen 到 `number`）。解构-pattern 路径（optional 化）/ 显式类型注解 DEFER。
- **计算名**：非字面量（`flags & STRING_OR_NUMBER_LITERAL_OR_UNIQUE == 0`）且可赋 `string|number|symbol`（经 `is_type_assignable_to`，union 目标关系 4u 已支持）→ 按 number/esSymbol/else 置 `has_computed_{number,symbol,string}_property`，成员以 `\u{FE}computed` 名（`INTERNAL_SYMBOL_NAME_COMPUTED`）合成进 `all_members`（用于索引值 union），**不入** `members`/`properties` 命名表。字面量/unique 计算名 → 晚绑定**命名**成员 DEFER（跳过）。
- `get_object_literal_index_info`：镜像 Go `getObjectLiteralIndexInfo` 的成员筛选 + `getUnionType` 合值（Go `UnionReductionSubtype` 对本轮 widen 后基元值观察等价）；`is_readonly = false`（`as const` 索引 readonly DEFER）。
- **端口偏离（必要）**：Go `getObjectLiteralIndexInfo` 经 `prop.Declarations[0].Name()` 读计算名做 `isSymbolWithNumericName`/`isSymbolWithSymbolName`；本港合成属性符号无 declarations，故把计算名表达式类型随符号存入 `ObjectLiteralMember`，谓词 `is_object_literal_member_with_{symbol,numeric}_name` 据此判（计算名经 `flags.intersects(ES_SYMBOL_LIKE/NUMBER_LIKE)`，静态名经 `is_numeric_literal_name`）。
- **记忆化偏离**：Go `checkComputedPropertyName` 经 `typeNodeLinks` 缓存且有 spread 预扫双跑；本港无表达式缓存，故**跳过 spread 预扫**（spread DEFER），每计算名只检查一次，避免 2464 双报。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 tracer（genuine RED）| `object_literal_shorthand_property_reads_referenced_var_type` | `const a = 1;\nconst o = { a };\no.a;` → `number`（红：shorthand 跳过 → `o` 空 → `o.a`=`error`，实测 `TypeId(3)≠TypeId(8)`）| `check.rs` 成员循环加 `ShorthandPropertyAssignment` 臂 + 新私有 `check_shorthand_property_assignment` |
| 2（slice 1 前 genuine RED；正控 green-on-arrival）| `object_literal_shorthand_property_mismatch_reports_2322` / `..._assignable_to_matching_annotation` | `const a = 1;\nconst o: { a: string } = { a };` → 1×2322；`= { a: number }` → 0（红：临时禁用 shorthand 臂实测源印为 `{}`，`Type '{}' is not assignable to type '{ a: string; }'.`）| 无（slice 1 解锁结构赋值）|
| 3 tracer（genuine RED）| `object_literal_computed_string_name_synthesizes_string_index` | `const k: string = "x";\nconst o = { [k]: 1 };\no["anything"];` → `number`（红：计算名跳过 → 无索引 → `error`，`TypeId(3)≠TypeId(8)`）| `check.rs` 计算名预处理 + `has_computed_*` 块 + 索引合成 + 新私有 `check_computed_property_name`/`get_object_literal_index_info`/`is_object_literal_member_with_{symbol,numeric}_name` + 自由 fn `is_numeric_literal_name` + 私有 struct `ObjectLiteralMember` |
| 3 正控（green-on-arrival）| `object_literal_named_property_coexists_with_computed_name` | `const k: string = "x";\nconst o = { b: 2, [k]: 1 };\no.b;` → `number`（命名属性不被索引吞）| 无 |
| 3b tracer（genuine RED）| `object_literal_computed_number_name_synthesizes_number_index` | `const k: number = 0;\nconst o = { [k]: 1 };\no[0];` → `number`（红：同 slice 3）| 无（slice 3 的 number 臂一并落地）|
| 3c（genuine RED）| `object_literal_computed_name_non_indexable_reports_2464` | `const k: boolean = true;\nconst o = { [k]: 1 };` → 1×2464（红：临时 `if false &&` 禁用 emission 实测 0 诊断）| `check.rs` `check_computed_property_name` 的 2464 emission |
| 3 端到端（slice 3 前 genuine RED；正控 green-on-arrival）| `object_literal_string_index_value_mismatch_reports_2322` / `..._is_assignable_to_number` | `const k: string="x";\nconst o={[k]:1};\nconst s: string = o["foo"];` → 1×2322；`const n: number = ...` → 0（红：slice 3 前无索引 → `o["foo"]`=`error` 可赋任意 0 诊断）| 无（经 4bg `getApplicableIndexInfoForName` + 4ad `getIndexedAccessType`）|
| 附加（单测）| `is_numeric_literal_name_matches_round_trip` | `"0"/"123"/"1.5"` 真；`"0xF00D"/"01"/"a"/""/"\u{FE}computed"` 假 | `is_numeric_literal_name` |

> 红→绿证据：slice 1 / 3 / 3b tracer **均 genuine RED**（`o.a`/`o["anything"]`/`o[0]`=`error`，`TypeId(3)≠TypeId(8)`）；slice 2 经**临时禁用 shorthand 臂**实测红（源印 `{}` 非 `{ a: number; }`）；slice 3c 经**临时 `if false &&` 禁用 emission**实测红（0 诊断）；端到端 mismatch 在 slice 3 前 genuine red（无索引 → `error` 0 诊断）。正控为 **green-on-arrival 守卫**（slice 1 / slice 3 impl 一并解锁）——**如实记录非伪造红**（同 4bc/4bd/4be/4bf/4bg 口径）。

**本轮交付**：
- `core/check.rs`：成员循环加 `ShorthandPropertyAssignment` 臂 + 计算名预处理 + `has_computed_{string,number,symbol}_property` 块 + 索引签名合成；新私有 `check_shorthand_property_assignment`、`check_computed_property_name`（含 2464）、`get_object_literal_index_info`、`is_object_literal_member_with_symbol_name`、`is_object_literal_member_with_numeric_name`；新私有自由 fn `is_numeric_literal_name`；新私有 struct `ObjectLiteralMember`；imports 加 `IndexInfo`/`IndexInfoId`、`INTERNAL_SYMBOL_NAME_COMPUTED`。
- 测试：`check_test.rs` +10（shorthand×3 + 计算名 string/number/coexist×3 + 2464×1 + 索引端到端×2 + `is_numeric_literal_name`×1）。
- **未改任何既有 pub fn 签名、无新 pub 项、无 `lib.rs` 改动、无新依赖。** 全部新方法/fn/struct 私有；复用 4bg 既有 `pub(crate)`（`get_applicable_index_info_for_name` 等）+ 4ad `getIndexedAccessType`。

**新公开 API 形状**：本轮**无新 pub 项**——公开 API 形状与 4bg 完全一致。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**450 单测**（+10，相对 4bg 基线 440）+ **134 doctest**（**±0**：新方法/fn 私有、不挂 doctest）。

**公开 API 仅加法（compiler 保持绿）**：未改任何既有 `pub fn` 签名、未加任何 pub 项。`check_object_literal` 对 shorthand 从「跳过」改为典型成员；对非字面量计算名从「跳过」改为合成索引签名（下游属性/元素访问 + 赋值性更精确，无既有测试回归）。`cargo build -p tsgo_compiler` 绿（实测）。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker`（`--lib` 450 单测 + `--doc` 134 doctest）绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（无其它 lane）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **`{ a = 1 }` shorthand 默认值**（解构-assignment-only，使属性 optional + 检查默认值）：本轮 `check_shorthand_property_assignment` 走非解构路径（`object_assignment_initializer.unwrap_or(name)`，对纯 `{ a }` 取 name）。blocked-by: 解构-assignment 目标类型化（`inDestructuringPattern` + `hasDefaultValue` + optional 化）。
- **shorthand 显式类型注解 `{ a }: T`**（grammar-error 形）：未做。blocked-by: `checkTypeAssignableToAndOptionallyElaborate` elaboration。
- **字面量 / unique-symbol 计算名 → 晚绑定命名成员**（`{ ["a"]: 1 }`/`{ [SYM]: 1 }`，`isTypeUsableAsPropertyName` → `getPropertyNameFromType` → 命名 late-bound 属性）：本轮**跳过**（计算名为字面量/unique 时不入命名表也不入索引）。blocked-by: late binding（`hasLateBindableName`/`getPropertyNameFromType` + `CheckFlagsLate` + `nameType` links）。
- **spread 成员 `{...o}`**（+ spread 触发的 `propertiesArray` offset 分段 / `getObjectLiteralIndexInfo(propertiesArray[offset:])`）：未做。blocked-by: `getSpreadType` + 多段 offset。
- **get/set/方法成员**（`checkObjectLiteralMethod` / accessor `checkNodeDeferred`）：未做。blocked-by: 访问器/方法签名收集。
- **上下文类型（类型流入字面量）**：`checkExpressionForMutableLocation` 仅无上下文 widen；`contextualTypeHasPattern` 的 implied-prop optional / 2353 binding-pattern 分支未做。blocked-by: `getApparentTypeOfContextualType` + `getContextualType` + pattern-for-type。
- **`as const` 索引 readonly**（`isConstContext` → index info `readonly`）：本轮 `is_readonly=false`（`{...} as const` 字面量本身仍 4bf/4be DEFER）。blocked-by: 对象字面量 const-context 类型化（`getRegularTypeOfObjectLiteral`）。
- **`checkComputedPropertyName` 的 `n in obj`-name 特例 + `typeNodeLinks` 缓存 + spread 预扫双跑**：未做（无表达式缓存，跳过预扫避免双报）。blocked-by: in-operator 计算名 + 表达式类型记忆化。
- **`isSymbolWithSymbolName` 的 `IsKnownSymbol` 臂 + `getObjectLiteralIndexInfo` 的 `components`（冲突计算名声明）**：未做。blocked-by: 已知符号（P6）+ 声明-carrying 合成符号。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/es6/shorthandPropertyAssignment/`（shorthand 属性基础形态）+ `tests/cases/conformance/types/members/objectTypeWithStringNamedNumericProperty*` / `tests/cases/conformance/expressions/objectLiterals/`（计算名索引签名）+ `tests/cases/conformance/types/computedPropertyNames/`（2464 计算名类型约束）最基础形态——无 spread/method/accessor/late-bound/contextual/const-readonly。

**推荐下一轮（4bi）**：(a) **字面量/unique 计算名 → 晚绑定命名成员**（`{ ["a"]: 1 }` 成命名属性 `a`，`getPropertyNameFromType` + late binding，承接本轮 DEFER）；(b) **`2561`「Did you mean to write」建议变体**（`getSuggestionForNonexistentProperty` 拼写建议，承接 4bg）；(c) **get/set/方法成员**（`checkObjectLiteralMethod` 收集签名）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 4bi 落地记录（worklog 摘要）—— `as const` const-context 的对象/数组字面量类型化（readonly 元组 + readonly 字面量属性）

**目标**：承接 4be（`as const` 对单体字面量的 freshness-stripping）/ 4bf（对象/数组字面量表达式类型化）。4be 显式 DEFER 了「数组/对象 as const」（彼时 `check_expression` 无 array/object 臂）。本轮落地 Go 的 const-context 透传：`[1, 2] as const` → **readonly 元组** `readonly [1, 2]`（元素是**保留的字面量** `1`/`2`，非 widen 到 `number`；元组 readonly）；`{ a: 1 } as const` → **readonly 字面量属性对象** `{ readonly a: 1; }`（属性符号带 `Readonly` check flag，属性类型是字面量 `1`）。逐行为红→绿，一次一个。单 lane（无其它 lane）。cargo 仅 `-p tsgo_checker`（未 `--workspace`）。未 `git commit`。

**Go 真值（ground truth，已逐一核对）**：
- `internal/checker/checker.go:Checker.isConstContext(13529)`：`parent := node.Parent`；`IsConstAssertion(parent)` || 上下文-const 分支（DEFER）|| `(IsParenthesizedExpression(parent)||IsArrayLiteralExpression(parent)||IsSpreadElement(parent)) && isConstContext(parent)` || `(IsPropertyAssignment(parent)||IsShorthandPropertyAssignment(parent)||IsTemplateSpan(parent)) && isConstContext(parent.Parent)`——**纯语法递归**，任意嵌套深度透传。
- `internal/ast/utilities.go:IsConstAssertion(2431)`：`AsExpression`/`TypeAssertionExpression` 且 `IsConstTypeReference(node.Type())`。
- `internal/checker/checker.go:checkExpressionForMutableLocation(13784)`：`isConstContext(node)` → `getRegularTypeOfLiteralType(t)`（保字面量，不 widen）；`isTypeAssertion(node)` → `t`（DEFER）；else → `getWidenedLiteralLikeTypeForContextualType(...)`（widen）。
- `internal/checker/checker.go:checkArrayLiteral(7989)`：`inConstContext := isConstContext(node)`；`checkMode&ForceTuple || inConstContext || inTupleContext` → `createArrayLiteralType(createTupleTypeEx(elementTypes, elementInfos, inConstContext && !mutableArrayLikeContextual /*readonly*/))`；否则 `Array<unionElement>`。
- `internal/checker/checker.go:checkObjectLiteral(13076,13104,13124)`：`inConstContext := isConstContext(node)`；`if inConstContext { checkFlags = ast.CheckFlagsReadonly }`；`prop = newSymbolEx(SymbolFlagsProperty|member.Flags, member.Name, checkFlags)`；`isReadonly := isConstContext(node)` 喂 `getObjectLiteralIndexInfo`。
- `internal/checker/checker.go:getRegularTypeOfLiteralType(25132)`：freshable→regularType；union→mapType（DEFER）；**否则（object/tuple）恒等返回**——故 `[…] as const`/`{…} as const` 经 `checkAssertion` 的 `getRegularTypeOfLiteralType` 恒等返回已 readonly 化的元组/对象（readonly 来自 `checkArrayLiteral`/`checkObjectLiteral` 而非 `getRegularTypeOfLiteralType`）。

**可达裁剪（faithful-but-reachable）**：
- 新私有自由 fn `is_const_context(program, node)`（递归镜像 Go，**任意嵌套深度**）+ `is_const_assertion(arena, node)`（`AsExpression`/`TypeAssertionExpression` + `is_const_type_reference`，复用 4be 既有 free fn）。
- `check_expression_for_mutable_location` 加 const 臂（`is_const_context` → `regular_type_of_literal_type`），数组元素/对象属性值共用此臂——这是字面量保留的共享机制。
- `check_array_literal` const 臂：`is_const_context(node)` → `create_tuple_type_ex(element_types, readonly=true)`（元素已在共享臂保为 regular 字面量）。空 `[] as const` → `readonly []`。
- `check_object_literal` const 臂：`member_check_flags = CheckFlags::READONLY`，喂每个 `new_object_literal_property`；`get_object_literal_index_info(_, _, _, is_readonly=in_const_context)`。
- 印刷：nodebuilder `type_to_string` 新增 `TUPLE` 旗标分流到新私有 `serialize_tuple`（`[e0, e1]` / `readonly [e0, e1]`）；`serialize_members` 对带 `Readonly` check flag 的合成属性印 `readonly ` 前缀（Go `isReadonlySymbol`）。
- **嵌套深度**：因 `is_const_context` 全递归（同 Go），支持**任意层** array/object 嵌套——`{ a: [1, 2] } as const` → `{ readonly a: readonly [1, 2]; }`、`[[1]] as const` → `readonly [readonly [1]]`（均有单测见证）。

**严格 TDD（逐行为 red→green，一次一个；每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）**：

| # | 切片 | 最小 input → observable（实测红/绿） | 实现触点 |
|---|---|---|---|
| 1 tracer（genuine RED）| `const_assertion_on_array_literal_keeps_literal_element_types` | `interface Array<T>{...}\nconst t = [1, 2] as const;\nt[0];` → `check_expression(t[0])==字面量 1`（红：旧无 const 臂 → `Array<number>`，`t[0]`=`number`，实测 `TypeId(8)≠TypeId(22)`）| `check.rs` 新 free fn `is_const_context`/`is_const_assertion`；`check_expression_for_mutable_location` const 臂；`check_array_literal` const 臂；`mod.rs` 新 `pub(crate) create_tuple_type_ex`；`types.rs` `ObjectType` 加 `readonly: bool` |
| 1b tracer（genuine RED）| `const_assertion_on_array_literal_prints_readonly_tuple` | `const t = [1, 2] as const;\nt;` → `type_to_string==readonly [1, 2]`（红：元组旧落 `serialize_members` 印 `{}`）| `nodebuilder.rs` `type_to_string` TUPLE 分流 + 新私有 `serialize_tuple` |
| 2a（green-on-arrival）| `const_assertion_on_object_literal_keeps_literal_property_type` | `const o = { a: 1 } as const;\no.a;` → `字面量 1`（slice 1 的共享 mutable-location const 臂落地即覆盖对象属性值）| 无 |
| 2b tracer（genuine RED）| `const_assertion_on_object_literal_marks_property_readonly` | `const o = { a: 1 } as const;\no;` → 属性 `a` 符号带 `CheckFlags::READONLY`（红：旧对象字面量属性 check flags 恒空）| `mod.rs` 新 `pub(crate) synthesized_symbol_check_flags`；`new_object_literal_property` 加 `check_flags` 入参；`check_object_literal` const 臂 |
| 2c tracer（genuine RED）| `const_assertion_on_object_literal_prints_readonly_member` | `const o = { a: 1 } as const;\no;` → `type_to_string=={ readonly a: 1; }`（红：旧印 `{ a: 1; }` 无 readonly）| `nodebuilder.rs` `serialize_members` readonly 前缀 |
| 3 NC（green-on-arrival）| `non_const_object_literal_property_is_widened_and_mutable` | `const o = { a: 1 };\no;` → `{ a: number; }` 且属性无 `Readonly`（4bf 不变，无 const 泄漏）| 无 |
| 3 NC（green-on-arrival）| `non_const_array_literal_is_array_not_tuple` | `const t = [1, 2];\nt;` → `Array<number>`（非元组、非 readonly，4bf 不变）| 无 |
| 附加 | `const_assertion_on_array_literal_second_element_keeps_literal` | `t[1]` → 字面量 `2`（位置存储见证）| 无 |
| 附加（嵌套深度）| `const_assertion_propagates_into_nested_array_literal` | `const o = { a: [1, 2] } as const;\no;` → `{ readonly a: readonly [1, 2]; }`（对象→数组 1 层嵌套透传）| 无 |
| 附加（嵌套深度）| `const_assertion_propagates_into_nested_inner_array_literal` | `const t = [[1]] as const;\nt;` → `readonly [readonly [1]]`（数组→数组嵌套透传）| 无 |
| 单测 | `create_tuple_type_ex_sets_readonly_flag`（mod_test）| `create_tuple_type_ex(_, true)` readonly=true、`create_tuple_type` readonly=false | `create_tuple_type_ex` |
| 单测 | `type_to_string_tuple_elements` / `type_to_string_readonly_tuple_elements`（nodebuilder_test）| `[string, number]` / `readonly [string, number]` | `serialize_tuple` 两分支 |

> 红→绿证据：slice 1 / 1b / 2b / 2c **均 genuine RED**（1：`t[0]`=`number`(`TypeId(8)`)≠字面量 `1`(`TypeId(22)`)；1b：元组印 `{}`；2b：属性 check flags 空、不含 `READONLY`；2c：印 `{ a: 1; }` 无 readonly）→ 最小触点转绿。slice 2a（对象属性字面量保留）为 **green-on-arrival**：与数组元素共用 `check_expression_for_mutable_location` const 臂，slice 1 落地即覆盖。NC + 嵌套 + 单测为守卫（**如实记录非伪造红**，同 4bc–4bh 口径）。

**本轮交付**：
- `core/types.rs`：`ObjectType` 加 `pub readonly: bool` 字段（默认 false，仅 TUPLE 有意义；所有 `ObjectType { .. }` 构造经 `..Default::default()`，加字段 additive-safe）。
- `core/mod.rs`：新 `pub(crate) create_tuple_type_ex(element_types, readonly)`（`create_tuple_type` 委托其 readonly=false）；新 `pub(crate) synthesized_symbol_check_flags`；`new_object_literal_property` 加 `check_flags: CheckFlags` 入参（`pub(crate)` 签名变更，仅 2 处 check.rs 调用）；`SynthesizedSymbol.check_flags` 去 `#[allow(dead_code)]`。
- `core/check.rs`：新私有 free fn `is_const_context`/`is_const_assertion`；`check_expression_for_mutable_location` const 臂；`check_array_literal` const-元组臂；`check_object_literal` const-readonly check flag + index readonly；`get_object_literal_index_info` 加 `is_readonly` 入参。
- `core/nodebuilder.rs`：`type_to_string` TUPLE 分流 + 新私有 `serialize_tuple`；`serialize_members` readonly 前缀（synthesized `CheckFlags::READONLY`）。
- 测试：`check_test.rs` +10（slice1/1b/2a/2b/2c + 2 NC + 3 附加）、`nodebuilder_test.rs` +2（tuple 印刷两分支）、`mod_test.rs` +1（`create_tuple_type_ex` readonly）。
- **未改任何既有 `pub fn` 签名、无新 `pub` 项、无 `lib.rs` 改动、无新依赖。** 仅：`ObjectType` 加 `pub` 字段（additive，所有构造经 `..Default::default()`）；新 `pub(crate)` 方法（`create_tuple_type_ex`/`synthesized_symbol_check_flags`）；既有 `pub(crate) new_object_literal_property` 加入参（非 `pub` API，task 显式许可）。

**新公开 API 形状**：无新 `pub` 项；`ObjectType` 新增 `pub readonly: bool`（默认 false，对既有读者 additive）。`cargo build -p tsgo_compiler` 绿（实测）。

**测试增量**：**463 单测**（+13，相对 4bh 基线 450）+ **134 doctest**（**±0**：新方法均 `pub(crate)`/私有，不挂 doctest）。

**支持的 const-context 嵌套深度**：**任意层**（`is_const_context` 全递归镜像 Go 的 `isConstContext`，覆盖 paren/array/spread/property-assignment/shorthand/template-span 链）。见证：`{ a: [1, 2] } as const` → `{ readonly a: readonly [1, 2]; }`、`[[1]] as const` → `readonly [readonly [1]]`。

**gate（实测，均已 RUN）**：`cargo test -p tsgo_checker`（`--lib` 463 单测 + `--doc` 134 doctest）绿；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿。未 `--workspace`（无其它 lane）。未 `git commit`。

**本轮 DEFER（带 blocked-by）**：
- **`isConstContext` 的上下文-const 分支**（`isValidConstAssertionArgument(node) && isConstTypeVariable(getContextualType(node), 0)`）：未做（可达子集仅语法 `as const`）。blocked-by: 上下文类型传递 + `isConstTypeVariable`（const 类型参数）。
- **`<const>expr` 前缀断言形（`TypeAssertionExpression`）的类型化**：`is_const_assertion` 已认 `TypeAssertionExpression`（故其内嵌 paren/array 的 const 透传可达），但 `check_expression` 仍无 `TypeAssertionExpression` 臂（直接 `<const>[1,2]` 的顶层断言未类型化）。blocked-by: `TypeAssertionExpression` 表达式臂（4be 既有 DEFER）。
- **`forceTuple` / tuple-like 上下文 / mutableArrayLike 上下文清 readonly**：本轮元组仅 const 路径，readonly 恒 `inConstContext`；Go 的 `checkMode&ForceTuple`、tuple-like contextual、`isMutableArrayLikeType` 清 readonly 未做。blocked-by: `forceTuple` check mode + contextual typing。
- **`createArrayLiteralType` 的 `ObjectFlagsArrayLiteral|ContainsObjectOrArrayLiteral` 克隆 + cachedTypes**：本港 const 元组直接 `create_tuple_type_ex`，未套 `createArrayLiteralType` 旗标克隆/缓存（可达子集元素访问+赋值性+印刷已足）。blocked-by: array-literal widening 旗标消费者 + 类型缓存键。
- **嵌套字面量的 per-property re-widening 在非-const `getWidenedType`**：const 元组/对象 readonly 字面量经 `get_widened_type`（4bg）恒等（元组无 `CONTAINS_OBJECT_OR_ARRAY_LITERAL` 旗标，对象成员已是 regular 字面量），与 Go 可观察等价；但 Go 的 `getWidenedTypeWithContext` 对元组类型参数递归 re-widen 未建。blocked-by: union/array widening + widening contexts。
- **元组 spread（`[...a] as const`）/ optional/variadic/labeled 元组元素 / `length` 与 `[number]` 成员的 const-readonly 形态**：本轮 fixed-arity 元组（承 4ae 子集），spread/变长元素 DEFER。blocked-by: `createNormalizedTupleType` + `TupleElementInfo` + iterator/spread typing。
- **readonly-tuple → `ReadonlyArray<T>` 赋值性 / mutable-array 赋 readonly-tuple 报错 / 2540（readonly 属性赋值）**：未做（任务 DEFER）。`o.a = 2` 的 2540 写诊断 blocked-by: 赋值目标 readonly 检查（`isReadonlySymbol` 消费端 + `checkReferenceExpression`）。
- **program（非合成）成员符号的 readonly 修饰印刷**：`serialize_members` 仅认合成符号 `CheckFlags::READONLY`；interface/class `readonly` 字段印刷 DEFER。blocked-by: bound 符号声明修饰 readonly。
- **`getRegularTypeOfObjectLiteral`（JSX/regular 对象配对）+ union `regularType` 链**：本轮 const 对象 readonly 来自 `checkObjectLiteral` 的 check flag（非 `getRegularTypeOfObjectLiteral`），Go 的 `getRegularTypeOfObjectLiteral`（fresh↔regular 对象配对、`regularTypeOfObjectLiteral` 缓存）未建。blocked-by: fresh/regular 对象配对 + union mapType。

**conformance 切片（登记，端到端对拍仍在 P10）**：`tests/cases/conformance/types/literal/constAssertions*`（`as const` readonly 元组/对象）+ `tests/cases/conformance/expressions/asOperator/`（`as const` 数组/对象字面量）+ `tests/cases/conformance/types/tuple/`（readonly 元组印刷/元素访问）最基础形态——无 spread/forceTuple/contextual/ReadonlyArray-赋值性/2540。

**推荐下一轮（4bj）**：(a) **2540 readonly 属性赋值诊断**（`o.a = 2` where `o` is `as const` → `Cannot assign to 'a' because it is a read-only property`，消费 `Readonly` check flag）；(b) **字面量/unique 计算名 → 晚绑定命名成员**（承接 4bh DEFER）；(c) **`<T>expr` / `<const>expr` 类型断言臂**（`TypeAssertionExpression`，承接 4be DEFER）。强 strict 语义最终仍 blocked-by P6 真 lib.d.ts。

## 与 Go 的已知偏离（divergence）

- **`Type` / `Symbol` / `Signature` / `IndexInfo` 全部 arena + 句柄索引**（`TypeId`/`SymbolId`/`SignatureId`/`IndexInfoId`），不用 `*T`。Go 的 interning `map[...]*Type` → `FxHashMap<Key, TypeId>`。（PORTING §5）
- **删除 `Type.checker` 反向指针**：操作改为 `checker.method(type_id)`。
- `TypeData` / `nodeData` 接口判别联合 → Rust `enum`（PORTING §3）。
- ~30 张 `core.LinkStore` → `FxHashMap`；`sync.Once`/惰性 getter → `OnceCell`。
- checker 池并行：rayon 在多 checker 实例层面，诊断按文件稳定排序保确定性（PORTING §6）。
- Go `any`（`TypeSystemEntity`/`GetConstantValue` 返回 `any`）→ 具体判别枚举（如 `enum ConstantValue { Str(String), Num(jsnum::Number) }`）。
- `//go:generate stringer` → 手写 `Display`。
- **多文件 program = 每文件独立 arena + 合并符号空间（4aa）**：Go 的节点/符号是裸指针，跨文件天然唯一；Rust 的 `NodeId`/`SymbolId` 是 per-arena 索引（每文件从 0 起），故多文件**保留每文件独立 arena**（NodeId 文件本地，只经所属文件 view 的 arena 访问），但**符号空间按文件偏移合并成一张 vec**（重写 `members`/`exports`/`parent`/`export_symbol`/`locals`/`node_symbol`/`globals` 的 SymbolId），使 checker 的 per-symbol 缓存（`value_symbol_links` 等）跨文件不冲突。文件句柄用高位编码文件序号（`FILE_INDEX_SHIFT`，低位为原始根 NodeId），与原始根区分，作 `file_view`/诊断分桶键。跨文件全局类型经 `view_for_symbol` 在声明文件 view 上构建并**预热成员类型**，使后续在其它文件 view 上的属性访问命中缓存而非读错 arena。（PORTING §5；与 Go 裸指针的偏离，必要且受认可）
- **保留 program（4l）**：Go `c.program = program`（共享非拥有指针）→ `Checker.program: Option<RetainedProgram>`，`RetainedProgram(Rc<dyn BoundProgram>)`（PORTING §3：共享非空指针 → `Rc`）。`Checker` **不带生命周期参数**（否则传染全 crate）；故 `Rc<dyn BoundProgram>` 为 `'static`，program 须拥有数据（compiler 借用式 `BoundFile<'a>` 需 owned/`Rc`-share，P6）。`Rc` 非 `Arc`：pool 顺序驱动，并行落地换 `Arc`。`check_source_file`/`get_diagnostics` 去掉 per-call program 参数，基于保留 program 工作；幂等经 `checked_files`（Go `sourceFileLinks.typeChecked`）。

## 转交 / 推迟（DEFER）

- **正确性主要靠 P10**：包内单测仅 `TestGetSymbolAtLocation`（4b）+ `TestTracerPushPreservesEndArgMutations`（4a）+ `BenchmarkNewChecker`（性能，P10）。其余检查行为 **DEFER 到 P10 conformance/fourslash/`.d.ts` baseline 对拍**。每个子阶段在 worklog 里记录其覆盖的 conformance 目录。
- emit resolver（4k）的消费者是 P5 printer/declaration transformer；node builder 序列化（4j）也主要在 P5 验证。
- 跨 phase：本包 import `tsgo_tsoptions`（README 列 P6）。同 `modulespecifiers`，存在 P4→P6 依赖倒挂，需在 README 协调（见本包"存疑/偏离"与 phase README）。
