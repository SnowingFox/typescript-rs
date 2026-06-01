# checker: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 个测试文件 / 3 个 `func`（2 个 `Test*` + 1 个 `Benchmark`）。**这是 5.9 万行代码的全部包内单测**——checker 的正确性**几乎完全靠 P10 conformance/fourslash/`.d.ts` baseline 端到端对拍**。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/checker/checker_test.go` | `internal/checker/tests/checker.rs`（集成：用 `compiler`/`tsoptions`/`vfstest` 起 program） | `TestGetSymbolAtLocation`（1）+ `BenchmarkNewChecker`（→ P10 性能） |
| `internal/checker/tracer_test.go` | `internal/checker/tracer.rs`（`#[cfg(test)] mod tests`，内部包，需访问 `NewTracer`/`testTraceEvent`） | `TestTracerPushPreservesEndArgMutations`（1） |

## `checker_test.go`

### `TestGetSymbolAtLocation`（单块，端到端）

> 起一个内存 program（`vfstest` + `bundled.WrapFS` + `compiler.NewProgram`），bind 后取 checker，对 3 个节点（interface 名、变量名、属性访问）各 `GetSymbolAtLocation`，断言均非空。这是 checker 第一个 tracer bullet（子阶段 **4b** 收口判据）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `get_symbol_at_location_resolves_interface_var_and_property` | 三类节点都能取到非空符号 | 源码 `interface Foo{bar:string} declare const foo:Foo; foo.bar;`；cfg(test) `StubProgram`（parse+bind）后对 `[interfaceId, varId, propAccess]` 各 `get_symbol_at_location(node, None)` → 均 `Some(symbol)` | `checker_test.go:TestGetSymbolAtLocation` | ✓ (4b, 桩 program) |
| `get_symbol_at_location_returns_expected_symbol_names` | 三节点解析到正确符号 | 同上 → interfaceId→`Foo`、varId→`foo`、propAccess→`bar` | `checker_test.go:TestGetSymbolAtLocation`（符号身份） | ✓ (4b) |
| `TestGetSymbolAtLocation`（真 program 版） | 同上，但用 `compiler.NewProgram` + host + tsconfig 起多文件 program | 同 Go 字面量 | `checker_test.go:TestGetSymbolAtLocation` | — (P6) |

> **4b 实测**：核心路径已用 cfg(test) `StubProgram`（`tsgo_parser` 解析 + `tsgo_binder` 绑定单文件，parser/binder 为 dev-dep）跑通并全绿——声明名走 `get_symbol_of_declaration`，`foo.bar` 走结构化属性解析（resolve 接收者 → 读其类型注解 `TypeReference` → 解析为接口符号 → 在 `members` 查 `bar`）。**真多文件 program 版**（`compiler`(P6)/`tsoptions`(P6)/`bundled`(P9)/`vfstest`(P1)）标 `—`(P6)。属性访问的完整类型化解析（`checkPropertyAccessExpression`）见 impl.md 4b DEFER（4d/4g）。

## `tracer_test.go`

### `TestTracerPushPreservesEndArgMutations`（单块）

> 验证 `Tracer.Push` 返回的 `pop()` 闭包在结束时**重新读取 args**（end 阶段对 args 的后续修改要体现到 trace），且 `Push` 注入 `checkerId` 但不污染调用方的 args map（begin 事件不含调用方后加的 `variances`，end 事件含）。子阶段 **4a** 收口判据。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `tracer_push_preserves_end_arg_mutations` | begin/end 事件的 args 快照语义 + checkerId 注入 | 内存 vfs + `start_tracing(fsys,"/trace","",deterministic=true)`；`tracer=new_tracer(tr, 7)`；`pop = tracer.push(CheckTypes,"getVariancesWorker",args{id:1},true)`；push 后 `args` 不含 `checkerId`；改 `args["variances"]=["out"]` 后 `pop()`；`args` 仍不含 `checkerId`（未污染调用方）；停 trace 读 `/trace/trace.json` → begin 事件 `checkerId==7.0` 且 `variances==null`；end 事件 `checkerId==7.0` 且 `variances==["out"]` | `tracer_test.go:TestTracerPushPreservesEndArgMutations` | — (后续 checker 轮次) |
| `copy_with_checker_index_*`（4a 已落地） | checkerId 注入的可移植不变量：注入 `checkerId` 不污染调用方 args、保留既有项、覆盖既有 `checkerId` | `Tracer::new(7).copy_with_checker_index(&{id:Int(1)})` → `{id:Int(1), checkerId:Int(7)}`，输入 map 仍不含 `checkerId` | `tracer.go:copyWithCheckerIndex` / `tracer_test.go`（不变量子集） | ✓ |

> **更正（4a 实测）**：`TestTracerPushPreservesEndArgMutations` 的端到端 round-trip **本轮不可直接收口**，但**不是因为 P1 依赖未就绪**（`tsgo_tracing`/`vfstest`/`tsgo_json` 均已就绪）。根因是 `tsgo_tracing::Tracing::push` 以**值**接收 `args` 并在 push 时快照，无法复现 Go `map[string]any` 的**共享可变**别名语义（Push 后、pop 前对 args 的修改要体现到 end 事件）。忠实复现需改 `tsgo_tracing`（越出 checker crate 边界）或在 checker 侧引入共享可变 args + 独立 begin/end 写入。本轮在 `tracer_test.rs` 移植了**可移植的不变量子集**（checkerId 注入语义，4 个 `#[test]` 全绿），完整 round-trip 推迟到后续 checker 轮次。

## 正确性主体：P10 conformance 兜底（DEFER）

checker 的真正测试是 TypeScript 的 conformance 套件。按子阶段建议，每个子阶段在其 worklog 里登记"本阶段应让哪些 conformance 目录转绿"，作为该子阶段的真实收口判据。建议映射（目录名相对 `tests/cases/conformance/`）：

| 子阶段 | 建议覆盖的 conformance 目录（示例） | 完成 |
|---|---|---|
| 4c 声明类型 | **跟踪切片**：`types/members/`(属性访问)、`interfaces/interfaceDeclarations/`(最小：无继承/泛型/索引) ；后续 `classes/`(声明)、`enums/`、`typeAliases/` | — (P10；4c 路径已由包内单测覆盖) |
| 4d 实例化+关系 | **跟踪切片**：`types/typeRelationships/`(赋值/可比较/恒等最小)、`generics/`(泛型接口+type 实参最小)、`types/typeParameters/` | — (P10；4d 关系/实例化/泛型路径已由包内单测覆盖) |
| 4e 推断 | **跟踪切片**：`types/typeInference/`(实参→类型参数最小)、`generics/`(泛型函数调用推断、`Foo<T>` 成员实例化) | — (P10；4e 推断/成员实例化路径已由包内单测覆盖) |
| 4f 控制流 | **跟踪切片**：`controlFlow/`(单层 `if` + `typeof`/truthiness 收窄、可达性)、`narrowing/`(typeof/`in`/字面量 equality) | — (P10；4f flow walk + 收窄原语路径已由包内单测覆盖) |
| 4g 表达式/语句 | **跟踪切片**：`expressions/`(字面量・标识符・属性/元素访问)、`salsa`/`diagnostics`(Cannot_find_name 2304 / Property_does_not_exist 2339) | — (P10；4g 表达式类型 + 两类诊断 + 调用解析起步已由包内单测覆盖) |
| 4h JSX | **跟踪切片**：`jsx/`(内在元素解析、未知标签 2339、属性可赋值性 2322、值元素名解析 2304、children 类型化) | — (P10；4h JSX 元素/属性/children 检查已由包内单测覆盖) |
| 4i 语法检查 | **跟踪切片**：`grammar*`(修饰符重复 1030 / `declare async` 1040 / 可访问性 1028) / parser-error baseline | — (P10；4i 修饰符 grammar 已由包内单测覆盖) |
| 4j node builder/序列化 | **跟踪切片**：类型打印（`*.types` baseline 的命名/引用/匿名/union 文本）；后续 `declarationEmit/`（`.d.ts` baseline）、quickinfo fourslash | — (P10；4j `type_to_string` 命名/引用/匿名/union + 诊断真名已由包内单测覆盖) |
| 4k emit resolver | **跟踪切片**：`emitResolver`/`declarationEmit`（可见性 / 类型序列化 / overload 实现判定）；declaration transformer baseline（经 P5） | — (P10；4k emit-resolver 可达核心已由包内单测覆盖) |
| 4m 变量声明赋值性 | **跟踪切片**：`types/typeRelationships/assignmentCompatibility/`、`variableDeclarations/`（注解 vs 初始化器 2322 + literal 源广义化文本） | — (P10；4m 变量声明赋值性 2322 + block 递归已由包内单测覆盖) |
| 4n 赋值表达式 + 语句容器递归 | **跟踪切片**：`types/typeRelationships/assignmentCompatibility/`（赋值表达式 `x=y` 2322）、`controlFlow/`、`statements/`（if/while/do/for/try 体内诊断浮现） | — (P10；4n 赋值表达式 2322 + 语句容器递归已由包内单测覆盖) |
| 4o 非赋值二元运算符 + switch/for-in/for-of | **跟踪切片**：`expressions/binaryOperators/`（关系/相等/算术 2362/2363/2365/2367）、`controlFlow/`、`statements/`（switch/for-in/for-of 体内诊断浮现） | — (P10；4o 关系/相等/算术运算符诊断 + switch/for-in/for-of 递归已由包内单测覆盖) |
| 4p 逻辑/`+`/复合赋值运算符 + throw/labeled | **跟踪切片**：`expressions/binaryOperators/`（`+`/逻辑/复合赋值结果类型 + 2365/2322/2362）、`statements/throwStatements/`、labeled（体内诊断浮现） | — (P10；4p `&&`/`\|\|`/`??`/`+` 结果类型 + 复合赋值 2322/2362 + throw/labeled 递归已由包内单测覆盖；ES-symbol/strictNullChecks/return/with DEFER) |
| 4q 调用实参检查（实参数 2554 + 实参类型 2345） | **跟踪切片**：`expressions/functionCalls/`、`expressions/callExpression/`、`functions/`（实参数/可选参数/实参类型不匹配最基础用例） | — (P10；4q 单文件函数声明 + 调用的实参数 2554 / 实参类型 2345 + 可选参 min/max + 返回类型已由包内单测覆盖；重载/泛型推断/rest·spread/this/上下文回调/`new` DEFER) |

> P10 对拍方式：以 Go 的 `tsc` baseline（`.errors.txt` / `.types` / `.d.ts`）为 ground truth，Rust checker 产出逐字节/逐诊断对齐（诊断顺序经稳定排序）。

## 4c 声明类型行为单测（§8.6，每函数一测）

> Go 侧无对应 `func Test*`（这些是声明类型的内部 helper）；按 §8.6 用 cfg(test) `StubProgram`（parse+bind）以 Go 已知语义做 expected，全部 `✓`。

| Rust 测试（`core/declared_types_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `declared_interface_type_exposes_members` | 接口声明类型是带成员的 object type；属性查找+缓存 | `interface Foo{bar:string}` → `get_declared_type_of_symbol(Foo)` 为 object；`get_property_of_type(_, "bar")`→`bar`；二次调用同 id | ✓ |
| `type_of_value_symbol_resolves_annotation_to_declared_type` | 值符号类型=注解声明类型 | `declare const foo:Foo` → `get_type_of_symbol(foo)` == `get_declared_type_of_symbol(Foo)`；属性 `bar` 经类型解析 | ✓ |
| `type_from_type_node_maps_keyword_types` | keyword 类型节点→intrinsic | `var x:number` 的注解 → `number_type` | ✓ |
| `declared_type_of_type_alias_resolves_rhs` | type-alias→RHS type node | `type T=number` → `number_type` | ✓ |
| `declared_type_of_enum_exposes_members` | enum→`exports` 成员的 object type（简化） | `enum E{A}` → object；`get_property_of_type(_, "A")`→`A` | ✓ |
| `apparent_type_is_identity_in_4c` | apparent 在 4c 为恒等 | `get_apparent_type(string)`==`string` | ✓ |
| `property_of_primitive_is_none` | primitive 无自有成员（4c） | `get_property_of_type(string,"length")`==`None` | ✓ |
| `get_global_type_resolves_builds_and_caches` | 在 globals 表解析+建声明类型+缓存；非类型/未知名→None | globals{Foo,foo} → `get_global_type("Foo")`=object（缓存）；`"foo"`/`"Missing"`→None | ✓ |
| `core/signatures_test.rs`（6 个） | `SignatureFlags` 位值/掩码、`Signature`/`IndexInfo` 默认、arena alloc/get、id index | 取自 `types.go` 字面量 | ✓ |

## 4d 实例化 / 关系 / 泛型行为单测（§8.6）

> 小手搭类型（type 参数 + mapper）或 `StubProgram` 接口，expected 取自 Go relater/instantiate 语义，全部 `✓`。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `mapper_test.rs::type_mapper_new_picks_simple_or_array` | 工厂选 Simple/Array | 1 对→Simple；多对→Array | ✓ |
| `mapper_test.rs::map_type_{simple_and_array,merged_composes,composite_reinstantiates,function}` | `TypeMapper.Map` 各变体 | Simple/Array 查表；Merged `m2(m1(t))`；Composite 变更则 re-instantiate；Function `f(t)` | ✓ |
| `mapper_test.rs::instantiate_type_substitutes_type_parameter` | 类型参数替换 + 非变量恒等 | `T`(tp) 经 `T→number` → number；`string` 不变 | ✓ |
| `mapper_test.rs::instantiate_type_maps_union_members` | union 递归实例化 | `{T\|number}` with `T→string` → `string\|number` | ✓ |
| `mapper_test.rs::instantiate_type_remaps_type_reference_arguments` | 泛型引用实参重映射 | `Ref<T>` with `T→string` → `Ref<string>` | ✓ |
| `mapper_test.rs::instantiate_signature_maps_return_type` | 签名返回类型实例化 | 返回 `T` with `T→number` → number；`target` 记录 | ✓ |
| `relations_test.rs::assignable_top_and_bottom` | any/unknown/never | string→any/unknown true；never→string true；string→never false；any→string true | ✓ |
| `relations_test.rs::assignable_literal_to_primitive` | literal→primitive | "a"→string true；"a"→number false；false→boolean true；false→number false | ✓ |
| `relations_test.rs::identity_of_intrinsics` | 恒等 | string≡string true；string≡number false；any≡any true | ✓ |
| `relations_test.rs::assignable_unions` | union source/target | member→union true；union→union true；union→member false | ✓ |
| `relations_test.rs::comparable_is_bidirectional` | 可比较双向 | "a"↔string true；string↔number false | ✓ |
| `relations_test.rs::assignable_structural_objects` | 结构化对象赋值 | `A{x:number}`↔`B{x:number}` true；`A`→`C{x:string}` false | ✓ |
| `relations_test.rs::assignable_structural_subset` | 缺属性 | `Q{x,y}`→`P{x}` true；`P`→`Q` false | ✓ |
| `relations_test.rs::relation_cache_get_set` | 缓存键（含 kind） | set/get/len；不同 kind 不同键 | ✓ |
| `relations_test.rs::comparable_is_lenient_about_optional_vs_required`（4u 切片3） | 可比较对 optional 宽松 | `S{a?}`→`T{a}`：assignable false、comparable true（`skipOptional`） | ✓ |
| `check_test.rs::missing_optional_target_property_is_assignable`（4u 切片1） | 缺失可选 target 属性 | `S{x}`→`T{x;a?}`：`var t:T=s` 无 `2322`（`requireOptionalProperties=false`） | ✓ |
| `check_test.rs::optional_source_property_not_assignable_to_required_target`（4u 切片2） | 可选 source→必需 target | `S{a?}`→`T{a}`：`var t:T=s` 报 `2322`（optional-in-source/required-in-target） | ✓ |
| `declared_types_test.rs::declared_interface_inherits_extends_members` | extends 继承成员 | `Derived extends Base` → 查到自有 `b` 与继承 `a` | ✓ |
| `declared_types_test.rs::generic_interface_records_type_parameters` | 泛型接口记录类型参数 | `interface Box<T>` → `type_parameters.len()==1`，有 `this_type` | ✓ |
| `declared_types_test.rs::type_reference_with_arguments_resolves_member` | `Foo<Args>` 引用 | `Box<string>` → target+args=[string]；经引用查到 `value` | ✓ |

## 4e 推断行为单测（§8.6）

> 小手搭泛型类型/签名（或 `StubProgram` 接口）+ 注入，expected 取自 Go inference 语义，全部 `✓`。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `inference_test.rs::inference_context_and_info_construction` | 上下文/槽/优先级构造 | `InferenceContext::new([T1,T2])`→2 槽；`InferenceInfo::new`空候选；`priority==NONE` | ✓ |
| `inference_test.rs::infer_bare_type_parameter` | bare type-param | infer `number` vs `T` → `[number]` | ✓ |
| `inference_test.rs::infer_from_generic_reference_arguments` | 同构泛型引用 | `Box<string>` vs `Box<T>` → `[string]` | ✓ |
| `inference_test.rs::infer_from_union_target` | union target | `string` vs `string\|T` → `[string]` | ✓ |
| `inference_test.rs::infer_from_object_members` | object 成员匹配 | `{x:number}` vs `{x:T}`(注入) → `[number]` | ✓ |
| `inference_test.rs::infer_with_no_candidates_yields_unknown` | 无候选默认 | 无实参 → `[unknown]` | ✓ |
| `inference_test.rs::infer_multiple_candidates_best_common` | 多候选 best-common | `[number,number]`→`number`；`[number,string]`→`string\|number` | ✓ |
| `inference_test.rs::get_inference_mapper_builds_array` | 推断 mapper | infer `number`→`T` → Array{[T],[number]} | ✓ |
| `inference_test.rs::infer_then_instantiate_signature_return` | 调用闭环 | `<T>(x:T):T` + 实参 number → 实例化返回 `number` | ✓ |
| `inference_test.rs::best_common_type_dominator_or_union` | best-common | `[number,number]`→number；`["a",string]`→string | ✓ |
| `inference_test.rs::subtype_reduce_removes_subsumed` | 子类型去重 | `[number,string]`→不变；`["a",string]`→`[string]` | ✓ |
| `declared_types_test.rs::type_of_property_through_reference_is_instantiated` | 成员经引用实例化 | `Box<string>.value`→`string`；裸 `Box.value`→`T` | ✓ |
| `declared_types_test.rs::declared_type_of_type_parameter_symbol` | 类型参数声明类型 | `Box<T>` 的 T 符号 → type-parameter 类型 | ✓ |

## 4f 控制流 / 收窄行为单测（§8.6）

> tracer bullet 先行（`narrow_type_by_typeof` 恒等桩看红→实现转绿）；其余以手搭 union / `StubProgram`（parse+bind 真 binder flow 图）+ Go flow 语义 expected，全部 `✓`。

| Rust 测试（`core/flow_test.rs`，除标注外） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `narrow_typeof_string_on_string_or_number` | `typeof` 收窄（tracer bullet） | `string\|number` by `"string"`(t)→`string`、(f)→`number`；by `"number"`(t)→`number` | ✓ |
| `narrow_truthiness_boolean_and_nullable` | truthiness 去 falsy | `boolean`(t)→`true`、(f)→`false`；`string\|undefined`(t)→`string` | ✓ |
| `narrow_equality_literal_union` | 字面量 equality（按值） | `"a"\|"b"` `=== "a"`(t)→`"a"`、(f)→`"b"`（value 比较，非 id） | ✓ |
| `narrow_in_object_union` | `in` 属性存在过滤 | `A{a}\|B{b}` `"a" in x`(t)→`A`、(f)→`B` | ✓ |
| `flow_typeof_narrows_in_then_branch` | flow walk 端到端 | `if (typeof x === "string"){ x }` 内 `x`（declared `string\|number`）→`string` | ✓ |
| `flow_no_condition_returns_declared` | 无条件→declared | `x;`（无 guard）→`string\|number` | ✓ |
| `reachable_flow_node_after_if` | 可达性（LABEL 路径） | `if(cond){} x;` 后 `x` 的 flow 节点 → reachable=true | ✓ |
| `flow_equality_narrows_literal_union` (4g) | flow walk 中 `x === <expr>` | `if (x === "a"){ x }` 内 `x`（declared `"a"\|"b"`）→`"a"` | ✓ |
| `narrow_truthiness_drops_empty_string_literal` (4g) | truthiness 经 `TypeFacts` | `"" \| "a"`(t)→`"a"` | ✓ |
| `program_test.rs::bound_program_exposes_flow_nodes` | `BoundProgram` flow 访问器 | `var x; x;` 的 `x` 用法 → `flow_node_of`=Some；`flow_node` 非 UNREACHABLE | ✓ |

## 4g 表达式 / 语句检查 / 诊断行为单测（§8.6）

> tracer bullet 先行（`check_expression` error-type 桩看红→identifier 实现转绿）；以 `StubProgram`（parse+bind）+ 手搭签名，expected 取自 Go check/诊断语义，全部 `✓`。

| Rust 测试（`core/check_test.rs`，除标注外） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `check_identifier_yields_declared_type` | identifier→声明类型（tracer bullet） | `declare const x: string; x;` → `string` | ✓ |
| `undefined_identifier_reports_cannot_find_name` | 未定义名诊断 | `y;` → 1 诊断 code 2304 "Cannot find name 'y'." | ✓ |
| `defined_identifier_reports_no_diagnostics` | 合法→零诊断 | `declare const x: string; x;` → `[]` | ✓ |
| `check_literal_expressions` | 字面量类型 | `"a"`→`"a"`；`1`→`1`；`true`→`true`；`null`→`null` | ✓ |
| `check_property_access_yields_member_type` | 属性访问 | `foo.bar`（`foo: Foo{bar:string}`）→`string` | ✓ |
| `missing_property_reports_diagnostic` | 缺属性诊断 | `foo.baz` → 1 诊断 code 2339 "Property 'baz' does not exist on type…" | ✓ |
| `check_element_access_string_index` | 字符串字面量索引（命名属性） | `foo["bar"]`（`bar` 属性）→ `string` | ✓ |
| `check_element_access_number_index_signature` (4ac) | 数字索引签名 | `Box { [n: number]: string }` + `b[0]` → `string` | ✓ |
| `check_element_access_string_index_signature` (4ac) | 字符串索引签名 | `Dict { [k: string]: number }` + `d["x"]` → `number` | ✓ |
| `check_element_access_string_index_with_variable_key` (4ac) | 计算 string 索引 | `d[key]`（`key: string`）→ `number` | ✓ |
| `check_element_access_array_element_type` (4ac) | 泛型 Array 元素 | 合成 `Array<T>` + `Array<number>` + `a[0]` → `number` | ✓ |
| `array_type_reference_index_signature_instantiates_element` (4ac, `declared_types_test.rs`) | 索引签名实例化 | `Array<number>` 的 number 索引 value → `number` | ✓ |
| `signatures_of_function_type` | call 签名读取 | 手搭函数 object → `[sig]`；primitive → `[]` | ✓ |
| `return_type_of_nongeneric_and_generic_call` | 调用返回类型（+泛型推断） | `()=>number` 调用→`number`；`<T>(x:T)=>T` 实参 `number`→`number` | ✓ |
| `type_facts_test.rs::type_with_facts_drops_falsy_literal_subtypes` | `get_type_with_facts` | `"" \| "a"` TRUTHY→`"a"`、FALSY→`""` | ✓ |
| `type_facts_test.rs::type_facts_of_primitives_and_literals` | `get_type_facts`/`has_type_facts` | `string`→TRUTHY\|FALSY；`undefined`/`null`→FALSY；`has_type_facts` 为成员 OR | ✓ |

## 4h JSX 检查行为单测（§8.6）

> tracer bullet 先行（`check_jsx_self_closing_element` error 桩看红→内在解析转绿）；以 `parse_and_bind_tsx`（ScriptKind::Tsx）+ 注入的 `JSX.IntrinsicElements` 表，expected 取自 Go jsx 语义，全部 `✓`。

| Rust 测试（`core/jsx_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `check_intrinsic_self_closing_element_resolves` | 内在标签解析（tracer bullet） | 表 `{div}` + `<div/>` → `any`，零诊断 | ✓ |
| `unknown_intrinsic_tag_reports_diagnostic` | 未知内在标签 | 表 `{div}` + `<span/>` → 1 诊断 2339 "Property 'span' does not exist on type 'JSX.IntrinsicElements'." | ✓ |
| `attribute_type_mismatch_reports_diagnostic` | 属性可赋值性 | `div:Attrs{id:string}` + `<div id={1}/>` → 1 诊断 2322 (…not assignable to type 'string') | ✓ |
| `value_element_unresolved_reports_cannot_find_name` | 值元素名解析（经 `check_source_file` 分发） | `<Foo/>`（Foo 未定义）→ 1 诊断 2304 "Cannot find name 'Foo'." | ✓ |
| `element_children_are_typed` | 配对元素 + children | 表 `{div}` + `<div>{y}</div>` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `fragment_children_are_typed` | 片段 + children | `<>{z}</>` → 1 诊断 2304 "Cannot find name 'z'." | ✓ |

## 4i 语法检查（grammar）行为单测（§8.6）

> tracer bullet 先行（`check_grammar_modifiers` no-op 桩看红→重复检测转绿）；经 `check_source_file` 公共入口驱动（含类成员遍历），expected 取自 Go grammarchecks 语义，全部 `✓`。

| Rust 测试（`core/grammar_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `duplicate_modifier_reports_already_seen` | 重复修饰符（tracer bullet） | `export export function f(){}` → 1 诊断 1030 "'export' modifier already seen." | ✓ |
| `async_in_ambient_context_reports_diagnostic` | ambient 冲突 | `declare async function f(){}` → 1 诊断 1040 "'async' modifier cannot be used in an ambient context." | ✓ |
| `duplicate_accessibility_modifier_reports_diagnostic` | 可访问性重复（类成员遍历） | `class C { public private x; }` → 1 诊断 1028 "Accessibility modifier already seen." | ✓ |

### 4s grammar 检查族深化（逐行为 红→绿，每条独立 RED）

> 每条独立 grammar 规则各为一个真红（0 vs 1）；经 `check_source_file` 公共入口驱动，expected 取自 Go grammarchecks 语义。修饰符上下文一律用**类成员/属性/构造器**（顶层/函数位关键字经实测被 parser 解析为标识符，不可达——见 impl.md「4s 落地记录」）。

| Rust 测试（`core/grammar_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `accessibility_modifier_after_static_must_precede` | 可访问性须前于 static（1029） | `class C { static public x = 1; }` → 1 诊断 1029 "'public' modifier must precede 'static' modifier." | ✓ |
| `accessor_modifier_with_readonly_reports_conflict` | `accessor`+`readonly` 冲突（1243） | `class C { readonly accessor x = 1; }` → 1 诊断 1243 "'accessor' modifier cannot be used with 'readonly' modifier." | ✓ |
| `readonly_modifier_on_method_reports_diagnostic` | `readonly` 仅限属性/索引签名（1024） | `class C { readonly m() {} }` → 1 诊断 1024 "'readonly' modifier can only appear on a property declaration or index signature." | ✓ |
| `accessor_modifier_on_method_reports_diagnostic` | `accessor` 仅限属性声明（1275） | `class C { accessor m() {} }` → 1 诊断 1275 "'accessor' modifier can only appear on a property declaration." | ✓ |
| `const_declaration_without_initializer_reports_diagnostic` | `const` 须初始化（1155；ambient 守卫） | `const x;` → 1 诊断 1155 "'const' declarations must be initialized." | ✓ |
| `constructor_with_return_type_annotation_reports_diagnostic` | 构造器返回类型注解（1093） | `class C { constructor(): void {} }` → 1 诊断 1093 "Type annotation cannot appear on a constructor declaration." | ✓ |
| `constructor_with_type_parameters_reports_diagnostic` | 构造器类型参数（1092） | `class C { constructor<T>() {} }` → 1 诊断 1092 "Type parameters cannot appear on a constructor declaration." | ✓ |

## 4j node builder / type→string 行为单测（§8.6）

> tracer bullet 先行（`symbol_to_string` 空串桩看红→实现转绿）；以 `StubProgram`（parse+bind）+ 4c/4e 类型构造，expected 取自 Go typeToString 文本，全部 `✓`。

| Rust 测试（`core/nodebuilder_test.rs`，除标注外） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `symbol_to_string_returns_declaration_name` | 符号名（tracer bullet） | `interface Foo {...}` → `symbol_to_string(Foo)` == "Foo" | ✓ |
| `type_to_string_named_interface` | 命名 interface→名 | `interface Foo { bar: string }` → "Foo" | ✓ |
| `type_to_string_type_reference` | 泛型引用 | `Box<T>` 的 `Box<string>` → "Box<string>" | ✓ |
| `type_to_string_anonymous_object_members` | 匿名 object 成员字面量 | 匿名 `{ value: string }` → "{ value: string; }" | ✓ |
| `type_to_string_union_of_named` | union 递归（程序感知） | `A \| B`（命名）→ "A \| B" | ✓ |
| `type_to_string_intrinsics_and_literals_delegate` | intrinsic/literal 委托 | `string`→"string"；`"x"` 字面量→"\"x\"" | ✓ |
| `check_test.rs::missing_property_reports_diagnostic`(更新) | 诊断用真名 | `foo.baz`（`foo: Foo`）→ "Property 'baz' does not exist on type 'Foo'." | ✓ |

## 4k emit resolver / API 收口行为单测（§8.6）

> tracer bullet 先行（`is_declaration_visible` `false` 桩看红→`export` 规则转绿）；以 `StubProgram`（parse+bind）+ 4j node builder，expected 取自 Go emitresolver 语义，全部 `✓`。

| Rust 测试（`core/emit_resolver_test.rs`，除标注外） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `exported_declaration_is_visible` | 声明可见性（tracer bullet） | `export function f(){}` → true；`function g(){}` → false | ✓ |
| `serialize_type_of_declaration_uses_real_type` | 声明类型序列化（复用 4j） | `declare const x: Foo`（`interface Foo`）→ "Foo" | ✓ |
| `implementation_of_overload_is_detected` | overload 实现判定 | 三个 `foo`（前两个无 body）→ impl(body) true、签名 false；单 `bar` false | ✓ |
| `mod_test.rs::new_checker_initializes_intrinsics`(更新) | `new_checker(Rc<dyn BoundProgram>)` 入口 | 任意 program → intrinsic 与 `Checker::new()` 一致；`string`→"string" | ✓ |

## 4l program 保留 + pool 驱动面行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4l 落地记录」S1–S4）；用 `StubProgram`（parse+bind）做真实绑定的小程序，expected 取自 Go `NewChecker`/`checkSourceFile`/`getDiagnostics` 语义，全部 `✓`。这是多-checker pool 的干净驱动面验证。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `mod_test.rs::new_checker_retains_program` | `new_checker` 保留 program（Go `c.program`，tracer bullet） | `declare const x: string;` → `program().root()` == 该文件根 | ✓ |
| `check_test.rs::undefined_identifier_reports_cannot_find_name`(更新) | `check_source_file(file)` 基于保留 program 驱动 → 2304 | `y;` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `check_test.rs::check_source_file_is_idempotent` | 幂等（Go `sourceFileLinks.typeChecked`） | `y;` 两次 `check_source_file` → 仍 1 诊断 | ✓ |
| `check_test.rs::get_diagnostics_triggers_checking` | `get_diagnostics(file)` 自跑 `check_source_file`（Go-faithful） | `y;`（只调 get）→ 1 诊断 2304 | ✓ |
| `check_test.rs::get_diagnostics_drives_property_does_not_exist` | 端到端经纯驱动面（`new_checker`→`get_diagnostics`）→ 2339 | `foo.baz`（`foo: Foo`）→ 1 诊断 2339 "Property 'baz' does not exist on type 'Foo'." | ✓ |
| `check_test.rs::defined_identifier_reports_no_diagnostics`(更新) | 保留 program 检查无误名 → 无诊断 | `declare const x: string;\nx;` → 0 诊断 | ✓ |
| `grammar_test.rs`(3 更新)/`jsx_test.rs`(3 更新) | 高层 grammar/JSX 测试迁移到保留-program 模型 | 经 `new_checker(rc)`+`check_source_file(root)`+`get_diagnostics(root)`，诊断不变 | ✓ |

## 4m 变量声明赋值性 / block 递归行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4m 落地记录」S1–S5）；经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，expected 取自 Go `checkVariableLikeDeclaration`/`reportRelationError`/`checkBlock` 语义，全部 `✓`。新增物均为私有 fn（公开 API 不变）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `variable_initializer_not_assignable_reports_diagnostic` | 不可赋值对象类型初始化器 → 2322（tracer bullet，命名类型） | `interface A{x:number} interface B{x:string} declare const b:B; var a:A=b;` → 1 诊断 2322 "Type 'B' is not assignable to type 'A'." | ✓ |
| `variable_initializer_literal_generalizes_to_base_type` | literal 源在消息里广义化为基础类型 | `var x: number = "s";` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'."（非 `"s"`） | ✓ |
| `variable_initializer_assignable_reports_no_diagnostic` | 可赋值初始化器 → 零诊断（守卫） | `var s: string = "ok"; var n: number = 1;` → `[]` | ✓ |
| `unannotated_variable_initializer_reports_no_diagnostic` | 未注解变量 → 零诊断（守卫；推断 DEFER，目标为 `any`） | `var z = "s";` → `[]` | ✓ |
| `variable_declaration_in_block_is_checked` | block 递归检查嵌套语句（Go `checkBlock`） | `{ var x: number = "s"; }` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |

## 4n 赋值表达式赋值性 / 语句容器递归行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4n 落地记录」S1–S12）；经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，expected 取自 Go `checkBinaryLikeExpression`/`checkAssignmentOperator`/`checkSourceElement` 各语句臂语义，全部 `✓`。新增物均为私有 fn/私有臂（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `assignment_expression_not_assignable_reports_diagnostic` | 赋值表达式 `x=y` 不可赋值 → 2322（tracer bullet，命名类型，错误节点=LHS） | `interface A{x:number} interface B{x:string} declare const a:A; declare const b:B; a = b;` → 1 诊断 2322 "Type 'B' is not assignable to type 'A'." | ✓ |
| `assignment_expression_literal_generalizes_to_base_type` | 赋值表达式 literal 源在消息里广义化 | `declare const n:number; n = "s";` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `assignment_expression_assignable_reports_no_diagnostic` | 可赋值赋值 → 零诊断（守卫） | `interface A{x:number} declare const a:A; declare const a2:A; a = a2;` → `[]` | ✓ |
| `statement_in_if_then_body_is_checked` | `if` then 分支递归（tracer bullet） | `if (true) { y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `statement_in_if_else_body_is_checked` | `if` else 分支递归（守卫） | `if (false) {} else { y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `statement_in_while_body_is_checked` | `while` 体递归（Go `checkWhileStatement`） | `while (true) { y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `statement_in_do_while_body_is_checked` | `do…while` 体递归（Go `checkDoStatement`） | `do { y; } while (true);` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `statement_in_for_body_is_checked` | `for` 体递归（Go `checkForStatement`） | `for (;;) { y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `declaration_in_for_initializer_is_checked` | `for` 初始化器声明列表被检查（守卫） | `for (var x:number="s"; ;) {}` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `statement_in_try_block_is_checked` | `try` 块递归（Go `checkTryStatement`→`checkBlock`） | `try { y; } catch (e) {}` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `statement_in_catch_block_is_checked` | `catch` 子句块递归（Go `checkCatchClause`） | `try {} catch (e) { y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `statement_in_finally_block_is_checked` | `finally` 块递归 | `try {} finally { y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |

## 4o 非赋值二元运算符 / switch·for-in·for-of 递归行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4o 落地记录」S1–S18）；运算符结果类型经 `check_expression` 直接断言（手搭 `declare const` 变量），诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，expected 取自 Go `checkBinaryLikeExpression`/`checkArithmeticOperandType`/`reportOperatorError`/`checkSwitchStatement`/`checkForInStatement`/`checkForOfStatement` 语义，全部 `✓`。新增物均为私有 fn/私有臂（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `relational_operator_yields_boolean` | 关系运算符结果 `boolean`（tracer bullet） | `declare const a:number; declare const b:number; a < b;` → `check_expression`=`boolean` | ✓ |
| `relational_operator_incomparable_reports_diagnostic` | 关系不可比 → 2365 | `declare const s:string; declare const n:number; s < n;` → 1 诊断 2365 "Operator '<' cannot be applied to types 'string' and 'number'." | ✓ |
| `relational_operator_comparable_reports_no_diagnostic` | 关系可比 → 零诊断（守卫） | `number < number` → `[]` | ✓ |
| `equality_operator_yields_boolean` | 相等运算符结果 `boolean` | `a === b`（number）→ `check_expression`=`boolean` | ✓ |
| `equality_operator_no_overlap_reports_diagnostic` | 相等无重叠 → 2367 | `s === n`（string/number）→ 1 诊断 2367 "This comparison appears to be unintentional because the types 'string' and 'number' have no overlap." | ✓ |
| `equality_operator_comparable_reports_no_diagnostic` | 相等可比 → 零诊断（守卫） | `number === number` → `[]` | ✓ |
| `arithmetic_operator_yields_number` | 非 `+` 算术结果 `number` | `a - b`（number）→ `check_expression`=`number` | ✓ |
| `arithmetic_nonnumeric_left_reports_diagnostic` | 算术 LHS 非数值 → 2362 | `declare const s:string; s - 1;` → 1 诊断 2362 "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type." | ✓ |
| `arithmetic_nonnumeric_right_reports_diagnostic` | 算术 RHS 非数值 → 2363 | `declare const s:string; 1 - s;` → 1 诊断 2363 "The right-hand side..." | ✓ |
| `arithmetic_numeric_operands_report_no_diagnostic` | 算术双数值 → 零诊断（守卫） | `number * number` → `[]` | ✓ |
| `statement_in_switch_case_clause_is_checked` | switch case-clause 语句递归（tracer） | `switch (1) { case 2: y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `statement_in_switch_default_clause_is_checked` | switch default-clause 语句递归（守卫） | `switch (1) { default: y; }` → 2304 | ✓ |
| `switch_expression_is_checked` | switch 表达式被检查 | `switch (y) {}` → 2304 | ✓ |
| `switch_case_clause_expression_is_checked` | case-clause 表达式被检查 | `switch (1) { case y: ; }` → 2304 | ✓ |
| `statement_in_for_in_body_is_checked` | for-in body 递归（tracer） | `for (var k in {}) { y; }` → 2304 | ✓ |
| `for_in_expression_is_checked` | for-in 迭代表达式被检查（守卫） | `for (var k in y) {}` → 2304 | ✓ |
| `statement_in_for_of_body_is_checked` | for-of body 递归 | `for (var x of []) { y; }` → 2304 | ✓ |
| `for_of_expression_is_checked` | for-of 迭代表达式被检查（守卫；元素类型化 DEFER） | `for (var x of y) {}` → 2304 | ✓ |

## 4p 逻辑/`+`/复合赋值运算符 / `throw`·labeled 语句行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4p 落地记录」P1–P4/L1–L2/M1–M2/Q1/C1–C6/S1–S2）；运算符结果类型经 `check_expression` 直接断言（手搭 `declare const`/`interface`，对象结果与右操作数 `TypeId` 比对，因 2-arg `type_to_string` 无 program 上下文无法解析命名），诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，expected 取自 Go `checkBinaryLikeExpression`/`checkAssignmentOperator`/`checkThrowStatement`/`checkLabeledStatement` 语义，全部 `✓`。新增物均为私有 fn/私有臂/私有 free fn（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `plus_operator_both_number_yields_number` | `+` 双 number-like → number（tracer） | `declare const a:number; declare const b:number; a + b;` → `check_expression`=`number` | ✓ |
| `plus_operator_string_operand_yields_string` | `+` 任一 string-like → string | `declare const s:string; declare const n:number; s + n;` → `check_expression`=`string` | ✓ |
| `plus_operator_incompatible_reports_diagnostic` | `+` 不可应用 → 2365 | `interface O{x:number} declare const a:O; declare const b:O; a + b;` → 1 诊断 2365 "Operator '+' cannot be applied to types 'O' and 'O'." | ✓ |
| `plus_operator_error_operand_does_not_cascade` | `+` any/error 不级联 2365 | `y + 1;` → 仅 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `logical_or_non_falsy_left_yields_left_type` | `\|\|` 非假左 → 左型（tracer） | `true \|\| 1;` → `check_expression`=`true` 型 | ✓ |
| `logical_or_falsy_left_yields_union` | `\|\|` 可假左 → union | `declare const s:string; declare const n:number; s \|\| n;` → `type_to_string`="string \| number" | ✓ |
| `logical_and_non_truthy_left_yields_left_type` | `&&` 非真左 → 左型（tracer） | `false && 1;` → `check_expression`=`false` 型 | ✓ |
| `logical_and_truthy_left_yields_right_when_falsy_part_empty` | `&&` 可真左 → union（对象右 falsy 部分空→折叠为右型） | `interface O{x:number} declare const a:number; declare const o:O; a && o;` → `check_expression`=右操作数型 | ✓ |
| `nullish_coalesce_non_nullable_left_yields_left_type` | `??` 非 nullable 左 → 左型（tracer；nullish 精化 DEFER） | `declare const s:string; declare const n:number; s ?? n;` → `check_expression`=`string` | ✓ |
| `compound_arithmetic_assignment_checks_operand` | 复合算术操作数检查 → 2362（tracer） | `declare const s:string; s *= 1;` → 1 诊断 2362 "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type." | ✓ |
| `plus_equals_assignment_not_assignable_reports_diagnostic` | `+=` 结果不可赋值 → 2322 | `declare const n:number; n += "s";` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `logical_and_equals_assignment_not_assignable_reports_diagnostic` | `&&=` 右值不可赋值 → 2322 | `declare const n:number; n &&= "s";` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `logical_or_equals_assignment_not_assignable_reports_diagnostic` | `\|\|=` 右值不可赋值 → 2322 | `declare const n:number; n \|\|= "s";` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `nullish_coalesce_equals_assignment_not_assignable_reports_diagnostic` | `??=` 右值不可赋值 → 2322 | `declare const n:number; n ??= "s";` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `plus_equals_assignment_assignable_reports_no_diagnostic` | `+=` 可赋值 → 零诊断（守卫） | `declare const n:number; n += 1;` → `[]` | ✓ |
| `throw_statement_expression_is_checked` | throw 表达式被检查（tracer） | `throw y;` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `labeled_statement_body_is_checked` | labeled 递归 | `lbl: y;` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |

## 4q 调用表达式实参检查（实参数 2554 + 实参类型 2345）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4q 落地记录」S1–S5 + 守卫）；用 `StubProgram`（parse+bind）做真实绑定的小程序——顶层 `function f(...)` 声明 + `f(...)` 调用，诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，返回类型经 `check_expression(call)` 直接断言，expected 取自 Go `checkCallExpression`/`isSignatureApplicable`/`getArgumentArityError`/`getSignatureFromDeclaration` 语义，全部 `✓`。新增物均为私有 fn/私有臂/私有 free fn（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `call_argument_not_assignable_reports_diagnostic` | 实参类型不可赋值 → 2345（tracer bullet；literal 源广义化） | `function f(a: number) {} f("s");` → 1 诊断 2345 "Argument of type 'string' is not assignable to parameter of type 'number'." | ✓ |
| `call_too_few_arguments_reports_diagnostic` | 实参过少 → 2554（报在 callee） | `function f(a: number) {} f();` → 1 诊断 2554 "Expected 1 arguments, but got 0." | ✓ |
| `call_too_many_arguments_reports_diagnostic` | 实参过多 → 2554（报在多余实参） | `function f(a: number) {} f(1, 2);` → 1 诊断 2554 "Expected 1 arguments, but got 2." | ✓ |
| `call_optional_parameter_allows_fewer_arguments` | 可选参 `?` 降低 min → 零诊断（守卫） | `function f(a: number, b?: number) {} f(1);` → `[]` | ✓ |
| `call_optional_parameter_too_many_reports_range` | 可选参 → 2554 的 `min-max` 范围 | `function f(a: number, b?: number) {} f(1, 2, 3);` → 1 诊断 2554 "Expected 1-2 arguments, but got 3." | ✓ |
| `call_result_type_is_signature_return_type` | 调用结果 = 签名返回类型 | `function f(a: number): string { return ""; } f(1);` → `check_expression`=`string` | ✓ |
| `call_well_typed_reports_no_diagnostic` | 正确调用 → 零诊断（守卫） | `function f(a: number) {} f(1);` → `[]` | ✓ |

## 4r 重载解析（2769/2575）+ 类成员体/属性初始化器 + 函数体下传/return 检查 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4r 落地记录」A1–A4/B1–B4/C1–C5；每 item 的 tracer——A1/B1/C1——均实测 red：A1 旧路径报 2345≠2769、B1/C1 旧路径 0 诊断；其余臂随同 item 的内聚函数一并落地，作分支覆盖/守卫）。用 `StubProgram`（parse+bind）小程序，诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，expected 取自 Go `resolveCall`/`reportCallResolutionErrors`/`getArgumentArityError`/`checkClassMember`/`checkPropertyDeclaration`/`checkReturnStatement` 语义，全部 `✓`。**诊断码按 Go ground truth**：重载实参类型均不符 → **2769**（交办单误标 2575）；重载实参数落区间内不匹配 → **2575**。新增物均为私有方法/私有臂（公开 API 不变）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `overloaded_call_matching_no_overload_reports_no_overload_matches` | 重载实参类型均不符 → 2769（tracer：red 2345→green 2769） | `declare function f(a: number): void; declare function f(a: string): void; f(true);` → 1 诊断 2769 "No overload matches this call." | ✓ |
| `overloaded_call_matching_an_overload_reports_no_diagnostic` | 有重载适用 → 零诊断（守卫） | `... f("s");` → `[]` | ✓ |
| `overloaded_call_single_arity_match_reports_argument_error` | 仅一个 arity 命中且实参不符 → 该候选 2345（无重载链） | `declare function f(a: number): void; declare function f(a: number, b: number): void; f("s");` → 1 诊断 2345 "Argument of type 'string' is not assignable to parameter of type 'number'." | ✓ |
| `overloaded_call_no_arity_match_reports_arity_error` | 无 arity 命中且落区间内 → 2575 | `declare function f(a:number):void; declare function f(a:number,b:number,c:number):void; declare const n:number; f(n, n);` → 1 诊断 2575 "No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments." | ✓ |
| `class_method_body_is_checked` | 类方法体下传（tracer：red 0→green 2304） | `class C { m() { y; } }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `class_property_initializer_not_assignable_reports_diagnostic` | 属性初始化器不可赋值 → 2322（字面量广义化） | `class C { x: number = "s"; }` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `class_property_initializer_assignable_reports_no_diagnostic` | 属性初始化器可赋值 → 零诊断（守卫） | `class C { x: number = 1; }` → `[]` | ✓ |
| `class_property_unannotated_initializer_reports_no_diagnostic` | 未注解属性（→`any`）→ 零诊断（守卫） | `class C { x = "s"; }` → `[]` | ✓ |
| `return_statement_expression_in_function_body_is_checked` | 函数体下传 + return 表达式被检查（tracer：red 0→green 2304） | `function f() { return y; }` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |
| `return_type_mismatch_with_annotation_reports_diagnostic` | 带注解返回类型不符 → 2322（广义化） | `function f(): number { return "s"; }` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `return_type_assignable_to_annotation_reports_no_diagnostic` | 带注解返回类型可赋值 → 零诊断（守卫） | `function f(): string { return "s"; }` → `[]` | ✓ |
| `return_in_unannotated_function_reports_no_return_type_diagnostic` | 未注解函数 → 返回类型检查 DEFER（守卫，无误报） | `function f() { return "s"; }` → `[]` | ✓ |
| `return_type_mismatch_in_method_body_reports_diagnostic` | 方法体内 return 对方法显式返回类型（B+C 组合） | `class C { m(): number { return "s"; } }` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |

## 4v 交叉类型 `TypeData::Intersection` 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4v 落地记录」切片 1–4；tracer 均实测 red：切片1 `todo!()` panic、切片2 节点得 `error_type`、切片3 缺 intersection 处理 → 误判、切片4 消息印 `{ ... } & { ... }`）。构造/interning 经公开 `Checker::get_intersection_type` 直接断言，关系经公开 `is_type_assignable_to`，端到端诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，命名打印经 `nodebuilder::type_to_string`。expected 取自 Go `getIntersectionType`/`addTypeToIntersection`/`getTypeFromIntersectionTypeNode`/`typeRelatedToEachType`/`someTypeRelatedToType` 语义，全部 `✓`。公开 API 仅做加法（新增 `TypeData::Intersection` 变体 + `IntersectionType` + `Type::intersection_types()` + `Checker::get_intersection_type`），未改既有签名。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `mod_test.rs::get_intersection_type_interns_by_members`（切片1） | 构造 + interning 身份 | `[A,B]` 两次（含 `[B,A]`）→ 同 `TypeId`，flags `INTERSECTION`，成员 `[A,B]` | ✓ |
| `mod_test.rs::get_intersection_type_trivial_reductions`（切片1） | trivial reduction | `[]`→`unknown`；`[A]`→`A`；`[A,unknown]`→`A`；`[A,never]`→`never`；`[A,any]`→`any` | ✓ |
| `mod_test.rs::get_intersection_type_flattens_and_dedups`（切片1） | 扁平化 + 去重 | `[A,A,B]`==`[A,B]`；`[(A&B),C]`==`[A,B,C]`（成员 `[A,B,C]`） | ✓ |
| `declared_types_test.rs::type_from_type_node_resolves_intersection`（切片2） | `A & B` 类型节点 → intersection | `var i: A & B` 注解 → flags `INTERSECTION`，成员 = sorted `[declared A, declared B]` | ✓ |
| `relations_test.rs::assignable_to_target_intersection_requires_each_constituent`（切片3） | target intersection = 每个成员 | `AB{x,y}`→`A&B` true；`A`→`A&B` false（缺 `y`） | ✓ |
| `relations_test.rs::assignable_from_source_intersection_needs_some_constituent`（切片3） | source intersection = 任一成员 | `A&B`→`A` true；`A&B`→`B` true | ✓ |
| `check_test.rs::variable_initializer_not_assignable_to_intersection_reports_diagnostic`（切片4） | 端到端 2322 + 命名打印 | `declare const a:A; var v:A & B = a;` → 1 诊断 2322 "Type 'A' is not assignable to type 'A & B'." | ✓ |
| `check_test.rs::variable_initializer_assignable_to_intersection_reports_no_diagnostic`（切片4） | 可赋值 → 零诊断（守卫） | `declare const ab:A & B; var v:A & B = ab;` → `[]` | ✓ |
| `nodebuilder_test.rs::type_to_string_intersection_of_named`（切片4） | intersection 命名递归打印 | `A & B`（命名接口）→ "A & B" | ✓ |

## 4w 合成交叉属性 + union 分配律 + source-intersection 结构化回退 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4w 落地记录」切片 1–4）。tracer 均实测 red：切片1 `get_property_of_type(A&B,…)` 返回 `None`、切片2 `A&B → AB` 不可赋值（误 false）、切片3 `X&(A|B)` 得 intersection 而非分配后 union。切片4 为端到端守卫（slices 1+2 通过 `get_diagnostics` 公开面复现"2322 不应触发"）。属性解析经公开 `get_property_of_type`/`get_properties_of_type`+`get_type_of_symbol`，构造经公开 `Checker::get_intersection_type`，关系经 `is_type_assignable_to`，端到端经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。expected 取自 Go `getPropertyOfUnionOrIntersectionType`(intersection 分支)/`getPropertiesOfUnionOrIntersectionType`/`getCrossProductIntersections`/`structuredTypeRelatedTo`(source-intersection 回退) 语义，全部 `✓`。**未新增/未改任何 `pub fn` 签名**（仅 `get_property_of_type`/`get_properties_of_type`/`get_intersection_type`/`structured_type_related_to` 的体扩展 + 两个私有 helper）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `declared_types_test.rs::intersection_synthesizes_properties_of_each_constituent`（切片1，tracer red→green） | intersection 属性解析（每个成员的属性都浮现，类型取该成员版本） | `interface A{a:number} interface B{b:string}` + `A&B` → `get_property_of_type("a")`/`("b")` 均 Some，类型 `number`/`string`；`"nope"`→None；`get_properties_of_type` 名集 `["a","b"]` | ✓ |
| `relations_test.rs::source_intersection_relates_structurally_to_object`（切片2，tracer red→green） | source intersection 经合成属性结构化回退 | `A{x} & B{y}` → `AB{x,y}` true；`A` → `AB` false（缺 `y`） | ✓ |
| `mod_test.rs::get_intersection_type_distributes_over_union`（切片3，tracer red→green） | union 分配律（cross-product） | `X & (A|B)` == `(X&A) | (X&B)`，且结果 flags `UNION` | ✓ |
| `check_test.rs::intersection_source_assignable_to_object_reports_no_diagnostic`（切片4，端到端守卫） | `A&B → AB` 经 synthesized props → 2322 不触发 | `declare const ab: A & B; var v: AB = ab;` → `[]` | ✓ |

## 4x 合成符号 arena + union 属性合成 + 多成员交叉属性真类型 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4x 落地记录」切片 1–4）。tracer 均实测 red：切片1 `A | B` 类型节点得 `error_type`（flags ANY）、切片2 `A&B` 的多成员属性 `a` 类型得首成员 `X`（非 `X&Y`）、切片3 `u.a`（union）得 `error_type`（union 属性未解析）、切片4 union 缺一成员属性仍解析（0 诊断而非 2339）。类型节点解析经公开 `get_type_from_type_node`，属性解析/类型经公开 `get_property_of_type`+`get_type_of_symbol`，端到端经 `check_expression` 与纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。expected 取自 Go `getTypeFromUnionTypeNode`/`createUnionOrIntersectionProperty`(union+intersection 分支)/`getPropertyOfUnionOrIntersectionType`(partial 过滤) 语义，全部 `✓`。**未新增/未改任何 `pub fn` 签名**（合成符号 arena 全为 `pub(crate)` + 私有 helper；`get_property_of_type`/`get_type_of_symbol`/`get_type_from_type_node` 体扩展）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `declared_types_test.rs::type_from_type_node_resolves_union`（切片1，tracer red→green） | `A \| B` 类型节点 → union | `var u: A \| B` 注解 → flags `UNION`，interns 同 `get_union_type([A,B])` | ✓ |
| `declared_types_test.rs::intersection_multi_constituent_property_has_intersected_type`（切片2，tracer red→green） | 多成员交叉属性真类型（合成符号 arena） | `interface X{p} Y{q} A{a:X} B{a:Y}` + `A&B` → `get_property_of_type("a")` 合成符号，类型 == `get_intersection_type([X,Y])`，flags `INTERSECTION` | ✓ |
| `check_test.rs::check_property_access_on_union_yields_union_of_member_types`（切片3，tracer red→green） | union 属性合成（全成员都有 → 类型并集） | `interface A{a:number} B{a:string} type U=A\|B; declare const u:U; u.a;` → `check_expression` == `number \| string` | ✓ |
| `check_test.rs::union_property_absent_from_one_constituent_reports_2339`（切片4，端到端 red→green） | union partial 过滤（缺一成员 → 不浮现） | `interface A{a:number} C{b:string} type U2=A\|C; declare const u2:U2; u2.a;` → 1 诊断 2339 | ✓ |

## 4y 合成属性 optional 标志传播 + disjoint-domain 交叉归约 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4y 落地记录」切片 A/B/D）。tracer 均实测 red：切片A `A|B`（`a` 在 B 为 `a?`）的 `a` 合成符号经 `resolved_symbol_flags` 不含 `OPTIONAL`、切片B `A&B`（`a?` 在两成员）的 `a` 合成符号不含 `OPTIONAL`、切片D `get_intersection_type([string,number])` 得 2 成员 intersection（非 `never`）。optional 探针经合成符号 arena 的 `resolved_symbol_flags`（relations 实际消费面），disjoint 归约经公开 `Checker::get_intersection_type` 直接断言。expected 取自 Go `createUnionOrIntersectionProperty`（`optionalFlag` union OR / intersection AND）与 `getIntersectionTypeEx`（`TypeFlagsDisjointDomains` 守卫，非 strict 子集）语义，全部 `✓`。**未新增/未改任何 `pub fn` 签名**（新增私有 `union_optional_flag`/`intersection_optional_flag`/`is_disjoint_domain_intersection` + `get_union_property`/`get_intersection_property`/`get_intersection_type` 体扩展）。切片 C（readonly 传播）DEFER（缺 `isReadonlySymbol`/修饰符基建 + 无 readonly 关系消费者，见 impl.md）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `declared_types_test.rs::union_property_is_optional_when_optional_in_any_constituent`（切片A，tracer red→green） | union optional 传播（OR） | `interface A{a:number} B{a?:string} C{a:string}` → `A\|B` 的 `a` 合成符号 `OPTIONAL`；`A\|C` 的 `a` 非 `OPTIONAL` | ✓ |
| `declared_types_test.rs::intersection_property_is_optional_only_when_optional_in_all_constituents`（切片B，tracer red→green） | intersection optional 传播（AND） | `interface A{a?:X} B{a?:Y} D{a:X}` → `A&B` 的 `a` 合成符号 `OPTIONAL`；`B&D` 的 `a` 非 `OPTIONAL` | ✓ |
| `mod_test.rs::get_intersection_type_disjoint_domains_reduce_to_never`（切片D，tracer red→green） | disjoint-domain 交叉归约 → `never` | `string&number` / `number&bigint` / `string&boolean` / `symbol&number` / `object&string` → `never`；`string&T`（类型变量）仍 `INTERSECTION` | ✓ |

## 4z 全局符号/类型解析（"lib globals" checker 侧半张）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4z 落地记录」切片 1–3）。tracer 均实测 red：切片1 `get_global_symbol("g", VALUE)` 返回 `None`（`StubProgram` 默认 `globals()`=None）、切片2 `get_global_type("Foo")` 临时桩返回 `None`（恢复 delegation 前实测真红）、切片3 注入 `interface String { length }` 后 `get_apparent_type(string)` 仍=`string`（≠ wrapper）。全程用**合成全局声明**经 `StubProgram::parse_and_bind`（script 根文件 `locals` = 该程序 globals，是 Go `c.globals` 的单文件 stand-in）。符号/类型解析经新 pub 入口 `Checker::get_global_symbol`/`get_global_type`，apparent 映射经公开 `get_apparent_type`/`get_property_of_type`/`get_type_of_symbol`。expected 取自 Go `getGlobalSymbol`（`resolveName(nil,…)` 全局 only）/`getGlobalType`/`getApparentType`（primitive→`globalStringType`）语义，全部 `✓`。**未改任何既有 `pub fn` 签名**（`BoundProgram::globals()` 为 trait 默认方法=加法；`get_global_symbol`/`get_global_type` 为新 pub 方法；`get_apparent_type` 仅体扩展）。真 lib.d.ts 加载仍 DEFER（P6-8）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `mod_test.rs::get_global_symbol_resolves_global_value_by_meaning`（切片1，tracer red→green） | 全局符号解析（读 program globals + meaning 过滤） | `declare var g: number;` → `get_global_symbol("g", VALUE)`=Some(`FUNCTION_SCOPED_VARIABLE`)；`("nope", VALUE)`=None（→2304）；`("g", TYPE)`=None | ✓ |
| `mod_test.rs::get_global_type_resolves_global_interface_off_program`（切片2，tracer red→green） | 全局类型解析（读 program globals + 建声明类型 + 缓存） | `interface Foo{bar:string} declare const foo:Foo;` → `get_global_type("Foo")`=Some(object，二次同 id)；`("foo")`=None；`("Missing")`=None | ✓ |
| `declared_types_test.rs::apparent_type_of_string_maps_to_global_string_wrapper`（切片3，tracer red→green） | apparent-type primitive→全局 `String` wrapper（读 `global_types` 缓存） | `interface String{length:number}` 注入 globals：建 `String` 前 apparent(`string`)=`string`、`get_property_of_type(string,"length")`=None；建后 apparent(`string`)/apparent(string-literal)=wrapper，且 string-literal 上 `length` 解析、类型=`number` | ✓ |

## 4aa 多文件 `BoundProgram` view 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4aa 落地记录」切片 1–3）。tracer 均实测 red：切片1 `MultiFileProgram` 未 override `source_files()` → 默认 1 文件 → `files.len()`==1（期望 2）；切片2 `check_source_file` 未用 `file_view` → 用 encoded 句柄索引 lib arena `index out of bounds` panic；切片3 诊断单一平表 → `get_diagnostics(fileB)` 含文件 A 的 2322。多文件 program 经新 harness `MultiFileProgram::build(&[(name,text)])`（每文件独立 arena + 合并符号空间 + 合并 globals + per-file `FileView`）装配。expected 取自 Go `program.SourceFiles()`/`Checker.globals`（合并各 global 文件 `Locals`）/`getApparentType`（`string`→`String`）/`collection.GetDiagnosticsForFile` 语义，全部 `✓`。新 trait 方法（`file_handle`/`source_files`/`file_view`/`view_for_symbol`）**均为带默认实现的加法**——单文件实现（`StubProgram`/`compiler::BoundFile`）无需改即满足；`check_source_file`/`get_diagnostics`/`new_checker`/`Diagnostic` 名字与签名不变。跨文件同名声明 MERGE、`globalThis`、并行（`Arc`）、真 lib.d.ts 仍 DEFER（见 impl.md「4aa 落地记录」DEFER）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `program_test.rs::multi_file_program_exposes_files_and_merged_globals`（切片1，tracer red→green） | 多文件 view 暴露多文件 + 合并 globals | A=`interface String{length:number}`、B=`declare const s:string;` → `source_files().len()`==2 且两句柄不等；合并 `globals()` 同时含 `String` 与 `s` | ✓ |
| `check_test.rs::cross_file_global_resolves_string_property_via_lib`（切片2，tracer red→green） | 跨文件 global 解析（source 对 lib `String` 解析 `length`） | A=`interface String{length:number}`（lib）、B=`declare const s:string;\ns.length;`（source）→ `get_diagnostics(fileB)` 无诊断（`length` 经合并 globals + apparent type 解析，无 2339） | ✓ |
| `check_test.rs::string_property_without_lib_reports_2339`（切片2 负向控制） | 无 lib 时 string 属性访问报 2339 | 单文件 `declare const s:string;\ns.length;` → `get_diagnostics(root)`=[2339] | ✓ |
| `check_test.rs::get_diagnostics_is_filtered_per_file`（切片3，tracer red→green） | per-file 诊断过滤（`GetDiagnosticsForFile`） | A=`var a:number="x";`（2322）、B=`var b:number=1;`（无）→ `get_diagnostics(fileA)`=[2322]、`get_diagnostics(fileB)`=[]，互不含 | ✓ |

## 4ab `instanceof` / `in` 表达式检查行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ab 落地记录」I1–I4 / N1–N4）。tracer 均实测 red：I1/N1 运算符落入 `_` 臂得 `error_type`（≠ boolean）、I2/I3 缺操作数检查 → 0 诊断、N2/N3 缺操作数检查 → 0 诊断。结果类型经 `check_expression` 直接断言，诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。**合成全局** `interface Function { bind: number }`（程序顶层声明，Go `c.globalFunctionType` 的单文件 stand-in）驱动 `instanceof` 右操作数 2359 检查；`in` 仅用 intrinsics（`string|number|symbol`、`object`）。expected 取自 Go `checkInstanceOfExpression`/`resolveInstanceofExpression`/`checkInExpression` 语义。**码确认**：`instanceof` 用 2358/2359；`in` 用 **2322**（Go `checkTypeAssignableTo(..., nil)` 默认关系错误——TS-go 无 2360/2361）。全部 `✓`。新增物均为私有 fn / 私有臂（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `instanceof_expression_yields_boolean`（I1，tracer red→green） | `instanceof` 结果 `boolean` | `declare const o:object; declare function f():void; o instanceof f;` → `check_expression`=`boolean` | ✓ |
| `instanceof_primitive_left_reports_diagnostic`（I2） | 左 primitive → 2358 | `declare function f():void; declare const s:string; s instanceof f;` → 1 诊断 2358 "The left-hand side of an 'instanceof' expression must be of type 'any', an object type or a type parameter." | ✓ |
| `instanceof_non_callable_right_reports_diagnostic`（I3，合成全局 `Function`） | 右非 Function/不可调用 → 2359 | `interface Function{bind:number} interface O{x:number} declare const a:O; declare const b:O; a instanceof b;` → 1 诊断 2359 "The right-hand side of an 'instanceof' expression must be either of type 'any', a class, function, or other type assignable to the 'Function' interface type, or an object type with a 'Symbol.hasInstance' method." | ✓ |
| `instanceof_function_subtype_right_reports_no_diagnostic`（I4a 守卫） | 右为 Function 子类型 → 无 2359 | `interface Function{bind:number} declare const a:Function; declare const b:Function; a instanceof b;` → `[]` | ✓ |
| `instanceof_callable_right_reports_no_diagnostic`（I4b 守卫） | 右可调用 → 无 2359（无需全局 Function） | `interface O{x:number} declare const o:O; declare function f():void; o instanceof f;` → `[]` | ✓ |
| `in_expression_yields_boolean`（N1，tracer red→green） | `in` 结果 `boolean` | `declare const k:string; declare const o:object; k in o;` → `check_expression`=`boolean` | ✓ |
| `in_expression_non_string_number_symbol_left_reports_diagnostic`（N2） | 左非 string\|number\|symbol → 2322 | `interface O{x:number} declare const o:O; declare const r:object; o in r;` → 1 诊断 2322 "Type 'O' is not assignable to type 'string \| number \| symbol'." | ✓ |
| `in_expression_non_object_right_reports_diagnostic`（N3） | 右非 object → 2322 | `declare const k:string; declare const s:string; k in s;` → 1 诊断 2322 "Type 'string' is not assignable to type 'object'." | ✓ |
| `in_expression_valid_operands_report_no_diagnostic`（N4 守卫） | 合法 in → 无诊断 | `declare const k:string; declare const o:object; k in o;` → `[]` | ✓ |

## 4ad `T[]` ArrayType 类型节点 + for-of 数组元素类型化行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ad 落地记录」切片 1 / 2a / 2b / 2b 守卫）。tracer 均实测 red：切片1 `ArrayType` 落 `_` 臂 → `a[0]`=`error_type`（≠ number）；切片2a `for (const x of [])` 误报 `1155`；切片2b `x` 未注解=`any` → body `const y: string = x` 无诊断（期望 2322）。结果类型经 `check_expression` 直接断言（切片1），诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`（切片2a/2b）。**合成全局** `interface Array<T> { [n: number]: T; length: number }`（程序顶层声明，Go `c.globalArrayType` 的单文件 stand-in）驱动数组解析与元素类型化。expected 取自 Go `getTypeFromArrayOrTupleTypeNode`/`getArrayType`/`checkForOfStatement`/`getIteratedTypeOrElementType`/`checkGrammarVariableDeclaration`（for-in/of 门控）语义，全部 `✓`。新增物均为私有 fn / 新 match 臂 / 既有体扩展（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。tuple/`ReadonlyArray`/完整 iterator 协议/字符串迭代/生成器/异步迭代器/`getIteratedTypeOrElementType` union 处理 / for-in 变量类型化 / 解构循环变量仍 DEFER（见 impl.md「4ad 落地记录」DEFER）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `check_element_access_number_array_element_type`（切片1，tracer red→green） | `T[]` ArrayType 节点 → 全局 `Array<elem>` 引用（元素访问端到端） | 合成 `Array<T>` + `declare const a: number[]; a[0];` → `check_expression`=`number` | ✓ |
| `for_of_const_loop_variable_without_initializer_reports_no_grammar_error`（切片2a，grammar 门控 red→green） | for-of `const` 变量无初始化器不报 `1155`（Go for-in/of 门控） | `for (const x of []) {}` → 无诊断 | ✓ |
| `for_of_loop_variable_is_typed_as_array_element`（切片2b，元素类型化 red→green） | for-of 循环变量按数组元素类型化 | 合成 `Array<T>` + `declare const a: number[]; for (const x of a) { const y: string = x; }` → 1 诊断 `2322` "Type 'number' is not assignable to type 'string'." | ✓ |
| `for_of_loop_variable_element_type_assignable_to_matching_target`（切片2b 守卫） | 元素类型真为 `number`（非 blanket 报错） | 同上但 body `const y: number = x;` → `[]` | ✓ |

## 4ae 元组类型节点 `[A, B]` + `ReadonlyArray` / `readonly T[]` 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ae 落地记录」切片 1 / 1 守卫 / 2）。tracer 均实测 red：切片1 `TupleType` 落 `_` 臂 → `t[0]`=`error_type`（TypeId(3)，≠ string）；切片2 `TypeOperator` 落 `_` 臂 → `r` 声明类型=`error_type` → `r[0]`=`error_type`。结果类型经 `check_expression` 直接断言。**合成全局** `interface Array<T>{...}` / `interface ReadonlyArray<T>{ readonly [n:number]:T; readonly length:number }`（程序顶层声明，Go `c.globalArrayType`/`c.globalReadonlyArrayType` 的单文件 stand-in）驱动 readonly 解析与元素类型化。expected 取自 Go `getTypeFromArrayOrTupleTypeNode`（tuple 分支）/`getArrayOrTupleTargetType`（readonly 选 `globalReadonlyArrayType`）/`getTypeFromTypeOperatorNode`（`readonly` 透传操作数）/`getTypeFromTypeReference`（`ReadonlyArray<T>` 引用形）语义，全部 `✓`。新增物：`get_type_from_type_node` 新增 `TupleType`/`TypeOperator` 两个 match 臂、私有 fn（`get_type_from_tuple_type_node`/`get_type_from_type_operator_node`/`is_readonly_type_operator_parent`/`get_tuple_element_by_literal_index`）、新增 pub fn `Checker::create_tuple_type`（加法，含 §8.6 doctest）；既有 pub fn 签名不变（`cargo build -p tsgo_compiler` 绿）。变长/可选/具名/rest 元组、tuple→数组可赋值性、`as const`、`keyof`/`unique symbol`、非字面量 `number` 索引（元素并集）仍 DEFER（见 impl.md「4ae 落地记录」DEFER）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `check_element_access_tuple_first_element_type`（切片1，tracer red→green） | `TupleType` 节点 → 定长元组类型；按字面量索引取首元素 | `declare const t: [string, number]; t[0];` → `check_expression`=`string` | ✓ |
| `check_element_access_tuple_second_element_type`（切片1 守卫） | 元素访问按位置（非 blanket 首元素） | `declare const t: [string, number]; t[1];` → `check_expression`=`number` | ✓ |
| `check_element_access_readonly_array_element_type`（切片2，tracer red→green） | `readonly T[]`（`TypeOperator` readonly）→ 全局 `ReadonlyArray<elem>` 引用（元素访问端到端） | 合成 `ReadonlyArray<T>` + `declare const r: readonly string[]; r[0];` → `check_expression`=`string` | ✓ |
| `check_element_access_readonly_array_type_reference_element_type`（切片3，确认/既有机制） | `ReadonlyArray<T>` 引用形复用 4v `getTypeFromTypeReference` 路径（无新构造代码） | 合成 `ReadonlyArray<T>` + `declare const r: ReadonlyArray<string>; r[0];` → `check_expression`=`string` | ✓ |

## 4af 元素访问失败诊断 2538 + for-in 变量 `string` + 元组 `length`/非字面量数字索引行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4af 落地记录」切片 1 / 2 / 2 守卫 / 3a / 3b）。tracer 均实测 red：切片1 `o[k]`（`k: boolean`）静默 `error_type` → 0 诊断（期望 1× 2538）；切片2 for-in 变量未类型化=`any` → body `const n: number = k` 无诊断（期望 2322）；切片3a `t.length` 走属性访问 miss → 返回 `error_type`（`type_to_string`="error"，期望 "2"）；切片3b `t[i]`（`i: number`）→ `error_type`（TypeId(3)，期望 `string\|number` 并集）。诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`（切片1/2/2守卫），结果类型经 `check_expression` 直接断言（切片3a/3b）。expected 取自 Go `getPropertyTypeForIndexType`（尾部 2538 臂 / tuple `[number]` 索引并集）/`getTypeForVariableLikeDeclaration`（for-in → `c.stringType`）/`createTupleTargetType`（`length` 成员=定长 → 数字字面量 arity）语义，全部 `✓`。新增物均为私有 fn（`assign_for_in_variable_types`/`get_tuple_length_type`/`get_tuple_number_index_type`）/ 新 match 臂 / 既有体扩展（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。`7053`/`noImplicitAny`、symbol-keyed 索引、`noUncheckedIndexedAccess`、变长元组 `length`、`keyof T` for-in 变量类型仍 DEFER（见 impl.md「4af 落地记录」DEFER）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `check_element_access_boolean_index_reports_2538`（切片1，tracer red→green） | 非 string/number/symbol-like 索引 → 2538 | `interface O{a:number} declare const o:O; declare const k:boolean; o[k];` → 1 诊断 2538 "Type 'false \| true' cannot be used as an index type."（boolean 打印为 `false \| true`，其→`boolean` 折叠 DEFER 至 4j 节点构建器；2538 码为本片受测行为） | ✓ |
| `for_in_loop_variable_is_typed_as_string`（切片2，tracer red→green） | for-in 循环变量类型化为 `string` | `for (const k in {}) { const n: number = k; }` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `for_in_loop_variable_string_assignable_to_matching_target`（切片2 守卫） | 变量真为 `string`（非 blanket 报错） | `for (const k in {}) { const s: string = k; }` → `[]` | ✓ |
| `tuple_length_resolves_to_numeric_literal_arity`（切片3a，tracer red→green） | 定长元组 `.length` → 数字字面量 arity | `declare const t: [string, number]; t.length;` → `type_to_string(check_expression)`="2"（区别于 `number` 原始类型） | ✓ |
| `check_element_access_tuple_non_literal_number_index_yields_element_union`（切片3b，tracer red→green） | 非字面量 `number` 索引元组 → 元素并集 | `declare const t: [string, number]; declare const i: number; t[i];` → `check_expression`=`string \| number` | ✓ |

## 4ag well-known symbol late-binding（`[Symbol.iterator]` → `__@iterator`）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ag 落地记录」切片 1 / 2 / 3）。binder 修复轮把 `[Symbol.x]` 计算名匿名绑为 `__computed`（不进 `I.members`、仅挂成员节点）；本轮 checker 侧把它 late-bind 到 `__@<name>`。tracer 均实测 red：切片1 `get_property_name_for_known_symbol_name` 方法不存在（编译错）；切片2 `get_property_of_type(I, "__@iterator")`=None（成员未 late-bind）；切片3 纯语法实现下无全局 `Symbol` 仍误绑（`Some(SymbolId(1))`，期望 None）。helper 经 `Checker::get_property_name_for_known_symbol_name` 直接断言，成员可达经公开 `get_property_of_type`（成员表查 `__@iterator` 键）。**合成全局** `interface SymbolConstructor { readonly iterator: unique symbol }` + `declare var Symbol: SymbolConstructor`（程序顶层声明，well-known-symbol 来源，Go `getGlobalESSymbolConstructorSymbolOrNil` 的单文件 stand-in）驱动全局 `Symbol` 身份守卫。expected 取自 Go `getPropertyNameForKnownSymbolName`（fallback `InternalSymbolNamePrefix + "@" + name`）/`getResolvedMembersOrExportsOfSymbol`+`lateBindMember`（晚绑成员）/`isSymbolOrSymbolForCall`（全局 `Symbol` 检查）语义，全部 `✓`。reachable 子集（unique-symbol 类型构造未落地，故走 fallback 名；复用 binder 的 `__computed` 符号未新建 `CheckFlagsLate` 符号）与 DEFER（完整迭代器协议 / unique-ES-symbol 类型 / `obj[Symbol.x]` 元素访问 late-bind / 新建晚绑符号+冲突诊断）见 impl.md「4ag 落地记录」。**公开 API 仅做加法**：新增 pub fn `Checker::get_property_name_for_known_symbol_name`（含 §8.6 doctest）；`get_declared_type_of_class_or_interface`/`get_property_of_type` 仅体扩展，既有签名不变（`cargo build -p tsgo_compiler` 绿）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `flow_test.rs::property_name_for_known_symbol_name_uses_at_prefixed_internal_name`（切片1，tracer red→green） | well-known symbol 晚绑名 helper（fallback 形态） | `c.get_property_name_for_known_symbol_name("iterator")`=`"\u{FE}@iterator"`、`("asyncIterator")`=`"\u{FE}@asyncIterator"`；escape 后 == Go 字面量 `"__@iterator"` | ✓ |
| `declared_types_test.rs::late_binds_well_known_symbol_iterator_member`（切片2，tracer red→green） | `[Symbol.iterator]` late-bind 到 `__@iterator` 成员可达 | `interface SymbolConstructor{readonly iterator: unique symbol} declare var Symbol: SymbolConstructor; interface I{ [Symbol.iterator](): void }` → `get_property_of_type(I, late_name)`=Some（声明含 `MethodSignature`）；字面名 `"iterator"`=None | ✓ |
| `declared_types_test.rs::computed_symbol_member_without_global_symbol_is_not_late_bound`（切片3，tracer red→green） | 无全局 `Symbol` → 不 late-bind（全局身份守卫） | `interface I{ [Symbol.iterator](): void }`（无 `declare var Symbol`）→ `get_property_of_type(I, late_name)`=None | ✓ |

## 4ah for-of over a `[Symbol.iterator]`-bearing object（iterator-protocol 元素类型化）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ah 落地记录」切片 1 / 2 / 2.5 / 3 / 3 守卫）。续 4ag late-bound `__@iterator` 成员，把 for-of 循环变量按迭代器协议类型化。tracer 均实测 red：切片1 `{ value: string }` 类型字面量落 `_` 臂 → `o.value`=`error_type`（TypeId(3)≠string，TypeId(7)）；切片2 method 成员 `get_type_of_symbol` 落 METHOD 未处理臂 → 0 call signatures（期望 1）；切片2.5 `value: T` 经 `resolve_name("T")` miss（类型参数不在接口 `locals`）→ `error_type`（≠ 类型参数）；切片3 `for (const x of it)` 中 `x` 保持 `any` → body `const n: number = x` 0 诊断（期望 1× 2322）。结果类型/成员经 `check_expression`/`get_signatures_of_type`/`get_return_type_of_call`/`get_type_of_symbol` 直接断言（切片1/2/2.5），诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`（切片3/3守卫）。**合成全局** `interface Iterator<T> { next(): { value: T } }` + `interface It { [Symbol.iterator](): Iterator<string> }` + 4ag 的 `SymbolConstructor`/`declare var Symbol`（程序顶层声明，Go lib 迭代器协议类型 + `c.globalIteratorType` 的单文件 stand-in）驱动。expected 取自 Go `getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode`/`getTypeOfSymbol`(METHOD)+`getSignaturesOfSymbol`(`MethodSignature`)/`resolveName`(类型参数 meaning)/`getIteratedTypeOfIterable`+`getIterationTypesOfIterable`/`checkForOfStatement` 语义，全部 `✓`。**reachable 子集 / DIVERGENCE**：不做匿名对象深实例化，元素类型取（未实例化）`next()` 结果的 `value` 属性类型再经迭代器引用的类型实参 mapper 实例化（`Iterator<string>.next(): {value:T}` → `string`），元素类型与 Go 一致；`getIterationTypesOfIterable` 完整 union/异步可迭代物/`Symbol.asyncIterator`/字符串迭代/生成器/`downlevelIteration`/`2488`/`2489` 诊断仍 DEFER（见 impl.md「4ah 落地记录」DEFER）。**公开 API 仅做加法/体扩展**：未新增/未改任何 `pub fn` 签名——`get_type_from_type_node`/`get_type_of_symbol`/`get_signatures_of_type`/`get_return_type_of_call` 原样保留；新增物全为私有 fn（`get_type_from_type_literal_node`/`resolve_type_parameter_in_scope`/`type_parameter_list_of`/`get_iterated_type_of_iterable`/`first_signature_return_type`/`type_reference_mapper`）/ 新 match 臂 / 既有体扩展，外加 `check.rs` 类型解析点改穿 `program.globals()`（体扩展，触发 `It` 的 `__@iterator` late-binding）（`cargo build -p tsgo_compiler` 绿）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `check_test.rs::check_property_access_type_literal_member`（切片1，tracer red→green） | `{ value: string }` 类型字面量解析为匿名对象，成员可达 | `declare const o: { value: string }; o.value;` → `string` | ✓ |
| `declared_types_test.rs::method_member_call_signature_return_type`（切片2，tracer red→green） | method 成员类型携带其调用签名（返回类型可达） | `interface I { m(): string }` → `get_signatures_of_type(typeof m)`.len()=1，返回类型=`string` | ✓ |
| `declared_types_test.rs::bare_type_parameter_reference_resolves_to_enclosing_type_parameter`（切片2.5，tracer red→green） | 裸 `T` 引用解析到外层泛型声明的类型参数 | `interface Iterator<T> { next(): { value: T } }` → `next()` 结果 `value` 成员类型=`Iterator` 的类型参数 | ✓ |
| `check_test.rs::for_of_iterable_loop_variable_is_typed_as_iterator_value`（切片3，tracer red→green） | for-of 经 `__@iterator` 调用签名 → `next()`-value 类型化循环变量 | 合成 `Iterator<T>`/`It` + `declare const it: It; for (const x of it) { const n: number = x; }` → 1 诊断 `2322` "Type 'string' is not assignable to type 'number'." | ✓ |
| `check_test.rs::for_of_iterable_loop_variable_value_assignable_to_matching_target`（切片3 守卫） | 元素类型可赋值于匹配 target（证明 `x` 真为 `string`） | 同上但 body `const s: string = x;` → 无诊断 | ✓ |

## 4ai for-of 迭代诊断（2488/2489）+ 字符串迭代 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ai 落地记录」切片 1 / 2 / 3 / 3 守卫）。tracer 均实测 red：切片1 `{ a: number }` 缺迭代器静默返回 None → 0 诊断（期望 1× 2488）；切片2 `[Symbol.iterator](): {}`（迭代器无 next）切片1 实现下静默 None → 0 诊断（期望 1× 2489）；切片3 `string` 落 iterator 协议 → 误报 2488（期望 2322，`c` 应为 `string`）。诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。**合成全局** `SymbolConstructor`/`declare var Symbol`（4ag/4ah，驱动 `[Symbol.iterator]` late-binding）。expected 取自 Go `reportTypeNotIterableError`（2488）/`getIterationTypesOfMethod`（next → 2489）/`getIteratedTypeOrElementType`（string-input → `c.stringType`）语义，码/文本经 `diagnostics_generated` 与 baseline `for-of14`/`for-of16` 确认，全部 `✓`。**DIVERGENCE**：Go 顶层报 2488 + 2489-related；reachable 子集（无 related-info 接线）把缺-`next` 的 2489 提为顶层（见 impl.md）。新增物均为私有 fn / 既有私有 fn 签名变更 / 既有体扩展（公开 API 不变；`cargo build -p tsgo_compiler` 绿）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `for_of_non_iterable_object_reports_2488`（切片1，tracer red→green） | 缺 `[Symbol.iterator]()` 的对象 for-of → 2488 | `declare const v: { a: number }; for (const x of v) {}` → 1 诊断 2488 "Type '{ a: number; }' must have a '[Symbol.iterator]()' method that returns an iterator." | ✓ |
| `for_of_iterator_without_next_method_reports_2489`（切片2，tracer red→green） | `[Symbol.iterator]()` 在但迭代器无 `next()` → 2489 | 合成 `Symbol` + `interface Bad { [Symbol.iterator](): {}; } declare const b: Bad; for (const x of b) {}` → 1 诊断 2489 "An iterator must have a 'next()' method." | ✓（→4aj 重构为 2488-primary + 2489-related，见下） |
| `for_of_over_string_types_element_as_string`（切片3，tracer red→green） | 字符串 for-of 把循环变量类型化为 `string`（不报 2488） | `declare const s: string; for (const c of s) { const n: number = c; }` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `for_of_over_string_element_assignable_to_string_target`（切片3 守卫） | 元素真为 `string`（非 blanket 报错，无 2488） | 同上但 body `const t: string = c;` → `[]` | ✓ |

## 4aj union-of-iterables 元素分配 + 诊断 related-information 基建（修复 4ai 偏离）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4aj 落地记录」切片 1 / 1 守卫 / 2）。tracer 均实测 red：切片1 `u: string[] | number[]` 的 union for-of 落到「整体非可迭代」→ 误报 2488 "Type 'Array<string> | Array<number>' must have a '[Symbol.iterator]()' method..."（期望 2322，`x` 应为 `string | number`）；切片2 缺-`next` 迭代器顶层报 2489（4ai 偏离）→ `diags[0].code == 2489`（期望顶层 2488 + 一条 2489-related）。诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`，related-info 经新增公开字段 `Diagnostic::related_information` 读取。**合成全局** `interface Array<T> { [n: number]: T; length: number }`（4ad，驱动数组元素类型化）+ `SymbolConstructor`/`declare var Symbol`（4ag/4ah，驱动 `[Symbol.iterator]` late-binding）。expected 取自 Go `getIterationTypesOfIterableWorker`（union 臂：逐成员 `getIterationTypesOfIterableWorker` + `combineIterationTypes`→`getIterationTypeUnion`→`getUnionType`；缺-types 成员 → 整体 `reportTypeNotIterableError`）与 `Diagnostic.AddRelatedInfo`（2489 经 `diagnosticOutput` 挂为 2488 的 related）语义，全部 `✓`。**公开 API 加法式**：`Diagnostic` 仅新增 `related_information: Vec<Diagnostic>`（默认空，既有读取面不受影响）+ 新增 `pub fn Diagnostic::add_related_info` + 重导出 `tsgo_checker::Category`；既有 `code`/`message`/`category`/`start`/`length` 与所有 `pub fn` 签名原样保留（`cargo build -p tsgo_compiler` 绿）。其余新增物为私有 fn（`diagnostic_for_node`/`add_diagnostic`）/ 既有私有 fn 签名变更（`error_node: Option<NodeId>`）/ 既有体扩展。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `for_of_union_of_iterables_distributes_element_type`（切片1，tracer red→green） | union for-of 元素分配（按成员并集） | `interface Array<T>{[n:number]:T;length:number} declare const u: string[] \| number[]; for (const x of u) { const s: string = x; }` → 1 诊断 2322 "Type 'string \| number' is not assignable to type 'string'." | ✓ |
| `for_of_union_of_iterables_element_assignable_to_union_target`（切片1 守卫） | 元素真为 `string \| number`（无 2488） | 同上但 body `const v: string \| number = x;` → `[]` | ✓ |
| `for_of_iterator_without_next_method_reports_2488_with_related_2489`（切片2，red→green；4ai 偏离修复） | 缺-`next` 迭代器 → 顶层 2488 + 2489-related | 合成 `Symbol` + `interface Bad { [Symbol.iterator](): {}; } declare const b: Bad; for (const x of b) {}` → 1 顶层诊断 2488 "Type 'Bad' must have a '[Symbol.iterator]()' method that returns an iterator."，其 `related_information` 含 1 条 2489 "An iterator must have a 'next()' method." | ✓ |

## 4ak `string | string[]` 混合 union + 非数组分流（2461/2495）+ `iterableExists` 门控 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ak 落地记录」切片 1 / 2 / 3 / 3 守卫）。切片1 实测**直接绿**（4aj union 分配已覆盖混合 union 元素并集），作行为守卫；切片2/3 为真红：切片2 现实现对无全局 `Iterable` 的普通非可迭代物总报 `2488`（期望 `2495`）；切片3 临时屏蔽 2461 臂实测报 `2495`-on-whole-union（期望 `2461`-on-remainder）。诊断经纯驱动面 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。**合成全局** `interface Array<T> { [n: number]: T; length: number }`（4ad，驱动数组元素类型化）；`iterableExists` 门控由是否声明合成全局 `interface Iterable<T> {}` 决定（Go `getGlobalIterableType() != emptyGenericType` 的代理，对应 `--target >= es2015`）。expected 取自 Go `getIteratedTypeOrElementType`（string-constituent split 行 6116-6181）/`getIterationDiagnosticDetails`（`allowsStrings` → 2495 vs 2461）/`getIterationTypesOfIterableWorker`（union 臂）语义，码经 `diagnostics_generated` 确认（**2461** = `Type_0_is_not_an_array_type`；**2495** = `Type_0_is_not_an_array_type_or_a_string_type`；downlevelIteration 门控码为 **2802** 非 2569，DEFER），全部 `✓`。**公开 API 不变**（新增物全为私有 fn `global_iterable_type_exists`/`iterate_union`/`report_not_array_type` + 既有私有 fn 加 `iterable_exists` 形参 + 既有体扩展；`cargo build -p tsgo_compiler` 绿）。既有 4ai/4aj 两个失败上报测试改声明合成全局 `interface Iterable<T> {}` 迁移到门控模型（保持 2488 语义）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `for_of_string_or_string_array_union_types_element_as_string`（切片1，行为守卫；非新红） | `string \| string[]` 混合 union 元素 `string` | 合成 `Array<T>` + `declare const u: string \| string[]; for (const x of u) { const n: number = x }` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `for_of_non_iterable_object_without_global_iterable_reports_2495`（切片2，tracer red→green） | 无全局 `Iterable` 时普通非可迭代物 for-of → 2495 | `declare const v: { a: number }; for (const x of v) {}`（无 `Iterable`）→ 1 诊断 2495 "Type '{ a: number; }' is not an array type or a string type." | ✓ |
| `for_of_string_or_non_array_union_reports_2461_on_remainder`（切片3，red→green） | 无 `Iterable` 时 `string \| <非数组>` 混合 union → 在非 string 余部报 2461 | `declare const u: string \| { a: number }; for (const x of u) {}`（无 `Iterable`）→ 1 诊断 2461 "Type '{ a: number; }' is not an array type." | ✓ |
| `for_of_string_or_non_array_union_element_is_string`（切片3 守卫） | 元素类型真为 `string`（string constituent 经 split 存活） | 同上但 body `const n: number = x;` → 2 诊断 `{2322, 2461}` | ✓ |
| `for_of_non_iterable_object_reports_2488`（4ai，**4al 迁移**：`interface Iterable<T> {}` 代理 → `--target es2015` 选项） | iterator-protocol world → 2488 | `--target es2015` + `declare const v: { a: number }; for (const x of v) {}` → 1 诊断 2488 | ✓ |
| `for_of_iterator_without_next_method_reports_2488_with_related_2489`（4aj，**4al 迁移**：`--target es2015` 选项） | iterator-protocol world → 缺 `next()` → 顶层 2488 + related 2489 | `--target es2015` + `... interface Bad { [Symbol.iterator](): {}; } ...` → 顶层 2488 + related 2489 | ✓ |

## 4al `compilerOptions` threading + strict 取值族 getters + 选项门控 2802 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4al 落地记录」S1 / S2 / S3）。**S1/S2**（`mod_test.rs`）：把 `compiler_options` 经新 `BoundProgram::compiler_options()`（带默认的加法 trait 方法，单文件实现 `StubProgram`/`compiler::BoundFile` 无需改）接入 checker，新增 `Checker::compiler_options`/`get_strict_option_value`/`strict_null_checks` 取值族 getter（Go `c.compilerOptions = program.Options()` + `GetStrictOptionValue` + `c.strictNullChecks`）。S2 实测真红：初版按"默认→false"断言，跑出 Go 真语义 `GetStrictOptionValue(Unknown) = strict != TSFalse`（默认 strict 未设 → **enabled**），据此把 expected 改为 Go 字面语义（ground truth）。**S3**（`check_test.rs`）：用 `--downlevelIteration`/`--target` 真选项替换 4ak 的 `global_iterable_type_exists`（全局 `Iterable` 存在性）代理，落地真 `2802` 门控——`[Symbol.iterator]`-only 可迭代物在 `--target < es2015 && !--downlevelIteration` 下报 `2802`，开 `--downlevelIteration` 或 `--target >= es2015` 则放行（解析元素类型）。S3 tracer 实测真红（现实现静默解析 → 0 诊断，期望 1× 2802）。expected 取自 Go `getIterationDiagnosticDetails`（`yieldType != nil` 臂 → 2802）/`GetStrictOptionValue`，码经 `diagnostics_generated` 确认（**2802** = `Type_0_can_only_be_iterated_through_when_using_the_downlevelIteration_flag_or_with_a_target_of_es2015_or_higher`）。**公开 API 仅加法**：新增 `BoundProgram::compiler_options()`（带默认）+ `Checker` 三个 pub getter；`cargo build -p tsgo_compiler` 绿。4ah/4ai/4aj 的 4 个 iterator-protocol 测试从 `Iterable` 声明代理迁移到 `--target es2015` 选项（同语义，opt-in iterator-world）。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `mod_test.rs::compiler_options_reflects_program_options`（S1） | checker 经保留 program 读出 `compilerOptions`（Go `program.Options()`） | `StubProgram` opts `target: Es2015` → `c.compiler_options().target == Es2015`；无选项 program → `None` | ✓ |
| `mod_test.rs::get_strict_option_value_follows_strict_and_explicit`（S2） | `GetStrictOptionValue`：显式 per-option 胜；否则 `strict != TSFalse` | `strict:true`+Unknown→true；显式 False→false；`strict:false`+Unknown→false；显式 True→true；默认(strict 未设)+Unknown→true | ✓ |
| `mod_test.rs::strict_null_checks_reads_option`（S2） | `c.strictNullChecks = GetStrictOptionValue(StrictNullChecks)` | `strict:true,strictNullChecks:false`→false；`strict:true`→true；`strict:false`→false | ✓ |
| `check_test.rs::for_of_symbol_iterator_iterable_without_downlevel_iteration_reports_2802`（S3 tracer，red→green） | `[Symbol.iterator]`-only 物在 `--target<es2015 && !--downlevelIteration` → 2802 | `--target es5` + 合成 `Symbol`/`Iterator<T>`/`It` + `declare const it: It; for (const x of it) {}` → 1 诊断 2802 "Type 'It' can only be iterated through when using the '--downlevelIteration' flag or with a '--target' of 'es2015' or higher." | ✓ |
| `check_test.rs::for_of_symbol_iterator_iterable_with_downlevel_iteration_resolves_element`（S3 companion） | 开 `--downlevelIteration` → 无 2802，元素解析为 `string` | `--downlevelIteration` + 同上 + body `const n: number = x` → 1 诊断 2322（无 2802） | ✓ |
| `check_test.rs::for_of_symbol_iterator_iterable_with_es2015_target_resolves_element`（S3 companion） | `--target es2015` → 无 2802，元素解析为 `string` | `--target es2015` + 同上 + body `const n: number = x` → 1 诊断 2322（无 2802） | ✓ |

## 4am strictNullChecks 赋值性门控（首个可观察 strictNullChecks 消费者）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4am 落地记录」S1/S2/S3/S4）。用 4al 接入的 `strict_null_checks()` getter 把关系层 `is_simple_type_related_to`（私有）里 `undefined`/`null` 的 "非 strict 下可赋给任意（非 union/intersection）类型" 规则从保守子集精化为 Go 完整门控。**S1/S2** 经 `get_diagnostics` + `parse_and_bind_with_options` 驱动 `--strictNullChecks` 两态，断言 `2322` 诊断差异（红：现实现恒报 `2322`，与 flag 无关 → tracer red→green；S2 的 UNDEFINED 臂经临时回退实测真红）。**S3** 守卫：strict 下 `undefined`→`string | undefined` 仍经结构化 target-union 规则放行（无诊断），证门控未过度收紧。**S4**（`relations_test.rs`）直接 `is_type_assignable_to` 双态覆盖 pub fn。expected 取 Go `relater.go:isSimpleTypeRelatedTo`（`(!strict && t 非 UnionOrIntersection) || t&(Undefined|Void)` / `... || t&Null`）+ 诊断 `2322 = Type_0_is_not_assignable_to_type_1`。**公开 API 仅加法**：改动仅在私有 `is_simple_type_related_to` 内部读已存在 getter；`cargo build -p tsgo_compiler` 绿。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `check_test.rs::null_initializer_to_non_nullable_ok_when_not_strict`（S1 tracer，red→green） | 非 strict 下 `null`→非 nullable 无 2322 | `--strictNullChecks false` + `var x: string = null;` → 0 诊断 | ✓ |
| `check_test.rs::null_initializer_to_non_nullable_reports_2322_under_strict`（S1 companion） | strict 下 `null`→非 nullable 报 2322（证 flag 差异） | `--strictNullChecks true` + 同输入 → 1 诊断 2322 "Type 'null' is not assignable to type 'string'." | ✓ |
| `check_test.rs::undefined_initializer_to_non_nullable_ok_when_not_strict`（S2，red→green） | 非 strict 下 `undefined`→非 nullable 无 2322 | `--strictNullChecks false` + `declare const u: undefined;\nvar x: string = u;` → 0 诊断 | ✓ |
| `check_test.rs::undefined_initializer_to_non_nullable_reports_2322_under_strict`（S2 companion） | strict 下 `undefined`→非 nullable 报 2322 | `--strictNullChecks true` + 同输入 → 1 诊断 2322 "Type 'undefined' is not assignable to type 'string'." | ✓ |
| `check_test.rs::undefined_initializer_to_nullable_union_ok_under_strict`（S3 guard） | strict 下 `undefined`→nullable union 仍放行 | `--strictNullChecks true` + `declare const u: undefined;\nvar x: string \| undefined = u;` → 0 诊断 | ✓ |
| `relations_test.rs::assignable_null_undefined_gated_on_strict_null_checks`（S4，关系级） | pub fn `is_type_assignable_to` 门控 | strict-off program：`null`/`undefined`→`string` 真；strict-on program：→`string` 假，但 `undefined`→`void`/各自→自身 真 | ✓ |

## 4an EmitResolver 引用解析核心（scope-aware resolveName + isReferenced）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4an 落地记录」S1/S2/S2c/S3）。在 `EmitResolver` 上落地两个加法式 pub 方法：`resolve_reference`（标识符 USE → 声明符号，经 `resolve_name` 作用域链上行，meaning=`VALUE|ALIAS`，innermost 遮蔽优先）+ `is_referenced`（importElision 原语：扫全文件值位 USE，排除声明自身名节点，任一解析到该声明符号即 referenced）。**S1/S2/S2c** 经 `StubProgram`（parse+bind）驱动，桩 `todo!()` 看红 → 实现转绿。**S3** 守卫锁定 headline 性质：作用域正确（被 inner 同名绑定遮蔽的 USE 不计为对外层 import 的引用），替代 name-match 替身。expected 取 Go `checker.go:resolveName`（innermost-scope wins）/`isReferenced(7041)`。**公开 API 仅加法**：新增物全为 `EmitResolver` pub 方法（已 re-export）+ 私有 helper；`cargo build -p tsgo_compiler` 绿。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `resolve_reference_picks_innermost_shadowing_declaration`（S1 tracer，red→green） | 标识符 USE 解析作用域正确、innermost 遮蔽优先 | `var a = 1;\nfunction f() { var a = 2; a; }`：USE `a` → inner `var a` 符号（≠ 外层 `a`） | ✓ |
| `is_referenced_true_for_used_import_binding`（S2，red→green） | 被使用的 import binding referenced | `import { x } from "m";\nx;`：specifier `x` → true | ✓ |
| `is_referenced_false_for_unused_import_binding`（S2 companion，red→green） | 未使用的 import binding 非 referenced（且 specifier 自身名节点被排除，否则误判 true） | `import { y } from "m";`：specifier `y` → false | ✓ |
| `is_referenced_is_scope_correct_not_name_match`（S3 guard） | 被遮蔽的 USE 不计为对外层 import 的引用（作用域正确，非 name-match） | `import { x } from "m";\nfunction f() { var x = 1; x; }`：specifier `x` → false | ✓ |

## 4ao EmitResolver value-alias 查询（IsValueAliasDeclaration / IsReferencedAliasDeclaration 可达子集）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ao 落地记录」S1/S2/S3/S4）。在 `EmitResolver` 上落地两个加法式 pub 方法：`is_value_alias_declaration`（export/import specifier 的 (property)name 按 **VALUE** meaning 在本作用域解析成功 ⇔ value alias）+ `is_referenced_alias_declaration`（`is_alias_symbol_declaration` 守卫 + 复用 4an `is_referenced` 作为 `referenced` 作用域正确替身）。全部经 `StubProgram`（parse+bind）驱动。**S1/S3** 桩 `todo!()` 看红 → 实现转绿；**S2/S4** 先把 impl 降级（specifier 硬编 true / 去掉 alias 守卫）看断言红 → 恢复真实逻辑转绿，留下逐行为 red→green 证据。expected 取 Go `emitresolver.go:isValueAliasDeclarationWorker(718)`（alias target 是否 VALUE）/`IsReferencedAliasDeclaration(680)`（`IsAliasSymbolDeclaration` 守卫 + referenced）。**公开 API 仅加法**：新增物全为 `EmitResolver` pub 方法（已 re-export）+ 私有 helper；`cargo build -p tsgo_compiler` 绿。跨模块 target value-ness / type-only-ness / 其余 alias 形态 DEFER（见 impl.md）。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `is_value_alias_declaration_true_for_exported_value`（S1 tracer，red→green） | export specifier 别名本地 value → value alias | `function f() {}\nexport { f };`：specifier → true | ✓ |
| `is_value_alias_declaration_false_for_exported_type_only`（S2，red→green） | export specifier 别名 type-only（interface）→ 非 value alias | `interface I {}\nexport { I };`：specifier → false | ✓ |
| `is_referenced_alias_declaration_true_for_used_import`（S3 tracer，red→green） | 被使用的 import binding（alias 声明）→ referenced alias | `import { x } from "m";\nx;`：specifier `x` → true | ✓ |
| `is_referenced_alias_declaration_false_for_non_alias`（S4 guard，red→green） | 被引用的 function（非 alias 声明）→ 非 referenced alias（`IsAliasSymbolDeclaration` 守卫） | `function f() {}\nf();`：`f` 声明 → false（虽 `is_referenced`=true） | ✓ |

## 4ap EmitResolver alias completion（`import =` 名排除 + `export =` value-alias 分支）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ap 落地记录」S1/S2/S3）。补齐 6ag（P5 importElision EXPORT 侧）实测 BLOCKED 的两处 Go-faithful 扩展，均**加法式**扩既有私有 helper / 既有 pub 方法体（**无签名变更、无新增 pub 项**）：(1) `declaration_name` 加 `ImportEqualsDeclaration` 臂（排除其自身名 `x`）；(2) `is_value_alias_declaration` 的 `match` 加 `ExportAssignment` 臂。全部经 `StubProgram`（parse+bind）驱动。**S1/S2** 为 genuine RED（修复前断言失败：`import x = require("m")` 的 `x` 自解析恒 referenced=true；`ExportAssignment` 落 `_ => false`）；**S3** 先把 S2 的 `ExportAssignment` 臂硬编 `true` 看 type-only 断言红 → 恢复"identifier 按 VALUE 解析、非 ident→true"真实逻辑转绿。expected 取 Go `emitresolver.go:isValueAliasDeclarationWorker(718)` 的 `KindExportAssignment` 臂 / `getNameOfDeclaration`（import-equals 名是其 identifier）。**公开 API 仅加法**：仅扩既有私有 helper + 既有 pub 方法体；`cargo build -p tsgo_compiler` 绿。entity-name 形 `import =` / 跨模块 target value-ness DEFER（见 impl.md）。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `is_referenced_false_for_unused_import_equals`（S1 tracer，genuine red→green） | 未用 `import =` 的自身名 `x` 被 `declaration_name` 排除 → 非 referenced | `import x = require("m");`：import-equals → false | ✓ |
| `is_referenced_true_for_used_import_equals`（S1 guard，green-on-arrival） | 被使用的 `import =` referenced（不过度省略） | `import x = require("m");\nx;`：import-equals → true | ✓ |
| `is_value_alias_declaration_true_for_export_assignment_value`（S2 tracer，genuine red→green） | `export = <value ident>` → value alias | `function f() {}\nexport = f;`：export-assignment → true | ✓ |
| `is_value_alias_declaration_false_for_export_assignment_type_only`（S3，red→green） | `export = <type-only ident>` → 非 value alias（按 VALUE 解析失败） | `interface I {}\nexport = I;`：export-assignment → false | ✓ |

## 4aq 函数/箭头**表达式**体下传（return 检查覆盖到表达式位函数）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4aq 落地记录」S1–S5）。补 4r「return 深化」DEFER 里点名的**函数/箭头表达式体下传**：4r 已检查 `FunctionDeclaration`/方法体的带注解 return（2322），但表达式位 `function (): T {…}` / `(): T => {…}` 因 `check_expression` 落 `_ => error_type` 臂、体从未下传，其 `return` 永不被检查。本轮在 `check_expression` 加 `FunctionExpression`/`ArrowFunction` 臂下传**块体**，使 return 经既有 `enclosing_explicit_return_type`（父链已含两类臂）对显式注解做可赋值检查。全部经 `StubProgram`（parse+bind）驱动，诊断经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。**S1/S2** 为 genuine RED（无对应臂 → 体未下传 → 0 vs 1）；**S3/S4** 守卫可赋值不误报；**S5** 守卫体确实下传到一般表达式检查（未注解箭头体内未定义名 → 2304）。expected 取 Go `checkExpression`→`checkFunctionExpressionOrObjectLiteralMethod`/`checkArrowFunction`→`checkSourceElement(body)`→`checkReturnStatement` 语义（2322/2304）。**公开 API 仅加法**：`check_expression`(pub) 体扩两个 match 臂（签名不变），新增物全为私有方法；`cargo build -p tsgo_compiler` 绿。concise 箭头体 / 未注解返回类型推断 / 函数自身类型 / async-generator 解包 DEFER（见 impl.md）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `return_type_mismatch_in_function_expression_body_reports_diagnostic`（S1 tracer，genuine red→green） | 函数表达式块体 return 对显式注解不符 → 2322（广义化） | `const f = function (): number { return "s"; };` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `return_type_mismatch_in_arrow_function_body_reports_diagnostic`（S2 tracer，genuine red→green） | 箭头块体 return 对显式注解不符 → 2322（广义化） | `const f = (): number => { return "s"; };` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `return_type_assignable_in_function_expression_body_reports_no_diagnostic`（S3 守卫） | 函数表达式可赋值 return → 零诊断 | `const f = function (): string { return "s"; };` → `[]` | ✓ |
| `return_type_assignable_in_arrow_function_body_reports_no_diagnostic`（S4 守卫） | 箭头可赋值 return → 零诊断 | `const f = (): string => { return "s"; };` → `[]` | ✓ |
| `arrow_function_body_descends_into_nested_expression`（S5 守卫） | 箭头体下传到一般表达式检查（未注解，return-type 检查不触发） | `const f = () => { return y; };` → 1 诊断 2304 "Cannot find name 'y'." | ✓ |

## 4ar 箭头 **concise 表达式体** 返回类型检查（2322）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ar 落地记录」S1–S3）。补 4aq 明确 DEFER 的**箭头 concise（非块）表达式体**：4aq 已检查 `(): T => { return … }` 的块体 return（2322），但 concise 形态 `(): T => expr` 无 `return` 语句、4aq 的 `check_arrow_function` 仅在 `body == Block` 时下传，故体表达式从未对注解检查。本轮在 `check_arrow_function` 加 `else` 臂：非块体把体表达式**当返回值**调既有 `check_return_statement_expression(program, body, body)`，经 `enclosing_explicit_return_type`（body 父链 = 箭头 → 找到 `type_node` 注解）对显式注解做可赋值检查。全部经 `StubProgram`（parse+bind）驱动，诊断经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`。**S1** 为 genuine RED（4aq 仅下传 `Block` 体 → concise 体未检查 → 0 vs 1）；**S2/S3** 守卫可赋值/匹配注解不误报，并确认 4aq 块体不回归。expected 取 Go `checkFunctionExpressionOrObjectLiteralMethodDeferred`（非块体 `checkExpression(body)`→`checkReturnExpression(node, returnType, body, body, …)`→`checkTypeAssignableToAndOptionallyElaborate`）语义（2322）。**公开 API 仅加法**：仅私有 `check_arrow_function` 体加一 `else` 臂（签名不变），无新增公开物；`cargo build -p tsgo_compiler` 绿。未注解返回类型推断（`getReturnTypeFromBody`）/ async 体 Promise 解包 / parenthesized·object-literal concise 体 / 函数自身类型 DEFER（见 impl.md）。

| Rust 测试（`core/check_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `return_type_mismatch_in_arrow_concise_body_reports_diagnostic`（S1 tracer，genuine red→green） | 箭头 concise 体对显式注解不符 → 2322（广义化） | `const f = (): number => "s";` → 1 诊断 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `return_type_assignable_in_arrow_concise_body_reports_no_diagnostic`（S2 守卫） | 数字字面量 concise 体可赋值 `number` → 零诊断 | `const f = (): number => 1;` → `[]` | ✓ |
| `return_type_matching_string_in_arrow_concise_body_reports_no_diagnostic`（S3 守卫） | 字符串 concise 体匹配 `string` 注解 → 零诊断 | `const f = (): string => "s";` → `[]` | ✓ |

## 4as EmitResolver `get_referenced_export_container`（CJS local-export use 改写原语，可达子集）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4as 落地记录」S1–S4）。在 `EmitResolver` 上落地一个**加法式** pub 方法 `get_referenced_export_container(program, use_node, prefix_locals) -> Option<NodeId>`：值位标识符 USE 经 `resolve_name`（meaning=`EXPORT_VALUE|VALUE|ALIAS`，命中模块顶层导出的 `EXPORT_VALUE` phantom local）解析 → `EXPORT_VALUE` 则取 `export_symbol` → parent 是 `VALUE_MODULE` 且 `value_declaration` 为 `SourceFile` 时返回该 `SourceFile`（CJS 据此 qualify 为 `exports.x`）；Go 的 `!prefix_locals && ExportHasLocal && !Variable` 守卫令导出 function/class/enum/namespace USE → None（仅导出 *variable* 改写）；非导出/被遮蔽 USE → None。全部经 `StubProgram`（parse+bind）驱动。**S1**（桩 `None` vs `Some(NodeId(9))`）、**S2**（S1-impl `Some(NodeId(7))` vs `None`）为 genuine RED（实测断言失败）→ 最小实现转绿；**S3/S4** 为 S1 实现的自然结果（绿-on-arrival 的覆盖守卫，非伪造红）。expected 取 Go `referenceresolver.go:GetReferencedExportContainer`（顶层导出 → SourceFile；`ExportHasLocal && !Variable` → nil；非导出/遮蔽 → nil）。**公开 API 仅加法**：新增物只有 `EmitResolver` 一个 pub 方法（已 re-export）；`cargo build -p tsgo_compiler` 绿。namespace/enum 容器（`FindAncestor`）/ 跨模块 UMD-export / `prefix_locals=true` 覆盖 / `startInDeclarationContainer` DEFER（见 impl.md）。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `get_referenced_export_container_source_file_for_exported_value_use`（S1 tracer，genuine red→green） | 顶层导出 *variable* USE → 该模块 `SourceFile` 容器 | `export const x = 1;\nx;`：USE `x` → `Some(source file)` | ✓ |
| `get_referenced_export_container_none_for_exported_function_use`（S2，genuine red→green） | 导出 function（`ExportHasLocal && !Variable`）非前缀 USE → None | `export function f() {}\nf;`：USE `f` → `None` | ✓ |
| `get_referenced_export_container_none_for_non_exported_local`（S3 守卫） | 非导出顶层 local（脚本文件）无导出容器 | `const y = 1;\ny;`：USE `y` → `None` | ✓ |
| `get_referenced_export_container_none_for_shadowing_local`（S4 守卫） | 作用域正确：被 inner 同名绑定遮蔽的 USE 解析到非导出 inner，不返回外层导出容器 | `export const x = 1;\nfunction f() { const x = 2; x; }`：inner USE `x` → `None` | ✓ |

## 4at EmitResolver `serialize_type_node_for_metadata`（legacy-decorator `design:type` 元数据地基，keyword-type 子集）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4at 落地记录」S1–S8）。在 `EmitResolver` 上落地一个**加法式** pub 方法 `serialize_type_node_for_metadata(program, type_node) -> SerializedTypeNode` + 一个**加法式** pub 枚举 `SerializedTypeNode`（命名 Go `serializeTypeNode` 发射的运行时构造器 / `void 0`，供 P5 装饰器变换据此构造 `__metadata("design:type", <Ctor>)`）。可达 keyword-type 子集，逐 Go switch 臂：`number`→`Number`、`string`→`String`、`boolean`→`Boolean`、`bigint`→`BigInt`、`symbol`→`Symbol`、`void`/`undefined`/`never` 及 `null` literal-type→`VoidZero`、`any`/`unknown`/`object`（及 catch-all）→`Object`。全部经 `StubProgram`（parse+bind）驱动，导航 `declare const x: T;` 的 `VariableDeclaration.type_node`。**S1–S7** 为 genuine RED（方法桩/前序臂返回默认 `Object`，实测断言 `Object != <期望>` 失败）→ 最小臂转绿；**S8** 为 `Object` 默认的自然结果（绿-on-arrival 覆盖守卫，非伪造红）。expected 取 Go `tstransforms/typeserializer.go:serializeTypeNode`（keyword 臂 `NewIdentifier("<Ctor>")` / `NewVoidZeroExpression()`）+ `serializeLiteralOfLiteralTypeNode`（`null`→`void 0`）字面量。**公开 API 仅加法**：新增物只有 `EmitResolver` 一个 pub 方法 + 一个 pub 枚举（均已 re-export）；`cargo build -p tsgo_compiler` 绿。`TypeReference`→entity ctor（`get_type_reference_serialization_kind`）/ union/array/function 递归 / 非-`null` literal 臂 / `SkipTypeParentheses` DEFER（见 impl.md）。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `serialize_type_node_number_keyword_is_number`（S1 tracer，genuine red→green） | `number` keyword 类型 → 全局 `Number` 构造器 | `declare const x: number;` → `SerializedTypeNode::Number` | ✓ |
| `serialize_type_node_string_keyword_is_string`（S2，genuine red→green） | `string` keyword 类型 → 全局 `String` 构造器 | `declare const x: string;` → `String` | ✓ |
| `serialize_type_node_boolean_keyword_is_boolean`（S3，genuine red→green） | `boolean` keyword 类型 → 全局 `Boolean` 构造器 | `declare const x: boolean;` → `Boolean` | ✓ |
| `serialize_type_node_void_undefined_never_are_void_zero`（S4，genuine red→green） | `void`/`undefined`/`never` → `void 0`（"undefined" 序列化） | `void`/`undefined`/`never` 三声明 → 各 `VoidZero` | ✓ |
| `serialize_type_node_null_literal_is_void_zero`（S5，genuine red→green） | `null` literal-type → `void 0`（`serializeLiteralOfLiteralTypeNode` 的 `KindNullKeyword` 臂） | `declare const x: null;`（`LiteralType`/literal=`NullKeyword`）→ `VoidZero` | ✓ |
| `serialize_type_node_bigint_keyword_is_bigint`（S6，genuine red→green） | `bigint` keyword 类型 → 全局 `BigInt` 构造器 | `declare const x: bigint;` → `BigInt` | ✓ |
| `serialize_type_node_symbol_keyword_is_symbol`（S7，genuine red→green） | `symbol` keyword 类型 → 全局 `Symbol` 构造器 | `declare const x: symbol;` → `Symbol` | ✓ |
| `serialize_type_node_any_unknown_object_are_object`（S8 守卫，green-on-arrival） | `any`/`unknown`/`object` → 全局 `Object`（Go `object` 显式臂 + `any`/`unknown` break 组皆汇于 `Object` switch tail） | `any`/`unknown`/`object` 三声明 → 各 `Object` | ✓ |

## 4au EmitResolver `serialize_type_node_for_metadata` 扩展（结构臂：`SkipTypeParentheses` + `TemplateLiteralType` + 非-`null` literal-type 臂）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4au 落地记录」A–F2）。**仅扩展既有 match**，复用 4at 的既有 `SerializedTypeNode` 变体——**未新增枚举变体**（`Function`/`Array` 变体经实测会破坏下游 `tsgo_transformers` 的**无 wildcard 穷尽 match** `serialized_type_to_expression`→ `cargo build -p tsgo_compiler` E0004，按边界 STOP+DEFER，不改 transformers）。全部经 `StubProgram`（parse+bind）驱动 `declare const x: T;` 的 `VariableDeclaration.type_node`。A–F2 均为 genuine RED（前序仅 `_ => Object` tail / 仅 `null` literal 臂，实测断言 `Object != <期望>` 失败）→ 最小臂转绿。expected 取 Go `tstransforms/typeserializer.go:serializeTypeNode`（`SkipTypeParentheses` 顶层 unwrap、`case KindTemplateLiteralType,KindStringKeyword->String`）+ `serializeLiteralOfLiteralTypeNode`（`KindStringLiteral->String`、`KindNumericLiteral->Number`、`KindBigIntLiteral->BigInt`、`KindTrueKeyword,KindFalseKeyword->Boolean`、`KindPrefixUnaryExpression`→递归 operand）字面量。**公开 API 仅加法**：未改任何既有 pub 签名、未加枚举变体；新增仅一个**私有** helper `serialize_literal_of_literal_type_node`；`cargo build -p tsgo_compiler` 绿。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `serialize_type_node_parenthesized_unwraps_to_inner`（A，genuine red→green） | Go `SkipTypeParentheses` 顶层 unwrap：`(T)` 派发到内层类型 | `declare const x: (number);`（`ParenthesizedType`）→ `Number` | ✓ |
| `serialize_type_node_template_literal_type_is_string`（B，genuine red→green） | `KindTemplateLiteralType` 臂（与 `string` 同组）→ 全局 `String` | `` declare const x: `a${string}b`; ``（`TemplateLiteralType`）→ `String` | ✓ |
| `serialize_type_node_string_literal_type_is_string`（C，genuine red→green） | `serializeLiteralOfLiteralTypeNode` `KindStringLiteral` 臂 → `String` | `declare const x: "a";`（`LiteralType`/literal=`StringLiteral`）→ `String` | ✓ |
| `serialize_type_node_numeric_literal_type_is_number`（D，genuine red→green） | `serializeLiteralOfLiteralTypeNode` `KindNumericLiteral` 臂 → `Number` | `declare const x: 1;`（`LiteralType`/literal=`NumericLiteral`）→ `Number` | ✓ |
| `serialize_type_node_boolean_literal_types_are_boolean`（E，genuine red→green） | `serializeLiteralOfLiteralTypeNode` `KindTrueKeyword,KindFalseKeyword` 臂 → `Boolean` | `declare const a: true;` / `declare const b: false;` → 各 `Boolean` | ✓ |
| `serialize_type_node_bigint_literal_type_is_bigint`（F1，genuine red→green） | `serializeLiteralOfLiteralTypeNode` `KindBigIntLiteral` 臂 → `BigInt` | `declare const x: 1n;`（`LiteralType`/literal=`BigIntLiteral`）→ `BigInt` | ✓ |
| `serialize_type_node_negative_numeric_literal_type_is_number`（F2，genuine red→green） | `serializeLiteralOfLiteralTypeNode` `KindPrefixUnaryExpression`→递归 operand | `declare const x: -1;`（`LiteralType`/literal=`PrefixUnaryExpression`(NumericLiteral)）→ `Number` | ✓ |

> green-on-arrival / 不可达说明：**无伪造红**（A–F2 全部实测看到 `Object != <期望>` 失败再转绿）。`serializeLiteralOfLiteralTypeNode` 的 `KindNoSubstitutionTemplateLiteral->String` 臂**当前不可达**——Rust parser `parseNonArrayType` 尚未把无替换模板（`` `abc` ``）类型路由到 `parseLiteralTypeNode`（落到 type-reference），故本港省略该臂并标 DEFER（blocked-by: parser `parseNonArrayType` 的 `NoSubstitutionTemplateLiteral` 臂）。

## 4av EmitResolver `serialize_type_node_for_metadata` 扩展（结构臂 `Array`/`Function` 变体：`ArrayType`/`TupleType`→`Array`、`FunctionType`/`ConstructorType`→`Function`）行为单测（§8.6）

> **协调跨-crate lane（checker 4av + transformers 6am 同 lane 落地）**，解锁 4au 的头号 DEFER。本轮**新增两个 `SerializedTypeNode` 加法式变体**（`Array`/`Function`）并扩 `serialize_type_node_for_metadata` 的 match——这是 4au 实测会破坏 `tsgo_transformers` 无-wildcard 穷尽 match 的那两组臂，故必须与 transformers 的 `serialized_type_to_expression` 对应臂在**同一 lane** 落地（先加 checker 变体→立即加 transformer 臂保持 workspace 可构建，再观察行为红→绿）。全部经 `StubProgram`（parse+bind）驱动 `declare const x: T;` 的 `VariableDeclaration.type_node`。S1/S3 为 genuine RED（前序 `ArrayType`/`FunctionType` 落 `_ => Object` tail，实测断言 `Object != <期望>` 失败）→ 最小臂转绿；S2/S4 在前一臂落地后扩既有臂的 group（`| Kind::TupleType` / `| Kind::ConstructorType`），各自亦 genuine RED（`Object != Array`/`Object != Function`）→ 转绿。expected 取 Go `tstransforms/typeserializer.go:serializeTypeNode`（`case KindArrayType, KindTupleType -> NewIdentifier("Array")`、`case KindFunctionType, KindConstructorType -> NewIdentifier("Function")`）字面量。**公开 API 加法**：新增两个 `SerializedTypeNode` 枚举变体（additive，但破坏穷尽 match——故本 lane 同时拥有 checker+transformers）；未改任何既有 pub 签名。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `serialize_type_node_array_type_is_array`（S1，genuine red→green） | Go `case KindArrayType` → 全局 `Array` 构造器 | `declare const x: number[];`（`ArrayType`）→ `Array` | ✓ |
| `serialize_type_node_tuple_type_is_array`（S2，genuine red→green） | Go `case KindTupleType`（与 `ArrayType` 同组）→ 全局 `Array` | `declare const x: [number, string];`（`TupleType`）→ `Array` | ✓ |
| `serialize_type_node_function_type_is_function`（S3，genuine red→green） | Go `case KindFunctionType` → 全局 `Function` 构造器 | `declare const x: () => void;`（`FunctionType`）→ `Function` | ✓ |
| `serialize_type_node_constructor_type_is_function`（S4，genuine red→green） | Go `case KindConstructorType`（与 `FunctionType` 同组）→ 全局 `Function` | `declare const x: new () => C;`（`ConstructorType`）→ `Function` | ✓ |

> 红→绿证据：S1–S4 **全部 genuine RED**（前序臂返回 `Object`，实测 `Object != Array`/`Object != Function`）→ 最小臂转绿。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿；transformer 侧的端到端红→绿见 transformers `tests.md` 6am。**测试增量**：343 单测（+4：S1–S4，相对 4au 基线 339）+ 132 doctest（**±0**：枚举变体非 pub fn，无需 doctest）。

## 4aw EmitResolver `get_type_reference_serialization_kind`（`TypeReference` 实体 value-ness 分类，可达单文件子集）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4aw 落地记录」S1–S4）。在 `EmitResolver` 上落地一个**加法式** pub 方法 `get_type_reference_serialization_kind(checker, program, type_node) -> TypeReferenceSerializationKind` + 一个**加法式** pub 枚举 `TypeReferenceSerializationKind`（12 变体 1:1 镜像 Go `printer.TypeReferenceSerializationKind` 的 iota）。这是 4at/4au/4av `serialize_type_node_for_metadata` 的 `TypeReference` 臂消费的分类原语（供 P5 装饰器变换为 `: SomeClass` 发射类标识符）。忠实端口 Go `emitresolver.go:GetTypeReferenceSerializationKind` 的结构：从 `TypeReference` 取 `type_name`（实体名）→ 分别以 **Value** 与 **Type** meaning `resolve_name`（Go 两次 `resolveEntityName`）→ 二者解析到同一 `class` 符号（可达单文件中唯一的运行时构造器）则 `TypeWithConstructSignatureAndValue`；否则 type 符号解析到非-error 声明类型（interface/type-alias）则 `ObjectType`；未解析（无 value 也无 type 符号）则 `Unknown`。全部经 `StubProgram`（parse+bind）驱动 `declare const x: T;` 的 `VariableDeclaration.type_node`（指向 `TypeReference`）。expected 取 Go `GetTypeReferenceSerializationKind` 的对应 `printer.TypeReferenceSerializationKind*` 值。**S1/S2** 为 genuine RED（S1 方法桩返回 `Unknown`，`Unknown != TypeWithConstructSignatureAndValue`；S2 前序仅 class 臂、interface 落 `Unknown` tail，`Unknown != ObjectType`）→ 最小臂转绿；**S3/S4** 为 S2 落地后的自然结果（green-on-arrival 覆盖守卫，**非伪造红**：Go 对 interface 与 type-alias 经 `getDeclaredTypeOfSymbol`→`else` tail 同样分类；未解析名经 `resolvedTypeSymbol == nil` tail 同样 `Unknown`）。**公开 API 仅加法**：新增物只有 `EmitResolver` 一个 pub 方法 + 一个 pub 枚举（均已 re-export）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿（新枚举无下游穷尽 match）。

| Rust 测试（`core/emit_resolver_test.rs`） | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `type_reference_to_local_class_is_construct_signature_and_value`（S1 tracer，genuine red→green） | 本地 class 引用 value+type 同解析到 class 符号（运行时构造器）→ `TypeWithConstructSignatureAndValue` | `class C {}` + `declare const x: C;` → `TypeReferenceSerializationKind::TypeWithConstructSignatureAndValue` | ✓ |
| `type_reference_to_interface_is_object_type`（S2，genuine red→green） | 仅-type 的 interface 引用（无 value 解析）→ `ObjectType` | `interface I {}` + `declare const x: I;` → `ObjectType` | ✓ |
| `type_reference_to_type_alias_is_object_type`（S3，green-on-arrival 守卫） | object-literal type-alias 引用：声明类型为匿名对象 `{}`（与 interface 同走 `else` tail）→ `ObjectType` | `type T = {};` + `declare const x: T;` → `ObjectType` | ✓ |
| `type_reference_to_unresolved_name_is_unknown`（S4，green-on-arrival 守卫） | 无声明的名（无 lib globals）value+type 均解析失败 → `Unknown` | `declare const x: Missing;` → `Unknown` | ✓ |

> 红→绿证据：S1/S2 genuine RED（实测 `Unknown != TypeWithConstructSignatureAndValue` / `Unknown != ObjectType`）→ 最小臂转绿；S3/S4 green-on-arrival（S2 实现的"type 解析 → ObjectType / 否则 Unknown"结构已覆盖，**如实记录非伪造红**，同 4at S8 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：347 单测（+4：S1–S4，相对 4av 基线 343）+ 134 doctest（+2：新 pub 枚举 + 新 pub 方法各一个 `# Examples`）。

## 4ay `getNonNullableType` union 抽 nullable + 非空断言 `x!` + 真值 narrowing 抽 nullable 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ay 落地记录」1a–3）。消费 4al/4am 的 `c.strict_null_checks()`，落地 `get_non_null_type`（`pub(crate)` 内部）+ `check_expression` 的 `NonNullExpression` 臂 + 确认真值 narrowing 抽 nullable 可观察。`get_non_null_type` 是 Go `GetNonNullableType`→`getAdjustedTypeWithFacts(t, NEUndefinedOrNull)` 的可达核心（按 `TypeFlags::NULLABLE|VOID` 过滤 union 成员，strict-gated）。`x!` 臂 = Go `checkNonNullAssertion` 非-chain 路径 `GetNonNullableType(checkExpression(operand))`。行为 1 经 `Checker::new()`（默认-strict 内在 checker）+ 手建 union 驱动，`type_to_string` 断言；行为 2/3 经 `StubProgram`（parse+bind）+ `get_diagnostics` 端到端断言诊断码/消息。expected 取 Go 语义（reduce 后 `string`；2322 消息）。**公开 API 仅加法**：无新 pub 项（`get_non_null_type` 内部、`x!` 走既有 `check_expression`）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `get_non_null_type_strict_removes_undefined`（`type_facts_test.rs`，1a tracer，genuine red→green） | strict 下 `getNonNullableType` 抽 `undefined` | `string \| undefined` → `type_to_string=="string"` | ✓ |
| `get_non_null_type_strict_removes_null`（`type_facts_test.rs`，1b，green-on-arrival 守卫） | 同一过滤抽 `null` | `string \| null` → `"string"` | ✓ |
| `get_non_null_type_strict_removes_null_and_undefined`（`type_facts_test.rs`，1c，green-on-arrival 守卫） | 同时抽 `null`+`undefined` | `string \| null \| undefined` → `"string"` | ✓ |
| `get_non_null_type_non_strict_is_identity`（`type_facts_test.rs`，1d，genuine red→green） | 非 strict 下恒等（Go gate 整个 reduction 于 `c.strictNullChecks`） | `--strictNullChecks false` + `string \| undefined` → 同一 TypeId（恒等） | ✓ |
| `non_null_assertion_strips_undefined_then_reports_2322_against_number`（`check_test.rs`，2，genuine red→green） | `x!` 类型 = `getNonNullableType(x)`=`string`，赋给 `number` 报 2322（源为 reduced `string`） | `declare const x: string \| undefined;\nvar n: number = x!;` → 1×2322「Type 'string' is not assignable to type 'number'.」 | ✓ |
| `plain_nullable_reference_reports_2322_with_union_source`（`check_test.rs`，2-contrast，baseline） | 去 `!` 时 `x` 保持整 union（与 2 对照出 `!` 效果；port 按 TypeId 排序故 `undefined` 先打印——显示偏离非语义偏离） | `var n: number = x;` → 1×2322「Type 'undefined \| string' is not assignable to type 'number'.」 | ✓ |
| `non_null_assertion_assignable_to_string_target`（`check_test.rs`，2-guard） | `x!`（reduced `string`）可赋给 `string` 目标 | `var s: string = x!;` → 0 诊断 | ✓ |
| `truthy_branch_narrows_out_nullable`（`check_test.rs`，3，green-on-arrival 守卫） | `if (x)` truthy 分支把 `x: string\|undefined` narrow 成 `string`（既有 4t flow + 4g `narrow_type_by_truthiness`） | `if (x) {\n  var n: number = x;\n}` → 1×2322「Type 'string' is not assignable to type 'number'.」 | ✓ |

> 红→绿证据：1a/1d/2 **genuine RED**（1a identity 桩 `"undefined \| string" != "string"`；1d 无-gate 过滤把非 strict 也 reduce → TypeId 不等；2 无 `NonNullExpression` 臂 → 0≠1 诊断）→ 最小触点转绿。1b/1c/3 **green-on-arrival 守卫**（同一过滤已抽 null；真值 narrowing 早在 4t/4g 落地，本轮仅确认 nullable 形态可观察，**如实记录非伪造红**，同 4aw S3/S4 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：355 单测（+8，相对 4aw 基线 347）+ 134 doctest（**±0**：`get_non_null_type` 为 `pub(crate)`、无 `# Examples`，不计 doctest）。

## 4az EQ/NE-nullable `TypeFacts` + 相等 flow narrowing + 属性/元素访问 possibly-null/undefined 诊断 + `undefined` 值 + 类型位 `null` 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4az 落地记录」A–C）。三件串联的 strictNullChecks 语义：扩 `get_type_facts`（EQ/NE/Is undefined/null fact 位，Go 位号，同步既有期望）+ 相等 flow narrowing 的 nullable 分支（`narrow_type_by_equality_to_value`）+ 属性/元素访问的 `checkNonNullType`/`reportObjectPossiblyNullOrUndefinedError`。**确认 Go 实达码**：对象为 entity-name 标识符 `x` → 发 **18047/18048/18049**（`_0_is_possibly_*`），任务文案的 **2531/2532/2533**（`Object_is_possibly_*`）是同函数的非-entity-name `else` 臂（本港忠实端口两路，2531/2532/2533 臂本轮不可达 → DEFER）。诊断测试经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`；narrowing 测试经 `get_flow_type_of_reference(declared)` 断言收窄 TypeId（与手建 `get_union_type` interned id 比较）。expected 取 Go 语义（fact 位 / narrowed 类型 / 诊断码+消息）。**公开 API 仅加法**：无新 pub 项（新方法/函数均 `pub(crate)`/私有，`TypeFacts` 仅加常量）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `property_access_on_possibly_undefined_reports_18048`（`check_test.rs`，A tracer，genuine red→green） | possibly-undefined 对象属性访问报 `18048` 后在非空类型上续查 | `declare const x: { a: number } \| undefined;\nx.a;` → 1×18048「'x' is possibly 'undefined'.」 | ✓ |
| `type_facts_of_primitives_and_literals`（`type_facts_test.rs`，同步既有期望） | `string`/`undefined`/`null` 的 facts 扩到全 EQ/NE/Is 子集（保持绿）| `string`→`NE*\|Truthy\|Falsy`、`undefined`→`EQUndefined\|EQUndefinedOrNull\|NENull\|Falsy\|IsUndefined`、`null`→`EQNull\|EQUndefinedOrNull\|NEUndefined\|Falsy\|IsNull` | ✓ |
| `property_access_on_possibly_null_reports_18047`（`check_test.rs`，A-guard，genuine red→green：类型位 null）| possibly-null 对象报 `18047`（红：类型位 `null`→`error`，落 2339）| `{ a: number } \| null` → 1×18047「'x' is possibly 'null'.」 | ✓ |
| `property_access_on_possibly_null_or_undefined_reports_18049`（`check_test.rs`，A-guard，genuine red→green）| 两 fact 齐备报 `18049` | `{ a: number } \| null \| undefined` → 1×18049「'x' is possibly 'null' or 'undefined'.」 | ✓ |
| `element_access_on_possibly_undefined_reports_18048`（`check_test.rs`，A-guard，green-on-arrival）| 元素访问同走 non-null 检查 | `x["a"]`（`{a}\|undefined`）→ 1×18048 | ✓ |
| `property_access_on_non_nullable_object_reports_nothing`（`check_test.rs`，A-guard）| 非空对象访问 0 诊断（`check_non_null_type` 恒等）| `{ a: number }` → 0 诊断 | ✓ |
| `undefined_value_resolves_without_cannot_find_name`（`check_test.rs`，B，genuine red→green）| `undefined` 值解析无 2304（红：`Cannot find name 'undefined'`）| `undefined;` → 0 诊断 | ✓ |
| `undefined_value_checks_as_undefined_type`（`check_test.rs`，B，类型见证）| `undefined` 值类型化为 `undefined`（红：`error_type`）| `check_expression(undefined)` → `undefined` 类型 | ✓ |
| `flow_equality_loose_null_keeps_both_nullables`（`flow_test.rs`，C tracer，genuine red→green）| loose `== null` 留 `null \| undefined`（`EQUndefinedOrNull`；红：旧 overlap 只留 `null`）| `string \| null \| undefined` + `if (x == null)` 真分支 → `null \| undefined` | ✓ |
| `flow_equality_ne_undefined_narrows_to_string`（`flow_test.rs`，C-guard，green-on-arrival）| 任务主例 `x !== undefined`→`string`（`NEUndefined`）| `string \| undefined` 真分支 → `string` | ✓ |
| `flow_equality_eq_undefined_narrows_to_undefined`（`flow_test.rs`，C-guard，green-on-arrival）| `x === undefined`→`undefined`（`EQUndefined`）| `string \| undefined` 真分支 → `undefined` | ✓ |
| `flow_equality_ne_null_narrows_to_string`（`flow_test.rs`，C-guard，green-on-arrival）| `null` 镜像 `x !== null`→`string`（`NENull`）| `string \| null` 真分支 → `string` | ✓ |
| `flow_equality_eq_null_narrows_to_null`（`flow_test.rs`，C-guard，green-on-arrival）| `x === null`→`null`（`EQNull`）| `string \| null` 真分支 → `null` | ✓ |
| `ne_undefined_branch_narrows_to_string_no_diagnostics`（`check_test.rs`，C 端到端，genuine red→green 经 B+C）| 任务 slice-2 例：narrow 成 `string` 可赋 `string`（红：B 前两次 2304）| `if (x !== undefined) {\n  var s: string = x;\n}` → 0 诊断 | ✓ |
| `plain_nullable_assigned_to_string_reports_2322`（`check_test.rs`，C 端到端对照）| 去 guard 时 union 不可赋 `string`（与上 0/1 对照）| `var s: string = x;` → 1×2322「Type 'undefined \| string' is not assignable to type 'string'.」 | ✓ |

> 红→绿证据：A / 18047 / B / C tracer **genuine RED**（`2339`≠`18048`；类型位 null→`error` 落 `2339`；`2304` / `error_type`≠`undefined`；`TypeId(6)`≠`TypeId(23)`）→ 最小触点转绿；18049 亦 genuine RED（依赖类型位 null）。element-access / non-nullable / strict 镜像 / 端到端-0诊断 为 green-on-arrival 守卫（**如实记录非伪造红**，同 4ay 口径）。**踩坑**：`narrow_type_by_equality_to_value` 用 `contains(NULLABLE)`（要求全位）误判单一位 `null`/`undefined` 不 nullable，改 `intersects` 转绿。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**369 单测**（+14，相对 4ay 基线 355）+ **134 doctest**（**±0**：新方法/函数均非 pub fn）。

## 4ba `??`/`??=` nullish 结果精化 + 调用接收者 possibly-null/undefined（`2721`/`2722`/`2723`）+ typeof narrowing 端到端 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4ba 落地记录」1–4）。三块：(1) `??`/`??=` 结果 = `getNonNullableType(left) | right`（当 `hasTypeFacts(left, EQUndefinedOrNull)`，消费 4az fact 位 + 4ay `get_non_null_type`）；(2) 调用接收者 non-null 检查走 `reportCannotInvokePossiblyNullOrUndefinedError`（**2721/2722/2723**——与属性访问的 18047/18048/18049 不同族，且无 entity-name vs Object 分支，消息恒定）；(3) typeof narrowing 端到端见证（flow 层早在 4f/4az 落地，`flow_typeof_narrows_in_then_branch` 已覆盖；本轮加诊断层见证）。诊断测试经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`；类型见证经 `check_expression`+`has_type_facts`。expected 取 Go 语义（结果可赋性 / 诊断码+消息）。**公开 API 仅加法**：无新 pub 项（`NonNullReporter` 私有 enum、`check_non_null_type_with_reporter`/`report_cannot_invoke_possibly_null_or_undefined_error` 私有方法、`??` 臂内部）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust 测试 | 验证内容 | input → expected | 完成 |
|---|---|---|---|
| `nullish_coalesce_removes_undefined_assignable_to_string`（`check_test.rs`，1 tracer，genuine red→green）| `??` 结果抽掉 nullable 左部，可赋给 `string`（红：旧臂返回 raw `string \| undefined` → 1×2322）| `declare const x: string \| undefined;\nvar s: string = x ?? "d";` → 0 诊断 | ✓ |
| `nullish_coalesce_result_drops_nullable_facts`（`check_test.rs`，1 类型见证）| `x ?? "d"` 结果不带 `IsUndefined`/`IsNull` fact（`GetNonNullableType(left)` 抽掉 `undefined`）| `check_expression(x ?? "d")` → `!has_type_facts(_, IS_UNDEFINED_OR_NULL)` | ✓ |
| `nullish_coalesce_assign_removes_undefined_assignable_to_string`（`check_test.rs`，2 `??=` 共享精化）| `??=` 与 `??` 共享结果精化臂；`(x ??= "d")` 值可赋 `string`（兼跑 `checkAssignmentOperator`：`"d"` 可赋 `string\|undefined` 引用）| `declare let x: string \| undefined;\nvar s: string = x ??= "d";` → 0 诊断 | ✓ |
| `call_on_possibly_undefined_callee_reports_2722`（`check_test.rs`，3 tracer，genuine red→green）| 调用 possibly-undefined 值报 **2722**（红：union callee 无调用签名 → 静默 `error`，0 诊断）| `declare const f: (() => void) \| undefined;\nf();` → 1×2722「Cannot invoke an object which is possibly 'undefined'.」 | ✓ |
| `call_on_possibly_null_callee_reports_2721`（`check_test.rs`，3-guard，green-on-arrival）| possibly-null callee 报 **2721**（`IsNull`-only）| `(() => void) \| null` → 1×2721「Cannot invoke an object which is possibly 'null'.」 | ✓ |
| `call_on_possibly_null_or_undefined_callee_reports_2723`（`check_test.rs`，3-guard，green-on-arrival）| 两 fact 齐备报 **2723** | `(() => void) \| null \| undefined` → 1×2723「Cannot invoke an object which is possibly 'null' or 'undefined'.」 | ✓ |
| `call_on_property_access_possibly_undefined_reports_18048`（`check_test.rs`，3-guard，属性访问族对照）| `o.m()` 走属性访问 non-null 检查报 **18048**（NOT 2722；接收者 `o` 在 callee non-null 检查前已被类型化），确认 4az 路径已覆盖 `o.m` | `declare const o: { m(): void } \| undefined;\no.m();` → 1×18048「'o' is possibly 'undefined'.」 | ✓ |
| `call_on_non_nullable_callee_reports_nothing`（`check_test.rs`，3-guard）| 非空 callee 调用 0 诊断（无 Is fact，恒等）| `declare const f: () => void;\nf();` → 0 诊断 | ✓ |
| `typeof_string_guard_narrows_var_assignment_no_diagnostics`（`check_test.rs`，4 端到端见证，green-on-arrival）| `if (typeof x === "string")` 内 `x: string\|number` narrow 成 `string`（flow 层 `flow_typeof_narrows_in_then_branch` 早已覆盖；本轮加诊断层）| `declare const x: string \| number;\nif (typeof x === "string") {\n  var s: string = x;\n}` → 0 诊断 | ✓ |
| `plain_string_or_number_assigned_to_string_reports_2322`（`check_test.rs`，4 对照 baseline）| 去 guard 时 union 不可赋 `string`（与上 0/1 对照出 typeof narrowing 效果）| `declare const x: string \| number;\nvar s: string = x;` → 1×2322「Type 'string \| number' is not assignable to type 'string'.」 | ✓ |

> 红→绿证据：slice 1（`nullish_coalesce_removes_undefined_assignable_to_string`）+ slice 3（`call_on_possibly_undefined_callee_reports_2722`）**genuine RED**（1×2322「undefined \| string」≠0；callee 无签名 0≠1）→ 最小触点转绿；`??=`/`null`/`null\|undefined`/`o.m()`/非空 callee/typeof 端到端 为 **green-on-arrival 守卫**（**如实记录非伪造红**，同 4ay/4az 口径——`??=` 与 `??` 共享臂故 slice-1 impl 落地后即绿；typeof 端到端 riding 既有 4f/4az flow 机器）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**379 单测**（+10，相对 4az 基线 369）+ **134 doctest**（**±0**：新方法/enum 均非 pub fn）。

## 4bb 判别联合 narrowing + `??` 结果 `UnionReductionSubtype` 化简 + 混用运算符 grammar `5076`（外加前置：字面量类型节点 + 字面量关系按值相等）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bb 落地记录」1a–3）。三件可达语义 + 两个前置 blocker：(1a) 字面量类型节点 `"a"`/`1`/`true` 解析为字面量类型（旧 4az DEFER 返回 `error_type`）；(1b) 关系层等值字面量关联（本港不 intern 字面量，等值是不同 id，需按 `(LITERAL flags, value)` 关联，复刻 Go interning）；(1c) 判别联合 narrowing（`obj.kind === "a"` → `getDiscriminantPropertyAccess` + `narrowTypeByDiscriminantProperty` → `narrowTypeByDiscriminant`）；(2) `??` 结果 `UnionReductionSubtype` 化简（`subtype_reduce`）；(3) 混用 `??` 与 `\|\|`/`&&` 无括号 → `5076`（`checkNullishCoalesceOperands`，3 分支 if/else-if）。诊断测试经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`；expected 取 Go 语义（诊断码+消息 / 收窄后属性可见性）。**确认 Go 码**：`5076`（混用）、`2367`（无重叠）、`2339`（属性不存在）、`2322`（不可赋）均逐一核对。**公开 API 仅加法**：无新 pub 项（新方法私有/`pub(crate)`；既有私有方法 `regular_type_of_literal_type`/`are_types_comparable`/`is_literal_type` 提 `pub(crate)`——additive 可见性，不入公开 API）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（位置 / 角色） | 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `string_literal_type_node_not_assignable_to_other_literal`（`check_test.rs`，1a tracer，genuine red→green）| 字面量类型节点 `"a"` 解析为字面量类型（非 `error`）；不可赋另一字面量（目标 `"b"` 是 unit，源不广义化）| `declare const x: "a";\nconst n: "b" = x;` → 1×2322「Type '"a"' is not assignable to type '"b"'.」 | ✓ |
| `string_literal_type_node_assignable_to_string`（`check_test.rs`，1a 对照）| `"a"` 是 `string` 子类型 → 可赋 | `declare const x: "a";\nconst s: string = x;` → 0 诊断 | ✓ |
| `equality_literal_in_its_union_reports_no_overlap_diagnostic`（`check_test.rs`，1b tracer，genuine red→green）| 字面量 vs 含它的字面量 union 是合法比较（红：两个 `"a"` 不同 id → 误报 2367）| `declare const k: "a" \| "b";\nk === "a";` → 0 诊断 | ✓ |
| `equality_literal_outside_union_reports_no_overlap`（`check_test.rs`，1b 对照）| 真 no-overlap 不被抑制 | `k: "a"\|"b"` vs `"c"` → 1×2367「…'"a" \| "b"' and '"c"' have no overlap.」 | ✓ |
| `discriminant_property_eq_narrows_union_in_then_branch`（`check_test.rs`，1c tracer，genuine red→green）| `if (v.kind === "a")` 内 `A\|B` 窄成 `A`，`v.x` 可见（红：未收窄 → `v.x` 在 `A\|B` 上 1×2339）| `type A={kind:"a";x:number}; type B={kind:"b";y:string}; declare const v:A\|B; if(v.kind==="a"){const n:number=v.x;}` → 0 诊断 | ✓ |
| `discriminant_narrowed_branch_rejects_other_constituent_property`（`check_test.rs`，1c 收窄见证）| 窄到 `A` 后访问 `B` 的属性 `v.y` 报 2339（消息是窄后 `A` 而非全 union）| 真分支内 `v.y;` → 1×2339「…does not exist on type '{ kind: "a"; x: number; }'.」 | ✓ |
| `discriminant_not_equal_narrows_to_complement_constituent`（`check_test.rs`，1c 取反见证）| `!==` 窄到补集 `B`，`v.y` 存在 | `if(v.kind!=="a"){const s:string=v.y;}` → 0 诊断 | ✓ |
| `nullish_coalesce_result_is_subtype_reduced`（`check_test.rs`，2 tracer，genuine red→green）| `??` 结果 subtype 化简（`"a"` 被 `string` 吸收）（红：旧不化简 → `string \| "a"`，消息源含字面量）| `declare const x:"a"\|undefined; declare const y:string; const n:number = x ?? y;` → 1×2322「Type 'string' is not assignable to type 'number'.」 | ✓ |
| `nullish_coalesce_mixed_with_logical_or_reports_5076`（`check_test.rs`，3 tracer，genuine red→green）| `a ?? b \|\| c` 解析 `(a??b)\|\|c`，`??` grandparent 是 `\|\|` 且左是 `??` → 报 5076（红：未实现，0≠1）| `a ?? b \|\| c;`（a/b/c:number）→ 1×5076「'??' and '\|\|' operations cannot be mixed without parentheses.」 | ✓ |
| `logical_or_then_nullish_coalesce_reports_5076`（`check_test.rs`，3 分支2，green-on-arrival）| `(a\|\|b) ?? c`，`??` 左是 `\|\|` → 5076（`\|\|`-then-`??` 序）| `a \|\| b ?? c;` → 1×5076「'\|\|' and '??' …」 | ✓ |
| `nullish_coalesce_with_logical_and_reports_5076`（`check_test.rs`，3 分支3，green-on-arrival）| `a ?? (b&&c)`，`??` 右是 `&&` → 5076（`??`-then-`&&` 序）| `a ?? b && c;` → 1×5076「'??' and '&&' …」 | ✓ |
| `parenthesized_nullish_coalesce_with_logical_or_reports_nothing`（`check_test.rs`，3 对照）| 括号消歧 → 无 5076（grandparent 是 paren 非 binary）| `(a ?? b) \|\| c;` → 0 诊断 | ✓ |

> 红→绿证据：1a/1b/1c/2/3 全部 **genuine RED**（各自实测：1a 0≠1、1b 误报 2367、1c 1×2339、2 消息源差、3 0≠1）→ 最小触点转绿。其余为 **green-on-arrival 守卫**（如实记录非伪造红，同臂/同函数其它分支落地即覆盖）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**391 单测**（+12，相对 4ba 基线 379）+ **134 doctest**（**±0**：新方法均非 pub fn / `pub(crate)` 用 `//` 注释不挂 doctest）。

## 4bc 字面量类型按值 interning（`getStringLiteralType`/`getNumberLiteralType` 值键缓存）+ union dedup by id + 退役 relations 按值绕过 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bc 落地记录」1–6）。把 Go 的 `getLiteralType` 值唯一语义落地：等值字面量（处处 `"a"`/`1`/`true`）intern 到同一 `TypeId`，给 union dedup / discriminant 一致的 Go id 语义，并退役 4bb 在 relations 层的按值绕过 `literals_equal_by_value`（`"a" === "a"` 改走 `source==target` 身份）。interning 经 `check_expression`（驱动面）+ 新 `pub(crate)` `get_string_literal_type`/`get_number_literal_type`（单测直测）；union dedup 经 `get_type_of_symbol`（声明类型解析）+ `type_to_string`/`union_types()`；shim 退役经既有 2367-FP 诊断测试保持绿（经身份）。expected 取 Go 语义（id 身份 / union 形状 / 诊断码）。**确认 Go 锚**：`getStringLiteralType(25164)`、`getNumberLiteralType(25173)`（NaN 单独缓存）、`checkExpressionWorker(7711/7717)`（fresh∘interned-regular，fresh DEFER）。**公开 API 仅加法**：无新 pub 项（`get_string_literal_type`/`get_number_literal_type` 为 `pub(crate)`；删除的 `literals_equal_by_value` 是 4bb 私有 fn）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（位置 / 角色） | 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `string_literal_expressions_intern_to_one_type_id`（`check_test.rs`，1 tracer，genuine red→green）| 两处 `"a"` 字面量表达式 intern 到同一 `TypeId`（红：`new_literal_type` 每次新 id，`TypeId(22)!=TypeId(23)`）| `"a";\n"a";` 两 `check_expression` → 同 id | ✓ |
| `distinct_string_literal_values_get_distinct_type_ids`（`check_test.rs`，1 distinct，green-on-arrival）| 不同值不串（值键缓存）| `"a";\n"b";` → 两个不同 id | ✓ |
| `number_literal_expressions_intern_to_one_type_id`（`check_test.rs`，2 tracer，genuine red→green）| 两处 `1` intern 到同一 `TypeId`（红：临时回退 numeric 臂见 `TypeId(22)!=TypeId(23)`）| `1;\n1;` → 同 id | ✓ |
| `distinct_number_literal_values_get_distinct_type_ids`（`check_test.rs`，2 distinct，green-on-arrival）| `1`/`2` 不同 id | `1;\n2;` → 两个不同 id | ✓ |
| `boolean_literal_expressions_intern_to_one_type_id`（`check_test.rs`，3，green-on-arrival 守卫）| 布尔字面量早是构造期单例（`true_type`/`false_type`）| `true;\ntrue;\nfalse;` → 两 `true` 同 id、`true`≠`false` | ✓ |
| `union_of_equal_string_literals_collapses_to_single_literal`（`declared_types_test.rs`，4，genuine red→green）| `"a" \| "a"` 经 interned id-dedup 塌缩成单字面量（红：临时回退 string 臂见 `"a" \| "a" != "a"`）| `declare const x: "a" \| "a";` → `type_to_string=="\"a\""` 且 `union_types().is_none()` | ✓ |
| `get_string_literal_type_interns_by_value`（`mod_test.rs`，单测 §8.6）| `pub(crate)` 缓存：等值同 id、异值异 id、印为 `"a"`、其自身即 regular | `get_string_literal_type("a")` 两次 → 同 id；`"b"` → 异 id；`regular_type_of_literal_type==self` | ✓ |
| `get_number_literal_type_interns_by_value_with_nan_and_zero_canonicalization`（`mod_test.rs`，单测 §8.6）| `pub(crate)` 缓存 + NaN/±0 规约（Go `nanType` 单例 + float map-key `0==-0`）| `1.0` 两次 → 同 id；`2.0` 异；`+0/-0` 同 id；`NaN` 两次 → 同 id | ✓ |
| `equality_literal_in_its_union_reports_no_overlap_diagnostic`（`check_test.rs`，4bb 既有，6 退役 shim refactor-绿）| interning 后 union 成员 `"a"` 与操作数 `"a"` 同 id，`source==target` 身份命中（删 `literals_equal_by_value` 后仍绿）| `declare const k: "a" \| "b";\nk === "a";` → 0 诊断（经身份）| ✓ |

> 红→绿证据：slice 1 / 2 / 4 **genuine RED**（1：`TypeId(22)!=TypeId(23)`；2：临时回退 numeric 臂同；4：临时回退 string 臂见 `"a" \| "a" != "a"`）→ 最小触点转绿。slice 3 / distinct 守卫为 **green-on-arrival**（布尔早单例；值键缓存天然分离异值）；slice 6 是 **refactor-绿**（interning 后 shim 冗余，删后 2367-FP 经身份保持绿 + relations 模块 13 测全绿）——**如实记录非伪造红**（同 4bb 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**399 单测**（+8，相对 4bb 基线 391）+ **134 doctest**（**±0**：新缓存方法为 `pub(crate)` 用 `//` 注释不挂 doctest；删除的私有 fn 无 doctest）。

## 4bd fresh-vs-regular 字面量配对 + `getWidenedLiteralType` + `let`/`var` 未注解初始化器 widening（const-gated）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bd 落地记录」1–3b）。三件可达语义：(1) 字面量**表达式**产 FRESH 字面量（配对到 4bc interned regular，`getFreshTypeOfLiteralType`∘`getStringLiteralType`）；(2) 未注解 `let x = "a"` widen 到 `string`、`const x = "a"` 保 `"a"`（`getWidenedLiteralType` + `getWidenedLiteralTypeForInitializer` const-gate）；(3) number/boolean widening（同臂扩展）。slice 1 经 `check_expression`+`get_string_literal_type`/`regular_type_of_literal_type`（id 身份）；slice 2/3 经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`（0-vs-2322）。expected 取 Go 语义（fresh/regular 配对 / 诊断码+消息）。**确认 Go 锚**：`getFreshTypeOfLiteralType(25146)`、`getRegularTypeOfLiteralType(25132)`、`isFreshLiteralType(25160)`、`getWidenedLiteralType(25346)`、`getWidenedLiteralTypeForInitializer(16756)`、`getTypeForVariableLikeDeclaration(16607)`、`checkVariableLikeDeclaration(5863)`。**公开 API 仅加法**：无新 pub 项（`get_fresh_type_of_literal_type`/`is_fresh_literal_type`/`get_widened_literal_type` 为 `pub(crate)`；var-decl 推断为私有 fn）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（位置 / 角色） | 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `string_literal_expression_is_fresh_paired_to_interned_regular`（`check_test.rs`，1 tracer，genuine red→green）| 字面量表达式产 fresh（≠interned regular），其 `regularType` 是 interned regular（红：旧返回 regular 自身，`TypeId(22)==TypeId(22)`）| `"a";` → `check_expression!=get_string_literal_type("a")` 且 `regular_type_of_literal_type(fresh)==get_string_literal_type("a")` | ✓ |
| `fresh_string_literal_expressions_still_intern_to_one_type_id`（`check_test.rs`，1 guard，green-on-arrival）| fresh 缓存在 regular 上 → 两处 `"a"` 同 id | `"a";\n"a";` → 同 id | ✓ |
| `let_binding_widens_string_literal_initializer_to_string`（`check_test.rs`，2 tracer，genuine red→green）| 未注解 `let` widen fresh 字面量到 `string`（红：未注解→`any`，0≠1）| `let x = "a";\nconst y: "a" = x;` → 1×2322「Type 'string' is not assignable to type '"a"'.」 | ✓ |
| `let_binding_widened_string_is_assignable_to_string`（`check_test.rs`，2 guard，green-on-arrival）| widened `string` 可赋基元 | `let x = "a";\nvar s: string = x;` → 0 诊断 | ✓ |
| `const_binding_keeps_string_literal_assignable_to_literal_target`（`check_test.rs`，2 guard，green-on-arrival）| `const` 保留字面量（combined flags 含 `Const`）| `const x = "a";\nconst y: "a" = x;` → 0 诊断 | ✓ |
| `let_binding_widens_number_literal_initializer_to_number`（`check_test.rs`，3a tracer，genuine red→green）| number 臂：`let n = 1` widen 到 `number`（红：slice 2 仅 string 臂，0≠1）| `let n = 1;\nconst m: 1 = n;` → 1×2322「Type 'number' is not assignable to type '1'.」 | ✓ |
| `let_binding_widened_number_is_assignable_to_number`（`check_test.rs`，3a guard，green-on-arrival）| widened `number` 可赋基元 | `let n = 1;\nvar x: number = n;` → 0 诊断 | ✓ |
| `let_binding_widens_boolean_literal_initializer_to_boolean`（`check_test.rs`，3b tracer，genuine red→green）| boolean 臂：`let b = true` widen 到 `boolean`（红：无 boolean 臂，0≠1）；源印为 `false \| true`（`boolean` 塌缩 DEFER 4j 印刷器）| `let b = true;\nconst c: true = b;` → 1×2322「Type 'false \| true' is not assignable to type 'true'.」 | ✓ |
| `let_binding_widened_boolean_is_assignable_to_boolean`（`check_test.rs`，3b guard，green-on-arrival）| widened `boolean` 可赋基元 | `let b = true;\nvar x: boolean = b;` → 0 诊断 | ✓ |
| `const_binding_keeps_boolean_literal_assignable_to_literal_target`（`check_test.rs`，3b guard，green-on-arrival）| `const` 布尔保留字面量 | `const b = true;\nconst c: true = b;` → 0 诊断 | ✓ |

> 红→绿证据：slice 1 / 2 / 3a / 3b **均 genuine RED**（1：`TypeId(22)==TypeId(22)`；2：未注解→`any` 0≠1；3a：仅 string 臂 0≠1；3b：仅 string+number 臂 0≠1）→ 最小触点转绿。guard 为 **green-on-arrival 守卫**（fresh 缓存分享 id；widened 可赋基元；const 保字面量经身份）——**如实记录非伪造红**（同 4bc 口径）。**踩坑**：slice 2 初版令函数/箭头初始化器被检查两次（推断+`check_variable_declaration`）→ 4 个既有 return-type 测试双报；改「无注解不二次检查」（镜像 Go `checkExpressionCached` 记忆化）转绿。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**409 单测**（+10，相对 4bc 基线 399）+ **134 doctest**（**±0**：新方法均 `pub(crate)` 不挂 doctest）。

## 4be `as const` const-assertion（抑制 widening + 保留 freshness-stripped 字面量）+ 非-const `as T` 结果类型 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4be 落地记录」slice 1 / 4）。建立在 4bd（fresh/regular 配对 + widening）之上：`check_expression` 新增 `AsExpression` 臂（Go `checkAssertion`）。**机制**：`as const`（`isConstTypeReference` 命中）→ `getRegularTypeOfLiteralType(exprType)`：把 fresh 字面量归一为 regular（非 fresh），于是 4bd 的 `getWidenedLiteralType` 在可变 binding 里对它**恒等**（只 widen fresh），字面量得以保留——`let x = "a" as const` 保 `"a"`（对照 4bd `let x = "a"` widen 到 `string`）。非-const `as T` → `getTypeFromTypeNode(typeNode)`（结果即 `T`）。全部经 `new_checker(Rc<StubProgram>)`+`get_diagnostics(root)`（0-vs-2322）驱动。expected 取 Go 语义（诊断码+消息）。**确认 Go 锚**：`checkAssertion(12238)`、`getRegularTypeOfLiteralType(25132)`、`getWidenedLiteralType(25346)`（只 widen `isFreshLiteralType`）、`ast.IsConstTypeReference(2439)`/`isConstTypeReference(128)`。**公开 API 仅加法**：无新 pub 项（`check_assertion`、`is_const_type_reference` 为私有；复用既有 `pub(crate) regular_type_of_literal_type`）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（位置 / 角色） | 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `const_assertion_on_string_literal_keeps_literal_type`（`check_test.rs`，slice 1 tracer，genuine red→green）| `"a" as const` 保留 regular 字面量 `"a"`（红：`as const` 未类型化→`error`，可赋任意，0≠1）| `let x = "a" as const;\nconst y: "b" = x;` → 1×2322「Type '"a"' is not assignable to type '"b"'.」 | ✓ |
| `const_assertion_on_string_literal_is_assignable_to_same_literal`（`check_test.rs`，slice 1 guard，green-on-arrival）| 经典 `as const`：保 `"a"` → 可赋同字面量目标（对照 4bd 同形 widen 报 2322）| `let x = "a" as const;\nconst y: "a" = x;` → 0 诊断 | ✓ |
| `const_assertion_on_string_literal_is_assignable_to_string`（`check_test.rs`，slice 1 guard，green-on-arrival）| 保留的字面量仍可赋基元 | `let x = "a" as const;\nvar s: string = x;` → 0 诊断 | ✓ |
| `const_assertion_on_number_literal_keeps_literal_type`（`check_test.rs`，slice 2，green-on-arrival）| const 臂 value-kind 无关（`getRegularTypeOfLiteralType` 归一任意 freshable）：`1 as const` 保 `1` | `let n = 1 as const;\nconst m: 2 = n;` → 1×2322「Type '1' is not assignable to type '2'.」 | ✓ |
| `const_assertion_on_number_literal_is_assignable_to_same_literal`（`check_test.rs`，slice 2 guard，green-on-arrival）| 经典 number `as const` | `let n = 1 as const;\nconst m: 1 = n;` → 0 诊断 | ✓ |
| `const_assertion_on_boolean_literal_keeps_literal_type`（`check_test.rs`，slice 3，green-on-arrival）| `true as const` 保 `true`（`trueType` 本就 fresh，const 臂归一到 regular `true`）| `let b = true as const;\nconst c: false = b;` → 1×2322「Type 'true' is not assignable to type 'false'.」 | ✓ |
| `const_assertion_on_boolean_literal_is_assignable_to_same_literal`（`check_test.rs`，slice 3 guard，green-on-arrival）| 经典 boolean `as const` | `let b = true as const;\nconst c: true = b;` → 0 诊断 | ✓ |
| `non_const_assertion_takes_asserted_type`（`check_test.rs`，slice 4 tracer，genuine red→green）| 非-const `as T` 结果即 `T`（红：非-const 臂返回 `error`，0≠1）；`"a"` 可比 `string` 故无 DEFER 的 2352 | `let x = "a" as string;\nconst y: "a" = x;` → 1×2322「Type 'string' is not assignable to type '"a"'.」 | ✓ |
| `non_const_assertion_to_matching_type_is_assignable`（`check_test.rs`，slice 4 guard，green-on-arrival）| `"a" as string` 可赋 `string` | `let x = "a" as string;\nvar s: string = x;` → 0 诊断 | ✓ |

> 红→绿证据：slice 1 / slice 4 **均 genuine RED**（1：`as const` 未类型化→`error` 可赋任意 0≠1；4：非-const 臂返回 `error` 0≠1）→ 最小触点转绿。number/boolean（slice 2/3）为 **green-on-arrival 守卫**：const 臂经 `getRegularTypeOfLiteralType` 对任意 freshable 字面量统一归一（值-kind 无关），slice 1 落地后天然泛化（布尔的 fresh/regular 对在构造期已建）——**如实记录非伪造红**（同 4bc/4bd 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**418 单测**（+9，相对 4bd 基线 409）+ **134 doctest**（**±0**：新方法均私有/`pub(crate)` 不挂 doctest）。**DEFER**：`[1,2] as const`/`{x:1} as const`（readonly 元组/对象）—— `check_expression` 无 `ArrayLiteralExpression`/`ObjectLiteralExpression` 臂（产 `error`），const 断言对其恒等；blocked-by: 数组/对象字面量类型化 + readonly 元组/对象冻结（`getRegularTypeOfObjectLiteral` + tuple readonly）。

## 4bf 对象字面量 / 数组字面量**表达式**类型化（`checkObjectLiteral` / `checkArrayLiteral` 可达子集）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bf 落地记录」slice 1 / 2 / 3）。补齐 `check_expression` 长期的地基缺口：`{ x: 1 }`（`ObjectLiteralExpression`）与 `[1, 2]`（`ArrayLiteralExpression`）此前返回 `error_type`，本轮把它们类型化。**机制**：
> - **对象字面量**（`checkObjectLiteral`）→ `createObjectLiteralType`/`newAnonymousType`：每个 `name: value` 成员经 `checkPropertyAssignment` → `checkExpressionForMutableLocation`（在此非-const/无上下文位 widen fresh 字面量）类型化，合成一个**瞬态属性符号**（4x 的 `new_synthesized_property` 之上新增 `new_object_literal_property`，直接置 `resolvedType`），放入匿名对象类型的 `members`/`properties`，对象类型带 `Anonymous|ObjectLiteral|FreshLiteral|ContainsObjectOrArrayLiteral` 旗标。
> - **数组字面量**（`checkArrayLiteral` 非-元组/非-解构/非-const 路径）→ `createArrayLiteralType(createArrayType(elementType))`：元素经 `checkExpressionForMutableLocation` widen，`getUnionType` 合元素类型，再 `createTypeReference(globalArrayType, [elementType])`（经名字解析 `Array` 的 4ad/4ae lib stand-in）。空 `[]` → strictNullChecks 开（默认 `strict != false`）取 `never`，关取 `undefined`（Go `implicitNeverType`/`undefinedWideningType`，widening 区分未建模）。
>
> 赋值性（slice 2/3 的 2322）经既有关系引擎（`properties_related_to` 迭代合成成员 + 4ad 数字索引签名实例化）天然解锁。**确认 Go 锚**：`checkObjectLiteral(13076)`、`checkPropertyAssignment(13587)`、`checkExpressionForMutableLocation(13784)`、`checkArrayLiteral(7989)`、`createArrayLiteralType(8071)`、`createArrayTypeEx(24566)`、`newAnonymousType(24951)`。**公开 API 仅加法**：新增 `pub(crate) Checker::new_object_literal_property`（其余 `check_object_literal`/`check_property_assignment`/`check_expression_for_mutable_location`/`check_array_literal`/`create_array_literal_type` 为私有方法，`property_name_text` 为私有自由 fn）；node builder `serialize_members` 改为合成符号感知（必要修复：对象字面量类型的成员是瞬态符号，旧路径经 `program.symbol(tagged)` 越界 panic）；未改任何既有 pub 签名；`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（位置 / 角色） | 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `object_literal_property_reads_member_type`（`check_test.rs`，slice 1 tracer，genuine red→green）| `{ a: 1 }` 类型化为匿名对象，成员 `a` 携 widen 后的 `number`；读 `o.a` 得 `number`（红：旧 `ObjectLiteralExpression` 产 `error`，`o.a`=`error`≠`number`，`TypeId(3)≠TypeId(8)`）| `const o = { a: 1 };\no.a;` → `check_expression(o.a)==number_type` | ✓ |
| `object_literal_string_property_reads_member_type`（`check_test.rs`，slice 1 guard）| 字符串成员经同路径 widen 到 `string` | `const o = { b: "x" };\no.b;` → `check_expression(o.b)==string_type` | ✓ |
| `object_literal_prints_structural_member_types`（`check_test.rs`，slice 1 guard）| 多成员匿名对象经 node builder 结构化印（成员声明序）| `const o = { a: 1, b: "x" };\no;` → `type_to_string=="{ a: number; b: string; }"` | ✓ |
| `object_literal_assignable_to_matching_annotation`（`check_test.rs`，slice 2，green-on-arrival，slice 1 解锁）| 结构赋值匹配注解（成员 `a: number` ↔ 目标 `a: number`）| `const o: { a: number } = { a: 1 };` → 0 诊断 | ✓ |
| `object_literal_property_mismatch_reports_2322`（`check_test.rs`，slice 2，slice 1 前 genuine red）| 成员 widen 类型不关联注解属性类型报 2322（红：slice 1 前对象=`error` 可赋任意 0 诊断）| `const o: { a: number } = { a: "x" };` → 1×2322「Type '{ a: string; }' is not assignable to type '{ a: number; }'.」 | ✓ |
| `array_literal_element_access_resolves_element_type`（`check_test.rs`，slice 3 tracer，genuine red→green）| `[1, 2]` 类型化为 `Array<number>`（元素 widen union），`arr[0]` 经数字索引签名实例化得 `number`（红：旧 `ArrayLiteralExpression` 产 `error`，`arr[0]`=`error`≠`number`）| `interface Array<T> {...}\nconst arr = [1, 2];\narr[0];` → `check_expression(arr[0])==number_type` | ✓ |
| `array_literal_element_assignable_to_number`（`check_test.rs`，slice 3，green-on-arrival）| `number[]` 元素可赋匹配注解 | `interface Array<T> {...}\nconst arr = [1, 2];\nconst n: number = arr[0];` → 0 诊断 | ✓ |
| `array_literal_element_mismatch_reports_2322`（`check_test.rs`，slice 3，tracer 前 genuine red）| `number` 元素不关联 `string` 注解报 2322（红：tracer 前 `arr`=`error` 0 诊断）| `interface Array<T> {...}\nconst arr = [1, 2];\nconst n: string = arr[0];` → 1×2322「Type 'number' is not assignable to type 'string'.」 | ✓ |
| `empty_array_literal_is_never_array_under_strict_null_checks`（`check_test.rs`，slice 3 guard）| 空 `[]` 在 strictNullChecks 开（默认）取 `never` 元素 → `Array<never>`（直接验字面量节点，避开 deferred evolving-array flow）| `interface Array<T> {...}\n[];` → `type_to_string=="Array<never>"` | ✓ |
| `empty_array_literal_is_undefined_array_without_strict_null_checks`（`check_test.rs`，slice 3 guard）| strictNullChecks 关时空 `[]` 取 `undefined` 元素 → `Array<undefined>`（经 `new_checker` 保留 program 读其 options）| `strict_null_checks=False` + `interface Array<T> {...}\n[];` → `type_to_string=="Array<undefined>"` | ✓ |

> 红→绿证据：slice 1 tracer（`TypeId(3)≠TypeId(8)`：`o.a`=`error`）/ slice 3 tracer（`arr[0]`=`error`≠`number`）**均 genuine RED**→最小触点转绿；slice 2 的 2322 在 slice 1 落地前也是 genuine red（对象=`error` 可赋任意，0 诊断）。slice 2/3 的「0 诊断」正例与对象多成员印刷为 **green-on-arrival 守卫**（slice 1 / slice 3 tracer impl 一并解锁结构赋值/印刷）——**如实记录非伪造红**（同 4bc/4bd/4be 口径）。**踩坑修正**：对象字面量类型的成员是瞬态合成符号，node builder `serialize_members` 旧版经 `program.symbol(tagged_id)` 越界 panic（`index 2147483648`）；改为 `is_synthesized_symbol` 分流到 `synthesized_symbol_name` 后转绿（同时修了 2322 消息里源类型 `{ a: string; }` 的印刷）。**踩坑修正 2**：空 `[]` 默认取 `never`（非 `undefined`）——默认 options `strict != false` 使 strictNullChecks 开；且经 `Checker::new()` 直接 `check_expression` 时 options 取默认（非 program 的），故 strictNullChecks-off 臂须经 `new_checker(Rc<program>)` 保留 program 才能驱动。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**428 单测**（+10，相对 4be 基线 418）+ **134 doctest**（**±0**：新增 `pub(crate)` 方法用 `//` 行注释、不挂 doctest）。**DEFER**：spread 成员（`{...o}`）/ 计算属性名 / get-set-方法成员 / shorthand / 上下文类型（类型流入字面量）/ 元组推断 / `as const` readonly 冻结 / excess-property `2353`；blocked-by: spread 类型合并（`getSpreadType`）+ 计算名类型化 + 访问器/方法签名收集 + shorthand 解析 + 上下文类型传递 + 关系引擎 excess-property elaboration。

## 4bg fresh object literal 的 excess-property 检查 `2353` 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bg 落地记录」slice 1–4）。承接 4bf：对象字面量已置 `FreshLiteral` 旗标。本轮在 `check_variable_declaration` 的赋值性检查点对 **fresh object literal** 跑 `hasExcessProperties` 可达子集：命中报 `2353`（错误节点定位到字面量成员名）并**抑制 `2322`**（镜像 Go `reportRelationError` 的 excess 抑制）。**Go 锚**：`relater.go:hasExcessProperties(2695)` / `recursiveTypeRelatedToWorker(2647)` / `reportRelationError(4773)` / `isKnownProperty(716)` / `isExcessPropertyCheckTarget(746)`；`checker.go:isEmptyObjectType(26326)` / `widenTypeForVariableLikeDeclaration(18101)`→`getWidenedType(18214)`→`getWidenedTypeOfObjectLiteral(18259)`；`getNamedMembers(21907)`/`isReservedMemberName(1584)`。诊断文本：`Object_literal_may_only_specify_known_properties_and_0_does_not_exist_in_type_1`（2353）。**公开 API 仅加法**：新增 `pub(crate)` 关系谓词（`is_object_literal_type`/`is_excess_property_check_target`/`is_known_property`/`is_empty_object_type`，`relations.rs`）+ `get_widened_type`（`check.rs`）+ `get_applicable_index_info_for_name`（`declared_types.rs`）；其余为私有方法/自由 fn；未改既有 pub 签名 / lib.rs 导出 / 依赖 / `.go`。

| Rust 测试（文件，切片，红/绿性质）| 行为 | input → expected | 完成 |
|---|---|---|---|
| `object_literal_excess_property_reports_2353`（`check_test.rs`，slice 1 tracer，genuine red→green）| fresh 字面量多余属性报 `2353`（红：0 诊断——关系忽略多余属性）| `const o: { a: number } = { a: 1, b: 2 };` → 1×2353「Object literal may only specify known properties, and 'b' does not exist in type '{ a: number; }'.」 | ✓ |
| `object_literal_no_excess_property_reports_nothing`（`check_test.rs`，slice 1 正控）| 无多余属性 → 0 诊断 | `const o: { a: number } = { a: 1 };` → 0 诊断 | ✓ |
| `non_fresh_object_source_reports_no_excess_property`（`check_test.rs`，slice 3，genuine red→green）| 非-fresh 源不触发 excess（变量 widening 剥离 freshness）（红：`src` 保留 fresh → 误报 2353）| `const src = { a: 1, b: 2 };\nconst o: { a: number } = src;` → 0 诊断 | ✓ |
| `index_signature_target_suppresses_excess_property`（`check_test.rs`，slice 2，genuine red→green）| 索引签名使任意 string 名属性"已知" → 抑制 2353（红：无索引通道误报 2353 + `__index` 泄漏致 2322）| `interface T { [k: string]: number }\nconst o: T = { a: 1, b: 2 };` → 0 诊断 | ✓ |
| `empty_object_target_suppresses_excess_property`（`check_test.rs`，slice 2b，genuine red→green）| 空对象 `{}` 接受任意属性 → 抑制 2353（红：误报 2353）| `const o: {} = { a: 1 };` → 0 诊断 | ✓ |
| `is_object_literal_type_distinguishes_literals`（`relations_test.rs`，谓词单测）| `isObjectLiteralType`：字面量 true；intrinsic/interface false | `const o = { a: 1 };` 初始化器类型 / `string` / `interface I` | ✓ |
| `is_excess_property_check_target_classifies_types`（`relations_test.rs`，谓词单测）| object/non-primitive=target；primitive≠target；union（some）/intersection（every）| `interface A`/`B` / `object` / `string` / `A\|string` / `A&B` | ✓ |
| `is_known_property_property_and_index_paths`（`relations_test.rs`，谓词单测）| 名义属性查 + 索引签名查 + union（some）递归 | `interface Named { x }` / `interface Indexed { [k]: number }` / `Named\|Indexed` | ✓ |
| `is_empty_object_type_recognizes_empty_targets`（`relations_test.rs`，谓词单测）| `{}`/`object` 空；有属性/索引签名 非空；primitive 非空 | `interface Empty {}` / `NonEmpty { x }` / `Indexed { [k] }` / `object` / `string` | ✓ |
| `get_widened_type_strips_object_literal_freshness`（`relations_test.rs`，谓词单测）| fresh object literal → 去 `ObjectLiteral` 旗标的 regular 类型（distinct id）；幂等；对非字面量恒等 | `const o = { a: 1 };` 初始化器类型 / `string` | ✓ |
| `applicable_index_info_for_name_matches_string_index`（`declared_types_test.rs`，单测）| `getApplicableIndexInfoForName`：string 索引适配任意名（值=number）；无索引签名→None | `interface S { [k: string]: number }` / `interface N { x: number }` | ✓ |
| `get_properties_of_type_excludes_reserved_index_member`（`declared_types_test.rs`，单测）| `getNamedMembers` 过滤 reserved `__index` 成员（只剩 `x`）| `interface I { x; [k]: number }` → `["x"]` | ✓ |

> 红→绿证据：slice 1（0→1×2353）/ slice 3（误报 2353→0，widening 剥离 fresh）/ slice 2（2353→2322→0，补索引通道 + reserved-名过滤）/ slice 2b（2353→0，空对象抑制门）**均 genuine RED**→最小触点转绿（每片单独 `cargo test -p tsgo_checker <name>` 看红/绿）。**关键洞察**：(1) Go `reportRelationError` 链头是 `2353`/`2561` excess 消息时早返回，故最终**只报 2353 不报 2322**；(2) Go 用 `getWidenedType` 在**变量声明类型**计算处丢 `FreshLiteral`/`ObjectLiteral` 旗标——故对象字面量赋变量后**经变量读回不再 fresh**（slice 3 不触发 excess）；本港此前缺 `widenTypeForVariableLikeDeclaration` 的 `getWidenedType` 步，本轮补上（可达子集：顶层 fresh object literal 去旗标）。**踩坑修正**：索引签名目标的 2353 抑制曾被 `__index` 成员泄漏进 `properties` 致 `2322` 遮蔽——补 `is_reserved_member_name` 过滤（镜像 `getNamedMembers`）使 target 名义属性表正确为空。**测试增量**：**440 单测**（+12，相对 4bf 基线 428：4 集成切片含正控 + 5 关系谓词单测 + `applicable_index_info_for_name` + reserved-名过滤）+ **134 doctest**（**±0**：新增 `pub(crate)` 方法/fn 用 `//` 行注释、不挂 doctest）。gate：clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿。**DEFER**：`2561`「Did you mean to write」建议变体（`getSuggestionForNonexistentProperty`）/ spread 经过的 excess（`shouldCheckAsExcessProperty` + `getSpreadType`）/ union-target excess 归约（`findMatchingDiscriminantType` + `checkTypes` 的 `Types_of_property_0_are_incompatible`）/ `suppressExcessPropertyErrors` + `// @ts-expect-error` / JS-literal 索引模拟 / `globalObjectType` subset（lib globals P6）/ JSX-attribute 消息变体 / 完整 `indexSignaturesRelatedTo` 结构关系 / late-bound·substitution·mapped 臂 / `getWidenedType` 全量（union/array/context/嵌套 re-widen）。

## 4bh 对象字面量 shorthand 属性 `{ a }` + 计算属性名索引签名 `{ [k]: v }` 行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bh 落地记录」slice 1–3 + 附加）。承接 4bf（对象字面量 `PropertyAssignment` 类型化）/ 4bg（excess-property 2353 + `getApplicableIndexInfoForName`）。本轮把 `checkObjectLiteral` 成员循环再镜像两类 `ObjectLiteralElement`：**shorthand 属性**（`{ a }` ≡ `{ a: a }`，成员类型 = 引用标识符经 `checkExpressionForMutableLocation` widen 的类型）+ **非字面量计算属性名**（`{ [k]: v }`，`k: string/number/symbol` 经 `getObjectLiteralIndexInfo` 合成 string/number/symbol **索引签名**，而非命名属性）。**Go 锚**：`checker.go:checkObjectLiteral(13076)`（成员 switch 13153 + 计算名 13117/13150 + `hasComputed*Property` 块 13244 + `createObjectLiteralType` 索引合成 13122）/ `checkShorthandPropertyAssignment(13603)` / `checkComputedPropertyName(26619)` / `getObjectLiteralIndexInfo(19576)` / `isSymbolWithSymbolName(19596)` / `isSymbolWithNumericName(19607)` / `utilities.go:isNumericLiteralName(860)` / `isTypeUsableAsPropertyName(841)`。诊断文本：`A_computed_property_name_must_be_of_type_string_number_symbol_or_any`（2464）。**公开 API 仅加法**：本轮**无新 pub 项**——新方法 `check_shorthand_property_assignment`/`check_computed_property_name`/`get_object_literal_index_info`/`is_object_literal_member_with_{symbol,numeric}_name` 均私有，新自由 fn `is_numeric_literal_name` 私有，私有 struct `ObjectLiteralMember`；复用 4bg `pub(crate)` 基建；未改既有 pub 签名 / lib.rs 导出 / 依赖 / `.go`；`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（文件，切片，红/绿性质）| 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `object_literal_shorthand_property_reads_referenced_var_type`（`check_test.rs`，slice 1 tracer，genuine red→green）| shorthand `{ a }` 取引用变量类型并 widen（`const a = 1` fresh `1` → `number`）；读 `o.a` 得 `number`（红：shorthand 未处理 → `o` 空对象 → `o.a`=`error`，`TypeId(3)≠TypeId(8)`）| `const a = 1;\nconst o = { a };\no.a;` → `check_expression(o.a)==number_type` | ✓ |
| `object_literal_shorthand_property_mismatch_reports_2322`（`check_test.rs`，slice 2，slice 1 前 genuine red）| shorthand 携引用类型流入 → 与注解不匹配报 2322（红：slice 1 前 `{ a }`=`{}`，报「源 `{}`」而非 `{ a: number; }` 的不匹配——实测 `Type '{}' is not assignable to ...`）| `const a = 1;\nconst o: { a: string } = { a };` → 1×2322「Type '{ a: number; }' is not assignable to type '{ a: string; }'.」 | ✓ |
| `object_literal_shorthand_property_assignable_to_matching_annotation`（`check_test.rs`，slice 2 正控，green-on-arrival）| shorthand 类型匹配注解 → 0 诊断 | `const a = 1;\nconst o: { a: number } = { a };` → 0 诊断 | ✓ |
| `object_literal_computed_string_name_synthesizes_string_index`（`check_test.rs`，slice 3 tracer，genuine red→green）| 非字面量 `string` 计算名 → string 索引签名（值=widen 后 `number`）；`o["anything"]` 得 `number`（红：计算名成员被跳过 → 无索引签名 → `o["anything"]`=`error`，`TypeId(3)≠TypeId(8)`）| `const k: string = "x";\nconst o = { [k]: 1 };\no["anything"];` → `check_expression==number_type` | ✓ |
| `object_literal_named_property_coexists_with_computed_name`（`check_test.rs`，slice 3 正控，green-on-arrival）| 计算名成员旁的命名属性仍是命名属性（不被索引签名吞）| `const k: string = "x";\nconst o = { b: 2, [k]: 1 };\no.b;` → `check_expression(o.b)==number_type` | ✓ |
| `object_literal_computed_number_name_synthesizes_number_index`（`check_test.rs`，slice 3b tracer，genuine red→green）| 非字面量 `number` 计算名 → number 索引签名；`o[0]` 得 `number`（红：同 slice 3，计算名成员被跳过）| `const k: number = 0;\nconst o = { [k]: 1 };\no[0];` → `check_expression(o[0])==number_type` | ✓ |
| `object_literal_computed_name_non_indexable_reports_2464`（`check_test.rs`，slice 3c，genuine red→green）| 计算名表达式既非 string/number/symbol-like 又不可赋其 union（且非 any）报 2464（红：emission 未接 → 0 诊断）| `const k: boolean = true;\nconst o = { [k]: 1 };` → 1×2464「A computed property name must be of type 'string', 'number', 'symbol', or 'any'.」 | ✓ |
| `object_literal_string_index_value_is_assignable_to_number`（`check_test.rs`，slice 3 端到端，green-on-arrival）| 合成 string 索引值（`number`）经 `getApplicableIndexInfoForName`/`getIndexedAccessType` 可赋 `number` 注解 | `const k: string = "x";\nconst o = { [k]: 1 };\nconst n: number = o["foo"];` → 0 诊断 | ✓ |
| `object_literal_string_index_value_mismatch_reports_2322`（`check_test.rs`，slice 3 端到端，slice 3 前 genuine red）| string 索引值 `number` 不可赋 `string` 注解报 2322（红：slice 3 前无索引签名 → `o["foo"]`=`error` 可赋任意 0 诊断）| `const k: string = "x";\nconst o = { [k]: 1 };\nconst s: string = o["foo"];` → 1×2322「Type 'number' is not assignable to type 'string'.」 | ✓ |
| `is_numeric_literal_name_matches_round_trip`（`check_test.rs`，单测）| `isNumericLiteralName`：JS-number 文本 round-trip == 文本（`"0"`/`"123"`/`"1.5"` 真；`"0xF00D"`/`"01"`/`"a"`/`""`/`"\u{FE}computed"` 假）| 见断言 | ✓ |

> 红→绿证据：slice 1 tracer（`TypeId(3)≠TypeId(8)`：`o.a`=`error`）/ slice 3 tracer（`o["anything"]`=`error`≠`number`）/ slice 3b tracer（`o[0]`=`error`）/ slice 3c（2464 emission 未接 → 0 诊断）**均 genuine RED**→最小触点转绿。slice 2 经**临时禁用 shorthand 臂**实测红：源印为 `{}`（实测 `Type '{}' is not assignable to type '{ a: string; }'.`），落地后转 `{ a: number; }`。slice 3c 经**临时 `if false &&` 禁用 emission**实测红（0 诊断）。正控「0 诊断」/ 命名属性共存为 **green-on-arrival 守卫**（slice 1 / slice 3 impl 一并解锁）——**如实记录非伪造红**（同 4bc/4bd/4be/4bf/4bg 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**450 单测**（+10，相对 4bg 基线 440：shorthand×3 + 计算名 string/number/coexist×3 + 2464×1 + 索引端到端×2 + `is_numeric_literal_name` 单测×1）+ **134 doctest**（**±0**：本轮新方法/fn 均私有、不挂 doctest）。gate：`cargo test -p tsgo_checker`（450+134）绿、clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿。**DEFER（带 blocked-by）**：`{ a = 1 }` shorthand 默认值（解构-only，使属性 optional）/ shorthand 显式类型注解（grammar-error，`checkTypeAssignableToAndOptionallyElaborate`）/ 字面量·unique-symbol 计算名 → 晚绑定**命名**成员（`isTypeUsableAsPropertyName`→`getPropertyNameFromType`）/ spread·accessor·method 成员 / 上下文类型 / `as const` readonly 索引（`isConstContext`→index `readonly`）/ `n in obj` 计算名特例 / `typeNodeLinks` 计算名缓存 + spread 预扫 / `components`（冲突计算名声明）+ 已知符号（`IsKnownSymbol`）。

## 4bi `as const` const-context 对象/数组字面量类型化（readonly 元组 + readonly 字面量属性）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bi 落地记录」slice 1/1b/2a/2b/2c + NC + 附加）。承接 4be（`as const` 单体字面量 freshness-stripping，显式 DEFER 了「数组/对象 as const」）/ 4bf（对象/数组字面量表达式类型化）。本轮落地 Go `isConstContext` 的语法递归透传：`[1, 2] as const` → readonly 元组 `readonly [1, 2]`（元素保留字面量 `1`/`2`）；`{ a: 1 } as const` → `{ readonly a: 1; }`（属性符号带 `Readonly` check flag + 字面量类型）。机制：`checkExpressionForMutableLocation` 的 `isConstContext` 臂返回 `getRegularTypeOfLiteralType`（保字面量），`checkArrayLiteral` const 臂建 readonly 元组，`checkObjectLiteral` const 臂给属性 `CheckFlagsReadonly`。**Go 锚**：`checker.go:isConstContext(13529)` / `checkExpressionForMutableLocation(13784)` / `checkArrayLiteral(7989)` / `checkObjectLiteral(13076,13104,13124)` / `getRegularTypeOfLiteralType(25132)`；`ast/utilities.go:IsConstAssertion(2431)`/`IsConstTypeReference(2439)`。**公开 API 仅加法**：`ObjectType` 加 `pub readonly: bool`（additive，所有构造经 `..Default::default()`）+ 新 `pub(crate)` `create_tuple_type_ex`/`synthesized_symbol_check_flags`；`pub(crate) new_object_literal_property` 加 `check_flags` 入参（非 pub API）；其余私有；未改既有 pub 签名 / lib.rs 导出 / 依赖 / `.go`；`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（文件，切片，红/绿性质）| 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `const_assertion_on_array_literal_keeps_literal_element_types`（`check_test.rs`，slice 1 tracer，genuine red→green）| `[1, 2] as const` → readonly 元组，元素保留字面量；`t[0]` 得字面量 `1`（红：旧无 const 臂 → `Array<number>`，`t[0]`=`number`，`TypeId(8)≠TypeId(22)`）| `interface Array<T>{...}\nconst t = [1, 2] as const;\nt[0];` → `check_expression(t[0])==get_number_literal_type(1)` | ✓ |
| `const_assertion_on_array_literal_prints_readonly_tuple`（`check_test.rs`，slice 1b tracer，genuine red→green）| readonly 元组印 `readonly [1, 2]`（红：元组旧落 `serialize_members` 印 `{}`）| `const t = [1, 2] as const;\nt;` → `type_to_string=="readonly [1, 2]"` | ✓ |
| `const_assertion_on_object_literal_keeps_literal_property_type`（`check_test.rs`，slice 2a，green-on-arrival）| const 对象属性保留字面量（共享 slice 1 的 mutable-location const 臂）| `const o = { a: 1 } as const;\no.a;` → `check_expression(o.a)==get_number_literal_type(1)` | ✓ |
| `const_assertion_on_object_literal_marks_property_readonly`（`check_test.rs`，slice 2b tracer，genuine red→green）| const 对象属性符号带 `CheckFlags::READONLY`（红：旧属性 check flags 恒空）| `const o = { a: 1 } as const;\no;` → `synthesized_symbol_check_flags(prop("a")).contains(READONLY)` | ✓ |
| `const_assertion_on_object_literal_prints_readonly_member`（`check_test.rs`，slice 2c tracer，genuine red→green）| const 对象印 `{ readonly a: 1; }`（红：旧印 `{ a: 1; }` 无 readonly）| `const o = { a: 1 } as const;\no;` → `type_to_string=="{ readonly a: 1; }"` | ✓ |
| `non_const_object_literal_property_is_widened_and_mutable`（`check_test.rs`，NC，green-on-arrival）| 无 `as const` 时对象属性 widen 且非 readonly（4bf 不变，无 const 泄漏）| `const o = { a: 1 };\no;` → `type_to_string=="{ a: number; }"` 且 prop 无 `READONLY` | ✓ |
| `non_const_array_literal_is_array_not_tuple`（`check_test.rs`，NC，green-on-arrival）| 无 `as const` 时数组仍 `Array<T>`（非元组、非 readonly，4bf 不变）| `interface Array<T>{...}\nconst t = [1, 2];\nt;` → `type_to_string=="Array<number>"` | ✓ |
| `const_assertion_on_array_literal_second_element_keeps_literal`（`check_test.rs`，附加）| 元组位置存储第 2 元素也保字面量；`t[1]` 得 `2` | `interface Array<T>{...}\nconst t = [1, 2] as const;\nt[1];` → `check_expression(t[1])==get_number_literal_type(2)` | ✓ |
| `const_assertion_propagates_into_nested_array_literal`（`check_test.rs`，附加·嵌套深度）| 对象→数组 1 层嵌套 const 透传 | `const o = { a: [1, 2] } as const;\no;` → `type_to_string=="{ readonly a: readonly [1, 2]; }"` | ✓ |
| `const_assertion_propagates_into_nested_inner_array_literal`（`check_test.rs`，附加·嵌套深度）| 数组→数组嵌套 const 透传 | `const t = [[1]] as const;\nt;` → `type_to_string=="readonly [readonly [1]]"` | ✓ |
| `create_tuple_type_ex_sets_readonly_flag`（`mod_test.rs`，单测）| `create_tuple_type_ex(_, true)` readonly=true、`create_tuple_type` readonly=false；元素位置存储；TUPLE 旗标 | 见断言 | ✓ |
| `type_to_string_tuple_elements`（`nodebuilder_test.rs`，单测）| 非-readonly 元组印 `[string, number]` | `create_tuple_type([string, number])` → `"[string, number]"` | ✓ |
| `type_to_string_readonly_tuple_elements`（`nodebuilder_test.rs`，单测）| readonly 元组印 `readonly [string, number]` | `create_tuple_type_ex([string, number], true)` → `"readonly [string, number]"` | ✓ |

> 红→绿证据：slice 1（`t[0]`=`number`(`TypeId(8)`)≠字面量 `1`(`TypeId(22)`)）/ 1b（元组印 `{}`）/ 2b（属性 check flags 空、不含 `READONLY`）/ 2c（印 `{ a: 1; }` 无 readonly）**均 genuine RED**→最小触点转绿。slice 2a 为 **green-on-arrival**（与数组元素共用 `check_expression_for_mutable_location` const 臂，slice 1 落地即覆盖对象属性值）。NC/嵌套/单测为守卫——**如实记录非伪造红**（同 4bc–4bh 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**支持的 const-context 嵌套深度**：**任意层**（`is_const_context` 全递归镜像 Go）。**测试增量**：**463 单测**（+13，相对 4bh 基线 450：check_test +10 / nodebuilder_test +2 / mod_test +1）+ **134 doctest**（**±0**：新方法均 `pub(crate)`/私有、不挂 doctest）。gate：`cargo test -p tsgo_checker`（463+134）绿、clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿。**DEFER（带 blocked-by）**：`isConstContext` 上下文-const 分支（`isConstTypeVariable`+`getContextualType`）/ `<const>expr`（`TypeAssertionExpression`）表达式臂 / `forceTuple`·tuple-like 上下文·mutableArrayLike 清 readonly / `createArrayLiteralType` 旗标克隆+缓存 / 嵌套元组 per-元素 re-widening / 元组 spread·变长元素·`length`/`[number]` const-readonly / readonly-tuple↔`ReadonlyArray` 赋值性 + 2540 readonly 写诊断 / program 成员 readonly 修饰印刷 / `getRegularTypeOfObjectLiteral` + union `regularType` 链。

## 4bj 上下文类型地基：箭头/函数表达式无注解参数的上下文参数类型（`getContextualType` 家族 + 函数类型节点）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bj 落地记录」slice 1/2 + 守卫 + 单测）。落地上下文类型可达切片：`const f: T = <expr>` 变量初始化器位 + `x = <expr>` 赋值 RHS 位 → 注解类型为上下文类型；据此为箭头/函数表达式**无注解参数**赋上下文参数类型（**惰性** `getTypeForVariableLikeDeclaration` 参数臂 + **急性** `contextuallyCheckFunctionExpressionOrObjectLiteralMethod`）。前置：函数类型节点 `(x: number) => void` 首次类型化为带 call 签名的匿名对象类型。**Go 锚**：`checker.go:getContextualType(29019)`/`getContextualTypeForInitializerExpression(29099)`/`getContextualTypeForVariableLikeDeclaration(29114)`/`getContextuallyTypedParameterType(29134)`/`getContextualSignature(10224)`/`getContextualCallSignature(10265)`/`isAritySmaller(10293)`/`assignContextualParameterTypes(10309)`/`assignParameterType(10364)`/`getApparentTypeOfContextualType(30361)`/`getContextualTypeForBinaryOperand(29485)`/`getContextualTypeForAssignmentExpression(29519)`/`contextuallyCheckFunctionExpressionOrObjectLiteralMethod(10112)`/`getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode`/`getTypeForVariableLikeDeclaration`。**公开 API 仅加法**：无新 `pub` 项、无既有 `pub fn` 签名变更、无 `lib.rs`/依赖/`.go` 改动；新增项均 `pub(crate)`（`ContextFlags` + contextual 方法）或私有；`get_type_from_type_node`(既有 pub) 对 `FunctionType`/`ConstructorType` 由 error 变对象类型（additive 行为扩展，零回归）。`cargo build -p tsgo_compiler` 绿。

| Rust `#[test]`（文件，切片，红/绿性质）| 验证行为 | 最小 input → expected（Go 语义） | 完成 |
|---|---|---|---|
| `arrow_parameter_is_contextually_typed_by_variable_annotation`（`contextual_test.rs`，slice 1 tracer，genuine red→green）| 无注解箭头参数被变量函数类型注解上下文化（**惰性**通道）（红：旧 `get_type_of_symbol(x)`=`any`，`TypeId(1)≠TypeId(8)`）| `const f: (x: number) => void = (x) => { x; };` → body `x` == `number` | ✓ |
| `contextual_parameter_type_flows_into_body_and_surfaces_mismatch`（`contextual_test.rs`，slice 2，genuine red→green）| 上下文参数类型真流入 body，误用 surfaces `2322`（**急性**通道）（红：临时中和 `get_contextual_signature` → `0≠1` 诊断）| `const f: (x: number) => void = (x) => { const s: string = x; };` → 1×2322 `number`→`string` | ✓ |
| `contextual_parameter_type_flows_into_body_assignable_ok`（`contextual_test.rs`，slice 2 正控，green-on-arrival）| 正确赋值无诊断 | `... { const n: number = x; }` → 0 诊断 | ✓ |
| `explicit_parameter_annotation_overrides_contextual_type`（`contextual_test.rs`，slice 3 守卫，green-on-arrival）| 显式注解胜过上下文类型（Go 不覆盖）| `const f: (x: number) => void = (x: string) => { x; };` → `x` == `string` | ✓ |
| `bare_arrow_parameter_without_context_stays_any`（`contextual_test.rs`，slice 3 守卫，无回归）| 无上下文签名 → 参数留 `any`（无 panic）| `const g = (x) => x;` → `get_type_of_symbol(x)` == `any` | ✓ |
| `get_contextual_type_of_variable_initializer_is_annotation_type`（`contextual_test.rs`，单测）| 注解变量初始化器位上下文类型 == 注解类型 | `const x: number = 0;` → `get_contextual_type(0)` == `Some(number)` | ✓ |
| `get_contextual_type_of_unconstrained_expression_is_none`（`contextual_test.rs`，单测）| 无上下文源 → None | `0;` → `get_contextual_type` == `None` | ✓ |
| `get_contextually_typed_parameter_type_uses_positional_signature_parameter`（`contextual_test.rs`，单测）| 按位读上下文签名参数；签名 2 参 | `const f: (a: string, b: number) => void = (a, b) => {};` → `a`==`string`、`b`==`number` | ✓ |
| `assign_contextual_parameter_types_caches_on_symbol_links`（`contextual_test.rs`，单测，急性独立见证）| 急性赋值缓存到 `value_symbol_links.resolved_type`（赋值前空，后 `Some(number)`）| `const f: (x: number) => void = (x) => {};` | ✓ |
| `function_type_node_resolves_to_object_with_call_signature`（`contextual_test.rs`，单测）| 函数类型节点 → 1 call 签名，参 0==`number`，返回==`void` | `let f: (x: number) => void;` → 见断言 | ✓ |
| `get_contextual_type_of_assignment_rhs_is_target_type`（`contextual_test.rs`，单测，赋值 RHS 臂）| 赋值 RHS 上下文类型 == 目标声明类型 | `let x: number;\nx = 0;` → `get_contextual_type(0)` == `Some(number)` | ✓ |
| `arrow_parameter_is_contextually_typed_via_assignment_rhs`（`contextual_test.rs`，单测，赋值 RHS → 箭头参数）| 赋给注解标识符的箭头参数也被上下文化 | `let f: (x: number) => void;\nf = (x) => { x; };` → body `x` == `number` | ✓ |

> 红→绿证据：slice 1 **genuine RED**（初始无实现：`x`=`TypeId(1)` any≠`TypeId(8)` number；临时中和惰性通道复现）；slice 2 **genuine RED**（临时中和 `get_contextual_signature` → `0≠1` 诊断）。两片测**不同通道**（slice 1 惰性、slice 2 急性），互补。正控/守卫/单测为 green-on-arrival——**如实记录非伪造红**（同 4bc–4bi 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**475 单测**（+12，相对 4bi 基线 463：contextual_test +12）+ **134 doctest**（**±0**：新方法/类型均 `pub(crate)`/私有、不挂 doctest）。gate：`cargo test -p tsgo_checker`（475+134）绿、clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿。**DEFER（带 blocked-by）**：call/new 实参（call-resolution contextual 通道）/ return·yield·await 位 / 对象·数组字面量元素（类型流入字面量，反方向）/ JSX / `getApparentTypeOfContextualType` 的 instantiate·mapType·union 判别 / union 上下文签名 + `getIntersectedSignatures` / 泛型签名推断 / `this` 参数 / rest·optional·IIFE 参数映射 / 参数默认初始化器协调 / binding-pattern / 赋值 access-expression 目标 + 复合·逻辑算子 / construct 签名·泛型函数类型·节点类型缓存 / `isContextSensitive` 门·`ContextChecked` 重入·`with` 早退·`findContextualNode` 缓存。

## 4bk 上下文类型**流入**对象/数组字面量（注解属性/元素类型反向流向字面量成员/元素，字面量保留）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bk 落地记录」slice 1/2 + 守卫 + 任务切片/负控 + 单测）。承接 4bj（上下文类型地基）/ 4bf（字面量自底向上类型化）。本轮接通**反方向**：`checkExpressionForMutableLocation` 的 default 分支 `getWidenedLiteralLikeTypeForContextualType(t, getContextualType(node))`——对象属性值 / 数组元素若处于**字面量上下文**则保留字面量而非 widen。**关键事实**：任务命名的两切片（`const xs: number[] = []; xs[0]`、`const o: { xs: number[] } = { xs: [] }; o.xs[0]`）**已 green-on-arrival**（`xs`/`o` 类型来自注解，与字面量本身无关；空数组 `[]` 在 Go 里恒 `Array<never>`），故真正 genuine RED 是字面量保留（对象伪 2322 / 数组 `Array<string>`→`Array<"a">`）。**Go 锚**：`checker.go:checkExpressionForMutableLocation(13784)`/`getWidenedLiteralLikeTypeForContextualType(25374)`/`isLiteralOfContextualType(25381)`/`getContextualType(29019)`/`getContextualTypeForObjectLiteralElement(29596)`/`getContextualTypeForElementExpression(29648)`/`getTypeOfPropertyOfContextualType(30226)`/`getTypeFromIndexInfosOfContextualType(30321)`/`getApparentTypeOfContextualType(30361)`/`getIteratedTypeOrElementType`/`checkArrayLiteral(7989)`（空数组 `implicitNeverType`）。**公开 API 仅加法**：无新 `pub` 项、无既有 `pub fn` 签名变更、无 `lib.rs`/依赖/`.go` 改动；新增 2 `pub(crate)` 方法（`is_literal_of_contextual_type`/`get_widened_literal_like_type_for_contextual_type`）+ 5 私有方法 + 2 私有自由 fn；`check_iterated_type_or_element_type` `fn`→`pub(crate)`（内部可见性放宽）。

| Rust 测试（文件 / 角色） | 验证行为 | 最小 input → expected | 完成 |
|---|---|---|---|
| `object_literal_property_literal_preserved_by_contextual_type`（`check_test.rs`，slice 1 tracer，genuine red→green）| 对象属性值字面量经上下文属性类型保留（不 widen）| `const o: { a: "x" } = { a: "x" };` → 0 诊断（红：1×2322 `{ a: string; }`→`{ a: "x"; }`）| ✓ |
| `object_literal_property_mismatched_literal_kind_still_reports_2322`（`check_test.rs`，slice 1 守卫，green-on-arrival）| kind 不匹配仍 widen + 报 2322 | `const o: { a: "x" } = { a: 1 };` → 1×2322 `{ a: number; }`→`{ a: "x"; }` | ✓ |
| `array_literal_element_literal_preserved_by_contextual_type`（`check_test.rs`，slice 2 tracer，genuine red→green）| 数组元素字面量经上下文元素类型保留 | `const xs: "a"[] = ["a"];` → 字面量 `["a"]` == `Array<"a">`（红：临时 `if false` 禁用 ArrayLiteralExpression 臂 → `Array<string>`）| ✓ |
| `array_literal_without_context_still_widens_elements`（`check_test.rs`，slice 2 守卫，无回归）| 无上下文 → 退化为 widen | `const ys = ["a"];` → `Array<string>` | ✓ |
| `annotated_empty_array_variable_element_access_is_number`（`check_test.rs`，任务切片 1，green-on-arrival）| 注解变量空数组元素访问（注解路径）| `const xs: number[] = []; xs[0];` → `number` | ✓ |
| `annotated_empty_array_literal_itself_stays_never_array`（`check_test.rs`，任务切片 1 负控，Go 忠实）| 空数组字面量本身恒 `Array<never>`（上下文不改空数组元素类型）| `const xs: number[] = []; xs;` 字面量 `[]` == `Array<never>` | ✓ |
| `annotated_object_empty_array_property_element_access_is_number`（`check_test.rs`，任务切片 2，green-on-arrival）| 注解对象空数组属性元素访问（注解路径）| `const o: { xs: number[] } = { xs: [] }; o.xs[0];` → `number` | ✓ |
| `get_contextual_type_of_object_literal_property_value_is_property_type`（`contextual_test.rs`，单测）| object-literal-element 臂 → 匹配属性类型 | `const o: { a: "x" } = { a: "x" };` → `get_contextual_type(value)` == `Some("x")` | ✓ |
| `get_contextual_type_of_unknown_object_literal_property_is_none`（`contextual_test.rs`，单测）| 名不在上下文 → None | `const o: { a: number } = { b: 1 };` → `get_contextual_type(b 值)` == `None` | ✓ |
| `get_contextual_type_of_array_literal_element_is_element_type`（`contextual_test.rs`，单测）| ArrayLiteralExpression 臂 → 迭代元素类型 | `const xs: "a"[] = ["a"];` → `get_contextual_type(element)` == `Some("a")` | ✓ |
| `is_literal_of_contextual_type_matches_same_kind_only`（`contextual_test.rs`，单测）| 同 kind 字面量上下文 true，余 false | string-lit/string-lit→T；/number-lit→F；/`string`→F；/None→F | ✓ |
| `get_widened_literal_like_type_for_contextual_type_preserves_or_widens`（`contextual_test.rs`，单测）| 匹配上下文保留、无/不匹配 widen | fresh`"x"`+`"x"`→regular`"x"`；+None→`string`；+`string`→`string` | ✓ |

> 红→绿证据：slice 1 **genuine RED**（实测伪 2322，字面量 widen 成 string）；slice 2 **genuine RED**（临时 `if false` 禁用 ArrayLiteralExpression 臂复现 `Array<string>`≠`Array<"a">`）。任务命名两切片为 **green-on-arrival 守卫**（注解路径，本轮无改动）；负控 `[]`==`Array<never>` 钉死 Go 忠实（上下文不改空数组元素类型）。其余守卫/单测为 green-on-arrival——**如实记录非伪造红**（同 4bc–4bj 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**487 单测**（+12，相对 4bj 基线 475：check_test +7、contextual_test +5）+ **134 doctest**（**±0**：新方法/类型均 `pub(crate)`/私有、不挂 doctest）。gate：`cargo test -p tsgo_checker`（487+134）绿、clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿。**DEFER（带 blocked-by）**：空数组元素「重写」（Go 恒 `implicitNeverType`，无可做）/ union 上下文 + 对象成员判别（`discriminateContextualTypeByObjectMembers`）/ contextual tuple 位置类型化（`inTupleContext`）/ call·return·yield·await 位 / spread 元素 + offset / computed·dynamic 名 / 元素显式注解 + object-literal 方法 / `removeMissingType`·circular-mapped·generic-mapped / `isTypeAssertion` 分支 + `instantiateContextualType` / `isLiteralOfContextualType` 的 union·instantiable·bigint·unique-symbol·Index·template 臂 / `getWidenedUniqueESSymbolType` / `pushCachedContextualType`·`findContextualNode`·`with` 早退。

## 4bl 上下文类型流入 call/new 实参（回调参数从被调签名取上下文参数类型，`getContextualTypeForArgument`）行为单测（§8.6）

> 逐行为 red→green（见 impl.md「4bl 落地记录」slice 1/2 + 守卫 + 单测）。承接 4bj（上下文类型地基：变量初始化器位/赋值 RHS 位 → 箭头/函数表达式无注解参数）。本轮接通 **call/new 实参位**：`getContextualType` 新增 `KindCallExpression|KindNewExpression → getContextualTypeForArgument(parent, node)` 臂；据此实参为箭头/函数表达式时，其无注解参数从**被调签名第 i 个参数类型**取上下文类型（回调参数类型化），经既有**惰性**（`getTypeForVariableLikeDeclaration` 参数臂）+ **急性**（`contextuallyCheckFunctionExpressionOrObjectLiteralMethod`）双通道落地。**避免递归**：上下文查找解析被调签名时**只类型化 callee（绝不重查实参）**，取单一 call 签名 → 不会重入实参检查（Go 用 `signatureLinks.resolvedSignature == resolvingSignature` 哨兵守同一环）；重载（>1 签名，须实参类型消歧）/ 非可调用（0 签名）→ None（DEFER）。**Go 锚**：`checker.go:getContextualType(29040 KindCallExpression/KindNewExpression)`/`getContextualTypeForArgument(29438)`/`getContextualTypeForArgumentAtIndex(29448)`/`getEffectiveCallArguments(29718 default 臂)`/`getResolvedSignature`（resolvingSignature 守卫）/`relater.go:getTypeAtPosition(1754)`/`tryGetTypeAtPosition(1762)`。**公开 API 仅加法**：无新 `pub` 项、无既有 `pub fn` 签名变更、无 `lib.rs`/依赖/`.go` 改动；新增项全为私有方法（`get_contextual_type_for_argument`/`get_contextual_type_for_argument_at_index`/`get_resolved_signature_for_contextual_argument`）+ 1 私有自由 fn（`call_arguments`）；`get_contextual_type` 新增 `CallExpression|NewExpression` 臂（既有 `pub(crate)` 方法的加法行为扩展）。

| Rust 测试（文件 / 角色） | 验证行为 | 最小 input → expected | 完成 |
|---|---|---|---|
| `callback_argument_parameter_is_contextually_typed_by_call_signature`（`contextual_test.rs`，slice 1 tracer，genuine red→green）| 回调实参无注解参数被被调签名参数类型上下文化（**惰性**通道）（红：旧无 call 实参臂 → `x`=`any`，`TypeId(1)≠TypeId(8)`）| `function f(cb: (x: number) => void) {}\nf((x) => { x; });` → body `x` == `number` | ✓ |
| `callback_argument_parameter_type_flows_into_body_and_surfaces_mismatch`（`contextual_test.rs`，slice 2，genuine red→green）| 上下文参数类型真流入回调 body，误用 surfaces `2322`（**急性**通道）（红：临时中和 call 实参臂 → `0≠1` 诊断）| `function f(cb: (x: number) => void) {}\nf((x) => { const s: string = x; });` → 1×2322 `number`→`string` | ✓ |
| `callback_argument_parameter_type_flows_into_body_assignable_ok`（`contextual_test.rs`，slice 2 正控，green-on-arrival）| 正确赋值无诊断 | `... f((x) => { const n: number = x; });` → 0 诊断 | ✓ |
| `plain_call_without_callback_still_checks_clean`（`contextual_test.rs`，slice 3 守卫，无回归）| 无回调实参的普通调用照常无诊断 | `function g(n: number) {}\ng(1);` → 0 诊断 | ✓ |
| `call_with_non_function_argument_does_not_crash`（`contextual_test.rs`，slice 3 守卫，无 crash）| 回调参数位的非函数实参不 crash、不误报 | `function f(cb: (x: number) => void) {}\nf(1 as any);` → 0 诊断 | ✓ |
| `nested_callback_call_does_not_recurse_infinitely`（`contextual_test.rs`，slice 3 守卫，递归）| 回调体内再调同函数（带自己的回调）终止（不栈溢出）| `function f(cb: (x: number) => void) {}\nf((x) => { f((y) => { y; }); });` → 0 诊断 | ✓ |
| `unresolved_callee_with_callback_reports_2304_once`（`contextual_test.rs`，slice 3 守卫，无双报，genuine red→green）| 未解析 callee + 回调实参：callee 诊断只报 1 次（上下文查找重解析 callee 不得重报）（红：实测 `3≠1`——主路径 1 次 + 急性/惰性各 1 次）| `g((x) => { x; });` → 1×2304 `Cannot find name 'g'.` | ✓ |
| `get_contextual_type_of_call_argument_is_signature_parameter_type`（`contextual_test.rs`，单测）| call 实参臂 → 被调签名第 0 参类型（函数类型，1 call 签名，参 0==`number`、返回==`void`）| `function f(cb: (x: number) => void) {}\nf((x) => {});` → `get_contextual_type(arrow)` 见断言 | ✓ |
| `get_contextual_type_of_callee_is_none`（`contextual_test.rs`，单测，-1 守卫）| callee 与 call 同父但非实参 → None（`slices.Index==-1`）| `function f(...) {}\nf((x) => {});` → `get_contextual_type(f)` == `None` | ✓ |
| `get_contextual_type_of_overloaded_call_argument_is_none`（`contextual_test.rs`，单测，重载 DEFER）| 重载 callee（>1 签名）→ None（消歧需实参类型，DEFER）| `declare function f(a: number): void;\ndeclare function f(a: string): void;\nf((x) => {});` → `get_contextual_type(arrow)` == `None` | ✓ |
| `get_contextual_type_of_extra_call_argument_is_any`（`contextual_test.rs`，单测，越界 any）| 越过（非 rest）参数列的实参 → `any`（`getTypeAtPosition` fallback）| `function f() {}\nf((x) => {});` → `get_contextual_type(arrow)` == `Some(any)` | ✓ |
| `get_contextual_type_of_new_expression_argument_without_construct_signature_is_none`（`contextual_test.rs`，单测，NewExpression 臂 + construct 签名 DEFER）| `new` 实参无 call 签名（construct 签名 DEFER）→ None | `declare const C: any;\nnew C((x) => {});` → `get_contextual_type(arrow)` == `None` | ✓ |

> 红→绿证据：slice 1 **genuine RED**（初始无 call 实参臂：`x`=`TypeId(1)` any ≠ `TypeId(8)` number）；slice 2 **genuine RED**（临时 `if true { return None }` 中和 call 实参臂 → body 无 2322，`0≠1`）；slice 3 双报守卫 **genuine RED**（实测 `3≠1`——上下文查找重解析 callee 重报 2304，修复为重解析期间快照+回滚 callee 诊断）。slice 1/2 测**不同通道**（惰性/急性）互补。其余 slice 3 守卫（无回归 / 无 crash / 递归终止）+ 单测为 green-on-arrival——**如实记录非伪造红**（同 4bc–4bk 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**499 单测**（+12，相对 4bk 基线 487：contextual_test +12）+ **134 doctest**（**±0**：新增项全为私有方法/自由 fn，不挂 doctest）。gate：`cargo test -p tsgo_checker`（499+134）绿、clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿。**DEFER（带 blocked-by）**：重载-目标上下文实参（`getResolvedSignature` 重载消歧 + 推断期 candidate 参数类型 union）/ `import(...)` 实参类型（`string`/`ImportCallOptions`/`any`）/ JSX 首实参签名（`getEffectiveFirstArgumentForJsxSignature`）/ rest 参数 indexed-access（`getIndexedAccessTypeEx`）/ spread·tagged-template·decorator 的 `getEffectiveCallArguments` 合成实参 / construct 签名（`new` 的 `__new`）/ 泛型签名从上下文返回类型推断 / return·yield·await 位 / `getApparentTypeOfContextualType` 的 instantiate·mapType·union 判别 / `findContextualNode` 缓存·`with` 早退。

## 4bm 类 heritage 检查（implements 满足 2420 + extends 兼容 2415）行为单测（§8.6）

`check_class_like_declaration`（`check.rs`，在 `check_statement` 的 `ClassDeclaration|ClassExpression` 臂迭代成员前调用）。Go: `internal/checker/checker.go:Checker.checkClassLikeDeclaration(4266)`（implements 臂 4338 / extends 臂 4287 / `issueMemberSpecificError` 广播头消息 4489 / `getTypeWithThisArgument` 单态原样 19428）。

| Rust 测试（文件 / 角色） | 验证行为 | 最小 input → expected | 完成 |
|---|---|---|---|
| `class_incorrectly_implements_interface_reports_diagnostic`（`check_test.rs`，slice 1 tracer，genuine red→green）| implements 子句被实现接口未满足（缺必需成员）→ 2420（红：旧无 heritage 检查 `0≠1`）| `interface I { x: number } class C implements I {}` → 1×2420 "Class 'C' incorrectly implements interface 'I'." | ✓ |
| `class_correctly_implements_interface_reports_no_diagnostic`（`check_test.rs`，slice 1 正控，green-on-arrival）| implements 满足 → 零诊断（守卫）| `interface I { x: number } class C implements I { x: number = 1 }` → `[]` | ✓ |
| `class_incorrectly_extends_base_class_reports_diagnostic`（`check_test.rs`，slice 2，genuine red→green）| extends 基类不兼容覆盖（`x: string` 覆盖 `x: number`）→ 2415（红：临时 `if false && …` 中和 extends 臂 `0≠1`）| `class B { x: number = 0 } class D extends B { x: string = "s" }` → 1×2415 "Class 'D' incorrectly extends base class 'B'." | ✓ |
| `class_correctly_extends_base_class_with_compatible_override_reports_no_diagnostic`（`check_test.rs`，slice 2 正控，green-on-arrival）| 兼容覆盖 → 零诊断 | `class B { x: number = 0 } class D extends B { x: number = 1 }` → `[]` | ✓ |
| `class_extends_base_class_without_override_reports_no_diagnostic`（`check_test.rs`，slice 2 正控，green-on-arrival）| 无覆盖（继承基类成员）→ 零诊断 | `class B { x: number = 0 } class D extends B {}` → `[]` | ✓ |
| `plain_class_without_heritage_reports_no_heritage_diagnostic`（`check_test.rs`，slice 3 守卫，无回归）| 无 heritage 子句不触发任何 heritage 检查 | `class C { x: number = 1 }` → `[]` | ✓ |
| `class_implements_unresolved_interface_reports_no_heritage_diagnostic`（`check_test.rs`，slice 3 守卫，未解析 skip）| implements 目标未解析（错误类型）→ 跳过满足检查（`!isErrorType`）；unresolved-name 诊断 DEFER | `class C implements Missing {}` → `[]` | ✓ |
| `class_implements_multiple_interfaces_reports_for_each_unsatisfied`（`check_test.rs`，slice 3 守卫，loop）| implements loop 逐接口判否：`I` 满足、`J` 不满足 → 仅 1×2420 命名 `J` | `interface I { x: number } interface J { y: string } class C implements I, J { x: number = 1 }` → 1×2420 "Class 'C' incorrectly implements interface 'J'." | ✓ |
| `class_extends_and_implements_both_relations_checked`（`check_test.rs`，slice 3 守卫，双臂）| extends 与 implements 双臂独立运行，extends 先于 implements | `class B { x: number = 0 } interface I { y: string } class D extends B implements I { x: string = "s" }` → 2 诊断（`[0]`=2415、`[1]`=2420）| ✓ |

> 红→绿证据：slice 1 **genuine RED**（初始无 heritage 检查：`0≠1`）；slice 2 **genuine RED**（临时 `if false && !is_type_assignable_to(...)` 中和 extends 臂 → `0≠1`，恢复变绿）。正控/守卫为 green-on-arrival——**如实记录非伪造红**（同 4bc–4bl 口径）。每片单独 `cargo test -p tsgo_checker <name>` 看红/绿。**测试增量**：**508 单测**（+9，相对 4bl 基线 499：check_test +9）+ **134 doctest**（**±0**：新增项全为私有方法，不挂 doctest）。gate：`cargo test -p tsgo_checker`（508+134）绿、clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿。**DEFER（带 blocked-by）**：成员特定阐释 2416（`issueMemberSpecificError` 成员遍历 + 产诊断关系引擎 chain，含 2741/2322）/ override-modifier 走查（`checkKindsOfPropertyMemberOverrides`：2416/2603/2610）/ 静态侧 extends 2417 + base 构造可达性 2654 / implements 非对象 2422 + implements-a-class 2720（`isValidBaseType`/`getReducedType`）/ 泛型基类带类型实参（`getTypeWithThisArgument` 类型实参替换 + `this` 代入）/ mixin·type-variable base constructor·抽象类实例化 2511·`super()` / class *expression* 经 `check_expression` 的 heritage 检查 / index 约束·属性初始化（`checkIndexConstraints`/`checkPropertyInitialization`）。

## 4bn 关系引擎诊断阐释链（reporting 路径：嵌套 2326/2200/2741/2739 + 叶 2322）行为单测（§8.6）

`build_relation_error_chain` + `report_type_not_assignable`（`relations.rs`/`check.rs`，在 var-decl/assignment/property-decl 失败位调用）。Go: `internal/checker/relater.go:Checker.checkTypeRelatedToEx(359)` / `Relater.reportError(4803)`（dotted 折叠）/ `reportRelationError(4725)`（头消息 + 2741/2739 头抑制）/ `propertiesRelatedTo(4081)` / `propertyRelatedTo(4244)` / `getUnmatchedPropertiesWorker(981)` / `reportUnmatchedProperty(4319)` / `createDiagnosticChainFromErrorChain(402)`。**所有 expected 用 `go build ./cmd/tsgo` 实跑快照核对**（非推断）。

| Rust 测试（文件 / 角色） | 验证行为 | 最小 input → expected | 完成 |
|---|---|---|---|
| `assignability_chain_single_level_property_mismatch`（`check_test.rs`，slice 1 tracer，genuine red→green）| 单层属性不兼容 → 2322 头 + 2326 子 + 叶 2322（红：实测 `message_chain.len()==0`）| `interface A{a:number} interface B{a:string}; declare const b:B; var x:A=b;` → 2322 "Type 'B' is not assignable to type 'A'." → 2326 "Types of property 'a' are incompatible." → 2322 "Type 'string' is not assignable to type 'number'." | ✓ |
| `assignability_chain_nested_property_collapses_to_dotted_message`（`check_test.rs`，slice 2，genuine red→green）| 嵌套对象属性 → dotted 2200（红：临时 `if false &&` 禁折叠 → 子码 2326≠2200）| `declare const src:{a:{b:string}}; const o:{a:{b:number}}=src;` → 2322 头 → 2200 "The types of 'a.b' are incompatible between these types." → 2322 叶 | ✓ |
| `assignability_missing_required_property_reports_2741_head`（`check_test.rs`，slice 3，genuine red→green）| 单缺失必需属性 → 单条 2741（2322 头抑制）（红：临时 `Some(2741) if false` 禁抑制 → 头码 2322≠2741）| `interface S{a:number} interface T{a:number;b:number}; declare const s:S; var t:T=s;` → 单条 2741 "Property 'b' is missing in type 'S' but required in type 'T'."（无子链）| ✓ |
| `assignability_chain_nested_missing_property_keeps_2326_over_2741`（`check_test.rs`，slice 3b，green-on-arrival）| 嵌套缺失 → 2326 'a' 上挂 2741（内层 2322 抑制，2326 不折叠）| `declare const src:{a:{b:number}}; const o:{a:{b:number;c:number}}=src;` → 2322 头 → 2326 'a' → 2741 'c' | ✓ |
| `assignability_multiple_missing_properties_report_2739_head`（`check_test.rs`，slice 3c，green-on-arrival）| 多缺失 → 单条 2739（2322 头抑制）| `declare const src:{}; const o:{a:number;b:number}=src;` → 单条 2739 "Type '{}' is missing the following properties from type '{ a: number; b: number; }': a, b" | ✓ |
| `assignability_flat_primitive_mismatch_has_no_chain`（`check_test.rs`，slice 4 守卫，无回归）| 扁平原语不匹配 → 单条 2322 无链 | `const n:number="x";` → 2322 "Type 'string' is not assignable to type 'number'."（`message_chain` 空）| ✓ |
| `assignability_chain_optional_source_required_target_reports_2327`（`check_test.rs`，附加，green-on-arrival）| optional 源 vs required 目标（非严格，类型匹配）→ 2327 子 | `interface S{a?:string} interface T{a:string}; declare const s:S; var t:T=s;` → 2322 头 → 2327 "Property 'a' is optional in type 'S' but required in type 'T'." | ✓ |
| `get_property_name_arg_brackets_quoted_names_only`（`relations_test.rs`，单测）| `getPropertyNameArg` 引号名加方括号、标识符名不变 | `"a"→"a"`、`"\"a b\""→"[\"a b\"]"`、`"'k'"→"['k']"` | ✓ |
| `add_to_dotted_name_joins_segments`（`relations_test.rs`，单测）| `addToDottedName` 点连接/索引附加/`new` 括号 | `("a","b")→"a.b"`、`("a","[0]")→"a[0]"`、`("new C","x")→"(new C).x"`、`("a","(b)")→"(a.b)"` | ✓ |
| `is_conversion_or_interface_implementation_message_matches_codes`（`relations_test.rs`，单测）| 2420 命中、2322/2741 不命中 | 见名 | ✓ |
| `chain_reporter_get_chain_message_and_args_match`（`relations_test.rs`，单测）| `getChainMessage`/`chainArgsMatch`（None 通配）| 见名 | ✓ |
| `chain_reporter_report_prepends_without_collapse`（`relations_test.rs`，单测）| `reportError` 单层 prepend（不折叠）| 2322 叶 + 2326 'a' → 头 2326 + 子 2322 | ✓ |
| `chain_reporter_collapses_nested_property_messages`（`relations_test.rs`，单测）| `reportError` dotted 折叠（直接驱动）| leaf 2322 / 2326 'b' / 2322 mid / 2326 'a' → 头 2200 "a.b" + 子 2322 | ✓ |
| `get_unmatched_property_finds_first_missing_required`（`relations_test.rs`，单测）| 首个缺失必需属性；反向无缺失 | `S{a}`/`T{a;b}` → 'b'；`T`→`S` → None | ✓ |
| `get_unmatched_property_skips_optional_target_unless_required`（`relations_test.rs`，单测）| optional 目标属性不计缺失（除非 requireOptional）| `S{a}`/`T{a;b?}` → assignable None / subtype Some | ✓ |
| `should_report_unmatched_property_error_is_true_for_object_with_properties`（`relations_test.rs`，单测）| 有属性 / 空对象（无 call 签名）均 true | `A{a}`/`E{}` → true/true | ✓ |
| `build_relation_error_chain_single_property_mismatch`（`relations_test.rs`，单测）| 公开入口产链（直接驱动）| `B`→`A`（x mismatch）→ 头 2322 + 2326 + 叶 2322 | ✓ |
| `build_relation_error_chain_returns_none_when_assignable`（`relations_test.rs`，单测）| 可赋值时返回 None | `C{a;b}`→`A{a}` → None | ✓ |

> 红→绿证据：slice 1 **genuine RED**（实测 `message_chain.len()==0`，头码/头消息已对但**无链**）；slice 2 **genuine RED**（临时 `if false &&` 禁折叠分支 → 子码 2326≠2200）；slice 3 **genuine RED**（临时 `Some(2741) if false` 禁头抑制 → 头码 2322≠2741）。两处 RED 实测后已恢复并复绿。其余 slice/单测 green-on-arrival（reporting 机器为一体）——**如实记录非伪造红**（同 4bc–4bm 口径）。**测试增量**：**526 单测**（+18，相对 4bm 基线 508：check_test +7 / relations_test +11）+ **135 doctest**（+1：新 `DiagnosticMessageChain` doctest）。gate：`cargo test -p tsgo_checker`（526 lib + 135 doc）绿、clippy `-D warnings` 干净、fmt 干净、`cargo build -p tsgo_compiler` 绿（**未触 `internal/ast`，故未跑 `--workspace`**）。**与 Go 实测一致的偏离记录**：任务原文 slice 1 写嵌套 2326→2326、slice 2 写 2322→2741，但 Go 实跑分别折叠为 dotted 2200、抑制头产单条 2741——已按 Go 真值匹配（遵 "verify against Go, do NOT invent"）。**DEFER（带 blocked-by）**：`elaborateError`/object·array-literal element-wise（字面量源走链路径是已知偏离）/ union·intersection 阐释 / signature·parameter 2328 + 返回类型折叠 / index-signature 2329·2330 / tuple·array element 链 / excess-prop "did you mean" 联动 / `'{0}' is declared here` related-info / type-parameter 约束·"Did you mean 2"·exactOptional·全限定名消歧 / `maybeKeys` 完整递归上限 / `headMessage` 透传 + heritage 成员特定 2416 / 其它 2322 站点（return·in·call 实参）增量接线。详见 impl.md「4bn 落地记录」。

## 测试基建依赖（说明）

- `TestGetSymbolAtLocation` 与 `BenchmarkNewChecker` 需要一个可运行的 `Program`（来自 `compiler`/`tsoptions`/`bundled`，分布在 P6/P9）。在 P4 阶段，要么用**最小桩 program**（只实现 checker 需要的 `Program` trait 子集 + 手喂 bound source file）提前跑通 `get_symbol_at_location`，要么把这两个测试的"真 program 版本"推迟到 P6。推荐：4b 用桩 program 跑 `get_symbol_at_location` 的核心路径，P6 再加真 program 集成测试。
- `TestTracerPushPreservesEndArgMutations` 只依赖 `tracing`/`vfstest`/`json`（P1），无需 program → 4a 即可收口。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（2 个）都已映射（`TestGetSymbolAtLocation`→**4b 桩 program 已绿**，真 program 版 →P6；`TestTracerPushPreservesEndArgMutations`→后续 checker 轮次，4a 落地其可移植不变量子集）
- [x] 表驱动子用例 —— N/A（两测试均单块）
- [x] expected 值均取自 Go 测试字面量（`Foo`/`foo`/`bar` 符号名；`checkerId==Int(7)`；完整 `checkerId==7.0`/`variances==["out"]` 待 round-trip 收口）
- [x] 每条带 `// Go:` 锚点（4a/4b 的 `*_test.rs` 均带 `// Go:`）
- [x] 与 impl.md 双向对齐：`get_symbol_at_location`/`resolve_name`/`get_symbol_of_declaration`(4b) 承载 `TestGetSymbolAtLocation` 端口；`Tracer` 完整 push(后续) 承载 tracer 测试；其余子系统正确性以"P10 conformance 目录"登记

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `BenchmarkNewChecker` | 需真 program + TS 子模块源；仅性能 | P10 |
| `TestGetSymbolAtLocation` 真 program 版 | 需 `compiler`/`tsoptions`/`bundled`（4b 已用桩 program 跑通核心路径） | P6 |
| 属性访问完整类型化解析（`checkPropertyAccessExpression`） | 4b 仅结构化解析 `const x: TypeRef`；需 apparent type/表达式检查 | 4d/4g |
| 全部类型检查正确性（conformance/fourslash/`.d.ts`） | checker 测试的本体 | P10（按上表分子阶段登记） |
