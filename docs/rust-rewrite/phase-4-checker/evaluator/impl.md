# evaluator: 实现方案（impl.md）

**crate**：`tsgo_evaluator`　**目标**：把"语法上可常量折叠的表达式"（枚举成员初始值、字符串/数字字面量、模板串、`+ - ~` 一元、二元算术/位运算、字符串拼接）求值成一个 `Result{Value, IsSyntacticallyString, ...}`。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_jsnum`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/evaluator/`（1 个非测试文件 `evaluator.go`，169 行）

## 这个包是什么（业务说明）

`evaluator` 是 checker 的一个小而独立的工具包：它实现 TypeScript 中**编译期常量求值**的语法层逻辑。典型用途是枚举成员初始值计算（`enum E { A = 1 << 2 }`）、模板字面量类型 / 常量拼接（`` `a${1}b` `` → `"a1b"`），以及 `isolatedModules` 下用来判断某个 enum 成员能否被当作常量内联的依据。

它**故意只做"语法可达"的求值**：遇到 `Identifier` / 属性访问 / 元素访问这类需要符号解析的实体时，它不自己解析，而是回调外部传入的 `evaluateEntity`（在 checker 里就是真正会去查符号、读其他文件的实现）。因此 `evaluator` 本身是纯函数式、无副作用的：给定 AST 节点 + 一个 entity 求值回调，返回 `Result`。

它处在 Phase 4 的最前面（叶子），因为 checker 在 `NewChecker` 时会 `c.evaluate = evaluator.NewEvaluator(c.evaluateEntity, ...)`，把这个折叠器装进自己。先移植它能让 checker 的常量求值路径有现成依赖。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包特有的点：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Result{Value any; ...bool}` | `struct Result { value: EvalValue, is_syntactically_string: bool, resolved_other_files: bool, has_external_references: bool }` | Go 用 `any` 装 `string` / `jsnum.Number` / `bool` / `jsnum.PseudoBigInt` / `nil`。Rust 用判别枚举 `EvalValue` 取代 `any`（PORTING §3：`interface{}` → enum 优先） |
| `Value any`（实际取值集合） | `enum EvalValue { None, Str(String), Num(jsnum::Number), Bool(bool), BigInt(jsnum::PseudoBigInt) }` | `None` 对应 Go 的 `nil`（求值失败/不可折叠）。**关键**：`evaluator` 实际只会产出 `None` / `Str` / `Num`；`Bool` / `BigInt` 只在 `AnyToString` / `IsTruthy` 的输入侧出现（调用方可能塞进来），故枚举要覆盖全 5 种 |
| `type Evaluator func(expr, location) Result` | `type Evaluator<'a> = dyn Fn(NodeId, Option<NodeId>) -> Result + 'a`（或泛型 `F: Fn(...)`） | Go 是头等函数闭包；Rust 用闭包 trait 对象或泛型参数 |
| `NewEvaluator(evaluateEntity, outerExpressionsToSkip)` 返回闭包 | 返回一个持有 `evaluate_entity` 的 `Evaluator` 结构体（带 `&Arena` + `eval` 方法递归），而非真闭包 | Go 用递归闭包 `var evaluate; evaluate = func...`；Rust 难表达自引用闭包，改为 `struct EvaluatorImpl { skip: OuterExpressionKinds, eval_entity: F }` + `fn eval(&self, arena, expr, location) -> Result`（递归调 `self.eval`） |
| `expr.AsPrefixUnaryExpression().Operand` 等 | `arena[expr].as_prefix_unary().operand` | AST 一律走 arena + `NodeId`（PORTING §5）。`expr.Text()` → `arena.node_text(expr)` |
| `jsnum.Number` 的算术/位运算方法 | 复用 `tsgo_jsnum`（P1 已移植）的同名方法 | `BitwiseNOT/OR/AND/XOR`、`SignedRightShift`、`UnsignedRightShift`、`LeftShift`、`Remainder`、`Exponentiate`、`String`、`IsNaN`，以及 `+ - * /` 运算符 |

**无 arena 自有所有权**：本包不持有任何长生命周期对象，只读 AST（通过传入的 arena 句柄）并返回值类型 `Result`。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/evaluator/evaluator.go` | `internal/evaluator/lib.rs` | crate 根（basename == 目录名 → `lib.rs`）。含 `Result`、`EvalValue`、`Evaluator` 工厂、模板求值、`AnyToString`、`IsTruthy` |

## 依赖白名单（本包新增的 crate）

无新增。仅依赖 workspace 内 `tsgo_ast` / `tsgo_core` / `tsgo_jsnum`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/evaluator/evaluator.go`）

- [x] `pub enum EvalValue { None, Str(String), Num(jsnum::Number), Bool(bool), BigInt(jsnum::PseudoBigInt) }` — 取代 Go 的 `Value any`
- [x] `pub struct Result { value, is_syntactically_string, resolved_other_files, has_external_references }` — 4 字段，全 `pub`　`// Go: evaluator.go:Result`
- [x] `pub fn new_result(value, is_syntactically_string, resolved_other_files, has_external_references) -> Result` — 构造器　`// Go: evaluator.go:NewResult`
- [x] `pub struct Evaluator<F>`（entity 求值回调签名 `Fn(&NodeArena, NodeId, Option<NodeId>) -> Result`；回调显式带 `&NodeArena` 是 arena 模型的必要偏离）　`// Go: evaluator.go:Evaluator`
- [x] `pub fn new_evaluator(eval_entity, outer_expressions_to_skip: OuterExpressionKinds) -> Evaluator<F>` — 工厂；`Evaluator::evaluate` 方法递归（取代 Go 自引用闭包）　`// Go: evaluator.go:NewEvaluator`
  - [x] `skip_outer_expressions(expr, skip | PARENTHESES)` 预处理（本地移植：`tsgo_ast` 尚未移植该工具；当前仅括号可达，其余 OEK 种类无 NodeData→惰性）
  - [x] `KindPrefixUnaryExpression`：`+`（原值）/ `-`（取负，`Number::from(-f64)`）/ `~`（`bitwise_not`），仅当操作数求值为 `Num`
  - [x] `KindBinaryExpression`：先求左右；`is_syntactically_string = (left||right).is_str && op==Plus`；数字×数字时 `| & >> >>> << ^ * / + - % **` 共 12 个运算分支；否则 (str|num)+(str|num) 且 op==Plus → 字符串拼接（num 侧用 `Number::to_string`）
  - [x] `KindStringLiteral`（+`KindNoSubstitutionTemplateLiteral` 同组）→ `Result{ Str(text), is_syntactically_string=true, false, false }`（**NoSub 模板字面量推迟**，见下）
  - [ ] `KindTemplateExpression` → 调 `evaluate_template_expression` — **DEFER(P4)**：`tsgo_ast` 未移植 Template/TemplateSpan/NoSubstitutionTemplate 的 `NodeData` 与构造器，无法访问 head/spans，亦无法构造测试 → 落到兜底 `None`，待 AST 模板节点移植后回填
  - [x] `KindNumericLiteral` → `Num(from_string(text))`
  - [x] `KindIdentifier` → `eval_entity(expr, location)`
  - [x] `KindElementAccessExpression | KindPropertyAccessExpression`：仅当 `is_entity_name_expression(expr.expression())` 才 `eval_entity`（`is_entity_name_expression`/`expression_of`/`name_of` 本地移植，因 `tsgo_ast` 未移植这些工具）
  - [x] 兜底 → `Result{ None, is_syntactically_string, resolved_other_files, has_external_references }`
- [ ] `fn evaluate_template_expression(expr, location, eval) -> Result` — **DEFER(P4)**：blocked-by `tsgo_ast` 模板 `NodeData` 未移植（同上）
- [x] `pub fn any_to_string(v: &EvalValue) -> String` — `Str`→原样、`Num`/`BigInt`→`.to_string()`、`Bool`→`core::if_else(..,"true","false")`；`None` → `panic!("Unhandled case in AnyToString")`　`// Go: evaluator.go:AnyToString`
- [x] `pub fn is_truthy(v: &EvalValue) -> bool` — `Str`→非空、`Num`→`!=0 && !is_nan`、`Bool`→原值、`BigInt`→`!= PseudoBigInt::default()`；`None` → `panic!`　`// Go: evaluator.go:IsTruthy`

### Cargo / crate 接线

- [x] `internal/evaluator/Cargo.toml`（`name = "tsgo_evaluator"` + path deps：`tsgo_ast` `tsgo_core` `tsgo_jsnum`）
- [x] 根 `Cargo.toml` workspace members 追加 `internal/evaluator`（脚手架阶段已加入）
- [x] `lib.rs` 直接以 `pub` 定义 `Result` / `EvalValue` / `Evaluator` / `OuterExpressionKinds` / `new_result` / `new_evaluator` / `any_to_string` / `is_truthy`（已在 crate 根可见，无需额外 `pub use`）

## TDD 推进顺序（tracer bullet → 增量）

1. `EvalValue` + `Result` + `new_result` + `any_to_string` / `is_truthy`（纯值逻辑，可立刻补行为级单测，见 tests.md）。
2. `new_evaluator` 的字面量分支（`StringLiteral` / `NumericLiteral`）：用一个 stub `eval_entity`（永远返回 `None`）即可测出 `"abc"` / `123`。
3. 一元 / 二元数字运算分支（依赖 `tsgo_jsnum` 算子）。
4. 字符串拼接分支 + 模板表达式（含 span 为 `None` 的短路）。
5. `Identifier` / 属性访问 → `eval_entity` 透传（注入记录式 stub 验证回调被调用、`resolved_other_files` / `has_external_references` 正确冒泡）。

## 与 Go 的已知偏离（divergence）

- Go `Value any` → Rust `EvalValue` 判别枚举：调用方（checker）原本对 `result.Value.(jsnum.Number)` 做断言，移植后改为 `match`/`if let EvalValue::Num(n)`。
- Go 用递归闭包 `var evaluate Evaluator; evaluate = func(...)`；Rust 改用持有回调的结构体方法 `Evaluator::evaluate` 递归（无法安全表达自引用闭包）。结构 1:1，行为一致。
- entity 回调签名 `func(expr, location) Result` → `Fn(&NodeArena, NodeId, Option<NodeId>) -> Result`：因 Rust 走 arena+`NodeId`，回调需显式拿到 `&NodeArena`（PORTING §5 的必要偏离）。
- AST 访问 `expr.AsXxx().Field` → `arena.data(id)` 上的 `match NodeData`（PORTING §5）。
- `jsnum::Number` 未实现 `+ - * / Neg` 运算符 → 算术经 `Number::from(f64::from(l) <op> f64::from(r))`（JS number 即 IEEE754 double，与 Go `type Number float64` 的运算语义一致）。
- **`tsgo_ast` 尚为代表性子集**：`SkipOuterExpressions`/`OuterExpressionKinds`/`OEKParentheses`、`IsEntityNameExpression`、`Node.Expression()`/`Node.Name()` 在 Rust 侧**尚未移植**。受"仅可编辑 `internal/evaluator/**`"边界约束，已在本 crate 内**本地移植**这些小工具（`skip_outer_expressions`/`is_outer_expression`/`is_entity_name_expression`/`expression_of`/`name_of` + 本地 `OuterExpressionKinds`），待上游 `tsgo_ast` 补齐后再上移并改为复用。

## 转交 / 推迟（DEFER）

- **模板字面量求值（`KindTemplateExpression` + `evaluate_template_expression` + `KindNoSubstitutionTemplateLiteral`）— DEFER(P4)**：blocked-by `tsgo_ast` 未移植 `TemplateExpression`/`TemplateSpan`/`NoSubstitutionTemplateLiteral` 的 `NodeData` 与构造器（当前为代表性子集）。既无法访问 head/spans，也无法构造测试节点，故这些输入落到兜底 `None`。AST 模板节点移植后回填实现 + 对应行为级测试（`eval_no_substitution_template` / `eval_template_with_number_span` / `eval_template_span_none_short_circuits`）。
- 其余无。entity 求值（`evaluateEntity`）由 checker 提供，不在本包范围内。
