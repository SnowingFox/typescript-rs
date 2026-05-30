# checker: 实现方案（impl.md）

**crate**：`tsgo_checker`　**目标**：TypeScript 的**类型检查器**——全仓最大、最硬的模块。负责符号解析、类型构造与实例化、子类型/赋值关系判定、类型推断、控制流分析与类型收窄、表达式/语句/JSX 的类型检查与诊断、`.d.ts` 序列化（node builder）、以及给 emit/语言服务用的查询 API。
**依赖（crate）**：`tsgo_ast` `tsgo_binder` `tsgo_collections` `tsgo_core` `tsgo_debug` `tsgo_diagnostics` `tsgo_evaluator` `tsgo_jsnum` `tsgo_module` `tsgo_modulespecifiers` `tsgo_scanner` `tsgo_tracing` `tsgo_tsoptions` `tsgo_tspath`，外加 `xxh3`（hash）
**Go 源**：`internal/checker/`（24 个非测试文件，**59,514 行**）

---

## checker 移植状态（4a–4m 收口 · 给下一 phase 看的"接缝"）

> **状态：checker 移植子阶段（4a–4k）+ 4l（program 保留 + pool 驱动面）+ 4m（变量声明赋值性 2322 + block 递归）已收口、gate 全绿（136 单测 + 110 doctest，clippy/fmt 干净）。** 这是一个**可达核心**的纵向切片，不是全量 checker；剩余深化 + 端到端正确性由 **P10 conformance** 兜底，跨 phase 依赖标注于下。
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

- [x] `BoundProgram` 瘦 trait（arena/root/symbol_of_node/symbol/locals）+ 测试桩 `StubProgram`（parse+bind dev-dep）（4b 落地，`core/program.rs` + `core/test_support.rs`）　`// Go: compiler/program.go:Program（子集）`
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
- [ ] 完整结构化：signatures/index 比较、variance、intersection、`Ternary`(Maybe) 递归模型、可选属性、错误报告　`// Go: relater.go`（4f+）

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
- [ ] DEFER(4m+/4n)：赋值表达式（`x = y`）/ 其余控制流语句容器（if/for/while/try/switch 体）/ 类检查 / 未用检查 / 变量声明的 binding-pattern・参数初始化器・`using` disposability・definite-assignment・未注解变量的初始化器推断（→ 经 widened 类型暴露不匹配）；重载选择 / 上下文敏感函数；`CallExpression` 经 bound program 端到端（需声明构造可调用类型 + interface call-signature 收集）；not-callable / arg-count 诊断；数字・计算索引・索引签名；完整 `TypeFacts`（typeof/EQ-NE/discriminant）　`// Go: checker.go`　blocked-by: 函数类型构造（声明→签名）+ 初始化器 widening/推断 + lib globals(P6) + node builder(4j)

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
- [ ] DEFER(4j+)：装饰器 grammar、完整修饰符顺序/位置矩阵（`must precede`/`cannot appear on …`，多数位置需解析为修饰符才可达）、参数属性、严格模式保留字（需 strict-mode 上下文检测）、其余 grammar 族（语句/类型/heritage/索引签名…）　`// Go: grammarchecks.go`　blocked-by: strict-mode 上下文 + `compilerOptions`(tsoptions/program wiring) + node builder(4j)

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
- [x] `Checker::new_checker(program)` 入口：4k 前向兼容（忽略 program）→ **4l 真正保留**（`Rc<dyn BoundProgram>` 存入 `Checker.program`，Go `c.program = program`）+ `program()` 访问器；`check_source_file(file)`/`get_diagnostics(file)` 基于保留 program 工作 + 幂等（`checked_files`）（`core/mod.rs`/`core/check.rs`）　`// Go: checker.go:NewChecker/checkSourceFile/getDiagnostics`　blocked-by: 全局/lib 绑定 + `getGlobalType` 全量（P6）
- [ ] DEFER(post-4k / P6)：`is_value_aliased`/`is_referenced_alias`（跨模块别名/导入解析）、`get_type_reference_serialization_kind`（需 `printer.TypeReferenceSerializationKind` + 类型解析）、`create_type_of_declaration`（输出类型*节点*而非字符串，需完整 node builder + `SymbolTracker`）、其余 `EmitResolver` 方法（参数/可达性/常量值/装饰器元数据）　`// Go: emitresolver.go`　blocked-by: `compiler.Program` + lib globals（P6）+ 完整 node builder

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
| `internal/checker/core/mod.rs` | `Checker` 骨架 + `Checker::new` + `new_checker(Rc<dyn BoundProgram>)`（保留 program）+ `program()`/`retained_program()`/`mark_file_checked()` + `RetainedProgram` + `new_type`/`new_literal_type`/`get_union_type`/`new_object_type`/`new_type_parameter`/`create_type_reference`/`new_signature`/`new_index_info`/`type_to_string` + link/arena/relation/instantiation/`program`/`checked_files` 字段 | `checker.go:Checker/NewChecker/newType/newLiteralType/getUnionType/newObjectType/newTypeParameter/createTypeReference/newSignature/newIndexInfo` + `printer.go:TypeToString` |
| `internal/checker/core/types.rs` | `TypeId`/`TypeFlags`/`ObjectFlags`/`format_type_flags`/`Type`/`TypeArena` + `TypeData::{Intrinsic,Literal,Union,Object,TypeParameter}` + `*Type` payloads | `types.go:TypeFlags/ObjectFlags/Type/TypeData/IntrinsicType/LiteralType/UnionType/ObjectType/TypeParameter/FormatTypeFlags` |
| `internal/checker/core/mapper.rs` | `TypeMapper`（Simple/Array/Merged/Composite/Function）+ `map_type`/`instantiate_type`/`instantiate_signature` | `mapper.go:TypeMapper` + `checker.go:instantiateType/instantiateTypeWorker/instantiateSignature` |
| `internal/checker/core/relations.rs` | `RelationKind`/`RelationCache` + `is_type_related_to`/`check_type_related_to`/`is_simple_type_related_to`/`structured_type_related_to`/`properties_related_to` + 便捷包装 | `relater.go:Relation/isTypeRelatedTo/isSimpleTypeRelatedTo/checkTypeRelatedTo/structuredTypeRelatedTo/propertiesRelatedTo` |
| `internal/checker/core/inference.rs` | `InferenceInfo`/`InferenceContext`/`InferencePriority` + `infer_types`/`get_inferred_type(s)`/`infer_type_arguments`/`get_inference_mapper`/`get_best_common_type`/`subtype_reduce` | `inference.go:inferTypes/inferFromTypes/getInferredType/getInferredTypes` + `checker.go:InferenceInfo/InferenceContext/getCommonSupertype/removeSubtypes` |
| `internal/checker/core/flow.rs` | `get_flow_type_of_reference`/`get_type_at_flow_node`（flow 遍历 + 循环缓存）+ `narrow_type_by_typeof`/`narrow_type_by_truthiness`(经 `TypeFacts`)/`narrow_type_by_equality`/`narrow_type_by_in` + `narrow_type_at_condition`/`narrow_type_by_binary`(typeof + `x === <expr>`)/`is_matching_reference` + `is_reachable_flow_node` | `flow.go:getFlowTypeOfReference/getTypeAtFlowNode/narrowTypeByTypeof/narrowTypeByTruthiness/narrowTypeByEquality/narrowTypeByInKeyword/narrowType/isMatchingReference/isReachableFlowNode` |
| `internal/checker/core/check.rs` | `check_source_file(file)`（基于保留 program + 幂等）/`check_statement`（expr-stmt/class/`VariableStatement`/`Block` 递归）/`check_expression`（literal/identifier/property/element）+ `check_identifier`/`check_property_access`/`check_element_access` + `check_variable_declaration`（赋值性 2322）+ `generalized_source_for_error`/`is_literal_type`/`type_could_have_top_level_singleton_types`/`get_base_type_of_literal_type`（错误源广义化）+ `Diagnostic` + `get_diagnostics(file)`（触发 check）/`error` + `get_signatures_of_type`/`get_return_type_of_call` | `checker.go:checkSourceFile/checkExpression/checkIdentifier/checkPropertyAccessExpression/checkVariableLikeDeclaration/checkBlock/getDiagnostics/error/getSignaturesOfType/getReturnTypeOfSignature + relater.go:reportRelationError/getBaseTypeOfLiteralType` |
| `internal/checker/core/type_facts.rs` | `TypeFacts`(TRUTHY/FALSY) + `get_type_facts`/`has_type_facts`/`get_type_with_facts`（truthiness 子集） | `utilities.go:TypeFacts/getTypeFacts/hasTypeFacts/getTypeWithFacts` |
| `internal/checker/core/jsx.rs` | `check_jsx_element`/`check_jsx_self_closing_element`/`check_jsx_fragment` + `check_jsx_opening_like`/`resolve_jsx_tag`/`is_intrinsic_tag_name`/`get_jsx_intrinsic_attributes_type`/`check_jsx_attributes`/`check_jsx_attribute_value`/`check_jsx_children` | `jsx.go:checkJsxElement/checkJsxSelfClosingElement/checkJsxFragment/checkJsxOpeningLikeElementOrOpeningFragment/getIntrinsicTagSymbol/isJsxIntrinsicTagName/checkJsxAttributes/checkJsxAttribute/checkJsxChildren` |
| `internal/checker/core/grammar.rs` | `check_grammar_modifiers`（重复/冲突/可访问性）+ `modifier_nodes`/`modifier_text` | `grammarchecks.go:checkGrammarModifiers` + `ast.go:Node.ModifierNodes` |
| `internal/checker/core/nodebuilder.rs` | `type_to_string`（命名/引用/匿名成员/union；intrinsic/literal 委托）+ `symbol_to_string` + `serialize_members` | `checker.go:typeToString/symbolToString` + `nodebuilderimpl.go` + `printer.go` |
| `internal/checker/core/emit_resolver.rs` | `EmitResolver` + `is_declaration_visible`/`serialize_type_of_declaration`/`is_implementation_of_overload` + `Checker::get_emit_resolver`(OnceCell)/`new_checker` | `emitresolver.go:EmitResolver/IsDeclarationVisible/SerializeTypeOfDeclaration/IsImplementationOfOverload` + `checker.go:GetEmitResolver/NewChecker` |
| `internal/checker/core/signatures.rs` | `Signature`/`SignatureFlags`/`SignatureArena`/`SignatureId` + `IndexInfo`/`IndexInfoArena`/`IndexInfoId` | `types.go:Signature/SignatureFlags/IndexInfo` |
| `internal/checker/core/declared_types.rs` | `get_declared_type_of_symbol`(+generic/extends/type-param)/`get_type_of_symbol`/`get_type_from_type_node`(+TypeReference)/`get_apparent_type`/`get_property_of_type`/`get_type_of_property_of_type`/`get_properties_of_type`/`resolve_structured_type_members`/`get_global_type` | `checker.go:getDeclaredType*/getDeclaredTypeOfTypeParameter/getTypeOfSymbol/getTypeFromTypeNode/getApparentType/getPropertyOfType/getTypeOfPropertyOfType/resolveStructuredTypeMembers/getBaseTypes/getGlobalType` |
| `internal/checker/core/symbols.rs` | `SymbolLinks<V>` + `*SymbolLinks`/`DeclaredTypeLinks`/`TypeAliasLinks` + `MergedSymbols` + `skip_alias` + `resolve_name` | `types.go:*SymbolLinks` + `checker.go:resolveName/getMergedSymbol/resolveSymbol` + `core/linkstore.go:LinkStore` |
| `internal/checker/core/program.rs` | `BoundProgram` 瘦 trait（arena/root/symbol_of_node/symbol/locals + 4f flow：`flow_node_of`/`flow_node`/`flow_list`） | `compiler/program.go:Program`（checker 查询子集） |
| `internal/checker/core/symbols_query.rs` | `get_symbol_at_location`/`get_symbol_of_declaration` + 属性访问（真类型路径）+ `is_declaration_name`/`name_of_declaration` | `checker.go:getSymbolAtLocation/getSymbolOfDeclaration/getSymbolOfNameOrPropertyAccessExpression` + `ast/utilities.go:IsDeclarationName` |
| `internal/checker/core/test_support.rs`（cfg(test)） | `StubProgram`（`tsgo_parser` 解析 + `tsgo_binder` 绑定）实现 `BoundProgram` | 测试桩（替代 P6 `compiler.Program`） |
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

## 与 Go 的已知偏离（divergence）

- **`Type` / `Symbol` / `Signature` / `IndexInfo` 全部 arena + 句柄索引**（`TypeId`/`SymbolId`/`SignatureId`/`IndexInfoId`），不用 `*T`。Go 的 interning `map[...]*Type` → `FxHashMap<Key, TypeId>`。（PORTING §5）
- **删除 `Type.checker` 反向指针**：操作改为 `checker.method(type_id)`。
- `TypeData` / `nodeData` 接口判别联合 → Rust `enum`（PORTING §3）。
- ~30 张 `core.LinkStore` → `FxHashMap`；`sync.Once`/惰性 getter → `OnceCell`。
- checker 池并行：rayon 在多 checker 实例层面，诊断按文件稳定排序保确定性（PORTING §6）。
- Go `any`（`TypeSystemEntity`/`GetConstantValue` 返回 `any`）→ 具体判别枚举（如 `enum ConstantValue { Str(String), Num(jsnum::Number) }`）。
- `//go:generate stringer` → 手写 `Display`。
- **保留 program（4l）**：Go `c.program = program`（共享非拥有指针）→ `Checker.program: Option<RetainedProgram>`，`RetainedProgram(Rc<dyn BoundProgram>)`（PORTING §3：共享非空指针 → `Rc`）。`Checker` **不带生命周期参数**（否则传染全 crate）；故 `Rc<dyn BoundProgram>` 为 `'static`，program 须拥有数据（compiler 借用式 `BoundFile<'a>` 需 owned/`Rc`-share，P6）。`Rc` 非 `Arc`：pool 顺序驱动，并行落地换 `Arc`。`check_source_file`/`get_diagnostics` 去掉 per-call program 参数，基于保留 program 工作；幂等经 `checked_files`（Go `sourceFileLinks.typeChecked`）。

## 转交 / 推迟（DEFER）

- **正确性主要靠 P10**：包内单测仅 `TestGetSymbolAtLocation`（4b）+ `TestTracerPushPreservesEndArgMutations`（4a）+ `BenchmarkNewChecker`（性能，P10）。其余检查行为 **DEFER 到 P10 conformance/fourslash/`.d.ts` baseline 对拍**。每个子阶段在 worklog 里记录其覆盖的 conformance 目录。
- emit resolver（4k）的消费者是 P5 printer/declaration transformer；node builder 序列化（4j）也主要在 P5 验证。
- 跨 phase：本包 import `tsgo_tsoptions`（README 列 P6）。同 `modulespecifiers`，存在 P4→P6 依赖倒挂，需在 README 协调（见本包"存疑/偏离"与 phase README）。
