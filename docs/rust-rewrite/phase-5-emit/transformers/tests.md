# transformers: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 文件 / **2 `func Test`** / 约 90 子用例（`TestTypeEraser` ~70 + `TestImportElision` ~20，均在 `tstransforms`）。

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
| `chain_identity` | 根 | Chain 串联恒等 transformer | 解析→chain→emit == 原文 | `Chain` 语义 | |
| `chain_single_passthrough` | 根 | 单元素 Chain 直接返回 | 同上 | `Chain` len<2 分支 | |
| `extract_modifiers_filters` | 根 | 仅保留 allowed 修饰符 | `public static` + allowed=static → 仅 static | `ExtractModifiers` | |
| `flatten_destructuring_array` | 根 | 数组解构赋值展开 | `[a, b] = c` → 顺序赋值 | `FlattenDestructuringAssignment` | |
| `es2016_exponentiation` | estransforms | `a ** b` 降级（target ES2015） | `a ** b` → `Math.pow(a, b)` | `newExponentiationTransformer` | |
| `es2020_optional_chain` | estransforms | `a?.b` 降级（target ES2019） | `a?.b` → 三元/临时变量展开 | `newOptionalChainTransformer` | |
| `es2020_nullish_coalescing` | estransforms | `a ?? b` 降级 | → `(a !== null && a !== void 0) ? a : b` | `newNullishCoalescingTransformer` | |
| `get_es_transformer_target_dispatch` | estransforms | 按 target 选链 | ESNext/ES2016/older → 正确链 | `GetESTransformer` | |
| `commonjs_named_import` | moduletransforms | ESM→CJS 命名导入 | `import {x} from "m"` → `const m_1 = require("m")` | `NewCommonJSModuleTransformer` | |
| `jsx_classic_runtime` | jsxtransforms | `<a/>` → createElement | `<a/>` → `React.createElement("a", null)` | `NewJSXTransformer` | |
| `const_enum_inline` | inliners | const enum 成员内联 | `const enum E{A=1} E.A` → `1 /* E.A */` | `NewConstEnumInliningTransformer` | |
| `declaration_simple` | declarations | 基础 `.d.ts` 形状 | `export const x = 1;` → `export declare const x: number;` | `NewDeclarationTransformer` | |

> 上述行为级 expected 取自 TS/Go 已知降级形态；精确字节由 P10 兜底。const enum/declaration 等依赖 checker（P4），未就绪则该行 `—`。

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
