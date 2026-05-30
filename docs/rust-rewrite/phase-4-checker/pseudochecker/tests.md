# pseudochecker: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**0 个 `*_test.go`**。Go 侧该包无任何直接单测。

## 0 直接单测的情况

- Go 侧 `internal/pseudochecker/` 无 `*_test.go`。该包正确性在 Go 仓库里完全靠 **`isolatedDeclarations`（ID）conformance / `.d.ts` baseline** 间接覆盖（如 `tests/cases/conformance/declarationEmit/*` 中带 `isolatedDeclarations` 的用例）。
- 因此本包归入 README "0 直接单测"清单，行为由 **P10 conformance/`.d.ts` parity** 兜底。
- 本轮**补充**行为级 Rust 测试：基于公开接口 `PseudoChecker::get_type_of_expression` / `get_type_of_declaration` / `get_return_type_of_signature`，输入用 P3 parser 产出（或手搓）的最小 AST，expected 用 `PseudoType` 的判别 Kind + 关键字段断言（依据 ID 语义已知值）。

## 补充行为级 Rust 测试（按推导入口分组）

### 表达式类型（`get_type_of_expression` / `type_from_expression`）

> 说明：本轮 `get_type_of_expression` 已落地，输入用 `tsgo_ast::NodeArena` 手搓最小 AST。**修正**：裸字面量经 `typeFromExpression` 实为 `MaybeConstLocation{const, regular}`（非裸 `StringLiteral`），故 expected 按 Go 实际分支断言。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `expr_string_literal_is_maybe_const` | 字符串字面量 → MaybeConstLocation | `"abc"` → `MaybeConstLocation{StringLiteral(n), String}` | lookup.go:typeFromExpression(KindStringLiteral) | ✓ |
| `expr_numeric_literal_is_maybe_const` | 数字字面量 → MaybeConstLocation | `123` → `MaybeConstLocation{NumericLiteral(n), Number}` | 同 | ✓ |
| `expr_bigint_literal_is_maybe_const` | bigint 字面量 → MaybeConstLocation | `123n` → `MaybeConstLocation{BigIntLiteral(n), BigInt}` | 同 | ✓ |
| `expr_true_false_are_maybe_const_boolean` | `true`/`false` → MaybeConstLocation | `true`→`{True,Boolean}`, `false`→`{False,Boolean}` | 同 | ✓ |
| `expr_null_keyword_is_null` | `null` → Null | `null` → `Null` | 同 | ✓ |
| `expr_identifier_undefined_vs_other` | `undefined` → Undefined；其它 id → Inferred | `undefined`→`Undefined`；`x`→`Inferred(x)` | lookup.go:typeFromExpression(KindIdentifier) | ✓ |
| `expr_parenthesized_unwraps_inner` | 括号透明，取内层表达式类型 | `("abc")` → 同 `"abc"` | lookup.go:typeFromExpression(KindParenthesizedExpression) | ✓ |
| `expr_complex_falls_back_to_inferred` | 复杂表达式 → Inferred | `f()`（调用）→ `Inferred(f())` | lookup.go:typeFromExpression 兜底 | ✓ |
| `expr_prefix_minus_number` | `-5` → 原始字面量前缀 | `-5` → NumericLiteral-like | lookup.go:typeFromPrimitiveLiteralPrefix | — P4：blocked-by tsgo_ast 缺 `IsPrimitiveLiteralValue` |
| `expr_as_const_string` | `"x" as const` → StringLiteral（const 上下文） | `"x" as const` → `StringLiteral` | lookup.go:typeFromTypeAssertion + IsInConstContext | — P4：blocked-by tsgo_ast 缺 AsExpression 数据/`IsInConstContext` |
| `expr_type_assertion` | `e as T` → Direct(T) | `x as Foo` → `Direct(Foo)` | lookup.go:typeFromTypeAssertion | — P4：blocked-by tsgo_ast 缺 AsExpression 数据/`IsConstTypeReference` |
| `expr_object_literal_simple` | `{a:1}` → ObjectLiteral | `{a: 1 as const}` → `ObjectLiteral` 含 1 个 PropertyAssignment | lookup.go:typeFromObjectLiteral | — P4：blocked-by tsgo_ast 缺 ObjectLiteral 节点数据 |
| `expr_object_literal_with_error_node` | 含 spread/shorthand → Inferred + error nodes | `{...x}` → `Inferred` with `error_nodes` 非空 | lookup.go:canGetTypeFromObjectLiteral | — P4：blocked-by tsgo_ast 缺 ObjectLiteral 节点数据 |
| `expr_array_literal_as_const` | `[1,2] as const` → Tuple | `[1,2] as const` → `Tuple{[NumLit,NumLit]}` | lookup.go:typeFromArrayLiteral | — P4：blocked-by tsgo_ast 缺 `IsInConstContext`/上下文类型 |
| `expr_arrow_function` | `() => 1` → SingleCallSignature | arrow → `SingleCallSignature{return_type}` | lookup.go:typeFromFunctionLikeExpression | — P4：blocked-by tsgo_ast 缺 function-like 节点数据 |

### 声明类型（`get_type_of_declaration` / `type_from_variable` / `type_from_property`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `decl_var_with_annotation` | 带类型注解变量 → Direct | `const x: number = ...` → `Direct(number)` | lookup.go:typeFromVariable | — P4：blocked-by tsgo_ast 缺 VariableDeclaration 节点数据 |
| `decl_var_without_annotation_infers` | 无注解 → 从初始值/Inferred | `const x = 1 as const` → `NumericLiteral` | lookup.go:typeFromVariable | — P4：blocked-by tsgo_ast 缺 VariableDeclaration 节点数据 |
| `decl_property_with_annotation` | 带注解属性 → Direct | `class { x: string }` → `Direct(string)` | lookup.go:typeFromProperty | — P4：blocked-by tsgo_ast 缺 PropertyDeclaration 节点数据 |

### 签名返回类型 / 访问器（`get_return_type_of_signature` / `get_type_of_accessor`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `signature_with_return_annotation` | 带返回注解 → Direct | `function f(): number` → `Direct(number)` | lookup.go:GetReturnTypeOfSignature/createReturnFromSignature | — P4：blocked-by tsgo_ast 缺 function-like 节点数据/`FunctionLikeData` |
| `signature_single_return_expr` | 无注解单 return → 推 | `function f(){ return 1 as const }` → `NumericLiteral` | lookup.go:typeFromSingleReturnExpression | — P4：blocked-by tsgo_ast 缺 `Body()`/`ForEachReturnStatement` |
| `accessor_get_with_annotation` | getter 带返回注解 | `get x(): T` → `Direct(T)` | lookup.go:typeFromAccessor/getTypeAnnotationFromAccessor | — P4：blocked-by tsgo_ast 缺 Get/SetAccessor 节点数据/`GetAllAccessorDeclarations` |

### 参数与 const 上下文（自由函数）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `is_in_const_context_true` | `as const` 上下文判定 | `[1] as const` 的元素位置 → `is_in_const_context==true` | lookup.go:IsInConstContext | — P4：blocked-by tsgo_ast 缺 `FindAncestor`/`IsConstAssertion`/`IsAssertionExpression` |
| `is_in_const_context_false` | 普通上下文 | 普通数组元素 → false | 同 | — P4：同上 |
| `param_optional_initialized` | 带默认值/rest 参数判可选 | `(a = 1)` → `is_optional_initialized_or_rest_parameter==true` | lookup.go:isOptionalInitializedOrRestParameter | — P4：blocked-by tsgo_ast 缺 ParameterDeclaration 节点数据 |
| `last_required_param_index` | 最后一个必需参数索引 | `(a, b?, c)` → index of `c` | lookup.go:lastRequiredParamIndex | — P4：blocked-by tsgo_ast 缺 ParameterDeclaration 节点数据 |
| `maybe_const_location_for_literal` | 字面量在普通位置 → MaybeConstLocation | 普通位置字面量 → `MaybeConstLocation{const_type, regular_type}` | lookup.go (NewPseudoTypeMaybeConstLocation 路径) | ✓（由上表 `expr_{string,numeric,bigint}_literal_is_maybe_const` 覆盖） |

### `PseudoType` 构造与判别（`type.rs` 单元）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `pseudotype_kind_values` | 判别枚举顺序与 Go iota 对齐 | `Direct as i16 == 0`, `Inferred==1`, …, **`BigIntLiteral==19`**（Go 源共 20 个常量；旧文档"==18"为 off-by-one，已更正） | type.go:PseudoTypeKind | ✓ |
| `object_element_signature_accessor` | 方法/访问器元素能取回 signature | Method/SetAccessor/GetAccessor → `signature()`→`Some(node)`；PropertyAssignment → `None`（含 `kind()`/`name()`/`optional()`） | type.go:PseudoObjectElement.Signature | ✓ |
| `no_payload_variants_report_their_kind` | 无负载变体 `kind()` 映射 | `Undefined.kind()==Undefined` … `True.kind()==True` | type.go:PseudoType（单例）| ✓ |
| `single_node_constructors` | 单节点构造器 + kind | `direct/string_literal/numeric_literal/bigint_literal` | type.go:NewPseudoTypeDirect/…Literal | ✓ |
| `inferred_constructors_carry_error_nodes` | `inferred` 空 / `inferred_with_errors` 携带 | error_nodes 空 vs 非空 | type.go:NewPseudoTypeInferred[WithErrors] | ✓ |
| `composite_constructors` | `no_result`/`maybe_const_location`/`union` | boxed 子类型 + kind | type.go:NewPseudoTypeNoResult/…MaybeConstLocation/…Union | ✓ |
| `pseudo_parameter_new` | `PseudoParameter::new` | rest/optional/name/boxed type | type.go:NewPseudoParameter | ✓ |
| `aggregate_constructors` | `single_call_signature`/`tuple`/`object_literal` | 子项保留 + kind | type.go:NewPseudoTypeSingleCallSignature/…Tuple/…ObjectLiteral | ✓ |

### `lib.rs` 单元（Go: `checker.go`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `new_stores_flags` | 构造器存两个 flag 并可读 | `new(true,false)` → `strict_null_checks()==true && exact_optional_property_types()==false` | checker.go:NewPseudoChecker | ✓ |

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（每个 Rust 测试函数上方有 `// Go: internal/pseudochecker/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）
- [x] 每个 Go `func Test*` 都已映射 —— **N/A**（Go 侧 0 单测）
- [x] expected 值均取自 ID 语义 / Go 实现分支（非 Rust 推断；已修正裸字面量→`MaybeConstLocation`、`BigIntLiteral` 判别==19）
- [x] 每条 ✓ 用例对应 impl.md 的一个已落地推导方法 / 构造器
- [x] 与 impl.md 双向对齐：已落地的 `type_from_expression` 分支与全部 `PseudoType`/`PseudoObjectElement` 变体均有用例覆盖；未落地分支（object/array/function/assertion/prefix/template/声明/访问器/签名）在 impl.md 与本表均标 DEFER(phase-4)+blocked-by

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 真实 `isolatedDeclarations` `.d.ts` 输出 | 需 `pseudotypenodebuilder`(checker 包) + emit | P4 checker / P5 / P10 |
| ID 报错诊断（spread/shorthand 等 error node 最终成诊断） | 端到端 | P10（`isolatedDeclarations` conformance） |
