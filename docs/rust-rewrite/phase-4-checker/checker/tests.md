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
| 4e 推断 | `types/typeInference/`、`generics/`(调用推断) | — (P10) |
| 4f 控制流 | `controlFlow/`、`narrowing/` | — (P10) |
| 4g 表达式/语句 | `expressions/`、`statements/`、`functions/` | — (P10) |
| 4h JSX | `jsx/` | — (P10) |
| 4i 语法检查 | `grammar*` / 各处 grammar 错误 baseline | — (P10) |
| 4j node builder/序列化 | `declarationEmit/`（`.d.ts` baseline）、quickinfo fourslash | — (P10) |
| 4k emit resolver | declaration transformer baseline（经 P5） | — (P10) |

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
