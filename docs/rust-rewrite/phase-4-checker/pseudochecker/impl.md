# pseudochecker: 实现方案（impl.md）

**crate**：`tsgo_pseudochecker`　**目标**：一个"假装的 checker"——不做真正类型检查，只把那些**语法上就能得到类型节点**的表达式/声明（带类型注解的变量、`as const` 字面量、对象/数组字面量、箭头/函数表达式签名、原始字面量等）求成一组 `PseudoType` 骨架，供"独立声明（isolatedDeclarations，ID）"模式下快速生成 `.d.ts`，无需启动完整 checker。
**依赖（crate）**：`tsgo_ast`
**Go 源**：`internal/pseudochecker/`（3 个非测试文件，约 1300 行：`lookup.go` ~720 行 + `type.go` ~380 行 + `checker.go` ~22 行）

## 这个包是什么（业务说明）

TypeScript 的 `isolatedDeclarations` 特性要求：能**只看单个文件的语法**就生成它的 `.d.ts`，不去解析其他文件、不做全程序类型推断。`pseudochecker` 就是这个特性的核心引擎：它接收 AST 节点，返回 `PseudoType`——一种"类型骨架"，记录"应该如何用这些节点构造一个类型"，但**故意不规约、不做真正的类型运算**。

`PseudoType` 是判别联合：有直接引用类型节点的（`Direct`）、引用表达式但太复杂只能交给真 checker 的（`Inferred`，通常 ID 下会报错）、原始字面量类型（`String`/`Number`/`StringLiteral`/...）、单调用签名（箭头/函数表达式）、tuple（`as const` 数组）、对象字面量（含方法/属性/getter/setter）、union、`MaybeConstLocation`（const vs 普通的二义性延迟到 nodebuilder 决定）。`lookup.go` 是入口逻辑（`GetTypeOfDeclaration`/`GetTypeOfExpression`/`GetReturnTypeOfSignature`/`GetTypeOfAccessor`），把 AST 形状映射成 `PseudoType`。

它是个**纯语法 + 极少 flag（strictNullChecks / exactOptionalPropertyTypes）** 的包，只依赖 `ast`。它在 Phase 4 因为 checker 之外它也独立服务 ID，且与 `pseudotypenodebuilder`（在 checker 包里，把 `PseudoType`→AST 节点）配套。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包关键决策——**这是 ast 同款判别联合模式的小型样板**：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `PseudoType{ Kind; data pseudoTypeData }` + `data interface{ AsPseudoType() *PseudoType }` + 各 `PseudoTypeXxx` struct 内嵌 `PseudoTypeBase` | `enum PseudoType { Undefined, Null, Any, String, ..., Direct(Box<PseudoTypeDirect>), Inferred(Box<PseudoTypeInferred>), Union(Vec<PseudoTypeId>), ObjectLiteral(...), ... }` | Go 用"嵌入 base + data 接口"模拟判别联合；Rust 直接用判别 `enum`（PORTING §3：`interface{}`→enum）。`Kind` 退化为枚举判别 |
| `newPseudoType(kind, data)`（回填 `Kind`/`data`，返回 `*PseudoType`） | enum 构造函数（`PseudoType::direct(node)` 等） | Go 的"data.AsPseudoType() 回填"技巧在 Rust 不需要 |
| 全局单例 `PseudoTypeUndefined/Null/Any/...`（`var ... = newPseudoType(...)`） | `enum` 的无负载变体（`PseudoType::Undefined` 等，零成本，无需单例） | Go 因为是指针所以做单例；Rust 用无字段枚举变体 |
| `*PseudoType` 在 union/tuple/object 里互相引用 | `Box<PseudoType>` 或 **`PseudoTypeId`（arena 索引）** | 这些是树状、无环、生命周期短，可用 `Box`；若要与 checker 缓存共享则用 arena+`PseudoTypeId`（PORTING §5）。**推荐 `Box`**（树形、ID 模式仅在需跨缓存共享时） |
| `PseudoObjectElement{ Kind; data pseudoObjectElementData }` + Method/PropertyAssignment/SetAccessor/GetAccessor | `enum PseudoObjectElement { Method{...}, PropertyAssignment{...}, SetAccessor{...}, GetAccessor{...} }`（共享 `name`/`optional`） | 同上判别联合 |
| `PseudoParameter{ Rest; Name *ast.Node; Optional; Type *PseudoType }` | `struct PseudoParameter { rest, name: NodeId, optional, type_: Box<PseudoType> }` | AST 引用用 `NodeId`（PORTING §5） |
| `*ast.Node` / `*ast.TypeParameterDeclaration` 字段 | `NodeId` / `TypeParameterDeclarationId`（arena 索引） | 全部 AST 引用 → 索引 |
| `PseudoChecker{ strictNullChecks; exactOptionalPropertyTypes bool }` | `struct PseudoChecker { strict_null_checks: bool, exact_optional_property_types: bool }` | 两个布尔 flag，无其它状态 |

**所有权图**：`PseudoType` 是从 AST（arena 句柄读）派生出的**树形骨架**，自身不持有 AST（只存 `NodeId`）。推荐 `Box<PseudoType>` 做子节点（树、无环、不跨缓存）。`PseudoChecker` 方法 `(ch *PseudoChecker)` 全部需要传入 arena 句柄读 AST。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/pseudochecker/checker.go` | `internal/pseudochecker/lib.rs` | crate 根。`PseudoChecker` struct + `NewPseudoChecker` |
| `internal/pseudochecker/type.go` | `internal/pseudochecker/type.rs` | `PseudoType` 枚举 + 各变体 payload + 构造器 + `PseudoObjectElement` + `PseudoParameter` |
| `internal/pseudochecker/lookup.go` | `internal/pseudochecker/lookup.rs` | 全部 `(ch *PseudoChecker)` 推导方法 + 自由函数 |

## 依赖白名单（本包新增的 crate）

无新增。仅依赖 `tsgo_ast`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `checker.go`）

- [ ] `pub struct PseudoChecker { strict_null_checks: bool, exact_optional_property_types: bool }`　`// Go: checker.go:PseudoChecker`
- [ ] `pub fn new_pseudo_checker(strict_null_checks, exact_optional_property_types) -> PseudoChecker`　`// Go: checker.go:NewPseudoChecker`
- [ ] 顶部 rustdoc 保留 Go 关于"晚绑定/符号合并 TODO"的设计说明

### `type.rs`（Go: `type.go`）

- [ ] `pub enum PseudoTypeKind`（19 个判别，与 Go iota 对齐：Direct/Inferred/NoResult/MaybeConstLocation/Union/Undefined/Null/Any/String/Number/BigInt/Boolean/False/True/SingleCallSignature/Tuple/ObjectLiteral/StringLiteral/NumericLiteral/BigIntLiteral）　`// Go: type.go:PseudoTypeKind`
- [ ] `pub enum PseudoType { ... }`（无负载变体 Undefined/Null/Any/String/Number/BigInt/Boolean/False/True；有负载变体见下）　`// Go: type.go:PseudoType`
  - [ ] `Direct{ type_node: NodeId }`　`// Go: type.go:PseudoTypeDirect`
  - [ ] `Inferred{ expression: NodeId, error_nodes: Vec<NodeId> }`（+ `new_pseudo_type_inferred[_with_errors]`）　`// Go: type.go:PseudoTypeInferred`
  - [ ] `NoResult{ declaration: NodeId }`　`// Go: type.go:PseudoTypeNoResult`
  - [ ] `MaybeConstLocation{ node: NodeId, const_type: Box<PseudoType>, regular_type: Box<PseudoType> }`　`// Go: type.go:PseudoTypeMaybeConstLocation`
  - [ ] `Union(Vec<PseudoType>)`　`// Go: type.go:PseudoTypeUnion`
  - [ ] `SingleCallSignature{ signature: NodeId, parameters: Vec<PseudoParameter>, type_parameters: Vec<NodeId>, return_type: Box<PseudoType> }`　`// Go: type.go:PseudoTypeSingleCallSignature`
  - [ ] `Tuple{ elements: Vec<PseudoType> }`　`// Go: type.go:PseudoTypeTuple`
  - [ ] `ObjectLiteral{ elements: Vec<PseudoObjectElement> }`　`// Go: type.go:PseudoTypeObjectLiteral`
  - [ ] `StringLiteral(NodeId)` / `NumericLiteral(NodeId)` / `BigIntLiteral(NodeId)`（`PseudoTypeLiteral`）　`// Go: type.go:PseudoTypeLiteral`
- [ ] 各构造器（`PseudoType::direct/inferred/no_result/maybe_const_location/union/single_call_signature/tuple/object_literal/string_literal/...`）对齐 Go `NewPseudoTypeXxx`
- [ ] `pub struct PseudoParameter { rest: bool, name: NodeId, optional: bool, type_: Box<PseudoType> }` + `new_pseudo_parameter`　`// Go: type.go:PseudoParameter`
- [ ] `pub enum PseudoObjectElement { Method{...}, PropertyAssignment{readonly, type_}, SetAccessor{signature, parameter}, GetAccessor{signature, type_} }`（共享 `name: NodeId`, `optional: bool`）+ `signature()` 取签名　`// Go: type.go:PseudoObjectElement/PseudoObjectElementKind`
- [ ] 各 `New...`：`new_pseudo_object_method` / `new_pseudo_property_assignment` / `new_pseudo_set_accessor` / `new_pseudo_get_accessor`　`// Go: type.go`

### `lookup.rs`（Go: `lookup.go`）

> 全部方法签名 `(&self, arena, node) -> PseudoType`（或 `Box<PseudoType>`）。
- [ ] `pub fn get_return_type_of_signature(&self, sig_node) -> PseudoType`　`// Go: lookup.go:GetReturnTypeOfSignature`
- [ ] `pub fn get_type_of_accessor(&self, accessor) -> PseudoType`　`// Go: lookup.go:GetTypeOfAccessor`
- [ ] `pub fn get_type_of_expression(&self, node) -> PseudoType`　`// Go: lookup.go:GetTypeOfExpression`
- [ ] `pub fn get_type_of_declaration(&self, node) -> PseudoType`（分派到 property/variable/accessor/...）　`// Go: lookup.go:GetTypeOfDeclaration`
- [ ] `fn type_from_property_assignment` / `type_from_expando_property` / `type_from_property` / `type_from_variable` / `type_from_accessor` / `infer_accessor_type`　`// Go: lookup.go`
- [ ] `fn get_type_annotation_from_all_accessor_declarations` / `get_type_annotation_from_accessor`　`// Go: lookup.go`
- [ ] `fn create_return_from_signature` / `type_from_single_return_expression`　`// Go: lookup.go`
- [ ] `fn type_from_expression`（核心分派：字面量/对象/数组/前缀一元/类型断言/函数表达式）　`// Go: lookup.go:typeFromExpression`
- [ ] `fn type_from_object_literal` + `get_accessor_member` + `can_get_type_from_object_literal`（返回阻止性错误节点列表）　`// Go: lookup.go`
- [ ] `fn type_from_array_literal` + `can_get_type_from_array_literal`　`// Go: lookup.go`
- [ ] `fn type_from_primitive_literal_prefix` / `type_from_type_assertion` / `type_from_function_like_expression`　`// Go: lookup.go`
- [ ] `fn clone_type_parameters` / `clone_parameters` / `type_from_parameter[_worker]`　`// Go: lookup.go`
- [ ] 自由函数：`is_value_signature_declaration`、`is_const_context_propagating_kind`、`pub fn is_in_const_context`、`is_undefined_pseudo_type`、`type_node_could_refer_to_undefined`、`could_already_refer_to_undefined_type`、`is_optional_initialized_or_rest_parameter`、`last_required_param_index`、`add_undefined_if_definitely_required`、`is_contextually_typed`　`// Go: lookup.go`

### Cargo / crate 接线

- [ ] `internal/pseudochecker/Cargo.toml`（`name = "tsgo_pseudochecker"` + path dep `tsgo_ast`）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明 `mod type_; mod lookup;`（文件名 `type.rs` 是 Rust 关键字冲突 → 模块名用 `pseudo_type` 或文件改 `ptype.rs`，见"已知偏离"）+ re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `PseudoType` 枚举 + 无负载单例变体（Undefined/Null/Any/String/...）+ 构造器。
2. `PseudoChecker::new` + `get_type_of_expression` 的字面量分支（`StringLiteral`/`NumericLiteral`/原始字面量）。
3. `type_from_variable`（带注解 → `Direct`）/ `type_from_type_assertion`（`as const` / `as T`）。
4. 对象/数组字面量（`type_from_object_literal`/`array_literal` + `can_get_*` 错误节点收集）。
5. 函数/箭头表达式签名（`type_from_function_like_expression` + 参数 clone + 可选/rest）。
6. const 上下文传播（`is_in_const_context` / `MaybeConstLocation`）。
> 全程用手搓的最小 AST（或 P3 parser 产出的节点）做输入；Go 侧无单测，断言用语义已知值（见 tests.md）。

## 与 Go 的已知偏离（divergence）

- "嵌入 base + data 接口"判别联合 → Rust `enum PseudoType`（更直接、零单例）。全局单例 `PseudoTypeUndefined` 等 → 无负载枚举变体。
- 子 `PseudoType` 引用用 `Box`（树形）；仅当需与 checker 缓存共享时改 arena+`PseudoTypeId`。
- 文件名 `type.go` → Rust 不能用 `type.rs`（关键字），模块改名（`ptype.rs` / `pseudo_type.rs`），属命名层面偏离，结构 1:1。
- AST 引用全部 `NodeId`（PORTING §5）。

## 转交 / 推迟（DEFER）

- `PseudoType` → AST 节点的回放（`pseudotypenodebuilder.go`）在 checker 包，不属本包。
- 真实 ID `.d.ts` 输出正确性靠 P10 `isolatedDeclarations` conformance 兜底。
- 顶部注释提到的"dumb late binder / mergeSymbol 抽取"是 Go 侧的未来 feature，移植时**保持现状**（不提前实现）。
