# transformers: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 文件 / **2 `func Test`** / 约 90 子用例（`TestTypeEraser` ~70 + `TestImportElision` ~20，均在 `tstransforms`，属 6b）。

> **6a 现状**：根包 Go 侧**无 `*_test.go`**，故按 PORTING §8.6「每函数一测」自写行为级单测。6a 落地 **13 个 `#[test]` + 9 个 doctest 全绿**（见末节「0 直接单测的子包」行为表，已逐条 ✓）。`TestTypeEraser`/`TestImportElision` 全量属 6b（tstransforms）。

> 字符串约定：`\n` = 换行，`\"` = 转义双引号。input/output 取自 Go 测试字面量。空输出 `""` 表示该 stage 把整条语句擦除。

> **6al 现状**：落地 `legacydecorators`（`--experimentalDecorators`）**首切片** —— 装饰的**属性成员** → 尾随 `__decorate(...)` 语句，含 `--emitDecoratorMetadata` 的 `design:type` 元数据（**消费 checker 4at** `EmitResolver::serialize_type_node_for_metadata`）。Go ground truth `legacydecorators.go`（`visitClassDeclaration`/`transformClassDeclarationWithoutClassDecorators`/`generateClassElementDecorationExpression`/`getClassMemberPrefix`/`getExpressionForPropertyName`）+ `metadata.go`/`typeserializer.go`（元数据注入），shape 对 `tsc --experimentalDecorators [--emitDecoratorMetadata]` 核对。**管线折叠（偏离 Go 两 transformer）**：Go 由 `MetadataTransformer` 先注入合成 `@__metadata(...)` 装饰器、再由 `LegacyDecoratorsTransformer` 收集成 `__decorate([...])`；本端口折成一遍——`generate_class_element_decoration_statement` 直接构造装饰器表达式列表 `[<真装饰器>, <metadata>]`（metadata 居末，同 Go `transformAllDecoratorsOfDeclaration` 顺序），emit 文本一致。**打印器约束**：装饰器数组 `[dec, __metadata(...)]` 恒单行（Rust printer 硬编码 array 单行，Go/tsc 多行），故 expected 取单行形。`__decorate`/`__metadata` helper 在本 crate 内定义（printer crate 不在编辑范围，同 `spread.rs` 的 `SPREAD_ARRAY_HELPER`），text/priority verbatim 自 Go `helpers.go`（`__decorate`=2、`__metadata`=3，故 prologue 中 decorate 先于 metadata）。`EmitReferenceResolver` 加 additive `serialize_type_node_for_metadata` 透传 + 新增 `new_legacy_decorators_transformer` / `new_legacy_decorators_transformer_with_resolver`（既有签名不变，standalone-only，未接入 compiler 管线，同 importElision）。`tsgo_transformers` +7 `#[test]`（slice1 instance 属性 `__decorate` **genuine RED→GREEN** + slice2 `design:type` 元数据 **genuine RED→GREEN** + slice3 static 前缀 `C` vs `C.prototype` **genuine RED→GREEN** + string→String 泛化 / TypeReference→Object（4at DEFER 边界）/ experimentalDecorators-off gate / class-decorator DEFER 守卫 4 个 coverage/guard，诚实记录：后 4 个 green-on-arrival）+ 3 doctest（resolver 透传 + 两个工厂）→ crate 合计 **226 unit + 44 doctest 全绿**（6ak 基线 219+41）。clippy（`-D warnings`）/ fmt（仅改动文件）/ `cargo build -p tsgo_compiler` 均已实跑通过（公共 API additive）。DEFER（均带 blocked-by）：class 装饰器 `@dec class C {}` → `let C = …; C = __decorate([dec], C)` 包裹（blocked-by class-alias/let-binding 包裹 + emit-name 形）、method/accessor 装饰器（`design:type`=`Function`、`design:returntype`）、参数装饰器 `__param` + `design:paramtypes`、`TypeReference` `design:type`（checker 4at DEFER→`Object`）、计算属性名、装饰器求值序边角、重载上的装饰器、`export`/`default` 修饰的装饰类。
>
> **6p 现状**：6p 是 **printer 上游基建轮**（emit-context node-based name generator `GenerateNameForNode`），**未新增/未改动 transformers 测试**，transformers 计数保持 6o 的 **126 unit + 24 doctest 全绿**。新增的 15 个 node-based 名字测试在 `tsgo_printer`（`namegenerator_test.rs`，见 `phase-4-checker/printer/tests.md`）。把 name generator 应用到 classfields（accessor backing / 私有方法 brand / class-expr hoist）的 transformers 测试是下一轮工作。
>
> **6q 现状**：消费 6p 的 node-based generated **private** name，落地 classfields `accessor` 自动访问器降级（ES2022-native 形态）。`tsgo_transformers` +4 `#[test]`（见下表 6q 行），crate 合计 **130 unit + 24 doctest 全绿**。clippy（`-D warnings`）/ fmt 均已实跑通过。
>
> **6aa 现状**：落地 ES2015 **数组字面量 spread** 与 **调用参 spread** 降级（pre-ES2015 目标，经 `__spreadArray` helper + `.apply`）。复用 6d-2 emit-helper 基建（request + 源文件 prologue attach，同 `objectrestspread`/`forawait`）。Go 端**无** ES2015 spread transform（`GetESTransformer` 链止于 `NewES2016Transformer`，更老目标不降级），故 ground truth 取 `tsc --target es5` 对拍——**briefing 修正**：briefing 称 `f(...args)` → `f.apply(void 0, __spreadArray([], args, false))`，实测 tsc 为 `f.apply(void 0, args)`（arg-list 单 spread 捷径直接传参，无 helper）；且 arg-list spread 段 `pack=false`、数组字面量 spread 段 `pack=true`。新增 `spread.rs`（含 `SPREAD_ARRAY_HELPER` 定义，printer crate 不在编辑范围，同 `forawait.rs`）+ `spread_test.rs`（8 个 `#[test]`：array tracer + 2 coverage + call tracer + 2 coverage + 2 DEFER 守卫）+ 1 doctest。`tsgo_transformers` 合计 **179 unit + 28 doctest 全绿**（6z 基线 171+27）。clippy（`-D warnings`）/ fmt 均已实跑通过。DEFER：`new C(...args)`、`super(...args)`、非简单成员受体 capture temp、`--downlevelIteration`、wiring 进 `GetESTransformer`（无 `NewES2015Transformer` 链）。
>
> **6ae 现状**：新增 `systemmodule`（`module: system`），落地 **System.register 包装核心**——把模块体包进 `System.register([<deps>], function (exports_1, context_1) { "use strict"; return { setters: [<setters>], execute: function () { <body> } }; })`。Go ground truth `moduletransforms/systemmodule.go:NewSystemModuleTransformer`/`transformSourceFile`/`createSystemModuleBody`（本仓 `_submodules/TypeScript` 空、无 `.go` 镜像，shape 取 **tsc `--module system` 已知输出**对拍 + 既有 CJS/ESM 端口约束）。复用 6e `collect_external_module_info` 收 external imports。**打印器约束**：array/object 字面量恒单行（`emit_expressions.rs` 硬编码 `multi_line=false`），故 return 对象/setters/deps/空 execute 块内联（tsc 实多行），仅外层 module body block 设 `EmitFlags::MULTI_LINE`（Go `multiLine:true`）多行；与既有 CJS `{ value: true }` 内联一致。**name generator 偏离（TODO(port)）**：binding-less import setter param Go 用 `createUniqueName("")`=`_1`，Rust 空 base 路径丢前导 `_`（`!base_name.is_empty()` 守卫）→ `1`，故传 base `"_"` 复现 `_1`（printer 出编辑范围）。`tsgo_transformers` +4 `#[test]`（empty tracer + side-effect import deps/setter + execute body + gate 透传，前 3 均 genuine RED→GREEN）+ 1 doctest，crate 合计 **192 unit + 31 doctest 全绿**（6ad 基线 188+30）。**ZERO** ast/printer/checker 增长（全走 arena 既有构造器 + factory `new_unique_name` + EmitContext `set_emit_flags`，复用 6e `collect_external_module_info`；新增 `new_system_module_transformer` 公共入口 + `pub mod systemmodule;`，未改既有公共链入口）。clippy（`-D warnings`）/ fmt 均已实跑通过。DEFER（均 blocked-by 真实 `ReferenceResolver`，同 CJS）：export-setter 接线（named export→`exports({...})`/setter 体转发）、import binding 重写·hoisting·live bindings；以及 `var __moduleName = context_1 && context_1.id;`、module-name 首参、`export *` star helper、dependency 分组/去重。
>
> **6ak 现状**：落地 `commonjsmodule` **CommonJS local-export use-site 重写**（消费 checker 4as `EmitResolver::get_referenced_export_container`，CJS 传 `prefix_locals=false`）——顶层**导出变量**的 value-position use 重写成 `exports.<name>`（`export const x = 1;\nx;` 的 use `x;` → `exports.x;`）。扩展 6ai 同一 identifier-substitution 阶段：每个 use 先查 export container（命中 SourceFile→`exports.x`），否则回落到既有 import-binding 匹配（镜像 Go `visitExpressionIdentifier` 顺序）。`EmitReferenceResolver` 加 additive `get_referenced_export_container` passthrough；新增私有 helper `substitute_exported_name_use`（复用既有 `exports` 标识符 + `new_property_access_expression`）。不回归 6e/6w 声明降级（`exports.x = void 0;\nexports.x = 1;`）。`tsgo_transformers` +3 `#[test]`（exported-var use **genuine RED→GREEN** + 2 scope/non-export guard **green-on-arrival**——诚实记录：guard 只断言 use 保持裸，slice1 impl 前后皆裸，是作用域正确性回归守卫而非依赖；不伪造 RED）+ 1 doctest（`get_referenced_export_container` passthrough）→ crate 合计 **219 unit + 41 doctest 全绿**（6aj 基线 216+40）。clippy（`-D warnings`）/ fmt（仅改动文件）/ `cargo build -p tsgo_compiler` 均已实跑通过（公共 API additive，CJS 构造器签名不变）。DEFER：exported function/class use-site（4as `ExportHasLocal && !Variable` 守卫返回 None）、namespace/enum export container（4as DEFER）、shorthand-property 展开、ESM/System export 引用重写。
>
> **6aj 现状**：补齐 6ai 明确 DEFER 的 **default & namespace import 作用域正确 use-site 重写行为级覆盖**（复用 6ai resolver 接线，消费 4an `resolve_reference`）。**诚实记录**：6ai 落地 resolver 臂时已对 `ImportBinding.decl`（named=`ImportSpecifier`/default=`ImportClause`/namespace=`NamespaceImport`）泛化匹配，故 default/namespace 的 resolver-path 行为早已正确，仅缺测试——本轮 4 个 `#[test]` 全部 test-first 但**观察到直接 GREEN 而非 RED**（不伪造 RED，破坏再修是 TDD 文档明禁的剧场行为）。测试非空泛硬证据：default slice（模块级 `d`→`m_1.default`）+ scope guard（内层 shadow `d` 保持裸）二者同绿即排除「resolver 返 None」与「按名匹配」两种退化。**无 impl 变更**，仅新增 4 `#[test]`（default use / namespace use / default scope guard / namespace scope guard）+ 更新 `commonjsmodule.rs` 模块 doc。`tsgo_transformers` 合计 **216 unit + 40 doctest 全绿**（6ai 基线 212+40）。clippy（`-D warnings`）/ fmt（仅改动文件）/ `cargo build -p tsgo_compiler` 均已实跑通过（公共 API additive）。DEFER：export 引用重写（`GetReferencedExportContainer`）、shorthand-property 展开、string-literal element-access 形、combined default+named interop 边角、ESM/System 引用重写。
>
> **6ai 现状**：落地 `commonjsmodule` **作用域正确的 import use-site 重写**（消费 checker 4an `EmitResolver::resolve_reference`）——CommonJS 下一个 imported binding 的 use 重写为 require-alias 上的限定成员访问（`x`→`m_1.x`），且**仅当**该 use 真解析到该 import（非局部/非 shadow）。新增 additive `new_common_js_module_transformer_with_resolver(opt, resolver)`（既有 `new_common_js_module_transformer` 签名不变）+ `EmitReferenceResolver` 两个 additive 透传 `resolve_reference`/`symbol_of_declaration` + `ImportBinding.decl` 字段。`tsgo_transformers` +3 `#[test]`（slice1 tracer + headline scope-correct guard 均 genuine RED→GREEN + call-arg coverage）+ 3 doctest，crate 合计 **212 unit + 40 doctest 全绿**（6ah 基线 209+37）。clippy（`-D warnings`）/ fmt / `cargo build -p tsgo_compiler` 均已实跑通过。DEFER：default/namespace use 经 resolver（仅按名回退覆盖）→ **6aj 已补**；export 引用重写（`GetReferencedExportContainer`）、shorthand-property 展开、string-literal element-access 形、ESM/System 引用重写。
>
> **6ad 现状**：落地 `commonjsmodule` **动态 `import()` 降级**（`module: commonjs`）：`import(expr)` → `Promise.resolve().then(() => __importStar(require(expr)))`。Go ground truth `commonjsmodule.go:createImportCallExpressionCommonJS`——**briefing 修正**：briefing 称「base form (no interop) `Promise.resolve().then(() => require("m"))` 先行、esModuleInterop 变体后补」，实测 Go（已对 raw GitHub `microsoft/typescript-go/main` 同文件核对，行 1908）**无条件** `NewImportStarHelper(require(...))`（**不**门控 `esModuleInterop`），且回调为 **arrow**（`() => …`，非 function expr）——故唯一形态即 importStar-wrapped，无两片之分。参数为 `isSimpleInlineableExpression`（字符串字面量等）时内联进 `require(arg)`、`Promise.resolve()` 无参、arrow 无形参；no-arg → `require()`。**解析器缺口**：`internal/parser` 仍 DEFER 动态 `import(...)` call-head（实测对 `import("m")` 死循环），`internal/parser` 不在本轮编辑边界，故 transformer 降级用**合成 AST**（= 解析器本应产出的 `CallExpression{import keyword, [...]}`）经公开 transformer 入口行为级验证；端到端 parse→transform blocked-by phase-3 解析器。`tsgo_transformers` +2 `#[test]`（string-literal arg tracer + no-arg 边界，均 genuine RED→GREEN）、无新 doctest，crate 合计 **188 unit + 30 doctest 全绿**（6ac 基线 186+30）。**ZERO** ast/printer/checker/parser 增长（全走 arena 既有构造器 + 6e-3 helper infra + 既有 `set_node_substitution`）。clippy（`-D warnings`）/ fmt 均已实跑通过。DEFER：`needSyncEval` 模板形（非 inlineable 参数）、spread 参数、top-level-await import、`import.meta`、解析器 call-head、`shouldTransformImportCall` 完整 module-kind 门控。
>
> **6ac 现状**：移植 `impliedmodule`（implied-module-format transformer）——按文件 emit module format **逐文件分派** CJS/ESM。Go ground truth `moduletransforms/impliedmodule.go:NewImpliedModuleTransformer`/`visitSourceFile`：`IsDeclarationFile` → 原样；否则 `format := getEmitModuleFormatOfFile(node)`，`format >= core.ModuleKindES2015` → `NewESModuleTransformer`，否则 → `NewCommonJSModuleTransformer`。Go-confirmed 谓词 `is_es_module_format(format) = format >= ModuleKind::Es2015`（commonjs=1 → CJS；es2015=5/esnext=99 → ESM）。`tsgo_transformers` +4 `#[test]`（commonjs→CJS tracer + esnext→ESM + es2015 谓词边界 + 声明文件守卫，均 genuine RED→GREEN，expected = 对应模块 transform 的同输入输出）+ 2 doctest，crate 合计 **186 unit + 30 doctest 全绿**（6ab 基线 182+28）。**ZERO** ast/printer/checker 增长（消费既有 CJS/ESM 公开入口 + `run_visit`，未动任一公开链 API）。clippy（`-D warnings`）/ fmt 均已实跑通过。DEFER：per-file `impliedNodeFormat`/`.cjs`/`.mjs` 探测（blocked-by `SourceFileMetaData` 未接线，同 compiler P6-2）、AMD/UMD/System 格式。
>
> **6ab 现状**：收口 6u DEFER 项——`esmodule` 的 **es2015 `export * as ns from "m"` 命名空间 re-export 改写**（复用 6p `new_generated_name_for_node`）。Go ground truth `moduletransforms/esmodule.go:visitExportDeclaration`：`Module <= ES2015` + `IsNamespaceExport` 时改写为 `import * as ns_1 from "m";` + `export { ns_1 as ns };`（名为 `default` → `export default default_1;`，`IsExportNamespaceAsDefaultDeclaration` 分支）；esnext 等 `Module > ES2015` **透传**（合法语法）。tsc `--module es2015/esnext` 对拍确认。`tsgo_transformers` +3 `#[test]`（es2015 named tracer + esnext passthrough guard + es2015 export-default coverage，均 genuine RED→GREEN），crate 合计 **182 unit + 28 doctest 全绿**（6aa 基线 179+28，doctest 无新增）。**ZERO** ast/printer/checker 增长（全走 arena 既有构造器 + 6p factory）。clippy（`-D warnings`）/ fmt 均已实跑通过。DEFER：type-only import elision（EmitResolver）、作用域正确引用重写（ReferenceResolver）、`export * as "default"` string-literal 名、import attributes 透传、`--module preserve`/`--rewriteRelativeImportExtensions`/动态 `import()`/Node16+ `import =`/external-helpers 注入。
>
> **6r 现状**：移除 6o「class-expr 含 static 字段保持不变」守卫的可达子集，落地 **ClassExpression static 字段语句 hoist**（逗号序列 + temp 包裹：`const C = class { static x = 1 }` → `var _a;\nconst C = (_a = class {}, _a.x = 1, _a)`）。复用 6c-3/6i 变量环境（`add_variable_declaration` + SourceFile `start/end_variable_environment`）+ 6p `new_temp_variable`（同一 temp 三处复用同名）；core 抽 `lower_class_parts`（受体无关 static 字段）。`tsgo_transformers` +4 `#[test]`（tracer 1 + 多 static 1 + 混合 instance+static 1 + 计算名 DEFER 守卫 1；6o 的 `class_expression_with_static_field_is_left_unchanged` 改名重写为 `named_class_expression_static_field_keeps_name_in_comma_sequence`，净 +0），crate 合计 **134 unit + 24 doctest 全绿**。clippy（`-D warnings`）/ fmt 均已实跑通过。computed/私有字段在表达式位仍 DEFER（需 Go `pendingExpressions` 内联）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/transformers/tstransforms/typeeraser_test.go` | `tstransforms/typeeraser.rs`（external `tstransforms_test`） | 1 |
| `internal/transformers/tstransforms/importelision_test.go` | `tstransforms/importelision_test.rs`（6af：import 侧值用例 ✓，crate-local `BoundProgram` + 4an `EmitResolver`；export 侧/`import =` DEFER）| 1 |

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

### 6af 落地状态（import 侧值用例 ✓，scope-correct）

被 checker **4an** `EmitResolver::is_referenced` 解锁后，`tstransforms/importelision.rs` 落地 **import 侧**可达子集（`internal/transformers/tstransforms/importelision_test.rs`，9 个 `#[test]`，驱动：`parse_shared` + `build_reference_resolver`（独立 parse+bind + 4an `EmitResolver`）→ `new_import_elision_transformer` → emit；ESM passthrough，未串 module transform）：

| Go 子用例 | Rust `#[test]` | 完成 |
|---|---|---|
| `ImportDeclaration#7`（`import { x } from "m";` 未用 → ∅）| `unused_named_import_is_elided`（**genuine RED**：先恒等 stub 见红）| ✓ |
| `ImportDeclaration#4`（`import { x } from "m"; x;` → 原样）| `used_named_import_is_kept` | ✓ |
| （强于 Go：scope-correct，内层 `var x` shadow → import 省略）| `shadowed_use_does_not_keep_import_alive`（**genuine RED**：先按名匹配式 keep=true 见红）| ✓ |
| （强于 Go：per-specifier 部分丢 `import { a, b } from "m"; a;` → `import { a } from "m"; a;`）| `unused_specifier_dropped_referenced_specifier_kept` | ✓ |
| `ImportDeclaration#1`（`import "m";` 副作用 → 保留）| `side_effect_only_import_is_kept` | ✓ |
| `ImportDeclaration#5`（`import * as ns from "m";` 未用 → ∅）| `unused_namespace_import_is_elided` | ✓ |
| `ImportDeclaration#2`（`import * as ns from "m"; ns;` → 原样）| `used_namespace_import_is_kept` | ✓ |
| `ImportDeclaration#6`（`import d from "m";` 未用 → ∅）| `unused_default_import_is_elided` | ✓ |
| `ImportDeclaration#3`（`import d from "m"; d;` → 原样）| `used_default_import_is_kept` | ✓ |

> **DEFER（6af 之后）**：`ImportEquals#1/#2`（需 `IsTopLevelValueImportEqualsWithEntityName`）、`ExportDeclaration#1..9` / `ExportAssignment#1/#2`（需 `IsValueAliasDeclaration`）、仅类型位置 use 续命（需 `markLinkedReferences` 的 type-vs-value meaning，4an 亦 DEFER）。这些子用例的整表对拍仍待 checker 补 `IsValueAliasDeclaration` / `IsTopLevelValueImportEqualsWithEntityName` 后回填，或 P10 端到端兜底。

### 6ag 落地状态（export specifier 侧 ✓，消费 4ao `is_value_alias_declaration`）

被 checker **4ao** `EmitResolver::is_value_alias_declaration` / `is_referenced_alias_declaration`（均 additive 公共方法）解锁后，`tstransforms/importelision.rs` 落地 **export specifier 侧**可达子集（同驱动：`parse_shared` + `build_reference_resolver` → `new_import_elision_transformer` → emit；import-elision 单跑，未串 type eraser）。`EmitReferenceResolver` additive 扩展 `is_value_alias_declaration`/`is_referenced_alias_declaration` 两个透传方法（`new_import_elision_transformer` 签名不变）：

| 关联 Go 子用例 | Rust `#[test]` | 完成 |
|---|---|---|
| `ExportDeclaration#8`/`#6` 同形（type-only export specifier → ∅；本轮 interface 透传因未串 type eraser）| `type_only_export_specifier_is_elided`（**genuine RED**：先无 export 各臂透传见红）| ✓ |
| `ExportDeclaration#7`/`#5` 同形（value export specifier 保留）| `value_export_specifier_is_kept` | ✓ |
| （per-specifier：type-only 丢、value 留）| `type_only_export_specifier_dropped_value_specifier_kept` | ✓ |
| `ExportDeclaration#1/#3`（`export * from "m"` 无 export clause → 保留）| `export_star_reexport_is_kept` | ✓ |

> **DEFER（6ag 之后，实测确认）**：
> - **`ImportEqualsDeclaration` 省略**（`import x = require("m")` 未用应省略）：**实测 BLOCKED**——checker 4ao `is_referenced` 的 `declaration_name` 未列 `ImportEqualsDeclaration`，声明自身名 `x` 未排除 → `is_referenced` 恒 true → used/unused 两例均"保留"、不可区分。无法在不改 `internal/checker/**`（边界外）下驱动区分性 GREEN，本轮不接此臂。blocked-by checker `declaration_name` 排除 `ImportEqualsDeclaration` 名 + `IsTopLevelValueImportEqualsWithEntityName`。
> - **`ExportAssignment` 省略**（`export =`/`export default`）：checker 4ao `is_value_alias_declaration` 对 `ExportAssignment` 落 `_ => false`（仅处理 import/export specifier），直接接线会误省略 `export default x`（Go 保留 value 形）。blocked-by checker `isValueAliasDeclarationWorker` 的 `ExportAssignment` 分支。
> - **跨模块 target value-ness**（`import { x } from "m"; export { x };`）：blocked-by checker `resolveExternalModuleSymbol`/`getExportSymbolOfValueSymbolIfExported`（4ao DEFER）。
> - **`export *` 带 export clause 的 namespace re-export 改写**、value 位置 use 的 type-only-ness、`verbatimModuleSyntax`/`isolatedModules` 策略变体。

### 6ah 落地状态（`import =` / `export =` 侧 ✓，消费 4ap `is_referenced_alias_declaration` / `is_value_alias_declaration`）

被 checker **4ap** 解锁（`is_referenced` 的 `declaration_name` 新增 `ImportEqualsDeclaration` 臂，未引用 `import x = require("m")` 的 `is_referenced_alias_declaration` 现报 false；`is_value_alias_declaration` 新增 `ExportAssignment` 臂）后，`tstransforms/importelision.rs` 落地 `import =`（external-module 形）/ `export =` 可达子集（同驱动：`parse_shared` + `build_reference_resolver` → `new_import_elision_transformer` → emit；import-elision 单跑，未串 type eraser）。`new_import_elision_transformer` 签名不变；仅多用 `EmitReferenceResolver` 既有透传方法（6ag 已加），ZERO ast/printer/checker 增长：

| 关联 Go 子用例 | Rust `#[test]` | 完成 |
|---|---|---|
| `ImportEquals` 外部模块形未用（`import x = require("m");` → ∅）| `unused_import_equals_require_is_elided`（**genuine RED**：6ag 透传见红）| ✓ |
| `ImportEquals` 外部模块形已用（`import x = require("m"); x;` 保留）| `used_import_equals_require_is_kept`（guard）| ✓ |
| `ExportAssignment#2` 同形（type-only `export = I` → ∅）| `type_only_export_equals_is_elided`（**genuine RED**：6ag 透传见红）| ✓ |
| `ExportAssignment#1` 同形（value `export = f` 保留）| `value_export_equals_is_kept`（guard）| ✓ |

> **DEFER（6ah 之后）**：entity-name `import x = a.b`（blocked-by checker `IsTopLevelValueImportEqualsWithEntityName`，4ap DEFER）；跨模块 re-export `import { x } from "m"; export = x` / `export { x }`（blocked-by `resolveExternalModuleSymbol`）；value 位置 use 的 type-only-ness、`verbatimModuleSyntax`/`isolatedModules`/const-enum 策略变体。

| 收口检查 | Go 对照 | 完成 |
|---|---|---|
| import 侧值用例（namespace/default/named × 用/未用 + side-effect + per-specifier + scope-correct）有对应 Rust 用例 | `importelision_test.go:TestImportElision`（import 侧）| ✓ |
| export specifier 侧（type-only 丢 / value 留 / per-specifier / `export *` 无 clause 保留）有对应 Rust 用例 | `importelision_test.go:TestImportElision`（export specifier 侧）| ✓ (6ag) |
| `import = require("m")`（external 形 用/未用）/ `export =`（value 留 / type-only 丢）有对应 Rust 用例 | `importelision_test.go:TestImportElision`（`import =`/`export =`）| ✓ (6ah) |
| entity-name `import =` / 跨模块 re-export / 仅类型续命 子用例 | `importelision_test.go:TestImportElision`（其余）| — (blocked-by checker `IsTopLevelValueImportEqualsWithEntityName` / `resolveExternalModuleSymbol` / `markLinkedReferences`) |

> **依赖提示**：6af 用 crate-local `BoundProgram`（`test_support.rs` 的 `build_reference_resolver`，`tsgo_parser`+`tsgo_binder`+4an `EmitResolver`），不再整体 DEFER；checker 的 `StubProgram` 是 `pub(crate)` 跨 crate 不可见故自建。export 侧 / `import =` 仍 `—`（blocked-by 上述 checker 查询）。

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
| `destructuring::array_binding_decomposes_to_element_accesses` | 根 | **6j tracer**：数组绑定模式 → 元素访问声明 | `var [a, b] = arr;` → `var a = arr[0], b = arr[1];` | `flattenArrayBindingOrAssignmentPattern` | ✓ |
| `destructuring::object_binding_decomposes_to_property_accesses` | 根 | **6j** 对象绑定模式 → 属性访问声明 | `var { a, b } = o;` → `var a = o.a, b = o.b;` | `flattenObjectBindingOrAssignmentPattern` | ✓ |
| `destructuring::array_default_guards_with_void_zero` | 根 | **6j** 数组默认值 → `=== void 0` 守卫（读一次 temp）| `var [a = 1] = arr;` → `var _a = arr[0], a = _a === void 0 ? 1 : _a;` | `createDefaultValueCheck` | ✓ |
| `destructuring::object_default_guards_with_void_zero` | 根 | **6j** 对象默认值 → `=== void 0` 守卫 | `var { a = 1 } = o;` → `var _a = o.a, a = _a === void 0 ? 1 : _a;` | `createDefaultValueCheck` | ✓ |
| `destructuring::nested_array_pattern_composes_element_accesses` | 根 | **6j**（泛化）嵌套数组模式递归 | `var [[a]] = x;` → `var a = x[0][0];` | `flattenArrayBindingOrAssignmentPattern`(递归) | ✓ |
| `destructuring::nested_object_pattern_composes_property_accesses` | 根 | **6j**（泛化）嵌套对象模式递归 | `var { a: { b } } = o;` → `var b = o.a.b;` | `flattenObjectBindingOrAssignmentPattern`(递归) | ✓ |
| `destructuring::array_rest_lowers_to_slice` | 根 | **6j** 数组 rest → `array.slice(i)` | `var [a, ...r] = arr;` → `var a = arr[0], r = arr.slice(1);` | `NewArraySliceCall` | ✓ |
| `destructuring::computed_key_captures_object_then_key_into_temps` | 根 | **6j** 非字面量计算键：先捕获对象再捕获键（保 `o`→`k` 序）| `var { [k]: a } = o;` → `var _a = o, _b = k, a = _a[_b];` | `createDestructuringPropertyAccess`(computed) + `BindingOrAssignmentElementContainsNonLiteralComputedName` | ✓ |
| `destructuring::non_simple_initializer_is_captured_in_a_temp_declaration` | 根 | **6j**（泛化）非简单 initializer → temp 同语句声明（hoist=false）| `var [a, b] = f();` → `var _a = f(), a = _a[0], b = _a[1];` | `ensureIdentifier`(hoist=false) | ✓ |
| `destructuring::array_assignment_decomposes_to_element_access_assignments` | 根 | **6k tracer**：数组字面量赋值 → 元素访问赋值逗号序列（赋值模式 flattener）| `[a, b] = arr;` → `a = arr[0], b = arr[1];` | `FlattenDestructuringAssignment` / `emitAssignment` | ✓ |
| `destructuring::array_assignment_non_simple_value_hoists_temp` | 根 | **6k** 非简单 value → hoist temp（`var`）+ capture 折进逗号 | `[a, b] = f();` → `var _a;\n_a = f(), a = _a[0], b = _a[1];` | `ensureIdentifier`(hoist=true) | ✓ |
| `destructuring::object_assignment_decomposes_to_property_access_assignments` | 根 | **6k** 对象字面量赋值 → 属性访问赋值；statement 前导 `(` 丢弃 | `({ a, b } = o);` → `a = o.a, b = o.b;` | `flattenObjectBindingOrAssignmentPattern`(assignment) | ✓ |
| `destructuring::array_assignment_default_guards_with_void_zero` | 根 | **6k** 数组赋值默认值 → `=== void 0` 守卫（temp hoist 进 `var`）| `[a = 1] = arr;` → `var _a;\n_a = arr[0], a = _a === void 0 ? 1 : _a;` | `createDefaultValueCheck` | ✓ |
| `destructuring::nested_array_assignment_composes_element_accesses` | 根 | **6k**（泛化）嵌套数组赋值递归 | `[[a]] = x;` → `a = x[0][0];` | `flattenArrayBindingOrAssignmentPattern`(递归, assignment) | ✓ |
| `destructuring::nested_object_assignment_composes_property_accesses` | 根 | **6k**（泛化）嵌套对象赋值递归 | `({ a: { b } } = o);` → `b = o.a.b;` | `flattenObjectBindingOrAssignmentPattern`(递归, assignment) | ✓ |
| `destructuring::array_assignment_rest_lowers_to_slice` | 根 | **6k** 数组赋值 rest → `array.slice(i)` | `[a, ...r] = arr;` → `a = arr[0], r = arr.slice(1);` | `NewArraySliceCall` | ✓ |
| `estransforms::objectrestspread::object_rest_assignment_keeps_bindings_and_lowers_rest` | estransforms | **6k tracer**：对象 rest **赋值**经泛型 flattener @ `FlattenLevelObjectRest`，kept 绑定保留 + 共享 `__rest`，括号保留 | `({ a, ...r } = o);` → prologue `__rest` + `({ a } = o, r = __rest(o, ["a"]));` | `visitBinaryExpression` + `flattenObjectBindingOrAssignmentPattern`(rest arm) | ✓ |
| `estransforms::objectrestspread::object_rest_assignment_lists_all_kept_keys` | estransforms | **6k** 多 kept 键（源序排除）| `({ a, b, ...r } = o);` → `({ a, b } = o, r = __rest(o, ["a", "b"]));` | `NewRestHelper` | ✓ |
| `estransforms::exponentiation::tests::exponentiation_operator_lowered_to_math_pow` | estransforms | `**` → `Math.pow`（**6c-1 tracer**） | `a ** b` → `Math.pow(a, b);` | `newExponentiationTransformer` | ✓ |
| `estransforms::exponentiation::tests::exponentiation_assignment_to_identifier_lowered` | estransforms | `**=`（标识符目标）降级 | `a **= b` → `a = Math.pow(a, b);` | `visitExponentiationAssignmentExpression` | ✓ |
| `estransforms::exponentiation::tests::exponentiation_assignment_to_property_access_hoists_temp` | estransforms | **6c-3** `a.x **= b` temp hoist | → `var _a;\n(_a = a).x = Math.pow(_a.x, b);` | `visitExponentiationAssignmentExpression` | ✓ |
| `estransforms::exponentiation::tests::exponentiation_assignment_to_element_access_hoists_temps` | estransforms | **6c-3** `a[x] **= b` 双 temp hoist | → `var _a, _b;\n(_a = a)[_b = x] = Math.pow(_a[_b], b);` | `visitExponentiationAssignmentExpression` | ✓ |
| `estransforms::exponentiation::tests::property_assignment_inside_function_body_hoists_into_body` | estransforms | **6i** per-scope var-env：`**=` property 目标在函数体内 hoist 进体 | `function f() { a.x **= b; }` → `function f() { var _a; (_a = a).x = Math.pow(_a.x, b); }` | `VisitFunctionBody` | ✓ |
| `printer::emitcontext::variable_environment_hoists_declarations_into_a_var_statement` | printer（additive） | **6c-3** var-env：hoist 成 `var` 语句 | 2 temp → 1 `VariableStatement`（2 decls） | `EndVariableEnvironment` | ✓ |
| `printer::emit_statements::syntax_list_statement_emits_children_in_sequence` | printer（additive） | **6c-3** `SyntaxList` 语句逐子 emit | 合成 `SyntaxList[a;, b;]` → `a;\nb;` | `emitList`(flatten) | ✓ |
| `estransforms::classfields::tests::instance_field_initializer_moves_to_constructor` | estransforms | 实例字段 → 合成构造器赋值 | `class C { x = 1 }` → `class C { constructor() { this.x = 1; } }` | `transformClassMembers` | ✓ |
| `estransforms::classfields::tests::multiple_instance_fields_move_to_constructor` | estransforms | 多字段 → 多赋值（源序） | `class C { x = 1; y = 2 }` → ctor `this.x=1; this.y=2;` | `transformClassMembers` | ✓ |
| `estransforms::classfields::tests::field_inits_prepend_to_existing_constructor` | estransforms | **6c-2** 既有构造器：字段插体顶部 | `class C { x=1; constructor(){ this.y=2 } }` → `this.x=1; this.y=2;` | `transformConstructorBody` | ✓ |
| `estransforms::classfields::tests::derived_class_synthesizes_constructor_with_super` | estransforms | **6c-2** 派生类合成构造器 + `super(...arguments)` | `class C extends B { x=1 }` → `constructor(){ super(...arguments); this.x=1 }` | `transformConstructorBody`(needsSyntheticConstructor) | ✓ |
| `estransforms::classfields::tests::field_inits_inserted_after_super_call` | estransforms | **6c-2** 既有 `super()` 后插入字段 | `class C extends B { x=1; constructor(){ super(); this.y=2 } }` → `super(); this.x=1; this.y=2;` | `transformConstructorBodyWorker` | ✓ |
| `estransforms::classfields::tests::static_field_becomes_assignment_after_class` | estransforms | **6c-3** static 字段 → 类后 `C.x = …` | `class C { static x = 1 }` → `class C {\n}\nC.x = 1;` | `addPropertyOrClassStaticBlockStatements` | ✓ |
| `estransforms::classfields::tests::private_field_initializer_uses_weakmap_set` | estransforms | **6c-4** 私有字段（直接 WeakMap，仅写） | `class C { #x = 1 }` → `var _C_x = new WeakMap();` + ctor `_C_x.set(this, 1);` | `transformPrivateFieldInitializer` | ✓ |
| `estransforms::classfields::tests::private_field_read_uses_weakmap_get` | estransforms | **6c-4** 私有读重写 | `m() { return this.#x; }` → `return _C_x.get(this);` | `createPrivateIdentifierAccess` | ✓ |
| `estransforms::classfields::tests::private_field_write_uses_weakmap_set` | estransforms | **6c-4** 私有写重写 | `m(v) { this.#x = v; }` → `_C_x.set(this, v);` | `createPrivateIdentifierAssignment` | ✓ |
| `estransforms::classfields::tests::computed_field_name_is_hoisted_to_temp` | estransforms | **6c-4** 计算字段名 → 类前 temp 缓存 | `class C { [k] = 1 }` → `var _a = k;` + ctor `this[_a] = 1;` | `getPropertyNameExpressionIfNeeded` | ✓ |
| `estransforms::classfields::tests::class_expression_instance_field_moves_to_constructor` | estransforms | **6o** ClassExpression 实例字段（表达式位降级）| `const C = class { x = 1 };` → `const C = class { constructor() { this.x = 1; } };` | `visitClassExpression` | ✓ |
| `estransforms::classfields::tests::class_expression_static_field_hoists_to_comma_sequence_with_temp` | estransforms | **6r** tracer：class-expr 单 static 字段 → 逗号序列 + temp | `const C = class { static x = 1 };` → `var _a;\nconst C = (_a = class {}, _a.x = 1, _a);` | `visitClassExpressionInNewClassLexicalEnvironment`（`hasTransformableStatics`）| ✓ |
| `estransforms::classfields::tests::class_expression_multiple_static_fields_share_one_temp` | estransforms | **6r** coverage：多 static 字段共用单 temp | `const C = class { static x = 1; static y = 2 };` → `var _a;\nconst C = (_a = class {}, _a.x = 1, _a.y = 2, _a);` | 同上 | ✓ |
| `estransforms::classfields::tests::class_expression_instance_and_static_fields_lower_together` | estransforms | **6r** coverage：实例字段进构造器 + static 进逗号序列 | `const C = class { x = 1; static y = 2 };` → `var _a;\nconst C = (_a = class { constructor() { this.x = 1; } }, _a.y = 2, _a);` | 同上 | ✓ |
| `estransforms::classfields::tests::named_class_expression_static_field_keeps_name_in_comma_sequence` | estransforms | **6r**（替换 6o guard）：具名 class-expr 保留名，static 受体仍是 temp | `const C = class D { static x = 1 };` → `var _a;\nconst C = (_a = class D {}, _a.x = 1, _a);` | 同上 | ✓ |
| `estransforms::classfields::tests::class_expression_with_computed_field_is_left_unchanged` | estransforms | **6r** DEFER guard：class-expr 计算字段需 `pendingExpressions` 内联，保持不变 | `const C = class { [k] = 1 };` → 原样 | `visitClassExpressionInNewClassLexicalEnvironment`（`pendingExpressions` 分支 DEFER）| ✓ |
| `estransforms::classfields::tests::instance_auto_accessor_lowers_to_backing_field_and_redirectors` | estransforms | **6q** 实例 `accessor` → backing 私有字段 + get/set 重定向器（ES2022-native；backing 名经 6p generated private name）| `class C { accessor x = 1; }` → `#x_accessor_storage = 1;` + `get x(){ return this.#x_accessor_storage; }` + `set x(value){ this.#x_accessor_storage = value; }` | `transformAutoAccessor` / `createAccessorProperty{BackingField,GetRedirector,SetRedirector}` | ✓ |
| `estransforms::classfields::tests::static_auto_accessor_keeps_static_modifier` | estransforms | **6q** static `accessor` → 保留 `static` 修饰（genuine RED→GREEN 扩展），receiver 仍用 `this` | `class C { static accessor x = 1; }` → `static #x_accessor_storage = 1;` + `static get/set x` | `transformAutoAccessor`(static) + `visitModifier`(keep static/strip accessor) | ✓ |
| `estransforms::classfields::tests::class_expression_auto_accessor_lowers_in_place` | estransforms | **6q** coverage：accessor 降级在类**表达式**位（统一 handler）| `const C = class { accessor x = 1 };` → `const C = class { #x_accessor_storage = 1; get/set x };` | `visitClassExpression` → `transformAutoAccessor` | ✓ |
| `estransforms::classfields::tests::auto_accessor_without_initializer` | estransforms | **6q** coverage：无初值 accessor → backing 字段无初值 | `class C { accessor x; }` → `#x_accessor_storage;` + get/set | `createAccessorPropertyBackingField`（nil init）| ✓ |
| `estransforms::optionalchain::tests::optional_property_access_lowered` | estransforms | **6d** 可选属性访问 | `a?.b;` → `a === null \|\| a === void 0 ? void 0 : a.b;` | `visitOptionalExpression` | ✓ |
| `estransforms::optionalchain::tests::optional_element_access_lowered` | estransforms | **6d** 可选元素访问 | `a?.[x];` → `… ? void 0 : a[x];` | `visitOptionalExpression` | ✓ |
| `estransforms::optionalchain::tests::optional_call_lowered` | estransforms | **6d** 可选调用 | `a?.();` → `… ? void 0 : a();` | `flattenChain`(call) | ✓ |
| `estransforms::optionalchain::tests::optional_method_call_lowered` | estransforms | **6d** 单 `?.` + 尾随调用 | `a?.b();` → `… ? void 0 : a.b();` | `flattenChain` | ✓ |
| `estransforms::optionalchain::tests::optional_chain_trailing_property_lowered` | estransforms | **6d** 尾随非可选属性段 | `a?.b.c;` → `… ? void 0 : a.b.c;` | `flattenChain` | ✓ |
| `estransforms::optionalchain::tests::non_simple_receiver_hoists_temp` | estransforms | **6h** 非简单 receiver → hoist temp（求值一次）| `f()?.b;` → `var _a;` + `(_a = f()) === null \|\| _a === void 0 ? void 0 : _a.b;` | `visitOptionalExpression`(`AddVariableDeclaration`+`NewAssignmentExpression`) | ✓ |
| `estransforms::optionalchain::tests::multiple_optional_links_nest_guards` | estransforms | **6h** 多 `?.`（链嵌套守卫）| `a?.b?.c;` → `var _a;` + `(_a = a === null \|\| a === void 0 ? void 0 : a.b) === null \|\| _a === void 0 ? void 0 : _a.c;` | `visitOptionalExpression`(receiver 递归 + temp) | ✓ |
| `estransforms::optionalchain::tests::non_simple_receiver_in_nested_chain_hoists_two_temps` | estransforms | **6h** 泛化：非简单 receiver + 嵌套 → 每链一 temp（内先 `_a`、外 `_b`）| `f()?.b?.c;` → `var _a, _b;` + `(_b = (_a = f()) === null \|\| _a === void 0 ? void 0 : _a.b) === null \|\| _b === void 0 ? void 0 : _b.c;` | `visitOptionalExpression` | ✓ |
| `estransforms::optionalchain::tests::non_simple_receiver_inside_function_body_hoists_into_body` | estransforms | **6i** tracer：函数声明体 per-scope var-env，temp 落体内非模块顶 | `function f() { return g()?.b; }` → `function f() { var _a; return (_a = g()) === null \|\| _a === void 0 ? void 0 : _a.b; }` | `VisitFunctionBody` | ✓ |
| `estransforms::optionalchain::tests::temp_in_arrow_concise_body_wraps_into_block` | estransforms | **6i** 箭头简明体：hoist temp 时包成 block | `function f() { return () => g()?.b; }` → `… () => { var _a; return (_a = g()) === null \|\| … : _a.b; }; }` | `VisitFunctionBody`（非 block 分支）| ✓ |
| `estransforms::optionalchain::tests::nested_function_bodies_hoist_into_their_own_scopes` | estransforms | **6i** 嵌套函数：内/外 temp 各落最近作用域（各 `_a`）| `function outer() { g()?.b; function inner() { return h()?.c; } }` → 两体各 `var _a;` | `VisitFunctionBody`（递归）| ✓ |
| `estransforms::optionalchain::tests::temp_inside_function_expression_body_hoists_into_body` | estransforms | **6i** 函数表达式体 per-scope var-env | `function f() { return function () { return g()?.b; }; }` → 内函数体 `var _a;` | `VisitFunctionBody` | ✓ |
| `estransforms::optionalchain::tests::temp_inside_method_body_hoists_into_method` | estransforms | **6i** 方法体 per-scope var-env（经 class members 线程化）| `class C { m() { return g()?.b; } }` → `m` 体内 `var _a;` | `VisitFunctionBody` | ✓ |
| `estransforms::objectrestspread::tests::object_spread_only_lowers_to_assign` | estransforms | **6d** 仅 spread | `const o = { ...x };` → `Object.assign({}, x)` | `visitObjectLiteralExpression` | ✓ |
| `estransforms::objectrestspread::tests::spread_then_property_chunks_pairwise` | estransforms | **6d** spread+属性 pairwise | `{ ...x, y }` → `Object.assign(Object.assign({}, x), { y })` | `chunkObjectLiteralElements` | ✓ |
| `estransforms::objectrestspread::tests::property_then_spread_uses_chunk_as_target` | estransforms | **6d** 首 chunk 作 target | `{ a, ...x }` → `Object.assign({ a }, x)` | `chunkObjectLiteralElements` | ✓ |
| `estransforms::objectrestspread::tests::object_rest_binding_lowers_to_rest_helper` | estransforms | **6g** tracer：对象 rest 绑定 → `__rest`（+ prologue helper）| `var { ...rest } = o;` → `var __rest = …;` + `var rest = __rest(o, []);` | `flattenObjectBindingOrAssignmentPattern`(rest 臂)/`NewRestHelper` | ✓ |
| `estransforms::objectrestspread::tests::leading_binding_is_excluded_from_rest_keys` | estransforms | **6g** leading 绑定保留对象模式 + 排除 key | `var { a, ...rest } = o;` → `var { a } = o, rest = __rest(o, ["a"]);` | `flattenObjectBindingOrAssignmentPattern` | ✓ |
| `estransforms::objectrestspread::tests::multiple_leading_bindings_list_all_rest_keys` | estransforms | **6g** 多 key（源序）| `var { a, b, ...rest } = o;` → `var { a, b } = o, rest = __rest(o, ["a", "b"]);` | `NewRestHelper` | ✓ |
| `estransforms::objectrestspread::tests::const_declaration_kind_is_preserved` | estransforms | **6g** 保留 `const`/`let` 声明种类 | `const { x, ...rest } = o;` → `const { x } = o, rest = __rest(o, ["x"]);` | `flattenDestructuringBinding`(list flags) | ✓ |
| `estransforms::objectrestspread::tests::renamed_leading_binding_excludes_property_key` | estransforms | **6g** 重命名绑定排除**属性**键 | `var { a: b, ...rest } = o;` → `var { a: b } = o, rest = __rest(o, ["a"]);` | `TryGetPropertyNameOfBindingOrAssignmentElement` | ✓ |
| `estransforms::objectrestspread::tests::non_simple_initializer_with_leading_binding_is_left_unchanged` | estransforms | **6g** DEFER 守卫：非简单 init（需 var-hoist temp）保持不变 | `var { a, ...rest } = f();` → 不变 | `ensureIdentifier`(hoist DEFER) | ✓ |
| `estransforms::namedevaluation::tests::anonymous_function_binding_gets_set_function_name` | estransforms | **6d-2** 匿名函数命名（emit-helper 基建端到端验证）| `var f = function () {};` → prologue `var __setFunctionName = …;` + `var f = __setFunctionName(function () { }, "f");` | `transformNamedEvaluationOfVariableDeclaration` | ✓ |
| `estransforms::r#async::tests::async_function_lowers_to_awaiter_wrapper` | estransforms | **6d-3** async 函数 → `__awaiter` 包装 + `await`→`yield` | `async function f() { await g(); }` → prologue `var __awaiter = …;` + `function f() { return __awaiter(this, void 0, void 0, function* () { yield g(); }); }` | `visitFunctionDeclaration`/`transformAsyncFunctionBody` | ✓ |
| `estransforms::r#async::tests::async_function_without_await_still_wraps` | estransforms | **6d-3** 无 await 仍包装 | `async function f() { g(); }` → `… function* () { g(); }` | `visitFunctionDeclaration` | ✓ |
| `estransforms::r#async::tests::async_generator_is_left_unchanged` | estransforms | **6d-3** async 生成器守卫（保持不变）| `async function* g() { yield 1; }` → 不变 | `FunctionFlagsGenerator` 守卫 | ✓ |
| `estransforms::r#async::tests::async_function_expression_lowers_to_awaiter_wrapper` | estransforms | **6m** async 函数表达式 → `__awaiter` 包装（首参 `this`）| `const f = async function () { await x; };` → `const f = function () { return __awaiter(this, void 0, void 0, function* () { yield x; }); };` | `visitFunctionExpression`/`transformAsyncFunctionBody` | ✓ |
| `estransforms::r#async::tests::async_method_lowers_to_awaiter_wrapper` | estransforms | **6m** async 方法 → `__awaiter` 包装（首参 `this`）| `class C { async m() { await x; } }` → `class C {\n    m() { return __awaiter(this, void 0, void 0, function* () { yield x; }); }\n}` | `visitMethodDeclaration`/`transformAsyncFunctionBody` | ✓ |
| `estransforms::r#async::tests::async_arrow_lowers_to_awaiter_wrapper_with_lexical_this` | estransforms | **6m** async 箭头 → 简明体 `__awaiter` 调用（lexical-this，顶层首参 `void 0`）| `const f = async () => { await x; };` → `const f = () => __awaiter(void 0, void 0, void 0, function* () { yield x; });` | `visitArrowFunction`/`transformAsyncFunctionBody` | ✓ |
| `estransforms::forawait::tests::async_generator_function_lowers_to_async_generator_wrapper` | estransforms | **6y** async 生成器函数声明 → `__asyncGenerator` 包装（tracer；`await x`→`yield __await(x)`、`yield y`→`yield yield __await(y)`，inner `function* g_1`，helper prologue）| `async function* g() { await x; yield y; }` → prologue `var __await = …; var __asyncGenerator = …;` + `function g() { return __asyncGenerator(this, arguments, function* g_1() { yield __await(x); yield yield __await(y); }); }` | `visitFunctionDeclaration`/`transformAsyncGeneratorFunctionBody` | ✓ |
| `estransforms::forawait::tests::async_generator_yield_delegate_uses_async_delegator` | estransforms | **6y** `yield*` 委托 → `__asyncDelegator`/`__asyncValues`（helper 序 asyncValues,await,asyncDelegator,asyncGenerator，tsc 对拍）| `async function* a() { yield* y; }` → `… function* a_1() { yield __await(yield* __asyncDelegator(__asyncValues(y))); }` | `visitYieldExpression`(asterisk) | ✓ |
| `estransforms::forawait::tests::async_generator_return_awaits_value` | estransforms | **6y** `return e` → `return yield __await(e)` | `async function* b() { return y; }` → `… function* b_1() { return yield __await(y); }` | `visitReturnStatement` | ✓ |
| `estransforms::forawait::tests::async_generator_bare_yield_uses_void_zero` | estransforms | **6y** bare `yield` → `yield yield __await(void 0)` | `async function* c() { yield; }` → `… function* c_1() { yield yield __await(void 0); }` | `visitYieldExpression`(no expr → `NewVoidZeroExpression`) | ✓ |
| `estransforms::forawait::tests::async_generator_method_is_left_unchanged` | estransforms | **6y** DEFER 守卫：async 生成器**方法**本轮不降级（不误转）| `class C { async *m() { await x; } }` → `class C {\n    async *m() { await x; }\n}` | `visitMethodDeclaration`(async-generator) DEFER | ✓ |
| `estransforms::forawait::tests::for_await_of_lowers_to_async_iteration_scaffold` | estransforms | **6z** `for await (x of y)` downlevel（tracer；`__asyncValues` 迭代器 + result temp + `for` 条件 `result = await iterator.next(), done = result.done, !done` + `result.value` 绑定 + `try/catch/finally` `iterator.return` 清理；非 identifier 源 `gen()` → 干净 `_e`/`_f`；tsc `--target es2017` 对拍）| `async function f() { for await (const x of gen()) {} }` → prologue `var __asyncValues = …;` + `async function f() {\n    var _a, e_1, _b, _c;\n    try {\n        for (var _d = true, _e = __asyncValues(gen()), _f; _f = await _e.next(), _a = _f.done, !_a; _d = true) {\n            _c = _f.value;\n            _d = false;\n            const x = _c;\n        }\n    }\n    catch (e_2) { e_1 = { error: e_2 }; }\n    finally {\n        try {\n            if (!_d && !_a && (_b = _e.return)) await _b.call(_e);\n        }\n        finally { if (e_1) throw e_1.error; }\n    }\n}`（catch 变量 `e_2`：Rust printer raw-text 名生成偏离 tsc `e_1_1`，cosmetic）| `transformForAwaitOfStatement`/`convertForOfStatementHead` | ✓ |
| `estransforms::forawait::tests::for_await_of_body_statements_follow_the_binding` | estransforms | **6z** body 语句拼在 `const x = _c` 后（block-splice）| `async function f() { for await (const x of gen()) { use(x); } }` → `… const x = _c;\n            use(x);\n …` | `convertForOfStatementHead`(block statements splice) | ✓ |
| `estransforms::forawait::tests::for_await_of_existing_variable_target_binds_with_assignment` | estransforms | **6z** existing-variable target（非声明）→ plain assignment `x = _c;` | `async function f() { for await (x of gen()) {} }` → `… _c = _f.value;\n            _d = false;\n            x = _c;\n …` | `CreateForOfBindingStatement`(非 VariableDeclarationList) | ✓ |
| `estransforms::spread::tests::array_spread_then_element_lowers_to_spread_array_segments` | estransforms | **6aa tracer**：数组字面量 spread → 嵌套 `__spreadArray` 段（spread 段 `pack=true`、字面量段 `pack=false`，起始 `[]`，helper prologue；tsc `--target es5` 对拍）| `[...a, b];` → `var __spreadArray = …;` + `__spreadArray(__spreadArray([], a, true), [b], false);` | `transformAndSpreadElements`(array) | ✓ |
| `estransforms::spread::tests::single_array_spread_folds_into_spread_array` | estransforms | **6aa** coverage：单 spread 段仍折叠（数组无 single-segment 捷径，仅 arg-list 有）| `const c = [...a];` → `const c = __spreadArray([], a, true);` | 同上 | ✓ |
| `estransforms::spread::tests::leading_literal_array_spread_starts_accumulator_at_first_segment` | estransforms | **6aa** coverage：前导非 spread → 累加器起于首字面量段 `[1]`（`starts_with_spread=false` 路径）| `[1, ...a, 2];` → `__spreadArray(__spreadArray([1], a, true), [2], false);` | 同上 | ✓ |
| `estransforms::spread::tests::call_with_single_spread_argument_lowers_to_apply` | estransforms | **6aa tracer**：调用参 spread → `.apply`（标识符 callee → `void 0` 受体；单 spread 捷径直接传参，无 helper）| `f(...args);` → `f.apply(void 0, args);` | `visitCallExpression` | ✓ |
| `estransforms::spread::tests::call_with_leading_argument_and_spread_folds_into_spread_array` | estransforms | **6aa** coverage：arg-list 多段折叠（spread 段 `pack=false`，区别于数组的 `true`）| `f(a, ...args);` → `f.apply(void 0, __spreadArray([a], args, false));` | `transformAndSpreadElements`(arg-list) | ✓ |
| `estransforms::spread::tests::member_call_with_spread_captures_receiver_as_this` | estransforms | **6aa** 成员调用 this-capture（简单标识符受体复用为 `apply` `this`，无 temp）| `o.m(...args);` → `o.m.apply(o, args);` | `visitCallExpression`(member) | ✓ |
| `estransforms::spread::tests::new_expression_spread_is_left_unchanged` | estransforms | **6aa** DEFER 守卫：`new C(...args)`（需 construct + `bind.apply`）保持不变 | `new C(...args);` → 原样 | `visitNewExpression` DEFER | ✓ |
| `estransforms::spread::tests::non_simple_member_receiver_call_spread_is_left_unchanged` | estransforms | **6aa** DEFER 守卫：非简单成员受体（`a.b.m`，需 capture temp）保持不变 | `a.b.m(...args);` → 原样 | `createCallBinding` temp-capture DEFER | ✓ |
| `moduletransforms::externalmoduleinfo::tests::named_import_is_an_external_import` | moduletransforms | **6e** 收集 external imports | `import { x } from "m";` → external_imports=1 | `collect` (KindImportDeclaration) | ✓ |
| `moduletransforms::externalmoduleinfo::tests::export_star_sets_flag_and_is_external_import` | moduletransforms | **6e** `export *` 标志 | `export * from "m";` → has_export_stars + external_imports=1 | `collect` (KindExportDeclaration) | ✓ |
| `moduletransforms::externalmoduleinfo::tests::local_named_export_records_exported_name` | moduletransforms | **6e** 本地命名导出名 | `export { x };` → exported_names=[x] | `addExportedNamesForExportDeclaration` | ✓ |
| `moduletransforms::externalmoduleinfo::tests::export_equals_is_recorded` | moduletransforms | **6e** `export =` | `export = x;` → export_equals=Some | `collect` (KindExportAssignment) | ✓ |
| `moduletransforms::externalmoduleinfo::tests::exported_const_records_exported_name` | moduletransforms | **6e** `export const` 导出名 | `export const y = 1;` → exported_names=[y] | `collectExportedVariableInfo` | ✓ |
| `jsxtransforms::jsx::tests::intrinsic_self_closing_element_lowers_to_create_element` | jsxtransforms | **6f** intrinsic 标签 → string-literal | `<div/>;` → `React.createElement("div", null);` | `visitJsxOpeningLikeElementCreateElement` | ✓ |
| `jsxtransforms::jsx::tests::component_self_closing_element_uses_identifier_tag` | jsxtransforms | **6f** 组件标签 → identifier | `<Foo/>;` → `React.createElement(Foo, null);` | `getTagName` | ✓ |
| `jsxtransforms::jsx::tests::string_attribute_becomes_props_object` | jsxtransforms | **6f** string 属性 → props 对象 | `<div id="x"/>;` → `…("div", { id: "x" });` | `transformJsxAttributeToObjectLiteralElement` | ✓ |
| `jsxtransforms::jsx::tests::expression_attribute_uses_inner_expression` | jsxtransforms | **6f** `{expr}` 属性 | `<div id={y}/>;` → `…("div", { id: y });` | `transformJsxAttributeInitializer` | ✓ |
| `jsxtransforms::jsx::tests::expression_child_becomes_trailing_argument` | jsxtransforms | **6f** 表达式子节点 | `<div>{x}</div>;` → `…("div", null, x);` | `transformJsxChildToExpression` | ✓ |
| `jsxtransforms::jsx::tests::text_child_becomes_string_literal` | jsxtransforms | **6f** 文本子节点 | `<div>hi</div>;` → `…("div", null, "hi");` | `visitJsxText` | ✓ |
| `jsxtransforms::jsx::tests::nested_element_child_is_lowered` | jsxtransforms | **6f** 嵌套元素子节点 | `<div><span/></div>;` → 递归 createElement | `transformJsxChildToExpression` | ✓ |
| `jsxtransforms::jsx::tests::fragment_lowers_to_react_fragment_create_element` | jsxtransforms | **6f** fragment | `<>{x}</>;` → `…(React.Fragment, null, x);` | `visitJsxOpeningFragmentCreateElement` | ✓ |
| `printer::emitcontext::node_substitution_round_trips` | printer（additive）| **6e-2 track1** 节点替换往返 | `set/get_node_substitution` | `EmitContext` substitution | ✓ |
| `printer::emitcontext::printer_emits_substituted_node` | printer（additive）| **6e-2 track1** emit 时替换 | 注册 `x`→`m_1.x`，emit `x;` → `m_1.x;` | `SubstituteNode`（emit hook）| ✓ |
| `moduletransforms::commonjsmodule::tests::named_import_and_use_lower_to_require_and_member_access` | moduletransforms | **6e-2 验证** CJS import+use | `import { x } from "m"; x;`（commonjs）→ `const m_1 = require("m");\nm_1.x;` | `transformCommonJSModule`+`visitIdentifier` | ✓ |
| `moduletransforms::commonjsmodule::tests::non_commonjs_module_kind_is_passthrough` | moduletransforms | **6e-2 track2** module 分支 | `module: none` → 原样 | compilerOptions gate | ✓ |
| `moduletransforms::commonjsmodule::tests::export_default_becomes_exports_default_with_marker` | moduletransforms | **6e-3** export default + marker | `export default 1;` → marker + `exports.default = 1;` | `visitExportAssignment`+`createUnderscoreUnderscoreESModule` | ✓ |
| `moduletransforms::commonjsmodule::tests::export_const_becomes_exports_assignment` | moduletransforms | **6e-3 + 6x** export const + void-0 | `export const y = 1;` → `exports.y = void 0;\nexports.y = 1;` | `transformInitializedVariable`+exportedNames init | ✓ |
| `moduletransforms::commonjsmodule::tests::local_named_export_becomes_exports_assignment` | moduletransforms | **6e-3 + 6x** export {x} + void-0 | `export { x };` → `exports.x = void 0;` + `exports.x = x;` | `appendExportsOfDeclaration`+exportedNames init | ✓ |
| `moduletransforms::commonjsmodule::tests::default_import_uses_import_default_helper` | moduletransforms | **6e-3** default import interop | `import d from "m"; d;` → `__importDefault` + `m_1.default` | `getHelperExpressionForImport` | ✓ |
| `moduletransforms::commonjsmodule::tests::namespace_import_uses_import_star_helper` | moduletransforms | **6e-3** namespace import interop | `import * as ns from "m"; ns;` → `__importStar` + `m_1` | `getHelperExpressionForImport` | ✓ |
| `moduletransforms::commonjsmodule::tests::export_star_uses_export_star_helper` | moduletransforms | **6e-3** export * | `export * from "m";` → `__exportStar(require("m"), exports);` | `NewExportStarHelper` | ✓ |
| `moduletransforms::commonjsmodule::tests::export_equals_becomes_module_exports_without_marker` | moduletransforms | **6w tracer** `export =` → `module.exports` | `export = x;`（commonjs）→ `module.exports = x;`（无 `__esModule`）| `appendExportEqualsIfNeeded`/`visitExportEquals` | ✓ |
| `moduletransforms::commonjsmodule::tests::exported_function_declaration_keeps_decl_and_assigns_export` | moduletransforms | **6w** 导出函数声明 | `export function f() {}`（commonjs）→ marker + `exports.f = f;` + `function f() { }`（赋值 hoist 在前）| `visitTopLevelFunctionDeclaration`+`appendExportsOfClassOrFunctionDeclaration` | ✓ |
| `moduletransforms::commonjsmodule::tests::exported_class_declaration_keeps_decl_and_assigns_export` | moduletransforms | **6w + 6x** 导出类声明 + void-0 | `export class C {}`（commonjs）→ marker + `exports.C = void 0;` + `class C {\n}` + `exports.C = C;`（类名进 exportedNames；函数声明不进）| `visitTopLevelClassDeclaration`+exportedNames init | ✓ |
| `moduletransforms::commonjsmodule::tests::exported_default_function_declaration_assigns_default_export` | moduletransforms | **6w coverage** 默认导出函数 | `export default function f() {}`（commonjs）→ `exports.default = f;` + `function f() { }` | `appendExportsOfClassOrFunctionDeclaration`（default）| ✓ |
| `moduletransforms::commonjsmodule::tests::exported_default_class_declaration_assigns_default_export` | moduletransforms | **6w coverage** 默认导出类 | `export default class C {}`（commonjs）→ `class C {\n}` + `exports.default = C;` | `appendExportsOfClassOrFunctionDeclaration`（default）| ✓ |
| `moduletransforms::commonjsmodule::tests::import_equals_require_lowers_to_const_require` | moduletransforms | **6x tracer** `import =` → `const require` | `import x = require("m"); x;`（commonjs）→ `const x = require("m");\nx;` | `visitTopLevelImportEqualsDeclaration` | ✓ |
| `moduletransforms::commonjsmodule::tests::multiple_exported_names_share_chained_void_zero_init` | moduletransforms | **6x** void-0 链（逆序） | `export const a; export const b;`（commonjs）→ `exports.b = exports.a = void 0;` + 各赋值 | `transformCommonJSModule`（exportedNames chunk）| ✓ |
| `moduletransforms::commonjsmodule::tests::dynamic_import_lowers_to_promise_resolve_then_import_star_require` | moduletransforms | **6ad tracer** 动态 `import("m")` 降级（合成 AST，解析器 DEFER call-head）| `const p = import("m");`（commonjs）→ `const p = Promise.resolve().then(() => __importStar(require("m")));` + `var __importStar = …;` prologue（Go **无条件** importStar 包裹）| `visitImportCallExpression`/`createImportCallExpressionCommonJS` | ✓ |
| `moduletransforms::commonjsmodule::tests::no_argument_dynamic_import_lowers_to_require_with_no_args` | moduletransforms | **6ad** 无参 `import()` 边界 | `const p = import();`（commonjs）→ `const p = Promise.resolve().then(() => __importStar(require()));` | `createImportCallExpressionCommonJS`（`arg == nil` → 空参 `require()`）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_named_import_use_rewrites_to_member_access` | moduletransforms | **6ai tracer**（genuine RED→GREEN）：作用域正确 named-import use 重写（消费 4an `resolve_reference`，经 `new_common_js_module_transformer_with_resolver`）| `import { x } from "m";\nx;`（commonjs）→ `const m_1 = require("m");\nm_1.x;` | `visitExpressionIdentifier`/`GetReferencedImportDeclaration`（按解析 symbol）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_shadowed_use_is_not_rewritten` | moduletransforms | **6ai headline scope guard**（genuine RED→GREEN）：内层 shadow `var x` 的 use 解析到局部、**不**重写（按名匹配会误写 `m_1.x`）| `import { x } from "m";\nfunction f() {\n    var x = 1;\n    x;\n}`（commonjs）→ `const m_1 = require("m");\nfunction f() {\n    var x = 1;\n    x;\n}` | `resolve_reference`（内层 `x`→局部 symbol，不命中 import）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_import_use_inside_call_argument_is_rewritten` | moduletransforms | **6ai** coverage：调用参内 use 重写（`console`/`log` 不命中 import）| `import { x } from "m";\nconsole.log(x);`（commonjs）→ `const m_1 = require("m");\nconsole.log(m_1.x);` | `resolve_reference`（嵌套表达式）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_default_import_use_rewrites_to_default_member` | moduletransforms | **6aj** slice1：作用域正确 default-import use 重写（经 resolver，test-first 观察 GREEN——6ai 泛化臂已覆盖）| `import d from "m";\nd;`（commonjs）→ `…__importDefault…\nconst m_1 = __importDefault(require("m"));\nm_1.default;` | `substituteExpressionIdentifier`（default→`.default`，`decl=ImportClause` symbol 匹配）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_namespace_import_use_rewrites_to_bare_alias` | moduletransforms | **6aj** slice2：作用域正确 namespace-import use 重写（经 resolver）| `import * as ns from "m";\nns;`（commonjs）→ `const m_1 = __importStar(require("m"));\nm_1;` | `substituteExpressionIdentifier`（namespace→裸 alias，`decl=NamespaceImport` symbol 匹配）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_shadowed_default_import_use_is_not_rewritten` | moduletransforms | **6aj** slice3 headline scope guard（default 形）：内层 shadow `var d` 解析到局部、**不**重写（按名匹配会误写 `m_1.default`）| `import d from "m";\nfunction f() {\n    var d = 1;\n    d;\n}`（commonjs）→ import 仍降级、内层 `d` 保持裸 | `resolve_reference`（内层 `d`→局部 symbol，不命中 import）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_shadowed_namespace_import_use_is_not_rewritten` | moduletransforms | **6aj** scope guard 镜像（namespace 形）：内层 shadow `var ns` 解析到局部、保持裸 | `import * as ns from "m";\nfunction f() {\n    var ns = 1;\n    ns;\n}`（commonjs）→ import 仍 `__importStar` 降级、内层 `ns` 保持裸 | `resolve_reference`（内层 `ns`→局部 symbol，不命中 import）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_exported_variable_use_rewrites_to_exports_access` | moduletransforms | **6ak tracer**（genuine RED→GREEN）：顶层导出变量 use 重写为 `exports.<name>`（消费 4as `get_referenced_export_container`，`prefix_locals=false`）；声明降级 6e/6w 不回归 | `export const x = 1;\nx;`（commonjs+resolver）→ `Object.defineProperty(exports, "__esModule", { value: true });\nexports.x = void 0;\nexports.x = 1;\nexports.x;` | `visitExpressionIdentifier`/`GetReferencedExportContainer`→SourceFile→`exports.x` | ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_export_use_shadowed_by_inner_local_is_not_rewritten` | moduletransforms | **6ak headline scope guard**（green-on-arrival）：内层 shadow `const x` 的 use 解析到局部、container=None、**不**前缀（按名匹配会误写 `exports.x`）；仅声明降级 | `export const x = 1;\nfunction f() {\n    const x = 2;\n    x;\n}`（commonjs+resolver）→ `…exports.x = void 0;\nexports.x = 1;\nfunction f() {\n    const x = 2;\n    x;\n}` | `get_referenced_export_container`（内层 `x`→局部、None）| ✓ |
| `moduletransforms::commonjsmodule::tests::scoped_non_exported_local_use_stays_bare` | moduletransforms | **6ak non-export guard**（green-on-arrival）：非导出顶层 local 的 use 无 export container、保持裸；无 `__esModule` marker | `const y = 1;\ny;`（commonjs+resolver）→ `const y = 1;\ny;` | `get_referenced_export_container`（非导出→None）| ✓ |
| `moduletransforms::estransforms::usestrict::tests::commonjs_module_gains_use_strict_prologue` | estransforms | **6x tracer** `"use strict"` 前插 | `export const y = 1;`（commonjs）→ `"use strict";\nexport const y = 1;` | `useStrictTransformer.visitSourceFile`+`EnsureUseStrict` | ✓ |
| `moduletransforms::estransforms::usestrict::tests::existing_use_strict_prologue_is_not_duplicated` | estransforms | **6x** dedup | `"use strict"; var x = 1;`（commonjs）→ 不重复 | `EnsureUseStrict`（短路）| ✓ |
| `moduletransforms::estransforms::usestrict::tests::esm_external_module_skips_use_strict` | estransforms | **6x** ESM-emit 跳过 | `export const y = 1;`（esnext）→ 原样（无 `"use strict"`）| `visitSourceFile`（ESM skip）| ✓ |
| `moduletransforms::esmodule::tests::value_import_and_use_preserved_under_esnext` | moduletransforms | **6u tracer** ESM value import 透传 | `import { x } from "m"; x;`（esnext）→ `import { x } from "m";\nx;` | `NewESModuleTransformer`/`visitSourceFile` | ✓ |
| `moduletransforms::esmodule::tests::export_equals_is_elided_and_empty_imports_appended` | moduletransforms | **6u** `export =` elision + empty imports | `export = x;`（esnext）→ `export {};` | `visitExportAssignment`+`createEmptyImports` | ✓ |
| `moduletransforms::esmodule::tests::import_equals_require_is_elided_and_empty_imports_appended` | moduletransforms | **6u** `import =` elision + empty imports | `import x = require("m");`（esnext）→ `export {};` | `visitImportEqualsDeclaration`（`< Node16`）| ✓ |
| `moduletransforms::esmodule::tests::export_default_is_preserved` | moduletransforms | **6u** export default 透传 | `export default 1;`（esnext）→ `export default 1;` | `visitExportAssignment`（非 export=）| ✓ |
| `moduletransforms::esmodule::tests::local_named_export_is_preserved` | moduletransforms | **6u** 本地 `export {x}` 透传 | `const x = 1; export { x };`（esnext）→ `const x = 1;\nexport { x };` | `visitExportDeclaration`（ModuleSpecifier nil）| ✓ |
| `moduletransforms::esmodule::tests::export_star_is_preserved` | moduletransforms | **6u** `export *` 透传 | `export * from "m";`（esnext）→ `export * from "m";` | `visitExportDeclaration`（Module > ES2015）| ✓ |
| `moduletransforms::esmodule::tests::re_export_is_preserved` | moduletransforms | **6u** `export {x} from` 透传 | `export { x } from "m";`（esnext）→ `export { x } from "m";` | `visitExportDeclaration`（re-export）| ✓ |
| `moduletransforms::esmodule::tests::namespace_import_is_preserved` | moduletransforms | **6u** `import * as ns` 透传 | `import * as ns from "m"; ns;`（esnext）→ `import * as ns from "m";\nns;` | `visitImportDeclaration`（RewriteRelativeImportExtensions off）| ✓ |
| `moduletransforms::esmodule::tests::non_module_file_is_passthrough` | moduletransforms | **6u** 非模块守卫 | `const x = 1;`（esnext，无指示器）→ `const x = 1;`（无 spurious `export {};`）| `visitSourceFile` guard | ✓ |
| `moduletransforms::esmodule::tests::value_import_and_use_preserved_under_es2015` | moduletransforms | **6u** es2015 同样透传 | `import { x } from "m"; x;`（es2015）→ `import { x } from "m";\nx;` | `visitSourceFile` | ✓ |
| `moduletransforms::esmodule::tests::namespace_reexport_rewrites_to_import_and_named_export_under_es2015` | moduletransforms | **6ab tracer**：es2015 `export * as ns` → namespace import + named re-export（`new_generated_name_for_node(ns)`→`ns_1`）| `export * as ns from "m";`（es2015）→ `import * as ns_1 from "m";\nexport { ns_1 as ns };` | `visitExportDeclaration`（`Module <= ES2015` + `IsNamespaceExport`）| ✓ |
| `moduletransforms::esmodule::tests::namespace_reexport_is_preserved_under_esnext` | moduletransforms | **6ab guard**：esnext 合法故透传（改写门控于 `Module <= ES2015`）| `export * as ns from "m";`（esnext）→ 原样 | `visitExportDeclaration`（`Module > ES2015` → preserve）| ✓ |
| `moduletransforms::esmodule::tests::namespace_reexport_as_default_rewrites_to_export_default_under_es2015` | moduletransforms | **6ab coverage**：名为 `default` → `export default <gen>`（`IsExportNamespaceAsDefaultDeclaration`）| `export * as default from "m";`（es2015）→ `import * as default_1 from "m";\nexport default default_1;` | `visitExportDeclaration`（`NewExportAssignment` 分支）| ✓ |
| `moduletransforms::impliedmodule::tests::commonjs_source_delegates_to_common_js_transform` | moduletransforms | **6ac tracer**：`module: commonjs` 委托 CJS transform | `export default 1;`（commonjs）→ `Object.defineProperty(exports, "__esModule", { value: true });\nexports.default = 1;`（= CJS transform 输出）| `impliedmodule.go:visitSourceFile`（format < ES2015 → NewCommonJSModuleTransformer）| ✓ |
| `moduletransforms::impliedmodule::tests::esnext_source_delegates_to_es_module_transform` | moduletransforms | **6ac**：`module: esnext` 委托 ESM transform | `export = x;`（esnext）→ `export {};`（= ESM transform 输出，`export =` elide + createEmptyImports）| `impliedmodule.go:visitSourceFile`（format >= ES2015 → NewESModuleTransformer）| ✓ |
| `moduletransforms::impliedmodule::tests::es2015_source_routes_to_es_module_transform` | moduletransforms | **6ac 谓词边界**：`module: es2015` 也属 ES 目标（`>= ES2015`，非 `== EsNext`）→ ESM | `export = x;`（es2015）→ `export {};` | `impliedmodule.go:visitSourceFile`（`format >= core.ModuleKindES2015`）| ✓ |
| `moduletransforms::impliedmodule::tests::declaration_file_is_returned_unchanged` | moduletransforms | **6ac 守卫**：`.d.ts` → 原样返回（跳过分派）| `export = x;`（`/main.d.ts`，commonjs）→ `export = x;`（无守卫则 CJS 降级为 `module.exports = x;`）| `impliedmodule.go:visitSourceFile`（`node.IsDeclarationFile` → return node）| ✓ |
| `jsxtransforms::jsx::tests::automatic_runtime_self_closing_element_lowers_to_jsx_call` | jsxtransforms | **6e-3** JSX automatic 选择 | `<div/>`（jsx=react-jsx）→ `jsx("div", {})` | `getJsxFactoryCallee` | ✓ |
| `tstransforms::runtimesyntax::tests::auto_numbered_enum_lowers_to_iife` | tstransforms | **6n tracer** 自动编号 enum → IIFE（数字反向映射）| `enum E { A, B }` → `var E;\n(function (E) {\n    E[E["A"] = 0] = "A";\n    E[E["B"] = 1] = "B";\n})(E \|\| (E = {}));` | `visitEnumDeclaration`/`transformEnumMember` | ✓ |
| `tstransforms::runtimesyntax::tests::explicit_numeric_initializer_sets_value_and_continues_autonumber` | tstransforms | **6n** 显式数字初值 + 自增续接 | `enum E { A = 5, B }` → `E[E["A"] = 5] = "A";\n    E[E["B"] = 6] = "B";` | `transformEnumMember`（句法求值替代 `GetEnumMemberValue`）| ✓ |
| `tstransforms::runtimesyntax::tests::string_initialized_member_omits_reverse_mapping` | tstransforms | **6n** 字符串成员无反向映射 | `enum E { X = "v" }` → `E["X"] = "v";` | `transformEnumMember`（`useExplicitReverseMapping=false`）| ✓ |
| `tstransforms::runtimesyntax::tests::const_enum_is_omitted` | tstransforms | **6n** const enum 省略（inlining DEFER）| `const enum E { A }` → `（空）` | `shouldEmitEnumDeclaration` | ✓ |
| `tstransforms::runtimesyntax::tests::instantiated_namespace_lowers_to_iife` | tstransforms | **6n tracer** 实例化 namespace → IIFE（`N.x = 1`）| `namespace N { export const x = 1; }` → `var N;\n(function (N) {\n    N.x = 1;\n})(N \|\| (N = {}));` | `visitModuleDeclaration`/`transformModuleBody`/`visitVariableStatement` | ✓ |
| `tstransforms::runtimesyntax::tests::uninstantiated_namespace_is_omitted` | tstransforms | **6n** 未实例化（type-only）namespace 省略 | `namespace N { interface I {} }` → `（空）` | `shouldEmitModuleDeclaration`/`IsInstantiatedModule` | ✓ |
| `tstransforms::legacydecorators::tests::instance_property_decorator_lowers_to_decorate_call` | tstransforms | **6al tracer**（genuine RED→GREEN）：实例属性装饰器 → 尾随 `__decorate(...)`（无 metadata；属性装饰器+类型被剥；helper prologue）| `class C { @dec x: number; }`（experimentalDecorators）→ `<__decorate>\nclass C {\n    x;\n}\n__decorate([dec], C.prototype, "x", void 0);` | `generateClassElementDecorationExpression`/`NewDecorateHelper` | ✓ |
| `tstransforms::legacydecorators::tests::property_decorator_emits_design_type_metadata` | tstransforms | **6al headline**（genuine RED→GREEN，**消费 checker 4at**）：`--emitDecoratorMetadata` 追加 `design:type` 元数据（`Number` 来自 `serialize_type_node_for_metadata`，metadata 居末）| `class C { @dec x: number; }`（+emitDecoratorMetadata）→ `…\n__decorate([dec, __metadata("design:type", Number)], C.prototype, "x", void 0);`（`__decorate`(2)+`__metadata`(3) prologue）| `injectClassElementTypeMetadata`/`serializeTypeNode`/`NewMetadataHelper` | ✓ |
| `tstransforms::legacydecorators::tests::static_property_decorator_uses_class_name_prefix` | tstransforms | **6al**（genuine RED→GREEN）：static 成员前缀 `C`（非 `C.prototype`），保留 `static` 修饰 | `class C { @dec static x: number; }` → `…\n__decorate([dec, __metadata("design:type", Number)], C, "x", void 0);` | `getClassMemberPrefix`（`IsStatic`）| ✓ |
| `tstransforms::legacydecorators::tests::string_typed_property_serializes_to_string_constructor` | tstransforms | **6al** coverage（green-on-arrival，泛化守卫）：`: string` → `String`（证 serializer 非硬编码 `Number`）| `class C { @dec x: string; }` → `…__metadata("design:type", String)…` | `serializeTypeNode`（KindStringKeyword）| ✓ |
| `tstransforms::legacydecorators::tests::type_reference_property_serializes_to_object_fallback` | tstransforms | **6al** DEFER 守卫（green-on-arrival）：`TypeReference` `: D` → `Object`（checker 4at 把 TypeReference 臂 DEFER 到 `Object` 尾）| `class C { @dec x: D; }` → `…__metadata("design:type", Object)…` | `serializeTypeNode` 尾（4at DEFER）| ✓ |
| `tstransforms::legacydecorators::tests::without_experimental_decorators_class_is_unchanged` | tstransforms | **6al** gate（green-on-arrival）：无 `--experimentalDecorators` → 装饰类原样透传（无 `__decorate`）| `class C { @dec x: number; }`（flag off）→ `class C {\n    @dec\n    x: number;\n}` | `visit`（option gate）| ✓ |
| `tstransforms::legacydecorators::tests::class_decorator_is_left_unchanged` | tstransforms | **6al** DEFER 守卫（green-on-arrival）：class 装饰器 `@dec class C {}` 暂不降级（`let C = …; C = __decorate(...)` 包裹 DEFER），原样透传 | `@dec class C {}` → `@dec\nclass C {\n}` | `transformClassDeclarationWithClassDecorators`（DEFER）| ✓ |
| `printer::emithelpers::requested_helper_definition_emitted_in_prologue` | printer（additive）| **6d-2** prologue emit helper 定义 | 挂 `__setFunctionName` → 源文件顶部出现其定义 | `emitHelpers` | ✓ |
| `printer::emithelpers::prologue_emits_helpers_in_priority_order` | printer（additive）| **6d-2** 优先级排序 | `__awaiter`(5) 先于 `__setFunctionName`(None) | `compareEmitHelpers` | ✓ |
| `printer::emitcontext::requested_helpers_round_trip_and_dedup` | printer（additive）| **6d-2** request/read + 去重 | 双 request 记录一次；read 清空 | `RequestEmitHelper`/`ReadEmitHelpers` | ✓ |
| `printer::emitcontext::requested_helper_pulls_in_dependencies_first` | printer（additive）| **6d-2** 依赖递归 | `request(__importStar)` → deps 先记录 | `RequestEmitHelper` | ✓ |
| `es2016_exponentiation`（旧占位，已被上面替代） | estransforms | `a ** b` 降级（target ES2015） | `a ** b` → `Math.pow(a, b)` | `newExponentiationTransformer` | ✓ |
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
| runtimesyntax（enum） | `tests/cases/conformance/enums/**`（`enumBasics`、`constEnums`、`enumMerging`…） | enum → IIFE 运行时对象（`var E; (function(E){...})(E||(E={}))`） | **6n ✓ 子集**（自动编号 / 显式数字初值 / 字符串成员无反向映射 / const enum 省略）；DEFER：const enum 成员引用 inlining（checker 常量求值）、非字面量初值常量折叠、merged/exported enum、`E.A` 成员引用重写（resolver）+ P10 |
| runtimesyntax（namespace） | `tests/cases/conformance/**/namespaces/**`、`.../moduleResolution/**` 中的 `namespace`/`module` | 实例化 namespace → IIFE；未实例化省略 | **6n ✓ 子集**（实例化 `export const` → `N.x = 1` IIFE / 未实例化省略）；DEFER：嵌套/点名 namespace、merged namespace、`export =` 互操作、exported function/class/import=、binding-pattern export、resolver 式成员引用重写 + P10 |
| legacydecorators + metadata | `tests/cases/conformance/decorators/**`（`--experimentalDecorators --emitDecoratorMetadata`） | `__decorate`/`__metadata` helper 注入与设计类型序列化 | **6al ✓ 子集**（属性装饰器 instance/static → `__decorate([dec], C.prototype/C, "x", void 0)`；`design:type` 元数据消费 checker 4at `serialize_type_node_for_metadata`，`Number`/`String`/…/`Object`；`__decorate`/`__metadata` helper prologue）/ DEFER（class 装饰器 `let C=…;C=__decorate(...)` 包裹、method/accessor 装饰器（`design:type=Function`/`design:returntype`）、参数装饰器 `__param`+`design:paramtypes`、计算名、`TypeReference design:type`（4at→`Object`）、装饰器求值序）+ P10 |
| typeserializer | （随 metadata）`decorators/**/metadata/**` | 类型 → `Object`/`Function`/`Number`… 元数据表达式 | **6al ✓ 子集**（keyword 类型经 checker 4at `SerializedTypeNode`→AST：`number→Number`/`string→String`/`boolean→Boolean`/`bigint→BigInt`/`symbol→Symbol`/`void·undefined·never·null→void 0`/其余→`Object`）/ DEFER（`TypeReference`/`Date`/class → 构造器（4at DEFER→`Object`）、union/intersection/conditional、`FunctionType`→`Function`、`ArrayType`→`Array`、字面量类型臂）+ P10 |

> 这些 baseline 不替代每函数单测（PORTING §8.6）；它们是 stage 完整后的端到端验收。6b 仅落地 typeeraser 剥类型子集与 `constant_expression`，故标记为登记项。

## estransforms conformance 切片（P10 端到端兜底）

`estransforms` 各 stage 的字节级正确性由 **P10 conformance parity** 对拍（`tsc --target` baseline）。6c-1 单测覆盖 `exponentiation` 全量 + `classfields` 实例字段子集；下列子集是完整化后 P10 必须绿的目标：

| transform | conformance 子集 | 验证内容 | 目标轮 |
|---|---|---|---|
| exponentiation | `tests/cases/conformance/es2016/exponentiationOperator/**` | `**`/`**=`（含 element/property-access 目标 + 临时变量）→ `Math.pow` | 6c-1 ✓（标识符）/ 6c-3 ✓（顶层 property/element temp 目标）/ 6i ✓（函数声明体 per-scope var-env：`function f() { a.x **= b; }` → 体内 `var _a;`）/ DEFER（嵌套在控制流语句体/方法/函数表达式/箭头/嵌套类的 temp）+ P10 |
| classfields | `tests/cases/conformance/classes/members/privateNames/**`、`.../esnext/classFields/**`、`.../classes/members/instanceAndStaticMembers/**`、`useDefineForClassFields/**` | 实例/静态字段、私有名（WeakMap）、计算名、accessor、class-expr、`super` 交互、`--target`/`useDefineForClassFields` 门控 | 6c-1/2 ✓（实例字段 + 构造器插入族）/ 6c-3 ✓（static 字段）/ 6c-4 ✓（私有实例字段 WeakMap + 计算名）/ **6o ✓（ClassExpression 实例字段：表达式位降级，纯实例字段单节点子集）** / **6q ✓（`accessor` 自动访问器：实例 + static + class-expr，ES2022-native backing 私有字段 + get/set 重定向器，backing 名经 6p generated private name）** / **6r ✓（ClassExpression static 字段语句 hoist：逗号序列 + temp 包裹，复用变量环境 + 6p `new_temp_variable`；实例+static 混合可达）** / DEFER（named-helper 私有形态、accessor 的 WeakMap/下级 target 形态（二遍 result visitor）+ 计算/装饰 accessor、私有 static/方法（WeakSet）、class-expr **计算名/私有字段** 语句 hoist（需 Go `pendingExpressions` 内联）、嵌套作用域 class-expr static hoist、target 门控）+ P10 |
| optionalchain | `tests/cases/conformance/es2020/optionalChaining*/**` | `?.` 属性/元素/调用 + 链 → 保护性条件表达式 | 6d ✓（单 `?.` + 尾随段 + 简单 receiver）/ 6h ✓（顶层语句的 temp-hoist receiver `f()?.b` + 多 `?.` `a?.b?.c`）/ 6i ✓（per-scope var-env：函数声明/表达式、箭头体、类方法体内 temp-hoist）/ 6s ✓（`(a?.b)()` this-capture → `(… ? void 0 : a.b).call(a)`；`delete a?.b` → `… ? true : delete a.b`，含括号 `delete (a?.b)`；新增 `SyntheticReferenceExpression` AST 节点）/ 6t ✓（可选 call 段 `leftThisArg` 线程化：`a?.b?.()` → `(_a = … : a.b) === … ? void 0 : _a.call(a)`、`a.b?.()`（非可选 member receiver）→ `(_a = a.b) === … ? void 0 : _a.call(a)`、嵌套 `a?.b.c?.()` → `_b.call(_a)`；baseline callChain.2/.3/.js）/ DEFER（嵌套在控制流语句体/`switch` case/对象方法简写/构造器/accessor 的 temp-hoist、call 段 `super` receiver `super.b?.()`+super→this、tagged template）+ P10 |
| objectrestspread | `tests/cases/conformance/es2018/objectRestSpread*/**`、`.../es2017/**` | 对象 spread → `Object.assign`；对象 rest 绑定 → `__rest` | 6d ✓（对象字面量 spread 子集）/ 6g ✓（变量声明对象 rest 绑定子集：`var { a, ...rest } = o` → `var { a } = o, rest = __rest(o, ["a"])`，简单 init + 字面量键）/ DEFER（泛型 `FlattenDestructuringBinding`=6h：嵌套/数组模式、默认值、计算键、非简单 init temp-hoist；rest 在参数/`for-of`/`catch`/赋值模式）+ P10 |
| namedevaluation | `tests/cases/conformance/es2022/namedEvaluation/**`、decorators/classFields 中的命名求值 | 匿名函数/类绑定 → `__setFunctionName(value, name)` | 6d-2 ✓（`var f = 匿名函数` 子集，emit-helper 基建端到端验证）/ DEFER（property/参数/binding/计算名 `__propKey`/匿名类 static 块、AssignedName 跟踪）+ P10 |
| async | `tests/cases/conformance/es2017/asyncFunctions/**` | async 函数 → `__awaiter(this, void 0, void 0, function* () { … })` + `await`→`yield` | 6d-3 ✓（顶层 async 函数声明）+ 6m ✓（async 函数表达式/方法/箭头，箭头顶层首参 `void 0`）/ DEFER（accessor、async 生成器 `__asyncGenerator`、async 方法内 super、`asyncContextHasLexicalThis` 跨嵌套作用域线程化、lexical-`arguments`/参数、top-level await）+ P10 |
| forawait | `tests/cases/conformance/es2018/asyncGenerators/**` | async 生成器函数声明 → `__asyncGenerator` 包装（6y 已落地）；`for await (x of y)` → `__asyncValues` + downlevel-`await(iterator.next())` + `result.value` 绑定 + `iterator.return` 清理 try/catch/finally（**6z 已落地**，async 非生成器函数内，非 identifier 源，见上 3 行为级测试，tsc 对拍）。DEFER：identifier 源（printer resolving 名生成）、async 生成器内 `for await`（enclosing-flags 线程化）、destructuring/top-level/label、async 生成器方法/表达式/箭头（super/hierarchy-facts）| 6y+6z 子集 ✓；identifier 源/生成器内/方法/箭头 DEFER + P10 |
| spread | `tests/cases/conformance/es6/spread/**`、`.../es2015/spread/**`（`--target es5`）| 数组字面量 spread `[...a, b]` → `__spreadArray` 段；调用参 spread `f(...args)` → `f.apply(void 0, args)` / `o.m(...args)` → `o.m.apply(o, args)` | **6aa ✓ 子集**（数组：tracer + 单 spread + 前导字面量；调用：标识符 callee + 简单成员受体 + arg-list 多段；tsc `--target es5` 对拍，含 2 DEFER 守卫）。Go 无 ES2015 spread transform，ground truth = tsc。DEFER：`new C(...args)`（construct + `bind.apply`）、`super(...args)`、非简单成员受体 capture temp、`--downlevelIteration`（`__read`/`__spread`）、wiring 进 `GetESTransformer`（无 `NewES2015Transformer`）+ P10 |
| using | `tests/cases/conformance/esnext/usingDeclarations*/**` | `using x = e` → try/finally + `__addDisposableResource`/`__disposeResources` | DEFER（**parser 不解析语句级 `using`**；transform + helper 就绪，待 parser 轮）+ P10 |
| esdecorator | `tests/cases/conformance/esDecorators/**` | 标准（TC39）装饰器降级 + helper emit | DEFER（待 checker 元数据 + helper-emit）/P10 |

> 6c-1 仅落地 `exponentiation`（标识符目标）与 `classfields`（无 heritage/无既有构造器的实例字段）子集；6c-4 收口 `classfields` 可达面（私有实例字段直接 WeakMap 形态 + 计算实例字段名）；其余（named-helper 私有形态、accessor、class-expr、私有 static/方法、参数属性、target 门控）为登记项，详见 `impl.md` 的「classfields 移植状态」。

## moduletransforms conformance 切片（P10 端到端兜底）

| transform | conformance 子集 | 验证内容 | 目标轮 |
|---|---|---|---|
| externalmoduleinfo | （无独立 baseline；由 CJS/ESM 输出间接覆盖）| import/export 结构化收集 | 6e ✓（单测覆盖：imports/export*/export names/export=）|
| commonjsmodule | `tests/cases/compiler/**`、`.../es2015/modules/**`（`--module commonjs`）| `import`→`require`、`export`→`exports.x`、`__importStar`/`__importDefault`/`__exportStar` interop、`__esModule` 标志 | 6e-2 ✓（named import + use→`m_1.x`）/ 6e-3 ✓（export default/const/`{}`/`*` + default/namespace import interop + `__esModule` 标志）/ **6v ✓（combined default+named import `import d, { x } from "m"` → `__importStar`（Go `getImportNeedsImportStarHelper`）+ `d`→`m_1.default`/`x`→`m_1.x`；re-export `export { x } from "m"` / `export { a as b } from "m"` → `var m_1 = require("m")` + live-binding `Object.defineProperty(exports, …, { get })` getter）** / **6w ✓（`export = e` → `module.exports = e;`（抑制 `__esModule`）；导出函数声明 `export function f() {}` → `exports.f = f;`（hoist 在声明前）+ 保留本地 `function f() {}`；导出类声明 `export class C {}` → 保留 `class C {}` + `exports.C = C;`（在声明后）；`export default function f() {}`/`export default class C {}` 命名默认 → `exports.default = f`/`C`）** / **6x ✓（`import x = require("m")` → `const x = require("m");`；`exports.<name> = void 0` 导出名零初始化——`__esModule` 后、链式逆序 chunk(50)，含 `export const`/local·re-export named/非 default `export class`，排除函数/default/`export *`；`"use strict"` prologue 由独立 `estransforms/usestrict` transformer 负责——**Go ground-truth 修正**：briefing 误以为在 CJS transform 内）** / **6ad ✓（动态 `import("m")`/`import()` → `Promise.resolve().then(() => __importStar(require(...)))`——Go **无条件** importStar 包裹（非 esModuleInterop 门控）、arrow 回调、inlineable 参数内联 / no-arg → `require()`；合成 AST 验证，解析器 call-head DEFER）** / **6ai ✓（作用域正确 import use-site 重写：消费 4an `resolve_reference`，named-import use `x`→`m_1.x` 仅当真解析到 import；内层 shadow `var x` 不重写——按名匹配会误写；经 additive `new_common_js_module_transformer_with_resolver`，既有签名不变）** / DEFER（动态 import 的 `needSyncEval` 模板形（非 inlineable 参数）+ spread 参数 + 解析器 call-head（phase-3）、匿名 `export default function/class`（需合成名）、`export import x = require("m")`→`exports.x = require(...)`、Node16+ `import =`→同步 require、`export =`/`import =` 互斥、`export * as ns from "m"`、`default as`-only 命名 import 走 `__importDefault`、string-literal export 名、local export 的 live-binding/引用重写（`GetReferencedExportContainer`）、shorthand-property 展开、import-only 模块 `__esModule` 完整门控、usestrict 精确 `format` 门控、导出函数赋值与 external-helpers import 的 prologue 精确排序、default/namespace use 经 resolver）+ P10 |
| esmodule | `.../es2015/modules/**`（`--module es2015/esnext`）| value import/export 保留 + `export =`/`import =` elision + `createEmptyImports` + es2015 `export * as ns` 改写 | 6u ✓（tracer 透传 + `export =`/`import =` elision → `export {};` + 守卫 + export default/`{x}`/`*`/re-export/`import * as ns` 透传）/ **6ab ✓（`export * as ns from "m"` 在 es2015 改写为 `import * as ns_1 from "m";` + `export { ns_1 as ns };`；名为 `default` → `export default default_1;`；esnext 透传守卫；`new_generated_name_for_node`/6p，tsc 对拍）** / DEFER（type-only import elision 需 EmitResolver、作用域正确引用重写需真实 ReferenceResolver、`--module preserve` `export =`→`module.exports`、`--rewriteRelativeImportExtensions`、动态 `import()`、Node16+ `import =`→同步 require、external-helpers 注入、`export * as "default"` string-literal 名）+ P10 |
| impliedmodule | （无独立 baseline；由 CJS/ESM 输出间接覆盖）| 按文件 emit module format 分派 CJS/ESM | **6ac ✓（`format >= ES2015`（`is_es_module_format`）→ ESM，否则 CJS；`IsDeclarationFile` → 原样；commonjs→CJS tracer + esnext→ESM + es2015 谓词边界 + 声明文件守卫）** / DEFER（per-file `impliedNodeFormat`/`.cjs`/`.mjs` 需 `SourceFileMetaData`；AMD/UMD/System）+ P10 |
| systemmodule | （无独立 baseline；shape 取 tsc `--module system` 对拍）| `System.register([deps], function (exports_1, context_1) { "use strict"; return { setters, execute }; })` 包装 | **6ae ✓（register wrapper：`exports_1`/`context_1` via `new_unique_name`、`"use strict"` 前导、外层 body `MULTI_LINE`；dependency list：external import specifier → deps 数组 + 每依赖空体 setter `function (_1) { }`；execute body：顶层 value 语句移入；gate 透传）** / DEFER（export-setter 接线 named export→`exports({...})`/setter 体、import binding 重写·hoisting·live bindings——均 blocked-by 真实 `ReferenceResolver`；`var __moduleName`、module-name 首参、`export *` star helper、dependency 分组/去重）+ P10 |

> 6e 仅落地 `externalmoduleinfo` 结构化分析子集；CJS/ESM 变换待一轮 substitution + resolver + compilerOptions 前置基建（详见 `impl.md` 的「moduletransforms 解锁前置」）。

## jsxtransforms conformance 切片（P10 端到端兜底）

| transform | conformance 子集 | 验证内容 | 目标轮 |
|---|---|---|---|
| jsx（classic）| `tests/cases/conformance/jsx/**`（`--jsx react`）| `<tag attrs>children</tag>` → `React.createElement(tag, props, ...children)`、fragment → `React.Fragment` | 6f ✓（intrinsic/组件标签、string/expr 属性、expr/text/嵌套 子节点、fragment）/ DEFER（spread attr、entity 解码、自定义 factory/namespace、whitespace 精确边界）+ P10 |
| jsx（automatic）| `tests/cases/conformance/jsx/**`（`--jsx react-jsx`/`react-jsxdev`）| `jsx`/`jsxs`/`jsxDEV` + implicit `react/jsx-runtime` import | 6e-3 ✓（运行时选择经 `compiler_options.jsx`：`<div/>`→`jsx("div", {})`）/ DEFER（children-in-props、`jsxs`、implicit-import 注入需 emitResolver `SetReferencedImportDeclaration`）+ P10 |

> 6f 落地 classic runtime 子集（硬编码 `React.createElement`/`React.Fragment` 工厂）；automatic runtime 与自定义 pragma/factory 待 compilerOptions/resolver 线程化（同 moduletransforms 缺口）。

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
| `TestImportElision` import 侧值用例 | ~~依赖 checker.NewChecker + EmitResolver~~ → **6af 已落地**（4an `EmitResolver::is_referenced`）| ✅ 完成 |
| `TestImportElision` export specifier 侧（type-only 丢 / value 留 / per-specifier / `export *`）| ~~依赖 checker `IsValueAliasDeclaration`~~ → **6ag 已落地**（4ao `EmitResolver::is_value_alias_declaration`）| ✅ 完成 |
| `TestImportElision` `import = require("m")`（external 形）/ `export =`（value/type-only）| ~~依赖 checker 4ao `declaration_name` 排除 `ImportEqualsDeclaration` 名 / `isValueAliasDeclarationWorker` 的 `ExportAssignment` 分支~~ → **6ah 已落地**（4ap 两处扩展）| ✅ 完成 |
| `TestImportElision` entity-name `import = a.b` / 跨模块 re-export / 仅类型续命 | 依赖 checker `IsTopLevelValueImportEqualsWithEntityName` / `resolveExternalModuleSymbol` / `markLinkedReferences`（均未移植）| checker 补后回填 / P10 |
| estransforms 各 target 降级字节级 | 需 `tsc --target` baseline | P10 |
| moduletransforms CJS/ESM/AMD parity | 需 `tsc --module` baseline | P10 |
| jsxtransforms classic/automatic runtime | 需 `tsc --jsx` baseline | P10 |
| declarations `.d.ts` 全量（含 isolatedDeclarations 诊断） | 依赖 nodebuilder/modulespecifiers + checker | P10 |
