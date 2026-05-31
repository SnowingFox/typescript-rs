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
| **6c-4** ✅ 子集 | 收口 `classfields` 可达面：**私有实例字段**（`#x` 直接 WeakMap `.get`/`.set` 形态）+ **计算实例字段名**（key 缓存进类前 temp）| `class C { #x = 1; m() { return this.#x; } }` → `var _C_x = new WeakMap(); class C { constructor() { _C_x.set(this, 1); } m() { return _C_x.get(this); } }`；`class C { [k] = 1 }` → `var _a = k; class C { constructor() { this[_a] = 1; } }` | 全走 arena 既有构造器（`new_new_expression`/`new_variable_statement`/`new_element_access_expression` 等均已存在），**本轮无需 ast/printer 增长**。DEFER：named-helper 形态、私有 static/方法/accessor（WeakSet）、`accessor` 字段、class-expr、param-props、name-generator 唯一命名、target 门控 |
| **6d** ✅ 子集 | `estransforms` part 2：两条**无 helper** 的 es stage：`optionalchain`（`a?.b`→保护性条件表达式）+ `objectrestspread`（对象 spread→`Object.assign`） | `a?.b;` → `a === null \|\| a === void 0 ? void 0 : a.b;`；`const o = { ...x, y };` → `const o = Object.assign(Object.assign({}, x), { y });` | 全走 arena 既有构造器（`new_conditional_expression`/`new_void_expression`/`new_object_literal_expression` 等均已存在），**本轮无需 ast/printer 增长**。DEFER：`namedevaluation`/`using`/`forawait`/`async`（**全部需 helper-library emit 基建**，Rust 侧未移植）+ optionalchain temp-hoist/this-capture + objectrestspread `__rest` 绑定 |
| **6d-2** ✅ | **printer helper-emit 基建**（`EmitHelper` 模型 + `request_emit_helper`/`read_emit_helpers` + 每节点 `add/get_emit_helper` + `compare_emit_helpers` 优先级排序 + `new_unscoped_helper_name` + 源文件 prologue emit `write_lines`），并以 `namedevaluation` tracer 端到端验证 | `var f = function(){}` → prologue `var __setFunctionName = …;` + `var f = __setFunctionName(function () { }, "f");` | additive 增长 `printer`（新增 `emithelpers.rs` + EmitContext helper 存储 + factory `new_unscoped_helper_name` + printer `emit_helpers`/`write_lines`）。解锁 `async`/`forawait`/`using`（6d-3）|
| **6d-3** ✅ 子集 | `estransforms` part 3：**`async`** 顶层函数声明 → `__awaiter` 包装 + `await`→`yield`（复用 6d-2 helper 基建）| `async function f() { await g(); }` → prologue `var __awaiter = …;` + `function f() { return __awaiter(this, void 0, void 0, function* () { yield g(); }); }` | 全走 arena 既有构造器（`new_function_declaration`/`new_function_expression`/`new_yield_expression`/`new_return_statement` 等均已存在），**本轮无需 ast/printer 增长**。DEFER：`forawait`（无最小 tracer，需完整 async-iteration 脚手架）+ `using`（**parser 不解析语句级 `using`**）+ async 方法/箭头/生成器/super/参数 |
| **6e** ✅ 子集 | `moduletransforms`：**`externalmoduleinfo`** 结构化分析（external imports / exported names / `export *` / `export =`）；CJS/ESM 变换 DEFER（基建缺口）| `collect_external_module_info` on parsed modules | 纯 AST 分析，**本轮无需 ast/printer 增长**。CJS/ESM 变换 blocked-by：emit substitution（use 重写）+ 真实 ReferenceResolver（当前 no-op）+ `TransformOptions` 无 compilerOptions |
| **6e-2** ✅ 基建 | 解锁基建 3 轨：**(1) emit 节点替换** `EmitContext` 节点替换表 + printer hook；**(2) `TransformOptions.compiler_options`**（module/jsx）；**(3) 真实 ReferenceResolver → DEFER**（checker 阻塞）。验证：minimal CommonJS `import { x } from "m"; x;` → `const m_1 = require("m"); m_1.x;` | substitution `x`→`m_1.x`；`module: commonjs` gate | additive 增长 `printer::EmitContext`（节点替换表 + `set/get_node_substitution`）+ `printer`（`emit_expression_node` 替换 hook）+ `TransformOptions.compiler_options`。**无 ast 增长**。track 3 blocked-by：checker `resolveName`/`EmitResolver` |
| **6e-3** ✅ 子集 | 基于 6e-2 基建生长 CommonJS 面（export default/const/`{}`/`*` + default/namespace import interop + `__esModule` 标志）+ JSX automatic runtime 选择 | `export default 1;` → marker + `exports.default = 1;`；`import d from "m"; d;` → `__importDefault` + `m_1.default`；`<div/>`（jsx=react-jsx）→ `jsx("div", {})` | 全走 arena 既有构造器 + 6d-2 helper infra（`__importDefault`/`__importStar`/`__exportStar`）+ 6e-2 substitution/compiler_options。**无 ast/printer 增长**。DEFER：`esmodule`（可达面近恒等，实质需 type-elision/interop 注入）、作用域正确解析、re-export、`export =`、动态 import |
| **6f** ✅ 子集 | `jsxtransforms`：**classic runtime** `React.createElement` 元素/属性/子节点/fragment 降级 | `<div id="x">{x}</div>;` → `React.createElement("div", { id: "x" }, x);` | 全走 arena 既有构造器（`new_call_expression`/`new_property_access_expression`/`new_string_literal`/`new_object_literal_expression`/`new_property_assignment`/`new_keyword_expression`/`new_spread_element` 等），**本轮无需 ast/printer 增长**。DEFER：automatic runtime（`jsx`/`jsxs` + implicit import，需 compilerOptions + emitResolver）、自定义 factory/pragma、spread attr、entity 解码 |
| 6f-2 / inliners | `inliners`：const enum 内联 | `const enum E{A=1} E.A`→`1` | checker 求值（const enum 值）|
| 6g | `declarations`：`.d.ts` transform 框架 + tracker + diagnostics | 基础 `.d.ts` 形状 | nodebuilder/modulespecifiers + checker resolver；正确性靠 P10 |
| 6h | 根 `destructuring` + `utilities` 其余（绑定↔赋值转换/super 定位/范围移动）+ `tstransforms/importelision`（checker 就绪后） | 数组解构展开；import elision | factory 构造器 + checker `EmitResolver` |
| **6af** ✅ 子集 | `tstransforms/importelision`：**作用域正确**的未引用 *value* import 省略，消费 checker 4an `EmitResolver::is_referenced`（经新增 additive `EmitReferenceResolver` 句柄）。整声明丢弃（全 binding 未引用）/ 逐 specifier 丢弃（部分引用），镜像 Go `ImportElisionTransformer` 形态 | `import { x } from "m";`（x 未用）→ ∅；`import { x } from "m"; x;` → 原样保留；`import { x } from "m"; function f(){ var x=1; x; }` → import 被省略（内层 `x` 不续命，证明真作用域解析 vs 按名匹配）| 新增 `EmitReferenceResolver`（包 `Rc<dyn BoundProgram>`+`EmitResolver`）作为 `new_import_elision_transformer(opt, resolver)` 的 **additive 入参**（**非** `TransformOptions` 字段——compiler crate 用穷举字面量构造 `TransformOptions`，加字段会破坏其编译且 compiler crate 不在编辑范围）。**无 ast/printer 增长**。DEFER：export 侧 / `import =`（需 `IsValueAliasDeclaration` / `IsTopLevelValueImportEqualsWithEntityName`）、type-only 位置 use 续命 value import、`verbatimModuleSyntax`/`isolatedModules`/`importsNotUsedAsValues` 策略变体、跨文件 alias |
| **6aj** ✅ 子集 | `moduletransforms/commonjsmodule`：**作用域正确的 default & namespace import use-site 重写行为级覆盖**（消费 4an `resolve_reference`，复用 6ai resolver 接线）。6ai 已把 resolver 臂写成对 `ImportBinding.decl` 泛化匹配（named/default/namespace 三类同路径），但仅 named 有行为级测试；本轮补 default/namespace 的行为级覆盖 + scope guard。**无 impl 变更**（6ai 泛化臂已覆盖；观察到 GREEN 而非 RED——见 worklog 诚实记录），仅新增测试。| `import d from "m";\nd;` → `…__importDefault…\nconst m_1 = __importDefault(require("m"));\nm_1.default;`；`import * as ns from "m";\nns;` → `const m_1 = __importStar(require("m"));\nm_1;`；shadow guard：`import d from "m";\nfunction f(){ var d=1; d; }` → 内层 `d` 保持裸（resolve 到局部）| **ZERO** ast/printer/checker 增长，复用 6ai `new_common_js_module_transformer_with_resolver` + `ImportBinding.decl` + `register_import_use_substitutions` resolver 臂；既有公共 API 全未变。DEFER：export 引用重写（`GetReferencedExportContainer`）、shorthand-property 展开、ESM/System 重写、combined default+named interop 边角 |
| **6ak** ✅ 子集 | `moduletransforms/commonjsmodule`：**CommonJS local-export use-site 重写**——消费 checker 4as `EmitResolver::get_referenced_export_container`（CJS 传 `prefix_locals=false`），把顶层**导出变量**的 value-position use 重写成 `exports.<name>`。扩展 6ai 的同一 identifier-substitution 阶段（resolver 臂）：每个 use 先查 export container，命中 SourceFile → 重写 `exports.x`，否则回落到既有 import-binding 匹配（Go `visitExpressionIdentifier` 顺序）。`EmitReferenceResolver` 加 additive `get_referenced_export_container` passthrough。不回归 6e/6w 声明降级。| `export const x = 1;\nx;` → `…__esModule…\nexports.x = void 0;\nexports.x = 1;\nexports.x;`（**genuine RED**：use 原本裸 `x;`）；scope guard：`export const x = 1;\nfunction f(){ const x=2; x; }` → 内层 `x` 保持裸（resolve 到局部，container=None）；non-export guard：`const y = 1;\ny;` → `y` 保持裸 | **ZERO** ast/printer/checker 增长（消费 4as as-is），复用 6ai resolver 接线 + 既有 `exports` 标识符 + `new_property_access_expression` + `set_node_substitution`；`EmitReferenceResolver::get_referenced_export_container` 为 additive passthrough，既有公共 fn 签名全未变。DEFER：exported function/class use-site（4as 的 `ExportHasLocal && !Variable` 守卫返回 None）、namespace/enum export container（4as DEFER）、shorthand-property 展开、ESM/System |

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
| `TransformOptions{ Context, CompilerOptions, Resolver, EmitResolver, GetEmitModuleFormatOfFile }` | `pub struct TransformOptions { context, compiler_options }`（6e-2）；`EmitResolver` 经 **additive `EmitReferenceResolver` 入参**（6af，**非** struct 字段）| 共享配置；`*EmitContext` 跨 stage 复用。⚠️ **不能给 `TransformOptions` 加字段**：`internal/compiler/emitter.rs` 用穷举字面量 `TransformOptions { context, compiler_options }` 构造它，加字段会破坏 `tsgo_compiler` 编译，而 compiler crate 不在本 lane 编辑范围。故 resolver 走新 factory `new_import_elision_transformer(opt, resolver)` 的额外入参（additive，不改既有签名）|
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
| `internal/transformers/destructuring.go` | `internal/transformers/destructuring.rs` | ✅ 6j+6k 子集 | (6j) `flatten_destructuring_binding`（绑定模式，`FlattenLevelAll`：数组/对象/嵌套/默认值/数组 rest/计算键）+ `new_destructuring_transformer` 驱动（镜像 es2015 变量声明降级）+ `FlattenLevel` + 绑定元素谓词。**(6k) `flatten_destructuring_assignment`（赋值模式，复用同一 `Flattener`，`FlattenMode` 判别）**：数组/对象字面量赋值目标 `[a,b]=arr`/`({a,b}=o)`、默认值、嵌套、数组 rest @ `FlattenLevelAll`（经驱动 `ExpressionStatement` 接入，`needs_value=false`，temp `hoistTempVariables=true` 落 var-env）；对象 rest @ `FlattenLevelObjectRest`（经 objectrestspread `visitBinaryExpression` 接入，复用 objectrestspread 的 `__rest` helper builder）。DEFER：`needs_value=true`（值位）+ `InlineExpressions` 逗号折叠的值返回路径、`CreateAssignmentCallback`（CJS export/namespace）、参数/`for-of`/`catch` 位置、exported（`hoistTempVariables=true`）var-env 路径、对象 rest 的非简单 value temp-hoist/重命名/默认/嵌套/计算键 |
| `internal/transformers/modifiervisitor.go` | `internal/transformers/modifiervisitor.rs` | ✅ 6a | `extract_modifiers` |
| `internal/transformers/utilities.go` | `internal/transformers/utilities.rs` | 6a 子集 / 6g 余下 | 6a：generated/helper/local/export-name 谓词 + `get_non_assignment_operator_for_compound_assignment`；余下（绑定模式转换/super 定位/范围移动）DEFER 6g |

### `tstransforms`（7 文件，含被测的 2 stage）

> 子模块 `pub mod tstransforms;` → `tstransforms/mod.rs`（含全部 DEFER + blocked-by 说明）。

| Go 文件 | Rust 文件 | 状态 | 说明 |
|---|---|---|---|
| `tstransforms/typeeraser.go` | `tstransforms/typeeraser.rs` | ✅ 6b+6c-prep | `new_type_eraser_transformer` + 移除式 `type_eraser_visit`（返回 `Option<NodeId>`，`None`=从列表省略）。6b 剥类型纯重建簇；**6c-prep 完成省略**：`interface`/`type`/ambient(`declare`)/`namespace export`/重载 → `NotEmittedStatement`；类型只读修饰符(public/private/…)、`implements`、`this` 参、`declare`/`abstract` 字段 → 移除；`as`/`satisfies`/`<T>x`/`x!` → `PartiallyEmittedExpression`；`import type`/`import type =` → `NotEmittedStatement`。DEFER：每-specifier `import { type x }`、命名空间实例化分析(`IsInstantiatedModule`)、method/ctor/accessor 重载、`compilerOptions` 分支、用法式 import elision（checker）—— 见 mod.rs |
| `tstransforms/utilities.go` | `tstransforms/utilities.rs` | ✅ 6b | `constant_expression`（+`ConstantValue`）：string/number/NaN/±Infinity/负数 → 工厂节点 |
| `tstransforms/importelision.go` | `tstransforms/importelision.rs` | ✅ 6af+6ag+6ah 子集 | `new_import_elision_transformer(opt, resolver)` + 移除式 `import_elision_visit`（返回 `Option<NodeId>`，`None`=省略）。消费 checker `EmitResolver`（经 additive `EmitReferenceResolver`）做**作用域正确**的未引用 value import / type-only export 省略：(import 侧/6af) `ImportSpecifier`/`NamespaceImport` 未引用→丢；`NamedImports` 全丢→`None`（rebuild 用 `NodeList::new` 避免源跨度推断尾逗号）；`ImportClause` 既无 default 名又无 namedBindings→`None`→整 `ImportDeclaration` 省略；side-effect-only `import "m";`（无 clause）不省略。(export specifier 侧/6ag) `ExportSpecifier`/`NamedExports`/`ExportDeclaration` 按 `is_value_alias_declaration` 丢 type-only。(`import =`/`export =` 侧/6ah，消费 4ap) external-module `import x = require("m")` 未引用→丢（`is_referenced_alias_declaration`）；`export = <value>` 留、type-only `export = I` 丢（`is_value_alias_declaration`）。`should_emit_alias_declaration` = `is_referenced`（`IsInJSFile` 短路 DEFER）。DEFER：entity-name `import x = a.b`（需 `IsTopLevelValueImportEqualsWithEntityName`）、跨模块 re-export（需 `resolveExternalModuleSymbol`）、type-only 位置 use 续命、`verbatimModuleSyntax`/`isolatedModules` 策略 —— 见 mod.rs |
| `tstransforms/runtimesyntax.go` | `tstransforms/runtimesyntax.rs` | ✅ 6n 子集 | `new_runtime_syntax_transformer`：**enum → IIFE**（`var E; (function(E){ E[E["A"]=0]="A"; … })(E\|\|(E={}))`：自动编号 + 显式数字初值 auto-续接 + 字符串成员无反向映射 + const enum 省略）+ **instantiated namespace → IIFE**（`export const x=1` → `N.x = 1`；未实例化 type-only namespace 省略，句法 `is_instantiated_module`）。**句法求值替代 checker `GetEnumMemberValue`**；IIFE 容器名用声明名文本（替代 `NewGeneratedNameForNode`）；body 多行经 `MULTI_LINE` emit flag（printer additive）。DEFER：const enum 成员引用 inlining（checker 常量求值）、非字面量初值常量折叠、`E.A`/`N.x` 成员引用重写（binder resolver）、exported/merged enum & namespace、嵌套/点名 namespace、`export =`、参数属性、`import=` 降级 |
| `tstransforms/legacydecorators.go` | `tstransforms/legacydecorators.rs` | ✅ 6al+6ao+6ap+6aq+6ar 子集 | **6ar** 加 **class 装饰器** lowering——`@dec class C {}` → `let C = class C {…}`（保名）绑定 + 尾随 `C = __decorate([dec], C);`（`transform_class_declaration_with_class_decorators`/`get_constructor_decoration_statement`/`generate_constructor_decoration_expression`）；**构造函数参数装饰器** → `__param(i, dec)` 进 **CLASS** `__decorate` 数组（`ClassOrConstructorParameterIsDecorated` 路由 + `first_constructor_with_body` + `append_constructor_param_decorators` + `rebuild_constructor_without_decorators`）；`--emitDecoratorMetadata` 下 class 追加 `design:paramtypes`（`append_class_type_metadata`/`serialize_constructor_parameter_types`，`shouldAddParamTypesMetadata`=有构造体；class **仅**此一项，无 `design:type`/`design:returntype`）。DEFER：class-alias 自引用改写、`export`/`default` 装饰类、`this`-参偏移。`new_legacy_decorators_transformer(opt)` / `new_legacy_decorators_transformer_with_resolver(opt, resolver)`：**属性装饰器**首切片——`@dec x: number;` → 剥装饰器/类型的属性 `x;` + 尾随 `__decorate([dec, …], C.prototype/C, "x", void 0);`（instance→`C.prototype`、static→`C`），经 `SyntaxList` 展开。`design:type` 元数据（`--emitDecoratorMetadata`）经 4at `serialize_type_node_for_metadata` 序列化为 `Number`/`String`/…/`void 0`/`Object`（折叠 Go 的 metadata-注入两-transformer，metadata 居末）。`DECORATE_HELPER`/`METADATA_HELPER` 本 crate 内定义（priority 2/3）。严格 gate on `experimentalDecorators`（+metadata on `emitDecoratorMetadata`）。**6an** 加 `TypeReference design:type` 分流（class→类标识符、interface/unresolved→`Object`）。**6ao** 加**方法装饰器** lowering——`@dec m() {}` → 剥装饰器/返回类型的方法 `m() { }` + 尾随 `__decorate([dec, …], C.prototype/C, "m", null)`（第 4 参 `null` for method vs 属性 `void 0`）；`design:type=Function`（硬编码，无 checker）+ `design:returntype`（返回注解经 checker 序列化、无注解→`void 0`，对每个方法恒发）。**6ap** 加 **`design:paramtypes`**（`serialize_parameter_types` 逐参序列化、无注解→`Object`、0 参→`[]`，插于 `design:type`/`design:returntype` 间）+ **参数装饰器 `__param(i, dec)`**（`PARAM_HELPER` priority 4；`append_param_decorators` 置于成员装饰器后、metadata 前；`member_is_decorated` 经 `NodeOrChildIsDecorated` 收方法的被装饰参数；`rebuild_parameter_without_decorators` 剥参数装饰器/类型）。DEFER：class 装饰器 `let C=…;C=__decorate(...)` 包裹、accessor 装饰器（accessor 合并）、构造函数参数装饰器、`this`-参数偏移 + rest-参数 `GetRestParameterElementType`、async 方法 `design:returntype=Promise`、`TypeReference` 的 lib-globals/qualified-name 类、计算名、混合 instance/static 语句序、求值序边角 —— 见 mod.rs / 模块 doc |
| `tstransforms/metadata.go` | `tstransforms/metadata.rs`（折入 `legacydecorators.rs`） | ✅ 6al+6ao+6ap+6aq+6ar 子集 | **6ar**：class 维度 `design:paramtypes`（`append_class_type_metadata`——`shouldAddParamTypesMetadata`(class)=`GetFirstConstructorWithBody!=nil`；`serialize_constructor_parameter_types` 逐构造参经 checker、跳 `this`、无注解→`Object`；class **仅**发 `design:paramtypes`，无 `design:type`/`design:returntype`），折入 `generate_constructor_decoration_expression`（metadata 居末）。`design:*` 元数据注入折入 `legacydecorators` 单遍（`append_type_metadata` 按 kind 分流）；`shouldAddTypeMetadata`(property+method+accessor) 子集。**6aq**：get/set 访问器臂——`design:type`=`serializeTypeNode(getAccessorTypeNode)`（setter value 参类型优先、否则 getter 返回类型、无注解→`Object`）+ `design:paramtypes`（`shouldAddParamTypesMetadata` 对访问器亦真；`getParametersOfDecoratedDeclaration`：getter 借 set accessor 参、否则自身参），**无** `design:returntype`（`shouldAddReturnTypeMetadata` 仅 method）。**6ao**：method 臂发 `design:type=Function`（硬编码）+ `design:returntype`（`shouldAddReturnTypeMetadata` 对每个方法恒真，有注解经 checker 序列化、否则 `void 0`）。**6ap**：method 臂加 `design:paramtypes`（`serialize_parameter_types` 逐参经 checker 序列化、无注解→`Object`、0 参→`[]`，插于 `design:type` 与 `design:returntype` 之间，对应 `shouldAddParamTypesMetadata`/`serializeParameterTypesOfNode`）。DEFER：`USE_NEW_TYPE_METADATA_FORMAT`、`this`-参数偏移 + rest-参数 `GetRestParameterElementType`、class-typed 构造参 `design:type`（lib-globals/qualified-name，同 `TypeReference` DEFER）。（class `injectClassTypeMetadata` 的 `design:paramtypes` 已于 6ar 落地。）|
| `tstransforms/typeserializer.go` | （checker 4at/4av/4aw `emit_resolver.rs` + `legacydecorators.rs` 的 `serialize_type_node`/`serialize_type_reference_node`/`serialized_type_to_expression`） | ✅ 6al+6am+6an 子集 | 类型→元数据表达式分两段：checker `serialize_type_node_for_metadata` 把 keyword/structural `: T` 注解 → `SerializedTypeNode` 枚举（keyword 子集 4at/4au + 6am/4av 加 `Array`/`Function`）；transformer `serialized_type_to_expression` 把枚举 → AST。**6an**：transformer `serialize_type_node` 把 `KindTypeReference` 臂分流到 `serialize_type_reference_node`，消费 checker 4aw `get_type_reference_serialization_kind`（经 `EmitReferenceResolver` 透传，内持 `Rc<RefCell<Checker>>` 解 `&mut Checker` 线程）：`TypeWithConstructSignatureAndValue`→`serialize_entity_name_as_expression`（类标识符 `C`）、`ObjectType`/`Unknown`→`Object`、其余 kind 1:1 镜像 Go switch（`Number`/`String`/`Boolean`/`BigInt`/`Array`/`Symbol`/`Function`/`Promise`/`void 0`，构建完备但 checker 暂落 `ObjectType` 故不可达）。DEFER：union/intersection/conditional 臂（`serializeTypeNode` 递归）、`Unknown` 的 `typeof`-条件守卫、qualified-name 实体 |

### `estransforms`（17 文件）

> 子模块 `pub mod estransforms;` → `estransforms/mod.rs`（含全部 DEFER + blocked-by 说明）。6c-1 落地 `exponentiation`（tracer）+ `classfields` 子集，**未触碰 ast/printer**（全走 arena 既有构造器）。

| Go 文件 | Rust 文件 | 状态 | 说明 |
|---|---|---|---|
| `estransforms/exponentiation.go` | `exponentiation.rs` | ✅ 6c-1+6c-3+6i | `new_exponentiation_transformer`：`a ** b`→`Math.pow(a, b)`、`a **= b`（标识符）→`a = Math.pow(a, b)`、**`a.x **= b` / `a[x] **= b`（temp hoist）**→`(_a=a).x=Math.pow(_a.x,b)` / `(_a=a)[_b=x]=Math.pow(_a[_b],b)`（ec-threaded + `var` hoist）。**6i** 函数声明体也开各自的变量环境：`function f() { a.x **= b; }` → 体内 `var _a;`（per-scope）。DEFER：`**=` temp-hoist 嵌套在仍未线程化的位置（控制流语句体、本 stage 的方法/函数表达式/箭头、嵌套类）|
| `estransforms/classfields.go` | `classfields.rs` | ✅ 6c-1/2/3/4 + 6o + 6q + 6r 子集 | `new_class_fields_transformer`：实例字段 → 构造器 `this.x = init`（完整构造器插入族）；**static 字段** → 类后 `C.x = init`（`SyntaxList[class, assignment...]`）；**私有实例字段** `#x`（直接 WeakMap 形态）→ 类前 `var _C_x = new WeakMap();` + 构造器 `_C_x.set(this, init)` + 成员体内私有访问重写（`obj.#x`→`_C_x.get(obj)`、`obj.#x = e`→`_C_x.set(obj, e)`）；**计算实例字段名** `[k] = init` → 类前 `var _a = k;` + 构造器 `this[_a] = init`；**(6o) ClassExpression 实例字段** `const C = class { x = 1 }` → `const C = class { constructor() { this.x = 1; } }`（同一降级在表达式位运行，仅当结果为单节点）；**(6r) ClassExpression static 字段语句 hoist** `const C = class { static x = 1 }` → `var _a;\nconst C = (_a = class {}, _a.x = 1, _a)`（逗号序列 + temp 包裹：temp 经 6p `new_temp_variable` 三处复用同名、`add_variable_declaration` hoist `var _a;` 进 SourceFile 变量环境；实例字段进合成构造器、static 进 `_a.x` 表达式协同；core 抽 `lower_class_parts` 受体无关）；**(6q) `accessor` 自动访问器**（实例 + static，类声明 + 类表达式）`accessor x = 1` → backing 私有字段 `#x_accessor_storage = 1` + `get x()`/`set x(value)` 重定向器（**ES2022-native 形态**：backing 字段保留为类成员，不走 WeakMap），backing 名经 **6p** node-based generated private name（`new_generated_private_name_for_node_ex` + `_accessor_storage` 后缀，三处引用同一节点→ emit 时同名）；经 ec-threaded `class_fields_visit_ec`/`visit_each_child_ec` 穿到类节点取得 `EmitContext::factory()`。DEFER：named-helper 私有形态（`__classPrivateFieldGet/Set`）、私有 static 字段、私有方法/accessor（WeakSet）、accessor 的 **WeakMap/下级 target 形态**（二遍 `accessorFieldResultVisitor` 把 backing 私有字段再降级）+ 计算/装饰/混入其他需降级成员的 accessor 类、**ClassExpression 计算名/私有字段语句 hoist**（需 Go `pendingExpressions` 内联，未移植）、嵌套作用域 class-expr static hoist（仅 SourceFile 开变量环境）、参数属性、prologue、匿名类成员、（非 accessor 的）name-generator 唯一命名、`--target`/`useDefineForClassFields` 门控 |
| `estransforms/definitions.go` | `estransforms/lib.rs` | — DEFER(P5) | `GetESTransformer` + 各版本链 `NewES20xxTransformer`（crate 根聚合）；blocked-by：依赖各 es stage 就绪 |
| `estransforms/async.go` | `async.rs` | ✅ 6d-3+6m 子集 | `new_async_transformer`：**async 函数声明**（6d-3）+ **async 函数表达式 / async 方法 / async 箭头**（6m）→ `__awaiter` 包装，body 内 `await X`→`yield X`，请求 `__awaiter` helper（prologue 注入）。6m 经 ec-threaded `visit_each_child_ec`（泛型 `VisitEachChild`，map-based 子节点替换）穿过容器节点（变量声明/类体）到达嵌套函数；非箭头（声明/表达式/方法）有 lexical `this` → 首参 `this` + `{ return __awaiter(...); }` 块包装；箭头 `this` 是 lexical，顶层无 lexical `this` → 首参 `void 0` + 直接以 `__awaiter(...)` 调用作简明体（不包块）。DEFER：async accessor、async 生成器（`__asyncGenerator`，已加守卫不误转）、async 方法内 `super`（需 `_super` 绑定）、`asyncContextHasLexicalThis` 跨嵌套作用域线程化（async 方法内的箭头应线程 `this`）、lexical-`arguments`/`_this` 捕获、默认/rest 参数、`for await`、top-level `await` |
| `estransforms/classthis.go` | `classthis.rs` | — DEFER(P5) | class `this`/`#brand` 辅助 |
| `estransforms/esdecorator.go` | `esdecorator.rs` | — DEFER(P5) | `newESDecoratorTransformer`（标准装饰器）；blocked-by：checker 元数据 + helper emit |
| `estransforms/forawait.go` | `forawait.rs` | ✅ 6y+6z 子集 | `new_for_await_transformer`：**async 生成器函数声明**（6y，ES2018 stage，先于 ES2017 async）`async function* g() {...}` → `function g() { return __asyncGenerator(this, arguments, function* g_1() {...}); }`，body 内 `await x`→`yield __await(x)`、`yield e`→`yield yield __await(e)`、`yield* e`→`yield __await(yield* __asyncDelegator(__asyncValues(e)))`、`return e`→`return yield __await(e)`、bare `yield`→`yield yield __await(void 0)`；inner 生成器名经 `new_generated_name_for_node`（`g`→`g_1`）。**6z** 落地 **`for await (x of y)` downlevel**（async 非生成器函数内）：`transformForAwaitOfStatement` + `convertForOfStatementHead` 完整 async-iteration 脚手架——`__asyncValues(<expr>)` 迭代器 temp + `result` temp + C-style `for`（条件 `result = await iterator.next(), done = result.done, !done`）+ 从 `result.value` 绑定循环变量 + `try/catch/finally` 的 `iterator.return` 清理（非生成器 → downlevel `await`），`done`/`errorRecord`/`returnMethod`/`value` temps 经 plain-function-body 变量环境 hoist 进函数体（新增 `visit_function_declaration` 镜像 optionalchain 6i 函数体处理）。**helper 定义**（`__await`/`__asyncGenerator`/`__asyncDelegator`/`__asyncValues`）因 `tsgo_printer` 出编辑范围而**定义在本 crate**（`forawait.rs` 内 `pub static`，verbatim Go 文本）。**Go/tsc 校正**：(1) 6y briefing 称「`yield x` 保留」是错的——Go/tsc 都是 `yield yield __await(y)`；(2) 6z catch 变量 Go/tsc 为 `e_1_1`（`NewGeneratedNameForNode(errorRecord "e_1")`），但 Rust printer `generate_name_for_node` 读 raw `arena().text()`（"e"）而非 resolving `getTextOfNode`（"e_1"）→ 落 `e_2`（fresh binding，纯 cosmetic 偏离）。DEFER：identifier 源 `for await (const x of y)`（iterator/result 名 derive 自 identifier，需 resolving `getTextOfNode` 解析嵌套 generated-name；非 identifier `gen()` 源用干净 `NewTempVariable`）、async 生成器内 `for await`（需 `createDownlevelAwait` 的 `yield __await` 形 = enclosing-function-flags 线程化）、destructuring 循环变量、top-level `for await`、label/`continue`/`break`、nested-loop `errorRecord` reset；async 生成器**方法/函数表达式/箭头**（需 `_super` + `hasLexicalThis`）、非简单参数列表、变量环境 merge、top-level await。blocked-by：printer resolving 名生成 + enclosing-function-flags 线程化 + destructuring flattener + `EmitContext` super-capture/参数机器 |
| `estransforms/logicalassignment.go` | `logicalassignment.rs` | — DEFER(P5) | `newLogicalAssignmentTransformer`（`&&=`/`\|\|=`/`??=`） |
| `estransforms/namedevaluation.go` | `namedevaluation.rs` | ✅ 6d-2 子集 | `new_named_evaluation_transformer`：`var f = <匿名函数>` → `var f = __setFunctionName(<fn>, "f")` + prologue 注入 helper 定义（6d-2 emit-helper 基建的端到端验证）。DEFER：完整 `isNamedEvaluation` 面（property/shorthand/参数/binding/属性声明/export=、计算名 `__propKey` 缓存、匿名类 `static { __setFunctionName(this,…) }`）+ `EmitContext.AssignedName` 跟踪 + target/useDefine 门控 |
| `estransforms/nullishcoalescing.go` | `nullishcoalescing.rs` | — DEFER(P5) | `newNullishCoalescingTransformer`（`??`） |
| `estransforms/objectrestspread.go` | `objectrestspread.rs` | ✅ 6d + 6g 子集 | `new_object_rest_spread_transformer`：(6d) 对象 spread `{ ...x, y }` → `Object.assign(Object.assign({}, x), { y })`（chunk + pairwise 折叠，`NewAssignHelper` = `Object.assign` 直出，无需 helper import）；**(6g) 对象 rest 绑定**（变量声明）`var { a, ...rest } = o;` → `var { a } = o, rest = __rest(o, ["a"]);`（ec-threaded：`__rest` helper request + 源文件边界 attach + prologue emit；leading 简单标识符/重命名绑定保留为对象绑定模式 decl，rest key 排除，简单 init 复用）；**(6k) 对象 rest 赋值** `({ a, ...r } = o);` → `({ a } = o, r = __rest(o, ["a"]));`（`ExpressionStatement` 臂 gate `reachable_object_rest_assignment` → 经根包 `flatten_destructuring_assignment` @ `FlattenLevelObjectRest`，**统一走泛型 flattener**，共享本文件 `new_rest_helper`（`pub(crate)`）；保留外层括号避免 `{` 被当块解析）。DEFER：泛型 `FlattenDestructuringBinding`（嵌套/数组模式、默认值、计算键需 temp 缓存、非简单 init temp）= **✅ 6j `destructuring.go`**（绑定模式 `FlattenLevelAll`）；参数/`for-of`/`catch` 位置（需参数/赋值 flattener 接线）；对象 rest 赋值的非简单 value temp-hoist/重命名/默认/嵌套/计算键 |
| `estransforms/optionalcatch.go` | `optionalcatch.rs` | — DEFER(P5) | `newOptionalCatchTransformer` |
| `estransforms/optionalchain.go` | `optionalchain.rs` | ✅ 6d+6h+6i+6s+6t 子集 | `new_optional_chain_transformer`：`a?.b` / `a?.[x]` / `a?.()` / `a?.b()` / `a?.b.c` → `a === null \|\| a === void 0 ? void 0 : <访问/调用>`（`flatten_chain` 折叠单 `?.` + 尾随非可选段，简单 receiver）；**6h** ec-threaded 后非简单 receiver hoist temp（`f()?.b` → `var _a; (_a = f()) === null \|\| _a === void 0 ? void 0 : _a.b`）+ 多 `?.`（`a?.b?.c` → 嵌套守卫、每链一 temp）；**6i** per-scope 变量环境：emit-context 线程化穿过函数体（函数声明/表达式、箭头体、类方法），每个开各自 var-env，temp 落在该作用域而非模块顶（箭头简明体 hoist temp 时包成 block）；**6s** 括号可选调用 `this`-capture（`(a?.b)()` → `(… ? void 0 : a.b).call(a)`，新增 `SyntheticReferenceExpression` AST 节点 + `new_function_call_call`）+ `delete a?.b`（→ `a === null \|\| a === void 0 ? true : delete a.b`，含括号变体 `delete (a?.b)`）；**6t** 可选 call 段 `leftThisArg` 线程化：首段为 call 时 receiver 走 `this`-capture（`is_call_chain(chain[0])`），captured `this` 经 `_t.call(thisArg, …)` 线程化——`a?.b?.()` → `var _a; (_a = a === null \|\| a === void 0 ? void 0 : a.b) === null \|\| _a === void 0 ? void 0 : _a.call(a)`、`a.b?.()`（非可选 member receiver）→ `var _a; (_a = a.b) === null \|\| _a === void 0 ? void 0 : _a.call(a)`、嵌套 `a?.b.c?.()` → temp this `_b.call(_a)`（leftThisArg 为 auto-gen temp 不 clone，普通 identifier clone+`NO_COMMENTS`）。DEFER：嵌套在仍未线程化位置（控制流语句体 `if`/`for`/`while`、`switch` case、对象字面量方法简写、构造器/accessor 体）的 temp-hoist；call 段 `super` receiver（`super.b?.()` 需 super→this 改写）、tagged template |
| `estransforms/taggedtemplate.go` | `taggedtemplate.rs` | — DEFER(P5) | `newTaggedTemplateLiftRestrictionTransformer` |
| `estransforms/usestrict.go` | `usestrict.rs` | — DEFER(P5) | `NewUseStrictTransformer` |
| `estransforms/using.go` | `using.rs` | — DEFER(parser) | `using`/`await using` → try/finally + dispose；`__addDisposableResource`/`__disposeResources` helper 已就绪、transform 可移植，但 **`tsgo_parser` 不解析语句级 `using x = expr;`**（报 "';' expected"），无法走 parse→transform→emit；parser 不在本轮编辑范围。blocked-by：parser `using` 声明支持（`await using` 另需 async 处置）|
| （无 Go 文件，upstream `es2015.ts`）| `spread.rs` | ✅ 6aa 子集 | `new_spread_transformer`：ES2015 **数组字面量 spread** + **调用参 spread** 降级（pre-ES2015 目标，经 `__spreadArray` helper + `.apply`）。**Go 端无 ES2015 spread transform**（`GetESTransformer` 链止于 `NewES2016Transformer`，更老目标 fall through 到它，不降级 spread），ground truth = `tsc --target es5` 对拍。数组：`transform_and_spread_elements`(arg-list=false) 把元素分段（连续非 spread 收成 `[...]` 字面量段、每个 spread 独立段），累加器起于 `[]`（首段为 spread）或首字面量段，逐段 `__spreadArray(acc, seg, pack)`，`pack = is_spread && !arg_list`（数组 spread 段 `true`、字面量段 `false`）：`[...a, b]` → `__spreadArray(__spreadArray([], a, true), [b], false)`、`[...a]` → `__spreadArray([], a, true)`、`[1, ...a, 2]` → `__spreadArray(__spreadArray([1], a, true), [2], false)`。调用：`try_lower_call_with_spread` → `<target>.apply(<this>, <args>)`，`<args>` = `transform_and_spread_elements`(arg-list=true)（arg-list spread 段 `pack=false`；单段捷径直接传 bare 段表达式，无 helper）；标识符 callee `f(...args)` → `f.apply(void 0, args)`、简单成员 callee `o.m(...args)` → `o.m.apply(o, args)`（标识符受体复用为 `this`，无 temp）、`f(a, ...args)` → `f.apply(void 0, __spreadArray([a], args, false))`。emit-helper 复用 6d-2 基建（`request_emit_helper` + 源文件 prologue attach，同 `objectrestspread`/`forawait`）；`SPREAD_ARRAY_HELPER` 定义在 `spread.rs`（`tsgo_printer` 出编辑范围，verbatim tsc 文本）。**briefing 校正**：briefing 称 `f(...args)` → `f.apply(void 0, __spreadArray([], args, false))`，实测 tsc 为 `f.apply(void 0, args)`（arg-list 单 spread 捷径）。DEFER：`new C(...args)`（construct + `C.bind.apply(...)`）、`super(...args)`、非简单成员受体 capture temp（`a.b.m(...args)` → `(_a = a.b).m.apply(_a, args)`）、`--downlevelIteration`（`__read`/`__spread`）、wiring 进 `GetESTransformer`（无 `NewES2015Transformer` 链）。blocked-by：`new`-target bind 形、`super` 受体捕获、`createCallBinding` temp-capture、迭代 helper、`definitions` dispatch 端口 |
| `estransforms/utilities.go` | `utilities.rs` | 子包共享工具 |

### `moduletransforms`（5 文件）

> 子模块 `pub mod moduletransforms;` → `moduletransforms/mod.rs`。6e 落地 `externalmoduleinfo`（结构化分析），**未触碰 ast/printer**（纯 AST 分析）。

| Go 文件 | Rust 文件 | 状态 | 说明 |
|---|---|---|---|
| `moduletransforms/externalmoduleinfo.go` | `externalmoduleinfo.rs` | ✅ 6e 子集 | `collect_external_module_info`：扫描顶层语句收集 **external imports**（`import …`/`export … from`）、**exported names**（`export { x }`、`export const`）、**`export *`** 标志、**`export =`**。DEFER：resolver 相关分类（`export {x}` 的 function-vs-binding via `GetReferencedImportDeclaration`）、`exportedBindings`/`exportedFunctions`、external-helpers import 创建（需 `GetExternalHelpersModuleName`）|
| `moduletransforms/commonjsmodule.go` | `commonjsmodule.rs` | ✅ 6e-2/6e-3 子集 | `new_common_js_module_transformer`（`module: commonjs`）：named/default/namespace import → `require`(+`__importDefault`/`__importStar`) + use 重写（`m_1.x`/`m_1.default`/`m_1`，节点替换）；`export default`/`export const`/`export {x}` → `exports.…`；`export * from` → `__exportStar`；有 value export 时顶部 `__esModule` 标志；**动态 `import("m")`/`import()`**（6ad）→ `Promise.resolve().then(() => __importStar(require(...)))`（Go **无条件** importStar 包裹，arrow 回调；inlineable 参数内联，no-arg → `require()`）。DEFER：动态 import 的 `needSyncEval` 模板形（非 inlineable 参数）+ 端到端 parse（解析器 DEFER `import(...)` call head）、`"use strict"`/作用域正确 use 重写（按名匹配，需真实 ReferenceResolver）|
| `moduletransforms/esmodule.go` | `esmodule.rs` | ✅ 6u+6ab 子集 | `new_es_module_transformer`（`--module es2015/esnext`）：`visitSourceFile` 守卫（`IsDeclarationFile \|\| !(IsExternalModule \|\| isolatedModules)` → 原样返回）+ value import/export **原样保留**（ESM 保留 import/export 语法）；`export = x` 在非 preserve 下 **elide**；`import x = require("m")`（emit module kind `< Node16`）**elide**；`createEmptyImports`（仍是 external module、非 preserve、无 indicator 剩余 → 追加 `export {};`）；**6ab**：`export * as ns from "m"` 命名空间 re-export 在 `Module <= ES2015` 改写为 `import * as ns_1 from "m";` + `export { ns_1 as ns };`（名为 `default` → `export default default_1;`），`esnext` 透传（`new_generated_name_for_node`/6p，tsc 对拍）。复用 6e `collect_external_module_info` 的同源 `IsExternalModuleIndicator` 逻辑。DEFER：**type-only import elision**（blocked-by checker `EmitResolver`）、**作用域正确引用重写**（blocked-by 真实 `ReferenceResolver`）、`--module preserve` 的 `export =`→`module.exports`、`--rewriteRelativeImportExtensions`、动态 `import()` 重写、Node16+ `import =`→同步 require、external-helpers（tslib）注入、`export * as "default"`（string-literal 名）|
| `moduletransforms/impliedmodule.go` | `impliedmodule.rs` | ✅ 6ac 子集 | `new_implied_module_transformer`：按文件 emit module format 分派——`is_es_module_format(format)`（`format >= ModuleKind::Es2015`）→ `new_es_module_transformer`，否则 → `new_common_js_module_transformer`；`IsDeclarationFile` → 原样返回。分派谓词消费 `compiler_options.module`（per-file `impliedNodeFormat` DEFER，blocked-by `SourceFileMetaData` 未接线，同 compiler P6-2）。委托经子 transformer 的 `run_visit`（复用同一 `EmitContext` 借用）。DEFER：per-file `impliedNodeFormat`/`.cjs`/`.mjs` 探测（需 `SourceFileMetaData`）、AMD/UMD/System 格式。 |
| `moduletransforms/systemmodule.go` | `systemmodule.rs` | ✅ 6ae 子集 | `new_system_module_transformer`（`module: system`）：把整个模块体包进 `System.register([<deps>], function (exports_1, context_1) { "use strict"; return { setters: [<setters>], execute: function () { <body> } }; })`。**register wrapper**：两个生成参数名 `exports_1`/`context_1`（`new_unique_name`，对拍 tsc `createUniqueName`）；外层 module body block 设 `MULTI_LINE`（Go `multiLine: true`），return 对象 / 空 execute 块走单行（Rust 打印器对 list-bearing 字面量不携带 per-node `MultiLine`，见 `emit_expressions.rs`）。**dependency list**：每个 external import（复用 6e `collect_external_module_info`）的 module specifier → `System.register` 依赖数组 + `setters` 里一个**空体** setter（`function (_1) { }`，binding-less import 的 param Go 用 `createUniqueName("")`=`_1`；Rust name generator 空 base 路径偏离丢前导 `_`，故传 `"_"` 复现 `_1`，TODO(port) 对齐）。**execute body**：顶层 value 语句（非 import/export 语法）按源序移入 `execute` 体。DEFER（均 blocked-by 真实 `ReferenceResolver`，同 CJS 缺口）：export-setter 接线（named export → `exports({...})`、setter 体转发 import binding）、import binding 重写 / hoisting / live bindings、`var __moduleName = context_1 && context_1.id;`、module-name 首参（`System.register("name", …)`）、`export *` star helper、dependency 分组/去重。|
| `moduletransforms/utilities.go` | `utilities.rs` | — DEFER(P5) | 子包共享工具（`getExternalModuleNameLiteral`/`rewriteModuleSpecifier` 等，多依赖 resolver/compilerOptions）|

### `jsxtransforms` / `inliners` / `declarations`

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `jsxtransforms/jsx.go` | `jsxtransforms/jsx.rs` | ✅ 6f + 6e-3 子集：`new_jsx_transformer`（classic `React.createElement`；**6e-3** automatic runtime 选择经 `compiler_options.jsx`：`<div/>`→`jsx("div", {})`）；DEFER automatic 的 children-in-props/`jsxs`/implicit import 注入、pragma、spread-attr |
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
- [x] destructuring（6j 子集）：`pub enum FlattenLevel`、`pub fn flatten_destructuring_binding`（绑定模式）、`new_destructuring_transformer` 驱动、绑定元素读取器/谓词（target / initializer / rest-indicator / elements / property-name / assigns-to-name / non-literal-computed-name）　`// Go: destructuring.go:FlattenDestructuringBinding/flattenDestructuringBinding`　**[6j]**
- [x] destructuring（6k 子集）：`flatten_destructuring_assignment`（赋值模式，复用 6j `Flattener` + `FlattenMode` 判别 + `emit_assignment`/`inline_expressions` 逗号折叠）、数组/对象/嵌套/默认/数组-rest @ `FlattenLevelAll`（驱动 `ExpressionStatement` 接入）、**对象 rest @ `FlattenLevelObjectRest` 经泛型 flattener**（objectrestspread `visitBinaryExpression` 接入，共享 `__rest` helper builder）　`// Go: destructuring.go:FlattenDestructuringAssignment`　**[6k]**
- [ ] destructuring（余下）：`pub type CreateAssignmentCallback`（CJS export/namespace 回调）、`needs_value=true` 值返回路径（`InlineExpressions` 尾随值折叠）、参数/`for-of`/`catch` 解构位置、exported var-env 路径、对象 rest 赋值的非简单 value temp-hoist/重命名/默认/嵌套/计算键（`computedTempVariables`）　`// Go: destructuring.go:FlattenDestructuringAssignment`　**DEFER** blocked-by：参数/for-of 接线 + checker-free 计算键 temp 缓存 + 值位 needs_value 接线
- [x] `extract_modifiers`　`// Go: modifiervisitor.go:ExtractModifiers`　**[6a]** — 直接过滤实现（visitor 暂无 node 删除语义），不经泛型 visitor
- [x] utilities（6a 子集）：`is_generated_identifier/is_helper_name/is_local_name/is_export_name`、`get_non_assignment_operator_for_compound_assignment`　`// Go: utilities.go:*`　**[6a]**
- [ ] utilities（余下）：`IsIdentifierReference`、`ConvertBindingPatternToAssignmentPattern`、`ConvertVariableDeclarationToAssignmentExpression`、`SingleOrMany`、`IsSimpleCopiableExpression`、`IsOriginalNodeSingleLine`、`IsSimpleInlineableExpression`、`FindSuperStatementIndexPath`、`GetSuperCallFromStatement`、`MoveRangePastModifiers`、`MoveRangePastDecorators`　`// Go: utilities.go:*`　**[6g]** blocked-by：factory 节点构造器 / `ast::is_string_literal_like`·`is_numeric_literal`·`skip_parentheses`·`is_super_call`·`can_have_modifiers` 等谓词未移植

### `tstransforms`（重点：被测 2 stage 先行）

- [ ] `pub struct TypeEraserTransformer` + `pub fn new_type_eraser_transformer(opt) -> Transformer`　`// Go: typeeraser.go:NewTypeEraserTransformer`（**先做：过 TestTypeEraser**）
- [x] `pub fn new_import_elision_transformer(opt, resolver) -> Transformer`（6af+6ag+6ah 子集）+ 移除式 `import_elision_visit`　`// Go: importelision.go:NewImportElisionTransformer`。消费 checker `EmitResolver`（经 additive `EmitReferenceResolver`）：6af 作用域正确未引用 value import 省略；6ag export specifier 侧（`is_value_alias_declaration`）；6ah external-module `import x = require("m")`（`is_referenced_alias_declaration`）+ `export =`（`is_value_alias_declaration`）。DEFER：entity-name `import x = a.b`（需 `IsTopLevelValueImportEqualsWithEntityName`）、跨模块 re-export（需 `resolveExternalModuleSymbol`）、type-only 位置续命、策略变体、`TestImportElision` 全表（~20 子用例）
- [x] `new_runtime_syntax_transformer`（6n 子集：enum → IIFE + instantiated namespace → IIFE；见 6n worklog）　`// Go: runtimesyntax.go:NewRuntimeSyntaxTransformer`
- [x] `new_legacy_decorators_transformer` / `new_legacy_decorators_transformer_with_resolver`（6al 子集：属性装饰器 + `design:type` 元数据，消费 4at `serialize_type_node_for_metadata`；metadata 注入折入单遍，见 6al worklog；**6an** 加 `TypeReference design:type` 分流——`serialize_type_node`/`serialize_type_reference_node` 消费 4aw `get_type_reference_serialization_kind`，class→类标识符、interface/unresolved→`Object`；**6ao** 加**方法装饰器** lowering——`rebuild_method_without_decorators` + `append_type_metadata` method 臂：`__decorate([…], C.prototype/C, "m", null)`（第 4 参 `null`）+ `design:type=Function`（硬编码）+ `design:returntype`（恒发，注解经 checker 序列化/否则 `void 0`）；**6ap** 加 `design:paramtypes`（`serialize_parameter_types`，逐参序列化/无注解→`Object`/0 参→`[]`）+ 参数装饰器 `__param(i, dec)`（`PARAM_HELPER`/`append_param_decorators`/`member_is_decorated`/`rebuild_parameter_without_decorators`）；**6aq** 加**访问器（get/set）装饰器** lowering——`rebuild_accessor_without_decorators` + `accessor_owns_decoration`（`getAllAccessorDeclarations` 配对 + `firstAccessorWithDecorators` 归属，get/set 对合并为**单** `__decorate([…], C.prototype/C, "x", null)`，第 4 参 `null`）+ `append_type_metadata` 访问器臂：`design:type`（`accessor_type_node`：setter value 参类型 / getter 返回类型，无注解→`Object`）**且** `design:paramtypes`（`serialize_accessor_parameter_types`：getter 借 setter 参 / 自身参，无参→`[]`），**无** `design:returntype`（`shouldAddReturnTypeMetadata` 仅 method））；**6ar** 加 **class 装饰器** lowering——`transform_class_declaration_with_class_decorators`（`let C = class C {…}`（保名）绑定 + 成员装饰语句 + 尾随 class `__decorate`）+ `get_constructor_decoration_statement`/`generate_constructor_decoration_expression`（class 装饰器 ++ 构造参 `__param` ++ class `design:paramtypes` metadata）+ `ClassOrConstructorParameterIsDecorated` 路由（构造参装饰类也走包裹）+ `rebuild_constructor_without_decorators`　`// Go: legacydecorators.go:NewLegacyDecoratorsTransformer / getAllDecoratorsOfAccessors / transformClassDeclarationWithClassDecorators / getConstructorDecorationStatement / metadata.go:getOldTypeMetadata / getAccessorTypeNode / typeserializer.go:serializeParameterTypesOfNode`。DEFER：class-alias 自引用改写、`export`/`default` 装饰类、计算/私有访问器名、accessor 参数装饰器、`this`-参数体擦除 + rest-参数类型、async 方法/访问器 `Promise` 返回、`TypeReference` 的 lib-globals/qualified-name 类
- [x] `set_accessor_value_parameter` / `set_accessor_type_annotation_node` / `accessor_type_node` / `parameters_of_decorated_accessor` / `find_accessor_pair` / `accessor_owns_decoration` / `serialize_accessor_parameter_types`（6aq accessor 装饰器维度：get/set 配对、归属、`design:type`/`design:paramtypes` 来源）　`// Go: metadata.go:GetSetAccessorValueParameter / getSetAccessorTypeAnnotationNode / getAccessorTypeNode / ast/utilities.go:GetAllAccessorDeclarations / typeserializer.go:getParametersOfDecoratedDeclaration`
- [x] `transform_class_declaration_with_class_decorators` / `rebuild_class_member` / `rebuild_constructor_without_decorators` / `class_has_decorator` / `class_or_constructor_parameter_is_decorated` / `first_constructor_with_body` / `constructor_has_decorated_parameter` / `class_decorator_expressions` / `get_constructor_decoration_statement` / `generate_constructor_decoration_expression` / `append_constructor_param_decorators` / `append_class_type_metadata` / `serialize_constructor_parameter_types`（6ar class 装饰器维度：`let`-wrap 包裹、构造参 `__param`、class `design:paramtypes`）　`// Go: legacydecorators.go:transformClassDeclarationWithClassDecorators / getConstructorDecorationStatement / generateConstructorDecorationExpression / getAllDecoratorsOfClass / visitConstructorDeclaration / ast/utilities.go:ClassOrConstructorParameterIsDecorated / GetFirstConstructorWithBody / metadata.go:shouldAddParamTypesMetadata`

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

### TRACK 3（私有名 `#x`）DEFER 说明 → 6c-4 已落地直接 WeakMap 形态

6c-3 的 (a)(b)(c) 已在 6c-4 落地（见 6c-4 worklog）；(d) named-helper 形态仍 DEFER（需 helper-library import emit）。

## 6c-4 worklog（red→green 推进记录）

收口 `classfields` 可达面，逐子用例红→绿（6c-1/2/3 全部 10 - 4 = 6 个旧用例保持绿）：

**私有实例字段 `#x`（直接 WeakMap 形态）**
1. `private_field_initializer_uses_weakmap_set`（tracer，仅写）：`class C { #x = 1 }` → 恒等 `class C {\n    #x = 1;\n}` 红 → 实现 `build_private_env`（首遍扫描成员构建 `#x`→`_C_x` brand 映射；私有方法/accessor 或匿名类有私有成员 → 返回 `None` 整类延后）、`make_weakmap_brand_var`（`var _C_x = new WeakMap();`）、`make_private_set`（构造器 `_C_x.set(this, init)`），经 `SyntaxList[brand-var, class]` 返回 → 绿。
2. `private_field_read_uses_weakmap_get`：`m() { return this.#x; }` → 实现 `lower_private_access`（递归重写成员体内私有访问）+ `make_private_get`（`obj.#x` → `_C_x.get(obj)`）→ 红→绿。
3. `private_field_write_uses_weakmap_set`：`m(v) { this.#x = v; }` → 在 `lower_private_access` 顶部先拦截 `=` 赋值（`obj.#x = e` → `_C_x.set(obj, e)`，避免左值被当读取降为 `.get`）+ `make_private_set_call`（receiver-general）→ 红→绿。

**计算实例字段名**
4. `computed_field_name_is_hoisted_to_temp`：`class C { [k] = 1 }` → 恒等红 → 实现计算名分支：key 缓存进类前 `var _a = k;`（`make_temp_var` + `computed_temp_name` 确定性 `_a`/`_b`/… 命名），构造器内 `this[_a] = 1`（`make_this_element_assignment`，元素访问）→ 绿。语义关键：计算 key 必须在**类定义时求值一次**（不能内联到构造期重复求值），故 hoist 到 temp 而非内联 `this[k]`。

**测试计数（6c-4 新增）**：`tsgo_transformers` +4 `#[test]`（私有字段 写/读/写 3 + 计算名 1）。crate 合计 48 unit + 12 doctest。

### upstream（ast/printer）增长（6c-4）

- **无**。私有字段 + 计算名降级全部用 arena 既有构造器（`new_new_expression`/`new_variable_statement`/`new_variable_declaration(_list)`/`new_element_access_expression`/`new_property_access_expression`/`new_call_expression`/`new_keyword_expression`/`new_binary_expression`/`new_token`/`new_expression_statement`/`new_class_like`/`new_syntax_list`）+ `visit_each_child` 默认递归即可。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## classfields 移植状态（6c-1..6c-4 收口）

**已移植（直接、行为正确的 down-level 子集）**：
- 实例字段初始化器 → 构造器 `this.x = init`（6c-1）。
- 完整构造器插入族：合成构造器、既有构造器插入、`extends` 合成 `super(...arguments)`、既有 `super()` 后插入（6c-2）。
- static 字段 → 类后 `C.x = init`（`SyntaxList`，6c-3）。
- 私有实例字段 `#x`（直接 WeakMap `.get`/`.set` 形态）：类前 `var _C_x = new WeakMap();` + 构造器 `.set` + 成员体内私有读/写重写（6c-4）。
- 计算实例字段名：key 缓存进类前 temp + 构造器 `this[_temp] = init`（6c-4）。
- **ClassExpression 实例字段**（6o）：`const C = class { x = 1 }` → `const C = class { constructor() { this.x = 1; } }`。纯实例字段降级结果为单节点（无 hoist），在表达式位安全。
- **ClassExpression static 字段语句 hoist**（6r）：`const C = class { static x = 1 }` → `var _a;\nconst C = (_a = class {}, _a.x = 1, _a)`。受体无关的 `lower_class_parts` + ec 线程化 `try_lower_class_expression`：temp 经 6p `new_temp_variable`（三处复用同名）、`add_variable_declaration` hoist `var _a;` 进 SourceFile 变量环境；逗号序列在 `const` 初值位由 printer `DISALLOW_COMMA` 优先级自动加括号。实例+static 混合亦可达（实例字段进合成构造器，static 进 `_a.x`）。计算名/私有字段在表达式位仍延后（需 `pendingExpressions`）。

**DEFER（→ P10 / checker / name-generator / helper-emit）**：
- named-helper 私有形态 `__classPrivateFieldGet/Set(this, _C_x, …, "f")`（需 helper-library import emit）。
- 私有 **static** 字段、私有 **方法/accessor**（`WeakSet` brand check）。
- `accessor` 自动访问器（→ 合成私有 backing 字段 + get/set 重定向器；需 emit-context name generator 生成 backing 私有名 + 二遍 result visitor）。
- **ClassExpression 语句 hoist**（6o 纯实例字段 + 6r static 字段已落地）：**剩余** 计算名/私有实例（WeakMap）/私有 static 字段在表达式位需 Go `visitClassExpression` 的 `pendingExpressions` 内联进逗号序列；本端口计算名/私有走确定性命名 + `SyntaxList` 语句，机制不兼容，未移植。嵌套作用域（函数体/块）的 class-expr static hoist 亦待按 6i 模式逐作用域线程化变量环境。
- **参数属性** `constructor(public x)`（属 `tstransforms` 的 TS-only 降级，非 estransforms）。
- 构造器 **prologue 指令** 顺序、**匿名类** static/私有成员（需生成类名）。
- name-generator 支撑的 **temp/brand 唯一命名**（当前 `_C_x` / `_a` 为确定性命名，存在理论碰撞）。
- 私有访问重写当前仅覆盖**保留成员体**（方法/accessor）；**既有构造器体**与**字段初始化器内**的私有访问（`constructor(){ this.#x = 2 }`、`#y = this.#x`）尚未重写（无测试驱动，避免投机实现）。
- `--target` / `useDefineForClassFields` **门控**（需 compilerOptions + checker）。

## 6d worklog（red→green 推进记录）

`estransforms` part 2。逐 stage 评估后发现 **6 个 stage 中仅 2 个无需 helper-library emit**，故只落地这两个；其余 4 个被同一基建缺口阻塞。

**stage 1 — `optionalchain`（无 helper，全落地）**
1. `optional_property_access_lowered`（tracer）：`a?.b;` → 恒等红 → 实现 `optional_chain_visit`（命中 `OPTIONAL_CHAIN` flag 的 PropertyAccess）+ `create_not_null_condition`（`left === null || right === void 0`，invert 形态）+ `make_void_zero` → `a === null || a === void 0 ? void 0 : a.b;` → 绿。
2. `optional_element_access_lowered`：`a?.[x];` → 加 ElementAccess 臂 → 绿。
3. `optional_call_lowered`：`a?.();` → 重构为统一的 `try_lower_optional_expression`（`flatten_chain` 折叠 receiver + 段序列 + `is_simple_copiable` 守卫）→ 绿。
4. 回归补 `optional_method_call_lowered`（`a?.b()`）+ `optional_chain_trailing_property_lowered`（`a?.b.c`）：单 `?.` + 尾随非可选段经 flatten 自然组合 → 直接绿（验证泛化）。

**stage 2 — `objectrestspread`（无 helper，spread 子集全落地）**
5. `object_spread_only_lowers_to_assign`（tracer）：`const o = { ...x };` → 恒等红 → 实现 `chunk_object_literal_elements`（非 spread 段 chunk 成对象字面量、spread 段就地）+ `visit_object_literal_expression`（首段非对象字面量则前置 `{}`，pairwise 折叠）+ `assign_helper`（`Object.assign(...)` 直出）→ `Object.assign({}, x)` → 绿。
6. `spread_then_property_chunks_pairwise`（`{ ...x, y }` → `Object.assign(Object.assign({}, x), { y })`）+ `property_then_spread_uses_chunk_as_target`（`{ a, ...x }` → `Object.assign({ a }, x)`）：chunk 逻辑既已完整 → 直接绿（验证 chunk 边界）。

**stage 3–6 — `namedevaluation` / `using` / `forawait` / `async`：DEFER（同一硬阻塞）**

经核查 Go 源：这 4 个 stage 的**每条输出路径**都引用 TS runtime helper（`__setFunctionName`/`__propKey`；`__addDisposableResource`/`__disposeResources`；`__asyncValues`/`__await`；`__awaiter`/`__generator`），且都经 `RequestEmitHelper` + `NewUnscopedHelperName` 生成。**Rust 侧 printer 尚无 helper-library emit 基建**（`internal/printer/{helpers,emitcontext,factory}.go` 的 `RequestEmitHelper`/unscoped helper-name/helper 定义 prologue 均未移植 — grep 确认 `.rs` 侧零命中）。因此这 4 个 stage **没有 helper-free tracer**，强行用普通标识符伪造 `__awaiter(...)` 会产出引用未定义 helper 的语义不完整代码，违反 TDD「只写当前测试所需的最小实现」。→ 全部 DEFER，建议先做一轮 **printer helper-emit 基建**（暂记 6d-2）再回来。

**测试计数（6d 新增）**：`tsgo_transformers` +8 `#[test]`（optionalchain 5 + objectrestspread 3）+2 doctest（两个 `new_*_transformer`）。crate 合计 56 unit + 14 doctest。

### upstream（ast/printer）增长（6d）

- **无**。optionalchain + objectrestspread 全部用 arena 既有构造器（`new_conditional_expression`/`new_void_expression`/`new_numeric_literal`/`new_keyword_expression`/`new_binary_expression`/`new_property_access_expression`/`new_element_access_expression`/`new_call_expression`/`new_object_literal_expression`/`new_token`）+ `visit_each_child` 默认递归即可。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## 6d-2 worklog（red→green 推进记录）

聚焦基建轮：移植 6d 发现缺失的 printer **emit-helper-library 基建**，解锁 namedevaluation/using/forawait/async。逐切片红→绿：

1. **EmitHelper 模型 + prologue emit（tracer，端到端）** — 新建 `internal/printer/emithelpers.rs`（`EmitHelper { name, scoped, text, priority, dependencies, import_name }` + `SET_FUNCTION_NAME_HELPER` 静态）；`EmitContext` 新增每节点 `add_emit_helper`/`get_emit_helpers`；printer 新增 `emit_helpers`（在 `emit_source_file_worker` 起始调用）+ `write_lines`（split_lines + guess_indentation，逐非空行 `write_line`+`write`）。测试 `requested_helper_definition_emitted_in_prologue`：把 helper 挂到源文件 → 恒等红（helper 不出现）→ 实现 emit → 绿（prologue 完整 helper 定义 + 语句）。
2. **request/read** — `EmitContext::request_emit_helper`（递归先入依赖 + 按名去重）+ `read_emit_helpers`（取出并清空）。测试 `requested_helpers_round_trip_and_dedup`。
3. **依赖递归** — 以真实 `IMPORT_STAR_HELPER`（依赖 `__createBinding`+`__setModuleDefault`）验证 `request` 先记录依赖。测试 `requested_helper_pulls_in_dependencies_first`。
4. **优先级排序** — `compare_emit_helpers`（低 priority 先、`None` 最后；相等稳定）+ printer `emit_helpers` 内 `sort_by`。测试 `compare_orders_by_priority_then_none_last`（纯）+ `prologue_emits_helpers_in_priority_order`（`__awaiter`(5) 先于 `__setFunctionName`(None)，红→绿）。
5. **unscoped helper-name** — factory `new_unscoped_helper_name`（标识符 + `EmitFlags::HELPER_NAME`）。doctest 验证 flag 置位。printer 当前不特判 HELPER_NAME（按普通标识符 emit `__setFunctionName`），module-emit 重写留待后续。
6. **端到端验证（namedevaluation）** — 新建 `estransforms/namedevaluation.rs`：`var f = function(){}`（匿名函数绑定标识符）→ `request_emit_helper(set_function_name)` + `var f = __setFunctionName(function () { }, "f")`；源文件边界 `read_emit_helpers` → `add_emit_helper(SF, …)`。测试 `anonymous_function_binding_gets_set_function_name` 断言 prologue helper 定义 + 重写后的调用。

**测试计数（6d-2 新增）**：`tsgo_printer` +6 `#[test]`（emithelpers 4 + emitcontext 2）+1 doctest（`new_unscoped_helper_name`）；`tsgo_transformers` +1 `#[test]`（namedevaluation）+1 doctest（`new_named_evaluation_transformer`）。printer 合计 173 unit + 24 doctest；transformers 57 unit + 15 doctest。

### upstream（printer）增长（6d-2）

- **新增** `internal/printer/emithelpers.rs`（`EmitHelper` 模型 + `compare_emit_helpers` + helper 定义静态）。`lib.rs` 导出 `EmitHelper`。
- **printer `EmitContext`**（additive）：`node_helpers` 表（每节点 helper）+ `requested_helpers` 列表；`add_emit_helper`/`get_emit_helpers`/`request_emit_helper`/`read_emit_helpers`。
- **printer `factory`**（additive）：`new_unscoped_helper_name`（`EmitFlags::HELPER_NAME`）。
- **printer `printer`**（additive）：`emit_helpers`（源文件 prologue，按优先级稳定排序）+ `write_lines`；`emit_source_file_worker` 调用 `emit_helpers`（无 helper 时无副作用，既有 167→173 测试无回归）。
- **未触碰** `internal/ast/*`（不需要新节点种类：unscoped helper name 复用普通 `Identifier` + emit flag）。

### helper 定义：已定义 vs 已消费（6d-2）

| helper | 定义 | 消费者 | 状态 |
|---|---|---|---|
| `__setFunctionName` | ✅ | `namedevaluation.rs` | **已消费**（6d-2 验证）|
| `__awaiter`(prio 5) | ✅ | `async.rs` | 已定义未消费（6d-3）；本轮用于优先级排序测试 |
| `__createBinding`/`__setModuleDefault`/`__importStar`(deps) | ✅ | `moduletransforms` | 已定义未消费；本轮用于依赖递归测试 |
| `__rest` | ✅ | `objectrestspread.rs`（rest 绑定）| **已消费**（6g：变量声明对象 rest 绑定）|
| `__addDisposableResource`/`__disposeResources` | ✅ | `using.rs` | 已定义未消费（6d-3）|
| `__importDefault`/`__exportStar` | ✅ | `moduletransforms` | 已定义未消费（6e）|
| `__await`/`__asyncGenerator`/`__asyncDelegator`/`__asyncValues` | ✅（定义在 `forawait.rs`）| `forawait.rs` | **已消费**（6y/6z）；`tsgo_printer` 出编辑范围故就地定义 |
| `__spreadArray` | ✅（定义在 `spread.rs`）| `spread.rs`（数组/调用参 spread）| **已消费**（6aa）；`tsgo_printer` 出编辑范围故就地定义（同 `forawait.rs`），文本 verbatim `tsc --target es5` |
| `__generator` | — | — | Go 本 port 无此 helper（async 用 `__awaiter` + 原生 `function*`）|

## 6d-3 worklog（red→green 推进记录）

`estransforms` part 3。先 probe 解析支持发现关键约束：`async`/async 箭头/`for await`/async 生成器**均可解析**，但**语句级 `using x = expr;` 不可解析**（parser 报 "';' expected"，且 parser 不在本轮编辑范围）。据此按可达性排序落地。

**`async`（顶层 async 函数声明，全落地）**
1. `async_function_lowers_to_awaiter_wrapper`（tracer）：`async function f() { await g(); }` → 恒等红 → 实现 `async_visit`（ec-threaded：SourceFile 收尾 read/attach helper、`FunctionDeclaration`+async 转换）+ `visit_async_function_declaration`（剥 async modifier）+ `build_awaiter_wrapper_body`（`return __awaiter(this, void 0, void 0, function* () { … })`、请求 `__awaiter`）+ `convert_await_to_yield`（`await X`→`yield X`，不下钻嵌套函数体）→ 绿（prologue `__awaiter` 定义 + 包装）。
2. `async_function_without_await_still_wraps`：`async function f() { g(); }` → 仍包装（触发于 async modifier 而非 await 存在）→ 绿。
3. `async_generator_is_left_unchanged`（守卫）：`async function* g() { yield 1; }` → 保持不变（async 生成器需 `__asyncGenerator`，已 DEFER；`is_async` 加 `asterisk_token.is_none()` 守卫避免误降级）→ 绿。

**`forawait`：DEFER（范围）** — 无最小 tracer，忠实降级必发完整 async-iteration 脚手架（见 Go→Rust 行）。

**`using`：DEFER（parser）** — 语句级 `using` 不可解析；transform 与 helper 均就绪，待 parser 支持。

**测试计数（6d-3 新增）**：`tsgo_transformers` +3 `#[test]`（async 3）+1 doctest（`new_async_transformer`）。crate 合计 60 unit + 16 doctest。

### upstream（ast/printer）增长（6d-3）

- **无**。async 包装全部用 arena 既有构造器（`new_function_declaration`/`new_function_expression`/`new_yield_expression`/`new_return_statement`/`new_block`/`new_call_expression`/`new_keyword_expression`/`new_numeric_literal`/`new_void_expression`/`new_token`）+ 6d-2 的 `request_emit_helper`/`new_unscoped_helper_name`。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## estransforms 移植状态（6c..6d-3 收口）

**已移植（行为正确的 down-level 子集）**：
- `exponentiation`（`**`/`**=` + temp-hoist 目标）— 6c-1/6c-3；**6i** 函数声明体 per-scope var-env（`function f() { a.x **= b; }` → 体内 `var _a;`）。
- `classfields`（实例/static/私有 WeakMap/计算名字段 + 构造器插入族）— 6c-1..6c-4。
- `optionalchain`（`a?.b`/`?.[]`/`?.()`/链尾随段，简单 receiver）— 6d；**temp-hoist receiver（`f()?.b`）+ 多 `?.`（`a?.b?.c`）** — 6h；**6i** per-scope var-env：函数声明/表达式、箭头体、类方法各自作用域 hoist temp。
- `objectrestspread`（对象字面量 spread → `Object.assign`）— 6d；**对象 rest 绑定（变量声明）→ `__rest`** — 6g。
- `namedevaluation`（`var f = 匿名函数` → `__setFunctionName`）— 6d-2。
- `async`（async 函数声明 → `__awaiter` 包装 + `await`→`yield`）— 6d-3；**async 函数表达式 / 方法 / 箭头**（ec-threaded `VisitEachChild` 穿容器节点；非箭头首参 `this` + 块包装、箭头首参 `void 0` + 简明体）— 6m。
- **printer emit-helper 基建**（`EmitHelper` + request/read + prologue emit）— 6d-2。

**DEFER（→ parser / 深度脚手架 / checker / name-generator）**：
- `forawait`：async-iteration 脚手架（无最小 tracer）。
- `using`/`await using`：**parser 不解析语句级 `using`**（硬阻塞，需 parser 轮）。
- `async` 余下（6m 已落地函数表达式/方法/箭头）：async accessor、async 生成器（`__asyncGenerator`）、async 方法内 `super`（需 `_super` 绑定）、`asyncContextHasLexicalThis` 跨嵌套作用域线程化、lexical-`arguments`/`_this` 捕获、默认/rest 参数、`for await`、top-level `await`。
- `objectrestspread` 余下（6g 已落地变量声明对象 rest 绑定子集）：泛型 `FlattenDestructuringBinding`（嵌套/数组绑定模式、默认值、计算键 temp 缓存、非简单 init temp）= **✅ 6j `destructuring.go`**（绑定模式 `FlattenLevelAll` 已移植，见 6j worklog）；**对象 rest 赋值** `({ a, ...r } = o)` = **✅ 6k**（经泛型 `FlattenDestructuringAssignment` @ `FlattenLevelObjectRest`，共享 `__rest`，见 6k worklog）；rest 在**参数**/`for-of`/`catch` 位置 + 对象-rest 赋值的非简单 value/重命名/默认/嵌套/计算键 = DEFER（需参数/for-of 接线 + 值位 needs_value + computedTempVariables）。
- `esdecorator`：checker 元数据。
- `nullishcoalescing`/`logicalassignment`/`optionalcatch`/`taggedtemplate`/`usestrict`/`classthis`：未排期（多数 factory 即可，部分需 helper）。
- `definitions`（`GetESTransformer` target 派发链）：依赖各 stage 就绪。
- 各 stage 的 `--target`/compilerOptions 门控 + name-generator 唯一命名精化。

## 6e worklog（red→green 推进记录）

`moduletransforms`。先勘察依赖发现 CJS/ESM **变换**被多处基建缺口阻塞（见下），故本轮聚焦**可达且可测**的共享分析 `externalmoduleinfo`，逐切片红→绿：

1. `named_import_is_an_external_import`（tracer）：`import { x } from "m";` → 恒等空 `ExternalModuleInfo` 红 → 实现 `collect_external_module_info`（扫描 `SourceFile` 顶层语句，`ImportDeclaration` → `external_imports`）→ 绿。
2. `export_star_sets_flag_and_is_external_import`：`export * from "m";` → 加 `ExportDeclaration` 臂（有 module specifier → external import；无 export clause → `has_export_stars_to_export_values`）→ 红→绿。
3. `local_named_export_records_exported_name`：`export { x };` → `add_exported_names_for_export_clause`（`NamedExports` 取 specifier name，按 text 去重）→ 红→绿。
4. `export_equals_is_recorded`：`export = x;` → `ExportAssignment` 臂（`is_export_equals` → `export_equals`）→ 红→绿。
5. `exported_const_records_exported_name`：`export const y = 1;` → `VariableStatement`+EXPORT modifier 臂 → `collect_exported_variable_name`（标识符绑定）→ 红→绿。

**CJS/ESM 变换 DEFER（基建缺口，逐项核查）**：
- **emit substitution（`onSubstituteNode`）未移植** → 无法重写 import *使用*（`x`→`m_1.x`）。`internal/printer/*.rs` 零命中 substitution。
- **`ReferenceResolver` 为 no-op 占位**（`referenceresolver.rs` 所有 `get_referenced_*` 返回 `None`/空）→ 无法解析引用到声明。
- **`TransformOptions` 仅含 `context`**（`compiler_options`/`resolver` 已 DEFER）→ 变换拿不到 module kind / `esModuleInterop`。
- `require` 调用降级本身可达（`getExternalModuleNameLiteral` 对简单字符串 specifier 不需 resolver），但忠实的 `transformCommonJSModule` 还需 `__esModule` 标志 + 源文件组装 + external-module 指示器；强行做简化分支会偏离门控行为且对任何"使用 import 的模块"产出错误代码。

**测试计数（6e 新增）**：`tsgo_transformers` +5 `#[test]`（externalmoduleinfo 5）+1 doctest（`collect_external_module_info`）。crate 合计 65 unit + 17 doctest。

### upstream（ast/printer）增长（6e）

- **无**。`externalmoduleinfo` 为纯 AST 分析，全走 `arena.data`/`arena.kind`/`arena.text` 读取既有节点。未触碰 `internal/ast/*` 或 `internal/printer/*`。

### moduletransforms 解锁前置（建议下一基建轮）

CJS/ESM/implied 变换需要一轮 **emit-substitution + 真实 ReferenceResolver + `TransformOptions` compilerOptions/resolver 线程化** 的前置基建（类比 6d-2 为 helper-依赖 stage 解锁 helper-emit 基建）。就绪后 CJS/ESM 即可逐用例红→绿。

## 6f worklog（red→green 推进记录）

`jsxtransforms`。JSX 在 `.tsx` 下可解析（新增 `parse_shared_tsx` 测试 helper），且 classic-runtime 元素→调用为纯结构化重写（无需 substitution/resolver），故全程可达。逐子用例红→绿（classic runtime）：

1. `intrinsic_self_closing_element_lowers_to_create_element`（tracer）：`<div/>;` → 恒等 `<div />;` 红 → 实现 `jsx_visit`（`JsxSelfClosingElement` 臂）+ `lower_create_element` + `get_tag_name`（intrinsic → string literal via `tsgo_scanner::is_intrinsic_jsx_name`）+ `make_react_create_element`（`React.createElement`）→ `React.createElement("div", null);` → 绿。
2. `component_self_closing_element_uses_identifier_tag`：`<Foo/>;` → `get_tag_name` 非 intrinsic 标识符直接复用 → `React.createElement(Foo, null);` → 直接绿。
3. `string_attribute_becomes_props_object` + `expression_attribute_uses_inner_expression`：`<div id="x"/>` / `<div id={y}/>` → `transform_jsx_attributes_to_object_props`（ES2018+ 对象字面量路径，硬编码）+ `transform_jsx_attribute_to_object_literal_element` + `transform_jsx_attribute_initializer`（string 重建 / JsxExpression 取内层）→ `{ id: "x" }` / `{ id: y }` → 红→绿。
4. `expression_child_becomes_trailing_argument` + `text_child_becomes_string_literal` + `nested_element_child_is_lowered`：加 `JsxElement` 臂 + children 处理（`transform_jsx_child_to_expression`：JsxExpression 取内层/spread、JsxText `fixup_whitespace_and_decode_entities`、嵌套递归）→ 红→绿。
5. `fragment_lowers_to_react_fragment_create_element`：`<>{x}</>;` → `JsxFragment` 臂 + `lower_fragment_create_element`（`React.Fragment` + `React.createElement`）→ 红→绿。

**automatic runtime（`jsx`/`jsxs`/`jsxDEV`）DEFER**：运行时选择来自 `compilerOptions.Jsx`（`--jsx react-jsx`），`TransformOptions` 无 `compiler_options`；implicit import 注入需 `emitResolver.SetReferencedImportDeclaration`（占位）。同 6e 的 compilerOptions/resolver 缺口。本端口硬编码 classic `React.createElement` 工厂。

**测试计数（6f 新增）**：`tsgo_transformers` +8 `#[test]`（jsx classic runtime）+1 doctest（`new_jsx_transformer`）。crate 合计 73 unit + 18 doctest。

### upstream（ast/printer）增长（6f）

- **无**。JSX classic 降级全走 arena 既有构造器（`new_call_expression`/`new_property_access_expression`/`new_string_literal`/`new_object_literal_expression`/`new_property_assignment`/`new_keyword_expression`/`new_spread_element`/`new_identifier`）+ `visit_each_child`。未触碰 `internal/ast/*` 或 `internal/printer/*`。新增 `parse_shared_tsx`/`parse_shared_named` 仅在 transformers 测试 harness。

## 6e-2 worklog（red→green 推进记录）

最高杠杆基建轮：解锁 6e/6f 旗标的缺口（emit substitution + compilerOptions + resolver）。先勘察 Go 侧：printer `onSubstituteNode`/`SubstituteNode` 仍为 `!!!` 注释 stub（Go port 也未接），`referenceresolver.rs` 为 no-op 占位，`TransformOptions` 仅含 `context`。三轨 + 验证：

**Track 1 — emit 节点替换（printer）**
1. `node_substitution_round_trips`（emitcontext_test）：`set_node_substitution`/`get_node_substitution` 往返 → 红（缺方法）→ 实现 `EmitContext.node_substitutions: FxHashMap<NodeId, NodeId>` + set/get → 绿。
2. `printer_emits_substituted_node`（端到端）：注册 `x`→`m_1.x`，emit `x;` → 恒等 `x;` 红 → printer `emit_expression_node` 顶部加替换 hook（`self.get_node_substitution(node).unwrap_or(node)`，单次替换；替换结果的子部件为不同节点不再被替换）→ `m_1.x;` 绿。**设计**：Go 的 `onSubstituteNode` 回调在 emit 时建节点；Rust printer 不可变借用 `EmitContext`（无法在 emit 时建节点），故改为 transform 预建替换节点 + 节点替换表，printer 查表（不可变）—— 行为等价的 Rust 适配。

**Track 2 — `TransformOptions.compiler_options`**
3. 扩展 `TransformOptions`（6a 仅 `context`）加 `compiler_options: tsgo_core::CompilerOptions`（Clone+Default）。更新全部 ~12 处 `TransformOptions { context: … }` 构造为 `..Default::default()`。proof 见验证（commonjs 读 `module == CommonJs` 分支）+ `non_commonjs_module_kind_is_passthrough`。

**Track 3 — 真实 ReferenceResolver：DEFER**
4. 核查：`tsgo_binder::bind_source_file` 产出**声明**符号，但 use→declaration 解析需 checker `resolveName`/`EmitResolver`（scope-aware），未移植；且 `referenceresolver.rs` 不在本轮编辑范围。→ DEFER，保留占位；验证用按名匹配作 stand-in（作用域正确性留待 track 3）。

**验证（blocked→green）— minimal CommonJS**
5. `named_import_and_use_lower_to_require_and_member_access`：`import { x } from "m"; x;`（`module: commonjs`）→ `const m_1 = require("m");\nm_1.x;`。新建 `moduletransforms/commonjsmodule.rs`：复用 6e `collect_external_module_info` 识别 external imports；named import → `const m_1 = require("m");`（`build_require_const`，`NodeFlags::CONST`）；收集 kept 语句中匹配 import 绑定名的标识符 use → `set_node_substitution(use, m_1.<name>)`（track 1）。`non_commonjs_module_kind_is_passthrough` 验证 track 2 分支。

**测试计数（6e-2 新增）**：`tsgo_printer` +2 `#[test]`（节点替换 往返 + 端到端）；`tsgo_transformers` +2 `#[test]`（commonjs 验证 + passthrough）+1 doctest（`new_common_js_module_transformer`）。printer 合计 175 unit + 24 doctest；transformers 合计 75 unit + 19 doctest。

### upstream（printer）增长（6e-2）

- **printer `EmitContext`**（additive）：`node_substitutions` 表 + `set_node_substitution`/`get_node_substitution`。
- **printer `printer`/`emit_expressions`**（additive）：`Printer::get_node_substitution` 访问器 + `emit_expression_node` 顶部替换 hook（空表时无副作用，既有 173→175 测试无回归）。
- **`TransformOptions`**（transformers）：加 `compiler_options` 字段（读 `tsgo_core::CompilerOptions`，已是 dep）。
- **未触碰** `internal/ast/*`（节点替换复用既有节点，无新 NodeData/Kind）。

### 现在可达 vs 仍 DEFER（6e-2 后）

- **现在可达**（基建就绪）：CommonJS use-rewrite（按名匹配）、按 module kind / jsx 分支的变换决策、任意 transform 注册 emit 替换。
- **仍 DEFER**：作用域正确的 use 解析（真实 ReferenceResolver，checker `resolveName`）；CJS 完整面（interop helpers、export 降级、`__esModule`/strict/hoisting、`export =`、动态 import）；ESM 变换；JSX automatic runtime 的 implicit-import 注入（需 emitResolver `SetReferencedImportDeclaration`，虽 jsx 运行时选择现可经 track 2 读取）。

## 6e-3 worklog（red→green 推进记录）

基于 6e-2 基建（emit 节点替换 + `compiler_options`）生长 CommonJS 面 + JSX automatic 选择。逐子用例红→绿（in-order pass，6e-2 import+use / passthrough 保持绿）：

**CommonJS（commonjsmodule.rs）**
1. `export_default_becomes_exports_default_with_marker`：`export default 1;` → 把 `transform_common_js_module` 重构为**源序单遍**（保 6e-2 顺序）+ `module_has_exports` → `make_es_module_marker`（`Object.defineProperty(exports, "__esModule", { value: true });`）+ `lower_export_default`（`exports.default = e`）→ 红→绿。
2. `export_const_becomes_exports_assignment`：`export const y = 1;` → `lower_export_variable_statement`（`exports.y = init`，对标 Go `transformInitializedVariable`）→ 绿。
3. `local_named_export_becomes_exports_assignment`：`const x = 1; export { x };` → `lower_export_declaration`（NamedExports 无 module specifier → `exports.b = a`/`exports.x = x`）→ 绿。
4. `default_import_uses_import_default_helper`：`import d from "m"; d;` → 重构 `ImportBinding`（加 `member: Option<String>`）+ `lower_import_to_require` 分派 default/namespace/named + `wrap_in_helper`（`__importDefault(require(…))`，请求 helper）；use `d` → `m_1.default` → 红→绿。
5. `namespace_import_uses_import_star_helper`：`import * as ns from "m"; ns;` → NamespaceImport 分支 + `__importStar`（递归请求 `__createBinding`/`__setModuleDefault` deps）；use `ns` → `m_1`（member None）→ 绿。
6. `export_star_uses_export_star_helper`：`export * from "m";` → `make_export_star`（`__exportStar(require("m"), exports)`，请求 helper）→ 绿。

**JSX automatic 选择（jsxtransforms/jsx.rs）**
7. `automatic_runtime_self_closing_element_lowers_to_jsx_call`：`<div/>`（`compiler_options.jsx = ReactJsx`）→ 给 jsx transform 线程 `automatic: bool`（读 track-2 `compiler_options.jsx`）；automatic 分支：callee `jsx`、props `{}`（非 `null`）、无 children → `jsx("div", {})`。DEFER：automatic children-in-props/`jsxs`/implicit import 注入 → 红→绿。

**esmodule.rs：DEFER** — 可达子集近恒等（value import/export 原样保留），实质工作需 type-eraser 集成 + interop 注入，留待后续。

**测试计数（6e-3 新增）**：`tsgo_transformers` +7 `#[test]`（commonjs 6 + jsx automatic 1）。crate 合计 81 unit + 19 doctest（本轮无新 doctest）。

### upstream（ast/printer）增长（6e-3）

- **无**。CJS export/interop + JSX automatic 全走 arena 既有构造器 + 6d-2 helper infra（`request_emit_helper` + `new_unscoped_helper_name`）+ 6e-2 substitution/compiler_options。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## 6g worklog（estransforms 深化 — objectrestspread `__rest`，red→green 推进记录）

> 本轮为 **estransforms 深化**：消费 6d-2 已定义但未消费的 `__rest` helper，落地 `objectrestspread` 的**对象 rest 绑定**（变量声明）可达子集。与计划表里前瞻性的 `6g=declarations`/`6h=destructuring` 不同名同号——本轮按「6g：estransforms 深化」的轮次范围执行；泛型 `FlattenDestructuringBinding`（`destructuring.go`）仍归 6h。

先确认基线绿（`cargo test -p tsgo_transformers` = 82 unit + 19 doctest）。逐行为红→绿：

1. **tracer**（`object_rest_binding_lowers_to_rest_helper`）：`var { ...rest } = o;` → 恒等红（`var { ...rest } = o;` ≠ `__rest` 形态）→ 把 `objectrestspread` 从 arena-only 改为 **ec-threaded**（镜像 `async.rs`）：`SourceFile` 臂 visit 顶层语句 + `read_emit_helpers`→`add_emit_helper`（prologue 注入），`VariableStatement` 臂走 rest 降级，其余/对象字面量 spread 仍走 arena-only `object_spread_visit`（保 6d 三个用例绿）。实现 `try_lower_variable_statement_with_rest` + `lower_object_rest_declaration` + `new_rest_helper`（`request_emit_helper(REST_HELPER)` + `new_unscoped_helper_name("__rest")` + `__rest(value, [])`）→ 绿（prologue `var __rest = …;` + `var rest = __rest(o, []);`）。**证明 `__rest` helper 经 6d-2 emit-helper 基建端到端消费。**
2. `leading_binding_is_excluded_from_rest_keys`：`var { a, ...rest } = o;` → 红 → 实现 leading 处理：非 rest 简单绑定 chunk 成 leading 对象绑定模式 decl（`{ a } = o`），其 key 收进 `__rest` 排除数组（`["a"]`）→ 绿。
3. `multiple_leading_bindings_list_all_rest_keys`：`var { a, b, ...rest } = o;` → `var { a, b } = o, rest = __rest(o, ["a", "b"]);`（多 key，源序）→ 直接绿（验证 key 循环）。
4. `const_declaration_kind_is_preserved`：`const { x, ...rest } = o;` → 保留 `const`（重建 list 时 copy `BLOCK_SCOPED` flags）→ 直接绿。
5. `renamed_leading_binding_excludes_property_key`：`var { a: b, ...rest } = o;` → 排除**属性键** `a`（非本地绑定 `b`）、leading 保留 `{ a: b }`（`reachable_leading_key` 取 property_name 文本）→ 直接绿。
6. `non_simple_initializer_with_leading_binding_is_left_unchanged`（DEFER 守卫）：`var { a, ...rest } = f();` → leading 非空 + init 非 simple-copiable（`f()` 引用两次需 var-hoist temp）→ 落在可达子集外，整句**保持不变**（不请求 helper、不部分降级）→ 直接绿。

> 设计：Go 经泛型 `FlattenDestructuringBinding`（在 `VariableDeclaration` 级返回单 decl 或 `SyntaxList`，printer 在 decl-list 内 flatten `SyntaxList`）。Rust 侧改在 **`VariableStatement` 级**直接重建声明列表（`Vec<NodeId>`，无 `SyntaxList`）—— 行为等价的适配，**避免 printer 对 decl-list 内 `SyntaxList` 的 flatten 增长**（boundary「能不碰就不碰」）。AST 绑定模式谓词（rest 目标/leading key 提取）作 `objectrestspread.rs` 私有读取助手实现（读 arena），未碰 `internal/ast/*`。

**测试计数（6g 新增）**：`tsgo_transformers` +6 `#[test]`（objectrestspread rest 绑定 6）。crate 合计 **88 unit + 19 doctest**（本轮无新 doctest：`new_object_rest_spread_transformer` 已有 doctest）。

### upstream（ast/printer）增长（6g）

- **无**。对象 rest 绑定降级全走 arena 既有构造器（`new_binding_pattern`/`new_variable_declaration(_list)`/`new_variable_statement`/`new_string_literal`/`new_array_literal_expression`/`new_call_expression`/`add_flags`）+ 6d-2 的 `request_emit_helper`/`read_emit_helpers`/`add_emit_helper`/`new_unscoped_helper_name`（`REST_HELPER` 早在 6d-2 定义）。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## 6h worklog（optionalchain 深化 — receiver temp-hoist + 多 `?.`，red→green 推进记录）

> 本轮为 **optionalchain 深化**：复用 6c-3 建立的 `EmitContext` 变量环境（`start/end_variable_environment` + `add_variable_declaration` + `new_temp_variable`），把 6d 的「单 `?.` + 简单 receiver」子集扩到「非简单 receiver hoist temp」+「多 `?.` 链」。结构镜像 `exponentiation.rs`/`async.rs`：把 transformer 从 arena-only 改为 **ec-threaded**（`SourceFile` 起变量环境、`ExpressionStatement` 下钻表达式），temp 只在**顶层语句路径**可 hoist；嵌套作用域降级仍走 arena-only 且 DEFER temp-hoist（无 var 环境接收其 `var`）。

先确认基线绿（`cargo test -p tsgo_transformers` = 88 unit + 19 doctest）。逐行为红→绿：

1. **`non_simple_receiver_hoists_temp`（receiver temp-hoist）**：`f()?.b;` → 恒等红（DEFER 原样 `f()?.b;`）→ 把入口从 `optional_chain_visit(arena,…)` 改为 ec-threaded `optional_chain_visit(ec,…)`（`SourceFile`→`visit_source_file` 起/收变量环境、`ExpressionStatement` 下钻、chain→`lower_optional_expression`），其余下钻保留 arena-only `optional_chain_visit_arena`（即原 arena 逻辑改名 + DEFER 守卫保留，保 6d 五个用例绿）。`lower_optional_expression`：receiver 非 `is_simple_copiable` 时 `new_temp_variable()` + `add_variable_declaration()` + `(_a = recv)` 赋值，守卫测赋值、访问读 temp（`create_not_null_condition(left=赋值, right=temp)`）→ `var _a;\n(_a = f()) === null || _a === void 0 ? void 0 : _a.b;` → 绿。（此时 `lower_optional_expression` 仍保留「receiver 本身是 optional chain → DEFER」守卫。）
2. **`multiple_optional_links_nest_guards`（多 `?.`）**：`a?.b?.c;` → 恒等红（守卫 DEFER）→ **移除**该守卫，receiver 经 `optional_chain_visit(ec, receiver)` 递归降级（内层 `a?.b` 先降级成条件表达式），其结果非简单 → 上一步的 temp-hoist 自然把它存入 `_a` → `var _a;\n(_a = a === null || a === void 0 ? void 0 : a.b) === null || _a === void 0 ? void 0 : _a.c;` → 绿。
3. **`non_simple_receiver_in_nested_chain_hoists_two_temps`（泛化，直接绿）**：`f()?.b?.c;` → 两条 deepening 复合，每链一 temp（内层先 `_a`、外层 `_b`，allocator 顺序由 receiver 递归先于外层 temp 决定）→ `var _a, _b;\n(_b = (_a = f()) === null || _a === void 0 ? void 0 : _a.b) === null || _b === void 0 ? void 0 : _b.c;` → 直接绿（验证 temp 计数 + 嵌套泛化）。

> 设计：Go `visitOptionalExpression` 经 `RestoreOuterExpressions` + `SyntheticReferenceExpression` 处理 this-capture；本轮**不**引入 `SyntheticReferenceExpression`（无 this-capture 的可达子集不需要），segment 构造（property/element/call）抽成共享 `build_chain_segments(arena, base, chain)`，arena-only 与 ec 两条路径共用（args 经 `optional_chain_visit_arena` 降级）。`create_not_null_condition` 改为分别接收 `left`（赋值/简单 receiver）与 `right`（temp/简单 receiver）—— 简单 receiver 两者同节点，行为与 6d 一致。

**测试计数（6h 新增）**：`tsgo_transformers` +3 `#[test]`（optionalchain temp-hoist 1 + 多 `?.` 1 + 泛化 1）。crate 合计 **91 unit + 19 doctest**（本轮无新 doctest：`new_optional_chain_transformer` 已有 doctest）。

### upstream（ast/printer）增长（6h）

- **无**（6g 报告预期的 ZERO upstream growth 达成）。temp-hoist + 多 `?.` 全走 arena 既有构造器（`new_binary_expression`/`new_conditional_expression`/`new_property_access_expression`/`new_element_access_expression`/`new_call_expression`/`new_token`/`new_keyword_expression`/`new_void_expression`/`new_numeric_literal`/`new_expression_statement`/`new_source_file`）+ 6c-3 的 `EmitContext` 变量环境（`start/end_variable_environment`/`add_variable_declaration`）+ `factory().new_temp_variable()`（均 6c-3 既有）。未触碰 `internal/ast/*` 或 `internal/printer/*`。括号可选调用 this-capture 的 `SyntheticReferenceExpression` 是唯一需 AST 增长的剩余子集 → 单独 DEFER。

## 6i worklog（per-scope 变量环境接线 — red→green 推进记录）

> 本轮把 6c-3 已落地的 `EmitContext` 变量环境从「仅 SourceFile 顶层」扩到「每个函数式作用域」：emit-context 线程化现在也穿过函数体（函数声明/表达式、箭头体、类方法），每个体在 visit 时 `start_variable_environment` → 访问体语句 → `end_variable_environment` 把收集到的 hoisted `var` 合并进该体语句表（镜像 Go `EmitContext.VisitFunctionBody`）。temp-hoisting 路径（6c-3 指数 `**=` + 6h optionalchain 非简单 receiver）本就用 `add_variable_declaration` 写入**当前（最近）**作用域栈顶，故只需在遍历中**打开/关闭**这些作用域，温度就自然落到最近的函数体。

先确认基线绿（`cargo test -p tsgo_transformers` = 91 unit + 19 doctest）。逐行为红→绿：

**optionalchain**
1. **`non_simple_receiver_inside_function_body_hoists_into_body`（tracer）**：`function f() { return g()?.b; }` → 恒等红（旧 DEFER：函数体内无 var-env，链原样留存）→ 在 ec-threaded `optional_chain_visit` 加 `FunctionDeclaration` + `ReturnStatement` 臂；`visit_function_declaration` 经 `visit_function_body`（`start_variable_environment` → 逐语句 `optional_chain_visit` → `end_variable_environment` 前置 hoisted `var` → 重建 block）重建函数 → `function f() { var _a; return (_a = g()) === null || _a === void 0 ? void 0 : _a.b; }`（body 单行：合成 `Block` 不携带 `Block.MultiLine`，沿用 6c-1 注记；**关键行为**：`var _a;` 落在 `f` 体内而非模块顶）→ 绿。
2. **`temp_in_arrow_concise_body_wraps_into_block`（箭头简明体）**：`function f() { return () => g()?.b; }` → 恒等红 → 加 `ArrowFunction` 臂；`visit_function_body` 扩展非 block（简明表达式）分支：在 var-env 内降级表达式，若有 hoisted decl 则包成 `{ <decls>; return <expr>; }`（镜像 Go `VisitFunctionBody` 非 block 分支），否则保持简明 → `function f() { return () => { var _a; return (_a = g()) === null || _a === void 0 ? void 0 : _a.b; }; }`（外层 `f` 环境空，无泄漏）→ 绿。
3. **`nested_function_bodies_hoist_into_their_own_scopes`（嵌套函数，直接绿/泛化）**：`function outer() { g()?.b; function inner() { return h()?.c; } }` → `visit_function_body` 逐语句递归 `optional_chain_visit` 已天然处理嵌套函数声明 → 直接绿；外/内各自 `_a`（printer 函数级 name-generation scope 重置 temp 计数），证明每 temp 落在**最近**作用域。
4. **`temp_inside_function_expression_body_hoists_into_body`（函数表达式）**：`function f() { return function () { return g()?.b; }; }` → 恒等红 → 加 `FunctionExpression` 臂（`visit_function_expression` 复用 `visit_function_body`）→ 绿。
5. **`temp_inside_method_body_hoists_into_method`（方法体）**：`class C { m() { return g()?.b; } }` → 恒等红（类体 arena-only，链未降级）→ 加 `ClassDeclaration` 臂（`visit_class_declaration` 线程化穿过 members）+ `MethodDeclaration` 臂（`visit_method_declaration` 复用 `visit_function_body`）→ `class C {\n    m() { var _a; return (_a = g()) === null || _a === void 0 ? void 0 : _a.b; }\n}` → 绿。

**exponentiation（可达，同样接线）**
6. **`property_assignment_inside_function_body_hoists_into_body`**：`function f() { a.x **= b; }` → 恒等红（arena-only 仅处理 `**`/标识符 `**=`，property `**=` 原样）→ 在 ec-threaded `exponentiation_visit` 加 `FunctionDeclaration` 臂 + `visit_function_body`（block-only，逐语句 `exponentiation_visit` → ExpressionStatement → `visit_binary_expression` property `**=` temp-hoist 落入 `f` 的 var-env）→ `function f() { var _a; (_a = a).x = Math.pow(_a.x, b); }` → 绿。

**移除的 DEFER 守卫**：optionalchain/exponentiation 的「非顶层作用域 temp-hoist DEFER（无 var-env）」对**函数体直属语句**已解除——函数声明/表达式、箭头体、类方法现各自开 var-env。仍 DEFER 的更窄子集：链/`**=` 嵌套在**仍未线程化的位置**（控制流语句体 `if`/`for`/`while`、`switch` case、对象字面量方法简写、构造器/accessor 体；exponentiation 本 stage 仅线程化了函数声明，方法/函数表达式/箭头待后续）。

**测试计数（6i 新增）**：`tsgo_transformers` +6 `#[test]`（optionalchain 5 + exponentiation 1），本轮无新 doctest。crate 合计 **97 unit + 19 doctest**（6h 基线 91+19）。

### upstream（ast/printer）增长（6i）

- **无**。per-scope 变量环境接线全部复用 6c-3 既有的 `EmitContext` 变量环境（`start_variable_environment`/`add_variable_declaration`/`end_variable_environment`，本就用作用域栈、写入栈顶）+ arena 既有节点构造器（`new_block`/`new_return_statement`/`new_function_declaration`/`new_function_expression`/`new_arrow_function`/`new_method_declaration`/`new_class_like`）+ printer 既有函数级 name-generation scope（temp 计数按作用域重置）。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## 6j worklog（destructuring 绑定展平器 `FlattenDestructuringBinding`，red→green 推进记录）

> 本轮把根包 `destructuring.go` 的 **`FlattenDestructuringBinding`**（绑定模式展平器，`FlattenLevelAll`）逐行为移植到 `destructuring.rs`，复用 6c-3/6i 的 `EmitContext` 变量环境基建与既有 arena 构造器。无 checker/resolver 依赖（纯 AST 变换）。
>
> **驱动选型（divergence）**：Go 的 `FlattenDestructuringBinding` 由 ES2015 transformer 的变量声明降级调用；该 es2015 stage 在本仓未单独成文件移植。本轮新增 `new_destructuring_transformer`（root 包 `destructuring.rs`）作为**薄驱动**镜像 es2015 的变量声明降级（`var [a,b]=arr` → `var a=arr[0], b=arr[1]`），既给展平器一个可测的公开入口（与全部 estransforms 测试一致经 parse→transform→emit），又是 Go 调用 `FlattenDestructuringBinding` 的等价接线点。`flatten_destructuring_binding` 本身是 `pub fn`（对应 Go 导出的 `FlattenDestructuringBinding`），递归子节点访问硬编码为本模块的 `destructuring_visit`（Go 的 `tx.Visitor()` 等价物，可达子集内恒等）。
>
> **hoist 选型**：Go es2015 传 `hoistTempVariables = (变量语句是否 exported)`。驱动取**非 exported**路径（`hoist=false`，常见情形），与 tsc ES5 输出一致——非简单 initializer 的 temp 落为**同语句声明**（`var _a = f(), …`）而非经 var-env hoist 的 `var _a;`。var-env 仍在源文件边界 start/end（复用 6i 基建），承接 `hoist=true`（exported，DEFER）与尾随表达式路径（DEFER）的 temp；可达子集（hoist=false）下其收集为空。

先确认基线绿（`cargo test -p tsgo_transformers` = 97 unit + 19 doctest）。逐行为红→绿：

1. **tracer（`array_binding_decomposes_to_element_accesses`）**：`var [a, b] = arr;` → 恒等红（驱动初版对绑定模式声明不展平，verbatim `var [a, b] = arr;`）→ 接 `VariableStatement` 臂 + `try_lower_variable_statement`（重建声明列表，展平绑定模式声明 + 展开 `SyntaxList`）+ `flatten_destructuring_binding` + `Flattener`（`flatten_binding_or_assignment_element` / `flatten_array_binding_or_assignment_pattern` 非 rest 路径 / `ensure_identifier` / `emit_binding` / decl 构建）→ `var a = arr[0], b = arr[1];` → 绿。**证明绑定展平器经薄驱动端到端运行。**
2. **`object_binding_decomposes_to_property_accesses`**：`var { a, b } = o;` → 红（`unimplemented!("object binding pattern")` panic）→ 实现 `flatten_object_binding_or_assignment_pattern`（`level < ObjectRest` 路径）+ `create_destructuring_property_access`（标识符键 → 属性访问）+ `try_get_property_name`（shorthand → target / rename → property_name）→ `var a = o.a, b = o.b;` → 绿。
3. **defaults（`array_default_guards_with_void_zero` / `object_default_guards_with_void_zero`）**：`var [a = 1] = arr;` → 红（`unimplemented!("default values")`）→ 实现 `flatten_binding_or_assignment_element` 的 `(Some init, Some value)` 臂 + `create_default_value_check`（`ensure_identifier(value)` → `value === void 0 ? default : value`）→ `var _a = arr[0], a = _a === void 0 ? 1 : _a;`；`var { a = 1 } = o;` → `var _a = o.a, a = _a === void 0 ? 1 : _a;` → 绿。
4. **nested（`nested_array_pattern_composes_element_accesses` / `nested_object_pattern_composes_property_accesses`，直接绿/泛化）**：`var [[a]] = x;` → `var a = x[0][0];`；`var { a: { b } } = o;` → `var b = o.a.b;`。嵌套绑定模式经 `flatten_binding_or_assignment_element` 递归到 `flatten_array/object` 已天然处理（target 取内层 pattern），无新代码 → 直接绿（验证递归泛化）。
5. **array rest（`array_rest_lowers_to_slice`）**：`var [a, ...r] = arr;` → 红（`unimplemented!("array rest")`）→ 实现数组 rest 臂（`i == num-1` + rest indicator）+ `new_array_slice_call`（`array.slice(i)`）→ `var a = arr[0], r = arr.slice(1);` → 绿。对象 rest **不在本轮**：driver `declaration_has_object_rest` 跳过含对象 rest 的声明，留给 6g `objectrestspread` 的 `__rest` 路径（"集成/共享，不重复"）。
6. **computed key（`computed_key_captures_object_then_key_into_temps`）**：`var { [k]: a } = o;` → 红（`unimplemented!("computed property keys")`）→ 实现 (a) `create_destructuring_property_access` 计算键臂（`ensure_identifier(visit(k))` → `value[_temp]`）+ (b) `flatten_destructuring_binding` 顶部**非字面量计算名特例**（`contains_non_literal_computed_name` → 先把 initializer 经 `ensure_identifier(_, reuse=false)` 捕获进 temp）+ `binding_assigns_to_name` / `update_variable_declaration_initializer` / `is_literal_expression`。语义关键：保 `o`→`k` 求值序 → `var _a = o, _b = k, a = _a[_b];` → 绿。
7. **非简单 initializer（`non_simple_initializer_is_captured_in_a_temp_declaration`，直接绿/泛化）**：`var [a, b] = f();` → `var _a = f(), a = _a[0], b = _a[1];`。`ensure_identifier`（hoist=false，非标识符）的 `emit_binding` 路径（slice 3 已建）对顶层 receiver 复用 → 直接绿（验证 temp 落为同语句声明，与 tsc ES5 一致）。

> 设计/divergence：(1) `pendingDecl` 仅承载 name/value（+ pending_expressions 占位）；Loc 不携带（合成节点 emit 不依赖，沿用 6g/6c-1 注记）。(2) `InlineExpressions` 逗号折叠仅 hoist=true / 尾随表达式路径需要，可达 hoist=false 子集不触发 → 本轮以 `debug_assert!(expressions.is_empty())` 守卫，折叠实现 DEFER。(3) AST 绑定谓词（`GetTargetOf…`/`GetElementsOf…`/`TryGetPropertyNameOf…`/`GetRestIndicatorOf…`/`IsDeclarationBindingElement`/`IsPropertyName`/`IsLiteralExpression`/`IsSimpleCopiableExpression`）作 `destructuring.rs` 私有 arena 读取器实现（镜像 6g），**未碰 `internal/ast/*`**。(4) `FlattenLevelObjectRest` 专属分支（数组 rest-containing element 的 per-temp、对象 kept-binding）以 `unreachable!` 占位（本轮 driver 仅 `FlattenLevelAll`，永不触达）。

**测试计数（6j 新增）**：`tsgo_transformers` +9 `#[test]`（array binding 1 + object binding 1 + defaults 2 + nested 2 + array rest 1 + computed 1 + 非简单 init 1）+3 doctest（`FlattenLevel`、`new_destructuring_transformer`、`flatten_destructuring_binding`）。crate 合计 **106 unit + 22 doctest**（6i 基线 97+19）。

### upstream（ast/printer）增长（6j）

- **无**（6g/6h/6i 的 ZERO upstream growth 连续达成）。绑定展平全走 arena 既有构造器（`new_variable_declaration(_list)`/`new_variable_statement`/`new_syntax_list`/`new_element_access_expression`/`new_property_access_expression`/`new_call_expression`/`new_numeric_literal`/`new_identifier`/`new_binary_expression`/`new_conditional_expression`/`new_void_expression`/`new_token`/`add_flags`/`new_source_file`）+ 6c-3 的 `EmitContext` 变量环境（`start/add/end_variable_environment`）+ `factory().new_temp_variable()` + 6d-2 的 `read_emit_helpers`/`add_emit_helper`（源文件边界，本轮无 helper 请求）。未触碰 `internal/ast/*` 或 `internal/printer/*`。

### 与 6g objectrestspread 的关系（对象 rest 共享，不重复）

- 6g `objectrestspread.rs` 已在 **`VariableStatement` 级**直接落地对象 rest 绑定（`var { a, ...rest } = o` → `var { a } = o, rest = __rest(o, ["a"])`，复用 6d-2 `REST_HELPER`）。本轮 6j 的泛型 `flatten_destructuring_binding` driver **跳过**含对象 rest 的声明（`declaration_has_object_rest`），不重复实现；两 transform 在真实 emit 链中各司其职（objectrestspread 处理对象 rest @ `FlattenLevelObjectRest`，destructuring 处理数组/对象/嵌套/默认/数组-rest/计算键 @ `FlattenLevelAll`）。把对象 rest 也纳入泛型 flattener（共享 `objectrestspread::new_rest_helper`）= 6k。

## 6k worklog（destructuring 赋值展平器 `FlattenDestructuringAssignment`，red→green 推进记录）

> 本轮把根包 `destructuring.go` 的 **`FlattenDestructuringAssignment`**（赋值目标模式）逐行为移植到 `destructuring.rs`，**复用 6j 的同一 `Flattener`**——新增 `FlattenMode { Binding, Assignment }` 判别（Go 用一组函数指针回调 `emitBindingOrAssignment`/`createArray|ObjectBindingOrAssignmentPattern`，Rust 改为单判别 + 各回调点分支），并把元素读取器（target/initializer/property-name/rest-indicator/elements）从绑定子集扩到「绑定 ∪ 赋值」全集（对象/数组字面量、property/shorthand/spread assignment、`[a=1]` 赋值元素）。无 checker/resolver 依赖（纯 AST 变换）。
>
> **驱动选型 / 接线点（divergence）**：Go 的 `FlattenDestructuringAssignment` 由 es2015（数组/对象通用降级）与 objectrestspread（对象 rest @ `FlattenLevelObjectRest`）从二元 `=` visitor 调用。Rust 侧：
> - **`FlattenLevelAll`（数组/对象/嵌套/默认/数组-rest）经根包 driver** `new_destructuring_transformer` 的新增 `ExpressionStatement` 臂接入（`try_lower_expression_statement`：skip-parens → `is_destructuring_assignment` → flatten，`needs_value=false`）——与 6j 绑定驱动同源（es2015 通用降级在本仓未单独成文件）。
> - **`FlattenLevelObjectRest`（对象 rest）经 objectrestspread `visitBinaryExpression` 接入**（新增 `ExpressionStatement` 臂 + `reachable_object_rest_assignment` gate → 根包 `flatten_destructuring_assignment` @ `ObjectRest`），**统一走泛型 flattener**，对象 rest 臂复用 objectrestspread 的 `new_rest_helper`（提为 `pub(crate)`）请求/构造 `__rest`——兑现 6j「把对象 rest 纳入泛型 flattener、共享 `__rest`」的建议，不再走 6g 的对象-rest 专路（6g 的变量声明对象 rest 路径保持不变）。
>
> **hoist 选型**：赋值模式恒 `hoistTempVariables = true`（Go 同），temp 经 var-env 落为前置 `var _a;`（数组非简单 value / 默认值的 temp）。可达对象-rest 子集 gate 要求 value 为 simple-copiable（无 temp，statement 路径无 var-env 承接），与 6g 的「非简单 init 留 verbatim」同纪律。

先确认基线绿（`cargo test -p tsgo_transformers` = 106 unit + 22 doctest）。逐行为红→绿（slices 2–5 在 slice 1 的赋值-mode 接线 + 6j 既有对象/默认/嵌套/rest 机器上**直接绿/泛化**，与 6j 的 nested 同纪律）：

1. **tracer（`array_assignment_decomposes_to_element_access_assignments`）**：`[a, b] = arr;` → 恒等红（driver 对 `ExpressionStatement` 不展平，verbatim）→ 接 `ExpressionStatement` 臂 + `try_lower_expression_statement` + `flatten_destructuring_assignment`（`FlattenMode::Assignment`）+ `flatten_destructuring_assignment` 方法（empty-literal 剥壳循环 + value visit + assigns-to-name/computed 守卫 + `flatten_binding_or_assignment_element(skip_initializer=true)`）+ `emit_binding_or_assignment`→`emit_assignment`（`visit(target) = value`）+ `inline_expressions`（reduceLeft comma 折叠）+ `build_assignment_result`（空 → `NewOmittedExpression`）+ 元素读取器赋值-全集扩展（`get_target/initializer/rest_indicator/elements_of_pattern`、`try_get_property_name` 加 PropertyAssignment）+ `is_object/array_binding_pattern` 纳入字面量 + 模块级 `is_destructuring_assignment`/`is_assignment_expression_equals`/`is_empty_array|object_literal`/`skip_parentheses` → `a = arr[0], b = arr[1];` → 绿。**证明赋值展平器经驱动端到端运行，且复用 6j `Flattener`。**
   - 紧接 `array_assignment_non_simple_value_hoists_temp`：`[a, b] = f();` → `var _a;\n_a = f(), a = _a[0], b = _a[1];`（`ensureIdentifier` hoist=true 把 capture 折进 comma + var-env 前置 `var _a;`）→ 红→绿（hoist 路径）。
2. **object（`object_assignment_decomposes_to_property_access_assignments`）**：`({ a, b } = o);` → `a = o.a, b = o.b;`。对象路径 `level<ObjectRest` + property-access 已由 6j 实现，shorthand 经 `get_target`/`try_get_property_name` 赋值扩展支撑 → 直接绿；statement 前导 `(` 括号在降级后丢弃（不再需要）。
3. **default（`array_assignment_default_guards_with_void_zero`）**：`[a = 1] = arr;` → `var _a;\n_a = arr[0], a = _a === void 0 ? 1 : _a;`。`get_initializer_of_element` 赋值-元素臂（`[a=1]` 二元 `=` → right）+ `create_default_value_check`（hoist=true）→ 直接绿。
4. **nested（`nested_array_assignment_composes_element_accesses` / `nested_object_assignment_composes_property_accesses`）**：`[[a]] = x;` → `a = x[0][0];`；`({ a: { b } } = o);` → `b = o.a.b;`。递归经 `flatten_binding_or_assignment_element`（target 取内层字面量模式）→ 直接绿（泛化）。
5. **array rest（`array_assignment_rest_lowers_to_slice`）**：`[a, ...r] = arr;` → `a = arr[0], r = arr.slice(1);`。`get_rest_indicator` 的 `SpreadElement` 臂 + 既有 `new_array_slice_call` → 直接绿。
6. **object rest（`object_rest_assignment_keeps_bindings_and_lowers_rest` / `..._lists_all_kept_keys`）**：`({ a, ...r } = o);` → 恒等红（旧路径误把 LHS `{a,...r}` 当对象 spread 降为 `(Object.assign({ a }, r) = o)`，暴露潜在 bug）→ 在 objectrestspread 加 `ExpressionStatement` 臂 + `reachable_object_rest_assignment` gate（左 = 对象字面量、末 = `...<标识符>`、leading = 简单 shorthand 无默认、value simple-copiable）→ 根包 `flatten_destructuring_assignment` @ `FlattenLevelObjectRest`；在 `flatten_object_binding_or_assignment_pattern` 落地 `level >= ObjectRest` 路径（kept-binding 累积 + `flush_kept_object_elements` 经 `create_object_pattern`（assignment → `ObjectLiteralExpression`）emit `{ a } = o` + `new_rest_helper_call` 复用 `objectrestspread::new_rest_helper` 排除 leading 键）→ `({ a } = o, r = __rest(o, ["a"]));`（prologue `var __rest = …;`，外层括号保留）→ 绿。**对象 rest 经泛型 flattener 统一、共享 `__rest`。**

> 设计/divergence：(1) Go 的 `f.tx.Visitor().VisitNode`（当前 transformer visitor）在 Rust flattener 中恒为 `destructuring_visit`（根 destructuring 的 visit）；可达子集内（简单 value/kept 元素/rest target）这些 visit 皆恒等，故无影响（非简单 value 的对象 spread 降级被 gate 排除 → verbatim）。(2) `needs_value=true` 值返回路径（尾随 value 折进 comma）已按 Go 写入入口但**本轮接线只传 `false`**（statement 上下文），值位接线 DEFER。(3) `CreateAssignmentCallback`（CJS export/namespace 成员赋值）DEFER（`emit_assignment` 走无回调分支）。(4) `IsLeftHandSideExpression` 守卫在 `is_destructuring_assignment`/`is_assignment_expression_equals` 中被「对象/数组字面量左 + `=` 算子」的种类检查 subsumed（`=` 的左操作数恒为合法赋值目标）。(5) `level >= ObjectRest` 对象路径的「decompose 非 kept 元素 + `computedTempVariables`」分支以 `unreachable!` 占位（gate 保证只有 kept 简单元素到达；计算键/嵌套 rest DEFER）。

**测试计数（6k 新增）**：`tsgo_transformers` +9 `#[test]`（destructuring 赋值 7：array 1 + 非简单 value 1 + object 1 + default 1 + nested 2 + array-rest 1；objectrestspread 对象 rest 赋值 2）+1 doctest（`flatten_destructuring_assignment`）。crate 合计 **115 unit + 23 doctest**（6j 基线 106+22）。

### upstream（ast/printer）增长（6k）

- **无**（6g/6h/6i/6j 的 ZERO upstream growth 连续达成）。赋值展平 + 对象-rest 接线全走 arena 既有构造器（`new_binary_expression`(comma/`=`)/`new_omitted_expression`/`new_object_literal_expression`/`new_array_literal_expression`/`new_parenthesized_expression`/`new_expression_statement`/`new_string_literal`/`new_element_access_expression`/`new_property_access_expression`/`new_call_expression`/`new_conditional_expression`/`new_void_expression`/`new_token`/`new_binding_pattern`/`add_flags`）+ 6c-3 的 `EmitContext` 变量环境 + `factory().new_temp_variable()` + 6d-2 的 `request_emit_helper`/`read_emit_helpers`/`add_emit_helper`（`REST_HELPER` 早在 6d-2 定义）。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## 6m worklog（async 深化 — async 函数表达式 / 方法 / 箭头，red→green 推进记录）

> 本轮把 6d-3 的 `__awaiter` 降级从**顶层 async 函数声明**扩到 **async 函数表达式 / async 方法 / async 箭头**。关键基建：把 6d-3 仅「SourceFile 顶层 + 顶层函数声明」的 visit 改为经 **ec-threaded 泛型 `VisitEachChild`**（`visit_each_child_ec`）递归穿过容器节点——镜像 Go `asyncTransformer.visit` 的 `default: tx.Visitor().VisitEachChild(node)` 默认臂。由于 arena 的 `visit_each_child` 闭包签名是 `FnMut(&mut NodeArena, NodeId)`（拿不到 `&mut EmitContext`，而 helper 请求在 ec 上），实现采用 **map-based 子节点替换**：先 `for_each_child` 收集直接子节点 → drop arena 借用 → 逐子节点 `async_visit(ec, child)`（可请求 `__awaiter`）建替换表 → `visit_each_child` 用表查（无变更子节点回退原节点，map 空时整节点原样返回保 source 位置）。`for_each_child` 是 `visit_each_child` 子节点集的超集，故查表恒成功；漏命中只会少降级（DEFER），不会出错。

先确认基线绿（`cargo test -p tsgo_transformers` = 115 unit + 23 doctest）。逐行为红→绿：

1. **tracer（`async_function_expression_lowers_to_awaiter_wrapper`）**：`const f = async function () { await x; };` → 恒等红（函数表达式藏在 `const f = …` 的变量声明里，旧 `_` 臂用 `|_, c| c` 不递归，原样留存）→ 把 `_` 臂改为 `visit_each_child_ec`（穿过 VariableStatement→DeclList→Decl 到达函数表达式）+ 加 `FunctionExpression if is_async_function_expression` 臂 + `visit_async_function_expression`（剥 async modifier，复用既有 `build_awaiter_wrapper_body` 块包装，首参 `this`）→ `const f = function () { return __awaiter(this, void 0, void 0, function* () { yield x; }); };`（prologue `__awaiter` 定义）→ 绿。**证明 ec-threaded `VisitEachChild` 经容器节点到达嵌套 async 函数并端到端降级。**
2. **`async_method_lowers_to_awaiter_wrapper`（async 方法）**：`class C { async m() { await x; } }` → 恒等红（类体经 `visit_each_child_ec` 递归，但无 MethodDeclaration async 臂）→ 加 `MethodDeclaration if is_async_method` 臂 + `visit_async_method_declaration`（剥 async modifier，复用 `build_awaiter_wrapper_body` 块包装，首参 `this`；ClassDeclaration 由泛型 `visit_each_child_ec` 自然穿过到达方法）→ `class C {\n    m() { return __awaiter(this, void 0, void 0, function* () { yield x; }); }\n}` → 绿。
3. **`async_arrow_lowers_to_awaiter_wrapper_with_lexical_this`（async 箭头）**：`const f = async () => { await x; };` → 恒等红 → 把 `build_awaiter_wrapper_body` 抽出 `build_awaiter_call(ec, body, has_lexical_this)`（参数化首参：`true`→`this`、`false`→`void 0`，对标 Go `NewAwaiterHelper`），`build_awaiter_wrapper_body` 调 `build_awaiter_call(_, _, true)` 再包 `{ return … }`（声明/表达式/方法保持绿）+ 加 `ArrowFunction if is_async_arrow` 臂 + `visit_async_arrow_function`（剥 async modifier，**body 直接为 `build_awaiter_call(_, _, false)` 的简明体，不包块**）→ `const f = () => __awaiter(void 0, void 0, void 0, function* () { yield x; });` → 绿。

> **lexical-this 处理（对 Go 确认）**：Go `transformAsyncFunctionBody` 用 `hasLexicalThis := tx.inHasLexicalThisContext()` 决定 `NewAwaiterHelper` 首参（`true`→`this`、`false`→`void 0`）。非箭头函数（声明/表达式/方法/accessor/constructor）经 `doWithContext(…|asyncContextHasLexicalThis, …)` **置位** lexical-this → 首参 `this`；箭头经 `doWithContext(asyncContextNonTopLevel, …)` **不置位**，继承外层（顶层 SourceFile 初始化为 `false`）→ 首参 `void 0`。本轮可达子集（顶层箭头）首参恒 `void 0`，与 tsc `--target ES2016-` 输出一致。`_this` 捕获是 ES2015 箭头 `this`-capture 关注点（非本 async stage），可达子集不触发；`asyncContextHasLexicalThis` 跨嵌套作用域线程化（async 方法内的箭头应继承 `this`）DEFER。
>
> 设计/divergence：(1) Go 在每个 function-like 臂存/恢复 `lexicalArguments`/super 状态 + 经 `transformAsyncFunctionParameterList`/`transformAsyncFunctionBodyWorker` 处理 super/`arguments`/参数；本轮可达子集（无 super、简单参数列表、无 lexical-`arguments` 使用）下这些均为恒等，故省略，留 DEFER。(2) async 生成器守卫沿用 6d-3（`is_async_*` 查 `asterisk_token.is_none()`，箭头无 asterisk 字段故仅查 async modifier）。(3) 方法重建传 `postfix_token` 透传、asterisk `None`（async 方法非生成器），对标 Go `UpdateMethodDeclaration` 的 `nil postfixToken`——本轮 reachable `m()` 的 postfix 恒 `None`。

**测试计数（6m 新增）**：`tsgo_transformers` +3 `#[test]`（async 函数表达式 1 + 方法 1 + 箭头 1），本轮无新 doctest（`new_async_transformer` 已有 doctest）。crate 合计 **118 unit + 23 doctest**（6k 基线 115+23）。

### upstream（ast/printer）增长（6m）

- **无**（6g/6h/6i/6j/6k 的 ZERO upstream growth 连续达成）。async 函数表达式/方法/箭头降级全走 arena 既有构造器（`new_function_expression`/`new_method_declaration`/`new_arrow_function`/`new_function_declaration`/`new_block`/`new_return_statement`/`new_yield_expression`/`new_call_expression`/`new_keyword_expression`/`new_numeric_literal`/`new_void_expression`/`new_token`）+ `for_each_child`/`visit_each_child`（既有）+ 6d-2 的 `request_emit_helper`/`read_emit_helpers`/`add_emit_helper`/`new_unscoped_helper_name`（`AWAITER_HELPER` 早在 6d-2 定义）。未触碰 `internal/ast/*` 或 `internal/printer/*`。

## 6n worklog（runtimesyntax — enum → IIFE / namespace → IIFE，red→green 推进记录）

> 本轮移植 `tstransforms/runtimesyntax.go` 的两条**互相独立**的运行时降级（各自 tracer + 增量切片），全程 checker-free（可达子集）。结构镜像 `async.rs` 的 ec-threaded visit（`SourceFile`/容器节点经 `visit_each_child_ec` 下钻，enum/module 在 ec 层降级，body 多行经 `MULTI_LINE` emit flag）。

先确认基线绿（`cargo test -p tsgo_transformers` = 118 unit + 23 doctest）。

**Item 1 — enum → IIFE（4 切片）**
1. **tracer（`auto_numbered_enum_lowers_to_iife`）**：`enum E { A, B }` → 恒等 stub 红（verbatim `enum E {\n    A,\n    B\n}`）→ 实现 `visit_enum_declaration` 全 IIFE 骨架（`var E;` + `(function (E) { … })(E || (E = {}))` 经 `wrap_in_iife`）+ `transform_enum_member`（自动编号 + 数字反向映射 `E[E["A"] = 0] = "A";`）+ `enum_qualified_element`/`expression_for_property_name`/`make_var_statement`/`make_parameter`/`make_iife_argument`/`assignment` → `var E;\n(function (E) {\n    E[E["A"] = 0] = "A";\n    E[E["B"] = 1] = "B";\n})(E || (E = {}));` → 绿。**证明 enum 运行时降级经 6a 驱动端到端运行，且 `(E = {})` 括号与 IIFE body 多行均由 printer 正确产出。**
2. **`explicit_numeric_initializer_sets_value_and_continues_autonumber`**：`enum E { A = 5, B }` → 红（slice-1 忽略初值，A 仍为 0）→ `transform_enum_member` 读 `NumericLiteral` 初值（句法 parse 设 `auto_value = n+1`）→ `E[E["A"] = 5] = "A";\n    E[E["B"] = 6] = "B";` → 绿。
3. **`string_initialized_member_omits_reverse_mapping`**：`enum E { X = "v" }` → 红（slice-2 对字符串初值仍走自动编号 + 反向映射）→ 加 `StringLiteral` 臂（值 = 字符串字面量，`use_explicit_reverse_mapping=false`）+ 反向映射条件化 → `E["X"] = "v";` → 绿。
4. **`const_enum_is_omitted`**：`const enum E { A }` → 红（被降级为运行时 IIFE）→ 加 `shouldEmitEnumDeclaration` 守卫（const 修饰符 + `!preserve_const_enums` → `NotEmittedStatement`）→ `（空）` → 绿。const enum 成员引用 inlining 仍 DEFER（inliners stage，blocked-by checker 常量求值）。

**Item 2 — namespace → IIFE（2 切片）**
5. **tracer（`instantiated_namespace_lowers_to_iife`）**：`namespace N { export const x = 1; }` → 红（verbatim namespace）→ 加 `ModuleDeclaration` 臂 + `visit_module_declaration`（复用 `wrap_in_iife`）+ `transform_module_body`（ModuleBlock 语句逐条；exported `VariableStatement` → `lower_exported_variable_statement`）→ exported `const x = 1` 直建 `N.x = init` 赋值语句（`PropertyAccess(N, x) = init`，**checker-free，替代 Go 经 resolver 重写标识符 `x`→`N.x` 的路径**）→ `var N;\n(function (N) {\n    N.x = 1;\n})(N || (N = {}));` → 绿。
6. **`uninstantiated_namespace_is_omitted`**：先观察 RED（临时移除 slice-5 误前置的 `is_instantiated_module` 守卫；按 tdd.md「回到上一个绿点重来」纠正）：`namespace N { interface I {} }` → 红（type-only namespace 被误降级，body 含 `interface I`）→ 恢复 `shouldEmitModuleDeclaration` 守卫（句法 `is_instantiated_module`：ModuleBlock 任一非 `interface`/`type` 语句 → 实例化）→ `（空）` → 绿。

**测试计数（6n 新增）**：`tsgo_transformers` +6 `#[test]`（enum 4 + namespace 2）+1 doctest（`new_runtime_syntax_transformer`）。crate 合计 **124 unit + 24 doctest**（6m 基线 118 + 23）。

### upstream（ast/printer）增长（6n）

- **printer（additive，唯一一处增长）**：`internal/printer/printer.rs` 两处把 Go `Block.MultiLine` 字段（Rust AST 未携带，原为 `if false` TODO 占位）改读 `MULTI_LINE` emit flag——`emit_block`（多行格式选择）与 `should_emit_block_function_body_on_single_line`（函数体多行守卫）。空表/未置 flag 时行为不变（既有 175 printer unit 测试无回归）；transformer 经 `set_emit_flags(body, EmitFlags::MULTI_LINE)` 触发 IIFE body 多行。`should_emit_on_multiple_lines` 既已存在，仅新增两处调用点。
- **ast**：**无**。enum/namespace 降级全走 arena 既有构造器（`new_variable_statement`/`new_variable_declaration(_list)`/`new_function_expression`/`new_parameter_declaration`/`new_parenthesized_expression`/`new_call_expression`/`new_expression_statement`/`new_binary_expression`/`new_element_access_expression`/`new_property_access_expression`/`new_object_literal_expression`/`new_numeric_literal`/`new_string_literal`/`new_identifier`/`new_token`/`new_syntax_list`/`new_not_emitted_statement`/`new_block`）+ `for_each_child`/`visit_each_child`。
- **divergences（checker-free 可达子集）**：(1) enum 成员值句法求值（自动编号 + 数字/字符串字面量初值）替代 checker `GetEnumMemberValue`；非字面量初值常量折叠 DEFER。(2) IIFE 容器/参数名用声明名文本替代 `NewGeneratedNameForNode`（顶层、非 merged、非 exported 子集等价）。(3) namespace exported 成员直建 `N.x = init` 替代 Go 经 binder `ReferenceResolver` 重写标识符的路径（reachable 子集输出等价）。(4) `is_instantiated_module` 为 `getModuleInstanceState` 的句法子集（type-only = `interface`/`type`；const-enum-only / import-export alias-target 分析 DEFER）。

## 6o worklog（classfields 深化 — ClassExpression 实例字段，red→green 推进记录）

> 本轮深化 `estransforms/classfields.go` 可达面。逐 stage 评估剩余 classfields 切片（static 边缘、accessor、私有方法、class-expr）后，确认 **ClassExpression 实例字段** 是唯一 checker-free 且无新基建依赖的干净可达切片：static 字段已在 6c-3 完整覆盖（多 static / static+instance / 派生类 static 均已工作，无遗留 RED）；`accessor` 字段需 emit-context name generator 生成 backing 私有名 + 二遍 result visitor，私有方法需 `WeakSet` brand-check 基建——两者均 DEFER（blocked-by 见下）。

先确认基线绿（`cargo test -p tsgo_transformers` = 124 unit + 24 doctest）。

**ClassExpression 实例字段（2 切片）**
1. **tracer（`class_expression_instance_field_moves_to_constructor`）**：`const C = class { x = 1 };` → 恒等红（`class_fields_visit` 仅判 `Kind::ClassDeclaration`，class-expr 字段未降级，verbatim `const C = class {\n    x = 1;\n};`）→ 实现：`class_fields_visit` 命中 `ClassDeclaration | ClassExpression` 均走 `try_lower_simple_class`；后者 `match` 同时接 `NodeData::ClassDeclaration(d) | ClassExpression(d)`、捕获 `kind`、用 `new_class_like(kind, …)` 按种类重建 → `const C = class {\n    constructor() { this.x = 1; }\n};` → 绿。**纯实例字段降级结果为单节点（无 hoist），在表达式位安全。**
2. **guard（`class_expression_with_static_field_is_left_unchanged`）**：`const C = class D { static x = 1 };`（**具名**类表达式，避免匿名类经 `name?` 提前 bail）→ 红（static 字段产 `SyntaxList[class, D.x = 1]`，printer 在表达式位 panic `emit_expression_node: unhandled kind SyntaxList`）→ 在 `try_lower_simple_class` 收尾分支加守卫：`kind == Kind::ClassExpression` 且需 hoist（`pre_statements`/`static_assignments` 非空 → 本应产 `SyntaxList`）时返回 `None`，整类表达式保持不变 → `const C = class D {\n    static x = 1;\n};` → 绿。**对应 Go `visitClassExpression` 的 `pendingExpressions`/temp-wrapper（IIFE/逗号序列）分支——未移植，故延后。**

**测试计数（6o 新增）**：`tsgo_transformers` +2 `#[test]`（class-expr 实例字段 1 + static guard 1），本轮无新 doctest（`new_class_fields_transformer` 已有 doctest）。crate 合计 **126 unit + 24 doctest**（6n 基线 124 + 24）。

### upstream（ast/printer）增长（6o）

- **无**。ClassExpression 降级复用既有 `new_class_like`（已支持 `Kind::ClassExpression` 分派 `NodeData::ClassExpression`）+ 既有构造器插入逻辑 + `visit_each_child` 默认递归。未触碰 `internal/ast/*` 或 `internal/printer/*`。

### DEFER（本轮确认的 blocked-by）

- **`accessor` 自动访问器**（`accessor x = 1` → 合成 `#x` backing 私有字段 + get/set 重定向器）：blocked-by emit-context **name generator**（`NewTempVariable`/`NewGeneratedNameForNode` 生成 backing 私有名）+ Go 的 `accessorFieldResultVisitor` **二遍 result visitor**（均未移植）。
- **私有方法/accessor**（`#m() {}` → `WeakSet` brand-check + lowered function）：blocked-by `WeakSet` brand 基建（`_C_instances = new WeakSet()` + 构造器 `.add(this)` + 访问处 brand check）+ name generator；当前 `build_private_env` 见私有方法/accessor 即返回 `None` 整类延后（守卫已就位）。
- **私有 static 字段**、**named-helper 私有形态** `__classPrivateFieldGet/Set`（需 helper-library import emit）。
- **ClassExpression 语句 hoist**（6o 已落地纯实例字段子集）：static/私有/计算字段在表达式位需 IIFE / 逗号序列 + temp 包裹（Go `visitClassExpression` 的 `pendingExpressions` 分支），需 emit-context name generator + 变量声明 hoist 接线。
- **`--target` / `useDefineForClassFields` 门控**：需 compilerOptions + checker。

## 6p note（printer name-generator 上游解锁 — 非 transformers 代码改动）

> 本轮（6p）是 **transformers 链上游基建轮**：完成 `tsgo_printer` emit-context 的 **node-based name generator**（`GenerateNameForNode`），**未改动 `internal/transformers/**`**（transformers 测试计数保持 6o 的 126 unit + 24 doctest）。记于此是为说明 classfields DEFER 列表的解锁状态。

**printer 侧 upstream 增长（additive，全部在 `tsgo_printer`）**：
- `EmitContext`：`get_node_for_generated_name`(+worker)。
- `NodeFactory`：`new_generated_name_for_node(_ex)` / `new_generated_private_name_for_node(_ex)`。
- `NameGenerator`：`generate_name` 的 `is_node()` 分派（此前 `todo!()`）+ node-id 缓存 + `generate_name_for_node(_cached)` kind switch（Identifier/Function-Class-decl/ExportAssignment/ClassExpression/Method-Accessor/ComputedPropertyName/default）。详见 `phase-4-checker/printer/impl.md` 的「Round 6p worklog」。
- printer 既有 `get_text_of_node` → `name_generator.generate_name` 接线早已存在，故 transform 造的 `new_generated_name_for_node`/`new_temp_variable` 标识符在 emit 时自动 materialize 成唯一名。

**对 classfields DEFER 的影响（现已可在后续 transformers 轮消费）**：
- `accessor` 自动访问器：backing 私有名可经 `new_generated_private_name_for_node`（仍需 Go 的 `accessorFieldResultVisitor` 二遍 result visitor）。→ **✅ 6q 已消费**（ES2022-native 形态，backing 字段保留为成员，不跑二遍 result visitor 的 WeakMap 降级；见下 6q worklog）。
- 私有方法/accessor（`WeakSet` brand）：brand 名 `_C_instances` 可经 name generator（仍需 `WeakSet` brand-check 基建）。
- **ClassExpression 语句 hoist**：class-expr 在表达式位的 IIFE/逗号序列包裹所需的 temp 名可经 `new_temp_variable`（仍需 `pendingExpressions`/temp-wrapper 接线）。
- 当前 classfields 的确定性 `_C_x`/`_a` 命名（理论碰撞）可改走 name generator 获得真正唯一名。

> 即：6p 提供"生成确定性唯一名"的能力；把它**应用到** classfields 各形态的 lowering（accessor backing / 私有方法 brand / class-expr hoist）是**下一个 transformers 轮**的工作（本轮不动 transformers 代码，避免半成品）。

## 6q worklog（classfields 深化 — `accessor` 自动访问器，消费 6p generated private name，red→green 推进记录）

> 本轮把 6p 的 node-based generated private name 生成能力**应用到** classfields 的 `accessor` 自动访问器降级（DEFER 列表里最可达的一项）。Go ground truth：`transformAutoAccessor`（`classfields.go:833`）合成 backing 私有字段 + get/set 重定向器，三者经 `NewGeneratedPrivateNameForNodeEx(node.Name(), {Suffix:"_accessor_storage"})` 取**同一**生成私有名（name generator 按解析后的源节点 id 缓存，故三处 emit 同名）。

先确认基线绿（`cargo test -p tsgo_transformers` = 126 unit + 24 doctest）。

**关键可达性裁剪（落地 ES2022-native 形态）**：Go `transformAutoAccessor` 末尾把 `[backingField, getter, setter]` 再过一遍 `accessorFieldResultVisitor`——在下级 target（`shouldTransformPrivateElements` 开）会把 backing 私有字段进一步降级成 WeakMap/`__classPrivateFieldGet`。但本端口的既有私有字段降级是**文本驱动**的直接 WeakMap 形态（`_C_<text>` brand + 文本匹配），而 6p 生成私有名在 transform 时**文本为空/占位**（仅 emit 时由 name generator materialize），二者不兼容。故本轮落地 **`target: ES2022`（私有元素 native、auto-accessor 始终降级）** 这个忠实子集：backing 私有字段保留为类成员，**不**再跑二遍 WeakMap 降级。该形态与 TS `target=ES2022, useDefineForClassFields=true` 输出逐字一致。

**ec 线程化（取 `EmitContext::factory()`）**：既有 `class_fields_visit` 仅持 `&mut NodeArena`，无法调 name generator。新增 ec-threaded 入口 `class_fields_visit_ec` + `visit_each_child_ec`（镜像 `async.rs` 的 map-based 子节点替换），穿过非类节点到达类节点；命中类时若含 `accessor` 成员 → `try_lower_auto_accessor_class(ec, …)`（可取 `ec.factory().new_generated_private_name_for_node_ex`），否则回落既有 arena-only `try_lower_simple_class`。

**红→绿切片（逐行为）**：
1. **tracer（`instance_auto_accessor_lowers_to_backing_field_and_redirectors`）**：`class C { accessor x = 1; }` → 红（既有代码把 `accessor x` 误当普通实例字段，产 `class C {\n    constructor() { this.x = 1; }\n}`，丢失访问器语义）→ 实现 `class_has_auto_accessor` 检测 + `try_lower_auto_accessor_class`/`expand_auto_accessor`（backing 私有名经 6p `new_generated_private_name_for_node_ex({suffix:"_accessor_storage"})`，三处引用同一 `x` 名节点 → 同名）+ `build_accessor_get_redirector`（`get x() { return this.#x_accessor_storage; }`）/`build_accessor_set_redirector`（`set x(value) { this.#x_accessor_storage = value; }`）→ `class C {\n    #x_accessor_storage = 1;\n    get x() { return this.#x_accessor_storage; }\n    set x(value) { this.#x_accessor_storage = value; }\n}` → 绿。**6p 生成私有名在 emit 时 materialize 成 `#x_accessor_storage`，三处一致。**
2. **`static_auto_accessor_keeps_static_modifier`（genuine 扩展，需新代码）**：`class C { static accessor x = 1; }` → 红（slice-1 仅接 `modifier_flags == ACCESSOR`，static 整类不变）→ 放宽校验为 `modifier_flags ⊆ {ACCESSOR, STATIC}` + `strip_accessor_modifier`（经 `extract_modifiers(…, ModifierFlags::STATIC)` 保留 `static`、丢 `accessor`，空表归一为 `None`）→ backing 字段/get/set 均带 `static`，receiver 仍用 `this`（static 成员内 `this` 即类对象）→ `class C {\n    static #x_accessor_storage = 1;\n    static get x() { return this.#x_accessor_storage; }\n    static set x(value) { this.#x_accessor_storage = value; }\n}` → 绿。
3. **coverage（`class_expression_auto_accessor_lowers_in_place`、`auto_accessor_without_initializer`）**：统一的 `try_lower_auto_accessor_class` 同时接 `NodeData::ClassDeclaration | ClassExpression`（`new_class_like(kind, …)` 按种类重建），且 initializer 为 `Option` 直透 → 两形态在 slice-1/2 实现下即绿（非独立 RED，作为可达面表征测试锁定：class-expr `const C = class { accessor x = 1 };`、无初值 `accessor x;`）。

**DEFER 守卫（保证不误降级）**：含 `accessor` 的类若该 accessor 形态不支持（计算/私有/字符串名、装饰器、可见性/readonly 修饰），`expand_auto_accessor` 返回 `None` → `try_lower_auto_accessor_class` 返回 `None` → 整类**保持不变**（不回落 `try_lower_simple_class` 以免按普通字段误降级）；混入需降级的普通/static/私有字段或构造器的 accessor 类同样整类延后（`PropertyDeclaration`/`Constructor` 邻居 → `None`）。

**测试计数（6q 新增）**：`tsgo_transformers` +4 `#[test]`（instance tracer 1 + static 1 + class-expr coverage 1 + no-init coverage 1），本轮无新 doctest。crate 合计 **130 unit + 24 doctest**（6o/6p 基线 126 + 24）。

### upstream（ast/printer）增长（6q）

- **无**。accessor 降级全走 arena 既有构造器（`new_property_declaration`/`new_accessor_declaration`/`new_parameter_declaration`/`new_property_access_expression`/`new_return_statement`/`new_block`/`new_binary_expression`/`new_expression_statement`/`new_keyword_expression`/`new_identifier`/`new_token`/`new_class_like`）+ 既有 `for_each_child`/`visit_each_child` + **6p 既有** printer `EmitContext::factory().new_generated_private_name_for_node_ex` / name generator（无新增 printer API）。未触碰 `internal/ast/*` 或 `internal/printer/*`。

### DEFER（本轮确认的 blocked-by，accessor 维度）

- **accessor 的 WeakMap / 下级 target 形态**：Go 二遍 `accessorFieldResultVisitor` 把 backing 私有字段再降级为 WeakMap/`__classPrivateFieldGet`。blocked-by：本端口的私有字段降级是文本驱动直接 WeakMap，与 6p 生成私有名（transform 时空文本）不兼容；需让 WeakMap brand 命名也走 name generator（或移植 named-helper 形态 + target 门控）。
- **计算名 / 装饰 / 可见性修饰 / 混入其他需降级成员的 accessor 类**：需计算名 cache（`findComputedPropertyNameCacheAssignment` + temp）/ 装饰器 / 与构造器插入族协同 —— 守卫已就位（不支持形态整类延后）。
- 其余 classfields DEFER（私有方法/accessor WeakSet、私有 static、class-expr 语句 hoist、参数属性、门控）同 6o 列表不变。

## 6r worklog（classfields 深化 — class-expression 语句 hoist（static 字段），消费变量环境 + 6p temp 生成器，red→green 推进记录）

> 本轮移除 6o 的「class-expression 含 static 字段→保持不变」守卫的**可达子集**，把表达式位含 static 字段的类表达式降级为 **逗号序列 + temp** 包裹（Go `visitClassExpressionInNewClassLexicalEnvironment` 的 `hasTransformableStatics` 分支：`InlineExpressions([temp = class, static assignments..., temp])`）。Go ground truth：`classfields.go:2006`。temp 经 `NewTempVariableEx` 分配并 `AddVariableDeclaration(temp)` hoist 成 `var _a;`。
>
> 先确认基线绿（`cargo test -p tsgo_transformers` = 130 unit + 24 doctest）。

**复用基建**：(1) **6c-3/6i 变量环境**：`class_fields_visit_ec` 新增 `SourceFile` 臂 `visit_source_file_ec`（`start_variable_environment` → 逐语句递归 → `end_variable_environment` 把 hoist 的 `var _a;` 前置），与 `exponentiation.rs` 的 `visit_source_file` 同形；(2) **6p temp 生成器**：`ec.factory().new_temp_variable()` 产 `_a`（emit 时 materialize），同一 temp NodeId 在 `_a = class` / `_a.x` / 尾随 `_a` 三处复用 → 同名；(3) `ec.add_variable_declaration(temp)` 登记 `var _a;`。

**重构（绿态下，服务新行为）**：抽 `lower_class_parts(arena, node) -> Option<ClassLoweringParts>`（`{class, pre_statements, static_fields: Vec<(name, init)>}`），把 static 字段保留为**受体无关**的 `(name, init)` 对（不再在成员循环里直接产 `C.x = 1` 语句）。两消费者：`try_lower_simple_class`（声明位/纯实例字段类表达式：建 `C.x = init` 语句 + `SyntaxList`）与新增 ec 线程化 `try_lower_class_expression`（表达式位：建 `_a.x = init` 表达式 + 逗号序列）。重构后全量 130 unit 仍绿（行为保持）。

**红→绿切片（逐行为）**：
1. **tracer（`class_expression_static_field_hoists_to_comma_sequence_with_temp`）**：`const C = class { static x = 1 };` → 红（6o 守卫令其恒等：`const C = class {\n    static x = 1;\n};`）→ 实现 `try_lower_class_expression`（`pre_statements` 空且 `static_fields` 空 → 纯实例字段，原样返回 class[6o]；`pre_statements` 非空 → DEFER 返回 `None`；否则 static-only → `_a = class`、各 `_a.x = init`、尾随 `_a` 折成逗号 `BinaryExpression`）+ `make_temp_static_assignment`（`_a.x = init` 表达式）→ `var _a;\nconst C = (_a = class {\n}, _a.x = 1, _a);` → 绿。**逗号序列在 `const` 初值位被 printer 的 `DISALLOW_COMMA` 优先级自动加括号；空类体打印为 `class {\n}`（端口既有 printer 行为）。**
2. **`named_class_expression_static_field_keeps_name_in_comma_sequence`（替换 6o 守卫）**：`const C = class D { static x = 1 };` → 重构后由恒等变红 → 类名 `D` 保留在被包裹的类表达式里，static 受体仍是 temp `_a`（非类名）→ `var _a;\nconst C = (_a = class D {\n}, _a.x = 1, _a);` → 绿。**温故：6o 用具名类是为绕过匿名类 `name?` 早退；temp 包裹后受体改用 `_a`，匿名/具名同形（仅保不保留名之差），故匿名 tracer 亦可达。**
3. **coverage（`class_expression_multiple_static_fields_share_one_temp`）**：`const C = class { static x = 1; static y = 2 };` → 多 static 字段在 `static_fields` 循环里产多个 `_a.<n> = …` 并共用单 temp → `var _a;\nconst C = (_a = class {\n}, _a.x = 1, _a.y = 2, _a);` → 即绿（同 tracer 路径，锁多字段可达面）。
4. **coverage（`class_expression_instance_and_static_fields_lower_together`）**：`const C = class { x = 1; static y = 2 };` → 实例字段进合成构造器（在被包裹的 class 值内），static 字段进逗号序列 → `var _a;\nconst C = (_a = class {\n    constructor() { this.x = 1; }\n}, _a.y = 2, _a);` → 即绿（构造器插入族 + static 逗号序列协同）。

**DEFER 守卫（保证不误降级）**：`try_lower_class_expression` 在 `pre_statements` 非空（计算名 temp 缓存 / `var _C_x = new WeakMap();` 私有 brand）时返回 `None` → 类表达式整体保持不变。`class_expression_with_computed_field_is_left_unchanged`（`const C = class { [k] = 1 };` → 恒等）锁定该 DEFER 边界。blocked-by：Go 用 `pendingExpressions` 把计算名 key 内联进逗号序列（`_a = k, …`），而本端口的计算名处理走**确定性 `_a` + `SyntaxList` 语句** hoist，二者机制不同；私有 static / 私有实例（WeakMap brand）在表达式位同需 `pendingExpressions` 接线。

**测试计数（6r 新增）**：`tsgo_transformers` +4 `#[test]`（tracer 1 + 多 static 1 + 混合 instance+static 1 + 计算名 DEFER 守卫 1；另：6o 的 `class_expression_with_static_field_is_left_unchanged` 被**改名重写**为 `named_class_expression_static_field_keeps_name_in_comma_sequence`，净计数 +0），本轮无新 doctest。crate 合计 **134 unit + 24 doctest**（6q 基线 130 + 24）。

### upstream（ast/printer）增长（6r）

- **无**。逗号序列/temp 包裹全走 arena 既有构造器（`new_binary_expression` + `Kind::CommaToken`/`EqualsToken` token、`new_property_access_expression`）+ 既有 printer 优先级括号（`emit_initializer` 的 `DISALLOW_COMMA` 已能为逗号序列加括号）+ **6c-3/6i 既有** `EmitContext` 变量环境（`start/end_variable_environment`、`add_variable_declaration`）+ **6p 既有** `factory().new_temp_variable`。未触碰 `internal/ast/*` 或 `internal/printer/*`。

### DEFER（本轮确认的 blocked-by，class-expr 维度）

- **class-expr 计算名 / 私有实例（WeakMap）/ 私有 static 字段的语句 hoist**：均需 Go `pendingExpressions` 内联进逗号序列；本端口计算名/私有降级走确定性命名 + `SyntaxList` 语句，机制不兼容。blocked-by：移植 `pendingExpressions` 接线（或让计算名/WeakMap brand 命名走 name generator 并产表达式而非语句）。
- **函数体内 / 嵌套作用域的 class-expr static hoist**：本轮仅在 `SourceFile` 开变量环境；嵌套作用域（函数体、块）的 temp hoist 需按 6i 模式逐作用域线程化 `VisitFunctionBody`（exponentiation 已有，可后续复用）。
- 其余 classfields DEFER（私有方法/accessor WeakSet、参数属性、`--target`/`useDefineForClassFields` 门控）同 6o/6q 列表不变。

## 6s worklog（optionalchain `(a?.b)()` this-capture + `delete a?.b`，新增 `SyntheticReferenceExpression` AST 节点；red→green 推进记录）

> 本轮解锁 6h 报告里 DEFER 的两个 optionalchain 子集：括号可选调用的 **this-capture**（`(a?.b)()`）与 **`delete a?.b`**。前者是 6h 注记中「唯一需 AST 增长的剩余子集」——需新增 Go `SyntheticReferenceExpression` 工厂节点（捆绑 `expression` + `thisArg`，供把调用降级成 `<access>.call(thisArg, …)`）。结构镜像 Go `optionalchain.go:visitCallExpression`/`visitParenthesizedExpression`/`visitNonOptionalExpression`/`visitDeleteExpression` + `visitOptionalExpression` 的 captureThisArg/isDelete 分支。
>
> **scope**：`-p tsgo_ast`（additive 节点）+ `-p tsgo_transformers`；printer/checker 仅 `cargo build` 确认 additive-safe（均无需改动，见下）。

先确认基线绿（`cargo test -p tsgo_ast` = 53 unit + 26 doctest；`cargo test -p tsgo_transformers` = 134 unit + 24 doctest）。逐行为红→绿：

**AST 节点（tracer，`-p tsgo_ast`）**
1. **`synthetic_reference_expression_visits_expression_then_this_arg`**：`new_synthetic_reference_expression(expr, this_arg)` → 编译红（构造器不存在）→ 新增 `NodeData::SyntheticReferenceExpression(SyntheticReferenceData{expression, this_arg})` variant + 构造器（lib.rs）+ `for_each_child`(lib.rs) + `visit_each_child`(visitor.rs)，遍历序 `expression`→`this_arg` → kind == `SyntheticReferenceExpression`、子节点 `[expression, this_arg]` → 绿。`Kind::SyntheticReferenceExpression` 早在 `kind_generated.rs`，故只补 `NodeData` 侧。详见 ast `impl.md` 「P5 期附加」。

**transformer（`-p tsgo_transformers`）**
2. **`parenthesized_optional_call_captures_this`（this-capture，tracer）**：`(a?.b)();` → 红（旧行为 `(a === null || a === void 0 ? void 0 : a.b)();`，丢失 `this`）→ 在 ec-threaded `optional_chain_visit` 加非可选 `CallExpression` 臂 `visit_call_expression`：检测被括号包裹且 `skip_parentheses` 后含 `OPTIONAL_CHAIN` 的 callee → `visit_parenthesized_expression`(captureThisArg=true) → `visit_non_optional_expression` → `lower_optional_expression`（扩 `capture_this_arg`/`is_delete` 参 + 新 `build_chain_segments_capturing`：末段 property/element access 若 captureThisArg 则捕获 thisArg（简单 receiver 直用、否则 hoist temp），并把结果包进 `SyntheticReferenceExpression`）→ 经 `new_function_call_call`（`target.call(thisArg, …args)`）→ `(a === null || a === void 0 ? void 0 : a.b).call(a);` → 绿。baseline: `conformance/.../callChain/parentheses.ts`。
3. **`delete_optional_access_lowered`（delete，tracer）**：`delete a?.b;` → 红（旧行为 `delete (a === null || … : a.b);`，delete 落在条件式整体上）→ 加 `DeleteExpression` 臂 `visit_delete_expression`：操作数 `skip_parentheses` 后含 `OPTIONAL_CHAIN` → `visit_non_optional_expression(isDelete=true)` → `lower_optional_expression(is_delete=true)`：守卫真值支用 `true`（`new_keyword_expression(Kind::TrueKeyword)`）、访问支用 `delete <access>`（`new_delete_expression`）→ `a === null || a === void 0 ? true : delete a.b;` → 绿。baseline: `conformance/.../delete/deleteChain.ts`。
4. **`delete_parenthesized_optional_access_keeps_parens`（泛化，直接绿）**：`delete (a?.b);` → `visit_delete_expression` 操作数为括号 → `visit_non_optional_expression` → `visit_parenthesized_expression(isDelete=true)`，内层降级（thisArg 为空，非 synthetic-ref）后重新包回括号 → `(a === null || a === void 0 ? true : delete a.b);`（保留原括号，delete 仍在 present 支）→ 直接绿。baseline: deleteChain.ts `delete (o1?.b);`。

> 设计/divergence：(1) `SyntheticReferenceExpression` 是 transform-only 节点，仅在 transform 内被 `visit_call_expression`/`visit_parenthesized_expression` 消费（解开成 `.call(…)` 或重包括号），**不进入 emit**；故顶层 `optional_chain_visit` 的可选 access 臂传 `capture_this_arg=false`，永不产出 synthetic-ref 给 printer。(2) `skip_parentheses_is_optional_chain` 为本模块私有 arena 读取器（穿过嵌套括号判 `OPTIONAL_CHAIN`），镜像 Go `SkipParentheses`，**未碰 `internal/ast/*`** 的 utilities。(3) `new_function_call_call` 镜像 Go `printer/factory.go:NewFunctionCallCall`（`target.call(thisArg, …args)` = `NewMethodCall(target,"call",[thisArg,…])`），就地用 arena 构造器，未新增 printer 工厂。(4) Go 的 `RestoreOuterExpressions`/`SetOriginal`/`AddEmitFlags(EFNoComments)`/super→this 改写、以及 call 段 `leftThisArg`（`a?.b?.()` 这类多链 call this 线程化）仍 **DEFER**（不在 `(a?.b)()`/`delete a?.b` 可达子集内）。

**测试计数（6s 新增）**：`tsgo_ast` +1 `#[test]`（synthetic-ref 构造+遍历）+1 doctest（`new_synthetic_reference_expression`）→ **54 unit + 27 doctest**（基线 53+26）。`tsgo_transformers` +3 `#[test]`（this-capture 1 + delete 1 + 括号 delete 泛化 1），本轮无新 doctest → **137 unit + 24 doctest**（6r 基线 134+24）。

### upstream（ast/printer/checker）增长（6s）

- **ast**：**新增**（additive）`NodeData::SyntheticReferenceExpression(SyntheticReferenceData{expression, this_arg})` + 构造器 `new_synthetic_reference_expression` + `for_each_child`/`visit_each_child` 两处穷尽匹配臂。`Kind` 侧无新增（`Kind::SyntheticReferenceExpression` 既有）。详见 ast `impl.md`/`tests.md`「P5 期附加」。
- **printer / checker**：**无需改动**。printer 主 emit 分发按 `Kind`（含 catch-all）、checker 不对 `NodeData` 做穷尽匹配，故新 `NodeData` variant 不破坏其编译；`cargo build -p tsgo_printer -p tsgo_checker` 均绿，**未触碰其源码**（无 compile-only arm）。synthetic-ref 在 emit 前已被 transform 解开，printer 无需 arm（与 Go 一致）。

### DEFER（本轮确认的 blocked-by，optionalchain 维度）

- **call 段 this 线程化（`a?.b?.()` / `a?.()` 多链 call 的 `leftThisArg`）**：Go `visitOptionalExpression` 在首段为 call 且左侧带 thisArg 时用 `NewFunctionCallCall` 线程化 `this`（含 `super`→`this` 改写、`EFNoComments` clone）。本轮只做 property/element 末段 captureThisArg（`(a?.b)()` 可达子集）。blocked-by：`leftThisArg` 线程化 + `EmitContext.HasAutoGenerateInfo`/`AddEmitFlags`/`clone` 接线。
- **`RestoreOuterExpressions` / `SetOriginal` / `node.Loc` 透传**：本端口合成节点不携带 Loc/Original（沿用 6g/6c-1 注记，emit 不依赖）；如后续 sourcemap/comment parity 需要再回填。
- **嵌套作用域内的括号可选调用 / delete temp-hoist**：本轮 this-capture 的非简单 thisArg temp 走 `add_variable_declaration`（落最近 var-env），可达；但更深的「控制流体 / switch case / 对象方法简写」嵌套仍随 6i DEFER 列表不变。

## 6t worklog（optionalchain 可选 call 段 `leftThisArg` 线程化；red→green 推进记录）

> 本轮解锁 6s DEFER 的 **call 段 `this` 线程化**：当 optional-chain 的首段是 call（`<receiver>?.()`）时，receiver 自身的接收者要作为该调用的 `this`（Go 的 `leftThisArg`），否则降级成裸 `_t()` 会丢 `this`。复用 6s 落地的 `SyntheticReferenceExpression`（`expression` + `this_arg`）+ `new_function_call_call`，以及 6c-3/6i 的 temp/var-env。结构镜像 Go `optionalchain.go:visitOptionalExpression`（`isCallChain(chain[0])` → receiver `this`-capture；segment loop 首段 call + `leftThisArg != nil` → `NewFunctionCallCall`）+ `visitPropertyOrElementAccessExpression`（非可选 access 的 captureThisArg 分支）。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing，**ZERO** ast/printer/checker 增长——6s 已加 `SyntheticReferenceExpression`）。

先确认基线绿（`cargo test -p tsgo_transformers` = 137 unit + 24 doctest）。逐行为红→绿：

1. **`optional_member_then_optional_call_threads_this`（tracer，genuine RED）**：`a?.b?.();` → 红（旧行为 `… ? void 0 : _a()`，丢 `this`）→ 在 `lower_optional_expression` 把 receiver 改走 `visit_non_optional_expression(receiver, capture=is_call_chain(chain[0]), false)`，若返回 `SyntheticReferenceExpression` 则拆出 `left_this_arg` + `captured`；`build_chain_segments_capturing` 增 `left_this_arg` 参，首段 call 且 `i==0` 时经 `prepare_call_this_arg` + `new_function_call_call` 线程化 → `var _a;\n(_a = a === null || a === void 0 ? void 0 : a.b) === null || _a === void 0 ? void 0 : _a.call(a);` → 绿。inner `a?.b` 经既有可选-access `capture_this_arg` 路径产出 synthetic-ref（thisArg=`a`）。baseline: `conformance/.../callChain/callChain.3.ts`（`a?.m?.({x:12})`）。
2. **`nested_optional_member_then_optional_call_threads_temp_this`（泛化，直接绿）**：`a?.b.c?.();` → inner `a?.b.c` 末段 `.c` 的 receiver `a.b` 非简单 → 捕获 `this` 时 hoist `_a`（`(_a = a.b).c`），外层 conditional hoist `_b`，call 段线程化 → `var _a, _b;\n(_b = a === null || a === void 0 ? void 0 : (_a = a.b).c) === null || _b === void 0 ? void 0 : _b.call(_a);`。`_a` 为 auto-gen temp（`has_auto_generate_info`）→ `prepare_call_this_arg` 直用不 clone。直接绿（slice 1 实现已泛化此分支）。
3. **`non_optional_member_then_optional_call_threads_this`（genuine RED）**：`a.b?.();`（**非可选** member receiver）→ 红（`… ? void 0 : _a()`）→ `visit_non_optional_expression` 增非可选 property/element access 臂 `visit_access_capturing_this`（镜像 Go `visitPropertyOrElementAccessExpression` 非可选 + captureThisArg）：access 自身接收者（简单直用、否则 hoist temp）作 thisArg，重建 access 包进 `SyntheticReferenceExpression` → 外层 hoist `_a`、call 段 `_a.call(a)` → `var _a;\n(_a = a.b) === null || _a === void 0 ? void 0 : _a.call(a);` → 绿。baseline: `conformance/.../callChain/callChain.js`（`o3.b?.().c`）。

> 设计/divergence：(1) `prepare_call_this_arg` 镜像 Go 的 leftThisArg 处理——非 auto-gen 节点 `clone_node` + `set_emit_flags(… | NO_COMMENTS)`（避免把 receiver 注释重复到 `this` 实参），auto-gen temp 直用；`super` receiver → `None`（DEFER，需 super→this 改写）。(2) receiver 经 `visit_non_optional_expression` 而非 `optional_chain_visit`：当 `capture=false`（首段非 call，如 `a?.b`/`f()?.b`/`a?.b?.c`）两者等价（已有用例保持绿），`capture=true` 才捕获 `this`。(3) `is_call_chain` 镜像 Go `isCallChain`（`CallExpression` 且带 `OPTIONAL_CHAIN`）。(4) element-access 参数沿用 arena-only `optional_chain_visit_arena`（与 `build_chain_segments_capturing` 一致），与 Go `VisitNode` 的细微差异同既有注记。(5) 仍 DEFER：`super` call receiver、tagged template。

**测试计数（6t 新增）**：`tsgo_transformers` +3 `#[test]`（`a?.b?.()` genuine RED 1 + `a?.b.c?.()` 泛化 1 + `a.b?.()` genuine RED 1），本轮无新 doctest → **140 unit + 24 doctest**（6s 基线 137+24）。

### upstream（ast/printer/checker）增长（6t）

- **无**。复用 6s 的 `SyntheticReferenceExpression` 节点 + `new_function_call_call` + `EmitContext.{has_auto_generate_info,emit_flags,set_emit_flags}` + `NodeArena.clone_node`（均既有），未触碰 `internal/ast/*`、`internal/printer/*`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，optionalchain 维度）

- **call 段 `super` receiver（`super.b?.()`）**：`prepare_call_this_arg` 命中 `Kind::SuperKeyword` 返回 `None`（整链 verbatim）。blocked-by：Go 的 `super`→`this`（`NewThisExpression`）改写未接线；baseline `callChainWithSuper`。
- **tagged-template 段 / call receiver（`a?.()?.()`）**：`build_chain_segments_capturing` 段 kind 越界（tagged template）仍 `None`；`visit_non_optional_expression` 的 `CallExpression` receiver 走 fallthrough 未做 `this`-capture。blocked-by：`flattenChain` 的 tagged-template 分支 + call-receiver `visitCallExpression(captureThisArg)`。
- **`RestoreOuterExpressions` / `SetOriginal` / `node.Loc` 透传**：沿用 6s 注记，合成节点不携带 Loc/Original（emit 不依赖），sourcemap/comment parity 需要时回填。

## 6u worklog（`esmodule` ESM 变换可达子集；red→green 推进记录）

> 本轮落地 `moduletransforms/esmodule.rs` 的可达核心（`--module es2015/esnext`）。ESM 目标与 CommonJS 不同：**保留** import/export 语法，故 value import/export 走恒等透传；结构性可达面是 `visitSourceFile` 守卫、`export =`/`import =` 的 elision、以及 `createEmptyImports`。无真实 ReferenceResolver / EmitResolver，故 **type-only import elision** 与**作用域正确引用重写** DEFER（与 6e-2 CommonJS 同缺口）。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing）。复用 6e `collect_external_module_info` 的同源 `IsExternalModuleIndicator` 结构逻辑（本轮以本地 `statement_is_external_module_indicator` 镜像 `internal/ast/utilities.go:IsExternalModuleIndicator`，供 `createEmptyImports` 判据）。**ZERO** ast/printer/checker 增长（全走 arena 既有构造器 `new_named_exports`/`new_export_declaration`/`new_source_file`）。

先确认基线绿（`cargo test -p tsgo_transformers` = 140 unit + 24 doctest）。逐行为红→绿：

1. **`value_import_and_use_preserved_under_esnext`（tracer，genuine RED）**：先建 `esmodule.rs` 桩（`es_module_visit` = `todo!()`）+ `esmodule_test.rs` tracer + `mod.rs` 注册 → `todo!()` panic 红 → 实现 `new_es_module_transformer` + `es_module_visit`（SourceFile → `transform_es_module` 恒等返回）→ `import { x } from "m"; x;`（`module: esnext`）保留为 `import { x } from "m";\nx;` → 绿。证明 transformer 入口已接线 + value import/export 透传。
2. **`export_equals_is_elided_and_empty_imports_appended`（genuine RED）**：`export = x;`（esnext）期望 `export {};` → 红（恒等透传出 `export = x;`）→ 把 `transform_es_module` 改为逐顶层语句重建：`ExportAssignment` 且 `is_export_equals` 在非 preserve 下 elide（镜像 Go `visitExportAssignment`：`GetEmitModuleKind() != Preserve → return nil`）+ 守卫（`is_declaration_file || !(is_external_module || isolatedModules)` → 原样返回，镜像 `visitSourceFile`）+ `createEmptyImports`（原 external module 指示器存在、emit module kind 非 preserve、重建语句中无 `IsExternalModuleIndicator` → 追加 `export {};`，镜像 `visitSourceFile` 末尾 + `utilities.go:createEmptyImports`）→ 绿。
3. **`import_equals_require_is_elided_and_empty_imports_appended`（genuine RED）**：为守纪律先**回退**步骤 2 中提前写入的 `import =` 分支（避免"无红先实现"），加 `import x = require("m");`（esnext）期望 `export {};` → 红（透传出 `import x = require("m");`）→ 再加回 `ImportEqualsDeclaration` 且 `(emit_module_kind as i32) < Node16` 的 elide 臂（镜像 Go `visitImportEqualsDeclaration`：`GetEmitModuleKind() < Node16 → return nil`；Node16+ 同步 require 形态 DEFER）→ `createEmptyImports` 复用 → 绿。
4. **可达透传行为（spec/regression，恒等到达即绿）**：`export default 1;`、`const x = 1; export { x };`、`export * from "m";`、`export { x } from "m";`、`import * as ns from "m"; ns;` 均经 catch-all 臂原样保留（镜像 Go 的 `visitExportAssignment` 非 export= 分支、`visitExportDeclaration` 的 `Module > ES2015` preserve 分支、`visitImportDeclaration` 的 `RewriteRelativeImportExtensions` off 分支）；`non_module_file_is_passthrough`（`const x = 1;` 无指示器 → 守卫原样返回，无 spurious `export {};`）；`value_import_and_use_preserved_under_es2015`（tracer 在 es2015 同样成立）。

> 设计/divergence：(1) `ModuleKind` 未实现 `PartialOrd`，`< Node16` 比较走 `as i32` 显式转换（镜像 Go 整数序）。(2) 采用**顶层语句重建**（与 CommonJS 6e-3 一致）而非完整递归 `visit_each_child` 分发——可达 ESM 变换全部作用于模块顶层语句，import/export 不可嵌套，行为等价且回避 `visit_each_child` 回调只拿 `&mut NodeArena`（拿不到 `EmitContext`）的签名错配。(3) `createEmptyImports` 判据以**原始** `external_module_indicator.is_some()` 代替 Go 的 `IsExternalModule(result)`——Go 的 `UpdateSourceFile` 保留 `ExternalModuleIndicator`，而 Rust `new_source_file` 重置为 `None`，故用原始值，语义等价。(4) `--module preserve` 的 `export =`→`module.exports = e` 与 es2015 `export * as ns` 命名空间改写本轮 DEFER（前者超出 es2015/esnext scope，后者需 `NewGeneratedNameForNode`）。

**测试计数（6u 新增）**：`tsgo_transformers` +10 `#[test]`（tracer 1 + export= elision 1 + import= elision 1 + 透传/守卫 7）+1 doctest（`new_es_module_transformer`）→ **150 unit + 25 doctest**（6t 基线 140+24）。

### upstream（ast/printer/checker）增长（6u）

- **无**。全走 arena 既有构造器（`new_named_exports`/`new_export_declaration`/`new_source_file`）+ 6e `collect_external_module_info` 同源结构逻辑 + 6e-2 `TransformOptions.compiler_options`，未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，esmodule 维度）

- **type-only import elision**（`import type` / 未被 value 引用的类型导入）：可达子集无法判定哪些 import 被 value 使用。blocked-by：checker `EmitResolver`（同 6e-2 CommonJS 缺口）。
- **作用域正确引用重写**：blocked-by 真实 `ReferenceResolver`（checker `resolveName`/`EmitResolver`，当前 no-op 占位）。
- `--module preserve` 的 `export =`→`module.exports = e`、`--rewriteRelativeImportExtensions` 模块说明符重写、动态 `import()` 重写、Node16+ `import =`→同步 require helper、es2015 `export * as ns from "m"` 命名空间改写、external-helpers（tslib）import 注入。

## 6ab worklog（`esmodule` `export * as ns from "m"` 命名空间 re-export 改写；red→green 推进记录）

> 本轮收口 6u 的 DEFER 项「es2015 `export * as ns from "m"` 命名空间改写」（解锁前置 6p `new_generated_name_for_node` 已就绪）。Go ground truth：`moduletransforms/esmodule.go:visitExportDeclaration`——当 `ModuleSpecifier != nil`、`Module <= ES2015`（即 es2015；esnext/更高合法故透传）、且 `ExportClause` 是 `NamespaceExport` 时，改写为 `import * as <gen> from "m"` + re-export `<gen>`；`gen = NewGeneratedNameForNode(ns)`。re-export 形态依 `IsExportNamespaceAsDefaultDeclaration`（名为 `default`）分两支：`export default <gen>`（`NewExportAssignment`）vs `export { <gen> as ns }`（`NewExportSpecifier(propertyName=gen, name=oldIdentifier)`）。
>
> **Go-confirmed 行为（含 tsc 对拍）**：`tsc --module es2015`：`export * as ns from "m"` → `import * as ns_1 from "m";\nexport { ns_1 as ns };`；`export * as default from "m"` → `import * as default_1 from "m";\nexport default default_1;`。`tsc --module esnext`：两者均**原样透传**（合法语法）。故改写仅在 `Module <= ES2015` 触发，否则透传（与 6u 的 catch-all preserve 臂一致）。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing）。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_namespace_import`/`new_import_clause`/`new_import_declaration`/`new_export_specifier`/`new_named_exports`/`new_export_declaration`/`new_export_assignment`）+ 6p `ec.factory().new_generated_name_for_node`（emit 时经既有 printer name-generator 接线 materialize 成 `ns_1`/`default_1`）。

逐行为红→绿：

1. **`namespace_reexport_rewrites_to_import_and_named_export_under_es2015`（tracer，genuine RED）**：`export * as ns from "m";`（es2015）期望 `import * as ns_1 from "m";\nexport { ns_1 as ns };` → 红（6u catch-all 臂透传出 `export * as ns from "m";`）→ 加 `ExportDeclaration` 臂（`export_is_namespace_reexport` = `ModuleSpecifier.is_some() && ExportClause is NamespaceExport`）+ `rewrite_namespace_reexport`（`new_generated_name_for_node(ns)` → namespace import + named export，同一 synth NodeId 复用于 import 与 export specifier 的 propertyName，镜像 Go 单指针复用）→ 绿。**本步先不门控 module**（最小实现）。
2. **`namespace_reexport_is_preserved_under_esnext`（guard，genuine RED）**：`export * as ns from "m";`（esnext）期望原样透传 → 红（步骤 1 的未门控改写在 esnext 也改写成了 `import * as ns_1 …`）→ 加门控 `(options.module as i32) <= (ModuleKind::Es2015 as i32)`（镜像 Go `Module > ES2015 → preserve`，用**原始** `compilerOptions.Module` 而非 `GetEmitModuleKind`）→ 绿。
3. **`namespace_reexport_as_default_rewrites_to_export_default_under_es2015`（coverage，genuine RED）**：`export * as default from "m";`（es2015）期望 `import * as default_1 from "m";\nexport default default_1;` → 红（步骤 1 的 named-export 臂产出 `export { default_1 as default };`）→ 在 `rewrite_namespace_reexport` 加 `IsExportNamespaceAsDefaultDeclaration` 分支（`arena.text(old_identifier) == "default"` → `new_export_assignment(None, false, None, gen)`；镜像 Go `ModuleExportNameIsDefault` + `NewExportAssignment`）→ 绿。

> 设计/divergence：(1) 门控用 `options.module`（原始 `--module` 值）而非 `get_emit_module_kind()`——镜像 Go `visitExportDeclaration` 的 `tx.compilerOptions.Module > core.ModuleKindES2015`。(2) `rewriteModuleSpecifier` 在可达子集为恒等（`--rewriteRelativeImportExtensions` off），故直接复用原 `module_specifier`/`attributes` NodeId（与 6u 同源 DEFER）。(3) 合成节点不携带 Loc/Original（emit 不依赖），Go 的 `SetOriginal(importDecl, ExportClause)` / `SetOriginal(exportDecl, node)` sourcemap/comment 透传 DEFER（与既有轮一致）。(4) `default` 探测用 `arena.text` 直接比较——可达子集名为 identifier；string-literal module-export-name（`export * as "default"`）DEFER。

**测试计数（6ab 新增）**：`tsgo_transformers` +3 `#[test]`（es2015 named rewrite tracer 1 + esnext passthrough guard 1 + es2015 export-default coverage 1）→ **182 unit + 28 doctest**（6aa 基线 179+28，doctest 无新增）。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净（均实跑）。

### DEFER（本轮确认的 blocked-by，esmodule 维度，承接 6u）

- **type-only import elision**、**作用域正确引用重写**：blocked-by checker `EmitResolver` / 真实 `ReferenceResolver`（同 6u）。
- `--module preserve` 的 `export =`→`module.exports`、`--rewriteRelativeImportExtensions` 说明符重写、动态 `import()`、import assertions/attributes 透传细节、Node16+ `import =`→同步 require、external-helpers（tslib）注入；`export * as "default"`（string-literal export 名）。

## 6ac worklog（`impliedmodule` 按文件 module format 分派 CJS/ESM；red→green 推进记录）

> 本轮移植 `moduletransforms/impliedmodule.go:NewImpliedModuleTransformer` + `visitSourceFile` 分派：检查文件 emit module format，`format >= core.ModuleKindES2015` → 委托 `NewESModuleTransformer`，否则 → `NewCommonJSModuleTransformer`；`node.IsDeclarationFile` → 原样返回。Go 缓存 cjs/esm 子 transformer（lazy）；Rust 侧 eager 构建两者（无副作用：`new_*_transformer` 仅设标志 + 装闭包，不动 arena），行为等价。
>
> **Go-confirmed 分派谓词**：`format := tx.getEmitModuleFormatOfFile(node)`（= `GetEmitModuleFormatOfFileWorker` = `GetImpliedNodeFormatForEmitWorker` 非 None 则取之，否则 `options.GetEmitModuleKind()`）；`if format >= core.ModuleKindES2015 { esm } else { cjs }`。ModuleKind 判别值：`CommonJs=1 < Es2015=5 <= EsNext=99 <= Node16=100 <= NodeNext=199`，故 commonjs/amd/umd/system → CJS，es2015/esnext/node16+ → ESM。
>
> **可达子集 / DEFER**：per-file `impliedNodeFormat`（来自 `SourceFileMetaData` / package.json `type`）**未接线**（同 compiler P6-2 缺口），故 `GetImpliedNodeFormatForEmitWorker` 的 Node16/NodeNext 分支不可达；本轮分派纯用 `compiler_options.module`（即 `GetEmitModuleKind` 在可达子集的退化值）。DEFER：per-file `impliedNodeFormat`/`.cjs`/`.mjs` 探测（blocked-by `SourceFileMetaData` 未 thread 进 `TransformOptions`）、AMD/UMD/System 格式。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing）。**ZERO** ast/printer/checker 增长——消费既有 CJS/ESM 公开入口（`new_common_js_module_transformer`/`new_es_module_transformer`）+ `Transformer::run_visit`（crate 内既有 `pub(crate)`，复用同一 `EmitContext` 借用避免 RefCell 重借）。未改动任一公开链 API。

逐行为红→绿：

1. **`commonjs_source_delegates_to_common_js_transform`（tracer，genuine RED）**：`module: commonjs` 下 `export default 1;` 期望 = CJS transform 同输入输出（`Object.defineProperty(exports, "__esModule", …)` + `exports.default = 1;`）→ 红（visit 体为 `todo!()` panic）→ 最小实现：无条件委托 `cjs.run_visit` → 绿。
2. **`esnext_source_delegates_to_es_module_transform`（genuine RED）**：`module: esnext` 下 `export = x;` 期望 = ESM transform 输出 `export {};`（`export =` 在 ES 目标 elide + `createEmptyImports`）→ 红（无条件 CJS 臂：module≠commonjs 故 CJS 透传出 `export = x;`）→ 加最小二分 `if format == ModuleKind::EsNext { esm } else { cjs }` → 绿。
3. **`es2015_source_routes_to_es_module_transform`（谓词边界，genuine RED）**：`module: es2015` 下 `export = x;` 期望 ESM 输出 `export {};`→ 红（步骤 2 的 `== EsNext` 把 es2015 误路由到 CJS 透传 `export = x;`）→ 提取谓词 `is_es_module_format(format) = (format as i32) >= (ModuleKind::Es2015 as i32)`（镜像 Go `format >= core.ModuleKindES2015`）替换 `== EsNext` → 绿。
4. **`declaration_file_is_returned_unchanged`（守卫，genuine RED）**：`.d.ts` 内 `export = x;`（module: commonjs）期望原样 `export = x;`→ 红（无守卫时委托 CJS 把 `export =` 降级成 `module.exports = x;`）→ 在分派前加 `IsDeclarationFile` 守卫（`NodeData::SourceFile.is_declaration_file` → 直接 `return node`；同时非 SourceFile 节点透传）→ 绿。

> 设计/divergence：(1) 分派谓词消费 `compiler_options.module`（而非 per-file `getEmitModuleFormatOfFile`）——per-file 路径 DEFER（blocked-by `SourceFileMetaData`）。(2) eager 构建 cjs/esm 子 transformer 取代 Go 的 lazy 缓存——无 arena 副作用，输出等价；规避 borrow-checker（闭包内 `&mut EmitContext` 已借用，`run_visit` 直接吃 `&mut EmitContext` 不重借 RefCell）。(3) `is_es_module_format` 设为 `pub fn`（带 doctest），便于谓词独立验证。

**测试计数（6ac 新增）**：`tsgo_transformers` +4 `#[test]`（commonjs→CJS tracer + esnext→ESM + es2015 谓词边界 + 声明文件守卫）+ 2 doctest（`new_implied_module_transformer` + `is_es_module_format`）→ **186 unit + 30 doctest**（6ab 基线 182+28）。`cargo test -p tsgo_transformers` 全绿、`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净（均实跑）。

### DEFER（本轮确认的 blocked-by，impliedmodule 维度）

- **per-file `impliedNodeFormat`/`.cjs`/`.mjs` 探测**：blocked-by `SourceFileMetaData`（package.json `type` + 文件扩展名）未 thread 进 `TransformOptions`（同 compiler P6-2 `getEmitModuleFormatOfFile` 缺口）。当前分派退化为按 `compiler_options.module` 全局判定。
- **AMD/UMD/System 格式**：Go `impliedmodule` 仅二分 CJS/ESM。System transform 本身的 register-wrapper 核心已在 6ae 独立落地（`systemmodule.rs`，`module: system` 直接入口），但 impliedmodule 分派器尚未把 `ModuleKind::System` 接到它（仍二分）；AMD/UMD transform 本身尚未移植。

## 6ad worklog（commonjsmodule 动态 `import()` 降级；red→green 推进记录）

> 本轮在 6e/6v/6w/6x CommonJS 面上长出 **动态 `import()` 降级**：`module: commonjs` 下把 `import(expr)` 降为 `Promise.resolve().then(() => __importStar(require(expr)))`。Go ground truth：`commonjsmodule.go:visitCallExpression`（`IsImportCall && shouldTransformImportCall` → `visitImportCallExpression`）→ `createImportCallExpressionCommonJS`。
>
> **Go 形态确认（关键，纠正任务简报）**：`createImportCallExpressionCommonJS`（commonjsmodule.go:1864）**无条件**把 `require(...)` 包进 `NewImportStarHelper`（= `__importStar(...)`），**与 `esModuleInterop` 无关**。已对 raw GitHub `microsoft/typescript-go/main` 同文件核对（行 1908 `requireCall := tx.Factory().NewImportStarHelper(...)` 无 `if getESModuleInterop`）——故**不存在简报里说的「无 interop 基础形 `Promise.resolve().then(() => require("m"))`」**；唯一形态即 importStar-wrapped。回调用 **arrow**（`NewArrowFunction`，`() => ...`，非 function expr）。参数 `arg` 为 `isSimpleInlineableExpression`（`!IsIdentifier && (StringLiteralLike|NumericLiteral|KeywordKind)`）时 `needSyncEval=false`：直接内联进 `require(arg)`、`Promise.resolve()` 无参、arrow 无形参；否则 `needSyncEval=true` 走 `Promise.resolve(\`${x}\`).then((s) => … require(s))` 模板形（本轮 DEFER）。
>
> **解析器缺口（blocked-by，决定测试形态）**：`internal/parser/lib.rs:parse_left_hand_side_expression_or_higher` 仍 **DEFER(phase-3) 动态 `import(...)` call head**（仅处理 `import.meta`）；实测 `parse_shared("const p = import(\"m\");")` **解析器死循环**（`import` 关键字落 `parse_primary_expression` 的 `_` 臂不消费 token）。`internal/parser` **不在本轮编辑边界**（仅 `internal/transformers/**` + 本两文档）。故 transformer 降级逻辑用**合成 AST**（直接 arena 构造 `CallExpression{ expression: ImportKeyword, arguments: [...] }` —— 即解析器**本应**产出的结构）经公开入口 `new_common_js_module_transformer(...).transform_source_file(...)` 行为级验证。端到端 parse→transform 路径 blocked-by 解析器动态 import call-head（phase-3）。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing）。复用 6e-3 interop helper infra（`request_emit_helper(IMPORT_STAR_HELPER)`/`wrap_in_helper("__importStar", …)`）+ 既有 emit-substitution（`set_node_substitution`，对标 Go `onSubstituteNode`，在打印期把 import-call 节点替换为降级表达式）。**ZERO** ast/printer/checker 增长——全走既有 arena 构造器（`new_keyword_expression`/`new_call_expression`/`new_property_access_expression`/`new_arrow_function`/`new_token`/`new_identifier`）。未改动任一公开链 API。

逐行为红→绿：

1. **`dynamic_import_lowers_to_promise_resolve_then_import_star_require`（tracer，genuine RED）**：合成 `const p = import("m");`（module: commonjs）→ 红（无降级，恒等保留 `const p = import("m");`，实测）→ 最小实现：在 kept-statement 循环加 `register_dynamic_import_substitutions`（递归 `collect_import_calls` 找 `is_import_call` = CallExpression 且 `expression.kind == ImportKeyword`），命中且首参为 `is_simple_inlineable_expression`（本轮：StringLiteral/NoSubstitutionTemplate/Numeric/BigInt/true/false/null 字面量子集）时经 `build_downleveled_import(vec![arg])` 建 `Promise.resolve().then(() => __importStar(require("m")))` + `set_node_substitution(call, lowered)` → 绿（prologue `var __importStar = …;`）。`let arg = arg?;` 守卫令无参 import() 暂 defer（恒等）以保 slice 2 真红。
2. **`no_argument_dynamic_import_lowers_to_require_with_no_args`（genuine RED）**：合成 `const p = import();` → 红（slice 1 的 `arg?` 守卫令无参恒等保留 `const p = import();`，实测）→ 把 `lower_dynamic_import_call` 的 `arg?` 改为 `match arg { None => Vec::new(), Some(arg) => { 非 inlineable → return None; vec![arg] } }`，无参时 `require()` 空参表（仍 importStar 包裹，对标 Go `requireArguments` nil 分支）→ `const p = Promise.resolve().then(() => __importStar(require()));` → 绿。

> 设计/divergence：(1) **importStar 无条件包裹**忠实 Go（非 esModuleInterop 门控）——故无「base form first then interop variant」两片，唯一形态即包裹形；简报「base form (no interop)」与 Go 背离，以 Go ground truth 为准。(2) **arrow** 回调（非 function expr），对标 Go `NewArrowFunction`。(3) 降级经 `set_node_substitution`（打印期替换）而非重建语句树——契合既有 use-site 重写架构，import-call 节点在 kept 语句子树内被就地替换。(4) **合成 AST** 测试（非 `parse_shared`）——解析器 DEFER 动态 import call-head（死循环），blocked-by phase-3 解析器；合成节点 = 解析器本应产出的结构，行为级走公开 transformer 入口。(5) `is_simple_inlineable_expression` 取 Go 谓词的字面量子集（KeywordKind 仅列 true/false/null）——本轮可达参数为 string literal，足够；完整 KeywordKind 集 DEFER。

**测试计数（6ad 新增）**：`tsgo_transformers` +2 `#[test]`（string-literal arg tracer 1 + no-arg 边界 1），本轮无新 doctest（`new_common_js_module_transformer` 已有 doctest）→ **188 unit + 30 doctest**（6ac 基线 186+30）。`cargo test -p tsgo_transformers` 全绿、`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净（均实跑）。

### upstream（ast/printer/checker）增长（6ad）

- **无**。动态 import 降级全走 arena 既有构造器 + 6e-3 helper infra（`request_emit_helper(IMPORT_STAR_HELPER)`/`wrap_in_helper`，`IMPORT_STAR_HELPER` 早在 6d-2 定义）+ 既有 `set_node_substitution`/`for_each_child`。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、`internal/parser/*`。

### DEFER（本轮确认的 blocked-by，动态 import 维度）

- **解析器动态 `import(...)` call head**（端到端 parse→transform）：blocked-by `internal/parser/lib.rs:parse_left_hand_side_expression_or_higher` 的 `DEFER(phase-3)`（实测解析器对 `import("m")` 死循环）。本轮 transformer 降级用合成 AST 验证。
- **`needSyncEval` 模板形**（非 inlineable 参数，如 `import(someVar)` / 拼接表达式 → `Promise.resolve(\`${x}\`).then((s) => __importStar(require(s)))`）：本轮对非 inlineable 参数 `return None`（恒等保留），blocked-by `NewTemplateExpression`/`NewTemplateHead`/`NewTemplateSpan`/`NewTemplateTail` + 参数化 arrow（`(s) =>`）构造的对标移植。
- **spread 参数 `import(...args)`**、**top-level-await import**、**`import.meta`**：同既有 DEFER。
- **`shouldTransformImportCall` 的 module-kind 门控**（Node16+/preserve 不降级、`ModuleKindNone && languageVersion>=ES2020` 透传）：本轮恒走降级（`module: commonjs` 已是降级前提），完整门控 DEFER。

## 6v worklog（commonjsmodule 深化 — combined default+named import + `export … from "m"` re-export，red→green 推进记录）

> 本轮在 6e-3 CommonJS 面上长出两个可达子集（name-matched，无真实 ReferenceResolver，同 6e-2/6e-3 缺口）：**combined default+named import**（`import d, { x } from "m"`）与 **named re-export**（`export { x } from "m"` / `export { a as b } from "m"`）。Go ground truth：`commonjsmodule.go:visitTopLevelImportDeclaration`（combined 落 `else` 单 `const m_1 = …` 分支 + `getHelperExpressionForImport`）+ `getImportNeedsImportStarHelper`（externalmoduleinfo.go）+ `visitTopLevelExportDeclaration`（NamedExports 分支：`var m_1 = require("m")` + 逐 specifier `createExportExpression(liveBinding=true)`）。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing）。复用 6e `collect_external_module_info`（识别 external import/export）、6e-2 emit-substitution（import use-site `x`→`m_1.x` 重写）、6e-3 interop helper builder（`request_emit_helper`/`wrap_in_helper`/`new_unscoped_helper_name`）。**ZERO** ast/printer/checker 增长。

**Go 形态确认（esModuleInterop on/off）**：用 `tsc --module commonjs`（开/关 esModuleInterop）对拍：
- combined `import d, { x } from "m"; d; x;` → 两种 interop 模式**输出相同**：`const m_1 = __importStar(require("m"));\nm_1.default;\nm_1.x;`。与 Go 一致——Go `getHelperExpressionForImport`（commonjsmodule.go:700）**不**门控 esModuleInterop，`getImportNeedsImportStarHelper` 对「默认 import 混非默认命名 ref」返回 true，故走 `__importStar`（**非**任务提示里写的 `__importDefault`——以 Go ground truth 为准）。本端口既有 default-only/namespace import 也已无条件发 helper，行为一致。
- re-export `export { x } from "m";` → `var m_1 = require("m");\nObject.defineProperty(exports, "x", { enumerable: true, get: function () { return m_1.x; } });`（Go `createExportExpression` liveBinding=true 的 getter 形态，**非**任务提示里的 `exports.x = m_1.x`——以 Go ground truth 为准；`var` 而非 `const`，对标 Go `NodeFlagsNone`）。

先确认基线绿（`cargo test -p tsgo_transformers` = 150 unit + 25 doctest）。逐行为红→绿：

1. **`combined_default_and_named_import_uses_import_star_helper`（tracer，genuine RED）**：`import d, { x } from "m"; d; x;` → 红（旧 `NamedImports` 臂 `if default_name.is_some() { return None; }` 守卫令 combined import 整句恒等保留：`import d, { x } from "m";\nd;\nx;`）→ 移除该守卫，在 `NamedImports` 臂改为：先读 named elements；若 `default_name` 存在则 push 默认绑定（`d`→`m_1.default`，member=`"default"`）并把 require 经 `wrap_in_helper("__importStar", …)` + `request_emit_helper(IMPORT_STAR_HELPER)`（Go `getImportNeedsImportStarHelper` 的可达子集：默认 import + 非默认命名 ref），否则裸 require → `const m_1 = __importStar(require("m"));\nm_1.default;\nm_1.x;`（prologue `var __importStar = …;`）→ 绿。use-site `d`/`x` 经既有 6e-2 substitution 重写。
2. **`re_export_named_binding_lowers_to_require_and_live_binding_getter`（genuine RED）**：`export { x } from "m";` → 红（`lower_export_declaration` 对带 module specifier 的 NamedExports `return None` → 整句恒等保留：`Object.defineProperty(exports, "__esModule", { value: true });\nexport { x } from "m";`）→ 把 module-specifier 分支重构为：非 string-literal specifier → None；无 export clause → `export *`（既有 `make_export_star`）；NamedExports → `var m_1 = require("m");`（新 `build_var_binding`，无 CONST flag）+ 逐 specifier `make_live_binding_export(export_name, m_1.<member>)`（新 helper，建 `Object.defineProperty(exports, "<name>", { enumerable: true, get: function () { return <value>; } });`，对标 Go `createExportExpression` liveBinding 分支）→ `…__esModule…;\nvar m_1 = require("m");\nObject.defineProperty(exports, "x", { enumerable: true, get: function () { return m_1.x; } });` → 绿。`build_const_binding`/`build_var_binding` 抽共享 `build_binding(_, _, _, is_const)`。
3. **`re_export_renamed_binding_uses_property_name_for_value`（coverage，直接绿/泛化）**：`export { a as b } from "m";` → export 名取 specifier `name`（`b`）、getter 值的 member 取 `property_name.unwrap_or(name)`（`a`）→ `Object.defineProperty(exports, "b", { enumerable: true, get: function () { return m_1.a; } });` → slice-2 实现下直接绿（锁定 rename 路径，对标 Go `specifier.PropertyNameOrName()` / `GetExportName(specifier)`）。

> 设计/divergence：(1) combined import 的 `__importStar` 选择取 Go `getImportNeedsImportStarHelper` 的**可达子集**（默认 import + 至少一个非默认命名 ref → importStar）；`import d, { default as y }`（仅默认 ref → Go 走 `__importDefault`）的 ref 计数边界 DEFER。(2) re-export require var 名沿用确定性 `<module>_1`（Go 用 `NewGeneratedNameForNode`）——多个 `export {} from "m"` 会撞名（`m_1`），同既有 import 的确定性命名 DEFER（blocked-by 碰撞无关的 `NewGeneratedNameForNode`）。(3) re-export 用 **live-binding** getter（Go liveBinding=true），而 *local* `export { x }`（无 module specifier）仍用既有 `exports.x = x` 简单赋值（Go 该路径 liveBinding=false 子集）——两路径忠实对应 Go 的 liveBinding 取值。(4) 未建 `exports.x = void 0` 导出名初始化、`"use strict"` prologue（沿用 6e-3 既有简化）。(5) `export * as ns from "m"`（NamespaceExport）、string-literal export 名 DEFER。

**测试计数（6v 新增）**：`tsgo_transformers` +3 `#[test]`（combined import 1 + re-export 1 + re-export rename coverage 1），本轮无新 doctest（`new_common_js_module_transformer` 已有 doctest）→ **153 unit + 25 doctest**（6u 基线 150+25）。

### upstream（ast/printer/checker）增长（6v）

- **无**。combined import + re-export 全走 arena 既有构造器（`new_variable_statement`/`new_variable_declaration(_list)`/`new_call_expression`/`new_property_access_expression`/`new_object_literal_expression`/`new_property_assignment`/`new_function_expression`/`new_block`/`new_return_statement`/`new_keyword_expression`/`new_string_literal`/`new_identifier`/`add_flags`）+ 6e `collect_external_module_info` + 6e-2 substitution/compiler_options + 6e-3 helper infra（`request_emit_helper`/`wrap_in_helper`/`new_unscoped_helper_name`，`IMPORT_STAR_HELPER` 早在 6d-2 定义）。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，commonjsmodule 维度）

- **作用域正确的 use 解析**（import use-site `d`/`x` 当前按名匹配；shadowing 局部同名会被误重写）：blocked-by 真实 `ReferenceResolver`（checker `resolveName`/`EmitResolver`，占位 no-op）。同 6e-2/6e-3 缺口。
- **碰撞无关的 require var 名**（确定性 `<module>_1`，多 import/re-export 同模块会撞）：blocked-by `NewGeneratedNameForNode`。
- **`default as`-only 命名 import**（Go 走 `__importDefault` 而非 `__importStar`）、**`export * as ns from "m"`**（NamespaceExport）、**string-literal export/import 名**、**`export =`**、**动态 `import()`**、**`import =`**、**`exports.x = void 0` 导出名初始化 + `"use strict"`**、**local `export { x }` 的 live-binding 形态**（当前用简单赋值）。

## 6w worklog（commonjsmodule `export =` + 导出函数/类声明降级；red→green 推进记录）

> 本轮在 6e/6e-3/6v CommonJS 面上长出三个可达子集（name-matched，无真实 ReferenceResolver，同 6e-2/6e-3 缺口）：**`export =`**（`export = e` → `module.exports = e;`，并**抑制** `__esModule` 标记）、**导出函数声明**（`export function f() {}` → 保留本地 `function f() {}` + `exports.f = f;`）、**导出类声明**（`export class C {}` → 保留本地 `class C {}` + `exports.C = C;`）。Go ground truth：`commonjsmodule.go:appendExportEqualsIfNeeded`/`visitExportEquals`（`module.exports = <visited expr>`）、`visitTopLevelFunctionDeclaration` + `appendExportsOfClassOrFunctionDeclaration`（剥 export/default 修饰、`exports.<name> = <localName>`；函数声明的导出赋值经 `exportedFunctions` 循环置于**自定义 prologue**、即声明之前，因函数声明 hoist）、`visitTopLevelClassDeclaration`（返回 `[class C {}, exports.C = C]`，类不 hoist 故赋值在声明**之后**就地）。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing）。复用 6e `collect_external_module_info`（已识别 `export_equals`）、6e-3 export-lowering 机器（`make_exports_assignment`）、`modifiervisitor::extract_modifiers`（剥 `EXPORT_DEFAULT` 修饰）。**ZERO** ast/printer/checker 增长。

**Go 形态确认（tsc --module commonjs）**：
- `export = x;` → `"use strict";\nmodule.exports = x;`（本端口沿用 6e-3 简化**不**发 `"use strict"`，且 `__esModule` 标记被抑制——`module_has_exports` 对 `is_export_equals` 的 `ExportAssignment` 返回 false，对标 Go：`export =` 是整模块 CommonJS 导出，不打 `__esModule`）→ 端口形态 `module.exports = x;`。
- `export function f() {}` → `"use strict";\nObject.defineProperty(exports, "__esModule", { value: true });\nexports.f = f;\nfunction f() { }`（赋值在声明**之前**，因函数 hoist）。
- `export class C {}` → `…__esModule…;\nclass C {\n}\nexports.C = C;`（赋值在声明**之后**，因类不 hoist）。
- `export default function f() {}` → `…__esModule…;\nexports.default = f;\nfunction f() { }`；`export default class C {}` → `…;\nclass C {\n}\nexports.default = C;`（导出名取 `default`）。

先确认基线绿（`cargo test -p tsgo_transformers` = 153 unit + 25 doctest）。逐行为红→绿：

1. **`export_equals_becomes_module_exports_without_marker`（tracer，genuine RED）**：`export = x;` → 红（`lower_export_default` 对 `is_export_equals` `return None` → 整句恒等保留 `export = x;`）→ 把 `lower_export_default` 的 `export =` 臂改为返回新 `make_module_exports_assignment(ec, expression)`（建 `module.exports = <expr>;`：`module.exports` property-access 作左值、`=` token、`expression` 作右值的 `ExpressionStatement`）→ `module.exports = x;`（无 `__esModule`，因 `module_has_exports` 已对 `is_export_equals` 返回 false）→ 绿。
2. **`exported_function_declaration_keeps_decl_and_assigns_export`（genuine RED）**：`export function f() {}` → 红（catch-all 臂恒等保留 `export function f() { }`，无标记）→ (a) `module_has_exports` 增 `FunctionDeclaration`/`ClassDeclaration` 带 EXPORT 修饰的臂（触发标记）；(b) `transform_common_js_module` 重构语句循环为 `body` + `hoisted_function_exports` 两段（marker 移到循环后，`out = [marker?] + hoisted_function_exports + body`）；(c) 新增 `FunctionDeclaration` + `declaration_has_export_modifier` 守卫臂，调 `lower_exported_function_declaration`（剥 `EXPORT_DEFAULT` 修饰重建 `function f() {}`、`exports.f = f;` 入 hoisted）→ `Object.defineProperty(exports, "__esModule", { value: true });\nexports.f = f;\nfunction f() { }` → 绿。
3. **`exported_class_declaration_keeps_decl_and_assigns_export`（genuine RED）**：`export class C {}` → 红（恒等保留 `export class C {\n}`，但标记已因 slice 2 的 `module_has_exports` 出现）→ 增 `ClassDeclaration` + 守卫臂，调 `lower_exported_class_declaration`（剥修饰经 `new_class_like` 重建 `class C {}`，`decl` 与 `exports.C = C;` 顺序入 `body`——类不 hoist）→ `…__esModule…;\nclass C {\n}\nexports.C = C;` → 绿。
4. **`exported_default_function_declaration_assigns_default_export` / `exported_default_class_declaration_assigns_default_export`（coverage，直接绿/泛化）**：`export default function f() {}` / `export default class C {}` → slice 2/3 的 `is_default`（`modifier_flags.contains(DEFAULT)`）分支已泛化导出名取 `default`，直接绿，锁定 named-default 路径（对标 Go `appendExportsOfClassOrFunctionDeclaration` 的 `HasSyntacticModifier(decl, ModifierFlagsDefault) → "default"`）。

> 设计/divergence：(1) **函数导出赋值 hoisting**：Go 把所有导出函数的 `exports.f = f;` 经 `exportedFunctions` 循环置于自定义 prologue（`EFCustomPrologue`），位于**整个 body 之前**（甚至 external-helpers import 之前）；本端口收集进 `hoisted_function_exports` 并置于 marker 之后、`body` 之前——对**单导出函数模块**（本轮 tracer）行为一致，但若模块在导出函数前还有其它顶层语句（如 `const a = 1; export function f(){}`），Go 会把 `exports.f = f;` 提到 `const a = 1` 之上而本端口提到 marker 之后、`a` 之前同样在前——**仍一致**（因 hoisted 段整体在 body 之前）；仅当存在 require-helper import 注入时排序细节 DEFER。(2) **类不 hoist**：`exports.C = C;` 就地跟在 `class C {}` 之后（Go `visitTopLevelClassDeclaration` 的 `[decl, export]` 顺序）。(3) 修饰剥离用 `extract_modifiers(_, _, !EXPORT_DEFAULT)`（保留 `async` 等其它修饰，对标 Go `ExtractModifiers(_, _, ^ModifierFlagsExportDefault)`）。(4) **匿名** `export default function () {}` / `export default class {}`（`name == None`）返回 `None` → 落 catch-all 恒等，DEFER（需 `NewGeneratedNameForNode` 合成名）。(5) 沿用确定性命名 / 不发 `"use strict"` / `exports.x = void 0` 初始化等 6e-3 既有简化；`export =` 与 `import =` 互斥、`export =` 抑制其它导出的全局副作用未建模（本轮 tracer 仅单 `export =`）。

**测试计数（6w 新增）**：`tsgo_transformers` +5 `#[test]`（`export =` 1 + 导出函数 1 + 导出类 1 + default 函数/类 coverage 2），本轮无新 doctest（`new_common_js_module_transformer` 已有 doctest）→ **158 unit + 25 doctest**（6v 基线 153+25）。

### upstream（ast/printer/checker）增长（6w）

- **无**。`export =`/导出函数/类全走 arena 既有构造器（`new_property_access_expression`/`new_binary_expression`/`new_token`/`new_expression_statement`/`new_function_declaration`/`new_class_like`/`new_identifier`）+ 6e `collect_external_module_info`（已含 `export_equals`）+ `modifiervisitor::extract_modifiers`（既有 crate 级 re-export）。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，commonjsmodule 维度）

- **作用域正确的 use 解析**（同 6e-2/6e-3/6v 缺口）：blocked-by 真实 `ReferenceResolver`（checker `resolveName`/`EmitResolver`，占位 no-op）。
- **碰撞无关的 require var 名**：blocked-by `NewGeneratedNameForNode`。
- **匿名 `export default function () {}` / `export default class {}`**（需合成名）、**`export =` 与 `import =` 互斥/`export =` 抑制其它导出**、**导出函数赋值与 external-helpers import 注入的精确 prologue 排序**、**`"use strict"` prologue + `exports.x = void 0` 导出名初始化**（→ 6x 落地）、**动态 `import()`**、**`import =`**（→ 6x 落地非导出 require 形态）、**`export * as ns from "m"`**、**string-literal export/import 名**、**local `export { x }` 的 live-binding 形态**（当前用简单赋值）。

## 6x worklog（commonjsmodule `import =`(require) + `exports.x = void 0` 导出名初始化；estransforms `usestrict` prologue 移植；red→green 推进记录）

> 本轮推进 6w-推荐的三块 CommonJS-surface（全结构性、resolver-free）：(1) **`import x = require("m")`** → `const x = require("m");`（emit module kind < Node16）；(2) **`exports.<name> = void 0;` 导出名初始化**（marker 之后、body 之前的零初始化）；(3) **`"use strict"` prologue**。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing；编辑边界 `internal/transformers/**`）。**ZERO** ast/printer/checker 增长。

### Go ground-truth 校正（briefing 偏离，已 VERIFY 跟随 Go + tsc）

> briefing 称"`"use strict"` prologue 由 commonjs transform 插入"。**实际 Go ground truth：`"use strict"` 不在 commonjsmodule.go——它是一个独立 transformer `estransforms/usestrict.go`（`NewUseStrictTransformer`）**，在 emit 管线里位于 ES down-leveler 之后、module transformer 之前（`emitter.go:161`）。`commonjsmodule.go:transformCommonJSModule` 仅用 `SplitStandardPrologue` **保留**源里已有的 prologue，从不**新增** `"use strict"`。条件（`usestrict.go:visitSourceFile`）：JSON 跳过；外部模块且按 ESM 输出（`moduleKind >= ES2015 && (moduleKind == Preserve || format >= ES2015)`）跳过；否则 `Factory().EnsureUseStrict(statements)` 前置 `"use strict";`（已存在则去重）。→ 故本轮把 `"use strict"` 作为**独立 transformer 移植到 `estransforms/usestrict.rs`**（仍在 `internal/transformers/**` 边界内），**不**塞进 commonjs transform。
>
> 其它 tsc-verified 形态（`tsc --module commonjs --target es2017`，已实测）：
> - `import x = require("./m"); x;` → `const x = require("./m");\nx;`（`x` 是真正的 `const`，**use 不重写**；未用的 import= 被 import-elision 擦除——本端口无 elision 故恒降级；import-only 模块下本端口沿用既有"仅 value 导出才发 `__esModule`"简化故无 marker，与既有 `named_import_and_use` 测试一致）。
> - `export const a = 1; export const b = 2;` → `exports.b = exports.a = void 0;`（**链式、逆序**——名按源序 `[a,b]` fold，最后一个名在最外层；Go `transformCommonJSModule` 的 50-名分块循环：`right = void 0`，`for name: right = (exports.<name> = right)`）。
> - `export class C {}` → **有** `exports.C = void 0;`（非默认类名进 `exportedNames`）；`export function f() {}` → **无** void-0（函数走 `exportedFunctions`，不进 `exportedNames`）；`export default 1/function/class`、`export *`、`export =` → **无** void-0。

先确认基线绿（`cargo test -p tsgo_transformers` = 158 unit + 25 doctest）。逐行为红→绿：

1. **`import_equals_require_lowers_to_const_require`（tracer，genuine RED）**：`import x = require("m"); x;` → 红（无 `ImportEqualsDeclaration` 臂 → catch-all 恒等保留 `import x = require("m");`）→ `transform_common_js_module` 增 `Kind::ImportEqualsDeclaration` 臂调 `lower_import_equals_to_require`（取 `ImportEqualsData{name, module_reference}`；仅 `ExternalModuleReference` 的 string-literal 形态降级；非导出；建 `const <name> = require("<m>");`，复用 `build_require_call`+`build_const_binding`；**不**入 `bindings` 故 use 保留）→ `const x = require("m");\nx;` → 绿。
2. **`export_const_becomes_exports_assignment`（更新既有 → genuine RED）**：把期望改为 tsc-correct（含 `exports.y = void 0;`）→ 红（无 void-0 init）→ `transform_common_js_module` 在 marker 后、`hoisted_function_exports` 前 `out.extend(make_exports_void_zero_inits(ec, &exported_name_texts))`（先 snapshot `info.exported_names` 文本避免跨 `arena_mut` 借用；最小实现：每名一句）→ `…__esModule…;\nexports.y = void 0;\nexports.y = 1;` → 绿；同步更新 `local_named_export`/`re_export_named`/`re_export_renamed`（单名，直接绿）。
3. **`multiple_exported_names_share_chained_void_zero_init`（genuine RED）**：`export const a=1; export const b=2;` → 红（每名独立句 `exports.a = void 0;\nexports.b = void 0;`）→ 把 `make_exports_void_zero_inits` 重构为 Go 的 50-名分块链式 fold（`right=void 0`，每名 `right = new_binary(exports.<name>, =, right)`，整块一句）→ `exports.b = exports.a = void 0;` → 绿。
4. **`exported_class_declaration_keeps_decl_and_assigns_export`（更新既有 → genuine RED）**：期望加 `exports.C = void 0;` → 红（`collect_external_module_info` 未收集类名）→ `externalmoduleinfo.rs` 增 `Kind::ClassDeclaration` 臂：非默认且有名的 `export class C {}` → `add_unique_exported_name(C)`（**排除** `default` 与函数声明，对标 Go `collectExternalModuleInfo` 的 ClassDeclaration 分支只对非默认调 `addExportedName`）→ `…;\nexports.C = void 0;\nclass C {\n}\nexports.C = C;` → 绿；`exported_function_declaration`（无 void-0）与 `exported_default_*`（默认排除）保持绿。
5. **`commonjs_module_gains_use_strict_prologue`（tracer，genuine RED）**：新建 `estransforms/usestrict.rs`（先 passthrough stub + 接入 `estransforms/mod.rs`），测 `export const y = 1;`（module=CommonJs）经 `new_use_strict_transformer` → 红（stub 恒等无 `"use strict"`）→ 还原真实逻辑（JSON 跳过；`is_external && emit_module_kind >= ES2015` 跳过 [format 门控 DEFER]；否则 `ensure_use_strict` 前置 `"use strict";`，首句已是 use-strict prologue 则去重）→ `"use strict";\nexport const y = 1;` → 绿。
6. **`existing_use_strict_prologue_is_not_duplicated` / `esm_external_module_skips_use_strict`（companion coverage，直接绿）**：`"use strict"; var x = 1;`（CommonJs）→ 不重复；`export const y = 1;`（EsNext，外部模块按 ESM 输出）→ 跳过、无 `"use strict"`——锁定 `EnsureUseStrict` 去重分支与 ESM-skip 分支。

> 设计/divergence：(1) **`"use strict"` 是独立 transformer**（见上 ground-truth 校正），不在 commonjs transform。(2) **void-0 链式逆序**精确对标 Go fold（最后名最外层），含 50-名分块（>50 导出走多句，无测试覆盖但忠实移植）。(3) void-0 仅处理 identifier 导出名（property-access 形态）；string-literal 导出名（element-access）DEFER。(4) **`import =` 仅非导出 + `= require("m")` 形态**：`export import x = require("m")`（Go → `exports.x = require(...)`）与内部模块引用（`import x = a.b`，Go panic「应在更早 transformer 处理」）返回 `None` 落 catch-all 恒等保留，DEFER。(5) **use-strict 的 `format` 门控**（`getEmitModuleFormatOfFile`，逐文件/`package.json`-`type`/resolver 相关）未入 `TransformOptions`，本端口以 emit module kind 近似 `format >= ES2015`——对非 Node ESM kind（`ES2015..=ESNext`/`Preserve`）精确，对 `Node16+`（CJS format 仍应发 `"use strict"`）不精确，DEFER。(6) import-only 模块仍沿用既有"仅 value 导出才发 `__esModule`"简化（Go 对任意外部模块都发）。

**测试计数（6x 新增）**：`tsgo_transformers` +5 `#[test]`（import= 1 + 多名 void-0 链 1 + use-strict tracer/dedup/ESM-skip 3）+1 doctest（`new_use_strict_transformer`），其余 4 处为既有测试更新（单名 void-0 × 3 + 导出类 void-0 × 1）→ **163 unit + 26 doctest**（6w 基线 158+25）。

### upstream（ast/printer/checker）增长（6x）

- **无**。三块全走 arena 既有构造器（`new_void_expression`/`new_numeric_literal`/`new_binary_expression`/`new_property_access_expression`/`new_token`/`new_expression_statement`/`new_string_literal`/`new_source_file`/`new_identifier`）+ 6e `collect_external_module_info`（本轮在 `internal/transformers/moduletransforms/externalmoduleinfo.rs` 内增非默认 `export class` 名收集，未触碰其它 crate）。新增 `internal/transformers/estransforms/usestrict.rs`（+ `usestrict_test.rs`，接入 `estransforms/mod.rs`）。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、`internal/compiler/*`、任何 `.go`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by）

- **作用域正确的 use 解析**（同 6e-2..6w 缺口）：blocked-by 真实 `ReferenceResolver`。
- **Node16+ `import = require()`**（同步 `require` helper 形态）：blocked-by emit module kind >= Node16 的 require-helper 注入（本轮仅 < Node16 的 `const x = require(...)`）。
- **`export import x = require("m")`**（→ `exports.x = require(...)`）、**内部模块 `import x = a.b`**、**string-literal 导出名 void-0（element-access）**、**use-strict 的逐文件 `format` 门控（`getEmitModuleFormatOfFile`）**、**import-only 模块的 `__esModule` marker**（对标 Go 任意外部模块）。

## 6x worklog（commonjsmodule `import =` + `exports.<name> = void 0` 导出名初始化 + 独立 `usestrict` transformer；red→green 推进记录）

> 本轮在 6e/6e-3/6v/6w CommonJS 面上长出 6w 推荐的三块 CommonJS-surface（结构性、resolver-free）：**`import =`**（`import x = require("m")` → `const x = require("m");`）、**`exports.<name> = void 0` 导出名初始化**（在 `__esModule` 标记之后为每个导出名零初始化）、以及 **`"use strict"` prologue**。三块都先用 tsc（`5.7.3 --module commonjs`）对拍确认形态，再 land。
>
> **Go ground-truth 修正（briefing 偏离）**：briefing 称 `"use strict"` 由 CommonJS transform 插入——**Go 并非如此**。`"use strict"` 由**独立 transformer** `estransforms/usestrict.go`（`NewUseStrictTransformer`）负责，在 emit 管线里排在 ES down-leveler 之后、module transformer 之前（`emitter.go:161`）。其条件：JSON 跳过；外部模块若以 ESM 形态 emit（`moduleKind >= ES2015 && (moduleKind == Preserve || format >= ES2015)`）则跳过（ESM 恒 strict）；否则 `Factory().EnsureUseStrict` 前插 `"use strict";`（已存在则不重复）。故本轮把它移植为 `estransforms/usestrict.rs`（仍在 `internal/transformers/**` 边界内），**不**塞进 commonjsmodule。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing）。复用 6e `collect_external_module_info`（`exported_names`、新增非 default `export class` 名收集）、6e-3 `make_exports_assignment` 机器、arena 既有 `new_void_expression`/`new_numeric_literal`/`new_import_equals_declaration` 数据。**ZERO** ast/printer/checker 增长。

**Go/tsc 形态确认（tsc 5.7.3 --module commonjs --target es2017，本端口为对应 transform 单跑的形态）**：
- `import x = require("./m"); x;` → tsc `"use strict";\nObject.defineProperty(exports, "__esModule", { value: true });\nconst x = require("./m");\nx;`（注：未用的 `import =` 被类型擦除 elide，需有 use 才 emit `const`）。本端口 commonjs-only 单跑：沿用 6e-3「仅 value 导出才发 `__esModule`」简化，import-only 模块不发标记（与既有 `named_import_and_use` 测试约定一致）→ `const x = require("m");\nx;`。`x` 是真 `const`，其 use 不改写（不入 `bindings`）。
- `export const y = 1;` → `…__esModule…;\nexports.y = void 0;\nexports.y = 1;`（导出名先零初始化）。
- `export const a = 1; export const b = 2;` → `…;\nexports.b = exports.a = void 0;\nexports.a = 1;\nexports.b = 2;`（**链式、逆序**：源序 `a,b` 折叠成 `exports.b = exports.a = void 0`，最后一个名在最外层；Go 50 一 chunk）。
- `const x = 1; export { x };` → `…;\nexports.x = void 0;\nconst x = 1;\nexports.x = x;`；`export { x } from "m"` / `export { a as b } from "m"` → 导出名（`x` / `b`）同样进零初始化，再 require + live-binding getter。
- `export class C {}` → `…;\nexports.C = void 0;\nclass C {\n}\nexports.C = C;`（类名进导出名）；但 `export function f() {}` **不**进（Go 把函数记进 `exportedFunctions`，不入 `exportedNames`）；`export default …` / `export = …` / `export * from "m"` 也不进。
- usestrict 单跑：`export const y = 1;`（module=CommonJs）→ `"use strict";\nexport const y = 1;`（只前插指令，不降级 export——降级是 module transformer 的事）；已存在 `"use strict"` 不重复；外部模块在 module=EsNext 下跳过。

先确认基线绿（`cargo test -p tsgo_transformers` = 158 unit + 25 doctest）。逐行为红→绿：

1. **`import_equals_require_lowers_to_const_require`（tracer，genuine RED）**：`import x = require("m"); x;` → 红（无 `ImportEqualsDeclaration` 臂 → catch-all 恒等保留 `import x = require("m");`）→ `transform_common_js_module` 增 `Kind::ImportEqualsDeclaration` 臂调 `lower_import_equals_to_require`（取 `ImportEqualsData.name`/`module_reference`，要求 `ExternalModuleReference` 且其 expression 为 `StringLiteral`，建 `const <name> = require("<m>");`；`export import =`/内部模块引用返回 `None` 落 catch-all DEFER）→ `const x = require("m");\nx;` → 绿。
2. **`export_const_becomes_exports_assignment`（既有测试改期望，genuine RED）**：把期望加 `exports.y = void 0;` → 红（无零初始化）→ `transform_common_js_module` 在 marker 后调 `make_exports_void_zero_inits(ec, &exported_name_texts)`（先 snapshot `info.exported_names` 文本，避免跨 `arena_mut` 持 `info` 借用）；最小实现：逐名一句 `exports.<name> = void 0;` → 单名通过 → 绿。
3. **`multiple_exported_names_share_chained_void_zero_init`（genuine RED）**：`export const a; export const b;` → 红（逐名两句 `exports.a = void 0;\nexports.b = void 0;`，期望单句链）→ 重构 `make_exports_void_zero_inits` 为 chunk(50) 折叠：`right = void 0`，对 chunk 内每名 `right = (exports.<name> = right)`，每 chunk 一句 → `exports.b = exports.a = void 0;` → 绿。同步更新 `local_named_export` / `re_export_named` / `re_export_renamed` 三既有测试期望（均单导出名，链式实现下直接绿，作回归覆盖）。
4. **`exported_class_declaration_keeps_decl_and_assigns_export`（既有测试改期望，genuine RED）**：期望加 `exports.C = void 0;` → 红（`collect_external_module_info` 未收类名）→ 增 `Kind::ClassDeclaration` 臂：带 EXPORT 且**非** DEFAULT、有名 → `add_unique_exported_name`（对标 Go `addExportedName(name)`；default 类只记 binding、函数声明记 `exportedFunctions`，均不收）→ `exports.C = void 0;` 出现 → 绿。既有 `exported_function_declaration`（不发 void-0）与 `exported_default_function/class`（default 不收）保持绿，锁定排除路径。
5. **`commonjs_module_gains_use_strict_prologue`（tracer，genuine RED）**：新建 `estransforms/usestrict.rs`（先 passthrough stub）+ 接进 `estransforms/mod.rs` + 写测试 → 红（恒等无 `"use strict"`）→ 实现 `transform_use_strict`：JSON 跳过；`is_external && emit_module_kind >= ES2015` 跳过（DEFER 精确 `format` 门控）；否则 `ensure_use_strict`（首句已是 `"use strict"` 前缀指令则原样返回，否则前插 `new_string_literal("use strict")` 的 `ExpressionStatement`）重建 SourceFile → `"use strict";\nexport const y = 1;` → 绿。companion 覆盖：`existing_use_strict_prologue_is_not_duplicated`（dedup 分支）、`esm_external_module_skips_use_strict`（module=EsNext 外部模块跳过）。

> 设计/divergence：(1) **`__esModule` for import-only 模块**：Go `shouldEmitUnderscoreUnderscoreESModule` = `isExternalModule && exportEquals == nil`（import-only 也发标记）；本端口沿用 6e-3「仅 value 导出发标记」简化（`module_has_exports`），故 `import x = require("m")` 单跑无标记——与既有 `named_import_and_use` 约定一致，DEFER 完整外部模块门控。(2) **void-0 链逆序**：与 tsc 一致（源序 `a,b` → `exports.b = exports.a = void 0`），chunk(50) 忠实移植。(3) **导出名集合**：复用/扩展 `collect_external_module_info.exported_names`，仅含可达子集（`export const`、local/re-export named、非 default `export class`）；string-literal 导出名走 element-access 形态 DEFER（本轮全 identifier）。(4) **usestrict 的 `format` 门控**：Go `getEmitModuleFormatOfFile`（per-file，依赖 package.json type/resolver）未线进 `TransformOptions`，本端口以 emit module kind 近似 `format >= ES2015`——对非 Node ESM kind（`ES2015..=ESNext`/`Preserve`）精确，对 `Node16+`（CJS format 仍应发 `"use strict"`）不精确，DEFER。(5) `import =` 仅 external-module-reference 非 export 形；`export import x = require("m")`（→ `exports.x = require(...)`）与 Node16+ 同步 require helper DEFER。(6) 沿用确定性命名 / 作用域无关 use 改写 / 不发 `"use strict"`（commonjs-only 单跑）等既有简化。

**测试计数（6x 新增）**：`tsgo_transformers` +5 `#[test]`（`import =` 1 + 多名 void-0 链 1 + usestrict tracer/dedup/ESM-skip 3；`export const`/`export class`/`local named`/`re-export ×2` 为既有测试改期望，不计数）+1 doctest（`new_use_strict_transformer`）→ **163 unit + 26 doctest**（6w 基线 158+25）。

### upstream（ast/printer/checker）增长（6x）

- **无**。`import =`/void-0/usestrict 全走 arena 既有构造器（`new_import_equals_declaration` 数据读取、`new_void_expression`/`new_numeric_literal`/`new_property_access_expression`/`new_binary_expression`/`new_token`/`new_expression_statement`/`new_string_literal`/`new_source_file`）+ 6e `collect_external_module_info`（本轮于 `moduletransforms` 内扩 `export class` 名收集，仍在 `internal/transformers/**`）。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by）

- **作用域正确的 use 解析**（同 6e-2…6w 缺口）：blocked-by 真实 `ReferenceResolver`。
- **碰撞无关的 require var 名**：blocked-by `NewGeneratedNameForNode`。
- **import-only 模块的 `__esModule` 标记 / 完整外部模块门控**：blocked-by 完整 `shouldEmitUnderscoreUnderscoreESModule`（`IsExternalModule`）端口决策。
- **`export import x = require("m")`（→ `exports.x = require(...)`）、Node16+ `import =` 同步 require helper**：blocked-by resolver / helper 基建。
- **usestrict 精确 `format` 门控（`getEmitModuleFormatOfFile`）**：blocked-by per-file module-format 分析（resolver）。
- **string-literal 导出名 void-0（element-access 形）、匿名 default decl、`export * as ns`、动态 `import()`、local `export { x }` live-binding**：同既有 DEFER。

## 6y worklog（estransforms `forawait` — async 生成器函数声明 → `__asyncGenerator` 包装；red→green 推进记录）

> 本轮落地 ES2018 **async 生成器函数声明**降级（`forawait.go` 的 async-generator 分支；`for await` 本身 DEFER）。Go ground truth 不在 `async.go`（那只管 ES2017 `__awaiter`），而在 **`forawait.go`**（ES2018 transformer，同时管 `for await` 与 async 生成器）：`visitFunctionDeclaration`（`Async && Generator` 分支）→ `transformAsyncGeneratorFunctionBody` → `NewAsyncGeneratorHelper(generatorFunc, hasLexicalThis)`；body 内 `visitAwaitExpression`/`visitYieldExpression`/`visitReturnStatement` 按 `enclosingFunctionFlags` 改写。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing，新增 `new_for_await_transformer` 公共入口，additive；不改既有 transformer 入口）。**ZERO** ast/printer/checker 增长（全走 arena 既有构造器 + `factory.new_generated_name_for_node`/`new_unscoped_helper_name`）。

### Go ground-truth 校正（briefing 偏离，已 VERIFY 跟随 Go + tsc）

> briefing 的 tracer 称「`await x` → `yield __await(x)`，且 **`yield x` 保留 `yield x`**」。**实测 Go（`forawait.go:visitYieldExpression` 非 `*` 分支 → `createDownlevelAwait`）+ tsc（`6.0.3 --target es2017`）均为 `yield e` → `yield yield __await(e)`**（先 await 再 yield 该值）。本端口跟随 Go：
> - `async function* g() { await x; yield y; }` → `function g() { return __asyncGenerator(this, arguments, function* g_1() { yield __await(x); yield yield __await(y); }); }`
> - `yield* y;` → `yield __await(yield* __asyncDelegator(__asyncValues(y)));`（prologue helper 序：`__asyncValues`,`__await`,`__asyncDelegator`,`__asyncGenerator`，tsc 对拍一致）
> - `return y;` → `return yield __await(y);`；bare `yield;` → `yield yield __await(void 0);`
>
> **helper 定义位置校正**：briefing 要求「确保 `__asyncGenerator`/`__await` 在 helper 表中（缺则移植 Go 定义）」。helper 表在 `internal/printer/emithelpers.rs`，而 printer 出本轮编辑范围（边界仅 `internal/transformers/**`）。故把 `__await`/`__asyncGenerator`/`__asyncDelegator`/`__asyncValues` 四个 `EmitHelper` **`pub static` 定义在 `forawait.rs` 内**（文本 verbatim Go `helpers.go`，tsc 对拍逐字一致；`request_emit_helper(&'static EmitHelper)` 跨 crate 可用；`ASYNC_GENERATOR_HELPER`/`ASYNC_DELEGATOR_HELPER` 的 `dependencies = &[&AWAIT_HELPER]` 引用本 crate static）。

先确认基线绿（`cargo test -p tsgo_transformers` = 163 unit + 26 doctest）。逐行为红→绿：

1. **`async_generator_function_lowers_to_async_generator_wrapper`（tracer，genuine RED）**：先建 `forawait.rs` 骨架（`for_await_visit` 仅 `SourceFile`→visit_source_file + 容器透传）+ 四个 helper static + `mod.rs` 注册 + tracer 测试 → 红（恒等输出 `async function* g() { await x; yield y; }`）→ 实现 `visit_async_generator_function_declaration`（剥 async 修饰 + 去 asterisk）+ `build_async_generator_wrapper_body`（inner `function* g_1`，名经 `new_generated_name_for_node`；`build_async_generator_call` 请求 `__await`+`__asyncGenerator`、`this`/`arguments` 实参）+ `convert_async_generator_body_node`（`AwaitExpression`→`yield __await(x)`；非 `*` `YieldExpression` 带 expr →`yield (yield __await(e))`）→ 绿。
2. **`async_generator_yield_delegate_uses_async_delegator`（yield*，genuine RED）**：`async function* a() { yield* y; }` → 红（`yield* y` 透传）→ 在 YieldExpression 臂加 `asterisk_token.is_some()` 分支：`__asyncValues(e)` → `__asyncDelegator(...)` → `yield* __asyncDelegator(...)`（复用原 asterisk token）→ `__await(...)` → `yield __await(...)`（`build_async_values_call`/`build_async_delegator_call`）→ 绿。helper 序经依赖（`asyncDelegator`→`await`）+ 请求序得 `asyncValues,await,asyncDelegator,asyncGenerator`，与 tsc 一致。
3. **`async_generator_return_awaits_value`（return，genuine RED）**：`async function* b() { return y; }` → 红（`return y;` 透传）→ 加 `ReturnStatement` 臂 → `return yield __await(y)`（`create_downlevel_await`，None→void 0）→ 绿。
4. **`async_generator_bare_yield_uses_void_zero`（bare yield，genuine RED）**：`async function* c() { yield; }` → 红（`yield;` 透传）→ 把非 `*` YieldExpression 分支的 expr 改 `match { Some=>convert, None=>void 0 }` → `yield yield __await(void 0)` → 绿。
5. **`async_generator_method_is_left_unchanged`（DEFER 守卫，characterization）**：`class C { async *m() { await x; } }` → 仅匹配 `FunctionDeclaration`，方法体不被改写（printer 仅把类规范化为多行）→ `class C {\n    async *m() { await x; }\n}`，锁定「async 生成器方法本轮不误降级」边界。

> 设计/divergence：(1) body 改写用 ec-threaded 递归（`convert_async_generator_body_node` + `visit_each_child_converting` map 替换），停在嵌套函数样作用域（各自 async 边界），镜像 async.rs 6m 模式但穿 `&mut EmitContext` 以请求 helper。(2) `hasLexicalThis` 对函数声明恒为 `true`（自身 `this`），Go 的 hierarchy-facts 线程化（方法/箭头/super）DEFER。(3) helper 请求序逐字镜像 Go factory（`NewAsyncGeneratorHelper`：await 后 asyncGenerator；`NewAsyncDelegatorHelper`：await 后 asyncDelegator），优先级均 `None` → printer stable-sort 保留请求序。(4) 未设 Go `NewAsyncGeneratorHelper` 的 `EFAsyncFunctionBody|EFReuseTempVariableScope` emit flags（async.rs awaiter 同样未设，可达子集 emit 不依赖）；变量环境 merge / 非简单参数 / super 捕获 DEFER。

**测试计数（6y 新增）**：`tsgo_transformers` +5 `#[test]`（tracer 1 + yield* 1 + return 1 + bare yield 1 + 方法 DEFER 守卫 1）+1 doctest（`new_for_await_transformer`）→ **168 unit + 27 doctest**（6x 基线 163+26）。

### upstream（ast/printer/checker）增长（6y）

- **无**。全走 arena 既有构造器（`new_function_declaration`/`new_function_expression`/`new_block`/`new_return_statement`/`new_yield_expression`/`new_call_expression`/`new_keyword_expression`/`new_identifier`/`new_token`/`new_void_expression`/`new_numeric_literal`）+ factory `new_generated_name_for_node`/`new_unscoped_helper_name` + EmitContext `request_emit_helper`/`read_emit_helpers`/`add_emit_helper`（均既有）。四个 `EmitHelper` static 定义在 `internal/transformers/estransforms/forawait.rs`（**未**触碰 `internal/printer/emithelpers.rs`）。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，forawait 维度）

- **`for await (x of y)` downlevel**：完整 async-iteration 脚手架（`__asyncValues` 迭代器 + downlevel-`await(.next())` + generated-name iterator/result/value temps + `iterator.return` 清理嵌套 try/finally，`convertForOfStatementHead`）。blocked-by：该脚手架体量大，部分实现会产出错误代码。
- **async 生成器方法 / 函数表达式 / 箭头**：blocked-by `EmitContext` super-capture（`_super`/`_superIndex` 绑定 + `AsyncSuperHelper`）+ hierarchy-facts `hasLexicalThis` 跨作用域线程化。
- **非简单参数列表**（默认/rest 参数 → 占位参 + inner 转发）、**变量环境 merge**（`StartVariableEnvironment`/`EndAndMergeVariableEnvironmentList`）、**top-level await**、**`EFAsyncFunctionBody`/`EFReuseTempVariableScope` emit flags**。

## 6z worklog（estransforms `forawait` — `for await (x of y)` downlevel → async-iteration 脚手架；red→green 推进记录）

> 本轮落地 6y 自身 DEFER 的 **`for await (x of y)` downlevel**（`forawait.go:transformForAwaitOfStatement` + `convertForOfStatementHead`）。Go ground truth：`visitForOfStatement`（`AwaitModifier != nil` 分支）→ `transformForAwaitOfStatement`：`__asyncValues(<expr>)` 迭代器 temp + `result` temp + C-style `for`（var list `nonUserCode=true, iterator=__asyncValues(expr), result`；条件 `result = downlevelAwait(iterator.next()), done = result.done, !done`；incrementor `nonUserCode=true`）+ `convertForOfStatementHead`（`value=result.value; nonUserCode=false; <绑定>; <body>`）+ `try { for } catch (e) { errorRecord = { error: e } } finally { try { if (!nonUserCode && !done && (returnMethod = iterator.return)) downlevelAwait(returnMethod.call(iterator)) } finally { if (errorRecord) throw errorRecord.error } }`。`done`/`errorRecord`/`returnMethod`/`value` 经 `AddVariableDeclaration` hoist。
>
> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing，复用 6y `new_for_await_transformer` 公共入口，不改入口）。**ZERO** ast/printer/checker 增长（全走 arena 既有构造器 + factory `new_temp_variable`/`new_unique_name`/`new_generated_name_for_node` + EmitContext `start/end_variable_environment`/`add_variable_declaration`/`set_emit_flags`/`request_emit_helper`）。

### Go/tsc ground-truth 校正（已 VERIFY 对拍 `tsc 5.6 --target es2017`）

> 在 `--target es2017` 下 `async`/`await` 原生保留、仅 ES2018 `for await` downlevel，故 tsc 输出**等价于 forawait-only stage**（plain async 函数内 `createDownlevelAwait` 走非生成器分支 → `await`，非 `yield __await`）。对拍输入 `async function f() { for await (const x of gen()) {} }` →
> ```js
> var __asyncValues = …;
> async function f() {
>     var _a, e_1, _b, _c;
>     try {
>         for (var _d = true, _e = __asyncValues(gen()), _f; _f = await _e.next(), _a = _f.done, !_a; _d = true) {
>             _c = _f.value;
>             _d = false;
>             const x = _c;
>         }
>     }
>     catch (e_1_1) { e_1 = { error: e_1_1 }; }
>     finally {
>         try { if (!_d && !_a && (_b = _e.return)) await _b.call(_e); }
>         finally { if (e_1) throw e_1.error; }
>     }
> }
> ```
> temp 映射：`done=_a`、`errorRecord(NewUniqueName "e")=e_1`、`returnMethod=_b`、`value=_c`、`nonUserCode=_d`、`iterator(NewTempVariable)=_e`、`result(NewTempVariable)=_f`、`catchVariable(NewGeneratedNameForNode errorRecord)=e_1_1`。仅请求 `__asyncValues` helper。
>
> **校正 1（identifier 源 DEFER，printer 限制）**：Go 对 **identifier 源**（`for await (const x of y)`）令 `iterator = NewGeneratedNameForNode(y)`（→ `y_1`）、`result = NewGeneratedNameForNode(iterator)`（→ `y_1_1`），依赖 printer 的 **resolving `getTextOfNode`**（对已生成名取其 resolved 文本作 base）。Rust printer 的 `namegenerator.generate_name_for_node` 用 raw `arena().text()`——对 generated-name 节点其文本为 `""`（`new_generated_name_for_node` 建的 identifier text=""）→ `result` 名退化为 `"1"`（非法 JS）。故 **identifier 源 DEFER**（blocked-by：printer resolving 名生成未移植）；本轮用**非 identifier 源 `gen()`**，iterator/result 走 `NewTempVariable` → 干净 `_e`/`_f`。
>
> **校正 2（catch 变量 cosmetic 偏离）**：同因，`catchVariable = NewGeneratedNameForNode(errorRecord "e_1")` Go/tsc 得 `e_1_1`，Rust 用 raw text `"e"` → `makeUniqueName("e")` → `"e_1"` 已 reserved → **`e_2`**。catch 变量是 fresh binding，名字语义无关 → 纯 cosmetic 偏离，已在测试注释 + DEFER 记录。锁定 Rust 实际输出 `e_2`。
>
> **校正 3（函数体 multi-line）**：Go `for await` 不重建函数体（原 parsed block 保留 `MultiLine`，`UpdateBlock` 透传）。Rust 必须重建函数体（注入 hoisted vars + 转换后 for-await）→ synthesized block 经 `emit_function_body` 的 `should_emit_block_function_body_on_single_line` 默认**单行**（与普通 block 经 `emit_block` 默认多行不对称）。故在 `visit_for_await_function_body` 重建时：若 hoist 非空（说明降级了 for-await）则置 `EmitFlags::MULTI_LINE`，匹配 tsc 多行脚手架；无 hoist 且无 child 改动则**原样返回**（保留源 block 多行布局与身份，不扰动无 for-await 的普通函数）。

先确认基线绿（`cargo test -p tsgo_transformers` = 168 unit + 27 doctest）。逐行为红→绿：

1. **`for_await_of_lowers_to_async_iteration_scaffold`（tracer，genuine RED）**：`async function f() { for await (const x of gen()) {} }` → 红（恒等透传 `for await (const x of gen()) { }`）→ 实现 `for_await_visit` 新增 `FunctionDeclaration`（非 async-gen）→ `visit_function_declaration`（变量环境包裹函数体）+ `ForOfStatement`（`await_modifier.is_some()`）→ `transform_for_await_of_statement`（完整脚手架，`create_downlevel_await_value` = 非生成器 `await`）+ `convert_for_of_statement_head` + `create_for_of_binding_statement`（VariableDeclarationList → `const x = _c`，保 `BLOCK_SCOPED` flags）+ helper `new_assignment`/`inline_expressions_ec`/`new_function_call_call`，catch/finally 子块置 `SINGLE_LINE`，函数体置 `MULTI_LINE` → 绿（catch 变量锁 `e_2`，见校正 2）。
2. **`for_await_of_body_statements_follow_the_binding`（body 引用 x，characterization）**：`{ use(x); }` → body 语句经 `convert_for_of_statement_head` 的 block-splice（`statements.extend(block.statements)`）拼在 `const x = _c` 后 → 绿。脚手架是单体 Go 函数的忠实端口，body 拼接随 tracer 一并落地（空 body tracer 无法区分；忠实端口不能产「部分脚手架」——6y DEFER 注明部分实现会产出错误代码），故此为锁定可达分支的 characterization 测试。
3. **`for_await_of_existing_variable_target_binds_with_assignment`（existing-variable target，characterization）**：`for await (x of gen())` → `create_for_of_binding_statement` 非-VariableDeclarationList 分支 → plain assignment `x = _c;`（非声明）→ 绿。

> 设计/divergence：(1) `enclosingFunctionFlags` **未线程化**——本轮只支持 async 非生成器 `for await`（downlevel = `await`）；async 生成器内 `for await`（需 `yield __await`）DEFER。(2) hoisted temps 经新增的 plain-function 变量环境（镜像 optionalchain 6i），top-level `for await`（无函数变量环境）DEFER。(3) nested-loop 的 `errorRecord` reset（`ancestorFacts & IterationContainer` → `errorRecord = void 0, __asyncValues(…)`）未实现，本轮 initializer 恒为 `__asyncValues(<expr>)`。(4) source-map ranges（`SetSourceMapRange`）/`SetOriginal`/`EFNoTokenTrailingSourceMaps` 未设（可达子集 emit 不依赖，与 6y/async.rs 一致）。

**测试计数（6z 新增）**：`tsgo_transformers` +3 `#[test]`（tracer 1 + body-ref 1 + assignment-target 1），doctest 不变 → **171 unit + 27 doctest**（6y 基线 168+27）。

### upstream（ast/printer/checker）增长（6z）

- **无**。全走 arena 既有构造器（`new_for_statement`/`new_try_statement`/`new_catch_clause`/`new_if_statement`/`new_throw_statement`/`new_variable_declaration(_list)`/`new_variable_statement`/`new_binary_expression`/`new_prefix_unary_expression`/`new_property_access_expression`/`new_call_expression`/`new_object_literal_expression`/`new_property_assignment`/`new_await_expression`/`new_expression_statement`/`new_block`/`new_token`/`new_identifier`/`new_keyword_expression`）+ factory `new_temp_variable`/`new_unique_name`/`new_generated_name_for_node` + EmitContext `start/end_variable_environment`/`add_variable_declaration`/`set_emit_flags`/`request_emit_helper`/`add_emit_helper`（均既有，6z 仅新增 `EmitFlags` re-export 引用，无新 API）。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，forawait 6z 维度）

- **identifier 源 `for await (const x of y)`**：blocked-by printer resolving `getTextOfNode`（嵌套 generated-name 解析，未移植）；Rust 现状会令 `result` 名退化为非法 `"1"`。
- **async 生成器内 `for await`**：blocked-by `createDownlevelAwait` 的生成器分支（`yield __await(...)`）= `enclosingFunctionFlags` 跨作用域线程化（本轮 transformer 是无状态 free-fn，未持 enclosing flags）。
- **destructuring 循环变量**（`for await (const [a, b] of …)`）：blocked-by destructuring flattener（`CreateForOfBindingStatement` 仅取 `Declarations[0].Name`，绑定模式需 6h destructuring）。
- **top-level `for await`**：blocked-by 源文件级变量环境 hoist（本轮只在 plain-function-body 起变量环境）+ top-level await。
- **label / `continue` / `break` interplay**（`visitLabeledStatement` + `RestoreEnclosingLabel`，`outermostLabeledStatement` 现恒 nil）、**nested-loop `errorRecord` reset**（`ancestorFacts & IterationContainer`）。

## 6aa worklog（estransforms `spread` — ES2015 数组字面量 + 调用参 spread → `__spreadArray`/`.apply`；red→green 推进记录）

> 本轮落地 ES2015 **数组字面量 spread**（`[...a, b]`）与 **调用参 spread**（`f(...args)`）降级（pre-ES2015 目标）。**关键发现：Go 端无 ES2015 spread transform**——`estransforms/` 无 `spread.go`/`es2015.go`，`GetESTransformer` 链止于 `NewES2016Transformer`（`default`/更老目标 fall through 到它，根本不降级 spread）。故 briefing 引用的 Go `transformAndSpreadElements`/`visitArrayLiteralExpression`/`spreadArrayHelper` **在本仓不存在**；ground truth 取 **`tsc --target es5` 实测对拍**（upstream `microsoft/TypeScript` `src/compiler/transformers/es2015.ts`）。新增 `spread.rs`（含 `SPREAD_ARRAY_HELPER` 定义，`tsgo_printer` 出编辑范围，同 `forawait.rs` 就地定义，verbatim tsc 文本）+ 注册 `pub mod spread;`。

### tsc `--target es5` 对拍（已 VERIFY，本轮 ground truth）

```
[...a, b];        -> __spreadArray(__spreadArray([], a, true), [b], false);
const c = [...a]; -> var c = __spreadArray([], a, true);   // const->var 是另一 stage，本 transform 仅降 spread
[1, ...a, 2];     -> __spreadArray(__spreadArray([1], a, true), [2], false);
f(...args);       -> f.apply(void 0, args);                // 单 spread 捷径，无 __spreadArray、无 helper
f(a, ...args);    -> f.apply(void 0, __spreadArray([a], args, false));
o.m(...args);     -> o.m.apply(o, args);                   // 标识符受体复用为 this
new C(...args);   -> new (C.bind.apply(C, __spreadArray([void 0], args, false)))();  // DEFER
```

> **briefing 校正**：briefing 称 `f(...args)` → `f.apply(void 0, __spreadArray([], args, false))`，实测 tsc 为 `f.apply(void 0, args)`（arg-list 单 spread 段经 single-segment 捷径直接传 bare 段表达式，不包 `__spreadArray`，也不请求 helper）。
> **pack 旗标规律**（对拍 6 例验证）：`pack = is_spread_segment && !is_argument_list`——数组字面量的 spread 段 `pack=true`、arg-list 的 spread 段恒 `pack=false`；任何字面量段恒 `pack=false`。最干净对照：`[...a, ...args]` → `__spreadArray(__spreadArray([], a, true), args, true)` vs `f(...a, ...args)` → `f.apply(void 0, __spreadArray(__spreadArray([], a, false), args, false))`（结构同、仅 pack 旗标差）。

### red→green 逐行为

1. **`array_spread_then_element_lowers_to_spread_array_segments`（tracer，genuine RED）**：`[...a, b];` → 红（skeleton 恒等透传 `[...a, b];`）→ 实现 `transform_and_spread_elements`（分段 + 累加器 + `__spreadArray` 折叠）+ `SPREAD_ARRAY_HELPER` request/prologue attach → 绿。
2. **`single_array_spread_folds_into_spread_array`（coverage/泛化）**：`const c = [...a];` → 复用 tracer 的折叠（单 spread 段，数组无 single-segment 捷径）→ 绿。
3. **`leading_literal_array_spread_starts_accumulator_at_first_segment`（coverage/泛化）**：`[1, ...a, 2];` → 走 `starts_with_spread=false` 路径（累加器起于 `[1]`）→ 绿。
4. **`call_with_single_spread_argument_lowers_to_apply`（call tracer，genuine RED）**：`f(...args);` → 红（透传 `f(...args);`）→ 实现 `CallExpression` 臂 + `try_lower_call_with_spread`（标识符 callee → `void 0` 受体）+ arg-list single-segment 捷径 → 绿。
5. **`call_with_leading_argument_and_spread_folds_into_spread_array`（coverage/泛化）**：`f(a, ...args);` → arg-list 多段折叠（`pack=false`）→ 绿。
6. **`member_call_with_spread_captures_receiver_as_this`（genuine RED）**：`o.m(...args);` → 先把 `PropertyAccessExpression` 受体臂回退为 `return None`(DEFER) → 红（透传 `o.m(...args);`）→ 实现简单标识符受体复用为 `apply` `this` → 绿。
7. **DEFER 守卫（characterization）**：`new C(...args);`（`NewExpression` 不处理）与 `a.b.m(...args);`（非简单成员受体 → `return None`）均保持不变 → 绿。

### scope / upstream 增长（6aa）

> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing；新增 `new_spread_transformer` 公共入口，未改既有入口）。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_array_literal_expression`/`new_call_expression`/`new_property_access_expression`/`new_keyword_expression`(true/false)/`new_identifier`/`new_numeric_literal`/`new_void_expression`）+ factory `new_unscoped_helper_name` + EmitContext `request_emit_helper`/`add_emit_helper`/`read_emit_helpers`（均既有）。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`。`SPREAD_ARRAY_HELPER` 作 `spread.rs` 内 `pub static EmitHelper`（不增 `tsgo_printer`）。
>
> **测试计数（6aa 新增）**：`tsgo_transformers` +8 `#[test]`（array tracer 1 + 2 coverage + call tracer 1 + 2 coverage + 2 DEFER 守卫）+ 1 doctest → **179 unit + 28 doctest**（6z 基线 171+27）。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，spread 维度）

- **`new C(...args)`**：blocked-by `new`-target bind 形（`new (C.bind.apply(C, __spreadArray([void 0], args, false)))()`，需 construct + `bind.apply` 合成）。
- **`super(...args)`**：blocked-by `super` 受体捕获（super-call 的 `_super.apply` / `_this = _super.call(...)` 形）。
- **非简单成员受体**（`foo().m(...args)`、`a.b.m(...args)`）：blocked-by `createCallBinding` 的 capture temp（受体须经变量环境 hoist 求值一次：`(_a = a.b).m.apply(_a, args)`）。
- **`--downlevelIteration`**：blocked-by `__read`/`__spread`/`__spreadArrays` 迭代 helper 形。
- **wiring 进 `GetESTransformer`**：无 `NewES2015Transformer` 链（`definitions` 端口本身 DEFER）；本轮仅以公共入口 `new_spread_transformer` 直测，待 ES2015 stage 成形后再接链。
- **packed-array-literal spread**（`[...[1, 2]]`）的 `PackedSpread`（`pack=false` 直接段）与 `isPackedArrayLiteral` single-segment 捷径：可达子集仅覆盖标识符 spread（`UnpackedSpread`）。

## 6ae worklog（`systemmodule` System.register 包装核心；red→green 推进记录）

> 本轮新增 `systemmodule.rs`（`module: system`），落地 **System.register 包装核心**：把模块体包进 `System.register([<deps>], function (exports_1, context_1) { "use strict"; return { setters: [<setters>], execute: function () { <body> } }; })`。Go ground truth `moduletransforms/systemmodule.go:NewSystemModuleTransformer`/`transformSourceFile`/`createSystemModuleBody`（本仓 `_submodules/TypeScript` 为空、无 `.go` 镜像，shape 取 **tsc `--module system` 已知输出**对拍 + 既有 CJS/ESM 端口的同源构造器/打印器约束）。复用 6e `collect_external_module_info` 收 external imports。

### Go-confirmed shape（打印器约束下）

```
System.register([<deps>], function (exports_1, context_1) {
    "use strict";
    return { setters: [<setters>], execute: function () { <body> } };
});
```

> **打印器约束**：Rust 打印器对 array/object 字面量**恒单行**（`emit_expressions.rs` 硬编码 `multi_line=false`，per-node `MultiLine` 未携带），故 return 对象 / setters 数组 / deps 数组 / 空 execute 块均**内联**（tsc 实为多行）；仅外层 module body block 设 `EmitFlags::MULTI_LINE`（Go `multiLine: true`）才多行。本轮 expected 取打印器实际输出（= tsc 结构，仅 whitespace 差），与既有 CJS `{ value: true }` 内联对象一致。
> **name generator 偏离（TODO(port)）**：binding-less import 的 setter param Go 用 `factory.createUniqueName("")`，tsc 渲染 `_1`；Rust name generator 空 base 路径有 `!base_name.is_empty()` 守卫，丢前导 `_` → 渲染 `1`（偏离 Go：Go 对空 base `charCodeAt(-1)=NaN !== '_'` 故补 `_`）。为复现 tsc `_1`，setter param 传 base `"_"`（`new_unique_name("_")` → `_1`）。printer 出编辑范围，记 TODO(port) 待对齐空 base 路径。

### red→green 逐行为

1. **`empty_module_wraps_in_system_register`（tracer，genuine RED）**：`""` → 红（passthrough 输出 `""`）→ 实现 register wrapper（`exports_1`/`context_1` via `new_unique_name`、`"use strict"` 前导、`return { setters: [], execute: function () { } }`，外层 body 设 `MULTI_LINE`）→ 绿 `System.register([], function (exports_1, context_1) {\n    "use strict";\n    return { setters: [], execute: function () { } };\n});`。
2. **`side_effect_import_adds_dependency_and_setter`（genuine RED）**：`import "m";` → 红（deps `[]`/setters `[]`）→ 实现 `collect_external_module_info` → 收 module specifier 进 deps 数组 + 每依赖一个空体 setter（`function (_1) { }`）→ 绿 `System.register(["m"], …, return { setters: [function (_1) { }], execute: function () { } } …)`。
3. **`top_level_value_statement_moves_into_execute_body`（genuine RED）**：`f();` → 红（execute 体空 `{ }`）→ 实现 `is_module_syntax_statement` 分区，非 import/export 语法的顶层语句按源序移入 execute 体 → 绿 `… execute: function () { f(); } …`。
4. **`non_system_module_kind_is_passthrough`（gate 覆盖）**：`module != System` → 透传（`f();` 不变）。

### scope / upstream 增长（6ae）

> **scope**：仅 `-p tsgo_transformers`（内部 transform plumbing；新增 `new_system_module_transformer` 公共入口 + `pub mod systemmodule;`，未改既有公共链入口）。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_array_literal_expression`/`new_object_literal_expression`/`new_property_assignment`/`new_function_expression`/`new_parameter_declaration`/`new_block`/`new_return_statement`/`new_string_literal`/`new_expression_statement`/`new_property_access_expression`/`new_call_expression`/`new_identifier`）+ factory `new_unique_name` + EmitContext `set_emit_flags`（均既有）。复用 6e `collect_external_module_info`。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`。
>
> **测试计数（6ae 新增）**：`tsgo_transformers` +4 `#[test]`（empty tracer + side-effect import + execute body + gate passthrough，前 3 均 genuine RED→GREEN）+ 1 doctest → **192 unit + 31 doctest**（6ad 基线 188+30）。`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净、`cargo fmt -p tsgo_transformers -- --check` 干净。

### DEFER（本轮确认的 blocked-by，systemmodule 维度）

- **export-setter 接线**：named export → register 体内 `exports({...})` 调用、每依赖 setter 体转发 import binding。blocked-by 真实 `ReferenceResolver`（checker `resolveName`/`EmitResolver`，同 CJS 缺口）。
- **import binding 重写 / hoisting / live bindings**：import 名用点改为依赖局部绑定、var/function hoist 进 register 体、live-binding `exports(...)` 更新。blocked-by `ReferenceResolver`。
- **`var __moduleName = context_1 && context_1.id;`**、**module-name 首参**（`System.register("name", [deps], …)`）、**`export *` star helper**、**dependency 分组/去重**（同模块多 import 合并为一组一 setter）、**`export =` 交互**。

## 6af worklog — `tstransforms/importelision`（作用域正确未引用 value import 省略）

> 本轮被 checker **4an** 解锁（`EmitResolver::is_referenced(program, node) -> bool` + `resolve_reference`，作用域正确：把 identifier *use* 经作用域链解析到声明 symbol，内层 shadowing 胜出；`is_referenced` 排除声明自身名节点、是真作用域查询而非按名匹配）。本轮把真·作用域正确的未引用 import 省略接进 transform 管线，落地 Go `ImportElisionTransformer` 的 import 侧可达子集。

### resolver 如何接线（additive）

- 新增公共类型 `EmitReferenceResolver`（`lib.rs`）：包 `Rc<dyn tsgo_checker::BoundProgram>` + `tsgo_checker::EmitResolver`，暴露 `is_referenced(node) -> bool`（委托 `EmitResolver::is_referenced(program, node)`，Go `emitResolver.IsReferencedAliasDeclaration`）。
- 经 **新 factory 的额外入参** `new_import_elision_transformer(opt: &TransformOptions, resolver: EmitReferenceResolver)` 传入——**不是** `TransformOptions` 字段。原因（关键约束）：`internal/compiler/emitter.rs:107` 用**穷举字面量** `TransformOptions { context, compiler_options }` 构造它，给 struct 加字段会破坏 `tsgo_compiler` 编译，而 compiler crate 不在本 lane 编辑范围。额外入参是不改任何既有签名的纯 additive 方案。
- 节点 id 对齐：`EmitReferenceResolver` 的 `BoundProgram` 自带独立 parse+bind 的 arena；因 parse 确定性，其节点 id 与 transform 读取的（独立 parse 的）`EmitContext` arena 一致，故 transform 拿到的原始 import 声明节点 id 在 resolver 程序里解析为同一句法节点。测试 helper `build_reference_resolver`（`test_support.rs`，`#[cfg(test)]`）用 `tsgo_parser`+`tsgo_binder` 建一个 crate-local `BoundProgram`（checker 的 `StubProgram` 是 `pub(crate)`，跨 crate 不可见，故自建）。

### red→green 切片（每片 input → emitted-shape）

1. **`unused_named_import_is_elided`（genuine RED）**：`import { x } from "m";`（x 未用）期望 `∅` → 红（先 stub `import_elision_visit` 为恒等透传，emit 出 `import { x } from "m";`）→ 实现 SourceFile/ImportDeclaration/ImportClause/NamedImports/ImportSpecifier 各臂（`is_referenced` 为 false → specifier 丢 → NamedImports 空 → ImportClause 空 → 整 ImportDeclaration 省略）→ 绿。
2. **`used_named_import_is_kept`（guard）**：`import { x } from "m";\nx;` → 原样保留（`x` 被 value 引用，`is_referenced` 为 true，specifier 与外层 import 存活）。
3. **`shadowed_use_does_not_keep_import_alive`（headline，genuine RED）**：`import { x } from "m";\nfunction f() {\n    var x = 1;\n    x;\n}` 期望仅函数 → 先把 keep-决策临时改成 `true`（模拟按名匹配——文本里 `x` 出现就续命）观察红（import 被错误保留）→ 还原为 `resolver.is_referenced(node)`（作用域正确：内层 `var x` shadow，外层 import 未引用，`is_referenced` 为 false）→ 绿。证明真作用域解析 vs 按名匹配。
4. **guard 群**：per-specifier 丢弃（`import { a, b } from "m";\na;` → `import { a } from "m";\na;`，含尾逗号修复——rebuild 用 `NodeList::new` 给 undefined range 避免 printer 从源跨度推断尾逗号）、side-effect-only `import "m";` 不省略、namespace import 省略/保留、default import 省略/保留。

### scope / upstream 增长（6af）

> **scope**：仅 `-p tsgo_transformers`。新增 `pub mod importelision;`（tstransforms）+ `pub fn new_import_elision_transformer` + 公共 `EmitReferenceResolver`（均 additive 新公共项，未改既有签名）。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_source_file`/`new_import_declaration`/`new_import_clause`/`new_named_imports`）+ `visit_nodes_removable`（既有）；消费 checker 4an 既有 `EmitResolver`。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`、README、root Cargo。
>
> **测试计数（6af 新增）**：`tsgo_transformers` +9 `#[test]`（slice1 + scope-correct + 7 guard；slice1 与 scope-correct 均 genuine RED→GREEN）+ 4 doctest（`EmitReferenceResolver` ×3 + `new_import_elision_transformer`）→ **201 unit + 35 doctest**（6ae 基线 192+31）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（证明公共 API additive）。

### DEFER（本轮确认的 blocked-by，importelision 维度）

- **export 侧**（`ExportAssignment` / `ExportDeclaration` / `NamedExports` / `ExportSpecifier`）：需 `EmitResolver::IsValueAliasDeclaration`（未移植）。blocked-by checker `isValueAliasDeclaration`。
- **`ImportEqualsDeclaration`**（`import x = require("m")` / `import x = a.b`）：需 `EmitResolver::IsTopLevelValueImportEqualsWithEntityName` + `shouldEmitImportEqualsDeclaration` 的 external-module 判定。blocked-by checker 相应查询。
- **type-only 位置 use 续命 value import**（一个*类型*位置的 use 不应让 value import 存活）：需每 use 点的 type-vs-value meaning。blocked-by checker `markLinkedReferences`（4an 自身亦 DEFER 了此项）。
- **`IsInJSFile` 短路**（`.js`/`.jsx` 源保留全部 import）：本轮测试为 `.ts`，`should_emit_alias_declaration` 简化为纯 `is_referenced`。blocked-by emit-context `ParseNode` + JS-file flag 接线。
- **`verbatimModuleSyntax`/`isolatedModules`/`importsNotUsedAsValues` 策略变体**、**跨文件 alias**。

### 推荐下一轮

- checker 侧补 `EmitResolver::is_value_alias_declaration`（解锁 export 侧 + `export =` elision）；之后本 lane 接线 importelision 的 `ExportDeclaration`/`NamedExports`/`ExportSpecifier`/`ExportAssignment` 各臂。
- 或：把 importElision transform 串进 CommonJS/ESM 管线（chain importElision → moduletransform），端到端验证未用 import 不再 emit `require(...)`。

## 6ag worklog — `tstransforms/importelision` EXPORT 侧（type-only export specifier 省略）

> 本轮被 checker **4ao** 解锁（`EmitResolver::is_value_alias_declaration(program, node) -> bool` + `is_referenced_alias_declaration(program, node) -> bool`，均为 additive 公共方法）。落地 Go `ImportElisionTransformer` 的 **export specifier 侧**可达子集：按 `is_value_alias_declaration` 丢弃 type-only / 非值 export specifier，空 `NamedExports` 整 `ExportDeclaration` 省略。

### resolver 如何接线（additive，复用 6af `EmitReferenceResolver`）

- `EmitReferenceResolver`（`lib.rs`）**additive 扩展**两个透传方法（未改既有签名 / 字段）：
  - `is_value_alias_declaration(node) -> bool` → 委托 `EmitResolver::is_value_alias_declaration(program, node)`（Go `emitResolver.IsValueAliasDeclaration`）。
  - `is_referenced_alias_declaration(node) -> bool` → 委托 `EmitResolver::is_referenced_alias_declaration(program, node)`（Go `emitResolver.IsReferencedAliasDeclaration`）。
- 节点 id 对齐同 6af（独立 parse+bind 的 `BoundProgram` 与 transform arena 因 parse 确定性共享 id）。`new_import_elision_transformer(opt, resolver)` 签名**不变**（resolver 仍是同一句柄，只是多用了它新增的方法）。

### red→green 切片（每片 input → emitted-shape；import-elision 单跑，未串 type eraser）

1. **`type_only_export_specifier_is_elided`（tracer，genuine RED）**：`interface I {}\nexport { I };` 期望 `interface I {\n}`（export 整丢，interface 透传——type eraser 未在本管线，故 interface 留存）→ 先无 export 各臂，`export { I };` 经 default 臂透传见红 → 实现 `ExportDeclaration`/`NamedExports`/`ExportSpecifier` 各臂（`is_value_alias_declaration(I)` 为 false → specifier 丢 → `NamedExports` 空 → `ExportDeclaration` export clause `None` → 整声明省略）→ 绿。
2. **`value_export_specifier_is_kept`（guard）**：`function f() {}\nexport { f };` → 原样（`f` 是 value，`is_value_alias_declaration` true，specifier 与外层 export 存活）。
3. **`type_only_export_specifier_dropped_value_specifier_kept`（per-specifier）**：`interface I {}\nfunction f() {}\nexport { I, f };` → `interface I {\n}\nfunction f() { }\nexport { f };`（逐 specifier 丢——type-only `I` 丢、value `f` 留，`UpdateNamedExports` over 存活元素；rebuild 用 `NodeList::new` 避免源跨度尾逗号）。
4. **`export_star_reexport_is_kept`（export_clause None 分支）**：`export * from "m";` → 原样（无 export clause，跳过 clause 访问直接 rebuild，镜像 Go `n.ExportClause == nil`）。

### scope / upstream 增长（6ag）

> **scope**：仅 `-p tsgo_transformers`。新增 `import_elision_visit` 的 `ExportDeclaration`/`NamedExports`/`ExportSpecifier` 各臂 + `visit_export_declaration`/`visit_named_exports`（私有）+ `EmitReferenceResolver` 两个 additive 透传方法。**ZERO** ast/printer/checker 增长——export 各臂全走 arena 既有构造器（`new_export_declaration`/`new_named_exports`）+ `visit_nodes_removable`（既有）；消费 checker 4ao 既有 `EmitResolver`。`new_import_elision_transformer` 签名未变。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`、README、root Cargo。
>
> **测试计数（6ag 新增）**：`tsgo_transformers` +4 `#[test]`（slice1 tracer genuine RED→GREEN + 3 guard/coverage）+ 2 doctest（`EmitReferenceResolver::is_value_alias_declaration` / `is_referenced_alias_declaration`）→ **205 unit + 37 doctest**（6af 基线 201+35）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（证明公共 API additive）。

### DEFER（本轮确认的 blocked-by，importelision export 维度）

- **`ImportEqualsDeclaration` 省略**（`import x = require("m")` 未引用应省略）：**实测 BLOCKED**。checker 4ao `is_referenced` 的 `declaration_name` 辅助**未列 `ImportEqualsDeclaration`**，故声明自身名 `x` 未被排除，扫描标识符时 `x` 解析回自身 symbol → `is_referenced` 恒为 true → `is_referenced_alias_declaration` 恒为 true → 未用 `import =` 也被错误保留（已用 throwaway 双例 used/unused 实测两者均"保留"、不可区分，确认无可观察的省略行为）。无法在不改 `internal/checker/**`（边界外）的前提下驱动出区分性 GREEN，故本轮不接 `ImportEqualsDeclaration` 臂。blocked-by checker 4ao `declaration_name` 未排除 `ImportEqualsDeclaration` 名节点（+ `IsTopLevelValueImportEqualsWithEntityName` 用于 entity-name 形）。
- **`ExportAssignment` 省略**（`export = e` / `export default e`）：checker 4ao `is_value_alias_declaration` 的 `match` 仅处理 `ImportSpecifier | ExportSpecifier`，`ExportAssignment` 落 `_ => false`；而 Go `isValueAliasDeclarationWorker` 的 `KindExportAssignment` 分支对非 identifier 表达式返回 `true`、对 identifier 走 `isAliasResolvedToValue`。直接接线会把所有 `export default x` 误省略（Go 会保留 `let x; export default x;`）。blocked-by checker `isValueAliasDeclarationWorker` 的 `ExportAssignment` 分支（4ao DEFER）。
- **跨模块 target value-ness**（re-export 一个 imported binding：`import { x } from "m"; export { x };`）：blocked-by checker `exports.go`/`resolveExternalModuleSymbol`/`getExportSymbolOfValueSymbolIfExported`（4ao DEFER）。
- **value 位置 use 的 type-only-ness**、**`verbatimModuleSyntax`/`isolatedModules` 策略变体**、**`export *`（带 export clause 的 namespace re-export 改写）**。

## 6ah worklog — `tstransforms/importelision` `import =` / `export =` 侧（external-module import-equals + export-assignment 省略）

> 本轮被 checker **4ap** 解锁——4ap 扩展了 `EmitResolver`：(a) `is_referenced` 的 `declaration_name` 辅助新增 `ImportEqualsDeclaration => Some(d.name)` 臂，未引用的 `import x = require("m")` 现把自身名 `x` 排除出引用扫描，故 `is_referenced_alias_declaration` 现报 **false**（6ag 实测时恒 true）；(b) `is_value_alias_declaration` 新增 `ExportAssignment` 臂（`export = <value>` → true，type-only `export = I` → false，非 identifier 表达式 → true）。本轮**仅消费**这两处扩展，**未改 `internal/checker/**`**。

### resolver 如何接线（additive，复用 6af/6ag `EmitReferenceResolver`）

- 直接复用 6ag 已加的 `EmitReferenceResolver::is_referenced_alias_declaration(node)` / `is_value_alias_declaration(node)` 两个透传方法——**本轮 ZERO 新增透传**，只是 import-elision 各臂首次调用它们处理 `import =`/`export =`。
- `new_import_elision_transformer(opt, resolver)` 签名**不变**（resolver 同一句柄）。节点 id 对齐同 6af/6ag（独立 parse+bind 的 `BoundProgram` 与 transform arena 因 parse 确定性共享 id）。

### red→green 切片（每片 input → emitted-shape；import-elision 单跑，未串 type eraser）

1. **`unused_import_equals_require_is_elided`（tracer，genuine RED）**：`import x = require("m");` 期望 `""` → 红（6ag default 臂透传出 `import x = require("m");`）→ 加 `ImportEqualsDeclaration` 臂 + `visit_import_equals_declaration`：`is_external_module_import_equals_declaration`（模块引用 kind == `ExternalModuleReference`）为真时，`!is_referenced_alias_declaration(node)` → `None`；否则（entity-name 形）`Some(node)`（DEFER）→ 绿。
2. **`used_import_equals_require_is_kept`（guard）**：`import x = require("m");\nx;` → 原样保留（`x` 被引用，`is_referenced_alias_declaration` true）。守护省略臂不误删被引用 import。
3. **`type_only_export_equals_is_elided`（genuine RED）**：`interface I {}\nexport = I;` 期望 `interface I {\n}`（interface 透传因未串 type eraser；`export =` 整丢）→ 红（default 臂透传出 `...\nexport = I;`）→ 加 `ExportAssignment` 臂：`is_value_alias_declaration(node).then_some(node)`（type-only `I` → false → `None`）→ 绿。
4. **`value_export_equals_is_kept`（guard）**：`function f() {}\nexport = f;` → `function f() { }\nexport = f;`（`f` 是 value，`is_value_alias_declaration` true → 保留）。守护省略臂不误删 value `export =`。

> divergence/设计：(1) `import =` 外部模块形保留时直接返回原 `Some(node)`（同 namespace-import 臂），而非 Go 的 `VisitEachChild`——其子（名 + `require(...)`）无可省略 import 结构，等价。(2) `should_emit_alias_declaration`（既有，调 `is_referenced`）**未改**，新 import-equals 臂直接调 `is_referenced_alias_declaration` 以精确镜像 Go `shouldEmitAliasDeclaration→IsReferencedAliasDeclaration`；二者对 alias-symbol 声明等价，故既有 14 用例不受影响。(3) `ExportAssignment` 臂未按 `is_export_equals` 分支——Go 的 `KindExportAssignment` 对 `export =` 与 `export default` 同样处理（均经 `isValueAliasDeclaration`），本轮 scope 仅测 `export =`。

### scope / upstream 增长（6ah）

> **scope**：仅 `-p tsgo_transformers`。新增 `import_elision_visit` 的 `ImportEqualsDeclaration`/`ExportAssignment` 两臂 + `visit_import_equals_declaration`/`is_external_module_import_equals_declaration`（私有）。**ZERO** ast/printer/checker 增长——`import =`/`export =` 臂全走 arena 既有 data 读取 + checker 4ap 既有 `EmitResolver`（经 6ag 既有透传）。`new_import_elision_transformer` 签名未变。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`、README、root Cargo。
>
> **测试计数（6ah 新增）**：`tsgo_transformers` +4 `#[test]`（2 genuine RED→GREEN + 2 guard）+ 0 doctest → **209 unit + 37 doctest**（6ag 基线 205+37）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（证明公共 API additive）。

### DEFER（本轮确认的 blocked-by，importelision `import =`/`export =` 维度）

- **entity-name `import x = a.b`**：4ap 仍 DEFER `IsTopLevelValueImportEqualsWithEntityName`（Go `shouldEmitImportEqualsDeclaration` 的 `!IsExternalModule && isTopLevelValueImportEqualsWithEntityName` 分支）。本轮 `visit_import_equals_declaration` 的 else 分支保留原样。blocked-by checker `IsTopLevelValueImportEqualsWithEntityName`。
- **跨模块 re-export**（`import { x } from "m"; export = x` / `export { x }`）：target value-ness 需跨模块解析。blocked-by checker `resolveExternalModuleSymbol`/`getExportSymbolOfValueSymbolIfExported`。
- **value 位置 use 的 type-only-ness**、**`verbatimModuleSyntax`/`isolatedModules`/const-enum 策略变体**、`import "m";` attribute 访问。

## 6ai worklog — `moduletransforms/commonjsmodule` 作用域正确的 import use-site 重写（消费 4an `resolve_reference`）

> 本轮被 checker **4an** 解锁——`EmitResolver::resolve_reference(program, node) -> Option<SymbolId>`：把一个**值位置**标识符 use 解析到它引用的声明 symbol（沿作用域链外推，内层 shadow 优先）。落地长期 DEFER 的「作用域正确 use 重写」（6e/6e-2/6e-3/6v/6w 一直按名匹配）：CommonJS 下一个 imported binding 的 use 必须重写为 require-alias 上的限定成员访问，且**仅当**该 use 真的解析到该 import（非局部 / 非 shadow）。**未改 `internal/checker/**`**。

### resolve_reference 如何消费 / 接线（additive）

- `EmitReferenceResolver`（`lib.rs`）**additive 扩展**两个透传方法（未改既有签名 / 字段，复用 6af 的句柄）：
  - `resolve_reference(node) -> Option<SymbolId>`（委托 `EmitResolver::resolve_reference(program, node)`，Go `Checker.resolveName`/`getResolvedSymbol`）。
  - `symbol_of_declaration(node) -> Option<SymbolId>`（委托 `BoundProgram::symbol_of_node`，Go `getSymbolOfDeclaration`）——把每个 import binding 映射到其声明 symbol。
- **新 factory** `new_common_js_module_transformer_with_resolver(opt, resolver)`（additive 第二入参）——既有 `new_common_js_module_transformer(opt)` 签名**不变**，内部都委托 `build_common_js_module_transformer(opt, Option<resolver>)`。emitter 构造的入口不受影响（CJS 入口当前 Rust emitter 未构造，但仍按 6af 约定走 additive 新 factory，不改既有签名）。
- **与既有 require-alias 的关联**：`ImportBinding` 新增 `decl: Option<NodeId>`（命名 import = `ImportSpecifier` 节点；default = `ImportClause`；namespace = `NamespaceImport`）。substitution 阶段：先对每个 binding 用 `symbol_of_declaration(decl)` 取声明 symbol；再对每个 use 标识符 `resolve_reference(use)`，若解析到的 symbol 命中某 binding 的声明 symbol，则重写为该 binding 既有的 `require_var`/`member`（`m_1.x`），**复用既有 alias 方案**，不另造命名。无 resolver 时回退按名匹配（pre-6ai 行为）。
- **节点 id 对齐**：同 6af——`EmitReferenceResolver` 的 `BoundProgram` 自带独立 parse+bind 的 arena；parse 确定性使其节点 id 与 transform 读取的 `EmitContext` arena 一致，故 transform 的 use 节点 id 在 resolver 程序里解析为同一句法节点。

### red→green 切片（每片 input → emitted-shape；行为级过真实 CJS transform pipeline）

1. **`scoped_named_import_use_rewrites_to_member_access`（tracer，genuine RED）**：`import { x } from "m";\nx;`（commonjs）期望 `const m_1 = require("m");\nm_1.x;`。先把 resolver 臂留空（no-op）→ 红（use 仍是裸 `x`）→ 填入按名匹配（minimal）→ 绿。
2. **`scoped_shadowed_use_is_not_rewritten`（headline scope guard，genuine RED）**：`import { x } from "m";\nfunction f() {\n    var x = 1;\n    x;\n}` 期望内层 `x` **不**重写：`const m_1 = require("m");\nfunction f() {\n    var x = 1;\n    x;\n}`。按名匹配会把内层 `x` 误写成 `m_1.x` → 红 → 切换到 `resolve_reference`（内层 `x` 解析到局部 `var x`，symbol 不命中 import）→ 绿。证明真作用域解析 vs 按名匹配的价值。import 仍照常降级为 `const m_1 = require("m");`（CJS 不省略未用 import——那是 import-elision 的职责）。
3. **`scoped_import_use_inside_call_argument_is_rewritten`（coverage）**：`import { x } from "m";\nconsole.log(x);` → `const m_1 = require("m");\nconsole.log(m_1.x);`（调用参内重写；`console`/`log` 解析不到 import 故不动）。

### scope / upstream 增长（6ai）

> **scope**：仅 `-p tsgo_transformers`。新增 `new_common_js_module_transformer_with_resolver` + `build_common_js_module_transformer`（私有）+ `substitute_import_use`（私有，抽出既有 emit 逻辑）+ `ImportBinding.decl` 字段 + `EmitReferenceResolver` 两个 additive 透传方法（`resolve_reference`/`symbol_of_declaration`）。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器 + 既有 `set_node_substitution`；消费 checker 4an 既有 `EmitResolver::resolve_reference`。既有 `new_common_js_module_transformer` 签名未变。未触碰 `internal/ast/*`、`internal/printer/*`、`internal/checker/*`、任何 `.go`、README、root Cargo。
>
> **测试计数（6ai 新增）**：`tsgo_transformers` +3 `#[test]`（slice1 tracer + scope guard 均 genuine RED→GREEN + call-arg coverage）+ 3 doctest（`EmitReferenceResolver::resolve_reference` / `symbol_of_declaration` + `new_common_js_module_transformer_with_resolver`）→ **212 unit + 40 doctest**（6ah 基线 209+37）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（证明公共 API additive）。

### DEFER（本轮确认的 blocked-by，CJS use-site 重写维度）

- **default / namespace import 的 use 重写经 resolver**：本轮 resolver 臂对三类 binding 都走同一 symbol 匹配，但仅命名 import 有行为级测试；default/namespace 仍主要由按名回退（无 resolver 的既有测试）覆盖。→ **6aj 已补**（default/namespace 行为级 + scope guard 覆盖；见 6aj worklog）。
- **export 引用重写**（local `export { x }` 的 `exports.x` use-site）：blocked-by `GetReferencedExportContainer`（4an 未移植）。
- **shorthand-property-assignment 展开**（`{ x }` → `{ x: m_1.x }`）、string-literal 名 element-access 形（`m_1["x"]`）：blocked-by `markLinkedReferences` + 完整 `visitExpressionIdentifier` 形态。
- **ESM / System 的作用域正确引用重写**：独立轮次（同缺口，blocked-by resolver 接线进 esmodule/systemmodule）。

## 6aj worklog — `moduletransforms/commonjsmodule` default & namespace import use-site 重写（行为级覆盖，复用 6ai resolver 接线）

> 本轮目标：把 6ai 明确 DEFER 的 **default & namespace import 的作用域正确 use 重写行为级覆盖** 补齐。Go ground truth：`commonjsmodule.go:substituteExpressionIdentifier` / import-reference 替换——default-import binding 的 use → `<alias>.default`（interop 走 `__importDefault(require(...))`），namespace-import binding 的 use → 裸 alias（`__importStar(require(...))` 的 `m_1`）。

### 诚实的 red→green 记录（关键：本轮观察到 GREEN 而非 RED）

逐行为 test-first 推进，每片先写测试再 `cargo test -p tsgo_transformers --lib <name>` 观察：

1. **`scoped_default_import_use_rewrites_to_default_member`**：`import d from "m";\nd;`（commonjs + resolver）期望 `var __importDefault = …;\nconst m_1 = __importDefault(require("m"));\nm_1.default;`。写完 → **直接 GREEN**。
2. **`scoped_namespace_import_use_rewrites_to_bare_alias`**：`import * as ns from "m";\nns;` 期望 `const m_1 = __importStar(require("m"));\nm_1;`。写完 → **直接 GREEN**。
3. **`scoped_shadowed_default_import_use_is_not_rewritten`**（headline scope guard，default 形）：`import d from "m";\nfunction f() {\n    var d = 1;\n    d;\n}` 期望内层 `d` **保持裸**（resolve 到局部 `var d`，symbol 不命中 import），import 仍降级为 `const m_1 = __importDefault(require("m"));`。写完 → **直接 GREEN**。
4. **`scoped_shadowed_namespace_import_use_is_not_rewritten`**（scope guard 镜像，namespace 形）：`import * as ns from "m";\nfunction f() {\n    var ns = 1;\n    ns;\n}` → 内层 `ns` 保持裸。写完 → **直接 GREEN**。

**为何没看到 RED（诚实说明）**：6ai 落地 resolver 臂时已把 `register_import_use_substitutions` 的匹配写成**对 `ImportBinding.decl` 泛化**——named=`ImportSpecifier`、default=`ImportClause`、namespace=`NamespaceImport` 三类 binding 都在 `lower_import_to_require` 里设了 `decl`，substitution 阶段统一用 `symbol_of_declaration(decl)` 取声明 symbol、再与 `resolve_reference(use)` 比对。故 default/namespace 的 resolver-path 行为**早在 6ai 就已正确**，只是缺行为级测试（6ai DEFER 明列）。本轮属于"补齐 DEFER 的行为覆盖"，**无 impl 变更**。我不伪造 RED（破坏再修是 TDD 文档明禁的剧场行为）。

**测试非空泛的硬证据**：slice 1（模块级 `d` → `m_1.default`，发生重写）证明 resolver 把 `d` 解析到 import；slice 3（内层 `d` 保持裸）证明 resolver 作用域正确。二者合证 resolver-path 真在工作且作用域正确——若 resolver 返回 None，slice 1 会红（`d` 不重写）；若按名匹配，slice 3 会红（内层 `d` 误写成 `m_1.default`）。两片同绿即排除这两种退化。

### scope / upstream 增长（6aj）

> **scope**：仅 `-p tsgo_transformers`。**ZERO** ast/printer/checker 增长，**ZERO** impl 变更——复用 6ai `new_common_js_module_transformer_with_resolver` + `ImportBinding.decl` 字段 + `register_import_use_substitutions` 的 resolver 臂 + 既有 `set_node_substitution`；消费 checker 4an 既有 `EmitResolver::resolve_reference`。仅新增 4 个 `#[test]` + 更新 `commonjsmodule.rs` 模块 doc（把 default/namespace 从"仅按名回退覆盖"更新为"6aj resolver 行为级覆盖含 scope guard"）。既有公共 fn 签名全未变。未触碰 `internal/checker/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6aj 新增）**：`tsgo_transformers` +4 `#[test]`（default use / namespace use / default scope guard / namespace scope guard，全 test-first 但观察到直接 GREEN——见上"诚实说明"）+ 0 doctest（无新公共 API）→ **216 unit + 40 doctest**（6ai 基线 212+40）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（证明公共 API additive）。

### DEFER（本轮确认的 blocked-by，CJS use-site 重写维度）

- **export 引用重写**（local `export { x }` 的 `exports.x` use-site）：blocked-by `GetReferencedExportContainer`（4an 未移植）。
- **shorthand-property-assignment 展开**（`{ x }` → `{ x: m_1.x }`）、string-literal 名 element-access 形（`m_1["x"]`）：blocked-by `markLinkedReferences` + 完整 `visitExpressionIdentifier` 形态。
- **combined default+named interop 边角**（超出可达最小面）、**ESM / System 的作用域正确引用重写**：独立轮次（同缺口，blocked-by resolver 接线进 esmodule/systemmodule）。

## 6ak worklog — `moduletransforms/commonjsmodule` CommonJS local-export use-site 重写（消费 4as `get_referenced_export_container`）

> 本轮目标：把 6ai/6aj 明确 DEFER 的 **local-export 引用重写**（`export const x = 1; x;` 的 use `x;` → `exports.x;`）补齐，由 checker 4as 落地的 `EmitResolver::get_referenced_export_container(program, node, prefix_locals) -> Option<NodeId>` 解锁。Go ground truth：`commonjsmodule.go:visitExpressionIdentifier`——当 `GetReferencedExportContainer` 返回 source file 时，identifier use 变成 `exports.<name>`（`exports` 对象上的 property access）。CJS 传 `prefix_locals = false`，故只有顶层**导出变量**被重写；exported function/class（`ExportHasLocal && !Variable`）、shadowed、非导出 use 都保持裸。

### get_referenced_export_container 的消费 / 接线

- **checker 侧（4as，as-is 消费，未改）**：`internal/checker/core/emit_resolver.rs:get_referenced_export_container`——以 `ExportValue | Value | Alias` meaning resolve use；若解析到带 `ExportValue` flag 的 phantom local，跟 `export_symbol` 到真正的 export symbol；`!prefix_locals && ExportHasLocal && !Variable` → 返回 None（exported fn/class 不前缀）；最后取 `parent`，当 parent 是 `ValueModule` 且 `value_declaration` 是 SourceFile 时返回该 SourceFile（否则 None）。
- **transformers 侧（本轮）**：`EmitReferenceResolver` 加 **additive** `get_referenced_export_container(node, prefix_locals) -> Option<NodeId>` passthrough，委托给 `EmitResolver::get_referenced_export_container(self.program.as_ref(), node, prefix_locals)`。
- **use-rewrite seam**：扩展 6ai 的 `register_import_use_substitutions` 的 resolver 臂——每个 use 标识符**先**查 `get_referenced_export_container(use, false)`，若返回 `Some(container)` 且 `arena.kind(container) == SourceFile`，则用新 helper `substitute_exported_name_use` 重写为 `exports.<name>`（复用既有 `exports` 标识符 + `new_property_access_expression`，不另造命名）并 `continue`；否则回落到既有 import-binding `resolve_reference` 匹配。顺序镜像 Go `visitExpressionIdentifier`（先 export container，后 import declaration）。
- **为何 import use 不会误命中 export container**：import alias symbol 在 binder 里以 `parent=None` 进 module locals（非 export-context），故 `get_referenced_export_container` 在 `parent_symbol?` 处短路返回 None；既有 import 重写测试（`m_1.x`/`m_1.default`/`m_1`）全绿即验证此点未回归。

### 诚实的 red→green 切片记录

逐行为 test-first，每片先写测试再 `cargo test -p tsgo_transformers <name>` 观察：

1. **`scoped_exported_variable_use_rewrites_to_exports_access`**（**genuine RED→GREEN**）：`export const x = 1;\nx;`（commonjs + resolver）期望 `Object.defineProperty(exports, "__esModule", { value: true });\nexports.x = void 0;\nexports.x = 1;\nexports.x;`。先观察 RED——实测 `left: "…exports.x = 1;\nx;"`（use 裸 `x;`，声明降级已正确 `exports.x = void 0;\nexports.x = 1;` 证明不回归 6e/6w）。加 passthrough + substitution 臂 → GREEN。
2. **`scoped_export_use_shadowed_by_inner_local_is_not_rewritten`**（scope guard，**green-on-arrival**）：`export const x = 1;\nfunction f() {\n    const x = 2;\n    x;\n}` → 内层 `x` 保持裸（resolve 到局部 `const x`，container=None），仅声明降级为 `exports.x`。写完直接 GREEN。
3. **`scoped_non_exported_local_use_stays_bare`**（non-export guard，**green-on-arrival**）：`const y = 1;\ny;` → `y` 保持裸（无 export container）、无 `__esModule` marker。写完直接 GREEN。

**为何 2/3 green-on-arrival（诚实说明）**：slice 2/3 只断言 use **保持裸**——在 slice 1 impl 之前 export-container 重写根本不存在（use 永远裸），slice 1 impl 之后 resolver 对 shadowed/非导出 use 返回 None 故仍裸。它们是 slice 1 重写**作用域正确性**的回归守卫（pin 住"只有顶层导出变量被前缀"），非 slice 1 的依赖。不伪造 RED（破坏再修是 TDD 文档明禁）。

**测试非空泛的硬证据**：slice 1（顶层 `x` 真被重写成 `exports.x`）证明 resolver 把 use 解析到 export container；slice 2（内层 `x` 保持裸）证明作用域正确——若按名匹配，slice 2 内层 `x` 会误写成 `exports.x`（红）；若 slice 1 的 resolver 返回 None，slice 1 会红。三片同绿排除这两种退化。

### scope / upstream 增长（6ak）

> **scope**：仅 `-p tsgo_transformers`。**ZERO** ast/printer/checker 增长——消费 checker 4as `get_referenced_export_container` as-is；复用 6ai `new_common_js_module_transformer_with_resolver` + `register_import_use_substitutions` resolver 臂 + 既有 `exports` 标识符构造 + `new_property_access_expression` + `set_node_substitution`。impl 变更仅：`EmitReferenceResolver` 加 1 个 additive passthrough fn + `commonjsmodule.rs` 加 1 个私有 helper `substitute_exported_name_use` + resolver 臂内插入 export-container 检查。既有公共 fn 签名（含 CJS 构造器）全未变。未触碰 `internal/checker/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6ak 新增）**：`tsgo_transformers` +3 `#[test]`（exported-var use genuine RED→GREEN + 2 scope/non-export guard green-on-arrival）+ 1 doctest（`EmitReferenceResolver::get_referenced_export_container`）→ **219 unit + 41 doctest**（6aj 基线 216+40）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（证明公共 API additive）。

### DEFER（本轮确认的 blocked-by，CJS export use-site 重写维度）

- **exported function/class use-site**：Go 的 `ExportHasLocal && !Variable` 守卫使非变量导出 use 返回 None（不前缀，按 runtime local 裸引用）——4as 已实现此守卫，故本轮天然排除，无需额外代码。
- **namespace/enum export container**（`ModuleDeclaration`/`EnumDeclaration` 容器）、**跨模块 UMD-export**（`symbolFile != referenceFile`）：blocked-by checker 4as DEFER（namespace/enum 容器解析 + `compiler.Program` P6）。
- **shorthand-property-assignment 展开**（`{ x }` → `{ x: exports.x }`）、`IsExportName`/`IsLocalName`/auto-generate-name 的完整 gating：blocked-by 完整 `visitExpressionIdentifier` 形态 + emit-flag 接线。
- **ESM / System 的 export 引用重写**：独立轮次（resolver 接线进 esmodule/systemmodule）。

## 6al worklog — `tstransforms/legacydecorators`（`--experimentalDecorators` 首切片 — 属性装饰器 + `design:type` 元数据，消费 4at `serialize_type_node_for_metadata`）

> 本轮目标：落地 P5 round 6al 的**首个端到端可观察切片**——legacy（实验性）装饰器 transform 的最小垂直切片。选定**属性装饰器 + `design:type` 元数据**形态，因为它正是行使 checker 4at `EmitResolver::serialize_type_node_for_metadata`（`: T` 注解 → 运行时构造器枚举 `SerializedTypeNode`）的最小路径。Go ground truth：`legacydecorators.go`（`visitClassDeclaration`/`transformClassDeclarationWithoutClassDecorators`/`generateClassElementDecorationExpression`/`getClassMemberPrefix`/`getExpressionForPropertyName`/`visitPropertyDeclaration`）+ `metadata.go`（`injectClassElementTypeMetadata`/`getOldTypeMetadata`/`shouldAddTypeMetadata`）+ `typeserializer.go`（`serializeTypeOfNode`/`serializeTypeNode`）+ `printer/factory.go`（`NewDecorateHelper`/`NewMetadataHelper`）+ `printer/helpers.go`（`decorateHelper`/`metadataHelper`，priority 2/3）。shape 对 `tsc --experimentalDecorators [--emitDecoratorMetadata]` 核对。

### 管线折叠（受认可的对 Go 两-transformer 的偏离）

Go 用两个串联 transformer：`MetadataTransformer` 先把合成 `@__metadata("design:type", T)` 装饰器注入被装饰成员的 modifier list，再由 `LegacyDecoratorsTransformer` 用 `isSyntheticMetadataDecorator` 分组、把全部装饰器（真 + 注入）收进 `__decorate([...])` 数组。本端口把两者**折成一遍**：`generate_class_element_decoration_statement` 直接构造装饰器表达式列表 `[<真装饰器>, <metadata>]`（metadata 居末，同 Go `transformAllDecoratorsOfDeclaration` 的顺序）。emit 文本与两-transformer 形态**逐字节一致**。理由：① 切片最小化；② 合成-装饰器往返纯属中间表示，对可观察输出无影响。已在模块 doc 写明此折叠。

### 关键形态确认（vs Go / tsc）

- **打印器单行约束**：装饰器数组 `[dec, __metadata("design:type", Number)]` 由 Rust printer 恒单行（`emit_expressions.rs` 硬编码 array 单行），Go/tsc 多行。故 expected 取单行形（同 6ae 的 array/object 内联约定）。
- **helper 定义位置**：`__decorate`/`__metadata` helper 在本 crate `legacydecorators.rs` 内以 `pub static EmitHelper` 定义（printer crate 不在编辑范围，同 `spread.rs` 的 `SPREAD_ARRAY_HELPER`/`forawait.rs`），text/`import_name`/`priority` verbatim 自 Go `helpers.go`。priority `__decorate`=2 < `__metadata`=3 → prologue 中 decorate 定义先于 metadata（printer 按 priority 排序）。
- **`void 0` 描述符**：属性（非 accessor）→ `__decorate(..., "x", void 0)`（Go `NewVoidZeroExpression`，使 `__decorate` 直接 `Object.defineProperty`）。
- **成员名**：identifier 名 → 字符串字面量 `"x"`（Go `getExpressionForPropertyName`）。
- **前缀**：instance → `C.prototype`；static → `C`（Go `getClassMemberPrefix` 的 `IsStatic` 分支）。
- **类型剥离**：`visitPropertyDeclaration` 重建属性时丢 type/postfix（Go 传 `nil`），故 `@dec x: number;` → `x;`（即便无 typeeraser 串联）。

### 诚实的 red→green 切片记录

逐行为 test-first，每片先写测试再 `cargo test -p tsgo_transformers legacydecorators` 观察：

1. **`instance_property_decorator_lowers_to_decorate_call`**（tracer，**genuine RED→GREEN**）：`class C { @dec x: number; }`（experimentalDecorators，无 metadata）期望 `<__decorate prologue>\nclass C {\n    x;\n}\n__decorate([dec], C.prototype, "x", void 0);`。先以恒等 skeleton 观察 RED——实测 `left: "class C {\n    @dec\n    x: number;\n}"`（装饰器+类型原样）。实现 `visit_class_declaration`（剥装饰器/类型 + 尾随 `__decorate` 语句 + SyntaxList 展开 + `__decorate` helper 注册）→ GREEN。证明 `__decorate` helper 注册 + 属性装饰器降级端到端跑通。
2. **`property_decorator_emits_design_type_metadata`**（headline，**genuine RED→GREEN，消费 checker 4at**）：同输入 + `--emitDecoratorMetadata` + resolver 期望追加 `__metadata("design:type", Number)`（`__metadata` prologue）。slice 1 后观察 RED——实测 `left` 缺 metadata（`__decorate([dec], …)`、无 `__metadata` 定义）。实现 metadata 臂（`emit_decorator_metadata && resolver.is_some()` 时，经 `EmitReferenceResolver::serialize_type_node_for_metadata(type_node)` 取 `SerializedTypeNode`，映射成 AST 表达式，`new_metadata_helper` 包成 `__metadata("design:type", …)` 并 push 到装饰器列表末）→ GREEN。**证明 checker 4at 集成**（`Number` 来自 checker，非硬编码）。
3. **`static_property_decorator_uses_class_name_prefix`**（**genuine RED→GREEN**）：`class C { @dec static x: number; }` 期望前缀 `C`（非 `C.prototype`）。slice 1/2 硬编码 `C.prototype`，故观察 RED——实测 `left: "…C.prototype, "x"…"`。实现 `is_static_member` + 前缀分支 → GREEN。
4. **`string_typed_property_serializes_to_string_constructor`**（coverage，**green-on-arrival**）：`: string` → `String`。slice 2 的 `serialized_type_to_expression` 已映射全 `SerializedTypeNode` 枚举，故写完直接 GREEN——泛化守卫（pin 住 serializer 非 `Number`-only）。
5. **`type_reference_property_serializes_to_object_fallback`**（DEFER 守卫，**green-on-arrival**）：`: D`（TypeReference）→ `Object`（checker 4at 把 TypeReference 臂 DEFER 到 `Object` 尾）。文档化 4at 边界。
6. **`without_experimental_decorators_class_is_unchanged`**（gate 守卫，**green-on-arrival**）：experimentalDecorators off → 装饰类原样透传（无 `__decorate`）。
7. **`class_decorator_is_left_unchanged`**（DEFER 守卫，**green-on-arrival**）：`@dec class C {}` → 原样透传（class 装饰器包裹 DEFER）。

**为何 4–7 green-on-arrival（诚实说明）**：4/5 是 slice 2 metadata 映射的泛化/边界守卫（slice 2 impl 已覆盖全枚举与 `Object` 尾），6/7 是 gate/DEFER 守卫（实现里本就 gate-off/passthrough）。它们 pin 住正确性（serializer 泛化、option gate、class-装饰器不误降级），非各自的 RED 依赖。不伪造 RED（破坏再修是 TDD 文档明禁）。**硬证据**：slice 2（`Number` 真来自 checker）+ slice 5（`TypeReference`→`Object`）同绿排除"硬编码 `Number`"；slice 3（static→`C`）+ slice 1（instance→`C.prototype`）同绿排除"前缀写死"。

### scope / upstream 增长（6al）

> **scope**：仅 `-p tsgo_transformers`。**ZERO** ast/printer/checker 增长——消费 checker 4at `serialize_type_node_for_metadata` as-is；走 arena 既有构造器（`new_class_like`/`new_property_declaration`/`new_property_access_expression`/`new_string_literal`/`new_call_expression`/`new_array_literal_expression`/`new_void_expression`/`new_expression_statement`/`new_syntax_list`）+ 6d-2 helper infra（`request_emit_helper`/`read_emit_helpers`/`add_emit_helper` + `factory().new_unscoped_helper_name`）+ `ast::modifier_to_flag`。impl 变更仅：`EmitReferenceResolver` 加 1 个 additive passthrough `serialize_type_node_for_metadata` + 新文件 `tstransforms/legacydecorators.rs`（+ `pub mod legacydecorators;`）含 `pub static DECORATE_HELPER/METADATA_HELPER` + 2 个 additive 工厂 `new_legacy_decorators_transformer` / `new_legacy_decorators_transformer_with_resolver`。**未改任何既有公共 fn 签名**；新工厂 standalone-only（未接入 compiler 管线，同 importElision），故 `cargo build -p tsgo_compiler` 不需改 emitter 的 exhaustive 构造。未触碰 `internal/checker/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6al 新增）**：`tsgo_transformers` +7 `#[test]`（slice1/2/3 genuine RED→GREEN + 4 coverage/guard green-on-arrival）+ 3 doctest（`EmitReferenceResolver::serialize_type_node_for_metadata` + 两个工厂）→ **226 unit + 44 doctest**（6ak 基线 219+41）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净（仅改动文件）；`cargo build -p tsgo_compiler` 绿（证明公共 API additive）。

### DEFER（本轮确认的 blocked-by，legacy 装饰器维度）

- **class 装饰器** `@dec class C {}` → `let C = class C {}; C = __decorate([dec], C);` 包裹（含自引用 class-alias 改写、`export`/`default` 后置 export 语句）。blocked-by：`GetLocalName`/`GetDeclarationName` emit-name 形 + `classAliases` substitution（`getReferencedValueDeclaration`）+ `let`-binding 包裹。
- **method/accessor 装饰器**：`design:type = Function`（硬编码，无 checker）、`design:returntype`、accessor 的 `getAllAccessorDeclarations` 合并。blocked-by：method/accessor 装饰形态 + `serializeReturnTypeOfNode`。
- **参数装饰器** `__param(i, dec)` + `design:paramtypes`（`[Object, …]`）。blocked-by：`getDecoratorsOfParameters` + `serializeParameterTypesOfNode`。
- **`TypeReference` `design:type`**（`: Date`/class → 实体构造器）：checker 4at 把 TypeReference 臂 DEFER 到 `Object`，故本轮 `: D` → `Object`。blocked-by：checker `GetTypeReferenceSerializationKind`（实体 value-ness + `printer.TypeReferenceSerializationKind`）+ `serializeTypeNode` 递归（union/intersection/conditional/`FunctionType`→`Function`/`ArrayType`→`Array`/字面量类型臂）。
- **计算属性名**（`@dec [k]: T`）：需 Go `pendingExpressions` 内联 + temp 缓存。blocked-by：`getPropertyNameExpressionIfNeeded`。
- **混合 instance+static 装饰成员的语句顺序**（Go instance-pass 先于 static-pass）：本轮按源序逐成员发 `__decorate`，单成员切片无差异；混合类待补。
- **装饰器表达式求值序边角**、**重载上的装饰器**、`emitDecoratorMetadata` 的 `design:type=Function`（method/class，硬编码无 checker——区别于本轮 6am 落地的**属性**的 `FunctionType` 注解 → `Function`）。

## 6am worklog — `tstransforms/legacydecorators` `serialized_type_to_expression` `Array`/`Function` 臂（协调跨-crate lane：transformers 6am + checker 4av）

> 本轮目标：解锁 6al/4au 的头号 DEFER——`design:type` 为 `Array`（数组/元组类型）/`Function`（函数/构造类型）。这两组臂需要 checker 新加 `SerializedTypeNode::Array`/`Function` 变体，而该变体会破坏本 crate `serialized_type_to_expression` 的**无-wildcard 穷尽 match**（4au 实测 `cargo build -p tsgo_compiler` E0004）。故本轮是一个**协调跨-crate lane**：同时拥有 `internal/checker/**`（加变体 + checker 臂，见 checker 4av）与 `internal/transformers/**`（加 `serialized_type_to_expression` 对应臂 + 端到端测试）。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。

### 关键纪律：workspace 在行为之间始终可构建

每个变体：**先在 checker 加枚举变体 → 立即在本 crate `serialized_type_to_expression` 加对应 match 臂**（保持穷尽，`tsgo_transformers`/`tsgo_compiler` 不进入非编译态）→ 再观察行为红→绿。transformer 臂在 checker 发射新变体之前**不可达**（纯穷尽-match 构建占位），故它不是"提前实现"——端到端翻绿的最小代码是 checker 臂（见下"诚实 RED"）。

### 关键形态确认（vs Go / tsc）

- Go `typeserializer.go:serializeTypeNode`：`case KindArrayType, KindTupleType -> NewIdentifier("Array")`、`case KindFunctionType, KindConstructorType -> NewIdentifier("Function")`。发射标识符 `Array`/`Function` 对 `tsc --experimentalDecorators --emitDecoratorMetadata` 核对。
- 类型注解被 `visitPropertyDeclaration` 剥离（属性体仅 `x;`），`design:type` 取自**原始** type-node（经 resolver 在 bound program 解析，node id 对齐）；故 `@dec x: number[];` → `class C { x; }` + `__metadata("design:type", Array)`。
- 数组 `[dec, __metadata(...)]` 恒单行（Rust printer 约束，同 6al）。

### 诚实的 red→green 切片记录

逐行为 test-first（端到端经真 transform 管线 emit 文本 + checker 经 bound program），每片先写测试再 `cargo test -p tsgo_transformers <name>` / `cargo test -p tsgo_checker <name>` 观察：

1. **`array_typed_property_serializes_to_array_constructor`**（**genuine RED→GREEN，协调 checker 4av**）：加 `SerializedTypeNode::Array` 变体 + 本 crate `SerializedTypeNode::Array => new_identifier("Array")` 臂（保持构建）。在**未加 checker `ArrayType` 臂**的窗口观察 RED——实测 `class C { @dec x: number[]; }`+meta 发射 `__metadata("design:type", Object)`（checker 仍落 `_ => Object`）。checker 加 `Kind::ArrayType => SerializedTypeNode::Array` → GREEN（`Array`）。**证明协调**：`Array` 来自 checker 新臂，本 crate 臂仅映射。
2. **`tuple_typed_property_serializes_to_array_constructor`**（**genuine RED→GREEN**）：`@dec x: [number, string]` → `Array`。在 checker `TupleType` 未并入 group 时观察 RED（发射 `Object`）→ checker 扩 `Kind::ArrayType | Kind::TupleType` → GREEN。
3. **`function_typed_property_serializes_to_function_constructor`**（**genuine RED→GREEN，协调 checker 4av**）：加 `SerializedTypeNode::Function` 变体 + 本 crate `Function => new_identifier("Function")` 臂。在未加 checker `FunctionType` 臂的窗口观察 RED（`@dec x: () => void` 发射 `Object`）→ checker 加 `Kind::FunctionType => SerializedTypeNode::Function` → GREEN（`Function`）。ConstructorType 的 checker 侧补充（`: new () => C` → `Function`）见 checker 4av S4（本 crate 无需另增臂，复用同一 `Function` 映射）。

> **为何全 genuine RED（诚实说明）**：每个端到端切片都在"transformer 臂已加（构建绿）但 checker 臂未加"的窗口实测看到 `__metadata("design:type", Object)`（期望 `Array`/`Function`），随后 checker 单处臂翻绿。不伪造 RED；transformer 臂是穷尽-match 的构建必需占位，翻绿的最小代码是 checker 臂。**硬证据**：array（slice 1）+ function（slice 3）同绿但发射不同标识符（`Array` vs `Function`），排除"硬编码"。

### scope / upstream 增长（6am）

> **scope**：`-p tsgo_transformers` + `-p tsgo_checker`（协调 lane，两 crate 同拥）。本 crate 改动仅 `serialized_type_to_expression` 加 2 臂（`Array`/`Function` → `new_identifier`）+ `legacydecorators_test.rs` +3 端到端测试；checker 改动见 4av（加 2 枚举变体 + 2 match 臂 + 4 测试）。**未改任何既有公共 fn 签名**；新工厂仍 standalone-only。未触碰 `internal/lsp/lsproto/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6am 新增）**：`tsgo_transformers` +3 `#[test]`（array/tuple/function 端到端，genuine RED→GREEN）+ 0 doctest → **229 unit + 44 doctest**（6al 基线 226+44）。`cargo test -p tsgo_transformers` 全绿；`cargo test -p tsgo_checker` 全绿（343+132）；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` / `cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿（**确认协调枚举变更跨 workspace 一致**——本 crate 穷尽 match 现覆盖 `Array`/`Function`）。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

### DEFER（本轮确认的 blocked-by，6am 维度）

- **`TypeReference` `design:type`**（`: Date`/class → 实体构造器）：checker 仍把 `TypeReference` 臂落 `Object`（4av 未动）。blocked-by：checker `GetTypeReferenceSerializationKind`（实体 value-ness + `printer.TypeReferenceSerializationKind`）。
- **union/intersection/conditional/`TypePredicate`** 注解 → 落 `Object`。blocked-by：checker `serializeTypeNode` 递归（4av DEFER）。
- **方法/访问器装饰器**：`design:type=Function`（硬编码，无 checker）、`design:returntype`、`design:paramtypes`（`__param`）。blocked-by：method/accessor/参数装饰形态 + `serializeReturnTypeOfNode`/`serializeParameterTypesOfNode`。
- **class 装饰器包裹**、**计算属性名**、**混合 instance/static 语句序**：同 6al DEFER。

## 6an worklog — `tstransforms/legacydecorators` `TypeReference` `design:type` 实体标识符发射（协调跨-crate lane：transformers 6an，消费 checker 4aw）

> 本轮目标：落地 6al/6am/4aw 共同的头号 DEFER——`design:type` 为 `TypeReference` 时按 checker 4aw `get_type_reference_serialization_kind` 的分类发射（class-typed → 类标识符本身；interface/type-alias/unresolved → `Object`）。Go ground truth：`typeserializer.go:serializeTypeNode` 的 `case KindTypeReference: return s.serializeTypeReferenceNode(node)` → `serializeTypeReferenceNode` 调 `resolver.GetTypeReferenceSerializationKind` 后 switch（`TypeWithConstructSignatureAndValue` → `serializeEntityNameAsExpression(node.TypeName)`；`ObjectType`/`Unknown` 可达臂 → `Object`；primitive/void/array/symbol/function/promise → 对应构造器/`void 0`）。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。

### `&mut Checker` 线程如何解（关键设计）

checker 4aw 的 `get_type_reference_serialization_kind(&self, checker: &mut Checker, program, type_node)` 需要 `&mut Checker`（为 `get_declared_type_of_symbol` 建声明类型）。transformer 经 `EmitReferenceResolver`（持 `Rc<dyn BoundProgram>` + 零大小 `EmitResolver`，全 `&self` 方法）消费。本轮按推荐方案：**给 `EmitReferenceResolver` 加一个 `checker: Rc<RefCell<Checker>>` 字段**（`new()` 内 `Checker::new()` 构造——`new()` 仅被测试 helper `build_reference_resolver` 调用，compiler crate 不构造它，故改 `new()` 体纯加法），并加一个加法式透传 `get_type_reference_serialization_kind(&self, type_node) -> TypeReferenceSerializationKind`，内部 `self.checker.borrow_mut()` 取 `&mut Checker` 再委托 4aw 方法。保留 4aw 签名不变（checker 侧本轮零改动）；`EmitReferenceResolver::new` 签名不变（resolver 仍同句柄，只多了内部字段 + 新方法）；`#[derive(Clone)]` 仍成立（`Rc<RefCell<..>>` 是 `Clone`，克隆共享同一 checker 缓存）。`EmitResolver` 零大小，故 `resolver`（由 throwaway checker 建）与新 owned `checker` 互不耦合，与 checker 测试"resolver 来自一个 checker、方法传另一个 `&mut c`"同构。

### 诚实的 red→green 切片记录

逐行为 test-first，每片先写测试再 `cargo test -p tsgo_transformers <name>` 观察（端到端经真 transform 管线 emit 文本）：

1. **`class_typed_property_serializes_to_entity_identifier`**（headline，**genuine RED→GREEN，消费 checker 4aw**）：`class C {}\nclass D { @dec x: C; }`+meta 期望 `…__metadata("design:type", C)…`。在未接线窗口观察 RED——实测发射 `__metadata("design:type", Object)`（`: C` 是 `TypeReference`，旧路径走 checker `serialize_type_node_for_metadata` 的 `Object` 尾）；唯一差异 `Object` vs `C`，其余结构（两 class、`D.prototype`）逐字节一致。实现：`EmitReferenceResolver` 加 `checker` 字段 + `get_type_reference_serialization_kind` 透传；transformer 加 `serialize_type_node`（`KindTypeReference` 分流）+ `serialize_type_reference_node`（switch on kind）+ `serialize_entity_name_as_expression`（identifier→`new_identifier(text)`）；`generate_class_element_decoration_statement` 改调 `serialize_type_node` → GREEN（`C`）。**证明 checker 4aw 集成**（`C` 来自 4aw 分类 + 实体名，非硬编码）。
2. **`interface_typed_property_serializes_to_object`**（**green-on-arrival**）：`interface I {}\nclass D { @dec x: I; }` → `Object`。`I` 仅 type-meaning → 4aw `ObjectType` → 本轮 `ObjectType => "Object"` 臂。behavior 1 实现后直接绿——`ObjectType` 臂随 switch 一并落地。守卫：type-only 引用**不**误发为自身标识符（排除"TypeReference 一律发实体名"）。注：`interface I {}` 原样透传（本 transformer 隔离运行、未串 type-eraser，full 管线会先擦除），断言取 `design:type` 值。
3. **`unresolved_type_reference_property_serializes_to_object`**（**green-on-arrival**）：`class D { @dec x: Missing; }` → `Object`。`Missing` 无声明（无 lib globals）→ 4aw `Unknown` → 本轮 `Unknown => "Object"` 臂（Go 完整形发 `typeof`-条件守卫，可达端口落 `Object` 尾，即 Go `serializingConditionalTypeBranch` 结果）。behavior 1 实现后直接绿。

> **为何 2/3 green-on-arrival（诚实说明）**：`serialize_type_reference_node` 是对 Go switch 的 1:1 端口，behavior 1 落地时为保持对 `TypeReferenceSerializationKind`（12 变体）的穷尽 match，`ObjectType`/`Unknown` 臂随 headline 一并写入（与 6am"穷尽-match 构建占位"同口径）。2/3 pin 住这两个可达分类映射正确（type-only/unresolved **不**发实体名），非各自 RED 依赖。不伪造 RED。**硬证据**：behavior 1（class→`C`）与 2/3（→`Object`）同绿但发射不同，排除"硬编码 `C`"或"TypeReference 一律 `Object`"。

### 穷尽 match 的不可达臂（faithful-but-unreachable，同 6am 口径）

`serialize_type_reference_node` 的 `VoidNullableOrNeverType`/`NumberLikeType`/`BigIntLikeType`/`StringLikeType`/`BooleanType`/`ArrayLikeType`/`ESSymbolType`/`TypeWithCallSignature`/`Promise` 臂 1:1 镜像 Go switch（发 `void 0`/`Number`/`BigInt`/`String`/`Boolean`/`Array`/`Symbol`/`Function`/`Promise`），但 checker 4aw 把这些 lib-globals 类全 DEFER 到 `ObjectType` 尾，故它们**当前不可达**（穷尽 match 的构建必需臂，非提前实现的行为）。待 checker 扩 lib globals（P6）后自然解锁。

### scope / upstream 增长（6an）

> **scope**：仅 `-p tsgo_transformers`（checker 侧本轮零改动——4aw 的 `get_type_reference_serialization_kind` + `TypeReferenceSerializationKind` 已就绪，直接消费）。本 crate 改动：`lib.rs` 的 `EmitReferenceResolver` 加 `checker: Rc<RefCell<Checker>>` 字段 + `get_type_reference_serialization_kind` 透传（加法）；`legacydecorators.rs` 加 `serialize_type_node`/`serialize_type_reference_node`/`serialize_entity_name_as_expression` + `generate_class_element_decoration_statement` 改调点 + 模块 doc 更新；`legacydecorators_test.rs` 替 1 个旧 DEFER 守卫（`type_reference_property_serializes_to_object_fallback`）为 behavior 1，+2 端到端测试（behavior 2/3）。**ZERO** ast/printer/checker 增长——走 arena 既有构造器（`new_identifier`）+ 既有 `make_void_zero`。**未改任何既有公共 fn 签名**（`EmitReferenceResolver::new` / 两工厂签名不变）。未触碰 `internal/lsp/lsproto/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6an 新增）**：`tsgo_transformers` net +2 `#[test]`（替 1 + 加 2：behavior 1 genuine RED→GREEN，2/3 green-on-arrival）+ 1 doctest（`EmitReferenceResolver::get_type_reference_serialization_kind`）→ **231 unit + 45 doctest**（6am 基线 229+44）。`tsgo_checker` 不变（347 unit + 134 doctest，本轮零改动）。`cargo test -p tsgo_transformers` 全绿；`cargo test -p tsgo_checker` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo clippy -p tsgo_checker --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` / `cargo fmt -p tsgo_checker -- --check` 干净；`cargo build -p tsgo_compiler` 绿（确认公共 API 加法）。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

### DEFER（本轮确认的 blocked-by，6an 维度）

- **`TypeReference` 的 lib-globals 类**（`: Promise<T>` → `Promise`、`: number`-via-alias → `Number` 等、`VoidNullableOrNeverType`/`ArrayLikeType`/`ESSymbolType`/`TypeWithCallSignature`）：transformer 臂已 1:1 镜像 Go switch（构建完备），但 checker 4aw 把这些类 DEFER 到 `ObjectType`，故不可达。blocked-by：checker resolved global lib types + 构造/调用签名收集（P6）。
- **`Unknown` 的 `typeof`-条件守卫**（Go：非-conditional-branch 时发 `typeof (_a = A) === "function" ? _a : Object`）：可达端口落 `Object` 尾。blocked-by：`NewTempVariable` + `AddVariableDeclaration`（emit-context temp 变量）+ `serializingConditionalTypeBranch` 上下文。
- **qualified-name 实体**（`: A.B` → `serializeQualifiedNameAsExpression` 属性访问链）：checker 4aw 把 qualified-name 落 `Unknown`，且 `serialize_entity_name_as_expression` 仅 identifier 臂。blocked-by：checker qualified-name `resolveEntityName` + namespace 解析。
- **union/intersection/conditional/`TypePredicate`** 注解、**方法/访问器/参数装饰器**、**class 装饰器包裹**、**计算名**、**混合 instance/static 语句序**：同 6al/6am DEFER。

## 6ao worklog — `tstransforms/legacydecorators` **方法装饰器** lowering + `design:type=Function`/`design:returntype`（transformers-only，消费既有 checker API）

> 本轮目标：落地 6al/6am/6an 共同 DEFER 的头号项——**装饰的方法成员**（`@dec m() {}`）的 lowering。Go ground truth：`legacydecorators.go`（`visitMethodDeclaration` 剥装饰器/类型参数/返回类型/postfix/full-signature 保留 asterisk/name/params/body；`generateClassElementDecorationExpression` 的 `descriptor` 对方法发 `NewKeywordExpression(KindNullKeyword)` 即 `null`，对属性发 `void 0`；`getClassMemberPrefix` 同属性 static→`C`/instance→`C.prototype`；`getExpressionForPropertyName` identifier→字符串字面量）+ `metadata.go`（`shouldAddTypeMetadata` 对 method 真→`serializeTypeOfNode` 的 `KindMethodDeclaration` 臂**硬编码** `NewIdentifier("Function")`、无 checker；`shouldAddReturnTypeMetadata` 对**每个** method 恒真→`serializeReturnTypeOfNode`：有返回注解则 `serializeTypeNode(node.Type())`、否则 `void 0`；`getOldTypeMetadata` 顺序 `design:type`→`design:paramtypes`→`design:returntype`）。**transformers-only**：消费既有 `EmitReferenceResolver::serialize_type_node_for_metadata`（返回类型注解走与属性 `design:type` 同一序列化路径），checker 零改动。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。

### confirmed-vs-Go shapes（动手前已核对）

- **第 4 个 `__decorate` 参数**：方法成员发 `null`（Go `else` 臂 `NewKeywordExpression(KindNullKeyword)`），属性发 `void 0`（`IsPropertyDeclaration && !HasAccessorModifier`）。✓ 确认。
- **`design:type=Function`**（方法）：Go `serializeTypeOfNode` 的 `case KindClassDeclaration, KindClassExpression, KindMethodDeclaration: return s.f.NewIdentifier("Function")` —— 硬编码标识符，**不**查 checker。✓ 确认（区别于属性的 `FunctionType` 注解经 checker → `Function`）。
- **metadata 顺序**：`design:type`、（`design:paramtypes`）、`design:returntype`（`getOldTypeMetadata` 追加序），经 `transformAllDecoratorsOfDeclaration` 的 metadata-居末分组落在真装饰器之后。✓ 本轮发 `design:type`+`design:returntype`（DEFER `design:paramtypes`）。
- **`design:returntype` 恒发**：`shouldAddReturnTypeMetadata` 对 method 无条件返回真，故 `@dec m() {}`（无返回注解）也发 `design:returntype`（= `void 0`）。✓ 确认（故本轮 slice 3/4 同时落 `design:type`+`design:returntype`，二者在 Go 中对方法耦合）。

### 诚实的 red→green 切片记录

逐行为 test-first，每片先写测试再 `cargo test -p tsgo_transformers <name>` 观察（端到端经真 transform 管线 emit 文本）：

1. **`instance_method_decorator_lowers_to_decorate_call`**（tracer bullet，**genuine RED→GREEN**）：`class C { @dec m() {} }`（无 metadata）期望 `class C {\n    m() { }\n}\n__decorate([dec], C.prototype, "m", null);`。RED 实测发射 `class C {\n    @dec\n    m() { }\n}`（装饰方法原样透传——旧路径只识别 `property_has_decorators`，方法落 `visit_each_child_ec` 且 `KindDecorator` 未 elide 故装饰器保留）。实现：`property_has_decorators`→`member_has_decorators`（property/method 皆查 `ModifierFlags::DECORATOR`）；`visit_class_declaration` 成员重建 match 加 `Kind::MethodDeclaration`→`rebuild_method_without_decorators`（剥装饰器、type-params/返回类型/postfix/full-sig 置 None、保留 asterisk/name/params/body）；`is_static_member`/`decorator_expressions_of`/`expression_for_property_name` 加 method 臂；descriptor 改 `if kind==PropertyDeclaration { void 0 } else { null }`。→ GREEN。
2. **`static_method_decorator_uses_class_name_prefix`**（**green-on-arrival**）：`class C { @dec static m() {} }` → `__decorate([dec], C, "m", null);`。slice 1 把 `is_static_member` 泛化到方法（读 property/method 的 `ModifierFlags::STATIC`），故 static 前缀 `C` 直接绿。**硬证据**：slice 1（instance→`C.prototype`）与 slice 2（static→`C`）同绿但前缀不同，排除"硬编码前缀"。
3. **`method_decorator_emits_design_type_function_and_void_returntype`**（**genuine RED→GREEN**）：`class C { @dec m() {} }`+`emitDecoratorMetadata` 期望 `[dec, __metadata("design:type", Function), __metadata("design:returntype", void 0)]`。RED 实测发射 `[dec, __metadata("design:type", Object)]`——方法误走属性 metadata 路径（`type_node` 取不到 → `Object`、且无 `design:returntype`）。实现：抽出 `append_type_metadata(ec, resolver, member, &mut exprs)` 按 kind 分流——property 臂同旧（注解→`serialize_type_node`、无注解→`Object`）；method 臂发 `design:type=Function`（`new_identifier("Function")` 硬编码）+ `design:returntype`（slice 3 最小：硬编码 `void 0`）。→ GREEN。
4. **`method_decorator_serializes_return_type_annotation`**（**genuine RED→GREEN**）：`class C { @dec m(): number { return 1; } }`+meta 期望 `…__metadata("design:returntype", Number)…`。RED 实测发射 `…design:returntype", void 0)…`（slice 3 硬编码 `void 0`）；唯一差异 `void 0` vs `Number`，其余逐字节一致。实现：method 臂的 `design:returntype` 改为读 `MethodDeclaration.type_node`——`Some(t) => serialize_type_node(ec, resolver, t)`（与属性 `design:type` 同一 checker 序列化路径，`: number` → `Number`）、`None => void 0`。→ GREEN。**证明 checker 集成**（`Number` 来自 `serialize_type_node_for_metadata`，非硬编码）。

### scope / upstream 增长（6ao）

> **scope**：仅 `-p tsgo_transformers`（checker 零改动——消费既有 `serialize_type_node_for_metadata` / `serialize_type_node` / `serialized_type_to_expression`）。本 crate 改动：`legacydecorators.rs` 加 `rebuild_method_without_decorators`（走既有 `new_method_declaration`）、`append_type_metadata`（抽出+kind 分流）、`member_has_decorators`/`is_static_member`/`decorator_expressions_of`/`expression_for_property_name` 泛化到 method 臂、descriptor 改 kind-based（`null` for method via 既有 `new_keyword_expression(KindNullKeyword)`）、`visit_class_declaration` 成员重建加 method 臂 + 模块 doc；`legacydecorators_test.rs` +4 `#[test]`。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_method_declaration`/`new_keyword_expression`/`new_identifier`）+ 既有 `make_void_zero`/`serialize_type_node`。**未改任何既有公共 fn 签名**（两工厂 `new_legacy_decorators_transformer[_with_resolver]` 不变）。未触碰 `internal/lsp/lsproto/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6ao 新增）**：`tsgo_transformers` +4 `#[test]`（slice1/3/4 genuine RED→GREEN + slice2 green-on-arrival）+ 0 doctest → **235 unit + 45 doctest**（6an 基线 231+45）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（确认公共 API 加法）。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

### DEFER（本轮确认的 blocked-by，6ao 维度）

- **`design:paramtypes` + 参数装饰器 `__param`**（Go 在 `design:type` 与 `design:returntype` 之间发 `__metadata("design:paramtypes", [...])`；`transformDecoratorsOfParameters` 发 `__param(i, dec)`）：需迭代参数 + 逐参类型序列化（`serializeParameterTypesOfNode`/`serializeTypeOfNode(parameter)`）+ `getDecoratorsOfParameters`（含 `this`-参数偏移、rest-参数 `GetRestParameterElementType`）。blocked-by：参数装饰收集 + 逐参 `serialize_type_node`。**本轮 emit 形是 Go/tsc 输出减去 `design:paramtypes` 条目的忠实子集**（已在测试注释显式标注）。
- **async 方法 `design:returntype=Promise`**（Go `serializeReturnTypeOfNode` 的 `IsAsyncFunction` 臂）：blocked-by：`IsAsyncFunction` 判定（modifier 检查）。
- **accessor（get/set）装饰器**：`getAllAccessorDeclarations` 合并 + `getAccessorTypeNode`（set 取首参类型、get 取返回类型）；`HasAccessorModifier` 属性的 `null` descriptor 分支。blocked-by：accessor 装饰形态 + accessor 合并。
- **class 装饰器包裹**（`let C=…;C=__decorate(...)`）、**计算方法名**、**重载上的装饰器**、**混合 instance/static 语句序**（Go 先全 instance 后全 static，本端口走成员声明序——单成员测试不受影响）：同 6al/6am/6an DEFER。

## 6ap worklog — `tstransforms/legacydecorators` **`design:paramtypes` 数组** + **参数装饰器 `__param`**（transformers-only，消费既有 checker API）

> 本轮目标：落地 6ao DEFER 的头号项——方法的 **`design:paramtypes`** 元数据条目（介于 `design:type` 与 `design:returntype` 之间）与 **参数装饰器 `__param(i, dec)`**。Go ground truth：`metadata.go`（`getOldTypeMetadata` 顺序 `design:type`→`design:paramtypes`→`design:returntype`；`shouldAddParamTypesMetadata` 对 method 真）+ `typeserializer.go`（`serializeParameterTypesOfNode`：逐参 `serializeTypeOfNode(parameter)`=`serializeTypeNode(parameter.Type())`、`serializeTypeNode(nil)`→`Object`、`NewArrayLiteralExpression`、0 参→`[]`；跳 `this` 参 + rest 参 `GetRestParameterElementType`）+ `legacydecorators.go`（`transformDecoratorsOfParameters`：`NewParamHelper(decorator.Expression(), i, …)`→`__param(i, dec)`，经 `getDecoratorsOfParameters` 收集；`transformAllDecoratorsOfDeclaration` 顺序 = 成员装饰器 ++ `__param` ++ metadata；`visitParamerDeclaration` 剥参数修饰器/类型；`NodeOrChildIsDecorated`/`ChildIsDecorated` 把"参数被装饰的方法"判为 decorated class element）+ `printer/helpers.go:paramHelper`（`__param` text，priority **4**）/`factory.go:NewParamHelper`（`__param(<numeric index>, <decorator expr>)`）。**transformers-only**：消费既有 `EmitReferenceResolver::serialize_type_node_for_metadata` / `serialize_type_node`，checker 零改动。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。

### confirmed-vs-Go shapes（动手前已核对）

- **metadata 顺序**：`design:type`→`design:paramtypes`→`design:returntype`（`getOldTypeMetadata` 追加序），metadata 整组经 `transformAllDecoratorsOfDeclaration` 落在真装饰器 + `__param` 之后。✓ 确认。
- **`design:paramtypes` 数组**：每参一个序列化类型（`serializeTypeOfNode(parameter)`→`serializeTypeNode(parameter.Type())`，与属性 `design:type` 同一 checker 路径）；无注解参→`Object`（`serializeTypeNode(nil)`）；0 参→`[]`（`NewArrayLiteralExpression([])`）。✓ 确认。
- **`__param` 形**：`NewParamHelper(expr, i, loc)` = `__param(<i>, <decorator expr>)`（首参数字字面量 index，次参装饰器表达式）；priority 4。✓ 确认（核 `factory.go`/`helpers.go`）。
- **`__param` 顺序/位置**：`transformAllDecoratorsOfDeclaration` = `transformDecorators(decorators)` ++ `transformDecoratorsOfParameters(parameters)` ++ `transformDecorators(metadata)` —— 即**成员装饰器先于 `__param`，`__param` 先于 metadata**。✓ 确认（核 `generateClassElementDecorationExpression` 注释 emit 示例 `[dec, __param(0, dec2), __metadata(...), ...]`）。
- **被装饰参数 → 方法是 decorated element**：即使方法本身无装饰器，`NodeOrChildIsDecorated`（`ChildIsDecorated` 的 method 臂 `Some(parameters, NodeIsDecorated)`）判其为 decorated；本端口 `member_is_decorated` 复刻。✓ 确认。
- **参数体剥离**：`visitParamerDeclaration` elide 修饰器（参数装饰器）+ 置 type/`?` 为 nil，保留 `...`/name/initializer（`m(@pdec a: number)`→`m(a)`）；本端口 `rebuild_parameter_without_decorators` 复刻，并在 `rebuild_method_without_decorators` 对每个参数应用。✓ 确认。

### 诚实的 red→green 切片记录

逐行为 test-first，每片先写测试再 `cargo test -p tsgo_transformers <name>` 观察（端到端经真 transform 管线 emit 文本）：

1. **`method_decorator_emits_design_paramtypes_for_typed_params`**（slice1，**genuine RED→GREEN**）：`class C { @dec m(a: number, b: string) {} }`+meta 期望 `m(a, b) { }` + `[dec, __metadata("design:type", Function), __metadata("design:paramtypes", [Number, String]), __metadata("design:returntype", void 0)]`。RED 实测发射 `m(a: number, b: string) { }`（参数类型未剥）+ `[dec, __metadata("design:type", Function), __metadata("design:returntype", void 0)]`（无 paramtypes）——两处差异。实现：`rebuild_method_without_decorators` 对每参 `rebuild_parameter_without_decorators`（剥修饰器/类型/`?`，保 `...`/name/init）；`append_type_metadata` method 臂在 type 与 returntype 间插 `serialize_parameter_types`（逐参 `serialize_type_node`、无注解→`Object`、`new_array_literal_expression`）。→ GREEN。
2. **empty params → `[]`**（slice2，**green-on-arrival**）：`class C { @dec m() {} }`+meta 现期望 `…__metadata("design:paramtypes", []), …`。由 slice1 的 `serialize_parameter_types`（0 参→空数组）直接覆盖；**更新**两个既有 6ao 测试（`method_decorator_emits_design_type_function_and_void_returntype`、`method_decorator_serializes_return_type_annotation`）从"6ao DEFER 子集（缺 paramtypes）"改为完整 Go 形（含 `[]`）。**硬证据**：slice1（`[Number, String]`）与 slice2（`[]`）同绿但数组内容随参数变，排除"硬编码空数组/硬编码内容"。
3. **`parameter_decorator_lowers_to_param_helper`**（slice3 tracer，**genuine RED→GREEN**）：`class C { m(@pdec a) {} }`（无 meta）期望 `<__decorate>\n<__param>\nclass C {\n    m(a) { }\n}\n__decorate([__param(0, pdec)], C.prototype, "m", null);`。RED 实测**编译错误**（`PARAM_HELPER` 未定义）；定义后若仅此则方法（无自身装饰器）不被 `member_has_decorators` 收、参数装饰器原样透传。实现：定义 `PARAM_HELPER`(priority 4)；`member_is_decorated`=`member_has_decorators || method_has_decorated_parameter`（经 `NodeOrChildIsDecorated`）替换 `visit_class_declaration` 的成员筛选 + 重建守卫；`append_param_decorators`（逐参 `parameter_decorator_expressions` → `new_param_helper(ec, i, expr)`）置于真装饰器后、emptiness 检查前；空判后再追 metadata。→ GREEN。
4. **`method_and_parameter_decorator_order_preserved`**（slice4，**green-on-arrival**）：`class C { @dec m(@pdec a) {} }`（无 meta）→ `[dec, __param(0, pdec)]`。由 slice3 的"真装饰器 ++ `__param`"序直接覆盖；守卫成员装饰器先于 `__param`。
5. **`parameter_decorator_with_metadata_emits_param_and_object_paramtype`**（额外 coverage，**green-on-arrival**）：`class C { m(@pdec a) {} }`+meta → `[__param(0, pdec), __metadata("design:type", Function), __metadata("design:paramtypes", [Object]), __metadata("design:returntype", void 0)]`。守卫 `__param` 先于 metadata + 无注解参 → `design:paramtypes [Object]`（`serializeTypeNode(nil)`）+ 三 helper 按 priority(2/3/4) prologue。

### scope / upstream 增长（6ap）

> **scope**：仅 `-p tsgo_transformers`（checker 零改动——消费既有 `serialize_type_node` / `serialize_type_node_for_metadata`）。本 crate 改动：`legacydecorators.rs` 加 `PARAM_HELPER`(static, priority 4)、`serialize_parameter_types`、`append_param_decorators`、`parameter_decorator_expressions`、`new_param_helper`、`member_is_decorated`/`method_has_decorated_parameter`/`parameter_has_decorators`、`rebuild_parameter_without_decorators`；`append_type_metadata` method 臂插 `design:paramtypes`；`rebuild_method_without_decorators` 逐参重建；`visit_class_declaration` 成员筛选/重建守卫改 `member_is_decorated`；`generate_class_element_decoration_statement` 在真装饰器后插 `append_param_decorators` + 模块/函数 doc 更新；`legacydecorators_test.rs` +4 `#[test]`（slice1/3/4 + 组合 coverage）、更新 2 既有 6ao 测试。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_array_literal_expression`/`new_numeric_literal`/`new_call_expression`/`new_parameter_declaration`/`new_unscoped_helper_name`）+ 既有 `serialize_type_node`/`request_emit_helper`。**未改任何既有公共 fn 签名**（两工厂不变）。未触碰 `internal/lsp/lsproto/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6ap）**：`tsgo_transformers` **235→239 unit**（+4 `#[test]`：slice1 genuine RED→GREEN + slice3 genuine RED→GREEN + slice4 green-on-arrival + 组合 coverage green-on-arrival；另更新 2 既有 6ao 测试含 `design:paramtypes []`）+ 45 doctest（不变）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（确认公共 API 加法）。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

### DEFER（本轮确认的 blocked-by，6ap 维度）

- **`this`-参数偏移**（Go `getDecoratorsOfParameters` / `serializeParameterTypesOfNode` 跳首个 `this` 参并调整 index）：可达子集（无 `this` 参）下 index 直映；blocked-by：`IsThisParameter` 判定（参数名 == `"this"`）。
- **rest-参数元素类型**（Go `serializeTypeNode(GetRestParameterElementType(p.Type()))`）：`...args: T[]` 应序列化元素类型 `T` 而非数组本身；blocked-by：`GetRestParameterElementType`（array/tuple 元素抽取）。
- **构造函数参数装饰器**（`getAllDecoratorsOfClass` 的 `getDecoratorsOfParameters(constructor)` + `generateConstructorDecorationExpression`）：目标为类构造器而非方法，依赖 class 装饰器包裹路径；blocked-by：class 装饰器 lowering。
- **accessor 参数装饰器**（set accessor 的 value 参；`getAllDecoratorsOfAccessors` 合并）：blocked-by：accessor 装饰形态 + `getAllAccessorDeclarations`。
- **async 方法 `design:returntype=Promise`**、**class 装饰器包裹**、**计算名**、**`TypeReference` lib-globals/qualified-name**：同 6ao/6an DEFER。

## 6aq worklog — `tstransforms/legacydecorators` **访问器（get/set）装饰器** lowering + get/set 对合并 + `design:type`/`design:paramtypes` from accessor type（transformers-only，消费既有 checker API）

> 本轮目标：落地 6ap DEFER 的"accessor 装饰器（accessor 合并）"项。Go ground truth：`legacydecorators.go`（`visitGetAccessorDeclaration`/`visitSetAccessorDeclaration` 剥装饰器/类型重建；`getAllDecoratorsOfAccessors`：经 `ast.GetAllAccessorDeclarations` 配对同名 get/set，**仅声明序第一个带装饰器的访问器**拥有发射的 `__decorate`，另一访问器返回 nil；`useLegacyDecorators && setAccessor != nil` 时参数取自 set accessor；`generateClassElementDecorationExpression` 的第 4 参对非属性成员（含访问器）= `null`）+ `metadata.go`（`shouldAddTypeMetadata` 对 get/set 真→`design:type`；`shouldAddParamTypesMetadata` **对 get/set 也真**→`design:paramtypes`；`shouldAddReturnTypeMetadata` **仅 method**→访问器**无** `design:returntype`；`getAccessorTypeNode`：有 setter 取 `getSetAccessorTypeAnnotationNode`(value 参类型) 否则取 getter 返回类型；`GetSetAccessorValueParameter` 跳首个 `this` 参）+ `typeserializer.go`（`serializeTypeOfNode` get/set 臂→`serializeTypeNode(getAccessorTypeNode)`；`serializeParameterTypesOfNode` + `getParametersOfDecoratedDeclaration`：getter 借 set accessor 的参数、否则自身参数）。**transformers-only**：消费既有 `EmitReferenceResolver::serialize_type_node_for_metadata` / `serialize_type_node`，checker 零改动。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。

### confirmed-vs-Go shapes（动手前已核对，**纠正任务提示**）

- **第 4 参 `null`**：访问器与方法一致用 `null`（`generateClassElementDecorationExpression` 的 `else` 分支，仅 `IsPropertyDeclaration && !HasAccessorModifier` 用 `void 0`）。✓ 实测对照 baseline `submodule/conformance/decoratorOnClassAccessor8`（全 6 类均 `…, "x", null`）。
- **访问器 metadata 集合 = `design:type` + `design:paramtypes`（**无** `design:returntype`）**：任务提示"accessors emit ONLY design:type"**与 Go/tsc 不符**——`shouldAddParamTypesMetadata` 对 `KindGetAccessor`/`KindSetAccessor` 返回 `true`，故同时发 `design:paramtypes`。✓ 实测对照 baseline class E（getter-only：`design:type Object` + `design:paramtypes []`）、class A/B/F（`design:type Number` + `design:paramtypes [Number]`）。`shouldAddReturnTypeMetadata` 仅 method → 访问器无 returntype。✓
- **`getAccessorTypeNode`（`design:type` 来源）**：有 set accessor 时取 set 的 value 参类型（**setter 赢**，即使同时有 getter）；否则取 getter 返回类型；无注解→`Object`。✓ 对照 class A（`@dec get x()` + `set x(value: number)` → `Number` 来自 setter）、class E（getter-only 无注解 → `Object`）。
- **get/set 对合并（单 `__decorate`，归属第一个带装饰器的访问器）**：`getAllDecoratorsOfAccessors` 用 `firstAccessorWithDecorators`（声明序首个带装饰器者）拥有发射；另一访问器（`accessor != firstAccessorWithDecorators`）返回 nil。✓ 对照 class B（getter 在前未装饰、`@dec set` 在后→setter 拥有单 `__decorate`）。两访问器都带装饰器属 TS 语法错误 → DEFER。
- **`design:paramtypes` 来源**（`getParametersOfDecoratedDeclaration`）：装饰的是 getter 且存在 setter → 借 setter 参数（class A → `[Number]`）；getter 无 setter → 自身（空）参数（class E → `[]`）；装饰的是 setter → 自身参数（class F → `[Number]`）。✓
- **未装饰伙伴访问器也被重建**：Go `transformClassDeclarationWithoutClassDecorators` 经 `VisitNodes(members)` 访问**每个**成员，故未装饰的 setter 伙伴也剥类型（`set x(value: number)`→`set x(value)`）。本端口对 `Kind::GetAccessor | Kind::SetAccessor` **无条件**重建以复刻。✓

### 诚实的 red→green 切片记录

逐行为 test-first，每片先写测试再 `cargo test -p tsgo_transformers <name>` 观察（端到端经真 transform 管线 emit 文本）：

1. **`instance_get_accessor_decorator_lowers_to_decorate_call`**（slice1 tracer，**genuine RED→GREEN**）：`class C { @dec get x() { return 1; } }` 期望 `class C {\n    get x() { return 1; }\n}\n__decorate([dec], C.prototype, "x", null);`。RED 实测整类透传（`class C {\n    @dec\n    get x() { return 1; }\n}`，访问器未被识别为装饰成员）。实现：`member_has_decorators`/`is_static_member`/`decorator_expressions_of`/`expression_for_property_name` 加 `GetAccessorDeclaration | SetAccessorDeclaration` 臂；`visit_class_declaration` 成员重建加 `Kind::GetAccessor | Kind::SetAccessor` 无条件臂 → `rebuild_accessor_without_decorators`（剥装饰器/类型参数/返回类型/full-signature、逐参 `rebuild_parameter_without_decorators`）。第 4 参经既有 `else`→`null`。→ GREEN。
2. **`instance_set_accessor_decorator_lowers_to_decorate_call`**（slice1 对称，**green-on-arrival**）：`class C { @dec set x(v) {} }` → `…set x(v) { }…__decorate([dec], C.prototype, "x", null);`。由 slice1 的 set 臂直接覆盖；守卫 setter 形对称。
3. **`get_accessor_decorator_emits_design_type_from_return_and_empty_paramtypes`**（slice2 headline，**genuine RED→GREEN**）：`class C { @dec get x(): number { return 1; } }`+meta 期望 `…[dec, __metadata("design:type", Number), __metadata("design:paramtypes", [])]…`。RED 实测仅 `[dec]`（无 metadata，`append_type_metadata` 访问器臂缺失）。实现：把原始成员 id（类型未剥）经 `member_ids` 穿入 `generate_class_element_decoration_statement`/`append_type_metadata`；加 `Kind::GetAccessor | Kind::SetAccessor` metadata 臂——`accessor_type_node`（`find_accessor_pair`→setter value 参类型 / getter 返回类型）经 `serialize_type_node` 发 `design:type`，`serialize_accessor_parameter_types`（`parameters_of_decorated_accessor`）发 `design:paramtypes`，**不**发 returntype；加 `accessor_owns_decoration`（`getAllAccessorDeclarations` 配对 + `firstAccessorWithDecorators` 归属）守卫在统计语句生成入口。→ GREEN。**硬证据**：getter-only 返回类型 `number`→`Number` + 空参 `[]`，与 setter 形（值参 `[Number]`）对照，排除硬编码。
4. **`set_accessor_decorator_emits_design_type_and_paramtypes_from_value_param`**（slice2 setter 形，**green-on-arrival**）：`class C { @dec set x(v: number) {} }`+meta → `design:type Number`（值参）+ `design:paramtypes [Number]`（自身参）。由 slice2 的统一访问器臂覆盖。
5. **`get_set_pair_emits_single_decorate_owned_by_getter`**（slice3 合并 headline，**green-on-arrival**）：`class C { @dec get x() { return 1; } set x(v) {} }`（无 meta）→ 两访问器都在体内、**单** `__decorate([dec], C.prototype, "x", null)`。守卫"未装饰 setter 伙伴不发自己的 `__decorate`"（其不入 `decorated_members`）+ slice1 的伙伴无条件重建。
6. **`get_set_pair_metadata_crosses_to_setter_value_param`**（slice3 跨访问器 metadata，**green-on-arrival**）：`@dec get x() { return 0; } set x(value: number) {}`+meta → 装饰的是 getter 但 `design:type`=`Number`（来自 setter 值参）、`design:paramtypes`=`[Number]`（借 setter 参）。对照 baseline class A。
7. **`get_set_pair_setter_decorated_second_owns_single_decorate`**（slice3 归属变体，**green-on-arrival**）：`get x() { return 0; } @dec set x(value: number) {}`+meta → setter（声明序第二）拥有单 `__decorate`，未装饰 getter（第一）不发。守卫 `firstAccessorWithDecorators` 取声明序首个**带装饰器**者。对照 baseline class B。
8. **`static_get_accessor_decorator_uses_class_name_prefix`**（slice4 tracer，**green-on-arrival**）：`class C { @dec static get x() { return 1; } }` → `…static get x() { return 1; }…__decorate([dec], C, "x", null);`（前缀 `C` 而非 `C.prototype`）。由 slice1 的 `is_static_member` 访问器臂覆盖。
9. **`static_get_accessor_decorator_with_metadata_uses_class_name_prefix`**（slice4 + metadata coverage，**green-on-arrival**）：`@dec static get x(): number`+meta → `…[dec, __metadata("design:type", Number), __metadata("design:paramtypes", [])], C, "x", null`。守卫 static 前缀与访问器 metadata 路径正交。

### scope / upstream 增长（6aq）

> **scope**：仅 `-p tsgo_transformers`（checker 零改动——消费既有 `serialize_type_node` / `serialize_type_node_for_metadata`）。本 crate 改动：`legacydecorators.rs` 加 `rebuild_accessor_without_decorators`、`AccessorPair`/`find_accessor_pair`/`accessor_name_text`/`accessor_owns_decoration`/`is_this_parameter`/`set_accessor_value_parameter`/`set_accessor_type_annotation_node`/`accessor_type_node`/`parameters_of_decorated_accessor`/`serialize_accessor_parameter_types`；`member_has_decorators`/`is_static_member`/`decorator_expressions_of`/`expression_for_property_name` 加访问器臂；`visit_class_declaration` 成员重建加访问器无条件臂 + 穿 `member_ids`；`generate_class_element_decoration_statement`/`append_type_metadata` 加 `members` 形参 + 访问器归属/metadata 臂 + 模块/函数 doc 更新；`legacydecorators_test.rs` +9 `#[test]`（slice1 tracer genuine RED→GREEN + slice2 headline genuine RED→GREEN + 7 green-on-arrival 覆盖 setter/对合并/static）。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_accessor_declaration`/`new_array_literal_expression`/`new_identifier`/`new_property_access_expression`/`new_metadata_helper` 等）+ 既有 `serialize_type_node`/`request_emit_helper`。**未改任何既有公共 fn 签名**（两工厂 `new_legacy_decorators_transformer`/`…_with_resolver` 不变）。未触碰 `internal/lsp/lsproto/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6aq）**：`tsgo_transformers` **239→248 unit**（+9 `#[test]`：slice1 tracer genuine RED→GREEN + slice2 headline genuine RED→GREEN + 7 green-on-arrival）+ 45 doctest（不变）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（确认公共 API 加法）。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

### DEFER（本轮确认的 blocked-by，6aq 维度）

- **计算/私有/字符串字面量访问器名**：`find_accessor_pair` 仅按 identifier 文本配对（`accessor_name_text` 对非 identifier 返回 `None`→单访问器降级）；blocked-by：`getPropertyNameForPropertyNameNode` / 计算名 cache / checker 符号配对。
- **访问器参数装饰器**（set accessor value 参的 `@pdec`；Go `getAllDecoratorsOfAccessors` 合并 `getDecoratorsOfParameters(setAccessor)`）：本端口 `member_is_decorated` 对访问器仅看自身装饰器（`method_has_decorated_parameter` 仅 method 臂）；blocked-by：访问器参数装饰器检测 + `__param` 接线到访问器目标。
- **`this`-参数体擦除**：`set x(this: T, v)` 的 `this` 参在降级体内未被擦除（`rebuild_accessor_without_decorators` 保留它）；metadata 路径已跳 `this`（`is_this_parameter` 的 `serialize_accessor_parameter_types`/`set_accessor_value_parameter` 复刻 Go `IsThisParameter`），但体渲染 DEFER；blocked-by：`this`-参数节点省略。
- **get+set 双装饰**（TS 语法错误，"Decorators cannot be applied to multiple get/set accessors of the same name"）：`accessor_owns_decoration` 已忠实仅让声明序第一个拥有（不会双发），但该错误形态不在可达有效代码内，未测。
- **rest 参 `GetRestParameterElementType`、async 访问器、class 装饰器包裹、`TypeReference` lib-globals/qualified-name**：同 6ap/6ao/6an DEFER。

## 6ar worklog — `tstransforms/legacydecorators` **class 装饰器** lowering（`let C = class C {…}; C = __decorate([…], C);` 包裹 + 构造函数参装饰器 `__param` + 构造 `design:paramtypes` 元数据）（transformers-only，消费既有 checker API）

> 本轮目标：落地 6al–6aq 共同 DEFER 的头号项——**class 自身被装饰**（class 装饰器 / 装饰的构造函数参数）的 lowering。Go ground truth：`legacydecorators.go`（`visitClassDeclaration` 的 `decorated` 分支 → `transformClassDeclarationWithClassDecorators`：`declName = GetLocalName(node)` 的 `let` 绑定 + `exprName = name` 的 `class C {…}` 类表达式 + `statements = [varStatement] ++ memberDecorations ++ getConstructorDecorationStatement`；`getConstructorDecorationStatement` → `generateConstructorDecorationExpression`：`getAllDecoratorsOfClass`（class 装饰器 + `getDecoratorsOfParameters(GetFirstConstructorWithBody)`）→ `transformAllDecoratorsOfDeclaration`（decorators ++ `__param` ++ metadata）→ `NewDecorateHelper(localName)` + `NewAssignmentExpression(localName, decorate)`；`visitConstructorDeclaration` 剥构造函数参装饰器/类型）+ `metadata.go`（`shouldAddParamTypesMetadata` 对 class = `GetFirstConstructorWithBody(node) != nil`；class **仅**发 `design:paramtypes`——`shouldAddTypeMetadata`/`shouldAddReturnTypeMetadata` 对 class 皆假）+ `ast/utilities.go`（`ClassOrConstructorParameterIsDecorated` / `GetFirstConstructorWithBody` / `ChildIsDecorated` 构造臂）。**transformers-only**：消费既有 `EmitReferenceResolver::serialize_type_node_for_metadata` / `serialize_type_node`，checker 零改动。另一 lane 并发改 `internal/lsp/lsproto/**`（构建不相交，未触碰）。

### confirmed-vs-Go shapes（动手前已核对）

- **`let`-binding 形 + 类表达式保名**：Go `declName = GetLocalName(node)`（非默认/非生成名 → 类名 `C`）的 `let C = …` 绑定；`exprName = name`（非生成名保留）→ `class C {…}` **保名**。✓ 对照 Go 源码注释 Example 1：`@dec class C {}` → `let C = class C {\n};\nC = __decorate([dec], C);`。Rust printer 类表达式作 var 初值不加括号（仅 statement-start 才需），实测确认。
- **装饰器顺序**：`getAllDecoratorsOfClass` 的 `node.Decorators()` 取源序；`transformAllDecoratorsOfDeclaration` = class 装饰器 ++ 构造参 `__param` ++ metadata。故 `@a @b` → `[a, b]`；构造参装饰器 `__param(i, dec)` 居 class 装饰器后、metadata 前。✓
- **构造函数参装饰器进 CLASS 数组**：`ClassOrConstructorParameterIsDecorated` 对「装饰的构造参」亦真 → 即便**无** class 装饰器，class 仍走包裹路径；`__param(i, dec)` 目标是构造器 `C`（非成员 `C.prototype/"m"`）。✓ 对照 tsc：`class C { constructor(@pdec a: number) {} }` → `…C = __decorate([__param(0, pdec)], C);`。
- **class 仅发 `design:paramtypes`**：`shouldAddParamTypesMetadata`(class) = 有构造体；`shouldAddTypeMetadata`/`shouldAddReturnTypeMetadata` 对 class 皆假 → class **无** `design:type`/`design:returntype`（成员专属）。无构造函数 → 无 `design:paramtypes`。✓ 对照 tsc。
- **成员 `__decorate` 先于 class `__decorate`**：`statements = [var] ++ memberDecorations ++ constructorDecoration`。✓
- **class-alias 自引用改写 status**：DEFER。Go 在类体内引用自身（`static x = C.y`）时改写为 `let C = C_1 = class C {…}` + `C = C_1 = __decorate(...)`，需 `getReferencedValueDeclaration`/`NewUniqueName`/`classAliases` substitution + per-node `ConstructorReference` flags。可达子集无自引用，故未触发；忠实 DEFER。

### 诚实的 red→green 切片记录

逐行为 test-first，每片先写测试再 `cargo test -p tsgo_transformers <name>` 观察（端到端经真 transform 管线 emit 文本）：

1. **`class_decorator_lowers_to_let_wrap_and_decorate`**（slice1 tracer，**genuine RED→GREEN**，**替** 6al 的 `class_decorator_is_left_unchanged` DEFER 守卫）：`@dec class C {}` 期望 `let C = class C {\n};\nC = __decorate([dec], C);`。RED 实测整类透传（`@dec\nclass C {\n}`，旧 DEFER 早退）。实现：`visit_class_declaration` 把 `has_class_decorator` 早退改 route 到新 `transform_class_declaration_with_class_decorators`（构造 `ClassExpression` 保名 + `let` 绑定 `add_flags(NodeFlags::LET)` + 成员重建 + 成员/class 装饰语句）；`generate_constructor_decoration_expression`（slice1 仅 `class_decorator_expressions`）+ `get_constructor_decoration_statement` + `class_has_decorator`/`class_decorator_expressions`；`rebuild_class_member` 抽既有成员重建（property/method 守卫 + accessor 无条件）共用。→ GREEN。
2. **`multiple_class_decorators_preserve_source_order`**（slice2，**green-on-arrival**）：`@a @b class C {}` → `C = __decorate([a, b], C);`。由 slice1 的 `class_decorator_expressions`（保 modifier list 序）直接覆盖；守卫反序/单装饰器退化。
3. **`constructor_parameter_decorator_decorates_class_constructor`**（slice3 headline，**genuine RED→GREEN**）：`class C { constructor(@pdec a: number) {} }` 期望 `let C = class C {\n    constructor(a) { }\n};\nC = __decorate([__param(0, pdec)], C);`。RED 实测整类透传（slice1 路由仅 `class_has_decorator`，构造参装饰类落 without-class 路径且 `member_is_decorated` 不识别构造器 → 不变换）。实现：路由改 `class_or_constructor_parameter_is_decorated`（+`first_constructor_with_body`/`constructor_has_decorated_parameter`）；`rebuild_class_member` 加 `Kind::Constructor` 臂 → `rebuild_constructor_without_decorators`（逐参剥装饰器/类型）；`generate_constructor_decoration_expression` 接 `append_constructor_param_decorators`（构造参 → `__param(i, dec)`）。→ GREEN。
4. **`class_decorator_with_decorated_method_emits_member_then_class_decorate`**（slice4a，**green-on-arrival**）：`@dec class C { @mdec m() {} }` → 成员 `__decorate([mdec], C.prototype, "m", null)` **先于** `C = __decorate([dec], C)`。由 slice1 的成员装饰循环（在包裹路径内、class 装饰前）覆盖；守卫语句序。
5. **`class_constructor_paramtypes_metadata_under_emit_decorator_metadata`**（slice4b headline，**genuine RED→GREEN**）：`@dec class C { constructor(a: number) {} }`+meta 期望 `C = __decorate([dec, __metadata("design:paramtypes", [Number])], C);`。RED 实测仅 `[dec]`（无 metadata，`generate_constructor_decoration_expression` 忽略 cfg）。实现：接 `cfg.emit_decorator_metadata` + `append_class_type_metadata`（`first_constructor_with_body` 门）+ `serialize_constructor_parameter_types`（逐构造参经既有 `serialize_type_node`、跳 `this`、无注解→`Object`）。→ GREEN。**硬证据**：`: number`→`Number` 经 checker，排除硬编码。
6. **`class_decorator_without_constructor_emits_no_paramtypes_metadata`**（slice4b，**green-on-arrival**）：`@dec class C {}`+meta → `C = __decorate([dec], C);`（无 metadata）。守卫 `GetFirstConstructorWithBody==nil` 门——metadata 是构造条件的，非无条件。
7. **`constructor_parameter_decorator_with_metadata_orders_param_before_paramtypes`**（slice4b coverage，**green-on-arrival**）：`class C { constructor(@pdec a: number) {} }`+meta → `C = __decorate([__param(0, pdec), __metadata("design:paramtypes", [Number])], C);`。守卫 `__param` 先于 metadata + 三 helper priority(2/3/4) prologue。

### scope / upstream 增长（6ar）

> **scope**：仅 `-p tsgo_transformers`（checker 零改动——消费既有 `serialize_type_node` / `serialize_type_node_for_metadata`）。本 crate 改动：`legacydecorators.rs` 加 `transform_class_declaration_with_class_decorators`、`rebuild_class_member`（抽既有成员重建 + 加 `Kind::Constructor` 臂）、`rebuild_constructor_without_decorators`、`class_has_decorator`、`class_or_constructor_parameter_is_decorated`、`first_constructor_with_body`、`constructor_has_decorated_parameter`、`class_decorator_expressions`、`get_constructor_decoration_statement`、`generate_constructor_decoration_expression`、`append_constructor_param_decorators`、`append_class_type_metadata`、`serialize_constructor_parameter_types`；`visit_class_declaration` 路由改 `class_or_constructor_parameter_is_decorated`；模块/函数 doc 更新（class 装饰器 + 构造参装饰器从 DEFER 移入「已落地」，新 DEFER = class-alias 自引用 + export/default）；`legacydecorators_test.rs` 替 1（`class_decorator_is_left_unchanged`→`class_decorator_lowers_to_let_wrap_and_decorate`）+ 加 6（multi / 构造参 / class+成员序 / 构造 metadata / 无构造无 metadata / 构造参+metadata 序）。**ZERO** ast/printer/checker 增长——全走 arena 既有构造器（`new_class_like(Kind::ClassExpression)`/`new_variable_declaration`/`new_variable_declaration_list`+`add_flags(NodeFlags::LET)`/`new_variable_statement`/`new_constructor_declaration`/`new_binary_expression`+`new_token(Kind::EqualsToken)`/`new_expression_statement`/`new_array_literal_expression`）+ 既有 `serialize_type_node`/`new_decorate_helper`/`new_param_helper`/`new_metadata_helper`/`request_emit_helper`。**未改任何既有公共 fn 签名**（两工厂 `new_legacy_decorators_transformer`/`…_with_resolver` 不变）。未触碰 `internal/lsp/lsproto/**`、任何其它 crate、root Cargo、任何 `.go`、README。
>
> **测试计数（6ar）**：`tsgo_transformers` **248→254 unit**（替 1 + 加 6 = 净 +6：slice1 tracer genuine RED→GREEN + slice3 headline genuine RED→GREEN + slice4b headline genuine RED→GREEN + 4 green-on-arrival）+ 45 doctest（不变）。`cargo test -p tsgo_transformers` 全绿；`cargo clippy -p tsgo_transformers --all-targets -- -D warnings` 干净；`cargo fmt -p tsgo_transformers -- --check` 干净；`cargo build -p tsgo_compiler` 绿（确认公共 API 加法）。未 `--workspace`（lsproto lane 并发）。未 `git commit`。

### DEFER（本轮确认的 blocked-by，6ar 维度）

- **class-alias 自引用改写**（`let C = C_1 = class C {…}` + 体内 `C` 引用→`C_1` + `C = C_1 = __decorate(...)` + `var C_1;`）：可达子集无自引用，未触发；blocked-by：`referenceResolver.GetReferencedValueDeclaration` + `NewUniqueName`/`AddVariableDeclaration` + `classAliases` substitution（`visitIdentifier`）+ per-node `ConstructorReference` flags（决定体内 vs 体外引用）。
- **`export` / `export default` 装饰类**：尾随 `export { C };`（`NewExternalModuleExport`）/ `export default C;`（`NewExportDefault`）语句、匿名默认导出的 `default_1` 重命名、modifier 过滤（`isNotExportOrDefaultOrDecorator`）保留 export/default。可达子集无 export/default；blocked-by：export/default modifier 处理 + `GetLocalName`/生成名 emit-name 形 + 默认导出 `GetGeneratedNameForNode`。
- **`this`-参数偏移**（构造函数首个 `this` 参跳过 + index 调整）：可达子集无 `this` 参；`serialize_constructor_parameter_types` 已复刻 `IsThisParameter` 跳过（metadata 路径），但 `append_constructor_param_decorators` 的 index 偏移 DEFER；blocked-by：`getDecoratorsOfParameters` 的 `firstParameterOffset` 完整移植。
- **计算/私有成员名、装饰器内私有标识符（`hasClassElementWithDecoratorContainingPrivateIdentifierInExpression` → static block 包裹）、`ClassExpression` 装饰（Go 不支持，原样）、async 方法 `Promise`、`TypeReference` lib-globals/qualified-name**：同 6aq/6ap DEFER。

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
