# checker: 实现方案（impl.md）

**crate**：`tsgo_checker`　**目标**：TypeScript 的**类型检查器**——全仓最大、最硬的模块。负责符号解析、类型构造与实例化、子类型/赋值关系判定、类型推断、控制流分析与类型收窄、表达式/语句/JSX 的类型检查与诊断、`.d.ts` 序列化（node builder）、以及给 emit/语言服务用的查询 API。
**依赖（crate）**：`tsgo_ast` `tsgo_binder` `tsgo_collections` `tsgo_core` `tsgo_debug` `tsgo_diagnostics` `tsgo_evaluator` `tsgo_jsnum` `tsgo_module` `tsgo_modulespecifiers` `tsgo_scanner` `tsgo_tracing` `tsgo_tsoptions` `tsgo_tspath`，外加 `xxh3`（hash）
**Go 源**：`internal/checker/`（24 个非测试文件，**59,514 行**）

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
- [ ] 深化：成员类型经引用 mapper 实例化（`getTypeOfPropertyOfSymbol`）、variance、index 签名继承、enum literal-union、apparent 的 primitive→global wrapper　`// Go: checker.go/relater.go`（4e/4g/P6）
- [ ] 导出符号表：`get_exports_of_module` / `getExportsAndProperties`　`// Go: exports.go`（4e+）

### 关系判定（4d）

- [x] `RelationKind`（Identity/Subtype/StrictSubtype/Assignable/Comparable）+ `RelationCache` + `is_type_related_to`/`check_type_related_to` + 便捷包装（assignable/identical/subtype/comparable）（4d 落地，`core/relations.rs`）　`// Go: relater.go:isTypeRelatedTo(170)/checkTypeRelatedTo(352)`
- [x] `is_simple_type_related_to`（any/unknown/never、string/number/bigint/boolean/esSymbol-like→primitive、undefined→void-like、null→null、object→non-primitive、any 可赋值）+ 结构化比较（union source/target + object `properties_related_to`）+ 缓存式递归守卫（pending=true 破环）（4d 落地，`core/relations.rs`）　`// Go: relater.go:isSimpleTypeRelatedTo/structuredTypeRelatedTo/propertiesRelatedTo`
- [ ] 完整结构化：signatures/index 比较、variance、intersection、`Ternary`(Maybe) 递归模型、可选属性、错误报告　`// Go: relater.go`（4e+）

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

- [x] `Tracer` + `checkerId` 注入（`copy_with_checker_index`，不污染调用方 args）（4a 落地，`tracer.rs`）　`// Go: tracer.go:Tracer/copyWithCheckerIndex`
- [ ] `Push`/`Pop`（separateBeginAndEnd 的 end-arg 变更保留）+ `RecordType`/`wrapType`/`tracedTypeAdapter`　`// Go: tracer.go`（DEFER：见下方"4a 落地记录"——依赖完整 Type 图，且 end-arg 别名语义需 `tsgo_tracing` 暴露独立 begin/end 写入或共享可变 args）

### Cargo / crate 接线

- [x] `internal/checker/Cargo.toml`（`name = "tsgo_checker"` + 全部 path deps + `bitflags`/`rustc-hash`）；`xxh3` DEFER 到需要类型 hash interning（4b+）
- [x] 根 `Cargo.toml` workspace members 追加（checker 已在 members）
- [x] `lib.rs` 声明 `mod`（`core`/`tracer`；`core` 含 `declared_types`/`mapper`/`program`/`relations`/`signatures`/`symbols`/`symbols_query`/`types` + cfg(test) `test_support`）+ re-export 4a–4d 公开 API（含 `TypeMapper`/`RelationKind`/`TypeParameter`/`get_properties_of_type`/`resolve_structured_type_members`）
- [x] checker `Cargo.toml`：`tsgo_parser`/`tsgo_binder` 移到 `[dev-dependencies]`（仅 cfg(test) 桩 program 用）
- [ ] `lib.rs` 声明其余 `mod`（inference/flow/jsx/...）+ re-export `NewChecker`/`EmitResolver`/`NodeBuilder` 等（4e+）

## TDD 推进顺序（tracer bullet → 增量）

1. 4a：arena + `Type`/`Signature` 表示 + `NewChecker` 能构造 + intrinsic 类型 → `TestTracerPushPreservesEndArgMutations`（只需 tracer + 构造）先绿。
2. 4b：`resolveName` + `getSymbolAtLocation` 最小路径 → **`TestGetSymbolAtLocation` 绿**（接口/变量/属性访问三种节点取到非空符号）。这是包内唯一真功能单测，作为第一个端到端 tracer bullet。
3. 4c–4k：每个子阶段挑一小撮对应 conformance 目录，先把"能产出诊断/类型字符串"打通，再逐步对齐 baseline（详见 tests.md 的 P10 策略）。

## Go 源 → Rust 子文件对照（4a–4d 实际拆分）

> 维护"Go 源文件 → Rust 子文件"映射（PORTING §2）。每个 Rust 函数仍带 `// Go: internal/checker/<file>.go:<Func>` 锚到**原** Go 文件。

| Rust 文件 | 承载内容 | 锚定的 Go 源 |
|---|---|---|
| `internal/checker/lib.rs` | crate 根：模块声明 + 公开 API re-export + 所有权模型说明 | `checker.go`（crate 根） |
| `internal/checker/core/mod.rs` | `Checker` 骨架 + `Checker::new` + `new_type`/`new_literal_type`/`get_union_type`/`new_object_type`/`new_type_parameter`/`create_type_reference`/`new_signature`/`new_index_info`/`type_to_string` + link/arena/relation/instantiation 字段 | `checker.go:Checker/NewChecker/newType/newLiteralType/getUnionType/newObjectType/newTypeParameter/createTypeReference/newSignature/newIndexInfo` + `printer.go:TypeToString` |
| `internal/checker/core/types.rs` | `TypeId`/`TypeFlags`/`ObjectFlags`/`format_type_flags`/`Type`/`TypeArena` + `TypeData::{Intrinsic,Literal,Union,Object,TypeParameter}` + `*Type` payloads | `types.go:TypeFlags/ObjectFlags/Type/TypeData/IntrinsicType/LiteralType/UnionType/ObjectType/TypeParameter/FormatTypeFlags` |
| `internal/checker/core/mapper.rs` | `TypeMapper`（Simple/Array/Merged/Composite/Function）+ `map_type`/`instantiate_type`/`instantiate_signature` | `mapper.go:TypeMapper` + `checker.go:instantiateType/instantiateTypeWorker/instantiateSignature` |
| `internal/checker/core/relations.rs` | `RelationKind`/`RelationCache` + `is_type_related_to`/`check_type_related_to`/`is_simple_type_related_to`/`structured_type_related_to`/`properties_related_to` + 便捷包装 | `relater.go:Relation/isTypeRelatedTo/isSimpleTypeRelatedTo/checkTypeRelatedTo/structuredTypeRelatedTo/propertiesRelatedTo` |
| `internal/checker/core/signatures.rs` | `Signature`/`SignatureFlags`/`SignatureArena`/`SignatureId` + `IndexInfo`/`IndexInfoArena`/`IndexInfoId` | `types.go:Signature/SignatureFlags/IndexInfo` |
| `internal/checker/core/declared_types.rs` | `get_declared_type_of_symbol`(+generic/extends)/`get_type_of_symbol`/`get_type_from_type_node`(+TypeReference)/`get_apparent_type`/`get_property_of_type`/`get_properties_of_type`/`resolve_structured_type_members`/`get_global_type` | `checker.go:getDeclaredType*/getTypeOfSymbol/getTypeFromTypeNode/getTypeFromTypeReference/getApparentType/getPropertyOfType/getPropertiesOfType/resolveStructuredTypeMembers/getBaseTypes/getGlobalType` |
| `internal/checker/core/symbols.rs` | `SymbolLinks<V>` + `*SymbolLinks`/`DeclaredTypeLinks`/`TypeAliasLinks` + `MergedSymbols` + `skip_alias` + `resolve_name` | `types.go:*SymbolLinks` + `checker.go:resolveName/getMergedSymbol/resolveSymbol` + `core/linkstore.go:LinkStore` |
| `internal/checker/core/program.rs` | `BoundProgram` 瘦 trait（arena/root/symbol_of_node/symbol/locals） | `compiler/program.go:Program`（checker 查询子集） |
| `internal/checker/core/symbols_query.rs` | `get_symbol_at_location`/`get_symbol_of_declaration` + 属性访问（真类型路径）+ `is_declaration_name`/`name_of_declaration` | `checker.go:getSymbolAtLocation/getSymbolOfDeclaration/getSymbolOfNameOrPropertyAccessExpression` + `ast/utilities.go:IsDeclarationName` |
| `internal/checker/core/test_support.rs`（cfg(test)） | `StubProgram`（`tsgo_parser` 解析 + `tsgo_binder` 绑定）实现 `BoundProgram` | 测试桩（替代 P6 `compiler.Program`） |
| `internal/checker/tracer.rs` | `Tracer` + `copy_with_checker_index`（checkerId 注入） | `tracer.go:Tracer/NewTracer/copyWithCheckerIndex` |

> 其余 Go 文件（`flow.go`/`inference.go`/`nodebuilderimpl.go`/`jsx.go`/`grammarchecks.go`/`utilities.go`/`exports.go`/`emitresolver.go`/`services.go`/...）尚未落地，按子阶段 4e..4k 推进。`relater.go`（~5k 行）与 `checker.go`（~3.2 万行）继续在 `core/relations.rs`/`core/inference.rs`/`core/flow.rs`/... 增量拆分。

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
