# Phase 5 — Emit / 代码生成（transformers + diagnosticwriter）

把 checker（P4）算好类型后的 AST，经 `transformers` 语义保持降级改写，交给 `printer`（已前移至 P4）打印成最终文本；并提供诊断的文本渲染（`diagnosticwriter`）。

> 方法论与共享契约见根 [PORTING.md](../PORTING.md)（必读）。本 README 只讲本 phase 的包、依赖序、纪律与决策。

> **⚠️ 依赖序修正（本轮）**：`printer`/`sourcemap`/`outputpaths` 已**前移到 P4**（它们是 checker 的构建前置：checker→printer，printer→sourcemap/tsoptions，modulespecifiers→outputpaths）。本 phase 现仅保留 **checker 之后**才能构建的 emit 部分：
> - **transformers**（依赖 `checker`/`printer`）——必须排在 checker 之后，留 P5。
> - **diagnosticwriter**（诊断 → 文本）——其唯一"较早"消费者 `tsoptions` 只在 `*_test.go` 引用（dev-dep），故无需前移，留 P5；真正消费它的是 `ls/lsconv`(P7) 与 `execute`(P9)。

## 本 phase 的包（依赖序）

```
diagnosticwriter（诊断渲染，叶子级）        transformers（emit 前置改写，依赖 checker/printer）
```

- **diagnosticwriter**（1 文件，0 单测）：诊断 → 文本（pretty/紧凑/汇总）。P10 兜底 + 本轮补行为级测试。
- **transformers**（40 文件 = 根 5 + 6 子包 35，2 测试 / **2 func**）：emit 前置的语义保持改写；依赖 `checker`/`printer`（均 P4）。

> **已前移到 P4 的包**：`outputpaths`、`sourcemap`、`printer`（详见 [phase-4-checker/README.md](../phase-4-checker/README.md)）。**nodebuilder** 一直在 P4。`transformers/declarations`（`.d.ts`）依赖 nodebuilder/modulespecifiers（P4），按 path 依赖跨 phase 引用即可。

## printer 是 emit 核心

`printer` 是本 phase 的中枢，也是整个移植里最重的包之一：

- **依赖 ast / checker / transformers**：
  - 依赖 `tsgo_ast`（arena + NodeId，遍历/打印每个节点）；
  - 依赖 checker 侧的 `EmitResolver`（trait 在本包定义，实现来自 P4）做名字解析、引用标记；
  - 与 `transformers` **共享同一个 `EmitContext`**——transformers 用 `printer.NodeFactory` 造节点、用 `EmitContext` 记录 original/emitFlags/注释/源映射范围，printer 据此打印出正确位置与注释。三者强耦合，是"AST→文本"的协作核心。
- **职责**：递归 emit（逐 NodeKind）+ **parenthesizer**（按运算符/类型优先级插括号）+ 缩进/换行（ListFormat）+ 注释发射 + source map 发射（调 `tsgo_sourcemap::Generator`）+ 名字生成（`NameGenerator`）+ JSX。
- **测试大户**：`printer_test.go`(65) + `namegenerator_test.go`(36) + `utilities_test.go`(4) = **105 func**，其中 `TestEmit` 一个函数就含 ~290 表驱动子用例（覆盖几乎全部 NodeKind）。tests.md 已逐 func + 逐子用例对齐。

## 单测覆盖统计（本轮文档对齐的 Go 测试规模）

| 包 | 实现文件 | 测试文件 | `func Test` 数 | 子用例（约） | tests.md 覆盖 |
|---|---|---|---|---|---|
| outputpaths | 2 | 0 | 0 | 0（P10 兜底 + 18 条行为级补测） | ✅ 全列 |
| diagnosticwriter | 1 | 0 | 0 | 0（P10 兜底 + 18 条行为级补测） | ✅ 全列 |
| **sourcemap** | 6 | 1 | **30** | 30（+ 8 条 decoder/mapper/util 补测） | ✅ 30 func 逐个 |
| **printer** | 15 | 3 | **105** | ~430（`TestEmit` 一个 ~290） | ✅ 105 func + 子用例逐条 |
| transformers | 40 | 2 | **2** | ~90（TypeEraser ~70 + ImportElision ~20，+ 12 条子包补测） | ✅ 2 func + 子用例逐条 |
| 合计 | 64 | 6 | **137** | ~550 | |

> printer 的 105 与 sourcemap 的 30 是本 phase 重点，已在各自 tests.md 逐 `func Test` + 逐表驱动子用例对齐，expected 全部取自 Go 字面量。

## 目录

```
phase-5-emit/
├── README.md            ← 本文件
├── diagnosticwriter/    {impl.md, tests.md}
└── transformers/        {impl.md, tests.md}   ← 根 + 6 子包（依赖 checker/printer）

# 已前移到 phase-4-checker/：outputpaths/、sourcemap/、printer/
```

> 下文「printer 是 emit 核心」「单测覆盖统计」「crate 布局决策」「VLQ 自实现」等小节中关于
> `printer`/`sourcemap`/`outputpaths` 的内容**仍然有效**，只是这些包已落在 **P4**（见 phase-4 目录）。
> 保留于此作为 emit 子系统的整体设计参考；`transformers`/`diagnosticwriter` 是本 phase 实际承载的两个包。

## crate 布局决策（本 phase 敲定）

| Go 包/子包 | crate | 备注 |
|---|---|---|
| `internal/outputpaths` | `tsgo_outputpaths` | |
| `internal/diagnosticwriter` | `tsgo_diagnosticwriter` | |
| `internal/sourcemap` | `tsgo_sourcemap` | VLQ **自实现**（见下） |
| `internal/printer` | `tsgo_printer` | emit 核心 |
| `internal/transformers` | `tsgo_transformers`（根） | + 下列 6 子 crate |
| `internal/transformers/estransforms` | `tsgo_estransforms` | 子包独立 crate |
| `internal/transformers/moduletransforms` | `tsgo_moduletransforms` | 子包独立 crate |
| `internal/transformers/tstransforms` | `tsgo_tstransforms` | 子包独立 crate（被测 2 stage） |
| `internal/transformers/jsxtransforms` | `tsgo_jsxtransforms` | 子包独立 crate |
| `internal/transformers/inliners` | `tsgo_inliners` | 子包独立 crate |
| `internal/transformers/declarations` | `tsgo_declarations` | 子包独立 crate（依赖 nodebuilder/modulespecifiers） |

> transformers 子包采用**独立 crate**（而非父 crate 子 module），因各子包有不同外部依赖且依赖深度不一，独立 crate 边界更干净、编译并行度更高。详见 `transformers/impl.md` 的"子包 crate 化决策"。

## 关键决策 / 存疑偏离（须转交）

1. **sourcemap VLQ：自实现（P5 敲定）**。`references/crate-map.md` 待定表里此项为"自实现 / `vlq` crate（决策 phase P5）"。结论自实现（Go 上游内联手写 ~50 行纯位运算，1:1 移植即可，不引入第三方）。
   - ⚠️ **转交项**：需把 `references/crate-map.md` 待定表 VLQ 行更新为"**自实现（P5 敲定）**"。本 phase 文档因边界限制（只允许写 `phase-5-emit/` 下 .md）未直接改该文件。
   - `Base64DataURL` 的标准 base64 倾向用 `base64` crate（成熟实现），同样应回填 crate-map「序列化/编码」。
2. **节点指针 → NodeId（PORTING §5）**：printer 的 `EmitContext` 全部旁路表 key、`NameGenerator` 的节点缓存、parenthesizer 节点比较，由 Go 指针/`==` 改为 `NodeId` 相等。`TestUniqueName2`/`TestGeneratedNameForNodeCached` 等"object identity 缓存"行为靠 `NodeId` 等价复原。
3. **EmitContext / NodeFactory 借用规划**：Go 里 factory 自引用 EmitContext 并写回其旁路表（裸指针图）。Rust 需让 factory 方法接 `&mut EmitContext`（或旁路表用 `RefCell`），避免别名冲突——结构保真的必要偏离，落地时在文件头细化。
4. **outputpaths 两套前缀判定并存**：`GetSourceFilePathInNewDir`（`ContainsPath`）vs `...Worker`（`HasPrefix`）是 Go 上游历史遗留，**必须各自 1:1 保留**，不可统一。
5. **ImportElision 测试依赖 P4 checker**：`TestImportElision` 需 `checker.NewChecker` + `EmitResolver.MarkLinkedReferencesRecursively`；checker 未就绪前该测试整体标 `—`（DEFER），不阻塞其余 transformers 工作。
6. **declarations `.d.ts` parity 推迟**：依赖 nodebuilder（P4）+ modulespecifiers + symbol tracker，完整正确性 `// DEFER(phase-10)` 由 conformance baseline 兜底；本 phase 仅承接 transform 框架。

## 实施纪律（每个包收口前）

1. 读 `impl.md` + `tests.md` + 对应 Go 源码 + `*_test.go`。
2. 先写 Rust 测试（red）→ 再写实现（green），逐文件、逐用例（printer/sourcemap 逐 func 对齐）。
3. 验证：`cargo test -p tsgo_<pkg>` 全绿 + `cargo clippy -p tsgo_<pkg>` 干净 + rustdoc 规范自检（PORTING §7）。
4. tests.md 与 Go 测试逐用例对齐审查（PORTING §8），impl.md 与 tests.md 互对齐。
5. 勾选文档，更新根 README 的 P5 进度。
