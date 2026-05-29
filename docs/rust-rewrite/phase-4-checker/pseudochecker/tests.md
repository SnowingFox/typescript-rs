# pseudochecker: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**0 个 `*_test.go`**。Go 侧该包无任何直接单测。

## 0 直接单测的情况

- Go 侧 `internal/pseudochecker/` 无 `*_test.go`。该包正确性在 Go 仓库里完全靠 **`isolatedDeclarations`（ID）conformance / `.d.ts` baseline** 间接覆盖（如 `tests/cases/conformance/declarationEmit/*` 中带 `isolatedDeclarations` 的用例）。
- 因此本包归入 README "0 直接单测"清单，行为由 **P10 conformance/`.d.ts` parity** 兜底。
- 本轮**补充**行为级 Rust 测试：基于公开接口 `PseudoChecker::get_type_of_expression` / `get_type_of_declaration` / `get_return_type_of_signature`，输入用 P3 parser 产出（或手搓）的最小 AST，expected 用 `PseudoType` 的判别 Kind + 关键字段断言（依据 ID 语义已知值）。

## 补充行为级 Rust 测试（按推导入口分组）

### 表达式类型（`get_type_of_expression` / `type_from_expression`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `expr_string_literal` | 字符串字面量 → StringLiteral | `"abc"` → `PseudoType::StringLiteral(node)` | lookup.go:typeFromExpression | |
| `expr_numeric_literal` | 数字字面量 → NumericLiteral | `123` → `NumericLiteral` | 同 | |
| `expr_true_false` | `true`/`false` → True/False | `true`→`True`, `false`→`False` | 同 | |
| `expr_null` | `null` → Null | `null` → `Null` | 同 | |
| `expr_prefix_minus_number` | `-5` → 原始字面量前缀 | `-5` → NumericLiteral-like（`typeFromPrimitiveLiteralPrefix`） | lookup.go:typeFromPrimitiveLiteralPrefix | |
| `expr_as_const_string` | `"x" as const` → StringLiteral（const 上下文） | `"x" as const` → `StringLiteral` | lookup.go:typeFromTypeAssertion + IsInConstContext | |
| `expr_type_assertion` | `e as T` → Direct(T) | `x as Foo` → `Direct(typeNode=Foo)` | lookup.go:typeFromTypeAssertion | |
| `expr_object_literal_simple` | `{a:1}` → ObjectLiteral | `{a: 1 as const}` → `ObjectLiteral` 含 1 个 PropertyAssignment | lookup.go:typeFromObjectLiteral | |
| `expr_object_literal_with_error_node` | 含 spread/shorthand → Inferred + error nodes | `{...x}` → `Inferred` with `error_nodes` 非空 | lookup.go:canGetTypeFromObjectLiteral | |
| `expr_array_literal_as_const` | `[1,2] as const` → Tuple | `[1,2] as const` → `Tuple{elements:[NumLit,NumLit]}` | lookup.go:typeFromArrayLiteral | |
| `expr_arrow_function` | `() => 1` → SingleCallSignature | arrow → `SingleCallSignature{return_type}` | lookup.go:typeFromFunctionLikeExpression | |
| `expr_complex_falls_back_inferred` | 复杂表达式 → Inferred | `f()`（调用）→ `Inferred` | lookup.go:typeFromExpression 兜底 | |

### 声明类型（`get_type_of_declaration` / `type_from_variable` / `type_from_property`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `decl_var_with_annotation` | 带类型注解变量 → Direct | `const x: number = ...` → `Direct(number)` | lookup.go:typeFromVariable | |
| `decl_var_without_annotation_infers` | 无注解 → 从初始值/Inferred | `const x = 1 as const` → `NumericLiteral` | lookup.go:typeFromVariable | |
| `decl_property_with_annotation` | 带注解属性 → Direct | `class { x: string }` → `Direct(string)` | lookup.go:typeFromProperty | |

### 签名返回类型 / 访问器（`get_return_type_of_signature` / `get_type_of_accessor`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `signature_with_return_annotation` | 带返回注解 → Direct | `function f(): number` → `Direct(number)` | lookup.go:GetReturnTypeOfSignature/createReturnFromSignature | |
| `signature_single_return_expr` | 无注解单 return → 推 | `function f(){ return 1 as const }` → `NumericLiteral` | lookup.go:typeFromSingleReturnExpression | |
| `accessor_get_with_annotation` | getter 带返回注解 | `get x(): T` → `Direct(T)` | lookup.go:typeFromAccessor/getTypeAnnotationFromAccessor | |

### 参数与 const 上下文（自由函数）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `is_in_const_context_true` | `as const` 上下文判定 | `[1] as const` 的元素位置 → `is_in_const_context==true` | lookup.go:IsInConstContext | |
| `is_in_const_context_false` | 普通上下文 | 普通数组元素 → false | 同 | |
| `param_optional_initialized` | 带默认值/rest 参数判可选 | `(a = 1)` → `is_optional_initialized_or_rest_parameter==true` | lookup.go:isOptionalInitializedOrRestParameter | |
| `last_required_param_index` | 最后一个必需参数索引 | `(a, b?, c)` → index of `c` | lookup.go:lastRequiredParamIndex | |
| `maybe_const_location_for_literal` | 字面量在可能 const 位置 → MaybeConstLocation | 字面量在普通位置 → `MaybeConstLocation{const_type, regular_type}` | lookup.go (NewPseudoTypeMaybeConstLocation 路径) | |

### `PseudoType` 构造与判别（`type.rs` 单元）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `pseudotype_kind_values` | 判别枚举顺序与 Go iota 对齐 | `Direct as i16 == 0`, `Inferred==1`, …, `BigIntLiteral==18` | type.go:PseudoTypeKind | |
| `object_element_signature_accessor` | 方法/访问器元素能取回 signature | Method/SetAccessor/GetAccessor → `signature()` 返回对应节点；PropertyAssignment → None | type.go:PseudoObjectElement.Signature | |

## 与 impl.md 的对齐核对

- [ ] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/pseudochecker/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）

- [ ] 每个 Go `func Test*` 都已映射 —— **N/A**（Go 侧 0 单测）
- [ ] expected 值均取自 ID 语义 / Go 实现分支（非 Rust 推断）
- [ ] 每条对应 impl.md 的一个推导方法 / 构造器
- [ ] 与 impl.md 双向对齐：每个 `type_from_*` 分支与每个 `PseudoType` 变体都有用例覆盖

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 真实 `isolatedDeclarations` `.d.ts` 输出 | 需 `pseudotypenodebuilder`(checker 包) + emit | P4 checker / P5 / P10 |
| ID 报错诊断（spread/shorthand 等 error node 最终成诊断） | 端到端 | P10（`isolatedDeclarations` conformance） |
