# conformance-parity: 端到端对拍策略（impl.md）

> 本子目录**不是一个 crate**，而是 P10 的**端到端对拍方法论 + 分批引入计划**。它把前面三个 crate（`tsgo_testutil` 引擎、`tsgo_testrunner` 驱动器、`tsgo_fourslash` 框架）组装起来，定义"同一批 `testdata/tests/cases` fixture 跑 Rust 编译管线 → 与 Go 生成的 `testdata/baselines/reference` 逐字节 diff"的完整口径。

**目标**：证明 P1→P9 移植的整条管线在 `testdata` 全量上与 Go 输出**逐字节一致**。
**依赖**：`tsgo_testrunner`（执行）+ `tsgo_testutil`（对拍）+ 整条 P1-P9 管线（`tsgo_compiler` / `tsgo_checker` / `tsgo_printer` / `tsgo_transformers` / `tsgo_sourcemap` / …）。

## 这是什么（业务说明）

conformance/compiler 测试是 TypeScript **编译器正确性的金标准**：每个 `testdata/tests/cases/{compiler,conformance}/*.ts` 是一个编译输入，`testdata/baselines/reference/{compiler,conformance}/` 下是它应该产生的全部输出快照（baseline）。一个 fixture 跑完会产出最多 8 类 baseline：

| baseline 类型 | 文件后缀 | 由谁产 | 验证的包 |
|---|---|---|---|
| 诊断 | `.errors.txt` | `tsbaseline::do_error_baseline` | `checker` / `binder` / `parser`（诊断正确性） |
| JS emit | `.js` | `tsbaseline::do_js_emit_baseline` | `printer` / `transformers` |
| 声明 emit | `.d.ts`（并入 `.js` baseline 块） | 同上 | `transformers`（declaration）/ `nodebuilder` |
| sourcemap | `.js.map` | `do_sourcemap_baseline` | `sourcemap` / `printer` |
| sourcemap record | `.sourcemap.txt` | `do_sourcemap_record_baseline` | `sourcemap` 解码 |
| 类型 | `.types` | `do_type_and_symbol_baseline` | `checker`（类型推断） |
| 符号 | `.symbols` | 同上 | `checker` / `binder`（符号解析） |
| 模块解析 trace | （并入 trace baseline） | `do_module_resolution_baseline` | `module` / `modulespecifiers` |

`testdata/baselines/reference` 共 **292MB**，含 **12500 `.js` / 12395 `.types` / 12395 `.symbols` / 7333 `.txt`(含 errors) / 144 `.map` / 2557 `.diff`(submodule) / …**。这几万个文件就是 ground truth。

> **这是所有"0 直接单测"包的最终兜底**：`scanner` / `evaluator` / `nodebuilder` / `printer` / `transformers` / `checker`（仅 3 单测）的正确性，全靠这里逐字节对齐证明（PORTING §8.5）。

## 对拍口径（每类 baseline 的"一致"定义）

总原则：**reference 是 ground truth，Rust 输出向它看齐，逐字节比对**（`baseline::write_comparison`）。不允许"调 baseline 迁就 Rust"。

### 1. 诊断 baseline（`.errors.txt`）

- 口径：诊断**排序后**（`CompareASTDiagnostics`）逐文件穿插源码行 + `!!! error TSxxxx: <message>` 逐字节一致。
- 关键点：
  - 诊断**消息文本**来自 `tsgo_diagnostics`（P2 的 i18n 消息表），必须与 Go 完全一致（含参数插值、引号、换行 `\r\n`）。
  - 诊断**排序稳定**（PORTING §6 确定性）：并行收集后按 (file, pos, code) 稳定排序。
  - `DiffFixupOld` 去 `==== ./` → `==== ` 前缀（compiler_runner `verifyDiagnostics`）。
  - 错误计数自检：`error_baseline` 末尾统计非 lib/非 tsconfig 文件的错误总数，须与逐行计数吻合。

### 2. JS / d.ts emit baseline（`.js`）

- 口径：每个输出文件以 `//// [path]` 头分块拼接，逐字节一致。
- 关键点：
  - **emit 输出顺序确定**：corsa 并行 emit，须按 input 源文件顺序重排（`testutil::new_compilation_result`）。
  - 默认 `NewLine = CRLF`、`NoErrorTruncation = true`、`SkipDefaultLibCheck`（`CompileFiles` 默认）——必须与 Go 一致，否则全片漂移。
  - `.d.ts` 与 `.js` 并入同一 baseline 块（`do_js_emit_baseline` 内拼接）。

### 3. sourcemap（`.js.map`）+ sourcemap record（`.sourcemap.txt`）

- 口径：`.js.map` JSON 逐字节；record 是人类可读的 span 映射文本（`SourceMapSpanWriter`）。
- 关键点：VLQ 编码 + mapping 解码必须与 Go `sourcemap` 包一致；record 里源码片段引用按 input text 切片。

### 4. types / symbols baseline（`.types` / `.symbols`）

- 口径：`TypeWriterWalker` 遍历每个文件的每个标识符，输出 `>identifier : <type>` / `>identifier : Symbol(...)`，逐字节一致。
- 关键点（**最硬，checker 正确性核心**）：
  - 类型**显示字符串**（`number | string` 的成员顺序等）依赖 union ordering（`compiler_runner::verify_union_ordering` 已自检稳定性）。
  - symbol ID 顺序：full walker（全编译）vs pull walker 可能不同，Go 只用 full walker；Rust 同。
  - `hasErrorBaseline` 影响输出（有错时部分类型显示 error）。

### 5. module resolution trace

- 口径：`traceResolution: true` 的用例输出解析 trace，经 `TracerForBaselining` 净化（版本号 → `FakeTSVersion`、package.json 缓存提示归一）后逐字节一致。

## 分批引入计划（按 conformance 子目录 + 按 baseline 类型双维度）

逐字节对齐几万个 baseline 不可能一步到位。分两个维度切：

### 维度 A：按 baseline 类型（先易后难，跨所有 fixture）

| 批 | baseline 类型 | 先决条件（phase） | 说明 |
|---|---|---|---|
| T1 | `.errors.txt`（诊断） | P2 diagnostics + P3 parser/binder + P4 checker | 最先：很多 fixture 只有 errors baseline；诊断对齐是 checker 正确性的第一道关 |
| T2 | `.symbols` | P3 binder + P4 checker | 符号解析（比类型简单） |
| T3 | `.types` | P4 checker 全量 | 类型推断（最硬） |
| T4 | `.js` / `.d.ts` | P5 printer/transformers | emit |
| T5 | `.js.map` / `.sourcemap.txt` | P5 sourcemap | sourcemap |
| T6 | module resolution trace | P4 module/modulespecifiers | trace |

> 对单个 fixture，只要它产出的某类 baseline 还没对齐，就把对应 `verify_*` 子 case 标 skip；该类全绿后开启。

### 维度 B：按 conformance 子目录（缩小爆炸半径）

`testdata/tests/cases/` 结构（当前仓库）：
```
testdata/tests/cases/
├── compiler/      (227 fixture，TestTypeRegression)
└── conformance/   (7 顶层子目录，TestTypeConformance)
```
`testdata/baselines/reference/` 对应：`compiler/`(710 入口) / `conformance/`(65 子目录)。

| 批 | 子目录 | 说明 |
|---|---|---|
| D1 | `compiler/`（回归用例，单文件多） | 量大但单文件为主，先打基础 |
| D2 | `conformance/` 逐子目录 | 按语言特性子目录（如 `types/` `expressions/` `es2015/` …）逐个开 |
| D3 | submodule（`../_submodules/TypeScript/tests/cases`，`TestSubmodule`，diff 模式） | 最后；依赖 submodule checkout + 三档 diff（`submodule`/`submoduleAccepted`/`submoduleTriaged`） |

### 推进矩阵（T × D）

实操按 **(T1, D1) → (T1, D2) → (T2, D1) → …** 推进：先让 `compiler/` 的诊断全绿，再铺到 conformance 各子目录，再上 symbols/types/emit。每个格子是一个可勾选的里程碑（见 tests.md §2）。

## 实现 TODO（编排层，可勾选）

> conformance-parity 不写新模块代码——它消费 `tsgo_testrunner::TestLocal`。本节是"让 TestLocal 全量开绿"的编排步骤。

- [ ] **跑通单 fixture 单 baseline**（tracer bullet）：选 `compiler/` 一个无 vary-by、仅 errors 的 fixture，`TestLocal` 路径产 `.errors.txt` 与 reference 逐字节一致。
- [ ] **诊断对齐全量（T1）**：`compiler/` → `conformance/` 各子目录的 `.errors.txt` 全绿；漂移项归类（消息文本差异 / 排序差异 / 计数差异）逐个修。
- [ ] **symbols 对齐（T2）**：`.symbols` 全绿。
- [ ] **types 对齐（T3）**：`.types` 全绿（含 union ordering 稳定性，靠 `verify_union_ordering` 守门）。
- [ ] **emit 对齐（T4）**：`.js` / `.d.ts` 全绿（emit 顺序确定 + CRLF + skippedEmitTests 名单照搬）。
- [ ] **sourcemap 对齐（T5）**：`.js.map` / `.sourcemap.txt` 全绿。
- [ ] **module resolution（T6）**：trace baseline 全绿。
- [ ] **结构自检**：`verify_union_ordering` / `verify_parent_pointers` 在全量 fixture 上通过。　`// Go: internal/testrunner/compiler_runner.go:verifyUnionOrdering`
- [ ] **submodule diff（D3）**：`TestSubmodule` 三档 diff baseline 与 reference 一致（DEFER 到 local 全绿后）。
- [ ] **baseline tracking**：启用 `TSGO_BASELINE_TRACKING_DIR`，检测"未被任何测试用到的 reference 文件"（与 Go 一致，防 baseline 腐烂）。

## 与 Go 的已知偏离（divergence）

1. **确定性是硬约束**：Go 已用顺序重排 / 稳定排序处理并行非确定性；Rust 并行（rayon）后必须复刻所有重排点（emit 输出、诊断排序、checker 并行）。`skippedEmitTests`（8 个文件名冲突非确定性用例）名单**逐字照搬**。
2. **`*testing.T` → `&Harness`**：失败累积线程安全；`t.Run` 子 case（每类 baseline）保留命名。
3. **diff 库**：submodule diff（D3）的 `.diff` 由 `similar` 产，须与 `patience` 字节一致（见 testutil/impl.md）。
4. **baseline 不可变**：reference 是 ground truth。若 Rust 输出与 reference 不一致，**默认是 Rust 错**，修 Rust；仅当确认是 Go 上游 bug 才走 `baseline-accept` 流程（与 Go 团队一致）。
5. **`unparsed` / `skipped` 名单**：`skippedTests`（compiler_runner，~35 个已移除 option 的用例）逐字照搬，不试图让它们通过。

## 转交 / 推迟（DEFER）

- **submodule diff（D3）**：依赖 `../_submodules/TypeScript` checkout + testutil 三档 diff 完整；local（D1/D2）全绿前不开。`// DEFER(phase-10): blocked-by: local 全绿 + submodule checkout`
- **`captureSuggestions` / 增量编译（incremental）baseline**：依赖 P6 `incremental` 包；少量用例，靠后。
- **bench fixture**（`testutil/fixtures`）：不属对拍，独立。
