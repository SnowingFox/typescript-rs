# transformers: 实现方案（impl.md）

**crate**：`tsgo_transformers`（**单 crate，子包作子 module**——见下「crate 结构（Round 6 修订）」）　**目标**：在 emit 之前对 AST 做语义保持的改写——擦除 TS 专有语法（类型/修饰符/枚举/命名空间）、把高版本 ECMAScript 语法降级到目标版本、模块格式转换（CJS/ESM）、JSX 转换、声明（`.d.ts`）生成。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_printer`（EmitContext/NodeFactory/EmitFlags）`tsgo_binder` `tsgo_checker`(部分子包) 等
**Go 源**：`internal/transformers/`（40 个非测试文件：根 5 + 6 子包 35）

## crate 结构（Round 6 修订，权威）

> **决策（覆盖下文「子包 crate 化决策」的旧方案）**：本移植把整个 `internal/transformers`（含全部子包）做成**一个 crate `tsgo_transformers`**，每个 Go 子包作为该 crate 的**子 module**：`.rs` 文件紧贴各自 `.go` 同目录，通过根 `lib.rs` 的 `pub mod <subpkg>;` → `<subpkg>/mod.rs` 挂载。
>
> 理由：(1) 子包之间高度共享根包的 `Transformer`/`Chain`/`EmitContext` 接线，单 crate 省去 6 个 `Cargo.toml` 与 path-dep 往返；(2) 单一 owner 推进；(3) 仍满足 PORTING §2「子包默认作父 crate 的子 module」。旧的「6 个独立子 crate」方案保留在下文作为被否决的备选记录。

## 分轮计划（transformers port plan，6a–6g）

按真实依赖与可测性把 transformers 拆成 7 个 TDD 小轮，每轮红→绿垂直推进、收口 gate 全绿：

| 轮 | 范围 | 关键 tracer bullet | 依赖/阻塞 |
|---|---|---|---|
| **6a** ✅ | 根传输基建：`transformer`（驱动）+`chain`+`modifiervisitor`+`utilities` 可达子集 | 恒等 transform 过解析后的 SourceFile → 结构等价（重 emit 同文本） | 仅 `tsgo_ast`/`tsgo_printer` 现有 API（无需 ast/printer 增长） |
| **6b** ✅ 子集 | `tstransforms`：`typeeraser`（剥类型纯重建簇）+ `tstransforms/utilities`（`constant_expression`）；`runtimesyntax`/`legacydecorators`/`metadata`/`typeserializer`/`importelision` DEFER | `var a: number`→`var a;` 经 6a 驱动重 emit（真 stage 端到端） | 落地首个 additive `printer::NodeFactory` 节点构造器增长（`new_identifier`/`new_string_literal`/`new_numeric_literal`/`new_prefix_unary_expression`）。DEFER 项 blocked-by：移除式 visitor、`NotEmittedStatement`/`PartiallyEmittedExpression` 节点种类（需 ast/lib.rs，超范围）、checker `EmitResolver`/类型序列化 |
| **6c-prep** ✅ | 共享降级原语：移除式访问 (`visit_nodes_removable`) + 两个省略节点种类 (`NotEmittedStatement`/`PartiallyEmittedExpression`) + **完成 typeeraser 省略**（作为证明） | 一个 transform 从 SourceFile 丢弃一条语句后重 emit 不含它 | additive 增长 `ast/{visitor.rs,lib.rs}`（移除式访问 + 2 节点种类 + 各自 `visit_each_child`/`for_each_child` 臂）与 `printer/*`（2 种类 emit）。解除 6b 的 DEFER（移除式 visitor + 节点种类）|
| **6c-1** ✅ 子集 | `estransforms` part 1：`exponentiation`（tracer）+ `classfields`（实例字段→构造器赋值子集） | `a ** b`→`Math.pow(a, b)` 经驱动重 emit | 全走 arena 既有构造器，**本轮无需 ast/printer 增长**；其余 es stage + classfields 余下 → 6c-2 |
| **6c-2** ✅ 子集 | `estransforms` part 2：完成 `classfields` 构造器插入族（既有构造器插入、`extends` 合成 `super(...arguments)`、既有 `super()` 后插入）| `class C extends B { x = 1; constructor() { super(); } }` → `super(); this.x = 1;` | 全走 arena 既有构造器，**本轮无需 ast/printer 增长**；static/私有名/accessor/temp-hoist → 6c-3 |
| **6c-3** ✅ 子集 | 两条复用基建 + 首消费者：`EmitContext` 变量环境（temp hoist）+ `exponentiation` `**=` element/property 目标；`SyntaxList` 节点种类 + static 字段 | `a[x] **= b` → `var _a, _b; (_a=a)[_b=x]=Math.pow(_a[_b],b)`；`class C { static x = 1 }` → `class C {} C.x = 1;` | additive 增长 `printer::EmitContext`（var-env）+ `ast`（`SyntaxList` 节点种类）+ `printer`（SyntaxList emit）。私有名（WeakMap/WeakSet）、accessor、计算名、class-expr、param-props、target 门控 → 6c-4 |
| 6c-4 | `classfields` 余下（私有名 WeakMap/WeakSet、accessor、计算名、class-expr、param-props、target 门控）→ `esdecorator` | `class C { #x = 1 }`；`@dec` | 需私有环境映射 + 私有访问表达式重写 + WeakMap brand 命名（+ helper-library emit）、checker（esdecorator 元数据）|
| 6d | `moduletransforms`：CJS/ESM/implied + externalModuleInfo | `import {x} from "m"`→`require` | factory + module 格式查询 |
| 6e | `jsxtransforms` + `inliners` | `<a/>`→`createElement`；const enum 内联 | factory + checker 求值（inliners） |
| 6f | `declarations`：`.d.ts` transform 框架 + tracker + diagnostics | 基础 `.d.ts` 形状 | nodebuilder/modulespecifiers + checker resolver；正确性靠 P10 |
| 6g | 根 `destructuring` + `utilities` 其余（绑定↔赋值转换/super 定位/范围移动）+ `tstransforms/importelision`（checker 就绪后） | 数组解构展开；import elision | factory 构造器 + checker `EmitResolver` |

## 这个包是什么（业务说明）

transformers 是 **emit 的前置改写层**，夹在 checker（P4）和 printer（本 phase）之间。它把"语义完整、含 TS/高版本语法"的 AST，逐 stage 改写成"目标 JS 可直接打印"的 AST。每个 transformer 是一个 `*ast.NodeVisitor` 包装：访问节点、用 `printer.NodeFactory` 造替换节点、用 `printer.EmitContext` 记录 original/emitFlags/注释/源映射，使 printer 的输出保留正确位置与注释。

stage 分组（= Go 子包）：

- **tstransforms**：TS→JS 的核心擦除——`typeEraser`（删类型注解/接口/类型别名/类型参数/修饰符里的可见性）、`importElision`（删仅类型 import/export）、`runtimeSyntax`（enum/namespace 降级为运行时代码）、`legacyDecorators` + `metadata` + `typeSerializer`（实验性装饰器与 emitDecoratorMetadata）。
- **estransforms**：ECMAScript 版本降级链（`GetESTransformer` 按 `target` 选链）——async/await、class fields、装饰器（标准）、可选链、nullish 合并、对象 rest/spread、for-await、逻辑赋值、using、指数运算、tagged template、optional catch、use strict。
- **moduletransforms**：模块系统改写——`commonjsModule`（→ `require`/`exports`）、`esmodule`、`impliedModule`（按文件 impliedFormat 选择）+ `externalModuleInfo`。
- **jsxtransforms**：JSX → `React.createElement` / 自动 runtime `_jsx`。
- **inliners**：`constEnum` 内联（`const enum` 成员替换为字面量）。
- **declarations**：`.d.ts` 声明 emit（最复杂子包，依赖 nodebuilder/modulespecifiers + symbol tracker 做可访问性诊断）。
- **根包**：`Transformer` 基类、`Chain`（串联多个 transformer，仅作用于 SourceFile）、`destructuring`（解构赋值/绑定降级展开）、`modifiervisitor`、`utilities`（is-generated-identifier、绑定模式↔赋值模式转换、super 调用定位等共享工具）。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Transformer struct{ emitContext, factory, visitor }` | `pub struct Transformer { context, factory, visitor }` | 基类；各 stage 组合它（Go 用嵌入 `transformers.Transformer`） |
| `func (tx *Transformer) NewTransformer(visit, emitContext)` | 构造函数返回 `Transformer`；`visit` 是 `Fn(&Node)->&Node` | Go 的"自初始化"模式 → Rust 构造器 |
| `TransformerFactory = func(opt) *Transformer` | `pub type TransformerFactory = fn(&TransformOptions) -> Option<Transformer>` | `Chain` 组合；返回 `Option`（nil → 跳过该 stage） |
| `chainedTransformer{ Transformer; components []*Transformer }` | `struct ChainedTransformer { base: Transformer, components: Vec<Transformer> }` | 嵌入 → 组合 |
| `TransformOptions{ Context, CompilerOptions, Resolver, EmitResolver, GetEmitModuleFormatOfFile }` | `pub struct TransformOptions<'a> { context, compiler_options, resolver, emit_resolver, get_emit_module_format_of_file }` | 共享配置；`*EmitContext` 跨 stage 复用 |
| `*ast.NodeVisitor` + `VisitEachChild`/`VisitSourceFile` | `tsgo_ast::NodeVisitor`（P2/P3 落地） | 访问器走 NodeId/arena |
| 各 stage `struct{ transformers.Transformer; ...state }` | 组合 `Transformer` + stage 私有状态 | 1:1 |
| `FlattenLevel int` / `CreateAssignmentCallback func(...)`（destructuring） | `#[repr(i32)] enum FlattenLevel` / `Box<dyn Fn(...)>` | 解构展开回调 |
| `DeclarationEmitHost` / `OutputPaths` / `SymbolTracker*` interface（declarations） | trait | 依赖 checker resolver + modulespecifiers |
| `GetSymbolAccessibilityDiagnostic = func(...)` | `type ... = fn(...) -> Option<SymbolAccessibilityDiagnostic>` | 可见性诊断回调表 |

### 子包 crate 化决策（关键）

Go 里 `internal/transformers/<sub>` 每个都是独立 package、各有不同 import 边（如 `declarations` 依赖 `nodebuilder`/`modulespecifiers`，`tstransforms` 依赖 `checker`/`module`），符合 PORTING §3 "每个 `internal/<pkg>` = 一个 crate"。**决策：子包各自独立 crate**，名沿用目录名：

| Go 子包 | crate | 依赖要点 |
|---|---|---|
| `internal/transformers`（根） | `tsgo_transformers` | ast/core/printer/binder |
| `internal/transformers/estransforms` | `tsgo_estransforms` | → `tsgo_transformers` |
| `internal/transformers/moduletransforms` | `tsgo_moduletransforms` | → `tsgo_transformers` |
| `internal/transformers/tstransforms` | `tsgo_tstransforms` | → `tsgo_transformers` + checker/module/symlinks/packagejson |
| `internal/transformers/jsxtransforms` | `tsgo_jsxtransforms` | → `tsgo_transformers` |
| `internal/transformers/inliners` | `tsgo_inliners` | → `tsgo_transformers` + checker（const enum 求值） |
| `internal/transformers/declarations` | `tsgo_declarations` | → `tsgo_transformers` + nodebuilder/modulespecifiers |

> 备选（PORTING §2 默认）：作 `tsgo_transformers` 的子 module。但因子包有独立外部依赖且依赖深度不一，独立 crate 更干净、编译并行度更高。此为本 phase 决策，记入 README。

## 文件清单 → Rust 模块

### 根包 `tsgo_transformers`（5 文件）

> crate 根 `internal/transformers/lib.rs` 只做聚合（`pub mod transformer/chain/modifiervisitor/utilities;` + 子包 `pub mod <subpkg>;` + `SharedEmitContext` 别名 + `#[cfg(test)] test_support`）。各 `.go` 按 basename 1:1 映射到同目录 `.rs`（PORTING §2）。

| Go 文件 | Rust 文件 | 状态 | 说明 |
|---|---|---|---|
| `internal/transformers/transformer.go` | `internal/transformers/transformer.rs`（`mod transformer`） | ✅ 6a | `Transformer`/`new_transformer`/`emit_context`/`transform_source_file`（+`run_visit` crate 私有，供 chain 复用借用） |
| `internal/transformers/chain.go` | `internal/transformers/chain.rs` | ✅ 6a | `TransformOptions`/`TransformerFactory`/`chain`（含 `chainedTransformer` 合成 visit） |
| `internal/transformers/destructuring.go` | `internal/transformers/destructuring.rs` | 6g | `FlattenDestructuringAssignment/Binding` + 绑定/赋值元素工具 |
| `internal/transformers/modifiervisitor.go` | `internal/transformers/modifiervisitor.rs` | ✅ 6a | `extract_modifiers` |
| `internal/transformers/utilities.go` | `internal/transformers/utilities.rs` | 6a 子集 / 6g 余下 | 6a：generated/helper/local/export-name 谓词 + `get_non_assignment_operator_for_compound_assignment`；余下（绑定模式转换/super 定位/范围移动）DEFER 6g |

### `tstransforms`（7 文件，含被测的 2 stage）

> 子模块 `pub mod tstransforms;` → `tstransforms/mod.rs`（含全部 DEFER + blocked-by 说明）。

| Go 文件 | Rust 文件 | 状态 | 说明 |
|---|---|---|---|
| `tstransforms/typeeraser.go` | `tstransforms/typeeraser.rs` | ✅ 6b+6c-prep | `new_type_eraser_transformer` + 移除式 `type_eraser_visit`（返回 `Option<NodeId>`，`None`=从列表省略）。6b 剥类型纯重建簇；**6c-prep 完成省略**：`interface`/`type`/ambient(`declare`)/`namespace export`/重载 → `NotEmittedStatement`；类型只读修饰符(public/private/…)、`implements`、`this` 参、`declare`/`abstract` 字段 → 移除；`as`/`satisfies`/`<T>x`/`x!` → `PartiallyEmittedExpression`；`import type`/`import type =` → `NotEmittedStatement`。DEFER：每-specifier `import { type x }`、命名空间实例化分析(`IsInstantiatedModule`)、method/ctor/accessor 重载、`compilerOptions` 分支、用法式 import elision（checker）—— 见 mod.rs |
| `tstransforms/utilities.go` | `tstransforms/utilities.rs` | ✅ 6b | `constant_expression`（+`ConstantValue`）：string/number/NaN/±Infinity/负数 → 工厂节点 |
| `tstransforms/importelision.go` | `tstransforms/importelision.rs` | — DEFER(P5) | `ImportElisionTransformer`；blocked-by：checker `EmitResolver.MarkLinkedReferencesRecursively` 未移植 |
| `tstransforms/runtimesyntax.go` | `tstransforms/runtimesyntax.rs` | — DEFER(P5) | enum/namespace/参数属性/`import=` 运行时降级；blocked-by：移除式 visitor + IIFE/赋值/块工厂构造器面 |
| `tstransforms/legacydecorators.go` | `tstransforms/legacydecorators.rs` | — DEFER(P5) | 实验性装饰器降级；blocked-by：checker 类型序列化 + 装饰器 helper 工厂 |
| `tstransforms/metadata.go` | `tstransforms/metadata.rs` | — DEFER(P5) | `emitDecoratorMetadata`；blocked-by：同上（typeserializer） |
| `tstransforms/typeserializer.go` | `tstransforms/typeserializer.rs` | — DEFER(P5) | 类型 → 元数据表达式；blocked-by：checker 类型→节点序列化 |

### `estransforms`（17 文件）

> 子模块 `pub mod estransforms;` → `estransforms/mod.rs`（含全部 DEFER + blocked-by 说明）。6c-1 落地 `exponentiation`（tracer）+ `classfields` 子集，**未触碰 ast/printer**（全走 arena 既有构造器）。

| Go 文件 | Rust 文件 | 状态 | 说明 |
|---|---|---|---|
| `estransforms/exponentiation.go` | `exponentiation.rs` | ✅ 6c-1+6c-3 | `new_exponentiation_transformer`：`a ** b`→`Math.pow(a, b)`、`a **= b`（标识符）→`a = Math.pow(a, b)`、**`a.x **= b` / `a[x] **= b`（temp hoist）**→`(_a=a).x=Math.pow(_a.x,b)` / `(_a=a)[_b=x]=Math.pow(_a[_b],b)`（顶层语句路径 ec-threaded + `var` hoist）。DEFER：非顶层作用域内的 temp-hoist `**=`（需作用域级 var-env 嵌套）|
| `estransforms/classfields.go` | `classfields.rs` | ✅ 6c-1/2/3 子集 | `new_class_fields_transformer`：实例字段 → 构造器 `this.x = init`（完整构造器插入族）；**static 字段** → 类后 `C.x = init`（返回 `SyntaxList[class, assignment...]`）。仅纯标识符字段（有初始化器）。DEFER 6c-4：私有名 `#x`（WeakMap/WeakSet）、`accessor` 字段、计算属性名、ClassExpression、参数属性、prologue、匿名类 static、`--target`/`useDefineForClassFields` 门控 |
| `estransforms/definitions.go` | `estransforms/lib.rs` | — DEFER(P5) | `GetESTransformer` + 各版本链 `NewES20xxTransformer`（crate 根聚合）；blocked-by：依赖各 es stage 就绪 |
| `estransforms/async.go` | `async.rs` | — DEFER(P5) | `newAsyncTransformer`（async/await 降级） |
| `estransforms/classthis.go` | `classthis.rs` | — DEFER(P5) | class `this`/`#brand` 辅助 |
| `estransforms/esdecorator.go` | `esdecorator.rs` | — DEFER(P5) | `newESDecoratorTransformer`（标准装饰器）；blocked-by：checker 元数据 + helper emit |
| `estransforms/forawait.go` | `forawait.rs` | `newforawaitTransformer` |
| `estransforms/logicalassignment.go` | `logicalassignment.rs` | `newLogicalAssignmentTransformer`（`&&=`/`\|\|=`/`??=`） |
| `estransforms/namedevaluation.go` | `namedevaluation.rs` | named evaluation 辅助 |
| `estransforms/nullishcoalescing.go` | `nullishcoalescing.rs` | `newNullishCoalescingTransformer`（`??`） |
| `estransforms/objectrestspread.go` | `objectrestspread.rs` | `newObjectRestSpreadTransformer` |
| `estransforms/optionalcatch.go` | `optionalcatch.rs` | `newOptionalCatchTransformer` |
| `estransforms/optionalchain.go` | `optionalchain.rs` | `newOptionalChainTransformer`（`?.`） |
| `estransforms/taggedtemplate.go` | `taggedtemplate.rs` | `newTaggedTemplateLiftRestrictionTransformer` |
| `estransforms/usestrict.go` | `usestrict.rs` | `NewUseStrictTransformer` |
| `estransforms/using.go` | `using.rs` | `newUsingDeclarationTransformer`（`using`/`await using`） |
| `estransforms/utilities.go` | `utilities.rs` | 子包共享工具 |

### `moduletransforms`（5 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `moduletransforms/commonjsmodule.go` | `commonjsmodule.rs` | `NewCommonJSModuleTransformer` |
| `moduletransforms/esmodule.go` | `esmodule.rs` | `NewESModuleTransformer` |
| `moduletransforms/impliedmodule.go` | `impliedmodule.rs` | `NewImpliedModuleTransformer`（按 impliedFormat 分派） |
| `moduletransforms/externalmoduleinfo.go` | `externalmoduleinfo.rs` | 收集导入/导出元信息 |
| `moduletransforms/utilities.go` | `utilities.rs` | 子包共享工具（crate 根 `lib.rs` 聚合 mod） |

### `jsxtransforms` / `inliners` / `declarations`

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `jsxtransforms/jsx.go` | `jsxtransforms/lib.rs` | `JSXTransformer` / `NewJSXTransformer` |
| `inliners/constenum.go` | `inliners/lib.rs` | `ConstEnumInliningTransformer` / `NewConstEnumInliningTransformer` |
| `declarations/transform.go` | `declarations/transform.rs`（crate 根聚合于 `lib.rs`） | `DeclarationTransformer` / `NewDeclarationTransformer` / `DeclarationEmitHost`/`OutputPaths` |
| `declarations/tracker.go` | `declarations/tracker.rs` | `SymbolTrackerImpl` / `NewSymbolTracker` / 可访问性追踪 |
| `declarations/diagnostics.go` | `declarations/diagnostics.rs` | `SymbolAccessibilityDiagnostic` + 诊断回调表 |
| `declarations/util.go` | `declarations/util.rs` | 子包工具 |

## 依赖白名单（本包新增的 crate）

无 §10 之外新增第三方。子包间 path 依赖如上表。`inliners`/`tstransforms`/`declarations` 依赖 checker（P4）做求值/类型序列化/符号可访问性。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### 根 `tsgo_transformers`

- [x] `pub struct Transformer` + `new_transformer(visit, context)` + `emit_context()/transform_source_file()`（`run_visit` crate 私有）　`// Go: transformer.go:Transformer`　**[6a]** — 偏离：Go `Visitor()/Factory()` 折叠进 `emit_context()`（Rust `EmitContext` 自持 arena/factory）；`visit` 签名 `FnMut(&mut EmitContext, NodeId) -> NodeId`
- [x] `pub struct TransformOptions` + `pub type TransformerFactory` + `pub fn chain(transforms) -> TransformerFactory`（含 `chainedTransformer` 合成 visit）　`// Go: chain.go:TransformOptions/TransformerFactory/Chain`　**[6a]** — `TransformOptions` 仅含 `context: Option<SharedEmitContext>`；其余字段 DEFER 6b+
- [ ] destructuring：`pub enum FlattenLevel`、`pub type CreateAssignmentCallback`、`FlattenDestructuringAssignment`、`FlattenDestructuringBinding`、`BindingOrAssignmentElementAssignsToName`、`BindingOrAssignmentElementContainsNonLiteralComputedName`、`GetInitializerOfBindingOrAssignmentElement`　`// Go: destructuring.go:*`　**[6g]**
- [x] `extract_modifiers`　`// Go: modifiervisitor.go:ExtractModifiers`　**[6a]** — 直接过滤实现（visitor 暂无 node 删除语义），不经泛型 visitor
- [x] utilities（6a 子集）：`is_generated_identifier/is_helper_name/is_local_name/is_export_name`、`get_non_assignment_operator_for_compound_assignment`　`// Go: utilities.go:*`　**[6a]**
- [ ] utilities（余下）：`IsIdentifierReference`、`ConvertBindingPatternToAssignmentPattern`、`ConvertVariableDeclarationToAssignmentExpression`、`SingleOrMany`、`IsSimpleCopiableExpression`、`IsOriginalNodeSingleLine`、`IsSimpleInlineableExpression`、`FindSuperStatementIndexPath`、`GetSuperCallFromStatement`、`MoveRangePastModifiers`、`MoveRangePastDecorators`　`// Go: utilities.go:*`　**[6g]** blocked-by：factory 节点构造器 / `ast::is_string_literal_like`·`is_numeric_literal`·`skip_parentheses`·`is_super_call`·`can_have_modifiers` 等谓词未移植

### `tstransforms`（重点：被测 2 stage 先行）

- [ ] `pub struct TypeEraserTransformer` + `pub fn new_type_eraser_transformer(opt) -> Transformer`　`// Go: typeeraser.go:NewTypeEraserTransformer`（**先做：过 TestTypeEraser**）
- [ ] `pub struct ImportElisionTransformer` + `pub fn new_import_elision_transformer(opt) -> Transformer`　`// Go: importelision.go:NewImportElisionTransformer`（**过 TestImportElision**，依赖 EmitResolver `MarkLinkedReferencesRecursively`）
- [ ] `new_runtime_syntax_transformer` / `new_legacy_decorators_transformer` / `new_metadata_transformer`　`// Go: runtimesyntax.go / legacydecorators.go / metadata.go`
- [ ] `get_set_accessor_value_parameter` + typeSerializer 内部　`// Go: typeserializer.go:GetSetAccessorValueParameter`

### `estransforms`

- [ ] `pub fn get_es_transformer(opts) -> Option<Transformer>` + 版本链常量 `NewESNext/ES2021/ES2020/ES2019/ES2018/ES2017/ES2016Transformer`（`Chain` 组合）　`// Go: definitions.go:GetESTransformer`
- [ ] 各 stage 构造器（逐文件）：`new_async/new_class_fields/new_es_decorator/new_exponentiation/new_forawait/new_logical_assignment/new_nullish_coalescing/new_object_rest_spread/new_optional_catch/new_optional_chain/new_tagged_template_lift_restriction/new_use_strict/new_using_declaration` + `classthis`/`namedevaluation`/`utilities` 辅助　`// Go: estransforms/*.go:new*`

### `moduletransforms` / `jsxtransforms` / `inliners`

- [ ] `new_commonjs_module_transformer/new_esmodule_transformer/new_implied_module_transformer` + `externalModuleInfo`　`// Go: moduletransforms/*.go`
- [ ] `new_jsx_transformer`　`// Go: jsxtransforms/jsx.go:NewJSXTransformer`
- [ ] `new_const_enum_inlining_transformer`　`// Go: inliners/constenum.go:NewConstEnumInliningTransformer`

### `declarations`

- [ ] `pub trait DeclarationEmitHost` / `OutputPaths` + `pub struct DeclarationTransformer` + `new_declaration_transformer(host, context, options, decl_path, decl_map_path)`　`// Go: transform.go:NewDeclarationTransformer`
- [ ] `SymbolTrackerImpl` + `new_symbol_tracker` + `SymbolTrackerSharedState`　`// Go: tracker.go:NewSymbolTracker`
- [ ] `SymbolAccessibilityDiagnostic` + `GetSymbolAccessibilityDiagnostic` 回调表　`// Go: diagnostics.go:*`

### Cargo / crate 接线（Round 6 修订）

- [x] 单 `Cargo.toml`（`tsgo_transformers`，已 scaffold + workspace member）　**[6a]**
- [x] 根 `lib.rs` 聚合：`pub mod transformer/chain/modifiervisitor/utilities;` + `SharedEmitContext` 别名 + `#[cfg(test)] test_support`；`pub use` 导出公共面　**[6a]**
- [ ] 后续轮追加 `pub mod tstransforms/estransforms/moduletransforms/jsxtransforms/inliners/declarations;`（各 `<subpkg>/mod.rs`）　**[6b–6f]**

## 6a worklog（red→green 推进记录）

每个行为严格红→绿，逐条记录"红在哪一步看到"：

1. **transformer 恒等 tracer bullet**（`identity_transform_round_trips_source_file`）：先写 `transform_source_file`/`new_transformer`/`emit_context` 为 `todo!()` → 跑测试见 `not yet implemented` panic（红）→ 实现 `new_transformer`/`emit_context`/`transform_source_file`（借 `Rc<RefCell<EmitContext>>` 一次跑 visit）→ 绿。
2. **transformer 重写 tree**（`rewriting_transform_rebuilds_tree`）：`a`→`x` 经 `visit_each_child` 重建，验证驱动把 EmitContext arena 穿到 `visit_each_child` 且 factory 造的替换节点回流到 emit（绿，复用 1 的实现）。
3. **chain 组合**（`chain_runs_stages_left_to_right`/`single_element_passthrough`/`skips_none_stages`/`all_none_yields_none`/`empty_panics`）：`chain` 为 `todo!()` → 5 用例红 → 实现（len<2 直返 / 过滤 None / 合成 visit 复用单借用）→ 全绿。
4. **extract_modifiers**（`none_returns_none`/`filters_disallowed`/`keeps_all_when_allowed`）：`todo!()` → 红 → 直接过滤实现（保留 allowed/非修饰符节点，重算 `modifier_flags`，保原 loc）→ 绿。
5. **utilities**（逐函数）：`get_non_assignment_operator_for_compound_assignment` → 红 → 绿；`is_generated_identifier` → 红 → 绿；`is_helper_name/is_local_name/is_export_name` 簇 → 红 → 绿。

**测试计数（6a）**：13 个 `#[test]`（transformer 2 + chain 5 + modifiervisitor 3 + utilities 3）+ 9 个 doctest，全绿。

### upstream（ast/printer）增长

- **本轮无**。6a 所需的 `NodeArena::visit_each_child`/`VisitOptions`、`EmitContext`（`with_arena`/`emit_flags`/`set_emit_flags`/`has_auto_generate_info`/`arena(_mut)`/`factory`）、`NodeFactory::new_temp_variable`、`ast::modifier_to_flag` 均已存在，故未触碰 `internal/ast/visitor.rs` 或 `internal/printer/{factory,emitcontext,utilities}.rs`。6b+ 的节点构造器需求将在那时按红→绿 additive 增长。

## 6b worklog（red→green 推进记录）

子包 `tstransforms`，逐行为红→绿：

1. **typeeraser tracer bullet**（`variable_declaration_type_is_erased`）：恒等 stub → 红（`var a: number;` ≠ `var a;`）→ 实现 `VariableDeclaration` 重建（丢类型）+ 默认 `visit_each_child` 递归 → 绿。**证明一个真实 tstransforms stage 经 6a `Transformer` 驱动端到端运行。**
2. `call_expression_type_arguments_erased`：`f<T>()`→`f();` —— `CallExpression` 丢类型实参（红→绿）。
3. `new_expression_type_arguments_erased`：`new f<T>()`→`new f();` 等 —— 调试发现 `new f<T>()` 把 `<T>` 挂在 `ExpressionWithTypeArguments`（被 callee 包裹），遂加 `ExpressionWithTypeArguments` 臂（红→绿）。
4. `expression_with_type_arguments_erased`：`F<T>`→`F;`（红→绿）。
5. `function_declaration_type_params_and_return_erased`：`function f<T>(): U {}`→`function f() { }`（红→绿，丢类型参数+返回类型）。
6. `class_declaration_type_params_erased` / `class_expression_*` / `function_expression_*`：类/函数表达式同形重建（红→绿）。
7. **printer factory 增长**（`new_identifier_is_synthesized` / `new_literals_and_prefix_unary_are_synthesized`）：在 `tsgo_printer` 加 `new_identifier`/`new_string_literal`/`new_numeric_literal`/`new_prefix_unary_expression`（各带 `onCreate` 置 `SYNTHESIZED`）—— 缺方法编译红 → 实现 → 绿。
8. **constant_expression**（`constant_expression_builds_literals` / `..._negates_with_prefix_unary`）：缺函数编译红 → 实现（string/number/NaN/±Infinity/负数）→ 绿。

**测试计数（6b 新增）**：transformers +10 `#[test]`（typeeraser 8 + utilities 2）；printer +2 `#[test]`（factory）+4 doctest。

### upstream（ast/printer）增长（6b）

- **printer**（additive，允许范围内）：`internal/printer/factory.rs` 新增 `new_identifier`/`new_string_literal`/`new_numeric_literal`/`new_prefix_unary_expression` + 私有 `on_create`（置 `NodeFlags::SYNTHESIZED`，镜像 Go `EmitContext.onCreate`），各带 `factory_test.rs` 用例。**未改既有 API、未碰 `ast`**。
- 仍**未**触碰 `internal/ast/visitor.rs`：6b 的剥类型纯重建走 arena 既有 `new_*` 构造器 + `visit_each_child` 默认递归即可；移除式 visitor 留待 6b-后续/6g。

## 6c-prep worklog（red→green 推进记录）

为解锁 es-downlevel 大 stage 与 runtimesyntax，先落地共享降级原语，逐行为红→绿：

1. **移除式访问**（`tsgo_ast::NodeArena::visit_nodes_removable`）：缺方法编译红 → 实现（`VisitRemovable` 回调，`None`=丢弃；附 `ast/visitor_test.rs` 用例）→ 绿。**端到端 tracer**（`transformer::tests::transform_can_drop_a_statement`）：transform 经 6a 驱动 + `visit_nodes_removable` 从 `a;\nb;` 丢 `a;` → 重 emit `b;`。
2. **省略节点种类**：`NotEmittedStatement`（unit）+ `PartiallyEmittedExpression(UnaryChildData)` 加为 `ast::NodeData` 变体（`Kind` 早已有）；加 arena 构造器 `new_not_emitted_statement`/`new_partially_emitted_expression`；补 `visit_each_child` + `for_each_child` 两处穷尽 match 臂。printer：`emit_statement` 加 `NotEmittedStatement`→无输出、`emit_expression_node` 加 `PartiallyEmittedExpression`→只 emit 内层；修正 `skip_partially_emitted_expressions`（旧 stub 匹配 `ExpressionStatement`，现匹配 `PartiallyEmittedExpression`）。各带 `_test.rs`（合成树 `check_synthetic`）红→绿。
3. **完成 typeeraser 省略**：把 `type_eraser_visit` 重构为返回 `Option<NodeId>`（绿→绿，保 8 个 6b 用例），再逐 case 红→绿：type-only 声明、ambient、namespace-export、类型只读修饰符、`declare`/`abstract` 字段、`implements`、`this` 参、断言四种、函数重载、`import type`/`import type =`。

**测试计数（6c-prep 新增）**：`tsgo_ast` +1（`visit_nodes_removable`）+2 doctest（两构造器）；`tsgo_printer` +2（两种类 emit）；`tsgo_transformers` +1（drop-statement tracer）+10（typeeraser 省略：8→18）。

### upstream（ast/printer）增长（6c-prep）

- **ast**（additive，允许范围内）：
  - `internal/ast/visitor.rs`：`visit_nodes_removable`（+ `VisitRemovable` 回调类型）。**未改既有 `visit_each_child` 签名**。
  - `internal/ast/lib.rs`：`NodeData::NotEmittedStatement` / `NodeData::PartiallyEmittedExpression(UnaryChildData)` 两个变体 + `new_not_emitted_statement` / `new_partially_emitted_expression` 构造器；`for_each_child` 补两臂。
  - 新增 `NodeData` 变体的爆炸半径：全工作区仅 `visit_each_child`、`for_each_child` 两处穷尽 match 需补臂（已补）；其余 crate（parser/binder/checker/…）要么按 `Kind` 分派、要么已有 catch-all，`cargo build --workspace --all-targets` 全绿，**无依赖 crate 的 match 臂被迫改动**。
- **printer**（additive）：`emit_statements.rs`（`emit_not_emitted_statement`）、`emit_expressions.rs`（`emit_partially_emitted_expression`）、`printer.rs`（修 `skip_partially_emitted_expressions`）。
- ast 新增节点种类已在本表「文件清单」记录；ast 包的 impl 文档无逐节点种类清单，故就近记于此（符合任务指引）。

## 6c-1 worklog（red→green 推进记录）

子模块 `estransforms` part 1，逐行为红→绿：

1. **exponentiation tracer**（`exponentiation_operator_lowered_to_math_pow`）：恒等 stub → 红（`a ** b;` ≠ `Math.pow(a, b);`）→ 实现 `BinaryExpression`/`**` → `Math.pow(left, right)`（arena 级 `math_pow` 助手）→ 绿。**验证 es-downlevel 路径经 6a 驱动端到端运行。**
2. `exponentiation_assignment_to_identifier_lowered`：`a **= b`（标识符目标）→ `a = Math.pow(a, b)`（红→绿）。
3. **classfields tracer**（`instance_field_initializer_moves_to_constructor`）：恒等 → 红（`class C {\n    x = 1;\n}`）→ 实现 `try_lower_simple_class`（收集实例字段初始化器 → 合成 `constructor() { this.x = init; }`，丢弃字段声明）→ 绿。
4. `multiple_instance_fields_move_to_constructor`：多个字段 → 多个 `this.<name> = ...`（红→绿）。

> 注：合成构造器体单行打印（`constructor() { this.x = 1; }`）—— Go `Block.MultiLine` 字段未随 Rust AST 携带（printer 既有 `TODO(port)`）；本轮测的是**降级行为**（字段→构造器赋值），格式属 printer 关注点。

**测试计数（6c-1 新增）**：`tsgo_transformers` +4 `#[test]`（exponentiation 2 + classfields 2）+2 doctest（两个 `new_*_transformer`）。

### upstream（ast/printer）增长（6c-1）

- **无**。`exponentiation` 与 `classfields` 子集全部用 arena 既有构造器（`new_identifier`/`new_property_access_expression`/`new_call_expression`/`new_binary_expression`/`new_token`/`new_keyword_expression`/`new_block`/`new_expression_statement`/`new_constructor_declaration`/`new_class_like`）+ `visit_each_child` 默认递归即可。未触碰 `internal/ast/*` 或 `internal/printer/*`。临时变量 hoist / helper emit 的 factory 增长留待 6c-2。

## 6c-2 worklog（red→green 推进记录）

完成 `classfields` 构造器插入族，逐行为红→绿（把 6c-1 的 `try_lower_simple_class` 扩展为完整插入逻辑，6c-1 两用例保持绿）：

1. `field_inits_prepend_to_existing_constructor`：`class C { x = 1; constructor() { this.y = 2; } }` → 把字段初始化器插到既有构造器体顶部（红→绿，重构 `try_lower_simple_class` 支持既有构造器 + `build_constructor_body`/`constructor_body_statements`）。
2. `derived_class_synthesizes_constructor_with_super`：`class C extends B { x = 1 }` → 合成 `constructor() { super(...arguments); this.x = 1; }`（红→绿，加 `heritage_has_extends` + `make_super_spread_arguments`）。
3. `field_inits_inserted_after_super_call`：`class C extends B { x = 1; constructor() { super(); this.y = 2; } }` → 在 `super()` 之后插入（红→绿，加 `find_super_statement_index`/`is_super_call_statement`/`skip_parentheses` —— 6a-DEFER 的 super 定位助手的简化子集，未含 TryStatement 嵌套）。

> 注：构造器体仍单行打印（`Block.MultiLine` 未携带，沿用 6c-1 注记）。Prologue 指令、参数属性插入位置为简化省略（DEFER 6c-3）。

**测试计数（6c-2 新增）**：`tsgo_transformers` +3 `#[test]`（classfields 构造器插入族）。

### upstream（ast/printer）增长（6c-2）

- **无**。构造器插入族全部用 arena 既有构造器 + `visit_each_child` 默认递归。`find_super_statement_index` 等 super 定位助手作为 `classfields.rs` 私有函数实现（6a 在 `transformers/utilities.rs` DEFER 的 `FindSuperStatementIndexPath` 全量路径版——含 TryStatement 嵌套——仍 DEFER）。static 字段所需的 `SyntaxList` 节点种类 + 语句展平、temp-hoist 的 `EmitContext` 变量环境留待 6c-3。

## 6c-3 worklog（red→green 推进记录）

两条复用基建（smallest-infra-first）+ 首消费者，逐行为红→绿：

**TRACK 1（temp hoist）**
1. `printer::EmitContext` 变量环境（`emitcontext_test.rs`：`variable_environment_hoists_declarations_into_a_var_statement` / `empty_variable_environment_hoists_nothing`）：缺方法编译红 → 实现 `start_variable_environment`/`add_variable_declaration`/`end_variable_environment`（var-scope 栈，`end` 产出 `var <decls>;` 语句）→ 绿。Go 全量 var/let 双栈 + functions + prologue 合并简化为单 var 栈（DEFER 余下）。
2. `exponentiation_assignment_to_property_access_hoists_temp`：`a.x **= b` → `var _a;\n(_a = a).x = Math.pow(_a.x, b);`。把 `exponentiation` 重构为 ec-threaded（SourceFile 包 var-env、ExpressionStatement/BinaryExpression 穿 ec；其余 arena-only 降级）→ 红→绿。
3. `exponentiation_assignment_to_element_access_hoists_temps`：`a[x] **= b` → `var _a, _b;\n(_a = a)[_b = x] = Math.pow(_a[_b], b);`（两个 temp）→ 红→绿。

**TRACK 2（SyntaxList → static 字段）**
4. `SyntaxList` 节点种类：`NodeData::SyntaxList(ListData)` + `new_syntax_list` + `visit_each_child`/`for_each_child` 两臂（`Kind::SyntaxList` 早已有）。printer `emit_statements_test.rs:syntax_list_statement_emits_children_in_sequence`：合成 `SyntaxList[a;, b;]` → 缺 emit 红 → 实现 `emit_syntax_list_statements`（逐子语句 + `write_line` 分隔）→ 绿。
5. `static_field_becomes_assignment_after_class`：`class C { static x = 1 }` → `class C {\n}\nC.x = 1;`。`try_lower_simple_class` 扩展：static 字段收集为 `C.x = init`，类后经 `SyntaxList[class, assignment]` 返回 → 红→绿。

**TRACK 3（私有名）**：DEFER（见下）。

**测试计数（6c-3 新增）**：`tsgo_printer` +3（var-env 2 + SyntaxList emit 1）；`tsgo_transformers` +3（exponentiation `**=` property/element 2 + classfields static 1）；`tsgo_ast` +1 doctest（`new_syntax_list`）。

### upstream（ast/printer）增长（6c-3）

- **printer**（additive）：`emitcontext.rs` 新增变量环境（`var_environments` 栈 + `start/add/end`）；`emit_statements.rs` 新增 `SyntaxList` 语句 emit。
- **ast**（additive）：`lib.rs` 新增 `NodeData::SyntaxList(ListData)` 变体 + `new_syntax_list` 构造器；`for_each_child` 补一臂；`visitor.rs` `visit_each_child` 补一臂。新增 `NodeData` 变体爆炸半径：全工作区仅 `visit_each_child`/`for_each_child` 两处穷尽 match 需补臂（已补），`cargo build --workspace --all-targets` 全绿，**无依赖 crate match 臂被迫改动**。
- **未触碰** `tsgo_binder`/`tsgo_checker`/`tsgo_parser` 逻辑。

### TRACK 3（私有名 `#x`）DEFER 说明

blocked-by：私有字段降级需要 (a) **私有环境映射**（`#x` → WeakMap brand 变量），(b) **私有访问表达式重写**（`this.#x` 读/写 → `_brand.get/set(this, …)`，需遍历方法体重写 `PropertyAccessExpression`-with-`PrivateIdentifier`），(c) **WeakMap brand 命名 + 类前 `var _C_x = new WeakMap();` 注入**，(d)（完整形态）`__classPrivateFieldGet/Set` helper-library import emit。这几块互锁且依赖尚未移植的私有访问遍历，留待 6c-4。

## TDD 推进顺序（tracer bullet → 增量）

1. 根 `Transformer` + `Chain`（最小：恒等 visitor 过一个 SourceFile，验证 chain 串联）。
2. `tstransforms::TypeEraser`（**第一个有测试的 stage**，过 `TestTypeEraser` ~70 子用例：类型注解/接口/类型别名/修饰符/类型参数/类型断言擦除 + JSX 类型参数 + verbatimModuleSyntax 分支）。
3. `tstransforms::ImportElision`（过 `TestImportElision` ~20 子用例，需 checker EmitResolver + `MarkLinkedReferencesRecursively`，故依赖 P4 checker 就绪）。
4. `destructuring` + `utilities`（被多 stage 复用，补行为级测试）。
5. estransforms 各 stage（按 target 链，逐 stage 增量；行为由 P10 conformance 兜底，本轮补关键路径行为级测试）。
6. moduletransforms / jsxtransforms / inliners（行为由 P10 兜底）。
7. declarations（最复杂，依赖 nodebuilder/modulespecifiers + symbol tracker；`.d.ts` parity 主要靠 P10）。

## 与 Go 的已知偏离（divergence）

- Go 用 struct 嵌入 `transformers.Transformer` 复用基类方法 → Rust 组合 + 委托（无继承）。
- `TransformerFactory` 返回 `*Transformer`（可 nil 表示该 stage 不适用）→ `Option<Transformer>`；`Chain` 过滤 `None`。
- 子包**作子 module**（见「crate 结构（Round 6 修订）」，覆盖旧的独立 crate 方案）；Go 的包级私有 `new*Transformer` 在 Rust 里是 crate 私有 `fn`，仅版本链/`GetESTransformer` 暴露。
- visitor 改写遍历走 NodeId/arena（PORTING §5）；`VisitEachChild` 语义由 `tsgo_ast::NodeArena::visit_each_child` 提供。
- **[6a]** Go `Transformer` 持共享 `*printer.EmitContext` 指针 → Rust `Rc<RefCell<EmitContext>>`（`SharedEmitContext`，PORTING §3 共享可变指针）；`EmitContext` 自持唯一 `NodeArena`，故 Go 的 `Visitor()`/`Factory()` 访问器折叠进 `emit_context()`（无独立 `NodeVisitor` 对象）。
- **[6a]** transform 的 `visit` 签名：Go `func(*Node) *Node` → Rust `Box<dyn FnMut(&mut EmitContext, NodeId) -> NodeId>`（`VisitFn`）；显式穿 `EmitContext`（arena 在其中）。
- **[6a]** `chainedTransformer` 不经泛型 visitor：合成后的 `visit` 直接顺序调用各 component 的 `run_visit`（复用同一 `&mut EmitContext` 借用），避免 `RefCell` 嵌套借用 panic。
- **[6a]** `ExtractModifiers` 直接过滤 + 重建 `ModifierList`（Rust 泛型 visitor 暂无"返回 nil 删除节点"语义）；行为等价（保留 allowed/非修饰符节点、重算 `ModifierFlags`、保原 list `Loc`）。

## 转交 / 推迟（DEFER）

- `declarations` 的完整 `.d.ts` 生成依赖 `nodebuilder`（P4 已划归 P4）+ `modulespecifiers` + checker resolver；本包仅承接 transform 框架，`.d.ts` 正确性 `// DEFER(phase-10)` 由 conformance baseline 兜底。
- estransforms/moduletransforms/jsxtransforms/inliners Go 侧 0 直接单测 → 行为由 **P10 conformance**（`tsc --target`/`--module`/`--jsx` baseline）兜底（见 tests.md）。
- `ImportElision` 测试需 checker（P4）+ `GetEmitResolver`；若 checker 未就绪，先 `// DEFER`，待 P4 收口后回填。
