# transformers: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 文件 / **2 `func Test`** / 约 90 子用例（`TestTypeEraser` ~70 + `TestImportElision` ~20，均在 `tstransforms`，属 6b）。

> **6a 现状**：根包 Go 侧**无 `*_test.go`**，故按 PORTING §8.6「每函数一测」自写行为级单测。6a 落地 **13 个 `#[test]` + 9 个 doctest 全绿**（见末节「0 直接单测的子包」行为表，已逐条 ✓）。`TestTypeEraser`/`TestImportElision` 全量属 6b（tstransforms）。

> 字符串约定：`\n` = 换行，`\"` = 转义双引号。input/output 取自 Go 测试字面量。空输出 `""` 表示该 stage 把整条语句擦除。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/transformers/tstransforms/typeeraser_test.go` | `tstransforms/typeeraser.rs`（external `tstransforms_test`） | 1 |
| `internal/transformers/tstransforms/importelision_test.go` | `tstransforms/importelision.rs`（external `tstransforms_test`，需 fake checker.Program） | 1 |

> 其余 5 子包（estransforms/moduletransforms/jsxtransforms/inliners/declarations）与根包 Go 侧**无 `*_test.go`**（见末节）。

---

## `typeeraser_test.go` — `TestTypeEraser`（表驱动，~70 子用例）

驱动：`ParseTypeScript(input, jsx)` → `NewTypeEraserTransformer({CompilerOptions, Context}).TransformSourceFile` → `CheckEmit(output)`。`vms:true` 表示 `VerbatimModuleSyntax=true`。

> **6b 进度**：剥类型纯重建簇 **9 子用例** ✅（`VariableDeclaration2`、`CallExpression`、`NewExpression1/2`、`ExpressionWithTypeArguments`、`FunctionDeclaration3`、`FunctionExpression`、`ClassDeclaration2`、`ClassExpression`）。
>
> **6c-prep 进度**：移除式 visitor + 省略节点种类落地后，再补省略子用例（逐条红→绿）：`InterfaceDeclaration`/`TypeAliasDeclaration` ✅、`VariableDeclaration1`/`ClassDeclaration1`/`FunctionDeclaration1`（ambient `declare`）✅、`NamespaceExportDeclaration` ✅、`Modifiers`/`PropertyDeclaration2`/`PropertyDeclaration3`（类型只读修饰符）✅、`PropertyDeclaration1`（`declare` 字段移除）✅、`HeritageClause`（`implements`）✅、`ParameterDeclaration`（`this` 参 + 参数类型/`?`）✅、`NonNull`/`TypeAssertion`/`As`/`Satisfies`（断言→`PartiallyEmittedExpression`）✅、`FunctionDeclaration2`（重载）✅、`ImportEqualsDeclaration#2`/`ImportDeclaration#6`（`import type`）✅。
>
> 其余子用例 **DEFER(P5)**：(a) 每-specifier `import { type x }` / `export { type x }`（需命名导入/导出重建 + `verbatimModuleSyntax`）；(b) 命名空间实例化分析 `IsInstantiatedModule`（`namespace N {}`/`namespace N { export interface ... }`）；(c) method/ctor/accessor 重载与抽象访问器省略；(d) `compilerOptions`（`vms`/`ExperimentalDecorators`/`ShouldPreserveConstEnums`）分支；(e) 用法式 import elision（checker `EmitResolver`，见 importelision）。

#### 修饰符 / 声明擦除
- `Modifiers`: `class C { public x; private y }` → `class C {\n    x;\n    y;\n}`
- `InterfaceDeclaration`: `interface I { }` → ``（删）
- `TypeAliasDeclaration`: `type T = U;` → ``
- `NamespaceExportDeclaration`: `export as namespace N;` → ``
- `UninstantiatedNamespace1`: `namespace N {}` → ``
- `UninstantiatedNamespace2`: `namespace N { export interface I {} }` → ``
- `UninstantiatedNamespace3`: `namespace N { export type T = U; }` → ``
- `ExpressionWithTypeArguments`: `F<T>` → `F;`

#### 类成员
- `PropertyDeclaration1`: `class C { declare x; }` → `class C {\n}`
- `PropertyDeclaration2`: `class C { public x: number; }` → `class C {\n    x;\n}`
- `PropertyDeclaration3`: `class C { public static x: number; }` → `class C {\n    static x;\n}`
- `ConstructorDeclaration1`: `class C { constructor(); }` → `class C {\n}`
- `ConstructorDeclaration2`: `class C { public constructor() {} }` → `class C {\n    constructor() { }\n}`
- `MethodDeclaration1`: `class C { m(); }` → `class C {\n}`
- `MethodDeclaration2`: `class C { public m<T>(): U {} }` → `class C {\n    m() { }\n}`
- `MethodDeclaration3`: `class C { public static m<T>(): U {} }` → `class C {\n    static m() { }\n}`
- `GetAccessorDeclaration1`: `class C { get m(); }` → `class C {\n    get m() { }\n}`
- `GetAccessorDeclaration2`: `class C { public get m<T>(): U {} }` → `class C {\n    get m() { }\n}`
- `GetAccessorDeclaration3`: `class C { public static get m<T>(): U {} }` → `class C {\n    static get m() { }\n}`
- `SetAccessorDeclaration1`: `class C { set m(v); }` → `class C {\n    set m(v) { }\n}`
- `SetAccessorDeclaration2`: `class C { public set m<T>(v): U {} }` → `class C {\n    set m(v) { }\n}`
- `SetAccessorDeclaration3`: `class C { public static set m<T>(v): U {} }` → `class C {\n    static set m(v) { }\n}`
- `IndexSignature`: `class C { [key: string]: number; }` → `class C {\n}`

#### 变量 / 继承 / 类 / 函数 / 箭头 / 参数
- `VariableDeclaration1`: `declare var a;` → ``
- `VariableDeclaration2`: `var a: number` → `var a;`
- `HeritageClause`: `class C implements I {}` → `class C {\n}`
- `ClassDeclaration1`: `declare class C {}` → ``
- `ClassDeclaration2`: `class C<T> {}` → `class C {\n}`
- `ClassExpression`: `(class C<T> {})` → `(class C {\n});`
- `FunctionDeclaration1`: `declare function f() {}` → ``
- `FunctionDeclaration2`: `function f();` → ``
- `FunctionDeclaration3`: `function f<T>(): U {}` → `function f() { }`
- `FunctionExpression`: `(function f<T>(): U {})` → `(function f() { });`
- `ArrowFunction`: `(<T>(): U => {})` → `(() => { });`
- `ParameterDeclaration`: `function f(this: x, a: number, b?: boolean) {}` → `function f(a, b) { }`

#### 调用 / new / 模板 / 断言
- `CallExpression`: `f<T>()` → `f();`
- `NewExpression1`: `new f<T>()` → `new f();`
- `NewExpression2`: `new f<T>` → `new f;`
- `TaggedTemplateExpression`: `` f<T>`` `` → `` f ``; ``
- `NonNullExpression`: `x!` → `x;`
- `TypeAssertionExpression#1`: `<T>x` → `x;`
- `TypeAssertionExpression#2`: `(<T>x).c` → `x.c;`
- `AsExpression#1`: `x as T` → `x;`
- `AsExpression#2`: `(x as T).c` → `x.c;`
- `SatisfiesExpression#1`: `x satisfies T` → `x;`
- `SatisfiesExpression#2`: `(x satisfies T).c` → `x.c;`

#### JSX 类型参数擦除（jsx:true）
- `JsxSelfClosingElement`: `<x<T> />` → `<x />;`
- `JsxOpeningElement`: `<x<T>></x>` → `<x></x>;`

#### import/export 仅类型擦除
- `ImportEqualsDeclaration#1`: `import x = require("m");` → `import x = require("m");`
- `ImportEqualsDeclaration#2`: `import type x = require("m");` → ``
- `ImportEqualsDeclaration#3`: `import x = y;` → `import x = y;`
- `ImportEqualsDeclaration#4`: `import type x = y;` → ``
- `ImportDeclaration#1`: `import "m";` → `import "m";`
- `ImportDeclaration#2`: `import * as x from "m"; x;` → `import * as x from "m";\nx;`
- `ImportDeclaration#3`: `import x from "m"; x;` → `import x from "m";\nx;`
- `ImportDeclaration#4`: `import { x } from "m"; x;` → `import { x } from "m";\nx;`
- `ImportDeclaration#5`: `import type * as x from "m";` → ``
- `ImportDeclaration#6`: `import type x from "m";` → ``
- `ImportDeclaration#7`: `import type { x } from "m";` → ``
- `ImportDeclaration#8`: `import { type x } from "m";` → ``
- `ImportDeclaration#9`(vms): `import { type x } from "m";` → `import {} from "m";`（VerbatimModuleSyntax）
- `ExportDeclaration#1`: `export * from "m";` → `export * from "m";`
- `ExportDeclaration#2`: `export * as x from "m";` → `export * as x from "m";`
- `ExportDeclaration#3`: `export { x } from "m";` → `export { x } from "m";`
- `ExportDeclaration#4`: `export type * from "m";` → ``
- `ExportDeclaration#5`: `export type * as x from "m";` → ``
- `ExportDeclaration#6`: `export type { x } from "m";` → ``
- `ExportDeclaration#7`: `export { type x } from "m";` → ``
- `ExportDeclaration#7`(vms): `export { type x } from "m";` → `export {} from "m";`（VerbatimModuleSyntax）

| 收口检查 | Go 对照 | 完成 |
|---|---|---|
| 以上每个 `t.Run(title)` 子用例都有对应 Rust 用例（~70 条） | `typeeraser_test.go:TestTypeEraser` | |

---

## `importelision_test.go` — `TestImportElision`（表驱动，~20 子用例）

驱动：解析 `input`（+ 可选 `other`）→ 建 `fakeProgram` → `checker.NewChecker` → `emitResolver.MarkLinkedReferencesRecursively(file)` → `NewTypeEraserTransformer` 再 `NewImportElisionTransformer` → `CheckEmit(output)`。模块格式固定 `ESNext`。

- `ImportEquals#1`: `import x = require("other"); x;` → `import x = require("other");\nx;`
- `ImportEquals#2`: `import x = require("other");` → ``（未引用 → 删）
- `ImportDeclaration#1`: `import "m";` → `import "m";`（副作用 import 保留）
- `ImportDeclaration#2`: `import * as x from "other"; x;` → `import * as x from "other";\nx;`
- `ImportDeclaration#3`: `import x from "other"; x;` → `import x from "other";\nx;`
- `ImportDeclaration#4`: `import { x } from "other"; x;` → `import { x } from "other";\nx;`
- `ImportDeclaration#5`: `import * as x from "other";` → ``（未引用）
- `ImportDeclaration#6`: `import x from "other";` → ``（未引用）
- `ImportDeclaration#7`: `import { x } from "other";` → ``（未引用）
- `ExportDeclaration#1`: `export * from "other";`（other=`export let x;`）→ `export * from "other";`
- `ExportDeclaration#2`: `export * as x from "other";`（other=`export let x;`）→ `export * as x from "other";`
- `ExportDeclaration#3`: `export * from "other";`（other=`export let x;`）→ `export * from "other";`
- `ExportDeclaration#4`: `export * as x from "other";`（other=`export let x;`）→ `export * as x from "other";`
- `ExportDeclaration#5`: `export { x } from "other";`（other=`export let x;`，值导出）→ `export { x } from "other";`
- `ExportDeclaration#6`: `export { x } from "other";`（other=`export type x = any;`，仅类型）→ ``
- `ExportDeclaration#7`: `export { x }; let x;` → `export { x };\nlet x;`
- `ExportDeclaration#8`: `export { x }; type x = any;` → ``（仅类型本地绑定）
- `ExportDeclaration#9`: `import { x } from "other"; export { x };`（other=`export type x = any;`）→ ``
- `ExportAssignment#1`: `let x; export default x;` → `let x;\nexport default x;`
- `ExportAssignment#2`: `type x = any; export default x;` → ``（默认导出的是类型）

| 收口检查 | Go 对照 | 完成 |
|---|---|---|
| 以上每个 `t.Run(title)` 子用例都有对应 Rust 用例（~20 条），含值/仅类型分支 | `importelision_test.go:TestImportElision` | |

> **依赖提示**：本测试需要可用的 `checker.NewChecker` + `EmitResolver.MarkLinkedReferencesRecursively`（P4）。在 checker 未就绪前整体 `—`（DEFER P4 收口后回填），其余 transformers 工作不受阻。

---

## 0 直接单测的子包（补行为级 Rust 测试）

根包、`estransforms`、`moduletransforms`、`jsxtransforms`、`inliners`、`declarations` Go 侧**无 `*_test.go`**；行为由 **P10 conformance parity** 兜底（`tsc --target/--module/--jsx/--experimentalDecorators/--emitDecoratorMetadata/--isolatedDeclarations` baseline 对拍）。本轮补关键路径行为级测试：

| Rust 测试 | 子包 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|---|
| `transformer::identity_transform_round_trips_source_file` | 根 | **6a tracer bullet**：恒等 transform 过解析后的 SourceFile | `const x = 1;\nf(x);` → 重 emit 同文本 | `Transformer.TransformSourceFile` | ✓ |
| `transformer::rewriting_transform_rebuilds_tree` | 根 | 非恒等 transform 经 `visit_each_child` 重建树 | `a;\nb;` →（`a`→`x`）→ `x;\nb;` | 驱动 + factory 接线 | ✓ |
| `chain::chain_runs_stages_left_to_right` | 根 | Chain 多 stage 左→右串联（共享 arena） | `a;` →（`a`→`b`,`b`→`c`）→ `c;` | `Chain` 语义 | ✓ |
| `chain::chain_single_element_passthrough` | 根 | 单元素 Chain 直接返回该 stage | `a;` →（`a`→`b`）→ `b;` | `Chain` len<2 分支 | ✓ |
| `chain::chain_skips_none_stages` | 根 | 跳过返回 `None` 的 stage | `a;` →（None,`a`→`b`）→ `b;` | `Chain` 过滤 nil | ✓ |
| `chain::chain_all_none_yields_none` | 根 | 全 `None` → 组合工厂返回 `None` | — | `Chain` 0-survivor 分支 | ✓ |
| `chain::chain_empty_panics` | 根 | 空 Chain panic | `chain([])` → panic | `Chain` len==0 | ✓ |
| `modifiervisitor::extract_modifiers_none_returns_none` | 根 | nil 修饰符 → `None` | `None` → `None` | `ExtractModifiers` | ✓ |
| `modifiervisitor::extract_modifiers_filters_disallowed` | 根 | 仅保留 allowed 修饰符 | `export abstract class C{}` + allowed=EXPORT → 仅 `export`（flags=EXPORT，保原 loc） | `ExtractModifiers` | ✓ |
| `modifiervisitor::extract_modifiers_keeps_all_when_allowed` | 根 | allowed 含全部 → 不删 | allowed=EXPORT\|ABSTRACT → 2 个修饰符全留 | `ExtractModifiers` 未变路径 | ✓ |
| `utilities::non_assignment_operator_maps_compound_tokens` | 根 | 复合赋值算符 → 非赋值算符 | `+=`→`+`…`??=`→`??`；非复合原样返回 | `GetNonAssignmentOperatorForCompoundAssignment` | ✓ |
| `utilities::generated_identifier_detects_auto_names` | 根 | factory 生成名为 generated；普通标识符不是 | `new_temp_variable()`→true；`a`→false | `IsGeneratedIdentifier` | ✓ |
| `utilities::emit_flag_name_predicates` | 根 | helper/local/export-name 谓词各认自己的 emit flag | 各 flag 置位后对应谓词 true、其余 false | `IsHelperName/IsLocalName/IsExportName` | ✓ |
| `tstransforms::typeeraser::tests::*`（18） | tstransforms | 剥类型纯重建（6b，9）+ 省略（6c-prep，9）经驱动重 emit | 见上「TestTypeEraser 6b/6c-prep 进度」 | `TypeEraserTransformer.visit` | ✓ |
| `ast::visitor::tests::visit_nodes_removable_drops_none_results` | ast（additive） | 列表访问丢弃 `None` 结果 | `[a,b,c]` 丢 `b` → `[a,c]` | `NodeVisitor.VisitNodes` 删 nil | ✓ |
| `transformer::tests::transform_can_drop_a_statement` | 根 | 驱动 + 移除式访问端到端丢语句 | `a;\nb;` 丢 `a;` → `b;` | 移除式 visitor tracer | ✓ |
| `printer::emit_statements::tests::not_emitted_statement_emits_nothing` | printer（additive） | `NotEmittedStatement` 无输出 | 合成 `[NotEmitted]` → `` | `emitNotEmittedStatement` | ✓ |
| `printer::emit_expressions::tests::partially_emitted_expression_emits_inner` | printer（additive） | `PartiallyEmittedExpression` 仅 emit 内层 | 合成 `PEE(x);` → `x;` | `emitPartiallyEmittedExpression` | ✓ |
| `tstransforms::utilities::tests::constant_expression_builds_literals` | tstransforms | string/number/NaN/+Inf → 工厂节点 | `"hello"`/`42`/`NaN`/`Infinity` → 字面量/标识符 | `constantExpression` | ✓ |
| `tstransforms::utilities::tests::constant_expression_negates_with_prefix_unary` | tstransforms | 负数/-Inf → `-` 前缀一元 | `-3`→`-(3)`；`-Infinity`→`-Infinity` | `constantExpression` | ✓ |
| `printer::factory::new_identifier_is_synthesized` | printer（additive） | 工厂建标识符置 SYNTHESIZED | `new_identifier("Infinity")` | `NodeFactory.NewIdentifier` | ✓ |
| `printer::factory::new_literals_and_prefix_unary_are_synthesized` | printer（additive） | 工厂建字面量/前缀一元置 SYNTHESIZED | string/numeric/prefix-unary | `NodeFactory.New*` | ✓ |
| `flatten_destructuring_array` | 根 | 数组解构赋值展开 | `[a, b] = c` → 顺序赋值 | `FlattenDestructuringAssignment` | —（6g） |
| `es2016_exponentiation` | estransforms | `a ** b` 降级（target ES2015） | `a ** b` → `Math.pow(a, b)` | `newExponentiationTransformer` | |
| `es2020_optional_chain` | estransforms | `a?.b` 降级（target ES2019） | `a?.b` → 三元/临时变量展开 | `newOptionalChainTransformer` | |
| `es2020_nullish_coalescing` | estransforms | `a ?? b` 降级 | → `(a !== null && a !== void 0) ? a : b` | `newNullishCoalescingTransformer` | |
| `get_es_transformer_target_dispatch` | estransforms | 按 target 选链 | ESNext/ES2016/older → 正确链 | `GetESTransformer` | |
| `commonjs_named_import` | moduletransforms | ESM→CJS 命名导入 | `import {x} from "m"` → `const m_1 = require("m")` | `NewCommonJSModuleTransformer` | |
| `jsx_classic_runtime` | jsxtransforms | `<a/>` → createElement | `<a/>` → `React.createElement("a", null)` | `NewJSXTransformer` | |
| `const_enum_inline` | inliners | const enum 成员内联 | `const enum E{A=1} E.A` → `1 /* E.A */` | `NewConstEnumInliningTransformer` | |
| `declaration_simple` | declarations | 基础 `.d.ts` 形状 | `export const x = 1;` → `export declare const x: number;` | `NewDeclarationTransformer` | |

> 上述行为级 expected 取自 TS/Go 已知降级形态；精确字节由 P10 兜底。const enum/declaration 等依赖 checker（P4），未就绪则该行 `—`。

## tstransforms conformance 切片（P10 端到端兜底）

`tstransforms` 各 stage 的字节级正确性由 **P10 conformance parity** 对拍（`tsc` baseline）。6b 的单测覆盖 `typeeraser` 剥类型子集；下列 `tests/cases/conformance/` 子集是 6b–6c 完整化后 P10 必须绿的目标（本轮仅登记，不在 6b 跑）：

| transform | conformance 子集 | 验证内容 | 目标轮 |
|---|---|---|---|
| typeeraser | `tests/cases/conformance/types/**`、`.../typeParameters/**`、`.../expressions/asOperator/**`、`satisfies/**` | 类型注解/类型参数/类型实参/断言擦除后 `--target` JS 输出 | 6b 完整化 + P10 |
| runtimesyntax（enum） | `tests/cases/conformance/enums/**`（`enumBasics`、`constEnums`、`enumMerging`…） | enum → IIFE 运行时对象（`var E; (function(E){...})(E||(E={}))`） | 6b-后续 + P10 |
| runtimesyntax（namespace） | `tests/cases/conformance/**/namespaces/**`、`.../moduleResolution/**` 中的 `namespace`/`module` | 实例化 namespace → IIFE；未实例化省略 | 6b-后续 + P10 |
| legacydecorators + metadata | `tests/cases/conformance/decorators/**`（`--experimentalDecorators --emitDecoratorMetadata`） | `__decorate`/`__metadata` helper 注入与设计类型序列化 | 6c+/P10 |
| typeserializer | （随 metadata）`decorators/**/metadata/**` | 类型 → `Object`/`Function`/`Number`… 元数据表达式 | 6c+/P10 |

> 这些 baseline 不替代每函数单测（PORTING §8.6）；它们是 stage 完整后的端到端验收。6b 仅落地 typeeraser 剥类型子集与 `constant_expression`，故标记为登记项。

## 与 impl.md 的对齐核对

- [ ] 2 个 Go `func Test*` 全部映射（TypeEraser + ImportElision），子用例逐条列出
- [ ] `TestTypeEraser` ~70 子用例对应 `typeeraser.rs` 擦除规则（修饰符/类型/接口/类型别名/类型参数/断言/JSX/仅类型 import-export/vms 分支）
- [ ] `TestImportElision` ~20 子用例对应 `importelision.rs`（引用判定/值 vs 仅类型/默认导出）
- [ ] 0 单测子包补行为级测试，对应 impl.md 各 `new_*_transformer` TODO
- [ ] expected 值均取自 Go 测试字面量（非 Rust 推断）
- [ ] 每条带 `// Go:` 锚点
- [ ] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `TestImportElision` 全量 | 依赖 checker.NewChecker + EmitResolver | P4 收口后回填 / P10 |
| estransforms 各 target 降级字节级 | 需 `tsc --target` baseline | P10 |
| moduletransforms CJS/ESM/AMD parity | 需 `tsc --module` baseline | P10 |
| jsxtransforms classic/automatic runtime | 需 `tsc --jsx` baseline | P10 |
| declarations `.d.ts` 全量（含 isolatedDeclarations 诊断） | 依赖 nodebuilder/modulespecifiers + checker | P10 |
