# transformers: 实现方案（impl.md）

**crate**：`tsgo_transformers`（根）+ 6 个子包 crate（见下）　**目标**：在 emit 之前对 AST 做语义保持的改写——擦除 TS 专有语法（类型/修饰符/枚举/命名空间）、把高版本 ECMAScript 语法降级到目标版本、模块格式转换（CJS/ESM）、JSX 转换、声明（`.d.ts`）生成。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_printer`（EmitContext/NodeFactory/EmitFlags）`tsgo_binder` `tsgo_checker`(部分子包) 等
**Go 源**：`internal/transformers/`（40 个非测试文件：根 5 + 6 子包 35）

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

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/transformers/transformer.go` | `internal/transformers/lib.rs`（含 `Transformer` 基类，crate 根） | `Transformer`/`NewTransformer`/`TransformSourceFile` |
| `internal/transformers/chain.go` | `internal/transformers/chain.rs` | `TransformOptions`/`TransformerFactory`/`Chain`/`chainedTransformer` |
| `internal/transformers/destructuring.go` | `internal/transformers/destructuring.rs` | `FlattenDestructuringAssignment/Binding` + 绑定/赋值元素工具 |
| `internal/transformers/modifiervisitor.go` | `internal/transformers/modifiervisitor.rs` | `ExtractModifiers` |
| `internal/transformers/utilities.go` | `internal/transformers/utilities.rs` | is-generated/helper/local/export name、绑定模式转换、super 定位、范围移动等共享工具 |

### `tstransforms`（7 文件，含被测的 2 stage）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `tstransforms/typeeraser.go` | `tstransforms/typeeraser.rs` | `TypeEraserTransformer` / `NewTypeEraserTransformer`（**TestTypeEraser 覆盖**） |
| `tstransforms/importelision.go` | `tstransforms/importelision.rs` | `ImportElisionTransformer` / `NewImportElisionTransformer`（**TestImportElision 覆盖**） |
| `tstransforms/runtimesyntax.go` | `tstransforms/runtimesyntax.rs` | enum/namespace → 运行时代码 |
| `tstransforms/legacydecorators.go` | `tstransforms/legacydecorators.rs` | 实验性装饰器降级 |
| `tstransforms/metadata.go` | `tstransforms/metadata.rs` | `emitDecoratorMetadata` |
| `tstransforms/typeserializer.go` | `tstransforms/typeserializer.rs` | 类型 → 元数据表达式；`GetSetAccessorValueParameter` |
| `tstransforms/utilities.go` | `tstransforms/utilities.rs` | 子包共享工具 |

### `estransforms`（17 文件）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `estransforms/definitions.go` | `estransforms/lib.rs` | `GetESTransformer` + 各版本链 `NewES20xxTransformer`（crate 根聚合） |
| `estransforms/async.go` | `async.rs` | `newAsyncTransformer`（async/await 降级） |
| `estransforms/classfields.go` | `classfields.rs` | `newClassFieldsTransformer` |
| `estransforms/classthis.go` | `classthis.rs` | class `this`/`#brand` 辅助 |
| `estransforms/esdecorator.go` | `esdecorator.rs` | `newESDecoratorTransformer`（标准装饰器） |
| `estransforms/exponentiation.go` | `exponentiation.rs` | `newExponentiationTransformer`（`**` → `Math.pow`） |
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

- [ ] `pub struct Transformer` + `new_transformer(visit, context)` + `emit_context()/visitor()/factory()/transform_source_file()`　`// Go: transformer.go:Transformer`
- [ ] `pub struct TransformOptions` + `pub type TransformerFactory` + `pub fn chain(transforms) -> TransformerFactory` + `ChainedTransformer`　`// Go: chain.go:TransformOptions/TransformerFactory/Chain`
- [ ] destructuring：`pub enum FlattenLevel`、`pub type CreateAssignmentCallback`、`FlattenDestructuringAssignment`、`FlattenDestructuringBinding`、`BindingOrAssignmentElementAssignsToName`、`BindingOrAssignmentElementContainsNonLiteralComputedName`、`GetInitializerOfBindingOrAssignmentElement`　`// Go: destructuring.go:*`
- [ ] `ExtractModifiers`　`// Go: modifiervisitor.go:ExtractModifiers`
- [ ] utilities：`IsGeneratedIdentifier/IsHelperName/IsLocalName/IsExportName/IsIdentifierReference`、`ConvertBindingPatternToAssignmentPattern`、`ConvertVariableDeclarationToAssignmentExpression`、`SingleOrMany`、`IsSimpleCopiableExpression`、`IsOriginalNodeSingleLine`、`IsSimpleInlineableExpression`、`FindSuperStatementIndexPath`、`GetSuperCallFromStatement`、`MoveRangePastModifiers`、`MoveRangePastDecorators`、`GetNonAssignmentOperatorForCompoundAssignment`　`// Go: utilities.go:*`

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

### Cargo / crate 接线

- [ ] 7 个 `Cargo.toml`（`tsgo_transformers` + 6 子包 crate；path deps 见决策表）
- [ ] 根 `Cargo.toml` workspace members 追加 7 个 crate
- [ ] 各 crate 根 `lib.rs` 声明 `mod` + `pub use`

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
- 子包 crate 化（见决策）；Go 的包级私有 `new*Transformer` 在 Rust 里是 crate 私有 `fn`，仅版本链/`GetESTransformer` 暴露。
- visitor 改写遍历走 NodeId/arena（PORTING §5）；`VisitEachChild` 语义由 `tsgo_ast` 提供。

## 转交 / 推迟（DEFER）

- `declarations` 的完整 `.d.ts` 生成依赖 `nodebuilder`（P4 已划归 P4）+ `modulespecifiers` + checker resolver；本包仅承接 transform 框架，`.d.ts` 正确性 `// DEFER(phase-10)` 由 conformance baseline 兜底。
- estransforms/moduletransforms/jsxtransforms/inliners Go 侧 0 直接单测 → 行为由 **P10 conformance**（`tsc --target`/`--module`/`--jsx` baseline）兜底（见 tests.md）。
- `ImportElision` 测试需 checker（P4）+ `GetEmitResolver`；若 checker 未就绪，先 `// DEFER`，待 P4 收口后回填。
