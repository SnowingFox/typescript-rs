# checker: 实现方案（impl.md）

**crate**：`tsgo_checker`　**目标**：TypeScript 的**类型检查器**——全仓最大、最硬的模块。负责符号解析、类型构造与实例化、子类型/赋值关系判定、类型推断、控制流分析与类型收窄、表达式/语句/JSX 的类型检查与诊断、`.d.ts` 序列化（node builder）、以及给 emit/语言服务用的查询 API。
**依赖（crate）**：`tsgo_ast` `tsgo_binder` `tsgo_collections` `tsgo_core` `tsgo_debug` `tsgo_diagnostics` `tsgo_evaluator` `tsgo_jsnum` `tsgo_module` `tsgo_modulespecifiers` `tsgo_scanner` `tsgo_tracing` `tsgo_tsoptions` `tsgo_tspath`，外加 `xxh3`（hash）
**Go 源**：`internal/checker/`（24 个非测试文件，**59,514 行**）

> ⚠️ 本 impl.md **不逐函数枚举**（`checker.go` 一个文件就 ~3.2 万行、上千个方法）。改为：① 文件职责总览（逐文件一句话）；② 按**子系统**拆 TODO 并锚到对应文件；③ 给出**执行期再拆子阶段（4a..4k）**的强烈建议。函数级 TODO 在每个子阶段真正动工时，由该子阶段的 worklog 细化。

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

- [ ] `Type` 头 + `enum TypeData`（全部变体）+ `Arena<TypeData>` + `TypeId`　`// Go: types.go:Type/TypeData`
- [ ] `Signature` + `Arena<Signature>` + `SignatureId`；`IndexInfo` + arena　`// Go: types.go:Signature/IndexInfo`
- [ ] `TypeFlags`/`ObjectFlags`/`TypeFormatFlags`/`SymbolFormatFlags`/`SignatureFlags`（bitflags，位值 1:1）　`// Go: types.go`
- [ ] interning map（string/number/bigint/enum/union/intersection/indexedAccess/template/...）→ `FxHashMap<Key, TypeId>`　`// Go: checker.go:Checker 字段`
- [ ] ~30 张 `LinkStore` → `FxHashMap<K, V>`（valueSymbol/alias/module/declaredType/...）　`// Go: checker.go:Checker 字段`
- [ ] 删除 `Type.checker` 反向指针，改 checker 持 arena（PORTING §5 偏离）

### 类型映射 / 实例化（4a 基建 + 4d 应用）

- [ ] `enum TypeMapper`（Simple/Array/Composite/Merged/Function）+ apply　`// Go: mapper.go:TypeMapper`
- [ ] `instantiate_type` / `instantiate_signature` / 实例化深度/计数限制　`// Go: checker.go:instantiateType(21958)/instantiateSignature(20470)`

### 符号解析 / 声明类型（4b/4c）

- [ ] `resolve_name`（含 meaning/excludeGlobals/use 标志）+ `get_symbol_at_location`　`// Go: checker.go:resolveName/GetSymbolAtLocation`
- [ ] 符号合并（mergedSymbols）+ alias 跳转（`SkipAlias`）　`// Go: checker.go`
- [ ] `get_type_of_symbol` / `get_declared_type_of_symbol` / `get_apparent_type`　`// Go: checker.go:getTypeOfSymbol(16352)/getDeclaredTypeOfSymbol(23531)/getApparentType(21587)`
- [ ] 导出符号表：`get_exports_of_module` / `getExportsAndProperties`　`// Go: exports.go`

### 关系判定（4d）

- [ ] `Relation` 缓存 + `is_type_related_to` / `check_type_related_to`（subtype/strictSubtype/assignable/comparable/identity）　`// Go: relater.go:isTypeRelatedTo(170)/checkTypeRelatedTo(352)`
- [ ] 结构化比较（properties/signatures/index）、可变性 variance、递归深度守卫　`// Go: relater.go`

### 类型推断（4e）

- [ ] `infer_types` + `InferenceInfo`/`InferenceState`（复用栈）+ 优先级　`// Go: inference.go:inferTypes(53)`
- [ ] 上下文类型 / 推断候选合并 / 签名推断　`// Go: inference.go`

### 控制流分析（4f）

- [ ] `get_flow_type_of_reference` + `FlowState` 复用 + flow 循环缓存　`// Go: flow.go:getFlowTypeOfReference(77)`
- [ ] 各 narrowing（typeof/instanceof/discriminant/truthiness/in/assertion）+ 可达性分析　`// Go: flow.go`

### 表达式 / 语句检查（4g）

- [ ] `check_source_file` / `check_expression` / 语句检查族 / `get_diagnostics`　`// Go: checker.go:checkSourceFile(2176)/checkExpression(7521)/getDiagnostics(13865)`
- [ ] 调用解析 / 重载选择 / 上下文敏感函数　`// Go: checker.go`

### JSX（4h）

- [ ] `check_jsx_element` 族 + 内在/值元素 + props + JSX runtime　`// Go: jsx.go`

### 语法检查（4i）

- [ ] grammar 错误族（修饰符/重复/位置/严格模式）　`// Go: grammarchecks.go`

### node builder / 序列化 / 可达性 / printer / hover / services（4j）

- [ ] `NodeBuilder` 门面 + impl（`type_to_type_node`/`serialize_type_for_declaration`/`symbol_to_entity_name`…）　`// Go: nodebuilder.go + nodebuilderimpl.go`
- [ ] 作用域追踪 / 节点深拷贝 / 符号可达性 / pseudo-type 回放　`// Go: nodebuilderscopes.go/nodecopy.go/symbolaccessibility.go/pseudotypenodebuilder.go`
- [ ] `type_to_string` / `symbol_to_string`（printer）+ hover 展开　`// Go: printer.go/nodebuilder_hover.go`
- [ ] 语言服务查询 API（`GetSymbolsInScope`/`GetContextualType`/`GetConstantValue`/…）　`// Go: services.go`
- [ ] `SymbolTracker` 实现　`// Go: symboltracker.go`

### emit resolver（4k）

- [ ] `EmitResolver` 全部方法 + `GetEmitResolver`（`OnceCell`）　`// Go: emitresolver.go + checker.go:GetEmitResolver(31832)`

### tracer（4a）

- [ ] `Tracer`（`Push`/`Pop`、`checkerId` 注入、end-arg 变更保留）　`// Go: tracer.go`

### Cargo / crate 接线

- [ ] `internal/checker/Cargo.toml`（`name = "tsgo_checker"` + 全部 path deps + `xxh3`）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明全部 `mod`（按上表）+ re-export 公开 API（`Checker`/`NewChecker`/`Type`/`Symbol` 查询入口/`EmitResolver`/`NodeBuilder`）

## TDD 推进顺序（tracer bullet → 增量）

1. 4a：arena + `Type`/`Signature` 表示 + `NewChecker` 能构造 + intrinsic 类型 → `TestTracerPushPreservesEndArgMutations`（只需 tracer + 构造）先绿。
2. 4b：`resolveName` + `getSymbolAtLocation` 最小路径 → **`TestGetSymbolAtLocation` 绿**（接口/变量/属性访问三种节点取到非空符号）。这是包内唯一真功能单测，作为第一个端到端 tracer bullet。
3. 4c–4k：每个子阶段挑一小撮对应 conformance 目录，先把"能产出诊断/类型字符串"打通，再逐步对齐 baseline（详见 tests.md 的 P10 策略）。

## 与 Go 的已知偏离（divergence）

- **`Type` / `Symbol` / `Signature` / `IndexInfo` 全部 arena + 句柄索引**（`TypeId`/`SymbolId`/`SignatureId`/`IndexInfoId`），不用 `*T`。Go 的 interning `map[...]*Type` → `FxHashMap<Key, TypeId>`。（PORTING §5）
- **删除 `Type.checker` 反向指针**：操作改为 `checker.method(type_id)`。
- `TypeData` / `nodeData` 接口判别联合 → Rust `enum`（PORTING §3）。
- ~30 张 `core.LinkStore` → `FxHashMap`；`sync.Once`/惰性 getter → `OnceCell`。
- checker 池并行：rayon 在多 checker 实例层面，诊断按文件稳定排序保确定性（PORTING §6）。
- Go `any`（`TypeSystemEntity`/`GetConstantValue` 返回 `any`）→ 具体判别枚举（如 `enum ConstantValue { Str(String), Num(jsnum::Number) }`）。
- `//go:generate stringer` → 手写 `Display`。

## 转交 / 推迟（DEFER）

- **正确性主要靠 P10**：包内单测仅 `TestGetSymbolAtLocation`（4b）+ `TestTracerPushPreservesEndArgMutations`（4a）+ `BenchmarkNewChecker`（性能，P10）。其余检查行为 **DEFER 到 P10 conformance/fourslash/`.d.ts` baseline 对拍**。每个子阶段在 worklog 里记录其覆盖的 conformance 目录。
- emit resolver（4k）的消费者是 P5 printer/declaration transformer；node builder 序列化（4j）也主要在 P5 验证。
- 跨 phase：本包 import `tsgo_tsoptions`（README 列 P6）。同 `modulespecifiers`，存在 P4→P6 依赖倒挂，需在 README 协调（见本包"存疑/偏离"与 phase README）。
