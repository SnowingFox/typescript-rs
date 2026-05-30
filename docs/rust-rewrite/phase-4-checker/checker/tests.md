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
| `check_element_access_string_index` | 字符串字面量索引 | `foo["bar"]` → `string` | ✓ |
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
