# Phase 4 — 类型检查（type checking）

> 全 10 phase 中**最硬的一个**。把 typescript-go 的类型检查子系统逐文件 1:1 移植到安全 Rust。
> 方法论与共享契约见根 [PORTING.md](../PORTING.md)（必读）。本 README 讲本 phase 的包、依赖序、规模、子阶段拆分与纪律。

## 范围与依赖序

本 phase 含 **13 个包**：checker 的类型系统依赖（8 个）+ **checker 的构建前置**（5 个，依据序修正前移自 P5/P6：`outputpaths` `sourcemap` `tracing` `tsoptions` `printer`）。按**包内真实非测试 import 边**排出依赖序（叶子先行）：

```
outputpaths ─┐(仅 ast/core/tspath)
evaluator ───┤
nodebuilder ─┤(叶子, 仅依赖 ast / jsnum / core)
packagejson ─┤
sourcemap ───┤(core/debug/json/scanner/stringutil/tspath)
tracing ─────┘(ast/json/scanner/tspath/vfs)
   │
   ▼
module ──► symlinks ──► tsoptions ──► modulespecifiers
   │                       │
   ├─────────► pseudochecker(仅依赖 ast, 可并行)
   ▼
printer  ◄── (ast/binder/evaluator/nodebuilder/sourcemap/tsoptions)
   │
   ▼
checker  ◄── (依赖 evaluator/module/modulespecifiers/nodebuilder/printer/tracing/tsoptions + P1–P3 全部)
```

推进顺序建议：`outputpaths`/`evaluator`/`nodebuilder`/`packagejson`/`sourcemap`/`tracing`（叶子，可并行）→ `module` → `symlinks` → `tsoptions` → `modulespecifiers` → `pseudochecker` → `printer` → `checker`。

> **为何 5 个包前移到 P4**：`checker`（非测试）构建依赖 `printer`/`tracing`/`tsoptions`，`modulespecifiers` 依赖 `outputpaths`/`tsoptions`，`printer` 依赖 `sourcemap`/`tsoptions`——它们都是 checker 的**构建前置**，必须排在 checker 之前（详见根 README「依赖序口径」与 gate-docs.sh D6）。`transformers`（依赖 checker/printer）相应留在 checker 之后（P5）。

## 包规模速查（采自当前仓库）

| 包 | 实现文件 | 实现行数 | 测试文件 | 测试 func | 包内单测覆盖 | crate |
|---|---|---|---|---|---|---|
| evaluator | 1 | 169 | 0 | 0 | **无**（P10 兜底） | `tsgo_evaluator` |
| nodebuilder | 1 | 79 | 0 | 0 | **无**（P10 兜底） | `tsgo_nodebuilder` |
| packagejson | 6 | ~460 | 4 | 5（4 Test+1 Bench） | 有（解析层 1:1） | `tsgo_packagejson` |
| symlinks | 1 | 135 | 2 | 13（8 Test+5 Bench） | 有（最全） | `tsgo_symlinks` |
| module | 4 | ~2,700 | 1 | 2（回归） | 弱（2 回归 + util 行为级） | `tsgo_module` |
| modulespecifiers | 5 | ~3,300 | 1 | 6 | 弱（6 点 + 行为级） | `tsgo_modulespecifiers` |
| pseudochecker | 3 | ~1,300 | 0 | 0 | **无**（P10 兜底） | `tsgo_pseudochecker` |
| **checker** | **24** | **59,514** | **2** | **3**（2 Test+1 Bench） | **几乎无**（P10 兜底） | `tsgo_checker` |

> **0 直接单测**的包：`evaluator`、`nodebuilder`、`pseudochecker`（与根 README 的"0 直接单测"清单一致）。这些包 + `checker` 的绝大部分，正确性都靠 **P10 conformance/fourslash/`.d.ts` baseline** 兜底，各自 `tests.md` 已补行为级 Rust 测试覆盖关键路径。

## checker 的体量与"为何正确性主要靠 P10 conformance"

`checker` 一个包就 **59,514 行 / 24 文件**，其中 `checker.go` 单文件 ~3.2 万行、上千个方法。它是全仓最大模块（约占 typescript-go 实现总量的 1/5）。

而它的**包内单测只有 3 个 func**（`TestGetSymbolAtLocation` + `TestTracerPushPreservesEndArgMutations` + `BenchmarkNewChecker`）。这不是疏漏，而是类型检查器的本质：检查器的正确性是"对海量真实 TS 程序产出正确的类型与诊断"，无法用少量手写单测覆盖。TypeScript 团队自己也是用 **conformance 套件**（`tests/cases/conformance/**`，配 `.errors.txt`/`.types`/`.d.ts` baseline）+ **fourslash**（4250 个交互用例）作为检查器的"测试"。

因此本 phase 的纪律是：
- **结构先行、逐文件 1:1**：`impl.md` 不逐函数枚举 checker（不可能），而是文件职责总览 + 子系统 TODO + `// Go:` 锚点。
- **包内单测当 tracer bullet**：`checker` 的 2 个真单测分别作为子阶段 4a（tracer）/ 4b（符号解析）的收口判据。
- **正确性 DEFER 到 P10**：每个 checker 子阶段在其 worklog 登记"应转绿的 conformance 目录"，以端到端 baseline 对拍为真实收口判据（见 `checker/tests.md`）。

## 强烈建议：checker 自身再拆 4a..4k 子阶段

`checker` 不可能一次 red→green。详见 [checker/impl.md](./checker/impl.md) 的"子阶段建议"。摘要：

| 子阶段 | 主题 | 关键收口 |
|---|---|---|
| 4a | 类型/符号所有权地基（types/mapper/tracer + Checker 骨架） | `NewChecker` 可构造；`TestTracerPushPreservesEndArgMutations` 绿 |
| 4b | 符号解析与模块导出 | `TestGetSymbolAtLocation` 绿（包内唯一真功能单测） |
| 4c | 声明类型 / 类型获取 | `getTypeOfSymbol`/`getDeclaredTypeOfSymbol` |
| 4d | 类型实例化 + 关系（relater） | `instantiateType`/`checkTypeRelatedTo` + 方差 |
| 4e | 类型推断（inference） | `inferTypes` |
| 4f | 控制流分析 / 收窄（flow） | `getFlowTypeOfReference` |
| 4g | 表达式/语句检查 + JSDoc | `checkSourceFile`/`checkExpression`/`getDiagnostics` |
| 4h | JSX | `checkJsxElement` |
| 4i | 语法检查（grammarchecks） | grammar 诊断 |
| 4j | node builder/序列化/可达性/printer/hover/services | `typeToString`/`SerializeTypeForDeclaration` |
| 4k | emit resolver | `GetEmitResolver`（交付 P5） |

建议每个子阶段在 `phase-4-checker/checker/4x-*/worklog.md` 维护函数级 TODO + 覆盖的 conformance 目录。README 的 P4 主条目下可补 4a..4k 子勾选。

## 所有权：Type / Symbol 图用 arena + TypeId/SymbolId（PORTING §5）

本 phase 是 PORTING §5 arena 模型最重的落地点：
- `Type` / `Symbol` / `Signature` / `IndexInfo` **全部 arena + 句柄索引**（`TypeId`/`SymbolId`/`SignatureId`/`IndexInfoId`），所有引用（union 成员、type reference、mapper、symbol links）都用句柄，不用 `&T`/`Rc<T>`。
- Go 的 interning `map[...]*Type` → `FxHashMap<Key, TypeId>`（影响诊断/emit 顺序处用 `IndexMap`）。
- **删除 `Type.checker` 反向指针**：类型操作改 `checker.method(type_id, ...)`，checker 持 arena。这是允许且必要的偏离（§5），已在 `checker/impl.md` 顶部注明。
- `pseudochecker` 的 `PseudoType` 是树形短生命周期骨架 → `Box`（仅在需跨缓存共享时才上 `PseudoTypeId` arena）。

## 目录结构

```
phase-4-checker/
├── README.md                  # 本文件
├── evaluator/        {impl.md, tests.md}
├── packagejson/      {impl.md, tests.md}
├── outputpaths/      {impl.md, tests.md}   # 前移自 P5
├── sourcemap/        {impl.md, tests.md}   # 前移自 P5
├── tracing/          {impl.md, tests.md}   # 前移自 P6
├── module/           {impl.md, tests.md}
├── symlinks/         {impl.md, tests.md}
├── tsoptions/        {impl.md, tests.md}   # 前移自 P6
├── modulespecifiers/ {impl.md, tests.md}
├── nodebuilder/      {impl.md, tests.md}
├── pseudochecker/    {impl.md, tests.md}
├── printer/          {impl.md, tests.md}   # 前移自 P5（emit 核心，checker 依赖）
└── checker/          {impl.md, tests.md}   # + 执行期建 4a..4k/worklog.md
```

## 实施纪律（每个包收口前）

1. 读该包 `impl.md` + `tests.md` + **对应 Go 源码 + `*_test.go`**。
2. 先写 Rust 测试（red）→ 再写实现（green），逐文件、逐用例；0 单测的包用各 `tests.md` 的行为级测试当 gate。
3. 验证：`cargo test -p tsgo_<pkg>` 全绿 + `cargo clippy -p tsgo_<pkg>` 干净 + rustdoc 规范自检（PORTING §7）。
4. `tests.md` 与 Go 测试逐用例对齐审查（PORTING §8），`impl.md`↔`tests.md` 互对齐。
5. checker 按 4a..4k 子阶段推进，每子阶段登记并对拍其 conformance 目录。
6. 勾选文档，更新根 README 进度。

## 跨 phase 依赖：已通过依赖序修正解决

- ~~`modulespecifiers`/`checker` → `tsoptions`（曾排 P6）的倒挂~~ **已解决**：`tsoptions` 连同 `outputpaths`/`sourcemap`/`tracing`/`printer` 一并前移到本 phase（P4），作为 checker 的构建前置，不再倒挂。
- `checker` 的两个"真 program"测试（`TestGetSymbolAtLocation`/`BenchmarkNewChecker`）依赖 `compiler`/`bundled`——其中对 `compiler` 的依赖**只在 `*_test.go`**，属 Rust **dev-dependency**（gate-docs.sh D6 已将其列为「仅测试边」，不约束 phase 序）。`bundled` 现位于 P1。建议 4b 用最小桩 program 跑核心路径，真 program 集成测试随 P6 compiler 落地后接入。

详见各包 `impl.md`/`tests.md` 的"转交 / 推迟（DEFER）"与"存疑/偏离"小节。
